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
//! The DAW blobs ride the same road (`Settings::from_json` is the plugin's
//! `#[persist = "settings"]` reader), here rather than in the plugin crate,
//! whose tests the gate does not run.
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
    [
        format!("colors {}", json(&s.strip_colors[..=V32_STRIPS])),
        format!("sends {}", json(&s.strip_sends[..V32_STRIPS])),
        format!("names {}", json(&s.strip_names[..s.strip_names.len().min(V32_STRIPS + 1)])),
        format!(
            "inserts {}",
            occupied(
                s.strip_inserts
                    .iter()
                    .take(V32_INSERT_SPACE)
                    .enumerate()
                    .map(|(b, p)| (b, p.as_str()))
            )
        ),
        format!("muted {:#x} soloed {:#x} phase {:#x}", s.strip_muted, s.strip_soloed, s.strip_phase),
        format!("kinds {}", json(&s.channel_kinds[..V32_CHANNELS])),
        format!("inputs {}", json(&s.channel_inputs)),
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
        .map(|c| format!("{}:{}", c.lane, c.param.key()))
        .collect();
    [
        format!("colors {}", json(&d.colors)),
        format!("sends {}", json(&d.sends)),
        format!("names {}", json(&d.names)),
        format!("inserts {}", occupied(d.inserts.iter().map(|(b, p)| (*b, p.as_str())))),
        format!("muted {:#x} soloed {:#x} phase {:?}", d.muted, d.soloed, d.phase),
        format!("tracks_added {:#x} armed {:#x}", d.tracks_added, d.armed),
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
inputs []
shuffles v18 v19 v20"#),
    ("settings-v18.json", r#"colors [100,101,102,103,104,105,106,107,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,108,109,110,111]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.09,0.1,0.11]
names ["s0","s1","s2","s3","s4","s5","s6","s7","","","","","","","","","","","","","","","","","","","","","","","","","s8","s9","s10","s11"]
inserts 0=/x/s0.vst3 3=/x/s1.vst3 6=/x/s2.vst3 9=/x/s3.vst3 12=@tangent-drums 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 96=/x/s8.vst3 99=/x/s9.vst3 102=/x/s10.vst3 105=/x/s11.vst3 1647=/x/pad0.vst3 1768=/x/pad40.vst3 2030=/x/pad127.vst3
muted 0x100000004 soloed 0x400000000 phase 0x0
kinds [1,1,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs []
shuffles v19 v20"#),
    ("settings-v19.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,116,0,118,0]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.17,0.18,0.19]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","","","","","","","","","","","","","","","","","s16","s17","s18","s19"]
inserts 0=@tangent-drums 3=@tangent-dx7 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 24=/x/s8.vst3 27=/x/s9.vst3 30=/x/s10.vst3 33=/x/s11.vst3 36=/x/s12.vst3 39=/x/s13.vst3 42=/x/s14.vst3 45=/x/s15.vst3 96=/x/s16.vst3 99=/x/s17.vst3 102=/x/s18.vst3 105=/x/s19.vst3 107=/x/s19b2.vst3 108=/x/pad0.vst3 229=/x/pad40.vst3 491=/x/pad127.vst3 493=/x/master-extra.vst3 1278=/x/c3pad5.vst3
muted 0x200000004 soloed 0x400000020 phase 0x900000000
kinds [1,1,2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs []
shuffles v20"#),
    ("settings-v20-27.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,0,0,0,0,0,0,0,0,124,125,126,127]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.25,0.26,0.27]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","","","","","","","","","s24","s25","s26","s27"]
inserts 0=@tangent-drums 3=@tangent-dx7 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 24=/x/s8.vst3 27=/x/s9.vst3 30=/x/s10.vst3 33=/x/s11.vst3 36=/x/s12.vst3 39=/x/s13.vst3 42=/x/s14.vst3 45=/x/s15.vst3 48=/x/s16.vst3 51=/x/s17.vst3 54=/x/s18.vst3 57=/x/s19.vst3 60=/x/s20.vst3 63=/x/s21.vst3 66=/x/s22.vst3 69=/x/s23.vst3 96=/x/s24.vst3 99=/x/s25.vst3 102=/x/s26.vst3 105=/x/s27.vst3 107=/x/s27b2.vst3 108=/x/pad0.vst3 229=/x/pad40.vst3 491=/x/pad127.vst3 493=/x/master-extra.vst3 1278=/x/c3pad5.vst3 9326=/x/c23pad127.vst3
muted 0x200000004 soloed 0x400200000 phase 0x900000008
kinds [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,1,0,0,0,0,0,0,0,0]
inputs []
shuffles v24"#),
    ("settings-v20.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 6=/x/s2.vst3 9=/x/s3.vst3 12=/x/s4.vst3 15=/x/s5.vst3 18=/x/s6.vst3 21=/x/s7.vst3 24=/x/s8.vst3 27=/x/s9.vst3 30=/x/s10.vst3 33=/x/s11.vst3 36=/x/s12.vst3 39=/x/s13.vst3 42=/x/s14.vst3 45=/x/s15.vst3 48=/x/s16.vst3 51=/x/s17.vst3 54=/x/s18.vst3 57=/x/s19.vst3 60=/x/s20.vst3 63=/x/s21.vst3 66=/x/s22.vst3 69=/x/s23.vst3 72=/x/s24.vst3 75=/x/s25.vst3 78=/x/s26.vst3 81=/x/s27.vst3 84=/x/s28.vst3 87=/x/s29.vst3 90=/x/s30.vst3 93=/x/s31.vst3 96=/x/s32.vst3 99=/x/s33.vst3 102=/x/s34.vst3 105=/x/s35.vst3 229=/x/pad40.vst3 493=/x/master-extra.vst3 12396=/x/c31pad127.vst3 12416=/x/folder5-extra.vst3 12577=/x/c3pedal6.vst3
muted 0x200000004 soloed 0x400000000 phase 0x900000000
kinds [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2]
inputs []
shuffles "#),
    ("settings-v21.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 107=/x/s35b2.vst3 229=/x/pad40.vst3 12416=/x/folder5-extra.vst3
muted 0x200000000 soloed 0x0 phase 0x800000000
kinds [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs []
shuffles "#),
    ("settings-v22.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.25,0.26,0.27,0.28,0.29,0.3,0.31,0.32,0.33,0.34,0.35]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","s24","s25","s26","s27","s28","s29","s30","s31","s32","s33","s34","s35"]
inserts 0=@tangent-drums 3=@tangent-dx7 96=/x/drumbus.vst3 107=/x/s35b2.vst3 12416=/x/folder5-extra.vst3
muted 0x4 soloed 0x100000000 phase 0x200000000
kinds [1,1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
inputs ["midi:all","midi:all","midi:Keystation 88"]
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
inputs []
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
    ("doc-v6-27.json", r#"colors [100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,0,0,0,0,0,0,0,0,124,125,126,127]
sends [0.01,0.02,0.03,0.04,0.05,0.06,0.07,0.08,0.09,0.1,0.11,0.12,0.13,0.14,0.15,0.16,0.17,0.18,0.19,0.2,0.21,0.22,0.23,0.24,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.25,0.26,0.27]
names ["s0","s1","s2","s3","s4","s5","s6","s7","s8","s9","s10","s11","s12","s13","s14","s15","s16","s17","s18","s19","s20","s21","s22","s23","","","","","","","","","s24","s25","s26","s27"]
inserts 0=@tangent-drums 3=@tangent-dx7 69=/x/c23.vst3 96=/x/drumbus.vst3 103=/x/fx.vst3 107=/x/master.vst3 489=/x/pad127.vst3 493=/x/master-extra.vst3 1280=/x/c3pad5.vst3
muted 0x200800000 soloed 0x400000000 phase None
tracks_added 0x800003 armed 0x2
curves 23:gain 32:gain 32:pan"#),
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

/// **Every settings file and DAW blob opens to its literal picture.**
#[test]
fn every_settings_fixture_opens_to_its_literal_picture() {
    for ((name, text), (want_name, want)) in SETTINGS.iter().zip(SETTINGS_WANT) {
        assert_eq!(*name, want_name, "the tables are out of step");
        assert_eq!(observe_settings(text), want, "{name} opened differently");
    }
}

/// **Every document opens to its literal picture**, master curves included.
#[test]
fn every_document_fixture_opens_to_its_literal_picture() {
    for ((name, text), (want_name, want)) in DOCUMENTS.iter().zip(DOCUMENTS_WANT) {
        assert_eq!(*name, want_name, "the tables are out of step");
        assert_eq!(observe_document(text), want, "{name} opened differently");
    }
}

/// **Opened once, every fixture is a fixed point**: written by this build
/// and read back, it writes the same bytes and opens to the same picture.
/// The DAW blobs are the plugin's whole persistence, so a blob that drifted
/// on each save would drift in every project that holds it.
#[test]
fn every_fixture_round_trips_once_opened() {
    for (name, text) in SETTINGS {
        let once = Settings::from_json(text).to_json();
        let twice = Settings::from_json(&once).to_json();
        assert_eq!(once, twice, "{name} is not a fixed point");
        assert_eq!(observe_settings(&once).lines().take(7).collect::<Vec<_>>(),
            observe_settings(text).lines().take(7).collect::<Vec<_>>(),
            "{name} changed through a save");
    }
    for (name, text) in DOCUMENTS {
        let once = Arrangement::from_json(text).to_json();
        let twice = Arrangement::from_json(&once).to_json();
        assert_eq!(once, twice, "{name} is not a fixed point");
        assert_eq!(observe_document(&once), observe_document(text), "{name} changed through a save");
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
/// The 24-channel window (d298cee..af9bda6) had no arm, so its drum bus,
/// click, fx return and master opened on channels 24..27 and its master's
/// automation on channel 24. Red at the base.
#[test]
fn a_27_sends_file_lifts_its_buses_master_and_master_curves() {
    let (_, text) = fixture!("settings-v20-27.json");
    let s = Settings::from_json(text);
    // The fixture paints strip i with 100 + i: drums 24, click 25, fx 26,
    // master 27 in the window; 32, 33, 34, 35 in V32.
    assert_eq!(&s.strip_colors[32..36], &[124, 125, 126, 127], "the buses and master");
    assert_eq!(&s.strip_colors[24..32], &[0; 8], "the widened channels came up painted");
    assert_eq!(s.strip_names[35], "s27", "the master's name");
    assert_eq!(s.strip_inserts[32 * 3], "/x/s24.vst3", "the drum bus's bay");
    assert_eq!(s.strip_inserts[35 * 3 + 2], "/x/s27b2.vst3", "the master's bay");
    assert_eq!(s.strip_inserts[164 * 3 + 1], "/x/master-extra.vst3", "the master's extra rack");
    assert_eq!(s.strip_inserts[(165 + 2 * 128 + 5) * 3], "/x/c3pad5.vst3", "channel 3's pad 5");
    assert_eq!(s.strip_muted, (1 << 33) | (1 << 2), "the muted click");
    assert_eq!(s.strip_soloed, (1 << 34) | (1 << 21), "the soloed fx");
    assert_eq!(s.strip_phase, (1 << 32) | (1 << 35) | (1 << 3), "the inverted drum bus and master");

    let (_, text) = fixture!("doc-v6-27.json");
    let a = Arrangement::from_json(text);
    let d = a.desk.as_ref().expect("a desk");
    assert_eq!(&d.colors[32..36], &[124, 125, 126, 127], "the document's buses and master");
    let seat = |bay: usize| d.inserts.iter().find(|(b, _)| *b == bay).map(|(_, p)| p.as_str());
    assert_eq!(seat(32 * 3), Some("/x/drumbus.vst3"));
    assert_eq!(seat(35 * 3 + 2), Some("/x/master.vst3"));
    assert_eq!(d.muted, (1 << 33) | (1 << 23), "the document's muted click");
    let lanes: Vec<(usize, String)> =
        a.automation.curves().iter().map(|c| (c.lane, c.param.key())).collect();
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
    let (_, text) = fixture!("settings-v19.json");
    let s = Settings::from_json(text);
    assert_eq!(s.strip_phase, (1 << 32) | (1 << 35), "phase did not follow the drum bus and master");
    // And it is the same lift that moved mute and solo.
    assert_eq!(s.strip_muted, (1 << 33) | (1 << 2));
    assert_eq!(s.strip_soloed, (1 << 34) | (1 << 5));
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
    for (name, text) in DOCUMENTS {
        let raw: Value = serde_json::from_str(text).expect("fixture");
        let p = provenance_of_document(&raw).expect("a legacy desk has a provenance");
        let doc = tagged(text);
        let read = Arrangement::from_json(&doc.to_string());
        let desk = read.desk.expect("a desk");
        let dense = doc["desk"]["inserts"].as_array().map_or(0, Vec::len);
        let tail = doc["desk"]["inserts_tail"].as_object().map_or(0, |t| t.len());
        assert_eq!(desk.inserts.len(), dense + tail, "{name}: a bay was lost or made up");
        for (bay, path) in &desk.inserts {
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
        [DeskEra::V8Pads16, DeskEra::V16NoDrumBus, DeskEra::V16, DeskEra::V24, DeskEra::V32, DeskEra::V32]
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
    // The transcription is the host's code: its loop headers, verbatim.
    let host = include_str!("../../tangent/src/desktop.rs");
    for line in [
        "let _ = std::fs::remove_file(insert_state_path(8, bay));",
        "for old in 9..=11 {",
        "for old in (8..=10).rev() {",
        "for pad in (0..16usize).rev() {",
        "let _ = std::fs::rename(&from, insert_state_path(12 + 36 + pad, bay));",
        "for old in (8..(12 + 128)).rev() {",
        "let _ = std::fs::rename(&from, insert_state_path(old + 8, bay));",
        "for old in (16..2069).rev() {",
        "let grow = 16;",
    ] {
        assert!(host.contains(line), "the host's shuffles changed: `{line}` is gone");
    }
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
    assert_eq!(names("settings-v20-27.json"), ["v24"]);
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

/// The lines of `src` outside its top-level test modules (`#[cfg(test)]` or
/// `#[cfg(all(test, ..))]` on a `mod` whose body closes at column 0), with
/// their line numbers.
fn outside_test_modules(src: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let opens = l.starts_with("#[cfg(")
            && l.contains("test")
            && lines.get(i + 1).is_some_and(|m| {
                (m.starts_with("mod ") || m.starts_with("pub(crate) mod ")) && m.trim_end().ends_with('{')
            });
        if opens {
            i += 2;
            while i < lines.len() && lines[i] != "}" {
                i += 1;
            }
            i += 1;
            continue;
        }
        out.push((i, l));
        i += 1;
    }
    out
}

/// **No code but the two choosers spells the real config directory or the
/// real media root**, so STAGING cannot be routed around by a second copy
/// of either path. `tangent_core::config::dir` is the config's only home
/// and `Settings::default_media_root` the media's; a test fixture that names
/// a folder of that name under a temp root is not a path to the owner's
/// files, so tests are read past their `#[cfg(test)]`.
#[test]
fn nothing_but_the_choosers_spells_the_real_config_or_media_path() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
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
                for (n, line) in outside_test_modules(&src) {
                    let line = line.split("//").next().unwrap_or("");
                    for spelled in ["\"Tangent Media", "\".config\"", "\".config/", "join(\"tangent\")", "\"tangent-staging\""] {
                        if !line.contains(spelled) {
                            continue;
                        }
                        let allowed = match spelled {
                            // The chooser, and the per-thread test root.
                            "\"Tangent Media" => {
                                rel == "tangent-ui/src/settings.rs"
                                    && (line.contains("Tangent Media Staging") || line.contains("test_path()"))
                            }
                            "\".config\"" => {
                                rel == "tangent-core/src/config.rs"
                                // The XDG user-dirs file, not Tangent's config.
                                || rel == "tangent-record/src/take.rs"
                            }
                            "\"tangent-staging\"" => rel == "tangent-core/src/config.rs",
                            _ => false,
                        };
                        if !allowed {
                            found.push(format!("{rel}:{}: {}", n + 1, line.trim()));
                        }
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "a second spelling of a real path: {found:#?}");
}
