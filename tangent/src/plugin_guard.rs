//! **A plugin that kills the app gets exactly one chance.**
//!
//! Loading a plugin runs somebody else's code inside this process, and some of
//! it cannot be survived. Rhodes Anthology assembles a Mach-O image in memory
//! and runs its initializers off pages no signature covers; under the hardened
//! runtime the kernel answers that with **SIGKILL**, delivered to the host,
//! `CODESIGNING / Invalid Page`. There is no unwind, no `catch_unwind`, no
//! `Result`, no last log line. The app is simply gone between one frame and
//! the next.
//!
//! That alone is survivable — it is one crash, and the owner can choose
//! something else. What made it unrecoverable is that the choice is
//! PERSISTED: the next launch read the same bay back, loaded the same plugin,
//! and died in the same instruction. The app could not be reopened at all, and
//! the only way back in was to hand-edit `settings.json`. The owner: "loaded
//! vst rhodes anthology -- instacrash tangent and won't reopen."
//!
//! **A file is the only instrument that works here.** Nothing in the process
//! outlives SIGKILL, so the record of "I am about to load this" has to be on
//! disk before the load begins and removed after it returns. What survives a
//! kill is therefore exactly the plugin that was being loaded when it
//! happened.
//!
//! The breadcrumb is written on EVERY load, not only at startup: a plugin
//! chosen from the picker kills the app just as thoroughly as one restored
//! from a bay, and the next launch would restore it and die.
//!
//! This is a guard, not a sandbox. It does not stop the first crash — nothing
//! in-process can. It stops the second one, which is the one that costs the
//! project.

use std::path::{Path, PathBuf};

/// Where the breadcrumb sits: beside the settings it protects.
///
/// **Never the live one under test.** The guard's own test arms, disarms
/// and consumes it, and in the real file that ate a real crash's blame, or
/// left a false one for the next launch when the test process died
/// mid-test. A file per test THREAD, so a test that loads a plugin cannot
/// arm the crumb another test is reading either.
fn crumb() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir().join(format!(
            "tangent-loading-plugin-{}-{:?}.txt",
            std::process::id(),
            std::thread::current().id()
        ))
    }
    #[cfg(not(test))]
    {
        tangent_core::config::file("loading-plugin.txt")
    }
}

/// **About to hand control to `bundle`.** Write it down first.
///
/// Failure is ignored on purpose. A machine that cannot write here still has
/// to be able to load plugins; the guard is worth having and is not worth
/// refusing to work over.
pub fn arm(bundle: &Path) {
    let path = crumb();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, bundle.to_string_lossy().as_bytes());
}

/// It came back. Whatever it did, it did not kill us.
pub fn disarm() {
    let _ = std::fs::remove_file(crumb());
}

/// **The breadcrumb as a scope guard**, which is the only shape that is
/// correct.
///
/// A load can leave early in half a dozen places — the bundle has no audio
/// module, the factory refuses the class, `create` fails, the warm-up times
/// out — and every one of those is a plugin that did NOT kill us. Disarming
/// by hand at the end of the happy path leaves the crumb behind for all of
/// them, and the next launch would refuse a plugin whose only crime was an
/// error message.
///
/// `Drop` runs on the early return and on a panic, and does not run on
/// SIGKILL. That is exactly the line this guard needs to draw, and it is
/// drawn by the language rather than by remembering.
pub struct Armed;

impl Armed {
    pub fn new(bundle: &Path) -> Self {
        arm(bundle);
        Self
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        disarm();
    }
}

/// **What killed the last launch**, taken once and cleared.
///
/// Cleared as it is read, so a second consecutive failure is attributed to
/// whatever was loading THEN rather than to this one forever.
pub fn take_blame() -> Option<PathBuf> {
    let path = crumb();
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The guard's own test never touches the live breadcrumb.** `crumb()`
    /// had no test redirect, so the test below disarmed, armed and consumed
    /// the running app's `~/.config/tangent/loading-plugin.txt`: it could eat
    /// a real crash's blame, or leave "Rhodes Anthology" blamed for the next
    /// launch if the test process died mid-test. A source scan, because the
    /// hazard is the very file a behavioural test would have to touch.
    #[test]
    fn the_guards_own_test_never_touches_the_live_breadcrumb() {
        let src = include_str!("plugin_guard.rs");
        let at = src.find("fn crumb() -> PathBuf {").expect("crumb");
        let body = &src[at..at + src[at..].find("\n}\n").expect("crumb's end")];
        assert!(
            body.contains("#[cfg(test)]") && body.contains("std::env::temp_dir()"),
            "crumb() has no test redirect: the guard's own test arms, disarms and \
             consumes the live app's ~/.config/tangent/loading-plugin.txt"
        );
        assert!(
            crumb().starts_with(std::env::temp_dir()),
            "under test the breadcrumb is {}",
            crumb().display()
        );
    }

    /// The three states, in the order a launch meets them.
    ///
    /// Serialised through one test because they share one file — two tests
    /// racing on it would pass or fail by timing, which is worse than no test.
    #[test]
    fn a_load_that_returns_leaves_nothing_and_one_that_does_not_names_itself() {
        // Nothing armed: nobody is blamed. This is the ordinary launch.
        disarm();
        assert_eq!(take_blame(), None);

        // Armed and disarmed — a plugin that loaded. Still nobody.
        arm(Path::new("/Library/Audio/Plug-Ins/VST3/Fine.vst3"));
        disarm();
        assert_eq!(take_blame(), None, "a plugin that came back was blamed");

        // Armed and NOT disarmed — the process died in there. Named, once.
        arm(Path::new("/Library/Audio/Plug-Ins/VST3/Rhodes Anthology.vst3"));
        assert_eq!(
            take_blame().as_deref(),
            Some(Path::new("/Library/Audio/Plug-Ins/VST3/Rhodes Anthology.vst3")),
            "the killer was not named"
        );
        // And only once: the blame is cleared as it is read, so the next
        // launch starts clean rather than refusing the same plugin forever.
        assert_eq!(take_blame(), None, "the blame outlived its launch");

        // A path with spaces survives the round trip — the one that actually
        // did this has two.
        arm(Path::new("/A path/with spaces.vst3"));
        assert_eq!(take_blame().as_deref(), Some(Path::new("/A path/with spaces.vst3")));

        // **A load that fails is not a load that killed us.** The guard
        // disarms on the way out of the scope however it leaves, so the
        // half-dozen early returns in `load_insert` cannot frame a plugin
        // whose only crime was an error message.
        fn refuses(p: &Path) -> Result<(), String> {
            let _crumb = Armed::new(p);
            Err("no audio module in the bundle".to_owned())
        }
        assert!(refuses(Path::new("/Library/Audio/Plug-Ins/VST3/Broken.vst3")).is_err());
        assert_eq!(take_blame(), None, "a plugin that merely failed was blamed");

        // And a panic is not a kill either.
        let p = Path::new("/Library/Audio/Plug-Ins/VST3/Panics.vst3");
        let _ = std::panic::catch_unwind(|| {
            let _crumb = Armed::new(p);
            panic!("inside the load");
        });
        assert_eq!(take_blame(), None, "a plugin that panicked was blamed");
    }
}
