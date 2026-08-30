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
use std::rc::Rc;
use wr_core::automaton::Automaton;
use wr_core::fa::Fa;
use wr_core::logging::Logging;
use wr_core::numsys::{CustomBaseCandidates, CustomBaseFiles, NumberSystem};
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

// ---------------------------------------------------------------------------
// U7 (idiomatic-refactor): `.clone()` reduction in `numsys.rs`/`automaton.rs`/
// `logicalops.rs`.
//
// U7's REMOVE-list touches six sites:
// - [`wr_core::numsys::NumberSystem::set_addition_automaton`] (a `.clone()` of
//   `addition.alphabet[0]`, used only for the zero/one/per-track-equality
//   validation reads below it, becomes a borrow).
// - [`wr_core::numsys::NumberSystem::with_custom_base_files`] (a `.clone()` of
//   `addition.alphabet[0]`, threaded into `set_less_than_automaton` and
//   `equality_automaton`, becomes a borrow).
// - [`wr_core::automaton::Automaton::apply_all_representations`] /
//   `apply_all_representations_with_output` (the `Rc` clone of
//   `self.all_reps[i]` becomes an `as_ref()` borrow; the DEEP clone one line
//   below it, out of the `Rc`, keeps cloning the same thing it always did --
//   it is re-spelled `(*n).clone()` -> `(**n).clone()` to deref through the
//   now-borrowed `Rc` instead of an owned one, still the same
//   mutated-via-`bind`, load-bearing `Automaton::clone()` per that function's
//   own doc comment).
// - `wr_core::logicalops::flip_ns` (a `.clone()` of the `Option<String>` read
//   out of `a.ns_name.get(i)` becomes an `as_deref()` borrow; already pinned
//   exactly by the pre-existing
//   `flip_ns_flips_the_recorded_number_system_name_not_just_the_direction`
//   test in `logicalops.rs`'s own `#[cfg(test)]` module -- `flip_ns` is
//   `pub(crate)`, so it cannot be exercised directly from this external test
//   crate, and no new test is added here).
// - `wr_core::logicalops::convert_ns` (a `.clone()` of
//   `track_ns_names()[0]`, a temporary with no other reference, becomes
//   `.into_iter().next().flatten()`; already pinned by the pre-existing
//   `convert_ns_parses_the_base_from_the_name_not_the_alphabet_size` test in
//   `logicalops.rs`'s own `#[cfg(test)]` module -- `convert_ns` is `pub`, but
//   this specific read has no behavioral difference to distinguish from a
//   fresh test here, so the existing coverage stands in for it).
//
// None of these change the alphabet, transition table, or any other field of
// the automata under test -- they only change whether an intermediate local
// is owned or borrowed. So every value below is either hand-derived from the
// (unmodified) construction code these functions call, or -- where the
// downstream computation (a cross product inside `apply_all_representations`)
// is impractical to hand-trace exactly -- captured from a live run of the
// current, pre-refactor implementation, same convention as
// `number_system_new_lsd_3_less_than_is_the_msd_shape_reversed` above.
// ---------------------------------------------------------------------------

/// Covers [`NumberSystem::set_addition_automaton`]'s alphabet-validation reads
/// (the `addition.alphabet[0]` clone this unit removes). Hand-derived from
/// `base_n_addition_automaton(2, Msd)`'s own doc comment: two states, `0`
/// (accepting, "carry 0") / `1` ("carry 1"), three tracks over `{0,1}` so
/// `alphabet_size = 8`, and the flat counter `l` runs `i` fastest inside `j`
/// inside `k` (`l = i + 2*j + 4*k`) -- confirmed against a live run before
/// being pinned here.
#[test]
fn number_system_set_addition_automaton_msd_2_has_the_hand_derived_carry_shape() {
    let ns = NumberSystem::new("msd_2").expect("msd_2 is a valid ordinary base");
    let addition = ns.addition();
    let fa = &addition.fa;

    assert_eq!(fa.q0, 0);
    assert_eq!(fa.q, 2);
    assert_eq!(fa.alphabet_size, 8, "three tracks of 2 symbols each: 2^3");
    assert_eq!(fa.o, vec![1, 0]);
    assert_eq!(
        fa.d,
        vec![
            map(&[(0, &[0]), (4, &[1]), (5, &[0]), (6, &[0])]),
            map(&[(1, &[1]), (2, &[1]), (3, &[0]), (7, &[1])]),
        ]
    );
    assert_eq!(addition.msd, vec![Some(true), Some(true), Some(true)]);
}

/// Covers [`NumberSystem::with_custom_base_files`]'s `alphabet` local (the
/// `addition.alphabet[0]` clone this unit removes), on its `equality_automaton`
/// consumer -- the other consumer, `less_than`, is already pinned exactly by
/// `number_system_new_msd_3_less_than_has_the_hand_derived_lexicographic_shape`
/// above. Hand-derived from [`wr_core::numsys::equality_automaton`]'s own
/// construction: a single accepting state self-looping on the diagonal
/// `i*size+i` for `i` in `0..size`, over alphabet `[0,1,2]` (`size = 3`) ->
/// symbols `{0, 4, 8}`.
#[test]
fn number_system_with_custom_base_files_msd_3_equality_has_the_hand_derived_diagonal_shape() {
    let ns = NumberSystem::new("msd_3").expect("msd_3 is a valid ordinary base");
    let eq = &ns.equality;

    assert_eq!(eq.fa.q0, 0);
    assert_eq!(eq.fa.q, 1);
    assert_eq!(eq.fa.alphabet_size, 9);
    assert_eq!(eq.fa.o, vec![1]);
    assert_eq!(eq.fa.d, vec![map(&[(0, &[0]), (4, &[0]), (8, &[0])])]);
    assert_eq!(eq.msd, vec![Some(true), Some(true)]);
}

/// The `lsd_3` twin of the test above: `equality_automaton` "is never reversed
/// for lsd" (its own doc comment -- `:144` sits outside the `if (!isMsd)`
/// blocks in Java), so the `(q0, q, o, d)` shape must be BYTE-IDENTICAL to the
/// `msd_3` case; only the per-track `msd` flag differs. This is exactly what
/// would break if `with_custom_base_files`'s shared `alphabet` local (used by
/// BOTH the `less_than` and `equality` constructors) were accidentally given a
/// direction-dependent value by this unit's edit.
#[test]
fn number_system_with_custom_base_files_lsd_3_equality_is_the_same_shape_direction_flag_only() {
    let ns = NumberSystem::new("lsd_3").expect("lsd_3 is a valid ordinary base");
    let eq = &ns.equality;

    assert_eq!(eq.fa.q0, 0);
    assert_eq!(eq.fa.q, 1);
    assert_eq!(eq.fa.alphabet_size, 9);
    assert_eq!(eq.fa.o, vec![1]);
    assert_eq!(eq.fa.d, vec![map(&[(0, &[0]), (4, &[0]), (8, &[0])])]);
    assert_eq!(eq.msd, vec![Some(false), Some(false)]);
}

/// A one-track automaton over `{0,1}` accepting the words with no `11`
/// substring, replicated from `automaton.rs`'s own `no_adjacent_ones` test
/// helper (private to that module's `#[cfg(test)]`, so re-built here from
/// public API) -- the restriction shape a Fibonacci-style custom base attaches
/// to a track.
fn no_adjacent_ones(label: &str) -> Automaton {
    let mut d0 = BTreeMap::new();
    d0.insert(0, vec![0]);
    d0.insert(1, vec![1]);
    let mut d1 = BTreeMap::new();
    d1.insert(0, vec![0]);
    Automaton::new(
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 1],
            d: vec![d0, d1],
        },
        vec![vec![0, 1]],
        vec![label.to_string()],
        vec![Some(true)],
    )
}

/// The `n`-track total automaton over `{0,1}` accepting everything, replicated
/// from `automaton.rs`'s own `universal_tracks` test helper (same reason as
/// [`no_adjacent_ones`] above).
fn universal_tracks(labels: &[&str], output: i32) -> Automaton {
    let n = labels.len();
    let alphabet_size = 1usize << n;
    let mut d0 = BTreeMap::new();
    for sym in 0..alphabet_size as i32 {
        d0.insert(sym, vec![0usize]);
    }
    Automaton::new(
        Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size,
            o: vec![output],
            d: vec![d0],
        },
        vec![vec![0, 1]; n],
        labels.iter().map(|s| s.to_string()).collect(),
        vec![Some(true); n],
    )
}

/// Covers [`Automaton::apply_all_representations`]'s loop body (the `Rc` clone
/// of `self.all_reps[i]` this unit turns into an `as_ref()` borrow). Two
/// tracks, `x` restricted to [`no_adjacent_ones`] and `y` unrestricted --
/// exactly `automaton.rs`'s own
/// `apply_all_representations_applies_every_restricted_track`/
/// `_intersects_the_restricted_track_only` inputs, which pin this same call
/// only by SEMANTICS (`accepts_word`); this pins the exact resulting
/// `(q0, q, o, d)` too, captured from a live run of the current (unmodified)
/// implementation -- the cross product inside `and` is not practical to
/// hand-trace exactly, same convention as
/// `number_system_new_lsd_3_less_than_is_the_msd_shape_reversed` above.
#[test]
fn apply_all_representations_single_restricted_track_has_this_exact_shape() {
    let mut a = universal_tracks(&["x", "y"], 1);
    a.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("ignored"))), None]);
    a.apply_all_representations(&mut Logging::new());

    assert_eq!(a.fa.q0, 1);
    assert_eq!(a.fa.q, 2);
    assert_eq!(a.fa.alphabet_size, 4);
    assert_eq!(a.fa.o, vec![1, 1]);
    assert_eq!(
        a.fa.d,
        vec![
            map(&[(0, &[1]), (2, &[1])]),
            map(&[(0, &[1]), (1, &[0]), (2, &[1]), (3, &[0])]),
        ]
    );
    assert_eq!(a.label, vec!["x", "y"]);
    assert_eq!(a.alphabet, vec![vec![0, 1], vec![0, 1]]);
    assert_eq!(a.msd, vec![Some(true), Some(true)]);
}

/// The `apply_all_representations_with_output` twin of the test above (covers
/// the SAME clone-removal shape, in the sibling function this unit also
/// touches) -- same inputs except the base automaton's single state carries a
/// DFAO output of `7` rather than the boolean `1`, so this also pins that the
/// output value survives (`IF_OTHER_OP`, not the plain `and` this function's
/// sibling uses). Captured from a live run of the current implementation, same
/// reasoning as above.
#[test]
fn apply_all_representations_with_output_single_restricted_track_has_this_exact_shape() {
    let mut b = universal_tracks(&["x", "y"], 7);
    b.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("ignored"))), None]);
    b.apply_all_representations_with_output(&mut Logging::new());

    assert_eq!(b.fa.q0, 0);
    assert_eq!(b.fa.q, 2);
    assert_eq!(b.fa.alphabet_size, 4);
    assert_eq!(b.fa.o, vec![7, 7]);
    assert_eq!(
        b.fa.d,
        vec![
            map(&[(0, &[0]), (1, &[1]), (2, &[0]), (3, &[1])]),
            map(&[(0, &[0]), (2, &[0])]),
        ]
    );
    assert_eq!(b.label, vec!["x", "y"]);
    assert_eq!(b.alphabet, vec![vec![0, 1], vec![0, 1]]);
    assert_eq!(b.msd, vec![Some(true), Some(true)]);
}

// ---------------------------------------------------------------------------
// U9 (idiomatic-refactor), Stage A: track-structure snapshots.
//
// `Automaton` today carries four parallel `Vec`s (`alphabet`/`msd`/`all_reps`/
// `ns_name`) plus the deliberately-NOT-parallel `label`. U9 introduces a `Track`
// type + a delegating accessor surface in Stage A (storage unchanged), migrates
// every direct four-vector access workspace-wide onto that surface in Stage B,
// then flips storage to `tracks: Vec<Track>` in Stage C. These tests capture the
// exact CURRENT per-track structure of four representative automata -- built
// from real `NumberSystem` construction paths, not hand-rolled `Fa` tables, so
// they exercise the real code that populates `ns_name`/`all_reps`/`msd` rather
// than a value this file just asserts back at itself -- so Stage B/C's storage
// change has a byte-exact contract to preserve. Both the OLD raw fields (the
// storage pinned) AND the NEW Stage-A accessors (the delegation pinned) are
// checked against the same expected values, so a Stage B/C regression in either
// direction shows up here.
// ---------------------------------------------------------------------------

/// Asserts one automaton's full track structure -- `alphabet`/`msd`/`ns_name`
/// (per `Automaton::track_ns_name_raw`'s "raw, not the `track_ns_names()`
/// reconstructed fallback" distinction) against `expected_ns_name`, and `label`
/// checked SEPARATELY (per its own documented carve-out: `label.len()` need not
/// equal `alphabet.len()`) -- through both the raw `pub` fields and the new
/// Stage-A `Track` accessors, which must agree exactly since both read the same
/// unchanged storage today.
fn assert_track_structure(
    a: &Automaton,
    expected_alphabet: &[Vec<i32>],
    expected_msd: &[Option<bool>],
    expected_ns_name: &[Option<&str>],
    expected_label: &[&str],
) {
    // The raw parallel-vector fields -- today's actual storage.
    assert_eq!(a.alphabet, expected_alphabet, "alphabet field");
    assert_eq!(a.msd, expected_msd, "msd field");
    assert_eq!(
        a.ns_name.iter().map(|n| n.as_deref()).collect::<Vec<_>>(),
        expected_ns_name,
        "ns_name field"
    );
    assert_eq!(
        a.label, expected_label,
        "label field (NOT parallel to the rest)"
    );

    // The new Stage-A accessor surface -- must delegate onto the exact same values.
    assert_eq!(a.track_count(), expected_alphabet.len(), "track_count()");
    assert_eq!(a.track_alphabets(), expected_alphabet, "track_alphabets()");
    assert_eq!(a.track_msds(), expected_msd, "track_msds()");
    assert_eq!(
        a.track_ns_names_raw()
            .iter()
            .map(|n| n.as_deref())
            .collect::<Vec<_>>(),
        expected_ns_name,
        "track_ns_names_raw()"
    );
    for i in 0..expected_alphabet.len() {
        assert_eq!(
            a.track_alphabet(i),
            &expected_alphabet[i][..],
            "track_alphabet({i})"
        );
        assert_eq!(a.track_msd(i), expected_msd[i], "track_msd({i})");
        assert_eq!(
            a.track_ns_name_raw(i),
            expected_ns_name[i],
            "track_ns_name_raw({i})"
        );
        let t = a.track(i);
        assert_eq!(t.alphabet, expected_alphabet[i], "track({i}).alphabet");
        assert_eq!(t.msd, expected_msd[i], "track({i}).msd");
        assert_eq!(
            t.ns_name.as_deref(),
            expected_ns_name[i],
            "track({i}).ns_name"
        );
    }
}

/// A plain ordinary base, two tracks, both arithmetic (msd), no custom-base
/// restriction. `NumberSystem::with_custom_base_files` sets `ns_name` on EVERY
/// track for EVERY number system it constructs -- including an ordinary base
/// with no custom file at all -- so `less_than()` already carries
/// `Some("msd_2")` on both tracks before this test does anything (confirmed by
/// reading `with_custom_base_files`'s own doc comment: "For a plain `msd_k`
/// this is exactly what `track_ns_names` would reconstruct anyway"). `bind`
/// installs the two distinct variable names an `eval`-style query would use.
#[test]
fn track_structure_of_a_plain_msd_2_bound_automaton() {
    let ns = NumberSystem::new("msd_2").expect("msd_2 is a valid ordinary base");
    let mut a = ns.less_than().clone();
    a.bind(vec!["x".to_string(), "y".to_string()]);

    assert_track_structure(
        &a,
        &[vec![0, 1], vec![0, 1]],
        &[Some(true), Some(true)],
        &[Some("msd_2"), Some("msd_2")],
        &["x", "y"],
    );
    assert!(
        a.all_reps.iter().all(Option::is_none),
        "an ordinary base has no all-reps restriction on any track"
    );
}

/// A genuine custom base (hand-built rather than file-loaded, so this test has
/// no dependency on `walnut-java/Custom Bases/` -- exactly the pattern
/// `numsys.rs`'s own `#[cfg(test)]` module uses for `msd_fib`, e.g.
/// `with_custom_base_files`'s test call sites): a resolved addition file (3
/// tracks) AND a resolved all-representations file, so every track ends up
/// with BOTH a real recorded `ns_name` (`"msd_myfib"`, not the `msd_2`
/// reconstruction its alphabet cardinality alone would suggest) AND a real
/// `all_reps` restriction -- the two facts an ordinary base's tracks never
/// carry, which is exactly what distinguishes a `Track`'s four fields from
/// each other. `ns.addition()`'s own construction (not this test) applies the
/// all-representations restriction and, per `Automaton::apply_all_representations`'s
/// documented label bookkeeping, leaves it bound to `randomLabel`'s numeric
/// names before `bind()` below overwrites them -- included here as further
/// proof `bind()` does not care about an automaton's PRIOR binding state.
#[test]
fn track_structure_of_a_custom_base_bound_automaton() {
    let mut logging = Logging::new();
    let files = CustomBaseFiles {
        addition: CustomBaseCandidates {
            main: Some(universal_tracks(&["_0", "_1", "_2"], 1)),
            complement: None,
        },
        less_than: CustomBaseCandidates::default(),
        all_representations: CustomBaseCandidates {
            main: Some(no_adjacent_ones("ignored")),
            complement: None,
        },
    };
    let ns = NumberSystem::with_custom_base_files("msd_myfib", files, &mut logging)
        .expect("a hand-built custom base with a resolved addition file must construct");
    let mut a = ns.addition().clone();
    a.bind(vec!["a".to_string(), "b".to_string(), "c".to_string()]);

    assert_track_structure(
        &a,
        &[vec![0, 1], vec![0, 1], vec![0, 1]],
        &[Some(true), Some(true), Some(true)],
        &[Some("msd_myfib"), Some("msd_myfib"), Some("msd_myfib")],
        &["a", "b", "c"],
    );
    assert!(
        a.all_reps.iter().all(Option::is_some),
        "a custom base with a resolved all-representations file restricts every track"
    );
}

/// A multi-track (3-track) automaton on an ORDINARY base -- `NumberSystem`'s
/// addition automaton is always 3-track (two addends + a sum), which
/// distinguishes this case's track COUNT from the plain 2-track comparison
/// case above without introducing a custom base's extra facets, isolating the
/// "more than two tracks" dimension on its own.
#[test]
fn track_structure_of_a_multi_track_msd_2_bound_automaton() {
    let ns = NumberSystem::new("msd_2").expect("msd_2 is a valid ordinary base");
    let mut a = ns.addition().clone();
    a.bind(vec!["x".to_string(), "y".to_string(), "z".to_string()]);

    assert_track_structure(
        &a,
        &[vec![0, 1], vec![0, 1], vec![0, 1]],
        &[Some(true), Some(true), Some(true)],
        &[Some("msd_2"), Some("msd_2"), Some("msd_2")],
        &["x", "y", "z"],
    );
    assert!(a.all_reps.iter().all(Option::is_none));
}

/// The unbound case: `NumberSystem`'s own cached `lessThan`/`addition`/`equality`
/// automata start life UNBOUND (`init_basic_automaton`'s own doc comment: "these
/// automata are bound later, by `comparison`/`arithmetic`") for any ordinary
/// base, so simply never calling `bind` on a freshly constructed one is enough
/// to exercise `label.len() != alphabet.len()` through a REAL construction path
/// rather than a hand-built one -- this is the one case where `label` is
/// intentionally NOT the same length as every other vector, the exact
/// carve-out `Track` deliberately excludes `label` to preserve.
#[test]
fn track_structure_of_an_unbound_msd_2_automaton() {
    let ns = NumberSystem::new("msd_2").expect("msd_2 is a valid ordinary base");
    let a = ns.less_than();

    assert!(
        !a.is_bound(),
        "sanity: NumberSystem's own automata start unbound"
    );
    assert_eq!(a.label.len(), 0, "label is empty, not merely mismatched");
    assert_track_structure(
        a,
        &[vec![0, 1], vec![0, 1]],
        &[Some(true), Some(true)],
        &[Some("msd_2"), Some("msd_2")],
        &[], // label deliberately NOT parallel -- 0 entries against 2 tracks.
    );
}
