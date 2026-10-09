//! What the Recorder band and the Export dialog are made of, as plain data.
//!
//! This module is the seam between the GUI and the machinery. `tangent-record`
//! owns cameras, audio devices, WAV files and take directories; `tangent-ui` must
//! not be able to reach any of that (see `tangent-ui/Cargo.toml` and
//! `scripts/check-firewall.sh`). So the band does not ASK for anything — it is
//! handed a [`RecorderView`] snapshot each frame by whoever is hosting it, and
//! it paints that.
//!
//! The direction of every field here is worth stating once, because it is what
//! keeps the firewall honest:
//!
//!   * **In** — the binary fills a `RecorderView` from the real recorder.
//!   * **Out** — [`recorder_panel::hit_test`] returns a `Hit`, the app turns it
//!     into a request, and the binary performs it after the frame.
//!
//! Nothing in this file opens, reads or writes anything.
//!
//! # Why some obvious things are `&str` and not computed here
//!
//! The folder name a take will get, and how many minutes fit on the disk, are
//! both things this module could *almost* work out. It deliberately does not.
//! Take naming and sanitisation live in `tangent_record::take` — every Windows
//! reserved name, every path-length edge — and a second implementation here
//! would agree with it right up until someone fixed a bug in one of them. So
//! the band is handed the answer as a string and renders it.
//!
//! See `docs/RECORDER-PLAN.md` §5.

use serde_json::{Map, Value};

/// Where a take is in its life.
///
/// Four states rather than a bool because two of them have to be visibly
/// different from a distance: pre-roll is the one where the user is walking
/// back to the bench, and finishing is the one where the files are still being
/// closed and pulling the power cable would cost the take.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum RecordState {
    /// Not recording. The preview is large, the meter is live, and the
    /// destination controls are all reachable.
    #[default]
    Idle,
    /// Counting the take in, in BEATS.
    ///
    /// Beats and not seconds because a count-in is a musical instruction: "two
    /// bars" is what a musician asks for and what the click is playing, and a
    /// countdown in seconds against a click in beats is two clocks disagreeing
    /// in front of the person trying to come in on time.
    CountIn { beat: u32, of: u32 },
    /// Rolling. The take started at the instant this was entered.
    Rolling,
    /// Stop was pressed and the files are being closed.
    ///
    /// Its own state because "the button did nothing" is what a user concludes
    /// from a UI that returns to Idle while a 2 GB file is still flushing.
    Finishing,
}

impl RecordState {
    /// Whether the take is live — the count-in included, because the layout has
    /// already switched and Record has already been pressed.
    pub fn is_active(self) -> bool {
        !matches!(self, RecordState::Idle)
    }

    /// Whether audio is actually being written. The count-in is not: the whole
    /// point of it is that the bars before the downbeat are not in the file.
    pub fn is_writing(self) -> bool {
        matches!(self, RecordState::Rolling | RecordState::Finishing)
    }
}

/// One channel's worth of level, as the meter needs it.
///
/// Peak AND rms, because they answer different questions: rms is what the
/// signal sounds like and peak is what will clip. A meter showing only one of
/// them is the reason people record either silence or distortion.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Level {
    /// Instantaneous peak, linear 0.0..=1.0+ (values above 1.0 are real and
    /// must not be clamped away before `clipped` is decided).
    pub peak: f32,
    /// Short-window rms, linear.
    pub rms: f32,
    /// Slow-falling peak hold, linear. Drawn as a line, not a fill.
    pub hold: f32,
    /// **What the NEEDLE shows**: the level with a VU's own ballistics on it.
    ///
    /// Its own field because `rms` cannot do this job. The window `rms` is
    /// measured over is whatever arrived between two repaints — it is reset by
    /// every publish — so its integration time is the frame interval, sixteen
    /// milliseconds on a good day and whatever the machine felt like on a bad
    /// one. A needle driven straight off that follows the waveform instead of
    /// the level, which is the flailing the owner reported, and it flails
    /// differently depending on how busy the app is.
    ///
    /// Filled by the app, which is the only layer that knows how much real
    /// time passed. Zero everywhere else, including in the host's conversion:
    /// a meter with no ballistics applied should read nothing rather than
    /// quietly show an unsmoothed number.
    pub vu: f32,
}

/// The whole meter, plus the one bit that survives the take.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Meters {
    pub left: Level,
    pub right: Level,
    /// Mono sources paint one bar rather than two identical ones.
    pub mono: bool,
    /// **Latched**, not instantaneous. Once a sample has clipped, this stays
    /// true until the next take is armed, so the report after Stop can say so.
    /// An indicator that clears itself is one the performer never sees, because
    /// they were looking at their hands when it happened.
    pub clipped: bool,
}

impl Meters {
    /// Silent, and never clipped — what the band shows with no input open.
    pub const SILENT: Meters = Meters {
        left: Level {
            peak: 0.0,
            rms: 0.0,
            hold: 0.0,
            vu: 0.0,
        },
        right: Level {
            peak: 0.0,
            rms: 0.0,
            hold: 0.0,
            vu: 0.0,
        },
        mono: false,
        clipped: false,
    };

    /// The loudest peak across whatever channels are live.
    pub fn peak(&self) -> f32 {
        if self.mono {
            self.left.peak
        } else {
            self.left.peak.max(self.right.peak)
        }
    }
}

/// How many instruments can be layered at once.
///
/// Three, because that is what the owner asked for and because it is the number
/// that covers the real cases: a piano, a pad under it, and something on top.
/// It is a constant rather than a `Vec` so the band can lay out a fixed number
/// of slots — a variable count would make the band's height depend on how many
/// instruments are loaded, and every band's height in this app is a function of
/// width alone.
/// **Five, up from three.** The two monitor faders moved into the transport
/// group, and the column they left is what the extra rows fit in: a layered
/// pad, a bass and a lead is three, and anybody building a rig wanted more than
/// that the first time they tried.
///
/// Not more than five, because the band lost a fifth of its height in the same
/// change. Six rows in the shorter band put the plugin name, the gain reading
/// and the OPEN WINDOW button at four points, which is a row nobody can read —
/// the count is bounded by what a row can legibly hold, not by the space.
pub const SLOTS: usize = 5;

/// A camera frame that has already been uploaded to the GPU by the host.
///
/// `tangent-ui` never touches a camera: the binary owns the device, converts the
/// frame, and calls `Context::load_texture`. All the band gets is a handle and
/// the frame's real pixel size, which is the only thing it needs to letterbox
/// correctly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preview {
    pub texture: egui::TextureId,
    /// The frame's own size in pixels. **Never reaches the layout** — see
    /// `recorder_panel::fit_preview`. A 4:3 camera must not make the window
    /// taller.
    pub size: egui::Vec2,
}

/// A device slot, as the band renders it.
///
/// Three states and they read differently: no device chosen, a device chosen
/// and open, or a device that was chosen and is now gone (unplugged mid-session
/// is the common one). The third is why this is not `Option<&str>`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceLabel<'a> {
    /// Nothing selected. The band offers the picker.
    None,
    /// Open and working.
    Open(&'a str),
    /// Named in settings but not present right now.
    Missing(&'a str),
}

impl<'a> DeviceLabel<'a> {
    pub fn text(self) -> &'a str {
        match self {
            DeviceLabel::None => "None",
            DeviceLabel::Open(n) | DeviceLabel::Missing(n) => n,
        }
    }

    pub fn is_missing(self) -> bool {
        matches!(self, DeviceLabel::Missing(_))
    }
}

/// Everything the band paints, for one frame.
///
/// A borrowed snapshot rather than owned state: it is rebuilt every frame from
/// whatever is true, so there is no second copy of the recorder's state to get
/// out of step with the first.
pub struct RecorderView<'a> {
    /// The band's oscilloscope — session state owned by the app; the view
    /// borrows it the way it borrows every string here.
    pub scope: &'a crate::scope::Scope,
    /// The band's spectrum analyzer, which fills the camera pane while no
    /// camera is chosen. Session state owned by the app, like the scope.
    pub analyzer: &'a crate::analyzer::Analyzer,
    /// Where the hand is over the band, in the band's own space, for the
    /// analyzer's readout under the pointer. `None` on the compositor's copy
    /// — a hairline following the cursor is not something to bake into a
    /// recording.
    pub pointer: Option<egui::Pos2>,
    /// How the analyzer spells an accidental: the same preference the staff
    /// and the keys already follow.
    pub prefer_flats: bool,
    pub state: RecordState,
    /// Seconds since the take started writing. Zero in `Idle` and during
    /// pre-roll.
    pub elapsed_s: f64,
    /// The playhead, in seconds from 0:00. What the readout shows in every
    /// state but the count-in: click the waveform at 1:24 and this reads 1:24
    /// before anything is pressed, then counts up from there.
    pub position_s: f64,
    /// The green button's audition is running.
    pub playing: bool,
    /// Armed lanes are being recorded into the arrangement — the red
    /// button's lit state. Not the capture-take's `state`.
    pub arr_recording: bool,
    /// A realtime EXPORT is playing the arrangement into a file. It runs on
    /// the take machine, so `state` alone cannot tell it from a take - and
    /// unlike a take it owns the transport: see `RecorderState::exporting`.
    pub exporting: bool,
    /// The arrangement viewport's `(start_s, len_s)` — the LCD's
    /// Start/End/Length rows are the TIMELINE's, never a clip's, and they
    /// are live whether or not a single clip exists.
    pub view_window: (f64, f64),
    pub meters: Meters,
    /// The OUTPUT's levels — after the effects, after the limiter, after the
    /// master, click included. Not the same signal as `meters`, which is what
    /// is being recorded.
    pub master: Meters,
    /// Decibels the limiter is taking off right now, zero for none.
    pub gr_db: f32,
    /// The output directory, already shortened for display (`~/Movies/Tangent`).
    pub dest: &'a str,
    /// The PROJECT folder and the EXPORT folder, shortened the same way.
    /// Empty strings are legal and read as "the default".
    pub project_dest: &'a str,
    pub export_dest: &'a str,
    /// The PRESET folder, likewise — the MEDIA tier's fourth row.
    pub preset_dest: &'a str,
    /// The IR folder, likewise — the MEDIA tier's fifth row, where the IR
    /// wizard saves and the convolver's shelf reads.
    pub irs_dest: &'a str,
    /// The typed take name. May be empty — the timestamp guarantees uniqueness.
    pub take_name: &'a str,
    /// The name field has keyboard focus, so it draws a caret and the app's
    /// single-letter shortcuts are suppressed.
    pub name_focused: bool,
    /// A numeric control being typed into, if any. Draws in place of that
    /// control's reading, with a caret; suppresses the single-key shortcuts for
    /// the same reason the name field does.
    pub editing: Option<&'a NumEdit>,
    /// What the folder will be called, computed by `tangent_record::take`.
    pub folder_preview: &'a str,
    /// The four faders, as LINEAR gains (not fader positions). See
    /// [`gain_to_fader`] for turning one into a knob angle.
    pub gains: Gains,
    /// The eight effect knobs, 0..=1, as they are drawn.
    pub fx: FxSends,
    /// The varispeed dial. Its own type rather than a bare `f32`, because the
    /// number the dial DRAWS at and the number it MEANS are related by the
    /// range, and only one place should know how.
    pub varispeed: crate::varispeed::Varispeed,
    /// The loudness column: three readings and which of them is showing.
    pub loudness: Loudness,
    /// What each of those numbers MEANS, in [`crate::recorder_panel::Fx::ALL`]
    /// order. Told by the host; see [`crate::ports::KnobUnit`].
    pub fx_units: [crate::ports::KnobUnit; crate::recorder_panel::Fx::ALL.len()],
    /// The backing track: its name, its length and its outline. Borrowed, not
    /// owned — the outline is a thousand floats and the view is built every
    /// frame.
    pub track: &'a crate::ports::TrackInfo,
    /// The arrangement overview the timeline draws. Empty means "draw the
    /// single wave the track info carries" — the plugin's face, and any
    /// host that has not pushed a document.
    pub scrub: ScrubView,
    /// The control a hand is on right now, if any. A knob shows its number
    /// while it is being turned and its name the rest of the time: there is
    /// nowhere on a knob to keep both, and the number is only wanted while it
    /// is changing.
    pub turning: Option<NumField>,
    /// The click is on. Independent of `metronome_in_take`.
    pub metronome_on: bool,
    /// The click is mixed into the recording as well as the monitors.
    ///
    /// **Off by default, and that is the important default in this struct.** A
    /// click bleeding into the take is a ruined take, and it is the mistake
    /// nobody notices until they open the file.
    pub metronome_in_take: bool,
    /// The live input is being heard. Never persisted — see
    /// `TangentApp::input_monitor`.
    pub input_monitor: bool,
    /// What the device send is DOING, for the cog's SEND chip: the host's
    /// status, not the setting (see `ports::DeviceSendStatus::chip`).
    pub device_send: &'a crate::ports::DeviceSendStatus,
    /// Beats and tempo, shared by the click, the count-in and the SMF's tempo
    /// mark — one number, because a click at 90 against a file that says 120
    /// is a take nobody can edit afterwards. **One number at a take's START**:
    /// the knob stays live through a take now, and a tempo moved mid-take
    /// plays at the new tempo while the take's `.mid` keeps the mark it began
    /// with - its event times exact, only its bar lines after the move off.
    pub tempo_bpm: f64,
    /// The take's time signature, shown beside the tempo and the count-in
    /// because the three describe one thing between them.
    pub time_signature: TimeSignature,
    /// Count-in length in BARS — what the cell shows and what a click cycles.
    pub count_in_bars: u32,
    pub count_in_beats: u32,
    pub camera: DeviceLabel<'a>,
    pub audio: DeviceLabel<'a>,
    pub preview: Option<Preview>,
    /// Recording time left on the destination volume, in minutes, at the
    /// current settings. `None` while it is still being measured.
    ///
    /// A duration and not bytes, deliberately: "214 GB free" means nothing to a
    /// pianist and "~58 min" means everything.
    pub disk_minutes: Option<f64>,

    /// The user asked not to see a clock while playing. §5: a running timer is
    /// the second most-cited performance distraction after a blinking light.
    pub hide_elapsed: bool,
    /// One line of status, shown under the transport. Where "recorded 4:12 to
    /// nocturne-2026-08-16-141203" and "no audio input selected" both go.
    pub message: Option<&'a str>,
    /// Set while the last take clipped, so the report after Stop can say so
    /// even after the meter has been reset.
    pub clip_warning: bool,
    /// **The limiter is out of circuit**, so its dial is drawn dark. See
    /// `TangentApp::limiter_disabled`. Appended, the struct's law.
    pub limiter_off: bool,
    /// **The tempo is held**: the knob, TAP and the word are drawn dim and
    /// the app refuses them. See [`RecorderState::tempo_held`]. Appended.
    pub tempo_held: bool,
    /// The count-in beat clicking now while a take is ALREADY writing - see
    /// [`RecorderState::count_beat`]. Appended.
    pub count_beat: Option<u32>,
    /// A Play waiting on a count's downbeat - see
    /// [`RecorderState::play_pending`]. Appended.
    pub play_pending: bool,
}

/// The band timeline's overview of the ARRANGEMENT: every clip as a block
/// against the whole song — Ableton's overview strip at Tangent density —
/// and the arrangement view's window when it is open to steer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrubView {
    /// `(lane, start_frac, len_frac, colour)` per clip, fractions of the
    /// SPAN — the same axis the seek and the playhead already run on. The
    /// colour indexes `mixer_panel::STRIP_COLORS`; zero is the desk's wood
    /// and draws as the timeline's own green.
    pub blocks: Vec<(usize, f32, f32, usize)>,
    /// The arrangement view's window as fractions of the span. Drawn as a
    /// rectangle a hand can pick up and slide.
    pub viewport: Option<(f32, f32)>,
    /// The overview's whole width in SECONDS: the song, or the viewport's
    /// end when the arrangement looks past the last clip — whichever is
    /// longer, so the rectangle is always telling the truth. Zero means
    /// "no document": the wave face falls back to the track's own length.
    pub domain_s: f64,
    /// **How many rows the overview stacks**: the desk's extent, handed in.
    /// It read the desk's compile-time width, which tied a picture of the
    /// song to a capacity constant; the desk says how deep it is now, and
    /// zero (no document) draws one row. Appended.
    pub rows: usize,
}

impl RecorderView<'_> {
    /// Whether the clip warning is on screen — and therefore whether it is
    /// something a press can land on.
    pub fn showing_clip(&self) -> bool {
        self.meters.clipped || self.clip_warning
    }

    /// A view with nothing configured, for tests and for the first frame.
    /// (The resting scope beside it: one static, silent instance for every
    /// view that has no live app to borrow from.)
    pub fn empty() -> RecorderView<'static> {
        RecorderView {
            scope: resting_scope(),
            analyzer: resting_analyzer(),
            pointer: None,
            prefer_flats: false,
            input_monitor: false,
            device_send: &crate::ports::DeviceSendStatus::Off,
            state: RecordState::Idle,
            elapsed_s: 0.0,
            position_s: 0.0,
            playing: false,
            arr_recording: false,
            exporting: false,
            view_window: (0.0, 60.0),
            meters: Meters::SILENT,
            master: Meters::SILENT,
            gr_db: 0.0,
            dest: "",
            project_dest: "",
            export_dest: "",
            preset_dest: "",
            irs_dest: "",
            take_name: "",
            name_focused: false,
            editing: None,
            folder_preview: "",
            gains: Gains::default(),
            fx: FxSends {
                reverb: 0.0,
                delay: 0.0,
                chorus: 0.0,
                phaser: 0.0,
                hpf: 0.0,
                lpf: 1.0,
                gain: 0.0,
                limiter: 0.0,
            },
            fx_units: [crate::ports::KnobUnit::Percent; crate::recorder_panel::Fx::ALL.len()],
            varispeed: crate::varispeed::Varispeed::default(),
            loudness: Loudness::default(),
            track: crate::ports::TrackInfo::NONE,
            scrub: ScrubView::default(),
            turning: None,
            metronome_on: false,
            metronome_in_take: false,
            tempo_bpm: DEFAULT_BPM,
            time_signature: TimeSignature::default(),
            count_in_bars: 1,
            count_in_beats: 4,
            camera: DeviceLabel::None,
            audio: DeviceLabel::None,
            preview: None,
            disk_minutes: None,
            hide_elapsed: false,
            message: None,
            clip_warning: false,
            limiter_off: false,
            tempo_held: false,
            count_beat: None,
            play_pending: false,
        }
    }
}

/// The four things with a volume control, as linear gains.
///
/// Linear rather than dB because that is what the audio path multiplies by;
/// the band converts for display and for the fader's travel. A struct rather
/// than four loose floats so that adding a fifth source is one field and not
/// five call sites.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gains {
    /// One per general channel — the fader of each of the eight, whatever
    /// the channel's kind.
    pub channels: [f32; CHANNELS],
    /// The click. Applies to what you hear; whether it reaches the FILE is
    /// `metronome_in_take`, which is a separate question with a separate
    /// answer.
    pub metronome: f32,
    /// **The master.** Last on the instrument bus, after the limiter, on both
    /// what you hear and what is written. Not the click, which has its own.
    pub master: f32,
    /// The backing track, which rolls with the transport.
    pub track: f32,
    /// What comes back from the effects bus, at its own fader.
    pub fx_return: f32,
}

/// A channel of the desk.
///
/// **Declared here rather than in the host** so that a painter can name one
/// without reaching across the firewall, and converted on the other side by an
/// exhaustive match — which is what makes the two orders provably the same
/// rather than the same by inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Strip {
    /// One general channel, which is MIDI or AUDIO by the user's choice.
    ///
    /// **The instrument/input split died here.** There used to be five
    /// instrument slots and four input strips, two different kinds of column
    /// with two different rule books. A channel is a channel now: its KIND
    /// (see [`ChannelKind`]) decides what feeds it — the note stream into a
    /// bay-1 instrument, or one of the interface's inputs — and its three
    /// bays take any plugin either way. The kind lives in the settings, NOT
    /// in this enum, so cycling a channel between MIDI and AUDIO keeps its
    /// fader, its send, its colour, its name and its inserts.
    ///
    /// **`Track` used to sit here and is retired.** The backing track was a
    /// strip of its own with one hardwired job; the arrangement makes that
    /// job "an audio clip on any lane", so the column went back to being an
    /// ordinary channel. See `docs/ARRANGEMENT.md` and the v13 migration.
    Channel(usize),
    /// **The DRUM BUS** — the drumpad's own channel, persistent, left of
    /// the click. The pads sum through it: its fader, pan, mute/solo, send
    /// and three bays govern the whole kit, and bus-wide effects go HERE,
    /// not in the pad window.
    Drums,
    Click,
    /// The return from the effects bus. It has no send of its own: a bus that
    /// could feed itself is a bus that howls.
    Fx,
}

/// What feeds a general channel, or nothing yet.
///
/// Plain data the settings hold as a number: 0 unused, 1 MIDI, 2 AUDIO. A
/// channel that is `None` here is a column with a plus in it — drawn, so the
/// desk never changes shape, but not yet a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Midi,
    Audio,
}

/// **Which lanes are shut off from LIVE MIDI**, one bit per channel, set = shut
/// off. The engine's `midi_off`.
///
/// Three rules, in order:
///
/// 1. A lane that is not a MIDI channel never hears a note.
/// 2. **A MIDI lane hears the keyboard when it is ARMED, and only then.**
/// 3. **While the MIDI editor is open it is the ONLY lane that hears.** An
///    audition there is a question about the clip in front of you; with two
///    instruments armed, one key played both of them.
///
/// **Rule 2 used to read "armed OR SELECTED", and the owner struck the
/// selection out on 2026-09-07**: "unarmed tracks still make sounds ... that's
/// nonsensical and should be overridden even if it was me". What it looked
/// like from the room is worth writing down, because it is what a report of
/// this will look like next time: an armed track making no sound while an
/// unarmed one sounded and metered, which is arm reading as a lie. Selection
/// is a quiet, sticky thing — every click on a header sets it and nothing
/// visibly says it also opens a MIDI gate — so the lane that answered the
/// keyboard was not the lane wearing the R.
///
/// **What it costs, stated plainly**: trying an instrument out now begins by
/// arming a track, which is the very thing the selection clause was added to
/// avoid — a half-finished take is one keypress from an audition. The editor's
/// own audition (rule 3) is untouched and remains the road that needs no arm.
/// Logic and Cubase both sound the selected instrument track without a record
/// arm, so this desk is now deliberately stricter than they are.
///
/// It is one mask rather than a gate per rule because the mask's EDGE is what
/// releases the notes a silenced lane was holding. A second gate would have no
/// edge of its own and would strand them sounding.
///
/// **Only the CHANNEL bits are ever set.** The engine's `Strip` numbering runs
/// past the channels — drums, click, fx — and those racks read this same mask
/// for their own bit. They have never been gated by it and must not start
/// being: a mask built as "everything off, then open the live ones" reaches
/// them, where the old "nothing off, then close the dead ones" did not, and
/// the click's own effect would have gone deaf to the keyboard.
pub fn live_midi_off(
    kinds: &[Option<ChannelKind>],
    armed: u32,
    editor_lane: Option<usize>,
) -> u32 {
    // Rule 3 short-circuits, but still answers to rule 1: an editor somehow
    // open on an audio lane silences the desk rather than opening it.
    // The channels' bits, and no others. See the note above.
    let all = ((1u64 << CHANNELS.min(32)) - 1) as u32;
    if let Some(lane) = editor_lane {
        return if kinds.get(lane).copied().flatten() == Some(ChannelKind::Midi) {
            all & !(1u32 << lane.min(31))
        } else {
            all
        };
    }
    let mut off = all;
    for ch in 0..CHANNELS.min(32) {
        let live =
            kinds.get(ch).copied().flatten() == Some(ChannelKind::Midi) && armed & (1 << ch) != 0;
        if live {
            off &= !(1u32 << ch);
        }
    }
    off
}

// ── which DEVICES, and which CHANNEL, one track listens to ──────────────────

/// How many distinct MIDI devices one session can tell apart.
///
/// **A bit per device in a `u32`**, which is what makes the routing decision
/// a mask AND — see [`MidiRoute`]. Thirty-two is the same number the desk has
/// channels, chosen for the same reason: it is past what anybody plugs in, and
/// a slot is a few bytes. A thirty-third device opens and is heard by every
/// lane that listens to everything, which is the safe direction to fail in —
/// it plays, rather than being silently deaf.
pub const MIDI_SOURCES: usize = 32;

/// The source slot given to MIDI the APP itself made — a clicked key, the
/// fretboard, a lattice note, the audition. Never a device, and admitted by
/// every lane: those notes already name their lane or are meant for the room,
/// and filtering them by a device set that no app-made note can ever be in
/// would silence the whole app.
pub const MIDI_SRC_APP: u8 = 0xFF;

/// **What one track listens to: a SET of devices and a channel.**
///
/// # The fault this replaces
///
/// The app opened exactly ONE MIDI device, app-wide, chosen from the global
/// menu — while every track's input chip offered a port of its own and wrote
/// `"midi:<port>"` into the settings. Nothing downstream ever read that port.
/// Two tracks could not have separate keyboards, and the chip said they could:
/// the owner's report was "the gui selector ... is lying to the user".
///
/// So a track's MIDI input is two things, and neither of them is one device:
///
/// * `devices` — the device NAMES it listens to. **Empty is EVERY device**,
///   which is the default and today's behaviour, and it stays expressible
///   because it is what most people want. It is also the only form that
///   keeps working when a device nobody has seen yet is plugged in.
/// * `channel` — the wire channel 0..=15, or `None` for **every channel**,
///   likewise the default. Stored on the wire (0-based) and shown 1-based,
///   because that is the split every MIDI spec and every keyboard makes.
///
/// # Names, not indices
///
/// A device is remembered by its NAME and never by its position in the port
/// list, because the list is re-enumerated whenever anything is plugged or
/// unplugged and an index means a different keyboard afterwards. The cost is
/// that two identical controllers on one machine can be indistinguishable —
/// CoreMIDI numbers them ("USB MIDI 1", "USB MIDI 2") and ALSA does too, so
/// in practice they are not, and a name that does collide fails by both being
/// heard rather than by the wrong one being heard.
///
/// A remembered device that is not plugged in right now is KEPT. Dropping it
/// would mean unplugging a keyboard and plugging it back in cost you the
/// whole setup, which is exactly the thing the owner asked for a dialog to
/// stop having to redo.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct MidiInputSel {
    /// The device names this track listens to. **Empty is every device.**
    pub devices: Vec<String>,
    /// The wire channel 0..=15. **`None` is every channel.**
    pub channel: Option<u8>,
}

impl MidiInputSel {
    /// Every device, every channel — the default, and what `"midi:all"` has
    /// always meant.
    pub const EVERYTHING: Self = Self {
        devices: Vec::new(),
        channel: None,
    };

    /// Whether this is the default: every device, every channel. The chip and
    /// the migration both ask, and asking in one place is what stops them
    /// disagreeing about what "all" is.
    pub fn is_everything(&self) -> bool {
        self.devices.is_empty() && self.channel.is_none()
    }

    /// Read the OLD one-device grammar — `"midi:all"` or `"midi:<port>"` —
    /// into the new shape. `None` for anything that is not a MIDI spec.
    ///
    /// **Every old value keeps meaning what it meant**, which for the old
    /// grammar is easy to state exactly: it could only ever name one port and
    /// never a channel, so `"midi:<port>"` becomes the one-element set on
    /// every channel and `"midi:all"` becomes the default.
    pub fn from_legacy(spec: &str) -> Option<Self> {
        match spec.strip_prefix("midi:")? {
            "all" | "" => Some(Self::EVERYTHING),
            port => Some(Self {
                devices: vec![port.to_owned()],
                channel: None,
            }),
        }
    }

    /// Write the OLD one-device grammar back, so that a build without this
    /// feature reads a file this build wrote and finds something TRUE in it.
    ///
    /// The repo's additive-key rule is not only "an old build must not
    /// crash" — it is that the keys an old build understands must still say
    /// what they said. `channel_inputs` is still the authority on a channel's
    /// KIND (see `TangentApp::set_channel_input_spec`), so it is written on
    /// every save from the selection, and the first device is what an old
    /// build would have had: one port, the one it could have opened.
    pub fn legacy_spec(&self) -> String {
        match self.devices.first() {
            None => "midi:all".to_owned(),
            Some(port) => format!("midi:{port}"),
        }
    }

    /// **Does this track want messages from `device` at all?** The device
    /// axis on its own — what decides which ports the app OPENS, where the
    /// channel is not yet known because no message has arrived.
    pub fn wants_device(&self, device: &str) -> bool {
        self.devices.is_empty() || self.devices.iter().any(|d| d == device)
    }

    /// **Does this track hear a message that arrived from `device` on wire
    /// channel `chan`?** The whole routing rule, in one place, so the engine's
    /// live path and the recorder's take path cannot come to disagree about
    /// it — which is the seam two lanes shipped mute through yesterday.
    pub fn accepts(&self, device: &str, chan: u8) -> bool {
        let chan_ok = self.channel.is_none_or(|c| c == chan & 0x0F);
        self.wants_device(device) && chan_ok
    }

    /// The same rule as [`Self::accepts`], compiled against a SLOT TABLE into
    /// two masks the audio thread can test with an `and`.
    ///
    /// `slots[i]` is the device holding source slot `i` — see `MIDI_SOURCES`.
    /// A device this track remembers but which is not in the table is not
    /// plugged in, contributes no bit, and is simply not heard until it comes
    /// back; it is never forgotten.
    ///
    /// An EMPTY device set compiles to every slot rather than to the slots
    /// that happen to be filled now, so a keyboard plugged in a minute from
    /// now is heard without anybody re-deriving anything.
    pub fn route(&self, slots: &[String]) -> MidiRoute {
        let src = if self.devices.is_empty() {
            u32::MAX
        } else {
            let mut m = 0u32;
            for (i, name) in slots.iter().enumerate().take(MIDI_SOURCES) {
                if self.devices.iter().any(|d| d == name) {
                    m |= 1u32 << i;
                }
            }
            m
        };
        let chan = match self.channel {
            None => u32::MAX,
            Some(c) => 1u32 << (c & 0x0F),
        };
        MidiRoute { src, chan }
    }

    /// The remembered devices that are NOT in `present`, in the order they
    /// were chosen. What the chip warns about and the dialog spells out.
    pub fn missing<'a>(&'a self, present: &[String]) -> Vec<&'a str> {
        self.devices
            .iter()
            .filter(|d| !present.iter().any(|p| p == *d))
            .map(String::as_str)
            .collect()
    }

    /// **What the input chip says, and it says what is true.**
    ///
    /// The chip used to print `MIDI: ALL` or `MIDI: <port>`, and both were
    /// wrong at once: `ALL` meant all PORTS while the app had exactly one open,
    /// and `<port>` named a port nothing ever connected to. It also said
    /// nothing about the channel, which is now a choice.
    ///
    /// The grammar, leading with the identity because the chip is CUT to its
    /// width and the tail is what goes:
    ///
    /// * `MIDI: ALL DEVICES` — the default. The axis is named, so the word
    ///   `ALL` can no longer be read as "all channels".
    /// * `MIDI: Keystation 88` — one device, every channel.
    /// * `MIDI: Keystation 88 +2` — three devices; the dialog names the rest.
    /// * `MIDI: Keystation 88 CH 3` — a channel chosen, shown 1-based.
    /// * `MIDI: !Keystation 88` — chosen, and NOT PLUGGED IN right now. The
    ///   `!` is on the chip rather than the device being dropped from the set,
    ///   because unplugging a keyboard must not cost you the setup.
    pub fn chip_text(&self, present: &[String]) -> String {
        let mut out = String::from("MIDI: ");
        if !self.missing(present).is_empty() {
            out.push('!');
        }
        match self.devices.split_first() {
            None => out.push_str("ALL DEVICES"),
            Some((first, rest)) => {
                out.push_str(first);
                if !rest.is_empty() {
                    out.push_str(&format!(" +{}", rest.len()));
                }
            }
        }
        if let Some(c) = self.channel {
            out.push_str(&format!(" CH {}", (c & 0x0F) + 1));
        }
        out
    }

    /// **The same fact in as few characters as it can be told in.**
    ///
    /// For the arrangement header's input BUTTON, which sits beside the lane
    /// name and has about a quarter of the room [`Self::chip_text`] was
    /// written for. The two things a glance needs are that this is MIDI and
    /// which channel it is narrowed to; the device names are what the mixer
    /// strip, the button's own dialog and the screen-reader node are for.
    ///
    /// The `!` keeps its meaning — a device this track was told to listen to
    /// is not plugged in — because that is the one fact whose absence is
    /// silent, and a mark nobody can miss is worth two characters anywhere.
    /// A `+` says there are device names not shown.
    pub fn short_text(&self, present: &[String]) -> String {
        let mut out = String::from("MIDI");
        if !self.missing(present).is_empty() {
            out.push('!');
        } else if !self.devices.is_empty() {
            out.push('+');
        }
        if let Some(c) = self.channel {
            out.push_str(&format!(" {}", (c & 0x0F) + 1));
        }
        out
    }
}

/// **Read the persisted device sets**, from either the settings file or a
/// project document — one reader, because two would be two answers to what a
/// stored `channel` of 99 means.
///
/// The shape is a SPARSE object keyed by channel index, the `strip_inserts`
/// tail's shape and for its reason: thirty-two rows that nearly all say
/// "everything" would be a hundred near-empty lines in every settings file
/// and in every DAW project the plugin saves. Inside a row, `devices` absent
/// means every device and `channel` absent means every channel — **absent
/// rather than null**, which is the rule `custom_font_path` set: a key
/// written as null is a key an older build has to know to ignore.
pub fn read_midi_inputs(stored: Option<&Value>) -> Vec<MidiInputSel> {
    let mut out: Vec<MidiInputSel> = Vec::new();
    let Some(Value::Object(o)) = stored else {
        return out;
    };
    for (k, v) in o {
        let Ok(ch) = k.parse::<usize>() else { continue };
        if ch >= CHANNELS {
            continue;
        }
        let Some(row) = v.as_object() else { continue };
        let devices = row
            .get("devices")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .filter(|d| !d.is_empty())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        // An out-of-range channel is DROPPED rather than wrapped: a
        // hand-edited 99 that became channel 3 would silently route
        // somebody's keyboard to a track they never named.
        let channel = row
            .get("channel")
            .and_then(Value::as_u64)
            .filter(|c| *c < 16)
            .map(|c| c as u8);
        while out.len() <= ch {
            out.push(MidiInputSel::EVERYTHING);
        }
        out[ch] = MidiInputSel { devices, channel };
    }
    out
}

/// The inverse of [`read_midi_inputs`]. `None` when there is nothing to say,
/// so the key is ABSENT rather than an empty object on every desk nobody has
/// narrowed.
pub fn write_midi_inputs(sels: &[MidiInputSel]) -> Option<Value> {
    let rows: Map<String, Value> = sels
        .iter()
        .enumerate()
        .filter(|(_, sel)| !sel.is_everything())
        .map(|(n, sel)| {
            let mut row = Map::new();
            if !sel.devices.is_empty() {
                row.insert(
                    "devices".into(),
                    Value::Array(sel.devices.iter().map(|d| Value::String(d.clone())).collect()),
                );
            }
            if let Some(c) = sel.channel {
                row.insert("channel".into(), Value::Number(u64::from(c).into()));
            }
            (n.to_string(), Value::Object(row))
        })
        .collect();
    (!rows.is_empty()).then(|| Value::Object(rows))
}

/// **Make a set of device selections agree with the old one-device strings,
/// letting the string an OLDER build actually wrote win.**
///
/// Both places that persist a desk — the settings file and the project
/// document — hold `channel_inputs` (the old grammar, still the authority on
/// a channel's KIND) beside the sets, and both can be written by a build that
/// understands only the first. Such a build rewrites the strings from its own
/// one-device menu and carries the sets through untouched as a stranger's
/// key, so after a downgrade round-trip the two can contradict each other and
/// the one that was actually edited has to win.
///
/// The rule: a string that CONTRADICTS the set replaces its device list.
/// `"midi:all"` against a narrowed set is somebody choosing every port on the
/// old build; a named port that is not in the set is somebody choosing a
/// keyboard this build never knew about. A named port that IS in the set
/// contradicts nothing — the old build could only ever name one of them — so
/// the set stands, which is what stops a downgrade that touched nothing from
/// quietly costing somebody their setup.
///
/// **The CHANNEL survives a contradiction.** A build without this feature has
/// no way to express a channel, so it cannot have meant to clear one; and the
/// chip prints `CH n`, so a filter that is standing is a filter you can see.
///
/// A channel whose string is not a MIDI spec at all is reset to the default:
/// anything stored for an audio or unbound channel is meaningless.
pub fn reconcile_midi_inputs(sels: &mut Vec<MidiInputSel>, legacy: &[String]) {
    for (ch, spec) in legacy.iter().enumerate() {
        let Some(from_legacy) = MidiInputSel::from_legacy(spec) else {
            if let Some(slot) = sels.get_mut(ch) {
                *slot = MidiInputSel::EVERYTHING;
            }
            continue;
        };
        while sels.len() <= ch {
            sels.push(MidiInputSel::EVERYTHING);
        }
        let held = &mut sels[ch];
        let contradicts = match from_legacy.devices.first() {
            None => !held.devices.is_empty(),
            Some(port) => !held.devices.iter().any(|d| d == port),
        };
        if contradicts {
            held.devices = from_legacy.devices;
        }
    }
    trim_midi_inputs(sels);
}

/// Drop trailing defaults, so a desk nobody has narrowed holds an EMPTY
/// vector — writes no key, and compares equal to a fresh one. Interior
/// defaults stay: the vector is indexed by channel.
pub fn trim_midi_inputs(sels: &mut Vec<MidiInputSel>) {
    while sels.last().is_some_and(MidiInputSel::is_everything) {
        sels.pop();
    }
}

/// One lane's routing rule as the audio thread holds it: two masks, tested
/// with an `and`.
///
/// Pushed whole per frame like `midi_off` and the arm mask, for the same
/// reason: a rule assembled a bit at a time in the middle of a block is a rule
/// that can be half-applied to one note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiRoute {
    /// One bit per source slot; set = this lane hears that device.
    pub src: u32,
    /// One bit per wire channel 0..=15; set = this lane hears that channel.
    pub chan: u32,
}

impl MidiRoute {
    /// Every device, every channel — the desk as it has always been, and what
    /// every lane holds until somebody narrows it.
    pub const EVERYTHING: Self = Self {
        src: u32::MAX,
        chan: u32::MAX,
    };

    /// Whether a message from source slot `src` on wire channel `chan` is
    /// admitted. [`MIDI_SRC_APP`] is admitted unconditionally — see the
    /// constant.
    pub fn admits(self, src: u8, chan: u8) -> bool {
        if src == MIDI_SRC_APP {
            return true;
        }
        let sbit = 1u32 << usize::from(src).min(MIDI_SOURCES - 1);
        let cbit = 1u32 << (chan & 0x0F);
        self.src & sbit != 0 && self.chan & cbit != 0
    }
}

impl Default for MidiRoute {
    fn default() -> Self {
        Self::EVERYTHING
    }
}

/// **Every lane's routing rule, compiled from the desk.**
///
/// The sibling of [`live_midi_off`], and deliberately separate from it: that
/// one answers "is this lane listening at all" from arm and the editor, this
/// one answers "to WHAT" from the track's own choice. Two questions, two
/// masks, and a lane hears a message only when both say yes — which keeps the
/// arm rule the one place arming is decided, rather than folding a device set
/// into it and having a re-plug look like a disarm.
///
/// A lane that is not a MIDI channel gets [`MidiRoute::EVERYTHING`] rather than
/// nothing: an AUDIO channel names an audio input, not a device set, and its
/// rack's MIDI-listening effects have always heard the whole room. Narrowing
/// them here would be a silent behaviour change nobody asked for, and
/// `live_midi_off` already shuts an audio lane's live MIDI off.
pub fn live_midi_routes(
    sels: &[MidiInputSel],
    kinds: &[Option<ChannelKind>],
    slots: &[String],
) -> [MidiRoute; CHANNELS] {
    std::array::from_fn(|ch| {
        if kinds.get(ch).copied().flatten() != Some(ChannelKind::Midi) {
            return MidiRoute::EVERYTHING;
        }
        sels.get(ch)
            .map_or(MidiRoute::EVERYTHING, |s| s.route(slots))
    })
}

/// **Every device any track asks for, deduplicated, in desk order.**
///
/// What the app has to have OPEN. A track listening to everything (the empty
/// set) asks for nothing by name and is served by whatever is open — which is
/// why the caller adds the auto-connect pick when this comes back empty: a
/// fresh desk names no device and must still hear the keyboard.
///
/// Only MIDI channels are asked. An audio channel's spec names an interface
/// input and would put a nonsense name in the open set.
pub fn wanted_midi_devices(sels: &[MidiInputSel], kinds: &[Option<ChannelKind>]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ch in 0..CHANNELS {
        if kinds.get(ch).copied().flatten() != Some(ChannelKind::Midi) {
            continue;
        }
        let Some(sel) = sels.get(ch) else { continue };
        for d in &sel.devices {
            if !out.iter().any(|o| o == d) {
                out.push(d.clone());
            }
        }
    }
    out
}

impl ChannelKind {
    /// The settings byte, and back. See `Settings::channel_kinds`.
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            1 => Some(ChannelKind::Midi),
            2 => Some(ChannelKind::Audio),
            _ => None,
        }
    }

    pub fn as_u8(kind: Option<Self>) -> u8 {
        match kind {
            None => 0,
            Some(ChannelKind::Midi) => 1,
            Some(ChannelKind::Audio) => 2,
        }
    }

    /// The other one — the icon's click.
    pub fn cycled(self) -> Self {
        match self {
            ChannelKind::Midi => ChannelKind::Audio,
            ChannelKind::Audio => ChannelKind::Midi,
        }
    }
}

// **Why there is no comping, and no take lanes.** The position was set when
// the recorder was designed (RECORDER-PLAN.md §13) and it is kept on purpose:
// THE TAKE IS WHAT WAS PLAYED. One gesture records the moment — audio, MIDI
// and video together — and keeps it whole; assembling a performance out of
// the best bars of seven passes is a different program, with an editing
// timeline this one deliberately does not grow. Play it again instead. Like
// the take's own shape, this is a limit you can feel the shape of, not a
// feature that has not arrived yet.

/// How many general channels this build's desk can CARRY today - the
/// capacity the engine tables, settings arrays and seat indices are still
/// sized to.
///
/// **The principle this number serves is the reverse of the one it was
/// written for** (docs/design/unlimited-tracks.md, section 2):
///
/// The desk is the list of tracks that exist, plus one place to add the
/// next. It grows by appending and the machine is its only limit: no part
/// of its shape is a compile-time number. A licence decides how many tracks
/// an install may CREATE: thirty-two free, no limit with a supporter key or
/// a certain code. It never hides, silences or truncates a track that
/// exists: a project with more tracks than this install may create opens
/// whole, plays whole and saves whole, and only the next new track is
/// refused. Buses, returns, sends and routes are unlimited on every tier and
/// are not tracks. Every track, bus and plugin seat has an identity that
/// does not depend on where it sits, so adding, removing or reordering
/// tracks renumbers nothing on disk and moves no plugin.
///
/// Until that lands this constant is the build's CEILING, not a tier: the
/// free tier is [`FREE_TRACKS`], asked through [`track_cap`], and the two
/// happen to agree at thirty-two. A supporter who reaches it is told so
/// (`TrackRefusal::Ceiling`). It is deleted when the desk becomes a list;
/// no wider interim value is ever built, because `Shared` and the
/// stack-held engine tables stop fitting near thirty-five lanes.
pub const CHANNELS: usize = 32;

/// **How many tracks a free install may CREATE: thirty-two.**
///
/// A count of tracks that exist, never a width: a desk that already holds
/// more (a supporter's project on another machine, a relocked cheater's
/// session) opens, plays and saves whole, and only the next new track is
/// refused. What counts is `TangentApp::counted_tracks`; buses, returns and
/// sends never do. It was sixteen columns of a thirty-two-wide desk until
/// the owner made the free desk thirty-two.
pub const FREE_TRACKS: usize = 32;

/// **The creation cap for an install of this standing**: `None` for a
/// supporter (a key that covers this major, or the Konami marker), which
/// has no track limit, and [`FREE_TRACKS`] otherwise.
///
/// Takes the standing as a bool on purpose: the one caller asks
/// `LicenseStore::is_supporter`, and a function that cannot see the store
/// cannot ask it the wrong question (`licensed()`, the "may the code
/// TOGGLE" question, is not this one).
pub fn track_cap(supporter: bool) -> Option<usize> {
    if supporter { None } else { Some(FREE_TRACKS) }
}

/// **How many strips the desk shows by default on a window with room for
/// them**, the ceiling `default_mixer_visible` hands its ladder.
///
/// A geometry number, not a tier: it was `FREE_CHANNELS`, which made the
/// default zoom move whenever the free desk did. Sixteen is the owner's
/// "make the default at full screen or max size 16 in one row".
pub const DEFAULT_MAX_STRIPS: usize = 16;

/// **The most strips or lanes either view lays out at once**, whatever the
/// zoom asks for: the clamp on the mixer's strip count and the arrangement's
/// lane count, and the top rung of their zoom ladder.
///
/// A geometry number, not a width. It was the channel count, which tied how
/// many strips a screen can carry to how many tracks the desk holds: a
/// desk that grows past it now scrolls instead, and a view never builds,
/// lays out, hit-tests or describes more than this many plus its pinned
/// strips (docs/design/unlimited-tracks.md, section 10). Sixty-four because
/// that is a full-width desk on the largest screen the owner uses at the
/// narrowest strip that still reads (`mixer_panel::MIN_STRIP_W`).
pub const VISIBLE_MAX: usize = 64;

/// **The palette's period**: how many colours a palette deals before it
/// repeats. Track `n` wears slot `n % PALETTE_SLOTS`, so tracks 0..31 keep
/// the colours they always had and track 32 starts the deal again rather
/// than reading past the table. Thirty-two because every palette in
/// `mixer_panel::PALETTES` was built thirty-two deep.
pub const PALETTE_SLOTS: usize = 32;

/// **Where the limiter's threshold ships**, as a knob position.
///
/// `limiter_mix` is 0..=1 across `effects::LIMITER_DB` = (−48, 0) dB, so
/// 47/48 is a −1 dBFS ceiling.
///
/// **In circuit, not bypassed.** It shipped fully clockwise, which the
/// engine treats as genuinely out — so nothing stood between the master and
/// full scale. That was defensible while every effect on the desk decayed;
/// the tape echo's Intensity reaches self-oscillation now, by design and
/// after the hardware, and a control that can run away wants a backstop that
/// ships on.
///
/// −1 dBFS rather than 0: it is the delivery ceiling every streaming service
/// asks for, it is where a true-peak limiter is useful rather than nominal,
/// and it takes nothing off a mix that was not already at the top.
///
/// **One constant, read by the settings default AND by the knob's own
/// reset**, so a double-click cannot quietly put the guard rail back in the
/// drawer — which is exactly what "reset to not applied" would have done.
pub const LIMITER_SHIPS_AT: f32 = 47.0 / 48.0;

/// How many effect inserts a channel has room for.
///
/// **Three, on every channel, and the number never changes.** A desk where one
/// channel can hold nine and another two is a desk you have to remember the
/// rules of; three everywhere is a shape you can see. It is also artistically
/// limiting on purpose — this started as a MIDI monitor, and a rack that grows
/// until the machine gives out is a different program.
///
/// Thirty-six across the desk if every one is filled. That is deliberately
/// reachable: anybody who fills thirty-six will find out what their hardware
/// can do, which is a thing worth learning from the app rather than from a
/// number this app made up.
pub const INSERTS: usize = 3;

/// How many inputs of one interface the desk has room for.
///
/// **`tangent_record::audio::MAX_PICKS`'s twin**, the same number counted
/// where the capture happens. This crate cannot reach that one (see the
/// firewall), so the host asserts they agree. A literal rather than
/// `CHANNELS`, because an input is a property of the interface and not of
/// the desk: the two only ever agreed by accident, and the day the desk
/// stops being a fixed width this must not move with it. Both go when the
/// picks become one per bound track.
pub const INPUTS: usize = 32;

/// How many channels the desk has, master aside.
pub const STRIPS: usize = CHANNELS + 3;

/// The master's own seat in the insert space. Seats `0..STRIPS` are the
/// channels and the buses; this is the one after them.
pub const MASTER_SEAT: usize = STRIPS;

/// **The master's SECOND rack.**
///
/// A seat of its own, past the pads, holding the master's extra three bays.
///
/// **Why a seat and not six bays on one.** `INSERTS` is the stride of the
/// persisted `strip_inserts` array and of the `insert-state-<seat>-<bay>.bin`
/// sidecars; raising it to six would change the stride for every strip on
/// the desk, need a migration for every saved session, and hand the
/// arrangement's track headers six bays they have no room to draw. An extra
/// seat is three more entries appended to an array that already grows by
/// appending, one more rack in the engine, and no other file learns
/// anything.
pub const MASTER_EXTRA_SEAT: usize = STRIPS + 1 + DRUM_PADS;

/// **How many bays a strip shows: six on the master and on every CHANNEL.**
///
/// It used to be three on a channel, six on the master, and six on a channel
/// only while that channel happened to be a FOLDER — so the size of a track's
/// rack changed under it when it was made a group and changed back when it
/// stopped being one, and three plugins had to go somewhere when it did. The
/// owner's ruling: "ALL tracks and folders now have 6 fx slots."
///
/// It costs nothing in the seat map. `seat_bay` has always answered slots
/// 3..5 for any channel out of that channel's folder-extra seat, folder or
/// not; the flag was only ever gating what got DRAWN. The two buses — the
/// click and the effects return — keep their three: they are not tracks, and
/// neither has ever had a second rack to inherit.
///
/// `folder` is no longer asked, so there is one function again rather than a
/// pair with a flag the answer does not depend on.
pub fn bays_on(strip: Option<Strip>) -> usize {
    // **Six on the CLICK too**, now that it has a seat for them
    // (`BUS_EXTRA_SEATS`) and the engine runs it: the owner's "make the click
    // have 6 fx slots so it matches the layout of the other tracks". The
    // other two buses have no column on the desk, so nobody can fill a rack
    // they would be given — they keep three until they do.
    if matches!(strip, None | Some(Strip::Channel(_) | Strip::Click)) {
        INSERTS * 2
    } else {
        INSERTS
    }
}

/// **How many bays the strip at a DESK index shows.** [`bays_on`] keyed by
/// index rather than by `Strip`, for the callers that hold one — the master
/// is `STRIPS` and is not in `Strip::all()`, which is the convention
/// [`Column::desk_index`] already writes down.
pub fn bays_at(desk_index: usize) -> usize {
    bays_on(Strip::all().get(desk_index).copied())
}

/// **Which rack seat and which bay a strip's Nth slot really is.**
///
/// Everything but the master answers itself. The master's first three slots
/// are its EXTRA rack and its last three are its original one, and that
/// order is deliberate: the new bays were added above the old ones on the
/// strip, a rack runs top to bottom, and a session that already had plugins
/// in the master must keep hearing them in the same place in the chain.
///
/// One function, because the alternative is every call site remembering the
/// rule — and a call site that forgets it edits somebody else's plugin.
pub fn seat_bay(desk_index: usize, slot: usize) -> (usize, usize) {
    if desk_index == MASTER_SEAT && slot >= INSERTS {
        (MASTER_SEAT, slot - INSERTS)
    } else if desk_index == MASTER_SEAT {
        (MASTER_EXTRA_SEAT, slot)
    } else if desk_index < CHANNELS && slot >= INSERTS {
        // A FOLDER's slots 3..5 - and, unlike the master's, in the plain
        // order: the extra rack goes BELOW the original, so slot and bay
        // numbering agree with the automation's and no `master_bay` twin
        // is needed. See `FOLDER_EXTRA_SEATS`.
        (folder_extra_seat(desk_index), slot - INSERTS)
    } else if desk_index < STRIPS && slot >= INSERTS {
        // A BUS's slots 3..5, in the same plain order a channel's take. See
        // `BUS_EXTRA_SEATS`.
        (bus_extra_seat(desk_index), slot - INSERTS)
    } else {
        (desk_index, slot)
    }
}

/// **Which STRIP a rack seat belongs to**, and the exact inverse of
/// [`seat_bay`] in the only respect anybody asks about: the desk index whose
/// slots live in that seat.
///
/// [`seat_bay`] is a one-way street and was enough while the only question
/// anybody asked of a bay was where to read it. The question a DRAG asks is
/// different — "is this the same strip I picked it up from" — and in seat
/// space the answer was wrong for exactly the bays that live in a second
/// rack: a channel's slots 0..2 are its own seat and its 3..5 are its
/// folder-extra one, so carrying a plugin from slot 1 to slot 5 of ONE track
/// read as a move between two tracks and copied it instead of moving it.
/// That is the owner's "drag only works between each group of 3".
///
/// A seat that is nobody's rack — a drum pad, an amp pedal — answers `None`.
/// Those are racks a strip OWNS but not racks a strip SHOWS, and a carry
/// that is not between two shown bays is not a carry this answers for.
pub fn rack_owner(seat: usize) -> Option<usize> {
    if seat <= MASTER_SEAT {
        // The channels, the three buses, and the master's own seat.
        return Some(seat);
    }
    if seat == MASTER_EXTRA_SEAT {
        return Some(MASTER_SEAT);
    }
    if let Some(ch) = folder_extra_of(seat) {
        return Some(ch);
    }
    if let Some(bus) = bus_extra_of(seat) {
        return Some(bus);
    }
    None
}

/// The BUS whose extra seat this is, if it is one — [`bus_extra_seat`]'s
/// inverse, and [`folder_extra_of`]'s sibling. See [`rack_owner`], which is
/// the question both of them exist to answer.
pub fn bus_extra_of(seat: usize) -> Option<usize> {
    seat.checked_sub(BUS_EXTRA_SEATS)
        .filter(|i| *i < BUSES)
        .map(|i| CHANNELS + i)
}

/// **Where the pads of drum lanes past the FIRST block begin.**
///
/// A drum instrument loads a full instance per channel, so every channel
/// that carries a drumpad needs its own hundred and twenty-eight pad racks.
/// They are APPENDED, after everything that already had a seat — the note on
/// `MASTER_EXTRA_SEAT` says why that is the safe direction: an appended seat
/// changes no index anybody has already written down, and this array grows
/// by appending.
///
/// Channel 0 keeps the ORIGINAL pad block at `STRIPS + 1`. That is what a
/// project written before lanes existed used, and `Settings::from_json`
/// moves it when such a project's drumpad turns out to be on another
/// channel. See `pad_rack_index`.
pub const EXTRA_PAD_SEATS: usize = MASTER_EXTRA_SEAT + 1;

/// **Where the FOLDER extra seats begin** - one appended seat per channel,
/// so a group can wear six bays the way the master does. Appended after
/// every pad block, for the reason `MASTER_EXTRA_SEAT` records: an appended
/// seat changes no index anybody has already written down.
///
/// Unlike the master's, a folder's extra bays sit BELOW its original three.
/// There was no legacy strip to keep the look of, so display order,
/// automation-bay order and chain order all agree here: slots and bays 0..2
/// are the channel's own seat, 3..5 the extra one.
pub const FOLDER_EXTRA_SEATS: usize = EXTRA_PAD_SEATS + (CHANNELS - 1) * DRUM_PADS;

/// Channel `ch`'s folder-extra seat. One function, same reason as
/// `pad_rack_index`: a call site that recomputes this edits somebody
/// else's plugin.
pub const fn folder_extra_seat(ch: usize) -> usize {
    FOLDER_EXTRA_SEATS + ch
}

/// The inverse: which channel's extra rack a seat is, or `None` for a seat
/// outside the folder-extra block.
///
/// It did not exist while nothing needed to travel BACK. `folder_extra_seat`
/// is a one-way street — every reader either knew the channel already or was
/// content to ask about a seat it had just built — and the price of that was
/// the pedalboard: a folder wears six bays, three of them seated past every
/// pad block, and the board that arms behind an amp is the CHANNEL's. With no
/// way home from the seat, the amp face in a folder's slots 3..5 could only
/// say "on channel strips only", and the engine could only run the board for
/// a lane whose own rack held the amp. Both were answering a question about
/// the seat when the question was about the channel. See [`pedal_owner`],
/// which is the question they should have been asking.
pub fn folder_extra_of(seat: usize) -> Option<usize> {
    seat.checked_sub(FOLDER_EXTRA_SEATS)
        .filter(|ch| *ch < CHANNELS)
}

/// How many pedal seats each channel owns for the amp's pedalboard.
///
/// Seven, the owner's board: comp, wah, drive, chorus, trem, echo, plate —
/// enough for the classic order with nothing doubled. A constant rather
/// than a growable count because the seat space is append-only: growing it
/// later is a new block after whatever is last by then, never a renumber
/// of these.
pub const AMP_PEDALS: usize = 7;

/// **The default pedalboard**, in chain order: dynamics first, then the
/// filters and drive, modulation, time, and the room last - the order a
/// player wires a real board, seeded so the amp arrives dressed rather
/// than as seven pluses.
///
/// **Dressed and switched OFF.** Every one of these lands BYPASSED - see
/// the app's `seed_amp_board`, which is the other half of this table and
/// the only place that writes it.
pub const AMP_DEFAULT_BOARD: [&str; AMP_PEDALS] = [
    "@tangent-comp",
    "@tangent-wah",
    "@tangent-drive",
    "@tangent-chorus",
    "@tangent-trem",
    "@tangent-echo",
    "@tangent-plate",
];

/// **Where the amp-pedal seats begin** - seven appended seats per channel,
/// the pedalboard the amp wears when it sits in a channel bay. Appended
/// after the folder-extra block, for the reason `MASTER_EXTRA_SEAT`
/// records: an appended seat changes no index anybody has already written
/// down, the settings reader re-pads `strip_inserts` to the grown
/// `INSERT_SPACE`, and the state sidecars are named by seat index going
/// forward.
///
/// A pedal seat is an ordinary rack - VST3s are legal tenants - that the
/// engine runs IN ORDER over the amp's input, before the amp's own bay.
/// Only a CHANNEL's amp gets them: an amp on the master, a bus or a
/// folder's extra rack plays bare in v1.
pub const AMP_PEDAL_SEATS: usize = FOLDER_EXTRA_SEATS + CHANNELS;

/// Channel `ch`'s pedal `pedal` seat. One function, same reason as
/// `pad_rack_index`: a call site that recomputes this edits somebody
/// else's plugin.
pub const fn amp_pedal_seat(ch: usize, pedal: usize) -> usize {
    AMP_PEDAL_SEATS + ch * AMP_PEDALS + pedal
}

/// The inverse: which channel's which pedal a seat is, or `None` for a
/// seat outside the pedal block. What lets a reorder carry a picker aimed
/// at a pedal without the call site re-deriving the block's shape.
pub fn amp_pedal_of(seat: usize) -> Option<(usize, usize)> {
    if seat < AMP_PEDAL_SEATS {
        return None;
    }
    let at = seat - AMP_PEDAL_SEATS;
    let ch = at / AMP_PEDALS;
    (ch < CHANNELS).then_some((ch, at % AMP_PEDALS))
}

/// **Whose pedalboard a rack seat wears** — the one question every part of
/// the amp's board should be asking, and the one none of them used to.
///
/// A channel answers itself. A folder's EXTRA seat answers the channel that
/// owns it, because a folder is a channel with six bays and a channel has
/// exactly one board (`AMP_PEDALS` seats, appended once per channel). The
/// master, the buses, the pads and the pedal seats themselves answer `None`:
/// there is no board there and never was.
///
/// **Why one function rather than the test each caller was making.** Three
/// places asked `seat < CHANNELS` and meant "does this seat get a board" —
/// the amp face's chip row, the picker the chips open, and the
/// default-board seeding — and the engine asked a fourth version of it by
/// running the board only in the lane pass, which a folder never reaches.
/// All four agreed, and all four were wrong the same way: an amp dropped
/// into a folder's second rack drew no board, and an amp in a folder's FIRST
/// rack drew a board that made no sound. Answering it in one place is the
/// argument `pad_rack_index` and `amp_pedal_seat` already make on this
/// module — a call site that recomputes the seat map edits somebody else's
/// plugin — with the sharper version for this map: a call site that
/// recomputes it here does not edit the wrong plugin, it silently declines
/// to run the right one.
///
/// A folder's two racks therefore SHARE one board. That is the v1 answer to
/// "a channel owns one board and a folder wants it twice", and it is written
/// here rather than left to the caller: the board runs once, at the top of
/// the fold, over the summed group — see the engine's `run_pedalboard`.
pub fn pedal_owner(seat: usize) -> Option<usize> {
    if seat < CHANNELS {
        return Some(seat);
    }
    folder_extra_of(seat)
}

/// **Where the BUSES' extra bays begin** — one appended seat per bus, so a
/// bus can wear six the way a channel and the master already do.
///
/// The CLICK is the one bus with a column on the desk, and the owner asked
/// for it to match a track: "make the click have 6 fx slots so it matches the
/// layout of the other tracks". The other two are mapped anyway rather than
/// special-cased, because a seat map with a hole in it is a hole somebody
/// finds by loading a plugin into it.
///
/// Appended, for the reason `MASTER_EXTRA_SEAT` records at length: an
/// appended seat changes no index anybody has already written down, and this
/// array grows by appending.
pub const BUS_EXTRA_SEATS: usize = AMP_PEDAL_SEATS + CHANNELS * AMP_PEDALS;

/// The number of buses — every strip past the channels. Three: the drum bus,
/// the click and the effects return.
pub const BUSES: usize = STRIPS - CHANNELS;

/// Bus `desk_index`'s extra seat. `desk_index` is a DESK index in
/// `Strip::index` space, so `CHANNELS..STRIPS`; anything else is not a bus
/// and gets the first seat, which is the same "answer rather than panic"
/// `pad_rack_index` and its neighbours make.
pub const fn bus_extra_seat(desk_index: usize) -> usize {
    if desk_index < CHANNELS || desk_index >= STRIPS {
        return BUS_EXTRA_SEATS;
    }
    BUS_EXTRA_SEATS + (desk_index - CHANNELS)
}

/// Every rack seat: strips, master, the first pad block, the master's
/// extra, a pad block for each channel after the first, a folder-extra
/// seat per channel, seven pedal seats per channel, and one extra seat per
/// bus.
pub const TOTAL_SEATS: usize = BUS_EXTRA_SEATS + BUSES;

/// **The channel permutation lifted onto the WHOLE seat space.** Channel
/// rows move, each folder-extra seat follows its channel, and every other
/// seat - buses, master, pads, the master's extra - stays put, per the
/// reorder rule written on the desktop's permute machinery. `order[new] =
/// old`, and the same here.
pub fn seat_order(order: &[usize]) -> Vec<usize> {
    let mut full: Vec<usize> = (0..TOTAL_SEATS).collect();
    for (new, old) in order.iter().enumerate() {
        if new < CHANNELS && *old < CHANNELS {
            full[new] = *old;
            full[folder_extra_seat(new)] = folder_extra_seat(*old);
            // The pedalboard travels with its channel too, exactly as the
            // folder-extra seat does: a reorder that left the pedals behind
            // would hand one channel's board to whoever took its row.
            for pedal in 0..AMP_PEDALS {
                full[amp_pedal_seat(new, pedal)] = amp_pedal_seat(*old, pedal);
            }
        } else if new != *old {
            // A SEAT-space entry - the drumpad's pad-FX swap permutes pad
            // seats directly - passes through untouched. Dropping these to
            // identity silently parted two pads' plugins from their state
            // files on every swap.
            if let (Some(slot), true) = (full.get_mut(new), *old < TOTAL_SEATS) {
                *slot = *old;
            }
        }
    }
    full
}

/// **Where channel `ch`'s pad `pad` keeps its bays.**
///
/// One function, because the alternative is every call site remembering
/// that channel 0 is the odd one out — and a call site that forgets edits
/// somebody else's plugin.
pub const fn pad_rack_index(ch: usize, pad: usize) -> usize {
    if ch == 0 {
        STRIPS + 1 + pad
    } else {
        EXTRA_PAD_SEATS + (ch - 1) * DRUM_PADS + pad
    }
}

/// How long the whole insert space is: every strip, the master, every pad of
/// every channel, and the master's second rack.
pub const INSERT_SPACE: usize = TOTAL_SEATS * INSERTS;

// ── The drumpad ─────────────────────────────────────────────────────────────

/// How many pads the drumpad addresses: EVERY MIDI key. Pad `n` IS note
/// `n` — the 4×4 window the panel shows is a PAGE over this space, stepped
/// by the bank band on its left. The engine mirrors this the way it
/// mirrors [`CHANNELS`].
pub const DRUM_PADS: usize = 128;

/// One visible PAGE of the pads: the 4×4 the hands know.
pub const DRUM_PAGE: usize = 16;

/// Where the pages anchor. Pages start at `4 + 16k`, so the classic kit
/// range (36..51, the old sixteen) sits WHOLE on one page — the owner's
/// reference screenshot, exactly. Keys 0..3 (C-1..D#-1) fall before the
/// first page and stay reachable by MIDI alone; the last page pads out
/// past 127 with blanks to keep the graph neat.
pub const DRUM_PAGE_ANCHOR: usize = 4;

/// How many pages the band offers: bases 4, 20, …, 116.
pub const DRUM_PAGES: usize = 8;

/// Where pad `i`'s insert bays live in the strip-indexed insert space:
/// after every strip and the master. One seat map, mirrored by the engine.
pub const fn pad_insert_index(pad: usize) -> usize {
    STRIPS + 1 + pad
}

/// The MIDI note of pad 0. ZERO now — pad `n` answers note `n`, the whole
/// keyboard. The classic kit's C1 home (36) is where the default PAGE
/// opens instead; see [`DRUM_PAGE_ANCHOR`].
pub const DRUM_PAD_BASE: u8 = 0;

/// How hard a mouse-struck pad hits. Firm — a pad tapped with a pointer is a
/// performance gesture, not an audition — but short of the top, for the same
/// reason [`AUDITION_VELOCITY`] stops short of it.
pub const DRUM_PAD_VELOCITY: u8 = 112;

/// One pad's knobs, as the engine wants them: plain numbers, pushed whole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrumPadParams {
    /// Linear gain, 0..=2. 1.0 is the sample as it came.
    pub gain: f32,
    /// -1 (hard left) ..= 1 (hard right), 0 centre.
    pub pan: f32,
    /// Semitones, -12..=12, applied when a hit starts. Fractional is legal.
    pub tune: f32,
    /// 0 = no choke; pads sharing a non-zero group cut each other, which is
    /// what lets a closed hat end an open one.
    pub choke: u8,
    /// Whether the pad OBEYS NOTE-OFFS: on, a release fades the voice out
    /// instead of letting the one-shot ring. Off by default, per the owner.
    pub obey: bool,
    /// The sample's playable window, fractions of its length. 0..1 whole.
    pub trim_start: f32,
    pub trim_end: f32,
}

impl Default for DrumPadParams {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            tune: 0.0,
            choke: 0,
            obey: false,
            trim_start: 0.0,
            trim_end: 1.0,
        }
    }
}

/// The whole kit's controls, pushed to the engine every frame like the desk:
/// writing an unchanged value costs nothing, and a change-detection cache
/// would be a second copy to get out of step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrumKitParams {
    pub pads: [DrumPadParams; DRUM_PADS],
    /// The kit's own fader, applied to the pads' sum.
    pub master: f32,
}

impl Default for DrumKitParams {
    fn default() -> Self {
        Self {
            pads: [DrumPadParams::default(); DRUM_PADS],
            master: 1.0,
        }
    }
}

/// A mixer COLUMN, which is not a desk index and may never be used as one.
///
/// **This exact aliasing has shipped three times.** Columns and desk indices
/// agree on every channel that has a column and disagree past the last one —
/// so a handler that read `Strip::all()`, `strip_names`, `strip_sends` or a
/// mute mask with a raw column index passed every manual test and was wrong on
/// exactly the master: the master's fader moved the metronome, its inserts
/// landed on the click, and its typed name went to a channel with no column to
/// show it. Each time the fix was one missed call site.
///
/// So the two spaces get two types, and the column is an ENUM now rather
/// than a number. It was `Column(usize)`, whose pinned click and master were
/// "one past the channels" and "two past", so a desk that grew a track moved
/// both of them and every `.0` that compared against the width was a fourth
/// shipping of the same bug waiting for the desk to change size (docs/design/
/// unlimited-tracks.md, section 10). A track column names its ROW; the click
/// and the master name themselves, and no arithmetic reaches either.
///
/// The ways out are [`Column::strip`], which answers what the column is, and
/// [`Column::strip_ref`], which hands the question of where its settings live
/// to [`StripRef`], the one home of strip-index arithmetic.
///
/// The derived order is the order the desk draws them in: every track, then
/// the click, then the master, which is the order `Column(usize)` sorted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Column {
    /// The strip of the track on this ROW of the desk.
    Track(usize),
    /// The click's pinned strip, left of the master.
    Click,
    /// The master's pinned strip, rightmost.
    Master,
}

impl Column {
    /// Which strip this column shows, or `None` for the master.
    ///
    /// The one place a column becomes a strip. It used to look itself up in
    /// a freshly built `Strip::shown()` array on every call, which cost a
    /// thirty-three-entry array per question; a match is the same answer.
    pub fn strip(self) -> Option<Strip> {
        match self {
            Column::Track(row) => Some(Strip::Channel(row)),
            Column::Click => Some(Strip::Click),
            Column::Master => None,
        }
    }

    /// What this column is, as the identity every desk array is read by.
    pub fn strip_ref(self) -> StripRef {
        match self {
            Column::Track(row) => StripRef::Row(row),
            Column::Click => StripRef::Click,
            Column::Master => StripRef::Master,
        }
    }

    /// Where this column's settings live in every desk array.
    ///
    /// A shim over [`StripRef::desk_index`], which is where the master's
    /// "past every strip" convention is written down.
    pub fn desk_index(self) -> usize {
        self.strip_ref().desk_index()
    }

    /// The track ROW this column draws, or `None` for the two pinned strips.
    pub fn row(self) -> Option<usize> {
        match self {
            Column::Track(row) => Some(row),
            Column::Click | Column::Master => None,
        }
    }

    /// Whether this column is one of the two pinned at the right, which
    /// never scroll and never clip.
    pub fn pinned(self) -> bool {
        matches!(self, Column::Click | Column::Master)
    }

    /// Every column of a desk `rows` tracks wide, in drawn order: the tracks,
    /// then the click, then the master. What a test walks where it walked
    /// `0..COLUMNS` before the columns had names.
    pub fn every(rows: usize) -> impl Iterator<Item = Column> {
        (0..rows).map(Column::Track).chain([Column::Click, Column::Master])
    }
}

/// Said the way a test message or a log line names it: `Track(3)`, `Click`,
/// `Master`.
impl std::fmt::Display for Column {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// **A strip of the desk, by what it is**: a track's row, one of the three
/// buses, or the master.
///
/// The ONE home of strip-index arithmetic until the specials leave strip
/// space (W5). The desk arrays (`strip_names`, `strip_sends`, the mute and
/// solo masks, the meter holds, the insert seats) are laid out as the rows,
/// then the drum bus, the click and the effects return, then the master;
/// every other piece of code asks this type where something lives instead
/// of adding a width to an index. The views do no such sums at all: a scan
/// (`no_view_code_does_strip_index_arithmetic`) holds them to it, so the day
/// the desk stops being a fixed width there is one place that changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StripRef {
    /// The track on this row.
    Row(usize),
    /// The drum bus, which has a desk slot and no column.
    Drums,
    /// The click.
    Click,
    /// The effects return, which has a desk slot and no column.
    Fx,
    /// The master, past every strip.
    Master,
}

impl StripRef {
    /// Its place in `Desk`'s arrays: the rows first, the three buses after
    /// them, the master last.
    pub const fn desk_index(self) -> usize {
        match self {
            StripRef::Row(row) => row,
            StripRef::Drums => CHANNELS,
            StripRef::Click => CHANNELS + 1,
            StripRef::Fx => CHANNELS + 2,
            StripRef::Master => STRIPS,
        }
    }

    /// The strip at a desk index, the exact inverse of [`Self::desk_index`].
    /// `None` past the master, which is no strip at all.
    pub const fn from_desk_index(i: usize) -> Option<StripRef> {
        Some(if i < CHANNELS {
            StripRef::Row(i)
        } else if i == CHANNELS {
            StripRef::Drums
        } else if i == CHANNELS + 1 {
            StripRef::Click
        } else if i == CHANNELS + 2 {
            StripRef::Fx
        } else if i == STRIPS {
            StripRef::Master
        } else {
            return None;
        })
    }

    /// The `Strip` this is, or `None` for the master, which has none.
    pub const fn strip(self) -> Option<Strip> {
        match self {
            StripRef::Row(row) => Some(Strip::Channel(row)),
            StripRef::Drums => Some(Strip::Drums),
            StripRef::Click => Some(Strip::Click),
            StripRef::Fx => Some(Strip::Fx),
            StripRef::Master => None,
        }
    }

    /// The mixer column it is drawn in, or `None` for the drum bus and the
    /// effects return, which have none.
    pub const fn column(self) -> Option<Column> {
        match self {
            StripRef::Row(row) => Some(Column::Track(row)),
            StripRef::Click => Some(Column::Click),
            StripRef::Master => Some(Column::Master),
            StripRef::Drums | StripRef::Fx => None,
        }
    }

    /// The track row, or `None` for a bus or the master.
    pub const fn row(self) -> Option<usize> {
        match self {
            StripRef::Row(row) => Some(row),
            _ => None,
        }
    }

    /// The master's desk index, for the arrays that keep it past the strips.
    pub const MASTER_INDEX: usize = StripRef::Master.desk_index();

    /// **The lane the automation store files this strip's curves under.**
    ///
    /// A different space from the desk index, and the reason it is spelt
    /// out: a track's curves sit on its row, and the MASTER's on the lane
    /// one past the rows, which is where the drum bus sits in the desk
    /// arrays. Only tracks and the master have curves, so a bus has no lane.
    /// The master's lane becomes `AutoLane::Master` in W6 and this goes.
    pub const fn auto_lane(self) -> Option<usize> {
        match self {
            StripRef::Row(row) => Some(row),
            StripRef::Master => Some(CHANNELS),
            StripRef::Drums | StripRef::Click | StripRef::Fx => None,
        }
    }

    /// The strip a stored automation lane belongs to: a track's row, the
    /// master's lane, or nothing past it.
    pub const fn from_auto_lane(lane: usize) -> Option<StripRef> {
        if lane < CHANNELS {
            Some(StripRef::Row(lane))
        } else if lane == CHANNELS {
            Some(StripRef::Master)
        } else {
            None
        }
    }
}

impl From<Strip> for StripRef {
    fn from(s: Strip) -> Self {
        match s {
            Strip::Channel(row) => StripRef::Row(row),
            Strip::Drums => StripRef::Drums,
            Strip::Click => StripRef::Click,
            Strip::Fx => StripRef::Fx,
        }
    }
}

impl Strip {
    /// The channels the MIXER draws, in order: the eight, then the CLICK,
    /// before MASTER.
    ///
    /// The click's fader used to live in the band; the band's fader rows are
    /// gone, so the click is a column like everything else. The effects
    /// return still has no column: its three knobs are the band's, and a bus
    /// that could be soloed against its own sources is a desk with a feedback
    /// question in it. BACKING had a column here and is retired — the
    /// arrangement's lanes carry audio now.
    pub fn shown() -> [Strip; CHANNELS + 1] {
        // **The drum bus strip is retired from the desk** — the drumpad is
        // a lane's instrument now and answers to that lane's own strip.
        // `Strip::Drums` keeps its desk index for the arrays and the
        // engine's fallback; it simply owns no column any more.
        std::array::from_fn(|i| {
            if i < CHANNELS {
                Strip::Channel(i)
            } else {
                Strip::Click
            }
        })
    }

    /// Every strip, in desk order.
    pub fn all() -> [Strip; STRIPS] {
        std::array::from_fn(|i| {
            if i < CHANNELS {
                Strip::Channel(i)
            } else {
                [Strip::Drums, Strip::Click, Strip::Fx][i - CHANNELS]
            }
        })
    }

    /// The column this strip is drawn in, or `None` for the drum bus and
    /// the effects return, which have none. The inverse of [`Column::strip`].
    pub fn column_of(self) -> Option<Column> {
        match self {
            Strip::Channel(row) => Some(Column::Track(row)),
            Strip::Click => Some(Column::Click),
            Strip::Drums | Strip::Fx => None,
        }
    }

    /// Its place in `Desk`'s arrays. See [`StripRef::desk_index`].
    pub const fn index(self) -> usize {
        match self {
            Strip::Channel(row) => StripRef::Row(row).desk_index(),
            Strip::Drums => StripRef::Drums.desk_index(),
            Strip::Click => StripRef::Click.desk_index(),
            Strip::Fx => StripRef::Fx.desk_index(),
        }
    }

    /// Whether it can send to the effects bus.
    ///
    /// Not the bus itself — a bus that could feed itself howls — and **not the
    /// click**: a metronome feeding the reverb is not something anyone asked
    /// for, and the send that existed for it was zero on every desk ever
    /// saved. Deleted rather than defaulted, on the cue-bus decision.
    pub const fn sends(self) -> bool {
        !matches!(self, Strip::Fx | Strip::Click)
    }

    /// What the strip is called where a sentence has to name it.
    ///
    /// Lower case and bare, because every caller so far puts it mid-sentence.
    /// A slot says which one, since "the instrument is muted" on a rack of
    /// five is not an instruction anybody can follow.
    pub fn label(self) -> String {
        match self {
            // Numbered because there are eight of them and "the channel is
            // muted" is not something anybody can act on.
            Strip::Channel(i) => format!("channel {}", i + 1),
            Strip::Drums => "the drum bus".to_owned(),
            Strip::Click => "the click".to_owned(),
            Strip::Fx => "the effects bus".to_owned(),
        }
    }
}

/// What to say about sources a take is going to be missing.
///
/// **The sentence lives here, not in the host**, for the reason every string
/// in this crate does: it is the part that can be tested without a device, a
/// window or a take. The host decides WHICH strips are lost — that needs an
/// engine and an open input — and this decides how to say it.
///
/// `None` for an empty list, so the caller can chain it straight into a status
/// line that is otherwise about live errors.
pub fn missing_from_take(lost: &[Strip]) -> Option<String> {
    let mut names: Vec<String> = lost.iter().map(|s| s.label()).collect();
    let many = names.len() > 1;
    let what = match names.len() {
        0 => return None,
        1 => names.remove(0),
        // "the input, instrument 2 and the backing track" — the list anybody
        // would say out loud, rather than a count of a number nobody can act
        // on.
        _ => {
            let last = names.pop()?;
            format!("{} and {last}", names.join(", "))
        }
    };
    let (is_are, it_them) = if many { ("are", "them") } else { ("is", "it") };
    Some(format!(
        "{what} {is_are} muted, so this take will not have {it_them} - the \
         mixer is on Tab"
    ))
}

/// The routing half of the desk: what goes to the effects, and what is heard.
///
/// **Levels are in [`Gains`] and not here**, because every one of them already
/// existed and is already pushed: the mixer's faders are the band's faders seen
/// a second time, which is the whole reason the band can stay exactly as it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Desk {
    /// How much of each strip goes to the effects bus, 0..=1. Indexed by
    /// [`Strip::index`]; the `Fx` slot is unused and stays zero.
    pub send: [f32; STRIPS],
    pub muted: [bool; STRIPS],
    pub soloed: [bool; STRIPS],
}

impl Default for Desk {
    /// **The routing this app had before it had a mixer.** The instrument sends
    /// everything, nothing else sends anything, nothing is muted and nothing is
    /// soloed — so a settings file written before any of this existed comes up
    /// sounding exactly as it did.
    fn default() -> Self {
        // **Every instrument sends everything, and nothing else sends
        // anything.** That is what an insert on the instrument bus was, so a
        // settings file written before any of this existed comes up sounding
        // exactly as it did.
        // **The first channel sends everything, and nothing else sends
        // anything.** Channel 0 is the desk's default instrument — the DX7 a
        // fresh install plays — and the send is what an insert on the
        // instrument bus was, so a new install comes up sounding exactly as
        // the old one did. Channels ADDED later get their send set to 1.0 at
        // the gesture instead, so a fresh track gets reverb without an index
        // here having to guess its kind.
        let mut send = [0.0; STRIPS];
        send[0] = 1.0;
        Self {
            send,
            muted: [false; STRIPS],
            soloed: [false; STRIPS],
        }
    }
}

impl Desk {
    /// Whether anything is soloed, which is what makes solo exclusive.
    pub fn any_solo(&self) -> bool {
        self.soloed.iter().any(|s| *s)
    }

    /// Whether a strip is heard — which is now also whether it is RECORDED.
    ///
    /// The take is the desk (the `record::TakeSource` choice that used to
    /// stand between them is gone), so this one rule
    /// answers "is it in the file" as well as "is it in the room". The audio
    /// thread's `strip_is_heard` is the same rule over the bit masks this same
    /// `Desk` was pushed as; two answers to that question is the shape of the
    /// bug that collapsing `TakeSource` removed.
    ///
    /// Solo wins, and a soloed strip is heard even if it is also muted —
    /// pressing solo on a muted channel is a request to hear it, and a solo
    /// button that sometimes does nothing is worse than one that overrules.
    pub fn heard(&self, strip: Strip) -> bool {
        if self.any_solo() {
            return self.soloed[strip.index()];
        }
        !self.muted[strip.index()]
    }
}

impl Default for Gains {
    /// Unity for the sources, and the click **under** them.
    ///
    /// -6 dB on the metronome is not timidity: a click at the same level as a
    /// piano is all you can hear, and every one of these gets turned down by
    /// hand within a minute otherwise.
    fn default() -> Self {
        Self {
            channels: [1.0; CHANNELS],
            metronome: 0.5,
            master: 1.0,
            track: 1.0,
            fx_return: 1.0,
        }
    }
}

/// The owned half of [`RecorderView`], held by the app and filled by the host.
///
/// Two types rather than one because they have opposite lifetimes and opposite
/// owners. This is a long-lived struct the desktop binary writes into once a
/// frame from the real recorder; `RecorderView` is the borrowed snapshot the
/// painter reads. Keeping them separate is what stops the panel from acquiring
/// a `String` field it would then be tempted to mutate mid-draw.
///
/// Every field is inert in a plugin: `Caps::capture_devices` is false there, the
/// band takes zero height, and nothing ever writes to this.
/// One input of the interface, as the desk needs to draw it.
///
/// **Filled by the host every frame, like the slots.** Which inputs are open
/// is a fact about the device and the picker, and `tangent-ui` cannot ask either
/// of them — see the firewall.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputState {
    /// What the desk calls it: "x - 3", "Scarlett 18i20 USB - 4/5". Empty for
    /// a strip nobody has filled, which is drawn as somewhere to put one.
    ///
    /// **Assembled by the host**, from the interface's own name or the user's
    /// alias for it, plus the channel. "Scarlett 18i20 USB  -  input 3" does
    /// not fit a mixer strip and never will, and the half worth shortening is
    /// the one that is the same on every channel of the box — so the alias
    /// replaces it, once, for all of them.
    pub label: String,
    /// Two channels rather than one, which is what decides two meter bars.
    /// A mono microphone drawn as two identical bars is a meter claiming a
    /// stereo signal.
    pub stereo: bool,
}

/// What arrangement record has collected SO FAR, pushed by the host each
/// frame so the view can draw the take growing behind the playhead.
#[derive(Debug, Clone, Default)]
pub struct ArrRecView {
    /// The playhead second the record began at.
    pub start_s: f64,
    /// The lanes being recorded: `(lane, is_midi)`.
    pub lanes: Vec<(usize, bool)>,
    /// The notes so far, clip-relative — keys still down included, sounding
    /// to now.
    pub notes: Vec<crate::arrangement::ArrNote>,
}

#[derive(Debug, Clone)]
pub struct RecorderState {
    /// The playhead, in seconds from 0:00 — what the readout and the waveform
    /// draw. Filled by the host once a frame; `tangent-ui` never advances a
    /// clock of its own, so this is a measurement, not a prediction.
    pub position_s: f64,
    /// Whether the green button's audition is running. Distinct from
    /// `state`, which is the TAKE's machine — play is the transport's.
    pub playing: bool,
    /// The red button's OWN machine: armed lanes are being recorded into the
    /// arrangement. Independent of `state`, which is the capture-take's.
    pub arr_recording: bool,
    /// **A realtime EXPORT is running.** It is a take of a SPAN - the song
    /// played start to end into a file - so it runs on the take machine and
    /// `state` shows it as Rolling. But where an ordinary take owns nothing,
    /// an export owns the transport and the lanes: its stems are bound to lane
    /// indices, a seek writes the wrong bars, and Stop cuts it short. The UI
    /// asks this, not `state`, whenever the question is "may the transport or
    /// the lanes move".
    pub exporting: bool,
    /// The record's progress, for the growing ghost clips. `None` idle.
    pub arr_rec: Option<ArrRecView>,
    pub state: RecordState,
    pub elapsed_s: f64,
    /// The loudest thing each strip made since the last frame, in
    /// [`Strip::ALL`] order. Pushed by the host, read by the mixer.
    pub strip_peaks: [[f32; 2]; STRIPS],
    pub meters: Meters,
    /// The output's levels and the limiter's reduction. See the same two
    /// fields on [`RecorderView`].
    pub master: Meters,
    pub gr_db: f32,
    /// The loudness column's three readings, published by the engine ten
    /// times a second. The WINDOW is a setting and is not stored here — see
    /// `TangentApp::recorder_layout_view`, which puts the two together.
    pub loudness: Loudness,
    /// What each lane's CLIPS are sounding, as a pitch mask. Playback only —
    /// the live notes are already in the app's own hands, because it is the
    /// thing that received them.
    pub lane_sounding: [u128; CHANNELS],
    /// The output directory, shortened for display.
    pub dest: String,
    /// The PROJECT and EXPORT folders, likewise. See the same two fields on
    /// [`RecorderView`].
    pub project_dest: String,
    pub export_dest: String,
    /// And the PRESET folder — the fourth standing folder.
    pub preset_dest: String,
    /// And the IR folder — the fifth.
    pub irs_dest: String,
    /// What the next take's folder will be called, from `tangent_record::take`.
    pub folder_preview: String,
    pub camera_name: Option<String>,
    /// The camera named in settings is not present right now.
    pub camera_missing: bool,
    /// The inputs of the interface that are open, one strip each.
    pub inputs: [InputState; INPUTS],
    pub audio_name: Option<String>,
    pub audio_missing: bool,
    pub preview: Option<Preview>,
    pub disk_minutes: Option<f64>,
    pub message: Option<String>,
    pub clip_warning: bool,
    /// The folder the last finished take went to, while it is still the thing
    /// on screen. `Some` is what makes Export mean "re-export that take"
    /// rather than "set up the next one".
    pub last_take_folder: Option<String>,
    /// **A count-in a take saved, or could save, something from is still
    /// clicking** - the take's own, pre-roll or recorded into the file, or
    /// the arrangement's record count, which a take can join mid-count. The
    /// host's fact; see [`RecorderState::tempo_held`].
    pub take_counting: bool,
    /// **The beat of a count-in recorded INTO the take**, within the bar.
    /// With the count in the file the session is writing (Rolling) from the
    /// press, so its own CountIn digit never shows; the engine's click is the
    /// only thing that knows the beat, and the glass shows this instead of
    /// the take's clock until the count is over. The host's fact.
    ///
    /// The host shows it for ANY count running over a writing take - the
    /// take's own in-file count, or the arrangement's count started over it or
    /// joined by it - and folds the engine's CLICK count by the signature's
    /// beats.
    pub count_beat: Option<u32>,
    /// **Play was pressed during a count-in and waits on its downbeat**, when
    /// the song starts. The host's fact; the band lights the Play triangle
    /// for it, and Pause takes it back, as Space does. Nothing showed it, so
    /// the triangle stayed dark over a song about to start.
    pub play_pending: bool,
}

/// Hand-written because `strip_peaks` is a thirty-five-row array and std
/// only derives `Default` for arrays up to thirty-two — the desk outgrew
/// the derive the day it went to thirty-two channels plus its three buses.
impl Default for RecorderState {
    fn default() -> Self {
        Self {
            position_s: 0.0,
            playing: false,
            arr_recording: false,
            exporting: false,
            arr_rec: None,
            state: RecordState::default(),
            elapsed_s: 0.0,
            strip_peaks: [[0.0; 2]; STRIPS],
            meters: Meters::default(),
            master: Meters::default(),
            gr_db: 0.0,
            loudness: Loudness::default(),
            lane_sounding: [0; CHANNELS],
            dest: String::new(),
            project_dest: String::new(),
            export_dest: String::new(),
            preset_dest: String::new(),
            irs_dest: String::new(),
            folder_preview: String::new(),
            camera_name: None,
            camera_missing: false,
            inputs: Default::default(),
            audio_name: None,
            audio_missing: false,
            preview: None,
            disk_minutes: None,
            message: None,
            clip_warning: false,
            last_take_folder: None,
            take_counting: false,
            count_beat: None,
            play_pending: false,
        }
    }
}


impl RecorderState {
    /// **The tempo holds still.** The knob stays in the transport row through
    /// a take - the owner: "the main buttons need to be intact while
    /// recording takes" - and turns freely while the take rolls. It holds for
    /// the one or two bars of a count-in a take has saved something from:
    /// the session counts its pre-roll on the wall clock at the tempo of the
    /// press while the engine re-reads the dial every callback, and a count
    /// recorded into the file has its head latched at the press - so a tempo
    /// moved then puts the digit off the click and the take off its lane. And
    /// through an export, where a tempo move retimes every MIDI clip
    /// mid-render. The arrangement's record count holds as well, before any
    /// take has joined it: a take pressed during it works its head out from
    /// the tempo at the press, so a tempo moved a beat earlier moves the
    /// downbeat out from under that head.
    pub fn tempo_held(&self) -> bool {
        self.exporting || self.take_counting || matches!(self.state, RecordState::CountIn { .. })
    }

    /// Borrow it as the painter wants it.
    ///
    /// The take name, the pre-roll and the hide-elapsed flag come from the app
    /// rather than from here: they are settings and edit state the app already
    /// owns, and a second copy would be a second thing to keep in step.
    /// What the camera is, as a label, without building a whole
    /// [`RecorderView`] to ask.
    ///
    /// The camera pane is drawn from the app's band layout now, which has no
    /// `RecorderView` in reach and no business making one — it needs one fact
    /// about the device, and this is it.
    pub fn camera_label(&self) -> DeviceLabel<'_> {
        match self.camera_name.as_deref() {
            None => DeviceLabel::None,
            Some(n) if self.camera_missing => DeviceLabel::Missing(n),
            Some(n) => DeviceLabel::Open(n),
        }
    }

    pub fn view<'a>(
        &'a self,
        take_name: &'a str,
        name_focused: bool,
        editing: Option<&'a NumEdit>,
        knobs: Knobs,
        hide_elapsed: bool,
        turning: Option<NumField>,
    ) -> RecorderView<'a> {
        let label = |name: &'a Option<String>, missing: bool| match name.as_deref() {
            None => DeviceLabel::None,
            Some(n) if missing => DeviceLabel::Missing(n),
            Some(n) => DeviceLabel::Open(n),
        };
        RecorderView {
            scope: resting_scope(),
            analyzer: resting_analyzer(),
            pointer: None,
            prefer_flats: false,
            input_monitor: false,
            device_send: &crate::ports::DeviceSendStatus::Off,
            scrub: ScrubView::default(),
            state: self.state,
            elapsed_s: self.elapsed_s,
            position_s: self.position_s,
            playing: self.playing,
            arr_recording: self.arr_recording,
            exporting: self.exporting,
            view_window: (0.0, 60.0),
            meters: self.meters,
            master: self.master,
            limiter_off: false,
            tempo_held: self.tempo_held(),
            count_beat: self.count_beat,
            play_pending: self.play_pending,
            gr_db: self.gr_db,
            dest: &self.dest,
            project_dest: &self.project_dest,
            export_dest: &self.export_dest,
            preset_dest: &self.preset_dest,
            irs_dest: &self.irs_dest,
            take_name,
            name_focused,
            editing,
            folder_preview: &self.folder_preview,
            gains: knobs.gains,
            metronome_on: knobs.metronome_on,
            metronome_in_take: knobs.metronome_in_take,
            tempo_bpm: knobs.tempo_bpm,
            count_in_beats: knobs.count_in_beats,
            count_in_bars: knobs.count_in_bars,
            time_signature: knobs.time_signature,
            fx: knobs.fx,
            // Nothing until the caller says otherwise, for the same reason as
            // the units below: the track is the host's to describe.
            track: crate::ports::TrackInfo::NONE,
            // Percent until the caller says otherwise: what each knob's number
            // MEANS is the host describing its own sweeps, not a preference,
            // so it is set over the top rather than threaded through here.
            fx_units: [crate::ports::KnobUnit::Percent; crate::recorder_panel::Fx::ALL.len()],
            varispeed: crate::varispeed::Varispeed::default(),
            loudness: Loudness::default(),
            turning,
            camera: label(&self.camera_name, self.camera_missing),
            audio: label(&self.audio_name, self.audio_missing),
            preview: self.preview,
            disk_minutes: self.disk_minutes,
            hide_elapsed,
            message: self.message.as_deref(),
            clip_warning: self.clip_warning,
        }
    }
}

/// The settings the band renders but does not own.
///
/// Where the six effect knobs are, 0..=1.
///
/// **One struct, because it was three loose floats in three places.** Adding
/// the filters and the limiter would have made that eighteen fields to keep in
/// step by hand, and a knob wired to the wrong one of them looks exactly like a
/// knob that does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FxSends {
    pub reverb: f32,
    pub delay: f32,
    pub chorus: f32,
    /// The phaser's wet mix. 0 is off.
    pub phaser: f32,
    /// The high-pass corner. 0 is out of the way.
    pub hpf: f32,
    /// The low-pass corner. **1.0 is out of the way** — see the binary's
    /// `Sends::lpf`.
    pub lpf: f32,
    /// Output gain into the limiter, 0..=1 across [`GAIN_DB`]. **0 is unity**
    /// — this one adds decibels rather than taking them away, which is why
    /// its resting place is the bottom of its travel and not the middle.
    pub gain: f32,
    /// The limiter's THRESHOLD. 1.0 is bypass.
    pub limiter: f32,
}

impl FxSends {
    /// In the order they are drawn: the row of sends, then the row of
    /// dynamics. Used by anything that walks all eight.
    pub fn get(&self, fx: crate::recorder_panel::Fx) -> f32 {
        use crate::recorder_panel::Fx;
        match fx {
            Fx::Reverb => self.reverb,
            Fx::Delay => self.delay,
            Fx::Chorus => self.chorus,
            Fx::Phaser => self.phaser,
            Fx::Hpf => self.hpf,
            Fx::Lpf => self.lpf,
            Fx::Gain => self.gain,
            Fx::Limiter => self.limiter,
        }
    }
}

/// Which loudness window the ladder is showing.
///
/// **Three, in this order, and the order is the point**: they are the same
/// measurement over three lengths of time, from the one that moves with the
/// music to the one that describes the whole record. Cycling walks along that,
/// which is why it is a click rather than a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoudnessWindow {
    /// 400 ms, ungated. What is happening now — the reading that moves.
    #[default]
    Momentary,
    /// 3 s, ungated. What the last phrase came to, which is what a mix
    /// engineer actually rides.
    ShortTerm,
    /// The gated mean over everything since it was last reset. **This is the
    /// number a streaming service reads**, and the only one worth comparing
    /// against a target.
    Integrated,
}

impl LoudnessWindow {
    pub const ALL: [LoudnessWindow; 3] = [
        LoudnessWindow::Momentary,
        LoudnessWindow::ShortTerm,
        LoudnessWindow::Integrated,
    ];

    /// The next one along, wrapping. See the type's own note on the order.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            LoudnessWindow::Momentary => LoudnessWindow::ShortTerm,
            LoudnessWindow::ShortTerm => LoudnessWindow::Integrated,
            LoudnessWindow::Integrated => LoudnessWindow::Momentary,
        }
    }

    /// The word under the ladder. Short, because the column is narrow: the
    /// standard's own abbreviations, which anybody who has read a loudness
    /// spec already knows and anybody who has not can learn from three
    /// letters as easily as from eleven.
    pub fn label(self) -> &'static str {
        match self {
            LoudnessWindow::Momentary => "M",
            LoudnessWindow::ShortTerm => "S",
            LoudnessWindow::Integrated => "I",
        }
    }

    /// The whole word, for the status line and the screen reader.
    pub fn title(self) -> &'static str {
        match self {
            LoudnessWindow::Momentary => "Momentary",
            LoudnessWindow::ShortTerm => "Short-term",
            LoudnessWindow::Integrated => "Integrated",
        }
    }

    /// Its index, for a persisted setting. **Append-only**, like every other
    /// index table in this app: a saved 2 has to keep meaning Integrated.
    pub fn index(self) -> usize {
        match self {
            LoudnessWindow::Momentary => 0,
            LoudnessWindow::ShortTerm => 1,
            LoudnessWindow::Integrated => 2,
        }
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
}

/// **Where the ladder ends, in LUFS.**
///
/// The top is above full scale — a loudness meter that stops at 0 hides
/// exactly the case it is there to warn about, because loudness is a MEAN
/// and a mean can pass 0 LUFS on material that has been squashed hard
/// enough.
///
/// **The floor came up from −40 to −30.** Forty was quieter than any mix
/// anybody would keep, and it cost the useful part of the ladder its
/// resolution: the whole span anybody actually works in — roughly −24 to −6
/// — was squeezed into two fifths of the bar. Thirty-three LU across the
/// column puts a labelled tick every two, which is close enough to read a
/// number off the bar itself.
pub const LUFS_RANGE: (f32, f32) = (-30.0, 3.0);

/// Silence, and anything close enough to it. Below the standard's own
/// absolute gate there is no useful reading, only a number falling towards
/// negative infinity, so the ladder shows nothing rather than a number.
pub const LUFS_SILENCE: f32 = -70.0;

/// The marks printed down the loudness ladder, and who asks for each.
///
/// **Not decoration.** The whole reason to put a loudness meter on a desk is
/// to hit a number somebody else chose, and the numbers are public: a
/// meter that made you look them up would be a meter you look up once and
/// then guess at for ever.
/// **A meter's number, without its sign.**
///
/// Every ladder in this app descends from 0 and draws the 0, so the minus in
/// front of every other number says what the top of the scale already says.
/// The owner's rule: "so long as 0 is visible, they are unnecessary."
///
/// **Not for a CONTROL's value.** A fader reads −4.5 dB and the gain dial
/// reads +6, and there the sign is the whole difference between cutting and
/// boosting — nothing on the scale says which way that one goes.
///
/// `abs` rather than trimming a character: `format!("{:.0}", -0.0)` is "-0",
/// and a meter that prints minus zero at the top of its ladder is worse than
/// one that prints the minus everywhere.
pub fn meter_number(v: f32) -> String {
    format!("{:.0}", v.abs())
}

/// **Where the bar turns red**, in LUFS.
///
/// The club/CD-loud end: not a target anybody publishes, it is where masters
/// that fight the loudness war land. Past it the meter says so.
///
/// It was one of four named marks — CD, STREAM, PODCAST, BROADCAST — drawn
/// as words down the side of the ladder. The words are gone (the owner's
/// call: with a labelled tick every two LU the numbers say where you are,
/// and anybody who cares about −14 already knows it is −14), and this is the
/// half of them that was doing work.
pub const LUFS_HOT: f32 = -9.0;

/// The three readings, as the ladder shows them.
///
/// All three are carried even though only one is drawn, because the window is
/// cycled by a click and a view that carried only the current one would go
/// blank for a whole second every time somebody pressed it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// 400 ms.
    pub momentary: f32,
    /// 3 s.
    pub short: f32,
    /// Gated, since the last reset.
    pub integrated: f32,
    /// Which one the ladder is showing.
    pub window: LoudnessWindow,
    /// **The stereo pair**, per channel, for the two sliding windows:
    /// `[mom L, mom R, short L, short R]`.
    ///
    /// The ladder is two bars tied together — one stereo meter with a seam
    /// down it, the master's own shape — and this is what fills them. There
    /// is no integrated pair on purpose: see
    /// `ivory::loudness::Loudness::read_stereo`.
    pub pair: [f32; 4],
}

impl Default for Loudness {
    /// Silence, which is what a desk with nothing playing is measuring.
    fn default() -> Self {
        Self {
            momentary: LUFS_SILENCE,
            short: LUFS_SILENCE,
            integrated: LUFS_SILENCE,
            window: LoudnessWindow::default(),
            pair: [LUFS_SILENCE; 4],
        }
    }
}

impl Loudness {
    /// The reading on show.
    pub fn now(self) -> f32 {
        match self.window {
            LoudnessWindow::Momentary => self.momentary,
            LoudnessWindow::ShortTerm => self.short,
            LoudnessWindow::Integrated => self.integrated,
        }
    }

    /// The reading as a fraction of the ladder, 0 at the bottom.
    ///
    /// `None` for silence — see [`LUFS_SILENCE`]. A ladder drawn at zero
    /// height and a ladder drawn for a signal 70 dB down look the same and
    /// mean different things, so the caller is made to say which.
    pub fn fraction(self) -> Option<f32> {
        let v = self.now();
        if !v.is_finite() || v <= LUFS_SILENCE {
            return None;
        }
        let (lo, hi) = LUFS_RANGE;
        Some(((v - lo) / (hi - lo)).clamp(0.0, 1.0))
    }

    /// **What the two bars stand at**, left and right, as fractions of the
    /// ladder — `None` where a channel is silent.
    ///
    /// On the INTEGRATED window both answer with the programme's own
    /// reading: an integrated figure is gated on the whole programme, so a
    /// per-channel one would be a number this app invented. The meter still
    /// reads as a stereo meter there; it simply has nothing to say about
    /// balance, which is honest, because neither does the standard.
    pub fn bars(self) -> [Option<f32>; 2] {
        let scale = |v: f32| {
            if !v.is_finite() || v <= LUFS_SILENCE {
                return None;
            }
            let (lo, hi) = LUFS_RANGE;
            Some(((v - lo) / (hi - lo)).clamp(0.0, 1.0))
        };
        match self.window {
            LoudnessWindow::Momentary => [scale(self.pair[0]), scale(self.pair[1])],
            LoudnessWindow::ShortTerm => [scale(self.pair[2]), scale(self.pair[3])],
            LoudnessWindow::Integrated => [self.fraction(), self.fraction()],
        }
    }

    /// What the number under the ladder says.
    pub fn text(self) -> String {
        match self.fraction() {
            // Unsigned, like every number on the ladder above it — see
            // `meter_number`. The scale descends from 0 and shows the 0.
            Some(_) => format!("{:.1}", self.now().abs()),
            None => "--".to_owned(),
        }
    }
}

/// Passed in rather than stored on [`RecorderState`] because every one of them
/// is a persisted user preference the app already holds, and a second copy is a
/// second thing to keep in step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Knobs {
    pub gains: Gains,
    pub metronome_on: bool,
    pub metronome_in_take: bool,
    pub tempo_bpm: f64,
    pub count_in_beats: u32,
    pub count_in_bars: u32,
    pub time_signature: TimeSignature,
    /// The six effect knobs, 0..=1. See `effects.rs` in the binary for what
    /// they reach: every instrument, and nothing else.
    pub fx: FxSends,
}

impl Default for Knobs {
    fn default() -> Self {
        Self {
            gains: Gains::default(),
            metronome_on: false,
            metronome_in_take: false,
            tempo_bpm: DEFAULT_BPM,
            count_in_beats: 4,
            count_in_bars: 1,
            fx: FxSends::default(),
            time_signature: TimeSignature::default(),
        }
    }
}

/// Something the band asked the host to do, drained after the frame.
///
/// The **request pattern**, for the same reason the directory picker uses it:
/// creating a take directory and opening a device must not happen halfway
/// through painting a frame. The plugin refuses simply by never draining.
///
/// Deliberately short. Everything else a click in the band can do — choosing a
/// folder, choosing a device, changing the count-in, opening the Export dialog
/// — is either a settings write or a dialog, and both of those are the app's
/// own business. A request enum that carried them would be a second, weaker
/// copy of the menu. What is here is what needs a THING the app does not own: a
/// take, or a window belonging to somebody else's code.
// Not `Copy`: `Audition` carries the notes it wants heard, and a list of them
// is the only honest shape for a chord. Not `Eq`: `Locate` carries seconds,
// and seconds are `f64`.
#[derive(Debug, Clone, PartialEq)]
pub enum RecorderRequest {
    /// The arrangement's red button: start a record (counted in from settings
    /// over a stopped song, or joining a take's count already running),
    /// cancel its count, or finish it - the host decides which, because only
    /// it knows what state it is in. Refused while a realtime export runs.
    /// Never a take: that is `Capture`.
    Toggle,
    /// The cassette beside the cog: start or stop a CAPTURE — the whole-take
    /// machinery (window video, all audio, the `.mid`), completely separate
    /// from the arrangement. `Toggle` is the arrangement's red button now;
    /// this is where the old take toggle lives. **The only request that
    /// reaches a take**: a take owns no part of the transport, so every
    /// other request here answers the same under one as without.
    Capture,
    /// Stop the TRANSPORT and the arrangement's record. Separate from
    /// `Toggle` because the band draws a distinct Stop control and "the
    /// button that stops it" must not also be able to start one. It does not
    /// end a take - the cassette does - except the realtime export, which is
    /// a take of a span and which Stop cuts short.
    Stop,
    /// PAUSE: hold the playhead where it stands. Ignored while the
    /// arrangement records or an export plays - a pause mid-record would let
    /// the capture run past the playhead. Not while a take runs: a take films
    /// whatever the transport does. A Play still waiting on a count-in's
    /// downbeat is taken back instead - by the band's Pause, and by Space
    /// while one waits (`RecorderState::play_pending`).
    Pause,
    /// Put out every clip latch: the user has seen it.
    DismissClip,
    /// Copy the effect in one bay to another channel's next free bay,
    /// **with the settings it is playing at**.
    ///
    /// A request rather than a settings write, because the useful half of it
    /// is host-side: an effect copied as a bare path is a fresh instance
    /// with factory defaults, which is not what anybody means by "copy this
    /// compressor to the other guitar". The host asks the live plugin for
    /// its state and lays it beside the target bay so the new instance comes
    /// up already dialled in.
    ///
    /// The target BAY is the hand's when the drop landed on one, and the
    /// host's — the next free — when it landed on a channel's paint or its
    /// header, where there is no bay under the hand to mean. The owner, on
    /// the first version choosing for him every time: "it always takes the
    /// first empty slot -- change this to be per slot freedom."
    CopyInsert {
        from_strip: usize,
        from_bay: usize,
        to_strip: usize,
        to_bay: Option<usize>,
    },
    /// Bring these clips up so their loudest point sits just under full
    /// scale.
    ///
    /// A request because the PEAK is host-side: `tangent-ui` holds a path and
    /// a length, and the samples live behind the firewall with the decoder.
    /// The host measures and writes the gain back through
    /// `TangentApp::set_clip_gain`.
    NormalizeClips(Vec<u64>),
    /// **Weld these audio clips on one track into one**, silences and all.
    ///
    /// A request for the reason `NormalizeClips` is one: the samples are
    /// host-side. The host lays the clips out exactly as playback does -
    /// trim, loop passes, fades, gain, warp and stretch - mixes them with the
    /// engine's own mixer, writes the result into the project's `media/`,
    /// and hands the one clip back through `TangentApp::finish_audio_glue`.
    ///
    /// The app decides whether the selection can be welded (one kind, one
    /// track, two or more clips or one that repeats, not on a frozen track)
    /// and sends nothing otherwise; the host keeps only the audio clips among
    /// the ids and re-checks the track. Where `caps.capture_devices` is false
    /// (a plugin, the Minimal build) `request_recorder` drops it, and nothing
    /// is said unless the app says it.
    GlueAudio(Vec<u64>),
    /// Start the INTEGRATED loudness reading again from nothing.
    ///
    /// A request rather than a setting, because the number does not live
    /// here: it is a gated mean the audio thread is accumulating, and the
    /// only thing the UI can do about it is ask for it to be dropped.
    ResetLoudness,
    /// Open one insert's OWN editor — the plugin's window, with its presets
    /// and its knobs: `(desk index, bay)`.
    ///
    /// A request rather than a dialog, because the window is not ours: the
    /// plugin draws into a native window the host creates after the frame. VST3
    /// requires it on the main thread, and creating an AppKit window with an
    /// egui frame still on the stack is the same re-entrancy the folder picker
    /// avoids.
    OpenInsertEditor(usize, usize),
    /// The green button: audition from the playhead. Pressed while
    /// auditioning, it PAUSES in place; Stop is what returns to the edit
    /// cursor. The host ignores it while the arrangement records or an export
    /// plays - each owns the transport then. A take does not.
    Play,
    /// Put the playhead at this many SECONDS from 0:00. Seconds, because
    /// seconds survive a device that changes rate; the host converts at the
    /// engine's own rate. Legal while rolling — that is what the generation
    /// stamp on the engine side is for.
    Locate(f64),
    /// Open the patch editor on what the built-in is playing: the host
    /// answers with `TangentApp::set_patch_edit`.
    EditPatch {
        slot: usize,
    },
    /// One row of the patch moved. See `dx7::edit` for what the address means.
    SetPatchParam {
        group: usize,
        index: usize,
        value: i32,
    },
    SetPatchName(String),
    /// Write the patch being edited into the user's own bank.
    SavePatch,
    /// Play patch `index` of the loaded cartridge in `slot`'s built-in.
    /// `usize::MAX` means the patch compiled into the app.
    ChoosePatch {
        slot: usize,
        index: usize,
    },
    /// Load `strip`'s own cartridge and push its names to the picker.
    ///
    /// Raised when the patch picker opens on a strip: since the instruments
    /// went per-lane, which thirty-two names the list should show is a fact
    /// about the STRIP, and the host is the only side that can read a file.
    LoadStripCartridge {
        strip: usize,
    },
    /// **Sound these notes through the loaded instrument.**
    ///
    /// The one way this crate can make a noise. It cannot reach the engine —
    /// the firewall is the whole point — so it says what it wants heard and
    /// the host plays it.
    ///
    /// **A held gesture, not a timed one.** `on` is the edge: true when the
    /// key or the mouse goes down, false when it comes up. A duration was
    /// tried first and was wrong twice over. A note that stops on a timer
    /// stops in the middle of a phrase somebody is still holding; and
    /// scheduling the note-off far in the future put it in the MIDI queue
    /// ahead of everything else, where it blocked the drain and delivered the
    /// second, third and fourth notes of a chord a second and a half after the
    /// first.
    ///
    /// The release is guaranteed by the sender, not by a timer: losing focus
    /// counts as letting go, so there is no state in which the key is up and
    /// the note is still sounding.
    Audition { notes: Vec<u8>, on: bool },
    /// **Strike one drumpad.** The same door as `Audition` — the host turns it
    /// into a note on MIDI channel 10, the drum channel, stamped now — but its
    /// own variant, because a pad is not the loaded instrument: the engine
    /// keeps channel 10 for the pads and the piano never hears it.
    ///
    /// The same held-gesture contract as `Audition`: `on` is the edge, and the
    /// sender guarantees the release. Pads are one-shots today and ignore the
    /// off; the off is sent anyway, so obey-note-off pads can exist without a
    /// protocol change.
    PadHit { pad: u8, on: bool },
    /// AUDITION a media file: decode and play it once, replacing any
    /// preview already sounding. The media explorer's click.
    PreviewFile(std::path::PathBuf),
    /// Silence the preview — the explorer closed.
    PreviewStop,
    /// **Turn one knob on a built-in insert, live.** `strip` is the SEAT and
    /// `slot` the bay — the same addressing `OpenInsertEditor` carries, so a
    /// sender converts through `seat_bay` exactly once, on its side. `id` is
    /// the unit's own stable param id and `value` normalised 0..=1; the host
    /// forwards to `Engine::set_insert_param`, which writes the durable
    /// ledger and rides the lock-free ring to the audio thread. The sender
    /// mirrors optimistically (`TangentApp::set_insert_param`), so a face
    /// never waits a frame to see its own drag.
    SetInsertParam { strip: usize, slot: usize, id: u32, value: f32 },
    /// **Set one knob on a built-in insert from words typed into its
    /// reading.** Same addressing as `SetInsertParam`; `text` is what the
    /// hand typed - "-12", "440", "1.5k", "off". The host reads it on the
    /// unit's own scale (`builtin_fx::value_for_reading`), applies it
    /// through the one road a drag takes, and pushes the face back so the
    /// reading shows the number it landed on. Words the unit cannot read
    /// change nothing. The sender does NOT mirror this one: it has no value
    /// to mirror, which is the whole reason the words travel.
    TypeInsertParam { strip: usize, slot: usize, id: u32, text: String },
    /// **Aim the engine's ONE insert tap at a mixer slot, or take it off**
    /// — the feed behind an EQ face's analyzer and an imager face's
    /// vectorscope. `strip` and `slot` are DESK terms (`Strip::index` and
    /// the strip's Nth slot); the host owns the `seat_bay` translation, the
    /// same way it owns it for `OpenInsertEditor`.
    ///
    /// A request because the tap is the engine's: this crate cannot reach
    /// `Engine::set_insert_tap` — the firewall is the whole point — so the
    /// face says which bay it is looking at and the host arms the tap and
    /// starts draining it into `TangentApp::insert_tap_feed`. The LAST
    /// request wins and `on: false` from the closing face disarms.
    ///
    /// **There can be TWO faces open now**, and this comment used to say
    /// there could not: the amp's pedal chips open the pedal's own window
    /// stacked over the amp's. There is still ONE tap, and the rule the app
    /// keeps is that it belongs to the window being worked in — the stacked
    /// one while it is up, handed back to the face underneath when it
    /// closes. Two windows sharing a tap means the top one's analyzer is
    /// live and the one below it is holding its last picture, which is the
    /// right answer for a needle nobody is looking at and much better than
    /// the alternative of a second tap nothing in the engine has.
    SetInsertTap { strip: usize, slot: usize, on: bool },
    // ── presets ─────────────────────────────────────────────────────────
    // The preset road's three requests, at the END because the enum is
    // append-only across parallel lanes. All three are requests for the
    // same reason the folder picker is: the files live behind the firewall,
    // and every one of them re-scans the unit's folder and re-pushes the
    // row list so what a panel shows is what the disk holds.
    /// Write `values` as a named preset for `unit` — "@tangent-eq",
    /// "keys:2" — under `Presets/<the unit's folder>/`. The host sanitizes
    /// `name` into the filename and keeps the typed form inside the file.
    SavePreset {
        unit: String,
        name: String,
        values: Vec<(String, f64)>,
        /// **The unit's state that is not a knob**, as `(key, text)` —
        /// `ports::BuiltinFace::extra` handed straight back, which for the
        /// sampler is the loaded FILE and for the convolver the impulse.
        ///
        /// Widened rather than a second request, because a preset is one
        /// file and two writes of it is a preset that can be half saved. A
        /// preset that remembers only the knobs gives you back the right
        /// envelope over silence — every crop, every stage and every voice
        /// count where you left it, playing nothing, because the file was
        /// never written down.
        ///
        // MERGE-ME(engine): `desktop.rs`'s `R::SavePreset` arm and
        // `save_unit_preset` must carry this into
        // `tangent_ui::preset::Preset::state` (the field is there and
        // round-trips), and `load_unit_preset` must apply what comes back
        // through whatever road binds a sampler's file — the same road
        // `SetInsertIr` uses for an impulse. Until then the UI writes the
        // path into the request and the host drops it on the floor, which
        // is the one thing this field exists to stop.
        extra: Vec<(String, String)>,
    },
    /// Read the preset at `path` and answer with
    /// [`crate::ports::PresetLoaded`] via `TangentApp::set_preset_loaded`.
    /// `unit` is who is asking, so a file that names a different unit is
    /// refused rather than applied to the wrong knobs.
    LoadPreset { unit: String, path: String },
    /// Remove the preset at `path`. The host refuses paths that resolve
    /// outside the preset root — a delete is the one request here that
    /// must never follow user text anywhere it points.
    DeletePreset { path: String },
    // ── the IR wizard's road ────────────────────────────────────────────
    // Appended at the END, like the presets' three above: the enum is
    // append-only across parallel lanes. All four are requests because the
    // capture engine, the sweep math and the `IRs/` folder all live behind
    // the firewall; the wizard says what it wants and reads the answers off
    // the pushed `ports::IrLive`.
    /// Play a level-setting sine on `out` while metering `inp`'s return —
    /// the wizard's stage-2 test tone. `on: false` stops it. Channels are
    /// device indices, zero-based; `level_db` is dBFS of the tone.
    IrTone {
        out: (usize, usize),
        inp: (usize, usize),
        level_db: f32,
        on: bool,
    },
    /// Arm one sweep capture through the hardware loop: play an exponential
    /// sweep of `sweep_s` seconds at `level_db` on `out`, record `inp` for
    /// the sweep plus `tail_s` of silence, then deconvolve. `reference`
    /// marks a loop-only (bypass) pass, kept to divide the next real pass.
    IrCapture {
        out: (usize, usize),
        inp: (usize, usize),
        sweep_s: f64,
        tail_s: f64,
        level_db: f32,
        reference: bool,
        /// How high the sweep climbs, in hertz — appended at the END of this
        /// variant, and clamped by the host to what the rate can carry. See
        /// `dialogs::IR_TOP_CHOICES` for the device this exists for.
        top_hz: f64,
    },
    /// Stop whatever the IR road is doing — tone or capture — and reclaim
    /// its buffers. Also what closing the wizard sends.
    IrCancel,
    /// Write the last finished capture into `IRs/<name>.wav` (32-bit float
    /// stereo) and re-push the shelf rows.
    IrSave { name: String },
    /// Point a loaded Tangent IR bay at another impulse file. `strip` is
    /// the SEAT (the `SetInsertParam` convention). The host rewrites the
    /// bay's state sidecar and reloads the bay, because an IR is applied
    /// to an instance before it crosses to the audio thread - a swap IS a
    /// reload, honestly.
    SetInsertIr { strip: usize, slot: usize, path: String },
    /// **Exchange two loaded insert bays, settings and all.** Appended at
    /// the END, this enum's law. Both addresses are SEATS, like every other
    /// bay-shaped request here.
    ///
    /// The owner: "dragging occupied fx slot x to any other occupied slot
    /// should swap them." The carry gesture already existed and already had
    /// a landing — but the landing only knew which CHANNEL was under the
    /// hand, so a drop was a copy into the target's next free bay and a drop
    /// onto a full rack was a refusal. Aimed at a bay it is a different verb,
    /// and the useful one: a rack is an ORDER, and re-ordering it by dragging
    /// is the gesture every desk has.
    ///
    /// A request rather than two settings writes for [`Self::CopyInsert`]'s
    /// reason, doubled: what has to move is the state each plugin is playing
    /// at right now, and only the host can ask a live instance for it. Two
    /// bare paths exchanged in the settings would swap the units and hand
    /// each of them the other's factory defaults.
    SwapInserts {
        a_strip: usize,
        a_bay: usize,
        b_strip: usize,
        b_bay: usize,
    },
    /// **Carry a unit to an EMPTY bay of the SAME STRIP.** The owner: "drag
    /// slot 1 into unoccupied slot two? it moves." A swap needs two units
    /// and a copy is a second instance of the same effect on one channel,
    /// which is never what dragging within a rack means; this is the third
    /// verb, and the one that lets a rack be re-ordered when there is a gap
    /// to move into. Host-side for `CopyInsert`'s reason: the unit's live
    /// state travels with it.
    ///
    /// **Two seats, not one**, because a strip's six slots are not one rack:
    /// a channel's 0..2 are its own seat and its 3..5 its folder-extra one
    /// (see [`seat_bay`] and [`rack_owner`]). One `strip` field could only
    /// ever express a move INSIDE a group of three, which is precisely the
    /// fault the owner reported — "drag only works between each group of 3".
    /// Whether the two seats are the same STRIP is the caller's question and
    /// `rack_owner` is where it is answered; this carries whatever the
    /// caller decided a move.
    MoveInsert {
        from_strip: usize,
        from_bay: usize,
        to_strip: usize,
        to_bay: usize,
    },
    // ── the DX7 bank builder ────────────────────────────────────────────
    //
    // Appended at the END, this enum's law. The picker's right-hand pane is
    // a bank being BUILT out of patches from any number of cartridges — see
    // `dx7::staging` on the host — and every move the pane makes is a
    // request here, because the voices live on the host and the pane holds
    // nothing but their names. The host answers each one with
    // `TangentApp::set_staged_bank`, the way `LoadStripCartridge` is
    // answered with `set_cartridge`.
    /// Copy patches `indices` of `slot`'s own cartridge — the picker's LEFT
    /// pane — into the staged bank: from slot `at` on, overwriting, when a
    /// drop named one; into the first empty slots when the Stage button or
    /// the right-click road did not.
    StagePatches {
        slot: usize,
        indices: Vec<usize>,
        at: Option<usize>,
    },
    /// Empty these staged slots. The rest keep their numbers.
    UnstagePatches { slots: Vec<usize> },
    /// Exchange two staged slots — a drag within the right pane, which is
    /// also how a patch moves into an empty one.
    SwapStaged { a: usize, b: usize },
    /// Play staged slot `index` through `slot`'s built-in, now. Auditioning,
    /// exactly as `ChoosePatch` is — but nothing is written to the settings,
    /// because a staged patch is in no file until Save bank puts it in one.
    ChooseStaged { slot: usize, index: usize },
    /// Open the patch editor on staged slot `index`, for `slot`'s strip, so
    /// that its Save goes back INTO that slot rather than into the next
    /// empty one. `EditPatch` remains the road in from the left pane.
    EditStaged { slot: usize, index: usize },
    /// Write the staged bank to `path` and make it `strip`'s cartridge — the
    /// same thing loading the file would do, so what was just built is what
    /// the picker shows and what the strip plays from now on. A path with no
    /// extension gets `.syx`. Saved onto the shelf's `default.syx`, it is
    /// also the bank every strip with no bank of its own opens on next time.
    SaveBank {
        strip: usize,
        path: std::path::PathBuf,
    },
    /// **Strip `strip` is holding `key` down, or is holding nothing.** The
    /// DX7 picker's press-and-hold audition, and the one note in this enum
    /// that says WHICH strip it is for. Appended at the END, this enum's law.
    ///
    /// [`RecorderRequest::Audition`] is the other note road and it names no
    /// strip on purpose: it goes through the engine's MIDI in and reaches
    /// every lane the desk's routing admits, which is exactly right for the
    /// on-screen keyboard and exactly wrong for a picker. That window is
    /// about one strip's built-in instrument, and that strip is as often as
    /// not neither armed nor selected — so a picker's note down that road
    /// sounds on whatever the desk happens to be pointing at, or on nothing.
    ///
    /// **A LEVEL, not an edge**, all the way to the audio thread. `key` is
    /// what is being held, not a thing that has happened, so the release is
    /// not a message that can go missing: the sender says "nothing" as often
    /// as it likes and the engine converges on it. Every previous held note
    /// in this app has been an on paired with an off somebody had to remember
    /// to send, and two of them have shipped ringing for ever.
    AuditionStrip { strip: usize, key: Option<u8> },
    /// **Put the copied effect in this bay**, with the settings the copied
    /// one is playing at — the right-click road's PASTE FX, and the answer
    /// to what a drag stopped doing when a carry inside one strip became a
    /// MOVE rather than a copy. The owner: "to overcome the copy paste issue
    /// this creates, add copy FX and paste FX to right click menu for FX
    /// inserts."
    ///
    /// [`Self::CopyInsert`]'s sibling, and it differs in exactly one thing:
    /// a LOADED target is replaced rather than refused. A drop that lands on
    /// a full bay is an accident of aim and gets a message saying so; a row
    /// somebody chose off that bay's own menu is not, and refusing it would
    /// be a menu row that does nothing on most of the bays it appears on.
    ///
    /// Host-side for [`Self::CopyInsert`]'s reason: the state a plugin is
    /// PLAYING at is the thing being pasted, and only the host can ask a
    /// live instance for it. Appended at the END, this enum's law.
    PasteInsert {
        from_strip: usize,
        from_bay: usize,
        to_strip: usize,
        to_bay: usize,
    },
}

/// How hard an auditioned note is struck.
///
/// Firmly, but not at the top of the scale: a sampled piano's loudest layer is
/// usually its harshest, and this is a note somebody asked to hear rather than
/// one they played.
pub const AUDITION_VELOCITY: u8 = 88;

/// The note an auditioned PATCH speaks — middle C, which this app spells C4
/// wherever it writes a pitch down (see `dialogs::pitch_name`, where MIDI 60
/// reading back as C4 is asserted rather than assumed).
///
/// One pitch and not the one under the pointer, because the picker's rows are
/// names and not keys: what a patch sounds like is a question about the patch,
/// and asking it at the same pitch every time is what makes two answers
/// comparable. Middle C because a DX7 bank is voiced around it — the operator
/// levels, the keyboard scaling breakpoints and the rate scaling all read from
/// there, so it is the pitch a patch was almost certainly auditioned at when
/// it was made.
pub const AUDITION_NOTE: u8 = 60;

// ── The export contract ─────────────────────────────────────────────────────

/// How many video files a take produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoMode {
    /// Audio and MIDI only.
    None,
    /// One file containing whichever of camera / display / audio are ticked.
    ///
    /// **The default, and it does not need a camera.** A take records the
    /// WINDOW — the piano, the chord, the diagrams, the band — and the camera
    /// is an inset when there is one. This was `None` on the theory that
    /// somebody recording a practice session wants audio and MIDI only; what
    /// actually happened is that a tester recorded a take, got no `.mp4`, and
    /// had no way to know video was a thing they had to go and switch on.
    ///
    /// A take that was supposed to produce a video and did not is the failure
    /// that wastes a performance, and it costs a rerun to find out.
    #[default]
    Composite,
    /// One file per source, each on its own.
    PerSource,
    /// Both of the above.
    Both,
}

impl VideoMode {
    pub const ALL: [VideoMode; 4] = [
        VideoMode::None,
        VideoMode::Composite,
        VideoMode::PerSource,
        VideoMode::Both,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoMode::None => "None",
            VideoMode::Composite => "One video, composited",
            VideoMode::PerSource => "Separate file per source",
            VideoMode::Both => "Both",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            VideoMode::None => "none",
            VideoMode::Composite => "composite",
            VideoMode::PerSource => "per_source",
            VideoMode::Both => "both",
        }
    }

    fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == k)
    }

    /// Whether a composite file is produced, which is what makes the layout and
    /// "composite contains" controls meaningful.
    pub fn has_composite(self) -> bool {
        matches!(self, VideoMode::Composite | VideoMode::Both)
    }

    pub fn wants_video(self) -> bool {
        !matches!(self, VideoMode::None)
    }
}

/// How the camera and the display share one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// **The display fills the frame; the camera is a small inset over it.**
    ///
    /// The default, and the opposite of what it was. `CameraAbove` was the
    /// default on the reading that this is a tutorial video — hands above,
    /// keys below — and that is a real shape, but it makes the app the
    /// SECONDARY thing: a fretboard and three theory diagrams squeezed into a
    /// band under a webcam are too small to read, which is the whole reason
    /// anybody would record them.
    ///
    /// This app draws the thing worth watching. The camera is the context.
    #[default]
    DisplayFull,
    /// Camera on top, the display below. The shape of every piano tutorial
    /// video ever made.
    CameraAbove,
    DisplayAbove,
    /// Camera fills the frame with the display floated over the bottom of it.
    CameraFull,
    SideBySide,
}

impl Layout {
    pub const ALL: [Layout; 5] = [
        Layout::DisplayFull,
        Layout::CameraAbove,
        Layout::DisplayAbove,
        Layout::CameraFull,
        Layout::SideBySide,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Layout::DisplayFull => "Full app, camera inset",
            Layout::CameraAbove => "Camera above, display below",
            Layout::DisplayAbove => "Display above, camera below",
            Layout::CameraFull => "Camera full frame, display overlaid",
            Layout::SideBySide => "Side by side",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Layout::DisplayFull => "display_full",
            Layout::CameraAbove => "camera_above",
            Layout::DisplayAbove => "display_above",
            Layout::CameraFull => "camera_full",
            Layout::SideBySide => "side_by_side",
        }
    }

    fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == k)
    }

    /// Where the camera and the display go inside one video frame.
    ///
    /// **The display gets a BAND, not a half.** This is the one decision in the
    /// whole layout that is worth arguing about, so: the display content is a
    /// piano keyboard and a chord name, which is a very wide and very short
    /// thing. Splitting a 16:9 frame down the middle gives it twice the height
    /// it can use and starves the camera of the room that shows a pianist's
    /// hands. So in landscape it takes roughly a third, and the camera takes
    /// the rest.
    ///
    /// **Portrait is the exception, and it is why 9:16 is worth offering.**
    /// A vertical frame has height to spare, so the keyboard can have 40% of it
    /// and still leave the camera a near-square pane — which is a better crop of
    /// a person at a piano than 16:9 ever gives. In 9:16 the stack is not a
    /// compromise, it is the best arrangement available.
    ///
    /// Returns `(camera, display)`. Either is `None` when that layer is not in
    /// the composite; when only one is, it takes the whole frame, because a
    /// letterbox around the only thing in the video is a bug rather than a
    /// layout.
    ///
    /// **Each layer arrives as `Some(aspect)` or `None`.** It used to be two
    /// bools and one float, and adding the camera's aspect would have made it
    /// two bools and two adjacent floats of different meaning — the exact shape
    /// that gets passed in the wrong order once and then silently lays out
    /// every video slightly wrong. Presence and proportion are one fact per
    /// layer, so they travel as one value.
    pub fn split(
        self,
        frame: egui::Rect,
        camera: Option<f32>,
        display: Option<f32>,
    ) -> Panes {
        let portrait = frame.height() > frame.width();
        match (camera, display) {
            (None, None) => Panes::default(),
            (Some(_), None) => Panes {
                camera: Some(frame),
                display: None,
            },
            (None, Some(_)) => Panes {
                camera: None,
                display: Some(frame),
            },
            (Some(cam), Some(disp)) => self.both(frame, portrait, disp, cam),
        }
    }

    /// How tall the `DisplayFull` inset is, as a fraction of the frame's short
    /// edge.
    ///
    /// **Smaller than it looks.** At 16:9 this is a sixth of the height and
    /// about a tenth of the width — a person in the corner of a screen, which
    /// is the whole brief. It started at a quarter of the WIDTH, which put a
    /// webcam over a sixth of the picture.
    const INSET_H: f32 = 0.17;

    /// What an inset is assumed to be shaped like when nothing has said.
    ///
    /// A webcam that has not delivered a frame yet has no size, and the inset
    /// still has to be somewhere. 16:9 is what almost every camera on a desk
    /// is, and it is the shape of the frame it sits in.
    pub const DEFAULT_CAMERA_ASPECT: f32 = 16.0 / 9.0;

    /// How tall the display band should be inside a frame this wide.
    ///
    /// **Its content's own height, not a fixed fraction.** The first version
    /// gave the band a flat 30% of a landscape frame and 40% of a portrait one,
    /// and the very first composited frame showed why that is wrong: in a
    /// 360x640 vertical frame the keyboard is 55 points tall and it was handed
    /// 256, so five sixths of the band was dead black and the camera lost a
    /// quarter of the picture to hold it.
    ///
    /// The cap is still there, because the band grows with every panel switched
    /// on and a fretboard plus three theory diagrams could otherwise take the
    /// whole frame.
    fn band_height(frame: egui::Rect, portrait: bool, display_aspect: f32) -> f32 {
        let natural = if display_aspect > 0.01 {
            frame.width() / display_aspect
        } else {
            frame.height() * 0.30
        };
        let cap = frame.height() * if portrait { 0.45 } else { 0.40 };
        natural.clamp(0.0, cap)
    }

    fn both(
        self,
        frame: egui::Rect,
        portrait: bool,
        display_aspect: f32,
        camera_aspect: f32,
    ) -> Panes {
        let band = Self::band_height(frame, portrait, display_aspect) / frame.height().max(1.0);
        match self {
            // The display gets the whole frame and the camera sits in a corner
            // of it. A QUARTER of the width, which is the size a face has to be
            // to read as a person and no larger — the point of this layout is
            // that the app is what you are watching.
            //
            // Bottom RIGHT: the app's own bands run full width and the busiest
            // of them is the keyboard along the bottom, but the right-hand end
            // of it is the top octaves, which are the least played. Top-right
            // would sit over the chord name.
            Layout::DisplayFull => {
                // **The camera's own shape, sized off the short edge, top
                // right.**
                //
                // Its own shape because that is the only size at which nothing
                // is thrown away: `paint_camera` centre-crops to whatever pane
                // it is given, so a square inset silently binned the sides of
                // every 16:9 webcam, and a 16:9 inset would do the same to a
                // 4:3 one. The pane follows the sensor and the whole picture
                // survives.
                //
                // Off the SHORT edge, so it is the same apparent size in a 9:16
                // reel as in a 16:9 video — a fraction of the width would be a
                // quarter of one frame and a postage stamp in the other. And
                // capped at a third of the width so that a very wide camera
                // cannot walk across the picture the layout exists to show.
                //
                // Top right rather than bottom right: the busiest thing in the
                // app is the keyboard along the bottom with the fretboard under
                // it, and both run full width. The top right corner is the end
                // of the theory band, which is the one region with air in it.
                let short = frame.width().min(frame.height());
                let h = short * Self::INSET_H;
                let w = (h * camera_aspect.max(0.05)).min(frame.width() * 0.33);
                let margin = short * 0.025;
                Panes {
                    camera: Some(egui::Rect::from_min_max(
                        egui::Pos2::new(frame.right() - margin - w, frame.top() + margin),
                        egui::Pos2::new(frame.right() - margin, frame.top() + margin + h),
                    )),
                    display: Some(frame),
                }
            }
            Layout::CameraAbove => {
                let cut = frame.bottom() - frame.height() * band;
                Panes {
                    camera: Some(above(frame, cut)),
                    display: Some(below(frame, cut)),
                }
            }
            Layout::DisplayAbove => {
                let cut = frame.top() + frame.height() * band;
                Panes {
                    camera: Some(below(frame, cut)),
                    display: Some(above(frame, cut)),
                }
            }
            // Overlaid, so the camera keeps the whole frame and the display
            // floats over the bottom of it. A little shorter than the stacked
            // band because it is covering the picture rather than sitting
            // beside it.
            Layout::CameraFull => {
                // Overlaid, so it sits a little tighter than the stacked band:
                // it is covering the picture rather than sitting beside it, and
                // an overlay is a caption, not a second pane.
                let h = Self::band_height(frame, portrait, display_aspect * 1.15);
                Panes {
                    camera: Some(frame),
                    display: Some(below(frame, frame.bottom() - h)),
                }
            }
            Layout::SideBySide => {
                // **In portrait this stacks instead**, and that is deliberate
                // rather than a limitation. Two halves of a 9:16 frame are two
                // 9:32 slivers: unusable for a face and unusable for a
                // keyboard. Silently giving the user the arrangement that works
                // is better than giving them the one they literally asked for
                // and letting them find out at the export.
                if portrait {
                    return Layout::CameraAbove.both(frame, portrait, display_aspect, camera_aspect);
                }
                let cut = frame.left() + frame.width() * 0.5;
                Panes {
                    camera: Some(egui::Rect::from_min_max(
                        frame.min,
                        egui::Pos2::new(cut, frame.bottom()),
                    )),
                    display: Some(egui::Rect::from_min_max(
                        egui::Pos2::new(cut, frame.top()),
                        frame.max,
                    )),
                }
            }
        }
    }

    /// Whether the display is drawn ON TOP of the camera rather than beside it.
    ///
    /// The compositor needs to know: an overlaid band is painted after the
    /// camera and over it, and a stacked one is painted into a region the
    /// camera never touches.
    pub fn overlays(self) -> bool {
        matches!(self, Layout::CameraFull | Layout::DisplayFull)
    }

    /// Whether the CAMERA is the thing floated on top rather than the display.
    ///
    /// The compositor paints the overlaid layer last, and which one that is
    /// differs: `CameraFull` floats the display over the camera, `DisplayFull`
    /// floats the camera over the display.
    pub fn camera_on_top(self) -> bool {
        matches!(self, Layout::DisplayFull)
    }
}

/// Where each layer goes in the composited frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Panes {
    pub camera: Option<egui::Rect>,
    pub display: Option<egui::Rect>,
}

fn above(frame: egui::Rect, cut: f32) -> egui::Rect {
    egui::Rect::from_min_max(frame.min, egui::Pos2::new(frame.right(), cut))
}

fn below(frame: egui::Rect, cut: f32) -> egui::Rect {
    egui::Rect::from_min_max(egui::Pos2::new(frame.left(), cut), frame.max)
}

/// The composited video's frame size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Resolution {
    #[default]
    Hd1080,
    Hd720,
    /// **9:16, for Reels / Shorts / TikTok.**
    ///
    /// Not a crop of the landscape frame — a frame shape of its own, which the
    /// layouts read and lay out differently. It is the one aspect where the
    /// stacked layout is not a compromise but the best possible arrangement:
    /// camera above and keyboard below, both full width, no wasted margin
    /// anywhere. In 16:9 the same stack has to give the keyboard a band; in
    /// 9:16 there is height to give it.
    Vertical1080,
    /// Whatever the camera is giving us. Sharpest, and the one that makes the
    /// file size unpredictable, which is why it is not the default.
    MatchCamera,
}

impl Resolution {
    pub const ALL: [Resolution; 4] = [
        Resolution::Hd1080,
        Resolution::Hd720,
        Resolution::Vertical1080,
        Resolution::MatchCamera,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Resolution::Hd1080 => "1920x1080",
            Resolution::Hd720 => "1280x720",
            // Named for what it is FOR. "1080x1920" is a number nobody
            // recognises; "Reels / Shorts" is the reason somebody is choosing
            // it, and the number is there for the one person who wants it.
            Resolution::Vertical1080 => "1080x1920  (Reels / Shorts)",
            Resolution::MatchCamera => "Match camera",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Resolution::Hd1080 => "1080",
            Resolution::Hd720 => "720",
            Resolution::Vertical1080 => "vertical",
            Resolution::MatchCamera => "camera",
        }
    }

    fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == k)
    }

    /// Pixels, or `None` for "whatever the camera says".
    pub fn pixels(self) -> Option<(u32, u32)> {
        match self {
            Resolution::Hd1080 => Some((1920, 1080)),
            Resolution::Hd720 => Some((1280, 720)),
            Resolution::Vertical1080 => Some((1080, 1920)),
            Resolution::MatchCamera => None,
        }
    }

    /// Whether the frame is taller than it is wide.
    ///
    /// The layouts read this rather than measuring the rectangle, so that a
    /// `MatchCamera` frame from a phone held upright is treated as the portrait
    /// frame it is.
    pub fn is_portrait(self, camera: Option<(u32, u32)>) -> bool {
        match self.pixels().or(camera) {
            Some((w, h)) => h > w,
            None => false,
        }
    }
}

/// Which of Tangent's own panels appear in the video.
///
/// A separate set from the live `Settings` flags on purpose: recording a clean
/// piano-and-chord video while keeping the fretboard on screen for your own use
/// is a thing people want, and it is one struct field rather than a code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayShows {
    pub piano: bool,
    pub chord: bool,
    pub fretboard: bool,
    pub theory: bool,
}

impl Default for DisplayShows {
    /// **Everything.**
    ///
    /// It was piano and chord name, on the reasoning that those are the two
    /// panels about what was just played. That reasoning was fine and the
    /// result was wrong: somebody who has turned every band on, and can see
    /// them, presses Record and gets a video with two of them in it — and
    /// nothing on screen ever said the video kept its own list.
    ///
    /// A video with a panel you did not want is a tick away from being fixed.
    /// A video missing the panel you play from is a take you cannot get back.
    fn default() -> Self {
        Self {
            piano: true,
            chord: true,
            fretboard: true,
            theory: true,
        }
    }
}

impl DisplayShows {
    /// Nothing ticked means there is no display layer to composite, which the
    /// dialog has to notice before it offers a layout for it.
    pub fn any(self) -> bool {
        self.piano || self.chord || self.fretboard || self.theory
    }
}

/// What the composited video contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Composite {
    /// **The window itself, captured off the screen, cursor and all** — and
    /// when it is on, it IS the video: the camera and the display layers are
    /// already in the picture, because the picture is the window they are
    /// drawn in. The owner's call: "the camera is the window is now literal,
    /// and anything process related or the cursor is presented in the
    /// capture, whether that's me turning a knob with cursor, or my tweaking
    /// plugins with their windows open or me using the midi editor." The
    /// composited layers below cannot show any of that: they are repainted
    /// from the app's state, and a plug-in's window, the cursor and a drag in
    /// progress are not state the app has. Ships on. Off gives the composite
    /// back, which is also what a platform with no screen capture falls back
    /// to on its own.
    pub window: bool,
    pub camera: bool,
    pub display: bool,
    /// The performance audio, muxed into the video as well as written to the
    /// `.wav`. Off makes a silent video, which is a real request (people who
    /// score to picture) and a common mistake, so it is stated rather than
    /// implied.
    pub audio: bool,
    pub layout: Layout,
    pub shows: DisplayShows,
}

impl Default for Composite {
    fn default() -> Self {
        Self {
            window: true,
            camera: true,
            display: true,
            audio: true,
            layout: Layout::default(),
            shows: DisplayShows::default(),
        }
    }
}

/// Everything the Export dialog decides.
///
/// One struct so that "use these settings for every take" is a single
/// round-trip through the settings file rather than eleven keys that can go out
/// of step with each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportSpec {
    /// Write the `.wav`.
    pub audio: bool,
    /// Write the `.mid`.
    pub midi: bool,
    /// The tempo written into the SMF's tempo meta event. It does not change a
    /// single timestamp — the file is real time either way — it changes what a
    /// DAW's bar ruler lines up with when you drop the file in.
    pub tempo_bpm: f64,
    pub video: VideoMode,
    pub composite: Composite,
    pub resolution: Resolution,
    pub fps: u32,
}

impl Default for ExportSpec {
    fn default() -> Self {
        Self {
            audio: true,
            midi: true,
            tempo_bpm: 120.0,
            video: VideoMode::default(),
            composite: Composite::default(),
            resolution: Resolution::default(),
            fps: 30,
        }
    }
}

/// Why a spec cannot be exported as it stands.
///
/// Returned rather than silently corrected: a dialog that quietly re-ticks a
/// box the user just unticked is worse than one that refuses and says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecProblem {
    /// Neither audio nor MIDI. The owner's brief is that a take produces the
    /// audio, the MIDI and the video; a directory with none of the first two is
    /// not a take.
    NothingToWrite,
    /// A composite was asked for with neither layer in it.
    EmptyComposite,
    /// The display layer is on but every panel inside it is off.
    EmptyDisplay,
}

impl SpecProblem {
    pub fn message(self) -> &'static str {
        match self {
            SpecProblem::NothingToWrite => "A take has to write the audio or the MIDI, or both.",
            SpecProblem::EmptyComposite => {
                "The composited video needs the camera, the display, or both."
            }
            SpecProblem::EmptyDisplay => {
                "The display layer is on but no panel is selected to draw in it."
            }
        }
    }
}

impl ExportSpec {
    /// The first thing wrong with this spec, if anything is.
    pub fn problem(&self) -> Option<SpecProblem> {
        if !self.audio && !self.midi {
            return Some(SpecProblem::NothingToWrite);
        }
        // A window capture is a whole picture on its own; the two layers
        // are only asked about when it is off.
        if self.video.has_composite() && !self.composite.window {
            if !self.composite.camera && !self.composite.display {
                return Some(SpecProblem::EmptyComposite);
            }
            if self.composite.display && !self.composite.shows.any() {
                return Some(SpecProblem::EmptyDisplay);
            }
        }
        None
    }

    pub fn is_valid(&self) -> bool {
        self.problem().is_none()
    }

    /// Whether a camera has to be open for this spec to be satisfiable.
    ///
    /// Used to decide whether the camera controls matter, and — after a take —
    /// which options can be offered at all. A take recorded without the camera
    /// can never gain it: those frames were composited live and nothing kept
    /// them. See `docs/RECORDER-PLAN.md` §5.
    pub fn needs_camera(&self) -> bool {
        match self.video {
            VideoMode::None => false,
            VideoMode::PerSource | VideoMode::Both => true,
            VideoMode::Composite => self.composite.camera,
        }
    }

    /// Whether this spec actually writes a video file, given the camera.
    ///
    /// The host asks before opening an encoder, and the answer has to be the
    /// same one `begin_video` acts on — this is that decision, not a second
    /// copy of it. **A camera is not required.** A take with no camera at all
    /// still records the window, which is the part somebody plays from; a
    /// tester recorded a whole session, had no webcam, and got no `.mp4`,
    /// because the only thing that had ever switched video on was picking a
    /// camera.
    pub fn produces_video(&self, camera_running: bool) -> bool {
        if !self.video.wants_video() {
            return false;
        }
        // The window is a picture whether or not a camera is open. The host
        // still falls back to the layers if the platform has no screen
        // capture, which is why they are still consulted below it.
        if self.video.has_composite() && self.composite.window {
            return true;
        }
        let camera = self.composite.camera && camera_running;
        let display = self.composite.display && self.composite.shows.any();
        camera || display
    }

    /// Roughly how many megabytes a minute this spec writes.
    ///
    /// Deliberately crude and deliberately honest about it: it exists to turn
    /// free disk space into a duration, and being 20% out on that changes
    /// nothing anyone does. Audio is exact (48 kHz, 24-bit, stereo = 8.24
    /// MB/min); the video terms are the bitrates the encoders are configured
    /// for, not measurements of a particular scene.
    pub fn megabytes_per_minute(&self) -> f64 {
        let mut mb = 0.0;
        if self.audio {
            // 48000 * 3 bytes * 2 channels * 60 s
            mb += 48_000.0 * 3.0 * 2.0 * 60.0 / 1_000_000.0;
        }
        if self.midi {
            // A busy performance is a few kB a minute. Rounds to nothing, and
            // saying so is better than pretending MIDI has no cost at all.
            mb += 0.01;
        }
        let per_stream = match self.resolution {
            // 12 Mbit/s and 6 Mbit/s, converted to MB per minute.
            Resolution::Hd1080 | Resolution::MatchCamera => 12.0 * 60.0 / 8.0,
            // Vertical 1080 is the same pixel count as landscape 1080 turned on
            // its side, so it costs the same. Listed separately rather than
            // folded in with Hd1080 because they are not the same frame and a
            // reader checking this table should see that they were both
            // considered.
            Resolution::Vertical1080 => 12.0 * 60.0 / 8.0,
            Resolution::Hd720 => 6.0 * 60.0 / 8.0,
        };
        let streams = match self.video {
            VideoMode::None => 0,
            VideoMode::Composite => 1,
            VideoMode::PerSource => {
                usize::from(self.composite.camera) + usize::from(self.composite.display)
            }
            VideoMode::Both => {
                1 + usize::from(self.composite.camera) + usize::from(self.composite.display)
            }
        };
        mb + per_stream * streams as f64
    }

    /// How many simultaneous video encoders this spec runs.
    ///
    /// Shown in the dialog because three 1080p30 encodes is free on
    /// VideoToolbox and Media Foundation and genuinely expensive on Linux's
    /// software encoder, and the user deserves to know which machine they are
    /// on before they find out during a take.
    pub fn encoder_count(&self) -> usize {
        match self.video {
            VideoMode::None => 0,
            VideoMode::Composite => 1,
            VideoMode::PerSource => {
                usize::from(self.composite.camera) + usize::from(self.composite.display)
            }
            VideoMode::Both => {
                1 + usize::from(self.composite.camera) + usize::from(self.composite.display)
            }
        }
    }

    // ── persistence ─────────────────────────────────────────────────────────
    //
    // Stored as ONE nested object under `record_export` rather than the eleven
    // flat `record_export_*` keys the plan sketched. The plan's own reason for
    // flat keys was consistency with the existing file, but this value is
    // written and read as a unit — "use these settings for every take" is one
    // decision — and eleven independent keys can be half-applied by a
    // hand-edited file in ways that produce a spec `problem()` rejects at the
    // moment of recording. A nested object is atomic: present and complete, or
    // absent and default.

    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("audio".into(), Value::Bool(self.audio));
        m.insert("midi".into(), Value::Bool(self.midi));
        m.insert(
            "tempo_bpm".into(),
            serde_json::Number::from_f64(self.tempo_bpm)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
        m.insert("video".into(), Value::String(self.video.key().into()));
        m.insert("camera".into(), Value::Bool(self.composite.camera));
        m.insert("display".into(), Value::Bool(self.composite.display));
        m.insert("window".into(), Value::Bool(self.composite.window));
        m.insert(
            "composite_audio".into(),
            Value::Bool(self.composite.audio),
        );
        m.insert(
            "layout".into(),
            Value::String(self.composite.layout.key().into()),
        );
        m.insert("show_piano".into(), Value::Bool(self.composite.shows.piano));
        m.insert("show_chord".into(), Value::Bool(self.composite.shows.chord));
        m.insert(
            "show_fretboard".into(),
            Value::Bool(self.composite.shows.fretboard),
        );
        m.insert(
            "show_theory".into(),
            Value::Bool(self.composite.shows.theory),
        );
        m.insert(
            "resolution".into(),
            Value::String(self.resolution.key().into()),
        );
        m.insert("fps".into(), Value::Number(self.fps.into()));
        Value::Object(m)
    }

    /// Per-key forgiving, like every other reader in `settings.rs`: a key that
    /// is missing or the wrong type keeps that field's default rather than
    /// throwing the whole object away.
    pub fn from_value(v: &Value) -> Self {
        let Value::Object(m) = v else {
            return Self::default();
        };
        let mut s = Self::default();
        let b = |k: &str, dst: &mut bool| {
            if let Some(x) = m.get(k).and_then(Value::as_bool) {
                *dst = x;
            }
        };
        b("audio", &mut s.audio);
        b("midi", &mut s.midi);
        b("camera", &mut s.composite.camera);
        b("display", &mut s.composite.display);
        b("window", &mut s.composite.window);
        b("composite_audio", &mut s.composite.audio);
        b("show_piano", &mut s.composite.shows.piano);
        b("show_chord", &mut s.composite.shows.chord);
        b("show_fretboard", &mut s.composite.shows.fretboard);
        b("show_theory", &mut s.composite.shows.theory);
        if let Some(t) = m.get("tempo_bpm").and_then(Value::as_f64) {
            if (MIN_BPM..=MAX_BPM).contains(&t) {
                s.tempo_bpm = t;
            }
        }
        if let Some(k) = m.get("video").and_then(Value::as_str) {
            if let Some(v) = VideoMode::from_key(k) {
                s.video = v;
            }
        }
        if let Some(k) = m.get("layout").and_then(Value::as_str) {
            if let Some(l) = Layout::from_key(k) {
                s.composite.layout = l;
            }
        }
        if let Some(k) = m.get("resolution").and_then(Value::as_str) {
            if let Some(r) = Resolution::from_key(k) {
                s.resolution = r;
            }
        }
        if let Some(f) = m.get("fps").and_then(Value::as_u64) {
            if FPS_CHOICES.contains(&(f as u32)) {
                s.fps = f as u32;
            }
        }
        s
    }
}

/// The tempo marks the dialog will accept. A DAW will happily import 0.001 BPM
/// and then draw a bar ruler several centuries long.
pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 300.0;

/// Frame rates offered. Deliberately short: 30 is what a webcam gives, 24 is
/// what people who want a film look ask for, 60 is what people recording fast
/// passages ask for, and everything else is a support question.
/// **15 is on this list for machines that render video on the CPU.**
///
/// A 2012-era integrated GPU has no Vulkan driver, so mesa's lavapipe
/// rasterises every composited frame on the same cores running the audio
/// callback and the encoder. At 1080p30 the owner's Linux box delivered 44% of
/// its frames and the app was unplayable while it filmed. A 15 fps video of a
/// good take beats a 30 fps video of an unplayable one.
pub const FPS_CHOICES: [u32; 4] = [15, 24, 30, 60];

/// Count-in choices, in BARS.
///
/// Bars now that there IS a time signature to count them in. It used to be
/// beats — 0, 4, 8 — with "(2 bars of 4)" in brackets and 4/4 assumed and
/// stated, because inventing a signature to describe a click would have been a
/// bigger lie than counting beats. In 6/8 that assumption stops being a
/// simplification and starts being wrong: two bars is twelve clicks, and no
/// number of beats is the right answer at every signature.
pub const COUNT_IN_CHOICES: [u32; 4] = [0, 1, 2, 4];

/// Gain range for every fader in the band, in dB, plus what 1.0 means.
///
/// A linear 0..=1 fader is unusable for audio — the whole useful range of a
/// monitor level is squeezed into the top fifth of the travel. These are the
/// endpoints the band's faders map onto.
pub const GAIN_MIN_DB: f32 = -60.0;
pub const GAIN_MAX_DB: f32 = 12.0;

/// A fader position (0..=1, left to right) as a linear gain.
///
/// Below the bottom of the scale is silence rather than -60 dB: a fader pulled
/// all the way down has to be OFF, and 0.001 is not off when the thing it is
/// attenuating is a piano recorded at full scale.
pub fn fader_to_gain(position: f32) -> f32 {
    let p = position.clamp(0.0, 1.0);
    if p <= 0.0 {
        return 0.0;
    }
    let db = GAIN_MIN_DB + p * (GAIN_MAX_DB - GAIN_MIN_DB);
    10f32.powf(db / 20.0)
}

/// The inverse, for drawing a stored gain back as a fader position.
pub fn gain_to_fader(gain: f32) -> f32 {
    if gain <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * gain.log10();
    ((db - GAIN_MIN_DB) / (GAIN_MAX_DB - GAIN_MIN_DB)).clamp(0.0, 1.0)
}

/// "-6.0 dB", "0.0 dB", "OFF" — what the number beside a fader says.
pub fn gain_text(gain: f32) -> String {
    if gain <= 0.0 {
        return "OFF".to_owned();
    }
    format!("{:+.1} dB", 20.0 * gain.log10())
}

// ── Typing a number in ──────────────────────────────────────────────────────
//
// Every control in the band that shows a number can be typed into as well as
// dragged, because a drag cannot hit 120 BPM on a band fifteen points high and
// "about 118" is not a tempo anybody wants in a MIDI file. The gesture is a
// TAP: press and release without moving opens the field, press and move is the
// drag it always was. Nothing had to be given up for it — the only thing a tap
// used to do was jump the value to wherever the cursor happened to be, which is
// the least precise thing either gesture can do.

/// A control in the band that carries a number, and so can be typed into.
///
/// Deliberately NOT the `Hit` itself: a `Hit` carries the value the gesture
/// would set, so two frames of the same drag are two different `Hit`s, and the
/// identity of the field being edited has to outlive that.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumField {
    /// The master, typed in **decibels** like the fader curve it shares —
    /// not a percentage. It is a level.
    Master,
    /// One of the six effect knobs, typed as a PERCENT. Every other field
    /// here is typed in the unit it is displayed in, and "40" for four tenths
    /// wet is the only reading of a send anybody has ever wanted to write.
    Fx(crate::recorder_panel::Fx),
    Tempo,
    /// The time signature. Typed rather than dragged — "6/8" is two numbers and
    /// a slash, and there is no continuum between 4/4 and 7/8 to drag along.
    Meter,
    /// The varispeed range's two fields, typed as plain percentages of
    /// normal speed — 85 and 115, the numbers printed beside them. **Not
    /// signed trims like the dial above:** a range is two absolute ends, and
    /// `Min: -15` would be a field disagreeing with the label next to it.
    VariMin,
    VariMax,
    /// The time signature that starts at one BAR of the arrangement, typed
    /// on its own chip on the ruler.
    ///
    /// Carries the bar because there can be many of them, unlike [`Meter`],
    /// which is the band's one song-wide signature.
    ///
    /// **Empty CLEARS the change** — except at bar zero, which a song must
    /// always have a signature from.
    MeterAt(u32),
    /// The varispeed dial, typed as a SIGNED PERCENTAGE — the number it
    /// reads. `+15`, `-7.5`, `0`. Typing 115 for fifteen percent up would be
    /// the field disagreeing with the readout directly above it.
    Varispeed,
    /// **A marker's NAME**, by index into the document's markers. Appended
    /// at the END, this enum's law. The only field here whose value is a
    /// word rather than a number — the machinery is the same, and a second
    /// inline-typing system for one field would be the thing to explain.
    MarkerName(usize),
}

impl NumField {
    /// **Whether the field is typed into the BAND** - its knobs, dial and the
    /// panels hung off them - rather than into the arrangement. Exhaustive on
    /// purpose: a new variant is appended at the END (this enum's law) and has
    /// to say where it lives, rather than defaulting to one side and being
    /// cleared, or kept, by accident.
    pub fn lives_in_band(self) -> bool {
        match self {
            NumField::Master
            | NumField::Fx(_)
            | NumField::Tempo
            | NumField::Meter
            | NumField::VariMin
            | NumField::VariMax
            | NumField::Varispeed => true,
            NumField::MeterAt(_) | NumField::MarkerName(_) => false,
        }
    }
}

/// A time typed into a trim field, in seconds.
///
/// **Both ways somebody writes one.** `12.5` is twelve and a half seconds and
/// `1:12.5` is a minute and twelve — a backing track is minutes long, so the
/// second is what anybody reading a player's display will type, and refusing
/// it would be a field that rejects the format it prints.
pub fn parse_time(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let (mins, rest) = match t.split_once(':') {
        Some((m, r)) => (m.trim().parse::<f64>().ok()?, r.trim()),
        None => (0.0, t),
    };
    let secs = rest.parse::<f64>().ok()?;
    if !mins.is_finite() || !secs.is_finite() || mins < 0.0 || secs < 0.0 {
        return None;
    }
    Some(mins * 60.0 + secs)
}

/// A signed percentage typed into the varispeed dial, as the trim itself.
///
/// **Lenient about the things somebody actually types**: a leading `+`, a
/// trailing `%`, and either. What it refuses is anything that is not a
/// number — a field that read `abc` as zero would silently disengage a dial
/// somebody meant to nudge.
pub fn parse_trim_percent(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    let t = t.strip_prefix('+').unwrap_or(t);
    let v: f32 = t.parse().ok()?;
    v.is_finite().then_some(v)
}

/// A percent typed into an effect send, as 0..=1.
///
/// Lenient about a trailing `%` because somebody typing a percentage will write
/// one, and refuses anything that is not a number rather than treating it as
/// zero — a field that silently reads "abc" as silence is a field that loses
/// your setting when you fumble a key.
pub fn parse_percent(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    let v: f32 = t.parse().ok()?;
    v.is_finite().then(|| (v / 100.0).clamp(0.0, 1.0))
}

/// A numeric field mid-edit: which one, and what has been typed so far.
///
/// The text is kept as TEXT and not parsed on every keystroke, because a field
/// passes through "-" and "12." on the way to "-12.5" and neither of those is a
/// number. Nothing is applied until it is committed.
#[derive(Clone, Debug)]
pub struct NumEdit {
    pub field: NumField,
    pub text: String,
    /// Where the next character lands — chars, not bytes; the field only
    /// ever holds ASCII digits and punctuation.
    pub caret: usize,
    /// **The seed is still standing, so the next character REPLACES it.**
    ///
    /// Two of the owner's rules met here and disagreed. `new`'s: a field
    /// opens empty "because typing over a selected value is what every DAW
    /// does and what the fingers expect: the first digit replaces, it does
    /// not append." `prefilled`'s: "the field opens looking exactly like the
    /// readout, and the arrows walk the digits." Every field opened by
    /// clicking uses `prefilled`, so the first rule was quietly lost — the
    /// signature field opened on "4/4", somebody typed 6/8, and the box held
    /// "4/46/8", which `TimeSignature::parse` refuses. Nothing happened, and
    /// the report was "I cannot change to a signature other than 4/4".
    ///
    /// Both rules hold with a flag: the box opens looking like the readout,
    /// and the first thing TYPED clears it. Backspace and the arrows put it
    /// down instead, because reaching for those says you meant to edit what
    /// is there rather than replace it.
    pub seeded: bool,
}

impl NumEdit {
    /// Start editing `field`, seeded EMPTY rather than with the current value.
    ///
    /// Empty because typing over a selected value is what every DAW does and
    /// what the fingers expect: the first digit replaces, it does not append.
    /// Seeding with "+0.0 dB" would mean deleting seven characters before the
    /// first useful one.
    pub fn new(field: NumField) -> Self {
        Self {
            field,
            text: String::new(),
            caret: 0,
            seeded: false,
        }
    }

    /// Start editing SEEDED with the current reading, the caret at its end
    /// — the owner's rule: the field opens looking exactly like the
    /// readout, and the arrows walk the digits.
    pub fn prefilled(field: NumField, seed: String) -> Self {
        let caret = seed.len();
        Self { field, text: seed, caret, seeded: true }
    }

    pub fn left(&mut self) {
        self.seeded = false;
        self.caret = self.caret.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.seeded = false;
        self.caret = (self.caret + 1).min(self.text.len());
    }

    /// The text with the caret drawn into it, for the band to paint.
    pub fn display(&self) -> String {
        let at = self.caret.min(self.text.len());
        format!("{}\u{2502}{}", &self.text[..at], &self.text[at..])
    }

    /// Accept a typed character, or ignore it.
    ///
    /// Silently drops anything that cannot be part of a number so that a
    /// stray letter — the app's own single-key shortcuts, typed at a field
    /// that has focus — cannot end up in the box.
    /// Whether `ch` may join `text` in this field, and how long the field
    /// may get.
    ///
    /// Split out of `push` so the answer can be asked BEFORE the seed is
    /// cleared. Deciding acceptance against the text that is about to be
    /// thrown away is what makes a refused keystroke — one of the app's own
    /// single-key shortcuts, typed at a field that has focus — empty the box
    /// on its way to being dropped. A guard caught exactly that.
    fn accepts(field: NumField, text: &str, caret: usize, ch: char) -> Option<usize> {
        /// A marker's name is a WORD, so it gets room a number never needs.
        /// Long enough for "Chorus (reprise)"; short enough that a flag
        /// cannot grow wider than the ruler it sits on.
        const NAME_MAX: usize = 24;
        const MAX: usize = 8;
        // **A NAME takes letters**, and it is the only field here that does.
        //
        // Every other branch exists to keep a letter OUT — the app's
        // single-key shortcuts are live while a field has focus, so a stray
        // `d` must not land in a number. `MarkerName` arrived after all of
        // that and fell through to the numeric filter, which dropped every
        // letter it was given: the owner could not type a marker name AT
        // ALL. Anything printable goes in; a control character does not.
        if matches!(field, NumField::MarkerName(_)) {
            return (!ch.is_control() && text.chars().count() < NAME_MAX).then_some(NAME_MAX);
        }
        // A signature is two numbers and a slash, and nothing else: no minus,
        // no decimal point.
        //
        // **`MeterAt` as well as `Meter`, which is the bug that shipped.**
        // This read `== NumField::Meter`, so the band's song-wide box took a
        // slash and the RULER's per-bar chip — the one the whole marker
        // system opens — did not. Every character of "3/4" but the slash
        // went in, `TimeSignature::parse` refused what was left, and the
        // owner's report was that time signatures were "dead too".
        if matches!(field, NumField::Meter | NumField::MeterAt(_)) {
            let ok = ch.is_ascii_digit() || (ch == '/' && !text.contains('/'));
            return (ok && text.len() < MAX).then_some(MAX);
        }
        let ok = ch.is_ascii_digit()
            // One dot, and a minus only in front. Neither check is about
            // rejecting bad input for its own sake: `.parse()` will do that
            // anyway. They stop the field from showing something it will then
            // silently refuse to accept.
            || (ch == '.' && !text.contains('.'))
            || (ch == '-' && text.is_empty() && caret == 0)
            // The filters read in hertz and "20k" is how anybody writes
            // twenty thousand of them. Only there: a percent field showing a
            // k it will refuse is the trap the filter exists to stop.
            || (matches!(ch, 'k' | 'K')
                && matches!(
                    field,
                    NumField::Fx(crate::recorder_panel::Fx::Hpf | crate::recorder_panel::Fx::Lpf)
                )
                && !text.to_ascii_lowercase().contains('k'));
        (ok && text.len() < MAX).then_some(MAX)
    }

    /// Accept a typed character, or ignore it.
    ///
    /// Silently drops anything that cannot be part of this field's value, so
    /// that a stray letter — the app's own single-key shortcuts, typed at a
    /// field that has focus — cannot end up in the box.
    pub fn push(&mut self, ch: char) {
        // **The first character typed replaces the seed.** See `seeded`.
        // Asked against an EMPTY text when the seed is standing, because
        // that is what the field is about to hold — and asked BEFORE
        // anything is cleared, so a refused character leaves the reading
        // exactly as it found it.
        let (text, caret): (&str, usize) = if self.seeded {
            ("", 0)
        } else {
            (self.text.as_str(), self.caret)
        };
        if Self::accepts(self.field, text, caret, ch).is_none() {
            return;
        }
        if self.seeded {
            self.seeded = false;
            self.text.clear();
            self.caret = 0;
        }
        let at = self.caret.min(self.text.len());
        self.text.insert(at, ch);
        self.caret = at + ch.len_utf8();
    }

    pub fn pop(&mut self) {
        self.seeded = false;
        let at = self.caret.min(self.text.len());
        if at > 0 {
            self.text.remove(at - 1);
            self.caret = at - 1;
        }
    }
}

/// Turn typed text into a LINEAR gain, or `None` if it is not a level.
///
/// Reads dB, which is what the field displays. Blank commits nothing.
pub fn parse_gain(text: &str) -> Option<f32> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let db: f32 = t.parse().ok()?;
    // Below the bottom of the fader is OFF, not a very quiet signal. This is
    // the only way to type OFF, and it is the one people reach for: -60 and
    // "as low as it goes" are the same intention.
    if db <= GAIN_MIN_DB {
        return Some(0.0);
    }
    Some(10f32.powf(db.min(GAIN_MAX_DB) / 20.0))
}

/// Turn typed text into a tempo, clamped to what the SMF writer can express.
pub fn parse_bpm(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let bpm: f64 = t.parse().ok()?;
    if !bpm.is_finite() {
        return None;
    }
    Some(bpm.clamp(MIN_BPM, MAX_BPM))
}

/// A time signature, as a musician writes it: `beats`/`unit`.
///
/// # What the tempo means, and why it is not negotiable
///
/// **`tempo_bpm` counts QUARTER notes**, always, whatever the unit is. That is
/// not a preference — an SMF tempo meta event is microseconds per quarter note
/// and nothing else, so any other reading would make the `.mid` disagree with
/// the click in every bar. In 6/8 at 120 a quarter is half a second, so an
/// eighth is a quarter of a second and a bar of six is a second and a half.
///
/// It is worth being loud about because the other convention is common: many
/// musicians set 6/8 by its dotted-quarter pulse and would expect 120 to mean
/// two of those per bar. Under this reading that is 6/8 at 360, which the range
/// allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    /// Beats in a bar — the top number. 1..=32.
    pub beats: u8,
    /// What gets the beat — the bottom number. A power of two, 1..=32.
    pub unit: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self { beats: 4, unit: 4 }
    }
}

/// The ones worth putting in a menu, in the order a musician would look for
/// them. Anything else can still be typed — see [`TimeSignature::parse`].
pub const TIME_SIGNATURES: [TimeSignature; 8] = [
    TimeSignature { beats: 4, unit: 4 },
    TimeSignature { beats: 3, unit: 4 },
    TimeSignature { beats: 2, unit: 4 },
    TimeSignature { beats: 6, unit: 8 },
    TimeSignature { beats: 9, unit: 8 },
    TimeSignature { beats: 12, unit: 8 },
    TimeSignature { beats: 5, unit: 4 },
    TimeSignature { beats: 7, unit: 8 },
];

impl TimeSignature {
    /// Whether this is a signature anything can play or write.
    ///
    /// The unit has to be a power of two because that is what a note value IS,
    /// and because the SMF meta event stores it as a power: 8 is written as 3.
    pub fn is_valid(self) -> bool {
        (1..=32).contains(&self.beats) && matches!(self.unit, 1 | 2 | 4 | 8 | 16 | 32)
    }

    /// How long one beat of THIS signature lasts, in seconds.
    ///
    /// `4.0 / unit` is the whole of it: the tempo counts quarters, so an eighth
    /// is half a quarter and a half-note is two of them.
    pub fn beat_seconds(self, tempo_bpm: f64) -> f64 {
        let bpm = if tempo_bpm.is_finite() {
            tempo_bpm.clamp(MIN_BPM, MAX_BPM)
        } else {
            DEFAULT_BPM
        };
        (4.0 / f64::from(self.unit.max(1))) * (60.0 / bpm)
    }

    /// Beats in `bars` bars of this signature.
    pub fn beats_in(self, bars: u32) -> u32 {
        u32::from(self.beats.max(1)) * bars
    }

    /// The SMF meta event's second byte: the unit as a POWER of two.
    ///
    /// 4 is written as 2, 8 as 3. Getting this wrong writes a file every DAW
    /// reads as a different signature, which is the sort of thing nobody
    /// notices until a bar line is in the wrong place.
    pub fn unit_power(self) -> u8 {
        match self.unit {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            16 => 4,
            32 => 5,
            // Unreachable through `is_valid`; 4/4 is the answer that cannot
            // make a file unreadable.
            _ => 2,
        }
    }

    /// `"6/8"`, for a menu and for the settings file alike.
    pub fn label(self) -> String {
        format!("{}/{}", self.beats, self.unit)
    }

    /// The inverse. Anything that is not two numbers around a slash, or is not
    /// a signature, is `None` rather than a guess.
    pub fn parse(text: &str) -> Option<Self> {
        let (a, b) = text.trim().split_once('/')?;
        let sig = Self {
            beats: a.trim().parse().ok()?,
            unit: b.trim().parse().ok()?,
        };
        sig.is_valid().then_some(sig)
    }
}

/// One audio device, as the status panel reports it.
///
/// Plain data, filled by the binary: `tangent-ui` cannot see a device and must
/// not learn how. `None` throughout means "no device open", which is a real
/// state now that an input is never opened until one is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamStats {
    pub sample_rate: u32,
    /// **The DEVICE's width, on both sides.**
    ///
    /// The output has always published this. The input published the TAKE's
    /// width — `TAP_CHANNELS`, a constant 2 — so a 20-input interface read
    /// "2 ch" on the INPUT line, "20 ch" on the OUTPUT line and "all 20, as
    /// one device" three rows further down, and the only number carrying no
    /// information at all was the one that looked like a contradiction.
    pub channels: u16,
    /// Frames per callback, when the device is running one it will admit to.
    /// `None` is cpal's `BufferSize::Default` — the device chose, and did not
    /// say what it chose.
    ///
    /// **This is what TANGENT ASKED FOR — which is not always what the user
    /// chose.** Three numbers live in this area and the panel used to name
    /// two: the CHOICE (a size off the BUFFER SIZE row, or "Device"), the ASK
    /// (this — the choice resolved through `pick_buffer`, which turns "Device"
    /// into `WANT_BUFFER_FRAMES`, clamps to the device's range and multiplies
    /// by `BUFFER_PERIODS` where the host reads a request as a ring), and the
    /// MEASUREMENT (`observed_frames`). Labelling this one "asked for" in a
    /// panel whose chip said "Device" is how the owner came to be told his
    /// interface had refused a 256 he never typed. The choice is not carried
    /// here on purpose — see `Dialog::AudioSetup`, where the readout and the
    /// chip read one variable and therefore cannot disagree.
    pub buffer_frames: Option<u32>,
    /// **What the callback is actually being handed**, counted rather than
    /// requested. `None` before the first callback, or on a side that does
    /// not measure it.
    ///
    /// On macOS a device's buffer size belongs to the DEVICE, not to the
    /// client: two apps cannot hold one interface at two sizes. Open a
    /// session while another host already has the same interface at 512 and
    /// the request for 128 does not get 128 — and every figure here used to
    /// be the request, so the panel said one thing and the room said another
    /// with nothing to catch it. This is the number that cannot lie.
    pub observed_frames: Option<u32>,
    /// The ring behind the period, where the host reads a Fixed request as
    /// a RING and mixes in its own periods regardless (WASAPI, ALSA) — the
    /// two are different numbers and the difference is buffering the
    /// millisecond figure above understates. `None` where the request IS
    /// the callback (CoreAudio, ASIO, JACK): render nothing there, never a
    /// zero, because naming a ring those hosts do not keep would invent
    /// one.
    pub buffer_ring: Option<u32>,
}

impl StreamStats {
    /// One buffer's worth of time, in milliseconds.
    /// **Whether the device granted what was asked for.**
    ///
    /// `false` means something else owns the interface's buffer size — the
    /// panel says so rather than repeating a number nobody is running.
    pub fn honoured(self) -> bool {
        match (self.buffer_frames, self.observed_frames) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }

    /// One buffer's worth of time, in milliseconds — of what is REALLY
    /// running, not of what was asked for.
    pub fn buffer_ms(self) -> Option<f64> {
        let frames = self.observed_frames.or(self.buffer_frames)?;
        (self.sample_rate > 0).then(|| f64::from(frames) * 1000.0 / f64::from(self.sample_rate))
    }
}

/// What the status panel shows: both sides of the audio path.
#[derive(Debug, Clone, Default)]
pub struct AudioStatus {
    /// The input, when one is open.
    pub input: Option<(String, StreamStats)>,
    /// The monitor output, when the engine is running.
    pub output: Option<(String, StreamStats)>,
    /// What the MONITORING path is holding, on top of the two device buffers.
    ///
    /// **The number that was missing, and the reason the panel disagreed with
    /// the room.** Input and output are two separate streams here, so the
    /// input's samples reach the output through a ring — and whatever that
    /// ring is holding is latency as surely as a buffer is. It used to build
    /// up unbounded and never drain; it is bounded now, but bounded is not
    /// zero and the estimate has to say so.
    pub monitor_ms: Option<f64>,
    /// **The smallest buffer this machine's audio graph will serve.**
    ///
    /// Carried so the panel can say why a request was not honoured instead of
    /// guessing. See `ports::AudioSetup::graph_floor_frames` for what the guess
    /// used to be and whose machine it was wrong about.
    pub graph_floor: Option<u32>,
    /// Dropouts since this stream opened, from whoever can actually count
    /// them — the graph's own tally where there is one, the client's
    /// otherwise. `None` means NOBODY COULD SEE, and the panel must render
    /// that as nothing at all: a zero from "nobody looked" has already been
    /// believed once on this project, through a take that was half silence.
    pub dropouts: Option<u64>,
    /// **What the render is costing against what it has**, as a percentage
    /// of the block's deadline: the last block, and the worst since the
    /// stream opened.
    ///
    /// The figure REAPER's performance meter has shown for twenty years and
    /// this app has never had. It is a pair rather than one number because
    /// the average is the misleading one: a desk can sit at four percent and
    /// still glitch, since what the room hears is the block that missed, not
    /// the mean of the ones that did not.
    ///
    /// `None` where no block has been rendered yet, or where the engine is
    /// not running.
    pub rt_load: Option<(f32, f32)>,
    /// **The eligibility probe's line, when it is running.** `None` in every
    /// build nobody has asked: see the engine's `ProbeCells`. It is here and
    /// not only in the bench because the half of that measurement which
    /// matters most, how often a look-ahead ring would be thrown away, is
    /// made of user-interface writes and cannot be measured by any offline
    /// render.
    pub probe: Option<String>,
}

impl AudioStatus {
    /// **An ESTIMATE of round-trip latency, and labelled as one everywhere it
    /// is shown.**
    ///
    /// It is one buffer in plus one buffer out, which is the part anybody can
    /// compute. It is not the whole truth: a converter's own analogue-to-digital
    /// and digital-to-analogue stages, the USB frame, and whatever the driver
    /// does in between are all real and none of them are reported by cpal on
    /// any platform. So this is a floor, not a measurement — the true figure is
    /// always larger, typically by a few milliseconds.
    ///
    /// Measuring it properly means a loopback: play a click, record it, count
    /// the samples between. That is worth doing and it is not this.
    pub fn round_trip_ms(&self) -> Option<f64> {
        let a = self.input.as_ref()?.1.buffer_ms()?;
        let b = self.output.as_ref()?.1.buffer_ms()?;
        // Plus whatever the monitor ring is carrying between them. A duplex
        // device would not pay this; two streams do.
        Some(a + b + self.monitor_ms.unwrap_or(0.0))
    }

    /// Whether the two sides disagree about the sample rate.
    ///
    /// Worth its own question because it is not cosmetic: the writer drains the
    /// instrument's ring at the INPUT's rate while the engine fills it at the
    /// OUTPUT's, so a mismatch overflows that ring and reports the losses
    /// against the take. Every device on the owner's machine is at 48k, which
    /// is exactly why this needs to be visible rather than discovered.
    pub fn rates_disagree(&self) -> bool {
        match (&self.input, &self.output) {
            (Some((_, a)), Some((_, b))) => {
                a.sample_rate > 0 && b.sample_rate > 0 && a.sample_rate != b.sample_rate
            }
            _ => false,
        }
    }
}

/// Buffer sizes the status panel offers, in frames.
///
/// Powers of two, because that is what every driver actually honours, and
/// stopping at 2048 because beyond it the latency is worse than the dropout it
/// was bought to prevent. `None` — the device's own default — is offered
/// alongside these and is what the app has always used.
pub const BUFFER_CHOICES: [u32; 6] = [64, 128, 256, 512, 1024, 2048];

/// Sample rates Setup offers, in Hz.
///
/// The six every interface names, and no more: this is a list to pick a
/// familiar number off, not a survey of what a driver will accept. What the
/// panel actually shows is this list intersected with what the INPUT device
/// reports — see `audio::input_rates` — because a rate offered and then
/// refused is a "could not open" every time somebody tries the biggest number.
///
/// `None`, the device's own, is offered alongside these and is what the app has
/// always used.
pub const SAMPLE_RATE_CHOICES: [u32; 6] =
    [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// Tempo the metronome and the SMF tempo mark share.
///
/// One number, deliberately: a click at 90 against a file that says 120 is a
/// take nobody can edit afterwards.
pub const DEFAULT_BPM: f64 = 120.0;

/// Format a duration the way a transport does: `M:SS` under an hour,
/// `H:MM:SS` over it.
///
/// Its own function so the band and the post-take message cannot disagree, and
/// because the hour case is exactly the one nobody tests and somebody's
/// two-hour practice session finds.
pub fn timecode(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// Turn free bytes into "about this many minutes", the only unit that means
/// anything to someone deciding whether to press record.
///
/// `None` when the spec writes nothing measurable, which would otherwise be a
/// division by zero reported as infinite recording time.
pub fn minutes_on_disk(free_bytes: u64, spec: &ExportSpec) -> Option<f64> {
    let per_min = spec.megabytes_per_minute();
    if per_min <= 0.0 {
        return None;
    }
    Some(free_bytes as f64 / 1_000_000.0 / per_min)
}

/// "~58 min", "~2 h 14 min", "under a minute".
pub fn disk_text(minutes: f64) -> String {
    if minutes < 1.0 {
        "under a minute".to_owned()
    } else if minutes < 90.0 {
        format!("~{} min", minutes.round() as u64)
    } else {
        let h = (minutes / 60.0).floor() as u64;
        let m = (minutes - h as f64 * 60.0).round() as u64;
        format!("~{h} h {m} min")
    }
}

#[cfg(test)]
mod tests {

    /// **A field opens on its reading, and the first thing typed replaces
    /// it.**
    ///
    /// The owner: "fix the fact that I cannot change to a signature other
    /// than 4/4 -- try 6/8 on a new time sig." Nothing was wrong with 6/8.
    /// The chip's field opens PREFILLED with the current signature and the
    /// caret at its end, so typing 6/8 left "4/46/8" in the box,
    /// `TimeSignature::parse` refused it, and the commit did nothing at all
    /// — which is indistinguishable from a signature the app will not
    /// accept. Every field opened by clicking had it; the signature is
    /// where it bites hardest, because a bar number can survive a wrong
    /// digit and a signature cannot parse at all.
    ///
    /// Both of the owner's rules hold now: the box opens looking like the
    /// readout and the arrows walk its digits, AND the first character
    /// typed clears it, "because typing over a selected value is what every
    /// DAW does and what the fingers expect".
    ///
    /// Revert-checks, all four performed: drop the `seeded` clear from
    /// `push` and the first block goes red on "4/46/8"; set `seeded` false
    /// in `prefilled` and the arrow block goes red because the seed is
    /// wiped by an edit that meant to keep it; drop the clear from `pop`
    /// and backspace wipes the whole reading; drop it from `left`/`right`
    /// and walking to a digit then typing throws the rest away.
    #[test]
    fn a_field_opens_on_its_reading_and_the_first_keystroke_replaces_it() {
        let typed = |mut e: NumEdit, s: &str| {
            for ch in s.chars() {
                e.push(ch);
            }
            e.text
        };
        // **The report, exactly.** Open on 4/4, type 6/8, get 6/8.
        let open = || NumEdit::prefilled(NumField::MeterAt(4), "4/4".to_owned());
        assert_eq!(typed(open(), "6/8"), "6/8", "typing over the reading appended to it");
        assert_eq!(typed(open(), "12/8"), "12/8");
        assert_eq!(typed(open(), "3/4"), "3/4");
        assert!(
            TimeSignature::parse(&typed(open(), "6/8")).is_some_and(|s| s.is_valid()),
            "what the field holds after typing 6/8 does not parse"
        );

        // An EMPTY field still just appends — nothing to replace.
        assert_eq!(typed(NumEdit::new(NumField::MeterAt(4)), "6/8"), "6/8");

        // **Backspace EDITS the reading rather than wiping it**, because
        // reaching for backspace says you meant to keep what is there.
        let mut e = open();
        e.pop();
        assert_eq!(e.text, "4/", "backspace threw the whole reading away");
        e.push('8');
        assert_eq!(e.text, "4/8", "a character after backspace replaced the edit");

        // And so do the arrows.
        let mut e = open();
        e.left();
        e.push('8');
        assert_eq!(e.text, "4/84", "walking the digits then typing wiped them");

        // The seed survives a character the field REFUSES, so a stray
        // shortcut key cannot empty a box on its way to being dropped.
        let mut e = open();
        e.push('x');
        assert_eq!(e.text, "4/4", "a refused character cleared the reading");
    }

    /// **A name takes letters, and a per-bar signature takes a slash.**
    ///
    /// Two reports from one function. `NumEdit` is a NUMBER field and every
    /// branch of `push` exists to keep letters out — the app's single-key
    /// shortcuts stay live while a field has focus, so a stray `d` must not
    /// land in a gain box. Two fields arrived afterwards and fell through to
    /// that filter:
    ///
    /// - `MarkerName` is a WORD. Every letter was dropped, so the owner
    ///   could not type a marker name at all — the field opened, took the
    ///   keystrokes, and stayed empty.
    /// - `MeterAt` is the RULER's per-bar signature. The slash was allowed
    ///   for `Meter`, the band's song-wide box, by an `==` that named one
    ///   field; "3/4" arrived as "34", `TimeSignature::parse` refused it,
    ///   and time signatures were "dead too". It also hid a third report —
    ///   with no chip on screen there was no way to reach "Edit time sig."
    ///
    /// The numeric fields are checked in the same test, because the reason
    /// this filter exists is that they must keep refusing letters.
    ///
    /// Revert-checks, all four performed: send `MarkerName` back to the
    /// numeric branch; put the slash rule back to `== NumField::Meter`;
    /// let `NAME_MAX` grow unbounded; and allow a second slash.
    #[test]
    fn a_name_takes_letters_and_a_per_bar_signature_takes_a_slash() {
        let typed = |field: NumField, s: &str| {
            let mut e = NumEdit::new(field);
            for ch in s.chars() {
                e.push(ch);
            }
            e.text
        };

        // A name is a word.
        assert_eq!(typed(NumField::MarkerName(0), "Chorus"), "Chorus");
        assert_eq!(typed(NumField::MarkerName(3), "Verse 2 (alt)"), "Verse 2 (alt)");
        // Long enough for a real name, and bounded so a flag cannot outgrow
        // the ruler it sits on.
        let long = typed(NumField::MarkerName(0), &"x".repeat(80));
        assert_eq!(long.chars().count(), 24, "the name field is unbounded");
        // A control character is still not a character.
        assert_eq!(typed(NumField::MarkerName(0), "a\u{7}b"), "ab");

        // **Both signature fields take the slash**, which is the half that
        // shipped broken.
        for f in [NumField::Meter, NumField::MeterAt(0), NumField::MeterAt(9)] {
            assert_eq!(typed(f, "3/4"), "3/4", "{f:?} would not take a slash");
            assert_eq!(typed(f, "7/8"), "7/8", "{f:?} would not take a slash");
            // One slash, and no letters: a signature is two numbers.
            assert_eq!(typed(f, "3//4"), "3/4", "{f:?} took a second slash");
            assert_eq!(typed(f, "3x/4"), "3/4", "{f:?} took a letter");
            assert_eq!(typed(f, "-3.5/4"), "35/4", "{f:?} took a sign or a point");
        }

        // And the numeric fields still refuse what this filter is for.
        assert_eq!(typed(NumField::Tempo, "12d0"), "120", "a letter reached a number");
        assert_eq!(typed(NumField::Tempo, "1/20"), "120", "a slash reached a number");
    }
    /// **Every pad of every channel gets its own seat, and no old seat moved.**
    ///
    /// A drum instrument loads a full instance per channel, so the pad racks
    /// are per channel. The seats for channels past the first are APPENDED —
    /// the direction `MASTER_EXTRA_SEAT`'s own note calls safe — because an
    /// index that shifts is a project whose plugins move to somebody else's
    /// pad.
    #[test]
    fn every_channels_pads_have_their_own_seats_and_nothing_moved() {
        use super::{pad_rack_index, CHANNELS, DRUM_PADS, MASTER_EXTRA_SEAT, STRIPS, TOTAL_SEATS};
        // Channel 0 keeps the ORIGINAL block. This is the assertion that
        // stops a saved session's pad plugins from moving.
        for pad in [0usize, 36, 127] {
            assert_eq!(pad_rack_index(0, pad), STRIPS + 1 + pad);
        }
        // And the master's extra rack is where it always was.
        assert_eq!(MASTER_EXTRA_SEAT, STRIPS + 1 + DRUM_PADS);

        // No two (channel, pad) pairs share a seat, and none collides with a
        // strip, the master or the master's extra.
        let mut seen = std::collections::HashSet::new();
        for ch in 0..CHANNELS {
            for pad in 0..DRUM_PADS {
                let seat = pad_rack_index(ch, pad);
                assert!(seat < TOTAL_SEATS, "ch {ch} pad {pad} is past the map");
                assert!(
                    seat > STRIPS || seat == STRIPS + 1,
                    "ch {ch} pad {pad} landed on a strip's seat"
                );
                assert_ne!(seat, MASTER_EXTRA_SEAT, "ch {ch} pad {pad} took the master's");
                assert!(seen.insert(seat), "ch {ch} pad {pad} shares seat {seat}");
            }
        }
        assert_eq!(seen.len(), CHANNELS * DRUM_PADS);
    }

    /// **The panel reports what is RUNNING, not what was asked for.**
    ///
    /// A device's buffer size belongs to the device, not to the client: on
    /// macOS two apps cannot hold one interface at two sizes, so opening a
    /// session while another host already has the same interface at 512 does
    /// not get you 128. Every figure here used to be the REQUEST, so the
    /// panel said 128 while the room ran 512 — and the estimate built on it
    /// was wrong by the same factor, in the direction that makes the app look
    /// faster than it is.
    #[test]
    fn the_buffer_reported_is_the_one_the_device_is_running() {
        use super::{AudioStatus, StreamStats};
        let stats = |asked, seen| StreamStats {
            sample_rate: 48_000,
            channels: 2,
            buffer_frames: asked,
            observed_frames: seen,
            buffer_ring: None,
        };
        // Granted: the two agree and nothing is flagged.
        let ok = stats(Some(128), Some(128));
        assert!(ok.honoured());
        assert!((ok.buffer_ms().unwrap() - 2.6667).abs() < 0.001);

        // REFUSED: the observed size wins, and `honoured` says so — that flag
        // is what the panel prints the explanation from.
        let taken = stats(Some(128), Some(512));
        assert!(!taken.honoured(), "a refused request looked granted");
        assert!(
            (taken.buffer_ms().unwrap() - 10.6667).abs() < 0.001,
            "the panel reported the request, not the reality: {:?}",
            taken.buffer_ms()
        );

        // Nothing observed yet — the first callback has not run. The request
        // is the best available answer and is NOT called a mismatch.
        let early = stats(Some(128), None);
        assert!(early.honoured());
        assert!((early.buffer_ms().unwrap() - 2.6667).abs() < 0.001);

        // And the round-trip estimate is built on the real numbers, so it
        // cannot be quietly flattering. Input 64 asked-and-granted, output
        // asked 128 but running 512, ring 4 ms.
        let status = AudioStatus {
            input: Some(("in".to_owned(), stats(Some(64), Some(64)))),
            output: Some(("out".to_owned(), stats(Some(128), Some(512)))),
            monitor_ms: Some(4.0),
            graph_floor: None,
            dropouts: None,
            rt_load: None,
            probe: None,
        };
        let rt = status.round_trip_ms().expect("both sides are open");
        assert!(
            (rt - (1.3333 + 10.6667 + 4.0)).abs() < 0.01,
            "the estimate used the requested buffer: {rt}"
        );
    }

    /// **The three rules that decide which instrument a key plays.**
    ///
    /// Every one of them is a bug the owner reported: an unarmed track
    /// sounding, and one key sounding two instruments at once in the editor.
    #[test]
    fn live_midi_reaches_armed_lanes_and_the_editors_lane_alone() {
        use super::{live_midi_off, ChannelKind};
        let off = |ch: usize, m: u32| m & (1 << ch) != 0;
        let mut kinds = vec![None; CHANNELS];
        kinds[0] = Some(ChannelKind::Midi);
        kinds[1] = Some(ChannelKind::Midi);
        kinds[2] = Some(ChannelKind::Audio);

        // Nothing armed: silence. The editor is the ONE exception below.
        let m = live_midi_off(&kinds, 0, None);
        assert!(off(0, m) && off(1, m) && off(2, m));

        // ARMED sounds, and it is the only thing that does.
        let m = live_midi_off(&kinds, 1 << 1, None);
        assert!(!off(1, m), "an armed MIDI lane takes no notes");
        assert!(off(0, m));

        // **An UNARMED lane is silent whatever else is true of it.** This is
        // the owner's 2026-09-07 ruling and the regression guard for it: the
        // desk used to open the SELECTED lane too, so an armed track and an
        // unarmed one sounded together and the unarmed one was the one you
        // heard. There is no selection in this function's arguments any more,
        // which is the strongest form the rule can take — but the case worth
        // naming is lane 0 live while lane 1 is the armed one.
        let m = live_midi_off(&kinds, 1 << 1, None);
        assert!(off(0, m), "an unarmed MIDI lane was handed the keyboard");

        // An armed AUDIO lane is still an audio lane. Rule 1 outranks rule 2.
        let m = live_midi_off(&kinds, 1 << 2, None);
        assert!(off(2, m), "an audio lane was handed a note");

        // **The editor takes the desk.** Both lanes armed, editor open on
        // lane 1: lane 0 goes quiet. This is "Mini v4 on track one, pianoteq
        // on track 2, audition note, it triggers BOTH".
        let m = live_midi_off(&kinds, 0b11, Some(1));
        assert!(!off(1, m), "the edited lane went quiet");
        assert!(off(0, m), "the other armed instrument sounded too");

        // **And the editor still needs no arm** — the one road to an audition
        // that survived rule 2 losing its selection clause.
        let m = live_midi_off(&kinds, 0, Some(1));
        assert!(!off(1, m), "the editor's own lane needs arming now");

        // And an editor somehow open on an audio lane silences every
        // CHANNEL rather than opening the desk up.
        let all = ((1u64 << CHANNELS) - 1) as u32;
        assert_eq!(live_midi_off(&kinds, 0b11, Some(2)), all);

        // **The strips past the channels are never in this mask.** Drums,
        // click and fx read it for their own bit and have never been gated
        // by it; a mask built as "everything off, then open the live ones"
        // reaches them where the old one did not.
        for (armed, ed) in [(0u32, None), (0b11, Some(1)), (0, Some(1))] {
            let m = live_midi_off(&kinds, armed, ed);
            assert_eq!(m & !all, 0, "a strip past the channels was gated: {m:#x}");
        }
    }


    // ── what a track listens to ─────────────────────────────────────────────

    fn sel(devices: &[&str], channel: Option<u8>) -> MidiInputSel {
        MidiInputSel {
            devices: devices.iter().map(|d| (*d).to_owned()).collect(),
            channel,
        }
    }

    fn slots() -> Vec<String> {
        vec!["Keystation 88".to_owned(), "Scarlett 2i2".to_owned()]
    }

    /// **Every device and every channel is the DEFAULT, and stays sayable.**
    ///
    /// It is what the app did before any of this existed and it is what most
    /// people want, so it has to survive being written down, read back, and
    /// compiled into masks — and it must admit a keyboard nobody has plugged
    /// in yet, which is the one thing a "list every port that is here right
    /// now" answer could not do.
    #[test]
    fn every_device_and_every_channel_is_the_default_and_admits_one_plugged_in_later() {
        let all = MidiInputSel::EVERYTHING;
        assert!(all.is_everything());
        assert!(all.accepts("a keyboard nobody has seen", 11));
        assert_eq!(all.legacy_spec(), "midi:all");
        assert_eq!(MidiInputSel::from_legacy("midi:all"), Some(all.clone()));

        // Compiled against a table with TWO devices in it, the mask is every
        // slot rather than those two — so slot 7, claimed an hour from now
        // when somebody plugs a third keyboard in, is admitted without
        // anybody re-deriving anything.
        let r = all.route(&slots());
        assert_eq!(r, MidiRoute::EVERYTHING);
        assert!(r.admits(7, 0));
        assert!(r.admits(0, 15));
    }

    /// **A narrowed track hears its devices on its channel, and nothing
    /// else** — the two axes, and each on its own.
    #[test]
    fn a_narrowed_track_hears_its_devices_on_its_channel() {
        let s = sel(&["Keystation 88"], Some(2));
        assert!(s.accepts("Keystation 88", 2));
        assert!(!s.accepts("Keystation 88", 3), "the channel was not honoured");
        assert!(!s.accepts("Scarlett 2i2", 2), "the device was not honoured");

        // And the same rule through the masks the audio thread tests.
        let r = s.route(&slots());
        assert!(r.admits(0, 2));
        assert!(!r.admits(0, 3));
        assert!(!r.admits(1, 2));
        // The app's own notes go everywhere — a clicked key came from no
        // device and can be in no device set.
        assert!(r.admits(MIDI_SRC_APP, 9));
    }

    /// **A device this track names and which is not plugged in is KEPT, is
    /// marked, and simply is not heard until it comes back.**
    ///
    /// Dropping it would mean unplugging a keyboard cost somebody the setup,
    /// which is the thing the dialog exists to stop having to redo. Showing
    /// it the same as a live one would mean nothing says why no notes arrive.
    #[test]
    fn a_device_that_has_gone_away_is_kept_and_said_to_be_gone() {
        let s = sel(&["Keystation 88", "Scarlett 2i2"], None);
        let present = vec!["Scarlett 2i2".to_owned()];
        assert_eq!(s.missing(&present), vec!["Keystation 88"]);
        // The set is untouched: it still names both.
        assert_eq!(s.devices.len(), 2);
        // And it is not heard, because no slot holds it.
        let r = s.route(&present);
        assert!(r.admits(0, 0), "the device that IS here went deaf");
        assert!(!r.admits(1, 0), "a slot nobody holds admitted a message");
    }

    /// **What the chip says, in every state it can be in.**
    ///
    /// It said `MIDI: ALL` or `MIDI: <port>` and both were wrong at once:
    /// `ALL` meant all PORTS while the app had exactly one open, and the port
    /// named one nothing ever connected to. With a channel choice beside it,
    /// a bare `ALL` would also read as all channels — so the axis is named.
    #[test]
    fn the_chip_says_what_the_track_is_actually_listening_to() {
        let here = slots();
        assert_eq!(
            MidiInputSel::EVERYTHING.chip_text(&here),
            "MIDI: ALL DEVICES"
        );
        assert_eq!(
            sel(&[], Some(2)).chip_text(&here),
            "MIDI: ALL DEVICES CH 3",
            "the channel is shown 1-based, as every keyboard's own display is"
        );
        assert_eq!(
            sel(&["Keystation 88"], None).chip_text(&here),
            "MIDI: Keystation 88"
        );
        assert_eq!(
            sel(&["Keystation 88"], Some(0)).chip_text(&here),
            "MIDI: Keystation 88 CH 1"
        );
        assert_eq!(
            sel(&["Keystation 88", "Scarlett 2i2"], None).chip_text(&here),
            "MIDI: Keystation 88 +1",
            "the dialog names the rest; the chip is cut to its width"
        );
        // **And a device that is not plugged in is visibly different.**
        assert_eq!(
            sel(&["Keystation 88"], None).chip_text(&[]),
            "MIDI: !Keystation 88",
            "an unplugged keyboard read exactly like a connected one"
        );
        assert_eq!(
            sel(&["Keystation 88", "Scarlett 2i2"], Some(9)).chip_text(&here[1..]),
            "MIDI: !Keystation 88 +1 CH 10"
        );
    }

    /// **TWO TRACKS ASKING FOR THE SAME DEVICE AND CHANNEL BOTH HEAR IT.**
    ///
    /// The decision, written down where the rule is. It is not "the first
    /// wins" and it is not refused, for three reasons.
    ///
    /// 1. **It is what this desk already does everywhere else.** Two armed
    ///    instruments both sound; a duplicated DX7 track sounds twice; two
    ///    drum lanes with the same kit both strike. Each of those was a
    ///    reported bug whose fix was "every masked lane renders its own
    ///    instance". A device that could only be claimed once would be the
    ///    same bug, reintroduced one layer up.
    /// 2. **Layering is the ordinary reason to want it.** A piano and a pad
    ///    from one keyboard is not an edge case, it is the first thing
    ///    anybody does with two instrument tracks.
    /// 3. **The alternatives need machinery nobody asked for.** "The first"
    ///    makes the answer depend on channel ORDER, which is invisible and
    ///    changes when tracks are reordered. "Refused" needs an arbitration
    ///    rule, a way to see who holds a device, and a way to take it — three
    ///    things to design and explain so that a thing people want can be
    ///    stopped from happening.
    ///
    /// The cost is stated plainly: somebody who ticks a keyboard on a second
    /// track and did not mean to layer will hear both, and the chip on each
    /// track is what says so.
    #[test]
    fn two_tracks_asking_for_the_same_device_and_channel_both_hear_it() {
        let both = sel(&["Keystation 88"], Some(2));
        let kinds: Vec<Option<ChannelKind>> = (0..CHANNELS)
            .map(|ch| (ch == 1 || ch == 4).then_some(ChannelKind::Midi))
            .collect();
        let mut sels = vec![MidiInputSel::EVERYTHING; CHANNELS];
        sels[1] = both.clone();
        sels[4] = both;
        let routes = live_midi_routes(&sels, &kinds, &slots());
        assert!(routes[1].admits(0, 2), "the first track went deaf");
        assert!(
            routes[4].admits(0, 2),
            "the second track was refused the device the first had claimed"
        );
        // And a track that asked for neither still hears neither.
        assert!(!routes[1].admits(1, 2));
    }

    /// **A lane that is not a MIDI channel keeps hearing everything.**
    ///
    /// An audio channel names an interface input, not a device set, and its
    /// rack's MIDI-listening effects — a wah, a tremolo — have always heard
    /// the whole room. Narrowing them here would be a silent behaviour change
    /// nobody asked for, and `live_midi_off` already shuts an audio lane's
    /// live MIDI off where that is what is wanted.
    #[test]
    fn an_audio_lane_is_not_narrowed_by_a_stale_device_set() {
        let kinds: Vec<Option<ChannelKind>> = (0..CHANNELS)
            .map(|ch| (ch == 3).then_some(ChannelKind::Audio))
            .collect();
        let mut sels = vec![MidiInputSel::EVERYTHING; CHANNELS];
        sels[3] = sel(&["Keystation 88"], Some(2));
        let routes = live_midi_routes(&sels, &kinds, &slots());
        assert_eq!(routes[3], MidiRoute::EVERYTHING);
    }

    /// **Every device any track asks for, and nothing an audio track's spec
    /// happens to say.**
    #[test]
    fn the_wanted_devices_are_the_union_of_what_the_midi_tracks_name() {
        let kinds: Vec<Option<ChannelKind>> = (0..CHANNELS)
            .map(|ch| match ch {
                1 | 2 => Some(ChannelKind::Midi),
                3 => Some(ChannelKind::Audio),
                _ => None,
            })
            .collect();
        let mut sels = vec![MidiInputSel::EVERYTHING; CHANNELS];
        sels[1] = sel(&["Keystation 88", "Scarlett 2i2"], None);
        sels[2] = sel(&["Keystation 88"], Some(0));
        sels[3] = sel(&["not a device"], None);
        assert_eq!(
            wanted_midi_devices(&sels, &kinds),
            vec!["Keystation 88".to_owned(), "Scarlett 2i2".to_owned()],
            "the union is wrong, or an audio lane's set leaked into it"
        );
    }

    /// **A downgrade round-trip that rewrote the old string wins; one that
    /// touched nothing costs nothing.**
    ///
    /// A build without this feature reads only `channel_inputs`, rewrites it
    /// from its own one-device menu, and carries the sets through as a
    /// stranger's key — so the two can end up contradicting each other and
    /// the one that was actually edited has to win. But the ordinary case is
    /// that nothing was edited, and there the whole set (its channel
    /// included) has to survive, or a downgrade nobody noticed costs somebody
    /// their setup.
    #[test]
    fn the_legacy_string_wins_only_when_it_contradicts_the_set() {
        let mut sels = vec![
            sel(&["Keystation 88", "Scarlett 2i2"], Some(2)),
            sel(&["Keystation 88"], Some(5)),
            sel(&["Keystation 88"], None),
            MidiInputSel::EVERYTHING,
        ];
        let legacy = vec![
            // Untouched: this build wrote the first device's name.
            "midi:Keystation 88".to_owned(),
            // An older build chose EVERY port. That contradicts a narrowed
            // set, so the set widens — and the CHANNEL stands, because a
            // build that cannot express one cannot have meant to clear it.
            "midi:all".to_owned(),
            // An older build chose a keyboard this set never named.
            "midi:Nord Stage".to_owned(),
            // Not a MIDI channel at all: anything stored is meaningless.
            "0+1".to_owned(),
        ];
        reconcile_midi_inputs(&mut sels, &legacy);
        assert_eq!(
            sels[0],
            sel(&["Keystation 88", "Scarlett 2i2"], Some(2)),
            "a downgrade that touched nothing cost somebody their second keyboard"
        );
        assert_eq!(sels[1], sel(&[], Some(5)));
        assert_eq!(sels[2], sel(&["Nord Stage"], None));
        // The audio channel's row is reset AND then trimmed away with every
        // other trailing default, which is why the vector is shorter than the
        // legacy list it was reconciled against.
        assert_eq!(sels.len(), 3, "a trailing default survived: {sels:?}");
    }

    /// **A desk nobody has narrowed holds an EMPTY vector**, so it writes no
    /// key and compares equal to a fresh one. Interior defaults stay, because
    /// the vector is indexed by channel.
    #[test]
    fn trailing_defaults_are_trimmed_and_interior_ones_are_not() {
        let mut v = vec![
            MidiInputSel::EVERYTHING,
            sel(&["Keystation 88"], None),
            MidiInputSel::EVERYTHING,
            MidiInputSel::EVERYTHING,
        ];
        trim_midi_inputs(&mut v);
        assert_eq!(v.len(), 2, "a trailing default survived: {v:?}");
        assert!(v[0].is_everything(), "an interior default was squeezed out");
        let mut all = vec![MidiInputSel::EVERYTHING; 8];
        trim_midi_inputs(&mut all);
        assert!(all.is_empty());
    }

    /// **The persisted shape, both ways** — sparse, absent when there is
    /// nothing to say, and forgiving of a hand-edit that names a channel this
    /// build has no wire for.
    #[test]
    fn the_device_sets_round_trip_through_their_sparse_object() {
        assert!(
            write_midi_inputs(&[MidiInputSel::EVERYTHING; 4]).is_none(),
            "a desk nobody narrowed wrote a key"
        );
        let mut v = vec![MidiInputSel::EVERYTHING; 6];
        v[2] = sel(&["Keystation 88"], Some(2));
        v[5] = sel(&[], Some(15));
        let written = write_midi_inputs(&v).expect("a narrowed desk writes");
        let back = read_midi_inputs(Some(&written));
        assert_eq!(back.len(), 6);
        assert_eq!(back[2], v[2]);
        assert_eq!(back[5], v[5]);
        assert!(back[0].is_everything());
        // A hand-edited channel 99 is DROPPED, not wrapped: wrapped it would
        // become channel 3 and silently route somebody's keyboard to a track
        // they never named.
        let bad: Value = serde_json::json!({"1": {"channel": 99}});
        assert_eq!(read_midi_inputs(Some(&bad))[1], MidiInputSel::EVERYTHING);
        // And an empty device name is not a device.
        let blank: Value = serde_json::json!({"1": {"devices": ["", "A"]}});
        assert_eq!(read_midi_inputs(Some(&blank))[1], sel(&["A"], None));
    }

    /// **A meter's numbers carry no sign, and 0 is not minus zero.**
    ///
    /// Every ladder descends from 0 and draws it, so the minus in front of
    /// every other number says what the top of the scale already says — the
    /// owner's rule. The `abs` matters: `format!("{:.0}", -0.0)` is "-0",
    /// which is the one number on the ladder that would still have a sign.
    #[test]
    fn a_meters_numbers_carry_no_sign() {
        use super::meter_number;
        assert_eq!(meter_number(0.0), "0");
        assert_eq!(meter_number(-0.0), "0", "minus zero reached the ladder");
        assert_eq!(meter_number(-6.0), "6");
        assert_eq!(meter_number(-24.0), "24");
        assert_eq!(meter_number(-0.4), "0");
        // A positive number keeps its digits and gains no sign either.
        assert_eq!(meter_number(3.0), "3");

        // And the loudness reading follows the ladder it sits under.
        let l = Loudness {
            momentary: -14.2,
            short: -14.2,
            integrated: -14.2,
            window: LoudnessWindow::Momentary,
            pair: [-14.2; 4],
        };
        assert_eq!(l.text(), "14.2", "the reading kept its sign");
        // Silence still says so rather than printing a number.
        let quiet = Loudness::default();
        assert_eq!(quiet.text(), "--");
    }

    /// **The master has six bays across two racks, and nothing else moved.**
    ///
    /// The extra three live past the pads — see `MASTER_EXTRA_SEAT` — so
    /// that `INSERTS` stays the stride of the persisted array and of the
    /// state sidecars. The mapping is the one thing that can go wrong: a
    /// call site that resolves a slot to the wrong seat edits somebody
    /// else's plugin, silently.
    #[test]
    fn the_masters_six_bays_live_in_two_racks_and_nobody_elses_move() {
        use super::{
            bays_on, seat_bay, Strip, INSERTS, INSERT_SPACE, MASTER_EXTRA_SEAT, MASTER_SEAT,
        };
        assert_eq!(bays_on(None), INSERTS * 2, "the master lost its second rack");
        assert_eq!(bays_on(Some(Strip::Channel(0))), INSERTS * 2);

        // **The first three are the EXTRA rack and the last three the
        // original**, because the new bays were drawn ABOVE the old ones and
        // a rack runs top to bottom — so a session that already had plugins
        // in the master hears them in the same place in the chain.
        for n in 0..INSERTS {
            assert_eq!(seat_bay(MASTER_SEAT, n), (MASTER_EXTRA_SEAT, n));
            assert_eq!(seat_bay(MASTER_SEAT, n + INSERTS), (MASTER_SEAT, n));
        }
        // Six distinct addresses, no collisions.
        let mut seen: Vec<(usize, usize)> =
            (0..INSERTS * 2).map(|n| seat_bay(MASTER_SEAT, n)).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), INSERTS * 2, "two master bays share a seat");

        // **Everything else answers itself.** Channels, the buses and every
        // pad are untouched by the rule.
        for seat in [0usize, 1, 5, Strip::Click.index(), Strip::Fx.index()] {
            for n in 0..INSERTS {
                assert_eq!(seat_bay(seat, n), (seat, n), "seat {seat} was remapped");
            }
        }
        for pad in [0usize, 1, 127] {
            // `pad_insert_index` already answers in SEATS.
            let seat = pad_insert_index(pad);
            assert_ne!(seat, MASTER_EXTRA_SEAT, "pad {pad} collides with the master");
            for n in 0..INSERTS {
                assert_eq!(seat_bay(seat, n), (seat, n), "pad {pad} was remapped");
            }
        }

        // And the insert space is long enough to hold the last of them.
        let (seat, bay) = seat_bay(MASTER_SEAT, INSERTS * 2 - 1);
        assert!(
            seat * INSERTS + bay < INSERT_SPACE,
            "the master's last bay is past the end of the insert space"
        );
    }

    /// **A folder's six slots map plainly, and its extra seat is nobody
    /// else's.** Unlike the master's swap, slots and bays agree here -
    /// 0..2 the channel's own seat, 3..5 the appended one - and every
    /// appended seat is distinct from every seat that already existed.
    #[test]
    fn a_folders_six_slots_have_their_own_appended_seats() {
        use super::{
            amp_pedal_of, amp_pedal_seat, bays_on, folder_extra_seat, pad_rack_index, seat_bay,
            seat_order, Strip, AMP_PEDALS, CHANNELS, DRUM_PADS, INSERTS, INSERT_SPACE,
            MASTER_EXTRA_SEAT, MASTER_SEAT, STRIPS, TOTAL_SEATS,
        };
        // The counts: **every channel wears six**, folder or not — the
        // owner's ruling, see `bays_on`. The two buses keep their three; the
        // master keeps its six.
        assert_eq!(bays_on(Some(Strip::Channel(3))), INSERTS * 2);
        assert_eq!(bays_on(Some(Strip::Channel(0))), INSERTS * 2);
        assert_eq!(bays_on(Some(Strip::Click)), INSERTS * 2);
        assert_eq!(bays_on(Some(Strip::Drums)), INSERTS);
        assert_eq!(bays_on(None), INSERTS * 2);

        // The mapping, plain order, both ends of the desk.
        for ch in [0usize, 7, CHANNELS - 1] {
            for slot in 0..INSERTS {
                assert_eq!(seat_bay(ch, slot), (ch, slot));
                assert_eq!(
                    seat_bay(ch, slot + INSERTS),
                    (folder_extra_seat(ch), slot),
                    "channel {ch} slot {} went astray",
                    slot + INSERTS
                );
            }
        }

        // No appended seat collides with anything that already had one.
        let mut seen = std::collections::HashSet::new();
        for seat in 0..STRIPS {
            assert!(seen.insert(seat));
        }
        assert!(seen.insert(MASTER_SEAT));
        assert!(seen.insert(MASTER_EXTRA_SEAT));
        for ch in 0..CHANNELS {
            for pad in 0..DRUM_PADS {
                assert!(seen.insert(pad_rack_index(ch, pad)));
            }
        }
        for ch in 0..CHANNELS {
            assert!(
                seen.insert(folder_extra_seat(ch)),
                "folder seat for channel {ch} collides"
            );
        }
        for ch in 0..CHANNELS {
            for pedal in 0..AMP_PEDALS {
                assert!(
                    seen.insert(amp_pedal_seat(ch, pedal)),
                    "pedal seat {pedal} of channel {ch} collides"
                );
                // And the inverse names the same seat, both ways.
                assert_eq!(amp_pedal_of(amp_pedal_seat(ch, pedal)), Some((ch, pedal)));
            }
        }
        assert_eq!(amp_pedal_of(0), None);
        assert_eq!(amp_pedal_of(folder_extra_seat(CHANNELS - 1)), None);
        assert_eq!(amp_pedal_of(TOTAL_SEATS), None, "a seat past the block answered");
        assert!(seen.iter().all(|s| *s < TOTAL_SEATS), "a seat past the end");
        let (seat, bay) = seat_bay(CHANNELS - 1, INSERTS * 2 - 1);
        assert!(
            seat * INSERTS + bay < INSERT_SPACE,
            "the last folder bay is past the end of the insert space"
        );
        // The last pedal bay of the last channel fits the insert space too.
        assert!(
            amp_pedal_seat(CHANNELS - 1, AMP_PEDALS - 1) * INSERTS + (INSERTS - 1)
                < INSERT_SPACE,
            "the last pedal bay is past the end of the insert space"
        );

        // **seat_order carries the extra seat AND the pedalboard with its
        // channel** and moves nothing else - two channel rows, two folder
        // seats and two boards of seven for one swap, identity elsewhere.
        let mut order: Vec<usize> = (0..CHANNELS).collect();
        order.swap(2, 5);
        let full = seat_order(&order);
        assert_eq!(full.len(), TOTAL_SEATS);
        assert_eq!(full[2], 5);
        assert_eq!(full[5], 2);
        assert_eq!(full[folder_extra_seat(2)], folder_extra_seat(5));
        assert_eq!(full[folder_extra_seat(5)], folder_extra_seat(2));
        for pedal in 0..AMP_PEDALS {
            assert_eq!(full[amp_pedal_seat(2, pedal)], amp_pedal_seat(5, pedal));
            assert_eq!(full[amp_pedal_seat(5, pedal)], amp_pedal_seat(2, pedal));
        }
        assert_eq!(full[MASTER_SEAT], MASTER_SEAT);
        assert_eq!(full[MASTER_EXTRA_SEAT], MASTER_EXTRA_SEAT);
        assert_eq!(
            (0..TOTAL_SEATS).filter(|i| full[*i] != *i).count(),
            4 + 2 * AMP_PEDALS,
            "the swap moved more than its own rows"
        );

        // And a SEAT-space permutation - the pad-FX swap's shape - passes
        // through instead of collapsing to identity, which silently parted
        // two pads' plugins from their state files.
        let (a, b) = (pad_rack_index(0, 3), pad_rack_index(0, 7));
        let mut pads: Vec<usize> = (0..TOTAL_SEATS.min(b + 1).max(b + 1)).collect();
        pads[a] = b;
        pads[b] = a;
        let lifted = seat_order(&pads);
        assert_eq!(lifted[a], b, "the pad swap was dropped");
        assert_eq!(lifted[b], a, "the pad swap was dropped");
    }

    /// **A folder-extra seat names its own channel, and nothing else does.**
    ///
    /// The pedalboard's whole fault was a seat that could not say whose it
    /// was. `folder_extra_seat` had no inverse, so four separate call sites
    /// asked `seat < CHANNELS` and read the answer as "does this seat get a
    /// board" — which is true of a channel's own seat, false of a folder's
    /// extra seat, and WRONG both times, because a folder's extra seat is a
    /// channel's. This pins the road back: every seat in the folder-extra
    /// block round-trips, every seat outside it answers `None`, and
    /// `pedal_owner` sends a channel to itself, a folder's extra rack to the
    /// channel that owns it and everything else nowhere.
    ///
    /// The exhaustive sweep is the point rather than a few spot checks. The
    /// three appended blocks (pads, folder-extras, pedals) sit next to each
    /// other, and an off-by-one at either end of the middle one would hand a
    /// folder's board to a drum pad or to another channel's pedal — a fault
    /// that makes sound in somebody else's track and looks like nothing at
    /// all in a spot check.
    #[test]
    fn a_folder_extra_seat_names_its_own_channel_and_nothing_else() {
        use super::{
            amp_pedal_seat, folder_extra_of, folder_extra_seat, pedal_owner, AMP_PEDALS,
            CHANNELS, MASTER_EXTRA_SEAT, MASTER_SEAT, TOTAL_SEATS,
        };
        for ch in 0..CHANNELS {
            assert_eq!(
                folder_extra_of(folder_extra_seat(ch)),
                Some(ch),
                "channel {ch}'s extra seat forgot whose it was"
            );
            assert_eq!(pedal_owner(folder_extra_seat(ch)), Some(ch));
            assert_eq!(pedal_owner(ch), Some(ch), "a channel is its own owner");
        }
        // Every seat in the whole space, one sweep: exactly the channels and
        // the folder-extra block answer, and each answers its own channel.
        for seat in 0..TOTAL_SEATS {
            let want = if seat < CHANNELS {
                Some(seat)
            } else {
                folder_extra_of(seat)
            };
            assert_eq!(
                pedal_owner(seat),
                want,
                "seat {seat} disagreed with the block it is in"
            );
        }
        // The named neighbours, spelled out because these are the ones a
        // reader will want to see: no board on the master, either rack, none
        // on a pad, none on a pedal seat itself.
        assert_eq!(folder_extra_of(0), None, "a channel is not an extra seat");
        assert_eq!(pedal_owner(MASTER_SEAT), None);
        assert_eq!(pedal_owner(MASTER_EXTRA_SEAT), None);
        assert_eq!(pedal_owner(folder_extra_seat(CHANNELS - 1) + 1), None);
        for ch in [0usize, CHANNELS - 1] {
            for pedal in 0..AMP_PEDALS {
                assert_eq!(
                    pedal_owner(amp_pedal_seat(ch, pedal)),
                    None,
                    "a pedal seat claimed a board of its own"
                );
            }
        }
        assert_eq!(pedal_owner(TOTAL_SEATS), None, "a seat past the end answered");
        assert_eq!(pedal_owner(usize::MAX), None);
    }

    /// **The ladder is a stereo pair, and the two bars are the CHANNELS.**
    ///
    /// A single bar said what the programme was doing and nothing about
    /// which side was doing it. On the two sliding windows the bars are
    /// measured per channel; on the integrated one they both answer with the
    /// programme's own reading, because an integrated figure is gated on the
    /// whole programme and a per-channel one would be invented.
    #[test]
    fn the_loudness_ladder_reads_as_a_stereo_pair() {
        use super::{Loudness, LoudnessWindow, LUFS_SILENCE};
        let l = Loudness {
            momentary: -14.0,
            short: -18.0,
            integrated: -16.0,
            window: LoudnessWindow::Momentary,
            // Left louder than right, and different again on the 3 s window.
            pair: [-15.0, -21.0, -19.0, -20.0],
        };
        let frac = |v: f32| {
            let (lo, hi) = LUFS_RANGE;
            (v - lo) / (hi - lo)
        };
        let near = |a: Option<f32>, b: f32| a.is_some_and(|x| (x - b).abs() < 1e-4);

        let [left, right] = l.bars();
        assert!(near(left, frac(-15.0)), "momentary left is {left:?}");
        assert!(near(right, frac(-21.0)), "momentary right is {right:?}");
        assert!(left > right, "the louder channel does not stand higher");

        let s = Loudness { window: LoudnessWindow::ShortTerm, ..l };
        let [left, right] = s.bars();
        assert!(near(left, frac(-19.0)));
        assert!(near(right, frac(-20.0)));

        // Integrated: both bars are the programme, and they AGREE — a
        // stereo meter with nothing to say about balance says the same
        // thing twice rather than making something up.
        let i = Loudness { window: LoudnessWindow::Integrated, ..l };
        let [left, right] = i.bars();
        assert_eq!(left, right);
        assert!(near(left, frac(-16.0)));

        // Silence draws nothing rather than a bar at the floor.
        let quiet = Loudness {
            pair: [LUFS_SILENCE; 4],
            ..l
        };
        assert_eq!(quiet.bars(), [None, None]);
    }
    use super::*;

    /// **The tempo holds only where a take saved it**, and the painter's view
    /// carries the same answer - one predicate, asked by both.
    #[test]
    fn the_tempo_holds_only_where_a_take_saved_it() {
        let case = |state: RecordState, counting: bool, exporting: bool| {
            let s = RecorderState {
                state,
                take_counting: counting,
                exporting,
                ..RecorderState::default()
            };
            assert_eq!(
                s.view("", false, None, Knobs::default(), false, None).tempo_held,
                s.tempo_held(),
                "the view disagrees in {state:?}"
            );
            s.tempo_held()
        };
        assert!(!case(RecordState::Idle, false, false));
        assert!(case(RecordState::Idle, true, false), "a count a take saved from");
        assert!(case(RecordState::CountIn { beat: 1, of: 4 }, false, false), "the take's own count");
        assert!(!case(RecordState::Rolling, false, false), "a rolling take holds the tempo");
        assert!(case(RecordState::Rolling, true, false), "a count recorded into a rolling take");
        assert!(case(RecordState::Rolling, false, true), "an export");
        assert!(!case(RecordState::Finishing, false, false));
    }

    #[test]
    fn a_take_that_writes_nothing_is_refused() {
        let mut s = ExportSpec::default();
        assert!(s.is_valid());
        s.audio = false;
        assert!(s.is_valid(), "MIDI alone is a perfectly good take");
        s.midi = false;
        assert_eq!(s.problem(), Some(SpecProblem::NothingToWrite));
        s.audio = true;
        assert!(s.is_valid());
    }

    #[test]
    fn a_composite_of_nothing_is_refused() {
        let mut s = ExportSpec {
            video: VideoMode::Composite,
            ..Default::default()
        };
        assert!(s.is_valid());
        // The layer rules are asked only with the window OFF: with it on
        // the picture is the window and the layers are already in it.
        s.composite.window = false;
        assert!(s.is_valid());
        s.composite.camera = false;
        s.composite.display = false;
        assert_eq!(s.problem(), Some(SpecProblem::EmptyComposite));
        // **And the window on its own is a whole video.** The owner's
        // "the camera is the window is now literal": with nothing else
        // ticked there is still something to record, and it is the window.
        s.composite.window = true;
        assert!(s.is_valid(), "a window capture with no layers was refused");
    }

    /// The subtle one: the display layer is on, so the composite is not empty,
    /// but every panel inside it is off — which composites a black rectangle
    /// over half the frame and looks like a bug in the encoder.
    #[test]
    fn a_display_layer_with_no_panels_is_refused() {
        let s = ExportSpec {
            video: VideoMode::Composite,
            composite: Composite {
                window: false,
                camera: false,
                display: true,
                shows: DisplayShows {
                    piano: false,
                    chord: false,
                    fretboard: false,
                    theory: false,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(s.problem(), Some(SpecProblem::EmptyDisplay));
    }

    /// Only meaningful when a composite is actually being made. A per-source
    /// export with no panels ticked is a camera-only video, which is fine.
    #[test]
    fn the_display_check_does_not_fire_without_a_composite() {
        let s = ExportSpec {
            video: VideoMode::PerSource,
            composite: Composite {
                display: true,
                shows: DisplayShows {
                    piano: false,
                    chord: false,
                    fretboard: false,
                    theory: false,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(s.is_valid());
    }

    #[test]
    fn an_export_spec_survives_a_round_trip() {
        let s = ExportSpec {
            audio: false,
            midi: true,
            tempo_bpm: 92.5,
            video: VideoMode::Both,
            composite: Composite {
                window: false,
                camera: false,
                display: true,
                audio: false,
                layout: Layout::SideBySide,
                shows: DisplayShows {
                    piano: false,
                    chord: true,
                    fretboard: true,
                    theory: true,
                },
            },
            resolution: Resolution::Hd720,
            fps: 60,
        };
        assert_eq!(ExportSpec::from_value(&s.to_value()), s);
    }

    /// The reason the reader validates rather than trusting: a hand-edited
    /// file, or one written by a future version, must not be able to put a
    /// value in that the dialog itself would refuse to produce.
    #[test]
    fn a_hostile_settings_file_cannot_inject_a_nonsense_spec() {
        let v: Value = serde_json::from_str(
            r#"{"tempo_bpm": 0.0001, "fps": 999, "video": "holograph",
                "layout": "diagonal", "resolution": "8k", "audio": "yes"}"#,
        )
        .unwrap();
        let s = ExportSpec::from_value(&v);
        assert_eq!(s, ExportSpec::default(), "every bad key fell back");
    }

    #[test]
    fn nothing_is_wrong_with_an_absent_export_block() {
        assert_eq!(ExportSpec::from_value(&Value::Null), ExportSpec::default());
    }

    /// Encoder count is what the dialog promises the CPU cost from, so it has
    /// to match the file count it also promises.
    #[test]
    fn the_encoder_count_matches_the_number_of_video_files() {
        let both_layers = Composite {
            camera: true,
            display: true,
            ..Default::default()
        };
        let cases = [
            (VideoMode::None, both_layers, 0),
            (VideoMode::Composite, both_layers, 1),
            (VideoMode::PerSource, both_layers, 2),
            (VideoMode::Both, both_layers, 3),
            (
                VideoMode::PerSource,
                Composite {
                    camera: true,
                    display: false,
                    ..Default::default()
                },
                1,
            ),
        ];
        for (mode, comp, want) in cases {
            let s = ExportSpec {
                video: mode,
                composite: comp,
                ..Default::default()
            };
            assert_eq!(s.encoder_count(), want, "{mode:?}");
        }
    }

    /// The bug this exists to stop coming back: record a take on a machine
    /// with no webcam, get a `.wav` and a `.mid` and no video, and have
    /// nothing anywhere explain that video was a setting.
    #[test]
    fn a_take_with_no_camera_still_writes_a_video() {
        assert!(
            ExportSpec::default().produces_video(false),
            "the window is the take"
        );
        assert!(ExportSpec::default().produces_video(true));

        // Off is still off, and a composite with nothing in it still writes
        // nothing — that is the case `begin_video` reports as an error.
        assert!(!ExportSpec {
            video: VideoMode::None,
            ..Default::default()
        }
        .produces_video(true));
        assert!(!ExportSpec {
            composite: Composite {
                window: false,
                camera: false,
                display: false,
                ..Default::default()
            },
            ..Default::default()
        }
        .produces_video(true));
        // The window alone is a video, camera or no camera — which is the
        // whole reason a take on a machine with no webcam has a picture now.
        assert!(ExportSpec {
            composite: Composite {
                window: true,
                camera: false,
                display: false,
                ..Default::default()
            },
            ..Default::default()
        }
        .produces_video(false));

        // Camera-only, no camera: nothing to encode. The window off, or it
        // would be a window-only take, which is the case two blocks up.
        let cam_only = ExportSpec {
            composite: Composite {
                window: false,
                display: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(cam_only.produces_video(true));
        assert!(!cam_only.produces_video(false));
    }

    /// The camera question decides what a *finished* take can be re-exported
    /// as, so getting it wrong offers the user an option that cannot work.
    #[test]
    fn needing_a_camera_follows_the_video_mode_and_the_tick() {
        let no_cam = Composite {
            camera: false,
            ..Default::default()
        };
        assert!(!ExportSpec {
            video: VideoMode::None,
            ..Default::default()
        }
        .needs_camera());
        assert!(ExportSpec {
            video: VideoMode::Composite,
            ..Default::default()
        }
        .needs_camera());
        assert!(!ExportSpec {
            video: VideoMode::Composite,
            composite: no_cam,
            ..Default::default()
        }
        .needs_camera());
    }

    #[test]
    fn audio_only_costs_what_the_arithmetic_says() {
        let s = ExportSpec {
            audio: true,
            midi: false,
            video: VideoMode::None,
            ..Default::default()
        };
        // 48k * 3 bytes * 2ch * 60s = 17.28 MB per minute.
        assert!((s.megabytes_per_minute() - 17.28).abs() < 0.01);
    }

    #[test]
    fn a_take_with_no_content_reports_no_disk_duration() {
        let s = ExportSpec {
            audio: false,
            midi: false,
            video: VideoMode::None,
            ..Default::default()
        };
        assert_eq!(minutes_on_disk(500_000_000_000, &s), None);
    }

    #[test]
    fn the_transport_grows_an_hours_field_when_it_needs_one() {
        assert_eq!(timecode(0.0), "0:00");
        assert_eq!(timecode(9.7), "0:09");
        assert_eq!(timecode(61.0), "1:01");
        assert_eq!(timecode(599.0), "9:59");
        assert_eq!(timecode(3599.0), "59:59");
        assert_eq!(timecode(3600.0), "1:00:00");
        assert_eq!(timecode(7384.0), "2:03:04");
        assert_eq!(timecode(-5.0), "0:00", "a negative clock reads zero");
    }

    #[test]
    fn disk_space_reads_as_a_duration_at_every_scale() {
        assert_eq!(disk_text(0.4), "under a minute");
        assert_eq!(disk_text(58.2), "~58 min");
        assert_eq!(disk_text(134.0), "~2 h 14 min");
    }

    #[test]
    fn a_missing_device_is_not_the_same_as_no_device() {
        assert!(!DeviceLabel::Open("FaceTime HD").is_missing());
        assert!(DeviceLabel::Missing("FaceTime HD").is_missing());
        assert_eq!(DeviceLabel::None.text(), "None");
        assert_eq!(DeviceLabel::Missing("Scarlett").text(), "Scarlett");
    }

    /// The count-in is not writing. If it were, every take would open with two
    /// bars of clicking.
    #[test]
    fn the_count_in_is_active_but_not_writing() {
        let p = RecordState::CountIn { beat: 2, of: 4 };
        assert!(p.is_active() && !p.is_writing());
        assert!(RecordState::Rolling.is_writing());
        assert!(RecordState::Finishing.is_writing());
        assert!(!RecordState::Idle.is_active());
    }
}

#[cfg(test)]
mod fader_tests {
    use super::*;

    /// A fader pulled all the way down has to be OFF. `-60 dB` is not off when
    /// what it is attenuating is a piano recorded at full scale — it is quiet,
    /// audible, and on the recording.
    #[test]
    fn the_bottom_of_a_fader_is_silence_and_not_merely_quiet() {
        assert_eq!(fader_to_gain(0.0), 0.0);
        assert_eq!(fader_to_gain(-1.0), 0.0, "and below the bottom, too");
        assert_eq!(gain_text(0.0), "OFF");
    }

    #[test]
    fn a_fader_round_trips_through_its_position() {
        for p in [0.05_f32, 0.25, 0.5, 0.8333333, 1.0] {
            let back = gain_to_fader(fader_to_gain(p));
            assert!(
                (back - p).abs() < 1e-4,
                "position {p} came back as {back}"
            );
        }
    }

    /// The one position anybody actually looks for.
    #[test]
    fn unity_gain_is_reachable_and_reads_as_zero_db() {
        let unity = gain_to_fader(1.0);
        assert!((fader_to_gain(unity) - 1.0).abs() < 1e-4);
        assert_eq!(gain_text(1.0), "+0.0 dB");
        assert!(
            unity > 0.0 && unity < 1.0,
            "0 dB has to be somewhere on the travel, not at an end"
        );
    }

    #[test]
    fn a_gain_above_unity_is_reachable_because_quiet_sources_exist() {
        assert!(fader_to_gain(1.0) > 1.0, "the scale runs past 0 dB");
        assert_eq!(gain_text(fader_to_gain(1.0)), "+12.0 dB");
    }

    /// The default that ruins takes if it is wrong.
    #[test]
    fn the_click_starts_under_the_music_and_out_of_the_file() {
        let k = Knobs::default();
        assert!(
            k.gains.channels.iter().all(|g| k.gains.metronome < *g),
            "the click starts under EVERY channel, not just the first"
        );
        assert!(!k.metronome_in_take, "a click in the file is a ruined take");
        assert!(!k.metronome_on, "and it does not start clicking on its own");
    }
}

#[cfg(test)]
mod typing_tests {
    use super::*;

    /// The two directions have to agree, or a field would show one number and
    /// accept a different one for it.
    #[test]
    fn what_a_fader_shows_is_what_it_will_read_back() {
        for pos in 0..=100 {
            let gain = fader_to_gain(pos as f32 / 100.0);
            if gain <= 0.0 {
                continue;
            }
            let shown = gain_text(gain);
            // The user retypes what they can see, which includes the unit and
            // the sign — both have to survive the round trip.
            let back = parse_gain(shown.trim_end_matches(" dB")).expect("that was a level");
            let db_in = 20.0 * gain.log10();
            let db_out = 20.0 * back.log10();
            assert!(
                (db_in - db_out).abs() < 0.06,
                "{shown} read back as {db_out:+.3} dB, not {db_in:+.3}"
            );
        }
    }

    #[test]
    fn a_level_at_or_below_the_bottom_of_the_fader_is_off() {
        // The only way to type OFF, and the one people reach for: "-60" and
        // "as quiet as it goes" are the same intention.
        assert_eq!(parse_gain("-60"), Some(0.0));
        assert_eq!(parse_gain("-99"), Some(0.0));
        assert_eq!(parse_gain("-inf"), Some(0.0));
    }

    #[test]
    fn a_level_above_the_top_is_pinned_rather_than_refused() {
        let g = parse_gain("40").expect("a number is a number");
        let db = 20.0 * g.log10();
        assert!(
            (db - f64::from(GAIN_MAX_DB) as f32).abs() < 1e-3,
            "{db} should have been pinned to {GAIN_MAX_DB}"
        );
    }

    #[test]
    fn text_that_is_not_a_number_changes_nothing() {
        // `None` and not a default: committing junk must leave the value the
        // user was already looking at exactly where it was.
        for junk in ["", "   ", "loud", "-", ".", "12x"] {
            assert_eq!(parse_gain(junk), None, "{junk:?} is not a level");
            assert_eq!(parse_bpm(junk), None, "{junk:?} is not a tempo");
        }
    }

    #[test]
    fn a_tempo_is_clamped_to_what_the_smf_writer_can_say() {
        assert_eq!(parse_bpm("120"), Some(120.0));
        assert_eq!(parse_bpm("0"), Some(MIN_BPM));
        assert_eq!(parse_bpm("100000"), Some(MAX_BPM));
        assert_eq!(parse_bpm("-4"), Some(MIN_BPM));
        // NaN and the infinities parse as an f64 and would sail through a
        // range check, because every comparison against NaN is false — and
        // `clamp` PANICS when handed one. Refused outright rather than
        // clamped: neither is reachable from the keyboard (the field drops
        // letters) and neither is a tempo.
        assert_eq!(parse_bpm("NaN"), None);
        assert_eq!(parse_bpm("inf"), None);
        assert_eq!(parse_bpm("-inf"), None);
    }

    /// A field with keyboard focus is a field the app's single-key shortcuts
    /// are typing into. Anything that is not part of a number has to bounce.
    #[test]
    fn the_field_refuses_everything_that_is_not_part_of_a_number() {
        let mut e = NumEdit::new(NumField::Tempo);
        for ch in "r1e2c3".chars() {
            e.push(ch);
        }
        assert_eq!(e.text, "123", "letters reached the field");
    }

    #[test]
    fn one_point_and_a_minus_only_in_front() {
        let mut e = NumEdit::new(NumField::Master);
        for ch in "-1.2.3-".chars() {
            e.push(ch);
        }
        assert_eq!(e.text, "-1.23");
        assert!(parse_gain(&e.text).is_some(), "and it parses");
    }

    #[test]
    fn a_field_starts_empty_so_the_first_digit_replaces() {
        // Seeded with the current reading, "+0.0 dB", somebody wanting -6 would
        // have to delete seven characters before typing anything.
        assert_eq!(NumEdit::new(NumField::Master).text, "");
    }

    #[test]
    fn a_field_cannot_be_typed_into_forever() {
        let mut e = NumEdit::new(NumField::Tempo);
        for _ in 0..200 {
            e.push('9');
        }
        assert!(e.text.len() <= 8, "the box grew to {}", e.text.len());
        // And what is in it is still a tempo rather than an overflow.
        assert_eq!(parse_bpm(&e.text), Some(MAX_BPM));
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use egui::{Pos2, Rect};

    /// The default panel selection — piano and chord name — as a width:height
    /// ratio. Measured from `band_sizes_at`: at 1300 wide the piano is 150 and
    /// the chord strip is 50, so the content is 1300:200.
    const PIANO_AND_CHORD: f32 = 1300.0 / 200.0;
    /// What almost every camera on a desk is, and what these tests assume
    /// unless they are about a camera that is not.
    const CAM_16_9: f32 = 16.0 / 9.0;

    fn landscape() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))
    }
    fn portrait() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1080.0, 1920.0))
    }

    /// Every layer that IS in the composite has somewhere to go, and every
    /// layer that is not has nowhere. A pane for a layer nobody asked for is a
    /// black rectangle in the finished video.
    #[test]
    fn a_layer_gets_a_pane_exactly_when_it_is_in_the_composite() {
        for l in Layout::ALL {
            for frame in [landscape(), portrait()] {
                for (cam, disp) in [(true, true), (true, false), (false, true), (false, false)] {
                    let p = l.split(frame, cam.then_some(CAM_16_9), disp.then_some(PIANO_AND_CHORD));
                    assert_eq!(p.camera.is_some(), cam, "{l:?} camera");
                    assert_eq!(p.display.is_some(), disp, "{l:?} display");
                }
            }
        }
    }

    /// The only layer in the video gets the WHOLE video. Anything else is a
    /// letterbox around the one thing the user asked for.
    #[test]
    fn one_layer_alone_fills_the_frame() {
        for l in Layout::ALL {
            let f = landscape();
            assert_eq!(l.split(f, Some(CAM_16_9), None).camera, Some(f), "{l:?}");
            assert_eq!(l.split(f, None, Some(PIANO_AND_CHORD)).display, Some(f), "{l:?}");
        }
    }

    /// Stacked panes must tile the frame: no gap, no overlap, nothing outside.
    /// A gap is a black stripe and an overlap is a layer eating another.
    #[test]
    fn stacked_panes_tile_the_frame_exactly() {
        for l in [Layout::CameraAbove, Layout::DisplayAbove, Layout::SideBySide] {
            for frame in [landscape(), portrait()] {
                let p = l.split(frame, Some(CAM_16_9), Some(PIANO_AND_CHORD));
                let (c, d) = (p.camera.expect("camera"), p.display.expect("display"));
                assert!(frame.contains_rect(c) && frame.contains_rect(d), "{l:?} escaped");
                let covered = c.area() + d.area();
                assert!(
                    (covered - frame.area()).abs() < 1.0,
                    "{l:?} covers {covered} of {}",
                    frame.area()
                );
                assert!(!l.overlays(), "{l:?} should not be an overlay");
            }
        }
    }

    /// **The display band fits its content, and the camera gets everything
    /// else.**
    ///
    /// The first version handed the band a flat fraction — 30% landscape, 40%
    /// portrait — and the very first composited frame showed the cost: in a
    /// 360x640 vertical frame the keyboard is 55 points tall and it was given
    /// 256, so five sixths of the band was black and the camera had lost a
    /// quarter of the picture to hold it.
    #[test]
    fn the_display_band_is_the_height_of_what_goes_in_it() {
        for frame in [landscape(), portrait()] {
            let d = Layout::CameraAbove
                .split(frame, Some(CAM_16_9), Some(PIANO_AND_CHORD))
                .display
                .expect("display");
            let natural = frame.width() / PIANO_AND_CHORD;
            assert!(
                (d.height() - natural).abs() < 1.0,
                "the band is {} tall for content that is {natural}",
                d.height()
            );
        }
    }

    /// And it is still CAPPED, because the band grows with every panel switched
    /// on. A fretboard and three theory diagrams must not take the whole frame.
    #[test]
    fn a_very_tall_display_is_capped_rather_than_swallowing_the_frame() {
        // Aspect 1.0 is a square display, far taller than any real selection.
        let f = landscape();
        let d = Layout::CameraAbove
            .split(f, Some(CAM_16_9), Some(1.0))
            .display
            .expect("display");
        assert!(
            d.height() <= f.height() * 0.41,
            "an outsized display took {} of {}",
            d.height(),
            f.height()
        );
        assert!(
            f.contains_rect(d),
            "and it must still be inside the frame"
        );
    }

    /// A vertical frame gives the camera nearly all of itself, which is the
    /// whole reason 9:16 is worth offering: a near-square camera pane is a far
    /// better crop of a person at a piano than 16:9 ever gives.
    #[test]
    fn the_vertical_camera_gets_the_bulk_of_the_frame() {
        let f = portrait();
        let p = Layout::CameraAbove.split(f, Some(CAM_16_9), Some(PIANO_AND_CHORD));
        let cam = p.camera.expect("camera");
        let share = cam.height() / f.height();
        assert!(
            share > 0.80,
            "the camera got {share} of a vertical frame - the keyboard does not \
             need the rest"
        );
    }

    /// In 9:16 the camera pane must stay a usable shape for a person. This is
    /// the assertion that justifies offering vertical at all.
    #[test]
    fn the_vertical_camera_pane_is_not_a_letterbox_slot() {
        let cam = Layout::CameraAbove
            .split(portrait(), Some(CAM_16_9), Some(PIANO_AND_CHORD))
            .camera
            .expect("camera");
        let aspect = cam.width() / cam.height();
        // Portrait-ish, and that is right: a 9:16 frame with a short keyboard
        // strip under it leaves the camera a tall pane, which frames a seated
        // player from head to hands. The bound that matters is that it is not a
        // LETTERBOX SLOT — nothing like the 9:32 sliver a side-by-side split
        // would give it.
        assert!(
            (0.45..=1.6).contains(&aspect),
            "a {aspect:.2}:1 camera pane is not a crop anybody wants of a person"
        );
    }

    /// Side by side in portrait would be two 9:32 slivers, so it stacks.
    /// Silently giving the arrangement that works beats giving the one that was
    /// literally asked for and letting the user find out at the export.
    #[test]
    fn side_by_side_stacks_when_the_frame_is_vertical() {
        let p = Layout::SideBySide.split(portrait(), Some(CAM_16_9), Some(PIANO_AND_CHORD));
        let stacked = Layout::CameraAbove.split(portrait(), Some(CAM_16_9), Some(PIANO_AND_CHORD));
        assert_eq!(p, stacked, "side by side should have stacked in 9:16");
        // And in landscape it is genuinely side by side.
        let wide = Layout::SideBySide.split(landscape(), Some(CAM_16_9), Some(PIANO_AND_CHORD));
        let (c, d) = (wide.camera.unwrap(), wide.display.unwrap());
        assert!((c.height() - landscape().height()).abs() < 1.0);
        assert!(c.right() <= d.left() + 1.0, "they did not sit side by side");
    }

    /// The overlay keeps the camera whole and floats the display over it, which
    /// is the one case where the panes are SUPPOSED to intersect.
    #[test]
    fn the_overlay_keeps_the_camera_whole_and_sits_on_top_of_it() {
        let f = landscape();
        let p = Layout::CameraFull.split(f, Some(CAM_16_9), Some(PIANO_AND_CHORD));
        assert_eq!(p.camera, Some(f), "the camera should still fill the frame");
        let d = p.display.expect("display");
        assert!(f.contains_rect(d));
        assert!(d.intersects(f));
        assert!(Layout::CameraFull.overlays(), "the compositor must paint it last");
        assert!(
            d.height() < f.height() * 0.30,
            "an overlay covering a third of the picture is not an overlay"
        );
    }

    /// **The default puts the APP on screen and the camera in the corner.**
    ///
    /// It was the other way round, and that made the thing worth watching the
    /// secondary one: a fretboard and three theory diagrams squeezed into a
    /// band under a webcam are too small to read, which is the whole reason
    /// anybody would record them.
    #[test]
    fn the_default_layout_gives_the_display_the_frame() {
        assert_eq!(Layout::default(), Layout::DisplayFull);
        for frame in [landscape(), portrait()] {
            let p = Layout::DisplayFull.split(frame, Some(CAM_16_9), Some(PIANO_AND_CHORD));
            assert_eq!(
                p.display,
                Some(frame),
                "the app should have the whole frame"
            );
            let cam = p.camera.expect("camera");
            assert!(frame.contains_rect(cam), "the inset escaped the frame");
            // **The SENSOR's shape**, whatever that is, because the compositor
            // centre-crops the camera into whatever pane it is handed — so a
            // pane of the wrong shape silently throws away the sides or the top
            // and bottom of every frame the camera delivers.
            for sensor in [CAM_16_9, 4.0 / 3.0, 1.0] {
                let c = Layout::DisplayFull
                    .split(frame, Some(sensor), Some(PIANO_AND_CHORD))
                    .camera
                    .expect("camera");
                assert!(
                    (c.width() / c.height() - sensor).abs() < 0.01,
                    "a {sensor}:1 sensor got a {}:1 pane",
                    c.width() / c.height()
                );
            }
            // Small enough to be context, big enough to be a person — and the
            // SAME apparent size in both frames, which is the whole reason the
            // height comes off the short edge. A fraction of the width would be
            // a quarter of a 16:9 frame and a fifteenth of a 9:16 one.
            let short = frame.width().min(frame.height());
            let share = cam.height() / short;
            assert!(
                (0.14..=0.22).contains(&share),
                "the inset is {share} of the short edge tall"
            );
            // It cannot walk across the picture however wide the camera is.
            let widest = Layout::DisplayFull
                .split(frame, Some(40.0), Some(PIANO_AND_CHORD))
                .camera
                .expect("camera");
            assert!(
                widest.width() <= frame.width() * 0.34,
                "a very wide camera took {} of the frame",
                widest.width() / frame.width()
            );
            // Clear of the edges, and in the TOP right: the keyboard and the
            // fretboard run full width along the bottom, and the top right is
            // the end of the theory band, which is where the air is.
            assert!(cam.right() < frame.right() && cam.top() > frame.top());
            assert!(cam.left() > frame.center().x && cam.bottom() < frame.center().y);
        }
    }

    /// The compositor has to know WHICH layer floats. `CameraFull` puts the
    /// display over the camera; `DisplayFull` puts the camera over the display.
    /// Painting the camera first regardless buried the inset under the app,
    /// which is the same as not drawing it.
    #[test]
    fn the_overlaid_layer_is_named_and_the_two_full_frames_disagree() {
        assert!(Layout::CameraFull.overlays());
        assert!(!Layout::CameraFull.camera_on_top());
        assert!(Layout::DisplayFull.overlays());
        assert!(Layout::DisplayFull.camera_on_top());
        for l in [Layout::CameraAbove, Layout::DisplayAbove, Layout::SideBySide] {
            assert!(!l.overlays(), "{l:?} is stacked, not overlaid");
            assert!(!l.camera_on_top(), "{l:?} floats nothing");
        }
    }

    /// A degenerate frame must not produce a pane with negative size, which is
    /// what a compositor would then try to render into.
    #[test]
    fn a_frame_with_no_area_produces_no_impossible_panes() {
        for w in [0.0_f32, 1.0] {
            for h in [0.0_f32, 1.0] {
                let f = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
                for l in Layout::ALL {
                    let p = l.split(f, Some(CAM_16_9), Some(PIANO_AND_CHORD));
                    for r in [p.camera, p.display].into_iter().flatten() {
                        assert!(
                            r.width() >= 0.0 && r.height() >= 0.0,
                            "{l:?} produced {r:?} from {f:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_vertical_resolution_is_a_real_portrait_frame() {
        assert_eq!(Resolution::Vertical1080.pixels(), Some((1080, 1920)));
        assert!(Resolution::Vertical1080.is_portrait(None));
        assert!(!Resolution::Hd1080.is_portrait(None));
        // And a phone held upright, with MatchCamera, is portrait too.
        assert!(Resolution::MatchCamera.is_portrait(Some((1080, 1920))));
        assert!(!Resolution::MatchCamera.is_portrait(Some((1920, 1080))));
    }

    /// Every resolution round-trips through its settings key, or a saved
    /// vertical export silently becomes 1080p the next time the app starts.
    #[test]
    fn every_resolution_survives_the_settings_file() {
        for r in Resolution::ALL {
            assert_eq!(Resolution::from_key(r.key()), Some(r), "{r:?}");
        }
        for l in Layout::ALL {
            assert_eq!(Layout::from_key(l.key()), Some(l), "{l:?}");
        }
    }
}

#[cfg(test)]
mod meter_tests {
    use super::*;

    /// **The tempo counts quarters, whatever the unit is.**
    ///
    /// Forced by the file format: an SMF tempo meta event is microseconds per
    /// quarter note and nothing else, so any other reading makes the `.mid`
    /// disagree with the click in every bar.
    #[test]
    fn a_beat_is_measured_against_the_quarter_note() {
        let bpm = 120.0;
        // A quarter at 120 is half a second, and 4/4 is four of them.
        let four_four = TimeSignature { beats: 4, unit: 4 };
        assert!((four_four.beat_seconds(bpm) - 0.5).abs() < 1e-12);

        // An eighth is half of that, so a bar of 6/8 is a second and a half.
        let six_eight = TimeSignature { beats: 6, unit: 8 };
        assert!((six_eight.beat_seconds(bpm) - 0.25).abs() < 1e-12);
        let bar = six_eight.beat_seconds(bpm) * f64::from(six_eight.beats);
        assert!((bar - 1.5).abs() < 1e-12, "a 6/8 bar at 120 is 1.5s, got {bar}");

        // And a half-note gets two quarters.
        let cut = TimeSignature { beats: 2, unit: 2 };
        assert!((cut.beat_seconds(bpm) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_signature_is_two_numbers_and_a_note_value() {
        assert!(TimeSignature { beats: 4, unit: 4 }.is_valid());
        assert!(TimeSignature { beats: 7, unit: 8 }.is_valid());
        assert!(TimeSignature { beats: 1, unit: 32 }.is_valid());
        // The unit has to be a power of two, because that is what a note value
        // IS — and because the SMF meta stores it as a power.
        assert!(!TimeSignature { beats: 4, unit: 6 }.is_valid());
        assert!(!TimeSignature { beats: 4, unit: 0 }.is_valid());
        assert!(!TimeSignature { beats: 0, unit: 4 }.is_valid());
        assert!(!TimeSignature { beats: 33, unit: 4 }.is_valid());
    }

    /// The SMF writes the unit as a POWER of two: 4 is 2, 8 is 3. Getting it
    /// wrong writes a file every DAW reads as a different signature, and a bar
    /// line in the wrong place is not something anybody notices until later.
    #[test]
    fn the_smf_meta_stores_the_unit_as_a_power_of_two() {
        for (unit, power) in [(1, 0), (2, 1), (4, 2), (8, 3), (16, 4), (32, 5)] {
            let sig = TimeSignature { beats: 4, unit };
            assert_eq!(sig.unit_power(), power, "{unit} should write as {power}");
        }
    }

    /// Every offered signature is playable, and round-trips through the text
    /// the settings file and the menu both use.
    #[test]
    fn every_offered_signature_survives_the_settings_file() {
        for sig in TIME_SIGNATURES {
            assert!(sig.is_valid(), "{} is offered and unplayable", sig.label());
            assert_eq!(TimeSignature::parse(&sig.label()), Some(sig));
        }
        // And a custom one the menu never offered.
        assert_eq!(
            TimeSignature::parse("11/16"),
            Some(TimeSignature { beats: 11, unit: 16 })
        );
        assert_eq!(TimeSignature::parse(" 3 / 4 "), Some(TimeSignature { beats: 3, unit: 4 }));
        // Nonsense is None rather than a guess.
        for bad in ["", "4", "4/", "/4", "4/6", "0/4", "x/y", "4/4/4"] {
            assert_eq!(TimeSignature::parse(bad), None, "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_count_in_is_bars_times_the_top_number() {
        assert_eq!(TimeSignature { beats: 4, unit: 4 }.beats_in(2), 8);
        assert_eq!(TimeSignature { beats: 6, unit: 8 }.beats_in(1), 6);
        assert_eq!(TimeSignature { beats: 7, unit: 8 }.beats_in(2), 14);
        assert_eq!(TimeSignature { beats: 4, unit: 4 }.beats_in(0), 0);
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn side(rate: u32, frames: Option<u32>) -> (String, StreamStats) {
        (
            "device".to_owned(),
            StreamStats {
observed_frames: None,
                buffer_ring: None,
                sample_rate: rate,
                channels: 2,
                buffer_frames: frames,
            },
        )
    }

    #[test]
    fn a_buffer_is_reported_in_milliseconds_of_its_own_rate() {
        // 256 frames at 48k is 5.333 ms; at 44.1k it is longer, which is the
        // whole reason the rate is part of the sum.
        let at48 = StreamStats {
observed_frames: None,
            buffer_ring: None,
            sample_rate: 48_000,
            channels: 2,
            buffer_frames: Some(256),
        };
        assert!((at48.buffer_ms().unwrap() - 5.3333).abs() < 1e-3);
        let at44 = StreamStats {
observed_frames: None,
            buffer_ring: None,
            sample_rate: 44_100,
            channels: 2,
            buffer_frames: Some(256),
        };
        assert!(at44.buffer_ms().unwrap() > at48.buffer_ms().unwrap());

        // A device that chose its own size did not say what it chose, and
        // inventing a number here would be inventing a latency figure.
        assert_eq!(
            StreamStats {
observed_frames: None,
                buffer_ring: None,
                sample_rate: 48_000,
                channels: 2,
                buffer_frames: None,
            }
            .buffer_ms(),
            None
        );
    }

    /// The round trip is BOTH sides, and absent when either is.
    #[test]
    fn the_round_trip_needs_both_halves() {
        let both = AudioStatus {
            input: Some(side(48_000, Some(256))),
            output: Some(side(48_000, Some(512))),
            monitor_ms: None,
            graph_floor: None,
            dropouts: None,
            rt_load: None,
            probe: None,
        };
        assert!((both.round_trip_ms().unwrap() - 16.0).abs() < 1e-3);

        // **And the ring between them counts.** Two separate streams pass the
        // input to the output through a ring, and whatever it is holding is
        // latency as surely as a buffer is — the estimate said 16 ms while the
        // room said otherwise, which is how the whole thing was found.
        let with_ring = AudioStatus {
            monitor_ms: Some(4.0),
            ..both
        };
        assert!((with_ring.round_trip_ms().unwrap() - 20.0).abs() < 1e-3);

        for one in [
            AudioStatus {
                input: Some(side(48_000, Some(256))),
                output: None,
                monitor_ms: None,
                graph_floor: None,
                dropouts: None,
                rt_load: None,
                probe: None,
            },
            AudioStatus {
                input: None,
                output: Some(side(48_000, Some(256))),
                monitor_ms: None,
                graph_floor: None,
                dropouts: None,
                rt_load: None,
                probe: None,
            },
            // And a side that will not say its buffer size cannot contribute
            // half a figure.
            AudioStatus {
                input: Some(side(48_000, None)),
                output: Some(side(48_000, Some(256))),
                monitor_ms: None,
                graph_floor: None,
                dropouts: None,
                rt_load: None,
                probe: None,
            },
        ] {
            assert_eq!(one.round_trip_ms(), None);
        }
    }

    /// **A rate mismatch is a fault, not a curiosity.**
    ///
    /// The writer drains the instrument's ring at the INPUT's rate while the
    /// engine fills it at the OUTPUT's, so a mismatch overflows that ring and
    /// reports the losses against the take — a symptom that points nowhere near
    /// its cause, which is exactly why the panel says it out loud.
    #[test]
    fn a_rate_mismatch_is_noticed() {
        assert!(AudioStatus {
            input: Some(side(44_100, Some(256))),
            output: Some(side(48_000, Some(256))),
            monitor_ms: None,
            graph_floor: None,
            dropouts: None,
            rt_load: None,
            probe: None,
        }
        .rates_disagree());

        assert!(!AudioStatus {
            input: Some(side(48_000, Some(256))),
            output: Some(side(48_000, Some(512))),
            monitor_ms: None,
            graph_floor: None,
            dropouts: None,
            rt_load: None,
            probe: None,
        }
        .rates_disagree());

        // One side alone cannot disagree with anything.
        assert!(!AudioStatus {
            input: Some(side(44_100, Some(256))),
            output: None,
            monitor_ms: None,
            graph_floor: None,
            dropouts: None,
            rt_load: None,
            probe: None,
        }
        .rates_disagree());
    }
}

#[cfg(test)]
mod missing_tests {
    use super::{missing_from_take, Strip};

    /// **The one that costs a take, so the sentence has to be an
    /// instruction.**
    ///
    /// Mute is now the only thing that decides what a file is made of. That is
    /// one rule instead of four, and worth it — but a rule nobody is told
    /// about is exactly the setting it replaced, which cost the owner twelve
    /// takes across three releases.
    #[test]
    fn it_names_what_is_lost_and_where_to_fix_it() {
        assert_eq!(missing_from_take(&[]), None, "nothing muted, nothing to say");

        let one = missing_from_take(&[Strip::Channel(0)]).expect("a muted channel is news");
        assert!(
            one.contains("channel 1 is muted") && one.contains("will not have it"),
            "{one}"
        );
        assert!(
            one.contains("Tab"),
            "it has to say where the mixer is, or it is a complaint rather than \
             an instruction: {one}"
        );

        // A list anybody would say out loud, and the verb agreeing with it.
        // "instrument 2 are muted" is the kind of thing that makes a person
        // trust the rest of the line less.
        let many = missing_from_take(&[Strip::Channel(1), Strip::Channel(0), Strip::Channel(4)])
            .expect("three of them is still news");
        assert!(
            many.contains("channel 2, channel 1 and channel 5"),
            "{many}"
        );
        assert!(many.contains("are muted") && many.contains("have them"), "{many}");
    }

    /// A channel says WHICH channel. "the channel is muted" on a desk of
    /// eight is not something anybody can act on.
    #[test]
    fn a_channel_says_which_one() {
        for i in 0..crate::recorder::CHANNELS {
            let said = Strip::Channel(i).label();
            assert_eq!(said, format!("channel {}", i + 1));
        }
    }
}

/// A silent scope for views with no app behind them — `RecorderView::empty`
/// and the base the app's struct-update overrides.
fn resting_scope() -> &'static crate::scope::Scope {
    static SCOPE: std::sync::OnceLock<crate::scope::Scope> = std::sync::OnceLock::new();
    SCOPE.get_or_init(|| crate::scope::Scope::new(48_000.0))
}

/// A silent analyzer for the same views: its scale and nothing on it.
fn resting_analyzer() -> &'static crate::analyzer::Analyzer {
    static A: std::sync::OnceLock<crate::analyzer::Analyzer> = std::sync::OnceLock::new();
    A.get_or_init(|| crate::analyzer::Analyzer::new(48_000.0))
}

/// The column and strip types, and the scan that keeps the views off
/// strip-index arithmetic (docs/design/unlimited-tracks.md, W2-views-windowed).
#[cfg(test)]
mod strip_ref_tests {
    use super::*;

    /// **A column, its strip and its desk slot agree, by name.** The
    /// aliasing pin re-pinned on the enum: the click is the click's column
    /// and slot, the master the master's, and every track its own row in
    /// all three spaces. `Column(usize)` called the click's desk index the
    /// master's column; no number links the two any more.
    #[test]
    fn a_column_a_strip_and_a_desk_slot_name_the_same_thing() {
        for col in Column::every(CHANNELS) {
            let r = col.strip_ref();
            assert_eq!(r.column(), Some(col), "{col} does not come back from its StripRef");
            assert_eq!(col.desk_index(), r.desk_index());
            assert_eq!(StripRef::from_desk_index(r.desk_index()), Some(r));
            assert_eq!(col.strip(), r.strip(), "{col} and its StripRef name different strips");
            if let Some(s) = col.strip() {
                assert_eq!(s.column_of(), Some(col));
                assert_eq!(s.index(), col.desk_index());
                assert_eq!(StripRef::from(s), r);
            }
        }
        assert_eq!(Column::Click.strip(), Some(Strip::Click));
        assert_eq!(Column::Master.strip(), None);
        assert_eq!(Column::Master.desk_index(), STRIPS);
        assert_eq!(Column::Click.desk_index(), Strip::Click.index());
        assert_ne!(Column::Click.desk_index(), Column::Master.desk_index());
        // The two buses with no column have none.
        assert_eq!(Strip::Drums.column_of(), None);
        assert_eq!(Strip::Fx.column_of(), None);
        // A track column past today's desk is still THAT track: the old
        // `Column(n)` answered "the master" for any n past the click.
        assert_eq!(Column::Track(40).strip(), Some(Strip::Channel(40)));
        assert_eq!(Column::Track(40).row(), Some(40));
        // Drawn order: every track, then the click, then the master.
        let order: Vec<Column> = Column::every(3).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
        assert_eq!(
            order,
            vec![Column::Track(0), Column::Track(1), Column::Track(2), Column::Click, Column::Master]
        );
    }

    /// **Every desk slot is exactly one StripRef**, and the automation lane
    /// space keeps the master apart from the drum bus it shares a number
    /// with in the desk arrays.
    #[test]
    fn every_desk_slot_is_one_strip_ref_and_the_auto_lanes_keep_the_master_apart() {
        let all: Vec<StripRef> = (0..=STRIPS).map(|i| {
            StripRef::from_desk_index(i).unwrap_or_else(|| panic!("slot {i} is nobody"))
        }).collect();
        for (i, r) in all.iter().enumerate() {
            assert_eq!(r.desk_index(), i);
        }
        assert_eq!(StripRef::from_desk_index(STRIPS + 1), None);
        assert_eq!(StripRef::MASTER_INDEX, STRIPS);
        assert_eq!(StripRef::Master.auto_lane(), Some(CHANNELS));
        assert_eq!(StripRef::Drums.auto_lane(), None, "the drum bus took the master's lane");
        assert_eq!(StripRef::from_auto_lane(CHANNELS), Some(StripRef::Master));
        assert_eq!(StripRef::from_auto_lane(3), Some(StripRef::Row(3)));
        assert_eq!(StripRef::from_auto_lane(CHANNELS + 1), None);
    }

    /// The code a scan reads: comments dropped, and every top-level
    /// `#[cfg(test)]` module cut out wherever it stands (the views' tests
    /// build fixtures and may say whatever they like; `recorder_panel.rs`
    /// has production code after its first one, so cutting at the first
    /// would hide it). A top-level module ends at the first `}` in column
    /// zero, which is rustfmt's shape and saves counting braces through
    /// strings and comments.
    fn code_of(src: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        let mut out = Vec::new();
        let mut k = 0;
        while k < lines.len() {
            let next = lines.get(k + 1).copied().unwrap_or("");
            if lines[k].trim_end() == "#[cfg(test)]"
                && (next.starts_with("mod ") || next.starts_with("pub(crate) mod "))
            {
                k += 2;
                while k < lines.len() && lines[k].trim_end() != "}" {
                    k += 1;
                }
                k += 1;
                continue;
            }
            out.push(lines[k].split("//").next().unwrap_or(""));
            k += 1;
        }
        out.join("\n")
    }

    /// One function's body out of a source file, by its signature's start.
    fn body_of<'a>(src: &'a str, sig: &str) -> &'a str {
        let at = src.find(sig).unwrap_or_else(|| panic!("`{sig}` is gone; re-anchor the scan"));
        let open = at + src[at..].find('{').expect("a body");
        let mut depth = 0usize;
        for (k, c) in src[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &src[at..open + k + 1];
                    }
                }
                _ => {}
            }
        }
        panic!("`{sig}` never closes")
    }

    /// What the scan refuses in view code, and the reason for each.
    fn banned_in(code: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (n, line) in code.lines().enumerate() {
            let squashed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
            let hits = [
                // A pinned strip found by adding to the width.
                squashed.contains("CHANNELS +") || squashed.contains("CHANNELS+"),
                // The master's slot by the strips' count.
                line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .any(|w| w == "STRIPS" || w == "MASTER_LANE" || w == "MASTER_COLUMN"),
                // A column or lane tested against the width.
                ["< CHANNELS", "<= CHANNELS", "> CHANNELS", ">= CHANNELS", "< recorder::CHANNELS",
                    ">= recorder::CHANNELS", "> recorder::CHANNELS", "<= recorder::CHANNELS",
                    "< crate::recorder::CHANNELS", ">= crate::recorder::CHANNELS"]
                    .iter()
                    .any(|p| squashed.contains(p)),
            ];
            if hits.iter().any(|h| *h) {
                out.push(format!("{}: {}", n + 1, line.trim()));
            }
        }
        out
    }

    /// The view builders and hit dispatch in `app.rs`: what builds a view,
    /// and what turns a press on one into a change.
    const APP_VIEW_FNS: [&str; 15] = [
        "fn mixer_view(&self)",
        "pub(crate) fn mixer_view_in(",
        "fn mixer_view_ranks(",
        "fn arrange_view<'a>(",
        "fn arr_lane_window(",
        "fn arr_clip_views(",
        "fn scrub_view(",
        "fn arrangement_input(",
        "fn apply_arrange_key(",
        "fn apply_mixer_hit(",
        "fn apply_mixer_hit_spread(",
        "fn apply_mixer_hit_one(",
        "fn mixer_value(",
        "pub fn strip_color(",
        "fn arr_open_automation_menu(",
    ];

    /// **No view does strip-index arithmetic.** Not `CHANNELS +` (a pinned
    /// strip found by adding to the width), not `STRIPS`, `MASTER_LANE` or
    /// `MASTER_COLUMN` (the master by a count), and not a column tested
    /// against the width: in the three panels, and in the app's view
    /// builders and hit dispatch. `StripRef` is where that arithmetic lives
    /// until the specials leave strip space, so the day the desk stops
    /// being a fixed width the views are not where it breaks.
    ///
    /// Revert-check: put `Hit::BusBay(_, b) => Some((recorder::STRIPS, b))`
    /// back in `arrangement_input`, or `let click_col = CHANNELS;` back in
    /// `mixer_panel::Layout::new`, and this names the line.
    #[test]
    fn no_view_code_does_strip_index_arithmetic() {
        let mut bad = Vec::new();
        for (file, src) in [
            ("mixer_panel.rs", include_str!("mixer_panel.rs")),
            ("arrangement_panel.rs", include_str!("arrangement_panel.rs")),
            ("recorder_panel.rs", include_str!("recorder_panel.rs")),
        ] {
            for hit in banned_in(&code_of(src)) {
                bad.push(format!("{file}:{hit}"));
            }
        }
        let app = include_str!("app.rs");
        for sig in APP_VIEW_FNS {
            for hit in banned_in(&code_of(body_of(app, sig))) {
                bad.push(format!("app.rs {sig}: {hit}"));
            }
        }
        assert!(bad.is_empty(), "strip-index arithmetic in view code:\n  {}", bad.join("\n  "));
        // And the scan can see what it bans: a planted line of each kind.
        for planted in [
            "let click_col = CHANNELS + 1;",
            "Some((recorder::STRIPS, b))",
            "if i >= CHANNELS { return; }",
            "let m = MASTER_LANE;",
        ] {
            assert!(!banned_in(planted).is_empty(), "the scan is blind to `{planted}`");
        }
    }
}
