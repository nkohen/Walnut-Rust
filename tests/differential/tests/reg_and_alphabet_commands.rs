// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Phase 3a U16's differential gate: `wr_cli::reg::reg`/`wr_cli::alphabet::alphabet_command`
//! against real `walnut-java` output, exercised through a real [`wr_cli::session::Session`]
//! rather than the raw `wr_core` primitives `tests/reg_brics_regex.rs` (U8) already checks.
//!
//! Unlike U8's own differential test (which drives `wr_core::regex` directly from an
//! already-parsed `Vec<Vec<i32>>` alphabet), this file's `reg` cases go through the
//! `listOfAlphabets` STRING-parsing layer `crate::alphabet::determine_alphabets_and_ns`
//! adds (`Alphabet.java:33-49`) -- the piece U16 actually contributes on top of U8's
//! already-shipped, already-reviewed regex engine. Since every one of U8's 60 captured
//! `reg` fixtures used the literal `{…}` set syntax (confirmed by that test's own doc
//! comment: `NS` is `None` for every entry), a representative subset is re-driven here
//! through the mechanically-reconstructed alphabet-declaration STRING (`{0,1} `, etc.)
//! that would have produced the same `Vec<Vec<i32>>` -- no new JVM capture needed for
//! that half. Two things U8's fixtures do NOT cover are captured fresh (see `CAPTURE.md`):
//! named number-system tokens (`msd_3`, `msd_2 lsd_2`) in a `reg` alphabet declaration,
//! and the `alphabet` command itself (source automaton -> `alphabet` -> result automaton).

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::alphabet::alphabet_command;
use wr_cli::reg::reg;
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;

fn fixture(sub: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{sub}/{name}.txt"))
}

/// Same scaffolding convention as `wr-cli`'s own tests (`session.rs`'s `temp_root`) --
/// a process-scoped directory under [`std::env::temp_dir`], laid out as a full Walnut
/// home tree so `Session::new` can resolve every library address.
fn temp_session(tag: &str) -> (Session, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("wr-differential-u16-{tag}-{}", std::process::id()));
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
    // `SessionPaths` does not append a trailing slash itself (Java's `Prover.parseArgs`
    // does that) -- supply one, matching `wr-cli`'s own test convention.
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    (session, dir)
}

/// Compares by semantic language equivalence (`CLAUDE.md`'s prime directive #1), not
/// structural/textual identity -- totalizing both sides first, since
/// `automaton_language_equivalent` requires total DFAs (mirrors
/// `tests/reg_brics_regex.rs`'s own comparison shape).
fn assert_equivalent_to_fixture(mut ours: Automaton, fixture_path: &Path, msg: &str) {
    let mut ground_truth = wr_io::reader::read_automaton_txt(fixture_path)
        .unwrap_or_else(|e| panic!("{msg}: fixture must parse cleanly, got {e:?}"));
    ours.fa.totalize(0);
    ground_truth.fa.totalize(0);
    assert_eq!(
        automaton_language_equivalent(&ours, &ground_truth),
        Ok(true),
        "{msg}: language must match real walnut-java's output"
    );
}

fn alphabet_declaration_string(alphabets: &[Vec<i32>]) -> String {
    let mut s = String::new();
    for track in alphabets {
        s.push('{');
        s.push_str(
            &track
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        s.push_str("} ");
    }
    s
}

// ---------------------------------------------------------------------------
// `reg`, through the string alphabet declaration
// ---------------------------------------------------------------------------

/// A representative subset of `tests/reg_brics_regex.rs`'s own 60-case corpus (same
/// fixture names, same `(alphabets, regex)` pairs -- see this file's module docs for why
/// re-driving them through the STRING layer needs no new capture), covering: single- and
/// multi-track declarations, intersection/negated-class/interval syntax, and both
/// WB-024 trigger shapes (`r19`, `r50`, `r58`).
#[allow(clippy::type_complexity)]
fn reg_corpus_subset() -> Vec<(&'static str, Vec<Vec<i32>>, &'static str)> {
    fn b2() -> Vec<Vec<i32>> {
        vec![vec![0, 1]]
    }
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
        ("r15", vec![vec![0, 1, 2]], "[0-1]*2"),
        ("r19", b2(), "2*"),
        ("r23", vec![vec![0, 1], vec![0, 1]], "[0,1][1,0]*"),
        ("r24", vec![vec![0, 1], vec![0, 1]], "([0,0]|[1,1])*"),
        ("r31", vec![vec![0, 1, 2]], "[0-2]"),
        ("r42", b2(), "[01][01]"),
        ("r50", vec![vec![0, 1, 2, 3], vec![0, 1]], "[9,9][0,0]"),
        ("r51", vec![vec![2, 4, 1]], "4*"),
        ("r52", vec![vec![1, 0]], "10*"),
        ("r58", b2(), "[10]"),
    ]
}

#[test]
fn reg_command_round_trips_through_the_string_alphabet_declaration() {
    let (session, dir) = temp_session("reg-corpus");
    for (name, alphabets, baseexp) in reg_corpus_subset() {
        let decl = alphabet_declaration_string(&alphabets);
        let tc = reg(&session, &decl, baseexp, name).unwrap_or_else(|e| {
            panic!("{name} (`{baseexp}`, decl `{decl}`) must build via wr_cli::reg::reg, got {e}")
        });
        let a = tc.automaton_pairs()[0].automaton().unwrap().clone();
        assert_equivalent_to_fixture(a, &fixture("reg", name), &format!("{name} (`{baseexp}`)"));
    }
    fs::remove_dir_all(&dir).ok();
}

/// Named number-system tokens in a `reg` alphabet declaration -- NOT exercised by
/// `tests/reg_brics_regex.rs`'s corpus (every one of its 60 fixtures uses a literal
/// `{…}` set). Captured fresh; see `CAPTURE.md`.
#[test]
fn reg_command_resolves_named_number_systems() {
    let (session, dir) = temp_session("reg-named-ns");
    let cases: [(&str, &str, &str); 3] = [
        ("u16_msd3", "msd_3 ", "0*1"),
        ("u16_mixed_ns", "msd_2 lsd_2 ", "[0,0][1,1]*"),
        ("u16_set", "{0,1,2} ", "1*"),
    ];
    for (name, decl, baseexp) in cases {
        let tc = reg(&session, decl, baseexp, name)
            .unwrap_or_else(|e| panic!("{name} (`{baseexp}`) must build, got {e}"));
        let a = tc.automaton_pairs()[0].automaton().unwrap().clone();
        assert_equivalent_to_fixture(a, &fixture("reg", name), name);
    }
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// `alphabet`
// ---------------------------------------------------------------------------

/// `alphabet baseC_restricted msd_2 $baseC;` against a real capture of `baseC` (a
/// one-track `{0,1,2,3}` automaton where every digit, including 2 and 3, has a real
/// outgoing transition -- see `CAPTURE.md`) -- exercises genuine transition PRUNING,
/// not just a header/metadata rewrite.
#[test]
fn alphabet_command_restricts_digits_and_prunes_transitions() {
    let (session, dir) = temp_session("alphabet-restrict");
    fs::copy(
        fixture("alphabet", "baseC"),
        dir.join("Automata Library").join("baseC.txt"),
    )
    .unwrap();

    let tc = alphabet_command(
        &session,
        "alphabet baseC_restricted msd_2 $baseC",
        Some("msd_2 "),
        false,
        "baseC.txt",
        "baseC_restricted",
    )
    .unwrap_or_else(|e| panic!("alphabet command must succeed, got {e}"));
    let a = tc.automaton_pairs()[0].automaton().unwrap().clone();
    assert_equivalent_to_fixture(
        a,
        &fixture("alphabet", "baseC_restricted"),
        "alphabet msd_2 restriction",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `alphabet baseB_asSet {0,1} {0,1} $baseB;` -- converts a two-track `msd_2 msd_2`
/// automaton to the equivalent literal-set alphabet (same digits, no number system),
/// exercising the `None`-NS / `all_reps` clearing path `set_alphabet` shares with `reg`.
#[test]
fn alphabet_command_converts_named_number_systems_to_a_literal_set() {
    let (session, dir) = temp_session("alphabet-asset");
    fs::copy(
        fixture("alphabet", "baseB"),
        dir.join("Automata Library").join("baseB.txt"),
    )
    .unwrap();

    let tc = alphabet_command(
        &session,
        "alphabet baseB_asSet {0,1} {0,1} $baseB",
        Some("{0,1} {0,1} "),
        false,
        "baseB.txt",
        "baseB_asSet",
    )
    .unwrap_or_else(|e| panic!("alphabet command must succeed, got {e}"));
    let a = tc.automaton_pairs()[0].automaton().unwrap().clone();
    assert_equivalent_to_fixture(
        a,
        &fixture("alphabet", "baseB_asSet"),
        "alphabet number-system-to-set conversion",
    );
    fs::remove_dir_all(&dir).ok();
}
