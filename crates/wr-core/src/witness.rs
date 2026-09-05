// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! Witness / counterexample extraction from a decided automaton — the blessed helpers a
//! consumer would otherwise re-derive from the public [`Fa`] (ct-research's "Line 1",
//! witness half).
//!
//! **No Java counterpart** except where noted: Walnut's own `test` command
//! ([`crate::search::shortest_accepted_word`]) enumerates accepted inputs but, being a
//! port, carries Walnut's quirks — it never returns the empty word, it errors on the
//! TRUE automaton, and it speaks in encoded symbols. The helpers here are what a
//! research consumer actually asks: *the* shortest word with a given property, the empty
//! word included, decoded per track, with a partial DFA's missing transitions treated
//! as rejection.
//!
//! # Semantics
//!
//! All searches are breadth-first over the automaton's states with symbols tried in
//! ascending order, so the returned word is **shortest**, and among the shortest the
//! **lexicographically smallest in symbol order** (for a one-track base-`k` automaton
//! that is numeric order of the digit string; for a multi-track automaton, the order of
//! the encoded symbols — see [`Witness::tracks`] for the per-track view).
//!
//! * [`shortest_accepted`] — the shortest word reaching an accepting state (`o != 0`);
//!   for an `eval`/`def` result with free variables, that is the shortest satisfying
//!   assignment.
//! * [`shortest_rejected`] — the shortest word reaching a non-accepting state **or a
//!   missing transition** (a partial DFA rejects there); for a claim the engine decided
//!   FALSE, evaluate its negation's automaton and use `shortest_accepted`, or use this on
//!   the claim's own automaton to get the shortest counterexample directly.
//! * [`shortest_word_where`] — the general form: a predicate on `(state, output)`.
//! * [`shortest_output`] — for a word automaton (DFAO): the shortest input whose output
//!   is a given value.
//!
//! # Leading/trailing zeros
//!
//! Walnut's automata accept representations with padding zeros (leading for msd,
//! trailing for lsd) — the engine's own zero-fixups guarantee that. The shortest word is
//! therefore never padded except when the empty word itself is the witness (the value
//! 0 on every track); [`Witness::track_value`] decodes a track's digits as a base-`k`
//! number either way.

use std::collections::VecDeque;

use crate::automaton::Automaton;
use crate::fa::Fa;

/// A word found by one of this module's searches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Witness {
    /// The word as encoded symbols (the [`Fa`]'s alphabet), shortest first.
    pub symbols: Vec<i32>,
    /// The state the word leads to, and that state's output.
    pub state: usize,
    pub output: i32,
}

impl Witness {
    /// The word split per track: `tracks[t][i]` is track `t`'s digit at position `i`,
    /// decoded through the automaton's alphabet (`Automaton::try_decode`). `None` if a
    /// symbol does not decode (an automaton whose alphabet does not match its `fa`).
    pub fn tracks(&self, a: &Automaton) -> Option<Vec<Vec<i32>>> {
        let n = a.track_count();
        let mut tracks = vec![Vec::with_capacity(self.symbols.len()); n];
        for &sym in &self.symbols {
            let digits = a.try_decode(sym).ok()?;
            if digits.len() != n {
                return None;
            }
            for (t, d) in digits.into_iter().enumerate() {
                tracks[t].push(d);
            }
        }
        Some(tracks)
    }

    /// Track `t`'s digits read as a base-`base` number in the track's own direction
    /// (msd: first digit most significant; lsd: first digit least significant), as a
    /// `u128`. `None` if the track does not decode, has no direction, holds a digit
    /// outside `0..base`, or overflows. For custom bases (Fibonacci, …) use
    /// [`Witness::tracks`] and evaluate the representation yourself.
    pub fn track_value(&self, a: &Automaton, t: usize, base: u32) -> Option<u128> {
        let tracks = self.tracks(a)?;
        let digits = tracks.get(t)?;
        let msd = a.track_msd(t)?;
        let mut value: u128 = 0;
        let ordered: Box<dyn Iterator<Item = &i32>> = if msd {
            Box::new(digits.iter())
        } else {
            Box::new(digits.iter().rev())
        };
        for &d in ordered {
            if d < 0 || d as u32 >= base {
                return None;
            }
            value = value
                .checked_mul(u128::from(base))?
                .checked_add(d as u128)?;
        }
        Some(value)
    }
}

/// Where a BFS step ended up.
enum Step {
    To(usize),
    Missing,
}

fn step(fa: &Fa, state: usize, symbol: i32) -> Result<Step, WitnessError> {
    match fa.d[state].get(&symbol) {
        None => Ok(Step::Missing),
        Some(dests) if dests.is_empty() => Ok(Step::Missing),
        Some(dests) if dests.len() == 1 => {
            let dest = dests[0];
            if dest >= fa.q {
                return Err(WitnessError::Malformed("destination out of range"));
            }
            Ok(Step::To(dest))
        }
        Some(_) => Err(WitnessError::NotDeterministic { state, symbol }),
    }
}

/// Why a search could not run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WitnessError {
    /// The TRUE/FALSE automaton has no alphabet to search over. (TRUE accepts the empty
    /// word and everything else; FALSE nothing — the caller already knows the answer.)
    TrueFalseAutomaton,
    /// Two destinations on one symbol: determinize first.
    NotDeterministic { state: usize, symbol: i32 },
    /// A structurally invalid `Fa`.
    Malformed(&'static str),
}

impl std::fmt::Display for WitnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WitnessError::TrueFalseAutomaton => {
                write!(f, "the TRUE/FALSE automaton has no alphabet to search")
            }
            WitnessError::NotDeterministic { state, symbol } => write!(
                f,
                "state {state} has several destinations on symbol {symbol}; determinize first"
            ),
            WitnessError::Malformed(what) => write!(f, "malformed automaton: {what}"),
        }
    }
}

impl std::error::Error for WitnessError {}

/// The shortest word (empty word included) leading to a state satisfying
/// `accept(state, output)`, or `Ok(None)` if no reachable state does. Missing
/// transitions are simply not followed.
pub fn shortest_word_where(
    fa: &Fa,
    accept: impl Fn(usize, i32) -> bool,
) -> Result<Option<Witness>, WitnessError> {
    if fa.is_true_false_automaton() {
        return Err(WitnessError::TrueFalseAutomaton);
    }
    if fa.q == 0 {
        return Ok(None);
    }
    if fa.q0 >= fa.q || fa.o.len() != fa.q || fa.d.len() != fa.q {
        return Err(WitnessError::Malformed("q0/o/d inconsistent with q"));
    }
    // parent[s] = (previous state, symbol) on the BFS tree; q0's is None.
    let mut parent: Vec<Option<(usize, i32)>> = vec![None; fa.q];
    let mut seen = vec![false; fa.q];
    let mut queue = VecDeque::new();
    seen[fa.q0] = true;
    queue.push_back(fa.q0);
    while let Some(s) = queue.pop_front() {
        if accept(s, fa.o[s]) {
            return Ok(Some(Witness {
                symbols: path_to(&parent, s),
                state: s,
                output: fa.o[s],
            }));
        }
        for symbol in 0..fa.alphabet_size as i32 {
            if let Step::To(t) = step(fa, s, symbol)? {
                if !seen[t] {
                    seen[t] = true;
                    parent[t] = Some((s, symbol));
                    queue.push_back(t);
                }
            }
        }
    }
    Ok(None)
}

fn path_to(parent: &[Option<(usize, i32)>], mut s: usize) -> Vec<i32> {
    let mut rev = Vec::new();
    while let Some((p, sym)) = parent[s] {
        rev.push(sym);
        s = p;
    }
    rev.reverse();
    rev
}

/// The shortest accepted word (`o != 0`), the empty word included.
pub fn shortest_accepted(fa: &Fa) -> Result<Option<Witness>, WitnessError> {
    shortest_word_where(fa, |_, o| o != 0)
}

/// The shortest word with output exactly `value` — for a DFAO, the first input at which
/// the automatic sequence takes that value.
pub fn shortest_output(fa: &Fa, value: i32) -> Result<Option<Witness>, WitnessError> {
    shortest_word_where(fa, |_, o| o == value)
}

/// The shortest rejected word: one leading to a non-accepting state (`o == 0`) **or**
/// into a missing transition (a partial DFA rejects everything past it). For the latter
/// the returned `state` is the last state on the path and `output` is that state's
/// output — the word itself is what matters.
pub fn shortest_rejected(fa: &Fa) -> Result<Option<Witness>, WitnessError> {
    if fa.is_true_false_automaton() {
        return Err(WitnessError::TrueFalseAutomaton);
    }
    if fa.q == 0 {
        return Ok(None);
    }
    if fa.q0 >= fa.q || fa.o.len() != fa.q || fa.d.len() != fa.q {
        return Err(WitnessError::Malformed("q0/o/d inconsistent with q"));
    }
    let mut parent: Vec<Option<(usize, i32)>> = vec![None; fa.q];
    let mut seen = vec![false; fa.q];
    let mut queue = VecDeque::new();
    seen[fa.q0] = true;
    queue.push_back(fa.q0);
    while let Some(s) = queue.pop_front() {
        if fa.o[s] == 0 {
            return Ok(Some(Witness {
                symbols: path_to(&parent, s),
                state: s,
                output: fa.o[s],
            }));
        }
        for symbol in 0..fa.alphabet_size as i32 {
            match step(fa, s, symbol)? {
                Step::Missing => {
                    let mut symbols = path_to(&parent, s);
                    symbols.push(symbol);
                    return Ok(Some(Witness {
                        symbols,
                        state: s,
                        output: fa.o[s],
                    }));
                }
                Step::To(t) => {
                    if !seen[t] {
                        seen[t] = true;
                        parent[t] = Some((s, symbol));
                        queue.push_back(t);
                    }
                }
            }
        }
    }
    Ok(None)
}

/// [`shortest_accepted`] on an [`Automaton`] (its `fa`), for the common
/// `eval`/`def`-result case.
pub fn shortest_accepted_automaton(a: &Automaton) -> Result<Option<Witness>, WitnessError> {
    shortest_accepted(&a.fa)
}

/// [`shortest_rejected`] on an [`Automaton`] (its `fa`).
pub fn shortest_rejected_automaton(a: &Automaton) -> Result<Option<Witness>, WitnessError> {
    shortest_rejected(&a.fa)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn row(pairs: &[(i32, usize)]) -> BTreeMap<i32, Vec<usize>> {
        pairs.iter().map(|&(s, d)| (s, vec![d])).collect()
    }

    /// msd_2 "x is a multiple of 3", total, over symbols {0,1}: states = x mod 3.
    fn multiples_of_three_msd() -> Fa {
        Fa::with_states(
            0,
            3,
            2,
            vec![1, 0, 0],
            vec![
                row(&[(0, 0), (1, 1)]),
                row(&[(0, 2), (1, 0)]),
                row(&[(0, 1), (1, 2)]),
            ],
        )
    }

    fn one_track_msd(fa: Fa) -> Automaton {
        Automaton::new(fa, vec![vec![0, 1]], vec!["x".into()], vec![Some(true)])
    }

    #[test]
    fn the_empty_word_is_a_witness_when_q0_accepts() {
        let fa = multiples_of_three_msd();
        let w = shortest_accepted(&fa).unwrap().unwrap();
        assert_eq!(w.symbols, Vec::<i32>::new());
        assert_eq!(w.state, 0);
        assert_eq!(w.output, 1);
        let a = one_track_msd(fa);
        assert_eq!(w.track_value(&a, 0, 2), Some(0));
    }

    #[test]
    fn shortest_rejected_finds_the_smallest_non_multiple() {
        let fa = multiples_of_three_msd();
        let w = shortest_rejected(&fa).unwrap().unwrap();
        // "1" reaches state 1 (x = 1, not a multiple of 3).
        assert_eq!(w.symbols, vec![1]);
        let a = one_track_msd(fa);
        assert_eq!(w.track_value(&a, 0, 2), Some(1));
    }

    #[test]
    fn shortest_output_and_lexicographic_tie_break() {
        let fa = multiples_of_three_msd();
        // State 2 (x ≡ 2): shortest is "10" (2); "01"... no — leading zero keeps state
        // 0 then "1" -> state 1; "10" is the first length-2 word reaching state 2.
        let w = shortest_word_where(&fa, |s, _| s == 2).unwrap().unwrap();
        assert_eq!(w.symbols, vec![1, 0]);
        let a = one_track_msd(fa.clone());
        assert_eq!(w.track_value(&a, 0, 2), Some(2));
        assert_eq!(
            shortest_output(&fa, 1).unwrap().unwrap().symbols,
            Vec::<i32>::new()
        );
        assert_eq!(shortest_output(&fa, 7).unwrap(), None);
    }

    #[test]
    fn a_missing_transition_is_a_rejection() {
        // Accepts exactly {ε, 0}: state 0 accepting, on 0 -> 1 (accepting, no
        // transitions); nothing on 1.
        let fa = Fa::with_states(0, 2, 2, vec![1, 1], vec![row(&[(0, 1)]), row(&[])]);
        let w = shortest_rejected(&fa).unwrap().unwrap();
        assert_eq!(w.symbols, vec![1]);
        assert_eq!(w.state, 0);
        // And an all-accepting total automaton rejects nothing.
        let total = Fa::with_states(0, 1, 2, vec![1], vec![row(&[(0, 0), (1, 0)])]);
        assert_eq!(shortest_rejected(&total).unwrap(), None);
    }

    #[test]
    fn lsd_track_values_read_the_word_reversed() {
        // lsd_2: the word "1 1 0" (digits least-significant first) is 3.
        let fa = Fa::with_states(
            0,
            4,
            2,
            vec![0, 0, 0, 1],
            vec![row(&[(1, 1)]), row(&[(1, 2)]), row(&[(0, 3)]), row(&[])],
        );
        let a = Automaton::new(
            fa.clone(),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![Some(false)],
        );
        let w = shortest_accepted(&fa).unwrap().unwrap();
        assert_eq!(w.symbols, vec![1, 1, 0]);
        assert_eq!(w.track_value(&a, 0, 2), Some(3));
        assert_eq!(w.tracks(&a), Some(vec![vec![1, 1, 0]]));
    }

    #[test]
    fn multi_track_words_decode_per_track() {
        // Two msd_2 tracks over {0,1}x{0,1}. `Automaton::encode` makes the FIRST track
        // the least significant digit of the symbol: symbol = x + 2*y. Accept after
        // reading symbol 2 = (x=0, y=1) then symbol 1 = (x=1, y=0): x = 01b = 1,
        // y = 10b = 2.
        let fa = Fa::with_states(
            0,
            3,
            4,
            vec![0, 0, 1],
            vec![row(&[(2, 1)]), row(&[(1, 2)]), row(&[])],
        );
        let a = Automaton::new(
            fa.clone(),
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".into(), "y".into()],
            vec![Some(true), Some(true)],
        );
        let w = shortest_accepted_automaton(&a).unwrap().unwrap();
        assert_eq!(w.symbols, vec![2, 1]);
        assert_eq!(w.tracks(&a), Some(vec![vec![0, 1], vec![1, 0]]));
        assert_eq!(w.track_value(&a, 0, 2), Some(1));
        assert_eq!(w.track_value(&a, 1, 2), Some(2));
        assert_eq!(w.track_value(&a, 2, 2), None);
        assert!(shortest_rejected_automaton(&a).unwrap().is_some());
    }

    #[test]
    fn errors_are_explicit() {
        assert_eq!(
            shortest_accepted(&Fa::trivial(true)),
            Err(WitnessError::TrueFalseAutomaton)
        );
        assert_eq!(
            shortest_rejected(&Fa::trivial(false)),
            Err(WitnessError::TrueFalseAutomaton)
        );
        let nfa = Fa::with_states(
            0,
            2,
            1,
            vec![0, 1],
            vec![[(0, vec![0, 1])].into_iter().collect(), row(&[])],
        );
        assert_eq!(
            shortest_accepted(&nfa),
            Err(WitnessError::NotDeterministic {
                state: 0,
                symbol: 0
            })
        );
        let empty = Fa::with_states(0, 0, 2, vec![], vec![]);
        assert_eq!(shortest_accepted(&empty), Ok(None));
        assert_eq!(shortest_rejected(&empty), Ok(None));
    }
}
