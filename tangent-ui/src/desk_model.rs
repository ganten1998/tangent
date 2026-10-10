//! **The desk as the list of tracks that exist**, by identity: the model the
//! format-2 reader produces and the writer consumes.
//!
//! **Why a model and not more arrays.** The desk is written today as some
//! forty parallel arrays and masks, each as wide as the legacy desk: a
//! channel's fader is `channel_gains[n]`, its name `strip_names[n]`, its
//! mute bit `1 << n` of a u64, its plugins somewhere in a 13,176-string
//! seat map. Every fact about one track is spread across all of them, so
//! every structural edit has to touch every one, and every array that a
//! reorder forgot parted a track from one of its facts. Masks also stop at
//! their word: a u32 is full at 32 rows and `1 << n` is undefined past 63.
//! Here a track is ONE [`TrackRow`] that carries every per-track fact, a
//! row's position in `rows` is its lane, and whatever names a track, a
//! plugin bay or a route endpoint does so by [`TrackId`] or [`SeatBay`],
//! which no edit renumbers.
//!
//! **Live (W6).** The settings file's desk is [`TrackRow`]s
//! (`Settings::rows`, with the identity maps beside them), the document's
//! is too (`DeskSnap::rows`), and every structural edit is one of the four
//! below, run on the live desk as one model (`Settings::with_model`). The
//! dense 32-wide shape ([`V22Desk`]) survives only as what the legacy files
//! are read in: [`DeskModel::from_v22`] reads it onto rows once, under the
//! identity rule, and nothing writes it back in production (format 2 is the
//! one writer since `desk_format::WRITE_V8`).
//!
//! **What a row does NOT hold.** Clips and curves live in the arrangement,
//! keyed by lane, and a kit lives in drumpad.json. Those are the positional
//! units a structural edit must carry; the model cannot reach them, so each
//! edit hands its caller a [`Reshape`] that says where every old lane went,
//! and the class of a row ([`RowClass`]) takes what the arrangement knows
//! about it as a [`RowUse`].
//!
//! **A folder's run is the third positional unit, and the model DOES reach
//! it**, because it is spelled entirely in row flags: a row with `in_folder`
//! belongs to the nearest `folder` row above it with only members between
//! ([`DeskModel::folder_of`], the same walk as `TangentApp::folder_of`).
//! Carrying the flags with their rows is therefore not enough: a row that
//! leaves or lands changes what the rows below it walk up to. So each edit
//! keeps every run whole itself. `remove_rows` hands a removed header's
//! members up to the folder it sat in, or out to the top level, exactly as
//! `remove_channel_inner` always has. `insert_rows` refuses to re-file any
//! row already on the desk. `permute` refuses to strand a member under no
//! folder; WHICH folder a moved row lands in is the gesture's to decide
//! (a drop sets the leaders' flags first), so it answers every row whose
//! folder changed in [`Reshape::refiled`] rather than guessing at intent.
//!
//! **Scalars that never lived in strip space keep their own keys** (the
//! drum bus's fader, the click's, the master's, the tempo): they are not
//! per-track facts and the desk object does not carry them.

use std::collections::{BTreeMap, BTreeSet};

use crate::desk_format::Dropped;
use crate::desk_id::{slot_key, Node, Rack, SeatBay, TrackId, BAYS, MAX_ROWS};
use crate::desk_legacy::{
    bind, decode_v32, decode_v32_strip, encode_v32_strip, LegacyOwner, V32_CHANNELS, V32_STRIPS,
};
use crate::desk_lift::DeskEra;
use crate::dialogs::{KEYS_PARAMS, KEYS_PATCHES};
use crate::recorder::MidiInputSel;
use crate::routing::{LaneKey, LaneRoute, LaneTarget};

/// One row's Keys knobs: every patch's eight, `KEYS_PATCHES * KEYS_PARAMS`,
/// laid out as one channel's slice of `Settings::strip_keys_params`.
pub const KEYS_SPAN: usize = KEYS_PATCHES * KEYS_PARAMS;

/// **What a row is for.** No kind (`None` on [`TrackRow::kind`]) is a row
/// that holds nothing of its own yet: a bare added track, a folder header,
/// the indented plus, or a vacant legacy row.
///
/// Bytes are the settings file's own `channel_kinds` values and are
/// append-only: 1 and 2 have meant MIDI and AUDIO since kinds existed, and
/// Bus takes 3 (design section 9), so a legacy desk and a format-2 one agree
/// on every kind either can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RowKind {
    Midi,
    Audio,
    Bus,
}

impl RowKind {
    /// The settings byte. Any byte this build has no kind for reads as no
    /// kind, which is what `ChannelKind::from_u8` has always done.
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(RowKind::Midi),
            2 => Some(RowKind::Audio),
            3 => Some(RowKind::Bus),
            _ => None,
        }
    }

    /// [`RowKind::from_byte`]'s inverse, 0 for no kind.
    pub const fn byte(kind: Option<Self>) -> u8 {
        match kind {
            None => 0,
            Some(RowKind::Midi) => 1,
            Some(RowKind::Audio) => 2,
            Some(RowKind::Bus) => 3,
        }
    }

    /// The format-2 spelling.
    pub const fn name(self) -> &'static str {
        match self {
            RowKind::Midi => "midi",
            RowKind::Audio => "audio",
            RowKind::Bus => "bus",
        }
    }

    /// [`RowKind::name`]'s inverse; anything else is `None`.
    pub fn from_name(s: &str) -> Option<Self> {
        [RowKind::Midi, RowKind::Audio, RowKind::Bus]
            .into_iter()
            .find(|k| k.name() == s)
    }
}

/// **One send: a copy of a strip into a return or a bus.** Today's send
/// knob is the entry `to: Fx, pre: false`; any number of others may follow
/// once buses exist (W7). A strip with the knob at zero carries no entry,
/// because a send at nothing is not a send, and a vacant row should not
/// write one.
///
/// Named as the contract names it; it shadows `std::marker::Send` only
/// inside a module that imports it bare.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Send {
    pub to: Node,
    pub level: f64,
    pub pre: bool,
}

/// **The level of a strip's send to the effects return**: today's send
/// knob, the post-fader `Fx` entry, or nothing when it has none.
pub fn fx_send_level(sends: &[Send]) -> f64 {
    sends.iter().find(|s| s.to == Node::Fx && !s.pre).map_or(0.0, |s| s.level)
}

/// **Turn a strip's send knob**: the post-fader `Fx` entry set to `level`,
/// added where there was none, and taken away at exactly zero (a send at
/// nothing is not a send; see [`Send`]). Every other send is left alone, so
/// the knob and a later bus's sends never step on each other.
pub fn set_fx_send_level(sends: &mut Vec<Send>, level: f64) {
    let at = sends.iter().position(|s| s.to == Node::Fx && !s.pre);
    match (at, level.to_bits() != 0) {
        (Some(i), true) => sends[i].level = level,
        (Some(i), false) => {
            sends.remove(i);
        }
        (None, true) => sends.push(Send { to: Node::Fx, level, pre: false }),
        (None, false) => {}
    }
}

/// The DX7's cartridge and patch on one row. `cart` is a path, empty for
/// the bank that ships: `strip_dx7_cartridge`'s own convention.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dx7 {
    pub cart: String,
    pub patch: usize,
}

/// The Keys patch on one row and every patch's knobs, [`KEYS_SPAN`] long,
/// so switching patch and back finds each sound as it was left.
#[derive(Debug, Clone, PartialEq)]
pub struct KeysSel {
    pub patch: usize,
    pub params: Vec<f64>,
}

impl Default for KeysSel {
    fn default() -> Self {
        KeysSel { patch: 0, params: crate::dialogs::keys_params_default(1) }
    }
}

/// A frozen row's render: the file (relative to the document, as a clip's
/// path is) and its length. The lane is the row's position, so it moves
/// with the row and cannot be left behind by a reorder, which is what a
/// `FrozenLane { lane, .. }` list could be.
#[derive(Debug, Clone, PartialEq)]
pub struct Frozen {
    pub path: String,
    pub len_s: f64,
}

/// **Everything that is true of one track**, carried as one value.
///
/// Every field here moves with the row on every structural edit because it
/// is IN the row. A field that names a lane or another row by position would
/// break that, which is what the exhaustive destructure tests below exist to
/// catch: adding a field fails them to compile until somebody has decided
/// how `append`, `insert_rows`, `remove_rows` and `permute` treat it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRow {
    pub id: TrackId,
    pub kind: Option<RowKind>,
    /// A bare track somebody added: it exists with no kind and no clips.
    pub added: bool,
    pub icon: u8,
    pub name: String,
    pub color: i64,
    pub gain: f64,
    pub pan: f64,
    /// The input spec, `channel_inputs`' grammar (`midi:all`, an audio
    /// pick), empty for none.
    pub input: String,
    pub midi: MidiInputSel,
    pub mute: bool,
    pub solo: bool,
    pub phase: bool,
    pub arm: bool,
    pub theory: bool,
    pub folder: bool,
    pub in_folder: bool,
    pub shut: bool,
    pub sends: Vec<Send>,
    pub dx7: Dx7,
    pub keys: KeysSel,
    pub frozen: Option<Frozen>,
}

impl TrackRow {
    /// **A row that holds nothing**, with every value a fresh legacy channel
    /// had: unity fader, centre pan, no input, every MIDI device, the
    /// shipped DX7 bank and Keys knobs. A vacant legacy row reads as exactly
    /// this, so a picture of a desk can list only what differs from it.
    pub fn vacant(id: TrackId) -> Self {
        TrackRow {
            id,
            kind: None,
            added: false,
            icon: 0,
            name: String::new(),
            color: 0,
            gain: 1.0,
            pan: 0.0,
            input: String::new(),
            midi: MidiInputSel::EVERYTHING,
            mute: false,
            solo: false,
            phase: false,
            arm: false,
            theory: false,
            folder: false,
            in_folder: false,
            shut: false,
            sends: Vec::new(),
            dx7: Dx7::default(),
            keys: KeysSel::default(),
            frozen: None,
        }
    }
}

/// The row's kind as the byte the legacy arrays spelt it (`RowKind::byte`),
/// for the tests written against those arrays.
#[cfg(test)]
impl TrackRow {
    pub(crate) fn kind_byte(&self) -> u8 {
        RowKind::byte(self.kind)
    }
}

/// **The strip-space facts of one special** (the drum bus, the click, the
/// fx return, the master): what used to sit at strip 32..=35 of the names,
/// colours, sends and masks. Their faders are scalars that never lived in
/// strip space and keep their own keys. The master has no send in any
/// legacy shape; its `sends` is still a list so a later build may give it
/// one without a new shape.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Special {
    pub name: String,
    pub color: i64,
    pub sends: Vec<Send>,
    pub mute: bool,
    pub solo: bool,
    pub phase: bool,
}

/// The four specials, each exactly once per desk.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Specials {
    pub drums: Special,
    pub click: Special,
    pub fx: Special,
    pub master: Special,
}

impl Specials {
    /// The special a node is, `None` for a track.
    pub fn get(&self, node: Node) -> Option<&Special> {
        match node {
            Node::Drums => Some(&self.drums),
            Node::Click => Some(&self.click),
            Node::Fx => Some(&self.fx),
            Node::Master => Some(&self.master),
            Node::Track(_) => None,
        }
    }

    pub fn get_mut(&mut self, node: Node) -> Option<&mut Special> {
        match node {
            Node::Drums => Some(&mut self.drums),
            Node::Click => Some(&mut self.click),
            Node::Fx => Some(&mut self.fx),
            Node::Master => Some(&mut self.master),
            Node::Track(_) => None,
        }
    }

    /// All four, in [`Node::SPECIALS`] order (the legacy strip order).
    pub fn iter(&self) -> impl Iterator<Item = (Node, &Special)> {
        [
            (Node::Drums, &self.drums),
            (Node::Click, &self.click),
            (Node::Fx, &self.fx),
            (Node::Master, &self.master),
        ]
        .into_iter()
    }
}

/// Where a route goes: a node (a MOVE, see `routing.rs`), or a copy out of
/// the box on hardware jacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTo {
    Node(Node),
    Hardware { first: usize, stereo: bool },
}

/// **One route, by identity at both ends.** The gain stays f32 because the
/// legacy table and the engine hold it so: widening it would let a format-2
/// file carry a gain no engine can play back exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteEdge {
    pub from: Node,
    pub to: RouteTo,
    pub gain: f32,
}

/// **One sidechain key**: the bay holding the keyed effect, and the node it
/// listens to. One per bay, as `KeyEdges::set` has always kept it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideKey {
    pub seat: SeatBay,
    pub from: Node,
}

/// **The desk.** A row's position in `rows` is its lane; everything else
/// names rows by id.
///
/// `next_id` is a u64 so that "above every id" can always be said: a file
/// may hold `t4294967295`, and a u32 counter would then have nowhere to go.
/// [`DeskModel::mint`] answers `None` once the u32 space is spent, which
/// takes a hostile file.
#[derive(Debug, Clone, PartialEq)]
pub struct DeskModel {
    pub next_id: u64,
    pub rows: Vec<TrackRow>,
    pub specials: Specials,
    pub plugins: BTreeMap<SeatBay, String>,
    pub bypass: BTreeSet<SeatBay>,
    pub routes: Vec<RouteEdge>,
    pub keys: Vec<SideKey>,
    /// **Which legacy shape this document's `state/` folder was written
    /// in**, while any plugin there may still have only its legacy sidecar.
    /// Documents only, and only until a verified mirror clears it (design
    /// 6.6). `None` on settings and on every desk born format 2.
    pub legacy_states: Option<DeskEra>,
}

impl Default for DeskModel {
    fn default() -> Self {
        DeskModel::empty()
    }
}

/// **What the arrangement and the drum panel know about a row** that the
/// model does not: whether it has clips, a curve, a kit. A row's class
/// depends on them, and they live elsewhere because they are positional
/// (clips and curves by lane) or kept in their own file (kits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowUse {
    pub clips: bool,
    pub curve: bool,
    pub kit: bool,
}

/// **What a row is, for drawing, counting and the engine** (design section
/// 4):
///
/// | Class | Drawn | Counted | Engine lane |
/// |---|---|---|---|
/// | Vacant | no | no | no |
/// | Placeholder | yes | no | yes |
/// | Bus | yes | no | yes |
/// | Counted | yes | yes | yes |
///
/// Counted is MIDI, AUDIO, a bare added track, a folder, or any row with
/// clips: exactly `TangentApp::row_counted`. Placeholder is today's
/// indented plus, and also any other uncounted row that still HOLDS
/// something (a curve, a kit, a route or key or incoming send, a freeze, a
/// chain plugin). The design's table names only the indent; a row with a
/// plugin and no kind is not in it, and calling it Vacant would hide a
/// sounding plugin and drop its lane, which is the one thing a class may
/// never do. Pedal-only and pad-only residue stays Vacant, as the design
/// rules for pedals: a deleted track's pedalboard is left loaded by
/// today's `remove_channel`, and drawing every such row back would
/// resurrect deleted tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowClass {
    /// Nothing at all: no kind, not added, no folder flag, no clips and no
    /// residue. Pedal-only residue still counts as Vacant.
    Vacant,
    /// Drawn and given a lane but not counted: only the in-folder flag (the
    /// indented plus), or residue (a chain plugin, a curve, a freeze, a kit,
    /// a route or key end) on a row that is not a track.
    Placeholder,
    /// A bus row. No bus kind exists before W7, so nothing is classed
    /// here yet; the variant is the contract's.
    Bus,
    /// A track: MIDI, AUDIO, bare added, a folder, or any row with clips.
    Counted,
}

/// **Where every old lane went** after a structural edit:
/// `old_to_new[old]` is the new lane, `None` for a removed row. The caller
/// rewrites what the model cannot reach (clip lanes, curve lanes, a kit
/// keyed by lane) through [`Reshape::lane`] in the same gesture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reshape {
    pub old_to_new: Vec<Option<usize>>,
    /// **Every kept row whose folder changed**, in new lane order: a
    /// removed header's members handed up or out, a moved row landing in
    /// another run. Membership is positional, so this is the one fact about
    /// a run the caller could not read off `old_to_new`; a gesture checks it
    /// against what it meant, and an empty list says no run moved under
    /// anybody.
    pub refiled: Vec<TrackId>,
}

impl Reshape {
    /// Where old lane `old` went. A lane past the old desk maps to `None`:
    /// nothing was there to move.
    pub fn lane(&self, old: usize) -> Option<usize> {
        self.old_to_new.get(old).copied().flatten()
    }
}

/// Why a structural edit changed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditRefused {
    /// A row index past the desk.
    NoSuchRow(usize),
    /// A row whose id the desk (or the same batch) already holds.
    DuplicateId(TrackId),
    /// The desk would pass [`MAX_ROWS`].
    TooManyRows,
    /// `permute` was handed something that is not a permutation of the
    /// rows.
    NotAPermutation,
    /// **The edit would break a folder run at this row**: leave a member
    /// (`in_folder`) with no folder above it that had one, place a new row
    /// that says it is a member under no folder, or (an insert) walk a row
    /// already on the desk into a different folder. A drop sets the moving
    /// rows' flags for where they land BEFORE the edit; this is what it says
    /// when one was missed, instead of the members below quietly joining
    /// the folder above or drawing indented under nothing.
    BreaksFolder(TrackId),
}

/// **What `remove_rows` took with the rows**: every plugin and bypass on
/// their racks (chains, pedals and pads: the pedalboard leaves with its
/// row), every route and key touching them, and every send into them. The
/// caller writes the plugin states to the shelf and keeps the rest for its
/// undo; nothing is left in the model naming a removed id. A removed
/// folder's members stay on the desk, handed up a level; they are listed in
/// `reshape.refiled`.
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
    pub reshape: Reshape,
    pub rows: Vec<(usize, TrackRow)>,
    pub plugins: BTreeMap<SeatBay, String>,
    pub bypass: BTreeSet<SeatBay>,
    pub routes: Vec<RouteEdge>,
    pub keys: Vec<SideKey>,
    /// `(sender, send)` for every send into a removed row.
    pub sends: Vec<(Node, Send)>,
}

/// Where an audited entry lives, so the reader can remove exactly the
/// entries [`DeskModel::check`] lists and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Locus {
    Row(usize),
    Plugin(SeatBay),
    Bypass(SeatBay),
    Route(usize),
    Key(usize),
    /// A send: the sender (`None` row = a special) and its index.
    Send(Sender, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Sender {
    Row(usize),
    Special(Node),
}

impl DeskModel {
    /// A desk with no rows.
    pub fn empty() -> Self {
        DeskModel {
            next_id: 0,
            rows: Vec::new(),
            specials: Specials::default(),
            plugins: BTreeMap::new(),
            bypass: BTreeSet::new(),
            routes: Vec::new(),
            keys: Vec::new(),
            legacy_states: None,
        }
    }

    /// The lane a track sits in. Linear: the cached index is `DeskIndex`'s
    /// job, and this is the reference it is checked against.
    pub fn row_of(&self, id: TrackId) -> Option<usize> {
        self.rows.iter().position(|r| r.id == id)
    }

    /// Every row's id, in lane order.
    pub fn ids(&self) -> Vec<TrackId> {
        self.rows.iter().map(|r| r.id).collect()
    }

    /// **A fresh id**: never held by any row this desk has had, because
    /// `next_id` only rises. `None` once the u32 space is spent, which only
    /// a hostile file can arrange; a caller treats it as a refusal.
    pub fn mint(&mut self) -> Option<TrackId> {
        let id = u32::try_from(self.next_id).ok()?;
        self.next_id += 1;
        Some(TrackId(id))
    }

    /// Whether `node` names something on this desk: a special always, a
    /// track while its row is here.
    pub fn has(&self, node: Node) -> bool {
        match node {
            Node::Track(id) => self.row_of(id).is_some(),
            _ => true,
        }
    }

    // ── folder runs ──────────────────────────────────────────────────────

    /// **The folder row `row` belongs to**: the nearest `folder` row above
    /// it, with only members between. The run has to be contiguous, the
    /// rule `TangentApp::folder_of` has always used: a member flag with a
    /// plain row between it and any folder belongs to nothing, so a stale
    /// flag draws flat instead of letting one reorder swallow every track
    /// below a folder.
    pub fn folder_of(&self, row: usize) -> Option<usize> {
        if !self.rows.get(row)?.in_folder {
            return None;
        }
        for n in (0..row).rev() {
            if self.rows[n].folder {
                return Some(n);
            }
            if !self.rows[n].in_folder {
                return None;
            }
        }
        None
    }

    /// **Every row that says it is a member and has no folder above it**:
    /// a run some edit or some older build broke. Empty for a desk whose
    /// runs are whole. Not part of [`Self::check`], deliberately: a stale
    /// flag names nothing, today's app already draws it flat, and the
    /// reader must carry it byte for byte (a dense desk round-trips whole),
    /// so it is not an entry to drop. What the edits promise is narrower and
    /// checked under cfg(test): none of them ever makes a new one.
    pub fn orphans(&self) -> Vec<TrackId> {
        let lanes = folder_lanes(self.rows.iter());
        self.rows
            .iter()
            .zip(lanes)
            .filter(|(r, f)| r.in_folder && f.is_none())
            .map(|(r, _)| r.id)
            .collect()
    }

    // ── classes ──────────────────────────────────────────────────────────

    /// The class of row `row`, or `None` past the desk.
    pub fn class(&self, row: usize, uses: RowUse) -> Option<RowClass> {
        let r = self.rows.get(row)?;
        let node = Node::Track(r.id);
        let endpoint = self.routes.iter().any(|e| e.from == node || e.to == RouteTo::Node(node))
            || self.keys.iter().any(|k| k.seat.seat.node == node || k.from == node)
            || self.sends_from().any(|(_, s)| s.to == node);
        let chain = self
            .plugins
            .keys()
            .any(|sb| sb.seat.node == node && matches!(sb.seat.rack, Rack::Chain(_)));
        Some(classify(r, uses, endpoint || chain))
    }

    /// Every row's class in one pass over the references, for the callers
    /// that want them all (`uses[row]`, missing entries reading as unused).
    pub fn classes(&self, uses: &[RowUse]) -> Vec<RowClass> {
        let mut holds: BTreeSet<TrackId> = BTreeSet::new();
        let mut note = |n: Node| {
            if let Node::Track(id) = n {
                holds.insert(id);
            }
        };
        for e in &self.routes {
            note(e.from);
            if let RouteTo::Node(n) = e.to {
                note(n);
            }
        }
        for k in &self.keys {
            note(k.seat.seat.node);
            note(k.from);
        }
        for (_, s) in self.sends_from() {
            note(s.to);
        }
        for sb in self.plugins.keys() {
            if matches!(sb.seat.rack, Rack::Chain(_)) {
                note(sb.seat.node);
            }
        }
        self.rows
            .iter()
            .enumerate()
            .map(|(n, r)| classify(r, uses.get(n).copied().unwrap_or_default(), holds.contains(&r.id)))
            .collect()
    }

    /// **How many tracks this desk holds against the cap**: the Counted
    /// rows. Buses, placeholders and vacant rows are free on every tier.
    pub fn counted(&self, uses: &[RowUse]) -> usize {
        self.classes(uses).into_iter().filter(|c| *c == RowClass::Counted).count()
    }

    /// Every send on the desk, with its sender.
    fn sends_from(&self) -> impl Iterator<Item = (Sender, &Send)> {
        let rows = self
            .rows
            .iter()
            .enumerate()
            .flat_map(|(n, r)| r.sends.iter().map(move |s| (Sender::Row(n), s)));
        let specials = self
            .specials
            .iter()
            .flat_map(|(node, sp)| sp.sends.iter().map(move |s| (Sender::Special(node), s)));
        rows.chain(specials)
    }

    // ── structural edits ─────────────────────────────────────────────────
    //
    // The ONLY four ways the row list changes shape. Each refuses before it
    // changes anything, each answers a `Reshape` for what lives outside the
    // model, and under cfg(test) each proves it left no reference dangling
    // that was not dangling before (`audit_after`).

    /// Add `row` at the end, answering its lane.
    pub fn append(&mut self, row: TrackRow) -> Result<usize, EditRefused> {
        let at = self.rows.len();
        self.insert_rows(at, vec![row])?;
        Ok(at)
    }

    /// **Insert `rows` at lane `at`**, every lane from `at` down moving by
    /// their count. The rows keep the ids they carry (an undo or a paste
    /// within the document restores its own), so every id must be new to
    /// the desk and to the batch; `next_id` rises past any of them.
    ///
    /// **The new rows' flags say which run they join** (a member flag below
    /// a folder or one of its members joins it), and they may not re-file
    /// anybody else. A plain row dropped into the middle of a run would cut
    /// the members below it loose, and a folder row there would take them
    /// over; both are refused with [`EditRefused::BreaksFolder`] naming the
    /// first row that would move, as is a new member with no folder above
    /// it. Inserting is unambiguous about what moved, so this can be strict.
    pub fn insert_rows(&mut self, at: usize, rows: Vec<TrackRow>) -> Result<Reshape, EditRefused> {
        let old = self.rows.len();
        if at > old {
            return Err(EditRefused::NoSuchRow(at));
        }
        if old.saturating_add(rows.len()) > MAX_ROWS {
            return Err(EditRefused::TooManyRows);
        }
        let mut seen: BTreeSet<TrackId> = self.rows.iter().map(|r| r.id).collect();
        for r in &rows {
            if !seen.insert(r.id) {
                return Err(EditRefused::DuplicateId(r.id));
            }
        }
        let k = rows.len();
        {
            let before = folder_ids(&self.rows.iter().collect::<Vec<_>>());
            let view: Vec<&TrackRow> =
                self.rows[..at].iter().chain(rows.iter()).chain(self.rows[at..].iter()).collect();
            let after = folder_ids(&view);
            for (n, r) in view.iter().enumerate() {
                let new = (at..at + k).contains(&n);
                let broken = if new {
                    r.in_folder && after[n].is_none()
                } else {
                    let was = if n < at { n } else { n - k };
                    after[n] != before[was]
                };
                if broken {
                    return Err(EditRefused::BreaksFolder(r.id));
                }
            }
        }
        #[cfg(test)]
        let before = self.audit_before();
        if let Some(top) = rows.iter().map(|r| u64::from(r.id.0) + 1).max() {
            self.next_id = self.next_id.max(top);
        }
        self.rows.splice(at..at, rows);
        #[cfg(test)]
        self.audit_after(before);
        Ok(Reshape {
            old_to_new: (0..old).map(|n| Some(if n < at { n } else { n + k })).collect(),
            refiled: Vec::new(),
        })
    }

    /// **Remove the rows at `lanes`** (any order, repeats ignored), and
    /// everything that names them: see [`Removed`]. The specials and every
    /// other row keep all their own references.
    ///
    /// **A folder takes its membership with it**, the rule
    /// `remove_channel_inner` has kept since folders existed, now that the
    /// row really goes. Membership is "the nearest folder above me", so
    /// deleting a header without touching its members would have them walk
    /// up into whatever folder sits further up (silently joining a group
    /// they were never put in), or, with none, draw indented under nothing.
    /// Each kept row instead stays in its nearest folder that survives: a
    /// removed inner folder's members land in the outer one, a removed
    /// top-level folder's go to the top level, and a nested folder among
    /// them stays a folder over its own members. Only `in_folder` flags are
    /// rewritten, and only where the walk would otherwise answer a
    /// different folder; the rows whose folder changed are in
    /// `reshape.refiled`.
    pub fn remove_rows(&mut self, lanes: &[usize]) -> Result<Removed, EditRefused> {
        let old = self.rows.len();
        let gone: BTreeSet<usize> = lanes.iter().copied().collect();
        if let Some(&bad) = gone.iter().find(|n| **n >= old) {
            return Err(EditRefused::NoSuchRow(bad));
        }
        #[cfg(test)]
        let before = self.audit_before();
        let ids: BTreeSet<TrackId> = gone.iter().map(|n| self.rows[*n].id).collect();
        let named = |n: Node| matches!(n, Node::Track(id) if ids.contains(&id));

        // Each row's folder before, and its nearest SURVIVING ancestor:
        // top-down, since a row's folder always sits above it, so one pass
        // settles a chain of removed nested headers however deep.
        let was = folder_lanes(self.rows.iter());
        let mut keeps: Vec<Option<usize>> = Vec::with_capacity(old);
        for f in &was {
            let up = match *f {
                Some(q) if gone.contains(&q) => keeps[q],
                other => other,
            };
            keeps.push(up);
        }
        let old_ids = self.ids();

        let mut rows = Vec::with_capacity(gone.len());
        let mut kept = Vec::with_capacity(old - gone.len());
        let mut old_to_new = Vec::with_capacity(old);
        let mut from = Vec::with_capacity(old - gone.len());
        for (n, row) in std::mem::take(&mut self.rows).into_iter().enumerate() {
            if gone.contains(&n) {
                old_to_new.push(None);
                rows.push((n, row));
            } else {
                old_to_new.push(Some(kept.len()));
                from.push(n);
                kept.push(row);
            }
        }
        self.rows = kept;

        // Rewrite the flags top-down, so each row is settled against the
        // rows above it as they now stand. Setting a member flag is enough
        // to reach the wanted folder: every kept row between that folder
        // and this one was in the same chain of runs, and every folder on
        // that chain below it was removed, so nothing else can catch it.
        let mut refiled = Vec::new();
        let mut reach: Option<usize> = None;
        for (lane, o) in from.iter().enumerate() {
            let want = keeps[*o].map(|q| old_ids[q]);
            let above = reach.map(|q| self.rows[q].id);
            let r = &mut self.rows[lane];
            // A row whose folder went is re-filed even when the walk happens
            // to answer right: with no folder left above it, a member flag
            // left on would be a stranded one.
            let refile = want != was[*o].map(|q| old_ids[q]);
            if refile || r.in_folder.then_some(above).flatten() != want {
                r.in_folder = want.is_some();
            }
            debug_assert_eq!(r.in_folder.then_some(above).flatten(), want, "a hand-up missed its folder");
            if refile {
                refiled.push(r.id);
            }
            reach = if r.folder {
                Some(lane)
            } else if r.in_folder {
                reach
            } else {
                None
            };
        }

        let (plugins, keep): (BTreeMap<_, _>, BTreeMap<_, _>) =
            std::mem::take(&mut self.plugins).into_iter().partition(|(sb, _)| named(sb.seat.node));
        self.plugins = keep;
        let (bypass, keep): (BTreeSet<_>, BTreeSet<_>) =
            std::mem::take(&mut self.bypass).into_iter().partition(|sb| named(sb.seat.node));
        self.bypass = keep;
        let (routes, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.routes)
            .into_iter()
            .partition(|e| named(e.from) || matches!(e.to, RouteTo::Node(n) if named(n)));
        self.routes = keep;
        let (keys, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.keys)
            .into_iter()
            .partition(|k| named(k.seat.seat.node) || named(k.from));
        self.keys = keep;
        let mut sends = Vec::new();
        for r in &mut self.rows {
            let node = Node::Track(r.id);
            r.sends.retain(|s| {
                let into = named(s.to);
                if into {
                    sends.push((node, *s));
                }
                !into
            });
        }
        for node in Node::SPECIALS {
            if let Some(sp) = self.specials.get_mut(node) {
                sp.sends.retain(|s| {
                    let into = named(s.to);
                    if into {
                        sends.push((node, *s));
                    }
                    !into
                });
            }
        }
        #[cfg(test)]
        self.audit_after(before);
        Ok(Removed {
            reshape: Reshape { old_to_new, refiled },
            rows,
            plugins,
            bypass,
            routes,
            keys,
            sends,
        })
    }

    /// **Reorder the rows**: `order[new] = old`, the convention every
    /// reorder in the app already uses. Every reference is by id, so the
    /// row list is all that changes.
    ///
    /// **Folder runs: refused if one strands a member, answered if one
    /// moves.** The flags travel with their rows and membership is read off
    /// the new order, so a permutation can leave a member under no folder:
    /// one child carried out of its run with its flag still set, a plain row
    /// dropped mid-run (every member below it is cut loose), a folder moved
    /// off its children. Each is refused with [`EditRefused::BreaksFolder`].
    /// It can also move a row into a different run, and that one the model
    /// cannot judge: "drag this child into that folder" and "drag that
    /// folder over this child" are the SAME permutation, and only the
    /// gesture knows which the hand did. So it is allowed, and every row it
    /// re-filed is listed in [`Reshape::refiled`] for the gesture to check
    /// against what it set the leaders' flags to.
    pub fn permute(&mut self, order: &[usize]) -> Result<Reshape, EditRefused> {
        let n = self.rows.len();
        if order.len() != n {
            return Err(EditRefused::NotAPermutation);
        }
        let mut old_to_new = vec![None; n];
        for (new, old) in order.iter().enumerate() {
            match old_to_new.get_mut(*old) {
                Some(slot @ None) => *slot = Some(new),
                _ => return Err(EditRefused::NotAPermutation),
            }
        }
        let mut refiled = Vec::new();
        {
            let before = folder_ids(&self.rows.iter().collect::<Vec<_>>());
            let view: Vec<&TrackRow> = order.iter().map(|old| &self.rows[*old]).collect();
            let after = folder_ids(&view);
            for (new, old) in order.iter().enumerate() {
                let r = view[new];
                if r.in_folder && after[new].is_none() && before[*old].is_some() {
                    return Err(EditRefused::BreaksFolder(r.id));
                }
                if after[new] != before[*old] {
                    refiled.push(r.id);
                }
            }
        }
        #[cfg(test)]
        let before = self.audit_before();
        let mut taken: Vec<Option<TrackRow>> = std::mem::take(&mut self.rows).into_iter().map(Some).collect();
        self.rows = order.iter().map(|old| taken[*old].take().expect("a permutation")).collect();
        #[cfg(test)]
        self.audit_after(before);
        Ok(Reshape { old_to_new, refiled })
    }

    /// What an edit is audited against: the dangling references and the
    /// stranded members the desk already had.
    #[cfg(test)]
    fn audit_before(&self) -> (Vec<Dropped>, BTreeSet<TrackId>) {
        (self.check(), self.orphans().into_iter().collect())
    }

    /// **An edit never makes a reference dangle, nor strands a member.**
    /// Weaker than "the desk is clean", deliberately: the model is pure and
    /// a test may build a desk that is dirty on purpose. W6 wires `check`
    /// and `orphans` into the undo sentinel, where the desk is the app's and
    /// must be clean.
    #[cfg(test)]
    fn audit_after(&self, (dropped, orphans): (Vec<Dropped>, BTreeSet<TrackId>)) {
        let after = self.check();
        let new: Vec<_> = after.iter().filter(|d| !dropped.contains(d)).collect();
        assert!(new.is_empty(), "a structural edit left references dangling: {new:?}");
        let stranded: Vec<_> = self.orphans().into_iter().filter(|id| !orphans.contains(id)).collect();
        assert!(stranded.is_empty(), "a structural edit stranded folder members: {stranded:?}");
    }

    // ── validation ───────────────────────────────────────────────────────

    /// **Every reference on this desk that names nothing**, one entry each:
    /// a plugin, bypass, route, key or send naming no row; a pedal or pad
    /// not on a track; a bay past the third; a chain at or past
    /// `MAX_CHAIN`; a second row with an id already held. Empty for a clean
    /// desk. The reader drops exactly these (all but a duplicate row, which
    /// refuses the desk instead).
    pub fn check(&self) -> Vec<Dropped> {
        self.audit().into_iter().map(|(_, d)| d).collect()
    }

    /// **Remove every entry `check` lists, and answer them.** The reader's
    /// half of the error policy: one bad entry costs that entry. A
    /// duplicate row is listed and left, because removing either copy would
    /// be a guess; the reader refuses such a desk before it gets here.
    pub(crate) fn prune(&mut self) -> Vec<Dropped> {
        let found = self.audit();
        let mut routes = BTreeSet::new();
        let mut keys = BTreeSet::new();
        let mut sends: BTreeSet<(Sender, usize)> = BTreeSet::new();
        for (locus, _) in &found {
            match *locus {
                Locus::Row(_) => {}
                Locus::Plugin(sb) => {
                    self.plugins.remove(&sb);
                }
                Locus::Bypass(sb) => {
                    self.bypass.remove(&sb);
                }
                Locus::Route(i) => {
                    routes.insert(i);
                }
                Locus::Key(i) => {
                    keys.insert(i);
                }
                Locus::Send(who, i) => {
                    sends.insert((who, i));
                }
            }
        }
        drop_at(&mut self.routes, |i| routes.contains(&i));
        drop_at(&mut self.keys, |i| keys.contains(&i));
        for (n, r) in self.rows.iter_mut().enumerate() {
            drop_at(&mut r.sends, |i| sends.contains(&(Sender::Row(n), i)));
        }
        for node in Node::SPECIALS {
            if let Some(sp) = self.specials.get_mut(node) {
                drop_at(&mut sp.sends, |i| sends.contains(&(Sender::Special(node), i)));
            }
        }
        found.into_iter().map(|(_, d)| d).collect()
    }

    /// The one walk `check` and `prune` share, so what is listed is exactly
    /// what is removed.
    fn audit(&self) -> Vec<(Locus, Dropped)> {
        let mut ids = BTreeSet::new();
        let mut out = Vec::new();
        for (n, r) in self.rows.iter().enumerate() {
            if !ids.insert(r.id) {
                out.push((Locus::Row(n), dropped("row", r.id.to_string())));
            }
        }
        let has = |node: Node| match node {
            Node::Track(id) => ids.contains(&id),
            _ => true,
        };
        let seat_ok = |sb: SeatBay| sb.is_valid() && sb.seat.rack.in_reach() && has(sb.seat.node);
        for (sb, _) in &self.plugins {
            if !seat_ok(*sb) {
                out.push((Locus::Plugin(*sb), dropped("plugin", sb.to_string())));
            }
        }
        for sb in &self.bypass {
            if !seat_ok(*sb) {
                out.push((Locus::Bypass(*sb), dropped("bypass", sb.to_string())));
            }
        }
        for (i, e) in self.routes.iter().enumerate() {
            let to_ok = match e.to {
                RouteTo::Node(n) => has(n),
                RouteTo::Hardware { .. } => true,
            };
            if !has(e.from) || !to_ok {
                out.push((Locus::Route(i), dropped("route", route_text(e))));
            }
        }
        for (i, k) in self.keys.iter().enumerate() {
            if !seat_ok(k.seat) || !has(k.from) {
                out.push((Locus::Key(i), dropped("key", format!("{}<{}", k.seat, k.from))));
            }
        }
        let mut per_sender: BTreeMap<Sender, usize> = BTreeMap::new();
        for (who, s) in self.sends_from() {
            let i = per_sender.entry(who).or_default();
            if !has(s.to) {
                let from = match who {
                    Sender::Row(n) => Node::Track(self.rows[n].id),
                    Sender::Special(node) => node,
                };
                out.push((Locus::Send(who, *i), dropped("send", format!("{from}>{}", s.to))));
            }
            *i += 1;
        }
        out
    }

    // ── the dense 32-wide shape ──────────────────────────────────────────

    /// **A dense legacy desk, as rows**: row r is lane r of `v`, with id
    /// `ids[r]`. A migrated desk passes `desk_legacy::identity_ids(32)`
    /// (legacy row r is `TrackId(r)` in every era); the persistence lane
    /// passes the live ids once they exist. `ids` must be distinct.
    ///
    /// Answers a `DeskRead` rather than a bare model, because the dense
    /// shape can say things the model cannot hold (a bay past the seat map,
    /// a route to a lane past the ids) and the error policy is that each
    /// such entry is dropped AND listed, never lost in silence. The caller
    /// sets `legacy_states` for a document; `next_id` is one past the
    /// highest id, which the caller may raise.
    pub fn from_v22(v: &V22Desk, ids: &[TrackId]) -> crate::desk_format::DeskRead {
        debug_assert_eq!(
            ids.iter().collect::<BTreeSet<_>>().len(),
            ids.len(),
            "from_v22 was handed a repeated id"
        );
        let mut lost = Vec::new();
        let bit = |mask: u64, at: usize| at < 64 && (mask >> at) & 1 == 1;
        let bit32 = |mask: u32, at: usize| at < 32 && (mask >> at) & 1 == 1;
        let mut rows = Vec::with_capacity(ids.len());
        for (r, id) in ids.iter().enumerate() {
            let mut row = TrackRow::vacant(*id);
            // A row past the dense width has nothing written down for it.
            if r < V32_CHANNELS {
                row.kind = v.kinds.get(r).and_then(|b| RowKind::from_byte(*b));
                row.added = bit32(v.tracks_added, r);
                row.icon = v.icons.get(r).copied().unwrap_or(0);
                row.gain = v.gains.get(r).copied().unwrap_or(1.0);
                row.pan = v.pans.get(r).copied().unwrap_or(0.0);
                row.input = v.inputs.get(r).cloned().unwrap_or_default();
                row.midi = v.midi_inputs.get(r).cloned().unwrap_or(MidiInputSel::EVERYTHING);
                row.arm = bit32(v.armed, r);
                row.theory = v.to_theory.get(r).copied().unwrap_or(false);
                row.folder = v.is_folder.get(r).copied().unwrap_or(false);
                row.in_folder = v.in_folder.get(r).copied().unwrap_or(false);
                row.shut = v.folder_shut.get(r).copied().unwrap_or(false);
                row.dx7 = Dx7 {
                    cart: v.dx7_cartridge.get(r).cloned().unwrap_or_default(),
                    patch: v.dx7_patch.get(r).copied().unwrap_or(0),
                };
                row.keys.patch = v.keys_patch.get(r).copied().unwrap_or(0);
                if let Some(p) = v.keys_params.get(r * KEYS_SPAN..(r + 1) * KEYS_SPAN) {
                    row.keys.params = p.to_vec();
                }
                row.frozen = v.frozen.get(r).cloned().flatten();
                // Strip space: the frozen map puts row r at strip r.
                let s = encode_v32_strip(LegacyOwner::Row(r as u16)).expect("a V32 row has a strip");
                row.name = v.names.get(s).cloned().unwrap_or_default();
                row.color = v.colors.get(s).copied().unwrap_or(0);
                if let Some(level) = v.sends.get(s) {
                    set_fx_send_level(&mut row.sends, *level);
                }
                row.mute = bit(v.muted, s);
                row.solo = bit(v.soloed, s);
                row.phase = bit(v.phase, s);
            }
            rows.push(row);
        }

        // The specials at their legacy strips, through the one formulation
        // of where those are (`desk_legacy`, Appendix A).
        let mut specials = Specials::default();
        crate::desk_legacy::read_specials(
            &mut specials,
            &v.names,
            &v.colors,
            &v.sends,
            (v.muted, v.soloed, Some(v.phase)),
        );

        let seat = |flat: usize| -> Option<SeatBay> {
            let (owner, rack, bay) = decode_v32(flat)?;
            Some(SeatBay::new(bind(owner, ids)?, rack, bay))
        };
        let mut plugins = BTreeMap::new();
        for (flat, path) in &v.inserts {
            if path.is_empty() {
                continue;
            }
            match seat(*flat) {
                Some(sb) => {
                    plugins.insert(sb, path.clone());
                }
                None => lost.push(dropped("plugin", format!("bay {flat}"))),
            }
        }
        let mut bypass = BTreeSet::new();
        for (s, b) in &v.bypass {
            match (*b < BAYS).then(|| s.checked_mul(BAYS)?.checked_add(*b)).flatten().and_then(seat) {
                Some(sb) => {
                    bypass.insert(sb);
                }
                None => lost.push(dropped("bypass", format!("seat {s} bay {b}"))),
            }
        }
        let lane = |n: usize| ids.get(n).map(|id| Node::Track(*id));
        let mut routes = Vec::new();
        for r in &v.routes {
            let to = match r.target {
                LaneTarget::Channel(n) => lane(n).map(RouteTo::Node),
                LaneTarget::Hardware { first, stereo } => Some(RouteTo::Hardware { first, stereo }),
            };
            match (lane(r.from), to) {
                (Some(from), Some(to)) => routes.push(RouteEdge { from, to, gain: r.gain }),
                _ => lost.push(dropped("route", format!("{:?}", r))),
            }
        }
        let mut keys = Vec::new();
        for k in &v.keys {
            let node = decode_v32_strip(k.strip).and_then(|o| bind(o, ids));
            match (node.and_then(|n| slot_key(n, k.slot)), lane(k.from)) {
                (Some(seat), Some(from)) => keys.push(SideKey { seat, from }),
                _ => lost.push(dropped("key", format!("{:?}", k))),
            }
        }

        let next_id = ids.iter().map(|id| u64::from(id.0) + 1).max().unwrap_or(0);
        let mut model = DeskModel {
            next_id,
            rows,
            specials,
            plugins,
            bypass,
            routes,
            keys,
            legacy_states: None,
        };
        lost.extend(model.prune());
        crate::desk_format::DeskRead { model, dropped: lost }
    }

    /// **A short desk's ids, filled out to `width` rows** (the dense live
    /// desk, until W6): each row the file never named gets an id NO row of
    /// this desk holds, and `next` stays above every id minted from it.
    ///
    /// Minted from `next` while it fits a u32, which `desk_format::read`
    /// already keeps above every id it read. Past that the counter is
    /// spent, and only a hand-made file arranges it: one row at
    /// `t4294967295` is enough, because the reader raises `next_id` to one
    /// past it. Clamping to `u32::MAX` there gave every padded row the SAME
    /// id as each other and as that row, and the two views of the desk then
    /// parted: `desk_ids` saw duplicates and answered identity, while the
    /// row signals and the kit writer used them as they were, so several
    /// lanes wrote one kit key and all but the last kit was lost. So the
    /// lowest ids no row holds fill the rest. Not refused: the desk reads
    /// whole, and refusing a song for its counter would cost the song.
    pub fn pad_ids(ids: &mut Vec<TrackId>, next: &mut u64, width: usize) {
        let mut free = 0u32;
        while ids.len() < width {
            let id = match u32::try_from(*next) {
                Ok(id) => {
                    *next += 1;
                    TrackId(id)
                }
                Err(_) => {
                    // At most `width` ids are held, so this ends long
                    // before the u32 space does.
                    while ids.contains(&TrackId(free)) {
                        free += 1;
                    }
                    TrackId(free)
                }
            };
            ids.push(id);
        }
    }
}

/// **The live routing table in the model's spelling**: each route by
/// identity at both ends, in the order it was made. The live desk keeps it as
/// `routing::Routes` (the routing dialog's and the engine bridge's shape);
/// these four are the one road between the two, so a desk read into the
/// model and written back out names every route and key as it did.
pub fn route_edges(routes: &crate::routing::Routes) -> Vec<RouteEdge> {
    use crate::routing::Target;
    routes
        .0
        .iter()
        .map(|r| RouteEdge {
            from: r.from,
            to: match r.target {
                Target::Node(n) => RouteTo::Node(n),
                Target::Hardware { first, stereo } => RouteTo::Hardware { first, stereo },
            },
            gain: r.gain,
        })
        .collect()
}

/// [`route_edges`]' inverse.
pub fn routes_of(edges: &[RouteEdge]) -> crate::routing::Routes {
    use crate::routing::{Route, Target};
    crate::routing::Routes(
        edges
            .iter()
            .map(|e| Route {
                from: e.from,
                target: match e.to {
                    RouteTo::Node(n) => Target::Node(n),
                    RouteTo::Hardware { first, stereo } => Target::Hardware { first, stereo },
                },
                gain: e.gain,
            })
            .collect(),
    )
}

/// **The live sidechain keys in the model's spelling**, one per bay.
pub fn side_keys(keys: &crate::routing::KeyEdges) -> Vec<SideKey> {
    keys.0.iter().map(|k| SideKey { seat: k.seat, from: k.from }).collect()
}

/// [`side_keys`]' inverse.
pub fn key_edges_of(keys: &[SideKey]) -> crate::routing::KeyEdges {
    crate::routing::KeyEdges(keys.iter().map(|k| crate::routing::KeyEdge { seat: k.seat, from: k.from }).collect())
}

fn classify(r: &TrackRow, uses: RowUse, holds: bool) -> RowClass {
    if r.kind == Some(RowKind::Bus) {
        RowClass::Bus
    } else if r.kind.is_some() || r.added || r.folder || uses.clips {
        RowClass::Counted
    } else if r.in_folder || uses.curve || uses.kit || r.frozen.is_some() || holds {
        RowClass::Placeholder
    } else {
        RowClass::Vacant
    }
}

/// Remove the entries of `v` whose index `gone` names, keeping the order.
fn drop_at<T>(v: &mut Vec<T>, gone: impl Fn(usize) -> bool) {
    let mut i = 0;
    v.retain(|_| {
        let keep = !gone(i);
        i += 1;
        keep
    });
}

/// **The folder lane each row sits in**, in one walk over `rows` in
/// order: [`DeskModel::folder_of`] for every row at once. `reach` is the
/// nearest folder above the next row with only members between, so a run
/// ends at the first row that is neither.
fn folder_lanes<'a>(rows: impl Iterator<Item = &'a TrackRow>) -> Vec<Option<usize>> {
    let mut out = Vec::new();
    let mut reach: Option<usize> = None;
    for (n, r) in rows.enumerate() {
        out.push(if r.in_folder { reach } else { None });
        reach = if r.folder {
            Some(n)
        } else if r.in_folder {
            reach
        } else {
            None
        };
    }
    out
}

/// The same walk answered by identity, so a desk before an edit and the
/// rows after it can be compared row by row whatever moved.
fn folder_ids(rows: &[&TrackRow]) -> Vec<Option<TrackId>> {
    folder_lanes(rows.iter().copied()).into_iter().map(|f| f.map(|q| rows[q].id)).collect()
}

fn dropped(what: &'static str, key: String) -> Dropped {
    Dropped { what, key }
}

/// A route's text for a dropped-entry notice: `t3>t7`, `t2>hw4`.
pub(crate) fn route_text(e: &RouteEdge) -> String {
    match e.to {
        RouteTo::Node(n) => format!("{}>{n}", e.from),
        RouteTo::Hardware { first, .. } => format!("{}>hw{first}", e.from),
    }
}

/// **The dense 32-wide desk**: the per-channel and per-strip arrays every
/// legacy settings file and document spelt the desk in, at the frozen V32
/// widths and no other, so this type means the same thing whatever the live
/// desk becomes. The legacy readers gather a file's lifted arrays into one
/// of these and read it onto rows ONCE ([`DeskModel::from_v22`], under the
/// identity rule); nothing live keeps one.
///
/// Channel arrays are [`V32_CHANNELS`] long (`keys_params` that times
/// [`KEYS_SPAN`]), strip arrays 36 (names, colours) or 35 (sends; the
/// master has none), masks are bits of strip space. Seats are the V32 flat
/// map, sparse: `inserts[flat] = path`, `bypass` as `(seat, bay)`. Routes
/// and keys keep their legacy lane and strip numbers.
///
/// [`V22Desk::vacant`] is the canonical empty one.
#[derive(Debug, Clone, PartialEq)]
pub struct V22Desk {
    pub kinds: Vec<u8>,
    pub icons: Vec<u8>,
    pub gains: Vec<f64>,
    pub pans: Vec<f64>,
    pub inputs: Vec<String>,
    pub midi_inputs: Vec<MidiInputSel>,
    pub tracks_added: u32,
    pub armed: u32,
    pub is_folder: Vec<bool>,
    pub in_folder: Vec<bool>,
    pub folder_shut: Vec<bool>,
    pub to_theory: Vec<bool>,
    pub dx7_cartridge: Vec<String>,
    pub dx7_patch: Vec<usize>,
    pub keys_patch: Vec<usize>,
    pub keys_params: Vec<f64>,
    pub frozen: Vec<Option<Frozen>>,
    pub names: Vec<String>,
    pub colors: Vec<i64>,
    pub sends: Vec<f64>,
    pub muted: u64,
    pub soloed: u64,
    pub phase: u64,
    pub inserts: BTreeMap<usize, String>,
    pub bypass: BTreeSet<(usize, usize)>,
    pub routes: Vec<LaneRoute>,
    pub keys: Vec<LaneKey>,
}

impl V22Desk {
    /// Every channel vacant (the values [`TrackRow::vacant`] has), every
    /// strip unnamed and unpainted, nothing seated, routed or keyed.
    pub fn vacant() -> Self {
        let c = V32_CHANNELS;
        V22Desk {
            kinds: vec![0; c],
            icons: vec![0; c],
            gains: vec![1.0; c],
            pans: vec![0.0; c],
            inputs: vec![String::new(); c],
            midi_inputs: vec![MidiInputSel::EVERYTHING; c],
            tracks_added: 0,
            armed: 0,
            is_folder: vec![false; c],
            in_folder: vec![false; c],
            folder_shut: vec![false; c],
            to_theory: vec![false; c],
            dx7_cartridge: vec![String::new(); c],
            dx7_patch: vec![0; c],
            keys_patch: vec![0; c],
            keys_params: crate::dialogs::keys_params_default(c),
            frozen: vec![None; c],
            names: vec![String::new(); V32_STRIPS + 1],
            colors: vec![0; V32_STRIPS + 1],
            sends: vec![0.0; V32_STRIPS],
            muted: 0,
            soloed: 0,
            phase: 0,
            inserts: BTreeMap::new(),
            bypass: BTreeSet::new(),
            routes: Vec::new(),
            keys: Vec::new(),
        }
    }
}

/// **A desk as text, one fact per line, listing only what differs from a
/// vacant row**: the corpus goldens' picture. Every field of every row is
/// considered (an exhaustive destructure), so a field the picture forgot is
/// a compile error rather than a fact the goldens cannot see.
#[cfg(test)]
pub(crate) fn picture(m: &DeskModel) -> String {
    let mut out = vec![format!(
        "rows {} next_id {} legacy {}",
        m.rows.len(),
        m.next_id,
        m.legacy_states.map_or("-", DeskEra::name)
    )];
    let sends_text = |sends: &[Send]| {
        sends
            .iter()
            .map(|s| format!(" send {}{} {}", s.to, if s.pre { " pre" } else { "" }, s.level))
            .collect::<String>()
    };
    for r in &m.rows {
        let TrackRow {
            id,
            kind,
            added,
            icon,
            name,
            color,
            gain,
            pan,
            input,
            midi,
            mute,
            solo,
            phase,
            arm,
            theory,
            folder,
            in_folder,
            shut,
            sends,
            dx7,
            keys,
            frozen,
        } = r;
        let base = TrackRow::vacant(*id);
        let mut line = String::new();
        if let Some(k) = kind {
            line += &format!(" {}", k.name());
        }
        for (on, word) in [
            (*added, "added"),
            (*mute, "mute"),
            (*solo, "solo"),
            (*phase, "phase"),
            (*arm, "arm"),
            (*theory, "theory"),
            (*folder, "folder"),
            (*in_folder, "in_folder"),
            (*shut, "shut"),
        ] {
            if on {
                line += &format!(" {word}");
            }
        }
        if *icon != base.icon {
            line += &format!(" icon={icon}");
        }
        if *name != base.name {
            line += &format!(" name={name:?}");
        }
        if *color != base.color {
            line += &format!(" color={color}");
        }
        if *gain != base.gain {
            line += &format!(" gain={gain}");
        }
        if *pan != base.pan {
            line += &format!(" pan={pan}");
        }
        if *input != base.input {
            line += &format!(" input={input:?}");
        }
        if *midi != base.midi {
            line += &format!(" midi={:?}/{:?}", midi.devices, midi.channel);
        }
        line += &sends_text(sends);
        if *dx7 != base.dx7 {
            line += &format!(" dx7={:?}/{}", dx7.cart, dx7.patch);
        }
        if keys.patch != base.keys.patch {
            line += &format!(" keys={}", keys.patch);
        }
        for (i, (p, b)) in keys.params.iter().zip(&base.keys.params).enumerate() {
            if p != b {
                line += &format!(" k{i}={p}");
            }
        }
        if keys.params.len() != base.keys.params.len() {
            line += &format!(" klen={}", keys.params.len());
        }
        if let Some(f) = frozen {
            line += &format!(" frozen={:?}/{}", f.path, f.len_s);
        }
        if !line.is_empty() {
            out.push(format!("{id}{line}"));
        }
    }
    for (node, sp) in m.specials.iter() {
        let Special { name, color, sends, mute, solo, phase } = sp;
        let mut line = String::new();
        for (on, word) in [(*mute, "mute"), (*solo, "solo"), (*phase, "phase")] {
            if on {
                line += &format!(" {word}");
            }
        }
        if !name.is_empty() {
            line += &format!(" name={name:?}");
        }
        if *color != 0 {
            line += &format!(" color={color}");
        }
        line += &sends_text(sends);
        if !line.is_empty() {
            out.push(format!("{node}{line}"));
        }
    }
    let plugins: Vec<String> = m.plugins.iter().map(|(sb, p)| format!("{sb}={p}")).collect();
    out.push(format!("plugins {}", plugins.join(" ")));
    let bypass: Vec<String> = m.bypass.iter().map(ToString::to_string).collect();
    out.push(format!("bypass {}", bypass.join(" ")));
    let routes: Vec<String> = m.routes.iter().map(|e| format!("{}@{}", route_text(e), e.gain)).collect();
    out.push(format!("routes {}", routes.join(" ")));
    let keys: Vec<String> = m.keys.iter().map(|k| format!("{}<{}", k.seat, k.from)).collect();
    out.push(format!("keys {}", keys.join(" ")));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk_id::{Rack, MAX_CHAIN};
    use crate::routing::{LaneKey as KeyEdge, LaneRoute as Route, LaneTarget as Target};
    use crate::desk_legacy::identity_ids;

    fn t(n: u32) -> TrackId {
        TrackId(n)
    }

    fn sb(node: Node, rack: Rack, bay: u8) -> SeatBay {
        SeatBay::new(node, rack, bay)
    }

    /// A row that differs from vacant in every field but one, so a test
    /// that destructures it after an edit can see each one travel. The one
    /// is `in_folder`: a member flag on every row is a chain of folders each
    /// nested in the one above, which no reorder can leave whole, so these
    /// rows are empty top-level folders and the folder-run tests below
    /// carry `in_folder` loud instead.
    fn loud_row(id: u32) -> TrackRow {
        let n = f64::from(id);
        TrackRow {
            id: t(id),
            kind: Some(RowKind::Audio),
            added: true,
            icon: 7,
            name: format!("row {id}"),
            color: 1000 + i64::from(id),
            gain: 0.5 + n / 1000.0,
            pan: -0.25,
            input: format!("audio:{id}"),
            midi: MidiInputSel { devices: vec![format!("dev{id}")], channel: Some(3) },
            mute: true,
            solo: true,
            phase: true,
            arm: true,
            theory: true,
            folder: true,
            in_folder: false,
            shut: true,
            sends: vec![Send { to: Node::Fx, level: n / 100.0, pre: true }],
            dx7: Dx7 { cart: format!("cart{id}"), patch: 9 },
            keys: KeysSel { patch: 5, params: vec![n / 7.0; KEYS_SPAN] },
            frozen: Some(Frozen { path: format!("frozen{id}.wav"), len_s: n }),
        }
    }

    /// **Every field of a row travels with it**, through every structural
    /// edit. Exhaustive on purpose (no `..`): a field added to `TrackRow`
    /// fails this to compile until its author has said what the four edits
    /// do with it. A positional field (a lane, another row's index) would
    /// be wrong to carry blindly, and this is where that gets noticed.
    fn assert_row_is(row: &TrackRow, want: &TrackRow) {
        let TrackRow {
            id,
            kind,
            added,
            icon,
            name,
            color,
            gain,
            pan,
            input,
            midi,
            mute,
            solo,
            phase,
            arm,
            theory,
            folder,
            in_folder,
            shut,
            sends,
            dx7,
            keys,
            frozen,
        } = row;
        assert_eq!(*id, want.id);
        assert_eq!(*kind, want.kind);
        assert_eq!(*added, want.added);
        assert_eq!(*icon, want.icon);
        assert_eq!(*name, want.name);
        assert_eq!(*color, want.color);
        assert_eq!(gain.to_bits(), want.gain.to_bits());
        assert_eq!(pan.to_bits(), want.pan.to_bits());
        assert_eq!(*input, want.input);
        assert_eq!(*midi, want.midi);
        assert_eq!(
            (*mute, *solo, *phase, *arm, *theory, *folder, *in_folder, *shut),
            (want.mute, want.solo, want.phase, want.arm, want.theory, want.folder, want.in_folder, want.shut)
        );
        assert_eq!(*sends, want.sends);
        assert_eq!(*dx7, want.dx7);
        assert_eq!(*keys, want.keys);
        assert_eq!(*frozen, want.frozen);
    }

    /// The same guard over the desk: a field added to `DeskModel` fails to
    /// compile here until somebody has said how the edits treat it.
    fn desk_parts(m: &DeskModel) -> usize {
        let DeskModel { next_id: _, rows, specials: _, plugins, bypass, routes, keys, legacy_states: _ } = m;
        rows.len() + plugins.len() + bypass.len() + routes.len() + keys.len()
    }

    /// Five loud rows with a plugin on each, a route chain, a key and a
    /// send into a row: every reference kind, all valid.
    fn wired(n: u32) -> DeskModel {
        let mut m = DeskModel::empty();
        for id in 0..n {
            m.append(loud_row(id * 10)).expect("append");
        }
        for id in 0..n {
            let node = Node::Track(t(id * 10));
            m.plugins.insert(sb(node, Rack::Chain(0), 0), format!("/p{id}.vst3"));
            m.plugins.insert(sb(node, Rack::Pedal(2), 1), format!("/pedal{id}.vst3"));
        }
        m.bypass.insert(sb(Node::Track(t(10)), Rack::Chain(0), 0));
        m.routes.push(RouteEdge { from: Node::Track(t(0)), to: RouteTo::Node(Node::Track(t(20))), gain: 0.5 });
        m.routes.push(RouteEdge {
            from: Node::Track(t(30)),
            to: RouteTo::Hardware { first: 2, stereo: false },
            gain: 1.0,
        });
        m.keys.push(SideKey { seat: sb(Node::Track(t(20)), Rack::Chain(0), 0), from: Node::Track(t(40)) });
        m.rows[0].sends.push(Send { to: Node::Track(t(30)), level: 0.3, pre: false });
        m.specials.master.sends.push(Send { to: Node::Track(t(10)), level: 0.1, pre: false });
        m
    }

    #[test]
    fn append_and_insert_carry_every_field_and_move_every_lane_below() {
        let mut m = wired(5);
        let before: Vec<TrackRow> = m.rows.clone();
        let parts = desk_parts(&m);
        let r = m.insert_rows(2, vec![loud_row(100), loud_row(101)]).expect("insert");
        assert_eq!(r.old_to_new, vec![Some(0), Some(1), Some(4), Some(5), Some(6)]);
        assert_eq!(m.next_id, 102, "next_id rises past an inserted id");
        for (old, want) in before.iter().enumerate() {
            assert_row_is(&m.rows[r.lane(old).expect("kept")], want);
        }
        assert_row_is(&m.rows[2], &loud_row(100));
        assert_eq!(desk_parts(&m), parts + 2, "an insert touched a reference");
        assert!(m.check().is_empty());
        assert_eq!(m.append(loud_row(200)), Ok(7));
        assert_eq!(m.insert_rows(9, vec![loud_row(300)]), Err(EditRefused::NoSuchRow(9)));
        assert_eq!(
            m.insert_rows(0, vec![loud_row(300), loud_row(300)]),
            Err(EditRefused::DuplicateId(t(300)))
        );
        assert_eq!(m.insert_rows(0, vec![loud_row(10)]), Err(EditRefused::DuplicateId(t(10))));
        assert_eq!(m.rows.len(), 8, "a refused insert changed the desk");
    }

    #[test]
    fn permute_moves_rows_whole_and_names_nothing_new() {
        let mut m = wired(5);
        let before = m.clone();
        let r = m.permute(&[4, 2, 0, 1, 3]).expect("permute");
        assert_eq!(r.old_to_new, vec![Some(2), Some(3), Some(1), Some(4), Some(0)]);
        for (old, want) in before.rows.iter().enumerate() {
            assert_row_is(&m.rows[r.lane(old).expect("kept")], want);
        }
        // Every reference is by id, so nothing but the row order moved.
        assert_eq!(
            (&m.plugins, &m.bypass, &m.routes, &m.keys, &m.specials, m.next_id),
            (&before.plugins, &before.bypass, &before.routes, &before.keys, &before.specials, before.next_id)
        );
        assert_eq!(m.permute(&[0, 0, 1, 2, 3]), Err(EditRefused::NotAPermutation));
        assert_eq!(m.permute(&[0, 1, 2, 3]), Err(EditRefused::NotAPermutation));
        assert_eq!(m.permute(&[0, 1, 2, 3, 5]), Err(EditRefused::NotAPermutation));
    }

    #[test]
    fn remove_takes_the_rows_and_everything_naming_them_and_nothing_else() {
        let mut m = wired(5);
        let before = m.clone();
        let gone = m.remove_rows(&[3, 1, 3]).expect("remove");
        assert_eq!(gone.reshape.old_to_new, vec![Some(0), None, Some(1), None, Some(2)]);
        assert_eq!(gone.rows.iter().map(|(n, r)| (*n, r.id)).collect::<Vec<_>>(), vec![(1, t(10)), (3, t(30))]);
        for (old, want) in before.rows.iter().enumerate() {
            if let Some(new) = gone.reshape.lane(old) {
                // Every field travels, less the sends into a removed row.
                let mut want = want.clone();
                want.sends.retain(|s| ![Node::Track(t(10)), Node::Track(t(30))].contains(&s.to));
                assert_row_is(&m.rows[new], &want);
            }
        }
        // The pedalboards leave with their rows, and the bypass on t10.
        assert_eq!(gone.plugins.len(), 4);
        assert!(gone.plugins.keys().all(|sb| [t(10), t(30)].contains(&sb.seat.node.track().unwrap())));
        assert_eq!(gone.bypass.len(), 1);
        // The hardware route left from t30; the t0 > t20 route stays.
        assert_eq!(gone.routes.len(), 1);
        assert_eq!(m.routes, vec![before.routes[0]]);
        assert_eq!(m.keys, before.keys, "a key between two kept rows stayed");
        assert_eq!(
            gone.sends,
            vec![
                (Node::Track(t(0)), Send { to: Node::Track(t(30)), level: 0.3, pre: false }),
                (Node::Master, Send { to: Node::Track(t(10)), level: 0.1, pre: false }),
            ]
        );
        assert_eq!(m.rows[0].sends, loud_row(0).sends, "t0 kept its own fx send");
        assert!(m.check().is_empty(), "{:?}", m.check());
        assert_eq!(m.next_id, before.next_id, "a removed id is never handed out again");
        assert_eq!(m.mint(), Some(t(41)));
        assert_eq!(m.remove_rows(&[0, 9]).map(|_| ()), Err(EditRefused::NoSuchRow(9)));
        assert_eq!(m.rows.len(), 3, "a refused removal changed the desk");
    }

    #[test]
    fn mint_is_monotonic_and_says_when_the_space_is_spent() {
        let mut m = DeskModel::empty();
        assert_eq!((m.mint(), m.mint()), (Some(t(0)), Some(t(1))));
        m.next_id = u64::from(u32::MAX);
        assert_eq!(m.mint(), Some(t(u32::MAX)));
        assert_eq!(m.mint(), None);
        assert_eq!(m.next_id, u64::from(u32::MAX) + 1, "a refused mint moved the counter");
    }

    /// **`check` lists every kind of dangling reference**, one entry each,
    /// and a clean desk lists nothing.
    #[test]
    fn check_finds_every_dangling_reference() {
        let mut m = wired(5);
        assert!(m.check().is_empty(), "{:?}", m.check());
        let ghost = Node::Track(t(99));
        let row = Node::Track(t(0));
        m.plugins.insert(sb(ghost, Rack::Chain(0), 1), "/ghost.vst3".into());
        m.plugins.insert(sb(Node::Drums, Rack::Pedal(1), 0), "/pedal-on-a-bus.vst3".into());
        m.plugins.insert(sb(row, Rack::Chain(MAX_CHAIN as u16), 0), "/far.vst3".into());
        m.plugins.insert(sb(row, Rack::Chain(0), 3), "/bay3.vst3".into());
        m.plugins.insert(sb(Node::Master, Rack::Pad(4), 0), "/pad-on-master.vst3".into());
        m.bypass.insert(sb(ghost, Rack::Chain(1), 2));
        m.routes.push(RouteEdge { from: ghost, to: RouteTo::Node(row), gain: 1.0 });
        m.routes.push(RouteEdge { from: row, to: RouteTo::Node(ghost), gain: 1.0 });
        m.keys.push(SideKey { seat: sb(ghost, Rack::Chain(0), 0), from: row });
        m.keys.push(SideKey { seat: sb(row, Rack::Chain(0), 0), from: ghost });
        m.rows[1].sends.push(Send { to: ghost, level: 1.0, pre: false });
        m.specials.fx.sends.push(Send { to: ghost, level: 1.0, pre: false });
        let mut dup = loud_row(10);
        dup.name = "a second t10".into();
        m.rows.push(dup);
        let found: Vec<(&str, String)> = m.check().into_iter().map(|d| (d.what, d.key)).collect();
        let want = [
            ("row", "t10"),
            ("plugin", "t0-c0-b3"),
            ("plugin", "t0-c2-b0"),
            ("plugin", "t99-c0-b1"),
            ("plugin", "drums-p1-b0"),
            ("plugin", "master-n4-b0"),
            ("bypass", "t99-c1-b2"),
            ("route", "t99>t0"),
            ("route", "t0>t99"),
            ("key", "t99-c0-b0<t0"),
            ("key", "t0-c0-b0<t99"),
            ("send", "t10>t99"),
            ("send", "fx>t99"),
        ];
        assert_eq!(found, want.map(|(w, k)| (w, k.to_owned())).to_vec());
        // `prune` removes exactly what `check` lists, bar the duplicate row.
        let mut pruned = m.clone();
        assert_eq!(pruned.prune().len(), want.len());
        assert_eq!(pruned.check().len(), 1, "only the duplicate row is left to refuse");
        assert_eq!(pruned.plugins.len(), m.plugins.len() - 5);
        assert_eq!(pruned.routes, wired(5).routes, "only the two ghost routes went");
    }

    /// **Row classes match design section 4** on synthetic rows, a
    /// pedal-only row included.
    #[test]
    fn row_classes_follow_the_table() {
        let mut m = DeskModel::empty();
        let none = RowUse::default();
        let mut add = |f: &dyn Fn(&mut TrackRow)| {
            let mut r = TrackRow::vacant(TrackId(m.rows.len() as u32));
            f(&mut r);
            m.append(r).expect("append")
        };
        let vacant = add(&|_| {});
        let named = add(&|r| r.name = "a name and a colour are not a track".into());
        let pedal_only = add(&|_| {});
        let pad_only = add(&|_| {});
        // The header first: an edit refuses a member flag under no folder.
        let folder = add(&|r| r.folder = true);
        let indent = add(&|r| r.in_folder = true);
        let folder_child_track = add(&|r| {
            r.in_folder = true;
            r.kind = Some(RowKind::Midi);
        });
        let bus = add(&|r| r.kind = Some(RowKind::Bus));
        let bus_added = add(&|r| {
            r.kind = Some(RowKind::Bus);
            r.added = true;
        });
        let midi = add(&|r| r.kind = Some(RowKind::Midi));
        let audio = add(&|r| r.kind = Some(RowKind::Audio));
        let bare = add(&|r| r.added = true);
        let frozen = add(&|r| r.frozen = Some(Frozen { path: "f.wav".into(), len_s: 1.0 }));
        let chain = add(&|_| {});
        let routed_to = add(&|_| {});
        let keyed_from = add(&|_| {});
        let sent_to = add(&|_| {});
        let sender = add(&|r| r.sends.push(Send { to: Node::Fx, level: 1.0, pre: false }));
        let node = |n: usize| Node::Track(TrackId(n as u32));
        m.plugins.insert(sb(node(pedal_only), Rack::Pedal(3), 0), "/pedal.vst3".into());
        m.plugins.insert(sb(node(pad_only), Rack::Pad(40), 0), "/pad.vst3".into());
        m.plugins.insert(sb(node(chain), Rack::Chain(1), 2), "/fx.vst3".into());
        m.routes.push(RouteEdge { from: node(midi), to: RouteTo::Node(node(routed_to)), gain: 1.0 });
        m.keys.push(SideKey { seat: sb(node(midi), Rack::Chain(0), 0), from: node(keyed_from) });
        m.rows[audio].sends.push(Send { to: node(sent_to), level: 1.0, pre: false });

        let class = |n: usize, u: RowUse| m.class(n, u).expect("a row");
        use RowClass::*;
        let want = [
            (vacant, Vacant),
            (named, Vacant),
            (pedal_only, Vacant),
            (pad_only, Vacant),
            (indent, Placeholder),
            (bus, Bus),
            (bus_added, Bus),
            (midi, Counted),
            (audio, Counted),
            (bare, Counted),
            (folder, Counted),
            (folder_child_track, Counted),
            (frozen, Placeholder),
            (chain, Placeholder),
            (routed_to, Placeholder),
            (keyed_from, Placeholder),
            (sent_to, Placeholder),
            (sender, Vacant),
        ];
        for (n, c) in want {
            assert_eq!(class(n, none), c, "row {n}");
        }
        assert_eq!(class(vacant, RowUse { clips: true, ..none }), Counted, "clips make a track");
        assert_eq!(class(indent, RowUse { clips: true, ..none }), Counted);
        assert_eq!(class(bus, RowUse { clips: true, ..none }), Bus, "a bus is never counted");
        assert_eq!(class(vacant, RowUse { curve: true, ..none }), Placeholder);
        assert_eq!(class(vacant, RowUse { kit: true, ..none }), Placeholder);
        assert_eq!(m.class(m.rows.len(), none), None);
        // The bulk walk agrees with the row-at-a-time one.
        let mut uses = vec![none; m.rows.len()];
        uses[vacant] = RowUse { clips: true, ..none };
        let bulk = m.classes(&uses);
        for (n, c) in bulk.iter().enumerate() {
            assert_eq!(*c, class(n, uses[n]), "row {n}");
        }
        assert_eq!(m.counted(&uses), 6);
        assert_eq!(m.counted(&[]), 5, "missing uses read as unused");
    }

    /// **A vacant dense desk is 32 vacant rows**, and every vacant row
    /// reads as Vacant: a migrated empty desk counts nothing.
    #[test]
    fn a_vacant_dense_desk_is_thirty_two_vacant_rows() {
        let read = DeskModel::from_v22(&V22Desk::vacant(), &identity_ids(32));
        assert!(read.dropped.is_empty());
        let m = read.model;
        assert_eq!(m.rows.len(), 32);
        assert_eq!(m.next_id, 32);
        for (r, row) in m.rows.iter().enumerate() {
            assert_row_is(row, &TrackRow::vacant(t(r as u32)));
        }
        assert_eq!(m.specials, Specials::default());
        assert_eq!(m.counted(&[]), 0);
    }

    /// **What the dense shape cannot name is dropped and listed**: a bay
    /// past the seat map, a bypass on a fourth bay, a route to a lane past
    /// the ids, a key on a seventh slot.
    #[test]
    fn the_dense_shape_drops_and_lists_what_has_no_row() {
        let mut v = V22Desk::vacant();
        v.inserts.insert(13_176, "/past.vst3".into());
        v.inserts.insert(3, "/kept.vst3".into());
        v.bypass.insert((1, 3));
        v.bypass.insert((usize::MAX, 0));
        v.routes.push(Route::new(2, Target::Channel(40)));
        v.routes.push(Route::new(40, Target::Hardware { first: 0, stereo: true }));
        v.keys.push(KeyEdge { strip: 3, slot: 6, from: 1 });
        v.keys.push(KeyEdge { strip: 36, slot: 0, from: 1 });
        let read = DeskModel::from_v22(&v, &identity_ids(32));
        let what: Vec<&str> = read.dropped.iter().map(|d| d.what).collect();
        assert_eq!(what, ["plugin", "bypass", "bypass", "route", "route", "key", "key"]);
        assert_eq!(read.model.plugins.len(), 1);
        assert!(read.model.check().is_empty());
    }

    /// **The dense round trip is the identity on a desk that uses every
    /// field**, specials, seats, routes and keys included.
    #[test]
    fn a_dense_desk_round_trips_whole() {
        let mut v = V22Desk::vacant();
        for r in 0..32 {
            v.kinds[r] = (r % 4) as u8;
            v.icons[r] = r as u8;
            v.gains[r] = 0.01 * r as f64;
            v.pans[r] = -0.5;
            v.inputs[r] = format!("midi:port{r}");
            v.midi_inputs[r] = MidiInputSel { devices: vec![format!("port{r}")], channel: Some((r % 16) as u8) };
            v.is_folder[r] = r % 5 == 0;
            v.in_folder[r] = r % 5 == 1;
            v.folder_shut[r] = r % 10 == 0;
            v.to_theory[r] = r % 3 == 0;
            v.dx7_cartridge[r] = format!("c{r}.syx");
            v.dx7_patch[r] = r;
            v.keys_patch[r] = r % 8;
            v.keys_params[r * KEYS_SPAN + r] = 0.123;
            v.frozen[r] = (r % 7 == 0).then(|| Frozen { path: format!("f{r}.wav"), len_s: r as f64 });
        }
        for s in 0..36 {
            v.names[s] = format!("s{s}");
            v.colors[s] = 100 + s as i64;
        }
        for s in 0..35 {
            v.sends[s] = if s % 2 == 0 { 0.0 } else { s as f64 / 100.0 };
        }
        v.sends[4] = -0.0;
        v.tracks_added = 0xdead_beef;
        v.armed = 0x8000_0001;
        v.muted = 0xf_0000_0001;
        v.soloed = 0xa_8000_0000;
        v.phase = 0x9_0000_0010;
        for flat in [0, 3, 96, 105, 107, 108, 492, 600, 12_400, 13_100, 13_175] {
            v.inserts.insert(flat, format!("/b{flat}.vst3"));
        }
        v.bypass.extend([(0, 0), (35, 2), (164, 1), (4_391, 2)]);
        v.routes.push(Route::new(3, Target::Channel(31)));
        v.routes.push(Route { from: 31, target: Target::Hardware { first: 6, stereo: false }, gain: 0.25 });
        v.keys.push(KeyEdge { strip: 35, slot: 1, from: 2 });
        v.keys.push(KeyEdge { strip: 35, slot: 4, from: 3 });
        v.keys.push(KeyEdge { strip: 33, slot: 5, from: 4 });
        v.keys.push(KeyEdge { strip: 7, slot: 3, from: 31 });
        let read = DeskModel::from_v22(&v, &identity_ids(32));
        assert!(read.dropped.is_empty(), "{:?}", read.dropped);
        // **Every field lands on its row**, read field by field: the dense
        // desk is a read-only shape since the live desk became rows (W6),
        // so the way back (`to_v22`) is gone and the read is checked against
        // the arrays it was given instead.
        let m = &read.model;
        assert_eq!(m.rows.len(), 32);
        let bit = |w: u64, r: usize| (w >> r) & 1 == 1;
        for (r, row) in m.rows.iter().enumerate() {
            assert_eq!(row.id, t(r as u32));
            assert_eq!(row.kind, RowKind::from_byte((r % 4) as u8), "row {r} kind");
            assert_eq!(row.icon, r as u8);
            assert_eq!(row.gain.to_bits(), (0.01 * r as f64).to_bits());
            assert_eq!(row.pan, -0.5);
            assert_eq!(row.input, format!("midi:port{r}"));
            assert_eq!(row.midi, v.midi_inputs[r]);
            assert_eq!((row.folder, row.in_folder, row.shut), (r % 5 == 0, r % 5 == 1, r % 10 == 0 && r % 5 == 0));
            assert_eq!(row.theory, r % 3 == 0);
            assert_eq!((row.dx7.cart.as_str(), row.dx7.patch), (format!("c{r}.syx").as_str(), r));
            assert_eq!(row.keys.patch, r % 8);
            assert_eq!(row.keys.params[r], 0.123);
            assert_eq!(row.frozen, v.frozen[r]);
            assert_eq!((row.name.as_str(), row.color), (format!("s{r}").as_str(), 100 + r as i64));
            assert_eq!(row.added, bit(0xdead_beef, r), "row {r} added");
            assert_eq!(row.arm, bit(0x8000_0001, r), "row {r} arm");
            assert_eq!((row.mute, row.solo, row.phase), (bit(0xf_0000_0001, r), bit(0xa_8000_0000, r), bit(0x9_0000_0010, r)));
            assert_eq!(fx_send_level(&row.sends), if r % 2 == 0 { 0.0 } else { r as f64 / 100.0 }, "row {r} send");
        }
        // The specials, off their frozen strips 32..=35.
        let sp = &m.specials;
        assert_eq!([sp.drums.name.as_str(), sp.click.name.as_str(), sp.fx.name.as_str(), sp.master.name.as_str()], ["s32", "s33", "s34", "s35"]);
        assert_eq!([sp.drums.color, sp.click.color, sp.fx.color, sp.master.color], [132, 133, 134, 135]);
        assert_eq!([sp.drums.mute, sp.click.mute, sp.fx.mute, sp.master.mute], [true, true, true, true]);
        assert_eq!([sp.drums.solo, sp.click.solo, sp.fx.solo, sp.master.solo], [false, true, false, true]);
        assert_eq!([sp.drums.phase, sp.click.phase, sp.fx.phase, sp.master.phase], [true, false, false, true]);
        assert_eq!(fx_send_level(&sp.click.sends), 0.33);
        // Every seat, bypass, route and key found its row's track.
        assert_eq!(m.plugins.len(), 11);
        assert_eq!(m.bypass.len(), 4);
        assert_eq!(m.routes.len(), 2);
        assert_eq!(m.keys.len(), 4);
        assert!(m.check().is_empty());
        // The master's keys sit on the racks its DISPLAYED slots show:
        // slot 1 is the extra rack, slot 4 the original.
        let master_keys: Vec<String> =
            read.model.keys.iter().take(2).map(|k| k.seat.to_string()).collect();
        assert_eq!(master_keys, ["master-c1-b1", "master-c0-b1"]);
    }

    /// **Rows bind through the ids handed in**, not through their lane: a
    /// permuted id list puts every plugin, route and key on the same ROWS.
    #[test]
    fn dense_rows_bind_through_the_ids_they_are_given() {
        let mut v = V22Desk::vacant();
        v.inserts.insert(0, "/row0.vst3".into());
        v.routes.push(Route::new(0, Target::Channel(1)));
        let ids: Vec<TrackId> = (0..32).map(|n| t(100 + 31 - n)).collect();
        let m = DeskModel::from_v22(&v, &ids).model;
        assert_eq!(m.next_id, 132);
        assert_eq!(m.plugins.keys().next().map(ToString::to_string).as_deref(), Some("t131-c0-b0"));
        assert_eq!(route_text(&m.routes[0]), "t131>t130");
    }

    /// **The model holds rows past the legacy width**, as many as the desk
    /// grows to (W6: there is no dense shape to stop at any more), at every
    /// width the design names, and every structural edit there leaves it
    /// naming only what is on it.
    #[test]
    fn the_model_holds_rows_past_the_legacy_width() {
        for width in [1usize, 32, 33, 64, 65, 200, 512] {
            let mut m = DeskModel::empty();
            for n in 0..width {
                let at = m.append(TrackRow::vacant(t(n as u32))).expect("append");
                assert_eq!(at, n);
            }
            assert_eq!(m.rows.len(), width);
            assert_eq!(m.next_id, width as u64);
            // An insert above the last, a removal above it and a reorder,
            // each by identity.
            let last = t(width as u32 - 1);
            let fresh = t(width as u32);
            m.insert_rows(0, vec![TrackRow::vacant(fresh)]).expect("insert");
            assert_eq!(m.row_of(last), Some(width));
            m.remove_rows(&[0]).expect("remove");
            assert_eq!(m.row_of(last), Some(width - 1));
            let order: Vec<usize> = (0..width).rev().collect();
            m.permute(&order).expect("permute");
            assert_eq!(m.row_of(last), Some(0));
            assert!(m.check().is_empty(), "width {width}: {:?}", m.check());
        }
    }

    // ── folder runs ──────────────────────────────────────────────────────

    /// A desk spelled one letter a row, ids 0, 1, 2... in lane order:
    /// `F` a top-level folder, `N` a folder nested in the run above it, `m`
    /// a member, `p` a plain row.
    fn runs(spec: &str) -> DeskModel {
        let mut m = DeskModel::empty();
        for (n, c) in spec.chars().enumerate() {
            let mut r = TrackRow::vacant(t(n as u32));
            r.kind = Some(RowKind::Midi);
            (r.folder, r.in_folder) = match c {
                'F' => (true, false),
                'N' => (true, true),
                'm' => (false, true),
                'p' => (false, false),
                _ => unreachable!("{c}"),
            };
            m.rows.push(r);
        }
        m.next_id = spec.len() as u64;
        m
    }

    /// Every row as `id:folder-id`, `-` for none, so a run reads at a
    /// glance.
    fn membership(m: &DeskModel) -> String {
        (0..m.rows.len())
            .map(|n| {
                let f = m.folder_of(n).map_or("-".to_owned(), |q| m.rows[q].id.0.to_string());
                format!("{}:{f}", m.rows[n].id.0)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// **The reviewed case**: [G, X, F, A, B, C] with F a top-level folder.
    /// Deleting F used to leave A and B with their member flags, walking up
    /// through X into G, a group nobody put them in.
    #[test]
    fn a_removed_folder_hands_its_members_out_not_into_the_folder_above() {
        let mut m = runs("FmFmmp");
        assert_eq!(membership(&m), "0:- 1:0 2:- 3:2 4:2 5:-");
        let gone = m.remove_rows(&[2]).expect("remove");
        assert_eq!(membership(&m), "0:- 1:0 3:- 4:- 5:-", "A and B joined G");
        assert_eq!(gone.reshape.refiled, vec![t(3), t(4)]);
        assert!(m.orphans().is_empty());
        assert!(!m.rows[2].in_folder && !m.rows[3].in_folder, "the flags came off");

        // A top-level header with nothing above: its members do not draw
        // indented under nothing.
        let mut m = runs("Fmmp");
        let gone = m.remove_rows(&[0]).expect("remove");
        assert_eq!(membership(&m), "1:- 2:- 3:-");
        assert_eq!(gone.reshape.refiled, vec![t(1), t(2)]);
        assert!(m.orphans().is_empty(), "{:?}", m.orphans());
    }

    /// **Handed UP, a level at a time**, the way `remove_channel_inner`
    /// does it: an inner folder's members land in the outer one; an outer
    /// folder's nested folder becomes a top-level folder and keeps its own.
    #[test]
    fn a_removed_folder_hands_its_members_to_the_folder_it_sat_in() {
        // G, X, F nested in G, A, B in F.
        let mut m = runs("FmNmmp");
        assert_eq!(membership(&m), "0:- 1:0 2:0 3:2 4:2 5:-");
        let gone = m.remove_rows(&[2]).expect("remove");
        assert_eq!(membership(&m), "0:- 1:0 3:0 4:0 5:-");
        assert_eq!(gone.reshape.refiled, vec![t(3), t(4)]);
        assert!(m.rows[2].in_folder, "a member of the outer folder keeps its flag");

        let mut m = runs("FmNmmp");
        let gone = m.remove_rows(&[0]).expect("remove");
        assert_eq!(membership(&m), "1:- 2:- 3:2 4:2 5:-");
        assert_eq!(gone.reshape.refiled, vec![t(1), t(2)], "F's own members stayed F's");
        assert!(m.rows[1].folder, "the nested folder is still a folder");

        // Both headers in one batch: everything below goes out to the top.
        let mut m = runs("FmNmmp");
        let gone = m.remove_rows(&[2, 0]).expect("remove");
        assert_eq!(membership(&m), "1:- 3:- 4:- 5:-");
        assert_eq!(gone.reshape.refiled, vec![t(1), t(3), t(4)]);
        assert!(m.orphans().is_empty());

        // A member leaving re-files nobody: the run closes up behind it.
        let mut m = runs("Fmmmp");
        let gone = m.remove_rows(&[2]).expect("remove");
        assert_eq!(membership(&m), "0:- 1:0 3:0 4:-");
        assert!(gone.reshape.refiled.is_empty());
    }

    /// **A stale member flag stays out of the folder above**: a member flag
    /// with a plain row between it and the folder belongs to nothing, and
    /// removing that plain row must not hand it a group.
    #[test]
    fn removing_a_row_never_lets_a_stale_flag_join_a_folder() {
        let mut m = runs("Fmpm");
        assert_eq!(m.orphans(), vec![t(3)], "an older file's stale flag");
        let gone = m.remove_rows(&[2]).expect("remove");
        assert_eq!(membership(&m), "0:- 1:0 3:-");
        assert!(gone.reshape.refiled.is_empty(), "its folder did not change: it had none");
        assert!(m.orphans().is_empty(), "the flag came off rather than joining 0");
    }

    /// **An insert re-files nobody already on the desk**, and a new member
    /// needs a folder above it. Refused whole, before anything changes.
    #[test]
    fn an_insert_that_would_break_a_run_is_refused() {
        let base = runs("Fmmp");
        let row = |id: u32, folder: bool, in_folder: bool| TrackRow { folder, in_folder, ..TrackRow::vacant(t(id)) };
        let cases = [
            (2, row(9, false, false), Err(EditRefused::BreaksFolder(t(2))), "a plain row cut the run"),
            (2, row(9, true, false), Err(EditRefused::BreaksFolder(t(2))), "a folder took 2 from 0"),
            (2, row(9, true, true), Err(EditRefused::BreaksFolder(t(2))), "a nested folder took 2"),
            (0, row(9, false, true), Err(EditRefused::BreaksFolder(t(9))), "a member under nothing"),
            (4, row(9, false, true), Err(EditRefused::BreaksFolder(t(9))), "after a plain row"),
        ];
        for (at, r, want, why) in cases {
            let mut m = base.clone();
            assert_eq!(m.insert_rows(at, vec![r]).map(|_| ()), want, "{why}");
            assert_eq!(m, base, "a refused insert changed the desk: {why}");
        }
        // A member inside the run, or a row past its end, is fine.
        let mut m = base.clone();
        let r = m.insert_rows(2, vec![row(9, false, true)]).expect("a new member");
        assert!(r.refiled.is_empty());
        assert_eq!(membership(&m), "0:- 1:0 9:0 2:0 3:-");
        m.insert_rows(4, vec![row(10, true, true), row(11, false, true)]).expect("a nested folder at the end");
        assert_eq!(membership(&m), "0:- 1:0 9:0 2:0 10:0 11:10 3:-");
        assert_eq!(m.append(row(12, false, false)), Ok(7));
    }

    /// **A permutation never strands a member**, and says every row it
    /// moved into another run.
    #[test]
    fn a_permutation_that_strands_a_member_is_refused() {
        // G, g1, g2, C, H, h1.
        let base = runs("FmmpFm");
        let cases: [(&[usize], u32, &str); 4] = [
            (&[0, 2, 3, 1, 4, 5], 1, "a child carried out of its run with its flag on"),
            (&[0, 1, 3, 2, 4, 5], 2, "a plain row dropped mid-run cut the rest loose"),
            (&[1, 2, 0, 3, 4, 5], 1, "a folder moved off its children"),
            (&[0, 1, 2, 3, 5, 4], 5, "a child moved above its own folder"),
        ];
        for (order, id, why) in cases {
            let mut m = base.clone();
            assert_eq!(m.permute(order).map(|_| ()), Err(EditRefused::BreaksFolder(t(id))), "{why}");
            assert_eq!(m, base, "a refused permutation changed the desk: {why}");
        }
        // A folder carried with its run re-files nobody.
        let mut m = base.clone();
        let r = m.permute(&[4, 5, 0, 1, 2, 3]).expect("a block move");
        assert!(r.refiled.is_empty());
        assert_eq!(membership(&m), "4:- 5:4 0:- 1:0 2:0 3:-");
        // A child dropped at the end of another run lands in it, and is
        // named: the gesture decides, the model reports.
        let mut m = base.clone();
        let r = m.permute(&[0, 1, 3, 4, 5, 2]).expect("into H");
        assert_eq!(r.refiled, vec![t(2)]);
        assert_eq!(membership(&m), "0:- 1:0 3:- 4:- 5:4 2:4");
    }

    /// **`in_folder` travels with its row through every edit**, loud: the
    /// field `loud_row` leaves quiet, carried here in runs that stay whole.
    #[test]
    fn folder_flags_travel_with_their_rows_through_every_edit() {
        let member = |id: u32| TrackRow { folder: false, in_folder: true, ..loud_row(id) };
        let mut header = loud_row(0);
        header.in_folder = false;
        let mut m = DeskModel::empty();
        m.append(header.clone()).expect("header");
        m.append(member(1)).expect("member");
        m.append(loud_row(2)).expect("a top-level folder");
        m.insert_rows(2, vec![member(3)]).expect("a member inside the run");
        let before = m.rows.clone();
        let r = m.permute(&[3, 0, 1, 2]).expect("a run moved whole");
        assert!(r.refiled.is_empty());
        for (old, want) in before.iter().enumerate() {
            assert_row_is(&m.rows[r.lane(old).expect("kept")], want);
        }
        // The empty folder now on top goes; the run below it is untouched.
        let gone = m.remove_rows(&[0]).expect("remove");
        assert!(gone.reshape.refiled.is_empty());
        assert_eq!(gone.rows[0].1.id, t(2));
        for (old, want) in before.iter().enumerate() {
            if let Some(now) = gone.reshape.lane(r.lane(old).expect("kept")) {
                assert_row_is(&m.rows[now], want);
            }
        }
        assert_eq!(m.rows.len(), 3);
    }

    /// The single-row walk and the bulk one agree on every row of a busy
    /// desk, stale flags included.
    #[test]
    fn folder_of_agrees_with_the_bulk_walk() {
        let m = runs("mFmNmmpmFFmNNmp");
        let bulk = folder_lanes(m.rows.iter());
        for (n, f) in bulk.iter().enumerate() {
            assert_eq!(m.folder_of(n), *f, "row {n}");
        }
        assert_eq!(m.folder_of(m.rows.len()), None);
        assert_eq!(m.orphans(), vec![t(0), t(7)]);
    }

    /// **Every small desk, every one- and two-row removal**: each kept row
    /// ends in its nearest folder that survived (read off the desk BEFORE,
    /// one `folder_of` hop at a time), nothing is newly stranded, and
    /// `refiled` is exactly the rows whose folder changed. Six rows of four
    /// letters is 4,096 desks, every nesting and stale-flag shape included.
    #[test]
    fn every_removal_keeps_each_row_in_its_nearest_surviving_folder() {
        const LETTERS: [char; 4] = ['F', 'N', 'm', 'p'];
        for code in 0..4usize.pow(6) {
            let spec: String = (0..6).map(|k| LETTERS[code / 4usize.pow(k) % 4]).collect();
            let base = runs(&spec);
            let stale: BTreeSet<TrackId> = base.orphans().into_iter().collect();
            let mut batches: Vec<Vec<usize>> = (0..6).map(|a| vec![a]).collect();
            batches.extend((0..6).flat_map(|a| (a + 1..6).map(move |b| vec![a, b])));
            for gone in batches {
                let mut m = base.clone();
                let out = m.remove_rows(&gone).expect("remove");
                let mut refiled = Vec::new();
                for (old, row) in base.rows.iter().enumerate() {
                    let Some(now) = out.reshape.lane(old) else { continue };
                    let mut up = base.folder_of(old);
                    while let Some(q) = up.filter(|q| gone.contains(q)) {
                        up = base.folder_of(q);
                    }
                    let want = up.map(|q| base.rows[q].id);
                    assert_eq!(m.folder_of(now).map(|q| m.rows[q].id), want, "{spec} less {gone:?}: row {old}");
                    if want != base.folder_of(old).map(|q| base.rows[q].id) {
                        refiled.push(row.id);
                    }
                }
                assert_eq!(out.reshape.refiled, refiled, "{spec} less {gone:?}");
                assert!(m.orphans().iter().all(|id| stale.contains(id)), "{spec} less {gone:?}");
            }
        }
    }
}
