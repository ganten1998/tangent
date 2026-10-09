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
//! **Pure and unwired (W2).** Nothing in the app reads or writes this yet.
//! `desk_format` serialises it; [`DeskModel::from_v22`] and
//! [`DeskModel::to_v22`] translate to and from the dense 32-wide shape the
//! live desk still keeps ([`V22Desk`]), so the persistence lane can put a
//! format-2 file under the live arrays before the arrays go. The adapters
//! from a `Settings` and a `DeskSnap` are here for the same reason, and W3
//! owns them from then on.
//!
//! **What a row does NOT hold.** Clips and curves live in the arrangement,
//! keyed by lane, and a kit lives in drumpad.json. Those are the positional
//! units a structural edit must carry; the model cannot reach them, so each
//! edit hands its caller a [`Reshape`] that says where every old lane went,
//! and the class of a row ([`RowClass`]) takes what the arrangement knows
//! about it as a [`RowUse`]. A folder's run is positional too, but its
//! flags ride on its rows, so they move with them.
//!
//! **Scalars that never lived in strip space keep their own keys** (the
//! drum bus's fader, the click's, the master's, the tempo): they are not
//! per-track facts and the desk object does not carry them.

use std::collections::{BTreeMap, BTreeSet};

use crate::desk_format::Dropped;
use crate::desk_id::{
    display_slot, slot_key, Node, Rack, SeatBay, TrackId, BAYS, MAX_ROWS,
};
use crate::desk_legacy::{
    bind, decode_v32, decode_v32_strip, encode_v32, encode_v32_strip, unbind, LegacyOwner,
    V32_CHANNELS, V32_STRIPS,
};
use crate::desk_lift::DeskEra;
use crate::dialogs::{KEYS_PARAMS, KEYS_PATCHES};
use crate::recorder::MidiInputSel;
use crate::routing::{KeyEdge, Route, Target};

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
    Vacant,
    Placeholder,
    Bus,
    Counted,
}

/// **Where every old lane went** after a structural edit:
/// `old_to_new[old]` is the new lane, `None` for a removed row. The caller
/// rewrites what the model cannot reach (clip lanes, curve lanes, a kit
/// keyed by lane) through [`Reshape::lane`] in the same gesture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reshape {
    pub old_to_new: Vec<Option<usize>>,
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
}

/// **What `remove_rows` took with the rows**: every plugin and bypass on
/// their racks (chains, pedals and pads: the pedalboard leaves with its
/// row), every route and key touching them, and every send into them. The
/// caller writes the plugin states to the shelf and keeps the rest for its
/// undo; nothing is left in the model naming a removed id.
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
        #[cfg(test)]
        let before = self.check();
        let k = rows.len();
        if let Some(top) = rows.iter().map(|r| u64::from(r.id.0) + 1).max() {
            self.next_id = self.next_id.max(top);
        }
        self.rows.splice(at..at, rows);
        #[cfg(test)]
        self.audit_after(&before);
        Ok(Reshape {
            old_to_new: (0..old).map(|n| Some(if n < at { n } else { n + k })).collect(),
        })
    }

    /// **Remove the rows at `lanes`** (any order, repeats ignored), and
    /// everything that names them: see [`Removed`]. The specials and every
    /// other row keep all their own references.
    pub fn remove_rows(&mut self, lanes: &[usize]) -> Result<Removed, EditRefused> {
        let old = self.rows.len();
        let gone: BTreeSet<usize> = lanes.iter().copied().collect();
        if let Some(&bad) = gone.iter().find(|n| **n >= old) {
            return Err(EditRefused::NoSuchRow(bad));
        }
        #[cfg(test)]
        let before = self.check();
        let ids: BTreeSet<TrackId> = gone.iter().map(|n| self.rows[*n].id).collect();
        let named = |n: Node| matches!(n, Node::Track(id) if ids.contains(&id));

        let mut rows = Vec::with_capacity(gone.len());
        let mut kept = Vec::with_capacity(old - gone.len());
        let mut old_to_new = Vec::with_capacity(old);
        for (n, row) in std::mem::take(&mut self.rows).into_iter().enumerate() {
            if gone.contains(&n) {
                old_to_new.push(None);
                rows.push((n, row));
            } else {
                old_to_new.push(Some(kept.len()));
                kept.push(row);
            }
        }
        self.rows = kept;

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
        self.audit_after(&before);
        Ok(Removed {
            reshape: Reshape { old_to_new },
            rows,
            plugins,
            bypass,
            routes,
            keys,
            sends,
        })
    }

    /// **Reorder the rows**: `order[new] = old`, the convention every
    /// reorder in the app already uses. Nothing but the row list changes:
    /// every reference is by id.
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
        #[cfg(test)]
        let before = self.check();
        let mut taken: Vec<Option<TrackRow>> = std::mem::take(&mut self.rows).into_iter().map(Some).collect();
        self.rows = order.iter().map(|old| taken[*old].take().expect("a permutation")).collect();
        #[cfg(test)]
        self.audit_after(&before);
        Ok(Reshape { old_to_new })
    }

    /// **An edit never makes a reference dangle.** Weaker than "the desk is
    /// clean", deliberately: the model is pure and a test may build a desk
    /// that is dirty on purpose. W6 wires `check` into the undo sentinel,
    /// where the desk is the app's and must be clean.
    #[cfg(test)]
    fn audit_after(&self, before: &[Dropped]) {
        let after = self.check();
        let new: Vec<_> = after.iter().filter(|d| !before.contains(d)).collect();
        assert!(new.is_empty(), "a structural edit left references dangling: {new:?}");
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
        let fx_send = |level: f64| {
            (level.to_bits() != 0).then_some(Send { to: Node::Fx, level, pre: false })
        };
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
                row.sends.extend(v.sends.get(s).copied().and_then(fx_send));
                row.mute = bit(v.muted, s);
                row.solo = bit(v.soloed, s);
                row.phase = bit(v.phase, s);
            }
            rows.push(row);
        }

        let mut specials = Specials::default();
        for (node, owner) in SPECIAL_OWNERS {
            let s = encode_v32_strip(owner).expect("every special has a strip");
            let sp = specials.get_mut(node).expect("a special");
            sp.name = v.names.get(s).cloned().unwrap_or_default();
            sp.color = v.colors.get(s).copied().unwrap_or(0);
            sp.sends.extend(v.sends.get(s).copied().and_then(fx_send));
            sp.mute = bit(v.muted, s);
            sp.solo = bit(v.soloed, s);
            sp.phase = bit(v.phase, s);
        }

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
                Target::Channel(n) => lane(n).map(RouteTo::Node),
                Target::Hardware { first, stereo } => Some(RouteTo::Hardware { first, stereo }),
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

    /// **The rows as a dense legacy desk**, and the ids in lane order, or
    /// `None` for a desk past 32 rows, which no dense shape can hold.
    ///
    /// Lossy by construction for what the dense shape never had: a send
    /// other than the post-fader Fx knob, a route or key with a special at
    /// a track's end, a chain past the second, a key on a pedal or pad.
    /// None of these can be made while the live desk is dense, so for every
    /// desk `from_v22` produced, this returns its input. Deleted with the
    /// dense desk in W6.
    pub fn to_v22(&self) -> Option<(V22Desk, Vec<TrackId>)> {
        if self.rows.len() > V32_CHANNELS {
            return None;
        }
        let ids = self.ids();
        let mut v = V22Desk::vacant();
        let knob = |sends: &[Send]| {
            sends.iter().find(|s| s.to == Node::Fx && !s.pre).map_or(0.0, |s| s.level)
        };
        let set = |mask: &mut u64, at: usize, on: bool| {
            if on {
                *mask |= 1 << at;
            }
        };
        for (r, row) in self.rows.iter().enumerate() {
            let TrackRow {
                id: _,
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
            v.kinds[r] = RowKind::byte(*kind);
            if *added {
                v.tracks_added |= 1 << r;
            }
            v.icons[r] = *icon;
            v.names[r] = name.clone();
            v.colors[r] = *color;
            v.gains[r] = *gain;
            v.pans[r] = *pan;
            v.inputs[r] = input.clone();
            v.midi_inputs[r] = midi.clone();
            set(&mut v.muted, r, *mute);
            set(&mut v.soloed, r, *solo);
            set(&mut v.phase, r, *phase);
            if *arm {
                v.armed |= 1 << r;
            }
            v.to_theory[r] = *theory;
            v.is_folder[r] = *folder;
            v.in_folder[r] = *in_folder;
            v.folder_shut[r] = *shut;
            v.sends[r] = knob(sends);
            v.dx7_cartridge[r] = dx7.cart.clone();
            v.dx7_patch[r] = dx7.patch;
            v.keys_patch[r] = keys.patch;
            for (i, p) in keys.params.iter().take(KEYS_SPAN).enumerate() {
                v.keys_params[r * KEYS_SPAN + i] = *p;
            }
            v.frozen[r] = frozen.clone();
        }
        for (node, owner) in SPECIAL_OWNERS {
            let s = encode_v32_strip(owner).expect("every special has a strip");
            let sp = self.specials.get(node).expect("a special");
            v.names[s] = sp.name.clone();
            v.colors[s] = sp.color;
            if let Some(slot) = v.sends.get_mut(s) {
                *slot = knob(&sp.sends);
            }
            set(&mut v.muted, s, sp.mute);
            set(&mut v.soloed, s, sp.solo);
            set(&mut v.phase, s, sp.phase);
        }
        let flat = |sb: &SeatBay| encode_v32(unbind(sb.seat.node, &ids)?, sb.seat.rack, sb.bay);
        for (sb, path) in &self.plugins {
            if let Some(f) = flat(sb) {
                v.inserts.insert(f, path.clone());
            }
        }
        for sb in &self.bypass {
            if let Some(f) = flat(sb) {
                v.bypass.insert((f / BAYS, f % BAYS));
            }
        }
        let lane = |n: Node| match unbind(n, &ids)? {
            LegacyOwner::Row(r) => Some(r as usize),
            _ => None,
        };
        for e in &self.routes {
            let target = match e.to {
                RouteTo::Node(n) => lane(n).map(Target::Channel),
                RouteTo::Hardware { first, stereo } => Some(Target::Hardware { first, stereo }),
            };
            if let (Some(from), Some(target)) = (lane(e.from), target) {
                v.routes.push(Route { from, target, gain: e.gain });
            }
        }
        for k in &self.keys {
            let strip = unbind(k.seat.seat.node, &ids).and_then(encode_v32_strip);
            if let (Some(strip), Some(slot), Some(from)) = (strip, display_slot(k.seat), lane(k.from)) {
                v.keys.push(KeyEdge { strip, slot, from });
            }
        }
        Some((v, ids))
    }
}

/// The specials and their legacy owners, in strip order.
const SPECIAL_OWNERS: [(Node, LegacyOwner); 4] = [
    (Node::Drums, LegacyOwner::Drums),
    (Node::Click, LegacyOwner::Click),
    (Node::Fx, LegacyOwner::Fx),
    (Node::Master, LegacyOwner::Master),
];

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

/// **The dense 32-wide desk**: the per-channel and per-strip arrays the
/// live `Settings` and `DeskSnap` still keep, at the frozen V32 widths and
/// no other, so this type means the same thing whatever the live constants
/// become.
///
/// Channel arrays are [`V32_CHANNELS`] long (`keys_params` that times
/// [`KEYS_SPAN`]), strip arrays 36 (names, colours) or 35 (sends; the
/// master has none), masks are bits of strip space. Seats are the V32 flat
/// map, sparse: `inserts[flat] = path`, `bypass` as `(seat, bay)`. Routes
/// and keys keep their legacy lane and strip numbers.
///
/// [`V22Desk::vacant`] is the canonical empty one; [`DeskModel::to_v22`]
/// always answers this shape at these lengths.
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
    pub routes: Vec<Route>,
    pub keys: Vec<KeyEdge>,
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

    /// **A settings file's desk, dense**: what the legacy chain left in
    /// `Settings` after it lifted the file onto V32. Arrays shorter than the
    /// V32 width (a trimmed `channel_midi_inputs`, a short `strip_names`)
    /// read as vacant past their end; an unset `channel_armed` resolves by
    /// the shipped rule, as `app::default_armed` does.
    pub fn from_settings(s: &crate::settings::Settings) -> Self {
        let mut v = V22Desk::vacant();
        let armed = s.channel_armed.unwrap_or_else(|| {
            crate::arrangement::armed_by_default(&s.channel_kinds, |n| {
                s.strip_inserts.get(n * BAYS).map_or("", String::as_str) == crate::dialogs::BUILTIN_PATH
            })
        });
        fill(&mut v.kinds, &s.channel_kinds);
        fill(&mut v.icons, &s.channel_icons);
        fill(&mut v.gains, &s.channel_gains);
        fill(&mut v.pans, &s.channel_pans);
        fill(&mut v.inputs, &s.channel_inputs);
        fill(&mut v.midi_inputs, &s.channel_midi_inputs);
        v.tracks_added = s.tracks_added as u32;
        v.armed = armed;
        fill(&mut v.is_folder, &s.strip_is_folder);
        fill(&mut v.in_folder, &s.strip_in_folder);
        fill(&mut v.folder_shut, &s.strip_folder_shut);
        fill(&mut v.to_theory, &s.strip_to_theory);
        fill(&mut v.dx7_cartridge, &s.strip_dx7_cartridge);
        fill(&mut v.dx7_patch, &s.strip_dx7_patch);
        fill(&mut v.keys_patch, &s.strip_keys_patch);
        fill(&mut v.keys_params, &s.strip_keys_params);
        v.frozen = frozen_dense(&s.frozen_lanes);
        fill(&mut v.names, &s.strip_names);
        fill(&mut v.colors, &s.strip_colors);
        fill(&mut v.sends, &s.strip_sends);
        v.muted = s.strip_muted;
        v.soloed = s.strip_soloed;
        v.phase = s.strip_phase;
        v.inserts = s
            .strip_inserts
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_empty())
            .map(|(b, p)| (b, p.clone()))
            .collect();
        v.bypass = s.insert_bypass.0.iter().copied().collect();
        v.routes = s.routes.0.clone();
        v.keys = s.insert_keys.0.clone();
        v
    }

    /// **A document's desk, dense.** A field the snapshot does not SAY
    /// (`None`, or past the end of a short array) takes `live`'s value,
    /// which is how `apply_desk_snapshot` treats it: the live desk stands
    /// for what an older document never wrote. The one exception is the
    /// snapshot's own: no `frozen` is nothing frozen (see
    /// `DeskSnap::frozen`).
    pub fn from_snap(d: &crate::arrangement::DeskSnap, live: &V22Desk) -> Self {
        let mut v = live.clone();
        fill(&mut v.kinds, &d.kinds);
        fill(&mut v.icons, &d.icons);
        fill(&mut v.names, &d.names);
        fill(&mut v.colors, &d.colors);
        fill(&mut v.sends, &d.sends);
        fill(&mut v.gains, &d.gains);
        fill(&mut v.pans, &d.pans);
        fill(&mut v.inputs, &d.inputs);
        fill(&mut v.midi_inputs, &d.midi_inputs);
        fill(&mut v.is_folder, &d.is_folder);
        fill(&mut v.in_folder, &d.in_folder);
        fill(&mut v.folder_shut, &d.folder_shut);
        v.muted = d.muted;
        v.soloed = d.soloed;
        v.tracks_added = d.tracks_added;
        v.armed = d.armed;
        v.inserts = d.inserts.iter().filter(|(_, p)| !p.is_empty()).cloned().collect();
        if let Some(p) = d.phase {
            v.phase = p;
        }
        if let Some(t) = &d.to_theory {
            fill(&mut v.to_theory, t);
        }
        if let Some(r) = &d.routes {
            v.routes = r.0.clone();
        }
        if let Some(k) = &d.insert_keys {
            v.keys = k.0.clone();
        }
        if let Some(b) = &d.bypass {
            v.bypass = b.0.iter().copied().collect();
        }
        if let Some(i) = &d.instruments {
            fill(&mut v.dx7_cartridge, &i.dx7_cartridge);
            fill(&mut v.dx7_patch, &i.dx7_patch);
            fill(&mut v.keys_patch, &i.keys_patch);
            fill(&mut v.keys_params, &i.keys_params);
        }
        v.frozen = frozen_dense(d.frozen.as_deref().unwrap_or(&[]));
        v
    }
}

/// Overwrite the front of a dense array with what a source says, leaving
/// the rest: a source longer than the V32 width is cut at it.
fn fill<T: Clone>(dst: &mut [T], src: &[T]) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d = s.clone();
    }
}

/// A frozen-lane list as one entry per V32 channel. The first entry for a
/// lane wins and a lane past the channels is not a row: the same answer
/// `TangentApp::lane_frozen`'s `find` gives.
fn frozen_dense(list: &[crate::arrangement::FrozenLane]) -> Vec<Option<Frozen>> {
    let mut out = vec![None; V32_CHANNELS];
    for f in list {
        if let Some(slot @ None) = out.get_mut(f.lane) {
            *slot = Some(Frozen { path: f.path.clone(), len_s: f.len_s });
        }
    }
    out
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
    use crate::desk_legacy::identity_ids;

    fn t(n: u32) -> TrackId {
        TrackId(n)
    }

    fn sb(node: Node, rack: Rack, bay: u8) -> SeatBay {
        SeatBay::new(node, rack, bay)
    }

    /// A row that differs from vacant in EVERY field, so a test that
    /// destructures it after an edit can see each one travel.
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
            in_folder: true,
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
        let indent = add(&|r| r.in_folder = true);
        let bus = add(&|r| r.kind = Some(RowKind::Bus));
        let bus_added = add(&|r| {
            r.kind = Some(RowKind::Bus);
            r.added = true;
        });
        let midi = add(&|r| r.kind = Some(RowKind::Midi));
        let audio = add(&|r| r.kind = Some(RowKind::Audio));
        let bare = add(&|r| r.added = true);
        let folder = add(&|r| r.folder = true);
        let folder_child_track = add(&|r| {
            r.in_folder = true;
            r.kind = Some(RowKind::Midi);
        });
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
        assert_eq!(m.to_v22(), Some((V22Desk::vacant(), identity_ids(32))));
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
        let (back, ids) = read.model.to_v22().expect("32 rows");
        assert_eq!(ids, identity_ids(32));
        assert_eq!(back, v);
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
        assert_eq!(m.to_v22().map(|(d, i)| (d == v, i == ids)), Some((true, true)));
    }

    /// Past 32 rows there is no dense shape to answer.
    #[test]
    fn a_desk_past_thirty_two_rows_has_no_dense_shape() {
        let mut m = DeskModel::empty();
        for n in 0..33 {
            m.append(TrackRow::vacant(t(n))).expect("append");
        }
        assert_eq!(m.to_v22(), None);
        m.remove_rows(&[32]).expect("remove");
        assert!(m.to_v22().is_some());
    }
}
