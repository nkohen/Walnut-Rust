// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-033** (`docs/WALNUT-BUGS.md`), one third of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-12 bundle (WB-013 + WB-033 + WB-034, one
//! shared root cause and one shared fix — see `../CAPTURE.md`'s entry for all three).
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-013-033-034` (commit `c75e630`), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's established practice for these
//! follow-up units, and the same note about re-pointing the commit reference once that
//! branch merges upstream.
//!
//! # `convert` on a track with no attached number system
//!
//! ## Capture recipe
//!
//! See `../CAPTURE.md`'s entry for this file (shared with `java_bugfix_wb013.rs`/
//! `java_bugfix_wb034.rs`). Summary: a one-track `{0,1}` automaton (no `msd_k`/`lsd_k`)
//! in `Automata Library/`, then `convert $wb033out msd_4 $wb033nsless;` (the `$` sigil on
//! BOTH names means "not a DFAO" — read/write the *plain* Automata Library, matching the
//! WB-033 entry's own trigger example).
//!
//! Output (captured 2026-08-21, `c75e630`):
//!
//! ```text
//! the automaton being converted has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
//! ```
//!
//! Before `c75e630` this crashed with a raw
//! `NullPointerException: Cannot invoke "Automata.NumberSystem.parseBase()" because "ns"
//! is null` instead. This port already raised a clean, recoverable
//! `ConvertNsError::NoNumberSystem` before this unit (never the raw Java crash, per
//! WB-013's established convention) — this fix is message-text-only, plus moving
//! `ConvertNsError::NoNumberSystem`'s `ProverError::is_handled()` classification out of
//! its own stale `false` arm into the general `ProverError::Convert(_) => true` bucket
//! (the exact shape `java_bugfix_wb037.rs`'s own fix took — see that file's module docs
//! for why asserting only `err.to_string()` cannot detect a stale classification bug).
//! `wb033out` is never written (the command errors out before writing anything), so
//! there is no result `.txt` fixture to capture — only the printed line.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated in
/// `java_bugfix_wb002.rs`/`java_bugfix_wb037.rs`/`java_bugfix_wb044.rs`).
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

/// WB-033 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `c75e630`
/// (branch `bugfix/wb-013-033-034`): `convertNS`'s unguarded `A.getNS().get(0)` used to
/// throw `NullPointerException` on an automaton whose track has no attached number
/// system; it now raises a clean `WalnutException` via the shared
/// `NumberSystem.requireNumberSystem` helper. This port already raised a clean
/// `Result::Err` here before this unit — the message text is now Java's fixed wording,
/// and (the fix this test actually pins, same shape as `wb037`'s) the error is now
/// classified as a handled `WalnutException`, so it renders message-only to stdout with
/// nothing on stderr, matching fixed Java exactly.
#[test]
fn wb033_convert_on_a_track_with_no_number_system_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb033");
    fs::write(
        dir.join("Automata Library/wb033nsless.txt"),
        "{0,1}\n\n0 1\n0 -> 0\n1 -> 0\n",
    )
    .unwrap();
    let mut input = io::Cursor::new(b"convert $wb033out msd_4 $wb033nsless;\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "convert $wb033out msd_4 $wb033nsless;\nthe automaton being converted has no \
         attached number system (its alphabet was declared explicitly, e.g. {0,1}, \
         rather than as msd_k/lsd_k)\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-013-033-034, commit c75e630) -- read_buffer's own echo of the command \
         line (console=false) precedes the printed message"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here means the error is still being classified as an unhandled JDK \
         exception (kind-prefixed rendering), the exact bug this test exists to catch"
    );
    assert!(
        !dir.join("Automata Library/wb033out.txt").exists(),
        "the command errors out before writing anything"
    );

    fs::remove_dir_all(&dir).ok();
}
