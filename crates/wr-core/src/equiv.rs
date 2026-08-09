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
//! # Trivial (TRUE/FALSE) automata — U0, and why this was a live wrong-answer path
//!
//! Walnut's own oracle handles them explicitly, and [`language_equivalent`] now ports
//! that logic verbatim from `src/test/java/Main/EqualityUtils.java`'s `faEqual`:
//!
//! ```text
//! if (a.isTRUE_FALSE_AUTOMATON() != b.isTRUE_FALSE_AUTOMATON()) return false;
//! if (a.isTRUE_FALSE_AUTOMATON() && b.isTRUE_FALSE_AUTOMATON()) {
//!   return a.isTRUE_AUTOMATON() == b.isTRUE_AUTOMATON();
//! }
//! // ...otherwise Brics language equivalence
//! ```
//!
//! Note what that means and does not mean: a trivial automaton is compared **only**
//! against another trivial automaton. `faEqual(TRUE, Σ*-over-some-alphabet)` is `false`
//! in Walnut even though the two accept the same words — the flag is part of the
//! compared value, not just an encoding of the language. This port keeps that,
//! deliberately: the golden corpus (85 trivial `automaton*` fixtures) is judged by
//! exactly this predicate, so a "smarter" oracle here would diverge from the bar Tier 1
//! is measured against.
//!
//! Without these branches the check was not merely incomplete, it was **wrong**: a
//! trivial `Fa` has `q == 0` and an empty `d`, on which
//! [`Fa::is_deterministic_and_total`] answers `true` vacuously (both `all()`s range
//! over empty iterators), so [`product_dfa`] would have built a 0-state product whose
//! symmetric difference is trivially empty — reporting the TRUE automaton and the FALSE
//! automaton *equivalent*. [`product_dfa`] therefore also rejects trivial inputs
//! outright now ([`EquivError::TrivialAutomaton`]) rather than relying on every caller
//! to pre-filter.
//!
//! Scope note: acceptance is treated as a plain 0/1 bit here (`Fa::is_accepting`), not
//! Walnut's more general DFAO word-output value — matches this crate's current scope
//! (predicate automata only); revisit when DFAO support lands.
//!
//! **Former known gap, flagged by the Phase-1 spike's final integration review —
//! PARTIALLY closed by [`automaton_language_equivalent`] below; read this carefully,
//! it is narrower than an earlier version of this doc claimed.** The functions above
//! (`complement`/`product_dfa`/`language_equivalent`) only ever operate on bare
//! [`Fa`]s and only check [`Fa::alphabet_size`] (an integer), never symbol *content*
//! or per-track *order* — `Fa` is deliberately track-agnostic (symbols are
//! pre-encoded integers, see the `fa` module docs), so it has no way to know whether
//! "symbol 1" means the same digit tuple on both sides being compared. That is still
//! true today and is not a defect in `Fa`'s design (see its own module docs) — but it
//! made it easy for a caller working at the `automaton::Automaton` layer (which DOES
//! carry `alphabet`/`label`) to accidentally call straight into the `Fa`-level oracle
//! and get a confidently wrong verdict, with no diagnostic.
//!
//! [`automaton_language_equivalent`] closes this ONLY for a mismatched-`alphabet`
//! shape (different arity, different per-track digit lists, or the same digits in a
//! different per-track order) — it checks `a`/`b`'s `Automaton::alphabet` for exact
//! positional equality and returns [`EquivError::MismatchedTrackStructure`] instead of
//! silently delegating to a meaningless `Fa`-level comparison.
//!
//! **Still open, and NOT a narrow corner case — adversarial review confirmed this is
//! a live, demonstrated wrong-answer path, not just a documented inconvenience:**
//! `automaton_language_equivalent` does NOT compare `label`. Two `Automaton`s over
//! DIFFERENT tracks that happen to share the same per-track alphabet CONTENT — e.g.
//! labels `["i", "x"]` vs `["x", "i"]`, both tracks `[0,1]`, which is the *common*
//! shape for same-base multi-track automata, not a rare one — pass the `alphabet`
//! check trivially (`[[0,1],[0,1]] == [[0,1],[0,1]]`) and get compared positionally
//! regardless of what each side's tracks actually mean. This silently returns a
//! confidently WRONG verdict whenever the two automata's tracks are permuted relative
//! to each other, with no error and no diagnostic — pinned deliberately (not just
//! described) by `automaton_language_equivalent_does_not_detect_permuted_labels`
//! below, so this gap cannot silently regress into an even-more-wrong assumption and
//! cannot be forgotten.
//!
//! Why `label` isn't compared, despite that gap: `wr_io`'s `.txt` reader assigns
//! placeholder numeric labels (`"0"`, `"1"`, …) to every automaton it reads (it has no
//! way to recover semantic names from the file format), so a strict `a.label ==
//! b.label` requirement would break exactly the differential-testing use case this
//! function exists for (comparing a freshly-computed `Automaton` against one read
//! back from a `walnut-java`-produced `.txt` file). Also not compared: `msd`
//! (msd/lsd-ness) — an msd-base-*k* and an lsd-base-*k* automaton over an identical
//! `alphabet` denote different numeric relations, and nothing here catches that
//! either. Callers that need real semantic-track alignment (not just "same shape")
//! must `canonize`/`sort_label` both operands to a shared label order themselves, or
//! independently confirm `label`/`msd` agree, before calling in — this function
//! checks structural COMPATIBILITY of the two symbol encodings, not that the two
//! automata mean the same thing track-for-track.

use crate::automaton::Automaton;
use crate::fa::Fa;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub enum EquivError {
    /// An input automaton was not deterministic-and-total.
    NotTotalDfa,
    /// The two `Fa`s being combined have different [`Fa::alphabet_size`] — the
    /// comparison is meaningless without a shared symbol encoding. Raised by the
    /// `Fa`-level functions ([`product_dfa`]/[`language_equivalent`]); see
    /// [`MismatchedTrackStructure`](EquivError::MismatchedTrackStructure) for the
    /// `Automaton`-level analogue.
    MismatchedAlphabet,
    /// The two `Automaton`s passed to [`automaton_language_equivalent`] have
    /// different track structure — a different number of tracks, or a track whose
    /// alphabet (digit list, in order) doesn't match positionally. Raised BEFORE any
    /// `Fa`-level comparison runs, since a mismatch here means the two automata's
    /// raw transition symbols cannot be assumed to mean the same digit tuples.
    MismatchedTrackStructure,
    /// A TRUE/FALSE automaton was handed to [`product_dfa`], whose construction is
    /// meaningless for it (it has no alphabet and no states — see this module's docs
    /// on why this must be an error rather than a vacuous success).
    TrivialAutomaton,
}

/// Complements a total DFA: flips every state's accept bit. `fa` must already be a
/// total DFA (see module docs on the 0/1-acceptance scope limit).
///
/// A trivial automaton is complemented by flipping its truth value, matching
/// `AutomatonLogicalOps.not`'s own short-circuit (`:146-149`).
pub fn complement(fa: &Fa) -> Result<Fa, EquivError> {
    if fa.is_true_false_automaton() {
        return Ok(Fa::trivial(!fa.is_true_automaton()));
    }
    if !fa.is_deterministic_and_total() {
        return Err(EquivError::NotTotalDfa);
    }
    Ok(Fa {
        true_false: None,
        q0: fa.q0,
        q: fa.q,
        alphabet_size: fa.alphabet_size,
        o: fa.o.iter().map(|&o| if o == 0 { 1 } else { 0 }).collect(),
        d: fa.d.clone(),
    })
}

/// Cross-product of two total DFAs over the same alphabet size, with acceptance of a
/// combined state `(i, j)` decided by `accept(a.is_accepting(i), b.is_accepting(j))`.
/// Both inputs must already be total DFAs, and neither may be a trivial (TRUE/FALSE)
/// automaton.
///
/// The trivial check must come FIRST: a trivial `Fa` passes
/// [`Fa::is_deterministic_and_total`] vacuously (see this module's docs), so without it
/// this would silently return a 0-state product instead of erroring.
pub fn product_dfa(a: &Fa, b: &Fa, accept: impl Fn(bool, bool) -> bool) -> Result<Fa, EquivError> {
    if a.is_true_false_automaton() || b.is_true_false_automaton() {
        return Err(EquivError::TrivialAutomaton);
    }
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
        true_false: None,
        q0: pair_id(a.q0, b.q0),
        q,
        alphabet_size,
        o,
        d,
    })
}

/// True iff `a` and `b` recognize the same language. Both must be total DFAs (or both
/// trivial). Computed as emptiness of the symmetric difference (`product_dfa` with
/// `accept = XOR`) — the same construction named in `docs/DESIGN.md` §5 Tier 1 as the
/// required `wr-core` deliverable.
///
/// The two leading branches are `EqualityUtils.faEqual`'s, ported verbatim (U0) — see
/// this module's docs, including why a trivial automaton is deliberately NOT compared
/// against a language-equal ordinary one.
pub fn language_equivalent(a: &Fa, b: &Fa) -> Result<bool, EquivError> {
    if a.is_true_false_automaton() != b.is_true_false_automaton() {
        return Ok(false);
    }
    if a.is_true_false_automaton() {
        return Ok(a.is_true_automaton() == b.is_true_automaton());
    }
    let sym_diff = product_dfa(a, b, |x, y| x != y)?;
    Ok(sym_diff.is_language_empty())
}

/// `Automaton`-level equivalence: like [`language_equivalent`], but takes
/// [`Automaton`]s rather than bare [`Fa`]s and checks track structure FIRST — see this
/// module's doc comment ("Former known gap... PARTIALLY closed") for the full picture,
/// including a real gap this function does NOT close.
///
/// Requires `a.alphabet == b.alphabet` exactly: same number of tracks, and for each
/// track the identical digit list in the identical order. This is deliberately
/// stricter than "same digits as a set" — a transition symbol's meaning depends on
/// each digit's *position* in its track's alphabet list (`automaton::Automaton`'s
/// module docs, "Symbol encoding"), so two tracks that are set-equal but differently
/// ORDERED (e.g. `[0, 1]` vs `[1, 0]`) encode the same digit to different integers
/// and are correctly rejected here, not silently treated as comparable. On a
/// mismatch, returns [`EquivError::MismatchedTrackStructure`] without ever touching
/// `a.fa`/`b.fa`.
///
/// **Does NOT compare [`Automaton::label`] — and this is a real, demonstrated gap, not
/// a benign simplification.** Two `Automaton`s whose tracks are PERMUTED relative to
/// each other (e.g. `a.label = ["i","x"]`, `b.label = ["x","i"]`) but whose
/// per-position `alphabet` lists happen to be identical (the common case for
/// same-base multi-track automata, where every track's alphabet is the same `0..k`
/// list) pass this check and get compared POSITIONALLY — silently answering a
/// different, meaningless question ("is `a`'s track 0 vs `b`'s track 0 the same
/// language slice") instead of the one the caller almost certainly wants ("do these
/// automata denote the same relation once matching tracks are lined up by name"). This
/// function makes no attempt to detect or correct for that; see this module's top doc
/// comment for why (in short: `wr_io`'s reader can't recover real labels, so a strict
/// label check would break the differential-testing use case this function was built
/// for) and for what a caller needing real semantic alignment must do instead
/// (`canonize`/`sort_label` both operands to a shared order first). Pinned explicitly
/// by `automaton_language_equivalent_does_not_detect_permuted_labels` below.
///
/// Once track structure is confirmed to match POSITIONALLY, this delegates to
/// [`language_equivalent`], which still enforces both `Fa`s be total DFAs (see this
/// module's top doc comment) and still checks `Fa::alphabet_size` itself as a second,
/// cheap line of defense (always redundant with the check here when `Automaton::new`'s
/// own documented invariant — `Π alphabet[i].len() == fa.alphabet_size` — holds, but
/// this function does not re-validate that invariant, matching `Automaton::new`'s own
/// "callers are responsible" convention). Note also that this invariant is guarded by
/// `Automaton::new`'s CALLER, not enforced continuously: a caller who mutates
/// `a.alphabet` directly without calling `Automaton::setup_encoder` leaves `a`'s
/// private `encoder` stale, at which point equal `alphabet` no longer implies equal
/// symbol encoding either — this function trusts `Automaton`'s own invariant, it does
/// not re-derive it from `encoder`.
pub fn automaton_language_equivalent(a: &Automaton, b: &Automaton) -> Result<bool, EquivError> {
    // The trivial cases are decided BEFORE the track-structure check (U0), matching
    // `EqualityUtils.faEqual`'s own ordering. Doing it the other way round would be
    // wrong in both directions: a trivial automaton has an EMPTY `alphabet`, so
    // `TRUE` vs `FALSE` would pass the `[] == []` structure check and fall through to
    // the `Fa` layer, while `TRUE` vs any ordinary automaton would report
    // `MismatchedTrackStructure` instead of the plain `false` Walnut's oracle gives.
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        return language_equivalent(&a.fa, &b.fa);
    }
    if a.alphabet != b.alphabet {
        return Err(EquivError::MismatchedTrackStructure);
    }
    language_equivalent(&a.fa, &b.fa)
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
            true_false: None,
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
            true_false: None,
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
            true_false: None,
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
            true_false: None,
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

    // --- automaton_language_equivalent (U8: closes the Automaton-level alphabet gap
    // named in this module's doc comment) ---

    fn single_track_automaton(fa: Fa, alphabet: Vec<i32>) -> Automaton {
        Automaton::new(fa, vec![alphabet], vec!["x".to_string()], vec![Some(true)])
    }

    #[test]
    fn automaton_language_equivalent_matches_on_identical_track_structure() {
        let a = single_track_automaton(contains_one_dfa(), vec![0, 1]);
        // Different STATE numbering (mirrors `language_equivalent_ignores_state_numbering`
        // above) but the same track structure -- the structural check must not itself
        // reject this, only a genuine alphabet mismatch should.
        let b = single_track_automaton(contains_one_dfa_relabeled(), vec![0, 1]);
        assert_eq!(automaton_language_equivalent(&a, &b), Ok(true));
    }

    #[test]
    fn automaton_language_equivalent_detects_a_real_language_difference() {
        let a = single_track_automaton(contains_one_dfa(), vec![0, 1]);
        let b = single_track_automaton(reject_all_dfa(), vec![0, 1]);
        assert_eq!(automaton_language_equivalent(&a, &b), Ok(false));
    }

    #[test]
    fn automaton_language_equivalent_rejects_mismatched_arity() {
        let a = single_track_automaton(contains_one_dfa(), vec![0, 1]);
        let b_fa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 4,
            o: vec![0],
            d: vec![Map::new()],
        };
        let b = Automaton::new(
            b_fa,
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(
            automaton_language_equivalent(&a, &b),
            Err(EquivError::MismatchedTrackStructure)
        );
    }

    #[test]
    fn automaton_language_equivalent_rejects_a_same_set_but_reordered_track_alphabet() {
        // Both tracks have the SET {0, 1}, so a set-equality check would wrongly wave
        // this through -- but symbol 0 means digit 0 on the `a` side and digit 1 on
        // the `b` side (see `automaton::Automaton`'s "Symbol encoding" docs: encoding
        // is by position-in-list, not literal digit value), so comparing their `Fa`s
        // directly would be meaningless. This is exactly the gap this function closes.
        let a = single_track_automaton(contains_one_dfa(), vec![0, 1]);
        let b = single_track_automaton(contains_one_dfa(), vec![1, 0]);
        assert_eq!(
            automaton_language_equivalent(&a, &b),
            Err(EquivError::MismatchedTrackStructure)
        );
    }

    #[test]
    fn automaton_language_equivalent_does_not_detect_permuted_labels() {
        // Adversarial-review finding: `automaton_language_equivalent` only compares
        // `alphabet` positionally, never `label` -- so two automata whose tracks are
        // PERMUTED relative to each other, but whose per-position alphabets happen to
        // coincide (the common same-base multi-track shape, not a rare corner case),
        // are silently compared as if the tracks lined up. This test PINS that gap
        // (a documented limitation, not a regression) so it can never be silently
        // "fixed" into an even-more-wrong assumption, and so a future reader relying
        // on label-based alignment here finds this test first.
        //
        // `a` accepts (as a length-1 word) iff its "i" track (position 0) is 1; `b`
        // reuses the IDENTICAL `Fa`/`alphabet` but swaps which label names which
        // position, so `b` actually means "x"==1, not "i"==1 -- a different relation
        // over the shared variables {i, x}. `automaton_language_equivalent` cannot
        // see this: it returns `Ok(true)`.
        let mut d0 = Map::new();
        // encoder [1,2]: symbol = d0 + 2*d1. Accept (move to state 1) iff d0 == 1:
        // symbols 1 (d0=1,d1=0) and 3 (d0=1,d1=1).
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        d0.insert(2, vec![0]);
        d0.insert(3, vec![1]);
        let mut d1 = Map::new();
        for sym in 0..4 {
            d1.insert(sym, vec![0]);
        }
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 4,
            o: vec![0, 1],
            d: vec![d0, d1],
        };
        let alphabet = vec![vec![0, 1], vec![0, 1]];
        let a = Automaton::new(
            fa.clone(),
            alphabet.clone(),
            vec!["i".to_string(), "x".to_string()],
            vec![Some(true), Some(true)],
        );
        let b = Automaton::new(
            fa,
            alphabet,
            vec!["x".to_string(), "i".to_string()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(a.label, vec!["i".to_string(), "x".to_string()]);
        assert_eq!(b.label, vec!["x".to_string(), "i".to_string()]);

        // The gap: reported equivalent purely because the underlying Fa/alphabet are
        // byte-identical, even though `a` denotes "i==1" and `b` denotes "x==1".
        assert_eq!(automaton_language_equivalent(&a, &b), Ok(true));
    }

    // --- trivial (TRUE/FALSE) automata: EqualityUtils.faEqual, ported (U0) ---

    /// A total DFA over `{0,1}` accepting EVERYTHING — the ordinary automaton whose
    /// language coincides with the TRUE automaton's. Used to pin the deliberate
    /// "`faEqual` compares the flag, not just the language" behavior below.
    fn accept_all_dfa() -> Fa {
        let mut d0 = Map::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1],
            d: vec![d0],
        }
    }

    #[test]
    fn trivial_automata_compare_by_truth_value() {
        let t = Fa::trivial(true);
        let f = Fa::trivial(false);
        assert_eq!(language_equivalent(&t, &t), Ok(true));
        assert_eq!(language_equivalent(&f, &f), Ok(true));
        assert_eq!(language_equivalent(&t, &f), Ok(false));
        assert_eq!(language_equivalent(&f, &t), Ok(false));
    }

    #[test]
    fn trivial_vs_ordinary_is_unequal_in_both_argument_orders() {
        // `faEqual`'s first line: `if (a.isTRUE_FALSE_AUTOMATON() !=
        // b.isTRUE_FALSE_AUTOMATON()) return false;`. Deliberately `false` even when the
        // LANGUAGES agree (`accept_all_dfa` accepts everything, like the TRUE
        // automaton) -- see this module's docs on why the port keeps that.
        let t = Fa::trivial(true);
        let all = accept_all_dfa();
        assert_eq!(language_equivalent(&t, &all), Ok(false));
        assert_eq!(language_equivalent(&all, &t), Ok(false));

        let f = Fa::trivial(false);
        let none = reject_all_dfa();
        assert_eq!(language_equivalent(&f, &none), Ok(false));
        assert_eq!(language_equivalent(&none, &f), Ok(false));
    }

    #[test]
    fn without_the_faequal_short_circuit_the_oracle_would_answer_wrongly() {
        // Regression guard for the specific wrong-answer path U0 closes: a trivial `Fa`
        // has `q == 0` and an empty `d`, so `is_deterministic_and_total()` is VACUOUSLY
        // true and the raw product construction would have produced a 0-state (hence
        // "empty symmetric difference") result -- reporting TRUE == FALSE. Pinning both
        // halves here means neither the vacuous-totality quirk nor the short-circuit
        // can be quietly removed without this failing.
        let t = Fa::trivial(true);
        assert!(
            t.is_deterministic_and_total(),
            "vacuously total -- this is exactly why the short-circuit is required"
        );
        assert_eq!(
            product_dfa(&t, &Fa::trivial(false), |x, y| x != y).unwrap_err(),
            EquivError::TrivialAutomaton
        );
        assert_eq!(
            product_dfa(&accept_all_dfa(), &t, |x, y| x != y).unwrap_err(),
            EquivError::TrivialAutomaton
        );
    }

    #[test]
    fn complement_of_a_trivial_automaton_flips_its_truth_value() {
        let c = complement(&Fa::trivial(true)).unwrap();
        assert!(c.is_true_false_automaton() && !c.is_true_automaton());
        let c = complement(&Fa::trivial(false)).unwrap();
        assert!(c.is_true_false_automaton() && c.is_true_automaton());
    }

    #[test]
    fn automaton_level_oracle_decides_trivial_cases_before_checking_track_structure() {
        let t = Automaton::true_false(true);
        let f = Automaton::true_false(false);
        assert_eq!(automaton_language_equivalent(&t, &t), Ok(true));
        // Both have an EMPTY `alphabet`, so an ordering that ran the structure check
        // first would fall through to the `Fa` layer here rather than answering.
        assert_eq!(automaton_language_equivalent(&t, &f), Ok(false));

        // Trivial vs ordinary must be a plain `false`, NOT `MismatchedTrackStructure`
        // (which is what the structure check would have produced, since `[]` != `[[0,1]]`).
        let ordinary = single_track_automaton(accept_all_dfa(), vec![0, 1]);
        assert_eq!(automaton_language_equivalent(&t, &ordinary), Ok(false));
        assert_eq!(automaton_language_equivalent(&ordinary, &t), Ok(false));
    }

    #[test]
    fn automaton_language_equivalent_still_requires_total_dfas() {
        let mut partial = contains_one_dfa();
        partial.d[0].remove(&1);
        let a = single_track_automaton(partial, vec![0, 1]);
        let b = single_track_automaton(contains_one_dfa(), vec![0, 1]);
        assert_eq!(
            automaton_language_equivalent(&a, &b),
            Err(EquivError::NotTotalDfa)
        );
    }
}
