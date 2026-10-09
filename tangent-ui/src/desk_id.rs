//! Identity on the desk: what a track, a rack and a plugin bay ARE, as
//! opposed to where they happen to sit.
//!
//! **Why this exists.** Every name the app has ever written down for a plugin
//! is a position: `insert-state-<seat>-<bay>.bin`, `strip_inserts[seat * 3 +
//! bay]`, a route's lane number, a key's seat. A position is an answer to
//! "where is it now", and a reorder, an insert or a delete changes that answer
//! for every row below the edit. So every structural edit had to carry a
//! shuffle for every table that held one, and each table that was missed (the
//! routes on a reorder, the pad seats in `seat_order`) parted somebody's plugin
//! from its state or its wiring. A position has a second fault: it is bounded
//! by the shape it was counted in, and the desk is about to stop having a
//! fixed shape.
//!
//! An identity does not move. [`TrackId`] is minted once per row and never
//! reused, so "track 7's second pad's first bay" is the same string before and
//! after any edit, and nothing on disk is renamed because the order changed.
//!
//! **Pure and caller-free on purpose.** This module has no idea what a
//! settings file, an engine or a licence is. It names things and converts
//! names to text and back. `desk_legacy` is its one bridge to the frozen
//! 32-channel seat map, and the proof that the two agree lives there.
//!
//! **Two bay resolvers, and they are not interchangeable.** The master shows
//! its six slots in a different order from the order its bays have always
//! been PERSISTED in (its extra rack was added above the original one, but
//! every saved curve already said bay 0 meant the original). [`slot_key`]
//! answers the DISPLAY question and is the only place that quirk lives;
//! [`auto_key`] answers the persisted and automation question and has no
//! quirk at all. A caller that uses one for the other compiles, passes every
//! round-trip test, and moves every master plugin's automation onto the other
//! rack. See the note on `instrument.rs:param_slot` for the same warning from
//! the engine's side.

use std::fmt;
use std::str::FromStr;

/// Bays in one rack. Three, as `recorder::INSERTS` has always been.
pub const BAYS: usize = 3;

/// Racks a strip SHOWS: its own and the one below (above, on the master), for
/// the six slots every track, the click and the master draw.
pub const STRIP_RACKS: usize = 2;

/// Pedal seats on one track's amp board, the owner's seven.
pub const AMP_PEDALS: usize = 7;

/// Pads on one track's drum instrument: every MIDI key.
pub const PADS: usize = 128;

/// **A sanity bound for a hostile file, not a tier.** A desk that claims more
/// rows than this is refused as structurally bad rather than allocated. A
/// supporter's real limit is the machine; this only stops a four-byte number
/// in a JSON file from asking for gigabytes.
pub const MAX_ROWS: usize = 65_536;

/// **Chains a reader accepts.** The grammar can SPELL `c65535`, so a typo or a
/// future build's file still parses; a desk reader drops a chain at or past
/// this as a referential error (one entry, listed, never the file). Raised
/// together with `AutoParam::Plugin`'s u8 bay under owner call 1, never alone,
/// because [`auto_key`]'s uniform `bay / BAYS` would otherwise name a chain
/// the reader then throws away.
pub const MAX_CHAIN: usize = 2;

/// The plugin-state sidecar's name prefix. The body is unchanged from the
/// legacy files: `<bundle path> NUL <TGST state>`.
pub const STATE_PREFIX: &str = "pstate-";

/// The plugin-state sidecar's extension.
const STATE_SUFFIX: &str = ".bin";

/// **A track row's identity**: per document, minted from a monotonic
/// `next_id`, never reused.
///
/// Legacy row r is `TrackId(r)` in every era (see `desk_legacy::identity_ids`
/// for why that holds), which is what lets a migrated document keep every
/// legacy sidecar name meaningful without a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(pub u32);

/// **Anything a route, a send, a key or a rack can be attached to**: a track
/// by its identity, or one of the four specials, which exist exactly once per
/// desk and so need no identity of their own.
///
/// The derived order puts every track before every special, and tracks in id
/// order. That is the order `plugins` and `bypass` are written in, and it is
/// NUMERIC: `t2` sorts before `t10`, which a string sort would get wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Node {
    Track(TrackId),
    Drums,
    Click,
    Fx,
    Master,
}

impl Node {
    /// The four specials, in their legacy strip order (drums, click, fx,
    /// master), which is also their sort order.
    pub const SPECIALS: [Node; 4] = [Node::Drums, Node::Click, Node::Fx, Node::Master];

    /// The track this node is, if it is one.
    pub const fn track(self) -> Option<TrackId> {
        match self {
            Node::Track(id) => Some(id),
            _ => None,
        }
    }
}

impl From<TrackId> for Node {
    fn from(id: TrackId) -> Self {
        Node::Track(id)
    }
}

/// **Which rack on a node.** A chain is a strip's own rack of bays (0 is the
/// original three, 1 the extra three); a pedal is one seat of a track's amp
/// board; a pad is one key of a track's drum instrument, with its own bays.
///
/// Pedals and pads belong to TRACKS only. The grammar refuses one on a
/// special, because no build ever had a seat for it: a string that names one
/// is a corrupt file or a forgery, and accepting it would let a reader invent
/// a rack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rack {
    Chain(u16),
    Pedal(u8),
    Pad(u8),
}

impl Rack {
    /// Whether a desk reader keeps a plugin in this rack: pedals and pads are
    /// bounded by the grammar already, so this is the chain's [`MAX_CHAIN`].
    pub const fn in_reach(self) -> bool {
        match self {
            Rack::Chain(c) => (c as usize) < MAX_CHAIN,
            Rack::Pedal(_) | Rack::Pad(_) => true,
        }
    }
}

/// **One rack, by identity**: whose it is and which of theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeatKey {
    pub node: Node,
    pub rack: Rack,
}

impl SeatKey {
    pub const fn new(node: Node, rack: Rack) -> Self {
        SeatKey { node, rack }
    }

    /// Whether the grammar can say this: a pedal below [`AMP_PEDALS`] and a
    /// pad below [`PADS`], each on a track. Every chain number is spellable;
    /// see [`MAX_CHAIN`] for where a chain is bounded instead.
    pub const fn is_valid(self) -> bool {
        let on_track = matches!(self.node, Node::Track(_));
        match self.rack {
            Rack::Chain(_) => true,
            Rack::Pedal(p) => on_track && (p as usize) < AMP_PEDALS,
            Rack::Pad(n) => on_track && (n as usize) < PADS,
        }
    }
}

/// **One plugin bay, by identity**: the key of `plugins`, `bypass`, every
/// seat-keyed session field and every `pstate-*.bin` sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeatBay {
    pub seat: SeatKey,
    pub bay: u8,
}

impl SeatBay {
    pub const fn new(node: Node, rack: Rack, bay: u8) -> Self {
        SeatBay { seat: SeatKey { node, rack }, bay }
    }

    /// Whether the grammar can say this: a valid seat and a bay below
    /// [`BAYS`].
    pub const fn is_valid(self) -> bool {
        self.seat.is_valid() && (self.bay as usize) < BAYS
    }
}

/// **Which bay a strip's DISPLAYED slot is.** Slots 0..2 are the first rack
/// shown, 3..5 the second.
///
/// Every node shows its own chain first and its extra chain below, EXCEPT the
/// master, whose extra rack was added ABOVE the original: its slots 0..2 are
/// `Chain(1)` and 3..5 are `Chain(0)`. That was deliberate (a rack runs top to
/// bottom, and a session that already had master plugins had to keep hearing
/// them last in the chain), and this function is the ONLY place the quirk
/// lives, as `recorder::seat_bay` was before it. `None` past the sixth slot.
///
/// For a persisted bay number or an automation key, this is the wrong
/// function: see [`auto_key`].
pub fn slot_key(node: Node, slot: usize) -> Option<SeatBay> {
    if slot >= BAYS * STRIP_RACKS {
        return None;
    }
    let shown = slot / BAYS;
    let chain = if node == Node::Master { STRIP_RACKS - 1 - shown } else { shown };
    Some(SeatBay::new(node, Rack::Chain(chain as u16), (slot % BAYS) as u8))
}

/// [`slot_key`]'s inverse: the displayed slot a bay sits in, or `None` for a
/// bay no strip shows (a pedal, a pad, a chain past the second, a bay past
/// the third).
pub fn display_slot(sb: SeatBay) -> Option<usize> {
    let Rack::Chain(chain) = sb.seat.rack else {
        return None;
    };
    let (chain, bay) = (chain as usize, sb.bay as usize);
    if chain >= STRIP_RACKS || bay >= BAYS {
        return None;
    }
    let shown = if sb.seat.node == Node::Master { STRIP_RACKS - 1 - chain } else { chain };
    Some(shown * BAYS + bay)
}

/// **Which bay a PERSISTED or automation bay number is**: `Chain(bay / 3)`,
/// bay `bay % 3`, on every node alike, the master included.
///
/// Uniform because the persisted order already was: `fx:<bay>:<id>` keys 0..2
/// have meant the master's ORIGINAL rack in every document ever saved, and its
/// extra rack took 3..5 when it arrived, exactly as a folder's and a bus's
/// did. Equal to `instrument.rs:param_slot` and `TangentApp::master_bay` over
/// their domain; it also answers for the buses, which have no automation lane
/// but do have six persisted bays. `None` past [`MAX_CHAIN`] racks.
pub fn auto_key(node: Node, bay: usize) -> Option<SeatBay> {
    if bay >= BAYS * MAX_CHAIN {
        return None;
    }
    Some(SeatBay::new(node, Rack::Chain((bay / BAYS) as u16), (bay % BAYS) as u8))
}

/// [`auto_key`]'s inverse: the persisted bay number of a chain bay, or `None`
/// for a pedal, a pad, or anything [`auto_key`] would not have produced.
pub fn auto_bay(sb: SeatBay) -> Option<usize> {
    let Rack::Chain(chain) = sb.seat.rack else {
        return None;
    };
    let (chain, bay) = (chain as usize, sb.bay as usize);
    (chain < MAX_CHAIN && bay < BAYS).then_some(chain * BAYS + bay)
}

/// The sidecar a bay's plugin state is kept in: `pstate-<seatbay>.bin`.
pub fn state_file_name(sb: SeatBay) -> String {
    format!("{STATE_PREFIX}{sb}{STATE_SUFFIX}")
}

/// [`state_file_name`]'s inverse, as strict as the grammar: a legacy
/// `insert-state-S-B.bin`, a temp file, or any name that is not exactly what
/// [`state_file_name`] would write answers `None`.
pub fn parse_state_file_name(name: &str) -> Option<SeatBay> {
    name.strip_prefix(STATE_PREFIX)?
        .strip_suffix(STATE_SUFFIX)?
        .parse()
        .ok()
}

// ── Text ────────────────────────────────────────────────────────────────────
//
// One grammar everywhere (design Appendix C): JSON keys, sidecar names, env
// vars and logs.
//
//   node    := "t" <u32> | "drums" | "click" | "fx" | "master"
//   rack    := "c" <u16> | "p" <0..=6> | "n" <0..=127>
//   seatkey := node "-" rack
//   seatbay := seatkey "-b" <0..=2>
//
// **Strict: a string is accepted only if formatting what it parsed to gives
// the same string back.** That one rule refuses leading zeros, a `+`, upper
// case and stray whitespace without a clause for each, and it is what makes
// the text a KEY: two spellings of one bay would be two entries in a JSON
// object and two sidecars on disk, and whichever the reader met second would
// silently win.

/// Why a name was refused. The text is kept so a dropped-entry notice can say
/// which key it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadId {
    pub text: String,
    pub why: &'static str,
}

impl fmt::Display for BadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a desk identity: {}", self.text, self.why)
    }
}

impl std::error::Error for BadId {}

fn bad(text: &str, why: &'static str) -> BadId {
    BadId { text: text.to_owned(), why }
}

/// Plain decimal digits only, then the canonical check: `std`'s integer
/// parsers take a leading `+`, which this grammar does not.
fn digits<T: FromStr>(s: &str) -> Option<T> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    s.parse().ok()
}

/// The canonical-text rule, applied once at every entry point.
fn canonical<T: fmt::Display>(text: &str, value: T) -> Result<T, BadId> {
    if value.to_string() == text {
        Ok(value)
    } else {
        Err(bad(text, "not in canonical form"))
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t{}", self.0)
    }
}

impl FromStr for TrackId {
    type Err = BadId;

    fn from_str(s: &str) -> Result<Self, BadId> {
        let n = s
            .strip_prefix('t')
            .and_then(digits::<u32>)
            .ok_or_else(|| bad(s, "a track is t and a decimal u32"))?;
        canonical(s, TrackId(n))
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Track(id) => id.fmt(f),
            Node::Drums => f.write_str("drums"),
            Node::Click => f.write_str("click"),
            Node::Fx => f.write_str("fx"),
            Node::Master => f.write_str("master"),
        }
    }
}

impl FromStr for Node {
    type Err = BadId;

    fn from_str(s: &str) -> Result<Self, BadId> {
        let node = match s {
            "drums" => Node::Drums,
            "click" => Node::Click,
            "fx" => Node::Fx,
            "master" => Node::Master,
            _ if s.starts_with('t') => Node::Track(s.parse()?),
            _ => return Err(bad(s, "unknown owner")),
        };
        canonical(s, node)
    }
}

impl fmt::Display for Rack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rack::Chain(c) => write!(f, "c{c}"),
            Rack::Pedal(p) => write!(f, "p{p}"),
            Rack::Pad(n) => write!(f, "n{n}"),
        }
    }
}

impl FromStr for Rack {
    type Err = BadId;

    fn from_str(s: &str) -> Result<Self, BadId> {
        let rack = if let Some(rest) = s.strip_prefix('c') {
            Rack::Chain(digits(rest).ok_or_else(|| bad(s, "a chain is c and a decimal u16"))?)
        } else if let Some(rest) = s.strip_prefix('p') {
            match digits::<u8>(rest) {
                Some(p) if (p as usize) < AMP_PEDALS => Rack::Pedal(p),
                _ => return Err(bad(s, "a pedal is p0 to p6")),
            }
        } else if let Some(rest) = s.strip_prefix('n') {
            match digits::<u8>(rest) {
                Some(n) if (n as usize) < PADS => Rack::Pad(n),
                _ => return Err(bad(s, "a pad is n0 to n127")),
            }
        } else {
            return Err(bad(s, "unknown rack"));
        };
        canonical(s, rack)
    }
}

impl fmt::Display for SeatKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.node, self.rack)
    }
}

impl FromStr for SeatKey {
    type Err = BadId;

    fn from_str(s: &str) -> Result<Self, BadId> {
        // No node spelling contains a '-', so the first one is the seam.
        let (node, rack) = s.split_once('-').ok_or_else(|| bad(s, "a seat is node-rack"))?;
        let key = SeatKey::new(node.parse()?, rack.parse()?);
        if !key.is_valid() {
            return Err(bad(s, "pedals and pads belong to tracks"));
        }
        canonical(s, key)
    }
}

impl fmt::Display for SeatBay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-b{}", self.seat, self.bay)
    }
}

impl FromStr for SeatBay {
    type Err = BadId;

    fn from_str(s: &str) -> Result<Self, BadId> {
        let (seat, bay) = s.rsplit_once('-').ok_or_else(|| bad(s, "a bay is node-rack-bN"))?;
        let bay = match bay.strip_prefix('b').and_then(digits::<u8>) {
            Some(b) if (b as usize) < BAYS => b,
            _ => return Err(bad(s, "a bay is b0 to b2")),
        };
        canonical(s, SeatBay { seat: seat.parse()?, bay })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator, so a property test that fails names
    /// the same input on every machine and every run.
    struct Xorshift(u64);

    impl Xorshift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn any_node(r: &mut Xorshift) -> Node {
        match r.below(6) {
            0 => Node::Drums,
            1 => Node::Click,
            2 => Node::Fx,
            3 => Node::Master,
            // Small ids most of the time, so the digit edges (9/10, 99/100)
            // are hit, and the whole u32 range the rest.
            4 => Node::Track(TrackId(r.below(130) as u32)),
            _ => Node::Track(TrackId(r.next() as u32)),
        }
    }

    /// Any bay the grammar can say, chains across all of u16.
    fn any_valid_bay(r: &mut Xorshift) -> SeatBay {
        loop {
            let node = any_node(r);
            let rack = match r.below(4) {
                0 => Rack::Chain(r.below(3) as u16),
                1 => Rack::Chain(r.next() as u16),
                2 => Rack::Pedal(r.below(AMP_PEDALS as u64) as u8),
                _ => Rack::Pad(r.below(PADS as u64) as u8),
            };
            let sb = SeatBay::new(node, rack, r.below(BAYS as u64) as u8);
            if sb.is_valid() {
                return sb;
            }
        }
    }

    /// Every node shape the edges care about, for the exhaustive walks.
    fn edge_nodes() -> Vec<Node> {
        let mut nodes = Node::SPECIALS.to_vec();
        for id in [0, 1, 9, 10, 31, 32, 99, 100, 65_535, 65_536, u32::MAX] {
            nodes.push(Node::Track(TrackId(id)));
        }
        nodes
    }

    fn every_rack() -> Vec<Rack> {
        let mut racks = vec![Rack::Chain(0), Rack::Chain(1), Rack::Chain(2), Rack::Chain(u16::MAX)];
        racks.extend((0..=u8::MAX).map(Rack::Pedal));
        racks.extend((0..=u8::MAX).map(Rack::Pad));
        racks
    }

    #[test]
    fn every_valid_bay_round_trips_through_text_and_its_sidecar_name() {
        let mut checked = 0;
        for node in edge_nodes() {
            for rack in every_rack() {
                for bay in 0..=u8::MAX {
                    let sb = SeatBay::new(node, rack, bay);
                    let text = sb.to_string();
                    let back = text.parse::<SeatBay>();
                    if sb.is_valid() {
                        assert_eq!(back, Ok(sb), "{text}");
                        assert_eq!(parse_state_file_name(&state_file_name(sb)), Some(sb));
                        assert_eq!(sb.seat.to_string().parse::<SeatKey>(), Ok(sb.seat));
                        checked += 1;
                    } else {
                        // An invalid value still PRINTS (a log line must say
                        // what it holds), but nothing reads that print back.
                        assert!(back.is_err(), "{text} parsed although invalid");
                        assert_eq!(parse_state_file_name(&state_file_name(sb)), None);
                    }
                }
            }
        }
        // 15 nodes x 4 chains x 3 bays, plus 11 track nodes x (7 pedals +
        // 128 pads) x 3 bays. A literal, so a validity rule that quietly
        // widens or narrows shows up here as a number.
        assert_eq!(checked, 15 * 4 * 3 + 11 * (7 + 128) * 3);

        let mut r = Xorshift(0x9E37_79B9_7F4A_7C15);
        for _ in 0..50_000 {
            let sb = any_valid_bay(&mut r);
            assert_eq!(sb.to_string().parse::<SeatBay>(), Ok(sb));
            assert_eq!(parse_state_file_name(&state_file_name(sb)), Some(sb));
        }
    }

    /// **The strictness property.** Mutate one byte of a good name in every
    /// way a typo, a hand edit or a case-folding filesystem might; whatever
    /// still parses must print back as exactly the mutated text. So there is
    /// never a second spelling of one bay.
    #[test]
    fn a_mutated_name_either_fails_or_is_itself_canonical() {
        const ALPHABET: &[u8] = b"0123456789-+ tTcCpPnNbBdrumsklixfaeMD_.";
        let mut r = Xorshift(0xD1B5_4A32_D192_ED03);
        let mut refused = 0;
        for _ in 0..40_000 {
            let mut bytes = any_valid_bay(&mut r).to_string().into_bytes();
            let at = r.below(bytes.len() as u64 + 1) as usize;
            let with = ALPHABET[r.below(ALPHABET.len() as u64) as usize];
            match r.below(3) {
                0 if at < bytes.len() => bytes[at] = with,
                1 if at < bytes.len() => {
                    bytes.remove(at);
                }
                _ => bytes.insert(at, with),
            }
            let text = String::from_utf8(bytes).expect("ascii in, ascii out");
            match text.parse::<SeatBay>() {
                Ok(sb) => {
                    assert_eq!(sb.to_string(), text, "a second spelling parsed");
                    assert!(sb.is_valid(), "{text} parsed to an invalid bay");
                }
                Err(_) => refused += 1,
            }
        }
        // Most single-byte edits must be refusals; if this falls the
        // grammar has gone lax somewhere.
        assert!(refused > 30_000, "only {refused} of 40000 mutations refused");
    }

    #[test]
    fn the_grammar_refuses_every_second_spelling_and_every_impossible_rack() {
        for good in [
            "t3-c0-b1",
            "t12-n36-b2",
            "t0-p4-b0",
            "master-c1-b2",
            "drums-c1-b2",
            "t4294967295-c65535-b2",
            "fx-c0-b0",
            "click-c1-b1",
        ] {
            assert_eq!(good.parse::<SeatBay>().map(|sb| sb.to_string()).as_deref(), Ok(good));
        }
        for bad in [
            // leading zeros, in every number
            "t03-c0-b1",
            "t00-c0-b1",
            "t3-c00-b1",
            "t3-c01-b1",
            "t3-p04-b0",
            "t3-n036-b0",
            "t3-c0-b01",
            "t3-c0-b00",
            // upper case, anywhere
            "T3-c0-b1",
            "t3-C0-b1",
            "t3-c0-B1",
            "Master-c0-b0",
            "MASTER-c0-b0",
            "Drums-c1-b0",
            "t3-P1-b0",
            "t3-N1-b0",
            // out of range
            "t3-p7-b0",
            "t3-p255-b0",
            "t3-n128-b0",
            "t3-n256-b0",
            "t3-c0-b3",
            "t3-c0-b255",
            "t3-c65536-b0",
            "t4294967296-c0-b0",
            // a pedal or a pad on a special
            "master-p0-b0",
            "drums-n36-b0",
            "click-p1-b0",
            "fx-n0-b0",
            // unknown owners and racks
            "bus-c0-b0",
            "row3-c0-b0",
            "d-c0-b0",
            "t-c0-b0",
            "t3-x0-b0",
            "t3-c-b0",
            "t3-p-b0",
            "t3-n-b0",
            // signs, whitespace, missing or extra parts
            "t+3-c0-b0",
            "t3-c+0-b0",
            "t3-c0-b+1",
            "t-3-c0-b0",
            "t3-c0-b-1",
            " t3-c0-b0",
            "t3-c0-b0 ",
            "t3 -c0-b0",
            "t3-c0",
            "t3-c0-b",
            "t3-c0-",
            "t3--c0-b0",
            "t3-c0--b0",
            "t3-c0-b0-b0",
            "-c0-b0",
            "",
            "t3_c0_b0",
        ] {
            assert!(bad.parse::<SeatBay>().is_err(), "{bad:?} parsed");
        }
        for bad in ["T3", "t03", "t", "t+1", "t-1", "Master", "masters", "", "t4294967296", "x"] {
            assert!(bad.parse::<Node>().is_err(), "{bad:?} parsed as a node");
        }
        for bad in ["master-p0", "t3-p7", "t3-n128", "t3-c00", "t3", "t3-"] {
            assert!(bad.parse::<SeatKey>().is_err(), "{bad:?} parsed as a seat");
        }
        for bad in [
            "insert-state-3-1.bin",
            "pstate-t3-c0-b1",
            "pstate-t3-c0-b1.bin.tmp",
            "PSTATE-t3-c0-b1.bin",
            "pstate-t03-c0-b1.bin",
            "pstate-master-p0-b0.bin",
            "xpstate-t3-c0-b1.bin",
        ] {
            assert_eq!(parse_state_file_name(bad), None, "{bad:?}");
        }
        assert_eq!(
            state_file_name(SeatBay::new(Node::Master, Rack::Chain(1), 2)),
            "pstate-master-c1-b2.bin"
        );
    }

    /// Sidecars and JSON keys are written in SeatBay order, which must be the
    /// numeric order of tracks with the specials after them: a string sort
    /// would put t10 before t2.
    #[test]
    fn seat_bays_sort_by_track_number_then_specials_then_rack_then_bay() {
        let mut v: Vec<SeatBay> = [
            "master-c0-b0",
            "t10-c0-b0",
            "drums-c0-b0",
            "t2-n3-b0",
            "t2-c1-b2",
            "t2-c1-b0",
            "t2-p6-b0",
            "fx-c1-b0",
            "click-c0-b2",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
        v.sort();
        let got: Vec<String> = v.iter().map(ToString::to_string).collect();
        assert_eq!(
            got,
            [
                "t2-c1-b0",
                "t2-c1-b2",
                "t2-p6-b0",
                "t2-n3-b0",
                "t10-c0-b0",
                "drums-c0-b0",
                "click-c0-b2",
                "fx-c1-b0",
                "master-c0-b0",
            ]
        );
    }

    #[test]
    fn display_slots_put_the_masters_extra_rack_first_and_nobody_elses() {
        for node in edge_nodes() {
            for slot in 0..BAYS * STRIP_RACKS {
                let sb = slot_key(node, slot).expect("six slots on every strip");
                assert!(sb.is_valid());
                assert_eq!(display_slot(sb), Some(slot), "{node} slot {slot}");
                let first_shown = if node == Node::Master { 1 } else { 0 };
                let chain = if slot < BAYS { first_shown } else { 1 - first_shown };
                assert_eq!(sb, SeatBay::new(node, Rack::Chain(chain), (slot % BAYS) as u8));
            }
            assert_eq!(slot_key(node, BAYS * STRIP_RACKS), None);
            assert_eq!(slot_key(node, usize::MAX), None);
        }
        // Nothing a strip does not show has a slot.
        let t = Node::Track(TrackId(4));
        for sb in [
            SeatBay::new(t, Rack::Pedal(0), 0),
            SeatBay::new(t, Rack::Pad(36), 1),
            SeatBay::new(t, Rack::Chain(2), 0),
            SeatBay::new(t, Rack::Chain(0), 3),
            SeatBay::new(Node::Master, Rack::Chain(2), 0),
        ] {
            assert_eq!(display_slot(sb), None, "{sb}");
        }
    }

    /// The persisted order has no quirk, and the two resolvers disagree on
    /// the master and ONLY on the master. Pinned both ways, because a caller
    /// that swaps them passes every round trip.
    #[test]
    fn automation_bays_are_uniform_and_differ_from_display_only_on_the_master() {
        for node in edge_nodes() {
            for bay in 0..BAYS * MAX_CHAIN {
                let sb = auto_key(node, bay).expect("six persisted bays");
                assert_eq!(
                    sb,
                    SeatBay::new(node, Rack::Chain((bay / BAYS) as u16), (bay % BAYS) as u8)
                );
                assert_eq!(auto_bay(sb), Some(bay));
                if node == Node::Master {
                    assert_ne!(slot_key(node, bay), Some(sb), "master slot {bay}");
                } else {
                    assert_eq!(slot_key(node, bay), Some(sb), "{node} bay {bay}");
                }
            }
            assert_eq!(auto_key(node, BAYS * MAX_CHAIN), None);
        }
        assert_eq!(auto_key(Node::Master, 0), Some(SeatBay::new(Node::Master, Rack::Chain(0), 0)));
        assert_eq!(slot_key(Node::Master, 0), Some(SeatBay::new(Node::Master, Rack::Chain(1), 0)));
        let t = Node::Track(TrackId(0));
        assert_eq!(auto_bay(SeatBay::new(t, Rack::Pad(0), 0)), None);
        assert_eq!(auto_bay(SeatBay::new(t, Rack::Pedal(0), 0)), None);
        assert_eq!(auto_bay(SeatBay::new(t, Rack::Chain(MAX_CHAIN as u16), 0)), None);
        assert_eq!(auto_bay(SeatBay::new(t, Rack::Chain(0), BAYS as u8)), None);
    }

    #[test]
    fn a_chain_past_max_chain_is_spellable_but_out_of_reach() {
        let far: SeatBay = "t1-c2-b0".parse().unwrap();
        assert!(far.is_valid());
        assert!(!far.seat.rack.in_reach());
        assert!(Rack::Chain(1).in_reach());
        assert!(Rack::Pedal(6).in_reach() && Rack::Pad(127).in_reach());
    }
}
