//! The arpeggiator: the keys held, walked in time with the song.
//!
//! **Driven by the transport, and by its memory of the transport.** While
//! the song plays, steps fall on the grid the playhead defines, so an
//! arpeggio bounced twice lands on the same samples twice. When the song
//! stops, the clock keeps running at the last tempo it saw — an
//! arpeggiator that goes silent when the transport does cannot be
//! auditioned, and that is the whole of why the free-run exists.
//!
//! **Hold is a latch.** With it on, lifting every key keeps the pattern
//! going; the next key pressed after that starts a new one. Turning it off
//! keeps only what is physically down. It is a parameter, so it automates
//! like everything else on the panel.
//!
//! The arpeggiator sits between the keyboard and the allocator: with a
//! mode selected, keys go to it and it hands notes to the voices, with
//! sample offsets, through [`Arp::run`].

use super::osc::Rng;

/// The most keys the pattern remembers.
pub const MAX_KEYS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ArpMode {
    #[default]
    Off,
    Up,
    Down,
    UpDown,
    Random,
    AsPlayed,
}

impl ArpMode {
    pub const ALL: [ArpMode; 6] = [
        ArpMode::Off,
        ArpMode::Up,
        ArpMode::Down,
        ArpMode::UpDown,
        ArpMode::Random,
        ArpMode::AsPlayed,
    ];
    pub fn from_norm(v: f32) -> ArpMode {
        let last = (Self::ALL.len() - 1) as f32;
        let at = ((v.clamp(0.0, 1.0) * last).round() as usize).min(Self::ALL.len() - 1);
        Self::ALL[at]
    }
    pub fn label(self) -> &'static str {
        match self {
            ArpMode::Off => "off",
            ArpMode::Up => "up",
            ArpMode::Down => "down",
            ArpMode::UpDown => "up-down",
            ArpMode::Random => "random",
            ArpMode::AsPlayed => "as played",
        }
    }
}

/// The rate switch's positions, in beats per step (a beat is a quarter).
pub const ARP_RATES: [(&str, f64); 7] = [
    ("1/4", 1.0),
    ("1/4T", 2.0 / 3.0),
    ("1/8", 0.5),
    ("1/8T", 1.0 / 3.0),
    ("1/16", 0.25),
    ("1/16T", 1.0 / 6.0),
    ("1/32", 0.125),
];

pub fn rate_index(v: f32) -> usize {
    let last = (ARP_RATES.len() - 1) as f32;
    ((v.clamp(0.0, 1.0) * last).round() as usize).min(ARP_RATES.len() - 1)
}

/// What the song is doing this block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    pub bpm: f64,
    pub playing: bool,
    /// The playhead at the block's first sample, in beats. The engine
    /// (`instrument::rack_transport`) hands a rack with no span of its own
    /// the tempo map's beat at the first frame of the block it renders, and
    /// walks a rendered-ahead lane's from that frame across its own grain in
    /// a straight line, so only a tempo change inside that grain is still
    /// off, by a frame or so on a ramp.
    pub beat: f64,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            playing: false,
            beat: 0.0,
        }
    }
}

/// The dials the arpeggiator reads each block.
#[derive(Clone, Copy, Debug)]
pub struct ArpParams {
    pub mode: ArpMode,
    pub rate: usize,
    /// 1..=4
    pub octaves: u8,
    /// 0..1 of a step.
    pub gate: f32,
    pub hold: bool,
}

/// A note the arpeggiator wants played or stopped at `at` frames into the
/// block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpOut {
    On { at: usize, key: u8, vel: u8 },
    Off { at: usize, key: u8 },
}

pub struct Arp {
    rate: f64,
    /// Keys physically down, in press order, with velocities.
    down: Vec<(u8, u8)>,
    /// The pattern: what the steps walk. Equals `down` unless latched.
    pattern: Vec<(u8, u8)>,
    hold: bool,
    /// The free-running clock, beats, advanced when the song is stopped.
    clock: f64,
    bpm: f64,
    /// Index of the last step boundary crossed, so a boundary fires once.
    last_step: i64,
    /// Position in the walk, and the direction for up-down.
    walk: usize,
    up: bool,
    rng: Rng,
    /// The note sounding now, and the sample (absolute, in this clock's
    /// frames) it ends at.
    sounding: Option<u8>,
    gate_left: i64,
    was_on: bool,
}

impl Arp {
    pub fn new(rate: f32) -> Self {
        Self {
            rate: f64::from(rate),
            down: Vec::with_capacity(MAX_KEYS),
            pattern: Vec::with_capacity(MAX_KEYS),
            hold: false,
            clock: 0.0,
            bpm: 120.0,
            last_step: -1,
            walk: 0,
            up: true,
            rng: Rng::new(0xA5A5),
            sounding: None,
            gate_left: 0,
            was_on: false,
        }
    }

    pub fn key_down(&mut self, key: u8, vel: u8) {
        // A fresh press after the latch was left holding a pattern with
        // nothing down starts a new pattern.
        if self.hold && self.down.is_empty() {
            self.pattern.clear();
            self.walk = 0;
            self.up = true;
        }
        if self.down.len() < MAX_KEYS && !self.down.iter().any(|d| d.0 == key) {
            self.down.push((key, vel));
        }
        if !self.pattern.iter().any(|d| d.0 == key) && self.pattern.len() < MAX_KEYS {
            self.pattern.push((key, vel));
        }
    }

    pub fn key_up(&mut self, key: u8) {
        self.down.retain(|d| d.0 != key);
        if !self.hold {
            self.pattern.retain(|d| d.0 != key);
            if self.walk >= self.pattern.len().max(1) {
                self.walk = 0;
            }
        }
    }

    /// Everything up: the pattern too, latch or no latch.
    pub fn all_up(&mut self) {
        self.down.clear();
        self.pattern.clear();
        self.walk = 0;
    }

    /// The note the arpeggiator is sounding, if any — so the caller can
    /// stop it when the mode goes to off.
    pub fn sounding(&self) -> Option<u8> {
        self.sounding
    }

    /// The same note, and the arpeggiator forgets it — for a caller that is
    /// stopping it ITSELF.
    ///
    /// Without this the switch leaving a mode races [`Arp::run`]: the run
    /// that sees the mode off pushes an `Off` for whatever was sounding at
    /// frame 0, which lands on top of the chord the caller has just put back
    /// under the player's fingers and cuts the one note they share.
    pub fn take_sounding(&mut self) -> Option<u8> {
        self.gate_left = 0;
        self.sounding.take()
    }

    /// The pattern's keys in the order the mode walks them, with the
    /// octave spread applied: entry `i` of `n · octaves`.
    fn note_at(&self, p: &ArpParams, i: usize) -> Option<(u8, u8)> {
        let n = self.pattern.len();
        if n == 0 {
            return None;
        }
        let oct = (i / n) as u8;
        let within = i % n;
        let (key, vel) = match p.mode {
            ArpMode::AsPlayed | ArpMode::Random => self.pattern[within],
            _ => {
                let mut sorted: [(u8, u8); MAX_KEYS] = [(0, 0); MAX_KEYS];
                sorted[..n].copy_from_slice(&self.pattern);
                sorted[..n].sort_unstable_by_key(|d| d.0);
                sorted[within]
            }
        };
        Some((key.saturating_add(12 * oct).min(127), vel))
    }

    /// Advance the walk one step and return what to play.
    fn step(&mut self, p: &ArpParams) -> Option<(u8, u8)> {
        let n = self.pattern.len();
        if n == 0 {
            return None;
        }
        let total = n * usize::from(p.octaves.clamp(1, 4));
        let i = match p.mode {
            ArpMode::Off => return None,
            ArpMode::Up | ArpMode::AsPlayed => {
                let i = self.walk % total;
                self.walk = (self.walk + 1) % total;
                i
            }
            ArpMode::Down => {
                let i = total - 1 - (self.walk % total);
                self.walk = (self.walk + 1) % total;
                i
            }
            ArpMode::UpDown => {
                if total == 1 {
                    0
                } else {
                    let i = self.walk.min(total - 1);
                    if self.up {
                        if i + 1 >= total {
                            self.up = false;
                            self.walk = i - 1;
                        } else {
                            self.walk = i + 1;
                        }
                    } else if i == 0 {
                        self.up = true;
                        self.walk = 1;
                    } else {
                        self.walk = i - 1;
                    }
                    i
                }
            }
            ArpMode::Random => (self.rng.next_u32() as usize) % total,
        };
        self.note_at(p, i)
    }

    /// Walk `frames` of time and collect what to play. With the song
    /// playing the grid is the playhead's; stopped, it is the free clock's.
    pub fn run(&mut self, p: &ArpParams, t: Transport, frames: usize, out: &mut Vec<ArpOut>) {
        // **The song's tempo, ALWAYS - rolling or not.**
        //
        // Both arms below used to take it under `if t.playing`, so a stopped
        // transport free-ran at whatever `Arp::new` seeded (120) until
        // somebody pressed play once. Auditioning a patch is the one thing
        // you do with the transport stopped, so on a 90 bpm song a held
        // chord at 1/8 gave four notes a second instead of three, and it
        // came right the moment you rolled past and stopped again - which
        // reads as an arp that is not wired to the tempo at all. The owner
        // put it exactly that way.
        //
        // The module header above already promised this ("the clock keeps
        // running at the last tempo it saw"); "saw" quietly meant "saw
        // while rolling". Only the CLOCK is conditional now, which is the
        // part that genuinely is: a rolling transport owns the position, a
        // stopped one has to keep its own.
        self.bpm = t.bpm.max(1.0);
        self.hold = p.hold;
        if !p.hold {
            // **The latch came off: keep what is physically down.**
            //
            // Cleared and re-extended rather than cloned. `down.clone()`
            // allocates a Vec on the AUDIO THREAD, every 32-frame chunk, for
            // as long as a key is held with hold off - which is the arp's
            // ordinary state. `extend_from_slice` writes into the capacity
            // `pattern` already has; the two vectors are the same length by
            // construction and neither ever grows past the held-key count.
            //
            // Disjoint borrows, so this needs no temporary at all: the clone
            // existed only to satisfy the borrow checker.
            self.pattern.clear();
            self.pattern.extend_from_slice(&self.down);
        }
        let on = p.mode != ArpMode::Off && !self.pattern.is_empty();
        if !on {
            if let Some(k) = self.sounding.take() {
                out.push(ArpOut::Off { at: 0, key: k });
            }
            if p.mode == ArpMode::Off {
                self.was_on = false;
            }
            // Keep the clock in step with the song even while silent.
            if t.playing {
                self.clock = t.beat;
            } else {
                self.clock += frames as f64 * self.bpm / 60.0 / self.rate;
            }
            return;
        }
        if t.playing {
            self.clock = t.beat;
        }
        let step_beats = ARP_RATES[p.rate.min(ARP_RATES.len() - 1)].1;
        let beats_per_frame = self.bpm / 60.0 / self.rate;
        let gate_frames = ((p.gate.clamp(0.02, 1.0) as f64) * step_beats / beats_per_frame) as i64;
        if !self.was_on {
            // Coming on: fire on the first frame rather than waiting for
            // the next boundary, so a held chord speaks at once.
            self.last_step = (self.clock / step_beats).floor() as i64 - 1;
            self.was_on = true;
        }
        for i in 0..frames {
            let beat = self.clock + i as f64 * beats_per_frame;
            let step = (beat / step_beats).floor() as i64;
            if self.gate_left > 0 {
                self.gate_left -= 1;
                if self.gate_left == 0 {
                    if let Some(k) = self.sounding.take() {
                        out.push(ArpOut::Off { at: i, key: k });
                    }
                }
            }
            if step != self.last_step {
                self.last_step = step;
                if let Some(k) = self.sounding.take() {
                    out.push(ArpOut::Off { at: i, key: k });
                }
                if let Some((key, vel)) = self.step(p) {
                    out.push(ArpOut::On { at: i, key, vel });
                    self.sounding = Some(key);
                    self.gate_left = gate_frames.max(1);
                }
            }
        }
        if !t.playing {
            self.clock += frames as f64 * beats_per_frame;
        }
    }

    pub fn clear(&mut self) {
        self.down.clear();
        self.pattern.clear();
        self.sounding = None;
        self.gate_left = 0;
        self.walk = 0;
        self.up = true;
        self.was_on = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn params(mode: ArpMode) -> ArpParams {
        ArpParams {
            mode,
            rate: 2, // 1/8
            octaves: 1,
            gate: 0.5,
            hold: false,
        }
    }

    /// Collect a second of output at 120 bpm, stopped transport, in blocks.
    fn run_secs(arp: &mut Arp, p: &ArpParams, t: Transport, secs: f32) -> Vec<(usize, ArpOut)> {
        let mut all = Vec::new();
        let mut out = Vec::new();
        let block = 128;
        let mut at = 0usize;
        while (at as f32) < secs * SR {
            out.clear();
            let mut t2 = t;
            if t.playing {
                t2.beat = t.beat + at as f64 * t.bpm / 60.0 / f64::from(SR);
            }
            arp.run(p, t2, block, &mut out);
            all.extend(out.iter().map(|o| (at, *o)));
            at += block;
        }
        all
    }

    fn ons(events: &[(usize, ArpOut)]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|(_, o)| match o {
                ArpOut::On { key, .. } => Some(*key),
                _ => None,
            })
            .collect()
    }

    /// Up walks the chord ascending, in eighths at 120 bpm: four notes a
    /// second, the same notes round and round.
    #[test]
    fn up_walks_the_chord_ascending_at_the_rate() {
        let mut arp = Arp::new(SR);
        for k in [64, 60, 67] {
            arp.key_down(k, 100);
        }
        let ev = run_secs(&mut arp, &params(ArpMode::Up), Transport::default(), 2.0);
        let keys = ons(&ev);
        assert_eq!(keys.len(), 8, "{keys:?}");
        assert_eq!(&keys[..6], &[60, 64, 67, 60, 64, 67]);
        // Every on has its off half a step later (gate 0.5): a quarter
        // second per eighth at 120, so 6000 frames.
        let first_on = ev.iter().position(|(_, o)| matches!(o, ArpOut::On { .. })).unwrap();
        let first_off = ev.iter().position(|(_, o)| matches!(o, ArpOut::Off { .. })).unwrap();
        let abs = |i: usize| {
            ev[i].0
                + match ev[i].1 {
                    ArpOut::On { at, .. } | ArpOut::Off { at, .. } => at,
                }
        };
        let gate = abs(first_off) as i64 - abs(first_on) as i64;
        assert!((gate - 6000).abs() <= 2, "gate lasted {gate} frames");
    }

    /// Down descends, up-down bounces without repeating the ends, as-played
    /// keeps the press order, and octaves stack.
    #[test]
    fn the_other_modes_walk_their_own_way() {
        let walk = |mode: ArpMode, octaves: u8| {
            let mut arp = Arp::new(SR);
            for k in [64, 60, 67] {
                arp.key_down(k, 100);
            }
            let mut p = params(mode);
            p.octaves = octaves;
            ons(&run_secs(&mut arp, &p, Transport::default(), 2.0))
        };
        assert_eq!(&walk(ArpMode::Down, 1)[..4], &[67, 64, 60, 67]);
        assert_eq!(&walk(ArpMode::UpDown, 1)[..6], &[60, 64, 67, 64, 60, 64]);
        assert_eq!(&walk(ArpMode::AsPlayed, 1)[..4], &[64, 60, 67, 64]);
        assert_eq!(&walk(ArpMode::Up, 2)[..7], &[60, 64, 67, 72, 76, 79, 60]);
        let r = walk(ArpMode::Random, 1);
        assert!(r.iter().all(|k| [60, 64, 67].contains(k)));
        assert_eq!(r, walk(ArpMode::Random, 1), "random is not repeatable");
    }

    /// **Hold latches.** Lift every key and the pattern goes on; press a
    /// new key and it starts over with just that.
    #[test]
    fn hold_keeps_the_pattern_after_the_keys_come_up() {
        let mut arp = Arp::new(SR);
        let mut p = params(ArpMode::Up);
        p.hold = true;
        arp.key_down(60, 100);
        arp.key_down(64, 100);
        let a = ons(&run_secs(&mut arp, &p, Transport::default(), 0.5));
        assert_eq!(&a[..2], &[60, 64]);
        arp.key_up(60);
        arp.key_up(64);
        let b = ons(&run_secs(&mut arp, &p, Transport::default(), 1.0));
        assert!(b.len() >= 3 && b.iter().all(|k| *k == 60 || *k == 64), "{b:?}");
        arp.key_down(67, 100);
        let c = ons(&run_secs(&mut arp, &p, Transport::default(), 0.5));
        assert!(c.iter().all(|k| *k == 67), "{c:?}");
        // The latch off: nothing is down, so nothing plays, and the
        // sounding note is stopped.
        p.hold = false;
        arp.key_up(67);
        let ev = run_secs(&mut arp, &p, Transport::default(), 0.3);
        assert!(ons(&ev).is_empty());
        assert!(ev.iter().any(|(_, o)| matches!(o, ArpOut::Off { .. })));
    }

    /// **STOPPED, it runs at the song's tempo and not at 120.**
    ///
    /// Auditioning a patch is the one thing you do with the transport
    /// stopped, and both tempo assignments used to sit under `if
    /// t.playing` - so the arp free-ran at whatever `Arp::new` seeded until
    /// somebody pressed play once, and came right the moment they stopped
    /// again. The owner: "i am not sure that analog's arp is hooked up with
    /// it's rate and the project tempo."
    ///
    /// The tempos are chosen so the two answers cannot be confused: at 90
    /// bpm an eighth is 1/3 s and at 120 it is 1/4 s, so one second holds
    /// three steps or four.
    #[test]
    fn stopped_it_runs_at_the_songs_tempo_and_not_at_a_default() {
        let steps = |bpm: f64| -> usize {
            let mut arp = Arp::new(SR);
            arp.key_down(60, 100);
            arp.key_down(64, 100);
            arp.key_down(67, 100);
            let p = ArpParams {
                mode: ArpMode::Up,
                // 1/8 - index 2 in `ARP_RATES`.
                rate: 2,
                octaves: 1,
                gate: 0.5,
                hold: false,
            };
            // STOPPED, and the beat it carries is irrelevant while it is.
            let t = Transport { bpm, playing: false, beat: 0.0 };
            run_secs(&mut arp, &p, t, 1.0)
                .iter()
                .filter(|(_, o)| matches!(o, ArpOut::On { .. }))
                .count()
        };
        // A fresh `Arp` has never seen a rolling transport, which is the
        // case that was broken: the seed was 120 and nothing overwrote it.
        assert_eq!(steps(120.0), 4, "the fixture does not step at 120");
        assert_eq!(
            steps(90.0),
            3,
            "a stopped arp ignored the song's tempo and free-ran at its seed"
        );
        assert_eq!(
            steps(180.0),
            6,
            "a stopped arp ignored a tempo ABOVE its seed too"
        );

        // **And a tempo changed while stopped is followed**, rather than
        // latched at whatever it first saw.
        let mut arp = Arp::new(SR);
        arp.key_down(60, 100);
        let p = ArpParams {
            mode: ArpMode::Up,
            rate: 2,
            octaves: 1,
            gate: 0.5,
            hold: false,
        };
        let slow = Transport { bpm: 60.0, playing: false, beat: 0.0 };
        let fast = Transport { bpm: 240.0, playing: false, beat: 0.0 };
        let a = run_secs(&mut arp, &p, slow, 1.0)
            .iter()
            .filter(|(_, o)| matches!(o, ArpOut::On { .. }))
            .count();
        let b = run_secs(&mut arp, &p, fast, 1.0)
            .iter()
            .filter(|(_, o)| matches!(o, ArpOut::On { .. }))
            .count();
        assert!(
            b > a * 3,
            "the dial moved from 60 to 240 while stopped and the arp went {a} -> {b}"
        );
    }

    /// **The transport's grid is the grid.** Playing from beat 0.9 at
    /// 100 bpm, the first boundary after the start lands where the
    /// playhead crosses beat 1.0 — 0.1 beats in, 2880 frames — and not on
    /// the block's own clock.
    #[test]
    fn a_playing_transport_places_steps_on_its_own_grid() {
        let mut arp = Arp::new(SR);
        arp.key_down(60, 100);
        let p = ArpParams { rate: 0, ..params(ArpMode::Up) }; // quarters
        let t = Transport { bpm: 100.0, playing: true, beat: 0.9 };
        let ev = run_secs(&mut arp, &p, t, 1.0);
        let on_frames: Vec<usize> = ev
            .iter()
            .filter_map(|(b, o)| match o {
                ArpOut::On { at, .. } => Some(b + at),
                _ => None,
            })
            .collect();
        // The first fires at once (coming on), the second on the grid at
        // beat 1.0 = 0.1 beats = 0.06 s = 2880 frames.
        assert_eq!(on_frames[0], 0);
        assert!((on_frames[1] as i64 - 2880).abs() <= 1, "{on_frames:?}");
        // Then every 0.6 s (a quarter at 100).
        assert!((on_frames[2] as i64 - (2880 + 28800)).abs() <= 1, "{on_frames:?}");
    }

    /// **Stopped, it free-runs at the last tempo it saw**: the same
    /// spacing after the transport stops as before.
    #[test]
    fn stopped_it_keeps_the_last_tempo() {
        let mut arp = Arp::new(SR);
        arp.key_down(60, 100);
        let p = params(ArpMode::Up);
        let t = Transport { bpm: 90.0, playing: true, beat: 0.0 };
        let _ = run_secs(&mut arp, &p, t, 0.5);
        let stopped = Transport { bpm: 90.0, playing: false, beat: 0.0 };
        let ev = run_secs(&mut arp, &p, stopped, 2.0);
        let on_frames: Vec<usize> = ev
            .iter()
            .filter_map(|(b, o)| match o {
                ArpOut::On { at, .. } => Some(b + at),
                _ => None,
            })
            .collect();
        // An eighth at 90 bpm is a third of a second: 16000 frames.
        for w in on_frames.windows(2) {
            assert!((w[1] as i64 - w[0] as i64 - 16000).abs() <= 1, "{on_frames:?}");
        }
        assert!(on_frames.len() >= 5);
    }
}
