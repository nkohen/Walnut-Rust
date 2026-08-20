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

use std::fs;
use std::path::PathBuf;

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A process-scoped Walnut home tree plus a `Prover` over it, console output sunk
/// (this file only ever inspects the returned `Err`'s message text, never stdout).
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
    (
        Prover::with_output(session, logging, Box::new(std::io::sink())),
        dir,
    )
}

/// WB-037 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `50636f4`
/// (branch `bugfix/wb-002-012-037-044`): `Join.joinCommand`'s unguarded
/// `subautomata.remove(0)` used to throw `IndexOutOfBoundsException` on `join <name>;`
/// with zero automata specified; it now raises a clean `WalnutException`. This port
/// already raised a clean `Result::Err` here before this unit — only the message text
/// is new, now matching Java's fixed wording verbatim instead of this port's own
/// previously-invented text.
#[test]
fn wb037_join_with_zero_automata_matches_fixed_java() {
    let (mut p, _dir) = prover("wb037");

    let err = p.dispatch("join wb037out;").unwrap_err();

    assert_eq!(
        err.to_string(),
        "Cannot join without any automata specified.",
        "must match real walnut-java's fixed error text verbatim (captured against \
         bugfix/wb-002-012-037-044, commit 50636f4)"
    );
}
