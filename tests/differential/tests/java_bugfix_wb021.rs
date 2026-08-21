// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-021** (`docs/WALNUT-BUGS.md`), the `bugfix/wb-021`
//! follow-up to `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s downstream port workflow.
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-021` (commit `c0d7fff`, stacked on `bugfix/wb-032`), **not mainline** —
//! see `java_bugfix_wb002.rs`'s module docs for why that's this project's
//! `../CAPTURE.md` discipline for these follow-up units.
//!
//! # `exportToBA` had no `TRUE_FALSE_AUTOMATON` guard, unlike its two siblings
//!
//! `AutomatonWriter.exportToBA` used to fall straight through to `FAtoCompactNFA` on a
//! trivial TRUE/FALSE automaton's meaningless/stale `Q`/alphabet/transition-table
//! fields, unlike `writeToTxtFormat`/`writeToGV` which both special-case
//! `isTRUE_FALSE_AUTOMATON()` first. The result: BOTH the TRUE and the FALSE automaton
//! silently exported to the same `.ba` bytes (`"0\n"`), with no way to tell which one a
//! `.ba` file had come from. The fix (`walnut-java` commit `c0d7fff`) adds the same
//! guard: TRUE now writes a single accepting sentinel state (`"0\n"`, byte-identical to
//! the old, buggy output — TRUE's export is unchanged), FALSE now writes a genuine
//! 0-byte file (the actual behavior change).
//!
//! ## Capture recipe (reproducible)
//!
//! Built in an isolated worktree, per this project's standing shared-checkout-safety
//! rule (`~/dev/walnut-java` may have other agents committing to it concurrently) — and
//! note `bugfix/wb-021` was ALREADY checked out at the main `~/dev/walnut-java` working
//! tree when this was captured, so the worktree below is added by commit hash
//! (detached), not by branch name, to avoid git's "branch already checked out" refusal
//! (the same situation `java_bugfix_wb032.rs`'s own recipe hit):
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb021 c0d7fff
//! cd /tmp/walnut-java-wb021
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! cat > "Command Files/wb021_capture.txt" <<'EOF'
//! eval wb021true "?msd_2 Ex x = 1";
//! eval wb021false "?msd_2 Ex (x = 1 & x = 2)";
//! export $wb021true BA;
//! export $wb021false BA;
//! EOF
//! java -jar target/Walnut-all.jar wb021_capture.txt < /dev/null
//!
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb021 --force
//! ```
//!
//! `java`/`mvnw` above actually ran under a JDK 17+ toolchain
//! (`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1`, selected explicitly
//! since the shell's default `java` resolves to a JDK 11 too old for this project's
//! class file version) — noted here since it's a prerequisite this recipe silently
//! assumes otherwise, per `java_bugfix_wb010.rs`'s own note.
//!
//! Two `?msd_2 Ex …` queries with no free variables were used to get real TRUE/FALSE
//! `Automaton`s out of the real `eval` command (rather than hand-authoring a `true`/
//! `false` library file, which real Walnut's own `eval` output console confirmed:
//! `eval wb021true "?msd_2 Ex x = 1";` prints `TRUE`, `eval wb021false "?msd_2 Ex (x = 1
//! & x = 2)";` prints `FALSE` — an unsatisfiable conjunction under one `∃`).
//!
//! ## Output (captured 2026-08-20, `c0d7fff`)
//!
//! `Session/<timestamp>/Result/wb021true.ba`: 2 bytes, `"0\n"` — copied byte-for-byte
//! into `crates/wr-io/tests/fixtures/writer_true.ba` (that fixture's own re-capture,
//! since it is the exact TRUE-automaton `.ba` shape `crates/wr-io/src/writer.rs`'s own
//! unit tests already compare against, now re-verified against the fixed jar rather
//! than mainline's — unchanged from the pre-fix bytes, confirming TRUE's export did not
//! change).
//!
//! `Session/<timestamp>/Result/wb021false.ba`: **0 bytes** — copied byte-for-byte into
//! `crates/wr-io/tests/fixtures/writer_false.ba`, replacing what used to be the same
//! `"0\n"` as the TRUE fixture. This is the actual fix: before it, `writer_true.ba` and
//! `writer_false.ba` were byte-identical; now they differ, and FALSE's `.ba` export
//! carries the standard empty-language representation (no initial-state line, since
//! `BAWriter` only writes one when there is exactly one state; no transitions; no
//! final-states section, vacuously "all states accept" over zero states).
//!
//! The command file was deleted from the isolated worktree afterward, matching every
//! recipe above.

use std::fs;
use std::path::PathBuf;

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// Real `walnut-java` fixed output for `export $wb021true BA;` — captured against
/// `bugfix/wb-021` (commit `c0d7fff`) per this file's own module docs. Byte-identical to
/// the PRE-fix output too (TRUE's export is unchanged by the fix).
const WB021_TRUE_BA: &[u8] = b"0\n";

/// Real `walnut-java` fixed output for `export $wb021false BA;` — captured against
/// `bugfix/wb-021` (commit `c0d7fff`). Genuinely empty; PRE-fix this used to be
/// byte-identical to [`WB021_TRUE_BA`] (`"0\n"`), which was the bug.
const WB021_FALSE_BA: &[u8] = b"";

/// A process-scoped Walnut home tree plus a `Prover` over it — same shape as
/// `java_bugfix_wb016.rs`'s own `prover` helper (output sunk — this test only inspects
/// the written `Result/*.ba` files, not console/detail text).
fn prover(tag: &str) -> (Prover, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-javabugfix-{tag}-{}",
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
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    let logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let prover = Prover::with_output(session, logging, Box::new(std::io::sink()));
    (prover, dir)
}

/// WB-021 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `c0d7fff`
/// (branch `bugfix/wb-021`): `AutomatonWriter.exportToBA` now checks
/// `isTRUE_FALSE_AUTOMATON()` first instead of falling through to `FAtoCompactNFA` on a
/// trivial automaton's stale fields. Runs the fix end-to-end through the real `eval`
/// (to produce a genuine TRUE automaton) and `export … BA` CLI commands — not just a
/// direct call to `wr_io::writer::export_to_ba` (that primitive already has its own
/// unit-level pins in `crates/wr-io/src/writer.rs`;
/// `ba_matches_real_walnut_output_for_true_automaton_wb021`/
/// `ba_matches_real_walnut_output_for_false_automaton_wb021`).
///
/// This file compares by BYTE IDENTITY rather than `wr_core::equiv`'s semantic
/// language-equivalence oracle — a deliberate exception per `CLAUDE.md`'s own carve-out
/// ("semantic equivalence... is about automaton LANGUAGE comparison"), not an
/// oversight: `.ba` is a serialization FORMAT, and the entire bug is about that format's
/// BYTES being unable to distinguish two different automata, not about whether two
/// automata accept the same language (`java_bugfix_wb016.rs`'s module docs make the
/// identical argument for a DFAO-output-comparison case; this repo's existing
/// `crates/wr-io/src/writer.rs::tests::ba_matches_real_walnut_output_for_*` tests
/// already establish byte-comparison as this module's own precedent for `.ba` output).
#[test]
fn wb021_export_ba_distinguishes_true_from_false() {
    let (mut p, dir) = prover("wb021");

    p.dispatch("eval wb021true \"?msd_2 Ex x = 1\";")
        .expect("eval of a satisfiable closed formula must succeed");
    p.dispatch("eval wb021false \"?msd_2 Ex (x = 1 & x = 2)\";")
        .expect("eval of an unsatisfiable closed formula must succeed (result is FALSE)");

    p.dispatch("export $wb021true BA;")
        .expect("export of the TRUE automaton to .ba must succeed now that WB-021 is fixed");
    p.dispatch("export $wb021false BA;")
        .expect("export of the FALSE automaton to .ba must succeed now that WB-021 is fixed");

    let true_ba = fs::read(dir.join("Result/wb021true.ba")).expect("wb021true.ba");
    let false_ba = fs::read(dir.join("Result/wb021false.ba")).expect("wb021false.ba");

    assert_eq!(
        true_ba, WB021_TRUE_BA,
        "TRUE's .ba export must be byte-identical to real walnut-java's fixed output \
         (captured against bugfix/wb-021, commit c0d7fff) -- and to the pre-fix output, \
         since the fix leaves TRUE's export unchanged"
    );
    assert_eq!(
        false_ba, WB021_FALSE_BA,
        "FALSE's .ba export must be byte-identical to real walnut-java's fixed output \
         (captured against bugfix/wb-021, commit c0d7fff) -- a genuine 0-byte file"
    );
    assert!(
        false_ba.is_empty(),
        "WB-021 fixed: FALSE's .ba export must be empty"
    );
    assert_ne!(
        true_ba, false_ba,
        "WB-021 fixed: TRUE and FALSE must no longer be indistinguishable in .ba output \
         -- if this ever fails, WB-021's fix needs re-verifying against a fresh real-jar \
         run, not this test relaxed back to the pre-fix behavior"
    );

    fs::remove_dir_all(&dir).ok();
}
