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
    /// The jazz grand's hammer, fitted at C4 against three unison strings of
    /// 2.33 N s/m each (see `tests::IdealString`) so that the first contact
    /// runs 2.1 ms at MIDI 8 and 1.0 ms at MIDI 127.
    ///
    /// **The mass is an effective mass, and it is light.** A C4 hammer HEAD
    /// weighs about eight grams and the simulation literature uses around
    /// three; this one is a gram. It has to be, because in this model the
    /// loud-end contact is a bounce on the string segment between hammer
    /// and agraffe, whose stiffness is `3 T / x0` — about 30 kN/m here —
    /// and a half period of one millisecond on that spring is only reached
    /// by a gram. Three grams lands at 1.3 ms. Either the effective mass a
    /// bending shank presents really is that low, or the 1.0 ms target
    /// (which was the old Gaussian's truncation width, not a measurement)
    /// wants re-baselining; the fit is honest about which number it kept.
    pub const JAZZ: HammerBase = HammerBase {
        mass: 1.0e-3,
        stiffness: 1.0e10,
        exponent: 3.0,
        hysteresis: 0.03,
    };
    /// The concert instrument's hammer: a tenth heavier, felt voiced harder
    /// and more nonlinear — a wider sweep from whisper to bark out of the
    /// same mechanism, which is what a hall instrument is regulated for.
    pub const CONCERT: HammerBase = HammerBase {
        mass: 1.1e-3,
        stiffness: 1.6e11,
        exponent: 3.3,
        hysteresis: 0.03,
    };
}

/// Hammer velocity in metres per second for a MIDI velocity given as a
/// fraction of 127 (the unit `Keys::note_on` already speaks).
///
/// Linear, from 0.5 m/s at the bottom of the scale to 6 m/s at the top:
/// the range Askenfelt and Jansson measured on pianists from pianissimo to
/// fortissimo. The curve from key speed to hammer speed is the ACTION's
/// business and is not modelled; this is the hammer's arrival speed.
pub fn velocity_of_midi(v: f32) -> f32 {
    0.5 + 5.5 * v.clamp(0.0, 1.0)
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
/// throw (0.05 m/s, a tenth of any pianissimo) leaves in eight
/// milliseconds; forty is a guard against a caller feeding a NaN or an
/// absurd scale, and it never fires in any test —
/// `force_is_smooth_and_hammer_always_leaves` asserts the contact ended
/// well inside it.
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

    pub fn in_contact(&self) -> bool {
        self.state == State::Contact
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

    /// How long the most recent strike's first contact lasted, in seconds;
    /// zero while it is still going on or if there has been none.
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
    fn midi_velocity_map_is_linear_and_bounded() {
        assert_eq!(velocity_of_midi(0.0), 0.5);
        assert_eq!(velocity_of_midi(1.0), 6.0);
        assert_eq!(velocity_of_midi(2.0), 6.0);
        assert!((velocity_of_midi(0.5) - 3.25).abs() < 1e-6);
    }
}
