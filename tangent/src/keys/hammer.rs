//! The hammer: a mass on a felt spring, in contact with the string for
//! exactly as long as the string lets it.
//!
//! **What this replaces, and why.** `string.rs` excites its waveguide with
//! a Gaussian force pulse whose width is a hand-tuned function of velocity:
//! `sigma_ms = (0.31 - 0.17 v^0.85) * felt`. That curve is a RESULT — the
//! contact time a real hammer produces — written down as if it were a
//! cause. It is right where it was measured and wrong everywhere it was
//! not: it cannot know that a string already moving toward the hammer
//! throws it back sooner, or that a string running away from it lets the
//! felt breathe and hit again. This file computes the contact instead of
//! describing it, and the width comes out of the arithmetic with no
//! velocity-to-width curve anywhere in the file. `contact_secs` exists so
//! the bench can watch that claim.
//!
//! **The model, in one paragraph** (Chaigne & Askenfelt, *Numerical
//! simulations of piano strings I*, JASA 95(2) 1994, eq. 6 and the hammer's
//! equation of motion). The hammer is a point mass `M` at position `eta`,
//! launched at the string with velocity `v`. Felt is a spring that stiffens
//! as it is squeezed: with compression `delta = eta - y(x0, t)` (the hammer
//! minus the string under it) the force is `F = K delta^p` for `delta > 0`
//! and nothing at all otherwise — felt pushes, it does not pull. The hammer
//! decelerates against that force, `M eta'' = -F`, and the SAME force goes
//! into the string as a velocity jump `F / (2 Z)`, `Z` the string's wave
//! impedance. The string's own motion is what closes the loop: it moves
//! away at `F / (2 Z)`, which unloads the felt, and the wave that left
//! toward the agraffe comes back inverted after `2 x0 / c` — one eighth of
//! the period at the striking ratio `design` uses — and pushes the hammer
//! off. Soft felt at pianissimo makes a long gentle hump; at fortissimo the
//! felt is stiff, the hammer bounces on the short string segment between
//! itself and the agraffe, and the contact is half as long. That halving
//! is the instrument's whole dynamic range and it is nowhere in the code.
//!
//! **Hysteresis.** Real felt loads stiffer than it unloads. The Hunt-Crossley
//! form adds a rate term, `F = K delta^p (1 + mu delta')`, so the same
//! compression carries more force on the way in than on the way out; the
//! difference is energy the felt eats. `mu = 0` is Chaigne and Askenfelt's
//! law exactly.
//!
//! **The constants are FITTED, not looked up.** The brief could not extract
//! Chaigne and Askenfelt's table of measured hammer constants, so `K`, `M`
//! and `p` are fitted here against the two contact times the instrument was
//! signed off with (2.1 ms at MIDI 8, 1.0 ms at MIDI 127), with `p` held
//! inside the 2.2..3.5 that Hall and Askenfelt measured on hammers taken from
//! real pianos. See `HammerBase::JAZZ` for what came out and what it means.
//!
//! **Units are SI throughout** — metres, seconds, newtons — because every
//! constant in the literature is, and a kernel that carried the waveguide's
//! dimensionless amplitudes would have to be re-fitted every time the level
//! calibration in `string.rs` moved. The caller owns ONE scale: how many
//! waveguide units a metre is. See [`Hammer::tick`] for the contract.

/// A hammer's material identity: what it weighs and what its felt is made
/// of. The knobs in `string.rs` may scale these; they do not replace them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HammerBase {
    /// Effective mass at the strike point, kilograms.
    pub mass: f32,
    /// Felt stiffness `K` in `F = K delta^p`, newtons per metre^p.
    pub stiffness: f32,
    /// Felt exponent `p`. Measured hammers sit in 2.2..3.5; a linear spring
    /// would be 1.0 and would have no dynamic range at all.
    pub exponent: f32,
    /// Hunt-Crossley rate coefficient `mu`, seconds per metre: the force is
    /// scaled by `1 + mu * delta'`. Zero is a lossless felt.
    pub hysteresis: f32,
}

impl HammerBase {
    /// The jazz grand's hammer, fitted at C4 against three unison strings
    /// of 2.33 N s/m each — first on `tests::IdealString`, then on the real
    /// loop in `string.rs`, which agree on the contact to the third
    /// decimal — so that the first contact runs 2.1 ms at MIDI 8 and 1.0 ms
    /// at MIDI 127 through [`velocity_of_midi`].
    ///
    /// **The felt is at the soft end of the measured range, and the
    /// partial table put it there.** With both ends of the contact pinned,
    /// the exponent is the one free parameter, and it sets how fast the
    /// contact collapses between them: a stiff power law (p 3) is down to a
    /// millisecond by mezzo-forte, a soft one (p 2.2) still at 1.08 there.
    /// `string::tests::scan_hammers` measured the C4 mezzo partials against
    /// the signed-off table for p from 2.2 to 3.5 and the whole row lands
    /// within 1.5 dB only at 2.2; at 3.0 the sixth and seventh partials are
    /// 9 and 13 dB hot, because a felt pulse's spectrum has a power-law
    /// tail where the Gaussian it replaces fell as `exp(-f^2)`, and the
    /// stiffer the felt the fatter the tail. The stiffness follows from the
    /// exponent and the soft-end contact; 4.57e7 N/m^2.2 is not a felt
    /// constant anyone would quote, because `K delta^p` with p 2.2 wants a
    /// different K than with p 3 for the same force at the same
    /// compression (0.3 mm here gives 8 N either way).
    ///
    /// **The mass is an effective mass, and it is light — decided, not
    /// doubted.** A C4 hammer head weighs about eight grams and the
    /// simulation literature uses around three; this one is a gram. It has
    /// to be, because in this model the loud-end contact is a bounce on
    /// the string segment between hammer and agraffe, whose stiffness is
    /// `3 T / x0` — about 30 kN/m here — and a half period of one
    /// millisecond on that spring is only reached by a gram. The scan
    /// measured the alternative: two and three grams at any exponent land
    /// the fortissimo contact at 1.17 to 1.40 ms, outside the 1.0 ms the
    /// instrument was signed off with, and their mezzo tables are 4 to 9 dB
    /// louder at the fundamental for the same arrival speed, which is the
    /// level calibration moving under the felt. So a gram it is; if the
    /// 1.0 ms target (the old Gaussian's truncation width, not a
    /// measurement) is ever re-baselined toward 1.3, three grams becomes
    /// the physical choice and this paragraph says where to look.
    ///
    /// **The fortissimo pulse is asymmetric, and stays so.** It rises
    /// fast, peaks early and carries the agraffe reflection out as a
    /// shoulder (`tests::loud_pulse_peaks_early`), and on the bench that
    /// reads as waveform asymmetry 0.20 at MIDI 127 where the Gaussian's
    /// was 0.08. The gamma pulse that failed before the Gaussian failed at
    /// 0.44 on the QUIET notes — a thump on every pianissimo — and the felt
    /// pulse at pianissimo is a symmetric hump measuring 0.045 there,
    /// under the Gaussian's 0.048. The asymmetry lives only where a real
    /// hammer's does.
    pub const JAZZ: HammerBase = HammerBase {
        mass: 1.0e-3,
        stiffness: 4.57e7,
        exponent: 2.2,
        hysteresis: 0.03,
    };
    /// The concert instrument's hammer: a tenth heavier on felt of the same
    /// exponent, 30% stiffer, fitted the same way against Piano 2's own
    /// signed-off tables — its mezzo partials land within 1.2 dB. The
    /// contact runs 2.0 ms at MIDI 8, a hair under the jazz hammer's, and
    /// the same 1.0 ms at 127: the loud end is the agraffe segment's
    /// bounce on either instrument, and a heavier head cannot shorten it.
    /// The first draft voiced this felt harder and more nonlinear (p 3.3)
    /// for "a wider sweep from whisper to bark"; measured, that sweep is
    /// 17 dB of extra brightness through the middle of the keyboard's
    /// dynamic range and nothing like the instrument, so the concert
    /// grand's difference is its mass, its plate, its wire and its ring,
    /// not a different felt.
    pub const CONCERT: HammerBase = HammerBase {
        mass: 1.1e-3,
        stiffness: 5.94e7,
        exponent: 2.2,
        hysteresis: 0.03,
    };

    /// The peak force this hammer would reach at `velocity` against a
    /// wall: all of its energy into the felt, `1/2 M v^2 = K d^(p+1) /
    /// (p+1)`, and `F = K d^p` at that compression. Against a string that
    /// gives way the true peak is lower, and with the rate term on the
    /// loading leg it can run a little over; this is the ceiling to within
    /// that, and it is what the piano seeds its level follower with before
    /// the hammer has said anything (see `PianoVoice::strike_full`).
    pub fn free_peak_force(&self, velocity: f32) -> f32 {
        let v = velocity.max(0.0);
        let p = self.exponent;
        let d = ((p + 1.0) * self.mass * v * v / (2.0 * self.stiffness)).powf(1.0 / (p + 1.0));
        self.stiffness * d.powf(p)
    }
}

/// The fortissimo hammer: 6 m/s, the top of the range Askenfelt and
/// Jansson measured on pianists (JASA 88, 1990: pianissimo near 0.5 m/s,
/// fortissimo 5 to 7).
pub const V_FF: f32 = 6.0;

/// The action's curve from MIDI velocity to hammer speed, as an exponent
/// on the MIDI fraction: 1.0 is proportional. See [`velocity_of_midi`].
pub const MAP_EXPONENT: f32 = 0.92;

/// Hammer velocity in metres per second for a MIDI velocity given as a
/// fraction of 127 (the unit `Keys::note_on` already speaks).
///
/// **This is the action's map, and the action is not modelled**, so the
/// map is fitted to the one thing already signed off: the instrument's
/// dynamic range. The top is physical, [`V_FF`]; below it the speed is
/// `V_FF * m^MAP_EXPONENT`, a power law a shade under proportional. The
/// first fit was linear with an offset, `0.5 + 5.5 m`, so that MIDI 1
/// still arrived at Askenfelt's pianissimo — and it took ten decibels out
/// of the instrument. A hammer's impulse is close to linear in its arrival
/// speed (`M v` on the way in and most of it again on the rebound at
/// pianissimo, half of it at fortissimo —
/// `energy_is_conserved_without_hysteresis` prints the figures), so the
/// peak level runs with the speed, and a hammer that cannot arrive slower
/// than 0.85 m/s at MIDI 8 gives 26 dB of peak from MIDI 8 to 127 where
/// the bench's velocity table has 35.7. MIDI is not a physical unit; the
/// exponent is the one that keeps every row of that table within 2 dB of
/// the signed-off one after a single gain (`string::tests::scan_hammers`
/// measured 0.90 to 0.95 passing and 1.0 not, on the mezzo partials), and
/// it puts MIDI 8 at 0.47 m/s — Askenfelt and Jansson's pianissimo, which
/// is a happier place for the softest note a keyboard sends than the
/// 0.38 a proportional map gives. `the_velocity_table_keeps_its_shape`
/// holds it.
pub fn velocity_of_midi(v: f32) -> f32 {
    V_FF * v.clamp(0.0, 1.0).powf(MAP_EXPONENT)
}

#[cfg(test)]
thread_local! {
    pub(super) static SOLVE_ITERS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    pub(super) static SOLVE_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How far below the point of first touch the hammer must have retreated
/// before it is out of the string's reach and the backcheck has it. A
/// string at x0 never swings more than a millimetre or two; three is out
/// of reach with margin, and it is a distance rather than a time because a
/// pianissimo hammer retreats slowly and a forte string swings wide.
const CHECK_DIST: f32 = 3.0e-3;

/// The longest the hammer is allowed to fly free between contacts before it
/// is declared caught regardless. A string that ran away from the hammer
/// comes back within one reflection time, a small fraction of a
/// millisecond; five milliseconds of nothing means nothing is coming.
const FLIGHT_LIMIT: f32 = 5.0e-3;

/// A contact longer than this is not physics. The slowest hammer the tests
/// throw (0.05 m/s, a tenth of any pianissimo) leaves in 4.6 ms on the
/// felt this file ships (`force_is_smooth_and_hammer_always_leaves` prints
/// the whole ladder); forty is a guard against a caller feeding a NaN or an
/// absurd scale, and it never fires in any test — that test asserts every
/// contact ended well inside it.
const CONTACT_LIMIT: f32 = 40.0e-3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// On the backcheck. Costs one comparison a sample.
    Idle,
    /// `strike` has been called; the next `tick` places the hammer on the
    /// string wherever the string is and lets it go.
    Armed,
    /// Felt compressed, force flowing.
    Contact,
    /// Off the string but still within its reach: retreating after a
    /// contact, or chasing a string that ran ahead of it.
    Flight,
}

/// A harder layer under a hammer's felt: past `depth` metres of compression
/// the hammer meets it, and the force gains `stiffness (x - depth)^exponent`
/// on top of the felt's own (`K x^p`), the whole scaled by the felt's
/// Hunt-Crossley rate term as the felt's is. A grand hammer is wool pressed
/// over a harder underfelt and wood: a soft blow never reaches it, a hard one
/// does, and the force pulse grows a sharp peak — which is how a hammer
/// brightens with velocity far more than one power law can (the modal
/// grand's reference: 22 to 40 dB of band excess, 1-4 kHz, from MIDI 41 to
/// 127 in the bass and tenor). [`FeltCore::NONE`] is the plain felt, and a
/// hammer that has never been given a core solves exactly as before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeltCore {
    /// Metres of compression before the core is met.
    pub depth: f32,
    /// Newtons per metre^exponent past the depth; zero is no core.
    pub stiffness: f32,
    pub exponent: f32,
}

impl FeltCore {
    pub const NONE: FeltCore = FeltCore { depth: 0.0, stiffness: 0.0, exponent: 1.0 };
}

/// One hammer. Owned by a voice, allocated once; `strike` and `tick` touch
/// nothing but this struct.
pub struct Hammer {
    base: HammerBase,
    state: State,
    /// Hammer position, metres, positive in the direction it travels.
    eta: f32,
    /// Hammer velocity, metres per second.
    eta_dot: f32,
    /// Last sample's felt compression, for the rate term.
    delta_prev: f32,
    /// Where the hammer first touched THIS strike; the retreat that ends
    /// the episode is measured from here.
    eta_touch: f32,
    /// Samples spent in the current contact.
    timer: u32,
    /// Seconds spent in the current flight.
    flight: f32,
    /// The first contact of the most recent strike, seconds. Zero until it
    /// has ended.
    contact: f32,
    /// Velocity waiting for the first tick after `strike`.
    launch: f32,
    /// The felt's harder layer, if this hammer was given one
    /// (`strike_with_core`); only the implicit steps read it.
    core: FeltCore,
    /// The midpoint step's last two forces in this contact, newest first:
    /// where its solve starts (`solve_midpoint`).
    forces: [f32; 2],
}

impl Hammer {
    pub fn new(base: HammerBase) -> Self {
        Hammer {
            base,
            state: State::Idle,
            eta: 0.0,
            eta_dot: 0.0,
            delta_prev: 0.0,
            eta_touch: 0.0,
            timer: 0,
            flight: 0.0,
            contact: 0.0,
            launch: 0.0,
            core: FeltCore::NONE,
            forces: [0.0; 2],
        }
    }

    /// Launch at `velocity` metres per second; see [`velocity_of_midi`].
    ///
    /// The strike moment IS the moment of first touch: on the next `tick`
    /// the hammer is placed on the string wherever the string then is, so
    /// the felt starts uncompressed and the force starts at zero however
    /// far a ringing string has wandered. Placing it at rest position
    /// instead would begin a restrike with a step of force — the pop the
    /// superposition law exists to forbid.
    ///
    /// A strike that lands while the hammer is still busy — mid-contact or
    /// in flight, which no real action can do but a sequencer can — keeps
    /// the hammer where it is and gives it the new velocity. Position is
    /// continuous, so the force is continuous; only its slope changes.
    pub fn strike(&mut self, velocity: f32) {
        let velocity = if velocity.is_finite() { velocity.max(0.0) } else { 0.0 };
        match self.state {
            State::Idle | State::Armed => {
                self.launch = velocity;
                self.state = State::Armed;
            }
            State::Contact | State::Flight => {
                self.eta_dot = velocity;
                self.flight = 0.0;
            }
        }
        self.contact = 0.0;
    }

    /// Whether the hammer is touching the string this sample.
    ///
    /// This and the two accessors below report on the mechanism rather than
    /// drive it, so the voice never asks: the hammer tests and `string.rs`'s
    /// contact-time sweeps are the readers.
    #[allow(dead_code)]
    pub fn in_contact(&self) -> bool {
        self.state == State::Contact
    }

    /// The hammer's velocity right now, metres per second, positive toward
    /// the string. After a contact this is the rebound: how hard the
    /// string threw it back.
    #[allow(dead_code)]
    pub fn velocity(&self) -> f32 {
        self.eta_dot
    }

    /// On the backcheck: nothing to tick, nothing that a `clear` would
    /// cut short. The opposite of "busy" — armed, in contact, or in flight
    /// within the string's reach all count as busy, because each of them
    /// still has a force to deliver.
    pub fn idle(&self) -> bool {
        self.state == State::Idle
    }

    /// `strike`, on a hammer that may have been re-voiced since it was
    /// built. The base is taken only while the hammer is on its check: a
    /// felt that changed stiffness mid-contact would be a step of force —
    /// the pop the superposition law forbids — and a strike arriving inside
    /// the previous one's contact is a sequencer's doing, not a hand's, so
    /// that strike keeps the felt it landed on and takes only the new
    /// velocity, exactly as `strike` does.
    pub fn strike_with(&mut self, base: HammerBase, velocity: f32) {
        if matches!(self.state, State::Idle | State::Armed) {
            self.base = base;
        }
        self.strike(velocity);
    }

    /// `strike_with`, the felt with a harder layer under it (`FeltCore`),
    /// taken as the base is: only while the hammer is on its check.
    pub(super) fn strike_with_core(&mut self, base: HammerBase, core: FeltCore, velocity: f32) {
        if matches!(self.state, State::Idle | State::Armed) {
            self.core = core;
        }
        self.strike_with(base, velocity);
    }

    /// One sample of the hammer. `string_disp` is the string's displacement
    /// under the hammer, in METRES, positive in the hammer's direction of
    /// travel — for the waveguide that is the two rails summed at the strike
    /// point, divided by the caller's units-per-metre scale. `dt` is one
    /// over the sample rate. Returns the force on the string this sample, in
    /// newtons, zero whenever the felt is not compressed.
    ///
    /// **What the caller does with the force.** It is a force, not a
    /// displacement: a point force on a string is a velocity jump of
    /// `F / (2 Z)` at the point, launched equally along both rails, so a
    /// displacement waveguide integrates `F / (2 Z) * dt` into each rail and
    /// a velocity waveguide adds `F / (2 Z)` directly. Pouring `F` in as if
    /// it were the old Gaussian's displacement would be the wrong quantity
    /// by one integration and the tap-pair comb would still hide it — for a
    /// while. A unison shares the force: three strings under one hammer
    /// each take a third of it, and their common displacement is what comes
    /// back in `string_disp`.
    ///
    /// The string's displacement is read BEFORE this sample's force is
    /// applied and the force lands a sample late, which is the explicit
    /// scheme the paper uses; at audio rates that is twenty microseconds
    /// against a contact of a thousand.
    #[allow(dead_code)] // The waveguide piano's step: no patch plays it; its tests and keysrig do.
    pub fn tick(&mut self, string_disp: f32, dt: f32) -> f32 {
        match self.state {
            State::Idle => return 0.0,
            State::Armed => {
                self.eta = string_disp;
                self.eta_dot = self.launch;
                self.eta_touch = string_disp;
                self.delta_prev = 0.0;
                self.timer = 0;
                self.flight = 0.0;
                self.state = State::Flight;
            }
            State::Contact | State::Flight => {}
        }

        let delta = self.eta - string_disp;
        let mut force = 0.0_f32;
        if delta > 0.0 {
            if self.state == State::Flight {
                self.state = State::Contact;
                self.timer = 0;
            }
            // Hunt-Crossley: the power law, scaled by the compression rate.
            // Clamped at zero because a felt unloading faster than its own
            // relaxation still cannot pull the string after it.
            let rate = (delta - self.delta_prev) / dt;
            let visc = (1.0 + self.base.hysteresis * rate).max(0.0);
            force = self.base.stiffness * delta.powf(self.base.exponent) * visc;
            if !force.is_finite() {
                force = 0.0;
            }
            self.timer += 1;
            if self.timer as f32 * dt > CONTACT_LIMIT {
                // Not physics. See `CONTACT_LIMIT`.
                self.clear();
                return 0.0;
            }
        } else if self.state == State::Contact {
            if self.contact == 0.0 {
                self.contact = self.timer as f32 * dt;
            }
            self.state = State::Flight;
            self.flight = 0.0;
        }

        // Semi-implicit Euler: velocity first, then position from the NEW
        // velocity. Symplectic, so the hammer's energy neither grows nor
        // leaks over a contact the way plain Euler's does, and stable for
        // any felt this file ships at any rate the engine runs.
        self.eta_dot -= force / self.base.mass * dt;
        self.eta += self.eta_dot * dt;
        self.delta_prev = delta;

        if self.state == State::Flight {
            self.flight += dt;
            let out_of_reach = self.eta_dot < 0.0 && self.eta < self.eta_touch - CHECK_DIST;
            if out_of_reach || self.flight > FLIGHT_LIMIT {
                self.state = State::Idle;
            }
        }
        if !self.eta.is_finite() {
            self.clear();
            return 0.0;
        }
        force
    }

    /// One sample of the hammer against a string that answers this
    /// sample's force within the sample: the IMPLICIT contact, for a string
    /// whose displacement under the felt is a linear function of the force,
    /// which the modal grand's is, exactly (`modal::Bank::render_contact`).
    ///
    /// `now` is the string's displacement under the hammer at this sample,
    /// metres, every force so far applied; `next_free` is where it will be
    /// one sample on if this sample's force is zero; `compliance` is how far
    /// one newton of this sample's force moves it from there, metres per
    /// newton. The hammer steps as `tick` steps it (velocity first, then
    /// position), so its compression one sample on is `free - a F`, with
    /// `free` that compression unforced and `a = dt^2 / M + compliance`;
    /// the force returned is the one the felt law gives AT that compression,
    /// `F = K x^p max(0, 1 + mu x')`, solved for F (`solve_felt`): backward
    /// Euler, first-order. The modal grand now steps with the second-order
    /// `tick_midpoint`; this one is kept exactly as it was for the other
    /// voices built on it.
    ///
    /// **Why not `tick`.** `tick` is explicit: the force is read off this
    /// sample's compression and acts a sample late. That is stable while the
    /// felt's stiffness times the compliance a sample adds stays under about
    /// two, and on the waveguide's C4 under its soft felt it does, by a wide
    /// margin. A treble course of the modal grand is a few grams of wire,
    /// struck by a hard felt, and there it did not: the force alternated
    /// sample to sample, a fortissimo A6 at the top of VOICING peaked at +16
    /// dBFS, and how far it overshot depended on the sample rate (+6.6 dBFS
    /// at 44.1 kHz against -3.9 at 96 for the loudest treble key at VOICING
    /// 0.75) — numerics, not felt. The implicit step has no such limit: its
    /// scalar equation has exactly one root in its bracket at any stiffness.
    /// Where `tick` was stable the two converge on the same pulse from
    /// either side as the rate rises (`grand::tests::
    /// the_implicit_contact_is_the_explicit_ones_physics_converged`), and the solve
    /// itself is held to the felt law (`implicit_tests::
    /// the_implicit_felt_solve_is_the_law`).
    #[allow(dead_code)] // The egrand voice's step; the grand steps with `tick_midpoint`.
    pub(super) fn tick_implicit(&mut self, now: f32, next_free: f32, compliance: f32, dt: f32) -> f32 {
        self.step_implicit(now, next_free, compliance, dt, false)
    }

    /// The same contact as `tick_implicit`, stepped by the implicit
    /// MIDPOINT rule: the sample's force is the felt law read halfway
    /// through the sample, at the mean of the compression now and one
    /// sample on, `F = K x_m^p max(0, 1 + mu x')` with `x' = (x1 - x0) / dt`;
    /// the hammer's velocity falls by `F dt / M` and its position moves by
    /// the mean of its two velocities. So the compression one sample on is
    /// `free - (dt^2 / 2M + compliance) F` (`free` that compression
    /// unforced), and the midpoint is half way to it: one scalar equation in
    /// F, solved by `solve_felt` as the backward step's is.
    ///
    /// **Why the grand moved to it.** Backward Euler reads the law at the
    /// END of the sample: first-order, so its error halves with each
    /// doubling of the rate, and in the top two octaves it was not small —
    /// C7's fortissimo n3 measured -39.5, -36.0 and -34.4 dB re n1 at 48,
    /// 96 and 192 kHz, a 48 kHz attack 6 dB darker than the physics it
    /// converged on (`grand::tests::print_rate_convergence`), so a treble
    /// voiced at 48 kHz played brighter at 96. The midpoint rule is
    /// second-order and conserves a spring's energy exactly; on the same
    /// keys the three rates agree to the figures in that test's doc.
    pub(super) fn tick_midpoint(&mut self, now: f32, next_free: f32, compliance: f32, dt: f32) -> f32 {
        self.step_implicit(now, next_free, compliance, dt, true)
    }

    /// `tick_implicit` (`midpoint` false, backward Euler) and
    /// `tick_midpoint` (true): one body, so the two can never drift apart
    /// in anything but the rule.
    fn step_implicit(&mut self, now: f32, next_free: f32, compliance: f32, dt: f32, midpoint: bool) -> f32 {
        match self.state {
            State::Idle => return 0.0,
            State::Armed => {
                self.eta = now;
                self.eta_dot = self.launch;
                self.eta_touch = now;
                self.delta_prev = 0.0;
                self.timer = 0;
                self.flight = 0.0;
                self.state = State::Flight;
            }
            State::Contact | State::Flight => {}
        }
        let mass = self.base.mass;
        // The compression now, and one sample on if no force acted; the
        // midpoint rule reads the law half way between them.
        let x_now = self.eta - now;
        let free = self.eta + self.eta_dot * dt - next_free;
        let (at, give, rate) = if midpoint {
            // At the midpoint the force's own give is half the end's, and
            // the compression's rate is twice its distance from now.
            (0.5 * (x_now + free), 0.5 * (0.5 * dt * dt / mass + compliance.max(0.0)), 2.0 * f64::from(self.base.hysteresis) / f64::from(dt))
        } else {
            (free, dt * dt / mass + compliance.max(0.0), f64::from(self.base.hysteresis) / f64::from(dt))
        };
        let mut force = 0.0_f32;
        if at > 0.0 {
            if self.state == State::Flight {
                self.state = State::Contact;
                self.timer = 0;
                self.forces = [0.0; 2];
            }
            // The backward step solves as it always has (`tick_implicit` is
            // another lane's, to the bit); the midpoint step by its own
            // solve, from the force's own trend (`solve_midpoint`).
            force = if midpoint {
                let guess = if self.timer >= 2 { (2.0 * self.forces[0] - self.forces[1]).max(0.0) } else { self.forces[0] };
                let f = self.solve_midpoint(x_now, at, give, rate, guess);
                self.forces = [f, self.forces[0]];
                f
            } else {
                self.solve_felt(x_now, x_now, at, give, rate, 1.0e-10)
            };
            self.timer += 1;
            if self.timer as f32 * dt > CONTACT_LIMIT {
                // Not physics. See `CONTACT_LIMIT`.
                self.clear();
                return 0.0;
            }
        } else if self.state == State::Contact {
            if self.contact == 0.0 {
                self.contact = self.timer as f32 * dt;
            }
            self.state = State::Flight;
            self.flight = 0.0;
        }
        if midpoint {
            let v0 = self.eta_dot;
            self.eta_dot -= force / mass * dt;
            self.eta += 0.5 * (v0 + self.eta_dot) * dt;
        } else {
            self.eta_dot -= force / mass * dt;
            self.eta += self.eta_dot * dt;
        }
        self.delta_prev = x_now;
        if self.state == State::Flight {
            self.flight += dt;
            let out_of_reach = self.eta_dot < 0.0 && self.eta < self.eta_touch - CHECK_DIST;
            if out_of_reach || self.flight > FLIGHT_LIMIT {
                self.state = State::Idle;
            }
        }
        if !self.eta.is_finite() || !force.is_finite() {
            self.clear();
            return 0.0;
        }
        force
    }

    /// The force that satisfies the felt law at the compression it leaves:
    /// `F = K x^p max(0, 1 + rate (x - x_now))` with `x = free - a F`
    /// (`rate` the hysteresis per metre the compression moves from
    /// `x_now`: `mu / dt` read at the end of a sample, `2 mu / dt` at its
    /// midpoint).
    ///
    /// The law says `K x^p w(x) = (free - x) / a`: the felt's force at the
    /// compression it ends the sample at equals what that compression
    /// leaves of the unforced one, over the compliance. Both sides are
    /// positive wherever the root can be (`0 < x < free`, `w > 0`), and it
    /// is solved in their logarithms, in the LOGIT of the compression,
    /// `y = ln x - ln(free - x)`:
    ///
    /// `psi(y) = ln K a + (p - 1) ln free + p ln s(y) - ln s(-y) + ln w = 0`,
    ///
    /// `s` the logistic function, so `x = free s(y)` and `F = free s(-y) / a`
    /// with no cancellation at either end. `psi` rises monotonically (its
    /// slope is at least min(1, p)), so the root is unique, and it is nearly
    /// a straight line at BOTH ends: where the felt is stiff against the
    /// string (the compression a small part of what the hammer would press
    /// unopposed, a treble fortissimo) it goes as `p y`, and where it is
    /// soft (the string barely yields, a bass pianissimo) as `y`. Newton
    /// takes either in a step or two. The first two forms tried each had
    /// one end as a power law, on which Newton crawls at `1 - 1/p` a step:
    /// five to eleven iterations a sample in the compression, and in its
    /// logarithm the soft end could stall at the bracket's edge. Here, from
    /// the last sample's compression, two to four; a step that leaves the
    /// bracket is a bisection instead, and at most forty are taken. In
    /// `f64`, once a sample, while the felt touches.
    fn solve_felt(&self, x_now: f32, guess: f32, free: f32, a: f32, rate: f64, step_tol: f64) -> f32 {
        #[cfg(test)]
        SOLVE_CALLS.with(|c| c.set(c.get() + 1));
        if self.core.stiffness > 0.0 && free > self.core.depth {
            return self.solve_felt_core(x_now, guess, free, a, rate, step_tol);
        }
        let p = f64::from(self.base.exponent);
        let (free, a, x_now) = (f64::from(free), f64::from(a), f64::from(x_now));
        // Under `x_w` the rate term would make the felt pull: no force there.
        let x_w = if rate > 0.0 { x_now - 1.0 / rate } else { f64::NEG_INFINITY };
        if x_w >= free {
            // Unloading faster than the felt relaxes: no force at any F.
            return 0.0;
        }
        let c = (f64::from(self.base.stiffness) * a).ln() + (p - 1.0) * free.ln();
        // The bracket: sixty nepers either way is a compression, or a force,
        // of e^-60 of the unforced one — nothing — and the rate term's floor.
        let mut lo = if x_w > 0.0 { (x_w / (free - x_w)).ln().max(-60.0) } else { -60.0 };
        let mut hi = 60.0_f64;
        let guess = f64::from(guess);
        let mut y = if guess > x_w.max(0.0) && guess < free { (guess / (free - guess)).ln() } else { 0.0 };
        if !(y > lo && y < hi) {
            y = 0.5 * (lo + hi);
        }
        for _ in 0..40 {
            #[cfg(test)]
            SOLVE_ITERS.with(|c| c.set(c.get() + 1));
            // ln s(y) and ln s(-y) as softplus terms, sharing one exponential.
            let e = (-y.abs()).exp();
            let l1p = e.ln_1p();
            let ln_s_pos = -((-y).max(0.0) + l1p);
            let ln_s_neg = -(y.max(0.0) + l1p);
            let sig = if y >= 0.0 { 1.0 / (1.0 + e) } else { e / (1.0 + e) };
            let w = 1.0 + rate * (free * sig - x_now);
            let next = if w <= 0.0 {
                lo = y;
                0.5 * (lo + hi)
            } else {
                let psi = c + p * ln_s_pos - ln_s_neg + w.ln();
                if psi.abs() <= 1.0e-9 {
                    break;
                }
                let slope = p * (1.0 - sig) + sig + rate * free * sig * (1.0 - sig) / w;
                if psi > 0.0 {
                    hi = y;
                } else {
                    lo = y;
                }
                let n = y - psi / slope;
                if n > lo && n < hi {
                    n
                } else {
                    0.5 * (lo + hi)
                }
            };
            if (next - y).abs() <= step_tol {
                y = next;
                break;
            }
            y = next;
        }
        // F = free s(-y) / a.
        let e = (-y.abs()).exp();
        let s_neg = if y >= 0.0 { e / (1.0 + e) } else { 1.0 / (1.0 + e) };
        (free * s_neg / a) as f32
    }

    /// The midpoint step's force: `F = w(x) (K x^p + Kc (x - d)_+^q)` at the
    /// midpoint compression `x = at - give F`, with `w = 1 + rate (x - x_now)`
    /// (`tick_midpoint`). Solved for the give `u = give F` by Newton in
    /// plain units inside the bracket `[0, at - max(0, x_w)]` where the law
    /// holds — `h(u) = w L - u / give` falls through zero exactly once there
    /// — with a bisection wherever a step would leave it; stopped on a step
    /// under 1e-7 of `u` (the worst case then 1.1e-7 off the law, a 32-bit
    /// force's last bit; at a millionth it was 9.2e-7). In `f64`, once a
    /// sample, while the felt touches.
    ///
    /// **Why not `solve_felt`'s logit.** The logit of the compression is
    /// what lets the backward step's one solve span sixty nepers of force
    /// from a cold start, and it costs seven transcendentals an iteration,
    /// three to four iterations a sample: the profile of a fortissimo
    /// cluster put a fifth of all its time there. A contact's force is a
    /// smooth pulse a hundred samples long, so the line through the last
    /// two forces lands within a part in a thousand of the next one, and
    /// Newton in plain units finishes from there in one or two steps at two
    /// transcendentals each, four with the core (`x^p` and `(x - d)^q` as
    /// exponentials of logarithms). The same law, held to it within a part
    /// in a million (`implicit_tests::the_midpoint_solve_is_the_law`).
    /// Measured through the grand (`grand::tests::print_solve_iterations`):
    /// printed there.
    fn solve_midpoint(&self, x_now: f32, at: f32, give: f32, rate: f64, guess: f32) -> f32 {
        #[cfg(test)]
        SOLVE_CALLS.with(|c| c.set(c.get() + 1));
        let (k, p) = (f64::from(self.base.stiffness), f64::from(self.base.exponent));
        let (kc, d, q) = if self.core.stiffness > 0.0 {
            (f64::from(self.core.stiffness), f64::from(self.core.depth), f64::from(self.core.exponent))
        } else {
            (0.0, f64::INFINITY, 1.0)
        };
        let (at, give, x_now) = (f64::from(at), f64::from(give), f64::from(x_now));
        // NaN-proof: a give that is not a positive number is no contact.
        if give.is_nan() || give <= 0.0 {
            return 0.0;
        }
        // Under `x_w` the rate term would make the felt pull: no force there.
        let x_w = if rate > 0.0 { x_now - 1.0 / rate } else { f64::NEG_INFINITY };
        let (mut lo, mut hi) = (0.0_f64, at - x_w.max(0.0));
        if hi.is_nan() || hi <= 0.0 {
            return 0.0;
        }
        // From the guess, or from no force at all: `h(0) > 0` there, so a
        // first step from nothing heads into the bracket.
        let mut u = (give * f64::from(guess)).clamp(0.0, hi);
        if u.is_nan() || u >= hi {
            u = 0.5 * (lo + hi);
        }
        for _ in 0..60 {
            #[cfg(test)]
            SOLVE_ITERS.with(|c| c.set(c.get() + 1));
            let x = at - u;
            let w = 1.0 + rate * (x - x_now);
            let next = if x <= 0.0 || w <= 0.0 || x.is_nan() || w.is_nan() {
                // Past the bracket's floor (only rounding gets here): the
                // law has no force, so the root is a smaller give.
                hi = u;
                0.5 * (lo + hi)
            } else {
                let xp = (p * x.ln()).exp();
                let over = x - d;
                let (oq, doq) = if over > 0.0 {
                    let e = (q * over.ln()).exp();
                    (e, q * e / over)
                } else {
                    (0.0, 0.0)
                };
                let law = k * xp + kc * oq;
                let h = w * law - u / give;
                if h > 0.0 {
                    lo = u;
                } else {
                    hi = u;
                }
                let slope = rate * law + w * (p * k * xp / x + kc * doq) + 1.0 / give;
                let n = u + h / slope;
                if n > lo && n < hi {
                    n
                } else {
                    0.5 * (lo + hi)
                }
            };
            if (next - u).abs() <= 1.0e-7 * next {
                u = next;
                break;
            }
            u = next;
        }
        (u / give) as f32
    }

    /// `solve_felt` with the core in the law: `F = w(x) (K x^p + Kc (x -
    /// d)_+^q)`, `x = free - a F`. The same logit `y` of the compression and
    /// the same bracket; the law is no longer one power, so `psi` is written
    /// out whole, `psi(y) = ln L(x) + ln w - ln(free s(-y) / a)`, with
    /// slope `(L'(x) / L(x) + rate / w) free s(y) s(-y) + s(y)`, still
    /// rising, so the root is still unique. Newton from the last sample's
    /// compression, a bisection where a step leaves the bracket, at most
    /// sixty steps; in `f64`. Taken only while the unforced compression
    /// reaches past the core's depth, so a blow that never meets the core
    /// is solved by `solve_felt` itself.
    fn solve_felt_core(&self, x_now: f32, guess: f32, free: f32, a: f32, rate: f64, step_tol: f64) -> f32 {
        let (k, p) = (f64::from(self.base.stiffness), f64::from(self.base.exponent));
        let (kc, d, q) = (f64::from(self.core.stiffness), f64::from(self.core.depth), f64::from(self.core.exponent));
        let (free, a, x_now) = (f64::from(free), f64::from(a), f64::from(x_now));
        let x_w = if rate > 0.0 { x_now - 1.0 / rate } else { f64::NEG_INFINITY };
        if x_w >= free {
            return 0.0;
        }
        let mut lo = if x_w > 0.0 { (x_w / (free - x_w)).ln().max(-60.0) } else { -60.0 };
        let mut hi = 60.0_f64;
        let guess = f64::from(guess);
        let mut y = if guess > x_w.max(0.0) && guess < free { (guess / (free - guess)).ln() } else { 0.0 };
        if !(y > lo && y < hi) {
            y = 0.5 * (lo + hi);
        }
        // The logarithms the loop shares: every power below is one
        // exponential of a logarithm it already has (`ln x` from the logit's
        // softplus terms, as `solve_felt` reads them), not a `powf` — four of
        // those an iteration were half the contact's cost in a slammed
        // cluster.
        let (ln_free, ln_a) = (free.ln(), a.ln());
        for _ in 0..60 {
            #[cfg(test)]
            SOLVE_ITERS.with(|c| c.set(c.get() + 1));
            let e = (-y.abs()).exp();
            let l1p = e.ln_1p();
            let sig = if y >= 0.0 { 1.0 / (1.0 + e) } else { e / (1.0 + e) };
            let x = free * sig;
            let w = 1.0 + rate * (x - x_now);
            let next = if w <= 0.0 || x <= 0.0 {
                lo = y;
                0.5 * (lo + hi)
            } else {
                // x^p and (x - d)^q, and their derivatives as each over its base.
                let xp = (p * (ln_free - ((-y).max(0.0) + l1p))).exp();
                let over = (x - d).max(0.0);
                let oq = if over > 0.0 { (q * over.ln()).exp() } else { 0.0 };
                let law = k * xp + kc * oq;
                let dlaw = p * k * xp / x + if over > 0.0 { q * kc * oq / over } else { 0.0 };
                // ln F = ln(free s(-y) / a).
                let ln_force = ln_free - ln_a - (y.max(0.0) + l1p);
                let psi = law.ln() + w.ln() - ln_force;
                if psi.abs() <= 1.0e-9 {
                    break;
                }
                let slope = (dlaw / law + rate / w) * free * sig * (1.0 - sig) + sig;
                if psi > 0.0 {
                    hi = y;
                } else {
                    lo = y;
                }
                let n = y - psi / slope;
                if n > lo && n < hi {
                    n
                } else {
                    0.5 * (lo + hi)
                }
            };
            if (next - y).abs() <= step_tol {
                y = next;
                break;
            }
            y = next;
        }
        let e = (-y.abs()).exp();
        let s_neg = if y >= 0.0 { e / (1.0 + e) } else { 1.0 / (1.0 + e) };
        (free * s_neg / a) as f32
    }

    /// How long the most recent strike's first contact lasted, in seconds;
    /// zero while it is still going on or if there has been none.
    #[allow(dead_code)]
    pub fn contact_secs(&self) -> f32 {
        self.contact
    }

    /// Back on the check, nothing remembered. For a voice being reset, never
    /// for a note being played: the state a `strike` finds is the state it
    /// lands on.
    pub fn clear(&mut self) {
        self.state = State::Idle;
        self.eta = 0.0;
        self.eta_dot = 0.0;
        self.delta_prev = 0.0;
        self.eta_touch = 0.0;
        self.timer = 0;
        self.flight = 0.0;
        self.contact = 0.0;
        self.launch = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// The string the hammer is fitted against: C4, three unison strings.
    const F0: f32 = 261.6;
    /// Speaking length of a C4 string on a mid-sized grand, metres.
    const LENGTH: f32 = 0.62;
    /// Linear density of 1.08 mm plain steel wire, kg/m; with the tension
    /// that pitches it (`T = mu c^2`, 757 N) the impedance `sqrt(T mu)` is
    /// 2.33 N s/m per string. The hammer sees the three in parallel.
    const MU: f32 = 7.2e-3;
    const UNISON: f32 = 3.0;

    /// A lossless ideal string with the hammer at one eighth of its length:
    /// a rigid agraffe on the near side, so a wave sent that way returns
    /// inverted after `2 x0 / c`, and a far side long enough that nothing
    /// comes back from it inside a test. Displacement waves; a force couples
    /// as a velocity jump `F / (2 Z)` on both rails.
    ///
    /// Keeps every sample it has ever carried, because the energy of a
    /// travelling wave on an ideal string is `Z` times the integral of its
    /// velocity squared and the two directions' energies add exactly — so
    /// the string's total energy is a sum over three histories: what is in
    /// flight on the near side, everything that has left toward the bridge,
    /// and whatever the test sent in from the bridge side that has yet to
    /// arrive.
    struct IdealString {
        rate: f32,
        z: f32,
        /// Samples from the hammer to the agraffe.
        n0: usize,
        /// Left-going wave at x0 (toward the agraffe), one entry per sample.
        left: Vec<f32>,
        /// Right-going wave at x0 (toward the bridge), one entry per sample.
        right: Vec<f32>,
        /// What arrives from the bridge side, per sample, for all time.
        incoming: Vec<f32>,
        /// Displacement generated at x0 by the force so far.
        g: f32,
        /// Sample index.
        n: usize,
    }

    impl IdealString {
        fn new(rate: f32, incoming: Vec<f32>) -> Self {
            let c = 2.0 * LENGTH * F0;
            let x0 = LENGTH / 8.0;
            let n0 = (x0 / c * rate).round() as usize;
            let z = MU * c * UNISON;
            IdealString {
                rate,
                z,
                n0,
                left: Vec::new(),
                right: Vec::new(),
                incoming,
                g: 0.0,
                n: 0,
            }
        }

        fn at_rest(rate: f32) -> Self {
            Self::new(rate, Vec::new())
        }

        fn incoming_now(&self) -> f32 {
            self.incoming.get(self.n).copied().unwrap_or(0.0)
        }

        /// The reflection arriving from the agraffe this sample.
        fn reflected(&self) -> f32 {
            let k = 2 * self.n0;
            if self.n >= k {
                -self.left[self.n - k]
            } else {
                0.0
            }
        }

        /// Displacement under the hammer, before this sample's force.
        fn displacement(&self) -> f32 {
            self.reflected() + self.incoming_now() + self.g
        }

        /// Apply this sample's force and advance.
        fn apply(&mut self, force: f32) {
            self.g += force / (2.0 * self.z) / self.rate;
            let l = self.incoming_now() + self.g;
            let r = self.reflected() + self.g;
            self.left.push(l);
            self.right.push(r);
            self.n += 1;
        }

        /// Total energy on the string, joules.
        fn energy(&self) -> f64 {
            let z = self.z as f64;
            let dt = 1.0 / self.rate as f64;
            let sum = |xs: &[f32]| -> f64 {
                xs.windows(2)
                    .map(|w| {
                        let d = w[1] as f64 - w[0] as f64;
                        d * d
                    })
                    .sum::<f64>()
            };
            let near_from = self.left.len().saturating_sub(2 * self.n0 + 1);
            let near = sum(&self.left[near_from..]);
            let far = sum(&self.right);
            let pending = if self.n < self.incoming.len() {
                sum(&self.incoming[self.n.saturating_sub(1)..])
            } else {
                0.0
            };
            z / dt * (near + far + pending)
        }
    }

    /// Strike a string at rest and run until the hammer is caught. Returns
    /// the force history and the hammer.
    fn strike_rest(base: HammerBase, v: f32, rate: f32) -> (Vec<f32>, Hammer, IdealString) {
        strike_into(base, v, rate, IdealString::at_rest(rate))
    }

    fn strike_into(base: HammerBase, v: f32, rate: f32, mut s: IdealString) -> (Vec<f32>, Hammer, IdealString) {
        let mut h = Hammer::new(base);
        h.strike(v);
        let dt = 1.0 / rate;
        let mut forces = Vec::new();
        for _ in 0..(0.1 * rate) as usize {
            let f = h.tick(s.displacement(), dt);
            s.apply(f);
            forces.push(f);
            if h.state == State::Idle {
                break;
            }
        }
        (forces, h, s)
    }

    fn contact_ms(base: HammerBase, midi: u8, rate: f32) -> f32 {
        let (_, h, _) = strike_rest(base, velocity_of_midi(f32::from(midi) / 127.0), rate);
        h.contact_secs() * 1e3
    }

    /// Acceptance 1: the contact time is an OUTPUT. It falls monotonically
    /// with velocity and lands on the two figures the instrument was signed
    /// off with, from a file that contains no width curve.
    #[test]
    fn contact_duration_comes_out_of_the_model() {
        let midis: Vec<u8> = (8..=127).step_by(7).chain(std::iter::once(127)).collect();
        let mut prev = f32::INFINITY;
        let mut table = String::new();
        for &m in &midis {
            let ms = contact_ms(HammerBase::JAZZ, m, RATE);
            table.push_str(&format!("  MIDI {m:3} -> {ms:.3} ms\n"));
            assert!(ms <= prev, "contact grew with velocity at MIDI {m}: {ms} > {prev}\n{table}");
            prev = ms;
        }
        let soft = contact_ms(HammerBase::JAZZ, 8, RATE);
        let loud = contact_ms(HammerBase::JAZZ, 127, RATE);
        println!("JAZZ contact durations at 48 kHz:\n{table}");
        assert!((soft - 2.1).abs() <= 0.15 * 2.1, "MIDI 8 contact {soft} ms is not near 2.1");
        assert!((loud - 1.0).abs() <= 0.15 * 1.0, "MIDI 127 contact {loud} ms is not near 1.0");
    }

    /// The concert hammer is a different hammer, not a broken one: same
    /// monotone sweep, a hair shorter at both ends.
    #[test]
    fn concert_hammer_sweeps_too() {
        let mut prev = f32::INFINITY;
        for m in (8..=127).step_by(7).chain(std::iter::once(127)) {
            let ms = contact_ms(HammerBase::CONCERT, m, RATE);
            assert!(ms <= prev, "CONCERT contact grew at MIDI {m}: {ms} > {prev}");
            prev = ms;
        }
        let soft = contact_ms(HammerBase::CONCERT, 8, RATE);
        let loud = contact_ms(HammerBase::CONCERT, 127, RATE);
        assert!((1.6..=2.4).contains(&soft), "CONCERT MIDI 8: {soft} ms");
        assert!((0.8..=1.2).contains(&loud), "CONCERT MIDI 127: {loud} ms");
        assert!(soft < contact_ms(HammerBase::JAZZ, 8, RATE));
    }

    /// The ODE is integrated at whatever rate the engine runs; the contact
    /// must be a property of the hammer, not of the clock.
    #[test]
    fn contact_duration_is_rate_independent() {
        for m in [8u8, 64, 127] {
            let at48 = contact_ms(HammerBase::JAZZ, m, 48_000.0);
            for rate in [44_100.0, 96_000.0, 192_000.0] {
                let ms = contact_ms(HammerBase::JAZZ, m, rate);
                assert!(
                    (ms - at48).abs() <= 0.04 * at48,
                    "MIDI {m}: {ms} ms at {rate} vs {at48} ms at 48k"
                );
            }
        }
    }

    /// Acceptance 3 (the second half): a hammer meeting a string already
    /// moving toward it is thrown back with MORE energy than it brought, and
    /// the string is left with less than it had. This is what
    /// `Mode::seed_within`'s `LANDING_FLOOR` fakes in the modal banks: a
    /// restrike against motion can subtract. Wave B: cite this test.
    #[test]
    fn hammer_meeting_a_string_moving_toward_it_takes_energy_out() {
        let v = velocity_of_midi(8.0 / 127.0);
        let base = HammerBase::JAZZ;
        // The string under the hammer runs toward it at 2 m/s for four
        // milliseconds — the strike point of a fortissimo note, restruck
        // pianissimo — as a wave arriving from the bridge side, its
        // displacement ramping negative (toward the hammer).
        let u = 2.0_f32;
        let ramp = (0.004 * RATE) as usize;
        let incoming: Vec<f32> = (0..ramp + 200)
            .map(|k| -u * (k.min(ramp) as f32) / RATE)
            .collect();
        let moving = IdealString::new(RATE, incoming);
        let before = moving.energy();
        let (_, h, after) = strike_into(base, v, RATE, moving);
        let ke_in = 0.5 * base.mass as f64 * (v as f64).powi(2);
        let ke_out = 0.5 * base.mass as f64 * (h.eta_dot as f64).powi(2);
        let (_, _, rest) = strike_rest(base, v, RATE);
        println!(
            "string energy before {before:.5} J, after {:.5} J (rest strike leaves {:.5} J); hammer in {ke_in:.5} J, out {ke_out:.5} J",
            after.energy(),
            rest.energy()
        );
        assert!(h.eta_dot < 0.0, "hammer never left: {}", h.eta_dot);
        assert!(ke_out > ke_in, "hammer was not thrown back harder than it came: {ke_out} <= {ke_in}");
        assert!(after.energy() < before, "string gained energy from a hammer it was pushing on");
        assert!(after.energy() < before + rest.energy(), "no less than superposition");
    }

    /// Bookkeeping check on the proxy and the scheme: with a lossless felt,
    /// what the hammer loses the string gains.
    #[test]
    fn energy_is_conserved_without_hysteresis() {
        let base = HammerBase { hysteresis: 0.0, ..HammerBase::JAZZ };
        for v in [0.5_f32, 2.0, 6.0] {
            let (_, h, s) = strike_rest(base, v, RATE);
            let ke_in = 0.5 * base.mass as f64 * (v as f64).powi(2);
            let ke_out = 0.5 * base.mass as f64 * (h.eta_dot as f64).powi(2);
            let gained = s.energy();
            let err = (ke_in - ke_out - gained) / ke_in;
            println!("v {v}: in {ke_in:.5} out {ke_out:.5} string {gained:.5} err {err:+.4}");
            assert!(err.abs() < 0.03, "energy error {err} at {v} m/s");
        }
        // And with the felt hysteresis on, the books close only with a loss.
        let (_, h, s) = strike_rest(HammerBase::JAZZ, 6.0, RATE);
        let ke_in = 0.5 * HammerBase::JAZZ.mass as f64 * 36.0;
        let ke_out = 0.5 * HammerBase::JAZZ.mass as f64 * (h.eta_dot as f64).powi(2);
        let lost = (ke_in - ke_out - s.energy()) / ke_in;
        println!("hysteresis eats {:.1}% at 6 m/s", lost * 100.0);
        assert!(lost > 0.03 && lost < 0.5, "hysteresis loss {lost} is not a felt");
    }

    /// Acceptance 3 (first half): the force is smooth enough for the
    /// waveguide, and the hammer leaves — at every velocity, including two
    /// no pianist can produce.
    #[test]
    fn force_is_smooth_and_hammer_always_leaves() {
        for v in [0.05_f32, 0.2, 0.5, 1.0, 2.0, 4.0, 6.0, 9.0, 12.0] {
            let (forces, h, _) = strike_rest(HammerBase::JAZZ, v, RATE);
            let peak = forces.iter().cloned().fold(0.0, f32::max);
            assert!(peak > 0.0 && peak.is_finite(), "no force at {v} m/s");
            let step = forces
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0, f32::max);
            assert!(
                step <= 0.25 * peak,
                "force stepped {step} N against a peak of {peak} N at {v} m/s"
            );
            assert!(forces.iter().all(|f| f.is_finite() && *f >= 0.0));
            assert!(!h.in_contact(), "stuck in contact at {v} m/s");
            assert!(h.state == State::Idle, "still airborne at {v} m/s");
            let secs = h.contact_secs();
            println!("{v} m/s: contact {:.2} ms, peak {peak:.2} N", secs * 1e3);
            assert!(secs > 0.0 && secs < 0.010, "contact {secs} s at {v} m/s");
            // Released by physics, not by the guard.
            assert!(secs < CONTACT_LIMIT * 0.5);
            // No step off the end: the last force before release is a
            // whisper of the peak.
            let last = forces.iter().rev().find(|f| **f > 0.0).copied().unwrap_or(0.0);
            assert!(last < 0.05 * peak, "released with {last} N of {peak} N at {v} m/s");
        }
    }

    /// Acceptance 4: Hunt-Crossley hysteresis. At the same compression the
    /// felt pushes harder while loading than while unloading — and with the
    /// rate term off, exactly as hard.
    #[test]
    fn loading_force_exceeds_unloading_force_at_equal_compression() {
        fn loop_gap(base: HammerBase) -> (f32, f32) {
            let mut h = Hammer::new(base);
            h.strike(6.0);
            let mut s = IdealString::at_rest(RATE);
            let dt = 1.0 / RATE;
            // (compression, force) per sample, from the hammer's own state.
            let mut trace: Vec<(f32, f32)> = Vec::new();
            for _ in 0..(0.01 * RATE) as usize {
                let y = s.displacement();
                let delta = h.eta - y;
                let f = h.tick(y, dt);
                s.apply(f);
                if f > 0.0 {
                    trace.push((delta, f));
                }
                if h.state == State::Idle {
                    break;
                }
            }
            let (peak_i, _) = trace
                .iter()
                .enumerate()
                .max_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap())
                .unwrap();
            let target = 0.6 * trace[peak_i].0;
            // Interpolate the force at `target` on each leg.
            let interp = |leg: &[(f32, f32)]| -> f32 {
                for w in leg.windows(2) {
                    let (d0, f0) = w[0];
                    let (d1, f1) = w[1];
                    if (d0 <= target && target <= d1) || (d1 <= target && target <= d0) {
                        let t = (target - d0) / (d1 - d0);
                        return f0 + t * (f1 - f0);
                    }
                }
                panic!("target compression not crossed");
            };
            (interp(&trace[..=peak_i]), interp(&trace[peak_i..]))
        }
        let (load, unload) = loop_gap(HammerBase::JAZZ);
        println!("at 60% of peak compression: loading {load:.2} N, unloading {unload:.2} N");
        assert!(load > 1.05 * unload, "no hysteresis: load {load} vs unload {unload}");
        let (load0, unload0) = loop_gap(HammerBase { hysteresis: 0.0, ..HammerBase::JAZZ });
        assert!(
            (load0 - unload0).abs() < 0.02 * load0,
            "lossless felt is not single-valued: {load0} vs {unload0}"
        );
    }

    /// The pulse a fortissimo hammer makes is not a Gaussian: it rises
    /// fast, peaks early, and its tail carries the agraffe reflection as a
    /// shoulder. Pin the shape so a re-voicing that changes it is a choice.
    #[test]
    fn loud_pulse_peaks_early() {
        let (forces, h, _) = strike_rest(HammerBase::JAZZ, 6.0, RATE);
        let n = (h.contact_secs() * RATE) as usize;
        let (peak_i, _) = forces
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!(peak_i < n / 2, "ff peak at sample {peak_i} of {n}");
        let (forces, h, _) = strike_rest(HammerBase::JAZZ, 0.85, RATE);
        let n = (h.contact_secs() * RATE) as usize;
        let (peak_i, _) = forces
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        // Pianissimo is nearly symmetric: the felt is the whole spring.
        assert!((peak_i as f32 / n as f32 - 0.5).abs() < 0.2, "pp peak at {peak_i} of {n}");
    }

    /// A restrike ADDS. Striking again mid-contact keeps the force
    /// continuous, and striking a caught hammer starts a clean contact
    /// wherever the string is.
    #[test]
    fn restrike_is_continuous() {
        let mut h = Hammer::new(HammerBase::JAZZ);
        let mut s = IdealString::at_rest(RATE);
        let dt = 1.0 / RATE;
        h.strike(2.0);
        let mut prev = 0.0;
        let mut worst = 0.0_f32;
        let mut peak = 0.0_f32;
        for i in 0..(0.02 * RATE) as usize {
            if i == 20 {
                h.strike(6.0);
            }
            let f = h.tick(s.displacement(), dt);
            s.apply(f);
            worst = worst.max((f - prev).abs());
            peak = peak.max(f);
            prev = f;
        }
        assert!(worst <= 0.25 * peak, "restrike stepped {worst} of {peak}");
        assert!(h.state == State::Idle);
        // A caught hammer, struck again while the string is displaced,
        // starts from zero force.
        let mut s2 = IdealString::new(RATE, vec![1.0e-3; 4000]);
        h.strike(3.0);
        let first = h.tick(s2.displacement(), dt);
        s2.apply(first);
        assert_eq!(first, 0.0);
        assert!(h.tick(s2.displacement(), dt) >= 0.0);
    }

    #[test]
    fn midi_velocity_map_is_proportional_and_bounded() {
        assert_eq!(velocity_of_midi(0.0), 0.0);
        assert_eq!(velocity_of_midi(1.0), V_FF);
        assert_eq!(velocity_of_midi(2.0), V_FF);
        assert!((velocity_of_midi(0.5) - V_FF * 0.5_f32.powf(MAP_EXPONENT)).abs() < 1e-6);
        // MIDI 8 is under Askenfelt's pianissimo, and above nothing.
        let soft = velocity_of_midi(8.0 / 127.0);
        assert!(soft > 0.1 && soft < 0.5, "MIDI 8 arrives at {soft} m/s");
    }
}

#[cfg(test)]
mod implicit_tests {
    use super::*;

    /// **The implicit step's felt solve IS the felt law.** Two thousand
    /// sample steps drawn across every regime a strike passes through —
    /// felt exponents 2 to 3.5, stiffness over three decades, compressions
    /// from a micrometre to a millimetre, the hammer still approaching
    /// (`x_now < 0`) or already deep in the felt, compliances from a
    /// stiff string's to a soft one's — against a 200-step bisection of the
    /// same equation in the force: within a part in a million (the inputs
    /// are `f32`), and never more than twelve iterations. Measured: 1e-7
    /// at worst, four to six iterations a step.
    /// **`tick_implicit` is the modal lane's, to the bit.** The backward
    /// step stays the egrand lane's to build on while the grand steps by
    /// the midpoint rule: a scripted fortissimo and pianissimo against one
    /// modal string, every force hashed, against the hash the modal lane's
    /// own hammer.rs (base cc701c73) gives for the same script.
    #[test]
    fn tick_implicit_is_unchanged_to_the_bit() {
        fn fnv(h: &mut u64, x: f32) {
            for b in x.to_bits().to_le_bytes() {
                *h ^= u64::from(b);
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for (speed, k, p) in [(4.0_f32, 2.0e9_f32, 2.5_f32), (0.4, 4.0e5, 2.2), (6.0, 3.0e12, 3.3)] {
            let mut h = Hammer::new(HammerBase { mass: 0.008, stiffness: k, exponent: p, hysteresis: 0.3 });
            h.strike(speed);
            let dt = 1.0 / 48_000.0_f32;
            let (w2, ms) = ((std::f32::consts::TAU * 262.0 * dt).powi(2), 0.02_f32);
            let (mut y0, mut y1) = (0.0_f32, 0.0_f32);
            for _ in 0..400 {
                let next_free = 2.0 * y1 - y0 - w2 * y1;
                let comp = dt * dt / ms;
                let f = h.tick_implicit(y1, next_free, comp, dt);
                fnv(&mut hash, f);
                y0 = y1;
                y1 = next_free + comp * f;
            }
        }
        println!("tick_implicit script hash {hash}");
        assert_eq!(hash, 2_133_509_257_800_458_177_u64, "tick_implicit moved");
    }

    #[test]
    fn the_implicit_felt_solve_is_the_law() {
        let mut seed = 0x9e37_79b9_u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let (mut worst, mut most) = (0.0_f64, 0u64);
        for _ in 0..2000 {
            let p = (2.0 + 1.5 * rnd()) as f32;
            let k = (10.0_f64.powf(1.0 + 2.0 * rnd()) / 1.0e-3_f64.powf(f64::from(p))) as f32;
            let h = Hammer::new(HammerBase { mass: 0.005, stiffness: k, exponent: p, hysteresis: 0.03 });
            let dt = 1.0 / 48_000.0_f32;
            let free = 10.0_f64.powf(-6.0 + 3.0 * rnd()) as f32;
            let x_now = (f64::from(free) * (rnd() * 1.4 - 0.4)) as f32;
            let a = 10.0_f64.powf(-9.0 + 3.0 * rnd()) as f32;
            SOLVE_ITERS.with(|c| c.set(0));
            let f = f64::from(h.solve_felt(x_now, x_now, free, a, 0.03 / f64::from(dt), 1.0e-10));
            most = most.max(SOLVE_ITERS.with(|c| c.get()));
            let (free, a, x_now, dt, k, p) = (f64::from(free), f64::from(a), f64::from(x_now), f64::from(dt), f64::from(k), f64::from(p));
            let g = |ff: f64| {
                let x = free - a * ff;
                let w = (1.0 + 0.03 / dt * (x - x_now)).max(0.0);
                ff - k * x.max(0.0).powf(p) * w
            };
            let (mut lo, mut hi) = (0.0_f64, free / a);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if g(mid) > 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let want = 0.5 * (lo + hi);
            worst = worst.max((f - want).abs() / want.max(1.0e-30));
        }
        println!("felt solve against bisection: worst relative error {worst:.1e}, most iterations {most}");
        assert!(worst < 1.0e-6, "the felt solve is {worst:.1e} off the law");
        assert!(most <= 12, "the felt solve took {most} iterations");
    }

    /// **The cored felt's solve is its law too.** The same 2000 random
    /// steps with a core under the felt (met at a random depth inside the
    /// unforced compression, 10 to 1000 times the felt's stiffness at a
    /// millimetre, exponent 1.5 to 3): the force returned satisfies `F = w(x)
    /// (K x^p + Kc (x - d)_+^q)` at the compression it leaves, against a
    /// 200-step bisection, within a part in a million. Measured: printed.
    #[test]
    fn the_cored_felt_solve_is_the_law() {
        let mut seed = 0x5eed_1234_u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let (mut worst, mut most) = (0.0_f64, 0u64);
        for _ in 0..2000 {
            let p = (2.0 + 1.5 * rnd()) as f32;
            let k = (10.0_f64.powf(1.0 + 2.0 * rnd()) / 1.0e-3_f64.powf(f64::from(p))) as f32;
            let free = 10.0_f64.powf(-5.0 + 2.0 * rnd()) as f32;
            let q = (1.5 + 1.5 * rnd()) as f32;
            let depth = (f64::from(free) * rnd()) as f32;
            let kc = (f64::from(k) * 1.0e-3_f64.powf(f64::from(p)) * 10.0_f64.powf(1.0 + 2.0 * rnd()) / 1.0e-3_f64.powf(f64::from(q))) as f32;
            let mut h = Hammer::new(HammerBase { mass: 0.005, stiffness: k, exponent: p, hysteresis: 0.03 });
            h.core = FeltCore { depth, stiffness: kc, exponent: q };
            let dt = 1.0 / 48_000.0_f32;
            let x_now = (f64::from(free) * (rnd() * 1.4 - 0.4)) as f32;
            let a = 10.0_f64.powf(-9.0 + 3.0 * rnd()) as f32;
            SOLVE_ITERS.with(|c| c.set(0));
            let f = f64::from(h.solve_felt(x_now, x_now, free, a, 0.03 / f64::from(dt), 1.0e-10));
            most = most.max(SOLVE_ITERS.with(|c| c.get()));
            let (free, a, x_now, dt, k, p) = (f64::from(free), f64::from(a), f64::from(x_now), f64::from(dt), f64::from(k), f64::from(p));
            let (kc, d, q) = (f64::from(kc), f64::from(depth), f64::from(q));
            let g = |ff: f64| {
                let x = free - a * ff;
                let w = (1.0 + 0.03 / dt * (x - x_now)).max(0.0);
                ff - (k * x.max(0.0).powf(p) + kc * (x - d).max(0.0).powf(q)) * w
            };
            let (mut lo, mut hi) = (0.0_f64, free / a);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if g(mid) > 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let want = 0.5 * (lo + hi);
            worst = worst.max((f - want).abs() / want.max(1.0e-30));
        }
        println!("cored felt solve against bisection: worst relative error {worst:.1e}, most iterations {most}");
        assert!(worst < 1.0e-6, "the cored felt solve is {worst:.1e} off the law");
        assert!(most <= 40, "the cored felt solve took {most} iterations");
    }
    /// **The midpoint solve is its law.** The cored law's 2000 random steps
    /// (half without a core), solved by `solve_midpoint` from a guess off the
    /// answer by up to a tenth either way, and from no force at all: the
    /// force returned satisfies `F = w(x) (K x^p + Kc (x - d)_+^q)` at the
    /// midpoint compression it leaves, `x = at - give F`, against a 200-step
    /// bisection, within a part in a million. From the guess, the iterations
    /// a sample the contact pays. Measured: printed.
    #[test]
    fn the_midpoint_solve_is_the_law() {
        let mut seed = 0x0dd5_eed5_u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let (mut worst, mut most, mut iters, mut calls) = (0.0_f64, 0u64, 0u64, 0u64);
        for trial in 0..2000 {
            let p = (2.0 + 1.5 * rnd()) as f32;
            let k = (10.0_f64.powf(1.0 + 2.0 * rnd()) / 1.0e-3_f64.powf(f64::from(p))) as f32;
            let at = 10.0_f64.powf(-5.0 + 2.0 * rnd()) as f32;
            let q = (1.5 + 1.5 * rnd()) as f32;
            let depth = (f64::from(at) * rnd()) as f32;
            let kc = if trial % 2 == 0 { 0.0 } else { (f64::from(k) * 1.0e-3_f64.powf(f64::from(p)) * 10.0_f64.powf(1.0 + 2.0 * rnd()) / 1.0e-3_f64.powf(f64::from(q))) as f32 };
            let mut h = Hammer::new(HammerBase { mass: 0.005, stiffness: k, exponent: p, hysteresis: 0.03 });
            h.core = FeltCore { depth, stiffness: kc, exponent: q };
            let dt = 1.0 / 48_000.0_f32;
            let rate = 2.0 * 0.03 / f64::from(dt);
            let x_now = (f64::from(at) * (rnd() * 1.4 - 0.4)) as f32;
            let give = 10.0_f64.powf(-9.0 + 3.0 * rnd()) as f32;
            let (atd, gd, xn) = (f64::from(at), f64::from(give), f64::from(x_now));
            let (kd, pd, kcd, dd, qd) = (f64::from(k), f64::from(p), f64::from(kc), f64::from(depth), f64::from(q));
            let g = |ff: f64| {
                let x = atd - gd * ff;
                let w = (1.0 + rate * (x - xn)).max(0.0);
                ff - (kd * x.max(0.0).powf(pd) + kcd * (x - dd).max(0.0).powf(qd)) * w
            };
            let (mut lo, mut hi) = (0.0_f64, atd / gd);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if g(mid) > 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let want = 0.5 * (lo + hi);
            for guess in [(want * (0.9 + 0.2 * rnd())) as f32, 0.0] {
                SOLVE_ITERS.with(|c| c.set(0));
                let f = f64::from(h.solve_midpoint(x_now, at, give, rate, guess));
                let it = SOLVE_ITERS.with(|c| c.get());
                most = most.max(it);
                if guess > 0.0 {
                    iters += it;
                    calls += 1;
                }
                worst = worst.max((f - want).abs() / want.max(1.0e-30));
            }
        }
        println!("midpoint solve against bisection: worst relative error {worst:.1e}, most iterations {most}, {:.2} from a guess a tenth off", iters as f64 / calls as f64);
        assert!(worst < 1.0e-6, "the midpoint solve is {worst:.1e} off the law");
        assert!(most <= 60, "the midpoint solve took {most} iterations");
    }

}
