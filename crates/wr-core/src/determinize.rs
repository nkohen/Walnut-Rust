// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Subset-construction determinization (`SC`, Walnut's default strategy) and
//! Brzozowski double-reversal determinization (`BRZ`).
//!
//! Ports `Automata/FA/DeterminizationStrategies.java`'s `SC` and `Brz`/`brzStep`
//! methods — the deferred `CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`/`OTF` strategies (see
//! `docs/DESIGN.md` §9 F3) are out of scope for this crate.
//!
//! **`SC` does NOT totalize.** A metastate/symbol pair with an empty union of
//! destinations is simply omitted from the output, matching Java's `SC` exactly —
//! totalization is a separate, explicit operation ([`crate::fa::Fa::totalize`]), never
//! conflated with determinize. Callers that need a total DFA (e.g. the equivalence
//! oracle) must totalize explicitly after determinizing.

use crate::fa::Fa;
use crate::minimize::MinimizeError;
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
        true_false: None,
        q0: 0,
        q: metastate_list.len(),
        alphabet_size: fa.alphabet_size,
        o,
        d,
    }
}

/// `DeterminizationStrategies.Brz`/`brzStep` (`Brz`: `DeterminizationStrategies.java:140-149`;
/// `brzStep`: `:151-163`), restricted to the `BRZ`->`SC` path (`strategy.removeBrzozowski()`
/// maps `BRZ` to `SC`; the `BRZ_CCL`/`BRZ_CCLS`->OTF paths are out of scope, see module
/// docs and `docs/BOUNDARY-MAP.md`'s confirmation that this split is clean: `brzStep`
/// never calls `OTF(...)` on the `SC`-mapped path). `initial` is the ORIGINAL
/// automaton's initial-state set (ordinarily `{fa.q0}`; Brzozowski's algorithm and
/// other callers may pass a genuine multi-state seed, same as [`subset_construction`]).
///
/// # Precondition NOT enforced here (lives in the not-yet-ported dispatcher)
///
/// Java's `DeterminizationStrategies.determinize` refuses `BRZ` (and every non-`SC`
/// strategy) on a DFAO: `if (strategy != SC) { if (fa.isFAO()) throw ... }`
/// (`:115-118`, `isFAO` = "some state's output is `> 1`", `FA.java:65-71`). This
/// function has no such guard and will silently collapse a DFAO's real output values
/// to plain 0/1 acceptance (via [`subset_construction`]'s/[`crate::minimize::minimize`]'s
/// binary-output handling) instead of erroring. The guard belongs in the not-yet-ported
/// `determinize` dispatcher (a future `wr-cli`/strategy-selection unit), not here —
/// noted so that unit doesn't accidentally skip it.
///
/// # Sequence
///
/// Matching Java's `Brz` body exactly: reverse -> `SC` -> `justMinimize()` -> reverse
/// (from the MINIMIZED result's own `q0`, not the original `initial` parameter —
/// Java's comment: "Note that initial state is now q0") -> `SC`. **The final `SC` is
/// NOT followed by another minimize** — Java's `Brz` calls `fa.justMinimize()` exactly
/// once, between the two `brzStep`s, not after the second one.
///
/// # Why this still yields the minimal DFA without a final minimize
///
/// The classical theorem is: `SC(reverse(A))` is the MINIMAL DFA of `L(A)`-reversed
/// *whenever `A` is deterministic and every one of `A`'s states is reachable from its
/// initial state* — minimization of the intermediate is NOT the hypothesis (an
/// earlier version of this doc claimed the theorem holds "regardless of whether the
/// intermediate was minimized", which is true here but for a different, unstated
/// reason, and is false in general: reverse-then-determinize a deterministic but
/// UNREACHABLE-states-having automaton and the result need not be minimal). What
/// actually discharges the hypothesis on both applications here: [`subset_construction`]
/// always emits only the part reachable from its own `q0` (state `0`) by construction,
/// and [`crate::minimize::minimize`] preserves that reachability on an
/// already-reachable input (Valmari's partition refinement never reintroduces a
/// pruned-away state) — so both the first and second `SC` call always receive a
/// reachable, deterministic input, satisfying the real hypothesis. This is also why
/// the `minimize.rs` WB-001 quirk (`q0` not co-reachable to acceptance) cannot fire on
/// the intermediate: a reachable automaton with any accepting state has that state
/// reachable from `q0`, hence `q0` is co-reachable to it. The mid-sequence minimize
/// is therefore a genuine performance optimization only (it shrinks what feeds the
/// potentially-exponential SECOND subset construction — note it's the FIRST one, on
/// the raw reversed input, that has no such shrinking and is the more surprising cost
/// center) — never a correctness requirement. Pinned by
/// `brzozowski_yields_the_minimal_dfa_cross_checked_against_direct_minimize` below
/// (DESIGN.md §5 Tier 4's named "Brzozowski double-reversal = minimal DFA,
/// cross-checked against the direct minimizer" property).
///
/// # Errors and panics
///
/// Returns `Err` if the intermediate `justMinimize()` call's preconditions somehow
/// fail — see [`crate::minimize::minimize`]'s documented `MinimizeError` variants
/// (both `NotDeterministic` and `ConflictingTransitions` are possible in principle,
/// though neither can actually occur here: [`subset_construction`]'s output is always
/// deterministic and reachable by construction, so the first `SC`'s result always
/// satisfies `minimize`'s preconditions). Like [`crate::minimize::minimize`] and
/// [`crate::trim::trim`], this crate generally guards the degenerate `fa.q == 0`
/// case — this function does NOT (matching Java, which throws
/// `IndexOutOfBoundsException` at the equivalent spot): a 0-state `fa` with a
/// non-empty `initial` panics inside [`crate::fa::Fa::reverse`].
pub fn brzozowski(fa: &Fa, initial: &BTreeSet<usize>) -> Result<Fa, MinimizeError> {
    debug_assert!(
        fa.o.iter().all(|&o| o <= 1),
        "brzozowski: caller must reject DFAOs first, matching DeterminizationStrategies.\
         determinize's dispatcher-level guard (DeterminizationStrategies.java:115-118) \
         -- this function has no way to error cleanly on one itself"
    );
    // Step 1 (`brzStep(fa, initialStates, SC, "Reverse")`): reverse, then SC.
    let mut reversed = fa.clone();
    let new_initial = reversed.reverse(initial);
    let determinized = subset_construction(&reversed, &new_initial);

    // `fa.justMinimize()`.
    let minimized = crate::minimize::minimize(&determinized)?;

    // Step 2 (`brzStep(fa, IntSet.of(fa.getQ0()), SC, "Reverse of reverse")`): seed
    // with the NEW automaton's `q0` (Java's comment: "Note that initial state is now
    // q0" — i.e. after minimizing, re-seed from the minimized result's own `q0`, not
    // the original `initial` parameter), reverse again, then SC. No minimize after
    // this one (see doc comment above).
    let mut reversed_again = minimized.clone();
    let seed2: BTreeSet<usize> = [reversed_again.q0].into_iter().collect();
    let new_initial2 = reversed_again.reverse(&seed2);
    Ok(subset_construction(&reversed_again, &new_initial2))
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
            true_false: None,
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
            true_false: None,
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

    // --- Brzozowski (DeterminizationStrategies.Brz/brzStep) ---

    #[test]
    fn brzozowski_matches_the_input_nfas_language_on_contains_one() {
        let nfa = contains_one_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let brz = brzozowski(&nfa, &initial).unwrap();
        assert!(brz.is_deterministic());
        for word in [vec![], vec![0, 0, 0], vec![1], vec![0, 1, 0], vec![1, 1, 1]] {
            assert_eq!(
                nfa.accepts_word(&word),
                brz.accepts_word(&word),
                "mismatch on {word:?}"
            );
        }
    }

    #[test]
    fn brzozowski_result_is_already_minimal_on_contains_one() {
        // `contains_one_nfa`'s minimal DFA has exactly 2 states: state 0
        // (non-accepting) --1--> state 1 (accepting, self-loops on everything) --
        // state 0 is NOT a sink (it does leave, on symbol 1; an earlier version of
        // this comment wrongly called it one). A hand-derived instance of the
        // property test below, pinning a concrete expected state count.
        let nfa = contains_one_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let brz = brzozowski(&nfa, &initial).unwrap();
        assert_eq!(brz.q, 2);
    }

    #[test]
    fn brzozowski_seeds_the_second_reversal_from_the_minimized_q0_not_the_original_initial() {
        // Adversarial-review finding (mutation-tested): every OTHER test's
        // intermediate (reverse -> SC -> minimize) result happens to land back at
        // `q0 == 0`, so a mutant that seeds the second reversal from the ORIGINAL
        // `initial` parameter instead of the minimized result's own `q0` passes them
        // all unchanged. This fixture's intermediate lands at `q0 == 1`, catching it.
        // L(fa) = {"", "0"}: q0 accepting, --0--> state 1 (accepting, dead end).
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 1],
            d: vec![d0, BTreeMap::new()],
        };
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let brz = brzozowski(&fa, &initial).unwrap();
        assert!(brz.accepts_word(&[]), "empty word must be accepted");
        assert!(brz.accepts_word(&[0]), "\"0\" must be accepted");
        assert!(!brz.accepts_word(&[1]), "\"1\" must be rejected");
        assert!(!brz.accepts_word(&[0, 0]), "\"00\" must be rejected");
    }

    #[test]
    fn brzozowski_handles_the_no_accepting_states_case() {
        // `Fa::reverse` returns the empty set here (no state was accepting before
        // the call) -- this is the first path in the crate that can hand
        // `subset_construction` a genuinely EMPTY `initial` set. A very plausible
        // real input (the "reject everything" automaton), introduced by this unit,
        // previously untested.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0],
            d: vec![d0],
        };
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let brz = brzozowski(&fa, &initial).unwrap();
        assert_eq!(brz.q, 1);
        assert!(brz.is_language_empty());
    }

    /// Picks a genuinely nondeterministic-seed-capable strategy: `q` states plus a
    /// NONEMPTY subset of `0..q` to use as a multi-state `initial` set (adversarial-
    /// review finding: every other test/proptest in this module seeds with the
    /// singleton `{fa.q0}` only, even though [`brzozowski`]'s own doc comment
    /// advertises multi-state seed support and a real caller — `wr-logic`'s
    /// `fix_leading_zeros` — passes one).
    fn arb_fa_and_nonempty_seed(
        q_max: usize,
        alphabet_size: usize,
    ) -> impl Strategy<Value = (Fa, BTreeSet<usize>)> {
        arb_nfa_fixed_alphabet(q_max, alphabet_size).prop_flat_map(|fa| {
            let q = fa.q;
            prop::collection::hash_set(0..q, 1..=q)
                .prop_map(move |seed| (fa.clone(), seed.into_iter().collect::<BTreeSet<usize>>()))
        })
    }

    proptest! {
        /// DESIGN.md §5 Tier 4's named property: "Brzozowski double-reversal =
        /// minimal DFA, cross-checked against the direct minimizer." Compares
        /// `brzozowski`'s result against BOTH the input NFA's own language (via
        /// `accepts_word`, the same anchor `determinize_preserves_language` uses
        /// above) AND `minimize(subset_construction(fa, seed))` (SC then an explicit
        /// final minimize) on two further axes: same language via the semantic-
        /// equivalence oracle (after totalizing both -- `subset_construction`/
        /// `minimize` don't preserve totality) AND same state count (proving
        /// Brzozowski's result is actually MINIMAL, not merely equivalent to
        /// something smaller). Seeded via [`arb_fa_and_nonempty_seed`], so this
        /// exercises genuine multi-state `initial` sets, not just `{fa.q0}`.
        #[test]
        fn brzozowski_yields_the_minimal_dfa_cross_checked_against_direct_minimize(
            (fa, initial) in arb_fa_and_nonempty_seed(4, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let brz = brzozowski(&fa, &initial).unwrap();
            prop_assert!(brz.is_deterministic());

            // Ground truth #1: language, against the input NFA directly (via a
            // reference NFA seeded with the same multi-state `initial`, since `fa`
            // itself only exposes single-`q0`-seeded `accepts_word`).
            let seeded_reference = subset_construction(&fa, &initial);
            prop_assert_eq!(
                seeded_reference.accepts_word(&word),
                brz.accepts_word(&word)
            );

            // Ground truth #2: exact minimality, against a direct SC-then-minimize.
            let direct_minimal = crate::minimize::minimize(&seeded_reference).unwrap();
            prop_assert_eq!(
                brz.q,
                direct_minimal.q,
                "Brzozowski's result must have exactly as many states as the \
                 directly-minimized DFA (it IS the minimal DFA, not just an \
                 equivalent one)"
            );

            let mut brz_total = brz.clone();
            brz_total.totalize(0);
            let mut direct_total = direct_minimal.clone();
            direct_total.totalize(0);
            prop_assert_eq!(
                crate::equiv::language_equivalent(&brz_total, &direct_total),
                Ok(true)
            );
        }
    }
}
