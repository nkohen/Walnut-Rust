// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-014** (`docs/WALNUT-BUGS.md`) — `docs/
//! WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-14. Unlike every other file in this
//! `java_bugfix_*` family, this is a **divergence-closed** unit, not a
//! **port-bug-fixed** one: no `wr-core`/`wr-logic`/`wr-io`/`wr-cli` production code changes
//! alongside this test. See `docs/WALNUT-BUGS.md` WB-014's own entry for the full account;
//! summarized here for this file's own context.
//!
//! # What WB-014 was, and why the port never had it
//!
//! Real (pre-fix) `walnut-java`'s `NumberSystem.getComputeIfAbsent` was
//! `numberSystemHash.computeIfAbsent(base, NumberSystem::new)`. Loading a **custom base**
//! whose `_addition.txt` header declares its alphabet with a number-system TOKEN
//! (`msd_2 msd_2 msd_2`) rather than an explicit set (`{0,1} {0,1} {0,1}`) makes the
//! `NumberSystem` constructor re-enter `getComputeIfAbsent` for `"msd_2"` WHILE the outer
//! `computeIfAbsent` for the custom base is still running — a structural modification of the
//! map from inside its own mapping-function callback, which `HashMap#computeIfAbsent` (since
//! JDK 9) detects and reports as a bare, undiagnosable `ConcurrentModificationException`.
//!
//! `wr_core::numsys::NumberSystem::with_custom_base_files` takes already-parsed automata and
//! performs no I/O, so its constructor cannot re-enter a name→`NumberSystem` cache the way
//! Java's constructor does — the cache lives entirely outside `wr-core`, in
//! `wr_cli::session::SessionPaths`/`PredicateEnv`'s memoized lookups, behind a `RefCell`
//! whose borrow is released before construction runs. So this port was **always**
//! architecturally immune to this specific crash, not by having been ported around it — see
//! `wr_io::reader::read_automaton_txt_with_custom_bases`'s own module docs (the "Recursion
//! and `docs/WALNUT-BUGS.md` WB-014" section), which already state this. Before this unit
//! that immunity had never actually been exercised end-to-end through the real CLI dispatch
//! path against a real cross-referencing-header custom base file — searched this repo for
//! `msd_wrtest`/`wrtest`/similar shapes before writing this file and found none — only
//! asserted architecturally. This file closes that coverage gap.
//!
//! `walnut-java` commit `6580f71` (branch `bugfix/wb-014`, stacked on `bugfix/wb-011`)
//! fixes the crash on the Java side, by a different mechanism (an explicit `get`/
//! construct-if-absent/`put` sequence, reentrancy-safe because a nested call for a
//! DIFFERENT base populates the map via its own separate `put`, which
//! `computeIfAbsent`'s structural-modification detector never sees). So **both engines now
//! succeed** on this input — this file is what confirms they agree, not just that neither
//! one crashes.
//!
//! # Capture recipe (reproducible)
//!
//! `bugfix/wb-014` was already checked out at the main `~/dev/walnut-java` working tree
//! when this was captured (the same situation `java_bugfix_wb021.rs`/
//! `java_bugfix_wb032.rs`/`java_bugfix_wb035.rs`/`java_bugfix_wb038.rs` hit), so the
//! worktree below is added by commit hash (detached), not by branch name, to avoid git's
//! "branch already checked out" refusal:
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb014 6580f71
//! cd /tmp/walnut-java-wb014
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! # WALNUT-BUGS.md's exact minimal repro (adapted only in the base's name):
//! printf 'msd_2 msd_2 msd_2\n\n0 1\n0 0 0 -> 0\n' > "Custom Bases/msd_wrtest_addition.txt"
//!
//! cat > "Command Files/wb014_capture.txt" <<'EOF'
//! eval wrtest1 "?msd_wrtest x=x";
//! eval wrtest3 "?msd_wrtest Ex x=x";
//! EOF
//! java -cp target/Walnut-all.jar Main.Prover wb014_capture.txt \
//!     >stdout.txt 2>stderr.txt </dev/null
//!
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb014 --force
//! ```
//!
//! `java`/`mvnw` above actually ran under a JDK 17+ toolchain
//! (`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1/Contents/Home`) — the
//! shell's default `java` resolves to a JDK 11 too old for this project's class file
//! version. `</dev/null` matters: without it the process runs the command file and then
//! blocks in the interactive REPL.
//!
//! `stderr.txt` was empty (no `ConcurrentModificationException`, no anything). `stdout.txt`
//! (captured 2026-08-22, up to the REPL banner that follows the command file):
//!
//! ```text
//! eval wrtest1 "?msd_wrtest x=x";
//! eval wrtest3 "?msd_wrtest Ex x=x";
//! ____
//! TRUE
//! ```
//!
//! `Session/<timestamp>/Automata Library/wrtest1.txt` (copied byte-for-byte below as
//! [`WRTEST1_CAPTURED`], the same "inline a small captured fixture directly" convention
//! `java_bugfix_wb032.rs`'s Case 2 and `java_bugfix_wb035.rs`'s dead-letter cases use for a
//! result this short — 4 content lines):
//!
//! ```text
//! msd_wrtest
//!
//! 0 1
//! 0 -> 0
//! 1 -> 0
//! ```
//!
//! The one-state, output-1, self-looping-on-every-digit shape is exactly `x=x`: TRUE for
//! every `x`, over an alphabet with no reachable dead state, so it collapses to one state
//! under Valmari/Brzozowski minimization on both engines. `wrtest3.txt` (the closed
//! `Ex x=x` case) was also written by both engines but is not captured as a fixture, per
//! this project's established convention for a trivial closed-formula result
//! (`../CAPTURE.md`'s `fixtures/u11/`/`fixtures/lsd/` entries) — the printed `TRUE` verdict
//! is the meaningful observable, checked directly against
//! [`wr_core::fa::Fa::is_true_automaton`] below.
//!
//! This port's own output for `wrtest1` (checked live against a release build before this
//! file was written, confirming the "architecturally immune" claim rather than trusting
//! it) is **byte-identical** to [`WRTEST1_CAPTURED`] — asserted directly below, alongside
//! the semantic-equivalence check [`CLAUDE.md`'s Prime Directive](../../../CLAUDE.md) calls
//! for as the default comparison. Byte identity isn't the point of this file (unlike
//! `java_bugfix_wb021.rs`, which is genuinely about writer fidelity) — it happens to hold
//! here because the automaton is a 1-state trivial case with nothing left for a
//! canonicalization difference to act on, so it costs nothing extra to check.
//!
//! The command file, the hand-authored `Custom Bases/` file, and the worktree were removed
//! afterward, matching every recipe in `../CAPTURE.md`.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated across the
/// rest of this `java_bugfix_*` file family — see `java_bugfix_wb013.rs`'s matching note).
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
/// standing in for real stdout/stderr — same shape as `java_bugfix_wb013.rs`'s own helper.
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

/// The custom base file this whole file is about: WALNUT-BUGS.md's exact minimal WB-014
/// repro, adapted only in the base's name (`msd_wrtest` rather than the entry's
/// `msd_wrtest` -- unchanged, it already used this name). A cross-referencing header
/// (`msd_2 msd_2 msd_2`, a number-system TOKEN on all three tracks) rather than an
/// explicit set (`{0,1} {0,1} {0,1}`) — the shape that used to crash real Java.
const WB014_ADDITION: &str = "msd_2 msd_2 msd_2\n\n0 1\n0 0 0 -> 0\n";

/// Real (fixed) `walnut-java`'s output for `eval wrtest1 "?msd_wrtest x=x";`, captured
/// against `bugfix/wb-014` (commit `6580f71`) per this file's own module docs.
const WRTEST1_CAPTURED: &str = "msd_wrtest\n\n0 1\n0 -> 0\n1 -> 0\n";

/// **The WB-014 repro itself, run to completion on both engines.** Before the Java-side
/// fix this would have crashed with a bare `ConcurrentModificationException`; now (and
/// always, on this port's side) it succeeds, and both engines compute the same
/// automaton for `x=x` over `msd_wrtest` — a custom base resolved entirely through a
/// cross-referencing (`msd_2 msd_2 msd_2`) header.
#[test]
fn wb014_cross_referencing_custom_base_header_now_succeeds_on_both_engines() {
    let (mut p, console, err, dir) = prover("wb014");
    fs::write(
        dir.join("Custom Bases/msd_wrtest_addition.txt"),
        WB014_ADDITION,
    )
    .unwrap();

    let mut input = io::Cursor::new(b"eval wrtest1 \"?msd_wrtest x=x\";\n".to_vec());
    p.read_buffer(&mut input, false);

    assert_eq!(
        err.text(),
        "",
        "fixed walnut-java writes nothing to stderr for this command (no \
         ConcurrentModificationException, no anything) -- this port must not either.\n\
         console:\n{}",
        console.text()
    );
    assert_eq!(
        console.text(),
        "eval wrtest1 \"?msd_wrtest x=x\";\n",
        "no other output is expected -- the automaton is written silently"
    );

    let ours_path = dir.join("Automata Library/wrtest1.txt");
    assert!(
        ours_path.is_file(),
        "the fixed jar writes wrtest1.txt for this command; the port must too"
    );
    let ours_text = fs::read_to_string(&ours_path).unwrap();
    assert_eq!(
        ours_text, WRTEST1_CAPTURED,
        "must be byte-identical to real fixed walnut-java's output (captured against \
         bugfix/wb-014, commit 6580f71) -- see this module's docs for why byte identity \
         happens to hold for this particular (trivial, 1-state) result"
    );

    // The semantic-equivalence check CLAUDE.md's Prime Directive calls for as the default
    // comparison (never byte/structural identity) -- run alongside the byte check above,
    // not instead of it, since the byte check costs nothing extra here. Both files share
    // the identical header token (`msd_wrtest`), and this port's own `Custom Bases/`
    // directory (the one `msd_wrtest_addition.txt` was just written into) is sufficient to
    // resolve it for both reads, since resolution only inspects the header/alphabet
    // structure, not any per-file identity.
    let custom_bases_dir = dir.join("Custom Bases");
    let ours = wr_io::reader::read_automaton_txt_with_custom_bases(&ours_path, &custom_bases_dir)
        .expect("the port's own output must parse");
    let captured_path = dir.join("Custom Bases/wb014_java_captured.txt");
    fs::write(&captured_path, WRTEST1_CAPTURED).unwrap();
    let java =
        wr_io::reader::read_automaton_txt_with_custom_bases(&captured_path, &custom_bases_dir)
            .expect("the captured fixture must parse");
    assert_eq!(
        automaton_language_equivalent(&ours, &java),
        Ok(true),
        "the port's own computed language must match real fixed walnut-java's"
    );

    fs::remove_dir_all(&dir).ok();
}

/// **Composes with the rest of the engine, not just a standalone success.** The same
/// cross-referencing-header custom base, now under an existential quantifier — exercising
/// `wr_core::quantify` (∃-projection) over a `NumberSystem` built entirely from a
/// recursively-resolved header, not just equality. Both engines print `____` then `TRUE`
/// (captured together with the case above, in the same session — see this module's docs
/// for the combined `stdout.txt`), matching this project's established convention for a
/// trivial closed-formula result (`../CAPTURE.md`'s `fixtures/u11/`/`fixtures/lsd/`
/// entries: no `.txt` fixture, the printed verdict is the meaningful observable).
#[test]
fn wb014_the_custom_base_composes_with_an_existential_quantifier() {
    let (mut p, console, err, dir) = prover("wb014-quant");
    fs::write(
        dir.join("Custom Bases/msd_wrtest_addition.txt"),
        WB014_ADDITION,
    )
    .unwrap();

    let mut input = io::Cursor::new(b"eval wrtest3 \"?msd_wrtest Ex x=x\";\n".to_vec());
    p.read_buffer(&mut input, false);

    assert_eq!(
        err.text(),
        "",
        "fixed walnut-java writes nothing to stderr here either"
    );
    assert_eq!(
        console.text(),
        "eval wrtest3 \"?msd_wrtest Ex x=x\";\n____\nTRUE\n",
        "must match real fixed walnut-java's stdout verbatim (captured against \
         bugfix/wb-014, commit 6580f71)"
    );

    let ours_path = dir.join("Automata Library/wrtest3.txt");
    let ours =
        wr_io::reader::read_automaton_txt_with_custom_bases(&ours_path, &dir.join("Custom Bases"))
            .expect("the port's own output must parse");
    assert!(
        ours.fa.is_true_automaton(),
        "a closed `Ex x=x` over any nonempty base is TRUE"
    );

    fs::remove_dir_all(&dir).ok();
}
