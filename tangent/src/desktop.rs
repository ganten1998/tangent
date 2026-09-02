//! The desktop half of the app: an eframe window, and a real MIDI device.
//!
//! Everything that draws lives in `tangent_ui::app::TangentApp`, which the VST3
//! build also uses. What is left here is exactly the three things a standalone
//! has and a plugin editor does not — an `eframe::App` impl, a `midir`
//! connection, and a command line — plus the `Caps` value that says so.
//!
//! The wrapper struct is not ceremony. `impl eframe::App for TangentApp` cannot
//! be written from this crate: the orphan rule forbids implementing a foreign
//! trait for a foreign type. That is a feature here rather than an obstacle —
//! it is the compiler stating that eframe is this crate's business and not the
//! shared crate's.

use crate::midi;
use tangent_ui::app::TangentApp;
use tangent_ui::host::Caps;
use tangent_ui::midi_event::MidiEvent;
use tangent_ui::ports::MidiPorts;
use tangent_ui::settings::Settings;
use std::sync::{mpsc, Arc};
// Only the recorder's camera-permission latch uses it, and a Minimal build has
// no recorder — so an ungated import is an unused-import warning there.
#[cfg(feature = "recorder")]
use std::sync::Mutex;

/// A real MIDI input, opened with `midir`.
///
/// Holds the egui context so the callback thread can wake the UI on every
/// event: repaints are event-driven rather than busy-looped (D-UI-3). A plugin
/// needs no equivalent — the host calls `process()` and then the editor.
pub struct DeviceMidi {
    ctx: egui::Context,
    conn: Option<midi::MidiConnection>,
    /// The recorder's raw feed. Owned here rather than by the recorder, and
    /// **created once per app** rather than once per connection, because midir
    /// seals a callback's captured state the moment the port opens — see
    /// `midi::RawMidiTap`. Every connection gets a clone of this `Arc`, so
    /// switching ports keeps the history the tick-0 controller snapshot needs.
    tap: Arc<midi::RawMidiTap>,
    /// The hardware fast lane — the midir callback's direct line to the
    /// audio callback. App-lifetime for the tap's own sealed-closure reason;
    /// each engine seats its ring here at start. See `midi::MidiFastLane`.
    lane: Arc<midi::MidiFastLane>,
    /// The app's single time origin. See [`DeviceMidi::timebase`].
    #[cfg(feature = "recorder")]
    timebase: tangent_record::audio::Timebase,
}

impl DeviceMidi {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            ctx,
            conn: None,
            // ~10 minutes of dense playing before anything is shed, which is
            // far more than the pre-roll needs and cheap: a few hundred KB.
            tap: Arc::new(midi::RawMidiTap::new(60_000)),
            lane: Arc::new(midi::MidiFastLane::new()),
            #[cfg(feature = "recorder")]
            timebase: tangent_record::audio::Timebase::new(),
        }
    }

    /// The startup priority chain (spec §10). Silent on failure by design: the
    /// app runs without MIDI rather than opening a dialog nobody asked for.
    pub fn auto_connect(&mut self, tx: mpsc::Sender<MidiEvent>) {
        self.conn = midi::auto_connect(
            tx,
            self.ctx.clone(),
            Arc::clone(&self.tap),
            Arc::clone(&self.lane),
        );
    }

    /// The fast lane, for the desktop to seat each engine's ring in.
    pub fn lane(&self) -> Arc<midi::MidiFastLane> {
        Arc::clone(&self.lane)
    }

    /// The raw feed, for the recorder.
    #[cfg(feature = "recorder")]
    pub fn tap(&self) -> Arc<midi::RawMidiTap> {
        Arc::clone(&self.tap)
    }

    /// The one epoch every stamp in the app is measured against.
    ///
    /// Owned here rather than by the session because the MIDI tap starts
    /// stamping the moment a port opens, which is before any recorder exists.
    /// One `Timebase`, created once, shared: two of them would put the MIDI and
    /// the audio in different worlds and every take would carry a constant
    /// offset nobody could account for.
    #[cfg(feature = "recorder")]
    pub fn timebase(&self) -> tangent_record::audio::Timebase {
        self.timebase
    }
}

impl MidiPorts for DeviceMidi {
    fn list(&self) -> Vec<String> {
        midi::list_port_names()
    }

    fn auto_connect(&mut self, tx: mpsc::Sender<MidiEvent>) {
        DeviceMidi::auto_connect(self, tx);
    }

    fn connect(&mut self, name: &str, tx: mpsc::Sender<MidiEvent>) -> Result<(), String> {
        // Close the old port FIRST (parity): some drivers refuse a second open
        // of the same device, so holding both across the switch fails on the
        // machines that matter and works on the ones that do not.
        self.conn = None;
        self.conn = Some(midi::connect_by_name(
            name,
            tx,
            self.ctx.clone(),
            Arc::clone(&self.tap),
            Arc::clone(&self.lane),
        )?);
        Ok(())
    }

    fn current(&self) -> Option<String> {
        self.conn.as_ref().map(|c| c.port_name.clone())
    }
}

/// The recorder, and everything that has to happen around a frame to drive it.
///
/// Absent from a Minimal build, where `tangent-record` is not linked at all.
#[cfg(feature = "recorder")]
struct Recorder {
    session: crate::record::Session,
    /// The playhead's one owner. See `transport.rs` for the whole argument;
    /// the short form is that the engine is dropped and rebuilt whenever the
    /// audio path changes, and a position that lived only in the engine
    /// returned to 0:00 mid-session with nothing host-side to restore it.
    transport: crate::transport::Transport,
    /// The monitor output: the hosted instrument and the click, summed in one
    /// callback.
    ///
    /// Its life is tied to the BAND rather than to the app, for the same reason
    /// the input stream's is: an output device held open by a chord display
    /// nobody is recording with is a device another app cannot get exclusive
    /// access to. `None` when the band is closed, or when the device would not
    /// open at all.
    engine: Option<crate::instrument::Engine>,
    /// Why the output device would not open, if it would not.
    engine_error: Option<String>,
    /// Notices the output going quiet without an error — the ASIO
    /// dead-driver case. Reset whenever the engine is (re)made.
    engine_watch: crate::instrument::StallWatch,
    /// The auto recheck — the owner's ask after the yank test. Alive only
    /// while the engine is down; see `Reprobe`.
    reprobe: Option<Reprobe>,
    /// The ReaStream sender, alive while the engine is and streaming while the
    /// setting is on. Dropped when the engine restarts so the next one hands it
    /// a fresh tap; see `reconcile_reastream`.
    reastream: Option<tangent_record::reastream::ReaStreamSender>,
    /// What is loaded in each insert slot, flat like `Settings::strip_inserts`.
    /// Remembered whether or not the load worked, so a plugin that refuses is
    /// asked once — `load_insert` blocks for seconds on a sampler, and a
    /// plugin that refuses must not be retried sixty times a second.
    /// What each BAY has loaded, flat: `seat * INSERTS + bay`.
    ///
    /// **`INSERT_SPACE`, which is EVERY seat — not the strips plus the first
    /// pad block.** All five places that size this said
    /// `(STRIPS + 1 + DRUM_PADS) * INSERTS`, which comes to 444 — and
    /// `MASTER_EXTRA_SEAT` IS seat 148, whose bays are 444, 445 and 446. So
    /// the reconcile walked `0..444`, never reached them, and a plugin in the
    /// master's SECOND rack was never handed to the engine: silent in
    /// playback, and present in the export, because the settings and the
    /// exporter both size themselves from the right constant.
    ///
    /// That divergence is the worst possible symptom. It reads as a broken
    /// exporter, or a broken plugin, and never as a short array.
    ///
    /// `open_insert_picker` carries the same story one layer up: the same
    /// seat was unreachable there and was fixed only there, which is how this
    /// survived. Deriving the length from the constant the rest of the app
    /// already uses is what stops there being a third time.
    inserts_loaded: Vec<Option<String>>,
    /// The drumpad kit could not be handed to the audio thread last frame
    /// (stalled callback, absent engine) — try again this one.
    pads_retry: bool,
    audio: crate::devices::Shared,
    camera: crate::devices::Shared,
    /// Why enumeration failed last time, so the band can say "permission" and
    /// not "no cameras" — two problems with completely different fixes.
    camera_denied: Arc<Mutex<Option<String>>>,
    /// A camera open that has been announced but not yet performed.
    ///
    /// `open_camera` blocks the calling thread for 63 ms on a built-in camera
    /// and a measured 1.9-3.9 s on an external UVC one, and it runs on the UI
    /// thread. Doing it the moment the selection goes stale freezes the window
    /// for up to four seconds with the PREVIOUS frame still painted and nothing
    /// saying why. So the intent is recorded on one frame — which paints
    /// "starting the camera…" and asks for an immediate repaint — and the
    /// blocking call happens on the next.
    camera_opening: bool,
    /// When the camera was first noticed to be running-but-silent, so the
    /// warning waits a few seconds rather than firing on frame one.
    camera_silent_since: Option<std::time::Instant>,
    /// The uploaded preview frame.
    ///
    /// Kept between frames on purpose. A 30 fps camera in a 60 fps window
    /// delivers nothing on half the frames, and a preview that cleared itself
    /// on a `None` would strobe black at 30 Hz.
    preview: Option<egui::TextureHandle>,
    preview_px: egui::Vec2,
    /// Whether the band was open on the previous frame, so opening and closing
    /// the input happens on the EDGE rather than being re-decided sixty times a
    /// second.
    band_was_open: bool,
    /// Recomputed on a timer rather than every frame: `statvfs` is a syscall,
    /// and the answer changes by megabytes, not by pixels.
    disk_checked_at: Option<std::time::Instant>,
    disk_bytes: Option<u64>,
    /// The take's video, while one is being filmed.
    video: Option<TakeVideo>,
    /// Whether this take has already tried to start filming.
    ///
    /// Without it a take whose video was REFUSED — no camera, no GPU, a file
    /// that would not open — retries on every window frame, and rewrites the
    /// same error sixty times a second over whatever else the band was saying.
    ///
    video_tried: bool,
    /// A live-input tap waiting for an engine to play it.
    ///
    /// The tap is made when the INPUT opens and can only be handed to an
    /// ENGINE, and the two are opened by different edges — so it is held here
    /// rather than dropped when the second one is not there yet. See
    /// `push_monitor_settings`, which is where it gets handed over.
    pending_monitor: Option<(
        rtrb::Consumer<f32>,
        u16,
        [u8; crate::instrument::INPUTS],
        u32,
        tangent_record::audio::Picks,
    )>,
    /// What each input strip is called, as the picker names it. Empty for one
    /// nobody has filled.
    ///
    /// Kept here rather than read back out of the app, because the host is
    /// what knows: which inputs are open is a fact about the device and the
    /// selection, and `tangent-ui` can reach neither.
    input_names: [String; crate::instrument::INPUTS],
    /// What the LAST take is worth saying about it, if anything.
    ///
    /// **Its own field, not `engine_error`.** The video path used to put this
    /// in the audio engine's error slot, which nothing in the video path ever
    /// cleared — so "frames were dropped" sat on the owner's screen for eight
    /// minutes and across several takes, outliving the take it described by a
    /// long way. This is cleared when a take starts and by the × on the status
    /// row, and it is the last thing in the message chain because a live
    /// problem always matters more than a finished one.
    take_note: Option<String>,
    /// The newest camera frame, kept as RGBA for the compositor.
    ///
    /// A copy, and a deliberate one: the preview uploads its own texture and
    /// then drops the frame, but a video tick happens on the take's clock and
    /// not the window's, so the pixels have to still be here when it does.
    /// Without this the video would only ever contain frames that happened to
    /// land on the same window frame as a tick.
    /// **And WHEN it arrived**, which is the part that was missing.
    ///
    /// This is the picture that goes into the recorded video, and it had no
    /// staleness guard of any kind: the on-screen preview is dropped the
    /// moment `camera_running` goes false, and this was kept for ever. A
    /// camera that opens, hands over one frame from its own stale buffer and
    /// then delivers nothing composited that one frame into every tick of
    /// every take — a picture six hours old, over a performance that
    /// happened tonight, and nothing on screen to say so.
    ///
    /// `camera_silent` cannot catch that: it asks whether the device has
    /// EVER delivered a frame, and this one delivered exactly one. Only the
    /// age of the picture in hand answers the question.
    camera_rgba: Option<(Vec<u8>, u32, u32, std::time::Instant)>,
    /// When to try starting the monitor output again, and how many tries are
    /// left.
    ///
    /// `start_engine` runs on the edge of the band opening, which is fine while
    /// the only way to lose the engine is to close the band. Changing the
    /// buffer size drops it deliberately and reopens the SAME CoreAudio device
    /// in the same breath — the one moment a transient failure is likely — and
    /// without a retry the app would sit there with no monitor and no
    /// instrument until somebody thought to close the band and open it again.
    ///
    /// Bounded, because a device that is genuinely gone must not be reopened
    /// sixty times a second for the rest of the session. The band shows the
    /// error either way.
    engine_retry: Option<(std::time::Instant, u8)>,
    /// An `open_audio_path` that arrived while a take was running.
    ///
    /// See that function: the reopen is REFUSED during a take, not queued
    /// behind it, so without this a device change mid-take would leave the
    /// engine down for the rest of the session. Cleared by the take ending.
    audio_path_deferred: bool,
    /// The count-in downbeat the session has already been armed with.
    ///
    /// See the use site: `count_in_done` is a latch, so without this the same
    /// instant is handed over every frame for the rest of the session.
    armed_downbeat: Option<i64>,
    /// The buffer size both streams were opened with.
    ///
    /// Changing it has to REOPEN them — a running stream cannot be resized —
    /// and reopening the output means reloading every instrument, which is
    /// five seconds each. So it is done on the edge, and never while a take is
    /// rolling: a take whose buffer changed halfway through is a take with a
    /// hole in it.
    buffer_open: Option<u32>,
    /// The sample rate both streams were opened with, and the system they were
    /// opened through. Same edge, same reopen, same refusal mid-take.
    ///
    /// The system is the heaviest of the three: it does not merely reopen the
    /// streams, it changes which driver stack they are opened against — so the
    /// device selected under the old one may not exist under the new one, and
    /// `reconcile_audio` will settle to "missing" and say so.
    rate_open: Option<u32>,
    system_open: Option<String>,
    /// **What "Default" resolved to**, cached for this driver stack.
    ///
    /// `None` is "not asked yet"; `Some(None)` is "asked, and this machine has
    /// no output to ask" — a distinction that matters because this is read
    /// from the per-frame path check, and collapsing the two would re-probe
    /// the audio device sixty times a second on a machine with no sound.
    /// Thrown away whenever the audio system changes underneath it.
    /// See `Engine::default_output_rate` for why Default must resolve at all.
    default_rate: Option<Option<u32>>,
    /// Dropouts already reported, so only the new ones are announced.
    xruns_seen: u64,
    /// The graph's own count of this process's xruns, watched on a timer.
    ///
    /// The authority on a PipeWire desktop, where the client-side counter is
    /// structurally blind — pipewire-alsa absorbs the EPIPE and cpal never
    /// hears it, verified with a 64-frame buffer the owner called "total
    /// noise" against an app log with zero backend errors. Off Linux it
    /// answers `None` forever and starts no thread.
    graph_dropouts: tangent_record::audio::GraphDropouts,
    /// The graph's floor, refreshed on the same edge that reopens the streams.
    ///
    /// Cached because reading it costs a process (`pw-metadata`), and a panel
    /// that spawned one every frame would be a worse bug than the one this
    /// fixes. The edge is the right moment anyway: the floor only matters when
    /// a buffer is being asked for.
    graph_floor: Option<u32>,
    /// The last finished take this host has already accounted for.
    ///
    /// Updated whether or not "Show when done" is ticked, which is the point:
    /// without it, ticking the box after a take would immediately open a Finder
    /// window for a recording made ten minutes ago.
    seen_take: Option<String>,
    /// `IVORY_OPEN_EDITOR=1` bookkeeping. See `after_frame`.
    dev_editor_at: Option<std::time::Instant>,
    dev_editor_done: bool,
    /// The red button is DOWN: armed lanes are being recorded into the
    /// arrangement. `None` the rest of the time.
    arr_rec: Option<ArrRec>,
    /// An EXPORT in flight: the arrangement playing start-to-end into an
    /// audio-only take that lands in the project's `export/`.
    bounce: Option<Bounce>,
    /// A FULL-SPEED export in flight, on its own offline engine.
    offline_bounce: Option<OfflineBounce>,
    /// The red button pressed, the COUNT-IN still clicking: the record
    /// begins on the downbeat.
    arr_rec_wait: bool,
    /// The realtime pass's delivery instructions, applied at the finish.
    pending_delivery: Option<tangent_ui::ports::ProjectExportSpec>,
}

/// One bounce: where it stops, what it writes, and whether it is waiting
/// for the files to close.
/// A FULL-SPEED export: its own engine with no device, pulled by hand a
/// slice of a second per UI frame — the machine renders as fast as it can
/// while the window stays alive. The owner's default; realtime is the
/// option now.
#[cfg(feature = "recorder")]
struct OfflineBounce {
    e: crate::instrument::Engine,
    spec: tangent_ui::ports::ProjectExportSpec,
    /// Frames to render, and how many have been.
    until: u64,
    done: u64,
    block: Vec<f32>,
    master: Option<(std::path::PathBuf, tangent_record::wav::WavWriter)>,
    stems: Vec<(usize, std::path::PathBuf, tangent_record::wav::WavWriter)>,
    stem_scratch: Vec<f32>,
    out_dir: std::path::PathBuf,
    /// The wave as it is heard, one (min, max) per drawn column. Written by
    /// position rather than appended, so a block that spans two columns fills
    /// both and a render that stalls leaves no gap.
    peaks: Vec<(f32, f32)>,
}

#[cfg(feature = "recorder")]
struct Bounce {
    /// Stop when the playhead passes this — the span plus a tail for the
    /// reverbs to ring out.
    until_s: f64,
    finishing: bool,
    /// Stems too. (The per-lane tap arrives with them.)
    stems: bool,
    /// One WAV per used lane, fed from the engine's stem tap as the pass
    /// runs — never buffered whole: a long song is a lot of floats.
    stem_writers: Vec<(usize, tangent_record::wav::WavWriter)>,
    /// The drain buffer, reused.
    stem_scratch: Vec<f32>,
}

/// How long `IVORY_OPEN_EDITOR` waits before opening, so the plugin has
/// finished loading and the window has drawn at least one ordinary frame.
#[cfg(feature = "recorder")]
const DEV_EDITOR_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

/// One press of the red button: what has been collected since, and where it
/// lands. `finish_arr_record` is the only way out, and it always hands the
/// document its clips.
#[cfg(feature = "recorder")]
struct ArrRec {
    /// The playhead second the record began at.
    start_s: f64,
    /// The same instant on the MIDI tap's clock, for stamping notes.
    start_ns: i64,
    /// Armed MIDI lanes at the press. Each gets an identical note clip — the
    /// live stream feeds every armed MIDI lane, so the record says what the
    /// room heard.
    midi_lanes: Vec<usize>,
    /// Armed AUDIO lanes at the press: (input pick, lane, frames so far).
    audio: Vec<(usize, usize, Vec<f32>)>,
    /// Finished notes, clip-relative seconds.
    notes: Vec<tangent_ui::arrangement::ArrNote>,
    /// Keys currently down: when they landed, and how hard.
    open: std::collections::HashMap<u8, (f64, u8)>,
}

#[cfg(feature = "recorder")]
impl ArrRec {
    /// One raw MIDI event off the tap, stamped against the record's T0 on
    /// the same clock the engine hears. Note edges only; the pedal rides the
    /// live path and is not written down yet.
    fn note_event(&mut self, t: i64, bytes: &[u8]) {
        let [status, data1, data2] = *bytes else {
            return;
        };
        let rel = ((t - self.start_ns) as f64 / 1e9).max(0.0);
        let key = data1 & 0x7F;
        match status & 0xF0 {
            0x90 if data2 > 0 => {
                self.open.insert(key, (rel, data2 & 0x7F));
            }
            0x80 | 0x90 => {
                if let Some((at, vel)) = self.open.remove(&key) {
                    self.notes.push(tangent_ui::arrangement::ArrNote {
                        at_s: at,
                        dur_s: (rel - at).max(0.05),
                        key,
                        vel,
                    });
                }
            }
            _ => {}
        }
    }
}

/// The standalone app: `TangentApp`, plus the eframe trait impl it cannot carry.
/// The settings' patch INDEX, as the engine's patch.
///
/// One function, in one place, because an index and an enum that disagree is a
/// silent wrong instrument rather than an error: the piano would simply come
/// up when the electric piano was asked for. `dialogs::KEYS_PATCH_NAMES` is
/// the same list in the same order and the picker reads it, so the two stay
/// aligned by construction as long as nobody reorders one of them alone.
#[cfg(feature = "recorder")]
fn keys_patch_of(index: usize) -> crate::keys::Patch {
    crate::keys::Patch::ALL
        .get(index)
        .copied()
        .unwrap_or(crate::keys::Patch::Piano1)
}

pub struct DesktopApp {
    app: TangentApp,
    /// **Whether the window is currently covering its whole screen** because
    /// it is borderless AND maximised. See `crate::macwin::cover_screen`.
    ///
    /// A latch, not a per-frame call: `setFrame` fires once on the way in and
    /// the presentation options once on the way out. Sending either every
    /// frame fights AppKit's own zoom animation, which is the same lesson the
    /// aspect snap already learned two hundred lines up.
    covering_screen: bool,
    /// The drawable size to restore after a decoration change, and how many
    /// frames it has been tried for. See `reconcile_decoration_size`.
    decor_want_content: Option<(f64, f64, u8)>,
    /// The hardware MIDI fast lane, cloned out of `DeviceMidi` before the
    /// ports box swallows it. `start_engine` seats each fresh engine's ring
    /// here; the midir callback plays through it. See `midi::MidiFastLane`.
    midi_lane: Arc<midi::MidiFastLane>,
    #[cfg(feature = "recorder")]
    recorder: Recorder,
    /// The launch splash, until it has been earned and faded. `None` after.
    splash: Option<Splash>,
    /// The MIDI port to open once the window is up: `Some(Some(name))` from
    /// `-p`, `Some(None)` for the priority chain, `None` once it has run.
    /// See where it is taken for why it is not done in the constructor.
    pending_midi: Option<Option<String>>,
    /// A frame has reached the screen. Deferred startup work waits for it.
    painted_once: bool,
    /// What the title bar says now, so the viewport command is sent only
    /// when the composed title actually changes — the window title carries
    /// the project's name and its dirtiness, and re-sending an unchanged
    /// string every frame is noise some window managers animate.
    last_title: String,
    /// Frames counted by the `IVORY_GEOM_PROBE` diagnostic. See `update`.
    probe_frames: u32,
    /// `IVORY_PACE_PROBE` accumulators: when the window last reported, how
    /// many frames since, and the frame times seen in that second.
    pace_since: Option<std::time::Instant>,
    pace_frames: u32,
    pace_dt: Vec<f32>,
    /// A screen reader has been seen. Stops the probe below from running
    /// again — see `screen_reader_attached`, which is not free of side
    /// effects.
    reader_seen: bool,
    /// The maps the engine was last given. Compared before pushing, because
    /// `Engine::set_maps` is a ring handover that WAITS for the displaced
    /// pair to come back — cheap once, and a stall if it ran every frame.
    #[cfg(feature = "recorder")]
    pushed_maps: Option<crate::instrument::Maps>,
    /// The automation the engine was last given, compared before pushing for
    /// the reason `pushed_maps` gives. Invalidated in `start_engine` beside
    /// it — a fresh engine has no curves.
    pushed_auto: Option<tangent_ui::automation::Automation>,
    /// The DX7 cartridge the built-in is playing out of, if one is loaded.
    ///
    /// **The host holds the voices; the UI holds the names.** Kept here rather
    /// than in `TangentApp` because a `Voice` is a synthesizer's business and
    /// `tangent-ui` is not allowed to know what one is — the same split as the
    /// plugin picker's paths-not-modules.
    #[cfg(feature = "recorder")]
    cartridge: Option<crate::dx7::Cartridge>,
    /// Every cartridge any strip names, loaded once and kept, keyed by path
    /// with `""` for the bank that ships. Grown on demand: a cartridge is
    /// kilobytes, a session rarely names more than two, and per-strip DX7s
    /// mean two strips can be playing out of two different banks at once.
    #[cfg(feature = "recorder")]
    cartridges: std::collections::HashMap<String, crate::dx7::Cartridge>,
    /// The patch the editor is working on, if it is open.
    ///
    /// **A copy, not a reference into the cartridge.** Editing is not choosing:
    /// what is being built here may never be saved, and it must not modify the
    /// bank it started from. What it does do is play, immediately — see
    /// `SetPatchParam`.
    #[cfg(feature = "recorder")]
    editing: Option<crate::dx7::Voice>,
    /// Which strip's DX7 the editor is voicing. Since the instruments went
    /// per-lane, "heard as it is turned" needs an address.
    #[cfg(feature = "recorder")]
    editing_strip: usize,
    /// The window was fullscreen when a picker was asked for, so it comes out
    /// of fullscreen, opens the picker next frame, and goes back after. See
    /// `picker_needs_windowed`.
    #[cfg(feature = "recorder")]
    refullscreen: bool,
    /// A picker deferred by one frame for that reason.
    #[cfg(feature = "recorder")]
    /// A request held back for one frame so the dialogs above it can stand
    /// down first. See `TangentApp::native_panel_up`.
    panel_armed: bool,
    /// The SAVE panel's own copy of `panel_armed`. The project menu is a child
    /// viewport, and closing it (`dialog = None`) does not destroy its window
    /// the same frame — a save panel opened before that window is gone lands
    /// BEHIND it, which is "Save As loads below the menu". One extra frame, as
    /// the pickers take. Its own flag so a load armed in the same `after_frame`
    /// cannot clear it.
    #[cfg(feature = "recorder")]
    save_armed: bool,
    deferred_file: Option<tangent_ui::ports::FileRequest>,
    #[cfg(feature = "recorder")]
    deferred_dir: Option<tangent_ui::ports::DirRequest>,
    /// A save panel waiting for the window to leave fullscreen first.
    #[cfg(feature = "recorder")]
    deferred_save: Option<tangent_ui::ports::SaveRequest>,
    /// The take report already shown, so it is raised once rather than every
    /// frame until the next take.
    #[cfg(feature = "recorder")]
    reported_take: Option<String>,
    /// What the in-app browser is filtering by, so the folder it navigates
    /// into is listed the same way the first one was.
    #[cfg(feature = "recorder")]
    browse_extensions: Vec<String>,
    /// The document's decoded audio, by resolved path — so an edit does not
    /// re-decode a hundred megabytes, and the engine's `Arc`s have an owner
    /// here. Cleared when the engine restarts: the cache is keyed to one
    /// sample rate. Entries no clip references leave with the edit.
    #[cfg(feature = "recorder")]
    track_cache: std::collections::HashMap<std::path::PathBuf, Arc<tangent_record::decode::Clip>>,
    /// Clips that have been time-stretched, by `(file, ratio in millionths,
    /// mode)`.
    ///
    /// **Rendered once and played statelessly**, which is the whole design —
    /// see `crate::stretch`. The key rounds the ratio to a millionth so a
    /// drag that lands on the same value twice is one render and not two;
    /// the tenth of a cent that rounds away is a hundredth of a sample on
    /// the longest clip anybody will import.
    #[cfg(feature = "recorder")]
    stretch_cache:
        std::collections::HashMap<(std::path::PathBuf, i64, String), Arc<tangent_record::decode::Clip>>,
    /// The drumpad's decoded samples, by resolved path — its own cache,
    /// because `track_cache` evicts by what the ARRANGEMENT references and
    /// a kit is not the arrangement's business. Same rate-keying, same
    /// clear on an engine restart.
    #[cfg(feature = "recorder")]
    pad_cache: std::collections::HashMap<std::path::PathBuf, Arc<tangent_record::decode::Clip>>,
    /// The scope drain's reusable buffer.
    scope_buf: Vec<f32>,
    /// The INSERT tap drain's reusable buffer, and where the tap is aimed —
    /// `None` while no plugin face has it armed, which is also the gate on
    /// draining it at all. Kept in ENGINE terms (seat, bay), translated once
    /// where the request lands, so an engine rebuilt mid-session could be
    /// re-armed from here without re-asking the UI.
    tap_buf: Vec<f32>,
    insert_tap_at: Option<(usize, usize)>,
    /// The IR wizard's host-side road: what is armed, the reference pass on
    /// hold, the worker chewing a finished capture, and the processed IRs
    /// waiting for a Save. See the "IRs on disk" section.
    ir: IrRoad,
}

/// How many columns the backing track's waveform is drawn from.
///
/// **Not the pixel width of the row.** The envelope is computed once when the
/// file is imported and drawn at whatever size the window happens to be, so it
/// has to be finer than the widest band anybody will open and coarse enough to
/// stay a few kilobytes. A thousand columns is under a pixel each at 1080p.
#[cfg(feature = "recorder")]
const TRACK_WAVE_BUCKETS: usize = 1000;

/// How old the camera's last frame may be and still be composited.
///
/// **One second.** The slowest format this app will open is fifteen frames a
/// second, so a camera that is working delivers one every sixty-seven
/// milliseconds; a whole second of nothing is a device that has stopped.
/// Generous enough that a stall on a busy machine does not blank the pane,
/// short enough that nothing six hours old ever reaches a file.
#[cfg(feature = "recorder")]
const CAMERA_FRAME_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// Buckets per CLIP picture in the arrangement. Smaller than the band's:
/// a clip is one of many and redrawn on every trim.
const CLIP_WAVE_BUCKETS: usize = 600;

/// Seconds on the timeline to frames at `rate` — the ONE conversion.
///
/// **It was four hand-written copies of the same closure**, in the live clip
/// builder (twice, once for MIDI and once for audio), in the offline bounce's
/// mirror of it, and in the arrangement recorder. They agreed, which is the
/// only reason nothing was wrong: `reload_clips` and `start_offline_bounce`
/// have no shared helper at all, so what you hear and what you export are two
/// separately-maintained readings of the same document.
///
/// One function so a tempo map — which turns this from a multiply into a walk
/// along a piecewise curve — cannot be taught to three of the four.
///
/// `max(0.0)` because a negative position is a document that has been edited
/// by hand, and `as u64` on a negative float saturates to zero anyway; saying
/// so is better than relying on it.
fn frames_at(rate: f64, seconds: f64) -> u64 {
    (seconds.max(0.0) * rate).round() as u64
}

/// A peak envelope of `len` frames starting `offset` frames into `clip`,
/// interleaved stereo folded to one magnitude per bucket — the played
/// span's picture, where `Clip::envelope` is the whole file's.
#[cfg(feature = "recorder")]
fn span_envelope(
    clip: &tangent_record::decode::Clip,
    offset: usize,
    len: usize,
    buckets: usize,
) -> Vec<f32> {
    let total = clip.samples.len() / 2;
    let start = offset.min(total);
    let end = (offset + len).min(total);
    let frames = end.saturating_sub(start);
    if frames == 0 || buckets == 0 {
        return Vec::new();
    }
    // **MIN and MAX, not a peak.** A column drawn from one number is a bar
    // chart of loudness; the shape anybody reads off a waveform is the
    // outline of both sides, and audio is not symmetric about zero. See
    // `tangent_ui::ports::ClipInfo`.
    let mut out = vec![0.0f32; buckets * 2];
    for i in 0..buckets {
        let a = start + i * frames / buckets;
        let b = (start + (i + 1) * frames / buckets).max(a + 1).min(end);
        // **Seeded at the infinities, not at zero.**
        //
        // Starting both at 0.0 forces every bucket to straddle it: a passage
        // that is entirely above the line — a bass note's positive half, or
        // anything with a DC offset on it — reports a minimum of zero and is
        // drawn as though it swung down to silence. Audio crosses zero often
        // enough that it mostly looked right, which is why it would have
        // survived a glance. See `frame_range_minmax`.
        let (lo, hi) = frame_range_minmax(clip, a, b);
        out[i * 2] = lo;
        out[i * 2 + 1] = hi;
    }
    out
}

/// The lowest and highest sample across `[a, b)` frames, both channels.
///
/// **MIN and MAX, not a peak, and seeded at the infinities** — see
/// [`span_envelope`], where both of those are explained and where this used to
/// be inline. It came out so the looped picture below could read the same
/// numbers rather than a second opinion about them.
#[cfg(feature = "recorder")]
fn frame_range_minmax(clip: &tangent_record::decode::Clip, a: usize, b: usize) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for f in a..b {
        for ch in 0..2 {
            let v = clip.samples.get(f * 2 + ch).copied().unwrap_or(0.0);
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    // An empty or wholly non-finite bucket is a flat line, which is the only
    // honest thing to draw for "nothing here".
    if lo <= hi {
        (lo.max(-1.0), hi.min(1.0))
    } else {
        (0.0, 0.0)
    }
}

/// **A LOOPED clip's picture: the same pass, drawn again and again.**
///
/// [`span_envelope`] reads the file straight through, which is right for a
/// clip that plays once and wrong for one that repeats — an eight-bar clip
/// looping one bar was drawn as eight bars of the file, so the waveform said
/// one thing and the speakers said another.
///
/// Each bucket's time is folded into the loop before it is looked up. A bucket
/// that straddles a repeat's seam reads only its first side, which is under a
/// pixel wide and is what every other bucket in this file does with a range it
/// cannot fit.
#[cfg(feature = "recorder")]
fn looped_span_envelope(
    clip: &tangent_record::decode::Clip,
    offset_s: f64,
    loop_s: f64,
    into_s: f64,
    span_s: f64,
    buckets: usize,
    rate: f64,
) -> Vec<f32> {
    let total = clip.samples.len() / 2;
    if buckets == 0 || total == 0 || !(loop_s > 1.0e-6) || !(span_s > 0.0) {
        return Vec::new();
    }
    let mut out = vec![0.0f32; buckets * 2];
    for i in 0..buckets {
        let t0 = into_s + span_s * i as f64 / buckets as f64;
        let t1 = into_s + span_s * (i + 1) as f64 / buckets as f64;
        let folded = t0.rem_euclid(loop_s);
        let a = frames_at(rate, offset_s + folded) as usize;
        let width = frames_at(rate, (t1 - t0).max(0.0)).max(1) as usize;
        let a = a.min(total);
        let b = (a + width).min(total);
        let (lo, hi) = frame_range_minmax(clip, a, b.max(a));
        out[i * 2] = lo;
        out[i * 2 + 1] = hi;
    }
    out
}

/// How many buckets a clip's picture wants, and which of its seconds.
///
/// **Only what is on screen, at the pixels it occupies.**
///
/// A picture computed once at import is sharp at one zoom and blocky at
/// every other, and a fixed budget cannot win: raise it enough for a
/// close-up of a five-minute file and every clip carries a megabyte for a
/// view nobody is looking at. Bounding the work by the WINDOW instead means
/// the cost of a redraw is the seconds on screen, whatever the clip's
/// length.
///
/// `None` when the clip is not in view at all, which is most of them on a
/// long song.
#[cfg(feature = "recorder")]
fn wave_span(
    clip_start_s: f64,
    clip_len_s: f64,
    view_start_s: f64,
    view_len_s: f64,
    lane_px: f32,
) -> Option<(f64, f64, usize)> {
    if clip_len_s <= 0.0 || view_len_s <= 0.0 || lane_px < 1.0 {
        return None;
    }
    // **A margin either side**, so a small pan does not have to redraw: the
    // staleness threshold in `arr_waves_stale` lets the view move by a
    // twentieth before it asks again, and a picture that covered exactly the
    // window would be short by that much for the whole of that movement.
    let margin = view_len_s * 0.10;
    let from = (view_start_s - margin).max(clip_start_s);
    let to = (view_start_s + view_len_s + margin).min(clip_start_s + clip_len_s);
    let span = to - from;
    if span <= 0.0 {
        return None;
    }
    let px_per_s = f64::from(lane_px) / view_len_s;
    // One bucket per pixel, and never more than a screenful of them: past
    // that they are drawn on top of each other.
    let buckets = ((span * px_per_s).ceil() as usize).clamp(1, 8_192);
    Some((from, span, buckets))
}

/// **Every stepped row has a list to step through.**
///
/// The panel decides which rows are named-value rows ([`FxRow::step`]) and the
/// HOST decides what those names are; they meet by string key and nothing
/// makes them agree. A row whose key the host never sends draws an empty box
/// that does nothing when pressed — which is what `lpf_slope` did for exactly
/// as long as it took to write this.
///
/// [`FxRow::step`]: tangent_ui::recorder_panel::FxRow::step
#[cfg(all(test, feature = "recorder"))]
#[test]
fn the_host_offers_a_choice_for_every_stepped_row() {
    let d = effect_defaults();
    for fx in tangent_ui::recorder_panel::Fx::ALL {
        for row in fx.rows(true) {
            if !row.step {
                continue;
            }
            let c = d
                .choices
                .iter()
                .find(|c| c.key == row.key)
                .unwrap_or_else(|| panic!("{} has no choices for {}", fx.title(), row.key));
            assert!(!c.options.is_empty(), "{} offers an empty list", row.key);
            assert!(
                c.options.iter().any(|(k, _)| *k == c.default),
                "{}'s default {:?} is not one of its options",
                row.key,
                c.default
            );
        }
    }
    // And every sliding row has a default value, for the same reason.
    for fx in tangent_ui::recorder_panel::Fx::ALL {
        for row in fx.rows(true) {
            if row.step || row.key.is_empty() {
                continue;
            }
            assert!(
                d.values.contains_key(row.key),
                "{} has no shipped value for {}",
                fx.title(),
                row.key
            );
        }
    }
}

/// **What a filter knob SAYS is where the filter actually is.**
///
/// The readout is computed in `tangent-ui` from two numbers this file hands it,
/// using the same exponential the DSP uses — so the only way they can disagree
/// is if one of the two ends is wrong. That would put a confident, precise,
/// incorrect frequency under the knob, which is worse than the percentage it
/// replaced.
#[cfg(all(test, feature = "recorder"))]
#[test]
fn a_filter_knob_reads_out_where_its_filter_actually_is() {
    use tangent_ui::ports::KnobUnit;
    let d = effect_defaults();
    // The limiter reads in decibels of threshold, and the same rule applies:
    // a number under the knob that is not where the limiter actually starts
    // working is worse than no number at all.
    let (_, limiter) = d
        .units
        .iter()
        .find(|(k, _)| k == "limiter_mix")
        .expect("the limiter has no unit and would read as a percentage");
    let KnobUnit::Decibels { low, high } = *limiter else {
        panic!("the limiter does not read in decibels")
    };
    assert!(
        (low - crate::effects::LIMITER_DB.0).abs() < 1.0e-3
            && (high - crate::effects::LIMITER_DB.1).abs() < 1.0e-3,
        "the limiter advertises {low}..{high} dB and thresholds {}..{}",
        crate::effects::LIMITER_DB.0,
        crate::effects::LIMITER_DB.1
    );
    // Fully right is off, twelve o'clock is -24, fully left is -48.
    assert_eq!(
        tangent_ui::recorder_panel::knob_reading(*limiter, 1.0),
        "0.0 dB",
        "the limiter's resting position is not 0 dB"
    );
    assert_eq!(
        tangent_ui::recorder_panel::knob_reading(*limiter, 0.5),
        "-24.0 dB"
    );
    assert_eq!(
        tangent_ui::recorder_panel::knob_reading(*limiter, 0.0),
        "-48.0 dB"
    );

    for (key, range) in [
        ("hpf_mix", crate::effects::HPF_HZ),
        ("lpf_mix", crate::effects::LPF_HZ),
    ] {
        let (_, unit) = d
            .units
            .iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("{key} has no unit and would read as a percentage"));
        let KnobUnit::Hertz { low, high } = *unit else {
            panic!("{key} does not read in hertz")
        };
        assert!(
            (low - range.0).abs() < 1.0e-3 && (high - range.1).abs() < 1.0e-3,
            "{key} advertises {low}..{high} Hz and sweeps {}..{} Hz",
            range.0,
            range.1
        );
        // And the readout agrees with the DSP at both ends and the middle.
        for t in [0.0_f32, 0.5, 1.0] {
            let said = tangent_ui::recorder_panel::knob_reading(*unit, t);
            let actual = range.0 * (range.1 / range.0).powf(t);
            let shown: f32 = said.split_whitespace().next().unwrap().parse().unwrap();
            let shown = if said.contains('k') {
                shown * 1_000.0
            } else {
                shown
            };
            assert!(
                (shown - actual).abs() / actual < 0.02,
                "{key} at {t} reads {said} and the filter is at {actual:.0} Hz"
            );
        }
    }
    // **The LPF ships OPEN.** Its default must be advertised as 1.0 and read
    // out as 20 kHz — the mix knobs' defaults were missing from `values`, so
    // a reset or the default marker fell to the bottom of the sweep, 200 Hz.
    let lpf_default = d
        .values
        .get("lpf_mix")
        .and_then(serde_json::Value::as_f64)
        .expect("lpf_mix has no advertised default");
    assert!(
        (lpf_default - 1.0).abs() < 1.0e-6,
        "the LPF default is {lpf_default}, not open"
    );
    let (_, lpf_unit) = d
        .units
        .iter()
        .find(|(k, _)| k == "lpf_mix")
        .expect("lpf unit");
    assert_eq!(
        tangent_ui::recorder_panel::knob_reading(*lpf_unit, lpf_default as f32),
        "20 kHz",
        "the LPF default does not read 20 kHz"
    );
}

/// **A directory listing is a listing, sorted and filtered.**
///
/// The in-app browser exists because `rfd` silently does nothing on a Linux
/// box with no portal and no zenity, so this is the only way to choose a file
/// there — and a browser that hid the file somebody wanted, or offered them a
/// `.DS_Store`, would be a worse answer than the silence it replaced.
#[cfg(all(test, feature = "recorder"))]
#[test]
fn the_browser_lists_folders_first_and_filters_what_it_offers() {
    let root = std::env::temp_dir().join(format!("tangent-browse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Zebra")).expect("mkdir");
    std::fs::create_dir_all(root.join("apple")).expect("mkdir");
    for f in ["b.wav", "A.MP3", "notes.txt", ".hidden.wav"] {
        std::fs::write(root.join(f), b"x").expect("write");
    }
    let exts: Vec<String> = ["wav", "mp3"].into_iter().map(str::to_owned).collect();
    let (rows, err) = DesktopApp::list_dir(&root, &exts);
    assert!(err.is_empty(), "{err}");
    let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();

    // The way up, then folders, then files - each sorted without regard to
    // case, which is the order every file manager uses.
    assert_eq!(
        names,
        ["..", "apple", "Zebra", "A.MP3", "b.wav"],
        "{names:?}"
    );
    assert!(rows[0].is_dir && rows[1].is_dir && rows[2].is_dir);
    assert!(!rows[3].is_dir);
    // **Case-insensitively filtered**: a file named `.MP3` is an mp3, and a
    // browser that only matched lowercase would hide half of anybody's music.
    assert!(
        !names.contains(&"notes.txt"),
        "an unmatched file was offered"
    );
    assert!(!names.contains(&".hidden.wav"), "a dotfile was offered");

    // No filter at all offers everything but the dotfiles.
    let (all, _) = DesktopApp::list_dir(&root, &[]);
    assert!(all.iter().any(|e| e.name == "notes.txt"));
    assert!(!all
        .iter()
        .any(|e| e.name.starts_with('.') && e.name != ".."));

    // A folder that is not there is a sentence, not a panic or an empty list
    // that looks like an empty folder.
    let (rows, err) = DesktopApp::list_dir(&root.join("nope"), &exts);
    assert!(rows.is_empty());
    assert!(!err.is_empty(), "a missing folder reported nothing");

    let _ = std::fs::remove_dir_all(&root);
}

/// [`effect_defaults`], for the offscreen screenshot test.
#[cfg(all(test, feature = "recorder"))]
pub fn effect_defaults_for_shot() -> tangent_ui::ports::EffectDefaults {
    effect_defaults()
}

/// What the effects ship as, for the panel that draws them.
///
/// Built from `Params::default()` rather than written out again, so the panel
/// and the audio cannot disagree about where a slider starts.
#[cfg(feature = "recorder")]
fn effect_defaults() -> tangent_ui::ports::EffectDefaults {
    use crate::effects::{Division, EchoMode, Params, Sends, Stages};
    let d = Params::default();
    // **The six MIX knobs' own defaults live on `Sends`**, so a reset and the
    // default marker land where the sound ships — not on a fallback. Their
    // absence read the LPF's default as the bottom of its sweep, 200 Hz, when
    // the shipped low-pass is fully OPEN at 20 kHz (`lpf = 1.0`).
    let m = Sends::default();
    let mut values = serde_json::Map::new();
    for (key, v) in [
        ("reverb_mix", m.reverb),
        ("delay_mix", m.delay),
        ("chorus_mix", m.chorus),
        ("phaser_mix", m.phaser),
        ("hpf_mix", m.hpf),
        ("lpf_mix", m.lpf),
        ("gain_mix", m.gain),
        ("limiter_mix", m.limiter),
        ("reverb_size", d.reverb_size),
        ("reverb_damp", d.reverb_damp),
        ("reverb_width", d.reverb_width),
        // The plate's own two, from `fx-dsp`. See docs/FX-DSP-HANDOFF.md §1.
        ("plate_dispersion", d.plate_dispersion),
        ("plate_predelay", d.plate_predelay),
        ("delay_feedback", d.delay_feedback),
        ("delay_tone", d.delay_tone),
        ("delay_width", d.delay_width),
        ("delay_free_hz", d.delay_free_hz),
        ("chorus_rate", d.chorus_rate),
        ("chorus_depth", d.chorus_depth),
        ("chorus_width", d.chorus_width),
        ("chorus_tone", d.chorus_tone),
        ("phaser_rate", d.phaser_rate),
        ("phaser_feedback", d.phaser_feedback),
        ("phaser_range", d.phaser_range),
        ("hpf_resonance", d.hpf_resonance),
        ("lpf_resonance", d.lpf_resonance),
        ("limiter_release", d.limiter_release),
        ("limiter_knee", d.limiter_knee),
    ] {
        values.insert(key.to_owned(), serde_json::Value::from(f64::from(v)));
    }
    tangent_ui::ports::EffectDefaults {
        values,
        // The filters read in hertz. "48%" on a corner frequency is a number
        // about the knob rather than about the sound, and the one thing
        // anybody wants to know from a filter is where it is.
        units: vec![
            (
                "hpf_mix".to_owned(),
                tangent_ui::ports::KnobUnit::Hertz {
                    low: crate::effects::HPF_HZ.0,
                    high: crate::effects::HPF_HZ.1,
                },
            ),
            (
                "lpf_mix".to_owned(),
                tangent_ui::ports::KnobUnit::Hertz {
                    low: crate::effects::LPF_HZ.0,
                    high: crate::effects::LPF_HZ.1,
                },
            ),
            // The limiter is a threshold, and a threshold has one unit.
            (
                "limiter_mix".to_owned(),
                tangent_ui::ports::KnobUnit::Decibels {
                    low: crate::effects::LIMITER_DB.0,
                    high: crate::effects::LIMITER_DB.1,
                },
            ),
            // And so is the gain. **`+12.0 dB` is the reading somebody needs
            // to see on the one control that can clip the master** — a
            // percentage would be a number about the knob.
            (
                "gain_mix".to_owned(),
                tangent_ui::ports::KnobUnit::Decibels {
                    low: crate::effects::GAIN_DB.0,
                    high: crate::effects::GAIN_DB.1,
                },
            ),
        ],
        choices: vec![
            // The Space Echo's twelve-way mode selector: which heads are
            // live and whether the spring is in. It is the instrument, not a
            // setting — see the handoff's §1.
            tangent_ui::ports::ChoiceParam {
                key: "echo_mode".to_owned(),
                options: EchoMode::ALL
                    .into_iter()
                    .map(|x| (x.key().to_owned(), x.label().to_owned()))
                    .collect(),
                default: d.echo_mode.key().to_owned(),
            },
            // The reverb's own selector: the plate, or the smooth tail that
            // is nobody's hardware. See `effects::ReverbMode`.
            tangent_ui::ports::ChoiceParam {
                key: "reverb_mode".to_owned(),
                options: crate::effects::ReverbMode::ALL
                    .into_iter()
                    .map(|x| (x.key().to_owned(), x.label().to_owned()))
                    .collect(),
                default: d.reverb_mode.key().to_owned(),
            },
            tangent_ui::ports::ChoiceParam {
                key: "delay_division".to_owned(),
                options: Division::ALL
                    .into_iter()
                    .map(|x| (x.key().to_owned(), x.label().to_owned()))
                    .collect(),
                default: d.delay_division.key().to_owned(),
            },
            slope_choice("hpf_slope", d.hpf_slope),
            slope_choice("lpf_slope", d.lpf_slope),
            tangent_ui::ports::ChoiceParam {
                key: "phaser_stages".to_owned(),
                options: Stages::ALL
                    .into_iter()
                    .map(|x| (x.key().to_owned(), x.label().to_owned()))
                    .collect(),
                default: d.phaser_stages.key().to_owned(),
            },
        ],
    }
}

/// Whether a bay's sentinel names one of the three built-in INSTRUMENTS,
/// which the engine renders by mask in `mix_channels` and whose bay is
/// therefore handed nothing.
///
/// **Narrower than `dialogs::is_builtin` on purpose, and the difference is
/// the whole built-in-effects road.** Every `@` path used to be filtered out
/// of the engine here, which was right while the instruments were the only
/// sentinels. Effect sentinels (`@tangent-trim`, and the `FX_UNITS` eight
/// when they arrive) go the OTHER way: `load_insert` seats them in the rack
/// as ordinary tenants, so the filter must name the instruments rather than
/// the prefix — a new effect sentinel then reaches the engine with no edit
/// here, and a new instrument sentinel is a deliberate line.
#[cfg(feature = "recorder")]
fn is_instrument_sentinel(path: &str) -> bool {
    matches!(
        path,
        tangent_ui::dialogs::BUILTIN_PATH
            | tangent_ui::dialogs::DRUMPAD_PATH
            | tangent_ui::dialogs::KEYS_PATH
    )
}

/// **The default pedalboard**, in chain order: dynamics first, then the
/// filters and drive, modulation, time, and the room last — the order a
/// player wires a real board, seeded so the amp arrives dressed rather
/// than as seven pluses.
#[cfg(feature = "recorder")]
const AMP_DEFAULT_BOARD: [&str; tangent_ui::recorder::AMP_PEDALS] = [
    "@tangent-comp",
    "@tangent-wah",
    "@tangent-drive",
    "@tangent-chorus",
    "@tangent-trem",
    "@tangent-echo",
    "@tangent-plate",
];

/// Whether the amp landing on channel `ch` should seed [`AMP_DEFAULT_BOARD`],
/// asked with a lookup instead of the settings so the once-only rule can be
/// tested as plain logic. `pedal_filled(pedal, bay)` answers whether that
/// cell holds anything.
///
/// **"Any non-empty pedal cell" is the whole marker, deliberately** — the
/// simpler of the two honest options (the other was a settings flag per
/// channel, a key carried forever for a fact the slots already state). The
/// seeded board itself trips it, so one amp load seeds exactly once; a
/// player who empties all twenty-one cells by hand AND re-seats the amp
/// gets a fresh board, which is the reading of that gesture we want.
#[cfg(feature = "recorder")]
fn amp_board_to_seed(
    ch: usize,
    pedal_filled: impl Fn(usize, usize) -> bool,
) -> Option<[&'static str; tangent_ui::recorder::AMP_PEDALS]> {
    if ch >= tangent_ui::recorder::CHANNELS {
        return None;
    }
    let untouched = (0..tangent_ui::recorder::AMP_PEDALS)
        .all(|p| (0..tangent_ui::recorder::INSERTS).all(|b| !pedal_filled(p, b)));
    untouched.then_some(AMP_DEFAULT_BOARD)
}

/// **Read one strip's three bays and hand the app what they can automate.**
///
/// A free function because it is called from two arms of the same match and
/// borrows the app and the engine at once; a method on `DesktopApp` would be
/// borrowing `self` twice.
///
/// The plugin's short name comes from the app's own bay text rather than from
/// the module, so the menu says exactly what the bay chip says.
#[cfg(feature = "recorder")]
fn push_fx_params(
    app: &mut tangent_ui::app::TangentApp,
    engine: &crate::instrument::Engine,
    strip: usize,
    inserts: usize,
) {
    let bays: [tangent_ui::ports::FxParams; tangent_ui::recorder::INSERTS] =
        std::array::from_fn(|bay| {
            if bay >= inserts {
                return tangent_ui::ports::FxParams::default();
            }
            let params: Vec<tangent_ui::ports::FxParam> = engine
                .insert_parameters(strip, bay)
                .into_iter()
                .map(|p| tangent_ui::ports::FxParam {
                    id: p.id,
                    title: p.title,
                    units: p.units,
                    default_normalised: p.default_normalised,
                    step_count: p.step_count,
                })
                .collect();
            if params.is_empty() {
                return tangent_ui::ports::FxParams::default();
            }
            tangent_ui::ports::FxParams {
                name: app.insert_short_name(strip, bay),
                params,
                // Read here, once, with the parameters it travels with:
                // whether the tenant reports an aux input bus a key could
                // feed. The UI gates the bay menu's "Sidechain" row on it.
                has_sidechain: engine_insert_wants_key(engine, strip, bay),
            }
        });
    app.set_fx_params(strip, bays);
}

/// Whether the bay's tenant can take a key — the engine's own answer, one
/// thin seam kept so the UI lane and the engine lane could land apart.
#[cfg(feature = "recorder")]
fn engine_insert_wants_key(engine: &crate::instrument::Engine, seat: usize, bay: usize) -> bool {
    engine.insert_wants_key(seat, bay)
}

/// The whole key table onto the engine — `(seat, bay, source)`, absent
/// means cleared, the `set_routes` idiom.
#[cfg(feature = "recorder")]
fn engine_set_insert_keys(engine: &crate::instrument::Engine, keys: &[(usize, usize, usize)]) {
    engine.set_insert_keys(keys);
}

/// **Read one seat's bays and hand the app every built-in face on it** —
/// the `push_fx_params` precedent with the values included, called from the
/// same place for the same reason: the reconcile is the only spot a bay's
/// contents change, so it is the only spot the face data can go stale.
///
/// All three bays every time rather than just the one that moved, because an
/// unload must CLEAR its face (the empty `BuiltinFace` is the tombstone) and
/// the caller only knows which flat index changed, not what it used to hold.
#[cfg(feature = "recorder")]
fn push_builtin_faces(
    app: &mut tangent_ui::app::TangentApp,
    engine: &crate::instrument::Engine,
    strip: usize,
) {
    for slot in 0..tangent_ui::recorder::INSERTS {
        let face = match engine.builtin_insert(strip, slot) {
            Some((spec, values)) => tangent_ui::ports::BuiltinFace {
                strip,
                slot,
                path: spec.path.to_owned(),
                params: spec
                    .params
                    .iter()
                    .zip(values)
                    .map(|(p, v)| tangent_ui::ports::BuiltinFaceParam {
                        id: p.id,
                        symbol: p.symbol.to_owned(),
                        name: p.name.to_owned(),
                        units: p.units.to_owned(),
                        default: p.default,
                        steps: p.steps,
                        reading: crate::builtin_fx::describe(spec.path, p.id, *v),
                        choices: crate::builtin_fx::choices(spec.path, p.id),
                        value: *v,
                    })
                    .collect(),
                // The EQ's picture, drawn by the audio path's own math on
                // the UI's own grid — the two crates share the constants,
                // so the curve the face paints IS the response the block
                // loop runs.
                curve_db: if spec.path == crate::builtin_fx::EQ_PATH {
                    let mut eq = crate::units::eq::EqParams::default();
                    for (i, node) in eq.nodes.iter_mut().enumerate() {
                        let base = i as u32 * 5;
                        let at = |k: u32| values.get((base + k) as usize).copied().unwrap_or(0.5);
                        // A node past the face's three has no values at all;
                        // absent must read as OFF, not as the toggle's noon.
                        node.on = values
                            .get(base as usize)
                            .is_some_and(|v| *v >= 0.5);
                        let last = (crate::units::eq::NodeKind::ALL.len() - 1) as f32;
                        let k = ((at(1) * last).round() as usize)
                            .min(crate::units::eq::NodeKind::ALL.len() - 1);
                        node.kind = crate::units::eq::NodeKind::ALL[k];
                        node.freq = at(2);
                        node.gain = at(3);
                        node.q = at(4);
                    }
                    let lo = tangent_ui::face::CURVE_LO_HZ;
                    let hi = tangent_ui::face::CURVE_HI_HZ;
                    let n = tangent_ui::face::CURVE_POINTS;
                    let freqs: Vec<f32> = (0..n)
                        .map(|i| lo * (hi / lo).powf(i as f32 / (n - 1) as f32))
                        .collect();
                    let live: Vec<crate::units::eq::Node> = eq
                        .nodes
                        .iter()
                        .filter(|p| p.on)
                        .map(|p| p.node())
                        .collect();
                    crate::units::eq::response_db(
                        &live,
                        &freqs,
                        engine.output().sample_rate as f32,
                    )
                } else {
                    Vec::new()
                },
                extra: engine.builtin_extra(strip, slot).to_vec(),
            },
            None => tangent_ui::ports::BuiltinFace {
                strip,
                slot,
                ..Default::default()
            },
        };
        app.set_builtin_face(face);
    }
}

/// One filter's slope, as a choice the panel can step through.
#[cfg(feature = "recorder")]
fn slope_choice(key: &str, default: crate::effects::Slope) -> tangent_ui::ports::ChoiceParam {
    tangent_ui::ports::ChoiceParam {
        key: key.to_owned(),
        options: crate::effects::Slope::ALL
            .into_iter()
            .map(|x| (x.key().to_owned(), x.label().to_owned()))
            .collect(),
        default: default.key().to_owned(),
    }
}

/// Turn the settings file's flat map into the parameters the DSP wants.
///
/// **The translation lives here, on the binary's side of the firewall.**
/// `tangent-ui` holds a map of numbers somebody moved and knows nothing about
/// comb filters; this is where a name becomes a field. A key that is missing,
/// or is the wrong kind of value, leaves that parameter at its default — which
/// is what makes a settings file written by an older build load without a
/// migration, and one written by a newer build load without an error.
#[cfg(feature = "recorder")]
fn effect_params_from(map: &serde_json::Map<String, serde_json::Value>) -> crate::effects::Params {
    use crate::effects::{Division, EchoMode, Params};
    let mut p = Params::default();
    let num = |key: &str| {
        map.get(key)
            .and_then(serde_json::Value::as_f64)
            .map(|v| v as f32)
    };
    for (key, dst) in [
        ("reverb_size", &mut p.reverb_size),
        ("reverb_damp", &mut p.reverb_damp),
        ("reverb_width", &mut p.reverb_width),
        ("plate_dispersion", &mut p.plate_dispersion),
        ("plate_predelay", &mut p.plate_predelay),
        ("delay_feedback", &mut p.delay_feedback),
        ("delay_tone", &mut p.delay_tone),
        ("delay_width", &mut p.delay_width),
        ("delay_free_hz", &mut p.delay_free_hz),
        ("chorus_rate", &mut p.chorus_rate),
        ("chorus_depth", &mut p.chorus_depth),
        ("chorus_width", &mut p.chorus_width),
        ("chorus_tone", &mut p.chorus_tone),
        ("phaser_rate", &mut p.phaser_rate),
        ("phaser_feedback", &mut p.phaser_feedback),
        ("phaser_range", &mut p.phaser_range),
        ("hpf_resonance", &mut p.hpf_resonance),
        ("lpf_resonance", &mut p.lpf_resonance),
        ("limiter_release", &mut p.limiter_release),
        ("limiter_knee", &mut p.limiter_knee),
    ] {
        if let Some(v) = num(key) {
            *dst = v;
        }
    }
    if let Some(d) = map
        .get("delay_division")
        .and_then(serde_json::Value::as_str)
        .and_then(Division::from_key)
    {
        p.delay_division = d;
    }
    // **The crossing that matters.** Without this the mode selector moves
    // and the audio does not — the silent failure this file already warns
    // about twice.
    if let Some(m) = map
        .get("echo_mode")
        .and_then(serde_json::Value::as_str)
        .and_then(EchoMode::from_key)
    {
        p.echo_mode = m;
    }
    if let Some(m) = map
        .get("reverb_mode")
        .and_then(serde_json::Value::as_str)
        .and_then(crate::effects::ReverbMode::from_key)
    {
        p.reverb_mode = m;
    }
    if let Some(v) = map
        .get("phaser_stages")
        .and_then(serde_json::Value::as_str)
        .and_then(crate::effects::Stages::from_key)
    {
        p.phaser_stages = v;
    }
    for (key, dst) in [
        ("hpf_slope", &mut p.hpf_slope),
        ("lpf_slope", &mut p.lpf_slope),
    ] {
        if let Some(v) = map
            .get(key)
            .and_then(serde_json::Value::as_str)
            .and_then(crate::effects::Slope::from_key)
        {
            *dst = v;
        }
    }
    p.sane()
}

/// A parsed cartridge as the picker wants it: names and a bank, no voices.
#[cfg(feature = "recorder")]
fn cartridge_info(
    cart: &crate::dx7::Cartridge,
    error: &str,
    factory: bool,
) -> tangent_ui::ports::CartridgeInfo {
    tangent_ui::ports::CartridgeInfo {
        bank: cart.name.clone(),
        bad_checksum: !cart.checksum_ok,
        voices: cart.names(),
        error: error.to_owned(),
        factory,
    }
}

/// How often free space is measured again while the band is open.
#[cfg(feature = "recorder")]
const DISK_RECHECK: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to wait before trying the monitor output again, and how many times.
///
/// Half a second is long enough for CoreAudio to finish releasing a device that
/// was dropped a moment ago, and five tries is long enough to cover it without
/// becoming a device that is reopened for ever.
#[cfg(feature = "recorder")]
const ENGINE_RETRY_AFTER: std::time::Duration = std::time::Duration::from_millis(500);
#[cfg(feature = "recorder")]
const ENGINE_TRIES: u8 = 5;

/// A background watch for the audio device coming back — "an auto recheck
/// would be nice", the owner's word after the USB-yank test, where a replug
/// recovered nothing because the timed retries had burnt out seconds earlier.
///
/// A thread rather than more timed retries, because the retries were also the
/// FREEZE: `Engine::start` against yanked hardware blocks inside the driver's
/// own timeouts, and five of those on the UI thread stacked to the owner's
/// 10-30 seconds of "Not Responding". This thread probes THE PLATFORM DEFAULT
/// host — never the chosen system: probing ASIO means loading drivers, loading
/// drivers from a second thread is the teardown bug all over again, and a
/// half-dead driver can hang a probe for ten seconds. All of that is fine on
/// this thread and fatal on the UI thread; the hardware's return is visible
/// through WASAPI/CoreAudio regardless of which system the engine reopens on.
///
/// One-shot: sets `live` and exits. The UI thread never joins it — `Drop`
/// signals `stop` and detaches, so a probe wedged inside a dying driver can
/// never wedge the app with it.
#[cfg(feature = "recorder")]
struct Reprobe {
    stop: Arc<std::sync::atomic::AtomicBool>,
    live: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "recorder")]
impl Reprobe {
    fn spawn() -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let live = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let live_t = Arc::clone(&live);
        let spawned = std::thread::Builder::new()
            .name("audio-reprobe".to_owned())
            .spawn(move || {
                use cpal::traits::{DeviceTrait, HostTrait};
                loop {
                    if stop_t.load(Ordering::Relaxed) {
                        return;
                    }
                    let alive = cpal::default_host()
                        .default_output_device()
                        .and_then(|d| d.default_output_config().ok())
                        .is_some_and(|c| c.channels() > 0);
                    if alive {
                        live_t.store(true, Ordering::Relaxed);
                        return;
                    }
                    // 2 s between probes, checked in slices so a stop signal
                    // lands within a tenth of a second.
                    for _ in 0..20 {
                        if stop_t.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            });
        if spawned.is_err() {
            // A machine that cannot spawn a thread: mark live so the UI
            // falls back to trying the open directly rather than waiting
            // for a watcher that does not exist.
            live.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Self { stop, live }
    }

    fn device_back(&self) -> bool {
        self.live.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(feature = "recorder")]
impl Drop for Reprobe {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The non-macOS stand-in for `macwin::Facts`.
///
/// A struct rather than a type alias so the two `window_facts` arms have the
/// same shape on every platform.
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
struct WindowFacts {
    gesture: tangent_ui::host::Gesture,
    os_aspect_lock: bool,
}

#[cfg(feature = "recorder")]
/// **A MIDI clip's notes, with its loop spelled out**, in frames.
///
/// One function because there are two customers — playback in `reload_clips`
/// and the offline bounce — and a schedule built twice is a schedule that
/// eventually disagrees with itself. It already had: the loop was expanded
/// for playback and not for the bounce, so an exported song would have been
/// missing every repeat after the first.
///
/// The notes are held ONCE in the document and expanded here rather than
/// copied into it, because copies would drift from the pass they came from
/// the moment one was edited, and would be indistinguishable from notes
/// somebody meant to write.
fn midi_clip_notes(
    clip: &tangent_ui::arrangement::ArrClip,
    rate: f64,
) -> Vec<crate::instrument::ClipNote> {
    let frames = |s: f64| frames_at(rate, s);
    // Bounded: a clip dragged to an hour over a loop of a sixteenth is not
    // something to hand the audio thread.
    let passes: usize = if clip.loop_s > 1.0e-6 {
        ((clip.len_s / clip.loop_s).ceil() as usize).clamp(1, 4_096)
    } else {
        1
    };
    let mut out: Vec<crate::instrument::ClipNote> = (0..passes)
        .flat_map(|pass| {
            let shift = clip.loop_s * pass as f64;
            clip.notes.iter().filter_map(move |n| {
                let at = n.at_s - clip.offset_s + shift;
                if at < -1.0e-9 {
                    return None;
                }
                // A repeat beginning past the clip's own end is not played:
                // the last pass is usually a partial one.
                if clip.loop_s > 1.0e-6 && at >= clip.len_s - 1.0e-9 {
                    return None;
                }
                // **The length is the END minus the START, both rounded
                // once from absolute time.**
                //
                // Rounding `at` and `dur` SEPARATELY and adding them is not
                // the same number: 6.153846 s rounds to frame 295385 and
                // 1.025641 s to 49231 frames, so the note went off at
                // 344616 — while the chord that begins exactly where it ends
                // rounds to 344615 and went ON one frame EARLIER. Every
                // pitch the two chords shared was switched off a sample
                // after it started, which is the owner's "the 3rd chords get
                // most notes cut off and sound like they're choked."
                //
                // Taking the difference of two absolute roundings makes a
                // note's end land on precisely the frame the next note's
                // start does, which is the case `mix_channels` already
                // handles by emitting the offs before the ons.
                let on = frames(at);
                Some(crate::instrument::ClipNote {
                    at: on,
                    dur: frames(at + n.dur_s).saturating_sub(on).max(1),
                    key: n.key.min(127),
                    vel: n.vel.min(127),
                })
            })
        })
        .collect();
    // The walk in `mix_channels` needs them sorted.
    out.sort_by_key(|n| n.at);
    out
}

#[cfg(feature = "recorder")]
/// **An audio clip's passes, with its loop spelled out**, one `Placed` each.
///
/// The twin of [`midi_clip_notes`], and it exists for the same reason: two
/// customers (playback and the offline bounce) and a schedule built twice is a
/// schedule that eventually disagrees with itself.
///
/// It had already disagreed with the MIDI one. The loop was spelled out for
/// MIDI clips and NOT for audio, so a session with two of each looped half its
/// tracks and played the other half once — the owner's "Looping misses 2/4
/// tracks when it loops. i can't tell why not all of them."
///
/// Every pass reads the SAME `offset` into the file: a repeat is the trimmed
/// span played again, not the file continuing. The fades belong to the clip
/// rather than to a pass, so the head fade is on the first and the tail fade
/// on the last; a fade on every repeat would be a tremolo nobody asked for.
fn audio_clip_passes(
    clip: &tangent_ui::arrangement::ArrClip,
    decoded: &std::sync::Arc<tangent_record::decode::Clip>,
    rate: f64,
    gain: f32,
) -> Vec<crate::instrument::Placed> {
    let frames = |s: f64| frames_at(rate, s);
    let len_s = clip.play_len_s(decoded.seconds());
    // Bounded exactly as the MIDI expansion is: an hour of clip over a
    // sixteenth of loop is not something to hand the audio thread.
    let passes: usize = if clip.loop_s > 1.0e-6 {
        ((len_s / clip.loop_s).ceil() as usize).clamp(1, 4_096)
    } else {
        1
    };
    let mut out = Vec::with_capacity(passes);
    for pass in 0..passes {
        let at = clip.loop_s * pass as f64;
        // The last pass is usually a PARTIAL one — the clip's own end is
        // the wall, not the loop's length.
        let this = if passes > 1 {
            clip.loop_s.min(len_s - at)
        } else {
            len_s
        };
        if this <= 1.0e-9 {
            break;
        }
        out.push(crate::instrument::Placed {
            clip: std::sync::Arc::clone(decoded),
            strip: clip.lane.min(tangent_ui::recorder::CHANNELS - 1),
            start: frames(clip.start_s + at),
            offset: frames(clip.offset_s),
            len: frames(this),
            fade_in: if pass == 0 { frames(clip.fade_in_s) } else { 0 },
            fade_out: if pass + 1 == passes {
                frames(clip.fade_out_s)
            } else {
                0
            },
            gain,
        });
    }
    out
}

/// The stretched copy of a decoded clip, if the clip asks for one — the ONE
/// place the answer comes from, for the live path and the offline bounce
/// alike. It lived inline in `reload_clips` only, so the export rendered the
/// unstretched audio under the clip's drawn window: wrong speed, wrong
/// pitch, and an open-ended clip came back at `file_s / ratio` of its heard
/// length. The same divergence class the export already paid for once with
/// `clip.gain`.
///
/// The cache is the CALLER's: the live path owns `self.stretch_cache`
/// (retained across reloads, swept beside `track_cache`), the bounce builds
/// a per-export one — its decodes are at the export rate, and stretched
/// copies of live-rate audio would be wrong-rate in the file (the rate rule
/// on the cache fields).
#[cfg(feature = "recorder")]
fn stretched_clip_audio(
    clip: &tangent_ui::arrangement::ArrClip,
    decoded: std::sync::Arc<tangent_record::decode::Clip>,
    path: &std::path::Path,
    cache: &mut std::collections::HashMap<
        (std::path::PathBuf, i64, String),
        std::sync::Arc<tangent_record::decode::Clip>,
    >,
) -> std::sync::Arc<tangent_record::decode::Clip> {
    if (clip.stretch - 1.0).abs() <= 1.0e-9 {
        return decoded;
    }
    let mode = crate::stretch::Mode::from_key(&clip.stretch_mode).unwrap_or_default();
    let key = (
        path.to_path_buf(),
        (clip.stretch * 1.0e6).round() as i64,
        mode.key().to_owned(),
    );
    match cache.get(&key) {
        Some(c) => std::sync::Arc::clone(c),
        None => {
            let made = std::sync::Arc::new(crate::stretch::stretch(&decoded, clip.stretch, mode));
            cache.insert(key, std::sync::Arc::clone(&made));
            made
        }
    }
}

// **This attribute belongs to the IMPL, not to what is above it.** It was
// stolen once: a helper inserted between it and the block below made every
// recorder method unconditional, and the Minimal build — `tangent` with no
// features, the fallback for a platform that cannot carry the rest — stopped
// compiling with five hundred errors. `scripts/check-firewall.sh` is what
// says so, and nothing else in the repo ever builds this configuration.
#[cfg(feature = "recorder")]
impl DesktopApp {
    /// Everything the band shows, refreshed from what is actually true.
    ///
    /// Pushed IN rather than pulled out, because `tangent-ui` cannot reach a
    /// device or a filesystem and must not learn how.
    fn fill_recorder_state(&mut self, ctx: &egui::Context) {
        use tangent_record::take;

        // The take that refused a reopen has ended, so take it now. Deferring
        // rather than dropping it is what keeps a device change made mid-take
        // from leaving the monitor dead for the rest of the session.
        if self.recorder.audio_path_deferred && !self.recorder.session.state().is_active() {
            self.open_audio_path(ctx);
        }

        let root = self.app.record_root();
        let spec = self.app.export_spec();
        let name = self.app.take_name().map(str::to_owned);

        // Free space, on a timer.
        let now = std::time::Instant::now();
        if self
            .recorder
            .disk_checked_at
            .is_none_or(|t| now.duration_since(t) >= DISK_RECHECK)
        {
            self.recorder.disk_checked_at = Some(now);
            self.recorder.disk_bytes = crate::record::available_bytes(&root);
        }

        // **Who is looking at the camera.**
        //
        // The conversion is the cost, and the capture thread was doing thirty
        // a second for as long as the band was open, whether or not anything
        // was looking. Measured on a 2013 MacBook Air: 35.6% of a core, idle,
        // with the pane hidden and no take rolling, on a machine that then
        // could not keep up with a take.
        //
        // **Three states and no numbers**, which is the fix for what the first
        // version of this got wrong. It sent a fixed ten a second for a
        // preview, on every host — and that number came from the Air's JPEG
        // decode. Only V4L2 does a JPEG decode; on macOS a conversion is a
        // BGRA-to-RGBA copy costing a fraction of a millisecond, so the cap
        // bought nothing and threw away two thirds of the preview's
        // smoothness. What a preview can afford is now measured where the
        // conversions happen. See `camera::FrameWant`.
        use tangent_record::camera::FrameWant;
        self.recorder.session.set_camera_want(
            if self.recorder.session.is_recording() && self.app.export_spec().video.wants_video() {
                FrameWant::Every
            } else if self.app.recorder_band_open() || self.app.camera_pane_showing() {
                FrameWant::Preview
            } else {
                FrameWant::None
            },
        );

        // The camera. Uploaded here rather than in `after_frame` because the
        // texture has to exist before the band that draws it is painted.
        if let Some(frame) = self.recorder.session.next_frame() {
            let size = [frame.width as usize, frame.height as usize];
            // `from_rgba_unmultiplied`, not `_premultiplied`: a camera frame is
            // opaque, so alpha is 255 everywhere and the two agree — but saying
            // premultiplied would be a claim about the data that happens to be
            // true, and it stops being true the moment anything composites.
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
            // Kept for the compositor, which ticks on the take's clock rather
            // than this one and will want a frame on a window frame that has
            // none of its own.
            //
            // **Moved, not cloned.** This was `frame.pixels.clone()`: a full
            // 3.7 MB copy per frame at 720p, on the UI thread, of pixels that
            // were about to be dropped anyway. And the buffer it displaces
            // goes BACK to the capture thread, which is what makes the
            // steady state allocation-free — see `FrameSlot::recycle`.
            let spent = self.recorder.camera_rgba.replace((
                frame.pixels,
                frame.width,
                frame.height,
                std::time::Instant::now(),
            ));
            if let Some((pixels, ..)) = spent {
                self.recorder.session.recycle_frame(pixels);
            }
            self.recorder.preview_px = egui::Vec2::new(frame.width as f32, frame.height as f32);
            match self.recorder.preview.as_mut() {
                // `set` reuses the GPU allocation; `load_texture` makes a new
                // one every frame, which at 30 fps is a texture leak with a
                // frame rate.
                Some(handle) => handle.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.recorder.preview = Some(ctx.load_texture(
                        "tangent-camera-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
        }

        let camera_uid = self.app.chosen_camera_uid().map(str::to_owned);
        // `camera_running`, not `camera_format().is_some()`. The format is
        // cached at open and never becomes `None`, so testing it could not
        // detect the case this exists for — a webcam unplugged mid-session left
        // its last frame on screen looking live, indefinitely.
        let camera_open = self.recorder.session.camera_running();
        // **Silent means "not sending a picture NOW", not "has never sent
        // one".**
        //
        // `Session::camera_silent` asks whether the device has EVER delivered
        // a frame, which cannot see the failure this was reported for: a
        // camera that opens, hands over one frame from its own stale buffer
        // and then stops. It had delivered one, so nothing here fired, and
        // the stale picture sat in the band and in every take's video looking
        // live.
        //
        // Asked of the picture in hand instead. `camera_silent` is kept in
        // the OR because it is true before the first frame arrives — when
        // there is no picture to have an age — and the three-second grace on
        // the message below is what covers an ordinary open.
        let no_fresh_frame = camera_open
            && !self
                .recorder
                .camera_rgba
                .as_ref()
                .is_some_and(|(.., at)| at.elapsed() <= CAMERA_FRAME_STALE_AFTER);
        if self.recorder.session.camera_silent() || no_fresh_frame {
            self.recorder
                .camera_silent_since
                .get_or_insert_with(std::time::Instant::now);
        } else {
            self.recorder.camera_silent_since = None;
        }
        if !camera_open {
            // Drop the stale picture when there is no camera behind it, or the
            // last frame of an unplugged webcam stays on screen looking live.
            self.recorder.preview = None;
            // **And the one the VIDEO uses**, which is the same rule and was
            // not applied to it. The preview has been protected since a
            // webcam was unplugged mid-session; the recording, which is the
            // copy that outlives the session, was not.
            if let Some((pixels, ..)) = self.recorder.camera_rgba.take() {
                self.recorder.session.recycle_frame(pixels);
            }
        }
        // A picture too old to be true goes as well, camera open or not.
        //
        // **A missing picture is diagnosable and a stale one is not.** A
        // black camera pane in a take is somebody asking why; a six-hour-old
        // frame of the same room is a take that looks fine until it does
        // not, and by then it has been sent somewhere.
        if self
            .recorder
            .camera_rgba
            .as_ref()
            .is_some_and(|(.., at)| at.elapsed() > CAMERA_FRAME_STALE_AFTER)
        {
            if let Some((pixels, ..)) = self.recorder.camera_rgba.take() {
                self.recorder.session.recycle_frame(pixels);
            }
        }

        // What both sides are doing, for the Audio Status panel. Pushed every
        // frame rather than fetched when the panel opens: a rate that changed
        // under you is exactly what it is there to show.
        self.app.set_audio_status(tangent_ui::recorder::AudioStatus {
            input: self.recorder.session.input_stats(),
            output: self.recorder.engine.as_ref().map(|e| {
                let o = e.output();
                (
                    o.device.clone(),
                    tangent_ui::recorder::StreamStats {
                        sample_rate: o.sample_rate,
                        channels: o.channels,
                        buffer_frames: o.buffer_frames,
                        // Counted in the callback, not asked for. See
                        // `StreamStats::observed_frames`.
                        observed_frames: e.observed_out_block(),
                        // Per HOST, not per platform — None under JACK and
                        // CoreAudio, where the request IS the callback.
                        buffer_ring: o.buffer_ring,
                    },
                )
            }),
            // Only while something is actually being monitored: a ring nobody
            // is listening to is not latency, it is a ring.
            monitor_ms: self
                .recorder
                .engine
                .as_ref()
                .filter(|_| self.app.input_monitor())
                .map(crate::instrument::Engine::monitor_backlog_ms),
            // **Read every frame, and cheaply.** `graph_clock` shells to
            // `pw-metadata` on Linux and answers `None` everywhere else, so
            // this is a process spawn per frame and that is too much — it is
            // cached in `Recorder::graph_floor` and refreshed on the same edge
            // that reopens the streams, which is the only moment it can have
            // changed for a reason this app cares about.
            graph_floor: self.recorder.graph_floor,
            // `None` off Linux and wherever the graph cannot be read — which
            // the panel must render as nothing, never as zero.
            dropouts: tangent_record::audio::merge_dropouts(
                self.recorder.graph_dropouts.latest(),
                self.recorder.engine.as_ref().map_or(0, |e| e.xruns()),
            ),
        });

        // The buffer size, on the edge. Reopening both streams is expensive —
        // the output takes every instrument with it — so it happens when the
        // choice CHANGES and not while a take is rolling.
        // **A dead output announces itself and comes back.** Two ways an
        // open stream dies without anybody asking: the backend admits it
        // (`fault`, via the error callback), or it just stops serving
        // callbacks while claiming to run — which is what an ASIO driver
        // unloaded beneath a live stream looks like, and what the owner met
        // as total silence with an empty console. Either way the engine is
        // torn down LOUDLY (the band shows `engine_error`) and the standing
        // retry path below rebuilds it through the same close-input-first
        // door every other reopen uses.
        if let Some(engine) = self.recorder.engine.as_ref() {
            let fault = engine.fault();
            let (running, callbacks) = engine.health();
            // **The dropout counter, actually read.**
            //
            // `Engine::xruns` exists because the ALSA backend used to swallow
            // underruns entirely (`vendor/cpal/src/host/alsa/mod.rs`), so a
            // glitch was audible in the room and invisible to the program. It
            // was then counted and READ BY NOTHING, which is the same failure
            // one step later: a number nobody looks at is not instrumentation.
            //
            // Logged on the edge rather than per frame, and at warn level,
            // because this is the line somebody grepping a log after "it
            // crackled" needs to find. It is also the only place the two
            // buffer settings can be told apart by anything other than an ear.
            // Merged, not preferred: each counter has been measured blind on
            // one of the two paths this ships on, in opposite directions —
            // `merge_dropouts`'s doc carries both measurements.
            let xruns = tangent_record::audio::merge_dropouts(
                self.recorder.graph_dropouts.latest(),
                engine.xruns(),
            )
            .unwrap_or(0);
            // A reopen makes NEW nodes with fresh counters, so the number can
            // legitimately fall; chase it down rather than hiding every
            // dropout the new stream has until it beats the old one's total.
            if xruns < self.recorder.xruns_seen {
                self.recorder.xruns_seen = xruns;
            }
            if xruns > self.recorder.xruns_seen {
                log::warn!(
                    "audio dropouts: {} since this stream opened (+{} just now) - \
                     the backend recovered each one, and each is a gap in what \
                     was heard and recorded",
                    xruns,
                    xruns - self.recorder.xruns_seen,
                );
                self.recorder.xruns_seen = xruns;
            }
            let stalled =
                self.recorder
                    .engine_watch
                    .note(running, callbacks, std::time::Instant::now());
            if fault.is_some() || stalled {
                let why = match fault {
                    Some(f) => {
                        format!("audio output failed: {f} - will reopen when the device is back")
                    }
                    None => "the audio output went quiet (device stopped serving \
                             audio) - will reopen when the device is back"
                        .to_owned(),
                };
                log::warn!("{why}");
                self.recorder.engine = None;
                self.recorder.engine_error = Some(why);
                self.recorder.inserts_loaded =
                    vec![None; tangent_ui::recorder::INSERT_SPACE];
                // **No timed retries here — the reprobe thread owns recovery.**
                // The yank test proved why: a watchdog-declared death is
                // usually GONE HARDWARE, and `Engine::start` against a dead
                // ASIO driver blocks the UI thread for however long the
                // driver's own timeouts take. Five timed tries stacked to the
                // owner's 10-30 seconds of "Not Responding". The reprobe
                // watches from a thread that can afford to wait, and the one
                // UI-thread open happens only once hardware is actually there;
                // a transient stall passes its first probe immediately, so
                // recovery is still ~a frame away in the harmless case.
                self.recorder.engine_retry = None;
            }
        }

        // A monitor that would not open a moment ago. See `engine_retry`.
        if self.recorder.engine.is_none() && self.app.recorder_band_open() {
            if let Some((at, _)) = self.recorder.engine_retry {
                if std::time::Instant::now() >= at {
                    self.open_audio_path(ctx);
                }
            }
        }
        // **The auto recheck.** While the engine is down with its retries
        // spent — the watchdog skips them entirely, the timed loop burns
        // through them — a background probe watches for the hardware's
        // return, and the one UI-thread reopen happens the frame it is seen.
        // The probe flag flips between frames, so ask for a wakeup rather
        // than trusting the next paint to somebody moving the mouse.
        if self.recorder.engine.is_some() {
            self.recorder.reprobe = None;
        } else if self
            .recorder
            .reprobe
            .as_ref()
            .is_some_and(Reprobe::device_back)
        {
            // **Only while the band is open.** Without this gate the replug
            // fired `open_audio_path` regardless: device dies with the band
            // open, the user closes the band, plugs the device back in — and
            // the probe restarted the engine AND reopened the microphone with
            // nothing on screen saying so, exactly the "no device held" promise
            // the band-close edge makes. The close edge also drops the probe
            // now; this guard covers the one frame where the close and the
            // replug land together.
            if self.app.recorder_band_open() {
                self.recorder.reprobe = None;
                self.open_audio_path(ctx);
            }
        } else if self.recorder.reprobe.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        } else if self.app.recorder_band_open()
            && self.recorder.engine_error.is_some()
            && self.recorder.engine_retry.is_none()
        {
            self.recorder.reprobe = Some(Reprobe::spawn());
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        // Whether a multichannel interface lists its inputs one by one. Pushed
        // every frame rather than on an edge: it is one atomic store, and the
        // list it changes is built fresh every time the picker opens, so there
        // is no state to reconcile and nothing to reopen.

        let want_buffer = self.app.buffer_frames();
        let want_system = self.app.audio_system();
        // **The driver stack decides what Default resolves to**, so a change
        // of system throws the cached answer away before it is asked for
        // again — an ASIO driver and WASAPI do not have to agree about what
        // the output's own rate is.
        if self.recorder.system_open != want_system {
            self.recorder.default_rate = None;
        }
        // Resolved, not raw: the change detector has to compare the rate the
        // streams will actually OPEN at, or a Default that resolves to 48000
        // reads as "no rate" forever and the reopen never fires.
        let want_rate = self.effective_sample_rate();
        let path_changed = self.recorder.buffer_open != want_buffer
            || self.recorder.rate_open != want_rate
            || self.recorder.system_open != want_system;
        if path_changed && !self.recorder.session.is_recording() {
            self.recorder.buffer_open = want_buffer;
            self.recorder.rate_open = want_rate;
            self.recorder.system_open = want_system.clone();
            // The graph may have been reconfigured since the last open — by
            // the user editing PipeWire's config to fix exactly what this
            // reports, which is the case worth getting right.
            self.recorder.graph_floor =
                tangent_record::audio::graph_clock().map(|g| g.floor());
            // A new stream has a new counter; keeping the old high-water mark
            // would hide every dropout the next one has until it beat the last.
            self.recorder.xruns_seen = 0;
            // The system FIRST, and before anything is opened: every device
            // lookup in `tangent_record::audio` goes through it, so setting it
            // after the streams were built would open them on the old stack and
            // then list devices from the new one.
            tangent_record::audio::set_system(want_system.as_deref());
            // Dropping the engine stops the output stream, and STARTING ONE
            // AGAIN is not optional. `start_engine` otherwise runs only on the
            // edge of the band opening, so dropping it here left the app with
            // no monitor and no instrument until the band was closed and
            // reopened — which is not a thing anybody would think to try.
            //
            // The old stream's `Drop` releases the device before the new one
            // asks for it, which is the ordering CoreAudio needs. The
            // five-second instrument load still happens over the following
            // frames, announced as usual.
            self.recorder.engine = None;
            self.recorder.inserts_loaded =
                vec![None; tangent_ui::recorder::INSERT_SPACE];
            self.open_audio_path(ctx);
        }

        let audio_uid = self.app.chosen_audio_uid().map(str::to_owned);
        let open_name = self.recorder.session.audio_device_name().map(str::to_owned);
        // "Missing" is a chosen device that is not open, which is a different
        // thing from having chosen nothing — an interface unplugged between
        // sessions must not silently look like a choice nobody made.
        let audio_missing = audio_uid.is_some() && open_name.is_none();
        // What the one status line says, worst news first: a device that will
        // not open beats a device that is denied beats the last take's report.
        let message = self
            .recorder
            .camera_opening
            .then(|| {
                "starting the camera - this can take a few seconds on a USB \
                 webcam"
                    .to_owned()
            })
            // The ENGINE's story outranks the input's: a dead OUTPUT is the
            // whole app silent, where a missing input is one source quiet.
            // The yank test proved the old order backwards — the sticky
            // input-fallback note sat in front of "the audio output went
            // quiet", which was the news that mattered.
            .or_else(|| self.recorder.engine_error.clone())
            .or_else(|| self.recorder.session.audio_error().map(str::to_owned))
            .or_else(|| self.recorder.session.camera_error().map(str::to_owned))
            .or_else(|| {
                // Only once it has had a moment: every camera delivers nothing
                // for the first few frames after `startRunning` returns.
                (self.recorder.camera_silent_since.is_some_and(|t| {
                    std::time::Instant::now().duration_since(t)
                        > std::time::Duration::from_secs(3)
                }))
                .then(|| {
                    // Where the fix lives is not the same sentence on every
                    // platform, and naming the wrong panel is worse than
                    // naming none.
                    if cfg!(target_os = "macos") {
                        "the camera is open but sending no picture - check \
                         Camera access in System Settings > Privacy & Security, \
                         and whether another app is holding it"
                    } else if cfg!(target_os = "linux") {
                        "the camera is open but sending no picture - another \
                         program may be holding it, or the format may not be \
                         supported"
                    } else {
                        "the camera is open but sending no picture"
                    }
                    .to_owned()
                })
            })
            .or_else(|| {
                self.recorder
                    .camera_denied
                    .lock()
                    .ok()
                    .and_then(|d| d.clone())
            })
            // **And what this take is going to be missing.**
            //
            // Mute is the only thing that decides what a take is made of now,
            // which is one rule where there used to be four — but a silent
            // rule that costs somebody a take is no better than the setting it
            // replaced. So a muted source with something in it says so, once,
            // while there is still a take to save.
            //
            // Only while one is RUNNING. Muting the microphone to practise is
            // a normal thing to do and a band that complained about it all
            // afternoon would teach everybody to ignore the line that also
            // says the camera was denied.
            .or_else(|| self.muted_out_of_the_take())
            // And what the LAST take was worth saying, which is the only thing
            // here that is about the past — so it goes last.
            .or_else(|| self.recorder.take_note.clone())
            // **The take's report is not here any more.** It was the last
            // `or_else` in this chain, which meant it sat in a one-line strip
            // competing with live errors and stayed up until something else
            // replaced it. It is a dialog now — see `report_take` below and
            // `Dialog::TakeSummary`. What remains in this chain is the live
            // problems, which belong in the band because they are true NOW.
            ;

        let preview = self
            .recorder
            .preview
            .as_ref()
            .map(|h| tangent_ui::recorder::Preview {
                texture: h.id(),
                size: self.recorder.preview_px,
            });

        // **Once per take, on the edge.** `last_summary` keeps answering with
        // the same take until the next one, so the message it carries is the
        // trigger: folders are timestamped, so two takes never produce the
        // same one. Before the state borrow, because raising a dialog is
        // another `&mut self.app`.
        if let Some(summary) = self.recorder.session.last_summary() {
            let (message, problem) = (summary.message(), summary.is_problem());
            let silent = summary.is_silent();
            if self.reported_take.as_deref() != Some(message.as_str()) {
                self.reported_take = Some(message.clone());
                // **A silent take gets a line, not a modal.** It is worth
                // saying — "I recorded silence" is the failure this recorder
                // exists to prevent — and it is not worth a window that
                // swallows every event until it is found and closed. See
                // `Summary::is_problem`.
                if silent {
                    self.recorder.take_note = Some(
                        "that take is silent - check the input, and the mute \
                         buttons in the mixer"
                            .to_owned(),
                    );
                }
                self.app.report_take(message, problem);
            }
        }
        // The drumpad's peaks, read-and-reset like the strips' — exactly
        // once a frame, and pushed before the state borrow below takes the
        // app for the rest of this function.
        if let Some(e) = self.recorder.engine.as_ref() {
            // **The pads light for the kit the window is SHOWING.** With a
            // full instance per channel there is a meter per lane, and the
            // window is looking at one of them — the first, until it learns
            // to choose.
            let lane = (0..tangent_ui::recorder::CHANNELS)
                .find(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
                .unwrap_or(0);
            self.app.note_drumpad_peaks(e.pad_peaks(lane));
        }
        // **The alias replaces the interface's name, once, for all of it.**
        // Read BEFORE the state borrow, like everything else here: the app is
        // borrowed mutably for the rest of this function.
        let alias = self.app.input_alias().trim().to_owned();
        // **The scope's tap, drained dry once a frame** — the speakers' mix
        // without the click, straight to the widget. `dt` is the audio time
        // the batch spans, which is what the auto-gain release wants.
        if let Some(e) = self.recorder.engine.as_mut() {
            self.scope_buf.clear();
            e.drain_scope(&mut self.scope_buf);
            let rate = e.output().sample_rate as f32;
            if !self.scope_buf.is_empty() {
                let dt = self.scope_buf.len() as f32 / 2.0 / rate.max(1.0);
                self.app.scope_feed(&self.scope_buf, dt, rate);
            }
        }
        // **The comp faces' gain-reduction needles**, one read per frame per
        // loaded comp bay — the read RESETS the engine-side max-hold, so it
        // must happen exactly once a frame and here is that once (the
        // `strip_peaks` rule, per bay). The walk is over the same ledger the
        // reconcile diffs every frame anyway, and a desk has a handful of
        // comps on twelve thousand mostly-`None` rows.
        if let Some(e) = self.recorder.engine.as_ref() {
            let inserts = tangent_ui::recorder::INSERTS;
            for (i, loaded) in self.recorder.inserts_loaded.iter().enumerate() {
                if loaded.as_deref() == Some(crate::builtin_fx::COMP_PATH) {
                    self.app
                        .set_insert_gr(i / inserts, i % inserts, e.insert_gr_db(i / inserts, i % inserts));
                }
            }
        }
        // **The denoise learn drain**, same cadence and same reason as the
        // needles above: while a learn runs (and on the frame after it
        // freezes) the audio thread publishes thresholds into one shared
        // slot, and this pulls them into the bay's durable ledger so the
        // next save — or the session's autosave — carries what was learned.
        // Idle cost is one atomic swap answering "nobody".
        if let Some(e) = self.recorder.engine.as_mut() {
            e.adopt_learned_thresholds();
        }
        // **The INSERT tap, drained dry once a frame — but only while a face
        // has it armed.** Disarmed, the engine pushes nothing and this skips
        // even the empty pop-loop, so an idle session pays nothing at all.
        if self.insert_tap_at.is_some() {
            if let Some(e) = self.recorder.engine.as_mut() {
                self.tap_buf.clear();
                e.drain_insert_tap(&mut self.tap_buf);
                let rate = e.output().sample_rate as f32;
                if !self.tap_buf.is_empty() {
                    self.app.insert_tap_feed(rate, &self.tap_buf);
                }
            }
        }
        // The IR road's own frame work: levels while the wizard meters, a
        // finished capture handed to the worker, the worker's answer landed
        // — and, first time through, the shelf's launch scan.
        if !self.ir.scanned {
            self.ir.scanned = true;
            self.push_ir_rows();
        }
        self.ir_tick();
        // **Read before the state is borrowed mutably.** Both are pure
        // functions of the settings, so hoisting them changes nothing but
        // the borrow.
        let project_dest = shorten_home(&self.app.project_root());
        let export_dest = shorten_home(&self.app.export_root());
        let preset_dest = shorten_home(&self.app.preset_root());
        let irs_dest = shorten_home(&self.app.irs_root());
        let state = self.app.recorder_state_mut();
        state.preview = preview;
        state.camera_name = self
            .recorder
            .session
            .camera_format()
            .map(|f| format!("{}x{} @ {:.0}fps", f.width, f.height, f.fps))
            .or_else(|| camera_uid.clone());
        state.camera_missing = camera_uid.is_some() && !camera_open;
        state.state = self.recorder.session.state();
        state.elapsed_s = self.recorder.session.elapsed();
        state.meters = self.recorder.session.meters();
        // The output, which is a different signal: the VU shows what is being
        // recorded and this shows what leaves. On a machine with an interface
        // plugged in those are not the same thing at all.
        if let Some(e) = self.recorder.engine.as_ref() {
            state.master = e.meters();
            state.gr_db = e.gain_reduction_db();
            let (m, st, i) = e.loudness();
            // **Read every frame and NOT reset on read**, unlike the peaks
            // above: the two sliding windows are still true between frames
            // and the integrated one is the whole programme. A read that
            // cleared them would make the ladder flicker at frame rate.
            state.loudness.momentary = m;
            state.loudness.short = st;
            state.loudness.integrated = i;
            state.loudness.pair = e.loudness_stereo();
            // What the timeline is playing, lane by lane — the half the
            // theory view could never see. See `Engine::lane_sounding`.
            state.lane_sounding = e.lane_sounding();
            // The desk's own meters. Read-and-reset on the engine's side, so
            // this has to happen exactly once a frame and here is that once.
            state.strip_peaks = e.strip_peaks();
            // **And the VU falls back to it when nothing else is feeding.**
            // The band's meter shows what is being recorded: the input when
            // there is one, and otherwise the instrument. With no interface
            // selected neither of the session's two sources exists, so it
            // answered SILENT — a dead needle and a clip lamp that could never
            // light, on a Mac with a piano plugged into it and the FM playing.
            // Read ONCE, because the peaks clear on read: the same numbers go
            // to both meters rather than one of them getting zero.
            if !self.recorder.session.has_meter_source() {
                state.meters = state.master;
            }
            // **And the lamp means "something clipped", whatever the needle is
            // showing.** With an input selected the VU meters the INPUT, so a
            // built-in FM driven into the ceiling clipped the output and lit
            // nothing at all — "choose a mic and clipping is not possible",
            // on both platforms, for the same reason. The needle still answers
            // one question; the lamp beside it was always the other one.
            state.meters.clipped |= state.master.clipped;
        } else {
            state.master = tangent_ui::recorder::Meters::SILENT;
            state.gr_db = 0.0;
            state.loudness = tangent_ui::recorder::Loudness::default();
            state.lane_sounding = [0; tangent_ui::recorder::CHANNELS];
        }
        // **Levels with nothing plugged in, for looking at the desk.**
        //
        // Every meter here comes from the engine, so a machine with no
        // interface and no instrument shows twelve strips of silence — and a
        // meter, a scale and a gain-reduction bar are exactly the things that
        // have to be LOOKED at rather than reasoned about. Pushed after the
        // engine's own numbers, because it is overriding them.
        //   IVORY_DEMO_LEVELS=1 /Applications/Tangent.app/Contents/MacOS/tangent
        if std::env::var("IVORY_DEMO_LEVELS").is_ok() {
            for (i, peaks) in state.strip_peaks.iter_mut().enumerate() {
                // Different on every channel and different on the two sides of
                // each, so a bar drawn from the wrong lane is visible.
                let l = 0.12 + 0.13 * (i % 6) as f32;
                *peaks = [l, l * 0.55];
            }
            state.gr_db = 7.5;
            // A loudness reading to look at too, and a real one: −13.8 is a
            // master a hair over the streaming target, which is the reading
            // the ladder most needs to be legible at.
            state.loudness = tangent_ui::recorder::Loudness {
                momentary: -11.2,
                short: -12.6,
                integrated: -13.8,
                ..tangent_ui::recorder::Loudness::default()
            };
            state.master = tangent_ui::recorder::Meters {
                left: tangent_ui::recorder::Level {
                    peak: 0.79,
                    rms: 0.5,
                    hold: 0.9,
                    vu: 0.0,
                },
                right: tangent_ui::recorder::Level {
                    peak: 0.44,
                    rms: 0.3,
                    hold: 0.55,
                    vu: 0.0,
                },
                mono: false,
                clipped: false,
            };
        }
        state.dest = shorten_home(&root);
        // The other two standing folders, for the settings panel's PROJECT
        // tier. Shortened the same way, because a full path in a half-row
        // box is a path nobody can read the end of.
        state.project_dest = project_dest;
        state.export_dest = export_dest;
        state.preset_dest = preset_dest;
        state.irs_dest = irs_dest;
        state.folder_preview = take::folder_name(
            &take::WallTime::now_utc(),
            name.as_deref().and_then(take::sanitise_slug).as_deref(),
        );
        state.audio_name = open_name.or(audio_uid);
        // The playhead and the audition, as plain data. The rate is the
        // engine's own; with no engine the transport answers from its last
        // known truth, which is the whole reason it owns the number.
        state.playing = self.recorder.transport.playing();
        state.position_s = {
            let engine = self.recorder.engine.as_ref();
            let rate = engine.map_or(0.0, |e| f64::from(e.output().sample_rate));
            self.recorder.transport.position_s(engine, rate)
        };
        // **One strip per input, filled from the selection.** The names and
        // the widths come from the same function the picks do, so the column
        // the mixer draws and the channels the capture keeps cannot drift
        // apart — see `devices::open_inputs`.
        let open_inputs = crate::devices::open_inputs(&self.recorder.audio);
        // **The alias replaces the interface's name, once, for all of it.**
        // "Scarlett 18i20 USB  -  input 3" does not fit a mixer strip; "x - 3"
        // does, and the half worth shortening is the one that is the same on
        // every channel of the box.
        for i in 0..tangent_ui::recorder::INPUTS {
            let (device, channel, stereo) = open_inputs
                .get(i)
                .cloned()
                .unwrap_or_else(|| (String::new(), String::new(), false));
            let label = if device.is_empty() {
                String::new()
            } else {
                let head = if alias.is_empty() {
                    device.as_str()
                } else {
                    alias.as_str()
                };
                if channel.is_empty() {
                    head.to_owned()
                } else {
                    format!("{head} - {channel}")
                }
            };
            self.recorder.input_names[i].clone_from(&label);
            state.inputs[i] = tangent_ui::recorder::InputState { label, stereo };
        }
        state.audio_missing = audio_missing;
        state.disk_minutes = self
            .recorder
            .disk_bytes
            .and_then(|b| tangent_ui::recorder::minutes_on_disk(b, &spec));
        state.message = message;
        state.clip_warning = self.recorder.session.clipped();
        // Cleared the moment a new take starts: "re-export the last take" stops
        // being a sensible offer once there is a take in progress.
        state.last_take_folder = (!self.recorder.session.is_recording())
            .then(|| {
                self.recorder
                    .session
                    .last_summary()
                    .filter(|s| s.problem.is_none())
                    .map(|s| s.folder.clone())
            })
            .flatten();
    }

    /// Everything that must happen OUTSIDE a frame: opening devices, raising
    /// native panels, creating directories.
    fn after_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        // **Nothing heavy until a frame has actually REACHED THE SCREEN.**
        //
        // eframe presents a frame when `update` RETURNS, and this runs inside
        // it — so every blocking thing here happens before the window has ever
        // been drawn. Loading one insert is enough: a the modelled piano takes seconds
        // to construct, and the first window draw waited on it, every launch.
        // The owner called it exactly — "it still waits for the vsts to load
        // before a single window draw every single time".
        //
        // The earlier "first frame at 140 ms" was measured at the TOP of
        // `update`, which is when the work STARTS, not when pixels appear.
        // It was the wrong number for the question.
        //
        // `painted_once` is set at the bottom of `update`, so the first pass
        // does nothing but paint: window and splash at once, and the desk
        // comes up behind it from the second pass on.
        if !self.painted_once {
            ctx.request_repaint();
            return;
        }
        use tangent_ui::recorder::RecorderRequest as R;

        // Dev hook, alongside IVORY_INLINE and IVORY_DEMO_NOTES: open slot 0's
        // editor once, a second in, without anybody clicking. It is how a
        // window-interaction bug gets reproduced and bisected from a script;
        // driving it by hand makes every measurement a different measurement.
        //   IVORY_OPEN_EDITOR=1 dist/Tangent.app/Contents/MacOS/tangent
        //
        // **`=<strip>,<bay>` names a different bay**, because bay 1 of channel
        // 1 holds the built-in on a fresh desk and therefore opens the PATCH
        // picker. The plugin picker is only ever reached through an EMPTY bay,
        // so with the hook pinned to slot 0 there was no way to put it on
        // screen from a script at all — which is the one thing the hook exists
        // to make possible.
        //   IVORY_OPEN_EDITOR=0,1 tangent      # channel 1, bay 2: empty
        let dev_editor_bay = match std::env::var("IVORY_OPEN_EDITOR").as_deref() {
            Ok("1") | Ok("patch") => Some((0, 0)),
            Ok(v) => v.split_once(',').and_then(|(s, b)| {
                Some((s.trim().parse().ok()?, b.trim().parse().ok()?))
            }),
            Err(_) => None,
        };
        if let (false, Some((dev_strip, dev_bay))) = (self.recorder.dev_editor_done, dev_editor_bay)
        {
            let due = self
                .recorder
                .dev_editor_at
                .get_or_insert_with(|| std::time::Instant::now() + DEV_EDITOR_DELAY);
            if std::time::Instant::now() >= *due {
                self.recorder.dev_editor_done = true;
                // Through the app's own gesture and not the engine directly,
                // so the hook opens whatever a CLICK would open — the built-in's
                // patch picker as readily as a VST3's window. A hook that called
                // the engine directly could only ever reproduce half the bugs.
                self.app.open_insert_face(dev_strip, dev_bay);
                // `=patch` goes one further, to the patch EDITOR, which is
                // otherwise two clicks in and cannot be reached from a script.
                if std::env::var("IVORY_OPEN_EDITOR").as_deref() == Ok("patch") {
                    self.app
                        .apply_dialog_action(tangent_ui::dialogs::DialogAction::EditPatch {
                            slot: 0,
                        });
                }
                eprintln!(
                    "IVORY_OPEN_EDITOR: opened channel {} bay {}",
                    dev_strip + 1,
                    dev_bay + 1
                );
            }
            ctx.request_repaint();
        }

        // The plugins' own windows, if any are open. Polled rather than
        // notified because the user closes one with the OS's close button,
        // which the plugin's view knows about and we only find out by asking.
        if let Some(e) = self.recorder.engine.as_mut() {
            let bays: Vec<(usize, usize)> = (0..=tangent_ui::recorder::STRIPS)
                .flat_map(|s| (0..tangent_ui::recorder::INSERTS).map(move |b| (s, b)))
                .collect();
            let was: Vec<bool> = bays
                .iter()
                .map(|&(st, b)| e.insert_editor_open(st, b))
                .collect();
            e.poll_editor();
            // **Ask the open plugins whether their latency moved.**
            //
            // Only while a window is (or was, this frame) open: `was` and
            // not the post-poll state, so the frame an editor closes on is
            // still covered. A preset click is the thing that changes a
            // plugin's latency, and clicking presets means looking at it.
            // See `Engine::poll_latency` for why not a timer and why not
            // on-close-only.
            if was.iter().any(|&w| w) && e.poll_latency() {
                // The engine republishes the alignment from the atomics on
                // its own next block; this is only so the READOUT catches up
                // in the same breath rather than on the next mouse move.
                ctx.request_repaint();
            }
            // An editor that has just closed is a preset that has just been
            // chosen. Save then, rather than only at quit, because the gap
            // between the two is where a force-quit loses the sound.
            let closed = bays.iter().zip(&was).any(|(&(st, b), &w)| {
                w && !self
                    .recorder
                    .engine
                    .as_ref()
                    .is_some_and(|e| e.insert_editor_open(st, b))
            });
            if closed {
                self.save_plugin_states();
            }
            // **While any plugin window is open, keep the frames coming.**
            // On Linux the poll above IS the plugin GUI's run loop — its
            // timers and X events fire from `Editor::closed` — so an app
            // that idles freezes every open editor. 16 ms is the cadence
            // plugin animations are written against; on the other platforms
            // the OS drives the windows and this only makes the close button
            // noticed within a frame rather than on the next mouse move.
            let any_open = self
                .recorder
                .engine
                .as_ref()
                .is_some_and(|e| bays.iter().any(|&(st, b)| e.insert_editor_open(st, b)));
            if any_open {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }

        // The always-on MIDI tap, drained whether or not a take is running.
        //
        // **This drain no longer SOUNDS anything.** For every release before
        // this one it forwarded each message to the engine — once per UI
        // frame, at the end of the egui pass, which put a whole repaint
        // interval between a key and its sound. The midir callback now plays
        // through `midi::MidiFastLane` straight into the audio callback
        // (drumpad mirror included), and this drain keeps only what actually
        // needs the UI thread: the arrangement's record and the take's .mid
        // (inside `pump_midi` itself). The lane learns the drumpad's state
        // here, at frame rate — exactly as fresh as the old mirror was.
        self.midi_lane.set_pads_live(self.app.drumpad_live());
        let mut arr = self.recorder.arr_rec.as_mut();
        self.recorder.session.pump_midi(|t, bytes| {
            // The arrangement's own collection: raw note edges, stamped
            // against the record's T0, on the clock the engine hears.
            if let Some(rec) = arr.as_deref_mut() {
                rec.note_event(t, bytes);
            }
        });
        // The capture rings, drained every frame a record is running — four
        // seconds of room in the engine, emptied sixty times a second.
        if let Some(rec) = self.recorder.arr_rec.as_mut() {
            if let Some(e) = self.recorder.engine.as_mut() {
                for (input, _lane, buf) in &mut rec.audio {
                    e.drain_capture(*input, buf);
                }
            }
        }
        // The red button's state, where the band can draw it — and the
        // record's PROGRESS, so the arrangement draws the take growing
        // behind the playhead: lanes, notes so far, keys still down
        // sounding to now.
        let progress = self.recorder.arr_rec.as_ref().map(|rec| {
            let now = self
                .recorder
                .engine
                .as_ref()
                .map_or(0.0, |e| (e.timebase().now() - rec.start_ns) as f64 / 1e9);
            let mut notes = rec.notes.clone();
            for (key, (at, vel)) in &rec.open {
                notes.push(tangent_ui::arrangement::ArrNote {
                    at_s: *at,
                    dur_s: (now - at).max(0.05),
                    key: *key,
                    vel: *vel,
                });
            }
            tangent_ui::recorder::ArrRecView {
                start_s: rec.start_s,
                lanes: rec
                    .midi_lanes
                    .iter()
                    .map(|l| (*l, true))
                    .chain(rec.audio.iter().map(|(_, l, _)| (*l, false)))
                    .collect(),
                notes,
            }
        });
        let state = self.app.recorder_state_mut();
        state.arr_recording = progress.is_some();
        state.arr_rec = progress;

        // Opening the BAND opens the input, not pressing Record — the meter
        // has to be live before arming, which is what kills the "I recorded
        // silence" failure class.
        let open = self.app.recorder_band_open();
        if open != self.recorder.band_was_open {
            self.recorder.band_was_open = open;
            if open {
                self.open_audio_path(ctx);
                self.reconcile_camera(true, ctx);
            } else {
                // A record cannot outlive its engine — the capture rings
                // live in it. Finished, not dropped: the clips land.
                self.finish_arr_record();
                self.recorder.session.close_input();
                self.recorder.session.close_camera();
                self.recorder.camera_opening = false;
                // Dropping it stops the output stream and unloads the plugin,
                // which is what "the band is closed" should mean: no device
                // held, no third-party code resident.
                self.recorder.engine = None;
                // The reprobe goes with it. It only exists because the engine
                // died while the band was OPEN; left alive past this edge it
                // kept polling for the lost device — and the replug then
                // reopened the engine and the microphone behind a closed band.
                // Recovery restarts from the open edge like everything else.
                self.recorder.reprobe = None;
                self.recorder.inserts_loaded =
                    vec![None; tangent_ui::recorder::INSERT_SPACE];
            }
        } else if open {
            self.reconcile_audio(false);
            self.reconcile_camera(false, ctx);
            self.push_monitor_settings();
        }
        // Beside the per-frame push and gated on change — see `push_maps`.
        self.push_maps();
        self.push_automation();
        self.push_auto_suspend();

        let root = self.app.record_root();
        let name = self.app.take_name().map(str::to_owned);

        // The count-in ends on the beat the player HEARD, not on the frame that
        // noticed it had. The audio thread knows that instant exactly — it
        // scheduled the click and it knows the device's output delay — and the
        // UI thread can only ever be a frame late and a buffer short.
        if let Some(downbeat) = self
            .recorder
            .engine
            .as_ref()
            .filter(|e| e.count_in_done())
            .and_then(|e| e.count_in_downbeat_ns())
        {
            // **Once per count-in, not once per frame.** `count_in_done` is a
            // LATCH: it stays true after the count finishes and is only cleared
            // when the next one starts. So this armed the session with the same
            // downbeat on every frame for the rest of the session — and the
            // next take that began WITHOUT a count-in took that stale instant
            // as its T0. With "record the count-in into the take" on, every
            // take starts with a count-in length of zero, so every take after
            // the first would have been timestamped from the first one's
            // downbeat: minutes of offset between the audio and the `.mid`.
            if self.recorder.armed_downbeat != Some(downbeat) {
                self.recorder.armed_downbeat = Some(downbeat);
                self.recorder.session.arm_at(downbeat);
            }
        }
        // The count-in, which is the one thing here that animates with no input
        // at all — so it also has to ask for the next frame.
        if self.recorder.session.tick(&root, name.as_deref()) {
            ctx.request_repaint();
        }
        // **The transport, reconciled AFTER the session has ticked** — the
        // rolling level is derived from the session's state, and publishing it
        // a frame early was how the take's first block started from the old
        // position. Rolling during the take proper, never the count-in: the
        // count-in is what counts you IN to the track, and a track that
        // started under it would have its downbeat a bar from the one being
        // clicked.
        {
            let session_rolling = matches!(
                self.recorder.session.state(),
                tangent_ui::recorder::RecordState::Rolling
            );
            let engine = self.recorder.engine.as_ref();
            self.recorder.transport.push(engine, session_rolling);
            // **The loop region, pushed every frame from the time
            // selection.** Frames off the engine's own rate; cleared to
            // (0,0) when there is no selection. Suppressed WHILE a take
            // records — a loop that yanked the playhead back mid-take would
            // overwrite what was just captured — the same guard the reorder
            // and locate paths keep.
            if let Some(e) = engine {
                let rate = f64::from(e.output().sample_rate).max(1.0);
                match self.app.loop_region().filter(|_| !session_rolling) {
                    Some((a, b)) => {
                        e.set_loop((a.max(0.0) * rate) as u64, (b.max(0.0) * rate) as u64)
                    }
                    None => e.set_loop(0, 0),
                }
            }
        }
        // And a repaint while the audition rolls, or the playhead only moves
        // when the mouse does.
        if self.recorder.transport.playing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        // And a repaint while anything is live, or the meter and the clock
        // update only when the mouse moves.
        if self.recorder.session.is_recording() || open {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        // The in-app browser's three messages, pumped out here with the native
        // dialogs because listing a directory is disk I/O and `tangent-ui` does
        // not do that.
        if let Some(dir) = self.app.take_browse_request() {
            let exts = self.browse_extensions.clone();
            let (entries, error) = Self::list_dir(&dir, &exts);
            self.app.set_browser(dir, entries, error);
        }
        if let Some((purpose, file)) = self.app.take_browsed_file() {
            self.finish_file_choice(purpose, &file);
        }
        if let Some((purpose, dir)) = self.app.take_browsed_dir() {
            self.finish_dir_choice(purpose, dir);
        }

        if let Some(request) = self.app.take_directory_request() {
            // The same one-frame settle the save panel keeps — see there.
            self.deferred_dir = Some(request);
            ctx.request_repaint();
            return;
        }
        if let Some(request) = self.deferred_dir.take() {
            if Self::picker_needs_windowed(ctx) {
                self.deferred_dir = Some(request);
                self.refullscreen = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                return;
            }
            // One frame for the dialogs to stop floating, as for files.
            if self.panel_armed {
                self.panel_armed = false;
            } else if self.app.native_panel_up() {
                self.panel_armed = true;
                self.deferred_dir = Some(request);
                ctx.request_repaint();
                return;
            }
            // No portal and no zenity: our own browser, as for files.
            if !Self::native_dialogs_work() {
                let at = request
                    .start_at
                    .filter(|p| p.is_dir())
                    .or_else(dirs::home_dir)
                    .unwrap_or_else(|| std::path::PathBuf::from("/"));
                self.browse_extensions = Vec::new();
                let (entries, _) = Self::list_dir(&at, &[]);
                self.app.native_panel_closed();
                self.app.open_browser(
                    request.title.clone(),
                    tangent_ui::dialogs::BrowseFor::Folder(request.purpose),
                    at,
                    entries,
                );
                return;
            }
            // Parented, for the reason `take_cartridge_request` gives: an
            // unparented panel can open behind the app on Windows.
            let mut dialog = rfd::FileDialog::new()
                .set_title(&request.title)
                .set_parent(frame);
            if let Some(start) = request.start_at.filter(|p| p.exists()) {
                dialog = dialog.set_directory(start);
            }
            let picked = dialog.pick_folder();
            self.app.native_panel_closed();
            if let Some(dir) = picked {
                self.finish_dir_choice(request.purpose, dir);
            }
            self.restore_fullscreen(ctx);
        }
        self.take_cartridge_request(ctx, frame);
        #[cfg(feature = "recorder")]
        self.take_save_panel(ctx, frame);
        self.run_project_request();
        #[cfg(feature = "recorder")]
        self.reconcile_reastream();
        #[cfg(feature = "recorder")]
        self.publish_device_outs();
        // Scanning reads directories, so it belongs out here with the other
        // things the UI is not allowed to do — and it runs AFTER the folder
        // picker above, so a folder added this frame is in the list this frame
        // rather than one frame later.
        if self.app.take_plugin_rescan() {
            let extra = self.app.plugin_folders();
            let report = tangent_host::scan::discover_reported(&extra);
            // One line per root on stderr — visible from a console launch —
            // and one honest sentence in the band. A rescan that found 0
            // was pixel-identical to one that never ran, and the Windows
            // tester could not tell which they had.
            for (root, fate) in &report.roots {
                match fate {
                    Ok(n) => eprintln!("vst3 scan: {} -> {n}", root.display()),
                    Err(e) => eprintln!("vst3 scan: {} -> {e}", root.display()),
                }
            }
            let mut note = format!("scan: {} VST3 found", report.found.len());
            if let Some(stale) = report.retired_seen.first() {
                // The retired plugin is CLEANUP, not a plugin — say where it
                // is and that the installers remove it now, so the user is
                // never left wondering why another DAW still lists it.
                note = format!(
                    "{note}. A retired Tangent plugin is still at {} - Tangent \
                     no longer ships or loads it; reinstalling Tangent (or \
                     deleting the bundle) removes it.",
                    stale.display()
                );
            }
            if report.found.is_empty() && report.vst2_seen > 0 {
                // **No false trail to a VST3 that does not exist.** This used to
                // send people to "ReaPlugs' VST3 build in Common Files\VST3" —
                // there is no such build. ReaPlugs (ReaStream included) ships
                // VST2 only, and Tangent hosts VST3. Say the true thing.
                note = format!(
                    "scan: 0 VST3 found. {} .dll files seen are VST2, which \
                     Tangent does not host (it loads VST3 only).",
                    report.vst2_seen
                );
            }
            self.recorder.take_note = Some(note);
            self.app
                .set_plugin_list(crate::plugin_catalog_from(report.found));
        }
        // **The user's effect, across the effects bus.**
        //
        // Reconciled here with the plugin rack and for the same reasons:
        // `Module::open` runs somebody else's initialiser and `Instance::create`
        // can take seconds, so it happens after a frame and never inside one.
        // Once per change, because `bus_effect_loaded` remembers what was asked
        // for whether or not it worked — a plugin that will not load must not
        // be retried sixty times a second.
        // **ONE per call, like the instrument rack**, and for the same reason:
        // loading blocks for seconds, and thirty-nine slots reconciled in one
        // frame would be a window that stopped for a minute. Each is
        // remembered whether or not it worked, so a plugin that will not load
        // is not retried sixty times a second.
        // **A track reorder moves the plugin-state sidecars with it** —
        // BEFORE the reconcile below, or the racks reload against the old
        // rows' knobs. Live states written down first, so what moves is
        // what was sounding.
        // **The master's mono fold**, pushed like every other desk setting:
        // the app owns the choice, the engine owns the buffer. Cheap enough
        // to send unconditionally — one relaxed store against a branch and a
        // compare.
        if let Some(e) = self.recorder.engine.as_ref() {
            e.set_master_mono(self.app.master_mono());
        }
        if let Some(order) = self.app.take_desk_state_perm() {
            // **Only the rows this permutation touches.** See
            // `save_plugin_states_as_loaded_for`.
            let mut moved = vec![false; tangent_ui::recorder::TOTAL_SEATS];
            for (new, old) in order.iter().enumerate() {
                if new != *old {
                    // The channel row, and its folder-extra seat - the
                    // second rack travels with the track, so its state
                    // must be written down before the move like the
                    // first one's.
                    for seat in [
                        new,
                        *old,
                        tangent_ui::recorder::folder_extra_seat(new),
                        tangent_ui::recorder::folder_extra_seat(*old),
                    ] {
                        if let Some(m) = moved.get_mut(seat) {
                            *m = true;
                        }
                    }
                }
            }
            // **Carry the plugins across instead of rebuilding them.**
            //
            // A track that moves takes its plugins with it, and the instance
            // the new seat wants is the one the old seat has. Moving it is
            // two ring pushes; rebuilding it is a bundle scan, an
            // instantiation and a state restore, one bay per frame — which
            // is the spinning wheel. Whatever could not be carried comes
            // back here and is rebuilt the slow way.
            let stranded: Vec<usize> = self
                .recorder
                .engine
                .as_mut()
                .map(|e| e.permute_channel_inserts(&order))
                .unwrap_or_default();
            // The host's own copy of what sits in each seat travels too, or
            // the reconcile below compares the new seat against the old
            // seat's plugin and reloads everything it just carried.
            // The channel order lifted onto the whole seat space, so each
            // folder-extra seat follows its channel through every copy of
            // the per-seat state - see `recorder::seat_order`.
            let full = tangent_ui::recorder::seat_order(&order);
            permute_loaded_rows(&mut self.recorder.inserts_loaded, &full);
            self.save_plugin_states_as_loaded_for(Some(&moved));
            permute_insert_states_in(
                insert_state_path(0, 0)
                    .parent()
                    .map(std::path::Path::to_path_buf),
                &full,
            );
            permute_insert_states_in(self.project_state_dir(), &full);
            // **Only the rows that MOVED**, which is why this is not
            // `force_insert_reload`.
            //
            // That marks every one of the ~450 bays on the desk — sixteen
            // channels, the buses, the master and 128 drum pads — and the
            // reconcile below reloads exactly one mismatched bay per frame.
            // So a reorder tore down and re-instantiated every plugin in the
            // session, one per frame, each one scanning a bundle and
            // restoring its state: the owner's "when a reorder occurs of any
            // kind -- massive delay". A row that did not move has the same
            // plugin with the same state and nothing to do.
            // **Only what could not be carried.** A seat whose plugin
            // moved is already correct: same instance, same state, new
            // index. Marking it would undo the whole point.
            let bays = tangent_ui::recorder::INSERTS;
            for seat in stranded {
                for b in 0..bays {
                    if let Some(slot) = self.recorder.inserts_loaded.get_mut(seat * bays + b) {
                        *slot = Some("\u{1f}reload".to_owned());
                    }
                }
            }
            // **The faces follow their seats.** The app's face mirror is
            // keyed by seat, and a carried instance changed seat without a
            // reload — nothing below will re-push it. Only the moved rows;
            // an unmoved face is already right.
            if let Some(e) = self.recorder.engine.as_ref() {
                for (seat, m) in moved.iter().enumerate() {
                    if *m {
                        push_builtin_faces(&mut self.app, e, seat);
                    }
                }
            }
        }
        // **A pasted track's plugin STATE — BEFORE the bay reconcile below.**
        //
        // The copy itself is the app's (kind, levels, bays, clips), but the
        // state a plugin saved lives in a file keyed by (strip, bay) and
        // nothing in `tangent-ui` may reach a file.
        //
        // The ORDER is the whole of it. This sat a hundred lines further
        // down, after the reconcile that loads the bay — so the pasted
        // instance was created, handed `saved_insert_state` for a file that
        // did not exist yet, and came up at the plugin's DEFAULTS. And it
        // stayed there: the reconcile only fires when a bay's WANTED path
        // stops matching its loaded one, and a state file appearing later
        // changes neither. A pasted drum sampler with no kit takes MIDI,
        // lights its pads and makes no sound.
        if let Some((from, to)) = self.app.take_insert_state_copy() {
            let (from, to) = (
                tangent_ui::recorder::Strip::Channel(from).index(),
                tangent_ui::recorder::Strip::Channel(to).index(),
            );
            for bay in 0..tangent_ui::recorder::INSERTS {
                let src = insert_state_path(from, bay);
                let dst = insert_state_path(to, bay);
                // A bay the source never used leaves the destination's own
                // file alone rather than deleting it: the paste has already
                // cleared that bay through `set_insert`, and a stale file
                // whose owner does not match is ignored by
                // `saved_insert_state` anyway.
                if src.exists() {
                    let _ = std::fs::copy(&src, &dst);
                }
            }
        }
        let inserts = tangent_ui::recorder::INSERTS;
        let want_at = |i: usize| self.app.insert(i / inserts, i % inserts).map(str::to_owned);
        if let Some(i) = (0..self.recorder.inserts_loaded.len())
            .find(|i| want_at(*i) != self.recorder.inserts_loaded[*i])
        {
            let want = want_at(i);
            self.recorder.inserts_loaded[i] = want.clone();
            // **THE DEFAULT PEDALBOARD.** The amp landing in a CHANNEL bay
            // whose pedal slots have never been touched seeds the classic
            // board, comp to plate, through `set_insert` — the same
            // settings road the track-paste above takes, so each pedal then
            // loads through this very reconcile, one bay a frame. The
            // decision and its once-only marker live in
            // [`amp_board_to_seed`].
            if want.as_deref() == Some(crate::builtin_fx::AMP_PATH) {
                let ch = i / inserts;
                let seed = amp_board_to_seed(ch, |pedal, bay| {
                    self.app
                        .insert(tangent_ui::recorder::amp_pedal_seat(ch, pedal), bay)
                        .is_some()
                });
                if let Some(board) = seed {
                    for (pedal, path) in board.iter().enumerate() {
                        self.app.set_insert(
                            tangent_ui::recorder::amp_pedal_seat(ch, pedal),
                            0,
                            Some(path),
                        );
                    }
                }
            }
            if let Some(e) = self.recorder.engine.as_mut() {
                // **A built-in INSTRUMENT is not a bundle.** Its sentinel
                // path rides in the bay like any plugin's — that is what lets
                // it be picked, saved and named by the same code — but the
                // ENGINE renders it in `mix_channels`, not in the rack, so
                // the bay itself is handed nothing. See
                // `sync_instrument_lanes`, driven by the masks the per-frame
                // walk pushes. A built-in EFFECT sentinel passes through: the
                // engine seats those in the rack. See `is_instrument_sentinel`.
                let engine_want = want.as_deref().filter(|p| !is_instrument_sentinel(p));
                let path = engine_want.map(std::path::Path::new);
                // **With the bay's saved preset, when one belongs to this
                // plugin.** The same bargain the instrument slots have had for
                // a while and the racks never did: an effect whose settings
                // died at every relaunch, silently, because nothing ever asked
                // its instance for them. See `insert_state_path`.
                let state =
                    engine_want.and_then(|p| saved_insert_state(i / inserts, i % inserts, p));
                match e.load_insert(i / inserts, i % inserts, path, state.as_deref()) {
                    Ok(_) => {
                        self.recorder.engine_error = None;
                        // **What the bay now offers an automation lane**, read
                        // once, HERE.
                        //
                        // This is the only place a bay's contents change, so
                        // it is the only place the parameter list can go
                        // stale — and reading it per frame would be a COM
                        // call per parameter per plugin per frame, on the
                        // main thread, for a list that changes when somebody
                        // loads a plugin.
                        push_fx_params(&mut self.app, e, i / inserts, inserts);
                        // And the built-in faces on the same seat, values
                        // included — the face data has exactly the staleness
                        // window the parameter list has, so it travels with
                        // it.
                        push_builtin_faces(&mut self.app, e, i / inserts);
                        // **The window, the moment the load lands.** Only for
                        // the pick that just happened — the app's flag answers
                        // once and never for a launch restore — and only when
                        // something was actually loaded: an unload has no
                        // window to show.
                        if path.is_some()
                            && self.app.take_open_insert_on_load(i / inserts, i % inserts)
                        {
                            if let Err(err) = e.open_insert_editor(i / inserts, i % inserts) {
                                self.recorder.engine_error =
                                    Some(format!("could not open the plugin's window: {err}"));
                            }
                        }
                    }
                    Err(err) => {
                        // The auto-open belongs to a load that happened.
                        let _ = self.app.take_open_insert_on_load(i / inserts, i % inserts);
                        // A failed load leaves the bay empty, so the menu must
                        // not go on offering the last plugin's parameters.
                        push_fx_params(&mut self.app, e, i / inserts, inserts);
                        // Nor its face: the tombstone push clears it.
                        push_builtin_faces(&mut self.app, e, i / inserts);
                        let which = want
                            .as_deref()
                            .and_then(|p| p.rsplit(['/', '\\']).next())
                            .unwrap_or("that plugin");
                        let channel = tangent_ui::recorder::Strip::all()
                            .get(i / inserts)
                            .map_or_else(|| "the master".to_owned(), |s| s.label());
                        self.recorder.engine_error =
                            Some(format!("{which} did not load on {channel}: {err}"));
                    }
                }
            }
        }

        // **The document's clips, whenever an edit touched them.** The app
        // marks; this decodes, pushes the engine's schedule, and rebuilds
        // the band's picture — after the frame, like every other slow thing.
        // **...and whenever the ZOOM has moved far enough to matter.** A
        // waveform is drawn at a zoom: the pictures are made at the pixels
        // the clips occupy, so a view that has travelled needs new ones or
        // the audio is smeared across columns it does not belong in. See
        // `TangentApp::arr_waves_stale`, which is the threshold that keeps
        // this off the frame path during a drag.
        if self.app.take_clips_dirty() || self.app.arr_waves_stale() {
            self.reload_clips();
        }

        // **The drumpad's kit, on the same bargain**: a drop, a clear or a
        // project switch marks; this decodes and pushes the samples whole.
        if std::mem::take(&mut self.recorder.pads_retry) || self.app.take_drumpad_dirty() {
            self.reload_pads();
        }

        // **The click's sound, on the same bargain.** Decoded here, off the
        // audio thread, and handed over on the engine's own ring.
        if self.app.take_click_sample_dirty() {
            self.reload_click();
        }


        // **The shipped bank, put back.** The same call a first launch makes,
        // which is the point: there is one definition of "the cartridge that
        // ships" and both paths read it.
        if let Some(strip) = self.app.take_factory_cartridge() {
            let cart = crate::dx7::factory();
            self.app.set_cartridge(cartridge_info(&cart, "", true));
            // **And the sound changes now — on the strip that asked.**
            // Refilling the list without changing what is playing is a button
            // that appears to do nothing, which is the complaint this
            // answers. Patch 0, because the strip's setting was reset to it.
            if let Some(v) = cart.voices.first().copied() {
                if let Some(e) = self.recorder.engine.as_mut() {
                    e.set_builtin_voice(strip, v);
                }
            }
            self.cartridge = Some(cart);
        }

        // The take's video, on the EDGES of the session's own state. Placed
        // after the request loop so that a Record pressed this frame is already
        // rolling by the time this asks, and a Stop pressed this frame is
        // already stopped — the alternative is a video that starts and ends one
        // window frame late at both ends.
        {
            // **`is_writing`, not `is_recording`.** The latter is true through
            // the COUNT-IN, and during a count-in there is no take folder yet —
            // so `begin_video` found nothing to write to, gave up, and set the
            // flag that stops it trying again. Anybody with a count-in got no
            // video at all, every time, silently. It is also the right rule on
            // its own terms: the bars before the downbeat are deliberately not
            // in the audio, and they have no business being in the video.
            let writing = self.recorder.session.state().is_writing();
            if writing {
                self.begin_video(frame);
                self.pump_video();
            } else {
                self.end_video();
                self.recorder.video_tried = false;
            }
        }
        if let Some(path) = self.app.take_reveal_request() {
            reveal(&path);
        }
        // The automatic one, at most once per finished take. The marker is
        // updated whether or not the tick is on, so turning it on after a take
        // does not immediately open a window for a recording already made.
        self.tick_bounce();
        self.tick_offline_bounce();
        // The arrangement's count-in reaching its downbeat: begin.
        if self.recorder.arr_rec_wait
            && self
                .recorder
                .engine
                .as_ref()
                .is_some_and(crate::instrument::Engine::count_in_done)
        {
            self.recorder.arr_rec_wait = false;
            self.start_arr_record();
        }
        if !self.recorder.session.is_recording() {
            if let Some(folder) = self
                .recorder
                .session
                .last_summary()
                .filter(|s| s.problem.is_none())
                .map(|s| s.folder.clone())
                .filter(|f| !f.is_empty())
            {
                if self.recorder.seen_take.as_deref() != Some(folder.as_str()) {
                    self.recorder.seen_take = Some(folder.clone());
                    if self.app.record_open_when_done() {
                        reveal(&self.app.record_root().join(&folder));
                    }
                }
            }
        }

        // The × on the status row. The message is the host's, so the host is
        // what puts it away — see `TangentApp::take_dismiss_message`.
        if self.app.take_dismiss_message() {
            self.recorder.take_note = None;
            // **And the engine's error, which is what is usually showing.**
            // "Pro-R 2 is an effect, not an instrument" is a correct refusal
            // and a finished conversation — the user has read it and there is
            // nothing to do — but nothing in that path ever cleared it, so it
            // sat there with an × beside it that did not apply to it. An ×
            // that dismisses one message and not the one under the cursor is
            // worse than no × at all.
            self.recorder.engine_error = None;
            // And the INPUT error, which outranks both in the banner's
            // priority order — with it uncleared the × was dead exactly
            // when the message was the sticky "is not connected".
            self.recorder.session.dismiss_audio_error();
        }
        while let Some(request) = self.app.take_recorder_request() {
            match request {
                // The transport's own two. Handled here rather than queued
                // deeper: `Transport` is plain state, and the reconcile in
                // `push_transport` is what carries it to the engine.
                R::Play => {
                    // PLAY only — pausing is the pause button's job now.
                    // Record owns the transport while a take is writing, and
                    // the arrangement's record owns it the same way.
                    if !self.recorder.session.is_recording()
                        && self.recorder.arr_rec.is_none()
                        && !self.recorder.transport.playing()
                    {
                        self.recorder.transport.toggle_play();
                    }
                }
                R::Pause => {
                    if !self.recorder.session.is_recording()
                        && self.recorder.arr_rec.is_none()
                        && self.recorder.transport.playing()
                    {
                        self.recorder.transport.toggle_play();
                    }
                }
                R::Locate(seconds) => {
                    self.recorder.transport.locate(seconds);
                }
                // **The red button is the ARRANGEMENT's.** Armed lanes in,
                // clips out; the take machinery does not stir. The capture —
                // the cassette — is where the old take toggle lives now.
                R::Toggle => {
                    self.recorder.take_note = None;
                    if self.recorder.arr_rec_wait {
                        // Pressed again mid-count: the count stops, nothing
                        // records.
                        self.recorder.arr_rec_wait = false;
                        if let Some(e) = self.recorder.engine.as_ref() {
                            e.cancel_count_in();
                        }
                    } else if self.recorder.arr_rec.is_some() {
                        self.finish_arr_record();
                    } else {
                        // **The count-in counts the ARRANGEMENT in too** —
                        // the owner's ask: the bars from the settings click
                        // first, and the record begins on the downbeat.
                        let beats = self.app.count_in_beats();
                        if beats > 0 && !self.recorder.transport.playing() {
                            if let Some(e) = self.recorder.engine.as_ref() {
                                e.start_count_in(beats, self.app.tempo_bpm());
                                self.recorder.arr_rec_wait = true;
                            } else {
                                self.start_arr_record();
                            }
                        } else {
                            self.start_arr_record();
                        }
                    }
                }
                R::Capture => {
                    // **Whatever the last take had to say, it is not about
                    // this one.** Stale post-take advice that outlives the
                    // take it describes is how a line about dropped frames sat
                    // on screen for eight minutes across several takes.
                    self.recorder.take_note = None;
                    // The click counts the take in, on the audio thread's own
                    // sample clock. Started here, at the press, so the first
                    // beat lands immediately rather than a frame later.
                    let beats = self.app.count_in_beats();
                    let in_take = self.app.count_in_in_take();
                    if let Some(e) = self.recorder.engine.as_ref() {
                        // The click's own switch for the count, which is not
                        // `metronome_in_take` — see `Shared::count_in_in_take`.
                        e.set_count_in_in_take(in_take);
                        if !self.recorder.session.is_recording() && beats > 0 {
                            e.start_count_in(beats, self.app.tempo_bpm());
                        } else {
                            e.cancel_count_in();
                        }
                    }
                    // A fresh take starts with a clean clip latch on the
                    // instrument's meter, exactly as `LevelTracker::arm` does
                    // for the input's — a clip from the last take reported
                    // against this one is worse than no indicator.
                    if let Some(e) = self.recorder.engine.as_ref() {
                        e.clear_clip();
                    }
                    let spec = self.app.export_spec();
                    // **Zero when the count-in goes INSIDE the take.** The
                    // session's count-in is a delay before the file opens; the
                    // engine's is the click. Passing zero here starts writing
                    // at the press while the click counts on regardless, which
                    // is exactly "start instantly with the count-in in the
                    // export" — the count is at the head of the file to trim to
                    // or to keep.
                    let wait = if in_take { 0 } else { beats };
                    self.recorder
                        .session
                        .toggle(&root, name.as_deref(), wait, spec);
                }
                // **One stop for both machines.** The square ends whichever
                // is running — the take by the session's own stop, the
                // audition by the transport's — and `Transport::stop` is what
                // sends the playhead home. Play's own second press is a PAUSE
                // now, so this is the only button that returns to 0:00.
                R::Stop => {
                    // A count-in still clicking dies quietly with the rest.
                    if self.recorder.arr_rec_wait {
                        self.recorder.arr_rec_wait = false;
                        if let Some(e) = self.recorder.engine.as_ref() {
                            e.cancel_count_in();
                        }
                    }
                    // The arrangement record first, while the playhead still
                    // says where the take ended — `Transport::stop` rewinds.
                    self.finish_arr_record();
                    self.recorder.session.stop();
                    self.recorder.transport.stop();
                }
                // **Every latch, or the light does not go out.** The warning
                // is an OR across the input tracker, the take summary and the
                // instrument bus's own — and the VU paints itself red from the
                // first of those, so a dismiss that missed one would look like
                // a button that does nothing.
                R::DismissClip => {
                    self.recorder.session.clear_clip();
                    if let Some(e) = self.recorder.engine.as_ref() {
                        e.clear_clip();
                    }
                }
                R::NormalizeClips(ids) => self.normalize_clips(&ids),
                R::CopyInsert { from_strip, from_bay, to_strip } => {
                    self.copy_insert(from_strip, from_bay, to_strip);
                }
                R::ResetLoudness => {
                    if let Some(e) = self.recorder.engine.as_ref() {
                        e.reset_loudness();
                    }
                }
                R::Audition { notes, on } => {
                    // **All of them at one instant.** The events are stamped
                    // once and pushed together, so a four-note chord arrives as
                    // a chord. They used to be sent as on/off pairs per note,
                    // which put a far-future note-off second in the queue,
                    // where the drain parked it as "not due yet" and stopped —
                    // delivering the rest of the chord when the first note
                    // ended rather than when it started.
                    //
                    // Through `send_midi`, so an auditioned note is
                    // indistinguishable from a played one: same path, same
                    // instrument, and it lands in a take that is rolling, which
                    // is what somebody demonstrating a voicing on camera wants.
                    let sent: Option<(i64, Vec<[u8; 3]>)> =
                        self.recorder.engine.as_ref().map(|e| {
                            let at = e.timebase().now();
                            let status = if on { 0x90 } else { 0x80 };
                            let vel = if on {
                                tangent_ui::recorder::AUDITION_VELOCITY
                            } else {
                                64
                            };
                            let events: Vec<[u8; 3]> =
                                notes.iter().map(|n| [status, *n, vel]).collect();
                            for ev in &events {
                                e.send_midi(at, ev);
                            }
                            (at, events)
                        });
                    // **And into the take's `.mid`.** The audio always carried
                    // these; the file never did, so a take of somebody
                    // demonstrating a voicing had the sound and no notes.
                    // Captured with the stamp the engine was given, so the two
                    // cannot drift.
                    if let Some((at, events)) = sent {
                        for ev in events {
                            self.recorder.session.capture_app_midi(at, ev);
                            // **The arrangement's collection hears the app's
                            // own surfaces too** — the on-screen piano, the
                            // key rows, the chord lattice. Only hardware
                            // MIDI reached `note_event`, so a performer
                            // with no cable recorded silence on every
                            // platform without one.
                            if let Some(rec) = self.recorder.arr_rec.as_mut() {
                                rec.note_event(at, &ev);
                            }
                        }
                    }
                }
                R::PadHit { pad, on } => {
                    // The pads' own audition: one note on channel 10, the
                    // drum channel, which `collect_notes` keeps for the kit.
                    // Through `send_midi` like `Audition` above, and into
                    // the take's `.mid` on the same stamp — a pad tapped
                    // over a rolling take is a performance.
                    let note = tangent_ui::recorder::DRUM_PAD_BASE.saturating_add(pad);
                    let sent = self.recorder.engine.as_ref().map(|e| {
                        let at = e.timebase().now();
                        let status = if on { 0x99 } else { 0x89 };
                        let vel = if on {
                            tangent_ui::recorder::DRUM_PAD_VELOCITY
                        } else {
                            64
                        };
                        let ev = [status, note, vel];
                        e.send_midi(at, &ev);
                        (at, ev)
                    });
                    if let Some((at, ev)) = sent {
                        self.recorder.session.capture_app_midi(at, ev);
                        // **A pad tapped over a rolling ARRANGEMENT record
                        // is a performance too** — the take's .mid already
                        // knew; the arrangement's own collection now hears
                        // the app-injected strike as well, or pads played
                        // by hand never landed in the recorded clip.
                        if let Some(rec) = self.recorder.arr_rec.as_mut() {
                            rec.note_event(at, &ev);
                        }
                    }
                }
                R::PreviewFile(path) => {
                    // The media explorer's audition: decode at the engine's
                    // rate and hand it over whole. The pad cache already
                    // holds anything that has been dropped on a pad.
                    if let Some(e) = self.recorder.engine.as_mut() {
                        let rate = e.output().sample_rate.max(8_000);
                        // Reuse a pad's decode when one exists, but never
                        // CACHE an audition: browsing a folder of songs
                        // would otherwise hold every one of them in memory
                        // for the rest of the session.
                        let clip = match self.pad_cache.get(&path) {
                            Some(c) => Some(std::sync::Arc::clone(c)),
                            None => tangent_record::decode::decode(&path, rate)
                                .ok()
                                .map(std::sync::Arc::new),
                        };
                        if clip.is_some() {
                            e.set_preview(clip);
                        }
                    }
                }
                R::PreviewStop => {
                    if let Some(e) = self.recorder.engine.as_mut() {
                        e.set_preview(None);
                    }
                }
                R::EditPatch { slot } => {
                    // Whatever THAT strip is playing, which is what somebody
                    // means by "edit this patch". An edit in flight belongs
                    // to the strip it was opened on; opening the editor on a
                    // different strip starts from that strip's own sound.
                    if self.editing_strip != slot {
                        self.editing = None;
                    }
                    self.editing_strip = slot;
                    let v = match self.editing {
                        Some(v) => v,
                        None => self.voice_for_strip(slot),
                    };
                    self.editing = Some(v);
                    self.push_patch_edit(None);
                }
                R::SetPatchParam {
                    group,
                    index,
                    value,
                } => {
                    if let Some(v) = self.editing.as_mut() {
                        crate::dx7::edit::apply(v, group, index, value);
                        let v = *v;
                        let strip = self.editing_strip;
                        // **Heard as it is turned.** An editor that only
                        // applied on OK makes every change a guess.
                        if let Some(e) = self.recorder.engine.as_mut() {
                            e.set_builtin_voice(strip, v);
                        }
                        self.push_patch_edit(None);
                    }
                }
                R::SetPatchName(name) => {
                    if let Some(v) = self.editing.as_mut() {
                        v.set_name(&name);
                        self.push_patch_edit(None);
                    }
                }
                R::SavePatch => {
                    let said = self.save_patch();
                    self.push_patch_edit(Some(said));
                }
                R::ChoosePatch { slot, index } => {
                    // `usize::MAX` is the built-in row. Anything else indexes
                    // the STRIP's own cartridge, and a stale index from a
                    // settings file written against a cartridge that has
                    // since been replaced simply finds nothing and leaves the
                    // sound alone.
                    let voice = if index == usize::MAX {
                        Some(crate::dx7::Voice::default())
                    } else {
                        let path = self.app.dx7_choice_of(slot).0.to_owned();
                        self.cartridge_at(&path).voices.get(index).copied()
                    };
                    if let (Some(v), Some(e)) = (voice, self.recorder.engine.as_mut()) {
                        e.set_builtin_voice(slot, v);
                    }
                }
                R::LoadStripCartridge { strip } => {
                    // The picker just opened on this strip: the list must be
                    // ITS bank's thirty-two names. Nothing is pushed to the
                    // engine — names are the UI's business, and the sound
                    // changes when a name is clicked, not when a list fills.
                    let path = self.app.dx7_choice_of(strip).0.to_owned();
                    let is_factory = path.is_empty();
                    let cart = self.cartridge_at(&path).clone();
                    self.app
                        .set_cartridge(cartridge_info(&cart, "", is_factory));
                    self.cartridge = Some(cart);
                }
                // An insert's own window. Same one-row-two-names bargain as an
                // instrument's: press it again and the window that is up goes
                // away.
                R::OpenInsertEditor(strip, slot) => {
                    if let Some(e) = self.recorder.engine.as_mut() {
                        if e.insert_editor_open(strip, slot) {
                            e.close_insert_editor(strip, slot);
                        } else if let Err(err) = e.open_insert_editor(strip, slot) {
                            self.recorder.engine_error =
                                Some(format!("could not open the effect's window: {err}"));
                        }
                    }
                }
                // A built-in face's knob, on its way to the sound. The app
                // already mirrored the value into its own face; the engine
                // writes the durable ledger and rides the lock-free ring to
                // the audio thread. A refused value (unknown id, stale seat)
                // moves nothing anywhere, which is the contract — the next
                // face push snaps the optimistic mirror back.
                R::SetInsertParam { strip, slot, id, value } => {
                    if let Some(e) = self.recorder.engine.as_mut() {
                        e.set_insert_param(strip, slot, id, value);
                        // **Push the face back, NOW.** The readings are
                        // host-printed strings and the EQ's curve is
                        // host-computed; a push that only happened at load
                        // time froze every readout at its birth values and
                        // let a stale frame yank a dragged slider backward.
                        // One strip, one frame behind the finger - the lag a
                        // reading can wear, where "never" could not.
                        push_builtin_faces(&mut self.app, e, strip);
                    }
                }
                R::SetInsertTap { strip, slot, on } => {
                    // The face speaks DESK terms; `seat_bay` is the one
                    // translation into the rack's, exactly as its own note
                    // demands — a call site that recomputes the master's
                    // two-rack fold taps somebody else's plugin.
                    let at = tangent_ui::recorder::seat_bay(strip, slot);
                    self.insert_tap_at = on.then_some(at);
                    if let Some(e) = self.recorder.engine.as_mut() {
                        e.set_insert_tap(self.insert_tap_at);
                    }
                    // Arming is also "a face just opened here": its preset
                    // shelf rides along, so the panel's list is filled by
                    // the time the eye reaches it.
                    if on {
                        if let Some(unit) = self
                            .app
                            .insert(strip, slot)
                            .map(str::to_owned)
                        {
                            self.push_preset_rows(&unit);
                        }
                    }
                }
                // ── presets ─────────────────────────────────────────────
                // The preset road's disk half. Every arm ends by re-scanning
                // the unit's folder and re-pushing the rows, so what a panel
                // shows is what the disk holds — the rule the request enum
                // promises.
                R::SavePreset { unit, name, values } => {
                    self.save_unit_preset(&unit, &name, &values);
                    self.push_preset_rows(&unit);
                }
                R::LoadPreset { unit, path } => {
                    self.load_unit_preset(&unit, std::path::Path::new(&path));
                    self.push_preset_rows(&unit);
                }
                R::DeletePreset { path } => {
                    self.delete_unit_preset(std::path::Path::new(&path));
                }
                // ── the IR wizard ───────────────────────────────────────
                // The wizard's four asks. Requests because the capture
                // engine, the sweep math and `IRs/` all live on this side;
                // the answers ride back on the pushed `IrLive`.
                R::IrTone { out, inp, level_db, on } => {
                    if let Some(e) = self.recorder.engine.as_mut() {
                        if on {
                            e.start_ir_tone(out, inp, level_db);
                        } else {
                            e.stop_ir();
                        }
                    }
                }
                R::IrCapture { out, inp, sweep_s, tail_s, level_db, reference } => {
                    self.start_ir_capture(out, inp, sweep_s, tail_s, level_db, reference);
                }
                R::IrCancel => {
                    if let Some(e) = self.recorder.engine.as_mut() {
                        e.stop_ir();
                    }
                    // The pending pass is dropped HERE, so the buffer the
                    // engine hands back finds nobody waiting and dies
                    // quietly — and the live picture says the road is at
                    // rest, or the wizard would show a progress bar with
                    // nothing behind it forever.
                    self.ir.pending = None;
                    self.ir.live.capturing = false;
                    self.ir.live.progress = 0.0;
                    self.ir.live.phase = String::new();
                    self.app.set_ir_live(self.ir.live.clone());
                    // A cancel keeps the reference and the finished result:
                    // closing the wizard mid-thought must not cost the pass
                    // somebody already captured. This is also the wizard's
                    // Done, so the shelf is re-read on the way out — a save
                    // that happened this session is on it for certain.
                    self.push_ir_rows();
                }
                R::SetInsertIr { strip, slot, path } => {
                    // A swap IS a reload: the IR is bound before the
                    // instance crosses threads, so rewrite the sidecar
                    // with the new path and the bay's own current knobs,
                    // then mark the reload sentinel - the reconcile does
                    // the rest, state and all.
                    if let Some(e) = self.recorder.engine.as_ref() {
                        if let Some((spec, values)) = e.builtin_insert(strip, slot) {
                            if spec.path == crate::builtin_fx::IR_PATH {
                                let mut extra = e.builtin_extra(strip, slot).to_vec();
                                extra.retain(|(k, _)| k != "ir_path");
                                extra.push(("ir_path".to_owned(), path));
                                let blob = crate::builtin_fx::state_text(spec, values, &extra)
                                    .into_bytes();
                                write_insert_state(strip, slot, spec.path, &blob);
                                let bays = tangent_ui::recorder::INSERTS;
                                if let Some(cell) =
                                    self.recorder.inserts_loaded.get_mut(strip * bays + slot)
                                {
                                    *cell = Some("\u{1f}reload".to_owned());
                                }
                            }
                        }
                    }
                }
                R::IrSave { name } => {
                    self.save_ir(&name);
                }
            }
            ctx.request_repaint();
        }
    }

    // ── presets on disk ─────────────────────────────────────────────────────
    //
    // The host's half of the preset system: `tangent-ui` holds the format
    // (`preset.rs`, pure data) and asks through `RecorderRequest`; everything
    // that touches `Presets/` is below. One folder per unit under
    // `Settings::preset_root`, made lazily on the first save.

    /// Where one unit's presets live, under the preset root.
    fn unit_preset_dir(&self, unit: &str) -> std::path::PathBuf {
        self.app.preset_root().join(preset_subfolder(unit))
    }

    /// Scan a unit's folder and push what is there as plain rows — the
    /// cartridge-list pattern. Files that do not parse as presets are
    /// skipped without a word (a preset folder is somewhere people put
    /// junk, the user-themes rule), and so is a preset that does not name
    /// THIS unit: it cannot be loaded from this list, so a row for it would
    /// be a button that refuses — and requiring the match here is what lets
    /// a delete know, from the file alone, whose list to refresh.
    fn push_preset_rows(&mut self, unit: &str) {
        let dir = self.unit_preset_dir(unit);
        let mut rows: Vec<tangent_ui::ports::PresetRow> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut paths: Vec<std::path::PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        e.eq_ignore_ascii_case(tangent_ui::preset::PRESET_EXT)
                    })
                })
                .collect();
            paths.sort();
            for path in paths {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(p) = tangent_ui::preset::Preset::from_json(&text) else {
                    continue;
                };
                if p.unit != unit {
                    continue;
                }
                // The file's own name when it says, the stem when it does
                // not — the same fallback a renamed theme gets.
                let name = if p.name.trim().is_empty() {
                    path.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                } else {
                    p.name
                };
                rows.push(tangent_ui::ports::PresetRow {
                    name,
                    path: path.to_string_lossy().into_owned(),
                });
            }
        }
        self.app.set_preset_rows(unit.to_owned(), rows);
    }

    /// Write `values` as a named preset for `unit`, atomically — the
    /// `Settings::save_to` model: whole file beside, then rename, so a
    /// crash mid-write costs a `.tmp` and never a preset.
    ///
    /// The filename is the SANITIZED name; the typed name goes inside the
    /// file. Same name, same unit, same file: saving "Warm Bus" twice is an
    /// update, not a "Warm Bus (2)".
    fn save_unit_preset(&mut self, unit: &str, name: &str, values: &[(String, f64)]) {
        let dir = self.unit_preset_dir(unit);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let stem = tangent_ui::preset::safe_stem(name);
        let path = dir.join(format!("{stem}.{}", tangent_ui::preset::PRESET_EXT));
        let preset = tangent_ui::preset::Preset {
            version: tangent_ui::preset::PRESET_VERSION,
            unit: unit.to_owned(),
            name: name.trim().to_owned(),
            values: values.to_vec(),
            extra: Default::default(),
        };
        let tmp = path.with_extension("tangentpreset.tmp");
        if std::fs::write(&tmp, preset.to_json()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// Read a preset back and hand it to the app as plain data. The unit
    /// check is the owner rule `write_insert_state` keeps for plugin state:
    /// a file that names a different unit moves nothing, out loud.
    fn load_unit_preset(&mut self, unit: &str, path: &std::path::Path) {
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| tangent_ui::preset::Preset::from_json(&text));
        let p = match parsed {
            Ok(p) => p,
            Err(e) => {
                self.recorder.take_note = Some(format!("preset: {e}"));
                return;
            }
        };
        if !p.unit.is_empty() && p.unit != unit {
            self.recorder.take_note =
                Some(format!("preset: that file belongs to {}", p.unit));
            return;
        }
        let name = if p.name.is_empty() {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            p.name
        };
        self.app.set_preset_loaded(tangent_ui::ports::PresetLoaded {
            unit: unit.to_owned(),
            name,
            values: p.values,
        });
    }

    /// Delete one preset file, then re-push the list it came off.
    ///
    /// **The one request that must never follow user text anywhere it
    /// points.** The path is resolved and has to land inside the preset
    /// root wearing our extension, or nothing happens — a request built
    /// from a stale row, a moved root or a hostile string deletes nothing.
    fn delete_unit_preset(&mut self, path: &std::path::Path) {
        let Some(canon) = preset_delete_target(&self.app.preset_root(), path) else {
            return;
        };
        // Whose list to refresh, read from the file before it goes.
        let unit = std::fs::read_to_string(&canon)
            .ok()
            .and_then(|text| tangent_ui::preset::Preset::from_json(&text).ok())
            .map(|p| p.unit)
            .filter(|u| !u.is_empty());
        let _ = std::fs::remove_file(&canon);
        if let Some(unit) = unit {
            self.push_preset_rows(&unit);
        }
    }

    // ── IRs on disk, and the wizard's host half ─────────────────────────────
    //
    // The preset road's shape, one shelf instead of one per unit: the host
    // scans `IRs/` and pushes plain rows; the wizard asks through
    // `RecorderRequest` and reads the answers off the pushed `IrLive`. The
    // MEASUREMENT lives here too — the engine only plays and records; the
    // sweep synthesis, the deconvolution and the file all happen on this
    // side, with the heavy math on a worker thread so a fifty-second
    // capture's FFTs never freeze a frame.

    /// Scan `IRs/` and push the rows — see [`scan_ir_rows`], which is the
    /// whole of it; this method only knows where the folder is and who
    /// receives the answer.
    fn push_ir_rows(&mut self) {
        let rows = scan_ir_rows(&self.app.irs_root());
        self.app.set_ir_rows(rows);
    }

    /// Arm one sweep pass — the wizard's stage-3 button, and its optional
    /// loop-only reference pass. Everything the engine will touch is built
    /// here: the sweep from the verified math in `units::sweep`, the level
    /// from the stage the meter approved, a second of pre-roll so the noise
    /// floor is measured and the loop's latency never eats the sweep's head.
    fn start_ir_capture(
        &mut self,
        out: (usize, usize),
        inp: (usize, usize),
        sweep_s: f64,
        tail_s: f64,
        level_db: f32,
        reference: bool,
    ) {
        let Some(e) = self.recorder.engine.as_mut() else {
            self.ir.live.phase = "no audio engine is running".to_owned();
            self.app.set_ir_live(self.ir.live.clone());
            return;
        };
        let rate = f64::from(e.output().sample_rate.max(1));
        // The recipe's band: from 10 Hz — "well below the lowest frequency
        // of interest" (Muller) — up to 22 kHz or 95% of Nyquist, whichever
        // the rate can carry.
        let f1 = 10.0;
        let f2 = 22_000.0f64.min(0.475 * rate);
        let sweep_s = if sweep_s.is_finite() { sweep_s.clamp(1.0, 120.0) } else { 15.0 };
        let tail_s = if tail_s.is_finite() { tail_s.clamp(1.0, 60.0) } else { 10.0 };
        let sweep = crate::units::sweep::ess(f1, f2, sweep_s, rate);
        if sweep.is_empty() {
            self.ir.live.phase = "the sweep could not be generated at this rate".to_owned();
            self.app.set_ir_live(self.ir.live.clone());
            return;
        }
        let pre = rate.round() as usize;
        let tail = (tail_s * rate).round() as usize;
        let level = 10.0f32.powf(level_db.clamp(-60.0, 0.0) / 20.0);
        e.start_ir_capture(crate::instrument::IrCaptureSpec {
            out_pair: out,
            in_pair: inp,
            sweep: sweep.clone(),
            pre_silence_frames: pre,
            post_silence_frames: tail,
            level,
        });
        let total = pre + sweep.len() + tail;
        if reference {
            // A fresh reference supersedes whatever was held.
            self.ir.reference = None;
            self.ir.live.has_reference = false;
        } else if let Some(r) = &self.ir.reference {
            // A held reference only divides a pass with ITS sweep: same
            // length, same window — the IRU lock semantics. A mismatched
            // one is quietly retired rather than silently misapplied.
            if r.sweep_len != sweep.len() || r.total != total {
                self.ir.reference = None;
                self.ir.live.has_reference = false;
            }
        }
        self.ir.pending = Some(IrPending {
            reference,
            sweep,
            pre,
            rate,
            f1,
            f2,
            duration: sweep_s,
            total,
        });
        self.ir.live.result = None;
        self.ir.live.capturing = true;
        self.ir.live.progress = 0.0;
        self.ir.live.phase = if reference {
            "capturing the loop-only reference".to_owned()
        } else {
            "capturing".to_owned()
        };
        self.app.set_ir_live(self.ir.live.clone());
    }

    /// The IR road's once-per-frame poll: return levels while the tone or a
    /// capture runs, a finished capture handed off to the worker thread, the
    /// worker's answer landed, and the whole picture pushed to the app.
    fn ir_tick(&mut self) {
        let mut busy = self.ir.pending.is_some() || self.ir.worker.is_some();
        if let Some(e) = self.recorder.engine.as_mut() {
            let (rms_db, peak_db) = e.ir_return_db();
            let (done_f, total_f) = e.ir_progress();
            if rms_db > -200.0 || total_f > 0 {
                busy = true;
            }
            self.ir.live.rms_db = rms_db;
            self.ir.live.peak_db = peak_db;
            self.ir.live.progress = if total_f > 0 {
                (done_f as f32 / total_f as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if let Some((capture, complete)) = e.take_captured() {
                busy = true;
                if let Some(p) = self.ir.pending.take() {
                    if !complete {
                        self.ir.live.capturing = false;
                        self.ir.live.phase = "cancelled".to_owned();
                    } else if p.reference {
                        self.ir.reference = Some(IrReference {
                            capture,
                            sweep_len: p.sweep.len(),
                            total: p.total,
                        });
                        self.ir.live.capturing = false;
                        self.ir.live.has_reference = true;
                        self.ir.live.phase =
                            "reference captured - now capture through the unit".to_owned();
                    } else {
                        // The heavy half, off this thread: a fifty-second
                        // capture deconvolves through multi-million-point
                        // FFTs, and a frozen frame would read as a hang.
                        let reference =
                            self.ir.reference.as_ref().map(|r| r.capture.clone());
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(process_ir_capture(
                                &capture, &p.sweep, reference.as_deref(), p.pre, p.rate,
                                p.f1, p.f2, p.duration,
                            ));
                        });
                        self.ir.worker = Some(rx);
                        self.ir.live.capturing = false;
                        self.ir.live.phase = "deconvolving".to_owned();
                    }
                }
                // With no pending pass this was a stopped tone (or a stale
                // job): the buffers just died on this thread, as designed.
            }
        }
        if let Some(rx) = self.ir.worker.take() {
            match rx.try_recv() {
                Ok((left, right, outcome)) => {
                    self.ir.finished = Some(IrFinished {
                        irs: [left, right],
                        rate: self.recorder.engine.as_ref().map_or(48_000, |e| e.output().sample_rate),
                    });
                    self.ir.live.result = Some(outcome);
                    self.ir.live.phase = String::new();
                    busy = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.ir.worker = Some(rx);
                    busy = true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.ir.live.phase = "deconvolution failed".to_owned();
                }
            }
        }
        // Pushed while anything moves, and once more as the road settles, so
        // the wizard's last frame shows the road at rest rather than stale.
        if busy || self.ir.was_busy {
            self.app.set_ir_live(self.ir.live.clone());
        }
        self.ir.was_busy = busy;
    }

    /// Write the processed pair as `IRs/<name>.wav` — 32-bit float stereo at
    /// the capture's rate, the format every convolver reads — and re-push
    /// the shelf.
    fn save_ir(&mut self, name: &str) {
        let Some(f) = self.ir.finished.as_ref() else {
            self.ir.live.phase = "nothing captured to save yet".to_owned();
            self.app.set_ir_live(self.ir.live.clone());
            return;
        };
        let dir = self.app.irs_root();
        if std::fs::create_dir_all(&dir).is_err() {
            self.ir.live.phase = "the IRs folder could not be created".to_owned();
            self.app.set_ir_live(self.ir.live.clone());
            return;
        }
        let stem = sanitize_file_stem(name);
        let path = dir.join(format!("{stem}.wav"));
        let spec = tangent_record::wav::WavSpec {
            sample_rate: f.rate,
            channels: 2,
            format: tangent_record::wav::SampleFormat::Float32,
        };
        let bext =
            tangent_record::wav::Bext::new(tangent_record::wav::Wallclock::now_utc(), spec);
        let frames = f.irs[0].len().max(f.irs[1].len());
        let mut interleaved = vec![0.0f32; frames * 2];
        for (i, fr) in interleaved.chunks_exact_mut(2).enumerate() {
            fr[0] = f.irs[0].get(i).copied().unwrap_or(0.0);
            fr[1] = f.irs[1].get(i).copied().unwrap_or(0.0);
        }
        let written = tangent_record::wav::WavWriter::create(&path, spec, &bext)
            .and_then(|mut w| {
                w.write_interleaved(&interleaved)?;
                w.finish()
            });
        self.ir.live.phase = match written {
            Ok(()) => format!("saved {stem}.wav"),
            Err(e) => format!("save failed: {e}"),
        };
        self.app.set_ir_live(self.ir.live.clone());
        self.push_ir_rows();
    }

    /// The patch the settings point at, if the cartridge has one there.
    ///
    /// The cartridge at `path`, loaded at most once and kept. `""`, or a
    /// file that has since gone missing, is the bank that ships — the same
    /// reading launch has always made, so a strip whose cartridge walked off
    /// a disk still makes a sound.
    fn cartridge_at(&mut self, path: &str) -> &crate::dx7::Cartridge {
        self.cartridges.entry(path.to_owned()).or_insert_with(|| {
            if path.is_empty() {
                crate::dx7::factory()
            } else {
                crate::dx7::Cartridge::load(std::path::Path::new(path))
                    .unwrap_or_else(|_| crate::dx7::factory())
            }
        })
    }

    /// The patch strip `strip`'s DX7 should play, out of its own cartridge.
    /// A stale index — a settings file written against a bank that got
    /// shorter — takes the bank's first patch rather than silence.
    fn voice_for_strip(&mut self, strip: usize) -> crate::dx7::Voice {
        let (path, patch) = {
            let (p, i) = self.app.dx7_choice_of(strip);
            (p.to_owned(), i)
        };
        let c = self.cartridge_at(&path);
        c.voices
            .get(patch)
            .or_else(|| c.voices.first())
            .copied()
            .unwrap_or_default()
    }

    /// Where a patch made here is written.
    ///
    /// **Beside the settings, not in them.** It is a SysEx bank: an ordinary
    /// one, which Dexed and a real DX7 will both open. A patch editor whose
    /// work could only be read back by this app would be a worse deal than the
    /// hardware offered in 1983.
    fn user_bank_path() -> std::path::PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        tangent_core::config::file("my-patches.syx")
    }

    /// Hand the editor what it is editing.
    fn push_patch_edit(&mut self, note: Option<String>) {
        let Some(v) = self.editing else { return };
        let path = Self::user_bank_path();
        let edit = crate::dx7::edit::to_edit(&v, &path.to_string_lossy());
        self.app.set_patch_edit(edit, note);
    }

    /// Write the patch being edited into the user's own bank.
    ///
    /// **Appended, never overwritten.** A save that replaced the last one
    /// would lose the version somebody was happy with an hour ago, and a bank
    /// holds thirty-two: the oldest is dropped only once it is truly full, and
    /// the message says so.
    fn save_patch(&mut self) -> String {
        let Some(v) = self.editing else {
            return String::new();
        };
        let path = Self::user_bank_path();
        let mut voices: Vec<crate::dx7::Voice> = crate::dx7::Cartridge::load(&path)
            .map(|c| c.voices)
            .unwrap_or_default();
        // Everything after the last patch anybody saved is the padding this
        // wrote last time. Trimming it is what makes "append" mean append.
        while voices.last() == Some(&crate::dx7::Voice::default()) {
            voices.pop();
        }
        let full = voices.len() >= 32;
        if full {
            voices.remove(0);
        }
        voices.push(v);
        let cart = crate::dx7::Cartridge::of("my-patches", voices);
        match cart.save(&path) {
            Ok(()) => {
                let where_to = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                if full {
                    format!("Saved to {where_to}. The bank was full, so the oldest went.")
                } else {
                    format!("Saved \"{}\" to {where_to}.", v.display_name())
                }
            }
            Err(e) => format!("Could not save: {e}"),
        }
    }

    /// Read the cartridge named in the settings, if it is still there.
    ///
    /// Failure is not reported: see the call site. The built-in patch always
    /// sounds, so the worst case is the sound somebody would have had on a
    /// fresh install.
    fn load_cartridge_at_launch(&mut self) {
        let path = self.app.dx7_choice().0.to_owned();
        // **No cartridge chosen means the one that ships**, not silence and not
        // a single patch. Sixteen electric pianos and sixteen jazz guitars are
        // in the binary; a fresh install has them all without opening a file
        // dialog, and "Load .syx..." is for somebody who wants somebody else's.
        let mut is_factory = path.is_empty();
        let cart = if path.is_empty() {
            crate::dx7::factory()
        } else {
            // A path that has moved falls back to the shipped bank rather than
            // to nothing: cartridges live in sample folders that get
            // reorganised, and an app that goes silent because a file moved is
            // an app that looks broken.
            crate::dx7::Cartridge::load(std::path::Path::new(&path)).unwrap_or_else(|_| {
                // The remembered file has gone. What is playing IS the shipped
                // bank now, and the picker must say so rather than offering to
                // go back to the bank it is already on.
                is_factory = true;
                crate::dx7::factory()
            })
        };
        self.app
            .set_cartridge(cartridge_info(&cart, "", is_factory));
        // Into the cache under the path the strips will ask for it by. No
        // voice is pushed here any more: at launch there is no engine to
        // hear it, and the per-frame lane sync ships every strip's own
        // voice the moment its instance lands.
        self.cartridges
            .insert(if is_factory { String::new() } else { path }, cart.clone());
        self.cartridge = Some(cart);
    }

    /// Raise the file panel for a cartridge and load what comes back.
    ///
    /// Out here with the folder picker and for the same reason: `rfd` runs a
    /// nested run loop, and raising one inside an egui frame re-enters the
    /// frame already on the stack.
    fn take_cartridge_request(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        // The same one-frame settle the save panel keeps — see there.
        if let Some(request) = self.app.take_file_request() {
            self.deferred_file = Some(request);
            ctx.request_repaint();
            return;
        }
        let Some(request) = self.deferred_file.take() else {
            self.restore_fullscreen(ctx);
            return;
        };
        // Out of fullscreen first, and open on the frame after: see
        // `picker_needs_windowed`. This is the case that froze the app.
        if Self::picker_needs_windowed(ctx) {
            self.deferred_file = Some(request);
            self.refullscreen = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            return;
        }
        // **And one frame for the dialogs to stop floating.**
        //
        // A dialog is always-on-top so that a modal one cannot end up buried
        // behind the main window, which is an app that has silently frozen.
        // The OS's file panel is parented to the MAIN window, so it opens
        // UNDERNEATH any such dialog — which is what "load sysex opens behind
        // the instrument window" is. `native_panel_up` takes the dialogs down
        // to a normal level, but a window level is a property of the surface
        // and therefore changes on the NEXT frame; opening the panel in the
        // same frame that asked for it would race that change and lose.
        if self.arm_panel(ctx, &request) {
            return;
        }
        let purpose = request.purpose;
        // **No portal, no zenity: our own browser.** Otherwise this opens a
        // dialog that never appears and returns the same `None` a cancel does.
        if !Self::native_dialogs_work() {
            let at = request
                .start_at
                .filter(|p| p.is_dir())
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| std::path::PathBuf::from("/"));
            let (entries, _) = Self::list_dir(&at, &request.extensions);
            self.browse_extensions = request.extensions.clone();
            // Our own browser is a dialog like any other, and wants to float.
            self.app.native_panel_closed();
            self.app.open_browser(
                request.title.clone(),
                tangent_ui::dialogs::BrowseFor::File(request.purpose),
                at,
                entries,
            );
            return;
        }
        let mut dialog = rfd::FileDialog::new().set_title(&request.title);
        // **Parented to the window that asked for it.**
        //
        // Without this the panel is a top-level window of its own, and on
        // Windows it can open BEHIND the app — the button appears to do
        // nothing, which is exactly what a tester reports as "I cannot load
        // any file". macOS puts an unparented panel in front regardless, which
        // is why this was invisible here.
        dialog = dialog.set_parent(frame);
        if let Some(start) = request.start_at.filter(|p| p.exists()) {
            dialog = dialog.set_directory(start);
        }
        if !request.extensions.is_empty() {
            let exts: Vec<&str> = request.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&request.extension_label, &exts);
            // **And everything else, as a second filter.**
            //
            // A filter DIMS non-matching files on macOS and HIDES them on
            // Windows. Cartridges in the wild are named `.syx`, `.SYX`, `.dx7`,
            // `.bin` and very often nothing at all — a folder that looks empty
            // is a dialog somebody closes again, and they are right to.
            dialog = dialog.add_filter("All files", &["*"]);
        }
        // **One panel or many, decided by the request.** Insert Media is the
        // one gesture where picking four stems at once is the point; every
        // other picker wants exactly one answer.
        let files: Vec<std::path::PathBuf> = if request.multi {
            dialog.pick_files().unwrap_or_default()
        } else {
            dialog.pick_file().into_iter().collect()
        };
        // **Whether or not anything was chosen**, on both counts: back to
        // fullscreen, because a cancel must not leave the window in a shape
        // the user did not ask for, and dialogs may float again, because a
        // cancelled panel is a panel that has gone.
        self.app.native_panel_closed();
        self.restore_fullscreen(ctx);
        if files.is_empty() {
            return;
        }
        if let tangent_ui::ports::FilePurpose::InsertMedia(lane) = purpose {
            self.insert_media(lane, &files);
            return;
        }
        self.finish_file_choice(purpose, &files[0]);
    }

    /// Raise the SAVE panel the app asked for, and answer it.
    ///
    /// The same nested-run-loop rule as the pickers: never inside an egui
    /// frame. The fullscreen dance is the pickers' too — see
    /// `picker_needs_windowed`.
    #[cfg(feature = "recorder")]
    fn take_save_panel(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        // **One frame of settle, always.** The request arrives on the frame
        // a project-dialog row was pressed; its viewport is destroyed a
        // frame later, and a native panel opened before that lands BEHIND
        // the project window — the owner watched all three do it.
        if let Some(request) = self.app.take_save_request() {
            self.deferred_save = Some(request);
            ctx.request_repaint();
            return;
        }
        let Some(request) = self.deferred_save.take() else {
            return;
        };
        if Self::picker_needs_windowed(ctx) {
            self.deferred_save = Some(request);
            self.refullscreen = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            return;
        }
        // **One more frame so the project menu's window is actually gone.**
        //
        // The one settle above is for the MAIN window leaving fullscreen; it is
        // not enough for the menu. The row that asked for this closed the menu
        // (`dialog = None`) on the frame the request arrived, but a child
        // viewport is not destroyed until the frame AFTER it stops being drawn,
        // and this runs inside `after_frame` — before the menu-less frame has
        // even been presented. Open the blocking panel now and it lands behind
        // the menu window that is still on screen and still always-on-top. So
        // hold one more frame, exactly as `arm_panel` does for the pickers,
        // and let that window go first.
        if self.save_armed {
            self.save_armed = false;
        } else if self.app.native_panel_up() {
            self.save_armed = true;
            self.deferred_save = Some(request);
            ctx.request_repaint();
            return;
        }
        if !Self::native_dialogs_work() {
            // No portal, no zenity: the in-app browser only picks files that
            // EXIST, and a save panel that silently never appears is worse
            // than the row doing nothing. Duplicate-project on such a rig
            // waits for the browser to learn to save.
            self.app.native_panel_closed();
            return;
        }
        // **Start where the thing being saved LIVES**, and let the panel
        // wander anywhere from there: the Projects folder for the project
        // purposes — the owner's word — and the drum kits' own preset
        // drawer for a kit. The drawer is made here, on the way in, because
        // this is the first save's lazy moment and a default directory that
        // does not exist is a panel that opens somewhere arbitrary. The
        // answered path is still honoured whole either way.
        let start = match request.purpose {
            tangent_ui::ports::SavePurpose::KitPreset => {
                let dir = self.app.preset_root().join(DRUM_PRESET_FOLDER);
                let _ = std::fs::create_dir_all(&dir);
                dir
            }
            _ => self.app.project_root(),
        };
        let mut dialog = rfd::FileDialog::new()
            .set_title(&request.title)
            .set_file_name(&request.suggested)
            .set_directory(start);
        dialog = dialog.set_parent(frame);
        let file = dialog.save_file();
        self.app.native_panel_closed();
        self.restore_fullscreen(ctx);
        let Some(file) = file else { return };
        self.app.finish_save_choice(request.purpose, &file);
    }

    /// Reload last session's backing track, if the file is still there.
    ///
    /// The project system's road in: reopen the remembered project, or make
    /// the first one — which is also where a v12 backing file becomes the
    /// document's first clip. The app owns all of that; this call is here
    /// because a TEST constructing an app must not create directories in
    /// anybody's real music folder, so only the host bootstraps.
    #[cfg(feature = "recorder")]
    fn open_project_at_launch(&mut self) {
        let root = self.app.project_root();
        // **The template first.** It is what a launch opens now, so it has
        // to exist before anything asks for it — it used to be seeded after
        // the project was already chosen, which was fine while launch
        // reopened whatever was last on screen.
        let template = self.templates_dir().join("default");
        self.app.seed_template(&template);
        // **Every launch starts on the default template.**
        //
        // It reopened the remembered project, so the app came up on whatever
        // was last touched and a session leaked into the next one. The owner:
        // "On open, Tangent should ALWAYS start on the default template --
        // for now, no exceptions."
        //
        // Cloned into the working scratch rather than opened in place: the
        // template is a stencil, and opening it directly would mean every
        // edit — and every save — writing over the thing every New is made
        // from. That is the "silently overwriting it" the owner was afraid
        // of, and it is one line away at all times.
        //
        // **Reclaim-by-deletion is for verbatim clones only.** This path
        // ran `remove_dir_all` over whatever `fresh_project_dir` handed
        // back, and unlike New it never counted the takes first — a launch
        // deleted an unsaved session's recordings without a question ever
        // being asked. A built kit is already `fresh_project_dir`'s
        // business; takes and the document are decided here, because the
        // New flow wants take-holding folders back (it has a dialog to ask
        // about them) and launch has no mouth yet. A document that differs
        // from the template's is a session somebody edited — every undo
        // writes it — so both step past to the next free name and nothing
        // is deleted. Plugin state alone (a preset tweaked in a scratch
        // with no other change) does not protect a folder: comparing
        // `state/` churn would strand a stale Untitled per launch.
        // **Nothing is cloned and nothing is created** - the owner: "no
        // creating folders called untitled PERIOD." The template is
        // adopted IN MEMORY, aimed at a prospective folder that
        // materialises only at the first thing worth keeping (a take, a
        // kit edit, a Save). And the litter of past sessions goes: any
        // stray Untitled that provably holds no work is cleared here.
        Self::sweep_disposable_untitleds(&root, &template);
        let dir = tangent_ui::app::fresh_project_dir(&root);
        let dir = if dir.exists() && !launch_reclaimable(&dir, &template) {
            next_untitled(&root, &dir)
        } else {
            dir
        };
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        self.app.adopt_template(&template, &dir);
        // The template's own plugin states become the session's globals -
        // and a template with no shelf CLEARS them, which is what a fresh
        // session means.
        self.copy_states_from_shelf(&template.join("state"));
        self.force_insert_reload();
        let _ = self.app.take_desk_reload();
    }

    /// What to do with a chosen folder, whichever picker chose it.
    #[cfg(feature = "recorder")]
    fn finish_dir_choice(&mut self, purpose: tangent_ui::ports::DirPurpose, dir: std::path::PathBuf) {
        match purpose {
            tangent_ui::ports::DirPurpose::RecordRoot => self.app.set_record_dir(dir),
            tangent_ui::ports::DirPurpose::ExportDir => self.app.set_export_dir(dir),
            tangent_ui::ports::DirPurpose::PluginFolder => self.app.add_plugin_folder(dir),
            tangent_ui::ports::DirPurpose::ProjectRoot => self.app.set_project_dir(dir),
            tangent_ui::ports::DirPurpose::ExportRoot => self.app.set_export_root(dir),
            tangent_ui::ports::DirPurpose::PresetRoot => self.app.set_preset_dir(dir),
            tangent_ui::ports::DirPurpose::IrsRoot => {
                self.app.set_irs_dir(dir);
                // A moved shelf is a different shelf: rescan where the
                // setting now points, so the rows match the folder shown.
                self.push_ir_rows();
            }
        }
    }

    /// Stand the chosen media on the timeline, the first on `lane`.
    ///
    /// The app owns the fan-out and the refusals; this is here only because
    /// the reload afterwards is the host's — a clip the document gained has
    /// to be decoded and pushed at the engine before it can be heard.
    #[cfg(feature = "recorder")]
    fn insert_media(&mut self, lane: usize, files: &[std::path::PathBuf]) {
        let at = self.app.playhead_s();
        let errors = self.app.insert_media_files(lane, files, at);
        if let Some(first) = errors.first() {
            self.app.set_track_info(tangent_ui::ports::TrackInfo {
                error: first.clone(),
                ..Default::default()
            });
        }
    }

    /// What to do with a chosen file, whichever picker chose it.
    #[cfg(feature = "recorder")]
    fn finish_file_choice(
        &mut self,
        purpose: tangent_ui::ports::FilePurpose,
        file: &std::path::Path,
    ) {
        if purpose == tangent_ui::ports::FilePurpose::BackingTrack {
            // Into the PROJECT: copied under media/, stood on the timeline.
            // The document edit marks the clips dirty and the reload below
            // this frame decodes and pushes the schedule.
            if let Err(error) = self.app.import_audio_file(file) {
                self.app.set_track_info(tangent_ui::ports::TrackInfo {
                    error,
                    ..Default::default()
                });
            }
            return;
        }
        if purpose == tangent_ui::ports::FilePurpose::Project {
            self.switch_project(file);
            return;
        }
        if purpose == tangent_ui::ports::FilePurpose::KitPreset {
            // The app owns this whole move: it reads the preset, copies each
            // sample into the project through the same `drumpad_load` a drop
            // uses, and marks the kit dirty so the reload below this frame
            // decodes and pushes it.
            self.app.apply_kit_preset(file);
            return;
        }
        if purpose == tangent_ui::ports::FilePurpose::ClickSample {
            // Only the PATH is stored — the decode happens on the flag below
            // this frame, on the same road the bundled asset takes, so a file
            // that has since moved falls back to the built-in click rather
            // than to silence.
            self.app.set_click_sample(&file.to_string_lossy());
            return;
        }
        match crate::dx7::Cartridge::load(file) {
            Ok(cart) => {
                // For the strip whose picker raised the panel — the answer
                // arrives with a path and no memory of who asked, so the app
                // remembered. The bank goes into the cache under its path,
                // the strip's setting starts naming it, and the strip's own
                // DX7 plays its first patch now.
                let strip = self.app.cartridge_pick_strip();
                let path = file.to_string_lossy().into_owned();
                self.app.set_cartridge(cartridge_info(&cart, "", false));
                self.app.set_dx7_cartridge_of(strip, path.clone());
                if let Some(v) = cart.voices.first().copied() {
                    if let Some(e) = self.recorder.engine.as_mut() {
                        e.set_builtin_voice(strip, v);
                    }
                }
                self.cartridges.insert(path, cart.clone());
                self.cartridge = Some(cart);
            }
            // Reported INTO the dialog rather than as a message box, because
            // the dialog is still open and the next thing the user will do is
            // pick a different file from the same folder.
            Err(e) => self.app.set_cartridge(tangent_ui::ports::CartridgeInfo {
                error: e,
                ..Default::default()
            }),
        }
    }


    /// Whether a picker has to wait for the window to leave fullscreen first.
    ///
    /// **Linux only, and it is not a hang.** Under i3 — and under any X11 WM
    /// that honours `_NET_WM_STATE_FULLSCREEN` properly — a fullscreen window
    /// sits above everything, including a modal file panel. `rfd::pick_file`
    /// is a BLOCKING call: it stops the main thread until the panel returns.
    /// Put those together and choosing a backing track from fullscreen froze
    /// the whole app with nothing in the console, because the app was waiting
    /// on a dialog that had opened underneath the window and could not be
    /// seen, focused or dismissed.
    ///
    /// So the window comes out of fullscreen first, the picker opens on the
    /// frame after — the change needs a frame to reach the WM — and fullscreen
    /// goes back when the picker is done. macOS and Windows put a panel in
    /// front of a fullscreen window themselves and are left alone.
    /// FreeBSD is included because this is an X11 problem, not a Linux one:
    /// the same window managers run there, `_NET_WM_STATE_FULLSCREEN` means the
    /// same thing, and `rfd` blocks the main thread identically. Nothing about
    /// the diagnosis above is kernel-specific.
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn picker_needs_windowed(ctx: &egui::Context) -> bool {
        ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    fn picker_needs_windowed(_ctx: &egui::Context) -> bool {
        false
    }

    /// Put fullscreen back after a picker that had to leave it — **and not
    /// while one is still open.**
    ///
    /// This is what 4.18.0 got wrong, and it turned a bad bug into a worse
    /// one. On a box with no portal the picker is our own `Dialog::FileBrowser`,
    /// which is a CHILD VIEWPORT — a second window. The sequence was: leave
    /// fullscreen, open the browser next frame, and then, on the frame after
    /// that, find no pending request and put fullscreen straight back. Under
    /// i3 a fullscreen window sits above everything, so the browser was buried
    /// the instant it appeared — and `app.rs` ignores all main-window input
    /// while a dialog is open, so `Z` did nothing either. Not a hang: a modal
    /// nobody could see, with the keyboard locked out. Force quit was the only
    /// way back, which is exactly what was reported.
    /// Hold a file-panel request back for exactly one frame, once.
    ///
    /// Returns whether the caller should give up this frame and try again on
    /// the next. See the call site for why a frame has to pass at all.
    fn arm_panel(&mut self, ctx: &egui::Context, request: &tangent_ui::ports::FileRequest) -> bool {
        if self.panel_armed {
            self.panel_armed = false;
            return false;
        }
        if !self.app.native_panel_up() {
            return false;
        }
        self.panel_armed = true;
        self.deferred_file = Some(request.clone());
        ctx.request_repaint();
        true
    }

    fn restore_fullscreen(&mut self, ctx: &egui::Context) {
        if !self.refullscreen {
            return;
        }
        // Still something on screen, or still something waiting to be shown.
        // **The save panel counts too.** It arrived after the pickers, with
        // its own deferred slot, and was missed here: `take_save_panel` runs
        // AFTER `take_cartridge_request` in `after_frame`, so on the frame
        // after a fullscreen Save As deferred itself this put fullscreen
        // straight back while the save was still waiting — the blocking rfd
        // panel then opened under the fullscreen window, which is the buried
        // modal the 4.18.0 note above calls force-quit territory.
        if self.app.open_dialog().is_some()
            || self.deferred_file.is_some()
            || self.deferred_dir.is_some()
            || self.deferred_save.is_some()
        {
            return;
        }
        self.refullscreen = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }

    /// Whether a NATIVE file dialog will actually appear.
    ///
    /// **`rfd` cannot tell you this and its failure looks like a cancel.** On
    /// Linux it has two backends compiled in — xdg-desktop-portal, then a
    /// zenity subprocess — and on a box with neither, `pick_file` returns
    /// `None`, which is the same thing it returns when somebody presses
    /// Cancel. The app cannot distinguish them, so a missing portal is a
    /// button that silently does nothing. Diagnosed on Void + XFCE; see
    /// `docs/LINUX-4.16-FINDINGS.md`.
    ///
    /// So it is probed instead, from the two facts that decide it:
    ///
    /// * a portal implementation that provides `FileChooser` — every portal
    ///   backend registers itself in a `.portal` file listing the interfaces
    ///   it implements, and the box in the report had one for Secret and none
    ///   for files, which is why "a portal is installed" is not the question;
    /// * or `zenity` somewhere on `PATH`.
    ///
    /// No new dependency, and wrong only in the direction that costs nothing:
    /// if this says no and a native dialog would have worked, the in-app
    /// browser opens instead and still chooses the file.
    /// FreeBSD runs this probe too, and needs it more than Linux does.
    ///
    /// Nothing in the diagnosis is Linux-specific: `rfd`'s two non-macOS
    /// backends are the portal and zenity, and a FreeBSD desktop is if anything
    /// LESS likely to have a portal running than the Void box that produced the
    /// original report. Left in the else-arm, FreeBSD asserted native dialogs
    /// always work and got the silent-dead-button bug back verbatim.
    ///
    /// The probe itself was already portable — a session bus, `dbus-send` from
    /// the same dbus package ports installs, `zenity` on `PATH` — and the file
    /// heuristic below even reads `/usr/local/share/...`, which is the FreeBSD
    /// prefix and was dead code until this gate included it.
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn native_dialogs_work() -> bool {
        fn which_on_path(exe: &str) -> bool {
            std::env::var_os("PATH")
                .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(exe).is_file()))
        }

        // A session bus at all. Without one there is no portal by definition.
        let has_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some();
        // **The SERVICE, not the files.** This used to look for portal backend
        // definitions on disk, and a sway machine promptly disproved it: the
        // package's files were present, no portal service was running, rfd
        // asked the portal and got nothing, fell back to a zenity that was not
        // installed, and Save, Load and Export all silently did nothing. The
        // files say a portal COULD run; only the bus says one will answer.
        //
        // So ask the bus: does `org.freedesktop.portal.Desktop` have an owner,
        // or is it activatable (systemd desktops start it on first call)?
        // `dbus-send` ships with dbus itself, so wherever a session bus
        // exists the question can be asked. Where it somehow cannot, fall
        // back to the file heuristic — wrong only in the direction that costs
        // a native panel, never the picker.
        let ask_bus = |method: &str, arg: Option<&str>| -> Option<String> {
            let mut cmd = std::process::Command::new("dbus-send");
            cmd.args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.DBus",
                "/org/freedesktop/DBus",
            ]);
            cmd.arg(format!("org.freedesktop.DBus.{method}"));
            if let Some(a) = arg {
                cmd.arg(format!("string:{a}"));
            }
            let out = cmd.output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        };
        const PORTAL: &str = "org.freedesktop.portal.Desktop";
        let chooser = has_bus
            && match ask_bus("NameHasOwner", Some(PORTAL)) {
                Some(reply) if reply.contains("boolean true") => true,
                Some(_) => ask_bus("ListActivatableNames", None)
                    .is_some_and(|names| names.contains(PORTAL)),
                // dbus-send missing or the call failed: the old heuristic,
                // which at least distinguishes "no portal installed" from
                // "portal installed".
                None => {
                    let portal_dirs = [
                        std::path::PathBuf::from("/usr/share/xdg-desktop-portal/portals"),
                        std::path::PathBuf::from("/usr/local/share/xdg-desktop-portal/portals"),
                    ];
                    portal_dirs.iter().any(|dir| {
                        std::fs::read_dir(dir).is_ok_and(|rd| {
                            rd.flatten().any(|e| {
                                e.path().extension().is_some_and(|x| x == "portal")
                                    && std::fs::read_to_string(e.path())
                                        .is_ok_and(|t| t.contains("FileChooser"))
                            })
                        })
                    })
                }
            };
        chooser || which_on_path("zenity")
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    fn native_dialogs_work() -> bool {
        true
    }

    /// The directory the in-app browser should show, and its rows.
    ///
    /// Directories first and then files, each sorted case-insensitively,
    /// because that is the order every file manager uses and the one a hand
    /// reaching down a list expects.
    #[cfg(feature = "recorder")]
    fn list_dir(
        at: &std::path::Path,
        extensions: &[String],
    ) -> (Vec<tangent_ui::dialogs::FileEntry>, String) {
        use tangent_ui::dialogs::FileEntry;
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let read = match std::fs::read_dir(at) {
            Ok(r) => r,
            Err(e) => return (Vec::new(), format!("cannot open that folder ({e})")),
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Dotfiles stay hidden. Somebody who needs one can still reach it,
            // because the browser follows wherever it is pointed.
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            if is_dir {
                dirs.push(FileEntry { name, path, is_dir });
            } else if extensions.is_empty()
                || path.extension().is_some_and(|x| {
                    let x = x.to_string_lossy().to_lowercase();
                    extensions.iter().any(|e| e.eq_ignore_ascii_case(&x))
                })
            {
                files.push(FileEntry { name, path, is_dir });
            }
        }
        let key = |e: &FileEntry| e.name.to_lowercase();
        dirs.sort_by_key(key);
        files.sort_by_key(key);
        // The way up first, when there is one.
        let mut out = Vec::with_capacity(dirs.len() + files.len() + 1);
        if let Some(up) = at.parent() {
            out.push(FileEntry {
                name: "..".to_owned(),
                path: up.to_path_buf(),
                is_dir: true,
            });
        }
        out.extend(dirs);
        out.extend(files);
        (out, String::new())
    }

    /// The document's clips, decoded and handed to the engine.
    ///
    /// **Decoded once per file, here, and held as one `Arc` each.** The
    /// renderer must never allocate, and the decoded audio is the largest
    /// thing this app ever holds — so it is built on this thread, cached by
    /// path so an edit does not re-decode a hundred megabytes, and pushed
    /// behind pointers. The cache is cleared when the engine restarts: a new
    /// device may run at a new rate, and the cache is keyed to exactly one.
    ///
    /// The band's picture is synthesized here too: the first clip's wave and
    /// name, the SPAN as the length — with one clip, exactly the backing
    /// track's old face; the lanes come with the arrangement view's phase.
    #[cfg(feature = "recorder")]
    /// The red button going DOWN: snapshot what is armed, open the capture
    /// tee, roll the transport. Refusals go to the band's status line — a
    /// press that does nothing silently is a broken button.
    fn start_arr_record(&mut self) {
        if self.recorder.arr_rec.is_some() {
            return;
        }
        let Some(e) = self.recorder.engine.as_ref() else {
            self.recorder.take_note =
                Some("Open the band to record — the audio path is closed.".to_owned());
            return;
        };
        if self.app.project_doc().is_none() {
            self.recorder.take_note = Some("No project is open.".to_owned());
            return;
        }
        // The armed lanes, split by what they record: a MIDI lane takes the
        // note stream, an AUDIO lane its input pick. **The pick walk is
        // `push_monitor_settings`' walk, exactly.** The moment any channel
        // names its input, picks belong to the BOUND channels only — an
        // unbound audio channel monitors nothing and consumes no pick. This
        // walk used to stay ordinal, counting every audio channel bound or
        // not, so on a desk mixing bound and NO INPUT lanes (a state the
        // settings migration deliberately preserves) the red button asked
        // the engine to tee a ring it never opened — the take came back
        // empty and the performance was silently gone — or drained another
        // channel's microphone onto the wrong lane.
        let armed = self.app.armed_mask();
        let mut midi_lanes = Vec::new();
        let mut audio = Vec::new();
        let mut mask = 0u32;
        let mut pick = 0usize;
        // An armed audio lane the walk had to pass over: R is lit but there
        // is no input behind it, and a press that does nothing silently is a
        // broken button.
        let mut armed_unbound = false;
        let any_explicit = (0..tangent_ui::recorder::CHANNELS).any(|ch| {
            self.app.channel_kind(ch) == Some(tangent_ui::recorder::ChannelKind::Audio)
                && TangentApp::parse_input_spec(self.app.channel_input_spec(ch)).is_some()
        });
        for ch in 0..tangent_ui::recorder::CHANNELS {
            match self.app.channel_kind(ch) {
                Some(tangent_ui::recorder::ChannelKind::Midi) => {
                    if armed & (1 << ch) != 0 {
                        midi_lanes.push(ch);
                    }
                }
                // A CLIP lane — used, no live source — arms in the UI like
                // any other; its R lit and the record ignoring it was the
                // owner's "non functional". It takes the note stream.
                None if armed & (1 << ch) != 0 => {
                    midi_lanes.push(ch);
                }
                Some(tangent_ui::recorder::ChannelKind::Audio) => {
                    if any_explicit
                        && TangentApp::parse_input_spec(self.app.channel_input_spec(ch)).is_none()
                    {
                        // NO INPUT on an explicit desk: the engine opened no
                        // ring for this lane, so there is nothing to tee.
                        if armed & (1 << ch) != 0 {
                            armed_unbound = true;
                        }
                    } else if pick < tangent_ui::recorder::INPUTS {
                        if armed & (1 << ch) != 0 {
                            mask |= 1 << pick;
                            audio.push((pick, ch, Vec::new()));
                        }
                        pick += 1;
                    }
                }
                None => {}
            }
        }
        if midi_lanes.is_empty() && audio.is_empty() {
            self.recorder.take_note = Some(if armed_unbound {
                "The armed channel has NO INPUT — its strip's input menu names \
                 what it records."
                    .to_owned()
            } else {
                "Nothing is armed — R on a channel is what records it.".to_owned()
            });
            return;
        }
        let rate = f64::from(e.output().sample_rate).max(1.0);
        let start_s = self.recorder.transport.position_s(Some(e), rate);
        let start_ns = e.timebase().now();
        e.set_arr_capture(mask);
        if !self.recorder.transport.playing() {
            self.recorder.transport.toggle_play();
        }
        self.recorder.arr_rec = Some(ArrRec {
            start_s,
            start_ns,
            midi_lanes,
            audio,
            notes: Vec::new(),
            open: std::collections::HashMap::new(),
        });
    }

    /// The record ending, by whichever door: the red button again, Stop, or
    /// the band closing. Tee off, one last drain, files into the project's
    /// `media/`, and the clips into the document. Reads the playhead, so it
    /// must run BEFORE anything rewinds the transport.
    fn finish_arr_record(&mut self) {
        let Some(mut rec) = self.recorder.arr_rec.take() else {
            return;
        };
        let rate = if let Some(e) = self.recorder.engine.as_mut() {
            e.set_arr_capture(0);
            // What was in flight when the tee closed. The block the device
            // is filling right now is not here yet and is let go: a tail a
            // buffer short, not a file that arrives late.
            for (input, _lane, buf) in &mut rec.audio {
                e.drain_capture(*input, buf);
            }
            f64::from(e.output().sample_rate).max(1.0)
        } else {
            48_000.0
        };
        let end_s = self
            .recorder
            .transport
            .position_s(self.recorder.engine.as_ref(), rate);
        let len_s = (end_s - rec.start_s).max(0.0);
        // The keys still down end where the record does.
        for (key, (at, vel)) in rec.open.drain() {
            rec.notes.push(tangent_ui::arrangement::ArrNote {
                at_s: at,
                dur_s: (len_s - at).max(0.05),
                key,
                vel,
            });
        }
        rec.notes.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
        let mut out: Vec<tangent_ui::arrangement::RecordedClip> = Vec::new();
        // Delivered even with NO notes: the punch promised destruction,
        // and a take of silence over a clip is an erase — the app carves
        // and simply lands nothing.
        if len_s >= 0.05 {
            for lane in &rec.midi_lanes {
                out.push(tangent_ui::arrangement::RecordedClip {
                    lane: *lane,
                    start_s: rec.start_s,
                    len_s,
                    path: None,
                    notes: rec.notes.clone(),
                });
            }
        }
        // The WAVs, one per armed input, into the project's own media/.
        if let Some(media) = self
            .app
            .project_doc()
            .and_then(|d| d.parent().map(|p| p.join("media")))
        {
            let _ = std::fs::create_dir_all(&media);
            for (_input, lane, frames) in &rec.audio {
                let seconds = frames.len() as f64 / 2.0 / rate;
                if seconds < 0.05 {
                    // **A dead input is said out loud, never shrugged at.**
                    // The transport ran a real take but this armed input
                    // handed over nothing — a stalled capture stream (the
                    // suspension family), not a blink. Skipping silently
                    // here was the owner's "record over a clip, silence,
                    // and the old waveform pops back": no clip landed, no
                    // carve ran, and nothing said why.
                    if len_s >= 0.5 {
                        self.recorder.take_note = Some(format!(
                            "channel {}: the input delivered no audio for a {:.1}s take -                              nothing was kept and the old clips stand. See Audio Status.",
                            lane + 1,
                            len_s
                        ));
                    }
                    continue;
                }
                let mut n = 1usize;
                let name = loop {
                    let cand = format!("rec-{}-{n}.wav", lane + 1);
                    if !media.join(&cand).exists() {
                        break cand;
                    }
                    n += 1;
                };
                let spec = tangent_record::wav::WavSpec {
                    sample_rate: rate as u32,
                    channels: 2,
                    format: tangent_record::wav::SampleFormat::Float32,
                };
                let bext =
                    tangent_record::wav::Bext::new(tangent_record::wav::Wallclock::now_utc(), spec);
                let write = tangent_record::wav::WavWriter::create(&media.join(&name), spec, &bext)
                    .and_then(|mut w| {
                        w.write_interleaved(frames)?;
                        w.finish()
                    });
                match write {
                    Ok(()) => out.push(tangent_ui::arrangement::RecordedClip {
                        lane: *lane,
                        start_s: rec.start_s,
                        len_s: seconds,
                        path: Some(format!("media/{name}")),
                        notes: Vec::new(),
                    }),
                    Err(e) => {
                        self.recorder.take_note = Some(format!("could not write {name}: {e}"));
                    }
                }
            }
        }
        // The app adds the rows and marks the clips dirty — which is
        // what makes `reload_clips` push the new schedule and the new sound.
        self.app.add_recorded_clips(out);
    }

    /// Start an export: the whole arrangement, played from the top through
    /// the real engine — racks, limiter, master — into an audio-only take
    /// the finish copies into the project's `export/`.
    fn start_bounce(&mut self, stems: bool, span: Option<(f64, f64)>) {
        if self.recorder.bounce.is_some()
            || self.recorder.session.is_recording()
            || self.recorder.arr_rec.is_some()
        {
            return;
        }
        if self.recorder.engine.is_none() {
            self.recorder.take_note =
                Some("Open the band to export — the audio path is closed.".to_owned());
            return;
        }
        let (from_s, until_s) = match span {
            Some((a, b)) if b > a => (a.max(0.0), b + 2.0),
            _ => (0.0, self.app.arr_span_s() + 2.0),
        };
        if until_s <= from_s + 2.0 && self.app.arr_span_s() <= 0.0 {
            self.recorder.take_note = Some("Nothing to export — the timeline is empty.".to_owned());
            return;
        }
        let root = self.app.record_root();
        // From the span's head, rolling before the take opens — the same
        // press-order the transport itself keeps.
        self.recorder.transport.stop();
        self.recorder.transport.locate(from_s);
        self.recorder.transport.toggle_play();
        let spec = tangent_ui::recorder::ExportSpec {
            audio: true,
            midi: false,
            video: tangent_ui::recorder::VideoMode::None,
            ..self.app.export_spec()
        };
        self.recorder.take_note = Some("Exporting — a real pass from the top…".to_owned());
        self.recorder.session.toggle(&root, Some("bounce"), 0, spec);
        // The stems: one writer per USED lane — clips on it, or a kind —
        // fed live from the engine's tap so nothing is buffered whole.
        let mut stem_writers = Vec::new();
        if stems {
            if let (Some(e), Some(dir)) = (
                self.recorder.engine.as_ref(),
                self.app
                    .project_doc()
                    .and_then(|d| d.parent().map(|p| p.join("export").join("stems"))),
            ) {
                let _ = std::fs::create_dir_all(&dir);
                let rate = e.output().sample_rate;
                let spec = tangent_record::wav::WavSpec {
                    sample_rate: rate,
                    channels: 2,
                    format: tangent_record::wav::SampleFormat::Float32,
                };
                let doc = self.app.arrangement().clone();
                for lane in 0..tangent_ui::recorder::CHANNELS {
                    // Only lanes with something to say — see the offline
                    // twin below for the reasoning.
                    let sounds = doc.clips.iter().any(|c| c.lane == lane)
                        || (0..tangent_ui::recorder::INSERTS)
                            .any(|b| self.app.insert(lane, b).is_some());
                    if !sounds {
                        continue;
                    }
                    let name: String = self
                        .app
                        .channel_display_name(lane)
                        .chars()
                        .map(|c| if std::path::is_separator(c) { '-' } else { c })
                        .collect();
                    let path = dir.join(format!("{:02}-{}.wav", lane + 1, name.trim()));
                    let _ = std::fs::remove_file(&path);
                    let bext =
                        tangent_record::wav::Bext::new(tangent_record::wav::Wallclock::now_utc(), spec);
                    if let Ok(w) = tangent_record::wav::WavWriter::create(&path, spec, &bext) {
                        stem_writers.push((lane, w));
                    }
                }
                e.set_stems(true);
            }
        }
        self.recorder.bounce = Some(Bounce {
            // Two seconds of tail: the reverbs' ring-out is part of the
            // track.
            until_s,
            finishing: false,
            stems,
            stem_writers,
            stem_scratch: Vec::new(),
        });
    }

    /// Feed the stem writers whatever the tap has — every frame a bounce
    /// with stems runs, and once more at the finish.
    fn drain_stem_tap(&mut self) {
        let Some(b) = self.recorder.bounce.as_mut() else {
            return;
        };
        if b.stem_writers.is_empty() {
            return;
        }
        let Some(e) = self.recorder.engine.as_mut() else {
            return;
        };
        b.stem_scratch.clear();
        e.drain_stems(&mut b.stem_scratch);
        let wide = tangent_ui::recorder::CHANNELS * 2;
        let frames = b.stem_scratch.len() / wide;
        if frames == 0 {
            return;
        }
        let mut lane_buf = Vec::with_capacity(frames * 2);
        for (lane, w) in &mut b.stem_writers {
            lane_buf.clear();
            for f in 0..frames {
                lane_buf.push(b.stem_scratch[f * wide + *lane * 2]);
                lane_buf.push(b.stem_scratch[f * wide + *lane * 2 + 1]);
            }
            let _ = w.write_interleaved(&lane_buf);
        }
    }

    /// Drive a bounce: stop at the end, wait for the files, file the WAV.
    fn tick_bounce(&mut self) {
        self.drain_stem_tap();
        let Some(b) = self.recorder.bounce.as_mut() else {
            return;
        };
        if !b.finishing {
            let rate = self
                .recorder
                .engine
                .as_ref()
                .map_or(48_000.0, |e| f64::from(e.output().sample_rate).max(1.0));
            let pos = self
                .recorder
                .transport
                .position_s(self.recorder.engine.as_ref(), rate);
            // Past the end — or somebody pressed Stop, which ends the pass
            // early and exports what there is.
            if pos >= b.until_s || !self.recorder.session.is_recording() {
                self.recorder.session.stop();
                self.recorder.transport.stop();
                b.finishing = true;
            }
        } else if !self.recorder.session.is_recording() {
            if let Some(sum) = self.recorder.session.last_summary() {
                let folder = sum.folder.clone();
                let problem = sum.problem.clone();
                if let Some(e) = self.recorder.engine.as_ref() {
                    e.set_stems(false);
                }
                self.drain_stem_tap();
                let Some(b) = self.recorder.bounce.take() else {
                    return;
                };
                let stems = b.stems;
                let mut wrote = 0usize;
                for (_, w) in b.stem_writers {
                    if w.finish().is_ok() {
                        wrote += 1;
                    }
                }
                self.finish_bounce(&folder, problem, stems);
                if stems && wrote > 0 {
                    self.recorder.take_note = Some(format!(
                        "exported  export/track + {wrote} stems in export/stems/"
                    ));
                }
            }
        }
    }

    /// The pass is done: the take's WAV becomes `export/track.wav` (counted
    /// up past what exists), and the band's status line says where.
    fn finish_bounce(&mut self, folder: &str, problem: Option<String>, _stems: bool) {
        // The reveal-when-done watcher must not open a Finder window for a
        // bounce — the export note says where the real file went.
        self.recorder.seen_take = Some(folder.to_owned());
        if let Some(p) = problem {
            self.recorder.take_note = Some(format!("export failed: {p}"));
            return;
        }
        let take_dir = self.app.record_root().join(folder);
        let Some(export_dir) = self
            .app
            .project_doc()
            .and_then(|d| d.parent().map(|p| p.join("export")))
        else {
            return;
        };
        let _ = std::fs::create_dir_all(&export_dir);
        let wav = std::fs::read_dir(&take_dir).ok().and_then(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "wav"))
        });
        let Some(wav) = wav else {
            self.recorder.take_note = Some("export: the take produced no audio file".to_owned());
            return;
        };
        let mut n = 1usize;
        let name = loop {
            let cand = if n == 1 {
                "track.wav".to_owned()
            } else {
                format!("track {n}.wav")
            };
            if !export_dir.join(&cand).exists() {
                break cand;
            }
            n += 1;
        };
        match std::fs::copy(&wav, export_dir.join(&name)) {
            Ok(_) => {
                self.recorder.take_note = Some(format!("exported  export/{name}"));
            }
            Err(e) => {
                self.recorder.take_note = Some(format!("export failed: {e}"));
            }
        }
        // A realtime EXPORT-dialog pass: deliver into the chosen folder in
        // the chosen shapes. The engine's rate rules a realtime pass; the
        // spec's rate only steers the offline one.
        if let Some(mut spec) = self.recorder.pending_delivery.take() {
            spec.rate = self
                .recorder
                .engine
                .as_ref()
                .map_or(spec.rate, |e| e.output().sample_rate);
            let Some(out_dir) = self.export_out_dir(&spec) else {
                return;
            };
            let _ = std::fs::create_dir_all(&out_dir);
            let stem_name = sanitize_file_stem(&spec.name);
            let mut paths = Vec::new();
            if spec.master {
                let dst = out_dir.join(format!("{stem_name}.wav"));
                if std::fs::copy(export_dir.join(&name), &dst).is_ok() {
                    paths.push(dst);
                }
            }
            if spec.stems {
                let src = export_dir.join("stems");
                let dst_dir = out_dir.join("stems");
                let _ = std::fs::create_dir_all(&dst_dir);
                if let Ok(rd) = std::fs::read_dir(&src) {
                    for f in rd.flatten() {
                        let dst = dst_dir.join(f.file_name());
                        if std::fs::copy(f.path(), &dst).is_ok() {
                            paths.push(dst);
                        }
                    }
                }
            }
            self.app.set_export_progress(None);
            match deliver_formats_inner(&paths, &spec, true) {
                Ok(()) => {
                    self.recorder.take_note = Some(format!("Exported  {}", out_dir.display()));
                    // A long export is a thing people walk away from; the
                    // first thing they want on returning is the file.
                    if spec.open_when_done {
                        reveal(&out_dir);
                    }
                }
                Err(e) => {
                    self.recorder.take_note = Some(format!("export: {e}"));
                }
            }
        }
    }

    // ── the FULL-SPEED export ──────────────────────────────────────────────

    /// Where an export lands: the spec's directory, or the project's own
    /// `export/` folder.
    fn export_out_dir(
        &self,
        spec: &tangent_ui::ports::ProjectExportSpec,
    ) -> Option<std::path::PathBuf> {
        if !spec.dir.trim().is_empty() {
            return Some(std::path::PathBuf::from(spec.dir.trim()));
        }
        // **The Exports folder, in a room of the project's own.**
        //
        // Exports used to land in `export/` beside the project document,
        // which put finished work in among the session's scratch and made
        // "where are my exports" a per-project answer. They go under one
        // roof now — see `Settings::media_root` — with a subfolder per
        // project, because two projects both writing `master.wav` into one
        // folder is one of them overwriting the other.
        let name = self
            .app
            .project_doc()
            .and_then(|d| d.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .filter(|s| !s.is_empty());
        Some(match name {
            Some(n) => self.app.export_root().join(n),
            None => self.app.export_root(),
        })
    }

    /// Start a full-speed export: an OFFLINE engine, furnished exactly like
    /// the live one — desk, racks with their saved states, clips, pads —
    /// and pulled by hand a slice of a second per frame. The click stays
    /// out; nothing live reaches it.
    fn start_offline_bounce(&mut self, spec: tangent_ui::ports::ProjectExportSpec) {
        use tangent_ui::ports::ExportFormat;
        if self.recorder.offline_bounce.is_some()
            || self.recorder.bounce.is_some()
            || self.recorder.session.is_recording()
            || self.recorder.arr_rec.is_some()
        {
            return;
        }
        // The whole project, or the arrange view's TIME SELECTION.
        let (from_s, span) = match spec.span {
            Some((a, b)) if b > a => (a.max(0.0), b - a),
            _ => (0.0, self.app.arr_span_s()),
        };
        if span <= 0.0 {
            self.recorder.take_note = Some("Nothing to export — the timeline is empty.".to_owned());
            return;
        }
        let Some(out_dir) = self.export_out_dir(&spec) else {
            self.recorder.take_note = Some("export: no project open".to_owned());
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            self.recorder.take_note = Some(format!("export: {e}"));
            return;
        }
        let rate = spec.rate.clamp(8_000, 192_000);
        let mut e =
            match crate::instrument::Engine::offline(rate, tangent_record::audio::Timebase::new()) {
                Ok(e) => e,
                Err(err) => {
                    self.recorder.take_note = Some(format!("export: {err}"));
                    return;
                }
            };
        // ── the desk, exactly as the live walk pushes it ──
        e.set_desk(&self.app.desk());
        let gains = self.app.gains();
        for ch in 0..tangent_ui::recorder::CHANNELS {
            e.set_channel_gain(ch, gains.channels[ch]);
            e.set_channel_pan(ch, self.app.channel_pan(ch));
        }
        // No live input of any kind: every arm down, every port silent.
        e.set_midi_off(u64::MAX);
        e.set_armed(0);
        e.set_monitor_on(false);
        e.set_drums(self.app.drums_gain(), self.app.drums_pan());
        // **The routing table, every frame, like every other desk value.**
        // Pruned first against what the device actually has: an interface
        // swapped for a smaller one leaves routes pointing at jacks that are
        // gone, and a fader that moves and does nothing is worse than a
        // route that admitted it went away. See `Routes::prune`.
        //
        // **In an EXPORT this drops the hardware routes and keeps the
        // sends**, which is exactly right: a rendered file has no jacks, but
        // a channel sent through another channel's reverb is part of the
        // mix and has to be in the file.
        self.publish_routes(&e);
        e.set_master_gain(gains.master);
        e.set_fx_return(gains.fx_return);
        let [reverb, delay, chorus, phaser, hpf, lpf, gain, limiter] = self.app.effect_sends();
        e.set_effects(crate::effects::Sends {
            reverb,
            delay,
            chorus,
            phaser,
            hpf,
            lpf,
            gain,
            limiter,
        });
        e.set_effect_params(effect_params_from(self.app.effect_params()));
        // **And the varispeed, with the same knobs and for the same reason.**
        // In an EXPORT this reaches the offline renderer exactly as it
        // reaches the live one — the owner's words were that it "applies to
        // exports when engaged", and it does so by being one more thing the
        // desk is pushed rather than a special case anywhere.
        e.set_varispeed(self.app.varispeed_ratio());
        // **The click stays out of the file**, whatever the band says.
        e.set_metronome_enabled(false);
        e.set_tempo(self.app.tempo_bpm());
        {
            let sig = self.app.time_signature();
            e.set_meter(u32::from(sig.beats), u32::from(sig.unit));
            // Beside the meter, because it is read with it: how many clicks
            // a beat of that signature carries.
            e.set_metronome_div(self.app.metronome_div());
            // Polarity, one bit per strip — see `Shared::strip_phase`.
            e.set_strip_phase(self.app.strip_phase());
        }
        // **The maps, in the export mirror too.** `start_offline_bounce`
        // furnishes a second engine BY HAND and duplicates four things the
        // live path also does; this is the fifth, and the one whose absence
        // would be least visible — an export whose bar lines are somewhere
        // else than the ones you were listening to.
        e.set_maps(crate::instrument::Maps {
            tempo: self.app.live_tempo_map(),
            meter: self.app.live_meter_map(),
        });
        // **And the curves, in the export mirror too.** An export that
        // ignored the automation would be the same class of quiet wrongness
        // as one whose bar lines were somewhere else: it plays, it sounds
        // plausible, and it is not what you were listening to.
        e.set_automation(self.app.engine_automation());
        // The built-in: rendered by the engine itself when a channel's
        // first bay names it — the live walk's own rule.
        let builtin_lanes: Vec<usize> = (0..tangent_ui::recorder::CHANNELS)
            .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::BUILTIN_PATH))
            .collect();
        e.set_builtin_strips(builtin_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)));
        // **Every lane holding the drumpad.**
        //
        // `.find()` alone was the bug: a drum instrument loads a full
        // instance per channel everywhere else, so the second lane was
        // silent while the shared pads lit up for the first. The mask is the
        // answer; the single index remains for the parts of the engine that
        // have not learned about lanes yet, and both come from this one walk
        // so they cannot disagree about which lane is first.
        let drum_lanes: Vec<usize> = (0..tangent_ui::recorder::CHANNELS)
            .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            .collect();
        e.set_drum_strips(drum_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)));
        // **And the modelled keyboard, in the EXPORT walk too.** An instrument
        // wired into the live engine and forgotten here is the worst kind of
        // export bug: everything sounds right while it is being played and the
        // rendered file is missing a track, which nobody finds until the mix
        // is somewhere else.
        let keys_lanes: Vec<usize> = (0..tangent_ui::recorder::CHANNELS)
            .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::KEYS_PATH))
            .collect();
        e.set_keys_strips(keys_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)));
        for &ch in &keys_lanes {
            e.set_keys_patch(ch, keys_patch_of(self.app.keys_patch_of(ch)));
            e.set_keys_params(ch, &self.app.keys_params_of(ch));
        }
        // Groups are part of the mix, so the BOUNCE has them too — an export
        // that ignored the folder buses would render a different record from
        // the one being listened to, which is the worst kind of export bug.
        e.set_folders(self.app.folder_parents());
        // Instances for every instrument lane — a fresh offline engine has
        // none — then each DX7 lane's sound out of its strip's OWN
        // cartridge. The cache makes that a map lookup; an export whose
        // second DX7 track rendered with the first track's bank would be the
        // same class of quiet wrongness as the bar lines being elsewhere.
        let (made_dx7, _) = e.sync_instrument_lanes(
            builtin_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)),
            keys_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)),
        );
        for ch in 0..tangent_ui::recorder::CHANNELS.min(32) {
            if made_dx7 & (1 << ch) != 0 {
                let v = self.voice_for_strip(ch);
                e.set_builtin_voice(ch, v);
            }
        }
        // ── every configured rack seat, saved state included ──
        let inserts = tangent_ui::recorder::INSERTS;
        for i in 0..(crate::instrument::RACK_SEATS * inserts) {
            let seat = i / inserts;
            let bay = i % inserts;
            let want = self.app.insert(seat, bay).map(str::to_owned);
            // Instrument sentinels only — effect sentinels load into the
            // export engine's racks exactly as they do the live one's.
            let engine_want = want.as_deref().filter(|p| !is_instrument_sentinel(p));
            let Some(path) = engine_want else { continue };
            let state = saved_insert_state(seat, bay, path);
            if let Err(err) = e.load_insert(
                seat,
                bay,
                Some(std::path::Path::new(path)),
                state.as_deref(),
            ) {
                self.recorder.take_note = Some(format!("export: a rack refused to load: {err}"));
            }
        }
        // ── the clips, decoded at the EXPORT rate with a cache of their
        // own, so the live engine's cache never learns a wrong-rate clip ──
        if let Some(doc_path) = self.app.project_doc().map(std::path::Path::to_path_buf) {
            let doc = self.app.arrangement().clone();
            let mut placed = Vec::new();
            let mut midi_placed = Vec::new();
            let mut cache: std::collections::HashMap<
                std::path::PathBuf,
                Arc<tangent_record::decode::Clip>,
            > = std::collections::HashMap::new();
            // Stretched copies, per-export like the decode cache above and
            // for the same reason: everything in here is at the EXPORT
            // rate. See `stretched_clip_audio`.
            let mut stretch_cache: std::collections::HashMap<
                (std::path::PathBuf, i64, String),
                Arc<tangent_record::decode::Clip>,
            > = std::collections::HashMap::new();
            let frames = |s: f64| frames_at(f64::from(rate), s);
            for clip in &doc.clips {
                if clip.kind == tangent_ui::arrangement::ClipKind::Midi {
                    let notes = midi_clip_notes(clip, f64::from(rate));
                    if !notes.is_empty() {
                        midi_placed.push(crate::instrument::PlacedMidi {
                            strip: clip.lane.min(tangent_ui::recorder::CHANNELS - 1),
                            start: frames(clip.start_s),
                            len: frames(clip.len_s),
                            notes: Arc::new(notes),
                        });
                    }
                    continue;
                }
                let path = tangent_ui::arrangement::Arrangement::resolve(&doc_path, &clip.path);
                let decoded = match cache.get(&path) {
                    Some(c) => Arc::clone(c),
                    None => match tangent_record::decode::decode(&path, rate) {
                        Ok(c) => {
                            let c = Arc::new(c);
                            cache.insert(path.clone(), Arc::clone(&c));
                            c
                        }
                        Err(_) => continue,
                    },
                };
                // **The clip's stretch, which the export used to drop** —
                // the same divergence the gain comment below records: the
                // live path rendered the stretched copy, the bounce the
                // raw decode, so a stretched clip exported at the wrong
                // speed and an open-ended one at the wrong length. The
                // cache is per-export and at the EXPORT rate; the live
                // `self.stretch_cache` holds live-rate audio and must not
                // feed a file at another rate.
                let decoded = stretched_clip_audio(clip, decoded, &path, &mut stretch_cache);
                // **The clip's own gain, which the export used to drop.**
                // It read `1.0` here while playback read `clip.gain`, so a
                // clip pulled down on the timeline came back at full level
                // in the file — the export disagreeing with what was heard.
                placed.extend(audio_clip_passes(
                    clip,
                    &decoded,
                    f64::from(rate),
                    clip.gain as f32,
                ));
            }
            e.set_clips(placed);
            e.set_midi_clips(midi_placed);
        }
        // ── the pads, decoded at the export rate ──
        {
            // **Each drum lane's own kit**, decoded at the export rate. The
            // live path does the same; an export that gave every lane one
            // kit would render a song nobody was listening to.
            for lane in (0..tangent_ui::recorder::CHANNELS)
                .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            {
                let mut kit = crate::instrument::PadKit::default();
                for (i, path) in self.app.drumpad_paths_of(lane).iter().enumerate() {
                    let Some(path) = path else { continue };
                    if let (Ok(c), Some(slot)) = (
                        tangent_record::decode::decode(path, rate),
                        kit.pads.get_mut(i),
                    ) {
                        *slot = Some(Arc::new(c));
                    }
                }
                e.set_pad_kit(lane, kit);
            }
            for lane in (0..tangent_ui::recorder::CHANNELS)
                .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            {
                e.set_pad_params(lane, &self.app.drum_kit_params_of(lane));
            }
        }
        e.set_stems(spec.stems);
        e.set_transport(1, (from_s * f64::from(rate)).round() as u64);
        e.set_rolling(true);
        // ── the files ──
        let wav_spec = tangent_record::wav::WavSpec {
            sample_rate: rate,
            channels: if spec.mono { 1 } else { 2 },
            format: match spec.depth {
                tangent_ui::ports::ExportDepth::Pcm16 => tangent_record::wav::SampleFormat::Int16,
                tangent_ui::ports::ExportDepth::Pcm24 => tangent_record::wav::SampleFormat::Int24,
                tangent_ui::ports::ExportDepth::Float32 => tangent_record::wav::SampleFormat::Float32,
            },
        };
        let bext = tangent_record::wav::Bext::new(tangent_record::wav::Wallclock::now_utc(), wav_spec);
        let stem_name = sanitize_file_stem(&spec.name);
        let master = if spec.master {
            let path = out_dir.join(format!("{stem_name}.wav"));
            let _ = std::fs::remove_file(&path);
            match tangent_record::wav::WavWriter::create(&path, wav_spec, &bext) {
                Ok(w) => Some((path, w)),
                Err(err) => {
                    self.recorder.take_note = Some(format!("export: {err}"));
                    return;
                }
            }
        } else {
            None
        };
        let mut stems = Vec::new();
        if spec.stems {
            let dir = out_dir.join("stems");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                self.recorder.take_note = Some(format!("export: stems dir: {e}"));
            }
            let doc = self.app.arrangement().clone();
            for lane in 0..tangent_ui::recorder::CHANNELS {
                // **Only lanes with something to SAY get a stem** — clips,
                // or an instrument in a bay (the DX7, the drumpad, an SD3
                // following the transport). A kind with neither is an armed
                // input that renders silence offline, and a folder of
                // silent WAVs reads as a bug.
                let sounds = doc.clips.iter().any(|c| c.lane == lane)
                    || (0..tangent_ui::recorder::INSERTS).any(|b| self.app.insert(lane, b).is_some());
                if !sounds {
                    continue;
                }
                let name: String = self
                    .app
                    .channel_display_name(lane)
                    .chars()
                    .map(|c| if std::path::is_separator(c) { '-' } else { c })
                    .collect();
                let path = dir.join(format!("{:02}-{}.wav", lane + 1, name.trim()));
                let _ = std::fs::remove_file(&path);
                if let Ok(w) = tangent_record::wav::WavWriter::create(&path, wav_spec, &bext) {
                    stems.push((lane, path, w));
                }
            }
        }
        // **Divided by the varispeed ratio, because `span` is PROGRAM time
        // and this is a count of DEVICE frames.**
        //
        // At 115% the piece is over in 87% of its written length, and an
        // export that still ran for the full span would end in thirteen
        // percent silence. Slower is the worse half: at 85% the piece needs
        // eighteen percent longer than it is written, and an export bounded
        // by the written length would simply stop before the end of the
        // music. Both are silent failures in a file somebody sends
        // somewhere.
        let ratio = f64::from(self.app.varispeed_ratio()).clamp(0.5, 2.0);
        let until = (((span + 2.0) / ratio) * f64::from(rate)).ceil() as u64;
        self.recorder.take_note = Some(format!(
            "Rendering at full speed… ({} stem writers)",
            stems.len()
        ));
        self.recorder.offline_bounce = Some(OfflineBounce {
            e,
            spec,
            until,
            done: 0,
            block: vec![0.0; 1024 * 2],
            master,
            stems,
            stem_scratch: Vec::new(),
            out_dir,
            peaks: vec![(0.0, 0.0); tangent_ui::ports::EXPORT_PEAKS],
        });
    }

    /// Drive the full-speed export: as many blocks as ~80 ms allows, every
    /// UI frame, until the span (plus a two-second tail) has rendered.
    fn tick_offline_bounce(&mut self) {
        let Some(mut b) = self.recorder.offline_bounce.take() else {
            return;
        };
        let budget = std::time::Instant::now();
        let wide = tangent_ui::recorder::CHANNELS * 2;
        let mut fold = Vec::new();
        while b.done < b.until {
            let n = 1024usize.min((b.until - b.done) as usize);
            let out = &mut b.block[..n * 2];
            out.fill(0.0);
            b.e.render_offline(out);
            if let Some((_, w)) = b.master.as_mut() {
                if b.spec.mono {
                    fold.clear();
                    for f in 0..n {
                        fold.push((out[f * 2] + out[f * 2 + 1]) * 0.5);
                    }
                    let _ = w.write_interleaved(&fold);
                } else {
                    let _ = w.write_interleaved(out);
                }
            }
            if !b.stems.is_empty() {
                b.stem_scratch.clear();
                b.e.drain_stems(&mut b.stem_scratch);
                let frames = b.stem_scratch.len() / wide;
                for (lane, _, w) in &mut b.stems {
                    fold.clear();
                    for f in 0..frames {
                        let l = b.stem_scratch[f * wide + *lane * 2];
                        let r = b.stem_scratch[f * wide + *lane * 2 + 1];
                        if b.spec.mono {
                            fold.push((l + r) * 0.5);
                        } else {
                            fold.push(l);
                            fold.push(r);
                        }
                    }
                    let _ = w.write_interleaved(&fold);
                }
            }
            // The wave, bucketed by where this block sits in the whole
            // render. Min and max rather than a mean: a peak that only
            // exists for two samples is exactly what somebody watching an
            // export is looking for.
            if b.until > 0 {
                let cols = tangent_ui::ports::EXPORT_PEAKS as u64;
                for f in 0..n {
                    let at = ((b.done + f as u64) * cols / b.until) as usize;
                    let Some(cell) = b.peaks.get_mut(at.min(tangent_ui::ports::EXPORT_PEAKS - 1))
                    else {
                        break;
                    };
                    let l = out[f * 2];
                    let r = out[f * 2 + 1];
                    cell.0 = cell.0.min(l).min(r);
                    cell.1 = cell.1.max(l).max(r);
                }
            }
            b.done += n as u64;
            if budget.elapsed() > std::time::Duration::from_millis(80) {
                break;
            }
        }
        if b.done < b.until {
            let fraction = (b.done as f64 / b.until as f64) as f32;
            self.app
                .set_export_progress(Some(tangent_ui::ports::ExportProgress {
                    fraction,
                    label: "Rendering".to_owned(),
                    peaks: b.peaks.clone(),
                }));
            self.recorder.offline_bounce = Some(b);
            return;
        }
        // Rendering is done; the format pass still has to run, and on a long
        // mp3 that is the part that used to look like a hang. Say so, and let
        // the window paint this frame before it starts.
        self.app
            .set_export_progress(Some(tangent_ui::ports::ExportProgress {
                fraction: 1.0,
                label: "Writing files".to_owned(),
                peaks: b.peaks.clone(),
            }));
        // ── done: close the files, then the format pass ──
        let OfflineBounce {
            spec,
            master,
            stems,
            out_dir,
            ..
        } = b;
        let mut problem: Option<String> = None;
        let mut master_path = None;
        if let Some((path, w)) = master {
            if let Err(e) = w.finish() {
                problem = Some(e.to_string());
            }
            master_path = Some(path);
        }
        let mut stem_paths = Vec::new();
        for (_, path, w) in stems {
            if let Err(e) = w.finish() {
                problem = Some(e.to_string());
            }
            stem_paths.push(path);
        }
        if let Some(p) = problem {
            self.recorder.take_note = Some(format!("export failed: {p}"));
            self.app.set_export_progress(None);
            return;
        }
        let mut paths = stem_paths;
        if let Some(m) = master_path {
            paths.push(m);
        }
        if let Err(e) = deliver_formats(&paths, &spec) {
            self.recorder.take_note = Some(format!("export: {e}"));
            self.app.set_export_progress(None);
            return;
        }
        self.recorder.take_note = Some(format!("Exported  {}", out_dir.display()));
        self.app.set_export_progress(None);
        if spec.open_when_done {
            reveal(&out_dir);
        }
    }

    /// The template shelf: `<record_root>/Projects/Templates`.
    fn templates_dir(&self) -> std::path::PathBuf {
        self.app.project_root().join("Templates")
    }

    /// Run whatever project action the app asked for this frame. The host
    /// runs these whole because it owns the plugin states and the shelf.
    fn run_project_request(&mut self) {
        let Some(req) = self.app.take_project_request() else {
            return;
        };
        match req {
            tangent_ui::ports::ProjectRequest::Save => {
                self.save_plugin_states();
                self.copy_states_to_project();
                self.app.save_project_doc();
            }
            tangent_ui::ports::ProjectRequest::Template(name) => {
                // Everything written down first, so the copy IS the session.
                self.save_plugin_states();
                self.copy_states_to_project();
                self.app.save_project_doc();
                let Some(src) = self
                    .app
                    .project_doc()
                    .and_then(std::path::Path::parent)
                    .map(std::path::Path::to_path_buf)
                else {
                    return;
                };
                // The NAME only — a template has one home, and a name with a
                // separator in it is a path pretending to be one.
                let name: String = name
                    .chars()
                    .filter(|c| !std::path::is_separator(*c))
                    .collect();
                if name.is_empty() {
                    return;
                }
                let dst = self.templates_dir().join(&name);
                let _ = std::fs::remove_dir_all(&dst);
                let _ = copy_dir_all(&src, &dst);
                // **A template is a stencil, not a saved project.** The
                // copy carries the session's `named` flag, and a clone of
                // a named template is born "kept" — so launch could never
                // reclaim a scratch again and minted "Untitled 6", "7"…
                // one per launch, the exact pile the reclaim exists to
                // prevent. The flag comes off the template's own document,
                // once, here at its minting.
                let doc = tangent_ui::arrangement::doc_in(&dst);
                let mut arr = tangent_ui::arrangement::Arrangement::load_from(&doc);
                if arr.named {
                    arr.named = false;
                    arr.save_to(&doc);
                }
            }
            tangent_ui::ports::ProjectRequest::Export(spec) => {
                self.recorder.take_note = Some(format!(
                    "export: {} {} → {}…",
                    if spec.realtime {
                        "realtime"
                    } else {
                        "full-speed"
                    },
                    match (spec.master, spec.stems) {
                        (true, false) => "master",
                        (false, true) => "stems",
                        _ => "master+stems",
                    },
                    if spec.dir.trim().is_empty() {
                        "project/export"
                    } else {
                        spec.dir.trim()
                    },
                ));
                if spec.realtime {
                    // The realtime pass: the live engine, the live rate. The
                    // spec's shaping (name, dir, formats, mono) is applied
                    // when the pass lands — see `finish_bounce`.
                    self.recorder.pending_delivery = Some(spec.clone());
                    self.start_bounce(spec.stems, spec.span);
                } else {
                    self.start_offline_bounce(spec);
                }
            }
            tangent_ui::ports::ProjectRequest::ExportTrack => self.start_bounce(false, None),
            tangent_ui::ports::ProjectRequest::ExportStems => self.start_bounce(true, None),
            tangent_ui::ports::ProjectRequest::New
            | tangent_ui::ports::ProjectRequest::NewKeeping
            | tangent_ui::ports::ProjectRequest::NewDiscarding => {
                // **Which of the three this is.** `New` is the menu row and
                // is allowed to stop and ask; the other two are the answers
                // to that question and never ask again.
                let asked = !matches!(req, tangent_ui::ports::ProjectRequest::New);
                let keeping =
                    matches!(req, tangent_ui::ports::ProjectRequest::NewKeeping);
                // The default template exists from the start: seeded here if
                // nothing ever saved over it.
                let template = self.templates_dir().join("default");
                self.app.seed_template(&template);
                // **New on a pristine scratch is a NO-OP.** The session on
                // the desk already IS what New would hand back: unnamed,
                // untouched since it was opened, and byte-identical to the
                // template on disk (the same test launch uses, so an
                // unnamed-but-SAVED session with real work in it still gets
                // its fresh scratch). Rebuilding it churned the folder every
                // press, and before the save-back below was gated on dirt it
                // minted an abandoned "Untitled N" each time - the owner's
                // "'new' is making a new 'untitled' session every single
                // time". No changes: no new folder, and the name never
                // advances.
                if !asked && self.app.new_is_noop() {
                    let pristine = self
                        .app
                        .project_doc()
                        .and_then(std::path::Path::parent)
                        .map(|d| launch_reclaimable(d, &template))
                        .unwrap_or(false);
                    if pristine {
                        return;
                    }
                }
                let root = self.app.project_root();
                // The scratch rule: an Untitled nobody SAVED is reclaimed,
                // so New lands on "Untitled" instead of minting a pile.
                let dir = tangent_ui::app::fresh_project_dir(&root);
                // **What is in the folder the reclaim would take?**
                //
                // The reclaim rule is right — an Untitled nobody saved should
                // not mint a pile of them — but it ran `remove_dir_all` over
                // that folder with `media/` inside it and no warning. A take
                // somebody recorded and never named is still a take they
                // played, and it is the only thing in this app that cannot be
                // made a second time. Everything else in there (the document,
                // the drumpad, the plugin state) genuinely is scratch.
                // **The scratch warning is gone from the ordinary path.**
                //
                // It existed because New reclaimed the unsaved Untitled and
                // could take recordings with it, and nothing had asked. The
                // save prompt asks now — before any of this runs, for New,
                // Open and Quit alike — so a second question here would be
                // the same question twice, and answering "keep it" minted
                // `Untitled 2`, then `3`: the owner's "New should not
                // continuously form untitled 1 2 3 etc if pressed
                // consecutively."
                //
                // Kept for the one case the prompt cannot cover: a scratch
                // with takes in it that is NOT the session on screen, which
                // the prompt was never asked about and which nobody has
                // agreed to lose.
                let takes = count_media(&dir);
                let is_current = self
                    .app
                    .project_doc()
                    .and_then(std::path::Path::parent)
                    .map(|p| p == dir)
                    .unwrap_or(false);
                if takes > 0 && !asked && !is_current {
                    // Ask, and do nothing else this frame. The answer comes
                    // back as `NewKeeping` or `NewDiscarding`.
                    self.app.warn_about_scratch(tangent_ui::ports::ScratchWarning {
                        name: dir
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "Untitled".to_owned()),
                        takes,
                    });
                    return;
                }
                // Keeping it means not reclaiming it at all: mint the next
                // free name and leave the recordings where they are.
                let dir = if keeping && takes > 0 {
                    next_untitled(&root, &dir)
                } else {
                    dir
                };
                // **Are we throwing away the session that is open right now?**
                // `fresh_project_dir` reclaims the current unsaved Untitled,
                // so New usually lands on the very folder on the desk.
                let discarding_current = self
                    .app
                    .project_doc()
                    .and_then(std::path::Path::parent)
                    .map(|p| p == dir)
                    .unwrap_or(false);
                // **The outgoing session is stashed FIRST** - unless it is
                // the very folder being discarded, where a save-back would
                // rewrite what New just agreed to clear.
                if !discarding_current {
                    self.save_plugin_states();
                    self.copy_states_to_project();
                    if self.app.project_dirty() {
                        self.app.save_project_doc();
                    }
                }
                // **A clean slate, and NOTHING new on disk** - the owner's
                // "no creating folders called untitled PERIOD". A
                // materialised leftover at the prospective name is cleared
                // (its takes were counted and asked about above); then the
                // template is adopted in memory, aimed at the folder that
                // does not exist yet.
                if dir.exists() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                self.app.adopt_template(&template, &dir);
                self.copy_states_from_shelf(&template.join("state"));
                self.force_insert_reload();
                let _ = self.app.take_desk_reload();
                self.track_cache.clear();
                self.stretch_cache.clear();
                self.pad_cache.clear();
                self.reload_clips();
                self.reload_pads();
            }
        }
    }

    /// The project's own plugin-state shelf: `<project>/state/`.
    fn project_state_dir(&self) -> Option<std::path::PathBuf> {
        Some(self.app.project_doc()?.parent()?.join("state"))
    }

    /// Copy the live plugin-state sidecars INTO the project, so the folder
    /// carries its instruments' knobs as well as their names.
    fn copy_states_to_project(&self) {
        let Some(dir) = self.project_state_dir() else {
            return;
        };
        // An UNMATERIALISED scratch has no folder and must not grow one
        // for a state shelf alone - "no untitled folders PERIOD". The
        // states still live in the global sidecars; a session worth
        // keeping materialises through a real writer first.
        if !dir.parent().is_some_and(std::path::Path::is_dir) {
            return;
        }
        let _ = std::fs::create_dir_all(&dir);
        // EVERY seat - the master's second rack and the folder-extra seats
        // were both invisible to the old STRIPS+1+DRUM_PADS bound, so a
        // project carried neither. See the seat-space doc in recorder.rs.
        for strip in 0..tangent_ui::recorder::TOTAL_SEATS {
            for bay in 0..tangent_ui::recorder::INSERTS {
                let global = insert_state_path(strip, bay);
                let mine = dir.join(format!("insert-state-{strip}-{bay}.bin"));
                if global.is_file() {
                    let _ = std::fs::copy(&global, &mine);
                } else {
                    let _ = std::fs::remove_file(&mine);
                }
            }
        }
    }

    /// Bring a project's plugin states OUT to the live sidecars. A bay the
    /// project has no state for loses its global sidecar too — the last
    /// session's reverb tail must not leak into this one's fresh plugin.
    fn copy_states_from_project(&self) {
        let Some(dir) = self.project_state_dir() else {
            return;
        };
        self.copy_states_from_shelf(&dir);
    }

    /// The same copy, from ANY shelf - the template's, at launch and New,
    /// where the session has no folder yet and must not grow one.
    fn copy_states_from_shelf(&self, dir: &std::path::Path) {
        for strip in 0..tangent_ui::recorder::TOTAL_SEATS {
            for bay in 0..tangent_ui::recorder::INSERTS {
                let global = insert_state_path(strip, bay);
                let mine = dir.join(format!("insert-state-{strip}-{bay}.bin"));
                if mine.is_file() {
                    let _ = std::fs::copy(&mine, &global);
                } else {
                    let _ = std::fs::remove_file(&global);
                }
            }
        }
    }

    /// **Clear the Untitled litter** - every stray scratch that provably
    /// holds no work: no takes in media/, a stock kit, and a document that
    /// is unnamed and clipless (or missing). Anything with a doubt stays.
    fn sweep_disposable_untitleds(root: &std::path::Path, template: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for e in entries.flatten() {
            let dir = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if !dir.is_dir() || !(name == "Untitled" || name.starts_with("Untitled ")) {
                continue;
            }
            if count_media(&dir) == 0
                && tangent_ui::app::TangentApp::scratch_is_disposable(&dir, template)
            {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }

    /// Make the insert reconcile revisit EVERY bay — load, reload with new
    /// state, or unload, one per frame as it always works. The sentinel can
    /// never equal a real path or an empty bay, so every slot mismatches.
    /// The reload sentinel on the bays of the rows a permutation MOVED.
    ///
    /// `order[new] = old`, so a row with `order[n] == n` is a row whose
    /// plugin, state and bays are all exactly where they were.
    fn force_insert_reload_for(&mut self, order: &[usize]) {
        let marks = reload_slots_for_loaded(
            &tangent_ui::recorder::seat_order(order),
            tangent_ui::recorder::INSERTS,
            &self.recorder.inserts_loaded,
        );
        for i in marks {
            if let Some(slot) = self.recorder.inserts_loaded.get_mut(i) {
                *slot = Some("\u{1f}reload".to_owned());
            }
        }
    }

    fn force_insert_reload(&mut self) {
        for slot in &mut self.recorder.inserts_loaded {
            *slot = Some("\u{1f}reload".to_owned());
        }
    }

    /// Switch sessions WHOLE: the old one keeps its desk and its plugin
    /// states, the new one arrives with its own — instruments included.
    fn switch_project(&mut self, doc: &std::path::Path) {
        // The old session first, while it is still the one on the desk.
        //
        // **The plugin states go UNCONDITIONALLY** - a knob turned in a
        // plugin's own window never dirties the document (the snapshot
        // stores bundle names, not their insides), and quitting shelves
        // them regardless, so a switch that skipped them would lose what a
        // quit keeps. Writing `state/` cannot strand a scratch either: the
        // reclaim compares only `project.tangent` and the takes.
        //
        // **The DOCUMENT answers to dirt.** The unconditional save-back
        // re-serialized even a pristine clone with the live desk's
        // defaults, so its bytes stopped matching the template and the
        // reclaim stopped recognising its own scratch: one abandoned
        // "Untitled N" per New, forever.
        self.save_plugin_states();
        self.copy_states_to_project();
        if self.app.project_dirty() {
            self.app.save_project_doc();
        }
        self.app.open_project(doc);
        // `open_project` applied the document's desk when it carried one;
        // the flag says so, and the states follow the desk.
        if self.app.take_desk_reload() {
            self.copy_states_from_project();
            self.force_insert_reload();
        }
    }

    fn reload_clips(&mut self) {
        // **Not gated on the document being SAVED.**
        //
        // This used to return here when there was no file on disk yet, which
        // meant a project nobody had saved could not push anything to the
        // engine at all — draw a note, move the playhead back, hear silence.
        // The path is needed for exactly one thing, resolving an AUDIO clip's
        // relative filename, and the MIDI branch below `continue`s long before
        // reaching it. So a MIDI note was being held back by a requirement
        // that had nothing to do with it.
        //
        // An audio clip with no document to resolve against is skipped where
        // it is read instead, which is the honest place to give up on one.
        let doc_path = self.app.project_doc().map(std::path::Path::to_path_buf);
        let rate = self
            .recorder
            .engine
            .as_ref()
            .map_or(48_000, |e| e.output().sample_rate);
        let doc = self.app.arrangement().clone();
        let mut placed = Vec::new();
        let mut midi_placed = Vec::new();
        let mut info: Option<tangent_ui::ports::TrackInfo> = None;
        let mut span = 0.0f64;
        let mut error = String::new();
        let mut fill_len: Vec<(u64, f64)> = Vec::new();
        let mut clip_infos: Vec<tangent_ui::ports::ClipInfo> = Vec::new();
        // The window the pictures are being made FOR. See `wave_span`.
        let wave_view = self.app.arr_wave_view();
        for clip in &doc.clips {
            // A MIDI item is a schedule of its own: notes in frames from
            // the clip's drawn left edge — the trim (`offset_s`) already
            // paid, a note the trim cut the head off SKIPPED rather than
            // re-attacked, and the walk in `mix_channels` needs them sorted.
            if clip.kind == tangent_ui::arrangement::ClipKind::Midi {
                span = span.max(clip.start_s + clip.len_s);
                let frames = |s: f64| frames_at(f64::from(rate), s);
                // The clip's notes with its loop spelled out — one function,
                // shared with the offline bounce. See `midi_clip_notes`.
                let notes = midi_clip_notes(clip, f64::from(rate));
                if !notes.is_empty() {
                    midi_placed.push(crate::instrument::PlacedMidi {
                        strip: clip.lane.min(tangent_ui::recorder::CHANNELS - 1),
                        start: frames(clip.start_s),
                        len: frames(clip.len_s),
                        notes: Arc::new(notes),
                    });
                }
                continue;
            }
            // An AUDIO clip needs somewhere to resolve its relative path
            // from. With no saved document there is nowhere, so this one clip
            // is skipped — the MIDI branch above returned long before here.
            let Some(doc_path) = doc_path.as_ref() else {
                continue;
            };
            let path = tangent_ui::arrangement::Arrangement::resolve(doc_path, &clip.path);
            let decoded = match self.track_cache.get(&path) {
                Some(c) => Arc::clone(c),
                None => match tangent_record::decode::decode(&path, rate) {
                    Ok(c) => {
                        let c = Arc::new(c);
                        self.track_cache.insert(path.clone(), Arc::clone(&c));
                        c
                    }
                    Err(e) => {
                        // The name is kept out of the info on purpose: a row
                        // that says a file name next to an error reads as
                        // "loaded but broken", and nothing is loaded.
                        error = e;
                        continue;
                    }
                },
            };
            // **The stretched copy, if this clip asks for one.**
            //
            // Everything below — the trim, the fades, the envelope, the
            // frame counts — then works on the stretched audio without
            // knowing it, which is the point of doing it here: a clip
            // stretched to 1.5 is a clip whose file is half again as long,
            // and nothing downstream has to hold a second opinion about
            // where its seconds are. Shared with the offline bounce — see
            // `stretched_clip_audio`; the sweep below must keep matching
            // its key construction.
            let decoded = stretched_clip_audio(clip, decoded, &path, &mut self.stretch_cache);
            let file_s = decoded.seconds();
            if clip.len_s == 0.0 {
                fill_len.push((clip.id, (file_s - clip.offset_s).max(0.0)));
            }
            let len_s = clip.play_len_s(file_s);
            span = span.max(clip.start_s + len_s);
            placed.extend(audio_clip_passes(
                clip,
                &decoded,
                f64::from(rate),
                clip.gain as f32,
            ));
            if info.is_none() {
                info = Some(tangent_ui::ports::TrackInfo {
                    name: clip.name.clone(),
                    seconds: 0.0,
                    wave: decoded.envelope(TRACK_WAVE_BUCKETS),
                    error: String::new(),
                });
            }
            // **The clip's own picture, at the resolution it is being drawn
            // at.** Only the visible slice, and only when the arrangement is
            // showing — see `wave_span`, which says why a fixed budget
            // cannot be right at every zoom.
            let frames = |s: f64| frames_at(f64::from(rate), s);
            // **A looped clip is drawn as it PLAYS**, the same pass over and
            // over — see `looped_span_envelope`. Reading the file straight
            // through would have shown eight bars of audio under a clip that
            // repeats one.
            let looped = clip.loop_s > 1.0e-6;
            let (wave, wave_from_s, wave_span_s) = match wave_view.and_then(|(vs, vl, px)| {
                wave_span(clip.start_s, len_s, vs, vl, px)
            }) {
                Some((from, span, buckets)) => {
                    // In the clip's own file, which is where the samples
                    // are: the drawn second `from` is `from - start_s` into
                    // the played span, past the left trim.
                    let into = (from - clip.start_s).max(0.0);
                    let wave = if looped {
                        looped_span_envelope(
                            &decoded,
                            clip.offset_s,
                            clip.loop_s,
                            into,
                            span,
                            buckets,
                            f64::from(rate),
                        )
                    } else {
                        span_envelope(
                            &decoded,
                            frames(clip.offset_s + into) as usize,
                            frames(span) as usize,
                            buckets,
                        )
                    };
                    (wave, from, span)
                }
                // Nothing on screen, or no panel: a coarse whole-clip
                // picture, which is what the band's own scrubber uses and
                // what a clip needs before anybody has looked at it.
                None => {
                    let wave = if looped {
                        looped_span_envelope(
                            &decoded,
                            clip.offset_s,
                            clip.loop_s,
                            0.0,
                            len_s,
                            CLIP_WAVE_BUCKETS,
                            f64::from(rate),
                        )
                    } else {
                        span_envelope(
                            &decoded,
                            frames(clip.offset_s) as usize,
                            frames(len_s) as usize,
                            CLIP_WAVE_BUCKETS,
                        )
                    };
                    (wave, clip.start_s, len_s)
                }
            };
            clip_infos.push(tangent_ui::ports::ClipInfo {
                id: clip.id,
                wave,
                file_s,
                wave_from_s,
                wave_span_s,
            });
        }
        self.app.set_clip_infos(clip_infos);
        if let Some(at) = wave_view {
            self.app.note_waves_drawn(at);
        }
        // Lengths the decode just established, written down so the scrubber
        // can draw next launch without one. Never marks the clips dirty.
        for (id, len_s) in fill_len {
            self.app.note_clip_len(id, len_s);
        }
        // Files no clip references any more leave the cache with the edit.
        // **Only when there is a document to resolve against.** Without one
        // nothing can match, and sweeping on that basis would throw away every
        // decoded file the moment a project became unsaved.
        if let Some(doc_path) = doc_path.as_ref() {
            self.track_cache.retain(|path, _| {
                doc.clips
                    .iter()
                    .any(|c| tangent_ui::arrangement::Arrangement::resolve(doc_path, &c.path) == *path)
            });
        }
        // And the stretched renders with them — including every ratio a drag
        // passed through on its way to the one that stuck. A drag across a
        // clip's edge is a hundred frames and a hundred ratios; without
        // this, that is a hundred whole copies of the file left in memory.
        if let Some(doc_path) = doc_path.as_ref() {
            self.stretch_cache.retain(|(path, ratio, mode), _| {
                doc.clips.iter().any(|c| {
                    (c.stretch * 1.0e6).round() as i64 == *ratio
                        && crate::stretch::Mode::from_key(&c.stretch_mode)
                            .unwrap_or_default()
                            .key()
                            == mode
                        && tangent_ui::arrangement::Arrangement::resolve(doc_path, &c.path) == *path
                })
            });
        }
        if let Some(e) = self.recorder.engine.as_mut() {
            e.set_clips(placed);
            e.set_midi_clips(midi_placed);
        }
        let mut info = info.unwrap_or_default();
        info.seconds = span;
        info.error = error;
        self.app.set_track_info(info);
    }

    /// The drumpad's samples: decode every slot at the engine's rate, push
    /// the kit whole, and tell the app what each pad turned out to be.
    /// `reload_clips`'s little sibling, with its own cache — see `pad_cache`.
    /// **Decode the chosen click and hand it to the engine.**
    ///
    /// An empty setting, a file that has moved, a file the decoder cannot
    /// read: all three end at the bundled click rather than at silence, and
    /// the setting is LEFT alone in the failure cases — a sound file living
    /// on a drive that is not plugged in today should come back tomorrow, not
    /// be forgotten because it was missing once.
    #[cfg(feature = "recorder")]
    fn reload_click(&mut self) {
        let path = self.app.click_sample().to_owned();
        let Some(engine) = self.recorder.engine.as_mut() else {
            return;
        };
        if path.is_empty() {
            engine.set_click(None);
            return;
        }
        match tangent_record::decode::decode(std::path::Path::new(&path), engine.output().sample_rate) {
            Ok(clip) => engine.set_click(Some(&clip.samples)),
            Err(e) => {
                engine.set_click(None);
                self.app.set_track_info(tangent_ui::ports::TrackInfo {
                    error: format!("that click sound would not load: {e}"),
                    ..Default::default()
                });
            }
        }
    }

    fn reload_pads(&mut self) {
        /// Columns in a pad's waveform ghost. A pad is fifty-odd points
        /// wide at most; forty-eight columns is under a point each.
        // 240 buckets: the trimmer draws the sample's real shape now, and the
        // pad ghost decimates for free.
        const PAD_WAVE_BUCKETS: usize = 240;
        let rate = self
            .recorder
            .engine
            .as_ref()
            .map_or(48_000, |e| e.output().sample_rate);
        // **Each drum lane's OWN kit.** A drum instrument loads a full
        // instance per channel, so this decodes once per lane — the cache
        // below means two lanes naming the same file decode it once and
        // share the `Arc`.
        let lanes: Vec<usize> = (0..tangent_ui::recorder::CHANNELS)
            .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            .collect();
        // The window's own lane is the one whose load STATUS the pads show.
        let shown = self.app.drumpad_lane();
        let mut delivered_all = !lanes.is_empty();
        for lane in lanes.iter().copied() {
        let paths = self.app.drumpad_paths_of(lane);
        let mut kit = crate::instrument::PadKit::default();
        let mut status: [tangent_ui::ports::PadStatus; crate::instrument::PADS] =
            std::array::from_fn(|_| tangent_ui::ports::PadStatus::default());
        for (i, path) in paths.iter().enumerate() {
            let Some(path) = path else { continue };
            let decoded = match self.pad_cache.get(path) {
                Some(c) => Some(Arc::clone(c)),
                None => match tangent_record::decode::decode(path, rate) {
                    Ok(c) => {
                        let c = Arc::new(c);
                        self.pad_cache.insert(path.clone(), Arc::clone(&c));
                        Some(c)
                    }
                    // The pad wears its file's name in red; there is no row
                    // to carry the decoder's sentence, and a moved folder
                    // is what this almost always is.
                    Err(_) => None,
                },
            };
            match (decoded, kit.pads.get_mut(i), status.get_mut(i)) {
                (Some(c), Some(slot), Some(st)) => {
                    st.loaded = true;
                    st.wave = c.envelope(PAD_WAVE_BUCKETS);
                    *slot = Some(c);
                }
                (None, _, Some(st)) => st.missing = true,
                _ => {}
            }
        }
        // **The pads only LIGHT once the kit actually LANDED.** A full ring
        // under a stalled callback — or no engine at all — used to drop the
        // kit silently while the ghosts drew anyway, and the Windows
        // tester's pads struck the DX7. Undelivered = try again next frame,
        // until the audio thread really has it.
        let landed = self
            .recorder
            .engine
            .as_mut()
            .is_some_and(|e| e.set_pad_kit(lane, kit));
        delivered_all &= landed;
        // The status belongs to the kit the WINDOW is showing: it is what
        // draws those pads' names, waveforms and missing-file marks.
        if landed && lane == shown {
            self.app.set_drumpad_status(status);
        }
        }
        // Files no pad references any more leave the cache with the edit —
        // asked across EVERY lane, or loading a second kit would evict the
        // first one's samples.
        let live: Vec<std::path::PathBuf> = lanes
            .iter()
            .flat_map(|l| self.app.drumpad_paths_of(*l))
            .flatten()
            .collect();
        self.pad_cache.retain(|path, _| live.iter().any(|p| p == path));
        if !delivered_all {
            self.recorder.pads_retry = true;
        }
    }

    /// Write every loaded bay's plugin state beside the settings.
    ///
    /// Called on quit and whenever an editor closes — the second matters more
    /// than the first, because closing the editor is the moment right after
    /// somebody chose a preset, and it is also the moment they are most likely
    /// to then force-quit or unplug something.
    /// Bring each clip up so its loudest point sits just under full scale.
    ///
    /// **The PLAYED span, not the file.** A clip trimmed to its quiet half
    /// should come up by what that half needs; measuring the whole file
    /// would normalise to a peak the clip does not play, which is the
    /// difference between "this take is level with the others" and "nothing
    /// happened".
    ///
    /// **And nothing is written to disk.** The gain is a number on the clip:
    /// undoable, reversible, and the same file still under it.
    #[cfg(feature = "recorder")]
    fn normalize_clips(&mut self, ids: &[u64]) {
        let Some(doc_path) = self.app.project_doc().map(std::path::Path::to_path_buf) else {
            return;
        };
        let rate = self
            .recorder
            .engine
            .as_ref()
            .map_or(48_000, |e| e.output().sample_rate);
        let doc = self.app.arrangement().clone();
        let mut done = 0usize;
        let mut silent = 0usize;
        for clip in doc.clips.iter().filter(|c| ids.contains(&c.id)) {
            if clip.kind != tangent_ui::arrangement::ClipKind::Audio {
                continue;
            }
            let path = tangent_ui::arrangement::Arrangement::resolve(&doc_path, &clip.path);
            let Some(decoded) = self.track_cache.get(&path).map(Arc::clone) else {
                continue;
            };
            let frames = |s: f64| frames_at(f64::from(rate), s) as usize;
            let from = frames(clip.offset_s);
            let len = frames(clip.play_len_s(decoded.seconds()));
            let end = (from + len).min(decoded.frames());
            let mut peak = 0.0f32;
            for f in from.min(end)..end {
                for ch in 0..2 {
                    let v = decoded.samples.get(f * 2 + ch).copied().unwrap_or(0.0);
                    if v.is_finite() {
                        peak = peak.max(v.abs());
                    }
                }
            }
            // **Silence has no peak to normalise to.** Dividing by it is an
            // infinite gain on a clip that would still be silent, and the
            // honest answer is to say so rather than to multiply nothing by
            // a very large number.
            if peak <= 1.0e-5 {
                silent += 1;
                continue;
            }
            // A hair under full scale: normalising exactly TO it leaves a
            // clip that clips the moment anything is added to it, and the
            // master's own limiter is not what this should be leaning on.
            const CEILING: f32 = 0.891; // −1 dBFS
            self.app
                .set_clip_gain(clip.id, f64::from(CEILING / peak));
            done += 1;
        }
        self.recorder.take_note = Some(match (done, silent) {
            (0, 0) => "Nothing to normalise.".to_owned(),
            (0, n) => format!("{n} clip(s) are silent - nothing to bring up."),
            (d, 0) => format!("Normalised {d} clip(s) to -1 dBFS."),
            (d, n) => format!("Normalised {d} clip(s); {n} were silent."),
        });
    }

    /// Copy one bay's effect to another channel's next free bay, with the
    /// settings it is playing at right now.
    ///
    /// **The state is written BEFORE the settings**, and that order is the
    /// whole function: the reconcile that loads the target's plugin reads
    /// the sidecar as it constructs the instance, so a sidecar written
    /// afterwards is a sidecar the plugin has already come up without. The
    /// symptom would be a copy that works on the second reload and not on
    /// the first, which is the kind of thing that gets reported as "it
    /// sometimes forgets".
    #[cfg(feature = "recorder")]
    fn copy_insert(&mut self, from_strip: usize, from_bay: usize, to_strip: usize) {
        let Some(bundle) = self.app.insert(from_strip, from_bay).map(str::to_owned) else {
            return;
        };
        if from_strip == to_strip {
            return;
        }
        let bays = tangent_ui::recorder::INSERTS;
        let Some(to_bay) = (0..bays).find(|b| self.app.insert(to_strip, *b).is_none()) else {
            // **Every bay full is a refusal with a reason.** Silently
            // replacing one would be throwing away an effect somebody dialled
            // in, from a gesture that says "copy".
            self.recorder.take_note = Some(format!(
                "{} has no free insert bay",
                self.app.channel_display_name(to_strip)
            ));
            return;
        };
        // The state it is playing at, not the one on disk: a plugin somebody
        // has been turning knobs on has not written a sidecar since it
        // loaded.
        if let Some(state) = self
            .recorder
            .engine
            .as_ref()
            .and_then(|e| e.save_insert_state(from_strip, from_bay))
        {
            write_insert_state(to_strip, to_bay, &bundle, &state);
        }
        self.app.set_insert(to_strip, to_bay, Some(&bundle));
    }

    fn save_plugin_states(&mut self) {
        // EVERY seat. The old STRIPS+1+DRUM_PADS bound predates the
        // master's second rack and the folder-extra seats, so a plugin
        // dialed in on any of those never got a sidecar and came back at
        // its defaults - while the project-copy loops below had already
        // been taught the full space and were copying stale files.
        for strip in 0..tangent_ui::recorder::TOTAL_SEATS {
            for bay in 0..tangent_ui::recorder::INSERTS {
                let Some(bundle) = self.app.insert(strip, bay).map(str::to_owned) else {
                    continue;
                };
                if let Some(state) = self
                    .recorder
                    .engine
                    .as_ref()
                    .and_then(|e| e.save_insert_state(strip, bay))
                {
                    write_insert_state(strip, bay, &bundle, &state);
                }
            }
        }
    }

    /// As [`Self::save_plugin_states`], but each sidecar's OWNER is what is
    /// really LOADED in the engine's slot — `inserts_loaded`'s truth — not
    /// what the settings say should be there. The reorder path needs this:
    /// the settings are already the NEW layout when it saves, and a state
    /// tagged with the new occupant's name is a state its own plugin
    /// rejects after the files move.
    /// Every loaded plugin's state to disk, keyed by where it is LOADED.
    ///
    /// `only` narrows it to a set of strips. A reorder needs the states of
    /// the rows it is about to move and no others — asking a sampler for its
    /// state is not free, and there are four hundred and fifty seats on this
    /// desk. Before this took an argument, making a folder saved the whole
    /// session's plugins to ask about a handful of rows.
    fn save_plugin_states_as_loaded_for(&mut self, only: Option<&[bool]>) {
        let bays = tangent_ui::recorder::INSERTS;
        for i in 0..self.recorder.inserts_loaded.len() {
            if let Some(want) = only {
                if !want.get(i / bays).copied().unwrap_or(false) {
                    continue;
                }
            }
            let Some(bundle) = self
                .recorder
                .inserts_loaded
                .get(i)
                .and_then(|b| b.clone())
                // The reload marker and the instrument sentinels have no
                // state to ask for; a built-in EFFECT bay answers from its
                // ledger like any LV2 bay and must not be skipped here, or
                // its knobs die at relaunch — the exact disease the sidecars
                // cure.
                .filter(|b| !b.starts_with('\u{1f}') && !is_instrument_sentinel(b))
            else {
                continue;
            };
            let (strip, bay) = (i / bays, i % bays);
            if let Some(state) = self
                .recorder
                .engine
                .as_ref()
                .and_then(|e| e.save_insert_state(strip, bay))
            {
                write_insert_state(strip, bay, &bundle, &state);
            }
        }
    }

    /// Start the monitor output, once, when the band opens.
    ///
    /// A failure here is not fatal and must not be: a machine with no output
    /// device, or one another app holds exclusively, still has a perfectly good
    /// chord display and a perfectly good recorder. The band says what happened
    /// and everything else carries on.
    /// Bring the whole audio path up: the OUTPUT first, with no input
    /// callback running, and the input after it.
    ///
    /// **The order is the whole of this function, and it is a deadlock fix.**
    ///
    /// Changing the buffer size from 64 to 128 hung the app on macOS with a
    /// spinning cursor and no way out but a force quit. The sample is
    /// unambiguous:
    ///
    /// ```text
    /// main thread  AudioOutputUnitStart -> StartIOProc
    ///              -> HALB_Mutex::Lock                    [blocked]
    /// IO thread    our input proc -> AudioUnitGetProperty
    ///              -> recursive_mutex::lock               [blocked]
    /// ```
    ///
    /// The input stream was still RUNNING while the output was rebuilt. Both
    /// are the same interface on the ordinary rig, so starting the output sets
    /// that device's buffer frame size — and the input's next callback then
    /// arrives with a frame count that does not match the one `coreaudio-rs`
    /// cached when the stream was built. Its input proc handles that by asking
    /// CoreAudio for the stream format FROM THE AUDIO CALLBACK, and
    /// reallocating there, which is its own sin. So the IO thread holds the
    /// HAL's lock and blocks on the unit's, while the main thread holds the
    /// unit's and blocks on the HAL's. Neither half of that is ours to fix.
    ///
    /// Closing the input first removes the window rather than racing it: no IO
    /// proc is running while the device is reconfigured. Rebuilding the input
    /// LAST matters just as much — the frame size it caches is then the size
    /// the device already has, so the branch cannot fire on the way back up
    /// either.
    ///
    /// The output has no equivalent hazard: its render callback is handed a
    /// buffer and never asks CoreAudio for anything.
    ///
    /// Every path that starts the engine goes through here, because a rule
    /// about ordering that only two of three callers follow is not a rule.
    fn open_audio_path(&mut self, ctx: &egui::Context) {
        // **Refused outright while a take is running, because the ordering
        // above CANNOT be honoured then.**
        //
        // `close_input` is a no-op during a take, and it has to be: swapping
        // the device out from under a recording ends the recording. So the
        // first line silently does nothing, `start_engine` builds an output
        // unit with the input's IO proc still live, and that is exactly the
        // AB-BA above — main thread inside `AudioComponentInstanceNew`
        // holding the component lock and waiting on the HAL's, input proc
        // holding the HAL's and waiting on the component's inside
        // `AudioUnitGetProperty`. Both threads park forever; the window is
        // the app, and it has to be force-quit.
        //
        // Nothing is lost by refusing. `open_input` declines during a take
        // too, so the only line that would have run is the one that
        // deadlocks — and a take cannot outlive its engine anyway, since the
        // capture rings live in it.
        if self.recorder.session.state().is_active() {
            self.recorder.audio_path_deferred = true;
            return;
        }
        self.recorder.audio_path_deferred = false;
        self.recorder.session.close_input();
        self.start_engine(ctx);
        self.reconcile_audio(true);
    }

    /// Bring the ReaStream sender in line with the engine and the setting.
    ///
    /// Called every frame; nearly always a no-op. The sender is created once
    /// per engine — the moment there is one — taking the once-only send tap even
    /// while streaming is off, so switching it on later is a config flip and
    /// never a re-take. `None` config idles the thread; `Some` targets it.
    #[cfg(feature = "recorder")]
    fn reconcile_reastream(&mut self) {
        use tangent_record::reastream::{ReaStreamConfig, ReaStreamSender};
        if self.recorder.engine.is_none() {
            // No engine, nothing to stream. A sender from a past engine (its
            // producer already gone) is dropped here.
            self.recorder.reastream = None;
            return;
        }
        let enabled = self.app.reastream_enabled();
        let (identifier, host, port) = self.app.reastream_target();
        let target = enabled.then(|| ReaStreamConfig {
            identifier,
            host,
            port,
        });
        if self.recorder.reastream.is_none() {
            let started = self.recorder.engine.as_mut().and_then(|engine| {
                engine.take_reastream_tap().map(|rx| {
                    let rate = engine.output().sample_rate;
                    engine.set_reastream_on(enabled);
                    ReaStreamSender::start(rx, rate, target.clone())
                })
            });
            if let Some(sender) = started {
                self.recorder.reastream = Some(sender);
            }
        } else {
            if let Some(sender) = self.recorder.reastream.as_ref() {
                sender.set_config(target.clone());
            }
            if let Some(engine) = self.recorder.engine.as_ref() {
                engine.set_reastream_on(enabled);
            }
        }
    }

    /// **The rate BOTH halves of the audio path open at.**
    ///
    /// The user's choice when there is one; otherwise the output device's own
    /// rate, imposed on the input as well. See `Engine::default_output_rate`
    /// for the failure this exists to end — two devices at two rates make a
    /// take that drifts, and "Default" used to be the setting that caused it.
    fn effective_sample_rate(&mut self) -> Option<u32> {
        if let Some(chosen) = self.app.sample_rate() {
            return Some(chosen);
        }
        if self.recorder.default_rate.is_none() {
            // One device open, cached until the driver stack changes —
            // including the "there is no output" answer, which costs the same
            // probe and would otherwise be paid for every frame.
            self.recorder.default_rate =
                Some(crate::instrument::Engine::default_output_rate(None));
        }
        self.recorder.default_rate.flatten()
    }

    fn start_engine(&mut self, ctx: &egui::Context) {
        if self.recorder.engine.is_some() {
            return;
        }
        // The RESOLVED rate, so the output and the input open at one rate
        // even when the user has chosen none. See `effective_sample_rate`.
        let rate = self.effective_sample_rate();
        match crate::instrument::Engine::start_sized(
            None,
            self.recorder.session.timebase(),
            self.app.buffer_frames(),
            rate,
        ) {
            Ok(e) => {
                self.recorder.engine = Some(e);
                self.recorder.engine_error = None;
                self.recorder.engine_retry = None;
                self.recorder.engine_watch = crate::instrument::StallWatch::new();
                // The old sender's tap came from the engine just replaced;
                // drop it so `reconcile_reastream` takes a fresh one from this
                // engine. Its thread stops when the socket and consumer drop.
                self.recorder.reastream = None;
                // **And it knows nothing about the song's musical time.** A
                // fresh `Renderer` starts at `Maps::default()` — 120 and 4/4 —
                // while `push_maps` is change-detected against what the LAST
                // engine was told. Leaving the cache alone means the equality
                // check short-circuits on the very next frame and `set_maps`
                // is never called again, so the click plays 120/4/4 for the
                // rest of the session while the ruler draws the real bar
                // lines. Silent, and sticky until somebody edits the tempo.
                //
                // Not covered by `set_tempo`: `BarClock` reads `Renderer::maps`
                // on the audio thread, not the per-frame tempo atomic.
                self.pushed_maps = None;
                self.pushed_auto = None;
                // A fresh engine has nothing loaded, whatever was loaded in
                // the one it replaces — forgetting that here is what asks the
                // reconcile below to fill the bays again.
                self.recorder.inserts_loaded =
                    vec![None; tangent_ui::recorder::INSERT_SPACE];
                // **The tap belongs to the ENGINE, so it is taken when the
                // engine starts.**
                //
                // It used to be taken in one place only: the success branch of
                // loading a VST3. Everything else that reaches the instrument
                // bus — the built-in DX7, the backing track, the click going
                // into the take — therefore had no path into the file at all,
                // and the failure was silent because the monitor still played
                // it. Somebody could load the built-in, play, hear it, watch
                // the meters move, and get a take with only the microphone in
                // it; the `.mid` even had every note. Twelve of twelve
                // manifests on the owner's Linux box said `sources: input`,
                // across three releases, for exactly this reason.
                //
                // `take_recorder_tap` is once-only (`Option::take`), so the
                // VST3 branch's own call is now a no-op that costs nothing and
                // is left where it is: it is the line that says the take has to
                // be able to record what it can hear.
                if let Some(tap) = self
                    .recorder
                    .engine
                    .as_mut()
                    .and_then(|e| e.take_recorder_tap())
                {
                    self.recorder.session.set_plugin_tap(Some(tap));
                }
                // **The fast lane gets this engine's ring.** From here the
                // midir callback feeds the audio callback directly; the old
                // producer (a dead engine's) is dropped inside, on this
                // thread. See `midi::MidiFastLane`.
                if let Some(tx) = self.recorder.engine.as_mut().and_then(|e| e.take_hw_midi()) {
                    self.midi_lane.install(tx);
                }
                self.push_monitor_settings();
                // **Video defaults for a machine with no GPU driver.**
                //
                // Asked here rather than at construction because the probe
                // enumerates drivers, and a launch that never opens the band
                // never films anything. Once ever, and only from the shipped
                // defaults — see `lower_video_defaults_for_cpu`.
                if crate::composite::renders_on_the_cpu() && self.app.lower_video_defaults_for_cpu()
                {
                    log::info!("no GPU driver for video: composite defaults lowered to 720p/15");
                }
                // The chosen patches need no push here any more: a rebuilt
                // engine has no instrument instances, so the per-frame lane
                // sync ships fresh ones and follows each with its strip's
                // own voice — the same road a session load takes.
                // **The document's clips, now that there is an engine to
                // hold them.** A rebuilt engine starts empty whatever the old
                // one held, and the decode cache is keyed to one sample rate
                // — the device that just opened may run another.
                self.track_cache.clear();
                self.stretch_cache.clear();
                self.reload_clips();
                // And the kit, on the same reasoning: the pad cache is
                // keyed to a rate this device may not run.
                self.pad_cache.clear();
                self.reload_pads();
            }
            Err(e) => {
                self.recorder.engine_error = Some(format!("no audio output: {e}"));
                // Try again shortly, a few times. See `engine_retry`.
                let left = self.recorder.engine_retry.map_or(ENGINE_TRIES, |(_, n)| n);
                self.recorder.engine_retry = (left > 0).then(|| {
                    (
                        std::time::Instant::now() + ENGINE_RETRY_AFTER,
                        left.saturating_sub(1),
                    )
                });
            }
        }
    }

    /// Copy the faders, the click and the tempo into the audio thread.
    ///
    /// Every frame, unconditionally. These are all atomic stores of a float or
    /// a bool behind a smoothing ramp, so writing an unchanged value costs
    /// nothing — and a change-detection cache here would be a second copy of
    /// the settings to get out of step with the first.
    /// Hand the desk's routes to the engine, dropping any that the current
    /// device cannot honour.
    ///
    /// Pruning here rather than at the picker: the picker knows what the
    /// device offered when it opened, and this runs every frame — so a route
    /// survives an interface being unplugged and replugged, and is dropped
    /// only against the device that is actually open.
    /// Tell the app how many outputs the open device has, so the routing
    /// picker offers the jacks that exist rather than a guess.
    fn publish_device_outs(&mut self) {
        let outs = self
            .recorder
            .engine
            .as_ref()
            .map_or(2, |e| e.output().channels as usize);
        self.app.set_device_outs(outs);
    }

    fn publish_routes(&self, e: &crate::instrument::Engine) {
        let outs = e.output().channels as usize;
        let mut routes = self.app.routes().clone();
        routes.prune(outs, tangent_ui::recorder::CHANNELS);
        e.set_routes(&routes);
        // **The sidechain key table rides beside the routes**, whole each
        // frame on the same bargain: absent means cleared, so the engine
        // never keeps an edge the desk no longer has. The desk speaks
        // (strip, slot) so saved edges survive seat renumbering; the engine
        // speaks (seat, bay), and THIS is the one place the translation
        // happens — through `seat_bay`, like every other slot address.
        let keys: Vec<(usize, usize, usize)> = self
            .app
            .insert_keys()
            .0
            .iter()
            .filter(|k| k.from < tangent_ui::recorder::CHANNELS)
            .map(|k| {
                let (seat, bay) = tangent_ui::recorder::seat_bay(k.strip, k.slot);
                (seat, bay, k.from)
            })
            .collect();
        engine_set_insert_keys(e, &keys);
    }

    fn push_monitor_settings(&mut self) {
        // **The live tap, if one has been waiting for an engine to appear.**
        //
        // First, and before the `&Engine` borrow below, because handing the
        // ring over needs `&mut`. Here rather than on an edge because this runs
        // every frame the band is open: whenever both halves exist, they are
        // joined, and neither one has to know which was built first.
        if let Some(tap) = self.recorder.pending_monitor.take() {
            match self.recorder.engine.as_mut() {
                Some(e) => e.set_monitor(Some(tap)),
                // No engine yet — put it back and try next frame.
                None => self.recorder.pending_monitor = Some(tap),
            }
        }
        // The SESSION's copy first, and outside the engine gate. It is what the
        // count-in's on-screen beat and the `.mid`'s tempo map are derived
        // from, and neither has anything to do with an output device — so a
        // machine with no monitor (or one another app holds) would have kept
        // counting and writing 4/4 while the band showed 6/8.
        self.recorder.session.set_meter(self.app.time_signature());
        // The placement facts, fresh each frame so `begin` latches this
        // frame's truth: the playhead (where the take will start on the
        // timeline), the head the count-in will leave in the file, and the
        // desk masks the take is cut under.
        {
            let engine = self.recorder.engine.as_ref();
            let rate = engine.map_or(0.0, |e| f64::from(e.output().sample_rate));
            let timeline_start = self.recorder.transport.position_s(engine, rate);
            let head = if self.app.count_in_in_take() {
                let beats = f64::from(self.app.count_in_beats());
                let bpm = self.app.tempo_bpm().max(1.0);
                beats * 60.0 / bpm
            } else {
                // A pre-roll count-in delays the file, it is not IN it.
                0.0
            };
            let desk = self.app.desk();
            let mask = |bits: &[bool]| {
                bits.iter()
                    .enumerate()
                    .fold(0u32, |m, (i, b)| if *b && i < 32 { m | 1 << i } else { m })
            };
            self.recorder.session.set_take_facts(
                timeline_start,
                head,
                mask(&desk.muted),
                mask(&desk.soloed),
            );
        }
        let Some(e) = self.recorder.engine.as_ref() else {
            return;
        };
        let gains = self.app.gains();
        e.set_metronome_gain(gains.metronome);
        e.set_drums(self.app.drums_gain(), self.app.drums_pan());
        // **The routing table, every frame, like every other desk value.**
        // Pruned first against what the device actually has: an interface
        // swapped for a smaller one leaves routes pointing at jacks that are
        // gone, and a fader that moves and does nothing is worse than a
        // route that admitted it went away. See `Routes::prune`.
        self.publish_routes(e);
        e.set_master_gain(gains.master);
        // **The channel that had no fader before there was a mixer**, and the
        // routing that decides what reaches the effects bus. Pushed whole
        // every frame, like the gains above and for the same reason.
        e.set_fx_return(gains.fx_return);
        e.set_desk(&self.app.desk());
        // The drumpad's knobs, whole, on the desk's own bargain: atomic
        // stores of numbers that only move when a hand is on them.
        // **The knobs, to every lane holding a kit.** One document kit still,
        // so they all get the same numbers; per-lane kits change what is sent
        // and not this loop.
        for lane in (0..tangent_ui::recorder::CHANNELS)
            .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
        {
            e.set_pad_params(lane, &self.app.drum_kit_params_of(lane));
        }
        // **The microphone fader, and it goes to ONE place now.** It used to
        // go to two — the session's writer thread and the engine — because the
        // writer was where the dry capture became the take. The take is the
        // desk, so the desk's own fader is the whole of it, and the second
        // copy would have been the fader applied to everything in the file
        // rather than to the microphone.
        // The eight general channels: their faders, which capture pick feeds
        // which of them, which one holds the built-in, and which are NOT fed
        // MIDI — four views of one fact, the channel_kinds array, pushed
        // together so the callback can never see them disagree for long.
        let mut picks: [Option<usize>; tangent_ui::recorder::INPUTS] =
            [None; tangent_ui::recorder::INPUTS];
        let mut next_pick = 0usize;
        let mut builtin_mask = 0u32;
        // Every lane holding the modelled keyboard, gathered in the same walk
        // as the DX7's so the two masks cannot disagree about the desk.
        let mut keys_mask = 0u32;
        let armed = self.app.armed_mask();
        let selected = self.app.selected_channel();
        // **The desk's EXPLICIT input bindings, when any channel has one.**
        // The strip's own menu names inputs now; the moment one channel
        // does, the whole desk routes explicitly — bound channels take
        // their named input, unbound audio channels take NOTHING. With no
        // bindings anywhere the old ordinal walk stands, so nothing anyone
        // set up before the menu existed changes under them.
        let any_explicit = (0..tangent_ui::recorder::CHANNELS).any(|ch| {
            self.app.channel_kind(ch) == Some(tangent_ui::recorder::ChannelKind::Audio)
                && TangentApp::parse_input_spec(self.app.channel_input_spec(ch)).is_some()
        });
        let mut explicit: Vec<tangent_record::audio::ChannelPick> = Vec::new();
        for ch in 0..tangent_ui::recorder::CHANNELS {
            e.set_channel_gain(ch, gains.channels[ch]);
            e.set_channel_pan(ch, self.app.channel_pan(ch));
            match self.app.channel_kind(ch) {
                Some(tangent_ui::recorder::ChannelKind::Midi) => {
                    // Which MIDI lanes hear the keyboard is decided WHOLE by
                    // `recorder::live_midi_off`, below this loop. Three rules
                    // that read each other, one of which silences all the
                    // rest — not a decision to make a bit at a time in the
                    // middle of the input picks.
                    // **EVERY lane holding it, not the first.** `is_none()`
                    // here was the DX7's half of the same bug the drumpad
                    // had: duplicate a built-in instrument track and the
                    // copy took MIDI and made no sound. The mask is the
                    // whole answer now — the first-lane index retired with
                    // the shared instance it belonged to.
                    if self.app.insert(ch, 0) == Some(tangent_ui::dialogs::BUILTIN_PATH) {
                        builtin_mask |= 1 << ch.min(31);
                    }
                    if self.app.insert(ch, 0) == Some(tangent_ui::dialogs::KEYS_PATH) {
                        keys_mask |= 1 << ch.min(31);
                    }
                }
                Some(tangent_ui::recorder::ChannelKind::Audio) => {
                    if any_explicit {
                        if let Some((a, b)) =
                            TangentApp::parse_input_spec(self.app.channel_input_spec(ch))
                        {
                            if next_pick < picks.len() {
                                picks[next_pick] = Some(ch);
                                explicit.push(match b {
                                    Some(b) => tangent_record::audio::ChannelPick::Stereo(a, b),
                                    None => tangent_record::audio::ChannelPick::Mono(a),
                                });
                                next_pick += 1;
                            }
                        }
                    } else if next_pick < picks.len() {
                        picks[next_pick] = Some(ch);
                        next_pick += 1;
                    }
                }
                None => {}
            }
        }
        // **Armed, selected, or alone in the editor** — the whole rule, in
        // one place, tested there. See `recorder::live_midi_off`.
        let kinds: Vec<_> = (0..tangent_ui::recorder::CHANNELS)
            .map(|ch| self.app.channel_kind(ch))
            .collect();
        let midi_off = tangent_ui::recorder::live_midi_off(
            &kinds,
            armed,
            selected,
            self.app.midi_editor_lane(),
        );
        crate::devices::set_channel_picks(&self.recorder.audio, explicit);
        e.set_midi_off(u64::from(midi_off));
        e.set_armed(armed);
        e.set_pick_strips(picks);
        // **The live punch**: while the arrangement records over a lane,
        // everything scheduled on it from the record start is being
        // replaced and must not sound. Pushed every frame like the arms;
        // cleared the same way when the take ends and the carve is real.
        {
            let mut punches: [Option<u64>; tangent_ui::recorder::CHANNELS] =
                [None; tangent_ui::recorder::CHANNELS];
            if let Some(rec) = self.recorder.arr_rec.as_ref() {
                let rate = f64::from(e.output().sample_rate).max(1.0);
                let at = frames_at(rate, rec.start_s);
                for l in &rec.midi_lanes {
                    if *l < punches.len() {
                        punches[*l] = Some(at);
                    }
                }
                for (_, l, _) in &rec.audio {
                    if *l < punches.len() {
                        punches[*l] = Some(at);
                    }
                }
            }
            e.set_punches(punches);
        }
        e.set_builtin_strips(builtin_mask);
        e.set_keys_strips(keys_mask);
        // **Every keyboard lane's own state, every frame.** This used to be
        // one guarded push because a patch change rebuilt the voice pool;
        // that died when the pool learned to carry both mechanisms, so a
        // patch is now a flag the engine compares per block, and three
        // atomic stores per lane per frame is what every fader already
        // costs. The change-detection state this retires is the point: a
        // cached "what the engine holds" is one more thing to disagree.
        for ch in 0..tangent_ui::recorder::CHANNELS {
            if keys_mask & (1 << ch.min(31)) != 0 {
                e.set_keys_patch(ch, keys_patch_of(self.app.keys_patch_of(ch)));
                e.set_keys_params(ch, &self.app.keys_params_of(ch));
            }
        }
        // The drumpad's lanes, by the same bay-1 sentinel rule — any kind of
        // channel counts, because playback does not ask the arm.
        // **Every lane holding the drumpad.**
        //
        // `.find()` alone was the bug: a drum instrument loads a full
        // instance per channel everywhere else, so the second lane was
        // silent while the shared pads lit up for the first. The mask is the
        // answer; the single index remains for the parts of the engine that
        // have not learned about lanes yet, and both come from this one walk
        // so they cannot disagree about which lane is first.
        let drum_lanes: Vec<usize> = (0..tangent_ui::recorder::CHANNELS)
            .filter(|ch| self.app.insert(*ch, 0) == Some(tangent_ui::dialogs::DRUMPAD_PATH))
            .collect();
        e.set_drum_strips(drum_lanes.iter().fold(0u32, |m, ch| m | 1 << ch.min(&31)));
        // **The folder tree**, pushed with everything else so a group made,
        // broken or reordered takes effect on the next block. Refused whole
        // if it is not a forest — see `Engine::set_folders` — because half a
        // tree is a routing graph nobody drew.
        e.set_folders(self.app.folder_parents());
        // Never read from a settings file — see `TangentApp::input_monitor`. It
        // is pushed every frame like every other monitor setting, and its value
        // at launch is false because the field it comes from starts false.
        e.set_monitor_on(self.app.input_monitor());
        // The backing track's level, pushed with the rest for the same
        // reason: the settings are the one live value, so a fader moved, a
        // project loaded and a hand-edited file all arrive by one path.
        //
        // **The transport is NOT pushed here.** This function runs before the
        // session ticks, so a level derived here lags the state change by one
        // frame — the take's first block used to start from the old position.
        // See `Transport::push`, called after the tick.
        let track_gain = self.app.track_playback();
        e.set_track_gain(track_gain);
        // Pushed every frame with the gains, and for the same reason: the
        // settings are the one live value, so a knob dragged, a project loaded
        // and a settings file hand-edited all arrive by the same path.
        let [reverb, delay, chorus, phaser, hpf, lpf, gain, limiter] = self.app.effect_sends();
        e.set_effects(crate::effects::Sends {
            reverb,
            delay,
            chorus,
            phaser,
            hpf,
            lpf,
            gain,
            limiter,
        });
        // The parameters too, and `set_effect_params` is what makes this cheap:
        // it compares before it takes the lock, so pushing the same eleven
        // numbers sixty times a second costs one comparison.
        e.set_effect_params(effect_params_from(self.app.effect_params()));
        // **And the varispeed, with the same knobs and for the same reason.**
        // In an EXPORT this reaches the offline renderer exactly as it
        // reaches the live one — the owner's words were that it "applies to
        // exports when engaged", and it does so by being one more thing the
        // desk is pushed rather than a special case anywhere.
        e.set_varispeed(self.app.varispeed_ratio());
        e.set_metronome_enabled(self.app.metronome_on());
        e.set_metronome_in_take(self.app.metronome_in_take());
        e.set_tempo(self.app.tempo_bpm());
        // The signature drives both halves of the click: which beat is accented
        // and how long a beat lasts. In 6/8 those are "every sixth" and "half a
        // quarter" — get the second wrong and the count-in is twice as long as
        // the bar it is counting.
        // NOT while a take is running. `Session::set_meter` refuses mid-take —
        // a `.mid` whose bar lines change halfway through is a file nobody can
        // edit — and pushing it to the engine anyway would move the click and
        // the accent while the countdown and the file kept the old meter. One
        // setting must not have two live values.
        if !self.recorder.session.is_recording() {
            let sig = self.app.time_signature();
            e.set_meter(u32::from(sig.beats), u32::from(sig.unit));
            // Beside the meter, because it is read with it: how many clicks
            // a beat of that signature carries.
            e.set_metronome_div(self.app.metronome_div());
            // Polarity, one bit per strip — see `Shared::strip_phase`.
            e.set_strip_phase(self.app.strip_phase());
        }
        // **The instances themselves, last, with the `&mut` the walk above
        // never needed.** Everything before this line is an atomic store;
        // shipping and retiring whole synths is ring work, so it waits for
        // the shared borrow to end. A DX7 lane born this frame is followed
        // by its strip's own voice — the cartridge cache makes that a map
        // lookup, not a file read — and a keyboard lane needs no follow-up,
        // its patch and knobs being per-lane atomics pushed above.
        let made_dx7 = self
            .recorder
            .engine
            .as_mut()
            .map_or(0, |e| e.sync_instrument_lanes(builtin_mask, keys_mask).0);
        for ch in 0..tangent_ui::recorder::CHANNELS.min(32) {
            if made_dx7 & (1 << ch) != 0 {
                let v = self.voice_for_strip(ch);
                if let Some(e) = self.recorder.engine.as_mut() {
                    e.set_builtin_voice(ch, v);
                }
            }
        }
    }

    /// Give the engine the tempo and meter maps, when they have changed.
    ///
    /// **On CHANGE and not every frame**, unlike everything else this crate
    /// pushes. The other setters are atomic stores and cost a comparison; a
    /// map is a `Vec` crossing on a ring, and `set_maps` waits up to
    /// `RETIRE_TIMEOUT` for the displaced pair to come back so it can be
    /// dropped here rather than in the callback. Running that sixty times a
    /// second would be a stall looking for somewhere to happen.
    #[cfg(feature = "recorder")]
    fn push_maps(&mut self) {
        let want = crate::instrument::Maps {
            tempo: self.app.live_tempo_map(),
            meter: self.app.live_meter_map(),
        };
        if self.pushed_maps.as_ref() == Some(&want) {
            return;
        }
        if let Some(e) = self.recorder.engine.as_mut() {
            e.set_maps(want.clone());
            self.pushed_maps = Some(want);
        }
    }

    /// The automation curves, on the same change-detected bargain.
    #[cfg(feature = "recorder")]
    fn push_automation(&mut self) {
        let want = self.app.engine_automation();
        if self.pushed_auto.as_ref() == Some(&want) {
            return;
        }
        if let Some(e) = self.recorder.engine.as_mut() {
            e.set_automation(want.clone());
            self.pushed_auto = Some(want);
        }
    }

    /// Which lanes the curves must leave alone, pushed every frame.
    ///
    /// Unlike the curves themselves this is one atomic word, so it costs a
    /// store and must NOT be change-detected: it has to take effect on the
    /// next block or a fader being written fights the curve under the hand.
    #[cfg(feature = "recorder")]
    fn push_auto_suspend(&mut self) {
        let mask = self.app.auto_suspend_mask();
        if let Some(e) = self.recorder.engine.as_ref() {
            e.set_auto_suspend(mask);
        }
    }

    /// What this take is going to be missing, and where to fix it.
    ///
    /// **Only sources that EXIST.** A muted slot with no instrument in it is
    /// not a loss, it is an empty channel — and naming five of those every
    /// take is how a status line stops being read.
    ///
    /// Named individually rather than counted, because "a source is muted" on
    /// a desk with eight of them is not an instruction anybody can follow.
    fn muted_out_of_the_take(&self) -> Option<String> {
        if !self.recorder.session.is_recording() {
            return None;
        }
        use tangent_ui::recorder::Strip;
        let desk = self.app.desk();
        let mut lost: Vec<Strip> = Vec::new();
        // Every channel with something ON it, by name: a MIDI channel with an
        // instrument in bay 1, an AUDIO channel whose pick is open. An unused
        // channel is not a loss; a piano that is loaded and muted is.
        for ch in 0..tangent_ui::recorder::CHANNELS {
            let strip = Strip::Channel(ch);
            let live = match self.app.channel_kind(ch) {
                Some(tangent_ui::recorder::ChannelKind::Midi) => {
                    self.app.insert(strip.index(), 0).is_some()
                }
                Some(tangent_ui::recorder::ChannelKind::Audio) => self
                    .app
                    .audio_ordinal(ch)
                    .and_then(|k| self.recorder.input_names.get(k))
                    .is_some_and(|n| !n.is_empty()),
                None => false,
            };
            if live && !desk.heard(strip) {
                lost.push(strip);
            }
        }
        // The backing track has no strip to mute since v13 retired BACKING —
        // it plays whenever it is loaded, so it can never be "lost".
        tangent_ui::recorder::missing_from_take(&lost)
    }

    /// Open the camera the user asked for, if it is not already open.
    ///
    /// **`open_camera` blocks for 300-800 ms** (over two seconds for a
    /// Continuity Camera), which is why this runs after the frame and why
    /// opening the band rather than pressing Record is what triggers it.
    ///
    /// Unlike the audio path there is no "system default": a camera nobody
    /// asked for must never be opened, because opening one turns on a light on
    /// the front of the machine.
    fn reconcile_camera(&mut self, force: bool, ctx: &egui::Context) {
        let sel = crate::devices::selection(&self.recorder.camera);
        if !sel.is_stale() && !force {
            return;
        }
        let wanted = sel.wanted;
        // Announce first, act next frame — but only when there is something to
        // wait for. Closing is instant, so making the user watch a frame of
        // "starting the camera…" in order to turn one OFF would be silly.
        if wanted.is_some() && !self.recorder.camera_opening {
            self.recorder.camera_opening = true;
            ctx.request_repaint();
            return;
        }
        self.recorder.camera_opening = false;
        // No `if wanted.is_none() { return }` guard here, and the guard that
        // used to be here was a bug: choosing "None - record without video"
        // left the selection stale forever, so the camera went on running with
        // its light on and its preview updating after the user said stop.
        // `open_camera(None)` closes it, which is what None means.
        self.recorder.session.open_camera(wanted.as_deref());
        let name = self
            .recorder
            .session
            .camera_format()
            .map(|f| format!("{}x{}", f.width, f.height));
        crate::devices::settle(
            &self.recorder.camera,
            wanted,
            name,
            self.recorder.session.camera_error().map(str::to_owned),
        );
    }

    /// Open the audio input the user asked for, if it is not already open.
    fn reconcile_audio(&mut self, force: bool) {
        // **The session's device IS the settings' device, always.** The
        // owner's interface selection has now been lost from the file twice
        // while a session that could record kept running — some path opened
        // the box without persisting the choice. Whatever opened it, the
        // moment a device is wanted it is written down; a file that
        // disagrees with a working session is how "input monitoring is
        // broken" happens a day later.
        {
            let wanted = crate::devices::selection(&self.recorder.audio).wanted;
            if let Some(uid) = wanted {
                if self.app.chosen_audio_uid() != Some(uid.as_str()) {
                    self.app.remember_audio_device(&uid);
                }
            }
        }
        // **The extras, pushed before the staleness check reads it.** Choosing
        // a second input changes which channels the take keeps — and used to
        // change how many the STREAM carried, which is why it was a reopen.
        crate::devices::set_extra_inputs(&self.recorder.audio, self.app.extra_inputs().to_vec());
        let (stale, same_device) = {
            let sel = crate::devices::selection(&self.recorder.audio);
            (
                sel.is_stale(),
                sel.wanted.is_some() && sel.wanted == sel.open,
            )
        };
        if !stale && !force {
            return;
        }
        // **The live path: the device stays open and the picks change under
        // it.** The rings carry whole device frames now, so choosing an input
        // is a message to the writer and a layout note to the renderer -
        // nothing is closed, nothing is allocated, and nothing goes quiet.
        // Only when the DEVICE itself is unchanged: a different interface is
        // a different stream and has to be opened.
        if stale && !force && same_device {
            if let Some(choice) = crate::devices::audio_selection(&self.recorder.audio) {
                if self.recorder.session.set_picks(choice.channels) {
                    // The monitor keeps its ring — the frames in it mean the
                    // same thing before and after — and is told which columns
                    // each strip owns now.
                    if let Some(e) = self.recorder.engine.as_ref() {
                        let dev = self.recorder.session.device_channels().unwrap_or(2);
                        e.set_monitor_layout(
                            choice
                                .channels
                                .widths::<{ tangent_ui::recorder::INPUTS }>(dev),
                            choice.channels,
                        );
                    }
                    let opened = crate::devices::selection(&self.recorder.audio).wanted;
                    crate::devices::settle(
                        &self.recorder.audio,
                        opened,
                        self.recorder.session.audio_device_name().map(str::to_owned),
                        self.recorder.session.audio_error().map(str::to_owned),
                    );
                    return;
                }
            }
        }
        match crate::devices::audio_selection(&self.recorder.audio) {
            Some(choice) => {
                // The engine's own device object rides along for ASIO duplex:
                // a single-client driver's input MUST join the output's
                // stream set, not open a second one. Ignored on every other
                // backend. See `tangent_record::audio::open_input`.
                // **The same rate the ENGINE opened at**, which is the whole
                // point of resolving Default: the writer drains the ring at
                // the input's rate while the engine fills it at the output's,
                // and two devices left to their own defaults drift apart.
                // Taken before `duplex_with` borrows the engine.
                let rate = self.effective_sample_rate();
                let duplex_with = self
                    .recorder
                    .engine
                    .as_ref()
                    .and_then(|e| e.output_device());
                self.recorder.session.open_input(
                    &choice.selection,
                    choice.channels,
                    self.app.buffer_frames(),
                    rate,
                    duplex_with,
                );
                // **The live tap, HELD until there is an engine to play it.**
                //
                // Taking it and dropping it on the floor is what the first
                // draft did, and the tap is made once per device open — so
                // whenever the engine did not exist at this exact moment,
                // monitoring was silently dead for the rest of the session
                // with no way to get it back short of re-picking the device.
                // That is not a rare window: `start_engine` fails and retries
                // on a busy output, and every retry lands here after the input
                // is already open.
                if let Some(tap) = self.recorder.session.take_monitor() {
                    self.recorder.pending_monitor = Some(tap);
                }
            }
            // The user picked "None - record MIDI only". Mapping that to the
            // system default (which is what happened before `explicit` existed)
            // opened the built-in microphone and put its name in the band.
            None => self.recorder.session.close_input(),
        }
        let opened = crate::devices::selection(&self.recorder.audio).wanted;
        crate::devices::settle(
            &self.recorder.audio,
            opened,
            self.recorder.session.audio_device_name().map(str::to_owned),
            self.recorder.session.audio_error().map(str::to_owned),
        );
        // **The device's width, measured HERE — on the reconcile edge — and
        // pushed once.** The UI reads the stored number every frame; asking
        // `AudioSetup::input_channels` per frame enumerates every device
        // through CoreAudio (cpal builds an AudioUnit per query) and buried
        // the app under blocked IPC — the owner's "ludicrous" lag.
        self.app
            .set_input_width(crate::devices::wanted_device_width(&self.recorder.audio));
        // **A multichannel interface arrives fully exposed** — every mono
        // input and the natural pairs, ticked the first time the device is
        // met. Once per device: the chooser can prune, and pruning sticks.
        if let Some((key, exposed)) =
            crate::devices::auto_expose(&self.recorder.audio, self.app.auto_exposed_devices())
        {
            self.app.note_auto_exposed(key, exposed);
        }
    }
}

/// Where one insert bay's plugin state lives. See [`state_path`] for why a
/// sidecar; keyed by CHANNEL AND BAY because that is what an insert is keyed
/// by, and validated by bundle path on the way back for the same reason the
/// slot files are — the wrong plugin's state is not a preset, it is arbitrary
/// bytes to a `setState` that will believe them.
#[cfg(feature = "recorder")]
/// Copy a directory tree, best effort — the project system's clone.
/// How many files are sitting in a project's `media/`.
///
/// **The one folder in a scratch project that holds work.** The document,
/// the drumpad sidecar and the plugin-state shelf are all re-creatable; a
/// recording is not. Files only, and one level deep, which is exactly how
/// `copy_into_media` writes them.
///
/// An unreadable directory counts as ZERO and the reclaim proceeds — the same
/// way a missing one does. Refusing to start a new project because a folder
/// could not be listed would turn a permissions problem into a stuck app.
fn count_media(project_dir: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(project_dir.join("media")) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .count()
}

/// Whether the folder a launch reclaim landed on may be deleted and
/// re-cloned. No takes — a recording nobody named is still the one thing
/// that cannot be played a second time — and a document still
/// byte-identical to the template's, because a scratch begins life as a
/// verbatim clone and stays one until somebody edits it (every undo writes
/// the document). A built kit never reaches here: `fresh_project_dir`
/// refuses those folders outright, for New and launch alike.
#[cfg(feature = "recorder")]
fn launch_reclaimable(dir: &std::path::Path, template: &std::path::Path) -> bool {
    count_media(dir) == 0
        && file_eq(
            &tangent_ui::arrangement::doc_in(dir),
            &tangent_ui::arrangement::doc_in(template),
        )
}

/// Byte-for-byte equality of two files; an unreadable file equals nothing.
#[cfg(feature = "recorder")]
fn file_eq(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// The next free `Untitled n` at or after `taken`, for a New that must not
/// reclaim the scratch it landed on.
///
/// Deliberately not `fresh_project_dir` again: that function's whole job is
/// to RECLAIM an unsaved Untitled, so asking it a second time hands back the
/// same folder. This one skips every directory that exists, saved or not.
fn next_untitled(root: &std::path::Path, taken: &std::path::Path) -> std::path::PathBuf {
    for n in 2..10_000 {
        let candidate = root.join(format!("Untitled {n}"));
        if candidate != taken && !candidate.exists() {
            return candidate;
        }
    }
    taken.to_path_buf()
}

/// Whether AccessKit is live — which is to say, whether an assistive
/// technology has actually attached.
///
/// **Lazy on every platform, which is the whole reason this is a question.**
/// `accesskit_winit` does not build a tree until something asks for one, and
/// when it does it calls `egui::Context::enable_accesskit`. So this is not
/// "was the app compiled with accesskit" — it is "is somebody reading the
/// screen right now", and the app can stay on its native windows until they
/// are.
///
/// Asked of the PLATFORM where the platform has an answer, and not at all
/// where it does not. See the body: the old form asked egui whether it was
/// building an accesskit tree, which is a question about the build and not
/// about the user.
///
/// No feature gate, and that is deliberate: `eframe`'s `accesskit` is taken
/// unconditionally in `tangent/Cargo.toml`, so a Minimal build has it too. A
/// person using a screen reader should not have to run the build that can
/// load arbitrary third-party code in order to be able to read the app.
fn screen_reader_attached(_ctx: &egui::Context) -> bool {
    // **macOS and Windows ask the OS**, which has a real answer with no egui
    // in hand — see `os_reader_attached`, which `Settings::load` also asks
    // before any context exists.
    #[cfg(any(target_os = "macos", windows))]
    {
        os_reader_attached()
    }
    // **Elsewhere the probe means what it says.** On Linux egui's accesskit
    // tree is built when AT-SPI asks for it, so Orca being up is exactly
    // what makes this true.
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        _ctx.accesskit_node_builder(egui::Id::new("ivory-a11y-probe"), |_| ())
            .is_some()
    }
}

/// The OS's own answer to "is a screen reader running right now", askable
/// with no egui context in hand — which is what lets `main` ask it before
/// eframe exists, where `Settings::load` needs it to tell a poisoned
/// pre-v20 `screen_reader_mode` apart from a setting a reader user chose.
///
/// **macOS asks AppKit, and used to answer `false` unconditionally.** The
/// old accesskit probe was meaningless here — the adapter builds a tree
/// whenever the feature is compiled in, so it said "a reader is attached"
/// on every Mac, always, and it was believed: this machine has VoiceOver
/// switched off and had `screen_reader_mode: true` written into its
/// settings file, which turned every dialog into an inline layer, disabled
/// the detached windows, and (with the anchors still computed in screen
/// points) threw every menu into a corner. The hardcoded `false` that
/// replaced it made no VoiceOver user's dialogs come inline unless they
/// found the mouse-only menu row. `NSWorkspace.isVoiceOverEnabled` is the
/// honest question — objc2-app-kit is a dependency this crate already
/// takes for `macwin`, so there is no longer any reason not to ask it.
/// A property read with no side effect, safe per the bindings, cheap
/// enough to poll.
///
/// Linux is `false` here on purpose, not an oversight: its real answer
/// needs the egui context (the AT-SPI-driven probe in
/// `screen_reader_attached`), and the lazy probe there never persisted a
/// false positive for the settings one-shot to care about.
pub(crate) fn os_reader_attached() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().isVoiceOverEnabled()
    }
    // **Windows asks the system, which has a real answer.**
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPI_GETSCREENREADER,
        };
        let mut on: i32 = 0;
        // SAFETY: `on` is a valid, owned i32 for the duration of the call,
        // and SPI_GETSCREENREADER writes exactly a BOOL through the pointer.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETSCREENREADER,
                0,
                std::ptr::from_mut(&mut on).cast(),
                0,
            )
        };
        ok != 0 && on != 0
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        false
    }
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Rename the channel rows' `insert-state-<strip>-<bay>.bin` files in `dir`
/// to follow a reorder: `order[new] = old`. Read whole, then write whole —
/// a rename chain would eat itself on any rotation.
/// Which insert slots a permutation actually disturbs.
///
/// `order[new] = old`, so a row with `order[n] == n` kept its plugin, its
/// state and its bays and has nothing to reload. Its own function because
/// the alternative — marking the whole desk — is what made a reorder take
/// seconds, and a property that costs seconds when it is wrong deserves a
/// test that does not need a window.
/// **The bays a reorder really has to reload.**
///
/// A moved row is not enough. The forced reload exists because a plugin's
/// STATE travels with its track — two channels holding the same plugin can
/// swap, leaving the path identical at both seats and the settings wrong at
/// both — so a bay whose path is unchanged still has to be re-instantiated.
/// But a bay that is EMPTY on both sides of the move has no plugin and no
/// state and nothing to do, and marking it makes the reconcile spend a frame
/// finding that out.
///
/// That mattered because a folder is inserted ABOVE its children: making one
/// shifts every row below it, so "the rows that moved" is very nearly the
/// whole desk, and the desk is mostly empty. `loaded` is what each seat has
/// in it now; a pair of `None`s is a pair of nothing.
/// The host's per-seat record of what is loaded, permuted with the channels.
///
/// Flat `strip * INSERTS + bay`, and only the channel rows move: the buses,
/// the master and the pads keep their seats.
fn permute_loaded_rows(rows: &mut [Option<String>], order: &[usize]) {
    let bays = tangent_ui::recorder::INSERTS;
    let n = order.len();
    let mut held: Vec<Option<String>> = Vec::with_capacity(n * bays);
    for new in 0..n {
        for b in 0..bays {
            held.push(rows.get(order[new] * bays + b).and_then(Clone::clone));
        }
    }
    for (i, v) in held.into_iter().enumerate() {
        if let Some(slot) = rows.get_mut(i) {
            *slot = v;
        }
    }
}

fn reload_slots_for_loaded(order: &[usize], bays: usize, loaded: &[Option<String>]) -> Vec<usize> {
    let mut out = Vec::new();
    let held = |strip: usize, bay: usize| -> bool {
        loaded
            .get(strip * bays + bay)
            .and_then(|b| b.as_deref())
            .is_some_and(|b| !b.is_empty())
    };
    for (new, old) in order.iter().enumerate() {
        if new == *old {
            continue;
        }
        for b in 0..bays {
            // Something was here, or something is coming here. Either way
            // the seat changes; anything else is two empties trading places.
            if held(new, b) || held(*old, b) {
                out.push(new * bays + b);
            }
        }
    }
    out
}

#[cfg(test)]
fn reload_slots_for(order: &[usize], bays: usize) -> Vec<usize> {
    let loaded: Vec<Option<String>> =
        (0..order.len() * bays).map(|_| Some("x".to_owned())).collect();
    reload_slots_for_loaded(order, bays, &loaded)
}

fn permute_insert_states_in(dir: Option<std::path::PathBuf>, order: &[usize]) {
    let Some(dir) = dir else {
        return;
    };
    let name = |strip: usize, bay: usize| dir.join(format!("insert-state-{strip}-{bay}.bin"));
    let bays = tangent_ui::recorder::INSERTS;
    // **Only the rows that MOVED.** A row whose files are already where they
    // belong is read and written back byte for byte otherwise, and a plugin
    // state can be megabytes — a sampler's especially. Making a folder shifts
    // most of the desk, so this ran over nearly every file in the session,
    // twice (two directories), on every drag: the owner's "any and every
    // operation regarding dragging children in and out produces spinning
    // wheel for 2-5 seconds".
    let mut held: Vec<Vec<Option<Vec<u8>>>> = Vec::new();
    for new in 0..order.len() {
        let old = order[new];
        if old == new {
            held.push(Vec::new());
            continue;
        }
        let mut row = Vec::with_capacity(bays);
        for b in 0..bays {
            match std::fs::read(name(old, b)) {
                Ok(bytes) => row.push(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => row.push(None),
                // A file that EXISTS but cannot be read (EIO, a dataless
                // Dropbox placeholder): abort whole — a permutation that
                // deletes what it could not carry is data loss.
                Err(_) => return,
            }
        }
        held.push(row);
    }
    for (new, row) in held.into_iter().enumerate() {
        // An untouched row was never read; writing it back would be writing
        // what is already there.
        if row.is_empty() {
            continue;
        }
        for (b, data) in row.into_iter().enumerate() {
            match data {
                Some(bytes) => {
                    let _ = std::fs::write(name(new, b), bytes);
                }
                None => {
                    let _ = std::fs::remove_file(name(new, b));
                }
            }
        }
    }
}

fn insert_state_path(strip: usize, bay: usize) -> std::path::PathBuf {
    let dir = Settings::path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    dir.join(format!("insert-state-{strip}-{bay}.bin"))
}

/// The preset drawer drum KITS live in, under `Presets/`. Kits keep their
/// own full-path save road (`SavePurpose::KitPreset` honours the whole
/// path), so this is only the drawer the save panel OPENS on — named here
/// beside [`preset_subfolder`] so the two spellings cannot drift apart.
#[cfg(feature = "recorder")]
const DRUM_PRESET_FOLDER: &str = "Tangent Drums";

/// The folder one unit's presets live in, under `Presets/`.
///
/// Derived from the unit's IDENTITY, human-readable on purpose — these are
/// folders people browse in a file manager. A Keys patch ("keys:2") files
/// under one shared drawer because the patches are one instrument's eight
/// sounds; a built-in effect files under its class name ("@tangent-eq" →
/// "Tangent EQ") via the table the loader already reads, so the folder is
/// spelled exactly as the bay's chip is. A sentinel this build has no row
/// for — a newer build's unit — still gets a stable folder of its own: the
/// sanitized identity, `@` shed. Never raw user text: everything joined
/// onto the root here has been through a table lookup or the sanitizer.
#[cfg(feature = "recorder")]
fn preset_subfolder(unit: &str) -> String {
    if unit == "keys" || unit.starts_with("keys:") {
        return "Tangent Keys".to_owned();
    }
    if let Some(spec) = crate::builtin_fx::spec_for(unit) {
        return spec.class.to_owned();
    }
    tangent_ui::preset::safe_stem(unit.trim_start_matches('@'))
}

/// What `DeletePreset` may actually remove: `path`, resolved, IF it lands
/// inside the preset `root` wearing our extension — `None` for everything
/// else. Its own function so the one guard between a request string and
/// `remove_file` is the thing a test holds, not a private method on the
/// whole app.
#[cfg(feature = "recorder")]
fn preset_delete_target(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Option<std::path::PathBuf> {
    // Canonicalized on BOTH sides, so `..` segments and symlinks in the
    // request cannot walk out of a root that compares equal as a string.
    let root = root.canonicalize().ok()?;
    let canon = path.canonicalize().ok()?;
    if !canon.starts_with(&root) {
        return None;
    }
    canon
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(tangent_ui::preset::PRESET_EXT))
        .then_some(canon)
}

#[cfg(feature = "recorder")]
/// A file stem the filesystem will take: separators out, trimmed, never
/// empty.
#[cfg(feature = "recorder")]
fn sanitize_file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if std::path::is_separator(c) || c == ':' {
                '-'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim();
    if s.is_empty() {
        "mix".to_owned()
    } else {
        s.to_owned()
    }
}

/// The format pass over freshly written WAVs: encode an MP3 beside each
/// where the spec asks for one, and remove the WAV where it does not.
#[cfg(feature = "recorder")]
fn deliver_formats(
    paths: &[std::path::PathBuf],
    spec: &tangent_ui::ports::ProjectExportSpec,
) -> Result<(), String> {
    deliver_formats_inner(paths, spec, false)
}

/// As [`deliver_formats`]; `rewrite_depth` re-encodes the WAVs at the
/// spec's word size — the REALTIME pass records float and delivers copies,
/// where the offline writers already wrote the asked depth directly.
#[cfg(feature = "recorder")]
fn deliver_formats_inner(
    paths: &[std::path::PathBuf],
    spec: &tangent_ui::ports::ProjectExportSpec,
    rewrite_depth: bool,
) -> Result<(), String> {
    use tangent_ui::ports::ExportFormat;
    let want_wav = spec.primary == ExportFormat::Wav || spec.secondary == Some(ExportFormat::Wav);
    let want_mp3 = spec.primary == ExportFormat::Mp3 || spec.secondary == Some(ExportFormat::Mp3);
    for path in paths {
        if rewrite_depth && want_wav && spec.depth != tangent_ui::ports::ExportDepth::Float32 {
            let decoded = tangent_record::decode::decode(path, spec.rate)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let wav_spec = tangent_record::wav::WavSpec {
                sample_rate: spec.rate,
                channels: 2,
                format: match spec.depth {
                    tangent_ui::ports::ExportDepth::Pcm16 => tangent_record::wav::SampleFormat::Int16,
                    _ => tangent_record::wav::SampleFormat::Int24,
                },
            };
            let bext =
                tangent_record::wav::Bext::new(tangent_record::wav::Wallclock::now_utc(), wav_spec);
            let tmp = path.with_extension("depth.tmp.wav");
            let mut w = tangent_record::wav::WavWriter::create(&tmp, wav_spec, &bext)
                .map_err(|e| format!("{}: {e}", tmp.display()))?;
            let _ = w.write_interleaved(&decoded.samples);
            w.finish().map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        }
        if want_mp3 {
            let decoded = tangent_record::decode::decode(path, spec.rate)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            encode_mp3(
                &decoded.samples,
                spec.rate,
                spec.mono,
                &path.with_extension("mp3"),
            )?;
        }
        if !want_wav {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

/// Interleaved stereo f32 in, a finished MP3 on disk out. 320 kbps, best
/// quality — an export is the one place nobody wants a knob for this.
#[cfg(feature = "recorder")]
#[cfg(feature = "mp3")]
fn encode_mp3(stereo: &[f32], rate: u32, mono: bool, path: &std::path::Path) -> Result<(), String> {
    use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, MonoPcm, Quality};
    let mut b = Builder::new().ok_or("mp3: the encoder would not initialise")?;
    b.set_num_channels(if mono { 1 } else { 2 })
        .map_err(|e| format!("mp3: {e}"))?;
    b.set_sample_rate(rate).map_err(|e| format!("mp3: {e}"))?;
    b.set_brate(Bitrate::Kbps320)
        .map_err(|e| format!("mp3: {e}"))?;
    b.set_quality(Quality::Best)
        .map_err(|e| format!("mp3: {e}"))?;
    let mut enc = b.build().map_err(|e| format!("mp3: {e}"))?;
    let frames = stereo.len() / 2;
    let mut out: Vec<u8> = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(frames));
    if mono {
        let folded: Vec<f32> = (0..frames)
            .map(|f| (stereo[f * 2] + stereo[f * 2 + 1]) * 0.5)
            .collect();
        enc.encode_to_vec(MonoPcm(&folded), &mut out)
            .map_err(|e| format!("mp3: {e}"))?;
    } else {
        let left: Vec<f32> = (0..frames).map(|f| stereo[f * 2]).collect();
        let right: Vec<f32> = (0..frames).map(|f| stereo[f * 2 + 1]).collect();
        enc.encode_to_vec(
            DualPcm {
                left: &left,
                right: &right,
            },
            &mut out,
        )
        .map_err(|e| format!("mp3: {e}"))?;
    }
    enc.flush_to_vec::<FlushNoGap>(&mut out)
        .map_err(|e| format!("mp3: {e}"))?;
    std::fs::write(path, &out).map_err(|e| format!("mp3: {e}"))?;
    Ok(())
}

/// No in-process LAME (the Windows cross-build) — encode through the bundled
/// ffmpeg instead, so MP3 export works there too. See
/// [`tangent_record::encode::encode_mp3_via_ffmpeg`].
#[cfg(feature = "recorder")]
#[cfg(not(feature = "mp3"))]
fn encode_mp3(
    interleaved: &[f32],
    rate: u32,
    mono: bool,
    path: &std::path::Path,
) -> Result<(), String> {
    tangent_record::encode::encode_mp3_via_ffmpeg(interleaved, rate, mono, path)
}

fn saved_insert_state(strip: usize, bay: usize, bundle: &str) -> Option<Vec<u8>> {
    let raw = std::fs::read(insert_state_path(strip, bay)).ok()?;
    let split = raw.iter().position(|b| *b == 0)?;
    let owner = std::str::from_utf8(&raw[..split]).ok()?;
    (owner == bundle).then(|| raw[split + 1..].to_vec())
}

#[cfg(feature = "recorder")]
fn write_insert_state(strip: usize, bay: usize, bundle: &str, state: &[u8]) {
    let mut out = Vec::with_capacity(bundle.len() + 1 + state.len());
    out.extend_from_slice(bundle.as_bytes());
    out.push(0);
    out.extend_from_slice(state);
    let _ = std::fs::write(insert_state_path(strip, bay), out);
}

/// `/Users/x/Movies/Tangent` reads as `~/Movies/Tangent`.
///
/// Not cosmetic at this width: the band's destination line has room for about
/// forty characters, and a home directory eats a quarter of them saying nothing
/// the user does not already know.
#[cfg(feature = "recorder")]
fn shorten_home(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().into_owned();
    let Some(home) = dirs::home_dir() else {
        return text;
    };
    let home = home.to_string_lossy();
    match text.strip_prefix(home.as_ref()) {
        Some(rest) => format!("~{rest}"),
        None => text,
    }
}

// ── the IR wizard's host-side state and math ────────────────────────────────

/// Everything the host holds for the IR road between frames. One struct so
/// the desktop grows one field however much the wizard learns to do.
#[derive(Default)]
struct IrRoad {
    /// The pass the engine is capturing right now, with everything the
    /// worker will need once the buffer comes back.
    pending: Option<IrPending>,
    /// The loop-only (bypass) pass, held to divide the next real pass — the
    /// exact deconvolution route, which cancels the interface and its
    /// latency in the quotient.
    reference: Option<IrReference>,
    /// The worker thread chewing a finished capture, if one is out.
    worker: Option<std::sync::mpsc::Receiver<(Vec<f32>, Vec<f32>, tangent_ui::ports::IrOutcome)>>,
    /// The processed pair, waiting for Save (or a retry to replace it).
    finished: Option<IrFinished>,
    /// The picture pushed to the app — see `ports::IrLive`.
    live: tangent_ui::ports::IrLive,
    /// The road moved last frame, so it gets one more push as it settles.
    was_busy: bool,
    /// The launch scan has run — once, on the first frame, when the app
    /// exists to receive the rows.
    scanned: bool,
}

/// What `start_ir_capture` remembers about the armed pass.
struct IrPending {
    reference: bool,
    sweep: Vec<f32>,
    /// Pre-roll silence frames — the noise-floor read.
    pre: usize,
    rate: f64,
    f1: f64,
    f2: f64,
    /// Sweep length in seconds, for the harmonic-advance arithmetic.
    duration: f64,
    /// Whole capture length in frames, the reference-match key.
    total: usize,
}

/// A held reference pass and the shape it was captured with. Only a pass
/// with the SAME shape may be divided by it — the IRU lock semantics.
struct IrReference {
    capture: Vec<f32>,
    sweep_len: usize,
    total: usize,
}

/// The processed stereo pair, ready to be written.
struct IrFinished {
    irs: [Vec<f32>; 2],
    rate: u32,
}

/// How many points the wizard's envelope picture carries. Finer than any
/// panel is wide, coarse enough to push every frame without a thought.
const IR_ENVELOPE_POINTS: usize = 360;

/// The whole measurement, capture in, shippable IR out — every step is the
/// verified recipe's, in its order, on `units::sweep`'s math.
///
/// The QC numbers ride the CLASSIC route (time-reversed-sweep convolution)
/// because that is the route whose negative times hold the harmonic images
/// where the probe reads them; the exported IR takes the REFERENCE division
/// when a loop-only pass exists, because that route is exact — latency,
/// converter ripple and interface coloration cancel in the quotient.
#[allow(clippy::too_many_arguments)]
fn process_ir_capture(
    capture: &[f32],
    sweep: &[f32],
    reference: Option<&[f32]>,
    pre: usize,
    rate: f64,
    f1: f64,
    f2: f64,
    duration: f64,
) -> (Vec<f32>, Vec<f32>, tangent_ui::ports::IrOutcome) {
    use crate::units::sweep as ess;
    let frames = capture.len() / 2;
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for fr in capture.chunks_exact(2) {
        left.push(fr[0]);
        right.push(fr[1]);
    }
    // The loop's floor, off the pre-roll — most of it, staying clear of the
    // last stretch where the loop's latency may already carry sweep onset.
    let floor_window = (pre * 4) / 5;
    let floor_db = ess::noise_floor_db(&left[..floor_window.min(left.len())])
        .max(ess::noise_floor_db(&right[..floor_window.min(right.len())]));

    // The distortion probe, on the classic route: the order-N image sits
    // dt_N ahead of the recorded response's own peak.
    let inv = ess::inverse_filter(f1, f2, duration, rate);
    let probe = ess::deconvolve_classic(&left, sweep, &inv);
    let probe_peak = ess::linear_peak(&probe.ir);
    let thd2 = ess::harmonic_level_db(&probe.ir, probe_peak, 2, duration, f1, f2, rate);
    let thd3 = ess::harmonic_level_db(&probe.ir, probe_peak, 3, duration, f1, f2, rate);

    // The IR itself: exact division when a reference exists, classic else.
    let (mut l, mut r, has_reference) = match reference.filter(|r| r.len() == capture.len()) {
        Some(refc) => {
            let mut rl = Vec::with_capacity(frames);
            let mut rr = Vec::with_capacity(frames);
            for fr in refc.chunks_exact(2) {
                rl.push(fr[0]);
                rr.push(fr[1]);
            }
            let mut hl = ess::deconvolve_by_reference(&left, &rl, f1, f2, rate);
            let mut hr = ess::deconvolve_by_reference(&right, &rr, f1, f2, rate);
            // The division is circular: harmonic images and regularization
            // ringing wrap to the END of the padded buffer. The buffer is at
            // least twice the capture, so keeping one capture's worth chops
            // the wrap off whole — Muller's windowing rule.
            hl.truncate(frames);
            hr.truncate(frames);
            (hl, hr, true)
        }
        None => {
            let dr = ess::deconvolve_classic(&right, sweep, &inv);
            (probe.ir.clone(), dr.ir, false)
        }
    };
    // One start for BOTH channels — the earlier foot — so the stereo pair
    // stays sample-locked (the IRU rule: edits apply across all tracks).
    let start = ess::find_onset(&l).min(ess::find_onset(&r));
    ess::trim_leading(&mut l, start);
    ess::trim_leading(&mut r, start);
    ess::remove_dc(&mut l);
    ess::remove_dc(&mut r);
    ess::truncate_at_floor(&mut l, floor_db, rate);
    ess::truncate_at_floor(&mut r, floor_db, rate);
    // Tail lengths equalized by silence, so the pair interleaves cleanly;
    // both ends were just faded to the floor, so the padding is inaudible.
    let len = l.len().max(r.len()).max(1);
    l.resize(len, 0.0);
    r.resize(len, 0.0);
    // One scale for the set: the level difference between the channels IS
    // the stereo image (Voxengo's warning).
    let mut set = [l, r];
    ess::normalize_set(&mut set, ess::NORM_TARGET_DB);
    let [l, r] = set;

    let peak_at = ess::linear_peak(&l).min(ess::linear_peak(&r));
    let seconds = (len as f64 / rate) as f32;
    let usable_s = ((len.saturating_sub(peak_at)) as f64 / rate) as f32;
    let outcome = tangent_ui::ports::IrOutcome {
        envelope_db: ir_envelope_db(&l, &r),
        seconds,
        usable_s,
        noise_floor_db: floor_db,
        thd2_db: thd2,
        thd3_db: thd3,
        has_reference,
    };
    (l, r, outcome)
}

/// The result stage's picture: the pair's level over time in dB, folded to
/// the louder channel, [`IR_ENVELOPE_POINTS`] buckets, floored at −100.
fn ir_envelope_db(l: &[f32], r: &[f32]) -> Vec<f32> {
    let len = l.len().max(r.len());
    if len == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(IR_ENVELOPE_POINTS);
    for b in 0..IR_ENVELOPE_POINTS {
        let from = b * len / IR_ENVELOPE_POINTS;
        let to = ((b + 1) * len / IR_ENVELOPE_POINTS).max(from + 1).min(len);
        let mut peak = 0.0f32;
        for i in from..to {
            peak = peak
                .max(l.get(i).copied().unwrap_or(0.0).abs())
                .max(r.get(i).copied().unwrap_or(0.0).abs());
        }
        out.push(if peak > 0.0 {
            (20.0 * peak.log10()).max(-100.0)
        } else {
            -100.0
        });
    }
    out
}

/// The shelf's scan: the folder itself plus ONE level of subfolders, so a
/// library sorted into `Reverbs/` and `Cabs/` still shows whole. Duration
/// comes off each WAV's header alone — a shelf of fifty files costs fifty
/// header reads and no decoding — and junk is skipped without a word, the
/// preset folder's own rule.
fn scan_ir_rows(root: &std::path::Path) -> Vec<tangent_ui::ports::IrRow> {
    let mut rows: Vec<tangent_ui::ports::IrRow> = Vec::new();
    let wavs = |dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>| {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.filter_map(|e| e.ok()) {
                let p = e.path();
                if p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
                {
                    out.push(p);
                }
            }
        }
    };
    let mut paths = Vec::new();
    wavs(root, &mut paths);
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut subs: Vec<std::path::PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        subs.sort();
        for sub in subs {
            wavs(&sub, &mut paths);
        }
    }
    paths.sort();
    for path in paths {
        let Some(seconds) = wav_seconds(&path) else {
            continue; // Not a WAV after all; a shelf skips junk quietly.
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        rows.push(tangent_ui::ports::IrRow {
            name,
            path: path.to_string_lossy().into_owned(),
            seconds,
        });
    }
    rows
}

/// A WAV's duration off its header alone — chunk walk, no decoding. `None`
/// for anything that is not a WAV, which a shelf scan skips quietly.
fn wav_seconds(path: &std::path::Path) -> Option<f32> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut hdr = [0u8; 12];
    f.read_exact(&mut hdr).ok()?;
    if &hdr[0..4] != b"RIFF" || &hdr[8..12] != b"WAVE" {
        return None;
    }
    let mut rate = 0u32;
    let mut block_align = 0u32;
    let mut data_bytes: Option<u64> = None;
    let mut chunk = [0u8; 8];
    while f.read_exact(&mut chunk).is_ok() {
        let size = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
        match &chunk[0..4] {
            b"fmt " => {
                let mut fmt = [0u8; 16];
                if size < 16 || f.read_exact(&mut fmt).is_err() {
                    return None;
                }
                rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                block_align = u32::from(u16::from_le_bytes([fmt[12], fmt[13]]));
                // Whatever the format appends past the common fields.
                f.seek(SeekFrom::Current((size - 16) as i64)).ok()?;
            }
            b"data" => {
                data_bytes = Some(size);
                break; // Nothing after data changes the duration.
            }
            _ => {
                // An odd chunk carries a RIFF pad byte its size omits.
                f.seek(SeekFrom::Current((size + (size & 1)) as i64)).ok()?;
            }
        }
    }
    let data = data_bytes?;
    if rate == 0 || block_align == 0 {
        return None;
    }
    Some((data / u64::from(block_align)) as f32 / rate as f32)
}

impl DesktopApp {
    /// The pointer during a FILE DRAG, in window-local points — or `None`
    /// when nothing is being dragged and on platforms that are not asked.
    ///
    /// The cursor in SCREEN points, the same unit egui reports the window's
    /// inner rect in.
    #[cfg(target_os = "macos")]
    fn os_cursor_points(_ctx: &egui::Context) -> Option<(f32, f32)> {
        use core_graphics::event::CGEvent;
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        // Core Graphics answers in global display points already — the same
        // logical unit, Retina or not — so there is no scale to apply.
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
        let event = CGEvent::new(source).ok()?;
        let p = event.location();
        Some((p.x as f32, p.y as f32))
    }

    /// Windows answers in physical pixels, so it is divided down.
    #[cfg(target_os = "windows")]
    fn os_cursor_points(ctx: &egui::Context) -> Option<(f32, f32)> {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: `p` is a valid, owned POINT for the duration of the call.
        if unsafe { GetCursorPos(&mut p) } == 0 {
            return None;
        }
        let ppp = ctx.pixels_per_point().max(0.1);
        Some((p.x as f32 / ppp, p.y as f32 / ppp))
    }

    /// Left alone elsewhere: X11 and Wayland want their own query, and the
    /// existing pointer path is what those builds keep until one is written.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn os_cursor_points(_ctx: &egui::Context) -> Option<(f32, f32)> {
        None
    }

    /// **An OS drag produces no pointer events.** winit reports `HoveredFile`
    /// when files enter the window and never a `CursorMoved` for the whole
    /// drag, so `egui`'s pointer is stale or absent for exactly as long as
    /// somebody is holding a file over the app. The first fix tracked the
    /// pointer while the file hovered, which tracked nothing at all: there
    /// was no position to read. So the cursor is asked of the platform.
    ///
    /// Only while files hover. Polling the OS for the mouse every frame of
    /// every session to answer a question nobody asked is not a thing to do
    /// in a paint loop.
    /// How many times the live-resize height table has been rebuilt.
    ///
    /// It is supposed to be a handful in a session; it is checked because
    /// that was an assertion in a comment and the table used to be LEAKED on
    /// every rebuild.
    #[cfg(target_os = "macos")]
    fn curve_rebuilds() -> u64 {
        crate::macwin::REBUILDS.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(not(target_os = "macos"))]
    fn curve_rebuilds() -> u64 {
        0
    }

    /// What the host can see about a live resize — see `macwin`.
    ///
    /// macOS can answer; nothing else can, and a host that cannot must say so
    /// rather than guess, because "a size nobody asked for" only means "a
    /// window manager placed us" where a live resize is actually visible.
    #[cfg(target_os = "macos")]
    fn window_facts(frame: &eframe::Frame, app: &TangentApp) -> crate::macwin::Facts {
        crate::macwin::publish(
            frame,
            &|w| app.aspect_height_at(w),
            app.aspect_width_floor(),
        )
    }

    /// Everywhere else: all-false, which keeps `aspect_snap` armed and the
    /// immediate window-manager latch disarmed.
    #[cfg(not(target_os = "macos"))]
    fn window_facts(_frame: &eframe::Frame, _app: &TangentApp) -> WindowFacts {
        WindowFacts::default()
    }

    /// **Borderless AND maximised means the whole screen**, and anything
    /// else means an ordinary window.
    ///
    /// Maximised means *the work area* on every platform — the screen minus
    /// what the shell reserves — and that is right for a window with a title
    /// bar and wrong for one without. Taking the frame off was the request to
    /// use the display; keeping a strip of dead desktop along two edges is
    /// the opposite of it.
    ///
    /// Fullscreen is excluded deliberately: the OS is already doing this, and
    /// a second opinion about the frame arriving mid-animation is a fight.
    ///
    /// Latched on `covering_screen` so each direction fires exactly once.
    #[cfg(target_os = "macos")]
    fn reconcile_screen_cover(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let (maximised, fullscreen) = ctx.input(|i| {
            let v = i.viewport();
            (v.maximized.unwrap_or(false), v.fullscreen.unwrap_or(false))
        });
        let want = self.app.borderless() && maximised && !fullscreen;
        if want == self.covering_screen {
            return;
        }
        if want {
            // Only latch if the window was actually found and moved, so a
            // failure retries next frame rather than latching a lie.
            self.covering_screen = crate::macwin::cover_screen(frame);
        } else {
            crate::macwin::uncover_screen();
            self.covering_screen = false;
        }
    }

    /// **Put the drawable area back after a title bar came or went.**
    ///
    /// The app says what it wants; only the host can reach a window. The
    /// request arrives on the frame after the decoration change and is
    /// applied for as long as it takes to stick — AppKit does not necessarily
    /// have the new style mask on the very next frame, and asking early is a
    /// no-op that would otherwise be the only attempt.
    #[cfg(target_os = "macos")]
    fn reconcile_decoration_size(&mut self, frame: &eframe::Frame) {
        if let Some(want) = self.app.take_decor_keep_content() {
            self.decor_want_content = Some((f64::from(want.x), f64::from(want.y), 0u8));
        }
        let Some((w, h, tries)) = self.decor_want_content else {
            return;
        };
        // Given up after a handful of frames rather than argued about
        // forever: a manager that keeps overruling this owns the geometry,
        // and the app's job is to stop asking.
        if tries >= 12 || crate::macwin::keep_content_size(frame, w, h) {
            self.decor_want_content = None;
        } else {
            self.decor_want_content = Some((w, h, tries + 1));
        }
    }

    /// Nothing to do off macOS yet: the same fault exists under i3 and under
    /// Windows' snap, and neither mechanism is written.
    #[cfg(not(target_os = "macos"))]
    fn reconcile_decoration_size(&mut self, _frame: &eframe::Frame) {
        let _ = self.app.take_decor_keep_content();
    }

    /// No-op off macOS for now. Windows and Linux have the same rule and a
    /// different mechanism — `ViewportCommand::Fullscreen` with the
    /// decorations already off is the usual answer there — and neither is
    /// wired up, so nothing pretends to be.
    #[cfg(not(target_os = "macos"))]
    fn reconcile_screen_cover(&mut self, _ctx: &egui::Context, _frame: &eframe::Frame) {}

    fn drag_pointer(ctx: &egui::Context) -> Option<egui::Pos2> {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return None;
        }
        let inner = ctx.input(|i| i.viewport().inner_rect)?;
        let (sx, sy) = Self::os_cursor_points(ctx)?;
        Some(egui::Pos2::new(sx - inner.min.x, sy - inner.min.y))
    }

    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        cli_port: Option<String>,
    ) -> Self {
        // **Did the last launch die inside a plugin?**
        //
        // Asked FIRST, before the settings reach the app and long before any
        // bay is reconciled, because the answer decides what the app is
        // allowed to try. A plugin the kernel refuses takes this process with
        // SIGKILL — see `crate::plugin_guard` — and the choice is persisted,
        // so the next launch reloads it and dies in the same instruction. The
        // app could not be reopened at all until the settings file was edited
        // by hand.
        let mut settings = settings;
        let killer = crate::plugin_guard::take_blame();
        if let Some(bad) = killer.as_ref() {
            // Out of every bay it sits in, not just the first: the same
            // plugin on two channels would otherwise take the second launch.
            let want = bad.to_string_lossy();
            let mut pulled = 0usize;
            for slot in settings.strip_inserts.iter_mut() {
                if *slot == want {
                    slot.clear();
                    pulled += 1;
                }
            }
            eprintln!(
                "tangent: the last launch was killed loading {}; \
                 removed it from {pulled} bay(s) so this one can start",
                want
            );
        }
        // Dev switch: run the DESKTOP binary through the PLUGIN's capability
        // set, so the in-canvas menu and dialogs can be looked at, screenshot
        // and debugged in a window — without a DAW, a rebuild, an installer
        // and a plugin rescan between each attempt.
        //
        //   IVORY_INLINE=1 /Applications/Tangent.app/Contents/MacOS/tangent
        //
        // Environment-gated, so a normal launch cannot reach it. It is a
        // faithful test of the path: the same `Caps::PLUGIN` the plugin uses,
        // which also means the window will not resize itself.
        //   IVORY_INLINE=menu also opens the menu on the first frame.
        let caps = if matches!(
            std::env::var("IVORY_INLINE").as_deref(),
            Ok("1") | Ok("menu")
        ) {
            eprintln!("IVORY_INLINE=1: running with the plugin's capabilities");
            Caps::PLUGIN
        } else {
            Caps::DESKTOP
        };
        let mut app = TangentApp::new(&cc.egui_ctx, settings, caps);
        // **How much of the screen this desktop actually leaves a window.**
        // Asked once, here, because it cannot be asked from `tangent-ui` at
        // all: the answer is behind Win32 and AppKit and that crate is linked
        // into a plugin. Without it the layout is sized against the RAW
        // display and a laptop's taskbar eats the bottom band.
        if let Some(f) = crate::display::usable_height_fraction() {
            app.set_display_usable_fraction(f);
        }
        // Said in the window as well as on stderr, which nobody sees. The
        // bay is empty and the owner has to know WHY, or the next thing they
        // do is load it again.
        if let Some(bad) = killer {
            let name = bad
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bad.to_string_lossy().into_owned());
            app.set_startup_notice(format!(
                "{name} crashed Tangent while loading, so it has been taken \
                 out of its slot. Its settings are kept; load it again to \
                 retry."
            ));
        }
        let mut device = DeviceMidi::new(cc.egui_ctx.clone());
        // Grabbed BEFORE the connection is made and before the session exists,
        // because everything that stamps a time has to share one epoch. Two
        // `Timebase::new()` calls would silently place the MIDI and the audio
        // in two different worlds, and the symptom would be a constant offset
        // in every take that no test covers.
        #[cfg(feature = "recorder")]
        let (tap, timebase) = (device.tap(), device.timebase());
        // `-p NAME` beats the priority chain, and a bad name is not fatal: the
        // app opens with no MIDI, which is the same outcome as no device.
        // **NOT opened here.** Opening a MIDI client is the first CoreMIDI
        // call the process makes and it pays for the whole subsystem coming
        // up — measured at 695 ms of the 790 ms before the first frame, on a
        // machine where the window and the GPU together took 100. The app
        // looked like it hung, and the splash could not appear until it was
        // over, because the splash is painted BY the app: it was proof the
        // load had finished rather than cover for it happening.
        //
        // It is remembered and run on the frame after the splash is up.
        let pending_midi = cli_port;
        let midi_lane = device.lane();
        app.set_ports(Some(Box::new(device)));

        #[cfg(feature = "recorder")]
        let recorder = {
            let (inputs, audio) = crate::devices::AudioInputs::new();
            let (cams, camera, camera_denied) = crate::devices::Cameras::new();
            // Seed from what the settings file remembers, or the reconciler —
            // which only ever acts on a difference — would never open the
            // chosen device and the app would look like it had forgotten.
            crate::devices::restore(
                &audio,
                app.chosen_audio_uid(),
                app.audio_explicitly_off(),
                app.exposed_input_channels(),
                app.extra_inputs(),
            );
            // No `explicitly_off` for the camera: absent already means no
            // camera there, because opening one turns on a light and a camera
            // nobody asked for must never be opened.
            crate::devices::restore(&camera, app.chosen_camera_uid(), false, &[], &[]);
            app.set_capture_devices(Some(Box::new(inputs)));
            app.set_cameras(Some(Box::new(cams)));
            app.set_audio_setup(Some(Box::new(crate::devices::Setup::new(&audio))));
            // **The saved system, before anything opens.** Every device lookup
            // in `tangent_record::audio` goes through it, including the one two
            // lines above this that restored the microphone. Left to the
            // per-frame edge in `frame()` it would still be applied, one frame
            // late — and one frame late means the engine starts on the platform
            // default and is then torn down and restarted, which is a five-
            // second instrument reload at every launch.
            tangent_record::audio::set_system(app.audio_system().as_deref());
            // **Takes, Projects and Exports, made real.** Once, here, in the
            // host — see `make_media_folders`. Doing it at launch rather than
            // at first write is what makes the three SHOW buttons on the cog
            // card work on a machine Tangent has never recorded on.
            app.make_media_folders();
            // Every installed VST3, by path and file name. This is a DIRECTORY
            // LISTING, not a scan: nothing is opened, so it costs milliseconds
            // even with 112 of them, and no plugin gets the chance to crash the
            // process before the window has appeared. A bundle is opened only
            // when the user picks it.
            let extra = app.plugin_folders();
            app.set_plugin_list(
                crate::plugin_catalog(&extra),
            );
            Recorder {
                transport: crate::transport::Transport::new(),
                pending_monitor: None,
                input_names: std::array::from_fn(|_| String::new()),
                take_note: None,
                arr_rec: None,
                bounce: None,
                offline_bounce: None,
                arr_rec_wait: false,
                pending_delivery: None,
                session: crate::record::Session::new(tap, timebase),
                audio,
                camera,
                camera_denied,
                engine: None,
                engine_error: None,
                engine_watch: crate::instrument::StallWatch::new(),
                reprobe: None,
                reastream: None,
                pads_retry: false,
                inserts_loaded: vec![None; tangent_ui::recorder::INSERT_SPACE],
                camera_opening: false,
                camera_silent_since: None,
                preview: None,
                preview_px: egui::Vec2::ZERO,
                band_was_open: false,
                disk_checked_at: None,
                disk_bytes: None,
                dev_editor_at: None,
                video: None,
                video_tried: false,
                camera_rgba: None,
                engine_retry: None,
                audio_path_deferred: false,
                armed_downbeat: None,
                // Seeded from the settings, not left at None: these are the
                // values the streams are about to be opened with, and a
                // tracker that starts out disagreeing with reality reports a
                // change on frame one that nobody made.
                buffer_open: app.buffer_frames(),
                rate_open: app.sample_rate(),
                system_open: app.audio_system(),
                default_rate: None,
                xruns_seen: 0,
                graph_dropouts: tangent_record::audio::GraphDropouts::start(),
                graph_floor: tangent_record::audio::graph_clock().map(|g| g.floor()),
                seen_take: None,
                dev_editor_done: false,
            }
        };

        let mut me = Self {
            pending_midi: Some(pending_midi),
            painted_once: false,
            last_title: crate::window_title("", false),
            probe_frames: 0,
            pace_since: None,
            pace_frames: 0,
            pace_dt: Vec::new(),
            reader_seen: false,
            #[cfg(feature = "recorder")]
            pushed_maps: None,
            pushed_auto: None,
            midi_lane,
            app,
            covering_screen: false,
            decor_want_content: None,
            #[cfg(feature = "recorder")]
            recorder,
            splash: Some(Splash {
                since: std::time::Instant::now(),
                done_at: None,
            }),
            #[cfg(feature = "recorder")]
            cartridge: None,
            #[cfg(feature = "recorder")]
            cartridges: std::collections::HashMap::new(),
            #[cfg(feature = "recorder")]
            editing: None,
            #[cfg(feature = "recorder")]
            editing_strip: 0,
            #[cfg(feature = "recorder")]
            refullscreen: false,
            #[cfg(feature = "recorder")]
            panel_armed: false,
            #[cfg(feature = "recorder")]
            save_armed: false,
            deferred_file: None,
            #[cfg(feature = "recorder")]
            deferred_dir: None,
            #[cfg(feature = "recorder")]
            deferred_save: None,
            #[cfg(feature = "recorder")]
            reported_take: None,
            #[cfg(feature = "recorder")]
            browse_extensions: Vec::new(),
            #[cfg(feature = "recorder")]
            track_cache: std::collections::HashMap::new(),
            #[cfg(feature = "recorder")]
            stretch_cache: std::collections::HashMap::new(),
            #[cfg(feature = "recorder")]
            pad_cache: std::collections::HashMap::new(),
            scope_buf: Vec::new(),
            tap_buf: Vec::new(),
            insert_tap_at: None,
            ir: IrRoad::default(),
        };
        // What the effects ship as, so the panel can draw a slider for a
        // parameter nobody has moved. See `ports::EffectDefaults`: the DSP owns
        // these numbers and the UI cannot reach them.
        #[cfg(feature = "recorder")]
        me.app.set_effect_defaults(effect_defaults());
        // The cartridge from last time, if it is still where it was. **Silently
        // when it is not**: cartridges live in sample folders that get
        // reorganised, and an error dialog at launch about a file somebody
        // loaded weeks ago is a worse outcome than quietly playing the built-in
        // patch, which is what a fresh install plays anyway.
        #[cfg(feature = "recorder")]
        me.load_cartridge_at_launch();
        #[cfg(feature = "recorder")]
        me.open_project_at_launch();
        // The one-launch sidecar shuffle, BEFORE any engine exists to load a
        // bay: the settings just crossed the v13 desk change and the
        // insert-state files are named by indices that moved.
        // **All FOUR shuffles under one gate.** The attribute was on the
        // first `if` alone, and an attribute on an `if` covers that `if` —
        // so the three below it were compiled into a Minimal build that has
        // none of the functions they call. The braces make the scope
        // explicit rather than leaving it to the next reader to count.
        #[cfg(feature = "recorder")]
        {
            if me.app.take_desk_state_shift() {
                shift_insert_states_for_v13();
            }
            if me.app.take_desk_state_shift_v14() {
                shift_insert_states_for_v14();
            }
            if me.app.take_pad_state_shift_v18() {
                shift_insert_states_for_v18();
            }
            if me.app.take_desk_state_shift_v19() {
                shift_insert_states_for_v19();
            }
            if me.app.take_desk_state_shift_v20() {
                shift_insert_states_for_v20();
            }
        }
        me
    }
}

/// v12 → v13: the BACKING strip retired, and every insert-state sidecar named
/// by a desk index above it moved down one — the click's 9 → 8, the bus's
/// 10 → 9, the master's 11 → 10 — while the track's own are deleted. Numbers
/// FROZEN as literals, the historical-migration rule: reading today's
/// constants would re-break this the next time the desk changes shape.
///
/// Ascending on purpose: 8 is deleted before 9 moves into it, and each rename
/// vacates the slot the next one wants. Failures are ignored — the sidecars
/// carry an owner check on read, so a file that could not move is ignored,
/// not misapplied; the cost is one plugin's saved knobs, once.
#[cfg(feature = "recorder")]
fn shift_insert_states_for_v13() {
    for bay in 0..3 {
        let _ = std::fs::remove_file(insert_state_path(8, bay));
        for old in 9..=11 {
            let from = insert_state_path(old, bay);
            if from.exists() {
                let _ = std::fs::rename(&from, insert_state_path(old - 1, bay));
            }
        }
    }
}

/// v17 -> v18: the pads grew to the whole keyboard; the old sixteen were
/// keys 36..51, so their seats moved from 12..27 to 48..63. Highest first,
/// same rule as every shuffle before it; numbers frozen as literals.
#[cfg(feature = "recorder")]
fn shift_insert_states_for_v18() {
    for bay in 0..3 {
        for pad in (0..16usize).rev() {
            let from = insert_state_path(12 + pad, bay);
            if from.exists() {
                let _ = std::fs::rename(&from, insert_state_path(12 + 36 + pad, bay));
            }
        }
    }
}

/// v18 -> v19: the channel count grew 8 -> 16, so every seat at or above the
/// old channel block stepped up by 8 — the drum bus 8 -> 16, the click 9 ->
/// 17, the fx return 10 -> 18, the master 11 -> 19, and the 128 pad seats
/// 12.. -> 20... Highest first, or the move eats itself; numbers frozen as
/// literals per the historical-migration rule. The upper bound covers the
/// old master (11) and all 128 pads (12..139).
#[cfg(feature = "recorder")]
/// 16 -> today's channels: every seat at or above the old channel block —
/// drums, click, fx, master, master-extra and all the pad blocks — steps
/// up by exactly the growth. Uniform because the seat map appends its
/// classes in order (channels, buses, pads), so one +grow over the old
/// range is provably right for every old seat >= 16. Highest first, or
/// the move eats itself. The old map's last seat was 2068 (frozen: the
/// sixteen-channel era's own arithmetic).
fn shift_insert_states_for_v20() {
    let grow = tangent_ui::recorder::CHANNELS - 16;
    for bay in 0..3 {
        for old in (16..2069).rev() {
            let from = insert_state_path(old, bay);
            if from.exists() {
                let _ = std::fs::rename(&from, insert_state_path(old + grow, bay));
            }
        }
    }
}

fn shift_insert_states_for_v19() {
    for bay in 0..3 {
        for old in (8..(12 + 128)).rev() {
            let from = insert_state_path(old, bay);
            if from.exists() {
                let _ = std::fs::rename(&from, insert_state_path(old + 8, bay));
            }
        }
    }
}

/// v13 -> v14: the drum bus took seat 8, so the click's, the bus's and the
/// master's sidecars walk UP one — highest first, or the move eats itself.
fn shift_insert_states_for_v14() {
    for bay in 0..3 {
        for old in (8..=10).rev() {
            let from = insert_state_path(old, bay);
            if from.exists() {
                let _ = std::fs::rename(&from, insert_state_path(old + 1, bay));
            }
        }
    }
}

impl eframe::App for DesktopApp {
    /// eframe hands over a Context; everything below wants a Ui, and
    /// `TangentApp::frame` is the bridge all three hosts share.
    ///
    /// The recorder brackets it: state in before, requests out after. Nothing
    /// that opens a device, raises a native panel or creates a directory
    /// happens between those two lines.
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        #[cfg(feature = "recorder")]
        self.fill_recorder_state(ctx);
        // Before the frame, not after: the app decides whether to raise the
        // Welcome card while it paints, and a flag set afterwards would be one
        // frame stale — which is exactly one frame of card over wordmark.
        // **After the first frame, never during it.** The splash is already
        // on screen by the time this runs, so the ~700 ms the MIDI subsystem
        // takes to come up is spent behind it rather than in front of a blank
        // window. `painted_once` is set at the bottom of this function.
        if self.painted_once {
            if let Some(port) = self.pending_midi.take() {
                match port {
                    Some(name) => {
                        if let Err(e) = self.app.connect_midi_named(&name) {
                            eprintln!("could not open MIDI port {name:?}: {e}");
                        }
                    }
                    None => self.app.midi_auto_connect(),
                }
            }
        }
        self.app.set_splash_up(self.splash.is_some());
        // **Where the cursor is during a FILE DRAG.** Asked of the OS,
        // because the window toolkit will not say — see `drag_pointer`.
        self.app.set_drag_pointer(Self::drag_pointer(ctx));
        // **What the OS can see about a gesture on our window.** Asked here
        // and not in the GUI crate for the same reason as `drag_pointer`
        // above: `tangent-ui` may not reach the host at all.
        //
        // Computed into an owned value first — the closure borrows `self.app`
        // immutably and the setters need it mutably.
        let facts = Self::window_facts(frame, &self.app);
        self.app.set_host_gesture(facts.gesture);
        self.app.set_os_aspect_lock(facts.os_aspect_lock);
        // **The title bar knows the session.** Composed in one place
        // (`crate::window_title`) and sent only on change; project_dirty is
        // a comparison of two serialisations, cheap at frame rate but not
        // worth a viewport command when nothing moved.
        {
            let want =
                crate::window_title(&self.app.project_name(), self.app.project_dirty());
            if want != self.last_title {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(want.clone()));
                self.last_title = want;
            }
        }
        self.reconcile_screen_cover(ctx, frame);
        self.reconcile_decoration_size(frame);
        // **The window is closing — is that all right?**
        //
        // With the autosave gone, a close is the last chance to keep an
        // afternoon's work, so it is intercepted and handed to the same
        // prompt New and Open use. `CancelClose` holds the window open while
        // the question is on screen; the answer either lets it go or does
        // not. Asked BEFORE the frame is drawn, so the dialog appears in the
        // same frame the close was requested in.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.app.intercept_quit() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        // The prompt said go: close for real. A second `close_requested`
        // would be intercepted again, so the app's own flag is what carries
        // the answer across.
        if self.app.take_quit_now() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // **The first frame is the SPLASH and nothing else.**
        //
        // The app's first `frame` costs about a second — the font atlas is
        // built and uploaded the first time anything is laid out — and it
        // runs inside `update`, so the window cannot appear until it is done.
        // Skipping it once puts the splash on screen roughly half a second
        // sooner; the app draws from the next pass, behind it.
        // **Has a screen reader turned up?** Asked HERE and not in the GUI
        // crate, because the answer only exists behind `egui/accesskit` and
        // `tangent-ui` may not name a feature — see `scripts/check-firewall.sh`
        // and the note in `tangent/Cargo.toml`. The answer crosses as a plain
        // bool, like every other fact this crate pushes into the app.
        //
        // **Polled until the first yes, then never again.** On Linux the only
        // way to ask is to WRITE a node and see whether egui had anywhere to
        // put it — so every probe while a reader is attached puts an extra,
        // nameless node in the tree that reader is reading. One is a shrug;
        // sixty a second is a bug. On macOS and Windows the poll is a
        // side-effect-free property read, cheap enough to repeat, and
        // repeating it is what catches VoiceOver being switched on
        // mid-session. `set_screen_reader` is sticky in the same direction,
        // so once it has said yes there is nothing to keep watching for —
        // and going BACK is the menu row's job, on purpose.
        if !self.reader_seen && screen_reader_attached(ctx) {
            self.reader_seen = true;
            self.app.set_screen_reader(true);
        }
        if self.painted_once {
            self.app.frame(ctx);
        }
        #[cfg(feature = "recorder")]
        self.after_frame(ctx, frame);
        self.paint_splash(ctx);
        self.painted_once = true;
        // **The geometry probe.** `IVORY_GEOM_PROBE=1` prints, once the
        // window has settled, everything the app believes about where it is
        // — then closes itself through the ordinary quit path, because an
        // audio client that is killed rather than quit wedges CoreAudio for
        // the whole machine. Diagnostic; costs one branch otherwise.
        // Read ONCE, not sixty times a second: `var_os` walks the
        // environment and allocates, and a diagnostic that costs something
        // when it is off is a diagnostic that gets deleted.
        static PROBE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *PROBE.get_or_init(|| std::env::var_os("IVORY_GEOM_PROBE").is_some()) {
            self.probe_frames += 1;
            if self.probe_frames == 120 {
                let vp = ctx.input(|i| {
                    (
                        i.viewport().inner_rect,
                        i.viewport().outer_rect,
                        i.viewport().monitor_size,
                        i.screen_rect(),
                    )
                });
                eprintln!("GEOM egui inner={:?} outer={:?} mon={:?} screen={:?}", vp.0, vp.1, vp.2, vp.3);
                eprintln!("GEOM app  {}", self.app.debug_geometry());
                eprintln!("GEOM ppp  {}", ctx.pixels_per_point());
                eprintln!(
                    "GEOM hook os_aspect_lock={} wm_managed={} sends={}",
                    self.app.debug_os_aspect_lock(),
                    self.app.debug_wm_managed(),
                    self.app.debug_inner_size_sends(),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
        // **The pacing probe.** `IVORY_PACE_PROBE=1` prints once a second what
        // the window is actually doing, because "the drag feels choppy" and
        // "the drag IS choppy" are different claims and only one of them can
        // be checked. Same OnceLock discipline as the geometry probe above:
        // reading the environment every frame is itself a cost.
        //
        // `fps` is frames actually drawn. `p99` is the slowest frame in the
        // second, which is what a stutter is — a mean can hide one 40 ms
        // frame in 240 good ones. `sends` counts window-size commands: on a
        // tiling manager that number staying flat is the whole fix.
        static PACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *PACE.get_or_init(|| std::env::var_os("IVORY_PACE_PROBE").is_some()) {
            let now = std::time::Instant::now();
            let since = *self.pace_since.get_or_insert(now);
            self.pace_frames += 1;
            let dt = ctx.input(|i| i.unstable_dt);
            if dt.is_finite() && dt > 0.0 {
                self.pace_dt.push(dt);
            }
            if now.duration_since(since).as_secs_f32() >= 1.0 {
                let secs = now.duration_since(since).as_secs_f32();
                self.pace_dt.sort_by(f32::total_cmp);
                let p99 = self
                    .pace_dt
                    .get(self.pace_dt.len().saturating_sub(1).min(
                        (self.pace_dt.len() as f32 * 0.99) as usize,
                    ))
                    .copied()
                    .unwrap_or(0.0);
                eprintln!(
                    "PACE fps={:.0} p99={:.2}ms sends={} wm={} oslock={} curve_rebuilds={}",
                    self.pace_frames as f32 / secs,
                    p99 * 1000.0,
                    self.app.debug_inner_size_sends(),
                    self.app.debug_wm_managed(),
                    self.app.debug_os_aspect_lock(),
                    Self::curve_rebuilds(),
                );
                self.pace_since = Some(now);
                self.pace_frames = 0;
                self.pace_dt.clear();
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        TangentApp::CLEAR_COLOR
    }

    /// A take still running when the window is closed is FINISHED, not
    /// abandoned.
    ///
    /// Without this the writer thread is torn down with the process and the
    /// `.wav` keeps the placeholder sizes in its header — a file most players
    /// treat as zero-length. Somebody who left the recorder running for a
    /// practice session and then quit would lose the whole thing.
    #[cfg(feature = "recorder")]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.recorder.session.stop();
        self.save_plugin_states();
        // **The document is NOT written here.**
        //
        // It used to be, unconditionally, which meant quitting always kept
        // the session whether or not that was wanted — an autosave at the
        // last possible moment, and the one that made "Save changes before
        // quitting?" a question with no consequences. The prompt owns that
        // decision now; by the time this runs it has already been answered.
        //
        // The plugin STATES still go, because they are not the document:
        // they are the shelf a loaded plugin keeps its knobs on, and losing
        // them would mean a project that opens with its instruments reset
        // even when the answer was Save.
        self.copy_states_to_project();
    }
}

/// Show `path` in the platform's file manager.
///
/// Best effort and deliberately silent on failure. There is no useful thing to
/// tell somebody whose file manager did not open — the take is written either
/// way, the folder is named on screen, and an error banner over the Recorder
/// would be reporting a problem with the CONVENIENCE as though it were a
/// problem with the recording.
///
/// `spawn` and not `status`: waiting on Finder or Explorer would block the UI
/// thread for as long as the window takes to appear.
fn reveal(path: &std::path::Path) {
    // A folder that does not exist yet is not an error either. The destination
    // is created when the first take is written, so pressing SHOW before ever
    // recording would otherwise open a window onto nothing.
    if !path.exists() {
        return;
    }
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(path);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    let _ = cmd.spawn();
}

// ───────────────────────────────────────────────────────────────────────────
// The take's video
// ───────────────────────────────────────────────────────────────────────────

/// The compositor and the encoder for a take that is being filmed.
///
/// Both live on the UI thread, and that is the whole thread design: the
/// compositor must be here because it paints the app, and the encoder is here
/// because moving IT here costs 384 kB a second of audio crossing a channel,
/// where moving the compositor to the writer thread would cost 250 MB a second
/// of composited frames going the other way.
/// The device the WINDOW is drawing with, when there is one to borrow.
///
/// eframe only exposes this when it was built with its `wgpu` feature, which is
/// macOS here: everywhere else the window draws with glow and there is nothing
/// to borrow. `None` is not a refusal any more, it is the compositor opening an
/// adapter of its own, so this is a fast path rather than a gate.
#[cfg(all(feature = "recorder", target_os = "macos"))]
fn window_device(frame: &eframe::Frame) -> Option<&egui_wgpu::RenderState> {
    frame.wgpu_render_state()
}

#[cfg(all(feature = "recorder", not(target_os = "macos")))]
fn window_device(_frame: &eframe::Frame) -> Option<&egui_wgpu::RenderState> {
    None
}

#[cfg(feature = "recorder")]
struct TakeVideo {
    compositor: crate::composite::Compositor,
    encoder: tangent_record::encode::Encoder,
    /// The next frame index to produce. The video's clock is the TAKE's clock:
    /// frame `n` is presented at `n / fps` seconds after the take started,
    /// whatever the camera has managed to deliver by then.
    next: u64,
    fps: u32,
    layout: tangent_ui::recorder::Layout,
    shows: tangent_ui::recorder::DisplayShows,
    camera: bool,
    display: bool,
    path: std::path::PathBuf,
    /// Frames the compositor or the encoder refused, so the summary can say so
    /// rather than the user counting them in the finished file.
    failed: u64,
    /// Ticks filled by repeating the previous frame because the machine could
    /// not composite in real time. The video stays on the wall clock; this is
    /// the honest count of how much of it is a freeze-frame.
    padded: u64,
    /// Ticks the take's own clock has passed, whether or not one was produced.
    ///
    /// **Not `next`.** `next` is the timeline position and it can fall behind:
    /// the burst that fills a gap is capped at a second of video, so on a
    /// machine that cannot keep up the deficit carries — and reporting `next`
    /// as "frames expected" quietly shortened the expectation to match what
    /// was delivered. The take then said it had lost nothing while losing
    /// half. This is the honest denominator.
    due: u64,
    /// Compose only every Nth tick, and pad the rest. 1 is every frame.
    ///
    /// Raised when a pump overruns its budget, so a machine that cannot keep
    /// up gives back the UI's time instead of spending it — see the pump. The
    /// video's clock is unaffected; its motion is coarser and `padded` says by
    /// how much.
    stride: u32,
    /// Whether the file carries an audio track, for the manifest's report.
    has_audio: bool,
    /// `Session::camera_frames_delivered` at take start. The camera opens with
    /// the band, not the take, so the per-take figure is a difference.
    cam_frames_at_start: u64,
    /// The same baselines for the three loss counters, so the manifest reports
    /// what happened during THIS take rather than since the camera opened.
    cam_superseded_at_start: u64,
    cam_unreadable_at_start: u64,
    cam_skipped_at_start: u64,
    cam_dropped_late_at_start: u64,
}

#[cfg(feature = "recorder")]
impl DesktopApp {
    /// Start filming, if this take is meant to be filmed.
    ///
    /// Every refusal is a message rather than a silent skip: a take that was
    /// supposed to produce an `.mp4` and did not is exactly the failure that
    /// wastes a performance.
    fn begin_video(&mut self, frame: &eframe::Frame) {
        let spec = self.app.export_spec();
        if !spec.video.wants_video() || self.recorder.video.is_some() || self.recorder.video_tried {
            return;
        }
        // The folder FIRST, and the flag only after it. Setting the flag before
        // this check is what turned "the take is not ready yet" into "this take
        // will never have video" — a one-frame condition becoming permanent.
        let Some(path) = self.recorder.session.take_video() else {
            return;
        };
        self.recorder.video_tried = true;
        // The camera's own size, for `MatchCamera` and for nothing else.
        let cam = self
            .recorder
            .session
            .camera_format()
            .map(|f| (f.width, f.height));
        let (w, h) = spec.resolution.pixels().or(cam).unwrap_or((1920, 1080));
        let want_camera = spec.composite.camera && self.recorder.session.camera_running();
        let want_display = spec.composite.display && spec.composite.shows.any();
        if !spec.produces_video(self.recorder.session.camera_running()) {
            self.recorder.engine_error =
                Some("the video has neither the camera nor the display in it".to_owned());
            return;
        }
        let video = tangent_record::encode::VideoSpec {
            width: w,
            height: h,
            fps: spec.fps.max(1),
        };
        // The audio track exists only when the writer is actually sending
        // samples — a take with the `.wav` unticked has no audio to mux, and a
        // silent video is a legitimate request in its own right.
        let audio = spec
            .composite
            .audio
            .then(|| self.recorder.session.video_audio_spec())
            .flatten()
            .map(|(rate, channels)| tangent_record::encode::AudioSpec {
                sample_rate: rate,
                channels,
            });
        let compositor = match crate::composite::Compositor::new(
            window_device(frame),
            video.width,
            video.height,
        ) {
            Ok(c) => c,
            Err(e) => {
                self.recorder.engine_error = Some(format!("no video this take: {e}"));
                return;
            }
        };
        // The offscreen context needs the app's fonts or every chord name in
        // the video renders in egui's default face.
        self.app.install_fonts(compositor.context());
        let has_audio = audio.is_some();
        let encoder = match tangent_record::encode::Encoder::create(&path, video, audio) {
            Ok(e) => e,
            Err(e) => {
                self.recorder.engine_error = Some(format!("no video this take: {e}"));
                return;
            }
        };
        self.recorder.video = Some(TakeVideo {
            compositor,
            encoder,
            next: 0,
            fps: video.fps,
            layout: spec.composite.layout,
            shows: spec.composite.shows,
            camera: want_camera,
            display: want_display,
            path,
            failed: 0,
            padded: 0,
            due: 0,
            stride: 1,
            has_audio,
            cam_frames_at_start: self.recorder.session.camera_frames_delivered(),
            cam_superseded_at_start: self.recorder.session.camera_frames_superseded(),
            cam_unreadable_at_start: self.recorder.session.camera_frames_unreadable(),
            cam_skipped_at_start: self.recorder.session.camera_frames_skipped(),
            cam_dropped_late_at_start: self.recorder.session.camera_frames_dropped_late(),
        });
    }

    /// Produce every video frame that is due, and drain the audio behind it.
    ///
    /// **Ticked from the take's own elapsed time**, not from the window's frame
    /// rate. The window may be drawing at 60, or at 8 while a plugin editor is
    /// dragging; neither may change how many frames a second the video has.
    fn pump_video(&mut self) {
        // Taken OUT for the duration, not borrowed. `&mut self.recorder.video`
        // and `&self.recorder.session` are both borrows of `self.recorder`, and
        // this function needs the encoder mutably while reading the session and
        // the app.
        let Some(mut v) = self.recorder.video.take() else {
            return;
        };
        // The audio first, so a long video stall cannot leave the encoder's two
        // inputs far apart in time — AVAssetWriter buffers the gap in memory.
        if let Some(rx) = self.recorder.session.video_audio() {
            while let Ok(chunk) = rx.try_recv() {
                if let Err(e) = v.encoder.push_audio(&chunk.samples, chunk.first_frame) {
                    self.recorder.engine_error = Some(e);
                    break;
                }
            }
        }
        let elapsed = self.recorder.session.elapsed();
        // The tick whose presentation time has arrived. Everything below is
        // about keeping `v.next` caught up to this number by the END of every
        // pump, because the timestamps come from `v.next` — a pump that leaves
        // ticks unproduced does not make a shorter video, it makes one whose
        // clock runs slow: each late frame carries late content at an early
        // timestamp, and a machine compositing at half speed used to squeeze
        // a whole performance into half its real duration.
        let due = (elapsed * f64::from(v.fps)) as u64;
        v.due = v.due.max(due);

        // Fresh frames first — capped by COUNT for the window-is-slow case (a
        // plugin drag at 8 fps still needs ~4 video frames per pump, and they
        // are cheap when the machine is fast), and by WALL TIME for the
        // machine-is-slow case: three 80 ms composites in one pump is the
        // classic spiral of death, a UI at 4 fps compositing ever further
        // behind. The first composite always runs; the budget only stops a
        // pump from doubling down on a machine that is already saturated.
        const MAX_PER_FRAME: u32 = 3;
        const BUDGET: std::time::Duration = std::time::Duration::from_millis(20);
        // **Input has priority: composite fewer frames rather than steal the
        // UI's time.** A machine that cannot composite at the asked-for rate
        // used to keep trying and pad the difference, which spends the whole
        // budget every pump and leaves the window at four frames a second with
        // notes queued behind it. Above `stride`, only every Nth tick is
        // composed and the rest are padded — the video's clock is unchanged,
        // its motion is coarser, and the window comes back.
        //
        // Doubling and halving rather than a fine control, because it is
        // reacting to a measurement made three times a pump: a fine one would
        // hunt. It stops at 4, which is 15 fps of real frames out of 60 and
        // already far past where a video is worth degrading further.
        //
        // A 15 fps video of a good take beats a 30 fps video of an unplayable
        // one — the owner's own words, and the reason this is not a setting.
        let pump_started = std::time::Instant::now();
        let mut made = 0;
        while v.next < due && made < MAX_PER_FRAME {
            // Behind, and composing only every `stride`th tick: pad this one
            // and move on without paying for a frame nobody asked for.
            if v.stride > 1 && !v.next.is_multiple_of(u64::from(v.stride)) {
                if let Some(last) = v.compositor.last_frame() {
                    let pts = (v.next as i64 * 1_000_000_000) / i64::from(v.fps);
                    if v.encoder.push(last, pts).is_err() {
                        v.failed += 1;
                    }
                    v.padded += 1;
                }
                v.next += 1;
                continue;
            }
            let pts = (v.next as i64 * 1_000_000_000) / i64::from(v.fps);
            // Only a picture that is still true — see `camera_rgba`. The
            // sweep above drops a stale one, and this is the second half of
            // the same rule for the window frames in between.
            let frame = self
                .recorder
                .camera_rgba
                .as_ref()
                .filter(|(.., at)| at.elapsed() <= CAMERA_FRAME_STALE_AFTER)
                .map(|(px, w, h, _)| (px.as_slice(), *w, *h));
            match v.compositor.frame(
                &self.app, v.layout, v.shows, v.camera, v.display, frame, pts,
            ) {
                // A frame from one tick ago is now readable, carrying its own
                // pts. The readback is pipelined so the UI thread never waits
                // on the rasteriser — see `composite`'s module docs.
                Ok(Some(ready)) => {
                    let pushed = v
                        .compositor
                        .last_frame()
                        .is_some_and(|bgra| v.encoder.push(bgra, ready).is_ok());
                    if !pushed {
                        v.failed += 1;
                    }
                }
                // The first tick of a take: submitted, nothing to encode yet.
                Ok(None) => {}
                Err(_) => v.failed += 1,
            }
            v.next += 1;
            made += 1;
            if pump_started.elapsed() > BUDGET {
                break;
            }
        }
        // What that pump cost, and what to do about it next time.
        let spent = pump_started.elapsed();
        if spent > BUDGET {
            v.stride = (v.stride * 2).min(4);
        } else if spent * 3 < BUDGET && v.stride > 1 {
            // Well under, for a while: try more frames again. A third of the
            // budget rather than half, so a machine sitting exactly on the
            // boundary settles instead of oscillating.
            v.stride /= 2;
        }

        // Still behind: the machine cannot composite this fast, so hold the
        // timeline instead of falling off it — the missed ticks repeat the
        // frame just made. A repeated frame is a visible stutter and an honest
        // one (`v.padded` reports it); a slow clock is invisible and wrong.
        // The burst is capped at a second of video: after a long UI stall the
        // encoder's queue is the wrong place to shove a gigabyte of
        // duplicates, and the deficit carries to the next pump.
        if v.next < due {
            let target = due.min(v.next + u64::from(v.fps));
            if let Some(last) = v.compositor.last_frame() {
                while v.next < target {
                    let pts = (v.next as i64 * 1_000_000_000) / i64::from(v.fps);
                    if v.encoder.push(last, pts).is_err() {
                        v.failed += 1;
                    }
                    v.next += 1;
                    v.padded += 1;
                }
            }
        }
        self.recorder.video = Some(v);
    }

    /// Close the video file. Must happen, or the container has no index.
    fn end_video(&mut self) {
        let Some(mut v) = self.recorder.video.take() else {
            return;
        };
        // **The frame still in the pipeline.** The readback is one deep, so
        // there is always exactly one composited frame that has been submitted
        // and not yet read; without this the last frame of every take is lost.
        // See `composite`'s module docs.
        if let Ok(Some(pts)) = v.compositor.flush() {
            let pushed = v
                .compositor
                .last_frame()
                .is_some_and(|bgra| v.encoder.push(bgra, pts).is_ok());
            if !pushed {
                v.failed += 1;
            }
        }
        // One last drain, for the samples the writer flushed at Stop. Without
        // it the video's audio is a poll interval shorter than the `.wav`.
        if let Some(rx) = self.recorder.session.video_audio() {
            while let Ok(chunk) = rx.try_recv() {
                let _ = v.encoder.push_audio(&chunk.samples, chunk.first_frame);
            }
        }
        let dropped = v.encoder.dropped_not_ready() + v.failed;
        let path = v.path.clone();
        match v.encoder.finish() {
            Ok(()) => {
                // **The camera's own shortfall comes first, because it has a
                // different cause and a different fix.**
                //
                // A UVC webcam integrates for as long as the light needs and
                // cannot produce frames faster than that, so in a dim room it
                // silently halves its rate — measured on the owner's machine,
                // `fps = min(30, 1/exposure)` exactly: 66.6 ms of exposure is
                // 15 fps against a negotiated 29.97. Nothing said so, and the
                // frames that never arrived were indistinguishable from frames
                // the take lost. Telling somebody to lower the video size when
                // the room is dark is advice that cannot work.
                //
                // **Only when it actually cost THIS take.** The camera
                // negotiates its own rate and the take composites at its own,
                // and those are different numbers: a camera at 15 feeding a
                // 15 fps take has not cost anybody anything, and saying so
                // after every take would be crying wolf. It becomes true the
                // moment the take asks for more than the room can give.
                let starved = self
                    .recorder
                    .session
                    .camera_rate_limited()
                    .filter(|r| r.actual_fps < f64::from(v.fps) * 0.9);
                if let Some(r) = starved {
                    self.recorder.take_note = Some(format!(
                        "camera gave {:.0} fps of the {} this take wanted - it \
                         needs more light",
                        r.actual_fps, v.fps
                    ));
                } else if dropped > 0 || v.padded > 0 {
                    // Both numbers, because they are different failures: a
                    // dropped frame is a hole, a padded one is a freeze —
                    // and both mean the same advice about this machine.
                    let mut what = Vec::new();
                    if dropped > 0 {
                        what.push(format!("{dropped} video frames lost"));
                    }
                    if v.padded > 0 {
                        what.push(format!("{} repeated", v.padded));
                    }
                    // **Short, and it names the right panel.** The old one
                    // ran to about 155 characters, which `fit_text` shrank
                    // past its own floor — so on a narrower window the line
                    // that says why the take is wrong drew NOTHING at all. And
                    // it sent the user to Take Settings, where the resolution
                    // is not: that is the Export dialog.
                    self.recorder.take_note = Some(format!(
                        "{} - lower the video size in Export",
                        what.join(", ")
                    ));
                }
                // The manifest was written at Stop, before this file existed;
                // fold the finished video into it. §7's report: what the file
                // is, what the rate implies, and what the camera delivered.
                let (width, height) = v.compositor.size();
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.recorder.session.record_video(
                    tangent_record::take::VideoReport {
                        container: tangent_record::encode::CONTAINER.to_owned(),
                        video_codec: tangent_record::encode::VIDEO_CODEC.to_owned(),
                        audio_codec: if v.has_audio {
                            tangent_record::encode::AUDIO_CODEC.to_owned()
                        } else {
                            String::new()
                        },
                        width,
                        height,
                        fps: f64::from(v.fps),
                        // Ticks scheduled over the take. The pump holds these
                        // to the wall clock, so this IS duration x rate.
                        frames_expected: v.due.max(v.next),
                        frames_received: self
                            .recorder
                            .session
                            .camera_frames_delivered()
                            .saturating_sub(v.cam_frames_at_start),
                        // **The three loss counters, which until now nothing
                        // read outside a unit test.** They are the difference
                        // between "the camera did not send it", "the UI could
                        // not keep up with it" and "we chose not to convert
                        // it" — three different faults with three different
                        // fixes, and a take that stutters is unexplainable
                        // without them.
                        frames_superseded: self
                            .recorder
                            .session
                            .camera_frames_superseded()
                            .saturating_sub(v.cam_superseded_at_start),
                        frames_unreadable: self
                            .recorder
                            .session
                            .camera_frames_unreadable()
                            .saturating_sub(v.cam_unreadable_at_start),
                        frames_skipped: self
                            .recorder
                            .session
                            .camera_frames_skipped()
                            .saturating_sub(v.cam_skipped_at_start),
                        // **The one the Linux side could never count.** V4L2
                        // reports every frame it produced in `sequence`, and
                        // the crate never surfaced the field — so a frame the
                        // driver overwrote while the app held its only other
                        // buffer was invisible. See `frames_dropped_late`.
                        frames_dropped_late: self
                            .recorder
                            .session
                            .camera_frames_dropped_late()
                            .saturating_sub(v.cam_dropped_late_at_start),
                        // The band's live "N video frames lost" is exactly
                        // this — record it so the manifest cannot contradict
                        // the counter the tester watched.
                        frames_encoder_dropped: dropped,
                        frames_padded: v.padded,
                    },
                    &file_name,
                );
            }
            Err(e) => {
                self.recorder.engine_error = Some(format!("the video could not be finished: {e}"));
                // A half-written mp4 has no index and no player will open it.
                // Removing it is kinder than leaving a file that looks like a
                // take and is not one.
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The launch splash
// ───────────────────────────────────────────────────────────────────────────

/// How long the splash stays up at minimum.
///
/// Long enough that a fast launch does not FLASH — a splash that appears and
/// vanishes inside two frames is a glitch, not a loading screen — and short
/// enough that it is never the thing keeping anybody waiting.
const SPLASH_MIN: std::time::Duration = std::time::Duration::from_millis(600);
/// And the longest it may EVER stay up.
///
/// The cap is the important half. Everything the splash waits on is a device,
/// and a device that never answers is exactly the case where the user must not
/// be left staring at a wordmark with no way through. After this it lifts
/// whether or not anything is ready, and the band underneath says what is still
/// going on — which it was going to do anyway.
const SPLASH_MAX: std::time::Duration = std::time::Duration::from_secs(12);
/// The fade out, once it has been earned.
const SPLASH_FADE: std::time::Duration = std::time::Duration::from_millis(280);

struct Splash {
    since: std::time::Instant,
    /// When everything it was waiting for finished, so the fade can run from
    /// there rather than from the moment the window opened.
    done_at: Option<std::time::Instant>,
}

impl DesktopApp {

    /// Whether any insert bay still has a plugin to load.
    ///
    /// The same comparison `reconcile_inserts` makes to pick the next one, so
    /// the splash and the loader cannot disagree about whether the desk is
    /// finished.
    #[cfg(feature = "recorder")]
    fn inserts_pending(&self) -> bool {
        // Nothing can load without an engine; a desk waiting for one is not
        // waiting for plugins, and the splash must not hang on that.
        if self.recorder.engine.is_none() {
            return false;
        }
        let inserts = tangent_ui::recorder::INSERTS;
        (0..self.recorder.inserts_loaded.len()).any(|i| {
            self.app.insert(i / inserts, i % inserts).map(str::to_owned)
                != self.recorder.inserts_loaded[i]
        })
    }

    /// Paint the splash, and decide when it goes.
    ///
    /// Drawn on the FOREGROUND layer after the app's own frame, so it covers
    /// everything including any dialog that opened underneath it — a Welcome
    /// card half-visible through a loading screen is the sort of detail that
    /// makes an app feel unfinished.
    fn paint_splash(&mut self, ctx: &egui::Context) {
        if self.splash.is_none() {
            return;
        }
        // Read BEFORE the splash is borrowed mutably — both look at `self`.
        #[cfg(feature = "recorder")]
        let inserts = self.inserts_pending();
        #[cfg(not(feature = "recorder"))]
        let inserts = false;
        #[cfg(feature = "recorder")]
        let camera = self.recorder.camera_opening;
        #[cfg(not(feature = "recorder"))]
        let camera = false;
        let Some(splash) = self.splash.as_mut() else {
            return;
        };
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(splash.since);

        // What is still being waited on. The splash no longer SAYS it — the
        // lattice is the whole picture — but readiness still depends on it.
        // **The PLUGINS, which are the whole of a slow launch.**
        //
        // Inserts load one per frame, deliberately — but a JUCE plugin takes
        // the better part of a second to come up, so a desk with several of
        // them spends seconds with the main thread inside somebody else's
        // constructor. The splash used to lift on elapsed time alone, and
        // since frames are a second apart while this is happening it was gone
        // by the second one: the owner watched a half-drawn window for seven
        // seconds and reported, exactly right, "insanely long and zero
        // splash".
        //
        // So it waits for them. That is what makes the splash cover the load
        // rather than announce the end of it — and `SPLASH_MAX` is still the
        // backstop for a plugin that never answers.
        let busy = camera || inserts;
        // "Ready" is a minimum time AND nothing outstanding — or the cap.
        if !busy && elapsed >= SPLASH_MIN && splash.done_at.is_none() {
            splash.done_at = Some(now);
        }
        if elapsed >= SPLASH_MAX && splash.done_at.is_none() {
            splash.done_at = Some(now);
        }

        let fade = match splash.done_at {
            None => 1.0,
            Some(at) => {
                let gone = now.duration_since(at).as_secs_f32();
                1.0 - (gone / SPLASH_FADE.as_secs_f32()).clamp(0.0, 1.0)
            }
        };
        if fade <= 0.0 {
            self.splash = None;
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("tangent-splash"),
        ));
        let rect = ctx.screen_rect();
        tangent_ui::splash::draw(&painter, rect, fade);
        // While it is up, the window must keep repainting — nothing else is
        // asking it to, and a splash that freezes mid-fade because no input
        // arrived is worse than none.
        ctx.request_repaint();
    }
}

#[cfg(all(test, feature = "recorder"))]
mod tests {

    /// **A unit's preset drawer is spelled the way its chip is.** The
    /// built-ins come off the loader's own table, so the folder and the bay
    /// label cannot drift; every Keys patch shares one drawer; and an
    /// identity this build has no row for still gets a stable,
    /// separator-free folder rather than a panic or a path.
    #[test]
    fn every_unit_identity_files_under_a_readable_safe_folder() {
        assert_eq!(super::preset_subfolder("keys"), "Tangent Keys");
        assert_eq!(super::preset_subfolder("keys:0"), "Tangent Keys");
        assert_eq!(super::preset_subfolder("keys:7"), "Tangent Keys");
        // The table row the trim proved the road with.
        assert_eq!(
            super::preset_subfolder(crate::builtin_fx::TRIM_PATH),
            "Tangent Trim"
        );
        // And every row the table will ever hold, by construction.
        for spec in crate::builtin_fx::UNITS {
            assert_eq!(super::preset_subfolder(spec.path), spec.class);
        }
        // A newer build's sentinel: sanitized identity, `@` shed.
        assert_eq!(super::preset_subfolder("@tangent-shine"), "tangent-shine");
        for hostile in ["../evil", "@a/b", ""] {
            let folder = super::preset_subfolder(hostile);
            assert!(!folder.contains('/') && !folder.contains('\\'));
            assert!(!folder.starts_with('.'), "{hostile:?} → {folder:?}");
            assert!(!folder.is_empty());
        }
    }

    /// **A delete request only ever lands inside the preset root, on one of
    /// our own files.** The request carries a path the UI got from a row the
    /// host pushed — but rows go stale, roots get moved, and a string is a
    /// string, so the guard is what stands between `DeletePreset` and
    /// `remove_file`.
    #[test]
    fn a_delete_request_cannot_reach_outside_the_preset_root() {
        let scratch = std::env::temp_dir().join(format!(
            "tangent-preset-delete-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let root = scratch.join("Presets");
        let drawer = root.join("Tangent EQ");
        std::fs::create_dir_all(&drawer).expect("drawer");
        let inside = drawer.join("warm.tangentpreset");
        std::fs::write(&inside, "{}").expect("inside");
        let beside = scratch.join("victim.tangentpreset");
        std::fs::write(&beside, "{}").expect("beside");
        let wrong_kind = drawer.join("notes.txt");
        std::fs::write(&wrong_kind, "keep me").expect("wrong kind");

        // The one legal target answers, resolved.
        assert_eq!(
            super::preset_delete_target(&root, &inside).as_deref(),
            Some(inside.canonicalize().expect("canon").as_path())
        );
        // A file OUTSIDE the root is refused even though it wears our
        // extension — and a traversal that names it through the root is
        // refused too, because the check runs on the resolved path.
        assert_eq!(super::preset_delete_target(&root, &beside), None);
        let sneaky = drawer.join("..").join("..").join("victim.tangentpreset");
        assert_eq!(super::preset_delete_target(&root, &sneaky), None);
        // A file inside the root that is not a preset is refused.
        assert_eq!(super::preset_delete_target(&root, &wrong_kind), None);
        // A root that does not exist deletes nothing rather than resolving
        // relative to whatever the process's directory happens to be.
        assert_eq!(
            super::preset_delete_target(&scratch.join("gone"), &inside),
            None
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// **The engine's knob table and the panel's are the same table.**
    ///
    /// The panel cannot see the audio crate, so `dialogs::KEYS_PARAM_SPECS`
    /// is a hand-copy of `keys::Patch::params()` — names, one-liners and
    /// defaults. A knob added on one side and forgotten on the other does not
    /// fail to compile and does not fail to run: it ships a panel whose
    /// slider writes into a slot the instrument never reads, or an instrument
    /// with a knob nothing can reach. This is the only thing standing between
    /// those two tables.
    #[test]
    fn the_panel_knobs_and_the_engine_knobs_are_the_same_knobs() {
        use tangent_ui::dialogs::{KEYS_PARAMS, KEYS_PARAM_SPECS};
        assert_eq!(
            KEYS_PARAM_SPECS.len(),
            crate::keys::Patch::ALL.len(),
            "one table has a sound the other does not"
        );
        assert_eq!(
            KEYS_PARAMS,
            crate::keys::MAX_PARAMS,
            "the panel and the engine disagree about how many knobs a sound has"
        );
        for (index, patch) in crate::keys::Patch::ALL.iter().enumerate() {
            let engine = patch.params();
            let panel = &KEYS_PARAM_SPECS[index];
            assert_eq!(
                engine.len(),
                panel.len(),
                "{patch:?}: knob counts differ"
            );
            for (knob, (spec, ui)) in engine.iter().zip(panel.iter()).enumerate() {
                assert_eq!(spec.name, ui.0, "{patch:?} knob {knob}: name");
                assert_eq!(spec.about, ui.1, "{patch:?} knob {knob}: description");
                assert!(
                    (spec.default - ui.2).abs() < 1.0e-6,
                    "{patch:?} knob {knob}: the panel would reset to {} and the engine to {}",
                    ui.2,
                    spec.default
                );
            }
        }
    }

    /// **The suite's two tables are one list.** The UI's `SUITE_UNITS`
    /// routes faces and fills the picker; the engine's `UNITS` seats the
    /// sound. A path in one and not the other is a face over an empty bay
    /// or a bay no picker can reach — both silent, both fatal to the whole
    /// idea, so the build refuses instead.
    #[test]
    fn the_suite_faces_and_the_suite_units_are_the_same_list() {
        for (path, name, about) in tangent_ui::faces::SUITE_UNITS {
            let spec = crate::builtin_fx::spec_for(path)
                .unwrap_or_else(|| panic!("{path} has a face and no unit"));
            assert_eq!(spec.class, name, "{path}: the chip and the face disagree");
            assert!(!about.is_empty());
            // The trademark rule, same as every effect since the phaser:
            // the NAME is ours; hardware lives in descriptions only.
            for word in ["Tascam", "Fender", "Rhodes", "Wurlitzer", "Studer", "Ampex"] {
                assert!(
                    !name.contains(word),
                    "{name} wears somebody else's name"
                );
            }
        }
        // And every registered effect unit that is not the hidden trim has
        // a face to open.
        for unit in crate::builtin_fx::UNITS {
            if unit.path == crate::builtin_fx::TRIM_PATH {
                continue;
            }
            assert!(
                tangent_ui::faces::suite_unit(unit.path).is_some(),
                "{} is seatable and faceless",
                unit.path
            );
        }
    }

    /// **The centre of every knob is the instrument the bench signed off.**
    ///
    /// Every mapping in `PianoKnobs`/`EpKnobs` passes through its own tuned
    /// constant at the default, so a strip nobody has touched, a sound just
    /// initialised, and the two-knob build that shipped first all make the
    /// identical sound. A knob whose default multiplies by something other
    /// than one is a silent retune of both patches.
    ///
    /// (The `#[test]` below was stranded on the NEXT function for a release
    /// — this pin compiled, was green in review, and never ran.)
    #[test]
    fn an_initialised_sound_is_the_bench_tuning() {
        let piano = crate::keys::PianoKnobs::from_params(&crate::keys::Patch::Piano1.defaults());
        assert!((piano.decay - 1.0).abs() < 1.0e-5, "decay {}", piano.decay);
        assert!(
            (piano.brilliance - 0.28).abs() < 1.0e-5,
            "brilliance {}",
            piano.brilliance
        );
        assert!((piano.unison - 1.0).abs() < 1.0e-5, "unison {}", piano.unison);
        assert!((piano.stretch - 1.0).abs() < 1.0e-5, "stretch {}", piano.stretch);
        assert!((piano.release - 1.0).abs() < 1.0e-5, "release {}", piano.release);
        assert!((piano.body - 1.0).abs() < 1.0e-5, "body {}", piano.body);

        let ep = crate::keys::EpKnobs::from_params(&crate::keys::Patch::EpMk1.defaults());
        assert!((ep.decay - 1.0).abs() < 1.0e-5, "decay {}", ep.decay);
        assert!((ep.bell - 1.0).abs() < 1.0e-5, "bell {}", ep.bell);
        assert!((ep.bark - 1.0).abs() < 1.0e-5, "bark {}", ep.bark);
        assert!(ep.chime.abs() < 1.0e-5, "chime {}", ep.chime);
        assert!((ep.release - 1.0).abs() < 1.0e-5, "release {}", ep.release);
        assert!((ep.body - 1.0).abs() < 1.0e-5, "body {}", ep.body);

        // The clav: pickup and tone are raw positions the voice reads as
        // shipped (0.35 under the string, rockers at 0.75); everything
        // multiplicative lands on 1.0 and the mute pad ships lifted.
        let clav = crate::keys::ClavKnobs::from_params(&crate::keys::Patch::Clav.defaults());
        assert!((clav.decay - 1.0).abs() < 1.0e-5, "decay {}", clav.decay);
        assert!((clav.pickup - 0.35).abs() < 1.0e-5, "pickup {}", clav.pickup);
        assert!((clav.tone - 0.75).abs() < 1.0e-5, "tone {}", clav.tone);
        assert!((clav.snap - 1.0).abs() < 1.0e-5, "snap {}", clav.snap);
        assert!((clav.release - 1.0).abs() < 1.0e-5, "release {}", clav.release);
        assert!(clav.mute.abs() < 1.0e-5, "mute {}", clav.mute);
        // The wah patch holds the rockers at the shipped setting whatever
        // knob 4 says: the pedal owns the filtering.
        let wah = crate::keys::ClavKnobs::from_params_wah(
            &crate::keys::Patch::ClavWah.defaults(),
        );
        assert!((wah.tone - 0.75).abs() < 1.0e-5, "wah tone {}", wah.tone);

        // The reed pianos: multiplicative knobs through 1.0 on the 200; the
        // 140 ships with its growl leaning (0.62 of a 0..2 map) and its
        // tremolo off — those two defaults ARE the difference between the
        // transistor and the tube instrument, so they are pinned.
        let wur =
            crate::keys::WurliKnobs::from_params(&crate::keys::Patch::Wurli200.defaults());
        assert!((wur.decay - 1.0).abs() < 1.0e-5, "decay {}", wur.decay);
        assert!((wur.reed - 1.0).abs() < 1.0e-5, "reed {}", wur.reed);
        assert!((wur.growl - 1.0).abs() < 1.0e-5, "growl {}", wur.growl);
        assert!((wur.release - 1.0).abs() < 1.0e-5, "release {}", wur.release);
        assert!((wur.thump - 1.0).abs() < 1.0e-5, "thump {}", wur.thump);
        let tube =
            crate::keys::WurliKnobs::from_params(&crate::keys::Patch::Wurli140.defaults());
        assert!((tube.growl - 1.24).abs() < 1.0e-5, "tube growl {}", tube.growl);
        assert!(
            (crate::keys::Patch::Wurli140.defaults()[5]).abs() < 1.0e-5,
            "the tube instrument's tremolo ships off"
        );

        // And WIDTH's default is the stock seating, which is 1.0 of a knob
        // that runs to 1.5 — the one default that is not the middle. True
        // of every patch, because WIDTH means the same thing on all eight.
        for &patch in &crate::keys::Patch::ALL {
            let w = patch.defaults()[crate::keys::P_WIDTH];
            assert!((w * 1.5 - 1.0).abs() < 1.0e-5, "{patch:?} width {w}");
        }
        let mut keys = crate::keys::Keys::new(48_000.0);
        keys.reset_params();
        let w = keys.params()[crate::keys::P_WIDTH];
        assert!((w * 1.5 - 1.0).abs() < 1.0e-5, "width {w}");
    }

    /// **A launch never deletes a session somebody touched.** The launch
    /// reclaim runs `remove_dir_all` before cloning the template — the one
    /// deletion in the app that no dialog ever guards, because at launch
    /// there is no mouth to ask with. So the predicate that licenses it
    /// must refuse anything but a verbatim, take-free clone.
    #[test]
    fn a_launch_reclaims_only_a_verbatim_take_free_clone() {
        let root = std::env::temp_dir()
            .join(format!("tangent-launch-reclaim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let template = root.join("Templates").join("default");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("project.tangent"), b"the stencil").unwrap();

        // A verbatim clone with an empty media/: reclaimable.
        let scratch = root.join("Untitled");
        std::fs::create_dir_all(scratch.join("media")).unwrap();
        std::fs::write(scratch.join("project.tangent"), b"the stencil").unwrap();
        assert!(super::launch_reclaimable(&scratch, &template));

        // One take inside: not for deletion, whatever the document says.
        std::fs::write(scratch.join("media").join("take.wav"), b"audio").unwrap();
        assert!(!super::launch_reclaimable(&scratch, &template));
        std::fs::remove_file(scratch.join("media").join("take.wav")).unwrap();

        // An edited document: somebody worked here.
        std::fs::write(scratch.join("project.tangent"), b"an edit").unwrap();
        assert!(!super::launch_reclaimable(&scratch, &template));

        // A document that cannot be read equals nothing — refused, because
        // deleting what could not be compared is the same wound.
        std::fs::remove_file(scratch.join("project.tangent")).unwrap();
        assert!(!super::launch_reclaimable(&scratch, &template));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **Every seat's bays are reachable, the master's second rack included.**
    ///
    /// `inserts_loaded` is flat — `seat * INSERTS + bay` — and the reconcile
    /// walks its whole length. Size it short and the seats past the end are
    /// never visited, silently: a plugin there is never handed to the engine,
    /// so it is missing from playback and PRESENT in the export, because
    /// settings and the exporter size themselves from `INSERT_SPACE`.
    ///
    /// It was sized `(STRIPS + 1 + DRUM_PADS) * INSERTS` in FIVE places, which
    /// is 444 — one seat short of `MASTER_EXTRA_SEAT`, whose bays are 444–446.
    /// The arithmetic is the whole bug, so the arithmetic is what this asserts.
    #[test]
    fn the_insert_table_reaches_every_seat_including_the_masters_second_rack() {
        use tangent_ui::recorder::{
            DRUM_PADS, INSERTS, INSERT_SPACE, MASTER_EXTRA_SEAT, MASTER_SEAT, STRIPS, TOTAL_SEATS,
        };
        // The size that was wrong, named so the test says what it prevents.
        let short = (STRIPS + 1 + DRUM_PADS) * INSERTS;
        assert!(
            MASTER_EXTRA_SEAT * INSERTS >= short,
            "the old sizing is no longer short; this test has lost its subject"
        );

        // Every seat the app can address has all its bays inside the table.
        for seat in [0, MASTER_SEAT, MASTER_EXTRA_SEAT, TOTAL_SEATS - 1] {
            for bay in 0..INSERTS {
                assert!(
                    seat * INSERTS + bay < INSERT_SPACE,
                    "seat {seat} bay {bay} falls outside INSERT_SPACE"
                );
            }
        }

        // And the master's second rack specifically is PAST the old bound —
        // which is the fact that made this invisible rather than a panic.
        assert!(
            MASTER_EXTRA_SEAT * INSERTS >= short,
            "the master's extra rack no longer sits past the old sizing"
        );
        assert!(
            (MASTER_EXTRA_SEAT + 1) * INSERTS <= INSERT_SPACE,
            "INSERT_SPACE does not cover the master's second rack"
        );
    }

    /// **The default pedalboard seeds exactly once, and only on a
    /// channel.** The once-only marker is "any non-empty pedal cell" — the
    /// seeded board itself trips it — so the decision is held here as the
    /// plain logic it is, with the reconcile's write path left to write.
    #[cfg(feature = "recorder")]
    #[test]
    fn the_default_pedalboard_seeds_once_and_only_on_channels() {
        use tangent_ui::recorder::{AMP_PEDALS, CHANNELS};
        let empty = |_p: usize, _b: usize| false;
        let board = super::amp_board_to_seed(3, empty).expect("an untouched channel seeds");
        assert_eq!(board.len(), AMP_PEDALS);
        assert!(board.iter().all(|p| p.starts_with("@tangent-")));
        assert_eq!(board[0], "@tangent-comp", "dynamics lead the chain");
        assert_eq!(board[AMP_PEDALS - 1], "@tangent-plate", "the room comes last");
        // ONE non-empty cell anywhere on the board — the seeded board
        // itself included — blocks a second seeding, first and last cells
        // both checked because the walk has two loops to get wrong.
        assert!(super::amp_board_to_seed(3, |p, b| p == 0 && b == 0).is_none());
        assert!(
            super::amp_board_to_seed(3, |p, b| p == AMP_PEDALS - 1 && b == 2).is_none(),
            "a tenant in the last pedal's last bay did not count as touched"
        );
        // Past the channels — the master's seat and everything after — no
        // board: those amps play bare in v1, and seeding would land the
        // pedals on seat math that means another channel's board.
        assert!(super::amp_board_to_seed(CHANNELS, empty).is_none());
        assert!(super::amp_board_to_seed(usize::MAX, empty).is_none());
    }

    /// **The export renders the STRETCHED audio, at the stretched length.**
    ///
    /// The live path stretched and the bounce did not, so a stretched clip
    /// exported at the wrong speed and pitch under its drawn window — and
    /// an open-ended one at `file_s / ratio` of its heard length. Same
    /// divergence class the export already paid for once with `clip.gain`.
    #[test]
    fn the_bounce_renders_a_stretched_clip_at_its_stretched_length() {
        use std::sync::Arc;
        // Two seconds of stereo silence at 8 kHz; a 2.0 tape stretch makes
        // exactly four ((frames * ratio).round() — tape is length-exact,
        // which is why it is the mode under test).
        let decoded = Arc::new(tangent_record::decode::Clip {
            samples: vec![0.0; 8_000 * 2 * 2],
            rate: 8_000,
            source: std::path::PathBuf::from("a.wav"),
        });
        let clip = tangent_ui::arrangement::ArrClip {
            kind: tangent_ui::arrangement::ClipKind::Audio,
            notes: Vec::new(),
            lane: 0,
            path: "a.wav".to_owned(),
            start_s: 0.0,
            offset_s: 0.0,
            // Open-ended: the length comes from the FILE, which is the
            // case the unstretched bounce got wrong by a factor of the
            // ratio.
            len_s: 0.0,
            loop_s: 0.0,
            fade_in_s: 0.0,
            fade_out_s: 0.0,
            name: "A".to_owned(),
            id: 1,
            extra: Default::default(),
            anchor: Default::default(),
            follow_tempo: false,
            stretch: 2.0,
            stretch_mode: "tape".to_owned(),
            gain: 1.0,
        };
        let mut cache = std::collections::HashMap::new();
        let got = super::stretched_clip_audio(
            &clip,
            Arc::clone(&decoded),
            std::path::Path::new("a.wav"),
            &mut cache,
        );
        assert!(
            (got.seconds() - 4.0).abs() < 1.0e-9,
            "the stretch did not double the audio: {} s",
            got.seconds()
        );
        // What the offline bounce now schedules: passes over the STRETCHED
        // copy, so the open-ended clip fills its heard length.
        let passes = super::audio_clip_passes(&clip, &got, 8_000.0, 1.0);
        assert_eq!(passes.len(), 1);
        assert_eq!(
            passes[0].len,
            32_000,
            "the bounce would schedule the unstretched length"
        );
        // Memoised: a second ask is the same Arc, not a second render.
        let again = super::stretched_clip_audio(
            &clip,
            Arc::clone(&decoded),
            std::path::Path::new("a.wav"),
            &mut cache,
        );
        assert!(Arc::ptr_eq(&got, &again), "the cache did not hold the copy");
        // And an unstretched clip passes through untouched.
        let plain = tangent_ui::arrangement::ArrClip {
            stretch: 1.0,
            ..clip
        };
        let through = super::stretched_clip_audio(
            &plain,
            Arc::clone(&decoded),
            std::path::Path::new("a.wav"),
            &mut cache,
        );
        assert!(Arc::ptr_eq(&decoded, &through));
    }

    /// **A looped AUDIO clip repeats, exactly as a looped MIDI one does.**
    ///
    /// The loop was spelled out for MIDI and not for audio, so a session with
    /// two of each looped half its tracks: the owner's "Looping misses 2/4
    /// tracks when it loops."
    #[test]
    fn a_looped_audio_clip_is_spelled_out_into_passes() {
        use std::sync::Arc;
        let audio = |loop_s: f64, len_s: f64| tangent_ui::arrangement::ArrClip {
            kind: tangent_ui::arrangement::ClipKind::Audio,
            notes: Vec::new(),
            lane: 2,
            path: "a.wav".to_owned(),
            start_s: 4.0,
            offset_s: 0.5,
            len_s,
            loop_s,
            fade_in_s: 0.1,
            fade_out_s: 0.2,
            name: "A".to_owned(),
            id: 1,
            extra: Default::default(),
            anchor: Default::default(),
            follow_tempo: false,
            stretch: 1.0,
            stretch_mode: String::new(),
            gain: 0.5,
        };
        let decoded = Arc::new(tangent_record::decode::Clip {
            samples: vec![0.0; 48_000 * 2 * 10],
            rate: 48_000,
            source: std::path::PathBuf::from("a.wav"),
        });

        // No loop: one pass, unchanged.
        let one = super::audio_clip_passes(&audio(0.0, 3.0), &decoded, 48_000.0, 0.5);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].start, 4 * 48_000);
        assert_eq!(one[0].len, 3 * 48_000);
        assert_eq!(one[0].fade_in, 4_800);
        assert_eq!(one[0].fade_out, 9_600);

        // A one-second loop under a 2.5 s clip: three passes, the last a
        // HALF one, each reading the same trim into the file.
        let many = super::audio_clip_passes(&audio(1.0, 2.5), &decoded, 48_000.0, 0.5);
        assert_eq!(many.len(), 3, "the loop was not spelled out");
        for (n, p) in many.iter().enumerate() {
            assert_eq!(p.start, (4 + n as u64) * 48_000, "pass {n} starts wrong");
            assert_eq!(p.offset, 24_000, "pass {n} lost the trim");
            assert_eq!(p.strip, 2);
            assert!((p.gain - 0.5).abs() < 1e-6, "pass {n} lost the clip's gain");
        }
        assert_eq!(many[2].len, 24_000, "the partial last pass was not cut");
        // The fades belong to the CLIP, not to a repeat: head on the first,
        // tail on the last, nothing in between. A fade on every pass would
        // be a tremolo nobody asked for.
        assert_eq!((many[0].fade_in, many[0].fade_out), (4_800, 0));
        assert_eq!((many[1].fade_in, many[1].fade_out), (0, 0));
        assert_eq!((many[2].fade_in, many[2].fade_out), (0, 9_600));
    }

    /// **A reorder reloads the rows that moved and nothing else.**
    ///
    /// The whole desk used to be marked — sixteen channels, the buses, the
    /// master and 128 drum pads, some 450 bays — and the reconcile loop
    /// reloads exactly one mismatched bay per frame, each one instantiating
    /// a plugin and restoring its state. Dragging one track past its
    /// neighbour therefore tore down and rebuilt every plugin in the
    /// session: the owner's "when a reorder occurs of any kind -- massive
    /// delay".
    #[test]
    fn a_reorder_only_reloads_the_rows_that_moved() {
        let bays = 3;
        // Track 0 dragged to position 2: rows 0, 1 and 2 move, the rest do
        // not.
        let mut order: Vec<usize> = (0..16).collect();
        let ch = order.remove(0);
        order.insert(2, ch);
        let hit = super::reload_slots_for(&order, bays);
        let rows: std::collections::BTreeSet<usize> =
            hit.iter().map(|i| i / bays).collect();
        assert_eq!(
            rows,
            [0usize, 1, 2].into_iter().collect(),
            "a reorder of three rows touched {rows:?}"
        );
        assert_eq!(hit.len(), 3 * bays, "not every bay of a moved row moved");
        // Nothing at all when nothing moved.
        let still: Vec<usize> = (0..16).collect();
        assert!(super::reload_slots_for(&still, bays).is_empty());
        // And never past the rows the permutation names — the drum pads
        // above the channels are not part of a channel reorder.
        assert!(hit.iter().all(|i| i / bays < 16));
    }
    use super::*;

    /// **A clip's picture covers what is on screen, at the pixels it
    /// occupies.**
    ///
    /// A fixed bucket count cannot be right at every zoom, and computing the
    /// whole of a long clip at screen resolution every time the view moves is
    /// a pass over the part of it nobody can see. The span is bounded by the
    /// WINDOW, which is what makes the cost independent of how long the clip
    /// is.
    #[test]
    fn a_clips_picture_follows_the_window_and_not_its_length() {
        // A five-minute clip, ten seconds of it on screen across 1000 points.
        let got = wave_span(0.0, 300.0, 100.0, 10.0, 1000.0);
        let (from, span, buckets) = got.expect("a clip under the window has a picture");
        // A margin either side, so a small pan does not have to redraw.
        assert!(from < 100.0 && from >= 88.0, "from {from}");
        assert!(span >= 10.0 && span <= 13.0, "span {span}");
        // About one bucket per pixel of the span, and never a screenful more.
        let px = span * 100.0;
        assert!(
            (buckets as f64 - px).abs() <= 2.0,
            "{buckets} buckets for {px:.0} pixels"
        );
        assert!(buckets <= 8_192);

        // **The same window over a clip a thousand times longer is the same
        // work.** This is the whole point.
        let long = wave_span(0.0, 300_000.0, 100.0, 10.0, 1000.0).unwrap();
        assert_eq!(long.2, buckets, "a longer clip cost more buckets");

        // Zoomed right in: more buckets for fewer seconds, which is the
        // resolution that was missing.
        let close = wave_span(0.0, 300.0, 100.0, 0.5, 1000.0).unwrap();
        assert!(close.1 < 1.0, "span {}", close.1);
        assert!(close.2 >= 500, "{} buckets at a half-second zoom", close.2);

        // A clip nowhere near the window has no picture to make.
        assert!(wave_span(0.0, 5.0, 100.0, 10.0, 1000.0).is_none());
        assert!(wave_span(400.0, 5.0, 100.0, 10.0, 1000.0).is_none());
        // ...and neither does a degenerate ask.
        assert!(wave_span(0.0, 0.0, 0.0, 10.0, 1000.0).is_none());
        assert!(wave_span(0.0, 10.0, 0.0, 0.0, 1000.0).is_none());
        assert!(wave_span(0.0, 10.0, 0.0, 10.0, 0.0).is_none());
        // A clip that only PARTLY overlaps is clipped to itself, never past
        // its own ends — a picture that claimed seconds the clip does not
        // have would draw somebody else's audio at its edge.
        let (from, span, _) = wave_span(105.0, 2.0, 100.0, 10.0, 1000.0).unwrap();
        assert!(from >= 105.0 - 1e-9, "from {from} is before the clip");
        assert!(from + span <= 107.0 + 1e-9, "it ran past the clip's end");
    }

    /// The envelope is MIN and MAX, and a silent clip is a flat line rather
    /// than a NaN.
    #[test]
    fn the_envelope_carries_both_sides() {
        let clip = tangent_record::decode::Clip {
            // A sawtooth from −1 to +1: the two sides are different, which is
            // what a peak-only envelope cannot show.
            samples: (0..1000)
                .flat_map(|i| {
                    let v = (i as f32 / 500.0) - 1.0;
                    [v, v]
                })
                .collect(),
            rate: 48_000,
            source: std::path::PathBuf::from("t.wav"),
        };
        let env = span_envelope(&clip, 0, 1000, 4);
        assert_eq!(env.len(), 8, "four buckets is four pairs");
        for k in 0..4 {
            let (lo, hi) = (env[k * 2], env[k * 2 + 1]);
            assert!(lo <= hi, "bucket {k} runs backwards: {lo}..{hi}");
            assert!(lo >= -1.0 && hi <= 1.0, "bucket {k} left the range");
        }
        // The first bucket is the negative quarter and the last the positive
        // one, which a mirrored peak envelope would have drawn identically.
        assert!(env[0] < -0.4 && env[1] < 0.0, "{:?}", &env[..2]);
        assert!(env[6] > 0.0 && env[7] > 0.4, "{:?}", &env[6..]);

        let quiet = tangent_record::decode::Clip {
            samples: vec![0.0; 200],
            rate: 48_000,
            source: std::path::PathBuf::from("q.wav"),
        };
        assert!(span_envelope(&quiet, 0, 100, 4).iter().all(|v| *v == 0.0));
        // Nothing to draw is nothing, not a panic.
        assert!(span_envelope(&quiet, 0, 0, 4).is_empty());
        assert!(span_envelope(&quiet, 0, 100, 0).is_empty());
        assert!(span_envelope(&quiet, 9_999, 100, 4).is_empty());
    }

    /// **A fresh engine is told the song's musical time.**
    ///
    /// `push_maps` is change-detected against `pushed_maps` because
    /// `set_maps` moves a `Vec` across a ring and blocks until the displaced
    /// pair comes back — it cannot run per frame. That makes the cache a
    /// promise about a PARTICULAR engine, and `start_engine` replaces the
    /// engine: a fresh `Renderer` begins at `Maps::default()`, 120 and 4/4.
    /// Leaving the cache alone means the equality check short-circuits on the
    /// next frame and `set_maps` is never called again, so the click plays
    /// 120/4/4 for the rest of the session while the ruler draws the song's
    /// real bar lines. Silent, and sticky until somebody edits the tempo.
    ///
    /// **A source scan, and the reason is that `DesktopApp` cannot be built
    /// in a test** — it owns real devices, a window and an engine. The two
    /// scanning tests in `settings.rs` set the precedent for a rule that
    /// cannot be expressed as a runtime assertion. It is narrow on purpose:
    /// it locks in the invalidation somebody would otherwise remove while
    /// tidying, and it will not notice a NEW engine-scoped cache being
    /// forgotten, which is a limit worth knowing rather than pretending away.
    #[test]
    fn a_restarted_engine_is_told_the_tempo_map_again() {
        let src = include_str!("desktop.rs");
        let start = src
            .find("fn start_engine")
            .expect("the engine starter is still called that");
        let body = &src[start..start + 4_000];
        assert!(
            body.contains("self.pushed_maps = None;"),
            "`start_engine` does not invalidate the tempo-map cache, so the new \
             engine keeps 120/4/4 while the document says otherwise"
        );
        // Beside the siblings it belongs with, which is what makes it get
        // noticed by the next person editing that block.
        let reastream = body.find("self.recorder.reastream = None;").expect("sibling");
        let maps = body.find("self.pushed_maps = None;").expect("checked above");
        let inserts = body.find("self.recorder.inserts_loaded").expect("sibling");
        assert!(
            reastream < maps && maps < inserts,
            "the map invalidation drifted away from the other engine-scoped \
             resets; keep them together or the next one is forgotten too"
        );
    }

    /// **The MP3 pass writes a real MPEG stream.** A sine goes in, a file
    /// the app's own decoder accepts comes out — encode and decode meeting
    /// proves the delivery path, not just LAME's return code.
    #[cfg(feature = "mp3")]
    #[test]
    fn an_mp3_export_round_trips_through_the_decoder() {
        let dir = std::env::temp_dir().join(format!("tangent-mp3-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("probe.mp3");
        let rate = 44_100u32;
        let stereo: Vec<f32> = (0..rate as usize)
            .flat_map(|i| {
                let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        encode_mp3(&stereo, rate, false, &path).expect("encode");
        let decoded = tangent_record::decode::decode(&path, rate).expect("decode");
        assert!(
            decoded.seconds() > 0.8 && decoded.seconds() < 1.4,
            "a second in, {} seconds out",
            decoded.seconds()
        );
        let peak = decoded.samples.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.3 && peak < 0.8, "the sine came back at {peak}");
        let _ = std::fs::remove_file(&path);
    }

    // ── the IR wizard's host half ───────────────────────────────────────────

    /// A synthetic "hardware loop" capture: pre-roll of near-silence, then
    /// the sweep convolved through a little two-tap room, then tail — with a
    /// −90 dB noise floor throughout, so the floor read and the truncation
    /// both have something honest to find.
    fn synthetic_ir_capture(
        sweep: &[f32],
        pre: usize,
        tail: usize,
        rate: f64,
    ) -> (Vec<f32>, Vec<f32>) {
        // The DUT: a direct arrival and one echo 30 ms later, decaying.
        let mut ir = vec![0.0f32; (0.15 * rate) as usize];
        ir[0] = 0.8;
        let echo = (0.030 * rate) as usize;
        for i in 0..(0.08 * rate) as usize {
            let t = i as f64 / rate;
            ir[echo + i] += (0.3 * (-t / 0.015).exp()) as f32
                * ((std::f64::consts::TAU * 900.0 * t).cos() as f32);
        }
        let wet = crate::units::sweep::fft_convolve(sweep, &ir);
        let frames = pre + sweep.len() + tail;
        let mut noise_seed = 0x2468_ace0_1357_9bdfu64;
        let mut noise = move || {
            noise_seed = noise_seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (((noise_seed >> 33) as f32 / (1u64 << 31) as f32) - 1.0) * 3.0e-5
        };
        let mut capture = vec![0.0f32; frames * 2];
        for f in 0..frames {
            let dry = f
                .checked_sub(pre)
                .and_then(|k| wet.get(k))
                .copied()
                .unwrap_or(0.0);
            capture[f * 2] = dry + noise();
            capture[f * 2 + 1] = dry + noise();
        }
        (capture, ir)
    }

    /// **The whole measurement on synthetic loop data**, both routes: the
    /// classic (no reference) pass recovers the room, reads its floor near
    /// the −90 dB planted for it, trims the loop's lead-in so the direct
    /// arrival sits at the front, and truncates the tail to a usable decay
    /// shorter than the buffer — and a loop-only reference pass makes the
    /// reference route say so.
    #[test]
    fn a_synthetic_capture_deconvolves_to_the_room_on_both_routes() {
        let rate = 48_000.0f64;
        let (f1, f2, dur) = (10.0, 22_000.0f64.min(0.475 * rate), 0.5);
        let sweep = crate::units::sweep::ess(f1, f2, dur, rate);
        let pre = 4_800usize;
        let tail = 9_600usize;
        let (capture, _room) = synthetic_ir_capture(&sweep, pre, tail, rate);

        // The classic route.
        let (l, r, out) =
            super::process_ir_capture(&capture, &sweep, None, pre, rate, f1, f2, dur);
        assert!(!out.has_reference);
        assert_eq!(out.envelope_db.len(), super::IR_ENVELOPE_POINTS);
        assert!(
            (-100.0..=-85.0).contains(&out.noise_floor_db),
            "the planted ~-95 dB floor read as {}",
            out.noise_floor_db
        );
        // The direct arrival leads: the trim put the peak near the front.
        let peak = crate::units::sweep::linear_peak(&l);
        assert!(
            (peak as f64) < 0.005 * rate,
            "the direct arrival sits {peak} samples in"
        );
        // The echo survives where the room put it, on both channels.
        let echo_at = (0.030 * rate) as usize;
        let near_echo = |ch: &[f32]| {
            ch[echo_at.saturating_sub(96)..(echo_at + 96).min(ch.len())]
                .iter()
                .fold(0.0f32, |a, &b| a.max(b.abs()))
        };
        assert!(
            near_echo(&l) > 0.05 && near_echo(&r) > 0.05,
            "the 30 ms echo went missing: {} / {}",
            near_echo(&l),
            near_echo(&r)
        );
        // Truncated to a decay, not the whole capture window.
        assert!(
            out.usable_s > 0.02 && out.usable_s < 0.30,
            "usable decay reads {} s",
            out.usable_s
        );
        // The set peaks at the norm target, channels locked.
        let top = l
            .iter()
            .chain(r.iter())
            .fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!((20.0 * top.log10() - (-0.2)).abs() < 0.1, "peak at {top}");
        assert_eq!(l.len(), r.len(), "the pair came back different lengths");

        // The reference route: a loop-only pass of the same shape.
        let mut reference = vec![0.0f32; capture.len()];
        for f in 0..capture.len() / 2 {
            let v = f
                .checked_sub(pre)
                .and_then(|k| sweep.get(k))
                .copied()
                .unwrap_or(0.0);
            reference[f * 2] = v;
            reference[f * 2 + 1] = v;
        }
        let (l2, _r2, out2) = super::process_ir_capture(
            &capture, &sweep, Some(&reference), pre, rate, f1, f2, dur,
        );
        assert!(out2.has_reference, "the reference pass was not used");
        let peak2 = crate::units::sweep::linear_peak(&l2);
        assert!(
            (peak2 as f64) < 0.005 * rate,
            "the reference route left the arrival {peak2} samples in"
        );
        assert!(
            near_echo(&l2) > 0.05,
            "the reference route lost the echo"
        );
    }

    /// **A saved IR round-trips through the shelf**: written as 32-bit float
    /// stereo, `wav_seconds` reads its length off the header alone,
    /// `read_pcm` gets the samples back, and the scan lists it — from the
    /// root and from one subfolder down — while junk is skipped without a
    /// word.
    #[test]
    fn a_saved_ir_round_trips_through_the_shelf_scan() {
        let root = std::env::temp_dir().join(format!(
            "tangent-irs-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("Reverbs");
        std::fs::create_dir_all(&sub).expect("irs dir");
        let spec = tangent_record::wav::WavSpec {
            sample_rate: 48_000,
            channels: 2,
            format: tangent_record::wav::SampleFormat::Float32,
        };
        let bext = tangent_record::wav::Bext::new(
            tangent_record::wav::Wallclock::from_unix(1_786_804_327, 0),
            spec,
        );
        // Half a second of decaying stereo, values past nothing special.
        let frames = 24_000usize;
        let samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let v = 0.9 * (-(i as f32) / 4_800.0).exp();
                [v, -v * 0.5]
            })
            .collect();
        for path in [root.join("spring.wav"), sub.join("plate.wav")] {
            let mut w = tangent_record::wav::WavWriter::create(&path, spec, &bext)
                .expect("create");
            w.write_interleaved(&samples).expect("write");
            w.finish().expect("finish");
        }
        // Junk beside them: not WAVs, not listed, not fatal.
        std::fs::write(root.join("notes.txt"), b"not audio").expect("junk");
        std::fs::write(root.join("broken.wav"), b"RIFFxxxx").expect("junk wav");

        let secs = super::wav_seconds(&root.join("spring.wav")).expect("header read");
        assert!((secs - 0.5).abs() < 0.01, "header says {secs} s");
        let bytes = std::fs::read(root.join("spring.wav")).expect("bytes");
        let (back_spec, back) =
            tangent_record::wav::read_pcm(&bytes).expect("read back");
        assert_eq!(back_spec.channels, 2);
        assert_eq!(back.len(), samples.len());
        let worst = back
            .iter()
            .zip(&samples)
            .fold(0.0f32, |a, (x, y)| a.max((x - y).abs()));
        assert!(worst < 1.0e-6, "float samples moved by {worst}");

        let rows = super::scan_ir_rows(&root);
        // Path-sorted, subfolder capital before root lowercase — the scan's
        // one ordering rule is "stable and boring".
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["plate", "spring"],
            "the shelf lists {rows:?}"
        );
        assert!(rows.iter().all(|r| (r.seconds - 0.5).abs() < 0.01));
        let _ = std::fs::remove_dir_all(&root);
    }
}
