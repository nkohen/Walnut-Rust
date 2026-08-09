// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! The semantic language-equivalence oracle — the comparison bar for all differential
//! and golden testing (`CLAUDE.md`'s Prime Directive: compare by SEMANTIC
//! LANGUAGE-EQUIVALENCE, never by byte/structural identity; this is the Rust-native
//! analogue of Walnut's own `EqualityUtils.faEqual`, which uses Brics language
//! equivalence, not text/state-numbering identity).
//!
//! [`complement`], [`product_dfa`], and [`language_equivalent`] all require their
//! inputs be total DFAs ([`Fa::is_deterministic_and_total`]) — checked as a hard
//! [`EquivError`], never `debug_assert!` (see the `fa` module docs on this regression
//! class). A silently-wrong oracle is the single highest-severity defect class in this
//! crate: every other tier of testing is judged against it.
//!
//! Scope note: acceptance is treated as a plain 0/1 bit here (`Fa::is_accepting`), not
//! Walnut's more general DFAO word-output value — matches this crate's current scope
//! (predicate automata only); revisit when DFAO support lands.
//!
//! **Known gap, flagged by the Phase-1 spike's final integration review (not yet
//! fixed): this module only checks [`Fa::alphabet_size`] (an integer), never symbol
//! *content* or per-track *order*.** `Fa` is deliberately track-agnostic (symbols are
//! pre-encoded integers, see the `fa` module docs), so it has no way to know whether
//! "symbol 1" means the same digit tuple on both sides being compared — that's a
//! property of the `automaton::Automaton` layer (its `alphabet`/`encoder`), which
//! this module never sees. A caller comparing two `Automaton`s built from
//! *differently-ordered* alphabets could get a confidently wrong "equivalent" or
//! "not equivalent" verdict with no diagnostic. Every call site so far (this crate's
//! own tests, `wr-logic`'s, and `tests/differential`) happens to compare
//! same-alphabet automata, so this hasn't produced a wrong answer yet — but nothing
//! enforces it. `tests/differential/tests/spike_ei_i_lt_x.rs` works around this by
//! asserting `Automaton::alphabet` equality explicitly before calling into here;
//! that pattern (or a real content-aware check added to `Automaton`, since `Fa` alone
//! can't express one) should be revisited before this oracle is used for genuinely
//! multi-track differential testing (Phase 3+).

use crate::fa::Fa;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub enum EquivError {
    /// An input automaton was not deterministic-and-total.
    NotTotalDfa,
    /// The two automata being combined have different alphabet sizes — the
    /// comparison is meaningless without a shared symbol encoding.
    MismatchedAlphabet,
}

/// Complements a total DFA: flips every state's accept bit. `fa` must already be a
/// total DFA (see module docs on the 0/1-acceptance scope limit).
pub fn complement(fa: &Fa) -> Result<Fa, EquivError> {
    if !fa.is_deterministic_and_total() {
        return Err(EquivError::NotTotalDfa);
    }
    Ok(Fa {
        q0: fa.q0,
        q: fa.q,
        alphabet_size: fa.alphabet_size,
        o: fa.o.iter().map(|&o| if o == 0 { 1 } else { 0 }).collect(),
        d: fa.d.clone(),
    })
}

/// Cross-product of two total DFAs over the same alphabet size, with acceptance of a
/// combined state `(i, j)` decided by `accept(a.is_accepting(i), b.is_accepting(j))`.
/// Both inputs must already be total DFAs.
pub fn product_dfa(a: &Fa, b: &Fa, accept: impl Fn(bool, bool) -> bool) -> Result<Fa, EquivError> {
    if !a.is_deterministic_and_total() || !b.is_deterministic_and_total() {
        return Err(EquivError::NotTotalDfa);
    }
    if a.alphabet_size != b.alphabet_size {
        return Err(EquivError::MismatchedAlphabet);
    }
    let alphabet_size = a.alphabet_size;
    let pair_id = |i: usize, j: usize| i * b.q + j;
    let q = a.q * b.q;
    let mut o = vec![0i32; q];
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); q];
    for i in 0..a.q {
        for j in 0..b.q {
            let pid = pair_id(i, j);
            o[pid] = i32::from(accept(a.is_accepting(i), b.is_accepting(j)));
            for sym in 0..alphabet_size as i32 {
                // Safe to index directly: both inputs were just confirmed
                // deterministic-and-total above, so every symbol in `0..alphabet_size`
                // has exactly one destination.
                let ai = a.d[i][&sym][0];
                let bj = b.d[j][&sym][0];
                d[pid].insert(sym, vec![pair_id(ai, bj)]);
            }
        }
    }
    Ok(Fa {
        q0: pair_id(a.q0, b.q0),
        q,
        alphabet_size,
        o,
        d,
    })
}

/// True iff `a` and `b` recognize the same language. Both must be total DFAs. Computed
/// as emptiness of the symmetric difference (`product_dfa` with `accept = XOR`) — the
/// same construction named in `docs/DESIGN.md` §5 Tier 1 as the required `wr-core`
/// deliverable.
pub fn language_equivalent(a: &Fa, b: &Fa) -> Result<bool, EquivError> {
    let sym_diff = product_dfa(a, b, |x, y| x != y)?;
    Ok(sym_diff.is_language_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeMap as Map;

    /// Generates a random small total DFA: `q0 = 0`, `q` states, `alphabet_size`
    /// symbols, every `(state, symbol)` pair mapped to some destination in `0..q`
    /// (transitions need not be "meaningful" — only totality is required for the
    /// oracle's precondition). `q_max`/`alpha_max` must both be at least 1.
    fn arb_total_dfa(q_max: usize, alpha_max: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max, 1..=alpha_max).prop_flat_map(|(q, alphabet_size)| {
            let o_strategy = prop::collection::vec(0i32..=1, q);
            let trans_strategy =
                prop::collection::vec(prop::collection::vec(0usize..q, alphabet_size), q);
            (o_strategy, trans_strategy).prop_map(move |(o, trans)| {
                let d = trans
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .enumerate()
                            .map(|(sym, dest)| (sym as i32, vec![dest]))
                            .collect::<Map<i32, Vec<usize>>>()
                    })
                    .collect();
                Fa {
                    q0: 0,
                    q,
                    alphabet_size,
                    o,
                    d,
                }
            })
        })
    }

    proptest! {
        /// Tier-4 property #1 (DESIGN.md §5): oracle self-consistency. A DFA is
        /// always equivalent to itself, and flipping a REACHABLE state's acceptance
        /// (here, q0 — always reachable via the empty word) always breaks
        /// equivalence.
        #[test]
        fn oracle_self_consistency(fa in arb_total_dfa(6, 3)) {
            prop_assert_eq!(language_equivalent(&fa, &fa), Ok(true));

            let mut flipped = fa.clone();
            flipped.o[flipped.q0] = if flipped.o[flipped.q0] == 0 { 1 } else { 0 };
            prop_assert_eq!(language_equivalent(&fa, &flipped), Ok(false));
        }
    }

    fn contains_one_dfa() -> Fa {
        let mut d0 = Map::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = Map::new();
        d1.insert(0, vec![1]);
        d1.insert(1, vec![1]);
        Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, d1],
        }
    }

    /// A differently-STATE-NUMBERED DFA recognizing the exact same language as
    /// `contains_one_dfa` (states swapped, transitions/outputs adjusted to match) —
    /// exercises that equivalence is by language, not by structure/numbering.
    fn contains_one_dfa_relabeled() -> Fa {
        let mut d0 = Map::new(); // was state 1 (accepting)
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        let mut d1 = Map::new(); // was state 0 (non-accepting, start)
        d1.insert(0, vec![1]);
        d1.insert(1, vec![0]);
        Fa {
            q0: 1,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 0],
            d: vec![d0, d1],
        }
    }

    fn reject_all_dfa() -> Fa {
        let mut d0 = Map::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        Fa {
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0],
            d: vec![d0],
        }
    }

    #[test]
    fn complement_flips_acceptance() {
        let fa = contains_one_dfa();
        let comp = complement(&fa).unwrap();
        assert!(!comp.is_accepting(1));
        assert!(comp.is_accepting(0));
        assert!(!comp.accepts_word(&[1]));
        assert!(comp.accepts_word(&[0, 0]));
    }

    #[test]
    fn complement_rejects_non_total_input() {
        let mut fa = contains_one_dfa();
        fa.d[0].remove(&1);
        assert_eq!(complement(&fa).unwrap_err(), EquivError::NotTotalDfa);
    }

    #[test]
    fn language_equivalent_self() {
        let fa = contains_one_dfa();
        assert_eq!(language_equivalent(&fa, &fa), Ok(true));
    }

    #[test]
    fn language_equivalent_ignores_state_numbering() {
        let a = contains_one_dfa();
        let b = contains_one_dfa_relabeled();
        assert_eq!(language_equivalent(&a, &b), Ok(true));
    }

    #[test]
    fn language_equivalent_detects_real_difference() {
        let a = contains_one_dfa();
        let b = reject_all_dfa();
        assert_eq!(language_equivalent(&a, &b), Ok(false));
    }

    #[test]
    fn language_equivalent_flipping_one_output_breaks_equivalence() {
        let a = contains_one_dfa();
        let mut b = contains_one_dfa();
        b.o[1] = 0; // flip the only accepting state to non-accepting
        assert_eq!(language_equivalent(&a, &b), Ok(false));
    }

    #[test]
    fn product_dfa_rejects_mismatched_alphabets() {
        let a = contains_one_dfa();
        // A total DFA over a genuinely different (3-symbol) alphabet, not just a
        // mislabeled 2-symbol one — must be `is_deterministic_and_total` itself so
        // the mismatch check (not the totality check) is what's actually exercised.
        let mut d0 = Map::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        d0.insert(2, vec![0]);
        let b = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 3,
            o: vec![0],
            d: vec![d0],
        };
        assert!(b.is_deterministic_and_total());
        assert_eq!(
            product_dfa(&a, &b, |x, y| x == y).unwrap_err(),
            EquivError::MismatchedAlphabet
        );
    }
}
