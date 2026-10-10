//! **Which way audio flows between the desk's strips**, as a graph of
//! [`Node`]s, and the one question a picker asks of it: would this new
//! route or send close a loop (design section 9).
//!
//! **Three kinds of edge carry audio from one strip to another:**
//!
//! - a **route** to a node MOVES its source there (`routing::Route`); a
//!   route to the jacks leaves the box and reaches no strip;
//! - a **send** COPIES its strip into the effects return or a bus row
//!   (`desk_model::Send`);
//! - a **folder** sums its members: every row in a folder's run reaches the
//!   folder row (`DeskModel::folder_of`'s walk).
//!
//! A sidechain KEY is not an edge. A key reads its source's stem from the
//! chunk before (`Renderer::key_prev`), so a compressor keyed from a track it
//! feeds is a ducker, never a feedback loop; refusing it would forbid the
//! commonest sidechain there is.
//!
//! **Why a loop is refused at the picker and nowhere else.** The engine plays
//! a loop it is handed: its members are planned in a final level one block
//! late (`desk_plan::build_plan`, today's Kahn tail), and a legacy desk that
//! saved one opens and sounds exactly as it did. What nobody should be
//! offered is a NEW loop, because the first sign of one is a howl. So the
//! routing and send pickers grey every target that can already reach the
//! source ([`Topology::would_cycle`]), and the routing window marks a route
//! that is part of an old loop as feedback ([`Topology::in_loop`]) rather
//! than removing it behind somebody's back.
//!
//! **Pure**: built from the desk's parts, knowing nothing of settings,
//! documents or egui, so its rules are pinned below without an app.

use std::collections::{BTreeMap, BTreeSet};

use crate::desk_id::Node;
use crate::desk_model::{DeskModel, RouteTo, Specials, TrackRow};

/// **The desk's audio graph**: every edge along which one strip's audio
/// reaches another, out and in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Topology {
    out: BTreeMap<Node, Vec<Node>>,
    into: BTreeMap<Node, Vec<Node>>,
}

impl Topology {
    /// **The graph of a desk**: the rows' sends and folder runs, the
    /// specials' sends, and `routes` as `(from, to)` pairs of every route
    /// to a node (a route to the jacks reaches no strip and is left out by
    /// the caller, or by [`Self::of_desk`] and [`Self::of_model`]).
    pub fn new(rows: &[TrackRow], specials: &Specials, routes: impl IntoIterator<Item = (Node, Node)>) -> Self {
        let mut t = Topology::default();
        // **A send is an edge only where it sounds** (`desk_model::bus_sends`,
        // the one rule the app and the bridge read): a send aimed at a track
        // that is no bus stays in the file but carries nothing, so it closes
        // no loop. A send to a node that is no track (the effects return)
        // is an edge as it always was.
        let buses = crate::desk_model::buses_of(rows);
        let sounds = |s: &crate::desk_model::Send| s.to.track().is_none_or(|id| buses.contains(&id));
        // A folder's run, by the walk every reader of the flags uses: a
        // member belongs to the nearest folder above it with only members
        // between, and a member flag with anything else between is stale
        // and belongs to nobody.
        let mut reach: Option<Node> = None;
        for r in rows {
            let me = Node::Track(r.id);
            if r.in_folder {
                if let Some(folder) = reach {
                    t.edge(me, folder);
                }
            }
            reach = if r.folder {
                Some(me)
            } else if r.in_folder {
                reach
            } else {
                None
            };
            for s in r.sends.iter().filter(|s| sounds(s)) {
                t.edge(me, s.to);
            }
        }
        for (node, sp) in specials.iter() {
            for s in sp.sends.iter().filter(|s| sounds(s)) {
                t.edge(node, s.to);
            }
        }
        for (from, to) in routes {
            t.edge(from, to);
        }
        t
    }

    /// The live desk's graph: its rows and specials, and its routing table
    /// as the app keeps it.
    pub fn of_desk(rows: &[TrackRow], specials: &Specials, routes: &crate::routing::Routes) -> Self {
        let pairs = routes.0.iter().filter_map(|r| match r.target {
            crate::routing::Target::Node(to) => Some((r.from, to)),
            crate::routing::Target::Hardware { .. } => None,
        });
        Self::new(rows, specials, pairs)
    }

    /// A model's graph.
    pub fn of_model(m: &DeskModel) -> Self {
        let pairs = m.routes.iter().filter_map(|e| match e.to {
            RouteTo::Node(to) => Some((e.from, to)),
            RouteTo::Hardware { .. } => None,
        });
        Self::new(&m.rows, &m.specials, pairs)
    }

    fn edge(&mut self, from: Node, to: Node) {
        self.out.entry(from).or_default().push(to);
        self.into.entry(to).or_default().push(from);
    }

    /// **Whether audio leaving `from` arrives at `to`** along one edge or
    /// more. A node reaches itself only around a loop.
    pub fn reaches(&self, from: Node, to: Node) -> bool {
        walk(&self.out, from).contains(&to)
    }

    /// **Every node whose audio arrives at `of`**, along one edge or more:
    /// the targets a picker for a route or a send leaving `of` must grey.
    /// One walk for the whole list, so a picker over a thousand rows asks
    /// it once rather than once a row.
    pub fn upstream(&self, of: Node) -> BTreeSet<Node> {
        walk(&self.into, of)
    }

    /// **Whether a new edge from `from` to `to` would close a loop**: `to`
    /// is `from` itself, or `to`'s audio already arrives at `from`.
    pub fn would_cycle(&self, from: Node, to: Node) -> bool {
        from == to || self.reaches(to, from)
    }

    /// **Whether the edge from `from` to `to` that the desk HAS is part of a
    /// loop**: what a window draws as feedback. A legacy desk could save
    /// one; it is played (one block late, as it always was) and shown for
    /// what it is.
    pub fn in_loop(&self, from: Node, to: Node) -> bool {
        self.out.get(&from).is_some_and(|o| o.contains(&to)) && self.would_cycle(from, to)
    }
}

/// **Whether a new route or send from `from` to `to` would close a loop** on
/// the live desk: [`Topology::would_cycle`] over [`Topology::of_desk`].
pub fn would_cycle(
    rows: &[TrackRow],
    specials: &Specials,
    routes: &crate::routing::Routes,
    from: Node,
    to: Node,
) -> bool {
    Topology::of_desk(rows, specials, routes).would_cycle(from, to)
}

/// Every node reached from `start` along `edges`, one edge or more:
/// `start` is in it only around a loop.
fn walk(edges: &BTreeMap<Node, Vec<Node>>, start: Node) -> BTreeSet<Node> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<Node> = edges.get(&start).cloned().unwrap_or_default();
    while let Some(n) = stack.pop() {
        if seen.insert(n) {
            if let Some(next) = edges.get(&n) {
                stack.extend(next.iter().copied());
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk_id::TrackId;
    use crate::desk_model::{RowKind, Send};
    use crate::routing::{Route, Routes, Target};

    fn t(n: u32) -> Node {
        Node::Track(TrackId(n))
    }

    fn row(n: u32) -> TrackRow {
        let mut r = TrackRow::vacant(TrackId(n));
        r.kind = Some(RowKind::Audio);
        r
    }

    fn bus(n: u32) -> TrackRow {
        let mut r = TrackRow::vacant(TrackId(n));
        r.kind = Some(RowKind::Bus);
        r
    }

    fn send(to: Node) -> Send {
        Send { to, level: 1.0, pre: false }
    }

    fn routed(pairs: &[(Node, Node)]) -> Routes {
        Routes(pairs.iter().map(|(f, to)| Route::new(*f, Target::Node(*to))).collect())
    }

    /// **A route, a send and a folder are each an edge, and a loop through
    /// any mix of them is a loop.**
    #[test]
    fn a_loop_through_routes_sends_and_folders_is_refused_and_a_chain_is_not() {
        // 0 sends to bus 5, bus 5 routes to bus 6.
        let mut rows = vec![row(0), row(1), bus(5), bus(6)];
        rows[0].sends.push(send(t(5)));
        let routes = routed(&[(t(5), t(6))]);
        let topo = Topology::of_desk(&rows, &Specials::default(), &routes);
        assert!(topo.reaches(t(0), t(6)), "a send then a route carries the audio on");
        assert!(!topo.reaches(t(6), t(0)));
        // Bus 6 back into track 0, or into bus 5, closes it; into track 1
        // does not, and nor does a second send from 0 to 6.
        assert!(topo.would_cycle(t(6), t(0)));
        assert!(topo.would_cycle(t(6), t(5)));
        assert!(!topo.would_cycle(t(6), t(1)));
        assert!(!topo.would_cycle(t(0), t(6)));
        // Nothing reaches itself without a loop, and a strip to itself is one.
        assert!(!topo.reaches(t(0), t(0)));
        assert!(topo.would_cycle(t(1), t(1)));
        // The free function is the same answer.
        assert!(would_cycle(&rows, &Specials::default(), &routes, t(6), t(0)));
        assert!(!would_cycle(&rows, &Specials::default(), &routes, t(6), t(1)));
    }

    /// **A folder sums its members**, so a folder routed into a bus that
    /// sits inside it is a loop, and so is a member sent to a bus that
    /// routes into one of the folder's own feeders. A stale member flag
    /// (a plain row between it and any folder) joins nobody.
    #[test]
    fn a_folder_reaches_from_its_members_and_a_stale_member_joins_nobody() {
        let mut rows = vec![row(10), bus(11), row(12), row(13), row(14)];
        rows[0].folder = true; // folder 10 holds 11 and 12
        rows[1].in_folder = true;
        rows[2].in_folder = true;
        rows[4].in_folder = true; // 14 under the plain row 13: stale
        let topo = Topology::of_desk(&rows, &Specials::default(), &Routes::default());
        assert!(topo.reaches(t(11), t(10)), "a member reaches its folder");
        assert!(topo.reaches(t(12), t(10)));
        assert!(!topo.reaches(t(14), t(10)), "a stale member flag joined a folder");
        // The folder into the bus inside it: a loop.
        assert!(topo.would_cycle(t(10), t(11)));
        // The folder into the plain row below its run: no loop.
        assert!(!topo.would_cycle(t(10), t(13)));
        // A nested run: 20 is a folder inside 10's run, 21 inside 20's.
        let mut rows = vec![row(10), row(20), row(21), row(22)];
        rows[0].folder = true;
        rows[1].folder = true;
        rows[1].in_folder = true;
        rows[2].in_folder = true;
        rows[3].in_folder = true;
        let topo = Topology::of_desk(&rows, &Specials::default(), &Routes::default());
        assert!(topo.reaches(t(21), t(20)) && topo.reaches(t(20), t(10)));
        assert!(topo.reaches(t(21), t(10)), "a nested member reaches the outer folder through the inner");
        assert!(topo.would_cycle(t(10), t(21)), "the outer folder into a nested member is a loop");
    }

    /// **A route to the jacks reaches no strip, and a key is no edge**: a
    /// track keyed from the bus it feeds is a ducker, not a loop.
    #[test]
    fn jacks_and_keys_close_no_loop() {
        let mut rows = vec![row(0), bus(1)];
        rows[0].sends.push(send(t(1)));
        let mut routes = routed(&[]);
        routes.0.push(Route::new(t(1), Target::Hardware { first: 2, stereo: true }));
        let mut m = DeskModel::empty();
        m.rows = rows.clone();
        m.routes = crate::desk_model::route_edges(&routes);
        // The bus's compressor keyed from track 0, which feeds it.
        m.keys.push(crate::desk_model::SideKey {
            seat: crate::desk_id::SeatBay::new(t(1), crate::desk_id::Rack::Chain(0), 0),
            from: t(0),
        });
        let topo = Topology::of_model(&m);
        assert_eq!(topo, Topology::of_desk(&rows, &Specials::default(), &routes), "a model and the live desk agree");
        assert!(!topo.reaches(t(1), t(0)));
        assert!(!topo.would_cycle(t(0), t(1)), "a second send along an edge is no loop");
        assert!(topo.would_cycle(t(1), t(0)));
    }

    /// **The specials send too**: the drum bus's send to a bus is an edge
    /// like a row's, so what the drum bus feeds is downstream of it, and a
    /// bus it feeds may not be routed back into it.
    #[test]
    fn a_specials_send_is_an_edge() {
        let rows = vec![bus(3), bus(4)];
        let mut specials = Specials::default();
        specials.drums.sends.push(send(t(3)));
        let topo = Topology::of_desk(&rows, &specials, &routed(&[(t(3), t(4))]));
        assert!(topo.reaches(Node::Drums, t(4)));
        assert_eq!(topo.upstream(t(4)), BTreeSet::from([Node::Drums, t(3)]));
        assert!(topo.would_cycle(t(4), Node::Drums));
    }

    /// **A legacy loop is drawn as feedback, edge by edge**, and an edge
    /// off the loop is not.
    #[test]
    fn an_old_loop_is_marked_feedback_and_its_tail_is_not() {
        let rows = vec![row(0), row(1), row(2)];
        let topo = Topology::of_desk(&rows, &Specials::default(), &routed(&[(t(0), t(1)), (t(1), t(0)), (t(1), t(2))]));
        assert!(topo.in_loop(t(0), t(1)));
        assert!(topo.in_loop(t(1), t(0)));
        assert!(!topo.in_loop(t(1), t(2)), "the way out of a loop is not feedback");
        assert!(!topo.in_loop(t(2), t(0)), "an edge the desk does not have is in no loop");
    }

    /// **`upstream` is `reaches` asked of every node at once**, over random
    /// graphs with loops, so a picker that greys from one walk greys exactly
    /// what asking row by row would.
    #[test]
    fn upstream_is_every_node_that_reaches() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |m: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % m
        };
        for _ in 0..200 {
            let n = 2 + next(12) as u32;
            let mut rows: Vec<TrackRow> = (0..n).map(|i| if next(3) == 0 { bus(i) } else { row(i) }).collect();
            for r in &mut rows {
                for _ in 0..next(3) {
                    r.sends.push(send(t(next(u64::from(n)) as u32)));
                }
                r.folder = next(5) == 0;
                r.in_folder = next(3) == 0;
            }
            let pairs: Vec<(Node, Node)> =
                (0..next(10)).map(|_| (t(next(u64::from(n)) as u32), t(next(u64::from(n)) as u32))).collect();
            let topo = Topology::of_desk(&rows, &Specials::default(), &routed(&pairs));
            for a in 0..n {
                let up = topo.upstream(t(a));
                for b in 0..n {
                    assert_eq!(up.contains(&t(b)), topo.reaches(t(b), t(a)), "{b} -> {a}");
                    assert_eq!(topo.would_cycle(t(a), t(b)), a == b || up.contains(&t(b)));
                }
            }
        }
    }

    /// **A thousand sends and a hundred buses are one walk each**: the
    /// picker asks once per window, never once per row.
    #[test]
    fn a_wide_desk_is_walked_once() {
        let mut rows: Vec<TrackRow> = (0..32).map(row).collect();
        rows.extend((100..200).map(bus));
        for (i, r) in rows.iter_mut().enumerate().take(32) {
            for k in 0..31 {
                r.sends.push(send(t(100 + ((i as u32 + k) % 100))));
            }
        }
        assert_eq!(rows.iter().map(|r| r.sends.len()).sum::<usize>(), 992);
        let topo = Topology::of_desk(&rows, &Specials::default(), &Routes::default());
        // Track i sends to buses 100 + (i..i + 31) % 100, so bus 150 hears
        // tracks 20 to 31 and nobody else.
        let up = topo.upstream(t(150));
        assert_eq!(up, (20..32).map(t).collect::<BTreeSet<_>>(), "every track that sends to bus 150 and nothing else");
        assert!(!topo.would_cycle(t(150), t(0)));
        assert!(topo.would_cycle(t(150), t(25)));
    }

    /// **A send aimed at a track that is no bus closes no loop** (W7-join's
    /// review): it is kept in the file, but nothing plays it
    /// (`desk_model::bus_sends`), so the picker must not grey a target for
    /// a loop through it. Made a bus, the same row's send is an edge again.
    #[test]
    fn a_send_to_a_track_that_is_no_bus_is_no_edge_until_the_track_is_a_bus() {
        // Track 1 sends to track 2, an audio row: the send is kept, plays
        // nowhere, and so a route from 2 back to 1 closes no loop.
        let mut rows = vec![row(1), row(2)];
        rows[0].sends.push(send(t(2)));
        let topo = Topology::of_desk(&rows, &Specials::default(), &routed(&[]));
        assert!(!topo.reaches(t(1), t(2)), "a send that sounds nowhere is an edge");
        assert!(!topo.would_cycle(t(2), t(1)), "a dead send greyed a route back");
        let topo = Topology::of_desk(&rows, &Specials::default(), &routed(&[(t(2), t(1))]));
        assert!(!topo.in_loop(t(2), t(1)), "a dead send marked a route as feedback");
        // Made a bus, the same send is an edge, and the route back loops.
        rows[1] = bus(2);
        let topo = Topology::of_desk(&rows, &Specials::default(), &routed(&[(t(2), t(1))]));
        assert!(topo.in_loop(t(2), t(1)), "a send to a bus is an edge");
        // The specials' sends read the same rule.
        let mut specials = Specials::default();
        specials.drums.sends.push(send(t(7)));
        let topo = Topology::of_desk(&[row(7)], &specials, &routed(&[]));
        assert!(!topo.reaches(Node::Drums, t(7)), "a special's dead send is an edge");
    }

}
