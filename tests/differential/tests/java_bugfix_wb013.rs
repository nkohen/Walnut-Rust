// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-013** (`docs/WALNUT-BUGS.md`), one third of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-12 bundle (WB-013 + WB-033 + WB-034, one
//! shared root cause and one shared fix — see `../CAPTURE.md`'s entry for all three).
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-013-033-034` (commit `c75e630`), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's established practice for these
//! follow-up units, and the same note about re-pointing the commit reference once that
//! branch merges upstream.
//!
//! # `eval`/`def` indexing a `{...}`-declared track with a repeated variable
//!
//! This is the first time WB-013 has ever been driven through a real command — its
//! original `docs/WALNUT-BUGS.md` entry (Phase 3a, U2) explicitly noted it was "not yet
//! wired to any real caller," a claim `docs/WALNUT-BUGS.md`'s own update for this unit
//! corrects: `Word`/`Function` token construction landed in Phase 3a's U4, well before
//! this PR, so `T[i][i]`-style repeated-variable indexing has been reachable from
//! `eval`/`def` for most of this project's history — this is simply the first time it was
//! *tested* end-to-end.
//!
//! ## Capture recipe
//!
//! See `../CAPTURE.md`'s entry for this file (shared with `java_bugfix_wb033.rs`/
//! `java_bugfix_wb034.rs`). Summary: a two-track word automaton declared `msd_2 {0,1}` —
//! track 0 is a real number system, track 1 (indexed by the repeated variable `i` below)
//! is not — then `eval wb013out "wb013T[i][i] = @1";`.
//!
//! Output (captured 2026-08-21, `c75e630`), printed TWICE per `EvalDef.compute`'s own
//! catch-log-then-rethrow shape (`Logging.printTruncatedStackTrace(e)` on the original
//! exception, then a second `WalnutException` wrapping `message + "\n\t: char at " +
//! t.getPositionInPredicate()`, caught again by `Prover.dispatch`'s own top-level catch —
//! this double-print shape is generic to any `act()` failure, not specific to this bug,
//! and this port's `wr_logic::eval::compute` already reproduces it, per that module's own
//! docs):
//!
//! ```text
//! the track indexed by the repeated variable i in wb013T has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
//! the track indexed by the repeated variable i in wb013T has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
//! <TAB>: char at 0
//! ```
//! (that last line's leading whitespace is one literal tab character, `\t`, written above
//! as the literal string `<TAB>` since clippy's `tabs_in_doc_comments` lint rejects a
//! real tab inside a doc comment.)
//!
//! Before `c75e630` this crashed with a raw
//! `NullPointerException: Cannot read field "equality" because "ns" is null` instead
//! (printed the same way, twice). This port already raised a clean, recoverable
//! `ExprError::RepeatedIdentifierMissingNumberSystem` before this unit (never the raw
//! Java crash, per WB-013's own established convention) — this fix is message-text-only,
//! plus moving `RepeatedIdentifierMissingNumberSystem`'s `ActError::is_handled()`
//! classification out of its own stale `false` arm to `true` (the exact shape
//! `java_bugfix_wb037.rs`'s own fix took for `JoinError::NoAutomataSpecified` — see that
//! file's module docs for why asserting only `err.to_string()` cannot detect a stale
//! classification bug). `wb013out` is never written (the command errors out before
//! writing anything), so there is no result `.txt` fixture to capture — only the printed
//! lines.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated in
/// `java_bugfix_wb002.rs`/`java_bugfix_wb033.rs`/`java_bugfix_wb034.rs`/
/// `java_bugfix_wb037.rs`/`java_bugfix_wb044.rs`).
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
/// standing in for real stdout/stderr. See `java_bugfix_wb037.rs`'s copy of this helper
/// for why they need to share one sink.
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

/// WB-013 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `c75e630`
/// (branch `bugfix/wb-013-033-034`): `VariableExpression.act`'s repeated-identifier
/// branch used to throw `NullPointerException` when the SAME variable indexes a track
/// with no attached number system; it now raises a clean `WalnutException` via the
/// shared `NumberSystem.requireNumberSystem` helper. This port already raised a clean
/// `Result::Err` here before this unit — the message text is now Java's fixed wording,
/// and (the fix this test actually pins) the error is now classified as a handled
/// `WalnutException`, so it renders message-only to stdout with nothing on stderr,
/// matching fixed Java exactly, including the double-print/`": char at N"` wrapping
/// `EvalDef.compute`'s own catch-log-then-rethrow applies to every `act()` failure.
#[test]
fn wb013_repeated_variable_indexing_a_track_with_no_number_system_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb013");
    fs::write(
        dir.join("Word Automata Library/wb013T.txt"),
        "msd_2 {0,1}\n\n0 0\n0 0 -> 0\n0 1 -> 0\n1 0 -> 0\n1 1 -> 0\n",
    )
    .unwrap();
    let mut input = io::Cursor::new(b"eval wb013out \"wb013T[i][i] = @1\";\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "eval wb013out \"wb013T[i][i] = @1\";\n\
         the track indexed by the repeated variable i in wb013T has no attached number \
         system (its alphabet was declared explicitly, e.g. {0,1}, rather than as \
         msd_k/lsd_k)\n\
         the track indexed by the repeated variable i in wb013T has no attached number \
         system (its alphabet was declared explicitly, e.g. {0,1}, rather than as \
         msd_k/lsd_k)\n\
         \t: char at 0\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-013-033-034, commit c75e630) -- read_buffer's own echo of the command \
         line (console=false) precedes the printed message, and EvalDef.compute's own \
         catch-log-then-rethrow prints the message twice (see this module's docs)"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here means the error is still being classified as an unhandled JDK \
         exception (kind-prefixed rendering), the exact bug this test exists to catch"
    );
    assert!(
        !dir.join("Automata Library/wb013out.txt").exists(),
        "the command errors out before writing anything"
    );

    fs::remove_dir_all(&dir).ok();
}
