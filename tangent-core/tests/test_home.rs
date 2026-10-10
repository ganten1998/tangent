//! **This crate's integration tests are a test build too** (design
//! W8-safe-tests): a test here that loads the licence, relocks the Konami
//! marker or runs the pre-5.0 purge does it in a scratch home, not in the
//! home of whoever ran `cargo test -p tangent-core`.
//!
//! A file under `tests/` is a crate of its own, and it links the ORDINARY
//! library: `#[cfg(test)]` is true only while the library's own unit tests
//! compile, so `config::home`'s redirect reached those and never these.
//! The library turns `test-home` on for them from its own
//! `[dev-dependencies]` (the crate itself, by path), which resolver 2 gives
//! to a test build alone. A workspace run unifies the binary's line in
//! anyway, which is how the gap stayed hidden; check.sh's two `cargo test -p
//! tangent-core` legs build this crate alone, and there this is what fails.
//! Revert check: that dev-dependency line removed fails the first assertion
//! under `cargo test -p tangent-core --test test_home`.

use tangent_core::config;
use tangent_core::license::LicenseStore;
use tangent_core::OverrideStore;

/// **No home path these tests can reach is a real one.** Every path is only
/// named, nothing is created or opened; a developer's switch that points the
/// config elsewhere on purpose wins, so it is left out when set.
#[test]
fn this_test_build_keeps_every_home_path_in_a_scratch_folder() {
    assert!(
        config::TEST_HOME,
        "tangent-core's integration tests link a tangent-core without test-home: they resolve the real home"
    );
    let home = config::home().expect("a test build always has a home");
    assert!(
        home.starts_with(std::env::temp_dir()),
        "tangent-core's integration tests resolve {} as home",
        home.display()
    );
    assert_eq!(
        home.file_name().and_then(|n| n.to_str()),
        Some(format!("tangent-test-home-{}", std::process::id()).as_str()),
        "the scratch home is not this process's own"
    );
    let mut named = vec![
        ("the documents folder", config::documents().expect("documents")),
        ("the video folder", config::videos().expect("videos")),
    ];
    if std::env::var_os("TANGENT_CONFIG_DIR").is_none() {
        named.push(("the config directory", config::dir()));
        named.push(("the Konami marker", config::file("cheat.unlock")));
        named.push(("the licence", LicenseStore::default_path()));
        named.push(("the taught chords", OverrideStore::default_path()));
    }
    let real: Vec<String> = named
        .iter()
        .filter(|(_, p)| !p.starts_with(&home))
        .map(|(what, p)| format!("{what}: {}", p.display()))
        .collect();
    assert!(real.is_empty(), "outside the scratch home {}:\n  {}", home.display(), real.join("\n  "));
}
