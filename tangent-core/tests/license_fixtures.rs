//! Frozen license fixtures.
//!
//! Two keys, and neither unlocks a shipped build:
//!
//! - `PUBLISHED_KEY` was minted by `tangent-keygen` with the real k1 signing
//!   key on 2026-08-11 and committed here as a fixture while a key unlocked
//!   only cosmetics. The repository was briefly public, so it is revoked
//!   (`REVOKED_ORDERS`). It stays as the revoked-key fixture, and it still
//!   pins the production chain: its signature verifies under k1 (the unit
//!   test `the_published_key_is_genuine_under_k1_and_revoked`), and here the
//!   license layer refuses it as revoked rather than as a bad signature. If
//!   it ever stops reporting `Revoked`, STOP: either the chain moved (every
//!   customer's key just died) or the revocation did.
//! - `FIXTURE_KEY` is `tests/fixtures/supporter.key`, signed by
//!   `license::test_keyring`, whose seed is public and whose key only a test
//!   build accepts (the `test-keyring` feature, from this crate's own
//!   dev-dependency line). Everything a working key must survive is proven
//!   on it.
//!
//! Requires the `license` feature (the GUI always enables it).
#![cfg(feature = "license")]

use tangent_core::license::{
    test_keyring, verify_key, License, LicenseError, LicenseStore, REVOKED_ORDERS, TIER_SUPPORTER,
};

/// Minted 2026-08-11, k1, name "Ganten", order 600b555348. Revoked.
const PUBLISHED_KEY: &str = include_str!("../../tests/fixtures/revoked-2026-08-11.key");

/// The test keyring's supporter, name "Tester".
const FIXTURE_KEY: &str = include_str!("../../tests/fixtures/supporter.key");

#[test]
fn the_published_key_is_revoked_not_forged() {
    assert_eq!(verify_key(PUBLISHED_KEY), Err(LicenseError::Revoked));
    assert!(REVOKED_ORDERS.contains(&[0x60, 0x0b, 0x55, 0x53, 0x48]));
    // Retyped by hand it is still the same genuine, revoked key: the
    // alphabet, aliases and CRC still decode it to a signature k1 made.
    let mangled = PUBLISHED_KEY
        .to_lowercase()
        .replace('0', "O")
        .replace('1', "l")
        .replace('\n', " ")
        .replace('-', " ");
    assert_eq!(verify_key(&mangled), Err(LicenseError::Revoked));
}

#[test]
fn a_stored_published_key_unlocks_nothing() {
    let dir = std::env::temp_dir().join(format!("ivory-lic-published-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("license.key");
    std::fs::write(&p, PUBLISHED_KEY).unwrap();
    let s = LicenseStore::load_from(&p);
    assert!(!s.is_supporter(), "a revoked key still unlocks on launch");
    assert!(s.display_name().is_none());
    assert!(s.stored_key_revoked());
    assert!(p.exists(), "the file is never eaten");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_fixture_is_the_test_keyrings() {
    assert_eq!(FIXTURE_KEY.trim_end(), test_keyring::mint(&test_keyring::fixture_license()));
    let l = verify_key(FIXTURE_KEY).expect("a test build accepts the test keyring");
    assert_eq!(l, test_keyring::fixture_license());
    assert_eq!(l.tier, TIER_SUPPORTER);
    assert_eq!(l.name.as_deref(), Some("Tester"));
    assert_eq!(l.max_major, 0, "perpetual");
}

#[test]
fn the_same_key_survives_being_retyped() {
    // Lowercased, O/0 and I/1 swapped, newlines collapsed, dashes dropped:
    // the shapes a human actually produces when copying a key by hand.
    let mangled = FIXTURE_KEY
        .to_lowercase()
        .replace('0', "O")
        .replace('1', "l")
        .replace('\n', " ")
        .replace('-', " ");
    assert_eq!(verify_key(&mangled).unwrap().name.as_deref(), Some("Tester"));
}

#[test]
fn one_altered_character_is_rejected() {
    for key in [FIXTURE_KEY, PUBLISHED_KEY] {
        let mut s: Vec<char> = key.chars().collect();
        let pos = s.iter().rposition(|c| c.is_ascii_digit()).expect("has a digit");
        s[pos] = if s[pos] == '7' { '8' } else { '7' };
        let tampered: String = s.into_iter().collect();
        assert!(
            matches!(verify_key(&tampered), Err(LicenseError::Checksum | LicenseError::BadSignature)),
            "a mutated key must never verify"
        );
    }
}

#[test]
fn an_installed_key_survives_a_reload() {
    let dir = std::env::temp_dir().join(format!("ivory-lic-install-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("license.key");
    let mut s = LicenseStore::load_from(&p);
    let l = s.install(FIXTURE_KEY).expect("a genuine key on a writable path installs");
    assert_eq!(l.name.as_deref(), Some("Tester"));
    assert!(p.exists(), "Ok from install means the key reached disk");
    assert!(!p.with_extension("key.tmp").exists(), "no temp debris");
    assert!(LicenseStore::load_from(&p).is_supporter());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn installing_the_published_key_changes_nothing() {
    let dir = std::env::temp_dir().join(format!("ivory-lic-install-pub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("license.key");
    let mut s = LicenseStore::load_from(&p);
    assert_eq!(s.install(PUBLISHED_KEY).unwrap_err(), LicenseError::Revoked);
    assert!(!p.exists(), "a refused key must not be written");
    assert!(!s.is_supporter());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_genuine_key_that_cannot_be_saved_is_honored_this_run_and_says_so() {
    // The store's parent "directory" is a regular FILE, so the temp-file write
    // fails deterministically on every platform, no permission games.
    let blocker = std::env::temp_dir().join(format!("ivory-lic-blocker-{}", std::process::id()));
    std::fs::write(&blocker, "not a directory").unwrap();
    let mut s = LicenseStore::load_from(blocker.join("license.key"));
    assert_eq!(s.install(FIXTURE_KEY).unwrap_err(), LicenseError::Unsaved);
    // The session honors the key: a disk problem must never turn a supporter
    // away...
    assert!(s.is_supporter());
    assert_eq!(s.display_name(), Some("Tester"));
    // ...but nothing reached disk, so the next "launch" comes up free. `install`
    // used to answer plain Ok here, and this restart was the first anyone
    // heard of it.
    assert!(!LicenseStore::load_from(blocker.join("license.key")).is_supporter());
    std::fs::remove_file(&blocker).ok();
}

#[test]
fn payload_preimage_is_byte_frozen() {
    // Reconstructing the license and re-serializing must reproduce the exact
    // bytes that were signed. This is the check that catches a "harmless"
    // field reorder before it reaches a customer.
    let l = verify_key(FIXTURE_KEY).unwrap();
    let rebuilt = License { ..l.clone() };
    assert_eq!(rebuilt.payload_bytes(), l.payload_bytes());
    assert_eq!(l.payload_bytes()[0], 1, "format version 1");
}
