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
//! Proven against frozen copies of today's functions (W1-rt-kernels-goldens),
//! consumed by the slot engine (W3), and since W6-engine-buses-sends the one
//! thing the callback's post-fork work reads: every route, send, bus, folder
//! and the delay compensation through them come out of [`PostTable`] and
//! [`RouteTable`], so no route order is worked out on the audio thread.

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
/// A `u32` because there are thirteen inputs and four derived bits, and a
/// lane carries one copy in the plan; the masks they replace were one word
/// for the whole desk, and that word is what filled at 32 and 64. It was a
/// `u16` until [`LaneFlags::LEFT_OUT`] needed the seventeenth bit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LaneFlags(pub u32);

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
    /// **Derived, never an input**: FEEDS a soloed bus (design 9: a soloed
    /// bus hears its feeders). Set on every lane routed or sent into a
    /// soloed bus, on the children of a folder that feeds one, and on up
    /// their own feeders. A feeder is heard WHOLE, its own way out and its
    /// send alike, as a solo-in-place desk plays the sources of a soloed
    /// return; and the same wherever it sits, so the folders it sits in
    /// carry it ([`Self::SOLO_CARRY`]). Such a lane is heard unless its own
    /// mute says otherwise, as [`Self::SOLO_ABOVE`] is. A desk with no bus
    /// never sets it.
    pub const SOLO_PATH: Self = Self(1 << 14);
    /// **Derived, never an input**: CARRIES what is heard through it to the
    /// master while something else is soloed, and nothing of its own. Set
    /// on every folder a bus sits in and every late folder a bus is routed
    /// into (a bus is never silenced by another strip's solo, design 9, so
    /// nothing on its way out may be), and on every folder a feeder of a
    /// soloed bus sits in. Such a folder is heard unless its own mute says
    /// otherwise, so its racks, its fader and its onward paths pass what
    /// reaches it; but its OWN ROW, the clips, kit and input standing on it,
    /// is no carried thing and stays silent while the solo stands
    /// ([`PostTable::row_quiet`]), and its other children answer to the solo
    /// as they always did. A desk with no bus never sets it.
    pub const SOLO_CARRY: Self = Self(1 << 15);
    /// **Left out of this EXPORT's mix**: an export of selected tracks
    /// (`ProjectExportSpec::tracks`) hears none of what this lane makes or
    /// sends, whatever its mute button, its Mute curve or a solo says. Not a
    /// mute: the desk's own mute is the person's and is never written by an
    /// export, so the export asks for silence with a bit of its own, set
    /// only on the plan of the pass (`engine_bridge::export_left_out`). It
    /// silences what stands below it as a muted folder does.
    pub const LEFT_OUT: Self = Self(1 << 16);
    /// **A folder's OWN ROW left out of this export**, and nothing else of
    /// it: the clips and the instrument standing on the folder row are
    /// silent (the folder joins [`PostTable::row_quiet`]) while its fader
    /// still carries its children to the master. What a folder's checklist
    /// row ("own clips") answers; see `engine_bridge::export_left_out`.
    pub const ROW_LEFT_OUT: Self = Self(1 << 17);

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
/// (only MUTED, SOLOED, PHASE, SUSPEND and MIDI_OFF mean anything here).
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

/// Where a send lands: Return 0 (the built-in effects) or a lane. **Only a
/// BUS lane takes a send** (design 9: "to Fx or to any bus row"): a send
/// naming a lane that is not a bus, or naming its own source, is not a send
/// and [`build_plan`] leaves it out, as it leaves out a route naming a lane
/// the plan does not play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendTo {
    Fx,
    Lane(Slot),
}

/// One send as the host hands it over, in the host's order: that order is
/// the order a bus sums its sends in.
///
/// **A strip's `Fx` entries ARE its send to Return 0** (design 9: today's
/// knob becomes the Fx entry, and `AutoParam::Send` stays the Fx send). A
/// strip the plan carries any Fx entry for is heard in the return through
/// its entries and no longer through its knob (`LaneShared::send`): a
/// post-fader entry is summed exactly where the knob was, from the same
/// signal, in the same order and on the same frame, so a bridge that
/// carries the knob as the strip's post-fader entry at the knob's level
/// moves no bit; a pre-fader one is summed beside it, from the strip's
/// signal before its fader, on that same frame. `AutoParam::Send` drives
/// the strip's entries (`SendTable::auto`), as it drove the knob. A strip
/// with no entry keeps its knob, so a plan that carries none plays today's
/// desk. Only a send to a BUS is gathered from the strip's send buffers,
/// read off the strip's own compensation line at the bus path's delay.
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
    /// **Ranks with a route out to a lane, whatever its gain**: today's
    /// "this strip is routed" question, which the room asks of a live input
    /// (`closed` cannot answer it: a folder and a row inside one are closed
    /// too).
    pub routed: LaneSet,
    /// **Every hardware route, by edge index, in table order**: what the
    /// out-pass walks per frame, so a desk of a thousand lane routes pays
    /// nothing at the jacks for them.
    pub hardware_edges: Box<[u32]>,
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SendTable {
    pub sends: Box<[PlanSend]>,
    pub into: Csr,
    /// **Each send by its identity**, `(from, to, index)` sorted, for the
    /// curves that automate one (`Engine::set_send_automation`): a curve
    /// names a send by its two ends, and a plan numbers its sends its own
    /// way. See [`send_key`].
    pub by_ends: Box<[(u64, u64, u32)]>,
    /// **Each send's automated level this chunk**, by index, or `None`
    /// where no curve drives it and the level is the gains cell's. The
    /// callback's own: written once a chunk by its automation pass, read by
    /// the returns that gather the send. The plan is the callback's while it
    /// plays, so this needs no atomics; the host's twin never reads it.
    pub auto: Box<[Option<f32>]>,
}

impl SendTable {
    /// **The sends from `from` to `to`**, by index: usually one, two for a
    /// strip sending to one return both pre- and post-fader. The plan's own
    /// passes walk the table by return; this is the tests' way in by ends.
    #[cfg(test)]
    pub fn find(&self, ends: (u64, u64)) -> impl Iterator<Item = u32> + '_ {
        let at = self.by_ends.partition_point(|&(f, t, _)| (f, t) < ends);
        self.by_ends[at..]
            .iter()
            .take_while(move |&&(f, t, _)| (f, t) == ends)
            .map(|&(_, _, i)| i)
    }
}

/// **A send's two ends as one sortable key**: the source (a lane by its
/// slot, the drum bus or the click past every slot) and the destination
/// (a lane by its slot, Return 0 past every slot). Slots, not ranks: a curve
/// is filed by identity and must find its send in whichever plan plays.
pub fn send_key(from: SendFrom, to: SendTo) -> (u64, u64) {
    let from = match from {
        SendFrom::Lane(s) => u64::from(s.0),
        SendFrom::Drums => u64::MAX - 1,
        SendFrom::Click => u64::MAX,
    };
    let to = match to {
        SendTo::Lane(s) => u64::from(s.0),
        SendTo::Fx => u64::MAX,
    };
    (from, to)
}

/// **What a post-fork node is** (design 7.8): a rank whose rack, or racks,
/// run AFTER the lane fork, over what other lanes put out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// A folder, in the fold phase: its own row and its children summed, its
    /// board and its two racks run once, at its fader. Today's
    /// `fold_folders`, transcribed.
    Folder,
    /// **A legacy channel-route destination**, in the route phase: what is
    /// routed to it gathered in table order and run through its OWN first
    /// rack a second time (the SEND-BARGAIN), at its clip gain. Today's sends
    /// pass, transcribed bit for bit, its cycle rule included.
    Dest,
    /// **A bus row**: its children (a bus can head a folder run), what is
    /// routed to it and what is sent to it, summed once and run once through
    /// its board and both racks, at its own fader, and out to the master
    /// when nothing holds it and it is routed nowhere ([`PostTable::direct`]).
    /// New in W6, and the only node whose sound is new.
    Bus,
    /// **A folder with a bus somewhere under it**, which therefore cannot
    /// fold until that bus has run: it moves from the fold phase into the
    /// route phase, gathers as a folder does (and what is routed to it, as a
    /// bus does), and runs once. Only a desk with a bus has one, so no
    /// existing desk's fold moves.
    LateFolder,
}

/// **The post-fork graph, per rank** (design 7.8): which ranks are nodes and
/// of which kind, what each one gathers, and the delay-compensation graph
/// through them.
///
/// A rank can be TWO nodes: a folder that is also a legacy route
/// destination folds its children in the fold phase and runs its rack over
/// its arrivals in the route phase, as today. `fold` is its fold-phase
/// role and `route` its route-phase role; [`DeskPlan::levels`] says which
/// phase a listed rank is in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostTable {
    /// Per rank: an early folder, which runs in the fold phase.
    pub fold: LaneSet,
    /// Per rank: its role in the route phase, if it has one.
    pub route: Box<[Option<NodeKind>]>,
    /// Per rank: its folder children, ascending rank (`fold_folders`' kid
    /// walk).
    pub kids: Csr,
    /// Per rank: the live lane routes into it, by edge index, in table
    /// order (the sends pass's gather, per destination).
    pub routes_in: Csr,
    /// Per rank: where its own output's compensated path goes. Its folder,
    /// and every bus or late folder it is routed into: the paths whose
    /// racks its wait must cover (design 9: a bus destination gets PDC on
    /// its paths). Routes into a legacy destination are not here, as today.
    pub pdc_main: Csr,
    /// Per rank: the buses it sends to, whose racks its send tap covers.
    pub pdc_send: Csr,
    /// Every rank once, each after every rank in its `pdc_main` and
    /// `pdc_send` (a post-order; a cycle's way back is cut), so one walk in
    /// this order settles every path's latency.
    pub pdc_order: Box<[u32]>,
    /// Ranks with a send to a BUS: their send buffers are filled, and only
    /// theirs. A send to Return 0 needs no buffer: it is summed where the
    /// strip's knob was (see [`InSend`]).
    pub sends_out: LaneSet,
    /// Ranks with a pre-fader send to a bus.
    pub sends_pre: LaneSet,
    /// Whether the plan has a bus at all: what makes the per-lane stem
    /// buffer wanted when nothing else does, since a bus publishes its
    /// output there for whatever reads it on (a route, a key, the jacks).
    pub buses: bool,
    /// Whether the drum bus sends to a bus: its send buffers are filled.
    pub drums_sends: bool,
    /// **The folders and buses whose own output goes straight to the
    /// master**, by rank. A folder's when no folder holds it and it is
    /// routed into no bus or late folder: today's `fold_folders` rule
    /// (`to_master`, whatever the folder is routed to) for every desk with
    /// no bus, since a route into a legacy destination copies as it always
    /// did, while a route into a bus MOVES the group, as it moves a track.
    /// A bus's when nothing holds it and it is routed to no lane, whether
    /// or not it heads a folder run of its own. A legacy destination's
    /// arrivals keep its `route_direct`.
    pub direct: LaneSet,
    /// **The folders whose own row is silent while this plan plays**: a
    /// folder heard only to carry a bus, or a soloed bus's feeder, to the
    /// master while something else is soloed ([`LaneFlags::SOLO_CARRY`],
    /// with nothing of its own soloed, inside a soloed group, or feeding).
    /// Its fader passes what it carries; the clips, kit and input standing
    /// on its row are not that.
    pub row_quiet: LaneSet,
    /// **What reaches the master from what lands at each node, through
    /// gains**, on a desk with a bus (empty on one without, whose room reads
    /// [`RouteTable::transfer`] and the folder chain as it always did): the
    /// steps of a walk that finalises each node's figure after everything it
    /// feeds, and their terms. See [`PostTable::carry_into`].
    pub carry: Box<[CarryStep]>,
    pub carry_terms: Box<[CarryTerm]>,
}

/// **One step of the bus-aware transfer**: `rank`'s figure is final once its
/// `count` terms from `first` in [`PostTable::carry_terms`] are summed.
///
/// A rank has up to two figures, for what lands in its two gathers: an early
/// folder's FOLD figure (its children and its row, in the fold phase), and a
/// node's ROUTE figure (what a legacy destination, a bus or a late folder
/// gathers in the route phase). A legacy destination's arrivals go out at its
/// clip gain and never through its folder, as the sends pass runs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarryStep {
    pub rank: u32,
    /// The fold figure; else the route figure.
    pub fold: bool,
    /// What it gathers reaches the master straight from its fader.
    pub direct: bool,
    /// A legacy destination's arrivals: at its clip gain, never its folder.
    pub dest: bool,
    pub first: u32,
    pub count: u32,
}

/// The way one carry term leaves its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryVia {
    /// Into its folder (or the bus heading its run), past its fader.
    Parent,
    /// Along a live lane route, by edge index, past its fader.
    Route(u32),
    /// A post-fader send to a bus, by send index.
    Post(u32),
    /// A pre-fader send to a bus, by send index: ahead of its fader, through
    /// its gate.
    Pre(u32),
}

/// One term of a carry step: where its node's audio goes on to, and which of
/// that node's figures it reads. `cut` is a cycle's way back, which a saved
/// file can still hold: it contributes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarryTerm {
    pub via: CarryVia,
    pub to: u32,
    pub fold: bool,
    pub cut: bool,
}

/// What one node does to a unit that lands in it, for [`PostTable::carry_into`]:
/// its gain and pan law per side (a destination's clip gain, every other
/// node's fader), its gate for a pre-fader send, and its own share into
/// Return 0, post-fader and pre-fader.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarrySide {
    pub gain: f32,
    pub pan: (f32, f32),
    pub gate: f32,
    pub fx_post: f32,
    pub fx_pre: f32,
}

/// Index of a rank's figure in the table [`PostTable::carry_into`] fills.
pub fn carry_at(rank: u32, fold: bool) -> usize {
    rank as usize * 2 + usize::from(fold)
}

impl PostTable {
    /// **What one unit landing at each node becomes, through gains**: per
    /// figure ([`carry_at`]), `[master left, master right, return left,
    /// return right]`, where the return is Return 0's input. `side(rank,
    /// dest)` is the node's own law; `level(via)` a route's gain or a send's
    /// level. A node's processing is taken as unity, as the room's
    /// subtrahend always took a folder's rack and a destination's. Nothing
    /// allocates; `out` is as long as twice the plan's lanes.
    pub fn carry_into(
        &self,
        side: impl Fn(usize, bool) -> CarrySide,
        level: impl Fn(CarryVia) -> f32,
        out: &mut [[f32; 4]],
    ) {
        for step in self.carry.iter() {
            let s = side(step.rank as usize, step.dest);
            let d = if step.direct { 1.0 } else { 0.0 };
            // Past the fader: the master's share, then Return 0's.
            let mut inside = [d, d, s.fx_post, s.fx_post];
            // Ahead of it, through the gate.
            let mut ahead = [0.0f32, 0.0, s.fx_pre * s.gate, s.fx_pre * s.gate];
            let first = step.first as usize;
            let terms = self.carry_terms.get(first..first + step.count as usize).unwrap_or(&[]);
            for t in terms {
                let next = if t.cut {
                    [0.0; 4]
                } else {
                    out.get(carry_at(t.to, t.fold)).copied().unwrap_or([0.0; 4])
                };
                match t.via {
                    CarryVia::Parent => {
                        for k in 0..4 {
                            inside[k] += next[k];
                        }
                    }
                    CarryVia::Route(_) | CarryVia::Post(_) => {
                        let g = level(t.via);
                        for k in 0..4 {
                            inside[k] += g * next[k];
                        }
                    }
                    CarryVia::Pre(_) => {
                        let g = level(t.via) * s.gate;
                        for k in 0..4 {
                            ahead[k] += g * next[k];
                        }
                    }
                }
            }
            let (pl, pr) = s.pan;
            let law = [s.gain * pl, s.gain * pr, s.gain * pl, s.gain * pr];
            if let Some(cell) = out.get_mut(carry_at(step.rank, step.fold)) {
                for k in 0..4 {
                    cell[k] = law[k] * inside[k] + ahead[k];
                }
            }
        }
    }
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
/// The fold phase's layers hold early folders ([`NodeKind::Folder`]); the
/// route phase's hold legacy destinations, buses and late folders, by their
/// [`PostTable::route`] role. Each layer's nodes depend only on earlier
/// layers, so a layer may run its nodes in parallel, and each node may write
/// its OWN stem column (nobody else writes it, so the order of those writes
/// is no sum) before the next layer starts, because that column is what the
/// next layer reads.
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
    /// Every early folder, descending rank: `fold_folders`' one walk up the
    /// channels, whatever each folder's depth.
    pub fold: Box<[u32]>,
    /// Every route-phase node: the sweep's placed order, then the cycle tail.
    /// On a desk with no bus (every desk before W6) this is exactly
    /// [`RouteTable::order`]. A bus and a late folder are placed by the same
    /// rule, with what is sent to a bus and what a folder holds as more
    /// feeders.
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
    /// Which ranks are post-fork nodes, what each gathers, and the delay
    /// compensation through them.
    pub post: PostTable,
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
    let is_bus: Vec<bool> = lanes.iter().map(|l| l.flags.contains(LaneFlags::BUS)).collect();

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
    let mut by_ends: Vec<(u64, u64, u32)> = Vec::new();
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
            // **A bus takes a send, and nothing else does**, and nothing
            // sends to itself: see `SendTo`.
            SendTo::Lane(sl) => match rank(sl) {
                Some(t) if is_bus[t as usize] && from != PlanSendFrom::Lane(t) => PlanSendTo::Lane(t),
                _ => continue,
            },
        };
        let (f, t) = send_key(s.from, s.to);
        by_ends.push((f, t, sends.len() as u32));
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
    by_ends.sort_unstable();

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
    let mut routed = LaneSet::with_capacity(n);
    for e in &edges {
        if matches!(e.to, RouteTo::Lane(_)) {
            closed.insert(e.from as usize);
            routed.insert(e.from as usize);
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
    let hardware_edges: Vec<u32> = edges
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.to, RouteTo::Hardware { .. }))
        .map(|(i, _)| i as u32)
        .collect();

    // ── the transfer, as today's recursion finalises it ────────────────
    let out_pairs: Vec<(usize, u32)> = edges
        .iter()
        .enumerate()
        .filter_map(|(i, e)| lane_edge(e).map(|(f, _)| (f, i as u32)))
        .collect();
    let outs = Csr::build(n, &out_pairs);
    let (transfer, transfer_edges) = transfer_walk(n, &edges, &outs);

    // ── the post-fork nodes ────────────────────────────────────────────
    //
    // **A folder holding a bus anywhere under it folds after that bus**, in
    // the route phase (a late folder); every other folder folds where it
    // always did. Children rank below their parent, so one walk down the
    // ranks carries "a bus is under here" up every chain.
    let mut bus_below = is_bus.clone();
    for r in (0..n).rev() {
        if let (true, Some(p)) = (bus_below[r], lanes[r].parent) {
            bus_below[p as usize] = true;
        }
    }
    let route_role: Vec<Option<NodeKind>> = (0..n)
        .map(|r| {
            if is_bus[r] {
                Some(NodeKind::Bus)
            } else if is_folder[r] && bus_below[r] {
                Some(NodeKind::LateFolder)
            } else if receives[r] {
                Some(NodeKind::Dest)
            } else {
                None
            }
        })
        .collect();
    let early: Vec<bool> = (0..n).map(|f| is_folder[f] && !bus_below[f]).collect();
    let kid_pairs: Vec<(usize, u32)> = lanes
        .iter()
        .enumerate()
        .filter_map(|(me, l)| l.parent.map(|p| (p as usize, me as u32)))
        .collect();
    let kids = Csr::build(n, &kid_pairs);
    let routes_in = Csr::build(
        n,
        &edges
            .iter()
            .enumerate()
            .filter_map(|(i, e)| lane_edge(e).map(|(_, t)| (t, i as u32)))
            .collect::<Vec<_>>(),
    );

    // ── levels: the fold, then the route phase, then the cycle tail ────
    let mut flevel = vec![0u32; n];
    for me in (0..n).rev() {
        if let (true, Some(p)) = (early[me], lanes[me].parent) {
            flevel[p as usize] = flevel[p as usize].max(flevel[me] + 1);
        }
    }
    let fold_depth = (0..n)
        .filter(|&f| early[f])
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
        .filter(|&f| early[f as usize])
        .collect();
    for k in 0..fold_depth {
        // Descending rank inside a layer, so a layer reads in the same
        // direction as the walk; the publish order is `fold_publish`.
        let layer: Vec<u32> = (0..n as u32)
            .rev()
            .filter(|&f| early[f as usize] && flevel[f as usize] == k)
            .collect();
        levels.push(layer.into_boxed_slice());
    }
    let first_level = levels.len() as u32;
    // **The route phase**: every legacy destination, bus and late folder,
    // each after every one of them it reads. A destination reads what is
    // routed to it; a bus that, what is sent to it and its own children; a
    // late folder its children and what is routed to it. On a desk with no
    // bus this is today's sweep over the destinations exactly.
    let in_route: Vec<bool> = route_role.iter().map(Option::is_some).collect();
    let mut rdep_pairs: Vec<(usize, u32)> = edges
        .iter()
        .filter_map(lane_edge)
        .filter(|&(f, _)| in_route[f])
        .map(|(f, t)| (t, f as u32))
        .collect();
    for s in &sends {
        if let (PlanSendFrom::Lane(f), PlanSendTo::Lane(t)) = (s.from, s.to) {
            if in_route[f as usize] {
                rdep_pairs.push((t as usize, f));
            }
        }
    }
    for &(p, kid) in &kid_pairs {
        if matches!(route_role[p], Some(NodeKind::Bus | NodeKind::LateFolder)) && in_route[kid as usize] {
            rdep_pairs.push((p, kid));
        }
    }
    let rdeps = Csr::build(n, &rdep_pairs);
    let (rplaced, rtail, rlevel) = sweep(n, &in_route, &rdeps);
    let route_depth = rplaced
        .iter()
        .map(|&d| rlevel[d as usize] + 1)
        .max()
        .unwrap_or(0);
    for k in 0..route_depth {
        let layer: Vec<u32> = rplaced
            .iter()
            .copied()
            .filter(|&d| rlevel[d as usize] == k)
            .collect();
        levels.push(layer.into_boxed_slice());
    }
    // The route phase publishes in the sweep's order (pass, then rank), not
    // layer by layer: a lone destination ranked above a chain's second link
    // is placed, and adds into `mix`, before it.
    let mut route_publish = rplaced.clone();
    route_publish.extend_from_slice(&rtail);
    let cycle_level = !rtail.is_empty();
    if cycle_level {
        levels.push(rtail.into_boxed_slice());
    }

    // ── the compensation graph ─────────────────────────────────────────
    //
    // A lane's audio pays the racks it is folded or routed THROUGH on its
    // way out, so its wait must leave room for them: its folder's (today's
    // `folder_lat`), and now a bus's or a late folder's it is routed into.
    // A route into a legacy destination pays that destination's rack
    // uncompensated, as it always has (the SEND-BARGAIN is bit for bit).
    let mut main_pairs: Vec<(usize, u32)> = lanes
        .iter()
        .enumerate()
        .filter_map(|(me, l)| l.parent.map(|p| (me, p)))
        .collect();
    for e in &edges {
        if let Some((f, t)) = lane_edge(e) {
            if matches!(route_role[t], Some(NodeKind::Bus | NodeKind::LateFolder)) {
                main_pairs.push((f, t as u32));
            }
        }
    }
    let pdc_main = Csr::build(n, &main_pairs);
    let send_pairs: Vec<(usize, u32)> = sends
        .iter()
        .filter_map(|s| match (s.from, s.to) {
            (PlanSendFrom::Lane(f), PlanSendTo::Lane(t)) => Some((f as usize, t)),
            _ => None,
        })
        .collect();
    let pdc_send = Csr::build(n, &send_pairs);
    let pdc_order = downstream_first(n, &pdc_main, &pdc_send);

    // ── who sends to a bus ─────────────────────────────────────────────
    //
    // Only a send to a bus is gathered from its strip's send buffers; one
    // to Return 0 is summed where the strip's knob was (see `InSend`).
    let mut sends_out = LaneSet::with_capacity(n);
    let mut sends_pre = LaneSet::with_capacity(n);
    for s in &sends {
        if let (PlanSendFrom::Lane(f), PlanSendTo::Lane(_)) = (s.from, s.to) {
            sends_out.insert(f as usize);
            if s.pre {
                sends_pre.insert(f as usize);
            }
        }
    }

    // ── straight to the master ─────────────────────────────────────────
    //
    // A folder whose group is routed into a bus or a late folder is MOVED
    // there, as a track routed into one is; one routed into a legacy
    // destination keeps going out as well, as `fold_folders` always sent
    // it. A bus heading a folder run is a folder too, so `closed` holds it,
    // and its own way out is asked here instead. Any lane route counts,
    // whatever its gain, as `closed` counts one.
    let carrier = |t: usize| matches!(route_role[t], Some(NodeKind::Bus | NodeKind::LateFolder));
    let mut into_carrier = vec![false; n];
    for e in &edges {
        if let RouteTo::Lane(t) = e.to {
            if carrier(t as usize) {
                into_carrier[e.from as usize] = true;
            }
        }
    }
    let mut direct = LaneSet::with_capacity(n);
    for r in 0..n {
        if lanes[r].parent.is_some() {
            continue;
        }
        let goes = match route_role[r] {
            Some(NodeKind::Bus) => !routed.contains(r),
            Some(NodeKind::LateFolder) => !into_carrier[r],
            _ => is_folder[r] && !into_carrier[r],
        };
        if goes {
            direct.insert(r);
        }
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

    // ── solo on a bus's path ───────────────────────────────────────────
    let carries: Vec<bool> = (0..n).map(carrier).collect();
    settle_solo_path(&mut lanes, &edges, &sends, &kids, &is_bus, &carries);

    let any_solo = lanes.iter().any(|l| l.flags.contains(LaneFlags::SOLOED))
        || [
            inputs.specials.drums,
            inputs.specials.click,
            inputs.specials.fx,
        ]
        .iter()
        .any(|f| f.contains(LaneFlags::SOLOED));
    // A folder heard only to carry: its own row is no carried thing. And a
    // folder whose own row an export leaves out (`ROW_LEFT_OUT`).
    let mut row_quiet = LaneSet::with_capacity(n);
    for (r, l) in lanes.iter().enumerate() {
        if l.flags.contains(LaneFlags::ROW_LEFT_OUT) {
            row_quiet.insert(r);
        }
    }
    if any_solo {
        for (r, l) in lanes.iter().enumerate() {
            let f = l.flags;
            let own = [LaneFlags::SOLOED, LaneFlags::SOLO_ABOVE, LaneFlags::SOLO_BELOW, LaneFlags::SOLO_PATH];
            if f.contains(LaneFlags::SOLO_CARRY) && !own.iter().any(|b| f.contains(*b)) {
                row_quiet.insert(r);
            }
        }
    }

    // ── the bus-aware transfer, on a desk with a bus ───────────────────
    let (carry, carry_terms) = if is_bus.iter().any(|b| *b) {
        let bus_sends: Vec<(usize, u32)> = sends
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match (s.from, s.to) {
                (PlanSendFrom::Lane(f), PlanSendTo::Lane(_)) => Some((f as usize, i as u32)),
                _ => None,
            })
            .collect();
        let send_outs = Csr::build(n, &bus_sends);
        carry_walk(&CarryGraph {
            lanes: &lanes,
            early: &early,
            role: &route_role,
            direct: &direct,
            closed: &closed,
            edges: &edges,
            outs: &outs,
            sends: &sends,
            send_outs: &send_outs,
        })
    } else {
        (Vec::new(), Vec::new())
    };

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

    let sends_n = sends.len();
    let drums_sends = sends.iter().any(|s| s.from == PlanSendFrom::Drums && s.to != PlanSendTo::Fx);
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
            routed,
            hardware_edges: hardware_edges.into_boxed_slice(),
        },
        sends: SendTable {
            sends: sends.into_boxed_slice(),
            into,
            by_ends: by_ends.into_boxed_slice(),
            auto: vec![None; sends_n].into_boxed_slice(),
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
        post: PostTable {
            fold: {
                let mut set = LaneSet::with_capacity(n);
                for (f, e) in early.iter().enumerate() {
                    if *e {
                        set.insert(f);
                    }
                }
                set
            },
            buses: is_bus.iter().any(|b| *b),
            drums_sends,
            route: route_role.into_boxed_slice(),
            kids,
            routes_in,
            pdc_main,
            pdc_send,
            pdc_order: pdc_order.into_boxed_slice(),
            sends_out,
            sends_pre,
            direct,
            row_quiet,
            carry: carry.into_boxed_slice(),
            carry_terms: carry_terms.into_boxed_slice(),
        },
        gains,
        any_solo,
    }
}

/// What [`carry_walk`] reads of the plan being built.
struct CarryGraph<'a> {
    lanes: &'a [PlanLane],
    early: &'a [bool],
    role: &'a [Option<NodeKind>],
    direct: &'a LaneSet,
    closed: &'a LaneSet,
    edges: &'a [PlanRoute],
    /// The live lane routes out of each rank, by edge, in table order.
    outs: &'a Csr,
    sends: &'a [PlanSend],
    /// The sends to a bus out of each rank, by send, in the host's order.
    send_outs: &'a Csr,
}

impl CarryGraph<'_> {
    /// A rank's figure for what lands in it from a CHILD: an early folder's
    /// fold figure, a bus's or late folder's route figure.
    fn parent_figure(&self, p: usize) -> bool {
        self.early.get(p).copied().unwrap_or(false)
    }

    /// The terms of figure `(r, fold)`, in a fixed order: its folder, its
    /// routes in table order, its sends in the host's order.
    fn terms(&self, r: usize, fold: bool) -> Vec<(CarryVia, u32, bool)> {
        let mut out = Vec::new();
        let dest = !fold && self.role.get(r).copied().flatten() == Some(NodeKind::Dest);
        if !dest {
            if let Some(p) = self.lanes.get(r).and_then(|l| l.parent) {
                out.push((CarryVia::Parent, p, self.parent_figure(p as usize)));
            }
        }
        for &e in self.outs.row(r) {
            if let Some(RouteTo::Lane(t)) = self.edges.get(e as usize).map(|x| x.to) {
                out.push((CarryVia::Route(e), t, false));
            }
        }
        for &i in self.send_outs.row(r) {
            if let Some(s) = self.sends.get(i as usize) {
                if let PlanSendTo::Lane(t) = s.to {
                    out.push((if s.pre { CarryVia::Pre(i) } else { CarryVia::Post(i) }, t, false));
                }
            }
        }
        out
    }
}

/// **The bus-aware transfer's walk**: every node's figure once, each after
/// every figure it reads, with an explicit stack; an edge back onto the
/// walk's own path (a saved cycle) is cut. A figure exists for each early
/// folder (its fold figure) and each legacy destination, bus and late folder
/// (its route figure); every term names one that exists, since a route's
/// target, a send's bus and a folder are all nodes.
fn carry_walk(g: &CarryGraph<'_>) -> (Vec<CarryStep>, Vec<CarryTerm>) {
    let n = g.lanes.len();
    let exists = |r: usize, fold: bool| {
        if fold {
            g.early.get(r).copied().unwrap_or(false)
        } else {
            g.role.get(r).copied().flatten().is_some()
        }
    };
    // 0 unseen, 1 on the walk, 2 finished; by `carry_at`.
    let mut state = vec![0u8; n * 2];
    let mut steps = Vec::new();
    let mut flat: Vec<CarryTerm> = Vec::new();
    // Per frame: the figure, its terms, how far through them, what it summed.
    let mut stack: Vec<(usize, bool, Vec<(CarryVia, u32, bool)>, usize, Vec<CarryTerm>)> = Vec::new();
    for root in 0..n {
        for fold in [false, true] {
            if !exists(root, fold) || state[carry_at(root as u32, fold)] != 0 {
                continue;
            }
            state[carry_at(root as u32, fold)] = 1;
            stack.push((root, fold, g.terms(root, fold), 0, Vec::new()));
            while let Some(top) = stack.last_mut() {
                if top.3 < top.2.len() {
                    let (via, to, tfold) = top.2[top.3];
                    top.3 += 1;
                    let at = carry_at(to, tfold);
                    if to as usize >= n || !exists(to as usize, tfold) {
                        continue;
                    }
                    let cut = state[at] == 1;
                    top.4.push(CarryTerm { via, to, fold: tfold, cut });
                    if state[at] == 0 {
                        state[at] = 1;
                        let terms = g.terms(to as usize, tfold);
                        stack.push((to as usize, tfold, terms, 0, Vec::new()));
                    }
                } else {
                    let (r, fold, _, _, mine) = stack.pop().expect("the frame just read");
                    state[carry_at(r as u32, fold)] = 2;
                    let dest = !fold && g.role.get(r).copied().flatten() == Some(NodeKind::Dest);
                    let direct = if dest { !g.closed.contains(r) } else { g.direct.contains(r) };
                    steps.push(CarryStep {
                        rank: r as u32,
                        fold,
                        direct,
                        dest,
                        first: flat.len() as u32,
                        count: mine.len() as u32,
                    });
                    flat.extend(mine);
                }
            }
        }
    }
    (steps, flat)
}

/// **Every rank once, each after everything downstream of it**: a
/// depth-first post-order over `main` and `send`, with an explicit stack. An
/// edge back onto the walk's own path (a cycle a saved file can still hold)
/// is cut, so the walk ends and the latencies it settles stay finite.
fn downstream_first(n: usize, main: &Csr, send: &Csr) -> Vec<u32> {
    // 0 unseen, 1 on the walk, 2 finished.
    let mut state = vec![0u8; n];
    let mut out = Vec::with_capacity(n);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        stack.push((root, 0));
        while let Some(top) = stack.last_mut() {
            let (u, at) = *top;
            let (a, b) = (main.row(u), send.row(u));
            if at < a.len() + b.len() {
                top.1 += 1;
                let v = if at < a.len() { a[at] } else { b[at - a.len()] } as usize;
                if v < n && state[v] == 0 {
                    state[v] = 1;
                    stack.push((v, 0));
                }
            } else {
                stack.pop();
                state[u] = 2;
                out.push(u as u32);
            }
        }
    }
    out
}

/// **Settle [`LaneFlags::SOLO_PATH`] and [`LaneFlags::SOLO_CARRY`]** once
/// the routes and sends are known. PATH upstream of every SOLOED bus
/// (whatever is routed or sent into it, what a folder feeding it holds, and
/// on up); CARRY on every folder one of those feeders sits in, so a feeder
/// is heard the same wherever it sits, and downstream of EVERY bus (the
/// folders holding it, the late folders it is routed into, and theirs). The
/// downstream half is set whether or not anything is soloed; [`resolve_heard`]
/// reads both only while something is.
fn settle_solo_path(
    lanes: &mut [PlanLane],
    edges: &[PlanRoute],
    sends: &[PlanSend],
    kids: &Csr,
    is_bus: &[bool],
    // A bus or a late folder: a node that runs what is routed into it once
    // and carries it on. A route into anything else is a legacy
    // destination's, whose arrivals have never reached the master through a
    // folder, and the solo does not follow it there either.
    carries: &[bool],
) {
    let n = lanes.len();
    if !is_bus.iter().any(|b| *b) {
        return;
    }
    let mut mark = vec![false; n];
    let mut walk: Vec<usize> = Vec::new();
    // Upstream of a soloed bus: its feeders, theirs, and a feeding folder's
    // children.
    for r in 0..n {
        if is_bus[r] && lanes[r].flags.contains(LaneFlags::SOLOED) {
            walk.push(r);
        }
    }
    let mut seen = vec![false; n];
    while let Some(x) = walk.pop() {
        if std::mem::replace(&mut seen[x], true) {
            continue;
        }
        let mut reach = |s: usize, walk: &mut Vec<usize>| {
            if s < n {
                mark[s] = true;
                walk.push(s);
            }
        };
        for e in edges {
            if let (RouteTo::Lane(t), true) = (e.to, e.live) {
                if t as usize == x {
                    reach(e.from as usize, &mut walk);
                }
            }
        }
        for s in sends {
            if let (PlanSendFrom::Lane(f), PlanSendTo::Lane(t)) = (s.from, s.to) {
                if t as usize == x {
                    reach(f as usize, &mut walk);
                }
            }
        }
        for &k in kids.row(x) {
            reach(k as usize, &mut walk);
        }
    }
    // **The folders a feeder sits in carry it**: a feeder in a folder is
    // heard as a loose one is, through that folder's fader, and the folder
    // passes nothing else of its own.
    let mut carry = vec![false; n];
    for x in 0..n {
        if !mark[x] {
            continue;
        }
        let mut up = lanes[x].parent;
        while let Some(p) = up.map(|p| p as usize) {
            if std::mem::replace(&mut carry[p], true) {
                break;
            }
            up = lanes[p].parent;
        }
    }
    // Downstream of every bus: where its output travels.
    let mut down = vec![false; n];
    let mut walk: Vec<usize> = (0..n).filter(|r| is_bus[*r]).collect();
    while let Some(x) = walk.pop() {
        if std::mem::replace(&mut down[x], true) {
            continue;
        }
        if let Some(p) = lanes[x].parent {
            carry[p as usize] = true;
            walk.push(p as usize);
        }
        for e in edges {
            if let (RouteTo::Lane(t), true) = (e.to, e.live) {
                let t = t as usize;
                if e.from as usize == x && carries.get(t).copied().unwrap_or(false) {
                    carry[t] = true;
                    walk.push(t);
                }
            }
        }
    }
    for ((l, m), c) in lanes.iter_mut().zip(mark).zip(carry) {
        l.flags = l.flags.with(LaneFlags::SOLO_PATH, m).with(LaneFlags::SOLO_CARRY, c);
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
            .with(LaneFlags::SOLO_BELOW, false)
            .with(LaneFlags::SOLO_PATH, false)
            .with(LaneFlags::SOLO_CARRY, false);
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
    // A lane LEFT OUT of an export is silent before any curve or button is
    // asked, and its children with it: see `LaneFlags::LEFT_OUT`.
    let muted = |r: usize| {
        lanes[r].flags.contains(LaneFlags::LEFT_OUT)
            || auto_mute
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
        // Left out of an export beats even a solo naming the lane.
        out[r] = if out[r] || flags.contains(LaneFlags::LEFT_OUT) {
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
        } else if flags.contains(LaneFlags::BUS)
            || flags.contains(LaneFlags::SOLO_PATH)
            || flags.contains(LaneFlags::SOLO_CARRY)
        {
            // **A bus is never silenced by another strip's solo** (design
            // 9, owner call 4): it is a return, and a return is exempt, as
            // Return 0 always was. And what is on a bus's path is heard on
            // the same terms: a soloed bus hears its feeders, whole, and a
            // heard bus or feeder reaches the master through the folders
            // carrying it (whose own rows stay silent: `row_quiet`). Its own
            // mute still rules, as the group's above does.
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
    let flags = plan.lanes.get(rank).map_or(LaneFlags::default(), |l| l.flags);
    if flags.contains(LaneFlags::LEFT_OUT) || flags.contains(LaneFlags::ROW_LEFT_OUT) {
        return false;
    }
    let soloed = flags.contains(LaneFlags::SOLOED);
    // A bus, and a soloed bus's feeder, is no SOURCE to a solo: see
    // `resolve_heard`. A folder that only CARRIES is one: this question is
    // asked of what stands on its own row, which the carry silences. A desk
    // with no bus has none of the three.
    let exempt = flags.contains(LaneFlags::BUS) || flags.contains(LaneFlags::SOLO_PATH);
    strip_heard(muted, soloed, plan.any_solo, !exempt)
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
    // Transcribed from `instrument.rs` at d821bb3 with the desk width made a
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
        let forged = LaneFlags::SOLO_ABOVE | LaneFlags::SOLO_BELOW | LaneFlags::SOLO_PATH | LaneFlags::SOLO_CARRY;
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
        assert!(plan.post.row_quiet.is_empty(), "a forged carry silenced a row");
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

    /// A random desk with buses: the tree, the routes, which rows are buses,
    /// and sends named at any lane as a careless host would name them (only
    /// those to a bus that is not their own source are sends). `buses`
    /// false makes a desk as every desk was before W6.
    struct BusDesk {
        n: usize,
        parents: Vec<i32>,
        raw: Vec<(usize, usize, bool, f32)>,
        buses: Vec<bool>,
        sends: Vec<(usize, usize, bool)>,
    }

    fn bus_desk(rng: &mut Rng, n: usize, buses: bool) -> (BusDesk, PlanInputs) {
        let shuffle = rng.chance(0.5);
        let (parents, slots) = random_tree(rng, n, shuffle);
        let raw = random_routes(rng, n);
        let bus: Vec<bool> = (0..n).map(|_| buses && rng.chance(0.25)).collect();
        let sends: Vec<(usize, usize, bool)> = (0..rng.below(8))
            .map(|_| (rng.below(n), rng.below(n), rng.chance(0.3)))
            .collect();
        let mut inputs = route_inputs(n, &slots, &parents, &raw);
        for (l, b) in inputs.lanes.iter_mut().zip(&bus) {
            l.flags = l.flags.with(LaneFlags::BUS, *b);
        }
        inputs.sends = sends
            .iter()
            .map(|&(f, t, pre)| InSend {
                from: SendFrom::Lane(Slot(slots[f])),
                to: SendTo::Lane(Slot(slots[t])),
                level: 0.5,
                pre,
            })
            .collect();
        inputs.sends.push(InSend {
            from: SendFrom::Drums,
            to: SendTo::Fx,
            level: 0.5,
            pre: false,
        });
        let d = BusDesk { n, parents, raw, buses: bus, sends };
        (d, inputs)
    }

    /// **The post-fork graph, worked out the slow way**: which folders fold
    /// early, which ranks are route-phase nodes (a bus, a folder with a bus
    /// under it, a lane something is routed into), and per node the nodes
    /// it waits for.
    struct RefGraph {
        early: Vec<bool>,
        late: Vec<bool>,
        receives: Vec<bool>,
        nodes: Vec<bool>,
        feeders: Vec<Vec<usize>>,
    }

    fn reference_graph(d: &BusDesk) -> RefGraph {
        let n = d.n;
        let is_folder: Vec<bool> = (0..n).map(|f| d.parents.contains(&(f as i32))).collect();
        let mut below = d.buses.clone();
        for _ in 0..n {
            for c in 0..n {
                if below[c] && d.parents[c] >= 0 {
                    below[d.parents[c] as usize] = true;
                }
            }
        }
        let late: Vec<bool> = (0..n).map(|f| is_folder[f] && below[f] && !d.buses[f]).collect();
        let early: Vec<bool> = (0..n).map(|f| is_folder[f] && !below[f]).collect();
        let live: Vec<(usize, usize)> = frozen_latch(n, &d.raw)
            .into_iter()
            .filter(|&(_, _, h, g)| !h && g > 0.0)
            .map(|(f, t, _, _)| (f, t))
            .collect();
        let receives: Vec<bool> = (0..n).map(|t| live.iter().any(|e| e.1 == t)).collect();
        let nodes: Vec<bool> = (0..n).map(|r| d.buses[r] || late[r] || receives[r]).collect();
        let mut feeders: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &(f, t) in &live {
            if nodes[f] {
                feeders[t].push(f);
            }
        }
        for &(f, t, _) in &d.sends {
            if d.buses[t] && f != t && nodes[f] {
                feeders[t].push(f);
            }
        }
        for c in 0..n {
            if d.parents[c] >= 0 {
                let p = d.parents[c] as usize;
                if (d.buses[p] || late[p]) && nodes[c] {
                    feeders[p].push(c);
                }
            }
        }
        RefGraph { early, late, receives, nodes, feeders }
    }

    /// **Today's sweep, over an explicit node set**: `frozen_kahn`'s walk,
    /// pass after pass, ascending, a node placed the moment every node it
    /// waits for is, with the node set given rather than read off the
    /// routes. On a desk with no bus the two walks are one walk.
    fn frozen_kahn_nodes(n: usize, nodes: &[bool], feeders: &[Vec<usize>]) -> Vec<usize> {
        let mut placed = vec![false; n];
        let mut order = Vec::new();
        for _ in 0..n {
            let mut moved = false;
            for dest in 0..n {
                if !nodes[dest] || placed[dest] {
                    continue;
                }
                if feeders[dest].iter().all(|f| placed[*f]) {
                    order.push(dest);
                    placed[dest] = true;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        for dest in 0..n {
            if nodes[dest] && !placed[dest] {
                order.push(dest);
            }
        }
        order
    }

    /// **The levels are a topological layering**: the early fold first
    /// (deepest folders first, descending rank within a layer, as
    /// `fold_folders` walks), then the route phase by longest feeder chain
    /// (Kahn order within a layer), then every cycle member and what hangs
    /// off one, ascending, in one final level. Checked against a naive
    /// fixpoint layering on random desks with folders, routes, buses, sends
    /// into lanes (kept only into a bus) and folders holding buses (which
    /// fold late); and on the desks without a bus, against today's walk over
    /// the routes alone, so no existing desk's schedule moved.
    #[test]
    fn the_levels_are_a_reference_topological_layering() {
        let mut rng = Rng(0x1e7e_1500_0000_0007);
        for trial in 0..5_000 {
            let n = 1 + rng.below(32);
            let with_buses = trial % 2 == 1;
            let (d, inputs) = bus_desk(&mut rng, n, with_buses);
            let plan = build_plan(&inputs, None);
            let g = reference_graph(&d);
            let parents = &d.parents;

            // The roles.
            for r in 0..n {
                let want = if d.buses[r] {
                    Some(NodeKind::Bus)
                } else if g.late[r] {
                    Some(NodeKind::LateFolder)
                } else if g.receives[r] {
                    Some(NodeKind::Dest)
                } else {
                    None
                };
                assert_eq!(plan.post.route[r], want, "trial {trial}: rank {r}'s route role");
                assert_eq!(plan.post.fold.contains(r), g.early[r], "trial {trial}: rank {r} folds early");
            }

            // The fold: depth by fixpoint, over the early folders.
            let mut depth = vec![0u32; n];
            for _ in 0..n {
                for c in 0..n {
                    if g.early[c] && parents[c] >= 0 {
                        let p = parents[c] as usize;
                        depth[p] = depth[p].max(depth[c] + 1);
                    }
                }
            }
            let mut want: Vec<Vec<u32>> = Vec::new();
            let fold_depth = (0..n)
                .filter(|f| g.early[*f])
                .map(|f| depth[f] + 1)
                .max()
                .unwrap_or(0);
            for k in 0..fold_depth {
                want.push(
                    (0..n as u32)
                        .rev()
                        .filter(|f| g.early[*f as usize] && depth[*f as usize] == k)
                        .collect(),
                );
            }
            assert_eq!(
                plan.routes.first_level as usize,
                want.len(),
                "trial {trial}"
            );

            // The route phase: the nodes, through the frozen sweep for the
            // order and a fixpoint for the layers.
            let order = frozen_kahn_nodes(n, &g.nodes, &g.feeders);
            if !with_buses {
                let routes: Vec<(usize, usize, bool, f32)> = frozen_latch(n, &d.raw)
                    .into_iter()
                    .filter(|&(_, _, h, gain)| !h && gain > 0.0)
                    .collect();
                assert_eq!(order, frozen_kahn(n, &routes), "trial {trial}: a desk with no bus left today's walk");
            }
            let mut level: Vec<Option<u32>> = vec![None; n];
            for _ in 0..=n {
                for dest in 0..n {
                    if !g.nodes[dest] || level[dest].is_some() {
                        continue;
                    }
                    if g.feeders[dest].iter().all(|f| level[*f].is_some()) {
                        level[dest] = Some(
                            g.feeders[dest]
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
                        .filter(|dest| level[**dest] == Some(k))
                        .map(|dest| *dest as u32)
                        .collect(),
                );
            }
            let cycle: Vec<u32> = (0..n)
                .filter(|dest| g.nodes[*dest] && level[*dest].is_none())
                .map(|dest| dest as u32)
                .collect();
            assert_eq!(plan.routes.cycle_level, !cycle.is_empty(), "trial {trial}");
            if !cycle.is_empty() {
                want.push(cycle);
            }
            let got: Vec<Vec<u32>> = plan.levels.0.iter().map(|l| l.to_vec()).collect();
            assert_eq!(
                got, want,
                "trial {trial}: parents {parents:?} buses {:?} feeders {:?}",
                d.buses, g.feeders
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
    /// `fold_folders`' descending walk over every early folder, and the
    /// sends pass's Kahn order over the route phase's nodes (the frozen
    /// sweep, with what is sent to a bus and what a late folder holds as
    /// more feeders); and each phase's publish order is a permutation of its
    /// own levels, so deferring the sums to the phase's end publishes
    /// everything once. On a desk with no bus the route publish is exactly
    /// `RouteTable::order`, today's.
    #[test]
    fn the_publish_order_is_todays_serial_walks() {
        let mut rng = Rng(0x9b11_5400_0000_0011);
        for trial in 0..5_000 {
            let n = 1 + rng.below(32);
            let with_buses = rng.chance(0.5);
            let (d, inputs) = bus_desk(&mut rng, n, with_buses);
            let plan = build_plan(&inputs, None);
            let g = reference_graph(&d);

            // `fold_folders`: `for ch in (0..CHANNELS).rev()`, skipping
            // whatever is not an early folder.
            let fold: Vec<u32> = (0..n as u32)
                .rev()
                .filter(|f| g.early[*f as usize])
                .collect();
            assert_eq!(plan.publish.fold.to_vec(), fold, "trial {trial}");

            if !with_buses {
                assert_eq!(
                    plan.publish.routes, plan.routes.order,
                    "trial {trial}: today's desk publishes in the sends pass's order"
                );
            }
            let kahn: Vec<u32> = frozen_kahn_nodes(n, &g.nodes, &g.feeders)
                .into_iter()
                .map(|x| x as u32)
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

    // ── buses, sends, the post-fork graph ───────────────────────────────

    fn bus(slot: u32, parent: Option<u32>) -> InLane {
        lane(slot, LaneFlags::BUS, parent)
    }

    fn send(from: u32, to: SendTo, pre: bool) -> InSend {
        InSend { from: SendFrom::Lane(Slot(from)), to, level: 0.5, pre }
    }

    /// **A send lands on a bus, and on nothing else**: a send to a lane that
    /// is not a bus, or to its own source, is left out as a route to a lane
    /// the plan does not play is; the rest are found by their two ends, in
    /// the host's order per return, and their sources are known.
    #[test]
    fn a_send_lands_only_on_a_bus_and_is_found_by_its_ends() {
        let plan = build_plan(
            &PlanInputs {
                lanes: vec![
                    lane(10, LaneFlags::default(), None),
                    lane(11, LaneFlags::default(), None),
                    bus(12, None),
                    bus(13, None),
                ],
                sends: vec![
                    send(10, SendTo::Lane(Slot(11)), false),
                    send(10, SendTo::Lane(Slot(12)), false),
                    send(12, SendTo::Lane(Slot(12)), false),
                    send(11, SendTo::Lane(Slot(12)), true),
                    send(12, SendTo::Lane(Slot(13)), false),
                    send(10, SendTo::Fx, true),
                    send(10, SendTo::Lane(Slot(12)), true),
                    InSend { from: SendFrom::Click, to: SendTo::Lane(Slot(13)), level: 0.25, pre: false },
                ],
                ..PlanInputs::default()
            },
            None,
        );
        let ends: Vec<(PlanSendFrom, PlanSendTo, bool)> =
            plan.sends.sends.iter().map(|s| (s.from, s.to, s.pre)).collect();
        assert_eq!(
            ends,
            vec![
                (PlanSendFrom::Lane(0), PlanSendTo::Lane(2), false),
                (PlanSendFrom::Lane(1), PlanSendTo::Lane(2), true),
                (PlanSendFrom::Lane(2), PlanSendTo::Lane(3), false),
                (PlanSendFrom::Lane(0), PlanSendTo::Fx, true),
                (PlanSendFrom::Lane(0), PlanSendTo::Lane(2), true),
                (PlanSendFrom::Click, PlanSendTo::Lane(3), false),
            ],
            "a send to a track, or to itself, was kept"
        );
        assert_eq!(plan.sends.into.row(0), &[3], "Return 0's sends");
        assert_eq!(plan.sends.into.row(2 + 1), &[0, 1, 4], "bus 12's, in the host's order");
        assert_eq!(plan.sends.into.row(3 + 1), &[2, 5], "bus 13's");
        let found: Vec<u32> = plan.sends.find(send_key(SendFrom::Lane(Slot(10)), SendTo::Lane(Slot(12)))).collect();
        assert_eq!(found, vec![0, 4], "both of 10's sends to 12, pre and post");
        assert_eq!(plan.sends.find(send_key(SendFrom::Lane(Slot(11)), SendTo::Lane(Slot(13)))).count(), 0);
        assert_eq!(plan.sends.find(send_key(SendFrom::Click, SendTo::Lane(Slot(13)))).collect::<Vec<_>>(), vec![5]);
        assert_eq!(plan.sends.auto.len(), 6);
        assert!(plan.sends.auto.iter().all(Option::is_none));
        let outs: Vec<usize> = plan.post.sends_out.iter().collect();
        assert_eq!(outs, vec![0, 1, 2]);
        let pre: Vec<usize> = plan.post.sends_pre.iter().collect();
        assert_eq!(pre, vec![0, 1]);
        assert!(plan.post.buses);
        // Bus 13 is fed by bus 12's send, so it runs after it.
        let route: Vec<u32> = plan.levels.0[plan.routes.first_level as usize..].iter().flat_map(|l| l.iter().copied()).collect();
        assert_eq!(route, vec![2, 3]);
        assert_eq!(&plan.publish.routes[..], &[2, 3]);
        assert_eq!(plan.post.route[2], Some(NodeKind::Bus));
        assert_eq!(plan.post.route[0], None, "a track that only sends is no node");
    }

    /// **A folder holding a bus folds after it**, in the route phase, and
    /// gathers its own row and its children; a folder holding none folds
    /// where it always did, and a bus heading a folder run gathers its
    /// children as a folder would, beside what is sent to it.
    #[test]
    fn a_folder_holding_a_bus_folds_after_the_bus() {
        // 0 folder { 1 track, 2 bus }; 3 folder { 4 track }; 5 bus { 6 };
        // 7 track sends to 2 and is routed into 5.
        let plan = build_plan(
            &PlanInputs {
                lanes: vec![
                    lane(0, LaneFlags::default(), None),
                    lane(1, LaneFlags::default(), Some(0)),
                    bus(2, Some(0)),
                    lane(3, LaneFlags::default(), None),
                    lane(4, LaneFlags::default(), Some(3)),
                    bus(5, None),
                    lane(6, LaneFlags::default(), Some(5)),
                    lane(7, LaneFlags::default(), None),
                ],
                routes: vec![InRoute { from: Slot(7), to: RouteEnd::Lane(Slot(5)), gain: 1.0 }],
                sends: vec![send(7, SendTo::Lane(Slot(2)), false)],
                ..PlanInputs::default()
            },
            None,
        );
        let fold: Vec<usize> = plan.post.fold.iter().collect();
        assert_eq!(fold, vec![3], "only the folder with no bus under it folds early");
        assert_eq!(plan.post.route[0], Some(NodeKind::LateFolder));
        assert_eq!(plan.post.route[2], Some(NodeKind::Bus));
        assert_eq!(plan.post.route[5], Some(NodeKind::Bus));
        assert_eq!(plan.post.route[3], None);
        assert_eq!(plan.routes.first_level, 1);
        assert_eq!(&plan.levels.0[0][..], &[3]);
        let route: Vec<Vec<u32>> = plan.levels.0[1..].iter().map(|l| l.to_vec()).collect();
        assert_eq!(route, vec![vec![2, 5], vec![0]], "the folder waits for its bus");
        assert_eq!(&plan.publish.routes[..], &[2, 5, 0]);
        assert_eq!(plan.post.kids.row(0), &[1, 2]);
        assert_eq!(plan.post.kids.row(5), &[6]);
        assert_eq!(plan.post.routes_in.row(5), &[0], "the route into bus 5, by edge");
        assert!(plan.routes.closed.contains(7), "a lane routed into a bus goes through it");
        assert!(plan.routes.routed.contains(7));
        // The compensation: 1 and 2 through folder 0; 7's own output through
        // bus 5, its send through bus 2; 6 through bus 5.
        assert_eq!(plan.post.pdc_main.row(1), &[0]);
        assert_eq!(plan.post.pdc_main.row(2), &[0]);
        assert_eq!(plan.post.pdc_main.row(7), &[5]);
        assert_eq!(plan.post.pdc_send.row(7), &[2]);
        assert_eq!(plan.post.pdc_main.row(6), &[5]);
        let at = |r: u32| plan.post.pdc_order.iter().position(|x| *x == r).expect("every rank");
        assert_eq!(plan.post.pdc_order.len(), 8);
        assert!(at(0) < at(2) && at(2) < at(7) && at(5) < at(7) && at(5) < at(6));
        // Out to the master: the late folder and the early one, held by
        // nothing; and bus 5, which heads a run of its own (so `closed`
        // holds it, as it holds every folder) and is routed nowhere. Bus 2
        // is folder 0's to carry.
        assert!(plan.routes.closed.contains(5), "a bus heading a run is a folder to `closed`");
        let direct: Vec<usize> = plan.post.direct.iter().collect();
        assert_eq!(direct, vec![0, 3, 5], "the nodes whose own output goes out");
    }

    /// **A folder routed into a bus is MOVED there; one routed into a
    /// legacy destination is copied, as it always was**: its own way out
    /// closes for the first and stays open for the second (`fold_folders`
    /// sent a group out whatever it was routed to, and that sound is kept).
    /// A bus routed to a lane goes out only through it, whatever the route's
    /// gain, as a track's route closes its way out; one routed to the jacks
    /// still goes out.
    #[test]
    fn a_folder_routed_into_a_bus_is_moved_and_one_routed_elsewhere_is_copied() {
        // 0 folder { 1 } routed into bus 6; 2 folder { 3 } routed into
        // track 7; 4 bus routed into track 7 at gain zero; 5 bus routed to
        // the jacks; 8 folder { 9 bus } routed into bus 6: a late folder.
        let plan = build_plan(
            &PlanInputs {
                lanes: vec![
                    lane(0, LaneFlags::default(), None),
                    lane(1, LaneFlags::default(), Some(0)),
                    lane(2, LaneFlags::default(), None),
                    lane(3, LaneFlags::default(), Some(2)),
                    bus(4, None),
                    bus(5, None),
                    bus(6, None),
                    lane(7, LaneFlags::default(), None),
                    lane(8, LaneFlags::default(), None),
                    bus(9, Some(8)),
                ],
                routes: vec![
                    InRoute { from: Slot(0), to: RouteEnd::Lane(Slot(6)), gain: 1.0 },
                    InRoute { from: Slot(2), to: RouteEnd::Lane(Slot(7)), gain: 1.0 },
                    InRoute { from: Slot(4), to: RouteEnd::Lane(Slot(7)), gain: 0.0 },
                    InRoute { from: Slot(5), to: RouteEnd::Hardware { first: 2, stereo: true }, gain: 1.0 },
                    InRoute { from: Slot(8), to: RouteEnd::Lane(Slot(6)), gain: 0.5 },
                ],
                ..PlanInputs::default()
            },
            None,
        );
        assert_eq!(plan.post.route[8], Some(NodeKind::LateFolder));
        let direct: Vec<usize> = plan.post.direct.iter().collect();
        assert_eq!(direct, vec![2, 5, 6], "who goes straight out");
        // Without a bus anywhere, the same folder routed into a track is
        // today's: out, and through the track.
        let legacy = build_plan(
            &PlanInputs {
                lanes: vec![
                    lane(0, LaneFlags::default(), None),
                    lane(1, LaneFlags::default(), Some(0)),
                    lane(2, LaneFlags::default(), None),
                ],
                routes: vec![InRoute { from: Slot(0), to: RouteEnd::Lane(Slot(2)), gain: 1.0 }],
                ..PlanInputs::default()
            },
            None,
        );
        assert_eq!(legacy.post.direct.iter().collect::<Vec<_>>(), vec![0]);
        assert!(legacy.post.carry.is_empty(), "a desk with no bus walked a carry");
    }

    /// **The bus-aware transfer follows every way out**: through a late
    /// folder, a bus heading a run, a folder routed into a bus, a bus's
    /// post- and pre-fader sends, and a legacy destination at its clip
    /// gain; and Return 0's share through each node's own send. Distinct
    /// gains per node, so a missing or doubled term shows.
    #[test]
    fn the_carry_follows_every_way_out_through_buses_and_folders() {
        // 0 late folder { 1 bus }; 2 track routed into bus 1 at 0.5; 3
        // early folder { 4 } routed into bus 5; 5 bus heading { 6 }, which
        // sends post-fader into bus 1 at 0.25 and pre-fader into bus 9 at
        // 0.5; 7 a legacy destination, loose, that 8 is routed into at 0.8;
        // 9 a loose bus.
        let mut sends = vec![
            InSend { from: SendFrom::Lane(Slot(5)), to: SendTo::Lane(Slot(1)), level: 0.25, pre: false },
            InSend { from: SendFrom::Lane(Slot(5)), to: SendTo::Lane(Slot(9)), level: 0.5, pre: true },
        ];
        sends.push(InSend { from: SendFrom::Lane(Slot(3)), to: SendTo::Fx, level: 0.75, pre: false });
        let plan = build_plan(
            &PlanInputs {
                lanes: vec![
                    lane(0, LaneFlags::default(), None),
                    bus(1, Some(0)),
                    lane(2, LaneFlags::default(), None),
                    lane(3, LaneFlags::default(), None),
                    lane(4, LaneFlags::default(), Some(3)),
                    bus(5, None),
                    lane(6, LaneFlags::default(), Some(5)),
                    lane(7, LaneFlags::default(), None),
                    lane(8, LaneFlags::default(), None),
                    bus(9, None),
                ],
                routes: vec![
                    InRoute { from: Slot(2), to: RouteEnd::Lane(Slot(1)), gain: 0.5 },
                    InRoute { from: Slot(3), to: RouteEnd::Lane(Slot(5)), gain: 1.0 },
                    InRoute { from: Slot(8), to: RouteEnd::Lane(Slot(7)), gain: 0.8 },
                ],
                sends,
                ..PlanInputs::default()
            },
            None,
        );
        // Each node's own law: gain 1 + rank / 10, centre at unity, gate
        // one, and folder 3's own send to Return 0 at its entry's level.
        let g = |r: usize| 1.0 + r as f32 / 10.0;
        let side = |r: usize, dest: bool| CarrySide {
            gain: if dest { 2.0 } else { g(r) },
            pan: (1.0, 1.0),
            gate: 1.0,
            fx_post: if r == 3 { 0.75 } else { 0.0 },
            fx_pre: 0.0,
        };
        let level = |via: CarryVia| match via {
            CarryVia::Parent => 1.0,
            CarryVia::Route(e) => plan.gain(plan.routes.edges[e as usize].gain),
            CarryVia::Post(i) | CarryVia::Pre(i) => plan.gain(plan.sends.sends[i as usize].gain),
        };
        let mut out = vec![[0.0f32; 4]; plan.lanes.len() * 2];
        plan.post.carry_into(side, level, &mut out);
        let fig = |r: u32, fold: bool| out[carry_at(r, fold)];
        let close = |a: f32, b: f32| (a - b).abs() <= 1e-6 * b.abs().max(1.0);
        // The late folder goes out; its bus goes out through it.
        let l0 = g(0);
        let b1 = g(1) * l0;
        let b9 = g(9);
        // Bus 5 goes out, sends a quarter of its output into bus 1 and half
        // of what reaches it, ahead of its fader, into bus 9.
        let b5 = g(5) * (1.0 + 0.25 * b1) + 0.5 * b9;
        // Folder 3 is moved into bus 5, and sends three quarters to Return 0.
        let f3 = g(3) * b5;
        let f3_fx = g(3) * 0.75;
        // The destination: its clip gain, and out.
        let d7 = 2.0;
        for (what, got, want) in [
            ("late folder 0", fig(0, false)[0], l0),
            ("bus 1", fig(1, false)[0], b1),
            ("bus 9", fig(9, false)[0], b9),
            ("bus 5", fig(5, false)[0], b5),
            ("folder 3", fig(3, true)[0], f3),
            ("folder 3's return", fig(3, true)[2], f3_fx),
            ("destination 7", fig(7, false)[0], d7),
        ] {
            assert!(close(got, want), "{what}: {got}, want {want}");
        }
        assert!(fig(3, false)[0] == 0.0 && fig(5, true)[0] == 0.0, "a figure no node has was written");
        assert_eq!(plan.post.carry.iter().filter(|s| s.fold).count(), 1, "one early folder, one fold figure");
    }

    /// **The carry walk is the recursion**, on random desks with buses and
    /// no cycle: every figure the steps finalise equals what the plain
    /// recursion over the plan's own tables (its folders, its live routes,
    /// its sends to buses) makes of random gains, so the walk's order puts
    /// every figure after what it reads. Desks the walk had to cut are
    /// skipped: a cut is a saved cycle's block of delay, not a figure.
    #[test]
    fn the_carry_walk_is_the_recursion_on_random_bus_desks() {
        let mut rng = Rng(0x0ca7_7700_0000_0013);
        let mut checked = 0;
        for trial in 0..2_000 {
            let n = 2 + rng.below(20);
            let (_, inputs) = bus_desk(&mut rng, n, true);
            let plan = build_plan(&inputs, None);
            if !plan.post.buses || plan.post.carry_terms.iter().any(|t| t.cut) {
                continue;
            }
            checked += 1;
            let gains: Vec<f32> = (0..n).map(|_| 0.25 + rng.unit()).collect();
            let gates: Vec<f32> = (0..n).map(|_| if rng.chance(0.5) { -1.0 } else { 1.0 }).collect();
            let side = |r: usize, dest: bool| CarrySide {
                gain: if dest { gains[r] * 0.5 } else { gains[r] },
                pan: (1.0, 0.5),
                gate: gates[r],
                fx_post: 0.0,
                fx_pre: 0.0,
            };
            let level = |via: CarryVia| match via {
                CarryVia::Parent => 1.0,
                CarryVia::Route(e) => plan.gain(plan.routes.edges[e as usize].gain),
                CarryVia::Post(i) | CarryVia::Pre(i) => plan.gain(plan.sends.sends[i as usize].gain),
            };
            let mut out = vec![[0.0f32; 4]; n * 2];
            plan.post.carry_into(side, level, &mut out);
            // The recursion, read off the plan's tables (remembered, or a
            // dense desk's paths would multiply).
            fn reference(
                plan: &DeskPlan,
                side: &dyn Fn(usize, bool) -> CarrySide,
                memo: &mut Vec<Option<[f32; 2]>>,
                r: usize,
                fold: bool,
            ) -> [f32; 2] {
                if let Some(v) = memo[carry_at(r as u32, fold)] {
                    return v;
                }
                let v = reference_once(plan, side, memo, r, fold);
                memo[carry_at(r as u32, fold)] = Some(v);
                v
            }
            fn reference_once(
                plan: &DeskPlan,
                side: &dyn Fn(usize, bool) -> CarrySide,
                memo: &mut Vec<Option<[f32; 2]>>,
                r: usize,
                fold: bool,
            ) -> [f32; 2] {
                let dest = !fold && plan.post.route[r] == Some(NodeKind::Dest);
                let s = side(r, dest);
                let direct = if dest { !plan.routes.closed.contains(r) } else { plan.post.direct.contains(r) };
                let mut inside = [f32::from(u8::from(direct)); 2];
                let mut ahead = [0.0f32; 2];
                let add = |into: &mut [f32; 2], g: f32, v: [f32; 2]| {
                    into[0] += g * v[0];
                    into[1] += g * v[1];
                };
                if !dest {
                    if let Some(p) = plan.lanes[r].parent {
                        let v = reference(plan, side, memo, p as usize, plan.post.fold.contains(p as usize));
                        add(&mut inside, 1.0, v);
                    }
                }
                for e in plan.routes.edges.iter() {
                    if let (RouteTo::Lane(t), true) = (e.to, e.live) {
                        if e.from as usize == r {
                            let v = reference(plan, side, memo, t as usize, false);
                            add(&mut inside, plan.gain(e.gain), v);
                        }
                    }
                }
                for snd in plan.sends.sends.iter() {
                    if let (PlanSendFrom::Lane(f), PlanSendTo::Lane(t)) = (snd.from, snd.to) {
                        if f as usize == r {
                            let v = reference(plan, side, memo, t as usize, false);
                            if snd.pre {
                                add(&mut ahead, plan.gain(snd.gain) * s.gate, v);
                            } else {
                                add(&mut inside, plan.gain(snd.gain), v);
                            }
                        }
                    }
                }
                [s.gain * s.pan.0 * inside[0] + ahead[0], s.gain * s.pan.1 * inside[1] + ahead[1]]
            }
            let mut memo = vec![None; n * 2];
            for step in plan.post.carry.iter() {
                let want = reference(&plan, &side, &mut memo, step.rank as usize, step.fold);
                let got = out[carry_at(step.rank, step.fold)];
                for k in 0..2 {
                    assert!(
                        (got[k] - want[k]).abs() <= 1e-4 * want[k].abs().max(1.0),
                        "trial {trial}: rank {} fold {} side {k}: {} against {}",
                        step.rank,
                        step.fold,
                        got[k],
                        want[k]
                    );
                }
            }
        }
        assert!(checked > 200, "only {checked} desks were checked");
    }

    /// **The route phase's gathers are the sends pass's, per destination**,
    /// on random desks of today's shape: `routes_in` for a destination is
    /// exactly the table-order list `RouteTable::inputs` holds for it, every
    /// node is a destination, and nothing is late, sent or on a bus's path.
    #[test]
    fn a_desk_with_no_bus_has_todays_destinations_and_nothing_new() {
        let mut rng = Rng(0x0dd5_0000_0000_0b05);
        for trial in 0..3_000 {
            let n = 1 + rng.below(32);
            let (d, inputs) = bus_desk(&mut rng, n, false);
            let _ = &d;
            let plan = build_plan(&inputs, None);
            for (pos, &dest) in plan.routes.order.iter().enumerate() {
                assert_eq!(plan.post.routes_in.row(dest as usize), plan.routes.inputs.row(pos), "trial {trial}");
                assert_eq!(plan.post.route[dest as usize], Some(NodeKind::Dest), "trial {trial}");
            }
            assert_eq!(
                plan.post.route.iter().filter(|r| r.is_some()).count(),
                plan.routes.order.len(),
                "trial {trial}: a node that is no destination"
            );
            for (r, l) in plan.lanes.iter().enumerate() {
                assert!(!l.flags.contains(LaneFlags::SOLO_PATH), "trial {trial}");
                let parent: Vec<u32> = l.parent.into_iter().collect();
                assert_eq!(plan.post.pdc_main.row(r), &parent[..], "trial {trial}: only the folder is paid for");
                assert!(plan.post.pdc_send.row(r).is_empty());
            }
            assert!(plan.post.sends_out.is_empty());
            assert!(!plan.post.buses, "trial {trial}");
        }
    }

    /// **The compensation order settles everything downstream first**: on
    /// random desks with buses, every rank comes after every rank it pays
    /// for (its folder, the buses and late folders it is routed into, the
    /// buses it sends to), except along a cycle, which is cut.
    #[test]
    fn the_compensation_order_puts_every_rank_after_what_it_pays_for() {
        let mut rng = Rng(0x0bd1_0000_0000_0042);
        for trial in 0..3_000 {
            let n = 1 + rng.below(24);
            let (_, inputs) = bus_desk(&mut rng, n, true);
            let plan = build_plan(&inputs, None);
            let order = &plan.post.pdc_order;
            let mut sorted = order.to_vec();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..n as u32).collect::<Vec<_>>(), "trial {trial}: not every rank once");
            let at = |r: u32| order.iter().position(|x| *x == r).expect("every rank");
            let next = |u: usize| -> Vec<u32> {
                plan.post.pdc_main.row(u).iter().chain(plan.post.pdc_send.row(u)).copied().collect()
            };
            // Whether `from` reaches `to` along the paid-for edges.
            let reaches = |from: usize, to: usize| {
                let mut seen = vec![false; n];
                let mut walk = vec![from];
                while let Some(x) = walk.pop() {
                    if x == to {
                        return true;
                    }
                    if std::mem::replace(&mut seen[x], true) {
                        continue;
                    }
                    walk.extend(next(x).into_iter().map(|v| v as usize));
                }
                false
            };
            for u in 0..n {
                for v in next(u) {
                    if !reaches(v as usize, u) {
                        assert!(at(v) < at(u as u32), "trial {trial}: {u} is settled before {v}, which it pays for");
                    }
                }
            }
        }
    }

    /// **A bus is never silenced by another strip's solo, and a soloed bus
    /// hears its feeders** (design 9, owner call 4): the bus's own mute
    /// still rules; a track sending or routed into a soloed bus is heard
    /// whole (its own way out too, as a solo-in-place desk plays the sources
    /// of a soloed return), and the SAME inside a folder, which carries it;
    /// a track feeding an unsoloed bus is not heard. A folder holding a bus,
    /// or a feeder, is heard so what it carries reaches the master, while
    /// its other children answer to the solo as before and its own row is
    /// silent (`row_quiet`) unless something of its own is soloed.
    #[test]
    fn a_bus_is_solo_safe_and_a_soloed_bus_hears_its_feeders() {
        // 0 routed into bus 2; 1 sends to bus 3, which is soloed; folder 4
        // holds bus 5 and track 6; 7 is loose; 8 feeds bus 3 through 9, a
        // track routed into 3 that 8 is routed into; folder 10 holds 11,
        // which sends to bus 3, and 12, which does not.
        let mut lanes = vec![
            lane(0, LaneFlags::default(), None),
            lane(1, LaneFlags::default(), None),
            bus(2, None),
            lane(3, LaneFlags::BUS | LaneFlags::SOLOED, None),
            lane(4, LaneFlags::default(), None),
            bus(5, Some(4)),
            lane(6, LaneFlags::default(), Some(4)),
            lane(7, LaneFlags::default(), None),
            lane(8, LaneFlags::default(), None),
            lane(9, LaneFlags::default(), None),
            lane(10, LaneFlags::default(), None),
            lane(11, LaneFlags::default(), Some(10)),
            lane(12, LaneFlags::default(), Some(10)),
        ];
        let inputs = |lanes: &[InLane]| PlanInputs {
            lanes: lanes.to_vec(),
            routes: vec![
                InRoute { from: Slot(0), to: RouteEnd::Lane(Slot(2)), gain: 1.0 },
                InRoute { from: Slot(8), to: RouteEnd::Lane(Slot(9)), gain: 1.0 },
                InRoute { from: Slot(9), to: RouteEnd::Lane(Slot(3)), gain: 1.0 },
            ],
            sends: vec![send(1, SendTo::Lane(Slot(3)), false), send(11, SendTo::Lane(Slot(3)), true)],
            ..PlanInputs::default()
        };
        let plan = build_plan(&inputs(&lanes), None);
        let mut out = vec![false; lanes.len()];
        resolve_heard(&plan, &[], &mut out);
        assert_eq!(
            out,
            vec![false, true, true, true, true, true, false, false, true, true, true, true, false],
            "who is heard with bus 3 soloed"
        );
        let carried: Vec<usize> = (0..lanes.len()).filter(|r| plan.lanes[*r].flags.contains(LaneFlags::SOLO_CARRY)).collect();
        assert_eq!(carried, vec![4, 10], "the folders carrying a bus and a feeder");
        let quiet: Vec<usize> = plan.post.row_quiet.iter().collect();
        assert_eq!(quiet, vec![4, 10], "a carrying folder's own row played through the solo");
        // A folder's own row is a SOURCE to the room's question: the carry
        // passes what reaches it, not what stands on it.
        let flat: Vec<bool> = (0..lanes.len()).map(|r| lane_heard_flat(&plan, r, false)).collect();
        assert_eq!(flat, vec![false, true, true, true, false, true, false, false, true, true, false, true, false]);
        // A bus muted is muted, solo or none.
        lanes[5].flags = lanes[5].flags | LaneFlags::MUTED;
        let plan = build_plan(&inputs(&lanes), None);
        resolve_heard(&plan, &[], &mut out);
        assert!(!out[5], "a muted bus was heard");
        // A TRACK soloed: every bus stays heard and so does what carries one;
        // the soloed bus's feeders no longer are, nor the folder that only
        // carried one of them, since nothing soloed is a bus.
        lanes[3].flags = LaneFlags::BUS;
        lanes[5].flags = LaneFlags::BUS;
        lanes[7].flags = LaneFlags::SOLOED;
        let plan = build_plan(&inputs(&lanes), None);
        resolve_heard(&plan, &[], &mut out);
        assert_eq!(out, vec![false, false, true, true, true, true, false, true, false, false, false, false, false]);
        assert_eq!(plan.post.row_quiet.iter().collect::<Vec<_>>(), vec![4], "folder 4 still only carries its bus");
        // A child of the carrying folder soloed: the folder is heard for its
        // own sake again (inside it something is soloed), row and all, as a
        // folder always was.
        lanes[7].flags = LaneFlags::default();
        lanes[6].flags = LaneFlags::SOLOED;
        let plan = build_plan(&inputs(&lanes), None);
        resolve_heard(&plan, &[], &mut out);
        assert!(out[4] && out[5] && out[6], "the soloed child, its folder and the bus beside it");
        assert!(plan.post.row_quiet.is_empty(), "a folder holding a soloed child lost its row");
        // And with no solo anywhere, every unmuted lane is heard and no row
        // is quiet: the derived bits change nothing then.
        lanes[6].flags = LaneFlags::default();
        let plan = build_plan(&inputs(&lanes), None);
        resolve_heard(&plan, &[], &mut out);
        assert!(out.iter().all(|h| *h));
        assert!(plan.post.row_quiet.is_empty());
    }

    /// **A lane LEFT OUT of an export is silent whatever its own switches
    /// say**, and so is what stands under it; nothing else moves. An export
    /// of selected tracks asks for this bit instead of writing the desk's
    /// mutes, so it has to beat the three things that would otherwise let a
    /// lane through: an unmuted button, a Mute curve holding it open, and a
    /// solo naming it.
    #[test]
    fn a_lane_left_out_of_an_export_is_silent_whatever_its_mute_curve_or_solo_say() {
        let mut lanes = vec![
            lane(0, LaneFlags::default(), None),
            lane(1, LaneFlags::LEFT_OUT, None),
            lane(2, LaneFlags::LEFT_OUT, None),
            lane(3, LaneFlags::default(), Some(2)),
        ];
        let inputs = |lanes: &[InLane]| PlanInputs { lanes: lanes.to_vec(), ..PlanInputs::default() };
        let plan = build_plan(&inputs(&lanes), None);
        let mut out = vec![false; lanes.len()];
        resolve_heard(&plan, &[], &mut out);
        assert_eq!(out, vec![true, false, false, false], "left out, or under a lane left out, and heard");
        // A Mute curve holding the lane OPEN does not bring it back.
        resolve_heard(&plan, &[None, Some(false), Some(false), None], &mut out);
        assert_eq!(out, vec![true, false, false, false], "a curve let a left-out lane through");
        assert!(!lane_heard_flat(&plan, 1, false), "the lane's own row was heard");
        assert!(lane_heard_flat(&plan, 0, false));
        // Nor does a solo on it.
        lanes[1].flags = LaneFlags::LEFT_OUT | LaneFlags::SOLOED;
        let plan = build_plan(&inputs(&lanes), None);
        resolve_heard(&plan, &[], &mut out);
        assert!(!out[1], "a solo let a left-out lane through");
        // A folder's OWN ROW left out: quiet, while it and its child are
        // heard and carry.
        lanes[1].flags = LaneFlags::default();
        lanes[2].flags = LaneFlags::ROW_LEFT_OUT;
        let plan = build_plan(&inputs(&lanes), None);
        resolve_heard(&plan, &[], &mut out);
        assert_eq!(out, vec![true, true, true, true], "a folder's own row took its group with it");
        assert_eq!(plan.post.row_quiet.iter().collect::<Vec<_>>(), vec![2], "the folder's own row is not quiet");
        assert!(!lane_heard_flat(&plan, 2, false), "the folder's own row was heard");
        // And the bit off, the desk is the desk again.
        for l in &mut lanes {
            l.flags = LaneFlags::default();
        }
        let plan = build_plan(&inputs(&lanes), None);
        resolve_heard(&plan, &[], &mut out);
        assert!(out.iter().all(|h| *h));
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
