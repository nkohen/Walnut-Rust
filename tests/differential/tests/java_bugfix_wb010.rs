// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-010** (`docs/WALNUT-BUGS.md`), the `bugfix/wb-010`
//! follow-up to `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s downstream port workflow.
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-010` (commit `c5ff914`, stacked on `bugfix/wb-016`, stacked on
//! `bugfix/wb-008-009`, stacked on `bugfix/wb-002-012-037-044`), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's this project's `../CAPTURE.md`
//! discipline for these follow-up units, and the same note about re-pointing the commit
//! reference once the branch merges upstream.
//!
//! # `leftQuotient`'s alphabet-subset guard ran backwards
//!
//! `AutomatonLogicalOps.leftQuotient` checked "`A`'s alphabet ⊆ `B`'s" before delegating
//! to `rightQuotient(reverse(A), reverse(B), skipSubsetCheck=true)` — but that delegated
//! call re-encodes `B`'s transition symbols under `A`'s alphabet, which needs the
//! OPPOSITE containment (`B` ⊆ `A`). Two symptoms, both covered below: a proper-subset
//! pair in the WRONG direction (`A` over `{0,1}`, `B` over `{0,1,2}`) that the old check
//! wrongly let through and that used to crash deep inside the cross product, now cleanly
//! rejected; and a genuinely valid pair (`A` over `{0,1,2}`, `B` over `{0,1}`) that the
//! old check wrongly rejected, now computing the correct result.
//!
//! ## Capture recipe (reproducible)
//!
//! Built in an isolated worktree, per this project's standing shared-checkout-safety
//! rule (`~/dev/walnut-java` may have other agents committing to it concurrently):
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add /tmp/walnut-java-wb010 bugfix/wb-010
//! cd /tmp/walnut-java-wb010
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! # -- Case 1: the WB-010 trigger shape (now cleanly rejected) --
//! cat > "Automata Library/wb010a.txt" <<'EOF'
//! msd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 0
//! EOF
//! cat > "Automata Library/wb010b.txt" <<'EOF'
//! msd_3
//!
//! 0 0
//! 0 -> 0
//! 1 -> 0
//! 2 -> 0
//! EOF
//! cat > "Command Files/wb010_capture.txt" <<'EOF'
//! leftquo wb010c wb010a wb010b;
//! EOF
//! java -jar target/Walnut-all.jar wb010_capture.txt < /dev/null
//!
//! # -- Case 2: the previously wrongly-rejected valid pair (now computes correctly) --
//! # A over {0,1,2}, L(A) = {"1", "00", "01"}; B over {0,1}, L(B) = {"0"}.
//! cat > "Automata Library/wb010d.txt" <<'EOF'
//! msd_3
//!
//! 0 0
//! 0 -> 1
//! 1 -> 2
//!
//! 1 0
//! 0 -> 3
//! 1 -> 4
//!
//! 2 1
//!
//! 3 1
//!
//! 4 1
//! EOF
//! cat > "Automata Library/wb010e.txt" <<'EOF'
//! msd_2
//!
//! 0 0
//! 0 -> 1
//!
//! 1 1
//! EOF
//! cat > "Command Files/wb010_capture2.txt" <<'EOF'
//! leftquo wb010f wb010d wb010e;
//! EOF
//! java -jar target/Walnut-all.jar wb010_capture2.txt < /dev/null
//!
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb010 --force
//! ```
//!
//! `java`/`mvnw` above actually ran under a JDK 17+ toolchain
//! (`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1`, selected explicitly
//! since the shell's default `java` resolves to a JDK 11 too old for this project's
//! class file version) — noted here since it's a prerequisite this recipe silently
//! assumes otherwise.
//!
//! ## Case 1 output (captured 2026-08-20, `c5ff914`, run twice independently — identical
//! both times)
//!
//! ```text
//! leftquo wb010c wb010a wb010b;
//! Second A's alphabet must be a subset of the first A's alphabet for left quotient.
//! ```
//!
//! Nothing is written to `Automata Library/wb010c.txt`, and the session survives (the
//! REPL prompt follows). Before `c5ff914` this pair instead crashed deep inside the
//! cross product with a corrupted symbol id (`RichAlphabet.encode`'s silent
//! `indexOf == -1`) — WB-010's own catalog entry has the pre-fix empirical detail.
//!
//! ## Case 2 output (`Session/<timestamp>/Automata Library/wb010f.txt`, captured
//! 2026-08-20, `c5ff914`)
//!
//! ```text
//! msd_3
//!
//! 0 0
//! 0 -> 1
//! 1 -> 1
//!
//! 1 1
//! ```
//!
//! A minimized 2-state automaton: state `0` (non-accepting, `q0`) reads a `0` or `1` to
//! state `1` (accepting, no outgoing edges — so any further digit, or a repeated `0`/`1`,
//! goes nowhere and is rejected). That is exactly `L(B) \ L(A) = {"0", "1"}` — before the
//! fix, this pair was instead wrongly REJECTED outright (the old guard required `A ⊆ B`
//! as a set, which is false here since `A`'s alphabet `{0,1,2}` is strictly larger).
//!
//! The command files and hand-authored automaton files were deleted from the isolated
//! worktree afterward, matching `../CAPTURE.md`'s established practice.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture` (duplicated across this file family; each captures different streams, so a
/// shared helper crate has felt like more machinery than these ~15-line structs warrant
/// — see `java_bugfix_wb037.rs`'s matching note).
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
    }
}

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

const WB010A: &str = "msd_2\n\n0 0\n0 -> 0\n1 -> 0\n";
const WB010B: &str = "msd_3\n\n0 0\n0 -> 0\n1 -> 0\n2 -> 0\n";
const WB010D: &str = "msd_3\n\n0 0\n0 -> 1\n1 -> 2\n\n1 0\n0 -> 3\n1 -> 4\n\n2 1\n\n3 1\n\n4 1\n";
const WB010E: &str = "msd_2\n\n0 0\n0 -> 1\n\n1 1\n";

/// Real `walnut-java` fixed output for Case 2 (`leftquo wb010f wb010d wb010e;`),
/// captured against `bugfix/wb-010` (commit `c5ff914`) per this file's own module docs.
const WB010F_CAPTURED: &str = "msd_3\n\n0 0\n0 -> 1\n1 -> 1\n\n1 1\n";

/// A process-scoped Walnut home tree plus a `Prover` over it, with `console`/`err`
/// standing in for real stdout/stderr — same shape as `java_bugfix_wb037.rs`'s `prover`
/// helper (see its doc comment for why `console` backs both `Prover`'s own `out` writer
/// and `Logging`'s console writer).
fn prover(tag: &str) -> (Prover, Capture, Capture, PathBuf) {
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
    let console = Capture::default();
    let err = Capture::default();
    let logging = Logging::with_writers(Box::new(console.clone()), Box::new(err.clone()));
    (
        Prover::with_output(session, logging, Box::new(console.clone())),
        console,
        err,
        dir,
    )
}

/// WB-010 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `c5ff914`
/// (branch `bugfix/wb-010`): `leftQuotient`'s alphabet-subset guard now checks the
/// correct direction (`B`'s alphabet ⊆ `A`'s), so `A` over `{0,1}`/`B` over `{0,1,2}` —
/// the shape the old, backwards guard let through to an unguarded internal re-encode —
/// is now rejected CLEANLY, before any of the quotient machinery runs.
/// `wr_core::logicalops::left_quotient` used to reproduce the bug verbatim (as a clean
/// panic rather than Java's silent corruption, per this crate's pre-existing
/// out-of-alphabet-digit guard on `Automaton::encode`); this commit ported the matching
/// fix. Drives the command through [`wr_cli::prover::Prover::read_buffer`] (the real
/// rendering path, per `java_bugfix_wb037.rs`'s established practice) rather than just
/// `dispatch`, so a stale `QuotientError::is_walnut_exception()` classification would be
/// caught too (it renders correctly here, but a fix that only handled the guard while
/// leaving the classification stale would show up as a non-empty `err` stream).
#[test]
fn wb010_leftquo_trigger_shape_rejected_matching_fixed_java() {
    let (mut p, console, err, dir) = prover("wb010-case1");
    fs::write(dir.join("Automata Library/wb010a.txt"), WB010A).unwrap();
    fs::write(dir.join("Automata Library/wb010b.txt"), WB010B).unwrap();

    let mut input = io::Cursor::new(b"leftquo wb010c wb010a wb010b;\n".to_vec());
    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "leftquo wb010c wb010a wb010b;\n\
         Second A's alphabet must be a subset of the first A's alphabet for left quotient.\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-010, commit c5ff914) -- read_buffer's own echo of the command line \
         (console=false) precedes the printed message"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here would mean the error is being classified as an unhandled JDK \
         exception (kind-prefixed rendering) instead of the clean message-only path"
    );
    assert!(
        !dir.join("Automata Library/wb010c.txt").exists(),
        "the command errors out before writing anything, on both engines"
    );

    fs::remove_dir_all(&dir).ok();
}

/// WB-010's other symptom, also fixed by `c5ff914`: before the fix, `A` over
/// `{0,1,2}`/`B` over `{0,1}` (a genuinely valid pair -- `B`'s alphabet really is a
/// subset of `A`'s) was wrongly REJECTED, because the old (backwards) guard demanded the
/// OPPOSITE containment (`A ⊆ B`), which is false whenever the alphabets aren't equal as
/// sets. After the fix this pair is accepted and computes the textbook left quotient.
/// Confirms the fixed Rust `leftquo` command's output is byte-identical to real
/// (fixed) `walnut-java`'s captured output, AND independently re-derives the expected
/// language from `wr_core::logicalops::left_quotient`'s own definition (not just trusting
/// the fixture matches), the same double-check
/// `crates/wr-core/src/logicalops.rs`'s hand-built pin for this exact pair uses.
#[test]
fn wb010_leftquo_previously_rejected_valid_pair_now_computes_correctly() {
    let (mut p, _console, _err, dir) = prover("wb010-case2");
    fs::write(dir.join("Automata Library/wb010d.txt"), WB010D).unwrap();
    fs::write(dir.join("Automata Library/wb010e.txt"), WB010E).unwrap();

    p.dispatch("leftquo wb010f wb010d wb010e;")
        .expect("leftquo succeeds now that B's alphabet genuinely is a subset of A's");

    let ours_text =
        fs::read_to_string(dir.join("Automata Library/wb010f.txt")).expect("wb010f.txt");
    assert_eq!(
        ours_text, WB010F_CAPTURED,
        "must be byte-identical to real walnut-java's fixed leftquo output (captured \
         against bugfix/wb-010, commit c5ff914)"
    );

    // Independent re-derivation, not just "matches the fixture": L(B) \ L(A) =
    // { z : exists w in L(B), wz in L(A) } = { z : "0"+z in L(A) }. L(A) = {"1", "00",
    // "01"}, so "0"+"" = "0" (not in L(A)), "0"+"0" = "00" (in L(A)) => "0" accepted,
    // "0"+"1" = "01" (in L(A)) => "1" accepted.
    let ours = wr_io::reader::read_automaton_txt(dir.join("Automata Library/wb010f.txt"))
        .expect("our wb010f.txt must read back");
    assert!(
        !ours.fa.accepts_word(&[]),
        "empty string should be rejected"
    );
    assert!(ours.fa.accepts_word(&[0]), "\"0\" should be accepted");
    assert!(ours.fa.accepts_word(&[1]), "\"1\" should be accepted");
    assert!(!ours.fa.accepts_word(&[2]), "\"2\" should be rejected");
    assert!(!ours.fa.accepts_word(&[0, 0]), "\"00\" should be rejected");
    assert!(!ours.fa.accepts_word(&[0, 1]), "\"01\" should be rejected");

    fs::remove_dir_all(&dir).ok();
}
