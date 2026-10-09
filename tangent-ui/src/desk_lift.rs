//! **Every desk this app ever wrote, lifted onto the frozen 32-wide shape.**
//!
//! The desk has had six shapes: eight channels with sixteen pads, eight
//! channels with the whole keyboard of pads, sixteen channels before and
//! after the drum bus, the 24-channel window of 2026-08-31 and today's
//! thirty-two. A settings file or a document says which one it was written
//! in only by its LENGTHS (the sends count is a strip count, the dense bay
//! count a seat map), and every reader that opens one has to land it on the
//! same place.
//!
//! **That place is the V32 shape, written down here as literals, and never
//! "today's".** The lifts used to read `recorder::CHANNELS`, which is right
//! for exactly as long as the desk does not change shape again: the day it
//! grew to 24 the sixteen-era lift kept working by luck and the 24-channel
//! window got no arm at all, so its buses and its master opened on channels
//! 24..26. A migration describes a move between two shapes the app HAD, and
//! both of those are history. The unlimited desk will have no compile-time
//! width at all, so a lift that reads one would have nothing to read.
//!
//! **One code path for the desk and for its provenance.** The bay lift is
//! generic over [`LiftBay`]: the readers run it over plugin paths, and
//! [`provenance_of`] runs the SAME function over legacy bay numbers, so the
//! answer to "which legacy file held this plugin's state" can never disagree
//! with where the reader put the plugin. A hand-written inverse would be a
//! second copy of six eras' arithmetic, and the first edit to one of them
//! would silently attach somebody's saved knobs to the wrong plugin.
//!
//! **The global shelf has its own map**, [`legacy_shelf_seat`]: the frozen
//! in-place renames the host ran on `insert-state-S-B.bin`, composed into a
//! pure function, so a new build can copy each legacy file to where the
//! renames would have put it without renaming anything.
//!
//! Nothing here reads a live capacity constant, and a scan in
//! `era_corpus` says so.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;

/// Channels in the frozen V32 shape.
pub const V32_CHANNELS: usize = 32;
/// Strips in the V32 shape: the channels, then drums, click and fx. The
/// master is strip 35 and is not one of these (it has no send).
pub const V32_STRIPS: usize = 35;
/// The drum bus, the click and the fx return's strip indices (and seats).
pub const V32_DRUMS: usize = 32;
pub const V32_CLICK: usize = 33;
pub const V32_FX: usize = 34;
/// The master's strip index, and its seat (`MASTER_SEAT`).
pub const V32_MASTER: usize = 35;
/// Bays per seat, in every era.
pub const V32_BAYS: usize = 3;
/// The seats every strip and the master own, as bays: `(35 + 1) * 3`.
/// What the v7 document head and the v21 settings head carry whole.
pub const V32_HEAD_BAYS: usize = 108;
/// Every bay of the V32 seat map: `4392 * 3`.
pub const V32_INSERT_SPACE: usize = 13_176;
/// The automation lane the V32 shape keeps the master's curves under: one
/// past the channels, `MASTER_LANE` as it stood.
pub const V32_MASTER_LANE: usize = 32;
/// Pads per drum lane, in every era since v18.
pub const PADS: usize = 128;
/// Where the legacy (channel 0) pad block starts in V32, one past the master.
pub const V32_PAD_BLOCK: usize = 36;
/// Where the per-lane pad blocks of channels 1.. start in V32, one past the
/// master's extra rack (seat 164).
pub const V32_EXTRA_PADS: usize = 165;

/// **Which shape a legacy desk was written in.** Append-only, and the text
/// names are what a document's `legacy_states` marker stores, so they are
/// frozen with the shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeskEra {
    /// Eight channels and sixteen pads (keys 36..51 at seats 12..27): the
    /// 84-bay desk from before the pad rescale.
    V8Pads16,
    /// Eight channels, 128 pads at seat 12: eleven sends.
    V8,
    /// Sixteen channels before a document carried the drum bus: eighteen
    /// sends, the click at strip 16.
    V16NoDrumBus,
    /// Sixteen channels with the drum bus, 4.82 through 5.0: nineteen sends.
    V16,
    /// The 24-channel window, d298cee..af9bda6 (2026-08-31): 27 sends.
    V24,
    /// Thirty-two channels: 35 sends. Nothing to lift.
    V32,
}

impl DeskEra {
    pub const ALL: [DeskEra; 6] = [
        Self::V8Pads16,
        Self::V8,
        Self::V16NoDrumBus,
        Self::V16,
        Self::V24,
        Self::V32,
    ];

    /// The marker text (`v8-pads16 v8 v16-no-drum-bus v16 v24 v32`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::V8Pads16 => "v8-pads16",
            Self::V8 => "v8",
            Self::V16NoDrumBus => "v16-no-drum-bus",
            Self::V16 => "v16",
            Self::V24 => "v24",
            Self::V32 => "v32",
        }
    }

    /// The inverse of [`Self::name`], exact: a marker this build cannot name
    /// is not guessed at.
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == s)
    }

    /// How many channels the era had.
    pub const fn channels(self) -> usize {
        match self {
            Self::V8Pads16 | Self::V8 => 8,
            Self::V16NoDrumBus | Self::V16 => 16,
            Self::V24 => 24,
            Self::V32 => 32,
        }
    }

    /// The sends length that is the era's fingerprint.
    pub const fn sends(self) -> usize {
        match self {
            Self::V8Pads16 | Self::V8 => 11,
            Self::V16NoDrumBus => 18,
            Self::V16 => 19,
            Self::V24 => 27,
            Self::V32 => 35,
        }
    }

    /// The era's whole seat map as dense bays, from its own arithmetic:
    /// the 84-bay pad sub-era; eight channels to the last pad (139); the
    /// drum-bus-less sixteen with no pad seats (19 strips); sixteen to its
    /// last extra pad (2068); the 24 window to 3100; V32 whole.
    pub const fn bays(self) -> usize {
        match self {
            Self::V8Pads16 => 84,
            Self::V8 => 140 * 3,
            Self::V16NoDrumBus => 19 * 3,
            Self::V16 => 2069 * 3,
            Self::V24 => 3101 * 3,
            Self::V32 => V32_INSERT_SPACE,
        }
    }
}

impl fmt::Display for DeskEra {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for DeskEra {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        Self::from_name(s).ok_or(())
    }
}

/// **The era a legacy desk was written in, from its lengths.**
///
/// Keyed on the sends count, the one array whose length has always been the
/// strip count and has never been trimmed. Anything this app never wrote
/// (no sends, a hand-edited length) reads as V32, which lifts nothing:
/// exactly what every reader did with an unknown length before.
///
/// **The 84-bay pad sub-era also needs eleven sends.** The readers used to
/// key the pad rescale on `inserts == 84` alone, and the settings writer
/// trimmed trailing empties until v21, so a sixteen-, 24- or 32-channel file
/// whose last plugin sat on seat 27 was 84 bays long and had channels 12..27
/// rescaled onto 48..63 at load. The sixteen-pad desk only ever had eight
/// channels, so the sends count settles it.
pub fn era_of(sends_len: usize, inserts_len: usize) -> DeskEra {
    match sends_len {
        11 if inserts_len == DeskEra::V8Pads16.bays() => DeskEra::V8Pads16,
        11 => DeskEra::V8,
        18 => DeskEra::V16NoDrumBus,
        19 => DeskEra::V16,
        27 => DeskEra::V24,
        _ => DeskEra::V32,
    }
}

/// **The era of a document's legacy `desk` object**, by the lengths
/// [`era_of`] keys on: the sends array, and the dense `inserts` array when
/// that is what the file holds (the fully sparse object is today's shape).
pub fn era_of_desk(desk: &Value) -> DeskEra {
    let sends = desk
        .get("sends")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let bays = match desk.get("inserts") {
        Some(Value::Array(a)) => a.len(),
        _ => 0,
    };
    era_of(sends, bays)
}

/// **Whether the settings reader has this era at all.** The drum-bus-less
/// sixteen is a document-only shape: the settings file grew its drum bus at
/// v14, in the eight-channel era, and never had eighteen sends, so the
/// settings reader leaves such a file exactly as it always did.
const fn lifts(era: DeskEra, settings_path: bool) -> bool {
    !matches!(era, DeskEra::V32) && !(settings_path && matches!(era, DeskEra::V16NoDrumBus))
}

/// The channel widening each era takes to reach V32: `(insert at, grow by)`,
/// in strips. Uniform because the seat map appends its classes in order
/// (channels, buses, master, pads), so one splice at the old channel count
/// moves every later seat by exactly the growth.
const fn widening(era: DeskEra) -> (usize, usize) {
    match era {
        DeskEra::V8Pads16 | DeskEra::V8 => (8, 24),
        DeskEra::V16NoDrumBus | DeskEra::V16 => (16, 16),
        DeskEra::V24 => (24, 8),
        DeskEra::V32 => (32, 0),
    }
}

/// **What a bay holds, as far as a lift cares**: it can be vacant, and it
/// moves. A plugin path for the readers, a legacy bay number for
/// [`provenance_of`].
pub trait LiftBay: Sized {
    fn vacant() -> Self;
}

impl LiftBay for String {
    fn vacant() -> Self {
        String::new()
    }
}

impl LiftBay for Option<u32> {
    fn vacant() -> Self {
        None
    }
}

/// **A legacy dense bay array, lifted onto the V32 seat map.**
///
/// The steps are the readers' own, in their order, each guarded by the
/// length it always checked:
///
/// - the sixteen-pad rescale (an 84-bay array grows to the eight-era 140
///   seats and pads 0..15 move to keys 36..51, highest first);
/// - the drum bus a drum-bus-less sixteen document lacked (three empty bays
///   at seat 16, only when the array is exactly that era's 19 strips);
/// - the channel widening at the era's channel count.
///
/// `settings_path` is the settings reader asking: it never had the
/// drum-bus-less era (see [`lifts`]).
pub fn lift_bays<B: LiftBay>(era: DeskEra, mut bays: Vec<B>, settings_path: bool) -> Vec<B> {
    if !lifts(era, settings_path) {
        return bays;
    }
    if era == DeskEra::V8Pads16 && bays.len() == DeskEra::V8Pads16.bays() {
        // Frozen eight-channel shape: strips 11, the pads after the master
        // at seat 12, three bays a seat.
        bays.resize_with(DeskEra::V8.bays(), B::vacant);
        for pad in (0..16usize).rev() {
            for bay in 0..V32_BAYS {
                let from = (12 + pad) * V32_BAYS + bay;
                let to = (12 + 36 + pad) * V32_BAYS + bay;
                bays[to] = std::mem::replace(&mut bays[from], B::vacant());
            }
        }
    }
    if era == DeskEra::V16NoDrumBus && bays.len() == DeskEra::V16NoDrumBus.bays() {
        let at = 16 * V32_BAYS;
        bays.splice(at..at, std::iter::repeat_with(B::vacant).take(V32_BAYS));
    }
    let (at, grow) = widening(era);
    let at = at * V32_BAYS;
    if at <= bays.len() {
        bays.splice(at..at, std::iter::repeat_with(B::vacant).take(grow * V32_BAYS));
    }
    bays
}

/// **A strip-indexed row (sends, colours, names), lifted onto V32's strips.**
///
/// `with_master` says whether the row carries the master (colours and names
/// do, at the end; sends do not). The drum-bus step inserts the drum bus at
/// strip 16 only into a row of exactly that era's length, the reader's own
/// guard; the widening inserts the new channels at the old channel count
/// when the row reaches that far.
pub fn lift_strip_rows<T: Clone>(
    era: DeskEra,
    mut rows: Vec<T>,
    vacant: T,
    with_master: bool,
    settings_path: bool,
) -> Vec<T> {
    if !lifts(era, settings_path) {
        return rows;
    }
    if era == DeskEra::V16NoDrumBus && rows.len() == 18 + usize::from(with_master) {
        rows.insert(16, vacant.clone());
    }
    let (at, grow) = widening(era);
    if at <= rows.len() {
        rows.splice(at..at, std::iter::repeat_n(vacant, grow));
    }
    rows
}

/// Bits below `at` keep their place; everything from `at` up steps by `by`.
const fn step_mask(mask: u64, at: usize, by: usize) -> u64 {
    let keep = mask & ((1u64 << at) - 1);
    let above = mask >> at;
    keep | (above << (at + by))
}

/// **A strip-space bitmask (mute, solo, PHASE), lifted onto V32's strips.**
///
/// Phase is the point: `adopt_desk` moved mute and solo and left
/// `strip_phase` where the sixteen-era file had it, so a phase-inverted drum
/// bus (strip 16 then) came back as an inverted channel 16 and the real drum
/// bus played the right way up. Every strip-space mask goes through here.
pub const fn lift_strip_mask(era: DeskEra, mask: u64, settings_path: bool) -> u64 {
    if !lifts(era, settings_path) {
        return mask;
    }
    let mask = if matches!(era, DeskEra::V16NoDrumBus) {
        step_mask(mask, 16, 1)
    } else {
        mask
    };
    let (at, grow) = widening(era);
    step_mask(mask, at, grow)
}

/// **The automation lane a document kept its master's curves under.**
///
/// `MASTER_LANE` was the channel count, so it moved every time the desk
/// grew: sixteen in every document stamped below 6 (the stamp that came
/// with the 24-channel desk), 24 in the 24-channel window, 32 since.
/// Version 6 alone cannot tell the window from 32, which is why the era is
/// asked: a 27-sends document stamped 6 kept its master at lane 24.
pub const fn master_auto_lane(version: u64, era: DeskEra) -> usize {
    if version < 6 {
        16
    } else if matches!(era, DeskEra::V24) {
        24
    } else {
        V32_MASTER_LANE
    }
}

/// **Where a legacy document's curve lands in the V32 shape**, or `None`
/// when it is dropped. The master's lane ([`master_auto_lane`]) becomes
/// [`V32_MASTER_LANE`]; a channel below it keeps its lane; anything at or
/// past the era's width named no strip the document had, and a level landing
/// on the wrong instrument is worse than one that does not land.
pub const fn lift_curve_lane(version: u64, era: DeskEra, lane: usize) -> Option<usize> {
    let master = master_auto_lane(version, era);
    if lane == master {
        Some(V32_MASTER_LANE)
    } else if lane < master {
        Some(lane)
    } else {
        None
    }
}

/// **The V32 seat of drum lane `ch`'s pad `pad`** (`recorder::pad_rack_index`
/// at 32 channels, frozen): channel 0 keeps the legacy block after the
/// master, every other channel its own block after the master's extra rack.
pub const fn v32_pad_seat(ch: usize, pad: usize) -> usize {
    if ch == 0 {
        V32_PAD_BLOCK + pad
    } else {
        V32_EXTRA_PADS + (ch - 1) * PADS + pad
    }
}

/// **Which legacy bay each lifted bay came from.** Keys are V32 flat bays
/// (`seat * 3 + bay`), values the era's own flat bays; a V32 bay that the
/// lift made (a widened channel, the drum bus) has no entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub era: DeskEra,
    pub to_legacy: BTreeMap<u32, u32>,
}

impl Provenance {
    /// The legacy flat bay that held what now sits at V32 flat bay `lifted`.
    pub fn legacy_bay(&self, lifted: u32) -> Option<u32> {
        self.to_legacy.get(&lifted).copied()
    }
}

/// The lift run over `len` legacy bay numbers: [`lift_bays`] itself, never
/// an inverse of it.
fn provenance_over(era: DeskEra, len: usize) -> Provenance {
    let len = u32::try_from(len.min(V32_INSERT_SPACE)).unwrap_or(0);
    let tags: Vec<Option<u32>> = (0..len).map(Some).collect();
    let to_legacy = lift_bays(era, tags, false)
        .into_iter()
        .enumerate()
        .filter_map(|(at, tag)| Some((u32::try_from(at).ok()?, tag?)))
        .collect();
    Provenance { era, to_legacy }
}

/// **The provenance of an era's whole seat map.** What a document's durable
/// `legacy_states` marker resolves to once its dense array is gone.
pub fn provenance_of(era: DeskEra) -> Provenance {
    provenance_over(era, era.bays())
}

/// **The provenance of one document's desk, exactly as its reader lifted
/// it**: the era from the file's lengths, and the lift run over the file's
/// OWN dense length, so a short array is mapped the way it was read.
///
/// `None` for a document with no desk (v1..v3: the live desk stands) and
/// for a format-2 desk without a `legacy_states` marker (nothing legacy
/// left to find). A format-2 desk with a marker answers that era's whole
/// map; a V32 desk, or today's fully sparse one, answers the identity.
pub fn provenance_of_document(doc: &Value) -> Option<Provenance> {
    let desk = doc.get("desk")?.as_object()?;
    if desk.get("format").and_then(Value::as_u64) == Some(2) {
        let era = desk
            .get("legacy_states")
            .and_then(Value::as_str)
            .and_then(DeskEra::from_name)?;
        return Some(provenance_of(era));
    }
    let desk = doc.get("desk")?;
    let era = era_of_desk(desk);
    match desk.get("inserts") {
        Some(Value::Array(a)) if era != DeskEra::V32 => Some(provenance_over(era, a.len())),
        _ => Some(provenance_of(era)),
    }
}

/// **The in-place renames of the global shelf**, one bit each. Which ones a
/// settings load raised is a SET, not a stamp: v13, v14 and v18 come from
/// the version, v19, v20 and v24 from the desk's sends length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ShelfShuffles(u8);

impl ShelfShuffles {
    /// The BACKING strip retired (pre-5.0): seat 8 dropped, 9..=11 down one.
    pub const V13: Self = Self(1);
    /// The drum bus at 8 (pre-5.0): 8..=10 up one.
    pub const V14: Self = Self(1 << 1);
    /// The pads to 128: 12..=27 to 48..=63.
    pub const V18: Self = Self(1 << 2);
    /// Eight channels to sixteen: 8..=139 up eight.
    pub const V19: Self = Self(1 << 3);
    /// Sixteen channels to 32: 16..=2068 up sixteen.
    pub const V20: Self = Self(1 << 4);
    /// The 24-channel window to 32: 24..=3100 up eight.
    pub const V24: Self = Self(1 << 5);
    /// None of them.
    pub const NONE: Self = Self(0);

    /// Each shuffle with its marker name, in composition order.
    pub const NAMED: [(Self, &'static str); 6] = [
        (Self::V13, "v13"),
        (Self::V14, "v14"),
        (Self::V18, "v18"),
        (Self::V19, "v19"),
        (Self::V20, "v20"),
        (Self::V24, "v24"),
    ];

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The names of the shuffles in the set, in composition order: what the
    /// pending marker writes.
    pub fn names(self) -> Vec<&'static str> {
        Self::NAMED
            .into_iter()
            .filter(|(s, _)| self.contains(*s))
            .map(|(_, n)| n)
            .collect()
    }

    /// The inverse of [`Self::names`]; `None` on a name this build does not
    /// know, so a marker from a later build is never half-read.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let mut set = Self::NONE;
        for n in names {
            let (s, _) = Self::NAMED.into_iter().find(|(_, k)| *k == n)?;
            set.insert(s);
        }
        Some(set)
    }
}

impl std::ops::BitOr for ShelfShuffles {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// One frozen rename step as a pure map: `from..=to` moves by `by` (up or
/// down), and the `by` seats it lands past the top overwrite whatever an
/// up-move finds there.
const fn shift_seat(seat: usize, from: usize, to: usize, up: usize) -> Option<usize> {
    if seat >= from && seat <= to {
        Some(seat + up)
    } else if seat > to && seat <= to + up {
        None
    } else {
        Some(seat)
    }
}

/// **Where the frozen in-place renames would have put a global-shelf file**,
/// or `None` where they deleted or overwrote it.
///
/// Applied to the seat; the bay never moves. Composition order v13, v14,
/// v18, v19, then v20 or v24 (exclusive), the order the host ran them in.
/// Each step is the host's own rename loop read as a function, including
/// what that loop destroyed: v13 deletes seat 8, and an up-move run highest
/// first overwrites the seats it lands past its top (v14's 11, v18's 48..=63,
/// v19's 140..=147, v20's 2069..=2084, v24's 3101..=3108). Those are seats
/// the era itself never had, so a real shelf of that era holds nothing there;
/// a file that does belongs to a later build, and the conversion leaves it
/// alone rather than guess.
pub const fn legacy_shelf_seat(set: ShelfShuffles, seat: usize) -> Option<usize> {
    let mut s = seat;
    if set.contains(ShelfShuffles::V13) {
        if s == 8 {
            return None;
        }
        if s >= 9 && s <= 11 {
            s -= 1;
        }
    }
    if set.contains(ShelfShuffles::V14) {
        match shift_seat(s, 8, 10, 1) {
            Some(n) => s = n,
            None => return None,
        }
    }
    if set.contains(ShelfShuffles::V18) {
        // Not a contiguous shift: 12..=27 jump to 48..=63, which they
        // overwrite, and 28..=47 stay.
        if s >= 12 && s <= 27 {
            s += 36;
        } else if s >= 48 && s <= 63 {
            return None;
        }
    }
    if set.contains(ShelfShuffles::V19) {
        match shift_seat(s, 8, 139, 8) {
            Some(n) => s = n,
            None => return None,
        }
    }
    if set.contains(ShelfShuffles::V20) {
        match shift_seat(s, 16, 2068, 16) {
            Some(n) => s = n,
            None => return None,
        }
    } else if set.contains(ShelfShuffles::V24) {
        match shift_seat(s, 24, 3100, 8) {
            Some(n) => s = n,
            None => return None,
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marker names round-trip and nothing else parses.
    #[test]
    fn every_era_name_round_trips_and_nothing_else_parses() {
        for e in DeskEra::ALL {
            assert_eq!(DeskEra::from_name(e.name()), Some(e));
            assert_eq!(e.to_string().parse::<DeskEra>(), Ok(e));
        }
        for bad in ["", "V16", "v16 ", "v-16", "v40", "v16-no-drum-bus!"] {
            assert_eq!(DeskEra::from_name(bad), None, "{bad:?} parsed");
        }
    }

    /// **The fingerprints, literally.** The pad sub-era needs both lengths;
    /// an 84-bay array beside any other sends count is that count's era.
    #[test]
    fn the_era_is_the_sends_length_and_the_pad_sub_era_needs_both() {
        assert_eq!(era_of(11, 84), DeskEra::V8Pads16);
        assert_eq!(era_of(11, 420), DeskEra::V8);
        assert_eq!(era_of(11, 36), DeskEra::V8);
        assert_eq!(era_of(18, 57), DeskEra::V16NoDrumBus);
        assert_eq!(era_of(19, 84), DeskEra::V16);
        assert_eq!(era_of(27, 84), DeskEra::V24);
        assert_eq!(era_of(35, 84), DeskEra::V32);
        assert_eq!(era_of(0, 84), DeskEra::V32);
        assert_eq!(era_of(64, 0), DeskEra::V32);
        for e in DeskEra::ALL {
            assert_eq!(era_of(e.sends(), e.bays()), e, "{e}'s own lengths");
        }
    }

    /// **Each era's widening, at literal seats.** A bay at the era's drum
    /// bus, master and first pad lands on V32's.
    #[test]
    fn each_eras_buses_master_and_pads_land_on_v32s_seats() {
        // (era, legacy seat, V32 seat): drums, master, ch0 pad 0, and the
        // master-extra where the era had one.
        let cases: [(DeskEra, usize, usize); 12] = [
            (DeskEra::V8, 8, 32),
            (DeskEra::V8, 11, 35),
            (DeskEra::V8, 12, 36),
            (DeskEra::V8, 139, 163),
            (DeskEra::V16, 16, 32),
            (DeskEra::V16, 19, 35),
            (DeskEra::V16, 148, 164),
            (DeskEra::V16, 2068, 2084),
            (DeskEra::V24, 24, 32),
            (DeskEra::V24, 27, 35),
            (DeskEra::V24, 156, 164),
            (DeskEra::V24, 3100, 3108),
        ];
        for (era, from, to) in cases {
            let p = provenance_of(era);
            for b in 0..3u32 {
                let lifted = (to * 3) as u32 + b;
                assert_eq!(
                    p.legacy_bay(lifted),
                    Some((from * 3) as u32 + b),
                    "{era}: seat {from} did not land on {to}"
                );
            }
        }
        // The pad sub-era: pad 2 at seat 14 goes to key 38's seat, 50,
        // then the widening carries it to 74.
        let p = provenance_of(DeskEra::V8Pads16);
        assert_eq!(p.legacy_bay(74 * 3 + 1), Some(14 * 3 + 1));
        // The drum-bus-less sixteen: the click at 16 goes past the bus to
        // 17, then to 33; the bus itself came from nowhere.
        let p = provenance_of(DeskEra::V16NoDrumBus);
        assert_eq!(p.legacy_bay(33 * 3), Some(16 * 3));
        assert_eq!(p.legacy_bay(32 * 3), None, "the drum bus came from nowhere");
        // Channels never move, in any era.
        for e in DeskEra::ALL {
            let p = provenance_of(e);
            for b in 0..(e.channels() * 3) as u32 {
                assert_eq!(p.legacy_bay(b), Some(b), "{e}: channel bay {b} moved");
            }
            for b in (e.channels() * 3) as u32..(V32_CHANNELS * 3) as u32 {
                assert_eq!(p.legacy_bay(b), None, "{e}: a widened channel has a past");
            }
        }
    }

    /// **The settings reader never had the drum-bus-less era**, and leaves
    /// an eighteen-sends file exactly as it always did.
    #[test]
    fn the_settings_path_has_no_drum_bus_less_era() {
        let rows = vec![1, 2, 3];
        assert_eq!(lift_strip_rows(DeskEra::V16NoDrumBus, rows.clone(), 0, false, true), rows);
        assert_eq!(lift_strip_mask(DeskEra::V16NoDrumBus, 1 << 17, true), 1 << 17);
        let bays = vec![String::from("x"); 57];
        assert_eq!(lift_bays(DeskEra::V16NoDrumBus, bays.clone(), true), bays);
        // And the document reader does.
        assert_eq!(lift_strip_mask(DeskEra::V16NoDrumBus, 1 << 17, false), 1 << (18 + 16));
    }

    /// **Phase rides the same lift as mute and solo.** The sixteen-era drum
    /// bus is strip 16 and the master strip 19; V32 has them at 32 and 35.
    #[test]
    fn a_sixteen_era_phase_bit_lands_on_v32s_strip() {
        assert_eq!(lift_strip_mask(DeskEra::V16, (1 << 16) | (1 << 19) | 0b101, false),
            (1 << 32) | (1 << 35) | 0b101);
        assert_eq!(lift_strip_mask(DeskEra::V24, (1 << 24) | (1 << 27), false), (1 << 32) | (1 << 35));
        assert_eq!(lift_strip_mask(DeskEra::V8, (1 << 8) | (1 << 11), true), (1 << 32) | (1 << 35));
        assert_eq!(lift_strip_mask(DeskEra::V32, u64::MAX, false), u64::MAX);
    }

    /// **The master's curves by era**, and nothing past an era's width.
    #[test]
    fn the_master_lane_follows_the_version_and_the_era() {
        assert_eq!(master_auto_lane(5, DeskEra::V16), 16);
        assert_eq!(master_auto_lane(0, DeskEra::V32), 16);
        assert_eq!(master_auto_lane(6, DeskEra::V24), 24);
        assert_eq!(master_auto_lane(6, DeskEra::V32), 32);
        assert_eq!(master_auto_lane(7, DeskEra::V32), 32);
        assert_eq!(lift_curve_lane(5, DeskEra::V16, 16), Some(32));
        assert_eq!(lift_curve_lane(5, DeskEra::V16, 15), Some(15));
        assert_eq!(lift_curve_lane(5, DeskEra::V16, 17), None);
        assert_eq!(lift_curve_lane(6, DeskEra::V24, 24), Some(32));
        assert_eq!(lift_curve_lane(6, DeskEra::V24, 23), Some(23));
        assert_eq!(lift_curve_lane(6, DeskEra::V24, 30), None);
        assert_eq!(lift_curve_lane(7, DeskEra::V32, 32), Some(32));
        assert_eq!(lift_curve_lane(7, DeskEra::V32, 31), Some(31));
        assert_eq!(lift_curve_lane(7, DeskEra::V32, 33), None);
    }

    /// The frozen pad map, against the V32 table (Appendix A).
    #[test]
    fn the_v32_pad_seats_are_the_frozen_table() {
        assert_eq!(v32_pad_seat(0, 0), 36);
        assert_eq!(v32_pad_seat(0, 127), 163);
        assert_eq!(v32_pad_seat(1, 0), 165);
        assert_eq!(v32_pad_seat(31, 127), 4132);
    }

    /// The shuffle-set names round-trip, and an unknown one refuses the set.
    #[test]
    fn shuffle_sets_round_trip_through_their_names() {
        let all = ShelfShuffles::NAMED.iter().fold(ShelfShuffles::NONE, |a, (s, _)| a | *s);
        assert_eq!(all.names(), vec!["v13", "v14", "v18", "v19", "v20", "v24"]);
        assert_eq!(ShelfShuffles::from_names(all.names()), Some(all));
        let some = ShelfShuffles::V19 | ShelfShuffles::V20;
        assert_eq!(ShelfShuffles::from_names(some.names()), Some(some));
        assert_eq!(ShelfShuffles::from_names(["v19", "v40"]), None);
        assert!(ShelfShuffles::NONE.is_empty());
        assert_eq!(ShelfShuffles::from_names([]), Some(ShelfShuffles::NONE));
    }
}
