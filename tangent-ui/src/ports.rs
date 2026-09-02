//! Where notes come from, when the app is the one choosing.
//!
//! The desktop app opens a MIDI device itself: it enumerates ports, picks one
//! at startup, and lets the user switch with a dialog. A VST3 editor does none
//! of that — the host hands it note events and there is no device list to
//! offer — which is exactly what `Caps::midi_ports` says.
//!
//! So the app holds an `Option<Box<dyn MidiPorts>>`: `Some` on the desktop,
//! `None` in a plugin. A trait rather than a `cfg`, because the alternative is
//! `midir` in the shared crate, and the whole point of `tangent-ui`'s dependency
//! list is that `midir` cannot be reached from here at all.
//!
//! Note what is NOT behind this trait: the `mpsc` channel the events arrive
//! on. That is `std`, it is host-agnostic, and a plugin uses the same one —
//! `process()` sends into it from the audio thread and the editor drains it on
//! the next frame, which is the same shape as `midir`'s callback thread.

/// One clip's picture, made by the host after decoding.
///
/// **The wave is MIN/MAX pairs, and it covers a SPAN rather than the whole
/// clip.**
///
/// Pairs because a waveform drawn as one number per column is a bar chart:
/// audio is not symmetric about zero and the shape people read is the
/// outline of both sides. A span because the resolution has to follow the
/// zoom — a picture computed once at import is sharp at one zoom and blocky
/// at every other, and computing the whole of a five-minute clip at
/// screen resolution every time the view moves is a pass over fourteen
/// million frames for the part of it nobody can see.
///
/// So the host draws only what is on screen, at the pixels it occupies, and
/// says which seconds it drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipInfo {
    pub id: u64,
    /// `[min, max, min, max, …]`, each in −1..=1. May be empty.
    pub wave: Vec<f32>,
    /// The whole FILE's length, which is what a trim is measured against.
    pub file_s: f64,
    /// The first second `wave` covers, in the clip's own drawn timeline.
    pub wave_from_s: f64,
    /// How many seconds it covers. Zero means there is no picture.
    pub wave_span_s: f64,
}

use crate::midi_event::MidiEvent;
use std::sync::mpsc;

/// A device the user can pick from a list.
///
/// Two fields and the distinction between them is the whole point. `uid` is
/// what gets written to the settings file; `name` is what gets drawn. Storing
/// the name would break for two identical webcams — which is not a hypothetical,
/// it is what happens the moment somebody adds a second camera for a side angle
/// — and again when the OS language changes and "Built-in Microphone" comes
/// How far an export has got, and what it has heard so far.
///
/// **The waveform is the point.** A percentage says a render is moving; a
/// waveform says what it is rendering, which is the difference between
/// waiting and watching. Buckets are (min, max) pairs in -1..1, one per
/// column of the drawn wave, filled left to right as the render walks the
/// timeline - so the picture grows the way REAPER's does.
#[derive(Debug, Clone, Default)]
pub struct ExportProgress {
    /// 0..=1.
    pub fraction: f32,
    /// What is happening, in words: rendering, encoding, writing.
    pub label: String,
    pub peaks: Vec<(f32, f32)>,
}

/// How many columns an export's waveform is drawn in.
pub const EXPORT_PEAKS: usize = 480;

/// What a project export writes, in which shape, and how fast.
///
/// Assembled by the export dialog, carried whole to the host. FULL SPEED is
/// the default by the owner's word: an offline engine renders the song as
/// fast as the machine goes; realtime is the option for the day a plugin
/// misbehaves offline.
#[derive(Debug, Clone, PartialEq)]

pub struct ProjectExportSpec {
    /// The master mix lands in a file.
    pub master: bool,
    /// Every used lane lands in a stems folder.
    pub stems: bool,
    /// Sample rate of the rendered files (offline). A realtime pass runs at
    /// the interface's rate whatever this says.
    pub rate: u32,
    /// Fold to one channel.
    pub mono: bool,
    /// Where the files land. Empty = the project's own export folder.
    pub dir: String,
    /// The file stem — "mix" makes mix.wav / mix.mp3.
    pub name: String,
    /// The formats: a primary, and an optional second copy.
    pub primary: ExportFormat,
    pub secondary: Option<ExportFormat>,
    /// A realtime pass through the live engine instead of the offline one.
    pub realtime: bool,
    /// The span to render, in song seconds. `None` is the whole project.
    pub span: Option<(f64, f64)>,
    /// The WAV word size.
    pub depth: ExportDepth,
    /// Show the folder when the render finishes. A long export is a thing
    /// somebody walks away from, and the first thing they want on returning
    /// is the file — not the app that made it.
    pub open_when_done: bool,
}

impl Default for ProjectExportSpec {
    fn default() -> Self {
        Self {
            master: true,
            stems: false,
            rate: 48_000,
            mono: false,
            dir: String::new(),
            name: "mix".to_owned(),
            primary: ExportFormat::Wav,
            secondary: None,
            open_when_done: true,
            realtime: false,
            span: None,
            depth: ExportDepth::Pcm24,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Wav,
    Mp3,
}

/// WAV word size. 24-bit PCM is the go-to for mixing and mastering hand-
/// offs; 32-float keeps headroom above 0 dB for stems and heavy bounces;
/// 16-bit exists for CD burns and the platforms that demand it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportDepth {
    Pcm16,
    Pcm24,
    Float32,
}

impl ExportFormat {
    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Wav => "wav",
            ExportFormat::Mp3 => "mp3",
        }
    }
}

/// back as "Micrófono integrado".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable, opaque, platform-supplied. Never shown to the user.
    pub uid: String,
    /// What the user calls it.
    pub name: String,
    /// The one the platform would pick if nobody chose. Marked in the list
    /// rather than auto-selected, so "System Default" stays a visible choice.
    pub default: bool,
}

/// A source of capture devices — cameras, or audio inputs.
///
/// One trait for both because the app does exactly the same three things with
/// each of them: list what is there, open one by uid, ask which is open. The
/// alternative was two traits differing only in their doc comments.
///
/// `Send` for the same reason [`MidiPorts`] is: the app that owns it has to be.
pub trait CaptureDevices: Send {
    /// Everything present right now. Re-read each time a picker opens, because
    /// devices are plugged and unplugged while the app runs — and on macOS a
    /// Continuity Camera appears and disappears as the phone comes and goes.
    fn list(&self) -> Vec<DeviceInfo>;

    /// Open this device by uid, closing whatever was open. `Err` carries a
    /// message written for the user.
    ///
    /// An empty uid means "close whatever is open and select nothing", which is
    /// how the None row in the picker is expressed without a second method.
    fn open(&mut self, uid: &str) -> Result<(), String>;

    /// The uid currently open, if any.
    fn current(&self) -> Option<String>;

    /// The display name of the device currently open.
    ///
    /// Separate from `current` because the band needs to *draw* something, and
    /// looking the uid back up through `list()` every frame means enumerating
    /// hardware sixty times a second.
    fn current_name(&self) -> Option<String>;
}

/// What the audio system itself can be asked, for the Setup panel.
///
/// A third port beside [`MidiPorts`] and [`CaptureDevices`], and not a method
/// on either: a system is not a device, and the two questions here are asked
/// once when a panel opens rather than every frame — which is the whole reason
/// they are not on [`crate::recorder::AudioStatus`], the struct the host
/// pushes sixty times a second. `rates` in particular re-enumerates hardware.
pub trait AudioSetup: Send {
    /// Every audio system this build can open, in the backend's own order.
    ///
    /// One on macOS and Windows; two on Linux and FreeBSD, where JACK is
    /// compiled in beside ALSA. (It costs those artifacts no dependency:
    /// libjack is `dlopen`ed on demand. ASIO is the one extra that genuinely
    /// needs a library present when the release is BUILT, which is why it is
    /// still behind a cargo feature.) Listed from the real enumeration either
    /// way, so the panel says what this build has rather than what was hoped.
    fn systems(&self) -> Vec<String>;

    /// The rates the input device now selected will actually open at.
    ///
    /// Empty when nothing is selected or the device cannot be reached, and the
    /// panel then offers only "the device's own" — which is what every build
    /// before this one did unconditionally.
    fn rates(&self) -> Vec<u32>;

    /// How many inputs the selected device has. 0 when nothing is selected.
    fn input_channels(&self) -> u16;

    /// Which inputs of the selected device the microphone picker offers as
    /// rows of their own.
    ///
    /// **A set, not a choice.** `(a, None)` is input `a` on its own; `(a,
    /// Some(b))` is the pair. Any number of them, mono and stereo together —
    /// the piano on 1/2 and a room mic on 6 are two microphones, not two
    /// settings of one, and the picker is where you say which one to record.
    ///
    /// Zero-based, like every index. What the UI prints is one-based, because
    /// that is what is on the front of the interface.
    fn exposed(&self) -> Vec<(u16, Option<u16>)>;

    /// Offer exactly these, and hand back what has to be remembered.
    ///
    /// **The uid grammar stays on the host's side of the firewall.** A choice
    /// is a pair of numbers here and a string there; `tangent-ui` cannot spell
    /// the string and must not learn to — so the return is opaque, stored
    /// verbatim in the settings file and handed straight back at startup,
    /// exactly as the chosen device's own uid already is.
    fn set_exposed(&mut self, picks: Vec<(u16, Option<u16>)>) -> Vec<String>;

    /// **What the monitor is really playing into**, for the device opened
    /// under `device` (`None` being the system default).
    ///
    /// Enumerates hardware, so it is snapshotted when the panel opens and
    /// never pushed per frame — the same rule `rates` follows.
    fn output_path(&self, device: Option<&str>) -> OutputPath;

    /// **The smallest buffer the machine's audio graph will actually serve**,
    /// or `None` where the question does not apply.
    ///
    /// A PipeWire graph runs one period for every client at once, clamped to a
    /// floor set in its configuration. A client asking for less is not refused —
    /// it is quietly given the floor. So the buffer control could ask for 256,
    /// be handed 512, and have nothing true to say about why.
    ///
    /// What it said instead was "another app owns this device", which is the
    /// CoreAudio explanation: on that platform another host holding the
    /// interface really does dictate the size. Here it is the graph, and no
    /// second app need exist. The panel told one user that about their own
    /// machine while nothing else was running, and the number that actually
    /// decided the outcome was one line in a config file the app never
    /// mentioned.
    ///
    /// A plain number, because the firewall means this side cannot ask a graph
    /// anything and should not learn how.
    fn graph_floor_frames(&self) -> Option<u32> {
        None
    }
}

/// **What the OS says about the device the monitor plays into.**
///
/// The panel could always say how many milliseconds the two buffers hold. It
/// could not say the thing that actually costs the time: that the "device" is
/// not hardware, and another application is standing between the mix and the
/// speakers. That gap cost the owner a week of believing the code had
/// regressed, while every measurement the app can take of ITSELF came back
/// clean — because with the device to itself there was nothing to find.
///
/// Plain data on purpose. Asking the OS needs CoreAudio on macOS and will
/// need something else on each other platform, and none of that may cross the
/// firewall into `tangent-ui`; the host fills this in and the panel only
/// prints it, exactly as it already does for device names and rates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputPath {
    /// The device was found and answered. **`false` prints nothing at all** —
    /// a build or a machine that cannot check must not imply the path is
    /// clear.
    pub known: bool,
    /// The device is published by SOFTWARE, not by hardware
    /// (`kAudioDeviceTransportTypeVirtual`). True of every interposer rather
    /// than of one product, which is why the transport is what gets asked
    /// about and not a list of vendor names to keep up to date.
    pub virtualised: bool,
    /// The device is an aggregate. Not a warning — it is how anyone with two
    /// interfaces runs them on one clock — but it is worth naming, because an
    /// aggregate has a drift-correction cost a single device does not.
    pub aggregate: bool,
    /// The transport in a word: "virtual", "USB", "built-in". Empty when the
    /// device reports one this build has no word for.
    pub transport: String,
    /// Who publishes it, so the line can say WHOSE — the difference between a
    /// warning and an instruction. Empty when unreported.
    pub maker: String,
    /// `kAudioDevicePropertyLatency + kAudioDevicePropertySafetyOffset`, in
    /// frames. **cpal reads neither on any platform**, so this is real delay
    /// the round-trip estimate has never included, whatever kind of device is
    /// open. Zero when unreported.
    pub extra_frames: u32,
}

impl OutputPath {
    /// Those frames as milliseconds at `rate`, when there are any.
    pub fn extra_ms(&self, rate: u32) -> Option<f64> {
        (self.known && self.extra_frames > 0 && rate > 0)
            .then(|| f64::from(self.extra_frames) * 1000.0 / f64::from(rate))
    }

    /// **Is there anything to SAY?**
    ///
    /// A USB interface reporting a normal safety offset is the ordinary case
    /// and deserves no line. The panel speaks when the path holds something
    /// the owner did not put there, or when it is holding time the round-trip
    /// figure does not include.
    pub fn worth_saying(&self) -> bool {
        self.known && (self.virtualised || self.aggregate || self.extra_frames > 0)
    }
}

/// A folder the app has asked the host to choose.
///
/// The **request pattern**, not a blocking call. `rfd`'s native panel runs a
/// nested run loop, and raising one from inside an egui frame means re-entering
/// the frame that is already on the stack. So `tangent-ui` records that it wants
/// a folder, the host drains it *after* `frame()` returns, and a plugin refuses
/// simply by never draining — the same shape as the plugin's `pending_resize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirRequest {
    /// Where the picker should open. `None` means the platform decides.
    pub start_at: Option<std::path::PathBuf>,
    /// Title for the panel.
    pub title: String,
    /// What the chosen folder is FOR.
    ///
    /// One request type with a purpose on it rather than two nearly identical
    /// ones: the host's job is the same either way — raise the native panel
    /// after the frame — and the only thing that differs is which of the app's
    /// setters gets the answer.
    pub purpose: DirPurpose,
}

/// Every extension a project document may wear: the canonical first, the
/// legacy forever. The Open panels and the fallback browser both consume
/// this, so both accept both.
pub const PROJECT_EXTENSIONS: &[&str] = &["tan", "tangent"];

/// A file the app has asked the host to choose.
///
/// The same request pattern as [`DirRequest`], and for the same reason: a
/// native panel cannot be raised from inside an egui frame. A plugin refuses
/// simply by never draining it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRequest {
    pub start_at: Option<std::path::PathBuf>,
    pub title: String,
    /// Extensions to offer, without dots, and the name of the group. Empty
    /// means every file.
    pub extensions: Vec<String>,
    pub extension_label: String,
    pub purpose: FilePurpose,
    /// Whether the panel may return MORE THAN ONE file.
    pub multi: bool,
}

/// The backing track, as the band needs to draw it.
///
/// **The waveform is precomputed by the host**, because the samples are on the
/// other side of the firewall and there are a hundred million of them.
/// `tangent-ui` gets an outline it can draw and a length it can label; what a
/// peak envelope is, and how many megabytes it came from, is the recorder's
/// business.
/// What the host learned decoding one drumpad slot, pushed after every kit
/// reload — the pad's picture of itself, from the side that can read files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadStatus {
    /// Decoded and handed to the engine: this pad will sound.
    pub loaded: bool,
    /// The document names a file the host could not read.
    pub missing: bool,
    /// A peak envelope, 0..=1, left to right across the sample.
    pub wave: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackInfo {
    /// The file's name, or empty when nothing is loaded.
    pub name: String,
    /// How long the file is, in seconds.
    pub seconds: f64,
    /// A peak envelope, 0..=1, left to right across the whole file.
    pub wave: Vec<f32>,
    /// Why the last import did not work, if it did not.
    pub error: String,
}

impl TrackInfo {
    /// Nothing loaded, as a borrowable constant — the band's `EMPTY` view is a
    /// `const` and cannot build a `String`.
    pub const NONE: &'static TrackInfo = &TrackInfo {
        name: String::new(),
        seconds: 0.0,
        wave: Vec::new(),
        error: String::new(),
    };

    /// Whether anything is loaded at all.
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }
}

/// What the effects ship as, pushed in by the host.
///
/// **The DSP owns these numbers, and it is on the other side of the firewall.**
/// `tangent-ui` draws a panel of sliders and has no idea what a comb filter's
/// feedback should be; writing a second copy of the defaults here to draw the
/// sliders at the right place would be two tables to keep in step, and the
/// symptom of them drifting is a panel that reads 62% while the audio does
/// something else. So the host hands them over once, the same way it hands
/// over the device list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectDefaults {
    /// Parameter key to its default, as the settings file would store it.
    pub values: serde_json::Map<String, serde_json::Value>,
    /// How a knob's position reads out, keyed by its mix key (`hpf_mix`).
    ///
    /// Anything not named here reads as a percentage, which is what a wet/dry
    /// send is and what a filter corner is not.
    pub units: Vec<(String, KnobUnit)>,
    /// Every parameter that is a list of names rather than a number.
    ///
    /// **General, because there are three of them now.** The delay's time was
    /// the only one, and it was two dedicated fields; a filter slope is the
    /// same shape of thing — a short list a row steps through — and a second
    /// and third copy of "divisions / default_division" is how the two drift.
    pub choices: Vec<ChoiceParam>,
}

/// What a knob's number means, for the one line of text under its name.
///
/// **The host decides, because the host owns the sweep.** `tangent-ui` cannot
/// name `HPF_HZ` — it is a DSP constant on the other side of the firewall —
/// but it can be handed two ends and told the shape between them, which is a
/// display mapping and not a filter.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum KnobUnit {
    /// 0..=1 as a whole-number percentage. The default for everything.
    #[default]
    Percent,
    /// A frequency, swept **exponentially** from `low` to `high` — which is
    /// what a frequency control is, and why an octave is the same distance
    /// everywhere on the dial.
    Hertz { low: f32, high: f32 },
    /// A level, swept **linearly in decibels** from `low` to `high`, because
    /// decibels already are the logarithm and taking it twice would put the
    /// useful half of a threshold in the last eighth of the travel.
    Decibels { low: f32, high: f32 },
}

/// One parameter whose values are named rather than continuous.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChoiceParam {
    /// The settings key, e.g. `delay_division`.
    pub key: String,
    /// `(key, label)`, in the order a row steps through them.
    pub options: Vec<(String, String)>,
    /// Which option applies when the settings file says nothing.
    pub default: String,
}

/// **One automatable parameter of a plugin in a bay**, as plain data.
///
/// The host reads these off the edit controller on the main thread and pushes
/// them in; `tangent-ui` cannot see a VST3 type and must not — the firewall
/// (`scripts/check-firewall.sh`) is what keeps it that way. So a parameter is
/// an id, a word and a unit, and everything COM about it stays behind the
/// boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct FxParam {
    /// The plugin's own id. **Not an index**: ids survive a plugin update and
    /// positions do not, and this is what an automation curve stores.
    pub id: u32,
    pub title: String,
    /// "dB", "Hz", "%" — empty when the plugin does not say.
    pub units: String,
    /// Where the knob rests, normalised. A lane with no points reads as this.
    pub default_normalised: f64,
    /// 0 for a continuous parameter, otherwise how many steps it has.
    pub step_count: i32,
}

/// What one insert bay offers an automation lane.
///
/// Kept per BAY rather than as one flat list, because the menu is read that
/// way — a plugin's name and then its parameters under it — and because two
/// bays can hold the same plugin, whose ids would then collide in a flat list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FxParams {
    /// The plugin's short name, for the menu row the parameters hang under.
    pub name: String,
    pub params: Vec<FxParam>,
    /// Whether the tenant can take a sidechain key — a VST3 reporting an aux
    /// input bus with channels. Pushed by the host because only it can read
    /// the plugin's bus list; the UI uses it to gate the bay menu's
    /// "Sidechain" row. Appended, defaulting false, so every other
    /// construction site keeps meaning what it meant.
    pub has_sidechain: bool,
}

/// One editable parameter of a patch.
///
/// **Untyped on purpose, like every other thing the host pushes in.**
/// `tangent-ui` cannot name a `Voice` and has no business knowing what keyboard
/// level scaling is; it draws a row with a name and a number in it and reports
/// which number changed. What that means to six operators is the DSP's problem,
/// on the other side of the firewall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchParam {
    pub name: String,
    pub value: i32,
    pub max: i32,
    /// Names for each value, when the parameter is a CHOICE rather than a
    /// number: an LFO wave is "triangle", not 0. Empty means a plain number.
    pub choices: Vec<String>,
    /// A word after the number — "Hz", "semitones". Empty for most.
    pub unit: String,
}

/// One page of the patch editor: the six operators, and the patch itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchGroup {
    pub title: String,
    pub params: Vec<PatchParam>,
}

/// Everything the patch editor shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchEdit {
    /// The patch's own name, ten characters in the format.
    pub name: String,
    /// 0..=31. Shown as 1..=32, the way every chart prints it.
    pub algorithm: usize,
    /// Where each operator's output goes: 0 is the speaker, 1..=6 is the
    /// operator it modulates. Pushed in because the routing table lives with
    /// the synth, and the diagram is the one thing this editor draws.
    pub routing: [u8; 6],
    /// Which operator the feedback loop is taken from, 1-based.
    pub feedback_op: u8,
    pub groups: Vec<PatchGroup>,
    /// Where a Save would write, for the editor to say so. Empty until the
    /// host knows.
    pub bank_path: String,
}

/// What the host found in a DX7 cartridge, for the picker to show.
///
/// **Names, not voices.** `tangent-ui` cannot parse SysEx and has no business
/// knowing what an operator is; it draws a list and reports an index, and the
/// host turns that index back into a patch. That is the same firewall the
/// plugin picker keeps by holding paths rather than modules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CartridgeInfo {
    /// What the cartridge calls itself. Empty means none is loaded.
    pub bank: String,
    /// It failed its own checksum. Loaded anyway; worth showing.
    pub bad_checksum: bool,
    /// The patch names, in cartridge order.
    pub voices: Vec<String>,
    /// Why the last attempt failed, if it did. Empty on success, and on
    /// failure everything above is left as it was.
    pub error: String,
    /// This IS the bank that ships in the binary.
    ///
    /// A flag rather than comparing `bank` to a known string: the shipped bank
    /// has a name like any other, so an empty name does not mean "the factory
    /// one" and never did. The picker uses it to decide whether there is
    /// anything to go back FROM — and `tangent-ui` may not name the constant,
    /// which lives on the other side of the firewall.
    pub factory: bool,
}

/// Why a file is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePurpose {
    /// A DX7 cartridge to load into the built-in FM instrument.
    Cartridge,
    /// **A sound for the metronome.** Any file the decoder reads; it is
    /// resampled and used for both the beat and the accent. Empty means the
    /// bundled click, which is always one button away.
    ClickSample,
    /// An audio file to play along to.
    BackingTrack,
    /// A project document (`project.tan`; the legacy `.tangent` reads forever).
    Project,
    /// Media to stand on the timeline at the playhead. The lane is the one
    /// the menu was opened on; anything picked past the first goes to a
    /// lane of its own.
    InsertMedia(usize),
    /// **A saved drum kit to load onto the showing lane.** Every sample it
    /// names is copied into the open project, exactly as dragging the files
    /// in one at a time would — see [`crate::drumpad::KitPreset`].
    KitPreset,
}

/// A place the app has asked the host to raise a SAVE panel for.
///
/// The same request pattern as [`FileRequest`]; the answer comes back
/// through `TangentApp::finish_save_choice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub title: String,
    /// The name the panel opens with.
    pub suggested: String,
    pub purpose: SavePurpose,
}

/// Why a save location is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePurpose {
    /// Duplicate the whole open project — document and media — to the
    /// chosen place, and open the copy.
    ProjectCopy,
    /// Save the open project as a TEMPLATE. Only the NAME is honoured:
    /// templates live under `Projects/Templates/<name>` wherever the panel
    /// was pointed, and saving one called `default` replaces the session
    /// every New Project clones.
    Template,
    /// **Write the showing lane's kit out as a preset.** Unlike a template,
    /// the whole path is honoured — a kit is a file you keep wherever you
    /// keep files.
    KitPreset,
}

/// A project action the HOST must run whole — it owns the plugin states
/// and the template shelf, so the app only asks.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectRequest {
    /// Clone the `default` template into a fresh Untitled and open it.
    ///
    /// **Asks first if the scratch it would reclaim holds recordings.** The
    /// reclaim rule — an Untitled nobody SAVED is taken back, so New lands on
    /// "Untitled" instead of minting a pile — used to run `remove_dir_all`
    /// over that folder, `media/` and all, with no warning. A take you
    /// recorded and did not name is still a take you played, and it is the
    /// one thing in this app that cannot be made again.
    ///
    /// The host answers this with [`ScratchWarning`] instead of acting when
    /// there is something to lose; the two variants below are what the
    /// dialog's buttons send back.
    New,
    /// New, and leave the unsaved scratch exactly where it is — a fresh
    /// "Untitled 2" rather than a reclaim. The SAFE answer, and the default.
    NewKeeping,
    /// New, and reclaim the scratch, recordings included. Reachable only from
    /// the confirmation.
    NewDiscarding,
    /// Write everything down now: plugin states to the project's shelf,
    /// the desk into the document.
    Save,
    /// `Save`, then copy the whole session folder to
    /// `Projects/Templates/<name>`, replacing what was there.
    Template(String),
    /// A full export by SPEC: sources, rate, formats, and whether it runs
    /// offline (default) or as a realtime pass.
    Export(ProjectExportSpec),
    /// Bounce the arrangement to one WAV in `<project>/export/` — a real
    /// pass through the engine, racks and limiter included.
    ExportTrack,
    /// The same pass, but every used lane lands in its own WAV under
    /// `<project>/export/stems/`.
    ExportStems,
}

/// What the host found in the scratch project New would reclaim.
///
/// Plain data across the firewall, like everything else a panel displays
/// about the disk: the host counts, the app asks the question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchWarning {
    /// The folder's name, so the question can say which session it means.
    pub name: String,
    /// How many files are sitting in its `media/`.
    pub takes: usize,
}

/// Why a folder is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirPurpose {
    /// The export dialog's destination folder.
    ExportDir,
    /// Where takes are written.
    RecordRoot,
    /// Another place to look for VST3 bundles.
    PluginFolder,
    /// Where PROJECTS are written — the settings panel's Project tier, and a
    /// standing choice rather than one export's destination.
    ProjectRoot,
    /// Where an export lands when the export dialog names no folder. The
    /// standing default; [`DirPurpose::ExportDir`] is this one export.
    ExportRoot,
    /// Where saved presets live — the fourth standing folder. Appended at
    /// the end, like every variant this enum gains.
    PresetRoot,
    /// Where captured impulse responses live — the fifth standing folder.
    /// Appended at the end, like every variant this enum gains.
    IrsRoot,
}

// ── presets ─────────────────────────────────────────────────────────────────
//
// The preset road, end to end: the host scans `Presets/<unit's folder>/` and
// pushes rows of plain data (the cartridge-list pattern); a panel asks for a
// save, a load or a delete through `RecorderRequest`; the host does the disk
// work and answers a load with [`PresetLoaded`]. `tangent-ui` never touches
// the files — see `crate::preset` for the file format itself, which lives in
// this crate only as data and serialization.

/// One saved preset the host found for a unit. Names and paths, never file
/// contents: the UI draws a list and hands the path back, exactly as the
/// plugin picker holds bundle paths rather than modules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresetRow {
    /// What the preset calls itself — the file's own `name`, or its stem
    /// when the file does not say.
    pub name: String,
    /// The file, absolute, for a later `LoadPreset`/`DeletePreset` to name.
    pub path: String,
}

/// A preset the host has read back, for the app to apply to `unit`.
///
/// The values are `(symbol, value)` pairs rather than a struct per unit:
/// symbols are the append-only address space every built-in already persists
/// by (see `builtin_fx`'s `ParamSpec` in the binary), so a preset from a
/// newer build carries symbols this build skips and one from an older build
/// simply leaves the missing knobs at their defaults.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetLoaded {
    /// The unit the FILE says it belongs to — "@tangent-eq", "keys:2". The
    /// applying side compares this against the unit it asked for, so a file
    /// moved into the wrong folder moves nothing.
    pub unit: String,
    pub name: String,
    pub values: Vec<(String, f64)>,
}

/// A source of MIDI the app picks for itself.
///
/// `Send` because the app that owns it has to be: `nih_plug_egui` requires the
/// editor's state to be `'static + Send`, and a trait object is only as `Send`
/// as its bound says.
pub trait MidiPorts: Send {
    /// Every input port that could be opened, right now. Re-read each time the
    /// dialog opens: devices are unplugged while the app is running.
    fn list(&self) -> Vec<String>;

    /// Open this port, closing whatever was open. `Err` carries a message
    /// meant for the user, so it says what failed rather than which type did.
    fn connect(&mut self, name: &str, tx: mpsc::Sender<MidiEvent>) -> Result<(), String>;

    /// The port currently open, if any.
    fn current(&self) -> Option<String>;

    /// Open whatever the startup priority chain picks — **deferred, and that
    /// is the point.**
    ///
    /// Doing this while the window is being built cost 695 ms of the 790 ms
    /// before the first frame: opening a MIDI client is the first CoreMIDI
    /// call in the process and it pays for the whole subsystem coming up.
    /// The app looked like it hung, and the splash — which is painted by the
    /// app — could not appear until it was over, so the splash was proof the
    /// load had FINISHED rather than cover for it happening.
    ///
    /// Default is a no-op, so the plugin and the test fake say nothing.
    fn auto_connect(&mut self, _tx: mpsc::Sender<MidiEvent>) {}
}

// ── built-in insert faces ───────────────────────────────────────────────────
// Appended as a section of its own: two sibling lanes edit this file in
// parallel worktrees, and a block at the end merges where an insertion
// mid-file conflicts.

/// One knob of a built-in effect's face, current value included.
///
/// The [`FxParam`] idea with the state key and the live value added: an
/// automation menu only needs identity and a default, but a face DRAWS the
/// knob, so it needs where the knob stands now — and the host owns that
/// number (the engine-side ledger is the durable truth), so it crosses the
/// firewall as plain data like everything else a panel displays.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinFaceParam {
    /// The unit's own stable id — what `RecorderRequest::SetInsertParam`
    /// names, and what automation persists as `fx:<bay>:<id>`.
    pub id: u32,
    /// The state blob's key. Carried so a face can be tested against the
    /// spec without reaching across the firewall for it.
    pub symbol: String,
    pub name: String,
    /// "dB", "Hz", "%" — what the value READS in. Empty for a plain 0..=1.
    pub units: String,
    /// Where the knob rests, normalised.
    pub default: f32,
    /// What the value READS as right now, printed by the unit's own
    /// mapping on the host side — the one place the number is turned into
    /// words, so the face and the sound can never disagree. Lags a drag by
    /// one push, which is a frame.
    pub reading: String,
    /// A stepped parameter's positions in order, empty when continuous.
    pub choices: Vec<String>,
    /// 0 continuous, 2 a toggle, otherwise the number of steps.
    pub steps: i32,
    /// Where the knob stands now, normalised — the host's ledger reading,
    /// refreshed on load and mirrored optimistically on a local drag.
    pub value: f32,
}

/// Everything one built-in insert bay's face needs to draw itself, pushed by
/// the host whenever the bay loads or changes — the `push_fx_params`
/// precedent, with values.
///
/// `strip` is the SEAT (the flat rack-seat space, stride `INSERTS`), the
/// addressing every insert road here already speaks. An empty `params` with
/// an empty `path` clears the entry: the bay unloaded or holds a plugin.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BuiltinFace {
    /// The EQ's magnitude response over the face kit's own log grid
    /// (`face::CURVE_POINTS` from `CURVE_LO_HZ` to `CURVE_HI_HZ`), in dB —
    /// computed with the audio path's math on the host side. Empty for
    /// units without a curve.
    pub curve_db: Vec<f32>,
    /// The bay's extra state lines (the denoise's learned profile, the
    /// IR's file path) - pushed so a face can SHOW what is loaded without
    /// a second road. Appended; empty for units that carry none.
    pub extra: Vec<(String, String)>,
    pub strip: usize,
    pub slot: usize,
    /// The unit's sentinel path (`"@tangent-comp"`), which is what routes a
    /// click on the chip to the right face.
    pub path: String,
    pub params: Vec<BuiltinFaceParam>,
}

// ── the IR shelf and the wizard's live picture ──────────────────────────────
// Appended as a section of its own, exactly as the faces' section above is:
// sibling lanes edit this file in parallel worktrees, and a block at the end
// merges where a mid-file insertion conflicts.

/// One impulse response the host found under `IRs/`. Names and paths, never
/// samples: the UI draws a list and hands the path back, the [`PresetRow`]
/// bargain exactly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IrRow {
    /// The file's stem — an IR file has no name field of its own.
    pub name: String,
    /// The file, absolute, for a convolver picker to load by.
    pub path: String,
    /// How long the IR runs, read from the WAV header alone so a shelf of
    /// fifty files costs fifty header reads and no decoding.
    pub seconds: f32,
}

/// What one finished capture measured, pushed by the host for the wizard's
/// result stage to draw. Plain data over the firewall: the samples stay on
/// the host, which is where saving happens.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IrOutcome {
    /// The IR's level over time in dB, both channels folded to the louder,
    /// evenly spaced over `seconds` — the picture the result stage paints.
    pub envelope_db: Vec<f32>,
    /// Length of the processed IR.
    pub seconds: f32,
    /// Seconds of tail above the measured noise floor — the usable decay.
    pub usable_s: f32,
    /// The loop's own floor, read from the capture's pre-roll silence, dBFS.
    pub noise_floor_db: f32,
    /// 2nd/3rd-harmonic energy against the linear response, dB — the free
    /// THD probe an exponential sweep carries (Farina).
    pub thd2_db: f32,
    pub thd3_db: f32,
    /// Whether a loop-only reference pass divided out the interface.
    pub has_reference: bool,
}

/// The wizard's live picture, pushed by the host once per frame while the
/// IR road is doing anything: return levels during the tone and the sweep,
/// capture progress, and the finished result when there is one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IrLive {
    /// Return RMS of the chosen input pair, dBFS. −200 when silent/idle.
    pub rms_db: f32,
    /// Return peak of the chosen input pair, dBFS, per block.
    pub peak_db: f32,
    /// A capture is armed or running.
    pub capturing: bool,
    /// 0..=1 of the capture buffer filled.
    pub progress: f32,
    /// What the road is doing, in words: "capturing", "deconvolving", "".
    pub phase: String,
    /// A reference pass has been captured and will divide the next DUT pass.
    pub has_reference: bool,
    /// The last finished capture's measurements, until a new arm clears it.
    pub result: Option<IrOutcome>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **An unknown path says NOTHING.**
    ///
    /// The default is what a Windows or Linux build returns today, and what
    /// macOS returns when the HAL will not answer. It has to read as "no
    /// information", never as "the path is clear" — a panel that quietly
    /// implies the output is direct is the exact false reassurance this
    /// whole readout exists to end.
    #[test]
    fn a_path_that_could_not_be_checked_makes_no_claim() {
        let unknown = OutputPath::default();
        assert!(!unknown.worth_saying());
        assert_eq!(unknown.extra_ms(48_000), None);
        // And a KNOWN path with nothing remarkable on it is equally silent:
        // an interface with no reported latency is the ordinary case and
        // deserves no line.
        let plain = OutputPath { known: true, transport: "USB".to_owned(), ..Default::default() };
        assert!(!plain.worth_saying());
    }

    /// The three things that ARE worth a line, and the arithmetic of the one
    /// that carries a number.
    #[test]
    fn a_virtual_device_and_a_reported_latency_both_speak() {
        let virt = OutputPath { known: true, virtualised: true, ..Default::default() };
        assert!(virt.worth_saying());

        let agg = OutputPath { known: true, aggregate: true, ..Default::default() };
        assert!(agg.worth_saying());

        let slow = OutputPath { known: true, extra_frames: 512, ..Default::default() };
        assert!(slow.worth_saying());
        // 512 frames at 48k is 10.666 ms, and the panel prints one decimal.
        let ms = slow.extra_ms(48_000).expect("512 frames is a duration");
        assert!((ms - 10.666).abs() < 0.001, "{ms}");
        // Halve the rate and the same frames are twice the time.
        let half = slow.extra_ms(24_000).expect("still a duration");
        assert!((half - 2.0 * ms).abs() < 0.001, "{half} against {ms}");
        // A rate of zero is a device that is not open. No division by it.
        assert_eq!(slow.extra_ms(0), None);
    }

    /// A recording fake, which is the other reason this is a trait: the port
    /// dialog and its two menu actions had no way to be tested at all while
    /// they went straight to a real device.
    #[derive(Default)]
    struct Fake {
        available: Vec<String>,
        open: Option<String>,
        /// Whether the priority chain was asked to run, and when.
        auto_calls: usize,
    }

    impl MidiPorts for Fake {
        fn list(&self) -> Vec<String> {
            self.available.clone()
        }
        fn connect(&mut self, name: &str, _tx: mpsc::Sender<MidiEvent>) -> Result<(), String> {
            if self.available.iter().any(|n| n == name) {
                self.open = Some(name.to_owned());
                Ok(())
            } else {
                Err(format!("no port named '{name}'"))
            }
        }
        fn current(&self) -> Option<String> {
            self.open.clone()
        }
        fn auto_connect(&mut self, _tx: mpsc::Sender<MidiEvent>) {
            self.auto_calls += 1;
            self.open = self.available.first().cloned();
        }
    }

    /// **The priority chain is a call somebody makes, not something a
    /// constructor does.**
    ///
    /// Opening a MIDI client is the first CoreMIDI call the process makes and
    /// it pays for the whole subsystem coming up — measured at 695 ms of the
    /// 790 ms before Tangent's first frame, on a machine where the window and
    /// the GPU together took 100. Because the splash is painted BY the app,
    /// nothing could be on screen until it was over: the window sat unpainted
    /// and the splash appeared only once the load had already finished.
    ///
    /// It runs on the frame after the splash is up now. This is the seam that
    /// makes that possible, and the default being a no-op is what lets the
    /// plugin and the fakes ignore it.
    #[test]
    fn the_priority_chain_is_a_separate_call_from_construction() {
        let (tx, _rx) = mpsc::channel();
        let mut p = Fake {
            available: vec!["USB-MIDI 1".into()],
            ..Default::default()
        };
        // Built, and nothing is open: construction opens no device.
        assert_eq!(p.current(), None);
        assert_eq!(p.auto_calls, 0);
        p.auto_connect(tx);
        assert_eq!(p.auto_calls, 1, "the chain was never asked to run");
        assert_eq!(p.current().as_deref(), Some("USB-MIDI 1"));
    }

    /// A source that has no chain says nothing rather than erroring — the
    /// plugin has no ports of its own to pick from.
    #[test]
    fn a_source_with_no_chain_ignores_the_request() {
        struct Bare;
        impl MidiPorts for Bare {
            fn list(&self) -> Vec<String> {
                Vec::new()
            }
            fn connect(&mut self, _n: &str, _tx: mpsc::Sender<MidiEvent>) -> Result<(), String> {
                Ok(())
            }
            fn current(&self) -> Option<String> {
                None
            }
        }
        let (tx, _rx) = mpsc::channel();
        let mut b = Bare;
        b.auto_connect(tx);
        assert_eq!(b.current(), None);
    }

    #[test]
    fn a_port_source_can_be_faked_without_a_device() {
        let (tx, _rx) = mpsc::channel();
        let mut p = Fake {
            available: vec!["Scarlett 2i2".into(), "USB-MIDI 1".into()],
            ..Default::default()
        };
        assert_eq!(p.current(), None);
        assert!(p.connect("USB-MIDI 1", tx.clone()).is_ok());
        assert_eq!(p.current().as_deref(), Some("USB-MIDI 1"));
        assert!(p.connect("Nothing", tx).is_err());
        assert_eq!(
            p.current().as_deref(),
            Some("USB-MIDI 1"),
            "a failed connect must not close the port that was working"
        );
    }

    /// The trait object has to be `Send`, or `TangentApp` stops being `Send` and
    /// `create_egui_editor` refuses it with a wall of trait errors in the
    /// plugin crate rather than here.
    #[test]
    fn the_trait_object_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Box<dyn MidiPorts>>();
        assert_send::<Box<dyn CaptureDevices>>();
    }

    /// A fake camera list, for the same reason the fake MIDI port exists: the
    /// Recorder band's device rows are otherwise untestable without hardware.
    #[derive(Default)]
    struct FakeDevices {
        present: Vec<DeviceInfo>,
        open: Option<String>,
    }

    impl CaptureDevices for FakeDevices {
        fn list(&self) -> Vec<DeviceInfo> {
            self.present.clone()
        }
        fn open(&mut self, uid: &str) -> Result<(), String> {
            if uid.is_empty() {
                self.open = None;
                return Ok(());
            }
            if self.present.iter().any(|d| d.uid == uid) {
                self.open = Some(uid.to_owned());
                Ok(())
            } else {
                Err(format!("no device with uid '{uid}'"))
            }
        }
        fn current(&self) -> Option<String> {
            self.open.clone()
        }
        fn current_name(&self) -> Option<String> {
            let uid = self.open.as_ref()?;
            self.present
                .iter()
                .find(|d| &d.uid == uid)
                .map(|d| d.name.clone())
        }
    }

    /// Two cameras that call themselves the same thing is the case that decides
    /// uid-not-name, so it is the case the fake is asked about.
    #[test]
    fn two_devices_sharing_a_name_are_still_told_apart() {
        let mut d = FakeDevices {
            present: vec![
                DeviceInfo {
                    uid: "0x1400000046d0825".into(),
                    name: "HD Pro Webcam C920".into(),
                    default: true,
                },
                DeviceInfo {
                    uid: "0x1a11000046d0825".into(),
                    name: "HD Pro Webcam C920".into(),
                    default: false,
                },
            ],
            open: None,
        };
        assert!(d.open("0x1a11000046d0825").is_ok());
        assert_eq!(d.current().as_deref(), Some("0x1a11000046d0825"));
        assert_eq!(d.current_name().as_deref(), Some("HD Pro Webcam C920"));
        assert!(d.open("gone").is_err());
        assert_eq!(
            d.current().as_deref(),
            Some("0x1a11000046d0825"),
            "a failed open must not close the device that was working"
        );
    }

    /// The None row. Expressed as an empty uid rather than a second method,
    /// which is the kind of thing that is obvious for a week and then not.
    #[test]
    fn an_empty_uid_closes_the_device() {
        let mut d = FakeDevices {
            present: vec![DeviceInfo {
                uid: "a".into(),
                name: "A".into(),
                default: true,
            }],
            open: Some("a".into()),
        };
        assert!(d.open("").is_ok());
        assert_eq!(d.current(), None);
        assert_eq!(d.current_name(), None);
    }
}
