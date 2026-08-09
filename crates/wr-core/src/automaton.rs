// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! A minimal multi-track automaton wrapper.
//!
//! Ports the pieces of `Automata/Automaton` + `Automata/RichAlphabet` that ∃-projection
//! ([`crate` sibling `wr-logic`'s `quantify`, not yet landed at the time of writing —
//! see `docs/BOUNDARY-MAP.md` §4.3]) structurally requires: per-track alphabets,
//! labels, msd/lsd-ness, and the mixed-radix symbol encoder/decoder. **This is
//! deliberately NOT full Java parity** — no `NumberSystem` objects attached per track,
//! no DFAO/`combine` bookkeeping, no `TRUE_FALSE_AUTOMATON` modeling (see
//! `docs/DESIGN.md` §8 Phase 1's spike scope; widen in Phase 2).
//!
//! # Symbol encoding (`RichAlphabet.encode`/`decode`)
//!
//! A transition symbol is one integer encoding a simultaneous digit-tuple across all
//! tracks, mixed-radix with **track 0 fastest-varying**: `encoder[i]` = product of the
//! alphabet sizes of tracks `0..i`, and `encode(digits) = Σ encoder[i] * index_of(digits[i]
//! in alphabet[i])` — indexing is by **position in the track's alphabet list, not by the
//! literal digit value** (this matters once a track's alphabet isn't a contiguous `0..k`
//! range). Replicate this exactly, or downstream product/quantify logic (keyed on
//! encoded ints) silently misaligns tracks.

use crate::fa::Fa;

/// A multi-track automaton: the raw [`Fa`] plus enough track metadata to encode/decode
/// symbols and (for `wr-logic`) know which tracks are quantifiable and how to fix up
/// leading/trailing zeros after projection.
#[derive(Debug, Clone)]
pub struct Automaton {
    pub fa: Fa,
    /// Per-track alphabet, e.g. `[0, 1, ..., base-1]` for an ordinary base-*k* track.
    pub alphabet: Vec<Vec<i32>>,
    /// Per-track variable name (e.g. `"i"`, `"x"`), parallel to `alphabet`.
    pub label: Vec<String>,
    /// Per-track msd (`Some(true)`)/lsd (`Some(false)`)/non-arithmetic (`None`) — parallel
    /// to `alphabet`. Mirrors `NumberSystem.determineMsd`'s three-way outcome (mixed
    /// arithmetic tracks or no arithmetic tracks both yield `None`/"skip the zero fixup").
    pub msd: Vec<Option<bool>>,
    /// `encoder[i]` = product of `alphabet[0..i]`'s sizes (`encoder[0] == 1`). Cached at
    /// construction, matching Java's `RichAlphabet.encoder` (there computed lazily on
    /// first `encode()` call; here eagerly, since this crate always needs it).
    encoder: Vec<usize>,
}

impl Automaton {
    /// Builds an `Automaton` from an already-constructed [`Fa`] and track metadata.
    /// `alphabet`, `label`, and `msd` must have the same length as each other and match
    /// `fa.alphabet_size` (`Π alphabet[i].len() == fa.alphabet_size`) — not asserted here
    /// (this is a Phase-1 slice, not a validating constructor); callers are responsible.
    pub fn new(
        fa: Fa,
        alphabet: Vec<Vec<i32>>,
        label: Vec<String>,
        msd: Vec<Option<bool>>,
    ) -> Self {
        let encoder = Self::compute_encoder(&alphabet);
        Automaton {
            fa,
            alphabet,
            label,
            msd,
            encoder,
        }
    }

    fn compute_encoder(alphabet: &[Vec<i32>]) -> Vec<usize> {
        let mut encoder = Vec::with_capacity(alphabet.len());
        let mut val = 1usize;
        for track in alphabet {
            encoder.push(val);
            val *= track.len();
        }
        encoder
    }

    /// Encodes a per-track digit tuple into a single transition symbol. Panics if a
    /// digit isn't present in its track's alphabet (a caller bug, not a data error —
    /// matches Java's `List.indexOf` returning `-1` and corrupting the arithmetic
    /// silently; panicking here is the improvement `PORTING.md`'s type/error mapping
    /// table calls for over stringly-typed Java exceptions).
    pub fn encode(&self, digits: &[i32]) -> i32 {
        let mut encoding: i32 = 0;
        for (i, &d) in digits.iter().enumerate() {
            let idx = self.alphabet[i]
                .iter()
                .position(|&v| v == d)
                .unwrap_or_else(|| panic!("digit {d} not in track {i}'s alphabet"));
            encoding += self.encoder[i] as i32 * idx as i32;
        }
        encoding
    }

    /// Decodes a transition symbol back into its per-track digit tuple. Inverse of
    /// [`Automaton::encode`] (`decode(encode(x)) == x`).
    pub fn decode(&self, mut sym: i32) -> Vec<i32> {
        let mut out = Vec::with_capacity(self.alphabet.len());
        for track in &self.alphabet {
            let size = track.len() as i32;
            let idx = sym.rem_euclid(size);
            out.push(track[idx as usize]);
            sym = sym.div_euclid(size);
        }
        out
    }

    /// The encoded symbol for the all-digit-value-0 tuple (`RichAlphabet.determineZero`)
    /// — used by the leading/trailing-zero fixup pass to find "read a 0 on every live
    /// track" edges.
    ///
    /// Simplified from Java's literal `encode([A[i].indexOf(0) for each i])`: Java
    /// looks up the *position* of value 0 in each track, then re-encodes that position
    /// as if it were itself a digit *value* — a double indirection that only coincides
    /// with directly encoding the literal all-zero tuple when 0 sits at position 0 in
    /// every track's alphabet (true for every alphabet this crate constructs so far —
    /// ordinary base-*k* tracks are `[0, 1, ..., k-1]`). Revisit if a non-zero-first
    /// track alphabet is ever introduced.
    pub fn determine_zero(&self) -> i32 {
        let zero_digits = vec![0; self.alphabet.len()];
        self.encode(&zero_digits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    fn trivial_fa(alphabet_size: usize) -> Fa {
        Fa {
            q0: 0,
            q: 1,
            alphabet_size,
            o: vec![0],
            d: vec![BTreeMap::new()],
        }
    }

    #[test]
    fn encode_matches_hand_derived_mixed_radix_value() {
        // Two tracks, base 3 each: encoder = [1, 3]. digits [2, 1] -> 2*1 + 1*3 = 5.
        let a = Automaton::new(
            trivial_fa(9),
            vec![vec![0, 1, 2], vec![0, 1, 2]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(a.encode(&[2, 1]), 5);
        assert_eq!(a.encode(&[0, 0]), 0);
        assert_eq!(a.encode(&[2, 2]), 8);
    }

    #[test]
    fn decode_matches_hand_derived_digits() {
        let a = Automaton::new(
            trivial_fa(9),
            vec![vec![0, 1, 2], vec![0, 1, 2]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(a.decode(5), vec![2, 1]);
        assert_eq!(a.decode(0), vec![0, 0]);
        assert_eq!(a.decode(8), vec![2, 2]);
    }

    #[test]
    fn encode_uses_index_in_alphabet_not_literal_value() {
        // A non-contiguous, non-zero-first track alphabet: [5, 7, 2]. Index of 2 is 2,
        // index of 5 is 0 — encode must key off POSITION, not the literal digit value.
        let a = Automaton::new(
            trivial_fa(3),
            vec![vec![5, 7, 2]],
            vec!["a".into()],
            vec![None],
        );
        assert_eq!(a.encode(&[5]), 0);
        assert_eq!(a.encode(&[7]), 1);
        assert_eq!(a.encode(&[2]), 2);
    }

    #[test]
    fn determine_zero_is_zero_for_standard_base_k_alphabet() {
        let a = Automaton::new(
            trivial_fa(9),
            vec![vec![0, 1, 2], vec![0, 1, 2]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(a.determine_zero(), 0);
    }

    proptest! {
        /// Tier-4 property #6 (DESIGN.md §5): encode/decode round-trip, including
        /// non-contiguous per-track alphabets — stresses the real
        /// index-in-list-not-literal-value indexing rule, which a round-trip test over
        /// only `0..k` alphabets could pass vacuously even with the indexing backwards.
        #[test]
        fn encode_decode_round_trip(
            // Up to 3 tracks, each a random small set of DISTINCT i32 values (mimicking
            // a real, possibly-non-contiguous alphabet), plus a digit tuple drawn from
            // those same per-track value sets.
            tracks in prop::collection::vec(
                prop::collection::hash_set(-5i32..20, 1..4).prop_map(|s| {
                    let mut v: Vec<i32> = s.into_iter().collect();
                    v.sort_unstable();
                    v
                }),
                1..4,
            ),
        ) {
            let seed = 0xC0FFEEu64;
            let mut state = seed;
            let mut digits = Vec::with_capacity(tracks.len());
            for track in &tracks {
                // Deterministic xorshift, no external RNG dependency needed for a
                // same-input-derived index pick.
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                digits.push(track[(state as usize) % track.len()]);
            }
            let a = Automaton::new(
                trivial_fa(tracks.iter().map(|t| t.len()).product()),
                tracks,
                vec!["t".into(); digits.len()],
                vec![None; digits.len()],
            );
            let sym = a.encode(&digits);
            prop_assert_eq!(a.decode(sym), digits);
        }
    }
}
