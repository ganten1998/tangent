//! The electric grand's hammer: a mass on TWO springs in series of
//! hardness — a soft outer felt, and under it the hard underfelt and the
//! wooden moulding that only a forte blow reaches.
//!
//! **Why not `hammer::Hammer`.** That hammer's felt is one power law,
//! `F = K x^p (1 + mu x')`, and against these short, stiff strings it could
//! not be the reference's: a felt soft enough for its mezzo — the C4 line
//! output falls 16 dB from the fundamental to the second partial at MIDI
//! 70, a contact about one period long — has no high partials left at
//! forte (C4 MIDI 127: the twelfth and sixteenth partials 9 to 11 dB short,
//! and above 3 kHz the first 12 ms of a C2 fortissimo 40 dB short of the
//! reference's), and one hard enough for its forte is far too bright at
//! mezzo. A real hammer is not one material: the felt is densest at the
//! moulding, and a blow that compresses the outer layer through meets
//! something an order of magnitude stiffer. The law here is the felt's
//! power law plus that core's, engaging at `core_at` of compression:
//! `F = [K x^p + K_core max(0, x - core_at)^q] max(0, 1 + mu x')`. Below
//! the core it IS `hammer::Hammer`'s law, and with no core it is that
//! hammer step for step (`with_no_core_it_is_the_shared_hammer`).
//!
//! **The step is `hammer::Hammer::tick_implicit`'s**: the force is solved
//! against the compression it leaves one sample on, which the modal bank
//! can say in advance (`modal::Bank::render_contact`), so no stiffness and
//! no rate makes it unstable — the core is many times stiffer than the felt
//! and an explicit step would chatter on it. The solve is safeguarded
//! Newton on the compression, bracketed by the two ends where the law's
//! residual changes sign; the residual rises monotonically, so the root is
//! unique (`the_two_layer_solve_is_the_law`).

/// A two-layer hammer's material: the shared hammer's four numbers and the
/// core's three.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeltCore {
    /// Effective mass at the strike line, kg.
    pub mass: f32,
    /// The outer felt: `K` (N / m^p) and `p`.
    pub stiffness: f32,
    pub exponent: f32,
    /// Hunt-Crossley rate coefficient, s/m.
    pub hysteresis: f32,
    /// The core: the compression it starts at, m, its stiffness `K_core`
    /// (N / m^q) and exponent `q`. A zero stiffness is no core.
    pub core_at: f32,
    pub core_stiffness: f32,
    pub core_exponent: f32,
}

/// As in `hammer.rs`: how far below first touch the hammer must have
/// retreated to be out of the string's reach, the longest free flight
/// before it is declared caught, and the longest contact that is physics.
const CHECK_DIST: f32 = 3.0e-3;
const FLIGHT_LIMIT: f32 = 5.0e-3;
const CONTACT_LIMIT: f32 = 40.0e-3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Idle,
    Armed,
    Contact,
    Flight,
}

/// One two-layer hammer. Owned by a voice; `strike` and `tick_implicit`
/// touch nothing but this struct.
pub struct CoreHammer {
    base: FeltCore,
    state: State,
    eta: f32,
    eta_dot: f32,
    eta_touch: f32,
    timer: u32,
    flight: f32,
    contact: f32,
    launch: f32,
}

impl CoreHammer {
    pub fn new(base: FeltCore) -> Self {
        CoreHammer {
            base,
            state: State::Idle,
            eta: 0.0,
            eta_dot: 0.0,
            eta_touch: 0.0,
            timer: 0,
            flight: 0.0,
            contact: 0.0,
            launch: 0.0,
        }
    }

    /// `hammer::Hammer::strike_with`: the material is taken only while the
    /// hammer is on its check; a strike that lands while it is busy keeps
    /// it where it is and gives it the new velocity.
    pub fn strike_with(&mut self, base: FeltCore, velocity: f32) {
        if matches!(self.state, State::Idle | State::Armed) {
            self.base = base;
        }
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

    pub fn idle(&self) -> bool {
        self.state == State::Idle
    }

    /// The most recent strike's first contact, seconds (zero until it ends):
    /// the contact-time diagnostics' reading.
    #[cfg(test)]
    pub fn contact_secs(&self) -> f32 {
        self.contact
    }

    pub fn clear(&mut self) {
        self.state = State::Idle;
        self.eta = 0.0;
        self.eta_dot = 0.0;
        self.eta_touch = 0.0;
        self.timer = 0;
        self.flight = 0.0;
        self.contact = 0.0;
        self.launch = 0.0;
    }

    /// One sample against a string whose displacement under the felt one
    /// sample on is `next_free + compliance F`: `hammer::Hammer::tick_implicit`
    /// with the two-layer law.
    pub fn tick_implicit(&mut self, now: f32, next_free: f32, compliance: f32, dt: f32) -> f32 {
        match self.state {
            State::Idle => return 0.0,
            State::Armed => {
                self.eta = now;
                self.eta_dot = self.launch;
                self.eta_touch = now;
                self.timer = 0;
                self.flight = 0.0;
                self.state = State::Flight;
            }
            State::Contact | State::Flight => {}
        }
        let mass = self.base.mass;
        let x_now = self.eta - now;
        let free = self.eta + self.eta_dot * dt - next_free;
        let mut force = 0.0_f32;
        if free > 0.0 {
            if self.state == State::Flight {
                self.state = State::Contact;
                self.timer = 0;
            }
            force = solve(&self.base, x_now, free, dt * dt / mass + compliance.max(0.0), dt);
            self.timer += 1;
            if self.timer as f32 * dt > CONTACT_LIMIT {
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
        self.eta_dot -= force / mass * dt;
        self.eta += self.eta_dot * dt;
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
}

/// The two-layer law's force at compression `x` and rate weight `w`, and its
/// derivative in `x` at fixed `w`.
fn law(b: &FeltCore, x: f64) -> (f64, f64) {
    let (k, p) = (f64::from(b.stiffness), f64::from(b.exponent));
    let mut f = k * x.powf(p);
    let mut d = k * p * x.powf(p - 1.0);
    let over = x - f64::from(b.core_at);
    if b.core_stiffness > 0.0 && over > 0.0 {
        let (kc, q) = (f64::from(b.core_stiffness), f64::from(b.core_exponent));
        f += kc * over.powf(q);
        d += kc * q * over.powf(q - 1.0);
    }
    (f, d)
}

/// The force that satisfies the law at the compression it leaves: find x in
/// the bracket with `a law(x) w(x) = free - x`, `w = max(0, 1 + mu (x -
/// x_now) / dt)`, and return `(free - x) / a`. The residual
/// `r(x) = a law(x) w(x) - (free - x)` is negative at the bracket's foot (no
/// force there: no compression, or the rate term at zero) and positive at
/// `free`, and rises in between, so the root is unique; Newton from the last
/// sample's compression, a bisection whenever a step would leave the
/// bracket, at most sixty steps, in `f64`.
fn solve(b: &FeltCore, x_now: f32, free: f32, a: f32, dt: f32) -> f32 {
    solve64(b, f64::from(x_now), f64::from(free), f64::from(a), f64::from(dt)) as f32
}

fn solve64(b: &FeltCore, x_now: f64, free: f64, a: f64, dt: f64) -> f64 {
    let rate = f64::from(b.hysteresis) / dt;
    let x_w = if rate > 0.0 { x_now - 1.0 / rate } else { f64::NEG_INFINITY };
    if x_w >= free {
        return 0.0;
    }
    let mut lo = x_w.max(0.0);
    let mut hi = free;
    let resid = |x: f64| -> (f64, f64) {
        let w = 1.0 + rate * (x - x_now);
        if w <= 0.0 || x <= 0.0 {
            return (-(free - x), 1.0);
        }
        let (f, d) = law(b, x);
        (a * f * w - (free - x), a * (d * w + f * rate) + 1.0)
    };
    let mut x = if x_now > lo && x_now < hi { x_now } else { 0.5 * (lo + hi) };
    for _ in 0..60 {
        let (r, slope) = resid(x);
        if r.abs() <= 1.0e-12 * free {
            break;
        }
        if r > 0.0 {
            hi = x;
        } else {
            lo = x;
        }
        let n = x - r / slope;
        let next = if n > lo && n < hi { n } else { 0.5 * (lo + hi) };
        if (next - x).abs() <= 1.0e-13 * free {
            x = next;
            break;
        }
        x = next;
    }
    ((free - x) / a).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::hammer::{Hammer, HammerBase};

    /// A string the hammer can press: a single mode of mass `m` and
    /// stiffness `k`, stepped exactly as the bank does it (its displacement
    /// one sample on is linear in this sample's force).
    struct Mode {
        y: f64,
        v: f64,
        m: f64,
        k: f64,
    }

    impl Mode {
        fn next_free(&self, dt: f64) -> (f64, f64) {
            // Semi-implicit Euler: v += (-k y + F) / m dt; y += v dt.
            let v = self.v - self.k * self.y / self.m * dt;
            (self.y + v * dt, dt * dt / self.m)
        }
        fn step(&mut self, force: f64, dt: f64) {
            self.v += (-self.k * self.y + force) / self.m * dt;
            self.y += self.v * dt;
        }
    }

    #[test]
    fn with_no_core_it_is_the_shared_hammer() {
        let dt = 1.0 / 48_000.0_f32;
        for (mass, k, p, mu, speed) in [(8e-3, 2.7e7, 1.92, 0.2, 3.5), (3e-3, 4e8, 2.3, 0.6, 6.0), (12e-3, 3e6, 2.6, 0.0, 0.5)] {
            let shared = HammerBase {
                mass,
                stiffness: k,
                exponent: p,
                hysteresis: mu,
            };
            let ours = FeltCore {
                mass,
                stiffness: k,
                exponent: p,
                hysteresis: mu,
                core_at: 1.0e-3,
                core_stiffness: 0.0,
                core_exponent: 2.0,
            };
            let (mut h1, mut h2) = (Hammer::new(shared), CoreHammer::new(ours));
            h1.strike_with(shared, speed);
            h2.strike_with(ours, speed);
            let (mut s1, mut s2) = (
                Mode { y: 0.0, v: 0.0, m: 4e-3, k: 1.1e4 },
                Mode { y: 0.0, v: 0.0, m: 4e-3, k: 1.1e4 },
            );
            let mut worst = 0.0_f64;
            let mut peak = 0.0_f64;
            for _ in 0..400 {
                let (n1, c1) = s1.next_free(f64::from(dt));
                let (n2, c2) = s2.next_free(f64::from(dt));
                let f1 = f64::from(h1.tick_implicit(s1.y as f32, n1 as f32, c1 as f32, dt));
                let f2 = f64::from(h2.tick_implicit(s2.y as f32, n2 as f32, c2 as f32, dt));
                s1.step(f1, f64::from(dt));
                s2.step(f2, f64::from(dt));
                worst = worst.max((f1 - f2).abs());
                peak = peak.max(f1.abs());
            }
            assert!(peak > 1.0, "the strike never pressed: peak {peak}");
            assert!(worst <= 1.0e-4 * peak, "mass {mass}: the two laws part by {worst} N against a peak of {peak} N");
        }
    }

    #[test]
    fn the_two_layer_solve_is_the_law() {
        let b = FeltCore {
            mass: 5e-3,
            stiffness: 3e7,
            exponent: 2.2,
            hysteresis: 0.3,
            core_at: 0.8e-3,
            core_stiffness: 4e9,
            core_exponent: 2.5,
        };
        let dt = 1.0 / 48_000.0_f64;
        let mut z = 0x1234_5678_9abc_def0_u64;
        let mut unit = || {
            z ^= z << 13;
            z ^= z >> 7;
            z ^= z << 17;
            (z >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..2000 {
            let free = (0.05 + 3.0 * unit()) * 1.0e-3;
            let x_now = free * (unit() * 1.2 - 0.1);
            let a = 10.0_f64.powf(-9.0 + 4.0 * unit());
            let f = solve64(&b, x_now, free, a, dt);
            let x = free - a * f;
            let w = (1.0 + f64::from(b.hysteresis) / dt * (x - x_now)).max(0.0);
            let want = if x > 0.0 && w > 0.0 { law(&b, x).0 * w } else { 0.0 };
            let tol = 1.0e-5 * want.max(f).max(1.0e-6);
            assert!((f - want).abs() <= tol, "F {f} against the law's {want} (free {free}, x_now {x_now}, a {a})");
        }
    }
}
