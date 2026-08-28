//! The monitor engine: one audio output stream that plays up to
//! [`SLOTS`] hosted VST3 instruments at once and a metronome, and taps the
//! instruments for the recorder.
//!
//! `ivory-host` could already load a plugin and render audio from it offline
//! (`examples/record_plugin.rs` writes a whole take that way). What it could not
//! do is make a sound, because Tangent had no audio OUTPUT anywhere — the
//! recorder's `audio.rs` opens an *input* stream and nothing else. A pianist has
//! to hear the piano while they play it, so this file exists.
//!
//! # There is exactly ONE output stream, and it is not optional
//!
//! Two cpal output streams on one device fight over it: on CoreAudio the second
//! one re-negotiates the device's buffer size out from under the first, and on
//! WASAPI shared mode they arrive at the mixer as two clients whose callbacks
//! are not phase-locked to each other. So the instruments and the click are
//! summed **in the same callback**, which is also the only way the click can be
//! sample-accurate against what they are playing — and the only way three
//! instruments can be sample-accurate against each other.
//!
//! And the stream's existence does not depend on a plugin. [`Engine::start`]
//! opens the device with nothing loaded; the metronome works immediately, and a
//! plugin is swapped in later while the stream keeps running. A metronome that
//! only works once you have found and loaded a VST3 is not a metronome.
//!
//! # Three instruments, one keyboard
//!
//! A pianist wants a pad under the piano and a bell on top, so there are
//! [`SLOTS`] of them and they all play **the same notes at the same time**,
//! each with its own gain. The count comes from `ivory_ui::recorder::SLOTS` and
//! is not redefined here: the band draws exactly as many faders as this file
//! renders, and a private constant is how those two quietly disagree.
//!
//! The three properties that make layering work rather than merely compile:
//!
//! 1. **One queue, drained once.** [`Renderer::collect_notes`] fills
//!    `notes`/`controls` once per block and every slot renders *that* list. A
//!    per-slot drain would give each event to whichever slot popped first and
//!    the other two would hear silence — the MIDI ring is a queue, not a
//!    broadcast.
//! 2. **The sum is a stereo bus.** Each slot's own width is resolved where it
//!    is still known (a mono instrument goes to both sides, an eight-out
//!    instrument contributes its first two — see [`stereo_of`]), and the sum of
//!    the three is what [`map_frame`] then maps onto the device. Once three
//!    instruments are added together there is no single source width left to
//!    ask about, so the question has to be settled per slot, before the add.
//! 3. **Each slot swaps on its own.** Loading a pad into slot 2 must not
//!    interrupt the piano in slot 1 by so much as a block, which is why the
//!    handoff below is per slot rather than one shared channel.
//!
//! # The two mixes, which are not the same sum
//!
//! ```text
//!                   slot gain
//!   instrument 1 ──►(×)─┐
//!   instrument 2 ──►(×)─┼─► stereo sum ─┬──────────────► device mix ──► speakers
//!   instrument 3 ──►(×)─┘               │                 (+ click, always)
//!                                       │
//!                                       └──────────────► recorder tap ──► the take
//!   click ──►(×)───────────────────────────────────────►  (+ click ONLY if
//!         metronome gain                                    metronome_in_take)
//! ```
//!
//! **The click reaches the speakers and NOT the take**, and that default is
//! deliberate: a click bleeding into the recording is a ruined take, and it is
//! ruined in a way you only discover on playback. [`Engine::set_metronome_in_take`]
//! turns it on for people who want a guide track. The consequence is that the
//! two sums are built separately, frame by frame, rather than the tap being a
//! branch off the finished monitor bus — tapping the final bus is how the click
//! gets into the take by accident six months from now.
//!
//! # Threading: the instruments render IN the audio callback
//!
//! [`ivory_host::Instance`] holds `ComPtr`s and is `!Send`; cpal wants a
//! `Send + 'static` callback. Two shapes were available and this file takes the
//! first:
//!
//! (a) move the instance into the callback behind a newtype with an
//!     `unsafe impl Send`, so the plugin renders directly under the device's
//!     deadline — which is what every real host does; or
//! (b) render on a dedicated thread into a ring the callback drains, which
//!     avoids the `unsafe` and costs **one whole buffer of latency**.
//!
//! Measured on the owner's machine (Scarlett 18i20, CoreAudio): the device
//! granted the 256-frame buffer that [`Engine::start`] asks for, and the backend
//! reports **5.33 ms** between the callback and playback, which is exactly one
//! buffer. Shape (b) would double that before the interface's own converter
//! delay is counted at all. A pianist feels the difference. See [`PluginBox`]
//! for the safety argument, which is about *when* things move rather than about
//! the move itself.
//!
//! Latency in this file is reported and never assumed:
//! [`Engine::output_delay_ns`] is the backend's own number, and RECORDER-PLAN
//! §3a is explicit about why a claimed latency is worse than an absent one.
//!
//! # The editor, and how it reaches a plugin the audio thread owns
//!
//! Shape (a) has a consequence nobody had to pay until the plugin needed a UI:
//! **there is no `&Instance` on this thread**. `IEditController::createView` is
//! a main-thread call on an object that left for the audio callback five
//! seconds ago.
//!
//! The answer is that the controller is a *separate, reference-counted COM
//! object*, so [`Engine::load_plugin`] takes a second reference to it — an
//! [`ivory_host::EditorHandle`] — in the last moment before `inst` is moved
//! into a [`PluginBox`], and keeps it here. Opening an editor later touches
//! that handle and never the instance. Nothing stops, nothing is retired, and
//! not one block of audio is missed by opening or closing a window.
//!
//! It also does not weaken [`PluginBox`]'s safety argument, because condition 3
//! is unchanged: the audio thread calls `process` and nothing else, and every
//! call this file makes on the controller is on this thread — which is the
//! split VST3 specifies in the first place.
//!
//! There is one handle and one window **per slot**, and they are indexed by the
//! same number everywhere: `open_editor(1)` opens the window belonging to the
//! instrument `load_plugin(1, …)` loaded. Three windows can be on screen at
//! once, which is the point — balancing a layer means hearing it against the
//! other two while you change it.
//!
//! The lifetime rule that comes with it is one line and it is enforced in
//! [`Engine::unload_plugin`]: **that slot's editor closes before that slot's
//! instrument is released**, because the window holds an `IPlugView` belonging
//! to a controller that `Instance::drop` is about to terminate.
//!
//! # What the callback may not do, and how each rule is kept
//!
//! **Never allocate.** Every buffer here is sized once, in [`Engine::start`] or
//! in [`Engine::load_plugin`]: the stereo sum bus, the device mix scratch, the
//! tap scratch, the note list and every ring. Pushes into `Vec`s are guarded by
//! their capacity rather than trusted. The only per-load allocation is a slot's
//! own channel buffers, whose width is the plugin's and therefore unknowable at
//! `start` — and it happens on the UI thread, in the same call that spends five
//! seconds warming the instrument up.
//!
//! There used to be **one exception, and it was not this file's to remove**:
//! `Instance::process` allocated internally on every call — a `Vec` of channel
//! pointers, an `AudioBusBuffers` vector, one scratch buffer per channel of
//! every output bus past the first, and a `ComWrapper`ed event list. Measured
//! on the modelled piano's eight-stereo-bus layout by putting the old code under a
//! counting allocator: **42 allocator calls for one block**, 21 allocations and
//! 21 frees, and **three loaded slots was three times that** — 126 a block,
//! about 12,000 a second. It is the one place layering made an existing
//! landmine bigger rather than adding a new one. They were identical in size
//! every time, so a general-purpose allocator served them from a free list and
//! it was never heard here, which is exactly why it survived so long.
//!
//! **It is fixed, in `ivory-host`'s `instance.rs`, where it always had to be**:
//! the scratch now lives on the `Instance`, sized once in `Instance::create`
//! from `Setup::max_block` and the bus layout. Nothing changed on this side —
//! the call is still `process_with_controls` — but the pre-grown `bufs` below
//! are now load-bearing rather than merely tidy: `process` resizes them to the
//! block length, and a `Vec` that has never held a block would grow **here**,
//! on the audio thread, which is the one allocation the host cannot make go
//! away on the caller's behalf. Measured: a hundred blocks of the modelled piano through
//! this path now touch Rust's allocator zero times.
//!
//! **Never lock.** `rtrb` SPSC rings — one for MIDI, one for the tap, and a
//! pair per slot for the handoff — and a handful of relaxed atomics. The only
//! `Mutex` in the file is written by cpal's *error* callback, which is a
//! different callback, with `try_lock` — exactly as `ivory-record`'s `audio.rs`
//! does it.
//!
//! **Never panic.** A panic across the cpal FFI boundary is undefined behaviour,
//! not a stack trace. Nothing in the render path indexes, unwraps, or divides by
//! a value it has not bounded first.
//!
//! # Where a note actually lands, stated plainly
//!
//! Events carry a host-timebase stamp and [`place`] converts it to a frame
//! offset inside the block. That machinery is real and tested — a stamp before
//! the block goes to offset 0, one after it is held for the next block, one
//! inside lands on its frame.
//!
//! **For live playing every event lands at offset 0, and no amount of
//! arithmetic changes that.** A block rendered at time T can only carry events
//! that were already known at T, so every one of them is by definition in the
//! past, and sub-block placement can only ever *delay* an event. Placement earns
//! its keep for events that are scheduled ahead of the clock, which is what
//! [`plugin_test`] plays and what a future MIDI-file player would.
//!
//! The real live latency is therefore not the block: it is **however often
//! [`Engine::send_midi`] is called**. Today the Recorder band drains the MIDI tap
//! once per UI frame, so a note waits up to ~16 ms before it is even queued. The
//! fix is not here — it is to call `send_midi` from `midi.rs`'s own `midir`
//! callback, which is why it takes `&self` and pushes into a lock-free ring
//! instead of taking `&mut self`. Until then the audio path's own 8 ms is the
//! smaller half of what the player feels.
//!
//! # The sustain pedal, and what it cost
//!
//! It works — and this section used to say it did not, which is why it is still
//! here. VST3 has no "send a CC": a control change reaches an instrument only as
//! a parameter change found through `IMidiMapping` on the EDIT CONTROLLER, a
//! second object the host must create and connect both ways. `ivory-host` now
//! does all of that, and `Instance::process_with_controls` is the door.
//!
//! **It sends the parameter change AND the legacy MIDI CC event, on every
//! control, always.** Measured on the modelled piano, which publishes a mapping for
//! CC64, accepts the parameter change, returns `kResultOk` and ignores it: a
//! held C4 released with the pedal down rang at 0.001452 RMS with the parameter
//! change alone — identical to six decimal places to the same phrase with no
//! pedal at all. Adding the legacy event took that to 0.012151, which is 8.4x.
//! A CC is a value rather than a delta, so a plugin honouring both sets the same
//! parameter twice and nothing is harmed.
//!
//! [`Engine::pedal_dropped`] therefore counts what an instrument published no
//! mapping for — "this plugin has no pedal" — rather than "we never tried". It
//! reports the MINIMUM across rendering slots, not the sum: one pedal press
//! refused by three plugins is one refusal, and summing would make the counter a
//! function of how many slots happen to be full.

// This module is a public API with no caller yet: the Recorder band that will
// drive it is being written alongside it, and `main.rs` does not wire
// `plugin_test` to a CLI flag (its own docs carry the four lines that do). A
// binary crate reports every unreached `pub` item as dead, so without this the
// build is fifty warnings deep and a real one could hide among them. **Delete
// this line the moment the band lands**: after that, anything it reports is a
// genuinely orphaned control.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ivory_host::{Control, Instance, Module, Note, Setup};
use ivory_record::audio::{Timebase, CLIP_LEVEL, DEFAULT_RING_SECONDS};
use ivory_record::clock::Nanos;
// The number of instrument slots is the GUI's, not this file's. Defining a
// second one here is how the band ends up drawing three faders for four
// renderers, or four for three, and neither side would fail to compile.
use ivory_ui::recorder::SLOTS;
use rtrb::{Consumer, Producer, RingBuffer};

// ───────────────────────────────────────────────────────────────────────────
// Constants
// ───────────────────────────────────────────────────────────────────────────

/// Frames per `process` call, fixed for the life of an instance.
///
/// 512 because that is the number every measurement in RECORDER-PLAN §8 was
/// taken at, and because the plugin sizes its internal buffers from it at
/// `setupProcessing` — asking for more later is an error, not a slow path. A
/// device callback larger than this is split into several `process` calls; one
/// smaller renders a short block. Both are covered below.
const MAX_BLOCK: i32 = 512;

/// Output buffer asked of the device, in frames. 256 is 5.3 ms at 48 kHz, which
/// every CoreAudio and WASAPI device on this machine accepts, and it is the
/// dominant term in what a player feels from the audio path. Overridable with
/// `IVORY_OUT_BUFFER` for a machine that crackles at it.
const WANT_BUFFER_FRAMES: u32 = 256;

/// The largest device callback the mix scratch is sized for. Anything larger is
/// rendered in several passes rather than reallocating on the audio thread.
const MAX_CALLBACK_FRAMES: usize = 8192;

/// Note events carried into one `process` call. A burst larger than this is
/// spread over the following blocks; nothing is dropped, because a dropped
/// note-off is a note held until the app quits.
const MAX_EVENTS_PER_BLOCK: usize = 128;

/// Control changes buffered per block.
///
/// Far smaller than the note budget and deliberately so: there are three
/// pedals, and a continuous half-pedal sweep from a good keyboard is a few
/// messages per block at most. Overflow is counted, not dropped silently.
const MAX_CONTROLS_PER_BLOCK: usize = 32;

/// The recorder tap is **always stereo**, whatever the instrument renders.
///
/// It is fixed at [`Engine::start`] and never changes, which is the property
/// [`RecorderTap`] needs: the writer thread holds the tap across a plugin swap,
/// and a take whose channel count changes halfway through is not a file any tool
/// will open. A mono instrument is written to both channels; a multi-output one
/// contributes its first two. See [`map_frame`].
///
/// **And so is the take**, because the take IS the tap — see
/// `record::TakeSource`. The file used to be as wide as the INPUT, so choosing
/// a mono microphone folded a stereo piano down to mono. That was defensible
/// while a take could be the dry microphone alone; it is not defensible now
/// that a take is the desk.
pub const TAP_CHANNELS: usize = 2;

/// Gain smoothing time constant. 10 ms is short enough to feel instant on a
/// fader and long enough that a jump from 0.0 to 1.0 is a fade rather than a
/// step discontinuity, which is a click in exactly the way the metronome is
/// supposed to have a monopoly on.
const GAIN_TAU_SECONDS: f64 = 0.010;

/// Release velocity for a note-off that carries none of its own.
///
/// 64/127, which is MIDI's own "no opinion" value. Not 0: several pianos map
/// release velocity to damper noise, and 0 there is a different sound rather
/// than a neutral one.
const DEFAULT_RELEASE_VELOCITY: f32 = 64.0 / 127.0;

/// How much faster the accented click is played back. `2^(4/12)` is a major
/// third up.
///
/// **Pitch and not level**, chosen deliberately: an accent made by playing the
/// same sample louder disappears the moment the user pulls the metronome gain
/// down or listens on anything with compression, whereas a pitch difference
/// survives both. It costs one extra resampled copy at startup and nothing at
/// all per frame.
const ACCENT_RATE_RATIO: f64 = 1.259_921_05;

/// Tempo bounds. Not taste: the beat clock counts down a frame at a time, so a
/// period below one frame would fire forever inside one callback.
const MIN_BPM: f64 = 20.0;
const MAX_BPM: f64 = 300.0;

/// How long [`Engine::load_plugin`] waits for the audio thread to hand the old
/// instance back before giving up and letting the stream's own teardown drop it.
/// Twenty buffers at the slowest plausible size; if the callback has not run in
/// that time it is not running at all.
const RETIRE_TIMEOUT: Duration = Duration::from_millis(250);

/// Peak-hold fall rate, matching `ivory-record`'s meter so the two read the same.
const HOLD_DECAY_DB_PER_SEC: f32 = 20.0;

/// How many pads the drumpad has. The UI's number, mirrored the way
/// [`CHANNELS`] is.
pub const PADS: usize = ivory_ui::recorder::DRUM_PADS;

/// Every rack the engine owns: the strips, the master, and one per PAD —
/// the pads' bays ride the same index space as everything else, seats
/// `STRIPS + 1 ..`, so loading, editors and state sidecars need no second
/// machinery. See `pad_rack_index`.
pub const RACK_SEATS: usize = ivory_ui::recorder::MASTER_EXTRA_SEAT + 1;

/// Where pad `i`'s bays live in the insert space.
pub const fn pad_rack_index(pad: usize) -> usize {
    STRIPS + 1 + pad
}

/// Pad 0's MIDI note; pad `n` answers `PAD_BASE + n`. See the UI's
/// `DRUM_PAD_BASE` — one constant, declared there, read here.
const PAD_BASE: u8 = ivory_ui::recorder::DRUM_PAD_BASE;

/// The channel nibble the pads own: 9, which the wire calls channel 10 — the
/// General MIDI drum channel. `collect_notes` diverts channel-10 notes to the
/// pads while a kit is loaded, so a drum controller that transmits on the
/// drum channel plays the drums, which is the least surprising thing a note
/// on channel 10 can do.
const PAD_CHANNEL: u8 = 9;

/// Frames a stolen pad voice takes to fade out. A retrigger or a choke that
/// truncated a waveform mid-sample would be a click — the exact click the
/// gain slew exists to prevent — so the cut voice rides this short ramp to
/// zero while its replacement starts clean. ~5 ms at 48 kHz.
const PAD_STEAL_FRAMES: f32 = 256.0;

/// Pad strikes carried into one chunk. Sixteen pads struck twice inside one
/// 512-frame block is flams on every pad at once; more than this is a MIDI
/// file being dumped, and the surplus lands at the top of the next chunk.
const MAX_PAD_HITS_PER_BLOCK: usize = 32;

// ───────────────────────────────────────────────────────────────────────────
// What the caller sees
// ───────────────────────────────────────────────────────────────────────────

/// The instrument currently loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub bundle: PathBuf,
    pub class: String,
    pub vendor: String,
    /// Channels on the plugin's **main** output bus, which is the only bus this
    /// engine reads. Buses past it are rendered into scratch and discarded —
    /// they have to be rendered, see `Instance::process`.
    pub channels: u16,
    /// The rate the plugin was set up for, which is always the device's. Nothing
    /// is resampled.
    pub sample_rate: u32,
}

/// What the warm-up concluded, so the band can be honest about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WarmUp {
    /// `false` means the instrument never made a sound and was declared ready by
    /// timeout. The take may be silent and the UI should say so.
    pub heard: bool,
    pub elapsed: Duration,
    /// Loudest peak seen while warming up. A value just under the readiness
    /// threshold reads very differently from an exact zero.
    pub peak: f32,
}

/// The open output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    pub device: String,
    pub channels: u16,
    pub sample_rate: u32,
    /// `None` when the device would not accept a fixed size and chose its own.
    pub buffer_frames: Option<u32>,
}

impl OutputInfo {
    /// One line for the band. Round-trip latency is not knowable from cpal 0.16
    /// (it reads neither `kAudioDevicePropertyLatency` nor the safety offset),
    /// so this reports the buffer and says nothing it cannot support.
    pub fn latency_line(&self) -> String {
        match self.buffer_frames {
            Some(f) if self.sample_rate > 0 => format!(
                "{} frames, {:.1} ms buffer at {} Hz{}",
                f,
                f as f32 * 1_000.0 / self.sample_rate as f32,
                self.sample_rate,
                // **The ring, where it is not the same as the period.** On
                // Linux the two differ by four (see `BUFFER_PERIODS`), so a
                // line that named only one of them would be answering a
                // different question than the one somebody debugging dropouts
                // is asking. Everywhere else they are the same number and
                // saying it twice would be noise.
                if BUFFER_PERIODS > 1 {
                    format!("  ({} frame ring)", f * BUFFER_PERIODS)
                } else {
                    String::new()
                }
            ),
            _ => format!("{} Hz, device buffer size", self.sample_rate),
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// MIDI, crossing to the audio thread
// ───────────────────────────────────────────────────────────────────────────

/// One MIDI message on its way to the audio thread.
///
/// Fixed size and `Copy` on purpose. A `Vec<u8>` here would mean an allocation
/// on the sender and a deallocation **in the callback**, which is the rule this
/// whole file is arranged around. Three bytes covers every channel message; a
/// SysEx dump is not something an instrument monitor needs to forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MidiEvent {
    pub(crate) stamp: Nanos,
    pub(crate) status: u8,
    pub(crate) data1: u8,
    pub(crate) data2: u8,
}

/// Raw MIDI bytes to a VST3 note, or `None` for anything that is not one.
///
/// Three traps, all of them cheap to get wrong and expensive to hear:
///
/// 1. **A note-on with velocity 0 IS a note-off.** Roughly half of all
///    keyboards release notes that way because it lets them stay in running
///    status, and a host that treats it as a note-on at silent velocity leaves
///    every note of the performance held down forever.
/// 2. **VST3 velocity is a float 0.0..=1.0, not a MIDI byte** (`instance.rs`
///    says so where `Note::velocity` is declared). Passing 100 makes every note
///    fortissimo and clipped, and it sounds like a broken plugin.
/// 3. **A byte below 0x80 is not a status byte.** midir delivers whole messages
///    and resolves running status itself, but a truncated or malformed buffer
///    must fall out here rather than be masked into a plausible note.
fn note_from_midi(status: u8, data1: u8, data2: u8) -> Option<Note> {
    if status < 0x80 {
        return None;
    }
    let pitch = i16::from(data1 & 0x7F);
    let velocity = data2 & 0x7F;
    match status & 0xF0 {
        0x90 if velocity != 0 => Some(Note {
            offset: 0,
            pitch,
            velocity: f32::from(velocity) / 127.0,
            on: true,
        }),
        // Trap 1.
        0x90 => Some(Note {
            offset: 0,
            pitch,
            velocity: DEFAULT_RELEASE_VELOCITY,
            on: false,
        }),
        0x80 => Some(Note {
            offset: 0,
            pitch,
            velocity: f32::from(velocity) / 127.0,
            on: false,
        }),
        _ => None,
    }
}

/// Sustain, sostenuto or soft: the three pedals, which cannot be delivered.
///
/// See the module docs. They are recognised only so they can be counted, and
/// they are counted so the gap is visible in the UI instead of being discovered
/// by a pianist in the middle of a phrase.
fn is_pedal(status: u8, data1: u8) -> bool {
    status >= 0x80 && status & 0xF0 == 0xB0 && matches!(data1, 64 | 66 | 67)
}

/// A channel of the mixer, and the bit it owns in `muted` / `soloed`.
///
/// **The master is not here on purpose.** Muting the master is the same as
/// turning it down and soloing it means nothing, so a strip that cannot do
/// either does not get a bit that would have to be defended against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strip {
    /// One general channel, MIDI or AUDIO by the desk's choice. See the UI's
    /// own `Strip` — the kind is not in the enum there either, and for the
    /// same reason: cycling a channel must not renumber it. `Track` sat here
    /// until v13 retired the BACKING strip; the clip plays under the master
    /// alone now.
    Channel(usize),
    /// The drum bus: the pads' own strip, left of the click.
    Drums,
    Click,
    Fx,
}

/// How many general channels the desk has. Mirrors the UI's constant; the
/// `From` impl below is what keeps the two enums provably the same shape.
pub const CHANNELS: usize = ivory_ui::recorder::CHANNELS;

/// How many inputs of one interface the desk has room for.
///
/// **Asserted equal to `ivory_record::audio::MAX_PICKS`** — the capture cannot
/// keep more than that, and a desk with more strips than the capture has picks
/// would draw channels nothing can ever fill. `ivory-ui` declares its own copy
/// because it may not reach either of these; the test below is what keeps all
/// three the same number.
pub const INPUTS: usize = ivory_ui::recorder::INPUTS;

/// How many channels the desk has, master aside.
pub const STRIPS: usize = CHANNELS + 3;

impl Strip {
    /// Its place in the arrays, and the bit it owns.
    pub const fn index(self) -> usize {
        match self {
            Strip::Channel(i) => i,
            Strip::Drums => CHANNELS,
            Strip::Click => CHANNELS + 1,
            Strip::Fx => CHANNELS + 2,
        }
    }

    fn bit(self) -> u32 {
        1 << self.index()
    }
}

impl From<ivory_ui::recorder::Strip> for Strip {
    /// **Exhaustive on purpose.** The UI declares its own `Strip` because it
    /// may not reach across the firewall for this one; adding a channel there
    /// and forgetting it here would silently route the new strip to whatever
    /// index happened to line up. This way it does not compile.
    fn from(ui: ivory_ui::recorder::Strip) -> Self {
        use ivory_ui::recorder::Strip as Ui;
        match ui {
            Ui::Channel(i) => Strip::Channel(i),
            Ui::Drums => Strip::Drums,
            Ui::Click => Strip::Click,
            Ui::Fx => Strip::Fx,
        }
    }
}

/// Whether a strip is heard, given the mute and solo masks.
///
/// **Solo is exclusive and mute loses to it.** With anything soloed, only the
/// soloed strips are heard — including a soloed strip that is also muted,
/// because pressing solo on a muted channel is unambiguously a request to hear
/// it and the alternative is a solo button that sometimes does nothing.
fn strip_is_heard(strip: Strip, muted: u32, soloed: u32) -> bool {
    if soloed != 0 {
        return soloed & strip.bit() != 0;
    }
    muted & strip.bit() == 0
}

/// Add what came back from the effects bus, at the bus's own fader.
fn add_return(
    mix: &mut [f32],
    aux: &[f32],
    frames: usize,
    gain: &mut f32,
    target: f32,
    coeff: f32,
) -> [f32; 2] {
    let mut peak = [0.0f32; 2];
    for f in 0..frames {
        *gain += (target - *gain) * coeff;
        let at = f * TAP_CHANNELS;
        for c in 0..TAP_CHANNELS {
            let (Some(v), Some(a)) = (mix.get_mut(at + c), aux.get(at + c)) else {
                return peak;
            };
            let wet = *a * *gain;
            peak[c.min(1)] = peak[c.min(1)].max(wet.abs());
            *v += wet;
        }
    }
    peak
}

impl Shared {
    /// Keep the loudest sample a strip has made since the UI last looked.
    ///
    /// `fetch_max` on the BITS, which is only correct for non-negative floats
    /// — and these are magnitudes, so they are. The same trick the limiter's
    /// gain reduction uses two fields up.
    fn note_strip_peak(&self, strip: Strip, peak: [f32; 2]) {
        let at = &self.strip_peak[strip.index()];
        for (slot, v) in at.iter().zip(peak) {
            if v > 0.0 {
                slot.fetch_max(v.to_bits(), Ordering::Relaxed);
            }
        }
    }
}

/// Where an event goes inside a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// This frame offset within the block.
    At(i32),
    /// After this block ends. Hold it; do not drop it.
    Later,
}

/// Place a host-timebase stamp inside a block that starts at `block_start`.
///
/// An event older than the block goes to offset 0 rather than being dropped:
/// the audio for those frames has not been rendered yet, so offset 0 is the
/// earliest it can possibly sound, and a late note is a note, whereas a dropped
/// note-off is a stuck key.
fn place(stamp: Nanos, block_start: Nanos, rate: f64, frames: usize) -> Placement {
    let delta = stamp.saturating_sub(block_start);
    if delta <= 0 {
        return Placement::At(0);
    }
    if !(rate.is_finite() && rate > 0.0) {
        return Placement::At(0);
    }
    let offset = (delta as f64) * rate / 1e9;
    if offset >= frames as f64 {
        return Placement::Later;
    }
    // `as i32` on a value already bounded by `frames`, which is bounded by
    // MAX_CALLBACK_FRAMES. No saturation to worry about.
    Placement::At(offset as i32)
}

// ───────────────────────────────────────────────────────────────────────────
// Channel mapping
// ───────────────────────────────────────────────────────────────────────────

/// Map one frame of the stereo sum onto one frame of some destination.
///
/// The sum is stereo and the device decides its own width, and they are
/// routinely different. The rules, all of them deliberate:
///
/// * **No source at all** (nothing loaded) writes silence, not stale samples.
/// * **Mono goes to every destination channel.** A mono source that only
///   appeared in the left speaker would be reported as a broken plugin. The
///   render path resolves mono at the slot instead (see [`stereo_of`]), so this
///   arm now serves callers that hand it a one-element source directly.
/// * **Two or more sources fill the first two destination channels** and leave
///   the rest silent. Taking the first two of a multi-output instrument is the
///   right answer because bus 0 is the main mix by VST3 convention — the other
///   seven of the modelled piano's eight are stem outputs of the same performance, and
///   summing them would be the same piano eight times.
/// * **A mono destination gets the average, not the sum.** Summing is +6 dB and
///   clips a mix that was correct in stereo.
fn map_frame(src: &[f32], dst: &mut [f32]) {
    match (src.len(), dst.len()) {
        (0, _) => dst.fill(0.0),
        (1, _) => dst.fill(src[0]),
        (_, 0) => {}
        (_, 1) => dst[0] = (src[0] + src[1]) * 0.5,
        _ => {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2..].fill(0.0);
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The metronome
// ───────────────────────────────────────────────────────────────────────────

/// The click, decoded and resampled to the device rate once, at startup.
///
/// Immutable after construction and shared with the callback through an `Arc`,
/// so the audio thread only ever dereferences it.
struct Click {
    normal: Vec<f32>,
    accent: Vec<f32>,
}

impl Click {
    /// Decode `assets/click.wav` and prepare both voices at `rate`.
    ///
    /// The asset is a mono 48 kHz 24-bit file with a `JUNK` chunk in front of
    /// `fmt ` — see `ivory_record::wav::read_pcm`, which exists because of it.
    fn load(rate: f64) -> Result<Self, String> {
        let bytes = include_bytes!("../../assets/click.wav");
        let (spec, samples) = ivory_record::wav::read_pcm(bytes)?;
        if spec.channels != 1 {
            // Only mono is folded into every output channel below; a stereo
            // asset would be read as interleaved garbage at double speed.
            return Err(format!(
                "the click asset is {}-channel and the metronome needs mono",
                spec.channels
            ));
        }
        let src = f64::from(spec.sample_rate);
        Ok(Self {
            normal: resample_linear(&samples, src, rate),
            accent: resample_linear(&samples, src * ACCENT_RATE_RATIO, rate),
        })
    }
}

/// Linear interpolation, which is the right tool exactly once.
///
/// A click is a broadband transient about half a second long, played at most a
/// few times a second, and its whole job is to mark an instant. Linear
/// interpolation's error is high-frequency and the material is already
/// high-frequency noise; a polyphase resampler here would be a dependency and a
/// week for a difference nobody can hear. Anything that has to survive being
/// *recorded* would deserve better, and the click is the one signal that
/// deliberately does not reach the take.
fn resample_linear(src: &[f32], from: f64, to: f64) -> Vec<f32> {
    if src.is_empty() || !(from.is_finite() && from > 0.0) || !(to.is_finite() && to > 0.0) {
        return Vec::new();
    }
    if (from - to).abs() < f64::EPSILON {
        return src.to_vec();
    }
    let step = from / to;
    let out_len = ((src.len() as f64) / step).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * step;
        let a = pos as usize;
        let frac = (pos - a as f64) as f32;
        let s0 = src[a.min(src.len() - 1)];
        let s1 = src[(a + 1).min(src.len() - 1)];
        out.push(s0 + (s1 - s0) * frac);
    }
    out
}

/// One click, playing.
#[derive(Debug, Default)]
struct Voice {
    pos: usize,
    accent: bool,
    playing: bool,
}

impl Voice {
    /// Retrigger from the top. A beat that arrives while the previous click is
    /// still ringing cuts it off rather than layering, which is what a
    /// hardware metronome does and what keeps 300 bpm from turning into mush.
    fn trigger(&mut self, accent: bool) {
        self.pos = 0;
        self.accent = accent;
        self.playing = true;
    }

    fn next(&mut self, click: &Click) -> f32 {
        if !self.playing {
            return 0.0;
        }
        let buf = if self.accent {
            &click.accent
        } else {
            &click.normal
        };
        match buf.get(self.pos) {
            Some(s) => {
                self.pos += 1;
                *s
            }
            None => {
                self.playing = false;
                0.0
            }
        }
    }
}

/// What a beat is, when one fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Beat {
    accent: bool,
    /// 1-based count-in beat, or 0 for a free-running metronome beat.
    count_in: u32,
    /// This beat is the downbeat that ends a count-in: recording starts here.
    downbeat: bool,
}

/// The beat clock, counted in **frames on the audio thread**, never in
/// `Instant`.
///
/// A count-in timed off the UI clock drifts away from the click the player is
/// actually hearing, and "the countdown said 4 but the click had already gone"
/// is worse than no countdown at all. Here the beat and the click are the same
/// event by construction, because the click is triggered by this.
///
/// `to_next` is fractional and accumulates with `+=` rather than being reset, so
/// a period of 22050.7 frames does not round down 22050 every beat and lose a
/// beat every half hour.
#[derive(Debug)]
struct Beats {
    period: f64,
    to_next: f64,
    /// Beats since the phase was last restarted, for the bar accent.
    index: u64,
    count_in_left: u32,
    count_in_total: u32,
    /// The last count-in beat has sounded; the next beat is the downbeat.
    awaiting_downbeat: bool,
    /// The `count_in_req` generation this clock has acted on.
    seen_req: u64,
    /// Whether the free-running metronome was on last frame, so switching it on
    /// restarts the phase at a downbeat instead of wherever the count happened
    /// to be.
    was_on: bool,
}

impl Beats {
    fn new(rate: f64, bpm: f64) -> Self {
        Self {
            period: period_frames(rate, bpm, 4),
            to_next: 0.0,
            index: 0,
            count_in_left: 0,
            count_in_total: 0,
            awaiting_downbeat: false,
            seen_req: 0,
            was_on: false,
        }
    }

    fn counting_in(&self) -> bool {
        self.count_in_left > 0 || self.awaiting_downbeat
    }

    /// Restart the phase so the next frame is beat 1.
    fn restart(&mut self, count_in: u32) {
        self.to_next = 0.0;
        self.index = 0;
        self.count_in_left = count_in;
        self.count_in_total = count_in;
        self.awaiting_downbeat = false;
    }

    /// Advance one frame. `Some` on the frame a beat lands.
    ///
    /// `on` is the free-running metronome switch; a count-in sounds whether it
    /// is on or not, because a count-in nobody can hear is not one.
    fn tick(&mut self, on: bool, beats_per_bar: u32) -> Option<Beat> {
        if !self.counting_in() {
            if on != self.was_on {
                self.was_on = on;
                if on {
                    self.restart(0);
                } else {
                    return None;
                }
            }
            if !on {
                return None;
            }
        }

        if self.to_next > 0.0 {
            self.to_next -= 1.0;
            return None;
        }
        // `period - 1.0`, not `period`: this frame IS the beat, so it counts
        // towards the next one. Adding the whole period here and decrementing
        // only on the frames that do not fire puts every beat one frame late,
        // which compounds — at 97 bpm it is 97 frames a minute, and it looks
        // exactly like the fractional-period drift this accumulator exists to
        // prevent.
        self.to_next += self.period - 1.0;

        let bar = beats_per_bar.max(1) as u64;
        let accent = self.index % bar == 0;
        self.index += 1;

        if self.count_in_left > 0 {
            self.count_in_left -= 1;
            let n = self.count_in_total - self.count_in_left;
            if self.count_in_left == 0 {
                self.awaiting_downbeat = true;
            }
            return Some(Beat {
                accent,
                count_in: n,
                downbeat: false,
            });
        }
        if self.awaiting_downbeat {
            self.awaiting_downbeat = false;
            // The downbeat always sounds: it is the "go" the whole count-in is
            // there to deliver. It also lands one beat period after the last
            // count-in beat, which is what "recording starts on the downbeat
            // after them" means.
            return Some(Beat {
                accent: true,
                count_in: 0,
                downbeat: true,
            });
        }
        Some(Beat {
            accent,
            count_in: 0,
            downbeat: false,
        })
    }
}

/// The gain, pan and send an automated lane is actually playing at.
///
/// **Free functions rather than methods, and the borrow checker asked for
/// it.** The live-monitor path holds `&mut self.monitor_scratch` across the
/// whole mix, so `self.lane_gain(..)` would be an immutable borrow of all of
/// `self` inside a mutable one. Taking the two fields it actually needs lets
/// the compiler see they are disjoint — and the methods below stay, because
/// every other site reads better for having them.
fn lane_gain_of(auto: &AutoNow, shared: &Shared, lane: usize) -> f32 {
    match auto.gain.get(lane).copied().flatten() {
        Some(v) => ivory_ui::recorder::fader_to_gain(v).clamp(0.0, 8.0),
        None => shared
            .channel_gains
            .get(lane)
            .map_or(1.0, |g| Shared::f32_of(g).clamp(0.0, 8.0)),
    }
}

fn lane_pan_of(auto: &AutoNow, shared: &Shared, lane: usize) -> f32 {
    match auto.pan.get(lane).copied().flatten() {
        Some(v) => (v * 2.0 - 1.0).clamp(-1.0, 1.0),
        None => shared.channel_pans.get(lane).map_or(0.0, Shared::f32_of),
    }
}

fn lane_send_of(auto: &AutoNow, shared: &Shared, lane: usize) -> f32 {
    match auto.send.get(lane).copied().flatten() {
        Some(v) => v.clamp(0.0, 1.0),
        None => shared
            .send
            .get(lane)
            .map_or(0.0, |g| Shared::f32_of(g).clamp(0.0, 1.0)),
    }
}

/// The most a chain may be compensated for.
///
/// **A ceiling, and a visible one.** A linear-phase mastering EQ can ask for
/// half a second; honouring that would mean a megabyte of ring per seat and
/// a desk that responds half a second after you touch it. Eight thousand
/// frames is about 170 ms at 48 kHz — past every lookahead compressor and
/// most convolution — and a plugin that wants more is reported rather than
/// silently half-compensated.
const MAX_PDC_FRAMES: usize = 8_192;

/// One channel's compensating delay: a ring per audio channel.
///
/// Pre-allocated whole and never resized, because the only thread that
/// reads it may not allocate. Its LENGTH is a per-block number; the storage
/// behind it never moves.
struct DelayLine {
    ring: [Vec<f32>; TAP_CHANNELS],
    write: usize,
    /// What it is delaying by right now. A change means the ring holds
    /// samples from a different alignment, so it is cleared rather than
    /// reinterpreted — a plugin swap is allowed to click once, and is not
    /// allowed to smear for ever.
    len: usize,
}

impl DelayLine {
    fn new() -> Self {
        Self {
            ring: std::array::from_fn(|_| vec![0.0; MAX_PDC_FRAMES + 1]),
            write: 0,
            len: 0,
        }
    }

    /// Delay `planar` in place by `len` frames.
    ///
    /// Zero is not a special case that skips the ring — it walks the same
    /// path with a read index equal to the write index, so switching
    /// compensation on and off cannot take a different route through this
    /// and behave differently.
    fn run(&mut self, planar: &mut [Vec<f32>], frames: usize, len: usize) {
        let len = len.min(MAX_PDC_FRAMES);
        if len != self.len {
            for r in &mut self.ring {
                r.fill(0.0);
            }
            self.write = 0;
            self.len = len;
        }
        if len == 0 {
            return;
        }
        let size = MAX_PDC_FRAMES + 1;
        for c in 0..TAP_CHANNELS {
            let (Some(buf), Some(ring)) = (planar.get_mut(c), self.ring.get_mut(c)) else {
                continue;
            };
            let mut w = self.write;
            for f in 0..frames.min(buf.len()) {
                let r = (w + size - len) % size;
                let out = ring[r];
                ring[w] = buf[f];
                buf[f] = out;
                w = (w + 1) % size;
            }
        }
        self.write = (self.write + frames) % size;
    }
}

/// Every automated control's value for one block.
///
/// Plain arrays rather than a lookup, because the audio thread asks for
/// these twelve times a block and an `Option<f32>` in an array is a load and
/// a branch.
#[derive(Debug, Clone, Copy)]
pub struct AutoNow {
    gain: [Option<f32>; CHANNELS],
    pan: [Option<f32>; CHANNELS],
    send: [Option<f32>; CHANNELS],
    mute: [Option<bool>; CHANNELS],
}

impl Default for AutoNow {
    fn default() -> Self {
        Self {
            gain: [None; CHANNELS],
            pan: [None; CHANNELS],
            send: [None; CHANNELS],
            mute: [None; CHANNELS],
        }
    }
}

/// The two maps the engine reads, swapped in together.
///
/// One struct because they are one fact: a bar line is where it is because of
/// the METER, and it happens when it does because of the TEMPO. A block that
/// caught a new tempo map against the old meter would put its bar lines in
/// places neither map describes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Maps {
    pub tempo: ivory_ui::tempo::TempoMap,
    pub meter: ivory_ui::tempo::MeterMap,
}

/// **The click, locked to the song's bar lines.**
///
/// [`Beats`] is a free-running phase accumulator: it counts from wherever the
/// phase last restarted, which is the moment play was pressed. That is right
/// for a count-in — which is a countdown and belongs to the keypress, not to
/// the song — and wrong for everything else. Pressing play at bar 9 put the
/// downbeat at start-of-playback rather than on bar 9's line, and with a
/// tempo MAP it is not merely wrong but meaningless: there is no single
/// period to accumulate.
///
/// So this one does not accumulate at all. It holds the next beat as a
/// `(bar, beat)` PAIR and asks the maps where that lands. Being a pure
/// function of the transport's position, it cannot drift, it survives a
/// locate and a loop wrap by being re-derived, and it puts a beat exactly
/// where the ruler draws one — because the ruler asks the same maps.
///
/// Integers and not a quarter count, deliberately: a signature change moves
/// how many beats a bar holds and how long each is, and `(bar, beat)` walks
/// that correctly by construction while an accumulated quarter would need the
/// change handled at every place it was read.
#[derive(Debug, Default)]
struct BarClock {
    bar: u32,
    beat: u32,
    /// The frame the next beat lands on, so the per-frame test is one
    /// comparison rather than two map walks.
    at_frame: f64,
    /// Where the transport was expected to be. A position that is not this is
    /// a locate, a loop wrap, or the first block after a count-in, and the
    /// answer to all three is the same: work out where we are from scratch.
    expect: u64,
    armed: bool,
}

impl BarClock {
    /// Re-derive from the transport's actual position.
    fn resync(&mut self, pos: u64, maps: &Maps, rate: f64) {
        let secs = pos as f64 / rate.max(1.0);
        let quarter = maps.tempo.beat_at_seconds(secs);
        let (bar, beat) = maps.meter.bar_beat_at_quarter(quarter);
        self.bar = bar;
        // **This beat if we are exactly ON it, otherwise the next.**
        //
        // Both halves matter and the first draft had only the second. Play
        // from 0:00 is exactly on beat one, and it has to click there — that
        // is what pressing play sounds like. A locate that lands on a beat
        // line should click too, for the same reason.
        //
        // But a resync a frame or two INTO a beat must not fire it: that beat
        // has already been and gone. The case that makes this load-bearing is
        // the count-in, where `Beats` sounds the downbeat and the transport
        // starts rolling — the next frame resyncs this clock a frame or two
        // past beat one, and firing there would be a second click on the "go".
        self.beat = beat.floor() as u32;
        self.normalise(maps);
        self.at_frame = self.frame_of(maps, rate);
        if self.at_frame < pos as f64 {
            self.beat += 1;
            self.normalise(maps);
            self.at_frame = self.frame_of(maps, rate);
        }
        self.armed = true;
    }

    /// Carry a beat index past the end of its bar, through however many
    /// signature changes that crosses.
    fn normalise(&mut self, maps: &Maps) {
        for _ in 0..64 {
            let sig = maps.meter.sig_at_bar(self.bar);
            let per_bar = u32::from(sig.beats.max(1));
            if self.beat < per_bar {
                return;
            }
            self.beat -= per_bar;
            self.bar = self.bar.saturating_add(1);
        }
    }

    /// The absolute frame the pending beat falls on.
    fn frame_of(&self, maps: &Maps, rate: f64) -> f64 {
        let sig = maps.meter.sig_at_bar(self.bar);
        // Beats of the SIGNATURE into quarters — the one exchange rate, and
        // the same one `MeterMap::quarters_in_bar` uses.
        let quarter = maps.meter.quarter_at_bar(self.bar)
            + f64::from(self.beat) * (4.0 / f64::from(sig.unit.max(1)));
        maps.tempo.seconds_at_beat(quarter) * rate
    }

    /// Advance one frame. `Some` on the frame a beat lands.
    ///
    /// `pos` is the transport's ABSOLUTE frame for this sample, which is what
    /// makes the whole thing derived rather than counted.
    fn tick(&mut self, pos: u64, maps: &Maps, rate: f64) -> Option<Beat> {
        if !self.armed || pos != self.expect {
            self.resync(pos, maps, rate);
        }
        self.expect = pos + 1;
        if (pos as f64) < self.at_frame {
            return None;
        }
        let accent = self.beat == 0;
        self.beat += 1;
        self.normalise(maps);
        self.at_frame = self.frame_of(maps, rate);
        // **A guard against a map that goes nowhere.** A tempo of zero is
        // impossible — `TempoMap` clamps to `MIN_BPM` — but a meter of one
        // beat per bar at a very high tempo can put two beats inside one
        // frame, and a `while` here would spin the audio thread. One beat per
        // frame is the honest ceiling: nothing can be heard faster.
        if self.at_frame <= pos as f64 {
            self.at_frame = pos as f64 + 1.0;
        }
        Some(Beat {
            accent,
            count_in: 0,
            downbeat: false,
        })
    }
}

/// Frames per beat, with the tempo clamped to something the frame counter can
/// represent.
///
/// `unit` is the time signature's bottom number, and it is what makes 6/8 count
/// eighths rather than quarters. **The tempo always counts QUARTER notes** —
/// forced by the file format, since an SMF tempo meta event is microseconds per
/// quarter and nothing else — so a beat of `1/unit` lasts `4/unit` quarters.
/// At 120 in 6/8 that is a quarter of a second, and a bar of six is a second
/// and a half.
fn period_frames(rate: f64, bpm: f64, unit: u32) -> f64 {
    let bpm = if bpm.is_finite() {
        bpm.clamp(MIN_BPM, MAX_BPM)
    } else {
        120.0
    };
    let rate = if rate.is_finite() && rate > 0.0 {
        rate
    } else {
        48_000.0
    };
    // Clamped to the note values that exist. A zero here would divide by
    // nothing on the audio thread, which is the one place that must not happen.
    let unit = match unit {
        1 | 2 | 4 | 8 | 16 | 32 => unit,
        _ => 4,
    };
    (rate * 60.0 / bpm * (4.0 / f64::from(unit))).max(1.0)
}

// ───────────────────────────────────────────────────────────────────────────
// The drum pads
// ───────────────────────────────────────────────────────────────────────────

/// The pads' samples: what the drumpad is loaded with.
///
/// Decoded by the host at the device's rate — stereo interleaved, exactly the
/// clips' contract, so there is no resampling and no channel mapping down
/// here — and swapped in WHOLE behind the racks' own protocol. The audio
/// thread never allocates a kit and never frees one.
#[derive(Debug)]
pub struct PadKit {
    pub pads: [Option<Arc<ivory_record::decode::Clip>>; PADS],
}

impl Default for PadKit {
    fn default() -> Self {
        Self {
            pads: std::array::from_fn(|_| None),
        }
    }
}

impl PadKit {
    /// Whether any pad is loaded — what decides if channel 10 belongs to the
    /// drums this block. An empty drumpad must not eat a channel-10 note
    /// that some other rig was using: with no kit, the diversion never
    /// happens and the note reaches the lanes exactly as it always did.
    fn live(&self) -> bool {
        self.pads.iter().any(Option::is_some)
    }
}

/// FTZ + DAZ on this thread (x86 only; a no-op elsewhere). Bit 15 is
/// flush-to-zero, bit 6 denormals-are-zero — a tail decaying into
/// subnormals runs orders of magnitude slower on x86 without them.
#[inline]
fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: MXCSR is per-thread state and these two bits only change
    // how this thread rounds subnormals — which is the point.
    unsafe {
        use core::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        _mm_setcsr(_mm_getcsr() | 0x8040);
    }
}

/// One strike, placed inside a chunk by `collect_notes`.
#[derive(Debug, Clone, Copy)]
struct PadHit {
    offset: i32,
    pad: usize,
    velocity: f32,
}

/// One pad voice sounding: a position, a rate and a fade.
#[derive(Debug, Clone, Copy, Default)]
struct PadPlay {
    playing: bool,
    /// Fractional frame position into the clip. Fractional because `step` is:
    /// the tune knob plays the sample faster or slower, varispeed-style.
    pos: f64,
    step: f64,
    /// The strike's velocity, as a gain.
    velocity: f32,
    /// 1.0 while sounding; a stolen voice ramps to 0 by `fade_step` a frame.
    fade: f32,
    fade_step: f32,
    /// The last frame the TRIM allows; the clip's own end still binds.
    /// Zero (the default) means unbounded — see `next`.
    until: f64,
}

impl PadPlay {
    fn start(velocity: f32, step: f64, from: f64, until: f64) -> Self {
        Self {
            playing: true,
            pos: from.max(0.0),
            step,
            velocity,
            fade: 1.0,
            fade_step: 0.0,
            until: until.max(from + 1.0),
        }
    }

    /// Begin the stealing fade. Idempotent: a voice already dying keeps the
    /// ramp it has.
    fn cut(&mut self) {
        if self.playing && self.fade_step == 0.0 {
            self.fade_step = 1.0 / PAD_STEAL_FRAMES;
        }
    }

    /// The next frame of this voice, or silence once it has ended.
    ///
    /// Linear interpolation, by the metronome's own argument: one-shot
    /// percussion is transient material and the tune knob's ± octave stays
    /// well inside where linear is honest for it. Bounds-checked with `get`,
    /// never indexed — this runs on the audio thread.
    fn next(&mut self, clip: &ivory_record::decode::Clip) -> (f32, f32) {
        if !self.playing {
            return (0.0, 0.0);
        }
        // The trim's own end, where one stands; `until` of zero (a
        // default-constructed voice) binds nothing.
        let frames = if self.until >= 2.0 {
            clip.frames().min(self.until as usize)
        } else {
            clip.frames()
        };
        let idx = self.pos as usize;
        if frames == 0 || idx + 1 >= frames {
            self.playing = false;
            return (0.0, 0.0);
        }
        let i = idx * 2;
        let (Some(&l0), Some(&r0), Some(&l1), Some(&r1)) = (
            clip.samples.get(i),
            clip.samples.get(i + 1),
            clip.samples.get(i + 2),
            clip.samples.get(i + 3),
        ) else {
            self.playing = false;
            return (0.0, 0.0);
        };
        let frac = (self.pos - idx as f64) as f32;
        let g = self.velocity * self.fade;
        let out = ((l0 + (l1 - l0) * frac) * g, (r0 + (r1 - r0) * frac) * g);
        self.pos += self.step;
        if self.fade_step > 0.0 {
            self.fade -= self.fade_step;
            if self.fade <= 0.0 {
                self.playing = false;
            }
        }
        out
    }
}

/// One pad's voices: the strike sounding now and the one it displaced.
///
/// Two, not a pool: a pad self-chokes on retrigger — the RS5k manager
/// default this drumpad is modelled on, and the behaviour that keeps a
/// rolled snare from turning into mush — so at most one voice is wanted and
/// one is dying. A third strike inside the ~5 ms steal window hard-replaces
/// the dying voice, which is a truncation nobody has heard under a flam.
#[derive(Debug, Clone, Copy, Default)]
struct PadVoice {
    main: PadPlay,
    dying: PadPlay,
}

impl PadVoice {
    fn trigger(&mut self, velocity: f32, step: f64, from: f64, until: f64) {
        if self.main.playing {
            let mut old = self.main;
            old.cut();
            self.dying = old;
        }
        self.main = PadPlay::start(velocity, step, from, until);
    }

    /// Choked by another pad: fade what is sounding, start nothing.
    fn cut(&mut self) {
        self.main.cut();
        self.dying.cut();
    }

    /// Hard stop, for a kit swap: these voices index into samples that just
    /// left, and a voice resumed inside a different waveform is a sound
    /// nobody asked for.
    fn silence(&mut self) {
        self.main = PadPlay::default();
        self.dying = PadPlay::default();
    }

    fn active(&self) -> bool {
        self.main.playing || self.dying.playing
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The state both threads can see
// ───────────────────────────────────────────────────────────────────────────

/// Everything the UI thread and the audio thread share.
///
/// Atomics only, all `Relaxed`: nothing here orders anything else, and every
/// field is a number one side publishes and the other reads. `f32` and `f64`
/// travel as their bit patterns because there is no `AtomicF32`; the
/// conversions are exact both ways.
#[derive(Debug)]
struct Shared {
    /// One fader per general channel, whatever the channel's kind.
    channel_gains: [AtomicU32; CHANNELS],
    /// Each channel's PAN, -1 (left) .. 1 (right), f32 bits. Centre is 0 dB
    /// — the equal-power law is normalised so nothing gets quieter for
    /// having a pan knob it never touched.
    channel_pans: [AtomicU32; CHANNELS],
    /// Which general channel the built-in DX7 belongs to: a DESK INDEX, or -1
    /// for none. It stopped being a slot when the slots stopped existing on
    /// the desk; the migration guarantees a desk with no instrument anywhere
    /// gets one channel seeded with the built-in, so there is no silent
    /// fallback left to need.
    builtin_strip: AtomicI64,
    /// Which general channel is the DRUMPAD's — its notes are pad strikes,
    /// not pitches. `-1` is nobody's. The host computes it from bay 1's
    /// sentinel, exactly as `builtin_strip`.
    drum_strip: AtomicI64,
    /// Which general channel each capture PICK feeds: desk indices, -1 for an
    /// unbound pick. The k-th AUDIO channel takes pick k — the HOST computes
    /// the order, the callback only reads it, which is the same trust split
    /// every other mask here follows.
    pick_strip: [AtomicI64; INPUTS],
    /// **The live punch, per lane, in project frames.** While the
    /// arrangement records over a lane, everything scheduled on it from
    /// this frame on is being REPLACED and must not sound — the owner
    /// heard the old clip all the way through the overdub. `u64::MAX` is
    /// "no punch"; the host sets the record start while rolling and
    /// clears it when the take ends and the carve makes it permanent.
    punch: [AtomicU64; CHANNELS],
    metro_gain: AtomicU32,
    /// The backing track's level, linear.
    track_gain: AtomicU32,
    /// Whether the backing track should be rolling. Set by the transport.
    /// Where to go, and the generation that makes it an EVENT.
    ///
    /// Two fields, because a bare store cannot be told from the same value
    /// re-asserted — which is exactly how the old `set_track_playing` level
    /// made a locate inexpressible: the edges were found ON the audio thread,
    /// so a seek while rolling changed nothing and a seek while stopped was
    /// overwritten by the rewind. The host stores the payload first and the
    /// generation last, Release; the callback consumes generation-first,
    /// Acquire. See `Renderer::run_transport`.
    transport_at: AtomicU64,
    transport_req: AtomicU64,
    /// The generation the callback has APPLIED. `ack == req` means
    /// `transport_pos` answers the question the host asked.
    transport_ack: AtomicU64,
    /// The transport is moving. A LEVEL, and correctly so — "should I be
    /// moving" genuinely is a level; only the seek was ever an event wearing a
    /// level's clothes. Safe to re-assert every frame, because nothing keys
    /// off its edges any more.
    rolling: AtomicBool,
    /// The frame the callback reached at the end of the last chunk.
    transport_pos: AtomicU64,
    /// **The loop region, in frames: `[loop_a, loop_b)`.** Active only when
    /// `loop_b > loop_a`; zero-width means no loop. While rolling, the
    /// callback wraps `pos` from `loop_b` back to `loop_a` — so the wrap
    /// lands on a block boundary at worst one buffer late, which is a clock
    /// job the UI frame loop could never do cleanly. Written host-side from
    /// the time selection.
    loop_a: AtomicU64,
    loop_b: AtomicU64,
    /// A backing track has been handed over, playing or not.
    ///
    /// Read from the UI thread, which cannot see `Renderer::track` — that lives
    /// on the audio thread. It exists so that "is there anything on the
    /// instrument bus" can be answered without one, which is the question
    /// `TakeSource::resolve` has to ask before every take.
    track_loaded: AtomicBool,
    /// **Input monitoring: hear what the microphone hears.**
    ///
    /// Never persisted, anywhere. It is off at every launch by construction —
    /// see `IvoryApp::input_monitor` — because the failure mode is a room full
    /// of feedback the moment somebody turns their speakers on after a relaunch
    /// they had forgotten was monitoring.
    monitor_on: AtomicBool,
    /// Whether the master out is being streamed over ReaStream. Off by default
    /// and read once per block: when off, the callback neither packs nor pushes
    /// the send ring, so a rig that never streams pays nothing for the feature.
    reastream_on: AtomicBool,
    /// Where the track starts and stops, in frames. `out` of zero means "to
    /// the end", so a clip with no trim needs no knowledge of its own length
    /// down here.

    /// A live-input ring on its way to the renderer, or `None` taking one away.
    ///
    /// Behind a mutex and picked up with `try_lock`, exactly like the backing
    /// track: the ring's read end is `!Sync` and belongs to one thread, and the
    /// audio thread may not wait on the UI thread to hand it over.
    /// The live input's ring, its total channel count, and how those channels
    /// are shared out between the input strips.
    ///
    /// **The widths are the other end of `Picks`.** The capture laid the
    /// chosen inputs end to end in one stream — a mono input then a stereo
    /// pair is one channel then two — and without the widths this side would
    /// have a block of three channels and no idea which strip owns which.
    /// A layout change for the monitor ring already handed over. Read in the
    /// same place `pending_monitor` is, and for the same reason: the audio
    /// thread must never block on the UI's lock.
    pending_monitor_layout: std::sync::Mutex<Option<([u8; INPUTS], ivory_record::audio::Picks)>>,
    pending_monitor: std::sync::Mutex<
        Option<
            Option<(
                rtrb::Consumer<f32>,
                u16,
                [u8; INPUTS],
                u32,
                ivory_record::audio::Picks,
            )>,
        >,
    >,
    /// Frames of the monitor ring that were STALE and thrown away, so that what
    /// is heard is what is happening.
    ///
    /// Non-zero at the start of every session by design — see `mix_monitor` —
    /// and climbing afterwards means the two device clocks are pulling apart.
    monitor_slip: AtomicU64,
    /// How much audio the monitor ring is holding, in frames: the monitoring
    /// path's own latency, on top of the two device buffers, as a number.
    monitor_backlog: AtomicU32,
    /// Bit `i` set: channel `i`'s rack voices ignore the note stream.
    ///
    /// **The MIDI arm, as one mask**, exactly the shape mute and solo travel
    /// in. Zero is every channel armed, which is the desk this app has always
    /// had — an empty channel armed plays nothing, so five loaded instruments
    /// still layer, and switching a channel to AUDIO is what sets its bit.
    midi_off: AtomicU32,
    /// Which channels are ARMED — live to the room. Bits over the eight
    /// generals; an unarmed AUDIO channel's input still drains and still
    /// reaches a take, but contributes nothing to the monitor mix. The MIDI
    /// half of arm rides `midi_off`, computed host-side. Starts ALL ARMED so
    /// a fresh engine behaves as every engine did until the host's first
    /// push says otherwise.
    armed: AtomicU32,
    /// Which input picks tee into the arrangement's capture rings — a bit
    /// per INPUT (not per channel). Zero is off. The host sets it when
    /// arrangement record starts and clears it on stop.
    arr_capture: AtomicU32,
    /// Frames the capture tee had to drop because a ring was full — the host
    /// stalled for seconds. Climbing means a take with a hole in it.
    cap_dropped: AtomicU64,
    /// The per-lane STEM tap is running — set by the host for an export
    /// pass, cleared when it ends.
    stems_on: AtomicBool,
    /// The DRUM BUS fader and pan — the pads sum through this strip.
    drums_gain: AtomicU32,
    drums_pan: AtomicU32,
    /// The master, as a LINEAR gain. The last thing on the instrument bus,
    /// after the limiter, reaching both the device mix and the take — the same
    /// rule the effects follow.
    ///
    /// **Not the click.** The click has its own fader and is added to the
    /// device mix after this, which is why the master and the click behave
    /// like two faders on a desk rather than one inside the other.
    master_gain: AtomicU32,
    /// **Fold the master to MONO on the way out.** A mix check, not a mix
    /// decision: it is what you press to hear what a phone speaker, a club
    /// PA or an AM radio will do to a wide stereo image. The take and the
    /// speakers both get it, because a fold you cannot record is a fold you
    /// cannot compare against.
    master_mono: AtomicBool,
    /// Decibels of gain reduction the limiter has applied since the UI last
    /// looked — a positive number, zero for none. Read and reset.
    gr_db: AtomicU32,
    /// How much of each strip goes to the effects bus, 0..=1, indexed by
    /// [`Strip::index`].
    ///
    /// **Post-fader, which is the one that behaves.** Pull a channel down and
    /// its reverb comes down with it; a pre-fader send is a thing you want
    /// twice a year and explains itself badly the other three hundred days.
    ///
    /// Every instrument slot defaults to 1.0 and everything else to 0.0, which
    /// is exactly the routing this app had when the effects were an insert on
    /// the instrument bus — so an upgrade sounds identical.
    send: [AtomicU32; STRIPS],
    /// **Routing, packed for the audio thread.** A `Vec<Route>` cannot cross
    /// into a callback that must not allocate, lock or drop, so the table is
    /// a fixed array of packed words plus a count: the UI writes, the render
    /// loop reads once per block into its own cache. See `pack_route`.
    routes: [AtomicU32; MAX_ROUTES],
    route_gain: [AtomicU32; MAX_ROUTES],
    route_count: AtomicUsize,
    /// The effects bus's own fader, applied to what comes back from it.
    fx_return: AtomicU32,
    /// The loudest sample each strip produced since the UI last looked, as
    /// f32 bits. Read and RESET, like the limiter's gain reduction beside it:
    /// a peak is a transient and an average would read as almost nothing on
    /// exactly the material a meter is there for.
    strip_peak: [[AtomicU32; 2]; STRIPS],
    /// The drumpad's knobs — per-pad gain, pan and tune as f32 bits, the
    /// choke group as a small integer — and the kit's own fader. Plain
    /// stores read once per chunk, exactly the channel gains' shape.
    pad_gain: [AtomicU32; PADS],
    pad_pan: [AtomicU32; PADS],
    pad_tune: [AtomicU32; PADS],
    pad_choke: [AtomicU32; PADS],
    /// Bit `i`: pad `i` OBEYS NOTE-OFFS — a release fades the voice.
    /// One bit per pad, two words for the 128 of them: word `pad >> 6`,
    /// bit `pad & 63`.
    pad_obey: [AtomicU64; 2],
    /// The playable window, fractions of the sample, f32 bits each.
    pad_trim_start: [AtomicU32; PADS],
    pad_trim_end: [AtomicU32; PADS],
    pad_master: AtomicU32,
    /// The loudest sample each pad made since the UI last looked, f32 bits,
    /// read-and-reset like `strip_peak` above — it is what lights a pad from
    /// the audio rather than from the gesture, so a MIDI strike shows too.
    pad_peak: [AtomicU32; PADS],
    /// One bit per strip. See [`Strip`].
    ///
    /// **Two masks rather than a flag each**, because solo is a question about
    /// ALL the strips at once — "is anything soloed" decides what every other
    /// strip does — and reading eight atomics to answer it would be eight
    /// chances for the answer to change halfway through.
    muted: AtomicU32,
    /// Lanes whose automation must NOT drive their control this block, one
    /// bit each.
    ///
    /// **Two quite different reasons share one mask**, because the audio
    /// thread's question is the same either way — "does the curve win here?"
    /// A lane in `AutoMode::Off` keeps its curve and stops obeying it; a
    /// lane being WRITTEN has a hand on the control, and a curve that fought
    /// the hand would make the fader jump back under it as each point
    /// landed. Both mean: leave this one to the fader.
    auto_suspend: AtomicU32,
    /// What each channel's insert chain delays its output by, in frames, and
    /// the largest of them.
    ///
    /// Published by the audio thread for the DESK to read — a number a
    /// person can see is the difference between "the mix feels smeared" and
    /// "that compressor is costing you 64 frames". Written every block from
    /// the racks themselves, so a swap cannot leave a stale figure.
    /// What each BAY's plugin delays by, written by the main thread and read
    /// by the callback.
    ///
    /// **In atomics rather than in the box**, because the figure has two
    /// authors and they are on different threads: it is first known at load
    /// and can change whenever a preset does, and only the main thread may
    /// ask a plugin for it. A voice writes zero — a voice is where the
    /// signal begins, so its own latency delays nothing upstream.
    seat_latency: Vec<AtomicU32>,
    lane_latency: [AtomicU32; CHANNELS],
    /// **What everything is aligned TO** — the largest of the sixteen
    /// channel chains, and nothing else.
    ///
    /// The master is deliberately not in it. Everything passes through the
    /// master on its way out, so its delay reaches every source equally and
    /// cannot pull anything out of phase with anything else — but putting it
    /// in the target made every channel and every clip wait for it FIRST,
    /// and then the master added it again on the way out. A 30-frame
    /// mastering limiter cost 60 frames between touching a key and hearing
    /// it. Nothing was out of alignment; it was just twice as slow to play.
    /// The three loudness readings, as `f32` bits — momentary, short-term,
    /// integrated. Published ten times a second, which is the rate they can
    /// actually change at: see `loudness::CELL_S`.
    /// **Seven now, not three.** `[momentary, short, integrated]` as
    /// before, then the stereo pair for the two sliding windows —
    /// `[mom L, mom R, short L, short R]`. Appended rather than
    /// interleaved, so every existing index still means what it did.
    lufs: [AtomicU32; 7],
    /// Bumped by the UI thread to ask for the integrated reading to start
    /// again. A generation and not a flag, so two presses in one callback are
    /// still one reset and neither is lost.
    lufs_reset: AtomicU32,
    /// The varispeed ratio, as `f32` bits. **1.0 is off**, and the whole
    /// resampling path is skipped there — see `crate::varispeed`.
    varispeed: AtomicU32,
    /// Which pitches each lane's CLIPS are sounding, two `u64` per lane.
    ///
    /// **Playback, and only playback.** The live notes are already in the
    /// app's own hands — it is the thing that received them — so what it
    /// cannot know is which lanes the TIMELINE is currently playing. That is
    /// the half the theory view needs to light a clip, and the half a
    /// per-track "send this to the theory view" switch has nothing to switch
    /// without.
    ///
    /// Two atomics rather than one `u128`: the wide one is not lock-free on
    /// every target this ships to, and a lock on the audio thread is not a
    /// thing to introduce for a picture.
    lane_sounding: Vec<AtomicU64>,
    max_latency: AtomicU32,
    /// **What is SHOWN** — the whole delay somebody is actually paying,
    /// master included.
    ///
    /// A separate number because a readout that hides latency you are paying
    /// is worth nothing, and an alignment target that includes a delay
    /// everything already shares costs you that delay twice. One atomic
    /// served both and the two wanted different answers.
    total_latency: AtomicU32,
    soloed: AtomicU32,
    /// The eight effect knobs, 0..=1. Four of them are sends on the effects
    /// bus and four are inserts on the master — see `effects.rs`, and see
    /// `Renderer::render` for which is which and why.
    reverb_mix: AtomicU32,
    delay_mix: AtomicU32,
    chorus_mix: AtomicU32,
    phaser_mix: AtomicU32,
    hpf_mix: AtomicU32,
    lpf_mix: AtomicU32,
    gain_mix: AtomicU32,
    limiter_mix: AtomicU32,
    /// What each effect is set to, behind a lock.
    ///
    /// **A lock, unlike the knobs above.** There are eleven of these and they
    /// only move when somebody opens a menu, so the renderer reaches for them
    /// once a block and only when `params_dirty` says something changed. Eleven
    /// atomics would be eleven names to keep in step for a value that is read
    /// as a whole or not at all.
    params: Mutex<crate::effects::Params>,
    params_dirty: AtomicBool,
    metro_on: AtomicBool,
    metro_in_take: AtomicBool,
    /// The COUNT-IN's clicks belong in the take, whatever `metro_in_take` says.
    ///
    /// Two flags rather than one because they answer different questions.
    /// `metro_in_take` is about the performance: a click bleeding through a
    /// take is a ruined take, which is why it is off by default. This one is
    /// about the count that comes first — and if it were not recorded, "record
    /// the count-in into the take" would produce a silence at the head of the
    /// file with nothing to line anything up against, which is the whole
    /// reason somebody asked for it.
    count_in_in_take: AtomicBool,
    bpm: AtomicU64,
    beats_per_bar: AtomicU32,
    /// The time signature's bottom number: what gets the beat.
    beat_unit: AtomicU32,

    /// Bumped by `start_count_in`/`cancel_count_in`. The callback restarts its
    /// beat clock when this changes, which is how a request crosses without a
    /// lock and without a queue that could hold two.
    count_in_req: AtomicU64,
    count_in_beats: AtomicU32,
    /// 1-based beat currently sounding, or 0 for none.
    beat_now: AtomicU32,
    count_in_done: AtomicBool,
    /// Host-timebase instant the count-in downbeat will be **heard**, including
    /// the device's own output delay. `i64::MIN` for "no downbeat yet".
    downbeat_ns: AtomicI64,

    /// Peak magnitude since the UI last read it. `fetch_max` on the bit pattern
    /// is exactly max for non-negative floats, and the UI clears it with a
    /// `swap`, so a peak between two UI frames can never be missed. RMS is a
    /// plain store of the last callback's value: it is a texture, not an event.
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
    clipped: AtomicBool,

    /// How far ahead of the callback its audio is heard, as the backend
    /// reports it. Published every callback because it is the only latency
    /// number that is measured rather than assumed — see [`output_delay_ns`].
    delay_ns: AtomicI64,

    tap_dropped: AtomicU64,
    midi_dropped: AtomicU64,
    pedal_dropped: AtomicU64,
    /// A patch waiting to be picked up by the renderer.
    ///
    /// A mutex, and the audio thread only ever `try_lock`s it: a patch change
    /// that has to wait one block is imperceptible, and a block that has to
    /// wait for a patch change is a dropout.
    pending_voice: std::sync::Mutex<Option<crate::dx7::Voice>>,
    running: AtomicBool,
    callbacks: AtomicU64,
    swaps: AtomicU64,
}

impl Shared {
    fn new() -> Self {
        Self {
            channel_gains: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
            channel_pans: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            builtin_strip: AtomicI64::new(-1),
            drum_strip: AtomicI64::new(-1),
            pick_strip: std::array::from_fn(|_| AtomicI64::new(-1)),
            punch: std::array::from_fn(|_| AtomicU64::new(u64::MAX)),
            metro_gain: AtomicU32::new(0.7f32.to_bits()),
            // Both effects OFF by default. A first launch has to sound like the
            // instrument and not like a room, and `Effects::process` skips its
            // whole cost at zero.
            track_gain: AtomicU32::new(1.0f32.to_bits()),
            transport_at: AtomicU64::new(0),
            transport_req: AtomicU64::new(0),
            transport_ack: AtomicU64::new(0),
            rolling: AtomicBool::new(false),
            transport_pos: AtomicU64::new(0),
            loop_a: AtomicU64::new(0),
            loop_b: AtomicU64::new(0),
            track_loaded: AtomicBool::new(false),
            monitor_on: AtomicBool::new(false),
            reastream_on: AtomicBool::new(false),
            pending_monitor: std::sync::Mutex::new(None),
            pending_monitor_layout: std::sync::Mutex::new(None),
            monitor_slip: AtomicU64::new(0),
            monitor_backlog: AtomicU32::new(0),
            midi_off: AtomicU32::new(0),
            armed: AtomicU32::new(u32::MAX),
            arr_capture: AtomicU32::new(0),
            cap_dropped: AtomicU64::new(0),
            stems_on: AtomicBool::new(false),
            drums_gain: AtomicU32::new(1.0f32.to_bits()),
            drums_pan: AtomicU32::new(0.0f32.to_bits()),
            master_gain: AtomicU32::new(1.0f32.to_bits()),
            master_mono: AtomicBool::new(false),
            // The first channel — the desk's default instrument — sends
            // everything and nothing else sends anything, which is the same
            // sound the old five-slots-send-all default made. See the UI's
            // `Desk::default`, which this mirrors.
            routes: std::array::from_fn(|_| AtomicU32::new(0)),
            route_gain: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
            route_count: AtomicUsize::new(0),
            send: std::array::from_fn(|i| {
                AtomicU32::new(if i == 0 { 1.0f32 } else { 0.0f32 }.to_bits())
            }),
            fx_return: AtomicU32::new(1.0f32.to_bits()),
            strip_peak: std::array::from_fn(|_| [AtomicU32::new(0), AtomicU32::new(0)]),
            pad_gain: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
            pad_obey: [AtomicU64::new(0), AtomicU64::new(0)],
            pad_trim_start: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            pad_trim_end: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
            pad_pan: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            pad_tune: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            pad_choke: std::array::from_fn(|_| AtomicU32::new(0)),
            pad_master: AtomicU32::new(1.0f32.to_bits()),
            pad_peak: std::array::from_fn(|_| AtomicU32::new(0)),
            muted: AtomicU32::new(0),
            auto_suspend: AtomicU32::new(0),
            seat_latency: (0..RACK_SEATS * INSERTS).map(|_| AtomicU32::new(0)).collect(),
            lane_latency: std::array::from_fn(|_| AtomicU32::new(0)),
            lufs: std::array::from_fn(|_| {
                AtomicU32::new(crate::loudness::ABSOLUTE_GATE.to_bits())
            }),
            lufs_reset: AtomicU32::new(0),
            varispeed: AtomicU32::new(1.0f32.to_bits()),
            lane_sounding: (0..CHANNELS * 2).map(|_| AtomicU64::new(0)).collect(),
            max_latency: AtomicU32::new(0),
            total_latency: AtomicU32::new(0),
            soloed: AtomicU32::new(0),
            gr_db: AtomicU32::new(0),
            reverb_mix: AtomicU32::new(0.0f32.to_bits()),
            phaser_mix: AtomicU32::new(0.0f32.to_bits()),
            gain_mix: AtomicU32::new(0.0f32.to_bits()),
            hpf_mix: AtomicU32::new(0.0f32.to_bits()),
            // The low-pass rests at the TOP of its travel, limiter-style:
            // zero here is a 200 Hz filter on an engine nobody touched.
            lpf_mix: AtomicU32::new(1.0f32.to_bits()),
            // **One, not zero.** The limiter's dial is a threshold and its
            // resting position is fully clockwise; starting this at zero is a
            // -48 dB threshold on an engine nobody has touched yet.
            limiter_mix: AtomicU32::new(1.0f32.to_bits()),
            delay_mix: AtomicU32::new(0.0f32.to_bits()),
            chorus_mix: AtomicU32::new(0.0f32.to_bits()),
            params: Mutex::new(crate::effects::Params::default()),
            params_dirty: AtomicBool::new(false),
            metro_on: AtomicBool::new(false),
            // THE default the owner called out: the click is a monitor signal,
            // not a take signal.
            metro_in_take: AtomicBool::new(false),
            count_in_in_take: AtomicBool::new(false),
            bpm: AtomicU64::new(120.0f64.to_bits()),
            beats_per_bar: AtomicU32::new(4),
            beat_unit: AtomicU32::new(4),
            count_in_req: AtomicU64::new(0),
            count_in_beats: AtomicU32::new(0),
            beat_now: AtomicU32::new(0),
            count_in_done: AtomicBool::new(false),
            downbeat_ns: AtomicI64::new(i64::MIN),
            delay_ns: AtomicI64::new(0),
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
            rms_l: AtomicU32::new(0),
            rms_r: AtomicU32::new(0),
            clipped: AtomicBool::new(false),
            tap_dropped: AtomicU64::new(0),
            midi_dropped: AtomicU64::new(0),
            pedal_dropped: AtomicU64::new(0),
            pending_voice: std::sync::Mutex::new(None),
            running: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            swaps: AtomicU64::new(0),
        }
    }

    fn f32_of(cell: &AtomicU32) -> f32 {
        f32::from_bits(cell.load(Ordering::Relaxed))
    }

    fn set_track_gain(&self, linear: f32) {
        self.track_gain
            .store(sane_gain(linear).to_bits(), Ordering::Relaxed);
    }

    /// One channel fader. An out-of-range channel is a no-op, never a panic:
    /// this is reached from UI code, where the panic hook turns a panic into
    /// a dialog and `exit(1)`.
    fn set_channel_gain(&self, ch: usize, linear: f32) {
        if let Some(cell) = self.channel_gains.get(ch) {
            cell.store(sane_gain(linear).to_bits(), Ordering::Relaxed);
        }
    }

    fn set_channel_pan(&self, ch: usize, pan: f32) {
        if let Some(cell) = self.channel_pans.get(ch) {
            let p = if pan.is_finite() {
                pan.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            cell.store(p.to_bits(), Ordering::Relaxed);
        }
    }

    /// All three effect knobs at once, 0..=1.
    ///
    /// Sanitised here rather than at the point of use, for the same reason the
    /// gains are: a NaN reaching a feedback loop does not stay in one sample.
    fn set_effects(&self, sends: crate::effects::Sends) {
        let sane = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        self.reverb_mix
            .store(sane(sends.reverb).to_bits(), Ordering::Relaxed);
        self.delay_mix
            .store(sane(sends.delay).to_bits(), Ordering::Relaxed);
        self.chorus_mix
            .store(sane(sends.chorus).to_bits(), Ordering::Relaxed);
        self.phaser_mix
            .store(sane(sends.phaser).to_bits(), Ordering::Relaxed);
        self.gain_mix
            .store(sane(sends.gain).to_bits(), Ordering::Relaxed);
        self.hpf_mix
            .store(sane(sends.hpf).to_bits(), Ordering::Relaxed);
        self.lpf_mix
            .store(sane(sends.lpf).to_bits(), Ordering::Relaxed);
        self.limiter_mix
            .store(sane(sends.limiter).to_bits(), Ordering::Relaxed);
    }

    /// What the effects are set to. Picked up by the renderer next block.
    ///
    /// **Only when it CHANGED.** This is pushed every frame with the gains, and
    /// taking a lock sixty times a second to write the same eleven numbers is a
    /// lock the audio thread can contend with for nothing.
    fn set_effect_params(&self, p: crate::effects::Params) {
        let Ok(mut g) = self.params.lock() else {
            return;
        };
        if *g != p {
            *g = p;
            self.params_dirty.store(true, Ordering::Release);
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The plugin, and getting it across
// ───────────────────────────────────────────────────────────────────────────

/// A loaded instrument plus the buffers rendering it needs.
///
/// **Field order is load-bearing.** Rust drops fields in declaration order, so
/// `inst` is declared before `module`: an `Instance` released after the factory
/// that made it is a call into a `ComPtr` whose owner has gone.
struct Hosted {
    inst: Instance,
    /// **Whether this plugin GENERATES rather than transforms.**
    ///
    /// Decided once, at load time, from the class's own `subCategories` — and
    /// carried IN the box, because the audio thread may not trust an index a
    /// UI thread wrote about which bay holds what. A voice in a rack is fed
    /// the block's notes and REPLACES the channel's signal; an effect is fed
    /// the channel's signal and transforms it in place. This is the whole of
    /// the difference between an instrument and an insert, and it travels
    /// with the instance it describes.
    voice: bool,
    /// Held, never read: dropping the module unmaps the library and every
    /// `ComPtr` into it becomes a dangling function table. Declared AFTER
    /// `inst` so it is dropped after it.
    #[allow(dead_code, reason = "kept alive for the instance's sake, not read")]
    module: Module,
    /// One `Vec` per channel of the main output bus, each pre-grown to
    /// `MAX_BLOCK` so `Instance::process`'s internal `resize` never allocates.
    bufs: Vec<Vec<f32>>,
    channels: usize,
}

/// The `Instance` on its way into or out of the audio callback.
///
/// # SAFETY
///
/// `Hosted` contains an `ivory_host::Instance` and an `ivory_host::Module`,
/// both of which hold `ComPtr`s and are therefore `!Send`. This asserts `Send`
/// for the box that carries one across, and the assertion rests on four things.
/// There are [`SLOTS`] of these in flight now rather than one, and **every
/// condition is per instance**, so three of them is three separate arguments of
/// the same shape rather than one argument that has to be re-made:
///
/// 1. **It is moved, not shared.** The value travels through an `rtrb` SPSC
///    ring, which moves it out of the producer's slot and into the consumer's
///    hands. At no instant do two threads hold a reference: the UI thread's
///    copy is gone the moment `push` returns, and the callback's copy is gone
///    the moment it pushes it to the retire ring. There is no `Arc`, no
///    `&Hosted` stored anywhere, and no way to observe one from the other side.
///    With three slots this is the condition that decided the ring layout:
///    **one ring pair per slot**, so the ring an instance arrives on *is* which
///    slot it belongs to. The alternative — one pair carrying `(slot, PluginBox)`
///    — would have the callback route on an index chosen by another thread, and
///    an index that is wrong is either a panic (undefined behaviour across the
///    cpal boundary) or two instruments in one slot and none in the other. Six
///    rings of two elements, allocated once at [`Engine::start`], buys the
///    routing back from arithmetic and puts it in the type.
/// 2. **It is moved exactly once in each direction, and the protocol enforces
///    it.** For every `PluginBox` the callback takes, it pushes exactly one
///    back; and it refuses to take one at all unless *that slot's* retire ring
///    has a free slot ([`Renderer::swap_plugins`]). The UI thread waits for that
///    return before it drops anything ([`Engine::hand_off`]). So an instance is
///    either on the UI thread, in one of its own two rings, or in the callback —
///    never in two places, and never dropped twice. Nothing is shared between
///    slots for this to be true of: slot 1's handoff cannot stall, delay or
///    misroute slot 0's, because they have no ring in common.
/// 3. **The only VST3 call the audio thread makes is `process`.** The SDK
///    splits its API into an initialisation context and a processing context.
///    `initialize`, `setupProcessing`, `activateBus`, `setActive` and
///    `terminate` are initialisation-context calls and every one of them happens
///    on the UI thread — the first four inside `Instance::create` before the box
///    is handed over, `terminate` inside `Instance::drop` after it has been
///    handed back. `setProcessing` is the interesting case, because
///    `Instance::process` calls it itself and latches the result: the FIRST
///    block a new instance renders is the warm-up's, on the UI thread, so the
///    transition into the realtime section is already made before the handoff
///    and every later call is a no-op that never reaches the plugin. That is the
///    safer order of the two — the state transition is not racing the move — and
///    it leaves `process` as the only thing the audio thread ever calls, which
///    is exactly the call VST3 designates for it.
///    Three loaded slots are three instances rendered one after another in the
///    same callback, each with its own buffers, and VST3 instances share no
///    mutable state with each other — two copies of the same plugin are the
///    same code and two separate objects, which is exactly what a DAW with the
///    same synth on two tracks already is.
/// 4. **Nothing is dropped in the callback.** `Instance::drop` calls
///    `setProcessing(0)`, `setActive(0)` and `terminate`, and a commercial
///    plugin's `terminate` frees sample memory, joins worker threads and, for
///    several of them, unmaps files. That is unbounded work under a real-time
///    deadline. The callback therefore never drops a `PluginBox`; it returns it,
///    and [`Engine::hand_off`] drops it on the UI thread — once per slot, on the
///    slot's own ring.
///
/// The one case the protocol cannot cover is a callback that stops running
/// while holding an instance — a device that vanished, or a stream that was
/// never started. Then [`Engine::hand_off`] times out and the instance is
/// dropped with the closure when the `cpal::Stream` drops. That happens on
/// whatever thread drops the stream, which for the macOS backend is this one
/// (`coreaudio-rs` stops and uninitialises the AudioUnit *before* freeing the
/// render callback box, so the audio thread is provably not inside it).
struct PluginBox(Option<Box<Hosted>>);

// SAFETY: see the type's documentation above. The four conditions there are the
// argument; this line is only where it is asserted. It stays ONE line for
// [`SLOTS`] instruments: `Slot` and `[Slot; SLOTS]` are `Send` because every
// field of them is, which the compiler works out for itself — the array of
// slots is derived-`Send`, not a second assertion. If a future change makes
// this file need a second `unsafe impl Send`, that is the signal that something
// is being shared rather than moved.
unsafe impl Send for PluginBox {}

/// A clip and where it begins on the timeline, in frames.
///
/// **`start` is 0 for the backing track, and it is one field and one
/// subtraction** — the whole difference between "the backing track" and "a
/// clip". It rides WITH the clip rather than in a separate atomic, so a clip
/// and its position can never disagree; the renderer positions it as
/// `transport_pos - start` and everything before the clip or past its end is
/// silence. A second clip on a lane needs no new concept and no change to the
/// handover protocol.
#[derive(Clone)]
pub struct Placed {
    pub clip: Arc<ivory_record::decode::Clip>,
    /// Which general channel — which LANE — the clip plays through: its
    /// fader, its mute and solo, its send. Not its rack yet: a rack also
    /// runs for the lane's LIVE input, and one rack must run once per block,
    /// which is the unification a later phase owes.
    pub strip: usize,
    /// The timeline frame where the clip begins.
    pub start: u64,
    /// Frames into the FILE where playback starts — the left trim.
    pub offset: u64,
    /// Frames the clip runs on the timeline. **The HOST resolves the doc's
    /// zero-means-whole-file sentinel before pushing**: the audio thread
    /// takes only literal numbers.
    pub len: u64,
    /// Frames of ramp up from silence at the head. Zero for none.
    pub fade_in: u64,
    /// Frames of ramp down to silence at the tail. Zero for none.
    pub fade_out: u64,
    /// The clip's own level, linear. 1.0 is untouched.
    ///
    /// **Where NORMALISE lives.** Rewriting the file would be destructive
    /// and un-undoable; a number on the clip is neither, and it costs one
    /// multiply in a loop that is already doing three.
    pub gain: f32,
}

impl Placed {
    /// A clip that plays a whole file from 0:00, edges bare — what the old
    /// single backing track was, and what most callers still mean.
    pub fn whole(clip: Arc<ivory_record::decode::Clip>, start: u64) -> Self {
        let len = clip.frames() as u64;
        Self {
            clip,
            strip: 0,
            start,
            offset: 0,
            len,
            fade_in: 0,
            fade_out: 0,
            gain: 1.0,
        }
    }
}

/// One MIDI clip, placed: its lane, its span, and its notes — sorted by
/// `at`, clip-relative frames, durations pre-clamped by the HOST so the
/// audio thread takes only literal numbers.
#[derive(Clone)]
pub struct PlacedMidi {
    pub strip: usize,
    /// The timeline frame where the clip begins.
    pub start: u64,
    /// Frames the clip runs; nothing past this sounds.
    pub len: u64,
    pub notes: Arc<Vec<ClipNote>>,
}

/// One note of a MIDI clip, in frames from the clip's start.
#[derive(Clone, Copy)]
pub struct ClipNote {
    pub at: u64,
    pub dur: u64,
    pub key: u8,
    pub vel: u8,
}

/// A clip note that is SOUNDING: where its off falls, and on which lane.
#[derive(Clone, Copy)]
struct ActiveClipNote {
    lane: u8,
    key: u8,
    off: u64,
}

/// How many effect inserts a channel has room for. See the UI's own copy.
pub const INSERTS: usize = ivory_ui::recorder::INSERTS;

/// One channel's insert chain: up to [`INSERTS`] effects, in series.
///
/// **A rack per channel, and each slot its own hand-off.** The pair of rings
/// IS the routing, exactly as it is for an instrument slot — an instance
/// arriving on this consumer belongs in this slot of this channel, and no
/// index has to say so. One pair carrying `(where, PluginBox)` would need the
/// audio thread to trust a number a UI thread wrote.
///
/// An empty rack costs nothing: `run` returns before it touches a buffer, and
/// the channel folds by exactly the path it did before racks existed.
struct Rack {
    slots: [PluginBox; INSERTS],
    incoming: [Consumer<PluginBox>; INSERTS],
    retiring: [Producer<PluginBox>; INSERTS],
}

impl Rack {
    /// How many frames this rack's chain delays its output by.
    ///
    /// The SUM of the bays, because they run in series: three plugins each
    /// looking ahead by 64 frames delay the signal by 192, not by 64.
    ///
    /// Recomputed from the boxes rather than cached, so a swap cannot leave
    /// a stale figure behind — three loads and two adds, once a block, and
    /// there is nowhere for it to go wrong.
    /// Record what one bay's plugin delays by. Answers whether it MOVED,
    /// so a caller polling every frame can do nothing on the frames where
    /// nothing happened — which is nearly all of them.
    ///
    /// Out of range is a silent `false` rather than a panic: this is reached
    /// from the plugin-load path, and a bay index that does not exist is a
    /// bay with nothing in it, delaying nothing.
    fn set_latency(shared: &Shared, seat: usize, bay: usize, frames: u32) -> bool {
        let Some(cell) = shared.seat_latency.get(seat * INSERTS + bay) else {
            return false;
        };
        cell.swap(frames, Ordering::Relaxed) != frames
    }

    fn latency(shared: &Shared, seat: usize) -> u32 {
        (0..INSERTS)
            .filter_map(|b| shared.seat_latency.get(seat * INSERTS + b))
            .map(|c| c.load(Ordering::Relaxed))
            .sum()
    }

    /// Take whatever the UI thread has sent, and give back what it displaces.
    ///
    /// Same shape as an instrument slot's swap and for the same reason: the
    /// callback never DROPS a plugin — dropping runs the vendor's teardown,
    /// which is not something to do between two blocks of audio — it returns
    /// it for the UI thread to drop.
    fn swap(&mut self) -> usize {
        let mut swapped = 0;
        for i in 0..INSERTS {
            // **Room to give the old one back, checked BEFORE taking the new
            // one.** This was `while let Ok(next) = incoming.pop()` followed by
            // `let _ = retiring.push(old)`, and `let _` on an rtrb push is a
            // trap: `PushError::Full(T)` carries the value, so discarding the
            // `Result` DROPS the `PluginBox` — which runs `IComponent::terminate`,
            // frees the vendor's sample memory and joins its worker threads,
            // on the audio callback, between two blocks.
            //
            // It is reachable: `hand_off_insert` gives up after `RETIRE_TIMEOUT`
            // and leaves the retired plugin sitting in the ring, so the next
            // load finds `retiring` full.
            //
            // The instrument slots have always had this right — see
            // `swap_plugins`, which tests `retiring.slots() == 0` first and
            // leaves the arrival in its ring for the next block. This is that,
            // and the two protocols are now the same protocol.
            if self.retiring[i].slots() == 0 {
                continue;
            }
            let Ok(next) = self.incoming[i].pop() else {
                continue;
            };
            let old = std::mem::replace(&mut self.slots[i], next);
            // Cannot fail: the room was checked one line above and this is the
            // only producer. Still not `unwrap` — a panic here is a panic on
            // the audio thread.
            let _ = self.retiring[i].push(old);
            swapped += 1;
        }
        swapped
    }

    /// Whether anything is loaded. The whole point of asking is to leave the
    /// common path exactly as it was.
    fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.0.is_none())
    }
}

/// Run a rack over one channel's PLANAR audio, in place.
///
/// A free function rather than a method, because every caller holds the rack
/// and the scratch as separate fields of the renderer and the borrow checker
/// only sees that if they are named separately.
///
/// `planar[c]` must hold at least `frames`. Channels a plugin does not write
/// keep what they had, and a plugin that answers with fewer channels than it
/// was given has its last one read twice rather than leaving a silent side —
/// the same rule the bus insert has always followed.
fn run_rack(
    rack: &mut Rack,
    planar: &mut [Vec<f32>],
    frames: usize,
    out: &mut [Vec<f32>],
    notes: &[ivory_host::Note],
    controls: &[ivory_host::Control],
) {
    for slot in &mut rack.slots {
        let Some(p) = slot.0.as_mut() else {
            continue;
        };
        if p.voice {
            // **A voice renders into its OWN buffers and replaces the
            // channel.** Its own, because `fx_out` is TAP_CHANNELS wide and an
            // instrument's main bus is as wide as it likes — the modelled piano's is
            // eight — and `process_through` refuses a block whose output is
            // narrower than the bus. `bufs` was allocated at the instrument's
            // width in `load_insert` for exactly this call.
            //
            // Replaces rather than mixes: a MIDI channel's signal IS its
            // instrument, and whatever was in `planar` before bay 1 ran is
            // silence or an upstream voice being superseded.
            if p.inst
                .process_with_controls(notes, controls, frames, &mut p.bufs)
                .is_err()
            {
                continue;
            }
        } else if p.inst.process_effect(planar, frames, out).is_err() {
            // A refused block leaves the audio ALONE rather than replacing it
            // with whatever was in the output buffers: a faulted insert is a
            // channel that stops being processed, not one that starts making
            // noise.
            continue;
        }
        let wrote = p.channels.max(1);
        let src_bufs: &[Vec<f32>] = if p.voice { &p.bufs } else { &out[..] };
        for c in 0..planar.len() {
            let Some(src) = src_bufs.get(c.min(wrote - 1)) else {
                continue;
            };
            let Some(dst) = planar.get_mut(c) else {
                continue;
            };
            dst[..frames].copy_from_slice(&src[..frames]);
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The renderer: everything that runs on the audio thread
// ───────────────────────────────────────────────────────────────────────────

/// The audio callback's own state. Built on the UI thread, moved in once.
struct Renderer {
    shared: Arc<Shared>,
    timebase: Timebase,
    rate: f64,
    dev_channels: usize,

    /// The instruments summed, interleaved stereo, one chunk long.
    ///
    /// **Sized at [`Engine::start`] and never again**, which is what makes the
    /// sum allocation-free no matter how many slots fill up later. Stereo
    /// because the tap is stereo for the life of the engine ([`TAP_CHANNELS`])
    /// and because a common bus has to have one width: the slots' own widths
    /// are resolved into it by [`stereo_of`], on the way in.
    mix: Vec<f32>,

    midi: Consumer<MidiEvent>,
    /// The HARDWARE fast lane: filled from the midir callback thread through
    /// `midi::MidiFastLane`, never from the UI. Its events are stamped in the
    /// far past on purpose, so every one lands at the current block's first
    /// frame — the soonest a block engine can sound anything. This is the
    /// ring that took the UI frame out of the player's hands; see the module
    /// docs on live latency.
    hw_midi: Consumer<MidiEvent>,
    /// One event popped but not yet due. `rtrb` has no peek, so an event that
    /// belongs to a later block lives here rather than being pushed back.
    pending: Option<MidiEvent>,
    notes: Vec<Note>,
    /// The instrument this app ships with.
    ///
    /// Rendered when a slot has asked for it, and when NO slot has anything at
    /// all — so a fresh install makes a sound, and choosing "Tangent DX7" in
    /// the picker keeps making one after a real plugin has been tried and
    /// removed. See `dx7/` for the synth and `builtin.rs` for what it grew out
    /// of.
    /// The backing track, and where in it the transport is.
    ///
    /// **Held by the renderer, swapped in whole.** A clip is a hundred
    /// megabytes of `Vec<f32>` and the audio thread must never allocate or
    /// free one, so it arrives behind an `Arc` through `pending_track` and the
    /// old one is dropped by whoever pushed the new one.
    clips: Vec<Placed>,
    /// The MIDI half of the schedule, and a cursor per clip — advanced while
    /// the transport rolls, re-sought on a jump.
    midi_clips: Vec<PlacedMidi>,
    midi_cursor: Vec<usize>,
    /// Clip notes sounding right now, so their offs land on time — and land
    /// AT ALL on a stop or a jump.
    active_clip: Vec<ActiveClipNote>,
    /// Per-lane note scratch for one block: the live events (armed) and the
    /// clips' both, copied — the rack takes one slice.
    lane_notes: Vec<Note>,
    /// The transport's position in frames, advanced by `run_transport` and
    /// nothing else. Project frames, not clip frames: a clip positions itself
    /// as `pos - placed.start`.
    pos: u64,
    /// One slewed gain per lane, for the clips standing on it. The lane's
    /// fader by way of `strip_is_heard`, advanced once per frame per lane.
    lane_gain: [f32; CHANNELS],
    /// The transport generation this callback has applied. See `run_transport`.
    seen_req: u64,
    /// Whether the last chunk rolled, for the note flush on a falling edge —
    /// renderer-local, because nothing OUTSIDE the callback keys off edges any
    /// more; this one exists so a stop can end the notes it strands.
    was_rolling: bool,
    /// A loop just wrapped `pos` backward; the NEXT block must flush so the
    /// MIDI clip cursors rewind. Set on the wrap block, consumed on the one
    /// after — and it MUST be the one after, because the wrap block returns
    /// the PRE-wrap `at` (still near the loop's end) and flushing there would
    /// fast-forward every cursor to the end again, leaving the head of the
    /// loop silent. Audio clips read straight off `at` and never needed this;
    /// the note scheduler is cursor-based and does.
    pending_flush: bool,
    /// The LAST block's clip walk parked a note it could not deliver — the
    /// lane's note list or the sounding ledger was full — and left the
    /// cursor standing on it. `pending`'s idea for the clips: the next
    /// block's walk must sound that note late rather than fast-forward over
    /// it as "behind the playhead". Cleared by anything that rewinds the
    /// cursors, because a rewind's behind-the-playhead notes really are
    /// skipped, not owed.
    clip_parked: bool,
    /// Every pitch currently sounding, from the note stream this renderer
    /// itself handed out. What a stop or a jump has to end.
    sounding: u128,
    /// The clip arriving, and the one going back to be dropped where it came
    /// from. The pair IS the routing, exactly as a plugin's is.
    track_incoming: Consumer<Vec<Placed>>,
    track_retiring: Producer<Vec<Placed>>,
    midi_incoming: Consumer<Vec<PlacedMidi>>,
    midi_retiring: Producer<Vec<PlacedMidi>>,
    /// The drumpad. The kit swaps in whole — `pad_incoming`/`pad_retiring`
    /// are the clips' protocol exactly — and the voices are fixed and
    /// reused, like the metronome's.
    pad_kit: PadKit,
    pad_incoming: Consumer<PadKit>,
    pad_retiring: Producer<PadKit>,
    /// The media explorer's AUDITION: one clip at a time, swapped in like
    /// the kit, played once from the top, summed into the mix.
    preview_incoming: Consumer<Option<Arc<ivory_record::decode::Clip>>>,
    preview_retiring: Producer<Option<Arc<ivory_record::decode::Clip>>>,
    preview: Option<Arc<ivory_record::decode::Clip>>,
    preview_pos: usize,
    pad_voices: [PadVoice; PADS],
    /// This chunk's strikes, filled by `collect_notes` beside the note list.
    pad_hits: Vec<PadHit>,
    /// Note-OFFS for pads that obey them, same frame offsets.
    pad_offs: Vec<PadHit>,
    /// The kit's fader, slewed one-pole like every fader.
    pad_gain: f32,
    builtin: crate::dx7::Dx7,
    /// The built-in's own output, before its slot gain.
    ///
    /// **It needs a buffer of its own for the same reason a plugin does.** The
    /// FM renders by ADDING into whatever it is handed, so rendering it
    /// straight onto the bus leaves no moment at which its contribution is
    /// separable — and therefore no moment at which a fader can be applied to
    /// it. That is exactly what happened: the DX7's slot fader moved a number
    /// that reached the settings, the engine and the meter, and never once
    /// reached the audio.
    builtin_scratch: Vec<f32>,
    /// Where each general channel's fader has slewed to. Per channel, because
    /// the eight move independently — the same treatment every fader gets.
    channel_gain: [f32; CHANNELS],
    /// Reverb, delay and chorus on the instrument sum. Free at rest.
    /// The effects BUS: reverb, delay and chorus, fed by the strips' sends.
    ///
    /// **Two instances, and the second one's reverb is never used.** An
    /// `Effects` can only be in one place in the graph, and these are two
    /// places: time effects belong on a send everything can reach, and a
    /// limiter belongs across the output it is protecting. The unused
    /// allocations cost about a megabyte of RAM that is never touched, which
    /// is a better trade than making every buffer in `effects.rs` optional to
    /// save it.
    effects: crate::effects::Effects,
    /// The master INSERT: high-pass, low-pass and the limiter, across
    /// everything on its way out.
    master_effects: crate::effects::Effects,
    /// The SPEAKERS' own master insert.
    ///
    /// **Two mixes leave this renderer, and they are not always the same
    /// one.** The take is the desk: every strip that is not muted is in it.
    /// The room is the desk minus an input nobody asked to hear — which is the
    /// ordinary rig, because monitoring is off at every launch and a
    /// microphone through speakers is how a room starts feeding back.
    ///
    /// The two diverge BEFORE the master insert, so the speakers need their
    /// own instance of it: a limiter is not a linear thing and its output for
    /// the room cannot be derived from its output for the file. It runs only
    /// when the two actually differ — see `room_live` — so a rig with no input
    /// pays nothing at all for it.
    room_effects: crate::effects::Effects,
    /// The room's mix, interleaved like `mix`, valid only while `room_live`.
    room: Vec<f32>,
    /// What the room must NOT be given: the input's own contribution to `mix`,
    /// already scaled by how much of it monitoring is currently withholding.
    ///
    /// Subtracted rather than separately summed, because everything upstream
    /// of the master insert is a plain sum and the difference between the two
    /// mixes is exactly this. One buffer, filled by the pass that made the
    /// samples in the first place.
    input_dry: Vec<f32>,
    /// Whether the room and the take differ this block.
    room_live: bool,
    /// How much of the input the room is hearing, slewed 0..1 by
    /// `Shared::monitor_on`.
    ///
    /// Slewed, and not a branch, because toggling monitoring is a hand on a
    /// control and a step of a whole microphone is a click in the speakers.
    /// It scales the send as well as the dry: a channel switched out of the
    /// room does not get to arrive back in it through the reverb.
    room_gain: f32,
    /// The click, one value per frame of the current chunk, and the same
    /// again as it should reach the FILE — zero on the frames the take is not
    /// meant to carry it. Two buffers rather than a flag, so the pass that
    /// writes the take does not re-derive a per-frame condition.
    click_out: Vec<f32>,
    /// The preview's frames for this chunk, stereo — added AFTER the tap,
    /// beside the click, so an audition is never printed into a take.
    preview_out: Vec<f32>,
    click_taped: Vec<f32>,
    /// A user effect ACROSS the effects bus, if one is loaded.
    ///
    /// **On the bus rather than on every channel**, and that is a design
    /// decision rather than a shortcut. A reverb is a send effect: one
    /// instance that four channels feed at four different amounts is what a
    /// desk does and what the plugin expects, and per-channel inserts would be
    /// nine instances of the same reverb running at once for a machine this
    /// app is careful about.
    /// One rack per channel, master last — `Strip::index` and then `STRIPS`.
    ///
    /// **The effects bus is one of them.** It used to be a field of its own
    /// with its own hand-off and its own function; a bus is a channel with a
    /// send instead of a fader, and one mechanism that every channel uses is
    /// worth more than a special case that only the bus understands.
    racks: [Rack; RACK_SEATS],
    /// Deinterleaved in and out for it. Preallocated, like everything else the
    /// audio thread touches.
    fx_in: Vec<Vec<f32>>,
    fx_out: Vec<Vec<f32>>,
    /// The effects bus's own buffer, one block long, interleaved like `mix`.
    aux: Vec<f32>,
    fx_return_gain: f32,
    /// The renderer's OWN copy of the parameters.
    ///
    /// Refreshed from `Shared` only when the dirty flag says to, so the audio
    /// thread never blocks on a lock in the common case and never blocks at
    /// all: a `try_lock` that fails simply means next block.
    effect_params: crate::effects::Params,
    /// Control changes for this block — the sustain pedal, above all.
    ///
    /// Preallocated beside `notes` and for the same reason: `process` runs on
    /// the audio thread and a `Vec` that grows there allocates under a
    /// real-time deadline. A pedal is one message per press, so this never
    /// needs to be large.
    controls: Vec<Control>,

    tap: Producer<f32>,
    /// Interleaved recorder mix for one block. Sized once.
    tap_scratch: Vec<f32>,
    /// One frame of mapped instrument output. Sized once, to the larger of the
    /// device's channel count and the tap's.
    frame: Vec<f32>,

    click: Click,
    voice: Voice,
    beats: Beats,
    /// The click while the transport ROLLS. See [`BarClock`]: derived from
    /// the position rather than accumulated, so it lands on the song's own
    /// bar lines and follows a tempo map that has no single period.
    bar_clock: BarClock,
    /// The tempo and meter maps the click and the ruler share.
    maps: Maps,
    /// **What every automated control reads THIS block.**
    ///
    /// Resolved once at the top of `render` and then asked twelve times —
    /// seven gain reads and five pan reads scattered across the routing
    /// fold, the live monitor, the arrangement clips, the file preview, the
    /// drum pads and the three MIDI paths in `mix_channels`. Resolving it
    /// per site would be twelve binary searches instead of one and, far
    /// worse, twelve chances to leave one out: a version that automated only
    /// `mix_clips` would look exactly right on an audio clip and do nothing
    /// at all to a MIDI lane or a live input.
    ///
    /// `None` means nothing automates that control, which is what leaves the
    /// fader in charge — the automation OVERRIDES the fader while a curve is
    /// there and is silent otherwise. Not a multiplier: a volume curve is
    /// the fader's position, the way every desk means it.
    auto: AutoNow,
    /// One compensating delay per CHANNEL. Sixteen, not one per rack seat:
    /// the pads are struck rather than played back and are deliberately out
    /// of the alignment — see `publish_latency` — and the master's chain
    /// delays everything equally, so it has nothing to be aligned against.
    pdc: Vec<DelayLine>,
    automation: ivory_ui::automation::Automation,
    auto_incoming: Consumer<ivory_ui::automation::Automation>,
    auto_retiring: Producer<ivory_ui::automation::Automation>,
    maps_incoming: Consumer<Maps>,
    maps_retiring: Producer<Maps>,
    /// Frames left during which a count-in click is still SOUNDING.
    ///
    /// `Beats::counting_in` goes false the moment the last count-in beat is
    /// consumed, and the click is a decaying sample — so a take that recorded
    /// the count-in by that flag alone would cut the last click off mid-decay,
    /// and the downbeat, which is the one everybody listens for, not at all.
    /// Reset to a beat's worth of frames every time a count-in beat or the
    /// downbeat fires.
    count_in_tail: u64,

    /// `process` calls made since the stream opened. Not a statistic: it is how
    /// the chunking test observes that a 4096-frame callback became eight
    /// blocks, which is not visible any other way without a real plugin.
    chunks: u64,
    /// Smoothed click gain. One-pole per frame, exactly like each slot's.
    metro_gain: f32,
    gain_coeff: f32,
    /// The live input, when one is open. Drained EVERY block whether or not
    /// anybody is listening — see `mix_monitor`.
    monitor: Option<rtrb::Consumer<f32>>,
    /// How many channels that ring carries per frame.
    monitor_channels: usize,
    /// Slewed, so switching monitoring on is a fade rather than a bang.
    monitor_gain: [f32; INPUTS],
    /// Channels per input strip, in stream order. Zero for one not open.
    monitor_widths: [usize; INPUTS],
    /// **Which DEVICE column each strip's channels live in.** The capture
    /// rings carry whole device frames now, so the picks that used to decide
    /// the ring's shape decide the READING of it instead — and a stereo pick
    /// need not be two adjacent inputs, so this cannot be an offset.
    monitor_picks: ivory_record::audio::Picks,
    /// The INPUT device's own block, in frames. What the monitor ring is
    /// allowed to hold before the surplus is stale rather than in flight.
    monitor_block: usize,
    monitor_scratch: Vec<f32>,
    /// The arrangement's capture tee, one ring per input pick: raw frames go
    /// in here while `arr_capture` has the pick's bit, and the host drains
    /// them into the project's WAV files.
    cap_tx: Vec<Producer<f32>>,
    /// The STEM tap: every lane's post-fader stereo, interleaved
    /// `CHANNELS * 2` wide, while `stems_on`. The export's second ear.
    stem_tx: Producer<f32>,
    /// **The scope's tap: the SPEAKERS' mix, before the click joins it.**
    /// "Everything the app is doing, minus the metronome" exists as a
    /// buffer at exactly one point in the block — this is it. A full ring
    /// drops the block, which is the policy: the scope is a picture.
    scope_tx: Producer<f32>,
    scope_scratch: Vec<f32>,
    /// The loudness meter, fed the same pair the VU faces read.
    ///
    /// **Not the same measurement, and deliberately at the same tap.** A
    /// loudness reading that disagreed with the meter beside it about WHAT it
    /// was measuring would be two meters arguing; they measure the same
    /// signal and answer different questions about it. See `crate::loudness`.
    loudness: crate::loudness::Loudness,
    /// The reset generation last acted on. See `Shared::lufs_reset`.
    lufs_seen: u32,

    // ── varispeed ──────────────────────────────────────────────────────────
    //
    // Idle these cost one atomic load a callback and nothing else: the whole
    // path below is skipped at a ratio of 1.0, which the dial's detent
    // guarantees is reachable. See `crate::varispeed`.
    /// Where the output side is reading from. ONE phase for all three
    /// output streams, so the take, the speakers and the scope are the same
    /// instant.
    vs_phase: crate::varispeed::Phase,
    vs_out: crate::varispeed::Stream,
    vs_tap: crate::varispeed::Stream,
    vs_scope: crate::varispeed::Stream,
    vs_reastream: crate::varispeed::Stream,
    /// The input's own phase, going the other way — see the module note.
    vs_in_phase: crate::varispeed::Phase,
    vs_in: crate::varispeed::Stream,
    /// The device output at PROGRAM rate, before it is squeezed into `out`.
    vs_dev: Vec<f32>,
    /// One reusable destination for resampling the three taps in place.
    vs_dst: Vec<f32>,
    /// The live input at REAL rate, before it is stretched into the program.
    vs_in_src: Vec<f32>,
    /// Whether the last callback was engaged, so the histories can be
    /// dropped on the way in rather than bleeding a block from before.
    vs_was_engaged: bool,
    /// The routing table, copied out of `Shared` once per block so the inner
    /// loops read plain values rather than atomics: (from, target, hardware,
    /// stereo, gain).
    route_cache: [(usize, usize, bool, bool, f32); MAX_ROUTES],
    route_n: usize,
    /// Any HARDWARE route this block — the flag that decides whether
    /// per-channel audio has to be kept at all. A desk that only uses sends
    /// pays nothing for the jacks it never asked for.
    routes_hw: bool,
    /// Whether each channel still reaches the master DIRECTLY: 1.0 normally,
    /// 0.0 once it feeds another channel.
    ///
    /// **Routing a channel to a channel MOVES it, it does not copy it.** It
    /// copied, once — so making a send put the same audio into the master
    /// twice, at the same level, and the owner heard every routed track play
    /// twice. A copy is what the SND knob already does, into the effects
    /// bus, which is why it is the knob you reach for to feed a reverb; this
    /// is the other thing, the one that bus-routing means: the channel now
    /// arrives through its destination, at that destination's fader, pan and
    /// rack. Hardware routes are NOT affected — a jack feed is a tap, and
    /// muting the master because somebody wanted a headphone mix would be
    /// its own bug.
    route_direct: [f32; CHANNELS],
    /// Incoming sends, per destination channel, interleaved stereo. Filled by
    /// the lane loop, processed through the destination's rack, then folded
    /// into the master.
    send_accum: Vec<f32>,
    /// Planar scratch for running a destination's rack over its sends — the
    /// same shape `pad_io` uses for the drum bus, and for the same reason.
    send_io: Vec<Vec<f32>>,
    /// The master's stereo pair onto the ReaStream send ring — the same signal
    /// the scope reads (what leaves), pushed only while streaming is on. Its own
    /// stride-2 scratch so the block is contiguous L,R for the sender to plane.
    reastream_tx: Producer<f32>,
    reastream_scratch: Vec<f32>,
    /// One block of stems, accumulated by the three mixers and pushed once.
    stem_scratch: Vec<f32>,
    /// Each PAD's own block, interleaved stereo — rendered here first so a
    /// pad's rack can run over its voice alone before the bus sums it.
    pad_out: Vec<f32>,
    /// The io pair a pad's rack processes in place.
    pad_io: Vec<Vec<f32>>,
    /// `stems_on`, read once a block so the three mixers agree.
    stems_live: bool,
}

impl Renderer {
    /// Run one channel's insert rack over an INTERLEAVED stereo buffer.
    ///
    /// The racks want planar — one array per channel, which is what every VST3
    /// effect asks for — and the mix and the bus are interleaved, so this is
    /// the conversion at both ends. Slots hand their audio over planar
    /// already and skip it.
    ///
    /// Returns immediately on an empty rack, so a channel nobody has put an
    /// effect on costs one branch and folds by exactly the path it always did.
    fn run_insert_chain(&mut self, at: usize, buf_is_aux: bool, frames: usize) {
        // **The block's notes, gated by the channel's own arm bit.** A voice
        // in this rack is fed the same list every slot reads — see
        // `collect_notes`, drained once per block — unless the channel is
        // switched away from MIDI, in which case it is fed nothing and holds
        // its tails. The gate is per CHANNEL and travels as one atomic mask,
        // like mute and solo.
        let armed = self.shared.midi_off.load(Ordering::Relaxed) & (1 << at.min(31)) == 0;
        let notes: &[ivory_host::Note] = if armed { &self.notes } else { &[] };
        let controls: &[ivory_host::Control] = if armed { &self.controls } else { &[] };
        let Some(rack) = self.racks.get(at) else {
            return;
        };
        if rack.is_empty() {
            return;
        }
        let n = frames * TAP_CHANNELS;
        {
            let src = if buf_is_aux { &self.aux } else { &self.mix };
            let Some(src) = src.get(..n) else {
                return;
            };
            for c in 0..TAP_CHANNELS {
                let Some(dst) = self.fx_in.get_mut(c) else {
                    return;
                };
                for (f, d) in dst.iter_mut().take(frames).enumerate() {
                    *d = src[f * TAP_CHANNELS + c];
                }
            }
        }
        let Some(rack) = self.racks.get_mut(at) else {
            return;
        };
        run_rack(
            rack,
            &mut self.fx_in,
            frames,
            &mut self.fx_out,
            notes,
            controls,
        );
        let dst = if buf_is_aux {
            self.aux.get_mut(..n)
        } else {
            self.mix.get_mut(..n)
        };
        let Some(dst) = dst else {
            return;
        };
        for c in 0..TAP_CHANNELS {
            let Some(src) = self.fx_in.get(c) else {
                continue;
            };
            for f in 0..frames {
                if let (Some(v), Some(s)) = (dst.get_mut(f * TAP_CHANNELS + c), src.get(f)) {
                    *v = *s;
                }
            }
        }
    }

    fn swap_plugins(&mut self) -> usize {
        // Every rack, by one protocol: whatever arrives replaces what is
        // there and the old one goes back to be dropped on the thread that
        // made it.
        let mut swapped = 0;
        for rack in &mut self.racks {
            swapped += rack.swap();
        }
        if swapped > 0 {
            self.shared
                .swaps
                .fetch_add(swapped as u64, Ordering::Relaxed);
        }
        swapped
    }

    /// Collect the events that belong to `[block_start, block_start + frames)`.
    ///
    /// Always drains, even with no plugin loaded: a queue left to fill up would
    /// fire a burst of stale note-ons the instant an instrument appeared.
    fn collect_notes(&mut self, block_start: Nanos, frames: usize) {
        self.notes.clear();
        self.controls.clear();
        self.pad_hits.clear();
        self.pad_offs.clear();
        // Whether channel 10 belongs to the drums this block. Read once: a
        // kit swap lands in `mix_pads`, so this answers for the PREVIOUS
        // chunk's kit — one chunk of grace on a load, never a torn read.
        let pads_live = self.pad_kit.live();
        loop {
            let event = match self.pending.take() {
                Some(e) => e,
                // The HARDWARE ring first: its events are past-stamped and
                // always due, so nothing from it can park in `pending` as
                // Later and stall the other ring. (A future-stamped APP
                // event held in `pending` does briefly gate this pop — only
                // `plugin_test` schedules ahead, and it has no keyboard.)
                None => match self.hw_midi.pop() {
                    Ok(e) => e,
                    Err(_) => match self.midi.pop() {
                        Ok(e) => e,
                        Err(_) => break,
                    },
                },
            };
            let offset = match place(event.stamp, block_start, self.rate, frames) {
                Placement::Later => {
                    self.pending = Some(event);
                    break;
                }
                Placement::At(offset) => offset,
            };
            // ── channel 10: the drum channel ───────────────────────────────
            // While a kit is loaded, a channel-10 note is a pad strike and
            // nothing else — it never reaches the lanes, because a kick that
            // also plays a C on the piano is a surprise, not a layer. Offs
            // are consumed too: pads are one-shots and ignore them, and so
            // is a note outside the rack's sixteen.
            if pads_live
                && event.status & 0x0F == PAD_CHANNEL
                && matches!(event.status & 0xF0, 0x80 | 0x90)
            {
                let velocity = event.data2 & 0x7F;
                if event.status & 0xF0 == 0x80 || velocity == 0 {
                    // A RELEASE: only pads that obey note-offs hear it.
                    let pad = usize::from((event.data1 & 0x7F).wrapping_sub(PAD_BASE));
                    if event.data1 & 0x7F >= PAD_BASE
                        && pad < PADS
                        && self.shared.pad_obey[pad >> 6].load(Ordering::Relaxed)
                            & (1u64 << (pad & 63))
                            != 0
                        && self.pad_offs.len() < self.pad_offs.capacity()
                    {
                        self.pad_offs.push(PadHit {
                            offset,
                            pad,
                            velocity: 0.0,
                        });
                    }
                } else if event.status & 0xF0 == 0x90 && velocity != 0 {
                    if self.pad_hits.len() == self.pad_hits.capacity() {
                        // Full. Hold it for the next chunk, exactly as the
                        // note list does two branches down.
                        self.pending = Some(event);
                        break;
                    }
                    let pad = usize::from((event.data1 & 0x7F).wrapping_sub(PAD_BASE));
                    if event.data1 & 0x7F >= PAD_BASE && pad < PADS {
                        self.pad_hits.push(PadHit {
                            offset,
                            pad,
                            velocity: f32::from(velocity) / 127.0,
                        });
                    }
                }
                continue;
            }
            if self.notes.len() == self.notes.capacity() {
                // Full. Hold this one for the next block rather than pushing
                // (which reallocates on the audio thread) or dropping it (which
                // hangs a note). A hundred and twenty-eight events in one block
                // is a MIDI file being dumped, not a person playing.
                self.pending = Some(event);
                break;
            }
            if let Some(note) = note_from_midi(event.status, event.data1, event.data2) {
                // The ledger of what is sounding, kept beside the push so
                // every path that delivers a note also records it — a stop or
                // a locate reads this to know which pitches it strands.
                if let Ok(pitch) = u8::try_from(note.pitch) {
                    if pitch < 128 {
                        if note.on {
                            self.sounding |= 1 << pitch;
                        } else {
                            self.sounding &= !(1 << pitch);
                        }
                    }
                }
                self.notes.push(Note { offset, ..note });
            } else if is_pedal(event.status, event.data1) {
                // Delivered as a VST3 parameter change, which is the only door
                // VST3 leaves open for a CC — see `ivory-host`'s `Control`.
                // Whether it ARRIVES depends on the plugin publishing a
                // mapping; `Rendered::unmapped` is what counts the ones that
                // did not, so `pedal_dropped` now means "this instrument has no
                // pedal" rather than "we never tried".
                if self.controls.len() < self.controls.capacity() {
                    self.controls.push(Control {
                        offset,
                        controller: i16::from(event.data1),
                        value: u16::from(event.data2),
                        // The channel is NOT decoration: the modelled piano publishes a
                        // different parameter for each channel's CC64, so
                        // sending channel 0 for a keyboard transmitting on
                        // channel 3 moves the wrong piano.
                        channel: i16::from(event.status & 0x0F),
                    });
                } else {
                    self.shared.pedal_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Render `out.len() / dev_channels` frames of the device mix.
    ///
    /// `out` is interleaved at the device's channel count. `now` is the host
    /// instant the callback was entered and `heard` is the instant its first
    /// frame will actually leave the DAC.
    /// Render `frames` PROGRAM frames into `out`, and answer how many the
    /// taps hold.
    ///
    /// **Program frames, which are device frames only when the varispeed
    /// dial is at rest.** Everything in here — the transport, the clips, the
    /// plugins, the click — is generated on demand and has no opinion about
    /// how fast it is being asked for; `render` is where the two clocks meet.
    /// See `crate::varispeed`.
    fn render_program(
        &mut self,
        out: &mut [f32],
        frames: usize,
        now: Nanos,
        heard: Nanos,
    ) -> usize {
        self.shared.callbacks.fetch_add(1, Ordering::Relaxed);
        self.shared
            .delay_ns
            .store(heard.saturating_sub(now), Ordering::Relaxed);
        self.swap_plugins();

        // Every sample inside `frames` is written below, so this is only here
        // for a buffer whose length is not a whole number of frames: its tail
        // must be silence and not whatever the driver last left there.
        out.fill(0.0);

        let dev_ch = self.dev_channels;
        if dev_ch == 0 {
            // `Engine::start` refuses a zero-channel device, so this is
            // unreachable — and it is written down anyway because the next line
            // is a division, an integer division by zero PANICS in Rust, and a
            // panic across the cpal boundary is undefined behaviour rather than
            // a stack trace.
            return 0;
        }
        // Clamped, not trusted: the scratch buffers below are sized for
        // MAX_CALLBACK_FRAMES and indexing past them would be the same panic in
        // a different costume. A buffer shorter than the caller claimed gets
        // its tail as silence, which is audible and survivable.
        let frames = frames.min(out.len() / dev_ch).min(MAX_CALLBACK_FRAMES);

        // How many frames one pass through the instruments may cover. `MAX_BLOCK`
        // is the plugins' limit — `Instance::process` REFUSES more rather than
        // truncating — and the sum bus is the other half of the same bound. It
        // is sized at `Engine::start` and never resized, so this is a
        // compile-time truth written down as a runtime one: a zero here would
        // make `n` zero and the loop below would never finish, which is a hung
        // audio thread rather than a wrong sample.
        let chunk = (MAX_BLOCK as usize).min(self.mix.len() / 2);
        if chunk == 0 {
            return 0;
        }

        // Read the controls once per callback, not once per frame: they are
        // published by the UI at human speed and re-reading them 256 times
        // costs 256 atomic loads for a value that cannot have changed. Three
        // slots is three more loads per callback, not three more per frame.
        // **And the desk's switches, which decide whether a slot is heard at
        // all.** Read here with the gains, for the same reason and at the same
        // cost: they are published at human speed.
        let muted = self.shared.muted.load(Ordering::Relaxed);
        let soloed = self.shared.soloed.load(Ordering::Relaxed);
        let metro_target = Shared::f32_of(&self.shared.metro_gain).clamp(0.0, 8.0);
        let metro_on = self.shared.metro_on.load(Ordering::Relaxed);
        let in_take = self.shared.metro_in_take.load(Ordering::Relaxed);
        let count_in_recorded = self.shared.count_in_in_take.load(Ordering::Relaxed);
        let bpb = self.shared.beats_per_bar.load(Ordering::Relaxed);
        // **The maps arriving, and the displaced pair going back.** The
        // clip list's own protocol: no room to return the old one, no swap
        // this block, because dropping a `Vec` here is an allocator call on
        // the audio thread.
        if !self.maps_retiring.is_full() {
            if let Ok(next) = self.maps_incoming.pop() {
                let old = std::mem::replace(&mut self.maps, next);
                let _ = self.maps_retiring.push(old);
                // The bar lines just moved. Whatever the clock was counting
                // towards is a beat in a song that no longer exists.
                self.bar_clock.armed = false;
            }
        }
        self.beats.period = period_frames(
            self.rate,
            f64::from_bits(self.shared.bpm.load(Ordering::Relaxed)),
            self.shared.beat_unit.load(Ordering::Relaxed),
        );
        // Acquire pairs with `start_count_in`'s Release on the bump, so a new
        // generation is seen with its OWN beat count — the beats are read
        // once per generation and a stale zero here is a count-in that never
        // clicks.
        let req = self.shared.count_in_req.load(Ordering::Acquire);
        if req != self.beats.seen_req {
            self.beats.seen_req = req;
            let want = self.shared.count_in_beats.load(Ordering::Relaxed);
            self.beats.restart(want);
            self.shared
                .count_in_done
                .store(want == 0, Ordering::Relaxed);
            self.shared.beat_now.store(0, Ordering::Relaxed);
            self.shared.downbeat_ns.store(i64::MIN, Ordering::Relaxed);
            self.voice.playing = false;
        }

        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        let mut sumsq_l = 0.0f64;
        let mut sumsq_r = 0.0f64;
        let mut clipped = false;
        let mut tap_frames = 0usize;

        // **The reset, taken once per callback and before any of it.** A
        // press mid-block would otherwise drop the histogram halfway through
        // and leave the four cells already accumulated in this callback
        // counted against a measurement that was supposed to start clean.
        let reset = self.shared.lufs_reset.load(Ordering::Relaxed);
        if reset != self.lufs_seen {
            self.lufs_seen = reset;
            self.loudness.reset_integrated();
        }
        let mut lufs_moved = false;

        let mut done = 0usize;
        while done < frames {
            let n = chunk.min(frames - done);
            self.chunks += 1;

            // Each chunk gets its own window in the host timeline, so an event
            // stamped inside the second half of a 1024-frame callback lands in
            // the second `process` call rather than being held for the next
            // callback entirely.
            let block_start = now + (done as f64 / self.rate * 1e9) as Nanos;
            // ONCE, for all three slots. Draining per slot would give each event
            // to whichever slot ran first — see the module docs.
            self.collect_notes(block_start, n);

            // **The bus starts empty every block, and it has to be emptied
            // BEFORE the slots write to it.** It was cleared after them once,
            // for one commit, and the symptom was a reverb knob that did
            // nothing at all: the instruments' sends were wiped a few lines
            // after they were made, so the only thing left on the bus was the
            // click.
            if let Some(aux) = self.aux.get_mut(..n * TAP_CHANNELS) {
                aux.fill(0.0);
            }
            // **And the mix itself.** `sum_slots` used to zero it on its way
            // through; with the legacy rack gone, forgetting this line leaves
            // every block summed on top of the last one — the backing track
            // a block ago is still in the buffer, at full level, for ever.
            if let Some(mix) = self.mix.get_mut(..n * TAP_CHANNELS) {
                mix.fill(0.0);
            }
            // **The transport first, before ANY notes are consumed.** A
            // locate or a falling edge strands whatever is sounding — the
            // keys did not come up just because the timeline moved — so the
            // strands are ENDED: the built-in by its own switch, every
            // hosted voice by note-offs appended to this block's list. Run
            // BEFORE `mix_channels` so the offs land the same block, and so
            // the channels know where the timeline stands — the MIDI clips
            // play from here.
            let (at, rolling, jumped) = self.run_transport(n);
            // **Every automated control, resolved once for this chunk.** It
            // has to come after `run_transport` — the value depends on where
            // the playhead now is — and before anything that mixes, because
            // twelve sites below read it. See `resolve_automation`.
            self.resolve_automation(at);
            self.publish_latency();
            let muted = self.muted_now(muted);
            let flush = jumped || (self.was_rolling && !rolling);
            if flush && self.sounding != 0 {
                self.builtin.all_notes_off();
                for pitch in 0..128u8 {
                    if self.sounding & (1 << pitch) != 0 && self.notes.len() < MAX_EVENTS_PER_BLOCK
                    {
                        self.notes.push(ivory_host::Note {
                            offset: 0,
                            pitch: i16::from(pitch),
                            velocity: 0.0,
                            on: false,
                        });
                    }
                }
                self.sounding = 0;
            }
            self.was_rolling = rolling;
            // The stem tap's block: the flag read once, the scratch
            // zeroed once, all three mixers write, one push at the end.
            // **The routing table, read once and never again this block.**
            // The count is acquired first so a table growing under us is seen
            // whole or not at all (see `set_routes`).
            self.route_n = self
                .shared
                .route_count
                .load(Ordering::Acquire)
                .min(MAX_ROUTES);
            self.routes_hw = false;
            let mut kept = 0usize;
            for i in 0..self.route_n {
                let Some((from, target, hardware, stereo)) =
                    unpack_route(self.shared.routes[i].load(Ordering::Relaxed))
                else {
                    continue;
                };
                // A route naming a channel this build does not have is
                // dropped here rather than indexed with: the desk can shrink
                // between the write and the read.
                if from >= CHANNELS || (!hardware && target >= CHANNELS) {
                    continue;
                }
                let gain = f32::from_bits(self.shared.route_gain[i].load(Ordering::Relaxed));
                let gain = if gain.is_finite() {
                    gain.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.route_cache[kept] = (from, target, hardware, stereo, gain);
                self.routes_hw |= hardware;
                kept += 1;
            }
            self.route_n = kept;
            // **Which channels have stopped going straight to the master.**
            // Read off the table that was just latched, so it answers for
            // exactly the routes this block will run — and recomputed every
            // block, so pulling the last route off a channel puts it back
            // rather than leaving it silent until the next restart.
            self.route_direct = [1.0; CHANNELS];
            for i in 0..self.route_n {
                let (from, _, hardware, _, _) = self.route_cache[i];
                if !hardware {
                    if let Some(d) = self.route_direct.get_mut(from) {
                        *d = 0.0;
                    }
                }
            }
            // Sends are accumulated per destination and must start empty.
            if self.route_n > 0 {
                if let Some(sa) = self.send_accum.get_mut(..CHANNELS * n * TAP_CHANNELS) {
                    sa.fill(0.0);
                }
            }
            self.stems_live = self.shared.stems_on.load(Ordering::Relaxed);
            // **Hardware routes need the same per-channel audio stems need.**
            // One buffer, two customers: whichever asks, it is filled and
            // zeroed exactly once.
            if self.stems_live || self.route_n > 0 {
                if let Some(st) = self.stem_scratch.get_mut(..n * CHANNELS * 2) {
                    st.fill(0.0);
                }
            }
            self.mix_channels(n, muted, soloed, at, rolling, flush);

            // A parameter change, if one is waiting and the lock is free this
            // block. Same shape as the DX7's pending voice, and for the same
            // reason: the audio thread waits for nothing.
            if self.shared.params_dirty.load(Ordering::Acquire) {
                if let Ok(g) = self.shared.params.try_lock() {
                    self.effect_params = *g;
                    self.shared.params_dirty.store(false, Ordering::Release);
                }
            }

            // ── the desk ───────────────────────────────────────────────────
            //
            // **The effects used to be an insert on the instrument bus** and
            // everything else joined downstream of them, which is why nothing
            // but the instrument could ever be reverberated and why the
            // limiter never saw the backing track it was supposed to be
            // protecting the output from.
            //
            // They are two things now. Reverb, delay and chorus are a BUS: a
            // strip sends a percentage of itself to them and what comes back
            // is added to the mix. The high-pass, the low-pass and the limiter
            // are an INSERT on the master, across everything on its way out.
            // Only three of the six were ever wet amounts; the other three are
            // a corner frequency, a corner frequency and a threshold, and a
            // send knob into those would not be a question anybody could
            // answer.
            //
            // The defaults are the old routing exactly — instrument sends
            // everything, nothing else sends anything — so an upgrade with the
            // mixer untouched sounds the same.
            let bpm = f64::from_bits(self.shared.bpm.load(Ordering::Relaxed));
            let level = |strip: Strip, gain: &AtomicU32| {
                if strip_is_heard(strip, muted, soloed) {
                    Shared::f32_of(gain).clamp(0.0, 8.0)
                } else {
                    0.0
                }
            };
            // Read here rather than through a closure: a closure over
            // `self.shared` borrows the whole of `self`, and the mixing below
            // needs `self` mutably.
            let send_of =
                |sh: &Shared, strip: Strip| Shared::f32_of(&sh.send[strip.index()]).clamp(0.0, 1.0);

            // **The slots did their own folding.** Each instrument is a channel
            // with its own fader, its own send and its own mute — see
            // `sum_slots`, which is where the summing and the sending are one
            // pass because they are one signal read twice.
            let coeff = self.gain_coeff;

            // The BACKING TRACK and the INPUT, each adding itself to the mix
            // and to the bus in one pass. Both were downstream of the effects
            // before and reached neither.
            //
            self.mix_clips(at, rolling, n, muted, soloed);
            self.mix_monitor(n, muted, soloed);
            self.mix_preview(n);
            // The drumpad, upstream of the bus return, the room split and
            // the master — so the pads are limited, mastered, heard and
            // taped exactly like every other source.
            self.mix_pads(n, muted, soloed);
            if self.stems_live {
                let want = n * CHANNELS * 2;
                let room = (self.stem_tx.slots() / (CHANNELS * 2)) * (CHANNELS * 2);
                let take = want.min(room);
                if take > 0 {
                    let _ = self.stem_tx.push_entire_slice(&self.stem_scratch[..take]);
                }
            }
            // ── the click, and what it sends ───────────────────────────
            //
            // **Its own pass, because the bus has to be finished before the
            // master is.** The click is generated per frame, in step with the
            // beat clock, and it now has a send like every other strip — so it
            // has to reach the bus BEFORE the bus is processed, while the tap
            // and the device it also feeds are written AFTER the master is.
            // One loop cannot be on both sides of that.
            // `metro_target` was read at the top of the block with the rest of
            // the metronome's state; the strip only decides whether it is heard.
            let click_level = if strip_is_heard(Strip::Click, muted, soloed) {
                metro_target
            } else {
                0.0
            };
            let click_send = send_of(&self.shared, Strip::Click);
            let mut click_peak = 0.0f32;
            for i in 0..n {
                let frame_index = done + i;

                let counting_in = self.count_in_tail > 0;
                self.count_in_tail = self.count_in_tail.saturating_sub(1);
                // **The click only speaks while the transport does** — armed
                // by M, HEARD under play or record (the count-in rides its
                // own gate inside `tick`). A practice click that started at
                // the keypress was the owner's complaint.
                // **Two clocks, and which one speaks is the whole point.**
                //
                // A count-in belongs to the KEYPRESS: it is a countdown, it
                // starts when you press record, and `Beats` counts it out
                // from there — which is also what makes the countdown on
                // screen and the click in the room the same event, the
                // property `Beats`'s own header explains at length.
                //
                // Everything else belongs to the SONG. `BarClock` derives
                // the beat from where the transport actually is, so pressing
                // play at bar 9 puts the accent on bar 9's line instead of
                // wherever playback happened to begin, and a tempo map —
                // which has no single period to accumulate — works at all.
                let heard_beat = if self.beats.counting_in() || !rolling {
                    self.beats.tick(metro_on && rolling, bpb)
                } else if metro_on {
                    self.bar_clock.tick(at + frame_index as u64, &self.maps, self.rate)
                } else {
                    // Not heard, but still followed: leaving the clock
                    // un-ticked would make the next frame look like a locate
                    // and re-derive, which is work in the callback for
                    // nothing. Ticking it and dropping the beat keeps it in
                    // step for the moment the click comes back on.
                    self.bar_clock.tick(at + frame_index as u64, &self.maps, self.rate);
                    None
                };
                if let Some(beat) = heard_beat {
                    self.voice.trigger(beat.accent);
                    if beat.count_in > 0 || beat.downbeat {
                        // A whole beat, so the sample has room to decay, and
                        // reset on the downbeat too — that click is the "go"
                        // and the one anybody trimming to the count will look
                        // for.
                        self.count_in_tail = self.beats.period.max(1.0) as u64;
                    }
                    self.shared.beat_now.store(beat.count_in, Ordering::Relaxed);
                    if beat.downbeat {
                        let at = heard + (frame_index as f64 / self.rate * 1e9) as Nanos;
                        self.shared.downbeat_ns.store(at, Ordering::Relaxed);
                        self.shared.count_in_done.store(true, Ordering::Relaxed);
                    }
                }

                self.metro_gain += (click_level - self.metro_gain) * self.gain_coeff;
                let click = self.voice.next(&self.click) * self.metro_gain;
                self.click_out[i] = click;
                // The click reaches the FILE when the performance is meant to
                // carry it, or while a count-in that was asked to be in the
                // take is still sounding. `counting_in` rather than a beat
                // number, because the click is a decaying sample: the sound of
                // beat four is still going during beat five and has to be
                // recorded for as long as it lasts.
                //
                // Kept as a second BUFFER rather than a flag, so the pass that
                // writes the take does not have to re-derive a per-frame
                // condition it can no longer see.
                self.click_taped[i] = if in_take || (count_in_recorded && counting_in) {
                    click
                } else {
                    0.0
                };
                click_peak = click_peak.max(click.abs());
                let at = i * TAP_CHANNELS;
                for c in 0..TAP_CHANNELS {
                    if let Some(v) = self.aux.get_mut(at + c) {
                        *v += click * click_send;
                    }
                }
            }
            // **The click's rack, run here in strip order** — the same
            // window every other channel's runs in — over the buffer that
            // both destinations read. The SUMMING stays post-master (see the
            // fold below), which is the cue-bus rule: the click is processed
            // with the channels and added after the ceiling, so it can never
            // push a take into the limiter and never be ducked by one.
            // Bit-identical for an empty rack, which is every desk until
            // somebody fills a bay.
            if !self.racks[Strip::Click.index()].is_empty() {
                for c in 0..TAP_CHANNELS {
                    if let Some(dst) = self.fx_in.get_mut(c) {
                        for (f, d) in dst.iter_mut().take(n).enumerate() {
                            *d = self.click_out[f];
                        }
                    }
                }
                {
                    let armed = self.shared.midi_off.load(Ordering::Relaxed)
                        & (1 << Strip::Click.index().min(31))
                        == 0;
                    let notes: &[ivory_host::Note] = if armed { &self.notes } else { &[] };
                    let controls: &[ivory_host::Control] = if armed { &self.controls } else { &[] };
                    run_rack(
                        &mut self.racks[Strip::Click.index()],
                        &mut self.fx_in,
                        n,
                        &mut self.fx_out,
                        notes,
                        controls,
                    );
                }
                // Back into BOTH buffers, and the take's keeps its gate: a
                // frame the take was not carrying stays absent however the
                // rack coloured it.
                click_peak = 0.0;
                for i in 0..n {
                    let wet = self.fx_in.first().map_or(0.0, |b| b[i]);
                    let gate = if self.click_taped[i] != 0.0 { 1.0 } else { 0.0 };
                    self.click_out[i] = wet;
                    self.click_taped[i] = wet * gate;
                    click_peak = click_peak.max(wet.abs());
                }
            }
            // The click is one voice on both sides.
            self.shared.note_strip_peak(Strip::Click, [click_peak; 2]);

            // ── channel-to-channel sends ───────────────────────────────────
            //
            // **A destination's rack processes what is sent to it, exactly as
            // the drum bus processes its summed kit.** `run_rack` with an
            // EMPTY note list is an effects pass: a reverb on channel 9 wets
            // what arrives, and a synth on channel 9 adds nothing, because a
            // synth has no opinion about audio it is handed. That is what
            // makes "put Pro-R on 9 and send 3 to 9" mean what it says.
            //
            // **A CHAIN WORKS, in one block, because the destinations are
            // processed in dependency order.**
            //
            // They used to run in index order with every gather done up
            // front, which meant the second hop of A->B->C read a stem B had
            // not written yet: the chain silently did nothing, C's rack and
            // fader never touched the audio, and A arrived at the master
            // through B regardless. Every desk worth comparing to — REAPER,
            // Ardour, Live, Pro Tools — treats a multi-hop chain as ordinary.
            //
            // So each destination is finished BEFORE anything it feeds
            // gathers, and each one gathers at the moment it runs rather than
            // in a pass beforehand.
            //
            // **A CYCLE keeps the old one-hop behaviour, and that is the
            // safety rule.** Anything left unordered gathers whatever the
            // stem holds at that moment, so A->B->A carries a block of delay
            // instead of feeding itself inside one callback. That is a better
            // answer than the two competitors that allow cycles at all:
            // REAPER gives up delay compensation across the whole project for
            // it, and Live cannot latency-compensate the path either.
            if self.route_n > 0 {
                // **Who feeds whom.** `receives` is every channel a send
                // lands on; `deps` is, per destination, the sources feeding
                // it that are themselves destinations — the only ones worth
                // waiting for, because everything else is already in the stem.
                let mut receives: u32 = 0;
                for i in 0..self.route_n {
                    let (_, target, hardware, _, gain) = self.route_cache[i];
                    if !hardware && gain > 0.0 && target < CHANNELS {
                        receives |= 1 << target;
                    }
                }
                let mut deps = [0u32; CHANNELS];
                for i in 0..self.route_n {
                    let (from, target, hardware, _, gain) = self.route_cache[i];
                    if hardware || gain <= 0.0 || target >= CHANNELS || from >= CHANNELS {
                        continue;
                    }
                    if receives & (1 << from) != 0 {
                        deps[target] |= 1 << from;
                    }
                }
                // Kahn's, over bitmasks: at most CHANNELS passes, each a
                // handful of bit tests. No allocation, and nothing runs at
                // all on a desk with no routes.
                let mut order = [0usize; CHANNELS];
                let mut n_order = 0usize;
                let mut placed: u32 = 0;
                for _ in 0..CHANNELS {
                    let mut moved = false;
                    for dest in 0..CHANNELS {
                        let bit = 1u32 << dest;
                        if receives & bit == 0 || placed & bit != 0 {
                            continue;
                        }
                        if deps[dest] & !placed == 0 {
                            order[n_order] = dest;
                            n_order += 1;
                            placed |= bit;
                            moved = true;
                        }
                    }
                    if !moved {
                        break;
                    }
                }
                // Whatever is left is in a cycle (a self-route counts): it
                // runs last, one hop, exactly as before.
                for dest in 0..CHANNELS {
                    let bit = 1u32 << dest;
                    if receives & bit != 0 && placed & bit == 0 {
                        order[n_order] = dest;
                        n_order += 1;
                    }
                }

                for oi in 0..n_order {
                    let dest = order[oi];
                    let base = dest * n * TAP_CHANNELS;
                    // Gather THIS destination's inputs now — so a bus that
                    // has already run is picked up whole, its rack and fader
                    // included, out of the stem it just wrote.
                    for i in 0..self.route_n {
                        let (from, target, hardware, _, gain) = self.route_cache[i];
                        if hardware || gain <= 0.0 || target != dest {
                            continue;
                        }
                        for f in 0..n {
                            let (Some(l), Some(r)) = (
                                self.stem_scratch.get(f * CHANNELS * 2 + from * 2).copied(),
                                self.stem_scratch
                                    .get(f * CHANNELS * 2 + from * 2 + 1)
                                    .copied(),
                            ) else {
                                break;
                            };
                            let at = base + f * TAP_CHANNELS;
                            if let Some(d) = self.send_accum.get_mut(at) {
                                *d += l * gain;
                            }
                            if let Some(d) = self.send_accum.get_mut(at + 1) {
                                *d += r * gain;
                            }
                        }
                    }
                    // Then through its own rack and into the master at that
                    // channel's own fader — the destination's fader rides the
                    // send, which is what a bus is.
                    let live = self
                        .send_accum
                        .get(base..base + n * TAP_CHANNELS)
                        .is_some_and(|b| b.iter().any(|v| *v != 0.0));
                    if !live {
                        continue;
                    }
                    for c in 0..TAP_CHANNELS {
                        let Some(dst) = self.send_io.get_mut(c) else {
                            break;
                        };
                        for f in 0..n {
                            dst[f] = self
                                .send_accum
                                .get(base + f * TAP_CHANNELS + c)
                                .copied()
                                .unwrap_or(0.0);
                        }
                    }
                    if !self.racks[dest].is_empty() {
                        run_rack(
                            &mut self.racks[dest],
                            &mut self.send_io,
                            n,
                            &mut self.fx_out,
                            &[],
                            &[],
                        );
                    }
                    let g = self.lane_gain[dest];
                    let (pl, pr) = pan_gains(self.lane_pan(dest));
                    // **A bus that is itself routed onward does not ALSO go
                    // to the master.** What it passes goes into its stem
                    // below, which is where the destination downstream of it
                    // gathers from. Without this the middle of a chain was
                    // heard twice over: once bypassing everything after it,
                    // and once properly through the end of the chain.
                    let direct = self.route_direct[dest];
                    for f in 0..n {
                        let l = self.send_io[0][f] * g * pl;
                        let r = self.send_io.get(1).map_or(l, |ch| ch[f] * g * pr);
                        if let Some(v) = self.mix.get_mut(f * TAP_CHANNELS) {
                            *v += l * direct;
                        }
                        if let Some(v) = self.mix.get_mut(f * TAP_CHANNELS + 1) {
                            *v += r * direct;
                        }
                        // The destination's METER shows what it is passing,
                        // and its hardware route carries it too: both read
                        // the stem, so both are told by writing it here.
                        if let Some(v) = self.stem_scratch.get_mut(f * CHANNELS * 2 + dest * 2) {
                            *v += l;
                        }
                        if let Some(v) = self.stem_scratch.get_mut(f * CHANNELS * 2 + dest * 2 + 1)
                        {
                            *v += r;
                        }
                    }
                }
            }

            // ── the bus, and then the master ───────────────────────────────
            let sends = crate::effects::Sends {
                reverb: Shared::f32_of(&self.shared.reverb_mix),
                delay: Shared::f32_of(&self.shared.delay_mix),
                chorus: Shared::f32_of(&self.shared.chorus_mix),
                phaser: Shared::f32_of(&self.shared.phaser_mix),
                // Not on the bus. See the comment above the desk — and note
                // that the limiter's AND the low-pass's "off" is the TOP of
                // their travel, so those are 1.0 where the others use 0.0.
                hpf: 0.0,
                lpf: 1.0,
                gain: 0.0,
                limiter: 1.0,
            };
            if let Some(aux) = self.aux.get_mut(..n * TAP_CHANNELS) {
                self.effects
                    .process(aux, n, TAP_CHANNELS, sends, &self.effect_params, bpm);
            }
            // **And then whatever the user put across the bus.** After the
            // built-in three, so somebody who loads a reverb and leaves the
            // knobs alone hears their reverb rather than theirs through ours.
            self.run_insert_chain(Strip::Fx.index(), true, n);
            // What comes back, at the bus's own fader.
            let return_target = level(Strip::Fx, &self.shared.fx_return);
            let fx_peak = add_return(
                &mut self.mix,
                &self.aux,
                n,
                &mut self.fx_return_gain,
                return_target,
                coeff,
            );
            self.shared.note_strip_peak(Strip::Fx, fx_peak);

            let master_sends = crate::effects::Sends {
                reverb: 0.0,
                delay: 0.0,
                chorus: 0.0,
                phaser: 0.0,
                hpf: Shared::f32_of(&self.shared.hpf_mix),
                lpf: Shared::f32_of(&self.shared.lpf_mix),
                gain: Shared::f32_of(&self.shared.gain_mix),
                limiter: Shared::f32_of(&self.shared.limiter_mix),
            };
            // **The room's mix is made here, one subtraction, while the sum
            // is still a sum.** Everything upstream of the master insert adds,
            // so the difference between the file's mix and the speakers' is
            // exactly the input that monitoring is withholding — and `mix_in`
            // wrote that down frame by frame as it made it.
            //
            // After the insert it could not be done at all: a limiter is not
            // linear, and there is no arithmetic that takes one voice back out
            // of what it decided.
            let split = self.room_live;
            if split {
                let end = n * TAP_CHANNELS;
                if let (Some(room), Some(mix), Some(dry)) = (
                    self.room.get_mut(..end),
                    self.mix.get(..end),
                    self.input_dry.get(..end),
                ) {
                    for i in 0..end {
                        room[i] = mix[i] - dry[i];
                    }
                }
            }
            // **The master's own rack, before the master's built-in insert.**
            // The limiter stays last on the way out, which is what a limiter
            // is for: an effect after it would undo the ceiling it just made.
            // **The master's SECOND rack, first.** Its three bays are drawn
            // above the original three on the strip, and a rack runs top to
            // bottom — so they run before them. That order is also what
            // keeps an existing session sounding the same: its plugins are
            // still in the seat they were in, in the same place in the
            // chain, with three empty bays added in front of them (and an
            // empty rack returns immediately).
            self.run_insert_chain(ivory_ui::recorder::MASTER_EXTRA_SEAT, false, n);
            self.run_insert_chain(STRIPS, false, n);
            let master = Shared::f32_of(&self.shared.master_gain);
            let mono = self.shared.master_mono.load(Ordering::Relaxed);
            if let Some(mix) = self.mix.get_mut(..n * TAP_CHANNELS) {
                self.master_effects.process(
                    mix,
                    n,
                    TAP_CHANNELS,
                    master_sends,
                    &self.effect_params,
                    bpm,
                );
                // **The master, last on the bus and after the limiter.** A
                // master that fed the limiter would be a second drive control;
                // this one is what leaves, which is what a master fader is.
                if (master - 1.0).abs() > 1.0e-6 {
                    for v in mix.iter_mut() {
                        *v *= master;
                    }
                }
                fold_mono(mix, mono);
            }
            // The speakers' own insert, the same settings, its own state —
            // and only while the two mixes differ. Idle it costs nothing; a
            // rig with no input device never reaches it at all.
            if split {
                if let Some(room) = self.room.get_mut(..n * TAP_CHANNELS) {
                    self.room_effects.process(
                        room,
                        n,
                        TAP_CHANNELS,
                        master_sends,
                        &self.effect_params,
                        bpm,
                    );
                    if (master - 1.0).abs() > 1.0e-6 {
                        for v in room.iter_mut() {
                            *v *= master;
                        }
                    }
                    fold_mono(room, mono);
                }
            }
            // How hard the limiter worked, for the meter beside it. Kept as
            // the WORST since the UI last looked: it asks sixty times a second
            // and the moment worth showing is a few samples long.
            let gr = self.master_effects.gain_reduction_db();
            if gr > 0.0 {
                self.shared.gr_db.fetch_max(gr.to_bits(), Ordering::Relaxed);
            }

            // ── out, to the take and to the speakers ───────────────────────
            //
            // Two mixes when monitoring is withholding an input, one when it
            // is not. The click has always been two already, at two levels —
            // `click_taped` and `click_out` — for the same reason: what is
            // worth hearing and what is worth keeping are not always the same
            // signal.
            for i in 0..n {
                let frame_index = done + i;
                let at2 = i * TAP_CHANNELS;
                let take = &self.mix[at2..at2 + TAP_CHANNELS];
                // **The loudness meter reads the TAKE'S mix, post-master.**
                //
                // It is a readout about the FILE — what a streaming service
                // will normalise, what the export will measure at — so it
                // has to be the programme that gets written down. That means
                // after the master's inserts, after the limiter, after the
                // master fader and after the mono fold, which is exactly
                // where `self.mix` stands by this point.
                //
                // And `self.mix` rather than the pair below: when monitoring
                // is withholding an input the speakers get `self.room`, and
                // measuring that would report a number for a mix nobody is
                // exporting. The click is not in it either — it joins the
                // tape separately, at its own level — which is right for the
                // same reason.
                lufs_moved |= self.loudness.push(take[0], take[1]);

                map_frame(take, &mut self.frame[..TAP_CHANNELS]);
                let tap_at = tap_frames * TAP_CHANNELS;
                for c in 0..TAP_CHANNELS {
                    let s = self.frame[c] + self.click_taped[i];
                    self.tap_scratch[tap_at + c] = s;
                }
                tap_frames += 1;

                let src = if split {
                    &self.room[at2..at2 + TAP_CHANNELS]
                } else {
                    &self.mix[at2..at2 + TAP_CHANNELS]
                };
                // The scope reads the speakers' pair BEFORE the click joins
                // — minus-the-click falls out of the tap point, no
                // subtraction anywhere.
                let scope_at = (tap_frames - 1) * TAP_CHANNELS;
                self.scope_scratch[scope_at] = src[0];
                self.scope_scratch[scope_at + 1] = src.get(1).copied().unwrap_or(src[0]);
                // ReaStream sends what leaves — the scope's pair, packed
                // contiguously so the block is L,R,L,R for the sender to plane.
                let ra_at = (tap_frames - 1) * 2;
                self.reastream_scratch[ra_at] = src[0];
                self.reastream_scratch[ra_at + 1] = src.get(1).copied().unwrap_or(src[0]);
                let at = frame_index * dev_ch;
                map_frame(src, &mut self.frame[..dev_ch]);
                for c in 0..dev_ch {
                    // **The click and the preview live on the MAIN PAIR
                    // only** — the same two channels `map_frame` gives the
                    // mix; everything past them is zero-filled. Adding them
                    // on every channel lit all twelve outputs of the
                    // tester's Babyface at once — phones, ADAT, the lot —
                    // which is where "deafeningly loud" auditions came from.
                    let cue = if c < 2 {
                        self.click_out[i] + self.preview_out[i * 2 + c]
                    } else {
                        0.0
                    };
                    let s = self.frame[c] + cue;
                    out[at + c] = s;
                    // **The meter and the clip lamp read the PROGRAM, not
                    // the cue.** The click and the audition join AFTER the
                    // limiter and the master on purpose — which meant a
                    // click atop a correctly limited mix lit the clip lamp
                    // and the owner rightly asked why the limiter "was not
                    // working". The lamp now answers for the mix the
                    // limiter actually guarantees; the cue rides above it,
                    // unmetered, on its own road.
                    let mag = self.frame[c].abs();
                    if mag >= CLIP_LEVEL {
                        clipped = true;
                    }
                    if c == 0 {
                        peak_l = peak_l.max(mag);
                        sumsq_l += f64::from(mag) * f64::from(mag);
                    } else if c == 1 {
                        peak_r = peak_r.max(mag);
                        sumsq_r += f64::from(mag) * f64::from(mag);
                    }
                }
                // **Hardware routes: each routed channel onto its own jacks.**
                //
                // ADDED, never assigned: a jack can carry more than one
                // routed channel (two players into one headphone amp), and
                // the master pair is already written above — routing a
                // channel to 1/2 puts it there TWICE on purpose, because a
                // route does not take a channel off the master. Bounds are
                // checked against the device rather than trusted: the table
                // is written by the UI and the interface can change under it.
                if self.routes_hw {
                    for ri in 0..self.route_n {
                        let (from, first, hardware, stereo, gain) = self.route_cache[ri];
                        if !hardware || gain <= 0.0 {
                            continue;
                        }
                        let lane_at = i * CHANNELS * 2 + from * 2;
                        let (Some(l), Some(r)) = (
                            self.stem_scratch.get(lane_at).copied(),
                            self.stem_scratch.get(lane_at + 1).copied(),
                        ) else {
                            continue;
                        };
                        if stereo {
                            if first + 1 < dev_ch {
                                out[at + first] += l * gain;
                                out[at + first + 1] += r * gain;
                            }
                        } else if first < dev_ch {
                            // A mono jack SUMS the pair. Taking the left
                            // alone would half-vanish anything panned right.
                            out[at + first] += (l + r) * gain;
                        }
                    }
                }
            }
            done += n;
        }

        // **The taps are pushed by the CALLER**, not here: under varispeed
        // they hold program frames and have to be squeezed to the device's
        // rate first, or the take would be a recording of a tape running at
        // the wrong speed. See `render`.
        //
        // Ten times a second at most, which is when a cell closed and the
        // readings actually moved. See `Loudness::push`.
        if lufs_moved {
            let (m, st, i) = self.loudness.read();
            let [ml, mr, sl, sr] = self.loudness.read_stereo();
            for (cell, v) in self.shared.lufs.iter().zip([m, st, i, ml, mr, sl, sr]) {
                cell.store(v.to_bits(), Ordering::Relaxed);
            }
        }
        // **What the timeline is playing, lane by lane.** Rebuilt from
        // `active_clip`, which is the authority on what is sounding from a
        // clip and already carries the lane; deriving it here costs one pass
        // over a list that is at most a few dozen long and saves keeping a
        // second copy in step.
        {
            let mut mask = [0u64; CHANNELS * 2];
            for a in &self.active_clip {
                let lane = usize::from(a.lane);
                if lane >= CHANNELS || a.key >= 128 {
                    continue;
                }
                mask[lane * 2 + usize::from(a.key >= 64)] |= 1u64 << (a.key % 64);
            }
            for (cell, v) in self.shared.lane_sounding.iter().zip(mask) {
                cell.store(v, Ordering::Relaxed);
            }
        }
        self.publish_meters(frames, peak_l, peak_r, sumsq_l, sumsq_r, clipped);
        tap_frames
    }

    /// One device callback.
    ///
    /// **Where the two clocks meet.** At rest they are the same clock and
    /// this costs one atomic load; engaged, the program is rendered at
    /// `frames × ratio` and everything that leaves — the speakers, the take,
    /// the scope, the ReaStream send — is resampled back to the device's rate
    /// together, from one shared phase, so they are all the same instant.
    fn render(&mut self, out: &mut [f32], now: Nanos, heard: Nanos) {
        let dev_ch = self.dev_channels;
        if dev_ch == 0 {
            return;
        }
        let dev_frames = (out.len() / dev_ch).min(MAX_CALLBACK_FRAMES);
        let Some(ratio) = self.varispeed_ratio() else {
            if self.vs_was_engaged {
                // **Dropped on the way OUT, not on the way in.** The
                // histories hold the last few frames of a signal that was
                // running at a different rate; carried into the next
                // engagement they would be spliced onto it, minutes or hours
                // later, as a click.
                self.clear_varispeed();
            }
            self.vs_was_engaged = false;
            let tap_frames = self.render_program(out, dev_frames, now, heard);
            self.push_taps(tap_frames);
            return;
        };
        if !self.vs_was_engaged {
            self.clear_varispeed();
        }
        self.vs_was_engaged = true;
        // How much program this callback needs, bounded by what the
        // program-rate scratches were sized for. The caller keeps the device
        // side small enough that the cap is never the binding one — see
        // `Renderer::max_device_frames`.
        let program = self
            .vs_phase
            .program_frames(dev_frames, ratio)
            .min(MAX_CALLBACK_FRAMES);
        if program == 0 {
            out.fill(0.0);
            return;
        }
        let frac = self.vs_phase.frac();
        // Out of `self` so the render below can borrow the rest of it. A
        // move, not an allocation.
        let mut dev = std::mem::take(&mut self.vs_dev);
        let need = program * dev_ch;
        if dev.len() < need {
            // Unreachable — it is sized for `MAX_CALLBACK_FRAMES * widest` at
            // construction — and written down because the alternative to
            // noticing is a panic on the audio thread.
            self.vs_dev = dev;
            out.fill(0.0);
            return;
        }
        let tap_frames = self.render_program(&mut dev[..need], program, now, heard);
        out.fill(0.0);
        self.vs_out.run(&dev, program, out, dev_frames, frac, ratio);
        self.vs_dev = dev;
        // The three taps, from the SAME phase and the same ratio: a take a
        // fraction of a frame away from what came out of the speakers is a
        // take that will not line up with anything.
        let mut dst = std::mem::take(&mut self.vs_dst);
        for (stream, src, ch) in [
            (&mut self.vs_tap, &mut self.tap_scratch, TAP_CHANNELS),
            (&mut self.vs_scope, &mut self.scope_scratch, TAP_CHANNELS),
            (&mut self.vs_reastream, &mut self.reastream_scratch, 2),
        ] {
            let n = dev_frames * ch;
            if dst.len() < n || src.len() < n {
                continue;
            }
            stream.run(src, tap_frames, &mut dst, dev_frames, frac, ratio);
            src[..n].copy_from_slice(&dst[..n]);
        }
        self.vs_dst = dst;
        self.vs_phase.advance(dev_frames, program, ratio);
        self.push_taps(dev_frames);
    }

    /// Hand the take, the scope and the ReaStream send what this callback
    /// made. `frames` is in DEVICE frames — see `render`.
    fn push_taps(&mut self, frames: usize) {
        self.push_tap(frames);
        let _ = self
            .scope_tx
            .push_entire_slice(&self.scope_scratch[..frames * TAP_CHANNELS]);
        // Only while streaming, and a full ring drops the block like the
        // scope's does — a stalled receiver must never back up into the audio
        // thread.
        if self.shared.reastream_on.load(Ordering::Relaxed) {
            let _ = self
                .reastream_tx
                .push_entire_slice(&self.reastream_scratch[..frames * 2]);
        }
    }

    /// The largest device callback this renderer can serve in one pass.
    ///
    /// **Half the usual, while the varispeed is engaged.** The program-rate
    /// scratches are sized for `MAX_CALLBACK_FRAMES`, and at the dial's top
    /// speed one device frame costs two program frames — so the device side
    /// has to be halved or the mix buffer is the thing that runs out. A few
    /// frames of slack for the interpolator's reach past the last read.
    fn max_callback_frames(&self) -> usize {
        match self.varispeed_ratio() {
            None => MAX_CALLBACK_FRAMES,
            Some(_) => (MAX_CALLBACK_FRAMES - 8) / 2,
        }
    }

    /// The varispeed ratio, or `None` at rest.
    ///
    /// Sanitised HERE, in one place, so no caller can be handed a NaN or a
    /// speed outside the dial's own limits — a NaN read position resamples to
    /// silence at best and indexes wildly at worst.
    fn varispeed_ratio(&self) -> Option<f64> {
        let r = f64::from(Shared::f32_of(&self.shared.varispeed));
        if !r.is_finite() || !(0.5..=crate::varispeed::MAX_RATIO).contains(&r) {
            return None;
        }
        ((r - 1.0).abs() > 1.0e-9).then_some(r)
    }

    /// Every varispeed history and both phases back to nothing.
    fn clear_varispeed(&mut self) {
        self.vs_phase.reset();
        self.vs_in_phase.reset();
        for s in [
            &mut self.vs_out,
            &mut self.vs_tap,
            &mut self.vs_scope,
            &mut self.vs_reastream,
            &mut self.vs_in,
        ] {
            s.clear();
        }
    }

    /// Render one chunk from **every** loaded slot and return how many channels
    /// each one wrote.
    ///
    /// Zero means silence, and that is the normal state of a slot nobody has
    /// filled. A block a plugin refuses latches that slot's fault and returns
    /// zero for it rather than leaving the previous block's samples in the
    /// buffers, which would loop the last 10 ms forever at full level — and
    /// leaves the other two rendering, because one instrument giving up is not a
    /// reason to stop the piano.
    ///
    /// Every slot is handed the *same* `&self.notes` and `&self.controls`. That
    /// is the layering invariant in one line, and it only holds because the
    /// queue was drained into those two lists before this was called.
    /// The built-in instrument, over the bus, when nothing else is playing it.
    ///
    /// **Only when every slot is empty.** It is a fallback, not a layer: a
    /// piano underneath somebody's own instrument would be a bug they could
    /// not switch off, and the moment they load one the app has the sound they
    /// chose. `widths` is what actually rendered this block, which is the
    /// honest test — a slot holding a faulted plugin is an empty slot as far as
    /// the bus is concerned.
    /// Add the live input to the bus.
    ///
    /// **The ring is drained every block whether or not anybody is
    /// listening.** Left alone it would fill within a second of the device
    /// opening and stay full, so switching monitoring on would play a second of
    /// stale audio before catching up — and the input's callback would be
    /// pushing into a ring that never had room, which is a drop counter ticking
    /// for no reason. Drained-and-discarded is the same discipline the take's
    /// instrument ring already follows between takes.
    ///
    /// **It reaches the take, and that is the point.** This adds the input to
    /// `self.mix`, and `self.mix` is what the tap carries — so the microphone
    /// arrives in the file with its fader, its send and the master's limiter
    /// already on it, which is what a desk is. The owner's rule, in one line:
    /// if the effects were audible while it was recorded, they are in the
    /// take.
    ///
    /// It reaches the SPEAKERS only when somebody asked to hear it, and the
    /// two used to be the same switch. See the gate below.
    fn mix_monitor(&mut self, frames: usize, muted: u32, soloed: u32) {
        // Cleared first, so every path out of here that adds nothing to the
        // mix also leaves the room equal to the take.
        self.room_live = false;
        // A ring arriving, or being taken away. `try_lock`, never `lock`.
        // **A layout change for the ring already held.** Read before the
        // ring handoff so a frame where both arrive takes the new ring's own
        // layout rather than being overwritten by an older one.
        if let Ok(mut layout) = self.shared.pending_monitor_layout.try_lock() {
            if let Some((widths, picks)) = layout.take() {
                if self.monitor.is_some() {
                    self.monitor_widths = widths.map(usize::from);
                    self.monitor_picks = picks;
                }
            }
        }
        if let Ok(mut pending) = self.shared.pending_monitor.try_lock() {
            if let Some(next) = pending.take() {
                match next {
                    Some((ring, channels, widths, block, picks)) => {
                        self.monitor = Some(ring);
                        self.monitor_channels = usize::from(channels).max(1);
                        self.monitor_widths = widths.map(usize::from);
                        self.monitor_picks = picks;
                        self.monitor_block = block.max(1) as usize;
                    }
                    None => {
                        self.monitor = None;
                        self.monitor_channels = 0;
                        self.monitor_widths = [0; INPUTS];
                        self.monitor_picks = ivory_record::audio::Picks::EVERYTHING;
                        self.monitor_block = 0;
                    }
                }
                // A new device fades in from silence rather than arriving at
                // whatever the last one was playing at.
                self.monitor_gain = [0.0; INPUTS];
            }
        }
        // Read before the ring is borrowed: `varispeed_ratio` takes `&self`
        // and the ring is a `&mut` into the same struct.
        let vs = self.varispeed_ratio();
        let Some(ring) = self.monitor.as_mut() else {
            return;
        };
        let ch_in = self.monitor_channels.max(1);
        // **How much REAL time this chunk of program covers.**
        //
        // At rest they are the same number. Engaged, they are not, and this
        // is the half of varispeed that is easy to forget: the program is
        // being rendered faster than real time, but the interface still
        // delivers exactly one second of audio per second. A desk that read
        // the input at program rate would ask for fifteen percent more than
        // will ever arrive, every callback, and the monitor path would run
        // dry — a gap in the take fifteen percent of the way through every
        // block, for ever.
        //
        // So the ring is read at REAL rate and stretched to fill the chunk,
        // which also means the two resamplings cancel: a player hears
        // themselves unshifted while the tape runs fast, which is what
        // monitoring through a varispeed'd machine does.
        let (real_frames, step) = match vs {
            Some(ratio) => {
                let step = 1.0 / ratio;
                (self.vs_in_phase.program_frames(frames, step), step)
            }
            None => (frames, 1.0),
        };
        let want = real_frames * ch_in;
        // **What is heard has to be what is HAPPENING, and it was not.**
        //
        // The consumer takes at most one output block per output callback, so
        // whatever built up in the ring stayed there for the rest of the
        // session — a one-way ratchet. And it always built up: the input
        // stream opens when the band does, the OUTPUT stream starts separately
        // and can be seconds behind it while a plugin loads, and every input
        // callback in between pushed a block nobody was draining. The ring is
        // 120 ms deep, so monitoring could sit a tenth of a second behind the
        // room for ever, at the smallest buffer the app offers, with nothing
        // to point at. The owner heard it against REAPER at the same rate and
        // buffer and was right.
        //
        // So the surplus is DROPPED rather than played. What is legitimately
        // in flight is one input block — the one the device is filling now —
        // plus one output block, the one this callback is about to take.
        // Anything beyond that is history.
        //
        // WHOLE FRAMES only. A partial frame would rotate the channels for the
        // rest of the session, which is the rule the capture ring already
        // insists on and for the same reason.
        //
        // This does not close the whole gap and cannot: two AudioUnits means
        // an output callback can only ever see input that arrived before it
        // started, which is one buffer a DUPLEX device does not pay.
        let in_flight = want + self.monitor_block * ch_in;
        let backlog = ring.slots();
        self.shared
            .monitor_backlog
            .store((backlog / ch_in) as u32, Ordering::Relaxed);
        if backlog > in_flight {
            let stale = ((backlog - in_flight) / ch_in) * ch_in;
            if stale > 0 {
                if let Ok(chunk) = ring.read_chunk(stale) {
                    chunk.commit_all();
                }
                self.shared
                    .monitor_slip
                    .fetch_add((stale / ch_in) as u64, Ordering::Relaxed);
            }
        }
        let got = ring.slots().min(want);
        // Read into scratch first: the ring's chunk borrows it, and the mix
        // below needs `self.mix` mutably at the same time.
        //
        // WHOLE FRAMES here too, and this clamp was the one place the rule
        // could break: `got` is frame-aligned (the producer pushes nothing
        // else) but the scratch is `widest * 4096`, which an eighteen-input
        // interface opened at its full width does not divide. Clamping to it
        // committed a partial frame out of the ring, and every read after
        // that mapped each interleaved column to the wrong input — sides
        // swapped, microphones on the wrong strips, the capture tee taping
        // the rotation — for the rest of the session, silently. Rounded
        // DOWN, exactly as the stale-drop above rounds.
        let take = (got.min(self.monitor_scratch.len()) / ch_in) * ch_in;
        let got = {
            let scratch = &mut self.monitor_scratch[..take];
            match ring.read_chunk(scratch.len()) {
                Ok(chunk) => {
                    let (a, b) = chunk.as_slices();
                    scratch[..a.len()].copy_from_slice(a);
                    scratch[a.len()..a.len() + b.len()].copy_from_slice(b);
                    let n = a.len() + b.len();
                    chunk.commit_all();
                    n
                }
                Err(_) => 0,
            }
        };
        // Stretched into the program's timeline, in place, so everything
        // below reads the same buffer it always did. `got` becomes the CHUNK
        // length: whatever arrived covers this chunk now.
        let got = match vs {
            None => got,
            Some(_) => {
                let src_frames = got / ch_in;
                let n = frames * ch_in;
                let frac = self.vs_in_phase.frac();
                let mut dst = std::mem::take(&mut self.vs_in_src);
                if dst.len() >= n && self.monitor_scratch.len() >= n {
                    self.vs_in.set_channels(ch_in);
                    self.vs_in
                        .run(&self.monitor_scratch, src_frames, &mut dst, frames, frac, step);
                    self.monitor_scratch[..n].copy_from_slice(&dst[..n]);
                }
                self.vs_in_src = dst;
                // **Advanced by what was ASKED for, not by what arrived.**
                // A short read is the interface being late; the phase is a
                // position in a stream that is still running at real time,
                // and slipping it by the shortfall would leave the input
                // permanently behind by however many frames were once
                // missing.
                self.vs_in_phase.advance(frames, real_frames, step);
                n
            }
        };

        // **Mute decides the take. Monitoring decides only the room.**
        //
        // A strip is on the bus whenever it is not muted, so the file gets what
        // the desk says it gets — and `monitor_on` narrows to the one question
        // it can answer without lying: whether the SPEAKERS get it too. It used
        // to answer both, and the take was the casualty: a microphone monitored
        // through the interface's own hardware — the ordinary way anybody
        // records one, because it is the only way with no latency — is not
        // monitored here, and produced a take with no microphone in it.
        //
        // Feedback is why the room half exists at all and why it is still off
        // at every launch. See `IvoryApp::input_monitor`.
        //
        // **One switch for every input, and one fader each.** Monitoring is a
        // control-room question and the answer is the same for the whole room;
        // how loud each source is, and whether it is in the take at all, is a
        // question per channel and has a strip per channel.
        let room_target = if self.shared.monitor_on.load(Ordering::Relaxed) {
            1.0
        } else {
            0.0
        };
        let frames_got = got / ch_in;
        let n = frames.min(frames_got);
        if let Some(dry) = self.input_dry.get_mut(..frames * TAP_CHANNELS) {
            dry.fill(0.0);
        }
        // The room's slew is the room's, not any one channel's: it moves once
        // per frame however many inputs are open, or four inputs would each
        // drag it a quarter of the way and the fade would depend on how many
        // microphones happened to be plugged in.
        let room_from = self.room_gain;
        let mut room_to = room_from;
        let mut withheld = 0.0f32;
        // **Every input of the interface, at its own offset in the block.**
        // `Picks` laid them end to end when the stream was opened — a mono
        // input then a stereo pair is one channel then two — and this is the
        // other end of that: which columns of the interleaved frame belong to
        // which strip.
        let mut offset = 0usize;
        let capturing = self.shared.arr_capture.load(Ordering::Relaxed);
        for input in 0..INPUTS {
            let width = self.monitor_widths[input].min(ch_in.saturating_sub(offset));
            if width == 0 {
                continue;
            }
            let at_ch = offset;
            offset += width;
            // **The device column this strip's k-th channel lives in.** The
            // ring is device-wide, so a strip's columns are wherever its pick
            // put them — not `at_ch + k`, which was the old ring's own
            // packing. `EVERYTHING` has no picks and maps straight through,
            // which is exactly right for "the whole device in its own order".
            let dev_col = |k: usize| {
                self.monitor_picks
                    .source(at_ch + k, ch_in)
                    .unwrap_or(at_ch + k)
                    .min(ch_in.saturating_sub(1))
            };
            // ── the arrangement's tee: RAW frames, folded to stereo exactly
            // as the monitor folds (a mono input to both sides) — pre-fader,
            // pre-rack, pre-arm. The take is the signal at the jack, not the
            // room's opinion of it. ──
            if capturing & (1 << input.min(31)) != 0 {
                if let Some(tx) = self.cap_tx.get_mut(input) {
                    if tx.slots() >= n * 2 {
                        for i in 0..n {
                            for c in 0..2 {
                                let col = dev_col(c.min(width - 1));
                                let _ = tx.push(self.monitor_scratch[i * ch_in + col]);
                            }
                        }
                    } else {
                        self.shared
                            .cap_dropped
                            .fetch_add(n as u64, Ordering::Relaxed);
                    }
                }
            }
            // **Which CHANNEL this pick feeds**, mapped by the host: the k-th
            // AUDIO channel takes pick k. An unbound pick still drains — the
            // ring has to keep moving — but is nobody's audio.
            let bound = self.shared.pick_strip[input].load(Ordering::Relaxed);
            let Some(strip) = usize::try_from(bound)
                .ok()
                .filter(|b| *b < CHANNELS)
                .map(Strip::Channel)
            else {
                self.monitor_gain[input] = 0.0;
                continue;
            };
            // **Arm is the live switch.** An unarmed audio channel's input
            // still drains (the ring must keep moving) and still reaches a
            // take — mute decides the take, and arm is not mute — but the
            // ROOM gets none of it: a project must open without headphones.
            let live =
                self.shared.armed.load(Ordering::Relaxed) & (1 << strip.index().min(31)) != 0;
            let heard = live && strip_is_heard(strip, muted, soloed);
            let target = if heard {
                lane_gain_of(&self.auto, &self.shared, strip.index())
            } else {
                0.0
            };
            // Silent and already faded out: the drain above was the whole job
            // for this one. The others still get their turn.
            if !heard && self.monitor_gain[input] <= 1.0e-6 {
                self.monitor_gain[input] = 0.0;
                continue;
            }
            // **The live monitor, which is the path automation would most
            // easily have been left out of** — it is not the arrangement, so
            // a version that automated clips would look right and do nothing
            // to somebody's own playing coming through the desk.
            let lane = strip.index().min(CHANNELS - 1);
            let send = lane_send_of(&self.auto, &self.shared, lane);
            let (pan_l, pan_r) = pan_gains(lane_pan_of(&self.auto, &self.shared, lane));
            let mut peak = [0.0f32; 2];
            // **This strip's own audio first, then its rack, then the fold.**
            //
            // It used to go straight into the mix a sample at a time, which is
            // cheaper and leaves nowhere for an insert to stand. The values
            // are identical — this is the same arithmetic through a buffer —
            // and what it buys is a channel that can carry an amp sim like
            // every other channel can.
            //
            // A mono input goes to both sides; a stereo one keeps its sides.
            // Anything wider is folded down by taking the first two, which is
            // what a monitor is for — hearing that something is arriving, not
            // auditioning a surround mix.
            for i in 0..n {
                self.monitor_gain[input] += (target - self.monitor_gain[input]) * self.gain_coeff;
                for c in 0..TAP_CHANNELS {
                    let lane = dev_col(c.min(width - 1));
                    let src = self.monitor_scratch[i * ch_in + lane] * self.monitor_gain[input];
                    if let Some(dst) = self.fx_in.get_mut(c) {
                        dst[i] = src;
                    }
                }
            }
            {
                // An input channel's rack can hold a voice too — the desk is
                // about to stop distinguishing — and the gate is the same one.
                let armed = self.shared.midi_off.load(Ordering::Relaxed)
                    & (1 << strip.index().min(31))
                    == 0;
                let notes: &[ivory_host::Note] = if armed { &self.notes } else { &[] };
                let controls: &[ivory_host::Control] = if armed { &self.controls } else { &[] };
                run_rack(
                    &mut self.racks[strip.index()],
                    &mut self.fx_in,
                    n,
                    &mut self.fx_out,
                    notes,
                    controls,
                );
            }
            let mut room = room_from;
            // **A LIVE INPUT is on the lane too, and it routes with it.**
            // The routed-away gate was put on the clip, pad and instrument
            // folds and missed this one, so a channel with an input open went
            // straight to the master here AND was gathered out of the stem
            // below and folded in again through its destination: the owner's
            // "100 percent a volume jump on a send added". Every path a lane
            // reaches the master by has to answer to the same switch.
            let direct = self.route_direct.get(strip.index()).copied().unwrap_or(1.0);
            for i in 0..n {
                // The room's own slew, advanced here rather than above: it
                // moves once per frame however many inputs are open, and
                // recomputing it in this pass is exact because both passes
                // walk the same frames in the same order.
                room += (room_target - room) * self.gain_coeff;
                // How much of this frame the room is NOT being given.
                let keep_out = 1.0 - room;
                withheld = withheld.max(keep_out);
                let at = i * TAP_CHANNELS;
                for c in 0..TAP_CHANNELS {
                    let src = self.fx_in.get(c).map_or(0.0, |b| b[i])
                        * if c == 0 { pan_l } else { pan_r };
                    peak[c.min(1)] = peak[c.min(1)].max(src.abs());
                    if let Some(v) = self.mix.get_mut(at + c) {
                        *v += src * direct;
                    }
                    // The stem tap hears the lane's LIVE input too — a
                    // bounced stem is everything the lane put on the bus.
                    if self.stems_live || self.route_n > 0 {
                        if let Some(sv) = self
                            .stem_scratch
                            .get_mut(i * CHANNELS * 2 + strip.index() * 2 + c.min(1))
                        {
                            *sv += src;
                        }
                    }
                    // The room's subtrahend, ADDED to rather than written:
                    // four inputs share one buffer and the last one to run
                    // would otherwise be the only one the speakers were spared.
                    if let Some(d) = self.input_dry.get_mut(at + c) {
                        *d += src * keep_out;
                    }
                    // Post-fader, like every other send here, and closed with
                    // the room: a channel nobody can hear must not arrive back
                    // in the speakers through the reverb.
                    if let Some(a) = self.aux.get_mut(at + c) {
                        *a += src * send * room;
                    }
                }
            }
            room_to = room;
            self.shared.note_strip_peak(strip, peak);
        }
        self.room_gain = room_to;
        // A hundredth of a decibel of an input still withheld is still two
        // mixes. The threshold is what stops a fully-monitored rig from
        // running a second limiter forever over a rounding error.
        self.room_live = withheld > 1.0e-4;
    }

    /// The whole transport: where this chunk starts, and whether it moves.
    ///
    /// **The locate is consumed BEFORE the level is read, in one call**, so a
    /// seek published before a stop can never be honoured after it — the
    /// Acquire pairs with the host's Release, and a callback that sees the new
    /// generation is guaranteed to see every store that preceded it. Per
    /// CHUNK, not per callback: `mix_track` runs inside the chunk loop, and
    /// advancing once per callback would hand every chunk after the first the
    /// same clip samples.
    ///
    /// Returns `(at, rolling, jumped)`. A jump — or the level falling — is what
    /// ends the notes the move strands; see the flush at the call site.
    /// **The gain lane `i` is actually playing at.**
    ///
    /// The automation curve when one is reading, the fader otherwise. This
    /// is the ONLY place `channel_gains` may be read from inside the render
    /// path, and `the_render_path_reads_no_fader_directly` is what keeps
    /// that true — a site that read the atomic itself would be a control
    /// automation silently did not reach, which is the exact failure this
    /// whole shape exists to prevent.
    ///
    /// The curve carries a fader POSITION, so it goes through the same
    /// `fader_to_gain` a mouse drag does. Storing amplitude instead would
    /// have made a -6 dB curve mean something else entirely — the mistake
    /// already made once on this branch in the keyboard nudge.
    fn lane_gain(&self, lane: usize) -> f32 {
        lane_gain_of(&self.auto, &self.shared, lane)
    }

    /// The same for pan, with the curve's `0..1` laid back onto `-1..1`.
    fn lane_pan(&self, lane: usize) -> f32 {
        lane_pan_of(&self.auto, &self.shared, lane)
    }

    /// The send lane `i` is actually feeding the effects bus at.
    ///
    /// Indexed by `Strip::index()` in `Shared`, which is a WIDER space than
    /// the sixteen channels — the click and the bus have sends too, and
    /// nothing automates those. Out-of-range lanes fall through to the
    /// atomic, which is the honest answer for a strip no curve can name.
    fn lane_send(&self, lane: usize) -> f32 {
        lane_send_of(&self.auto, &self.shared, lane)
    }

    /// Resolve every automated control for this block.
    ///
    /// Once, at the top of `render`, so the twelve sites below are a load
    /// and a branch rather than twelve binary searches. Seconds and not
    /// frames because that is what the document stores — converting here
    /// costs one divide and avoids a bake that could go stale against the
    /// curve it came from.
    fn resolve_automation(&mut self, at: u64) {
        use ivory_ui::automation::AutoParam;
        if !self.auto_retiring.is_full() {
            if let Ok(next) = self.auto_incoming.pop() {
                let old = std::mem::replace(&mut self.automation, next);
                let _ = self.auto_retiring.push(old);
            }
        }
        let suspend = self.shared.auto_suspend.load(Ordering::Relaxed);
        if self.automation.is_empty() {
            // The overwhelmingly common song. One branch instead of sixty-four
            // lookups, and it leaves every fader exactly where it was.
            self.auto = AutoNow::default();
            return;
        }
        let secs = at as f64 / self.rate.max(1.0);
        for lane in 0..CHANNELS {
            if suspend & (1 << lane) != 0 {
                self.auto.gain[lane] = None;
                self.auto.pan[lane] = None;
                self.auto.send[lane] = None;
                self.auto.mute[lane] = None;
                continue;
            }
            self.auto.gain[lane] = self
                .automation
                .value_at(lane, AutoParam::Gain, secs)
                .map(|v| v as f32);
            self.auto.pan[lane] = self
                .automation
                .value_at(lane, AutoParam::Pan, secs)
                .map(|v| v as f32);
            self.auto.send[lane] = self
                .automation
                .value_at(lane, AutoParam::Send, secs)
                .map(|v| v as f32);
            // A switch written as a curve: heard below half, muted above it.
            self.auto.mute[lane] = self
                .automation
                .value_at(lane, AutoParam::Mute, secs)
                .map(|v| v >= 0.5);
        }
    }

    /// Tell the desk what each chain is costing.
    ///
    /// **Measured, never assumed.** Every figure comes from the racks
    /// themselves this block, so a plugin swapped in or out is reflected the
    /// next time anybody looks, and a stale number cannot survive a load.
    ///
    /// The maximum is over the CHANNELS and the master's own chain — the
    /// paths whose output reaches the same mix and therefore have to agree
    /// about when "now" is. The drum pads are deliberately not in it: there
    /// are a hundred and twenty-eight of them, they are struck rather than
    /// played back, and letting one padded with a lookahead limiter drag the
    /// whole desk late would be a strange way to lose a mix.
    fn publish_latency(&self) {
        let mut max = 0;
        for lane in 0..CHANNELS {
            // `lane`, emphatically not `STRIPS`. Asking every channel for
            // the MASTER's figure reads plausibly and is wrong twice over:
            // the readout says each of the sixteen costs what the master
            // costs, and a limiter on the master alone drags every channel
            // into compensating for a delay it does not have.
            let l = self
                .racks
                .get(lane)
                .map_or(0, |_| Rack::latency(&self.shared, lane));
            self.shared.lane_latency[lane].store(l, Ordering::Relaxed);
            max = max.max(l);
        }
        // **The alignment target is the channels alone.** See `max_latency`:
        // the master delays every source equally, so aligning to it makes
        // everything wait for a delay it is going to pay again anyway.
        self.shared.max_latency.store(max, Ordering::Relaxed);
        // ...and the READOUT is the whole of it, because a figure that hides
        // latency somebody is paying is worth nothing.
        let master = Rack::latency(&self.shared, ivory_ui::recorder::STRIPS);
        self.shared
            .total_latency
            .store(max.saturating_add(master), Ordering::Relaxed);
    }

    /// The mute mask with automation folded in.
    ///
    /// The mask is a `u32` of bits the desk owns; a curve overrides one bit
    /// without disturbing the others, so a lane nobody has automated goes on
    /// answering to its own button.
    fn muted_now(&self, muted: u32) -> u32 {
        let mut out = muted;
        for lane in 0..CHANNELS {
            match self.auto.mute[lane] {
                Some(true) => out |= 1 << lane,
                Some(false) => out &= !(1 << lane),
                None => {}
            }
        }
        out
    }

    fn run_transport(&mut self, frames: usize) -> (u64, bool, bool) {
        let req = self.shared.transport_req.load(Ordering::Acquire);
        let seek = req != self.seen_req;
        if seek {
            self.seen_req = req;
            self.pos = self.shared.transport_at.load(Ordering::Relaxed);
            self.shared.transport_ack.store(req, Ordering::Release);
        }
        // The wrap's flush, armed on the PREVIOUS block — consumed now, when
        // `at` is the wrapped position and rewinding the cursors lands them
        // at the loop's head. See `pending_flush`.
        let flush = seek || std::mem::take(&mut self.pending_flush);
        let rolling = self.shared.rolling.load(Ordering::Relaxed);
        let at = self.pos;
        if rolling {
            self.pos += frames as u64;
            // **The loop wrap.** Active only when `b > a`; when `pos` has
            // reached or passed `b`, fold the overshoot back from `a` so a
            // block that straddles the boundary keeps its phase. Guarded so
            // a playhead that STARTED past the loop (playing on ahead) is
            // left alone rather than yanked backward — the loop catches the
            // clock only when it runs INTO the end from inside or before.
            let a = self.shared.loop_a.load(Ordering::Relaxed);
            let b = self.shared.loop_b.load(Ordering::Relaxed);
            if b > a && at < b && self.pos >= b {
                self.pos = a + (self.pos - b) % (b - a);
                // Arm the flush for the NEXT block, not this one — see the
                // field. The MIDI cursors must rewind against the wrapped
                // `at`, which is what the next block carries.
                self.pending_flush = true;
            }
        }
        self.shared.transport_pos.store(self.pos, Ordering::Relaxed);
        (at, rolling, flush)
    }

    /// Add the backing track to the bus, if one is loaded and the transport is
    /// rolling.
    ///
    /// The clip is stereo at the device's rate — `decode` guarantees both — so
    /// there is no resampling and no channel mapping here, which is the whole
    /// reason it is guaranteed there.
    fn mix_clips(&mut self, at: u64, rolling: bool, frames: usize, muted: u32, soloed: u32) {
        // A clip LIST arriving — and the displaced one going BACK, by the
        // racks' own protocol: no room to return it, no swap this block.
        // Dropping it here would free a hundred megabytes inside the
        // callback.
        if self.track_retiring.slots() > 0 {
            if let Ok(next) = self.track_incoming.pop() {
                let old = std::mem::replace(&mut self.clips, next);
                let _ = self.track_retiring.push(old);
            }
        }
        // **Stopped means silent, exactly as it always has.** Without this the
        // parked position is re-read every callback and the clips buzz one
        // block of themselves under a readout that says nothing is moving.
        if !rolling || self.clips.is_empty() {
            return;
        }
        // **Per LANE: the clip plays through its channel.** Its fader, its
        // mute and solo, its post-fader send — the strip's own facts, read
        // the way every other source reads them. Not its rack yet: the rack
        // also runs for the lane's live input, one rack must run once per
        // block, and that unification is a later phase's.
        let coeff = self.gain_coeff;
        let mut target = [0.0f32; CHANNELS];
        let mut send = [0.0f32; CHANNELS];
        let mut pan = [(1.0f32, 1.0f32); CHANNELS];
        let mut punch = [u64::MAX; CHANNELS];
        for lane in 0..CHANNELS {
            let strip = Strip::Channel(lane);
            target[lane] = if strip_is_heard(strip, muted, soloed) {
                self.lane_gain(lane)
            } else {
                0.0
            };
            send[lane] = self.lane_send(strip.index());
            pan[lane] = pan_gains(self.lane_pan(lane));
            punch[lane] = self.shared.punch[lane].load(Ordering::Relaxed);
        }
        // Stems and ROUTING share this buffer: a send reads the
        // lane's own audio out of it, so it is filled whenever either
        // customer asks.
        let stems = self.stems_live || self.route_n > 0;
        // Copied out before the buffers are borrowed: the fold below holds
        // `mix`/`aux`/`stem` mutably, and this has to be readable inside it.
        let direct = self.route_direct;
        let (Some(mix), Some(aux), Some(stem)) = (
            self.mix.get_mut(..frames * TAP_CHANNELS),
            self.aux.get_mut(..frames * TAP_CHANNELS),
            self.stem_scratch.get_mut(..frames * CHANNELS * 2),
        ) else {
            return;
        };
        // How far the whole graph is behind, which is what the clips must
        // wait for. Read once per block: it changes when a plugin is loaded,
        // not within a buffer.
        let pdc = self.shared.max_latency.load(Ordering::Relaxed);
        let mut peak = [[0.0f32; 2]; CHANNELS];
        for f in 0..frames {
            for lane in 0..CHANNELS {
                self.lane_gain[lane] += (target[lane] - self.lane_gain[lane]) * coeff;
            }
            // Project frame. The u64 compares come BEFORE any cast, because
            // `p` grows without bound; before a clip and past its end are
            // both silence, and both are ordinary.
            // **The clips' own compensation, and it is arithmetic rather
            // than a buffer.**
            //
            // An audio clip does not pass through its channel's insert rack
            // — see the note in this function's header — so it arrives with
            // no latency at all, while a MIDI lane carrying a lookahead
            // plugin arrives `L` frames late. Aligning them means holding
            // the clips back by the graph's own maximum.
            //
            // `mix_clips` derives everything from the absolute frame, which
            // is what makes locate, loop and offline export correct for
            // free. Reading `p - delay` keeps that property exactly: there
            // is no cursor to rewind on a flush, nothing to reset on a loop
            // wrap, and a seek lands in the right place because it always
            // did. A delay LINE here would have needed all three.
            //
            // **`checked_sub`, not `saturating_sub`.** Saturating pins `p` at
            // zero for the first `pdc` frames — which is precisely the window
            // compensation exists for — so a clip starting at 0:00 emitted
            // its first sample over and over while the plugin lane's own
            // delay line was correctly putting out silence. A DC step
            // followed by a discontinuity, on every play-from-zero with a
            // lookahead plugin loaded.
            //
            // Before the compensated zero there is no project frame, and the
            // honest answer is the same one the clip loop gives for any
            // other position a clip does not cover: nothing. The per-lane
            // gain smoothing above has already run, so the fader still
            // settles across these frames.
            let Some(p) = (at + f as u64).checked_sub(u64::from(pdc)) else {
                continue;
            };
            for placed in &self.clips {
                let Some(k) = p.checked_sub(placed.start) else {
                    continue;
                };
                if k >= placed.len {
                    continue;
                }
                // **Being recorded over, right now**: from the punch frame
                // on, this lane's clips are the material the take is
                // replacing, and the take must not be heard against them.
                if p >= punch[placed.strip.min(CHANNELS - 1)] {
                    continue;
                }
                let i = match usize::try_from(placed.offset + k) {
                    Ok(frame) => frame * 2,
                    Err(_) => continue,
                };
                // Bounds-checked rather than trusted: `k < len` is the
                // clamp, and this is the line that would panic on the audio
                // thread if the host ever pushed a len past the file.
                let (cl, cr) = match (placed.clip.samples.get(i), placed.clip.samples.get(i + 1)) {
                    (Some(a), Some(b)) => (*a, *b),
                    _ => continue,
                };
                // The fades: linear ramps in frames, computed from the u64
                // distances so a four-minute clip cannot lose precision.
                // The head ramp rises 0 -> 1 over `fade_in`; the tail falls
                // to exactly 0 ON the last frame, so a faded clip cannot
                // click shut.
                let mut env = 1.0f32;
                if placed.fade_in > 0 && k < placed.fade_in {
                    env *= k as f32 / placed.fade_in as f32;
                }
                let left = placed.len - k;
                if placed.fade_out > 0 && left <= placed.fade_out {
                    env *= (left - 1) as f32 / placed.fade_out as f32;
                }
                let lane = placed.strip.min(CHANNELS - 1);
                // **The clip's own level, before the lane's.** Sanitised
                // here rather than trusted: this multiplies into somebody's
                // monitors, and the rule the gains follow everywhere else.
                let clip_g = if placed.gain.is_finite() {
                    placed.gain.clamp(0.0, 64.0)
                } else {
                    1.0
                };
                let g = self.lane_gain[lane] * clip_g;
                let (l, r) = (cl * env * g * pan[lane].0, cr * env * g * pan[lane].1);
                peak[lane][0] = peak[lane][0].max(l.abs());
                peak[lane][1] = peak[lane][1].max(r.abs());
                // The METER and the STEM still see the whole lane; only the
                // path to the master closes when the lane feeds a bus.
                mix[f * TAP_CHANNELS] += l * direct[lane];
                mix[f * TAP_CHANNELS + 1] += r * direct[lane];
                aux[f * TAP_CHANNELS] += l * send[lane];
                aux[f * TAP_CHANNELS + 1] += r * send[lane];
                if stems {
                    stem[f * CHANNELS * 2 + lane * 2] += l;
                    stem[f * CHANNELS * 2 + lane * 2 + 1] += r;
                }
            }
        }
        for (lane, pk) in peak.iter().enumerate() {
            if pk[0] > 0.0 || pk[1] > 0.0 {
                self.shared.note_strip_peak(Strip::Channel(lane), *pk);
            }
        }
    }

    /// The drumpad: sixteen one-shot voices, summed straight onto the bus.
    ///
    /// Not a strip: the pads have no lane, no arm and no M/S bit — they are
    /// a performance surface like the keyboard, and their sum joins `mix`
    /// upstream of the room split and the master insert, so the limiter, the
    /// speakers and the take all treat them like everything else. Runs
    /// whether or not the transport rolls: a drum machine that only sounds
    /// while the timeline moves is broken.
    /// The media explorer's audition: swap, play once, and fill the
    /// PREVIEW buffer the out-pass adds AFTER the take's tap — the click's
    /// own road. An audition clicked over a rolling take must never be
    /// printed into the recording; it is for the ears alone.
    fn mix_preview(&mut self, frames: usize) {
        if let Some(po) = self.preview_out.get_mut(..frames * 2) {
            po.fill(0.0);
        }
        if self.preview_retiring.slots() > 0 {
            if let Ok(next) = self.preview_incoming.pop() {
                let old = std::mem::replace(&mut self.preview, next);
                let _ = self.preview_retiring.push(old);
                self.preview_pos = 0;
            }
        }
        let Some(clip) = self.preview.as_deref() else {
            return;
        };
        let total = clip.frames();
        if self.preview_pos >= total {
            return;
        }
        const PREVIEW_GAIN: f32 = 0.8;
        // **The audition rides the faders a LOADED pad would.** The preview
        // joins the out-pass AFTER the master multiplied the mix, so without
        // these it played the raw file at unity into a desk everyone else
        // obeys. The drum bus fader (the pads' own strip law) and the master
        // scale it; mute and solo are deliberately ignored — clicking a file
        // is a question, and a muted bus should not eat the answer.
        let drum_wanted = self.shared.drum_strip.load(Ordering::Relaxed);
        let drum_lane = (drum_wanted >= 0)
            .then_some(drum_wanted as usize)
            .filter(|ch| *ch < CHANNELS);
        let bus = match drum_lane {
            Some(ch) => self.lane_gain(ch),
            None => Shared::f32_of(&self.shared.drums_gain).clamp(0.0, 8.0),
        };
        let master = Shared::f32_of(&self.shared.master_gain);
        let level = PREVIEW_GAIN * bus * master;
        for f in 0..frames {
            let at = self.preview_pos + f;
            if at >= total {
                break;
            }
            let l = clip.samples.get(at * 2).copied().unwrap_or(0.0);
            let r = clip.samples.get(at * 2 + 1).copied().unwrap_or(0.0);
            if let Some(v) = self.preview_out.get_mut(f * 2) {
                *v += l * level;
            }
            if let Some(v) = self.preview_out.get_mut(f * 2 + 1) {
                *v += r * level;
            }
        }
        self.preview_pos += frames;
    }

    fn mix_pads(&mut self, frames: usize, muted: u32, soloed: u32) {
        // A kit arriving — and the displaced one going BACK, by the racks'
        // own protocol: no room to return it, no swap this block. The voices
        // are silenced across a swap; see `PadVoice::silence`.
        if self.pad_retiring.slots() > 0 {
            if let Ok(next) = self.pad_incoming.pop() {
                let old = std::mem::replace(&mut self.pad_kit, next);
                let _ = self.pad_retiring.push(old);
                for v in &mut self.pad_voices {
                    v.silence();
                }
            }
        }
        // Silence still flows through LIVE racks — a reverb tail on the
        // bus or a pad outlives the voice that fed it.
        let any_rack = !self.racks[Strip::Drums.index()].is_empty()
            || (0..PADS).any(|i| !self.racks[pad_rack_index(i)].is_empty());
        if self.pad_hits.is_empty()
            && self.pad_offs.is_empty()
            && !self.pad_voices.iter().any(PadVoice::active)
            && !any_rack
        {
            return;
        }
        let coeff = self.gain_coeff;
        // **The DRUM BUS is the pads' strip now**: its fader, pan, mute,
        // solo and send govern the summed kit, and its rack runs over it —
        // bus-wide effects live on the DRUMS channel, exactly as the owner
        // ruled. The old in-panel master is gone.
        // **The kit belongs to its LANE now.** With a drumpad assigned to
        // a channel, that channel's fader, pan, mute, solo, send, meter —
        // and STEM — govern the summed kit: the visible drum bus strip is
        // retired, and the lane is the one place the drums answer to. The
        // old hidden Drums internals only rule when no lane carries the
        // pad (the drumpad window played bare), where they rest at unity.
        let drum_wanted = self.shared.drum_strip.load(Ordering::Relaxed);
        let drum_lane = (drum_wanted >= 0)
            .then_some(drum_wanted as usize)
            .filter(|ch| *ch < CHANNELS);
        let bus_strip = drum_lane.map_or(Strip::Drums, Strip::Channel);
        let master = if strip_is_heard(bus_strip, muted, soloed) {
            match drum_lane {
                Some(ch) => self.lane_gain(ch),
                None => Shared::f32_of(&self.shared.drums_gain).clamp(0.0, 8.0),
            }
        } else {
            0.0
        };
        let (bus_l, bus_r) = pan_gains(match drum_lane {
            Some(ch) => self.lane_pan(ch),
            None => Shared::f32_of(&self.shared.drums_pan),
        });
        // Through the accessor like every other control, even though the
        // drum BUS sits past the sixteen channels a curve can name: it falls
        // through to the atomic, and going round it is how the other three
        // sites came to be missed.
        let bus_send = self.lane_send(bus_strip.index());
        // The knobs, read once per chunk like the channel gains: they are
        // published at human speed. The clamps are the audio thread refusing
        // to trust numbers it did not make; the setter sanitised them too.
        let mut gain = [0.0f32; PADS];
        let mut choke = [0u8; PADS];
        let mut pan_l = [1.0f32; PADS];
        let mut pan_r = [1.0f32; PADS];
        let mut step = [1.0f64; PADS];
        let mut trim0 = [0.0f32; PADS];
        let mut trim1 = [1.0f32; PADS];
        for i in 0..PADS {
            trim0[i] = Shared::f32_of(&self.shared.pad_trim_start[i]).clamp(0.0, 1.0);
            trim1[i] = Shared::f32_of(&self.shared.pad_trim_end[i]).clamp(0.0, 1.0);
            if trim1[i] <= trim0[i] + 0.001 {
                trim0[i] = 0.0;
                trim1[i] = 1.0;
            }
            gain[i] = Shared::f32_of(&self.shared.pad_gain[i]).clamp(0.0, 2.0);
            choke[i] = self.shared.pad_choke[i].load(Ordering::Relaxed).min(255) as u8;
            let pan = Shared::f32_of(&self.shared.pad_pan[i]).clamp(-1.0, 1.0);
            // The linear law, not constant-power: a drum sits still, and
            // linear keeps a centred pad at exactly unity.
            pan_l[i] = (1.0 - pan).min(1.0);
            pan_r[i] = (1.0 + pan).min(1.0);
            let tune = f64::from(Shared::f32_of(&self.shared.pad_tune[i]).clamp(-12.0, 12.0));
            step[i] = (tune / 12.0).exp2();
        }
        for c in 0..TAP_CHANNELS {
            if let Some(dst) = self.fx_in.get_mut(c) {
                dst[..frames].fill(0.0);
            }
        }
        if let Some(po) = self.pad_out.get_mut(..frames * PADS * 2) {
            po.fill(0.0);
        }
        let mut sounded = [false; PADS];
        let mut peak = [0.0f32; PADS];
        for f in 0..frames {
            // **Offs FIRST** — the lane walk's own rule, held here too: a
            // release sharing a frame with the next strike must fade the
            // voice that ENDED, never the one just born. The hits ran
            // first once, and every legato note on an obeying pad choked.
            for h in &self.pad_offs {
                if h.offset != f as i32 {
                    continue;
                }
                if let Some(v) = self.pad_voices.get_mut(h.pad) {
                    v.cut();
                }
            }
            // The strikes that land on this frame: whoever shares the choke
            // group fades, the struck pad starts clean. The group is read at
            // the strike, so moving a knob never cuts a voice already gone.
            for h in &self.pad_hits {
                if h.offset != f as i32 {
                    continue;
                }
                if choke[h.pad] != 0 {
                    for (q, v) in self.pad_voices.iter_mut().enumerate() {
                        if q != h.pad && choke[q] == choke[h.pad] {
                            v.cut();
                        }
                    }
                }
                if let Some(v) = self.pad_voices.get_mut(h.pad) {
                    let frames = self.pad_kit.pads[h.pad]
                        .as_deref()
                        .map_or(0.0, |c| c.frames() as f64);
                    v.trigger(
                        h.velocity,
                        step[h.pad],
                        f64::from(trim0[h.pad]) * frames,
                        f64::from(trim1[h.pad]) * frames,
                    );
                }
            }
            for (i, v) in self.pad_voices.iter_mut().enumerate() {
                if !v.active() {
                    continue;
                }
                let Some(clip) = self.pad_kit.pads[i].as_deref() else {
                    // A voice with no sample is a swap artefact; end it.
                    v.silence();
                    continue;
                };
                let (ml, mr) = v.main.next(clip);
                let (dl, dr) = v.dying.next(clip);
                let g = gain[i];
                let l = (ml + dl) * g * pan_l[i];
                let r = (mr + dr) * g * pan_r[i];
                peak[i] = peak[i].max(l.abs()).max(r.abs());
                sounded[i] = true;
                let at = (f * PADS + i) * 2;
                if let Some(v) = self.pad_out.get_mut(at) {
                    *v += l;
                }
                if let Some(v) = self.pad_out.get_mut(at + 1) {
                    *v += r;
                }
            }
        }
        // **Each pad's own bays, over its voice alone** — then the bus sum.
        // A pad with an empty rack skips the copy and sums straight across.
        for i in 0..PADS {
            let rack_at = pad_rack_index(i);
            let rack_live = !self.racks[rack_at].is_empty();
            // A live rack processes even silence — its tail is its own.
            if !sounded[i] && !rack_live {
                continue;
            }
            if rack_live {
                for c in 0..TAP_CHANNELS.min(2) {
                    if let Some(dst) = self.pad_io.get_mut(c) {
                        for f in 0..frames {
                            dst[f] = self.pad_out[(f * PADS + i) * 2 + c];
                        }
                    }
                }
                run_rack(
                    &mut self.racks[rack_at],
                    &mut self.pad_io,
                    frames,
                    &mut self.fx_out,
                    &[],
                    &[],
                );
                for c in 0..TAP_CHANNELS.min(2) {
                    let src = &self.pad_io[c];
                    if let Some(dst) = self.fx_in.get_mut(c) {
                        for f in 0..frames {
                            dst[f] += src[f];
                        }
                    }
                }
            } else {
                for c in 0..TAP_CHANNELS.min(2) {
                    if let Some(dst) = self.fx_in.get_mut(c) {
                        for f in 0..frames {
                            dst[f] += self.pad_out[(f * PADS + i) * 2 + c];
                        }
                    }
                }
            }
        }
        for (i, pk) in peak.iter().enumerate() {
            if *pk > 0.0 {
                self.shared.pad_peak[i].fetch_max(pk.to_bits(), Ordering::Relaxed);
            }
        }
        // The bus's own rack, over the summed kit.
        if !self.racks[Strip::Drums.index()].is_empty() {
            run_rack(
                &mut self.racks[Strip::Drums.index()],
                &mut self.fx_in,
                frames,
                &mut self.fx_out,
                &[],
                &[],
            );
        }
        // The fold: the DRUMS strip's fader, pan, send and peak, exactly as
        // every other strip does it.
        let want = frames * TAP_CHANNELS;
        // Stems and ROUTING share this buffer: a send reads the
        // lane's own audio out of it, so it is filled whenever either
        // customer asks.
        let stems = self.stems_live || self.route_n > 0;
        let (Some(mix), Some(aux), Some(stem)) = (
            self.mix.get_mut(..want),
            self.aux.get_mut(..want),
            self.stem_scratch.get_mut(..frames * CHANNELS * 2),
        ) else {
            return;
        };
        let mut bus_peak = [0.0f32; 2];
        // The kit rides its lane's routing like anything else on that lane:
        // route the drum lane to a bus and the kit goes with it.
        let direct = drum_lane.map_or(1.0, |ch| self.route_direct[ch]);
        for f in 0..frames {
            self.pad_gain += (master - self.pad_gain) * coeff;
            let g = self.pad_gain;
            let l = self.fx_in.first().map_or(0.0, |b| b[f]) * g * bus_l;
            let r = self.fx_in.get(1).map_or(0.0, |b| b[f]) * g * bus_r;
            bus_peak[0] = bus_peak[0].max(l.abs());
            bus_peak[1] = bus_peak[1].max(r.abs());
            mix[f * TAP_CHANNELS] += l * direct;
            mix[f * TAP_CHANNELS + 1] += r * direct;
            aux[f * TAP_CHANNELS] += l * bus_send;
            aux[f * TAP_CHANNELS + 1] += r * bus_send;
            // The kit lands in ITS LANE's stem — the gap the owner's
            // first stems export would have hit next.
            if stems {
                if let Some(ch) = drum_lane {
                    stem[f * CHANNELS * 2 + ch * 2] += l;
                    stem[f * CHANNELS * 2 + ch * 2 + 1] += r;
                }
            }
        }
        self.shared.note_strip_peak(bus_strip, bus_peak);
    }

    /// The eight general channels: the built-in, the bay voices, and their
    /// effect bays — everything except AUDIO channels, whose racks run where
    /// their input arrives (see `mix_monitor`).
    ///
    /// Replaces `render_builtin`, and generalises exactly what that function
    /// already did for one synth: render into a private buffer, run the
    /// channel's rack, fold at the channel's fader and send, note the peak.
    fn mix_channels(
        &mut self,
        frames: usize,
        muted: u32,
        soloed: u32,
        at: u64,
        rolling: bool,
        flush: bool,
    ) {
        // What the slowest chain on the desk costs — read once per block,
        // because it changes when a plugin is loaded and not within a
        // buffer. See `publish_latency`.
        let pdc_max = self.shared.max_latency.load(Ordering::Relaxed);
        // A patch change, if one is waiting and the lock is free this block.
        if let Ok(mut g) = self.shared.pending_voice.try_lock() {
            if let Some(v) = g.take() {
                self.builtin.set_voice(v);
            }
        }
        // The note the LAST block's walk parked, if any — taken so it lasts
        // exactly one block unless the walk below parks again.
        let mut parked = std::mem::take(&mut self.clip_parked);
        // A MIDI schedule arriving, by the racks' own swap protocol; its
        // cursors start over, and fast-forward past what is behind the
        // playhead without sounding it.
        if self.midi_retiring.slots() > 0 {
            if let Ok(next) = self.midi_incoming.pop() {
                let old = std::mem::replace(&mut self.midi_clips, next);
                let _ = self.midi_retiring.push(old);
                self.midi_cursor.clear();
                self.midi_cursor.resize(self.midi_clips.len(), 0);
                // Fresh cursors owe nothing: what stands behind the playhead
                // now is the fast-forward's to skip, not a parked note's due.
                parked = false;
            }
        }
        // A jump rewinds every cursor: each fast-forwards to the playhead on
        // its next pass, skipping without sounding.
        if flush {
            for c in &mut self.midi_cursor {
                *c = 0;
            }
            parked = false;
        }
        let wanted = self.shared.builtin_strip.load(Ordering::Relaxed);
        let builtin_at = (wanted >= 0).then_some(wanted as usize);
        let drum_wanted = self.shared.drum_strip.load(Ordering::Relaxed);
        let drum_at = (drum_wanted >= 0).then_some(drum_wanted as usize);
        let midi_off = self.shared.midi_off.load(Ordering::Relaxed);
        let builtin_armed = builtin_at.is_some_and(|ch| midi_off & (1 << ch.min(31)) == 0);
        // CC 64 is the damper, live input only. Half scale is the switch
        // point every synth uses.
        if builtin_armed {
            for c in &self.controls {
                if c.controller == 64 {
                    self.builtin.set_pedal(c.value >= 64);
                }
            }
        }

        // The per-lane note list, taken out of `self` so the loop below can
        // read the live list and the clips while filling it.
        let mut lane_notes = std::mem::take(&mut self.lane_notes);
        let coeff = self.gain_coeff;
        for ch in 0..CHANNELS {
            let is_builtin = builtin_at == Some(ch);
            let rack_live = !self.racks[ch].is_empty();
            // An AUDIO channel's bit is set and its rack runs in
            // `mix_monitor`, over its input — running it here too would fold
            // its effect tails twice.
            let armed = midi_off & (1 << ch.min(31)) == 0;

            // ── this lane's notes: the LIVE list while armed, the CLIPS'
            // whenever the transport stands on them — playback does not ask
            // the arm, which is the whole point of the arm ──
            lane_notes.clear();
            if armed {
                lane_notes.extend_from_slice(&self.notes);
            }
            if flush {
                // The strands the jump or the stop left on THIS lane — swept
                // as their offs are emitted, entry by entry, never wholesale:
                // a note the jump LANDS ON registers later this same block,
                // and a clear at the end would strand it instead.
                let mut i = 0;
                while i < self.active_clip.len() {
                    let a = self.active_clip[i];
                    if usize::from(a.lane) == ch {
                        if lane_notes.len() < lane_notes.capacity() {
                            lane_notes.push(Note {
                                offset: 0,
                                pitch: i16::from(a.key),
                                velocity: 0.0,
                                on: false,
                            });
                        }
                        if a.key < 128 {
                            self.sounding &= !(1u128 << a.key);
                        }
                        self.active_clip.swap_remove(i);
                        continue;
                    }
                    i += 1;
                }
            }
            let mut lane_has_clips = false;
            if rolling {
                let end = at + frames as u64;
                // **Offs FIRST.** Two notes on one key back to back — the
                // owner's tied chords — put the first's off and the
                // second's on in the same block, and ons pushed first got
                // silenced by the off behind them: only the first chord
                // ever sounded. The drain runs again after the walk for
                // notes that both start and end inside this block.
                let mut i = 0;
                while i < self.active_clip.len() {
                    let a = self.active_clip[i];
                    if usize::from(a.lane) == ch && a.off < end {
                        if lane_notes.len() < lane_notes.capacity() {
                            lane_notes.push(Note {
                                offset: a.off.saturating_sub(at) as i32,
                                pitch: i16::from(a.key),
                                velocity: 0.0,
                                on: false,
                            });
                        }
                        if a.key < 128 {
                            self.sounding &= !(1u128 << a.key);
                        }
                        self.active_clip.swap_remove(i);
                        continue;
                    }
                    i += 1;
                }
                for idx in 0..self.midi_clips.len() {
                    if self.midi_clips[idx].strip.min(CHANNELS - 1) != ch {
                        continue;
                    }
                    lane_has_clips = true;
                    let mc = &self.midi_clips[idx];
                    let mut cur = self.midi_cursor.get(idx).copied().unwrap_or(0);
                    while cur < mc.notes.len() {
                        let nt = mc.notes[cur];
                        let on_f = mc.start + nt.at;
                        if on_f >= end {
                            break;
                        }
                        // Behind the playhead (a jump landed past it), past
                        // the clip's trimmed end — or inside the region a
                        // rolling record is REPLACING: skipped, not sounded.
                        // The one exception is a note the LAST block parked:
                        // it is behind by a block at most, and it sounds now,
                        // at the top of this one, instead of being dropped.
                        if (on_f < at && !parked)
                            || nt.at >= mc.len
                            || on_f >= self.shared.punch[ch].load(Ordering::Relaxed)
                        {
                            cur += 1;
                            continue;
                        }
                        if lane_notes.len() >= lane_notes.capacity()
                            || self.active_clip.len() >= 256
                        {
                            // **Withheld, not dropped.** `cur += 1` used to
                            // run before this break, so the note it consumed
                            // was never pushed anywhere and the stored cursor
                            // stood past it: one clip note vanished from the
                            // arrangement, for good, every time this fired —
                            // where the live path parks its overflow in
                            // `pending`. The cursor now stays ON the note and
                            // `clip_parked` keeps the next block's walk from
                            // fast-forwarding over it.
                            self.clip_parked = true;
                            break;
                        }
                        cur += 1;
                        lane_notes.push(Note {
                            // `saturating_sub`, because a parked note's `on_f`
                            // is behind `at` and it lands at offset 0 — late
                            // by a block, which is the price of the park.
                            offset: on_f.saturating_sub(at) as i32,
                            pitch: i16::from(nt.key),
                            velocity: f32::from(nt.vel) / 127.0,
                            on: true,
                        });
                        let end_rel = (nt.at + nt.dur.max(1)).min(mc.len);
                        self.active_clip.push(ActiveClipNote {
                            lane: ch as u8,
                            key: nt.key,
                            off: mc.start + end_rel,
                        });
                        if nt.key < 128 {
                            self.sounding |= 1u128 << nt.key;
                        }
                    }
                    self.midi_cursor[idx] = cur;
                }
                // The offs that fall due inside this block, on this lane.
                let mut i = 0;
                while i < self.active_clip.len() {
                    let a = self.active_clip[i];
                    if usize::from(a.lane) == ch && a.off < end {
                        if lane_notes.len() < lane_notes.capacity() {
                            lane_notes.push(Note {
                                offset: a.off.saturating_sub(at) as i32,
                                pitch: i16::from(a.key),
                                velocity: 0.0,
                                on: false,
                            });
                        }
                        if a.key < 128 {
                            self.sounding &= !(1u128 << a.key);
                        }
                        self.active_clip.swap_remove(i);
                        continue;
                    }
                    i += 1;
                }
            } else {
                lane_has_clips = self
                    .midi_clips
                    .iter()
                    .any(|m| m.strip.min(CHANNELS - 1) == ch);
            }

            // The DRUMPAD lane: every note here is a pad strike — the same
            // sixteen one-shots the drumpad window plays, so the editor's
            // roll and a MIDI keyboard trigger exactly what a finger does.
            // The strikes render in `mix_pads`, on the DRUMS bus; this lane
            // itself carries no signal, but its fader still has to advance
            // so a later un-assignment starts at the written level.
            if drum_at == Some(ch) && !is_builtin {
                let obey = [
                    self.shared.pad_obey[0].load(Ordering::Relaxed),
                    self.shared.pad_obey[1].load(Ordering::Relaxed),
                ];
                for n in &lane_notes {
                    let Ok(key) = u8::try_from(n.pitch) else {
                        continue;
                    };
                    let pad = usize::from(key.wrapping_sub(PAD_BASE));
                    if key < PAD_BASE || pad >= PADS {
                        continue;
                    }
                    if !n.on {
                        // Pads are one-shots — unless this one OBEYS
                        // note-offs, where the release fades the voice.
                        if obey[pad >> 6] & (1u64 << (pad & 63)) != 0
                            && self.pad_offs.len() < self.pad_offs.capacity()
                        {
                            self.pad_offs.push(PadHit {
                                offset: n.offset,
                                pad,
                                velocity: 0.0,
                            });
                        }
                        continue;
                    }
                    if self.pad_hits.len() == self.pad_hits.capacity() {
                        break;
                    }
                    self.pad_hits.push(PadHit {
                        offset: n.offset,
                        pad,
                        velocity: n.velocity,
                    });
                }
                let target = if strip_is_heard(Strip::Channel(ch), muted, soloed) {
                    self.lane_gain(ch)
                } else {
                    0.0
                };
                for _ in 0..frames {
                    self.channel_gain[ch] += (target - self.channel_gain[ch]) * coeff;
                }
                continue;
            }

            // The built-in is one synth: its lane's notes — live and clip
            // both — feed it here, and it goes quiet only when neither is
            // speaking for it.
            if is_builtin {
                if lane_notes.is_empty()
                    && !builtin_armed
                    && !lane_has_clips
                    && self.builtin.active()
                {
                    self.builtin.all_notes_off();
                } else {
                    for n in &lane_notes {
                        let Ok(pitch) = u8::try_from(n.pitch) else {
                            continue;
                        };
                        if n.on {
                            self.builtin.note_on(pitch, n.velocity);
                        } else {
                            self.builtin.note_off(pitch);
                        }
                    }
                }
            }

            // A lane renders when something can SOUND on it: its own live
            // arm, or clips standing on it — through the built-in or a rack.
            let feeds = armed || lane_has_clips || !lane_notes.is_empty();
            if !feeds || (!is_builtin && !rack_live) {
                // **The gain is advanced whether or not anything renders.** A
                // fader moved during a rest has to have arrived by the next
                // note, or it comes in at the old level and slides.
                let target = if strip_is_heard(Strip::Channel(ch), muted, soloed) {
                    self.lane_gain(ch)
                } else {
                    0.0
                };
                for _ in 0..frames {
                    self.channel_gain[ch] += (target - self.channel_gain[ch]) * coeff;
                }
                continue;
            }
            // The channel's own signal, planar for the rack.
            if is_builtin && self.builtin.active() {
                let want = frames * TAP_CHANNELS;
                let Some(scratch) = self.builtin_scratch.get_mut(..want) else {
                    continue;
                };
                scratch.fill(0.0);
                self.builtin.render(scratch, frames, TAP_CHANNELS);
                for c in 0..TAP_CHANNELS {
                    let Some(dst) = self.fx_in.get_mut(c) else {
                        continue;
                    };
                    for f in 0..frames {
                        dst[f] = scratch[f * TAP_CHANNELS + c];
                    }
                }
            } else {
                // Silence in: a bay-1 voice REPLACES it, an effect transforms
                // it, and a rack of effects over silence is silence.
                for c in 0..TAP_CHANNELS {
                    if let Some(dst) = self.fx_in.get_mut(c) {
                        dst[..frames].fill(0.0);
                    }
                }
                if !rack_live {
                    continue;
                }
            }
            if rack_live {
                let controls: &[ivory_host::Control] = if armed { &self.controls } else { &[] };
                run_rack(
                    &mut self.racks[ch],
                    &mut self.fx_in,
                    frames,
                    &mut self.fx_out,
                    &lane_notes,
                    controls,
                );
            }
            // **And the lane waits for the slowest of its neighbours.**
            //
            // Straight after the rack and before the fold, which is the one
            // place a channel's own audio exists as a separate buffer. A
            // chain that delays by `L` needs `max - L` more to arrive with
            // everything else; the chain with the most latency waits for
            // nothing, and the clips — which pass through no rack at all —
            // wait for the whole of `max` in `mix_clips`.
            //
            // Runs whether or not the rack is live, so switching the last
            // plugin off does not leave the lane early by its own latency
            // for one block before the figures catch up.
            {
                let own = Rack::latency(&self.shared, ch);
                let want = usize::try_from(pdc_max.saturating_sub(own)).unwrap_or(0);
                if let Some(line) = self.pdc.get_mut(ch) {
                    line.run(&mut self.fx_in, frames, want);
                }
            }
            // The fold: fader, send, peak — the channel's own, exactly as
            // every other strip does it.
            let target = if strip_is_heard(Strip::Channel(ch), muted, soloed) {
                self.lane_gain(ch)
            } else {
                0.0
            };
            let send = self.lane_send(Strip::Channel(ch).index());
            let (pan_l, pan_r) = pan_gains(self.lane_pan(ch));
            let want = frames * TAP_CHANNELS;
            // Stems and ROUTING share this buffer: a send reads the
            // lane's own audio out of it, so it is filled whenever either
            // customer asks.
            let stems = self.stems_live || self.route_n > 0;
            let direct = self.route_direct[ch];
            let (Some(mix), Some(aux), Some(stem)) = (
                self.mix.get_mut(..want),
                self.aux.get_mut(..want),
                self.stem_scratch.get_mut(..frames * CHANNELS * 2),
            ) else {
                continue;
            };
            let mut peak = [0.0f32; 2];
            for f in 0..frames {
                self.channel_gain[ch] += (target - self.channel_gain[ch]) * coeff;
                let g = self.channel_gain[ch];
                let l = self.fx_in.first().map_or(0.0, |b| b[f]) * g * pan_l;
                let r = self.fx_in.get(1).map_or(0.0, |b| b[f]) * g * pan_r;
                peak[0] = peak[0].max(l.abs());
                peak[1] = peak[1].max(r.abs());
                mix[f * TAP_CHANNELS] += l * direct;
                mix[f * TAP_CHANNELS + 1] += r * direct;
                aux[f * TAP_CHANNELS] += l * send;
                aux[f * TAP_CHANNELS + 1] += r * send;
                if stems {
                    stem[f * CHANNELS * 2 + ch * 2] += l;
                    stem[f * CHANNELS * 2 + ch * 2 + 1] += r;
                }
            }
            self.shared.note_strip_peak(Strip::Channel(ch), peak);
        }
        self.lane_notes = lane_notes;
    }

    /// Push the recorder's mix, whole frames only.
    ///
    /// **Only whole frames**, exactly as `ivory-record`'s capture ring insists:
    /// a ring-full event that split a frame would swap the take's channels from
    /// that point on and nothing downstream could detect it. When the ring is
    /// full the frames are counted and dropped, because a short take that says
    /// so beats a stalled audio thread.
    fn push_tap(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        let want = frames * TAP_CHANNELS;
        let room = (self.tap.slots() / TAP_CHANNELS) * TAP_CHANNELS;
        let n = want.min(room);
        if n > 0 {
            let _ = self.tap.push_entire_slice(&self.tap_scratch[..n]);
        }
        if n < want {
            self.shared
                .tap_dropped
                .fetch_add(((want - n) / TAP_CHANNELS) as u64, Ordering::Relaxed);
        }
    }

    fn publish_meters(
        &self,
        frames: usize,
        peak_l: f32,
        peak_r: f32,
        sumsq_l: f64,
        sumsq_r: f64,
        clipped: bool,
    ) {
        let s = &self.shared;
        s.peak_l.fetch_max(peak_l.to_bits(), Ordering::Relaxed);
        s.peak_r.fetch_max(peak_r.to_bits(), Ordering::Relaxed);
        if frames > 0 {
            let n = frames as f64;
            s.rms_l
                .store(((sumsq_l / n).sqrt() as f32).to_bits(), Ordering::Relaxed);
            s.rms_r
                .store(((sumsq_r / n).sqrt() as f32).to_bits(), Ordering::Relaxed);
        }
        if clipped {
            s.clipped.store(true, Ordering::Relaxed);
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The recorder's read end
// ───────────────────────────────────────────────────────────────────────────

/// The recorder's half of the tap. `Send`, taken once, moved to the writer
/// thread.
///
/// It has to be a separate handle rather than a method on [`Engine`], because
/// the two live on different threads and always will: `Engine` owns a
/// `cpal::Stream` and a VST3 instance and is therefore pinned to the UI thread,
/// while `record.rs`'s writer runs on its own so that a file write can never
/// block a frame.
///
/// **It survives a plugin swap.** The ring belongs to the engine, not to the
/// instrument: [`RecorderTap::channels`] is fixed at [`Engine::start`] and a
/// different instrument is mapped into it (see [`map_frame`]) rather than
/// changing the width of a take that is already rolling.
pub struct RecorderTap {
    rx: Consumer<f32>,
    channels: usize,
    sample_rate: u32,
    dropped: Arc<Shared>,
}

impl RecorderTap {
    /// Build a tap over a ring the caller owns the other end of.
    ///
    /// For tests in sibling modules — `record.rs`'s writer holds a `RecorderTap`
    /// and its behaviour around one (draining it even when the instrument is
    /// not being recorded, counting its losses only when it is) cannot be
    /// tested against a `None`. Ask me how I know: the first version of that
    /// test passed with the fix removed, because there was no tap in it.
    /// Returns the tap, the write end, and a closure that counts a frame the
    /// ring could not take — the three things the engine wires together.
    ///
    /// The closure rather than the counter itself, so `Shared` stays private:
    /// a test in a sibling module needs to BUMP the count, not to see the type
    /// that holds it.
    #[cfg(test)]
    pub(crate) fn for_test(slots: usize, channels: usize) -> (Self, Producer<f32>, impl Fn(u64)) {
        let shared = Arc::new(Shared::new());
        let counter = Arc::clone(&shared);
        let (tx, rx) = RingBuffer::<f32>::new(slots);
        (
            Self {
                rx,
                channels,
                sample_rate: 48_000,
                dropped: shared,
            },
            tx,
            move |frames: u64| {
                counter.tap_dropped.fetch_add(frames, Ordering::Relaxed);
            },
        )
    }
    /// Append everything available. Interleaved; returns frames moved.
    pub fn drain(&mut self, out: &mut Vec<f32>) -> usize {
        let available = self.rx.slots();
        let whole = (available / self.channels) * self.channels;
        if whole == 0 {
            return 0;
        }
        let Ok(chunk) = self.rx.read_chunk(whole) else {
            return 0;
        };
        let (a, b) = chunk.as_slices();
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        chunk.commit_all();
        whole / self.channels
    }

    /// Append at most `frames` frames. Interleaved; returns frames moved.
    ///
    /// The bounded form, which is what the recorder wants: the take's length is
    /// decided by the INPUT device's timestamps, so each written block asks for
    /// exactly as many instrument frames as the block it is being summed into.
    /// Draining everything available would run the two clocks against each
    /// other and the ring would slowly empty or overflow depending on which
    /// device was faster.
    pub fn drain_frames(&mut self, frames: usize, out: &mut Vec<f32>) -> usize {
        let want = frames.saturating_mul(self.channels);
        let whole = (self.rx.slots().min(want) / self.channels) * self.channels;
        if whole == 0 {
            return 0;
        }
        let Ok(chunk) = self.rx.read_chunk(whole) else {
            return 0;
        };
        let (a, b) = chunk.as_slices();
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        chunk.commit_all();
        whole / self.channels
    }

    /// Discard everything buffered and zero [`RecorderTap::dropped`].
    ///
    /// **Call this at the instant the take starts writing**, exactly as
    /// `ivory-record`'s `LevelTracker::arm` is called, and for the same reason.
    /// The ring runs from the moment the engine opens, so by the time a user has
    /// chosen an instrument, waited out its five-second warm-up and pressed
    /// Record, it holds minutes-old monitor audio and has overflowed several
    /// times over. Without this the first take of every session reports tens of
    /// thousands of dropped frames and `take.json` calls a perfectly good
    /// recording short. The probe measures exactly that: 80384 frames "lost"
    /// before a single note was played.
    ///
    /// It is deliberately not automatic. The ring keeps running between takes so
    /// that a pre-roll has something to reach back into, which is the whole
    /// reason it is four seconds deep rather than four blocks.
    pub fn arm(&mut self) {
        let waiting = self.rx.slots();
        if waiting > 0 {
            if let Ok(chunk) = self.rx.read_chunk(waiting) {
                chunk.commit_all();
            }
        }
        self.dropped.tap_dropped.store(0, Ordering::Relaxed);
    }

    /// Frames the ring could not hold. **Non-zero means the take is short**,
    /// and `take.json` has to say so — a silently short take is the worst
    /// outcome the recorder has.
    pub fn dropped(&self) -> u64 {
        self.dropped.tap_dropped.load(Ordering::Relaxed)
    }

    /// Always [`TAP_CHANNELS`], for the life of the engine.
    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The engine
// ───────────────────────────────────────────────────────────────────────────

/// The UI thread's end of one slot's plugin handoff.
///
/// Kept as a pair because the pair is the protocol: nothing may go in until
/// whatever is in there has come out, and holding the two halves together is
/// what makes that one function ([`Engine::hand_off`]) instead of a convention.
struct Handoff {
    to_audio: Producer<PluginBox>,
    from_audio: Consumer<PluginBox>,
}

/// How many routes the desk can carry at once.
///
/// A ceiling rather than a `Vec`, because this table is read on the audio
/// thread: sixty-four is four routes per channel on a sixteen-channel desk,
/// past anything a person builds by hand, and the whole array is 512 bytes.
pub const MAX_ROUTES: usize = 64;

/// Pack a route into one word the audio thread can read atomically.
///
/// `from` and `target` are channel indices (0..64 fits a byte with room),
/// `hardware` says which kind of destination it is, and `stereo` only means
/// anything for hardware. Bit 31 marks the word as occupied, so a zeroed
/// slot can never read as "channel 0 to channel 0".
fn pack_route(from: usize, target: usize, hardware: bool, stereo: bool) -> u32 {
    (from as u32 & 0xff)
        | ((target as u32 & 0xff) << 8)
        | (u32::from(hardware) << 16)
        | (u32::from(stereo) << 17)
        | (1 << 31)
}

/// The inverse of [`pack_route`]. `None` for an empty slot.
fn unpack_route(w: u32) -> Option<(usize, usize, bool, bool)> {
    if w & (1 << 31) == 0 {
        return None;
    }
    Some((
        (w & 0xff) as usize,
        ((w >> 8) & 0xff) as usize,
        w & (1 << 16) != 0,
        w & (1 << 17) != 0,
    ))
}

/// The monitor engine: one output stream, [`SLOTS`] optional instruments layered
/// on top of each other, one metronome.
///
/// Everything instrument-shaped is indexed by slot and **an out-of-range slot is
/// never a panic**: it is a no-op or an `Err`, because these are called straight
/// from UI code and this app's panic hook turns a panic into a dialog and
/// `exit(1)`. A menu that got its arithmetic wrong should misbehave, not take
/// the session with it.
///
/// `!Send` and `!Sync`, because `cpal::Stream` is. That is not a limitation to
/// work around — it is what makes the `RefCell` below sound and what pins the
/// VST3 initialisation-context calls to one thread, which is where VST3 wants
/// them.
pub struct Engine {
    // Declared first so it is dropped first: stopping the device before
    // anything it might still be reading goes away. Held, never read — the
    // whole value of this field is its `Drop`.
    #[allow(
        dead_code,
        reason = "held so the stream lives; dropping it stops audio"
    )]
    stream: Option<cpal::Stream>,
    /// The device the stream above plays through — kept for ASIO duplex.
    ///
    /// cpal keeps ASIO stream state per `Device` OBJECT, and the driver
    /// underneath has exactly one buffer set: `ASIOCreateBuffers` from a
    /// SECOND `Device` on the same driver destroys the first one's buffers
    /// (asio-sys says so in `prepare_output_stream`'s own doc). So the input
    /// must open through THIS object, not through a lookalike from a fresh
    /// enumeration — that lookalike is how the output went silent the moment
    /// the input opened, with no error anywhere. Dropped after `stream` by
    /// declaration order.
    out_device_obj: Option<cpal::Device>,
    /// **The OFFLINE renderer, when this engine has no device at all.** An
    /// export engine: the same rings, the same racks, the same push surface
    /// — `load_insert`, `set_clips`, everything — but the render is PULLED
    /// by [`Engine::render_offline`] as fast as the machine goes instead of
    /// being pulled by a callback at the speed of sound.
    offline: Option<Box<Renderer>>,
    output: OutputInfo,
    shared: Arc<Shared>,
    timebase: Timebase,

    /// The sender half of the MIDI ring. `RefCell` rather than a `Mutex`: this
    /// type cannot cross threads, so there is nothing to lock against, and
    /// `send_midi` must not block. A `RefCell` borrow is a counter check with no
    /// atomics and no waiting.
    midi: RefCell<Producer<MidiEvent>>,
    /// The hardware fast lane's write end, waiting for the desktop to move it
    /// into the app-lifetime `midi::MidiFastLane` (once, at engine start).
    hw_midi_tx: Option<Producer<MidiEvent>>,

    /// The UI thread's ends of every rack's hand-offs. See `Rack`.
    insert_handoff: [[Handoff; INSERTS]; RACK_SEATS],
    /// The clip's two rings, in the plugin handoff's exact shape and for its
    /// exact reason: the audio thread must never free a hundred megabytes of
    /// samples, so the displaced clip comes BACK here and is dropped by the
    /// thread that sent its replacement.
    maps_to_audio: Producer<Maps>,
    maps_from_audio: Consumer<Maps>,
    auto_to_audio: Producer<ivory_ui::automation::Automation>,
    auto_from_audio: Consumer<ivory_ui::automation::Automation>,
    track_to_audio: Producer<Vec<Placed>>,
    track_from_audio: Consumer<Vec<Placed>>,
    midi_to_audio: Producer<Vec<PlacedMidi>>,
    midi_from_audio: Consumer<Vec<PlacedMidi>>,
    pad_to_audio: Producer<PadKit>,
    pad_from_audio: Consumer<PadKit>,
    preview_to_audio: Producer<Option<Arc<ivory_record::decode::Clip>>>,
    preview_from_audio: Consumer<Option<Arc<ivory_record::decode::Clip>>>,
    cap_rx: Vec<Consumer<f32>>,
    stem_rx: Consumer<f32>,
    scope_rx: Consumer<f32>,
    /// The ReaStream send ring's read end, handed to the sender thread once per
    /// engine when streaming is switched on. `None` after it has been taken.
    reastream_rx: Option<Consumer<f32>>,
    /// What is in each of them, for the desk to name.
    inserts_loaded: [[Option<Loaded>; INSERTS]; RACK_SEATS],

    tap: Option<RecorderTap>,
    fault: Arc<Mutex<Option<String>>>,

    /// The open editor window for each loaded insert, if the user has asked
    /// for one. **An insert is a plugin with a window like any other**, and
    /// until this existed there was no way to reach one: a reverb loaded into
    /// a channel came up, worked, and could not be adjusted — every control
    /// it has is inside a window nothing opened.
    ///
    /// The engine owns them because the engine owns the plugins, and the two
    /// have a lifetime rule: an editor must never outlive the instance whose
    /// controller built its view. Declared **before** `insert_editor_handles`
    /// so that even a plain `drop(engine)` releases every window and its
    /// `IPlugView` before the controller references that made them.
    insert_editors: [[Option<ivory_host::Editor>; INSERTS]; RACK_SEATS],
    /// Each loaded insert's edit controller, taken in [`Engine::load_insert`]
    /// **before** the instance leaves for the audio thread — the one moment
    /// there is an `&Instance` on this thread. See [`Engine::open_editor`].
    insert_editor_handles: [[Option<ivory_host::EditorHandle>; INSERTS]; RACK_SEATS],
    /// Each loaded insert's processor reference, taken beside the editor's and
    /// for the same reason: after the handoff there is no `&Instance` to ask.
    /// This is what lets a bay's settings survive a relaunch — no insert has
    /// ever had one before, so every effect's knobs and every bay instrument's
    /// preset died with the process.
    insert_state_handles: [[Option<ivory_host::StateHandle>; INSERTS]; RACK_SEATS],
    /// How to ask each seated plugin its latency again, from THIS thread.
    ///
    /// The audio thread holds the instance and may not make the call; it can
    /// only be told the figure has gone stale. See `poll_latency`.
    insert_latency_handles: [[Option<ivory_host::LatencyHandle>; INSERTS]; RACK_SEATS],
    /// Why a bay's saved state was not restored, if it was not.
    ///
    /// A stale or hand-edited blob must not stop the plugin loading — the
    /// user wants their piano more than they want their preset — so the load
    /// continues with the plugin's defaults and the reason lands here, where
    /// [`Engine::insert_state_error`] can put it in front of someone.
    /// Silently ignoring it is how "it keeps forgetting my piano" becomes
    /// unreportable.
    insert_state_errors: [[Option<String>; INSERTS]; RACK_SEATS],

    /// Meter hold, decayed on the UI thread because that is where it is read.
    hold: Cell<(f32, f32)>,
    hold_at: Cell<Instant>,
}

/// Sum an interleaved stereo buffer to mono, in place.
///
/// **A no-op when it is off**, and the branch is outside the loop: this runs
/// on the audio callback for every frame that leaves the desk.
///
/// Halved rather than summed. Two identical channels added together are 6 dB
/// louder than either, so a fold that summed would make every mono check
/// clip and teach the user that mono is loud rather than what mono does to
/// their image.
fn fold_mono(buf: &mut [f32], on: bool) {
    if !on {
        return;
    }
    for f in buf.chunks_exact_mut(TAP_CHANNELS) {
        let m = (f[0] + f[1]) * 0.5;
        f[0] = m;
        f[1] = m;
    }
}

impl Engine {
    /// Open the default output device (or `out_device` by name) and start it.
    ///
    /// No plugin yet: the metronome works from here on, and an instrument is
    /// loaded into the running stream later.
    pub fn start(out_device: Option<&str>) -> Result<Self, String> {
        Self::start_with(out_device, Timebase::new())
    }

    /// As [`Engine::start`], with a buffer size and a rate the user chose.
    pub fn start_sized(
        out_device: Option<&str>,
        timebase: Timebase,
        buffer_frames: Option<u32>,
        sample_rate: Option<u32>,
    ) -> Result<Self, String> {
        Self::start_inner(out_device, timebase, buffer_frames, sample_rate, false)
    }

    /// As [`Engine::start`], sharing the recorder's timebase.
    ///
    /// Worth doing: it is what makes [`Engine::count_in_downbeat_ns`] comparable
    /// with the MIDI tap's stamps and with the take's `T0`, so a take can start
    /// on the downbeat the player actually heard rather than on the UI frame
    /// that noticed.
    pub fn start_with(out_device: Option<&str>, timebase: Timebase) -> Result<Self, String> {
        Self::start_inner(out_device, timebase, None, None, false)
    }

    /// An engine with NO device: everything an [`Engine`] is — racks,
    /// clips, pads, desk — pulled by hand for a full-speed export.
    pub fn offline(rate: u32, timebase: Timebase) -> Result<Self, String> {
        Self::start_inner(None, timebase, None, Some(rate.max(8_000)), true)
    }

    /// One rendered block of an OFFLINE engine, `out` interleaved stereo.
    /// Does nothing on a live engine — the callback owns that render.
    pub fn render_offline(&mut self, out: &mut [f32]) {
        let Some(r) = self.offline.as_mut() else {
            return;
        };
        // **Split the same way the device callback is**, and for the same
        // reason: the varispeed path halves what one pass can serve, and a
        // buffer past that would be rendered from too little program and come
        // out holding the last frame over and over. The exporter hands over a
        // kilo-frame at a time so this never bites today; it is here because
        // the day it changes, the symptom is a stutter in a finished file.
        let ch = 2usize;
        let mut done = 0usize;
        while done + ch <= out.len() {
            let cap = r.max_callback_frames();
            let frames = ((out.len() - done) / ch).min(cap);
            if frames == 0 {
                break;
            }
            let n = frames * ch;
            r.render(&mut out[done..done + n], 0, 0);
            done += n;
        }
    }

    fn start_inner(
        out_device: Option<&str>,
        timebase: Timebase,
        asked_buffer: Option<u32>,
        asked_rate: Option<u32>,
        offline: bool,
    ) -> Result<Self, String> {
        // OFFLINE: no host, no device, no stream — two channels at the
        // asked rate, and the render is pulled by hand.
        let dev = if offline {
            None
        } else {
            // The system the user chose, not whatever the platform hands
            // out. One process, ONE HOST: `ivory_record::audio` owns the
            // instance because that is where the input opens, and the two
            // sides going through separate host instances is exactly the
            // ASIO teardown bug — a second instance's empty driver-guard
            // lets the SDK unload the driver the first one is playing
            // through. `with_host` is the shared, cached instance.
            let (device, name, supported, host_is_asio) =
                ivory_record::audio::with_host(|host| -> Result<_, String> {
                    let device = resolve_output(host, out_device)?;
                    let name = device
                        .name()
                        .unwrap_or_else(|_| "unnamed output device".to_string());
                    let supported = device
                        .default_output_config()
                        .map_err(|e| format!("{name}: no default output config ({e})"))?;
                    // The backend actually in use, not the setting: the
                    // setting can name a system this build lacks, and the
                    // buffer arithmetic must follow the real driver stack.
                    let asio = host.id().name() == "ASIO";
                    Ok((device, name, supported, asio))
                })?;
            Some((device, name, supported, host_is_asio))
        };
        let channels = dev.as_ref().map_or(2, |(_, _, c, _)| c.channels());
        // **The rate is asked for, not imposed.** Setup offers only what the
        // INPUT device supports, and this is the output — a different device
        // with its own ranges — so the request is honoured when this device can
        // also do it and dropped when it cannot. Dropped and not refused:
        // failing to open the engine because the monitor path disagrees about a
        // rate would take the whole app's sound away over a preference.
        //
        // Worth honouring at all because `AudioStatus::rates_disagree` is a
        // warning with no fix otherwise: the writer drains the instrument's
        // ring at the input's rate while the engine fills it at the output's,
        // and the take drifts.
        let dev = dev.map(|(device, name, supported, asio)| {
            let supported = match asked_rate {
                // Narrowed through a RANGE rather than rebuilt:
                // `with_sample_rate` belongs to the range, and taking it
                // from one the device actually reported is what makes the
                // result a configuration the device has agreed to rather
                // than one assembled from three numbers that each looked
                // right.
                Some(want) => device
                    .supported_output_configs()
                    .ok()
                    .and_then(|mut cs| {
                        cs.find(|c| {
                            c.channels() == channels
                                && c.sample_format() == supported.sample_format()
                                && c.min_sample_rate().0 <= want
                                && want <= c.max_sample_rate().0
                        })
                    })
                    .map(|c| c.with_sample_rate(cpal::SampleRate(want)))
                    .unwrap_or(supported),
                None => supported,
            };
            (device, name, supported, asio)
        });
        let rate = dev.as_ref().map_or_else(
            || asked_rate.unwrap_or(48_000),
            |(_, _, s, _)| s.sample_rate().0,
        );
        let name = dev
            .as_ref()
            .map_or_else(|| "offline".to_owned(), |(_, n, _, _)| n.clone());
        if channels == 0 || rate == 0 {
            return Err(format!("{name} reports {channels} channels at {rate} Hz"));
        }
        let (buffer_size, buffer_frames) = match dev.as_ref() {
            Some((_, _, supported, asio)) => {
                pick_buffer(supported.buffer_size(), asked_buffer, *asio)
            }
            // Offline renders in MAX_BLOCK bites; the "buffer" is a label.
            None => (cpal::BufferSize::Default, None),
        };

        let shared = Arc::new(Shared::new());
        let fault = Arc::new(Mutex::new(None));
        let click = Click::load(f64::from(rate))?;

        // Every ring is allocated here and never again.
        let (midi_tx, midi_rx) = RingBuffer::<MidiEvent>::new(1024);
        let (hw_midi_tx, hw_midi_rx) = RingBuffer::<MidiEvent>::new(1024);
        let tap_frames = (DEFAULT_RING_SECONDS * rate as f32) as usize;
        let (tap_tx, tap_rx) = RingBuffer::<f32>::new(tap_frames.max(4096) * TAP_CHANNELS);

        // **A rack per channel, master last, three hand-off pairs each.**
        // Thirty-nine small rings rather than one shared pair, because the
        // ring an instance travels on is what says where it belongs, and a
        // pair carrying an index would be the audio thread trusting a number
        // a UI thread wrote. See `PluginBox`. Two and two per bay because at
        // most one handoff per bay is ever in flight — `hand_off_insert`
        // waits for the return before it starts another — and the spare
        // element is what lets `Rack::swap` check for room before it commits.
        let mut rack_ends: Vec<[Handoff; INSERTS]> = Vec::with_capacity(RACK_SEATS);
        let racks: [Rack; RACK_SEATS] = std::array::from_fn(|_| {
            let mut ends: Vec<Handoff> = Vec::with_capacity(INSERTS);
            let mut incoming = Vec::with_capacity(INSERTS);
            let mut retiring = Vec::with_capacity(INSERTS);
            for _ in 0..INSERTS {
                let (to_audio, inc) = RingBuffer::<PluginBox>::new(2);
                let (ret, from_audio) = RingBuffer::<PluginBox>::new(2);
                ends.push(Handoff {
                    to_audio,
                    from_audio,
                });
                incoming.push(inc);
                retiring.push(ret);
            }
            let mut ends = ends.into_iter();
            rack_ends.push(std::array::from_fn(|_| {
                ends.next().expect("one Handoff per insert")
            }));
            let mut inc = incoming.into_iter();
            let mut ret = retiring.into_iter();
            Rack {
                slots: std::array::from_fn(|_| PluginBox(None)),
                incoming: std::array::from_fn(|_| inc.next().expect("one per insert")),
                retiring: std::array::from_fn(|_| ret.next().expect("one per insert")),
            }
        });
        let mut rack_ends = rack_ends.into_iter();
        let insert_handoff: [[Handoff; INSERTS]; RACK_SEATS] =
            std::array::from_fn(|_| rack_ends.next().expect("one set per channel"));
        // The effect across the bus used to have a pair of its own here. It
        // does not any more: the bus is a channel like the others and its
        // effect is `insert_handoff[Strip::Fx.index()]`, so what was left was
        // two rings built on every engine start with nothing on either end.

        let dev_ch = channels as usize;
        let widest = dev_ch.max(TAP_CHANNELS);
        // The clip's pair: capacity two, one in flight plus the spare that
        // lets the swap check for room, exactly as a slot's.
        let (track_to_audio, track_incoming) = RingBuffer::<Vec<Placed>>::new(2);
        let (track_retiring, track_from_audio) = RingBuffer::<Vec<Placed>>::new(2);
        let (midi_to_audio, midi_incoming) = RingBuffer::<Vec<PlacedMidi>>::new(2);
        let (midi_retiring, midi_from_audio) = RingBuffer::<Vec<PlacedMidi>>::new(2);
        // The maps' pair. A map is a growable `Vec` and the callback may not
        // free one, so it travels the same road the clip list does: in on one
        // ring, the displaced one back out on the other, dropped on the UI
        // thread.
        let (maps_to_audio, maps_incoming) = RingBuffer::<Maps>::new(2);
        let (maps_retiring, maps_from_audio) = RingBuffer::<Maps>::new(2);
        // The automation's pair, same protocol, same reason: curves are
        // `Vec`s and the callback may not free one.
        let (auto_to_audio, auto_incoming) =
            RingBuffer::<ivory_ui::automation::Automation>::new(2);
        let (auto_retiring, auto_from_audio) =
            RingBuffer::<ivory_ui::automation::Automation>::new(2);
        // The drumpad kit's pair, same capacity, same reason.
        let (pad_to_audio, pad_incoming) = RingBuffer::<PadKit>::new(2);
        let (pad_retiring, pad_from_audio) = RingBuffer::<PadKit>::new(2);
        let (preview_to_audio, preview_incoming) =
            RingBuffer::<Option<Arc<ivory_record::decode::Clip>>>::new(2);
        let (preview_retiring, preview_from_audio) =
            RingBuffer::<Option<Arc<ivory_record::decode::Clip>>>::new(2);
        // The capture rings: four seconds each, stereo. The host drains every
        // UI frame and would have to stall for seconds to lose a sample; a
        // full ring drops and says so in `cap_dropped`.
        let mut cap_tx = Vec::with_capacity(INPUTS);
        let mut cap_rx = Vec::with_capacity(INPUTS);
        for _ in 0..INPUTS {
            let (tx, rx) = RingBuffer::<f32>::new(rate as usize * 2 * 4);
            cap_tx.push(tx);
            cap_rx.push(rx);
        }
        // The stem tap: four seconds of every lane in stereo. Idle it costs
        // a ring nobody fills.
        let (stem_tx, stem_rx) = RingBuffer::<f32>::new(rate as usize * CHANNELS * 2 * 4);
        // The scope's third of a second of stereo.
        let (scope_tx, scope_rx) = RingBuffer::<f32>::new(1 << 15);
        // The ReaStream send ring, the same generous third of a second so the
        // sender thread's scheduling jitter never starves or overruns it.
        let (reastream_tx, reastream_rx) = RingBuffer::<f32>::new(1 << 15);
        let renderer = Renderer {
            shared: Arc::clone(&shared),
            timebase,
            rate: f64::from(rate),
            dev_channels: dev_ch,
            // Sized for one whole render block, here and never again — this is
            // the buffer that makes summing the desk allocation-free whether
            // its channels fill up now or in an hour.
            mix: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            midi: midi_rx,
            hw_midi: hw_midi_rx,
            pending: None,
            notes: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            clips: Vec::new(),
            midi_clips: Vec::new(),
            midi_cursor: Vec::new(),
            active_clip: Vec::with_capacity(256),
            lane_notes: Vec::with_capacity(MAX_EVENTS_PER_BLOCK * 2),
            pos: 0,
            lane_gain: [1.0; CHANNELS],
            seen_req: 0,
            was_rolling: false,
            pending_flush: false,
            clip_parked: false,
            sounding: 0,
            bar_clock: BarClock::default(),
            maps: Maps::default(),
            maps_incoming,
            maps_retiring,
            auto: AutoNow::default(),
            pdc: (0..CHANNELS).map(|_| DelayLine::new()).collect(),
            automation: ivory_ui::automation::Automation::default(),
            auto_incoming,
            auto_retiring,
            track_incoming,
            track_retiring,
            midi_incoming,
            midi_retiring,
            pad_kit: PadKit::default(),
            pad_incoming,
            pad_retiring,
            preview_incoming,
            preview_retiring,
            preview: None,
            preview_pos: 0,
            pad_voices: [PadVoice::default(); PADS],
            pad_hits: Vec::with_capacity(MAX_PAD_HITS_PER_BLOCK),
            pad_offs: Vec::with_capacity(MAX_PAD_HITS_PER_BLOCK * 2),
            pad_gain: 1.0,
            builtin: crate::dx7::Dx7::new(rate as f32),
            builtin_scratch: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            channel_gain: [1.0; CHANNELS],
            effects: crate::effects::Effects::new_send(rate as f32),
            master_effects: crate::effects::Effects::new(rate as f32),
            room_effects: crate::effects::Effects::new(rate as f32),
            room: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            input_dry: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            room_live: false,
            room_gain: 0.0,
            aux: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            racks,
            fx_in: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            fx_out: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            click_out: vec![0.0; MAX_BLOCK as usize],
            preview_out: vec![0.0; MAX_BLOCK as usize * 2],
            click_taped: vec![0.0; MAX_BLOCK as usize],
            fx_return_gain: 1.0,
            effect_params: crate::effects::Params::default(),
            controls: Vec::with_capacity(MAX_CONTROLS_PER_BLOCK),
            tap: tap_tx,
            tap_scratch: vec![0.0; MAX_CALLBACK_FRAMES * TAP_CHANNELS],
            frame: vec![0.0; widest],
            click,
            voice: Voice::default(),
            beats: Beats::new(f64::from(rate), 120.0),
            count_in_tail: 0,
            chunks: 0,
            metro_gain: 0.7,
            gain_coeff: gain_coefficient(f64::from(rate)),
            monitor: None,
            monitor_channels: 0,
            monitor_gain: [0.0; INPUTS],
            monitor_widths: [0; INPUTS],
            monitor_picks: ivory_record::audio::Picks::EVERYTHING,
            monitor_block: 0,
            monitor_scratch: vec![0.0; widest * 4096],
            cap_tx,
            stem_tx,
            stem_scratch: vec![0.0; MAX_BLOCK as usize * CHANNELS * 2],
            scope_tx,
            scope_scratch: vec![0.0; MAX_CALLBACK_FRAMES * TAP_CHANNELS],
            loudness: crate::loudness::Loudness::new(rate as f32),
            lufs_seen: 0,
            vs_phase: crate::varispeed::Phase::new(),
            vs_out: crate::varispeed::Stream::new(widest),
            vs_tap: crate::varispeed::Stream::new(TAP_CHANNELS),
            vs_scope: crate::varispeed::Stream::new(TAP_CHANNELS),
            vs_reastream: crate::varispeed::Stream::new(2),
            vs_in_phase: crate::varispeed::Phase::new(),
            vs_in: crate::varispeed::Stream::new(widest),
            vs_dev: vec![0.0; MAX_CALLBACK_FRAMES * widest],
            vs_dst: vec![0.0; MAX_CALLBACK_FRAMES * widest],
            vs_in_src: vec![0.0; MAX_CALLBACK_FRAMES * widest],
            vs_was_engaged: false,
            reastream_tx,
            reastream_scratch: vec![0.0; MAX_CALLBACK_FRAMES * 2],
            route_cache: [(0, 0, false, false, 0.0); MAX_ROUTES],
            route_direct: [1.0; CHANNELS],
            route_n: 0,
            routes_hw: false,
            send_accum: vec![0.0; CHANNELS * MAX_BLOCK as usize * TAP_CHANNELS],
            send_io: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            pad_out: vec![0.0; MAX_BLOCK as usize * PADS * 2],
            pad_io: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            stems_live: false,
        };

        let (stream, out_device_obj, offline_renderer) = match dev {
            Some((device, name, supported, _)) => {
                let config = cpal::StreamConfig {
                    channels,
                    sample_rate: supported.sample_rate(),
                    buffer_size,
                };
                let stream = build_stream(
                    &device,
                    &config,
                    supported.sample_format(),
                    renderer,
                    Arc::clone(&shared),
                    Arc::clone(&fault),
                )?;
                stream
                    .play()
                    .map_err(|e| format!("{name}: could not start the output stream ({e})"))?;
                // The device outlives the stream on purpose: ASIO's input
                // must open through this same object. See `out_device_obj`.
                (Some(stream), Some(device), None)
            }
            // Offline: the renderer stays HERE, pulled by hand.
            None => (None, None, Some(Box::new(renderer))),
        };
        shared.running.store(true, Ordering::Relaxed);

        Ok(Self {
            stream,
            out_device_obj,
            offline: offline_renderer,
            output: OutputInfo {
                device: name,
                channels,
                sample_rate: rate,
                buffer_frames,
            },
            shared: Arc::clone(&shared),
            timebase,
            midi: RefCell::new(midi_tx),
            hw_midi_tx: Some(hw_midi_tx),
            insert_handoff,
            maps_to_audio,
            maps_from_audio,
            auto_to_audio,
            auto_from_audio,
            track_to_audio,
            track_from_audio,
            midi_to_audio,
            midi_from_audio,
            pad_to_audio,
            pad_from_audio,
            preview_to_audio,
            preview_from_audio,
            cap_rx,
            stem_rx,
            scope_rx,
            reastream_rx: Some(reastream_rx),
            inserts_loaded: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            tap: Some(RecorderTap {
                rx: tap_rx,
                channels: TAP_CHANNELS,
                sample_rate: rate,
                dropped: shared,
            }),
            fault,
            insert_editors: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            insert_editor_handles: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            insert_state_handles: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            insert_latency_handles: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            insert_state_errors: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            hold: Cell::new((0.0, 0.0)),
            hold_at: Cell::new(Instant::now()),
        })
    }

    // ── the instruments ─────────────────────────────────────────────────────

    /// As [`Engine::load_plugin`], and restore `state` into the instrument
    /// before it is warmed up or handed over.
    ///
    /// `state` is whatever [`Engine::save_slot_state`] returned in an earlier
    /// session. Anything else — a truncated file, another plugin's blob, a
    /// hand-edited one — is refused by `ivory_host::state` before it can reach
    /// the plugin, and refusing it is **not** a failed load: the instrument
    /// comes up with its defaults and [`Engine::state_error`] says why.
    ///
    /// # Create, restore, warm up, and the order is the whole point
    ///
    /// The restore happens between `Instance::create` and the warm-up, and both
    /// of those neighbours matter:
    ///
    /// * **After create**, because there is nothing to restore into before it;
    /// * **Before the warm-up**, because a preset change makes an instrument
    ///   reload its samples. State that arrives afterwards starts a NEW load
    ///   immediately after `ivory_host::ready` finished waiting for the old
    ///   one, so the gate says Ready and the instrument then goes quiet — the
    ///   exact failure that module exists to prevent, reintroduced by ordering.
    ///   It also means the first thing the warm-up probe renders is the default
    ///   piano rather than the user's.
    ///
    /// `Instance::load_state` refuses a restore after the instance has rendered
    /// anything, so this ordering is enforced by the host rather than by
    /// convention.
    /// Put a user effect across the effects bus, or take it away with `None`.
    ///
    /// **No warm-up, unlike an instrument.** `ready::warm_up` plays a note and
    /// waits to hear something, which is the right gate for a thing that makes
    /// sound and a meaningless one for a thing that changes it: an effect
    /// handed silence correctly produces silence, so the gate would fail every
    /// reverb ever written.
    ///
    /// Blocking, like every other load here — `Module::open` runs somebody
    /// else's initialiser — so the host calls it after a frame, never inside
    /// one.
    pub fn load_insert(
        &mut self,
        strip: usize,
        slot: usize,
        bundle: Option<&Path>,
        state: Option<&[u8]>,
    ) -> Result<Option<Loaded>, String> {
        // **The whole seat map, pads included.** This capped at `STRIPS`,
        // which is the channels + master + click/fx — every PAD seat lives
        // at `STRIPS + 1 + pad` and was silently refused here, so a pad's
        // FX bay stored a plugin, the host dispatched it, and the engine
        // dropped it on the floor: "drum FX slots do nothing". `mix_pads`
        // already runs each pad's rack; they just never got filled.
        if strip >= RACK_SEATS || slot >= INSERTS {
            return Ok(None);
        }
        let Some(bundle) = bundle else {
            // Window first, then the controller and state references, then the
            // instance.
            self.close_insert_editor(strip, slot);
            if let Some(h) = self
                .insert_editor_handles
                .get_mut(strip)
                .and_then(|r| r.get_mut(slot))
            {
                *h = None;
            }
            if let Some(h) = self
                .insert_state_handles
                .get_mut(strip)
                .and_then(|r| r.get_mut(slot))
            {
                *h = None;
            }
            if let Some(h) = self
                .insert_latency_handles
                .get_mut(strip)
                .and_then(|r| r.get_mut(slot))
            {
                *h = None;
            }
            // An emptied bay delays nothing. Left standing, its old figure
            // would hold every other lane late forever, waiting on a plugin
            // that is no longer there.
            Rack::set_latency(&self.shared, strip, slot, 0);
            // A refused UNLOAD changes nothing worth reporting: the box is
            // empty, and the seat keeps its tenant until the callback drains
            // the ring or the engine is torn down.
            let _ = self.hand_off_insert(strip, slot, PluginBox(None));
            self.inserts_loaded[strip][slot] = None;
            if let Some(e) = self
                .insert_state_errors
                .get_mut(strip)
                .and_then(|r| r.get_mut(slot))
            {
                *e = None;
            }
            return Ok(None);
        };
        let module = Module::open(bundle)?;
        let classes = module.audio_modules();
        let class = classes
            .first()
            .ok_or_else(|| format!("{} has no Audio Module Class", bundle.display()))?
            .clone();
        // **Any plugin loads in any bay, and there is no refusal left to
        // word.** The rack used to refuse instruments the way slots refuse
        // effects; the desk stopped distinguishing, so a bay decides WHAT the
        // plugin is instead of whether it may exist: a voice is fed the
        // block's notes and replaces the channel, an effect is fed the channel
        // and transforms it. See `Hosted::voice` and `run_rack`.
        let voice = class.kind() == ivory_host::scan::Kind::Instrument;
        let setup = Setup {
            sample_rate: f64::from(self.output.sample_rate),
            max_block: MAX_BLOCK,
        };
        let mut inst = Instance::create(&module, &class, setup)?;
        if !voice && inst.audio_inputs().is_empty() {
            // Still a real refusal for an EFFECT: with no audio input there is
            // nothing to send it and it would answer with silence for ever. A
            // voice has no audio input by design.
            return Err(format!(
                "{} has no audio input, so there is nothing to send to it",
                class.name
            ));
        }
        let channels = inst
            .audio_outputs()
            .first()
            .map(|b| b.channels.max(0) as usize)
            .unwrap_or(0);
        if channels == 0 {
            return Err(format!("{} has no audio output channels", class.name));
        }
        // **The preset, BEFORE the warm-up**, for the reason the slot path
        // documents: a preset change makes a sampled instrument reload, and
        // state that arrives after the gate has closed means the gate waited
        // for the wrong load. `load_state` also refuses a restore once the
        // instance has rendered, so this is the only moment it can happen.
        let state_error = match state {
            Some(bytes) => inst.load_state(bytes).err(),
            None => None,
        };
        // **A voice warms up like the slot it used to be.** The gate exists so
        // a sampled piano is not handed to the callback while it still renders
        // silence; nothing about moving into a bay changed that. An effect
        // needs no warm-up — see the note on `load` — and gets none.
        if voice {
            let gate = ivory_host::ready::warm_up(&mut inst, ivory_host::Policy::default());
            if gate.state() == ivory_host::ReadyState::Failed {
                return Err(gate
                    .reason()
                    .unwrap_or("the instrument failed to warm up")
                    .to_string());
            }
        }
        // Processing on, or the plugin is active and refuses every block.
        inst.set_processing(true)?;
        // **The editor's and the state's only chance to get a reference**, for
        // exactly the reason spelled out in `load_plugin_with_state`: one line
        // below, `inst` goes into a `Hosted`, into a `PluginBox`, and across a
        // ring into the audio callback, and after that there is no `&Instance`
        // on this thread and no safe way to make one. The state handle is what
        // stops an effect's settings — or a bay instrument's preset — dying at
        // every relaunch, which until now they did.
        let editor_handle = inst.editor_handle();
        let state_handle = inst.state_handle();
        // The third of the three cloned pointers this host keeps once an
        // instance has gone to the audio thread. See `Instance::latency_handle`.
        //
        // **A VOICE gets none of it**, and so is never polled and never
        // counted: a voice is where the signal begins, so its own delay
        // holds nothing else up, and counting it would push every other lane
        // late to match a delay nobody can hear.
        let latency_handle = (!voice).then(|| inst.latency_handle());
        // **After activation and after the state was loaded**, which is why
        // it is read here and not where the instance was made: VST3 says the
        // figure means nothing before that, a lookahead limiter has nothing
        // to report until it knows its block size, and a preset restored
        // from a saved state can change the answer.
        let latency_now = if voice { 0 } else { inst.latency_samples() };
        let loaded = Loaded {
            bundle: bundle.to_path_buf(),
            class: class.name.clone(),
            vendor: module.vendor().to_owned(),
            channels: u16::try_from(channels).unwrap_or(u16::MAX),
            sample_rate: self.output.sample_rate,
        };
        // Whatever was in THIS bay is going away, so its window and then its
        // controller and state references go first — the lifetime rule from
        // `editors`.
        // `hand_off_insert` is where the old instance is dropped, and dropping
        // it terminates the controller the old handle points at.
        self.close_insert_editor(strip, slot);
        if let Some(h) = self
            .insert_editor_handles
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *h = None;
        }
        if let Some(h) = self
            .insert_state_handles
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *h = None;
        }
        if let Some(h) = self
            .insert_latency_handles
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *h = None;
        }
        if let Some(refused) = self.hand_off_insert(
            strip,
            slot,
            PluginBox(Some(Box::new(Hosted {
                inst,
                module,
                bufs: vec![vec![0.0; MAX_BLOCK as usize]; channels.max(TAP_CHANNELS)],
                channels,
                voice,
            }))),
        ) {
            // **The seat never took it, so nothing here may say it did.**
            // The refused box dies NOW, on this thread — references first,
            // the lifetime rule from `editors`: the fresh handles point at
            // its controller and must not outlive it. The ledger is left
            // alone; the bay's previous tenant is still seated in the
            // callback (its retire rides the same full ring), and naming it
            // is more honest than naming the plugin that just died.
            drop(editor_handle);
            drop(state_handle);
            drop(refused);
            return Err(format!(
                "the audio engine did not take {} — the seat's hand-off ring \
                 is full, which means the callback has stalled",
                class.name
            ));
        }
        if let Some(h) = self
            .insert_editor_handles
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *h = editor_handle;
        }
        if let Some(h) = self
            .insert_state_handles
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *h = Some(state_handle);
        }
        if let Some(h) = self
            .insert_latency_handles
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *h = latency_handle;
        }
        Rack::set_latency(&self.shared, strip, slot, latency_now);
        self.inserts_loaded[strip][slot] = Some(loaded.clone());
        if let Some(e) = self
            .insert_state_errors
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *e = state_error.map(|err| err.to_string());
        }
        Ok(Some(loaded))
    }

    /// Why the state handed to [`Engine::load_insert`] was not restored into
    /// this bay.
    ///
    /// `None` is the normal answer: nothing was offered, or it was restored.
    /// `Some` means the plugin came up with its defaults and the user's
    /// preset did not survive — worth saying out loud, because the symptom is
    /// "it keeps forgetting my piano" and nothing else in the app would know.
    pub fn insert_state_error(&self, strip: usize, slot: usize) -> Option<&str> {
        self.insert_state_errors.get(strip)?.get(slot)?.as_deref()
    }

    /// What is in one insert, if anything.
    pub fn insert(&self, strip: usize, slot: usize) -> Option<&Loaded> {
        self.inserts_loaded.get(strip)?.get(slot)?.as_ref()
    }

    /// An insert's half of [`Engine::hand_off`], and the same wait.
    ///
    /// Returns the box the seat did NOT take; `None` means the callback owns
    /// it now. `rtrb` hands a refused value back inside the push error — the
    /// exact trap `Rack::swap` documents — and this used to discard error and
    /// instance both: a freshly created plugin died right here, silently, and
    /// `load_insert` carried on installing its editor and state handles as if
    /// it were seated. The desk then named a plugin that existed nowhere, and
    /// the auto-open built a view on a terminated controller.
    #[must_use]
    fn hand_off_insert(&mut self, strip: usize, slot: usize, next: PluginBox) -> Option<PluginBox> {
        let Some(pair) = self
            .insert_handoff
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        else {
            return Some(next);
        };
        while pair.from_audio.pop().is_ok() {}
        if let Err(rtrb::PushError::Full(refused)) = pair.to_audio.push(next) {
            // The seat's two-deep ring already holds two undelivered boxes,
            // which means the callback has stalled. Handing the box back is
            // what lets the caller refuse honestly.
            return Some(refused);
        }
        let deadline = Instant::now() + RETIRE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(old) = pair.from_audio.pop() {
                drop(old);
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    // ── the plugins' own editors ────────────────────────────────────────────

    /// One insert's own window: open it, or raise it if it is already up.
    ///
    /// The same handle trick as [`Engine::open_editor`] — see the long comment
    /// in `load_plugin_with_state` for why a reference taken at load time is
    /// the only way to reach a plugin the audio thread owns.
    pub fn open_insert_editor(&mut self, strip: usize, slot: usize) -> Result<(), String> {
        // **The whole seat map, pads included** — the same stale bound
        // `load_insert` had already been cured of. This still capped at
        // `STRIPS`, so a pad's effect loaded, ran, and answered every click
        // on its bay with a silent `Ok`: a plugin whose window could never
        // open, and whose controls could therefore never be touched.
        if strip >= RACK_SEATS || slot >= INSERTS {
            return Ok(());
        }
        self.poll_editor();
        if let Some(editor) = self
            .insert_editors
            .get(strip)
            .and_then(|r| r.get(slot))
            .and_then(Option::as_ref)
        {
            editor.focus();
            return Ok(());
        }
        let Some(handle) = self
            .insert_editor_handles
            .get(strip)
            .and_then(|r| r.get(slot))
            .and_then(Option::as_ref)
        else {
            return Err(format!("nothing is loaded in insert {}", slot + 1));
        };
        // The CHANNEL and the bay, both, because the same compressor on four
        // channels gives four windows that are otherwise identical.
        let title = match self.insert(strip, slot) {
            Some(l) => format!("{} - Tangent, insert {}", l.class, slot + 1),
            None => format!("Insert {} - Tangent", slot + 1),
        };
        let editor = ivory_host::Editor::open_handle(handle, &title).map_err(|e| e.to_string())?;
        if let Some(e) = self
            .insert_editors
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *e = Some(editor);
        }
        Ok(())
    }

    /// Close one insert's window if it is open. Safe when it is not.
    pub fn close_insert_editor(&mut self, strip: usize, slot: usize) {
        if let Some(e) = self
            .insert_editors
            .get_mut(strip)
            .and_then(|r| r.get_mut(slot))
        {
            *e = None;
        }
    }

    /// Whether the plugin in this bay offers an editor to open.
    ///
    /// `false` for an empty bay, for one that does not exist, and for a
    /// plugin that has no UI — which is the honest thing to grey a control
    /// on. The first call after a load is not free (VST3 has no `hasEditor`,
    /// so the only way to ask is to build a view and throw it away); every
    /// call after that is a cached bool.
    pub fn insert_has_editor(&self, strip: usize, slot: usize) -> bool {
        self.insert_editor_handles
            .get(strip)
            .and_then(|r| r.get(slot))
            .and_then(Option::as_ref)
            .is_some_and(ivory_host::EditorHandle::has_editor)
    }

    /// Is this insert's window open right now?
    pub fn insert_editor_open(&self, strip: usize, slot: usize) -> bool {
        self.insert_editors
            .get(strip)
            .and_then(|r| r.get(slot))
            .is_some_and(Option::is_some)
    }

    /// Ask any plugin that says its latency changed for the new figure.
    ///
    /// **Called while an editor is OPEN, which is when a preset changes.**
    /// The obvious alternative — a timer — pays for itself sixty times a
    /// minute for ever to catch something that only happens while somebody
    /// is looking at a plugin's window. And the obvious cheap version —
    /// re-reading on editor CLOSE — misses the ordinary case, which is
    /// clicking through presets with the window still open and then playing.
    ///
    /// Nothing can change while no window is open, because this host does
    /// not forward program changes: `midi_event::parse_message` takes note
    /// on, note off and CC 64 and nothing else, and a test asserts it. So a
    /// plugin here cannot be told to change preset behind your back.
    ///
    /// A voice is never seen here: it is given no handle at load, so the
    /// loop below skips it and its bay stays at zero.
    ///
    /// Answers whether anything moved, so the caller can push a fresh
    /// figure rather than doing it every frame.
    pub fn poll_latency(&mut self) -> bool {
        let mut moved = false;
        for strip in 0..RACK_SEATS {
            for slot in 0..INSERTS {
                let Some(h) = self
                    .insert_latency_handles
                    .get(strip)
                    .and_then(|r| r.get(slot))
                    .and_then(Option::as_ref)
                else {
                    continue;
                };
                if !h.take_dirty() {
                    continue;
                }
                let now = h.latency_samples();
                if Rack::set_latency(&self.shared, strip, slot, now) {
                    moved = true;
                }
            }
        }
        moved
    }

    /// Notice windows the user closed and let go of them. Call once a frame.
    ///
    /// **All three at once**, because the caller has one frame loop and should
    /// not have to know how many slots there are to keep it honest. Cheap: one
    /// bool read per open window, nothing at all for a slot with none. Without
    /// it a closed editor stays "open" as far as [`Engine::editor_open`] is
    /// concerned, and the menu row keeps saying Close for a window that is not
    /// there.
    pub fn poll_editor(&mut self) {
        // An insert's window closed by its own close button would otherwise
        // stay "open" here for ever, and
        // the click that should reopen it would silently raise nothing.
        for rack in &mut self.insert_editors {
            for editor in rack {
                if editor.as_ref().is_some_and(ivory_host::Editor::closed) {
                    *editor = None;
                }
            }
        }
    }

    /// Is anything loaded at all?
    ///
    /// The take-source decision: a take records the instruments when there is an
    /// instrument to record, and one full slot out of three is enough. Asking
    /// per slot would make "record the plugin" mean "record slot 0".
    /// Put the built-in instrument in a slot, or take it out.
    ///
    /// A slot holding it is not a plugin: nothing is opened, nothing can fail,
    /// and it is playing on the next block.
    /// Which general CHANNEL the built-in DX7 belongs to. See
    /// `Shared::builtin_strip`: the host computes it from bay 1's sentinel and
    /// the channel's kind; the callback only reads it.
    pub fn set_builtin_strip(&self, strip: Option<usize>) {
        self.shared
            .builtin_strip
            .store(strip.map_or(-1, |s| s as i64), Ordering::Relaxed);
    }

    /// Which general CHANNEL the drumpad belongs to. Its lane's notes —
    /// live and clip both — strike the sixteen pads instead of sounding a
    /// pitch, and the kit renders on the DRUMS bus as it always has.
    pub fn set_drum_strip(&self, strip: Option<usize>) {
        self.shared
            .drum_strip
            .store(strip.map_or(-1, |s| s as i64), Ordering::Relaxed);
    }

    /// One general channel's fader.
    pub fn set_channel_gain(&self, ch: usize, linear: f32) {
        self.shared.set_channel_gain(ch, linear);
    }

    /// Which channel each capture pick feeds: desk indices, `None` unbound.
    /// The live punch: `Some(frame)` while lane records over its clips.
    pub fn set_punches(&self, punches: [Option<u64>; CHANNELS]) {
        for (cell, p) in self.shared.punch.iter().zip(punches) {
            cell.store(p.unwrap_or(u64::MAX), Ordering::Relaxed);
        }
    }

    pub fn set_pick_strips(&self, strips: [Option<usize>; INPUTS]) {
        for (cell, strip) in self.shared.pick_strip.iter().zip(strips) {
            cell.store(strip.map_or(-1, |s| s as i64), Ordering::Relaxed);
        }
    }

    /// Load a patch into the built-in.
    ///
    /// Sent as a message rather than written through, because the renderer owns
    /// it and it is on the audio thread. A voice is 155 small numbers and this
    /// happens when somebody clicks a name in a list, so the cost is nothing.
    pub fn set_builtin_voice(&mut self, voice: crate::dx7::Voice) {
        if let Ok(mut g) = self.shared.pending_voice.lock() {
            *g = Some(voice);
        }
    }

    /// Whether a backing track is loaded, playing or not.
    ///
    /// Loaded is the question, not playing: a take is armed before the
    /// transport rolls, and a track that starts with it would otherwise decide
    /// the take's sources one buffer too late.
    pub fn track_loaded(&self) -> bool {
        self.shared.track_loaded.load(Ordering::Relaxed)
    }

    // ── the instruments' state ──────────────────────────────────────────────

    /// Which channels' rack voices ignore the note stream, as one mask.
    ///
    /// Pushed whole, like mute and solo: the UI computes it from every
    /// channel's kind and arm switch, and the audio thread reads it per chunk.
    /// Zero is every channel armed, which is the desk as it has always been.
    pub fn set_midi_off(&self, mask: u32) {
        self.shared.midi_off.store(mask, Ordering::Relaxed);
    }

    /// Which channels are armed — live to the room. See `Shared::armed`.
    pub fn set_armed(&self, mask: u32) {
        self.shared.armed.store(mask, Ordering::Relaxed);
    }

    /// One insert's current state, for persisting. `None` for an empty bay —
    /// or for a plugin whose `getState` failed, which is a preset to choose
    /// again rather than an error to stop on.
    pub fn save_insert_state(&self, strip: usize, slot: usize) -> Option<Vec<u8>> {
        self.insert_state_handles
            .get(strip)?
            .get(slot)?
            .as_ref()
            .and_then(|h| h.save().ok())
    }

    pub fn output(&self) -> &OutputInfo {
        &self.output
    }

    // ── MIDI in ─────────────────────────────────────────────────────────────

    /// Queue a raw MIDI message for the audio thread.
    ///
    /// Never blocks, never allocates, never fails loudly: a full ring counts the
    /// loss in [`Engine::midi_dropped`] rather than stalling the caller. Takes
    /// `&self` so it can one day be called from `midi.rs`'s own `midir` callback
    /// instead of once per UI frame, which is where the latency actually is.
    ///
    /// A short or empty buffer is padded with zeroes and then ignored by
    /// [`note_from_midi`], which is the only place that decides what a message
    /// means.
    pub fn send_midi(&self, stamp_host_ns: Nanos, bytes: &[u8]) {
        let Some(&status) = bytes.first() else {
            return;
        };
        let event = MidiEvent {
            stamp: stamp_host_ns,
            status,
            data1: bytes.get(1).copied().unwrap_or(0),
            data2: bytes.get(2).copied().unwrap_or(0),
        };
        let Ok(mut tx) = self.midi.try_borrow_mut() else {
            self.shared.midi_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if tx.push(event).is_err() {
            self.shared.midi_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Messages the MIDI ring could not hold.
    pub fn midi_dropped(&self) -> u64 {
        self.shared.midi_dropped.load(Ordering::Relaxed)
    }

    /// Pedal messages received and **not delivered to the instrument**, because
    /// `ivory-host` has no path for a control change. See the module docs; this
    /// is here so the band can say so rather than the user finding out.
    pub fn pedal_dropped(&self) -> u64 {
        self.shared.pedal_dropped.load(Ordering::Relaxed)
    }

    // ── the recorder's tap ──────────────────────────────────────────────────

    /// Take the recorder's read end. `None` if it has already been taken.
    pub fn take_recorder_tap(&mut self) -> Option<RecorderTap> {
        self.tap.take()
    }

    /// Take the ReaStream send ring's read end, once per engine, to hand to the
    /// sender thread. `None` if it has already been taken. Taking it does NOT
    /// start the flow — [`Engine::set_reastream_on`] gates the callback's push,
    /// so a consumer that is dropped without the flag on simply drains nothing.
    pub fn take_reastream_tap(&mut self) -> Option<Consumer<f32>> {
        self.reastream_rx.take()
    }

    /// Switch the master-out ReaStream push on or off. Read once per block by
    /// the callback; off means the send ring is neither packed nor pushed.
    pub fn set_reastream_on(&self, on: bool) {
        self.shared.reastream_on.store(on, Ordering::Relaxed);
    }

    /// Publish the desk's routing table to the audio thread.
    ///
    /// **The count is stored LAST and cleared FIRST**, so the render loop can
    /// never read a half-written table: growing, the new words land before
    /// the count that admits them; shrinking, the count drops before the
    /// words go. A torn read would route a channel to a jack nobody chose.
    pub fn set_routes(&self, routes: &ivory_ui::routing::Routes) {
        use ivory_ui::routing::Target;
        let n = routes.0.len().min(MAX_ROUTES);
        self.shared.route_count.store(0, Ordering::Release);
        for (i, r) in routes.0.iter().take(n).enumerate() {
            let (target, hardware, stereo) = match r.target {
                Target::Hardware { first, stereo } => (first, true, stereo),
                Target::Channel(c) => (c, false, false),
            };
            self.shared.routes[i].store(
                pack_route(r.from, target, hardware, stereo),
                Ordering::Relaxed,
            );
            self.shared.route_gain[i].store(r.gain.to_bits(), Ordering::Relaxed);
        }
        self.shared.route_count.store(n, Ordering::Release);
    }

    /// Take the hardware fast lane's write end, for `midi::MidiFastLane`.
    ///
    /// Once-only, like the recorder tap: the desktop seats it in the
    /// app-lifetime lane when the engine starts, and from then on the midir
    /// callback thread feeds the audio callback directly — no UI frame in
    /// the loop. `None` if it has already been taken.
    pub fn take_hw_midi(&mut self) -> Option<Producer<MidiEvent>> {
        self.hw_midi_tx.take()
    }

    // ── metering ────────────────────────────────────────────────────────────

    /// Levels for the band's meter, measured on the **program mix** — the mix
    /// the limiter guarantees. The click and the audition ride above it on
    /// the cue road, unmetered, so a metronome can never light the clip lamp
    /// over a correctly limited master.
    ///
    /// The peak is read-and-cleared, so a transient between two UI frames is
    /// never missed; the hold decays here rather than in the callback, at the
    /// same 20 dB/s `ivory-record`'s meter uses so the two read alike.
    pub fn meters(&self) -> ivory_ui::recorder::Meters {
        let s = &self.shared;
        let peak_l = f32::from_bits(s.peak_l.swap(0, Ordering::Relaxed));
        let peak_r = f32::from_bits(s.peak_r.swap(0, Ordering::Relaxed));
        let rms_l = Shared::f32_of(&s.rms_l);
        let rms_r = Shared::f32_of(&s.rms_r);

        let now = Instant::now();
        let dt = now.duration_since(self.hold_at.get()).as_secs_f32();
        self.hold_at.set(now);
        let decay = 10f32.powf(-HOLD_DECAY_DB_PER_SEC * dt / 20.0);
        let (hl, hr) = self.hold.get();
        let hold = ((hl * decay).max(peak_l), (hr * decay).max(peak_r));
        self.hold.set(hold);

        let mono = self.output.channels <= 1;
        ivory_ui::recorder::Meters {
            left: ivory_ui::recorder::Level {
                peak: peak_l,
                rms: rms_l,
                hold: hold.0,
                // Ballistics are the app's — see `Level::vu`.
                vu: 0.0,
            },
            right: ivory_ui::recorder::Level {
                peak: if mono { peak_l } else { peak_r },
                rms: if mono { rms_l } else { rms_r },
                hold: if mono { hold.0 } else { hold.1 },
                vu: 0.0,
            },
            mono,
            clipped: s.clipped.load(Ordering::Relaxed),
        }
    }

    /// Clear the clip latch. The *only* thing that does: a latch that clears
    /// itself is one the performer never sees, because they were looking at
    /// their hands when it happened.
    pub fn clear_clip(&self) {
        self.shared.clipped.store(false, Ordering::Relaxed);
    }

    /// Which pitches each lane's CLIPS are sounding right now.
    ///
    /// Playback only — see `Shared::lane_sounding`.
    pub fn lane_sounding(&self) -> [u128; CHANNELS] {
        std::array::from_fn(|lane| {
            let lo = self
                .shared
                .lane_sounding
                .get(lane * 2)
                .map_or(0, |c| c.load(Ordering::Relaxed));
            let hi = self
                .shared
                .lane_sounding
                .get(lane * 2 + 1)
                .map_or(0, |c| c.load(Ordering::Relaxed));
            u128::from(lo) | (u128::from(hi) << 64)
        })
    }

    /// The loudness ladder's three readings, in LUFS.
    /// `loudness::ABSOLUTE_GATE` means "nothing measured yet".
    pub fn loudness(&self) -> (f32, f32, f32) {
        let r = |i: usize| f32::from_bits(self.shared.lufs[i].load(Ordering::Relaxed));
        (r(0), r(1), r(2))
    }

    /// The stereo pair behind the ladder: `[mom L, mom R, short L, short R]`.
    /// See `loudness::Loudness::read_stereo` for why there is no integrated
    /// pair.
    pub fn loudness_stereo(&self) -> [f32; 4] {
        std::array::from_fn(|i| f32::from_bits(self.shared.lufs[i + 3].load(Ordering::Relaxed)))
    }

    /// Start the integrated reading again. See `Loudness::reset_integrated`.
    pub fn reset_loudness(&self) {
        self.shared.lufs_reset.fetch_add(1, Ordering::Relaxed);
    }

    /// How fast the whole desk plays. 1.0 is off. See `crate::varispeed`.
    ///
    /// Sanitised on the way in as well as on the way out: this multiplies
    /// into a read position on the audio thread, and a NaN there is not a
    /// display glitch.
    pub fn set_varispeed(&self, ratio: f32) {
        let r = if ratio.is_finite() {
            f64::from(ratio).clamp(0.5, crate::varispeed::MAX_RATIO)
        } else {
            1.0
        };
        self.shared
            .varispeed
            .store((r as f32).to_bits(), Ordering::Relaxed);
    }



    // ── health ──────────────────────────────────────────────────────────────

    /// True while the device is open and the callback has not reported a fatal
    /// error.
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    /// Callbacks served. A count that stops climbing while [`Engine::is_running`]
    /// is true is a device that has gone quiet without saying so.
    pub fn callbacks(&self) -> u64 {
        self.shared.callbacks.load(Ordering::Relaxed)
    }

    /// One health reading for [`StallWatch`], taken under one lock discipline.
    pub fn health(&self) -> (bool, u64) {
        (self.is_running(), self.callbacks())
    }

    /// The device the output stream is playing through, for ASIO duplex: the
    /// input MUST open on this same object or the driver's single buffer set
    /// is rebuilt out from under the running output. See `out_device_obj`.
    pub fn output_device(&self) -> Option<&cpal::Device> {
        self.out_device_obj.as_ref()
    }

    /// **Measured** output latency: how far ahead of its callback the backend
    /// says a buffer will be heard.
    ///
    /// The only latency number in the product that is not an assumption. cpal
    /// 0.16 reads neither `kAudioDevicePropertyLatency` nor the safety offset
    /// (`ivory-record`'s `audio.rs` documents the dig), so the device's own
    /// converter delay is still outside it — but the difference between the
    /// callback instant and the playback instant is real, comes from the
    /// backend, and is what separates a 5 ms path from a 40 ms one. Zero means
    /// the backend declined to say.
    pub fn output_delay_ns(&self) -> Nanos {
        self.shared.delay_ns.load(Ordering::Relaxed)
    }

    /// Whatever cpal last reported about the device.
    pub fn fault(&self) -> Option<String> {
        self.fault.lock().ok().and_then(|g| g.clone())
    }

    // ── gains ───────────────────────────────────────────────────────────────

    /// The three effect knobs, 0..=1.
    pub fn set_effects(&self, sends: crate::effects::Sends) {
        self.shared.set_effects(sends);
    }

    /// What each effect is set to.
    pub fn set_effect_params(&self, params: crate::effects::Params) {
        self.shared.set_effect_params(params);
    }

    /// Hand the renderer a clip and its place on the timeline, or `None` to
    /// take the lane's clip away.
    ///
    /// **The displaced clip dies HERE, on this thread.** The push waits for the
    /// return by the racks' own bargain: whatever the callback was holding
    /// comes back on `track_from_audio` and is dropped by the caller that sent
    /// its replacement — never by the audio thread, whose `drop` of a
    /// hundred-megabyte `Vec` was a measured, reachable stall.
    pub fn set_clips(&mut self, clips: Vec<Placed>) {
        // Whatever came back earlier is dropped first, so the ring always has
        // room and two rapid loads cannot wedge.
        while self.track_from_audio.pop().is_ok() {}
        self.shared
            .track_loaded
            .store(!clips.is_empty(), Ordering::Relaxed);
        if self.track_to_audio.push(clips).is_err() {
            return;
        }
        let deadline = Instant::now() + RETIRE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(old) = self.track_from_audio.pop() {
                drop(old);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Hand the audio thread the arrangement's MIDI schedule, whole — the
    /// same swap protocol as `set_clips`, so the old list drops HERE, never
    /// in the callback.
    pub fn set_midi_clips(&mut self, clips: Vec<PlacedMidi>) {
        while self.midi_from_audio.pop().is_ok() {}
        if self.midi_to_audio.push(clips).is_err() {
            return;
        }
        let deadline = Instant::now() + RETIRE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(old) = self.midi_from_audio.pop() {
                drop(old);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Hand the audio thread the drumpad's samples, whole — the clips' own
    /// swap protocol, so the displaced kit drops HERE, never in the callback.
    /// `true` when the kit was HANDED to the audio thread — the swap
    /// itself happens on its next block. `false` means the ring was full
    /// (a stalled callback) and the kit was NOT delivered: the caller must
    /// retry, or the pads light up over an engine that never got them —
    /// the Windows tester's pads striking the DX7.
    pub fn set_pad_kit(&mut self, kit: PadKit) -> bool {
        while self.pad_from_audio.pop().is_ok() {}
        if self.pad_to_audio.push(kit).is_err() {
            return false;
        }
        let deadline = Instant::now() + RETIRE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(old) = self.pad_from_audio.pop() {
                drop(old);
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // The push landed; the retiree is merely late. Delivered.
        true
    }

    /// AUDITION a decoded file: it replaces whatever preview is playing
    /// and starts from the top. `None` stops. The kit swap's own protocol,
    /// so nothing deallocates on the audio thread.
    pub fn set_preview(&mut self, clip: Option<Arc<ivory_record::decode::Clip>>) {
        while self.preview_from_audio.pop().is_ok() {}
        if self.preview_to_audio.push(clip).is_err() {
            return;
        }
        let deadline = Instant::now() + RETIRE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(old) = self.preview_from_audio.pop() {
                drop(old);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The drumpad's knobs, pushed whole every frame like the desk.
    /// Sanitised here, on the way in: a NaN survives `clamp` and a NaN in a
    /// gain does not stay in one sample.
    pub fn set_pad_params(&self, kit: &ivory_ui::recorder::DrumKitParams) {
        let sane = |v: f32, lo: f32, hi: f32, or: f32| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                or
            }
        };
        let sh = &self.shared;
        let mut obey = [0u64; 2];
        for (i, p) in kit.pads.iter().enumerate().take(PADS) {
            sh.pad_gain[i].store(sane(p.gain, 0.0, 2.0, 1.0).to_bits(), Ordering::Relaxed);
            sh.pad_pan[i].store(sane(p.pan, -1.0, 1.0, 0.0).to_bits(), Ordering::Relaxed);
            sh.pad_tune[i].store(sane(p.tune, -12.0, 12.0, 0.0).to_bits(), Ordering::Relaxed);
            sh.pad_choke[i].store(u32::from(p.choke), Ordering::Relaxed);
            if p.obey {
                obey[i >> 6] |= 1u64 << (i & 63);
            }
            sh.pad_trim_start[i].store(
                sane(p.trim_start, 0.0, 1.0, 0.0).to_bits(),
                Ordering::Relaxed,
            );
            sh.pad_trim_end[i].store(sane(p.trim_end, 0.0, 1.0, 1.0).to_bits(), Ordering::Relaxed);
        }
        sh.pad_obey[0].store(obey[0], Ordering::Relaxed);
        sh.pad_obey[1].store(obey[1], Ordering::Relaxed);
        sh.pad_master
            .store(sane_gain(kit.master).to_bits(), Ordering::Relaxed);
    }

    /// The loudest sample each pad has made since this was last asked —
    /// read-and-reset, the strips' own idiom. What lights a pad from the
    /// audio rather than from the gesture, so a hardware strike shows too.
    pub fn pad_peaks(&self) -> [f32; PADS] {
        std::array::from_fn(|i| f32::from_bits(self.shared.pad_peak[i].swap(0, Ordering::Relaxed)))
    }

    /// Which input picks tee into the capture rings — a bit per input, zero
    /// off. Flipped on when arrangement record starts, off when it stops.
    pub fn set_arr_capture(&self, mask: u32) {
        self.shared.arr_capture.store(mask, Ordering::Relaxed);
    }

    /// Drain one input's captured frames (interleaved stereo) into `out`.
    pub fn drain_capture(&mut self, input: usize, out: &mut Vec<f32>) {
        let Some(rx) = self.cap_rx.get_mut(input) else {
            return;
        };
        while let Ok(v) = rx.pop() {
            out.push(v);
        }
    }

    /// One channel's pan, -1..1, centre unity. Pushed each frame with the
    /// gains, by the same rule.
    pub fn set_channel_pan(&self, ch: usize, pan: f32) {
        self.shared.set_channel_pan(ch, pan);
    }

    /// The drum bus's fader and pan, pushed each frame with the gains.
    pub fn set_drums(&self, gain: f32, pan: f32) {
        self.shared
            .drums_gain
            .store(sane_gain(gain).to_bits(), Ordering::Relaxed);
        let p = if pan.is_finite() {
            pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.shared.drums_pan.store(p.to_bits(), Ordering::Relaxed);
    }

    /// The stem tap's switch: on for an export pass, off after.
    pub fn set_stems(&self, on: bool) {
        self.shared.stems_on.store(on, Ordering::Relaxed);
    }

    /// Drain the stem tap — interleaved `CHANNELS * 2` floats per frame —
    /// into `out`.
    pub fn drain_stems(&mut self, out: &mut Vec<f32>) {
        while let Ok(v) = self.stem_rx.pop() {
            out.push(v);
        }
    }

    /// Everything the scope tap delivered since the last frame —
    /// interleaved stereo, the speakers' mix without the click.
    pub fn drain_scope(&mut self, out: &mut Vec<f32>) {
        while let Ok(v) = self.scope_rx.pop() {
            out.push(v);
        }
    }

    /// Put the playhead at `frames`, stamped with the host's generation.
    ///
    /// Payload first, generation last with Release — `run_transport`'s Acquire
    /// pairs with it, so a callback that sees the new generation sees the new
    /// position. The generation is the HOST's monotonic counter, stored rather
    /// than incremented here, so a freshly built engine (req 0, ack 0) never
    /// matches a host that has published anything and is re-located on its
    /// first pushed frame.
    pub fn set_transport(&self, generation: u64, frames: u64) {
        self.shared.transport_at.store(frames, Ordering::Relaxed);
        self.shared
            .transport_req
            .store(generation, Ordering::Release);
    }

    /// The transport level: rolling or stopped. Safe to re-assert every frame;
    /// nothing keys off its edges.
    pub fn set_rolling(&self, rolling: bool) {
        self.shared.rolling.store(rolling, Ordering::Relaxed);
    }

    /// Set the loop region in frames, or clear it with `a >= b`. Re-asserted
    /// every frame from the host; the callback reads it in `run_transport`.
    pub fn set_loop(&self, a: u64, b: u64) {
        let (a, b) = if b > a { (a, b) } else { (0, 0) };
        self.shared.loop_a.store(a, Ordering::Relaxed);
        self.shared.loop_b.store(b, Ordering::Relaxed);
    }

    /// Where the callback's clock actually is, in frames. Monotone while
    /// rolling, constant while stopped, and exact for a given locate once
    /// [`Engine::transport_acked`] answers true for its generation.
    pub fn transport_position(&self) -> u64 {
        self.shared.transport_pos.load(Ordering::Relaxed)
    }

    /// Whether the callback has applied the locate stamped `generation`.
    pub fn transport_acked(&self, generation: u64) -> bool {
        self.shared.transport_ack.load(Ordering::Acquire) == generation
    }

    pub fn set_track_gain(&self, linear: f32) {
        self.shared.set_track_gain(linear);
    }

    /// Hear the live input, or stop hearing it.
    ///
    /// **Never restored from a settings file.** Off at every launch, because
    /// the failure mode is somebody turning their speakers on after a relaunch
    /// they had forgotten was monitoring, and getting a room full of feedback.
    pub fn set_monitor_on(&self, on: bool) {
        self.shared.monitor_on.store(on, Ordering::Relaxed);
    }

    /// What the monitoring path is holding, in milliseconds.
    ///
    /// **Latency the panel could not see.** The estimate beside it is one
    /// buffer in plus one buffer out; this is the ring between them, which on
    /// a session where the output started late used to hold a tenth of a
    /// second and never give it back. It is bounded now — see `mix_monitor` —
    /// and this is what makes the bound visible rather than trusted.
    pub fn monitor_backlog_ms(&self) -> f64 {
        let rate = f64::from(self.output.sample_rate.max(1));
        f64::from(self.shared.monitor_backlog.load(Ordering::Relaxed)) * 1000.0 / rate
    }

    /// Tell the renderer which columns of the monitor ring belong to which
    /// strip, without handing it a new ring.
    ///
    /// **This is the monitor's half of a live pick change.** The ring carries
    /// whole device frames for the life of the stream, so choosing different
    /// inputs does not change what is IN it — only which columns each strip
    /// reads. Nothing has to be flushed and nothing can be misread: every
    /// frame in the ring means the same thing before and after.
    pub fn set_monitor_layout(&self, widths: [u8; INPUTS], picks: ivory_record::audio::Picks) {
        if let Ok(mut g) = self.shared.pending_monitor_layout.lock() {
            *g = Some((widths, picks));
        }
    }

    /// Hand the renderer the live input's ring. `None` takes it away.
    pub fn set_monitor(
        &mut self,
        tap: Option<(
            rtrb::Consumer<f32>,
            u16,
            [u8; INPUTS],
            u32,
            ivory_record::audio::Picks,
        )>,
    ) {
        if let Ok(mut g) = self.shared.pending_monitor.lock() {
            *g = Some(tap);
        }
    }

    /// The desk: what every strip sends to the effects bus, and what is heard.
    ///
    /// **Pushed whole, every frame, like the gains.** The masks are built here
    /// rather than toggled bit by bit so that "is anything soloed" can never be
    /// answered from a half-written state, and the `From` below is exhaustive —
    /// which is what makes the UI's strip order and this one provably the same
    /// rather than the same by inspection.
    pub fn set_desk(&self, desk: &ivory_ui::recorder::Desk) {
        let mut muted = 0u32;
        let mut soloed = 0u32;
        let sh = &self.shared;
        for ui in ivory_ui::recorder::Strip::all() {
            let here = Strip::from(ui);
            sh.send[here.index()].store(
                desk.send[ui.index()].clamp(0.0, 1.0).to_bits(),
                Ordering::Relaxed,
            );
            if desk.muted[ui.index()] {
                muted |= here.bit();
            }
            if desk.soloed[ui.index()] {
                soloed |= here.bit();
            }
        }
        sh.muted.store(muted, Ordering::Relaxed);
        sh.soloed.store(soloed, Ordering::Relaxed);
    }

    /// The loudest thing each strip has made since this was last called, and
    /// it CLEARS as it reads. Zero for a strip that has been silent.
    pub fn strip_peaks(&self) -> [[f32; 2]; STRIPS] {
        std::array::from_fn(|i| {
            std::array::from_fn(|c| {
                f32::from_bits(self.shared.strip_peak[i][c].swap(0, Ordering::Relaxed))
            })
        })
    }

    /// What comes back from the effects bus, at its own fader.
    pub fn set_fx_return(&self, linear: f32) {
        self.shared
            .fx_return
            .store(linear.clamp(0.0, 8.0).to_bits(), Ordering::Relaxed);
    }

    /// Fold the master to mono, or stop. See [`Shared::master_mono`].
    pub fn set_master_mono(&self, on: bool) {
        self.shared.master_mono.store(on, Ordering::Relaxed);
    }

    /// The master, as a linear gain. See [`Shared::master_gain`].
    pub fn set_master_gain(&self, linear: f32) {
        self.shared
            .master_gain
            .store(sane_gain(linear).to_bits(), Ordering::Relaxed);
    }

    /// Decibels the limiter has taken off since this was last asked.
    ///
    /// **Read and reset**, like the meter peaks and for the same reason: gain
    /// reduction is a transient a few samples long, and a UI that asks sixty
    /// times a second would otherwise miss the moment the meter exists for.
    pub fn gain_reduction_db(&self) -> f32 {
        f32::from_bits(self.shared.gr_db.swap(0, Ordering::Relaxed))
    }

    pub fn set_metronome_gain(&self, linear: f32) {
        self.shared
            .metro_gain
            .store(sane_gain(linear).to_bits(), Ordering::Relaxed);
    }

    // ── the metronome ───────────────────────────────────────────────────────

    /// Turn the free-running click on or off. Switching it on restarts the
    /// phase, so it always begins on an accented beat 1.
    pub fn set_metronome_enabled(&self, on: bool) {
        self.shared.metro_on.store(on, Ordering::Relaxed);
    }

    /// Whether the click is mixed into what the recorder captures.
    ///
    /// **False by default**, and that is the standard: a click bleeding into
    /// the take ruins it, and ruins it in a way nobody notices until playback.
    pub fn set_metronome_in_take(&self, on: bool) {
        self.shared.metro_in_take.store(on, Ordering::Relaxed);
    }

    /// Whether the COUNT-IN's clicks go into the take. See
    /// [`Shared::count_in_in_take`] for why this is not the same switch.
    pub fn set_count_in_in_take(&self, on: bool) {
        self.shared.count_in_in_take.store(on, Ordering::Relaxed);
    }

    /// Hand the audio thread the tempo and meter maps, whole.
    ///
    /// The same swap protocol as [`Engine::set_clips`], and for the same
    /// reason: a map is a growable `Vec` and the callback may neither read a
    /// growing one nor free the old one.
    pub fn set_maps(&mut self, maps: Maps) {
        while self.maps_from_audio.pop().is_ok() {}
        if self.maps_to_audio.push(maps).is_err() {
            return;
        }
        let deadline = Instant::now() + RETIRE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(old) = self.maps_from_audio.pop() {
                drop(old);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Hand the audio thread every automation curve, whole.
    ///
    /// The same swap protocol as [`Engine::set_maps`], for the same reason.
    /// Which lanes' curves must not drive their controls, one bit each.
    ///
    /// An atomic rather than a ring: it is one word, it changes as often as
    /// a hand touches a fader, and it must take effect on the very next
    /// block or the control fights the hand.
    /// What each channel's inserts are costing, in frames, and the largest
    /// figure on the desk.
    ///
    /// Plain numbers across the firewall, like everything else a panel
    /// learns about the engine.
    /// What each channel's inserts cost, and the WHOLE delay through the
    /// desk — master included, because that is what a person is waiting on.
    ///
    /// Not `max_latency`, which is the narrower number the aligners use. See
    /// both fields on `Shared`.
    pub fn latency_frames(&self) -> ([u32; CHANNELS], u32) {
        (
            std::array::from_fn(|i| self.shared.lane_latency[i].load(Ordering::Relaxed)),
            self.shared.total_latency.load(Ordering::Relaxed),
        )
    }


    pub fn set_auto_suspend(&self, mask: u32) {
        self.shared.auto_suspend.store(mask, Ordering::Relaxed);
    }

    pub fn set_automation(&mut self, auto: ivory_ui::automation::Automation) {
        while self.auto_from_audio.pop().is_ok() {}
        if self.auto_to_audio.push(auto).is_err() {
            return;
        }
        let deadline = Instant::now() + RETIRE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(old) = self.auto_from_audio.pop() {
                drop(old);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn set_tempo(&self, bpm: f64) {
        self.shared.bpm.store(bpm.to_bits(), Ordering::Relaxed);
    }

    /// Beats per bar, which decides which beat is accented.
    pub fn set_beats_per_bar(&self, beats: u32) {
        self.shared
            .beats_per_bar
            .store(beats.max(1), Ordering::Relaxed);
    }

    /// The time signature, both halves at once.
    ///
    /// One call rather than two setters, because the two are read on the audio
    /// thread on different frames: setting 6 then 8 leaves one callback seeing
    /// 6/4, which is a bar and a half of clicks at the wrong speed. They are
    /// still two atomics — a callback can still catch them mid-change — but the
    /// window is one store apart rather than a whole UI frame.
    pub fn set_meter(&self, beats: u32, unit: u32) {
        self.shared.beat_unit.store(
            match unit {
                1 | 2 | 4 | 8 | 16 | 32 => unit,
                _ => 4,
            },
            Ordering::Relaxed,
        );
        self.shared
            .beats_per_bar
            .store(beats.max(1), Ordering::Relaxed);
    }

    /// Count `beats` in at `bpm`, then report [`Engine::count_in_done`].
    ///
    /// **Beats, not seconds** — the pre-roll is N clicks at the take's tempo and
    /// recording starts on the downbeat after them, which is one further beat
    /// period. The clicks and the countdown are the same event: both come from
    /// the audio thread's frame counter, so what the UI shows cannot drift from
    /// what the player hears.
    pub fn start_count_in(&self, beats: u32, bpm: f64) {
        let s = &self.shared;
        s.bpm.store(bpm.to_bits(), Ordering::Relaxed);
        s.count_in_beats.store(beats, Ordering::Relaxed);
        s.count_in_done.store(false, Ordering::Relaxed);
        s.beat_now.store(0, Ordering::Relaxed);
        s.downbeat_ns.store(i64::MIN, Ordering::Relaxed);
        // Published last, **with Release** — the callback's Acquire on the
        // generation pairs with it, so every store above is in place when it
        // acts. This was Relaxed, which promises no such thing: on the
        // owner's Apple-silicon Mac the bump could overtake the beats store,
        // and the callback then latched the OLD beat count for the whole
        // generation — a first count-in read as zero beats declares itself
        // done before a single click. Same idiom as `set_transport`.
        s.count_in_req.fetch_add(1, Ordering::Release);
    }

    /// Abandon a count-in. [`Engine::count_in_done`] stays false.
    pub fn cancel_count_in(&self) {
        let s = &self.shared;
        s.count_in_beats.store(0, Ordering::Relaxed);
        s.count_in_done.store(false, Ordering::Relaxed);
        s.beat_now.store(0, Ordering::Relaxed);
        // Release, as in `start_count_in` and for the same reason: a cancel
        // whose zeroed beat count arrives AFTER its generation is a count-in
        // still clicking.
        s.count_in_req.fetch_add(1, Ordering::Release);
    }

    /// The count-in beat sounding now, 1-based, for the big countdown. `None`
    /// once the count is over or if none is running.
    pub fn metronome_beat(&self) -> Option<u32> {
        match self.shared.beat_now.load(Ordering::Relaxed) {
            0 => None,
            n => Some(n),
        }
    }

    /// True from the downbeat that ends the count-in until the next
    /// [`Engine::start_count_in`].
    pub fn count_in_done(&self) -> bool {
        self.shared.count_in_done.load(Ordering::Relaxed)
    }

    /// When the downbeat was, or will be, **heard** — host timebase, including
    /// the device's own output delay.
    ///
    /// This is the number a take's `T0` should use. Polling
    /// [`Engine::count_in_done`] once a frame is accurate to a UI frame; this is
    /// accurate to a sample, and it is the difference between a take that starts
    /// on the beat and one that starts within 16 ms of it.
    pub fn count_in_downbeat_ns(&self) -> Option<Nanos> {
        match self.shared.downbeat_ns.load(Ordering::Relaxed) {
            i64::MIN => None,
            n => Some(n),
        }
    }

    /// The timebase this engine stamps against, so a caller can place its own
    /// events in the same one.
    pub fn timebase(&self) -> Timebase {
        self.timebase
    }
}

impl Drop for Engine {
    /// Retire **every** instrument before the stream goes, so `terminate` runs
    /// here rather than wherever the backend happens to free its callback box.
    ///
    /// One at a time and each with its own bounded wait: three instances that
    /// all have to come back is three round trips, and the alternative — pushing
    /// all three and then waiting — would have the UI thread holding a deadline
    /// for a callback that may already have stopped.
    fn drop(&mut self) {
        // **Every insert too, and its window BEFORE its instance.**
        //
        // This unloaded the instrument slots and nothing else, and then the
        // fields dropped in declaration order: `stream` is first, so the
        // callback box went, then the `Renderer`, then `racks`, and every
        // insert's `Hosted` was terminated — and only after all that did
        // `insert_editors` drop and call `IPlugView::removed()` and
        // `setFrame(null)` on a view whose controller no longer existed.
        //
        // That is precisely the lifetime rule `editors` is documented with and
        // ordered for, and it was reachable by quitting with an effect's
        // window open. `load_insert(.., None)` is the whole teardown in the
        // right order: close the window, drop the controller reference, then
        // retire the instance and drop it HERE, on this thread.
        //
        // **Every seat, the pad bays included.** This stopped at `STRIPS` —
        // the stale bound `load_insert` was cured of — so a pad's effect was
        // never retired at all and its terminate ran inside the stream
        // teardown: the exact ordering this Drop exists to prevent, back for
        // the hundred and twenty-eight seats it forgot. Only OCCUPIED seats
        // are retired, because the hand-off's bounded wait is per seat: 444
        // empty round trips is seconds of quit on a live callback and nearly
        // two minutes on a stalled one.
        for strip in 0..RACK_SEATS {
            for slot in 0..INSERTS {
                if self.inserts_loaded[strip][slot].is_some() {
                    let _ = self.load_insert(strip, slot, None, None);
                }
            }
        }
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // cpal::Stream is not Debug.
        f.debug_struct("Engine")
            .field("output", &self.output)
            .field("running", &self.is_running())
            .finish()
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Opening the device
// ───────────────────────────────────────────────────────────────────────────

fn sane_gain(linear: f32) -> f32 {
    if linear.is_finite() {
        linear.clamp(0.0, 8.0)
    } else {
        0.0
    }
}

/// One-pole coefficient for [`GAIN_TAU_SECONDS`] at `rate`.
/// The pan law: equal power, normalised so CENTRE is unity — a knob at
/// rest changes nothing, hard sides ride ~3 dB up on the loud side, the
/// way every desk marked "0 dB centre" does it. `pan` is -1..1.
fn pan_gains(pan: f32) -> (f32, f32) {
    let t = (pan.clamp(-1.0, 1.0) + 1.0) * (std::f32::consts::PI / 4.0);
    let root2 = std::f32::consts::SQRT_2;
    (t.cos() * root2, t.sin() * root2)
}

fn gain_coefficient(rate: f64) -> f32 {
    if !(rate.is_finite() && rate > 0.0) {
        return 1.0;
    }
    (1.0 - (-1.0 / (rate * GAIN_TAU_SECONDS)).exp()) as f32
}

/// Does this device actually offer a working output?
///
/// **A registered driver with no hardware behind it still enumerates.** Windows
/// carries the ASIO driver list in the registry, so a Focusrite Thunderbolt
/// ASIO stays listed — and can even be the host's *default* — on a machine that
/// only has the USB box plugged in. It reports no config, or zero channels.
/// Opening the engine on it is the silent-app-under-ASIO bug the owner hit: the
/// default resolves to the driver that sorts first, the stream builds against
/// nothing, and it plays silence with no error to show. A device that cannot
/// name one working config is a device this codebase must skip, not seat.
fn output_is_live(device: &cpal::Device) -> bool {
    device
        .default_output_config()
        .map(|c| c.channels() > 0)
        .unwrap_or(false)
}

/// The index of the output whose name a caller asked for: exact first, then the
/// loose substring a human would type. Pure, so it can be tested without a sound
/// card — cpal has no stable device identifier on any backend, `name()` is the
/// only identity method there is (`ivory-record`'s `audio.rs` documents the dig
/// through the backends), so a name and a substring are all a caller has.
fn match_output_name(names: &[String], want: &str) -> Option<usize> {
    if let Some(i) = names.iter().position(|n| n == want) {
        return Some(i);
    }
    let lower = want.to_lowercase();
    names.iter().position(|n| n.to_lowercase().contains(&lower))
}

/// Find the output device to open.
///
/// With a name: exactly, then loosely. Without one: the platform default when
/// it can actually play, and otherwise the first output that can — because the
/// default is not to be trusted under ASIO, where it may be a hardware-less
/// driver that sorts ahead of the real one. See [`output_is_live`].
fn resolve_output(host: &cpal::Host, want: Option<&str>) -> Result<cpal::Device, String> {
    let Some(want) = want else {
        // The happy path is one call and no probing: the default plays, seat
        // it. Only when it does NOT do we pay to enumerate — so a normal
        // machine loads exactly the driver it always did.
        if let Some(device) = host.default_output_device() {
            if output_is_live(&device) {
                return Ok(device);
            }
            log::warn!(
                "default audio output {:?} reports no working config; \
                 looking for one that does (a registered driver with no \
                 hardware attached, most often)",
                device.name().ok()
            );
        }
        // The default is missing or dead. Take the first output that is real.
        return host
            .output_devices()
            .map_err(|e| format!("could not enumerate audio outputs ({e})"))?
            .find(output_is_live)
            .ok_or_else(|| "no audio output on this machine reports a working config".to_string());
    };
    // **Names first, devices dropped as we go — never all held at once.**
    //
    // Under ASIO a `Device` holds its driver loaded, and the shared host's
    // guard refuses to load any OTHER driver while one is held — so a
    // collected `Vec<Device>` would contain only the first loadable driver
    // and every later row (the one the user asked for, say) would silently
    // vanish from the list. Iterating lazily loads and releases one at a
    // time, which is the only enumeration a single-client driver world
    // permits. Two passes, but device lists are a handful of rows.
    let names: Vec<String> = host
        .output_devices()
        .map_err(|e| format!("could not enumerate audio outputs ({e})"))?
        .map(|d| d.name().unwrap_or_else(|_| "unnamed".to_string()))
        .collect();
    if let Some(i) = match_output_name(&names, want) {
        return host
            .output_devices()
            .map_err(|e| format!("could not enumerate audio outputs ({e})"))?
            .nth(i)
            .ok_or_else(|| format!("audio output {want:?} disappeared between two listings"));
    }
    Err(format!(
        "no audio output matching {want:?}; this machine has {}",
        names.join(", ")
    ))
}

/// How many periods the requested size is asked for, per platform.
///
/// **cpal's ALSA host reads `Fixed(v)` as the whole RING, not the period**, and
/// splits it into four (`host/alsa/mod.rs`: `period = v / 4`). So asking for
/// 256 on Linux does not produce 256-frame callbacks — it produces a 256-frame
/// ring refilled every 64 frames, which is four times the callback rate the
/// number was chosen for, on a plain SCHED_OTHER thread. macOS and WASAPI read
/// the same call as the period.
///
/// Multiplying by four on Linux makes the two agree: the callback cadence is
/// the 256 frames it was always meant to be, and what deepens is the safety
/// ring behind it to the four periods ALSA assumes anyway. Measured on a 2012
/// MacBook Air under four CPU hogs: six underruns in thirty seconds at the
/// stock setting, none at all with the ring four times deeper.
///
/// **And no realtime promotion to go with it.** The obvious companion fix
/// makes it dramatically worse through pipewire-alsa on the same box — 75
/// underruns against 6, starting the moment the thread is promoted, because
/// the plugin's own data loop already runs at RT 83 and a client at FIFO 70
/// inverts priority against its non-RT IPC thread. See
/// `docs/LINUX-4.16-FINDINGS.md`.
const BUFFER_PERIODS: u32 = if cfg!(target_os = "linux") {
    4
} else if cfg!(target_os = "windows") {
    // WASAPI treats `Fixed(n)` as the TOTAL shared-mode buffer capacity,
    // and the engine mixes in ~10 ms periods regardless: a 256-frame
    // request is 5.8 ms — LESS than one period, zero jitter budget, and
    // the tester's "incredible breakup" at 44.1k/256. Two periods is the
    // ASIO-style double buffer: 256 → 11.6 ms, the same maths REAPER
    // wears at the same setting.
    2
} else {
    1
};

/// Ask for [`WANT_BUFFER_FRAMES`], inside whatever the device will accept.
///
/// A `Fixed` size outside the supported range is a build error rather than a
/// clamp, so the range is read first. `SupportedBufferSize::Unknown` means the
/// backend will not say, and there the only safe answer is `Default`.
///
/// **`asio` changes the arithmetic, and getting this wrong CRASHED the app.**
/// `BUFFER_PERIODS` exists because WASAPI and ALSA read `Fixed(v)` as a ring
/// to be carved into periods — but ASIO's created size IS the callback
/// period, so multiplying there asks the driver for a 2x buffer while the
/// INPUT side of the same driver asks for the plain size. An ASIO driver has
/// one buffer set: when the input joined, `ASIOCreateBuffers` re-granted the
/// smaller size, and cpal's output callback — which derives its channel count
/// by dividing its build-time buffer by the CURRENT size — suddenly computed
/// four channels on a stereo interface and indexed off the end of a two-entry
/// array (`asio/stream.rs:747`, "len is 2 but the index is 2", the canary's
/// exact panic). Under ASIO the request is therefore the plain period — the
/// same `Some -> Fixed / None -> Default` translation the input side uses on
/// the same setting, so the two directions request identically, the re-grant
/// equals the original grant, and the division stays at the channel count.
fn pick_buffer(
    supported: &cpal::SupportedBufferSize,
    asked: Option<u32>,
    asio: bool,
) -> (cpal::BufferSize, Option<u32>) {
    // The user's choice first, then the dev override, then the default. The
    // env var stays ahead of the built-in so a debugging session does not have
    // to go through the settings file, and behind the SETTING so that what the
    // Audio Status panel says is what the panel was told.
    let choice = asked.or_else(|| {
        std::env::var("IVORY_OUT_BUFFER")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
    });
    if asio {
        return match (choice, supported) {
            (Some(v), cpal::SupportedBufferSize::Range { min, max }) if *min <= *max => {
                let v = v.clamp(*min, *max);
                (cpal::BufferSize::Fixed(v), Some(v))
            }
            // No stated choice: the driver's own preferred size — the value
            // in its control panel, and the one thing every ASIO driver
            // grants without argument. The input's `Default` lands on the
            // same number, which is the equality that matters.
            _ => (cpal::BufferSize::Default, None),
        };
    }
    let want = choice.unwrap_or(WANT_BUFFER_FRAMES);
    match supported {
        cpal::SupportedBufferSize::Range { min, max } if *min <= *max => {
            // The PERIOD is what `want` means; the ring is however many
            // periods this platform's host expects to be handed. See
            // `BUFFER_PERIODS`.
            let ring = want.saturating_mul(BUFFER_PERIODS).clamp(*min, *max);
            // What is reported back is the period, because that is the number
            // the callback runs at and the one the Audio Status panel is
            // talking about.
            let period = (ring / BUFFER_PERIODS).max(1);
            (cpal::BufferSize::Fixed(ring), Some(period))
        }
        _ => (cpal::BufferSize::Default, None),
    }
}

/// Build the stream for whatever sample type the device speaks.
///
/// The same shape `ivory-record`'s capture path uses, and for the same reason:
/// CoreAudio hands out `f32`, WASAPI shared mode usually does, and ALSA
/// routinely does not. A monitor that only works on two of three platforms is
/// not one.
fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    renderer: Renderer,
    shared: Arc<Shared>,
    fault: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String> {
    macro_rules! open {
        ($t:ty) => {
            build_for::<$t>(device, config, renderer, shared, fault)
        };
    }
    match format {
        cpal::SampleFormat::F32 => open!(f32),
        cpal::SampleFormat::F64 => open!(f64),
        cpal::SampleFormat::I32 => open!(i32),
        // **24-bit is what a Realtek ASIO output actually offers**, and the
        // first Windows tester's launch died on this arm's absence: "the
        // output speaks i24, which is not supported" and a silent app.
        // cpal carries the packed type; the conversion is FromSample like
        // every other arm.
        cpal::SampleFormat::I24 => open!(cpal::I24),
        cpal::SampleFormat::I16 => open!(i16),
        cpal::SampleFormat::I64 => open!(i64),
        cpal::SampleFormat::U16 => open!(u16),
        cpal::SampleFormat::I8 => open!(i8),
        cpal::SampleFormat::U8 => open!(u8),
        other => Err(format!(
            "the output device speaks {other}, which is not supported"
        )),
    }
}

/// The entire cpal-facing body of the monitor path.
///
/// Deliberately a dozen lines: everything that could be wrong is in
/// [`Renderer::render`], which is reachable from a test with no device.
fn build_for<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut renderer: Renderer,
    shared: Arc<Shared>,
    fault: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32> + Send + 'static,
{
    let dev_ch = config.channels as usize;
    let timebase = renderer.timebase;
    let rate = f64::from(config.sample_rate.0);
    // Sized once, on this thread. Everything after this point is the audio
    // thread's and allocates nothing.
    let mut scratch = vec![0.0f32; MAX_CALLBACK_FRAMES * dev_ch];

    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                // **Denormals flushed, every callback.** On x86 a reverb or
                // limiter tail decaying into denormal floats runs orders of
                // magnitude slower — fine on this Mac's ARM, breakup and
                // static on the Windows tester's box at small buffers. Set
                // per callback because WASAPI owns the thread and may move
                // us; the two MXCSR ops cost nothing against one block.
                flush_denormals();
                let now = timebase.now();
                let heard = now + output_delay_ns(info);
                let mut done = 0usize;
                // A callback larger than the mix scratch is rendered in several
                // passes rather than reallocating it. Each pass is later in the
                // timeline by exactly the frames already written, or every event
                // in the second pass would be placed against the first pass's
                // window.
                while done + dev_ch <= data.len() {
                    // **Asked of the renderer rather than assumed**, because
                    // the answer halves while the varispeed is engaged — see
                    // `Renderer::max_callback_frames`.
                    let cap = renderer.max_callback_frames();
                    let frames = ((data.len() - done) / dev_ch).min(cap);
                    let samples = frames * dev_ch;
                    let ahead = ((done / dev_ch) as f64 / rate * 1e9) as Nanos;
                    renderer.render(&mut scratch[..samples], now + ahead, heard + ahead);
                    for (d, s) in data[done..done + samples]
                        .iter_mut()
                        .zip(&scratch[..samples])
                    {
                        *d = T::from_sample(*s);
                    }
                    done += samples;
                }
                // A trailing partial frame is silenced rather than left as
                // whatever the driver had there.
                for d in data[done..].iter_mut() {
                    *d = T::from_sample(0.0f32);
                }
            },
            move |err| {
                // Never panic here: on macOS this fires for a bus-powered
                // interface browning out, which is a routine event. A contended
                // lock is skipped rather than waited on, and the flag is what
                // anything acts on.
                if matches!(err, cpal::StreamError::DeviceNotAvailable) {
                    shared.running.store(false, Ordering::Relaxed);
                }
                if let Ok(mut slot) = fault.try_lock() {
                    *slot = Some(err.to_string());
                }
            },
            None,
        )
        .map_err(|e| format!("could not open the output stream ({e})"))
}

/// How far ahead of the callback the audio it is writing will be heard.
///
/// cpal gives both instants in the device's own timebase and only lets them be
/// compared with each other (`StreamInstant`'s fields are private), so the
/// difference is the only thing readable — and the difference is exactly what is
/// wanted. `None` means playback is not after the callback, which is not
/// physical; zero is the honest answer there.
fn output_delay_ns(info: &cpal::OutputCallbackInfo) -> Nanos {
    let t = info.timestamp();
    t.playback
        .duration_since(&t.callback)
        .map(|d| d.as_nanos() as Nanos)
        .unwrap_or(0)
}

// ───────────────────────────────────────────────────────────────────────────
// The probe
// ───────────────────────────────────────────────────────────────────────────

/// Process CPU time so far, in seconds, or `None` where it cannot be asked.
///
/// [`plugin_test`] and nowhere else. "What does a third instrument cost" has to
/// be answered with a measurement, and the cheap honest place to take one is the
/// whole process across a fixed window: in the probe the audio thread is the
/// only thing doing real work, and everything else is a 16 ms pump.
///
/// Deliberately **not** a timer inside the audio callback. A DSP-load meter is a
/// good feature and it is not this change's to add: two clock reads per block is
/// a change to the hot path that nothing in the product asks for yet.
#[cfg(unix)]
fn cpu_seconds() -> Option<f64> {
    // SAFETY: `rusage` is a plain C struct of integers, so an all-zero bit
    // pattern is a valid value of it, and `getrusage` overwrites the whole thing
    // before anything below reads it. Zeroed rather than `MaybeUninit` because a
    // failed call still has to leave something defined behind.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: the pointer is to a live, correctly-typed local that outlives the
    // call, and `RUSAGE_SELF` is the constant the same crate declares for this
    // parameter. `getrusage` writes only through that pointer.
    let ok = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0;
    if !ok {
        return None;
    }
    let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    Some(secs(usage.ru_utime) + secs(usage.ru_stime))
}

/// Windows has `GetProcessTimes` and no `libc`; the probe says "not measured"
/// rather than growing a second platform path for a developer tool.
#[cfg(not(unix))]
fn cpu_seconds() -> Option<f64> {
    None
}

/// Developer probe: load an instrument into every slot, count in, play a phrase,
/// report.
///
/// **Not wired to the CLI from here.** `main.rs` owns argument parsing and is
/// being edited elsewhere; adding four lines beside `--record-test` turns this
/// on:
///
/// ```text
/// #[cfg(feature = "recorder")]
/// "--plugin-test" => {
///     instrument::plugin_test(args.next());
///     std::process::exit(0);
/// }
/// ```
///
/// It is the only way to exercise the whole monitor chain — device, plugins,
/// warm-up, MIDI queue, event placement, layering, metronome, meters — against
/// real hardware, which no unit test can do. The phrase is played with stamps in
/// the *future*, so sub-block event placement is actually under test rather than
/// collapsing to offset 0 the way live playing does.
///
/// **Layering is measured, not asserted.** The same instrument is loaded into
/// each slot in turn and the same phrase played to all of them, so the claim
/// "every loaded slot gets every note" becomes a number: identical instruments
/// playing identical notes sum coherently, so two slots must be +6 dB on one and
/// three must be +9.5 dB. A peak that does not move is a slot that never heard
/// the note; a peak that moves by less is a sum that is not coherent, which
/// means the slots are not being given the same events.
pub fn plugin_test(filter: Option<String>) {
    /// Every slot at the same gain, so the peaks below are comparable, and low
    /// enough that three of them summed still fit under full scale.
    const LAYER_GAIN: f32 = 0.5;
    /// Three layers, one per channel: the count the old three-slot rack had,
    /// kept because +6.0/+9.5 dB are the numbers the table below explains.
    const LAYERS: usize = 3;
    /// Long enough to cover the phrase and its release tail.
    const MEASURE_SECONDS: f64 = 3.5;

    let filter = filter.unwrap_or_else(|| "piano".to_string());
    let Some(bundle) = ivory_host::discover().into_iter().find(|p| {
        p.file_name()
            .map(|n| {
                n.to_string_lossy()
                    .to_lowercase()
                    .contains(&filter.to_lowercase())
            })
            .unwrap_or(false)
    }) else {
        eprintln!("no VST3 matching {filter:?}");
        std::process::exit(1);
    };

    let mut engine = match Engine::start(None) {
        Ok(e) => e,
        Err(why) => {
            eprintln!("could not open an audio output: {why}");
            std::process::exit(1);
        }
    };
    println!(
        "output:   {} ({})",
        engine.output().device,
        engine.output().latency_line()
    );

    println!(
        "loading:  {} into channel 1's first bay (this blocks for the warm-up)",
        bundle.display()
    );
    let loaded = match engine.load_insert(0, 0, Some(&bundle), None) {
        Ok(Some(l)) => l,
        Ok(None) => {
            eprintln!("the bay refused the path");
            std::process::exit(1);
        }
        Err(why) => {
            eprintln!("could not load the instrument: {why}");
            std::process::exit(1);
        }
    };
    println!(
        "instrument: {} [{}], {} channels at {} Hz",
        loaded.class, loaded.vendor, loaded.channels, loaded.sample_rate
    );

    // ── the state, which is what makes a preset survive a restart ───────────
    //
    // Read here rather than at the end, because the other two channels are
    // loaded WITH it: the layering measurement below rests on the three
    // instruments being identical, and "restored from channel 1's own bytes"
    // is a stronger claim than "the same plugin file".
    let preset = engine.save_insert_state(0, 0);
    match &preset {
        Some(b) => println!("state:    {} bytes from channel 1", b.len()),
        None => println!("state:    this instrument handed over none"),
    }

    for ch in 0..LAYERS {
        engine.set_channel_gain(ch, LAYER_GAIN);
    }
    engine.set_metronome_gain(0.6);
    engine.set_beats_per_bar(4);
    engine.set_metronome_in_take(false);

    // The recorder's read end, taken exactly as `record.rs`'s writer thread
    // will take it. Drained below on every poll: a tap nobody reads fills its
    // ring in four seconds and reports a colossal `dropped` count, which is
    // correct behaviour and a very confusing probe.
    let mut tap = engine
        .take_recorder_tap()
        .expect("the tap has not been taken");
    // Arm it: the ring has been running since the device opened, right through
    // the five-second warm-up, so it is full of monitor audio nobody asked for.
    tap.arm();
    let mut scratch: Vec<f32> = Vec::new();
    let mut tap_frames = 0usize;
    let mut tap_peak = 0.0f32;

    /// Move whatever is waiting into the totals and report the loudest sample
    /// in it. A free function rather than a closure so the totals stay ordinary
    /// locals that can be read between calls.
    fn drain(tap: &mut RecorderTap, scratch: &mut Vec<f32>, frames: &mut usize) -> f32 {
        scratch.clear();
        *frames += tap.drain(scratch);
        scratch.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    }

    /// The same ii-V-I `record_plugin.rs` plays, and for the same reason: the
    /// chord onsets are deliberately not multiples of the block size, so every
    /// event has to be placed by the clock rather than by rounding.
    ///
    /// A function rather than a straight line of code because it is played over
    /// and over: once per layering measurement, and then again for as long as
    /// somebody has an editor open and needs something to hear.
    fn play_phrase(engine: &Engine) {
        let t0 = engine.timebase().now() + 200_000_000;
        let chords: [(&[u8], i64); 4] = [
            (&[50, 62, 65, 69, 72], 0),
            (&[43, 62, 65, 67, 71], 913),
            (&[48, 64, 67, 71, 74], 1_847),
            (&[36, 48, 55, 64, 67], 2_791),
        ];
        for (notes, at) in chords {
            for (i, pitch) in notes.iter().enumerate() {
                let on = t0 + (at + i as i64 * 7) * 1_000_000;
                engine.send_midi(on, &[0x90, *pitch, 72 + i as u8 * 6]);
                engine.send_midi(on + 880_000_000, &[0x80, *pitch, 64]);
            }
        }
    }

    /// Play the phrase once and report what it did: the loudest sample in the
    /// device mix, the loudest in the tap, and the process's CPU seconds per
    /// second of wall clock while it rang.
    ///
    /// **Layering is judged on the TAP peak and not the device peak**, because
    /// the tap is instruments-only by construction and the device mix is not.
    /// Measured before this was: the count-in's last click was still ringing
    /// when the first window opened and it is louder than the piano, so the
    /// device read 0.5841 against a tap of 0.2807 for the same phrase, and the
    /// one-slot reference was the metronome. Hence the settle below as well.
    fn measure(
        engine: &Engine,
        tap: &mut RecorderTap,
        scratch: &mut Vec<f32>,
        frames: &mut usize,
        seconds: f64,
    ) -> (f32, f32, Option<f64>) {
        // Let the previous phrase's release and any click finish sounding. A
        // peak is a maximum over the window, so anything still ringing when the
        // window opens is this measurement's answer.
        let settle = Instant::now() + Duration::from_millis(900);
        while Instant::now() < settle {
            let _ = drain(tap, scratch, frames);
            ivory_host::editor::pump(Duration::from_millis(8));
        }
        // Whatever is left over is not this phrase's. Reading the meter clears
        // its peak (it is read-and-clear by design), and the tap is drained to
        // the same end.
        engine.meters();
        let _ = drain(tap, scratch, frames);
        let cpu0 = cpu_seconds();
        let began = Instant::now();
        play_phrase(engine);
        let (mut peak, mut heard_in_tap) = (0.0f32, 0.0f32);
        while began.elapsed().as_secs_f64() < seconds {
            peak = peak.max(engine.meters().peak());
            heard_in_tap = heard_in_tap.max(drain(tap, scratch, frames));
            // `pump` and not `sleep`: with a plugin editor on screen this is
            // the only thing servicing its window.
            ivory_host::editor::pump(Duration::from_millis(8));
        }
        let elapsed = began.elapsed().as_secs_f64();
        let cores = match (cpu0, cpu_seconds()) {
            (Some(a), Some(b)) if elapsed > 0.0 => Some((b - a) / elapsed),
            _ => None,
        };
        (peak, heard_in_tap, cores)
    }

    println!("counting in 4 beats at 96 bpm...");
    engine.start_count_in(4, 96.0);
    let mut shown = 0;
    let mut click_in_tap = 0.0f32;
    while !engine.count_in_done() {
        if let Some(beat) = engine.metronome_beat() {
            if beat != shown {
                shown = beat;
                println!("  {beat}");
            }
        }
        click_in_tap = click_in_tap.max(drain(&mut tap, &mut scratch, &mut tap_frames));
        // `pump` and not `sleep`, for the same reason as in `measure`.
        ivory_host::editor::pump(Duration::from_millis(2));
    }
    println!("go.");

    // ── layering, measured ──────────────────────────────────────────────────
    //
    // The metronome stays OFF through this: the peaks below are supposed to be
    // the instruments and nothing else, and a click landing inside the window
    // would be counted as part of the sum.
    println!(
        "\nlayering: the same instrument on each of three channels, every fader at gain {LAYER_GAIN}"
    );
    let mut rows: Vec<(usize, f32, f32, Option<f64>)> = Vec::new();
    for slot in 0..LAYERS {
        if slot > 0 {
            println!("  loading channel {} with the same instrument...", slot + 1);
            if let Err(why) = engine.load_insert(slot, 0, Some(&bundle), preset.as_deref()) {
                println!("  channel {} would not load: {why}", slot + 1);
                break;
            }
        }
        let (peak, in_tap, cores) = measure(
            &engine,
            &mut tap,
            &mut scratch,
            &mut tap_frames,
            MEASURE_SECONDS,
        );
        tap_peak = tap_peak.max(in_tap);
        rows.push((slot + 1, peak, in_tap, cores));
    }

    println!("\n  chans  device peak   tap peak   tap vs one   process CPU");
    let one = rows.first().map(|r| r.2).unwrap_or(0.0);
    for (n, peak, in_tap, cores) in &rows {
        let db = if one > 0.0 && *in_tap > 0.0 {
            format!("{:+.1} dB", 20.0 * (in_tap / one).log10())
        } else {
            "-".to_string()
        };
        let cpu = match cores {
            Some(c) => format!("{c:.2} cores"),
            None => "not measured".to_string(),
        };
        println!("  {n:>5}  {peak:>11.4}  {in_tap:>9.4}  {db:>11}   {cpu}");
    }
    println!(
        "  two slots must be about +6.0 dB on one and three about +9.5 dB: the same\n  \
         instrument playing the same notes sums coherently, so a peak that did not\n  \
         move is a channel that never got the note. Read the TAP column for that -\n  \
         the device column is what you hear, which includes the click."
    );

    // ── the plugins' own editors ────────────────────────────────────────────
    //
    // Opened here because this probe is the only place the whole chain exists
    // at once: a signed bundle (which is what a plugin with a licence check
    // will talk to), a real output device, and a window. A human can open
    // the modelled piano's UI on slot 1, change the instrument, and hear it change against
    // the other two — which is the one claim about this feature that no test can
    // make.
    println!(
        "\neditor:   {}",
        if engine.insert_has_editor(0, 0) {
            "yes"
        } else {
            "this plugin has none"
        }
    );
    if engine.insert_has_editor(0, 0) {
        // This process will never call `[NSApp run]`: `main.rs` exits before
        // eframe starts. Without this the window opens behind everything, never
        // takes the keyboard, and looks broken.
        ivory_host::editor::become_foreground();
        match engine.open_insert_editor(0, 0) {
            Ok(()) => println!("          channel 1 open. Close the window to end this probe."),
            Err(why) => println!("          could not open it: {why}"),
        }
    }

    // The click joins in from here: everything that had to be measured has
    // been, and what is left is a human listening to three instruments.
    engine.set_metronome_enabled(true);
    play_phrase(&engine);
    // Five seconds when there is nothing to look at, which is what this probe
    // always did. With an editor open it runs until the window is closed, and
    // the phrase repeats so there is always something playing while the
    // instrument is being changed underneath it.
    let deadline = if engine.insert_editor_open(0, 0) {
        None
    } else {
        Some(Instant::now() + Duration::from_secs(5))
    };
    let mut next_phrase = Instant::now() + Duration::from_secs(6);
    let mut peak = rows.iter().fold(0.0f32, |a, r| a.max(r.1));
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        // 16 ms, so this loop runs at the rate the app's own frame loop would.
        ivory_host::editor::pump(Duration::from_millis(16));
        engine.poll_editor();
        if deadline.is_none() && !engine.insert_editor_open(0, 0) {
            println!("\neditor closed.");
            break;
        }
        peak = peak.max(engine.meters().peak());
        tap_peak = tap_peak.max(drain(&mut tap, &mut scratch, &mut tap_frames));
        if Instant::now() >= next_phrase {
            next_phrase = Instant::now() + Duration::from_secs(6);
            play_phrase(&engine);
        }
    }
    // Closing the window must not have stopped anything. The instruments are
    // still loaded and still rendering, and the next two seconds prove it.
    if deadline.is_none() {
        let check = Instant::now() + Duration::from_secs(2);
        let before = engine.callbacks();
        play_phrase(&engine);
        let mut after_peak = 0.0f32;
        while Instant::now() < check {
            after_peak = after_peak.max(engine.meters().peak());
            tap_peak = tap_peak.max(drain(&mut tap, &mut scratch, &mut tap_frames));
            std::thread::sleep(Duration::from_millis(16));
        }
        println!(
            "after the editor: {} more callbacks, peak {after_peak:.4} - the instruments \
             are running instruments, not windows",
            engine.callbacks() - before
        );
        peak = peak.max(after_peak);
    }

    // Did anything the user did in the editor reach the bytes? This is the one
    // end of the persistence feature no test can check: pick a different piano
    // in the modelled piano's own UI, close the window, and watch the state change.
    match (preset.as_deref(), engine.save_insert_state(0, 0)) {
        (Some(before), Some(after)) if before == after.as_slice() => {
            println!("state:           unchanged, {} bytes", after.len());
        }
        (Some(before), Some(after)) => println!(
            "state:           CHANGED, {} bytes -> {} bytes. That is the preset the \
             next launch would restore.",
            before.len(),
            after.len()
        ),
        (_, after) => println!("state:           {:?} bytes now", after.map(|b| b.len())),
    }

    // ── unloading, one slot at a time ───────────────────────────────────────
    //
    // The other slots must keep playing while one is retired, which is the
    // property the per-slot handoff exists for. Slot 2 goes and slots 1 and 3
    // are asked to make a sound afterwards.
    if rows.len() == LAYERS {
        let _ = engine.load_insert(1, 0, None, None);
        engine.meters();
        play_phrase(&engine);
        let until = Instant::now() + Duration::from_secs(2);
        let mut after = 0.0f32;
        while Instant::now() < until {
            after = after.max(engine.meters().peak());
            tap_peak = tap_peak.max(drain(&mut tap, &mut scratch, &mut tap_frames));
            ivory_host::editor::pump(Duration::from_millis(16));
        }
        println!(
            "\nchannel 2 unloaded: peak {after:.4} from the other two - unloading one \
             instrument must not silence its neighbours"
        );
        println!(
            "loaded now:      {:?}",
            (0..LAYERS)
                .map(|s| engine.insert(s, 0).map(|l| l.class.as_str()))
                .collect::<Vec<_>>()
        );
        peak = peak.max(after);
    }

    println!("\npeak heard:      {peak:.4} (device mix: instruments plus click)");
    println!(
        "tap captured:    {tap_frames} frames of {}-channel audio at {} Hz, peak {tap_peak:.4}",
        tap.channels(),
        tap.sample_rate()
    );
    println!(
        "click in tap:    peak {click_in_tap:.4} during the count-in - must be 0.0000, \
         because metronome_in_take is off"
    );
    println!(
        "output delay:    {:.2} ms (measured, converter delay not included)",
        engine.output_delay_ns() as f64 / 1e6
    );
    println!("callbacks:       {}", engine.callbacks());
    println!("midi dropped:    {}", engine.midi_dropped());
    println!("tap frames lost: {}", tap.dropped());
    println!(
        "pedal messages:  {} seen and NOT delivered - see instrument.rs",
        engine.pedal_dropped()
    );
    if let Some(why) = engine.fault() {
        println!("fault:           {why}");
    }
    if peak < 1e-4 {
        println!("\nSILENT - nothing reached the output.");
        std::process::exit(2);
    }
}

/// Notices an output that has gone quiet without saying so.
///
/// The callback count is the only truth the engine has about its own stream:
/// `is_running` stays true after a device dies silently (the error callback
/// only fires when the backend ADMITS the death, and ASIO's does not when the
/// SDK unloads a driver out from under it), so the watchdog is "the count
/// stopped climbing while the stream claims to run". This was the invisible
/// half of the ASIO silence: the owner's Focusrite opened, initialized, showed
/// every sign of life, and played nothing — with `engine_error` empty and
/// stderr blank, because nothing read these counters. Now something does.
///
/// Pure — fed readings, answers "stalled?" — so it is testable without a sound
/// card. The caller decides what recovery looks like (drop + reopen).
#[derive(Debug, Clone, Copy)]
pub struct StallWatch {
    /// The last callback count seen, and when it last MOVED.
    seen: Option<(u64, std::time::Instant)>,
}

/// How long the callback count may sit still, while the stream claims to run,
/// before the output is declared dead.
///
/// Two seconds is ~180 missed callbacks at typical sizes — far past jitter,
/// system sleep excepted (and a rebuild on wake is the right call anyway) —
/// while short enough that "I switched to ASIO and hear nothing" becomes a
/// message and a recovery in the time it takes to reach for the fader.
pub const STALL_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

impl StallWatch {
    pub fn new() -> Self {
        Self { seen: None }
    }

    /// Feed one reading; `true` means the output is stalled and should be
    /// rebuilt. `now` is a parameter so tests need not sleep.
    pub fn note(&mut self, running: bool, callbacks: u64, now: std::time::Instant) -> bool {
        if !running {
            // A stream that admits it stopped is the FAULT path's business,
            // not the watchdog's; stop measuring so a later restart begins
            // fresh.
            self.seen = None;
            return false;
        }
        match self.seen {
            Some((count, _)) if count != callbacks => {
                self.seen = Some((callbacks, now));
                false
            }
            Some((_, since)) => now.duration_since(since) >= STALL_AFTER,
            None => {
                // First reading: start the clock here. A device that never
                // serves ONE callback is exactly the dead-driver case, and
                // this is what catches it — the count never moves off its
                // initial value and the clock runs out.
                self.seen = Some((callbacks, now));
                false
            }
        }
    }
}

impl Default for StallWatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {

    /// **The mono fold sums to the MEAN, not the sum.**
    ///
    /// Two identical channels added together are 6 dB louder than either, so
    /// a fold that summed would clip every mono check and teach the user
    /// that mono is loud rather than what mono does to their image.
    #[test]
    fn folding_to_mono_keeps_the_level() {
        // Hard left, nothing right: the fold puts half of it in both.
        let mut buf = [1.0f32, 0.0, 0.5, -0.5, 0.3, 0.3];
        super::fold_mono(&mut buf, true);
        assert!((buf[0] - 0.5).abs() < 1e-6, "{buf:?}");
        assert!((buf[1] - 0.5).abs() < 1e-6, "{buf:?}");
        // Out of phase cancels, which is the whole reason to press it.
        assert!(buf[2].abs() < 1e-6, "{buf:?}");
        assert!(buf[3].abs() < 1e-6, "{buf:?}");
        // Already mono comes out UNCHANGED — not 6 dB louder.
        assert!((buf[4] - 0.3).abs() < 1e-6, "{buf:?}");
        assert!((buf[5] - 0.3).abs() < 1e-6, "{buf:?}");

        // Off is a no-op, and the branch is outside the loop for a reason:
        // this runs on the audio callback.
        let mut same = [1.0f32, 0.0];
        super::fold_mono(&mut same, false);
        assert_eq!(same, [1.0, 0.0]);
    }
    use super::*;

    // ── the output stall watchdog ──────────────────────────────────────────

    #[test]
    fn a_climbing_callback_count_is_never_a_stall() {
        let mut w = StallWatch::new();
        let t0 = std::time::Instant::now();
        assert!(!w.note(true, 0, t0));
        for i in 1..100u64 {
            assert!(
                !w.note(true, i, t0 + STALL_AFTER * 3 * i as u32),
                "a count that moved since last look stalled at step {i}"
            );
        }
    }

    #[test]
    fn a_count_that_stops_climbing_while_running_is_a_stall_after_the_grace() {
        let mut w = StallWatch::new();
        let t0 = std::time::Instant::now();
        w.note(true, 41, t0);
        w.note(true, 42, t0 + std::time::Duration::from_millis(10));
        // Still inside the grace: quiet, but not yet declared.
        assert!(!w.note(true, 42, t0 + std::time::Duration::from_millis(500)));
        // Past it: dead. This is the ASIO driver unloaded under a live stream.
        assert!(w.note(
            true,
            42,
            t0 + std::time::Duration::from_millis(10) + STALL_AFTER
        ));
    }

    #[test]
    fn a_device_that_never_serves_one_callback_is_caught_too() {
        // The count never moves off zero; the first reading starts the clock.
        let mut w = StallWatch::new();
        let t0 = std::time::Instant::now();
        assert!(!w.note(true, 0, t0));
        assert!(w.note(true, 0, t0 + STALL_AFTER));
    }

    #[test]
    fn a_stream_that_admits_stopping_is_not_the_watchdogs_business() {
        let mut w = StallWatch::new();
        let t0 = std::time::Instant::now();
        w.note(true, 7, t0);
        // The fault path owns this: running=false must not read as a stall,
        // and must reset the clock so a restart starts fresh.
        assert!(!w.note(false, 7, t0 + STALL_AFTER * 5));
        assert!(!w.note(
            true,
            7,
            t0 + STALL_AFTER * 5 + std::time::Duration::from_millis(1)
        ));
    }

    // ── choosing the output device by name ─────────────────────────────────

    #[test]
    fn an_output_name_matches_exactly_before_it_matches_loosely() {
        let names = vec![
            "Focusrite USB ASIO".to_string(),
            "Focusrite Thunderbolt ASIO".to_string(),
        ];
        // An exact name takes its own row even though "Focusrite" is a
        // substring of both — otherwise the first loose hit would win and the
        // choice would depend on registry order.
        assert_eq!(
            match_output_name(&names, "Focusrite Thunderbolt ASIO"),
            Some(1)
        );
    }

    #[test]
    fn an_output_name_matches_a_human_typed_substring() {
        let names = vec![
            "Speakers (Realtek High Definition Audio)".to_string(),
            "Focusrite USB ASIO".to_string(),
        ];
        assert_eq!(match_output_name(&names, "focusrite usb"), Some(1));
    }

    #[test]
    fn an_unmatched_output_name_selects_nothing_rather_than_the_first_row() {
        // The killer to avoid: a name that matches nothing must NOT silently
        // fall through to device zero. resolve_output turns a None here into a
        // loud error naming what the machine has, not a wrong device opened
        // without a word — see the ASIO Thunderbolt-vs-USB silence.
        let names = vec![
            "Focusrite Thunderbolt ASIO".to_string(),
            "Focusrite USB ASIO".to_string(),
        ];
        assert_eq!(match_output_name(&names, "MOTU"), None);
    }

    // ── raw MIDI to a VST3 note ────────────────────────────────────────────

    #[test]
    fn a_note_on_becomes_a_note_on_with_its_velocity_scaled_to_a_fraction() {
        let n = note_from_midi(0x90, 60, 100).expect("a note-on is a note");
        assert!(n.on);
        assert_eq!(n.pitch, 60);
        assert!(
            (n.velocity - 100.0 / 127.0).abs() < 1e-6,
            "velocity came out {}; VST3 wants 0.0..=1.0 and passing the MIDI byte \
             makes every note fortissimo and clipped",
            n.velocity
        );
        assert!(n.velocity <= 1.0);
    }

    #[test]
    fn a_note_on_with_velocity_zero_is_a_note_off() {
        // Half of all keyboards release notes this way, because it lets them
        // stay in running status. A host that reads it as a quiet note-on
        // leaves every note of the performance held down.
        let n = note_from_midi(0x90, 60, 0).expect("velocity 0 is still a note event");
        assert!(!n.on, "note-on velocity 0 was treated as a note-on");
        assert_eq!(n.pitch, 60);
        assert!(
            n.velocity > 0.0,
            "a release velocity of exactly 0 is a different sound on any piano \
             that maps it to damper noise"
        );
    }

    #[test]
    fn a_note_off_carries_its_release_velocity_through() {
        let n = note_from_midi(0x80, 72, 64).expect("a note-off is a note");
        assert!(!n.on);
        assert_eq!(n.pitch, 72);
        assert!((n.velocity - 64.0 / 127.0).abs() < 1e-6);
    }

    #[test]
    fn notes_are_read_from_every_midi_channel_not_only_channel_one() {
        for channel in 0..16u8 {
            let on = note_from_midi(0x90 | channel, 64, 90).expect("channel {channel}");
            assert!(on.on);
            let off = note_from_midi(0x80 | channel, 64, 0).expect("channel {channel}");
            assert!(!off.on);
        }
    }

    #[test]
    fn channel_messages_that_are_not_notes_are_ignored() {
        // Control change, program change, pitch bend, aftertouch, poly
        // aftertouch, and every system message.
        for status in [0xB0, 0xC0, 0xD0, 0xE0, 0xA0, 0xF0, 0xF8, 0xFE, 0xFF] {
            assert!(
                note_from_midi(status, 64, 100).is_none(),
                "status {status:#04x} was read as a note"
            );
        }
    }

    #[test]
    fn a_data_byte_mistaken_for_a_status_byte_is_refused_rather_than_masked_into_a_note() {
        // Running status resolved wrongly, or a truncated buffer. `0x10 & 0xF0`
        // is not 0x90, but `0x90 & 0x7F` is 0x10, and a reader that masks first
        // invents notes out of pitch bytes.
        for status in 0x00..0x80u8 {
            assert!(note_from_midi(status, 60, 100).is_none(), "{status:#04x}");
        }
    }

    #[test]
    fn a_pitch_or_velocity_with_its_high_bit_set_is_masked_rather_than_overflowing() {
        let n = note_from_midi(0x90, 0xFF, 0xFF).expect("still a note-on");
        assert_eq!(n.pitch, 127);
        assert!(n.velocity <= 1.0);
    }

    #[test]
    fn the_three_pedals_are_recognised_only_so_that_they_can_be_counted() {
        // They cannot be delivered: `Instance::process` takes notes and nothing
        // else. Counting them is what makes the gap visible instead of leaving
        // a pianist to discover it mid-phrase.
        assert!(is_pedal(0xB0, 64), "sustain");
        assert!(is_pedal(0xB5, 66), "sostenuto");
        assert!(is_pedal(0xB0, 67), "soft");
        assert!(!is_pedal(0xB0, 1), "the mod wheel is not a pedal");
        assert!(!is_pedal(0x90, 64), "a note at pitch 64 is not a pedal");
        assert!(note_from_midi(0xB0, 64, 127).is_none());
    }

    // ── event placement ────────────────────────────────────────────────────

    const RATE: f64 = 48_000.0;

    #[test]
    fn an_event_stamped_before_the_block_lands_at_offset_zero_rather_than_being_dropped() {
        // This is every live note: a block rendered at T can only carry events
        // known at T, so all of them are in the past. Offset 0 is the earliest
        // frame that has not been rendered yet.
        assert_eq!(place(0, 1_000_000, RATE, 512), Placement::At(0));
        assert_eq!(place(-5_000_000, 0, RATE, 512), Placement::At(0));
        assert_eq!(place(i64::MIN, 0, RATE, 512), Placement::At(0));
    }

    #[test]
    fn an_event_stamped_after_the_block_is_held_for_the_next_one() {
        // 512 frames at 48 kHz is 10.667 ms.
        assert_eq!(place(11_000_000, 0, RATE, 512), Placement::Later);
        assert_eq!(place(10_666_667, 0, RATE, 512), Placement::Later);
        assert_eq!(place(i64::MAX, 0, RATE, 512), Placement::Later);
    }

    #[test]
    fn an_event_inside_the_block_lands_on_its_own_frame() {
        // 1 ms into a block at 48 kHz is frame 48; 5 ms is frame 240.
        assert_eq!(place(1_000_000, 0, RATE, 512), Placement::At(48));
        assert_eq!(place(5_000_000, 0, RATE, 512), Placement::At(240));
        // The last frame of the block is inside it; one sample later is not.
        assert_eq!(place(10_666_000, 0, RATE, 512), Placement::At(511));
        // And an offset is measured from the block's own start, not from zero.
        assert_eq!(
            place(101_000_000, 100_000_000, RATE, 512),
            Placement::At(48)
        );
    }

    #[test]
    fn a_nonsense_sample_rate_places_everything_at_zero_instead_of_dividing_by_it() {
        assert_eq!(place(5_000_000, 0, 0.0, 512), Placement::At(0));
        assert_eq!(place(5_000_000, 0, f64::NAN, 512), Placement::At(0));
    }

    // ── channel mapping ────────────────────────────────────────────────────

    #[test]
    fn a_mono_instrument_is_heard_from_both_speakers() {
        let mut dst = [0.0; 2];
        map_frame(&[0.5], &mut dst);
        assert_eq!(
            dst,
            [0.5, 0.5],
            "a mono plugin in one speaker reads as broken"
        );
    }

    #[test]
    fn a_stereo_instrument_maps_straight_through() {
        let mut dst = [0.0; 2];
        map_frame(&[0.25, -0.75], &mut dst);
        assert_eq!(dst, [0.25, -0.75]);
    }

    #[test]
    fn a_multi_output_instrument_contributes_its_first_two_channels_and_no_more() {
        // the modelled piano's eight are stem outputs of the SAME performance; summing
        // them would be the same piano eight times, 18 dB up and clipped.
        let src = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let mut dst = [9.0; 2];
        map_frame(&src, &mut dst);
        assert_eq!(dst, [0.1, 0.2]);
    }

    #[test]
    fn a_mono_device_gets_the_average_and_not_the_sum() {
        let mut dst = [9.0; 1];
        map_frame(&[0.5, 0.5], &mut dst);
        assert_eq!(
            dst,
            [0.5],
            "summing to mono is +6 dB and clips a correct mix"
        );
        map_frame(&[0.5], &mut dst);
        assert_eq!(dst, [0.5]);
    }

    #[test]
    fn a_device_with_more_channels_than_the_instrument_leaves_the_rest_silent() {
        let mut dst = [9.0; 6];
        map_frame(&[0.5, -0.5], &mut dst);
        assert_eq!(dst, [0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
        // Mono is the exception: it goes everywhere, because a mono instrument
        // has no left or right to be wrong about.
        map_frame(&[0.5], &mut dst);
        assert_eq!(dst, [0.5; 6]);
    }

    #[test]
    fn no_instrument_at_all_writes_silence_rather_than_the_previous_block() {
        let mut dst = [9.0; 2];
        map_frame(&[], &mut dst);
        assert_eq!(dst, [0.0, 0.0]);
    }

    // ── the beat clock ─────────────────────────────────────────────────────

    /// Run the beat clock for `frames` and return every beat it fired, with the
    /// frame it fired on. No device, no click, no audio.
    fn beats_over(b: &mut Beats, frames: usize, on: bool, bpb: u32) -> Vec<(usize, Beat)> {
        (0..frames)
            .filter_map(|i| b.tick(on, bpb).map(|beat| (i, beat)))
            .collect()
    }

    #[test]
    fn a_count_in_is_measured_in_beats_and_recording_starts_one_beat_after_the_last_one() {
        // 120 bpm at 48 kHz is 24000 frames per beat. Four beats of count-in
        // click at frames 0, 24000, 48000 and 72000, and the downbeat that ends
        // it is at 96000 — one further beat, which is what "the downbeat after
        // them" means.
        let mut b = Beats::new(48_000.0, 120.0);
        b.restart(4);
        let fired = beats_over(&mut b, 120_000, false, 4);
        let counted: Vec<_> = fired.iter().filter(|(_, x)| x.count_in > 0).collect();
        assert_eq!(counted.len(), 4, "expected four count-in beats");
        assert_eq!(counted[0].0, 0);
        assert_eq!(counted[1].0, 24_000);
        assert_eq!(counted[2].0, 48_000);
        assert_eq!(counted[3].0, 72_000);
        assert_eq!(
            counted.iter().map(|(_, x)| x.count_in).collect::<Vec<_>>(),
            vec![1, 2, 3, 4],
            "the countdown must be 1-based and in order"
        );
        let down: Vec<_> = fired.iter().filter(|(_, x)| x.downbeat).collect();
        assert_eq!(down.len(), 1);
        assert_eq!(
            down[0].0, 96_000,
            "recording starts a whole beat after beat 4"
        );
    }

    #[test]
    fn a_count_in_sounds_even_when_the_metronome_is_switched_off() {
        // A count-in nobody can hear is not one. This is the whole reason the
        // clock takes `on` rather than being gated by it.
        let mut b = Beats::new(48_000.0, 120.0);
        b.restart(2);
        let fired = beats_over(&mut b, 80_000, false, 4);
        assert_eq!(fired.len(), 3, "two count-in beats and the downbeat");
        // And once it is over, a switched-off metronome is silent again.
        let after = beats_over(&mut b, 80_000, false, 4);
        assert!(after.is_empty());
    }

    #[test]
    fn the_beat_clock_does_not_drift_on_a_tempo_whose_period_is_not_a_whole_number() {
        // 44100 Hz at 97 bpm is 27278.35 frames per beat. Truncating loses a
        // beat every hour and a half, which is exactly long enough to be blamed
        // on something else.
        let mut b = Beats::new(44_100.0, 97.0);
        b.restart(0);
        b.was_on = true;
        let fired = beats_over(&mut b, 44_100 * 60, true, 4);
        let expected = (44_100.0 * 60.0 / (44_100.0 * 60.0 / 97.0)) as usize;
        assert!(
            fired.len().abs_diff(expected) <= 1,
            "fired {} beats in a minute at 97 bpm, expected about {expected}",
            fired.len()
        );
        let last = fired.last().expect("some beats").0 as f64;
        let ideal = (fired.len() - 1) as f64 * (44_100.0 * 60.0 / 97.0);
        assert!(
            (last - ideal).abs() < 2.0,
            "the last beat landed at {last} and should be at {ideal:.1}"
        );
    }

    /// **A delay line delays by exactly what it was asked for**, across a
    /// block boundary and at any block size.
    ///
    /// The arithmetic that aligns a desk. An off-by-one here is a channel a
    /// sample early for ever — inaudible on its own, and a comb filter the
    /// moment it meets the same material on another lane.
    #[test]
    fn a_compensating_delay_holds_a_signal_back_by_exactly_its_length() {
        for len in [0usize, 1, 7, 64, 511] {
            for block in [1usize, 3, 64, 256] {
                let mut line = DelayLine::new();
                // A ramp, so every frame is distinguishable from every other.
                let total = 1_024usize;
                let mut got = Vec::with_capacity(total);
                let mut sent = 0usize;
                while sent < total {
                    let n = block.min(total - sent);
                    let mut planar: Vec<Vec<f32>> = (0..TAP_CHANNELS)
                        .map(|_| (0..n).map(|f| (sent + f) as f32).collect())
                        .collect();
                    line.run(&mut planar, n, len);
                    got.extend_from_slice(&planar[0][..n]);
                    sent += n;
                }
                for (i, v) in got.iter().enumerate() {
                    // Before the line has filled, it puts out the silence it
                    // started with; after that, the input from `len` frames
                    // ago. Both are exactly what a delay is.
                    let want = if i < len { 0.0 } else { (i - len) as f32 };
                    assert!(
                        (*v - want).abs() < 1e-6,
                        "len={len} block={block}: frame {i} came out {v}, wanted {want}"
                    );
                }
            }
        }
    }

    /// Changing the length clears the ring rather than reinterpreting it.
    ///
    /// A plugin swap is allowed to click once. It is not allowed to smear
    /// for ever, which is what reading old samples at a new alignment does.
    #[test]
    fn changing_the_compensation_does_not_reinterpret_old_samples() {
        let mut line = DelayLine::new();
        let mut planar: Vec<Vec<f32>> =
            (0..TAP_CHANNELS).map(|_| vec![1.0; 64]).collect();
        line.run(&mut planar, 64, 32);
        // Now ask for a different length: what comes out is silence, not
        // somebody else's samples read at the wrong offset.
        let mut next: Vec<Vec<f32>> = (0..TAP_CHANNELS).map(|_| vec![1.0; 64]).collect();
        line.run(&mut next, 64, 16);
        assert!(
            next[0][..16].iter().all(|v| v.abs() < 1e-6),
            "the ring was reinterpreted: {:?}",
            &next[0][..16]
        );
        // ...and it is delaying by the NEW length from then on.
        assert!(next[0][16..].iter().all(|v| (*v - 1.0).abs() < 1e-6));
    }

    /// **Before the compensated zero there is silence, not a held sample.**
    ///
    /// The clips wait for the graph's latency by reading `p - pdc`. Written
    /// with `saturating_sub` that pins `p` at zero for the first `pdc`
    /// frames — exactly the window compensation exists for — so a clip
    /// starting at 0:00 emitted its first sample over and over while the
    /// plugin lane's own delay line correctly put out silence. A DC step,
    /// then a discontinuity, on every play-from-zero with a lookahead plugin
    /// loaded.
    ///
    /// This asserts the arithmetic rather than the mixer, because the bug
    /// was the arithmetic: an unrepresentable project frame has to be
    /// ABSENT, not clamped.
    #[test]
    fn a_clip_reads_nothing_before_the_compensated_zero() {
        let pdc = 128u32;
        // Inside the compensation window there is no project frame at all.
        for f in 0..pdc as u64 {
            assert!(
                f.checked_sub(u64::from(pdc)).is_none(),
                "frame {f} produced a project frame inside the window"
            );
            // ...and the old arithmetic pinned every one of them to zero,
            // which is what made a clip repeat its first sample.
            assert_eq!(f.saturating_sub(u64::from(pdc)), 0);
        }
        // At the boundary and after it, the frame is the real one, offset.
        for f in [pdc as u64, pdc as u64 + 1, 48_000] {
            assert_eq!(
                f.checked_sub(u64::from(pdc)),
                Some(f - u64::from(pdc)),
                "frame {f} did not compensate cleanly"
            );
        }
        // With no compensation the two agree, which is why nothing showed up
        // until a plugin with lookahead was loaded.
        for f in [0u64, 1, 999] {
            assert_eq!(f.checked_sub(0), Some(f.saturating_sub(0)));
        }
    }

    /// Write one bay's figure the way `set_seat_latency` does, without a
    /// plugin: what is asserted below is the arithmetic and the indexing,
    /// and a real VST3 would only be a slower way to reach it.
    fn seat(shared: &Shared, strip: usize, bay: usize, frames: u32) -> bool {
        Rack::set_latency(shared, strip, bay, frames)
    }

    /// A rack's latency is the SUM of its bays.
    #[test]
    fn a_chains_latency_is_the_sum_of_what_is_in_it() {
        let shared = Shared::new();
        assert_eq!(Rack::latency(&shared, 0), 0);
        assert!(seat(&shared, 0, 0, 64), "0 -> 64 is a change");
        assert!(!seat(&shared, 0, 0, 64), "and writing it again is not");
        assert_eq!(Rack::latency(&shared, 0), 64);
        // In SERIES: three plugins looking ahead 64 each delay by 192.
        seat(&shared, 0, 1, 64);
        seat(&shared, 0, 2, 64);
        assert_eq!(Rack::latency(&shared, 0), 192);
        // **Each seat is its own**, which is the whole reason the figures
        // live in an indexed table rather than inside the boxes: a bay is
        // read by the seat it sits in, and one rack's contents never leak
        // into another's total.
        assert_eq!(Rack::latency(&shared, 1), 0, "channel 1 has nothing in it");
        // **A voice is where the signal BEGINS**, so its own figure is not a
        // delay imposed on anything upstream — there is no upstream, and
        // counting it would push every other lane late to match a delay
        // nobody can hear. It is kept out at the source: a voice is given no
        // `LatencyHandle` at load and writes a zero into its bay, so by the
        // time the sum runs there is nothing to filter.
        assert!(
            !seat(&shared, 1, 0, 0),
            "a bay that was already empty has not moved"
        );
        seat(&shared, 1, 1, 32);
        assert_eq!(Rack::latency(&shared, 1), 32);
        // A seat past the end of the table is a seat with nothing in it, and
        // says so instead of panicking: this is reached from the load path.
        assert!(!seat(&shared, RACK_SEATS, 0, 99));
    }

    /// **Two figures, because one number cannot answer both questions.**
    ///
    /// What everything is ALIGNED to is the channels alone. What is SHOWN is
    /// the whole delay, master included. A single atomic served both and got
    /// one of them wrong whichever way it was written: leave the master out
    /// and the readout lies about latency you are paying; put it in and
    /// every channel and every clip waits for the master's delay *before*
    /// the master adds it again on the way out, so a 30-frame mastering
    /// limiter costs 60 frames between the key and the sound.
    #[test]
    fn the_alignment_target_and_the_readout_are_different_numbers() {
        let shared = Arc::new(Shared::new());
        let (tap_tx, _tap_rx) = RingBuffer::<f32>::new(1 << 16);
        let r = test_renderer(Arc::clone(&shared), tap_tx, 2);

        // Nothing loaded: both are zero and there is nothing to tell apart.
        r.publish_latency();
        assert_eq!(shared.max_latency.load(Ordering::Relaxed), 0);
        assert_eq!(shared.total_latency.load(Ordering::Relaxed), 0);

        // A limiter on the MASTER only. Everything downstream of every
        // channel, so it pulls nothing out of phase with anything.
        seat(&shared, ivory_ui::recorder::STRIPS, 0, 30);
        r.publish_latency();
        assert_eq!(
            shared.max_latency.load(Ordering::Relaxed),
            0,
            "no channel should wait for a delay that is applied after all of them"
        );
        assert_eq!(
            shared.total_latency.load(Ordering::Relaxed),
            30,
            "but it is thirty frames somebody is paying, so it must be shown"
        );

        // Now a lookahead compressor on channel 3 alone.
        seat(&shared, 3, 1, 128);
        r.publish_latency();
        let (lanes, shown) = (
            std::array::from_fn::<u32, CHANNELS, _>(|i| {
                shared.lane_latency[i].load(Ordering::Relaxed)
            }),
            shared.total_latency.load(Ordering::Relaxed),
        );
        // **Per-lane, and the lane it was actually put in.** Asking every
        // channel for `STRIPS` — the master's seat — reads perfectly
        // plausibly and is wrong twice: the readout claims all sixteen cost
        // what the master costs, and one plugin on the master drags every
        // channel into compensating for a delay it does not have.
        assert_eq!(lanes[3], 128);
        assert_eq!(lanes[0], 0, "channel 0 is empty and must say so");
        assert_eq!(lanes[2], 0);
        assert_eq!(
            shared.max_latency.load(Ordering::Relaxed),
            128,
            "the other fifteen channels align to the slowest of them"
        );
        assert_eq!(shown, 158, "and the readout adds the master's on top");
    }

    /// **No site in the render path reads a fader behind automation's back.**
    ///
    /// The named riskiest failure of the whole automation feature: the gain
    /// a lane plays at is read in SEVEN places and its pan in FIVE, spread
    /// across the routing fold, the live monitor, the arrangement clips, the
    /// file preview, the drum pads and three separate paths inside
    /// `mix_channels`. A version that automated only `mix_clips` would look
    /// exactly right on an audio clip and do nothing at all to a MIDI lane
    /// or to somebody's own playing coming through the desk — and it would
    /// pass any test that only listened to a clip.
    ///
    /// So the atomics are read in ONE place each, and this is what keeps it
    /// that way. A source scan, following the two in `settings.rs`, because
    /// the rule is about where code may appear and no runtime assertion can
    /// say that.
    #[test]
    fn the_render_path_reads_no_fader_directly() {
        let src = include_str!("instrument.rs");
        // The render path is everything between the `Renderer`'s own impl and
        // the `Engine`'s — the tests below and the UI-thread setters above
        // are allowed to touch the atomics, and do.
        let from = src.find("impl Renderer {").expect("the renderer's impl moved");
        let to = src.find("impl Engine {").expect("the engine's impl moved");
        assert!(from < to, "the two impls are not in the order this assumes");
        let body = &src[from..to];

        for (field, accessor) in [
            ("channel_gains[", "lane_gain"),
            ("channel_pans[", "lane_pan"),
            // **Added after `send[` was missed on three of four paths.** The
            // accessor was written and only one site ever called it, so Send
            // automation moved the live monitor and nothing else — and this
            // scan, which exists precisely to catch that, was not looking
            // for it. A guard that covers two of the three controls it is
            // guarding is a guard somebody trusts by mistake.
            ("shared.send[", "lane_send"),
        ] {
            // The accessors themselves are the exception, and they are free
            // functions ABOVE the impl — so anything left in here is a site
            // that went round them.
            let stray: Vec<&str> = body
                .lines()
                .filter(|l| l.contains(field))
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            assert!(
                stray.is_empty(),
                "these lines read `{field}` directly instead of `{accessor}`, so \
                 automation does not reach them:\n{}",
                stray.join("\n")
            );
        }
    }

    /// Automation OVERRIDES the fader, and only where there is a curve.
    #[test]
    fn a_curve_takes_over_its_control_and_leaves_the_others_alone() {
        use ivory_ui::automation::{AutoParam, Automation, Curve, Point, Shape};
        let mut auto = AutoNow::default();
        let shared = Shared::new();
        // Fader at full, nothing automated: the fader wins.
        shared.channel_gains[3].store(1.0f32.to_bits(), Ordering::Relaxed);
        assert!((lane_gain_of(&auto, &shared, 3) - 1.0).abs() < 1e-6);

        // A curve at half FADER TRAVEL is not half amplitude — it goes
        // through the same decibel curve a mouse drag does.
        auto.gain[3] = Some(0.5);
        let want = ivory_ui::recorder::fader_to_gain(0.5);
        assert!(
            (lane_gain_of(&auto, &shared, 3) - want).abs() < 1e-6,
            "a curve at 0.5 gave {} rather than {want}",
            lane_gain_of(&auto, &shared, 3)
        );
        // ...and the lane beside it is untouched.
        shared.channel_gains[4].store(0.25f32.to_bits(), Ordering::Relaxed);
        assert!((lane_gain_of(&auto, &shared, 4) - 0.25).abs() < 1e-6);

        // Pan comes back off 0..1 onto -1..1, centre at half.
        auto.pan[2] = Some(0.5);
        assert!(lane_pan_of(&auto, &shared, 2).abs() < 1e-6, "half is not centre");
        auto.pan[2] = Some(0.0);
        assert!((lane_pan_of(&auto, &shared, 2) + 1.0).abs() < 1e-6, "zero is not hard left");

        // And the curve really is what feeds it.
        let mut a = Automation::default();
        a.set(Curve::sanitised(
            1,
            AutoParam::Gain,
            vec![
                Point { at_s: 0.0, value: 1.0, shape: Shape::Linear },
                Point { at_s: 10.0, value: 0.0, shape: Shape::Linear },
            ],
        ));
        let half = a.value_at(1, AutoParam::Gain, 5.0).expect("a value");
        assert!((half - 0.5).abs() < 1e-9, "halfway down gave {half}");
    }

    /// Walk a `BarClock` over a span of frames and answer where it fired.
    fn locked_beats(maps: &Maps, rate: f64, from: u64, frames: u64) -> Vec<(u64, bool)> {
        let mut c = BarClock::default();
        let mut out = Vec::new();
        for i in 0..frames {
            let pos = from + i;
            if let Some(b) = c.tick(pos, maps, rate) {
                out.push((pos, b.accent));
            }
        }
        out
    }

    fn maps_at(bpm: f64) -> Maps {
        Maps {
            tempo: ivory_ui::tempo::TempoMap::constant(bpm),
            meter: ivory_ui::tempo::MeterMap::default(),
        }
    }

    /// **The click lands on the SONG's bar lines, not on the keypress.**
    ///
    /// `Beats` counts from wherever the phase last restarted, so pressing
    /// play at bar 9 put the downbeat at start-of-playback. `BarClock` is
    /// derived from the transport's position, so starting anywhere lands the
    /// accents where the ruler draws them.
    #[test]
    fn the_locked_click_accents_the_songs_own_bars_wherever_play_began() {
        let rate = 48_000.0;
        let maps = maps_at(120.0);
        // 120 bpm, 4/4: a beat every 24000 frames, a bar every 96000.
        // Start midway through bar 2, a third of a beat in.
        let from = 96_000 * 2 + 24_000 + 8_000;
        let fired = locked_beats(&maps, rate, from, 96_000 * 2);
        assert!(!fired.is_empty(), "the locked click never fired");
        for (at, accent) in &fired {
            // Every beat is on a beat line of the song, wherever we began.
            let off = *at % 24_000;
            assert!(off == 0, "a beat landed {off} frames off the grid at {at}");
            // ...and the accents are the bar lines, again the song's.
            assert_eq!(
                *accent,
                at % 96_000 == 0,
                "the accent at {at} disagrees with the bar line"
            );
        }
    }

    /// A locate does not shift the grid; it is re-derived.
    #[test]
    fn the_locked_click_survives_a_locate_and_a_loop_wrap() {
        let rate = 48_000.0;
        let maps = maps_at(120.0);
        let mut c = BarClock::default();
        // Play a little from zero...
        for i in 0..30_000u64 {
            c.tick(i, &maps, rate);
        }
        // ...then jump somewhere unrelated, mid-beat, exactly as a loop wrap
        // or a click on the ruler does.
        let mut fired = Vec::new();
        for i in 0..96_000u64 {
            let pos = 480_000 + 5_000 + i;
            if let Some(b) = c.tick(pos, &maps, rate) {
                fired.push((pos, b.accent));
            }
        }
        assert!(!fired.is_empty(), "nothing fired after the jump");
        for (at, _) in &fired {
            assert_eq!(at % 24_000, 0, "the grid shifted after a locate: {at}");
        }
    }

    /// **The click follows a tempo map**, which a phase accumulator cannot.
    #[test]
    fn the_locked_click_follows_a_tempo_change() {
        let rate = 48_000.0;
        let maps = Maps {
            // 120 for the first two bars (8 quarters), then 60.
            tempo: ivory_ui::tempo::TempoMap::sanitised(vec![
                ivory_ui::tempo::TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: false },
                ivory_ui::tempo::TempoPoint { at_beat: 8.0, bpm: 60.0, ramp: false },
            ]),
            meter: ivory_ui::tempo::MeterMap::default(),
        };
        let fired = locked_beats(&maps, rate, 0, 48_000 * 12);
        // Before the change: 0.5s apart. After: 1.0s apart.
        let gaps: Vec<u64> = fired.windows(2).map(|w| w[1].0 - w[0].0).collect();
        assert!(gaps.len() > 8, "too few beats to judge: {gaps:?}");
        for g in &gaps[..7] {
            assert!(g.abs_diff(24_000) <= 1, "before the change a gap was {g}");
        }
        for g in &gaps[8..] {
            assert!(g.abs_diff(48_000) <= 1, "after the change a gap was {g}");
        }
    }

    /// A signature change moves how many beats a bar holds.
    #[test]
    fn the_locked_click_follows_a_signature_change() {
        let rate = 48_000.0;
        let maps = Maps {
            tempo: ivory_ui::tempo::TempoMap::constant(120.0),
            meter: ivory_ui::tempo::MeterMap::sanitised(vec![
                ivory_ui::tempo::MeterPoint {
                    at_bar: 0,
                    sig: ivory_ui::recorder::TimeSignature { beats: 4, unit: 4 },
                },
                ivory_ui::tempo::MeterPoint {
                    at_bar: 2,
                    sig: ivory_ui::recorder::TimeSignature { beats: 3, unit: 4 },
                },
            ]),
        };
        let fired = locked_beats(&maps, rate, 0, 48_000 * 12);
        let accents: Vec<u64> = fired.iter().filter(|(_, a)| *a).map(|(at, _)| *at).collect();
        // Two bars of four (96000 frames each), then bars of three (72000).
        assert_eq!(accents[0], 0);
        assert_eq!(accents[1], 96_000);
        assert_eq!(accents[2], 192_000, "the third bar line");
        assert_eq!(accents[3], 192_000 + 72_000, "a bar of three is shorter");
    }

    #[test]
    fn the_metronome_accents_the_first_beat_of_every_bar() {
        let mut b = Beats::new(48_000.0, 120.0);
        b.restart(0);
        b.was_on = true;
        let fired = beats_over(&mut b, 24_000 * 9, true, 3);
        let accents: Vec<usize> = fired
            .iter()
            .enumerate()
            .filter(|(_, (_, x))| x.accent)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(accents, vec![0, 3, 6], "three beats to the bar");
    }

    #[test]
    fn switching_the_metronome_on_restarts_the_phase_so_it_begins_on_a_downbeat() {
        // Otherwise the first click after switching it on lands wherever the
        // free-running count happened to be, and the accent is on the wrong
        // beat for the rest of the session.
        let mut b = Beats::new(48_000.0, 120.0);
        let silent = beats_over(&mut b, 10_000, false, 4);
        assert!(silent.is_empty());
        let fired = beats_over(&mut b, 24_000, true, 4);
        assert_eq!(fired.first().expect("a beat").0, 0);
        assert!(fired[0].1.accent);
    }

    #[test]
    fn an_absurd_tempo_cannot_make_the_beat_clock_fire_forever_inside_one_block() {
        // A period below one frame would never let the countdown run down.
        assert!(period_frames(48_000.0, 1e9, 4) >= 1.0);
        assert!(period_frames(48_000.0, 0.0, 4) >= 1.0);
        assert!(period_frames(48_000.0, f64::NAN, 4) >= 1.0);
        assert!(period_frames(0.0, 120.0, 4) >= 1.0);
        let mut b = Beats::new(48_000.0, f64::INFINITY);
        b.restart(0);
        b.was_on = true;
        assert!(beats_over(&mut b, 1_000, true, 4).len() < 1_000);
    }

    // ── the click ──────────────────────────────────────────────────────────

    #[test]
    fn the_click_asset_decodes_and_is_prepared_at_the_devices_rate() {
        let at_48 = Click::load(48_000.0).expect("assets/click.wav must decode");
        assert!(!at_48.normal.is_empty());
        assert!(
            at_48.accent.len() < at_48.normal.len(),
            "the accent is the same sample played faster, so it must be shorter"
        );
        let at_44 = Click::load(44_100.0).expect("44.1 kHz devices exist");
        let ratio = at_44.normal.len() as f64 / at_48.normal.len() as f64;
        assert!(
            (ratio - 44_100.0 / 48_000.0).abs() < 0.01,
            "resampling to 44.1 kHz produced {ratio:.4} of the samples"
        );
    }

    #[test]
    fn resampling_preserves_the_length_in_seconds_and_never_divides_by_zero() {
        let src: Vec<f32> = (0..1000).map(|i| (i as f32 / 100.0).sin()).collect();
        let up = resample_linear(&src, 48_000.0, 96_000.0);
        assert!((up.len() as f64 / 2000.0 - 1.0).abs() < 0.01);
        let same = resample_linear(&src, 48_000.0, 48_000.0);
        assert_eq!(same, src, "no resampling should be an exact copy");
        assert!(resample_linear(&[], 48_000.0, 48_000.0).is_empty());
        assert!(resample_linear(&src, 0.0, 48_000.0).is_empty());
        assert!(resample_linear(&src, 48_000.0, f64::NAN).is_empty());
    }

    #[test]
    fn a_retriggered_click_starts_again_rather_than_layering() {
        let click = Click {
            normal: vec![1.0, 0.5, 0.25],
            accent: vec![2.0],
        };
        let mut v = Voice::default();
        assert_eq!(v.next(&click), 0.0, "an untriggered voice is silent");
        v.trigger(false);
        assert_eq!(v.next(&click), 1.0);
        assert_eq!(v.next(&click), 0.5);
        v.trigger(false);
        assert_eq!(v.next(&click), 1.0, "the retrigger must start from the top");
        assert_eq!(v.next(&click), 0.5);
        assert_eq!(v.next(&click), 0.25);
        assert_eq!(v.next(&click), 0.0, "and then stop");
        assert_eq!(v.next(&click), 0.0);
        v.trigger(true);
        assert_eq!(v.next(&click), 2.0, "the accent is a different buffer");
    }

    // ── the rings ──────────────────────────────────────────────────────────

    #[test]
    fn the_recorder_tap_can_be_moved_to_the_writer_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<RecorderTap>();
        // And the handoff, which is the whole point of the newtype.
        assert_send::<PluginBox>();
        assert_send::<MidiEvent>();
        // A rack, and the whole renderer, are `Send` because every field of
        // them is — DERIVED, not asserted. `unsafe impl Send` appears exactly
        // once in this file and a second one would mean something is being
        // shared rather than moved. cpal requires this of the closure it takes,
        // so it is checked here rather than discovered at the call site.
        assert_send::<Rack>();
        assert_send::<[Rack; RACK_SEATS]>();
        assert_send::<Renderer>();
    }

    /// A tap and its producer, with no device anywhere.
    fn tap_pair(frames: usize) -> (Producer<f32>, RecorderTap, Arc<Shared>) {
        let shared = Arc::new(Shared::new());
        let (tx, rx) = RingBuffer::<f32>::new(frames * TAP_CHANNELS);
        (
            tx,
            RecorderTap {
                rx,
                channels: TAP_CHANNELS,
                sample_rate: 48_000,
                dropped: Arc::clone(&shared),
            },
            shared,
        )
    }

    #[test]
    fn the_tap_moves_whole_frames_and_reports_how_many() {
        let (mut tx, mut tap, _) = tap_pair(64);
        tx.push_entire_slice(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
            .unwrap();
        let mut out = Vec::new();
        assert_eq!(tap.drain(&mut out), 3);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(tap.drain(&mut out), 0, "draining twice must not repeat");
        assert_eq!(tap.channels(), 2);
        assert_eq!(tap.sample_rate(), 48_000);
    }

    #[test]
    fn a_tap_holding_half_a_frame_yields_nothing_rather_than_swapping_the_channels() {
        // If a partial frame ever left the ring, every sample after it would be
        // on the wrong lane for the rest of the take, and no listening test in
        // mono would catch it.
        let (mut tx, mut tap, _) = tap_pair(64);
        tx.push_entire_slice(&[1.0]).unwrap();
        let mut out = Vec::new();
        assert_eq!(tap.drain(&mut out), 0);
        assert!(out.is_empty());
        tx.push_entire_slice(&[2.0]).unwrap();
        assert_eq!(tap.drain(&mut out), 1);
        assert_eq!(out, vec![1.0, 2.0]);
    }

    #[test]
    fn arming_the_tap_throws_away_the_monitor_audio_that_was_playing_before_the_take() {
        // Measured in the probe before this existed: 80384 frames "lost" during
        // a plugin's own warm-up, which would have made `take.json` call the
        // first take of every session short.
        let shared = Arc::new(Shared::new());
        let (mut tx, rx) = RingBuffer::<f32>::new(8 * TAP_CHANNELS);
        tx.push_entire_slice(&[1.0; 8]).unwrap();
        shared.tap_dropped.store(9_999, Ordering::Relaxed);
        let mut tap = RecorderTap {
            rx,
            channels: TAP_CHANNELS,
            sample_rate: 48_000,
            dropped: Arc::clone(&shared),
        };

        tap.arm();
        assert_eq!(
            tap.dropped(),
            0,
            "a take must not inherit the monitor's losses"
        );
        let mut out = Vec::new();
        assert_eq!(tap.drain(&mut out), 0, "stale audio was left in the ring");

        // And it keeps working afterwards: arming empties the ring, it does not
        // break it.
        tx.push_entire_slice(&[0.5, 0.5]).unwrap();
        assert_eq!(tap.drain(&mut out), 1);
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn a_tap_ring_that_overflows_counts_the_loss_rather_than_dropping_it_silently() {
        // A silently short take is the worst outcome this path can produce, so
        // the number has to reach `take.json`.
        let shared = Arc::new(Shared::new());
        let (tx, rx) = RingBuffer::<f32>::new(8 * TAP_CHANNELS);
        let mut r = test_renderer(Arc::clone(&shared), tx, 2);
        for (i, s) in r.tap_scratch.iter_mut().enumerate() {
            *s = i as f32;
        }
        r.push_tap(8);
        assert_eq!(shared.tap_dropped.load(Ordering::Relaxed), 0);
        r.push_tap(8);
        assert_eq!(
            shared.tap_dropped.load(Ordering::Relaxed),
            8,
            "eight frames had nowhere to go and must be counted"
        );

        let mut tap = RecorderTap {
            rx,
            channels: TAP_CHANNELS,
            sample_rate: 48_000,
            dropped: Arc::clone(&shared),
        };
        let mut out = Vec::new();
        assert_eq!(tap.drain(&mut out), 8, "what did fit is still intact");
        assert_eq!(tap.dropped(), 8);
    }

    // ── the render loop ────────────────────────────────────────────────────

    /// A renderer with no device, no plugin and no click. Everything below
    /// drives this directly, which is why none of it needs hardware.
    impl Renderer {
        /// Replace the track rings with fresh ones and hand back the far
        /// ends, so a test can send clips and collect what comes back —
        /// the same two-way trip the engine makes.
        fn hijack_track_rings(&mut self) -> (Producer<Vec<Placed>>, Consumer<Vec<Placed>>) {
            let (to, incoming) = RingBuffer::<Vec<Placed>>::new(2);
            let (retiring, from) = RingBuffer::<Vec<Placed>>::new(2);
            self.track_incoming = incoming;
            self.track_retiring = retiring;
            (to, from)
        }

        fn hijack_midi_rings(&mut self) -> (Producer<Vec<PlacedMidi>>, Consumer<Vec<PlacedMidi>>) {
            let (to, incoming) = RingBuffer::<Vec<PlacedMidi>>::new(2);
            let (retiring, from) = RingBuffer::<Vec<PlacedMidi>>::new(2);
            self.midi_incoming = incoming;
            self.midi_retiring = retiring;
            (to, from)
        }
    }

    fn test_renderer(shared: Arc<Shared>, tap: Producer<f32>, dev_ch: usize) -> Renderer {
        // The far halves are dropped here on purpose: `rtrb` handles an
        // abandoned peer (`pop` reports empty, `push` fills to capacity), so a
        // renderer under test needs no partner threads at all.
        let (_, midi) = RingBuffer::<MidiEvent>::new(1024);
        let (_, hw_midi) = RingBuffer::<MidiEvent>::new(1024);
        // Empty racks: every test here is about the mixing, and an empty rack
        // is the path every channel takes until somebody loads something.
        let racks: [Rack; RACK_SEATS] = std::array::from_fn(|_| {
            let mut inc = Vec::new();
            let mut ret = Vec::new();
            for _ in 0..INSERTS {
                let (_to, i) = RingBuffer::<PluginBox>::new(1);
                let (r, _from) = RingBuffer::<PluginBox>::new(1);
                inc.push(i);
                ret.push(r);
            }
            let mut inc = inc.into_iter();
            let mut ret = ret.into_iter();
            Rack {
                slots: std::array::from_fn(|_| PluginBox(None)),
                incoming: std::array::from_fn(|_| inc.next().expect("one per insert")),
                retiring: std::array::from_fn(|_| ret.next().expect("one per insert")),
            }
        });
        Renderer {
            monitor: None,
            monitor_channels: 0,
            monitor_gain: [0.0; INPUTS],
            monitor_widths: [0; INPUTS],
            monitor_picks: ivory_record::audio::Picks::EVERYTHING,
            monitor_block: 0,
            monitor_scratch: Vec::new(),
            cap_tx: Vec::new(),
            stem_tx: RingBuffer::<f32>::new(1 << 16).0,
            stem_scratch: vec![0.0; MAX_BLOCK as usize * CHANNELS * 2],
            scope_tx: RingBuffer::<f32>::new(1 << 15).0,
            scope_scratch: vec![0.0; MAX_CALLBACK_FRAMES * TAP_CHANNELS],
            loudness: crate::loudness::Loudness::new(48_000.0),
            lufs_seen: 0,
            vs_phase: crate::varispeed::Phase::new(),
            vs_out: crate::varispeed::Stream::new(dev_ch.max(TAP_CHANNELS)),
            vs_tap: crate::varispeed::Stream::new(TAP_CHANNELS),
            vs_scope: crate::varispeed::Stream::new(TAP_CHANNELS),
            vs_reastream: crate::varispeed::Stream::new(2),
            vs_in_phase: crate::varispeed::Phase::new(),
            vs_in: crate::varispeed::Stream::new(dev_ch.max(TAP_CHANNELS)),
            vs_dev: vec![0.0; MAX_CALLBACK_FRAMES * dev_ch.max(TAP_CHANNELS)],
            vs_dst: vec![0.0; MAX_CALLBACK_FRAMES * dev_ch.max(TAP_CHANNELS)],
            vs_in_src: vec![0.0; MAX_CALLBACK_FRAMES * dev_ch.max(TAP_CHANNELS)],
            vs_was_engaged: false,
            reastream_tx: RingBuffer::<f32>::new(1 << 15).0,
            reastream_scratch: vec![0.0; MAX_CALLBACK_FRAMES * 2],
            route_cache: [(0, 0, false, false, 0.0); MAX_ROUTES],
            route_direct: [1.0; CHANNELS],
            route_n: 0,
            routes_hw: false,
            send_accum: vec![0.0; CHANNELS * MAX_BLOCK as usize * TAP_CHANNELS],
            send_io: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            pad_out: vec![0.0; MAX_BLOCK as usize * PADS * 2],
            pad_io: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            stems_live: false,
            racks,
            fx_in: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            fx_out: vec![vec![0.0; MAX_BLOCK as usize]; TAP_CHANNELS],
            master_effects: crate::effects::Effects::new(RATE as f32),
            room_effects: crate::effects::Effects::new(RATE as f32),
            room: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            input_dry: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            room_live: false,
            room_gain: 0.0,
            aux: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            click_out: vec![0.0; MAX_BLOCK as usize],
            preview_out: vec![0.0; MAX_BLOCK as usize * 2],
            click_taped: vec![0.0; MAX_BLOCK as usize],
            fx_return_gain: 1.0,
            clips: Vec::new(),
            midi_clips: Vec::new(),
            midi_cursor: Vec::new(),
            active_clip: Vec::with_capacity(256),
            lane_notes: Vec::with_capacity(MAX_EVENTS_PER_BLOCK * 2),
            pos: 0,
            lane_gain: [1.0; CHANNELS],
            seen_req: 0,
            was_rolling: false,
            pending_flush: false,
            clip_parked: false,
            sounding: 0,
            // Abandoned peers, like the MIDI ring above: pop reports empty
            // and push fills to capacity, which is all a test needs.
            track_incoming: RingBuffer::<Vec<Placed>>::new(2).1,
            track_retiring: RingBuffer::<Vec<Placed>>::new(2).0,
            midi_incoming: RingBuffer::<Vec<PlacedMidi>>::new(2).1,
            midi_retiring: RingBuffer::<Vec<PlacedMidi>>::new(2).0,
            pad_kit: PadKit::default(),
            pad_incoming: RingBuffer::<PadKit>::new(2).1,
            pad_retiring: RingBuffer::<PadKit>::new(2).0,
            preview_incoming: RingBuffer::<Option<Arc<ivory_record::decode::Clip>>>::new(2).1,
            preview_retiring: RingBuffer::<Option<Arc<ivory_record::decode::Clip>>>::new(2).0,
            preview: None,
            preview_pos: 0,
            pad_voices: [PadVoice::default(); PADS],
            pad_hits: Vec::with_capacity(MAX_PAD_HITS_PER_BLOCK),
            pad_offs: Vec::with_capacity(MAX_PAD_HITS_PER_BLOCK * 2),
            pad_gain: 1.0,
            shared,
            timebase: Timebase::new(),
            rate: RATE,
            dev_channels: dev_ch,
            mix: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            builtin_scratch: vec![0.0; MAX_BLOCK as usize * TAP_CHANNELS],
            channel_gain: [1.0; CHANNELS],
            midi,
            hw_midi,
            pending: None,
            notes: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            builtin: crate::dx7::Dx7::new(RATE as f32),
            effects: crate::effects::Effects::new_send(RATE as f32),
            effect_params: crate::effects::Params::default(),
            controls: Vec::with_capacity(MAX_CONTROLS_PER_BLOCK),
            tap,
            tap_scratch: vec![0.0; MAX_CALLBACK_FRAMES * TAP_CHANNELS],
            frame: vec![0.0; dev_ch.max(TAP_CHANNELS)],
            click: Click {
                normal: vec![1.0; 16],
                accent: vec![1.0; 8],
            },
            voice: Voice::default(),
            beats: Beats::new(RATE, 120.0),
            bar_clock: BarClock::default(),
            maps: Maps::default(),
            maps_incoming: RingBuffer::<Maps>::new(2).1,
            maps_retiring: RingBuffer::<Maps>::new(2).0,
            auto: AutoNow::default(),
            pdc: (0..CHANNELS).map(|_| DelayLine::new()).collect(),
            automation: ivory_ui::automation::Automation::default(),
            auto_incoming: RingBuffer::<ivory_ui::automation::Automation>::new(2).1,
            auto_retiring: RingBuffer::<ivory_ui::automation::Automation>::new(2).0,
            count_in_tail: 0,
            chunks: 0,
            metro_gain: 1.0,
            gain_coeff: 1.0,
        }
    }

    /// A renderer wired to a live MIDI producer, so events can be queued the way
    /// `send_midi` queues them.
    fn renderer_with_midi(dev_ch: usize) -> (Renderer, Producer<MidiEvent>, Arc<Shared>) {
        let shared = Arc::new(Shared::new());
        // **The fresh desk's own shape**: channel 0 is MIDI with the built-in
        // in bay 1, exactly what `Settings::default` produces and the host
        // pushes. The silent auto-fallback died with the slots, so a renderer
        // under test says so the way the app does — by assignment.
        shared.builtin_strip.store(0, Ordering::Relaxed);
        let (tap_tx, _tap_rx) = RingBuffer::<f32>::new(1 << 16);
        let (midi_tx, midi_rx) = RingBuffer::<MidiEvent>::new(1024);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, dev_ch);
        r.midi = midi_rx;
        (r, midi_tx, shared)
    }

    fn queue(tx: &mut Producer<MidiEvent>, stamp: Nanos, bytes: [u8; 3]) {
        tx.push(MidiEvent {
            stamp,
            status: bytes[0],
            data1: bytes[1],
            data2: bytes[2],
        })
        .expect("the test ring is large enough");
    }

    /// **A bay's settings survive: state out, state back in, no complaint.**
    ///
    /// The round trip is the whole feature — `save_insert_state` is what the
    /// quit path writes to disk and the `state` argument is what the reconcile
    /// hands back at launch. Before this existed, every effect's knobs died
    /// with the process, silently, masked only by nobody expecting better.
    ///
    ///     cargo test -p ivory a_bays_settings -- --ignored --nocapture
    #[test]
    #[ignore = "needs an audio device and FabFilter Pro-R 2; runs a vendor's initialiser"]
    fn a_bays_settings_survive_a_save_and_a_reload() {
        let Some(bundle) = ivory_host::discover().into_iter().find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().contains("Pro-R"))
                .unwrap_or(false)
        }) else {
            panic!("no VST3 matching Pro-R; this test needs one installed");
        };
        let mut engine = Engine::start(None).expect("an audio output");
        let at = Strip::Channel(5).index();
        engine
            .load_insert(at, 0, Some(&bundle), None)
            .expect("an effect must load");
        let state = engine
            .save_insert_state(at, 0)
            .expect("a loaded insert must answer for its state");
        assert!(!state.is_empty(), "a state of zero bytes is not a state");

        // The trip back: the same bytes into a fresh instance of the same
        // plugin, which must accept them without complaint.
        engine
            .load_insert(at, 1, Some(&bundle), Some(&state))
            .expect("the state a plugin saved must restore into it");
        assert!(engine.insert(at, 1).is_some());
    }

    /// **An instrument lives in a bay now: no refusal, a warm-up, notes in,
    /// sound out — and the arm mask is a real gate.**
    ///
    /// On the MASTER's rack, because it is the one that runs unconditionally —
    /// the track's runs only while the backing track plays and the inputs'
    /// only while the monitor is open. A voice on the master replaces the whole
    /// mix, which is exactly what "a voice replaces the channel" means, so the
    /// master meter moving is the assertion.
    ///
    ///     cargo test -p ivory an_instrument_in_a_bay -- --ignored --nocapture
    #[test]
    #[ignore = "needs an audio device and the modelled piano installed; runs a vendor's initialiser"]
    fn an_instrument_in_a_bay_renders_notes_and_the_arm_gate_holds() {
        let Some(bundle) = ivory_host::discover().into_iter().find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase().contains("piano"))
                .unwrap_or(false)
        }) else {
            panic!("no VST3 matching the modelled piano; this test needs one installed");
        };
        let mut engine = Engine::start(None).expect("an audio output");
        engine
            .load_insert(STRIPS, 0, Some(&bundle), None)
            .expect("an instrument must load in a bay - the refusal is gone");
        assert!(
            engine.insert(STRIPS, 0).is_some(),
            "the bay does not report its instrument"
        );

        let peak_over = |engine: &Engine, ms: u64| -> f32 {
            let deadline = Instant::now() + Duration::from_millis(ms);
            let mut peak = 0.0f32;
            while Instant::now() < deadline {
                let m = engine.meters();
                peak = peak.max(m.left.peak).max(m.right.peak);
                std::thread::sleep(Duration::from_millis(20));
            }
            peak
        };

        // Armed: a chord, and the master must move.
        let t = engine.timebase().now();
        for n in [48u8, 60, 64, 67] {
            engine.send_midi(t, &[0x90, n, 100]);
        }
        let heard = peak_over(&engine, 1500);
        for n in [48u8, 60, 64, 67] {
            engine.send_midi(engine.timebase().now(), &[0x80, n, 0]);
        }
        assert!(
            heard > 0.001,
            "a bay instrument fed notes made no sound (peak {heard})"
        );

        // Let the tails die, then gate the channel and play again.
        std::thread::sleep(Duration::from_millis(2500));
        engine.set_midi_off(1 << STRIPS.min(31));
        let t = engine.timebase().now();
        for n in [48u8, 60, 64, 67] {
            engine.send_midi(t, &[0x90, n, 100]);
        }
        let gated = peak_over(&engine, 1200);
        for n in [48u8, 60, 64, 67] {
            engine.send_midi(engine.timebase().now(), &[0x80, n, 0]);
        }
        assert!(
            gated < heard * 0.2,
            "the arm gate did not hold: armed peak {heard}, gated peak {gated}"
        );
    }

    /// **The engine retires its own inserts, on this thread, before anything
    /// else drops.**
    ///
    /// `Drop for Engine` unloaded the instrument slots and nothing else, and
    /// then the fields dropped in declaration order — `stream` is first, so the
    /// callback box went, then the `Renderer`, then `racks`, and every insert's
    /// instance was terminated from wherever cpal happens to free its closure.
    /// Only after all of that did `insert_editors` drop and call
    /// `IPlugView::removed()` and `setFrame(null)` on a view whose controller
    /// no longer existed — which is precisely the rule `editors` is documented
    /// with and ordered for, and it was reachable by quitting with an effect's
    /// window open.
    ///
    /// **What this test cannot do, honestly.** The window half is not reachable
    /// from a test harness: libtest runs every test on a spawned thread, even
    /// at `--test-threads=1`, and `Editor::open_handle` refuses with "a plugin
    /// editor can only be opened on the main thread" — correctly, because that
    /// is what VST3 and AppKit require. So this tests the teardown PRIMITIVE
    /// that `Drop` now calls for every bay, and the ordering above it is two
    /// lines of reading. The window path is exercised by quitting the app with
    /// an effect window open.
    ///
    /// Ignored like every test that opens somebody else's binary.
    ///
    ///     cargo test -p ivory an_insert_is_retired -- --ignored --nocapture
    #[test]
    #[ignore = "needs an audio device and FabFilter Pro-R 2; runs a vendor's initialiser"]
    fn an_insert_is_retired_by_the_call_drop_makes() {
        let Some(bundle) = ivory_host::discover().into_iter().find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().contains("Pro-R"))
                .unwrap_or(false)
        }) else {
            panic!("no VST3 matching Pro-R; this test needs one installed");
        };
        let mut engine = Engine::start(None).expect("an audio output");
        let at = Strip::Channel(5).index();
        engine
            .load_insert(at, 0, Some(&bundle), None)
            .expect("an effect must load on an insert");
        assert!(engine.insert(at, 0).is_some(), "nothing was loaded");
        assert!(
            !engine.insert_editor_open(at, 0),
            "a window opened that nobody asked for"
        );

        // The exact call `Drop for Engine` now makes for every bay: close the
        // window, drop the controller reference, retire the instance and drop
        // it HERE rather than in the callback box's teardown.
        engine
            .load_insert(at, 0, None, None)
            .expect("unloading an insert must not fail");
        assert!(engine.insert(at, 0).is_none(), "the bay kept its effect");
        assert!(
            !engine.insert_editor_open(at, 0),
            "a window survived the unload"
        );

        // And dropping a live engine with a loaded insert completes.
        engine
            .load_insert(at, 1, Some(&bundle), None)
            .expect("an effect must load on an insert");
        drop(engine);
    }

    /// **A rack never drops a plugin, and the proof is that it declines to take
    /// the new one.**
    ///
    /// The callback may not run a vendor's teardown: `terminate` frees sample
    /// memory and joins worker threads, and doing that between two blocks of
    /// audio is a dropout at best. So the protocol is that the old instance
    /// goes BACK for the UI thread to drop, and the only way to guarantee that
    /// is to check there is room for it before taking the new one.
    ///
    /// `Rack::swap` did the opposite — pop, replace, `let _ = push(old)` — and
    /// `let _` on an rtrb push is a trap, because `PushError::Full(T)` carries
    /// the value: discarding the `Result` drops the plugin, on the audio
    /// thread, exactly where it must not happen. It is reachable, because
    /// `hand_off_insert` gives up after `RETIRE_TIMEOUT` and leaves the old one
    /// sitting in the ring, so the next load finds `retiring` full.
    ///
    /// Measured by OCCUPANCY, which needs no real plugin: with no room to give
    /// one back, the arrival must still be in its ring afterwards.
    #[test]
    fn a_full_retiring_ring_makes_a_rack_wait_rather_than_drop() {
        let mut to_audio = Vec::new();
        let mut from_audio = Vec::new();
        let mut inc = Vec::new();
        let mut ret = Vec::new();
        for _ in 0..INSERTS {
            let (t, i) = RingBuffer::<PluginBox>::new(1);
            let (r, f) = RingBuffer::<PluginBox>::new(1);
            to_audio.push(t);
            inc.push(i);
            ret.push(r);
            from_audio.push(f);
        }
        let mut inc = inc.into_iter();
        let mut ret = ret.into_iter();
        let mut rack = Rack {
            slots: std::array::from_fn(|_| PluginBox(None)),
            incoming: std::array::from_fn(|_| inc.next().expect("one per insert")),
            retiring: std::array::from_fn(|_| ret.next().expect("one per insert")),
        };

        // Bay 0's retiring ring is full — the UI thread timed out and never
        // collected what it was given.
        rack.retiring[0]
            .push(PluginBox(None))
            .expect("room for one");
        // And a fresh plugin has been sent to that bay.
        to_audio[0].push(PluginBox(None)).expect("room for one");
        assert_eq!(
            rack.incoming[0].slots(),
            1,
            "the arrival never reached the ring"
        );

        rack.swap();

        assert_eq!(
            rack.incoming[0].slots(),
            1,
            "the rack took a plugin it had nowhere to put the old one - which is \
             a `terminate` on the audio thread"
        );

        // And once the UI thread collects, the very next swap goes through.
        from_audio[0]
            .pop()
            .expect("the retired one was there to collect");
        rack.swap();
        assert_eq!(
            rack.incoming[0].slots(),
            0,
            "the rack never took the arrival"
        );
        assert_eq!(
            from_audio[0].slots(),
            1,
            "the displaced one was not handed back"
        );
    }

    /// The other bays are untouched by a bay that is waiting.
    #[test]
    fn one_stuck_bay_does_not_stall_the_rest_of_the_rack() {
        let mut to_audio = Vec::new();
        let mut from_audio = Vec::new();
        let mut inc = Vec::new();
        let mut ret = Vec::new();
        for _ in 0..INSERTS {
            let (t, i) = RingBuffer::<PluginBox>::new(1);
            let (r, f) = RingBuffer::<PluginBox>::new(1);
            to_audio.push(t);
            inc.push(i);
            ret.push(r);
            from_audio.push(f);
        }
        let mut inc = inc.into_iter();
        let mut ret = ret.into_iter();
        let mut rack = Rack {
            slots: std::array::from_fn(|_| PluginBox(None)),
            incoming: std::array::from_fn(|_| inc.next().expect("one per insert")),
            retiring: std::array::from_fn(|_| ret.next().expect("one per insert")),
        };
        rack.retiring[0]
            .push(PluginBox(None))
            .expect("room for one");
        for t in &mut to_audio {
            t.push(PluginBox(None)).expect("room for one");
        }

        rack.swap();

        assert_eq!(rack.incoming[0].slots(), 1, "bay 0 should still be waiting");
        for i in 1..INSERTS {
            assert_eq!(
                rack.incoming[i].slots(),
                0,
                "bay {i} was held up by a bay it has nothing to do with"
            );
        }
    }

    /// **Mute, solo, and the one interaction between them.**
    ///
    /// Solo is exclusive and mute loses to it: pressing solo on a channel that
    /// is muted is unambiguously a request to hear it, and the alternative is a
    /// solo button that sometimes does nothing.
    #[test]
    fn solo_beats_mute_and_silences_everything_else() {
        use Strip::*;
        let all: Vec<Strip> = ivory_ui::recorder::Strip::all()
            .into_iter()
            .map(Strip::from)
            .collect();
        let none = 0;
        // Nothing muted, nothing soloed: everybody is heard.
        for s in &all {
            assert!(
                strip_is_heard(*s, none, none),
                "{s:?} was silent for no reason"
            );
        }
        // One muted: it alone is silent.
        let m = Channel(0).bit();
        assert!(!strip_is_heard(Channel(0), m, none));
        assert!(strip_is_heard(Channel(7), m, none));
        // One soloed: it alone is heard, and mute does not enter into it.
        let so = Channel(7).bit();
        assert!(strip_is_heard(Channel(7), none, so));
        assert!(!strip_is_heard(Channel(0), none, so));
        assert!(!strip_is_heard(Channel(4), none, so));
        // Soloed AND muted: heard, because pressing solo asked for it.
        assert!(
            strip_is_heard(Channel(7), Channel(7).bit(), Channel(7).bit()),
            "a soloed channel was silenced by its own mute"
        );
        // Two soloed: both, and nothing else.
        let two = Channel(7).bit() | Fx.bit();
        assert!(strip_is_heard(Channel(7), none, two) && strip_is_heard(Fx, none, two));
        assert!(!strip_is_heard(Click, none, two));
        // **Each channel is its own strip.** Muting one must not take its
        // neighbour with it, which is the whole reason the lumped instrument
        // strip had to go.
        let one = Channel(2).bit();
        assert!(!strip_is_heard(Channel(2), one, none));
        assert!(
            strip_is_heard(Channel(3), one, none),
            "muting one channel muted another"
        );
        // Every strip owns a bit of its own, or two of them would mute together.
        let mut bits: Vec<u32> = all.iter().map(|s| s.bit()).collect();
        let n = bits.len();
        bits.sort_unstable();
        bits.dedup();
        assert_eq!(bits.len(), n, "two strips share a bit");
    }

    /// The bus comes back at its own fader, and adds rather than replaces.
    #[test]
    fn the_effects_bus_returns_at_its_own_level() {
        let frames = 3;
        let mut mix = vec![1.0_f32; frames * TAP_CHANNELS];
        let aux = vec![0.5_f32; frames * TAP_CHANNELS];
        let mut gain = 0.0;
        let _ = add_return(&mut mix, &aux, frames, &mut gain, 1.0, 1.0);
        for (i, v) in mix.iter().enumerate() {
            assert!(
                (v - 1.5).abs() < 1.0e-6,
                "frame {i} came back as {v} rather than dry plus wet"
            );
        }
        // At zero return the bus is inaudible and the dry is untouched.
        let mut mix = vec![1.0_f32; frames * TAP_CHANNELS];
        let mut gain = 0.0;
        let _ = add_return(&mut mix, &aux, frames, &mut gain, 0.0, 1.0);
        assert!(mix.iter().all(|v| (v - 1.0).abs() < 1.0e-6));
    }

    /// **Muting the instrument silences the app.**
    ///
    /// End to end through the real renderer, because the gating happens in
    /// three places — the fader, the send and the strip — and a unit test of
    /// any one of them would pass with the wiring wrong.
    #[test]
    fn a_muted_instrument_reaches_neither_the_speakers_nor_the_bus() {
        let (mut r, mut tx, shared) = renderer_with_midi(2);
        // Every general channel muted: the built-in is on one of them and
        // this is a test about the strip, not about which channel it landed
        // on. Assigned explicitly — the silent auto-fallback died with the
        // slots, so a renderer under test says where the DX7 lives.
        shared.builtin_strip.store(0, Ordering::Relaxed);
        let mut mask = 0;
        for i in 0..CHANNELS {
            mask |= Strip::Channel(i).bit();
        }
        shared.muted.store(mask, Ordering::Relaxed);
        // Everything the bus could carry it with, turned up.
        shared.reverb_mix.store(1.0f32.to_bits(), Ordering::Relaxed);
        for i in 0..SLOTS {
            shared.send[i].store(1.0f32.to_bits(), Ordering::Relaxed);
        }

        queue(&mut tx, 0, [0x90, 60, 100]);
        let mut out = vec![0.0_f32; 2 * 2048];
        for _ in 0..4 {
            r.render(&mut out, 0, 0);
            let peak = out.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            assert!(
                peak < 1.0e-4,
                "a muted instrument produced {peak} at the device"
            );
        }
    }

    /// **An audition stays on the main pair and obeys the faders.**
    ///
    /// The tester's Babyface lit all twelve outputs when a media-explorer
    /// clip was clicked: the out-pass added the preview (and the click) to
    /// EVERY device channel, while `map_frame` gives the mix only the first
    /// two — and the preview also joined after the master had already
    /// multiplied the mix, so it played the raw file at unity into a desk
    /// running below it. Channels past the pair must stay silent, and the
    /// pair must carry the file scaled by the master and the drum bus fader.
    #[test]
    fn an_audition_keeps_to_the_main_pair_and_obeys_the_faders() {
        let (mut r, _tx, shared) = renderer_with_midi(8);
        shared
            .master_gain
            .store(0.5f32.to_bits(), Ordering::Relaxed);
        // No drum lane assigned: the hidden Drums internals rule and rest at
        // unity, so the expected level is file * PREVIEW_GAIN(0.8) * 0.5.
        r.preview = Some(std::sync::Arc::new(ivory_record::decode::Clip {
            samples: vec![0.25_f32; 2 * 4096],
            rate: RATE as u32,
            source: std::path::PathBuf::from("audition.wav"),
        }));
        r.preview_pos = 0;

        let frames = 512;
        let mut out = vec![0.0_f32; 8 * frames];
        r.render(&mut out, 0, 0);

        let expected = 0.25 * 0.8 * 0.5;
        for f in 0..frames {
            for c in 0..8 {
                let s = out[f * 8 + c];
                if c < 2 {
                    assert!(
                        (s - expected).abs() < 1.0e-4,
                        "channel {c} frame {f}: {s} instead of {expected}"
                    );
                } else {
                    assert!(
                        s.abs() < 1.0e-6,
                        "the audition leaked onto device channel {c}: {s}"
                    );
                }
            }
        }
    }

    /// **A fresh install makes a sound.**
    ///
    /// The whole point of the built-in: no plugin loaded, a note arrives, and
    /// audio comes out of the device. Asserted through the real renderer and
    /// not the synth's own unit tests, because the wiring is what was missing
    /// for every release before this one, not the DSP.
    #[test]
    fn a_note_sounds_with_no_plugin_loaded_at_all() {
        let (mut r, mut tx, _) = renderer_with_midi(2);
        let mut out = vec![0.0_f32; 2 * 2048];

        // Silence before anything is played, which is what makes the assertion
        // below mean something.
        r.render(&mut out, 0, 0);
        assert!(
            out.iter().all(|s| *s == 0.0),
            "the device was not silent before a note"
        );

        queue(&mut tx, 0, [0x90, 60, 100]);
        let mut heard = vec![0.0_f32; 2 * 2048];
        r.render(&mut heard, 0, 0);
        let peak = heard.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.01,
            "a note with no plugin loaded produced {peak}, which is silence"
        );
        assert!(peak <= 1.0, "the built-in clipped the device at {peak}");
    }

    /// **The hardware fast lane sounds without the UI.**
    ///
    /// The midir callback pushes into `hw_midi` through `midi::MidiFastLane`;
    /// no UI frame, no `send_midi`. The renderer must treat that ring exactly
    /// like the app's — a note-on in it comes out of the device on the very
    /// next render, past-stamped to the block's first frame.
    #[test]
    fn a_hardware_note_sounds_without_the_ui_in_the_loop() {
        let (mut r, _tx, _) = renderer_with_midi(2);
        let (mut hw_tx, hw_rx) = RingBuffer::<MidiEvent>::new(64);
        r.hw_midi = hw_rx;

        let mut out = vec![0.0_f32; 2 * 2048];
        r.render(&mut out, 0, 0);
        assert!(out.iter().all(|s| *s == 0.0), "not silent before the note");

        hw_tx
            .push(MidiEvent {
                stamp: 0,
                status: 0x90,
                data1: 60,
                data2: 100,
            })
            .expect("the test ring holds one note");
        let mut heard = vec![0.0_f32; 2 * 2048];
        r.render(&mut heard, 0, 0);
        let peak = heard.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.01,
            "a hardware-lane note produced {peak}, which is silence"
        );
    }

    /// A kit whose pads hold one constant-value stereo sample, for asserting
    /// exact levels through the real render path.
    fn test_kit(pads: &[usize], frames: usize, value: f32) -> PadKit {
        let mut kit = PadKit::default();
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![value; frames * 2],
            rate: RATE as u32,
            source: std::path::PathBuf::new(),
        });
        for &p in pads {
            kit.pads[p] = Some(Arc::clone(&clip));
        }
        kit
    }

    /// **A channel-10 note strikes the pad and spares the piano.**
    ///
    /// End to end through the real renderer: the diversion happens in
    /// `collect_notes` and the sound in `mix_pads`, and a unit test of
    /// either would pass with the other unwired. The level is exact —
    /// constant sample × velocity — because nothing else is running.
    #[test]
    fn a_channel_ten_note_strikes_the_pad_and_spares_the_piano() {
        let (mut r, mut tx, _shared) = renderer_with_midi(2);
        r.pad_kit = test_kit(&[36], 48_000, 0.5);

        queue(&mut tx, 0, [0x99, 36, 100]);
        let mut out = vec![0.0_f32; 2 * 2048];
        r.render(&mut out, 0, 0);
        let peak = out.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        let want = 0.5 * 100.0 / 127.0;
        assert!(
            (peak - want).abs() < 0.01,
            "the pad struck at {peak}, wanted {want}"
        );
        assert!(
            !r.builtin.active(),
            "a drum-channel note reached the piano through a loaded kit"
        );
    }

    /// **With no kit, channel 10 reaches the lanes exactly as it always
    /// did** — an empty drumpad must not eat somebody's channel-10 rig.
    #[test]
    fn an_empty_kit_lets_channel_ten_reach_the_lanes() {
        let (mut r, mut tx, _shared) = renderer_with_midi(2);
        queue(&mut tx, 0, [0x99, 60, 100]);
        let mut out = vec![0.0_f32; 2 * 2048];
        r.render(&mut out, 0, 0);
        let peak = out.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.01,
            "a channel-10 note with no kit loaded produced {peak}"
        );
        assert!(r.builtin.active(), "the built-in never saw the note");
    }

    /// **A shared choke group: the second pad ends the first.** The steal
    /// fade is ~5 ms, so past it the buffer carries ONE pad's level, not
    /// two — which is the hat pattern the groups exist for. And the same
    /// arithmetic pins the self-choke: a retriggered pad never doubles.
    #[test]
    fn a_choke_group_cuts_the_other_pad_and_a_retrigger_never_doubles() {
        let (mut r, mut tx, shared) = renderer_with_midi(2);
        r.pad_kit = test_kit(&[36, 37], 480_000, 0.5);
        shared.pad_choke[36].store(1, Ordering::Relaxed);
        shared.pad_choke[37].store(1, Ordering::Relaxed);
        let one = 0.5 * 100.0 / 127.0;

        let mut out = vec![0.0_f32; 2 * 2048];
        queue(&mut tx, 0, [0x99, 36, 100]);
        r.render(&mut out, 0, 0);

        queue(&mut tx, 0, [0x99, 37, 100]);
        r.render(&mut out, 0, 0);
        // The tail of the buffer is well past the steal fade: pad 37 alone.
        let tail = &out[out.len() / 2..];
        let peak = tail.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            (peak - one).abs() < 0.01,
            "after the choke the tail held {peak}, wanted one pad's {one}"
        );

        // And the self-choke: strike 37 again — still one voice's worth.
        queue(&mut tx, 0, [0x99, 37, 100]);
        r.render(&mut out, 0, 0);
        let tail = &out[out.len() / 2..];
        let peak = tail.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            (peak - one).abs() < 0.01,
            "a retrigger doubled the pad to {peak}"
        );
    }

    /// **A kit swap silences the voices and returns the old kit** — the
    /// clips' protocol, held to on the pads: the displaced samples come
    /// back whole to be dropped off the audio thread, and no voice plays
    /// on into a waveform that just left.
    #[test]
    fn a_kit_swap_silences_the_voices_and_returns_the_old_kit() {
        let (mut r, mut tx, _shared) = renderer_with_midi(2);
        r.pad_kit = test_kit(&[36], 480_000, 0.5);
        let mut out = vec![0.0_f32; 2 * 2048];
        queue(&mut tx, 0, [0x99, 36, 100]);
        r.render(&mut out, 0, 0);
        assert!(out.iter().any(|s| s.abs() > 0.01), "the pad never sounded");

        let (mut to, incoming) = RingBuffer::<PadKit>::new(2);
        let (retiring, mut from) = RingBuffer::<PadKit>::new(2);
        r.pad_incoming = incoming;
        r.pad_retiring = retiring;
        to.push(test_kit(&[1], 4800, 0.25))
            .expect("room for the kit");

        r.render(&mut out, 0, 0);
        let peak = out.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            peak < 1.0e-4,
            "a voice survived the swap into a different kit at {peak}"
        );
        let old = from.pop().expect("the displaced kit came back");
        assert!(old.pads[36].is_some(), "the wrong kit was returned");
    }

    /// **A hardware route puts a channel on its own jacks, and does not take
    /// it off the master.**
    ///
    /// The owner's own example: a track sent to outs 3/4. The master pair
    /// keeps the whole mix — a route is a send, not a move — and the routed
    /// jacks carry that one channel at its route fader.
    #[test]
    fn a_hardware_route_reaches_its_jacks_without_leaving_the_master() {
        use std::sync::Arc;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![0.5; 4000],
            rate: 48_000,
            source: std::path::PathBuf::from("c.wav"),
        });
        // Four device outputs, so 3/4 exists to route to.
        let (mut r, _tx, shared) = renderer_with_midi(4);
        let (mut to_audio, _from) = r.hijack_track_rings();
        to_audio
            .push(vec![Placed {
                clip,
                strip: 1,
                start: 0,
                offset: 0,
                len: 1000,
                fade_in: 0,
                fade_out: 0,
                gain: 1.0,
            }])
            .expect("room");
        // Channel 1 -> outs 3/4 (zero-based first = 2), at half.
        shared.routes[0].store(pack_route(1, 2, true, true), Ordering::Relaxed);
        shared.route_gain[0].store(0.5f32.to_bits(), Ordering::Relaxed);
        shared.route_count.store(1, Ordering::Release);
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);

        let frames = 256;
        let mut out = vec![0.0f32; frames * 4];
        r.render(&mut out, 0, 0);

        let main = out[4 * 100];
        let routed = out[4 * 100 + 2];
        assert!(main.abs() > 1.0e-4, "the master pair went silent: {main}");
        assert!(routed.abs() > 1.0e-4, "nothing reached outs 3/4: {routed}");
        // The route carries the channel at its own fader, and the master is
        // untouched by the routing — so the routed jack is half the main.
        assert!(
            (routed - main * 0.5).abs() < 1.0e-3,
            "3/4 carried {routed} against a master of {main}"
        );
    }

    // ── varispeed ───────────────────────────────────────────────────────────

    /// Play a sine clip through the whole renderer at `ratio` and hand back
    /// what left the main pair.
    fn varispeed_out(ratio: f32, hz: f64, seconds: f64) -> (Vec<f32>, u64) {
        use std::sync::Arc;
        let rate = 48_000.0_f64;
        // Long enough that the clip never runs out even at half speed.
        let n = (rate * seconds * 3.0) as usize;
        let clip = Arc::new(ivory_record::decode::Clip {
            // **Interleaved stereo** — `Clip::samples` is, and generating one
            // sample per index rather than one FRAME per index makes a tone
            // at twice the frequency asked for, which is exactly the kind of
            // wrong a varispeed test cannot afford to be built on.
            samples: (0..n)
                .flat_map(|f| {
                    let v = (std::f64::consts::TAU * hz * f as f64 / rate).sin() as f32;
                    [v, v]
                })
                .collect(),
            rate: 48_000,
            source: std::path::PathBuf::from("s.wav"),
        });
        let (mut r, _tx, shared) = renderer_with_midi(2);
        let (to, _from) = r.hijack_track_rings();
        let mut to = to;
        to.push(vec![Placed {
            clip,
            strip: 1,
            start: 0,
            offset: 0,
            len: n as u64,
            fade_in: 0,
            fade_out: 0,
            gain: 1.0,
        }])
        .expect("room");
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);
        shared
            .varispeed
            .store(ratio.to_bits(), Ordering::Relaxed);

        let block = 256;
        let blocks = ((rate * seconds) as usize) / block;
        let mut out = Vec::with_capacity(blocks * block * 2);
        let mut buf = vec![0.0f32; block * 2];
        for _ in 0..blocks {
            buf.fill(0.0);
            r.render(&mut buf, 0, 0);
            out.extend_from_slice(&buf);
        }
        (out, shared.transport_pos.load(Ordering::Relaxed))
    }

    /// **The pitch follows the speed, through the whole desk.**
    ///
    /// `varispeed::tests` proves the resampler; this proves it is WIRED —
    /// that a clip on a channel, mixed, master-faded and handed to the
    /// device, comes out at the frequency the dial says. A resampler that
    /// worked perfectly and was never reached would pass every test in that
    /// module.
    #[test]
    fn the_desk_plays_faster_and_sharper_when_the_dial_is_off_centre() {
        let rate = 48_000.0_f64;
        let count = |out: &[f32]| {
            // The settled part, on the left channel.
            let l: Vec<f32> = out.chunks(2).skip(4_000).map(|c| c[0]).collect();
            let crossings = l.windows(2).filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0)).count();
            crossings as f64 / 2.0 / (l.len() as f64 / rate)
        };
        let (rest, _) = varispeed_out(1.0, 500.0, 0.5);
        let at_rest = count(&rest);
        assert!(
            (at_rest - 500.0).abs() < 12.0,
            "the clip did not play at 500 Hz at rest: {at_rest:.1}"
        );
        for ratio in [0.85_f32, 1.15, 2.0] {
            let (out, _) = varispeed_out(ratio, 500.0, 0.5);
            let got = count(&out);
            let want = 500.0 * f64::from(ratio);
            assert!(
                (got - want).abs() / want < 0.03,
                "at {ratio}x the desk played {got:.1} Hz where {want:.1} was wanted"
            );
        }
    }

    /// **The timeline moves at the same speed the audio does.**
    ///
    /// The clock and the sound have to agree or the playhead walks away from
    /// the music — which is the failure that would make varispeed useless for
    /// the one thing anybody records with it, which is playing along.
    #[test]
    fn the_playhead_moves_with_the_audio() {
        let (_, at_rest) = varispeed_out(1.0, 500.0, 0.25);
        for ratio in [0.85_f32, 1.15, 2.0] {
            let (_, moved) = varispeed_out(ratio, 500.0, 0.25);
            let want = at_rest as f64 * f64::from(ratio);
            let got = moved as f64;
            assert!(
                (got - want).abs() / want < 0.02,
                "at {ratio}x the playhead reached {got} where {want:.0} was wanted"
            );
        }
    }

    /// **Dead centre is the un-engaged path, exactly.**
    ///
    /// The detent guarantees 1.0 is reachable, and this is what makes
    /// reaching it free: at rest the resampler is not in circuit at all, so
    /// there is no interpolation error to hear and nothing to switch out.
    #[test]
    fn at_rest_nothing_is_resampled() {
        let (a, _) = varispeed_out(1.0, 500.0, 0.2);
        let (b, _) = varispeed_out(1.0, 500.0, 0.2);
        assert_eq!(a, b, "the un-engaged path is not deterministic");
        // A ratio a hair off unity IS engaged — the detent lives in the UI,
        // and the engine's job is to do what it is told.
        let (c, _) = varispeed_out(1.02, 500.0, 0.2);
        assert_ne!(a, c, "1.02 was treated as off");
    }

    /// A device callback bigger than the varispeed path can serve in one pass
    /// is split, not truncated.
    #[test]
    fn a_long_callback_is_split_rather_than_cut_short() {
        let (mut r, _tx, shared) = renderer_with_midi(2);
        assert_eq!(r.max_callback_frames(), MAX_CALLBACK_FRAMES, "at rest");
        shared.varispeed.store(2.0f32.to_bits(), Ordering::Relaxed);
        let cap = r.max_callback_frames();
        assert!(cap < MAX_CALLBACK_FRAMES, "the cap did not move");
        // **The bound that matters**: at the dial's fastest, one device frame
        // costs two program frames, and the program-rate scratches are sized
        // for `MAX_CALLBACK_FRAMES`. Whatever the cap is, twice it plus the
        // interpolator's reach has to fit.
        let ph = crate::varispeed::Phase::new();
        assert!(
            ph.program_frames(cap, crate::varispeed::MAX_RATIO) <= MAX_CALLBACK_FRAMES,
            "a full callback at 2x asks for more program than the mix holds"
        );
        // And a callback at the cap renders without truncating.
        let mut out = vec![0.0f32; cap * 2];
        r.render(&mut out, 0, 0);
    }

    /// **A mono jack SUMS the pair rather than taking the left.**
    ///
    /// A channel panned hard right would otherwise vanish into a mono
    /// output, which is the kind of silence nobody can debug.
    #[test]
    fn a_mono_route_sums_rather_than_dropping_half_the_channel() {
        use std::sync::Arc;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![0.5; 4000],
            rate: 48_000,
            source: std::path::PathBuf::from("c.wav"),
        });
        let (mut r, _tx, shared) = renderer_with_midi(4);
        let (mut to_audio, _from) = r.hijack_track_rings();
        to_audio
            .push(vec![Placed {
                clip,
                strip: 1,
                start: 0,
                offset: 0,
                len: 1000,
                fade_in: 0,
                fade_out: 0,
                gain: 1.0,
            }])
            .expect("room");
        // Hard right, and a MONO route to out 4 (index 3).
        shared.channel_pans[1].store(1.0f32.to_bits(), Ordering::Relaxed);
        shared.routes[0].store(pack_route(1, 3, true, false), Ordering::Relaxed);
        shared.route_gain[0].store(1.0f32.to_bits(), Ordering::Relaxed);
        shared.route_count.store(1, Ordering::Release);
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);

        let frames = 256;
        let mut out = vec![0.0f32; frames * 4];
        r.render(&mut out, 0, 0);
        let mono = out[4 * 100 + 3];
        assert!(
            mono.abs() > 1.0e-4,
            "a hard-panned channel vanished into its mono jack: {mono}"
        );
    }

    /// **A send reaches the destination channel and rides its fader.**
    ///
    /// The bus behaviour: channel 1 sent to channel 5 is heard through
    /// channel 5, so pulling channel 5 down pulls the send down with it. That
    /// is what makes a channel usable AS a bus.
    #[test]
    fn a_send_arrives_at_the_destination_and_its_fader_rides_it() {
        use std::sync::Arc;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![0.5; 4000],
            rate: 48_000,
            source: std::path::PathBuf::from("c.wav"),
        });
        let render = |dest_gain: f32| {
            let clip = Arc::clone(&clip);
            let (mut r, _tx, shared) = renderer_with_midi(2);
            let (mut to_audio, _from) = r.hijack_track_rings();
            to_audio
                .push(vec![Placed {
                    clip,
                    strip: 1,
                    start: 0,
                    offset: 0,
                    len: 1000,
                    fade_in: 0,
                    fade_out: 0,
                    gain: 1.0,
                }])
                .expect("room");
            // Channel 1 -> channel 5, full.
            shared.routes[0].store(pack_route(1, 5, false, false), Ordering::Relaxed);
            shared.route_gain[0].store(1.0f32.to_bits(), Ordering::Relaxed);
            shared.route_count.store(1, Ordering::Release);
            shared.channel_gains[5].store(dest_gain.to_bits(), Ordering::Relaxed);
            shared.transport_at.store(0, Ordering::Relaxed);
            shared.transport_req.store(1, Ordering::Release);
            shared.rolling.store(true, Ordering::Relaxed);
            let mut out = vec![0.0f32; 256 * 2];
            r.render(&mut out, 0, 0);
            out[2 * 200]
        };
        let open = render(1.0);
        let shut = render(0.0);
        assert!(open.abs() > 1.0e-4, "the send never arrived: {open}");
        assert!(
            open.abs() > shut.abs() + 1.0e-4,
            "the destination's fader did not ride the send ({open} vs {shut})"
        );
    }

    /// **A routed channel is HEARD ONCE.**
    ///
    /// It used to be heard twice: the lane went straight to the master the
    /// way it always had, and the send put the very same audio in again
    /// through the destination, at the same level. The owner's words were
    /// "every send created is a duplicate of that channel heard — 2 instances
    /// of same channel", and they were describing a doubling of exactly 6 dB.
    ///
    /// Routing a channel to a channel MOVES it. A copy is what the SND knob
    /// does into the effects bus, and that is still there for reverb.
    #[test]
    fn routing_a_channel_moves_it_rather_than_playing_it_twice() {
        use std::sync::Arc;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![0.5; 4000],
            rate: 48_000,
            source: std::path::PathBuf::from("c.wav"),
        });
        let render = |route: bool| {
            let clip = Arc::clone(&clip);
            let (mut r, _tx, shared) = renderer_with_midi(2);
            let (mut to_audio, _from) = r.hijack_track_rings();
            to_audio
                .push(vec![Placed {
                    clip,
                    strip: 1,
                    start: 0,
                    offset: 0,
                    len: 1000,
                    fade_in: 0,
                    fade_out: 0,
                    gain: 1.0,
                }])
                .expect("room");
            if route {
                shared.routes[0].store(pack_route(1, 5, false, false), Ordering::Relaxed);
                shared.route_gain[0].store(1.0f32.to_bits(), Ordering::Relaxed);
                shared.route_count.store(1, Ordering::Release);
            }
            shared.transport_at.store(0, Ordering::Relaxed);
            shared.transport_req.store(1, Ordering::Release);
            shared.rolling.store(true, Ordering::Relaxed);
            let mut out = vec![0.0f32; 256 * 2];
            r.render(&mut out, 0, 0);
            out[2 * 200]
        };
        let plain = render(false);
        let routed = render(true);
        assert!(plain.abs() > 1.0e-4, "nothing played unrouted: {plain}");
        assert!(
            routed.abs() > 1.0e-4,
            "the route swallowed the channel: {routed}"
        );
        // The destination is empty and at unity, so arriving through it must
        // sound like arriving directly — not like arriving twice.
        assert!(
            (routed.abs() - plain.abs()).abs() < 1.0e-3,
            "routing changed the level: {plain} unrouted vs {routed} routed"
        );
        assert!(
            routed.abs() < plain.abs() * 1.5,
            "the channel is still being heard twice: {routed} against {plain}"
        );
    }

    /// **A LIVE INPUT routes with its lane, like everything else on it.**
    ///
    /// The routed-away gate went on the clip, pad and instrument folds and
    /// missed the monitor, so a channel with an input open reached the master
    /// twice: once straight out of `mix_monitor`, and once more gathered out
    /// of the stem and folded in through its destination. The owner heard it
    /// immediately — "100 percent a volume jump on a send added" — because a
    /// live input is exactly what somebody testing a send has open.
    #[test]
    fn a_live_input_does_not_double_when_its_lane_is_routed() {
        const BLOCK: usize = 128;
        let feed = |route: bool| {
            let shared = Arc::new(Shared::new());
            let (tap_tx, _tap_rx) = RingBuffer::<f32>::new(1 << 16);
            let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
            let (mut in_tx, in_rx) = RingBuffer::<f32>::new(1 << 15);
            r.monitor = Some(in_rx);
            r.monitor_channels = 1;
            r.monitor_widths = [1, 0, 0, 0];
            r.monitor_block = BLOCK;
            r.monitor_scratch = vec![0.0; 8192];
            shared.pick_strip[0].store(1, Ordering::Relaxed);
            shared.midi_off.store(1 << 1, Ordering::Relaxed);
            shared.channel_gains[1].store(1.0f32.to_bits(), Ordering::Relaxed);
            r.channel_gain[1] = 1.0;
            shared.monitor_on.store(true, Ordering::Relaxed);
            r.room_gain = 1.0;
            if route {
                // Channel 1 -> channel 5, at unity, through an empty rack.
                shared.routes[0].store(pack_route(1, 5, false, false), Ordering::Relaxed);
                shared.route_gain[0].store(1.0f32.to_bits(), Ordering::Relaxed);
                shared.route_count.store(1, Ordering::Release);
                shared.channel_gains[5].store(1.0f32.to_bits(), Ordering::Relaxed);
                r.lane_gain[5] = 1.0;
            }
            let mut out = vec![0.0f32; BLOCK * 2];
            let mut heard = 0.0f32;
            for _ in 0..4 {
                for _ in 0..BLOCK {
                    let _ = in_tx.push(0.5);
                }
                out.fill(0.0);
                r.render(&mut out, 0, 0);
                heard = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
            }
            heard
        };
        let plain = feed(false);
        let routed = feed(true);
        assert!(
            plain > 1.0e-4,
            "the input was never heard unrouted: {plain}"
        );
        assert!(routed > 1.0e-4, "the route swallowed the input: {routed}");
        assert!(
            routed < plain * 1.5,
            "the live input is being heard twice: {routed} routed against {plain}"
        );
    }


    /// **A CHAIN carries through in one block: A to B to C.**
    ///
    /// It used to do nothing at all. Destinations ran in index order with
    /// every gather done up front, so the second hop read a stem the first
    /// had not written yet — the level was identical with and without the C
    /// hop, and C's fader and rack never touched the audio. Every desk worth
    /// comparing to treats a multi-hop chain as ordinary; this is the test
    /// that says Tangent does.
    #[test]
    fn a_chain_of_routes_carries_through_to_the_end_of_it() {
        use std::sync::Arc;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![0.5; 4000],
            rate: 48_000,
            source: std::path::PathBuf::from("c.wav"),
        });
        // **Channel 1 -> 7 -> 5: the chain runs DOWN the desk.**
        //
        // Deliberately descending. An ascending chain is carried by plain
        // index order whether anything was sorted or not, so a 1 -> 5 -> 7
        // test passes against the broken build and proves nothing — which is
        // exactly what the first version of this test did.
        let render = |chain: bool, end_gain: f32| {
            let clip = Arc::clone(&clip);
            let (mut r, _tx, shared) = renderer_with_midi(2);
            let (mut to_audio, _from) = r.hijack_track_rings();
            to_audio
                .push(vec![Placed {
                    clip,
                    strip: 1,
                    start: 0,
                    offset: 0,
                    len: 1000,
                    fade_in: 0,
                    fade_out: 0,
                    gain: 1.0,
                }])
                .expect("room");
            let first = if chain { 7 } else { 5 };
            shared.routes[0].store(pack_route(1, first, false, false), Ordering::Relaxed);
            shared.route_gain[0].store(1.0f32.to_bits(), Ordering::Relaxed);
            let n = if chain {
                shared.routes[1].store(pack_route(7, 5, false, false), Ordering::Relaxed);
                shared.route_gain[1].store(1.0f32.to_bits(), Ordering::Relaxed);
                2
            } else {
                1
            };
            shared.route_count.store(n, Ordering::Release);
            // 7 is the middle of the chain and stays open; 5 is the END, and
            // its fader is what the last assertion closes.
            shared.channel_gains[7].store(1.0f32.to_bits(), Ordering::Relaxed);
            shared.channel_gains[5].store(end_gain.to_bits(), Ordering::Relaxed);
            r.lane_gain[7] = 1.0;
            r.lane_gain[5] = end_gain;
            shared.transport_at.store(0, Ordering::Relaxed);
            shared.transport_req.store(1, Ordering::Release);
            shared.rolling.store(true, Ordering::Relaxed);
            let mut out = vec![0.0f32; 256 * 2];
            r.render(&mut out, 0, 0);
            out[2 * 200]
        };
        let one_hop = render(false, 1.0);
        let chained = render(true, 1.0);
        let shut = render(true, 0.0);
        assert!(one_hop.abs() > 1.0e-4, "nothing played at all: {one_hop}");
        // Through two unity buses it still arrives once, at the same level.
        assert!(
            (chained.abs() - one_hop.abs()).abs() < 1.0e-3,
            "the chain changed the level: {one_hop} one hop vs {chained} chained"
        );
        // **The END of the chain is what governs it.** This is the assertion
        // the old code could not pass: closing C silenced nothing, because
        // the audio was reaching the master through B and bypassing C.
        assert!(
            shut.abs() < 1.0e-4,
            "closing the last link did not silence the chain: {shut}"
        );
    }

    /// **A CYCLE does not feed itself inside one callback.** Left unordered
    /// on purpose: it gathers whatever the stem holds at that moment, so
    /// A->B->A carries a block of delay rather than running away. The test
    /// is that a block renders, finitely, at a sane level.
    #[test]
    fn a_routing_cycle_stays_finite() {
        use std::sync::Arc;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![0.5; 4000],
            rate: 48_000,
            source: std::path::PathBuf::from("c.wav"),
        });
        let (mut r, _tx, shared) = renderer_with_midi(2);
        let (mut to_audio, _from) = r.hijack_track_rings();
        to_audio
            .push(vec![Placed {
                clip,
                strip: 1,
                start: 0,
                offset: 0,
                len: 4000,
                fade_in: 0,
                fade_out: 0,
                gain: 1.0,
            }])
            .expect("room");
        // 1 -> 5, 5 -> 6, 6 -> 5: a loop with a tail feeding it.
        for (i, (a, b)) in [(1usize, 5usize), (5, 6), (6, 5)].iter().enumerate() {
            shared.routes[i].store(pack_route(*a, *b, false, false), Ordering::Relaxed);
            shared.route_gain[i].store(1.0f32.to_bits(), Ordering::Relaxed);
        }
        shared.route_count.store(3, Ordering::Release);
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);
        let mut out = vec![0.0f32; 256 * 2];
        for _ in 0..8 {
            out.fill(0.0);
            r.render(&mut out, 0, 0);
            for v in &out {
                assert!(v.is_finite(), "a cycle produced {v}");
                assert!(v.abs() < 8.0, "a cycle ran away to {v}");
            }
        }
    }

    /// **A route to a channel that does not exist is ignored, not indexed
    /// with.** The table is written by the UI and the desk can shrink under
    /// it; a bad word must cost the route and nothing else.
    #[test]
    fn a_route_naming_a_channel_off_the_desk_is_dropped() {
        let (mut r, _tx, shared) = renderer_with_midi(2);
        shared.routes[0].store(pack_route(200, 201, false, false), Ordering::Relaxed);
        shared.route_gain[0].store(1.0f32.to_bits(), Ordering::Relaxed);
        shared.route_count.store(1, Ordering::Release);
        let mut out = vec![0.0f32; 128 * 2];
        // The point is that this returns at all.
        r.render(&mut out, 0, 0);
        assert_eq!(r.route_n, 0, "a nonsense route survived the block's read");
    }

    /// **A packed route survives the round trip.** The table is the one place
    /// the UI and the audio thread agree, and it is bit-packed.
    #[test]
    fn a_route_packs_and_unpacks_unchanged() {
        for (from, target, hw, stereo) in [
            (0usize, 0usize, false, false),
            (15, 2, true, true),
            (3, 9, false, false),
            (7, 17, true, false),
        ] {
            let w = pack_route(from, target, hw, stereo);
            assert_eq!(unpack_route(w), Some((from, target, hw, stereo)));
        }
        // A zeroed slot is EMPTY, not "channel 0 to channel 0".
        assert_eq!(unpack_route(0), None);
    }

    /// **An arrangement of clips plays placed, trimmed, and faded.**
    ///
    /// The document's promises, heard: a clip stands where `start` says, a
    /// trim starts the FILE where `offset` says, `len` is the clamp, two
    /// clips overlapping SUM, a fade-in ramps from silence, and a fade-out
    /// reaches exactly zero on the last frame so a faded clip cannot click
    /// shut.
    #[test]
    fn an_arrangement_of_clips_plays_placed_trimmed_and_faded() {
        use std::sync::Arc;
        let constant = |v: f32| {
            Arc::new(ivory_record::decode::Clip {
                samples: vec![v; 2000],
                rate: 48_000,
                source: std::path::PathBuf::from("c.wav"),
            })
        };
        let a = constant(0.5);
        let b = constant(1.0);

        let (mut r, _tx, shared) = renderer_with_midi(2);
        let (mut to_audio, _from_audio) = r.hijack_track_rings();
        to_audio
            .push(vec![
                // A: the first 500 frames of the timeline, bare.
                Placed {
                    clip: Arc::clone(&a),
                    strip: 0,
                    start: 0,
                    offset: 0,
                    len: 500,
                    fade_in: 0,
                    fade_out: 0,
                    gain: 1.0,
                },
                // B: trimmed 100 frames into its file, 300 long, standing at
                // 400 — so 400..500 overlaps A — with 100-frame ramps.
                Placed {
                    clip: Arc::clone(&b),
                    strip: 0,
                    start: 400,
                    offset: 100,
                    len: 300,
                    fade_in: 100,
                    fade_out: 100,
                    gain: 1.0,
                },
            ])
            .expect("room");
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);

        let mut out = vec![0.0_f32; 2 * 1024];
        r.render(&mut out, 0, 0);
        let left = |f: usize| out[2 * f];
        let close = |f: usize, want: f32, what: &str| {
            assert!(
                (left(f) - want).abs() < 1.0e-3,
                "frame {f} ({what}): heard {}, wanted {want}",
                left(f)
            );
        };
        close(10, 0.5, "A alone");
        close(450, 0.5 + 0.5, "A plus B halfway up its fade-in");
        close(550, 1.0, "B alone, past its fade");
        // B's tail: at k=250, 50 frames remain of a 100-frame fade-out.
        close(650, 0.49, "B halfway down its fade-out");
        close(699, 0.0, "B's LAST frame is exactly silent");
        close(720, 0.0, "past everything");
    }

    /// **The pan law: centre is unity, a hard side is alone.** A clip on a
    /// lane panned hard left leaves the right bus empty and rides the left
    /// at the law's +3 dB; at centre both sides carry exactly what they
    /// carried before pan knobs existed.
    #[test]
    fn a_lanes_pan_places_its_clips() {
        use std::sync::Arc;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: vec![0.5; 4000],
            rate: 48_000,
            source: std::path::PathBuf::from("c.wav"),
        });
        let run = |pan: f32| -> (f32, f32) {
            let (mut r, _tx, shared) = renderer_with_midi(2);
            let (mut to_audio, _from) = r.hijack_track_rings();
            to_audio
                .push(vec![Placed {
                    clip: Arc::clone(&clip),
                    strip: 0,
                    start: 0,
                    offset: 0,
                    len: 2000,
                    fade_in: 0,
                    fade_out: 0,
                    gain: 1.0,
                }])
                .expect("room");
            shared.set_channel_pan(0, pan);
            shared.transport_at.store(0, Ordering::Relaxed);
            shared.transport_req.store(1, Ordering::Release);
            shared.rolling.store(true, Ordering::Relaxed);
            let mut out = vec![0.0_f32; 2 * 1024];
            r.render(&mut out, 0, 0);
            r.render(&mut out, 0, 0);
            (out[2 * 512], out[2 * 512 + 1])
        };
        let (l, r) = run(0.0);
        assert!(
            (l - 0.5).abs() < 0.01 && (r - 0.5).abs() < 0.01,
            "centre is not unity: {l} {r}"
        );
        let (l, r) = run(-1.0);
        assert!(r.abs() < 1.0e-3, "hard left leaks right: {r}");
        assert!(
            (l - 0.5 * std::f32::consts::SQRT_2).abs() < 0.01,
            "hard left is not +3dB: {l}"
        );
        let (l, r) = run(1.0);
        assert!(l.abs() < 1.0e-3, "hard right leaks left: {l}");
        assert!((r - 0.5 * std::f32::consts::SQRT_2).abs() < 0.01);
    }

    /// **A MIDI clip sounds with its lane UNARMED — playback never asks the
    /// arm.** The arm is the LIVE switch: it decides whether the keyboard
    /// reaches the lane, and nothing else. A project must open silent to the
    /// touch and still PLAY — that is the whole "no headphones blast" rule,
    /// and this is the half of it that could regress silently: the gate that
    /// mutes live input sits one condition away from muting the document.
    ///
    /// Stopped first (a rolling transport is the clip's other gate), then
    /// rolling — same desk, same clip, arm off throughout.
    #[test]
    fn a_midi_clip_sounds_on_an_unarmed_lane() {
        use std::sync::Arc;
        let (mut r, _tx, shared) = renderer_with_midi(2);
        let (mut to_audio, _from) = r.hijack_midi_rings();
        // One long C4, forte, from the top of the timeline, on the
        // built-in's lane.
        to_audio
            .push(vec![PlacedMidi {
                strip: 0,
                start: 0,
                len: 96_000,
                notes: Arc::new(vec![ClipNote {
                    at: 0,
                    dur: 48_000,
                    key: 60,
                    vel: 100,
                }]),
            }])
            .expect("room");
        // Lane 0 UNARMED: the bit set is the arm off, exactly what the host
        // pushes when the R goes dark.
        shared.midi_off.store(1, Ordering::Relaxed);
        shared.armed.store(!1u32, Ordering::Relaxed);

        let level = |r: &mut Renderer| -> f32 {
            let mut out = vec![0.0_f32; 2 * 1024];
            let mut peak = 0.0f32;
            for _ in 0..8 {
                out.fill(0.0);
                r.render(&mut out, 0, 0);
                peak = out.iter().fold(peak, |a, v| a.max(v.abs()));
            }
            peak
        };
        assert!(
            level(&mut r) < 1.0e-6,
            "a stopped transport must keep the clip silent"
        );
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);
        assert!(
            level(&mut r) > 1.0e-3,
            "the clip must SOUND while rolling, arm or no arm"
        );
    }

    /// **A jump that lands ON a note does not strand it.** The flush sweeps
    /// what was sounding BEFORE the jump; a note whose on is emitted in the
    /// very block that flushed must keep its ledger entry, or its off is
    /// never owed and the key hangs until the next flush.
    #[test]
    fn a_jump_landing_on_a_note_keeps_its_off_owed() {
        use std::sync::Arc;
        let (mut r, _tx, shared) = renderer_with_midi(2);
        let (mut to_audio, _from) = r.hijack_midi_rings();
        to_audio
            .push(vec![PlacedMidi {
                strip: 0,
                start: 0,
                len: 96_000,
                notes: Arc::new(vec![ClipNote {
                    at: 48_000,
                    dur: 24_000,
                    key: 64,
                    vel: 90,
                }]),
            }])
            .expect("room");
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);
        let mut out = vec![0.0_f32; 2 * 1024];
        r.render(&mut out, 0, 0);
        assert!(r.active_clip.is_empty(), "nothing is due at the top");
        // The jump, straight onto the note's own frame: the same block
        // flushes the strands AND starts the note.
        shared.transport_at.store(48_000, Ordering::Relaxed);
        shared.transport_req.store(2, Ordering::Release);
        r.render(&mut out, 0, 0);
        assert_eq!(
            r.active_clip.len(),
            1,
            "the flush swept the note the jump landed on — its off is lost"
        );
    }

    /// **The drumpad is a lane's instrument now.** A MIDI clip on the drum
    /// strip strikes the pads — the editor's roll plays the kit — and a
    /// lane NOT assigned stays a pitch lane.
    #[test]
    fn a_drum_lane_strikes_the_pads() {
        use std::sync::Arc;
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        e.set_pad_kit(test_kit(&[0], 4_800, 0.5));
        e.set_midi_clips(vec![PlacedMidi {
            strip: 7,
            start: 0,
            len: 48_000,
            notes: Arc::new(vec![ClipNote {
                at: 0,
                dur: 2_400,
                key: PAD_BASE, // pad 0
                vel: 100,
            }]),
        }]);
        e.set_drum_strip(Some(7));
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 1024 * 2];
        let mut peak = 0.0f32;
        for _ in 0..4 {
            out.fill(0.0);
            e.render_offline(&mut out);
            peak = peak.max(out.iter().fold(0.0f32, |a, b| a.max(b.abs())));
        }
        assert!(peak > 0.05, "the drum lane's note struck no pad: {peak}");

        // Un-assign the lane: the same clip now feeds nothing (no builtin,
        // no rack) and the render is silence — the strike path really was
        // the assignment, not some default.
        let mut quiet = Engine::offline(48_000, Timebase::new()).expect("offline");
        quiet.set_pad_kit(test_kit(&[0], 4_800, 0.5));
        quiet.set_midi_clips(vec![PlacedMidi {
            strip: 7,
            start: 0,
            len: 48_000,
            notes: Arc::new(vec![ClipNote {
                at: 0,
                dur: 2_400,
                key: PAD_BASE,
                vel: 100,
            }]),
        }]);
        quiet.set_transport(1, 0);
        quiet.set_rolling(true);
        let mut still = 0.0f32;
        for _ in 0..4 {
            out.fill(0.0);
            quiet.render_offline(&mut out);
            still = still.max(out.iter().fold(0.0f32, |a, b| a.max(b.abs())));
        }
        assert!(
            still < 1.0e-6,
            "an unassigned lane still struck the pads: {still}"
        );
    }

    /// **The kit answers to its LANE's strip now**: the drum channel's
    /// fader and mute govern the summed pads, and the pads land in that
    /// lane's stem — the retired drum-bus strip rules nothing.
    #[test]
    fn the_kit_answers_to_its_lanes_strip() {
        use std::sync::Arc;
        let clip = || {
            vec![PlacedMidi {
                strip: 7,
                start: 0,
                len: 48_000,
                notes: Arc::new(vec![ClipNote {
                    at: 0,
                    dur: 2_400,
                    key: PAD_BASE,
                    vel: 100,
                }]),
            }]
        };
        let render_peak = |mute_lane: bool, gain: f32| {
            let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
            e.set_pad_kit(test_kit(&[0], 4_800, 0.5));
            e.set_midi_clips(clip());
            e.set_drum_strip(Some(7));
            e.set_channel_gain(7, gain);
            if mute_lane {
                e.shared
                    .muted
                    .store(Strip::Channel(7).bit(), Ordering::Relaxed);
            }
            e.set_transport(1, 0);
            e.set_rolling(true);
            let mut out = vec![0.0f32; 1024 * 2];
            let mut peak = 0.0f32;
            // The LAST block only: the bus gain smooths from unity, so the
            // first blocks carry a fading transient whatever the target.
            for block in 0..4 {
                out.fill(0.0);
                e.render_offline(&mut out);
                if block == 3 {
                    peak = out.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                }
            }
            peak
        };
        let loud = render_peak(false, 1.0);
        assert!(loud > 0.05, "the lane-routed kit is silent: {loud}");
        let muted = render_peak(true, 1.0);
        assert!(
            muted < loud * 0.05,
            "muting the drum lane left the kit sounding: {muted} vs {loud}"
        );
        let quiet = render_peak(false, 0.25);
        assert!(
            quiet < loud * 0.7,
            "the lane fader has no hand on the kit: {quiet} vs {loud}"
        );
    }

    /// **Obey-note-offs and the trim window are real.** A pad that obeys
    /// goes quiet at the release; one that does not rings on. A trimmed
    /// pad ends where the trim says, whatever the note length.
    #[test]
    fn a_pad_obeys_note_offs_and_its_trim() {
        use std::sync::Arc;
        let render = |obey: bool, trim_end: f32| {
            let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
            e.set_pad_kit(test_kit(&[0], 48_000, 0.5));
            let mut kit = ivory_ui::recorder::DrumKitParams::default();
            kit.pads[0].obey = obey;
            kit.pads[0].trim_end = trim_end;
            e.set_pad_params(&kit);
            e.set_midi_clips(vec![PlacedMidi {
                strip: 7,
                start: 0,
                len: 48_000,
                notes: Arc::new(vec![ClipNote {
                    at: 0,
                    dur: 2_400, // the off lands 50 ms in
                    key: PAD_BASE,
                    vel: 100,
                }]),
            }]);
            e.set_drum_strip(Some(7));
            e.set_transport(1, 0);
            e.set_rolling(true);
            let mut out = vec![0.0f32; 1024 * 2];
            // Peak of the block at ~0.2 s — past the off and the fade.
            let mut late = 0.0f32;
            for block in 0..12 {
                out.fill(0.0);
                e.render_offline(&mut out);
                if block == 11 {
                    late = out.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                }
            }
            late
        };
        let ringing = render(false, 1.0);
        assert!(
            ringing > 0.05,
            "a one-shot went quiet on its own: {ringing}"
        );
        let obeyed = render(true, 1.0);
        assert!(
            obeyed < ringing * 0.05,
            "the release did not fade the obeying pad: {obeyed} vs {ringing}"
        );
        // A trim ending at 60 ms kills even the disobedient one-shot well
        // before the 0.23 s mark.
        let trimmed = render(false, 0.06);
        assert!(
            trimmed < ringing * 0.05,
            "the trim window did not bind: {trimmed} vs {ringing}"
        );
    }

    /// **The count-in still delivers its downbeat while the transport
    /// stands still** — the arrangement record WAITS on it, and the
    /// metronome's rolling gate must not strangle it.
    #[test]
    fn a_count_in_finishes_without_the_transport_rolling() {
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        // The owner's shape: metronome armed, transport STOPPED, count-in
        // requested by the red button.
        e.set_metronome_enabled(true);
        e.set_tempo(120.0);
        e.start_count_in(2, 120.0);
        assert!(!e.count_in_done(), "done before a single click");
        let mut out = vec![0.0f32; 1024 * 2];
        // Two beats at 120 bpm = 1 s = ~47 blocks; give it 80.
        let mut done_at = None;
        for block in 0..80 {
            out.fill(0.0);
            e.render_offline(&mut out);
            if e.count_in_done() {
                done_at = Some(block);
                break;
            }
        }
        assert!(
            done_at.is_some(),
            "the count-in never finished — the record waits forever"
        );
    }

    #[test]
    fn a_tied_note_on_one_key_sounds_again() {
        use std::sync::Arc;
        // Two notes on the SAME key, back to back: the second must arrive
        // as a fresh on AFTER the first's off — the owner's tied chords
        // played once and went quiet.
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        e.set_midi_clips(vec![PlacedMidi {
            strip: 0,
            start: 0,
            len: 96_000,
            notes: Arc::new(vec![
                ClipNote {
                    at: 0,
                    dur: 4_800,
                    key: 60,
                    vel: 100,
                },
                ClipNote {
                    at: 4_800,
                    dur: 4_800,
                    key: 60,
                    vel: 100,
                },
            ]),
        }]);
        e.set_builtin_strip(Some(0));
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 1024 * 2];
        // Roll through both notes; the tie lands mid-way.
        let mut heard_first = 0.0f32;
        let mut heard_after_tie = 0.0f32;
        for block in 0..12 {
            out.fill(0.0);
            e.render_offline(&mut out);
            // Blocks 6..9 sit inside the SECOND note (4800+ frames in).
            if (1..4).contains(&block) {
                heard_first = heard_first.max(out.iter().fold(0.0f32, |a, b| a.max(b.abs())));
            }
            if (6..9).contains(&block) {
                heard_after_tie =
                    heard_after_tie.max(out.iter().fold(0.0f32, |a, b| a.max(b.abs())));
            }
        }
        assert!(
            heard_first > 0.01,
            "the FIRST note never sounded: {heard_first}"
        );
        assert!(
            heard_after_tie > 0.005,
            "the second note of the tie never sounded: {heard_after_tie}"
        );
    }

    /// **Every pad seat is inside the rack space `load_insert` admits.** The
    /// guard capped at `STRIPS`, so pad FX (seats `STRIPS+1..`) were refused
    /// and did nothing; this pins that the whole seat map fits `RACK_SEATS`.
    #[test]
    fn pad_seats_are_loadable_rack_indices() {
        for pad in [0, 1, PADS / 2, PADS - 1] {
            let seat = pad_rack_index(pad);
            assert!(
                seat > STRIPS && seat < RACK_SEATS,
                "pad {pad} seat {seat} is outside the loadable range 0..{RACK_SEATS}"
            );
        }
    }

    /// **A pad seat reaches the editor door.** `open_insert_editor` kept the
    /// pre-pads bound after `load_insert` was widened, so every pad seat was
    /// refused with a silent `Ok(())`: the plugin loaded, ran, and its window
    /// could never open. An EMPTY pad bay must answer "nothing is loaded" —
    /// the honest error PAST the guard — and only a seat past the whole map
    /// is the silent no-op.
    #[test]
    fn a_pad_seat_reaches_the_editor_door() {
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        assert!(
            e.open_insert_editor(pad_rack_index(0), 0).is_err(),
            "pad 0's empty bay answered Ok — the guard is refusing pad seats \
             again, and a pad plugin's window can never open"
        );
        assert!(
            e.open_insert_editor(pad_rack_index(PADS - 1), INSERTS - 1)
                .is_err(),
            "the last pad's last bay is inside the seat map too"
        );
        // Past the whole map is still a no-op, never a panic and never an
        // error a menu would have to word.
        assert!(e.open_insert_editor(RACK_SEATS, 0).is_ok());
    }

    /// **A refused hand-off comes back instead of dying silently.** The
    /// seat's ring is two deep; with nothing pulling the render, the third
    /// box must be handed BACK — it used to be dropped inside the discarded
    /// push error, instance and all, while `load_insert` carried on
    /// installing the dead instance's editor and state handles.
    #[test]
    fn a_full_insert_ring_hands_the_box_back() {
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        assert!(e.hand_off_insert(0, 0, PluginBox(None)).is_none());
        assert!(e.hand_off_insert(0, 0, PluginBox(None)).is_none());
        assert!(
            e.hand_off_insert(0, 0, PluginBox(None)).is_some(),
            "a push into a full ring reported delivery — the box it holds \
             was dropped on the spot and the caller cannot know"
        );
        // A seat that does not exist cannot take anything either.
        assert!(e.hand_off_insert(RACK_SEATS, 0, PluginBox(None)).is_some());
    }

    /// **A looped MIDI clip keeps sounding past the first wrap.** The note
    /// scheduler is cursor-based and forward-only; a loop moves `pos`
    /// backward without a locate, so without the wrap's flush the cursor
    /// sits at the clip's end and every pass after the first is silent —
    /// exactly the regression the owner heard (audio clips survived, MIDI
    /// did not). A short loop over one repeating note; sound must be heard
    /// well after the first wrap.
    #[test]
    fn a_looped_midi_clip_survives_the_wrap() {
        use std::sync::Arc;
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        // A note every 2400 frames across a 24000-frame clip.
        let notes: Vec<ClipNote> = (0..10)
            .map(|i| ClipNote {
                at: i * 2_400,
                dur: 1_200,
                key: 60,
                vel: 100,
            })
            .collect();
        e.set_midi_clips(vec![PlacedMidi {
            strip: 0,
            start: 0,
            len: 24_000,
            notes: Arc::new(notes),
        }]);
        e.set_builtin_strip(Some(0));
        // Loop the whole clip: [0, 24000). At 512 frames/block that wraps
        // about every 47 blocks.
        e.set_loop(0, 24_000);
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 512 * 2];
        let mut heard_late = 0.0f32;
        // 200 blocks ≈ 4 wraps; sample loudness only AFTER the second wrap.
        for block in 0..200 {
            out.fill(0.0);
            e.render_offline(&mut out);
            if block > 100 {
                heard_late = heard_late.max(out.iter().fold(0.0f32, |a, b| a.max(b.abs())));
            }
        }
        assert!(
            heard_late > 0.005,
            "the looped MIDI clip went silent after wrapping: {heard_late}"
        );
    }

    /// **A clip note the capacity break withholds is parked, not dropped.**
    /// The walk consumed the note (`cur += 1`) BEFORE checking the block's
    /// room, so when the note list or the sounding ledger was full the
    /// stored cursor stood past a note nothing ever delivered — and the next
    /// block's behind-the-playhead skip guaranteed it stayed lost. The live
    /// path parks its overflow in `pending`; the clips park on the cursor:
    /// it stays ON the note, and the note sounds — one block late — when
    /// there is room again.
    #[test]
    fn a_clip_note_the_full_block_withholds_sounds_the_next_block() {
        let shared = Arc::new(Shared::new());
        let (tap_tx, _tap_rx) = RingBuffer::<f32>::new(1 << 16);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
        // 256 fillers on lane 0, all striking at frame 0: exactly enough to
        // fill the sounding ledger before lane 1's walk runs. Their offs
        // fall due in the SECOND block, which is what makes room.
        let fillers: Vec<ClipNote> = (0..256)
            .map(|_| ClipNote {
                at: 0,
                dur: 700,
                key: 60,
                vel: 100,
            })
            .collect();
        // And the victim on lane 1, due the same first block.
        r.midi_clips = vec![
            PlacedMidi {
                strip: 0,
                start: 0,
                len: 48_000,
                notes: Arc::new(fillers),
            },
            PlacedMidi {
                strip: 1,
                start: 0,
                len: 48_000,
                notes: Arc::new(vec![ClipNote {
                    at: 0,
                    dur: 40_000,
                    key: 100,
                    vel: 100,
                }]),
            },
        ];
        r.midi_cursor = vec![0, 0];
        shared.rolling.store(true, Ordering::Relaxed);
        let mut out = vec![0.0f32; 512 * 2];
        // Block one: the ledger fills, the victim hits the break.
        r.render(&mut out, 0, 0);
        assert_eq!(
            r.midi_cursor[1], 0,
            "the cursor moved past the withheld note — consumed by the \
             capacity break, delivered by nothing"
        );
        // Block two: the fillers' offs fall due, the ledger empties, and the
        // parked note sounds.
        out.fill(0.0);
        r.render(&mut out, 0, 0);
        assert!(
            r.sounding & (1 << 100) != 0,
            "the withheld clip note never sounded"
        );
        assert!(
            r.active_clip.iter().any(|a| a.key == 100 && a.lane == 1),
            "the parked note is missing from the sounding ledger, so its \
             off will never be emitted"
        );
        assert_eq!(r.midi_cursor[1], 1);
    }

    #[test]
    fn an_offline_engine_taps_stems_too() {
        use std::sync::Arc;
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        e.set_clips(vec![Placed {
            clip: Arc::new(ivory_record::decode::Clip {
                samples: vec![0.5; 96_000],
                rate: 48_000,
                source: std::path::PathBuf::from("c.wav"),
            }),
            strip: 2,
            start: 0,
            offset: 0,
            len: 48_000,
            fade_in: 0,
            fade_out: 0,
            gain: 1.0,
        }]);
        e.set_stems(true);
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 1024 * 2];
        let mut drained: Vec<f32> = Vec::new();
        for _ in 0..8 {
            out.fill(0.0);
            e.render_offline(&mut out);
            e.drain_stems(&mut drained);
        }
        assert!(!drained.is_empty(), "the stem tap delivered nothing");
        let wide = CHANNELS * 2;
        let frames = drained.len() / wide;
        let lane2_peak = (0..frames)
            .map(|f| drained[f * wide + 2 * 2].abs())
            .fold(0.0f32, f32::max);
        assert!(
            lane2_peak > 0.1,
            "lane 2's stem is silent: {lane2_peak} over {frames} frames"
        );
    }

    /// **The loop wraps the transport on the audio thread.** A region set
    /// over [1000, 3000) frames; roll from 0 in 512-frame blocks and the
    /// clock must never reach 3000 — it folds back near 1000. A playhead
    /// that STARTS past the region (someone playing on ahead of the loop)
    /// is left alone.
    #[test]
    fn the_loop_wraps_the_clock() {
        let mut e = Engine::offline(48_000, Timebase::new()).expect("offline");
        e.set_loop(1000, 3000);
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 512 * 2];
        let mut max_seen = 0u64;
        for _ in 0..40 {
            out.fill(0.0);
            e.render_offline(&mut out);
            max_seen = max_seen.max(e.transport_position());
        }
        assert!(
            max_seen < 3000,
            "the clock ran past the loop end: {max_seen}"
        );
        assert!(
            max_seen >= 1000,
            "the clock never entered the loop: {max_seen}"
        );

        // A clear (a >= b) frees the clock to run straight past.
        e.set_loop(0, 0);
        e.set_transport(2, 0);
        for _ in 0..40 {
            out.fill(0.0);
            e.render_offline(&mut out);
        }
        assert!(
            e.transport_position() > 3000,
            "the cleared loop still held the clock at {}",
            e.transport_position()
        );

        // A playhead started PAST the region is not yanked back.
        e.set_loop(1000, 3000);
        e.set_transport(3, 5000);
        out.fill(0.0);
        e.render_offline(&mut out);
        assert!(
            e.transport_position() >= 5000,
            "a playhead ahead of the loop was pulled back to {}",
            e.transport_position()
        );
    }

    #[test]
    fn an_offline_engine_renders_the_song_by_hand() {
        use std::sync::Arc;
        // No device anywhere near this: the offline door.
        let mut e = match Engine::offline(48_000, Timebase::new()) {
            Ok(e) => e,
            Err(err) => panic!("the offline engine refused to build: {err}"),
        };
        e.set_clips(vec![Placed {
            clip: Arc::new(ivory_record::decode::Clip {
                samples: vec![0.5; 96_000],
                rate: 48_000,
                source: std::path::PathBuf::from("c.wav"),
            }),
            strip: 0,
            start: 0,
            offset: 0,
            len: 48_000,
            fade_in: 0,
            fade_out: 0,
            gain: 1.0,
        }]);
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 1024 * 2];
        let mut heard = 0.0f32;
        // A couple of blocks so the clip swap lands and the lane gain slews.
        for _ in 0..8 {
            out.fill(0.0);
            e.render_offline(&mut out);
            heard = heard.max(out[2 * 512].abs());
        }
        assert!(heard > 0.1, "the offline engine rendered silence: {heard}");
        // And the transport really advanced — this is a pull, not a loop
        // over the same block.
        assert!(
            e.transport_position() >= 8 * 1024,
            "the playhead did not move: {}",
            e.transport_position()
        );
    }

    #[test]
    fn a_punched_lane_goes_quiet_from_the_punch_frame_on() {
        use std::sync::Arc;
        let clip = Placed {
            clip: Arc::new(ivory_record::decode::Clip {
                samples: vec![1.0; 8000],
                rate: 48_000,
                source: std::path::PathBuf::from("c.wav"),
            }),
            strip: 0,
            start: 0,
            offset: 0,
            len: 4000,
            fade_in: 0,
            fade_out: 0,
            gain: 1.0,
        };
        let (mut r, _tx, shared) = renderer_with_midi(2);
        let (mut to_audio, _from) = r.hijack_track_rings();
        to_audio.push(vec![clip]).expect("room");
        shared.transport_at.store(0, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);
        // The punch lands at frame 1024: the first block sounds, the
        // second is the material being recorded over and must not.
        shared.punch[0].store(1024, Ordering::Relaxed);
        let mut out = vec![0.0_f32; 2 * 1024];
        r.render(&mut out, 0, 0);
        assert!(
            out[2 * 1000].abs() > 0.1,
            "the clip before the punch went missing: {}",
            out[2 * 1000]
        );
        out.fill(0.0);
        r.render(&mut out, 0, 0);
        assert!(
            out[2 * 512].abs() < 1.0e-6,
            "the punched region still sounds: {}",
            out[2 * 512]
        );
        // And clearing the punch brings the lane back — the take ended,
        // the carve owns the truth now.
        shared.punch[0].store(u64::MAX, Ordering::Relaxed);
        out.fill(0.0);
        r.render(&mut out, 0, 0);
        assert!(
            out[2 * 512].abs() > 0.1,
            "the lane stayed quiet after the punch lifted"
        );
    }

    #[test]
    fn a_lanes_mute_and_solo_govern_its_clips() {
        use std::sync::Arc;
        let constant = |v: f32| {
            Arc::new(ivory_record::decode::Clip {
                samples: vec![v; 4000],
                rate: 48_000,
                source: std::path::PathBuf::from("c.wav"),
            })
        };
        let clip_on = |lane: usize, level: f32| Placed {
            clip: constant(level),
            strip: lane,
            start: 0,
            offset: 0,
            len: 2000,
            fade_in: 0,
            fade_out: 0,
            gain: 1.0,
        };
        let run = |muted: u32, soloed: u32| -> f32 {
            let (mut r, _tx, shared) = renderer_with_midi(2);
            let (mut to_audio, _from) = r.hijack_track_rings();
            to_audio
                .push(vec![clip_on(0, 0.5), clip_on(2, 1.0)])
                .expect("room");
            shared.transport_at.store(0, Ordering::Relaxed);
            shared.transport_req.store(1, Ordering::Release);
            shared.rolling.store(true, Ordering::Relaxed);
            shared.muted.store(muted, Ordering::Relaxed);
            shared.soloed.store(soloed, Ordering::Relaxed);
            let mut out = vec![0.0_f32; 2 * 1024];
            // Two renders: the first lets the lane gains slew to their
            // targets, the second is what is measured.
            r.render(&mut out, 0, 0);
            r.render(&mut out, 0, 0);
            out[2 * 512]
        };
        assert!((run(0, 0) - 1.5).abs() < 0.01, "both lanes should sum");
        assert!(
            (run(1 << 2, 0) - 0.5).abs() < 0.01,
            "muting lane 3 should leave lane 1 alone"
        );
        assert!(
            (run(0, 1 << 2) - 1.0).abs() < 0.01,
            "soloing lane 3 should silence lane 1"
        );
    }

    /// **A patch you pick has to be the patch you hear.** The picker is a list
    /// of names and a list of names is easy to get right while the voice behind
    /// it never reaches the audio thread — the whole chain is a `Mutex` the
    /// renderer checks once a block, and a chain that silently does nothing
    /// looks exactly like a cartridge full of similar patches.
    ///
    /// Two patches chosen to be unmistakably different: an organ (algorithm 32,
    /// six carriers, no modulation) against the tine piano.
    /// **The transport is the position, and the clip merely stands on it.**
    ///
    /// This replaces the trim-points test — trim is gone, the playhead is the
    /// feature — and it is the audio half of `transport.rs`'s contract:
    ///
    /// 1. stopped is silent and the position does not move;
    /// 2. a locate is an EVENT — the same generation re-asserted moves nothing;
    /// 3. a locate lands while rolling, which the old level could never say;
    /// 4. `Placed::start` positions a clip with one subtraction;
    /// 5. the callback acks the generation it applied;
    /// 6. a displaced clip comes BACK on the ring — never dropped on the
    ///    audio thread, which used to free a hundred megabytes mid-callback;
    /// 7. a stop ends the notes it strands.
    #[test]
    fn the_backing_track_follows_the_transport() {
        use std::sync::Arc;
        let frames = 1000usize;
        let clip = Arc::new(ivory_record::decode::Clip {
            samples: (0..frames).flat_map(|i| [i as f32, i as f32]).collect(),
            rate: 48_000,
            source: std::path::PathBuf::from("t.wav"),
        });

        let (mut r, mut tx, shared) = renderer_with_midi(2);
        let (mut to_audio, mut from_audio) = r.hijack_track_rings();
        to_audio
            .push(vec![Placed::whole(Arc::clone(&clip), 0)])
            .expect("room");

        // 1. Stopped: silence, and the clock does not move.
        let mut out = vec![0.0_f32; 2 * 256];
        r.render(&mut out, 0, 0);
        assert_eq!(
            out.iter().fold(0.0f32, |m, s| m.max(s.abs())),
            0.0,
            "the track played with the transport stopped"
        );
        assert_eq!(shared.transport_pos.load(Ordering::Relaxed), 0);

        // 2 + 3. Locate to 100 and roll: playback starts at frame 100.
        shared.transport_at.store(100, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);
        let mut out = vec![0.0_f32; 2 * 256];
        r.render(&mut out, 0, 0);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert!(
            (left[0] - 100.0).abs() < 0.5,
            "it started at frame {} and the locate said 100",
            left[0]
        );
        // 5. And the callback has acked the generation it applied.
        assert_eq!(shared.transport_ack.load(Ordering::Acquire), 1);

        // 2. The SAME generation re-asserted is not a second locate.
        shared.transport_req.store(1, Ordering::Release);
        let mut out = vec![0.0_f32; 2 * 64];
        r.render(&mut out, 0, 0);
        assert!(
            (out[0] - 356.0).abs() < 0.5,
            "re-asserting a generation re-located: got {}, wanted continuity at 356",
            out[0]
        );

        // 3. A JUMP while rolling lands mid-flight — the thing the old
        // edge-detected level made inexpressible.
        shared.transport_at.store(500, Ordering::Relaxed);
        shared.transport_req.store(2, Ordering::Release);
        let mut out = vec![0.0_f32; 2 * 64];
        r.render(&mut out, 0, 0);
        assert!(
            (out[0] - 500.0).abs() < 0.5,
            "a locate while rolling did nothing: got {}, wanted 500",
            out[0]
        );

        // 1 again. Stop: silence, and the position FREEZES — returning to
        // zero is the HOST's rule, applied by a locate, not the callback's.
        shared.rolling.store(false, Ordering::Relaxed);
        let mut out = vec![0.0_f32; 2 * 64];
        r.render(&mut out, 0, 0);
        let frozen = shared.transport_pos.load(Ordering::Relaxed);
        let mut out = vec![0.0_f32; 2 * 64];
        r.render(&mut out, 0, 0);
        assert_eq!(
            shared.transport_pos.load(Ordering::Relaxed),
            frozen,
            "the clock moved while stopped"
        );

        // 4. A clip PLACED at 300, located to 250: fifty frames of silence,
        // then the clip from its own first sample.
        to_audio
            .push(vec![Placed::whole(Arc::clone(&clip), 300)])
            .expect("room");
        shared.transport_at.store(250, Ordering::Relaxed);
        shared.transport_req.store(3, Ordering::Release);
        shared.rolling.store(true, Ordering::Relaxed);
        let mut out = vec![0.0_f32; 2 * 128];
        r.render(&mut out, 0, 0);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert_eq!(left[0], 0.0, "before the clip's start is not silence");
        assert_eq!(left[49], 0.0, "frame 299 is inside the clip somehow");
        assert!(
            (left[50] - 0.0).abs() < 0.5 && (left[60] - 10.0).abs() < 0.5,
            "the placed clip did not start at its own frame 0: {} then {}",
            left[50],
            left[60]
        );

        // 6. The displaced clip came BACK rather than dying in the callback.
        // The ring holds every displacement in order — the first swap
        // displaced the initial `None` — so drain it and the clip must be in
        // what returns.
        let mut came_back = false;
        while let Ok(returned) = from_audio.pop() {
            if returned
                .first()
                .is_some_and(|p| Arc::ptr_eq(&p.clip, &clip))
            {
                came_back = true;
            }
        }
        assert!(came_back, "the displaced clip never came back on the ring");

        // 7. A note sounding when the transport stops is ENDED, not stranded.
        //
        // Stamp 0, not `timebase.now()`: the renderer's clock is only a few
        // hundred frames old, and the WALL clock is however long this test
        // has been running — on a slower machine that stamp lands beyond the
        // next block and the note is correctly held as not-yet-due, which
        // reads as a failure here. Zero is already due on any machine.
        queue(&mut tx, 0, [0x90, 60, 100]);
        let mut out = vec![0.0_f32; 2 * 64];
        r.render(&mut out, 0, 0);
        assert_ne!(r.sounding & (1 << 60), 0, "the note-on was never seen");
        shared.rolling.store(false, Ordering::Relaxed);
        let mut out = vec![0.0_f32; 2 * 64];
        r.render(&mut out, 0, 0);
        assert_eq!(r.sounding, 0, "stopping left a note stranded");
    }

    /// **The built-in's slot fader moves the built-in.**
    ///
    /// It did not, and it looked exactly like a working control: the number
    /// reached the settings, the engine and the meter. What it never reached
    /// was the audio — the FM renders by ADDING into the bus, so it went
    /// straight on with no gain applied while every plugin beside it went
    /// through `mix_in` with its own. Reported as "the level slider does
    /// nothing for the DX7 but works for VSTs", which is precisely what that
    /// is.
    #[test]
    fn the_builtin_is_moved_by_its_own_slot_fader() {
        let level = |gain: f32| {
            let (mut r, mut tx, shared) = renderer_with_midi(2);
            // The built-in is CHANNEL 0's now; its fader is the channel's.
            if let Some(cell) = shared.channel_gains.first() {
                cell.store(gain.to_bits(), Ordering::Relaxed);
            }
            r.channel_gain[0] = gain;
            queue(&mut tx, 0, [0x90, 60, 100]);
            let mut out = vec![0.0_f32; 2 * 4096];
            r.render(&mut out, 0, 0);
            out.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
        };

        let unity = level(1.0);
        assert!(unity > 0.01, "the built-in was silent at unity");
        let half = level(0.5);
        assert!(
            (half / unity - 0.5).abs() < 0.05,
            "half gain came out at {:.3} of unity",
            half / unity
        );
        let off = level(0.0);
        assert!(
            off < unity * 0.02,
            "the fader at zero still let {off} through"
        );
    }

    #[test]
    fn choosing_a_patch_changes_what_comes_out() {
        use crate::dx7::{Op, Voice};

        let sound = |r: &mut Renderer, tx: &mut rtrb::Producer<MidiEvent>| {
            queue(tx, 0, [0x90, 60, 100]);
            let mut out = vec![0.0_f32; 2 * 4096];
            r.render(&mut out, 0, 0);
            queue(tx, 0, [0x80, 60, 0]);
            let mut quiet = vec![0.0_f32; 2 * 4096];
            r.render(&mut quiet, 0, 0);
            out
        };

        let (mut r, mut tx, _) = renderer_with_midi(2);
        let piano = sound(&mut r, &mut tx);

        // An organ: every operator a carrier, so it holds where the piano
        // decays and its spectrum is nothing like one.
        let mut organ = Voice::default();
        organ.algorithm = 31;
        organ.feedback = 0;
        organ.ops = [Op {
            rate: [99, 99, 99, 80],
            level: [99, 99, 99, 0],
            output_level: 80,
            coarse: 1,
            detune: 7,
            ..Op::default()
        }; 6];
        // The path a click takes: through `Shared`, picked up by the renderer.
        if let Ok(mut g) = r.shared.pending_voice.lock() {
            *g = Some(organ);
        }
        let after = sound(&mut r, &mut tx);

        let peak = |v: &[f32]| v.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            peak(&piano) > 0.01 && peak(&after) > 0.01,
            "one of them was silent"
        );
        // Not "the buffers differ" — two renders of the SAME patch differ, on
        // free-running phase alone. The organ sustains and the piano does not,
        // so the end of the block is where they cannot be confused.
        let tail = |v: &[f32]| peak(&v[v.len() * 3 / 4..]);
        assert!(
            tail(&after) > tail(&piano) * 2.0,
            "the patch never reached the audio thread: the tine piano ended at \
             {:.4} and the organ at {:.4}, and an organ does not decay",
            tail(&piano),
            tail(&after)
        );
    }

    /// **The knobs reach the audio, and they reach the FILE.**
    ///
    /// The effects sit on the instrument sum, upstream of the tap, so a take
    /// carries what was heard. Asserted through the real renderer because the
    /// position in the chain is the whole feature: `effects.rs` proves the DSP
    /// against an impulse, and none of that says whether it is wired in.
    #[test]
    fn the_effect_knobs_reach_both_the_device_and_the_take() {
        let tail = |reverb: f32| {
            let (mut r, mut tx, _) = renderer_with_midi(2);
            r.shared.set_effects(crate::effects::Sends {
                reverb,
                ..crate::effects::Sends::default()
            });
            queue(&mut tx, 0, [0x90, 60, 100]);
            let mut out = vec![0.0_f32; 2 * 4096];
            r.render(&mut out, 0, 0);
            queue(&mut tx, 0, [0x80, 60, 0]);
            // Long after the note is released and the built-in has decayed:
            // anything here is a room, because nothing else is left to make it.
            for _ in 0..6 {
                let mut quiet = vec![0.0_f32; 2 * 4096];
                r.render(&mut quiet, 0, 0);
            }
            let mut last = vec![0.0_f32; 2 * 4096];
            r.render(&mut last, 0, 0);
            last.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
        };
        let dry = tail(0.0);
        let wet = tail(1.0);
        assert!(
            wet > dry * 4.0 && wet > 1.0e-5,
            "the reverb knob is not wired in: dry tail {dry:e}, wet tail {wet:e}"
        );
    }

    #[test]
    fn events_are_placed_at_their_own_frame_inside_the_block_they_belong_to() {
        let (mut r, mut tx, _) = renderer_with_midi(2);
        queue(&mut tx, 1_000_000, [0x90, 60, 100]); // 1 ms: frame 48
        queue(&mut tx, 5_000_000, [0x90, 64, 100]); // 5 ms: frame 240
        queue(&mut tx, 20_000_000, [0x90, 67, 100]); // past a 512-frame block
        r.collect_notes(0, 512);
        assert_eq!(r.notes.len(), 2);
        assert_eq!(r.notes[0].offset, 48);
        assert_eq!(r.notes[0].pitch, 60);
        assert_eq!(r.notes[1].offset, 240);
        assert!(
            r.pending.is_some(),
            "the third event belongs to a later block and must be held"
        );
    }

    #[test]
    fn a_callback_larger_than_one_render_block_is_split_into_blocks_and_keeps_every_event() {
        // 4096 frames against a 512-frame max_block is eight `process` calls.
        // `Instance::process` REFUSES anything larger — it returns an error
        // rather than truncating — so a host that hands the whole callback over
        // gets silence and no clue why.
        let (mut r, mut tx, _) = renderer_with_midi(2);
        let now = 1_000_000_000;
        // One note in each of the eight blocks: 512 frames is 10.667 ms.
        for k in 0..8i64 {
            queue(
                &mut tx,
                now + k * 10_666_667 + 1_000_000,
                [0x90, 60 + k as u8, 100],
            );
        }
        let mut out = vec![0.0f32; 4096 * 2];
        r.render(&mut out, now, now);
        assert_eq!(
            r.chunks, 8,
            "4096 frames must become eight 512-frame blocks"
        );
        assert!(
            r.pending.is_none(),
            "an event was left over after 4096 frames"
        );
        assert_eq!(
            r.midi.slots(),
            0,
            "{} events never left the queue",
            r.midi.slots()
        );
    }

    #[test]
    fn each_block_of_a_split_callback_gets_only_the_events_that_belong_to_it() {
        // The trap this catches: computing every offset against the CALLBACK's
        // start rather than the BLOCK's. Then an event 30 ms in has offset 1440,
        // which `Instance::process` places past the end of a 512-frame block —
        // or, worse, a host clamps it to 511 and every note in the second half
        // of the callback lands on the same frame.
        let (mut r, mut tx, _) = renderer_with_midi(2);
        // 15 ms is frame 720: block 1 (frames 512..1024), offset 208.
        queue(&mut tx, 15_000_000, [0x90, 60, 100]);
        let mut out = vec![0.0f32; 2048 * 2];
        r.collect_notes(0, 512);
        assert!(r.notes.is_empty(), "it does not belong to block 0");
        let block1_start = (512.0 / RATE * 1e9) as Nanos;
        r.collect_notes(block1_start, 512);
        assert_eq!(r.notes.len(), 1);
        assert_eq!(r.notes[0].offset, 208);
        assert!(
            r.notes[0].offset < 512,
            "an offset past the block is refused by process"
        );
        let _ = &mut out;
    }

    #[test]
    fn a_callback_smaller_than_one_render_block_still_delivers_its_events() {
        let (mut r, mut tx, _) = renderer_with_midi(2);
        let now = 0;
        queue(&mut tx, now, [0x90, 60, 100]);
        let mut out = vec![0.0f32; 64 * 2];
        r.render(&mut out, now, now);
        assert!(r.midi.slots() == 0 && r.pending.is_none());
    }

    #[test]
    fn an_event_stamped_beyond_the_callback_is_held_and_not_lost() {
        let (mut r, mut tx, _) = renderer_with_midi(2);
        let now = 0;
        // 64 frames is 1.33 ms; this event is 5 ms out.
        queue(&mut tx, 5_000_000, [0x90, 60, 100]);
        let mut out = vec![0.0f32; 64 * 2];
        r.render(&mut out, now, now);
        assert!(
            r.pending.is_some(),
            "a future event must be held, not dropped"
        );
        // And it comes back out on a later callback that covers it.
        r.render(&mut out, 5_000_000, 5_000_000);
        assert!(r.pending.is_none());
    }

    #[test]
    fn a_burst_larger_than_the_event_buffer_is_spread_over_blocks_rather_than_dropped() {
        // Pushing past the capacity would reallocate on the audio thread;
        // dropping would hang a note. It has to be neither.
        let (mut r, mut tx, _) = renderer_with_midi(2);
        for i in 0..300u32 {
            queue(&mut tx, 0, [0x90, 40 + (i % 40) as u8, 100]);
        }
        let mut out = vec![0.0f32; 512 * 2];
        let capacity = r.notes.capacity();
        r.render(&mut out, 0, 0);
        assert_eq!(r.notes.capacity(), capacity, "the note buffer reallocated");
        assert!(
            r.pending.is_some() || r.midi.slots() > 0,
            "the rest must be held"
        );
        for _ in 0..8 {
            r.render(&mut out, 0, 0);
        }
        assert!(r.midi.slots() == 0, "the burst never drained");
    }

    /// **The sustain pedal reaches the instrument**, as a VST3 control.
    ///
    /// This test used to assert the opposite — that pedals were counted and
    /// discarded — because `ivory-host` had no way to send a CC. It does now
    /// (`Instance::process_with_controls`, via `IMidiMapping`), so the
    /// assertion is inverted: the pedal becomes a `Control` on the right
    /// channel with the right value, and `pedal_dropped` counts only what an
    /// instrument published no mapping for.
    #[test]
    fn the_sustain_pedal_becomes_a_control_for_the_instrument() {
        let (mut r, mut tx, shared) = renderer_with_midi(2);
        queue(&mut tx, 0, [0xB3, 64, 127]); // sustain DOWN, channel 3
        queue(&mut tx, 0, [0xB0, 1, 64]); // mod wheel: not a pedal
        let mut out = vec![0.0f32; 128 * 2];
        r.render(&mut out, 0, 0);

        assert_eq!(r.controls.len(), 1, "{:?}", r.controls);
        let c = r.controls[0];
        assert_eq!(c.controller, Control::SUSTAIN);
        assert_eq!(c.value, 127);
        assert_eq!(
            c.channel, 3,
            "the channel is not decoration: the modelled piano publishes a different \
             parameter for each channel's CC64"
        );
        assert_eq!(
            shared.pedal_dropped.load(Ordering::Relaxed),
            0,
            "nothing is dropped merely for being a pedal any more"
        );
    }

    /// All three pedals travel, not just sustain.
    #[test]
    fn sostenuto_and_the_soft_pedal_travel_too() {
        let (mut r, mut tx, _shared) = renderer_with_midi(2);
        queue(&mut tx, 0, [0xB0, 64, 127]);
        queue(&mut tx, 0, [0xB0, 66, 64]);
        queue(&mut tx, 0, [0xB0, 67, 10]);
        let mut out = vec![0.0f32; 128 * 2];
        r.render(&mut out, 0, 0);
        let sent: Vec<i16> = r.controls.iter().map(|c| c.controller).collect();
        assert_eq!(
            sent,
            vec![Control::SUSTAIN, Control::SOSTENUTO, Control::SOFT]
        );
    }

    /// The control buffer is fixed-size and preallocated, like the note buffer:
    /// a `Vec` that grows on the audio thread allocates under a real-time
    /// deadline. Overflow is counted rather than silently dropped.
    #[test]
    fn a_flood_of_pedal_messages_never_reallocates_on_the_audio_thread() {
        let (mut r, mut tx, shared) = renderer_with_midi(2);
        for i in 0..(MAX_CONTROLS_PER_BLOCK * 2) {
            queue(&mut tx, 0, [0xB0, 64, (i % 128) as u8]);
        }
        let capacity = r.controls.capacity();
        let mut out = vec![0.0f32; 128 * 2];
        r.render(&mut out, 0, 0);
        assert_eq!(r.controls.capacity(), capacity, "the buffer reallocated");
        assert_eq!(r.controls.len(), MAX_CONTROLS_PER_BLOCK);
        assert!(
            shared.pedal_dropped.load(Ordering::Relaxed) > 0,
            "the overflow has to be reportable"
        );
    }

    #[test]
    fn midi_is_drained_even_with_no_instrument_loaded() {
        // A queue left to fill up would fire a burst of stale note-ons the
        // instant an instrument appeared, minutes after they were played.
        let (mut r, mut tx, _) = renderer_with_midi(2);
        for i in 0..50u8 {
            queue(&mut tx, 0, [0x90, 40 + i, 100]);
        }
        let mut out = vec![0.0f32; 512 * 2];
        r.render(&mut out, 0, 0);
        assert_eq!(r.midi.slots(), 0);
    }

    #[test]
    fn the_queue_is_drained_once_a_block_and_every_slot_reads_the_same_list() {
        // The trap this exists for: draining the MIDI ring inside the per-slot
        // render loop. It compiles, it looks symmetrical, and it gives each
        // event to whichever slot happened to pop first — the piano plays the
        // C, the pad plays the E, and nobody can work out why a chord came out
        // as an arpeggio across three instruments.
        //
        // With no plugin loaded there is nothing to render, so what is asserted
        // is the shape: after a block, the events are sitting in ONE list that
        // every slot was handed by reference, and the queue behind it is empty.
        let (mut r, mut tx, _) = renderer_with_midi(2);
        queue(&mut tx, 0, [0x90, 60, 100]);
        queue(&mut tx, 0, [0x90, 64, 100]);
        queue(&mut tx, 0, [0x90, 67, 100]);
        let mut out = vec![0.0f32; 512 * 2];
        r.render(&mut out, 0, 0);
        assert_eq!(
            r.notes.len(),
            3,
            "all three notes must survive in one shared list, not be shared out"
        );
        assert_eq!(r.midi.slots(), 0, "and the queue must be empty afterwards");
    }

    #[test]
    fn the_click_reaches_the_speakers_and_not_the_take() {
        // The one default the owner called out. The two mixes are built
        // separately, so this is a property of the render loop rather than of
        // a flag somebody might read in the wrong place.
        let shared = Arc::new(Shared::new());
        let (tap_tx, tap_rx) = RingBuffer::<f32>::new(1 << 16);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
        shared.metro_on.store(true, Ordering::Relaxed);
        shared.metro_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        // The click only speaks while the transport does — the owner's
        // rule — so this render ROLLS.
        shared.rolling.store(true, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        r.beats.was_on = true;
        r.beats.restart(0);

        let mut out = vec![0.0f32; 256 * 2];
        r.render(&mut out, 0, 0);

        let heard = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(
            heard > 0.5,
            "the click never reached the output (peak {heard})"
        );

        let mut tap = RecorderTap {
            rx: tap_rx,
            channels: TAP_CHANNELS,
            sample_rate: 48_000,
            dropped: Arc::clone(&shared),
        };
        let mut captured = Vec::new();
        assert_eq!(tap.drain(&mut captured), 256);
        let bled = captured.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert_eq!(
            bled, 0.0,
            "the click bled into the take, which is the failure nobody notices until playback"
        );
    }

    /// **Three copies of one number.**
    ///
    /// `ivory-ui` declares its own `INPUTS` because it may not reach across
    /// the firewall, and `ivory-record` declares `MAX_PICKS` because that is
    /// where the channels are actually kept. A desk with more strips than the
    /// capture has picks would draw channels nothing can ever fill; fewer, and
    /// an input the user chose would be captured and never heard.
    #[test]
    fn the_desk_has_as_many_inputs_as_the_capture_can_keep() {
        assert_eq!(INPUTS, ivory_record::audio::MAX_PICKS);
        assert_eq!(INPUTS, ivory_ui::recorder::INPUTS);
        assert_eq!(STRIPS, ivory_ui::recorder::STRIPS, "the desks disagree");
        // And every UI strip maps onto a host strip at the same index, which
        // is what makes the mute masks and the send array the same arrays.
        for ui in ivory_ui::recorder::Strip::all() {
            assert_eq!(Strip::from(ui).index(), ui.index(), "{ui:?} moved");
        }
    }

    /// **Monitoring must play what is happening, not what happened.**
    ///
    /// The consumer took at most one output block per output callback, so
    /// anything that built up in the ring stayed there for the rest of the
    /// session. And it always built up: the input stream opens with the band,
    /// the OUTPUT stream starts separately and can be seconds behind it while
    /// a plugin loads, and every input callback in between pushed a block
    /// nobody was draining. The ring is 120 ms deep, so monitoring could sit a
    /// tenth of a second behind the room for ever — at the smallest buffer the
    /// app offers, with nothing on screen to point at.
    ///
    /// This fills the ring the way a late start does and checks two things:
    /// the backlog comes down to what is legitimately in flight, and what is
    /// HEARD is the newest audio rather than the oldest.
    #[test]
    fn a_monitor_that_fell_behind_catches_up_instead_of_lagging() {
        const BLOCK: usize = 128;
        let shared = Arc::new(Shared::new());
        let (tap_tx, _tap_rx) = RingBuffer::<f32>::new(1 << 16);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
        let (mut in_tx, in_rx) = RingBuffer::<f32>::new(1 << 15);
        // A second of mono input nobody drained: the old audio is 0.25 and the
        // NEWEST block — the part that should be heard — is 0.75.
        let stale = BLOCK * 40;
        for _ in 0..stale {
            let _ = in_tx.push(0.25);
        }
        r.monitor = Some(in_rx);
        r.monitor_channels = 1;
        r.monitor_widths = [1, 0, 0, 0];
        r.monitor_block = BLOCK;
        r.monitor_scratch = vec![0.0; 8192];
        // Pick 0 feeds channel 1 — an AUDIO channel by assignment, exactly
        // what the host pushes for the first audio-kind channel.
        shared.pick_strip[0].store(1, Ordering::Relaxed);
        shared.midi_off.store(1 << 1, Ordering::Relaxed);
        shared.channel_gains[1].store(1.0f32.to_bits(), Ordering::Relaxed);
        r.channel_gain[1] = 1.0;
        shared.monitor_on.store(true, Ordering::Relaxed);
        r.room_gain = 1.0;

        // Then the room, one input block per output block, which is what a
        // device that is keeping up actually does.
        let mut out = vec![0.0f32; BLOCK * 2];
        let mut heard = 0.0f32;
        for _ in 0..4 {
            for _ in 0..BLOCK {
                let _ = in_tx.push(0.75);
            }
            out.fill(0.0);
            r.render(&mut out, 0, 0);
            heard = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        }
        // Caught up: a second of stale audio did not have to be played out
        // before the room could be heard.
        assert!(
            (heard - 0.75).abs() < 0.05,
            "after four blocks the monitor is still playing {heard} - the audio \
             from a second ago, which is a backlog nothing drains and latency \
             nobody can see"
        );
        let slipped = shared.monitor_slip.load(Ordering::Relaxed);
        assert!(
            slipped >= stale as u64 / 2,
            "only {slipped} stale frames were dropped, of {stale}"
        );
        // And what is LEFT is what is in flight — one input block and one
        // output block — rather than everything that fits.
        assert!(
            r.monitor.as_ref().is_some_and(|m| m.slots() <= BLOCK * 3),
            "the ring is still holding a backlog"
        );
    }

    /// **The monitor drain takes WHOLE frames even when the scratch cuts it
    /// short.** The clamp read `got.min(scratch.len())`, and the scratch is
    /// `widest * 4096` — which the full width of an eighteen-input interface
    /// does not divide. One clamped read committed a partial frame, and
    /// every read after it mapped the interleaved columns one lane over:
    /// sides swapped, microphones on the wrong strips, the capture tee
    /// taping the rotation, for the rest of the session. A deliberately tiny
    /// scratch stands in for the big one: 8 samples against a three-wide
    /// input is the same misfit, provable in one block.
    #[test]
    fn a_scratch_clamped_monitor_read_keeps_whole_frames() {
        let shared = Arc::new(Shared::new());
        let (tap_tx, _tap_rx) = RingBuffer::<f32>::new(1 << 16);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
        let (mut in_tx, in_rx) = RingBuffer::<f32>::new(1 << 14);
        // A hundred frames of a three-wide input: one mono pick and one
        // stereo pair, laid end to end the way `Picks` lays them.
        let pushed = 100 * 3;
        for _ in 0..pushed {
            let _ = in_tx.push(0.5);
        }
        r.monitor = Some(in_rx);
        r.monitor_channels = 3;
        r.monitor_widths = [1, 2, 0, 0];
        r.monitor_block = 512;
        // NOT a multiple of three, exactly as `widest * 4096` is not a
        // multiple of eighteen.
        r.monitor_scratch = vec![0.0; 8];
        let mut out = vec![0.0f32; 512 * 2];
        r.render(&mut out, 0, 0);
        let left = r.monitor.as_ref().map_or(0, Consumer::slots);
        let consumed = pushed - left;
        assert!(consumed > 0, "nothing was drained at all");
        assert_eq!(
            consumed % 3,
            0,
            "the drain committed a partial frame ({consumed} samples of a \
             3-wide input): every read from here on is rotated one lane"
        );
    }

    /// **The take is the desk. The room is the desk minus what nobody asked
    /// to hear.**
    ///
    /// Monitoring used to decide both, and the take was the casualty: a
    /// microphone monitored through the interface's own hardware — the
    /// ordinary way anybody records one, because it is the only way with no
    /// latency — is not monitored here, and produced a file with no microphone
    /// in it. Now mute decides the file and monitoring decides the speakers,
    /// which is why this asserts on BOTH sides of the same render.
    #[test]
    fn an_unmonitored_input_is_in_the_take_and_out_of_the_room() {
        for monitored in [false, true] {
            let shared = Arc::new(Shared::new());
            let (tap_tx, tap_rx) = RingBuffer::<f32>::new(1 << 16);
            let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
            // The live input, one mono channel of a steady half.
            let (mut in_tx, in_rx) = RingBuffer::<f32>::new(1 << 14);
            for _ in 0..4096 {
                let _ = in_tx.push(0.5);
            }
            r.monitor = Some(in_rx);
            r.monitor_channels = 1;
            r.monitor_widths = [1, 0, 0, 0];
            r.monitor_scratch = vec![0.0; 8192];
            // At the fader's destination rather than on its way there: the
            // slews are per sample and this test is about routing. The pick
            // feeds channel 1, an AUDIO channel by assignment.
            shared.pick_strip[0].store(1, Ordering::Relaxed);
            shared.midi_off.store(1 << 1, Ordering::Relaxed);
            shared.channel_gains[1].store(1.0f32.to_bits(), Ordering::Relaxed);
            r.channel_gain[1] = 1.0;
            r.monitor_gain[0] = 1.0;
            shared.monitor_on.store(monitored, Ordering::Relaxed);
            r.room_gain = if monitored { 1.0 } else { 0.0 };

            let mut out = vec![0.0f32; 256 * 2];
            r.render(&mut out, 0, 0);

            let mut tap = RecorderTap {
                rx: tap_rx,
                channels: TAP_CHANNELS,
                sample_rate: 48_000,
                dropped: Arc::clone(&shared),
            };
            let mut captured = Vec::new();
            tap.drain(&mut captured);
            let taped = captured.iter().fold(0.0f32, |a, s| a.max(s.abs()));
            let room = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));

            assert!(
                taped > 0.4,
                "the input is missing from the take (peak {taped}) with \
                 monitoring {monitored} - a take is the desk and the strip was \
                 not muted"
            );
            if monitored {
                assert!(
                    room > 0.4,
                    "monitoring is on and the speakers got nothing (peak {room})"
                );
            } else {
                assert!(
                    room < 1.0e-3,
                    "the input reached the speakers (peak {room}) with \
                     monitoring off, which is the feedback this switch exists \
                     to prevent"
                );
            }
        }
    }

    #[test]
    fn metronome_in_take_puts_the_click_into_the_tap_as_well() {
        let shared = Arc::new(Shared::new());
        let (tap_tx, tap_rx) = RingBuffer::<f32>::new(1 << 16);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
        shared.metro_on.store(true, Ordering::Relaxed);
        shared.metro_in_take.store(true, Ordering::Relaxed);
        shared.metro_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        // The click only speaks while the transport does, since the owner's
        // rule — so this take ROLLS.
        shared.rolling.store(true, Ordering::Relaxed);
        shared.transport_req.store(1, Ordering::Release);
        r.beats.was_on = true;
        r.beats.restart(0);

        let mut out = vec![0.0f32; 256 * 2];
        r.render(&mut out, 0, 0);

        let mut tap = RecorderTap {
            rx: tap_rx,
            channels: TAP_CHANNELS,
            sample_rate: 48_000,
            dropped: Arc::clone(&shared),
        };
        let mut captured = Vec::new();
        tap.drain(&mut captured);
        let bled = captured.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(
            bled > 0.5,
            "asked for a guide click and got none (peak {bled})"
        );
    }

    #[test]
    fn a_channel_fader_reaches_zero_without_a_step_discontinuity() {
        // A gain that jumps is a click, which is the one thing the metronome is
        // supposed to have a monopoly on. The smoother runs whether or not the
        // channel has anything on it, so that a channel filled a minute after
        // its fader moved does not start at the gain the fader used to have
        // and jump to the one it has now.
        let shared = Arc::new(Shared::new());
        let (tap_tx, _rx) = RingBuffer::<f32>::new(1 << 16);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
        r.gain_coeff = gain_coefficient(RATE);
        r.channel_gain = [1.0; CHANNELS];
        shared.set_channel_gain(0, 0.0);
        let mut out = vec![0.0f32; 8 * 2];
        r.render(&mut out, 0, 0);
        assert!(
            r.channel_gain[0] > 0.9,
            "a 10 ms smoother must not cross most of its range in 8 frames \
             (it reached {})",
            r.channel_gain[0]
        );
        // 600 blocks of 8 frames is 4800 frames, 100 ms, ten time constants.
        for _ in 0..600 {
            r.render(&mut out, 0, 0);
        }
        assert!(
            r.channel_gain[0] < 0.01,
            "the smoother never arrived ({})",
            r.channel_gain[0]
        );
        assert_eq!(
            (r.channel_gain[1], r.channel_gain[2]),
            (1.0, 1.0),
            "one fader moved and took its neighbours with it"
        );
    }

    #[test]
    fn a_gain_written_to_a_channel_that_does_not_exist_is_ignored_rather_than_panicking() {
        // Reached from UI code, where this app's panic hook turns a panic into a
        // dialog and `exit(1)`. A menu that got its arithmetic wrong should
        // misbehave, not end the session.
        let shared = Shared::new();
        shared.set_channel_gain(CHANNELS, 0.25);
        shared.set_channel_gain(usize::MAX, 0.25);
        for cell in &shared.channel_gains {
            assert_eq!(
                Shared::f32_of(cell),
                1.0,
                "a real channel was written instead"
            );
        }
        shared.set_channel_gain(CHANNELS - 1, 0.25);
        assert_eq!(Shared::f32_of(&shared.channel_gains[CHANNELS - 1]), 0.25);
    }

    #[test]
    fn the_meter_reads_the_device_mix_and_never_misses_a_peak_between_ui_frames() {
        let shared = Arc::new(Shared::new());
        // A loud callback and then a quiet one. A meter that stored rather than
        // max-ed would report the quiet one.
        shared.peak_l.fetch_max(0.9f32.to_bits(), Ordering::Relaxed);
        shared.peak_l.fetch_max(0.1f32.to_bits(), Ordering::Relaxed);
        assert_eq!(f32::from_bits(shared.peak_l.load(Ordering::Relaxed)), 0.9);
        // And reading clears it, so the next window starts empty.
        assert_eq!(
            f32::from_bits(shared.peak_l.swap(0, Ordering::Relaxed)),
            0.9
        );
        assert_eq!(f32::from_bits(shared.peak_l.load(Ordering::Relaxed)), 0.0);
    }

    #[test]
    fn a_count_in_publishes_its_beat_and_its_downbeat_from_the_audio_thread() {
        let shared = Arc::new(Shared::new());
        let (tap_tx, _rx) = RingBuffer::<f32>::new(1 << 16);
        let mut r = test_renderer(Arc::clone(&shared), tap_tx, 2);
        // Two beats at 240 bpm: 12000 frames apart at 48 kHz.
        shared.bpm.store(240.0f64.to_bits(), Ordering::Relaxed);
        shared.count_in_beats.store(2, Ordering::Relaxed);
        shared.count_in_req.fetch_add(1, Ordering::Relaxed);

        let mut out = vec![0.0f32; 4_096 * 2];
        r.render(&mut out, 0, 0);
        assert_eq!(shared.beat_now.load(Ordering::Relaxed), 1);
        assert!(!shared.count_in_done.load(Ordering::Relaxed));

        // 36000 frames covers beat 2 (12000) and the downbeat (24000).
        for _ in 0..8 {
            r.render(&mut out, 0, 0);
        }
        assert!(shared.count_in_done.load(Ordering::Relaxed));
        assert_ne!(shared.downbeat_ns.load(Ordering::Relaxed), i64::MIN);
        assert_eq!(
            shared.beat_now.load(Ordering::Relaxed),
            0,
            "the countdown must clear when it is over"
        );
    }

    #[test]
    fn the_gain_coefficient_is_a_real_time_constant_and_survives_a_nonsense_rate() {
        // 10 ms at 48 kHz is about 1/480 per frame.
        let c = gain_coefficient(48_000.0);
        assert!((c - 1.0 / 480.0).abs() < 1e-4, "got {c}");
        assert_eq!(gain_coefficient(0.0), 1.0);
        assert_eq!(gain_coefficient(f64::NAN), 1.0);
    }

    #[test]
    fn a_gain_set_to_nonsense_is_silence_rather_than_a_nan_in_the_take() {
        assert_eq!(sane_gain(f32::NAN), 0.0);
        assert_eq!(sane_gain(f32::INFINITY), 0.0);
        assert_eq!(sane_gain(-1.0), 0.0);
        assert_eq!(sane_gain(100.0), 8.0);
        assert_eq!(sane_gain(0.5), 0.5);
    }

    #[test]
    fn asio_asks_for_the_plain_period_and_never_multiplies() {
        let range = |min, max| cpal::SupportedBufferSize::Range { min, max };
        // **The exact numbers from the canary's crash.** Windows multiplies
        // by BUFFER_PERIODS=2 for WASAPI, so the output asked ASIO for
        // Fixed(512) while the input side asked plain — the driver re-granted
        // 256 when the input joined, and cpal's output callback divided its
        // 512-frame buffer by the new 256 into FOUR channels on a stereo
        // interface: "len is 2 but the index is 2", a startup crash. Under
        // ASIO the request must be the plain period, identical to the input
        // side's translation of the same setting.
        let (size, frames) = pick_buffer(&range(16, 1024), Some(256), true);
        assert!(
            matches!(size, cpal::BufferSize::Fixed(256)),
            "asio asked for {size:?}; multiplying is the crash"
        );
        assert_eq!(frames, Some(256));

        // No stated choice: Default, the driver's own preferred size — the
        // same place the input's `None` lands, which is the equality that
        // keeps the two directions' grants identical.
        let (size, frames) = pick_buffer(&range(16, 1024), None, true);
        assert!(matches!(size, cpal::BufferSize::Default));
        assert_eq!(frames, None);

        // A choice outside the device's range is clamped, not a build error.
        let (size, frames) = pick_buffer(&range(64, 128), Some(256), true);
        assert!(matches!(size, cpal::BufferSize::Fixed(128)));
        assert_eq!(frames, Some(128));
    }

    #[test]
    fn a_device_that_will_not_say_what_buffer_sizes_it_takes_gets_its_own_choice() {
        // `BufferSize::Fixed` outside the supported range is a build error, not
        // a clamp, so guessing is worse than deferring.
        let (size, frames) = pick_buffer(&cpal::SupportedBufferSize::Unknown, None, false);
        assert!(matches!(size, cpal::BufferSize::Default));
        assert_eq!(frames, None);
        // **The RING is what is asked for and the PERIOD is what is
        // reported**, and on Linux those differ by four — see
        // `BUFFER_PERIODS`. Written in terms of the constant rather than in
        // literal frames, because a test that hard-codes 512 passes on the
        // machine it was written on and fails on the one the fix is for.
        let range = |min, max| cpal::SupportedBufferSize::Range { min, max };
        let ring_of = |b: cpal::BufferSize| match b {
            cpal::BufferSize::Fixed(n) => n,
            cpal::BufferSize::Default => 0,
        };

        // Roomy device: the ring is the wanted period times the count.
        let (size, frames) = pick_buffer(&range(16, 8192), Some(256), false);
        assert_eq!(ring_of(size), 256 * BUFFER_PERIODS);
        assert_eq!(frames, Some(256), "the period is not what was asked for");

        // Clamped up to the minimum, and the period follows the ring.
        let (size, frames) = pick_buffer(&range(4096, 8192), Some(256), false);
        assert_eq!(ring_of(size), 4096);
        assert_eq!(frames, Some(4096 / BUFFER_PERIODS));

        // And down to the maximum.
        let (size, frames) = pick_buffer(&range(16, 64), Some(256), false);
        assert_eq!(ring_of(size), 64);
        assert_eq!(frames, Some(64 / BUFFER_PERIODS));

        // A period is never reported as zero, whatever the device says: it is
        // divided into and displayed.
        let (_, frames) = pick_buffer(&range(1, 2), Some(1), false);
        assert!(frames.is_some_and(|f| f >= 1));
    }

    // ── with a real device and a real plugin ───────────────────────────────

    #[test]
    #[ignore = "needs an audio output device; run with \
                `cargo test -p ivory -- --ignored the_engine_opens`"]
    fn the_engine_opens_an_output_and_runs_the_metronome_with_no_plugin_loaded() {
        // The claim this proves is the one the whole restructure is about: the
        // click does not wait for a VST3.
        let mut engine = Engine::start(None).expect("an audio output");
        assert!(engine.is_running());
        assert!(
            (0..=STRIPS).all(|st| (0..INSERTS).all(|b| engine.insert(st, b).is_none())),
            "a fresh engine has something loaded"
        );
        engine.set_metronome_enabled(true);
        engine.set_tempo(120.0);
        std::thread::sleep(Duration::from_millis(600));
        assert!(engine.callbacks() > 0, "the device never called back");
        assert!(
            engine.meters().peak() > 0.0,
            "the metronome made no sound without a plugin"
        );
        assert!(engine.take_recorder_tap().is_some());
        assert!(
            engine.take_recorder_tap().is_none(),
            "the tap is taken once"
        );
    }

    #[test]
    #[ignore = "needs an audio output device; run with \
                `cargo test -p ivory -- --ignored the_count_in --nocapture`"]
    fn the_count_in_runs_in_beats_on_a_real_device_and_reports_its_own_latency() {
        // The numbers this prints are the only measured ones in the file: the
        // buffer the device actually granted, and the backend's own
        // callback-to-playback delta. Everything else about latency in this
        // product is an assumption, and RECORDER-PLAN §3a is explicit that the
        // difference matters.
        let engine = Engine::start(None).expect("an audio output");
        println!("device:   {}", engine.output().device);
        println!("buffer:   {}", engine.output().latency_line());

        engine.set_metronome_gain(0.5);
        engine.start_count_in(4, 120.0);
        let began = Instant::now();
        let mut seen: Vec<(u32, Duration)> = Vec::new();
        while !engine.count_in_done() && began.elapsed() < Duration::from_secs(10) {
            if let Some(b) = engine.metronome_beat() {
                if seen.last().map(|(n, _)| *n) != Some(b) {
                    seen.push((b, began.elapsed()));
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        println!(
            "delay:    {:.2} ms measured callback-to-playback",
            engine.output_delay_ns() as f64 / 1e6
        );
        for (beat, at) in &seen {
            println!("  beat {beat} at {:.0} ms", at.as_secs_f64() * 1e3);
        }
        assert_eq!(
            seen.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert!(engine.count_in_done());
        // Four beats at 120 bpm plus the downbeat is 2.5 s of clicks.
        let total = began.elapsed().as_secs_f64();
        assert!(
            (2.0..3.2).contains(&total),
            "a four-beat count-in at 120 bpm took {total:.2} s"
        );
        assert!(engine.count_in_downbeat_ns().is_some());
    }

    #[test]
    #[ignore = "needs an audio output device and a real VST3 instrument; run with \
                `cargo test -p ivory -- --ignored a_bays_state --nocapture`"]
    fn a_bays_state_can_be_saved_while_it_plays_and_handed_back_to_the_next_load() {
        // The engine's half of the persistence story: the instance is inside
        // the audio callback the whole time, so this exercises the second
        // reference to its processor rather than any path that stops the audio.
        // That the RESTORED SOUND is right is proved in `ivory-host`'s
        // `instance.rs`, where a rendered probe can be measured; here the claim
        // is that the bytes can be got at all while the stream runs, and that a
        // bad blob costs the preset rather than the piano.
        let bundle = piano();
        let mut engine = Engine::start(None).expect("an audio output");
        engine.load_insert(0, 0, Some(&bundle), None).expect("load");
        assert!(engine.insert_state_error(0, 0).is_none());

        let now = engine.timebase().now();
        engine.send_midi(now, &[0x90, 60, 100]);
        let bytes = engine
            .save_insert_state(0, 0)
            .expect("a loaded bay has state to save");
        engine.send_midi(engine.timebase().now(), &[0x80, 60, 64]);
        println!("bay 0 state: {} bytes", bytes.len());
        assert!(bytes.len() > 18, "only the container came back");
        assert!(engine.is_running(), "saving state stopped the stream");
        assert!(engine.fault().is_none(), "{:?}", engine.fault());

        // An empty bay has nothing to save, and neither has one that does not
        // exist. Both are `None` rather than an error, because the caller's
        // only move either way is to write nothing.
        assert!(engine.save_insert_state(0, 1).is_none());
        assert!(engine.save_insert_state(STRIPS + 1, 0).is_none());

        engine
            .load_insert(0, 0, Some(&bundle), Some(&bytes))
            .expect("reload with state");
        assert!(
            engine.insert_state_error(0, 0).is_none(),
            "a blob this engine just wrote was refused: {:?}",
            engine.insert_state_error(0, 0)
        );

        // And the case that will actually happen: a settings file that has been
        // truncated, edited, or written by a different plugin. The instrument
        // must still load.
        let mut broken = bytes.clone();
        broken.truncate(broken.len() / 2);
        engine
            .load_insert(0, 0, Some(&bundle), Some(&broken))
            .expect("a corrupt blob must not stop the instrument loading");
        assert!(
            engine.insert_state_error(0, 0).is_some(),
            "a truncated blob was silently accepted, which is how 'it keeps \
             forgetting my piano' becomes unreportable"
        );
        assert!(engine.insert(0, 0).is_some(), "the instrument did not load");
        // ...and the error does not outlive the bay.
        let _ = engine.load_insert(0, 0, None, None);
        assert!(engine.insert_state_error(0, 0).is_none());
    }

    #[test]
    #[ignore = "needs a real VST3 instrument installed; run with \
                `cargo test -p ivory -- --ignored a_real_instrument`"]
    fn a_real_instrument_is_heard_and_can_be_swapped_while_the_stream_runs() {
        let bundle = piano();
        let mut engine = Engine::start(None).expect("an audio output");
        let loaded = engine
            .load_insert(0, 0, Some(&bundle), None)
            .expect("load")
            .expect("a real bundle in a real bay");
        assert_eq!(engine.insert(0, 0), Some(&loaded));

        let now = engine.timebase().now();
        engine.send_midi(now, &[0x90, 60, 100]);
        engine.send_midi(now, &[0x90, 64, 100]);
        engine.send_midi(now, &[0x90, 67, 100]);
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            engine.meters().peak() > 0.001,
            "the instrument was not heard"
        );
        engine.send_midi(engine.timebase().now(), &[0x80, 60, 64]);
        engine.send_midi(engine.timebase().now(), &[0x80, 64, 64]);
        engine.send_midi(engine.timebase().now(), &[0x80, 67, 64]);

        // The swap: the stream keeps running and the tap keeps its width.
        let mut tap = engine.take_recorder_tap().expect("a tap");
        assert_eq!(tap.channels(), TAP_CHANNELS);
        let _ = engine.load_insert(0, 0, None, None);
        assert!(engine.insert(0, 0).is_none());
        engine
            .load_insert(0, 0, Some(&bundle), None)
            .expect("reload");
        assert_eq!(
            tap.channels(),
            TAP_CHANNELS,
            "the tap changed width mid-take"
        );
        assert!(engine.is_running());
        let mut out = Vec::new();
        tap.drain(&mut out);
        assert!(engine.fault().is_none(), "{:?}", engine.fault());
    }

    #[test]
    #[ignore = "needs a real VST3 instrument installed; run with \
                `cargo test -p ivory -- --ignored two_channels --nocapture`"]
    fn two_channels_holding_the_same_instrument_are_twice_as_loud_as_one() {
        // The claim of the whole feature, measured rather than asserted: the
        // same instrument playing the same notes twice sums coherently, so the
        // peak must double. It is the only way to tell "both channels got the
        // notes" from "one got them and the other is silent", which every
        // structural test in this file would happily pass.
        let bundle = piano();
        let mut engine = Engine::start(None).expect("an audio output");
        engine
            .load_insert(0, 0, Some(&bundle), None)
            .expect("channel 1");
        for ch in 0..CHANNELS {
            engine.set_channel_gain(ch, 0.4);
        }
        let one = chord_peak(&engine);
        engine
            .load_insert(1, 0, Some(&bundle), None)
            .expect("channel 2");
        let two = chord_peak(&engine);
        println!(
            "one channel {one:.4}, two {two:.4}, ratio {:.2}x",
            two / one
        );
        assert!(one > 0.001, "the first instrument was never heard ({one})");
        // Wide bounds on purpose: a real piano's peak is a transient and the
        // two instances are separately seeded. 1.5x rules out "the second
        // channel is silent" (1.0x) and 2.5x rules out "something is summing
        // twice".
        assert!(
            (1.5..2.5).contains(&(two / one)),
            "two channels measured {:.2}x one, which is neither a layered pair \
             (2.0x) nor a channel that heard nothing (1.0x)",
            two / one
        );
        assert!(engine.fault().is_none(), "{:?}", engine.fault());
    }

    #[test]
    #[ignore = "needs a real VST3 instrument installed; run with \
                `cargo test -p ivory -- --ignored unloading_one_channel`"]
    fn unloading_one_channel_leaves_the_others_playing() {
        // The per-bay handoff in one test: the rings channel 2's bay uses are
        // not the rings channel 1's uses, so retiring one instrument cannot
        // take the other with it.
        let bundle = piano();
        let mut engine = Engine::start(None).expect("an audio output");
        engine
            .load_insert(0, 0, Some(&bundle), None)
            .expect("channel 1");
        engine
            .load_insert(1, 0, Some(&bundle), None)
            .expect("channel 2");
        for ch in 0..CHANNELS {
            engine.set_channel_gain(ch, 0.4);
        }
        assert!(chord_peak(&engine) > 0.001);

        let _ = engine.load_insert(1, 0, None, None);
        assert!(engine.insert(1, 0).is_none());
        assert!(engine.insert(0, 0).is_some(), "the wrong bay was retired");
        let alone = chord_peak(&engine);
        assert!(
            alone > 0.001,
            "unloading channel 2 silenced channel 1 as well ({alone})"
        );
        assert!(engine.is_running());
        assert!(engine.fault().is_none(), "{:?}", engine.fault());
    }

    #[test]
    #[ignore = "needs a real VST3 instrument installed; run with \
                `cargo test -p ivory -- --ignored a_bay_that_does_not_exist`"]
    fn a_bay_that_does_not_exist_is_a_no_op_and_never_a_panic() {
        // Every one of these is reachable from a menu, and a panic here goes
        // through the app's hook to a dialog and `exit(1)`.
        //
        // `RACK_SEATS`, not `STRIPS + 1`: that seat is pad 0's FX bay now,
        // a bay that very much exists — the first seat that does not is the
        // one past the whole map.
        let bundle = piano();
        let mut engine = Engine::start(None).expect("an audio output");
        assert!(matches!(
            engine.load_insert(RACK_SEATS, 0, Some(&bundle), None),
            Ok(None)
        ));
        assert!(matches!(
            engine.load_insert(0, INSERTS, Some(&bundle), None),
            Ok(None)
        ));
        assert!(engine.open_insert_editor(RACK_SEATS, 0).is_ok());
        let _ = engine.load_insert(RACK_SEATS, 0, None, None);
        engine.close_insert_editor(RACK_SEATS, 0);
        engine.set_channel_gain(CHANNELS, 0.5);
        assert!(!engine.insert_has_editor(RACK_SEATS, 0));
        assert!(!engine.insert_editor_open(RACK_SEATS, 0));
        assert!(engine.insert(RACK_SEATS, 0).is_none());
        assert!(engine.is_running(), "a bad bay number stopped the stream");
    }

    /// The one instrument every one of these tests needs, found the way the
    /// probe finds it.
    fn piano() -> PathBuf {
        let bundles = ivory_host::discover();
        let Some(bundle) = bundles.into_iter().find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase().contains("piano"))
                .unwrap_or(false)
        }) else {
            panic!("no VST3 matching the modelled piano; this test needs one installed");
        };
        bundle
    }

    /// Play a chord, hold it for half a second, and report the loudest thing the
    /// device mix saw. Releases the notes afterwards, so the next call measures
    /// its own chord rather than the last one still ringing.
    fn chord_peak(engine: &Engine) -> f32 {
        // Read once to clear the peak: it is read-and-clear by design, and a
        // stale peak from the previous chord would be this one's answer.
        engine.meters();
        let now = engine.timebase().now();
        for pitch in [60u8, 64, 67] {
            engine.send_midi(now, &[0x90, pitch, 100]);
        }
        let mut peak = 0.0f32;
        let until = Instant::now() + Duration::from_millis(700);
        while Instant::now() < until {
            peak = peak.max(engine.meters().peak());
            std::thread::sleep(Duration::from_millis(8));
        }
        for pitch in [60u8, 64, 67] {
            engine.send_midi(engine.timebase().now(), &[0x80, pitch, 64]);
        }
        // Let the release finish, or the next chord starts on top of this one's
        // tail and measures both.
        std::thread::sleep(Duration::from_millis(800));
        peak
    }
}
