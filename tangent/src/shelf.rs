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
    let _ = mirror_whole(from, names, to);
}

/// [`mirror_shelf`], answering whether the mirror is WHOLE: every state
/// `from` holds landed in `to`, and every stale one there is gone. What a
/// save road asks before it may call its debt paid ([`mirror_settled`]).
pub fn mirror_whole(from: &Path, names: &ShelfNames, to: &Path) -> bool {
    // BOTH read before either is written, so the second read is of the
    // shelf as it arrived rather than of the copies just made.
    let src = readable_states(from, names);
    let dst = shelf_states(to);
    let mut whole = true;
    for (sb, path) in &src {
        fs_call();
        whole &= std::fs::read(path).is_ok_and(|bytes| write_atomic(&to.join(state_file_name(*sb)), &bytes));
    }
    for sb in dst.iter().filter(|sb| !src.contains_key(sb)) {
        fs_call();
        step();
        whole &= std::fs::remove_file(to.join(state_file_name(*sb))).is_ok() || !to.join(state_file_name(*sb)).is_file();
    }
    whole
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

/// **The mirrors save roads owe a project's `state/`**, one folder each
/// beside the live shelf (see [`owe_mirror`]): `<seq>/`, holding the record
/// ([`OWED_RECORD`]: the home, what its document was before the road wrote
/// it, and what the road leaves it as) and the states the road owes it, as
/// the live shelf held them when the debt was written down. Paid in `seq`
/// order, oldest first, so the newest debt to a home is the last word in
/// its `state/`; and a whole mirror of the live shelf into a home is newer
/// than every debt to it, and takes them all ([`mirror_into_home`]).
pub const OWED_DIR: &str = ".states-owed";

/// The record in a debt's folder.
pub const OWED_RECORD: &str = "owed.json";

/// **A debt as the first journal wrote it** (W4-integrate): one record
/// beside the live shelf, owing the live shelf itself. Read at launch, the
/// way that journal paid it, and never written again: a debt now carries
/// its own states (see [`owe_mirror`] for why).
const SINGLE_OWED_NAME: &str = ".states-owed.json";

/// **One mirror a save road owes**, written down before its document lands:
/// its folder, to be settled by the same road once it has mirrored
/// ([`mirror_settled`]).
#[derive(Debug)]
#[must_use = "a debt is settled by the road that wrote it down"]
pub struct OwedMirror {
    dir: PathBuf,
}

/// **What a save road leaves its home's document as**, written down with
/// the debt it owes the home's `state/` ([`owe_mirror`]), so the settling
/// knows the road's document when it sees it.
///
/// The document the road FOUND tells a road that wrote from one that never
/// got there only when the write changed the bytes, and often it does not:
/// a knob turned in a plugin's own window never dirties the document (the
/// snapshot names bundles, not their insides), so Ctrl+S after one rewrites
/// the very bytes it found. A launch after a kill read that as a road that
/// never wrote and let the debt go, the knobs it held with it, and the
/// template then replaced the live shelf: the project reopened on the
/// knobs of the save before. So the road says what it leaves, before it
/// writes anything.
#[derive(Clone, Copy, Debug)]
pub enum RoadDoc<'a> {
    /// The road writes the home's document: the bytes it writes.
    Writes(&'a [u8]),
    /// The road writes no document (the save-back of a session with
    /// nothing unsaved, a quit, Reset Template): the one in the home
    /// stands, and the mirror alone is owed.
    Stands,
}

/// **Write down, BEFORE a save road's document lands in `home`, that the
/// live shelf in `shelf` is owed to `home`'s `state/`** (design 6.6: "None
/// of them can leave a plugin at defaults").
///
/// Every save road writes the document first and mirrors the live shelf
/// into the project's `state/` after it: Save As and the first save into a
/// folder that has no `state/` yet, Save and the save-backs of an Open, a
/// New and a quit's prompt in place, where `state/` has nothing for a
/// plugin seated since the last save. A process killed between the two
/// left those plugins at their defaults, with the knobs on a live shelf the
/// next launch replaces with the template's before anybody can open
/// anything. And the roads that write no document mirror all the same (the
/// knobs never dirty it): a kill partway through that mirror left `state/`
/// holding some plugins' new knobs and the rest's old ones.
///
/// So the road writes the debt first: the home, what its document was
/// before the road (its bytes' fingerprint, or nothing), what the road
/// leaves it as (`doc`, the same fingerprint), and **the states
/// themselves**, the live shelf's files linked (or, where the volume cannot
/// link, copied) into the debt's own folder, which is built aside and
/// renamed into place whole. The launch that finds the debt
/// ([`pay_owed_mirror`]) can then tell a document the road wrote from one
/// it never reached, and pays from the debt's own files, never from
/// whatever the live shelf holds by then: a debt that has to wait (its
/// home on a volume that is away, a mirror that did not land) waits with
/// the knobs it is owed, across any number of sessions in between. A link
/// costs no bytes, and stays the old file when the live one is replaced,
/// because every write to a shelf is temp and rename.
///
/// What an earlier road still owes `home` is settled first
/// ([`settle_owed`]): this road's mirror is newer, and a debt paid after it
/// would put older knobs over it. A home that cannot be reached (its
/// folder's folder is not there: a volume unmounted) takes no document from
/// this road either, and is owed nothing; nor is a road that writes no
/// document owed anything by a home that has none (there is no project
/// there to open). `None` when nothing was written down.
pub fn owe_mirror(shelf: &Path, home: &Path, doc: RoadDoc<'_>) -> Option<OwedMirror> {
    if !reachable(home) {
        return None;
    }
    settle_owed(shelf, Some(home));
    // After the settling, which may have patched the marker out of the
    // document it found.
    let before = doc_fingerprint(home);
    let after = match doc {
        RoadDoc::Writes(bytes) => fingerprint(bytes),
        RoadDoc::Stands => before.clone()?,
    };
    write_debt(shelf, home, before.as_deref(), Some(&after), shelf)
}

/// **The road has mirrored**: a WHOLE mirror (every state landed, every
/// stale one gone) takes its debt away (the mirror itself took it already,
/// with every older debt to its home, when it was [`mirror_into_home`]).
/// Any other leaves the debt owed, with its states, for the next launch or
/// the next open of the project to pay; taking it away would drop the
/// mirror in silence. `debt` is the one [`owe_mirror`] answered (`None`:
/// nothing was owed).
pub fn mirror_settled(debt: Option<OwedMirror>, whole: bool) {
    let Some(debt) = debt else { return };
    if !whole {
        log::warn!(
            "plugin states: a save's mirror did not land whole; owed until the next launch ({})",
            debt.dir.display()
        );
        return;
    }
    let_go(&debt.dir);
}

/// **The live shelf into `home`'s `state/`, for a save road**: the mirror
/// every road makes, by identity ([`mirror_whole`]), answering whether it
/// is whole; and a whole one takes every debt still owed to `home` with
/// it. `home` is a folder that is there: none is ever made for a shelf
/// alone, and a mirror into one that is not lands nothing.
///
/// Every debt is older than a mirror of the live shelf the session makes
/// after it, and once a whole one has landed, `state/` holds the session's
/// newest knobs exactly; a debt left standing is paid over them at the
/// next launch or open, the older knobs back and the state of a plugin
/// seated since removed as stale. Debts were left standing: one kept
/// because its own mirror did not land whole (a write refused for a
/// moment, a file a sync client held) outlived every later whole mirror
/// into the same folder, because a road's settling took its own debt and
/// no other, a road whose retry of the older one failed again owed its own
/// beside it, and a road that held no debt took nothing. So the mirror
/// clears them itself, whatever debt its road holds or could not write
/// down: every debt that stood when it began, and none written since,
/// which only another instance on the same shelf could have written.
pub fn mirror_into_home(shelf: &Path, home: &Path) -> bool {
    fs_call();
    if !home.is_dir() {
        return false;
    }
    let older = debt_folders(shelf);
    let state = home.join("state");
    let _ = std::fs::create_dir_all(&state);
    let whole = mirror_whole(shelf, &ShelfNames::Pstate, &state);
    if whole {
        supersede(&older, home);
    }
    whole
}

/// **Every debt in `older` owed to `home` goes**, oldest first: a whole
/// mirror of the live shelf, newer than each, has landed there
/// ([`mirror_into_home`]). A folder a kill left half-built is no debt, and
/// is left to the settling.
fn supersede(older: &[PathBuf], home: &Path) {
    for entry in older {
        if read_record(entry).is_some_and(|r| r.home == home) {
            let_go(entry);
        }
    }
}

/// **The folders on `shelf`'s [`OWED_DIR`]**, oldest first: every debt,
/// and every folder a kill left half-built.
fn debt_folders(shelf: &Path) -> Vec<PathBuf> {
    fs_call();
    let Ok(entries) = std::fs::read_dir(shelf.join(OWED_DIR)) else {
        return Vec::new();
    };
    let mut debts: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    debts.sort();
    debts
}

/// What paying the debts came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwedPayment {
    /// Homes whose `state/` now holds what was owed them.
    pub paid: Vec<PathBuf>,
    /// **Homes still owed**, their debts kept with their states: the home
    /// cannot be reached (a volume that is away), or its mirror did not
    /// land. Paid at a later launch or open, once it can be.
    pub kept: Vec<PathBuf>,
}

/// **The mirrors save roads owed and never made, made now** (see
/// [`owe_mirror`]): at launch, before anything replaces the live shelf.
///
/// A debt is paid only when the road's document LANDED (the home's
/// document is the one the road said it leaves, or is there and is not the
/// one it found: see [`landed`]), because a road killed before a write
/// that would have changed its document owes nothing, and a folder that
/// held somebody else's project must not be handed this session's states.
/// The payment is the mirror every save road makes, by identity, from the
/// debt's own states, and the document's `legacy_states` marker is then
/// cleared if the folder is whole, as `copy_states_to_project` clears it.
///
/// **A home that cannot be reached stays owed** (the owner: "must stay
/// owed, not be dropped, until it can be paid or the user discards the
/// project"): a project on a volume that is not mounted has no document to
/// look at, so whether its road wrote one cannot be told yet, and the debt
/// waits, states and all. Reachable again, it is paid, or let go when its
/// document is not the one the road wrote (the road never got there, or
/// the project was deleted since: discarded). A payment whose mirror does
/// not land whole is kept too.
///
/// A debt in the first journal's form (one record owing the live shelf) is
/// taken as that journal took it: its states are the live shelf's, which
/// at launch are still the ones the dying session wrote.
pub fn pay_owed_mirror(shelf: &Path) -> OwedPayment {
    adopt_single_record(shelf);
    settle_owed(shelf, None)
}

/// **The debts a session can pay while it runs**: [`pay_owed_mirror`]'s
/// settling, for the open of a project (a volume plugged back in, the
/// project on it opened before any relaunch). Never the first journal's
/// record, whose states are the live shelf's: mid-session, that shelf is
/// this session's.
pub fn pay_kept_mirrors(shelf: &Path) -> OwedPayment {
    settle_owed(shelf, None)
}

/// **Whether a road could write into `home`**: the folder is there, or the
/// folder it would be made in is. Neither, and the volume it lives on is
/// away (or it was deleted with the folder holding it).
fn reachable(home: &Path) -> bool {
    fs_call();
    home.is_dir() || home.parent().is_some_and(Path::is_dir)
}

/// **Whether the road's document landed in `home`**: there, and either the
/// document the road said it leaves (`after`) or any but the one it found
/// (`before`).
///
/// The first is what the fingerprint of `before` cannot see: a road whose
/// document is, byte for byte, the one it found (knobs alone turned since
/// the last save, or a road that writes no document at all). Whether the
/// kill came before its write or after it, the home holds exactly the
/// document the road leaves, and the debt is paid: what the road meant to
/// keep is kept. The second is how a debt that does not say (the first
/// journal's) is judged, and how a document the road wrote and something
/// changed since (the marker's patch) still is.
fn landed(home: &Path, before: Option<&str>, after: Option<&str>) -> bool {
    fs_call();
    if !tangent_ui::arrangement::doc_in(home).is_file() {
        return false;
    }
    let now = doc_fingerprint(home);
    now.as_deref() != before || (after.is_some() && now.as_deref() == after)
}

/// **Settle the debts on `shelf`** (every one, or `only` those owed to that
/// home), oldest first: each landed one is paid from its own states, each
/// whose home is reachable and whose document never landed is let go, and
/// the rest wait. A folder a kill left half-built (`.writing`, or no
/// record) is a debt whose road never reached its document, and goes.
///
/// **A home with a debt that waits takes no later debt this time**: the
/// later one is newer, and paid now and let go, it would leave the older
/// to be paid over it once that one can be. They wait together and are
/// paid in order.
fn settle_owed(shelf: &Path, only: Option<&Path>) -> OwedPayment {
    let mut out = OwedPayment::default();
    let mut waiting: BTreeSet<PathBuf> = BTreeSet::new();
    for entry in debt_folders(shelf) {
        let Some(Record { home, before, after }) = read_record(&entry) else {
            let_go(&entry);
            continue;
        };
        if only.is_some_and(|h| h != home) {
            continue;
        }
        let behind = waiting.contains(&home);
        let landed = !behind && landed(&home, before.as_deref(), after.as_deref());
        let paid = landed && {
            let state = home.join("state");
            let _ = std::fs::create_dir_all(&state);
            mirror_whole(&entry, &ShelfNames::Pstate, &state)
        };
        if paid {
            let doc = tangent_ui::arrangement::doc_in(&home);
            if let Some(plugins) = doc_plugins(&doc) {
                clear_legacy_marker(&doc, &plugins);
            }
            let_go(&entry);
            out.paid.push(home);
        } else if !behind && !landed && reachable(&home) {
            let_go(&entry);
        } else {
            waiting.insert(home.clone());
            out.kept.push(home);
        }
    }
    out
}

/// **A debt's record**: its home, and its document's fingerprint before the
/// road and after it ([`RoadDoc`]); `None` where it had none, or the record
/// does not say (the first journal's).
struct Record {
    home: PathBuf,
    before: Option<String>,
    after: Option<String>,
}

/// The record of the debt in `entry`: `None` for a folder that is not one
/// (a name other than a debt's own number, which a folder still being
/// built wears) or a record that will not read.
fn read_record(entry: &Path) -> Option<Record> {
    let name = entry.file_name()?.to_str()?;
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    fs_call();
    let v: Value = serde_json::from_str(&std::fs::read_to_string(entry.join(OWED_RECORD)).ok()?).ok()?;
    let said = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(Record { home: PathBuf::from(v.get("home")?.as_str()?), before: said("before"), after: said("after") })
}

/// **A debt's folder, built aside and renamed into place whole**: every
/// pstate file on `states` linked in (copied where it cannot be linked),
/// then the record. `None`, and nothing left behind, when any of it did not
/// land.
fn write_debt(shelf: &Path, home: &Path, before: Option<&str>, after: Option<&str>, states: &Path) -> Option<OwedMirror> {
    let owed = shelf.join(OWED_DIR);
    let seq = next_seq(&owed);
    let entry = owed.join(format!("{seq:010}"));
    let building = owed.join(format!("{seq:010}.writing"));
    let _ = std::fs::remove_dir_all(&building);
    fs_call();
    if std::fs::create_dir_all(&building).is_err() {
        return None;
    }
    let record = serde_json::json!({ "home": home.to_string_lossy(), "before": before, "after": after });
    let linked = shelf_states(states)
        .into_iter()
        .all(|sb| keep_state(&states.join(state_file_name(sb)), &building.join(state_file_name(sb))));
    let built = linked
        && serde_json::to_string_pretty(&record).is_ok_and(|text| write_atomic(&building.join(OWED_RECORD), text.as_bytes()))
        && {
            step();
            fs_call();
            std::fs::rename(&building, &entry).is_ok()
        };
    if !built {
        let _ = std::fs::remove_dir_all(&building);
        return None;
    }
    Some(OwedMirror { dir: entry })
}

/// **One state into a debt**: a link to the live file when the volume can
/// make one (no bytes, and still the old file after the live one is
/// replaced, temp and rename), a copy when it cannot.
fn keep_state(from: &Path, to: &Path) -> bool {
    step();
    fs_call();
    std::fs::hard_link(from, to).is_ok() || std::fs::read(from).is_ok_and(|bytes| write_atomic(to, &bytes))
}

/// The next debt's number: one past the highest on `owed`.
fn next_seq(owed: &Path) -> u64 {
    fs_call();
    std::fs::read_dir(owed)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str()?.split('.').next()?.parse::<u64>().ok())
                .max()
                .map_or(0, |n| n + 1)
        })
        .unwrap_or(0)
}

/// **A debt goes**: its folder, and with it the links it held (the live
/// shelf's files are untouched). When the folder will not go, its record
/// does, so what is left reads as a half-built debt and goes at the next
/// settling instead of being paid again.
fn let_go(entry: &Path) {
    step();
    fs_call();
    if std::fs::remove_dir_all(entry).is_err() && entry.exists() {
        let _ = std::fs::remove_file(entry.join(OWED_RECORD));
        log::warn!("plugin states: a settled debt could not be cleared ({})", entry.display());
    }
}

/// **The first journal's record, taken as that journal took it**: a debt
/// whose states are the live shelf's, which at launch are still the ones the
/// dying session wrote, so they go into a debt of their own before anything
/// replaces them. The record goes once the debt stands, or when it names no
/// home. Nothing writes one any more.
fn adopt_single_record(shelf: &Path) {
    let single = shelf.join(SINGLE_OWED_NAME);
    fs_call();
    let Ok(text) = std::fs::read_to_string(&single) else {
        return;
    };
    let record = serde_json::from_str::<Value>(&text).ok();
    let home = record.as_ref().and_then(|v| v.get("home")).and_then(Value::as_str).map(PathBuf::from);
    let before = record.as_ref().and_then(|v| v.get("before")).and_then(Value::as_str).map(str::to_owned);
    let kept = home.is_none_or(|home| write_debt(shelf, &home, before.as_deref(), None, shelf).is_some());
    if kept {
        step();
        fs_call();
        let _ = std::fs::remove_file(&single);
    }
}

/// **What `home`'s document is**, as a [`fingerprint`] of its bytes: `None`
/// when it has none.
fn doc_fingerprint(home: &Path) -> Option<String> {
    std::fs::read(tangent_ui::arrangement::doc_in(home)).ok().map(|bytes| fingerprint(&bytes))
}

/// **A document's bytes as a fingerprint** (FNV-1a, 64 bits, in hex). Only
/// ever compared with itself, across one save: the document a road found,
/// and the one it leaves.
fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
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

/// **One write of the journal that is not a temp and rename** (a link, a
/// folder renamed into place, a removal): where the crash tests may kill
/// the process too, before it. Nothing in a build.
#[inline]
fn step() {
    #[cfg(test)]
    kill_point();
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

    /// **The debts written down on `shelf`**, by their folders' names (a
    /// half-built one included).
    fn debts(shelf: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(shelf.join(OWED_DIR))
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        out.sort();
        out
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
    /// a temp or a marker (the conversion's pending one, a debt's record,
    /// the first journal's record), every `remove_dir_all` on a debt's own
    /// folder (`OWED_DIR`, which holds links and copies of identity files
    /// and its record, never a legacy sidecar), and a debt's folder is
    /// renamed only from where it was built; and no other host source
    /// spells the legacy prefix except the template copy that SKIPS such
    /// files.
    #[test]
    fn no_new_build_path_renames_or_deletes_a_legacy_sidecar() {
        let me = include_str!("shelf.rs");
        let me = &me[..me.find("\n#[cfg(test)]\nmod tests {").expect("this module")];
        let line_of = |at: usize| {
            let line = me[..at].rfind('\n').map_or(0, |n| n + 1);
            let end = me[at..].find('\n').map_or(me.len(), |n| at + n);
            &me[line..end]
        };
        for (at, _) in me.match_indices("remove_file(").chain(me.match_indices("rename(")) {
            let call = line_of(at);
            assert!(
                ["state_file_name(", "&tmp", "&pending", "OWED_RECORD", "&single", "&building", "tmp, path"]
                    .iter()
                    .any(|ok| call.contains(ok)),
                "shelf.rs removes or renames something that may be a legacy sidecar: `{}`",
                call.trim()
            );
        }
        for (at, _) in me.match_indices("remove_dir_all(") {
            let call = line_of(at);
            assert!(
                ["remove_dir_all(entry)", "remove_dir_all(&building)"].iter().any(|ok| call.contains(ok)),
                "shelf.rs removes a folder that may not be a debt's: `{}`",
                call.trim()
            );
        }
        let built = &me[me.find("fn write_debt(").expect("write_debt")..];
        let built = &built[..built.find("\n}\n").expect("its end")];
        assert!(
            built.contains("let entry = owed.join(") && built.contains("let building = owed.join("),
            "a debt's folder is built or renamed outside `OWED_DIR`"
        );
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
            let debt = owe_mirror(&live, &home, RoadDoc::Writes(app.save_choice_doc_text().as_bytes()));
            assert!(debt.is_some(), "{tag}: the owed mirror was not written down");
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
            assert_eq!(pay_owed_mirror(&live).paid, vec![home.clone()], "{tag}: the relaunch paid no mirror");
            assert!(debts(&live).is_empty(), "{tag}: the owed mirror outlived its payment");
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
        let ours = RoadDoc::Writes(b"the document the save would have written");
        assert!(owe_mirror(&live, &theirs, ours).is_some(), "the owed mirror was not written down");
        assert_eq!(pay_owed_mirror(&live), OwedPayment::default(), "a mirror was paid for a document the road never wrote");
        assert_eq!(files(&theirs.join("state")), before, "the relaunch wrote into a folder the save never reached");
        assert!(debts(&live).is_empty(), "the record outlived the relaunch");
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

    // ── W5-save-gaps: every save road's debt, and debts that wait ─────────

    /// The sessions a save road starts from: a kept project with a plugin
    /// seated since its last save (its document changes), the same project
    /// with knobs alone turned since (its document's bytes do not: a knob
    /// turned in a plugin's own window never dirties it), and a scratch
    /// whose folder was never written.
    #[derive(Clone, Copy, Debug)]
    enum Session {
        Kept,
        KeptKnobs,
        NeverWritten,
    }

    /// The bay a test plugin sits in: row `row`'s own rack, bay `bay`.
    fn chain_bay(app: &tangent_ui::app::TangentApp, row: usize, bay: u8) -> SeatBay {
        SeatBay::new(Node::Track(app.track_ids()[row]), Rack::Chain(0), bay)
    }

    /// **A session for a save road**, in `root`: answers the app and the
    /// folder it stands in. A kept project holds two plugins its last save
    /// wrote the states of (`saved <bay>`); a never-written scratch was
    /// adopted from a template holding them and has no folder yet. A third
    /// plugin has been seated since, whose state the project has nowhere:
    /// the one a kill between the document and the mirror used to leave at
    /// its defaults. Not where knobs alone were turned: that session's
    /// document is the one on disk, byte for byte.
    fn a_session(
        road: Session,
        ctx: &egui::Context,
        root: &Path,
    ) -> (tangent_ui::app::TangentApp, PathBuf) {
        let app = || {
            tangent_ui::app::TangentApp::with_license(ctx, Settings::default(), Caps::PLUGIN, crate::composite::tiers::free("shelf"))
        };
        let mut a = app();
        a.bootstrap_project(&root.join("Projects"));
        assert!(a.save_project_first(&root.join("Projects").join("Song.tan")), "fixture: no first save");
        a.set_plugin(chain_bay(&a, 0, 1), Some("/x/a.vst3"));
        a.set_plugin(chain_bay(&a, 1, 1), Some("/x/b.vst3"));
        a.save_project_doc();
        let song = root.join("Projects").join("Song");
        for (row, bundle) in [(0, "/x/a.vst3"), (1, "/x/b.vst3")] {
            let sb = chain_bay(&a, row, 1);
            assert!(write_state_in(&song.join("state"), sb, bundle, format!("saved {sb}").as_bytes()), "a saved state");
        }
        let (mut session, home) = match road {
            Session::Kept | Session::KeptKnobs => (a, song),
            Session::NeverWritten => {
                let mut fresh = app();
                let home = root.join("Projects").join("Untitled");
                fresh.adopt_template(&song, &home);
                assert!(!home.exists(), "fixture: the scratch's folder exists");
                (fresh, home)
            }
        };
        if matches!(road, Session::KeptKnobs) {
            assert_eq!(
                Some(session.session_doc_text().into_bytes()),
                std::fs::read(tangent_ui::arrangement::doc_in(&home)).ok(),
                "fixture: the session holds unsaved work"
            );
        } else {
            session.set_plugin(chain_bay(&session, 1, 2), Some("/x/c.vst3"));
        }
        (session, home)
    }

    /// **Every save road, by what it does to its home's document**: writes
    /// it in place (Save, and the save prompt's Save of a quit, a New or an
    /// Open, which is the host's Save: with a plugin seated since, or knobs
    /// alone turned and the bytes the same; the save-backs of an Open and a
    /// New, writing a never-written scratch for the first time), writes none
    /// and mirrors alone (the save-backs of an Open and a New with nothing
    /// unsaved, a quit, Reset Template), or writes it in a new home (Save
    /// As, the first save).
    #[derive(Clone, Copy, Debug)]
    enum SaveRoad {
        Save,
        SaveKnobs,
        SaveBack,
        MirrorAlone,
        SaveAs,
        FirstSave,
    }

    impl SaveRoad {
        /// The session the road starts from.
        fn session(self) -> Session {
            match self {
                Self::Save | Self::SaveAs => Session::Kept,
                Self::SaveKnobs | Self::MirrorAlone => Session::KeptKnobs,
                Self::SaveBack | Self::FirstSave => Session::NeverWritten,
            }
        }

        /// The new home's panel answer, for the roads that move.
        fn target(self, root: &Path) -> Option<(tangent_ui::ports::SavePurpose, PathBuf)> {
            use tangent_ui::ports::SavePurpose;
            match self {
                Self::SaveAs => Some((SavePurpose::ProjectCopy, root.join("Copy"))),
                Self::FirstSave => Some((SavePurpose::ProjectFirst, root.join("Projects").join("Kept.tan"))),
                Self::Save | Self::SaveKnobs | Self::SaveBack | Self::MirrorAlone => None,
            }
        }

        /// **The document the road writes**, as the host's road tells its
        /// debt (`TangentApp::session_doc_text`, `save_choice_doc_text`):
        /// `None` for the road that writes none.
        fn writes(self, app: &tangent_ui::app::TangentApp) -> Option<String> {
            match self {
                Self::MirrorAlone => None,
                Self::SaveAs | Self::FirstSave => Some(app.save_choice_doc_text()),
                Self::Save | Self::SaveKnobs | Self::SaveBack => Some(app.session_doc_text()),
            }
        }
    }

    /// **A save road, as the host runs it** (`save_project_and_states`, the
    /// save-backs in `switch_project` and New's arm, a quit's and Reset
    /// Template's mirror, the `SaveCopy` and `SaveFirst` chores): the knobs
    /// onto the live shelf (`save_plugin_states`), the debt saying what the
    /// road leaves (`owe_states_to_own_home`, `owe_states_beside_own_document`,
    /// `owe_states_to_new_home`), the app's document, the mirror and the
    /// marker (`copy_states_to_project`), the debt settled. A kill may land
    /// at any write of the shelf's and just before the document.
    fn save_road(road: SaveRoad, app: &mut tangent_ui::app::TangentApp, live: &Path, home: &Path, root: &Path) {
        for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(app)) {
            write_state_in(live, sb, &bundle, format!("live {sb}").as_bytes());
        }
        let writes = road.writes(app);
        let debt = owe_mirror(live, home, writes.as_deref().map_or(RoadDoc::Stands, |doc| RoadDoc::Writes(doc.as_bytes())));
        kill_point();
        match road.target(root) {
            Some((purpose, target)) => app.finish_save_choice(purpose, &target),
            None if writes.is_some() => app.save_project_doc(),
            None => {}
        }
        let whole = mirror_into_home(live, home);
        let doc = tangent_ui::arrangement::doc_in(home);
        if let Some(plugins) = doc_plugins(&doc) {
            clear_legacy_marker(&doc, &plugins);
        }
        mirror_settled(debt, whole);
    }

    /// **Somebody else's knobs on the live shelf**: what a later session, or
    /// the template a launch stands on, leaves there before a debt is paid.
    fn scramble(live: &Path) {
        for sb in shelf_states(live) {
            assert!(write_state_in(live, sb, "/x/a stranger.vst3", b"not yours"), "a stranger's state");
        }
    }

    /// Every half-written file under `dir`: a temp, or a debt half-built.
    fn torn(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for sub in [dir.to_path_buf(), dir.join("state"), dir.join(OWED_DIR)] {
            out.extend(files(&sub).into_keys().filter(|n| n.ends_with(".tmp")));
            if let Ok(rd) = std::fs::read_dir(&sub) {
                out.extend(rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".writing")));
            }
        }
        out
    }

    /// **The debts standing on `shelf`**: written down whole (a folder still
    /// being built is none).
    fn standing(shelf: &Path) -> Vec<String> {
        debts(shelf).into_iter().filter(|n| n.bytes().all(|b| b.is_ascii_digit())).collect()
    }

    /// **What keeps `home` from being as its road meant it**: a document
    /// other than the one the road leaves (`leaves`), fewer plugins in it
    /// than the session seated (`seated`), or a plugin of it not on the
    /// knobs it was playing (`live <bay>`). Empty when it is as meant.
    fn short_of_meant(home: &Path, leaves: Option<&[u8]>, seated: usize) -> Vec<String> {
        let doc = tangent_ui::arrangement::doc_in(home);
        if std::fs::read(&doc).ok().as_deref() != leaves {
            return vec!["the document is not the one the road leaves".to_owned()];
        }
        let Some(desk) = Arrangement::load_from(&doc).desk else {
            return vec!["the document holds no desk".to_owned()];
        };
        let plugins = stateful(&desk.desk_model().model.plugins);
        let mut short: Vec<String> = plugins
            .iter()
            .filter(|(sb, bundle)| read_state(&home.join("state"), *sb, bundle) != Some(format!("live {sb}").into_bytes()))
            .map(|(sb, bundle)| format!("{sb} ({bundle}) is not on the knobs it was playing"))
            .collect();
        if plugins.len() < seated {
            short.push(format!("the document names {} plugins of {seated}", plugins.len()));
        }
        short
    }

    /// **A kill at every write of every save road reopens with every
    /// state** (W5-save-gaps 4). Save, the prompt's Save and the save-backs
    /// of an Open and a New write the session's document in its own folder,
    /// Save As and the first save in a new one, and every one of them
    /// mirrors the knobs after it; the folder's `state/` held nothing of a
    /// plugin seated since its last save (and a never-written scratch's
    /// folder, or a new home, had no `state/` at all), so a kill between the
    /// two reopened those plugins at their defaults, with the knobs on a
    /// live shelf the relaunch replaces. The road writes its debt first now,
    /// states and all, saying what document it leaves. So does a Save of
    /// knobs alone, whose document's bytes do not change and whose debt the
    /// relaunch used to let go, and a road that writes no document and only
    /// mirrors (a clean Open's or New's save-back, a quit, Reset Template),
    /// which a kill partway through left half on new knobs.
    ///
    /// Killed at each write in turn (every state onto the live shelf, the
    /// debt's links, record and rename, just before the document, every
    /// state of the mirror, the debts the mirror takes, the settling), the
    /// relaunch (whose live shelf holds a stranger's knobs by then) pays
    /// what is owed. A road whose debt stood at the kill leaves its home
    /// holding the document it said it leaves, every plugin of it on the
    /// knobs it was playing, or, killed before a document that would have
    /// changed, exactly as its last save left it. A road whose debt never
    /// stood left the project as its last save did, or (its mirror whole)
    /// as it meant. Run whole, every road writes the very document it said
    /// it would. No debt outlives the relaunch, and nothing is left
    /// half-written.
    #[test]
    fn a_kill_at_every_write_of_every_save_road_reopens_with_every_state() {
        let ctx = egui::Context::default();
        let roads =
            [SaveRoad::Save, SaveRoad::SaveKnobs, SaveRoad::SaveBack, SaveRoad::MirrorAlone, SaveRoad::SaveAs, SaveRoad::FirstSave];
        for road in roads {
            let seated = if matches!(road.session(), Session::KeptKnobs) { 2 } else { 3 };
            let mut at = 0;
            let mut landed_kills = 0;
            loop {
                let tag = format!("{road:?}, killed at write {at}");
                let root = scratch(&format!("save-road-{road:?}"));
                let (mut app, own) = a_session(road.session(), &ctx, &root);
                let home = match road.target(&root) {
                    Some((purpose, target)) => app.save_choice_dir(purpose, &target).unwrap_or_else(|| panic!("{tag}: no home")),
                    None => own,
                };
                let live = scratch(&format!("save-road-live-{road:?}"));
                let doc = tangent_ui::arrangement::doc_in(&home);
                let doc_before = std::fs::read(&doc).ok();
                let state_before = files(&home.join("state"));
                let leaves = road.writes(&app).map(String::into_bytes).or_else(|| doc_before.clone());
                KILL_AFTER.with(|k| k.set(Some(at)));
                let killed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    save_road(road, &mut app, &live, &home, &root);
                }))
                .is_err();
                KILL_AFTER.with(|k| k.set(None));
                let stood = !standing(&live).is_empty();
                let as_left = files(&home.join("state"));
                // The relaunch.
                scramble(&live);
                let paid = pay_owed_mirror(&live);
                assert!(paid.kept.is_empty(), "{tag}: a reachable home's debt was kept: {paid:?}");
                assert!(debts(&live).is_empty(), "{tag}: a debt outlived the relaunch: {:?}", debts(&live));
                let now = std::fs::read(&doc).ok();
                if !killed {
                    let short = short_of_meant(&home, leaves.as_deref(), seated);
                    assert!(short.is_empty(), "{tag}: the road, run whole: {short:?}");
                } else if stood && now == leaves {
                    landed_kills += 1;
                    let short = short_of_meant(&home, leaves.as_deref(), seated);
                    assert!(short.is_empty(), "{tag}: the relaunch did not keep what the road meant: {short:?}");
                } else if stood {
                    assert_eq!(now, doc_before, "{tag}: the home holds a document neither the road's nor its last save's");
                    assert_eq!(files(&home.join("state")), state_before, "{tag}: a road that never wrote its document touched the project");
                } else {
                    assert_eq!(files(&home.join("state")), as_left, "{tag}: the relaunch wrote into a project nothing was owed");
                    let untouched = now == doc_before && as_left == state_before;
                    let short = short_of_meant(&home, leaves.as_deref(), seated);
                    assert!(
                        untouched || short.is_empty(),
                        "{tag}: no debt stood, and the project is neither as its last save left it nor whole: {short:?}"
                    );
                }
                // (A knob's temp the kill cut short may stand on the live
                // shelf, under no state's name: the next session's knobs
                // write over it.)
                assert!(
                    torn(&home).is_empty() && torn(&live.join(OWED_DIR)).is_empty(),
                    "{tag}: half-written: {:?} {:?}",
                    torn(&home),
                    torn(&live.join(OWED_DIR))
                );
                let _ = std::fs::remove_dir_all(&root);
                let _ = std::fs::remove_dir_all(&live);
                if !killed {
                    break;
                }
                at += 1;
            }
            assert!(at > 8, "{road:?}: fixture: the road made only {at} writes");
            assert!(landed_kills >= 3, "{road:?}: fixture: only {landed_kills} kills fell between the document and the settling");
        }
    }

    /// **A Save of knobs alone is paid at launch, though its document's
    /// bytes did not change** (W5-save-gaps 4). A knob turned in a plugin's
    /// own window never dirties the document, so Ctrl+S after one rewrites
    /// the bytes it found, and a debt judged by the document its road FOUND
    /// read that as a road that never wrote: the launch let it go, the knobs
    /// with it, and the template replaced the live shelf, so the project
    /// reopened on the knobs of the save before. The debt says what its road
    /// leaves now. Killed after the document and before the mirror, or
    /// before the document (which is already the one the road leaves), the
    /// relaunch pays, and every plugin reopens on the knobs it was playing
    /// at the press. A road whose document would have changed, killed before
    /// writing it, is still let go, the project as its last save left it.
    #[test]
    fn a_save_of_knobs_alone_is_paid_at_launch_though_its_document_did_not_change() {
        let ctx = egui::Context::default();
        let turn = |app: &tangent_ui::app::TangentApp, live: &Path| {
            for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(app)) {
                write_state_in(live, sb, &bundle, format!("turned {sb}").as_bytes());
            }
        };
        for after_the_document in [true, false] {
            let tag = if after_the_document { "killed after the document" } else { "killed before the document" };
            let root = scratch("knobs-alone");
            let (mut app, home) = a_session(Session::KeptKnobs, &ctx, &root);
            let doc = tangent_ui::arrangement::doc_in(&home);
            let on_disk = std::fs::read(&doc).expect("the document");
            let live = scratch("knobs-alone-live");
            turn(&app, &live);
            let _debt = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
            if after_the_document {
                app.save_project_doc();
            }
            // Killed here: no mirror, no settling.
            assert_eq!(std::fs::read(&doc).expect("the document"), on_disk, "{tag}: fixture: the save changed the document's bytes");
            scramble(&live);
            assert_eq!(pay_owed_mirror(&live).paid, vec![home.clone()], "{tag}: the relaunch let a Save of knobs alone go");
            assert!(debts(&live).is_empty(), "{tag}: the paid debt was kept");
            let desk = Arrangement::load_from(&doc).desk.expect("a desk");
            let plugins = stateful(&desk.desk_model().model.plugins);
            assert_eq!(plugins.len(), 2, "{tag}: fixture: the document names {} plugins", plugins.len());
            for (sb, bundle) in plugins {
                assert_eq!(
                    read_state(&home.join("state"), sb, &bundle),
                    Some(format!("turned {sb}").into_bytes()),
                    "{tag}: {sb} ({bundle}) reopened on the knobs of the save before"
                );
            }
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&live);
        }
        // A plugin seated since, so the document would change: killed before
        // it is written, the debt is let go and writes nothing.
        let root = scratch("knobs-alone-seated");
        let (mut app, home) = a_session(Session::KeptKnobs, &ctx, &root);
        app.set_plugin(chain_bay(&app, 2, 1), Some("/x/d.vst3"));
        let state = files(&home.join("state"));
        let live = scratch("knobs-alone-seated-live");
        turn(&app, &live);
        let _debt = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
        scramble(&live);
        assert_eq!(pay_owed_mirror(&live), OwedPayment::default(), "a road killed before a document that would change was paid");
        assert_eq!(files(&home.join("state")), state, "a road that never wrote its document touched the project");
        assert!(debts(&live).is_empty(), "the debt of a road that never wrote was kept");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **A whole mirror takes every older debt to its home with it**
    /// (W5-save-gaps 4). A debt is kept when its own mirror does not land
    /// whole (one write refused for a moment), and every later mirror of the
    /// live shelf into the same folder is newer than it. It used to outlive
    /// them all: a later road's settling took its own debt and no other, a
    /// later road whose retry of the older debt failed again owed its own
    /// beside it, and a road that held no debt took nothing, so the next
    /// Open's payment or launch put the older knobs back over the newer ones
    /// and removed the state of a plugin seated since, as stale.
    ///
    /// After a Save whose mirror was refused a write, each later road
    /// mirrors whole into the same project: a Save whose retry of the older
    /// debt is refused again, a road that writes no document (a quit, or an
    /// Open's save-back with knobs alone turned), and a mirror whose road
    /// could not write a debt of its own down. Then an Open's payment, as
    /// `switch_project` makes it on the line after its mirror, and a
    /// relaunch, a stranger's knobs on the live shelf: nothing is owed, and
    /// every plugin keeps the later knobs.
    #[test]
    fn a_whole_mirror_takes_every_older_debt_to_its_home() {
        #[derive(Clone, Copy, Debug)]
        enum Later {
            Save,
            MirrorAlone,
            NoDebt,
        }
        let ctx = egui::Context::default();
        for later in [Later::Save, Later::MirrorAlone, Later::NoDebt] {
            let tag = format!("{later:?}");
            let root = scratch(&format!("older-debts-{later:?}"));
            let (mut app, home) = a_session(Session::Kept, &ctx, &root);
            let live = scratch(&format!("older-debts-live-{later:?}"));
            let knobs = |app: &tangent_ui::app::TangentApp, word: &str| {
                for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(app)) {
                    write_state_in(&live, sb, &bundle, format!("{word} {sb}").as_bytes());
                }
            };
            // The earlier Save: its document lands, its mirror is refused a
            // write, and its debt is kept.
            knobs(&app, "older");
            let earlier = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
            app.save_project_doc();
            FAIL_WRITE.with(|f| f.set(Some(0)));
            let whole = mirror_into_home(&live, &home);
            FAIL_WRITE.with(|f| f.set(None));
            assert!(!whole, "{tag}: fixture: the earlier mirror landed whole");
            mirror_settled(earlier, whole);
            assert_eq!(standing(&live).len(), 1, "{tag}: fixture: the earlier debt was not kept");
            // Since: every knob turned, and a plugin seated, but where the
            // later road writes no document (nothing is unsaved).
            if !matches!(later, Later::MirrorAlone) {
                app.set_plugin(chain_bay(&app, 2, 1), Some("/x/d.vst3"));
            }
            knobs(&app, "later");
            match later {
                Later::Save | Later::MirrorAlone => {
                    let text = app.session_doc_text();
                    let doc = match later {
                        Later::Save => RoadDoc::Writes(text.as_bytes()),
                        _ => {
                            assert!(!app.project_dirty_exact(), "{tag}: fixture: the session holds unsaved work");
                            RoadDoc::Stands
                        }
                    };
                    // Its retry of the earlier debt, refused again.
                    FAIL_WRITE.with(|f| f.set(Some(0)));
                    let debt = owe_mirror(&live, &home, doc);
                    let refused = FAIL_WRITE.with(std::cell::Cell::take).is_none();
                    assert!(
                        refused && debt.is_some() && standing(&live).len() == 2,
                        "{tag}: fixture: the retry was not refused, or the later debt not written: {:?}",
                        debts(&live)
                    );
                    if matches!(later, Later::Save) {
                        app.save_project_doc();
                    }
                    let whole = mirror_into_home(&live, &home);
                    assert!(whole, "{tag}: fixture: the later mirror did not land whole");
                    mirror_settled(debt, whole);
                }
                Later::NoDebt => {
                    app.save_project_doc();
                    assert!(mirror_into_home(&live, &home), "{tag}: fixture: the later mirror did not land whole");
                }
            }
            assert!(debts(&live).is_empty(), "{tag}: a debt outlived a whole mirror into its home: {:?}", debts(&live));
            assert_eq!(pay_kept_mirrors(&live), OwedPayment::default(), "{tag}: an Open paid an older debt over the later knobs");
            scramble(&live);
            assert_eq!(pay_owed_mirror(&live), OwedPayment::default(), "{tag}: the relaunch paid an older debt over the later knobs");
            let desk = Arrangement::load_from(&tangent_ui::arrangement::doc_in(&home)).desk.expect("a desk");
            let plugins = stateful(&desk.desk_model().model.plugins);
            let seated = if matches!(later, Later::MirrorAlone) { 3 } else { 4 };
            assert_eq!(plugins.len(), seated, "{tag}: fixture: the document names {} plugins", plugins.len());
            for (sb, bundle) in plugins {
                assert_eq!(
                    read_state(&home.join("state"), sb, &bundle),
                    Some(format!("later {sb}").into_bytes()),
                    "{tag}: {sb} ({bundle}) went back to older knobs, or lost its state"
                );
            }
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&live);
        }
    }

    /// **A debt whose project cannot be reached stays owed, with its own
    /// states, until it can be paid or the project is discarded**
    /// (W5-save-gaps 4: "an owed mirror whose destination volume is
    /// unmounted at the next launch must stay owed"). A kept project on a
    /// volume of its own, killed between its document and its mirror; the
    /// volume is away at the relaunch, which keeps the debt (said, never
    /// dropped) and stands on a template whose knobs replace the live shelf;
    /// a second relaunch and an open keep it too. The volume back, the next
    /// open pays it from the knobs it kept, the stranger's on the live shelf
    /// notwithstanding, and a kill mid-payment leaves it to the next one.
    /// A debt to a project deleted while it waited is let go, and writes
    /// nothing.
    #[test]
    fn a_debt_to_a_project_that_cannot_be_reached_waits_with_its_states() {
        let ctx = egui::Context::default();
        let root = scratch("away");
        let vol = root.join("vol");
        let (mut app, home) = a_session(Session::Kept, &ctx, &vol);
        let live = scratch("away-live");
        // Killed after the document, before the mirror.
        let killed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(&app)) {
                write_state_in(&live, sb, &bundle, format!("live {sb}").as_bytes());
            }
            let _debt = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
            app.save_project_doc();
            panic!("killed before the mirror");
        }))
        .is_err();
        assert!(killed, "fixture: the road was not killed");
        let away = root.join("vol (away)");
        std::fs::rename(&vol, &away).expect("the volume goes away");
        let paid = pay_owed_mirror(&live);
        assert_eq!(paid, OwedPayment { paid: Vec::new(), kept: vec![home.clone()] }, "the relaunch dropped a debt it could not pay");
        scramble(&live);
        assert_eq!(pay_owed_mirror(&live).kept, vec![home.clone()], "a second relaunch dropped the debt");
        assert_eq!(pay_kept_mirrors(&live).kept, vec![home.clone()], "an open while the volume is away dropped the debt");
        assert_eq!(debts(&live).len(), 1, "the debt is not one debt");
        // A road to a home that is away owes it nothing: its document cannot
        // land there, and a debt written now would read the project's older
        // document, once the volume is back, as one this road wrote.
        assert!(
            owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes())).is_none(),
            "a road owed a mirror to a home it cannot reach"
        );
        assert!(owe_mirror(&live, &home, RoadDoc::Stands).is_none(), "a road owed a mirror to a home it cannot reach");
        assert_eq!(debts(&live).len(), 1, "a road to a home that is away wrote a debt down");

        std::fs::rename(&away, &vol).expect("the volume comes back");
        let mut at = 0;
        loop {
            KILL_AFTER.with(|k| k.set(Some(at)));
            let killed = std::panic::catch_unwind(|| pay_kept_mirrors(&live)).is_err();
            KILL_AFTER.with(|k| k.set(None));
            if !killed {
                break;
            }
            at += 1;
        }
        assert!(at > 2, "fixture: the payment made only {at} writes");
        assert!(debts(&live).is_empty(), "the paid debt was kept: {:?}", debts(&live));
        let desk = Arrangement::load_from(&tangent_ui::arrangement::doc_in(&home)).desk.expect("a desk");
        for (sb, bundle) in stateful(&desk.desk_model().model.plugins) {
            assert_eq!(
                read_state(&home.join("state"), sb, &bundle),
                Some(format!("live {sb}").into_bytes()),
                "{sb} ({bundle}) came back without the knobs its debt kept"
            );
        }
        assert!(torn(&home).is_empty() && torn(&live).is_empty(), "half-written: {:?} {:?}", torn(&home), torn(&live));

        // Discarded: the project deleted while its debt waited.
        let (mut app, home) = a_session(Session::Kept, &ctx, &root.join("gone"));
        for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(&app)) {
            write_state_in(&live, sb, &bundle, format!("live {sb}").as_bytes());
        }
        let _debt = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
        app.save_project_doc();
        std::fs::remove_dir_all(&home).expect("the project is deleted");
        assert_eq!(pay_owed_mirror(&live), OwedPayment::default(), "a debt to a deleted project was paid or kept");
        assert!(debts(&live).is_empty(), "a debt to a deleted project was kept");
        assert!(!home.exists(), "paying a deleted project's debt made its folder again");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **A road whose mirror did not land whole leaves its debt owed**, and
    /// the next launch pays it: one write of the mirror refused (a full
    /// disk, a permission), the project's `state/` is short of a plugin, and
    /// taking the debt away there would drop that state in silence. The
    /// debt keeps its own states, so the relaunch pays them whatever the
    /// live shelf holds by then.
    #[test]
    fn a_mirror_that_did_not_land_whole_leaves_its_debt_for_the_next_launch() {
        let ctx = egui::Context::default();
        let root = scratch("unwhole");
        let (mut app, home) = a_session(Session::Kept, &ctx, &root);
        let live = scratch("unwhole-live");
        for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(&app)) {
            write_state_in(&live, sb, &bundle, format!("live {sb}").as_bytes());
        }
        let debt = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
        assert!(debt.is_some(), "fixture: no debt was written down");
        app.save_project_doc();
        FAIL_WRITE.with(|f| f.set(Some(0)));
        let whole = mirror_into_home(&live, &home);
        let struck = FAIL_WRITE.with(std::cell::Cell::take).is_none();
        assert!(struck && !whole, "fixture: the mirror's write was not refused (whole: {whole})");
        mirror_settled(debt, whole);
        assert_eq!(debts(&live).len(), 1, "a mirror that did not land took its debt away");
        scramble(&live);
        assert_eq!(pay_owed_mirror(&live).paid, vec![home.clone()], "the relaunch did not pay the debt");
        assert!(debts(&live).is_empty(), "the paid debt was kept");
        let desk = Arrangement::load_from(&tangent_ui::arrangement::doc_in(&home)).desk.expect("a desk");
        for (sb, bundle) in stateful(&desk.desk_model().model.plugins) {
            assert_eq!(read_state(&home.join("state"), sb, &bundle), Some(format!("live {sb}").into_bytes()), "{sb} ({bundle})");
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **A road to a home settles what an earlier road still owes it, before
    /// it owes its own**: an earlier road's document landed and its mirror
    /// never ran (killed, its project away at the launch that should have
    /// paid it), and a later road saves into the same project. The earlier
    /// debt is settled when the later one is written down, so the next
    /// launch, a stranger's knobs on the live shelf, owes nothing, and the
    /// project keeps the later knobs: paid after them, the earlier debt
    /// would have put the older ones back.
    #[test]
    fn a_road_to_a_home_settles_what_an_earlier_road_still_owes_it() {
        let ctx = egui::Context::default();
        let root = scratch("supersede");
        let (mut app, home) = a_session(Session::Kept, &ctx, &root);
        let live = scratch("supersede-live");
        let knobs = |app: &tangent_ui::app::TangentApp, word: &str| {
            for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(app)) {
                write_state_in(&live, sb, &bundle, format!("{word} {sb}").as_bytes());
            }
        };
        knobs(&app, "older");
        let earlier = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
        assert!(earlier.is_some(), "fixture: the earlier road wrote no debt");
        app.save_project_doc();
        // Killed: no mirror, no settling.
        app.set_plugin(chain_bay(&app, 2, 1), Some("/x/d.vst3"));
        knobs(&app, "later");
        let debt = owe_mirror(&live, &home, RoadDoc::Writes(app.session_doc_text().as_bytes()));
        app.save_project_doc();
        let whole = mirror_into_home(&live, &home);
        mirror_settled(debt, whole);
        assert!(debts(&live).is_empty(), "the earlier debt outlived the later road: {:?}", debts(&live));
        scramble(&live);
        assert_eq!(pay_owed_mirror(&live), OwedPayment::default(), "the next launch paid an earlier debt over the later knobs");
        let desk = Arrangement::load_from(&tangent_ui::arrangement::doc_in(&home)).desk.expect("a desk");
        for (sb, bundle) in stateful(&desk.desk_model().model.plugins) {
            assert_eq!(read_state(&home.join("state"), sb, &bundle), Some(format!("later {sb}").into_bytes()), "{sb} ({bundle})");
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **Two debts to one home are paid in order, or wait together**: the
    /// older one's payment did not land (one of its writes refused), so the
    /// newer one waits behind it rather than being paid and let go, which
    /// would have left the older to be paid over it at the next launch. The
    /// next launch pays both, oldest first, and the project keeps the newer
    /// knobs.
    #[test]
    fn two_debts_to_one_home_are_paid_in_order_or_wait_together() {
        let ctx = egui::Context::default();
        let root = scratch("in-order");
        let (mut app, home) = a_session(Session::Kept, &ctx, &root);
        let live = scratch("in-order-live");
        let knobs = |app: &tangent_ui::app::TangentApp, word: &str| {
            for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(app)) {
                write_state_in(&live, sb, &bundle, format!("{word} {sb}").as_bytes());
            }
        };
        // Two roads killed after their documents, each written down as its
        // road wrote it (the later road never got to settle the earlier).
        let owed = |app: &tangent_ui::app::TangentApp| {
            let leaves = fingerprint(app.session_doc_text().as_bytes());
            write_debt(&live, &home, doc_fingerprint(&home).as_deref(), Some(&leaves), &live)
        };
        knobs(&app, "older");
        let older = owed(&app);
        app.save_project_doc();
        app.set_plugin(chain_bay(&app, 2, 1), Some("/x/d.vst3"));
        knobs(&app, "later");
        let newer = owed(&app);
        app.save_project_doc();
        assert!(older.is_some() && newer.is_some() && debts(&live).len() == 2, "fixture: not two debts");
        scramble(&live);
        FAIL_WRITE.with(|f| f.set(Some(0)));
        let first = pay_owed_mirror(&live);
        FAIL_WRITE.with(|f| f.set(None));
        assert_eq!(first, OwedPayment { paid: Vec::new(), kept: vec![home.clone(), home.clone()] }, "a newer debt was paid ahead of an older one that waits");
        assert_eq!(debts(&live).len(), 2, "a debt was let go while an older one to its home waits");
        let second = pay_owed_mirror(&live);
        assert_eq!(second.paid, vec![home.clone(), home.clone()], "the next launch did not pay both");
        assert!(debts(&live).is_empty(), "a paid debt was kept");
        let desk = Arrangement::load_from(&tangent_ui::arrangement::doc_in(&home)).desk.expect("a desk");
        for (sb, bundle) in stateful(&desk.desk_model().model.plugins) {
            assert_eq!(read_state(&home.join("state"), sb, &bundle), Some(format!("later {sb}").into_bytes()), "{sb} ({bundle})");
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **A debt the first journal wrote is paid as that journal paid it**:
    /// one record beside the live shelf (`.states-owed.json`), owing the
    /// live shelf itself, which at launch still holds the dying session's
    /// knobs. Its home's document landed, so the launch mirrors those knobs
    /// into it and the record goes; one whose document never landed goes
    /// too, and pays nothing.
    #[test]
    fn a_debt_written_by_the_first_journal_is_paid_at_launch() {
        let ctx = egui::Context::default();
        let root = scratch("single-record");
        let (mut app, home) = a_session(Session::Kept, &ctx, &root);
        let live = scratch("single-record-live");
        for (sb, bundle) in stateful(&crate::engine_bridge::desk_bays(&app)) {
            write_state_in(&live, sb, &bundle, format!("live {sb}").as_bytes());
        }
        let record = serde_json::json!({ "home": home.to_string_lossy(), "before": doc_fingerprint(&home) });
        std::fs::write(live.join(SINGLE_OWED_NAME), record.to_string()).expect("the record");
        app.save_project_doc();
        assert_eq!(pay_owed_mirror(&live).paid, vec![home.clone()], "the first journal's debt was not paid");
        assert!(!live.join(SINGLE_OWED_NAME).exists() && debts(&live).is_empty(), "the first journal's debt outlived the launch");
        let desk = Arrangement::load_from(&tangent_ui::arrangement::doc_in(&home)).desk.expect("a desk");
        for (sb, bundle) in stateful(&desk.desk_model().model.plugins) {
            assert_eq!(read_state(&home.join("state"), sb, &bundle), Some(format!("live {sb}").into_bytes()), "{sb} ({bundle})");
        }
        // Never landed: let go, nothing written.
        let before = files(&home.join("state"));
        let record = serde_json::json!({ "home": home.to_string_lossy(), "before": doc_fingerprint(&home) });
        std::fs::write(live.join(SINGLE_OWED_NAME), record.to_string()).expect("the record");
        scramble(&live);
        assert_eq!(pay_owed_mirror(&live), OwedPayment::default(), "a debt whose document never landed was paid");
        assert_eq!(files(&home.join("state")), before, "a debt whose document never landed wrote into the project");
        assert!(!live.join(SINGLE_OWED_NAME).exists() && debts(&live).is_empty(), "the record outlived the launch");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&live);
    }

    /// **Every road that mirrors the live shelf into a project owes the
    /// mirror first and settles it after**, in the host (a source scan,
    /// because a `DesktopApp` cannot be built in a test).
    ///
    /// - Each call of the app's `save_project_doc` in `desktop.rs` follows
    ///   the knobs onto the live shelf and the debt that says what the road
    ///   writes (Save, `switch_project`, New's arm:
    ///   `owe_states_to_own_home`, with `TangentApp::session_doc_text`).
    /// - Each mirror (`copy_states_to_project`) follows a debt owed in its
    ///   own road since the mirror before it, and is settled by its own
    ///   answer before the next: the in-place writes, the save-backs of an
    ///   Open and a New with nothing unsaved (owed beside the document as
    ///   it stands), a quit, Reset Template, Save As and the first save.
    /// - The one mirror into a project is the one that takes every older
    ///   debt to it (`shelf::mirror_into_home`).
    /// - New settles before the template replaces the live shelf; an Open
    ///   pays what is still owed (`pay_kept_mirrors`) before it reads the
    ///   project's states; the host's Save carries a prompt's interrupted
    ///   action on (`host_saved`) after its road.
    /// - Nothing in the host takes a debt away but the settling, and the
    ///   launch is the one place the first journal's record is paid from
    ///   the live shelf.
    #[test]
    fn every_road_that_mirrors_into_a_project_owes_the_mirror_first() {
        let whole = include_str!("desktop.rs");
        let src = &whole[..whole.find("\nmod tests {").expect("the tests module")];
        // The road a call stands in: its fn's bounds, and its name.
        let road_of = |at: usize| -> (usize, usize, String) {
            let start = src[..at].rfind("\n    fn ").expect("the road's fn");
            let end = src[start + 1..].find("\n    fn ").map_or(src.len(), |n| start + 1 + n);
            let name = src[start..end].trim_start().trim_start_matches("fn ").split('(').next().unwrap_or("").to_owned();
            (start, end, name)
        };
        let documents: Vec<usize> = src.match_indices("self.app.save_project_doc();").map(|(at, _)| at).collect();
        assert_eq!(documents.len(), 3, "a road writes the document that this scan does not know");
        for at in documents {
            let (start, _, name) = road_of(at);
            let before = &src[start..at];
            let knobs = before.rfind("self.save_plugin_states();").unwrap_or_else(|| panic!("{name}: no knobs before the document"));
            let owe = before.rfind("self.owe_states_to_own_home();").unwrap_or_else(|| panic!("{name}: the document lands with no debt"));
            assert!(knobs < owe, "{name}: the debt is written before the knobs are on the live shelf");
        }
        let owes = ["self.owe_states_to_own_home();", "self.owe_states_beside_own_document();", "self.owe_states_to_new_home(purpose, &path);"];
        let mirrors: Vec<usize> = src.match_indices("self.copy_states_to_project();").map(|(at, _)| at).collect();
        assert_eq!(mirrors.len(), 7, "a road mirrors into a project that this scan does not know");
        for (k, &at) in mirrors.iter().enumerate() {
            let (start, end, name) = road_of(at);
            let from = k.checked_sub(1).map_or(start, |p| mirrors[p].max(start));
            let to = mirrors.get(k + 1).map_or(end, |n| (*n).min(end));
            assert!(owes.iter().any(|o| src[from..at].contains(o)), "{name}: a mirror into a project with no debt owed first");
            assert!(src[..at].ends_with("whole = "), "{name}: a mirror whose answer settles nothing");
            assert!(
                src[at..to].contains("crate::shelf::mirror_settled(debt, whole);"),
                "{name}: a mirror whose debt is never settled"
            );
        }
        // With nothing unsaved, the save-backs owe the mirror all the same,
        // beside the document as it stands; and so do a quit and Reset
        // Template, which write none.
        let switch = &src[src.find("fn switch_project(").expect("switch_project")..];
        assert!(
            switch.contains("} else if !discarded {\n            debt = self.owe_states_beside_own_document();"),
            "an Open's save-back with nothing unsaved mirrors with no debt"
        );
        let new_arm = &src[src.find("tangent_ui::ports::ProjectRequest::NewDiscarding => {").expect("New's arm")..];
        assert!(
            new_arm.contains("} else if !discarded {\n                        debt = self.owe_states_beside_own_document();"),
            "New's save-back with nothing unsaved mirrors with no debt"
        );
        let settled = new_arm.find("crate::shelf::mirror_settled(debt, whole);").expect("New settles its debt");
        let template = new_arm.find("self.app.adopt_template(&template, &dir);").expect("New adopts the template");
        assert!(settled < template, "New settles its debt after the template replaced the live shelf");
        for (road, from, to) in [
            ("a quit", "fn on_exit(&mut self", "\n    }\n"),
            ("Reset Template", "tangent_ui::ports::ProjectRequest::ResetTemplate => {", "tangent_ui::ports::ProjectRequest::Save => {"),
        ] {
            let body = &src[src.find(from).unwrap_or_else(|| panic!("{road} is gone"))..];
            let body = &body[..body.find(to).unwrap_or_else(|| panic!("{road}'s end"))];
            let owe = body.find("let debt = self.owe_states_beside_own_document();");
            let mirror = body.find("let whole = self.copy_states_to_project();");
            assert!(owe.is_some_and(|o| mirror.is_some_and(|m| o < m)), "{road} mirrors with no debt owed beside its document");
        }
        // What each debt says its road leaves.
        let body = |name: &str| {
            let at = src.find(name).unwrap_or_else(|| panic!("`{name}` is gone"));
            &src[at..at + src[at..].find("\n    }\n").expect("its end")]
        };
        let own = body("fn owe_states_to_own_home(");
        assert!(
            own.contains("let doc = self.app.session_doc_text();")
                && own.contains("crate::shelf::owe_mirror(&crate::shelf::global_dir(), &home, crate::shelf::RoadDoc::Writes(doc.as_bytes()))"),
            "the own home's debt does not say the document its road writes"
        );
        assert!(
            body("fn owe_states_beside_own_document(")
                .contains("crate::shelf::owe_mirror(&crate::shelf::global_dir(), &home, crate::shelf::RoadDoc::Stands)"),
            "a road that writes no document owes a debt that says it writes one"
        );
        assert!(
            body("fn owe_states_to_new_home(").contains("let doc = self.app.save_choice_doc_text();"),
            "a new home's debt does not say the document its save writes there"
        );
        // The one mirror into a project, the one that takes the older debts.
        assert!(
            body("fn copy_states_to_project(").contains("crate::shelf::mirror_into_home(&crate::shelf::global_dir(), home)"),
            "the mirror into a project leaves the debts it makes old standing"
        );
        assert!(!src.contains("crate::shelf::mirror_whole("), "a road mirrors into a project and leaves the older debts standing");
        let switch_open = switch.find("self.app.open_project(doc);").expect("the open");
        let pay = switch.find("crate::shelf::pay_kept_mirrors(&crate::shelf::global_dir())").expect("an Open pays what is owed");
        assert!(pay < switch_open, "an Open reads the project's states before paying what is owed it");
        let save = &src[src.find("tangent_ui::ports::ProjectRequest::Save => {").expect("Save's arm")..];
        let road = save.find("self.save_project_and_states();").expect("Save's road");
        let carry = save.find("self.app.host_saved();").expect("Save carries a prompt on");
        assert!(road < carry, "a prompt's interrupted action carries on before the Save landed");
        assert!(!src.contains("crate::shelf::mirror_paid("), "a road takes a debt away without asking whether its mirror landed");
        assert_eq!(src.matches("crate::shelf::pay_owed_mirror(").count(), 1, "the live shelf pays a debt on more than one road");
    }
}
