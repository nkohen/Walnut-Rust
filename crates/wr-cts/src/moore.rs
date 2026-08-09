// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! A standalone, textbook **Moore partition-refinement** DFA minimizer — the
//! *independent second minimizer* named in `docs/DESIGN.md` §5 Tier 4 ("the existing
//! substrate's Moore `minimize` and the ported Valmari `minimize` must produce the same
//! language on every generated automaton — two independent implementations agreeing is
//! strong evidence").
//!
//! # Why this is fresh code rather than a port or a reuse
//!
//! DESIGN.md §4 planned to reuse `RustConstantTermSequences`'s Moore minimizer. Direct
//! investigation found that substrate has no usable minimize entry point for this crate's
//! automata (single-track / `DFAO<ModInt, S>`-specific), so per the user's explicit
//! decision this unit authors a small standalone minimizer instead. Consequently this
//! file is **not** a port of any Java source, and `CLAUDE.md`'s mechanical-port rule 2
//! ("faithful behavior, including quirks") does **not** apply to it: it implements the
//! textbook-correct algorithm, and where Walnut's Valmari minimizer is known to be wrong
//! (`docs/WALNUT-BUGS.md` WB-001, see below) this deliberately does **not** reproduce the
//! defect.
//!
//! # The algorithm (Moore, 1956)
//!
//! 1. Discard states not forward-reachable from `q0` (Moore's refinement is well-defined
//!    on them, but they would survive as extra, unreachable classes and the result would
//!    then not be *minimal*; see [`minimize`]'s doc comment).
//! 2. Start with the two-block partition {non-accepting, accepting}.
//! 3. Repeatedly refine: two states stay together only if they are currently together
//!    **and** their `δ(·, a)`-successors are currently together for every symbol `a`.
//! 4. Stop when a round produces no new blocks — i.e. at the fixpoint, which is the
//!    Myhill–Nerode partition of the reachable states.
//! 5. Quotient: one state per block, transitions read off any representative.
//!
//! This is roughly `O(n² · |Σ|)` (at most `n` rounds, each `O(n · |Σ|)` signature builds
//! plus a map insert per state), against Valmari's asymptotically much better near-linear
//! bound. Speed is explicitly not the point here — *independence* is, so the
//! implementation stays as close to the textbook statement as possible.
//!
//! # Relationship to [`wr_core::minimize`] (Valmari) — they are NOT interchangeable
//!
//! Both compute the same Myhill–Nerode partition, but they return different automata,
//! for two verified reasons (both read off `wr-core`'s source, not assumed):
//!
//! - **Dead states.** `wr_core::minimize::minimize` runs Valmari's co-reachability
//!   pre-pass and *drops* every state that cannot reach acceptance, together with the
//!   transitions into it — so it returns the minimal **partial** DFA. This function
//!   returns the minimal **complete** DFA, which keeps the single dead class whenever
//!   some reachable word has an empty right quotient. The two state counts therefore
//!   differ by exactly that class — except when the language is empty, where Valmari
//!   (via `trim`'s canonical 1-state fallback) still returns one state. The exact
//!   relation, with its full case analysis, is stated and asserted on
//!   `valmari_and_moore_agree_on_the_minimal_dfa` below; note it is deliberately NOT
//!   "totalize Valmari's output and compare", which is off by one on the empty-language
//!   case.
//! - **`q0`-reachability.** `wr_core::minimize::minimize` documents "every state should
//!   be reachable from `q0`" as a *precondition* (callers are told to run
//!   `wr_core::trim::trim` first). This function has no such precondition: it prunes
//!   forward-unreachable states itself, as step 1 above.
//!
//! # `docs/WALNUT-BUGS.md` WB-001 is deliberately NOT reproduced here
//!
//! WB-001 (verified by re-reading `wr-core/src/minimize.rs`'s module docs and the
//! `WALNUT-BUGS.md` entry): in Walnut's Valmari, states found non-co-reachable are parked
//! outside every block's tracked range but keep the block id `0` they were initialized
//! with, and `replaceFields` computes the new start state as `blocks.S[q0]`
//! unconditionally. So if `q0` cannot reach an accepting state *while some accepting
//! state exists elsewhere*, `q0` silently aliases onto block `0` — which may be the
//! accepting block. Its minimal trigger (2 states, alphabet size 1: a non-accepting
//! self-looping `q0` and a disjoint accepting self-loop) turns the language `∅` into
//! `Σ*`. `wr_core::minimize::minimize` ports that verbatim, on purpose.
//!
//! Nothing in Moore's algorithm has an analogue of that parked-element/aliasing step, and
//! this implementation additionally prunes unreachable states up front, so the trigger
//! shape cannot arise. `moore_gives_the_correct_answer_on_wb_001s_trigger` pins the
//! divergence explicitly (asserting *both* sides: the wrong Valmari answer and the right
//! Moore one), so it can never regress into an accidental, undocumented agreement.
//!
//! # Scope note
//!
//! Outputs are rebuilt as plain accept/reject bits (`0`/`1`), matching
//! `wr_core::minimize`'s identical narrowing and this project's current
//! predicate-automaton scope (see `wr_core::equiv`'s module docs). DFAO word-output
//! values (`o > 1`) do not survive; states differing only in a `> 1` output value are
//! treated as equally accepting and may be merged.
//!
//! # Where the "independence" in this cross-check actually comes from
//!
//! Adversarial review noted (correctly) that this file's refinement loop uses the same
//! basic technique — a per-state signature keyed on `(current class, successor classes)`,
//! with dense class ids assigned by first-insertion order — as a private test-only helper
//! already living in `wr_core::minimize`'s own test module
//! (`moore_co_reachable_class_count`). That is a real observation: if that specific
//! signature/fixpoint idiom had a shared blind spot, Moore-vs-Valmari agreement alone
//! would not be strong evidence against it. The test suite below does not rely on that
//! agreement alone for exactly this reason — `nerode_class_count` (test module) computes
//! state equivalence by brute-force word enumeration, sharing no code or design with
//! EITHER minimizer, and `valmari_and_moore_agree_on_the_minimal_dfa`/
//! `moore_reaches_the_brute_force_nerode_class_count` chain through it. That brute-force
//! oracle, not the mere existence of two minimizers, is what makes this cross-check
//! trustworthy against a shared-technique blind spot.

use std::collections::BTreeMap;
use wr_core::fa::Fa;

/// Why [`minimize`] refused an input (or, for [`MooreError::Internal`], failed its own
/// post-hoc invariant check).
///
/// Every variant is a real, returned error — never a `debug_assert!`, per `PORTING.md`'s
/// named "debug_assert erasing side effects" regression class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MooreError {
    /// `fa.q == 0`. A zero-state automaton has no start state to quotient around;
    /// `wr_core::minimize` passes it through unchanged, but this function has no
    /// meaningful minimal *complete* DFA to return, so it is rejected loudly.
    NoStates,
    /// The input is structurally inconsistent: `o`/`d` length disagrees with `q`, `q0`
    /// is out of range, or a transition names a destination `>= q`. Carries a short
    /// static description.
    Malformed(&'static str),
    /// The input is not deterministic-and-total ([`Fa::is_deterministic_and_total`]) —
    /// the same precondition `wr_core::equiv`'s oracle requires. NFAs are rejected, not
    /// subset-constructed.
    NotTotalDfa,
    /// `alphabet_size` does not fit in `i32` (the symbol type), or
    /// `q * alphabet_size` overflows `usize`.
    TooLarge,
    /// The refinement fixpoint failed its own consistency check before quotienting.
    /// Unreachable if this module is correct; returned rather than assumed so that a
    /// latent defect surfaces as a loud error instead of a silently wrong automaton.
    Internal(&'static str),
}

/// Minimizes a **deterministic, complete** automaton with Moore's partition-refinement
/// algorithm, returning the unique minimal **complete** DFA for the same language
/// (up to state renumbering).
///
/// The result is itself deterministic and total, has `q >= 1`, preserves
/// `fa.alphabet_size`, and has outputs narrowed to `0`/`1` (see the module docs).
/// State numbering is *not* canonical — compare by language, never by structure
/// (`CLAUDE.md` prime directive #1).
///
/// # Why forward-unreachable states are pruned first
///
/// Refining over *all* states would still yield a language-preserving quotient, but the
/// blocks containing only unreachable states would survive as unreachable states of the
/// result — so the output would not be minimal, and the state count (the load-bearing
/// half of the Tier-4 cross-check) would be wrong. Pruning is therefore part of the
/// algorithm here, not an optional pre-pass. Note this is *forward* reachability only,
/// deliberately: unlike `wr_core::trim::trim`, dead (non-co-reachable) states are
/// **kept**, because the minimal *complete* DFA needs its dead class.
///
/// # Errors
///
/// See [`MooreError`]. In particular an NFA or a partial DFA is rejected with
/// [`MooreError::NotTotalDfa`] rather than being determinized or totalized implicitly.
pub fn minimize(fa: &Fa) -> Result<Fa, MooreError> {
    // ---- preconditions (checked before any indexing) ----
    if fa.q == 0 {
        return Err(MooreError::NoStates);
    }
    if fa.o.len() != fa.q || fa.d.len() != fa.q {
        return Err(MooreError::Malformed("o/d length disagrees with q"));
    }
    if fa.q0 >= fa.q {
        return Err(MooreError::Malformed("q0 >= q"));
    }
    let alphabet_i32 = i32::try_from(fa.alphabet_size).map_err(|_| MooreError::TooLarge)?;
    if !fa.is_deterministic_and_total() {
        return Err(MooreError::NotTotalDfa);
    }

    let width = fa.alphabet_size;
    let symbols: Vec<i32> = (0..alphabet_i32).collect();

    // ---- flatten δ into a dense table: delta[s * width + a] ----
    // Checked, not `as`/wrapping: `q * alphabet_size` is the only product in this file
    // that could overflow in principle (`CLAUDE.md`'s checked-arithmetic discipline).
    let table_len = fa.q.checked_mul(width).ok_or(MooreError::TooLarge)?;
    let mut delta = vec![0usize; table_len];
    for (s, row) in fa.d.iter().enumerate() {
        for (a, sym) in symbols.iter().enumerate() {
            // Safe to index: `is_deterministic_and_total` was just confirmed above, so
            // every symbol in `0..alphabet_size` has exactly one destination.
            let dest = row[sym][0];
            if dest >= fa.q {
                return Err(MooreError::Malformed("transition destination >= q"));
            }
            delta[s * width + a] = dest;
        }
    }

    // ---- step 1: keep only the states forward-reachable from q0 ----
    let mut reachable = vec![false; fa.q];
    reachable[fa.q0] = true;
    let mut stack = vec![fa.q0];
    while let Some(s) = stack.pop() {
        for a in 0..width {
            let t = delta[s * width + a];
            if !reachable[t] {
                reachable[t] = true;
                stack.push(t);
            }
        }
    }
    let states: Vec<usize> = (0..fa.q).filter(|&s| reachable[s]).collect();

    // ---- step 2: the initial two-block partition (accepting vs non-accepting) ----
    // Block ids are assigned densely in order of first appearance, so a partition with
    // only one non-empty block gets `1` block, not `2` — an empty block would inflate
    // the class count and break the "unchanged count means fixpoint" test below.
    let mut class = vec![usize::MAX; fa.q];
    let mut num_classes;
    {
        let mut ids: BTreeMap<bool, usize> = BTreeMap::new();
        for &s in &states {
            let next = ids.len();
            class[s] = *ids.entry(fa.is_accepting(s)).or_insert(next);
        }
        num_classes = ids.len();
    }

    // ---- steps 3+4: refine to the fixpoint ----
    //
    // Each round re-blocks by the signature `(current block, blocks of all successors)`.
    // Because the signature *includes* the current block, every round is a refinement of
    // the previous partition: blocks only ever split, never merge or move. Hence
    //   (a) the block count is non-decreasing and bounded by `states.len()`, so the loop
    //       runs at most `states.len()` times (no unbounded iteration), and
    //   (b) an unchanged block count means no block split, i.e. the partition is
    //       identical to the previous round and therefore stable — so this really does
    //       iterate to a fixpoint, and a single pass is not enough (pinned by
    //       `refinement_needs_several_rounds_to_separate_a_distant_difference`).
    loop {
        let mut signature_ids: BTreeMap<Vec<usize>, usize> = BTreeMap::new();
        let mut next_class = vec![usize::MAX; fa.q];
        for &s in &states {
            let mut signature = Vec::with_capacity(width + 1);
            signature.push(class[s]);
            for a in 0..width {
                // `delta[s * width + a]` is reachable because `s` is, so its class is set.
                signature.push(class[delta[s * width + a]]);
            }
            let next = signature_ids.len();
            next_class[s] = *signature_ids.entry(signature).or_insert(next);
        }
        let refined_count = signature_ids.len();
        class = next_class;
        if refined_count == num_classes {
            break;
        }
        num_classes = refined_count;
    }

    // ---- step 5: quotient ----
    let new_q = num_classes;
    let mut rep = vec![usize::MAX; new_q];
    for &s in &states {
        let c = class[s];
        if rep[c] == usize::MAX {
            rep[c] = s;
        }
    }
    if rep.contains(&usize::MAX) {
        return Err(MooreError::Internal("a block ended up with no member"));
    }

    // Post-hoc check that the partition really is stable, so the "read the transitions
    // off an arbitrary representative" step below is well-defined. This should be
    // implied by the loop above; verifying it costs O(n * |Σ|) and converts a latent
    // defect in this file into a loud error rather than a silently wrong automaton.
    for &s in &states {
        let r = rep[class[s]];
        if fa.is_accepting(s) != fa.is_accepting(r) {
            return Err(MooreError::Internal(
                "block mixes accepting and non-accepting",
            ));
        }
        for a in 0..width {
            if class[delta[s * width + a]] != class[delta[r * width + a]] {
                return Err(MooreError::Internal(
                    "block members disagree on a successor",
                ));
            }
        }
    }

    let mut o = Vec::with_capacity(new_q);
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(new_q);
    for &r in &rep {
        o.push(i32::from(fa.is_accepting(r)));
        let mut row = BTreeMap::new();
        for (a, &sym) in symbols.iter().enumerate() {
            row.insert(sym, vec![class[delta[r * width + a]]]);
        }
        d.push(row);
    }

    Ok(Fa {
        q0: class[fa.q0],
        q: new_q,
        alphabet_size: fa.alphabet_size,
        o,
        d,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeSet;
    use wr_core::equiv::language_equivalent;

    fn row(pairs: &[(i32, usize)]) -> BTreeMap<i32, Vec<usize>> {
        pairs.iter().map(|&(s, d)| (s, vec![d])).collect()
    }

    // ---------------------------------------------------------------- brute-force
    // oracle
    //
    // A *third* implementation, deliberately sharing nothing with either Moore
    // (partition refinement) or Valmari (refinable-partition with cords): it computes
    // each reachable state's acceptance signature over EVERY word up to length `q` and
    // counts distinct signatures. Myhill–Nerode guarantees two states of an `n`-state
    // DFA that agree on all words of length `< n` are equivalent, so length `<= q` is
    // strictly more than sufficient. Exponential in `q` — which is exactly why the
    // generators below stay tiny (`CLAUDE.md`'s "generate SMALL" discipline).
    //
    // This is what makes the property tests non-degenerate: an implementation that
    // stopped refining after one pass, or that was off by one round, would report a
    // smaller class count than this oracle on any automaton whose distinguishing word
    // is longer than one symbol.

    fn all_words(alphabet_size: usize, max_len: usize) -> Vec<Vec<i32>> {
        let mut out: Vec<Vec<i32>> = vec![Vec::new()];
        let mut frontier: Vec<Vec<i32>> = vec![Vec::new()];
        for _ in 0..max_len {
            let mut next: Vec<Vec<i32>> = Vec::new();
            for w in &frontier {
                for a in 0..alphabet_size as i32 {
                    let mut extended = w.clone();
                    extended.push(a);
                    next.push(extended);
                }
            }
            out.extend(next.iter().cloned());
            frontier = next;
        }
        out
    }

    /// Number of Myhill–Nerode classes among the states reachable from `q0`, computed by
    /// brute force. Requires a deterministic, total `fa`.
    fn nerode_class_count(fa: &Fa) -> usize {
        assert!(fa.is_deterministic_and_total());
        let mut reachable = vec![false; fa.q];
        reachable[fa.q0] = true;
        let mut stack = vec![fa.q0];
        while let Some(s) = stack.pop() {
            for sym in 0..fa.alphabet_size as i32 {
                let t = fa.d[s][&sym][0];
                if !reachable[t] {
                    reachable[t] = true;
                    stack.push(t);
                }
            }
        }

        let words = all_words(fa.alphabet_size, fa.q);
        let mut signatures: BTreeSet<Vec<bool>> = BTreeSet::new();
        for (s, &is_reachable) in reachable.iter().enumerate() {
            if !is_reachable {
                continue;
            }
            let sig: Vec<bool> = words
                .iter()
                .map(|w| {
                    let mut cur = s;
                    for &sym in w {
                        cur = fa.d[cur][&sym][0];
                    }
                    fa.is_accepting(cur)
                })
                .collect();
            signatures.insert(sig);
        }
        signatures.len()
    }

    #[test]
    fn brute_force_oracle_self_check() {
        // The oracle is only trustworthy if it is itself right on cases derived by hand.
        // "contains at least one 1" is minimal (2 classes: seen-a-1 vs not).
        let contains_one = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        };
        assert_eq!(nerode_class_count(&contains_one), 2);

        // Three states, two of them equivalent accepting sinks: 2 classes.
        let two_sinks = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 1],
            d: vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 2), (1, 2)]),
            ],
        };
        assert_eq!(nerode_class_count(&two_sinks), 2);

        // Everything equivalent: 1 class.
        let all_reject = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 0],
            d: vec![row(&[(0, 1)]), row(&[(0, 0)])],
        };
        assert_eq!(nerode_class_count(&all_reject), 1);
    }

    // ----------------------------------------------------- hand-derived unit tests

    #[test]
    fn merges_two_equivalent_accepting_sinks() {
        // States 1 and 2 both accept every continuation, so they are equivalent but
        // structurally distinct. L = Σ⁺, whose minimal complete DFA has exactly 2
        // states (ε is rejected, every nonempty word accepted; no dead class).
        let fa = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 1],
            d: vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 2), (1, 2)]),
            ],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2);
        assert_eq!(min.q, nerode_class_count(&fa));
        assert!(min.is_deterministic_and_total());
        assert_eq!(language_equivalent(&fa, &min), Ok(true));
        assert!(!min.accepts_word(&[]));
        assert!(min.accepts_word(&[1]));
        assert!(min.accepts_word(&[0, 1, 0]));
    }

    #[test]
    fn already_minimal_dfa_is_a_no_op_in_size() {
        // "contains at least one 1": ε is rejected and `1` accepted, so the two states
        // are distinguishable — a correct minimizer must not merge them.
        let fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2, "no false merge on an already-minimal DFA");
        assert_eq!(language_equivalent(&fa, &min), Ok(true));
    }

    #[test]
    fn single_state_automaton_survives_unchanged() {
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1],
            d: vec![row(&[(0, 0), (1, 0)])],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 1);
        assert_eq!(min.q0, 0);
        assert!(min.is_accepting(min.q0), "L = Σ*");
        assert_eq!(language_equivalent(&fa, &min), Ok(true));
    }

    #[test]
    fn forward_unreachable_states_are_pruned_not_merged() {
        // State 2 is unreachable from q0 AND inequivalent to both reachable states (it
        // is a non-accepting sink, whereas state 0 leads to acceptance and state 1
        // accepts). If unreachable states were refined along with the rest, they would
        // survive as a third, unreachable block and the result would have 3 states.
        let fa = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 0],
            d: vec![
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 2), (1, 2)]),
            ],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2, "the unreachable dead sink must not survive");
        assert_eq!(min.q, nerode_class_count(&fa));
        assert!(!min.accepts_word(&[]));
        assert!(min.accepts_word(&[0]));
        assert!(min.accepts_word(&[1, 1, 0]));
    }

    #[test]
    fn non_zero_start_state_is_handled() {
        // Same automaton, but entered at state 1 — L = Σ* here, so everything reachable
        // collapses to one accepting state. Exercises `class[fa.q0]` with `q0 != 0`.
        let fa = Fa {
            q0: 1,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 0],
            d: vec![
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 2), (1, 2)]),
            ],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 1);
        assert!(min.is_accepting(min.q0));
        assert_eq!(language_equivalent(&fa, &min), Ok(true));
    }

    #[test]
    fn dead_states_are_kept_because_the_minimal_complete_dfa_needs_them() {
        // L = {ε}. The minimal COMPLETE DFA has 2 states: the accepting start and a
        // dead sink. (`wr_core::minimize::minimize` would return only 1, having dropped
        // the dead state — the documented difference, see the module docs.)
        let fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 0],
            d: vec![row(&[(0, 1), (1, 1)]), row(&[(0, 1), (1, 1)])],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2);
        assert!(min.is_deterministic_and_total());
        assert!(min.accepts_word(&[]));
        assert!(!min.accepts_word(&[0]));
    }

    #[test]
    fn refinement_needs_several_rounds_to_separate_a_distant_difference() {
        // THE anti-"one pass is enough" test. Two parallel 3-step chains that are
        // indistinguishable until the very last step:
        //
        //   0 --0--> 1 --*--> 2 --*--> 3 (accepting sink)
        //   0 --1--> 4 --*--> 5 --*--> 6 --*--> 7 (dead sink)
        //
        // L = {0}·Σ^{>=2}. Right languages: L(0)=0·Σ^{>=2}, L(1)=Σ^{>=2}, L(2)=Σ^{>=1},
        // L(3)=Σ*, L(4)=L(5)=L(6)=L(7)=∅ — so exactly 5 Myhill-Nerode classes:
        // {0} {1} {2} {3} {4,5,6,7}.
        //
        // Hand-traced round by round (initial partition = {3} vs the rest):
        //   round 1 -> {0,1,4,5,6,7} {2} {3}                        (3 blocks)
        //   round 2 -> {0,4,5,6,7} {1} {2} {3}                      (4 blocks)
        //   round 3 -> {0} {4,5,6,7} {1} {2} {3}                    (5 blocks)
        //   round 4 -> unchanged, fixpoint
        // A single-pass implementation would answer 3; stopping one round early would
        // answer 4. Only a genuine fixpoint iteration answers 5.
        let fa = Fa {
            q0: 0,
            q: 8,
            alphabet_size: 2,
            o: vec![0, 0, 0, 1, 0, 0, 0, 0],
            d: vec![
                row(&[(0, 1), (1, 4)]),
                row(&[(0, 2), (1, 2)]),
                row(&[(0, 3), (1, 3)]),
                row(&[(0, 3), (1, 3)]),
                row(&[(0, 5), (1, 5)]),
                row(&[(0, 6), (1, 6)]),
                row(&[(0, 7), (1, 7)]),
                row(&[(0, 7), (1, 7)]),
            ],
        };
        assert_eq!(
            nerode_class_count(&fa),
            5,
            "oracle agrees with the hand trace"
        );
        let min = minimize(&fa).unwrap();
        assert_eq!(
            min.q, 5,
            "must iterate to the fixpoint, not stop after a pass"
        );
        assert_eq!(language_equivalent(&fa, &min), Ok(true));

        // Spot-check the language directly, independent of the state count.
        assert!(min.accepts_word(&[0, 0, 0]));
        assert!(min.accepts_word(&[0, 1, 1, 0]));
        assert!(!min.accepts_word(&[0, 0]));
        assert!(!min.accepts_word(&[1, 0, 0]));
        assert!(!min.accepts_word(&[]));
    }

    #[test]
    fn empty_alphabet_collapses_to_the_start_states_class() {
        // Degenerate but constructible: with no symbols, only `q0` is reachable and the
        // language is {ε} or ∅ depending on `o[q0]`.
        let fa = Fa {
            q0: 1,
            q: 2,
            alphabet_size: 0,
            o: vec![1, 0],
            d: vec![BTreeMap::new(), BTreeMap::new()],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 1);
        assert!(!min.is_accepting(min.q0), "q0 was the non-accepting state");
        assert!(min.d[0].is_empty());
    }

    // ------------------------------------------------- WB-001: deliberate divergence

    #[test]
    fn moore_gives_the_correct_answer_on_wb_001s_trigger() {
        // `docs/WALNUT-BUGS.md` WB-001's minimal verified trigger, verbatim: q0 is
        // non-accepting and self-loops; state 1 is a disjoint accepting self-loop that
        // q0 can never reach. The true language is ∅.
        //
        // This test asserts BOTH sides on purpose, so the divergence is pinned rather
        // than merely described:
        //   * `wr_core::minimize::minimize` (ported Valmari) answers Σ* — the ported bug.
        //   * this Moore minimizer answers ∅ — the mathematically correct result.
        // The cross-check property test below therefore MUST scope its inputs to where
        // WB-001 cannot fire; see its doc comment.
        let fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![row(&[(0, 0)]), row(&[(0, 1)])],
        };
        assert!(fa.is_language_empty(), "sanity: the true language is empty");

        let valmari = wr_core::minimize::minimize(&fa).unwrap();
        assert_eq!(valmari.q, 1);
        assert!(
            valmari.is_accepting(valmari.q0),
            "ported WB-001: Valmari turns ∅ into Σ* here"
        );

        let moore = minimize(&fa).unwrap();
        assert_eq!(moore.q, 1);
        assert!(
            !moore.is_accepting(moore.q0),
            "Moore is not bound by the mechanical-port rule and gets this right"
        );
        assert!(moore.is_language_empty());
        assert_eq!(language_equivalent(&fa, &moore), Ok(true));
    }

    #[test]
    fn valmari_and_moore_agree_on_the_empty_language() {
        // Deterministic pin for the one case where the two minimizers' state counts do
        // NOT line up under the naive "totalize Valmari's output" adjustment — found by
        // the cross-check property test below rejecting a first draft of exactly that
        // adjustment, and pinned here so it cannot regress into a proptest-seed-only
        // memory.
        //
        // A single non-accepting self-looping state: L = ∅.
        //   * Moore returns the minimal COMPLETE DFA: 1 (dead) state, WITH transitions.
        //   * `trim` returns its canonical 1-state dead automaton, and Valmari's
        //     co-reachability pre-pass then drops every transition (nothing is
        //     co-reachable), so its output is 1 state with an EMPTY transition table.
        //     `totalize` therefore re-adds a sink and yields 2 — hence the cross-check
        //     property compares raw state counts with a `max(1, ..)`, not totalized ones.
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0],
            d: vec![row(&[(0, 0), (1, 0)])],
        };
        let moore = minimize(&fa).unwrap();
        assert_eq!(moore.q, 1);
        assert!(moore.is_language_empty());
        assert!(moore.is_deterministic_and_total());

        let valmari = wr_core::minimize::minimize(&wr_core::trim::trim(&fa)).unwrap();
        assert_eq!(valmari.q, 1);
        assert!(
            valmari.d[0].is_empty(),
            "Valmari drops every transition when nothing is co-reachable"
        );
        assert_eq!(totalized(&valmari).q, 2, "...so totalizing it adds a sink");

        // The relation the cross-check property actually asserts:
        assert_eq!(dead_state_count(&moore), 1);
        assert_eq!(valmari.q, (moore.q - dead_state_count(&moore)).max(1));
        assert_eq!(language_equivalent(&totalized(&valmari), &moore), Ok(true));
    }

    // ------------------------------------------------------------ precondition tests

    #[test]
    fn rejects_nondeterministic_input() {
        let mut fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        };
        fa.d[0].insert(1, vec![0, 1]);
        assert_eq!(minimize(&fa).unwrap_err(), MooreError::NotTotalDfa);
    }

    #[test]
    fn rejects_partial_input() {
        let mut fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        };
        fa.d[0].remove(&1);
        assert!(fa.is_deterministic(), "deterministic but no longer total");
        assert_eq!(minimize(&fa).unwrap_err(), MooreError::NotTotalDfa);
    }

    #[test]
    fn rejects_zero_state_automaton() {
        let fa = Fa {
            q0: 0,
            q: 0,
            alphabet_size: 2,
            o: vec![],
            d: vec![],
        };
        assert_eq!(minimize(&fa).unwrap_err(), MooreError::NoStates);
    }

    #[test]
    fn rejects_structurally_inconsistent_input() {
        let bad_lengths = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![row(&[(0, 0)]), row(&[(0, 1)])],
        };
        assert!(matches!(
            minimize(&bad_lengths).unwrap_err(),
            MooreError::Malformed(_)
        ));

        let bad_q0 = Fa {
            q0: 5,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![row(&[(0, 0)]), row(&[(0, 1)])],
        };
        assert!(matches!(
            minimize(&bad_q0).unwrap_err(),
            MooreError::Malformed(_)
        ));

        let bad_dest = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![row(&[(0, 9)]), row(&[(0, 1)])],
        };
        assert!(matches!(
            minimize(&bad_dest).unwrap_err(),
            MooreError::Malformed(_)
        ));
    }

    // ------------------------------------------------------------------- generators

    /// Random small TOTAL DFA over a FIXED alphabet size, with a RANDOM `q0`.
    ///
    /// The alphabet size is fixed rather than randomized for the reason `wr-core`'s
    /// `fa.rs`/`determinize.rs` generators document: a randomized alphabet desynchronizes
    /// from any correlated word strategy. `q0` IS randomized — that is what exercises the
    /// forward-reachability pruning step (with `q0` always `0` and destinations drawn
    /// from the whole state space, unreachable states are rarer and the pruning branch is
    /// under-tested).
    fn arb_total_dfa(q_max: usize, alphabet_size: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max).prop_flat_map(move |q| {
            let o_strategy = prop::collection::vec(0i32..=1, q);
            let trans_strategy =
                prop::collection::vec(prop::collection::vec(0usize..q, alphabet_size), q);
            (o_strategy, trans_strategy, 0..q).prop_map(move |(o, trans, q0)| {
                let d = trans
                    .into_iter()
                    .map(|r| {
                        r.into_iter()
                            .enumerate()
                            .map(|(sym, dest)| (sym as i32, vec![dest]))
                            .collect::<BTreeMap<i32, Vec<usize>>>()
                    })
                    .collect();
                Fa {
                    q0,
                    q,
                    alphabet_size,
                    o,
                    d,
                }
            })
        })
    }

    /// [`arb_total_dfa`] plus a GUARANTEED-mergeable pair: one state is duplicated
    /// (identical output, identical successors) and a random subset of the edges into the
    /// original is redirected to the copy.
    ///
    /// Why this exists: a purely random DFA does contain equivalent states often, but
    /// most of them are equivalent for the *shallow* reason of being dead. This
    /// generator plants pairs that are equivalent for a structural reason instead, and —
    /// because the copy sits at the highest state index while the original may be
    /// anywhere — makes the merge non-adjacent in the state numbering. A minimizer that
    /// only ever merged the dead class would still pass a dead-state-only generator; it
    /// fails here.
    ///
    /// Adversarial-review-noted subtlety: `redirect` ranges over `0..copy`, which
    /// includes `dup` itself, so a self-loop on `dup` can get redirected to point at
    /// `copy` instead — since `copy`'s row is a clone taken BEFORE this loop runs, the
    /// result is `dup --sym--> copy` while `copy --sym--> dup` (a mutual 2-cycle), not
    /// two independent self-loops. This is still a genuine Nerode-equivalent pair: `{dup,
    /// copy}` remains closed under every transition either way (on the redirected
    /// symbol, both destinations land back inside `{dup, copy}`), so the planted pair is
    /// provably still mergeable — just via a 2-cycle instead of two self-loops on some
    /// draws, not a generator bug.
    fn arb_total_dfa_with_a_planted_duplicate(
        q_max: usize,
        alphabet_size: usize,
    ) -> impl Strategy<Value = Fa> {
        arb_total_dfa(q_max, alphabet_size).prop_flat_map(move |fa| {
            let q = fa.q;
            let redirect_len = q * alphabet_size;
            (
                Just(fa),
                0..q,
                prop::collection::vec(any::<bool>(), redirect_len),
            )
                .prop_map(move |(fa, dup, redirect)| {
                    let mut out = fa;
                    let copy = out.q;
                    out.o.push(out.o[dup]);
                    out.d.push(out.d[dup].clone());
                    out.q += 1;
                    for s in 0..copy {
                        for a in 0..alphabet_size {
                            let sym = a as i32;
                            let dest = out.d[s][&sym][0];
                            if dest == dup && redirect[s * alphabet_size + a] {
                                out.d[s].insert(sym, vec![copy]);
                            }
                        }
                    }
                    out
                })
        })
    }

    /// Adds a non-accepting sink if needed, purely to satisfy the equivalence oracle's
    /// total-DFA precondition. Language-preserving: the sink is unreachable except via
    /// transitions that previously had no destination (i.e. rejected the word anyway).
    fn totalized(fa: &Fa) -> Fa {
        let mut t = fa.clone();
        t.totalize(0);
        t
    }

    /// How many states of `fa` cannot reach any accepting state (backward closure from
    /// the accepting states, complemented). For a *minimal complete* DFA this is 0 or 1;
    /// the cross-check test asserts that rather than assuming it.
    fn dead_state_count(fa: &Fa) -> usize {
        let mut reverse: Vec<Vec<usize>> = vec![Vec::new(); fa.q];
        for (s, row) in fa.d.iter().enumerate() {
            for dests in row.values() {
                for &t in dests {
                    reverse[t].push(s);
                }
            }
        }
        let mut co_reachable = vec![false; fa.q];
        let mut stack = Vec::new();
        for (s, co) in co_reachable.iter_mut().enumerate() {
            if fa.is_accepting(s) {
                *co = true;
                stack.push(s);
            }
        }
        while let Some(s) = stack.pop() {
            for &p in &reverse[s] {
                if !co_reachable[p] {
                    co_reachable[p] = true;
                    stack.push(p);
                }
            }
        }
        co_reachable.iter().filter(|&&c| !c).count()
    }

    proptest! {
        /// Moore preserves the language. Both sides are total by construction, so the
        /// `wr_core::equiv` oracle applies directly with no totalization fixups.
        #[test]
        fn moore_preserves_language(fa in arb_total_dfa(5, 2)) {
            let min = minimize(&fa).unwrap();
            prop_assert_eq!(language_equivalent(&fa, &min), Ok(true));
        }

        /// Moore's output really is MINIMAL — checked against the brute-force
        /// Myhill–Nerode oracle above, which shares no code or approach with it.
        ///
        /// This is the property that catches UNDER-merging (a stalled or single-pass
        /// refinement loop); `moore_preserves_language` alone would be satisfied by the
        /// identity function.
        #[test]
        fn moore_reaches_the_brute_force_nerode_class_count(fa in arb_total_dfa(5, 2)) {
            let min = minimize(&fa).unwrap();
            prop_assert_eq!(min.q, nerode_class_count(&fa));
        }

        /// Same, over automata that always contain a planted equivalent pair.
        #[test]
        fn moore_is_minimal_on_planted_duplicates(
            fa in arb_total_dfa_with_a_planted_duplicate(5, 2),
        ) {
            let min = minimize(&fa).unwrap();
            prop_assert_eq!(min.q, nerode_class_count(&fa));
            prop_assert_eq!(language_equivalent(&fa, &min), Ok(true));
        }

        /// The output is a well-formed total DFA and never larger than the input.
        #[test]
        fn moore_output_is_a_well_formed_total_dfa(fa in arb_total_dfa(5, 3)) {
            let min = minimize(&fa).unwrap();
            prop_assert!(min.is_deterministic_and_total());
            prop_assert!(min.q >= 1);
            prop_assert!(min.q <= fa.q);
            prop_assert_eq!(min.alphabet_size, fa.alphabet_size);
            prop_assert!(min.q0 < min.q);
            prop_assert!(min.o.iter().all(|&x| x == 0 || x == 1));
        }

        /// Idempotence: a second pass finds nothing further to merge.
        #[test]
        fn moore_is_idempotent(fa in arb_total_dfa(5, 2)) {
            let once = minimize(&fa).unwrap();
            let twice = minimize(&once).unwrap();
            prop_assert_eq!(once.q, twice.q);
            prop_assert_eq!(language_equivalent(&once, &twice), Ok(true));
        }

        /// **The Tier-4 cross-check** (`CLAUDE.md` correctness ladder / `docs/DESIGN.md`
        /// §5: "the ported Valmari `minimize` and an independent Moore `minimize` must
        /// agree — two independent implementations agreeing is strong evidence").
        /// Asserts BOTH the exact state count (so the two really reach the *same* unique
        /// minimal DFA, not merely two language-equivalent automata of different sizes)
        /// AND language equivalence.
        ///
        /// # Two adjustments, and why each is the right one rather than a fudge
        ///
        /// 1. **`trim` on the Valmari side — scoping away `docs/WALNUT-BUGS.md` WB-001.**
        ///    Verified from `wr-core/src/minimize.rs`'s module docs and the WB-001 entry:
        ///    the bug fires only when `q0` cannot reach an accepting state *while some
        ///    accepting state exists*. `wr_core::trim::trim` keeps exactly the states
        ///    that are both forward-reachable from `q0` and co-reachable to acceptance,
        ///    so after it either every state can reach acceptance (trigger impossible) or
        ///    no accepting state exists at all in `trim`'s canonical 1-state dead
        ///    automaton (trigger impossible again). This is the convention already used
        ///    for exactly this situation by `minimize.rs`'s own Tier-4 properties and by
        ///    `logicalops.rs`'s De Morgan tests — re-derived here rather than copied,
        ///    because those call sites trim for a *caller-side* reason (`not`/
        ///    `just_minimize` not establishing the precondition) and this one trims for
        ///    the property's own reason.
        ///
        ///    Scoping (option (a)) is the right framing rather than "assert the two
        ///    disagree on WB-001 inputs" (option (b)): the question this property asks is
        ///    "do two implementations of the same mathematics agree", which is only
        ///    meaningful where the mathematically correct answer is unambiguous. The
        ///    WB-001 divergence is a *deliberate*, separately pinned difference — see
        ///    `moore_gives_the_correct_answer_on_wb_001s_trigger` above, which asserts it
        ///    directly on WB-001's own minimal trigger. `trim` is itself property-tested
        ///    as language-preserving, so this stays an end-to-end check against the
        ///    ORIGINAL automaton's language.
        ///
        /// 2. **The dead class — the partial-vs-complete difference.** Valmari drops
        ///    every state that cannot reach acceptance, returning the minimal PARTIAL
        ///    DFA; Moore returns the minimal COMPLETE DFA. They differ by exactly the
        ///    dead class, which the minimal complete DFA has at most one of (also
        ///    asserted below, since it is a fact this adjustment leans on). So the
        ///    expected relation is `valmari.q == max(1, moore.q - dead_states(moore))`.
        ///    Case analysis, in full:
        ///    * **No dead class.** Every state of `trim(fa)` is co-reachable, every
        ///      transition survives, so Valmari returns the whole quotient: `|M|`.
        ///    * **Dead class and `L != ∅`.** `trim` removes the dead states and Valmari
        ///      quotients what is left: `|M| - 1`.
        ///    * **`L == ∅`.** `trim`'s `keep` set is empty, so it returns its canonical
        ///      1-state dead automaton and Valmari returns 1 state — while `|M| - 1` is
        ///      `0`. That is what the `max(1, ..)` covers (the same convention
        ///      `minimize.rs`'s own `minimize_agrees_with_moore_reference` uses).
        ///
        ///    **This is deliberately NOT written as `totalized(valmari).q == moore.q`.**
        ///    That formulation is tempting and *almost* works, but it is wrong on the
        ///    `L == ∅` case, which a first draft of this test got wrong and proptest
        ///    caught immediately: with no accepting state anywhere, Valmari's
        ///    co-reachability pre-pass drops *every* transition (pinned upstream by
        ///    `minimize.rs`'s `no_accepting_states_collapses_to_one_dead_state`, which
        ///    asserts `min.d[0].is_empty()`), so `totalize` re-adds a sink and yields 2
        ///    states against Moore's 1. The relation above is stated over the raw
        ///    Valmari output instead, so no such hidden `totalize` behavior can smuggle
        ///    in an off-by-one. `totalize` is still used, but only to satisfy the
        ///    equivalence oracle's total-DFA precondition for the LANGUAGE comparison,
        ///    where adding a non-accepting sink is language-preserving by construction.
        ///
        ///    The adjustment cannot mask a real disagreement: `dead_state_count` is read
        ///    off the MOORE side only, so it depends on the language, never on how well
        ///    Valmari merged.
        #[test]
        fn valmari_and_moore_agree_on_the_minimal_dfa(fa in arb_total_dfa(5, 2)) {
            let moore = minimize(&fa).unwrap();
            let valmari = wr_core::minimize::minimize(&wr_core::trim::trim(&fa)).unwrap();

            let dead = dead_state_count(&moore);
            prop_assert!(dead <= 1, "a minimal complete DFA has at most one dead state");
            prop_assert_eq!(valmari.q, (moore.q - dead).max(1));
            prop_assert_eq!(language_equivalent(&totalized(&valmari), &moore), Ok(true));

            // ...and both really are the minimal DFA of the ORIGINAL language, not just
            // consistent with each other.
            prop_assert_eq!(moore.q, nerode_class_count(&fa));
            prop_assert_eq!(language_equivalent(&fa, &moore), Ok(true));
        }

        /// The same cross-check over automata with a planted equivalent pair, so the
        /// agreement is exercised on inputs that definitely require a non-trivial merge
        /// rather than only on inputs that were already (near-)minimal.
        #[test]
        fn valmari_and_moore_agree_on_planted_duplicates(
            fa in arb_total_dfa_with_a_planted_duplicate(5, 2),
        ) {
            let moore = minimize(&fa).unwrap();
            let valmari = wr_core::minimize::minimize(&wr_core::trim::trim(&fa)).unwrap();

            let dead = dead_state_count(&moore);
            prop_assert!(dead <= 1, "a minimal complete DFA has at most one dead state");
            prop_assert_eq!(valmari.q, (moore.q - dead).max(1));
            prop_assert_eq!(language_equivalent(&totalized(&valmari), &moore), Ok(true));
            prop_assert_eq!(moore.q, nerode_class_count(&fa));
        }
    }
}
