// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `numsys` — base-k numeration systems (msd/lsd only).
//!
//! Ports the base-k paths of Walnut's `Automata/NumberSystem`. Lives INSIDE
//! `wr-core` (not a separate crate) on purpose: in Walnut, `Automaton` holds a
//! `List<NumberSystem>` field (19 refs) and `NumberSystem` references `Automaton`
//! 121 times and constructs it — genuine bidirectional coupling that a crate
//! boundary cannot express (adversarial-review kit-finding #1). The adder,
//! comparator, and constant automata over base-k that the FOL decider composes
//! all live here, alongside the automaton types they produce.
//!
//! DROPPED: Ostrowski / Fibonacci / Pell / negative bases (DESIGN.md §3).
//! Property targets (Tier 4): the adder automaton computes real addition; the
//! comparator is a total order; msd and lsd agree after reversal.
//!
//! # Phase 1 spike scope
//!
//! Only [`less_than_msd`] so far (`NumberSystem.lexicographicLessThan`, msd path
//! only). Addition and the lsd direction (build msd, then generic `reverse`) are
//! deferred to Phase 2 — see `docs/DESIGN.md` §8 and the Phase 1 plan
//! (`.claude/plans/fluttering-foraging-spindle.md`) for why: the msd construction
//! alone is enough to exercise the ∃-projection pipeline this spike targets, and
//! `reverse` is unexercised by any query in scope here.

use crate::automaton::Automaton;
use crate::fa::Fa;
use std::collections::BTreeMap;

/// Ports `NumberSystem.determineMsd(List<NumberSystem>)` (`NumberSystem.java:197-209`,
/// package-private `static Boolean`): `None` ("skip the zero fixup") if any track is
/// non-arithmetic (Java: `ns == null`) or if the arithmetic tracks disagree on
/// direction; otherwise the shared direction.
///
/// `msd: &[Option<bool>]` is this crate's stand-in for Java's per-track
/// `List<NumberSystem>` (see [`crate::automaton::Automaton`]'s struct doc comment on
/// `msd`), so `None` plays the `null` role and `Some(b)` plays `ns.isMsd() == b`.
///
/// Java's loop leaves `isMsd = true` untouched for an *empty* list, so zero tracks
/// defaults to msd. This IS reachable through [`crate::quantify::quantify`] — not by
/// quantifying away every track (that is rejected as
/// [`crate::quantify::QuantifyError::AllTracksQuantified`] before this function is ever
/// reached), but by quantifying on an automaton that already has zero tracks: the
/// `a.label.is_empty()` early return leaves `a.msd` empty and unchanged, and `quantify`
/// still unconditionally consults this function afterward (a faithfully-ported quirk —
/// see `quantify`'s module docs).
///
/// Lives here rather than next to `quantify` because it is a `NumberSystem` method in
/// Java, and because `NumberSystem`'s own base-*k* constructions are the other consumer
/// once they land.
pub fn determine_msd(msd: &[Option<bool>]) -> Option<bool> {
    let mut is_msd = true;
    let mut seen_any = false;
    for entry in msd {
        let v = (*entry)?;
        if seen_any && v != is_msd {
            return None;
        }
        is_msd = v;
        seen_any = true;
    }
    Some(is_msd)
}

/// Builds the 2-state lexicographic-less-than automaton over base `base`, msd-first
/// (`NumberSystem.lexicographicLessThan`, called with `isMsd = true`; the lsd
/// direction is `AutomatonLogicalOps.reverse` of this — not built here, see module
/// docs). Two tracks, labeled `"a"`/`"b"` by default (relabel via `automaton.label`
/// before use — e.g. the Phase-1 spike relabels to `["i", "x"]`); accepts iff the
/// first track's value is lexicographically less than the second's, reading digits
/// most-significant-first.
///
/// State 0 (initial, non-accepting): "equal so far" — self-loops on any digit pair
/// `(i, i)`, moves to state 1 on `(i, j)` with `i < j`, and has NO transition on
/// `(i, j)` with `i > j` (once a digit proves `a > b`, the predicate can never
/// become true — a missing transition, not a totalized sink; this automaton is
/// deliberately partial, matching Java exactly).
/// State 1 (accepting, "already decided a < b"): self-loops on every digit pair.
pub fn less_than_msd(base: i32) -> Automaton {
    assert!(base >= 2, "less_than_msd requires base >= 2");
    let digits: Vec<i32> = (0..base).collect();
    let alphabet_size = (base * base) as usize;

    let mut automaton = Automaton::new(
        Fa {
            q0: 0,
            q: 2,
            alphabet_size,
            o: vec![0, 1],
            d: vec![BTreeMap::new(), BTreeMap::new()],
        },
        vec![digits.clone(), digits.clone()],
        vec!["a".to_string(), "b".to_string()],
        vec![Some(true), Some(true)],
    );

    for &i in &digits {
        for &j in &digits {
            let sym = automaton.encode(&[i, j]);
            match i.cmp(&j) {
                std::cmp::Ordering::Equal => {
                    automaton.fa.d[0].entry(sym).or_default().push(0);
                }
                std::cmp::Ordering::Less => {
                    automaton.fa.d[0].entry(sym).or_default().push(1);
                }
                std::cmp::Ordering::Greater => {}
            }
            automaton.fa.d[1].entry(sym).or_default().push(1);
        }
    }

    automaton
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds a two-track word (one digit pair per position, msd-first) into `fa` and
    /// returns whether it's accepted — using `automaton.encode` so the test doesn't
    /// hand-compute symbol ids (that would just re-derive the encoder formula, not
    /// exercise it).
    fn accepts(a: &Automaton, digit_pairs: &[(i32, i32)]) -> bool {
        let word: Vec<i32> = digit_pairs
            .iter()
            .map(|&(x, y)| a.encode(&[x, y]))
            .collect();
        a.fa.accepts_word(&word)
    }

    #[test]
    fn less_than_is_deterministic_and_two_states() {
        let a = less_than_msd(2);
        assert_eq!(a.fa.q, 2);
        assert!(a.fa.is_deterministic());
    }

    #[test]
    fn equal_length_representations_hand_cases() {
        // base 2, 3-digit msd-first representations.
        let a = less_than_msd(2);
        // 010 (=2) < 011 (=3): digits equal at pos0 (0,0) and pos1 (1,1), differ at
        // pos2 (0,1) with 0<1.
        assert!(accepts(&a, &[(0, 0), (1, 1), (0, 1)]));
        // 011 (=3) is NOT less than 010 (=2): differ at pos2 with 1 > 0.
        assert!(!accepts(&a, &[(0, 0), (1, 1), (1, 0)]));
        // Identical representations: never less than.
        assert!(!accepts(&a, &[(1, 1), (0, 0), (1, 1)]));
        // 100 (=4) < 101 (=5).
        assert!(accepts(&a, &[(1, 1), (0, 0), (0, 1)]));
    }

    #[test]
    fn base_3_hand_case() {
        let a = less_than_msd(3);
        // 12 (base3, =5) < 20 (base3, =6): first digit 1<2 decides immediately.
        assert!(accepts(&a, &[(1, 2), (2, 0)]));
        // 20 is not < 12.
        assert!(!accepts(&a, &[(2, 1), (0, 2)]));
    }

    #[test]
    fn a_greater_than_b_dead_ends_partial_automaton() {
        // Once a digit proves a > b, state 0 has no outgoing transition for that
        // symbol and the run dies — must not be misread as "eventually true".
        let a = less_than_msd(2);
        let sym_gt = a.encode(&[1, 0]); // a-digit=1 > b-digit=0
        assert!(!a.fa.d[0].contains_key(&sym_gt));
    }
}
