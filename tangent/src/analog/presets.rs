//! The factory presets: twenty-four sounds, in the words of the panel.
//!
//! **What a factory preset is here.** A name, a line about it, and the
//! dials that differ from the shipped defaults, by SYMBOL — the same
//! append-only address the state blob and the preset file use, so a
//! preset written today still names the same dials after the table grows.
//! [`Factory::values`] fills the rest in from the defaults, because a
//! preset is a whole sound and not a diff: loading "Warm Pad" after "Acid
//! Bass" must not leave the bass's glide behind.
//!
//! **Where they go.** The host seeds them as ordinary preset files into
//! `Presets/Tangent Analog/` the first time that folder is asked for
//! (`desktop::push_preset_rows`), and from then on they are the user's:
//! rename them, delete them, save over them. A folder that is deleted
//! whole is seeded again; a file that is deleted stays deleted. No settings
//! key remembers any of it.
//!
//! **Voiced on the bench, not by ear.** `analogrig presets` renders each
//! one with a phrase suited to its kind and prints the peak, and the test
//! at the foot holds every one under the ceiling and above silence. What
//! the bench cannot judge — whether "Pedal Bass" is a good pedal bass — is
//! the owner's, and the names below are evocative on purpose and
//! trademarks nowhere (see the naming note in `docs/ANALOG.md`).

use super::{defaults, spec, PARAMS, PARAM_SLOTS};

/// One shipped sound.
pub struct Factory {
    pub name: &'static str,
    pub about: &'static str,
    /// `(symbol, value)` for every dial that is not at its default.
    pub set: &'static [(&'static str, f32)],
}

impl Factory {
    /// Every dial, by symbol, in the table's order: the defaults with this
    /// preset's own values laid over them.
    pub fn values(&self) -> Vec<(&'static str, f32)> {
        PARAMS
            .iter()
            .map(|p| {
                let v = self
                    .set
                    .iter()
                    .find(|(s, _)| *s == p.symbol)
                    .map_or(p.default, |(_, v)| *v);
                (p.symbol, v)
            })
            .collect()
    }

    /// The same, indexed by id, for a synth's `set_all`.
    pub fn by_id(&self) -> Vec<(u32, f32)> {
        self.values()
            .into_iter()
            .map(|(s, v)| (spec_id(s), v))
            .collect()
    }
}

fn spec_id(symbol: &str) -> u32 {
    PARAMS.iter().find(|p| p.symbol == symbol).map_or(u32::MAX, |p| p.id)
}

// The switch positions as normalised values, so the table below reads in
// the panel's words rather than in fifths and fifteenths.
const RANGE_LO: f32 = 0.0;
const RANGE_32: f32 = 0.2;
const RANGE_16: f32 = 0.4;
const RANGE_8: f32 = 0.6;
const RANGE_4: f32 = 0.8;
const TRI: f32 = 0.0;
const TRI_SAW: f32 = 0.2;
const SAW: f32 = 0.4;
const SQUARE: f32 = 0.6;
const WIDE: f32 = 0.8;
const NARROW: f32 = 1.0;
const MONO: f32 = 0.0;
const UNISON: f32 = 0.5;
const POLY: f32 = 1.0;
const LOW: f32 = 0.0;
const LAST: f32 = 0.5;
const RETRIG: f32 = 1.0;
const LADDER: f32 = 0.0;
const SEM: f32 = 1.0;
const ENS_OFF: f32 = 0.0;
const ENS_I: f32 = 1.0 / 3.0;
const ENS_II: f32 = 2.0 / 3.0;
const ENS_III: f32 = 1.0;
const ARP_UP: f32 = 0.2;
const ARP_UP_DOWN: f32 = 0.6;
const RATE_8TH: f32 = 2.0 / 6.0;
const RATE_16TH: f32 = 4.0 / 6.0;
const LFO_TRI: f32 = 0.0;
const LFO_SINE: f32 = 0.25;
const ON: f32 = 1.0;
const OFF: f32 = 0.0;

/// An oscillator tune knob, in cents from centre.
const fn cents(c: f32) -> f32 {
    0.5 + c / 1400.0
}
/// An oscillator tune knob, in semitones from centre.
const fn semis(s: f32) -> f32 {
    0.5 + s / 14.0
}
/// The Voices dial for `n` voices.
const fn voices(n: u32) -> f32 {
    (n - 1) as f32 / 15.0
}

/// **The twenty-four.** Five leads, five basses, three brass, four pads,
/// three plucks, two string ensembles, two arpeggios: the brief's spread.
pub const FACTORY: [Factory; 24] = [
    // ── leads ──
    Factory {
        name: "First Lead",
        about: "two saws a few cents apart, gliding, the one everybody plays first",
        set: &[
            ("voice_mode", MONO), ("priority", LAST), ("glide", 0.32),
            ("o2_wave", SAW), ("o2_tune", cents(8.0)), ("mix_o2", 0.7),
            ("cutoff", 0.68), ("emphasis", 0.32), ("contour", 0.45),
            ("f_decay", 0.5), ("f_sustain", 0.3), ("a_release", 0.22),
            ("lfo_delay", 0.35), ("osc_mod", ON), ("mod_depth", 0.22),
            ("volume", 0.93),
        ],
    },
    Factory {
        name: "Sync Lead",
        about: "oscillator two drags oscillator one's reset up a fifth: the tearing lead",
        set: &[
            ("voice_mode", MONO), ("sync", ON), ("o1_range", RANGE_4), ("mix_o1", 0.85),
            ("o2_range", RANGE_8), ("o2_tune", semis(5.0)), ("mix_o2", 0.0),
            ("cutoff", 0.82), ("emphasis", 0.18), ("contour", 0.3), ("f_decay", 0.45),
            ("f_sustain", 0.5), ("a_release", 0.2), ("filter_mod", ON), ("mod_depth", 0.3),
            ("volume", 0.99),
        ],
    },
    Factory {
        name: "Whistle",
        about: "two squares an octave up, a slow vibrato, the filter nearly closed",
        set: &[
            ("voice_mode", MONO), ("glide", 0.28), ("o1_range", RANGE_4), ("o1_wave", SQUARE),
            ("o2_range", RANGE_4), ("o2_wave", SQUARE), ("o2_tune", cents(12.0)), ("mix_o2", 0.6),
            ("cutoff", 0.55), ("emphasis", 0.5), ("contour", 0.25), ("f_sustain", 0.5),
            ("a_attack", 0.15), ("a_release", 0.25), ("lfo_rate", 0.5), ("lfo_delay", 0.3),
            ("osc_mod", ON), ("mod_depth", 0.18),
            ("volume", 0.96),
        ],
    },
    Factory {
        name: "Fat Unison",
        about: "eight voices on one key, detuned and thrown across the field",
        set: &[
            ("voice_mode", UNISON), ("voices", voices(8)), ("detune", 0.45), ("unison_spread", 0.75),
            ("o2_wave", SAW), ("o2_tune", cents(-6.0)), ("mix_o2", 0.7), ("cutoff", 0.62),
            ("emphasis", 0.2), ("contour", 0.35), ("f_sustain", 0.4), ("drive", 0.2),
            ("a_release", 0.28), ("drift", 0.4),
            ("volume", 0.72),
        ],
    },
    Factory {
        name: "Pulse Lead",
        about: "a wide and a narrow pulse, the LFO moving both edges",
        set: &[
            ("voice_mode", MONO), ("trigger", RETRIG), ("o1_wave", WIDE), ("o2_wave", NARROW),
            ("o2_tune", cents(7.0)), ("mix_o2", 0.65), ("pwm", 0.35), ("lfo_rate", 0.4),
            ("cutoff", 0.7), ("emphasis", 0.3), ("contour", 0.4), ("f_decay", 0.45),
            ("f_sustain", 0.35), ("a_release", 0.2),
            ("volume", 0.88),
        ],
    },
    // ── basses ──
    Factory {
        name: "Model Bass",
        about: "the 1970 machine's bass: two saws and a square an octave under, low-note priority",
        set: &[
            ("voice_mode", MONO), ("priority", LOW), ("o1_range", RANGE_16), ("o2_range", RANGE_16),
            ("o2_wave", SAW), ("o2_tune", cents(5.0)), ("mix_o2", 0.65), ("o3_range", RANGE_32),
            ("o3_wave", SQUARE), ("mix_o3", 0.4), ("cutoff", 0.5), ("emphasis", 0.25),
            ("contour", 0.55), ("f_decay", 0.4), ("f_sustain", 0.1), ("a_release", 0.15),
            ("volume", 0.91),
        ],
    },
    Factory {
        name: "Pedal Bass",
        about: "two saws at 32 feet and a square above them, driven, for the left foot",
        set: &[
            ("voice_mode", MONO), ("priority", LOW), ("o1_range", RANGE_32), ("o2_range", RANGE_32),
            ("o2_wave", SAW), ("o2_tune", cents(4.0)), ("mix_o2", 0.7), ("o3_range", RANGE_16),
            ("o3_wave", SQUARE), ("mix_o3", 0.35), ("cutoff", 0.36), ("emphasis", 0.15),
            ("contour", 0.4), ("f_sustain", 0.3), ("a_sustain", 0.9), ("a_release", 0.2),
            ("drive", 0.35), ("volume", 0.78),
        ],
    },
    Factory {
        name: "Pluck Bass",
        about: "a square and a saw with the contour doing the plucking",
        set: &[
            ("voice_mode", MONO), ("o1_range", RANGE_16), ("o1_wave", SQUARE), ("o2_range", RANGE_16),
            ("o2_wave", SAW), ("mix_o2", 0.6), ("cutoff", 0.45), ("emphasis", 0.2),
            ("contour", 0.65), ("f_decay", 0.28), ("f_sustain", 0.0), ("a_decay", 0.45),
            ("a_sustain", 0.4), ("a_release", 0.15),
            ("volume", 0.96),
        ],
    },
    Factory {
        name: "Sub Bass",
        about: "a triangle two octaves down and nothing in its way",
        set: &[
            ("voice_mode", MONO), ("priority", LOW), ("o1_range", RANGE_32), ("o1_wave", TRI),
            ("mix_o1", 0.9), ("o2_range", RANGE_16), ("o2_wave", TRI), ("mix_o2", 0.35),
            ("cutoff", 0.42), ("emphasis", 0.0), ("contour", 0.2), ("f_sustain", 0.6),
            ("drive", 0.1), ("a_release", 0.18), ("volume", 0.82),
        ],
    },
    Factory {
        name: "Acid Bass",
        about: "one saw, the ladder near singing, the notes sliding into each other",
        set: &[
            ("voice_mode", MONO), ("glide", 0.3), ("o1_range", RANGE_16), ("mix_o1", 0.85),
            ("cutoff", 0.35), ("emphasis", 0.75), ("contour", 0.7), ("f_decay", 0.35),
            ("f_sustain", 0.05), ("a_sustain", 0.85), ("a_release", 0.12), ("volume", 0.98),
        ],
    },
    // ── brass ──
    Factory {
        name: "Poly Brass",
        about: "two saws and a slow filter bite, eight voices, a little drift between them",
        set: &[
            ("o2_wave", SAW), ("o2_tune", cents(6.0)), ("mix_o2", 0.7), ("cutoff", 0.55),
            ("emphasis", 0.2), ("contour", 0.55), ("f_attack", 0.25), ("f_decay", 0.45),
            ("f_sustain", 0.45), ("a_attack", 0.2), ("a_release", 0.3), ("spread", 0.5),
            ("drift", 0.35), ("volume", 0.7),
        ],
    },
    Factory {
        name: "Brass Stab",
        about: "the same section hit hard and let go quickly",
        set: &[
            ("o2_wave", SAW), ("o2_tune", cents(-5.0)), ("mix_o2", 0.7), ("o3_range", RANGE_16),
            ("o3_wave", SQUARE), ("mix_o3", 0.3), ("cutoff", 0.6), ("emphasis", 0.15),
            ("contour", 0.6), ("f_decay", 0.4), ("f_sustain", 0.2), ("a_sustain", 0.7),
            ("a_release", 0.18), ("vel_filter", 0.5), ("volume", 0.7),
        ],
    },
    Factory {
        name: "SEM Brass",
        about: "the two-pole filter's brass: rounder, no bass lost to the resonance",
        set: &[
            ("filter_model", SEM), ("sem_mode", 0.0), ("o2_wave", SAW), ("o2_tune", cents(7.0)),
            ("mix_o2", 0.7), ("cutoff", 0.5), ("emphasis", 0.4), ("contour", 0.5),
            ("f_attack", 0.2), ("f_decay", 0.45), ("f_sustain", 0.4), ("a_attack", 0.18),
            ("a_release", 0.3), ("spread", 0.4), ("volume", 0.62),
        ],
    },
    // ── pads ──
    Factory {
        name: "Warm Pad",
        about: "saws a few cents apart under a slow filter, the first ensemble setting behind them",
        set: &[
            ("o2_wave", SAW), ("o2_tune", cents(8.0)), ("mix_o2", 0.7), ("cutoff", 0.5),
            ("emphasis", 0.1), ("contour", 0.2), ("f_attack", 0.5), ("f_sustain", 0.6),
            ("a_attack", 0.55), ("a_sustain", 0.9), ("a_release", 0.6), ("ensemble", ENS_I),
            ("ens_width", 0.6), ("drift", 0.4), ("lfo_delay", 0.5), ("volume", 0.64),
        ],
    },
    Factory {
        name: "Glass Pad",
        about: "a triangle and a pulse the LFO keeps moving, through the SEM leaning high",
        set: &[
            ("filter_model", SEM), ("sem_mode", 0.62), ("emphasis", 0.35), ("o1_wave", TRI),
            ("o2_wave", WIDE), ("o2_tune", cents(-7.0)), ("mix_o2", 0.6), ("pwm", 0.4),
            ("lfo_rate", 0.3), ("lfo_wave", LFO_SINE), ("cutoff", 0.55), ("contour", 0.15),
            ("f_attack", 0.45), ("a_attack", 0.6), ("a_sustain", 0.9), ("a_release", 0.7),
            ("ensemble", ENS_II), ("ens_width", 0.7), ("volume", 0.62),
        ],
    },
    Factory {
        name: "Dark Pad",
        about: "sixteen voices, a saw an octave under the other, the filter low and the big ensemble",
        set: &[
            ("voices", voices(16)), ("o2_range", RANGE_16), ("o2_wave", SAW), ("o2_tune", cents(4.0)),
            ("mix_o2", 0.65), ("cutoff", 0.36), ("emphasis", 0.12), ("contour", 0.15),
            ("f_attack", 0.6), ("f_sustain", 0.7), ("a_attack", 0.65), ("a_sustain", 0.9),
            ("a_release", 0.7), ("ensemble", ENS_III), ("ens_width", 0.8), ("drift", 0.6),
            ("spread", 0.5), ("volume", 0.68),
        ],
    },
    Factory {
        name: "Hollow Pad",
        about: "squares an octave apart through the notch, the empty middle that reads as distance",
        set: &[
            ("filter_model", SEM), ("sem_mode", 0.5), ("emphasis", 0.45), ("o1_wave", SQUARE),
            ("o2_range", RANGE_4), ("o2_wave", SQUARE), ("o2_tune", cents(5.0)), ("mix_o2", 0.4),
            ("cutoff", 0.5), ("contour", 0.1), ("f_attack", 0.5), ("a_attack", 0.5),
            ("a_sustain", 0.9), ("a_release", 0.6), ("ensemble", ENS_I), ("drift", 0.4),
            ("volume", 0.56),
        ],
    },
    // ── plucks ──
    Factory {
        name: "Pluck",
        about: "a saw with a square an octave up, the contour snapping shut",
        set: &[
            ("o2_range", RANGE_4), ("o2_wave", SQUARE), ("mix_o2", 0.3), ("cutoff", 0.45),
            ("emphasis", 0.3), ("contour", 0.6), ("f_decay", 0.3), ("f_sustain", 0.0),
            ("a_decay", 0.5), ("a_sustain", 0.0), ("a_release", 0.3), ("spread", 0.4),
            ("vel_filter", 0.4),
            ("volume", 0.91),
        ],
    },
    Factory {
        name: "Harp",
        about: "a triangle and a saw two octaves up, plucked through the SEM, a little chorus",
        set: &[
            ("filter_model", SEM), ("o1_wave", TRI), ("mix_o1", 0.8), ("o2_range", RANGE_4),
            ("o2_wave", SAW), ("mix_o2", 0.3), ("cutoff", 0.55), ("emphasis", 0.25),
            ("contour", 0.5), ("f_decay", 0.35), ("f_sustain", 0.0), ("a_decay", 0.55),
            ("a_sustain", 0.0), ("a_release", 0.35), ("ensemble", ENS_I), ("spread", 0.5),
        ],
    },
    Factory {
        name: "Clav Pluck",
        about: "two narrow pulses, driven, a short sharp filter: the funk one",
        set: &[
            ("o1_wave", NARROW), ("o2_wave", NARROW), ("o2_tune", cents(9.0)), ("mix_o2", 0.6),
            ("cutoff", 0.6), ("emphasis", 0.4), ("contour", 0.5), ("f_decay", 0.25),
            ("f_sustain", 0.05), ("a_decay", 0.4), ("a_sustain", 0.2), ("a_release", 0.15),
            ("drive", 0.2), ("vel_filter", 0.5),
            ("volume", 0.93),
        ],
    },
    // ── strings ──
    Factory {
        name: "Strings",
        about: "sixteen slow saws, drifting, through the second ensemble setting",
        set: &[
            ("voices", voices(16)), ("o2_wave", SAW), ("o2_tune", cents(5.0)), ("mix_o2", 0.7),
            ("cutoff", 0.55), ("emphasis", 0.05), ("contour", 0.15), ("f_attack", 0.4),
            ("f_sustain", 0.7), ("a_attack", 0.5), ("a_sustain", 0.9), ("a_release", 0.55),
            ("ensemble", ENS_II), ("ens_width", 0.8), ("drift", 0.45), ("lfo_delay", 0.5),
            ("osc_mod", ON), ("mod_depth", 0.12), ("volume", 0.6),
        ],
    },
    Factory {
        name: "Ensemble Strings",
        about: "three saws at three feet through the big ensemble: the string machine",
        set: &[
            ("voices", voices(12)), ("o2_range", RANGE_4), ("o2_wave", SAW), ("mix_o2", 0.5),
            ("o3_range", RANGE_16), ("o3_wave", SAW), ("mix_o3", 0.3), ("cutoff", 0.6),
            ("emphasis", 0.0), ("contour", 0.1), ("f_sustain", 0.8), ("a_attack", 0.45),
            ("a_sustain", 0.9), ("a_release", 0.5), ("ensemble", ENS_III), ("ens_width", 0.9),
            ("drift", 0.5), ("volume", 0.58),
        ],
    },
    // ── arpeggios ──
    Factory {
        name: "Arp Pluck",
        about: "the pluck walked up two octaves in sixteenths, held",
        set: &[
            ("o2_range", RANGE_4), ("o2_wave", SQUARE), ("mix_o2", 0.3), ("cutoff", 0.5),
            ("emphasis", 0.3), ("contour", 0.55), ("f_decay", 0.3), ("f_sustain", 0.0),
            ("a_decay", 0.4), ("a_sustain", 0.0), ("a_release", 0.25), ("arp_mode", ARP_UP),
            ("arp_rate", RATE_16TH), ("arp_oct", 1.0 / 3.0), ("arp_gate", 0.4), ("arp_hold", ON),
            ("spread", 0.5),
            ("volume", 0.99),
        ],
    },
    Factory {
        name: "Arp Bass Line",
        about: "a square and a saw at 16 feet, up and down in eighths, one octave",
        set: &[
            ("voice_mode", MONO), ("o1_range", RANGE_16), ("o1_wave", SQUARE), ("o2_range", RANGE_16),
            ("o2_wave", SAW), ("mix_o2", 0.6), ("cutoff", 0.42), ("emphasis", 0.25),
            ("contour", 0.6), ("f_decay", 0.35), ("f_sustain", 0.1), ("a_decay", 0.5),
            ("a_sustain", 0.6), ("a_release", 0.12), ("arp_mode", ARP_UP_DOWN),
            ("arp_rate", RATE_8TH), ("arp_oct", 0.0), ("arp_gate", 0.6), ("arp_hold", ON),
            ("volume", 0.93),
        ],
    },
];

// Silence the "unused" lint on the positions the table happens not to use
// this release: the words are the point, and the next preset will.
#[allow(dead_code)]
const UNUSED_POSITIONS: [f32; 6] = [RANGE_LO, TRI_SAW, ENS_OFF, LFO_TRI, OFF, LADDER];

/// A quick way to say "every dial in this preset, by id", for the tests
/// and the bench.
pub fn slots(f: &Factory) -> [f32; PARAM_SLOTS] {
    let mut out = defaults();
    for (id, v) in f.by_id() {
        if let Some(p) = spec(id) {
            out[p.id as usize] = v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::arp::Transport;
    use crate::analog::measure::{db, peak};
    use crate::analog::{Analog, Event};

    /// Every preset names only dials the table has, keeps every value on
    /// the dial, has a name of its own, and the set covers the brief's
    /// spread: mono, unison and poly all present, both filters, the
    /// ensemble and the arpeggiator each in use.
    #[test]
    fn the_factory_names_real_dials_and_covers_the_brief() {
        assert_eq!(FACTORY.len(), 24);
        let mut names: Vec<&str> = FACTORY.iter().map(|f| f.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 24, "a name repeats");
        for f in &FACTORY {
            assert!(!f.about.is_empty(), "{} says nothing about itself", f.name);
            for (s, v) in f.set {
                assert!(PARAMS.iter().any(|p| p.symbol == *s), "{}: no dial called {s}", f.name);
                assert!((0.0..=1.0).contains(v), "{}: {s} = {v} is off the dial", f.name);
                let p = PARAMS.iter().find(|p| p.symbol == *s).unwrap();
                if p.steps > 0 {
                    let last = (p.steps - 1) as f32;
                    let on_step = ((v * last).round() / last - v).abs() < 1e-5;
                    assert!(on_step, "{}: {s} = {v} sits between positions", f.name);
                }
            }
            let mut seen: Vec<&str> = f.set.iter().map(|(s, _)| *s).collect();
            seen.sort_unstable();
            let n = seen.len();
            seen.dedup();
            assert_eq!(seen.len(), n, "{} sets a dial twice", f.name);
            assert_eq!(f.values().len(), PARAMS.len());
            for word in ["Moog", "Minimoog", "Oberheim", "Taurus", "Solina", "Prophet", "Roland", "Juno", "ARP "] {
                assert!(!f.name.contains(word), "{} wears somebody else's name", f.name);
            }
        }
        let mode = |f: &Factory, sym: &str| f.values().iter().find(|(s, _)| *s == sym).unwrap().1;
        assert!(FACTORY.iter().any(|f| mode(f, "voice_mode") == MONO));
        assert!(FACTORY.iter().any(|f| mode(f, "voice_mode") == UNISON));
        assert!(FACTORY.iter().any(|f| mode(f, "voice_mode") == POLY));
        assert!(FACTORY.iter().any(|f| mode(f, "filter_model") == SEM));
        assert!(FACTORY.iter().any(|f| mode(f, "ensemble") > 0.0));
        assert_eq!(FACTORY.iter().filter(|f| mode(f, "arp_mode") > 0.0).count(), 2);
    }

    /// Render a preset with a phrase suited to its kind — a held chord for
    /// the poly sounds, a line for the mono ones — and return the mono mix.
    pub(crate) fn audition(f: &Factory, secs: f32) -> Vec<f32> {
        const SR: f32 = 48_000.0;
        const BLOCK: usize = 128;
        let mut synth = Analog::new(SR);
        synth.set_all(&f.by_id());
        let mono = f.values().iter().any(|(s, v)| *s == "voice_mode" && *v < 0.25);
        let frames = (secs * SR) as usize;
        let mut out = vec![0.0_f32; frames * 2];
        let mut block = vec![0.0_f32; BLOCK * 2];
        let notes: Vec<(usize, u8, bool)> = if mono {
            // A line: four notes, the last held.
            vec![
                (0, 45, true), ((0.4 * SR) as usize, 45, false),
                ((0.4 * SR) as usize, 52, true), ((0.8 * SR) as usize, 52, false),
                ((0.8 * SR) as usize, 48, true), ((1.2 * SR) as usize, 48, false),
                ((1.2 * SR) as usize, 57, true), ((secs * 0.75 * SR) as usize, 57, false),
            ]
        } else {
            // A chord, hands not quite together, held most of the way.
            let off = (secs * 0.7 * SR) as usize;
            vec![
                (0, 48, true), (400, 55, true), (800, 60, true), (1200, 64, true), (1600, 67, true),
                (off, 48, false), (off, 55, false), (off, 60, false), (off, 64, false), (off, 67, false),
            ]
        };
        let mut at = 0usize;
        while at < frames {
            let n = BLOCK.min(frames - at);
            let evs: Vec<Event> = notes
                .iter()
                .filter(|(t, _, _)| *t >= at && *t < at + n)
                .map(|(t, k, on)| {
                    if *on {
                        Event::NoteOn { at: t - at, key: *k, vel: 0.85 }
                    } else {
                        Event::NoteOff { at: t - at, key: *k }
                    }
                })
                .collect();
            synth.render(&mut block, n, 2, &evs, Transport::default());
            out[at * 2..(at + n) * 2].copy_from_slice(&block[..n * 2]);
            at += n;
        }
        out.chunks_exact(2).map(|f| 0.5 * (f[0] + f[1])).collect()
    }

    /// **Every preset plays, clip-free, under the ceiling and above the
    /// floor**: a chord or a line through each one peaks between −18 and
    /// −1 dBFS, with every sample finite. A preset that clips or one that
    /// whispers is a preset nobody keeps.
    #[test]
    fn every_factory_preset_plays_clean_and_audible() {
        let mut bad = Vec::new();
        for f in &FACTORY {
            let x = audition(f, 2.0);
            assert!(x.iter().all(|v| v.is_finite()), "{}: non-finite", f.name);
            let pk = db(peak(&x));
            if !(-18.0..=-1.0).contains(&pk) {
                bad.push(format!("{}: peak {pk:+.1} dBFS", f.name));
            }
        }
        assert!(bad.is_empty(), "presets off the level window:\n  {}", bad.join("\n  "));
    }
}
