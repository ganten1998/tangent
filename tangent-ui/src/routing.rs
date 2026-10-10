//! Track routing: where a channel goes instead of, or as well as, the master.
//!
//! Two destinations, and they behave differently on purpose:
//!
//! - a **hardware output** takes a COPY of the channel out of the box on its
//!   own pair of jacks (3/4, 5/6, a single mono jack), which is how somebody
//!   feeds a headphone amp, a second interface, or a stage monitor. The
//!   channel stays in the master mix; the jacks are extra.
//! - a **channel route MOVES the channel.** It comes off the master and is
//!   folded into the destination channel instead, through whatever is loaded
//!   on it — a submix. The routing dialog says exactly this ("Routing to a
//!   channel MOVES it there"), and the engine enforces it by closing the
//!   channel's direct path to the master for as long as such a route exists
//!   (`route_direct` in `instrument.rs`). For a COPY — the classic reverb
//!   feed — the strip's SEND knob into the effects bus is the copy; calling
//!   both of them "send" is how you end up expecting to hear a track twice.
//!
//! **No duplicates**, by the owner's word: the same channel to the same
//! destination twice is two faders fighting over one path, and nobody means
//! that. [`Routes::add`] refuses it rather than silently merging.
//!
//! **Both ends by identity** ([`Node`], [`SeatBay`]). A route used to name
//! its tracks by row and a key its bay by strip and slot, so every
//! structural edit had to carry both tables through a permutation, and the
//! one that forgot (a reorder moved the key edges and left the routes behind
//! for years) made a moved track route somebody else's audio. A node does
//! not move when its row does, so a reorder, an insert above and a delete
//! elsewhere leave every entry here exactly as it was, and there is no remap
//! to forget. The legacy files and the engine's test shim still speak lanes:
//! [`LaneRoutes`] and [`LaneKeys`] are those frozen shapes, and the only
//! road between the two is through a desk's row ids.

use serde_json::{Map, Value};

use crate::desk_id::{display_slot, slot_key, Node, SeatBay, TrackId};
use crate::desk_legacy::{bind, decode_v32_strip, encode_v32_strip, unbind, LegacyOwner};

/// How many routes the desk carries at most.
///
/// The engine's routing table is a fixed array of atomics read on the audio
/// thread (`MAX_ROUTES` in `instrument.rs`, which must stay this number), and
/// `set_routes` publishes only this many entries. Sixty-four is four routes
/// per channel on a sixteen-channel desk, past anything a person builds by
/// hand — the ceiling exists so the audio thread never allocates, not to
/// ration anybody.
pub const MAX_ROUTES: usize = 64;

/// Where a route sends the audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Device output channels, zero-based. `stereo` sends the channel's own
    /// left and right to `first` and `first + 1`; mono sums them into `first`
    /// alone — a mono jack is one jack, and sending only the left channel to
    /// it would silently drop half of anything panned.
    Hardware { first: usize, stereo: bool },
    /// Another strip, by identity. This one MOVES the channel there — see
    /// the module docs; the copy is the SEND knob, not a route.
    Node(Node),
}

impl Target {
    /// What the strip under the button says. Hardware is named the way an
    /// interface names its jacks — one-based, because no interface in the
    /// world prints "0" on its back panel. A strip is named by `name`, the
    /// desk's own word for it: a route to the bass says "Bass", wherever the
    /// bass track stands, because a number is the one thing about a track
    /// that a reorder changes and nobody looks for.
    pub fn label(self, name: impl Fn(Node) -> String) -> String {
        match self {
            Target::Hardware { first, stereo: true } => format!("{}/{}", first + 1, first + 2),
            Target::Hardware { first, stereo: false } => format!("Out {}", first + 1),
            Target::Node(n) => name(n),
        }
    }
}

/// One route: a source strip, a destination, and how much of it goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Route {
    /// The strip this leaves FROM.
    pub from: Node,
    pub target: Target,
    /// 0..=1, the strip's own fader. Unity by default: a route that arrived
    /// silent would read as a route that does not work.
    pub gain: f32,
}

impl Route {
    pub fn new(from: Node, target: Target) -> Self {
        Self {
            from,
            target,
            gain: 1.0,
        }
    }
}

/// Every route on the desk, in the order they were added.
///
/// A flat list rather than a per-channel map: it serialises as one array, it
/// iterates in one pass, and "every route from the bass" is a filter rather
/// than a lookup that can be missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Routes(pub Vec<Route>);

impl Routes {
    /// Add a route unless the exact same one already exists, or the desk is
    /// already carrying [`MAX_ROUTES`].
    ///
    /// Returns whether it was added, so the caller can say why nothing
    /// happened. "The exact same one" is source AND destination: the bass to
    /// 3/4 and the bass to 5/6 are two different routes, and the drums to
    /// 3/4 is a third.
    ///
    /// The cap is refused HERE, not merely clamped downstream: the engine's
    /// `set_routes` publishes only the first `MAX_ROUTES` entries to the
    /// audio thread, so a sixty-fifth route accepted into this list would be
    /// stored, saved, drawn on the strip — and silently never sound. A fader
    /// that moves and does nothing, forever, is exactly the failure `prune`'s
    /// docs warn about, so the refusal happens where the caller can still say
    /// no out loud.
    pub fn add(&mut self, route: Route) -> bool {
        if self.0.len() >= MAX_ROUTES {
            return false;
        }
        if self.0.iter().any(|r| r.from == route.from && r.target == route.target) {
            return false;
        }
        self.0.push(route);
        true
    }

    /// Every route leaving one strip, with its index in the whole list so a
    /// strip can address itself back.
    pub fn from(&self, node: Node) -> Vec<(usize, Route)> {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, r)| r.from == node)
            .map(|(i, r)| (i, *r))
            .collect()
    }

    /// **Drop every route pointing INTO one strip.**
    ///
    /// Called when a track becomes a folder, because a group cannot be a send
    /// destination: the fold that would carry the arriving audio has already
    /// run by the time a send is gathered, so the audio is processed and then
    /// goes nowhere. Rather than leave a route that silently drops what it
    /// carries, the route goes — and folders are not offered in the picker in
    /// the first place. See `TangentApp::routing_dialog`.
    ///
    /// Returns how many were dropped, so a caller can tell somebody.
    pub fn drop_into(&mut self, node: Node) -> usize {
        let before = self.0.len();
        self.0.retain(|r| r.target != Target::Node(node));
        before - self.0.len()
    }

    /// **Drop every route touching a deleted strip**: the ones leaving it
    /// and the ones arriving into it.
    ///
    /// `KeyEdges::drop_node`'s job for routes, called from the same
    /// track-delete path. Neither end may outlive its track: a route left
    /// FROM the deleted track would wait, inert, for nobody, and one left
    /// INTO it would fold its source into a strip that is not there and out
    /// of earshot. Returns how many went, so a caller can tell somebody.
    pub fn drop_node(&mut self, node: Node) -> usize {
        let before = self.0.len();
        self.0.retain(|r| r.from != node && r.target != Target::Node(node));
        before - self.0.len()
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.0.len() {
            self.0.remove(index);
        }
    }

    pub fn set_gain(&mut self, index: usize, gain: f32) {
        if let Some(r) = self.0.get_mut(index) {
            r.gain = gain.clamp(0.0, 1.0);
        }
    }

    /// Drop routes that name strips or device outputs that no longer exist,
    /// and any that would send a strip to itself.
    ///
    /// **Called when the device changes, and it must not be silent about
    /// scale.** Somebody who unplugs an 18-out interface and plugs in a
    /// 2-out one has lost every route past the second jack; keeping them
    /// would mean a fader that moves and does nothing, forever. `on_desk`
    /// answers whether a strip is there to route from or to.
    pub fn prune(&mut self, device_outs: usize, on_desk: impl Fn(Node) -> bool) -> usize {
        let before = self.0.len();
        self.0.retain(|r| {
            if !on_desk(r.from) {
                return false;
            }
            match r.target {
                Target::Hardware { first, stereo } => {
                    let last = if stereo { first + 1 } else { first };
                    last < device_outs
                }
                Target::Node(n) => on_desk(n) && n != r.from,
            }
        });
        before - self.0.len()
    }

    /// Whether anything routes to hardware — the flag the render loop reads
    /// to decide whether to keep per-channel audio at all.
    pub fn any_hardware(&self) -> bool {
        self.0
            .iter()
            .any(|r| matches!(r.target, Target::Hardware { .. }))
    }

    /// **This table in the legacy LANE shape**, each track at the row `ids`
    /// gives it: what a legacy settings file or document wrote. A route
    /// whose end no row holds, or a special (no legacy route had one), has
    /// no lane shape and is left out.
    pub fn to_lanes(&self, ids: &[TrackId]) -> LaneRoutes {
        LaneRoutes(
            self.0
                .iter()
                .filter_map(|r| {
                    let target = match r.target {
                        Target::Hardware { first, stereo } => LaneTarget::Hardware { first, stereo },
                        Target::Node(n) => LaneTarget::Channel(lane_of(n, ids)?),
                    };
                    Some(LaneRoute { from: lane_of(r.from, ids)?, target, gain: r.gain })
                })
                .collect(),
        )
    }

    /// **A legacy LANE table, by identity**: lane r is the track `ids[r]`.
    /// A route naming a lane past the ids names no track and is left out,
    /// as the publish always left it unplayed (`prune`).
    pub fn from_lanes(lanes: &LaneRoutes, ids: &[TrackId]) -> Self {
        let track = |lane: usize| ids.get(lane).copied().map(Node::Track);
        Routes(
            lanes
                .0
                .iter()
                .filter_map(|r| {
                    let target = match r.target {
                        LaneTarget::Hardware { first, stereo } => Target::Hardware { first, stereo },
                        LaneTarget::Channel(n) => Target::Node(track(n)?),
                    };
                    Some(Route { from: track(r.from)?, target, gain: r.gain })
                })
                .collect(),
        )
    }
}

/// The row a node stands on under `ids`, when it is a track a row holds.
fn lane_of(node: Node, ids: &[TrackId]) -> Option<usize> {
    match unbind(node, ids)? {
        LegacyOwner::Row(r) => Some(usize::from(r)),
        _ => None,
    }
}

/// The hardware destinations a device offers, as the picker lists them.
///
/// Stereo pairs first, then the mono jacks: a pair is what most people want
/// and a mono jack is the exception, so the common answer is at the top. Pairs
/// are consecutive and even-aligned — 1/2, 3/4, 5/6 — because that is how
/// every interface labels its own outputs, and inventing 2/3 would be a pair
/// no manual mentions.
pub fn hardware_targets(device_outs: usize) -> Vec<Target> {
    let mut out = Vec::new();
    let mut first = 0;
    while first + 1 < device_outs {
        out.push(Target::Hardware { first, stereo: true });
        first += 2;
    }
    for ch in 0..device_outs {
        out.push(Target::Hardware {
            first: ch,
            stereo: false,
        });
    }
    out
}

/// One sidechain key edge: the effect in bay `seat` ducks to what strip
/// `from` is doing.
///
/// Desk topology, not plugin state: which channel keys a compressor is the
/// desk's fact, exactly as which channel a route leaves from is — so it lives
/// beside [`Routes`] rather than inside the plugin's own preset blob, and it
/// survives the plugin being swapped for another build of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEdge {
    /// The bay holding the keyed effect, by identity: whose rack and which
    /// bay of it, wherever its row stands.
    pub seat: SeatBay,
    /// The SOURCE strip.
    pub from: Node,
}

/// Every sidechain key on the desk — the [`Routes`] idiom: one flat sparse
/// list, serialised as one array, filtered rather than looked up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyEdges(pub Vec<KeyEdge>);

impl KeyEdges {
    /// Point one bay's key at `from`, or take it off with `None`.
    ///
    /// Replace-or-remove by bay, because a bay listens to at most one
    /// strip: a compressor has one key input, and keeping two edges for one
    /// bay would leave the engine to pick a winner silently.
    pub fn set(&mut self, seat: SeatBay, from: Option<Node>) {
        self.0.retain(|k| k.seat != seat);
        if let Some(from) = from {
            self.0.push(KeyEdge { seat, from });
        }
    }

    /// Which strip keys this bay, when one does.
    pub fn get(&self, seat: SeatBay) -> Option<Node> {
        self.0.iter().find(|k| k.seat == seat).map(|k| k.from)
    }

    /// Drop every edge touching a deleted strip — as its source, or as the
    /// owner of the bay that was keyed.
    ///
    /// [`Routes::drop_node`]'s job for keys, called from the same
    /// track-delete path: an edge left standing would key the bass from a
    /// track that is not there, and one on the deleted track's bays names
    /// effects that just unloaded. Returns how many went, so a caller can
    /// tell somebody.
    pub fn drop_node(&mut self, node: Node) -> usize {
        let before = self.0.len();
        self.0.retain(|k| k.from != node && k.seat.seat.node != node);
        before - self.0.len()
    }

    /// **These edges in the legacy LANE shape** (a strip index, a displayed
    /// slot and a source row), under `ids`: what a legacy file wrote. An
    /// edge on a bay no strip shows, or from a source no row holds, has no
    /// lane shape and is left out.
    pub fn to_lanes(&self, ids: &[TrackId]) -> LaneKeys {
        LaneKeys(
            self.0
                .iter()
                .filter_map(|k| {
                    let strip = unbind(k.seat.seat.node, ids).and_then(encode_v32_strip)?;
                    Some(LaneKey { strip, slot: display_slot(k.seat)?, from: lane_of(k.from, ids)? })
                })
                .collect(),
        )
    }

    /// **Legacy LANE edges, by identity**: a strip and its displayed slot
    /// through `desk_id::slot_key` (the master's two racks show the other
    /// way round), the source row through `ids`. An edge naming no strip,
    /// slot or row is left out.
    pub fn from_lanes(lanes: &LaneKeys, ids: &[TrackId]) -> Self {
        KeyEdges(
            lanes
                .0
                .iter()
                .filter_map(|k| {
                    let node = bind(decode_v32_strip(k.strip)?, ids)?;
                    Some(KeyEdge {
                        seat: slot_key(node, k.slot)?,
                        from: Node::Track(*ids.get(k.from)?),
                    })
                })
                .collect(),
        )
    }
}

// ── The legacy LANE shapes ──────────────────────────────────────────────────
//
// What every settings file and document before format 2 wrote, and what the
// engine's own test shim (`Engine::set_routes`) still drives its lanes with:
// a route by row numbers, a key by strip index and displayed slot. Frozen,
// read and written through `to_lanes`/`from_lanes` and nowhere else.

/// Where a legacy route went: jacks, or a channel by its LANE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneTarget {
    Hardware { first: usize, stereo: bool },
    Channel(usize),
}

/// One legacy route, by lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneRoute {
    pub from: usize,
    pub target: LaneTarget,
    pub gain: f32,
}

impl LaneRoute {
    pub fn new(from: usize, target: LaneTarget) -> Self {
        Self { from, target, gain: 1.0 }
    }
}

/// A legacy routing table, by lane.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaneRoutes(pub Vec<LaneRoute>);

impl LaneRoutes {
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.0
                .iter()
                .map(|r| {
                    let mut m = Map::new();
                    m.insert("from".into(), Value::from(r.from as i64));
                    match r.target {
                        LaneTarget::Hardware { first, stereo } => {
                            m.insert("kind".into(), Value::String("hardware".into()));
                            m.insert("first".into(), Value::from(first as i64));
                            m.insert("stereo".into(), Value::Bool(stereo));
                        }
                        LaneTarget::Channel(n) => {
                            m.insert("kind".into(), Value::String("channel".into()));
                            m.insert("channel".into(), Value::from(n as i64));
                        }
                    }
                    m.insert("gain".into(), Value::from(f64::from(r.gain)));
                    Value::Object(m)
                })
                .collect(),
        )
    }

    /// Read routes back. A malformed entry is skipped rather than failing the
    /// whole list — the same forgiveness the rest of the settings file gets.
    pub fn from_json(v: &Value) -> Self {
        let Value::Array(items) = v else {
            return Self::default();
        };
        let mut out = Self::default();
        for it in items {
            let Value::Object(m) = it else { continue };
            let Some(from) = m.get("from").and_then(Value::as_i64) else {
                continue;
            };
            let gain = m
                .get("gain")
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
                .clamp(0.0, 1.0) as f32;
            let target = match m.get("kind").and_then(Value::as_str) {
                Some("hardware") => {
                    let Some(first) = m.get("first").and_then(Value::as_i64) else {
                        continue;
                    };
                    LaneTarget::Hardware {
                        first: first.max(0) as usize,
                        stereo: m.get("stereo").and_then(Value::as_bool).unwrap_or(true),
                    }
                }
                Some("channel") => {
                    let Some(n) = m.get("channel").and_then(Value::as_i64) else {
                        continue;
                    };
                    LaneTarget::Channel(n.max(0) as usize)
                }
                _ => continue,
            };
            // Not `push`: a hand-edited file with the same route twice must
            // not produce two faders over one path. **And not a cap**,
            // which is for a GESTURE (`Routes::add` refuses a route the
            // engine would not play, while somebody is there to hear why):
            // a read that stopped at the cap cut every route past the
            // sixty-fourth out of the file at the next save, with nobody
            // told. Every route is read.
            let route = LaneRoute { from: from.max(0) as usize, target, gain };
            if !out.0.iter().any(|r| r.from == route.from && r.target == route.target) {
                out.0.push(route);
            }
        }
        out
    }
}

/// One legacy key edge: a strip index (`Strip::index` space, the master at
/// 35), the strip's displayed slot, and the source LANE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneKey {
    pub strip: usize,
    pub slot: usize,
    pub from: usize,
}

/// Legacy key edges, by strip and slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaneKeys(pub Vec<LaneKey>);

impl LaneKeys {
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.0
                .iter()
                .map(|k| {
                    let mut m = Map::new();
                    m.insert("strip".into(), Value::from(k.strip as i64));
                    m.insert("slot".into(), Value::from(k.slot as i64));
                    m.insert("from".into(), Value::from(k.from as i64));
                    Value::Object(m)
                })
                .collect(),
        )
    }

    /// Read the keys back. A malformed entry is skipped rather than failing
    /// the list — the same forgiveness `LaneRoutes::from_json` extends — and
    /// a bay named twice keeps its FIRST edge, because two sources on one key
    /// input is a hand-edited file's mistake, not a thing to publish.
    pub fn from_json(v: &Value) -> Self {
        let Value::Array(items) = v else {
            return Self::default();
        };
        let mut out = Self::default();
        for it in items {
            let Value::Object(m) = it else { continue };
            // `as_u64`, not `as_i64`: a negative index is malformed, and
            // clamping it to zero would key some real channel by accident.
            let (Some(strip), Some(slot), Some(from)) = (
                m.get("strip").and_then(Value::as_u64),
                m.get("slot").and_then(Value::as_u64),
                m.get("from").and_then(Value::as_u64),
            ) else {
                continue;
            };
            let (strip, slot) = (strip as usize, slot as usize);
            if !out.0.iter().any(|k| k.strip == strip && k.slot == slot) {
                out.0.push(LaneKey { strip, slot, from: from as usize });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk_id::Rack;

    /// The track a test names by number: its id, which no row's position
    /// decides.
    fn t(n: u32) -> Node {
        Node::Track(TrackId(n))
    }

    fn bay(n: u32, slot: usize) -> SeatBay {
        slot_key(t(n), slot).expect("a shown slot")
    }

    /// The desk's own names, for the labels: a track by a word.
    fn named(n: Node) -> String {
        match n {
            Node::Track(TrackId(8)) => "Pro-R bus".to_owned(),
            other => other.to_string(),
        }
    }

    #[test]
    fn the_same_route_twice_is_refused() {
        let mut r = Routes::default();
        let hw = Target::Hardware { first: 2, stereo: true };
        assert!(r.add(Route::new(t(3), hw)));
        assert!(!r.add(Route::new(t(3), hw)), "the same route was added twice");
        assert_eq!(r.0.len(), 1);
        // A different source to the same jacks is a different route, and a
        // real thing to want: two channels feeding one headphone amp.
        assert!(r.add(Route::new(t(4), hw)));
        // And the same source to different jacks.
        assert!(r.add(Route::new(t(3), Target::Hardware { first: 4, stereo: true })));
        assert_eq!(r.0.len(), 3);
    }

    /// **A route names its destination the way the desk does** - by name,
    /// since the number is the one thing a reorder changes - and an output
    /// the way the interface prints it.
    #[test]
    fn a_send_names_the_channel_the_owner_would_name() {
        // The owner's example: "pro r could be on 9 and i could send 3 to 9".
        let mut r = Routes::default();
        assert!(r.add(Route::new(t(2), Target::Node(t(8)))));
        assert_eq!(r.0[0].target.label(named), "Pro-R bus", "a strip is named by its name");
        assert_eq!(
            Target::Hardware { first: 2, stereo: true }.label(named),
            "3/4",
            "outputs are named the way an interface prints them"
        );
        assert_eq!(Target::Hardware { first: 0, stereo: false }.label(named), "Out 1");
    }

    #[test]
    fn a_channel_cannot_send_to_itself() {
        // Feedback with extra steps. Pruning is where it is caught, because
        // that is the one place that sees the whole desk.
        let mut r = Routes::default();
        r.add(Route::new(t(5), Target::Node(t(5))));
        let dropped = r.prune(8, |_| true);
        assert_eq!(dropped, 1);
        assert!(r.0.is_empty());
    }

    #[test]
    fn routes_past_the_end_of_a_smaller_interface_are_dropped() {
        let mut r = Routes::default();
        r.add(Route::new(t(0), Target::Hardware { first: 0, stereo: true }));
        r.add(Route::new(t(1), Target::Hardware { first: 6, stereo: true }));
        r.add(Route::new(t(2), Target::Node(t(9))));
        // An 18-out interface unplugged, a 2-out one plugged in.
        let dropped = r.prune(2, |_| true);
        assert_eq!(dropped, 1, "the 7/8 route should be gone");
        assert_eq!(r.0.len(), 2);
        // The channel send survives: it never touched the interface.
        assert!(r.0.iter().any(|x| x.target == Target::Node(t(9))));
    }

    /// **A route to or from a strip the desk does not have is pruned**, and
    /// only that one: the desk says which strips are there, not a width.
    #[test]
    fn a_route_naming_a_strip_the_desk_lacks_is_pruned() {
        let mut r = Routes::default();
        r.add(Route::new(t(40), Target::Node(t(2))));
        r.add(Route::new(t(2), Target::Node(t(41))));
        r.add(Route::new(t(2), Target::Node(t(3))));
        let held = |n: Node| n != t(40) && n != t(41);
        assert_eq!(r.prune(2, held), 2);
        assert_eq!(r.0, vec![Route::new(t(2), Target::Node(t(3)))]);
    }

    #[test]
    fn the_picker_lists_pairs_before_mono_jacks_and_aligns_them() {
        let t = hardware_targets(4);
        assert_eq!(t[0], Target::Hardware { first: 0, stereo: true });
        assert_eq!(t[1], Target::Hardware { first: 2, stereo: true });
        // Then the four mono jacks.
        assert_eq!(t[2], Target::Hardware { first: 0, stereo: false });
        assert_eq!(t.len(), 2 + 4);
        // No 2/3: pairs are even-aligned, as every interface labels them.
        assert!(!t.contains(&Target::Hardware { first: 1, stereo: true }));
        // A mono interface offers exactly one jack and no pair at all.
        assert_eq!(hardware_targets(1), vec![Target::Hardware { first: 0, stereo: false }]);
    }

    /// **The lane shape round-trips through the legacy settings file**, and
    /// a table by identity round-trips through it under a desk's ids.
    #[test]
    fn routes_round_trip_through_the_settings_file() {
        let mut r = LaneRoutes::default();
        r.0.push(LaneRoute::new(2, LaneTarget::Hardware { first: 2, stereo: true }));
        r.0.push(LaneRoute { from: 3, target: LaneTarget::Channel(8), gain: 0.42 });
        let back = LaneRoutes::from_json(&r.to_json());
        assert_eq!(back.0.len(), 2);
        assert_eq!(back.0[0].target, LaneTarget::Hardware { first: 2, stereo: true });
        assert_eq!(back.0[1].target, LaneTarget::Channel(8));
        assert!((back.0[1].gain - 0.42).abs() < 1e-6);
        // By identity and back, under ids that are not the identity: the
        // lanes are where those tracks stand.
        let ids: Vec<TrackId> = [7, 3, 9, 1, 5, 0, 2, 4, 8].map(TrackId).to_vec();
        let routes = Routes::from_lanes(&back, &ids);
        assert_eq!(routes.0[1].from, t(1), "lane 3 is the track standing there");
        assert_eq!(routes.0[1].target, Target::Node(t(8)));
        assert_eq!(routes.to_lanes(&ids), back);
    }

    #[test]
    fn a_hand_edited_file_with_a_duplicate_yields_one_route() {
        let json: Value = serde_json::from_str(
            r#"[{"from":1,"kind":"hardware","first":2,"stereo":true,"gain":1.0},
                {"from":1,"kind":"hardware","first":2,"stereo":true,"gain":0.5},
                {"from":1,"kind":"nonsense"}]"#,
        )
        .expect("valid json");
        let r = LaneRoutes::from_json(&json);
        assert_eq!(r.0.len(), 1, "the duplicate and the junk should both be gone");
        assert!((r.0[0].gain - 1.0).abs() < 1e-6, "the FIRST one wins");
    }

    #[test]
    fn only_hardware_routes_ask_the_render_loop_for_extra_work() {
        let mut r = Routes::default();
        assert!(!r.any_hardware());
        r.add(Route::new(t(0), Target::Node(t(3))));
        assert!(!r.any_hardware(), "a send needs no per-channel device buffer");
        r.add(Route::new(t(0), Target::Hardware { first: 2, stereo: true }));
        assert!(r.any_hardware());
    }

    #[test]
    fn a_sixty_fifth_route_is_refused_rather_than_stored_and_never_sounding() {
        // The engine publishes only the first MAX_ROUTES entries to the audio
        // thread, so anything held past that would be a route the settings
        // file remembers and the desk never plays. `add` must say no while
        // somebody is still there to hear it.
        let mut r = Routes::default();
        for i in 0..MAX_ROUTES as u32 {
            assert!(
                r.add(Route::new(t(i % 16), Target::Node(t(16 + i)))),
                "route {i} is under the cap and distinct, it should fit"
            );
        }
        // Not a duplicate of anything above — the refusal is the cap alone.
        assert!(
            !r.add(Route::new(t(0), Target::Hardware { first: 0, stereo: true })),
            "route MAX_ROUTES + 1 must be refused"
        );
        assert_eq!(r.0.len(), MAX_ROUTES);
        // Taking one off makes room again: the cap is a ceiling, not a latch.
        r.remove(0);
        assert!(r.add(Route::new(t(0), Target::Hardware { first: 0, stereo: true })));
        assert_eq!(r.0.len(), MAX_ROUTES);
    }

    #[test]
    fn a_gain_is_always_a_fraction() {
        let mut r = Routes::default();
        r.add(Route::new(t(0), Target::Node(t(1))));
        r.set_gain(0, 9.0);
        assert!((r.0[0].gain - 1.0).abs() < 1e-6);
        r.set_gain(0, -3.0);
        assert!(r.0[0].gain.abs() < 1e-6);
    }

    /// **A key edge survives the legacy file's round trip**, the master's
    /// second rack included: its displayed slots 0..2 are its EXTRA chain,
    /// so slot 5 is its original chain's third bay (`slot_key`).
    #[test]
    fn a_key_edge_survives_the_json_round_trip() {
        let mut k = LaneKeys::default();
        k.0.push(LaneKey { strip: 3, slot: 1, from: 7 });
        k.0.push(LaneKey { strip: 35, slot: 5, from: 0 });
        let back = LaneKeys::from_json(&k.to_json());
        assert_eq!(back, k);
        let ids = crate::desk_legacy::identity_ids(32);
        let edges = KeyEdges::from_lanes(&back, &ids);
        assert_eq!(edges.get(bay(3, 1)), Some(t(7)));
        assert_eq!(
            edges.get(SeatBay::new(Node::Master, Rack::Chain(0), 2)),
            Some(t(0)),
            "the master's slot 5 is its original rack's third bay"
        );
        assert_eq!(edges.to_lanes(&ids), back);
    }

    #[test]
    fn setting_a_bays_key_replaces_it_and_none_takes_it_off() {
        let mut k = KeyEdges::default();
        k.set(bay(2, 0), Some(t(5)));
        k.set(bay(2, 0), Some(t(9)));
        assert_eq!(k.get(bay(2, 0)), Some(t(9)), "a bay listens to one channel");
        assert_eq!(k.0.len(), 1, "the replaced edge did not linger");
        k.set(bay(2, 0), None);
        assert_eq!(k.get(bay(2, 0)), None);
        assert!(k.0.is_empty());
    }

    #[test]
    fn malformed_key_rows_are_skipped_and_a_doubled_bay_keeps_its_first() {
        let v: Value = serde_json::from_str(
            r#"[
                {"strip": 4, "slot": 1, "from": 2},
                {"strip": 4, "slot": 1, "from": 9},
                {"strip": -1, "slot": 0, "from": 3},
                {"strip": 5, "from": 3},
                "not even an object"
            ]"#,
        )
        .unwrap();
        let k = LaneKeys::from_json(&v);
        assert_eq!(k.0.len(), 1, "only the first well-formed row survives");
        assert_eq!(k.0[0], LaneKey { strip: 4, slot: 1, from: 2 }, "the doubled bay kept its first edge");
    }

    #[test]
    fn deleting_a_channel_takes_its_key_edges_at_both_ends() {
        let mut k = KeyEdges::default();
        k.set(bay(2, 0), Some(t(5))); // 2's bay keyed by the deleted track
        k.set(bay(5, 1), Some(t(0))); // the deleted track's own bay is keyed
        k.set(bay(3, 2), Some(t(1))); // untouched, to prove it is not a sweep
        assert_eq!(k.drop_node(t(5)), 2);
        assert_eq!(k.0, vec![KeyEdge { seat: bay(3, 2), from: t(1) }]);
    }

    /// **A deleted channel takes its routes at both ends**, and only its
    /// own: a route out of it, a route into it, and its hardware copy go;
    /// the neighbours' routes stay exactly as they were.
    #[test]
    fn deleting_a_channel_drops_its_routes_into_and_out_of_it() {
        let mut r = Routes::default();
        r.add(Route::new(t(5), Target::Node(t(9)))); // out of the deleted track
        r.add(Route::new(t(2), Target::Node(t(5)))); // into it
        r.add(Route::new(t(5), Target::Hardware { first: 2, stereo: true })); // its jacks
        r.add(Route::new(t(3), Target::Node(t(9)))); // a stranger's, kept
        r.add(Route::new(t(4), Target::Hardware { first: 0, stereo: false })); // kept
        assert_eq!(r.drop_node(t(5)), 3);
        assert_eq!(
            r.0,
            vec![
                Route::new(t(3), Target::Node(t(9))),
                Route::new(t(4), Target::Hardware { first: 0, stereo: false }),
            ]
        );
        assert_eq!(r.drop_node(t(5)), 0, "a second delete finds nothing");
    }

    /// **A reorder changes nothing here**: both ends of a route and of a key
    /// are tracks, and a track is the same track on any row. Read in the
    /// legacy lane shape before and after, the same table says each track's
    /// NEW row - which is all a reorder ever had to do to it.
    #[test]
    fn a_reorder_leaves_both_ends_of_a_route_and_a_key_on_their_tracks() {
        let mut r = Routes::default();
        r.add(Route::new(t(0), Target::Node(t(2))));
        r.add(Route::new(t(2), Target::Hardware { first: 4, stereo: true }));
        r.add(Route::new(t(7), Target::Node(t(0))));
        let mut k = KeyEdges::default();
        k.set(bay(0, 1), Some(t(2)));
        k.set(SeatBay::new(Node::Master, Rack::Chain(0), 0), Some(t(0)));
        let ids = crate::desk_legacy::identity_ids(8);
        // Rows 0 and 2 swap, and 7 moves to 1: the tracks, not the tables.
        let mut moved = ids.clone();
        moved.swap(0, 2);
        moved.swap(1, 7);
        assert_eq!(r.to_lanes(&ids).0[0], LaneRoute::new(0, LaneTarget::Channel(2)), "fixture");
        assert_eq!(
            r.to_lanes(&moved).0,
            vec![
                LaneRoute::new(2, LaneTarget::Channel(0)),
                LaneRoute::new(0, LaneTarget::Hardware { first: 4, stereo: true }),
                LaneRoute::new(1, LaneTarget::Channel(2)),
            ],
            "read by lane, each route stands where its tracks went"
        );
        let lanes = k.to_lanes(&moved);
        assert!(lanes.0.contains(&LaneKey { strip: 2, slot: 1, from: 0 }), "{lanes:?}");
        assert!(lanes.0.contains(&LaneKey { strip: 35, slot: 3, from: 2 }), "{lanes:?}");
    }
}
