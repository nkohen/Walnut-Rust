// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for the **`ost` command** (Ostrowski numeration), checked
//! against a real `walnut-java` CLI capture — the port of
//! `Main/Commands/Ost.java` + `Automata/Numeration/Ostrowski.java`.
//!
//! # What this adds over the other tiers
//!
//! * **Tier 2** (`wr_cli::ost`'s own test module) replicates `OstrowskiTest.java`: eight
//!   byte-for-byte comparisons against the `.txt` files Java's unit tests compare
//!   against. Those fixtures are the SHIPPED number systems (`msd_fib`/`msd_pell`/…),
//!   none of which reaches the `preperiod[0] == 1` rotation branch through its *single*
//!   -digit sub-case, and none of which is driven through `Prover::dispatch`.
//! * **Tier 1** (`tests/golden`, gated-slow) replays exactly one `ost` fixture, id 625
//!   (`ost test625 [0 3 1] [1 2];`), whose pre-period starts with 3 — so it does not
//!   exercise either rotation branch at all.
//! * **Tier 3** (`tests/differential-gen`) never emits `ost` (it generates `eval`
//!   queries only).
//!
//! So the genuinely new coverage here is: the whole command through the real dispatch
//! path; both `preperiod[0] == 1` rotation sub-branches; and — the part no other tier
//! checks at all — a **follow-up query over the freshly created base**, i.e. that the
//! two files `ost` writes are actually loadable by `wr_core::numsys`'s custom-base
//! resolver and compute the same language real Walnut computes over the same base.
//!
//! # Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Command Files/ost_capture.txt" <<'EOF'
//! ost o [1 2] [3];
//! ost rotsingle [] [1];
//! ost numsys2 [0 3 1] [1 2];
//! eval ostq1 "?msd_o Ex x+x=y";
//! eval ostq2 "?msd_rotsingle Ax (x<3) => (x+1>x)";
//! eval ostq3 "?msd_numsys2 Ex,y x+y=z & x=y";
//! EOF
//! java -jar target/Walnut-all.jar ost_capture.txt < /dev/null
//! S="Session/<timestamp>"
//! cp "$S/Custom Bases/"*.txt .../fixtures/ostrowski/
//! cp "$S/Result/ostq1.txt" "$S/Result/ostq2.txt" "$S/Result/ostq3.txt" \
//!    .../fixtures/ostrowski/
//! ```
//!
//! The command file and `Session/<timestamp>/` directory were deleted from the
//! `walnut-java` checkout afterward, matching `../CAPTURE.md`'s existing recipes. The
//! capture was run twice, independently, and every one of the nine files came out
//! byte-identical both times.
//!
//! The three `ost` commands cover the constructor's three distinct pre-period shapes:
//!
//! | command | pre-period after `removeLeadingZeros` | branch taken |
//! |---|---|---|
//! | `ost o [1 2] [3];` | `[1, 2]` | `Ostrowski.java:105-107` — multi-digit rotation |
//! | `ost rotsingle [] [1];` | `[]` → copy-filled `[1]` | `:109-111` — single-digit rotation |
//! | `ost numsys2 [0 3 1] [1 2];` | `[3, 1]` | no rotation (fixture 625's shape) |
//!
//! # Result: no divergence
//!
//! All nine captured files matched on the first run, with no production change. The
//! written `.txt` files are compared **byte-for-byte** (not just semantically): this
//! command's entire observable output is two files, and a re-canonicalized-but-equivalent
//! automaton would be a real divergence a `wr_core::equiv` comparison could never see.
//! The follow-up queries' results are compared by `wr_core::equiv` semantic equivalence,
//! which is `CLAUDE.md`'s bar for a computed automaton.

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ostrowski")
}

/// A process-scoped Walnut home tree with an EMPTY `Custom Bases/` — `ost` has to create
/// everything it later reads.
fn temp_prover(tag: &str) -> (Prover, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-ostrowski-{tag}-{}",
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
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    let logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let prover = Prover::with_output(session, logging, Box::new(std::io::sink()));
    (prover, dir)
}

/// Byte-for-byte, against the captured `walnut-java` file of the same name.
fn assert_written_file_matches_capture(dir: &Path, name: &str) {
    let ours = fs::read_to_string(dir.join("Custom Bases").join(name))
        .unwrap_or_else(|e| panic!("{name} must have been written: {e}"));
    let theirs = fs::read_to_string(fixtures().join(name))
        .unwrap_or_else(|e| panic!("capture {name} must exist: {e}"));
    assert_eq!(theirs, ours, "{name} differs from real walnut-java");
}

/// Semantic equivalence of an `Automata Library/` result against the captured one, with
/// the two normalizations every differential file in this crate uses (`sort_label`
/// because Java's writer canonizes, `totalize` because Walnut's `.txt` automata are
/// partial).
fn assert_result_matches_capture(prover: &Prover, dir: &Path, name: &str) {
    let paths = prover.session().paths();
    let mut ours = wr_io::reader::read_automaton_txt_with_custom_base_resolver(
        dir.join("Automata Library").join(format!("{name}.txt")),
        paths,
    )
    .unwrap_or_else(|e| panic!("our {name}.txt must read back: {e:?}"));
    let mut theirs = wr_io::reader::read_automaton_txt_with_custom_base_resolver(
        fixtures().join(format!("{name}.txt")),
        paths,
    )
    .unwrap_or_else(|e| panic!("captured {name}.txt must read back: {e:?}"));

    assert_eq!(
        ours.is_true_false_automaton(),
        theirs.is_true_false_automaton(),
        "{name}: one side collapsed to a TRUE/FALSE automaton and the other did not"
    );
    if theirs.is_true_false_automaton() {
        assert_eq!(
            ours.is_true_automaton(),
            theirs.is_true_automaton(),
            "{name}: opposite truth verdicts"
        );
        return;
    }

    assert_eq!(theirs.alphabet, ours.alphabet, "{name}: alphabet");
    assert_eq!(
        theirs.track_ns_names(),
        ours.track_ns_names(),
        "{name}: per-track number-system names"
    );
    ours.sort_label();
    ours.fa.totalize(0);
    theirs.fa.totalize(0);
    assert!(
        automaton_language_equivalent(&ours, &theirs).expect("comparable"),
        "{name}: language differs from real walnut-java"
    );
}

/// `ost o [1 2] [3];` — the multi-digit rotation branch (`Ostrowski.java:105-107`),
/// then `?msd_o Ex x+x=y` over the base it just created.
#[test]
fn multi_digit_rotation_and_a_query_over_the_new_base() {
    let (mut prover, dir) = temp_prover("rotmulti");
    assert!(prover.dispatch("ost o [1 2] [3];").expect("ost succeeds"));
    assert_written_file_matches_capture(&dir, "msd_o.txt");
    assert_written_file_matches_capture(&dir, "msd_o_addition.txt");

    assert!(prover
        .dispatch("eval ostq1 \"?msd_o Ex x+x=y\";")
        .expect("eval over the new base succeeds"));
    assert_result_matches_capture(&prover, &dir, "ostq1");

    fs::remove_dir_all(&dir).ok();
}

/// `ost rotsingle [] [1];` — the single-digit rotation branch
/// (`Ostrowski.java:109-111`), the one `OstrowskiTest.java` had no coverage for at all
/// before this unit's Phase-0 pass added
/// `createOstrowskiSinglePreperiodDigitOneRotatesFromPeriod`. The follow-up is a CLOSED
/// formula, so it also checks the TRUE/FALSE collapse path over a custom base.
#[test]
fn single_digit_rotation_and_a_closed_query_over_the_new_base() {
    let (mut prover, dir) = temp_prover("rotsingle");
    assert!(prover
        .dispatch("ost rotsingle [] [1];")
        .expect("ost succeeds"));
    assert_written_file_matches_capture(&dir, "msd_rotsingle.txt");
    assert_written_file_matches_capture(&dir, "msd_rotsingle_addition.txt");

    assert!(prover
        .dispatch("eval ostq2 \"?msd_rotsingle Ax (x<3) => (x+1>x)\";")
        .expect("eval over the new base succeeds"));
    assert_result_matches_capture(&prover, &dir, "ostq2");
    // The capture's own verdict, spelled out rather than only compared: real Walnut
    // printed TRUE for this one.
    assert_eq!(
        fs::read_to_string(fixtures().join("ostq2.txt")).unwrap(),
        "true"
    );

    fs::remove_dir_all(&dir).ok();
}

/// `ost numsys2 [0 3 1] [1 2];` — golden fixture 625's own arguments, under a different
/// name, plus a three-track follow-up query the corpus does not have.
#[test]
fn a_preperiodic_system_and_a_three_track_query_over_the_new_base() {
    let (mut prover, dir) = temp_prover("numsys2");
    assert!(prover
        .dispatch("ost numsys2 [0 3 1] [1 2];")
        .expect("ost succeeds"));
    assert_written_file_matches_capture(&dir, "msd_numsys2.txt");
    assert_written_file_matches_capture(&dir, "msd_numsys2_addition.txt");

    assert!(prover
        .dispatch("eval ostq3 \"?msd_numsys2 Ex,y x+y=z & x=y\";")
        .expect("eval over the new base succeeds"));
    assert_result_matches_capture(&prover, &dir, "ostq3");

    fs::remove_dir_all(&dir).ok();
}

/// Three `ost` commands in one session, each creating its own base, then a query that
/// mixes nothing — the point is only that the second and third `ost` do not disturb the
/// first's files (the BFS state is reset per `initAutomaton` call, and `Ostrowski` is a
/// fresh object per command).
#[test]
fn three_systems_in_one_session_do_not_interfere() {
    let (mut prover, dir) = temp_prover("three");
    for cmd in [
        "ost o [1 2] [3];",
        "ost rotsingle [] [1];",
        "ost numsys2 [0 3 1] [1 2];",
    ] {
        assert!(prover
            .dispatch(cmd)
            .unwrap_or_else(|e| panic!("{cmd}: {e}")));
    }
    for name in [
        "msd_o.txt",
        "msd_o_addition.txt",
        "msd_rotsingle.txt",
        "msd_rotsingle_addition.txt",
        "msd_numsys2.txt",
        "msd_numsys2_addition.txt",
    ] {
        assert_written_file_matches_capture(&dir, name);
    }
    fs::remove_dir_all(&dir).ok();
}
