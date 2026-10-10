//! **The save roads' own tests** (W5-save-gaps): what a save writes is what
//! the session shows, and no kill or Don't Save loses a kit. A child of
//! `app`, in a file of its own, so it reads the app's private roads the way
//! its `tests` module does.

use super::tests::{headless, headless_with, project_scratch};
use super::*;

/// **The function `at` falls in**: the nearest `fn` above it, by name.
fn enclosing_fn(src: &str, at: usize) -> &str {
    src[..at]
        .lines()
        .rev()
        .find_map(|line| {
            let t = line.trim_start();
            let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
            let name = t.strip_prefix("fn ")?;
            Some(&name[..name.find(['(', '<'])?])
        })
        .unwrap_or("")
}

/// **`src` with every `#[cfg(test)]` item blanked out**: the production
/// source. An item ends at its first line's `;`, or at the line that closes
/// it at its own indent, which is how rustfmt lays out every item here.
fn production(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut skipping: Option<String> = None;
    let mut armed: Option<String> = None;
    for line in src.lines() {
        if let Some(close) = skipping.as_deref() {
            if line == close {
                skipping = None;
            }
            out.push('\n');
            continue;
        }
        if let Some(indent) = armed.take() {
            let t = line.trim_start();
            if t.starts_with("#[") || t.starts_with("///") || t.starts_with("//") {
                armed = Some(indent);
            } else if !t.ends_with(';') && !t.ends_with(',') {
                // A field or an initialiser ends at its comma; anything else
                // with a body ends at the brace that closes it.
                skipping = Some(format!("{indent}}}"));
            }
            out.push('\n');
            continue;
        }
        if line.trim_start() == "#[cfg(test)]" {
            armed = Some(line[..line.len() - line.trim_start().len()].to_owned());
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// **The roads that write a document that is not the session's**, each
/// with why it may write without the session's live desk. Anything else
/// that writes a document goes through `write_session_doc`.
const NOT_THE_SESSION: [(&str, &str); 4] = [
    ("bootstrap_project", "a first document for a test's app, born with no desk of its own"),
    ("new_project", "a fresh empty document, opened as the next session"),
    ("seed_template", "the shipped desk, never the session's (`template_desk`)"),
    (
        "write_template_into",
        "the session as a stencil: a clone whose desk is taken live (`desk_snapshot`) in the same function",
    ),
];

/// What `a_save_writes_the_desk_the_session_shows` finds wrong with `src`.
fn stale_desk_roads(src: &str) -> Vec<String> {
    let prod = production(src);
    let mut problems = Vec::new();
    let mut writes: Vec<usize> = prod.match_indices(".save_to(").map(|(at, _)| at).collect();
    writes.extend(prod.match_indices(".save_to_as(").map(|(at, _)| at));
    for at in writes {
        let road = enclosing_fn(&prod, at);
        let call = prod[prod[..at].rfind('\n').map_or(0, |n| n + 1)..prod[at..].find('\n').map_or(prod.len(), |n| at + n)].trim();
        if road == "write_session_doc" {
            continue;
        }
        if !NOT_THE_SESSION.iter().any(|(r, _)| *r == road) {
            problems.push(format!(
                "`{road}` writes a document (`{call}`) without the session's live desk: route it \
                 through `write_session_doc`, or name it in NOT_THE_SESSION with why"
            ));
        }
    }
    let body = |name: &str| -> &str {
        let head = format!("fn {name}(");
        let Some(start) = prod.find(&head) else { return "" };
        &prod[start..start + prod[start..].find("\n    }\n").unwrap_or(prod.len() - start)]
    };
    let writer = body("write_session_doc");
    let fresh = writer.find("self.arrangement.desk = Some(self.desk_snapshot());");
    let write = writer.find("self.arrangement.save_to(doc);");
    if !matches!((fresh, write), (Some(f), Some(w)) if f < w) {
        problems.push("`write_session_doc` writes before it takes the live desk".to_owned());
    }
    for road in ["save_project_doc", "save_project_as"] {
        if !body(road).contains("self.write_session_doc(&doc);") {
            problems.push(format!("`{road}` does not write through `write_session_doc`"));
        }
    }
    let stencil = body("write_template_into");
    let live = stencil.find("arr.desk = Some(self.desk_snapshot());");
    let saved = stencil.find("arr.save_to(&doc);");
    if !matches!((live, saved), (Some(l), Some(s)) if l < s) {
        problems.push("`write_template_into` writes its stencil before taking the live desk".to_owned());
    }
    problems
}

/// **No save road writes a desk older than the one on screen** (W5-save-gaps
/// 1, the road list): every document write in this file is the session's
/// one writer (`write_session_doc`, which takes the live desk first), or a
/// road named with why it writes some other document. Save and Save As go
/// through the one writer. A road written later that calls `save_to` on its
/// own fails here by name, and so do the two plantings of this test's
/// revert check: Save As writing the document it holds, and the writer
/// skipping the refresh.
#[test]
fn a_save_writes_the_desk_the_session_shows() {
    let src = include_str!("app.rs");
    let problems = stale_desk_roads(src);
    assert!(problems.is_empty(), "the save roads:\n  {}", problems.join("\n  "));
    let stale_copy = src.replacen(
        "        self.write_session_doc(&doc);\n        self.project_doc = doc.clone();",
        "        self.arrangement.save_to(&doc);\n        self.project_doc = doc.clone();",
        1,
    );
    assert_ne!(stale_copy, src, "fixture: Save As's write is not where the planting looks");
    let found = stale_desk_roads(&stale_copy);
    assert!(
        found.iter().any(|p| p.contains("`save_project_as` writes a document")),
        "a Save As writing its stale desk passed the scan: {found:?}"
    );
    let unrefreshed = src.replacen("        self.arrangement.desk = Some(self.desk_snapshot());\n        self.arrangement.save_to(doc);", "        self.arrangement.save_to(doc);", 1);
    assert_ne!(unrefreshed, src, "fixture: the writer is not where the planting looks");
    assert!(
        stale_desk_roads(&unrefreshed).iter().any(|p| p.contains("before it takes the live desk")),
        "a writer that skips the refresh passed the scan"
    );
}

/// The four ways a row becomes a track that the menu, the chips and the
/// keys reach.
#[derive(Clone, Copy, Debug)]
enum Add {
    Bare,
    Midi,
    Audio,
    Folder,
}

/// The four roads that write the session's document somewhere it reopens
/// from.
#[derive(Clone, Copy, Debug)]
enum Keep {
    Save,
    SaveAs,
    FirstSave,
    Template,
}

/// What a reopened desk must hold of row `n`: its track, kind, count,
/// folder-hood, and its kit's first sample.
fn row_facts(app: &TangentApp, n: usize) -> (crate::desk_id::TrackId, Option<recorder::ChannelKind>, bool, bool, String) {
    (app.track_id_at(n), app.channel_kind(n), app.row_counted(n), app.is_folder(n), app.kit_at(n).pads[36].path.clone())
}

/// **A track added and then kept is in the document that was written, with
/// its kit** (W5-save-gaps 1): each way a row becomes a track (a bare add, a
/// MIDI or an audio kind given to an empty row, an empty slot made a folder)
/// and each road that writes the session (Save, Save As, the first save,
/// Save As Template), the written document reopened in another app. Save As
/// wrote the document's copy of the desk as it stood, and a bare add or a
/// kind given to a row does not refresh it: the copy reopened without the
/// track, while its kit sidecar, written from the live rows, filed the kit
/// under an id the copy did not hold.
#[test]
fn a_track_added_then_kept_reopens_with_its_kit_on_every_save_road() {
    for keep in [Keep::Save, Keep::SaveAs, Keep::FirstSave, Keep::Template] {
        for add in [Add::Bare, Add::Midi, Add::Audio, Add::Folder] {
            let tag = format!("{keep:?} after a {add:?} add");
            let root = project_scratch(&format!("save-gaps-desk-{keep:?}-{add:?}"));
            let (_ctx, mut app) = headless(Caps::DESKTOP);
            app.bootstrap_project(&root.join("Projects"));
            if !matches!(keep, Keep::FirstSave) {
                // A kept project to save in place, or to copy.
                assert!(app.save_project_first(&root.join("Projects").join("Song.tan")), "{tag}: fixture: no first save");
            }
            let row = 5;
            assert!(!app.row_counted(row), "{tag}: fixture: row {row} is not empty");
            match add {
                Add::Bare => assert!(app.add_bare_track(row), "{tag}: fixture: no track was added"),
                Add::Midi => app.set_channel_kind(row, Some(recorder::ChannelKind::Midi)),
                Add::Audio => app.set_channel_kind(row, Some(recorder::ChannelKind::Audio)),
                Add::Folder => {
                    app.select_alone(row);
                    assert!(app.make_folder(), "{tag}: fixture: no folder was made");
                }
            }
            assert!(app.row_counted(row), "{tag}: fixture: row {row} is not a track");
            app.kit_at_mut(row).pads[36].path = "media/kick.wav".into();
            app.drumpad_edited();
            let want = row_facts(&app, row);
            let written = match keep {
                Keep::Save => {
                    app.save_project_doc();
                    app.project_doc().expect("the project").to_path_buf()
                }
                Keep::SaveAs => {
                    assert!(app.save_project_as(&root.join("Copy")), "{tag}: the copy was not made");
                    root.join("Copy").join(crate::arrangement::DOC_NAME)
                }
                Keep::FirstSave => {
                    assert!(app.save_project_first(&root.join("Projects").join("First.tan")), "{tag}: no first save");
                    root.join("Projects").join("First").join(crate::arrangement::DOC_NAME)
                }
                Keep::Template => {
                    app.write_template_into(&root.join("Stencil")).expect("the stencil");
                    root.join("Stencil").join(crate::arrangement::DOC_NAME)
                }
            };
            assert!(written.is_file(), "{tag}: nothing was written at {written:?}");
            let (_c, mut back) = headless(Caps::DESKTOP);
            back.open_project(&written);
            assert_eq!(row_facts(&back, row), want, "{tag}: the document written lost the track or its kit");
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}

/// **A drum track's removal leaves its kit on disk until a save writes the
/// document without it** (W5-save-gaps 2). The removal took the kit out of
/// the session and rewrote the sidecar at once, so Don't Save after it
/// reopened the project with the drum track's kit gone while its document
/// still held the track. Now the removal changes the session only:
///
/// - removed and not saved, the project reopens with the kit on its track,
///   also after a pad of another drum track is edited (the sidecar saving
///   itself files kits by the document's rows, the removed track's
///   included);
/// - removed and undone, then saved, the kit is there;
/// - removed and saved, the kit is gone from disk, and the undo still brings
///   it back in the session.
#[test]
fn a_removed_drum_tracks_kit_leaves_the_disk_with_the_save_and_not_before() {
    let root = project_scratch("save-gaps-kit-removal");
    let (_ctx, mut app) = headless(Caps::DESKTOP);
    app.bootstrap_project(&root.join("Projects"));
    assert!(app.save_project_first(&root.join("Projects").join("Drums.tan")), "fixture: no first save");
    let doc = app.project_doc().expect("the project").to_path_buf();
    for n in [3, 4] {
        assert!(app.add_bare_track(n), "fixture: row {n} took no track");
    }
    app.kit_at_mut(3).pads[36].path = "media/kick.wav".into();
    app.kit_at_mut(4).pads[38].path = "media/snare.wav".into();
    app.drumpad_edited();
    app.save_project_doc();
    let drums = app.track_id_at(3);
    let other = app.track_id_at(4);
    let reopened = || {
        let (_c, mut back) = headless(Caps::DESKTOP);
        back.open_project(&doc);
        back
    };
    let kick = |a: &TangentApp| a.row_of(drums).map(|r| a.kit_at(r).pads[36].path.clone());
    let kicked = Some("media/kick.wav".to_owned());

    // Removed, not saved: Don't Save reopens the document on disk.
    app.remove_selected_channels(3);
    assert_eq!(app.row_of(drums), None, "fixture: the drum track was not removed");
    assert_eq!(kick(&reopened()), kicked, "a removal took the kit off the disk before any save");
    // A pad of the other drum track edited after it: the sidecar saves itself.
    app.set_drumpad_lane(app.row_of(other).expect("the other drum track"));
    let row = app.row_of(other).expect("the other drum track");
    app.kit_at_mut(row).pads[40].path = "media/tom.wav".into();
    app.drumpad_edited();
    let back = reopened();
    assert_eq!(kick(&back), kicked, "a pad edit after the removal took the removed track's kit off the disk");
    let back_row = back.row_of(other).expect("the other drum track on disk");
    assert_eq!(back.kit_at(back_row).pads[40].path, "media/tom.wav", "the pad edit itself never reached the disk");

    // Undone (the pad edit, then the removal), then saved: the kit is there.
    app.undo();
    app.undo();
    assert_eq!(app.row_of(drums), Some(3), "fixture: the undo did not bring the drum track back");
    assert_eq!(kick(&app), kicked, "the undo did not bring the kit back in the session");
    app.save_project_doc();
    assert_eq!(kick(&reopened()), kicked, "a removal undone and saved lost the kit");

    // Removed and saved: gone from disk; the undo brings it back in the session.
    app.remove_selected_channels(3);
    app.save_project_doc();
    let back = reopened();
    assert_eq!(back.row_of(drums), None, "fixture: the saved document still holds the drum track");
    assert!(
        (0..recorder::CHANNELS).all(|r| back.kit_at(r).pads[36].path.is_empty()),
        "a saved removal left the kit on a row of the document"
    );
    app.undo();
    assert_eq!(kick(&app), kicked, "the undo of a saved removal did not bring the kit back");
    let _ = std::fs::remove_dir_all(&root);
}

/// **A release build's project, in `dir`**: a legacy document and a kit
/// sidecar in the shape a release build writes (the panel's kit at the top
/// level, every lane's under `lane_kits`, `primary_lane`), with a kit on
/// lane 2. Answers the two files' bytes.
fn a_release_builds_project(dir: &std::path::Path) -> (Vec<u8>, Vec<u8>) {
    use crate::drumpad::DrumpadDoc;
    std::fs::create_dir_all(dir.join("media")).expect("the project");
    let doc = crate::arrangement::doc_in(dir);
    let text = include_str!("../../tests/fixtures/eras/doc-v7.json").as_bytes().to_vec();
    std::fs::write(&doc, &text).expect("the legacy document");
    let mut kit = DrumpadDoc::default();
    kit.pads[36].path = "media/kick.wav".to_owned();
    kit.save_beside(&doc);
    let sidecar = DrumpadDoc::sidecar(&doc);
    let mut legacy: serde_json::Value = serde_json::from_slice(&std::fs::read(&sidecar).expect("the kit")).expect("json");
    let top = legacy.clone();
    legacy["primary_lane"] = 2.into();
    legacy["lane_kits"] = serde_json::json!({ "2": top });
    let kit_bytes = serde_json::to_vec_pretty(&legacy).expect("json");
    std::fs::write(&sidecar, &kit_bytes).expect("the legacy kit");
    (text, kit_bytes)
}

/// **The way back a folder keeps for a release build**: each file's
/// `.pre-v8` when a format-2 write replaced it, the file itself while it is
/// still the release build's (no format-2 write reached it).
fn way_back(dir: &std::path::Path) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let kept = |file: &std::path::Path, format2: &dyn Fn(&serde_json::Value) -> bool| {
        let mut backup = file.as_os_str().to_owned();
        backup.push(crate::arrangement::PRE_V8_SUFFIX);
        std::fs::read(std::path::PathBuf::from(backup)).ok().or_else(|| {
            std::fs::read(file)
                .ok()
                .filter(|b| serde_json::from_slice::<serde_json::Value>(b).ok().is_some_and(|v| !format2(&v)))
        })
    };
    let doc = crate::arrangement::doc_in(dir);
    (
        kept(&doc, &|v| v.get("desk").is_some_and(crate::desk_format::is_format2)),
        kept(&crate::drumpad::DrumpadDoc::sidecar(&doc), &|v| v.get("track_kits").is_some()),
    )
}

/// **Every road that first writes a release build's project in format 2
/// keeps the way back, kit included** (W5-save-gaps 3, design 6.7: "restoring
/// `<doc>.pre-v8` hands a release build its project back whole"): Save,
/// Save As into an existing folder (the release build's project written
/// over), the kit sidecar saving itself after a pad edit, and the template
/// seeding of a stock template. After each, the folder holds the release
/// build's document and kit sidecar byte for byte, as a `.pre-v8` where a
/// format-2 write replaced it, and the two restored into a folder of their
/// own open in a release build's readers with the kit on lane 2 and the
/// panel there. And the launch's reclaim of a scratch never takes a release
/// build's kit that is not the template's (`fresh_project_dir` steps past
/// it), so no road deletes one where no backup was kept. (Save As Template
/// is the host's, and its test is `desktop`'s.)
#[test]
fn every_first_format_2_write_keeps_a_release_builds_kit_to_go_back_to() {
    use crate::drumpad::{DrumpadDoc, FILE_NAME};
    let root = project_scratch("save-gaps-kit-backup");
    let restores = |what: &str, dir: &std::path::Path, want: &(Vec<u8>, Vec<u8>)| {
        let (doc, kit) = way_back(dir);
        assert_eq!(doc.as_deref(), Some(want.0.as_slice()), "{what}: the release build's document is not kept");
        assert_eq!(kit.as_deref(), Some(want.1.as_slice()), "{what}: the release build's kit sidecar is not kept");
        let back = root.join(format!("restored {what}"));
        std::fs::create_dir_all(&back).expect("a restore");
        std::fs::write(back.join(crate::arrangement::DOC_NAME), &want.0).expect("restored document");
        std::fs::write(back.join(FILE_NAME), &want.1).expect("restored kit");
        let (kits, panel) = DrumpadDoc::load_all_beside(&crate::arrangement::doc_in(&back), recorder::CHANNELS, None);
        assert_eq!((kits[2].pads[36].path.as_str(), panel), ("media/kick.wav", Some(2)), "{what}: the restored kit is not on lane 2");
        let reopened = crate::arrangement::Arrangement::read_from(&crate::arrangement::doc_in(&back));
        assert!(
            matches!(&reopened, crate::arrangement::LoadOutcome::Loaded(a) if a.desk.as_ref().is_some_and(|d| d.legacy_states.0.is_some())),
            "{what}: the restored document is not the release build's"
        );
    };
    let kicked = |app: &TangentApp| app.kit_at(2).pads[36].path.clone();

    // Save, in place.
    let song = root.join("Song");
    let want = a_release_builds_project(&song);
    let (_c1, mut app) = headless(Caps::DESKTOP);
    app.open_project(&crate::arrangement::doc_in(&song));
    assert_eq!(kicked(&app), "media/kick.wav", "fixture: the kit did not open on lane 2");
    app.save_project_doc();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(DrumpadDoc::sidecar(&crate::arrangement::doc_in(&song))).expect("kit")).expect("json");
    assert!(saved.get("track_kits").is_some(), "fixture: the save did not write the kit in format 2");
    restores("Save", &song, &want);

    // Save As over a release build's project in an existing folder.
    let theirs = root.join("Theirs");
    let want = a_release_builds_project(&theirs);
    let (_c2, mut app) = headless_with(Caps::DESKTOP, Settings { project_subdir: false, ..Settings::default() });
    app.bootstrap_project(&root.join("Projects"));
    app.kit_at_mut(2).pads[38].path = "media/snare.wav".into();
    assert!(app.save_project_as(&theirs), "the Save As was refused");
    assert_eq!(crate::arrangement::doc_in(&theirs), app.project_doc().expect("the copy").to_path_buf(), "fixture: the copy is not in the folder");
    restores("Save As into an existing folder", &theirs, &want);

    // The kit sidecar saving itself, a pad edited before any save.
    let edited = root.join("Edited");
    let want = a_release_builds_project(&edited);
    let (_c3, mut app) = headless(Caps::DESKTOP);
    app.open_project(&crate::arrangement::doc_in(&edited));
    app.set_drumpad_lane(2);
    app.kit_at_mut(2).pads[40].path = "media/tom.wav".into();
    app.drumpad_edited();
    restores("a pad edit", &edited, &want);
    app.save_project_doc();
    restores("a pad edit, then Save", &edited, &want);

    // The template seeding of a stock template a release build wrote.
    let template = root.join("Templates").join("default");
    std::fs::create_dir_all(template.join("media")).expect("the template");
    let mut stock = crate::arrangement::Arrangement::default();
    stock.desk = Some(TangentApp::template_desk());
    stock.save_to_as(&crate::arrangement::doc_in(&template), crate::desk_format::Format::Legacy);
    let want_doc = std::fs::read(crate::arrangement::doc_in(&template)).expect("the stock document");
    let (_, want_kit) = {
        let scratch = root.join("kit-only");
        let pair = a_release_builds_project(&scratch);
        std::fs::write(template.join(FILE_NAME), &pair.1).expect("the template's kit");
        pair
    };
    let (_c4, app) = headless(Caps::DESKTOP);
    app.seed_template(&template);
    let reseeded: serde_json::Value = serde_json::from_slice(&std::fs::read(crate::arrangement::doc_in(&template)).expect("doc")).expect("json");
    assert!(crate::desk_format::is_format2(&reseeded["desk"]), "fixture: the stock template was not reseeded in format 2");
    restores("the template seeding", &template, &(want_doc, want_kit.clone()));

    // The launch's reclaim steps past a release build's scratch whose kit is
    // not the template's, and leaves every file of it as it was.
    let projects = root.join("Reclaim");
    let scratch = projects.join("Untitled");
    let want = a_release_builds_project(&scratch);
    // An unnamed scratch with no clips: what a reclaim weighs by its kit.
    let mut clipless: serde_json::Value = serde_json::from_slice(&want.0).expect("json");
    clipless["clips"] = serde_json::json!([]);
    let clipless = serde_json::to_vec_pretty(&clipless).expect("json");
    std::fs::write(crate::arrangement::doc_in(&scratch), &clipless).expect("the scratch's document");
    let want = (clipless, want.1);
    let fresh = fresh_project_dir(&projects);
    assert_ne!(fresh, scratch, "the reclaim handed back a scratch holding a release build's own kit");
    restores("the launch's reclaim", &scratch, &want);
    let _ = std::fs::remove_dir_all(&root);
}
