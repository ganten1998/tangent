//! Spelling a chord's notes from its name.
//!
//! A pitch class has two names on a black key and the right one is not a
//! matter of taste once a chord has been named: in D7 the third is F sharp,
//! because the third of D is an F of some kind, and calling it G flat writes
//! a diminished fourth. The user's sharps-or-flats preference is for the
//! cases nothing else decides; a named chord decides every one of its notes.
//!
//! So each note is spelled by its DEGREE above the root: the letter that many
//! steps up from the root's letter, bent by whatever it takes to reach the
//! pitch. The degree comes from the interval, read in the chord's context
//! where an interval has two meanings (three semitones is a #9 beside a major
//! third and a minor third without one; six is a b5 in a diminished chord and
//! a #11 everywhere else; eight is a #5 in an augmented chord and a b13
//! otherwise).
//!
//! Only single sharps and flats are produced. A degree that would need a
//! double accidental (the bb7 of a diminished seventh, the #9 of G#7) has no
//! spelling here, and the caller falls back to its own rule for that note.

/// The natural pitch class of each letter, C through B.
const NATURAL_PC: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const LETTERS: [char; 7] = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];

/// A named chord's spelling: the root's letter and, for each interval above
/// the root, which degree (in letters above the root's) names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ChordSpelling {
    root_pc: u8,
    /// 0 = C .. 6 = B.
    root_letter: u8,
    /// Letters above the root's letter, for each interval 0..=11.
    degree: [u8; 12],
}

impl ChordSpelling {
    /// Read the spelling off one of the detector's chord labels.
    ///
    /// `None` for anything that is not a chord name: an empty label, a scale
    /// or an interval readout (both have a space in them, "C Whole Tone",
    /// "C (M3)"), or text that does not start with a note name.
    pub fn from_label(label: &str) -> Option<ChordSpelling> {
        if label.contains(' ') {
            return None;
        }
        let mut chars = label.chars();
        let letter = chars.next()?;
        let root_letter = LETTERS.iter().position(|&l| l == letter)? as u8;
        let natural = NATURAL_PC[root_letter as usize];
        let rest = chars.as_str();
        let (root_pc, quality) = if let Some(q) = rest.strip_prefix('#') {
            ((natural + 1) % 12, q)
        } else if let Some(q) = rest.strip_prefix('b') {
            ((natural + 11) % 12, q)
        } else {
            (natural, rest)
        };
        // The slash bass is a note of the chord, spelled like the others.
        let quality = quality.split('/').next().unwrap_or("");
        Some(ChordSpelling {
            root_pc,
            root_letter,
            degree: degrees(quality),
        })
    }

    pub fn root_pc(&self) -> u8 {
        self.root_pc
    }

    /// The same chord with a black-key root spelled the other way: flats for
    /// `true`, sharps for `false`. A white-key root is left alone.
    ///
    /// For a context that knows better than the label which way the root goes,
    /// such as a key signature: the detector names roots by the user's
    /// preference alone, and an A flat chord in E flat major is not a G sharp
    /// chord because the preference says sharps.
    pub fn with_root_flats(self, flats: bool) -> ChordSpelling {
        if NATURAL_PC.contains(&self.root_pc) {
            return self;
        }
        let letter = if flats {
            NATURAL_PC.iter().position(|&n| n == (self.root_pc + 1) % 12)
        } else {
            NATURAL_PC.iter().position(|&n| (n + 1) % 12 == self.root_pc)
        };
        ChordSpelling {
            root_letter: letter.unwrap_or(self.root_letter as usize) as u8,
            ..self
        }
    }

    /// The letter (0 = C .. 6 = B) and alteration (-1, 0 or +1) that spell
    /// pitch class `pc` in this chord, or `None` when that would take a double
    /// accidental.
    pub fn letter(&self, pc: u8) -> Option<(u8, i8)> {
        let interval = (pc % 12 + 12 - self.root_pc) % 12;
        let letter = (self.root_letter + self.degree[interval as usize]) % 7;
        let alter = (i32::from(pc % 12) - i32::from(NATURAL_PC[letter as usize]) + 6)
            .rem_euclid(12)
            - 6;
        (alter.abs() <= 1).then_some((letter, alter as i8))
    }

    /// `pc`'s name in this chord, ASCII accidentals ("F#", "Eb", "B").
    pub fn name(&self, pc: u8) -> Option<String> {
        let (letter, alter) = self.letter(pc)?;
        let mut s = LETTERS[letter as usize].to_string();
        match alter {
            1 => s.push('#'),
            -1 => s.push('b'),
            _ => {}
        }
        Some(s)
    }
}

/// Which degree names each interval, given the chord's quality text.
///
/// The degree is counted in letters above the root: 0 the root, 1 a second or
/// ninth, 2 a third, 3 a fourth or eleventh, 4 a fifth, 5 a sixth or
/// thirteenth, 6 a seventh.
fn degrees(quality: &str) -> [u8; 12] {
    let diminished = quality.contains('°') || quality.contains('ø') || quality.contains("b5");
    let augmented = quality.starts_with('+') || quality.contains("#5");
    let minor = quality.starts_with('m') && !quality.starts_with("maj");
    let major_third = !minor && !diminished && !quality.contains("sus")
        && !matches!(quality.chars().next(), Some('2' | '4' | '5'));
    [
        0, // root
        1, // b9
        1, // 9
        // A #9 beside a major third, a minor third otherwise.
        if major_third { 1 } else { 2 },
        2, // major third
        3, // 11
        // A b5 in a diminished chord, a #11 everywhere else.
        if diminished { 4 } else { 3 },
        4, // fifth
        // A #5 in an augmented chord, a b13 (or b6) everywhere else.
        if augmented { 4 } else { 5 },
        5, // 6 / 13 (and the bb7 of a diminished seventh, as a sixth)
        6, // b7
        6, // major seventh
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(label: &str, pcs: &[u8]) -> Vec<String> {
        let s = ChordSpelling::from_label(label).expect(label);
        pcs.iter().map(|&pc| s.name(pc).unwrap_or_else(|| "?".into())).collect()
    }

    /// The case that started this: D F# C Eb Ab B, named D7(b9,#11). Every
    /// note is named for what it is IN that chord.
    #[test]
    fn d7_b9_sharp11_with_its_thirteenth_is_spelled_from_d() {
        assert_eq!(
            names("D7(b9,#11)", &[2, 6, 0, 3, 8, 11]),
            ["D", "F#", "C", "Eb", "G#", "B"],
        );
    }

    /// The other reading of the same notes spells them the other way, which
    /// is the point: the spelling belongs to the name, not to the notes.
    #[test]
    fn ab7_sharp9_sharp11_is_spelled_from_a_flat() {
        assert_eq!(
            names("Ab7(#9,#11)", &[8, 0, 3, 6, 11, 2]),
            ["Ab", "C", "Eb", "Gb", "B", "D"],
        );
    }

    #[test]
    fn the_two_readings_of_an_ambiguous_interval_follow_the_quality() {
        // Three semitones: a #9 beside a major third, a minor third without.
        assert_eq!(names("C7(#9)", &[3, 4]), ["D#", "E"]);
        assert_eq!(names("Cm7", &[3]), ["Eb"]);
        // Six: a b5 in a diminished chord, a #11 in anything else.
        assert_eq!(names("Cø7", &[6]), ["Gb"]);
        assert_eq!(names("C°", &[6]), ["Gb"]);
        assert_eq!(names("CΔ7(#11)", &[6]), ["F#"]);
        // Eight: a #5 in an augmented chord, a b13 otherwise.
        assert_eq!(names("C+", &[8]), ["G#"]);
        assert_eq!(names("CΔ7#5", &[8]), ["G#"]);
        assert_eq!(names("C7(b13)", &[8]), ["Ab"]);
    }

    #[test]
    fn a_flat_root_keeps_its_chord_in_flats_and_a_sharp_root_in_sharps() {
        assert_eq!(names("Eb7", &[3, 7, 10, 1]), ["Eb", "G", "Bb", "Db"]);
        assert_eq!(names("F#m", &[6, 9, 1]), ["F#", "A", "C#"]);
        assert_eq!(names("Gb", &[6, 10, 1]), ["Gb", "Bb", "Db"]);
    }

    #[test]
    fn a_degree_that_needs_a_double_accidental_is_left_to_the_caller() {
        // The #9 of G# would be A double sharp.
        let s = ChordSpelling::from_label("G#7(#9)").unwrap();
        assert_eq!(s.name(11), None);
        assert_eq!(s.name(0), Some("B#".into()));
    }

    #[test]
    fn a_slash_bass_is_spelled_as_a_note_of_the_chord() {
        assert_eq!(names("D7/F#", &[6]), ["F#"]);
    }

    #[test]
    fn a_key_can_turn_the_root_around() {
        let s = ChordSpelling::from_label("G#").unwrap().with_root_flats(true);
        assert_eq!([s.name(8), s.name(0), s.name(3)], [Some("Ab".into()), Some("C".into()), Some("Eb".into())]);
        let s = ChordSpelling::from_label("D").unwrap().with_root_flats(true);
        assert_eq!(s.name(6), Some("F#".into()));
    }

    #[test]
    fn scales_intervals_and_text_are_not_chords() {
        for label in ["C Whole Tone", "C (M3)", "Chromatic Scale", "", "hello"] {
            assert_eq!(ChordSpelling::from_label(label), None, "{label:?}");
        }
    }
}
