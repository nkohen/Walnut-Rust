// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Existential quantification (∃-elimination) — the logic layer's entry point.
//!
//! # What this module is, after U6
//!
//! The Phase-1 spike implemented the whole ∃-projection pipeline here, because
//! `docs/BOUNDARY-MAP.md` §4.3 recommended porting `Automata/AutomatonQuantification.java`
//! into `wr-logic` — that recommendation only traced `AutomatonQuantification`'s
//! OUTGOING calls. The U6 architecture unit found the missing INCOMING edge that
//! overturns it (not previously recorded anywhere): `wr-core`'s `NumberSystem` calls
//! `AutomatonQuantification.quantify` ten times to build its base-*k* adder/comparator
//! automata by quantifying carry variables away, so a `wr-logic`-resident implementation
//! would force `wr-core` to depend on `wr-logic` — a genuine Cargo cycle, since
//! `wr-logic` must depend on `wr-core`. `docs/BOUNDARY-MAP.md` §4.3 and `docs/DESIGN.md`'s
//! crate-mapping table are updated to record this as superseding the original
//! recommendation.
//!
//! The projection primitive therefore now lives in [`wr_core::quantify`] (read that
//! module's docs for the full argument and for every ported quirk/divergence), and
//! [`exists`] is a thin delegation to it. Nothing was lost in the move: the Phase-1
//! implementation contained no formula/AST-level logic at all — no `Token`, no
//! `Predicate`, no parser state — only "delete these tracks from every symbol,
//! determinize, fix up leading zeros", which is automaton theory over
//! [`wr_core::automaton::Automaton`].
//!
//! What will give this module its own content is the formula-level quantifier
//! elimination that is still to come: `∀ = ¬∃¬`, and the quantifier bookkeeping over a
//! parsed `Predicate` that decides *which* labels to hand [`exists`] in the first place.
//!
//! U6 also removed this module's ad-hoc copy of `AutomatonLogicalOps`'s
//! `fixLeadingZerosProblem`/`zeroReachableStates`: [`wr_core::logicalops`] carries the
//! general port, and [`wr_core::quantify::quantify`] calls it. The regression tests
//! written here against those helpers (and against `NumberSystem.determineMsd`, now
//! [`wr_core::numsys::determine_msd`]) were deliberately left in place, unchanged, since
//! they exist to protect *this* pipeline.

use std::collections::BTreeSet;
use wr_core::automaton::Automaton;

pub use wr_core::quantify::QuantifyError;

/// Existentially quantifies `labels` out of `a`, in place — `wr-logic`'s entry point to
/// [`wr_core::quantify::quantify`], which is where the algorithm, its error contract and
/// its ported-quirk documentation all live.
pub fn exists(a: &mut Automaton, labels: &BTreeSet<String>) -> Result<(), QuantifyError> {
    wr_core::quantify::quantify(a, labels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    // U6: these four used to be module-level items/imports of this file. The tests below
    // are byte-for-byte what they were before the move, so the names they reference are
    // re-imported here rather than the test bodies being rewritten.
    use std::collections::BTreeMap;
    use wr_core::fa::Fa;
    use wr_core::logicalops::zero_reachable_states;
    use wr_core::numsys::{determine_msd, less_than_msd};

    fn labels(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// Encodes a single-track word (one digit per position) through `a`'s own encoder,
    /// so tests never hand-compute symbol ids.
    fn word1(a: &Automaton, digits: &[i32]) -> Vec<i32> {
        digits.iter().map(|&d| a.encode(&[d])).collect()
    }

    /// A 2-track automaton over tracks `y` (quantified) and `x`, digits `{0, 1}` each,
    /// built from an explicit transition list of `((state, y, x), dest)`.
    fn two_track(q: usize, o: Vec<i32>, edges: &[((usize, i32, i32), usize)]) -> Automaton {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q,
                alphabet_size: 4,
                o,
                d: vec![BTreeMap::new(); q],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["y".to_string(), "x".to_string()],
            vec![Some(true), Some(true)],
        );
        for &((from, y, x), to) in edges {
            let sym = a.encode(&[y, x]);
            a.fa.d[from].entry(sym).or_default().push(to);
        }
        a
    }

    // ---------------------------------------------------------------- spike criterion

    /// **The Phase-1 spike's exit criterion.** `∃i (i < x)` over msd base-2 must be
    /// exactly "`x` is nonzero", i.e. "the msd-first representation of `x` contains a 1".
    ///
    /// Hand-derivation of the intermediate NFA (2-track symbol = `i + 2x`): state 0
    /// self-loops on `(0,0)`=0 and `(1,1)`=3, goes to state 1 on `(0,1)`=2, and has no
    /// edge for `(1,0)`=1. Projecting `i` away maps symbols `{0,1} -> 0` and `{2,3} -> 1`,
    /// so state 0 gets `0 -> {0}` and `1 -> {1} ∪ {0}` — a genuine two-symbol collapse
    /// requiring destination merging. The result is the classic 2-state "contains a 1".
    #[test]
    fn exists_i_less_than_x_is_x_nonzero() {
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];

        exists(&mut a, &labels(&["i"])).unwrap();

        assert_eq!(a.label, vec!["x".to_string()]);
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.msd, vec![Some(true)]);
        assert_eq!(a.fa.alphabet_size, 2);
        assert_eq!(a.fa.q, 2, "expected the minimal 'contains a 1' DFA");
        assert!(a.fa.is_deterministic());

        // x = 0 in any number of digits: rejected.
        assert!(!a.fa.accepts_word(&word1(&a, &[0])));
        assert!(!a.fa.accepts_word(&word1(&a, &[0, 0, 0])));
        // Any representation containing a 1: accepted (msd-first, leading zeros allowed).
        assert!(a.fa.accepts_word(&word1(&a, &[1])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 1, 0])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 0, 1])));
        assert!(a.fa.accepts_word(&word1(&a, &[1, 1])));
        // The empty representation denotes no value at all: rejected.
        assert!(!a.fa.accepts_word(&word1(&a, &[])));
    }

    // ------------------------------------------------------------------- error paths

    #[test]
    fn unknown_label_is_not_a_free_variable() {
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];
        assert_eq!(
            exists(&mut a, &labels(&["z"])),
            Err(QuantifyError::NotFreeVariable("z".to_string()))
        );
        // The automaton is untouched by the rejected call.
        assert_eq!(a.label, vec!["i".to_string(), "x".to_string()]);
        assert_eq!(a.fa.q, 2);
    }

    /// U0 changed this behavior: quantifying away EVERY track used to be a hard
    /// `QuantifyError::AllTracksQuantified` (the Phase-1 stand-in for a representation
    /// this crate lacked), and now produces Java's real answer, a TRUE/FALSE automaton
    /// — `AutomatonQuantification.java:58-65`. The test is kept (not deleted) and
    /// re-pointed at the new contract, which is strictly stronger: it checks the truth
    /// VALUE, not just that the call succeeded.
    #[test]
    fn quantifying_every_track_yields_a_true_false_automaton() {
        // `∃i ∃x (i < x)` over msd base-2: satisfiable (e.g. i=0, x=1), so TRUE.
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];
        assert!(!a.is_empty(), "sanity: L(i < x) is non-empty");

        exists(&mut a, &labels(&["i", "x"])).unwrap();

        assert!(a.is_true_false_automaton());
        assert!(a.is_true_automaton(), "∃i ∃x (i < x) is TRUE");
        // `Automaton.clear()` wiped the track metadata (`AutomatonQuantification:63`).
        assert!(a.alphabet.is_empty());
        assert!(a.label.is_empty());
        assert!(a.msd.is_empty());
        assert_eq!(a.get_arity(), 0);
        assert!(!a.is_empty(), "the TRUE automaton's language is not empty");
    }

    /// The FALSE half of the same branch: an empty-language input must quantify to the
    /// FALSE automaton, not the TRUE one. Without this, a `!A.isEmpty()` accidentally
    /// written as `A.isEmpty()` (or evaluated AFTER the flag is set, which would make
    /// `isEmpty` take its own trivial branch and always answer "empty") would pass the
    /// test above.
    #[test]
    fn quantifying_every_track_of_an_empty_language_yields_false() {
        // Two tracks, no accepting state at all: L = ∅.
        let mut a = two_track(2, vec![0, 0], &[((0, 0, 0), 1)]);
        assert!(a.is_empty(), "sanity: this automaton accepts nothing");

        exists(&mut a, &labels(&["y", "x"])).unwrap();

        assert!(a.is_true_false_automaton());
        assert!(!a.is_true_automaton(), "∃y ∃x (false) is FALSE");
        assert!(a.is_empty());
    }

    /// Quantifying anything out of an ALREADY-trivial automaton is a no-op that leaves
    /// it trivial — Java's `quantifyHelper` bails at its `A.getLabel().isEmpty()` check
    /// (`:50-52`) and `quantify` then returns at `:39` without consulting
    /// `determineMsd` or running any zero fixup.
    #[test]
    fn quantifying_an_already_trivial_automaton_is_a_noop() {
        for truth in [true, false] {
            let mut a = Automaton::true_false(truth);
            exists(&mut a, &labels(&["anything"])).unwrap();
            assert!(a.is_true_false_automaton());
            assert_eq!(a.is_true_automaton(), truth);

            let mut a = Automaton::true_false(truth);
            exists(&mut a, &BTreeSet::new()).unwrap();
            assert!(a.is_true_false_automaton());
            assert_eq!(a.is_true_automaton(), truth);
        }
    }

    #[test]
    fn lsd_tracks_are_rejected_rather_than_mishandled() {
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];
        a.msd = vec![Some(false), Some(false)];
        assert_eq!(
            exists(&mut a, &labels(&["i"])),
            Err(QuantifyError::UnsupportedLsdFixup)
        );
        // The projection itself still happened (Java sequences it before the fixup).
        assert_eq!(a.label, vec!["x".to_string()]);
    }

    #[test]
    fn non_arithmetic_or_mixed_tracks_skip_the_fixup() {
        assert_eq!(determine_msd(&[Some(true), Some(true)]), Some(true));
        assert_eq!(determine_msd(&[Some(false), Some(false)]), Some(false));
        assert_eq!(determine_msd(&[Some(true), Some(false)]), None);
        assert_eq!(determine_msd(&[Some(true), None]), None);
        assert_eq!(determine_msd(&[None]), None);
        // Java's loop never runs on an empty list, leaving the `true` initializer. See
        // `determine_msd`'s docs on when this is actually reachable through `exists`
        // (an already label-less input automaton, not "quantified away every track" —
        // that path is now a hard error).
        assert_eq!(determine_msd(&[]), Some(true));
    }

    #[test]
    fn zero_state_automaton_is_a_noop_not_a_panic() {
        // Legal to construct via this crate's API even though a real Walnut Automaton
        // never reaches it. Regression test for a reviewer-found panic: quantify_helper
        // used to index the stale q0 into an empty transition table via
        // trim -> subset_construction.
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 0,
                alphabet_size: 4,
                o: vec![],
                d: vec![],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["y".to_string(), "x".to_string()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(exists(&mut a, &labels(&["y"])), Ok(()));
        assert_eq!(
            a.fa.q, 0,
            "left untouched, matching fix_leading_zeros's precedent"
        );
    }

    // ---------------------------------------------------- symbol collapse / merging

    /// Two old symbols collapse onto one new symbol *and* their destinations must be
    /// unioned for the language to survive. Original 2-track language (over `(y, x)`):
    /// `{(0,0)}` and `{(1,0), (·,1)}`. Projecting `y` gives state 0 the single new symbol
    /// `x = 0` with destinations `{1, 2}` — state 1 accepts immediately, state 2 needs one
    /// more `x = 1`. Keeping only the first destination loses `"01"`; keeping only the
    /// last loses `"0"`. Asserting both are accepted pins the union.
    #[test]
    fn collapsing_symbols_union_their_destinations() {
        let mut a = two_track(
            4,
            vec![0, 1, 0, 1],
            &[
                ((0, 0, 0), 1),
                ((0, 1, 0), 2),
                ((2, 0, 1), 3),
                ((2, 1, 1), 3),
            ],
        );

        exists(&mut a, &labels(&["y"])).unwrap();

        assert!(a.fa.accepts_word(&word1(&a, &[0])), "lost the first branch");
        assert!(
            a.fa.accepts_word(&word1(&a, &[0, 1])),
            "lost the second branch"
        );
        assert!(!a.fa.accepts_word(&word1(&a, &[1, 1])));
        assert!(!a.fa.accepts_word(&word1(&a, &[0, 1, 0])));
    }

    // ------------------------------------------------------------- >2-track survivors

    /// Regression coverage for the `kept`/`new_label`/`new_alphabet`/reduced-digit-tuple
    /// index bookkeeping when MORE THAN ONE track survives and the quantified track is
    /// neither first nor last. `two_track`-based tests above always leave exactly one
    /// (trivially "ordered") survivor, so a track-order bug in the general N-track case
    /// would go undetected by them.
    ///
    /// 3 tracks `a`, `b` (quantified), `c`; the only accepting transition is on the
    /// EXACT symbol `(a=1, b=0, c=0)` — deliberately asymmetric in `a` vs `c` (unlike an
    /// equality- or sum-based predicate), so a bug that silently reordered the
    /// surviving tracks to `[c, a]` instead of `[a, c]` would flip which reduced symbol
    /// is accepted, not just how many states exist.
    #[test]
    fn quantifying_a_middle_track_preserves_survivor_order() {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 8,
                o: vec![0, 1],
                d: vec![BTreeMap::new(), BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1], vec![0, 1]],
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            vec![Some(true), Some(true), Some(true)],
        );
        let sym = a.encode(&[1, 0, 0]); // a=1, b=0, c=0
        a.fa.d[0].entry(sym).or_default().push(1);

        exists(&mut a, &labels(&["b"])).unwrap();

        assert_eq!(a.label, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(a.alphabet, vec![vec![0, 1], vec![0, 1]]);

        // Only (a=1, c=0) is accepted. (a=0, c=1) is the swapped pair — accepting it
        // instead is exactly the symptom a [c, a] order bug would produce.
        assert!(a.fa.accepts_word(&[a.encode(&[1, 0])]));
        assert!(!a.fa.accepts_word(&[a.encode(&[0, 1])]));
        assert!(!a.fa.accepts_word(&[a.encode(&[0, 0])]));
        assert!(!a.fa.accepts_word(&[a.encode(&[1, 1])]));
    }

    // ------------------------------------------- zeroReachableStates + its mutation

    #[test]
    fn zero_reachable_states_forces_a_q0_self_loop() {
        // q0 has no zero-transition at all going in.
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![BTreeMap::from([(1, vec![1])]), BTreeMap::new()],
        };
        let reached = zero_reachable_states(&mut fa, 0);
        assert_eq!(reached, BTreeSet::from([0]));
        assert_eq!(
            fa.d[0].get(&0),
            Some(&vec![0]),
            "the (q0, zero) -> q0 self-loop must be written into the real table"
        );
    }

    #[test]
    fn zero_reachable_states_is_a_multi_step_closure_and_does_not_duplicate() {
        // 0 -0-> 1 -0-> 2, and 0 already self-loops on zero.
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 1,
            o: vec![0, 0, 1],
            d: vec![
                BTreeMap::from([(0, vec![0, 1])]),
                BTreeMap::from([(0, vec![2])]),
                BTreeMap::new(),
            ],
        };
        let reached = zero_reachable_states(&mut fa, 0);
        assert_eq!(reached, BTreeSet::from([0, 1, 2]), "BFS must be transitive");
        assert_eq!(
            fa.d[0].get(&0),
            Some(&vec![0, 1]),
            "an existing self-loop must not be duplicated"
        );
    }

    /// End-to-end proof that the forced self-loop reaches the *language*: the original
    /// automaton accepts exactly the one-position word `(y=0, x=1)`, and `q0` has no
    /// zero-transition whatsoever. Without the mutation, subset construction from `{q0}`
    /// would yield "exactly one digit, and it is 1" — with it, arbitrarily many leading
    /// zeros are absorbed.
    #[test]
    fn leading_zero_fixup_absorbs_leading_zeros_via_the_forced_self_loop() {
        let mut a = two_track(2, vec![0, 1], &[((0, 0, 1), 1)]);
        assert!(
            !a.fa.d[0].contains_key(&a.encode(&[0, 0])),
            "precondition: q0 has no zero-transition before quantification"
        );

        exists(&mut a, &labels(&["y"])).unwrap();

        assert!(a.fa.accepts_word(&word1(&a, &[1])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 1])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 0, 0, 1])));
        assert!(!a.fa.accepts_word(&word1(&a, &[0])));
        assert!(!a.fa.accepts_word(&word1(&a, &[1, 1])));
    }

    /// Faithfully-ported quirk: `quantify` consults `determineMsd` and runs the fixup even
    /// when `quantifyHelper` short-circuited on an empty label set, so `exists(a, &{})`
    /// still rewrites the automaton. Same 2-track automaton as above, nothing quantified:
    /// the language gains leading `(0,0)` symbols it did not have.
    #[test]
    fn empty_label_set_still_runs_the_zero_fixup() {
        let mut a = two_track(2, vec![0, 1], &[((0, 0, 1), 1)]);
        let zero = a.encode(&[0, 0]);
        let one = a.encode(&[0, 1]);
        assert!(!a.fa.accepts_word(&[zero, one]));

        exists(&mut a, &BTreeSet::new()).unwrap();

        assert_eq!(a.label, vec!["y".to_string(), "x".to_string()]);
        assert!(a.fa.accepts_word(&[one]));
        assert!(
            a.fa.accepts_word(&[zero, zero, one]),
            "the fixup ran despite nothing being quantified"
        );
    }

    // --------------------------------------------------------------- Tier-4 properties

    /// Random 2-track NFA over tracks `y`/`x` with digits `{0, 1}` (so `alphabet_size` is
    /// 4), `q0 = 0`.
    ///
    /// `pin_zero_class` forces `q0`'s two "x-digit is 0" symbols (`(0,0)` and `(1,0)`) to
    /// map to exactly `{q0}`. That makes the *projected* automaton's `(q0, zero)` row
    /// exactly `{q0}`, which makes `fixLeadingZerosProblem` provably language-neutral
    /// (the zero-closure of `q0` is `{q0}`, and the forced self-loop already exists) — so
    /// the property below can compare against a pure ∃-projection oracle with no
    /// leading-zero closure baked into it. The fixup's own behavior is covered by the
    /// hand tests above and by `quantified_language_is_closed_under_leading_zeros`, which
    /// uses the unpinned generator.
    fn arb_two_track(q_max: usize, pin_zero_class: bool) -> impl Strategy<Value = Automaton> {
        (1..=q_max).prop_flat_map(move |q| {
            let o = prop::collection::vec(0i32..=1, q);
            let table = prop::collection::vec(
                prop::collection::vec(prop::collection::vec(any::<bool>(), q), 4),
                q,
            );
            (o, table).prop_map(move |(o, table)| {
                let d: Vec<BTreeMap<i32, Vec<usize>>> = table
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
                                (!dests.is_empty()).then_some((sym as i32, dests))
                            })
                            .collect()
                    })
                    .collect();
                let mut a = Automaton::new(
                    Fa {
                        true_false: None,
                        q0: 0,
                        q,
                        alphabet_size: 4,
                        o,
                        d,
                    },
                    vec![vec![0, 1], vec![0, 1]],
                    vec!["y".to_string(), "x".to_string()],
                    vec![Some(true), Some(true)],
                );
                if pin_zero_class {
                    for y in [0, 1] {
                        let sym = a.encode(&[y, 0]);
                        a.fa.d[0].insert(sym, vec![0]);
                    }
                }
                a
            })
        })
    }

    /// Independent ground truth for `∃y`: does *some* `y`-word of the same length make the
    /// ORIGINAL automaton accept? Brute force over all `2^|x|` candidate `y`-words, using
    /// only `Fa::accepts_word` and `Automaton::encode` — it never calls `exists` or any of
    /// its helpers, so it is a genuine oracle rather than a re-derivation.
    fn brute_force_exists_y(original: &Automaton, x_digits: &[i32]) -> bool {
        let n = x_digits.len();
        for mask in 0..(1u32 << n) {
            let word: Vec<i32> = (0..n)
                .map(|i| {
                    let y = ((mask >> i) & 1) as i32;
                    original.encode(&[y, x_digits[i]])
                })
                .collect();
            if original.fa.accepts_word(&word) {
                return true;
            }
        }
        false
    }

    proptest! {
        /// Tier-4 (CLAUDE.md §correctness ladder, DESIGN.md §5): ∃-elimination against an
        /// independent brute-force oracle. This is the analogue of `minimize`'s
        /// Moore-reference cross-check — the property that catches a *wrong* projection,
        /// as opposed to language-preservation properties an identity function satisfies.
        #[test]
        fn quantify_matches_brute_force_existential(
            original in arb_two_track(4, true),
            x_digits in prop::collection::vec(0i32..2, 0..4),
        ) {
            let mut a = original.clone();
            exists(&mut a, &labels(&["y"])).unwrap();

            let word: Vec<i32> = x_digits.iter().map(|&d| a.encode(&[d])).collect();
            prop_assert_eq!(
                a.fa.accepts_word(&word),
                brute_force_exists_y(&original, &x_digits),
                "mismatch on x = {:?}", x_digits
            );
        }

        /// The invariant `fixLeadingZerosProblem` exists to establish, checked on the
        /// UNPINNED generator (where the fixup genuinely does work): the quantified
        /// automaton's language is closed under adding *and* removing a leading zero.
        ///
        /// This holds because the final DFA's start metastate `Z0` satisfies
        /// `δ(Z0, zero) = Z0`: `⊆` because `Z0` is the zero-closure of `q0`, and `⊇`
        /// because every `q ∈ δ(q0, 0^b)` with `b ≥ 1` is in `δ(Z0, zero)` while `q0`
        /// itself is, thanks to the forced self-loop. Drop the mutation and the `⊇`
        /// direction fails.
        #[test]
        fn quantified_language_is_closed_under_leading_zeros(
            original in arb_two_track(4, false),
            x_digits in prop::collection::vec(0i32..2, 0..4),
        ) {
            let mut a = original;
            exists(&mut a, &labels(&["y"])).unwrap();

            let zero = a.encode(&[0]);
            let word: Vec<i32> = x_digits.iter().map(|&d| a.encode(&[d])).collect();
            let mut padded = vec![zero];
            padded.extend_from_slice(&word);

            prop_assert_eq!(a.fa.accepts_word(&word), a.fa.accepts_word(&padded));
        }

        /// Structural post-conditions of `exists`: the quantified track is gone from every
        /// parallel metadata list, the alphabet size matches the surviving tracks, and the
        /// result is a DFA (projection introduces nondeterminism; determinization must
        /// remove it again).
        #[test]
        fn quantify_leaves_a_well_formed_single_track_dfa(original in arb_two_track(4, false)) {
            let mut a = original;
            exists(&mut a, &labels(&["y"])).unwrap();

            prop_assert_eq!(a.label.len(), 1);
            prop_assert_eq!(a.label[0].as_str(), "x");
            prop_assert_eq!(a.alphabet.len(), 1);
            prop_assert_eq!(a.msd, vec![Some(true)]);
            prop_assert_eq!(a.fa.alphabet_size, 2);
            prop_assert_eq!(a.fa.d.len(), a.fa.q);
            prop_assert!(a.fa.is_deterministic());
            prop_assert!(a.fa.q0 < a.fa.q);
        }
    }
}
