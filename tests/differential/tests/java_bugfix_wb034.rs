// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-034** (`docs/WALNUT-BUGS.md`), one third of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-12 bundle (WB-013 + WB-033 + WB-034, one
//! shared root cause and one shared fix — see `../CAPTURE.md`'s entry for all three).
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-013-033-034` (commit `c75e630`), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's established practice for these
//! follow-up units, and the same note about re-pointing the commit reference once that
//! branch merges upstream.
//!
//! # `transduce` on an input track with no attached number system
//!
//! ## Capture recipe
//!
//! See `../CAPTURE.md`'s entry for this file (shared with `java_bugfix_wb013.rs`/
//! `java_bugfix_wb033.rs`). Summary: a two-track (Thue-Morse-shaped) `{0,1}` word
//! automaton in `Word Automata Library/` (no `msd_k`/`lsd_k`), transduced through the
//! repo's own shipped `Transducer Library/RUNSUM2.txt`: `transduce wb034out RUNSUM2
//! wb034nsless;`.
//!
//! Output (captured 2026-08-21, `c75e630`):
//!
//! ```text
//! the automaton being transduced has no attached number system (its alphabet was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)
//! ```
//!
//! Before `c75e630` this crashed with a raw
//! `NullPointerException: Cannot invoke "Automata.NumberSystem.isMsd()" because the
//! return value of "java.util.List.get(int)" is null` instead. This port already raised
//! a clean, recoverable `TransduceError::NoNumberSystem` before this unit (never the raw
//! Java crash, per WB-013's established convention) — this fix is message-text-only.
//!
//! `wr_cli::prover`'s `LoggableError for ProverError` already classified this variant as a
//! handled `WalnutException`, and that stayed correct once Java's own exception became
//! real — so unlike `wb033`'s sibling fix, no `is_handled()` change was needed for WB-034
//! specifically. This test still drives the real `read_buffer` ->
//! `print_truncated_stack_trace` rendering path (not just `err.to_string()`) and asserts
//! the fixed jar's exact stdout/stderr split, both because that is the only way to
//! actually confirm the classification is (still) correct, and for consistency with its
//! `wb033`/`wb037` siblings.
//!
//! What that arm used to be, and no longer is: a single UNCONDITIONAL
//! `ProverError::Transduce(_) => true` covering every `TransduceError`. Adversarial review
//! of this fix's own diff found that bucket also swept in `NoTransducerTransition` and
//! `NoTransducerOutput`, which port `Transducer.createMap`'s and the `sigma` unboxing's
//! genuinely still-unfixed raw `NullPointerException`s (a separate open Java defect, NOT
//! part of WB-035 — WB-035 *is* fixed upstream, `7f54eff`, and did not touch them).
//! Reproduced live and fixed in the same pass; see `docs/WALNUT-BUGS.md`'s WB-034 entry and
//! `wr_cli::prover`'s own arm for the full account. Nothing about WB-034's own verdict
//! changed as a result.
//!
//! `wb034out` is never written (the command errors out before writing anything), so
//! there is no result `.txt` fixture to capture — only the printed line.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// Same shape (and the same shipped `RUNSUM2` fixture text) as
/// `wr_cli::transduce`'s own private test module.
const RUNSUM2_TXT: &str = "{0, 1}\n\n0\n0 -> 0 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 1\n1 -> 0 / 0\n";

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated in
/// `java_bugfix_wb002.rs`/`java_bugfix_wb033.rs`/`java_bugfix_wb037.rs`/
/// `java_bugfix_wb044.rs`).
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

/// WB-034 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `c75e630`
/// (branch `bugfix/wb-013-033-034`): `transduceNonDeterministic`'s unguarded
/// `M.getNS().get(0)` used to throw `NullPointerException` on an input word automaton
/// whose track has no attached number system; it now raises a clean `WalnutException`
/// via the shared `NumberSystem.requireNumberSystem` helper. This port already raised a
/// clean `Result::Err` here before this unit — only the message text changes, to Java's
/// fixed wording.
#[test]
fn wb034_transduce_on_a_track_with_no_number_system_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb034");
    fs::write(dir.join("Transducer Library/RUNSUM2.txt"), RUNSUM2_TXT).unwrap();
    fs::write(
        dir.join("Word Automata Library/wb034nsless.txt"),
        "{0,1}\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n",
    )
    .unwrap();
    let mut input = io::Cursor::new(b"transduce wb034out RUNSUM2 wb034nsless;\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "transduce wb034out RUNSUM2 wb034nsless;\nthe automaton being transduced has no \
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
         stderr here would mean the error is being classified as an unhandled JDK \
         exception (kind-prefixed rendering)"
    );
    assert!(
        !dir.join("Word Automata Library/wb034out.txt").exists(),
        "the command errors out before writing anything"
    );

    fs::remove_dir_all(&dir).ok();
}
