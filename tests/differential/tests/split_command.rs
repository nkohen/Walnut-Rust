// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **`split` / `rsplit`** (`Main/Commands/Split.java`), spot-checked
//! against real `walnut-java` CLI output — `docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`, Layer B.
//!
//! # Why this file exists, stated honestly
//!
//! Like `negative_base.rs`, this is **not** the primary evidence. That is Tier 1: the corpus
//! has 15 `split`/`rsplit` fixtures (8 `split` + 7 `rsplit`, ids 440-450 and 523-526),
//! covering all four sign combinations, the four round-trips, the `[]` null slot, and
//! 1-input DFAOs — and this port now replays all 15 green.
//!
//! What this file adds:
//!
//! * **The fast tier**, since `tests/golden` is `#[ignore]`d.
//! * **The non-DFAO operand**, which the corpus does not exercise on EITHER engine: every
//!   one of its 15 fixtures splits a word automaton (`T2`, `FTM`, `FASQ`), so
//!   `Split.java:26-30`'s Automata-Library branch is dead there. Java's own coverage
//!   measured that branch at zero before this unit added `SplitTest.java`. The cases below
//!   all split a plain `eval` result instead.
//! * **A CUSTOM-BASE operand** (`msd_fib`), which reaches
//!   `NumberSystem.setBaseChangeAutomaton`'s FILE branch — `determineNegativeNS("msd_fib")`
//!   is `msd_neg_fib`, and `Custom Bases/msd_neg_fib_base_change.txt` is the one
//!   base-change file Walnut ships, so this is the only way to exercise
//!   `loadAutomatonOrNull` for a base change at all. Every corpus fixture's operand is
//!   `msd_2`, so the corpus never reaches it either.
//! * **A round-trip through the real dispatch**, `split` then `rsplit` on the result, which
//!   is the only shape that can catch the two commands' three `reverse` ternaries being
//!   transcribed consistently-but-wrongly (a mirror-image pair round-trips just as well as
//!   the correct pair, so the round-trip alone is NOT sufficient — hence the per-command
//!   comparisons against captured Java output as well).
//!
//! **Result: no divergence.** All four cases below match real `walnut-java` exactly, on the
//! first run.
//!
//! # Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Command Files/spcap.txt" <<'EOF'
//! eval spsrc  "?msd_2 x < y";
//! split spout spsrc[+][-];
//! rsplit spback[+][-] spout;
//! split sponly spsrc[+][];
//! EOF
//! java -jar target/Walnut-all.jar spcap.txt < /dev/null
//! S="Session/<timestamp>/Automata Library"
//! cp "$S/spsrc.txt" "$S/spout.txt" "$S/spback.txt" "$S/sponly.txt" \
//!    .../fixtures/split/
//!
//! # …and, as a second run, the custom-base case:
//! cat > "Command Files/fibsplit.txt" <<'EOF'
//! eval fbsrc "?msd_fib x < y";
//! split fbout fbsrc[+][-];
//! EOF
//! java -jar target/Walnut-all.jar fibsplit.txt < /dev/null
//! cp "$S2/fbsrc.txt" "$S2/fbout.txt" .../fixtures/split/
//! # plus the six shipped Custom Bases files those two need, copied from walnut-java's own
//! # `Custom Bases/`: msd_fib{,_addition}.txt and
//! # msd_neg_fib{,_addition,_less_than,_base_change}.txt
//! ```
//!
//! `spsrc` is captured too, and is not decoration: it is what the port's own `eval` must
//! produce before `split` can be compared at all, so a divergence in the operand shows up
//! as its own failure rather than being blamed on `split`.
//!
//! The command file and session directory were deleted from the `walnut-java` checkout
//! afterward, matching every other recipe in `../CAPTURE.md`.
//!
//! # What these tests catch (mutation-verified, not asserted)
//!
//! Each mutation was really applied, run, and reverted — in an isolated copy of the tree,
//! so a concurrently-running reviewer's own test runs were never poisoned. The "caught by"
//! column is OBSERVED, not expected. `S&R` = `split_and_rsplit_match_real_walnut`,
//! `fib` = `split_over_a_custom_base_loads_its_shipped_base_change_file`,
//! `null` = `wr_cli::split`'s `process_split_skips_null_input_slots`,
//! `diff` = `wr_cli::split`'s `split_and_rsplit_are_different_computations`.
//!
//! ```text
//!                                                     S&R  fib  null diff
//! S1  flip the PLUS arm's `reverse` ternary            C    C    .    .
//! S2  flip the MINUS arm's bind ternary                C    C    .    .
//! S3  flip the MINUS arm's arithmetic ternary          C    C    .    .
//! S4  flip the base-change reverse polarity            C    .    .    .
//! S5  quantify the empty set instead of `quantifiers`  C    C    C    .
//! S6  treat the `[]` null slot as an error             .    .    C    .
//! ```
//!
//! Three things that matrix says, worth stating rather than leaving to be inferred:
//!
//! * **All three `reverse` ternaries are independently pinned** (S1-S3), which is what the
//!   module docs above claim and is the whole reason these cases compare against captured
//!   Java output per-command instead of relying on the `split`→`rsplit` round trip. A
//!   round trip alone cannot see a mirror-image pair of errors; a per-command comparison
//!   against real `walnut-java` can, and does.
//! * **S4 is invisible to the `fib` row, correctly**: `msd_neg_fib` LOADS its base-change
//!   automaton from `Custom Bases/msd_neg_fib_base_change.txt`, so the programmatic
//!   `baseNBaseChange`-then-reverse path never runs there. That is the file branch's whole
//!   point, and `wr_core::numsys`'s
//!   `a_supplied_base_change_file_is_used_instead_of_the_programmatic_construction` is its
//!   unit-level twin.
//! * **`split_and_rsplit_are_different_computations` catches NOTHING here**, and that is
//!   an honest weakness rather than an oversight: it compares the port against ITSELF with
//!   the flag flipped, so any mutation to shared code moves both sides together. It is
//!   kept as a cheap structural tripwire against `reverse` being dropped entirely (which
//!   would make the two identical), not as evidence of correctness — the evidence is the
//!   captured-Java rows.
//!
//! # Normalization
//!
//! `random_label()` is the last thing `processSplit` does (`Split.java:120`), relabelling
//! every track `"0"`, `"1"`, … in track order — so a captured `.txt` and the port's result
//! agree on labels by construction, and `sort_label()` is a no-op here rather than the
//! load-bearing normalization it is in `lsd_custom_base.rs`. `totalize(0)` is still needed:
//! real Walnut's automata are partial.

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/split")
}

/// A process-scoped Walnut home tree plus a `Prover` over it, with console output sunk.
fn prover(tag: &str) -> (Prover, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-split-{tag}-{}",
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
        "Command Files",
        "Transducer Library",
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    // The six shipped custom-base files the `msd_fib` case needs. Installed for every
    // session, not just that one: they are inert for the `msd_2` cases (nothing looks a
    // custom base up unless a header or a query names one), and installing them
    // unconditionally means a future case cannot silently depend on which helper it used.
    for name in [
        "msd_fib.txt",
        "msd_fib_addition.txt",
        "msd_neg_fib.txt",
        "msd_neg_fib_addition.txt",
        "msd_neg_fib_less_than.txt",
        "msd_neg_fib_base_change.txt",
    ] {
        fs::copy(
            fixtures_dir().join(name),
            dir.join("Custom Bases").join(name),
        )
        .unwrap_or_else(|e| panic!("must be able to install fixtures/split/{name}: {e}"));
    }
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    let logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    (
        Prover::with_output(session, logging, Box::new(std::io::sink())),
        dir,
    )
}

/// Compares the port's freshly written `Automata Library/<name>.txt` against the captured
/// `walnut-java` fixture of the same name, by semantic language equivalence plus the
/// structural facts a language-only comparison would miss.
fn compare(dir: &Path, name: &str, expected_tracks: usize) {
    // Both sides go through the CUSTOM-BASE reader, not the plain one: `msd_fib` needs it,
    // and `split`'s own output over a negative base is headed `msd_neg_2`. (That header
    // needs no file — see `wr_io::reader`'s `a_negative_base_header_needs_no_resolver` —
    // but reading both sides the same way is what keeps this comparison honest.)
    let custom_bases = dir.join("Custom Bases");
    let read = |p: PathBuf, what: &str| -> Automaton {
        wr_io::reader::read_automaton_txt_with_custom_bases(&p, &custom_bases)
            .unwrap_or_else(|e| panic!("{name}: {what} must parse: {e:?}"))
    };
    let mut ours = read(
        dir.join("Automata Library").join(format!("{name}.txt")),
        "the file the port wrote",
    );
    let mut theirs = read(
        fixtures_dir().join(format!("{name}.txt")),
        "the walnut-java fixture",
    );

    assert_eq!(theirs.alphabet.len(), expected_tracks, "{name}: fixture");
    assert_eq!(ours.alphabet.len(), expected_tracks, "{name}: ours");
    assert_eq!(ours.alphabet, theirs.alphabet, "{name}: alphabets");
    assert_eq!(
        ours.track_ns_names(),
        theirs.track_ns_names(),
        "{name}: per-track number systems"
    );
    ours.fa.totalize(0);
    theirs.fa.totalize(0);
    assert!(
        automaton_language_equivalent(&ours, &theirs).expect("total DFAs after totalize"),
        "{name}: diverges from real walnut-java"
    );
}

/// The whole captured session, replayed through the port's real `Prover::dispatch`.
///
/// Run as ONE test rather than four because the commands are genuinely sequential — the
/// operand `split` reads is `eval`'s output, and `rsplit`'s operand is `split`'s — so
/// splitting them would either duplicate the setup or silently share state between tests.
#[test]
fn split_and_rsplit_match_real_walnut() {
    let (mut p, dir) = prover("session");

    // 1. The operand. A plain automaton in the AUTOMATA library, so `split` takes the
    //    `isDFAO == false` branch no golden fixture reaches.
    p.dispatch("eval spsrc \"?msd_2 x < y\";").unwrap();
    compare(&dir, "spsrc", 2);
    assert!(
        !dir.join("Word Automata Library/spsrc.txt").is_file(),
        "the operand must NOT be in the word library, or the wrong branch is exercised"
    );

    // 2. `split`, with both signs on the two tracks.
    p.dispatch("split spout spsrc[+][-];").unwrap();
    compare(&dir, "spout", 2);

    // 3. `rsplit` back over the result — note the operand is the FOURTH regex group for
    //    `rsplit` and the second for `split` (`GROUP_RSPLIT_AUTOMATA = 4`), so getting the
    //    indices wrong here fails as "automaton does not exist", not as a divergence.
    p.dispatch("rsplit spback[+][-] spout;").unwrap();
    compare(&dir, "spback", 2);

    // 4. The `[]` null slot: track 0 is converted, track 1 is passed through untouched.
    p.dispatch("split sponly spsrc[+][];").unwrap();
    compare(&dir, "sponly", 2);

    fs::remove_dir_all(&dir).ok();
}

/// The custom-base operand: `msd_fib`, whose negative twin `msd_neg_fib` has a SHIPPED
/// `Custom Bases/msd_neg_fib_base_change.txt`. This is the only case in the whole test
/// suite — this port's or Java's — that reaches `setBaseChangeAutomaton`'s file branch
/// (`NumberSystem.java:453`) rather than its programmatic `baseNBaseChange` one, because
/// every `split` fixture in the golden corpus operates on an `msd_2` automaton.
///
/// It also pins that the base-change automaton's OWN header (`msd_fib msd_neg_fib`)
/// resolves through the session's custom-base loader — a file whose two tracks are two
/// different number systems, one of them negative.
#[test]
fn split_over_a_custom_base_loads_its_shipped_base_change_file() {
    let (mut p, dir) = prover("fib");
    p.dispatch("eval fbsrc \"?msd_fib x < y\";").unwrap();
    compare(&dir, "fbsrc", 2);
    p.dispatch("split fbout fbsrc[+][-];").unwrap();
    compare(&dir, "fbout", 2);
    // The result really is still over the custom base, not the `msd_2` its `{0,1}`
    // alphabet alone would suggest.
    let out = wr_io::reader::read_automaton_txt_with_custom_bases(
        dir.join("Automata Library/fbout.txt"),
        &dir.join("Custom Bases"),
    )
    .unwrap();
    assert_eq!(
        out.track_ns_names(),
        vec![Some("msd_fib".to_string()), Some("msd_fib".to_string())]
    );
    // …and that really is the CUSTOM base's restriction, not `msd_2`'s absence of one:
    // `msd_fib.txt` is the Zeckendorf "no two adjacent 1s" automaton.
    assert!(out.all_reps.iter().all(Option::is_some));
    fs::remove_dir_all(&dir).ok();
}

/// `Split.java:32` through the real dispatch, for both commands — the one error case the
/// corpus does cover (fixture 671, `split NONEXISTENT NONEXISTENT [+] [-] []`), here
/// checked for `rsplit` too, whose different group indices make it a genuinely separate
/// path.
#[test]
fn a_missing_operand_reports_walnuts_own_message_through_dispatch() {
    let (mut p, dir) = prover("missing");
    assert_eq!(
        p.dispatch("split out nope[+];").unwrap_err().to_string(),
        "Automaton nope does not exist."
    );
    assert_eq!(
        p.dispatch("rsplit out[+] nope;").unwrap_err().to_string(),
        "Automaton nope does not exist."
    );
    fs::remove_dir_all(&dir).ok();
}
