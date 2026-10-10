//! **The save roads' own tests** (W5-save-gaps): what a save writes is what
//! the session shows, and no kill or Don't Save loses a kit. A child of
//! `app`, in a file of its own, so it reads the app's private roads the way
//! its `tests` module does.

use super::tests::{headless, project_scratch};
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
