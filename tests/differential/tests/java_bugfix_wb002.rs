// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-002** (`docs/WALNUT-BUGS.md`), one of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-11 bundle. Checked against real
//! `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-002-012-037-044` (commit `aa4a241`), **not mainline** — a first for this
//! project's `../CAPTURE.md` discipline, which has always captured against mainline
//! behavior (buggy or not) until now. Whoever revisits this file once that branch merges
//! upstream should re-point the commit reference below at the merged sha, but the
//! captured TEXT itself does not change: it is what the fix actually made Java print,
//! which is exactly what this port now needs to match too.
//!
//! # A degenerate single-state, non-accepting, self-looping automaton
//!
//! No `reg`/`eval` query reliably produces this exact minimized shape (Trimmer/minimize
//! never leave a *non-accepting* self-loop as the sole surviving state on any input this
//! project's other fixtures happen to generate), so — same discipline as
//! `../CAPTURE.md`'s `u16r03`/`baseB`/etc. entries that seed a source automaton by hand —
//! the trigger automaton is hand-written directly into `Automata Library/` before `inf`
//! runs on it, both here and in the real capture below.
//!
//! ## Capture recipe (reproducible, against the FIXED branch)
//!
//! ```bash
//! cd ~/dev/walnut-java
//! git checkout bugfix/wb-002-012-037-044   # already built: target/Walnut-all.jar
//! cat > "Automata Library/wb002trigger.txt" <<'EOF'
//! msd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 0
//! EOF
//! cat > "Command Files/wb002_capture.txt" <<'EOF'
//! inf wb002trigger;
//! EOF
//! java -jar target/Walnut-all.jar wb002_capture.txt < /dev/null
//! ```
//!
//! Output (captured 2026-08-20, `aa4a241`):
//!
//! ```text
//! Automaton wb002trigger accepts finitely many values.
//! ```
//!
//! Before `aa4a241` this crashed with `java.lang.NullPointerException` instead (this
//! port's own pre-fix behavior — `InfiniteError::DegenerateSelfLoop`, a recoverable
//! `Result::Err` rather than a panic — is described in `wr_core::infinite`'s module docs
//! and git history). The command file and the hand-written automaton file were deleted
//! from the `walnut-java` checkout afterward, per `../CAPTURE.md`'s established practice
//! (no `Session/<timestamp>/` directory to clean up here: `inf` writes no automaton file,
//! only the one printed line).

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable stdout sink — same shape as `wr_cli::prover`'s own private
/// test-module `Capture`, duplicated here since that one isn't exported.
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

/// A process-scoped Walnut home tree plus a `Prover` over it, with a `Capture` standing
/// in for real stdout (so the printed verdict line can be asserted on) and `Logging`'s
/// own writers sunk (nothing in this file inspects detail text).
fn prover(tag: &str) -> (Prover, Capture, PathBuf) {
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
    let capture = Capture::default();
    (
        Prover::with_output(session, logging, Box::new(capture.clone())),
        capture,
        dir,
    )
}

/// WB-002 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `aa4a241`
/// (branch `bugfix/wb-002-012-037-044`): `Infinite.infinite` used to throw
/// `NullPointerException` on a single-state, non-accepting, self-looping automaton;
/// it now cleanly reports the language as finite. `wr_core::infinite::infinite` used to
/// reproduce the crash as `Err(InfiniteError::DegenerateSelfLoop)`; this unit removed
/// that guard (see that module's docs, "Porting the fix: deleting a guard, not adding
/// one") so it now answers `None` (finite) for the exact same shape, matching the fixed
/// Java exactly.
#[test]
fn wb002_degenerate_self_loop_automaton_matches_fixed_java() {
    let (mut p, out, dir) = prover("wb002");
    fs::write(
        dir.join("Automata Library/wb002trigger.txt"),
        "msd_2\n\n0 0\n0 -> 0\n1 -> 0\n",
    )
    .unwrap();

    p.dispatch("inf wb002trigger;").unwrap();

    assert_eq!(
        out.text(),
        "Automaton wb002trigger accepts finitely many values.\n",
        "must match real walnut-java's fixed output verbatim (captured against \
         bugfix/wb-002-012-037-044, commit aa4a241)"
    );

    fs::remove_dir_all(&dir).ok();
}
