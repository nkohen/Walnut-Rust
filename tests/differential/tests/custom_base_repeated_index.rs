// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for a **repeated variable indexing a custom-base word-automaton
//! track** — `W[i][i]`, the shape that reaches `Word.java:62`'s
//! `wordAutomaton.getNS().get(i)` and, through it, `wr_logic::token`'s
//! `track_equality_automaton`.
//!
//! # Why this file exists
//!
//! `track_equality_automaton` used to *reconstruct* a whole `NumberSystem` from the track's
//! recorded name in order to read one field off it (`ns.equality`), which forced it to
//! guess "is this a base I can rebuild from no files at all?". Three successive adversarial
//! review rounds each found a different spelling of that guess wrong, always with the same
//! consequence — a wrong answer, or a refusal, on an input real Walnut computes:
//!
//! 1. `Option<NumberSystem>` conflated Java's own `null` (a `{...}`-declared track, WB-013)
//!    with "this port could not rebuild one", so an `msd_fib` track reported WB-013's
//!    Java-verbatim "your alphabet was declared explicitly, e.g. `{0,1}`" — false — on the
//!    *handled*/stdout channel.
//! 2. Keying the split on `all_reps[i].is_some()` missed every custom base with no
//!    all-representations file (that file is optional, `NumberSystem.java:147-149`).
//! 3. Keying it on `NumberSystem::new(name).is_ok()` instead missed a programmatically
//!    parseable name that a `Custom Bases/` file SHADOWS with a different alphabet, and in
//!    the other direction refused perfectly computable custom bases whose alphabet happens
//!    to be a contiguous `0..k-1`.
//!
//! The fix computes the equality automaton directly from the track's own alphabet,
//! direction and all-representations restriction — no name parsing, no reconstruction, no
//! guess. This file is the end-to-end proof, against real `walnut-java` output, on the
//! three shapes that were previously wrong:
//!
//! | base | shape | before this fix | Java |
//! |---|---|---|---|
//! | `msd_bar` over `{0, 1, 5}` | custom base, **no** all-representations file | refused (`walnut-rs.PortLimitation`) | 1-state `msd_bar` automaton |
//! | `msd_neg_3` over `{0, 1, 2, 3}` | `Custom Bases/` files **shadowing** a programmatic negative base whose own alphabet is `{0, 1, 2}` | fabricated the unshadowed base, then died in the cross product with Java's own `"variables with the same label must have the same alphabet"` — on **stdout**, as a `handled` `WalnutException` | 1-state `msd_neg_3` automaton over `{0, 1, 2, 3}` |
//! | `msd_baz` over `{0, 1, 2}` | custom base, contiguous alphabet, no all-representations file | refused (`walnut-rs.PortLimitation`) | 1-state `msd_baz` automaton |
//!
//! All three now match real Java byte-for-byte. The `{...}`-declared-track case that
//! WB-013 is actually about is unaffected and still fails, with Java's own fixed message —
//! see `java_bugfix_wb013.rs`.
//!
//! # Capture recipe
//!
//! See `../CAPTURE.md`'s entry for this file. In short: three throwaway custom bases and
//! three two-track word automata over them, then `eval <name> "<W>[i][i] = @1";`. Every
//! input file and every captured result lives in `fixtures/custom_base_repeated_index/`, so
//! the recipe is fully reproducible from this repo alone.
//!
//! Note `msd_neg_3` needs BOTH an `_addition.txt` and a `_less_than.txt` over `{0, 1, 2, 3}`:
//! `NumberSystem.setLessThanAutomaton` falls back to the *programmatic* negative-base
//! comparator (over `{0, 1, 2}`) when no file is present, and real Walnut then refuses the
//! base outright with `"Inputs of _less_than.txt must have the same alphabet as the alphabet
//! of inputs of _addition.txt : base msd_neg_3"`. A shadowing custom base has to shadow both
//! halves — verified live, both ways.
//!
//! # Comparison method
//!
//! Semantic language equivalence (`wr_core::equiv`), never byte identity, per `CLAUDE.md` —
//! plus three structural assertions a language-only comparison would miss and which are
//! exactly what the three defects above broke: the **alphabet** (defect 2 and 3 substituted
//! a different one), the recorded **number-system name** (defect 3's contiguous case would
//! otherwise be indistinguishable from `msd_3`), and the **direction**.
//!
//! # What these tests catch (mutation-verified, not asserted)
//!
//! Each mutation was applied to `wr_logic::token::track_equality_automaton`, this file
//! re-run, and the mutation reverted:
//!
//! | mutation | caught here? |
//! |---|---|
//! | build the equality automaton over a contiguous alphabet derived from the track's cardinality (round 2's bug) | **yes** — `custom_base_with_no_all_representations_file_matches_java` (the other two bases' alphabets *are* contiguous, so only `msd_bar` can see it) |
//! | drop the all-representations fold (`set_all_reps` + `apply_all_representations`) | **no** — none of these three bases ships an all-representations file, by construction; caught instead by `token.rs`'s `an_all_representations_restriction_is_applied_to_the_computed_equality` |
//! | drop the number-system NAME installed on the equality automaton's own tracks | **no**, and this is worth stating rather than leaving to be rediscovered: the *result*'s header name comes from the word automaton's own track through the cross product, so the equality automaton's copy is unobservable here. It is ported because Java installs it (`NumberSystem.java:364-366`/`:392`) and pinned by eight `token.rs` unit tests, not by this file. |

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::eval_def::{eval_def_command_with_stdout, EvalDefError};
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;
use wr_logic::predicate_env::FreshIdentifiers;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/custom_base_repeated_index")
}

/// A process-scoped Walnut home tree seeded with the custom-base files and the word
/// automaton one case needs — same scaffolding convention as `lsd_custom_base.rs`.
fn temp_session(tag: &str, custom_bases: &[&str], word_automata: &[&str]) -> (Session, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-custom-base-repeated-index-{tag}-{}",
        std::process::id()
    ));
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
    let src = fixtures();
    for name in custom_bases {
        fs::copy(src.join(name), dir.join("Custom Bases").join(name))
            .unwrap_or_else(|e| panic!("must be able to install {name}: {e}"));
    }
    for name in word_automata {
        fs::copy(src.join(name), dir.join("Word Automata Library").join(name))
            .unwrap_or_else(|e| panic!("must be able to install {name}: {e}"));
    }
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    (session, dir)
}

fn run_eval(session: &Session, predicate: &str, name: &str) -> Result<Automaton, EvalDefError> {
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut fresh = FreshIdentifiers::new();
    let mut stdout = Vec::new();
    let tc = eval_def_command_with_stdout(
        session,
        &mut logging,
        &mut fresh,
        false,
        false,
        predicate,
        Some(name),
        None,
        &mut stdout,
    )?;
    Ok(tc.automaton_pairs()[0].automaton().unwrap().clone())
}

/// Runs `eval <out> "<word>[i][i] = @1";` over a throwaway session holding `custom_bases`
/// and `<word>.txt`, then compares the result against the captured `walnut-java` fixture.
fn check(
    tag: &str,
    custom_bases: &[&str],
    word: &str,
    expected_fixture: &str,
    expected_alphabet: &[i32],
    expected_ns_name: &str,
) {
    let (session, dir) = temp_session(tag, custom_bases, &[&format!("{word}.txt")]);
    let mut ours = run_eval(
        &session,
        &format!("{word}[i][i] = @1"),
        &format!("{tag}out"),
    )
    .unwrap_or_else(|e| panic!("{tag}: real Walnut computes this successfully, got {e}"));

    // The fixture's own header names the custom base, so reading it back re-resolves the
    // base through `Custom Bases/` exactly as `wr-io` would for any library automaton.
    let mut ground_truth = wr_io::reader::read_automaton_txt_with_custom_bases(
        fixtures().join(expected_fixture),
        &dir.join("Custom Bases"),
    )
    .unwrap_or_else(|e| panic!("{tag}: fixture must parse cleanly: {e}"));

    ours.sort_label();
    ours.fa.totalize(0);
    ground_truth.fa.totalize(0);

    assert_eq!(
        ground_truth.track_alphabets(),
        vec![expected_alphabet.to_vec()],
        "{tag}: precondition -- the captured fixture's own alphabet"
    );
    assert_eq!(
        ours.track_alphabets(),
        vec![expected_alphabet.to_vec()],
        "{tag}: the result must be over the TRACK's alphabet, not one fabricated from its \
         cardinality or from an unshadowed programmatic base of the same name"
    );
    assert_eq!(
        ours.track_ns_names(),
        vec![Some(expected_ns_name.to_string())],
        "{tag}: the custom base's own NAME must survive into the written header"
    );
    assert_eq!(
        ours.track_msds(),
        vec![Some(true)],
        "{tag}: every base here is msd_*"
    );
    assert!(
        automaton_language_equivalent(&ours, &ground_truth).unwrap(),
        "{tag}: language differs from real walnut-java's answer"
    );
}

/// Round 2's finding: a custom base shipping only `<name>_addition.txt` (no
/// all-representations `<name>.txt`) over a NON-contiguous alphabet.
///
/// Before the fix this was refused outright with `walnut-rs.PortLimitation`; the round
/// before that, it silently fabricated `msd_3` from the alphabet's cardinality and died in
/// the cross product on **stdout**, wearing legitimate Walnut output's clothes.
#[test]
fn custom_base_with_no_all_representations_file_matches_java() {
    check(
        "bar",
        &["msd_bar_addition.txt"],
        "BAR2",
        "expected_barout.txt",
        &[0, 1, 5],
        "msd_bar",
    );
}

/// Round 3's review finding: `Custom Bases/msd_neg_3_{addition,less_than}.txt` over
/// `{0, 1, 2, 3}` SHADOW the programmatic negative base `msd_neg_3`, whose own alphabet is
/// `{0, 1, 2}`.
///
/// The `NumberSystem::new(name).is_ok()` discriminator answered "programmatic, rebuild it"
/// and substituted the unshadowed base — a genuinely different alphabet, which then failed
/// the cross product with Java's own verbatim "variables with the same label must have the
/// same alphabet" text. Direct computation never asks the question.
#[test]
fn custom_base_shadowing_a_programmatic_negative_base_matches_java() {
    check(
        "neg3",
        &["msd_neg_3_addition.txt", "msd_neg_3_less_than.txt"],
        "NEG3",
        "expected_neg3out.txt",
        &[0, 1, 2, 3],
        "msd_neg_3",
    );
}

/// Round 3's *capability regression*: a custom base whose alphabet happens to be a
/// contiguous `0..k-1`, with no all-representations file. `NumberSystem::new("msd_baz")`
/// fails, so the name-keyed discriminator refused it — even though real Walnut computes an
/// answer and the equality automaton is trivially derivable from the track itself.
#[test]
fn contiguous_alphabet_custom_base_matches_java_rather_than_being_refused() {
    check(
        "baz",
        &["msd_baz_addition.txt"],
        "BAZ2",
        "expected_bazout.txt",
        &[0, 1, 2],
        "msd_baz",
    );
}
