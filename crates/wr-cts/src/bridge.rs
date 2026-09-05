// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! The `wr-core` ↔ `RustConstantTermSequences` bridge: converters between
//! [`wr_core::fa::Fa`] (a single-track DFAO: outputs in `o`, digits as symbols) and the
//! substrate's [`DFAO<ModInt, S>`] (states of any hashable type `S`, transitions keyed by
//! `(S, ModInt)`, outputs read off a state by a caller-supplied function).
//!
//! Why this exists (ct-research's "Line 2"): a constant-term sequence's automaton is
//! computed on the substrate from the p-kernel / a linear representation, already
//! minimal, and ought to enter the engine **as an object** — not by serializing it to
//! Walnut `.txt` and parsing it back — and, in the other direction, the engine's automata
//! ought to be usable as seeds/inputs for the substrate's linear-representation
//! machinery. Both directions are exact: a round trip through either pair of functions
//! reproduces the original automaton's transition function and outputs, up to the state
//! renumbering documented on each function.
//!
//! # Conventions bridged
//!
//! | | `wr_core::fa::Fa` | substrate `DFAO<ModInt, S>` |
//! | --- | --- | --- |
//! | initial state | `q0` (any index) | `states[0]` |
//! | alphabet | symbols `0..alphabet_size` (a one-track base-`p` automaton's digit `i` encodes to symbol `i`) | `ModInt::new(i, p)` for `i` in `0..p` |
//! | output | `o[q]` (`i32`) | `output_of(&states[q])` (the caller's projection — e.g. a `LaurentPoly` state's constant term) |
//! | totality | partial allowed (a missing transition rejects) | total by construction (`evaluate` unwraps) |
//! | direction | a property of the enclosing [`Automaton`]'s track (`msd_p` / `lsd_p`) | a property of how the caller feeds digits (`compute_msd` / `compute_lsd`) |
//!
//! The substrate reads a DFAO either least-significant-digit-first (`compute_lsd`, what
//! `poly_auto`/`lin_rep_machine` are built for) or most-significant-first
//! (`lin_rep_reverse_machine`). The transition function is the same object either way;
//! only the enclosing Walnut number system differs, so the [`Automaton`]-level
//! converters take an explicit [`Direction`] and refuse to guess.
//!
//! # Scope
//!
//! Single-track only, matching the substrate's `DFAO`. A multi-track engine automaton
//! (a predicate over several variables) has no substrate counterpart and is refused
//! ([`BridgeError::NotSingleTrack`]). The substrate's RZ-minimization / minimal-dual
//! machinery lives in ct-research, not in the substrate crate; this module gives it the
//! engine's automata as `DFAO<ModInt, usize>` values and takes its results back — it
//! does not reimplement any of it.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::hash::Hash;

use rust_constant_term_sequences::dfao::DFAO;
use rust_constant_term_sequences::laurent_poly::LaurentPoly;
use rust_constant_term_sequences::mod_int::ModInt;
use rust_constant_term_sequences::mod_int_vector::ModIntVector;
use wr_core::automaton::Automaton;
use wr_core::fa::Fa;

/// Which end of a number's base-`p` expansion the automaton reads first — the Walnut
/// number system the bridged automaton is declared over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Most significant digit first: Walnut's `msd_p`.
    Msd,
    /// Least significant digit first: Walnut's `lsd_p` — the substrate's
    /// `compute_lsd` / `poly_auto` / `lin_rep_machine` convention.
    Lsd,
}

impl Direction {
    /// The Walnut number-system name for base `p`.
    pub fn ns_name(self, p: u64) -> String {
        match self {
            Direction::Msd => format!("msd_{p}"),
            Direction::Lsd => format!("lsd_{p}"),
        }
    }

    fn is_msd(self) -> bool {
        matches!(self, Direction::Msd)
    }
}

/// Everything a conversion can refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BridgeError {
    /// The substrate DFAO has no states (so no initial state).
    NoStates,
    /// The same state value appears twice in `states`; the transition map could not
    /// distinguish the two, so neither can this bridge.
    DuplicateState { index: usize },
    /// A transition key carries a different modulus than the one requested.
    ModulusMismatch { expected: u64, found: u64 },
    /// `(state, digit)` has no transition. Substrate DFAOs are total by construction;
    /// engine automata may be partial, in which case pass one through
    /// [`totalize_dead`] first (or let the substrate never read that input).
    MissingTransition { state: usize, symbol: i32 },
    /// A transition's destination is not one of `states`.
    UnknownDestination { state: usize, symbol: i32 },
    /// An engine automaton with a nondeterministic choice (two destinations for one
    /// symbol) — determinize it first.
    NotDeterministic { state: usize, symbol: i32 },
    /// The engine automaton's alphabet size is not the modulus asked for.
    AlphabetMismatch { expected: usize, found: usize },
    /// The engine automaton is the trivial TRUE/FALSE automaton, which has no alphabet
    /// or states to bridge.
    TrueFalseAutomaton,
    /// The engine automaton has `tracks` tracks; the substrate's DFAO is single-track.
    NotSingleTrack { tracks: usize },
    /// The engine automaton's one track is not exactly the digits `0..p` in order.
    NotBaseKAlphabet { alphabet: Vec<i32> },
    /// The engine automaton's track has no msd/lsd direction (`None`).
    NoDirection,
    /// A destination id is outside `0..q` (a malformed `Fa`).
    DestinationOutOfRange {
        state: usize,
        symbol: i32,
        dest: usize,
    },
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::NoStates => write!(f, "the DFAO has no states"),
            BridgeError::DuplicateState { index } => {
                write!(f, "DFAO state #{index} duplicates an earlier state")
            }
            BridgeError::ModulusMismatch { expected, found } => write!(
                f,
                "a transition digit has modulus {found}, expected {expected}"
            ),
            BridgeError::MissingTransition { state, symbol } => {
                write!(f, "no transition from state {state} on digit {symbol}")
            }
            BridgeError::UnknownDestination { state, symbol } => write!(
                f,
                "the transition from state {state} on digit {symbol} leads to a state not in `states`"
            ),
            BridgeError::NotDeterministic { state, symbol } => write!(
                f,
                "state {state} has more than one destination on symbol {symbol}; determinize first"
            ),
            BridgeError::AlphabetMismatch { expected, found } => write!(
                f,
                "alphabet size {found} does not match the modulus {expected}"
            ),
            BridgeError::TrueFalseAutomaton => {
                write!(f, "the TRUE/FALSE automaton has no alphabet to bridge")
            }
            BridgeError::NotSingleTrack { tracks } => write!(
                f,
                "the automaton has {tracks} tracks; the substrate DFAO is single-track"
            ),
            BridgeError::NotBaseKAlphabet { alphabet } => write!(
                f,
                "the track alphabet {alphabet:?} is not the digits 0..p in order"
            ),
            BridgeError::NoDirection => {
                write!(f, "the track has no msd/lsd direction; pass one explicitly")
            }
            BridgeError::DestinationOutOfRange {
                state,
                symbol,
                dest,
            } => write!(
                f,
                "destination {dest} of state {state} on symbol {symbol} is out of range"
            ),
        }
    }
}

impl std::error::Error for BridgeError {}

// ------------------------------------------------------------------ DFAO -> Fa

/// Convert a substrate DFAO over base `modulus` into an [`Fa`], reading each state's
/// output through `output_of`.
///
/// State `i` of the result is `dfao.states[i]`, so `q0 = 0` and the numbering is the
/// substrate's own. Every `(state, digit)` for `digit` in `0..modulus` must have a
/// transition (substrate DFAOs are built total); the result is therefore a total DFA.
pub fn fa_from_dfao<S, F>(
    dfao: &DFAO<ModInt, S>,
    modulus: u64,
    output_of: F,
) -> Result<Fa, BridgeError>
where
    S: Clone + Eq + Hash,
    F: Fn(&S) -> i32,
{
    if dfao.states.is_empty() {
        return Err(BridgeError::NoStates);
    }
    let mut index: HashMap<&S, usize> = HashMap::with_capacity(dfao.states.len());
    for (i, s) in dfao.states.iter().enumerate() {
        if index.insert(s, i).is_some() {
            return Err(BridgeError::DuplicateState { index: i });
        }
    }
    for (_, digit) in dfao.transitions.keys() {
        if digit.modulus != modulus {
            return Err(BridgeError::ModulusMismatch {
                expected: modulus,
                found: digit.modulus,
            });
        }
    }
    let n = dfao.states.len();
    let alphabet_size = usize::try_from(modulus).expect("modulus fits usize");
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(n);
    for (i, s) in dfao.states.iter().enumerate() {
        let mut row = BTreeMap::new();
        for a in 0..modulus {
            let symbol = i32::try_from(a).expect("digit fits i32");
            let key = (s.clone(), ModInt::new(a, modulus));
            let dest = dfao
                .transitions
                .get(&key)
                .ok_or(BridgeError::MissingTransition { state: i, symbol })?;
            let j = *index
                .get(dest)
                .ok_or(BridgeError::UnknownDestination { state: i, symbol })?;
            row.insert(symbol, vec![j]);
        }
        d.push(row);
    }
    let o = dfao.states.iter().map(output_of).collect();
    Ok(Fa::with_states(0, n, alphabet_size, o, d))
}

/// [`fa_from_dfao`] wrapped as a one-track Walnut word automaton over `msd_p`/`lsd_p`
/// (per `direction`), in the shape `wr_io`'s reader produces for a `Word Automata
/// Library/` file: one track with alphabet `0..p`, label `"0"`, the number-system name
/// set, no valid-representation restriction (base-`p` needs none).
///
/// The result is what `wr_cli::embed::Engine::register_word_automaton` takes — the
/// "load an already-minimal ct-DFAO directly into the engine" path.
pub fn automaton_from_dfao<S, F>(
    dfao: &DFAO<ModInt, S>,
    modulus: u64,
    direction: Direction,
    output_of: F,
) -> Result<Automaton, BridgeError>
where
    S: Clone + Eq + Hash,
    F: Fn(&S) -> i32,
{
    let fa = fa_from_dfao(dfao, modulus, output_of)?;
    let alphabet: Vec<i32> = (0..modulus).map(|a| a as i32).collect();
    let mut a = Automaton::new(
        fa,
        vec![alphabet],
        vec!["0".to_string()],
        vec![Some(direction.is_msd())],
    );
    a.set_track_ns_name(0, Some(direction.ns_name(modulus)));
    Ok(a)
}

/// The constant-term output of a polynomial-state DFAO (`DFAO::poly_auto`): the
/// constant term of the state's Laurent polynomial, as a residue.
pub fn poly_constant_term(state: &LaurentPoly) -> i32 {
    state.constant_term().value as i32
}

/// The constant-term output of a linear-representation DFAO (`DFAO::lin_rep_machine`):
/// the state vector's constant-term coordinate, as a residue.
pub fn vector_constant_term(state: &ModIntVector) -> i32 {
    state.constant_term().value as i32
}

/// [`automaton_from_dfao`] for `DFAO::poly_auto`'s output, with the modulus read off the
/// states themselves.
pub fn automaton_from_poly_dfao(
    dfao: &DFAO<ModInt, LaurentPoly>,
    direction: Direction,
) -> Result<Automaton, BridgeError> {
    let modulus = dfao.states.first().ok_or(BridgeError::NoStates)?.modulus;
    automaton_from_dfao(dfao, modulus, direction, poly_constant_term)
}

/// [`automaton_from_dfao`] for `DFAO::lin_rep_machine` / `lin_rep_reverse_machine`'s
/// output, with the modulus read off the states themselves.
pub fn automaton_from_lin_rep_dfao(
    dfao: &DFAO<ModInt, ModIntVector>,
    direction: Direction,
) -> Result<Automaton, BridgeError> {
    let modulus = dfao.states.first().ok_or(BridgeError::NoStates)?.modulus;
    automaton_from_dfao(dfao, modulus, direction, vector_constant_term)
}

// ------------------------------------------------------------------ Fa -> DFAO

/// Convert a total, deterministic single-track [`Fa`] over base `modulus` into a
/// substrate `DFAO<ModInt, usize>` whose state values are the `Fa`'s own state ids.
///
/// `states[0]` must be the initial state, so the returned `states` vector lists
/// `fa.q0` first and the remaining ids in increasing order; the state *values* are the
/// original ids, so `dfao.transitions[(q, d)]` is exactly `fa.d[q][d][0]`. Outputs are
/// not part of the substrate type — read them from `fa.o[state]` (the state value IS the
/// index into `o`).
pub fn dfao_from_fa(fa: &Fa, modulus: u64) -> Result<DFAO<ModInt, usize>, BridgeError> {
    if fa.is_true_false_automaton() {
        return Err(BridgeError::TrueFalseAutomaton);
    }
    let alphabet_size = usize::try_from(modulus).expect("modulus fits usize");
    if fa.alphabet_size != alphabet_size {
        return Err(BridgeError::AlphabetMismatch {
            expected: alphabet_size,
            found: fa.alphabet_size,
        });
    }
    if fa.q == 0 {
        return Err(BridgeError::NoStates);
    }
    let mut transitions = HashMap::with_capacity(fa.q * alphabet_size);
    for (q, row) in fa.d.iter().enumerate().take(fa.q) {
        for a in 0..modulus {
            let symbol = a as i32;
            let dests = row
                .get(&symbol)
                .filter(|v| !v.is_empty())
                .ok_or(BridgeError::MissingTransition { state: q, symbol })?;
            if dests.len() > 1 {
                return Err(BridgeError::NotDeterministic { state: q, symbol });
            }
            let dest = dests[0];
            if dest >= fa.q {
                return Err(BridgeError::DestinationOutOfRange {
                    state: q,
                    symbol,
                    dest,
                });
            }
            transitions.insert((q, ModInt::new(a, modulus)), dest);
        }
    }
    let mut states = Vec::with_capacity(fa.q);
    states.push(fa.q0);
    states.extend((0..fa.q).filter(|&q| q != fa.q0));
    Ok(DFAO {
        states,
        transitions,
    })
}

/// [`dfao_from_fa`] for a one-track engine [`Automaton`] over `msd_p`/`lsd_p`: checks the
/// single-track/base-`p` shape, reads the direction off the track, and returns both.
pub fn dfao_from_automaton(a: &Automaton) -> Result<(DFAO<ModInt, usize>, Direction), BridgeError> {
    if a.is_true_false_automaton() {
        return Err(BridgeError::TrueFalseAutomaton);
    }
    let tracks = a.track_count();
    if tracks != 1 {
        return Err(BridgeError::NotSingleTrack { tracks });
    }
    let alphabet = a.track_alphabet(0);
    let is_base_k = alphabet
        .iter()
        .enumerate()
        .all(|(i, &d)| i32::try_from(i).map(|i| i == d).unwrap_or(false));
    if !is_base_k || alphabet.is_empty() {
        return Err(BridgeError::NotBaseKAlphabet {
            alphabet: alphabet.to_vec(),
        });
    }
    let direction = match a.track_msd(0) {
        Some(true) => Direction::Msd,
        Some(false) => Direction::Lsd,
        None => return Err(BridgeError::NoDirection),
    };
    let dfao = dfao_from_fa(&a.fa, alphabet.len() as u64)?;
    Ok((dfao, direction))
}

/// Make a partial deterministic [`Fa`] total by routing every missing transition to a
/// fresh dead state with output `dead_output` (appended as the last state, looping on
/// every symbol). Pass `0` for the usual rejecting sink; any other value makes the sink
/// an accepting/valued state and changes the language accordingly. A no-op copy if the input is already total. Use before [`dfao_from_fa`] on
/// an engine automaton that came out of `minimize` (which drops dead states).
pub fn totalize_dead(fa: &Fa, dead_output: i32) -> Fa {
    let n = fa.q;
    let needs_dead = (0..n).any(|q| {
        (0..fa.alphabet_size as i32).any(|a| fa.d[q].get(&a).is_none_or(|v| v.is_empty()))
    });
    if !needs_dead {
        return fa.clone();
    }
    let dead = n;
    let mut d = fa.d.clone();
    for row in d.iter_mut().take(n) {
        for a in 0..fa.alphabet_size as i32 {
            if row.get(&a).is_none_or(|v| v.is_empty()) {
                row.insert(a, vec![dead]);
            }
        }
    }
    let mut dead_row = BTreeMap::new();
    for a in 0..fa.alphabet_size as i32 {
        dead_row.insert(a, vec![dead]);
    }
    d.push(dead_row);
    let mut o = fa.o.clone();
    o.push(dead_output);
    Fa::with_states(fa.q0, n + 1, fa.alphabet_size, o, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_constant_term_sequences::laurent_poly::LaurentPoly;
    use wr_core::equiv::language_equivalent;

    /// Thue–Morse as a substrate DFAO over base 2 with `usize` states: state = parity
    /// of the number of 1s read so far; output = that parity.
    fn thue_morse_dfao() -> DFAO<ModInt, usize> {
        let mut transitions = HashMap::new();
        for s in 0..2usize {
            transitions.insert((s, ModInt::new(0, 2)), s);
            transitions.insert((s, ModInt::new(1, 2)), 1 - s);
        }
        DFAO {
            states: vec![0, 1],
            transitions,
        }
    }

    #[test]
    fn dfao_to_fa_reproduces_the_transition_function_and_outputs() {
        let dfao = thue_morse_dfao();
        let fa = fa_from_dfao(&dfao, 2, |&s| s as i32).unwrap();
        assert_eq!(fa.q, 2);
        assert_eq!(fa.q0, 0);
        assert_eq!(fa.alphabet_size, 2);
        assert_eq!(fa.o, vec![0, 1]);
        assert_eq!(fa.d[0][&0], vec![0]);
        assert_eq!(fa.d[0][&1], vec![1]);
        assert_eq!(fa.d[1][&1], vec![0]);
        // The engine agrees with the substrate on every input up to length 8.
        for n in 0..256u64 {
            let expected = dfao.compute_lsd(n, 2, |&s| s);
            let mut q = fa.q0;
            for digit in ModInt::get_digits(n, 2) {
                q = fa.d[q][&(digit.value as i32)][0];
            }
            assert_eq!(fa.o[q] as usize, expected, "n = {n}");
        }
    }

    #[test]
    fn fa_to_dfao_puts_q0_first_and_keeps_ids_as_values() {
        let mut fa = fa_from_dfao(&thue_morse_dfao(), 2, |&s| s as i32).unwrap();
        fa.q0 = 1; // start at the odd state
        let dfao = dfao_from_fa(&fa, 2).unwrap();
        assert_eq!(dfao.states, vec![1, 0]);
        assert_eq!(dfao.transitions[&(1usize, ModInt::new(1, 2))], 0);
        assert_eq!(dfao.compute_lsd(3, 2, |&s| s), 1); // 3 = 11b: two flips from 1
                                                       // Round trip: back to an Fa, language-equivalent to the original (as DFAs on
                                                       // accept = output != 0).
        let back = fa_from_dfao(&dfao, 2, |&s| fa.o[s]).unwrap();
        assert!(language_equivalent(&fa, &back).unwrap());
    }

    #[test]
    fn a_partial_engine_automaton_is_refused_unless_totalized() {
        let mut fa = fa_from_dfao(&thue_morse_dfao(), 2, |&s| s as i32).unwrap();
        fa.d[1].remove(&0);
        assert_eq!(
            dfao_from_fa(&fa, 2).unwrap_err(),
            BridgeError::MissingTransition {
                state: 1,
                symbol: 0
            }
        );
        let total = totalize_dead(&fa, 0);
        assert_eq!(total.q, 3);
        assert_eq!(total.d[1][&0], vec![2]);
        assert_eq!(total.d[2][&1], vec![2]);
        let dfao = dfao_from_fa(&total, 2).unwrap();
        assert_eq!(dfao.states.len(), 3);
        // Already-total input is returned unchanged.
        assert_eq!(totalize_dead(&total, 0).q, 3);
    }

    #[test]
    fn shape_errors_are_reported_not_guessed() {
        let fa = fa_from_dfao(&thue_morse_dfao(), 2, |&s| s as i32).unwrap();
        assert_eq!(
            dfao_from_fa(&fa, 3).unwrap_err(),
            BridgeError::AlphabetMismatch {
                expected: 3,
                found: 2
            }
        );
        assert_eq!(
            dfao_from_fa(&Fa::trivial(true), 2).unwrap_err(),
            BridgeError::TrueFalseAutomaton
        );
        let mut nfa = fa.clone();
        nfa.d[0].insert(0, vec![0, 1]);
        assert_eq!(
            dfao_from_fa(&nfa, 2).unwrap_err(),
            BridgeError::NotDeterministic {
                state: 0,
                symbol: 0
            }
        );
        let empty: DFAO<ModInt, usize> = DFAO {
            states: vec![],
            transitions: HashMap::new(),
        };
        assert_eq!(
            fa_from_dfao(&empty, 2, |&s| s as i32).unwrap_err(),
            BridgeError::NoStates
        );
        let mut dup = thue_morse_dfao();
        dup.states.push(0);
        assert_eq!(
            fa_from_dfao(&dup, 2, |&s| s as i32).unwrap_err(),
            BridgeError::DuplicateState { index: 2 }
        );
        let mut missing = thue_morse_dfao();
        missing.transitions.remove(&(1usize, ModInt::new(0, 2)));
        assert_eq!(
            fa_from_dfao(&missing, 2, |&s| s as i32).unwrap_err(),
            BridgeError::MissingTransition {
                state: 1,
                symbol: 0
            }
        );
        assert_eq!(
            fa_from_dfao(&thue_morse_dfao(), 3, |&s| s as i32).unwrap_err(),
            BridgeError::ModulusMismatch {
                expected: 3,
                found: 2
            }
        );
    }

    #[test]
    fn the_automaton_wrapper_has_the_readers_word_automaton_shape() {
        let a = automaton_from_dfao(&thue_morse_dfao(), 2, Direction::Lsd, |&s| s as i32).unwrap();
        assert_eq!(a.track_count(), 1);
        assert_eq!(a.track_alphabet(0), &[0, 1]);
        assert_eq!(a.label, vec!["0".to_string()]);
        assert_eq!(a.track_msd(0), Some(false));
        assert_eq!(a.track_ns_names_raw(), vec![Some("lsd_2".to_string())]);
        assert!(a.track_all_reps(0).is_none());
        let (back, direction) = dfao_from_automaton(&a).unwrap();
        assert_eq!(direction, Direction::Lsd);
        assert_eq!(back.states, vec![0, 1]);
        let msd =
            automaton_from_dfao(&thue_morse_dfao(), 2, Direction::Msd, |&s| s as i32).unwrap();
        assert_eq!(dfao_from_automaton(&msd).unwrap().1, Direction::Msd);
        assert_eq!(msd.track_ns_names_raw(), vec![Some("msd_2".to_string())]);
    }

    #[test]
    fn a_multi_track_or_non_base_k_automaton_is_refused() {
        let fa = fa_from_dfao(&thue_morse_dfao(), 2, |&s| s as i32).unwrap();
        let two_tracks = Automaton::new(
            fa.clone(),
            vec![vec![0], vec![0, 1]],
            vec!["0".into(), "1".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(
            dfao_from_automaton(&two_tracks).unwrap_err(),
            BridgeError::NotSingleTrack { tracks: 2 }
        );
        let odd = Automaton::new(
            fa.clone(),
            vec![vec![1, 2]],
            vec!["0".into()],
            vec![Some(true)],
        );
        assert_eq!(
            dfao_from_automaton(&odd).unwrap_err(),
            BridgeError::NotBaseKAlphabet {
                alphabet: vec![1, 2]
            }
        );
        let undirected = Automaton::new(fa, vec![vec![0, 1]], vec!["0".into()], vec![None]);
        assert_eq!(
            dfao_from_automaton(&undirected).unwrap_err(),
            BridgeError::NoDirection
        );
    }

    #[test]
    fn a_real_constant_term_dfao_bridges_with_its_constant_term_as_output() {
        // The central binomial coefficients mod 2: CT((x + x^{-1})^n) = binom(n, n/2)
        // for even n, 0 for odd — as a DFAO over base 2 from `poly_auto`.
        let p = LaurentPoly::from_vec(vec![(1, 1), (-1, 1)], 2);
        let q = LaurentPoly::one(2);
        let dfao = DFAO::poly_auto(&p, &q, 1000).expect("small automaton");
        let a = automaton_from_poly_dfao(&dfao, Direction::Lsd).unwrap();
        assert_eq!(a.fa.q, dfao.states.len());
        for n in 0..64u64 {
            let expected = dfao.compute_ct(n).value as i32;
            let mut state = a.fa.q0;
            for digit in ModInt::get_digits(n, 2) {
                state = a.fa.d[state][&(digit.value as i32)][0];
            }
            assert_eq!(a.fa.o[state], expected, "n = {n}");
        }
    }
}
