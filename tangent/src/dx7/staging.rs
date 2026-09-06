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
//! stays empty rather than closing up. When the bank is written, the empties
//! go out as `Voice::default()` — the INIT VOICE, which is what a real
//! cartridge holds where nothing was stored and what `Cartridge::of` pads
//! with — because the format has no way to say "nothing here". That is also
//! why the bank cannot be read back from a cartridge alone: a slot holding
//! INIT VOICE might be empty or might be an INIT VOICE somebody staged to
//! start from, and only the slot mask beside the file knows which.
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

    /// The bank as a cartridge, empties filled with INIT VOICE.
    pub fn to_cartridge(&self, name: &str) -> Cartridge {
        Cartridge::of(
            name,
            self.slots.iter().map(|s| s.unwrap_or_default()).collect(),
        )
    }

    /// The occupied slots, in order.
    pub fn filled(&self) -> Vec<usize> {
        (0..self.slots.len()).filter(|&i| self.slots[i].is_some()).collect()
    }

    /// Write the bank down under `dir`, as a cartridge and a slot mask.
    ///
    /// **Two files, because one cannot say it.** The cartridge is an ordinary
    /// one, and an ordinary cartridge has thirty-two voices in it whatever
    /// was staged: an empty slot goes out as INIT VOICE. The mask — thirty-
    /// two characters, `1` where a slot holds something — is what tells that
    /// from an INIT VOICE somebody staged on purpose when the bank comes
    /// back. Written on every change rather than at quit, because
    /// quit is the one moment a crash, a kill or a power cut is guaranteed to
    /// miss; the owner reported the click sound resetting and the staged
    /// bank dying in the same day, and "state the user built survives a
    /// restart" is the rule now, not the exception.
    pub fn save_to(&self, dir: &std::path::Path) -> Result<(), String> {
        self.to_cartridge("staged").save(&dir.join(STAGED_FILE))?;
        let mask: String = self
            .slots
            .iter()
            .map(|s| if s.is_some() { '1' } else { '0' })
            .collect();
        let path = dir.join(STAGED_SLOTS_FILE);
        std::fs::write(&path, mask).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The bank written by [`Self::save_to`], or `None` when there is none.
    ///
    /// A cartridge without its mask — somebody deleted one file of the two —
    /// is read as generously as it can be: every slot that is not INIT VOICE
    /// is taken as filled. A cartridge that will not parse
    /// is no bank at all, and the session starts empty rather than refusing
    /// to start.
    pub fn restore_from(dir: &std::path::Path) -> Option<Self> {
        let cart = Cartridge::load(&dir.join(STAGED_FILE)).ok()?;
        let mask = std::fs::read_to_string(dir.join(STAGED_SLOTS_FILE)).ok();
        let slots = cart
            .voices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let filled = match &mask {
                    Some(m) => m.as_bytes().get(i) == Some(&b'1'),
                    None => *v != Voice::default(),
                };
                filled.then_some(*v)
            })
            .collect();
        Some(Self { slots })
    }
}

/// Where the staged bank is written down between sessions, under the
/// config directory beside the settings: an ordinary cartridge, and the
/// slot mask that tells its empties from its pianos.
pub const STAGED_FILE: &str = "staged.syx";
pub const STAGED_SLOTS_FILE: &str = "staged.slots";

/// The bank the patch editor's Save used to append to, beside the settings,
/// before the staged bank existed. Nothing reads it any more; `prepare_shelf`
/// copies it onto the SYSEX shelf once so "Load bank..." can reach it.
pub const OLD_USER_BANK: &str = "my-patches.syx";

/// A bank path as the save road will write it: `.syx` added to a bare name,
/// anything with an extension left alone. On macOS the save panel is an open
/// panel with a name field (`macwin::save_panel`) that, for a bank, refuses
/// nothing and appends nothing, so "default" typed by hand has to become
/// `default.syx` somewhere — and the same somewhere has to be asked whether
/// that file already exists, which is why this is a function and not two
/// lines in two places.
pub fn bank_file(path: std::path::PathBuf) -> std::path::PathBuf {
    let mut path = path;
    if path.extension().is_none() {
        path.set_extension("syx");
    }
    path
}

/// **Make the SYSEX shelf, and put last year's patches on it.** Once per
/// launch, idempotent.
///
/// The folder is made here rather than by the first panel that opens on it,
/// because the instruction "put your `default.syx` in `Tangent Media/SYSEX`"
/// needs the folder to exist before anybody has pressed Save bank — the
/// chicken-and-egg the lazy version had.
///
/// `old_bank` is `my-patches.syx` beside the settings: every patch the
/// editor's Save wrote before the staged bank existed. The Save goes to the
/// staged bank now and nothing reads that file, so without this those
/// patches would be on the disk and unreachable from the app. It is COPIED
/// onto the shelf, never moved, and never over a file already there — a
/// `my-patches.syx` the user made themselves is theirs. `Some(dst)` says a
/// copy was made; `None` says there was nothing to do, which is every launch
/// but one.
pub fn prepare_shelf(
    shelf: &std::path::Path,
    old_bank: &std::path::Path,
) -> std::io::Result<Option<std::path::PathBuf>> {
    std::fs::create_dir_all(shelf)?;
    let Some(name) = old_bank.file_name() else {
        return Ok(None);
    };
    let dst = shelf.join(name);
    if !old_bank.is_file() || dst.exists() {
        return Ok(None);
    }
    std::fs::copy(old_bank, &dst)?;
    Ok(Some(dst))
}

/// What became of an edit when the hand moved on — see [`park_edit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parked {
    /// The edit is still open: the bank was full and it had nowhere to go,
    /// so it stays where it was and "Patch..." resumes it.
    pub kept_open: bool,
    /// A slot of the staged bank changed, so the pane and the disk need
    /// telling.
    pub changed: bool,
    /// What to say in the picker, if anything happened worth a line.
    pub note: Option<String>,
}

/// **An unsaved edit is never lost.** What happens to the edit in flight when
/// the hand chooses another sound, opens the editor on another strip, or on
/// another staged slot.
///
/// An edit that was never touched is simply let go: the editor opens on
/// every patch anybody looks at, and a staged bank that grew a copy of each
/// of them would be a bank nobody asked for. A DIRTY edit that came from a
/// staged slot goes back into that slot — its Save would have done exactly
/// that, and nobody is asked. A dirty edit that came from the loaded bank
/// has no slot of its own, so it takes the first empty one and the picker
/// says so; and when there is no empty one it stays open, and the picker
/// says that instead. The one thing this never does is the thing the first
/// cut did: drop it in silence.
pub fn park_edit(
    staging: &mut Staging,
    edit: Option<Voice>,
    dirty: bool,
    from_slot: Option<usize>,
) -> Parked {
    let untouched = Parked {
        kept_open: false,
        changed: false,
        note: None,
    };
    let Some(v) = edit else { return untouched };
    if !dirty {
        return untouched;
    }
    let name = v.display_name();
    if let Some(slot) = from_slot {
        staging.set(slot, v);
        return Parked {
            kept_open: false,
            changed: true,
            note: Some(format!(
                "Your edit of \u{201c}{name}\u{201d} went back into staged slot {}.",
                slot + 1
            )),
        };
    }
    match staging.first_free() {
        Some(slot) => {
            staging.set(slot, v);
            Parked {
                kept_open: false,
                changed: true,
                note: Some(format!(
                    "Your edit of \u{201c}{name}\u{201d} was kept in staged slot {}, so it \
                     is not lost.",
                    slot + 1
                )),
            }
        }
        None => Parked {
            kept_open: true,
            changed: false,
            note: Some(format!(
                "The staged bank is full, so your edit of \u{201c}{name}\u{201d} stays \
                 open: Patch... resumes it."
            )),
        },
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
    /// were put, and the empties are INIT VOICE rather than garbage.
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

    /// **The staged bank survives a restart, empties and INIT VOICEs
    /// apart.** A slot holding INIT VOICE on purpose and an empty slot write
    /// the same 128 bytes; the mask is what brings each back as itself. And
    /// with no files there is no bank, rather than an error.
    ///
    /// Revert-check performed: reading occupancy from the voices alone (the
    /// no-mask fallback, unconditionally) brings slot 6 back empty and the
    /// second assertion reddens.
    #[test]
    fn a_staged_bank_survives_a_restart_with_its_empties_and_its_init_voices_apart() {
        let dir = std::env::temp_dir().join(format!("tangent-staged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a config dir");
        assert!(Staging::restore_from(&dir).is_none(), "a bank came from nowhere");

        let mut s = Staging::new();
        s.set(5, Voice::default());
        s.set(9, named("KEEP"));
        s.save_to(&dir).expect("written");
        let back = Staging::restore_from(&dir).expect("read back");
        assert_eq!(back.get(5), Some(&Voice::default()), "the staged INIT VOICE was read as an empty slot");
        assert!(back.get(6).is_none(), "an empty slot came back as an INIT VOICE");
        assert_eq!(back.get(9).map(Voice::display_name).as_deref(), Some("KEEP"));
        assert_eq!(back, s);

        // Without the mask, the generous reading: everything that is not
        // INIT VOICE is a patch.
        std::fs::remove_file(dir.join(STAGED_SLOTS_FILE)).expect("mask gone");
        let back = Staging::restore_from(&dir).expect("still a bank");
        assert_eq!(back.filled(), vec![9]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The shelf is made, and the old bank is copied onto it exactly
    /// once.** The original stays; a second launch does nothing; and a file
    /// of the same name the user put there themselves is never written over.
    ///
    /// Revert-check performed: copying whenever the old bank exists (the
    /// `dst.exists()` test dropped) reddens the last assertion — the user's
    /// own file is replaced by the old one.
    #[test]
    fn the_shelf_is_made_and_the_old_bank_copied_onto_it_once_and_never_over_a_users_file() {
        let root = std::env::temp_dir().join(format!("tangent-shelf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let shelf = root.join("SYSEX");
        let old = root.join("config").join(OLD_USER_BANK);
        std::fs::create_dir_all(old.parent().unwrap()).expect("a config dir");

        // No old bank: the shelf is still made, and nothing is copied.
        assert_eq!(prepare_shelf(&shelf, &old).expect("prepared"), None);
        assert!(shelf.is_dir(), "the shelf was not made");

        let mut mine = Staging::new();
        mine.set(0, named("OLD ONE"));
        mine.to_cartridge("my-patches").save(&old).expect("the old bank");
        let dst = prepare_shelf(&shelf, &old).expect("prepared").expect("a copy was made");
        assert_eq!(dst, shelf.join(OLD_USER_BANK));
        assert!(old.is_file(), "the original was moved rather than copied");
        assert_eq!(Cartridge::load(&dst).expect("a cartridge").names()[0], "OLD ONE");

        // Again: nothing to do.
        assert_eq!(prepare_shelf(&shelf, &old).expect("prepared"), None);

        // A file of the user's own under that name is theirs.
        let mut theirs = Staging::new();
        theirs.set(0, named("THEIRS"));
        theirs.to_cartridge("my-patches").save(&dst).expect("their file");
        assert_eq!(prepare_shelf(&shelf, &old).expect("prepared"), None);
        assert_eq!(
            Cartridge::load(&dst).expect("a cartridge").names()[0],
            "THEIRS",
            "the user's own file on the shelf was written over"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `.syx` goes onto a bare name and onto nothing else.
    #[test]
    fn a_bare_bank_name_gets_the_extension_and_a_named_one_keeps_its_own() {
        use std::path::PathBuf;
        assert_eq!(bank_file(PathBuf::from("/s/default")), PathBuf::from("/s/default.syx"));
        assert_eq!(bank_file(PathBuf::from("/s/default.SYX")), PathBuf::from("/s/default.SYX"));
        assert_eq!(bank_file(PathBuf::from("/s/rom1a.bin")), PathBuf::from("/s/rom1a.bin"));
    }

    /// **An untouched edit is let go and a dirty one is never lost**: back
    /// into its own slot, into the first empty slot with a note, or kept
    /// open with a note when there is no slot to take it.
    ///
    /// Revert-check performed: dropping the `dirty` gate stages the
    /// untouched edit and the first assertion reddens; staging a slot-born
    /// edit into a fresh slot instead of its own reddens the second.
    #[test]
    fn an_untouched_edit_is_let_go_and_a_dirty_one_is_never_lost() {
        let mut s = Staging::new();
        let p = park_edit(&mut s, Some(named("LOOKED AT")), false, None);
        assert_eq!(p, Parked { kept_open: false, changed: false, note: None });
        assert_eq!(s.count(), 0, "an edit nobody touched was staged");
        assert_eq!(park_edit(&mut s, None, true, None).changed, false);

        s.set(3, named("WAS THREE"));
        let p = park_edit(&mut s, Some(named("NOW THREE")), true, Some(3));
        assert!(p.changed && !p.kept_open);
        assert_eq!(s.get(3).map(Voice::display_name).as_deref(), Some("NOW THREE"));
        assert_eq!(s.count(), 1, "a slot-born edit took a second slot");
        assert!(p.note.as_deref().is_some_and(|n| n.contains("slot 4")), "{:?}", p.note);

        let p = park_edit(&mut s, Some(named("HOMELESS")), true, None);
        assert!(p.changed && !p.kept_open);
        assert_eq!(s.get(0).map(Voice::display_name).as_deref(), Some("HOMELESS"));
        assert!(p.note.as_deref().is_some_and(|n| n.contains("slot 1")), "{:?}", p.note);

        for i in 0..SLOTS {
            s.set(i, named("FULL"));
        }
        let p = park_edit(&mut s, Some(named("NOWHERE")), true, None);
        assert!(p.kept_open && !p.changed, "a full bank lost the edit: {p:?}");
        assert!(p.note.as_deref().is_some_and(|n| n.contains("full")), "{:?}", p.note);
        assert!(s.filled().iter().all(|&i| s.get(i).map(Voice::display_name).as_deref() == Some("FULL")));
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
