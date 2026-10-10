//! The frozen 32-channel seat map, and the one road from it to identity.
//!
//! **Frozen means literal.** Every number here is the shape the 32-channel
//! builds wrote to disk, as design Appendix A records it, written out rather
//! than computed from `recorder::CHANNELS` and friends. Those live constants
//! are about to stop meaning anything (the desk stops having a width), and a
//! legacy reader that derived its map from them would silently re-read every
//! old file in a new shape the day one of them changed. The settings files,
//! documents and `insert-state-<seat>-<bay>.bin` sidecars already on people's
//! disks do not change, so neither does this.
//!
//! **The map, flat bay = seat * 3 + bay:**
//!
//! | Seats | Owner |
//! |---|---|
//! | 0..=31 | Row(c) Chain(0) |
//! | 32 / 33 / 34 | Drums / Click / Fx, Chain(0) |
//! | 35 | Master Chain(0), persisted bays 0..2, displayed as slots 3..5 |
//! | 36..=163 | Row(0) Pad(seat - 36), the original pad block |
//! | 164 | Master Chain(1), persisted bays 3..5, displayed as slots 0..2 |
//! | 165..=4132 | Row(1 + (seat-165)/128) Pad((seat-165)%128) |
//! | 4133..=4164 | Row(seat - 4133) Chain(1), the folder-extra rack |
//! | 4165..=4388 | Row((seat-4165)/7) Pedal((seat-4165)%7) |
//! | 4389 / 4390 / 4391 | Drums / Click / Fx, Chain(1) |
//!
//! The odd shape (row 0's pads in a block of their own, the master's extra
//! rack wedged between two pad blocks) is history, not design: each block was
//! APPENDED when its feature arrived, because an appended seat renumbers
//! nothing anybody had already written down. That rule is exactly why the map
//! can never be tidied, and why [`decode_v32`] is proven against the live
//! seat functions in the tests below while those still exist (the
//! cross-checks are deleted with them in W8).
//!
//! **Identity, once.** A legacy owner becomes a [`Node`] through [`bind`] and
//! a list of row ids. Every era lift inserted its new channels after the old
//! ones, so no legacy lane index ever moved, and a migrated desk binds through
//! [`identity_ids`]: legacy row r is `TrackId(r)`.

use crate::desk_id::{Node, Rack, SeatBay, TrackId, AMP_PEDALS, BAYS, PADS};

/// Channels on the legacy desk.
pub const V32_CHANNELS: usize = 32;

/// Legacy strips, master aside: 32 channels and the three buses. The master
/// is strip [`V32_STRIPS`] itself, so strip space (colours and names 36 wide,
/// sends 35, the u64 mute/solo/phase masks) runs `0..=V32_STRIPS`.
pub const V32_STRIPS: usize = 35;

/// Every legacy rack seat.
pub const V32_TOTAL_SEATS: usize = 4_392;

/// The legacy flat bay space, `V32_TOTAL_SEATS * 3`: the length of a
/// 32-channel `strip_inserts`.
pub const V32_INSERT_SPACE: usize = 13_176;

/// The v21 HEAD of `strip_inserts`: the 36 strip seats (channels, buses and
/// the master's own rack) times three. The tail is everything past it.
pub const V32_HEAD_BAYS: usize = 108;

// The block bases, as literals, and below them the proof that the blocks tile
// the seat space with no gap and no overlap.
const DRUMS: usize = 32;
const CLICK: usize = 33;
const FX: usize = 34;
const MASTER: usize = 35;
const ROW0_PADS: usize = 36;
const MASTER_EXTRA: usize = 164;
const EXTRA_PADS: usize = 165;
const FOLDER_EXTRA: usize = 4_133;
const PEDALS: usize = 4_165;
const BUS_EXTRA: usize = 4_389;

const _: () = {
    assert!(DRUMS == V32_CHANNELS && CLICK == DRUMS + 1 && FX == CLICK + 1);
    assert!(MASTER == FX + 1 && MASTER == V32_STRIPS);
    assert!(ROW0_PADS == MASTER + 1);
    assert!(MASTER_EXTRA == ROW0_PADS + PADS);
    assert!(EXTRA_PADS == MASTER_EXTRA + 1);
    assert!(FOLDER_EXTRA == EXTRA_PADS + (V32_CHANNELS - 1) * PADS);
    assert!(PEDALS == FOLDER_EXTRA + V32_CHANNELS);
    assert!(BUS_EXTRA == PEDALS + V32_CHANNELS * AMP_PEDALS);
    assert!(V32_TOTAL_SEATS == BUS_EXTRA + 3);
    assert!(V32_INSERT_SPACE == V32_TOTAL_SEATS * BAYS);
    assert!(V32_HEAD_BAYS == (V32_STRIPS + 1) * BAYS);
};

/// **Whose a legacy seat was**, in legacy terms: a row by its position, or a
/// special. A row here is a LANE INDEX, not an identity; [`bind`] is the only
/// way to turn it into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LegacyOwner {
    Row(u16),
    Drums,
    Click,
    Fx,
    Master,
}

/// The special a bus strip or bus-extra seat belongs to, by its offset in
/// drums, click, fx order. Shared by the strip and seat decoders so the order
/// is written down once.
fn bus(offset: usize) -> Option<LegacyOwner> {
    [LegacyOwner::Drums, LegacyOwner::Click, LegacyOwner::Fx].get(offset).copied()
}

fn bus_offset(owner: LegacyOwner) -> Option<usize> {
    match owner {
        LegacyOwner::Drums => Some(0),
        LegacyOwner::Click => Some(1),
        LegacyOwner::Fx => Some(2),
        LegacyOwner::Row(_) | LegacyOwner::Master => None,
    }
}

/// A row index that has a legacy seat, or `None`.
fn row(owner: LegacyOwner) -> Option<usize> {
    match owner {
        LegacyOwner::Row(r) if (r as usize) < V32_CHANNELS => Some(r as usize),
        _ => None,
    }
}

/// Lossless: every caller's value is below [`V32_TOTAL_SEATS`].
fn row_at(r: usize) -> LegacyOwner {
    LegacyOwner::Row(r as u16)
}

/// **Whose a legacy rack seat was, and which of theirs.** `None` past
/// [`V32_TOTAL_SEATS`]. A bijection with [`encode_v32_seat`] over
/// `0..V32_TOTAL_SEATS`.
pub fn decode_v32_seat(seat: usize) -> Option<(LegacyOwner, Rack)> {
    Some(if seat < DRUMS {
        (row_at(seat), Rack::Chain(0))
    } else if seat < MASTER {
        (bus(seat - DRUMS)?, Rack::Chain(0))
    } else if seat == MASTER {
        (LegacyOwner::Master, Rack::Chain(0))
    } else if seat < MASTER_EXTRA {
        (LegacyOwner::Row(0), Rack::Pad((seat - ROW0_PADS) as u8))
    } else if seat == MASTER_EXTRA {
        (LegacyOwner::Master, Rack::Chain(1))
    } else if seat < FOLDER_EXTRA {
        let at = seat - EXTRA_PADS;
        (row_at(1 + at / PADS), Rack::Pad((at % PADS) as u8))
    } else if seat < PEDALS {
        (row_at(seat - FOLDER_EXTRA), Rack::Chain(1))
    } else if seat < BUS_EXTRA {
        let at = seat - PEDALS;
        (row_at(at / AMP_PEDALS), Rack::Pedal((at % AMP_PEDALS) as u8))
    } else if seat < V32_TOTAL_SEATS {
        (bus(seat - BUS_EXTRA)?, Rack::Chain(1))
    } else {
        return None;
    })
}

/// [`decode_v32_seat`]'s inverse: the legacy seat a rack had, or `None` for
/// one the 32-channel desk never had a seat for (a row past 31, a chain past
/// 1, a pedal or pad off a row or out of range).
pub fn encode_v32_seat(owner: LegacyOwner, rack: Rack) -> Option<usize> {
    let (r, special) = (row(owner), bus_offset(owner));
    match rack {
        Rack::Chain(0) => match owner {
            LegacyOwner::Master => Some(MASTER),
            _ => r.or_else(|| special.map(|b| DRUMS + b)),
        },
        Rack::Chain(1) => match owner {
            LegacyOwner::Master => Some(MASTER_EXTRA),
            _ => r.map(|r| FOLDER_EXTRA + r).or_else(|| special.map(|b| BUS_EXTRA + b)),
        },
        Rack::Chain(_) => None,
        Rack::Pad(n) if (n as usize) < PADS => match r? {
            0 => Some(ROW0_PADS + n as usize),
            r => Some(EXTRA_PADS + (r - 1) * PADS + n as usize),
        },
        Rack::Pedal(p) if (p as usize) < AMP_PEDALS => Some(PEDALS + r? * AMP_PEDALS + p as usize),
        Rack::Pad(_) | Rack::Pedal(_) => None,
    }
}

/// **Whose a legacy FLAT bay was** (`strip_inserts[i]`, or the `S-B` of an
/// `insert-state-S-B.bin` as `S * 3 + B`): owner, rack and bay. A bijection
/// with [`encode_v32`] over `0..V32_INSERT_SPACE`, proven exhaustively below.
pub fn decode_v32(flat: usize) -> Option<(LegacyOwner, Rack, u8)> {
    let (owner, rack) = decode_v32_seat(flat / BAYS)?;
    Some((owner, rack, (flat % BAYS) as u8))
}

/// [`decode_v32`]'s inverse, or `None` for anything the legacy map has no
/// flat bay for.
pub fn encode_v32(owner: LegacyOwner, rack: Rack, bay: u8) -> Option<usize> {
    if bay as usize >= BAYS {
        return None;
    }
    Some(encode_v32_seat(owner, rack)? * BAYS + bay as usize)
}

/// **Whose a legacy STRIP index was**: the index of `strip_names`,
/// `strip_colors`, `strip_sends` and the strip masks' bits. `0..=31` rows,
/// then drums, click, fx, and the master at [`V32_STRIPS`].
pub fn decode_v32_strip(strip: usize) -> Option<LegacyOwner> {
    if strip < V32_CHANNELS {
        Some(row_at(strip))
    } else if strip == V32_STRIPS {
        Some(LegacyOwner::Master)
    } else {
        bus(strip.checked_sub(V32_CHANNELS)?)
    }
}

/// [`decode_v32_strip`]'s inverse.
pub fn encode_v32_strip(owner: LegacyOwner) -> Option<usize> {
    match owner {
        LegacyOwner::Master => Some(V32_STRIPS),
        _ => row(owner).or_else(|| bus_offset(owner).map(|b| V32_CHANNELS + b)),
    }
}

/// **A legacy owner, as identity**: row r becomes `ids[r]`, a special stays
/// itself. `None` for a row the id list does not reach, which a reader treats
/// as a dangling entry rather than inventing a track for it.
pub fn bind(owner: LegacyOwner, ids: &[TrackId]) -> Option<Node> {
    Some(match owner {
        LegacyOwner::Row(r) => Node::Track(*ids.get(r as usize)?),
        LegacyOwner::Drums => Node::Drums,
        LegacyOwner::Click => Node::Click,
        LegacyOwner::Fx => Node::Fx,
        LegacyOwner::Master => Node::Master,
    })
}

/// [`bind`]'s inverse: a node's legacy owner under `ids`, its row being the
/// id's POSITION in the list. `None` for a track the list does not hold. A row
/// past 31 unbinds (it is still a row) but has no legacy seat, which
/// [`encode_v32`] then says.
pub fn unbind(node: Node, ids: &[TrackId]) -> Option<LegacyOwner> {
    Some(match node {
        Node::Track(id) => {
            LegacyOwner::Row(u16::try_from(ids.iter().position(|x| *x == id)?).ok()?)
        }
        Node::Drums => LegacyOwner::Drums,
        Node::Click => LegacyOwner::Click,
        Node::Fx => LegacyOwner::Fx,
        Node::Master => LegacyOwner::Master,
    })
}

/// **The identity rule, as a list: legacy row r is `TrackId(r)`.**
///
/// It holds in every era because every lift (8 to 16, 16 to 24, 24 to 32)
/// inserted its new channels AFTER the old ones, and the v13/v14 strip changes
/// sat above the channels, so no legacy lane index ever moved. A migrated
/// desk's `next_id` is therefore the row count, and in a migrated document
/// any id below 32 is a legacy row: the fact `legacy_states` fallbacks use to
/// find a plugin's pre-identity sidecar without a table.
pub fn identity_ids(rows: usize) -> Vec<TrackId> {
    (0..rows).map_while(|r| u32::try_from(r).ok()).map(TrackId).collect()
}

// ── A legacy (seat, bay) and an identity, under a desk's row ids ────────────
//
// The one road between the two that the live code may take: the host still
// names a bay by the frozen map's `(seat, bay)` until its own lane moves it
// onto identities (C11's rule: a new form beside the old, the old deleted by
// the lane that stops using it), and the legacy files and their pickers
// speak it. Every crossing goes through these two, so none of them does a
// sum of its own.

/// **The bay a legacy `(seat, bay)` is, by identity**, under row ids `ids`:
/// the seat through the frozen map, its row through [`bind`]. `None` for a
/// seat past the map, a bay past the third, or a row the ids do not reach.
pub fn bay_at(ids: &[TrackId], seat: usize, bay: usize) -> Option<SeatBay> {
    let (owner, rack) = decode_v32_seat(seat)?;
    let sb = SeatBay::new(bind(owner, ids)?, rack, u8::try_from(bay).ok()?);
    sb.is_valid().then_some(sb)
}

/// [`bay_at`]'s inverse: where the frozen map keeps a bay, under `ids`.
/// `None` for a track no row holds, a row past the map, or a rack it never
/// had a seat for.
pub fn seat_of(ids: &[TrackId], sb: SeatBay) -> Option<(usize, usize)> {
    let owner = unbind(sb.seat.node, ids)?;
    Some((encode_v32_seat(owner, sb.seat.rack)?, usize::from(sb.bay)))
}

/// **A legacy FLAT bay** (`strip_inserts[i]`, an `insert-state-S-B.bin`'s
/// `S * 3 + B`) **by identity**, under row ids `ids`: [`decode_v32`] and
/// [`bind`].
pub fn bay_of_flat(ids: &[TrackId], flat: usize) -> Option<SeatBay> {
    let (owner, rack, bay) = decode_v32(flat)?;
    let sb = SeatBay::new(bind(owner, ids)?, rack, bay);
    sb.is_valid().then_some(sb)
}

/// [`bay_of_flat`]'s inverse: the flat bay the frozen map keeps a bay at,
/// under `ids`.
pub fn flat_of(ids: &[TrackId], sb: SeatBay) -> Option<usize> {
    encode_v32(unbind(sb.seat.node, ids)?, sb.seat.rack, sb.bay)
}

/// **A dense legacy bay table, by identity**: `inserts[flat]` is the plugin
/// in the bay [`decode_v32`] says, its row bound through `ids`. Empty cells
/// and cells naming no row are left out: a seat a reader cannot place is a
/// plugin no row of this desk holds.
pub fn bind_bays(inserts: &[String], ids: &[TrackId]) -> std::collections::BTreeMap<SeatBay, String> {
    inserts
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.is_empty())
        .filter_map(|(flat, p)| Some((bay_of_flat(ids, flat)?, p.clone())))
        .collect()
}

/// [`bind_bays`]'s inverse: bays by identity as the dense legacy table, a
/// full [`V32_INSERT_SPACE`] of cells, each track at the row `ids` gives
/// it. A bay with no flat cell (a track no row holds) is left out.
pub fn dense_bays(plugins: &std::collections::BTreeMap<SeatBay, String>, ids: &[TrackId]) -> Vec<String> {
    let mut dense = vec![String::new(); V32_INSERT_SPACE];
    for (sb, path) in plugins {
        if let Some(cell) = flat_of(ids, *sb).and_then(|f| dense.get_mut(f)) {
            cell.clone_from(path);
        }
    }
    dense
}

// ── The frozen seat map under its old names ────────────────────────────────
//
// **Retired here from `recorder` (W5).** The live app names every bay by
// identity, so nothing in it computes a seat any more; these are the same
// functions at the frozen V32 literals, kept for the readers that still
// speak the frozen map: the engine's legacy shims and their tests, the
// host's tests, the era corpus. `recorder` re-exports the names those
// readers spell until each moves off them. Each is a second formulation of
// [`decode_v32`]'s map, and the tests below hold the two to each other.

/// The master's own seat: the one after the 35 strips.
pub const MASTER_SEAT: usize = MASTER;

/// The master's second rack, past the first pad block.
pub const MASTER_EXTRA_SEAT: usize = MASTER_EXTRA;

/// Where the pads of drum lanes past the first block begin.
pub const EXTRA_PAD_SEATS: usize = EXTRA_PADS;

/// Where the folder-extra seats begin, one per channel.
pub const FOLDER_EXTRA_SEATS: usize = FOLDER_EXTRA;

/// Where the amp-pedal seats begin, seven per channel.
pub const AMP_PEDAL_SEATS: usize = PEDALS;

/// Where the buses' extra seats begin, one per bus.
pub const BUS_EXTRA_SEATS: usize = BUS_EXTRA;

/// The buses: every strip past the channels (drums, click, fx).
pub const BUSES: usize = V32_STRIPS - V32_CHANNELS;

/// Every legacy rack seat ([`V32_TOTAL_SEATS`]).
pub const TOTAL_SEATS: usize = V32_TOTAL_SEATS;

/// The legacy flat bay space ([`V32_INSERT_SPACE`]).
pub const INSERT_SPACE: usize = V32_INSERT_SPACE;

/// **Which seat and bay a strip's Nth DISPLAYED slot was**, by the strip's
/// legacy index: the master's first three slots are its extra rack, a
/// channel's and a bus's 3..5 their second seats.
pub fn seat_bay(desk_index: usize, slot: usize) -> (usize, usize) {
    if desk_index == MASTER && slot >= BAYS {
        (MASTER, slot - BAYS)
    } else if desk_index == MASTER {
        (MASTER_EXTRA, slot)
    } else if desk_index < V32_CHANNELS && slot >= BAYS {
        (folder_extra_seat(desk_index), slot - BAYS)
    } else if desk_index < V32_STRIPS && slot >= BAYS {
        (bus_extra_seat(desk_index), slot - BAYS)
    } else {
        (desk_index, slot)
    }
}

/// **Which strip a rack seat's slots are shown on**, or `None` for a seat
/// no strip shows (a pad, a pedal).
pub fn rack_owner(seat: usize) -> Option<usize> {
    if seat <= MASTER {
        return Some(seat);
    }
    if seat == MASTER_EXTRA {
        return Some(MASTER);
    }
    folder_extra_of(seat).or_else(|| bus_extra_of(seat))
}

/// The bus whose extra seat this is, as its strip index.
pub fn bus_extra_of(seat: usize) -> Option<usize> {
    seat.checked_sub(BUS_EXTRA).filter(|i| *i < BUSES).map(|i| V32_CHANNELS + i)
}

/// Channel `ch`'s folder-extra seat.
pub const fn folder_extra_seat(ch: usize) -> usize {
    FOLDER_EXTRA + ch
}

/// Which channel's folder-extra seat this is.
pub fn folder_extra_of(seat: usize) -> Option<usize> {
    seat.checked_sub(FOLDER_EXTRA).filter(|ch| *ch < V32_CHANNELS)
}

/// Channel `ch`'s pedal `pedal` seat.
pub const fn amp_pedal_seat(ch: usize, pedal: usize) -> usize {
    PEDALS + ch * AMP_PEDALS + pedal
}

/// Which channel's which pedal a seat is.
pub fn amp_pedal_of(seat: usize) -> Option<(usize, usize)> {
    let at = seat.checked_sub(PEDALS)?;
    let ch = at / AMP_PEDALS;
    (ch < V32_CHANNELS).then_some((ch, at % AMP_PEDALS))
}

/// **Whose pedalboard a rack seat wears**: a channel's own seat and its
/// folder-extra seat both answer the channel; nothing else has a board.
pub fn pedal_owner(seat: usize) -> Option<usize> {
    if seat < V32_CHANNELS {
        return Some(seat);
    }
    folder_extra_of(seat)
}

/// Bus `desk_index`'s extra seat; anything that is not a bus gets the
/// first, the "answer rather than panic" its neighbours make.
pub const fn bus_extra_seat(desk_index: usize) -> usize {
    if desk_index < V32_CHANNELS || desk_index >= V32_STRIPS {
        return BUS_EXTRA;
    }
    BUS_EXTRA + (desk_index - V32_CHANNELS)
}

/// Where channel `ch`'s pad `pad` kept its bays: channel 0 in the original
/// block after the master, every other channel appended.
pub const fn pad_rack_index(ch: usize, pad: usize) -> usize {
    if ch == 0 {
        ROW0_PADS + pad
    } else {
        EXTRA_PADS + (ch - 1) * PADS + pad
    }
}

/// Channel 0's pad `pad` seat: the original pad block.
pub const fn pad_insert_index(pad: usize) -> usize {
    ROW0_PADS + pad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk_id::{auto_key, display_slot, slot_key, SeatBay, MAX_CHAIN, STRIP_RACKS};
    use crate::recorder;

    /// Every (owner, rack, bay) a reader could ever ask about, and a margin
    /// past each edge.
    fn candidates() -> Vec<(LegacyOwner, Rack, u8)> {
        let mut owners: Vec<LegacyOwner> = (0..=40).map(LegacyOwner::Row).collect();
        owners.extend([LegacyOwner::Row(u16::MAX), LegacyOwner::Drums, LegacyOwner::Click]);
        owners.extend([LegacyOwner::Fx, LegacyOwner::Master]);
        let mut racks: Vec<Rack> = (0..=3).map(Rack::Chain).collect();
        racks.push(Rack::Chain(u16::MAX));
        racks.extend((0..=u8::MAX).map(Rack::Pedal));
        racks.extend((0..=u8::MAX).map(Rack::Pad));
        let mut all = Vec::new();
        for &o in &owners {
            for &r in &racks {
                for b in 0..=4 {
                    all.push((o, r, b));
                }
            }
        }
        all
    }

    #[test]
    fn decode_v32_is_a_bijection_over_every_legacy_bay() {
        for flat in 0..V32_INSERT_SPACE {
            let (o, r, b) = decode_v32(flat).unwrap_or_else(|| panic!("flat {flat} undecoded"));
            assert_eq!(encode_v32(o, r, b), Some(flat), "flat {flat} = {o:?} {r:?} b{b}");
        }
        assert_eq!(decode_v32(V32_INSERT_SPACE), None);
        assert_eq!(decode_v32(usize::MAX), None);
        // The other direction: of every triple anybody could build, exactly
        // 13,176 have a legacy bay, and each decodes back to itself. With the
        // loop above that is a bijection, not just a round trip.
        let mut hit = vec![false; V32_INSERT_SPACE];
        for (o, r, b) in candidates() {
            if let Some(flat) = encode_v32(o, r, b) {
                assert!(flat < V32_INSERT_SPACE);
                assert!(!std::mem::replace(&mut hit[flat], true), "{flat} encoded twice");
                assert_eq!(decode_v32(flat), Some((o, r, b)));
            }
        }
        assert!(hit.iter().all(|h| *h));
    }

    #[test]
    fn the_frozen_numbers_are_still_the_live_desks() {
        assert_eq!(V32_CHANNELS, recorder::CHANNELS);
        assert_eq!(V32_STRIPS, recorder::STRIPS);
        assert_eq!(V32_TOTAL_SEATS, TOTAL_SEATS);
        assert_eq!(V32_INSERT_SPACE, INSERT_SPACE);
        assert_eq!(BAYS, recorder::INSERTS);
        assert_eq!(PADS, recorder::DRUM_PADS);
        assert_eq!(AMP_PEDALS, recorder::AMP_PEDALS);
        assert_eq!(MASTER, MASTER_SEAT);
        assert_eq!(MASTER_EXTRA, MASTER_EXTRA_SEAT);
        assert_eq!(EXTRA_PADS, EXTRA_PAD_SEATS);
        assert_eq!(FOLDER_EXTRA, FOLDER_EXTRA_SEATS);
        assert_eq!(PEDALS, AMP_PEDAL_SEATS);
        assert_eq!(BUS_EXTRA, BUS_EXTRA_SEATS);
    }

    /// The strip a legacy owner's slots are drawn on, in `Strip::index` space.
    fn strip_of(o: LegacyOwner) -> usize {
        encode_v32_strip(o).expect("an owner with a strip")
    }

    #[test]
    fn every_live_seat_function_agrees_with_the_decode() {
        for ch in 0..V32_CHANNELS {
            let row = LegacyOwner::Row(ch as u16);
            for pad in 0..PADS {
                assert_eq!(
                    decode_v32_seat(pad_rack_index(ch, pad)),
                    Some((row, Rack::Pad(pad as u8))),
                    "pad_rack_index({ch}, {pad})"
                );
            }
            assert_eq!(decode_v32_seat(folder_extra_seat(ch)), Some((row, Rack::Chain(1))));
            for p in 0..AMP_PEDALS {
                assert_eq!(
                    decode_v32_seat(amp_pedal_seat(ch, p)),
                    Some((row, Rack::Pedal(p as u8))),
                    "amp_pedal_seat({ch}, {p})"
                );
            }
        }
        for d in V32_CHANNELS..V32_STRIPS {
            assert_eq!(
                decode_v32_seat(bus_extra_seat(d)),
                Some((decode_v32_strip(d).unwrap(), Rack::Chain(1)))
            );
        }
        for pad in 0..PADS {
            assert_eq!(
                decode_v32_seat(pad_insert_index(pad)),
                Some((LegacyOwner::Row(0), Rack::Pad(pad as u8)))
            );
        }
        // The two owner questions, for EVERY seat and a margin past the end.
        for seat in 0..V32_TOTAL_SEATS + 200 {
            let decoded = decode_v32_seat(seat);
            let shown = match decoded {
                Some((o, Rack::Chain(_))) => Some(strip_of(o)),
                _ => None,
            };
            assert_eq!(rack_owner(seat), shown, "rack_owner({seat})");
            let board = match decoded {
                Some((LegacyOwner::Row(r), Rack::Chain(_))) => Some(r as usize),
                _ => None,
            };
            assert_eq!(pedal_owner(seat), board, "pedal_owner({seat})");
            let folder = match decoded {
                Some((LegacyOwner::Row(r), Rack::Chain(1))) => Some(r as usize),
                _ => None,
            };
            assert_eq!(folder_extra_of(seat), folder, "folder_extra_of({seat})");
            let pedal = match decoded {
                Some((LegacyOwner::Row(r), Rack::Pedal(p))) => Some((r as usize, p as usize)),
                _ => None,
            };
            assert_eq!(amp_pedal_of(seat), pedal, "amp_pedal_of({seat})");
            let bus_extra = match decoded {
                Some((o, Rack::Chain(1))) if bus_offset(o).is_some() => Some(strip_of(o)),
                _ => None,
            };
            assert_eq!(bus_extra_of(seat), bus_extra, "bus_extra_of({seat})");
        }
    }

    /// **`slot_key` reproduces `seat_bay` for every (desk index, slot)**, the
    /// master's extra-first order included, and `display_slot` inverts it.
    #[test]
    fn display_slots_reproduce_seat_bay_on_every_strip() {
        let ids = identity_ids(V32_CHANNELS);
        for desk_index in 0..=V32_STRIPS {
            let node = bind(decode_v32_strip(desk_index).unwrap(), &ids).unwrap();
            for slot in 0..BAYS * STRIP_RACKS {
                let (seat, bay) = seat_bay(desk_index, slot);
                let sb = slot_key(node, slot).unwrap();
                let owner = unbind(sb.seat.node, &ids).unwrap();
                assert_eq!(
                    encode_v32(owner, sb.seat.rack, sb.bay),
                    Some(seat * BAYS + bay),
                    "seat_bay({desk_index}, {slot}) vs {sb}"
                );
                assert_eq!(display_slot(sb), Some(slot));
            }
        }
    }

    /// **`auto_key` is the persistence order**: a channel's bays through
    /// `seat_bay` (which agrees with it off the master), and the master's
    /// through the `master_bay` / `param_slot` rule, original rack first.
    /// `param_slot` itself is `pub(crate)` in the binary and out of reach from
    /// here; this is its rule spelled with the live constants it reads.
    #[test]
    fn automation_bays_reproduce_the_persisted_order() {
        let ids = identity_ids(V32_CHANNELS);
        let flat = |sb: SeatBay| {
            encode_v32(unbind(sb.seat.node, &ids).unwrap(), sb.seat.rack, sb.bay).unwrap()
        };
        for lane in 0..V32_CHANNELS {
            for bay in 0..BAYS * MAX_CHAIN {
                let (seat, b) = seat_bay(lane, bay);
                assert_eq!(flat(auto_key(Node::Track(ids[lane]), bay).unwrap()), seat * BAYS + b);
            }
        }
        for bay in 0..BAYS * MAX_CHAIN {
            let (seat, b) = if bay < recorder::INSERTS {
                (MASTER_SEAT, bay)
            } else {
                (MASTER_EXTRA_SEAT, bay - recorder::INSERTS)
            };
            assert_eq!(flat(auto_key(Node::Master, bay).unwrap()), seat * BAYS + b, "master bay {bay}");
        }
        // And the bus seats a persisted bay names, uniformly.
        for d in V32_CHANNELS..V32_STRIPS {
            let node = bind(decode_v32_strip(d).unwrap(), &ids).unwrap();
            for bay in 0..BAYS * MAX_CHAIN {
                let (seat, b) = seat_bay(d, bay);
                assert_eq!(flat(auto_key(node, bay).unwrap()), seat * BAYS + b);
            }
        }
    }

    /// The strip decode is `Strip::index`'s inverse, the master being the one
    /// past `Strip::all()` as `Column::desk_index` writes it.
    #[test]
    fn the_strip_decode_is_strip_index_inverted() {
        for strip in recorder::Strip::all() {
            let expect = match strip {
                recorder::Strip::Channel(c) => LegacyOwner::Row(c as u16),
                recorder::Strip::Drums => LegacyOwner::Drums,
                recorder::Strip::Click => LegacyOwner::Click,
                recorder::Strip::Fx => LegacyOwner::Fx,
            };
            assert_eq!(decode_v32_strip(strip.index()), Some(expect), "{strip:?}");
        }
        assert_eq!(recorder::Strip::all().len(), V32_STRIPS);
        assert_eq!(decode_v32_strip(MASTER_SEAT), Some(LegacyOwner::Master));
    }

    #[test]
    fn strips_bind_and_unbind_through_identity() {
        for strip in 0..=V32_STRIPS {
            let o = decode_v32_strip(strip).unwrap();
            assert_eq!(encode_v32_strip(o), Some(strip));
        }
        assert_eq!(decode_v32_strip(V32_STRIPS + 1), None);
        assert_eq!(encode_v32_strip(LegacyOwner::Row(32)), None);
        assert_eq!(decode_v32_strip(32), Some(LegacyOwner::Drums));
        assert_eq!(decode_v32_strip(35), Some(LegacyOwner::Master));

        // Identity ids: row r is TrackId(r).
        let ids = identity_ids(40);
        assert_eq!(ids.len(), 40);
        assert!(ids.iter().enumerate().all(|(r, id)| id.0 as usize == r));
        assert_eq!(bind(LegacyOwner::Row(7), &ids), Some(Node::Track(TrackId(7))));
        assert_eq!(bind(LegacyOwner::Row(40), &ids), None);
        assert_eq!(bind(LegacyOwner::Master, &[]), Some(Node::Master));

        // Under a non-identity list a row binds by POSITION.
        let shuffled = [TrackId(9), TrackId(3), TrackId(41)];
        assert_eq!(bind(LegacyOwner::Row(2), &shuffled), Some(Node::Track(TrackId(41))));
        assert_eq!(unbind(Node::Track(TrackId(3)), &shuffled), Some(LegacyOwner::Row(1)));
        assert_eq!(unbind(Node::Track(TrackId(4)), &shuffled), None);
        for o in [LegacyOwner::Drums, LegacyOwner::Click, LegacyOwner::Fx, LegacyOwner::Master] {
            assert_eq!(unbind(bind(o, &shuffled).unwrap(), &shuffled), Some(o));
        }
        // A row past the legacy width unbinds but has no legacy seat.
        assert_eq!(unbind(Node::Track(TrackId(35)), &ids), Some(LegacyOwner::Row(35)));
        assert_eq!(encode_v32(LegacyOwner::Row(35), Rack::Chain(0), 0), None);
    }

    /// Spot values from Appendix A, as literals: the ones a misread table
    /// would get wrong first.
    #[test]
    fn the_appendix_spot_values() {
        use LegacyOwner::*;
        let at = |seat: usize| decode_v32_seat(seat).unwrap();
        assert_eq!(at(0), (Row(0), Rack::Chain(0)));
        assert_eq!(at(31), (Row(31), Rack::Chain(0)));
        assert_eq!(at(32), (Drums, Rack::Chain(0)));
        assert_eq!(at(34), (Fx, Rack::Chain(0)));
        assert_eq!(at(35), (Master, Rack::Chain(0)));
        assert_eq!(at(36), (Row(0), Rack::Pad(0)));
        assert_eq!(at(163), (Row(0), Rack::Pad(127)));
        assert_eq!(at(164), (Master, Rack::Chain(1)));
        assert_eq!(at(165), (Row(1), Rack::Pad(0)));
        assert_eq!(at(4132), (Row(31), Rack::Pad(127)));
        assert_eq!(at(4133), (Row(0), Rack::Chain(1)));
        assert_eq!(at(4164), (Row(31), Rack::Chain(1)));
        assert_eq!(at(4165), (Row(0), Rack::Pedal(0)));
        assert_eq!(at(4388), (Row(31), Rack::Pedal(6)));
        assert_eq!(at(4389), (Drums, Rack::Chain(1)));
        assert_eq!(at(4391), (Fx, Rack::Chain(1)));
        assert_eq!(decode_v32_seat(4392), None);
        assert_eq!(decode_v32(164 * 3 + 2), Some((Master, Rack::Chain(1), 2)));
        // Never a pedal or pad off a row, and never a third chain.
        assert_eq!(encode_v32(Master, Rack::Pad(0), 0), None);
        assert_eq!(encode_v32(Drums, Rack::Pedal(0), 0), None);
        assert_eq!(encode_v32(Row(0), Rack::Chain(2), 0), None);
        assert_eq!(encode_v32(Row(0), Rack::Chain(0), 3), None);
    }
}
