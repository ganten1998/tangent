//! The tape's start and stop: Play spins the whole desk up from a
//! standstill, and Stop or Pause wind it down to a halt, with the pitch
//! following the speed as a tape's does.
//!
//! # One speed, decided here, composed there
//!
//! The motor is a speed `s` in `[FLOOR, 1]`, decided once per pass on the
//! audio thread in DEVICE frames, and the renderer plays the desk at
//! `dial * s` through the varispeed path (`crate::varispeed`). It never
//! writes the dial: the host rewrites that every frame. Time is whole device
//! frames, every length is `round(seconds * rate)`, so 44.1, 48 and 96 kHz
//! run the same machine in seconds.
//!
//! # Pure, and the renderer's to wire
//!
//! Nothing here touches a buffer it was not handed or an atomic. The
//! renderer asks [`Motor::plan`] at the top of each pass, [`Motor::chunk`]
//! once per chunk from its transport, [`Motor::stage`] once the pass is
//! rendered, and applies the [`Stage`] to every destination with that
//! destination's [`Tone`]. Nothing here allocates after construction, locks
//! or panics, and every number that crosses in is sanitised: a NaN time, a
//! zero rate or a billion-frame pass leaves the speed in `[FLOOR, 1]` and
//! every gain in `[0, 1]`.
//!
//! # Where the numbers come from
//!
//! Published real-machine data for professional quarter-inch servo-capstan
//! machines (start and stop times, the capture criterion), the NAB playback
//! time constants for the reproducer, and the house's own declick, silence
//! and tail window. Each constant says which.

/// **The shortest and longest a start or a stop may take**, in seconds.
///
/// 0.1 s is the fastest start any machine documents (synchronous-capstan
/// machines); no machine documents a multi-second spin-down from play, so
/// ten seconds is an artistic reach with room. Restated from
/// `tangent_ui::motor`, as `varispeed::MAX_RATIO` restates the dial's
/// limit, and pinned equal to it by
/// `the_engine_takes_the_uis_start_and_stop_range`.
pub(crate) const MIN_S: f32 = 0.1;
pub(crate) const MAX_S: f32 = 10.0;

/// **The start out of the box: half a second**, the slowest start published
/// for professional quarter-inch servo-capstan machines at 15 in/s (0.5 s to
/// 0.1 % weighted flutter, on three of them). The slow end because the owner
/// asked for a slow start; its linear phase is 0.90 m/s^2 at 15 in/s.
pub(crate) const START_DEFAULT_S: f32 = 0.5;

/// **The stop out of the box: 0.7 s**, the slowest stop from play any
/// professional machine publishes (a 30 in/s figure; at 15 in/s published
/// stops are about 0.25 to 0.40 s). Its first deceleration is 1.05 m/s^2 at
/// 15 in/s.
pub(crate) const STOP_DEFAULT_S: f32 = 0.7;

/// **The start's finish line**: within 0.1 % of speed, the published start
/// criterion ("to 0.1 % ... flutter"). The start law's literals below are
/// derived from it, and the tests pin them to it.
#[cfg_attr(not(test), allow(dead_code, reason = "the start law's literals are derived from it; the tests pin them to it"))]
const CAPTURE_EPS: f64 = 1e-3;

/// What is left of the climb when the servo locks: the last tenth. The
/// start is constant acceleration up to 90 % of the climb (`1 - E0`, the
/// lock), then a critically damped capture with a matched slope; the lock's
/// height is a modelling choice.
const E0: f64 = 0.1;

/// **The start law's constants, written out in full** so no build computes
/// them differently. `Y` is the root of `(1 + y) e^(-2y) = CAPTURE_EPS / E0`,
/// `KAPPA = (1 - E0) + Y E0` the linear phase's slope in `u`, `U_C =
/// (1 - E0) / KAPPA` the lock's place in `u`, and `A = 2 KAPPA / E0` the
/// capture's rate, which is what makes the slope continuous at the lock and
/// puts `phi(1)` at `1 - CAPTURE_EPS` exactly.
#[cfg_attr(not(test), allow(dead_code, reason = "KAPPA, U_C and A are derived from it; the tests pin them to it"))]
const Y: f64 = 2.995122173123205;
const KAPPA: f64 = 1.1995122173123205;
const U_C: f64 = 0.7503049881530839;
const A: f64 = 23.99024434624641;

/// **The NAB reproducer's two time constants** for 7.5 and 15 in/s, in
/// seconds: the bass's 3180 us and the treble's 50 us.
const T1_S: f64 = 3180e-6;
const T2_S: f64 = 50e-6;

/// **Where the wind-down ends: `T2 / T1` of speed**, 1/63.6 (six octaves
/// down). The reproducer's middle closes there (see [`Tone`]): below it the
/// two shelves overlap and nothing of the programme is left to hear.
/// Derived, so no lossy literal stands in for it.
pub(crate) const FLOOR: f64 = T2_S / T1_S;

/// **Where the final fade begins: two octaves above the floor.** At and
/// above it the fade is exactly 1.
const KNEE: f64 = 4.0 * FLOOR;

/// **The stop's friction against its braking**, `beta = b tau / v0`: the
/// law `dv/dt = -(b + v / tau)` is dynamic braking proportional to speed
/// plus a constant drag, a linear ramp mixed with an exponential tail. A
/// modelling choice; it puts the default stop's first deceleration at
/// 1.05 m/s^2.
const FRICTION: f64 = 0.25;

/// `ln((1 + FRICTION) / (FLOOR + FRICTION))`: how many time constants a stop
/// from full speed takes to reach the floor, so `tau = stop_s / LAMBDA`.
const LAMBDA: f64 = 1.5484433997312583;

/// **The house declick, 10 ms**: `instrument::GAIN_TAU_SECONDS`, the fade
/// every gain jump takes so it is a fade and not a step. Every glide here
/// takes it: a dip before a start, a cut, a hold's release, a snap.
const CUT_S: f64 = 0.010;

/// **How long the reproducer runs on at full speed before it is switched
/// out**: 15.7 time constants of its slowest pole (the bass shelf's, `T1`),
/// so what is left of its transient when it is bypassed is under 1.5e-7 of
/// what it settled from.
const SETTLE_S: f64 = 0.050;

/// **The hold's window**, in seconds: it answers within a tenth of a second
/// of a tail reaching its end.
const HOLD_WINDOW_S: f64 = 0.1;

/// **How many windows the hold's envelope spans**: the loudest of the last
/// three (0.3 s), so a repeat of an echo less than 0.3 s from the last keeps
/// it held.
const HOLD_SPAN: usize = 3;

/// **The house's line between a floor and a decay, 1.5 dB**, as an
/// AMPLITUDE ratio because the hold compares peaks: the 1.5 dB the IR
/// settle (`instrument::SETTLE_FALL_RATIO`, a power ratio) and the tail
/// truncator use. An envelope that has not fallen by this much in
/// `HOLD_SPAN` windows is a floor, not a tail.
const HOLD_FALL: f32 = 0.841;

/// **What counts as silence, -80 dBFS**: `instrument::PARK_FLOOR`, the
/// house's own, restated because that one is private to the renderer.
const PARK_FLOOR: f32 = 1.0e-4;

/// **The longest a hold may last, eight seconds**: `instrument::RACK_PARK_S`,
/// where the racks park anyway. Restated for the same reason.
const RACK_PARK_S: f64 = 8.0;

// ── the laws ──────────────────────────────────────────────────────────────

/// Zero to one with zero slope at both ends.
fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// **The start's shape**, from 0 to `1 - CAPTURE_EPS` over `u` in `[0, 1]`:
/// constant acceleration to the lock, then a critically damped capture with
/// the same slope ("fixed DC current until phase lock, then PLL"). It never
/// overshoots, so the speed never passes the dial.
fn phi(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    if u <= U_C {
        KAPPA * u
    } else {
        let v = u - U_C;
        1.0 - (E0 + KAPPA * v) * (-A * v).exp()
    }
}

/// **The final fade** `smoothstep((log2 s - log2 FLOOR) / 2)`: unity at and
/// above [`KNEE`], 0.5 one octave above the floor, 0 at it. It hides the
/// subsonic tail and the DC hold a near-zero read speed leaves, over the
/// last two octaves of a stop (10.55 % of it) and the first two of a start
/// (4 %). Written as `log2(s / FLOOR)` so the octaves land exactly: 1 at the
/// knee, a half at twice the floor, and a NaN reads as the floor.
fn fade(s: f64) -> f64 {
    if s >= KNEE {
        return 1.0;
    }
    if !(s > FLOOR) {
        return 0.0;
    }
    smoothstep((s / FLOOR).log2() / 2.0)
}

/// `seconds * rate` as whole frames: nonsense, negatives and NaN are none.
fn frames_of(seconds_x_rate: f64) -> u64 {
    if seconds_x_rate.is_finite() && seconds_x_rate > 0.0 {
        // Saturating: a billion-second ramp is a long one, not a wrap.
        seconds_x_rate.round() as u64
    } else {
        0
    }
}

/// A start or stop time as the machine takes it: a time that is not a
/// number is the default, and every other is held to `MIN_S..=MAX_S`.
fn sane_time(s: f64, default: f32) -> f64 {
    if s.is_finite() {
        s.clamp(f64::from(MIN_S), f64::from(MAX_S))
    } else {
        f64::from(default)
    }
}

// ── the reproducer ────────────────────────────────────────────────────────

/// One first-order section, normalised: `y = b0 x + (b1 x1 - a1 y1)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Section {
    b0: f32,
    b1: f32,
    a1: f32,
}

impl Section {
    /// `g (1 + s / wz) / (1 + s / wp)`, bilinear with both corners
    /// prewarped. Each corner's warped angle is held inside `(0, 1.4]`
    /// radians, short of Nyquist, so a rate no device runs at still gives a
    /// stable section (`a1` in `(-1, 1)`) and finite coefficients.
    fn design(g: f64, wz: f64, wp: f64, rate: f64) -> Self {
        let half = 0.5 / rate;
        let warp = |w: f64| 1.0 / (w * half).clamp(1e-12, 1.4).tan();
        let kz = warp(wz);
        let kp = warp(wp);
        let a0 = 1.0 + kp;
        Self {
            b0: (g * (1.0 + kz) / a0) as f32,
            b1: (g * (1.0 - kz) / a0) as f32,
            a1: ((1.0 - kp) / a0) as f32,
        }
    }

    /// `self + (to - self) u`, coefficient by coefficient: two equal ends
    /// give that set back exactly.
    fn lerp(self, to: Self, u: f32) -> Self {
        Self {
            b0: self.b0 + (to.b0 - self.b0) * u,
            b1: self.b1 + (to.b1 - self.b1) * u,
            a1: self.a1 + (to.a1 - self.a1) * u,
        }
    }
}

/// **The reproducer's response at one motor speed `k`**: two first-order
/// shelves, designed once per pass end.
///
/// ```text
/// H_k(s) = [ (k + s T1) / (1 + s T1) ]  *  [ (1 + s T2) / (1 + s T2 / k) ]
///           bass: DC gain k                treble: HF gain k
/// ```
///
/// The NAB reproducer curve, built for full speed, read at the motor's
/// speed: as the tape slows the treble dulls (the top shelf's gain is `k`),
/// the middle holds, and the bass sinks (the head's output falls with the
/// speed, the bottom shelf's DC gain `k`). At `k = 1` both shelves are
/// flat, and the design gives `b0 == 1.0` and `b1 == a1` bit for bit, so a
/// settled state passes every sample untouched.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ToneCoefs {
    bass: Section,
    treble: Section,
}

impl ToneCoefs {
    /// The response at speed `k`, at `rate`. A speed that is not one, or a
    /// rate that is not, is the flat response, so nothing here is ever NaN.
    fn at(k: f64, rate: f64) -> Self {
        const FLAT: Section = Section { b0: 1.0, b1: 0.0, a1: 0.0 };
        if !(rate.is_finite() && rate > 0.0 && k.is_finite()) {
            return Self { bass: FLAT, treble: FLAT };
        }
        let k = k.clamp(FLOOR, 1.0);
        Self {
            bass: Section::design(k, k / T1_S, 1.0 / T1_S, rate),
            treble: Section::design(1.0, 1.0 / T2_S, k / T2_S, rate),
        }
    }

    fn lerp(self, to: Self, u: f32) -> Self {
        Self {
            bass: self.bass.lerp(to.bass, u),
            treble: self.treble.lerp(to.treble, u),
        }
    }
}

/// **One Direct Form I step, in the pinned arithmetic form.** The feedback
/// terms are summed FIRST and the direct term added to their difference:
/// with `b1 == a1` and a settled state (`x1 == y1`) the bracket is exactly
/// zero and the sample passes bit for bit. `(b0 x + b1 x1) - a1 y1` rounds
/// the input into the history before the cancellation and does not.
#[inline]
fn df1(x: f32, x1: f32, y1: f32, c: Section) -> f32 {
    let (b0, b1, a1) = (c.b0, c.b1, c.a1);
    b0 * x + (b1 * x1 - a1 * y1)
}

/// **One destination's reproducer state**: the bass and the treble
/// section's last input and output, four floats a channel, f32 Direct Form
/// I, allocated here and nowhere else.
///
/// One per destination (the device output, the take's tap, the scope, the
/// network send), because each carries its own signal; the [`Stage`] that
/// drives them is one, so all four are the same instant.
#[derive(Debug)]
pub(crate) struct Tone {
    state: Box<[[f32; 4]]>,
}

impl Tone {
    /// State for up to `channels` channels, all of it zero. A buffer wider
    /// than this has its extra channels gained but not toned.
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            state: vec![[0.0; 4]; channels.max(1)].into_boxed_slice(),
        }
    }

    /// Every section back to rest: what a bypass after the settle leaves,
    /// and what the next ramp starts from.
    pub(crate) fn clear(&mut self) {
        for s in self.state.iter_mut() {
            *s = [0.0; 4];
        }
    }

    /// One interleaved frame through both sections, in place, at `c`.
    fn frame(&mut self, frame: &mut [f32], c: ToneCoefs) {
        for (x, st) in frame.iter_mut().zip(self.state.iter_mut()) {
            let [bx1, by1, tx1, ty1] = *st;
            let bass = df1(*x, bx1, by1, c.bass);
            let y = df1(bass, tx1, ty1, c.treble);
            *st = [*x, bass, bass, y];
            *x = y;
        }
    }
}

// ── the machine ───────────────────────────────────────────────────────────

/// After a cut: what the level stood for when it was asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum After {
    /// A Play during a Stop's coast: cut, take the held return, spin up.
    Start,
    /// A locate during a coast, or a Stop after the Play that asked for a
    /// start: cut, and hold.
    Park,
}

/// **What the motor is doing**: each mode's speed, the clock the transport
/// keeps, whether seeks apply, and the gain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// At rest, at speed 1 (the dial alone): the transport is parked, seeks
    /// apply, the gain is the envelope (1, or 0 to 1 while holding).
    Parked,
    /// A Play from a parked desk that is still heard: the parked desk fades
    /// to silence over `CUT_S`, the start held, before the spin-up begins at
    /// the floor where nothing is heard.
    Dip,
    /// The start law, from the floor or from a coast's speed. Rolls; a
    /// locate is a jump.
    SpinUp,
    /// At speed. Rolls; seeks apply; the gain is 1.
    Running,
    /// The stop law. The tape coasts (rolls with the host's level down) and
    /// the transport's return is held until it has stopped.
    WindDown,
    /// The stop law carries on while the envelope cuts to silence over
    /// `CUT_S`; then a start or a hold.
    Cut(After),
}

/// **What the motor shows**, published once a pass for the host
/// (`Shared::motor_show`). Appended at the END only.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MotorShow {
    Parked = 0,
    /// Parked after a stop with the slowed tails held silent (see [`Hold`]).
    Holding,
    Dipping,
    SpinningUp,
    Running,
    WindingDown,
    Cutting,
}

impl MotorShow {
    /// Back from the byte it was published as; a byte no show has is
    /// `Parked`, the state that moves nothing.
    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Holding,
            2 => Self::Dipping,
            3 => Self::SpinningUp,
            4 => Self::Running,
            5 => Self::WindingDown,
            6 => Self::Cutting,
            _ => Self::Parked,
        }
    }

    /// **A tape coasting to a stop**: the playhead the host draws follows
    /// it, and the host asks for frames while it moves.
    pub(crate) fn coasting(self) -> bool {
        matches!(self, Self::WindingDown | Self::Cutting)
    }

    /// **The motor has done what the level asked**: at speed for a playing
    /// transport, at rest (holding or not) for a stopped one. What the
    /// host's exempt-edge latch waits for.
    pub(crate) fn settled(self, playing: bool) -> bool {
        if playing {
            self == Self::Running
        } else {
            matches!(self, Self::Parked | Self::Holding)
        }
    }
}

/// A change of the host's level the motor has not acted on yet. `hard`: it
/// came while the motor was not allowed, so the next pass snaps to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edge {
    up: bool,
    hard: bool,
}

/// **One pass's inputs**, read by the renderer at the pass's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlanIn {
    /// The pass's length in DEVICE frames.
    pub(crate) frames: u64,
    /// The device's rate. Not a positive number: not allowed.
    pub(crate) rate: f64,
    /// Whether the motor may move at all this pass (the setting on, a live
    /// host, no capture, no hardware loop, no IR job). False snaps it.
    pub(crate) ok: bool,
    /// A count-in STARTED this pass: the motor snaps and the gain jumps to
    /// 1 under the count's first click.
    pub(crate) count: bool,
    /// The cut generation (`Engine::cut_motor`); a new one cuts a coast.
    pub(crate) cut: u32,
    /// The start and stop times, in seconds, latched at each ramp's start.
    pub(crate) start_s: f64,
    pub(crate) stop_s: f64,
}

/// **What the motor decided for one pass.** Every field is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plan {
    /// The motor's speed at the pass's first and last frame; the renderer
    /// plays `dial * s`, linear in device time across the pass.
    pub(crate) s0: f64,
    pub(crate) s1: f64,
    /// Both ends at exactly 1: the song's click may sound.
    pub(crate) at_speed: bool,
    /// The reproducer is in circuit this pass.
    pub(crate) tone: bool,
    /// The reproducer's settle ends this pass: zero its state and bypass.
    pub(crate) tone_clear: bool,
    /// **Nothing for the output stage to do**: at rest or at speed, the
    /// envelope at exactly 1, no hold, the reproducer out and clear. A
    /// steady pass is bypassed and its samples are untouched.
    pub(crate) steady: bool,
    /// What the motor shows for this pass (`Shared::motor_show`).
    pub(crate) show: MotorShow,
}

impl Plan {
    /// A motor at rest and at speed: nothing to do.
    const STEADY: Self = Self {
        s0: 1.0,
        s1: 1.0,
        at_speed: true,
        tone: false,
        tone_clear: false,
        steady: true,
        show: MotorShow::Parked,
    };
}

/// **What one chunk of the transport does**, from [`Motor::chunk`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// The level is up and the start waits for the motor: the transport
    /// stays parked this chunk.
    pub(crate) hold_start: bool,
    /// The tape coasts: the transport rolls whatever the level says.
    pub(crate) coast: bool,
    /// A seek waits for the tape to stop: none is applied or acked.
    pub(crate) hold_seek: bool,
}

/// **The declick envelope**: `from + (to - from) smoothstep(t / len)`,
/// evaluated per sample while it glides, so a 10 ms glide lasts 10 ms at
/// every pass length. `t` is the time of the last sample already played;
/// the next pass's samples are `t + 1 ..= t + frames`, so a glide reaches
/// `to` on exactly its `len`th sample.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Env {
    from: f64,
    to: f64,
    t: u64,
    len: u64,
}

impl Env {
    const fn settled(v: f64) -> Self {
        Self { from: v, to: v, t: 0, len: 0 }
    }

    fn at(&self, t: u64) -> f64 {
        if t >= self.len {
            self.to
        } else {
            self.from + (self.to - self.from) * smoothstep(t as f64 / self.len as f64)
        }
    }

    /// The value of the last sample played.
    fn now(&self) -> f64 {
        self.at(self.t)
    }

    fn gliding(&self) -> bool {
        self.t < self.len
    }

    /// Done gliding, and at `v`.
    fn rests_at(&self, v: f64) -> bool {
        !self.gliding() && self.to == v
    }

    /// **From where it is now to `to`, over `len` frames.** A glide to
    /// where it already is, or over no frames, is no glide, and a glide
    /// already on its way to `to` keeps its own clock: a motor that is never
    /// allowed to move asks for one every pass, and must stay steady, or
    /// land when the first one said it would.
    fn glide_to(&mut self, to: f64, len: u64) {
        if self.gliding() && self.to == to {
            return;
        }
        let now = self.now();
        *self = if now == to || len == 0 {
            Self::settled(to)
        } else {
            Self { from: now, to, t: 0, len }
        };
    }
}

/// **After the stop: the slowed tails, held silent until they are over.**
///
/// The desk is the tape: reverb and delay tails were slowed with it, and
/// with no reset for hosted plugins they would come back at normal pitch
/// the moment the parked desk plays on. So a wind-down ends with the gain
/// at 0, holding, until a player is heard ([`Motor::stage`]'s `live`), the
/// tail has died (its envelope under [`PARK_FLOOR`]), what is left is the
/// desk's own floor (an envelope that stopped falling by [`HOLD_FALL`]
/// across [`HOLD_SPAN`] windows: hiss, an amp's idle hum, a drone is not a
/// tail), or [`RACK_PARK_S`] has passed.
///
/// The envelope is the loudest PRE-gain sample of the device output in each
/// [`HOLD_WINDOW_S`] window, the loudest of the last `HOLD_SPAN` windows,
/// so the gaps between an echo's repeats do not let go of a tail that is
/// still repeating.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hold {
    /// Frames held so far.
    held: u64,
    /// Frames into the window now open.
    win_t: u64,
    /// The loudest sample of the window now open.
    win_peak: f32,
    /// The last `2 * HOLD_SPAN` windows' peaks, oldest first.
    peaks: [f32; 2 * HOLD_SPAN],
    /// Windows closed so far, up to `2 * HOLD_SPAN`.
    filled: usize,
}

impl Hold {
    const fn new() -> Self {
        Self {
            held: 0,
            win_t: 0,
            win_peak: 0.0,
            peaks: [0.0; 2 * HOLD_SPAN],
            filled: 0,
        }
    }

    /// The loudest of `HOLD_SPAN` windows, `back` windows before the newest.
    fn envelope(&self, back: usize) -> f32 {
        let end = self.peaks.len() - back;
        self.peaks[end - HOLD_SPAN..end].iter().fold(0.0, |a, p| a.max(*p))
    }

    /// Whether the windows closed so far say the hold is over.
    fn over(&self) -> bool {
        let now = self.envelope(0);
        // The tail has died.
        if self.filled >= HOLD_SPAN && now < PARK_FLOOR {
            return true;
        }
        // It stopped falling: the desk's own floor, not a tail.
        self.filled >= 2 * HOLD_SPAN && now > HOLD_FALL * self.envelope(HOLD_SPAN)
    }

    /// **One pass of `frames` frames whose loudest pre-gain sample was
    /// `peak`**; true when the hold is over. A pass longer than a window
    /// closes every window it spans with its own peak, which ends within
    /// `2 * HOLD_SPAN + 1` of them however long the pass is (a run of equal
    /// windows is either under the floor or not falling), so this loop is
    /// bounded whatever it is handed.
    fn feed(&mut self, frames: u64, peak: f32, rate: f64) -> bool {
        let peak = if peak.is_finite() { peak.abs() } else { 0.0 };
        let window = frames_of(HOLD_WINDOW_S * rate).max(1);
        self.held = self.held.saturating_add(frames);
        let mut left = frames;
        loop {
            if self.win_t >= window {
                self.peaks.rotate_left(1);
                self.peaks[2 * HOLD_SPAN - 1] = self.win_peak;
                self.filled = (self.filled + 1).min(2 * HOLD_SPAN);
                self.win_t = 0;
                self.win_peak = 0.0;
                if self.over() {
                    return true;
                }
            }
            if left == 0 {
                break;
            }
            let take = left.min(window - self.win_t);
            self.win_peak = self.win_peak.max(peak);
            self.win_t += take;
            left -= take;
        }
        self.held >= frames_of(RACK_PARK_S * rate)
    }
}

/// Where the reproducer is: out and clear, in circuit, or running on at
/// full speed with this many frames of its settle left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToneRun {
    Off,
    On,
    Settle(u64),
}

/// **What the output stage does to one pass**, from [`Motor::stage`]:
/// `y = tone(x) * lerp(f0, f1, (j + 1) / n) * env_j`, the same for every
/// destination so they stay one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Stage {
    /// The pass's length, in device frames.
    frames: usize,
    /// Nothing to do: every sample is left exactly as it is.
    bypass: bool,
    /// The reproducer at the pass's two ends, interpolated per sample.
    tone: Option<(ToneCoefs, ToneCoefs)>,
    /// Zero each destination's reproducer before the pass.
    tone_clear: bool,
    /// The final fade at the pass's two ends.
    f0: f64,
    f1: f64,
    /// The envelope as the pass begins.
    env: Env,
}

impl Stage {
    /// A pass that is left alone.
    const BYPASS: Self = Self {
        frames: 0,
        bypass: true,
        tone: None,
        tone_clear: false,
        f0: 1.0,
        f1: 1.0,
        env: Env::settled(1.0),
    };

    /// The gain of the pass's `j`th sample: the fade, linear across the
    /// pass, times the envelope at that sample.
    fn gain(&self, j: usize) -> f64 {
        let n = self.frames.max(1) as f64;
        let u = (j + 1) as f64 / n;
        let fade = self.f0 + (self.f1 - self.f0) * u;
        (fade * self.env.at(self.env.t.saturating_add(j as u64 + 1))).clamp(0.0, 1.0)
    }

    /// **The pass's output stage, applied in place** to `buf`, interleaved
    /// at `channels`, through this destination's own `tone`. A pass whose
    /// every gain is 1 and whose reproducer is out leaves every sample as
    /// it is, bit for bit.
    pub(crate) fn apply(&self, tone: &mut Tone, buf: &mut [f32], channels: usize) {
        if self.bypass {
            return;
        }
        if self.tone_clear {
            tone.clear();
        }
        let ch = channels.max(1);
        let n = self.frames.min(buf.len() / ch);
        let unity = self.f0 == 1.0 && self.f1 == 1.0 && self.env.rests_at(1.0);
        if n == 0 || (unity && self.tone.is_none()) {
            return;
        }
        for (j, frame) in buf.chunks_exact_mut(ch).take(n).enumerate() {
            if let Some((c0, c1)) = self.tone {
                // Divided, not multiplied by a reciprocal: the last sample
                // lands on the pass's far end exactly.
                tone.frame(frame, c0.lerp(c1, (j + 1) as f32 / self.frames as f32));
            }
            if !unity {
                let g = self.gain(j) as f32;
                for x in frame.iter_mut() {
                    *x *= g;
                }
            }
        }
    }
}

/// **The tape's motor**: one per renderer. See the module note for who
/// calls what, and when.
#[derive(Clone, Debug)]
pub(crate) struct Motor {
    mode: Mode,
    /// The host level the mode stands for: up in Dip, SpinUp, Running and
    /// Cut(Start); down in Parked, WindDown and Cut(Park). `None` until the
    /// first chunk: a fresh renderer ADOPTS the first level it reads.
    acted: Option<bool>,
    /// An edge `chunk` saw that the next `plan` acts on.
    pending: Option<Edge>,
    /// A seek arrived while the tape coasted and waits for it to stop.
    seek_waiting: bool,
    /// The last cut generation seen, latched every pass in every mode.
    cut_seen: u32,
    /// Device frames into the current ramp.
    t: u64,
    /// The current ramp's length, in device frames.
    len: u64,
    /// The speed the current ramp started from.
    v0: f64,
    /// The stop law's time constant, in device frames.
    tau: f64,
    /// The speed at the last pass's end.
    speed: f64,
    env: Env,
    hold: Option<Hold>,
    tone_run: ToneRun,
    /// The device rate of the last pass, or 0 for none worth the name.
    rate: f64,
    /// The gain the last pass ended at: the fade times the envelope, or 1
    /// for a steady pass (`Motor::plan`).
    gain_end: f64,
    last: Plan,
}

impl Default for Motor {
    fn default() -> Self {
        Self::new()
    }
}

impl Motor {
    /// A motor at rest, at speed, at full gain, that has seen no level yet.
    pub(crate) const fn new() -> Self {
        Self {
            mode: Mode::Parked,
            acted: None,
            pending: None,
            seek_waiting: false,
            cut_seen: 0,
            t: 0,
            len: 0,
            v0: 1.0,
            tau: 0.0,
            speed: 1.0,
            env: Env::settled(1.0),
            hold: None,
            tone_run: ToneRun::Off,
            rate: 0.0,
            gain_end: 1.0,
            last: Plan::STEADY,
        }
    }

    /// The frames of the house declick at the last pass's rate.
    fn cut_len(&self) -> u64 {
        frames_of(CUT_S * self.rate)
    }

    /// **The start from `s_a`**, the torque of a full start: a partial
    /// climb takes the same acceleration, so its length is the full start's
    /// times the part of the climb left.
    fn spin_up_from(&mut self, s_a: f64, start_s: f64) {
        let s_a = if s_a.is_finite() { s_a.clamp(FLOOR, 1.0) } else { FLOOR };
        self.mode = Mode::SpinUp;
        self.v0 = s_a;
        self.t = 0;
        self.len = frames_of(start_s * self.rate * (1.0 - s_a) / (1.0 - FLOOR));
        self.speed = s_a;
    }

    /// **The stop from `v0`**, by the brakes' law, `tau` latched here: it
    /// reaches the floor after `tau ln((v0 + FRICTION) / (FLOOR + FRICTION))`,
    /// the whole `stop_s` from full speed.
    fn wind_down_from(&mut self, v0: f64, stop_s: f64) {
        let v0 = if v0.is_finite() { v0.clamp(FLOOR, 1.0) } else { 1.0 };
        self.mode = Mode::WindDown;
        self.v0 = v0;
        self.t = 0;
        self.tau = stop_s * self.rate / LAMBDA;
        self.len = frames_of(self.tau * ((v0 + FRICTION) / (FLOOR + FRICTION)).ln());
        self.speed = v0;
    }

    /// The start law at `t` frames into the ramp.
    fn start_at(&self, t: u64) -> f64 {
        let u = if self.len == 0 { 1.0 } else { t as f64 / self.len as f64 };
        (self.v0 + (1.0 - self.v0) * phi(u)).clamp(FLOOR, 1.0)
    }

    /// The stop law at `t` frames into the ramp: `(v0 + FRICTION)
    /// e^(-t/tau) - FRICTION`, written around `v0` so its first frame is
    /// `v0` exactly, and held at the floor once the ramp is over.
    fn stop_at(&self, t: u64) -> f64 {
        if t >= self.len || !(self.tau > 0.0) {
            return FLOOR;
        }
        let x = t as f64 / self.tau;
        (self.v0 + (self.v0 + FRICTION) * (-x).exp_m1()).clamp(FLOOR, 1.0)
    }

    /// **The steady state at once**: at speed if the level is up, at rest
    /// if not, nothing held. The gain never steps: out of a ramp, a dip, a
    /// cut or a hold the envelope glides from the gain the last pass ended
    /// at to 1, except for a count, whose first click starts in this pass's
    /// first frame and masks the jump. Already at rest or at speed, the
    /// envelope is left on its way: a motor that is not allowed snaps every
    /// pass, and each snap restarting the glide would never let it land.
    /// A held seek stays waiting; at rest and at speed seeks apply, so it
    /// lands in the next chunk.
    fn snap(&mut self, count: bool) {
        let moving = !matches!(self.mode, Mode::Parked | Mode::Running) || self.hold.is_some();
        let level = self.pending.take().map_or(self.acted, |e| Some(e.up));
        self.mode = if level == Some(true) { Mode::Running } else { Mode::Parked };
        self.acted = level;
        self.hold = None;
        self.speed = 1.0;
        self.t = 0;
        self.len = 0;
        if count {
            self.env = Env::settled(1.0);
        } else if moving {
            self.env = Env::settled(self.gain_end.clamp(0.0, 1.0));
            self.env.glide_to(1.0, self.cut_len());
        }
    }

    /// **A ramp that ended in the last pass hands over**: a dip at
    /// silence spins up from the floor, a start at its length is at speed,
    /// a stop at its length parks holding, a cut at silence starts or
    /// holds.
    fn end_ramps(&mut self, start_s: f64) {
        match self.mode {
            Mode::Dip if self.env.rests_at(0.0) => {
                self.spin_up_from(FLOOR, start_s);
                self.env = Env::settled(1.0);
            }
            Mode::SpinUp if self.t >= self.len => {
                self.mode = Mode::Running;
                self.speed = 1.0;
            }
            Mode::WindDown if self.t >= self.len => self.park_holding(),
            Mode::Cut(After::Start) if self.env.rests_at(0.0) => {
                self.spin_up_from(FLOOR, start_s);
                self.env = Env::settled(1.0);
            }
            Mode::Cut(After::Park) if self.env.rests_at(0.0) => self.park_holding(),
            _ => {}
        }
    }

    /// At rest after a stop: the gain at 0, the tails held.
    fn park_holding(&mut self) {
        self.mode = Mode::Parked;
        self.speed = 1.0;
        self.t = 0;
        self.len = 0;
        self.env = Env::settled(0.0);
        self.hold = Some(Hold::new());
    }

    /// **A soft edge, acted on.** Every row starts its ramp at
    /// `t = 0`; a cut keeps the stop law's own clock.
    fn take_edge(&mut self, up: bool, start_s: f64, stop_s: f64) {
        let cut = self.cut_len();
        match (self.mode, up) {
            // Silent (holding): spin up from the floor directly.
            (Mode::Parked, true) if self.env.now() == 0.0 => {
                self.hold = None;
                self.spin_up_from(FLOOR, start_s);
                self.env = Env::settled(1.0);
            }
            // Heard: dip it to silence first.
            (Mode::Parked, true) => {
                self.hold = None;
                self.mode = Mode::Dip;
                self.env.glide_to(0.0, cut);
            }
            // Play after Stop: cut, then the held return, then the start.
            (Mode::WindDown, true) if self.seek_waiting => {
                self.mode = Mode::Cut(After::Start);
                self.env.glide_to(0.0, cut);
            }
            // Play after Pause: up again from where the tape is, in place.
            (Mode::WindDown, true) => self.spin_up_from(self.speed, start_s),
            (Mode::Cut(_), true) => self.mode = Mode::Cut(After::Start),
            (Mode::Running, false) => self.wind_down_from(1.0, stop_s),
            (Mode::SpinUp, false) => self.wind_down_from(self.speed, stop_s),
            (Mode::Cut(_), false) => self.mode = Mode::Cut(After::Park),
            // Nothing rolled yet (normally applied in the chunk itself).
            (Mode::Dip, false) => {
                self.mode = Mode::Parked;
                self.env.glide_to(1.0, cut);
            }
            // An edge the mode already stands for.
            _ => {}
        }
        self.acted = Some(up);
    }

    /// The reproducer's run for a pass: in while the speed is below 1, on
    /// at full speed for `SETTLE_S` after, then cleared and out.
    fn tone_step(&mut self, below: bool, frames: u64) -> (bool, bool) {
        match self.tone_run {
            _ if below => {
                self.tone_run = ToneRun::On;
                (true, false)
            }
            ToneRun::On => {
                let settle = frames_of(SETTLE_S * self.rate);
                self.tone_run = ToneRun::Settle(settle.saturating_sub(frames));
                (true, false)
            }
            ToneRun::Settle(left) if left > 0 => {
                self.tone_run = ToneRun::Settle(left.saturating_sub(frames));
                (true, false)
            }
            ToneRun::Settle(_) => {
                self.tone_run = ToneRun::Off;
                (false, true)
            }
            ToneRun::Off => (false, false),
        }
    }

    fn show(&self) -> MotorShow {
        match self.mode {
            Mode::Parked if self.hold.is_some() => MotorShow::Holding,
            Mode::Parked => MotorShow::Parked,
            Mode::Dip => MotorShow::Dipping,
            Mode::SpinUp => MotorShow::SpinningUp,
            Mode::Running => MotorShow::Running,
            Mode::WindDown => MotorShow::WindingDown,
            Mode::Cut(_) => MotorShow::Cutting,
        }
    }

    /// **The pass's plan, once per pass, before anything sizes the
    /// program**: the motor's speeds at the pass's two ends, its flags, and
    /// what it shows. In order: the cut latch, the ramps that ended, a snap
    /// (not allowed, a count, or a hard edge), the soft edge, the cut, and
    /// the pass's segment. The ramp's clock advances here; the envelope's
    /// and the hold's in [`Motor::stage`].
    pub(crate) fn plan(&mut self, inp: &PlanIn) -> Plan {
        let rate_ok = inp.rate.is_finite() && inp.rate > 0.0;
        self.rate = if rate_ok { inp.rate } else { 0.0 };
        let start_s = sane_time(inp.start_s, START_DEFAULT_S);
        let stop_s = sane_time(inp.stop_s, STOP_DEFAULT_S);
        // **The cut latch, every pass, in every mode**: a cut asked
        // outside a coast (every ruler click while playing asks one) is
        // forgotten here and can never fire on a later stop.
        let cut = inp.cut != self.cut_seen;
        self.cut_seen = inp.cut;
        self.end_ramps(start_s);
        if !inp.ok || !rate_ok || inp.count || self.pending.is_some_and(|e| e.hard) {
            self.snap(inp.count);
        } else {
            if let Some(edge) = self.pending.take() {
                self.take_edge(edge.up, start_s, stop_s);
            }
            if cut && self.mode == Mode::WindDown {
                self.mode = Mode::Cut(After::Park);
                self.env.glide_to(0.0, self.cut_len());
            }
        }
        let n = inp.frames;
        let (s0, s1) = match self.mode {
            Mode::Parked | Mode::Dip | Mode::Running => (1.0, 1.0),
            Mode::SpinUp => (self.start_at(self.t), self.start_at(self.t.saturating_add(n))),
            Mode::WindDown | Mode::Cut(_) => (self.stop_at(self.t), self.stop_at(self.t.saturating_add(n))),
        };
        let (tone, tone_clear) = self.tone_step(s0 < 1.0 || s1 < 1.0, n);
        self.t = self.t.saturating_add(n);
        self.speed = s1;
        let steady = matches!(self.mode, Mode::Parked | Mode::Running)
            && self.env.rests_at(1.0)
            && self.hold.is_none()
            && !tone
            && !tone_clear;
        // **A steady pass is at full gain, and says so here.** Its stage
        // never runs (the renderer skips it, and `Motor::stage` would bypass
        // it), so the stage cannot say it. A count's snap jumps the envelope
        // to 1 out of a hold whose reproducer has already settled, and that
        // pass is steady at once: left to the stage, the gain stayed at the
        // hold's 0, and the master meter read silence under a playing desk
        // until the next ramp.
        if steady {
            self.gain_end = 1.0;
        }
        self.last = Plan {
            s0,
            s1,
            at_speed: s0 == 1.0 && s1 == 1.0,
            tone,
            tone_clear,
            steady,
            show: self.show(),
        };
        self.last
    }

    /// **One chunk of the transport**, from `run_transport`, with the host's
    /// `level` (`Shared::rolling`), whether the motor may move (`ok`, read
    /// after the level), and whether a seek is waiting to be applied. It
    /// reads the MODE, not the plan: inside a pass the mode changes only by
    /// the two apply-now arms below (always to rest or speed, at the speed
    /// the pass was planned at) and by adoption.
    ///
    /// **The pass that meets Stop or Pause rolls on.** Its speed is
    /// fixed for the pass (the resampler's two ratios are), so the chunk
    /// that first sees the level fall and every chunk after it coast at the
    /// planned speed, apply no seek and ack nothing; the wind-down starts at
    /// the next pass from where the tape is, which is the same as starting
    /// it here.
    ///
    /// With the motor never allowed (every offline engine, every test that
    /// does not arm it) every edge is applied in the chunk that sees it and
    /// this returns `Chunk::default()`: today's transport, exactly.
    pub(crate) fn chunk(&mut self, level: bool, ok: bool, seek_pending: bool) -> Chunk {
        let Some(acted) = self.acted else {
            // A fresh renderer adopts the first level it reads, with no
            // edge: an engine rebuilt mid-song rolls at once and never spins
            // up because of a rebuild.
            self.acted = Some(level);
            if level {
                self.mode = Mode::Running;
            }
            return Chunk::default();
        };
        let coasting = matches!(self.mode, Mode::WindDown | Mode::Cut(_));
        if level == acted {
            // An edge undone inside the pass.
            self.pending = None;
            if seek_pending && coasting {
                self.seek_waiting = true;
            }
            // In a dip the level is up and the start waits for the motor.
            return Chunk {
                hold_start: level && self.mode == Mode::Dip,
                coast: coasting,
                hold_seek: coasting,
            };
        }
        let ramping = !matches!(self.mode, Mode::Parked | Mode::Running);
        if !level && self.mode == Mode::Dip {
            // Nothing has rolled yet: apply now; Stop's locate lands at once.
            self.mode = Mode::Parked;
            self.acted = Some(false);
            self.pending = None;
            self.env.glide_to(1.0, self.cut_len());
            return Chunk::default();
        }
        if !ok && !ramping {
            // Not allowed, at rest or at speed: exactly today's edge, in
            // this chunk.
            self.mode = if level { Mode::Running } else { Mode::Parked };
            self.acted = Some(level);
            self.pending = None;
            self.hold = None;
            self.env.glide_to(1.0, self.cut_len());
            return Chunk::default();
        }
        self.pending = Some(Edge { up: level, hard: !ok });
        if level {
            // Up from rest holds the start; up from a coast goes on coasting.
            if seek_pending && coasting {
                self.seek_waiting = true;
            }
            Chunk {
                hold_start: !coasting,
                coast: coasting,
                hold_seek: coasting,
            }
        } else {
            // Down from speed or from a start: the pass rolls on at its
            // planned speed, no seek applied or acked.
            if seek_pending {
                self.seek_waiting = true;
            }
            Chunk {
                hold_start: false,
                coast: true,
                hold_seek: true,
            }
        }
    }

    /// **The tape is moving under the transport's own level**: at speed or
    /// spinning up. A read of the transport that the host's stores tore (the
    /// generation moved between its two loads, after the level was read up)
    /// cannot tell a Stop from a locate for such a tape: the level it read
    /// may already have fallen. So that chunk rolls on and leaves the seek
    /// to the next, which reads the two whole (`Renderer::run_transport`).
    pub(crate) fn moving(&self) -> bool {
        matches!(self.mode, Mode::Running | Mode::SpinUp)
    }

    /// **A seek was applied**: the transport calls this whenever it applies
    /// one, so a seek held through a coast is waiting no longer.
    pub(crate) fn seek_applied(&mut self) {
        self.seek_waiting = false;
    }

    /// **At rest or at speed with no edge pending**: the varispeed path need
    /// not be ready for a ramp next pass. A hold or a glide does not engage
    /// the path; a pending edge does, because the next pass may begin a
    /// wind-down at ratio 1 that needs the path's program frames.
    pub(crate) fn idle(&self) -> bool {
        matches!(self.mode, Mode::Parked | Mode::Running) && self.pending.is_none()
    }

    /// **The gain the last pass ended at**, fade times envelope, in
    /// `[0, 1]`, and 1 once a steady pass is planned (it is at full gain
    /// throughout): the master meter's scale, read at the pass's start.
    pub(crate) fn gain_now(&self) -> f64 {
        self.gain_end
    }

    /// **The slowed tails are held**: the renderer measures the pass's
    /// pre-gain peak for [`Motor::stage`] only while this is true.
    pub(crate) fn holding(&self) -> bool {
        self.hold.is_some()
    }

    /// The last pass's plan.
    pub(crate) fn last(&self) -> Plan {
        self.last
    }

    /// **The pass's output stage, once the pass is rendered**: the hold
    /// heard and advanced, the envelope advanced, and the [`Stage`] every
    /// destination is put through.
    ///
    /// - `frames`: the pass's device frames, the plan's own.
    /// - `live`: a player was heard this pass (a live note or pad, an input
    ///   the room heard above the park floor, a preview). A hold lets go at
    ///   once, gliding up from the pass's first frame.
    /// - `landed`: the pass put every level on its target (a realtime
    ///   export's head). The envelope is exactly 1 for the pass and the
    ///   reproducer is cleared, so an export never starts under a glide.
    /// - `peak`: while [`Motor::holding`], the pass's loudest PRE-gain
    ///   sample of the device output; otherwise ignored.
    ///
    /// A steady pass returns a bypass at once and advances nothing (it has
    /// nothing to advance).
    pub(crate) fn stage(&mut self, frames: usize, live: bool, landed: bool, peak: f32) -> Stage {
        let plan = self.last;
        if plan.steady {
            return Stage::BYPASS;
        }
        let n = frames as u64;
        if let Some(hold) = self.hold.as_mut() {
            if live || hold.feed(n, peak, self.rate) {
                self.hold = None;
                self.env.glide_to(1.0, self.cut_len());
            }
        }
        let (mut tone, mut tone_clear) = (plan.tone, plan.tone_clear);
        if landed {
            self.env = Env::settled(1.0);
            self.hold = None;
            self.tone_run = ToneRun::Off;
            tone = false;
            tone_clear = true;
        }
        let stage = Stage {
            frames,
            bypass: false,
            tone: tone.then(|| (ToneCoefs::at(plan.s0, self.rate), ToneCoefs::at(plan.s1, self.rate))),
            tone_clear,
            f0: fade(plan.s0),
            f1: fade(plan.s1),
            env: self.env,
        };
        self.env.t = self.env.t.saturating_add(n);
        self.gain_end = (stage.f1 * self.env.now()).clamp(0.0, 1.0);
        stage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three rates every law is held at.
    const RATES: [f64; 3] = [44_100.0, 48_000.0, 96_000.0];

    /// A pass's inputs at `rate`: allowed, no count, no cut, the times out
    /// of the box.
    fn inp(rate: f64, frames: u64) -> PlanIn {
        PlanIn {
            frames,
            rate,
            ok: true,
            count: false,
            cut: 0,
            start_s: f64::from(START_DEFAULT_S),
            stop_s: f64::from(STOP_DEFAULT_S),
        }
    }

    /// **One pass as the renderer drives it**: the plan, one chunk with the
    /// host's level and whether a seek waits, the stage with nothing live,
    /// nothing landed and a silent desk.
    fn step(m: &mut Motor, i: &PlanIn, level: bool, seek: bool) -> (Plan, Chunk, Stage) {
        let p = m.plan(i);
        let c = m.chunk(level, i.ok, seek);
        let s = m.stage(i.frames as usize, false, false, 0.0);
        (p, c, s)
    }

    fn pass(m: &mut Motor, i: &PlanIn, level: bool) -> Plan {
        step(m, i, level, false).0
    }

    /// A renderer's motor that read `level` first: built under a stopped
    /// transport, or a playing one.
    fn adopted(i: &PlanIn, level: bool) -> Motor {
        let mut m = Motor::new();
        pass(&mut m, i, level);
        m
    }

    /// Passes at `level` until a plan shows `show`.
    fn until(m: &mut Motor, i: &PlanIn, level: bool, show: MotorShow) -> Plan {
        for _ in 0..100_000 {
            let p = pass(m, i, level);
            if p.show == show {
                return p;
            }
        }
        panic!("the motor never showed {show:?}");
    }

    /// From where `m` stands, the passes at `level` that show `during`, as
    /// (frames from the first of them, s0, s1), until one shows `after`; and
    /// that pass's frame.
    fn ramp_of(m: &mut Motor, i: &PlanIn, level: bool, during: MotorShow, after: MotorShow) -> (Vec<(u64, f64, f64)>, u64) {
        let mut ramp = Vec::new();
        let mut at = 0;
        for _ in 0..100_000 {
            let p = pass(m, i, level);
            if p.show == after && !ramp.is_empty() {
                return (ramp, at);
            }
            if p.show == during {
                ramp.push((at, p.s0, p.s1));
                at += i.frames;
            }
        }
        panic!("the ramp never ended");
    }

    /// **The speed the tape plays `x` frames into a ramp**: linear across the
    /// pass that holds `x`, which is how the resampler plays a pass.
    fn played(ramp: &[(u64, f64, f64)], frames: u64, x: f64) -> f64 {
        let &(t0, s0, s1) = ramp
            .iter()
            .find(|(t0, ..)| (*t0 as f64) <= x && x < (*t0 + frames) as f64)
            .expect("the instant is inside the ramp");
        s0 + (s1 - s0) * (x - t0 as f64) / frames as f64
    }

    /// The real road to each state, at `i`'s rate and pass.
    fn running(i: &PlanIn) -> Motor {
        adopted(i, true)
    }

    fn parked(i: &PlanIn) -> Motor {
        adopted(i, false)
    }

    /// A stop run out: playing, Stop, the wind-down to the floor, the hold.
    fn holding(i: &PlanIn) -> Motor {
        let mut m = running(i);
        until(&mut m, i, false, MotorShow::Holding);
        m
    }

    /// A hold whose reproducer has settled and been cleared, the tails
    /// still held: the gain at 0 and nothing else left to move.
    fn settled_hold(i: &PlanIn) -> Motor {
        let mut m = holding(i);
        loop {
            let p = pass(&mut m, i, false);
            assert_eq!(p.show, MotorShow::Holding, "the hold let go before its reproducer settled");
            if !p.tone && !p.tone_clear {
                return m;
            }
        }
    }

    /// The first pass of a start from the floor (out of a hold, so no dip).
    fn spinning(i: &PlanIn) -> Motor {
        let mut m = holding(i);
        until(&mut m, i, true, MotorShow::SpinningUp);
        m
    }

    /// The first pass of a stop from speed; `seek`: Stop's return waits.
    fn winding(i: &PlanIn, seek: bool) -> Motor {
        let mut m = running(i);
        step(&mut m, i, false, seek);
        assert_eq!(pass(&mut m, i, false).show, MotorShow::WindingDown);
        m
    }

    /// A stop past the knee, where the fade is under 1.
    fn deep_in_a_stop(i: &PlanIn) -> Motor {
        let mut m = winding(i, false);
        loop {
            let p = pass(&mut m, i, false);
            assert_eq!(p.show, MotorShow::WindingDown, "the stop ended before its fade began");
            if p.s1 < KNEE {
                return m;
            }
        }
    }

    /// Every gain a stage gives its pass.
    fn gains(st: &Stage, frames: usize) -> Vec<f64> {
        (0..frames).map(|j| st.gain(j)).collect()
    }

    /// **A start reaches speed in its own time at every rate**, by the
    /// servo's law.
    ///
    /// A start from the floor is constant acceleration to the lock and a
    /// critically damped capture, `start_s` long: at 0.5 s it is at speed
    /// at the first pass boundary at or after 22050 / 24000 / 48000 frames,
    /// and the tape plays, at u = 0.25, 0.5, 0.75 and 0.9 of it, the
    /// published shape's 0.31089, 0.60605, 0.90121 and 0.99242, within
    /// 0.004. Read as the resampler plays it, linear across the pass that
    /// holds the instant: the speed at the NEAREST boundary of a 256-frame
    /// pass is up to half a pass off the instant, 0.0064 of speed at
    /// 44.1 kHz in the linear phase, so it cannot be held to 0.004 and pins
    /// nothing about the law. The literals keep the law's own identities
    /// (the lock at 90 % of the climb, the slope continuous there), the
    /// speed is continuous from pass to pass and only rises, and at equal
    /// seconds the three rates play the same speed, because a start is a
    /// time and not a count of frames.
    #[test]
    fn a_start_reaches_speed_in_its_time_at_every_rate() {
        assert!((KAPPA * U_C - (1.0 - E0)).abs() < 1e-12, "the lock is not at 90 % of the climb");
        assert!((A - 2.0 * KAPPA / E0).abs() < 1e-12, "the capture's slope does not meet the linear phase's");
        assert!((KAPPA - ((1.0 - E0) + Y * E0)).abs() < 1e-12, "KAPPA is not the root's");
        assert!((phi(1.0) - (1.0 - CAPTURE_EPS)).abs() < 1e-12, "the start does not end at the capture criterion: {}", phi(1.0));
        for rate in RATES {
            let i = inp(rate, 256);
            let mut m = parked(&i);
            let (ramp, running) = ramp_of(&mut m, &i, true, MotorShow::SpinningUp, MotorShow::Running);
            let len = (rate * 0.5) as u64;
            assert!(
                running >= len && running - len < 256,
                "at {rate} Hz the start was at speed {running} frames in, not at the first boundary at or after {len}"
            );
            assert_eq!(ramp[0].1, FLOOR, "the start did not begin at the floor");
            for w in ramp.windows(2) {
                assert_eq!(w[0].2.to_bits(), w[1].1.to_bits(), "at {rate} Hz the speed jumped between passes");
            }
            for &(at, s0, s1) in &ramp {
                assert!(s1 > s0 && s1 < 1.0, "at {rate} Hz the pass at {at} went from {s0} to {s1}");
            }
            for (u, want) in [(0.25, 0.31089), (0.5, 0.60605), (0.75, 0.90121), (0.9, 0.99242)] {
                let s = played(&ramp, 256, u * len as f64);
                assert!((s - want).abs() < 0.004, "at {rate} Hz and u = {u} the tape plays at {s:.5}, not {want}");
            }
        }
        // 10 ms passes put every rate's boundaries on the same instants.
        let curve = |rate: f64| {
            let i = inp(rate, (rate / 100.0) as u64);
            let mut m = parked(&i);
            let (ramp, running) = ramp_of(&mut m, &i, true, MotorShow::SpinningUp, MotorShow::Running);
            (ramp.iter().map(|r| r.2).collect::<Vec<f64>>(), running as f64 / rate)
        };
        let (base, took) = curve(48_000.0);
        assert_eq!(base.len(), 50, "a half-second start is not fifty 10 ms passes");
        for rate in [44_100.0, 96_000.0] {
            let (other, t) = curve(rate);
            assert_eq!(other.len(), base.len(), "at {rate} Hz the start took a different number of passes");
            assert_eq!(t, took);
            for (k, (a, b)) in base.iter().zip(&other).enumerate() {
                assert!((a - b).abs() < 1e-9, "at {rate} Hz pass {k} plays {b} where 48 kHz plays {a}");
            }
        }
    }

    /// **A stop falls by the brakes' law and ends at the floor.**
    ///
    /// Dynamic braking proportional to speed plus a constant drag, `dv/dt =
    /// -(b + v / tau)`: from full speed at 0.7 s the tape plays 0.59877,
    /// 0.32633, 0.14134 and 0.06023 of speed at u = 0.25, 0.5, 0.75 and 0.9
    /// within 0.004 (read as it plays, as the start is), it reaches the
    /// floor exactly, and the motor holds at the first pass boundary at or
    /// after 30870 / 33600 / 67200 frames. `LAMBDA`'s literal is the law's,
    /// the speed is continuous from pass to pass and only falls.
    #[test]
    fn a_stop_falls_by_the_brakes_law_and_ends_at_the_floor() {
        assert!((((1.0 + FRICTION) / (FLOOR + FRICTION)).ln() - LAMBDA).abs() < 1e-12, "LAMBDA is not the law's");
        for rate in RATES {
            let i = inp(rate, 256);
            let mut m = running(&i);
            let (ramp, held) = ramp_of(&mut m, &i, false, MotorShow::WindingDown, MotorShow::Holding);
            let len = (rate * 0.7).round() as u64;
            assert!(
                held >= len && held - len < 256,
                "at {rate} Hz the stop held {held} frames in, not at the first boundary at or after {len}"
            );
            assert_eq!(ramp[0].1, 1.0, "the stop did not begin at full speed");
            assert_eq!(ramp.last().expect("a stop").2, FLOOR, "the stop did not end at the floor");
            for w in ramp.windows(2) {
                assert_eq!(w[0].2.to_bits(), w[1].1.to_bits(), "at {rate} Hz the speed jumped between passes");
            }
            for &(at, s0, s1) in &ramp {
                assert!(s1 <= s0 && s1 >= FLOOR, "at {rate} Hz the pass at {at} went from {s0} to {s1}");
            }
            for (u, want) in [(0.25, 0.59877), (0.5, 0.32633), (0.75, 0.14134), (0.9, 0.06023)] {
                let s = played(&ramp, 256, u * len as f64);
                assert!((s - want).abs() < 0.004, "at {rate} Hz and u = {u} the tape plays at {s:.5}, not {want}");
            }
        }
    }

    /// **A half stop and a re-spin keep the machine's law.**
    ///
    /// A stop pressed in a start winds down from the speed it had, and a
    /// Play in a Pause's coast spins up from where the tape is: neither is
    /// a full ramp, and neither may take a full ramp's time. The brakes
    /// take a stop from 0.6 to the floor in 0.75094 of a full stop, and the
    /// servo, at the same torque, a re-spin from 0.4 in 0.60958 of a full
    /// start, each within one pass; and that torque is the same: the linear
    /// phase of the re-spin rises as fast as a start from the floor's.
    #[test]
    fn a_half_stop_and_a_re_spin_keep_the_machines_law() {
        let rate = 48_000.0;
        let i = inp(rate, 256);
        let mut m = parked(&i);
        m.wind_down_from(0.6, i.stop_s);
        let (_, held) = ramp_of(&mut m, &i, false, MotorShow::WindingDown, MotorShow::Holding);
        let want = 0.75094 * i.stop_s * rate;
        assert!((held as f64 - want).abs() <= 256.0, "a stop from 0.6 took {held} frames, not {want:.0}");
        let mut m = running(&i);
        m.spin_up_from(0.4, i.start_s);
        let (ramp, at_speed) = ramp_of(&mut m, &i, true, MotorShow::SpinningUp, MotorShow::Running);
        let want = 0.60958 * i.start_s * rate;
        assert!((at_speed as f64 - want).abs() <= 256.0, "a re-spin from 0.4 took {at_speed} frames, not {want:.0}");
        let part = (ramp[1].2 - ramp[1].1) / 256.0;
        let mut m = running(&i);
        m.spin_up_from(FLOOR, i.start_s);
        let (full_ramp, _) = ramp_of(&mut m, &i, true, MotorShow::SpinningUp, MotorShow::Running);
        let full = (full_ramp[1].2 - full_ramp[1].1) / 256.0;
        assert!((part / full - 1.0).abs() < 1e-3, "a re-spin accelerates at {part:e} a frame, a full start at {full:e}");
    }

    /// The reproducer's response at `hz`, in dB, from its coefficients as
    /// they run (f32), at `rate`.
    fn response_db(c: ToneCoefs, hz: f64, rate: f64) -> f64 {
        let w = std::f64::consts::TAU * hz / rate;
        let (re, im) = (w.cos(), -w.sin());
        let section = |s: Section| {
            let (b0, b1, a1) = (f64::from(s.b0), f64::from(s.b1), f64::from(s.a1));
            let num = (b0 + b1 * re).hypot(b1 * im);
            let den = (1.0 + a1 * re).hypot(a1 * im);
            num / den
        };
        20.0 * (section(c.bass) * section(c.treble)).log10()
    }

    /// **The floor is where the reproducer's middle closes.**
    ///
    /// At `k = T2 / T1` the bass shelf's rising corner and the treble
    /// shelf's falling one meet, and nothing of the middle is left above
    /// the two: the largest gain anywhere from 1 Hz to 20 kHz is -6.02 dB
    /// (near 50 Hz), within 0.1, at every rate. Below that speed the motor
    /// has nothing left to play, which is why the wind-down ends there.
    #[test]
    fn the_floor_is_where_the_reproducers_middle_closes() {
        assert_eq!(FLOOR, T2_S / T1_S);
        assert!((FLOOR.log2() + 5.99).abs() < 0.01, "the floor is not six octaves down: {}", FLOOR.log2());
        for rate in RATES {
            let c = ToneCoefs::at(FLOOR, rate);
            let (most, at) = (0..=4301)
                .map(|e| 10f64.powf(f64::from(e) / 1000.0))
                .map(|hz| (response_db(c, hz, rate), hz))
                .fold((f64::MIN, 0.0), |a, b| if b.0 > a.0 { b } else { a });
            assert!((most + 6.02).abs() < 0.1, "at {rate} Hz the floor's loudest is {most:.3} dB, at {at:.1} Hz");
            assert!((at - 50.0).abs() < 5.0, "at {rate} Hz the floor's loudest is at {at:.1} Hz");
        }
    }

    /// A steady sine at `hz` through the reproducer at speed `k`, as it
    /// runs: the gain over the last half of a second, in dB.
    fn sine_db(k: f64, hz: f64, rate: f64) -> f64 {
        let c = ToneCoefs::at(k, rate);
        let mut tone = Tone::new(1);
        let n = rate as usize;
        let (mut sx, mut sy) = (0.0_f64, 0.0_f64);
        for j in 0..n {
            let x = (std::f64::consts::TAU * hz * j as f64 / rate).sin() as f32;
            let mut f = [x];
            tone.frame(&mut f, c);
            if j >= n / 2 {
                sx += f64::from(x) * f64::from(x);
                sy += f64::from(f[0]) * f64::from(f[0]);
            }
        }
        10.0 * (sy / sx).log10()
    }

    /// **The reproducer holds the middle and dulls the top**, as the analog
    /// curve does.
    ///
    /// Steady sines through the Direct Form I as it runs, f32, at a fixed
    /// speed, against the analog transfer `H_k`: at half speed 500 Hz is
    /// -0.34 dB and 5 kHz -4.96; at a quarter 25 Hz has sunk 6.03 dB and
    /// 250 Hz only 0.54; at a tenth 1 kHz is -9.96. Within 0.25 dB at 44.1,
    /// 48 and 96 kHz: the prewarped bilinear design is within 0.15 dB of
    /// the analog curve at all three (5 kHz at half speed is the worst, the
    /// warp of the top shelf at 44.1 kHz).
    #[test]
    fn the_reproducer_holds_the_middle_and_dulls_the_top() {
        let pins = [(0.5, 500.0, -0.34), (0.5, 5_000.0, -4.96), (0.25, 25.0, -6.03), (0.25, 250.0, -0.54), (0.1, 1_000.0, -9.96)];
        for rate in RATES {
            for (k, hz, want) in pins {
                let got = sine_db(k, hz, rate);
                assert!((got - want).abs() < 0.25, "at {rate} Hz and k = {k}, {hz} Hz came out {got:.3} dB, not {want}");
            }
        }
    }

    /// Noise in `[-1, 1)` with no zeros of either sign, from a seed.
    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed.max(1);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 0.999_999
            })
            .collect()
    }

    /// **At full speed the reproducer is the identity, bit for bit.**
    ///
    /// The reproducer runs on at full speed for `SETTLE_S` after every ramp
    /// and at a steady speed under a snap, so at `k = 1` it must pass the
    /// desk untouched, or every stop leaves a 50 ms smear on what follows.
    /// The design gives both shelves `b0 == 1.0` and `b1 == a1` bit for bit
    /// at every rate, and the pinned Direct Form I, `b0 x + (b1 x1 - a1 y1)`,
    /// cancels the feedback exactly when the state is settled: 4096 samples
    /// of noise come out the same bits from a zero state, from a settled one
    /// and through coefficients interpolated between two full-speed ends,
    /// and through a stage that applies them. The form is read from the
    /// source, because `(b0 x + b1 x1) - a1 y1` computes the same filter and
    /// does not cancel.
    #[test]
    fn at_full_speed_the_reproducer_is_the_identity_bit_for_bit() {
        let x = noise(4096, 0x5eed);
        for rate in RATES {
            let c = ToneCoefs::at(1.0, rate);
            for s in [c.bass, c.treble] {
                assert_eq!(s.b0.to_bits(), 1.0f32.to_bits(), "at {rate} Hz b0 is {}", s.b0);
                assert_eq!(s.b1.to_bits(), s.a1.to_bits(), "at {rate} Hz b1 {} is not a1 {}", s.b1, s.a1);
            }
            let same = |y: &[f32]| y.iter().zip(&x).all(|(a, b)| a.to_bits() == b.to_bits());
            let mut tone = Tone::new(1);
            let mut y = x.clone();
            for f in y.chunks_exact_mut(1) {
                tone.frame(f, c);
            }
            assert!(same(&y), "at {rate} Hz the zero state changed the noise");
            let mut tone = Tone::new(1);
            tone.state[0] = [0.25; 4];
            let mut y = x.clone();
            for f in y.chunks_exact_mut(1) {
                tone.frame(f, c);
            }
            assert!(same(&y), "at {rate} Hz a settled state changed the noise");
            let mut tone = Tone::new(1);
            let mut y = x.clone();
            for (j, f) in y.chunks_exact_mut(1).enumerate() {
                tone.frame(f, c.lerp(c, (j + 1) as f32 / 4096.0));
            }
            assert!(same(&y), "at {rate} Hz coefficients between two full-speed ends changed the noise");
            let st = Stage { frames: 4096, bypass: false, tone: Some((c, c)), tone_clear: true, f0: 1.0, f1: 1.0, env: Env::settled(1.0) };
            let mut tone = Tone::new(1);
            tone.state[0] = [0.5, -0.5, 0.25, 0.75];
            let mut y = x.clone();
            st.apply(&mut tone, &mut y, 1);
            assert!(same(&y), "at {rate} Hz a full-speed stage changed the noise");
        }
        let src = include_str!("motor.rs");
        let shipped = &src[..src.find("\n#[cfg(test)]\nmod tests").expect("the tests module")];
        assert!(shipped.contains("b0 * x + (b1 * x1 - a1 * y1)"), "the Direct Form I is no longer the pinned form");
        assert!(!shipped.contains("b0 * x + b1 * x1"), "the Direct Form I sums the input into the history first");
    }

    /// **The fade closes the last two octaves and nothing above.**
    ///
    /// Above the knee every gain of a start or a stop is exactly 1, so the
    /// fade cannot dull a pass the motor is merely passing through; it is a
    /// half one octave above the floor and nothing at it, where the read
    /// speed's subsonic tail and DC hold would otherwise be heard.
    #[test]
    fn the_fade_closes_the_last_two_octaves_and_nothing_above() {
        assert_eq!(fade(1.0), 1.0);
        assert_eq!(fade(KNEE), 1.0);
        assert!((fade(2.0 * FLOOR) - 0.5).abs() < 1e-9, "one octave up the fade is {}", fade(2.0 * FLOOR));
        assert_eq!(fade(FLOOR), 0.0);
        assert_eq!(fade(f64::NAN), 0.0, "a speed that is not one is not heard");
        // And it is one curve: no step anywhere, the knee included, where a
        // knee that is not two octaves up would leave the fade short of 1.
        let mut last = 0.0;
        for k in 0..=1000 {
            let s = FLOOR + (KNEE - FLOOR) * f64::from(k) / 1000.0;
            let f = fade(s);
            assert!(f >= last && (0.0..=1.0).contains(&f), "the fade fell or left [0, 1] at {s}");
            assert!(f - last < 0.01, "the fade stepped by {} at {s}", f - last);
            last = f;
        }
    }

    /// **Nothing the settings can reach makes the motor explode**, and every
    /// road through its state machine goes where it says.
    ///
    /// The times and the rate arrive from the host and from a device, so
    /// NaN, both infinities, zero, negatives and a billion are fed for each,
    /// over passes from none to a trillion frames, through a script of
    /// presses, refusals, counts, cuts and seeks: every speed stays in
    /// `[FLOOR, 1]`, every gain in `[0, 1]`, nothing is ever NaN, and what
    /// it publishes reads back as itself. Then each transition of the
    /// machine, by the real road: the soft edges from each mode, the ramps
    /// that hand over, the cut, the snaps, and the two edges a chunk applies
    /// at once.
    #[test]
    fn nothing_the_settings_can_reach_makes_the_motor_explode() {
        let nasty = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e9];
        let rates = [f64::NAN, f64::INFINITY, 0.0, -48_000.0, 1e9, 1.0, 48_000.0];
        let times = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e9, 0.1];
        for &rate in &rates {
            for &time in &times {
                for frames in [0_u64, 1, 256, 8192, 1 << 40] {
                    let mut m = Motor::new();
                    let mut tone = Tone::new(2);
                    for k in 0..96_u32 {
                        let i = PlanIn {
                            frames,
                            rate,
                            ok: k % 23 != 7,
                            count: k % 31 == 30,
                            cut: k / 11,
                            start_s: time,
                            stop_s: nasty[(k as usize) % nasty.len()].max(time),
                        };
                        let p = m.plan(&i);
                        for s in [p.s0, p.s1] {
                            assert!(s.is_finite() && (FLOOR..=1.0).contains(&s), "rate {rate}, time {time}: speed {s}");
                        }
                        if p.steady {
                            assert_eq!(m.gain_now(), 1.0, "rate {rate}, time {time}: a steady pass is not at full gain");
                        }
                        assert_eq!(MotorShow::from_u8(p.show as u8), p.show);
                        let level = (k / 6) % 2 == 1;
                        let c = m.chunk(level, i.ok, k % 3 == 0);
                        assert!(!(c.hold_start && c.coast), "a chunk both held the start and coasted");
                        if !c.hold_seek {
                            m.seek_applied();
                        }
                        let st = m.stage(frames as usize, k % 17 == 0, k % 19 == 0, if k % 5 == 0 { f32::NAN } else { 0.5 });
                        assert_eq!(st.bypass, p.steady, "a steady pass was staged, or a moving one bypassed");
                        let n = (frames as usize).min(64);
                        for j in [0, n / 2, n.saturating_sub(1)] {
                            let g = st.gain(j);
                            assert!(g.is_finite() && (0.0..=1.0).contains(&g), "rate {rate}, time {time}: gain {g}");
                        }
                        let mut buf = vec![0.5f32; 128];
                        st.apply(&mut tone, &mut buf, 2);
                        assert!(buf.iter().all(|v| v.is_finite() && v.abs() <= 4.0), "rate {rate}, time {time}: the stage put out {buf:?}");
                        let g = m.gain_now();
                        assert!(g.is_finite() && (0.0..=1.0).contains(&g), "rate {rate}, time {time}: gain now {g}");
                    }
                }
            }
        }
        for v in 0..=u8::MAX {
            let show = MotorShow::from_u8(v);
            if v <= 6 {
                assert_eq!(show as u8, v, "show {v} does not read back");
            } else {
                assert_eq!(show, MotorShow::Parked, "byte {v} is a show");
            }
        }

        // **The soft edges**, from each mode, by the real road.
        let i = inp(48_000.0, 256);
        let mut m = holding(&i);
        let (p, ..) = step(&mut m, &i, true, false);
        assert_eq!(p.show, MotorShow::Holding, "the press pass held");
        let (p, _, st) = step(&mut m, &i, true, false);
        assert_eq!((p.show, p.s0), (MotorShow::SpinningUp, FLOOR), "a Play from a hold spins up from the floor");
        assert_eq!(st.f0, 0.0, "and starts at silence");
        let mut m = parked(&i);
        step(&mut m, &i, true, false);
        assert_eq!(pass(&mut m, &i, true).show, MotorShow::Dipping, "a Play from a heard desk dips it first");
        let mut m = winding(&i, false);
        let was = m.speed;
        step(&mut m, &i, true, false);
        let p = pass(&mut m, &i, true);
        assert_eq!(p.show, MotorShow::SpinningUp, "a Play after Pause spins up in place");
        assert!(p.s0 < 1.0 && p.s0 <= was, "a Play after Pause spun up from {} where the tape was at {was}", p.s0);
        let mut m = winding(&i, true);
        step(&mut m, &i, true, false);
        assert_eq!(pass(&mut m, &i, true).show, MotorShow::Cutting, "a Play after Stop cuts first");
        assert_eq!(m.mode, Mode::Cut(After::Start));
        let p = until(&mut m, &i, true, MotorShow::SpinningUp);
        assert_eq!(p.s0, FLOOR, "after the cut the start is from the floor");
        // A cut is 480 frames: 64-frame passes leave room to turn it twice.
        let small = inp(48_000.0, 64);
        let mut m = winding(&small, false);
        let cut = PlanIn { cut: 1, ..small };
        assert_eq!(pass(&mut m, &cut, false).show, MotorShow::Cutting, "a cut during a coast cuts");
        assert_eq!(m.mode, Mode::Cut(After::Park));
        step(&mut m, &cut, true, false);
        pass(&mut m, &cut, true);
        assert_eq!(m.mode, Mode::Cut(After::Start), "a Play in a cut to a park makes it a cut to a start");
        step(&mut m, &cut, false, false);
        pass(&mut m, &cut, false);
        assert_eq!(m.mode, Mode::Cut(After::Park), "a Stop in a cut to a start makes it a cut to a park");
        let p = until(&mut m, &cut, false, MotorShow::Holding);
        assert_eq!((p.s0, p.s1), (1.0, 1.0), "a cut to a park holds at rest");
        let mut m = running(&i);
        step(&mut m, &i, false, false);
        let p = pass(&mut m, &i, false);
        assert_eq!((p.show, p.s0), (MotorShow::WindingDown, 1.0), "a Stop at speed winds down from 1");
        let mut m = spinning(&i);
        pass(&mut m, &i, true);
        let was = m.speed;
        step(&mut m, &i, false, false);
        let was_end = m.speed;
        let p = pass(&mut m, &i, false);
        assert_eq!(p.show, MotorShow::WindingDown, "a Stop in a start winds down");
        assert!(was < was_end && p.s0 == was_end, "a Stop in a start wound down from {} where the tape was at {was_end}", p.s0);
        // **The ramps that hand over**, the dip at silence, a start's end
        // and a cut's to a start, each above; a stop's end in `holding`.
        let mut m = parked(&i);
        step(&mut m, &i, true, false);
        until(&mut m, &i, true, MotorShow::Dipping);
        assert_eq!(until(&mut m, &i, true, MotorShow::SpinningUp).s0, FLOOR, "a dip hands over at the floor");
        // **The snaps**: refused in every mode that moves, the steady state.
        let no = PlanIn { ok: false, ..i };
        for (what, mut m, level) in [
            ("a dip", { let mut m = parked(&i); step(&mut m, &i, true, false); pass(&mut m, &i, true); m }, true),
            ("a start", spinning(&i), true),
            ("a stop", winding(&i, true), false),
            ("a cut", { let mut m = winding(&i, true); step(&mut m, &i, true, false); pass(&mut m, &i, true); m }, true),
            ("a hold", holding(&i), false),
        ] {
            let p = pass(&mut m, &no, level);
            let want = if level { MotorShow::Running } else { MotorShow::Parked };
            assert_eq!((p.show, p.s0, p.s1), (want, 1.0, 1.0), "refused in {what}");
            assert!(m.idle(), "refused in {what}, the motor is not idle");
        }
        // **The two edges a chunk applies at once**: a Stop in a dip, and
        // any edge at rest or at speed while not allowed.
        let mut m = parked(&i);
        step(&mut m, &i, true, false);
        pass(&mut m, &i, true);
        m.plan(&i);
        assert_eq!(m.chunk(false, true, true), Chunk::default(), "a Stop in a dip waited");
        assert_eq!(m.mode, Mode::Parked);
        let mut m = running(&i);
        m.plan(&no);
        assert_eq!(m.chunk(false, false, true), Chunk::default(), "a refused Stop at speed waited");
        assert_eq!(m.mode, Mode::Parked);
        assert_eq!(m.chunk(true, false, false), Chunk::default(), "a refused Play at rest waited");
        assert_eq!(m.mode, Mode::Running);
    }

    /// **The pass that meets Stop coasts on and holds its return.**
    ///
    /// A pass's speed is fixed (the resampler's two ratios are), so the
    /// chunk that first reads the level down, and every chunk after it in
    /// that pass, coasts at the planned speed: Stop's return to the cursor
    /// is held, nothing is acked or flushed, and the tape is not left
    /// standing still for the rest of the pass. The wind-down starts at the
    /// next pass, from 1. The same from a start (it winds down from where
    /// the start had got to), and with the motor not allowed while it
    /// ramps: the pass coasts and holds, the next one snaps to rest, and
    /// the held return applies in its first chunk.
    #[test]
    fn the_pass_that_meets_stop_coasts_on_and_holds_its_return() {
        let i = inp(48_000.0, 256);
        let coast = Chunk { hold_start: false, coast: true, hold_seek: true };
        let mut m = running(&i);
        m.plan(&i);
        assert_eq!(m.chunk(true, true, false), Chunk::default(), "the pass rolled before Stop");
        assert_eq!(m.chunk(false, true, true), coast, "the chunk that met Stop did not coast and hold");
        assert_eq!(m.chunk(false, true, true), coast, "a later chunk of that pass did not coast and hold");
        assert!(m.seek_waiting && !m.idle());
        m.stage(256, false, false, 0.0);
        let p = m.plan(&i);
        assert_eq!((p.show, p.s0), (MotorShow::WindingDown, 1.0), "the wind-down did not start next pass, from 1");
        assert!(p.show.coasting() && !p.show.settled(false));
        loop {
            let c = m.chunk(false, true, true);
            if m.mode != Mode::WindDown {
                break;
            }
            assert_eq!(c, coast, "the coast let the return through");
            m.stage(256, false, false, 0.0);
            if m.plan(&i).show == MotorShow::Holding {
                break;
            }
        }
        assert_eq!(m.chunk(false, true, true), Chunk::default(), "the return did not apply when the tape stopped");
        m.seek_applied();
        assert!(!m.seek_waiting && m.last().show.settled(false));

        let mut m = spinning(&i);
        until(&mut m, &i, true, MotorShow::SpinningUp);
        while m.speed < 0.4 {
            pass(&mut m, &i, true);
        }
        m.plan(&i);
        let was = m.speed;
        assert_eq!(m.chunk(false, true, true), coast, "the chunk that met Stop in a start did not coast");
        m.stage(256, false, false, 0.0);
        let p = m.plan(&i);
        assert_eq!(p.show, MotorShow::WindingDown);
        assert_eq!(p.s0.to_bits(), was.to_bits(), "a Stop in a start wound down from {} where the tape was at {was}", p.s0);

        let mut m = spinning(&i);
        m.plan(&i);
        assert_eq!(m.chunk(false, false, true), coast, "a refused Stop in a start did not coast and hold");
        assert_eq!(m.chunk(false, false, true), coast);
        m.stage(256, false, false, 0.0);
        let p = m.plan(&PlanIn { ok: false, ..i });
        assert_eq!(p.show, MotorShow::Parked, "the refused Stop did not snap to rest");
        assert_eq!(m.chunk(false, false, true), Chunk::default(), "the held return did not apply in the next pass's first chunk");
    }

    /// **A cut asked outside a coast is forgotten.**
    ///
    /// Every ruler click while the song plays asks the engine to cut a
    /// coast, and most of them meet none. The cut generation is latched
    /// every pass, in every mode, so one asked at speed or at rest can never
    /// fire on the next stop: that stop winds down the whole way. One asked
    /// during the coast does cut it.
    #[test]
    fn a_cut_asked_outside_a_coast_is_forgotten() {
        let mut i = inp(48_000.0, 256);
        for level in [true, false] {
            let mut m = adopted(&i, level);
            i.cut += 1;
            pass(&mut m, &i, level);
            if !level {
                until(&mut m, &i, true, MotorShow::Running);
            }
            let mut shows = Vec::new();
            loop {
                let p = pass(&mut m, &i, false);
                shows.push(p.show);
                if p.show == MotorShow::Holding {
                    break;
                }
            }
            assert!(shows.contains(&MotorShow::WindingDown), "no wind-down");
            assert!(!shows.contains(&MotorShow::Cutting), "a cut asked while {} fired on the next stop", if level { "playing" } else { "parked" });
        }
        let mut m = winding(&i, false);
        i.cut += 1;
        assert_eq!(pass(&mut m, &i, false).show, MotorShow::Cutting, "a cut during the coast did not cut it");
    }

    /// **A count snaps the tape where it is and jumps the gain.**
    ///
    /// A count-in's first click starts in the very frame the count does,
    /// so the motor goes to its steady state in that pass, from a stop's
    /// coast, from a hold and from a start alike, and the gain is 1 at the
    /// pass's first frame, the jump under the click. From a hold whose
    /// reproducer has settled, that pass is steady at once and its stage
    /// never runs, and the motor still says it is at full gain: that is
    /// the master meter's scale. Refused instead (an
    /// export, a record pressed mid-ramp), the gain never steps: it glides
    /// from where it was to 1 over exactly the house declick,
    /// `round(CUT_S * rate)` frames, every frame's change within the
    /// smoothstep's own steepest, and stays on its way through the refusals
    /// that follow (a refused motor snaps every pass).
    #[test]
    fn a_count_snaps_the_tape_where_it_is_and_jumps_the_gain() {
        let i = inp(48_000.0, 256);
        let cut = (CUT_S * 48_000.0).round() as usize;
        let starts: [(&str, fn(&PlanIn) -> Motor, bool); 4] = [
            ("a stop's coast", deep_in_a_stop, false),
            ("a hold", holding, false),
            ("a hold past its settle", settled_hold, false),
            ("a start", spinning, true),
        ];
        for (what, build, level) in starts {
            let mut m = build(&i);
            assert!(m.gain_now() < 1.0, "{what} was already at full gain");
            let p = m.plan(&PlanIn { count: true, ..i });
            m.chunk(level, true, false);
            let st = m.stage(256, false, false, 0.0);
            let want = if level { MotorShow::Running } else { MotorShow::Parked };
            assert_eq!((p.show, p.s0, p.s1), (want, 1.0, 1.0), "a count in {what}");
            assert_eq!(st.gain(0), 1.0, "under a count from {what} the gain is not 1 at its first frame");
            assert_eq!(m.gain_now(), 1.0, "after a count from {what} the motor does not say it is at full gain");

            let mut m = build(&i);
            let from = m.gain_now();
            let no = PlanIn { ok: false, ..i };
            let mut all = Vec::new();
            for _ in 0..3 {
                let p = m.plan(&no);
                assert_eq!(p.show, want, "refused in {what}");
                m.chunk(level, false, false);
                all.extend(gains(&m.stage(256, false, false, 0.0), 256));
            }
            assert_eq!(all[cut - 1], 1.0, "refused in {what}, the glide had not landed at frame {cut}");
            assert!(all[cut - 2] < 1.0, "refused in {what}, the glide landed early");
            let steepest = 1.5 * (1.0 - from) / cut as f64 + 1e-12;
            let mut last = from;
            for (j, g) in all.iter().enumerate() {
                assert!((g - last).abs() <= steepest, "refused in {what}, frame {j} stepped {:.6}", g - last);
                last = *g;
            }
            let rests = (0..32).any(|_| {
                let p = m.plan(&no);
                m.chunk(level, false, false);
                m.stage(256, false, false, 0.0);
                p.steady
            });
            assert!(rests, "refused in {what}, the motor never came to rest");
        }
    }

    /// One hold driven by a desk whose pass-loudest pre-gain sample at
    /// `t` seconds into the hold is `level(t)`, at 48 kHz in 256-frame
    /// passes: when it let go (seconds into the hold, at the end of the
    /// pass that did), and when the hold's own envelope first came within
    /// `HOLD_FALL` of `floor`.
    fn hold_against(level: impl Fn(f64) -> f64, floor: f32) -> (f64, Option<f64>) {
        let i = inp(48_000.0, 256);
        // The hold's first pass, silent, is behind it already.
        let mut m = holding(&i);
        let mut t = 256.0 / 48_000.0;
        let mut arrived = None;
        for _ in 0..10_000 {
            let p = m.plan(&i);
            assert_eq!(p.show, MotorShow::Holding);
            m.chunk(false, true, false);
            let peak = (0..256).step_by(8).map(|j| level(t + f64::from(j) / 48_000.0)).fold(0.0, f64::max) as f32;
            let st = m.stage(256, false, false, peak);
            t += 256.0 / 48_000.0;
            match m.hold {
                Some(h) => {
                    assert_eq!(st.gain(255), 0.0, "the hold let sound through at {t:.3} s");
                    if arrived.is_none() && h.filled >= HOLD_SPAN && h.envelope(0) * HOLD_FALL <= floor {
                        arrived = Some(t);
                    }
                }
                None => return (t, arrived),
            }
        }
        panic!("the hold never let go");
    }

    /// **The hold lets go at once for a player.**
    ///
    /// A note played, a pad struck, an input the room hears, a preview:
    /// whoever plays into a parked desk is never held silent behind the last
    /// stop's tails. The gain glides up from the pass's first frame and is
    /// at 1 within `CUT_S` of it, at every rate.
    #[test]
    fn the_hold_lets_go_at_once_for_a_player() {
        for rate in RATES {
            let i = inp(rate, 256);
            let mut m = holding(&i);
            m.plan(&i);
            m.chunk(false, true, false);
            let st = m.stage(256, true, false, 0.5);
            assert!(!m.holding(), "at {rate} Hz a player did not release the hold");
            let mut all = gains(&st, 256);
            for _ in 0..4 {
                all.extend(gains(&pass_stage(&mut m, &i), 256));
            }
            let cut = (CUT_S * rate).round() as usize;
            assert!(all[0] > 0.0, "at {rate} Hz the release did not begin at the pass's first frame");
            assert_eq!(all[cut - 1], 1.0, "at {rate} Hz the gain was not 1 within CUT_S");
        }
    }

    /// A pass's stage, parked and silent.
    fn pass_stage(m: &mut Motor, i: &PlanIn) -> Stage {
        step(m, i, false, false).2
    }

    /// **The hold waits out a tail and lets go at the desk's own floor.**
    ///
    /// A plate's decay (RT60 2 s, 30 dB a second) from -10 dBFS onto a
    /// steady -50 dBFS floor, the hiss a tape unit or an amp idles at: held,
    /// silent, while it falls, and let go once what is left has stopped
    /// falling, never before 0.6 s (two spans of windows). The envelope is
    /// the loudest of the last `HOLD_SPAN` windows and is compared with the
    /// span before it, so the release comes within `HOLD_SPAN + 1` windows
    /// of that envelope reaching the floor (within `HOLD_FALL` of it), which
    /// is within `2 * HOLD_SPAN + 1` windows of the tail itself reaching it;
    /// a pin of `HOLD_SPAN + 1` windows from the tail's own arrival cannot
    /// hold for any comparison across a span (measured: 0.57 s).
    #[test]
    fn the_hold_waits_out_a_tail_and_lets_go_at_the_desks_own_floor() {
        let (top, floor) = (10f64.powf(-10.0 / 20.0), 10f64.powf(-50.0 / 20.0));
        let (released, arrived) = hold_against(|t| (top * 10f64.powf(-30.0 * t / 20.0)).max(floor), floor as f32);
        let reached = 40.0 / 30.0;
        let arrived = arrived.expect("the envelope never reached the floor");
        assert!(released >= 2.0 * HOLD_SPAN as f64 * HOLD_WINDOW_S, "let go at {released:.3} s, before two spans");
        assert!(released > reached, "let go at {released:.3} s while the tail still fell to {reached:.3}");
        assert!(released - arrived <= (HOLD_SPAN + 1) as f64 * HOLD_WINDOW_S, "let go {:.3} s after the envelope's floor", released - arrived);
        assert!(released - reached <= (2 * HOLD_SPAN + 1) as f64 * HOLD_WINDOW_S, "let go {:.3} s after the tail's floor", released - reached);
    }

    /// **The hold bridges an echo's gaps.**
    ///
    /// A clean digital delay repeats 0.25 s apart, 3 dB quieter each time,
    /// with digital silence between: the loudest of the last 0.3 s always
    /// holds a repeat, so the hold rides through every gap until the
    /// repeats are under the park floor, and lets go within `HOLD_SPAN + 1`
    /// windows of the last one it could hear. Repeats 0.5 s apart are
    /// further apart than the span and let go in the first gap: the
    /// documented limit, a longer span would hold every desk longer.
    #[test]
    fn the_hold_bridges_an_echos_gaps() {
        let echo = |gap: f64| {
            move |t: f64| {
                let k = (t / gap).floor();
                if t - k * gap < 0.02 { 0.5 * 10f64.powf(-3.0 * k / 20.0) } else { 0.0 }
            }
        };
        let (released, _) = hold_against(echo(0.25), PARK_FLOOR);
        let heard = (0..).find(|k| 0.5 * 10f64.powf(-3.0 * f64::from(*k) / 20.0) < f64::from(PARK_FLOOR)).expect("a quiet one") - 1;
        let last = f64::from(heard) * 0.25 + 0.02;
        assert!(released > last, "let go at {released:.3} s, before the repeat at {last:.3}");
        assert!(released - last <= (HOLD_SPAN + 1) as f64 * HOLD_WINDOW_S, "held {:.3} s past the last repeat it heard", released - last);
        let (released, _) = hold_against(echo(0.5), PARK_FLOOR);
        assert!(released < 0.5, "repeats 0.5 s apart held across the first gap, to {released:.3} s");
    }

    /// **The hold never outlasts the racks' park.**
    ///
    /// A tail that keeps falling, 2 dB every 0.3 s from 0 dBFS (more than
    /// the 1.5 dB a floor is allowed, and still -53 dBFS at eight seconds),
    /// is a decay every window says to wait for. The hold lets go at
    /// `RACK_PARK_S`, where the racks park anyway, within one window.
    #[test]
    fn the_hold_never_outlasts_the_racks_park() {
        let (released, _) = hold_against(|t| 10f64.powf(-(2.0 / 0.3) * t / 20.0), PARK_FLOOR);
        assert!(
            released >= RACK_PARK_S && released <= RACK_PARK_S + HOLD_WINDOW_S,
            "a falling tail let go at {released:.3} s, not at {RACK_PARK_S} s"
        );
    }

    /// **The engine takes the UI's start and stop range**, and the start
    /// law's literals are the root they were written from.
    ///
    /// The reach and the two defaults live in `tangent_ui::motor`, where the
    /// field clamps and the file reads them; the engine restates them to
    /// sanitise what it is pushed (`Engine::set_tape_motor`), as the dial's
    /// `varispeed::MAX_RATIO` restates its `HARD_MAX`. Had the two drifted,
    /// the field would offer a time the tape never takes, or the shipped
    /// setting would be a different machine from the one the engine falls
    /// back to. And the start law's constants are literals with full digits,
    /// so they are pinned to the equation they solve: `Y` is the root of
    /// `(1 + y) e^(-2y) = CAPTURE_EPS / E0` within 1e-12, and the start ends
    /// at `phi(1) = 0.999`, the published "to 0.1 %", within 1e-12.
    #[test]
    fn the_engine_takes_the_uis_start_and_stop_range() {
        assert_eq!(MIN_S, tangent_ui::motor::MIN_S, "the engine's shortest time is not the field's");
        assert_eq!(MAX_S, tangent_ui::motor::MAX_S, "the engine's longest time is not the field's");
        assert_eq!(START_DEFAULT_S, tangent_ui::motor::START_DEFAULT_S, "the shipped start differs");
        assert_eq!(STOP_DEFAULT_S, tangent_ui::motor::STOP_DEFAULT_S, "the shipped stop differs");
        // The shipped setting, as the desk pushes it, is the engine's own.
        let shipped = tangent_ui::motor::StartStop::default();
        assert!(!shipped.on(), "the setting ships on");
        assert_eq!((shipped.start_s(), shipped.stop_s()), (START_DEFAULT_S, STOP_DEFAULT_S));
        let root = (1.0 + Y) * (-2.0 * Y).exp();
        assert!(
            (root - CAPTURE_EPS / E0).abs() < 1e-12,
            "Y is not the capture's root: (1 + Y) e^(-2Y) = {root:e}"
        );
        assert!((phi(1.0) - 0.999).abs() < 1e-12, "the start ends at {}", phi(1.0));
    }

    /// **The motor's silence, tail window and declick are the renderer's.**
    ///
    /// Restated here because the renderer's are its own (`instrument`'s
    /// `PARK_FLOOR`, `RACK_PARK_S` and `GAIN_TAU_SECONDS`): the hold calls a
    /// tail over where the racks call a lane silent, lets go no later than
    /// they park, and every glide is the house's 10 ms. A floor here above
    /// the renderer's would let go of a tail the racks still hear.
    #[test]
    fn the_motor_keeps_the_renderers_silence_tail_window_and_declick() {
        assert_eq!(PARK_FLOOR, crate::instrument::PARK_FLOOR, "the hold's silence is not the racks'");
        assert_eq!(RACK_PARK_S, crate::instrument::RACK_PARK_S, "the hold's cap is not the racks' park");
        assert_eq!(CUT_S, crate::instrument::GAIN_TAU_SECONDS, "the motor's glides are not the house declick");
    }
}
