//! **A set of lanes with no width written into its type.**
//!
//! Every lane mask in the engine today is a `u32` or a `u64`, and both are
//! full at the width the desk happens to have. Past it, `1 << n` is not an
//! error in a release build: it wraps, and lane 32 answers for lane 0. The
//! clamps that were added to stop the panic in debug builds (`.min(31)`,
//! `.min(63)`) are worse, because they fold every lane past the edge onto the
//! last one, so muting lane 40 mutes lane 31 too.
//!
//! A `LaneSet` is a bit per lane in as many words as the desk needs, bought
//! ONCE, at a capacity the host chooses. Nothing here allocates after
//! [`LaneSet::with_capacity`]: the audio thread may insert, remove, test,
//! clear and iterate, and the one thing it may not do is grow it. That is what
//! makes "past capacity" an answer rather than a reallocation: `insert`
//! refuses and says so, `contains` says no, and nothing aliases.
//!
//! The capacity is whole words. A set asked for 33 lanes holds 64, which
//! costs nothing and means the edge is always a word boundary, so iteration
//! never has to mask a partial last word.

/// One bit per lane, in a fixed number of words.
///
/// See the module docs. `Clone` is offered for the UI thread's convenience
/// and allocates; nothing on the audio thread clones one.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct LaneSet {
    words: Box<[u64]>,
}

impl std::fmt::Debug for LaneSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl LaneSet {
    /// A set that can hold lanes `0..lanes`, rounded up to whole words.
    /// The only call here that allocates.
    pub fn with_capacity(lanes: usize) -> Self {
        Self {
            words: vec![0u64; lanes.div_ceil(64)].into_boxed_slice(),
        }
    }

    /// How many lanes this set can hold: always a multiple of 64.
    pub fn capacity(&self) -> usize {
        self.words.len() * 64
    }

    /// Add `lane`. **`false` past capacity**, and the set is unchanged: a
    /// lane the set has no bit for is refused rather than folded onto one it
    /// does, which is the whole difference from `1 << n`.
    pub fn insert(&mut self, lane: usize) -> bool {
        let Some(w) = self.words.get_mut(lane / 64) else {
            return false;
        };
        *w |= 1u64 << (lane % 64);
        true
    }

    /// Take `lane` out. Says whether it was in. A lane past capacity was
    /// never in, so it answers `false` and touches nothing.
    pub fn remove(&mut self, lane: usize) -> bool {
        let Some(w) = self.words.get_mut(lane / 64) else {
            return false;
        };
        let bit = 1u64 << (lane % 64);
        let was = *w & bit != 0;
        *w &= !bit;
        was
    }

    /// Whether `lane` is in. `false` past capacity, never a neighbour's bit.
    pub fn contains(&self, lane: usize) -> bool {
        self.words
            .get(lane / 64)
            .is_some_and(|w| w & (1u64 << (lane % 64)) != 0)
    }

    /// Empty it, keeping the capacity. O(words), no allocation.
    pub fn clear(&mut self) {
        self.words.fill(0);
    }

    /// Whether nothing is in it.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|w| *w == 0)
    }

    /// How many lanes are in it.
    pub fn len(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// The lanes in it, ascending: the order every fold and every walk in
    /// the engine is written in, so a mask walk and a set walk visit lanes in
    /// the same sequence and a render cannot tell them apart.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(i, &w)| {
            let mut rest = w;
            std::iter::from_fn(move || {
                if rest == 0 {
                    return None;
                }
                let bit = rest.trailing_zeros() as usize;
                rest &= rest - 1;
                Some(i * 64 + bit)
            })
        })
    }

    /// Today's `u32` lane mask, as a set of one word. The bridge from the
    /// masks this type replaces, so a walk can be moved over one mask at a
    /// time and proven bit-exact at each step. The walks are all moved, so
    /// only the tests that build a set from a word still ask for one.
    #[cfg(test)]
    pub fn from_u32(mask: u32) -> Self {
        Self::from_u64(u64::from(mask))
    }

    /// Today's `u64` strip mask, as a set of one word. Tests only, as above.
    #[cfg(test)]
    pub fn from_u64(mask: u64) -> Self {
        Self {
            words: vec![mask].into_boxed_slice(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LaneSet;

    /// **Past capacity is a refusal, never a neighbour.** The defect this
    /// type exists to end: `1u32 << 32` wraps to lane 0 in a release build,
    /// and `.min(31)` folds lane 40 onto lane 31. Both are checked against
    /// the lanes they would have hit.
    #[test]
    fn a_lane_past_capacity_is_refused_and_aliases_nothing() {
        let mut s = LaneSet::with_capacity(33);
        assert_eq!(s.capacity(), 64, "rounded up to whole words");
        assert!(s.insert(63));
        assert!(!s.insert(64), "a lane the set has no bit for was accepted");
        assert!(!s.insert(64 + 31), "a lane past the edge was accepted");
        assert!(
            !s.contains(0) && !s.contains(31),
            "a refused lane landed on a neighbour"
        );
        assert!(!s.contains(64));
        assert!(
            !s.remove(64),
            "removing a lane that cannot be in it said it was"
        );
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![63]);
        let none = LaneSet::with_capacity(0);
        assert_eq!(none.capacity(), 0);
        assert!(!none.contains(0) && none.is_empty());
    }

    /// **The set walk visits what the mask walk visits, in the same order**,
    /// at every width the masks have and past them: what lets W2 move the
    /// walks over without a render changing a bit.
    #[test]
    fn a_set_holds_exactly_what_was_put_in_it_ascending() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for cap in [1usize, 32, 33, 64, 65, 200, 512, 4096] {
            let mut s = LaneSet::with_capacity(cap);
            let mut want = vec![false; s.capacity()];
            for _ in 0..cap * 3 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let lane = (x as usize) % cap;
                if x & (1 << 40) == 0 {
                    assert!(s.insert(lane));
                    want[lane] = true;
                } else {
                    assert_eq!(s.remove(lane), want[lane], "remove misreported lane {lane}");
                    want[lane] = false;
                }
            }
            let got: Vec<usize> = s.iter().collect();
            let expect: Vec<usize> = (0..want.len()).filter(|i| want[*i]).collect();
            assert_eq!(got, expect, "capacity {cap}");
            assert_eq!(s.len(), expect.len());
            assert_eq!(s.is_empty(), expect.is_empty());
            for lane in 0..s.capacity() {
                assert_eq!(s.contains(lane), want[lane], "capacity {cap} lane {lane}");
            }
            s.clear();
            assert!(s.is_empty() && s.iter().next().is_none());
            assert_eq!(
                s.capacity(),
                cap.div_ceil(64) * 64,
                "clear changed the capacity"
            );
        }
    }

    /// **The bridges from the masks read every bit, and only those.**
    #[test]
    fn the_masks_cross_over_bit_for_bit() {
        let m32 = 0x8000_0001u32 | (1 << 17);
        let s = LaneSet::from_u32(m32);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0, 17, 31]);
        let m64 = u64::MAX;
        let s = LaneSet::from_u64(m64);
        assert_eq!(s.len(), 64);
        assert_eq!(s.iter().last(), Some(63));
        assert!(!s.contains(64));
        assert!(LaneSet::from_u64(0).is_empty());
    }
}
