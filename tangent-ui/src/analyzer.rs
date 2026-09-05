//! The spectrum analyzer that fills the camera pane when no camera is chosen.
//!
//! **A tool, not a visualiser** — the owner's condition for its existence.
//! The pane used to say NO CAMERA / Select Camera in the middle of a grey
//! well, on every launch, for everybody who records audio and not video —
//! which is most launches. What stands there now has to earn the room: a
//! reading you can act on, not a picture that moves.
//!
//! So it carries what a working analyzer carries. A decibel scale on BOTH
//! edges, printed as decibels below full scale with no minus signs (the
//! owner's rule, and the same convention the loudness ladder beside it
//! follows), so 0 is the ceiling and 90 is the floor. A frequency scale along
//! the foot, logarithmic by default because that is how pitch is heard and
//! how every EQ in this app draws its own curve. A readout under the pointer
//! — the frequency, the nearest note and how many cents off it, and the
//! level — because "what is that peak" is the question an analyzer gets
//! asked most, and in a music app the answer is a note. And a readout of the
//! loudest thing on screen, for the same reason. Everything else an analyzer
//! has that changes what the picture MEANS is behind the right button:
//!
//! - **Mode.** A curve, one point per pixel column, is the FFT's own shape.
//!   Third-octave and octave bars are the RTA: the energy in each standard
//!   band, which is what a room or a mix is tuned by.
//! - **Scale.** Logarithmic or linear frequency. Linear is the one that
//!   shows harmonics as evenly spaced teeth, which is what a harmonic
//!   series looks like and how aliasing is spotted.
//! - **Range.** 60, 90 or 120 dB of window. Deeper shows the noise floor;
//!   shallower spreads the top of the mix.
//! - **Average.** Off, fast or slow exponential smoothing. Off is honest
//!   and jittery; slow is what a mix's tonal balance is read from.
//! - **Peak.** Off, falling at 12 dB/s, or held until reset. A held peak is
//!   how a transient that lasted two frames is still there to be read.
//! - **Tilt.** 0, 3 or 4.5 dB per octave, rising. Pink noise — and most
//!   finished music — falls at about 3 dB an octave, so an untilted analyzer
//!   shows every mix as a slope down to the right and says nothing; tilted,
//!   flat means balanced. 4.5 is what the current generation of plug-in
//!   analyzers ship with; 3 is the pink-noise figure.
//! - **Window.** Hann, or Blackman–Harris. Hann is the sensible default;
//!   Blackman–Harris trades a wider main lobe for a leakage floor some
//!   40 dB lower, which is what separates a quiet partial from the skirt of
//!   a loud one an octave away.
//! - **FFT size.** 2048, 4096 or 8192 points. Bigger resolves lower and
//!   finer and responds slower; at 48 kHz that is 23, 12 and 6 Hz per bin.
//! - **Channel.** The mix, left, right, mid or side. Side is how a stereo
//!   image's spectral content is inspected without listening to it.
//! - **Freeze** holds the picture for reading, and **Reset peaks** clears a
//!   held ceiling.
//!
//! It reads the scope's ring — the master mix after the limiter and before
//! the click — so what it shows is what the speakers get, minus the
//! metronome. Pure painter like the scope: [`Analyzer::tick`] mutates,
//! [`Analyzer::draw`] takes `&self`.
//!
//! The FFT is the forty lines `faces.rs` already keeps for the EQ, for the
//! reason written there: a crate costs a THIRD-PARTY-LICENSES diff at
//! release and this does not.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

use crate::scope::{Scope, ScopeStyle};

/// The lowest and highest frequencies the picture covers. Twenty to twenty
/// thousand is hearing; the EQ's own curve uses the same pair.
pub const LO_HZ: f32 = 20.0;
pub const HI_HZ: f32 = 20_000.0;

/// Display bins for the curve: enough that a 2560-point pane has a bin per
/// pixel column and a 600-point one reduces four to a column.
const BINS: usize = 512;

/// The peak line falls at this rate in Decay mode. Slow enough to read a
/// transient that lasted two frames, fast enough that the line is not a
/// history lesson.
const PEAK_FALL_DB_S: f32 = 12.0;

/// The lowest level the picture will ever show, whatever the range says.
/// Below −140 the FFT is reading its own rounding.
const FLOOR_DB: f32 = -140.0;

/// The dB below full scale a spectrum reads as silence, for the resting
/// picture: the scope draws a flat line, this draws its scale and nothing
/// else.
const SILENT_DB: f32 = -110.0;

/// Third-octave band centres, the ISO 266 preferred frequencies. What a real
/// time analyzer has printed under its bars for fifty years.
const THIRD_OCTAVE_HZ: [f32; 31] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0,
    500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0,
    8000.0, 10_000.0, 12_500.0, 16_000.0, 20_000.0,
];

/// Octave band centres, the same series a third as dense.
const OCTAVE_HZ: [f32; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16_000.0,
];

macro_rules! cycling {
    ($(#[$m:meta])* $name:ident { $($v:ident => $key:literal, $label:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
        pub enum $name {
            #[default]
            $($v),+
        }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$v),+];
            pub fn label(self) -> &'static str {
                match self { $($name::$v => $label),+ }
            }
            pub fn key(self) -> &'static str {
                match self { $($name::$v => $key),+ }
            }
            pub fn from_key(k: &str) -> Option<Self> {
                match k { $($key => Some($name::$v),)+ _ => None }
            }
            /// The next choice, wrapping: what one press of the menu row does.
            pub fn next(self) -> Self {
                let i = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
                Self::ALL[(i + 1) % Self::ALL.len()]
            }
        }
    };
}

// The `#[default]` is the FIRST variant of each, so the shipped analyzer is
// the first row of every choice: a log curve over 90 dB, fast averaging, a
// falling peak, tilted 4.5 dB an octave, Hann, 4096 points, the mix.
cycling!(Mode { Curve => "curve", "Curve", Third => "third", "1/3 octave bars", Octave => "octave", "Octave bars" });
cycling!(Scale { Log => "log", "Logarithmic", Linear => "linear", "Linear" });
cycling!(Range { Db90 => "90", "90 dB", Db120 => "120", "120 dB", Db60 => "60", "60 dB" });
cycling!(Average { Fast => "fast", "Fast", Slow => "slow", "Slow", Off => "off", "Off" });
cycling!(Peak { Decay => "decay", "Falling", Hold => "hold", "Held", Off => "off", "Off" });
cycling!(Tilt { Four5 => "4.5", "4.5 dB/oct", Three => "3", "3 dB/oct", Zero => "0", "None" });
cycling!(Window { Hann => "hann", "Hann", BlackmanHarris => "bh", "Blackman-Harris" });
cycling!(Fft { N4096 => "4096", "4096", N8192 => "8192", "8192", N2048 => "2048", "2048" });
cycling!(Channel { Mix => "mix", "Mix", Left => "left", "Left", Right => "right", "Right", Mid => "mid", "Mid", Side => "side", "Side" });

impl Range {
    pub fn db(self) -> f32 {
        match self {
            Range::Db60 => 60.0,
            Range::Db90 => 90.0,
            Range::Db120 => 120.0,
        }
    }
}

impl Average {
    /// Time constant in seconds; `None` is no smoothing at all.
    fn tau(self) -> Option<f32> {
        match self {
            Average::Off => None,
            Average::Fast => Some(0.08),
            Average::Slow => Some(0.5),
        }
    }
}

impl Tilt {
    pub fn db_per_octave(self) -> f32 {
        match self {
            Tilt::Zero => 0.0,
            Tilt::Three => 3.0,
            Tilt::Four5 => 4.5,
        }
    }
}

impl Window {
    /// The window's value at sample `i` of `n`.
    fn at(self, i: usize, n: usize) -> f32 {
        let x = std::f32::consts::TAU * i as f32 / n as f32;
        match self {
            Window::Hann => 0.5 - 0.5 * x.cos(),
            // The four-term minimum-sidelobe window, Harris 1978.
            Window::BlackmanHarris => {
                0.358_75 - 0.488_29 * x.cos() + 0.141_28 * (2.0 * x).cos()
                    - 0.011_68 * (3.0 * x).cos()
            }
        }
    }

    /// The window's coherent gain — its mean — which is what a sine's
    /// measured amplitude has to be divided by to read true. Hann is a half;
    /// Blackman–Harris is its constant term.
    fn coherent_gain(self) -> f32 {
        match self {
            Window::Hann => 0.5,
            Window::BlackmanHarris => 0.358_75,
        }
    }
}

impl Fft {
    pub fn size(self) -> usize {
        match self {
            Fft::N2048 => 2048,
            Fft::N4096 => 4096,
            Fft::N8192 => 8192,
        }
    }
}

impl Channel {
    fn of(self, [l, r]: [f32; 2]) -> f32 {
        match self {
            Channel::Mix | Channel::Mid => (l + r) * 0.5,
            Channel::Left => l,
            Channel::Right => r,
            Channel::Side => (l - r) * 0.5,
        }
    }
}

/// Every choice the right button offers, as one value — what the settings
/// file keeps and what the menu shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Config {
    pub mode: Mode,
    pub scale: Scale,
    pub range: Range,
    pub average: Average,
    pub peak: Peak,
    pub tilt: Tilt,
    pub window: Window,
    pub fft: Fft,
    pub channel: Channel,
    /// Not persisted: a frozen analyzer on the next launch would be a broken
    /// one. Here rather than on the state so the menu can show it.
    pub frozen: bool,
}

/// One row of the analyzer's menu: which choice a press advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Mode,
    Scale,
    Range,
    Average,
    Peak,
    Tilt,
    Window,
    Fft,
    Channel,
    Freeze,
    ResetPeaks,
}

impl Param {
    /// The menu's rows, in the order they read: what the picture IS first,
    /// how it is measured second, the two verbs last.
    pub const ALL: [Param; 11] = [
        Param::Mode,
        Param::Scale,
        Param::Range,
        Param::Average,
        Param::Peak,
        Param::Tilt,
        Param::Window,
        Param::Fft,
        Param::Channel,
        Param::Freeze,
        Param::ResetPeaks,
    ];
}

impl Config {
    /// The settings-file form: the keys, comma-separated, in field order.
    /// One key rather than nine because it is one object — a person copies an
    /// analyzer setup whole, the way they copy a theme.
    pub fn to_key(self) -> String {
        [
            self.mode.key(),
            self.scale.key(),
            self.range.key(),
            self.average.key(),
            self.peak.key(),
            self.tilt.key(),
            self.window.key(),
            self.fft.key(),
            self.channel.key(),
        ]
        .join(",")
    }

    /// The inverse, lenient: a field this build does not know keeps its
    /// default rather than throwing the rest away, so a file from a newer
    /// build loses nothing it can read.
    pub fn from_key(s: &str) -> Self {
        let mut c = Self::default();
        let mut it = s.split(',').map(str::trim);
        macro_rules! take {
            ($field:ident, $t:ident) => {
                if let Some(v) = it.next().and_then($t::from_key) {
                    c.$field = v;
                }
            };
        }
        take!(mode, Mode);
        take!(scale, Scale);
        take!(range, Range);
        take!(average, Average);
        take!(peak, Peak);
        take!(tilt, Tilt);
        take!(window, Window);
        take!(fft, Fft);
        take!(channel, Channel);
        c
    }

    /// What the menu prints for a row: the name and the current choice.
    pub fn row_label(self, p: Param) -> String {
        match p {
            Param::Mode => format!("Mode: {}", self.mode.label()),
            Param::Scale => format!("Scale: {}", self.scale.label()),
            Param::Range => format!("Range: {}", self.range.label()),
            Param::Average => format!("Average: {}", self.average.label()),
            Param::Peak => format!("Peak hold: {}", self.peak.label()),
            Param::Tilt => format!("Tilt: {}", self.tilt.label()),
            Param::Window => format!("Window: {}", self.window.label()),
            Param::Fft => format!("FFT: {} points", self.fft.label()),
            Param::Channel => format!("Channel: {}", self.channel.label()),
            Param::Freeze => (if self.frozen { "Unfreeze" } else { "Freeze" }).to_owned(),
            Param::ResetPeaks => "Reset peaks".to_owned(),
        }
    }

    /// A press on a row. Returns whether the persisted part changed, so the
    /// caller knows to save.
    pub fn press(&mut self, p: Param) -> bool {
        match p {
            Param::Mode => self.mode = self.mode.next(),
            Param::Scale => self.scale = self.scale.next(),
            Param::Range => self.range = self.range.next(),
            Param::Average => self.average = self.average.next(),
            Param::Peak => self.peak = self.peak.next(),
            Param::Tilt => self.tilt = self.tilt.next(),
            Param::Window => self.window = self.window.next(),
            Param::Fft => self.fft = self.fft.next(),
            Param::Channel => self.channel = self.channel.next(),
            Param::Freeze => {
                self.frozen = !self.frozen;
                return false;
            }
            Param::ResetPeaks => return false,
        }
        true
    }
}

/// The frequency of MIDI note number `n`, and the reverse, both from A4.
fn hz_of_midi(n: f32) -> f32 {
    440.0 * 2.0_f32.powf((n - 69.0) / 12.0)
}
fn midi_of_hz(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// The nearest note to `hz` and how far off it, as "A4 +12".
///
/// Cents are printed with their sign because they are a DIRECTION, not a
/// level: the no-minus rule is about the decibel axes, where every number is
/// a depth below one ceiling and the sign said the same thing ninety times.
pub fn note_of(hz: f32, prefer_flats: bool) -> String {
    if !(hz > 0.0) || !hz.is_finite() {
        return String::new();
    }
    let m = midi_of_hz(hz);
    let n = m.round();
    let cents = ((m - n) * 100.0).round() as i32;
    let names = if prefer_flats {
        tangent_core::patterns::NOTE_NAMES_FLAT
    } else {
        tangent_core::patterns::NOTE_NAMES
    };
    let idx = (n as i32).rem_euclid(12) as usize;
    let octave = (n as i32).div_euclid(12) - 1;
    if cents == 0 {
        format!("{}{}", names[idx], octave)
    } else {
        format!("{}{} {:+}", names[idx], octave, cents)
    }
}

/// A frequency printed the way an analyzer prints it: "440", "1.2k", "20k".
pub fn hz_label(hz: f32) -> String {
    if hz >= 1000.0 {
        let k = hz / 1000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{}k", k.round() as i32)
        } else {
            format!("{k:.1}k")
        }
    } else {
        format!("{}", hz.round() as i32)
    }
}

/// The analyzer's state: what the last FFT read, smoothed and peak-held, on
/// the picture's own grid. Owned by the app beside the scope.
pub struct Analyzer {
    pub cfg: Config,
    rate: f32,
    /// The curve, dB per display bin, this frame's reading before smoothing.
    fine: Vec<f32>,
    /// The curve after averaging — what is drawn.
    avg: Vec<f32>,
    /// The curve's peak hold.
    peak: Vec<f32>,
    /// The bars: energy per third-octave band, smoothed, and its peak hold.
    third: Vec<f32>,
    third_peak: Vec<f32>,
    octave: Vec<f32>,
    octave_peak: Vec<f32>,
    /// The loudest display bin last frame, for the readout.
    loud_hz: f32,
    loud_db: f32,
    /// Whether the window that fed the last FFT touched full scale.
    clipped: bool,
    /// Whether anything has been read at all.
    fed: bool,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Analyzer {
    pub fn new(rate: f32) -> Self {
        Self {
            cfg: Config::default(),
            rate: rate.max(8_000.0),
            fine: vec![FLOOR_DB; BINS],
            avg: vec![FLOOR_DB; BINS],
            peak: vec![FLOOR_DB; BINS],
            third: vec![FLOOR_DB; THIRD_OCTAVE_HZ.len()],
            third_peak: vec![FLOOR_DB; THIRD_OCTAVE_HZ.len()],
            octave: vec![FLOOR_DB; OCTAVE_HZ.len()],
            octave_peak: vec![FLOOR_DB; OCTAVE_HZ.len()],
            loud_hz: 0.0,
            loud_db: FLOOR_DB,
            clipped: false,
            fed: false,
            re: Vec::new(),
            im: Vec::new(),
        }
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.max(8_000.0);
    }

    /// Clear the held ceilings — the menu's Reset peaks.
    pub fn reset_peaks(&mut self) {
        for v in self
            .peak
            .iter_mut()
            .chain(self.third_peak.iter_mut())
            .chain(self.octave_peak.iter_mut())
        {
            *v = FLOOR_DB;
        }
    }

    /// The loudest thing on screen: frequency and dB below full scale.
    pub fn loudest(&self) -> Option<(f32, f32)> {
        (self.fed && self.loud_db > SILENT_DB).then_some((self.loud_hz, self.loud_db))
    }

    /// The frequency of display bin `i` on the current scale.
    fn bin_hz(&self, i: usize) -> f32 {
        let t = i as f32 / (BINS - 1) as f32;
        match self.cfg.scale {
            Scale::Log => LO_HZ * (HI_HZ / LO_HZ).powf(t),
            Scale::Linear => LO_HZ + (HI_HZ - LO_HZ) * t,
        }
    }

    /// Where `hz` sits across the picture, 0..1, on the current scale.
    pub fn x_of_hz(&self, hz: f32) -> f32 {
        match self.cfg.scale {
            Scale::Log => (hz / LO_HZ).log2() / (HI_HZ / LO_HZ).log2(),
            Scale::Linear => (hz - LO_HZ) / (HI_HZ - LO_HZ),
        }
    }

    fn hz_of_x(&self, x: f32) -> f32 {
        match self.cfg.scale {
            Scale::Log => LO_HZ * (HI_HZ / LO_HZ).powf(x),
            Scale::Linear => LO_HZ + (HI_HZ - LO_HZ) * x,
        }
    }

    /// The reading one frame of the ring gives, then the smoothing and the
    /// peaks advanced by `dt`.
    pub fn tick(&mut self, scope: &Scope, dt: f32) {
        if self.cfg.frozen {
            return;
        }
        let n = self.cfg.fft.size();
        if scope.frames() < n {
            return;
        }
        if self.re.len() != n {
            self.re = vec![0.0; n];
            self.im = vec![0.0; n];
        }
        let mut peak = 0.0f32;
        for i in 0..n {
            let s = self.cfg.channel.of(scope.stereo_back(n - 1 - i));
            peak = peak.max(s.abs());
            self.re[i] = s * self.cfg.window.at(i, n);
            self.im[i] = 0.0;
        }
        self.clipped = peak >= 0.995;
        crate::faces::fft(&mut self.re, &mut self.im);

        // Single-sided amplitude, normalised for the window's coherent gain,
        // so a full-scale sine reads 0 dB at its bin whichever window is on.
        let half = n / 2;
        let norm = 2.0 / (n as f32 * self.cfg.window.coherent_gain());
        let rate = self.rate;
        let bin_of = |hz: f32| hz / rate * n as f32;
        let amp = |b: usize| {
            let b = b.clamp(1, half - 1);
            (self.re[b] * self.re[b] + self.im[b] * self.im[b]).sqrt() * norm
        };
        let to_db = |a: f32| (20.0 * a.max(1e-9).log10()).max(FLOOR_DB);
        let tilt = self.cfg.tilt.db_per_octave();
        let tilt_at = |hz: f32| tilt * (hz / 1000.0).log2();

        // **The curve: the loudest FFT bin in each display bin's span, or
        // the two nearest bins interpolated where the span is narrower than
        // a bin** — which is every low-frequency column of a log scale. A
        // peak pick is what an analyzer is; an interpolation is what keeps
        // the bottom octave from being a staircase of the same three bins.
        let mut loud = (0.0f32, FLOOR_DB);
        for i in 0..BINS {
            let f0 = if i == 0 { self.bin_hz(0) } else { (self.bin_hz(i - 1) + self.bin_hz(i)) * 0.5 };
            let f1 = if i + 1 == BINS { self.bin_hz(i) } else { (self.bin_hz(i) + self.bin_hz(i + 1)) * 0.5 };
            let (b0, b1) = (bin_of(f0), bin_of(f1));
            let a = if b1 - b0 >= 1.0 {
                let mut m = 0.0f32;
                let mut b = b0.ceil() as usize;
                while (b as f32) <= b1 {
                    m = m.max(amp(b));
                    b += 1;
                }
                m
            } else {
                let bc = bin_of(self.bin_hz(i));
                let lo = bc.floor();
                let t = bc - lo;
                amp(lo as usize) * (1.0 - t) + amp(lo as usize + 1) * t
            };
            let hz = self.bin_hz(i);
            let db = to_db(a) + tilt_at(hz);
            self.fine[i] = db;
            if db > loud.1 {
                loud = (hz, db);
            }
        }
        // **The readout is refined past the grid.** The loudest display bin
        // names a frequency to the grid's resolution — 995 Hz for a 1 kHz
        // sine on a 512-point log grid — and its level to the FFT bin's,
        // which on a sine that falls between two bins is a decibel low. A
        // parabola through the loudest FFT bin and its two neighbours, in
        // decibels, gives the peak's true position to a tenth of a bin and
        // its true level to a tenth of a decibel: the standard trick, and
        // the difference between a readout that says 1k / 6 dB and one that
        // says 995 / 7 for the same sine.
        if loud.1 > FLOOR_DB {
            let centre = bin_of(loud.0).round() as usize;
            let lo = centre.saturating_sub(2).max(1);
            let hi = (centre + 2).min(half - 2);
            let k = (lo..=hi).max_by(|&a, &b| amp(a).total_cmp(&amp(b))).unwrap_or(centre);
            let (a, b, c) = (to_db(amp(k - 1)), to_db(amp(k)), to_db(amp(k + 1)));
            let denom = a - 2.0 * b + c;
            let delta = if denom.abs() > 1e-6 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
            let hz = (k as f32 + delta) * rate / n as f32;
            let db = b - 0.25 * (a - c) * delta;
            loud = (hz, db + tilt_at(hz));
        }
        self.loud_hz = loud.0;
        self.loud_db = loud.1;

        // **The bars: the ENERGY in each band**, which is the RTA's meaning
        // and not the curve's. A third-octave band at 1 kHz is 231 Hz wide
        // and holds twenty FFT bins at this size; summing their power is
        // what a band level is, and picking their peak would read a sine
        // and a noise band of equal power ten decibels apart.
        let band = |centre: f32, width_octaves: f32| {
            let f0 = centre / 2.0_f32.powf(width_octaves * 0.5);
            let f1 = centre * 2.0_f32.powf(width_octaves * 0.5);
            let (b0, b1) = (bin_of(f0).max(1.0), bin_of(f1).min(half as f32 - 1.0));
            let mut p = 0.0f32;
            let mut b = b0.ceil() as usize;
            let mut count = 0;
            while (b as f32) <= b1 {
                let a = amp(b);
                p += a * a;
                count += 1;
                b += 1;
            }
            if count == 0 {
                // Narrower than a bin: read the bin it is in.
                let a = amp(bin_of(centre).round() as usize);
                p = a * a;
            }
            to_db(p.sqrt()) + tilt_at(centre)
        };
        let thirds: Vec<f32> = THIRD_OCTAVE_HZ.iter().map(|&c| band(c, 1.0 / 3.0)).collect();
        let octaves: Vec<f32> = OCTAVE_HZ.iter().map(|&c| band(c, 1.0)).collect();

        // Smoothing, then the peaks, on every series at once.
        let k = match self.cfg.average.tau() {
            None => 1.0,
            Some(tau) => 1.0 - (-dt.max(0.0) / tau).exp(),
        };
        let fall = PEAK_FALL_DB_S * dt.max(0.0);
        let peak_mode = self.cfg.peak;
        let settle = |avg: &mut [f32], peak: &mut [f32], now: &[f32]| {
            for i in 0..avg.len() {
                avg[i] += (now[i] - avg[i]) * k;
                peak[i] = match peak_mode {
                    Peak::Off => avg[i],
                    Peak::Decay => (peak[i] - fall).max(avg[i]),
                    Peak::Hold => peak[i].max(avg[i]),
                };
            }
        };
        let fine = self.fine.clone();
        settle(&mut self.avg, &mut self.peak, &fine);
        settle(&mut self.third, &mut self.third_peak, &thirds);
        settle(&mut self.octave, &mut self.octave_peak, &octaves);
        self.fed = true;
    }

    /// The curve as drawn, dB per display bin — for tests and readouts.
    pub fn curve(&self) -> &[f32] {
        &self.avg
    }

    /// The level under `x` (0..1 across the picture), on whichever series is
    /// showing, in dB below full scale as a positive number.
    fn level_at(&self, x: f32) -> f32 {
        let hz = self.hz_of_x(x);
        let db = match self.cfg.mode {
            Mode::Curve => {
                let i = ((x * (BINS - 1) as f32).round() as usize).min(BINS - 1);
                self.avg[i]
            }
            Mode::Third => nearest_band(&THIRD_OCTAVE_HZ, &self.third, hz),
            Mode::Octave => nearest_band(&OCTAVE_HZ, &self.octave, hz),
        };
        -db
    }

    /// Paint into `r`. `pointer` is where the hand is, in the same space as
    /// `r`, for the readout; `prefer_flats` is how the note is spelt.
    ///
    /// The layout is decided from the rect: the two scale columns and the
    /// frequency row are given up in that order as the pane shrinks, so a
    /// small pane is a picture without numbers and never numbers without a
    /// picture.
    pub fn draw(
        &self,
        painter: &Painter,
        r: Rect,
        style: &ScopeStyle,
        pointer: Option<Pos2>,
        prefer_flats: bool,
    ) {
        if !r.is_positive() {
            return;
        }
        painter.rect_filled(r, 0.0, style.face);
        let font_h = (r.height() * 0.055).clamp(7.0, 10.0);
        let font = FontId::new(font_h, crate::fonts::courier());
        let ink = style.trace;
        let dim = style.trace.gamma_multiply(0.62);
        let faint = style.grid;

        // The scale columns, if there is room for a number in them.
        let col_w = if r.width() >= 200.0 { font_h * 2.6 } else { 0.0 };
        let foot_h = if r.height() >= 90.0 { font_h * 1.5 } else { 0.0 };
        let head_h = if r.height() >= 90.0 { font_h * 1.6 } else { 0.0 };
        let plot = Rect::from_min_max(
            Pos2::new(r.left() + col_w, r.top() + head_h),
            Pos2::new(r.right() - col_w, r.bottom() - foot_h),
        );
        if !plot.is_positive() {
            return;
        }
        let range = self.cfg.range.db();
        // dB below full scale → y. 0 at the top, `range` at the foot.
        let y_of = |db_below: f32| plot.top() + (db_below / range).clamp(0.0, 1.0) * plot.height();
        let x_of = |hz: f32| plot.left() + self.x_of_hz(hz).clamp(0.0, 1.0) * plot.width();

        // ── the graticule ──────────────────────────────────────────────────
        // Horizontals every 10 dB — 20 when a deep range would print them on
        // top of each other — with the number on BOTH edges, no sign: it is a
        // depth below one ceiling, and a column of minuses says the same
        // thing every line. The ceiling and the floor are always labelled.
        let step = if range > 100.0 && plot.height() < 160.0 { 20.0 } else { 10.0 };
        let thin = Stroke::new(1.0, faint);
        let mut db = 0.0;
        while db <= range + 0.5 {
            let y = y_of(db);
            painter.add(Shape::dashed_line(
                &[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                thin,
                1.0,
                3.0,
            ));
            if col_w > 0.0 {
                let label = format!("{}", db as i32);
                let anchor_y = y.clamp(r.top() + font_h * 0.6, r.bottom() - font_h * 0.6);
                painter.text(
                    Pos2::new(plot.left() - 3.0, anchor_y),
                    Align2::RIGHT_CENTER,
                    &label,
                    font.clone(),
                    dim,
                );
                painter.text(
                    Pos2::new(plot.right() + 3.0, anchor_y),
                    Align2::LEFT_CENTER,
                    &label,
                    font.clone(),
                    dim,
                );
            }
            db += step;
        }
        // Verticals: the decades and their multiples on a log scale, every
        // 2 kHz on a linear one; labels on the ones a reader steers by.
        let (lines, labels): (Vec<f32>, Vec<f32>) = match self.cfg.scale {
            Scale::Log => {
                let mut lines = Vec::new();
                for decade in [10.0f32, 100.0, 1000.0, 10_000.0] {
                    for m in 1..10 {
                        let f = decade * m as f32;
                        if (LO_HZ..=HI_HZ).contains(&f) {
                            lines.push(f);
                        }
                    }
                }
                lines.push(HI_HZ);
                (lines, vec![20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0])
            }
            Scale::Linear => {
                let lines: Vec<f32> = (1..=10).map(|k| k as f32 * 2000.0).collect();
                (lines.clone(), (1..=5).map(|k| k as f32 * 4000.0).collect())
            }
        };
        for f in &lines {
            let x = x_of(*f);
            let strong = labels.contains(f);
            painter.add(Shape::dashed_line(
                &[Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0, if strong { faint.gamma_multiply(1.6) } else { faint }),
                1.0,
                3.0,
            ));
        }
        if foot_h > 0.0 {
            // Skip a label whose neighbour would print over it: on a narrow
            // pane the 20 and the 50 are eleven points apart.
            let mut last_x = f32::NEG_INFINITY;
            let room = font_h * 2.4;
            for f in &labels {
                let x = x_of(*f);
                if x - last_x < room {
                    continue;
                }
                last_x = x;
                painter.text(
                    Pos2::new(x, r.bottom() - foot_h * 0.5),
                    Align2::CENTER_CENTER,
                    hz_label(*f),
                    font.clone(),
                    dim,
                );
            }
        }
        painter.rect_stroke(plot, 0.0, Stroke::new(1.0, faint.gamma_multiply(2.0)), egui::StrokeKind::Inside);

        // ── the picture ────────────────────────────────────────────────────
        let silent = !self.fed || self.loud_db <= SILENT_DB;
        if !silent {
            match self.cfg.mode {
                Mode::Curve => {
                    // One point per pixel column, each the loudest display
                    // bin in its span — so a pane narrower than the grid
                    // still shows every peak the grid has.
                    let cols = (plot.width().floor() as usize).max(8);
                    let mut line = Vec::with_capacity(cols);
                    let mut hold = Vec::with_capacity(cols);
                    let mut fill = Vec::with_capacity(cols);
                    for c in 0..cols {
                        let i0 = BINS * c / cols;
                        let i1 = (BINS * (c + 1) / cols).max(i0 + 1).min(BINS);
                        let mut a = FLOOR_DB;
                        let mut p = FLOOR_DB;
                        for i in i0..i1 {
                            a = a.max(self.avg[i]);
                            p = p.max(self.peak[i]);
                        }
                        let x = plot.left() + (c as f32 + 0.5) * plot.width() / cols as f32;
                        line.push(Pos2::new(x, y_of(-a)));
                        hold.push(Pos2::new(x, y_of(-p)));
                        fill.push((x, y_of(-a)));
                    }
                    for (x, y) in &fill {
                        if plot.bottom() - y > 0.6 {
                            painter.line_segment(
                                [Pos2::new(*x, *y), Pos2::new(*x, plot.bottom())],
                                Stroke::new(1.0, style.fill),
                            );
                        }
                    }
                    if self.cfg.peak != Peak::Off {
                        painter.add(Shape::line(hold, Stroke::new(1.0, dim)));
                    }
                    painter.add(Shape::line(line.clone(), Stroke::new(3.0, style.glow)));
                    painter.add(Shape::line(line, Stroke::new(1.4, ink)));
                }
                Mode::Third | Mode::Octave => {
                    let (centres, levels, peaks, width): (&[f32], &[f32], &[f32], f32) =
                        match self.cfg.mode {
                            Mode::Third => (&THIRD_OCTAVE_HZ, &self.third, &self.third_peak, 1.0 / 3.0),
                            _ => (&OCTAVE_HZ, &self.octave, &self.octave_peak, 1.0),
                        };
                    for (i, &c) in centres.iter().enumerate() {
                        let f0 = c / 2.0_f32.powf(width * 0.5);
                        let f1 = c * 2.0_f32.powf(width * 0.5);
                        let x0 = x_of(f0.max(LO_HZ)) + 1.0;
                        let x1 = x_of(f1.min(HI_HZ)) - 1.0;
                        if x1 <= x0 {
                            continue;
                        }
                        let y = y_of(-levels[i]);
                        if plot.bottom() - y > 0.5 {
                            painter.rect_filled(
                                Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, plot.bottom())),
                                0.0,
                                style.fill,
                            );
                            painter.line_segment(
                                [Pos2::new(x0, y), Pos2::new(x1, y)],
                                Stroke::new(1.4, ink),
                            );
                        }
                        if self.cfg.peak != Peak::Off {
                            let yp = y_of(-peaks[i]);
                            if plot.bottom() - yp > 0.5 {
                                painter.line_segment(
                                    [Pos2::new(x0, yp), Pos2::new(x1, yp)],
                                    Stroke::new(1.0, dim),
                                );
                            }
                        }
                    }
                }
            }
        }

        // ── the readouts ───────────────────────────────────────────────────
        if head_h > 0.0 {
            let head_y = r.top() + head_h * 0.5;
            // The loudest thing on screen, with its note: the answer to the
            // question this instrument is asked most.
            if let Some((hz, db)) = self.loudest() {
                let words = format!(
                    "PEAK {} Hz  {}  {:.0} dB",
                    hz_label(hz),
                    note_of(hz, prefer_flats),
                    -db
                );
                painter.text(
                    Pos2::new(plot.left() + 4.0, head_y),
                    Align2::LEFT_CENTER,
                    words,
                    font.clone(),
                    ink,
                );
            }
            // What the picture is, in the corner: enough to know the tilt is
            // on and the average is slow without opening the menu.
            let tags = format!(
                "{}{} · {} · {} · {}",
                if self.cfg.frozen { "FROZEN · " } else { "" },
                match self.cfg.scale {
                    Scale::Log => "LOG",
                    Scale::Linear => "LIN",
                },
                self.cfg.range.label(),
                self.cfg.average.label().to_uppercase(),
                if self.cfg.tilt == Tilt::Zero {
                    "FLAT".to_owned()
                } else {
                    format!("{}/OCT", self.cfg.tilt.label().trim_end_matches(" dB/oct"))
                }
            );
            painter.text(
                Pos2::new(plot.right() - 4.0, head_y),
                Align2::RIGHT_CENTER,
                tags,
                FontId::new(font_h * 0.9, crate::fonts::courier()),
                dim,
            );
        }
        if let Some(p) = pointer.filter(|p| plot.contains(*p)) {
            let x = (p.x - plot.left()) / plot.width();
            let hz = self.hz_of_x(x);
            painter.line_segment(
                [Pos2::new(p.x, plot.top()), Pos2::new(p.x, plot.bottom())],
                Stroke::new(1.0, dim),
            );
            let level = if silent { None } else { Some(self.level_at(x)) };
            let words = match level {
                Some(l) => format!("{} Hz  {}  {:.1} dB", hz_label(hz), note_of(hz, prefer_flats), l),
                None => format!("{} Hz  {}", hz_label(hz), note_of(hz, prefer_flats)),
            };
            // Beside the hairline, on whichever side has the room.
            let w = words.chars().count() as f32 * font_h * 0.62 + 8.0;
            let (anchor, align) = if p.x + w + 6.0 <= plot.right() {
                (Pos2::new(p.x + 5.0, plot.top() + font_h * 0.9), Align2::LEFT_CENTER)
            } else {
                (Pos2::new(p.x - 5.0, plot.top() + font_h * 0.9), Align2::RIGHT_CENTER)
            };
            let galley_rect = Rect::from_min_size(
                if align == Align2::LEFT_CENTER { anchor - egui::vec2(0.0, font_h * 0.7) } else { anchor - egui::vec2(w, font_h * 0.7) },
                egui::vec2(w, font_h * 1.4),
            );
            painter.rect_filled(galley_rect, 2.0, style.face.gamma_multiply(0.92));
            painter.text(anchor, align, words, font.clone(), ink);
        }
        if self.clipped {
            painter.circle_filled(Pos2::new(plot.right() - 5.0, plot.top() + 5.0), 2.0, style.clip);
        }
    }
}

/// The level of the band whose centre is nearest `hz`.
fn nearest_band(centres: &[f32], levels: &[f32], hz: f32) -> f32 {
    let mut best = (f32::MAX, FLOOR_DB);
    for (c, l) in centres.iter().zip(levels) {
        let d = (c.log2() - hz.max(1.0).log2()).abs();
        if d < best.0 {
            best = (d, *l);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A scope fed `secs` of the given stereo generator.
    fn fed(secs: f32, mut gen: impl FnMut(usize) -> (f32, f32)) -> Scope {
        let mut s = Scope::new(RATE);
        let n = (secs * RATE) as usize;
        let mut buf = Vec::with_capacity(n * 2);
        for i in 0..n {
            let (l, r) = gen(i);
            buf.push(l);
            buf.push(r);
        }
        s.tick(&buf, secs);
        s
    }

    fn sine(hz: f32, amp: f32) -> impl FnMut(usize) -> (f32, f32) {
        move |i| {
            let v = amp * (std::f32::consts::TAU * hz * i as f32 / RATE).sin();
            (v, v)
        }
    }

    fn settled(a: &mut Analyzer, scope: &Scope) {
        // Averaging off and the peak off: the reading is the FFT's own.
        a.cfg.average = Average::Off;
        a.cfg.peak = Peak::Off;
        a.cfg.tilt = Tilt::Zero;
        a.tick(scope, 0.016);
    }

    /// **A sine reads at its frequency and its level**, whichever window is
    /// on — the normalisation that every number on the scale depends on.
    /// −6 dBFS at 1 kHz must read 1 kHz within a bin and 6 dB within one,
    /// for both windows, because a window's coherent gain is the thing a
    /// wrong constant hides behind. Revert-check, performed: set either
    /// gain to 1.0 and its row misses by six decibels.
    #[test]
    fn a_sine_reads_at_its_frequency_and_level() {
        let scope = fed(0.5, sine(1000.0, 0.5));
        for window in Window::ALL {
            let mut a = Analyzer::new(RATE);
            a.cfg.window = *window;
            settled(&mut a, &scope);
            let (hz, db) = a.loudest().expect("a sine is not silence");
            // To a tenth of a bin and a tenth of a decibel: the parabolic
            // refinement in `tick` is what makes these tolerances possible,
            // and the point of holding them this tight is that a readout
            // printed to the hertz has to be right to the hertz.
            assert!((hz - 1000.0).abs() < 2.0, "{window:?}: peak at {hz} Hz");
            assert!((db - (-6.02)).abs() < 0.2, "{window:?}: −6 dBFS read {db:.2} dB");
        }
    }

    /// **The tilt is a slope of exactly what it says.** Two equal sines an
    /// octave apart read equal untilted and `tilt` apart tilted, which is
    /// the whole meaning of "dB per octave" and the property that makes a
    /// pink mix read flat.
    #[test]
    fn the_tilt_lifts_each_octave_by_what_it_says() {
        let scope = fed(0.5, |i| {
            let t = i as f32 / RATE;
            let v = 0.25 * (std::f32::consts::TAU * 1000.0 * t).sin()
                + 0.25 * (std::f32::consts::TAU * 2000.0 * t).sin();
            (v, v)
        });
        let read = |tilt: Tilt| {
            let mut a = Analyzer::new(RATE);
            settled(&mut a, &scope);
            a.cfg.tilt = tilt;
            a.tick(&scope, 0.016);
            let at = |hz: f32| {
                let i = (a.x_of_hz(hz) * (BINS - 1) as f32).round() as usize;
                a.curve()[i - 2..=i + 2].iter().cloned().fold(FLOOR_DB, f32::max)
            };
            at(2000.0) - at(1000.0)
        };
        assert!(read(Tilt::Zero).abs() < 0.6, "untilted octave differs by {}", read(Tilt::Zero));
        assert!((read(Tilt::Three) - 3.0).abs() < 0.6, "3 dB/oct read {}", read(Tilt::Three));
        assert!((read(Tilt::Four5) - 4.5).abs() < 0.6, "4.5 dB/oct read {}", read(Tilt::Four5));
    }

    /// **A band reads its energy, not its loudest bin.** A sine and a noise
    /// band of the same power in a third-octave bar must read within a couple
    /// of decibels of each other, which the curve's peak pick would put ten
    /// apart — the difference between an RTA and a spectrum.
    #[test]
    fn a_bar_reads_energy() {
        let tone = fed(0.5, sine(1000.0, 0.5));
        // Band-limited noise around 1 kHz at the same RMS as the sine: sixteen
        // sines spread across the band, incoherent phases.
        let noise = fed(0.5, |i| {
            let t = i as f32 / RATE;
            let mut v = 0.0;
            for k in 0..16 {
                let f = 900.0 + 15.0 * k as f32;
                v += (std::f32::consts::TAU * f * t + k as f32 * 1.7).sin();
            }
            // Sixteen incoherent sines of amplitude a have RMS a·√8; the lone
            // sine has RMS 0.5/√2. Match them.
            let v = v * (0.5 / std::f32::consts::SQRT_2) / (8.0f32).sqrt();
            (v, v)
        });
        let mut a = Analyzer::new(RATE);
        a.cfg.mode = Mode::Third;
        settled(&mut a, &tone);
        let tone_db = a.third[17]; // 1 kHz
        settled(&mut a, &noise);
        let noise_db = a.third[17];
        assert!(
            (tone_db - noise_db).abs() < 2.5,
            "equal-power tone and noise read {tone_db:.1} and {noise_db:.1} in the same band"
        );
    }

    /// The menu's choices survive the settings file, and a file from a build
    /// that knows more keeps what this one can read.
    #[test]
    fn the_config_round_trips_and_forgives() {
        let mut c = Config::default();
        c.mode = Mode::Octave;
        c.range = Range::Db120;
        c.channel = Channel::Side;
        c.window = Window::BlackmanHarris;
        assert_eq!(Config::from_key(&c.to_key()), c);
        assert_eq!(Config::from_key(""), Config::default());
        assert_eq!(Config::from_key("garbage,,,"), Config::default());
        let newer = format!("{},something-new", c.to_key());
        assert_eq!(Config::from_key(&newer), c);
        // Every row cycles through every choice and comes home.
        for p in Param::ALL {
            let mut d = Config::default();
            let mut seen = 0;
            loop {
                d.press(p);
                seen += 1;
                if d == Config::default() || seen > 8 {
                    break;
                }
            }
            assert!(seen <= 8, "{p:?} never came home");
        }
    }

    /// The note readout: A4, the accidental in whichever spelling, and the
    /// cents with their sign.
    #[test]
    fn the_readout_names_the_note() {
        assert_eq!(note_of(440.0, false), "A4");
        assert_eq!(note_of(466.16, false), "A#4");
        assert_eq!(note_of(466.16, true), "Bb4");
        assert_eq!(note_of(261.63, false), "C4");
        assert!(note_of(445.0, false).starts_with("A4 +"), "{}", note_of(445.0, false));
        assert_eq!(hz_label(1000.0), "1k");
        assert_eq!(hz_label(1250.0), "1.2k");
        assert_eq!(hz_label(63.0), "63");
    }

    /// **Freeze holds, Reset clears, Hold holds.** Three verbs, each driven.
    #[test]
    fn freeze_and_reset_do_what_they_say() {
        let loud = fed(0.5, sine(1000.0, 0.5));
        let quiet = fed(0.5, sine(1000.0, 0.01));
        let mut a = Analyzer::new(RATE);
        a.cfg.average = Average::Off;
        a.cfg.peak = Peak::Hold;
        a.tick(&loud, 0.016);
        let held = a.peak.iter().cloned().fold(FLOOR_DB, f32::max);
        a.tick(&quiet, 0.016);
        let still = a.peak.iter().cloned().fold(FLOOR_DB, f32::max);
        assert!((held - still).abs() < 0.01, "a held peak fell from {held} to {still}");
        a.reset_peaks();
        a.tick(&quiet, 0.016);
        let after = a.peak.iter().cloned().fold(FLOOR_DB, f32::max);
        assert!(after < held - 20.0, "reset left the peak at {after}");
        a.cfg.frozen = true;
        let before = a.curve().to_vec();
        a.tick(&loud, 0.016);
        assert_eq!(a.curve(), &before[..], "a frozen analyzer moved");
    }

    /// **The painted frame carries the scale on both edges, no minus signs,
    /// and the bands along the foot.** Read off the shapes, not off the
    /// arithmetic: a label the layout laid out and then clipped is not a
    /// label.
    #[test]
    fn the_scale_is_on_both_edges_without_minuses_and_the_bands_along_the_foot() {
        let ctx = crate::face::fit::a_ctx(crate::fonts::FontChoice::default());
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let scope = fed(0.5, sine(1000.0, 0.5));
        let mut a = Analyzer::new(RATE);
        settled(&mut a, &scope);
        let r = Rect::from_min_size(Pos2::new(10.0, 10.0), egui::vec2(480.0, 200.0));
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let painter = ui.painter();
                a.draw(painter, r, &ScopeStyle::lcd(), None, false);
            });
        });
        let mut texts: Vec<(Rect, String)> = Vec::new();
        fn walk(s: &Shape, out: &mut Vec<(Rect, String)>) {
            match s {
                Shape::Text(t) => out.push((t.galley.rect.translate(t.pos.to_vec2()), t.galley.text().to_owned())),
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        let mid = r.center().x;
        let left: Vec<&String> = texts.iter().filter(|(b, _)| b.center().x < mid - r.width() * 0.3).map(|(_, s)| s).collect();
        let right: Vec<&String> = texts.iter().filter(|(b, _)| b.center().x > mid + r.width() * 0.3).map(|(_, s)| s).collect();
        for side in [&left, &right] {
            for want in ["0", "30", "60", "90"] {
                assert!(side.iter().any(|s| s.as_str() == want), "no {want} on one edge: {side:?}");
            }
        }
        assert!(
            texts.iter().all(|(_, s)| !s.contains('-') || s.contains("dB")),
            "a minus sign on the scale: {:?}",
            texts.iter().map(|(_, s)| s).collect::<Vec<_>>()
        );
        let foot: Vec<&String> = texts.iter().filter(|(b, _)| b.center().y > r.bottom() - 16.0).map(|(_, s)| s).collect();
        for want in ["100", "1k", "10k"] {
            assert!(foot.iter().any(|s| s.as_str() == want), "no {want} along the foot: {foot:?}");
        }
        assert!(
            texts.iter().any(|(_, s)| s.starts_with("PEAK ") && s.contains("1k Hz") && s.contains("6 dB")),
            "no peak readout: {:?}",
            texts.iter().map(|(_, s)| s).collect::<Vec<_>>()
        );
    }
}
