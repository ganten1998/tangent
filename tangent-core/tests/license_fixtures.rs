//! Frozen license fixtures.
//!
//! This key was minted by `tangent-keygen` with the real k1 signing key. It is a
//! REAL, valid supporter key — deliberately committed, because its only power
//! is unlocking cosmetics in a free app, and what it buys us is worth far more:
//! it pins the entire chain (payload layout, base32 alphabet, CRC, signature,
//! public keyring) so that no future refactor can quietly stop honoring keys
//! that are already in customers' hands. If this test ever fails, STOP — you
//! are about to ship a build that locks out everyone who paid.
//!
//! Requires the `license` feature (the GUI always enables it).
#![cfg(feature = "license")]

use tangent_core::license::{verify_key, License, LicenseError, LicenseStore, TIER_SUPPORTER};

/// Minted 2026-08-11, k1, name "Ganten".
const FIXTURE_KEY: &str = "\
X92VZ-CG104-4PW03-01DAN-6J068-XGPWX\n\
35DST-BBRPW-74RT6-0NEES-MW1JC-MC7J9\n\
3G47V-DNHKH-RKJKC-KM6FE-G0VK3-WDP8W\n\
7FDPP-G210V-SD8F6-23VEN-VR121-7KQ55\n\
WMVM5-0FMHM-9K6E0-2";

#[test]
fn a_real_issued_key_still_verifies() {
    let l = verify_key(FIXTURE_KEY).expect("issued keys must never stop verifying");
    assert_eq!(l.tier, TIER_SUPPORTER);
    assert_eq!(l.name.as_deref(), Some("Ganten"));
    assert_eq!(l.max_major, 0, "perpetual");
    assert_eq!(l.order_id, [0x60, 0x0b, 0x55, 0x53, 0x48]);
}

#[test]
fn the_same_key_survives_being_retyped() {
    // Lowercased, O/0 and I/1 swapped, newlines collapsed, dashes dropped —
    // the shapes a human actually produces when copying a key by hand.
    let mangled = FIXTURE_KEY
        .to_lowercase()
        .replace('0', "O")
        .replace('1', "l")
        .replace('\n', " ")
        .replace('-', " ");
    assert_eq!(verify_key(&mangled).unwrap().name.as_deref(), Some("Ganten"));
}

#[test]
fn one_altered_character_is_rejected() {
    let mut s: Vec<char> = FIXTURE_KEY.chars().collect();
    let pos = s.iter().rposition(|c| c.is_ascii_digit()).expect("has a digit");
    s[pos] = if s[pos] == '7' { '8' } else { '7' };
    let tampered: String = s.into_iter().collect();
    assert!(
        matches!(verify_key(&tampered), Err(LicenseError::Checksum | LicenseError::BadSignature)),
        "a mutated key must never verify"
    );
}

#[test]
fn an_installed_key_survives_a_reload() {
    let dir = std::env::temp_dir().join(format!("ivory-lic-install-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("license.key");
    let mut s = LicenseStore::load_from(&p);
    let l = s.install(FIXTURE_KEY).expect("a genuine key on a writable path installs");
    assert_eq!(l.name.as_deref(), Some("Ganten"));
    assert!(p.exists(), "Ok from install means the key reached disk");
    assert!(!p.with_extension("key.tmp").exists(), "no temp debris");
    assert!(LicenseStore::load_from(&p).is_supporter());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_genuine_key_that_cannot_be_saved_is_honored_this_run_and_says_so() {
    // The store's parent "directory" is a regular FILE, so the temp-file write
    // fails deterministically on every platform — no permission games.
    let blocker = std::env::temp_dir().join(format!("ivory-lic-blocker-{}", std::process::id()));
    std::fs::write(&blocker, "not a directory").unwrap();
    let mut s = LicenseStore::load_from(blocker.join("license.key"));
    assert_eq!(s.install(FIXTURE_KEY).unwrap_err(), LicenseError::Unsaved);
    // The session honors the key — a disk problem must never turn a supporter
    // away…
    assert!(s.is_supporter());
    assert_eq!(s.display_name(), Some("Ganten"));
    // …but nothing reached disk, so the next "launch" comes up free. `install`
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
