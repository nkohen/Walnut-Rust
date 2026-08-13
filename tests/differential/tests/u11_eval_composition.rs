// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Phase 3a's U11 integration checkpoint, spot-checked against real `walnut-java` CLI
//! output. `crates/wr-logic/src/eval.rs`'s own unit tests already prove the parser
//! (U2-U4), the operand semantics (U9's `RelationalOperator`/`ArithmeticOperator`, U10's
//! `LogicalOperator`), and `wr-core`'s engine compose end-to-end using only
//! `wr-logic`+`wr-core`; this crate additionally depends on `wr-io` (to read the
//! captured fixture) and is the right place — not `wr-logic` itself, which must stay
//! free of a `wr-io` dependency — for the "does it also match real Walnut's actual
//! decision-procedure output" half of that unit's Done-when, per `CLAUDE.md`'s semantic-
//! equivalence rule (never byte/structural comparison).
//!
//! Four cases, each captured the same one-time way as `../CAPTURE.md`'s existing entries
//! (see this file's own capture recipe below):
//!
//! * `u11check`: `x>=2 & x<5` (a free-variable boolean+relational composition) —
//!   compared via [`wr_core::equiv::automaton_language_equivalent`] against the fixture.
//! * `u11closed`: `Ex (x < 5 & x >= 2)` (adds ∃-quantification) — real Walnut prints
//!   `TRUE`, matching this port's own `end_to_end_predicate_string_to_automaton_no_wr_cli_no_wr_io`
//!   unit test in `eval.rs`; checked here as a trivial-automaton verdict rather than a
//!   fixture file (a closed formula has no `.txt` automaton body worth capturing beyond
//!   the printed `TRUE`/`FALSE` line, matching how `AutomatonWriter` itself has no
//!   meaningful `.txt` body for a trivial automaton per `docs/DESIGN.md`'s
//!   `TRUE_FALSE_AUTOMATON` handling).
//! * `u11xyz`: `x + y = z` — a **three-track** case, which is what makes the track-order
//!   normalization below load-bearing rather than theoretical (see [`compare_to_fixture`]).
//! * `zphi`: `a < b` — a **two-track** case, and the ground truth behind the
//!   hand-transcribed `zphi_a_less_than_b()` fixture in `wr-logic`'s own `eval.rs` tests
//!   (that crate must stay free of a `wr-io` dependency, so it cannot read this file
//!   itself; comparing the same predicate here is what keeps the transcription honest).
//!
//! ## Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java
//! cat > "Command Files/u11_capture.txt" <<'EOF'
//! eval u11check "?msd_2 x>=2 & x<5";
//! EOF
//! java -jar target/Walnut-all.jar u11_capture.txt < /dev/null
//! cp "Session/<timestamp>/Automata Library/u11check.txt" \
//!    ~/dev/walnut-rs/tests/differential/fixtures/u11/boolean_relational.txt
//!
//! cat > "Command Files/u11_capture2.txt" <<'EOF'
//! eval u11closed "?msd_2 Ex (x < 5 & x >= 2)";
//! EOF
//! java -jar target/Walnut-all.jar u11_capture2.txt < /dev/null
//! # stdout prints "____\nTRUE" -- no fixture file needed, see module docs above.
//!
//! cat > "Command Files/u11_capture3.txt" <<'EOF'
//! eval u11xyz "?msd_2 x + y = z";
//! def zphi "?msd_2 a < b";
//! EOF
//! java -jar target/Walnut-all.jar u11_capture3.txt < /dev/null
//! cp "Session/<timestamp>/Automata Library/u11xyz.txt" \
//!    ~/dev/walnut-rs/tests/differential/fixtures/u11/addition_three_track.txt
//! cp "Session/<timestamp>/Automata Library/zphi.txt" \
//!    ~/dev/walnut-rs/tests/differential/fixtures/u11/zphi_a_lt_b.txt
//! ```
//!
//! The command files and `Session/<timestamp>/` directories were deleted from the
//! `walnut-java` checkout afterward, matching `../CAPTURE.md`'s existing recipes.

use std::path::Path;

use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_logic::predicate_env::InMemoryPredicateEnv;

/// Evaluates `predicate` through the Rust pipeline and compares it, by semantic language
/// equivalence, against the captured `walnut-java` fixture at `fixtures/u11/<fixture>`.
///
/// **The `sort_label()` call is not cosmetic.** Java's `AutomatonWriter.writeToTxtFormat`
/// calls `automaton.canonize()` (`Automata/Writer/AutomatonWriter.java:52`), and
/// `canonize` calls `sortLabel` (`Automata/Automaton.java:328`, `:348-379`) — so **every**
/// captured `.txt` fixture has its tracks in alphabetical label order. The port's
/// `evaluate()` returns tracks in whatever order the parser/quantifier pipeline happened to
/// produce (`["z", "x", "y"]` for `x + y = z`), and
/// [`wr_core::equiv::automaton_language_equivalent`] does **not** detect a label
/// permutation whose per-position alphabets match — U8's own documented limitation: it
/// silently returns a WRONG verdict rather than erroring. Without this normalization the
/// three-track case below reports `Ok(false)` and reads as a decision-procedure bug when
/// the only thing wrong is the comparison.
fn compare_to_fixture(predicate: &str, fixture: &str, expected_alphabet: &[Vec<i32>]) {
    let env = InMemoryPredicateEnv::new();
    let mut ours = wr_logic::eval::evaluate(&env, predicate)
        .expect("the Rust pipeline must evaluate this predicate end-to-end");
    ours.sort_label();

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/u11")
        .join(fixture);
    let mut ground_truth: Automaton =
        wr_io::reader::read_automaton_txt(&path).expect("fixture must parse cleanly");

    // Real Walnut's automaton for a free-variable predicate is not necessarily total
    // (missing transitions mean implicit rejection) -- same totalize-before-comparing
    // step `../tests/spike_ei_i_lt_x.rs` already established for exactly this reason.
    ours.fa.totalize(0);
    ground_truth.fa.totalize(0);

    assert_eq!(ground_truth.alphabet, expected_alphabet);
    assert_eq!(ours.alphabet, expected_alphabet);
    assert_eq!(
        automaton_language_equivalent(&ours, &ground_truth),
        Ok(true),
        "the Rust pipeline's result for `{predicate}` must be language-equivalent to real \
         walnut-java's output (see this file's module docs for the capture)"
    );
}

#[test]
fn boolean_relational_composition_matches_real_walnut_output() {
    compare_to_fixture("?msd_2 x>=2 & x<5", "boolean_relational.txt", &[vec![0, 1]]);
}

/// The multi-track case. This is the one that fails (`Ok(false)`) if
/// [`compare_to_fixture`]'s `sort_label()` is removed: the port produces tracks in
/// `["z", "x", "y"]` order, the fixture is in `["x", "y", "z"]` order, and the equivalence
/// oracle cannot see the difference on its own.
#[test]
fn three_track_addition_matches_real_walnut_output() {
    compare_to_fixture(
        "?msd_2 x + y = z",
        "addition_three_track.txt",
        &[vec![0, 1], vec![0, 1], vec![0, 1]],
    );
}

/// The two-track `def zphi "?msd_2 a < b"` case — also the ground truth behind
/// `wr-logic`'s hand-transcribed `zphi_a_less_than_b()` test fixture (see module docs).
#[test]
fn two_track_comparison_matches_real_walnut_output() {
    compare_to_fixture("?msd_2 a < b", "zphi_a_lt_b.txt", &[vec![0, 1], vec![0, 1]]);
}

#[test]
fn quantifier_elimination_composition_matches_real_walnut_verdict() {
    // Real `walnut-java` prints "____\nTRUE" for `eval u11closed "?msd_2 Ex (x < 5 & x
    // >= 2)";` (see this file's module docs) -- a closed formula collapses to a trivial
    // automaton with no meaningful `.txt` body, so the ground truth here is the printed
    // verdict itself, not a fixture file.
    let env = InMemoryPredicateEnv::new();
    let ours = wr_logic::eval::evaluate(&env, "?msd_2 Ex (x < 5 & x >= 2)")
        .expect("the Rust pipeline must evaluate this predicate end-to-end");

    assert!(ours.fa.is_true_false_automaton());
    assert!(
        ours.fa.is_true_automaton(),
        "must match real walnut-java's printed TRUE verdict"
    );
}
