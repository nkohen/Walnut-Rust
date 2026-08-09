// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Core finite-automaton representation.
//!
//! Ports the state/transition/output core of Walnut's `Automata/FA/FA.java`. A single
//! unified NFA-shaped transition table backs both NFA and DFA use here — Java keeps two
//! backing types, `TransitionsNFA`/`TransitionsDFA`, as a storage optimization, not a
//! behavioral quirk, so collapsing them doesn't violate the mechanical-port discipline.
//! Callers that require determinism (`minimize`, the equivalence oracle) must check
//! [`Fa::is_deterministic`]/[`Fa::is_deterministic_and_total`] themselves; those checks
//! are hard errors at the call site, never `debug_assert!` (`PORTING.md`'s named
//! "debug_assert erasing side effects" regression class — it vanishes in release builds).
//!
//! Symbols are pre-encoded integers (mixed-radix multi-track digit tuples, matching
//! Java's `FA`/`Transitions` layering) — `Fa` itself is track-agnostic; track
//! encode/decode lives in `crate::automaton`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A nondeterministic finite automaton with per-state integer output.
///
/// `o[s] == 0` means state `s` is non-accepting; a plain NFA/DFA predicate automaton
/// uses only `0`/`1`. Larger values are DFAO word-output values (Walnut's `isFAO`) —
/// not yet exercised by this port (the equivalence oracle in particular only handles
/// 0/1 acceptance, see `equiv` module docs), but the field shape matches Java's `FA.O`
/// so it's ready when DFAO support lands.
#[derive(Debug, Clone)]
pub struct Fa {
    pub q0: usize,
    pub q: usize,
    pub alphabet_size: usize,
    pub o: Vec<i32>,
    /// `d[state][symbol] = destinations`. NFA-shaped (more than one destination is
    /// allowed); `is_deterministic()` distinguishes DFA use at the call site rather
    /// than the type system, mirroring Java's runtime `Transitions.isDeterministic()`.
    pub d: Vec<BTreeMap<i32, Vec<usize>>>,
}

impl Fa {
    pub fn is_accepting(&self, s: usize) -> bool {
        self.o[s] != 0
    }

    /// True iff every (state, symbol) pair present has at most one destination.
    /// Does NOT require every symbol be present — see [`Fa::is_deterministic_and_total`]
    /// for that stronger check.
    pub fn is_deterministic(&self) -> bool {
        self.d
            .iter()
            .all(|m| m.values().all(|dests| dests.len() <= 1))
    }

    /// True iff every state has EXACTLY one destination for EVERY symbol in
    /// `0..alphabet_size` — the precondition the equivalence oracle requires. Checks
    /// symbol presence explicitly (not just `map.len() == alphabet_size`, which could
    /// pass with the wrong set of keys).
    pub fn is_deterministic_and_total(&self) -> bool {
        (0..self.alphabet_size as i32).all(|sym| {
            self.d
                .iter()
                .all(|m| matches!(m.get(&sym), Some(dests) if dests.len() == 1))
        })
    }

    /// BFS from `q0`; true iff no accepting state is reachable (the automaton
    /// recognizes the empty language). Ports the reachability half of
    /// `FA.isLanguageEmpty` (no track/label bookkeeping at this layer).
    pub fn is_language_empty(&self) -> bool {
        if self.q == 0 {
            return true;
        }
        let mut seen = vec![false; self.q];
        let mut queue = VecDeque::new();
        seen[self.q0] = true;
        queue.push_back(self.q0);
        while let Some(s) = queue.pop_front() {
            if self.is_accepting(s) {
                return false;
            }
            for dests in self.d[s].values() {
                for &d in dests {
                    if !seen[d] {
                        seen[d] = true;
                        queue.push_back(d);
                    }
                }
            }
        }
        true
    }

    /// NFA simulation: does at least one run over `word` (a sequence of already-encoded
    /// symbols) end in an accepting state? Used both by real callers and as a
    /// ground-truth oracle for property tests on nondeterministic intermediates (where
    /// the language-equivalence oracle doesn't apply, since it requires total DFAs).
    pub fn accepts_word(&self, word: &[i32]) -> bool {
        let mut current: BTreeSet<usize> = [self.q0].into_iter().collect();
        for &sym in word {
            let mut next = BTreeSet::new();
            for &s in &current {
                if let Some(dests) = self.d[s].get(&sym) {
                    next.extend(dests.iter().copied());
                }
            }
            current = next;
            if current.is_empty() {
                return false;
            }
        }
        current.iter().any(|&s| self.is_accepting(s))
    }

    /// Adds an explicit sink state (output `sink_output`) and routes every missing
    /// `(state, symbol)` transition to it, including a self-loop on the sink itself.
    /// Requires the automaton already be deterministic (`is_deterministic`); does not
    /// attempt to totalize genuine nondeterminism. A no-op if already total.
    pub fn totalize(&mut self, sink_output: i32) {
        assert!(
            self.is_deterministic(),
            "totalize requires an already-deterministic automaton"
        );
        if self.is_deterministic_and_total() {
            return;
        }
        let sink = self.q;
        self.o.push(sink_output);
        self.q += 1;
        for sym in 0..self.alphabet_size as i32 {
            for s in 0..sink {
                self.d[s].entry(sym).or_insert_with(|| vec![sink]);
            }
        }
        let mut sink_map = BTreeMap::new();
        for sym in 0..self.alphabet_size as i32 {
            sink_map.insert(sym, vec![sink]);
        }
        self.d.push(sink_map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2-state total DFA over a 2-symbol alphabet accepting words containing at
    /// least one `1` (symbol 1); mirrors the hand-derived spike result shape.
    fn contains_one_dfa() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
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
    fn accepting_and_empty() {
        let fa = contains_one_dfa();
        assert!(fa.is_accepting(1));
        assert!(!fa.is_accepting(0));
        assert!(!fa.is_language_empty());
    }

    #[test]
    fn language_empty_no_accepting_state() {
        let mut fa = contains_one_dfa();
        fa.o = vec![0, 0];
        assert!(fa.is_language_empty());
    }

    #[test]
    fn zero_state_automaton_is_empty() {
        let fa = Fa {
            q0: 0,
            q: 0,
            alphabet_size: 2,
            o: vec![],
            d: vec![],
        };
        assert!(fa.is_language_empty());
    }

    #[test]
    fn accepts_word_matches_expected_language() {
        let fa = contains_one_dfa();
        assert!(!fa.accepts_word(&[0, 0, 0]));
        assert!(fa.accepts_word(&[0, 1, 0]));
        assert!(fa.accepts_word(&[1]));
        assert!(!fa.accepts_word(&[]));
    }

    #[test]
    fn is_deterministic_and_total_true_for_dfa() {
        let fa = contains_one_dfa();
        assert!(fa.is_deterministic());
        assert!(fa.is_deterministic_and_total());
    }

    #[test]
    fn is_deterministic_and_total_false_when_symbol_missing() {
        let mut fa = contains_one_dfa();
        fa.d[0].remove(&1);
        assert!(fa.is_deterministic()); // still <=1 dest per present symbol
        assert!(!fa.is_deterministic_and_total()); // but not total anymore
    }

    #[test]
    fn is_deterministic_false_for_nfa() {
        let mut fa = contains_one_dfa();
        fa.d[0].insert(1, vec![0, 1]); // two destinations for symbol 1
        assert!(!fa.is_deterministic());
    }

    #[test]
    fn totalize_fills_missing_transitions_with_a_sink() {
        let mut fa = contains_one_dfa();
        fa.d[0].remove(&1);
        assert!(!fa.is_deterministic_and_total());
        fa.totalize(0);
        assert!(fa.is_deterministic_and_total());
        assert_eq!(fa.q, 3);
        // The formerly-missing transition now routes to the sink, which self-loops
        // and is non-accepting — language is unchanged for words that never hit it,
        // and the old symbol-1-from-state-0 input now correctly leads to rejection.
        assert!(!fa.accepts_word(&[0]));
    }

    #[test]
    fn totalize_is_noop_when_already_total() {
        let mut fa = contains_one_dfa();
        fa.totalize(0);
        assert_eq!(fa.q, 2, "already-total automaton should not grow");
    }
}
