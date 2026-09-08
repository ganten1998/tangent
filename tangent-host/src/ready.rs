//! The readiness gate: deciding when a hosted instrument will actually make a
//! sound, before anything is allowed to arm.
//!
//! # The measurement this file exists for
//!
//! RECORDER-PLAN §8 "Spike 2", finding 2, on the owner's own machine. A note
//! played immediately after [`Instance::create`] returned:
//!
//! ```text
//! plugin                  cold      after a 5 s warm-up
//! the modelled piano              0.342     -
//! Analog Lab V            0.224     -
//! Augmented GRAND PIANO   0.003     0.167
//! CP-70 V                 0.0007    0.143
//! Piano V3                0.000     0.217
//! Stage-73 V2             0.000     0.198
//! ```
//!
//! **Four of six instruments are silent or near-silent cold and all four are
//! fine five seconds later.** Instantiation is not loading: these plugins hand
//! back a working `IComponent`, accept `setActive`, return `kResultOk` from
//! `process`, and write zeros while a background thread is still reading
//! samples off disk. Nothing in the VST3 ABI reports this — there is no "am I
//! loaded" call — so the only instrument that can measure it is rendering.
//!
//! A recorder that arms on *instantiated* rather than on *ready* produces a
//! silent take from most of that library, and the user reports it as "Tangent
//! doesn't work with my piano".
//!
//! # The policy, and what is dishonest about it
//!
//! Deciding "ready" from "we saw non-silence" is circular: you have to play
//! something to see output, and a plugin may be legitimately silent (a muted
//! channel, a preset with the output gain down, a patch that only responds to
//! a controller). So this gate does not pretend to *know*. It does four things
//! and says which one produced the answer, in [`Evidence`]:
//!
//! 1. **Listen first, with no events at all** ([`Policy::listen_first`]). A
//!    patch that is already making noise needs no probe, and probing it would
//!    put a note into a monitor path for nothing. It also guarantees the
//!    plugin has rendered at least one block before it is ever sent a note.
//! 2. **Probe** with one note, repeatedly, until it sounds. Repeatedly and not
//!    once, because a note-on delivered while the sampler is still loading is
//!    simply lost: the plugin that becomes ready at t=3 s never retro-sounds a
//!    note it was handed at t=0.
//! 3. **A minimum warm-up floor** ([`Policy::floor`]), which is the defence
//!    against the *measured* false positive in the table above. Augmented
//!    GRAND cold reads 0.003 — that is above a -60 dBFS threshold, so a gate
//!    built on a threshold alone would have declared it ready and recorded it
//!    35 dB down. Two independent guards are used: the threshold is set at
//!    -40 dBFS ([`Policy::sound_threshold`]), which 0.003 does not clear, and
//!    the floor holds the gate shut regardless of what was heard.
//! 4. **A timeout, after which the plugin is declared ready anyway**
//!    ([`Policy::timeout`]) with `Evidence::Timeout`. This is the honest part:
//!    an instrument that never sounds is indistinguishable from one that is
//!    still loading, and hanging the UI forever on that ambiguity is worse
//!    than letting the user record and hear the result. `Evidence::Timeout`
//!    means "this is a decision, not an observation", and the UI should say
//!    so.
//!
//! **The limits, stated rather than buried.** This gate cannot tell a plugin
//! that finished loading from one whose first velocity layer happens to be
//! resident; it cannot detect an instrument that needs a program change or a
//! CC before it sounds; and on a `Evidence::Timeout` it is asserting nothing
//! at all. What it *does* guarantee is that the four measured-silent plugins
//! above are not armed cold.
//!
//! # The probe must not reach the take
//!
//! Three structural rules, not conventions:
//!
//! * **No audio leaves this module.** The renderer hands back a peak, a single
//!   `f32`, and never a buffer. There is no path by which a probe sample could
//!   be written to a file, because this code never holds one.
//! * **The probe is always released**, and then drained until the tail is
//!   measurably quiet ([`Policy::quiet_blocks`]) or the drain cap expires.
//!   [`Readiness::stuck_note`] reports a note this gate believes is still
//!   held, and [`Readiness::cancel`] releases it; the state machine cannot do
//!   that from `Drop` because it does not own the renderer.
//! * **Readiness completes before arming.** [`Readiness::may_arm`] is the one
//!   call an integrator needs, and it is false until the probe is released and
//!   the state is [`State::Ready`].
//!
//! The probe defaults to C7 at half velocity. High, because a grand piano's
//! top octave decays in about a second while its bottom octave rings for
//! twenty, and the whole cost of this gate's tail is that decay. Half velocity
//! rather than pianissimo because the measured table was taken at 0.79 and the
//! quiet layers of a sampled piano are the ones most likely to still be
//! streaming.
//!
//! # Rendering is not free, and it is not the same as waiting
//!
//! The plugin's loader runs on **wall** time; this gate runs on **CPU** time.
//! Rendering as fast as the machine allows turns five seconds of audio into a
//! few hundred milliseconds of wall clock and gives the loader thread almost
//! nothing, while burning a core. So [`Policy::max_rate`] paces the loop at
//! realtime by default and [`Readiness::next_block_due`] says when the next
//! block is owed. Every duration in [`Policy`] is wall time for the same
//! reason: a floor expressed in rendered-audio seconds is satisfied instantly
//! on a fast machine, which is precisely backwards.

use std::time::{Duration, Instant};

use crate::instance::{Instance, Note, Setup};

/// Render one block, delivering `notes`, and return the peak absolute sample
/// across the main output bus.
///
/// A scalar and not a buffer, deliberately: see the module docs. A non-finite
/// return (see [`block_peak`]) means the plugin emitted a NaN or an infinity.
pub type RenderBlock<'a> = dyn FnMut(&[Note]) -> Result<f32, String> + 'a;

/// What the UI shows. `Loading` is the only non-terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Still warming up. **Arming here is the bug this module exists to
    /// prevent.**
    Loading,
    /// Safe to arm. Read [`Readiness::evidence`] before believing it made a
    /// sound.
    Ready,
    /// The instrument cannot be used. [`Readiness::reason`] says why, in words
    /// a user can read.
    Failed,
}

/// How [`State::Ready`] was concluded. The difference matters to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// The instrument was already making sound before it was sent anything, so
    /// no probe was ever played.
    Unprompted,
    /// A probe note sounded, was released, and its tail was drained.
    Probe,
    /// Nothing ever sounded and [`Policy::timeout`] expired. **This is a
    /// decision, not an observation**, and the UI should say so: the take may
    /// well be silent.
    Timeout,
}

/// Where the gate is in its sequence. Exposed so a stuck plugin is visibly
/// stuck rather than showing a generic spinner for thirty seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Rendering with no events, watching for a patch that sounds on its own.
    Listening,
    /// Playing and releasing the probe note until something is heard.
    Probing,
    /// Probe released; waiting for its tail to fall below the threshold.
    Draining,
    /// Silent, waiting out the remainder of [`Policy::floor`].
    Settling,
    /// Terminal.
    Done,
}

impl Phase {
    /// Text for the Recorder band. No em dashes: this is user-visible copy.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Listening => "Loading instrument",
            Phase::Probing => "Testing instrument",
            Phase::Draining => "Clearing the test note",
            Phase::Settling => "Warming up",
            Phase::Done => "Ready",
        }
    }
}

/// The detection policy. Every duration is **wall** time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Policy {
    /// How long to render with no events before the first probe.
    ///
    /// Two jobs: catch a patch that is already sounding, and guarantee the
    /// plugin has rendered at least one block before it is handed a note.
    pub listen_first: Duration,

    /// Peak below which a block counts as silence.
    ///
    /// **0.01, which is -40 dBFS, and the number is set by a measurement.**
    /// Augmented GRAND PIANO cold reads 0.003 and is not ready; warm it reads
    /// 0.167. A -60 dBFS threshold (0.001) passes the cold reading and would
    /// have armed it 35 dB down. Every warm reading in the §8 table is 0.14 or
    /// above, so -40 dBFS separates them with 23 dB of margin at full
    /// velocity.
    pub sound_threshold: f32,

    /// Consecutive sounding blocks needed before the gate believes it.
    ///
    /// Several plugins emit a single-block click when their load finishes;
    /// one loud block is a discontinuity, not an instrument.
    pub confirm_blocks: u32,

    /// Consecutive blocks below [`Policy::sound_threshold`] that end the drain.
    pub quiet_blocks: u32,

    /// MIDI pitch of the probe note. C7 by default: high enough that a piano's
    /// decay is about a second rather than twenty, low enough to be inside the
    /// range of every 88-key instrument.
    pub probe_pitch: i16,

    /// 0.0..=1.0. VST3 velocity is a float, not a MIDI byte (instance.rs), and
    /// passing 64 here would make the probe fortissimo and clipped.
    pub probe_velocity: f32,

    /// Release velocity for the probe's note-off. Some pianos map it to damper
    /// noise, so it is neither 0 nor 1.
    pub release_velocity: f32,

    /// How long each probe note is held before it is released.
    pub probe_hold: Duration,

    /// Silence between one probe release and the next note-on.
    pub probe_gap: Duration,

    /// Longest wait for the probe tail to go quiet. A pad with a thirty-second
    /// reverb would otherwise hold the gate shut forever, so the cap wins and
    /// [`Readiness::probe_tail_capped`] says it did.
    pub drain_cap: Duration,

    /// Minimum wall time before [`State::Ready`], whatever was heard.
    ///
    /// **Five seconds, because five seconds is the only number there is
    /// evidence for**: the §8 table says four plugins that were silent cold
    /// were all fine after a 5 s warm-up. Anything shorter is a guess wearing
    /// a smaller number. It is paid when the Recorder view opens, not when
    /// Record is pressed, so in the normal flow the user never waits for it.
    pub floor: Duration,

    /// When a never-sounding instrument is declared ready anyway.
    ///
    /// Fifteen seconds: three times the measured worst case, and a bounded
    /// one-time cost for an instrument that genuinely never sounds. See
    /// [`Evidence::Timeout`].
    pub timeout: Duration,

    /// Ceiling on rendered-audio seconds per wall second.
    ///
    /// 1.0, i.e. realtime. Rendering faster does not make the plugin's loader
    /// thread finish sooner (it runs on wall time) and it costs a core. See
    /// [`Readiness::next_block_due`].
    pub max_rate: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            listen_first: Duration::from_millis(100),
            sound_threshold: 0.01,
            confirm_blocks: 3,
            quiet_blocks: 8,
            probe_pitch: 96,
            probe_velocity: 0.5,
            release_velocity: 0.5,
            probe_hold: Duration::from_millis(400),
            probe_gap: Duration::from_millis(200),
            drain_cap: Duration::from_secs(3),
            floor: Duration::from_secs(5),
            timeout: Duration::from_secs(15),
            max_rate: 1.0,
        }
    }
}

impl Policy {
    /// **The policy a bay LOAD warms up under, which is not the policy an
    /// arming decision needs.**
    ///
    /// [`Policy::floor`] says in its own doc that it "is paid when the
    /// Recorder view opens, not when Record is pressed, so in the normal flow
    /// the user never waits for it". That was true of the instrument SLOT it
    /// was written for: one instrument, warmed once, behind a view somebody
    /// had just asked to open. It stopped being true the moment the racks
    /// began hosting instruments, because a rack is filled ONE BAY PER FRAME
    /// at every project open — so five seconds of deliberate wall clock
    /// became five seconds PER BAY, serially, on the UI thread, in front of
    /// somebody waiting for his desk.
    ///
    /// Measured on the owner's own session, seven bays of one sampled Rhodes
    /// with each bay's saved preset restored, in `--release`, by
    /// `what_a_deskful_of_sampled_instruments_costs_to_open`:
    ///
    /// ```text
    ///                        Policy::default()   Policy::at_load()
    ///   module_for                   346 ms            398 ms
    ///   Instance::create            1.92 s            1.87 s
    ///   load_state                  3.04 s            2.13 s
    ///   warm_up                    35.03 s            1.65 s
    ///   ----------------------------------------------------------
    ///   seven bays                 40.33 s            6.05 s
    /// ```
    ///
    /// **Nothing that is READ was given up.** Every bay still reaches
    /// [`State::Ready`] by [`Evidence::Probe`] on both policies — the
    /// instrument really sounded, and the gate is still a measurement rather
    /// than a stopwatch. What goes is [`Phase::Settling`]: the wait AFTER the
    /// instrument has demonstrated it can sound, which exists so a RECORDER
    /// does not arm on a sampler whose later velocity layers are still
    /// streaming. A bay load arms nothing. The loading path reads exactly one
    /// thing from this gate — [`State::Failed`] — and every road to that
    /// verdict is still open.
    ///
    /// **The floor is shortened rather than removed, and 250 ms is not a
    /// round number for its own sake.** The non-finite guard in
    /// [`Readiness::step`] tolerates garbage until the floor and refuses it
    /// after, on the stated ground that "a plugin rendering garbage while it
    /// initialises is exactly the condition this gate is waiting out". A zero
    /// floor would make the gate STRICTER than it has ever been — the first
    /// NaN of a plugin's first block would refuse the load — which is the
    /// "my piano stopped loading" bug wearing a performance fix's clothes. A
    /// floor a shade longer than the gate's own measured life (237 ms end to
    /// end on the real instrument above) keeps the tolerance window covering
    /// the whole warm-up, exactly as it does today, and costs 21 ms a bay.
    ///
    /// **The timeout comes down for a second measured reason.** An instrument
    /// that never answers the probe waits out the whole of it, and
    /// [`Policy::probe_pitch`] is C7: a bass, a sub, a kick, or any keyboard
    /// whose top note is below it never sounds and never can. Three of seven
    /// bays did exactly that on the first run of the probe above, at 15.1 s
    /// each. One second is comfortably past the 237 ms a loaded sampler took
    /// to answer, and an instrument that has not answered by then is handed
    /// to the rack, where it goes on loading on its own thread precisely as
    /// it would have here.
    ///
    /// **And the probe note does not arrive in the rack behind it**, which
    /// was the one thing the shortened floor could plausibly have broken:
    /// [`Policy::quiet_blocks`] of drain still runs, and the instance the
    /// rack is handed measured -73.8 dBFS on a zero floor and -76.9 dBFS on
    /// this one — thirty-odd dB below the gate's own -40 dBFS idea of
    /// silence, and falling. The drain does that work; the floor never did.
    ///
    /// **[`Policy::default`] is left exactly as it is**, floor and all,
    /// because the §8 evidence it carries is about ARMING and that decision
    /// is still owed: [`Readiness::may_arm`] has no caller yet, and when it
    /// gets one, this is the policy it must not use.
    pub fn at_load() -> Self {
        Self {
            floor: Duration::from_millis(250),
            timeout: Duration::from_secs(1),
            ..Self::default()
        }
    }
}

/// The gate itself: a state machine over one instrument.
///
/// Time is supplied by the caller rather than read here, exactly as
/// `tangent-record`'s `clock.rs` does it, so a fifteen-second timeout is a
/// microsecond unit test and every branch below is reachable without a plugin.
#[derive(Debug, Clone)]
pub struct Readiness {
    setup: Setup,
    policy: Policy,
    state: State,
    phase: Phase,
    evidence: Option<Evidence>,
    reason: Option<String>,
    elapsed: Duration,
    blocks: u64,
    peak: f32,
    nonfinite: u64,
    sounding_run: u32,
    quiet_run: u32,
    probes: u32,
    /// The pitch this gate believes is currently sounding because it sent the
    /// note-on. See the commit ordering in [`Readiness::step`]: it is always
    /// wrong in the direction of "a note may still be down".
    held: Option<i16>,
    probe_deadline: Duration,
    drain_started: Duration,
    tail_capped: bool,
}

impl Readiness {
    pub fn new(setup: Setup, policy: Policy) -> Self {
        Self {
            setup,
            policy,
            state: State::Loading,
            phase: Phase::Listening,
            evidence: None,
            reason: None,
            elapsed: Duration::ZERO,
            blocks: 0,
            peak: 0.0,
            nonfinite: 0,
            sounding_run: 0,
            quiet_run: 0,
            probes: 0,
            held: None,
            probe_deadline: Duration::ZERO,
            drain_started: Duration::ZERO,
            tail_capped: false,
        }
    }

    /// Render exactly one block and advance the machine.
    ///
    /// `elapsed` is wall time since the gate was created. It is clamped to be
    /// monotone: `Instant` is, but a caller that computes elapsed from two
    /// different clocks is not, and a backwards step must stall the gate
    /// rather than rewind the floor and start the warm-up again.
    ///
    /// Once [`State::Ready`] or [`State::Failed`] the call is a no-op that
    /// returns the latched state. A terminal state that can flip back to
    /// `Loading` is an armed recorder that disarms itself mid-take.
    pub fn step(&mut self, elapsed: Duration, render: &mut RenderBlock<'_>) -> State {
        if self.state != State::Loading {
            return self.state;
        }
        self.elapsed = elapsed.max(self.elapsed);

        let event = self.event_for_this_block();
        // A note-on is committed BEFORE the render and a note-off only AFTER a
        // render that succeeded. Both halves lean the same way: if the block
        // errors, this gate believes the note is still down. A spurious extra
        // note-off costs nothing; a missing one is an instrument left sounding
        // into whatever the user records next.
        if let Some(n) = event.filter(|n| n.on) {
            self.held = Some(n.pitch);
            self.probes += 1;
        }
        let notes: &[Note] = match &event {
            Some(n) => std::slice::from_ref(n),
            None => &[],
        };

        let peak = match render(notes) {
            Ok(p) => p,
            Err(why) => {
                self.state = State::Failed;
                self.reason = Some(why);
                return self.state;
            }
        };
        self.blocks += 1;
        if event.is_some_and(|n| !n.on) {
            self.held = None;
        }

        // A NaN is not "quiet" and it is not "loud"; it is a defect. It must be
        // tested for explicitly because `f32::max` RETURNS THE OTHER OPERAND
        // when one side is NaN, so a running peak silently swallows every NaN
        // it ever sees and reports a healthy number.
        let finite = peak.is_finite();
        if finite {
            self.peak = self.peak.max(peak.abs());
        } else {
            self.nonfinite += 1;
            // Tolerated before the floor: a plugin rendering garbage while it
            // initialises is exactly the condition this gate is waiting out.
            // After the floor it is a plugin that will fill the take with
            // non-finite samples, which `wav.rs` writes as silence and other
            // tools render as full-scale noise.
            if self.elapsed >= self.policy.floor {
                self.state = State::Failed;
                self.reason = Some(format!(
                    "the instrument produced a non-finite sample after {:.1} s of warm-up",
                    self.elapsed.as_secs_f32()
                ));
                return self.state;
            }
        }

        if finite && peak.abs() >= self.policy.sound_threshold {
            self.sounding_run += 1;
            self.quiet_run = 0;
        } else {
            self.sounding_run = 0;
            self.quiet_run += 1;
        }

        self.advance_phase();
        self.state
    }

    /// At most one note event per block, by construction, so this allocates
    /// nothing on a loop that runs several hundred times per warm-up.
    fn event_for_this_block(&mut self) -> Option<Note> {
        match self.phase {
            Phase::Probing => {
                if self.elapsed < self.probe_deadline {
                    return None;
                }
                match self.held {
                    Some(pitch) => {
                        self.probe_deadline = self.elapsed + self.policy.probe_gap;
                        Some(Note {
                            offset: 0,
                            pitch,
                            velocity: self.policy.release_velocity,
                            on: false,
                        })
                    }
                    None => {
                        self.probe_deadline = self.elapsed + self.policy.probe_hold;
                        Some(Note {
                            offset: 0,
                            pitch: self.policy.probe_pitch,
                            velocity: self.policy.probe_velocity,
                            on: true,
                        })
                    }
                }
            }
            // The release. Unconditional: whatever else happens, the gate does
            // not leave the probe sounding.
            Phase::Draining => self.held.map(|pitch| Note {
                offset: 0,
                pitch,
                velocity: self.policy.release_velocity,
                on: false,
            }),
            Phase::Listening | Phase::Settling | Phase::Done => None,
        }
    }

    fn advance_phase(&mut self) {
        match self.phase {
            Phase::Listening => {
                if self.sounding_run >= self.policy.confirm_blocks {
                    // It sounds without being asked, so no probe is played at
                    // all and there is nothing to drain: the noise is the
                    // plugin's own patch and silencing it is not this gate's
                    // business.
                    self.evidence = Some(Evidence::Unprompted);
                    self.enter_settling();
                } else if self.elapsed >= self.policy.listen_first {
                    self.phase = Phase::Probing;
                    // Due immediately, so the first probe goes out on the very
                    // next block rather than after a gap.
                    self.probe_deadline = self.elapsed;
                }
            }
            Phase::Probing => {
                if self.sounding_run >= self.policy.confirm_blocks {
                    self.evidence = Some(Evidence::Probe);
                    self.enter_draining();
                } else if self.elapsed >= self.policy.timeout {
                    self.evidence = Some(Evidence::Timeout);
                    self.enter_draining();
                }
            }
            Phase::Draining => {
                if self.held.is_none() && self.quiet_run >= self.policy.quiet_blocks {
                    self.enter_settling();
                } else if self.elapsed.saturating_sub(self.drain_started) >= self.policy.drain_cap {
                    self.tail_capped = true;
                    self.enter_settling();
                }
            }
            Phase::Settling => {
                if self.elapsed >= self.policy.floor {
                    self.phase = Phase::Done;
                    self.state = State::Ready;
                }
            }
            Phase::Done => {}
        }
    }

    fn enter_draining(&mut self) {
        self.phase = Phase::Draining;
        self.drain_started = self.elapsed;
        // Reset rather than inherit: the drain must observe quiet *after* the
        // release, not credit itself with a gap between two probe notes.
        self.quiet_run = 0;
    }

    fn enter_settling(&mut self) {
        self.phase = Phase::Settling;
        if self.elapsed >= self.policy.floor {
            self.phase = Phase::Done;
            self.state = State::Ready;
        }
    }

    /// Release a probe note left held by an abandoned warm-up.
    ///
    /// The state machine cannot do this in `Drop`, because it does not own the
    /// renderer. Any caller that stops stepping a gate that is still
    /// [`State::Loading`] owes this call, and [`Readiness::stuck_note`] says
    /// whether one is owed. The render result is discarded: this is a
    /// best-effort release on a path that is already going wrong, and a plugin
    /// that refuses the block cannot be made to accept it.
    pub fn cancel(&mut self, render: &mut RenderBlock<'_>) {
        let Some(pitch) = self.held.take() else {
            return;
        };
        let off = Note {
            offset: 0,
            pitch,
            velocity: self.policy.release_velocity,
            on: false,
        };
        let _ = render(std::slice::from_ref(&off));
    }

    /// **The one call an integrator needs.** False until the probe is released
    /// and the warm-up is complete.
    pub fn may_arm(&self) -> bool {
        self.state == State::Ready && self.held.is_none()
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// `None` while loading, and `Some(Evidence::Timeout)` means nothing was
    /// ever heard.
    pub fn evidence(&self) -> Option<Evidence> {
        self.evidence
    }

    /// Human-readable failure cause. `None` unless [`State::Failed`].
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Wall time consumed so far. Part of the budget: a gate whose elapsed
    /// keeps climbing while the phase does not move is a stuck plugin.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Blocks rendered and discarded. The other half of the budget.
    pub fn blocks(&self) -> u64 {
        self.blocks
    }

    /// Loudest finite peak seen during the whole warm-up. Worth logging: a
    /// value just under [`Policy::sound_threshold`] is a plugin that is
    /// working and quiet, which reads very differently from an exact zero.
    pub fn peak_seen(&self) -> f32 {
        self.peak
    }

    /// Blocks in which the plugin emitted a NaN or an infinity.
    pub fn nonfinite_blocks(&self) -> u64 {
        self.nonfinite
    }

    /// Probe note-ons played. Zero means either [`Evidence::Unprompted`] or a
    /// gate that has not started probing yet.
    pub fn probes_played(&self) -> u32 {
        self.probes
    }

    /// True when the drain gave up on a tail that had not gone quiet.
    ///
    /// Ready is still Ready, but the caller has been told that a decaying
    /// probe may still be audible for a moment after arming.
    pub fn probe_tail_capped(&self) -> bool {
        self.tail_capped
    }

    /// A note this gate believes is still sounding. See [`Readiness::cancel`].
    pub fn stuck_note(&self) -> Option<i16> {
        self.held
    }

    /// Seconds of audio one `process` call covers.
    pub fn block_seconds(&self) -> f64 {
        let frames = self.setup.max_block.max(1) as f64;
        let rate = if self.setup.sample_rate > 0.0 {
            self.setup.sample_rate
        } else {
            48_000.0
        };
        frames / rate
    }

    /// Audio rendered and thrown away so far.
    pub fn audio_rendered(&self) -> Duration {
        Duration::from_secs_f64(self.blocks as f64 * self.block_seconds())
    }

    /// Rendered-audio seconds per wall second: this gate's CPU cost, expressed
    /// the way a DAW expresses it. Above [`Policy::max_rate`] means the pacing
    /// is not being honoured and a core is being burned for nothing.
    pub fn realtime_ratio(&self) -> f64 {
        let wall = self.elapsed.as_secs_f64();
        if wall <= 0.0 {
            return 0.0;
        }
        self.audio_rendered().as_secs_f64() / wall
    }

    /// Wall time at which the next block is owed, so the loop can sleep
    /// instead of spinning. See [`Policy::max_rate`].
    pub fn next_block_due(&self) -> Duration {
        if !(self.policy.max_rate.is_finite() && self.policy.max_rate > 0.0) {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(self.blocks as f64 * self.block_seconds() / self.policy.max_rate)
    }

    /// 0.0..=1.0 for a progress bar.
    ///
    /// The denominator is [`Policy::floor`] and not [`Policy::timeout`],
    /// because the floor is what a warm-up almost always costs and a bar
    /// scaled to the timeout would crawl for five seconds and then jump. While
    /// loading it saturates just below 1.0: a bar reading 100% next to a state
    /// that is still `Loading` is how a stuck plugin looks finished.
    pub fn progress(&self) -> f32 {
        if self.state != State::Loading {
            return 1.0;
        }
        let denom = self.policy.floor.as_secs_f32().max(f32::EPSILON);
        (self.elapsed.as_secs_f32() / denom).clamp(0.0, 0.99)
    }

    /// One line for the Recorder band. No em dashes: this is user-visible.
    pub fn status_line(&self) -> String {
        match self.state {
            State::Loading => format!(
                "{}... {:.1} s",
                self.phase.label(),
                self.elapsed.as_secs_f32()
            ),
            State::Ready => match self.evidence {
                Some(Evidence::Timeout) => {
                    "Instrument ready, but it never made a sound while warming up".to_string()
                }
                _ => "Instrument ready".to_string(),
            },
            State::Failed => format!(
                "Instrument failed: {}",
                self.reason.as_deref().unwrap_or("no reason recorded")
            ),
        }
    }
}

/// Render one block into `bufs` and return its peak, discarding the audio.
///
/// `bufs` is owned by the caller and reused, so a warm-up costs no allocation
/// per block. Its contents are meaningless after the call returns and are
/// never read by anything else: this is the "discarded buffer" of §8.
///
/// A NaN or an infinity anywhere in the block makes the return `f32::NAN`,
/// which is what [`Readiness::step`] tests for. It has to be tested rather than
/// max-ed away, because `f32::max` returns the *other* operand when one side is
/// NaN, so the obvious running-peak loop reports a healthy number from a block
/// full of NaNs.
pub fn block_peak(
    inst: &mut Instance,
    bufs: &mut Vec<Vec<f32>>,
    notes: &[Note],
) -> Result<f32, String> {
    let channels = inst
        .audio_outputs()
        .first()
        .map(|b| b.channels.max(0) as usize)
        .unwrap_or(0);
    if channels == 0 {
        return Err("the instrument has no audio output channels".to_string());
    }
    if bufs.len() < channels {
        bufs.resize(channels, Vec::new());
    }
    let frames = inst.setup().max_block.max(0) as usize;
    inst.process(notes, frames, bufs)?;

    let mut peak = 0.0f32;
    let mut finite = true;
    for ch in bufs.iter().take(channels) {
        for s in ch.iter().take(frames) {
            if s.is_finite() {
                let a = s.abs();
                if a > peak {
                    peak = a;
                }
            } else {
                finite = false;
            }
        }
    }
    Ok(if finite { peak } else { f32::NAN })
}

/// Warm an instrument up, blocking until it is [`State::Ready`] or
/// [`State::Failed`].
///
/// A convenience for examples, tests and any caller that can afford to block.
/// **The Recorder band must not use it**: it should own a [`Readiness`], step
/// it from the same thread that owns the instrument, and paint
/// [`Readiness::status_line`] every frame, so a fifteen-second timeout is a
/// progress bar rather than a frozen window.
///
/// The loop sleeps to honour [`Policy::max_rate`] rather than spinning, and the
/// probe note is released on every exit path, including failure.
pub fn warm_up(inst: &mut Instance, policy: Policy) -> Readiness {
    let setup = inst.setup();
    let mut gate = Readiness::new(setup, policy);
    let mut bufs: Vec<Vec<f32>> = Vec::new();
    let started = Instant::now();

    while gate.state() == State::Loading {
        let due = gate.next_block_due();
        let now = started.elapsed();
        if due > now {
            std::thread::sleep(due - now);
        }
        let at = started.elapsed();
        gate.step(at, &mut |notes| block_peak(inst, &mut bufs, notes));
    }
    // Failure can latch with the probe still down.
    gate.cancel(&mut |notes| block_peak(inst, &mut bufs, notes));
    gate
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic instrument. No plugin, no device, no audio: it decides what
    /// to "render" from the notes it is handed and how long it has been alive,
    /// which is exactly the two things a real one does.
    struct Fake {
        /// Silent before this, whatever it is sent. The §8 finding, modelled.
        loads_at: Duration,
        /// Peak while sounding.
        level: f32,
        /// Blocks the tail keeps sounding after a note-off.
        tail_blocks: u32,
        /// Sounds with no input at all, like a patch with a running arpeggiator.
        self_sounding: bool,
        /// Blocks after which every render fails.
        fails_at: Option<u64>,
        /// Blocks during which the output is NaN.
        nonfinite_until: Option<u64>,
        /// Blocks after which the note dies even though it is still held.
        note_dies_after: Option<u32>,

        holding: bool,
        held_blocks: u32,
        tail_left: u32,
        calls: u64,
        log: Vec<Note>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                loads_at: Duration::ZERO,
                level: 0.2,
                tail_blocks: 0,
                self_sounding: false,
                fails_at: None,
                nonfinite_until: None,
                note_dies_after: None,
                holding: false,
                held_blocks: 0,
                tail_left: 0,
                calls: 0,
                log: Vec::new(),
            }
        }

        fn render(&mut self, at: Duration, notes: &[Note]) -> Result<f32, String> {
            self.calls += 1;
            if self.fails_at.is_some_and(|n| self.calls > n) {
                return Err("the instrument stopped responding".to_string());
            }
            for n in notes {
                self.log.push(*n);
                if n.on {
                    self.holding = true;
                    self.held_blocks = 0;
                } else {
                    self.holding = false;
                    self.tail_left = self.tail_blocks;
                }
            }
            if self.nonfinite_until.is_some_and(|n| self.calls <= n) {
                return Ok(f32::NAN);
            }
            let loaded = at >= self.loads_at;
            let note_alive = self.holding
                && !self
                    .note_dies_after
                    .is_some_and(|n| self.held_blocks >= n);
            let sounding = loaded && (self.self_sounding || note_alive || self.tail_left > 0);
            if self.holding {
                self.held_blocks += 1;
            } else if self.tail_left > 0 {
                self.tail_left -= 1;
            }
            Ok(if sounding { self.level } else { 0.0 })
        }

        fn note_ons(&self) -> usize {
            self.log.iter().filter(|n| n.on).count()
        }

        fn note_offs(&self) -> usize {
            self.log.iter().filter(|n| !n.on).count()
        }
    }

    /// A policy whose every wait is short, so a fifteen-second timeout is a
    /// few hundred synthetic blocks instead of fifteen real seconds.
    fn quick() -> Policy {
        Policy {
            listen_first: Duration::from_millis(50),
            floor: Duration::from_millis(500),
            timeout: Duration::from_millis(2_000),
            drain_cap: Duration::from_millis(400),
            probe_hold: Duration::from_millis(150),
            probe_gap: Duration::from_millis(100),
            ..Policy::default()
        }
    }

    /// Step the gate on a fake clock until it settles, and return when.
    fn drive(gate: &mut Readiness, fake: &mut Fake, limit: Duration) -> Duration {
        let block = Duration::from_secs_f64(gate.block_seconds());
        let mut at = Duration::ZERO;
        while gate.state() == State::Loading && at < limit {
            at += block;
            gate.step(at, &mut |notes| fake.render(at, notes));
        }
        at
    }

    fn gate(policy: Policy) -> Readiness {
        Readiness::new(Setup::default(), policy)
    }

    #[test]
    fn an_instrument_that_sounds_immediately_still_waits_out_the_warm_up_floor() {
        // the modelled piano's shape: 0.342 cold. The gate cannot tell it apart from
        // Augmented GRAND's 0.003, so it pays the floor either way.
        let mut fake = Fake::new();
        let mut g = gate(quick());
        let at = drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert_eq!(g.evidence(), Some(Evidence::Probe));
        assert!(
            at >= quick().floor,
            "ready at {at:?}, which is before the {:?} floor",
            quick().floor
        );
    }

    #[test]
    fn an_instrument_silent_for_its_first_second_is_not_declared_ready_until_it_sounds() {
        // Piano V3 and Stage-73 V2: 0.000 cold, 0.2 after warming up. A gate
        // that armed on instantiation would have recorded a silent take.
        let mut fake = Fake::new();
        fake.loads_at = Duration::from_millis(1_000);
        let mut g = gate(quick());
        let at = drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert_eq!(g.evidence(), Some(Evidence::Probe));
        assert!(
            at >= Duration::from_millis(1_000),
            "declared ready at {at:?}, before the instrument could make a sound"
        );
        assert!(
            fake.note_ons() > 1,
            "one probe at t=0 is lost forever on a plugin that loads later; \
             the gate must keep probing, and it played {} notes",
            fake.note_ons()
        );
    }

    #[test]
    fn an_instrument_that_never_makes_a_sound_is_declared_ready_rather_than_hanging_the_ui() {
        let mut fake = Fake::new();
        fake.level = 0.0;
        let mut g = gate(quick());
        let at = drive(&mut g, &mut fake, Duration::from_secs(30));
        assert_eq!(g.state(), State::Ready);
        assert_eq!(
            g.evidence(),
            Some(Evidence::Timeout),
            "ready-by-timeout must be distinguishable from ready-by-measurement, \
             or the UI cannot warn that the take may be silent"
        );
        assert!(
            at < quick().timeout + quick().drain_cap + Duration::from_millis(200),
            "took {at:?}, which is longer than timeout plus drain cap"
        );
        assert!(g.status_line().contains("never made a sound"));
    }

    #[test]
    fn an_instrument_that_sounds_and_then_falls_silent_is_still_ready() {
        // A plucked or percussive patch: the probe rings for a moment and dies
        // while the key is still down. That is a working instrument, not a
        // failing one.
        let mut fake = Fake::new();
        fake.note_dies_after = Some(4);
        let mut g = gate(quick());
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert_eq!(g.evidence(), Some(Evidence::Probe));
    }

    #[test]
    fn every_probe_note_is_released_before_the_gate_reports_ready() {
        let mut fake = Fake::new();
        fake.loads_at = Duration::from_millis(800);
        let mut g = gate(quick());
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert_eq!(
            fake.note_ons(),
            fake.note_offs(),
            "a probe note-on with no matching off is a note held into the take"
        );
        assert_eq!(g.stuck_note(), None);
        assert!(g.may_arm());
    }

    #[test]
    fn no_note_is_sent_before_the_instrument_has_rendered_a_block() {
        let mut fake = Fake::new();
        fake.level = 0.0;
        let mut g = gate(quick());
        g.step(Duration::from_millis(11), &mut |notes| {
            fake.render(Duration::from_millis(11), notes)
        });
        assert!(
            fake.log.is_empty(),
            "the very first block must carry no events: it is how a self-sounding \
             patch is detected, and it means no plugin is handed a note before it \
             has been asked to render anything"
        );
    }

    #[test]
    fn an_instrument_already_making_noise_is_never_sent_a_probe_note() {
        let mut fake = Fake::new();
        fake.self_sounding = true;
        let mut g = gate(quick());
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert_eq!(g.evidence(), Some(Evidence::Unprompted));
        assert_eq!(
            g.probes_played(),
            0,
            "a patch that already sounds needs no probe, and probing it would put \
             a note into the monitor path for nothing"
        );
        assert!(fake.log.is_empty());
    }

    #[test]
    fn the_gate_waits_for_the_probe_tail_to_go_quiet_before_it_reports_ready() {
        // Twenty blocks of tail is ~210 ms at 512/48k, well inside the drain
        // cap, so the gate should absorb all of it rather than arming on top
        // of a ringing note.
        let mut fake = Fake::new();
        fake.tail_blocks = 20;
        let mut g = gate(Policy {
            floor: Duration::from_millis(10),
            ..quick()
        });
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert!(
            !g.probe_tail_capped(),
            "a 210 ms tail fits inside the {:?} drain cap",
            quick().drain_cap
        );
        // The last thing the instrument was told was a note-off, and the gate
        // kept rendering afterwards until it went quiet.
        assert!(!fake.log.last().expect("a probe was played").on);
        assert_eq!(fake.tail_left, 0, "the tail was not fully drained");
    }

    #[test]
    fn a_tail_that_never_decays_is_capped_and_reported_rather_than_waited_on_forever() {
        let mut fake = Fake::new();
        fake.tail_blocks = u32::MAX;
        let mut g = gate(Policy {
            floor: Duration::from_millis(10),
            ..quick()
        });
        let at = drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert!(
            g.probe_tail_capped(),
            "an endless reverb tail must not hold the gate shut, and the caller \
             must be told that a decaying probe may still be audible"
        );
        assert!(at < Duration::from_secs(10));
        assert_eq!(g.stuck_note(), None);
    }

    #[test]
    fn a_render_error_fails_the_gate_with_a_reason_a_user_can_read() {
        let mut fake = Fake::new();
        fake.fails_at = Some(3);
        let mut g = gate(quick());
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Failed);
        assert_eq!(g.reason(), Some("the instrument stopped responding"));
        assert!(g.status_line().starts_with("Instrument failed:"));
        assert!(!g.may_arm());
    }

    #[test]
    fn a_block_that_fails_on_the_probe_release_still_reports_the_note_as_held() {
        // `probe_hold: ZERO` makes the block after the note-on the release
        // block, so the failure lands exactly on the note-off.
        let policy = Policy {
            probe_hold: Duration::ZERO,
            ..quick()
        };
        let mut fake = Fake::new();
        fake.level = 0.0;
        let mut g = gate(policy);
        let block = Duration::from_secs_f64(g.block_seconds());
        let mut at = Duration::ZERO;
        while g.stuck_note().is_none() {
            at += block;
            g.step(at, &mut |notes| fake.render(at, notes));
            assert!(at < Duration::from_secs(5), "the gate never probed");
        }

        at += block;
        let mut dead = |_: &[Note]| -> Result<f32, String> { Err("the plugin died".to_string()) };
        assert_eq!(g.step(at, &mut dead), State::Failed);
        assert_eq!(
            g.stuck_note(),
            Some(policy.probe_pitch),
            "a block that errored is not proof the note-off reached the plugin, and \
             a gate that forgets the note leaves an instrument sounding into the take"
        );
        g.cancel(&mut |notes| fake.render(at, notes));
        assert_eq!(fake.note_offs(), 1);
    }

    #[test]
    fn a_terminal_state_is_latched_and_never_returns_to_loading() {
        // The UI polls this. A Ready that can flip back to Loading is a
        // recorder that disarms itself mid-take.
        let mut fake = Fake::new();
        let mut g = gate(quick());
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        let blocks = g.blocks();
        for k in 1..50u64 {
            let at = Duration::from_secs(60 + k);
            let mut dead = |_: &[Note]| -> Result<f32, String> { Err("gone".to_string()) };
            assert_eq!(g.step(at, &mut dead), State::Ready);
        }
        assert_eq!(g.blocks(), blocks, "a latched gate must render nothing more");
    }

    #[test]
    fn a_non_finite_sample_after_the_floor_fails_rather_than_poisoning_the_take() {
        let mut fake = Fake::new();
        fake.nonfinite_until = Some(u64::MAX);
        let mut g = gate(quick());
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Failed);
        assert!(
            g.reason().is_some_and(|r| r.contains("non-finite")),
            "got {:?}",
            g.reason()
        );
        assert!(g.nonfinite_blocks() > 0);
    }

    #[test]
    fn a_non_finite_sample_before_the_floor_is_tolerated_because_that_is_what_the_floor_is_for() {
        // A plugin rendering garbage while it initialises is exactly the
        // condition the warm-up exists to wait out. Failing on block 1 would
        // reject an instrument that works.
        let mut fake = Fake::new();
        fake.nonfinite_until = Some(8);
        let mut g = gate(quick());
        drive(&mut g, &mut fake, Duration::from_secs(10));
        assert_eq!(g.state(), State::Ready);
        assert_eq!(g.nonfinite_blocks(), 8);
    }

    #[test]
    fn the_measured_cold_reading_of_augmented_grand_does_not_clear_the_threshold() {
        // RECORDER-PLAN §8 Spike 2: Augmented GRAND PIANO reads 0.003 cold and
        // 0.167 warm. A -60 dBFS threshold would have called 0.003 "it made a
        // sound" and armed the take 35 dB down. This test is the reason the
        // default is -40 dBFS and it fails the moment somebody lowers it.
        let p = Policy::default();
        assert!(p.sound_threshold > 0.003, "cold Augmented GRAND PIANO");
        assert!(p.sound_threshold > 0.0007, "cold CP-70 V");
        assert!(p.sound_threshold < 0.14, "every warm reading in the table");
    }

    #[test]
    fn arming_is_refused_for_the_whole_of_the_warm_up() {
        let mut fake = Fake::new();
        fake.loads_at = Duration::from_millis(900);
        let mut g = gate(quick());
        let block = Duration::from_secs_f64(g.block_seconds());
        let mut at = Duration::ZERO;
        while g.state() == State::Loading {
            assert!(!g.may_arm(), "armed while still loading, at {at:?}");
            at += block;
            g.step(at, &mut |notes| fake.render(at, notes));
            assert!(at < Duration::from_secs(10), "the gate never settled");
        }
        assert!(g.may_arm());
    }

    #[test]
    fn elapsed_and_blocks_are_reported_so_a_stuck_instrument_is_visibly_stuck() {
        let mut fake = Fake::new();
        fake.level = 0.0;
        let mut g = gate(quick());
        let block = Duration::from_secs_f64(g.block_seconds());
        for k in 1..=100u32 {
            let at = block * k;
            g.step(at, &mut |notes| fake.render(at, notes));
        }
        assert_eq!(g.state(), State::Loading);
        assert_eq!(g.blocks(), 100);
        assert_eq!(g.phase(), Phase::Probing);
        assert!(g.elapsed() > Duration::ZERO);
        assert!(g.progress() > 0.0 && g.progress() < 1.0);
        assert!(
            g.status_line().starts_with("Testing instrument"),
            "got {:?}",
            g.status_line()
        );
    }

    #[test]
    fn a_progress_bar_never_reads_full_while_the_state_is_still_loading() {
        let mut fake = Fake::new();
        fake.level = 0.0;
        let mut g = gate(quick());
        let block = Duration::from_secs_f64(g.block_seconds());
        for k in 1..=150u32 {
            let at = block * k;
            g.step(at, &mut |notes| fake.render(at, notes));
        }
        assert_eq!(g.state(), State::Loading);
        assert!(
            g.elapsed() > quick().floor,
            "this test is only meaningful past the floor"
        );
        assert!(
            g.progress() < 1.0,
            "a bar at 100% beside a Loading state is how a stuck plugin looks finished"
        );
    }

    #[test]
    fn rendering_is_paced_at_realtime_so_the_plugins_loader_thread_gets_wall_time() {
        // Rendering as fast as the machine allows turns a five second warm-up
        // into a few hundred milliseconds of wall clock, which is the one
        // resource the plugin's background loader actually needs.
        let g = gate(Policy::default());
        assert_eq!(g.next_block_due(), Duration::ZERO);

        let mut fake = Fake::new();
        let mut g = gate(quick());
        let block = Duration::from_secs_f64(g.block_seconds());
        for k in 1..=100u32 {
            let at = block * k;
            g.step(at, &mut |notes| fake.render(at, notes));
        }
        let due = g.next_block_due();
        let expected = block * g.blocks() as u32;
        assert!(
            due.abs_diff(expected) < Duration::from_micros(50),
            "block {} is due at {due:?}, expected {expected:?}",
            g.blocks()
        );
        assert!(
            (g.realtime_ratio() - 1.0).abs() < 0.05,
            "at max_rate 1.0 the gate should render about one second of audio per \
             second of wall clock, got {:.3}",
            g.realtime_ratio()
        );
    }

    #[test]
    fn a_backwards_clock_stalls_the_gate_rather_than_restarting_the_warm_up() {
        let mut fake = Fake::new();
        let mut g = gate(quick());
        let block = Duration::from_secs_f64(g.block_seconds());
        for k in 1..=20u32 {
            let at = block * k;
            g.step(at, &mut |notes| fake.render(at, notes));
        }
        let before = g.elapsed();
        g.step(Duration::ZERO, &mut |notes| fake.render(Duration::ZERO, notes));
        assert_eq!(
            g.elapsed(),
            before,
            "a caller mixing two clocks must stall the gate, not rewind the floor \
             and warm the instrument up all over again"
        );
    }

    #[test]
    fn cancel_releases_a_probe_note_left_held_by_an_abandoned_warm_up() {
        let mut fake = Fake::new();
        fake.level = 0.0;
        let mut g = gate(quick());
        let block = Duration::from_secs_f64(g.block_seconds());
        let mut at = Duration::ZERO;
        // Walk forward until the probe is down, then walk away from the gate.
        while g.stuck_note().is_none() {
            at += block;
            g.step(at, &mut |notes| fake.render(at, notes));
            assert!(at < Duration::from_secs(5), "the gate never probed");
        }
        assert_eq!(fake.note_ons(), 1);
        assert_eq!(fake.note_offs(), 0);

        g.cancel(&mut |notes| fake.render(at, notes));
        assert_eq!(g.stuck_note(), None);
        assert_eq!(
            fake.note_offs(),
            1,
            "abandoning a warm-up mid-probe must not leave the instrument sounding"
        );
    }

    #[test]
    fn cancel_on_a_gate_holding_nothing_renders_nothing() {
        let mut g = gate(quick());
        let mut called = 0u32;
        g.cancel(&mut |_: &[Note]| -> Result<f32, String> {
            called += 1;
            Ok(0.0)
        });
        assert_eq!(called, 0);
    }

    #[test]
    fn a_setup_with_a_nonsense_block_size_does_not_divide_by_zero() {
        let g = Readiness::new(
            Setup {
                sample_rate: 0.0,
                max_block: 0,
            },
            Policy::default(),
        );
        assert!(g.block_seconds() > 0.0);
        assert_eq!(g.audio_rendered(), Duration::ZERO);
        assert_eq!(g.realtime_ratio(), 0.0);
    }

    #[test]
    #[ignore = "needs a real VST3 instrument installed; every other test here runs \
                with no plugin, no device and no audio"]
    fn a_real_instrument_warms_up_and_then_makes_a_sound() {
        let Some(bundle) = crate::scan::discover().into_iter().find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase().contains("piano"))
                .unwrap_or(false)
        }) else {
            panic!("no VST3 matching the modelled piano; this test needs one installed");
        };
        let module = crate::scan::Module::open(&bundle).expect("open module");
        let class = module
            .audio_modules()
            .into_iter()
            .next()
            .expect("no Audio Module Class");
        let mut inst =
            Instance::create(&module, &class, Setup::default()).expect("instantiate");

        let gate = warm_up(&mut inst, Policy::default());
        assert_eq!(gate.state(), State::Ready, "{}", gate.status_line());
        assert_ne!(
            gate.evidence(),
            Some(Evidence::Timeout),
            "a real piano must be detected by measurement, not by timeout"
        );
        assert_eq!(gate.stuck_note(), None);
        assert!(gate.elapsed() >= Policy::default().floor);
    }

    // ── the LOAD policy ───────────────────────────────────────────────────

    /// **The load policy still concludes by measurement.** This is the whole
    /// claim: `Policy::at_load` gives up the WAIT, not the evidence. An
    /// instrument that sounds is declared ready because it was heard —
    /// `Evidence::Probe` — exactly as it is under `Policy::default`, and the
    /// probe it played is released and drained before the gate lets go.
    #[test]
    fn the_load_policy_still_decides_by_hearing_the_instrument() {
        let mut fake = Fake::new();
        fake.loads_at = Duration::from_millis(120);
        let mut g = gate(Policy::at_load());
        let at = drive(&mut g, &mut fake, Duration::from_secs(30));
        assert_eq!(g.state(), State::Ready, "{}", g.status_line());
        assert_eq!(
            g.evidence(),
            Some(Evidence::Probe),
            "the load policy fell back on a timer instead of hearing the instrument"
        );
        assert_eq!(g.stuck_note(), None, "the probe was left sounding into the rack");
        assert_eq!(fake.note_ons(), fake.note_offs(), "a probe went out unreleased");
        assert!(
            at < Policy::default().floor,
            "a bay load waited {at:?}, which is the arming floor of {:?} all over again",
            Policy::default().floor
        );
    }

    /// **The arming floor is not the loading path's to pay, and the numbers
    /// say how much of it there was.** Seven bays of one real sampled
    /// instrument measured 40.3 s under `default` and 6.05 s under `at_load`;
    /// this is the shape of that difference with no plugin in the room.
    #[test]
    fn a_bay_load_does_not_wait_out_the_arming_floor() {
        let ready_at = |policy: Policy| {
            let mut fake = Fake::new();
            let mut g = gate(policy);
            let at = drive(&mut g, &mut fake, Duration::from_secs(60));
            assert_eq!(g.state(), State::Ready);
            at
        };
        let arming = ready_at(Policy::default());
        let loading = ready_at(Policy::at_load());
        assert!(
            arming >= Policy::default().floor,
            "the arming policy stopped paying its floor; the §8 evidence is about that wait"
        );
        assert!(
            loading * 4 < arming,
            "the load policy took {loading:?} against the arming policy's {arming:?}, \
             which is not the difference the owner's forty seconds needed"
        );
    }

    /// **A plugin rendering garbage while it initialises still LOADS.**
    ///
    /// `step` tolerates a non-finite sample until the floor and refuses it
    /// after, on the ground that garbage during initialisation is the very
    /// condition the gate is waiting out. A floor of ZERO would therefore
    /// have made this gate stricter than it has ever been — the first NaN of
    /// a plugin's first block would refuse the load — so `at_load`'s floor is
    /// a shade LONGER than the gate's own life rather than nothing at all.
    /// This is the test that stops somebody rounding it down to zero.
    #[test]
    fn the_load_policy_still_waits_out_a_plugin_that_starts_up_rendering_nans() {
        let mut fake = Fake::new();
        // Ten blocks of NaN is about the gate's whole listening window at
        // this block size, which is what a sampler wiring up its buffers
        // looks like from out here.
        fake.nonfinite_until = Some(10);
        let mut g = gate(Policy::at_load());
        drive(&mut g, &mut fake, Duration::from_secs(30));
        assert_eq!(
            g.state(),
            State::Ready,
            "an instrument that NaN'd while it initialised was refused: {}",
            g.status_line()
        );
        assert!(g.nonfinite_blocks() > 0, "the fake never rendered the NaNs this is about");
    }

    /// **An instrument that never answers the probe is handed over in about a
    /// second, not fifteen.**
    ///
    /// `probe_pitch` is C7, and plenty of instruments cannot sound there at
    /// all: a bass, a sub, a kick, any keyboard whose top note is below it.
    /// Under the arming policy each of those waits out the full timeout, and
    /// three of seven bays did exactly that at 15.1 s each the first time the
    /// real probe was run. The verdict is the same — ready, by decision — and
    /// the wait is not.
    #[test]
    fn an_instrument_that_cannot_sound_at_the_probe_pitch_is_not_a_fifteen_second_wait() {
        let give_up_at = |policy: Policy| {
            let mut fake = Fake::new();
            fake.level = 0.0;
            let mut g = gate(policy);
            let at = drive(&mut g, &mut fake, Duration::from_secs(60));
            assert_eq!(g.state(), State::Ready);
            assert_eq!(g.evidence(), Some(Evidence::Timeout));
            assert_eq!(g.stuck_note(), None);
            at
        };
        let arming = give_up_at(Policy::default());
        let loading = give_up_at(Policy::at_load());
        assert!(
            arming >= Policy::default().timeout,
            "the arming policy stopped waiting out its own timeout"
        );
        assert!(
            loading * 5 < arming,
            "a silent instrument cost {loading:?} at load against {arming:?} at arming"
        );
    }

    /// **The arming policy is untouched, and that is deliberate.**
    ///
    /// `may_arm` has no caller yet; when it gets one, `Policy::default` is
    /// the policy that decision must use and `at_load` is the one it must
    /// not. A test rather than a comment, because the cheapest way to
    /// "simplify" this pair is to make one of them the other.
    #[test]
    fn the_arming_policy_keeps_the_numbers_its_evidence_is_about() {
        assert_eq!(Policy::default().floor, Duration::from_secs(5));
        assert_eq!(Policy::default().timeout, Duration::from_secs(15));
        assert!(Policy::at_load().floor < Policy::default().floor);
        assert!(Policy::at_load().timeout < Policy::default().timeout);
        // Everything the gate DECIDES by is the same on both, which is why
        // the load policy's verdict is still a measurement.
        let (a, b) = (Policy::default(), Policy::at_load());
        assert_eq!(a.sound_threshold, b.sound_threshold);
        assert_eq!(a.confirm_blocks, b.confirm_blocks);
        assert_eq!(a.quiet_blocks, b.quiet_blocks);
        assert_eq!(a.listen_first, b.listen_first);
        assert_eq!(a.probe_pitch, b.probe_pitch);
        assert_eq!(a.drain_cap, b.drain_cap);
        assert_eq!(a.max_rate, b.max_rate);
    }

    /// Throwaway probe: **stage by stage, what does one bay of a real
    /// sampled instrument cost to bring up, and what does a deskful of them
    /// cost?** Ignored by default; run with
    ///
    /// ```text
    /// TANGENT_PROBE_BUNDLE="/Library/Audio/Plug-Ins/VST3/Rhodes Anthology.vst3" \
    /// TANGENT_PROBE_STATE=".../state/insert-state-0-0.bin" \
    /// TANGENT_PROBE_N=7 \
    /// cargo test -p tangent-host --lib --release -- --ignored what_a_deskful --nocapture
    /// ```
    ///
    /// The sequence is `Engine::prepare_vst3`'s, in its order and with its
    /// arguments: the module (which `scan::module_for` caches, so the second
    /// and later bays of one bundle pay a hash lookup rather than a
    /// `bundleEntry`), the class list, `Instance::create`, the saved state,
    /// [`warm_up`], and `set_processing`. Nothing here is a model of the
    /// load; it IS the load, minus the bookkeeping either side of it.
    ///
    /// **Run it in `--release`.** A debug build's own arithmetic is nothing
    /// beside somebody else's optimised sampler, but the block loop in this
    /// file runs several hundred times per warm-up and the difference shows.
    #[test]
    #[ignore = "measures a machine, and runs a vendor's initialiser several times"]
    fn what_a_deskful_of_sampled_instruments_costs_to_open() {
        let Some(bundle) = std::env::var_os("TANGENT_PROBE_BUNDLE").map(std::path::PathBuf::from)
        else {
            panic!("set TANGENT_PROBE_BUNDLE to a VST3 bundle; this probe measures a real one");
        };
        // A sidecar on disk is `<bundle path>\0<container>`; the app's
        // `saved_insert_state` splits it at the NUL and checks the owner
        // before handing the rest to the plugin. Handing the whole file over
        // instead is a blob the container rejects in a microsecond, which
        // measures a plugin at its DEFAULT preset — a different instrument
        // from the one the project opens, and for a sampler a different
        // amount of disk to read.
        //
        // A COMMA-SEPARATED LIST, one per bay, because a real session's bays
        // hold different presets and a preset decides both how much the
        // plugin loads and whether the gate's probe note is inside its
        // range. One path is used for every bay.
        let states: Vec<Vec<u8>> = std::env::var("TANGENT_PROBE_STATE")
            .ok()
            .into_iter()
            .flat_map(|v| v.split(',').map(str::to_owned).collect::<Vec<_>>())
            .filter(|p| !p.is_empty())
            .map(|p| std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}")))
            .map(|raw| match raw.iter().position(|b| *b == 0) {
                Some(at) => raw[at + 1..].to_vec(),
                None => raw,
            })
            .collect();
        let n: usize = std::env::var("TANGENT_PROBE_N")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| states.len().max(1));

        // **The policy is a knob, because the policy is the question.** The
        // floor and the timeout are the two numbers that decide what a bay
        // load costs, and the only honest way to choose them was to run the
        // real instrument under each. [`Policy::at_load`] is the default here
        // because it is what `Engine::prepare_vst3` passes, so a bare run of
        // this probe measures the app; `TANGENT_PROBE_FLOOR_MS=5000
        // TANGENT_PROBE_TIMEOUT_MS=15000` puts [`Policy::default`]'s numbers
        // back and reproduces the 40.3 s the fix was measured against.
        let ms = |var: &str, fallback: Duration| -> Duration {
            std::env::var(var)
                .ok()
                .and_then(|v| v.parse().ok())
                .map_or(fallback, Duration::from_millis)
        };
        let policy = Policy {
            floor: ms("TANGENT_PROBE_FLOOR_MS", Policy::at_load().floor),
            timeout: ms("TANGENT_PROBE_TIMEOUT_MS", Policy::at_load().timeout),
            ..Policy::at_load()
        };
        println!(
            "{} x {n}, floor {:?} / timeout {:?}",
            bundle.display(),
            policy.floor,
            policy.timeout
        );
        let whole = Instant::now();
        // Held, not dropped: seven bays of a sampled instrument are seven
        // live instances on a real desk, and a probe that freed each one
        // before making the next would measure a machine that is not the
        // one the owner has.
        let mut kept: Vec<Instance> = Vec::new();
        let mut totals = [Duration::ZERO; 7];
        for bay in 0..n {
            let t = Instant::now();
            let module = crate::scan::module_for(&bundle).expect("open the bundle");
            let (t, open) = (Instant::now(), t.elapsed());
            let classes = module.audio_modules();
            let class = classes.first().expect("an Audio Module Class").clone();
            let (t, list) = (Instant::now(), t.elapsed());
            let mut inst =
                Instance::create(&module, &class, Setup::default()).expect("instantiate");
            let (t, create) = (Instant::now(), t.elapsed());
            let restored = match states.get(bay % states.len().max(1)) {
                Some(bytes) => inst.load_state(bytes).is_ok(),
                None => false,
            };
            let (t, restore) = (Instant::now(), t.elapsed());
            let voice = class.kind() == crate::scan::Kind::Instrument;
            let gate = voice.then(|| warm_up(&mut inst, policy));
            let (t, warm) = (Instant::now(), t.elapsed());
            inst.set_processing(true).expect("processing on");
            let processing = t.elapsed();
            // **What the RACK is handed.** The gate's drain ends eight blocks
            // below -40 dBFS, and under `Policy::default` five more seconds
            // of settling follow it, so the probe note is long gone by the
            // time the instance crosses to the audio thread. Shorten the
            // floor and that is no longer true by construction, so it is
            // measured instead: the peak of the first blocks the instance
            // renders after the gate has let go, which is what a listener
            // would hear.
            let mut after: f32 = 0.0;
            let mut bufs2: Vec<Vec<f32>> = Vec::new();
            for _ in 0..8 {
                after = after.max(block_peak(&mut inst, &mut bufs2, &[]).unwrap_or(0.0).abs());
            }
            // **And what CLOSING this bay costs**, because a project switch
            // pays it before the incoming one is read: `save_plugin_states`
            // asks every occupied bay of the OUTGOING session for its state,
            // and `save_insert_state`'s own doc says asking a sampler for one
            // is not free. Measured on the same instance rather than guessed
            // at.
            let t = Instant::now();
            let saved = inst.state_handle().save().map(|b| b.len()).unwrap_or(0);
            let get_state = t.elapsed();
            totals[6] += get_state;
            for (slot, d) in
                [open, list, create, restore, warm, processing].into_iter().enumerate()
            {
                totals[slot] += d;
            }
            println!(
                "  bay {bay}: module_for {open:?} | audio_modules {list:?} | create {create:?} \
                 | load_state {restore:?} ({}) | warm_up {warm:?} ({}) | set_processing \
                 {processing:?} | handed to the rack at {:.1} dBFS | getState {get_state:?} \
                 ({saved} bytes)",
                if restored { "restored" } else { "none" },
                match &gate {
                    None => "an effect, no gate".to_owned(),
                    Some(g) => format!("{:?}, {:?}", g.state(), g.evidence()),
                },
                20.0 * after.max(1e-9).log10(),
            );
            kept.push(inst);
        }
        let elapsed = whole.elapsed();
        let names = ["module_for", "audio_modules", "create", "load_state", "warm_up", "processing"];
        // The open is what the first six cost; `getState` belongs to CLOSING
        // the outgoing session, so it is reported beside the table and not
        // inside it.
        let open_total: Duration = totals[..6].iter().sum();
        println!("  --- {n} bays, {elapsed:?} of wall clock ---");
        for (name, total) in names.into_iter().zip(totals) {
            println!(
                "  {name:<14} {total:>12?}  {:>5.1}% of the open",
                total.as_secs_f64() / open_total.as_secs_f64() * 100.0
            );
        }
        println!(
            "  {:<14} {:>12?}  what a project SWITCH pays for these bays before the \
             incoming document is read (save_plugin_states)",
            "getState", totals[6],
        );
        // Kept alive to here on purpose; dropping them is not part of the
        // number above.
        drop(kept);
    }
}
