// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-044** (`docs/WALNUT-BUGS.md`), one of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-11 bundle. Checked against real
//! `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-002-012-037-044` (commit `d757221`), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's a first for this project's
//! `../CAPTURE.md` discipline, and the same note about re-pointing the commit reference
//! once that branch merges upstream.
//!
//! # `split`/`rsplit` on a TRUE/FALSE automaton
//!
//! ## Capture recipe
//!
//! ```bash
//! cd ~/dev/walnut-java   # bugfix/wb-002-012-037-044, already built: target/Walnut-all.jar
//! cat > "Automata Library/wb044t.txt" <<'EOF'
//! true
//! EOF
//! cat > "Command Files/wb044_capture.txt" <<'EOF'
//! split wb044out wb044t[+];
//! EOF
//! java -jar target/Walnut-all.jar wb044_capture.txt < /dev/null
//! ```
//!
//! Output (captured 2026-08-20, `d757221`):
//!
//! ```text
//! Cannot split automaton with no output values.
//! ```
//!
//! Before `d757221` this crashed with
//! `java.lang.IndexOutOfBoundsException: Index 0 out of bounds for length 0` instead;
//! this port faithfully reproduced that AS A PANIC (`Vec::remove`'s own out-of-bounds
//! panic), recovered by `Prover::caught` exactly the way Java's own top-level catch
//! recovers — see `docs/WALNUT-BUGS.md` WB-044's "Rust port" note (pre-fix) and
//! `crates/wr-cli/src/split.rs`'s git history. Now both engines raise a clean,
//! diagnosable error with the exact same text, before ever reaching that former panic
//! site. `wb044out` is never written (the command errors out before writing anything),
//! so there is no result `.txt` fixture to capture — only the printed line, exactly
//! like the two closed-formula cases in `../CAPTURE.md`'s `fixtures/lsd/` and
//! `fixtures/u11/` entries.
//!
//! # This file used to assert only `err.to_string()` — strengthened for the same reason
//! `java_bugfix_wb037.rs` was
//!
//! An adversarial review of this unit's sibling `java_bugfix_wb037.rs` found that
//! asserting only `err.to_string()` cannot detect a stale error-classification bug (that
//! file's message text was right, but the render CHANNEL was wrong). `SplitError`'s
//! classification (`ProverError::Split(_) => true`) was independently confirmed correct
//! for this bug during that same review, so there is no known defect here — but this
//! file now exercises the real rendering path the same way, both as defense-in-depth and
//! for consistency with its sibling.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated in
/// `java_bugfix_wb002.rs`/`java_bugfix_wb037.rs`).
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
/// AND `Logging`'s console writer — see `java_bugfix_wb037.rs`'s copy of this helper for
/// why they need to share one sink.
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

/// WB-044 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `d757221`
/// (branch `bugfix/wb-002-012-037-044`): `Split.processSplitCommand`'s unguarded
/// `subautomata.remove(0)` used to throw `IndexOutOfBoundsException` when splitting a
/// TRUE/FALSE automaton (empty output vector, so `uncombine` returns nothing); it now
/// raises a clean `WalnutException`. This port used to faithfully reproduce the crash AS
/// A PANIC recovered by `Prover::caught`; this unit added the matching guard, so it now
/// raises the same clean `Result::Err`, with Java's exact fixed message text, before
/// ever reaching that former panic site.
#[test]
fn wb044_split_on_a_true_false_automaton_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb044");
    fs::write(dir.join("Automata Library/wb044t.txt"), "true\n").unwrap();
    let mut input = io::Cursor::new(b"split wb044out wb044t[+];\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "split wb044out wb044t[+];\nCannot split automaton with no output values.\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-002-012-037-044, commit d757221) -- read_buffer's own echo of the \
         command line (console=false) precedes the printed message"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException"
    );
    assert!(
        !dir.join("Automata Library/wb044out.txt").exists(),
        "the command errors out before writing anything"
    );

    fs::remove_dir_all(&dir).ok();
}
