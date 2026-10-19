//! Where Tangent keeps what it remembers about you.
//!
//! `~/.config/tangent/` on every platform — literally, including Windows and
//! macOS, which each have a blessed location of their own that this
//! deliberately ignores. That is parity with the Python original and it is
//! why a settings file copied between machines works at all.
//!
//! **`TANGENT_CONFIG_DIR` points a build anywhere**, a developer's switch:
//! a build under test, or one pointed at a copy of somebody's files, reads
//! and writes there and leaves `~/.config/tangent` alone. The
//! unlimited-tracks branch kept its internal builds in
//! `~/.config/tangent-staging` this way round, automatically, until its last
//! wave; the first build after it is the one that opens the real directory
//! and migrates it (settings.json is copied to `settings.pre-v23.json` before
//! the first format-2 write, and no legacy plugin-state file is renamed or
//! deleted).
//!
//! **A test build never sees a real home.** [`home`] is the one place any
//! crate here asks the platform where a person's home is: the config
//! directory below, `Settings::media_root`'s `Documents/Tangent Media`, the
//! old `Movies/Tangent` take folder, and the folder the explorer and the file
//! panels open on are all built from it ([`documents`] and [`videos`] beside
//! it), and `tangent-ui`'s `ban_scan` refuses a second road. A test build's
//! home is a scratch folder of its own (see [`home`]), so a test that saves
//! settings, presses New or relocks the Konami marker does it to a folder
//! nobody owns, and a test that reads the taught chords reads none. It is
//! decided here, in the build, because `#[cfg(test)]` is narrower than "a
//! test build": it is true only while this library itself compiles as its
//! own unit tests, and false in the ordinary copy of it that everything
//! else links: `tangent-ui`'s tests, the binary's, and this crate's own
//! integration tests under `tests/`. That is how the first two came to
//! resolve the owner's real `~/.config/tangent`, `Documents/Tangent Media`
//! and `Movies/Tangent` once the internal builds' staging folder was gone,
//! and to read his taught chords into every app they built; the third
//! resolved the real config whenever this crate was tested alone. All three
//! turn on the `test-home` feature from their `[dev-dependencies]` (this
//! crate by naming itself), which resolver 2 gives to a test build alone;
//! `scripts/check-home.sh features` proves that every one of those test
//! builds carries it and no build that ships does, and
//! `scripts/check-home.sh run` runs the suite in a home it watches.

use std::ffi::OsString;
use std::path::PathBuf;

/// The directory name in use since 5.0.
const NAME: &str = "tangent";

/// The override, honoured by every build. An empty value is not a path:
/// it would put the config in the working directory.
const OVERRIDE: &str = "TANGENT_CONFIG_DIR";

/// **Whether this build keeps every home path in a scratch folder**: this
/// crate's unit tests, and any build with the `test-home` feature, which only
/// a `[dev-dependencies]` line turns on (the binary's, `tangent-ui`'s, and
/// this crate's own for its integration tests), so only a test build. Public
/// so a test in another crate can say which kind of build it is in, rather
/// than infer it from where a path happens to land.
pub const TEST_HOME: bool = cfg!(any(test, feature = "test-home"));

/// What a test build's scratch home is called in the temp folder, before
/// the process id.
#[cfg(any(test, feature = "test-home"))]
const SCRATCH_PREFIX: &str = "tangent-test-home-";

/// **Where a person's home is**: the root every path to their files is built
/// under, and the only road any code here takes to it.
///
/// The platform's own answer in every build that ships (`dirs`, which reads
/// `HOME` on Unix and the profile folder on Windows), with `HOME` itself
/// behind it for a platform that has no answer: the fallback the media
/// explorer carried on its own before this was the one road (`dirs` first,
/// because `HOME` alone had left the Windows tester's explorer at the drive
/// root).
///
/// **A test build's is a scratch folder** ([`TEST_HOME`]):
/// `<temp>/tangent-test-home-<pid>`, made empty when the process first asks.
/// Per process and not per thread (as `tangent-ui`'s own per-thread settings
/// file is), because a home is resolved on whatever thread asks, an export's
/// worker or the shelf's writer as much as the test's own, and one test's
/// threads must agree on it. Emptied on first use, because a process id is
/// reused and a run must not meet the files a dead process left behind.
/// Nothing deletes it afterwards: it holds what the tests wrote, which is
/// what somebody debugging one of them wants to see, and the temp folder is
/// the operating system's to sweep.
pub fn home() -> Option<PathBuf> {
    #[cfg(any(test, feature = "test-home"))]
    {
        Some(scratch_home().to_path_buf())
    }
    #[cfg(not(any(test, feature = "test-home")))]
    {
        dirs::home_dir().or_else(|| std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from))
    }
}

/// **The platform's documents folder**, under [`home`]'s rule:
/// `FOLDERID_Documents` on Windows and `$XDG_DOCUMENTS_DIR` on Linux rather
/// than an English name, which is the bug in every hardcoded `~/Documents`;
/// a test build's is `Documents` in its scratch home.
pub fn documents() -> Option<PathBuf> {
    #[cfg(any(test, feature = "test-home"))]
    {
        home().map(|h| h.join("Documents"))
    }
    #[cfg(not(any(test, feature = "test-home")))]
    {
        dirs::document_dir()
    }
}

/// **The platform's video folder** (`Movies` on a Mac), where takes went
/// before `Documents/Tangent Media`, under [`home`]'s rule: a test build's is
/// `Movies` in its scratch home.
pub fn videos() -> Option<PathBuf> {
    #[cfg(any(test, feature = "test-home"))]
    {
        home().map(|h| h.join("Movies"))
    }
    #[cfg(not(any(test, feature = "test-home")))]
    {
        dirs::video_dir()
    }
}

/// This process's scratch home, made once. See [`home`].
#[cfg(any(test, feature = "test-home"))]
fn scratch_home() -> &'static std::path::Path {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        fresh_folder(std::env::temp_dir().join(format!("{SCRATCH_PREFIX}{}", std::process::id())))
    })
}

/// `at`, emptied and made: whatever a dead process with the same id left
/// there is gone.
#[cfg(any(test, feature = "test-home"))]
fn fresh_folder(at: PathBuf) -> PathBuf {
    let _ = std::fs::remove_dir_all(&at);
    let _ = std::fs::create_dir_all(&at);
    at
}

fn config_root() -> PathBuf {
    home().unwrap_or_else(|| PathBuf::from(".")).join(".config")
}

/// `~/.config/tangent`, or `TANGENT_CONFIG_DIR` when it is set.
pub fn dir() -> PathBuf {
    dir_from(std::env::var_os(OVERRIDE), config_root())
}

/// [`dir`] with its two inputs named, so the choice can be tested without
/// touching the process environment or the real home.
fn dir_from(over: Option<OsString>, root: PathBuf) -> PathBuf {
    if let Some(p) = over.filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    root.join(NAME)
}

/// `dir()` with a file name on the end.
pub fn file(name: &str) -> PathBuf {
    dir().join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **One name, and the override wins.** It was one name until the
    /// unlimited-tracks branch gave its internal builds a twin
    /// (`tangent-staging`), and it is one name again since that branch's
    /// last wave; pinned through the pure chooser rather than by reading the
    /// process environment, so the override's answer is pinned too.
    #[test]
    fn the_config_directory_is_tangent_and_the_override_wins() {
        let root = PathBuf::from("/home/someone/.config");
        assert_eq!(dir_from(None, root.clone()), root.join("tangent"));
        // The override wins; an empty one does not.
        let over = Some(OsString::from("/elsewhere/cfg"));
        assert_eq!(dir_from(over, root.clone()), PathBuf::from("/elsewhere/cfg"));
        assert_eq!(dir_from(Some(OsString::new()), root.clone()), root.join("tangent"));

        // And the live answer is the chooser's.
        let d = dir();
        assert_eq!(d, dir_from(std::env::var_os(OVERRIDE), config_root()));
        if std::env::var_os(OVERRIDE).is_none() {
            assert!(d.ends_with(NAME), "the config directory is {}", d.display());
        }
        assert_eq!(
            file("license.key"),
            d.join("license.key"),
            "file() does not agree with dir()"
        );
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

    /// **A test build's home is a scratch folder, and every path built on it
    /// is in there**: the config directory and its files, the documents and
    /// the video folders. This crate's unit tests are a test build by
    /// `cfg(test)`; its integration tests, the binary's and `tangent-ui`'s
    /// are by the `test-home` feature their `[dev-dependencies]` turn on,
    /// and each pins the same fact from its own side (`tests/test_home.rs`
    /// here, `home_tests` in the binary, `settings` in `tangent-ui`).
    /// Revert check: `home()` answering `dirs::home_dir()`
    /// in every build fails the scratch-folder assertion on any machine
    /// whose home is not in the temp folder.
    #[test]
    fn a_test_build_keeps_every_home_path_in_its_own_scratch_folder() {
        assert!(TEST_HOME, "this crate's own tests are a test build");
        let home = home().expect("a test build always has a home");
        assert!(
            home.starts_with(std::env::temp_dir()),
            "a test build's home is {}, not a scratch folder",
            home.display()
        );
        assert_eq!(
            home.file_name().and_then(|n| n.to_str()),
            Some(format!("{SCRATCH_PREFIX}{}", std::process::id()).as_str()),
            "the scratch home is not this process's own"
        );
        assert!(home.is_dir(), "the scratch home was named and never made");
        assert_ne!(Some(&home), dirs::home_dir().as_ref(), "the scratch home is somebody's real one");
        assert_eq!(config_root(), home.join(".config"));
        if std::env::var_os(OVERRIDE).is_none() {
            assert_eq!(dir(), home.join(".config").join(NAME));
            assert!(file("cheat.unlock").starts_with(&home));
        }
        assert_eq!(documents(), Some(home.join("Documents")));
        assert_eq!(videos(), Some(home.join("Movies")));
        // One home per process: every thread asks and gets the same answer.
        let other = std::thread::spawn(super::home).join().expect("a second thread");
        assert_eq!(other, Some(home));
    }

    /// **A scratch home starts empty**, whatever a dead process with the same
    /// id left in the folder of that name.
    #[test]
    fn a_scratch_home_starts_empty_whatever_was_left_in_it() {
        let at = scratch("fresh").join("tangent-test-home-0");
        std::fs::create_dir_all(at.join(".config/tangent")).expect("a dead process's home");
        std::fs::write(at.join(".config/tangent/settings.json"), b"{}").expect("its settings");
        let made = fresh_folder(at.clone());
        assert_eq!(made, at);
        assert!(made.is_dir(), "the folder was not made");
        assert_eq!(std::fs::read_dir(&made).expect("the folder").count(), 0, "what was left survived");
        let _ = std::fs::remove_dir_all(at.parent().expect("the scratch root"));
    }
}
