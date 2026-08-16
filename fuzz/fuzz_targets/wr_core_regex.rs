// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Tier-5 fuzz target: `wr-core`'s hand-rolled Brics-dialect regex engine
//! (`crates/wr-core/src/regex.rs` — the Phase 3a unit transliterated from the real
//! `dk.brics:automaton` sources, the largest single unit of that phase).
//!
//! Crash-freedom only, same contract as the other two targets: any `Fa`/`AutomatonDFA`
//! or any `RegexError` is a pass. `tests/differential/tests/reg_brics_regex.rs` already
//! checks the *semantics* against the real `walnut-java` CLI over 60 cases; this target
//! exists for the inputs that corpus by construction does not contain — truncated
//! escapes, unbalanced brackets, empty character classes, reversed ranges, deeply nested
//! groups, lone surrogates, and everything else a hand-written parser gets wrong.
//!
//! Both public entry points are driven, because they are different parsers, not one
//! wrapping the other:
//!
//! * [`wr_core::regex::convert_from_brics`] — the single-track form, which maps each
//!   alphabet digit to a literal character and hands the whole string to the Brics
//!   parser.
//! * [`wr_core::regex::determine_encoded_regex`] + `AutomatonDFA::from_encoded_regex` —
//!   the multi-track path the `reg` command actually uses, which pre-scans the string
//!   for `[a,b,…]` alphabet vectors and rewrites them to private-use code units before
//!   the Brics parser ever sees it. That pre-scan is its own hand-rolled matcher and is
//!   the part with no single-track equivalent.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wr_core::automaton::AutomatonDFA;
use wr_core::regex::{convert_from_brics, determine_encoded_regex};

/// Ceiling on input size, matching the documented `-max_len=128`, and enforced here so a
/// seed or replayed artifact cannot bypass it. Every one of the 58 seeds extracted from
/// `tests/differential/tests/reg_brics_regex.rs`'s corpus is well under it (the longest
/// real Walnut `reg` expression this port has seen is ~40 characters).
const MAX_INPUT_LEN: usize = 128;

/// Largest repetition count allowed inside a `{…}` span, expressed as a digit-run
/// length: 2 digits, i.e. counts below 100.
///
/// Brics' `{n,m}` operator is the one construct where a handful of input bytes buys
/// unbounded work — `0{0,99999999}` is eleven characters and an eight-figure NFA, an
/// OOM that is textbook regex-expansion cost rather than a defect in this port (real
/// Walnut, on the real Brics library, does exactly the same). Bounding the *count* while
/// leaving the rest of the `{…}` grammar (missing comma, missing brace, non-numeric
/// body, `{,n}`, `{n,}`) fully fuzzable is the narrowest filter that keeps the run
/// productive. The scan is deliberately brace-span-scoped rather than global: digit runs
/// are entirely ordinary *outside* `{…}` (`(0101)*` over the base-2 alphabet), so a
/// global digit filter would reject a large share of the real seed corpus.
const MAX_REPEAT_DIGIT_RUN: usize = 2;

/// Structural budget on the *construction*-blowing operators, in the same
/// "cost, not bug" spirit as [`MAX_REPEAT_DIGIT_RUN`].
///
/// Thompson construction is linear, but Brics' `e+` desugars to `concat(e, star(e))` —
/// so `e++++…` **doubles** the automaton per `+`, and `~` forces a determinize before
/// complementing, which is exponential on top of that. `~+++…+(0*1~h0*1*)` (52 `+`s, 63
/// characters, found by this target within four minutes) does not finish in ten minutes,
/// and real Walnut on the real `dk.brics:automaton` library blows up identically —
/// nothing about it is specific to this port.
///
/// The three limits below bound the compiled size while leaving the *parser* — which is
/// what this target is really for, and the part `PORTING.md` flags as risky — completely
/// unconstrained. Every seed in the committed corpus passes them comfortably (the
/// heaviest, `((0)|(1))*&~(0*)&~(1*)`, uses 3 repetitions and 2 complements).
/// The third limit, [`MAX_ATOMS`], bounds the *other* half of the classic blowup, the one
/// no operator count catches: `Σ*` (spelled `~(0*)`, or `(0|1)*`, or `.*`) followed by a
/// long concatenation. Determinizing `Σ*·α₁α₂…αₙ` needs a state per subset of "which of
/// the last n symbols matched", i.e. 2ⁿ, and n is just the concatenation length — so
/// `(0|1)*` plus 40 more characters is a 2⁴⁰-state DFA from a 46-character input. This
/// target found exactly that shape (`..444444*4444444444.....@4444…`) and OOM'd at 2 GB.
/// Again: real Walnut/Brics blows up identically; the exponent is in the algorithm, not
/// in this port. Capping the atom count at 20 caps the intermediate DFA at ~2²⁰ states,
/// which fits the documented `-rss_limit_mb` budget.
///
/// "Atom" is counted as any character that is not one of the postfix/infix operators
/// `* + ? ~ & | ( )` — a deliberate over-count (it also counts `[`, `]`, `,`, `-`, so a
/// character class counts as several atoms rather than one), which is the safe direction
/// for a bound.
const MAX_REPETITION_OPS: usize = 10;
const MAX_CONSECUTIVE_REPETITION_OPS: usize = 2;
const MAX_COMPLEMENT_OPS: usize = 3;
const MAX_ATOMS: usize = 20;

fn structure_is_in_budget(s: &str) -> bool {
    let mut repetitions = 0usize;
    let mut consecutive = 0usize;
    let mut complements = 0usize;
    let mut atoms = 0usize;
    for c in s.chars() {
        match c {
            '*' | '+' | '?' => {
                repetitions += 1;
                consecutive += 1;
                if repetitions > MAX_REPETITION_OPS || consecutive > MAX_CONSECUTIVE_REPETITION_OPS
                {
                    return false;
                }
            }
            '~' => {
                complements += 1;
                if complements > MAX_COMPLEMENT_OPS {
                    return false;
                }
                consecutive = 0;
            }
            '&' | '|' | '(' | ')' => consecutive = 0,
            _ => {
                consecutive = 0;
                atoms += 1;
                if atoms > MAX_ATOMS {
                    return false;
                }
            }
        }
    }
    true
}

/// A conservative brace-span scanner: it does not have to mirror the parser's own
/// brace handling (unbalanced and nested braces are still fuzzed), only to never let
/// through an input whose `{n,m}` repetition counts exceed the limit above.
///
/// It used to bound `[…]` alphabet-vector digit runs as well — that was the
/// known-crash bypass for Phase 4 U30's finding F1 (`reg r {0,1} "([8888888800])"`
/// panicking in `wr_core::util::parse_int`), and it is gone now that
/// `determine_encoded_regex` reports the overflow as `RegexError::NumberFormat`
/// instead. `[…]` digit runs are therefore fully fuzzed again.
fn numeric_literals_are_in_budget(s: &str) -> bool {
    let mut limit: Option<usize> = None;
    let mut run = 0usize;
    for c in s.chars() {
        match c {
            '{' => {
                limit = Some(MAX_REPEAT_DIGIT_RUN);
                run = 0;
            }
            '}' | ']' => {
                limit = None;
                run = 0;
            }
            _ if c.is_ascii_digit() => {
                run += 1;
                if let Some(max) = limit {
                    if run > max {
                        return false;
                    }
                }
            }
            _ => run = 0,
        }
    }
    true
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_LEN {
        return;
    }
    // A `reg` expression reaches this code as a `String` lexed out of the user's command
    // line, so non-UTF-8 bytes are not a reachable input shape.
    let Ok(regex) = std::str::from_utf8(data) else {
        return;
    };
    if !numeric_literals_are_in_budget(regex) || !structure_is_in_budget(regex) {
        return;
    }

    // Single-track, two alphabets: `{0,1}` (the overwhelmingly common real case) and a
    // non-contiguous, non-zero-based one, which exercises the `List.indexOf`-shaped
    // dense-symbol mapping rather than the identity mapping `{0,1}` happens to produce.
    let _ = convert_from_brics(&[0, 1], regex);
    let _ = convert_from_brics(&[2, 4, 1], regex);

    // Multi-track: the `[a,b]` vector pre-scan plus the full `reg` pipeline
    // (Thompson construction -> determinize -> minimize -> dead-transition prune).
    let alphabets = vec![vec![0, 1], vec![0, 1]];
    if let Ok(encoded) = determine_encoded_regex(regex, &alphabets) {
        // `Alphabet.determineAlphabetsAndNS` leaves the `NumberSystem` null for a literal
        // `{…}` alphabet, which is this crate's `None` — matching how
        // `tests/differential/tests/reg_brics_regex.rs` drives the same pair.
        let msd = vec![None; alphabets.len()];
        let _ = AutomatonDFA::from_encoded_regex(&encoded, alphabets, msd);
    }
});
