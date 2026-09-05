//! **What Tangent ships onto the user's shelves, and how it gets there.**
//!
//! Two packs, both the owner's own work: three impulse responses captured
//! with the IR unit's wizard from his Alesis Quadraverb on 2026-09-05
//! (`assets/irs/README.md`), and two drum kits built on the pad strip
//! (`assets/kits/README.md`). They exist so that a fresh install is not an
//! empty IR browser and an empty Load Kit panel. The owner: "make sure a user
//! can see them immediately after installing and opening IR (Experimental)"
//! and "make my 2 kits available as well OOB to load from the default loading
//! Tangent media presets drums".
//!
//! **Compiled in with `include_bytes!`**, the way the click and the DX7 bank
//! are, and for the same reason: one road on all three platforms, with
//! nothing for a packaging script to forget — the Linux tarballs have already
//! shipped once without a library nothing declared, and an asset copied at
//! package time is the same shape of fault. Four megabytes on the binary,
//! once.
//!
//! **Written to a shelf once, ever.** [`seed`] puts each file into its folder
//! when it is not there and writes its name to a stamp beside it; a file that
//! is later deleted stays deleted, because a shelf that resurrects what
//! somebody threw away is a shelf nobody trusts. A file the user already has
//! under the same name — the owner's own folders, for one — is left exactly
//! as it is, and stamped as accounted for.

use std::path::Path;

/// A pack: files by the name they ship under.
pub type Pack = &'static [(&'static str, &'static [u8])];

/// The impulse responses, for the `IRs` shelf.
///
/// The names are the owner's own, verbatim — unit and program in the name,
/// the way the wizard's field had them — so a shelf that already holds his
/// captures gains nothing twice.
pub const IRS: Pack = &[
    (
        "HOLMES'PLATE (ALESIS QUADRAVERB).wav",
        include_bytes!("../../assets/irs/HOLMES'PLATE (ALESIS QUADRAVERB).wav"),
    ),
    (
        "KINGS CHAMBER (ALESIS QUADRAVERB).wav",
        include_bytes!("../../assets/irs/KINGS CHAMBER (ALESIS QUADRAVERB).wav"),
    ),
    (
        "TAJ MAHAL modw (ALESIS QUADRAVERB).wav",
        include_bytes!("../../assets/irs/TAJ MAHAL modw (ALESIS QUADRAVERB).wav"),
    ),
];

/// The drum kits, for the `Presets/Tangent Drums` drawer — the folder Load
/// Kit opens on (`DRUM_PRESET_FOLDER`). Self-contained: every sample is
/// embedded in the `.kit`.
pub const KITS: Pack = &[
    ("CR78.kit", include_bytes!("../../assets/kits/CR78.kit")),
    ("LinnDrum.kit", include_bytes!("../../assets/kits/LinnDrum.kit")),
];

/// The stamp beside the files: one shipped name per line. Hidden, and
/// neither a WAV nor a `.kit`, so no shelf's scan or panel filter lists it
/// and no browser verb touches it.
pub const STAMP: &str = ".shipped";

/// Put a pack on a shelf, each file once. Answers how many were written.
///
/// A name already stamped is skipped without looking at the disk — that is
/// the "stays deleted" half. A name not yet stamped is written only when
/// nothing stands under it, and stamped either way. The folder is made on
/// the way in: it is the app's own, exactly as `panel_start` treats it, and
/// a fresh install has no other reason to have made it yet.
pub fn seed(root: &Path, pack: Pack) -> usize {
    let stamp = root.join(STAMP);
    let done = std::fs::read_to_string(&stamp).unwrap_or_default();
    let mut written = 0;
    let mut record = String::new();
    for (name, bytes) in pack {
        if done.lines().any(|line| line == *name) {
            continue;
        }
        let target = root.join(name);
        if !target.exists() {
            if std::fs::create_dir_all(root).is_err() || std::fs::write(&target, bytes).is_err() {
                // Not stamped: a shelf that could not be written gets another
                // try next launch rather than a note saying it was done.
                continue;
            }
            written += 1;
        }
        record.push_str(name);
        record.push('\n');
    }
    if !record.is_empty() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&stamp) {
            let _ = f.write_all(record.as_bytes());
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::{seed, IRS, KITS, STAMP};

    fn shelf(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tangent-shipped-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// **A fresh shelf gets the whole IR pack, the scan lists every file
    /// with a real length, and what is deleted stays deleted.**
    ///
    /// Revert-check performed: drop the stamp read from `seed` and the
    /// deleted file comes back on the third call, which the last assert
    /// names.
    #[test]
    fn the_irs_land_once_and_a_deleted_one_stays_deleted() {
        let root = shelf("irs");
        assert_eq!(seed(&root, IRS), IRS.len(), "a fresh shelf did not get the pack");
        for (name, bytes) in IRS {
            let on_disk = std::fs::read(root.join(name)).expect("the file was written");
            assert_eq!(on_disk.len(), bytes.len(), "{name} landed short");
        }
        // The shelf's own scanner sees them, as WAVs with a duration — which
        // is what the browser lists and what the unit loads.
        let rows = crate::desktop::scan_ir_rows(&root);
        assert_eq!(rows.len(), IRS.len(), "the scan does not list the pack");
        for row in &rows {
            assert!(row.seconds > 1.0, "{} scans as {} s", row.name, row.seconds);
            assert!(
                IRS.iter().any(|(n, _)| n.trim_end_matches(".wav") == row.name),
                "the scan lists a name the pack does not ship: {}",
                row.name
            );
        }
        // Nothing twice.
        assert_eq!(seed(&root, IRS), 0, "a second launch wrote the pack again");
        // Thrown away is thrown away.
        let gone = root.join(IRS[0].0);
        std::fs::remove_file(&gone).expect("delete one");
        assert_eq!(seed(&root, IRS), 0);
        assert!(!gone.exists(), "a deleted impulse response came back");
        assert!(root.join(STAMP).is_file(), "no stamp was left");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **The kits land as kits Load Kit can open**: every one reads back
    /// through the same reader the LOAD chip uses, with pads and with the
    /// samples embedded — a kit shipped as paths alone would be a kit that
    /// plays nothing on any machine but the owner's.
    #[test]
    fn the_kits_land_and_read_back_whole() {
        let root = shelf("kits");
        assert_eq!(seed(&root, KITS), KITS.len(), "a fresh drawer did not get the kits");
        for (name, _) in KITS {
            let kit = tangent_ui::drumpad::KitPreset::load(&root.join(name))
                .unwrap_or_else(|e| panic!("{name} does not read back: {e}"));
            assert!(!kit.pads.is_empty(), "{name} has no pads");
            assert!(!kit.media.is_empty(), "{name} carries no samples");
            assert!(
                kit.pads.iter().all(|p| p.blob.is_some()),
                "{name} has a pad that points at a file instead of embedded audio"
            );
        }
        assert_eq!(seed(&root, KITS), 0, "a second launch wrote the kits again");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **A file the user already has under a shipped name is left alone.**
    /// The owner's own shelves hold these files under these names; the seed
    /// must not write over his captures with the copy of them it carries.
    #[test]
    fn a_file_already_on_the_shelf_is_not_written_over() {
        let root = shelf("theirs");
        std::fs::create_dir_all(&root).expect("the shelf");
        let mine = root.join(IRS[1].0);
        std::fs::write(&mine, b"the user's own capture").expect("write");
        assert_eq!(seed(&root, IRS), IRS.len() - 1, "the pack counted the user's file as its own");
        assert_eq!(
            std::fs::read(&mine).expect("read"),
            b"the user's own capture",
            "the user's file was written over"
        );
        assert!(
            std::fs::read_to_string(root.join(STAMP))
                .expect("stamp")
                .lines()
                .any(|l| l == IRS[1].0),
            "the user's file was not stamped as accounted for"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
