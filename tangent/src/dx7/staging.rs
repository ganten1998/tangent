//! The bank being BUILT: thirty-two numbered slots, empty until somebody
//! puts a patch in one, written out as an ordinary cartridge.
//!
//! # Why a second bank
//!
//! A cartridge is thirty-two patches in a fixed order, and the ones people
//! collect were assembled by somebody else: the six sounds worth keeping out
//! of ROM1A sit beside twenty-six that are not, and the two out of a trader's
//! bank sit in a different file. The owner's ask was a place to mix them —
//! "stage their perfect bank" — and then have that bank be the one the
//! instrument opens on. So the picker grew a second pane. The left is a bank
//! read off a disk and never written to; the right is this, and only this is
//! ever saved.
//!
//! # Slots, not a list
//!
//! Numbered from the start rather than appended to, because a DX7 stores a
//! voice TO a number and a cartridge sheet is printed as A1 through B16: what
//! somebody drags onto slot 7 goes in slot 7, and a slot they have not filled
//! stays empty rather than closing up. The empties are written as the
//! instrument's own initial voice, which is what a real cartridge holds where
//! nothing was stored, and what `Cartridge::of` has always padded with.
//!
//! # On the synth's side of the firewall
//!
//! `tangent-ui` cannot name a [`Voice`]. It holds thirty-two `Option<String>`
//! mirrors of what is here, moves names around by index, and the host applies
//! the same moves to the voices — the arrangement the cartridge list and the
//! patch editor already use, so nothing about a patch ever crosses the wall.

use super::sysex::Cartridge;
use super::voice::Voice;

/// The folder under the media root where banks are kept, and where both
/// panels open: `~/Documents/Tangent Media/SYSEX`. The UI spells the same
/// name in `Settings::sysex_root`, which is the one that resolves the
/// media root; `the_two_crates_agree_on_the_sysex_shelf` holds them together.
pub const SYSEX_FOLDER: &str = "SYSEX";

/// Slots in a bank. A cartridge's thirty-two, and not one more.
pub const SLOTS: usize = 32;

/// The bank under construction.
#[derive(Debug, Clone, PartialEq)]
pub struct Staging {
    slots: Vec<Option<Voice>>,
}

impl Default for Staging {
    fn default() -> Self {
        Self::new()
    }
}

impl Staging {
    /// Thirty-two empty slots.
    pub fn new() -> Self {
        Self {
            slots: vec![None; SLOTS],
        }
    }

    /// What is in slot `i`, if anything.
    pub fn get(&self, i: usize) -> Option<&Voice> {
        self.slots.get(i).and_then(Option::as_ref)
    }

    /// The first empty slot, if the bank is not full.
    pub fn first_free(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }

    /// How many slots hold a patch.
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// Put `v` in slot `i`. Out of range is ignored rather than clamped into
    /// a neighbour, for the reason `edit::apply` gives: an index this build
    /// does not have is a UI built against a different one, and writing it
    /// somewhere else would overwrite a patch nobody meant to touch.
    pub fn set(&mut self, i: usize, v: Voice) {
        if let Some(slot) = self.slots.get_mut(i) {
            *slot = Some(v);
        }
    }

    /// Put `voices` into the bank and say how many landed.
    ///
    /// **With a target, they go from that slot on and OVERWRITE.** A drop on
    /// slot 7 with three patches in hand fills 7, 8 and 9, whatever was in
    /// them, and stops at the last slot rather than wrapping — the same thing
    /// storing to a numbered slot on the instrument does, and the only rule
    /// under which what somebody aimed at is what they get.
    ///
    /// **Without one, they go to the first EMPTY slots**, leaving what is
    /// already staged where it is. That is the "Stage" button and the
    /// right-click road: no slot was named, so nothing is overwritten.
    pub fn stage(&mut self, voices: &[Voice], at: Option<usize>) -> usize {
        let mut landed = 0;
        match at {
            Some(at) => {
                for (k, v) in voices.iter().enumerate() {
                    let Some(slot) = self.slots.get_mut(at + k) else { break };
                    *slot = Some(*v);
                    landed += 1;
                }
            }
            None => {
                for v in voices {
                    let Some(free) = self.first_free() else { break };
                    self.slots[free] = Some(*v);
                    landed += 1;
                }
            }
        }
        landed
    }

    /// Empty these slots. The others keep their numbers: a bank is a set of
    /// numbered slots, not a list that closes up.
    pub fn unstage(&mut self, slots: &[usize]) {
        for &i in slots {
            if let Some(slot) = self.slots.get_mut(i) {
                *slot = None;
            }
        }
    }

    /// Exchange two slots — a drag within the staged pane. Either may be
    /// empty, which is how a patch is MOVED to an empty slot with the same
    /// gesture.
    pub fn swap(&mut self, a: usize, b: usize) {
        if a < self.slots.len() && b < self.slots.len() {
            self.slots.swap(a, b);
        }
    }

    /// Every slot's name, for the picker's mirror. `None` where a slot is
    /// empty, so the picker can draw the number and nothing beside it rather
    /// than thirty-two rows saying INIT VOICE.
    pub fn names(&self) -> Vec<Option<String>> {
        self.slots
            .iter()
            .map(|s| s.as_ref().map(Voice::display_name))
            .collect()
    }

    /// The bank as a cartridge, empties filled with the initial voice.
    pub fn to_cartridge(&self, name: &str) -> Cartridge {
        Cartridge::of(
            name,
            self.slots.iter().map(|s| s.unwrap_or_default()).collect(),
        )
    }
}

/// The bank the instrument opens on when a strip names no cartridge of its
/// own: `default.syx` in the SYSEX shelf if the user has saved one there,
/// and the bank compiled into the app if not.
///
/// **A file first and the binary second**, which is the whole point of the
/// shelf: the owner's words were "so the user can have their chosen load
/// every time". The second value says which it was, so the picker can call
/// the bank by its right name and the log can say what was read. A file that
/// is there but will not parse is treated as not there — the fresh-install
/// sound is a better answer than silence, and the picker's "Load bank" will
/// show the parse error the moment somebody opens the file by hand.
pub fn default_bank_at(shelf: &std::path::Path) -> (Cartridge, bool) {
    let path = shelf.join(tangent_ui::dialogs::DEFAULT_BANK_FILE);
    match Cartridge::load(&path) {
        Ok(c) => (c, true),
        Err(_) => (super::sysex::factory(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(n: &str) -> Voice {
        let mut v = Voice::default();
        v.set_name(n);
        v
    }

    /// A fresh bank is thirty-two empties, and the mirror says so slot by
    /// slot rather than as a shorter list — the picker numbers them.
    #[test]
    fn a_fresh_bank_is_thirty_two_empty_slots() {
        let s = Staging::new();
        assert_eq!(s.names().len(), SLOTS);
        assert!(s.names().iter().all(Option::is_none));
        assert_eq!(s.count(), 0);
        assert_eq!(s.first_free(), Some(0));
    }

    /// **Aimed at a slot, patches land THERE and overwrite; aimed at nothing,
    /// they take the first empties and overwrite nothing.** The two roads
    /// into the bank — a drop and the Stage button — and the rule each one
    /// keeps.
    ///
    /// Revert-check performed: making the aimed road skip occupied slots
    /// (`if slot.is_none()`) reddens the first half; making the unaimed road
    /// start at 0 without looking reddens the second.
    #[test]
    fn a_drop_overwrites_from_its_slot_and_a_stage_fills_the_gaps() {
        let mut s = Staging::new();
        s.set(7, named("OLD 7"));
        s.set(9, named("OLD 9"));
        let landed = s.stage(&[named("A"), named("B"), named("C")], Some(7));
        assert_eq!(landed, 3);
        assert_eq!(s.get(7).map(Voice::display_name).as_deref(), Some("A"));
        assert_eq!(s.get(8).map(Voice::display_name).as_deref(), Some("B"));
        assert_eq!(
            s.get(9).map(Voice::display_name).as_deref(),
            Some("C"),
            "a drop must overwrite the slot it was aimed at"
        );

        let mut s = Staging::new();
        s.set(0, named("KEEP 0"));
        s.set(2, named("KEEP 2"));
        let landed = s.stage(&[named("X"), named("Y")], None);
        assert_eq!(landed, 2);
        assert_eq!(s.get(0).map(Voice::display_name).as_deref(), Some("KEEP 0"));
        assert_eq!(s.get(1).map(Voice::display_name).as_deref(), Some("X"));
        assert_eq!(s.get(2).map(Voice::display_name).as_deref(), Some("KEEP 2"));
        assert_eq!(s.get(3).map(Voice::display_name).as_deref(), Some("Y"));
    }

    /// A drop near the end stops at slot 32 and does not wrap to slot 1, and
    /// a full bank takes nothing more from the unaimed road. Both are counted
    /// honestly so the note can say "2 of 3 landed".
    #[test]
    fn the_bank_ends_at_thirty_two_and_nothing_wraps() {
        let mut s = Staging::new();
        let landed = s.stage(&[named("A"), named("B"), named("C")], Some(30));
        assert_eq!(landed, 2);
        assert!(s.get(0).is_none(), "the third patch wrapped round to slot 1");

        let mut s = Staging::new();
        for i in 0..SLOTS {
            s.set(i, named(&format!("V{i}")));
        }
        assert_eq!(s.first_free(), None);
        assert_eq!(s.stage(&[named("MORE")], None), 0);
        assert_eq!(s.count(), SLOTS);
    }

    /// Removing leaves a hole rather than closing the list up — slot numbers
    /// are the thing somebody aimed at — and a swap moves a patch into an
    /// empty slot with the same call it exchanges two with.
    #[test]
    fn unstaging_leaves_a_hole_and_swapping_moves_into_one() {
        let mut s = Staging::new();
        s.set(0, named("A"));
        s.set(1, named("B"));
        s.set(2, named("C"));
        s.unstage(&[1]);
        assert!(s.get(1).is_none());
        assert_eq!(s.get(2).map(Voice::display_name).as_deref(), Some("C"));
        s.swap(2, 5);
        assert!(s.get(2).is_none());
        assert_eq!(s.get(5).map(Voice::display_name).as_deref(), Some("C"));
        // Out of range is ignored, never clamped onto a neighbour.
        s.swap(0, SLOTS);
        s.unstage(&[SLOTS + 4]);
        s.set(SLOTS, named("LOST"));
        assert_eq!(s.get(0).map(Voice::display_name).as_deref(), Some("A"));
        assert_eq!(s.count(), 2);
    }

    /// **What is written is a cartridge**: the same parser that reads other
    /// people's banks reads this one back, the filled slots are where they
    /// were put, and the empties are the initial voice rather than garbage.
    #[test]
    fn a_staged_bank_writes_out_as_a_cartridge_with_its_slots_in_place() {
        let mut s = Staging::new();
        s.set(3, named("THREE"));
        s.set(31, named("LAST"));
        let cart = s.to_cartridge("mine");
        let back = Cartridge::parse(&cart.to_bytes(), "mine").expect("a cartridge");
        assert!(back.checksum_ok);
        assert_eq!(back.names()[3], "THREE");
        assert_eq!(back.names()[31], "LAST");
        assert_eq!(back.voices[0], Voice::default());
        assert_eq!(back.voices[17], Voice::default());
    }

    /// **The default bank is the file when there is one and the binary when
    /// there is not**, and a file that will not parse counts as not there.
    ///
    /// Against a folder made here, never the user's real shelf: a test that
    /// read `~/Documents` would pass or fail on what the owner happened to
    /// have saved that week.
    ///
    /// Revert-check performed: answering `factory()` unconditionally reddens
    /// the middle assertion; answering the file unconditionally panics on the
    /// first call, where there is no file to answer with.
    #[test]
    fn the_default_bank_is_the_file_when_there_is_one_and_the_binary_when_not() {
        let shelf = std::env::temp_dir().join(format!("tangent-sysex-shelf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&shelf);
        std::fs::create_dir_all(&shelf).expect("a shelf");
        let file = shelf.join(tangent_ui::dialogs::DEFAULT_BANK_FILE);

        let (cart, from_file) = default_bank_at(&shelf);
        assert!(!from_file);
        assert_eq!(cart.name, super::super::sysex::FACTORY_NAME);

        let mut mine = Staging::new();
        mine.set(0, named("MY DEFAULT"));
        mine.to_cartridge("default").save(&file).expect("saved");
        let (cart, from_file) = default_bank_at(&shelf);
        assert!(from_file, "a default.syx on the shelf was not read");
        assert_eq!(cart.names()[0], "MY DEFAULT");
        assert_eq!(cart.name, "default", "the picker calls it by the file's stem");

        std::fs::write(&file, b"not a cartridge at all").expect("clobbered");
        let (cart, from_file) = default_bank_at(&shelf);
        assert!(!from_file, "a file that does not parse must not be the default");
        assert_eq!(cart.name, super::super::sysex::FACTORY_NAME);
        let _ = std::fs::remove_dir_all(&shelf);
    }

    /// The shelf's name and the default file's name are spelled in two
    /// crates — the UI resolves the folder and suggests the file name, the
    /// host reads both — and a drift between them is a bank saved where the
    /// next launch does not look. Pinned here, once.
    #[test]
    fn the_two_crates_agree_on_the_sysex_shelf() {
        let s = tangent_ui::settings::Settings::default();
        assert_eq!(s.sysex_root(), s.media_root().join(SYSEX_FOLDER));
        assert_eq!(tangent_ui::dialogs::DEFAULT_BANK_FILE, "default.syx");
    }
}
