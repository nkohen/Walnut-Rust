// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-037** (`docs/WALNUT-BUGS.md`), one of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-11 bundle. Checked against real
//! `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-002-012-037-044` (commit `50636f4`), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's a first for this project's
//! `../CAPTURE.md` discipline, and the same note about re-pointing the commit reference
//! once that branch merges upstream.
//!
//! # `join` with zero automata specified
//!
//! ## Capture recipe
//!
//! ```bash
//! cd ~/dev/walnut-java   # bugfix/wb-002-012-037-044, already built: target/Walnut-all.jar
//! cat > "Command Files/wb037_capture.txt" <<'EOF'
//! join wb037out;
//! EOF
//! java -jar target/Walnut-all.jar wb037_capture.txt < /dev/null
//! ```
//!
//! Output (captured 2026-08-20, `50636f4`):
//!
//! ```text
//! Cannot join without any automata specified.
//! ```
//!
//! Before `50636f4` this crashed with
//! `java.lang.IndexOutOfBoundsException: Index 0 out of bounds for length 0` instead.
//! This port already raised a clean, recoverable `JoinError::NoAutomataSpecified` before
//! this unit (never the raw Java crash) — only the message TEXT changes here, from an
//! invented wording to Java's own fixed text, verbatim. `wb037out` is never written
//! (the command errors out before writing anything), so there is no result `.txt`
//! fixture to capture — only the printed line, exactly like the two closed-formula
//! cases in `../CAPTURE.md`'s `fixtures/lsd/` and `fixtures/u11/` entries.
//!
//! # This file used to assert only `err.to_string()` — that was the wrong observable
//!
//! An adversarial review of this unit found that asserting the internal `Display` string
//! from `Prover::dispatch`'s returned `Err` cannot detect a real classification bug: the
//! message text changed correctly, but the `ProverError::is_handled()` arm for this
//! variant was left stale (still `false`, "unhandled JDK exception"), so the port
//! actually rendered the new message on the WRONG channel — kind-prefixed
//! (`Main.WalnutException: …`) to stderr, instead of the plain line real (fixed) Walnut
//! prints to stdout. `err.to_string()` is identical either way, since it never goes
//! through `Logging::print_truncated_stack_trace_with_length`, the code that actually
//! decides the channel/prefix. This file now drives the command through
//! [`wr_cli::prover::Prover::read_buffer`] (the real rendering path `Prover::run`/the
//! CLI actually uses) and asserts BOTH streams, exactly as `java_bugfix_wb002.rs` already
//! did for its own (success-path) case.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated in
/// `java_bugfix_wb002.rs`/`java_bugfix_wb044.rs`; each captures a different stream, so a
/// shared helper crate felt like more machinery than three ~15-line structs warrant).
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

/// A process-scoped Walnut home tree plus a `Prover` over it, with `console`/`err`
/// standing in for real stdout/stderr. `console` backs BOTH `Prover`'s own `out` writer
/// AND `Logging`'s console writer — in real production (`Prover::new`) both are
/// `io::stdout()`, the same physical stream, so a command's own direct prints and
/// `Logging::print_truncated_stack_trace`'s rendering interleave on one stdout; sharing
/// one `Capture` here reproduces that merged view instead of splitting it into two
/// channels a real user's terminal never distinguishes.
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

/// WB-037 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `50636f4`
/// (branch `bugfix/wb-002-012-037-044`): `Join.joinCommand`'s unguarded
/// `subautomata.remove(0)` used to throw `IndexOutOfBoundsException` on `join <name>;`
/// with zero automata specified; it now raises a clean `WalnutException`. This port
/// already raised a clean `Result::Err` here before this unit — the message text is now
/// Java's fixed wording, and (the fix this test actually pins) the error is now
/// classified as a handled `WalnutException`, so it renders message-only to stdout with
/// nothing on stderr, matching fixed Java exactly.
#[test]
fn wb037_join_with_zero_automata_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb037");
    let mut input = io::Cursor::new(b"join wb037out;\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "join wb037out;\nCannot join without any automata specified.\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-002-012-037-044, commit 50636f4) -- read_buffer's own echo of the \
         command line (console=false) precedes the printed message"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here means the error is still being classified as an unhandled JDK \
         exception (kind-prefixed rendering), the exact bug this test exists to catch"
    );
    assert!(
        !dir.join("Automata Library/wb037out.txt").exists(),
        "the command errors out before writing anything"
    );

    fs::remove_dir_all(&dir).ok();
}
