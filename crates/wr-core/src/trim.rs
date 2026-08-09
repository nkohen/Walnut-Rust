// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Dead-state removal.
//!
//! Ports `Automata/FA/Trimmer.java`: keeps exactly the states that are BOTH
//! forward-reachable from `q0` AND backward-co-reachable to some accepting state
//! (i.e. can still reach acceptance), then densely renumbers the survivors. This is a
//! *different* reachability check from [`crate::minimize`]'s own accepting-states-
//! forward pass — `trim` is the only place that checks reachability from `q0`; running
//! `minimize` alone does not substitute for it.

use crate::fa::Fa;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Removes states unreachable from `q0` or unable to reach any accepting state, and
/// densely renumbers the survivors (stable by ascending old id, matching Java's
/// `Trimmer.quotient`). If no state survives (the language is empty), returns a
/// canonical 1-state non-accepting automaton — matching Java's "make Walnut happy"
/// special case, since a 0-state automaton isn't supported elsewhere in this port
/// either.
///
/// # The TRUE/FALSE short-circuit (U0)
///
/// Java's guard is `if (a.isTRUE_FALSE_AUTOMATON() || a.getQ() <= 1) return;`
/// (`Trimmer.java:31-33`). Only the **first half** is added here, and it is genuinely
/// load-bearing rather than decorative: a trivial automaton left behind by
/// `AutomatonQuantification`'s all-tracks-quantified path has a stale non-zero `q` with
/// an EMPTY `d` (see `crate::fa`'s module docs), so without this guard
/// `forward_reachable` would index `fa.d[fa.q0]` out of bounds and panic — exactly the
/// `IndexOutOfBoundsException` Java's own guard prevents.
///
/// The `getQ() <= 1` half is deliberately NOT added: this crate has always run the full
/// algorithm on 1-state automata, which is language-preserving but not
/// structure-identical (a 1-state automaton whose language is empty is rebuilt as the
/// canonical fully-self-looping 1-state sink instead of being returned untouched).
/// That is a pre-existing, already-reviewed divergence; changing it is out of this
/// unit's scope and would silently perturb every existing `trim` call site.
pub fn trim(fa: &Fa) -> Fa {
    if fa.is_true_false_automaton() {
        return fa.clone();
    }
    if fa.q == 0 {
        return fa.clone();
    }

    let forward = forward_reachable(fa);
    let backward = backward_co_reachable_to_accepting(fa);

    let keep: Vec<usize> = (0..fa.q).filter(|&s| forward[s] && backward[s]).collect();

    if keep.is_empty() {
        let mut d0 = BTreeMap::new();
        for sym in 0..fa.alphabet_size as i32 {
            d0.insert(sym, vec![0]);
        }
        return Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: fa.alphabet_size,
            o: vec![0],
            d: vec![d0],
        };
    }

    let old_to_new: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new_id, &old_id)| (old_id, new_id))
        .collect();

    let new_d: Vec<BTreeMap<i32, Vec<usize>>> = keep
        .iter()
        .map(|&old_id| {
            let mut row = BTreeMap::new();
            for (&sym, dests) in &fa.d[old_id] {
                let mapped: Vec<usize> = dests
                    .iter()
                    .filter_map(|d| old_to_new.get(d).copied())
                    .collect();
                if !mapped.is_empty() {
                    row.insert(sym, mapped);
                }
            }
            row
        })
        .collect();

    Fa {
        true_false: None,
        // Safe: keep.is_empty() was handled above, and if keep is nonempty then q0
        // must be in it — any forward-reachable state that's also backward-reachable
        // proves q0 itself can reach acceptance (via q0 -> ... -> that state -> ... ->
        // an accepting state), so q0 is backward-reachable too, and q0 is trivially
        // forward-reachable from itself.
        q0: *old_to_new
            .get(&fa.q0)
            .expect("q0 is always in `keep` when `keep` is nonempty"),
        q: keep.len(),
        alphabet_size: fa.alphabet_size,
        o: keep.iter().map(|&old_id| fa.o[old_id]).collect(),
        d: new_d,
    }
}

fn forward_reachable(fa: &Fa) -> Vec<bool> {
    let mut seen = vec![false; fa.q];
    let mut queue = VecDeque::new();
    seen[fa.q0] = true;
    queue.push_back(fa.q0);
    while let Some(s) = queue.pop_front() {
        for dests in fa.d[s].values() {
            for &d in dests {
                if !seen[d] {
                    seen[d] = true;
                    queue.push_back(d);
                }
            }
        }
    }
    seen
}

fn backward_co_reachable_to_accepting(fa: &Fa) -> Vec<bool> {
    let mut reverse: Vec<Vec<usize>> = vec![Vec::new(); fa.q];
    for (s, row) in fa.d.iter().enumerate() {
        for dests in row.values() {
            for &d in dests {
                reverse[d].push(s);
            }
        }
    }
    let mut seen = vec![false; fa.q];
    let mut queue = VecDeque::new();
    for (s, seen_s) in seen.iter_mut().enumerate() {
        if fa.is_accepting(s) {
            *seen_s = true;
            queue.push_back(s);
        }
    }
    while let Some(s) = queue.pop_front() {
        for &p in &reverse[s] {
            if !seen[p] {
                seen[p] = true;
                queue.push_back(p);
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn drops_unreachable_and_dead_end_states() {
        // 4 states: 0 (start) -> 1 (accepting); 2 unreachable from q0; 3 reachable
        // from q0 but a dead end (self-loops, never accepting) — 3 must be dropped
        // even though it IS forward-reachable, since it can't reach acceptance.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        d0.insert(1, vec![3]);
        let d1 = BTreeMap::new(); // accepting, no outgoing transitions
        let mut d2 = BTreeMap::new();
        d2.insert(0, vec![1]);
        let mut d3 = BTreeMap::new();
        d3.insert(0, vec![3]);
        d3.insert(1, vec![3]);
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 4,
            alphabet_size: 2,
            o: vec![0, 1, 0, 0],
            d: vec![d0, d1, d2, d3],
        };
        let trimmed = trim(&fa);
        assert_eq!(trimmed.q, 2, "only q0 and the accepting state survive");
        assert!(trimmed.accepts_word(&[0]));
        assert!(!trimmed.accepts_word(&[1])); // led to the dropped dead-end state 3
    }

    #[test]
    fn empty_language_collapses_to_canonical_one_state() {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 1,
            o: vec![0],
            d: vec![d0],
        };
        let trimmed = trim(&fa);
        assert_eq!(trimmed.q, 1);
        assert!(trimmed.is_language_empty());
    }

    #[test]
    fn trivial_automaton_passes_through_untouched() {
        // `Trimmer.trimAutomaton:31` (U0). Uses the STALE-`q` shape specifically --
        // `q > 0` with an empty `d` -- because that is the one the guard actually saves
        // from an out-of-bounds index in `forward_reachable`; the `q == 0` shape below
        // is already handled by the next guard down.
        let fa = Fa {
            true_false: Some(true),
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![],
            d: vec![],
        };
        let trimmed = trim(&fa);
        assert!(trimmed.is_true_false_automaton() && trimmed.is_true_automaton());
        assert_eq!(trimmed.q, 3, "left exactly as-is, stale q included");
    }

    #[test]
    fn zero_state_automaton_passes_through() {
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 0,
            alphabet_size: 2,
            o: vec![],
            d: vec![],
        };
        let trimmed = trim(&fa);
        assert_eq!(trimmed.q, 0);
    }

    fn arb_nfa_fixed_alphabet(q_max: usize, alphabet_size: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max).prop_flat_map(move |q| {
            let row_strategy =
                prop::collection::vec(prop::collection::vec(any::<bool>(), q), alphabet_size);
            let table_strategy = prop::collection::vec(row_strategy, q);
            let o_strategy = prop::collection::vec(0i32..=1, q);
            (table_strategy, o_strategy).prop_map(move |(table, o)| {
                let d = table
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .enumerate()
                            .filter_map(|(sym, incl)| {
                                let dests: Vec<usize> = incl
                                    .into_iter()
                                    .enumerate()
                                    .filter_map(|(dest, keep)| keep.then_some(dest))
                                    .collect();
                                if dests.is_empty() {
                                    None
                                } else {
                                    Some((sym as i32, dests))
                                }
                            })
                            .collect::<BTreeMap<i32, Vec<usize>>>()
                    })
                    .collect();
                Fa {
                    true_false: None,
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
        /// Tier-4 property #5 (DESIGN.md §5): trim preserves language on words the
        /// original automaton actually accepts/rejects — random automata from this
        /// generator routinely contain states unreachable from q0 (destinations are
        /// picked from the full state universe), so this exercises real dead weight,
        /// not just a hand-picked case.
        #[test]
        fn trim_preserves_language(
            fa in arb_nfa_fixed_alphabet(5, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let trimmed = trim(&fa);
            prop_assert_eq!(fa.accepts_word(&word), trimmed.accepts_word(&word));
        }

        /// Trimming twice is the same as trimming once (no further states to drop),
        /// modulo state renumbering — checked via accepted words, since exact state
        /// identity isn't the invariant (this project compares by language).
        #[test]
        fn trim_is_idempotent_on_language(
            fa in arb_nfa_fixed_alphabet(5, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let once = trim(&fa);
            let twice = trim(&once);
            prop_assert_eq!(once.accepts_word(&word), twice.accepts_word(&word));
        }
    }
}
