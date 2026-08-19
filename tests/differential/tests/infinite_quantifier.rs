// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for the **`I` (infinitely-often) quantifier**, spot-checked
//! against real `walnut-java` CLI output — `docs/BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md`
//! item 1.
//!
//! # Why this file exists
//!
//! A backlog note hypothesized that `act_quantifier`'s `I` arm might apply
//! `remove_leading_zeros`'s msd fixup unconditionally, mirroring a bug Phase 3b's L1 fixed
//! in the sibling `wr_core::quantify` path (used by `E`/`A`, not `I`). Investigation (a
//! fresh Opus subagent, 47 live queries against the real `walnut-java` CLI, 0
//! divergences — see `CLAUDE.md`'s "Current status" for the account) refuted the
//! hypothesis: `remove_leading_zeros` was already msd/lsd-aware on both sides. But the
//! investigation also found the real gap the hypothesis pointed at: **the only existing
//! coverage of `I` over `lsd` — Tier-1 golden fixture 520
//! (`eval test520 "?lsd_10 Ix x > 0"`) — cannot distinguish a correct fixup from the
//! hypothesized unconditional-msd bug**: `x > 0` is cofinite either way (an lsd word with
//! a nonzero FIRST symbol still admits unboundedly many trailing zeros, so both the
//! correct and the buggy fixup leave it infinite), so that fixture would pass unchanged
//! under the bug. U29's Tier-3 generator doesn't help either — it explicitly never emits
//! `I` queries at all (documented in its own "Known, explicit non-coverage" section). So
//! there was no coverage that could actually have caught this. This file, together with
//! `crates/wr-logic/src/token.rs`'s new
//! `infinite_quantifier_over_lsd_matches_msd_shape` /
//! `infinite_quantifier_lsd_two_variables_or_fold` /
//! `infinite_quantifier_mixed_numeration_system_selects_correct_direction` unit tests,
//! closes that gap with cases that are actually direction-sensitive (verified by
//! mutating `remove_leading_zeros_helper`'s `if !msd { reverse_with_ctx(...) }` branch
//! and confirming every lsd case here and in `token.rs` fails).
//!
//! `I` always collapses to a TRUE/FALSE trivial automaton (see `wr_core::infinite`'s and
//! `Token::act_quantifier`'s module docs), so — unlike `lsd_numeration.rs`'s comparison
//! cases — there is no automaton to capture into a fixture; every case here is checked
//! against real Walnut's printed verdict directly, the same convention
//! `lsd_numeration.rs`'s own `lsd_closed_formulae_match_real_walnut_verdicts` already
//! uses.
//!
//! ## A note on verdict polarity
//!
//! Six of the eight `assert_verdict` cases below are FALSE, and the two TRUE cases
//! (`Ix x>=5`) are direction-INsensitive controls, not discriminating cases — like
//! fixture 520, an unconditional-msd bug would still leave `x>=5` infinite (cofinite sets
//! stay infinite regardless of which end gets a nonzero-digit requirement). The
//! `infinite_quantifier_mixed_numeration_system_matches_real_walnut_output` test below is
//! what actually closes that gap for THIS file: it is TRUE for `lsd` and would flip to
//! FALSE under the unconditional-msd bug, the same polarity-discriminating shape as
//! `token.rs`'s hand-built-automaton unit test, but here built through the real `reg`
//! command dispatch (`wr_cli::reg::reg`) instead of a hand-rolled `Fa` table.
//!
//! ## Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Command Files/inf_capture.txt" <<'EOF'
//! eval q01 "?msd_2 Ix x<5";
//! eval q02 "?lsd_2 Ix x<5";
//! eval q03 "?msd_2 Ix x>=5";
//! eval q04 "?lsd_2 Ix x>=5";
//! eval q05 "?msd_3 Ix x<4";
//! eval q06 "?lsd_3 Ix x<4";
//! eval q11 "?msd_2 Ix,y (x<5 & y<5)";
//! eval q12 "?lsd_2 Ix,y (x<5 & y<5)";
//! EOF
//! java -jar target/Walnut-all.jar inf_capture.txt < /dev/null
//! # each `eval` prints "____\nTRUE" or "____\nFALSE" to stdout; no fixture files needed.
//! ```
//!
//! All eight verdicts below were confirmed this way (they also match the investigation
//! subagent's independently-run q01/q02/q03/q04/q05/q06/q11/q12 queries, same numbering).

use std::fs;
use std::path::PathBuf;

use wr_cli::reg::reg;
use wr_cli::session::Session;
use wr_core::logging::Logging;
use wr_logic::predicate_env::InMemoryPredicateEnv;

/// Runs `predicate` through the Rust pipeline and asserts it collapses to the given
/// TRUE/FALSE verdict — the shape every `I`-quantified top-level result has.
fn assert_verdict(predicate: &str, expected: bool) {
    let env = InMemoryPredicateEnv::new();
    let m = wr_logic::eval::evaluate(&env, predicate)
        .unwrap_or_else(|e| panic!("`{predicate}` must evaluate end-to-end: {e}"));
    assert!(
        m.fa.is_true_false_automaton(),
        "`{predicate}`: I must collapse to a trivial automaton"
    );
    assert_eq!(
        m.fa.is_true_automaton(),
        expected,
        "`{predicate}` must match real walnut-java's printed verdict"
    );
}

/// `Ix x<5` — five witnesses (values `0..4`), so FALSE on both sides of the msd/lsd
/// divide. The `lsd_2` half is the case that had zero prior coverage.
#[test]
fn infinite_quantifier_finite_language_matches_real_walnut_both_directions() {
    assert_verdict("?msd_2 Ix x<5", false);
    assert_verdict("?lsd_2 Ix x<5", false);
}

/// `Ix x>=5` — infinitely many witnesses, so TRUE on both sides.
#[test]
fn infinite_quantifier_infinite_language_matches_real_walnut_both_directions() {
    assert_verdict("?msd_2 Ix x>=5", true);
    assert_verdict("?lsd_2 Ix x>=5", true);
}

/// Base 3 instead of base 2, so a base-specific digit-decode bug in
/// `remove_leading_zeros_helper`'s reversal branch can't hide behind base 2's degenerate
/// digit-equals-index alphabet.
#[test]
fn infinite_quantifier_base_three_matches_real_walnut_both_directions() {
    assert_verdict("?msd_3 Ix x<4", false);
    assert_verdict("?lsd_3 Ix x<4", false);
}

/// Two quantified variables — `remove_leading_zeros`'s per-track OR-fold is exactly where
/// a multi-track lsd bug would hide behind a single-variable test passing by coincidence.
#[test]
fn infinite_quantifier_two_variables_matches_real_walnut_both_directions() {
    assert_verdict("?msd_2 Ix,y (x<5 & y<5)", false);
    assert_verdict("?lsd_2 Ix,y (x<5 & y<5)", false);
}

// ---------------------------------------------------------------------------
// The polarity-discriminating mixed-numeration-system case
// ---------------------------------------------------------------------------

/// Same scaffolding convention as `reg_and_alphabet_commands.rs`/`lsd_numeration.rs`: a
/// process-scoped directory laid out as a full Walnut home tree.
fn temp_session(tag: &str) -> (Session, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("wr-differential-inf-{tag}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    for sub in [
        "Result",
        "Automata Library",
        "Word Automata Library",
        "Custom Bases",
        "Macro Library",
        "Morphism Library",
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    (session, dir)
}

/// The mixed-NS regression pin from `token.rs`'s
/// `infinite_quantifier_mixed_numeration_system_selects_correct_direction`, but built
/// through the REAL `reg` command dispatch (`wr_cli::reg::reg`, the same one real
/// `reg mixr lsd_2 msd_2 "[0,0]*[1,1]";` goes through) instead of a hand-rolled `Fa`
/// table — genuine coverage of the string-parsing layer that unit test necessarily
/// skips, over the same automaton: `{(0,0)ⁿ(1,1) : n ≥ 0}`, track 0 (`x`) bound `lsd_2`,
/// track 1 (`y`) bound `msd_2`.
///
/// This exercises `wr_core::logicalops::remove_leading_zeros` +
/// `wr_core::infinite::infinite` directly (the exact two primitives, in the exact order,
/// `Token::act_quantifier`'s `I` arm calls — `crates/wr-logic/src/token.rs:1413-1427`)
/// rather than routing through a `predicate` string. A plain `reg`-built automaton carries
/// no track labels (matching real Java's `Reg.java`: a `reg`-built automaton is anonymous
/// until something binds it to variable names, normally `Word`/`Function`'s
/// `$mixr(x,y)` construction) — this test does that binding directly and positionally
/// (`build_mixr`, below) rather than going through a full `$mixr(x,y)`-referencing query,
/// which would additionally need `reg`'s result written to and re-read from
/// `Automata Library/` — plumbing `lsd_numeration.rs`'s `lsd_def_then_reuse_...` test
/// already covers, for a different command, and this file's job (pinning the
/// direction-selection primitive) doesn't need repeated.
///
/// Confirmed against the real `walnut-java` CLI (`reg mixr lsd_2 msd_2 "[0,0]*[1,1]";`
/// then `eval t1 "?lsd_2 ?msd_2 Ix $mixr(x,y)";` → TRUE, `eval t2 "?lsd_2 ?msd_2 Iy
/// $mixr(x,y)";` → FALSE).
#[test]
fn infinite_quantifier_mixed_numeration_system_matches_real_walnut_output() {
    let (session, dir) = temp_session("mixed-ns");

    let build_mixr = || {
        let mut a = reg(
            &session,
            &mut Logging::new(),
            "lsd_2 msd_2 ",
            "[0,0]*[1,1]",
            "mixr",
        )
        .expect("reg must build the mixed-NS automaton")
        .automaton_pairs()[0]
            .automaton()
            .expect("reg's TestCase must carry the built automaton")
            .clone();
        // `reg` itself never assigns track labels (real Java's `Reg.java` doesn't either —
        // a `reg`-built automaton is anonymous until something binds it to variable names,
        // normally `Word`/`Function`'s `$mixr(x,y)` construction). Do that binding
        // directly, positionally, matching `$mixr(x,y)`'s track order (track 0 -> `x`,
        // track 1 -> `y`, i.e. `lsd_2` -> `x`, `msd_2` -> `y`, matching the declaration
        // order above).
        a.label = vec!["x".to_string(), "y".to_string()];
        a
    };

    let x_filtered = wr_core::logicalops::remove_leading_zeros(&build_mixr(), &["x".to_string()])
        .expect("remove_leading_zeros over the lsd track must not fail");
    let x_verdict = wr_core::infinite::infinite(&x_filtered)
        .expect("infinite must not fail on a small trimmed automaton");
    assert!(
        x_verdict.is_some(),
        "Ix $mixr(x,y) must be TRUE (infinitely many lsd x values: 1, 2, 4, 8, ...)"
    );

    let y_filtered = wr_core::logicalops::remove_leading_zeros(&build_mixr(), &["y".to_string()])
        .expect("remove_leading_zeros over the msd track must not fail");
    let y_verdict = wr_core::infinite::infinite(&y_filtered)
        .expect("infinite must not fail on a small trimmed automaton");
    assert!(
        y_verdict.is_none(),
        "Iy $mixr(x,y) must be FALSE (exactly one msd y value: 1)"
    );

    fs::remove_dir_all(&dir).ok();
}
