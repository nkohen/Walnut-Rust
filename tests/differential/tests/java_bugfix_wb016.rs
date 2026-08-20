// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-016** (`docs/WALNUT-BUGS.md`), the `bugfix/wb-016`
//! follow-up to `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s downstream port workflow.
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-016` (commit `d6e9799`, itself stacked on `bugfix/wb-008-009`, stacked on
//! `bugfix/wb-002-012-037-044`), **not mainline** — see `java_bugfix_wb002.rs`'s module
//! docs for why that's this project's `../CAPTURE.md` discipline for these follow-up
//! units, and the same note about re-pointing the commit reference once the branch
//! merges upstream.
//!
//! # `reverse` of a hand-authored word automaton (DFAO) whose `q0` isn't state `0`
//!
//! `WordAutomaton.reverseWithOutput` rebuilds a DFAO's states from scratch via BFS
//! (Theorem 4.3.3, Allouche & Shallit) and used to never assign the rebuilt automaton's
//! `q0`, leaving it at its STALE pre-reversal value — silently wrong (or out of bounds)
//! after the complete state renumbering, whenever the input's `q0` wasn't already `0`.
//! `wb016a` is hand-authored with its `q0` declared FIRST in the file as state `1` (not
//! `0`) — `AutomatonReader` sets `q0` to whichever state is declared first, and since
//! this automaton is already deterministic and total, no auto-determinize/minimize on
//! read renumbers it back to `q0 == 0` (see `wr_io::reader`'s module docs: canonicalize
//! only happens on WRITE, not on read of an already-deterministic automaton). It is the
//! same 2-state shape as `wr_core::word_automaton`'s own
//! `reverse_with_output_wb016_q0_is_correct_on_non_zero_initial_state` unit test: state
//! `1` (`q0`) has output `10` and transitions `0 -> 1, 1 -> 0`; state `0` has output `20`
//! and transitions `0 -> 0, 1 -> 1`.
//!
//! By Theorem 4.3.3, a DFAO's reversal evaluated at the empty string always equals the
//! original evaluated at the empty string (both are `""`, the reversal of itself) — so
//! the reversed automaton's initial state must have output equal to `wb016a`'s own
//! `O(q0) = O(1) = 10`. Before the fix this was `20` (WB-016's own catalog entry
//! empirically confirmed the identical shape against the real jar).
//!
//! ## Capture recipe (reproducible)
//!
//! Built in an isolated worktree, per this project's standing shared-checkout-safety
//! rule (`~/dev/walnut-java` may have other agents committing to it concurrently):
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add /tmp/walnut-java-wb016 bugfix/wb-016
//! cd /tmp/walnut-java-wb016
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Word Automata Library/wb016a.txt" <<'EOF'
//! msd_2
//!
//! 1 10
//! 0 -> 1
//! 1 -> 0
//!
//! 0 20
//! 0 -> 0
//! 1 -> 1
//! EOF
//! cat > "Command Files/wb016_capture.txt" <<'EOF'
//! reverse wb016b wb016a;
//! EOF
//! java -jar target/Walnut-all.jar wb016_capture.txt < /dev/null
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb016 --force
//! ```
//!
//! Output (`Word Automata Library/wb016b.txt`, captured 2026-08-20, `d6e9799`, run twice
//! independently — byte-identical both times):
//!
//! ```text
//! lsd_2
//!
//! 0 10
//! 0 -> 0
//! 1 -> 1
//!
//! 1 20
//! 0 -> 1
//! 1 -> 0
//! ```
//!
//! `q0` (state `0`, since `reverse` always canonicalizes on write) has output `10` — the
//! correct value per Theorem 4.3.3, confirming the fix. Before `d6e9799` this would
//! instead have been `20` (WB-016: the stale, un-updated `q0` pointing at the wrong
//! rebuilt state) — see `wr_core::word_automaton::reverse_with_output_with_ctx`'s doc
//! comment (pre-fix revision, in this crate's git history) for the exact wrong-answer
//! shape this port used to faithfully reproduce. The base flips `msd_2` -> `lsd_2`
//! because `reverse` always passes `reverseMsd = true` (`Reverse.reverseCommand`, ported
//! in `crate::reverse::reverse_command`).
//!
//! The command file and the hand-authored automaton file were deleted from the isolated
//! worktree afterward, matching `../CAPTURE.md`'s established practice.

use std::fs;
use std::path::PathBuf;

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

const WB016A: &str = "msd_2\n\n1 10\n0 -> 1\n1 -> 0\n\n0 20\n0 -> 0\n1 -> 1\n";

/// Real `walnut-java` output for `reverse wb016b wb016a;`, captured against the fixed
/// `bugfix/wb-016` branch (commit `d6e9799`) per this file's own module docs.
const WB016B_CAPTURED: &str = "lsd_2\n\n0 10\n0 -> 0\n1 -> 1\n\n1 20\n0 -> 1\n1 -> 0\n";

/// A process-scoped Walnut home tree plus a `Prover` over it (output sunk — this test
/// only inspects the written `Word Automata Library/` file, not console/detail text).
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

/// WB-016 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `d6e9799`
/// (branch `bugfix/wb-016`): `WordAutomaton.reverseWithOutput` now sets `q0` to the BFS
/// root (state `0`) immediately after the state-rebuild `setFields` call, instead of
/// leaving it at its stale pre-reversal value. `wr_core::word_automaton::
/// reverse_with_output_with_ctx` used to reproduce the bug verbatim; this commit ported
/// the matching fix (`word_a.fa.q0 = 0;` right after `set_fields`). Confirms the fixed
/// Rust `reverse` output is byte-identical to the fixed Java's real captured output, on
/// a hand-authored word automaton whose `q0` is declared as a non-zero state index —
/// exactly the input shape that made the bug observable (masked otherwise, since a
/// `determinizeAndMinimize`/canonicalize pipeline always leaves `q0 == 0`).
#[test]
fn wb016_reverse_matches_fixed_java() {
    let (mut p, dir) = prover("wb016");
    fs::write(dir.join("Word Automata Library/wb016a.txt"), WB016A).unwrap();

    p.dispatch("reverse wb016b wb016a;")
        .expect("reverse succeeds");

    let ours_text =
        fs::read_to_string(dir.join("Word Automata Library/wb016b.txt")).expect("wb016b.txt");
    assert_eq!(
        ours_text, WB016B_CAPTURED,
        "must be byte-identical to real walnut-java's fixed reverse output \
         (captured against bugfix/wb-016, commit d6e9799)"
    );

    // Also confirm the Theorem-4.3.3 property directly (not just "matches the fixture"),
    // the same shape of assertion `wr_core::word_automaton`'s own unit test uses.
    let ours = wr_io::reader::read_automaton_txt(dir.join("Word Automata Library/wb016b.txt"))
        .expect("our wb016b.txt must read back");
    assert_eq!(
        ours.fa.o[ours.fa.q0], 10,
        "reversed DFAO's initial-state output must equal wb016a's own O(q0) = O(1) = 10"
    );

    fs::remove_dir_all(&dir).ok();
}
