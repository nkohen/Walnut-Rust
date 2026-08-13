// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Phase 3a U8's differential gate: `wr_core::regex`'s hand-rolled Brics-dialect engine
//! against the real `walnut-java` CLI's `reg` command.
//!
//! Every case below was run through the real jar (see `../CAPTURE.md`'s
//! "`reg` corpus" section for the exact reproduction recipe); the resulting
//! `Automata Library/*.txt` files are committed under `../fixtures/reg/`. The Rust side
//! rebuilds the same automaton from the same `(alphabets, regex)` pair and compares by
//! **language equivalence** via `wr_core`'s oracle, never structurally
//! (`CLAUDE.md` prime directive #1).
//!
//! Two extra assertions ride along, both deliberate:
//!
//! * **state count.** The oracle alone would pass a correct-but-non-minimal result.
//!   Real Walnut prints `Set from brics:N states` and writes an `N`-state file, and this
//!   port's pipeline is specifically built to reproduce Brics' own
//!   determinize/totalize/minimize/`removeDeadTransitions` shape — so a state-count
//!   divergence is a real signal (a missing dead-state prune, or a non-minimal result),
//!   not noise. State *numbering* is deliberately NOT compared; Brics numbers states out
//!   of an identity-hashed `HashSet`.
//! * **`&`, `~` and `[^…]` have their own cases.** Zero of the golden corpus's 37 `reg`
//!   fixtures exercise those three operators, so golden-corpus round-tripping alone would
//!   pass with them unimplemented. `r03`/`r05`/`r17`/`r18`/`r25`/`r27`–`r30`/`r35`–`r37`/
//!   `r41`/`r44`/`r46`/`r47`/`r54`/`r55`/`r57`/`r59` cover intersection and complement;
//!   `r04`/`r16`/`r45` cover the negated character class.

use std::path::{Path, PathBuf};

use wr_core::automaton::AutomatonDFA;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::regex::{determine_encoded_regex, RegexError};

/// `Main/Commands/Reg.reg` (`Reg.java:18-40`), minus the file writing: encode the
/// user's regex against the declared alphabets, then build the automaton.
fn reg(alphabets: Vec<Vec<i32>>, baseexp: &str) -> Result<AutomatonDFA, RegexError> {
    let encoded = determine_encoded_regex(baseexp, &alphabets)?;
    // `Alphabet.determineAlphabetsAndNS` leaves the `NumberSystem` null for a literal
    // `{…}` alphabet, which is this crate's `None`.
    let msd = vec![None; alphabets.len()];
    AutomatonDFA::from_encoded_regex(&encoded, alphabets, msd)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/reg/{name}.txt"))
}

/// One track, `{0, 1}`.
fn b2() -> Vec<Vec<i32>> {
    vec![vec![0, 1]]
}

/// `(fixture name, declared alphabets, the regex exactly as typed in the command file)`.
#[allow(clippy::type_complexity)]
fn corpus() -> Vec<(&'static str, Vec<Vec<i32>>, &'static str)> {
    vec![
        ("r01", b2(), "0*"),
        ("r02", b2(), "(0|1)*1"),
        ("r03", b2(), "~(0*)"),
        ("r04", b2(), "[^0]*"),
        ("r05", b2(), "(0*1*)&(1*0*)"),
        ("r06", b2(), "."),
        ("r07", b2(), ""),
        ("r08", vec![vec![0, 1, 2]], "[0-1]*"),
        ("r09", b2(), "0*1"),
        ("r10", b2(), "0?1+"),
        ("r11", b2(), "()"),
        ("r12", b2(), "#"),
        ("r13", b2(), "@"),
        ("r14", b2(), ".."),
        ("r15", vec![vec![0, 1, 2]], "[0-1]*2"),
        ("r16", vec![vec![0, 1, 2]], "[^1]*"),
        ("r17", b2(), "~(0*1*)"),
        ("r18", b2(), "(00)*&(000)*"),
        ("r19", b2(), "2*"),
        ("r20", b2(), "~()"),
        ("r21", b2(), "0 1 *"),
        ("r22", b2(), "((0|1)(0|1))*"),
        ("r23", vec![vec![0, 1], vec![0, 1]], "[0,1][1,0]*"),
        ("r24", vec![vec![0, 1], vec![0, 1]], "([0,0]|[1,1])*"),
        ("r25", vec![vec![0, 1], vec![0, 1]], "~([0,0]*)"),
        // The command file's `"\\*"` is a literal backslash, backslash, star: Brics'
        // `parseCharExp` eats the first backslash as an escape, so this is `(\)*` and,
        // since `\` is outside the alphabet, the language is exactly `{ε}`.
        ("r26", b2(), "\\\\*"),
        ("r27", b2(), "0|1&1"),
        ("r28", b2(), "~0|1"),
        ("r29", b2(), "~0*"),
        ("r30", b2(), "~~0"),
        ("r31", vec![vec![0, 1, 2]], "[0-2]"),
        // Inverted character range — `BasicAutomata.makeCharRange`'s `min <= max` guard
        // makes it the empty language rather than an error.
        ("r32", vec![vec![0, 1, 2]], "[2-0]"),
        ("r33", b2(), "0-1"),
        ("r34", b2(), "[0-]*"),
        ("r35", b2(), "(0|1)&~(00)"),
        ("r36", b2(), "~(~(0*))"),
        ("r37", b2(), "@&0*"),
        ("r38", b2(), "#|0"),
        ("r39", b2(), "0**"),
        ("r40", b2(), "(0|1)?(0|1)?"),
        ("r41", b2(), "~[^0]"),
        // `[01]` is swallowed by `Reg`'s alphabet-vector pattern as the single-element
        // vector `[1]` (`01` parses as the integer 1), NOT as a character class.
        ("r42", b2(), "[01][01]"),
        ("r43", b2(), "\\\\."),
        ("r44", b2(), "~#"),
        ("r45", b2(), "[^01]"),
        ("r46", b2(), "0&1"),
        ("r47", b2(), "((0)|(1))*&~(0*)&~(1*)"),
        ("r48", b2(), "(0000000)*&(00000000000)*"),
        ("r49", b2(), "0(0|1)(0|1)(0|1)1"),
        // WB-024: `[9,9]` encodes to -5, which `+128` turns into `{` — harmless here
        // because nothing follows it that could be read as a repeat count. See
        // `wb_024_*` below for the order-flipped companion, which throws.
        ("r50", vec![vec![0, 1, 2, 3], vec![0, 1]], "[9,9][0,0]"),
        // Alphabet order matters: `4` is at index 1 of `{2,4,1}`.
        ("r51", vec![vec![2, 4, 1]], "4*"),
        // `UtilityMethods.removeDuplicates` on the declared alphabet.
        ("r52", vec![vec![1, 0]], "10*"),
        (
            "r53",
            vec![vec![0, 1], vec![0, 1], vec![0, 1]],
            "[0,0,0]*[1,1,1]",
        ),
        ("r54", b2(), "~(.)"),
        ("r55", b2(), "(0|1)*&~((0|1)*00(0|1)*)"),
        ("r56", vec![vec![0, 1, 2, 3]], "[1-2]*"),
        ("r57", b2(), "0*&~(0)&~()"),
        // `[10]` is likewise a vector, holding the integer 10 — which is in neither
        // track's alphabet, so WB-024's `-1` path makes it the empty language.
        ("r58", b2(), "[10]"),
        ("r59", b2(), "((0*)|(1*))&(.....)"),
        ("r60", b2(), "@@@"),
    ]
}

#[test]
fn reg_matches_real_walnut_output() {
    for (name, alphabets, baseexp) in corpus() {
        let alphabet_snapshot = alphabets.clone();
        let ours = reg(alphabets, baseexp)
            .unwrap_or_else(|e| panic!("{name} (`{baseexp}`) must build, got {e}"));
        let mut ours = ours.into_automaton();

        let mut ground_truth = wr_io::reader::read_automaton_txt(fixture(name))
            .unwrap_or_else(|e| panic!("{name}'s fixture must parse cleanly, got {e:?}"));

        assert_eq!(
            ours.alphabet, alphabet_snapshot,
            "{name} (`{baseexp}`): the built automaton must carry the declared tracks"
        );
        assert_eq!(
            ours.fa.q, ground_truth.fa.q,
            "{name} (`{baseexp}`): state count must match real Walnut's \
             `Set from brics:N states` (ours {}, walnut {})",
            ours.fa.q, ground_truth.fa.q
        );

        ours.fa.totalize(0);
        ground_truth.fa.totalize(0);
        assert_eq!(
            automaton_language_equivalent(&ours, &ground_truth),
            Ok(true),
            "{name} (`{baseexp}`): language must match real walnut-java's output"
        );
    }
}

/// The three parse failures captured from the same run. Real Walnut lets dk.brics'
/// `IllegalArgumentException` escape uncaught, printing
/// `java.lang.IllegalArgumentException: <message>` — the message and its character
/// position (an index into the *wrapped* `"(" + regex + ")&[…]*"` string) are what this
/// pins.
#[test]
fn reg_parse_errors_match_real_walnut_messages() {
    let cases: Vec<(Vec<Vec<i32>>, &str, &str)> = vec![
        (b2(), "0|", "expected ')' at position 10"),
        (b2(), "<abc>", "'abc' not found"),
        (b2(), "<1-5>", "interval syntax error at position 5"),
        (b2(), "0{2,3}", "integer expected at position 3"),
    ];
    for (alphabets, baseexp, expected) in cases {
        let err = reg(alphabets, baseexp).expect_err(&format!(
            "`{baseexp}` must fail exactly as real Walnut does"
        ));
        assert_eq!(err.message(), expected, "for regex `{baseexp}`");
    }
}

/// `docs/WALNUT-BUGS.md` WB-024, end to end through the same entry point the `reg`
/// command uses: two regexes that differ only in the ORDER of their two alphabet
/// vectors behave completely differently, because the out-of-alphabet digit `9` encodes
/// to `-1` per track and `convertEncodingForBrics`'s `+128` then lands inside dk.brics'
/// reserved ASCII range instead of above it.
#[test]
fn wb_024_alphabet_offset_collision_is_order_dependent() {
    let alphabets = vec![vec![0, 1, 2, 3], vec![0, 1]];

    // `[9,9]` -> encode = 1*(-1) + 4*(-1) = -5 -> char 123 == '{'.
    // Followed by `[0,0]` -> char 128, so the '{' is a harmless literal.
    let ok = reg(alphabets.clone(), "[9,9][0,0]").expect("the '{' comes first: parses fine");
    assert!(
        ok.automaton().fa.is_language_empty(),
        "'{{' is not in the alphabet, so the language is empty"
    );

    // Flip the two vectors and the same '{' now follows an expression, where dk.brics
    // reads it as the start of a `{n,m}` repeat count.
    let err = reg(alphabets, "[0,0][9,9]").expect_err("the '{' now starts a repeat count");
    assert_eq!(err.message(), "integer expected at position 3");
}
