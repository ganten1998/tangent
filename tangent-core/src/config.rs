//! Where Tangent keeps what it remembers about you.
//!
//! `~/.config/tangent/` on every platform — literally, including Windows and
//! macOS, which each have a blessed location of their own that this
//! deliberately ignores. That is parity with the Python original and it is
//! why a settings file copied between machines works at all.
//!
//! **It used to be `~/.config/ivory/`, and that directory is DELETED.** Not
//! migrated: 5.0 is a clean break, the tester group is ten people who are
//! known personally, and new supporter keys go out by email. Carrying a
//! second read path forward for a decade to spare ten people one email is the
//! wrong trade, and a migration that runs once and is then dead code forever
//! is worse than no migration at all.
//!
//! What the old directory held, so that nobody is surprised by what goes:
//! `license.key`, `settings.json`, `overrides.json`, `my-patches.syx`.
//!
//! The deletion is [`purge_pre_5_0`], and only the HOST calls it — a VST3
//! scanned in the background by somebody else's DAW must not be deleting
//! directories in the user's home.

use std::path::PathBuf;

/// The directory name in use since 5.0.
const NAME: &str = "tangent";

/// What it was called before 5.0. Only ever used to find it and remove it.
const PRE_5_0: &str = "ivory";

fn config_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
}

/// `~/.config/tangent`. The only answer; there is no fallback.
pub fn dir() -> PathBuf {
    config_root().join(NAME)
}

/// `dir()` with a file name on the end.
pub fn file(name: &str) -> PathBuf {
    dir().join(name)
}

/// **Delete `~/.config/ivory` if it is there.** Once, at launch, by the host.
///
/// A hard pivot rather than a migration — see the module docs. Nothing is
/// read out of it first and nothing is kept.
///
/// Deliberately narrow: it builds the path itself from `$HOME` and a
/// hardcoded name, so there is no argument a caller could pass that would
/// make this remove something else. `remove_dir_all` on a path that is not
/// there is an error, not a panic, and the error is dropped — a machine that
/// never ran a pre-5.0 build is the common case, not a fault.
///
/// Answers whether it removed anything, for the log.
pub fn purge_pre_5_0() -> bool {
    let old = config_root().join(PRE_5_0);
    // `is_dir` follows symlinks, and a symlink here would make the recursive
    // delete walk into wherever it points. Refuse anything that is not a real
    // directory rather than being clever about it.
    let Ok(meta) = std::fs::symlink_metadata(&old) else {
        return false;
    };
    if !meta.is_dir() {
        return false;
    }
    std::fs::remove_dir_all(&old).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path has no fallback and no conditional: one name, always.
    #[test]
    fn the_config_directory_is_tangent_and_nothing_else() {
        let d = dir();
        assert!(d.ends_with(NAME), "the config directory is {}", d.display());
        assert_eq!(
            file("license.key"),
            d.join("license.key"),
            "file() does not agree with dir()"
        );
        assert!(
            !d.to_string_lossy().contains(PRE_5_0),
            "the pre-5.0 name is still reachable as a read path"
        );
    }

    /// **The purge takes a real directory and refuses a symlink.**
    ///
    /// Mirrored against a scratch root rather than the real `$HOME`, because a
    /// test that touches the actual config directory would delete the licence
    /// of whoever ran it — and this crate has already been bitten once by a
    /// test reading the real `~/.config/ivory/license.key` and concluding the
    /// build was a supporter's.
    fn purge_at(root: &std::path::Path) -> bool {
        let old = root.join(PRE_5_0);
        let Ok(meta) = std::fs::symlink_metadata(&old) else {
            return false;
        };
        if !meta.is_dir() {
            return false;
        }
        std::fs::remove_dir_all(&old).is_ok()
    }

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "tangent-config-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("scratch");
        p
    }

    #[test]
    fn an_old_config_directory_and_everything_in_it_goes() {
        let root = scratch("purge");
        let old = root.join(PRE_5_0);
        std::fs::create_dir_all(old.join("nested")).expect("old");
        std::fs::write(old.join("license.key"), b"stale").expect("key");
        std::fs::write(old.join("nested/settings.json"), b"{}").expect("settings");

        assert!(purge_at(&root), "the purge reported doing nothing");
        assert!(!old.exists(), "the pre-5.0 directory survived");
        // A second launch is a quiet no-op, not an error.
        assert!(!purge_at(&root), "the purge claimed to remove it twice");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A symlink where the old directory was must not be followed — the
    /// recursive delete would walk out of `~/.config` entirely.
    #[test]
    fn a_symlink_in_place_of_the_old_directory_is_refused() {
        let root = scratch("symlink");
        let elsewhere = root.join("precious");
        std::fs::create_dir_all(&elsewhere).expect("elsewhere");
        std::fs::write(elsewhere.join("keep.txt"), b"do not delete").expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&elsewhere, root.join(PRE_5_0)).expect("symlink");
        #[cfg(not(unix))]
        return;

        assert!(!purge_at(&root), "a symlink was treated as the old directory");
        assert!(
            elsewhere.join("keep.txt").is_file(),
            "the purge followed a symlink and deleted what it pointed at"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
