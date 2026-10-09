//! **The desk as the audio thread will read it: one immutable plan per shape.**
//!
//! Today the callback rebuilds the desk's shape from atomics every chunk: it
//! latches the route table and runs Kahn's walk over bitmasks, reads the
//! folder tree into a `[i32; CHANNELS]`, and asks `channel_is_heard` per lane
//! with a `u64` mute mask. Every one of those is a fixed width, and every one
//! of them is recomputed whether or not anything moved.
//!
//! A [`DeskPlan`] is that work done ONCE, on the UI thread, when an input that
//! decides an ORDER changes: route existence, a gain crossing zero, the
//! hardware flag, folder parents, kinds, arm. It is moved in boxed, validated
//! before it plays (`lane_bank::apply_ops`), and retired whole. What only
//! SCALES the sound (a fader, a send level, a route's gain while it stays
//! above zero) stays an atomic, so moving a knob never rebuilds a plan.
//!
//! **Identity in, ranks out.** [`PlanInputs`] names every lane and endpoint by
//! its [`Slot`], which does not move when the desk is reordered. The plan
//! translates once into RANKS (the shown order), because rank is what governs
//! every order the engine sums in: the decide visit, the fold, Kahn's
//! tie-break, the level publish. A legacy desk installs row r at `Slot(r)` in
//! rank r, so for every desk that exists today rank, slot and lane index are
//! the same number and the transcribed walks below are bit-exact by
//! construction (design section 7.9).
//!
//! Unwired in this wave (W1-rt-kernels-goldens): proven against frozen copies
//! of today's functions, consumed by W3-slot-engine.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::laneset::LaneSet;
use tangent_ui::recorder::MidiRoute;

/// Where a lane lives in the engine's spine. Not a `TrackId`: the bridge owns
/// `TrackId -> Slot`, and a slot is reused only after its lane has come home.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Slot(pub u32);

/// A capture pick: one bound input, stable for the life of its binding so a
/// shape change never reopens the stream (design section 7.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PickId(pub u32);

/// Per-lane switches, one bit each, in place of the desk-wide masks.
///
/// A `u16` because there are twelve and a lane carries one copy in the plan;
/// the masks they replace were one word for the whole desk, and that word is
/// what filled at 32 and 64.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LaneFlags(pub u16);

impl LaneFlags {
    pub const ARMED: Self = Self(1 << 0);
    pub const FROZEN: Self = Self(1 << 1);
    pub const MIDI_OFF: Self = Self(1 << 2);
    pub const DRUM: Self = Self(1 << 3);
    pub const BUILTIN: Self = Self(1 << 4);
    pub const KEYS: Self = Self(1 << 5);
    pub const MUTED: Self = Self(1 << 6);
    pub const SOLOED: Self = Self(1 << 7);
    pub const PHASE: Self = Self(1 << 8);
    pub const SUSPEND: Self = Self(1 << 9);
    pub const BUS: Self = Self(1 << 10);
    /// On its way out: the 10 ms ramp before a Remove (design 7.13).
    pub const RETIRING: Self = Self(1 << 11);
    /// **Derived, never an input**: a strict ancestor of this lane is
    /// soloed. [`build_plan`] clears what it is handed and settles it, so
    /// [`resolve_heard`] never has to climb a chain.
    pub const SOLO_ABOVE: Self = Self(1 << 12);
    /// **Derived, never an input**: a strict descendant is soloed.
    pub const SOLO_BELOW: Self = Self(1 << 13);

    pub const fn contains(self, f: Self) -> bool {
        self.0 & f.0 == f.0
    }

    #[must_use]
    pub const fn with(self, f: Self, on: bool) -> Self {
        if on {
            Self(self.0 | f.0)
        } else {
            Self(self.0 & !f.0)
        }
    }
}

impl std::ops::BitOr for LaneFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// The four strips that are not rows: their switches, in the lanes' bits
/// (only MUTED, SOLOED, PHASE and SUSPEND mean anything here).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpecialFlags {
    pub drums: LaneFlags,
    pub click: LaneFlags,
    pub fx: LaneFlags,
    pub master: LaneFlags,
}

/// One lane as the host hands it to [`build_plan`]: by identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InLane {
    pub slot: Slot,
    pub gen: u32,
    pub flags: LaneFlags,
    /// The folder this row sits in, by its slot. Must sit ABOVE the child in
    /// rank (today's `set_folders` refuses anything else); one that does not
    /// is dropped from the plan, and the row plays loose.
    pub parent: Option<Slot>,
    pub punch: Option<u64>,
    pub midi: MidiRoute,
    pub pick: Option<PickId>,
}

/// Where a route goes, by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteEnd {
    Lane(Slot),
    Hardware { first: u16, stereo: bool },
}

/// One route as the host hands it over, in the host's table order: that
/// order is the gather order, so it is kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InRoute {
    pub from: Slot,
    pub to: RouteEnd,
    pub gain: f32,
}

/// Who sends, by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendFrom {
    Lane(Slot),
    Drums,
    Click,
}

/// Where a send lands: Return 0 (the built-in effects) or a lane (a bus row,
/// from W6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendTo {
    Fx,
    Lane(Slot),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InSend {
    pub from: SendFrom,
    pub to: SendTo,
    pub level: f32,
    pub pre: bool,
}

/// One sidechain key: the keyed bay in today's engine seat space (W3 moves
/// it to a `RackAddr`), and the lane whose stem feeds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InKey {
    pub seat: u32,
    pub bay: u8,
    pub from: Slot,
}

/// Everything [`build_plan`] reads. Lanes are in RANK order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanInputs {
    pub lanes: Vec<InLane>,
    pub specials: SpecialFlags,
    pub routes: Vec<InRoute>,
    pub sends: Vec<InSend>,
    pub keys: Vec<InKey>,
}

/// One lane as the plan holds it: `parent` is a RANK here, always below the
/// child's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanLane {
    pub slot: Slot,
    pub gen: u32,
    pub flags: LaneFlags,
    pub parent: Option<u32>,
    pub punch: Option<u64>,
    pub midi: MidiRoute,
    pub pick: Option<PickId>,
}

/// Rows of indices, packed: `items[start[r]..start[r + 1]]` is row r.
/// Built stably, so a row lists its items in the order they were offered,
/// which is what keeps every gather in today's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Csr {
    pub start: Box<[u32]>,
    pub items: Box<[u32]>,
}

impl Csr {
    /// Counting sort over `(row, item)` pairs, stable within a row.
    fn build(rows: usize, pairs: &[(usize, u32)]) -> Self {
        let mut start = vec![0u32; rows + 1];
        for &(r, _) in pairs {
            if r < rows {
                start[r + 1] += 1;
            }
        }
        for r in 0..rows {
            start[r + 1] += start[r];
        }
        let mut fill: Vec<u32> = start[..rows].to_vec();
        let mut items = vec![0u32; start[rows] as usize];
        for &(r, it) in pairs {
            if r < rows {
                items[fill[r] as usize] = it;
                fill[r] += 1;
            }
        }
        Self {
            start: start.into_boxed_slice(),
            items: items.into_boxed_slice(),
        }
    }

    pub fn rows(&self) -> usize {
        self.start.len().saturating_sub(1)
    }

    /// Row `r`, empty past the end rather than a panic: the audio thread
    /// reads these.
    pub fn row(&self, r: usize) -> &[u32] {
        let (Some(&a), Some(&b)) = (self.start.get(r), self.start.get(r + 1)) else {
            return &[];
        };
        self.items.get(a as usize..b as usize).unwrap_or(&[])
    }
}

/// Where a planned route goes: a lane by RANK, or a pair of jacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTo {
    Lane(u32),
    Hardware { first: u16, stereo: bool },
}

/// One route in the plan, in the host's table order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanRoute {
    /// Rank of the source.
    pub from: u32,
    pub to: RouteTo,
    /// Index into [`DeskPlan::gains`].
    pub gain: u32,
    /// Whether the gain was above zero when the plan was built. Only a live
    /// route orders anything or is gathered; a gain crossing zero republishes
    /// the plan, so this is never stale.
    pub live: bool,
}

/// One step of the iterative route transfer: `lane`'s figure is final once
/// its `count` edges from `first` in [`RouteTable::transfer_edges`] are
/// summed, in table order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferStep {
    pub lane: u32,
    pub first: u32,
    pub count: u32,
}

/// One edge of a transfer step. `cut` is an edge today's recursive walk met
/// on its own path (a cycle's way back), which contributes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferEdge {
    pub edge: u32,
    pub to: u32,
    pub cut: bool,
}

/// The routes, ordered once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteTable {
    /// Every route that names lanes in this plan, in the host's order.
    pub edges: Box<[PlanRoute]>,
    /// Destination RANKS in the order the sends pass runs them: today's
    /// Kahn sweep (ascending tie-break, a destination placed as soon as its
    /// feeders are), then every destination left unplaced, ascending. Those
    /// are cycle members and what hangs off them, which gather whatever the
    /// stem holds and so run one block late.
    pub order: Box<[u32]>,
    /// How many of `order` the sweep placed; the rest is the cycle tail.
    pub placed: u32,
    /// Per position in `order`: the live, non-hardware edges gathered into
    /// that destination, in table order.
    pub inputs: Csr,
    /// Ranks whose direct path to the master is closed: any non-hardware
    /// route out (whatever its gain, as today), a folder, or a row inside
    /// one. Today's `route_direct`, as a set.
    pub closed: LaneSet,
    /// Whether any route goes to the jacks.
    pub hardware: bool,
    /// Today's recursive `route_transfer`, as a flat list: every lane once,
    /// in the order the recursion finalised it.
    pub transfer: Box<[TransferStep]>,
    pub transfer_edges: Box<[TransferEdge]>,
    /// Index into [`DeskPlan::levels`] where the route phase starts; the
    /// levels before it are the folder fold.
    pub first_level: u32,
    /// Whether the last level is the cycle tail (run serially, one block
    /// late).
    pub cycle_level: bool,
}

/// One send in the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanSendFrom {
    Lane(u32),
    Drums,
    Click,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanSendTo {
    Fx,
    Lane(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanSend {
    pub from: PlanSendFrom,
    pub to: PlanSendTo,
    pub pre: bool,
    /// Index into [`DeskPlan::gains`].
    pub gain: u32,
}

/// The sends, and per destination who sends to it: row 0 is Return 0, row
/// `r + 1` is the lane at rank r.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendTable {
    pub sends: Box<[PlanSend]>,
    pub into: Csr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanKey {
    pub seat: u32,
    pub bay: u8,
    /// Rank of the lane whose stem keys the bay.
    pub from: u32,
}

/// The sidechain keys, sorted by `(seat, bay)`, and the ranks whose stems
/// must be snapshotted for them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyTable {
    pub keys: Box<[PlanKey]>,
    pub sources: LaneSet,
}

/// The post-fork graph in topological layers of RANKS: a COMPUTE schedule.
/// Each layer's nodes depend only on earlier layers, so a layer may run its
/// nodes in parallel, and each node may write its OWN stem column (nobody
/// else writes it, so the order of those writes is no sum) before the next
/// layer starts, because that column is what the next layer reads.
///
/// **The listed order is not the publish order.** What a node adds into the
/// shared sums (`mix`, `aux`) must be added in [`Publish`]'s order, which is
/// today's serial walks, and that order crosses layers: `fold_folders` is
/// one descending walk over every folder, so a shallow folder ranked above a
/// deep one publishes between the deep one and its parent; the sends pass
/// adds in Kahn order, which places a lone destination by rank, ahead of a
/// deeper one. f32 `+=` is not associative, so publishing layer by layer
/// would move bits on exactly those desks. A runner therefore holds each
/// node's sums until its whole phase has run and then adds them in
/// [`Publish`] order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Levels(pub Box<[Box<[u32]>]>);

/// The order the post-fork nodes add into `mix` and `aux`: today's serial
/// walks, transcribed. Each phase is a permutation of its own levels (the
/// fold's are `levels[..first_level]`, the route phase's the rest).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Publish {
    /// Every folder, descending rank: `fold_folders`' one walk up the
    /// channels, whatever each folder's depth.
    pub fold: Box<[u32]>,
    /// Every route-phase node: the sweep's placed order, then the cycle tail.
    /// On a desk with no sends into lanes (every desk today) this is exactly
    /// [`RouteTable::order`]; a bus row that only sends reach is placed by
    /// the same rule, which is W6's to keep.
    pub routes: Box<[u32]>,
}

/// The shape of the desk for as long as nothing that decides an order moves.
#[derive(Debug, Default)]
pub struct DeskPlan {
    /// Counts up from 1 across [`build_plan`] calls; 0 is the empty plan.
    pub gen: u64,
    pub lanes: Box<[PlanLane]>,
    /// Slot -> rank, [`NO_RANK`] for a slot this plan does not play.
    pub by_slot: Box<[u32]>,
    pub specials: SpecialFlags,
    pub routes: RouteTable,
    pub sends: SendTable,
    pub keys: KeyTable,
    pub levels: Levels,
    /// The order the levels' nodes publish their sums in (not the levels'
    /// own order; see [`Levels`]).
    pub publish: Publish,
    /// f32 bits: every route's gain by edge index, then every send's level.
    /// The host keeps the last clone (design 7.14 invariant 5).
    pub gains: Arc<[AtomicU32]>,
    /// Anything soloed, specials included: what makes solo exclusive.
    pub any_solo: bool,
}

/// The `by_slot` answer for a slot the plan does not play.
pub const NO_RANK: u32 = u32::MAX;

impl DeskPlan {
    /// The plan an engine starts with: nothing plays.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The rank a slot plays at, if it plays.
    pub fn rank_of(&self, slot: Slot) -> Option<u32> {
        self.by_slot
            .get(slot.0 as usize)
            .copied()
            .filter(|r| *r != NO_RANK)
    }

    /// A gain or send level, as the audio thread reads it.
    pub fn gain(&self, index: u32) -> f32 {
        self.gains
            .get(index as usize)
            .map_or(0.0, |g| f32::from_bits(g.load(Ordering::Relaxed)))
    }
}

/// What a route's gain is to the walk: today's clamp at the latch, so a
/// non-finite gain is silence and nothing past unity.
fn sane_gain(g: f32) -> f32 {
    if g.is_finite() {
        g.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Today's Kahn sweep, in O(V + E + V log V) rather than O(V * (V + E)).
///
/// The sweep visits destinations ascending, pass after pass, placing each as
/// soon as every feeder that is itself a destination is placed. A
/// destination is therefore placed in pass
/// `max over feeders s of (pass(s) + [s > d])` (zero with none): a feeder
/// below it was placed earlier in the same sweep, one above it only by the
/// pass before. So the placed order is `(pass, rank)` ascending, computed
/// here over an ordinary topological order. Whatever a cycle starves is left
/// unplaced and goes last, ascending, as today.
///
/// `deps` row d lists the feeders of d that are themselves destinations
/// (duplicates allowed: two routes s -> d count twice in and out).
/// Returns `(placed order, cycle tail, level of each placed node)`, where a
/// node's level is one more than its deepest feeder's.
fn sweep(n: usize, receives: &[bool], deps: &Csr) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    let mut indeg = vec![0u32; n];
    let mut succ_pairs: Vec<(usize, u32)> = Vec::new();
    for d in 0..n {
        if !receives[d] {
            continue;
        }
        for &s in deps.row(d) {
            indeg[d] += 1;
            succ_pairs.push((s as usize, d as u32));
        }
    }
    let succ = Csr::build(n, &succ_pairs);
    let mut topo: Vec<u32> = (0..n as u32)
        .filter(|&d| receives[d as usize] && indeg[d as usize] == 0)
        .collect();
    let mut head = 0;
    while head < topo.len() {
        let u = topo[head] as usize;
        head += 1;
        for &v in succ.row(u) {
            let v = v as usize;
            indeg[v] -= 1;
            if indeg[v] == 0 {
                topo.push(v as u32);
            }
        }
    }
    let mut pass = vec![0u32; n];
    let mut level = vec![0u32; n];
    let mut is_placed = vec![false; n];
    for &u in &topo {
        let u = u as usize;
        is_placed[u] = true;
        let (mut p, mut l) = (0u32, 0u32);
        for &s in deps.row(u) {
            let s = s as usize;
            p = p.max(pass[s] + u32::from(s > u));
            l = l.max(level[s] + 1);
        }
        pass[u] = p;
        level[u] = l;
    }
    let mut placed = topo;
    placed.sort_unstable_by_key(|&d| (pass[d as usize], d));
    let tail: Vec<u32> = (0..n as u32)
        .filter(|&d| receives[d as usize] && !is_placed[d as usize])
        .collect();
    (placed, tail, level)
}

/// Build the plan for `inputs`. `prev` is the plan this one replaces: when
/// the edges are the same edges, by identity, the gains `Arc` is carried
/// over, so a gain the bridge writes lands in one place whichever of the two
/// plans the callback is holding.
pub fn build_plan(inputs: &PlanInputs, prev: Option<&DeskPlan>) -> DeskPlan {
    let n = inputs.lanes.len();
    let width = inputs
        .lanes
        .iter()
        .map(|l| l.slot.0 as usize + 1)
        .max()
        .unwrap_or(0);
    let mut by_slot = vec![NO_RANK; width];
    for (rank, l) in inputs.lanes.iter().enumerate() {
        by_slot[l.slot.0 as usize] = rank as u32;
    }
    let rank = |s: Slot| by_slot.get(s.0 as usize).copied().filter(|r| *r != NO_RANK);

    let mut lanes: Vec<PlanLane> = inputs
        .lanes
        .iter()
        .enumerate()
        .map(|(me, l)| PlanLane {
            slot: l.slot,
            gen: l.gen,
            flags: l.flags,
            // Above the child or not at all: the walks below rely on it, and
            // today's `set_folders` holds the same line.
            parent: l.parent.and_then(rank).filter(|p| (*p as usize) < me),
            punch: l.punch,
            midi: l.midi,
            pick: l.pick,
        })
        .collect();
    settle_solo_facts(&mut lanes);

    // ── gains: routes, then sends ───────────────────────────────────────
    let mut gain_values: Vec<f32> = Vec::new();
    let mut edges: Vec<PlanRoute> = Vec::new();
    let mut hardware = false;
    for r in &inputs.routes {
        // A route naming a lane this plan does not play is not a route: the
        // desk can shrink between the host's model and this build, as today.
        let Some(from) = rank(r.from) else { continue };
        let to = match r.to {
            RouteEnd::Lane(s) => match rank(s) {
                Some(t) => RouteTo::Lane(t),
                None => continue,
            },
            RouteEnd::Hardware { first, stereo } => RouteTo::Hardware { first, stereo },
        };
        let g = sane_gain(r.gain);
        hardware |= matches!(to, RouteTo::Hardware { .. });
        edges.push(PlanRoute {
            from,
            to,
            gain: gain_values.len() as u32,
            live: g > 0.0,
        });
        gain_values.push(g);
    }
    let mut sends: Vec<PlanSend> = Vec::new();
    for s in &inputs.sends {
        let from = match s.from {
            SendFrom::Lane(sl) => match rank(sl) {
                Some(r) => PlanSendFrom::Lane(r),
                None => continue,
            },
            SendFrom::Drums => PlanSendFrom::Drums,
            SendFrom::Click => PlanSendFrom::Click,
        };
        let to = match s.to {
            SendTo::Fx => PlanSendTo::Fx,
            SendTo::Lane(sl) => match rank(sl) {
                Some(r) => PlanSendTo::Lane(r),
                None => continue,
            },
        };
        sends.push(PlanSend {
            from,
            to,
            pre: s.pre,
            gain: gain_values.len() as u32,
        });
        gain_values.push(if s.level.is_finite() {
            s.level.max(0.0)
        } else {
            0.0
        });
    }

    // ── Kahn over the live lane routes ─────────────────────────────────
    let lane_edge = |e: &PlanRoute| match e.to {
        RouteTo::Lane(t) if e.live => Some((e.from as usize, t as usize)),
        _ => None,
    };
    let mut receives = vec![false; n];
    for e in &edges {
        if let Some((_, t)) = lane_edge(e) {
            receives[t] = true;
        }
    }
    let dep_pairs: Vec<(usize, u32)> = edges
        .iter()
        .filter_map(lane_edge)
        .filter(|&(f, _)| receives[f])
        .map(|(f, t)| (t, f as u32))
        .collect();
    let deps = Csr::build(n, &dep_pairs);
    let (placed, tail, _) = sweep(n, &receives, &deps);
    let mut order = placed.clone();
    order.extend_from_slice(&tail);
    let mut position = vec![u32::MAX; n];
    for (i, &d) in order.iter().enumerate() {
        position[d as usize] = i as u32;
    }
    let input_pairs: Vec<(usize, u32)> = edges
        .iter()
        .enumerate()
        .filter_map(|(i, e)| lane_edge(e).map(|(_, t)| (position[t] as usize, i as u32)))
        .collect();
    let inputs_csr = Csr::build(order.len(), &input_pairs);

    // ── what no longer goes straight to the master ─────────────────────
    let mut closed = LaneSet::with_capacity(n);
    for e in &edges {
        if matches!(e.to, RouteTo::Lane(_)) {
            closed.insert(e.from as usize);
        }
    }
    let mut is_folder = vec![false; n];
    for (me, l) in lanes.iter().enumerate() {
        if let Some(p) = l.parent {
            is_folder[p as usize] = true;
            closed.insert(me);
        }
    }
    for (f, is) in is_folder.iter().enumerate() {
        if *is {
            closed.insert(f);
        }
    }

    // ── the transfer, as today's recursion finalises it ────────────────
    let out_pairs: Vec<(usize, u32)> = edges
        .iter()
        .enumerate()
        .filter_map(|(i, e)| lane_edge(e).map(|(f, _)| (f, i as u32)))
        .collect();
    let outs = Csr::build(n, &out_pairs);
    let (transfer, transfer_edges) = transfer_walk(n, &edges, &outs);

    // ── levels: the fold, then the route phase, then the cycle tail ────
    let mut flevel = vec![0u32; n];
    for me in (0..n).rev() {
        if let (true, Some(p)) = (is_folder[me], lanes[me].parent) {
            flevel[p as usize] = flevel[p as usize].max(flevel[me] + 1);
        }
    }
    let fold_depth = (0..n)
        .filter(|&f| is_folder[f])
        .map(|f| flevel[f] + 1)
        .max()
        .unwrap_or(0);
    let mut levels: Vec<Box<[u32]>> = Vec::new();
    // Today's publish order for the fold: ONE walk up the channels over
    // every folder. Not the layers concatenated: a shallow folder ranked
    // between a deep one and its parent sits in an earlier layer but
    // publishes between them.
    let fold_publish: Vec<u32> = (0..n as u32)
        .rev()
        .filter(|&f| is_folder[f as usize])
        .collect();
    for k in 0..fold_depth {
        // Descending rank inside a layer, so a layer reads in the same
        // direction as the walk; the publish order is `fold_publish`.
        let layer: Vec<u32> = (0..n as u32)
            .rev()
            .filter(|&f| is_folder[f as usize] && flevel[f as usize] == k)
            .collect();
        levels.push(layer.into_boxed_slice());
    }
    let first_level = levels.len() as u32;
    // The route phase also carries sends INTO lanes (bus rows, from W6):
    // a bus must run after everything that sends to it.
    let mut lreceives = receives.clone();
    for s in &sends {
        if let PlanSendTo::Lane(t) = s.to {
            lreceives[t as usize] = true;
        }
    }
    let mut ldep_pairs: Vec<(usize, u32)> = edges
        .iter()
        .filter_map(lane_edge)
        .map(|(f, t)| (t, f as u32))
        .collect();
    for s in &sends {
        if let (PlanSendFrom::Lane(f), PlanSendTo::Lane(t)) = (s.from, s.to) {
            ldep_pairs.push((t as usize, f));
        }
    }
    ldep_pairs.retain(|&(_, f)| lreceives[f as usize]);
    let ldeps = Csr::build(n, &ldep_pairs);
    let (lplaced, ltail, llevel) = sweep(n, &lreceives, &ldeps);
    let route_depth = lplaced
        .iter()
        .map(|&d| llevel[d as usize] + 1)
        .max()
        .unwrap_or(0);
    for k in 0..route_depth {
        let layer: Vec<u32> = lplaced
            .iter()
            .copied()
            .filter(|&d| llevel[d as usize] == k)
            .collect();
        levels.push(layer.into_boxed_slice());
    }
    // The route phase publishes in the sweep's order (pass, then rank), not
    // layer by layer: a lone destination ranked above a chain's second link
    // is placed, and adds into `mix`, before it.
    let mut route_publish = lplaced.clone();
    route_publish.extend_from_slice(&ltail);
    let cycle_level = !ltail.is_empty();
    if cycle_level {
        levels.push(ltail.into_boxed_slice());
    }

    // ── sends by destination, keys by bay ──────────────────────────────
    let into_pairs: Vec<(usize, u32)> = sends
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let row = match s.to {
                PlanSendTo::Fx => 0,
                PlanSendTo::Lane(t) => t as usize + 1,
            };
            (row, i as u32)
        })
        .collect();
    let into = Csr::build(n + 1, &into_pairs);
    let mut keys: Vec<PlanKey> = inputs
        .keys
        .iter()
        .filter_map(|k| {
            rank(k.from).map(|from| PlanKey {
                seat: k.seat,
                bay: k.bay,
                from,
            })
        })
        .collect();
    keys.sort_by_key(|k| (k.seat, k.bay));
    let mut sources = LaneSet::with_capacity(n);
    for k in &keys {
        sources.insert(k.from as usize);
    }

    let any_solo = lanes.iter().any(|l| l.flags.contains(LaneFlags::SOLOED))
        || [
            inputs.specials.drums,
            inputs.specials.click,
            inputs.specials.fx,
        ]
        .iter()
        .any(|f| f.contains(LaneFlags::SOLOED));

    // ── carry the gains Arc when the edges are the same edges ──────────
    let gains: Arc<[AtomicU32]> = match prev {
        Some(p) if same_edges(p, &lanes, &edges, &sends) => {
            let g = Arc::clone(&p.gains);
            for (cell, v) in g.iter().zip(&gain_values) {
                cell.store(v.to_bits(), Ordering::Relaxed);
            }
            g
        }
        _ => gain_values
            .iter()
            .map(|v| AtomicU32::new(v.to_bits()))
            .collect(),
    };

    DeskPlan {
        gen: prev.map_or(1, |p| p.gen + 1),
        lanes: lanes.into_boxed_slice(),
        by_slot: by_slot.into_boxed_slice(),
        specials: inputs.specials,
        routes: RouteTable {
            edges: edges.into_boxed_slice(),
            order: order.into_boxed_slice(),
            placed: placed.len() as u32,
            inputs: inputs_csr,
            closed,
            hardware,
            transfer: transfer.into_boxed_slice(),
            transfer_edges: transfer_edges.into_boxed_slice(),
            first_level,
            cycle_level,
        },
        sends: SendTable {
            sends: sends.into_boxed_slice(),
            into,
        },
        keys: KeyTable {
            keys: keys.into_boxed_slice(),
            sources,
        },
        levels: Levels(levels.into_boxed_slice()),
        publish: Publish {
            fold: fold_publish.into_boxed_slice(),
            routes: route_publish.into_boxed_slice(),
        },
        gains,
        any_solo,
    }
}

/// Settle [`LaneFlags::SOLO_ABOVE`] and [`LaneFlags::SOLO_BELOW`]: down the
/// ranks for the first, up them for the second. Both are structural (they
/// ignore mutes, exactly as today's two walks do: the up-walk asks only
/// whether something on the chain is soloed, the descendant walk only whether
/// this lane is on a soloed lane's chain), so they belong to the plan.
fn settle_solo_facts(lanes: &mut [PlanLane]) {
    let soloed = |l: &PlanLane| l.flags.contains(LaneFlags::SOLOED);
    for l in lanes.iter_mut() {
        l.flags = l
            .flags
            .with(LaneFlags::SOLO_ABOVE, false)
            .with(LaneFlags::SOLO_BELOW, false);
    }
    for r in 0..lanes.len() {
        if let Some(p) = lanes[r].parent.map(|p| p as usize) {
            let above = soloed(&lanes[p]) || lanes[p].flags.contains(LaneFlags::SOLO_ABOVE);
            lanes[r].flags = lanes[r].flags.with(LaneFlags::SOLO_ABOVE, above);
        }
    }
    for r in (0..lanes.len()).rev() {
        if let Some(p) = lanes[r].parent.map(|p| p as usize) {
            if soloed(&lanes[r]) || lanes[r].flags.contains(LaneFlags::SOLO_BELOW) {
                lanes[p].flags = lanes[p].flags.with(LaneFlags::SOLO_BELOW, true);
            }
        }
    }
}

/// Whether `prev` carries exactly these edges, compared by SLOT so a reorder
/// that moves ranks but not edges still shares one gains table.
fn same_edges(
    prev: &DeskPlan,
    lanes: &[PlanLane],
    edges: &[PlanRoute],
    sends: &[PlanSend],
) -> bool {
    let slot_now = |r: u32| lanes.get(r as usize).map(|l| l.slot);
    let slot_then = |r: u32| prev.lanes.get(r as usize).map(|l| l.slot);
    let route_id = |e: &PlanRoute, at: &dyn Fn(u32) -> Option<Slot>| {
        let to = match e.to {
            RouteTo::Lane(t) => (at(t), None),
            RouteTo::Hardware { first, stereo } => (None, Some((first, stereo))),
        };
        (at(e.from), to)
    };
    let send_id = |s: &PlanSend, at: &dyn Fn(u32) -> Option<Slot>| {
        let from = match s.from {
            PlanSendFrom::Lane(r) => (0u8, at(r)),
            PlanSendFrom::Drums => (1, None),
            PlanSendFrom::Click => (2, None),
        };
        let to = match s.to {
            PlanSendTo::Fx => None,
            PlanSendTo::Lane(r) => at(r),
        };
        (from, to, s.pre)
    };
    prev.routes.edges.len() == edges.len()
        && prev.sends.sends.len() == sends.len()
        && prev.gains.len() == edges.len() + sends.len()
        && prev
            .routes
            .edges
            .iter()
            .zip(edges)
            .all(|(a, b)| route_id(a, &slot_then) == route_id(b, &slot_now))
        && prev
            .sends
            .sends
            .iter()
            .zip(sends)
            .all(|(a, b)| send_id(a, &slot_then) == send_id(b, &slot_now))
}

/// Today's `route_transfer` recursion, run once with an explicit stack so
/// the plan can hand the callback a flat list.
///
/// The recursion visits every lane ascending; a lane's live out-edges are
/// walked in table order, each either finished already (its figure is used),
/// on the walk's own path (a cycle's way back: it contributes nothing), or
/// walked into. Which of the three each edge is depends only on the graph,
/// so it is decided here and the callback only sums.
fn transfer_walk(
    n: usize,
    edges: &[PlanRoute],
    outs: &Csr,
) -> (Vec<TransferStep>, Vec<TransferEdge>) {
    let mut done = vec![false; n];
    let mut on_path = vec![false; n];
    let mut steps = Vec::with_capacity(n);
    let mut flat: Vec<TransferEdge> = Vec::new();
    // Per frame: the lane, how far through its out-edges, and its edges so
    // far (pushed to `flat` only when the lane finalises, so each step's
    // edges are contiguous).
    let mut stack: Vec<(usize, usize, Vec<TransferEdge>)> = Vec::new();
    for root in 0..n {
        if done[root] {
            continue;
        }
        on_path[root] = true;
        stack.push((root, 0, Vec::new()));
        while let Some(top) = stack.last_mut() {
            let (lane, at) = (top.0, top.1);
            let row = outs.row(lane);
            if at < row.len() {
                top.1 += 1;
                let e = row[at];
                let RouteTo::Lane(to) = edges[e as usize].to else {
                    continue;
                };
                let t = to as usize;
                if done[t] {
                    top.2.push(TransferEdge {
                        edge: e,
                        to,
                        cut: false,
                    });
                } else if on_path[t] {
                    top.2.push(TransferEdge {
                        edge: e,
                        to,
                        cut: true,
                    });
                } else {
                    // Walked into: when it finishes it is done, and the edge
                    // reads its figure.
                    top.2.push(TransferEdge {
                        edge: e,
                        to,
                        cut: false,
                    });
                    on_path[t] = true;
                    stack.push((t, 0, Vec::new()));
                }
            } else {
                let (lane, _, mine) = stack.pop().expect("the frame just read");
                on_path[lane] = false;
                done[lane] = true;
                steps.push(TransferStep {
                    lane: lane as u32,
                    first: flat.len() as u32,
                    count: mine.len() as u32,
                });
                flat.extend(mine);
            }
        }
    }
    (steps, flat)
}

impl RouteTable {
    /// What one unit reaching each lane becomes at the master, per side:
    /// today's `route_transfer`, iteratively. `side(rank)` is the lane's
    /// `(gain, left, right)` (its smoothed fader and pan law); `edge_gain(i)`
    /// is route i's gain. `out[rank]` receives the figure; nothing allocates.
    pub fn transfer(
        &self,
        side: impl Fn(usize) -> (f32, f32, f32),
        edge_gain: impl Fn(usize) -> f32,
        out: &mut [[f32; 2]],
    ) {
        for step in self.transfer.iter() {
            let lane = step.lane as usize;
            let mut on = [0.0f32; 2];
            let first = step.first as usize;
            let edges = self
                .transfer_edges
                .get(first..first + step.count as usize)
                .unwrap_or(&[]);
            for e in edges {
                let next = if e.cut {
                    [0.0; 2]
                } else {
                    out.get(e.to as usize).copied().unwrap_or([0.0; 2])
                };
                let g = edge_gain(e.edge as usize);
                on[0] += g * next[0];
                on[1] += g * next[1];
            }
            let (g, pl, pr) = side(lane);
            let direct = if self.closed.contains(lane) { 0.0 } else { 1.0 };
            if let Some(t) = out.get_mut(lane) {
                *t = [g * pl * (direct + on[0]), g * pr * (direct + on[1])];
            }
        }
    }
}

/// Whether each special strip is heard this chunk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpecialHeard {
    pub drums: bool,
    pub click: bool,
    pub fx: bool,
}

/// `strip_is_heard`, for one strip given its own two switches.
///
/// Soloed by name: heard, whatever its own mute says. Muted: not. Otherwise
/// solo silences SOURCES only: a return and the click are exempt (the
/// owner's "when soloed, the main fx in the recorder view do not apply
/// themselves").
fn strip_heard(muted: bool, soloed: bool, any_solo: bool, source: bool) -> bool {
    if soloed {
        return true;
    }
    if muted {
        return false;
    }
    !(any_solo && source)
}

/// **Who is heard this chunk**: today's `muted_now`, `channel_is_heard` and
/// `strip_is_heard`, in two passes over the ranks with no allocation.
///
/// `auto_mute[rank]` is that lane's resolved Mute curve (`None`: the button
/// rules), and `out[rank]` receives the folder-aware answer.
///
/// The walks today climb the folder chain per lane and, for a folder, scan
/// every lane for a soloed descendant. Two of the facts they find are
/// STRUCTURAL: whether a strict ancestor is soloed, and whether a strict
/// descendant is. Solo is a plan input, so [`build_plan`] settles both into
/// [`LaneFlags::SOLO_ABOVE`] and [`LaneFlags::SOLO_BELOW`]. The one fact
/// that moves per chunk is "a strict ancestor is muted", because a Mute
/// curve can change it; parents always sit above their children, so it folds
/// down the ranks in `out` itself, and each lane's answer then needs only
/// its own slot. The `.min(63)` that made lane 64 answer for lane 63 has
/// nowhere to live.
pub fn resolve_heard(
    plan: &DeskPlan,
    auto_mute: &[Option<bool>],
    out: &mut [bool],
) -> SpecialHeard {
    let n = plan.lanes.len().min(out.len());
    let any_solo = plan.any_solo;
    let lanes = &plan.lanes;
    let muted = |r: usize| {
        auto_mute
            .get(r)
            .copied()
            .flatten()
            .unwrap_or_else(|| lanes[r].flags.contains(LaneFlags::MUTED))
    };
    // Down: `out[r]` = a strict ancestor is muted. Checked before solo, and
    // deliberately: an explicit solo overrides the channel's OWN mute, but
    // nothing overrides a group being switched off.
    for r in 0..n {
        out[r] = match lanes[r].parent.map(|p| p as usize) {
            Some(p) if p < r => out[p] || muted(p),
            _ => false,
        };
    }
    for r in 0..n {
        let flags = lanes[r].flags;
        out[r] = if out[r] {
            false
        } else if !any_solo {
            !muted(r)
        } else if flags.contains(LaneFlags::SOLOED) {
            // Soloed by name: heard, whatever its own mute says.
            true
        } else if flags.contains(LaneFlags::SOLO_ABOVE) || flags.contains(LaneFlags::SOLO_BELOW) {
            // Inside a soloed group, or a group holding something soloed:
            // heard, unless its OWN mute says otherwise. Soloing a group is
            // not a request to hear the one track in it somebody switched
            // off, and a group is heard when something inside it is soloed,
            // or soloing a track in a folder mutes the desk ("track 7s solo
            // is a mute for everything").
            !muted(r)
        } else {
            false
        };
    }
    let sp = plan.specials;
    let special = |f: LaneFlags, source: bool| {
        strip_heard(
            f.contains(LaneFlags::MUTED),
            f.contains(LaneFlags::SOLOED),
            any_solo,
            source,
        )
    };
    SpecialHeard {
        drums: special(sp.drums, true),
        click: special(sp.click, false),
        fx: special(sp.fx, false),
    }
}

/// `strip_is_heard(Strip::Channel(r))` with the folder tree left out: the
/// flat question the bus-gain pass asks today. `muted` is the lane's mute
/// with its curve folded in.
pub fn lane_heard_flat(plan: &DeskPlan, rank: usize, muted: bool) -> bool {
    let soloed = plan
        .lanes
        .get(rank)
        .is_some_and(|l| l.flags.contains(LaneFlags::SOLOED));
    strip_heard(muted, soloed, plan.any_solo, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny xorshift, so a failure names a seed and replays.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            if n == 0 {
                0
            } else {
                (self.next() % n as u64) as usize
            }
        }
        fn chance(&mut self, p: f64) -> bool {
            (self.next() % 1_000_000) as f64 / 1_000_000.0 < p
        }
        fn unit(&mut self) -> f32 {
            (self.next() % 1_000_001) as f32 / 1_000_000.0
        }
    }

    // ── frozen copies of today's walks ──────────────────────────────────
    //
    // Transcribed from `instrument.rs` at 624d78a with the desk width made a
    // parameter and the masks made u128, so the copies answer at every
    // width up to 63 lanes plus the three specials. Everything else is
    // verbatim, the `.min(..)` clamp included (at these widths it never
    // fires, which is the point: below the edge the copy IS today's code).

    /// `strip_is_heard`. `index` is the strip's bit; `source` is
    /// `Strip::is_source`.
    fn frozen_strip_is_heard(index: usize, source: bool, muted: u128, soloed: u128) -> bool {
        let bit = 1u128 << index;
        if soloed & bit != 0 {
            return true;
        }
        if muted & bit != 0 {
            return false;
        }
        !(soloed != 0 && source)
    }

    /// `channel_is_heard`.
    fn frozen_channel_is_heard(ch: usize, parents: &[i32], muted: u128, soloed: u128) -> bool {
        let channels = parents.len();
        let bit = |n: usize| 1u128 << n.min(127);
        let mut up = Some(ch);
        let mut hops = 0;
        while let Some(n) = up {
            if hops > channels {
                break;
            }
            if muted & bit(n) != 0 && n != ch {
                return false;
            }
            up = parents
                .get(n)
                .copied()
                .filter(|p| *p >= 0)
                .map(|p| p as usize);
            hops += 1;
        }
        if soloed == 0 {
            return frozen_strip_is_heard(ch, true, muted, soloed);
        }
        let mut up = Some(ch);
        let mut hops = 0;
        while let Some(n) = up {
            if hops > channels {
                break;
            }
            if soloed & bit(n) != 0 {
                return n == ch || muted & bit(ch) == 0;
            }
            up = parents
                .get(n)
                .copied()
                .filter(|p| *p >= 0)
                .map(|p| p as usize);
            hops += 1;
        }
        for kid in 0..channels {
            if kid == ch || soloed & bit(kid) == 0 {
                continue;
            }
            let mut up = parents
                .get(kid)
                .copied()
                .filter(|p| *p >= 0)
                .map(|p| p as usize);
            let mut hops = 0;
            while let Some(n) = up {
                if hops > channels {
                    break;
                }
                if n == ch {
                    return muted & bit(ch) == 0;
                }
                up = parents
                    .get(n)
                    .copied()
                    .filter(|p| *p >= 0)
                    .map(|p| p as usize);
                hops += 1;
            }
        }
        false
    }

    /// `Renderer::muted_now`.
    fn frozen_muted_now(muted: u128, auto: &[Option<bool>]) -> u128 {
        let mut out = muted;
        for (lane, a) in auto.iter().enumerate() {
            match a {
                Some(true) => out |= 1 << lane,
                Some(false) => out &= !(1 << lane),
                None => {}
            }
        }
        out
    }

    /// The Kahn sweep in the sends pass, over today's latched
    /// `(from, target, hardware, gain)` table, `CHANNELS` made `n` (at most
    /// 32: the masks are u32, as today).
    fn frozen_kahn(n: usize, cache: &[(usize, usize, bool, f32)]) -> Vec<usize> {
        let mut receives: u32 = 0;
        for &(_, target, hardware, gain) in cache {
            if !hardware && gain > 0.0 && target < n {
                receives |= 1 << target;
            }
        }
        let mut deps = [0u32; 32];
        for &(from, target, hardware, gain) in cache {
            if hardware || gain <= 0.0 || target >= n || from >= n {
                continue;
            }
            if receives & (1 << from) != 0 {
                deps[target] |= 1 << from;
            }
        }
        let mut order = Vec::new();
        let mut placed: u32 = 0;
        for _ in 0..n {
            let mut moved = false;
            for dest in 0..n {
                let bit = 1u32 << dest;
                if receives & bit == 0 || placed & bit != 0 {
                    continue;
                }
                if deps[dest] & !placed == 0 {
                    order.push(dest);
                    placed |= bit;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        for dest in 0..n {
            let bit = 1u32 << dest;
            if receives & bit != 0 && placed & bit == 0 {
                order.push(dest);
            }
        }
        order
    }

    /// The latch at the top of the block: drop what names a lane the desk
    /// does not have, clamp the gain. Returns the cache and each kept
    /// route's index in the input.
    fn frozen_latch(
        n: usize,
        routes: &[(usize, usize, bool, f32)],
    ) -> Vec<(usize, usize, bool, f32)> {
        routes
            .iter()
            .filter(|&&(from, target, hardware, _)| !(from >= n || (!hardware && target >= n)))
            .map(|&(f, t, h, g)| {
                (
                    f,
                    t,
                    h,
                    if g.is_finite() {
                        g.clamp(0.0, 1.0)
                    } else {
                        0.0
                    },
                )
            })
            .collect()
    }

    /// `route_direct`: closed by any non-hardware route out, and for a
    /// folder and a row in one.
    fn frozen_route_direct(
        n: usize,
        cache: &[(usize, usize, bool, f32)],
        parents: &[i32],
    ) -> Vec<f32> {
        let mut direct = vec![1.0f32; n];
        for &(from, _, hardware, _) in cache {
            if !hardware {
                if let Some(d) = direct.get_mut(from) {
                    *d = 0.0;
                }
            }
        }
        let mut folder_is = vec![false; n];
        for p in parents {
            if *p >= 0 {
                folder_is[*p as usize] = true;
            }
        }
        for ch in 0..n {
            if parents[ch] >= 0 || folder_is[ch] {
                direct[ch] = 0.0;
            }
        }
        direct
    }

    /// `Renderer::route_transfer`, `CHANNELS` made `n`.
    fn frozen_transfer(
        n: usize,
        cache: &[(usize, usize, bool, f32)],
        gain: &[f32],
        pan: &[(f32, f32)],
        direct: &[f32],
    ) -> Vec<[f32; 2]> {
        #[allow(clippy::too_many_arguments)]
        fn walk(
            n: usize,
            cache: &[(usize, usize, bool, f32)],
            gain: &[f32],
            pan: &[(f32, f32)],
            direct: &[f32],
            d: usize,
            path: u64,
            t: &mut [[f32; 2]],
            done: &mut u64,
        ) -> [f32; 2] {
            let bit = 1u64 << d;
            if *done & bit != 0 {
                return t[d];
            }
            if path & bit != 0 {
                return [0.0; 2];
            }
            let mut on = [0.0f32; 2];
            for &(from, target, hardware, g) in cache {
                if from != d || hardware || g <= 0.0 || target >= n {
                    continue;
                }
                let next = walk(n, cache, gain, pan, direct, target, path | bit, t, done);
                on[0] += g * next[0];
                on[1] += g * next[1];
            }
            let (pl, pr) = pan[d];
            let g = gain[d];
            t[d] = [g * pl * (direct[d] + on[0]), g * pr * (direct[d] + on[1])];
            *done |= bit;
            t[d]
        }
        let mut t = vec![[0.0f32; 2]; n];
        let mut done = 0u64;
        for d in 0..n {
            walk(n, cache, gain, pan, direct, d, 0, &mut t, &mut done);
        }
        t
    }

    // ── fixtures ────────────────────────────────────────────────────────

    fn lane(slot: u32, flags: LaneFlags, parent: Option<u32>) -> InLane {
        InLane {
            slot: Slot(slot),
            gen: 1,
            flags,
            parent: parent.map(Slot),
            punch: None,
            midi: MidiRoute::EVERYTHING,
            pick: None,
        }
    }

    /// A random folder tree over `n` rows: each row is in a folder above it
    /// with some chance, and the slots are a shuffle (so the plan's rank
    /// translation is exercised) when `shuffle`.
    fn random_tree(rng: &mut Rng, n: usize, shuffle: bool) -> (Vec<i32>, Vec<u32>) {
        let parents: Vec<i32> = (0..n)
            .map(|c| {
                if c > 0 && rng.chance(0.4) {
                    rng.below(c) as i32
                } else {
                    -1
                }
            })
            .collect();
        let mut slots: Vec<u32> = (0..n as u32).map(|s| s * 3 + 1).collect();
        if shuffle {
            for i in (1..n).rev() {
                let j = rng.below(i + 1);
                slots.swap(i, j);
            }
        } else {
            slots = (0..n as u32).collect();
        }
        (parents, slots)
    }

    struct Soup {
        parents: Vec<i32>,
        slots: Vec<u32>,
        muted: Vec<bool>,
        soloed: Vec<bool>,
        auto: Vec<Option<bool>>,
        /// Drums, click, fx: (muted, soloed).
        special: [(bool, bool); 3],
    }

    fn soup(rng: &mut Rng, n: usize) -> Soup {
        let shuffle = rng.chance(0.5);
        let (parents, slots) = random_tree(rng, n, shuffle);
        let solo_p = [0.0, 0.03, 0.3][rng.below(3)];
        let mute_p = [0.0, 0.1, 0.4][rng.below(3)];
        let muted = (0..n).map(|_| rng.chance(mute_p)).collect();
        let soloed = (0..n).map(|_| rng.chance(solo_p)).collect();
        let auto = (0..n)
            .map(|_| match rng.below(5) {
                0 => Some(true),
                1 => Some(false),
                _ => None,
            })
            .collect();
        let special = [0; 3].map(|_| (rng.chance(0.2), rng.chance(solo_p)));
        Soup {
            parents,
            slots,
            muted,
            soloed,
            auto,
            special,
        }
    }

    fn plan_of(s: &Soup) -> DeskPlan {
        let flags = |m: bool, so: bool| {
            LaneFlags::default()
                .with(LaneFlags::MUTED, m)
                .with(LaneFlags::SOLOED, so)
        };
        let lanes = (0..s.parents.len())
            .map(|r| {
                let parent = (s.parents[r] >= 0).then(|| s.slots[s.parents[r] as usize]);
                lane(s.slots[r], flags(s.muted[r], s.soloed[r]), parent)
            })
            .collect();
        let sp = |i: usize| flags(s.special[i].0, s.special[i].1);
        build_plan(
            &PlanInputs {
                lanes,
                specials: SpecialFlags {
                    drums: sp(0),
                    click: sp(1),
                    fx: sp(2),
                    master: LaneFlags::default(),
                },
                ..PlanInputs::default()
            },
            None,
        )
    }

    /// A reference with no width at all: the same rules as the frozen copy,
    /// over `Vec<bool>`, climbing chains as today does. Anchored to the
    /// frozen copy below 64 lanes, then used where the copy cannot reach.
    fn reference_heard(s: &Soup) -> (Vec<bool>, [bool; 3]) {
        let n = s.parents.len();
        let muted: Vec<bool> = (0..n).map(|r| s.auto[r].unwrap_or(s.muted[r])).collect();
        let any_solo = s.soloed.iter().any(|x| *x) || s.special.iter().any(|x| x.1);
        let chain = |mut r: usize| {
            let mut up = Vec::new();
            while s.parents[r] >= 0 {
                r = s.parents[r] as usize;
                up.push(r);
            }
            up
        };
        let lanes = (0..n)
            .map(|r| {
                let up = chain(r);
                if up.iter().any(|&a| muted[a]) {
                    return false;
                }
                if !any_solo {
                    return !muted[r];
                }
                if s.soloed[r] {
                    return true;
                }
                let above = up.iter().any(|&a| s.soloed[a]);
                let below = (0..n).any(|k| k != r && s.soloed[k] && chain(k).contains(&r));
                (above || below) && !muted[r]
            })
            .collect();
        let sp = |i: usize, source: bool| {
            let (m, so) = s.special[i];
            so || (!m && !(any_solo && source))
        };
        (lanes, [sp(0, true), sp(1, false), sp(2, false)])
    }

    // ── resolve_heard ───────────────────────────────────────────────────

    /// **`resolve_heard` is today's three functions, exactly**: over random
    /// flag soups at every width up to 63 lanes (folders, mutes, solos, Mute
    /// curves, the specials, slots shuffled against ranks), each lane equals
    /// `channel_is_heard` over `muted_now`, the flat form equals
    /// `strip_is_heard`, and each special equals `strip_is_heard` on its
    /// strip.
    #[test]
    fn resolve_heard_equals_the_frozen_walks_up_to_63_lanes() {
        let mut rng = Rng(0x00c0_ffee_1234_5678);
        let mut out = vec![false; 64];
        for trial in 0..20_000 {
            let n = 1 + rng.below(63);
            let s = soup(&mut rng, n);
            let plan = plan_of(&s);
            let special = resolve_heard(&plan, &s.auto, &mut out);
            // Today's masks: lanes at their index, specials past them.
            let mut muted: u128 = 0;
            let mut soloed: u128 = 0;
            for r in 0..n {
                muted |= u128::from(s.muted[r]) << r;
                soloed |= u128::from(s.soloed[r]) << r;
            }
            for (i, (m, so)) in s.special.iter().enumerate() {
                muted |= u128::from(*m) << (n + i);
                soloed |= u128::from(*so) << (n + i);
            }
            let now = frozen_muted_now(muted, &s.auto);
            for r in 0..n {
                let want = frozen_channel_is_heard(r, &s.parents, now, soloed);
                assert_eq!(out[r], want, "trial {trial}: lane {r} of {n}");
                let flat = frozen_strip_is_heard(r, true, now, soloed);
                assert_eq!(
                    lane_heard_flat(&plan, r, now & (1 << r) != 0),
                    flat,
                    "trial {trial}: lane {r}'s flat answer"
                );
            }
            let want = SpecialHeard {
                drums: frozen_strip_is_heard(n, true, now, soloed),
                click: frozen_strip_is_heard(n + 1, false, now, soloed),
                fx: frozen_strip_is_heard(n + 2, false, now, soloed),
            };
            assert_eq!(special, want, "trial {trial}: the specials");
            let (r_lanes, r_sp) = reference_heard(&s);
            assert_eq!(
                &out[..n],
                &r_lanes[..],
                "trial {trial}: the width-free reference disagrees"
            );
            assert_eq!([special.drums, special.click, special.fx], r_sp);
        }
    }

    /// **And past every mask**, at 300 lanes, against the width-free
    /// reference the test above anchors. Lane 64 no longer answers for lane
    /// 63, nor lane 299 for anybody.
    #[test]
    fn resolve_heard_equals_the_reference_at_300_lanes() {
        let mut rng = Rng(0x0300_0300_0300_0301);
        let mut out = vec![false; 300];
        for trial in 0..300 {
            let s = soup(&mut rng, 300);
            let plan = plan_of(&s);
            let special = resolve_heard(&plan, &s.auto, &mut out);
            let (want, sp) = reference_heard(&s);
            assert_eq!(out, want, "trial {trial}");
            assert_eq!(
                [special.drums, special.click, special.fx],
                sp,
                "trial {trial}"
            );
        }
        // And it allocates nothing doing it.
        let s = soup(&mut rng, 300);
        let plan = plan_of(&s);
        // `here`: the walk runs on this thread only, and the hands' figure
        // would only catch a neighbour test's pool by its thread names.
        let (_, c) = crate::rt_alloc::count_allocs(|| resolve_heard(&plan, &s.auto, &mut out));
        assert_eq!(c.here, 0, "resolve_heard allocated: {c:?}");
    }

    /// The two derived bits are the plan's to set: whatever the host hands
    /// over in them is cleared, or a stale bit would hear a lane.
    #[test]
    fn the_derived_solo_bits_are_never_taken_from_the_inputs() {
        let forged = LaneFlags::SOLO_ABOVE | LaneFlags::SOLO_BELOW;
        let plan = build_plan(
            &PlanInputs {
                lanes: vec![lane(0, forged, None), lane(1, LaneFlags::SOLOED, None)],
                ..PlanInputs::default()
            },
            None,
        );
        let mut out = [false; 2];
        resolve_heard(&plan, &[], &mut out);
        assert_eq!(out, [false, true], "a forged derived bit was believed");
    }

    // ── routes ──────────────────────────────────────────────────────────

    /// A random route table over `n` lanes, naming lanes the desk does not
    /// have now and then (as a shrinking desk does), with hardware routes,
    /// zero and out-of-range gains and self-routes.
    fn random_routes(rng: &mut Rng, n: usize) -> Vec<(usize, usize, bool, f32)> {
        let count = rng.below(65);
        (0..count)
            .map(|_| {
                let from = rng.below(n + 2);
                let to = rng.below(n + 2);
                let hardware = rng.chance(0.15);
                let gain = match rng.below(10) {
                    0 | 1 => 0.0,
                    2 => 1.5,
                    3 if rng.chance(0.3) => f32::NAN,
                    _ => rng.unit(),
                };
                (from, to, hardware, gain)
            })
            .collect()
    }

    fn route_inputs(
        n: usize,
        slots: &[u32],
        parents: &[i32],
        routes: &[(usize, usize, bool, f32)],
    ) -> PlanInputs {
        // A lane index past the desk maps to a slot no lane holds.
        let slot = |i: usize| {
            if i < n {
                Slot(slots[i])
            } else {
                Slot(10_000 + i as u32)
            }
        };
        PlanInputs {
            lanes: (0..n)
                .map(|r| {
                    let parent = (parents[r] >= 0).then(|| slots[parents[r] as usize]);
                    lane(slots[r], LaneFlags::default(), parent)
                })
                .collect(),
            routes: routes
                .iter()
                .map(|&(f, t, h, g)| InRoute {
                    from: slot(f),
                    to: if h {
                        RouteEnd::Hardware {
                            first: t as u16,
                            stereo: f % 2 == 0,
                        }
                    } else {
                        RouteEnd::Lane(slot(t))
                    },
                    gain: g,
                })
                .collect(),
            ..PlanInputs::default()
        }
    }

    /// **The plan's route order is today's Kahn walk, on 10,000 random
    /// graphs** with cycles, self-routes, zero gains, hardware routes and
    /// routes to lanes the desk does not have; and so are the gathers (each
    /// destination's inputs, in table order), the closed direct paths, and
    /// the transfer figures, bit for bit.
    #[test]
    fn the_route_order_is_todays_kahn_walk_on_random_graphs() {
        let mut rng = Rng(0x5eed_0000_0000_0042);
        for trial in 0..10_000 {
            let n = 1 + rng.below(32);
            let shuffle = rng.chance(0.5);
            let (parents, slots) = random_tree(&mut rng, n, shuffle);
            let parents: Vec<i32> = if rng.chance(0.5) {
                parents
            } else {
                vec![-1; n]
            };
            let raw = random_routes(&mut rng, n);
            let plan = build_plan(&route_inputs(n, &slots, &parents, &raw), None);
            let cache = frozen_latch(n, &raw);
            assert_eq!(
                plan.routes.edges.len(),
                cache.len(),
                "trial {trial}: the latch"
            );

            let want = frozen_kahn(n, &cache);
            let got: Vec<usize> = plan.routes.order.iter().map(|r| *r as usize).collect();
            assert_eq!(got, want, "trial {trial}: n {n}, routes {cache:?}");

            for (pos, &dest) in want.iter().enumerate() {
                let gathered: Vec<u32> = cache
                    .iter()
                    .enumerate()
                    .filter(|(_, &(_, t, h, g))| !h && g > 0.0 && t == dest)
                    .map(|(i, _)| i as u32)
                    .collect();
                assert_eq!(
                    plan.routes.inputs.row(pos),
                    &gathered[..],
                    "trial {trial}: gather of {dest}"
                );
            }

            let direct = frozen_route_direct(n, &cache, &parents);
            for (r, d) in direct.iter().enumerate() {
                assert_eq!(
                    plan.routes.closed.contains(r),
                    *d == 0.0,
                    "trial {trial}: lane {r}'s direct path"
                );
            }
            assert_eq!(
                plan.routes.hardware,
                cache.iter().any(|r| r.2),
                "trial {trial}"
            );

            let gain: Vec<f32> = (0..n).map(|_| rng.unit() * 1.5).collect();
            let pan: Vec<(f32, f32)> = (0..n).map(|_| (rng.unit(), rng.unit())).collect();
            let want = frozen_transfer(n, &cache, &gain, &pan, &direct);
            let mut got = vec![[f32::NAN; 2]; n];
            plan.routes.transfer(
                |r| (gain[r], pan[r].0, pan[r].1),
                |e| plan.gain(plan.routes.edges[e].gain),
                &mut got,
            );
            for r in 0..n {
                for c in 0..2 {
                    assert_eq!(
                        got[r][c].to_bits(),
                        want[r][c].to_bits(),
                        "trial {trial}: lane {r} side {c}: {} against {}",
                        got[r][c],
                        want[r][c]
                    );
                }
            }
        }
    }

    /// The cases worth reading, with literal answers.
    #[test]
    fn a_chain_runs_in_dependency_order_and_a_cycle_runs_last() {
        let ids: Vec<u32> = (0..8).collect();
        let none = vec![-1; 8];
        // 3 -> 2 -> 1: the sweep places 2 in pass 0 and 1 in pass 1.
        let p = build_plan(
            &route_inputs(8, &ids, &none, &[(3, 2, false, 1.0), (2, 1, false, 1.0)]),
            None,
        );
        assert_eq!(&p.routes.order[..], &[2, 1]);
        assert_eq!(p.routes.placed, 2);
        // The golden desk's cycle: 1 <-> 2, 4 -> 4, both into 3.
        let p = build_plan(
            &route_inputs(
                8,
                &ids,
                &none,
                &[
                    (1, 2, false, 0.6),
                    (2, 3, false, 0.5),
                    (4, 3, false, 0.5),
                    (2, 5, false, 0.0),
                    (2, 1, false, 0.4),
                    (4, 4, false, 0.3),
                ],
            ),
            None,
        );
        assert_eq!(
            &p.routes.order[..],
            &[1, 2, 3, 4],
            "all four hang off a cycle"
        );
        assert_eq!(p.routes.placed, 0);
        assert!(p.routes.cycle_level);
        assert_eq!(
            p.levels.0.last().map(|l| &l[..]),
            Some(&[1u32, 2, 3, 4][..])
        );
        // Lane 5's only route in has no gain: it is not a destination, but
        // lane 2's direct path is still closed by it, as today.
        assert!(!p.routes.order.contains(&5));
    }

    // ── levels ──────────────────────────────────────────────────────────

    /// **The levels are a topological layering**: the
    /// fold first (deepest folders first, descending rank within a layer,
    /// as `fold_folders` walks), then the route phase by longest feeder
    /// chain (Kahn order within a layer), then every cycle member and what
    /// hangs off one, ascending, in one final level. Checked against a naive
    /// fixpoint layering on random desks with folders, routes and sends into
    /// lanes.
    #[test]
    fn the_levels_are_a_reference_topological_layering() {
        let mut rng = Rng(0x1e7e_1500_0000_0007);
        for trial in 0..5_000 {
            let n = 1 + rng.below(32);
            let shuffle = rng.chance(0.5);
            let (parents, slots) = random_tree(&mut rng, n, shuffle);
            let raw = random_routes(&mut rng, n);
            let lane_sends: Vec<(usize, usize)> = (0..rng.below(6))
                .map(|_| (rng.below(n), rng.below(n)))
                .collect();
            let mut inputs = route_inputs(n, &slots, &parents, &raw);
            inputs.sends = lane_sends
                .iter()
                .map(|&(f, t)| InSend {
                    from: SendFrom::Lane(Slot(slots[f])),
                    to: SendTo::Lane(Slot(slots[t])),
                    level: 0.5,
                    pre: false,
                })
                .collect();
            inputs.sends.push(InSend {
                from: SendFrom::Drums,
                to: SendTo::Fx,
                level: 0.5,
                pre: false,
            });
            let plan = build_plan(&inputs, None);

            // The fold: depth by fixpoint.
            let is_folder: Vec<bool> = (0..n).map(|f| parents.contains(&(f as i32))).collect();
            let mut depth = vec![0u32; n];
            for _ in 0..n {
                for c in 0..n {
                    if is_folder[c] && parents[c] >= 0 {
                        let p = parents[c] as usize;
                        depth[p] = depth[p].max(depth[c] + 1);
                    }
                }
            }
            let mut want: Vec<Vec<u32>> = Vec::new();
            let fold_depth = (0..n)
                .filter(|f| is_folder[*f])
                .map(|f| depth[f] + 1)
                .max()
                .unwrap_or(0);
            for k in 0..fold_depth {
                want.push(
                    (0..n as u32)
                        .rev()
                        .filter(|f| is_folder[*f as usize] && depth[*f as usize] == k)
                        .collect(),
                );
            }
            assert_eq!(
                plan.routes.first_level as usize,
                want.len(),
                "trial {trial}"
            );

            // The route phase: live lane routes plus sends into lanes, as
            // gain-1 edges, through the frozen sweep for the order.
            let mut edges: Vec<(usize, usize, bool, f32)> = frozen_latch(n, &raw)
                .into_iter()
                .filter(|&(_, _, h, g)| !h && g > 0.0)
                .collect();
            edges.extend(lane_sends.iter().map(|&(f, t)| (f, t, false, 1.0)));
            let order = frozen_kahn(n, &edges);
            let receivers: Vec<bool> = (0..n).map(|d| edges.iter().any(|e| e.1 == d)).collect();
            let mut level: Vec<Option<u32>> = vec![None; n];
            for _ in 0..=n {
                for d in 0..n {
                    if !receivers[d] || level[d].is_some() {
                        continue;
                    }
                    let feeders: Vec<usize> = edges
                        .iter()
                        .filter(|e| e.1 == d && receivers[e.0])
                        .map(|e| e.0)
                        .collect();
                    if feeders.iter().all(|f| level[*f].is_some()) {
                        level[d] = Some(
                            feeders
                                .iter()
                                .map(|f| level[*f].unwrap() + 1)
                                .max()
                                .unwrap_or(0),
                        );
                    }
                }
            }
            let top = level.iter().flatten().map(|l| l + 1).max().unwrap_or(0);
            for k in 0..top {
                want.push(
                    order
                        .iter()
                        .filter(|d| level[**d] == Some(k))
                        .map(|d| *d as u32)
                        .collect(),
                );
            }
            let cycle: Vec<u32> = (0..n)
                .filter(|d| receivers[*d] && level[*d].is_none())
                .map(|d| d as u32)
                .collect();
            assert_eq!(plan.routes.cycle_level, !cycle.is_empty(), "trial {trial}");
            if !cycle.is_empty() {
                want.push(cycle);
            }
            let got: Vec<Vec<u32>> = plan.levels.0.iter().map(|l| l.to_vec()).collect();
            assert_eq!(
                got, want,
                "trial {trial}: parents {parents:?} edges {edges:?}"
            );
        }
    }

    /// **The sums publish in today's serial order, which is not the levels'
    /// order.** The desks the review found, with literal answers: the layers
    /// concatenated would add into `mix` and `aux` in a different order, and
    /// f32 `+=` is not associative, so a runner that published layer by
    /// layer would move the folder and route goldens.
    #[test]
    fn the_publish_order_crosses_the_levels_where_rank_and_depth_disagree() {
        let ids: Vec<u32> = (0..10).collect();
        let none = vec![-1; 10];
        let flat = |p: &DeskPlan, phase: std::ops::Range<usize>| -> Vec<u32> {
            p.levels.0[phase]
                .iter()
                .flat_map(|l| l.iter().copied())
                .collect()
        };

        // Folder 2 holds lane 3; folder 7 holds folder 8, which holds lane 9.
        let mut parents = vec![-1; 10];
        parents[3] = 2;
        parents[8] = 7;
        parents[9] = 8;
        let p = build_plan(&route_inputs(10, &ids, &parents, &[]), None);
        let fold = p.routes.first_level as usize;
        let layers: Vec<&[u32]> = p.levels.0[..fold].iter().map(|l| &l[..]).collect();
        assert_eq!(layers, [&[8u32, 2][..], &[7][..]]);
        assert_eq!(&p.publish.fold[..], &[8, 7, 2], "fold_folders' one walk");
        assert_ne!(flat(&p, 0..fold), p.publish.fold.to_vec());

        // Folder 2 holds row 3; folder 4 holds folder 5, which holds row 6:
        // contiguous, as a desk draws it.
        let mut parents = vec![-1; 10];
        parents[3] = 2;
        parents[5] = 4;
        parents[6] = 5;
        let p = build_plan(&route_inputs(10, &ids, &parents, &[]), None);
        let fold = p.routes.first_level as usize;
        assert_eq!(flat(&p, 0..fold), [5, 2, 4]);
        assert_eq!(&p.publish.fold[..], &[5, 4, 2]);

        // Routes 0 -> 1, 1 -> 2, 0 -> 3: Kahn places 1, 2, 3 in one pass;
        // the layers are [1, 3] then [2].
        let p = build_plan(
            &route_inputs(
                10,
                &ids,
                &none,
                &[(0, 1, false, 1.0), (1, 2, false, 1.0), (0, 3, false, 1.0)],
            ),
            None,
        );
        let fold = p.routes.first_level as usize;
        assert_eq!(fold, 0);
        let all = p.levels.0.len();
        assert_eq!(flat(&p, 0..all), [1, 3, 2]);
        assert_eq!(&p.routes.order[..], &[1, 2, 3]);
        assert_eq!(&p.publish.routes[..], &p.routes.order[..]);

        // 2 -> 3, and a lone destination 5 fed by a lane that is none.
        let p = build_plan(
            &route_inputs(
                10,
                &ids,
                &none,
                &[(2, 3, false, 1.0), (0, 2, false, 1.0), (4, 5, false, 1.0)],
            ),
            None,
        );
        let all = p.levels.0.len();
        assert_eq!(flat(&p, 0..all), [2, 5, 3]);
        assert_eq!(&p.publish.routes[..], &[2, 3, 5]);
    }

    /// **The publish order is today's two walks, on random desks**:
    /// `fold_folders`' descending walk over every folder, and the sends
    /// pass's Kahn order (the frozen sweep, with sends into lanes as more
    /// edges); and each phase's publish order is a permutation of its own
    /// levels, so deferring the sums to the phase's end publishes everything
    /// once.
    #[test]
    fn the_publish_order_is_todays_serial_walks() {
        let mut rng = Rng(0x9b11_5400_0000_0011);
        for trial in 0..5_000 {
            let n = 1 + rng.below(32);
            let shuffle = rng.chance(0.5);
            let (parents, slots) = random_tree(&mut rng, n, shuffle);
            let raw = random_routes(&mut rng, n);
            let lane_sends: Vec<(usize, usize)> = if rng.chance(0.5) {
                Vec::new()
            } else {
                (0..rng.below(6))
                    .map(|_| (rng.below(n), rng.below(n)))
                    .collect()
            };
            let mut inputs = route_inputs(n, &slots, &parents, &raw);
            inputs.sends = lane_sends
                .iter()
                .map(|&(f, t)| InSend {
                    from: SendFrom::Lane(Slot(slots[f])),
                    to: SendTo::Lane(Slot(slots[t])),
                    level: 0.5,
                    pre: false,
                })
                .collect();
            let plan = build_plan(&inputs, None);

            // `fold_folders`: `for ch in (0..CHANNELS).rev()`, skipping
            // whatever is not `folder_is`.
            let folder_is: Vec<bool> = (0..n).map(|f| parents.contains(&(f as i32))).collect();
            let fold: Vec<u32> = (0..n as u32)
                .rev()
                .filter(|f| folder_is[*f as usize])
                .collect();
            assert_eq!(plan.publish.fold.to_vec(), fold, "trial {trial}");

            let mut edges: Vec<(usize, usize, bool, f32)> = frozen_latch(n, &raw)
                .into_iter()
                .filter(|&(_, _, h, g)| !h && g > 0.0)
                .collect();
            if lane_sends.is_empty() {
                assert_eq!(
                    plan.publish.routes, plan.routes.order,
                    "trial {trial}: today's desk publishes in the sends pass's order"
                );
            }
            edges.extend(lane_sends.iter().map(|&(f, t)| (f, t, false, 1.0)));
            let kahn: Vec<u32> = frozen_kahn(n, &edges)
                .into_iter()
                .map(|d| d as u32)
                .collect();
            assert_eq!(plan.publish.routes.to_vec(), kahn, "trial {trial}");

            let first = plan.routes.first_level as usize;
            let sorted = |v: Vec<u32>| {
                let mut v = v;
                v.sort_unstable();
                v
            };
            let phase = |r: std::ops::Range<usize>| -> Vec<u32> {
                sorted(
                    plan.levels.0[r]
                        .iter()
                        .flat_map(|l| l.iter().copied())
                        .collect(),
                )
            };
            assert_eq!(phase(0..first), sorted(fold), "trial {trial}");
            assert_eq!(
                phase(first..plan.levels.0.len()),
                sorted(kahn),
                "trial {trial}"
            );
        }
    }

    // ── identity, gains, the rest ───────────────────────────────────────

    /// **A plan speaks ranks; its inputs speak slots.** A parent that does
    /// not sit above its child is dropped, as `set_folders` refuses one, and
    /// a slot past every lane is no rank.
    #[test]
    fn slots_become_ranks_and_a_parent_below_its_child_is_dropped() {
        let plan = build_plan(
            &PlanInputs {
                lanes: vec![
                    lane(40, LaneFlags::default(), None),
                    lane(7, LaneFlags::default(), Some(40)),
                    lane(300, LaneFlags::default(), Some(7)),
                    lane(2, LaneFlags::default(), Some(300)),
                ],
                keys: vec![
                    InKey {
                        seat: 35,
                        bay: 0,
                        from: Slot(300),
                    },
                    InKey {
                        seat: 0,
                        bay: 2,
                        from: Slot(999),
                    },
                ],
                ..PlanInputs::default()
            },
            None,
        );
        assert_eq!(plan.rank_of(Slot(300)), Some(2));
        assert_eq!(plan.rank_of(Slot(2)), Some(3));
        assert_eq!(plan.rank_of(Slot(3)), None);
        assert_eq!(plan.rank_of(Slot(100_000)), None);
        let parents: Vec<Option<u32>> = plan.lanes.iter().map(|l| l.parent).collect();
        assert_eq!(parents, vec![None, Some(0), Some(1), Some(2)]);
        let bad = build_plan(
            &PlanInputs {
                lanes: vec![
                    lane(0, LaneFlags::default(), Some(1)),
                    lane(1, LaneFlags::default(), None),
                ],
                ..PlanInputs::default()
            },
            None,
        );
        assert_eq!(
            bad.lanes[0].parent, None,
            "a parent below its child was kept"
        );
        assert_eq!(
            &plan.keys.keys[..],
            &[PlanKey {
                seat: 35,
                bay: 0,
                from: 2
            }],
            "a key from no lane was kept"
        );
        assert!(plan.keys.sources.contains(2));
    }

    /// **The gains table is carried when the edges are the same edges**, by
    /// slot, so a reorder (ranks move, slots do not) keeps writing into the
    /// table the callback holds; and a new edge gets a new table.
    #[test]
    fn the_gains_follow_the_edges_not_the_ranks() {
        let route = |f: u32, t: u32, g: f32| InRoute {
            from: Slot(f),
            to: RouteEnd::Lane(Slot(t)),
            gain: g,
        };
        let mut inputs = PlanInputs {
            lanes: vec![
                lane(5, LaneFlags::default(), None),
                lane(9, LaneFlags::default(), None),
            ],
            routes: vec![route(5, 9, 0.5)],
            sends: vec![InSend {
                from: SendFrom::Lane(Slot(9)),
                to: SendTo::Fx,
                level: 0.25,
                pre: false,
            }],
            ..PlanInputs::default()
        };
        let a = build_plan(&inputs, None);
        assert_eq!(a.gen, 1);
        assert_eq!(a.gain(a.routes.edges[0].gain), 0.5);
        assert_eq!(a.gain(a.sends.sends[0].gain), 0.25);
        // A reorder, and a gain that moved but did not cross zero.
        inputs.lanes.reverse();
        inputs.routes[0].gain = 0.75;
        let b = build_plan(&inputs, Some(&a));
        assert_eq!(b.gen, 2);
        assert!(
            Arc::ptr_eq(&a.gains, &b.gains),
            "a reorder bought a new gains table"
        );
        assert_eq!(a.gain(0), 0.75, "the carried table holds the new gain");
        assert_eq!(
            b.routes.edges[0].from, 1,
            "the route's source moved rank with its lane"
        );
        // A new edge is a new table.
        inputs.routes.push(route(9, 5, 0.1));
        let c = build_plan(&inputs, Some(&b));
        assert!(
            !Arc::ptr_eq(&b.gains, &c.gains),
            "a new edge wrote into the old table"
        );
        assert_eq!(c.gains.len(), 3);
        assert_eq!(
            c.sends.into.row(0),
            &[0],
            "the send lands on Return 0's row"
        );
    }

    /// **`any_solo` counts the specials**, as today's `soloed != 0` does: a
    /// soloed drum bus silences the lanes and spares the click and the
    /// return.
    #[test]
    fn a_soloed_special_is_a_solo() {
        let plan = build_plan(
            &PlanInputs {
                lanes: vec![lane(0, LaneFlags::default(), None)],
                specials: SpecialFlags {
                    drums: LaneFlags::SOLOED,
                    ..SpecialFlags::default()
                },
                ..PlanInputs::default()
            },
            None,
        );
        assert!(plan.any_solo);
        let mut out = [true];
        let sp = resolve_heard(&plan, &[], &mut out);
        assert_eq!(out, [false]);
        assert_eq!(
            sp,
            SpecialHeard {
                drums: true,
                click: true,
                fx: true
            }
        );
    }
}
