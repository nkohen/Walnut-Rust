// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Subset-construction determinization (`SC`, Walnut's default strategy).
//!
//! Ports `Automata/FA/DeterminizationStrategies.java`'s `SC` method only — the other
//! strategies (`BRZ`, and the deferred `CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`/`OTF`, see
//! `docs/DESIGN.md` §9 F3) are out of scope for this crate so far.
//!
//! **Does NOT totalize.** A metastate/symbol pair with an empty union of destinations
//! is simply omitted from the output, matching Java's `SC` exactly — totalization is a
//! separate, explicit operation ([`crate::fa::Fa::totalize`]), never conflated with
//! determinize. Callers that need a total DFA (e.g. the equivalence oracle) must
//! totalize explicitly after determinizing.

use crate::fa::Fa;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Determinizes `fa` via subset construction, starting from the metastate `initial`
/// (a *set* of NFA states, matching Java's generalized multi-initial-state entry point
/// used e.g. by Brzozowski's algorithm — for an ordinary single-initial-state NFA,
/// pass `[fa.q0].into_iter().collect()`).
///
/// Metastates are hash-consed (deduplicated) via `metastate_to_id`, and processed as a
/// worklist that grows by appending newly-discovered metastates — the same
/// array-append-as-worklist shape as the Java `metastateList`, not a separate queue.
pub fn subset_construction(fa: &Fa, initial: &BTreeSet<usize>) -> Fa {
    let mut metastate_list: Vec<BTreeSet<usize>> = vec![initial.clone()];
    let mut metastate_to_id: HashMap<BTreeSet<usize>, usize> = HashMap::new();
    metastate_to_id.insert(initial.clone(), 0);

    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
    let mut cursor = 0;
    while cursor < metastate_list.len() {
        let current = metastate_list[cursor].clone();
        let mut row = BTreeMap::new();
        for sym in 0..fa.alphabet_size as i32 {
            let mut union: BTreeSet<usize> = BTreeSet::new();
            for &q in &current {
                if let Some(dests) = fa.d[q].get(&sym) {
                    union.extend(dests.iter().copied());
                }
            }
            if union.is_empty() {
                // SC does not totalize: no transition is recorded here at all.
                continue;
            }
            let next_id = metastate_list.len();
            let id = *metastate_to_id.entry(union.clone()).or_insert_with(|| {
                metastate_list.push(union);
                next_id
            });
            row.insert(sym, vec![id]);
        }
        d.push(row);
        cursor += 1;
    }

    let o = metastate_list
        .iter()
        .map(|ms| i32::from(ms.iter().any(|&q| fa.is_accepting(q))))
        .collect();

    Fa {
        q0: 0,
        q: metastate_list.len(),
        alphabet_size: fa.alphabet_size,
        o,
        d,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A genuinely nondeterministic 2-state NFA over {0,1}: state 0 (start,
    /// non-accepting) has TWO destinations on symbol 1 (self-loop and to state 1);
    /// state 1 (accepting) self-loops on everything. Recognizes "contains a 1".
    fn contains_one_nfa() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0, 1]); // nondeterministic choice
        let mut d1 = BTreeMap::new();
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

    #[test]
    fn determinize_contains_one_preserves_language() {
        let nfa = contains_one_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let dfa = subset_construction(&nfa, &initial);
        assert!(dfa.is_deterministic());
        for word in [vec![], vec![0, 0, 0], vec![1], vec![0, 1, 0], vec![1, 1, 1]] {
            assert_eq!(
                nfa.accepts_word(&word),
                dfa.accepts_word(&word),
                "mismatch on {word:?}"
            );
        }
    }

    #[test]
    fn determinize_does_not_totalize() {
        // A 1-state NFA with no transition at all on symbol 1.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        let nfa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0],
            d: vec![d0],
        };
        let initial: BTreeSet<usize> = [0].into_iter().collect();
        let dfa = subset_construction(&nfa, &initial);
        assert!(
            !dfa.is_deterministic_and_total(),
            "SC must not fabricate a sink transition"
        );
        assert!(!dfa.d[0].contains_key(&1));
    }

    /// Generates a random small NFA (possibly with real nondeterminism, and possibly
    /// with states unreachable from `q0`) over a FIXED alphabet size, so a
    /// correlated random word can be generated independently and still land on real
    /// symbols.
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
        /// Tier-4 property #2 (DESIGN.md §5): determinize preserves language. Checked
        /// via `accepts_word` (not the equivalence oracle, since the NFA side isn't a
        /// total DFA and the oracle requires one).
        #[test]
        fn determinize_preserves_language(
            fa in arb_nfa_fixed_alphabet(4, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
            let dfa = subset_construction(&fa, &initial);
            prop_assert!(dfa.is_deterministic());
            prop_assert_eq!(fa.accepts_word(&word), dfa.accepts_word(&word));
        }
    }
}
