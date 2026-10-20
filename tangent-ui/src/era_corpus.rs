//! **The era corpus: one file per desk shape this app has written, and what
//! each one opens as, written down as literals.**
//!
//! The fixtures live in `tests/fixtures/eras/` and are SYNTHETIC: built by
//! `tests/fixtures/eras/gen.py` from the frozen era tables (every strip
//! painted, named and sent at a value that says which strip it was, a plugin
//! on every strip and on the pads, masks on the buses), so a strip that lands
//! one seat off shows up as the wrong number in the wrong place. Real files
//! cannot be committed, and a fixture made by today's writer would describe
//! today's shape and no other.
//!
//! **Every expectation is a literal.** An expectation computed from the
//! live constants (`CHANNELS - 16`, `Strip::Drums.index()`) agrees with any
//! lift that reads the same constants, which is the one fault this corpus
//! exists to catch: the day the desk changes shape, both sides move together
//! and the test stays green over every file it no longer opens right. The
//! observation below is a plain-text picture of what a fixture opened as,
//! and each fixture's picture is pinned as written.
//!
//! The DAW blobs are what the retired VST3 saved into a DAW project (its
//! `#[persist = "settings"]` field, written by `Settings::to_json`), kept as
//! fixtures because `Settings::from_json` still reads that text.
//!
//! `TANGENT_ERA_DUMP=/some/dir cargo test -p tangent-ui --lib
//! era_corpus::write_observations -- --ignored` writes each fixture's
//! picture to a file, for reading when a fixture is added.

use crate::arrangement::Arrangement;
use crate::desk_lift::{V32_CHANNELS, V32_INSERT_SPACE, V32_STRIPS};
use crate::settings::Settings;

/// Every fixture, by file name.
macro_rules! fixture {
    ($name:literal) => {
        ($name, include_str!(concat!("../../tests/fixtures/eras/", $name)))
    };
}

/// The settings files and DAW blobs, by file name.
pub(crate) const SETTINGS: [(&str, &str); 11] = [
    fixture!("settings-v12.json"),
    fixture!("settings-v17-pads16.json"),
    fixture!("settings-v18.json"),
    fixture!("settings-v19.json"),
    fixture!("settings-v20-27.json"),
    fixture!("settings-v20.json"),
    fixture!("settings-v21.json"),
    fixture!("settings-v22.json"),
    fixture!("daw-3x.json"),
    fixture!("daw-4x.json"),
    fixture!("daw-5x.json"),
];

/// The documents, by file name.
pub(crate) const DOCUMENTS: [(&str, &str); 6] = [
    fixture!("doc-v4-11.json"),
    fixture!("doc-v5-18.json"),
    fixture!("doc-v5-19.json"),
    fixture!("doc-v6-27.json"),
    fixture!("doc-v6-35.json"),
    fixture!("doc-v7.json"),
];

fn json<T: serde::Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).expect("serialisable")
}

/// The occupied bays of a dense desk, `bay=path`, ascending.
fn occupied<'a>(bays: impl IntoIterator<Item = (usize, &'a str)>) -> String {
    bays.into_iter()
        .filter(|(_, p)| !p.is_empty())
        .map(|(b, p)| format!("{b}={p}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// **What a settings file opened as**, one fact per line: the dense strip
/// rows, every occupied bay, every strip mask, the channel arrays the
/// migrations write, and the one-launch shelf flags. Read over the V32
/// shape the lifts land on (its strips, its channels, its seat map) and not
/// over the live arrays, so the picture is the same whatever width the live
/// desk is compiled at.
pub(crate) fn observe_settings(text: &str) -> String {
    let s = Settings::from_json(text);
    let flags = s.shelf_shuffles().names();
    // The strip rows in the legacy STRIP SPACE (`Settings::strip_space`):
    // the live desk keeps the rows and the specials apart (W5), and the
    // picture joins them back at their frozen strips, so it says exactly
    // what it said while they shared one array.
    let strips = s.strip_space();
    [
        format!("colors {}", json(&strips.colors[..=V32_STRIPS])),
        format!("sends {}", json(&strips.sends[..V32_STRIPS])),
        format!("names {}", json(&strips.names[..strips.names.len().min(V32_STRIPS + 1)])),
        format!(
            "inserts {}",
            occupied(
                s.legacy_inserts()
                    .iter()
                    .take(V32_INSERT_SPACE)
                    .enumerate()
                    .map(|(b, p)| (b, p.as_str()))
            )
        ),
        format!("muted {:#x} soloed {:#x} phase {:#x}", strips.muted, strips.soloed, strips.phase),
        format!("kinds {}", json(&s.rows[..V32_CHANNELS].iter().map(|r| r.kind_byte()).collect::<Vec<_>>())),
        // Every row's input: a row always has one, empty for none, where the
        // legacy list was empty for a file that said nothing.
        format!("inputs {}", json(&s.rows[..V32_CHANNELS].iter().map(|r| r.input.clone()).collect::<Vec<_>>())),
        format!("shuffles {}", flags.join(" ")),
    ]
    .join("\n")
}

/// **What a document opened as**: its desk's strip rows, occupied seats and
/// masks, the rows it says exist and are armed, and every curve's lane.
pub(crate) fn observe_document(text: &str) -> String {
    let a = Arrangement::from_json(text);
    let d = a.desk.as_ref().expect("every era fixture has a desk");
    let curves: Vec<String> = a
        .automation
        .curves()
        .iter()
        // The master's by the lane number its era filed it under.
        .map(|c| format!("{}:{}", c.lane.index_or(crate::desk_lift::V32_MASTER_LANE), c.param.key()))
        .collect();
    // Joined back into strip space, for `observe_settings`' reason.
    let strips = d.strip_space();
    [
        format!("colors {}", json(&strips.colors)),
        format!("sends {}", json(&strips.sends)),
        format!("names {}", json(&strips.names)),
        format!("inserts {}", occupied(d.legacy_bays().iter().map(|(b, p)| (*b, p.as_str())))),
        format!(
            "muted {:#x} soloed {:#x} phase {:?}",
            strips.muted,
            strips.soloed,
            d.says().contains(crate::arrangement::Says::PHASE).then_some(strips.phase)
        ),
        format!("tracks_added {:#x} armed {:#x}", d.word(|r| r.added), d.word(|r| r.arm)),
        format!("curves {}", curves.join(" ")),
    ]
    .join("\n")
}


// ── what each fixture opens as ──────────────────────────────────────────

/// Each settings fixture and DAW blob, observed (`observe_settings`). Pinned
/// as written: the colours say which legacy strip each V32 strip came from
/// (100 + i), the bays which plugin, the masks which bits.
const SETTINGS_WANT: [(&str, &str); 11] = [
    ("settings-v12.json", r#"colors [100,101,102,103,104,105,106,107,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,110,0]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.1,0.11]
names ["s0","s1","s2","s3","s4","s5","s6","s7","","","","","","","","","","","","","","","","","","","","","","","","","","s9","s10","s11"]
inserts 0=@tangent-dx7 3=/x/s1.vst3 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 99=/x/s9.vst3 102=/x/s10.vst3 105=/x/s11.vst3 107=/x/s11b2.vst3
muted 0x200000004 soloed 0x400000020 phase 0x0
kinds [1,0,0,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["midi:all","","","0","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v13 v14 v18 v19 v20"#),
    ("settings-v17-pads16.json", r#"colors [100,101,102,103,104,105,106,107,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,108,109,110,111]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.09,0.1,0.11]
names ["s0","s1","s2","s3","s4","s5","s6","s7","","","","","","","","","","","","","","","","","","","","","","","","","s8","s9","s10","s11"]
inserts 0=@tangent-drums 3=/x/s1.vst3 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 96=/x/s8.vst3 99=/x/s9.vst3 102=/x/s10.vst3 105=/x/s11.vst3 216=/x/pad0.vst3 223=/x/pad2.vst3 263=/x/pad15.vst3
muted 0x200000000 soloed 0x8 phase 0x0
kinds [1,1,0,0,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v18 v19 v20"#),
    ("settings-v18.json", r#"colors [100,101,102,103,104,105,106,107,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,108,109,110,111]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.09,0.1,0.11]
names ["s0","s1","s2","s3","s4","s5","s6","s7","","","","","","","","","","","","","","","","","","","","","","","","","s8","s9","s10","s11"]
inserts 0=/x/s0.vst3 3=/x/s1.vst3 6=/x/s2.vst3 9=/x/s3.vst3 12=@tangent-drums 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 96=/x/s8.vst3 99=/x/s9.vst3 102=/x/s10.vst3 105=/x/s11.vst3 1647=/x/pad0.vst3 1768=/x/pad40.vst3 2030=/x/pad127.vst3
muted 0x100000004 soloed 0x400000000 phase 0x0
kinds [1,1,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v19 v20"#),
    ("settings-v19.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,116,0,118,0]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.17,0.18,0.19]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","","","","","","","","","","","","","","","","","s16","s17","s18","s19"]
inserts 0=@tangent-drums 3=@tangent-dx7 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 24=/x/s8.vst3 27=/x/s9.vst3 30=/x/s10.vst3 33=/x/s11.vst3 36=/x/s12.vst3 39=/x/s13.vst3 42=/x/s14.vst3 45=/x/s15.vst3 96=/x/s16.vst3 99=/x/s17.vst3 102=/x/s18.vst3 105=/x/s19.vst3 107=/x/s19b2.vst3 108=/x/pad0.vst3 229=/x/pad40.vst3 491=/x/pad127.vst3 493=/x/master-extra.vst3 1278=/x/c3pad5.vst3
muted 0x200000004 soloed 0x400000020 phase 0x900000000
kinds [1,1,2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v20"#),
    ("settings-v20-27.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,0,0,0,0,0,0,0,0]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27"]
inserts 0=@tangent-drums 3=@tangent-dx7 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 24=/x/s8.vst3 27=/x/s9.vst3 30=/x/s10.vst3 33=/x/s11.vst3 36=/x/s12.vst3 39=/x/s13.vst3 42=/x/s14.vst3 45=/x/s15.vst3 48=/x/s16.vst3 51=/x/s17.vst3 54=/x/s18.vst3 57=/x/s19.vst3 60=/x/s20.vst3 63=/x/s21.vst3 66=/x/s22.vst3 69=/x/s23.vst3 72=/x/s24.vst3 75=/x/s25.vst3 78=/x/s26.vst3 81=/x/s27.vst3 83=/x/s27b2.vst3 84=/x/pad0.vst3 205=/x/pad40.vst3 467=/x/pad127.vst3 469=/x/master-extra.vst3 1254=/x/c3pad5.vst3 9302=/x/c23pad127.vst3
muted 0x2000004 soloed 0x4200000 phase 0x9000008
kinds [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,1,0,0,0,0,0,0,0,0]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles "#),
    ("settings-v20.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 24=/x/s8.vst3 27=/x/s9.vst3 30=/x/s10.vst3 33=/x/s11.vst3 36=/x/s12.vst3 39=/x/s13.vst3 42=/x/s14.vst3 45=/x/s15.vst3 48=/x/s16.vst3 51=/x/s17.vst3 54=/x/s18.vst3 57=/x/s19.vst3 60=/x/s20.vst3 63=/x/s21.vst3 66=/x/s22.vst3 69=/x/s23.vst3 72=/x/s24.vst3 75=/x/s25.vst3 78=/x/s26.vst3 81=/x/s27.vst3 84=/x/s28.vst3 87=/x/s29.vst3 90=/x/s30.vst3 93=/x/s31.vst3 96=/x/s32.vst3 99=/x/s33.vst3 102=/x/s34.vst3 105=/x/s35.vst3 229=/x/pad40.vst3 493=/x/master-extra.vst3 12396=/x/c31pad127.vst3 12416=/x/folder5-extra.vst3 12577=/x/c3pedal6.vst3
muted 0x200000004 soloed 0x400000000 phase 0x900000000
kinds [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles "#),
    ("settings-v21.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 107=/x/s35b2.vst3 229=/x/pad40.vst3 12416=/x/folder5-extra.vst3
muted 0x200000000 soloed 0x0 phase 0x800000000
kinds [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles "#),
    ("settings-v22.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 107=/x/s35b2.vst3 12416=/x/folder5-extra.vst3
muted 0x4 soloed 0x100000000 phase 0x200000000
kinds [1,1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["midi:all","midi:all","midi:Keystation 88","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles "#),
    ("daw-3x.json", r#"colors [3,7,16,21,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
sends [1.0,0.4,0.0,0.2,1.0,1.0,1.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.5]
names ["Piano","Lead","Mic","Amp","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","Sum"]
inserts 0=/x/Grand 8.vst3 1=/x/Pro-R 2.vst3 3=/x/Diva.vst3 4=/x/Pro-C 3.vst3 21=@tangent-drums 105=/x/Pro-L 2.vst3
muted 0x4 soloed 0x0 phase 0x0
kinds [1,1,2,2,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["midi:all","midi:all","","","","","","midi:all","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v13 v14 v18 v19 v20"#),
    ("daw-4x.json", r#"colors [100,101,102,103,104,105,106,107,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,108,109,110,111]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.09,0.1,0.11]
names ["s0","s1","s2","s3","s4","s5","s6","s7","","","","","","","","","","","","","","","","","","","","","","","","","s8","s9","s10","s11"]
inserts 0=@tangent-dx7 21=@tangent-drums 97=/x/drumbus.vst3 105=/x/Pro-L 2.vst3 2916=/x/pad3.vst3
muted 0x100000000 soloed 0x2 phase 0x0
kinds [1,2,0,0,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["","0","","","","","","midi:all","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v18 v19 v20"#),
    ("daw-5x.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,116,117,118,119]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.17,0.18,0.19]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","","","","","","","","","","","","","","","","","s16","s17","s18","s19"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 105=/x/Pro-L 2.vst3 217=/x/kick.vst3 887=/x/c2pad2.vst3
muted 0x200000000 soloed 0x10 phase 0x100000000
kinds [1,1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v20"#),
];

/// Each document fixture, observed (`observe_document`).
const DOCUMENTS_WANT: [(&str, &str); 6] = [
    ("doc-v4-11.json", r#"colors [100,101,102,103,104,105,106,107,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,108,109,110,111]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.09,0.1,0.11]
names ["s0","s1","s2","s3","s4","s5","s6","s7","","","","","","","","","","","","","","","","","","","","","","","","","s8","s9","s10","s11"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 100=/x/click.vst3 107=/x/master.vst3 222=/x/pad2.vst3 262=/x/pad15.vst3
muted 0x200000001 soloed 0x400000000 phase None
tracks_added 0xff armed 0x2
curves 0:gain 32:gain"#),
    ("doc-v5-18.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,116,117,118]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.17,0.18]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","","","","","","","","","","","","","","","","","","s16","s17","s18"]
inserts 0=@tangent-drums 3=@tangent-dx7 45=/x/c15.vst3 99=/x/click.vst3 103=/x/fx.vst3 107=/x/master.vst3
muted 0x200000000 soloed 0x400000008 phase None
tracks_added 0xffff armed 0x2
curves 3:pan 32:gain 32:mute"#),
    ("doc-v5-19.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,116,117,118,119]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.17,0.18,0.19]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","","","","","","","","","","","","","","","","","s16","s17","s18","s19"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 99=/x/click.vst3 107=/x/master.vst3 229=/x/pad40.vst3 492=/x/master-extra.vst3 1280=/x/c3pad5.vst3
muted 0x200000000 soloed 0x100000004 phase None
tracks_added 0x7 armed 0x2
curves 2:gain 32:gain"#),
    ("doc-v6-27.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27"]
inserts 0=@tangent-drums 3=@tangent-dx7 69=/x/c23.vst3 72=/x/drumbus.vst3 79=/x/fx.vst3 83=/x/master.vst3 465=/x/pad127.vst3 469=/x/master-extra.vst3 1256=/x/c3pad5.vst3
muted 0x2800000 soloed 0x4000000 phase None
tracks_added 0x800003 armed 0x2
curves 23:gain 24:gain 24:pan"#),
    ("doc-v6-35.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 93=/x/c31.vst3 96=/x/drumbus.vst3 107=/x/master.vst3 229=/x/pad40.vst3
muted 0x200000000 soloed 0x80000000 phase None
tracks_added 0x80000003 armed 0x80000000
curves 31:gain 32:gain"#),
    ("doc-v7.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 107=/x/master.vst3 229=/x/pad40.vst3 12416=/x/folder5-extra.vst3
muted 0x4 soloed 0x0 phase Some(38654705664)
tracks_added 0x3 armed 0x2
curves 1:gain 32:gain 32:fx:4:7"#),
];

/// **The 24-channel window as it opens once [`crate::desk_lift::V24_LIFT`]
/// is on**: buses and master at 32..35, pads +8, the master's curves on
/// the master. Pinned now, read under `with_v24_lift`, so the switch flips
/// onto a lift that is already tested; the tables above pin what the window
/// opens as today (the base's picture, each plugin beside its own sidecar).
const V24_LIFTED_WANT: [(&str, &str); 2] = [
    ("settings-v20-27.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,0,0,0,0,0,0,0,0,124,125,126,127]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.25,0.26,0.27]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","","","","","","","","","s24","s25","s26","s27"]
inserts 0=@tangent-drums 3=@tangent-dx7 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 24=/x/s8.vst3 27=/x/s9.vst3 30=/x/s10.vst3 33=/x/s11.vst3 36=/x/s12.vst3 39=/x/s13.vst3 42=/x/s14.vst3 45=/x/s15.vst3 48=/x/s16.vst3 51=/x/s17.vst3 54=/x/s18.vst3 57=/x/s19.vst3 60=/x/s20.vst3 63=/x/s21.vst3 66=/x/s22.vst3 69=/x/s23.vst3 96=/x/s24.vst3 99=/x/s25.vst3 102=/x/s26.vst3 105=/x/s27.vst3 107=/x/s27b2.vst3 108=/x/pad0.vst3 229=/x/pad40.vst3 491=/x/pad127.vst3 493=/x/master-extra.vst3 1278=/x/c3pad5.vst3 9326=/x/c23pad127.vst3
muted 0x200000004 soloed 0x400200000 phase 0x900000008
kinds [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,1,0,0,0,0,0,0,0,0]
inputs ["","","","","","","","","","","","","","","","","","","","","","","","","","","","","","","",""]
shuffles v24"#),
    ("doc-v6-27.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,0,0,0,0,0,0,0,0,124,125,126,127]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.25,0.26,0.27]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","","","","","","","","","s24","s25","s26","s27"]
inserts 0=@tangent-drums 3=@tangent-dx7 69=/x/c23.vst3 96=/x/drumbus.vst3 103=/x/fx.vst3 107=/x/master.vst3 489=/x/pad127.vst3 493=/x/master-extra.vst3 1280=/x/c3pad5.vst3
muted 0x200800000 soloed 0x400000000 phase None
tracks_added 0x800003 armed 0x2
curves 23:gain 32:gain 32:pan"#),
];

/// The picture `name` opens to in this build: the table's, or the lifted
/// window's once [`crate::desk_lift::V24_LIFT`] is on, so the switch flips
/// onto pictures already pinned.
fn want_now(name: &str, table: &'static str) -> &'static str {
    V24_LIFTED_WANT
        .iter()
        .find(|(n, _)| crate::desk_lift::V24_LIFT && *n == name)
        .map_or(table, |(_, w)| w)
}

/// **Every settings file and DAW blob opens to its literal picture.**
#[test]
fn every_settings_fixture_opens_to_its_literal_picture() {
    for ((name, text), (want_name, want)) in SETTINGS.iter().zip(SETTINGS_WANT) {
        assert_eq!(*name, want_name, "the tables are out of step");
        assert_eq!(observe_settings(text), want_now(name, want), "{name} opened differently");
    }
}

/// **Every document opens to its literal picture**, master curves included.
#[test]
fn every_document_fixture_opens_to_its_literal_picture() {
    for ((name, text), (want_name, want)) in DOCUMENTS.iter().zip(DOCUMENTS_WANT) {
        assert_eq!(*name, want_name, "the tables are out of step");
        assert_eq!(observe_document(text), want_now(name, want), "{name} opened differently");
    }
}

/// **The 24-channel window opens to its lifted picture under the switch**,
/// and to the base's without it (the tables above).
#[test]
fn the_24_channel_window_opens_to_its_lifted_picture_under_the_switch() {
    for (name, want) in V24_LIFTED_WANT {
        let (_, text) = SETTINGS
            .iter()
            .chain(DOCUMENTS.iter())
            .find(|(n, _)| *n == name)
            .expect("fixture");
        let seen = crate::desk_lift::with_v24_lift(|| {
            if name.starts_with("settings") {
                observe_settings(text)
            } else {
                observe_document(text)
            }
        });
        assert_eq!(seen, want, "{name} lifted differently");
    }
}

/// **Opened once, every fixture is a fixed point**: written by this build
/// and read back, it writes the same bytes and opens to the same picture.
/// A DAW blob was the retired VST3's whole persistence, so a blob that
/// drifted on each save drifted in every project that held it; the JSON
/// road keeps the same promise.
#[test]
fn every_fixture_round_trips_once_opened() {
    round_trips();
    crate::desk_lift::with_v24_lift(round_trips);
}

fn round_trips() {
    // Compared by the pictures a legacy file and a format-2 one must agree
    // on (`settings_picture`, `observe_document_model`): what this build
    // writes is format 2, which spells a name or an input the legacy array
    // stopped short of as an empty one, and a desk by its rows.
    for (name, text) in SETTINGS {
        let once = Settings::from_json(text).to_json();
        let twice = Settings::from_json(&once).to_json();
        assert_eq!(once, twice, "{name} is not a fixed point");
        assert_eq!(settings_picture(&once), settings_picture(text), "{name} changed through a save");
    }
    for (name, text) in DOCUMENTS {
        let once = Arrangement::from_json(text).to_json();
        let twice = Arrangement::from_json(&once).to_json();
        assert_eq!(once, twice, "{name} is not a fixed point");
        assert_eq!(observe_document_model(&once), observe_document_model(text), "{name} changed through a save");
    }
}

/// Writes every fixture's observation into `$TANGENT_ERA_DUMP`.
#[test]
#[ignore = "writes files; run by hand with TANGENT_ERA_DUMP set"]
fn write_observations() {
    let Some(dir) = std::env::var_os("TANGENT_ERA_DUMP") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("dump dir");
    for (name, text) in SETTINGS {
        std::fs::write(dir.join(format!("{name}.txt")), observe_settings(text)).expect("write");
    }
    for (name, text) in DOCUMENTS {
        std::fs::write(dir.join(format!("{name}.txt")), observe_document(text)).expect("write");
    }
}

/// **A 27-sends file lifts its buses, its master and its master's curves.**
/// The 24-channel window (9224522..936f3bb) had no arm, so its drum bus,
/// click, fx return and master opened on channels 24..27 and its master's
/// automation on channel 24. Red at the base.
///
/// Read under `with_v24_lift`: the lift is pinned here and switched on
/// with the copy-only shelf (see `crate::desk_lift::V24_LIFT`).
#[test]
fn a_27_sends_file_lifts_its_buses_master_and_master_curves() {
    crate::desk_lift::with_v24_lift(a_27_sends_file_lifts);
}

fn a_27_sends_file_lifts() {
    use crate::recorder::StripRef;
    use crate::settings::StripSwitch;
    let (_, text) = fixture!("settings-v20-27.json");
    let s = Settings::from_json(text);
    // The fixture paints strip i with 100 + i: drums 24, click 25, fx 26,
    // master 27 in the window; 32, 33, 34, 35 in V32. Read in the legacy
    // strip space the specials were joined back into (W5: the live desk
    // keeps them apart, by what each is, as the second half checks).
    let strips = s.strip_space();
    assert_eq!(&strips.colors[32..36], &[124, 125, 126, 127], "the buses and master");
    assert_eq!(&strips.colors[24..32], &[0; 8], "the widened channels came up painted");
    assert_eq!(strips.names[35], "s27", "the master's name");
    let paint: Vec<i64> = StripRef::SPECIALS.iter().map(|sr| s.strip_paint(*sr)).collect();
    assert_eq!(paint, [124, 125, 126, 127], "the specials do not hold their own paint");
    assert_eq!(s.strip_name(StripRef::Master), "s27", "the master does not hold its own name");
    assert_eq!(s.legacy_inserts()[32 * 3], "/x/s24.vst3", "the drum bus's bay");
    assert_eq!(s.legacy_inserts()[35 * 3 + 2], "/x/s27b2.vst3", "the master's bay");
    assert_eq!(s.legacy_inserts()[164 * 3 + 1], "/x/master-extra.vst3", "the master's extra rack");
    assert_eq!(s.legacy_inserts()[(165 + 2 * 128 + 5) * 3], "/x/c3pad5.vst3", "channel 3's pad 5");
    assert_eq!(strips.muted, (1 << 33) | (1 << 2), "the muted click");
    assert_eq!(strips.soloed, (1 << 34) | (1 << 21), "the soloed fx");
    assert_eq!(strips.phase, (1 << 32) | (1 << 35) | (1 << 3), "the inverted drum bus and master");
    assert_eq!((s.word(|r| r.mute), s.word(|r| r.solo), s.word(|r| r.phase)), (1 << 2, 1 << 21, 1 << 3), "the rows' own switches");
    assert!(s.strip_switch(StripRef::Click, StripSwitch::Mute), "the click does not hold its mute");
    assert!(s.strip_switch(StripRef::Fx, StripSwitch::Solo), "the fx return does not hold its solo");
    assert!(
        s.strip_switch(StripRef::Drums, StripSwitch::Phase) && s.strip_switch(StripRef::Master, StripSwitch::Phase),
        "the drum bus and the master do not hold their polarity"
    );

    let (_, text) = fixture!("doc-v6-27.json");
    let a = Arrangement::from_json(text);
    let d = a.desk.as_ref().expect("a desk");
    let doc_strips = d.strip_space();
    assert_eq!(&doc_strips.colors[32..36], &[124, 125, 126, 127], "the document's buses and master");
    let paint: Vec<i64> = d.specials.iter().map(|(_, sp)| sp.color).collect();
    assert_eq!(paint, [124, 125, 126, 127], "the document's specials do not hold their own paint");
    let bays = d.legacy_bays();
    let seat = |bay: usize| bays.iter().find(|(b, _)| *b == bay).map(|(_, p)| p.as_str());
    assert_eq!(seat(32 * 3), Some("/x/drumbus.vst3"));
    assert_eq!(seat(35 * 3 + 2), Some("/x/master.vst3"));
    assert_eq!(doc_strips.muted, (1 << 33) | (1 << 23), "the document's muted click");
    assert!(d.specials.click.mute && d.word(|r| r.mute) == 1 << 23, "the document's click does not hold its own mute");
    // The master's by the lane number its era filed it under (W6 names it).
    let lanes: Vec<(usize, String)> = a
        .automation
        .curves()
        .iter()
        .map(|c| (c.lane.index_or(crate::desk_lift::V32_MASTER_LANE), c.param.key()))
        .collect();
    assert!(lanes.contains(&(32, "gain".to_owned())), "the master's gain curve: {lanes:?}");
    assert!(lanes.contains(&(32, "pan".to_owned())), "the master's pan curve: {lanes:?}");
    assert!(lanes.contains(&(23, "gain".to_owned())), "channel 23's own curve: {lanes:?}");
    assert!(!lanes.iter().any(|(l, _)| *l == 24), "a master curve stayed on channel 24: {lanes:?}");
}

/// **A sixteen-era phase-inverted drum bus lands on strip 32**, and its
/// inverted master on 35. `adopt_desk` lifted mute and solo and left the
/// phase mask where the file had it. Red at the base.
#[test]
fn a_sixteen_era_phase_inverted_drum_bus_lands_on_strip_32() {
    use crate::recorder::StripRef;
    use crate::settings::StripSwitch;
    let (_, text) = fixture!("settings-v19.json");
    let s = Settings::from_json(text);
    // In the legacy strip space the specials were joined back into, and by
    // what each strip is (W5).
    let strips = s.strip_space();
    assert_eq!(strips.phase, (1 << 32) | (1 << 35), "phase did not follow the drum bus and master");
    assert_eq!(s.word(|r| r.phase), 0, "a row took the drum bus's polarity");
    assert!(
        s.strip_switch(StripRef::Drums, StripSwitch::Phase) && s.strip_switch(StripRef::Master, StripSwitch::Phase),
        "the drum bus and the master do not hold their polarity"
    );
    // And it is the same lift that moved mute and solo.
    assert_eq!(strips.muted, (1 << 33) | (1 << 2));
    assert_eq!(strips.soloed, (1 << 34) | (1 << 5));
}

// ── provenance, the shelf map and the scans ─────────────────────────────

use crate::desk_lift::{
    legacy_shelf_seat, provenance_of, provenance_of_document, DeskEra, ShelfShuffles,
};
use serde_json::Value;

/// Every bay of a document's dense `inserts` (and its tail) replaced by a tag
/// naming the legacy bay it sat at, so a read says where each one went.
fn tagged(text: &str) -> Value {
    let mut v: Value = serde_json::from_str(text).expect("fixture");
    let desk = v["desk"].as_object_mut().expect("a desk");
    if let Some(Value::Array(a)) = desk.get_mut("inserts") {
        for (i, x) in a.iter_mut().enumerate() {
            *x = Value::String(format!("/tag/{i}"));
        }
    }
    if let Some(Value::Object(t)) = desk.get_mut("inserts_tail") {
        for (k, x) in t.iter_mut() {
            *x = Value::String(format!("/tag/{k}"));
        }
    }
    v
}

/// **Provenance maps every lifted bay back to the legacy bay it came from**,
/// for every era fixture, because it is the reader's own lift run over bay
/// numbers. Every bay of the file is tagged with its own index, the document
/// is read the ordinary way, and each tag found at a V32 bay must be what
/// `provenance_of_document` names for that bay. And none is lost: every
/// legacy bay arrives somewhere.
#[test]
fn provenance_maps_every_lifted_bay_back_to_its_legacy_bay() {
    // The window reads unlifted until the switch, and its provenance is the
    // identity then, as its reader is.
    let window = if crate::desk_lift::V24_LIFT { DeskEra::V24 } else { DeskEra::V32 };
    provenance_agrees_with_the_reader(window);
    crate::desk_lift::with_v24_lift(|| provenance_agrees_with_the_reader(DeskEra::V24));
}

fn provenance_agrees_with_the_reader(window: DeskEra) {
    for (name, text) in DOCUMENTS {
        let raw: Value = serde_json::from_str(text).expect("fixture");
        let p = provenance_of_document(&raw).expect("a legacy desk has a provenance");
        let doc = tagged(text);
        let read = Arrangement::from_json(&doc.to_string());
        let desk = read.desk.expect("a desk");
        let dense = doc["desk"]["inserts"].as_array().map_or(0, Vec::len);
        let tail = doc["desk"]["inserts_tail"].as_object().map_or(0, |t| t.len());
        let flat = desk.legacy_bays();
        assert_eq!(flat.len(), dense + tail, "{name}: a bay was lost or made up");
        for (bay, path) in &flat {
            let legacy: u32 = path.strip_prefix("/tag/").and_then(|n| n.parse().ok()).expect("tag");
            assert_eq!(
                p.legacy_bay(*bay as u32),
                Some(legacy),
                "{name} ({}): V32 bay {bay} came from legacy bay {legacy}",
                p.era
            );
        }
        // A file at its era's whole length is that era's whole map.
        if dense == p.era.bays() {
            assert_eq!(p, provenance_of(p.era), "{name}: the document and the era disagree");
        }
    }
    // The eras, by name, as each fixture is.
    let eras: Vec<DeskEra> = DOCUMENTS
        .iter()
        .map(|(_, t)| provenance_of_document(&serde_json::from_str(t).expect("fixture")).expect("p").era)
        .collect();
    assert_eq!(
        eras,
        [DeskEra::V8Pads16, DeskEra::V16NoDrumBus, DeskEra::V16, window, DeskEra::V32, DeskEra::V32]
    );
    // No desk is no provenance; a format-2 desk answers its marker.
    assert_eq!(provenance_of_document(&serde_json::json!({"clips": []})), None);
    assert_eq!(
        provenance_of_document(&serde_json::json!({"desk": {"format": 2, "legacy_states": "v24"}})),
        Some(provenance_of(DeskEra::V24))
    );
    assert_eq!(provenance_of_document(&serde_json::json!({"desk": {"format": 2}})), None);
}

/// The frozen in-place renames, transcribed from the host's
/// `shift_insert_states_for_v13`..`v20` loop for loop (and the v24 one the
/// 24-channel window would have needed), run over an in-memory shelf:
/// `seat -> the seat the file started at`. A rename overwrites, as
/// `std::fs::rename` does.
fn run_in_place(set: ShelfShuffles, shelf: &mut std::collections::BTreeMap<usize, usize>) {
    let mv = |shelf: &mut std::collections::BTreeMap<usize, usize>, from: usize, to: usize| {
        if let Some(f) = shelf.remove(&from) {
            shelf.insert(to, f);
        }
    };
    if set.contains(ShelfShuffles::V13) {
        shelf.remove(&8);
        for old in 9..=11 {
            mv(shelf, old, old - 1);
        }
    }
    if set.contains(ShelfShuffles::V14) {
        for old in (8..=10).rev() {
            mv(shelf, old, old + 1);
        }
    }
    if set.contains(ShelfShuffles::V18) {
        for pad in (0..16usize).rev() {
            mv(shelf, 12 + pad, 12 + 36 + pad);
        }
    }
    if set.contains(ShelfShuffles::V19) {
        for old in (8..(12 + 128)).rev() {
            mv(shelf, old, old + 8);
        }
    }
    if set.contains(ShelfShuffles::V20) {
        for old in (16..2069).rev() {
            mv(shelf, old, old + 16);
        }
    } else if set.contains(ShelfShuffles::V24) {
        for old in (24..=3100).rev() {
            mv(shelf, old, old + 8);
        }
    }
}

/// **The composed shelf map is the frozen in-place renames, exactly.** For
/// every set an era can raise (every combination of the version shuffles,
/// alone or with v20 or v24), a shelf holding EVERY V32 seat is shuffled in
/// place, and `legacy_shelf_seat` must name where each file ended, or `None`
/// where the renames deleted or overwrote it.
#[test]
fn the_composed_shelf_map_is_the_frozen_in_place_renames() {
    // **The transcription is the code the host ran**, held verbatim to its
    // loop headers for as long as that code stood (`run_in_place` above),
    // and frozen with it since: the copy-only conversion replaced it
    // (W4-integrate (B); 6e371d1 is the last tree that renamed in place).
    // What the host must now be is a COPY through this map, never a rename.
    let host = include_str!("../../tangent/src/desktop.rs");
    // Production only: the host's own tests spell the name to forbid it.
    let host = &host[..host.find("\nmod tests {").unwrap_or(host.len())];
    assert!(!host.contains("fn shift_insert_states_for_v"), "an in-place shelf rename is back in the host");
    let shelf = include_str!("../../tangent/src/shelf.rs");
    assert!(
        shelf.contains("legacy_shelf_seat(set, legacy)"),
        "the global shelf's conversion no longer copies through this map"
    );
    let version = [ShelfShuffles::V13, ShelfShuffles::V14, ShelfShuffles::V18, ShelfShuffles::V19];
    for bits in 0..16u8 {
        for widen in [ShelfShuffles::NONE, ShelfShuffles::V20, ShelfShuffles::V24] {
            let mut set = widen;
            for (i, s) in version.iter().enumerate() {
                if bits & (1 << i) != 0 {
                    set.insert(*s);
                }
            }
            let mut shelf: std::collections::BTreeMap<usize, usize> =
                (0..4392).map(|s| (s, s)).collect();
            run_in_place(set, &mut shelf);
            let mut landed = std::collections::BTreeMap::new();
            for (at, from) in &shelf {
                landed.insert(*from, *at);
            }
            for seat in 0..4392 {
                assert_eq!(
                    legacy_shelf_seat(set, seat),
                    landed.get(&seat).copied(),
                    "{:?}: seat {seat}",
                    set.names()
                );
            }
        }
    }
}

/// **Each era's settings raise the shelf set that takes their seats to V32**,
/// and the map then agrees with the lifted desk: a plugin at legacy seat S
/// is at V32 seat `legacy_shelf_seat(set, S)` on the desk too.
#[test]
fn each_settings_era_raises_the_shelf_set_its_desk_lift_needs() {
    let names = |name: &str| {
        let (_, text) = SETTINGS.iter().find(|(n, _)| *n == name).expect("fixture");
        Settings::from_json(text).shelf_shuffles().names()
    };
    assert_eq!(names("settings-v12.json"), ["v13", "v14", "v18", "v19", "v20"]);
    assert_eq!(names("settings-v17-pads16.json"), ["v18", "v19", "v20"]);
    assert_eq!(names("settings-v18.json"), ["v19", "v20"]);
    assert_eq!(names("settings-v19.json"), ["v20"]);
    // The window raises its shuffle only where it is lifted: an unlifted
    // window's plugins sit on their own seats and need no shelf move.
    let window: &[&str] = if crate::desk_lift::V24_LIFT { &["v24"] } else { &[] };
    assert_eq!(names("settings-v20-27.json"), window);
    assert_eq!(crate::desk_lift::with_v24_lift(|| names("settings-v20-27.json")), ["v24"]);
    assert_eq!(names("settings-v20.json"), Vec::<&str>::new());
    assert_eq!(names("settings-v22.json"), Vec::<&str>::new());
    // The shelf and the desk agree, seat for seat, for the eras that moved
    // only by widening: the drum bus, the master, the master's extra and a
    // channel pad of the 16 and 24 eras.
    for (name, set, pairs) in [
        ("settings-v19.json", ShelfShuffles::V20, &[(16, 32), (19, 35), (148, 164), (410, 426)][..]),
        ("settings-v20-27.json", ShelfShuffles::V24, &[(24, 32), (27, 35), (156, 164), (418, 426)][..]),
        ("settings-v18.json", ShelfShuffles::V19 | ShelfShuffles::V20, &[(8, 32), (11, 35), (12, 36)][..]),
    ] {
        for (legacy, v32) in pairs {
            assert_eq!(legacy_shelf_seat(set, *legacy), Some(*v32), "{name}: seat {legacy}");
        }
    }
}

/// The host's global-shelf and `state/` file name for a seat's bay:
/// `desktop.rs`'s `sidecar_name`, transcribed and held to it below.
fn sidecar(seat: usize, bay: usize) -> String {
    format!("insert-state-{seat}-{bay}.bin")
}

/// **What a session does to a seat-named shelf**, as the host does it: every
/// plugin reads its seat's file, keeps the knobs only when the file names
/// its own bundle (the owner check), and on save writes what it has back
/// over the same name, its own knobs or its defaults. `bays` is the desk as
/// opened, flat V32 bay to plugin. Answers how many plugins opened at
/// their defaults (no file, or somebody else's).
fn host_load_and_save<'a>(dir: &std::path::Path, bays: impl Iterator<Item = (usize, &'a str)>) -> usize {
    let mut at_defaults = 0;
    for (flat, bundle) in bays.filter(|(_, p)| !p.is_empty()) {
        let path = dir.join(sidecar(flat / 3, flat % 3));
        let knobs = std::fs::read(&path)
            .ok()
            .and_then(|b| {
                let nul = b.iter().position(|c| *c == 0)?;
                (&b[..nul] == bundle.as_bytes()).then(|| b[nul + 1..].to_vec())
            })
            .unwrap_or_else(|| {
                at_defaults += 1;
                b"defaults".to_vec()
            });
        let mut out = bundle.as_bytes().to_vec();
        out.push(0);
        out.extend_from_slice(&knobs);
        std::fs::write(&path, out).expect("write a sidecar");
    }
    at_defaults
}

/// Every file in `dir`, by name, with its bytes.
fn shelf_bytes(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("the shelf")
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).expect("read")))
        .collect()
}

/// A shelf of the file's own era: one sidecar per occupied legacy bay, at
/// that bay's seat, holding that plugin's bundle and knobs named for it.
fn legacy_shelf(tag: &str, legacy: &[Value]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tangent-era-shelf-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp shelf");
    for (flat, p) in legacy.iter().enumerate() {
        let Some(p) = p.as_str().filter(|p| !p.is_empty()) else { continue };
        let mut body = p.as_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(format!("knobs-of-legacy-bay-{flat}").as_bytes());
        std::fs::write(dir.join(sidecar(flat / 3, flat % 3)), body).expect("seed");
    }
    dir
}

/// **A 24-channel-window shelf survives a load and a save untouched.**
///
/// Until the copy-only shelf conversion landed, the host read and wrote
/// plugin state by SEAT over the legacy names, on the global shelf for the
/// settings desk and in a project's `state/` for a document. So the window
/// had to open with every plugin on the seat the file had it at, and a whole
/// session (open, every plugin's read, a save, a reopen, the reads and the
/// save again) had to leave every legacy file byte for byte as it was, each
/// plugin having found its own knobs. Under the lift it would not: the drum
/// bus at seat 32 opens what is the window's channel 0 pad 4 and writes over
/// it, which is why the lift waited (`crate::desk_lift::V24_LIFT`); that half
/// is asserted too, so this test cannot pass by testing nothing.
///
/// **The lift is on now, with the shelf that makes it safe**: the host names
/// every state by its bay's identity and only ever READS a legacy name
/// (`tangent/src/shelf.rs`), so the by-seat host above is the history this
/// transcription keeps, and the window's survival through the real
/// conversion and the real reader is `tangent`'s
/// `shelf::tests::every_era_projects_states_reach_their_own_bays` and
/// `shelf::tests::the_global_shelf_converts_every_settings_era`.
#[test]
fn a_24_channel_window_shelf_survives_a_load_and_save_untouched() {
    // The transcription is the host's legacy naming, which it now only reads.
    let host = include_str!("../../tangent/src/shelf.rs");
    for line in [
        "const LEGACY_PREFIX: &str = \"insert-state-\";",
        "format!(\"{LEGACY_PREFIX}{seat}-{bay}.bin\")",
    ] {
        assert!(host.contains(line), "the host's legacy sidecar names changed: `{line}` is gone");
    }

    let (_, text) = SETTINGS.iter().find(|(n, _)| *n == "settings-v20-27.json").expect("fixture");
    let raw: Value = serde_json::from_str(text).expect("fixture");
    let legacy = raw["strip_inserts"].as_array().expect("a dense desk").clone();
    let cycle = |tag: &str| {
        let dir = legacy_shelf(tag, &legacy);
        let before = shelf_bytes(&dir);
        let opened = Settings::from_json(text);
        let lost = host_load_and_save(&dir, opened.legacy_inserts().iter().map(String::as_str).enumerate());
        let reopened = Settings::from_json(&opened.to_json());
        host_load_and_save(&dir, reopened.legacy_inserts().iter().map(String::as_str).enumerate());
        let after = shelf_bytes(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        (before, after, lost, opened.shelf_shuffles())
    };
    let (before, after, lost, shuffles) = cycle("settings");
    if !crate::desk_lift::V24_LIFT {
        assert!(before.len() > 30, "the fixture seeds a real shelf");
        assert_eq!(lost, 0, "a plugin opened without its own knobs");
        assert_eq!(after, before, "a settings load and save changed a legacy sidecar");
        assert!(shuffles.is_empty(), "an unlifted window asked the host to move its shelf");
    }
    let (before, after, lost, _) = crate::desk_lift::with_v24_lift(|| cycle("settings-lifted"));
    assert!(lost > 0, "the lifted window's buses should miss their legacy knobs");
    assert_ne!(after, before, "the lifted window should write past its own shelf");

    let (_, text) = DOCUMENTS.iter().find(|(n, _)| *n == "doc-v6-27.json").expect("fixture");
    let raw: Value = serde_json::from_str(text).expect("fixture");
    let legacy = raw["desk"]["inserts"].as_array().expect("a dense desk").clone();
    let cycle = |tag: &str| {
        let dir = legacy_shelf(tag, &legacy);
        let before = shelf_bytes(&dir);
        let opened = Arrangement::from_json(text);
        let desk = opened.desk.as_ref().expect("a desk");
        let lost = host_load_and_save(&dir, desk.legacy_bays().iter().map(|(b, p)| (*b, p.as_str())));
        let reopened = Arrangement::from_json(&opened.to_json());
        let desk = reopened.desk.as_ref().expect("a desk");
        host_load_and_save(&dir, desk.legacy_bays().iter().map(|(b, p)| (*b, p.as_str())));
        let after = shelf_bytes(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        (before, after, lost)
    };
    let (before, after, lost) = cycle("doc");
    if !crate::desk_lift::V24_LIFT {
        assert!(before.len() > 5, "the fixture seeds a real state folder");
        assert_eq!(lost, 0, "a plugin opened without its own knobs");
        assert_eq!(after, before, "a document open and save changed a legacy sidecar in state/");
    }
    let (before, after, lost) = crate::desk_lift::with_v24_lift(|| cycle("doc-lifted"));
    assert!(lost > 0, "the lifted window's buses should miss their legacy knobs");
    assert_ne!(after, before, "the lifted window should write past its own state/");
}

/// **The 24-channel lift switches on only beside the copy-only shelf.**
///
/// Lifted, the window's plugins sit eight seats above their legacy
/// sidecars, and only the identity-named shelf (`convert_global_shelf`
/// copying through `legacy_shelf_seat`, and the project reader trying
/// pstate names, then legacy ones through the provenance) pairs them back
/// without writing over a legacy file. So `V24_LIFT` may be true only in a
/// tree that has both, and no tree may ever rename the window's shelf in
/// place.
#[test]
fn the_v24_lift_waits_for_the_copy_only_shelf() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let host = std::fs::read_to_string(root.join("tangent/src/desktop.rs")).expect("desktop.rs");
    assert!(
        !host.contains("fn shift_insert_states_for_v24"),
        "the 24-channel window's shelf must be copied, never renamed in place"
    );
    if crate::desk_lift::V24_LIFT {
        let shelf = std::fs::read_to_string(root.join("tangent/src/shelf.rs")).unwrap_or_default();
        for needed in ["fn convert_global_shelf", "PstateThenLegacy"] {
            assert!(shelf.contains(needed), "V24_LIFT is on without shelf.rs's `{needed}`");
        }
        assert!(host.contains("convert_global_shelf("), "V24_LIFT is on and the host never converts");
    }
}

/// Every identifier in `code` outside `//` comments.
pub(crate) fn idents(code: &str) -> Vec<&str> {
    code.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .flat_map(|l| l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')))
        .filter(|w| !w.is_empty())
        .collect()
}

/// The live capacity constants no historical migration may read: every one
/// of them is about to stop being a constant, and a lift that read one
/// would quietly redefine itself.
pub(crate) const BANNED: [&str; 14] = [
    "CHANNELS",
    "STRIPS",
    "TOTAL_SEATS",
    "INSERT_SPACE",
    "MASTER_LANE",
    "MASTER_AUTO_LANE",
    "MASTER_SEAT",
    "INPUTS",
    "SLOTS",
    "INSERTS",
    "DRUM_PADS",
    "Strip",
    "pad_rack_index",
    "recorder",
];

/// The banned identifiers a region reads, by name; `allow` lists the ones a
/// region may name for a reason it states.
pub(crate) fn banned_in(code: &str, allow: &[&str]) -> Vec<String> {
    let ids = idents(code);
    BANNED
        .iter()
        .filter(|b| !allow.contains(b) && ids.contains(b))
        .map(|b| (*b).to_owned())
        .collect()
}

/// The source between two anchors, the first included.
pub(crate) fn region<'a>(src: &'a str, from: &str, to: &str) -> &'a str {
    let a = src.find(from).unwrap_or_else(|| panic!("anchor `{from}` is gone"));
    let b = a + src[a..].find(to).unwrap_or_else(|| panic!("anchor `{to}` is gone"));
    &src[a..b]
}

/// **`desk_lift` names no live constant and no `recorder` item at all**, and
/// the document reader's lifts (the era lift, the master's curve lane, the
/// automation bound, the head) read only `desk_lift`'s literals.
#[test]
fn the_lifts_read_no_live_constant() {
    let lift = include_str!("desk_lift.rs");
    let code = &lift[..lift.find("#[cfg(test)]").expect("the tests")];
    assert_eq!(banned_in(code, &[]), Vec::<String>::new(), "desk_lift reads a live constant");
    let doc = include_str!("arrangement.rs");
    for (what, region) in [
        ("lift_eras", region(doc, "fn lift_eras(", "\n}\n")),
        ("read_automation", region(doc, "fn read_automation(", "Automation::from_curves(curves)")),
        ("from_map's master lane", region(doc, "let era = map", "let mut clips = Vec::new();")),
        ("HEAD_BAYS", region(doc, "pub const HEAD_BAYS", ";")),
    ] {
        assert_eq!(banned_in(region, &[]), Vec::<String>::new(), "arrangement.rs {what} reads a live constant");
    }
}

/// **The spellings of a real path**: Tangent's media root, its config
/// directory, and the staging twins the unlimited-tracks branch gave its
/// internal builds, which no code may spell again.
const REAL_PATH_SPELLINGS: [&str; 6] = [
    "\"Tangent Media",
    "\".config\"",
    "\".config/",
    "join(\"tangent\")",
    "\"tangent-staging",
    "Media Staging",
];

/// **The lines that may spell one, each exactly as written and once**, in
/// the file named: the config chooser's `.config`, the XDG user-dirs file's
/// (not Tangent's config), and the media chooser's real default.
///
/// **An exemption names a LINE, and the line spends it** (design
/// W8-safe-tests). It was a file and a fragment: any line of settings.rs
/// holding `.join("Tangent Media")` or `test_path()` passed, so a second
/// hard-coded media root beside the chooser passed as the chooser, and so
/// did the chooser's own line written twice. The chooser's other answer,
/// this crate's per-thread test root, needs none: it stands under
/// `#[cfg(test)]`, which the scan does not read.
const REAL_PATH_LINES: [(&str, &str); 3] = [
    ("tangent-core/src/config.rs", "home().unwrap_or_else(|| PathBuf::from(\".\")).join(\".config\")"),
    ("tangent-record/src/take.rs", ".unwrap_or_else(|| home.join(\".config\"));"),
    ("tangent-ui/src/settings.rs", ".join(\"Tangent Media\")"),
];

/// **Where `src`, the file at `rel`, spells a real path off the lines that
/// may**, one entry per line, with the indices of the [`REAL_PATH_LINES`]
/// it spent. It reads what ships, strings and all (the lexer's `live`
/// view): comments, and every item under a cfg no build outside a test
/// satisfies, are not read, since a fixture that names a folder of that name
/// under a temp root is not a path to the owner's files.
///
/// **Test code is what the lexer says it is**, the same reading as the ban
/// and copy scans (`ban_scan::is_test_cfg`, any item, however laid out).
/// This read a line at a time and skipped a whole `mod` block at column 0
/// when the line above it started `#[cfg(` and held the letters `test`
/// anywhere, so a `#[cfg(not(test))]` module or a `#[cfg(feature =
/// "latest")]` one, both of which ship, was skipped with every path in it
/// (design W8-safe-tests, items 2 and 4).
fn real_path_spellings(rel: &str, src: &str) -> (Vec<String>, Vec<usize>) {
    let mut found = Vec::new();
    let mut spent: Vec<usize> = Vec::new();
    let live = crate::copy_scan::lex(src).live;
    for (n, line) in live.lines().enumerate() {
        if !REAL_PATH_SPELLINGS.iter().any(|s| line.contains(s)) {
            continue;
        }
        let exempt = (0..REAL_PATH_LINES.len())
            .find(|k| REAL_PATH_LINES[*k] == (rel, line.trim()) && !spent.contains(k));
        match exempt {
            Some(k) => spent.push(k),
            None => found.push(format!("{rel}:{}: {}", n + 1, line.trim())),
        }
    }
    (found, spent)
}

/// **No code but the two choosers spells the real config directory or the
/// real media root**, so `TANGENT_CONFIG_DIR` and `TANGENT_MEDIA_ROOT`
/// cannot be routed around by a second copy of either path.
/// `tangent_core::config::dir` is the config's only home and
/// `Settings::default_media_root` the media's ([`REAL_PATH_LINES`]). The
/// staging twins the unlimited-tracks branch gave its internal builds
/// (`~/.config/tangent-staging`, `Documents/Tangent Media Staging`) went
/// with its last wave, and no code may spell either again.
#[test]
fn nothing_but_the_choosers_spells_the_real_config_or_media_path() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    let mut spent = Vec::new();
    for krate in ["tangent", "tangent-ui", "tangent-core", "tangent-host", "tangent-record", "plugin"] {
        let mut stack = vec![root.join(krate).join("src")];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                // This file spells them to look for them.
                if p.extension().and_then(|x| x.to_str()) != Some("rs") || p.ends_with("era_corpus.rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&p).expect("source");
                let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                let (hits, lines) = real_path_spellings(&rel, &src);
                found.extend(hits);
                spent.extend(lines);
            }
        }
    }
    assert!(found.is_empty(), "a second spelling of a real path: {found:#?}");
    // Every exemption still names a line that is there, or it would excuse
    // the next line written the same way.
    for (k, (file, line)) in REAL_PATH_LINES.iter().enumerate() {
        assert!(spent.contains(&k), "{file} no longer has `{line}`: the exemption names nothing, drop or move it");
    }
}

/// **A second media root beside the chooser fails the scan, the chooser's
/// own line written again included, and so does one in a module that
/// ships, whatever its cfg spells** (design W8-safe-tests). Planted in
/// settings.rs beside the chooser as it is written there (the real default,
/// and the per-thread test root under `#[cfg(test)]`, which is test code
/// and not read), and the chooser alone in another file. Revert checks: the
/// fragment rule this replaced passes the first four plants, and the line
/// reader it read through skipped the `not(test)`, `feature = "latest"`,
/// `all(unix, not(test))` and `feature = "test-harness"` modules whole.
#[test]
fn a_second_media_root_beside_the_chooser_fails_the_scan_when_planted() {
    let chooser = "fn default_media_root() -> PathBuf {\n    documents()\n        .join(\"Tangent Media\")\n}\n\
                   fn media_root(&self) -> PathBuf {\n    #[cfg(test)]\n    {\n        \
                   return test_path().with_file_name(\"Tangent Media\");\n    }\n    #[cfg(not(test))]\n    {\n        \
                   Self::default_media_root()\n    }\n}\n";
    let (hits, spent) = real_path_spellings("tangent-ui/src/settings.rs", chooser);
    assert!(hits.is_empty(), "the chooser itself was refused: {hits:?}");
    assert_eq!(spent, [2]);
    for plant in [
        "    let elsewhere = home.join(\"Documents\").join(\"Tangent Media\");",
        "        .join(\"Tangent Media\")",
        "    let root = test_path().with_file_name(\"Tangent Media\");",
        "    return test_path().with_file_name(\"Tangent Media\");",
        "    let root = PathBuf::from(\"Tangent Media\"); // test_path()",
        "    let twin = documents().join(\"Tangent Media Staging\");",
    ] {
        let src = format!("{chooser}fn second() -> PathBuf {{\n{plant}\n}}\n");
        let (hits, _) = real_path_spellings("tangent-ui/src/settings.rs", &src);
        assert_eq!(hits.len(), 1, "passed as the chooser: {plant}");
    }
    // A module that ships is read whatever its cfg spells; a test module is
    // not, however its declaration is laid out.
    let body = "    fn root() -> PathBuf {\n        documents().join(\"Tangent Media\")\n    }\n}\n";
    for (module, ships) in [
        ("#[cfg(not(test))]\nmod legacy {", true),
        ("#[cfg(feature = \"latest\")]\npub(crate) mod x {", true),
        ("#[cfg(all(unix, not(test)))]\nmod y {", true),
        ("#[cfg(feature = \"test-harness\")]\nmod z {", true),
        ("#[cfg(test)]\nmod tests {", false),
        ("#[cfg(all(test, unix))]\npub(crate) mod tests {", false),
        ("#[cfg(test)] mod tests {", false),
        ("#[cfg(test)]\n// Why it is a test.\nmod tests {", false),
    ] {
        let src = format!("{chooser}{module}\n{body}");
        let (hits, _) = real_path_spellings("tangent-ui/src/settings.rs", &src);
        assert_eq!(hits.len(), usize::from(ships), "{module}: {hits:?}");
    }
    let (hits, spent) = real_path_spellings("tangent/src/desktop.rs", chooser);
    assert_eq!(hits.len(), 1, "the chooser's real default passed in another file");
    assert!(spent.is_empty());
}


// ── format 2 (W3-persist-v23-v8) ─────────────────────────────────────────
//
// The format-2 writers are reached through their `*_as(Format::V2)` entry
// points: `desk_format::WRITE_V8` is never flipped here.

use crate::desk_format::Format;

/// FNV-1a over a text, for pinning bytes without committing them.
fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// **What production writes for the default settings**, pinned at the
/// flip (W4-integrate (B)): the format-2 file a fresh install's first save
/// writes, stamp included. The fixtures' are `FORMAT2_BYTES` below.
///
/// Moved once since, for 5.2, by the shipped piano's path alone
/// (`@tangent-keys` where `@tangent-dx7` was, one byte longer): the new
/// bytes with the old path put back hash to the old pin, 0xe028_4149_425a_c38c
/// over 30430. Moved again for 5.3 by `"welcome_seen":""` alone
/// (`Settings::welcome_seen`, empty on a fresh install so the bytes never
/// follow the version): taken out, the bytes hash to the 5.2 pin,
/// 0x6a72_ca52_e572_89f1 over 30431.
const DEFAULT_V2_BYTES: (u64, usize) = (0x909e_1ec0_9e9c_0cc9, 30449);

/// **Production writes the format-2 bytes**, stamps included, for every
/// fixture and for the default settings: `WRITE_V8` is on, so what a user's
/// install writes is exactly what the format-2 writers write, and the legacy
/// desk is never written again. Until the flip this pinned the base's legacy
/// bytes (8494d80) for the same fixtures, which no writer of this tree
/// writes any more: the legacy writer now stamps what this build is.
#[test]
fn production_writes_the_format_2_bytes() {
    assert!(crate::desk_format::WRITE_V8, "the switch is off: production writes the legacy desk");
    // `to_json` is the JSON road's text (compact, a DAW blob's shape); the
    // file is the same map pretty-printed, which `FORMAT2_BYTES` pins
    // through `settings_v2`.
    let compact = |s: &Settings| Value::Object(s.to_map_as(Format::V2)).to_string();
    for (name, text) in SETTINGS {
        let s = Settings::from_json(text);
        assert_eq!(s.to_json(), compact(&s), "{name}: production does not write the format-2 desk");
    }
    for (name, text) in DOCUMENTS {
        let a = Arrangement::from_json(text);
        assert_eq!(a.to_json(), document_v2(&a), "{name}: production does not write the format-2 desk");
    }
    let default = Settings::default().to_json();
    assert_eq!(default, compact(&Settings::default()), "the default settings are not written as format 2");
    assert_eq!((fnv(&default), default.len()), DEFAULT_V2_BYTES, "the default settings' bytes drifted");
}

/// `observe_settings` without its shuffle line (a format-2 file raises
/// none: its desk is already V32), with the names and the inputs read as
/// the desk reads them: a strip or a channel the array stops short of has
/// no name and no input, which a format-2 row spells as an empty one.
fn settings_picture(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = observe_settings(text).lines().take(7).map(str::to_owned).collect();
    let s = Settings::from_json(text);
    let mut names: Vec<String> = s.rows.iter().map(|r| r.name.clone()).collect();
    names.resize(V32_STRIPS + 1, String::new());
    lines[2] = format!("names {}", json(&names));
    let mut inputs: Vec<String> = s.rows.iter().map(|r| r.input.clone()).collect();
    inputs.resize(V32_CHANNELS, String::new());
    lines[6] = format!("inputs {}", json(&inputs));
    lines
}

fn settings_v2(s: &Settings) -> String {
    serde_json::to_string_pretty(&Value::Object(s.to_map_as(Format::V2))).expect("json")
}

fn document_v2(a: &Arrangement) -> String {
    a.to_json_with_desk_as(a.desk.as_ref(), Format::V2)
}

/// A document's desk as the model would write it, and its curves: the
/// picture two reads of one desk must agree on, whichever shape each read.
fn observe_document_model(text: &str) -> String {
    let a = Arrangement::from_json(text);
    let d = a.desk.as_ref().expect("a desk");
    let curves: Vec<String> = a
        .automation
        .curves()
        .iter()
        .map(|c| format!("{}:{}", c.lane.index_or(crate::desk_lift::V32_MASTER_LANE), c.param.key()))
        .collect();
    format!("{}\ncurves {}", crate::desk_model::picture(&d.desk_model().model), curves.join(" "))
}

/// **Every fixture saves as format 2 and reopens equal** (a fixed point):
/// the settings files and the DAW blobs as v23, the documents as v8, each
/// carrying one `desk` object and no legacy desk key, written again from
/// its own read as the same bytes, and opening to the picture the legacy
/// file opened to. A document carries its era as `legacy_states`.
#[test]
fn every_fixture_saves_as_format_2_and_reopens_equal() {
    for (name, text) in SETTINGS {
        let once = settings_v2(&Settings::from_json(text));
        let v: Value = serde_json::from_str(&once).expect("json");
        assert_eq!(v["settings_version"], 23, "{name} was not stamped 23");
        assert!(crate::desk_format::is_format2(&v["desk"]), "{name} has no format-2 desk");
        for key in ["channel_kinds", "strip_inserts", "strip_names", "routes", "tracks_added"] {
            assert!(v.get(key).is_none(), "{name} still writes the legacy key {key}");
        }
        let twice = settings_v2(&Settings::from_json(&once));
        assert_eq!(once, twice, "{name} is not a fixed point as v23");
        assert_eq!(settings_picture(&once), settings_picture(text), "{name} opened differently as v23");
    }
    for (name, text) in DOCUMENTS {
        let legacy = Arrangement::from_json(text);
        let once = document_v2(&legacy);
        let v: Value = serde_json::from_str(&once).expect("json");
        assert_eq!(v["arrangement_version"], 8, "{name} was not stamped 8");
        assert!(crate::desk_format::is_format2(&v["desk"]), "{name} has no format-2 desk");
        let era = crate::desk_lift::era_of_desk(&serde_json::from_str::<Value>(text).expect("json")["desk"]);
        assert_eq!(v["desk"]["legacy_states"], era.name(), "{name} lost its legacy marker");
        for key in ["kinds", "inserts", "names", "muted", "insert_bypass"] {
            assert!(v["desk"].get(key).is_none(), "{name} still writes the legacy key {key}");
        }
        let back = Arrangement::from_json(&once);
        assert_eq!(back.desk.as_ref().and_then(|d| d.legacy_states.0), Some(era), "{name}");
        assert_eq!(document_v2(&back), once, "{name} is not a fixed point as v8");
        assert_eq!(observe_document_model(&once), observe_document_model(text), "{name} opened differently as v8");
    }
}

/// **The master's curves are written by name** in format 2 and read back
/// as the master, whatever lane the era kept them at: a 27-sends document's
/// master curve at lane 24 reads as the master (under the lift that knows
/// the window, which switches on with the copy-only shelf), is written as
/// `{"target": "master"}`, and reopens on the master's lane.
#[test]
fn a_master_curve_is_persisted_as_the_master() {
    crate::desk_lift::with_v24_lift(|| {
        let (_, text) = DOCUMENTS.iter().find(|(n, _)| *n == "doc-v6-27.json").expect("fixture");
        let legacy = Arrangement::from_json(text);
        let master = crate::automation::AutoLane::Master;
        let on_master = |a: &Arrangement| a.automation.curves().iter().filter(|c| c.lane == master).count();
        assert!(on_master(&legacy) > 0, "the fixture's lane-24 curve did not read as the master");
        let once = document_v2(&legacy);
        let v: Value = serde_json::from_str(&once).expect("json");
        let curves = v["automation"].as_array().expect("curves");
        let named = curves.iter().filter(|c| c["target"] == "master").count();
        assert_eq!(named, on_master(&legacy), "the master's curves were not written by name");
        assert!(
            curves.iter().all(|c| c.get("lane") != Some(&Value::from(crate::desk_lift::V32_MASTER_LANE as u64))),
            "a master curve was written as a lane"
        );
        assert_eq!(on_master(&Arrangement::from_json(&once)), on_master(&legacy));
    });
}

/// A temp folder for one test, empty.
fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tangent-w3-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// **The first format-2 write over a legacy document keeps it**, byte for
/// byte, as `<doc>.pre-v8`, written once: a second format-2 write, and a
/// format-2 write over a legacy file that already has a backup, leave the
/// backup alone; a legacy write makes none.
#[test]
fn the_first_format_2_save_keeps_the_legacy_document_once() {
    for (name, text) in DOCUMENTS {
        let dir = scratch("pre-v8");
        let doc = dir.join(crate::arrangement::DOC_NAME);
        std::fs::write(&doc, text).expect("fixture");
        let backup = Arrangement::pre_v8_path(&doc);
        let mut a = Arrangement::load_from(&doc);
        a.save_to_as(&doc, Format::Legacy);
        assert!(!backup.exists(), "{name}: a legacy write made a backup");
        std::fs::write(&doc, text).expect("fixture");
        a.save_to_as(&doc, Format::V2);
        assert_eq!(std::fs::read_to_string(&backup).expect("backup"), text, "{name}: not the original");
        a.named = !a.named;
        a.save_to_as(&doc, Format::V2);
        assert_eq!(std::fs::read_to_string(&backup).expect("backup"), text, "{name}: rewritten");
        // A legacy file again, with a backup already standing: the first
        // backup is the one kept.
        std::fs::write(&doc, "{\"arrangement_version\": 7, \"clips\": []}").expect("legacy");
        a.save_to_as(&doc, Format::V2);
        assert_eq!(std::fs::read_to_string(&backup).expect("backup"), text, "{name}: overwritten");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The five dangling entries of design 6.3 (a plugin, a bypass, a route, a
/// key and a send, each naming a track the desk does not hold), put into a
/// format-2 `desk` object.
fn dangle(desk: &mut Value) {
    let ghost = "t4000";
    desk["plugins"]
        .as_object_mut()
        .expect("plugins")
        .insert(format!("{ghost}-c0-b0"), Value::from("/ghost.vst3"));
    desk["bypass"].as_array_mut().expect("bypass").push(Value::from(format!("{ghost}-c0-b1")));
    desk["routes"]
        .as_array_mut()
        .expect("routes")
        .push(serde_json::json!({"from": "t1", "to": ghost, "gain": 1.0}));
    desk["keys"]
        .as_array_mut()
        .expect("keys")
        .push(serde_json::json!({"seat": format!("{ghost}-c0-b0"), "from": "t1"}));
    desk["rows"][0]["sends"]
        .as_array_mut()
        .expect("sends")
        .push(serde_json::json!({"to": ghost, "level": 0.5, "pre": false}));
}

/// **A dangling entry costs that entry**: a v23 settings file, a DAW blob
/// and a v8 document each carrying one dangling plugin, bypass, route, key
/// and send open with exactly those five dropped and said, and write
/// everything else back byte for byte.
#[test]
fn dangling_entries_are_dropped_and_said_and_nothing_else_moves() {
    for (name, text) in SETTINGS {
        let clean = settings_v2(&Settings::from_json(text));
        let mut v: Value = serde_json::from_str(&clean).expect("json");
        dangle(&mut v["desk"]);
        let s = Settings::from_json(&v.to_string());
        let notice = s.desk_notice.clone().expect("no notice was raised");
        for what in ["plugin", "bypass", "route", "key", "send"] {
            assert!(notice.contains(what), "{name}: the notice does not name the {what}: {notice}");
        }
        assert!(s.desk_refused.is_none() && s.unreadable.is_none(), "{name}: more than the entries went");
        assert_eq!(settings_v2(&s), clean, "{name}: more than the five entries moved");
    }
    for (name, text) in DOCUMENTS {
        let clean = document_v2(&Arrangement::from_json(text));
        let mut v: Value = serde_json::from_str(&clean).expect("json");
        dangle(&mut v["desk"]);
        let a = Arrangement::from_json(&v.to_string());
        let dropped = &a.desk.as_ref().expect("the desk read").dropped.0;
        let mut kinds: Vec<&str> = dropped.iter().map(|d| d.what).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, ["bypass", "key", "plugin", "route", "send"], "{name}");
        assert_eq!(document_v2(&a), clean, "{name}: more than the five entries moved");
    }
}

/// A format-2 desk the reader must refuse whole: two rows with one id.
fn refusable(desk: &mut Value) {
    let first = desk["rows"][0]["id"].clone();
    desk["rows"][1]["id"] = first;
}

/// **A refused desk refuses the document's OPEN and touches nothing**: the
/// strict read says why, and a lenient read and its save keep the raw desk
/// as the file had it. (The settings file's half, a side file and every
/// other setting kept, is `a_refused_settings_desk_keeps_every_other_setting`
/// in settings.rs.)
#[test]
fn a_refused_desk_refuses_the_open_and_touches_nothing() {
    let (_, text) = DOCUMENTS[5];
    let mut v: Value = serde_json::from_str(&document_v2(&Arrangement::from_json(text))).expect("json");
    refusable(&mut v["desk"]);
    let dir = scratch("refused-doc");
    let doc = dir.join(crate::arrangement::DOC_NAME);
    let bytes = serde_json::to_string_pretty(&v).expect("json");
    std::fs::write(&doc, &bytes).expect("doc");
    assert!(matches!(
        Arrangement::read_from(&doc),
        crate::arrangement::LoadOutcome::Refused(why) if why.contains("two rows")
    ));
    // A lenient read keeps the raw desk, so even a save cannot lose it.
    let lenient = Arrangement::load_from(&doc);
    assert!(lenient.desk.is_none());
    lenient.save_to_as(&doc, Format::V2);
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&doc).expect("doc")).expect("json");
    assert_eq!(after["desk"], v["desk"], "a lenient save wrote the refused desk away");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **What the format-2 writers write for every era fixture**, pinned: the
/// hash and the length of the v23 settings file or DAW blob and of the v8
/// document this build makes of each, so the format-2 shape cannot drift
/// under a writer that stays its own fixed point. Pinned as hashes rather
/// than committed files: each is some seventy kilobytes, nearly all of it
/// the Keys rack's knobs, and `write_format_2_fixtures` writes them out for
/// reading. Re-pin only with a deliberate change to the format.
///
/// Re-pinned with `DEFAULT_V2_BYTES` on 2026-10-17 for the keyboard's
/// ninth and tenth sounds and its re-voiced defaults (the clav's Pickup
/// 0.35 to 0.6, the reeds' Width, Tremolo and Growl): every file differs
/// from its earlier pin in its rows' `keys.params` alone, at those knobs
/// and in the appended sounds' shipped defaults, checked structurally
/// against the earlier writer's output (`default-settings.json` is dumped
/// beside them for the same check).
///
/// Moved again for 5.3, every settings file and DAW blob by one line alone,
/// `"welcome_seen": "0"` (`Settings::welcome_seen`: a file from before the
/// key reads as older than any version): with that line taken out, each
/// hashes to its earlier pin. The documents carry no settings and did not
/// move.
const FORMAT2_BYTES: [(&str, u64, usize); 17] = [
    ("settings-v12.json", 0xa470_7f4f_e31c_e6d0, 77183),
    ("settings-v17-pads16.json", 0xc551_8ae6_e64e_12e0, 77399),
    ("settings-v18.json", 0xa405_20a6_61a9_2567, 77400),
    ("settings-v19.json", 0xbac8_06a1_730a_980d, 78700),
    // Lifted (`desk_lift::V24_LIFT`, on with the switch since W4-integrate
    // (B)): the window's buses and master on the specials, its pads on
    // their rows. Unlifted it wrote 0xbee3_aa56_2a6b_446b, 72152.
    ("settings-v20-27.json", 0xb498_f5bf_7031_5ec9, 79928),
    ("settings-v20.json", 0x0cd4_0366_5e9b_7b3f, 81045),
    ("settings-v21.json", 0x32d3_850d_c563_2682, 79852),
    ("settings-v22.json", 0x56e2_8870_5674_407a, 79887),
    ("daw-3x.json", 0x7354_c8db_4e34_21d8, 76689),
    ("daw-4x.json", 0x4f61_170f_aff8_0569, 77088),
    ("daw-5x.json", 0x98be_ad91_7144_8817, 78025),
    // **The documents read their drum row's badge** (W6): a document from
    // before the badges derives them as it is read, the drum where the
    // drumpad stands, where the derivation ran at the apply while the desk
    // had arrays to leave empty. Each of the six below differs from its
    // earlier pin by that one `"icon": 1` and nothing else (checked by
    // reverting it, which gives the earlier pin back byte for byte).
    ("doc-v4-11.json", 0x0659_5de6_fedb_123d, 74148),
    ("doc-v5-18.json", 0x7344_d5a8_dd20_bfda, 75110),
    ("doc-v5-19.json", 0x5935_245b_4a23_9807, 75107),
    // Lifted, as `settings-v20-27.json`. Unlifted: 0xba6f_3803_9396_0a03, 68492.
    ("doc-v6-27.json", 0xc97f_dce6_7d2f_80cd, 76256),
    ("doc-v6-35.json", 0xb2c0_241b_88d7_aa45, 76843),
    ("doc-v7.json", 0xe7ba_8593_1aad_eaef, 77067),
];

/// The format-2 bytes this build writes for every legacy fixture, by name.
fn written_v2() -> impl Iterator<Item = (&'static str, String)> {
    SETTINGS
        .iter()
        .map(|(n, t)| (*n, settings_v2(&Settings::from_json(t))))
        .chain(DOCUMENTS.iter().map(|(n, t)| (*n, document_v2(&Arrangement::from_json(t)))))
}

/// **The v23 and v8 expectations**: every era fixture's format-2 bytes are
/// the pinned ones.
#[test]
fn every_fixture_writes_its_pinned_format_2_bytes() {
    for ((name, text), (want_name, hash, len)) in written_v2().zip(FORMAT2_BYTES) {
        assert_eq!(name, want_name, "the tables are out of step");
        assert_eq!((fnv(&text), text.len()), (hash, len), "{name}: the format-2 bytes drifted");
    }
}

/// Writes every fixture's format-2 file into `$TANGENT_ERA_V2_DUMP`, for
/// reading, or for re-pinning after a deliberate change to the format.
#[test]
#[ignore = "writes files; run by hand with TANGENT_ERA_V2_DUMP set"]
fn write_format_2_fixtures() {
    let Some(dir) = std::env::var_os("TANGENT_ERA_V2_DUMP") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("dump dir");
    for (name, text) in written_v2() {
        std::fs::write(dir.join(name), text).expect("write");
    }
    // And what a fresh install writes (`DEFAULT_V2_BYTES`), compact as
    // production writes it.
    std::fs::write(dir.join("default-settings.json"), Settings::default().to_json()).expect("write");
}
