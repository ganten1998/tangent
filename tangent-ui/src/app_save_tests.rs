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
