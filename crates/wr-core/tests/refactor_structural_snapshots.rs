// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Structural (exact `(q0, q, o, d)`) snapshot tests, captured from the UNCHANGED
//! implementation, ahead of the idiomatic-refactor unit U6 (`for i in 0..n`
//! index-loop -> iterator conversion in `numsys.rs`/`fa.rs`/`search.rs`/
//! `ostrowski.rs`/`minimize.rs`/`trim.rs`).
//!
//! U6's convert-list touches loops inside these public-path functions:
//! - [`wr_core::fa::Fa::totalize`]
//! - [`wr_core::fa::Fa::reverse`]
//! - [`wr_core::fa::Fa::concat_states`]
//! - [`wr_core::fa::Fa::restrict_output_to`]
//! - [`wr_core::fa::Fa::add_distinguished_dead_state`] (exercises the `pub(crate)`
//!   [`wr_core::fa::Fa::totalize_relaxed`], which this external test crate cannot
//!   name directly)
//! - [`wr_core::trim::trim`] (the empty-language / `keep.is_empty()` branch)
//! - [`wr_core::numsys::NumberSystem::new`] (exercises
//!   `NumberSystem::set_less_than_automaton`'s per-track alphabet-validation loop,
//!   both the `msd` and `lsd` branches)
//!
//! Every automaton here is small and hand-built (or, where the construction is a
//! real `NumberSystem`/`lexicographic_less_than` automaton too large to safely
//! hand-derive, hand-derived from the documented construction rules and then
//! cross-checked against a live run) so the expected `(q0, q, o, d)` is fully
//! traceable from the current, unmodified source rather than copied blind from
//! program output. These tests assert full transition-table equality — including
//! `Vec<usize>` destination-list order and exact `BTreeMap` key sets — and are
//! PERMANENT (not deleted after the refactor lands): they are the regression gate
//! that proves the loop-shape change in each function did not alter its output.

use std::collections::BTreeMap;
use wr_core::fa::Fa;
use wr_core::numsys::NumberSystem;
use wr_core::trim::trim;

fn map(entries: &[(i32, &[usize])]) -> BTreeMap<i32, Vec<usize>> {
    entries.iter().map(|&(k, v)| (k, v.to_vec())).collect()
}

// ---------------------------------------------------------------------------
// Fa::totalize
// ---------------------------------------------------------------------------

/// `Fa::totalize`'s hot loop (`fa.rs`) fills every missing `(state, symbol)` pair
/// of the states BEFORE the new sink with `vec![sink]`, then appends a
/// self-looping sink state. Hand-derived: state 0 already has symbol 0 -> 1 (kept
/// untouched); every other missing pair, plus the sink's own two self-loops, is
/// filled in ascending `(sym, state)`/`(sym)` order -- but since every fill target
/// is a `BTreeMap::entry` keyed by symbol, the final per-state map is
/// order-independent of insertion sequence, only of the KEY SET, which this test
/// pins.
#[test]
fn fa_totalize_fills_missing_transitions_with_a_new_sink_state() {
    let mut fa = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 2,
        o: vec![1, 0],
        d: vec![map(&[(0, &[1])]), map(&[])],
    };

    fa.totalize(9);

    assert_eq!(fa.q0, 0);
    assert_eq!(fa.q, 3);
    assert_eq!(fa.o, vec![1, 0, 9]);
    assert_eq!(
        fa.d,
        vec![
            map(&[(0, &[1]), (1, &[2])]),
            map(&[(0, &[2]), (1, &[2])]),
            map(&[(0, &[2]), (1, &[2])]),
        ]
    );
}

/// A no-op case: every `(state, symbol)` pair already has exactly one
/// destination, so `totalize` must leave the automaton byte-for-byte unchanged
/// (no sink appended).
#[test]
fn fa_totalize_is_a_no_op_on_an_already_total_dfa() {
    let mut fa = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 2,
        o: vec![0, 1],
        d: vec![map(&[(0, &[1]), (1, &[0])]), map(&[(0, &[1]), (1, &[1])])],
    };
    let before = fa.clone();

    fa.totalize(9);

    assert_eq!(fa.q0, before.q0);
    assert_eq!(fa.q, before.q);
    assert_eq!(fa.o, before.o);
    assert_eq!(fa.d, before.d);
}

// ---------------------------------------------------------------------------
// Fa::reverse
// ---------------------------------------------------------------------------

/// `Fa::reverse` on a 3-state cycle `0 -(0)-> 1 -(0)-> 2 -(0)-> 0` with state 1
/// accepting and `q0 == 0`. Hand-derived: every edge reverses direction, and the
/// accepting/initial roles swap -- the previously-accepting state 1 becomes an
/// initial state (returned, and its own output cleared to 0), and the caller's
/// `old_initial_states = {0}` becomes accepting (`o[0] = 1`). `self.q0` is left
/// stale (documented, unchanged) at its old value.
#[test]
fn fa_reverse_reverses_edges_and_swaps_initial_and_accepting_roles() {
    let mut fa = Fa {
        true_false: None,
        q0: 0,
        q: 3,
        alphabet_size: 1,
        o: vec![0, 1, 0],
        d: vec![map(&[(0, &[1])]), map(&[(0, &[2])]), map(&[(0, &[0])])],
    };
    let old_initial: std::collections::BTreeSet<usize> = [0].into_iter().collect();

    let new_initial = fa.reverse(&old_initial);

    let expected_new_initial: std::collections::BTreeSet<usize> = [1].into_iter().collect();
    assert_eq!(new_initial, expected_new_initial);
    assert_eq!(fa.q0, 0, "q0 is left stale, matching Java");
    assert_eq!(fa.q, 3);
    assert_eq!(fa.o, vec![1, 0, 0]);
    assert_eq!(
        fa.d,
        vec![map(&[(0, &[2])]), map(&[(0, &[0])]), map(&[(0, &[1])])]
    );
}

/// A multi-initial-state seed: reversing with `old_initial_states = {0, 2}` on
/// the same cycle marks BOTH as accepting afterward, not just one.
#[test]
fn fa_reverse_with_a_multi_state_seed_marks_every_seed_state_accepting() {
    let mut fa = Fa {
        true_false: None,
        q0: 0,
        q: 3,
        alphabet_size: 1,
        o: vec![0, 1, 0],
        d: vec![map(&[(0, &[1])]), map(&[(0, &[2])]), map(&[(0, &[0])])],
    };
    let old_initial: std::collections::BTreeSet<usize> = [0, 2].into_iter().collect();

    let new_initial = fa.reverse(&old_initial);

    let expected_new_initial: std::collections::BTreeSet<usize> = [1].into_iter().collect();
    assert_eq!(new_initial, expected_new_initial);
    assert_eq!(fa.o, vec![1, 0, 1]);
    assert_eq!(
        fa.d,
        vec![map(&[(0, &[2])]), map(&[(0, &[0])]), map(&[(0, &[1])])]
    );
}

// ---------------------------------------------------------------------------
// Fa::concat_states
// ---------------------------------------------------------------------------

/// `first`: 2 states, state 1 accepting WITH A DFAO OUTPUT OF `2` (not `1`) --
/// deliberately, so the assertion below can actually distinguish "collapsed to
/// `1`" from "left alone": a state that was already `1` before the call would
/// still read `1` after even if the inlined `Fa::set_output_if_equal` body were
/// buggily replaced with a no-op. `other`: 2 states, state 0 (== `other.q0`)
/// accepting -- so `other` accepts epsilon, which must keep `first`'s own
/// accepting state (state 1) accepting after concatenation (not just graft
/// `other`'s transitions onto it).
///
/// Hand-derived per `concat_states`'s doc comment: `other`'s states are appended
/// at indices `original_q..original_q+other.q` (`other` state 0 -> `n` state 2,
/// `other` state 1 -> `n` state 3, with every destination shifted by
/// `+original_q`); `other`'s ACTUAL `q0` (state 0, i.e. `n` state 2)'s
/// transitions are grafted onto every of `first`'s own accepting states (just
/// state 1 here); and because `other` accepts epsilon, state 1's accepting flag
/// is explicitly kept -- but per `concat_states`'s own doc comment ("this writes
/// a plain `0`/`1`... a `first`-operand accepting state with a DFAO output `> 1`
/// ... has that output collapsed to `1` here, not preserved"), "kept" means
/// collapsed to the canonical `1`, not left at its original `2` (confirmed
/// against a live run, not assumed).
#[test]
fn fa_concat_states_grafts_other_and_keeps_first_accepting_when_other_accepts_epsilon() {
    let mut n = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 1,
        o: vec![0, 2],
        d: vec![map(&[(0, &[1])]), map(&[])],
    };
    let other = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 1,
        o: vec![1, 0],
        d: vec![map(&[(0, &[1])]), map(&[(0, &[0])])],
    };
    let original_q = 2;

    Fa::concat_states(&other, &mut n, original_q);

    assert_eq!(n.q0, 0, "concat_states never touches q0");
    assert_eq!(n.q, 4);
    assert_eq!(
        n.o,
        vec![0, 1, 1, 0],
        "state 1's DFAO output must collapse from 2 to the canonical 1, not be left at 2"
    );
    assert_eq!(
        n.d,
        vec![
            map(&[(0, &[1])]),
            map(&[(0, &[3])]),
            map(&[(0, &[3])]),
            map(&[(0, &[2])]),
        ]
    );
}

/// Same shapes, but `other`'s `q0` (state 0) is NOT accepting -- `other` rejects
/// epsilon, so `first`'s own accepting state (state 1) must have its accepting
/// flag CLEARED after the graft (WB-009).
#[test]
fn fa_concat_states_clears_first_accepting_when_other_rejects_epsilon() {
    let mut n = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 1,
        o: vec![0, 1],
        d: vec![map(&[(0, &[1])]), map(&[])],
    };
    let other = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 1,
        o: vec![0, 1],
        d: vec![map(&[(0, &[1])]), map(&[(0, &[0])])],
    };
    let original_q = 2;

    Fa::concat_states(&other, &mut n, original_q);

    assert_eq!(n.q, 4);
    assert_eq!(n.o, vec![0, 0, 0, 1]);
    assert_eq!(
        n.d,
        vec![
            map(&[(0, &[1])]),
            map(&[(0, &[3])]),
            map(&[(0, &[3])]),
            map(&[(0, &[2])]),
        ]
    );
}

// ---------------------------------------------------------------------------
// Fa::restrict_output_to
// ---------------------------------------------------------------------------

/// Collapses every state whose DFAO output equals `2` to `1`, everything else to
/// `0`. Pure in-place `o` rewrite; `q0`/`q`/`d` untouched.
#[test]
fn fa_restrict_output_to_collapses_matching_outputs_to_one() {
    let mut fa = Fa {
        true_false: None,
        q0: 1,
        q: 3,
        alphabet_size: 1,
        o: vec![0, 2, 5],
        d: vec![map(&[]), map(&[]), map(&[])],
    };

    fa.restrict_output_to(2);

    assert_eq!(fa.q0, 1);
    assert_eq!(fa.q, 3);
    assert_eq!(fa.o, vec![0, 1, 0]);
    assert_eq!(fa.d, vec![map(&[]), map(&[]), map(&[])]);
}

// ---------------------------------------------------------------------------
// Fa::add_distinguished_dead_state (exercises the pub(crate) totalize_relaxed)
// ---------------------------------------------------------------------------

/// State 0 has `0 -(0)-> 1` but is missing symbol 1; state 1 has no transitions
/// at all. Minimum output is `0`, so the dead state's output must be `-1`.
/// Hand-derived per `totalize_relaxed`'s key-presence-only (not
/// destination-count) fill discipline: both states get every missing
/// `(state, symbol)` pair routed to the fresh sink (index `2`), and the sink
/// self-loops on every symbol.
#[test]
fn fa_add_distinguished_dead_state_totalizes_via_the_relaxed_pass() {
    let mut fa = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 2,
        o: vec![0, 1],
        d: vec![map(&[(0, &[1])]), map(&[])],
    };

    let added = fa.add_distinguished_dead_state();

    assert!(added);
    assert_eq!(fa.q0, 0);
    assert_eq!(fa.q, 3);
    assert_eq!(fa.o, vec![0, 1, -1]);
    assert_eq!(
        fa.d,
        vec![
            map(&[(0, &[1]), (1, &[2])]),
            map(&[(0, &[2]), (1, &[2])]),
            map(&[(0, &[2]), (1, &[2])]),
        ]
    );
}

/// Already-total (every pair present, one destination each, per the RELAXED
/// key-presence definition used here -- see `Fa::add_distinguished_dead_state`'s
/// own doc comment on why this is a different notion from
/// `is_deterministic_and_total`): no dead state is added.
#[test]
fn fa_add_distinguished_dead_state_is_a_no_op_when_every_pair_is_present() {
    let mut fa = Fa {
        true_false: None,
        q0: 0,
        q: 1,
        alphabet_size: 1,
        o: vec![1],
        d: vec![map(&[(0, &[0])])],
    };
    let before = fa.clone();

    let added = fa.add_distinguished_dead_state();

    assert!(!added);
    assert_eq!(fa.q0, before.q0);
    assert_eq!(fa.q, before.q);
    assert_eq!(fa.o, before.o);
    assert_eq!(fa.d, before.d);
}

// ---------------------------------------------------------------------------
// trim's empty-language branch
// ---------------------------------------------------------------------------

/// No state has nonzero output, so the backward co-reachable-to-accepting set is
/// empty regardless of forward reachability -- `keep` is empty, and `trim`
/// returns its canonical 1-state non-accepting automaton, self-looping on every
/// symbol. Hand-derived directly from `trim`'s `keep.is_empty()` branch.
#[test]
fn trim_collapses_a_language_with_no_accepting_state_to_the_canonical_empty_automaton() {
    let fa = Fa {
        true_false: None,
        q0: 0,
        q: 2,
        alphabet_size: 3,
        o: vec![0, 0],
        d: vec![map(&[(0, &[1])]), map(&[(0, &[0])])],
    };

    let trimmed = trim(&fa);

    assert_eq!(trimmed.q0, 0);
    assert_eq!(trimmed.q, 1);
    assert_eq!(trimmed.alphabet_size, 3);
    assert_eq!(trimmed.o, vec![0]);
    assert_eq!(trimmed.d, vec![map(&[(0, &[0]), (1, &[0]), (2, &[0])])]);
}

/// Same branch with a single-symbol alphabet, to pin the loop at its smallest
/// nontrivial size too.
#[test]
fn trim_collapses_to_the_canonical_empty_automaton_with_a_single_symbol_alphabet() {
    let fa = Fa {
        true_false: None,
        q0: 0,
        q: 1,
        alphabet_size: 1,
        o: vec![0],
        d: vec![map(&[(0, &[0])])],
    };

    let trimmed = trim(&fa);

    assert_eq!(trimmed.q, 1);
    assert_eq!(trimmed.alphabet_size, 1);
    assert_eq!(trimmed.o, vec![0]);
    assert_eq!(trimmed.d, vec![map(&[(0, &[0])])]);
}

// ---------------------------------------------------------------------------
// NumberSystem::new -> set_less_than_automaton's per-track validation loop
// ---------------------------------------------------------------------------

/// `NumberSystem::new("msd_3")` has no custom-base files, so
/// `set_less_than_automaton` falls to `lexicographic_less_than([0,1,2], Msd)`
/// (base "3" is not a negative-base name) with NO reversal (msd). Hand-derived
/// from `lexicographic_less_than`'s two documented symbol expressions
/// (`j*size+i` at state 0, `i*size+j` at state 1's unconditional self-loop) over
/// `size = 3`.
#[test]
fn number_system_new_msd_3_less_than_has_the_hand_derived_lexicographic_shape() {
    let ns = NumberSystem::new("msd_3").expect("msd_3 is a valid ordinary base");
    let less_than = ns.less_than();
    let fa = &less_than.fa;

    assert_eq!(fa.q0, 0);
    assert_eq!(fa.q, 2);
    assert_eq!(fa.alphabet_size, 9, "two tracks of 3 symbols each: 3*3");
    assert_eq!(fa.o, vec![0, 1]);
    assert_eq!(
        fa.d,
        vec![
            map(&[
                (0, &[0]),
                (3, &[1]),
                (4, &[0]),
                (6, &[1]),
                (7, &[1]),
                (8, &[0])
            ]),
            map(&[
                (0, &[1]),
                (1, &[1]),
                (2, &[1]),
                (3, &[1]),
                (4, &[1]),
                (5, &[1]),
                (6, &[1]),
                (7, &[1]),
                (8, &[1]),
            ]),
        ]
    );
    // The converted loop's ONLY side effect: per-track `msd = Some(direction ==
    // Msd)`. Two tracks, direction msd -> both `Some(true)` (confirmed live).
    assert_eq!(less_than.msd, vec![Some(true), Some(true)]);
}

/// `NumberSystem::new("lsd_3")` takes the SAME `lexicographic_less_than` shape as
/// `msd_3` (the construction itself does not depend on direction) but then
/// reverses it (`set_less_than_automaton`'s `if direction == Lsd { reverse(...) }`
/// step, `MsdFlip::Keep`) before the per-track validation loop this unit touches
/// ever runs -- so this pins BOTH that this loop still runs correctly on a
/// POST-reversal automaton, and that the reversal itself (a different, untouched
/// function) is unaffected by this unit.
#[test]
fn number_system_new_lsd_3_less_than_is_the_msd_shape_reversed() {
    let ns = NumberSystem::new("lsd_3").expect("lsd_3 is a valid ordinary base");
    let less_than = ns.less_than();
    let fa = &less_than.fa;

    // `reverse(&mut less_than, MsdFlip::Keep)` here is `crate::logicalops::reverse`
    // (Automaton-level -- takes `&mut Automaton`, not `Fa::reverse`'s
    // `&mut Fa` + explicit initial-state set), which redetermizes after reversing
    // a naive edge-reversal of a deterministic automaton is not generally
    // deterministic itself -- so this table is NOT simply
    // `number_system_new_msd_3_less_than_...`'s table with edges flipped
    // (confirmed by hand-tracing raw `Fa::reverse` against that table and finding
    // it does NOT match this one). Captured from a live run instead; `q`
    // happening to stay 2 is this specific automaton's shape, not a general
    // property of `logicalops::reverse`.
    assert_eq!(fa.q0, 0);
    assert_eq!(fa.q, 2);
    assert_eq!(fa.alphabet_size, 9);
    assert_eq!(fa.o, vec![0, 1]);
    assert_eq!(
        fa.d,
        vec![
            map(&[
                (0, &[0]),
                (1, &[0]),
                (2, &[0]),
                (3, &[1]),
                (4, &[0]),
                (5, &[0]),
                (6, &[1]),
                (7, &[1]),
                (8, &[0]),
            ]),
            map(&[
                (0, &[1]),
                (1, &[0]),
                (2, &[0]),
                (3, &[1]),
                (4, &[1]),
                (5, &[0]),
                (6, &[1]),
                (7, &[1]),
                (8, &[1]),
            ]),
        ]
    );
    // Same converted loop, direction lsd this time -> both tracks `Some(false)`
    // (confirmed live, not assumed from the msd_3 case above).
    assert_eq!(less_than.msd, vec![Some(false), Some(false)]);
}
