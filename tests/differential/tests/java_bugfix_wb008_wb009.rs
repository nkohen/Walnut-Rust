// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-008** and **WB-009** (`docs/WALNUT-BUGS.md`), the
//! `bugfix/wb-008-009` follow-up to `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-11
//! bundle. Checked against real `walnut-java` output **captured against the FIXED
//! branch** `bugfix/wb-008-009` (commit `b5d462b`, itself stacked on
//! `bugfix/wb-002-012-037-044`), **not mainline** — see `java_bugfix_wb002.rs`'s module
//! docs for why that's this project's `../CAPTURE.md` discipline for these follow-up
//! units, and the same note about re-pointing the commit reference once the branch
//! merges upstream.
//!
//! # `concat` of two hand-authored automata, designed to trigger BOTH bugs at once
//!
//! `FA.concatStates` used to (a) graft the second operand's transitions from its
//! state-index-0 rather than its real `q0` (WB-008), and (b) never un-mark the first
//! operand's own final states as accepting after grafting, leaking `L(first)` into what
//! should be exactly `L(first)·L(other)` whenever epsilon is not in `L(other)` (WB-009).
//! One query triggers both: `other`'s `q0` must be a state OTHER than index 0 (WB-008),
//! and `other` must not accept the empty string (WB-009).
//!
//! `wb008009a` (the first operand) accepts exactly the strings over `{0,1}` that end in
//! an ODD-length run of `1`s (state 1's `1`-transition goes back to state 0, not a
//! self-loop, so "11" is rejected — an earlier revision of this comment said "end in
//! `1`", which is wrong; caught by adversarial review of the sibling regression test in
//! `crates/wr-core/src/fa.rs`, corrected here without touching the fixture, since the
//! captured/asserted behavior below is still correct for the language as actually
//! built). `wb008009b` (the second operand) is hand-authored with its `q0` declared FIRST
//! in the file as state `2` (not `0`) — `AutomatonReader` sets `q0` to whichever state
//! is declared first, and since this automaton is already deterministic and total, no
//! auto-determinize/minimize on read renumbers it back to `q0 == 0` (see
//! `wr_io::reader`'s module docs: canonicalize only happens on WRITE, not on read of an
//! already-deterministic automaton) — and accepts exactly `Sigma+` (any nonempty
//! string): from `q0` (state `2`), any symbol moves to state `1` (accepting), which then
//! self-loops on everything. State `0` is an unreachable, non-accepting distractor (a
//! self-loop on both symbols) — present specifically so that WB-008's wrong graft target
//! (state INDEX `0`, not `other.q0`) is directly observable as a wrong transition rather
//! than merely a wrong final verdict.
//!
//! ## Capture recipe (reproducible)
//!
//! ```bash
//! cd ~/dev/walnut-java
//! git checkout bugfix/wb-008-009   # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Automata Library/wb008009a.txt" <<'EOF'
//! msd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 1
//!
//! 1 1
//! 0 -> 0
//! 1 -> 0
//! EOF
//! cat > "Automata Library/wb008009b.txt" <<'EOF'
//! msd_2
//!
//! 2 0
//! 0 -> 1
//! 1 -> 1
//!
//! 0 0
//! 0 -> 0
//! 1 -> 0
//!
//! 1 1
//! 0 -> 1
//! 1 -> 1
//! EOF
//! cat > "Command Files/wb008009_capture.txt" <<'EOF'
//! concat wb008009c wb008009a wb008009b;
//! EOF
//! java -jar target/Walnut-all.jar wb008009_capture.txt < /dev/null
//! ```
//!
//! Output (`Automata Library/wb008009c.txt`, captured 2026-08-20, `b5d462b`, run twice
//! independently — byte-identical both times):
//!
//! ```text
//! msd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 1
//!
//! 1 0
//! 0 -> 2
//! 1 -> 2
//!
//! 2 1
//! 0 -> 2
//! 1 -> 2
//! ```
//!
//! This is the correct, minimized `L(first)·L(other)`: `{ w : w contains a '1' not in
//! its last position }` (equivalently: state 0 loops on `0`, jumps to state 1 on the
//! first `1`; state 1 unconditionally advances to the accepting sink state 2 on the
//! NEXT symbol, whatever it is — i.e. once a `1` is seen with at least one more symbol
//! still to come, the word is accepted). Before `b5d462b` this would instead have
//! computed the wrong graft target (WB-008: splicing in state-index-0's self-loop
//! instead of `other.q0`'s real "any symbol -> accepting" transition) AND leaked
//! `L(first)` itself into the result unconditionally (WB-009) — see
//! `wr_core::fa::Fa::concat_states`'s doc comment (pre-fix revision, in this crate's
//! git history) for the exact wrong-answer shape this port used to faithfully reproduce.
//!
//! The command file and the two hand-authored automaton files were deleted from the
//! `walnut-java` checkout afterward, matching `../CAPTURE.md`'s established practice (no
//! `Session/<timestamp>/` directory to clean up here beyond the one `concat` itself
//! wrote to, which is also not part of that repo's tracked history).

use std::fs;
use std::io;
use std::path::PathBuf;

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::equiv::language_equivalent;
use wr_core::logging::Logging;

const WB008009A: &str = "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 0\n1 -> 0\n";
const WB008009B: &str =
    "msd_2\n\n2 0\n0 -> 1\n1 -> 1\n\n0 0\n0 -> 0\n1 -> 0\n\n1 1\n0 -> 1\n1 -> 1\n";

/// Real `walnut-java` output for `concat wb008009c wb008009a wb008009b;`, captured
/// against the fixed `bugfix/wb-008-009` branch (commit `b5d462b`) per this file's own
/// module docs.
const WB008009C_CAPTURED: &str =
    "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 0\n0 -> 2\n1 -> 2\n\n2 1\n0 -> 2\n1 -> 2\n";

/// A process-scoped Walnut home tree plus a `Prover` over it (output sunk — this test
/// only inspects the written `Automata Library/` file, not console/detail text).
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
    let logging = Logging::with_writers(Box::new(io::sink()), Box::new(io::sink()));
    let prover = Prover::with_output(session, logging, Box::new(io::sink()));
    (prover, dir)
}

/// WB-008 + WB-009 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit
/// `b5d462b` (branch `bugfix/wb-008-009`): `FA.concatStates` now grafts the second
/// operand's transitions from its real `q0` (not state-index-0), and un-marks the first
/// operand's own final states as accepting except when the second operand itself accepts
/// epsilon. `wr_core::fa::Fa::concat_states` used to reproduce both bugs verbatim; this
/// commit ported the matching fix. Confirms the fixed Rust `concat` output is
/// semantically equivalent to the fixed Java's real captured output, on an input pair
/// specifically designed to trigger both bugs in the same query.
#[test]
fn wb008_wb009_concat_matches_fixed_java() {
    let (mut p, dir) = prover("wb008009");
    fs::write(dir.join("Automata Library/wb008009a.txt"), WB008009A).unwrap();
    fs::write(dir.join("Automata Library/wb008009b.txt"), WB008009B).unwrap();

    p.dispatch("concat wb008009c wb008009a wb008009b;")
        .expect("concat succeeds");

    let ours = wr_io::reader::read_automaton_txt(dir.join("Automata Library/wb008009c.txt"))
        .expect("our wb008009c.txt must read back");

    let capture_path = dir.join("wb008009c_captured.txt");
    fs::write(&capture_path, WB008009C_CAPTURED).unwrap();
    let mut theirs =
        wr_io::reader::read_automaton_txt(&capture_path).expect("captured fixture must read back");

    let mut ours_fa = ours.fa.clone();
    ours_fa.totalize(0);
    theirs.fa.totalize(0);
    assert_eq!(
        language_equivalent(&ours_fa, &theirs.fa),
        Ok(true),
        "must be semantically equivalent to real walnut-java's fixed concat output \
         (captured against bugfix/wb-008-009, commit b5d462b)"
    );

    fs::remove_dir_all(&dir).ok();
}
