//! **The plugin-state shelves, named by identity** (design 6.6, contract C10).
//!
//! A plugin's knobs live in a file of their own: on the LIVE shelf beside
//! `settings.json` while the app runs, and in a project's or a template's
//! `state/` folder when the session is kept. The file is named by the bay it
//! belongs to, `pstate-<seatbay>.bin` (Appendix C: `t3-c0-b1`,
//! `master-c1-b2`), so nothing about a track's ROW is in the name and a
//! reorder renames nothing. The body is the old sidecar's, `<bundle path> NUL
//! <state>`, and the bundle is the owner every read checks: the wrong
//! plugin's state is not a preset, it is arbitrary bytes to a `setState`
//! that will believe them.
//!
//! **A new build never renames or deletes a legacy `insert-state-S-B.bin`.**
//! Those are the files a release build reads, and they are the way back:
//! restoring `settings.pre-v23.json` or a `<doc>.pre-v8` hands a release
//! build its plugins back whole only because nothing here ever touched them.
//! So the legacy names are only ever READ:
//!
//! - the global shelf is COPIED once into pstate names
//!   ([`convert_global_shelf`]), through the composed map of the frozen
//!   in-place renames the old builds ran (`desk_lift::legacy_shelf_seat`),
//!   from a durable source set, crash-safe at every point;
//! - a project's or a template's `state/` is read in place: pstate first,
//!   then, while its document carries the `legacy_states` marker, the legacy
//!   name the era's provenance gives the same bay ([`ShelfNames`]);
//! - the marker is cleared only once every plugin of the document ON DISK
//!   has its pstate file there ([`clear_legacy_marker`]), by a patch of that
//!   one key.
//!
//! **Every write is temp + rename**, so a crash leaves the old file or the
//! new one and never half of either.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tangent_ui::desk_id::{parse_state_file_name, state_file_name, SeatBay};
use tangent_ui::desk_legacy::{bind, decode_v32, identity_ids, V32_CHANNELS, V32_TOTAL_SEATS};
use tangent_ui::desk_lift::{legacy_shelf_seat, provenance_of, DeskEra, Provenance, ShelfShuffles};
use tangent_ui::settings::{Settings, SHELF_DONE_NAME, SHELF_PENDING_NAME};

/// What a legacy sidecar's name begins with. Read, never written.
const LEGACY_PREFIX: &str = "insert-state-";

/// **The LIVE shelf**: the config folder beside `settings.json`, where a
/// plugin's knobs sit while the app is running.
pub fn global_dir() -> PathBuf {
    Settings::path().parent().map(Path::to_path_buf).unwrap_or_default()
}

/// Where one bay's state lives on the live shelf.
pub fn state_path(sb: SeatBay) -> PathBuf {
    global_dir().join(state_file_name(sb))
}

/// **A bay's saved state from the live shelf**, when the file is `bundle`'s:
/// the owner check, on every read.
pub fn saved_state(sb: SeatBay, bundle: &str) -> Option<Vec<u8>> {
    read_state(&global_dir(), sb, bundle)
}

/// [`saved_state`] from any shelf.
pub fn read_state(dir: &Path, sb: SeatBay, bundle: &str) -> Option<Vec<u8>> {
    let (owner, blob) = owner_and_state(&dir.join(state_file_name(sb)))?;
    (owner == bundle).then_some(blob)
}

/// **A state file and WHOSE it is**, without asking whether it is the one
/// wanted: the bundle the write tagged it with, and the state after the NUL.
pub fn owner_and_state(path: &Path) -> Option<(String, Vec<u8>)> {
    fs_call();
    let raw = std::fs::read(path).ok()?;
    let split = raw.iter().position(|b| *b == 0)?;
    let owner = std::str::from_utf8(&raw[..split]).ok()?.to_owned();
    Some((owner, raw[split + 1..].to_vec()))
}

/// **Write a bay's state to the live shelf**, tagged with its owner, temp
/// and rename.
pub fn write_state(sb: SeatBay, bundle: &str, state: &[u8]) {
    write_state_in(&global_dir(), sb, bundle, state);
}

/// [`write_state`] onto any shelf. `false` when it did not land.
pub fn write_state_in(dir: &Path, sb: SeatBay, bundle: &str, state: &[u8]) -> bool {
    let mut out = Vec::with_capacity(bundle.len() + 1 + state.len());
    out.extend_from_slice(bundle.as_bytes());
    out.push(0);
    out.extend_from_slice(state);
    write_atomic(&dir.join(state_file_name(sb)), &out)
}

/// **`bytes` at `path`, through a temp file beside it and a rename.** The
/// temp name is not a state file's name (`parse_state_file_name` refuses
/// it), so one a crash left behind is never read as one.
fn write_atomic(path: &Path, bytes: &[u8]) -> bool {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs_call();
    if std::fs::write(&tmp, bytes).is_ok() && {
        #[cfg(test)]
        kill_point();
        !write_refused() && std::fs::rename(&tmp, path).is_ok()
    } {
        return true;
    }
    let _ = std::fs::remove_file(&tmp);
    false
}

/// **A legacy sidecar's bytes, for the copy**: `None` when the file cannot
/// be read (a refused permission, a volume gone), which the conversion
/// counts as a copy still owed and never as one done.
fn read_legacy(path: &Path) -> Option<Vec<u8>> {
    fs_call();
    if read_refused() {
        return None;
    }
    std::fs::read(path).ok()
}

/// **Which bays a shelf holds a pstate file for**: ONE directory read. A
/// name is acted on only when it is exactly what [`state_file_name`] would
/// write (the strict grammar), so a temp file, a legacy sidecar or anybody
/// else's file is never read as one.
pub fn shelf_states(dir: &Path) -> BTreeSet<SeatBay> {
    fs_call();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let sb = parse_state_file_name(e.file_name().to_str()?)?;
            fs_call();
            dir.join(e.file_name()).is_file().then_some(sb)
        })
        .collect()
}

/// **The legacy sidecars a shelf holds**, by their `(seat, bay)`: names that
/// round-trip exactly, as the old builds wrote them, at any seat a legacy
/// build ever had. Read only.
fn legacy_states(dir: &Path) -> BTreeSet<(usize, usize)> {
    fs_call();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let at = legacy_seat_bay(name.to_str()?)?;
            fs_call();
            dir.join(&name).is_file().then_some(at)
        })
        .collect()
}

/// The `(seat, bay)` a legacy sidecar's name names, or `None` for anything
/// that is not exactly one: bounded by the widest legacy seat space and by
/// the three bays, and round-tripping, so `insert-state-07-1.bin` is not
/// seat 7's.
fn legacy_seat_bay(name: &str) -> Option<(usize, usize)> {
    let (seat, bay) = name.strip_prefix(LEGACY_PREFIX)?.strip_suffix(".bin")?.split_once('-')?;
    let (seat, bay): (usize, usize) = (seat.parse().ok()?, bay.parse().ok()?);
    (seat < V32_TOTAL_SEATS && bay < tangent_ui::desk_id::BAYS && legacy_name(seat, bay) == name)
        .then_some((seat, bay))
}

/// A legacy sidecar's name, for READING it.
fn legacy_name(seat: usize, bay: usize) -> String {
    format!("{LEGACY_PREFIX}{seat}-{bay}.bin")
}

/// **The bay a V32 legacy flat bay is, by identity**: the frozen map and
/// the identity rule (legacy row r is `TrackId(r)` in every era, design 3).
fn v32_bay(flat: usize) -> Option<SeatBay> {
    let (owner, rack, bay) = decode_v32(flat)?;
    let node = bind(owner, &identity_ids(V32_CHANNELS))?;
    let sb = SeatBay::new(node, rack, bay);
    sb.is_valid().then_some(sb)
}

/// **How a shelf's states are named, for reading.** A shelf of this build's
/// (the live one, a format-2 project, any template) is pstate only. A
/// project or template whose document still carries the `legacy_states`
/// marker may hold its plugins under the legacy names of its ERA, and the
/// era's provenance says which legacy bay held what now sits at each V32
/// bay: those are read wherever a bay has no pstate file yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelfNames {
    Pstate,
    PstateThenLegacy(Provenance),
}

impl ShelfNames {
    /// The names a document's `state/` is read by, from its marker.
    pub fn for_marker(era: Option<DeskEra>) -> Self {
        match era {
            Some(era) => Self::PstateThenLegacy(provenance_of(era)),
            None => Self::Pstate,
        }
    }
}

/// **Every state a shelf holds, by the bay it belongs to**: its pstate
/// files, then, under [`ShelfNames::PstateThenLegacy`], each legacy file's
/// bay through the era's provenance where no pstate file answers for it.
fn readable_states(dir: &Path, names: &ShelfNames) -> BTreeMap<SeatBay, PathBuf> {
    let mut out: BTreeMap<SeatBay, PathBuf> =
        shelf_states(dir).into_iter().map(|sb| (sb, dir.join(state_file_name(sb)))).collect();
    if let ShelfNames::PstateThenLegacy(prov) = names {
        // Provenance runs lifted (V32) bay -> legacy bay; a shelf is read
        // the other way.
        let lifted: BTreeMap<u32, u32> = prov.to_legacy.iter().map(|(f, l)| (*l, *f)).collect();
        for (seat, bay) in legacy_states(dir) {
            let Some(flat) = u32::try_from(seat * tangent_ui::desk_id::BAYS + bay).ok() else {
                continue;
            };
            let Some(sb) = lifted.get(&flat).and_then(|f| v32_bay(*f as usize)) else {
                continue;
            };
            out.entry(sb).or_insert_with(|| dir.join(legacy_name(seat, bay)));
        }
    }
    out
}

/// **Mirror one shelf onto another, by identity.** Every state `from` holds
/// (by `names`) lands in `to` under its pstate name; every pstate file `to`
/// holds that `from` does not is REMOVED, because a stale state is the last
/// session's reverb tail arriving in this session's fresh plugin. A legacy
/// sidecar in `to` is never touched, and nothing in `from` is ever written.
///
/// **Two directory reads, not a walk of the seat space**: what a mirror asks
/// is which states EXIST, and each shelf answers that in one read (see
/// `what_the_shelf_walk_costs_at_project_open`).
pub fn mirror_shelf(from: &Path, names: &ShelfNames, to: &Path) {
    // BOTH read before either is written, so the second read is of the
    // shelf as it arrived rather than of the copies just made.
    let src = readable_states(from, names);
    let dst = shelf_states(to);
    for (sb, path) in &src {
        fs_call();
        if let Ok(bytes) = std::fs::read(path) {
            write_atomic(&to.join(state_file_name(*sb)), &bytes);
        }
    }
    for sb in dst.iter().filter(|sb| !src.contains_key(sb)) {
        fs_call();
        let _ = std::fs::remove_file(to.join(state_file_name(*sb)));
    }
}

// ── the displaced-state shelf ───────────────────────────────────────────
//
// In memory, beside the files above: what a plugin was PLAYING when a bay
// relabel (a paste, a swap, a delete, an undo) took it out of its bay, kept
// by the bay's identity and the plugin's bundle (contract C10), so the next
// time that bay seats that bundle it comes back as it was rather than at
// its last save.

/// **One plugin's settings on the displaced-state shelf**, and when they
/// were put there. See `DesktopApp::displaced_states`.
pub(crate) struct ShelvedState {
    /// What the plugin's `save_insert_state` answered as it left the bay.
    pub(crate) blob: Vec<u8>,
    /// Later shelvings count higher; the lowest goes first when the shelf
    /// is full.
    stamp: u64,
}

/// The displaced-state shelf: `(bay, bundle)` to what that plugin was playing
/// when the reconcile took it out of that bay, the bay by IDENTITY (contract
/// C10: "DisplacedShelf keyed (SeatBay, bundle)"). A track's bays are its
/// own wherever its row goes, so a reorder moves nothing here, and a track
/// that leaves and comes back by an undo finds its own entries waiting.
pub(crate) type DisplacedShelf = std::collections::HashMap<(SeatBay, String), ShelvedState>;

/// The most plugin states the shelf holds, and the most bytes of them.
pub(crate) const SHELF_MAX_ENTRIES: usize = 256;
pub(crate) const SHELF_MAX_BYTES: usize = 64 << 20;

/// **Shelve one plugin's settings, and when the shelf is full drop the
/// OLDEST, never all of it.**
///
/// Every unload shelves, so the shelf fills in ordinary work, and the answer
/// to a full shelf used to be to clear it: thirty-two entries, and the
/// thirty-third threw away exactly the settings the next Ctrl+Z was about to
/// ask for. Undo walks back from the newest end, so the oldest entry is the
/// one least likely to be asked for; the newest is kept whatever its size,
/// because it is the one the very next undo wants. An empty blob restores
/// nothing and is not kept.
pub(crate) fn shelve_displaced(shelf: &mut DisplacedShelf, key: (SeatBay, String), blob: Vec<u8>) {
    if blob.is_empty() {
        return;
    }
    let stamp = shelf.values().map(|s| s.stamp + 1).max().unwrap_or(0);
    shelf.insert(key, ShelvedState { blob, stamp });
    loop {
        let bytes: usize = shelf.values().map(|s| s.blob.len()).sum();
        if shelf.len() <= 1 || (shelf.len() <= SHELF_MAX_ENTRIES && bytes <= SHELF_MAX_BYTES) {
            break;
        }
        let Some(oldest) = shelf.iter().min_by_key(|(_, s)| s.stamp).map(|(k, _)| k.clone()) else {
            break;
        };
        shelf.remove(&oldest);
    }
}

/// **The settings a bay's incoming plugin is seated with**: what the shelf
/// kept of that bundle at that seat FIRST, its sidecar when the shelf has
/// nothing, and nothing when neither belongs to it.
///
/// The shelf outranks the file because it is newer by construction: an
/// entry is what the plugin was playing when it left the bay, every road
/// that writes the bay's sidecar for that bundle afterwards forgets the
/// entry, and the shelf is keyed by the bay's IDENTITY, so a reorder cannot
/// leave an entry at a seat its track has moved away from. The file first
/// would be the last SAVE: a
/// unit dialled in since, deleted and brought back by Ctrl+Z, came back at
/// the setting it had before the dial.
pub(crate) fn incoming_state(
    shelf: &DisplacedShelf,
    key: &(SeatBay, String),
    sidecar: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    shelf.get(key).map(|s| s.blob.clone()).or_else(sidecar)
}


/// What a global-shelf conversion did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShelfConversion {
    /// It ran: no `.shelf-v23` stood when it started.
    pub ran: bool,
    /// Legacy files copied to a pstate name.
    pub copied: usize,
    /// Legacy files whose pstate name was already there (a copy an earlier,
    /// interrupted run made, or a state written since): left alone.
    pub kept: usize,
    /// Legacy files that name a bay and whose copy did not land: the read
    /// failed, or the write or its rename did.
    pub failed: usize,
    /// **It could not finish, and the next launch runs it again**: a copy
    /// failed, or `.shelf-v23` itself did not land. The pending marker
    /// stands (written here when this run's set came from the launch), so
    /// that run copies through the same set, and copy-if-absent makes it
    /// redo only what is missing.
    pub owed: bool,
}

/// **The global shelf's one conversion** (design 6.6), before any project
/// opens: see [`convert_shelf_in`].
pub fn convert_global_shelf(launch: ShelfShuffles) -> ShelfConversion {
    convert_shelf_in(&global_dir(), launch)
}

/// **Copy every legacy sidecar on `dir` to its pstate name, once.**
///
/// The source set is the durable `.shelf-v23.pending` marker when it stands
/// (written before the first format-2 settings write, from the shuffles the
/// load that read the legacy file raised), and `launch` (this launch's
/// `Settings::shelf_shuffles`) when it does not: a load raises the set and a
/// format-2 file raises none, so the marker is what survives a crash
/// between the two. Each legacy file's seat goes through the composed map
/// of the renames the old builds ran in place (`legacy_shelf_seat`), then
/// the frozen V32 map and the identity rule, to a bay; the file is copied
/// to that bay's pstate name, temp and rename, unless the name is already
/// there. `.shelf-v23` is written LAST and the marker removed after it.
///
/// So it is idempotent, safe to kill at any point (a rerun copies what is
/// missing and keeps what is there), and it never runs again once
/// `.shelf-v23` stands: a pstate file deleted after that never comes back.
/// No legacy file is renamed or deleted.
///
/// **And `.shelf-v23` is written only over a WHOLE copy.** A legacy file
/// that names a bay and could not be read, or whose copy did not land (a
/// refused permission, a full disk, a rename that failed), leaves the
/// conversion owed: no `.shelf-v23`, and the pending marker standing,
/// written here from this run's set when the launch supplied it. Marked
/// done over a failed copy, the conversion never ran again and that bay's
/// knobs stayed in a file this build never reads (the global shelf is read
/// by identity names alone), so its plugin opened at its defaults for
/// good. Owed, the next launch copies through the same set, and
/// copy-if-absent makes that run redo only the copies still missing.
pub fn convert_shelf_in(dir: &Path, launch: ShelfShuffles) -> ShelfConversion {
    let pending = dir.join(SHELF_PENDING_NAME);
    if dir.join(SHELF_DONE_NAME).exists() {
        // A crash between the two writes below left the marker behind.
        let _ = std::fs::remove_file(&pending);
        return ShelfConversion::default();
    }
    let marked = std::fs::read_to_string(&pending)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| {
            let names: Vec<String> = v
                .get("shuffles")?
                .as_array()?
                .iter()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect();
            ShelfShuffles::from_names(names.iter().map(String::as_str))
        });
    let set = marked.unwrap_or(launch);
    let mut done = ShelfConversion { ran: true, ..ShelfConversion::default() };
    for (legacy, bay) in legacy_states(dir) {
        // Where the old builds' renames would have put it: `None` where they
        // deleted or overwrote it, which no file of that era's own holds.
        let Some(seat) = legacy_shelf_seat(set, legacy) else { continue };
        let Some(sb) = v32_bay(seat * tangent_ui::desk_id::BAYS + bay) else {
            continue;
        };
        let to = dir.join(state_file_name(sb));
        fs_call();
        if to.exists() {
            done.kept += 1;
            continue;
        }
        let landed = read_legacy(&dir.join(legacy_name(legacy, bay))).is_some_and(|bytes| write_atomic(&to, &bytes));
        if landed {
            done.copied += 1;
        } else {
            done.failed += 1;
        }
    }
    // LAST, and only over a whole copy: from here on the conversion never
    // runs again.
    if done.failed == 0 && write_atomic(&dir.join(SHELF_DONE_NAME), b"") {
        let _ = std::fs::remove_file(&pending);
        return done;
    }
    // **Owed.** The marker is what names the set to the next launch, whose
    // format-2 settings raise none: one this run read stands as it is, and
    // one it did not (the set was the launch's, or the marker would not
    // parse) is written now.
    done.owed = true;
    if marked.is_none() {
        write_pending(dir, set);
    }
    done
}

/// **The pending marker, naming `set`**, temp and rename: the shape
/// `Settings::save_to` writes before the first format-2 settings write, read
/// by [`convert_shelf_in`] for `shuffles` alone.
fn write_pending(dir: &Path, set: ShelfShuffles) -> bool {
    let marker = serde_json::json!({ "shuffles": set.names() });
    serde_json::to_string_pretty(&marker).is_ok_and(|text| write_atomic(&dir.join(SHELF_PENDING_NAME), text.as_bytes()))
}

/// **A mirror the live shelf owes a project's NEW home**, written down
/// beside the live shelf: see [`owe_mirror`].
pub const MIRROR_OWED_NAME: &str = ".states-owed.json";

/// **Write down, BEFORE a save road's document lands in `home`, that the
/// live shelf in `shelf` is owed to `home`'s `state/`** (design 6.6: "None
/// of them can leave a plugin at defaults").
///
/// Save As and the first save write the document into a folder that has no
/// `state/` of its own (a copy brings `media/` and nothing else; a folder
/// born at the chosen place has nothing at all), and the host mirrors the
/// live shelf into it only after the document has landed. A process killed
/// between the two left a project whose plugins all opened at their
/// defaults, while the knobs sat on the live shelf the next launch replaces
/// with the template's before anybody can open anything. So the road writes
/// this first, temp and rename: the home, and what its document was before
/// the save (its bytes' fingerprint, or nothing), so that the launch that
/// finds it ([`pay_owed_mirror`]) can tell a document the road wrote from
/// one it never reached. [`mirror_paid`] takes it away once the road has
/// mirrored. `false` when it did not land.
pub fn owe_mirror(shelf: &Path, home: &Path) -> bool {
    let owed = serde_json::json!({
        "home": home.to_string_lossy(),
        "before": doc_fingerprint(home),
    });
    serde_json::to_string_pretty(&owed).is_ok_and(|text| write_atomic(&shelf.join(MIRROR_OWED_NAME), text.as_bytes()))
}

/// The road has mirrored (or wrote nothing): no mirror is owed.
pub fn mirror_paid(shelf: &Path) {
    let owed = shelf.join(MIRROR_OWED_NAME);
    fs_call();
    let _ = std::fs::remove_file(&owed);
}

/// **A mirror a save road owed and never made, made now** (see
/// [`owe_mirror`]): at launch, before anything replaces the live shelf, so
/// the shelf is still the one the dying session wrote its knobs to. Only
/// when the road's document LANDED (the home's document is there and is
/// not the one the road found), because a road killed before its write
/// owes nothing, and a folder that held somebody else's project must not
/// be handed this session's states. The mirror is the one every save road
/// makes, by identity, and the document's `legacy_states` marker is then
/// cleared if the folder is whole, as `copy_states_to_project` clears it.
/// Answers the home it paid, if any; the record goes either way.
pub fn pay_owed_mirror(shelf: &Path) -> Option<PathBuf> {
    let owed = shelf.join(MIRROR_OWED_NAME);
    let text = std::fs::read_to_string(&owed).ok()?;
    let record = serde_json::from_str::<Value>(&text).ok();
    let home = record
        .as_ref()
        .and_then(|v| v.get("home"))
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let before = record.as_ref().and_then(|v| v.get("before")).and_then(Value::as_str).map(str::to_owned);
    let paid = home.filter(|home| {
        let doc = tangent_ui::arrangement::doc_in(home);
        doc.is_file() && doc_fingerprint(home) != before
    });
    if let Some(home) = paid.as_deref() {
        let state = home.join("state");
        let _ = std::fs::create_dir_all(&state);
        mirror_shelf(shelf, &ShelfNames::Pstate, &state);
        let doc = tangent_ui::arrangement::doc_in(home);
        if let Some(plugins) = doc_plugins(&doc) {
            clear_legacy_marker(&doc, &plugins);
        }
    }
    mirror_paid(shelf);
    paid
}

/// **What `home`'s document is**, as a fingerprint of its bytes (FNV-1a,
/// 64 bits, in hex): `None` when it has none. Only ever compared with
/// itself, across one save.
fn doc_fingerprint(home: &Path) -> Option<String> {
    let bytes = std::fs::read(tangent_ui::arrangement::doc_in(home)).ok()?;
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    Some(format!("{hash:016x}"))
}

/// **The plugins a document ON DISK names**, by bay: its format-2 desk's
/// `plugins` object. `None` for a file that is not a format-2 document (a
/// legacy one cannot carry the marker), or one that will not parse.
pub fn doc_plugins(doc: &Path) -> Option<BTreeMap<SeatBay, String>> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(doc).ok()?).ok()?;
    let desk = v.get("desk")?;
    if !tangent_ui::desk_format::is_format2(desk) {
        return None;
    }
    Some(
        desk.get("plugins")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, p)| Some((k.parse().ok()?, p.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// **Clear a document's `legacy_states` marker, once its `state/` is whole**
/// (design 6.6): only when every plugin `plugins` names (the document ON
/// DISK's, see [`doc_plugins`]) that keeps a state has its pstate file in
/// the document's `state/`, owned by it, and then by patching that one key
/// out of the file, temp and rename: the session is never re-serialised to
/// do it. An instrument the engine renders itself keeps no state and is
/// not waited for. `true` when the file no longer carries the marker.
pub fn clear_legacy_marker(doc: &Path, plugins: &BTreeMap<SeatBay, String>) -> bool {
    let Some(state) = doc.parent().map(|d| d.join("state")) else {
        return false;
    };
    let whole = plugins
        .iter()
        .filter(|(_, p)| !crate::desktop::is_instrument_sentinel(p))
        .all(|(sb, p)| read_state(&state, *sb, p).is_some());
    whole && strip_legacy_marker(doc)
}

/// **Patch the `legacy_states` key out of a document's desk**, and nothing
/// else: parsed, the one key removed, and written back in the document
/// writer's own layout (pretty JSON in its own key order), so on a file
/// this writer wrote the bytes change by that key's line alone. Temp and
/// rename. `true` when the file carries no marker afterwards.
pub fn strip_legacy_marker(doc: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(doc) else {
        return false;
    };
    let Ok(mut v) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let Some(desk) = v.get_mut("desk").and_then(Value::as_object_mut) else {
        return false;
    };
    if desk.shift_remove("legacy_states").is_none() {
        return true;
    }
    serde_json::to_string_pretty(&v).is_ok_and(|out| write_atomic(doc, out.as_bytes()))
}

#[cfg(test)]
thread_local! {
    /// Counted so a shelf walk's cost is a NUMBER rather than a thing to
    /// reason about: the reorder and project-open tests assert on it. Per
    /// THREAD, because the suite runs on parallel threads in one process and
    /// every shelf call runs on its caller's: a count read here is exactly
    /// this test's own, with no lock against the rest of the suite.
    static FS_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many filesystem calls the shelf has made on this thread.
#[cfg(test)]
pub(crate) fn fs_calls() -> usize {
    FS_CALLS.with(std::cell::Cell::get)
}

#[inline]
fn fs_call() {
    #[cfg(test)]
    FS_CALLS.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
thread_local! {
    /// **A process killed mid-write, for the crash tests**: when set, the
    /// write that finds it at zero dies between its temp file and its rename
    /// (the worst moment a kill can pick), every earlier one counting it
    /// down. Per thread, like the count above.
    static KILL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn kill_point() {
    KILL_AFTER.with(|k| match k.get() {
        Some(0) => {
            k.set(None);
            panic!("killed between a temp file and its rename");
        }
        Some(n) => k.set(Some(n - 1)),
        None => {}
    });
}

#[cfg(test)]
thread_local! {
    /// **A write that fails and a read that fails, for the failure tests**:
    /// when set, the write (or the legacy read) that finds its count at zero
    /// fails the way a full disk, a refused permission or a refused rename
    /// fails it, and the process goes on; every earlier one counts it down.
    /// The kill above ends the run; these let it carry on past the failure,
    /// which is what decides whether it may still call itself done.
    static FAIL_WRITE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static FAIL_READ: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Whether the countdown in `cell` has reached this call.
#[cfg(test)]
fn counted_out(cell: &std::cell::Cell<Option<usize>>) -> bool {
    match cell.get() {
        Some(0) => {
            cell.set(None);
            true
        }
        Some(n) => {
            cell.set(Some(n - 1));
            false
        }
        None => false,
    }
}

/// Whether this write's rename is refused (tests only; never in a build).
#[inline]
fn write_refused() -> bool {
    #[cfg(test)]
    return FAIL_WRITE.with(counted_out);
    #[cfg(not(test))]
    false
}

/// Whether this legacy read fails (tests only; never in a build).
#[inline]
fn read_refused() -> bool {
    #[cfg(test)]
    return FAIL_READ.with(counted_out);
    #[cfg(not(test))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tangent_ui::arrangement::Arrangement;
    use tangent_ui::desk_id::{Node, Rack};
    use tangent_ui::host::Caps;

    macro_rules! fixture {
        ($name:literal) => {
            ($name, include_str!(concat!("../../tests/fixtures/eras/", $name)))
        };
    }

    /// The era documents, one per shape a document's desk was ever written
    /// in (`DeskEra`): the 16-pad sub-era, the drum-bus-less sixteen, the
    /// sixteen, the 24-channel window, the 32 dense and the 32 head + tail.
    const DOCUMENTS: [(&str, &str); 6] = [
        fixture!("doc-v4-11.json"),
        fixture!("doc-v5-18.json"),
        fixture!("doc-v5-19.json"),
        fixture!("doc-v6-27.json"),
        fixture!("doc-v6-35.json"),
        fixture!("doc-v7.json"),
    ];

    /// The era settings files. (The DAW blobs keep their plugins' state in
    /// the DAW's project, never on a shelf of ours.)
    const SETTINGS: [(&str, &str); 8] = [
        fixture!("settings-v12.json"),
        fixture!("settings-v17-pads16.json"),
        fixture!("settings-v18.json"),
        fixture!("settings-v19.json"),
        fixture!("settings-v20-27.json"),
        fixture!("settings-v20.json"),
        fixture!("settings-v21.json"),
        fixture!("settings-v22.json"),
    ];

    /// A folder of this test's own, empty: nothing here ever touches the
    /// live shelf or anybody's home.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tangent-shelf-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// The knobs a legacy sidecar holds in these fixtures: named for the
    /// legacy bay they were saved at, so a state that arrives anywhere
    /// says where it came from.
    fn knobs(flat: usize) -> Vec<u8> {
        format!("knobs of legacy bay {flat}").into_bytes()
    }

    /// **Every occupied legacy bay of a legacy desk**, flat bay to bundle:
    /// its dense array (or the fully sparse object) and its sparse tail, a
    /// seat the array speaks for keeping the array's word.
    fn legacy_bays(desk: &Value, dense: &str, tail: &str) -> BTreeMap<usize, String> {
        let mut out = BTreeMap::new();
        let mut take = |k: usize, p: &Value| {
            if let Some(p) = p.as_str().filter(|p| !p.is_empty()) {
                out.entry(k).or_insert_with(|| p.to_owned());
            }
        };
        match desk.get(dense) {
            Some(Value::Array(a)) => a.iter().enumerate().for_each(|(k, p)| take(k, p)),
            Some(Value::Object(o)) => o.iter().filter_map(|(k, p)| Some((k.parse().ok()?, p))).for_each(|(k, p)| take(k, p)),
            _ => {}
        }
        if let Some(Value::Object(o)) = desk.get(tail) {
            o.iter().filter_map(|(k, p)| Some((k.parse().ok()?, p))).for_each(|(k, p)| take(k, p));
        }
        out
    }

    /// **A folder as an old build left it**: one legacy sidecar per
    /// occupied bay, at that bay's seat, holding its bundle and its knobs.
    fn seed(dir: &Path, bays: &BTreeMap<usize, String>) {
        for (flat, bundle) in bays {
            let mut body = bundle.as_bytes().to_vec();
            body.push(0);
            body.extend_from_slice(&knobs(*flat));
            std::fs::write(dir.join(legacy_name(flat / 3, flat % 3)), body).expect("a legacy sidecar");
        }
    }

    /// Every file in `dir`, by name, with its bytes.
    fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).expect("read")))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The legacy sidecars of a folder, by name, with their bytes.
    fn legacy_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        files(dir).into_iter().filter(|(n, _)| n.starts_with(LEGACY_PREFIX)).collect()
    }

    /// The plugins a desk names that keep a state: everything but the
    /// instruments the engine renders itself.
    fn stateful(plugins: &BTreeMap<SeatBay, String>) -> Vec<(SeatBay, String)> {
        plugins
            .iter()
            .filter(|(_, p)| !crate::desktop::is_instrument_sentinel(p))
            .map(|(sb, p)| (*sb, p.clone()))
            .collect()
    }

    /// **Every plugin found its OWN knobs on `shelf`**: for each bay the
    /// opened desk names, the state file at its identity name is owned by
    /// its plugin (the owner check) and holds, byte for byte, the knobs of
    /// the legacy bay that plugin was saved at. Answers how many it checked.
    fn every_state_found(
        what: &str,
        shelf: &Path,
        plugins: &BTreeMap<SeatBay, String>,
        bays: &BTreeMap<usize, String>,
    ) -> usize {
        // The fixtures name every bay's plugin uniquely, so the bundle says
        // which legacy bay a plugin came from.
        let from: BTreeMap<&str, usize> = bays.iter().map(|(f, p)| (p.as_str(), *f)).collect();
        let mut checked = 0;
        for (sb, bundle) in stateful(plugins) {
            let flat = *from.get(bundle.as_str()).unwrap_or_else(|| panic!("{what}: {bundle} came from no legacy bay"));
            assert_eq!(
                read_state(shelf, sb, &bundle),
                Some(knobs(flat)),
                "{what}: {sb} ({bundle}, legacy bay {flat}) opened without its own knobs"
            );
            checked += 1;
        }
        checked
    }

    /// A legacy document of `era` with a plugin in every `stride`th bay of
    /// its whole seat map (and in every bay of its head, its first 120),
    /// each named for its legacy bay: a desk whose open lifts every kind of
    /// seat the era had (channels, buses, the master and its extra rack,
    /// pads, folder-extra racks and pedals) at once.
    fn every_kind_of_seat(era: DeskEra, stride: usize) -> (String, BTreeMap<usize, String>) {
        let sends = match era {
            DeskEra::V8Pads16 | DeskEra::V8 => 11,
            DeskEra::V16NoDrumBus => 18,
            DeskEra::V16 => 19,
            DeskEra::V24 => 27,
            DeskEra::V32 => 35,
        };
        let len = era.bays();
        let bays: BTreeMap<usize, String> = (0..len)
            .filter(|f| f % stride == 0 || *f < 120 || *f + 3 >= len)
            .map(|f| (f, format!("/x/b{f}.vst3")))
            .collect();
        let dense: Vec<&str> = (0..len).map(|f| bays.get(&f).map_or("", String::as_str)).collect();
        let doc = serde_json::json!({
            "arrangement_version": 6,
            "clips": [],
            "desk": {"sends": vec![0.0; sends], "inserts": dense},
        });
        (doc.to_string(), bays)
    }

    /// **For every era, every plugin's state reaches its own bay after
    /// open** (W4-integrate, done-when "Era projects"): the era fixture
    /// projects, and a desk of each era with a plugin on every kind of seat
    /// it had. A project folder's `state/` holds the legacy sidecars an old
    /// build wrote; the open reads it pstate-then-legacy through the era's
    /// provenance (`ShelfNames::for_marker`, from the marker the legacy read
    /// sets), onto the live shelf by identity, and every plugin of the
    /// lifted desk finds its own knobs there with the owner check passing
    /// and the bytes equal. The folder's legacy files are left byte for
    /// byte, and the live shelf holds identity names only.
    #[test]
    fn every_era_projects_states_reach_their_own_bays() {
        let mut cases: Vec<(String, String, BTreeMap<usize, String>)> = DOCUMENTS
            .iter()
            .map(|(name, text)| {
                let raw: Value = serde_json::from_str(text).expect("json");
                ((*name).to_owned(), (*text).to_owned(), legacy_bays(&raw["desk"], "inserts", "inserts_tail"))
            })
            .collect();
        for era in [DeskEra::V8Pads16, DeskEra::V8, DeskEra::V16NoDrumBus, DeskEra::V16, DeskEra::V24, DeskEra::V32] {
            let stride = if era.bays() > 1000 { 7 } else { 1 };
            let (text, bays) = every_kind_of_seat(era, stride);
            cases.push((format!("every seat of {era}"), text, bays));
        }
        let mut eras = BTreeSet::new();
        for (name, text, bays) in cases {
            let project = scratch("era-project");
            let state = project.join("state");
            std::fs::create_dir_all(&state).expect("state/");
            seed(&state, &bays);
            let before = files(&state);
            let opened = Arrangement::from_json(&text);
            let desk = opened.desk.as_ref().expect("a desk");
            let era = desk.legacy_states.0.unwrap_or_else(|| panic!("{name}: a legacy document lost its era"));
            eras.insert(era.name());
            let plugins = desk.desk_model().model.plugins;
            let live = scratch("era-live");
            mirror_shelf(&state, &ShelfNames::for_marker(Some(era)), &live);
            let checked = every_state_found(&name, &live, &plugins, &bays);
            assert!(checked >= bays.len().min(4), "{name}: only {checked} plugins to check");
            assert_eq!(files(&state), before, "{name}: the open touched the project's state/");
            for n in files(&live).keys() {
                assert!(parse_state_file_name(n).is_some(), "{name}: the live shelf holds `{n}`");
            }
            let _ = std::fs::remove_dir_all(&project);
            let _ = std::fs::remove_dir_all(&live);
        }
        assert_eq!(
            eras.into_iter().collect::<Vec<_>>(),
            ["v16", "v16-no-drum-bus", "v24", "v32", "v8", "v8-pads16"],
            "an era went untested"
        );
    }

    /// **The global shelf of every settings era converts onto its bays**
    /// (design 6.6): a config folder holding the legacy sidecars an old
    /// build left, at the seats that build gave them, converts through the
    /// set the load raised (`Settings::shelf_shuffles`). Every legacy file is
    /// copied, byte for byte, to the identity name of the bay the old
    /// builds' own in-place renames took its seat to; every plugin of the
    /// desk the load lifted finds its own knobs there, the owner check
    /// passing; every legacy file is left as it was, and the conversion is
    /// marked done.
    ///
    /// **One exception, and it is the old builds' own**: a pre-lanes kit's
    /// pad plugins, which the settings reader moves onto the drum channel's
    /// pads (`move_legacy_pad_racks`) and whose sidecars no build ever moved
    /// with them. Their states wait at channel 0's pads, where every build
    /// left them, and those plugins open at their defaults as they did on
    /// every build since the move: a pre-existing loss the conversion
    /// neither causes nor, copying through the frozen renames, repairs.
    /// Asserted as exactly that, so a change to either side shows here.
    #[test]
    fn the_global_shelf_converts_every_settings_era() {
        for (name, text) in SETTINGS {
            let raw: Value = serde_json::from_str(text).expect("json");
            let bays = legacy_bays(&raw, "strip_inserts", "strip_inserts_tail");
            let dir = scratch("convert-era");
            seed(&dir, &bays);
            let before = legacy_files(&dir);
            let s = Settings::from_json(text);
            let set = s.shelf_shuffles();
            let done = convert_shelf_in(&dir, set);
            assert!(done.ran, "{name}: the conversion did not run");
            let mut landed = BTreeMap::new();
            for (flat, bundle) in &bays {
                // `None`: the renames deleted or overwrote it, as v13 did
                // the retired backing strip's.
                let Some(seat) = legacy_shelf_seat(set, flat / 3) else { continue };
                let sb = v32_bay(seat * tangent_ui::desk_id::BAYS + flat % 3).expect("a V32 bay");
                assert_eq!(
                    read_state(&dir, sb, bundle),
                    Some(knobs(*flat)),
                    "{name}: legacy bay {flat} ({bundle}) was not copied to {sb}"
                );
                landed.insert(bundle.as_str(), sb);
            }
            let mut found = 0;
            for (sb, bundle) in stateful(&s.desk_model().model.plugins) {
                if read_state(&dir, sb, &bundle).is_some() {
                    found += 1;
                    continue;
                }
                let at = landed.get(bundle.as_str()).copied();
                let moved_pad = matches!(sb.seat.rack, Rack::Pad(_))
                    && at.is_some_and(|at| {
                        at.seat.node == Node::Track(tangent_ui::desk_id::TrackId(0))
                            && (at.seat.rack, at.bay) == (sb.seat.rack, sb.bay)
                    });
                assert!(moved_pad, "{name}: {sb} ({bundle}) opened without its own knobs; its state is at {at:?}");
            }
            assert!(found > 0, "{name}: no plugin found its knobs");
            assert_eq!(legacy_files(&dir), before, "{name}: a legacy sidecar was renamed, rewritten or deleted");
            assert!(dir.join(SHELF_DONE_NAME).is_file(), "{name}: the conversion is not marked done");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// The settings file the conversion tests stand on: v12, which raises
    /// every one of the old in-place renames at once.
    fn v12() -> (&'static str, BTreeMap<usize, String>, BTreeMap<SeatBay, String>) {
        let (_, text) = SETTINGS[0];
        let raw: Value = serde_json::from_str(text).expect("json");
        let s = Settings::from_json(text);
        assert_eq!(s.shelf_shuffles().names(), ["v13", "v14", "v18", "v19", "v20"], "fixture: v12's set");
        (text, legacy_bays(&raw, "strip_inserts", "strip_inserts_tail"), s.desk_model().model.plugins)
    }

    /// **A kill after the settings' first format-2 save, and before the
    /// conversion, relaunches with every state on its seat.** The format-2
    /// file the relaunch reads raises no shuffle at all, so only the durable
    /// marker the save wrote first (`.shelf-v23.pending`, `tangent-ui`'s
    /// `Settings::save_to`, whose own tests pin that it is written before
    /// the first format-2 write and from the set the load raised) can name
    /// the set; the conversion reads it, copies, writes `.shelf-v23` and only
    /// then takes the marker away. The two files are written here as that
    /// save leaves them.
    #[test]
    fn a_kill_after_the_settings_save_converts_from_the_pending_marker() {
        let (text, bays, plugins) = v12();
        let dir = scratch("kill-saved");
        seed(&dir, &bays);
        let before = legacy_files(&dir);
        let launch = Settings::from_json(text);
        let marker = serde_json::json!({"settings_version": 12, "shuffles": launch.shelf_shuffles().names()});
        std::fs::write(dir.join(SHELF_PENDING_NAME), marker.to_string()).expect("the marker");
        let path = dir.join("settings.json");
        std::fs::write(&path, launch.to_json()).expect("the format-2 settings");
        // Killed here. The relaunch:
        let relaunch = Settings::from_json(&std::fs::read_to_string(&path).expect("the settings"));
        assert!(relaunch.shelf_shuffles().is_empty(), "fixture: a format-2 file raised a shuffle");
        let done = convert_shelf_in(&dir, relaunch.shelf_shuffles());
        assert!(done.ran && done.copied > 0, "the relaunch converted nothing: {done:?}");
        every_state_found("v12 relaunched", &dir, &plugins, &bays);
        assert!(dir.join(SHELF_DONE_NAME).is_file(), "the conversion is not marked done");
        assert!(!dir.join(SHELF_PENDING_NAME).exists(), "the marker outlived the conversion");
        assert_eq!(legacy_files(&dir), before, "a legacy sidecar was renamed, rewritten or deleted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A kill in the middle of the copies, or after some of them, and the
    /// conversion's later life** (design 6.6): idempotent, crash-safe at
    /// every point, and never run twice.
    ///
    /// The folder a conversion was killed in holds the copies it had made,
    /// a half-written temp of the one it was making, the pending marker and
    /// no `.shelf-v23`; the relaunch (whose format-2 settings raise nothing)
    /// finishes it from the marker, keeps what was copied, and every plugin
    /// finds its own knobs. Run again it does nothing at all; a state
    /// deleted after it never comes back; and through all of it no legacy
    /// sidecar is renamed, rewritten or deleted.
    #[test]
    fn the_conversion_survives_a_kill_mid_copy_and_never_runs_twice() {
        let (_, bays, plugins) = v12();
        let set = ShelfShuffles::from_names(["v13", "v14", "v18", "v19", "v20"]).expect("the names");
        // What a whole conversion makes, to take the killed one's copies from.
        let whole = scratch("kill-whole");
        seed(&whole, &bays);
        convert_shelf_in(&whole, set);
        let made: Vec<String> = shelf_states(&whole).into_iter().map(state_file_name).collect();
        assert!(made.len() > 4, "fixture: v12 makes {} states", made.len());

        let dir = scratch("kill-mid");
        seed(&dir, &bays);
        let before = legacy_files(&dir);
        std::fs::write(dir.join(SHELF_PENDING_NAME), r#"{"settings_version": 12, "shuffles": ["v13", "v14", "v18", "v19", "v20"]}"#)
            .expect("the marker");
        let k = made.len() / 2;
        for n in &made[..k] {
            std::fs::copy(whole.join(n), dir.join(n)).expect("an earlier copy");
        }
        std::fs::write(dir.join(format!("{}.tmp", made[k])), b"half of a st").expect("a torn temp");

        let done = convert_shelf_in(&dir, ShelfShuffles::NONE);
        assert_eq!((done.kept, done.copied), (k, made.len() - k), "the relaunch did not finish what was left");
        every_state_found("v12 after a kill mid-copy", &dir, &plugins, &bays);
        assert!(!dir.join(format!("{}.tmp", made[k])).exists(), "the torn temp was left behind");
        assert!(!dir.join(SHELF_PENDING_NAME).exists(), "the marker outlived the conversion");

        // Idempotent: a second run is no run.
        let settled = files(&dir);
        assert_eq!(convert_shelf_in(&dir, set), ShelfConversion::default(), "the conversion ran twice");
        assert_eq!(files(&dir), settled, "a second run changed the shelf");
        // Never again: a state deleted since stays deleted, even with a
        // marker somebody's older build left standing.
        let gone = dir.join(&made[0]);
        std::fs::remove_file(&gone).expect("delete a state");
        std::fs::write(dir.join(SHELF_PENDING_NAME), r#"{"shuffles": ["v20"]}"#).expect("a stray marker");
        assert_eq!(convert_shelf_in(&dir, set), ShelfConversion::default(), "the conversion ran again");
        assert!(!gone.exists(), "a deleted state came back");
        assert!(!dir.join(SHELF_PENDING_NAME).exists(), "a stray marker outlived a finished conversion");
        assert_eq!(legacy_files(&dir), before, "a legacy sidecar was renamed, rewritten or deleted");
        let _ = std::fs::remove_dir_all(&whole);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A kill at EVERY point of the conversion relaunches with every
    /// state on its seat** (design 6.6). The conversion is killed between
    /// the temp file and the rename of each of its writes in turn, the
    /// copies and `.shelf-v23` alike, which leaves every shape an
    /// interrupted run can: some copies made, one half made, the marker
    /// standing, `.shelf-v23` absent. Each relaunch (its format-2 settings
    /// raising nothing, so the marker names the set) finishes the job:
    /// every plugin finds its own knobs, nothing is left half-written, the
    /// marker is gone, and no legacy sidecar was touched by either run.
    #[test]
    fn a_kill_at_every_point_of_the_conversion_relaunches_whole() {
        let (_, bays, plugins) = v12();
        let marker = r#"{"settings_version": 12, "shuffles": ["v13", "v14", "v18", "v19", "v20"]}"#;
        let mut writes = 0;
        loop {
            let dir = scratch("kill-every");
            seed(&dir, &bays);
            let before = legacy_files(&dir);
            std::fs::write(dir.join(SHELF_PENDING_NAME), marker).expect("the marker");
            KILL_AFTER.with(|k| k.set(Some(writes)));
            let killed = std::panic::catch_unwind(|| convert_shelf_in(&dir, ShelfShuffles::NONE)).is_err();
            KILL_AFTER.with(|k| k.set(None));
            if killed {
                assert!(!dir.join(SHELF_DONE_NAME).exists(), "write {writes}: a killed conversion was marked done");
                let done = convert_shelf_in(&dir, ShelfShuffles::NONE);
                assert!(done.ran, "write {writes}: the relaunch did not finish the conversion");
            }
            every_state_found(&format!("v12 killed at write {writes}"), &dir, &plugins, &bays);
            assert!(dir.join(SHELF_DONE_NAME).is_file(), "write {writes}: not marked done");
            assert!(!dir.join(SHELF_PENDING_NAME).exists(), "write {writes}: the marker outlived the conversion");
            let torn: Vec<String> = files(&dir).into_keys().filter(|n| n.ends_with(".tmp")).collect();
            assert!(torn.is_empty(), "write {writes}: {torn:?} left half-written");
            assert_eq!(legacy_files(&dir), before, "write {writes}: a legacy sidecar was touched");
            let _ = std::fs::remove_dir_all(&dir);
            if !killed {
                break;
            }
            writes += 1;
        }
        assert!(writes > 4, "fixture: the conversion made only {writes} writes");
    }

    /// **A copy that FAILS leaves the conversion owed, and the next launch
    /// finishes it** (design 6.6: every state reaches its V32 seat, and
    /// `.shelf-v23` is the word that none is left to copy). A kill ends the
    /// run; a failure lets it carry on, which is exactly when it could
    /// still have called itself done. So each failure is injected at each
    /// point it can strike, and the run goes on past it: every write in
    /// turn (a temp that will not rename, the copies and `.shelf-v23`
    /// alike), then every read of a legacy file, with the set coming from a
    /// standing marker and from the launch alone.
    ///
    /// The failed run leaves no `.shelf-v23`, a marker naming the set it
    /// copied through (the one that stood, untouched, or one it wrote from
    /// the launch's set, because the relaunch's format-2 settings raise
    /// none), no temp file and every legacy file as it was. The relaunch
    /// copies what is missing and marks the conversion done, and every
    /// plugin finds its own knobs.
    #[test]
    fn a_failed_copy_leaves_the_conversion_owed_and_the_next_launch_finishes_it() {
        let (_, bays, plugins) = v12();
        let set = ShelfShuffles::from_names(["v13", "v14", "v18", "v19", "v20"]).expect("the names");
        let marker = r#"{"settings_version": 12, "shuffles": ["v13", "v14", "v18", "v19", "v20"]}"#;
        let names = |dir: &Path| -> Option<Vec<String>> {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(SHELF_PENDING_NAME)).ok()?).ok()?;
            Some(v["shuffles"].as_array()?.iter().filter_map(|n| n.as_str().map(str::to_owned)).collect())
        };
        let mut struck_any = (0, 0);
        for from_marker in [true, false] {
            for (what, key) in [("write", &FAIL_WRITE), ("read", &FAIL_READ)] {
                let mut at = 0;
                loop {
                    let tag = format!("{what} {at}, the set from {}", if from_marker { "the marker" } else { "the launch" });
                    let dir = scratch("fail-copy");
                    seed(&dir, &bays);
                    let before = legacy_files(&dir);
                    if from_marker {
                        std::fs::write(dir.join(SHELF_PENDING_NAME), marker).expect("the marker");
                    }
                    key.with(|k| k.set(Some(at)));
                    let done = convert_shelf_in(&dir, if from_marker { ShelfShuffles::NONE } else { set });
                    let struck = key.with(std::cell::Cell::take).is_none();
                    if struck {
                        if what == "write" {
                            struck_any.0 += 1;
                        } else {
                            struck_any.1 += 1;
                        }
                        assert!(done.ran && done.owed, "{tag}: a failed run called itself finished: {done:?}");
                        // A read that fails, or a copy whose write does, is
                        // one copy owed; the one write that is not a copy is
                        // `.shelf-v23` itself.
                        assert!(done.failed == 1 || (what == "write" && done.failed == 0), "{tag}: {done:?}");
                        assert!(!dir.join(SHELF_DONE_NAME).exists(), "{tag}: the conversion was marked done over a failure");
                        assert_eq!(
                            names(&dir).as_deref(),
                            Some(&set.names().into_iter().map(str::to_owned).collect::<Vec<_>>()[..]),
                            "{tag}: no marker names the set the relaunch must copy through"
                        );
                        if from_marker {
                            assert_eq!(
                                std::fs::read_to_string(dir.join(SHELF_PENDING_NAME)).expect("the marker"),
                                marker,
                                "{tag}: the standing marker was rewritten"
                            );
                        }
                        let torn: Vec<String> = files(&dir).into_keys().filter(|n| n.ends_with(".tmp")).collect();
                        assert!(torn.is_empty(), "{tag}: {torn:?} left behind");
                        assert_eq!(legacy_files(&dir), before, "{tag}: a legacy sidecar was touched");
                        // The relaunch: its format-2 settings raise nothing.
                        // It copies exactly the copy that was owed (the
                        // failed bay had no file, or copy-if-absent would
                        // have kept it) and nothing else.
                        let again = convert_shelf_in(&dir, ShelfShuffles::NONE);
                        assert!(again.ran && !again.owed && again.failed == 0, "{tag}: the relaunch did not finish: {again:?}");
                        assert_eq!(again.copied, done.failed, "{tag}: the relaunch did not copy exactly what was owed");
                    }
                    every_state_found(&tag, &dir, &plugins, &bays);
                    assert!(dir.join(SHELF_DONE_NAME).is_file(), "{tag}: not marked done");
                    assert!(!dir.join(SHELF_PENDING_NAME).exists(), "{tag}: the marker outlived the conversion");
                    let torn: Vec<String> = files(&dir).into_keys().filter(|n| n.ends_with(".tmp")).collect();
                    assert!(torn.is_empty(), "{tag}: {torn:?} left half-written");
                    assert_eq!(legacy_files(&dir), before, "{tag}: a legacy sidecar was touched");
                    let _ = std::fs::remove_dir_all(&dir);
                    if !struck {
                        break;
                    }
                    at += 1;
                }
            }
        }
        assert!(struck_any.0 > 8 && struck_any.1 > 4, "fixture: too few failures struck: {struck_any:?}");
    }

    /// **No new-build path renames or deletes a legacy sidecar** (design
    /// 6.6, the scan half; every test above audits the files). The legacy
    /// names exist in exactly one place, this module, which only READS
    /// them: every `remove_file` and `rename` in it acts on an identity name,
    /// a temp or a marker (the conversion's pending one, or the record of a
    /// mirror a save road owes); and no other host source spells the legacy
    /// prefix except the template copy that SKIPS such files.
    #[test]
    fn no_new_build_path_renames_or_deletes_a_legacy_sidecar() {
        let me = include_str!("shelf.rs");
        let me = &me[..me.find("\n#[cfg(test)]\nmod tests {").expect("this module")];
        for (at, _) in me.match_indices("remove_file(").chain(me.match_indices("rename(")) {
            let line = me[..at].rfind('\n').map_or(0, |n| n + 1);
            let end = me[at..].find('\n').map_or(me.len(), |n| at + n);
            let call = &me[line..end];
            assert!(
                ["state_file_name(", "&tmp", "&pending", "&owed", "tmp, path"].iter().any(|ok| call.contains(ok)),
                "shelf.rs removes or renames something that may be a legacy sidecar: `{}`",
                call.trim()
            );
        }
        for (at, _) in me.match_indices("legacy_name(") {
            let line = &me[me[..at].rfind('\n').unwrap_or(0)..me[at..].find('\n').map_or(me.len(), |n| at + n)];
            assert!(
                ["std::fs::read(", "dir.join(", "fn legacy_name", "== name"].iter().any(|ok| line.contains(ok)),
                "a legacy name is built for something other than reading it: `{}`",
                line.trim()
            );
        }
        for (file, src) in [
            ("desktop.rs", include_str!("desktop.rs")),
            ("engine_bridge.rs", include_str!("engine_bridge.rs")),
            ("main.rs", include_str!("main.rs")),
        ] {
            let src = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
            // Code only: a comment may name the files it is about.
            let src: String = src.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n");
            let spelled = src.matches("insert-state-").count();
            let skipped = src.matches("name.starts_with(\"insert-state-\")").count();
            assert_eq!(spelled, skipped, "{file} spells a legacy sidecar name for something but skipping it");
            assert!(!src.contains("fn shift_insert_states_for_v"), "{file} renames the shelf in place again");
        }
    }

    /// **A format-2 project without the marker ignores stray legacy files**
    /// (design 6.6): its `state/` is read by identity alone, so an
    /// `insert-state-*` file somebody's older build dropped beside it is
    /// neither read for any bay nor touched.
    #[test]
    fn a_format_2_project_without_the_marker_ignores_stray_legacy_files() {
        let state = scratch("stray-state");
        let own = SeatBay::new(Node::Track(tangent_ui::desk_id::TrackId(0)), Rack::Chain(0), 0);
        assert!(write_state_in(&state, own, "/x/A.vst3", b"its own"), "a state file");
        // A stray at the legacy name of the very same bay, and one of a bay
        // the project does not hold.
        std::fs::write(state.join(legacy_name(0, 0)), b"/x/A.vst3\0a stranger's").expect("seed");
        std::fs::write(state.join(legacy_name(5, 1)), b"/x/B.vst3\0another stranger's").expect("seed");
        let before = files(&state);
        let live = scratch("stray-live");
        mirror_shelf(&state, &ShelfNames::for_marker(None), &live);
        assert_eq!(read_state(&live, own, "/x/A.vst3").as_deref(), Some(&b"its own"[..]));
        assert_eq!(
            shelf_states(&live),
            [own].into_iter().collect(),
            "a stray legacy file was read into a bay"
        );
        assert_eq!(files(&state), before, "the project's state/ was touched");
        let _ = std::fs::remove_dir_all(&state);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// A project folder with a v8 document of `text`'s desk in it (the
    /// marker included), its `state/` seeded with the legacy sidecars the
    /// legacy document's plugins were saved in, a file of the user's own and
    /// a drumpad sidecar. Answers the document's path, its plugins and the
    /// legacy bays.
    fn a_legacy_project(tag: &str, text: &str) -> (PathBuf, BTreeMap<SeatBay, String>, BTreeMap<usize, String>) {
        let raw: Value = serde_json::from_str(text).expect("json");
        let bays = legacy_bays(&raw["desk"], "inserts", "inserts_tail");
        let project = scratch(tag);
        let state = project.join("state");
        std::fs::create_dir_all(&state).expect("state/");
        std::fs::create_dir_all(project.join("media")).expect("media/");
        std::fs::write(project.join("media").join("take.wav"), b"RIFF and a take").expect("a take");
        std::fs::write(project.join("notes.txt"), b"mine").expect("a file of the user's own");
        std::fs::write(project.join(tangent_ui::drumpad::FILE_NAME), b"{\"lane_kits\": {}}").expect("a kit");
        seed(&state, &bays);
        let doc = tangent_ui::arrangement::doc_in(&project);
        let a = Arrangement::from_json(text);
        a.save_to(&doc);
        let plugins = a.desk.as_ref().expect("a desk").desk_model().model.plugins;
        (doc, plugins, bays)
    }

    /// **The marker clears only after a verified mirror, and its patch
    /// changes only that key** (design 6.6). Over a project whose `state/`
    /// holds only its legacy sidecars, the clear refuses: the document on
    /// disk names plugins with no identity file yet. A mirror that leaves
    /// one plugin out still refuses. Once every plugin that keeps a state
    /// has its own file there (the engine's own instruments keep none, and
    /// are not waited for), the clear takes the one key out of the file:
    /// the document is the same JSON without `desk.legacy_states`, and the
    /// same TEXT but for that one line.
    #[test]
    fn the_marker_clears_only_after_a_verified_mirror_and_its_patch_changes_only_that_key() {
        let (_, text) = DOCUMENTS[3];
        let (doc, plugins, _) = a_legacy_project("marker", text);
        let state = doc.parent().expect("the project").join("state");
        let marked = std::fs::read_to_string(&doc).expect("the document");
        assert!(marked.contains("\"legacy_states\": \"v24\""), "fixture: the saved document carries no marker");
        let on_disk = doc_plugins(&doc).expect("a format-2 document");
        assert_eq!(on_disk, plugins, "fixture: the document on disk names other plugins");

        assert!(!clear_legacy_marker(&doc, &on_disk), "the marker cleared over a folder of legacy names only");
        assert_eq!(std::fs::read_to_string(&doc).expect("doc"), marked, "a refused clear wrote the document");

        // The live shelf, holding every plugin's state but one's.
        let live = scratch("marker-live");
        let keeps = stateful(&on_disk);
        assert!(keeps.len() > 1, "fixture: one plugin to keep a state");
        for (sb, bundle) in &keeps[1..] {
            assert!(write_state_in(&live, *sb, bundle, b"live"), "a live state");
        }
        mirror_shelf(&live, &ShelfNames::Pstate, &state);
        assert!(!clear_legacy_marker(&doc, &on_disk), "the marker cleared with a plugin missing its file");
        assert_eq!(std::fs::read_to_string(&doc).expect("doc"), marked, "a refused clear wrote the document");

        // A state for the one left out, owned by somebody else: still no.
        let (sb0, bundle0) = &keeps[0];
        assert!(write_state_in(&live, *sb0, "/x/Somebody else.vst3", b"theirs"), "a state");
        mirror_shelf(&live, &ShelfNames::Pstate, &state);
        assert!(!clear_legacy_marker(&doc, &on_disk), "a file owned by another plugin passed for its own");

        assert!(write_state_in(&live, *sb0, bundle0, b"live"), "a state");
        mirror_shelf(&live, &ShelfNames::Pstate, &state);
        assert!(clear_legacy_marker(&doc, &on_disk), "a whole folder kept its marker");
        let cleared = std::fs::read_to_string(&doc).expect("doc");
        let mut want: Value = serde_json::from_str(&marked).expect("json");
        want["desk"].as_object_mut().expect("desk").shift_remove("legacy_states");
        assert_eq!(serde_json::from_str::<Value>(&cleared).expect("json"), want, "the patch moved more than the marker");
        let line = marked.lines().find(|l| l.contains("\"legacy_states\"")).expect("the marker's line");
        let without: Vec<&str> = marked.lines().filter(|l| *l != line).collect();
        assert_eq!(cleared.lines().collect::<Vec<_>>(), without, "the patch rewrote more than the marker's line");
        // Cleared is cleared: asked again, it says so and writes nothing.
        assert!(clear_legacy_marker(&doc, &on_disk));
        assert_eq!(std::fs::read_to_string(&doc).expect("doc"), cleared);
        let _ = std::fs::remove_dir_all(doc.parent().expect("the project"));
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **A project folder is untouched by an open, a mirror and a save,
    /// apart from its identity state files and the marker patch** (design
    /// 6.6): the open reads `state/` onto the live shelf; the save road's
    /// mirror writes the live states back by identity and clears the marker.
    /// Every file that was there before is byte for byte as it was (the
    /// legacy sidecars, the take, the kit, the user's own file), the
    /// document differs by the marker alone, and nothing new is there but
    /// `state/`'s identity files.
    #[test]
    fn a_folder_is_untouched_by_open_mirror_and_save_but_for_its_states_and_marker() {
        for (name, text) in DOCUMENTS {
            let (doc, plugins, bays) = a_legacy_project("untouched", text);
            let project = doc.parent().expect("the project").to_path_buf();
            let tree = |root: &Path| -> BTreeMap<String, Vec<u8>> {
                let mut out = files(root);
                for sub in ["state", "media"] {
                    out.extend(files(&root.join(sub)).into_iter().map(|(n, b)| (format!("{sub}/{n}"), b)));
                }
                out
            };
            let before = tree(&project);
            let marked: Value = serde_json::from_slice(&before[tangent_ui::arrangement::DOC_NAME]).expect("json");
            let era = Arrangement::load_from(&doc).desk.and_then(|d| d.legacy_states.0).expect("the marker");
            // The open: `state/` onto the live shelf.
            let live = scratch("untouched-live");
            mirror_shelf(&project.join("state"), &ShelfNames::for_marker(Some(era)), &live);
            every_state_found(name, &live, &plugins, &bays);
            assert_eq!(tree(&project), before, "{name}: the open touched the project folder");
            // The save road: the live shelf back into `state/`, then the
            // marker, verified against the document on disk.
            mirror_shelf(&live, &ShelfNames::Pstate, &project.join("state"));
            let on_disk = doc_plugins(&doc).expect("a format-2 document");
            assert!(clear_legacy_marker(&doc, &on_disk), "{name}: a whole folder kept its marker");
            let after = tree(&project);
            for (file, bytes) in &before {
                if file == tangent_ui::arrangement::DOC_NAME {
                    continue;
                }
                assert_eq!(after.get(file), Some(bytes), "{name}: {file} was changed or removed");
            }
            for file in after.keys().filter(|f| !before.contains_key(*f)) {
                let state = file.strip_prefix("state/").and_then(parse_state_file_name);
                assert!(state.is_some(), "{name}: the save wrote `{file}` into the project");
            }
            let mut want = marked;
            want["desk"].as_object_mut().expect("desk").shift_remove("legacy_states");
            let doc_now: Value = serde_json::from_slice(&after[tangent_ui::arrangement::DOC_NAME]).expect("json");
            assert_eq!(doc_now, want, "{name}: the document moved by more than its marker");
            let _ = std::fs::remove_dir_all(&project);
            let _ = std::fs::remove_dir_all(&live);
        }
    }

    /// **A kill between a NEW home's document and its mirror reopens with
    /// every state** (W4-integrate, done-when "Save-road crashes", for the
    /// two roads that move a session to a new folder: Save As and the first
    /// save). Their folder holds no `state/` until the host mirrors the live
    /// shelf into it after the document lands, and the relaunch replaces
    /// that shelf with the template's before anything can be opened, so a
    /// kill between the two used to leave every plugin of the new project at
    /// its defaults. The host writes the owed mirror down first
    /// (`DesktopApp::owe_states_to_new_home`), to the home the app says the
    /// save will choose (`TangentApp::save_choice_dir`), and the relaunch
    /// pays it before it opens anything (`pay_owed_mirror`).
    ///
    /// Driven through the app's own saves, each killed after its document
    /// lands: Save As of a legacy project into a new folder and into an
    /// existing one (the wrap), the first save of a kept project (a copy),
    /// of an unnamed scratch that is real (moved, `state/` and all), and of
    /// one never written (born at the chosen place). Each time the folder
    /// the app chose is the one `save_choice_dir` named, the relaunch
    /// mirrors into it, the marker goes when the folder is whole, and every
    /// plugin of the reopened document finds the knobs it was PLAYING (the
    /// live shelf's, not the last save's). A road killed before its document
    /// lands owes nothing: the relaunch writes nothing into the folder,
    /// which here holds somebody else's project.
    #[test]
    fn a_kill_between_a_new_homes_document_and_its_mirror_reopens_with_every_state() {
        use tangent_ui::ports::SavePurpose;
        #[derive(Clone, Copy, Debug)]
        enum Road {
            SaveAs,
            SaveAsWrapped,
            FirstKept,
            FirstMoved,
            FirstNeverWritten,
        }
        let ctx = egui::Context::default();
        let (_, text) = DOCUMENTS[3];
        for road in [Road::SaveAs, Road::SaveAsWrapped, Road::FirstKept, Road::FirstMoved, Road::FirstNeverWritten] {
            let tag = format!("{road:?}");
            let (doc, _, _) = a_legacy_project("new-home", text);
            let root = doc.parent().expect("the project").to_path_buf();
            let mut app = tangent_ui::app::TangentApp::with_license(
                &ctx,
                Settings::default(),
                Caps::PLUGIN,
                crate::composite::tiers::free("shelf"),
            );
            match road {
                Road::FirstKept => {
                    // A kept project: its document says somebody saved it.
                    let mut a = Arrangement::load_from(&doc);
                    a.named = true;
                    a.save_to(&doc);
                    app.open_project(&doc);
                }
                Road::FirstNeverWritten => {
                    // A scratch adopted from the project as a stencil,
                    // aimed at a folder that does not exist yet.
                    app.adopt_template(&root, &root.join("never-written"));
                }
                _ => app.open_project(&doc),
            }
            // The live shelf: what every plugin of the session is PLAYING.
            let live = scratch("new-home-live");
            let playing = crate::engine_bridge::desk_bays(&app);
            let keeps = stateful(&playing);
            assert!(keeps.len() > 1, "{tag}: fixture: no plugin keeps a state");
            for (sb, bundle) in &keeps {
                assert!(write_state_in(&live, *sb, bundle, format!("live {sb}").as_bytes()), "a live state");
            }
            let away = scratch("new-home-target");
            let (purpose, target) = match road {
                Road::SaveAs => (SavePurpose::ProjectCopy, away.join("Copy")),
                // An existing folder: the save wraps the project in its own.
                Road::SaveAsWrapped => (SavePurpose::ProjectCopy, away.clone()),
                _ => (SavePurpose::ProjectFirst, away.join("Kept.tan")),
            };
            // The road, as `owe_states_to_new_home` and the chore run it.
            let home = app.save_choice_dir(purpose, &target).unwrap_or_else(|| panic!("{tag}: no home"));
            assert!(owe_mirror(&live, &home), "{tag}: the owed mirror was not written down");
            app.finish_save_choice(purpose, &target);
            // Killed here: no mirror, no clear.
            let landed = app.project_doc().and_then(Path::parent).map(Path::to_path_buf);
            assert_eq!(landed.as_deref(), Some(home.as_path()), "{tag}: the save chose another home than it said");
            assert!(tangent_ui::arrangement::doc_in(&home).is_file(), "{tag}: the document did not land");
            let unprotected = shelf_states(&home.join("state"));
            assert!(
                keeps.iter().any(|(sb, _)| !unprotected.contains(sb)),
                "{tag}: fixture: the new home already held every state before the relaunch"
            );
            // The relaunch.
            assert_eq!(pay_owed_mirror(&live), Some(home.clone()), "{tag}: the relaunch paid no mirror");
            assert!(!live.join(MIRROR_OWED_NAME).exists(), "{tag}: the owed mirror outlived its payment");
            let reopened = Arrangement::load_from(&tangent_ui::arrangement::doc_in(&home));
            let desk = reopened.desk.as_ref().expect("a desk");
            assert_eq!(desk.legacy_states.0, None, "{tag}: a whole folder kept its marker");
            let back = scratch("new-home-back");
            mirror_shelf(&home.join("state"), &ShelfNames::for_marker(desk.legacy_states.0), &back);
            for (sb, bundle) in stateful(&desk.desk_model().model.plugins) {
                assert_eq!(
                    read_state(&back, sb, &bundle),
                    Some(format!("live {sb}").into_bytes()),
                    "{tag}: {sb} ({bundle}) reopened without the knobs it was playing"
                );
            }
            for dir in [&root, &live, &away, &back] {
                let _ = std::fs::remove_dir_all(dir);
            }
        }

        // Killed BEFORE the document lands: nothing is owed, and a folder
        // holding somebody else's project is left as it was.
        let (doc, _, _) = a_legacy_project("new-home-early", text);
        let theirs = doc.parent().expect("their project").to_path_buf();
        let before = files(&theirs.join("state"));
        let live = scratch("new-home-early-live");
        assert!(write_state_in(&live, SeatBay::new(Node::Master, Rack::Chain(0), 0), "/x/m.vst3", b"live"), "a state");
        assert!(owe_mirror(&live, &theirs), "the owed mirror was not written down");
        assert_eq!(pay_owed_mirror(&live), None, "a mirror was paid for a document the road never wrote");
        assert_eq!(files(&theirs.join("state")), before, "the relaunch wrote into a folder the save never reached");
        assert!(!live.join(MIRROR_OWED_NAME).exists(), "the record outlived the relaunch");
        let _ = std::fs::remove_dir_all(&theirs);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **A kill between the document write and the mirror reopens with
    /// every state** (W4-integrate, done-when "Save-road crashes"). Every
    /// save road writes the document through the app (`save_project_doc`:
    /// the host's `save_project_and_states`, `switch_project`, the New arm,
    /// `on_exit`, and the app's own Save of the save prompt), and that write
    /// is format 2 now: it must carry the document's `legacy_states` marker,
    /// because the legacy sidecars are all `state/` holds until the mirror
    /// runs. Here the app opens each era's project, saves it, and is killed
    /// before any mirror; the reopen finds the marker on the format-2 file
    /// and every plugin its own knobs, read by the era's legacy names.
    /// (`seed_template`'s heal patches one key of the file in place and
    /// keeps its format: `tangent-ui`'s own test holds that.)
    #[test]
    fn a_kill_between_the_document_and_the_mirror_reopens_with_every_state() {
        let ctx = egui::Context::default();
        for (name, text) in DOCUMENTS {
            let (doc, plugins, bays) = a_legacy_project("kill-save", text);
            // The project as the legacy build left it: its legacy document.
            std::fs::write(&doc, text).expect("the legacy document");
            let mut app = tangent_ui::app::TangentApp::with_license(
                &ctx,
                Settings::default(),
                Caps::PLUGIN,
                crate::composite::tiers::free("shelf"),
            );
            app.open_project(&doc);
            let era = app.doc_legacy_states().unwrap_or_else(|| panic!("{name}: the open lost the era"));
            app.save_project_doc();
            // Killed here: no mirror, no clear.
            let saved: Value = serde_json::from_str(&std::fs::read_to_string(&doc).expect("doc")).expect("json");
            assert!(tangent_ui::desk_format::is_format2(&saved["desk"]), "{name}: the save did not write format 2");
            let reopened = Arrangement::load_from(&doc);
            let desk = reopened.desk.as_ref().expect("a desk");
            assert_eq!(desk.legacy_states.0, Some(era), "{name}: the save dropped the marker");
            let live = scratch("kill-save-live");
            mirror_shelf(&doc.parent().expect("project").join("state"), &ShelfNames::for_marker(desk.legacy_states.0), &live);
            let checked = every_state_found(name, &live, &desk.desk_model().model.plugins, &bays);
            assert_eq!(checked, stateful(&plugins).len(), "{name}: the save moved a plugin");
            let _ = std::fs::remove_dir_all(doc.parent().expect("project"));
            let _ = std::fs::remove_dir_all(&live);
        }
    }
}
