// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! `wr_core::resource` driven through the REAL construction primitives (not the
//! module's own unit tests, which exercise the scope machinery on synthetic events):
//! the trajectory an observer sees from `subset_construction`/`minimize`/the cross
//! product, the exact breach each primitive reports under a budget, and — the
//! drop-in invariant — that observing a construction does not change its output.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use wr_core::automaton::Automaton;
use wr_core::determinize::{determinize, subset_construction, Strategy};
use wr_core::fa::Fa;
use wr_core::logging::Logging;
use wr_core::minimize::{minimize, MinimizeError, Minimizer};
use wr_core::otf::OtfPolicy;
use wr_core::product::cross_product_internal;
use wr_core::resource::{
    run, BudgetError, DeterminizationRecord, Event, Exhausted, ExhaustedReason, Instrumentation,
    Meter, Operation, ResourceBudget, Trajectory,
};
use wr_core::walnut_panic::catch_walnut_panic;

fn row(pairs: &[(i32, &[usize])]) -> BTreeMap<i32, Vec<usize>> {
    pairs.iter().map(|&(s, d)| (s, d.to_vec())).collect()
}

/// "The k-th symbol from the end is 1" over {0,1}: the textbook NFA whose subset
/// construction is genuinely 2^k states, and whose minimal DFA is ALSO 2^k states —
/// a *real* explosion, not a transient one.
fn kth_from_end_nfa(k: usize) -> Fa {
    let mut d = Vec::new();
    d.push(row(&[(0, &[0]), (1, &[0, 1])]));
    for i in 1..k {
        d.push(row(&[(0, &[i + 1]), (1, &[i + 1])]));
    }
    d.push(row(&[]));
    let mut o = vec![0; k + 1];
    o[k] = 1;
    Fa::with_states(0, k + 1, 2, o, d)
}

/// "Ends with 1", with a redundant state 2 that adds nothing to the language but does
/// add a metastate: subset construction yields {0}, {0,1,2}, {0,2} (3 states, one per
/// BFS level — the third level's expansion discovers nothing new, so there is no
/// fourth), and minimization collapses {0} ≡ {0,2} down to 2 — a *transient*
/// explosion in miniature.
fn ends_with_one_redundant_nfa() -> Fa {
    Fa::with_states(
        0,
        3,
        2,
        vec![0, 1, 0],
        vec![
            row(&[(0, &[0]), (1, &[0, 1, 2])]),
            row(&[]),
            row(&[(0, &[2]), (1, &[1])]),
        ],
    )
}

/// Field-for-field structural identity (`Fa` deliberately has no `PartialEq`; the
/// crate's rule is to compare languages — but here the claim IS structural).
fn same_fa(a: &Fa, b: &Fa) -> bool {
    a.q0 == b.q0
        && a.q == b.q
        && a.alphabet_size == b.alphabet_size
        && a.o == b.o
        && a.d == b.d
        && a.true_false == b.true_false
}

fn shared() -> Rc<RefCell<Trajectory>> {
    Rc::new(RefCell::new(Trajectory::new()))
}

fn observed() -> (Instrumentation, Rc<RefCell<Trajectory>>) {
    let t = shared();
    (Instrumentation::new().with_observer(t.clone()), t)
}

#[test]
fn observing_a_subset_construction_does_not_change_its_output() {
    let fa = kth_from_end_nfa(4);
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let plain = subset_construction(&fa, &initial);
    let (instr, t) = observed();
    let observed = run(&instr, || subset_construction(&fa, &initial)).unwrap();
    assert!(
        same_fa(&observed, &plain),
        "an observer must be invisible to the result"
    );
    assert_eq!(plain.q, 16);
    assert_eq!(t.borrow().peak_states(), 16);
}

#[test]
fn the_trajectory_reports_levels_and_the_final_metastate_count() {
    let fa = kth_from_end_nfa(3);
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let (instr, t) = observed();
    run(&instr, || subset_construction(&fa, &initial)).unwrap();
    let t = t.borrow();
    let events = t.events();
    assert_eq!(
        events[0],
        Event::SubsetConstructionStarted {
            input_states: 4,
            initial_size: 1
        }
    );
    // Level 0 is the initial metastate alone; every later level's `metastates` is the
    // running total, which never decreases.
    assert_eq!(
        events[1],
        Event::SubsetLevel {
            level: 0,
            frontier: 1,
            members: 1,
            metastates: 1
        }
    );
    let mut last_total = 0;
    let mut levels = 0;
    for e in events {
        if let Event::SubsetLevel {
            level, metastates, ..
        } = e
        {
            assert_eq!(*level, levels);
            assert!(*metastates >= last_total);
            last_total = *metastates;
            levels += 1;
        }
    }
    assert_eq!(
        *events.last().unwrap(),
        Event::SubsetConstructionFinished { states: 8, levels }
    );
}

#[test]
fn a_transient_explosion_shows_as_peak_above_minimized() {
    let fa = ends_with_one_redundant_nfa();
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let (instr, t) = observed();
    run(&instr, || {
        let dfa = subset_construction(&fa, &initial);
        minimize(&dfa).unwrap()
    })
    .unwrap();
    assert_eq!(
        t.borrow().determinizations(),
        vec![DeterminizationRecord {
            input_states: 3,
            levels: 3,
            peak_states: 3,
            minimized: Some(2),
        }]
    );
}

#[test]
fn a_real_explosion_shows_as_peak_equal_to_minimized() {
    let fa = kth_from_end_nfa(3);
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let (instr, t) = observed();
    run(&instr, || {
        minimize(&subset_construction(&fa, &initial)).unwrap()
    })
    .unwrap();
    let recs = t.borrow().determinizations();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].peak_states, 8);
    assert_eq!(recs[0].minimized, Some(8));
}

#[test]
fn the_dispatcher_reports_its_strategy() {
    let fa = kth_from_end_nfa(2);
    let mut a = Automaton::new(
        fa,
        vec![vec![0, 1]],
        vec!["x".to_string()],
        vec![Some(true)],
    );
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let (instr, t) = observed();
    run(&instr, || {
        determinize(&mut a, &initial, None, &mut Logging::new()).unwrap();
    })
    .unwrap();
    assert_eq!(
        t.borrow().events()[0],
        Event::Determinize {
            strategy: Strategy::Sc,
            input_states: 3
        }
    );
    assert_eq!(a.fa.q, 4);
}

#[test]
fn a_state_cap_stops_subset_construction_at_the_first_metastate_past_it() {
    let fa = kth_from_end_nfa(4); // 16 metastates if allowed to finish
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let instr = Instrumentation::new().with_budget(ResourceBudget::states(8));
    let outcome = run(&instr, || subset_construction(&fa, &initial));
    match outcome {
        Err(BudgetError::Exhausted(e)) => {
            assert_eq!(e.reason, ExhaustedReason::States);
            assert_eq!(e.operation, Operation::SubsetConstruction);
            assert_eq!(e.limit, 8);
            // Checked after every merged metastate: one metastate contributes at most
            // `alphabet_size` new ones, so the overshoot is bounded by that.
            assert!(e.at > 8 && e.at <= 8 + 2, "at = {}", e.at);
            assert!(e
                .to_string()
                .starts_with("EXPLODED-states: subset construction"));
        }
        other => panic!("expected a states breach, got {other:?}"),
    }
    // Exactly at the cap is NOT a breach: 8 metastates under a cap of 8 succeeds.
    let fa3 = kth_from_end_nfa(3);
    let ok = run(&instr, || subset_construction(&fa3, &initial)).unwrap();
    assert_eq!(ok.q, 8);
}

#[test]
fn a_state_cap_stops_the_cross_product() {
    // "even number of 1s" × "ends with 1": 4 reachable pairs.
    let even_ones = Fa::with_states(
        0,
        2,
        2,
        vec![1, 0],
        vec![row(&[(0, &[0]), (1, &[1])]), row(&[(0, &[1]), (1, &[0])])],
    );
    let ends_with_one = Fa::with_states(
        0,
        2,
        2,
        vec![0, 1],
        vec![row(&[(0, &[0]), (1, &[1])]), row(&[(0, &[0]), (1, &[1])])],
    );
    // Two independent single tracks: every (a_sym, b_sym) pair is a distinct product
    // symbol `a_sym * 2 + b_sym`.
    let all_inputs = [0, 1, 2, 3];
    let product = |logging: &mut Logging| {
        cross_product_internal(
            &even_ones,
            &ends_with_one,
            4,
            &all_inputs,
            |x, y| x & y,
            logging,
        )
    };
    let full = product(&mut Logging::new());
    assert_eq!(full.q, 4);

    let instr = Instrumentation::new().with_budget(ResourceBudget::states(2));
    match run(&instr, || product(&mut Logging::new())) {
        Err(BudgetError::Exhausted(e)) => {
            assert_eq!(e.operation, Operation::CrossProduct);
            assert_eq!(e.reason, ExhaustedReason::States);
            assert_eq!(e.limit, 2);
            assert!(e.at > 2 && e.at <= 4, "at = {}", e.at);
        }
        other => panic!("expected a cross-product breach, got {other:?}"),
    }
    // And the observer sees the pair count.
    let (instr, t) = observed();
    run(&instr, || product(&mut Logging::new())).unwrap();
    assert_eq!(
        t.borrow().events(),
        &[
            Event::CrossProductStarted {
                left_states: 2,
                right_states: 2
            },
            Event::CrossProductFinished { states: 4 }
        ]
    );
}

#[test]
fn a_state_cap_stops_minimization_at_entry() {
    let fa = kth_from_end_nfa(3);
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let dfa = subset_construction(&fa, &initial); // 8 states
    let instr = Instrumentation::new().with_budget(ResourceBudget::states(7));
    let err = run(&instr, || minimize(&dfa).unwrap()).expect_err("a breach");
    assert_eq!(
        err,
        BudgetError::Exhausted(Exhausted {
            reason: ExhaustedReason::States,
            operation: Operation::Minimize,
            at: 8,
            limit: 7,
        })
    );
}

#[test]
fn exhaustion_passes_through_the_inner_walnut_panic_boundary() {
    // `catch_walnut_panic` is the boundary wr-logic's eval loop draws around every
    // `act()`. It models Java's `catch (RuntimeException)`, which would NOT absorb an
    // `OutOfMemoryError` -- so a breached budget must come out of it as a re-raised
    // panic, not as an `Err(message)`.
    let instr = Instrumentation::new().with_budget(ResourceBudget::states(1));
    let outcome = run(&instr, || {
        let inner: Result<(), String> =
            catch_walnut_panic(|| Meter::current().check(Operation::Minimize, 5));
        inner
    });
    assert!(
        matches!(outcome, Err(BudgetError::Exhausted(_))),
        "the inner boundary must re-raise, got {outcome:?}"
    );
    // ... while an ordinary ported guard is still absorbed there, exactly as before.
    let absorbed = run(&instr, || catch_walnut_panic(|| panic!("a Walnut guard")));
    assert_eq!(absorbed, Ok(Err("a Walnut guard".to_string())));
}

#[test]
fn nothing_installed_means_nothing_observed_and_nothing_capped() {
    // The drop-in contract: outside any scope, a construction far past any budget a
    // test would set simply runs, and no observer exists to hear about it.
    let fa = kth_from_end_nfa(5);
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    assert!(!Meter::current().has_budget());
    assert_eq!(subset_construction(&fa, &initial).q, 32);
}

// ------------------------------------------------------------- item C: the seams

/// Σ* via a chain of accepting states: every reachable metastate is `{0} ∪ C`, so plain
/// SC builds 2^k of them while `SC_OTF` collapses every one to `{0}`.
fn sigma_star_via_chain(k: usize) -> Fa {
    let n = k + 1;
    let mut d = Vec::with_capacity(n);
    d.push(row(&[(0, &[0]), (1, &[0, 1])]));
    for i in 1..k {
        d.push(row(&[(0, &[i + 1]), (1, &[i + 1])]));
    }
    d.push(row(&[]));
    Fa::with_states(0, n, 2, vec![1; n], d)
}

#[test]
fn a_scope_default_strategy_selects_sc_otf_and_the_dispatcher_reports_it() {
    let fa = sigma_star_via_chain(5);
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let wrap = |fa: &Fa| {
        Automaton::new(
            fa.clone(),
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    };

    // Plain: 32 metastates.
    let mut plain = wrap(&fa);
    determinize(&mut plain, &initial, None, &mut Logging::new()).unwrap();
    assert_eq!(plain.fa.q, 32);

    // Scope default SC_OTF: one metastate, and the observer sees the strategy.
    let t = shared();
    let instr = Instrumentation::new()
        .with_default_strategy(Strategy::ScOtf)
        .with_observer(t.clone());
    let mut reduced = wrap(&fa);
    run(&instr, || {
        determinize(&mut reduced, &initial, None, &mut Logging::new()).unwrap();
    })
    .unwrap();
    assert_eq!(reduced.fa.q, 1);
    let events = t.borrow().events().to_vec();
    assert_eq!(
        events[0],
        Event::Determinize {
            strategy: Strategy::ScOtf,
            input_states: 6
        }
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::SimulationComputed { nfa_states: 6, .. })));

    // A policy that refuses the preorder degrades to plain SC, and says so.
    let t2 = shared();
    let instr = Instrumentation::new()
        .with_default_strategy(Strategy::ScOtf)
        .with_otf_policy(OtfPolicy { max_nfa_states: 2 })
        .with_observer(t2.clone());
    let mut degraded = wrap(&fa);
    run(&instr, || {
        determinize(&mut degraded, &initial, None, &mut Logging::new()).unwrap();
    })
    .unwrap();
    assert_eq!(degraded.fa.q, 32);
    assert!(t2.borrow().events().iter().any(|e| matches!(
        e,
        Event::SimulationSkipped {
            nfa_states: 6,
            limit: 2
        }
    )));
}

#[test]
fn a_scope_default_strategy_never_touches_a_dfao() {
    // A word automaton (outputs > 1): only SC may determinize it, so the scope default
    // is ignored rather than turned into `DfaoWithNonScStrategy`.
    let fa = Fa::with_states(
        0,
        2,
        2,
        vec![2, 3],
        vec![
            row(&[(0, &[0, 1]), (1, &[1])]),
            row(&[(0, &[1]), (1, &[0])]),
        ],
    );
    let mut a = Automaton::new(
        fa,
        vec![vec![0, 1]],
        vec!["x".to_string()],
        vec![Some(true)],
    );
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let t = shared();
    let instr = Instrumentation::new()
        .with_default_strategy(Strategy::ScOtf)
        .with_observer(t.clone());
    run(&instr, || {
        determinize(&mut a, &initial, None, &mut Logging::new()).unwrap();
    })
    .unwrap();
    assert!(matches!(
        t.borrow().events()[0],
        Event::Determinize {
            strategy: Strategy::Sc,
            ..
        }
    ));
}

/// A minimizer that delegates to Valmari but counts its calls and returns a
/// deliberately NON-minimal (but equivalent) automaton: the untouched input. That is a
/// legal `Minimizer` per the trait's contract, and it makes the seam observable.
struct Identity(RefCell<usize>);

impl Minimizer for Identity {
    fn minimize(&self, fa: &Fa) -> Result<Fa, MinimizeError> {
        *self.0.borrow_mut() += 1;
        if !fa.is_deterministic() {
            return Err(MinimizeError::NotDeterministic);
        }
        Ok(fa.clone())
    }
    fn name(&self) -> &str {
        "identity"
    }
}

#[test]
fn a_scoped_minimizer_replaces_valmari_on_the_construction_path_only() {
    let fa = kth_from_end_nfa(3);
    let initial: BTreeSet<usize> = [0].into_iter().collect();
    let dfa = subset_construction(&fa, &initial); // 8 states, already minimal
                                                  // A transient case where Valmari would shrink: the redundant "ends with 1".
    let transient = subset_construction(&ends_with_one_redundant_nfa(), &initial); // 3
    assert_eq!(minimize(&transient).unwrap().q, 2);

    let identity = Rc::new(Identity(RefCell::new(0)));
    let t = shared();
    let instr = Instrumentation::new()
        .with_minimizer(identity.clone())
        .with_observer(t.clone());
    let (via_logging, bare) = run(&instr, || {
        (
            wr_core::minimize::minimize_with_logging(&transient, &mut Logging::new()).unwrap(),
            minimize(&transient).unwrap(),
        )
    })
    .unwrap();
    assert_eq!(
        via_logging.q, 3,
        "the construction path used the custom minimizer"
    );
    assert_eq!(bare.q, 2, "the bare Valmari reference is never redirected");
    assert_eq!(*identity.0.borrow(), 1);
    // The custom path still reports the same events a Valmari run would.
    assert!(t.borrow().events().contains(&Event::MinimizeFinished {
        before: 3,
        after: 3
    }));
    // And the seam is on the real eval-path entry point too.
    let mut a = Automaton::new(
        dfa.clone(),
        vec![vec![0, 1]],
        vec!["x".to_string()],
        vec![Some(true)],
    );
    run(&instr, || a.determinize_and_minimize()).unwrap();
    assert_eq!(*identity.0.borrow(), 2);
    assert_eq!(a.fa.q, 8);
}
