//! **The desk's shape, worked out once**: which rows are drawn and in what
//! order, where each one ranks, what class it is, which folder holds it,
//! how many children a folder has and how many clips a row carries.
//!
//! Every view used to answer these by walking the whole desk for each strip
//! it drew. `folder_of` walks up from a row to find its folder, so asking
//! whether each of thirty-two strips is hidden was a quadratic walk; the
//! mixer's layout ranked every column to find the dozen it would place; and
//! a strip's "is this lane used" asked every clip in the song. That was
//! fine at sixteen strips and is not a shape that survives a desk that
//! grows by appending (docs/design/unlimited-tracks.md, section 10).
//!
//! So the facts are gathered here, in one O(rows + clips) pass, and the
//! views read them: building a view, laying it out, hit-testing it and
//! describing it to a screen reader are O(visible) once the index stands.
//!
//! **Pure.** It is built from [`RowFacts`] and per-row clip counts that the
//! app gathers, and knows nothing of settings, documents or egui, so its
//! rules are pinned against frozen copies of the app's own functions in the
//! tests below rather than through an app.
//!
//! **When it is rebuilt.** Its [`DeskIndex::generation`] moves whenever its
//! inputs move, and undo, redo and a load force a new generation outright.
//! The app does not look on every ask: an ask is the cached index unless
//! something marked it (the roads every row, clip, folder, kind, plugin,
//! route or kit edit passes through, and once at the top of each frame).
//! A marked ask gathers the facts and compares them against the ones the
//! standing index was built from, exactly rather than by a hash, because a
//! collision here would draw a desk that does not exist, and rebuilds only
//! on a difference. See `TangentApp::desk_index`.

use crate::recorder::ChannelKind;

/// What the app knows about one row, the inputs of the index.
///
/// The folder flags are the RAW stored flags. The index applies the
/// positional rules itself (a member flag with no folder above it is a
/// stale flag and makes no member), exactly as the app's `folder_of`,
/// `in_folder`, `folder_shut` and `lane_hidden` do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowFacts {
    /// The row's is-a-folder flag.
    pub folder: bool,
    /// The row's lives-in-the-folder-above flag.
    pub in_folder: bool,
    /// The row's folder-is-shut flag. Meaningless off a folder.
    pub shut: bool,
    /// What feeds it live, if anything.
    pub kind: Option<ChannelKind>,
    /// Added as a bare track: used, with no kind chosen yet.
    pub added: bool,
    /// Something keeps the row from being vacant without making it a
    /// counted track: a chain plugin, a curve, a freeze, a kit, or either
    /// end of a route or a sidechain key (section 4's Vacant column, and
    /// W2-desk-format's `holds`). Pedal-only residue is NOT residue
    /// (section 4: it stays Vacant and is never reused).
    pub residue: bool,
}

/// **What a row is**, for drawing, counting and the engine (section 4).
///
/// One type, `desk_model::RowClass`, re-exported: this lane declared its
/// own while W2-desk-format wrote that file in parallel, and the two were
/// joined at the wave-2 merge. Section 4's table has a gap both lanes
/// filled the same way: a row with no kind that holds a curve, a kit, a
/// freeze, a chain plugin or a route or key end is a Placeholder, never
/// Vacant, so a sounding plugin or a route's end is never hidden.
pub use crate::desk_model::RowClass;

/// The cached shape of the desk. See the module header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskIndex {
    generation: u64,
    facts: Vec<RowFacts>,
    clips: Vec<u32>,
    /// The drawn rows, top to bottom (the arrangement) or left to right
    /// (the desk): every row not inside a shut folder.
    shown: Vec<u32>,
    /// Each row's place in `shown`, or `u32::MAX` for a hidden row.
    rank: Vec<u32>,
    parent: Vec<Option<u32>>,
    children: Vec<u32>,
    class: Vec<RowClass>,
    counted: usize,
}

const HIDDEN: u32 = u32::MAX;

impl DeskIndex {
    /// Work the shape out from the rows' facts and their clip counts.
    ///
    /// `clips[row]` is how many clips stand on `row`; a shorter list reads
    /// as zero past its end, and counts past the facts are ignored (a clip
    /// on a lane the desk does not reach is drawn nowhere, as today).
    pub fn build(generation: u64, facts: Vec<RowFacts>, mut clips: Vec<u32>) -> Self {
        let n = facts.len();
        clips.resize(n, 0);
        // **The folder above each row, in one pass.** `folder_of` walks up
        // from a member through the members above it until it meets a
        // folder; the row above has already been answered, so each row's
        // answer is its neighbour's: the neighbour itself when it is a
        // folder, the neighbour's folder when it is a member, nobody
        // otherwise. Row 0 has nobody above it.
        let mut parent: Vec<Option<u32>> = Vec::with_capacity(n);
        for row in 0..n {
            let p = if row == 0 || !facts[row].in_folder {
                None
            } else if facts[row - 1].folder {
                Some((row - 1) as u32)
            } else if facts[row - 1].in_folder {
                parent[row - 1]
            } else {
                None
            };
            parent.push(p);
        }
        let class: Vec<RowClass> = (0..n)
            .map(|row| {
                let f = &facts[row];
                // A bus first, whatever else it carries: it is drawn and
                // laned like a track and never counted (`DeskModel::class`'s
                // order, which this mirrors).
                if f.kind == Some(ChannelKind::Bus) {
                    RowClass::Bus
                } else if f.kind.is_some() || f.added || f.folder || clips[row] > 0 {
                    RowClass::Counted
                } else if f.in_folder || f.residue {
                    RowClass::Placeholder
                } else {
                    RowClass::Vacant
                }
            })
            .collect();
        // **Drawn: every row not inside a shut folder and not Vacant**
        // (design section 4). A vacant row is no track and holds nothing
        // anybody made, so the views leave it out: a legacy desk's thirty
        // empty rows, and a pedalboard an older build left on a removed
        // track's row, take no strip and no lane. The one place a new
        // track goes is the trailing add after the last drawn row.
        let shut = |row: usize| facts[row].folder && facts[row].shut;
        let mut shown = Vec::with_capacity(n);
        let mut rank = vec![HIDDEN; n];
        for row in 0..n {
            let hidden = parent[row].is_some_and(|f| shut(f as usize)) || class[row] == RowClass::Vacant;
            if !hidden {
                rank[row] = shown.len() as u32;
                shown.push(row as u32);
            }
        }
        // `folder_children`: the run under a folder of member-flagged rows
        // that are not folders themselves. Counted backwards so each row
        // knows the length of the run that starts below it.
        let mut children = vec![0u32; n];
        let mut run = 0u32;
        for row in (0..n).rev() {
            if facts[row].folder {
                children[row] = run;
            }
            run = if facts[row].in_folder && !facts[row].folder { run + 1 } else { 0 };
        }
        let counted = class.iter().filter(|c| **c == RowClass::Counted).count();
        Self { generation, facts, clips, shown, rank, parent, children, class, counted }
    }

    /// Whether this index was built from exactly these inputs. The app's
    /// staleness test: equal inputs, equal index.
    pub fn built_from(&self, facts: &[RowFacts], clips: &[u32]) -> bool {
        if self.facts != facts {
            return false;
        }
        // A trailing run of zeros is the same as no entry: `build` pads.
        let (a, b) = (&self.clips[..], clips);
        let common = a.len().min(b.len());
        a[..common] == b[..common]
            && a[common..].iter().all(|c| *c == 0)
            && b[common..].iter().all(|c| *c == 0)
    }

    /// Which build this is. Moves on every change to the inputs and on
    /// every undo, redo and load.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many rows the desk has, drawn or not.
    pub fn extent(&self) -> usize {
        self.facts.len()
    }

    /// The drawn rows in order.
    pub fn shown(&self) -> &[u32] {
        &self.shown
    }

    /// How many rows are drawn: what a scroll range answers to.
    pub fn shown_len(&self) -> usize {
        self.shown.len()
    }

    /// A row's place among the drawn rows, or `None` for a hidden row (or a
    /// row past the desk).
    pub fn rank_of(&self, row: usize) -> Option<usize> {
        self.rank.get(row).filter(|r| **r != HIDDEN).map(|r| *r as usize)
    }

    /// The drawn row at a rank.
    pub fn row_at(&self, rank: usize) -> Option<usize> {
        self.shown.get(rank).map(|r| *r as usize)
    }

    /// The rank of the first drawn row at or below `row`; `shown_len()` when
    /// there is none. How a scroll held as a row becomes a window.
    pub fn rank_at_or_after(&self, row: usize) -> usize {
        self.shown.partition_point(|r| (*r as usize) < row)
    }

    /// The drawn rows from `first` (a rank), at most `len` of them.
    pub fn window(&self, first: usize, len: usize) -> &[u32] {
        let a = first.min(self.shown.len());
        let b = first.saturating_add(len).min(self.shown.len());
        &self.shown[a..b]
    }

    /// Whether a row is not drawn: inside a shut folder, or Vacant.
    pub fn hidden(&self, row: usize) -> bool {
        self.rank.get(row).is_some_and(|r| *r == HIDDEN)
    }

    /// The folder a row lives in (the app's `folder_of`).
    pub fn parent(&self, row: usize) -> Option<usize> {
        self.parent.get(row).copied().flatten().map(|p| p as usize)
    }

    /// Whether a row is a folder.
    pub fn is_folder(&self, row: usize) -> bool {
        self.facts.get(row).is_some_and(|f| f.folder)
    }

    /// Whether a row really is inside a folder: its flag, and a folder above
    /// it (the app's `in_folder`).
    pub fn in_folder(&self, row: usize) -> bool {
        self.facts.get(row).is_some_and(|f| f.in_folder) && self.parent(row).is_some()
    }

    /// Whether a row is a SHUT folder (the app's `folder_shut`).
    pub fn shut(&self, row: usize) -> bool {
        self.facts.get(row).is_some_and(|f| f.folder && f.shut)
    }

    /// How many direct children a folder holds (`folder_children().len()`).
    pub fn child_count(&self, row: usize) -> usize {
        self.children.get(row).map_or(0, |c| *c as usize)
    }

    /// How many clips stand on a row.
    pub fn clip_count(&self, row: usize) -> usize {
        self.clips.get(row).map_or(0, |c| *c as usize)
    }

    /// What a row is. A row past the desk is Vacant.
    pub fn class(&self, row: usize) -> RowClass {
        self.class.get(row).copied().unwrap_or(RowClass::Vacant)
    }

    /// How many rows are counted tracks.
    pub fn counted(&self) -> usize {
        self.counted
    }

    /// **A synthetic desk of `rows` rows** for the bounds tests and the perf
    /// rungs: every row a MIDI track, a folder of three every `folder_every`
    /// rows (shut when `shut`), and one clip per row.
    #[cfg(test)]
    pub(crate) fn synthetic(rows: usize, folder_every: usize, shut: bool) -> Self {
        let facts: Vec<RowFacts> = (0..rows)
            .map(|r| {
                let at = if folder_every == 0 { usize::MAX } else { r % folder_every };
                RowFacts {
                    folder: at == 0,
                    in_folder: (1..=3).contains(&at),
                    shut: at == 0 && shut,
                    kind: Some(ChannelKind::Midi),
                    ..RowFacts::default()
                }
            })
            .collect();
        Self::build(1, facts, vec![1; rows])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The app's own folder rules, frozen**, so the index is pinned to
    /// them rather than to a re-derivation of itself: `folder_of`,
    /// `in_folder`, `folder_shut`, `lane_hidden` and `folder_children` as
    /// `app.rs` wrote them at this lane's base, over a list of flags.
    mod frozen {
        use super::RowFacts;

        pub fn folder_of(f: &[RowFacts], ch: usize) -> Option<usize> {
            if ch == 0 || !f.get(ch).is_some_and(|r| r.in_folder) {
                return None;
            }
            for n in (0..ch).rev() {
                if f[n].folder {
                    return Some(n);
                }
                if !f[n].in_folder {
                    return None;
                }
            }
            None
        }

        pub fn in_folder(f: &[RowFacts], ch: usize) -> bool {
            f.get(ch).is_some_and(|r| r.in_folder) && folder_of(f, ch).is_some()
        }

        pub fn folder_shut(f: &[RowFacts], ch: usize) -> bool {
            f.get(ch).is_some_and(|r| r.folder) && f[ch].shut
        }

        pub fn lane_hidden(f: &[RowFacts], ch: usize) -> bool {
            folder_of(f, ch).is_some_and(|p| folder_shut(f, p))
        }

        pub fn folder_children(f: &[RowFacts], ch: usize) -> usize {
            if !f.get(ch).is_some_and(|r| r.folder) {
                return 0;
            }
            let mut out = 0;
            for r in &f[ch + 1..] {
                if !r.in_folder || r.folder {
                    break;
                }
                out += 1;
            }
            out
        }
    }

    /// A small deterministic generator, so the property walk needs no crate.
    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        *seed >> 33
    }

    fn random_desk(seed: &mut u64, rows: usize) -> (Vec<RowFacts>, Vec<u32>) {
        let facts = (0..rows)
            .map(|_| {
                let bits = lcg(seed);
                RowFacts {
                    folder: bits & 7 == 0,
                    in_folder: bits & 2 != 0,
                    shut: bits & 16 != 0,
                    kind: match (bits >> 5) & 3 {
                        0 => Some(ChannelKind::Midi),
                        1 => Some(ChannelKind::Audio),
                        2 => Some(ChannelKind::Bus),
                        _ => None,
                    },
                    added: bits & 128 != 0,
                    residue: bits & 256 != 0,
                }
            })
            .collect();
        let clips = (0..rows).map(|_| (lcg(seed) % 3) as u32 * u32::from(lcg(seed) % 4 == 0)).collect();
        (facts, clips)
    }

    /// **Every cached fact equals the app's own rule**, on a thousand random
    /// desks at the widths the design names, stale member flags, folders in
    /// a run and shut folders over shut folders included.
    #[test]
    fn the_index_answers_what_the_apps_folder_rules_answer() {
        let mut seed = 0x5eed_u64;
        for rows in [1usize, 2, 32, 33, 64, 65, 200] {
            for _ in 0..150 {
                let (facts, clips) = random_desk(&mut seed, rows);
                let ix = DeskIndex::build(7, facts.clone(), clips.clone());
                let mut rank = 0;
                for row in 0..rows {
                    let f = facts[row];
                    let vacant =
                        !(f.kind.is_some() || f.added || f.folder || clips[row] > 0 || f.in_folder || f.residue);
                    // A vacant row is not drawn either (section 4).
                    let hidden = frozen::lane_hidden(&facts, row) || vacant;
                    assert_eq!(ix.hidden(row), hidden, "row {row} of {facts:?}");
                    assert_eq!(ix.parent(row), frozen::folder_of(&facts, row), "row {row}");
                    assert_eq!(ix.in_folder(row), frozen::in_folder(&facts, row), "row {row}");
                    assert_eq!(ix.shut(row), frozen::folder_shut(&facts, row), "row {row}");
                    assert_eq!(ix.child_count(row), frozen::folder_children(&facts, row));
                    assert_eq!(ix.clip_count(row), clips[row] as usize);
                    if hidden {
                        assert_eq!(ix.rank_of(row), None);
                    } else {
                        assert_eq!(ix.rank_of(row), Some(rank));
                        assert_eq!(ix.row_at(rank), Some(row));
                        rank += 1;
                    }
                    // Section 4's classes, from the facts directly.
                    let f = facts[row];
                    let counted = f.kind.is_some() || f.added || f.folder || clips[row] > 0;
                    let want = if f.kind == Some(ChannelKind::Bus) {
                        RowClass::Bus
                    } else if counted {
                        RowClass::Counted
                    } else if f.in_folder || f.residue {
                        RowClass::Placeholder
                    } else {
                        RowClass::Vacant
                    };
                    assert_eq!(ix.class(row), want, "row {row}");
                }
                assert_eq!(ix.shown_len(), rank);
                assert_eq!(
                    ix.counted(),
                    (0..rows).filter(|r| ix.class(*r) == RowClass::Counted).count()
                );
                assert_eq!(ix.generation(), 7);
            }
        }
    }

    /// **The classes of section 4, on rows named for what they are.**
    #[test]
    fn row_classes_match_the_design_table() {
        let row = |f: RowFacts| f;
        let facts = vec![
            // 0: nothing at all.
            row(RowFacts::default()),
            // 1: a folder, which counts.
            row(RowFacts { folder: true, ..RowFacts::default() }),
            // 2: only the in-folder flag: today's indented plus.
            row(RowFacts { in_folder: true, ..RowFacts::default() }),
            // 3: a MIDI track.
            row(RowFacts { kind: Some(ChannelKind::Midi), ..RowFacts::default() }),
            // 4: a bare added track.
            row(RowFacts { added: true, ..RowFacts::default() }),
            // 5: clips and nothing else: a clip lane counts.
            row(RowFacts::default()),
            // 6: a chain plugin left on a removed track: drawn, not counted.
            row(RowFacts { residue: true, ..RowFacts::default() }),
            // 7: a bus: drawn and laned, and never counted, whatever it
            // carries (a bus holds no clips, and a stray count of them
            // would still not make it a track).
            row(RowFacts { kind: Some(ChannelKind::Bus), added: true, ..RowFacts::default() }),
        ];
        let ix = DeskIndex::build(1, facts, vec![0, 0, 0, 0, 0, 2, 0, 1]);
        use RowClass::*;
        let got: Vec<RowClass> = (0..8).map(|r| ix.class(r)).collect();
        assert_eq!(got, vec![Vacant, Counted, Placeholder, Counted, Counted, Counted, Placeholder, Bus]);
        assert_eq!(ix.counted(), 4);
        assert!(!ix.hidden(7), "a bus is drawn");
        // Past the desk is nothing.
        assert_eq!(ix.class(99), Vacant);
    }

    /// **A shut folder's children leave the drawn order and nothing else
    /// does**, and the window and the rank lookup agree about it.
    #[test]
    fn a_shut_folder_takes_its_children_out_of_the_drawn_order() {
        let ix = DeskIndex::synthetic(12, 4, true);
        // Folders at 0, 4, 8, each with three children, all shut.
        assert_eq!(ix.shown(), &[0, 4, 8]);
        assert_eq!(ix.rank_at_or_after(1), 1, "row 1 is hidden; the next drawn is row 4");
        assert_eq!(ix.rank_at_or_after(4), 1);
        assert_eq!(ix.rank_at_or_after(9), 3);
        assert_eq!(ix.window(1, 5), &[4, 8]);
        assert_eq!(ix.child_count(4), 3);
        let open = DeskIndex::synthetic(12, 4, false);
        assert_eq!(open.shown_len(), 12);
        assert_eq!(open.window(10, 5), &[10, 11]);
    }

    /// **Equal inputs, equal index; any difference is a rebuild.** A
    /// trailing zero clip count is no difference: a lane with none and a
    /// lane past the list are the same lane.
    #[test]
    fn built_from_is_exact_about_its_inputs() {
        let (facts, clips) = random_desk(&mut 99, 40);
        let ix = DeskIndex::build(3, facts.clone(), clips.clone());
        assert!(ix.built_from(&facts, &clips));
        let mut longer = clips.clone();
        longer.extend([0, 0]);
        assert!(ix.built_from(&facts, &longer));
        let mut moved = clips.clone();
        moved[39] += 1;
        assert!(!ix.built_from(&facts, &moved));
        let mut flipped = facts.clone();
        flipped[17].shut = !flipped[17].shut;
        assert!(!ix.built_from(&flipped, &clips));
        assert!(!ix.built_from(&facts[..39], &clips));
    }
}
