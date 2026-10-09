//! **The lanes the callback owns, and the one door anything enters them by.**
//!
//! Today a lane is a column in some forty `[_; CHANNELS]` arrays on the
//! `Renderer`, so a desk's width is a compile-time number and adding a track
//! means a wider engine. A [`Spine`] is the other shape: a vector of boxed
//! lanes indexed by [`Slot`], bought at a capacity, plus the desk-wide
//! buffers every lane reads ([`DeskBuffers`]), which stay OUT of the lanes so
//! a keyed lane can read another lane's last-chunk stem without aliasing
//! anything (design section 7.3).
//!
//! **Everything reaches the spine as an [`EngineOp`]**, FIFO, on one ring, at
//! the top of the callback ([`apply_ops`]); everything it lets go of leaves on
//! a second ring, for the host to drop. The callback only ever moves
//! pointers. That is the whole real-time argument, and it has three rules:
//!
//! * **Retire room first.** An op is taken only when the retire ring has room
//!   for everything it displaces, and every op here displaces at most one
//!   entry, so "room" is one free slot. With none, the op WAITS a block; it is
//!   never taken and dropped, and nothing is ever freed here.
//! * **A plan is validated before it plays.** One that names a slot twice, an
//!   empty slot or a lane of another generation is refused whole, counted,
//!   and retired, and the previous plan keeps playing. O(N), using the
//!   spine's own `check` set, so validation allocates nothing either.
//! * **Everything slot-bound carries its generation.** An `Attach` for a lane
//!   that is not the one it was built for goes straight back.
//!
//! Generic over the lane `L` and the attachable part `P`, so the protocol is
//! proven here, in W1, against a toy lane, and W3 drops the real `Lane` in.

use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::{Consumer, Producer};

use crate::desk_plan::{DeskPlan, PickId, Slot};
use crate::laneset::LaneSet;

/// The longest chunk the renderer ever runs, in frames: the engine's own
/// `MAX_BLOCK`, which the desk buffers are sized by. Stated here rather than
/// imported because the engine's is private to it; the test below pins the
/// two together.
pub const MAX_BLOCK: usize = 512;

/// What the spine needs to know about a lane.
pub trait SpineLane {
    /// The generation it was built for. Every plan entry, schedule entry and
    /// attachment names one, and a mismatch is a lane that is not theirs.
    fn gen(&self) -> u32;
    /// The claim stamp: the round that last took this lane. The claiming hand
    /// swaps it in; finding its own round there already means another hand
    /// holds the lane, which is the take-once guarantee `LaneCells::take`
    /// gave (design 7.6).
    fn claim_word(&self) -> &AtomicU64;
}

/// A lane that can take a part after it was built: pads, a kit, a chain.
pub trait Attach<P> {
    /// Seat `part`, handing back whatever it displaced. Called on the audio
    /// thread, so it may only move: the displaced part goes home through the
    /// retire ring.
    fn attach(&mut self, part: P) -> Option<P>;
}

/// The buffers every lane reads and no lane owns.
///
/// Each is `MAX_BLOCK x capacity x 2` floats, frame-major (`f * capacity * 2
/// + slot * 2 + side`, today's `stem_scratch` layout with the capacity in
/// place of `CHANNELS`), written only in the serial passes and borrowed only
/// as `&` by the fork.
#[derive(Debug, Default)]
pub struct DeskBuffers {
    pub stem_scratch: Box<[f32]>,
    pub dry_scratch: Box<[f32]>,
    /// Last chunk's stems, for the sidechain keys: one chunk of key latency.
    pub key_prev: Box<[f32]>,
    /// How many frames of `key_prev` are this chunk's.
    pub key_prev_frames: usize,
    capacity: usize,
}

impl DeskBuffers {
    pub fn with_capacity(capacity: usize) -> Self {
        let len = MAX_BLOCK * capacity * 2;
        Self {
            stem_scratch: vec![0.0; len].into_boxed_slice(),
            dry_scratch: vec![0.0; len].into_boxed_slice(),
            key_prev: vec![0.0; len].into_boxed_slice(),
            key_prev_frames: 0,
            capacity,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Carry the key snapshot into `to`, re-strided for its capacity, so a
    /// Grow keeps key latency at one chunk rather than dropping a block of
    /// keys. Allocates nothing: both sides are already sized.
    fn carry_keys_into(&self, to: &mut Self) {
        let frames = self.key_prev_frames.min(MAX_BLOCK);
        let (from_w, to_w) = (self.capacity * 2, to.capacity * 2);
        let w = from_w.min(to_w);
        for f in 0..frames {
            let (Some(src), Some(dst)) = (
                self.key_prev.get(f * from_w..f * from_w + w),
                to.key_prev.get_mut(f * to_w..f * to_w + w),
            ) else {
                break;
            };
            dst.copy_from_slice(src);
        }
        to.key_prev_frames = frames;
    }
}

/// One input binding the monitor plays: which pick, onto which lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorPick {
    pub pick: PickId,
    pub slot: Slot,
    pub gen: u32,
    pub width: u8,
}

/// The live input's ring AND its layout, which arrive together. Today they
/// arrive through two mutexes and the renderer assigns the ring in place,
/// which frees the old `Consumer` on the audio thread; here the old plan
/// leaves whole on the retire ring.
#[derive(Debug, Default)]
pub struct MonitorPlan {
    pub ring: Option<Consumer<f32>>,
    pub channels: u16,
    pub block: u32,
    pub picks: Box<[MonitorPick]>,
}

/// The callback's lanes and the desk-wide state they share.
#[derive(Debug)]
pub struct Spine<L> {
    pub lanes: Vec<Option<Box<L>>>,
    pub gens: Vec<u32>,
    pub desk: DeskBuffers,
    /// Scratch for plan validation, one bit per slot, always left empty.
    pub check: LaneSet,
    /// Per-chunk rank scratch, capacity long, for the walks W3 moves here.
    pub rank_scratch: Vec<u32>,
    pub monitor: Option<Box<MonitorPlan>>,
    /// One capture ring per pick, by `PickId`.
    pub captures: Vec<Option<Producer<f32>>>,
    /// Picks whose capture is running: their ring may not be swapped.
    pub capturing: LaneSet,
}

/// A spine the host built larger, every buffer allocated, for a Grow.
pub type SpineShell<L> = Spine<L>;

impl<L> Spine<L> {
    /// A spine with room for `lanes` lanes and `picks` capture picks, all of
    /// it allocated here, on the host, so the callback never has to.
    pub fn with_capacity(lanes: usize, picks: usize) -> Self {
        Self {
            lanes: (0..lanes).map(|_| None).collect(),
            gens: vec![0; lanes],
            desk: DeskBuffers::with_capacity(lanes),
            check: LaneSet::with_capacity(lanes),
            rank_scratch: vec![0; lanes],
            monitor: None,
            captures: (0..picks).map(|_| None).collect(),
            capturing: LaneSet::with_capacity(picks),
        }
    }

    pub fn capacity(&self) -> usize {
        self.lanes.len()
    }
}

impl<L: SpineLane> Spine<L> {
    /// [`claim_lane`] over this spine's lanes.
    pub fn claim(&self, slot: Slot, gen: u32, round: u64) -> Option<&L> {
        claim_lane(&self.lanes, slot, gen, round)
    }
}

/// **Take a lane for this round, once.** `None` for an empty slot, a lane of
/// another generation, or a lane some hand already took this round. `round`
/// must be non-zero and new per fork: a lane is born with 0.
///
/// Over the lane slice rather than the spine, because the slice is what the
/// hands share: the spine also holds the capture rings, which are one
/// thread's alone.
pub fn claim_lane<L: SpineLane>(
    lanes: &[Option<Box<L>>],
    slot: Slot,
    gen: u32,
    round: u64,
) -> Option<&L> {
    let lane = lanes.get(slot.0 as usize)?.as_deref()?;
    if lane.gen() != gen {
        return None;
    }
    (lane.claim_word().swap(round, Ordering::AcqRel) != round).then_some(lane)
}

/// Everything that changes the spine, in one FIFO.
pub enum EngineOp<L, P> {
    Grow(Box<SpineShell<L>>),
    /// **`Option<Box<L>>`, not the contract's `Box<L>`**: the boxes move into
    /// the spine one by one and the emptied list goes home on the retire
    /// ring, which needs somewhere to leave a hole. The host always fills
    /// every entry with `Some`.
    Install(Box<[(Slot, Option<Box<L>>)]>),
    /// `out` is sized by the host, one entry per slot, and comes home holding
    /// the lanes.
    Remove {
        slots: Box<[Slot]>,
        out: Box<[Option<Box<L>>]>,
    },
    Attach {
        slot: Slot,
        gen: u32,
        part: P,
    },
    Plan(Box<DeskPlan>),
    Monitor(Box<MonitorPlan>),
    Capture {
        pick: PickId,
        ring: Option<Producer<f32>>,
    },
}

impl<L, P> std::fmt::Debug for EngineOp<L, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Grow(s) => write!(f, "Grow({})", s.capacity()),
            Self::Install(l) => write!(f, "Install({} lanes)", l.len()),
            Self::Remove { slots, .. } => write!(f, "Remove({slots:?})"),
            Self::Attach { slot, gen, .. } => write!(f, "Attach({slot:?}, gen {gen})"),
            Self::Plan(p) => write!(f, "Plan(gen {})", p.gen),
            Self::Monitor(m) => write!(f, "Monitor({} picks)", m.picks.len()),
            Self::Capture { pick, ring } => write!(f, "Capture({pick:?}, {})", ring.is_some()),
        }
    }
}

/// What one [`apply_ops`] call did. Every refusal is counted: tests assert
/// they stay zero, and the host shows the stall line for a plan refusal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub applied: u32,
    pub plan_refused: u32,
    pub install_refused: u32,
    pub attach_retired: u32,
    pub grow_refused: u32,
    pub capture_refused: u32,
    /// Remove named a slot that held no lane.
    pub remove_empty: u32,
    /// Ops are still queued because the retire ring had no room.
    pub waiting: bool,
}

/// **Apply every queued op, in order, until the queue is empty or the retire
/// ring has no room.** The top of the callback, before the first chunk.
///
/// Allocates and frees nothing: every value either moves into the spine or
/// leaves on `retire`, and the host drops what comes back.
pub fn apply_ops<L, P>(
    spine: &mut Spine<L>,
    plan: &mut Box<DeskPlan>,
    ops: &mut Consumer<EngineOp<L, P>>,
    retire: &mut Producer<EngineOp<L, P>>,
) -> ApplyReport
where
    L: SpineLane + Attach<P>,
{
    let mut report = ApplyReport::default();
    loop {
        if ops.is_empty() {
            break;
        }
        // **Room first.** Every op below sends at most one value home, so one
        // free slot is all an op can need. Checked BEFORE the pop: an op that
        // cannot be finished is never started.
        if retire.is_full() {
            report.waiting = true;
            break;
        }
        let Ok(op) = ops.pop() else { break };
        let home = apply_one(spine, plan, op, &mut report);
        report.applied += 1;
        if let Some(home) = home {
            // Cannot fail: the room was there before the pop and this side
            // is the ring's only producer, so it can only have grown. Were it
            // ever to fail, leaking is the lesser fault than freeing a lane
            // on the audio thread.
            if let Err(rtrb::PushError::Full(lost)) = retire.push(home) {
                std::mem::forget(lost);
            }
        }
    }
    report
}

/// One op. Returns what goes home.
fn apply_one<L, P>(
    spine: &mut Spine<L>,
    plan: &mut Box<DeskPlan>,
    op: EngineOp<L, P>,
    report: &mut ApplyReport,
) -> Option<EngineOp<L, P>>
where
    L: SpineLane + Attach<P>,
{
    match op {
        EngineOp::Grow(mut shell) => {
            let fits = shell.capacity() >= spine.capacity()
                && shell.captures.len() >= spine.captures.len()
                && shell.desk.capacity() >= spine.desk.capacity();
            if !fits {
                report.grow_refused += 1;
                return Some(EngineOp::Grow(shell));
            }
            for i in 0..spine.lanes.len() {
                std::mem::swap(&mut spine.lanes[i], &mut shell.lanes[i]);
                shell.gens[i] = spine.gens[i];
            }
            spine.desk.carry_keys_into(&mut shell.desk);
            std::mem::swap(&mut spine.monitor, &mut shell.monitor);
            for i in 0..spine.captures.len() {
                std::mem::swap(&mut spine.captures[i], &mut shell.captures[i]);
            }
            for p in spine.capturing.iter() {
                shell.capturing.insert(p);
            }
            // The shell becomes the spine, and the old spine, emptied of
            // lanes, goes home in the shell's own box.
            std::mem::swap(spine, &mut *shell);
            Some(EngineOp::Grow(shell))
        }
        EngineOp::Install(mut list) => {
            if !install_fits(spine, &list) {
                report.install_refused += 1;
                return Some(EngineOp::Install(list));
            }
            for (slot, lane) in list.iter_mut() {
                let at = slot.0 as usize;
                if let Some(lane) = lane.take() {
                    spine.gens[at] = lane.gen();
                    spine.lanes[at] = Some(lane);
                }
            }
            Some(EngineOp::Install(list))
        }
        EngineOp::Remove { slots, mut out } => {
            for (i, slot) in slots.iter().enumerate() {
                let taken = spine.lanes.get_mut(slot.0 as usize).and_then(Option::take);
                if taken.is_none() {
                    report.remove_empty += 1;
                }
                if let Some(cell) = out.get_mut(i) {
                    *cell = taken;
                } else if let Some(lane) = taken {
                    // A host that sized `out` short: the lane goes back where
                    // it was rather than being dropped here.
                    spine.lanes[slot.0 as usize] = Some(lane);
                }
            }
            Some(EngineOp::Remove { slots, out })
        }
        EngineOp::Attach { slot, gen, part } => {
            let at = slot.0 as usize;
            let ours = spine.gens.get(at) == Some(&gen);
            match spine.lanes.get_mut(at).and_then(Option::as_mut) {
                Some(lane) if ours && lane.gen() == gen => {
                    lane.attach(part).map(|old| EngineOp::Attach {
                        slot,
                        gen,
                        part: old,
                    })
                }
                _ => {
                    report.attach_retired += 1;
                    Some(EngineOp::Attach { slot, gen, part })
                }
            }
        }
        EngineOp::Plan(mut next) => {
            if !plan_is_valid(spine, &next) {
                report.plan_refused += 1;
                return Some(EngineOp::Plan(next));
            }
            std::mem::swap(plan, &mut next);
            Some(EngineOp::Plan(next))
        }
        EngineOp::Monitor(next) => spine.monitor.replace(next).map(EngineOp::Monitor),
        EngineOp::Capture { pick, ring } => {
            let at = pick.0 as usize;
            if at >= spine.captures.len() || spine.capturing.contains(at) {
                report.capture_refused += 1;
                return Some(EngineOp::Capture { pick, ring });
            }
            std::mem::replace(&mut spine.captures[at], ring).map(|old| EngineOp::Capture {
                pick,
                ring: Some(old),
            })
        }
    }
}

/// Every target in range, empty, named once, and carrying a lane.
fn install_fits<L>(spine: &mut Spine<L>, list: &[(Slot, Option<Box<L>>)]) -> bool {
    let mut ok = true;
    let mut marked = 0;
    for (slot, lane) in list {
        let at = slot.0 as usize;
        let free = spine.lanes.get(at).is_some_and(Option::is_none);
        if !free || lane.is_none() || spine.check.contains(at) {
            ok = false;
            break;
        }
        spine.check.insert(at);
        marked += 1;
    }
    for (slot, _) in list.iter().take(marked) {
        spine.check.remove(slot.0 as usize);
    }
    ok
}

/// **The take-once guarantee, at the door.** A plan that repeats a slot,
/// names an empty one, or carries a generation the slot does not hold is
/// refused. O(N) over the plan's lanes; `check` is left empty either way.
fn plan_is_valid<L: SpineLane>(spine: &mut Spine<L>, plan: &DeskPlan) -> bool {
    let mut ok = true;
    let mut marked = 0;
    for lane in plan.lanes.iter() {
        let at = lane.slot.0 as usize;
        let held = spine
            .lanes
            .get(at)
            .and_then(Option::as_deref)
            .is_some_and(|l| l.gen() == lane.gen);
        if !held || spine.gens.get(at) != Some(&lane.gen) || spine.check.contains(at) {
            ok = false;
            break;
        }
        spine.check.insert(at);
        marked += 1;
    }
    for lane in plan.lanes.iter().take(marked) {
        spine.check.remove(lane.slot.0 as usize);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk_plan::{build_plan, InLane, LaneFlags, PlanInputs};
    use rtrb::RingBuffer;
    use tangent_ui::recorder::MidiRoute;

    /// A lane with nothing in it but what the protocol needs, and one part.
    #[derive(Debug)]
    struct Toy {
        gen: u32,
        claim: AtomicU64,
        part: Option<Box<u32>>,
    }

    impl SpineLane for Toy {
        fn gen(&self) -> u32 {
            self.gen
        }
        fn claim_word(&self) -> &AtomicU64 {
            &self.claim
        }
    }

    impl Attach<Box<u32>> for Toy {
        fn attach(&mut self, part: Box<u32>) -> Option<Box<u32>> {
            self.part.replace(part)
        }
    }

    type Op = EngineOp<Toy, Box<u32>>;

    fn toy(gen: u32) -> Box<Toy> {
        Box::new(Toy {
            gen,
            claim: AtomicU64::new(0),
            part: None,
        })
    }

    fn rings(n: usize) -> (Producer<Op>, Consumer<Op>, Producer<Op>, Consumer<Op>) {
        let (tx, rx) = RingBuffer::<Op>::new(n);
        let (rtx, rrx) = RingBuffer::<Op>::new(n);
        (tx, rx, rtx, rrx)
    }

    fn install(slots: impl Iterator<Item = (u32, u32)>) -> Op {
        EngineOp::Install(
            slots
                .map(|(s, g)| (Slot(s), Some(toy(g))))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    }

    fn plan_for(slots: &[(u32, u32)]) -> Box<DeskPlan> {
        Box::new(build_plan(
            &PlanInputs {
                lanes: slots
                    .iter()
                    .map(|&(s, g)| InLane {
                        slot: Slot(s),
                        gen: g,
                        flags: LaneFlags::default(),
                        parent: None,
                        punch: None,
                        midi: MidiRoute::EVERYTHING,
                        pick: None,
                    })
                    .collect(),
                ..PlanInputs::default()
            },
            None,
        ))
    }

    fn drain(rx: &mut Consumer<Op>) -> Vec<Op> {
        std::iter::from_fn(|| rx.pop().ok()).collect()
    }

    /// The desk buffers are sized by the engine's own chunk ceiling, which is
    /// private to it; this is the one place the two numbers meet.
    #[test]
    fn the_desk_buffers_are_sized_by_the_engines_block() {
        let src = include_str!("instrument.rs");
        assert!(
            src.contains(&format!("const MAX_BLOCK: i32 = {MAX_BLOCK};")),
            "instrument.rs's MAX_BLOCK moved; DeskBuffers must follow it"
        );
    }

    /// **The whole protocol allocates and frees nothing on the callback**,
    /// at every capacity the design names: Install, Attach (seating and
    /// displacing), Plan, Monitor (arriving and displacing), Capture
    /// (arriving and displacing), Remove and Grow, all in one apply. The
    /// rings are built and drained outside the count, as the host does.
    #[test]
    fn applying_every_op_allocates_and_frees_nothing() {
        for cap in [33usize, 65, 200, 1000] {
            let mut spine: Spine<Toy> = Spine::with_capacity(cap, 4);
            let mut plan = Box::new(DeskPlan::empty());
            let (mut tx, mut rx, mut rtx, mut rrx) = rings(64);
            let all: Vec<(u32, u32)> = (0..cap as u32).map(|s| (s, 7)).collect();
            tx.push(install(all.iter().copied())).unwrap();
            tx.push(EngineOp::Attach {
                slot: Slot(3),
                gen: 7,
                part: Box::new(1),
            })
            .unwrap();
            tx.push(EngineOp::Attach {
                slot: Slot(3),
                gen: 7,
                part: Box::new(2),
            })
            .unwrap();
            tx.push(EngineOp::Plan(plan_for(&all))).unwrap();
            tx.push(EngineOp::Plan(plan_for(&all[1..]))).unwrap();
            for _ in 0..2 {
                let (_, mon) = RingBuffer::<f32>::new(16);
                tx.push(EngineOp::Monitor(Box::new(MonitorPlan {
                    ring: Some(mon),
                    channels: 2,
                    block: 128,
                    picks: vec![MonitorPick {
                        pick: PickId(0),
                        slot: Slot(1),
                        gen: 7,
                        width: 2,
                    }]
                    .into_boxed_slice(),
                })))
                .unwrap();
                let (cap_tx, _) = RingBuffer::<f32>::new(16);
                tx.push(EngineOp::Capture {
                    pick: PickId(1),
                    ring: Some(cap_tx),
                })
                .unwrap();
            }
            tx.push(EngineOp::Remove {
                slots: vec![Slot(0)].into_boxed_slice(),
                out: vec![None].into_boxed_slice(),
            })
            .unwrap();
            tx.push(EngineOp::Grow(Box::new(Spine::with_capacity(cap * 2, 4))))
                .unwrap();
            let (report, counted) = crate::rt_alloc::count_allocs(|| {
                apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx)
            });
            assert_eq!(
                counted.total(),
                0,
                "capacity {cap}: {counted:?} for {report:?}"
            );
            assert_eq!(report.applied, 11);
            assert_eq!(
                report,
                ApplyReport {
                    applied: 11,
                    ..ApplyReport::default()
                },
                "capacity {cap}: something was refused"
            );
            assert_eq!(spine.capacity(), cap * 2, "the grow did not land");
            assert!(spine.lanes[0].is_none() && spine.lanes[1].is_some());
            assert_eq!(spine.lanes[3].as_ref().unwrap().part.as_deref(), Some(&2));
            assert_eq!(
                plan.lanes.len(),
                cap - 1,
                "the second plan is the one playing"
            );
            let home = drain(&mut rrx);
            // Install's empty list, the displaced part, the first empty plan,
            // the first real plan, the first monitor, the first capture ring,
            // the removed lane and the old spine.
            assert_eq!(home.len(), 8, "{home:?}");
        }
    }

    /// **No room to retire, no op taken.** A full retire ring leaves the
    /// queue standing, every op still in it, and the next apply after the
    /// host drains finishes them.
    #[test]
    fn a_full_retire_ring_leaves_the_ops_queued() {
        let mut spine: Spine<Toy> = Spine::with_capacity(8, 1);
        let mut plan = Box::new(DeskPlan::empty());
        let (mut tx, mut rx, mut rtx, mut rrx) = rings(4);
        for _ in 0..4 {
            rtx.push(EngineOp::Attach {
                slot: Slot(0),
                gen: 0,
                part: Box::new(0),
            })
            .unwrap();
        }
        tx.push(install([(0, 1)].into_iter())).unwrap();
        tx.push(EngineOp::Plan(plan_for(&[(0, 1)]))).unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert!(r.waiting && r.applied == 0, "{r:?}");
        assert_eq!(rx.slots(), 2, "an op was taken with nowhere to retire it");
        assert!(spine.lanes[0].is_none());
        drop(drain(&mut rrx));
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert_eq!(r.applied, 2);
        assert!(!r.waiting);
        assert_eq!(plan.lanes.len(), 1);
    }

    /// **A bad plan is refused whole, counted, and sent home, and the old one
    /// keeps playing**: a slot named twice, an empty slot, a stale gen.
    #[test]
    fn a_plan_naming_a_slot_twice_an_empty_slot_or_a_stale_gen_is_refused() {
        let mut spine: Spine<Toy> = Spine::with_capacity(40, 1);
        let mut plan = Box::new(DeskPlan::empty());
        let (mut tx, mut rx, mut rtx, mut rrx) = rings(16);
        tx.push(install([(0, 1), (1, 1), (33, 2)].into_iter()))
            .unwrap();
        tx.push(EngineOp::Plan(plan_for(&[(0, 1), (33, 2)])))
            .unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert_eq!(r.plan_refused, 0);
        let good_gen = plan.gen;
        drop(drain(&mut rrx));
        for (forged, (why, bad)) in (100u64..).zip([
            ("twice", vec![(0, 1), (1, 1), (0, 1)]),
            ("empty", vec![(0, 1), (5, 1)]),
            ("stale", vec![(0, 1), (33, 1)]),
            ("past capacity", vec![(400, 1)]),
        ]) {
            // A plan's lane list is built by `build_plan`, which keys slots
            // uniquely; a repeated slot is forged here the way a host bug
            // would forge it.
            let mut p = plan_for(&bad);
            if why == "twice" {
                let mut lanes = p.lanes.to_vec();
                lanes.push(lanes[0]);
                p.lanes = lanes.into_boxed_slice();
            }
            p.gen = forged;
            tx.push(EngineOp::Plan(p)).unwrap();
            let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
            assert_eq!(r.plan_refused, 1, "{why}: {r:?}");
            assert_eq!(
                plan.gen, good_gen,
                "{why}: the previous plan stopped playing"
            );
            assert_eq!(plan.lanes.len(), 2);
            match rrx.pop() {
                Ok(EngineOp::Plan(back)) => {
                    assert_eq!(back.gen, forged, "{why}: the wrong plan came home");
                }
                other => panic!("{why}: the refused plan did not come home: {other:?}"),
            }
            assert!(spine.check.is_empty(), "{why}: validation left its marks");
        }
    }

    /// **An Attach rides the room-first rule and the generation check.** A
    /// part for a lane of another generation, or for an empty slot, goes
    /// straight home; a part that displaces one sends the old one home.
    #[test]
    fn an_attach_for_another_generation_goes_home_and_a_displaced_part_retires() {
        let mut spine: Spine<Toy> = Spine::with_capacity(4, 1);
        let mut plan = Box::new(DeskPlan::empty());
        let (mut tx, mut rx, mut rtx, mut rrx) = rings(8);
        tx.push(install([(2, 5)].into_iter())).unwrap();
        tx.push(EngineOp::Attach {
            slot: Slot(2),
            gen: 4,
            part: Box::new(40),
        })
        .unwrap();
        tx.push(EngineOp::Attach {
            slot: Slot(1),
            gen: 5,
            part: Box::new(10),
        })
        .unwrap();
        tx.push(EngineOp::Attach {
            slot: Slot(2),
            gen: 5,
            part: Box::new(50),
        })
        .unwrap();
        tx.push(EngineOp::Attach {
            slot: Slot(2),
            gen: 5,
            part: Box::new(51),
        })
        .unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert_eq!(r.attach_retired, 2, "{r:?}");
        assert_eq!(spine.lanes[2].as_ref().unwrap().part.as_deref(), Some(&51));
        let parts: Vec<u32> = drain(&mut rrx)
            .into_iter()
            .filter_map(|op| match op {
                EngineOp::Attach { part, .. } => Some(*part),
                _ => None,
            })
            .collect();
        assert_eq!(parts, vec![40, 10, 50], "what came home, in order");

        // And with no room, the attach waits rather than seating.
        let (mut tx, mut rx, mut rtx, _rrx) = rings(1);
        rtx.push(EngineOp::Attach {
            slot: Slot(0),
            gen: 0,
            part: Box::new(0),
        })
        .unwrap();
        tx.push(EngineOp::Attach {
            slot: Slot(2),
            gen: 5,
            part: Box::new(52),
        })
        .unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert!(r.waiting && r.applied == 0);
        assert_eq!(spine.lanes[2].as_ref().unwrap().part.as_deref(), Some(&51));
    }

    /// **Install refuses an occupied slot, whole.** Half an install would
    /// leave the host's slot table and the spine disagreeing.
    #[test]
    fn an_install_onto_an_occupied_slot_is_refused_whole() {
        let mut spine: Spine<Toy> = Spine::with_capacity(4, 1);
        let mut plan = Box::new(DeskPlan::empty());
        let (mut tx, mut rx, mut rtx, mut rrx) = rings(8);
        tx.push(install([(1, 1)].into_iter())).unwrap();
        tx.push(install([(0, 2), (1, 2)].into_iter())).unwrap();
        tx.push(install([(3, 2), (3, 2)].into_iter())).unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert_eq!(r.install_refused, 2, "{r:?}");
        assert!(spine.lanes[0].is_none() && spine.lanes[3].is_none());
        assert_eq!(spine.lanes[1].as_ref().unwrap().gen, 1);
        let back = drain(&mut rrx);
        let full = back
            .iter()
            .filter(|op| matches!(op, EngineOp::Install(l) if l.iter().all(|(_, b)| b.is_some())))
            .count();
        assert_eq!(
            full, 2,
            "a refused install did not come home whole: {back:?}"
        );
    }

    /// **A second claim of a lane in one round answers None**, and the next
    /// round may take it again. Also: no claim of a lane of another
    /// generation, or of an empty slot.
    #[test]
    fn a_lane_is_claimed_once_per_round() {
        let mut spine: Spine<Toy> = Spine::with_capacity(70, 1);
        spine.lanes[64] = Some(toy(3));
        spine.gens[64] = 3;
        assert!(spine.claim(Slot(64), 3, 1).is_some());
        assert!(
            spine.claim(Slot(64), 3, 1).is_none(),
            "two hands held one lane"
        );
        assert!(
            spine.claim(Slot(64), 3, 2).is_some(),
            "the next round could not take it"
        );
        assert!(
            spine.claim(Slot(64), 2, 3).is_none(),
            "a stale gen was claimed"
        );
        assert!(
            spine.claim(Slot(0), 0, 3).is_none(),
            "an empty slot was claimed"
        );
        assert!(spine.claim(Slot(9999), 0, 3).is_none());

        // Under contention: eight threads race for every lane of 1000, a
        // hundred rounds; each lane is taken exactly once per round.
        let mut spine: Spine<Toy> = Spine::with_capacity(1000, 1);
        for s in 0..1000 {
            spine.lanes[s] = Some(toy(1));
            spine.gens[s] = 1;
        }
        let lanes = &spine.lanes[..];
        for round in 1..=100u64 {
            let taken: usize = std::thread::scope(|sc| {
                let hands: Vec<_> = (0..8)
                    .map(|_| {
                        sc.spawn(move || {
                            (0..1000u32)
                                .filter(|s| claim_lane(lanes, Slot(*s), 1, round).is_some())
                                .count()
                        })
                    })
                    .collect();
                hands.into_iter().map(|h| h.join().unwrap()).sum()
            });
            assert_eq!(taken, 1000, "round {round}: lanes taken {taken} times");
        }
    }

    /// **A Grow carries every lane, its generation, the monitor, the capture
    /// rings and one chunk of keys**, and sends the old shell home.
    #[test]
    fn a_grow_carries_the_lanes_and_the_key_snapshot() {
        let mut spine: Spine<Toy> = Spine::with_capacity(33, 2);
        let mut plan = Box::new(DeskPlan::empty());
        let (mut tx, mut rx, mut rtx, mut rrx) = rings(8);
        tx.push(install([(0, 1), (32, 9)].into_iter())).unwrap();
        apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        spine.capturing.insert(1);
        let (cap_tx, _cap_rx) = RingBuffer::<f32>::new(4);
        spine.captures[1] = Some(cap_tx);
        for f in 0..3usize {
            for v in 0..33 * 2 {
                spine.desk.key_prev[f * 33 * 2 + v] = (f * 1000 + v) as f32;
            }
        }
        spine.desk.key_prev_frames = 3;
        tx.push(EngineOp::Grow(Box::new(Spine::with_capacity(100, 2))))
            .unwrap();
        tx.push(EngineOp::Grow(Box::new(Spine::with_capacity(10, 2))))
            .unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert_eq!(r.grow_refused, 1, "a smaller shell was accepted: {r:?}");
        assert_eq!(spine.capacity(), 100);
        assert_eq!(spine.gens[32], 9);
        assert_eq!(spine.lanes[32].as_ref().unwrap().gen, 9);
        assert!(spine.lanes[0].is_some() && spine.lanes[1].is_none());
        assert!(spine.capturing.contains(1) && spine.captures[1].is_some());
        assert_eq!(spine.desk.key_prev_frames, 3);
        for f in 0..3usize {
            for v in 0..33 * 2 {
                assert_eq!(spine.desk.key_prev[f * 100 * 2 + v], (f * 1000 + v) as f32);
            }
        }
        let home = drain(&mut rrx);
        let old = home
            .iter()
            .find_map(|op| match op {
                EngineOp::Grow(s) if s.capacity() == 33 => Some(s),
                _ => None,
            })
            .expect("the old spine came home");
        assert!(
            old.lanes.iter().all(Option::is_none),
            "a lane went home with the old shell"
        );

        // A capture refused while its pick is capturing; allowed once not.
        let (c2, _) = RingBuffer::<f32>::new(4);
        tx.push(EngineOp::Capture {
            pick: PickId(1),
            ring: Some(c2),
        })
        .unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert_eq!(r.capture_refused, 1);
        spine.capturing.remove(1);
        let (c3, _) = RingBuffer::<f32>::new(4);
        tx.push(EngineOp::Capture {
            pick: PickId(1),
            ring: Some(c3),
        })
        .unwrap();
        let r = apply_ops(&mut spine, &mut plan, &mut rx, &mut rtx);
        assert_eq!(r.capture_refused, 0);
    }
}
