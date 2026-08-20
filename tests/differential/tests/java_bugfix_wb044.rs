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
    let (mut p, dir) = prover("wb044");
    fs::write(dir.join("Automata Library/wb044t.txt"), "true\n").unwrap();

    let err = p.dispatch("split wb044out wb044t[+];").unwrap_err();

    assert_eq!(
        err.to_string(),
        "Cannot split automaton with no output values.",
        "must match real walnut-java's fixed error text verbatim (captured against \
         bugfix/wb-002-012-037-044, commit d757221)"
    );
}
