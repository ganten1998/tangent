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
    /// Beats and tempo, shared by the click, the count-in and the SMF's tempo
    /// mark — one number, because a click at 90 against a file that says 120
    /// is a take nobody can edit afterwards.
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
            input_monitor: false,
            state: RecordState::Idle,
            elapsed_s: 0.0,
            position_s: 0.0,
            playing: false,
            arr_recording: false,
            view_window: (0.0, 60.0),
            meters: Meters::SILENT,
            master: Meters::SILENT,
            gr_db: 0.0,
            dest: "",
            project_dest: "",
            export_dest: "",
            preset_dest: "",
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
/// 2. A MIDI lane hears the keyboard when it is ARMED **or SELECTED**. Arm
///    used to be the whole of it, which meant trying an instrument out began
///    by arming a track — and a half-finished take was one keypress away.
///    Selecting a track is already the gesture that means "this one".
/// 3. **While the MIDI editor is open it is the ONLY lane that hears.** An
///    audition there is a question about the clip in front of you; with two
///    instruments armed, one key played both of them.
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
    selected: Option<usize>,
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
        let live = kinds.get(ch).copied().flatten() == Some(ChannelKind::Midi)
            && (armed & (1 << ch) != 0 || selected == Some(ch));
        if live {
            off &= !(1u32 << ch);
        }
    }
    off
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
// the fixed desk below, this is a limit you can feel the shape of, not a
// feature that has not arrived yet.

/// How many general channels the desk can CARRY — the capacity every
/// engine table, settings array and seat index is sized to.
///
/// **Thirty-two as of 2026-08-31, the owner's word** — the supporter desk
/// (it was twenty-four for one commit the same evening; the owner went
/// bigger). It grew from eight (nine columns was the accident of five
/// slots plus four inputs) to sixteen at 4.82.0, and to thirty-two with
/// the supporter unlock. What a given INSTALL shows is
/// [`active_channels`]: sixteen free, the full thirty-two with a key (or
/// the Konami road).
/// The engine always runs the full width, so a supporter's session opened
/// on a free install still SOUNDS whole — a licence must never silence
/// somebody's music — and the free desk simply cannot start new work past
/// sixteen. Fixed at each tier, like everything about the desk's shape:
/// an app whose strips appear and disappear as things load is a desk that
/// changes shape under your hands, so an unused channel is drawn as a
/// plus rather than not drawn.
pub const CHANNELS: usize = 32;

/// The free desk: the sixteen the app has always shown without a key.
pub const FREE_CHANNELS: usize = 16;

/// How many channels this install works at. The one place the paywall's
/// arithmetic lives: LAYOUT iteration and new-track pickers ask this;
/// engine capacity, persistence and permutation walks stay at
/// [`CHANNELS`] so nothing ever truncates somebody's data.
pub fn active_channels(licensed: bool) -> usize {
    if licensed { CHANNELS } else { FREE_CHANNELS }
}

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
/// **Must equal `tangent_record::audio::MAX_PICKS`**, which is the same number
/// counted where the capture happens. This crate cannot reach that one — see
/// the firewall — so the host asserts they agree.
pub const INPUTS: usize = 4;

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

/// How many bays a strip shows. The master has two racks' worth.
pub fn bays_on(strip: Option<Strip>) -> usize {
    bays_of(strip, false)
}

/// The same question asked with the one fact `Strip` cannot carry: whether
/// the channel is currently a FOLDER. A folder shows two racks' worth,
/// like the master - see `folder_extra_seat`.
pub fn bays_of(strip: Option<Strip>, folder: bool) -> usize {
    if strip.is_none() || (folder && matches!(strip, Some(Strip::Channel(_)))) {
        INSERTS * 2
    } else {
        INSERTS
    }
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
    } else {
        (desk_index, slot)
    }
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

/// Every rack seat: strips, master, the first pad block, the master's
/// extra, a pad block for each channel after the first, and a folder-extra
/// seat per channel.
pub const TOTAL_SEATS: usize = FOLDER_EXTRA_SEATS + CHANNELS;

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
/// So the two spaces get two types. A `Column` comes from geometry — a press,
/// a drawn strip — and the ONLY ways out of it are [`Column::strip`], which
/// answers what the column is, and [`Column::desk_index`], which answers where
/// its settings live. Indexing anything desk-shaped with `.0` is greppable,
/// deliberate, and almost always the bug being reintroduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Column(pub usize);

impl Column {
    /// Which strip this column shows, or `None` for the master.
    ///
    /// The one place a column becomes a strip. `shown()` and `all()` are not
    /// the same list — the click and the effects bus are strips with no column
    /// of their own — and everything that turns a press into a change goes
    /// through here.
    pub fn strip(self) -> Option<Strip> {
        Strip::shown().get(self.0).copied()
    }

    /// Where this column's settings live in every desk array.
    ///
    /// The master's are last, past every channel — it is not in `shown()` and
    /// has no `Strip` of its own, so `STRIPS` is its slot by convention and
    /// this is where that convention is written down.
    pub fn desk_index(self) -> usize {
        self.strip().map_or(STRIPS, Strip::index)
    }
}


/// How many COLUMNS the desk draws: every strip that has one, then the master.
///
/// Re-exported from `mixer_panel::COLUMNS` would be the other direction and
/// this crate's panels do not depend on each other, so it is stated here and
/// `a_column_and_a_strip_agree_about_the_desk` holds the two together.
pub const COLUMNS_FOR_TEST: usize = CHANNELS + 2;

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

    /// The column this strip is drawn in, or `None` for the click and the
    /// bus, which have none. The inverse of [`Column::strip`].
    pub fn column_of(self) -> Option<Column> {
        Self::shown().iter().position(|s| *s == self).map(Column)
    }

    /// Its place in `Desk`'s arrays.
    pub const fn index(self) -> usize {
        match self {
            Strip::Channel(i) => i,
            Strip::Drums => CHANNELS,
            Strip::Click => CHANNELS + 1,
            Strip::Fx => CHANNELS + 2,
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
    /// The take is the desk (see `record::TakeSource`), so this one rule
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
        }
    }
}


impl RecorderState {
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
            input_monitor: false,
            scrub: ScrubView::default(),
            state: self.state,
            elapsed_s: self.elapsed_s,
            position_s: self.position_s,
            playing: self.playing,
            arr_recording: self.arr_recording,
            view_window: (0.0, 60.0),
            meters: self.meters,
            master: self.master,
            gr_db: self.gr_db,
            dest: &self.dest,
            project_dest: &self.project_dest,
            export_dest: &self.export_dest,
            preset_dest: &self.preset_dest,
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
    /// The one button. Start a pre-roll, start a take, or stop one — the
    /// session decides which, because only it knows what state it is in.
    Toggle,
    /// The cassette beside the cog: start or stop a CAPTURE — the whole-take
    /// machinery (window video, all audio, the `.mid`), completely separate
    /// from the arrangement. `Toggle` is the arrangement's red button now;
    /// this is where the old take toggle lives.
    Capture,
    /// Stop, unconditionally. Separate from `Toggle` because the band draws a
    /// distinct Stop control and "the button that stops it" must not also be
    /// able to start one.
    Stop,
    /// PAUSE: hold the playhead where it stands. Ignored while any record
    /// owns the transport — a pause mid-record would let the capture run
    /// past the playhead.
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
    /// The target BAY is the host's to choose — the next free one — for the
    /// reason the owner gave: a drop that landed on an occupied insert
    /// should still work rather than refusing or overwriting.
    CopyInsert {
        from_strip: usize,
        from_bay: usize,
        to_strip: usize,
    },
    /// Bring these clips up so their loudest point sits just under full
    /// scale.
    ///
    /// A request because the PEAK is host-side: `tangent-ui` holds a path and
    /// a length, and the samples live behind the firewall with the decoder.
    /// The host measures and writes the gain back through
    /// `TangentApp::set_clip_gain`.
    NormalizeClips(Vec<u64>),
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
    /// The green button: audition from the playhead, no take. Pressed while
    /// auditioning, it PAUSES in place; Stop is what returns to 0:00. The
    /// host ignores it while a take is writing — record owns the transport
    /// then.
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
    /// **Aim the engine's ONE insert tap at a mixer slot, or take it off**
    /// — the feed behind an EQ face's analyzer and an imager face's
    /// vectorscope. `strip` and `slot` are DESK terms (`Strip::index` and
    /// the strip's Nth slot); the host owns the `seat_bay` translation, the
    /// same way it owns it for `OpenInsertEditor`.
    ///
    /// A request because the tap is the engine's: this crate cannot reach
    /// `Engine::set_insert_tap` — the firewall is the whole point — so the
    /// face says which bay it is looking at and the host arms the tap and
    /// starts draining it into `TangentApp::insert_tap_feed`. Only one face
    /// is ever open, so the LAST request wins and `on: false` from the
    /// closing face disarms.
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
}

/// How hard an auditioned note is struck.
///
/// Firmly, but not at the top of the scale: a sampled piano's loudest layer is
/// usually its harshest, and this is a note somebody asked to hear rather than
/// one they played.
pub const AUDITION_VELOCITY: u8 = 88;

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
        if self.video.has_composite() {
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
        }
    }

    /// Start editing SEEDED with the current reading, the caret at its end
    /// — the owner's rule: the field opens looking exactly like the
    /// readout, and the arrows walk the digits.
    pub fn prefilled(field: NumField, seed: String) -> Self {
        let caret = seed.len();
        Self { field, text: seed, caret }
    }

    pub fn left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
    }

    pub fn right(&mut self) {
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
    pub fn push(&mut self, ch: char) {
        const MAX: usize = 8;
        // A signature is two numbers and a slash, and nothing else: no minus,
        // no decimal point. Handled first so those two cannot creep in.
        if self.field == NumField::Meter {
            let ok = ch.is_ascii_digit() || (ch == '/' && !self.text.contains('/'));
            if ok && self.text.len() < MAX {
                let at = self.caret.min(self.text.len());
                self.text.insert(at, ch);
                self.caret = at + 1;
            }
            return;
        }
        let ok = ch.is_ascii_digit()
            // One dot, and a minus only in front. Neither check is about
            // rejecting bad input for its own sake: `.parse()` will do that
            // anyway. They stop the field from showing something it will then
            // silently refuse to accept.
            || (ch == '.' && !self.text.contains('.'))
            || (ch == '-' && self.text.is_empty() && self.caret == 0)
            // The filters read in hertz and "20k" is how anybody writes
            // twenty thousand of them. Only there: a percent field showing a
            // k it will refuse is the trap the filter exists to stop.
            || (matches!(ch, 'k' | 'K')
                && matches!(
                    self.field,
                    NumField::Fx(
                        crate::recorder_panel::Fx::Hpf | crate::recorder_panel::Fx::Lpf
                    )
                )
                && !self.text.to_ascii_lowercase().contains('k'));
        if ok && self.text.len() < MAX {
            let at = self.caret.min(self.text.len());
            self.text.insert(at, ch);
            self.caret = at + 1;
        }
    }

    pub fn pop(&mut self) {
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
    pub channels: u16,
    /// Frames per callback, when the device is running one it will admit to.
    /// `None` is cpal's `BufferSize::Default` — the device chose, and did not
    /// say what it chose.
    ///
    /// **This is what was ASKED for.** See `observed_frames`.
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
        };
        let rt = status.round_trip_ms().expect("both sides are open");
        assert!(
            (rt - (1.3333 + 10.6667 + 4.0)).abs() < 0.01,
            "the estimate used the requested buffer: {rt}"
        );
    }

    /// **The three rules that decide which instrument a key plays.**
    ///
    /// Every one of them is a bug the owner reported: arming to audition,
    /// and one key sounding two instruments at once in the editor.
    #[test]
    fn live_midi_reaches_armed_selected_and_the_editors_lane_alone() {
        use super::{live_midi_off, ChannelKind};
        let off = |ch: usize, m: u32| m & (1 << ch) != 0;
        let mut kinds = vec![None; super::CHANNELS];
        kinds[0] = Some(ChannelKind::Midi);
        kinds[1] = Some(ChannelKind::Midi);
        kinds[2] = Some(ChannelKind::Audio);

        // Nothing armed, nothing selected: silence, which is what it always
        // did and what the two rules below are exceptions to.
        let m = live_midi_off(&kinds, 0, None, None);
        assert!(off(0, m) && off(1, m) && off(2, m));

        // ARMED sounds.
        let m = live_midi_off(&kinds, 1 << 1, None, None);
        assert!(!off(1, m), "an armed MIDI lane takes no notes");
        assert!(off(0, m));

        // SELECTED sounds, unarmed — the whole point. Trying an instrument
        // out used to begin by arming a track, one keypress from a take.
        let m = live_midi_off(&kinds, 0, Some(0), None);
        assert!(!off(0, m), "a selected MIDI lane still needs arming");
        assert!(off(1, m));

        // A selected AUDIO lane is still an audio lane.
        let m = live_midi_off(&kinds, 0, Some(2), None);
        assert!(off(2, m), "an audio lane was handed a note");

        // **The editor takes the desk.** Both lanes armed, editor open on
        // lane 1: lane 0 goes quiet. This is "Mini v4 on track one, piano
        // on track 2, audition note, it triggers BOTH".
        let m = live_midi_off(&kinds, 0b11, Some(0), Some(1));
        assert!(!off(1, m), "the edited lane went quiet");
        assert!(off(0, m), "the other armed instrument sounded too");

        // And an editor somehow open on an audio lane silences every
        // CHANNEL rather than opening the desk up.
        let all = ((1u64 << super::CHANNELS) - 1) as u32;
        assert_eq!(live_midi_off(&kinds, 0b11, Some(0), Some(2)), all);

        // **The strips past the channels are never in this mask.** Drums,
        // click and fx read it for their own bit and have never been gated
        // by it; a mask built as "everything off, then open the live ones"
        // reaches them where the old one did not.
        for (armed, sel, ed) in [(0u32, None, None), (0b11, Some(0), Some(1)), (0, Some(0), None)] {
            let m = live_midi_off(&kinds, armed, sel, ed);
            assert_eq!(m & !all, 0, "a strip past the channels was gated: {m:#x}");
        }
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
        let l = super::Loudness {
            momentary: -14.2,
            short: -14.2,
            integrated: -14.2,
            window: super::LoudnessWindow::Momentary,
            pair: [-14.2; 4],
        };
        assert_eq!(l.text(), "14.2", "the reading kept its sign");
        // Silence still says so rather than printing a number.
        let quiet = super::Loudness::default();
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
        assert_eq!(bays_on(Some(Strip::Channel(0))), INSERTS);

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
            let seat = super::pad_insert_index(pad);
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
            bays_of, folder_extra_seat, pad_rack_index, seat_bay, seat_order, Strip, CHANNELS,
            DRUM_PADS, INSERTS, INSERT_SPACE, MASTER_EXTRA_SEAT, MASTER_SEAT, STRIPS,
            TOTAL_SEATS,
        };
        // The counts: a folder wears six, a plain channel three, the click
        // never folds, and the master keeps its six either way.
        assert_eq!(bays_of(Some(Strip::Channel(3)), true), INSERTS * 2);
        assert_eq!(bays_of(Some(Strip::Channel(3)), false), INSERTS);
        assert_eq!(bays_of(Some(Strip::Click), true), INSERTS);
        assert_eq!(bays_of(None, false), INSERTS * 2);

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
        assert!(seen.iter().all(|s| *s < TOTAL_SEATS), "a seat past the end");
        let (seat, bay) = seat_bay(CHANNELS - 1, INSERTS * 2 - 1);
        assert!(
            seat * INSERTS + bay < INSERT_SPACE,
            "the last folder bay is past the end of the insert space"
        );

        // **seat_order carries the extra seat with its channel** and moves
        // nothing else - four moved rows for one swap, identity elsewhere.
        let mut order: Vec<usize> = (0..CHANNELS).collect();
        order.swap(2, 5);
        let full = seat_order(&order);
        assert_eq!(full.len(), TOTAL_SEATS);
        assert_eq!(full[2], 5);
        assert_eq!(full[5], 2);
        assert_eq!(full[folder_extra_seat(2)], folder_extra_seat(5));
        assert_eq!(full[folder_extra_seat(5)], folder_extra_seat(2));
        assert_eq!(full[MASTER_SEAT], MASTER_SEAT);
        assert_eq!(full[MASTER_EXTRA_SEAT], MASTER_EXTRA_SEAT);
        assert_eq!(
            (0..TOTAL_SEATS).filter(|i| full[*i] != *i).count(),
            4,
            "the swap moved more than its own four rows"
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
            let (lo, hi) = super::LUFS_RANGE;
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
        s.composite.camera = false;
        s.composite.display = false;
        assert_eq!(s.problem(), Some(SpecProblem::EmptyComposite));
    }

    /// The subtle one: the display layer is on, so the composite is not empty,
    /// but every panel inside it is off — which composites a black rectangle
    /// over half the frame and looks like a bug in the encoder.
    #[test]
    fn a_display_layer_with_no_panels_is_refused() {
        let s = ExportSpec {
            video: VideoMode::Composite,
            composite: Composite {
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
                camera: false,
                display: false,
                ..Default::default()
            },
            ..Default::default()
        }
        .produces_video(true));

        // Camera-only, no camera: nothing to encode.
        let cam_only = ExportSpec {
            composite: Composite {
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
            },
            AudioStatus {
                input: None,
                output: Some(side(48_000, Some(256))),
                monitor_ms: None,
                graph_floor: None,
                dropouts: None,
            },
            // And a side that will not say its buffer size cannot contribute
            // half a figure.
            AudioStatus {
                input: Some(side(48_000, None)),
                output: Some(side(48_000, Some(256))),
                monitor_ms: None,
                graph_floor: None,
                dropouts: None,
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
        }
        .rates_disagree());

        assert!(!AudioStatus {
            input: Some(side(48_000, Some(256))),
            output: Some(side(48_000, Some(512))),
            monitor_ms: None,
            graph_floor: None,
            dropouts: None,
        }
        .rates_disagree());

        // One side alone cannot disagree with anything.
        assert!(!AudioStatus {
            input: Some(side(44_100, Some(256))),
            output: None,
            monitor_ms: None,
            graph_floor: None,
            dropouts: None,
        }
        .rates_disagree());
    }
}

#[cfg(test)]
mod missing_tests {
    use super::{missing_from_take, Strip, SLOTS};

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
