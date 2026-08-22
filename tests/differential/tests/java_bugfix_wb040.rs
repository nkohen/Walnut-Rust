// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-040** (`docs/WALNUT-BUGS.md`) — `docs/
//! WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-17. Like `java_bugfix_wb014.rs`, this is a
//! **divergence-closed** unit, not a **port-bug-fixed** one: no `wr-core`/`wr-logic`/
//! `wr-io`/`wr-cli` production code changes alongside this test. See `docs/WALNUT-BUGS.md`
//! WB-040's own entry for the full account; summarized here for this file's own context.
//!
//! # What WB-040 was, and why the port never had it
//!
//! Real (pre-fix) `walnut-java`'s `AutomatonWriter.writeToGV` called `automaton.canonize()`
//! **on the live object it was handed** — and that object is the very `Automaton`
//! `DeterminizationStrategies.determinize`'s own `[export n gv]` pre-determinization dump
//! hook is about to run `determinize` on. `canonize()` rebuilds `Q`/`q0`/`O`/`d` via a
//! BFS-from-`q0` permutation map and **drops any state absent from it** — and
//! `AutomatonLogicalOps.reverse` never updates `fa.q0` to the new (post-reversal) initial
//! state, so a `gv` export mid-`reverse` could canonize from a stale `q0`, silently trim a
//! state the surrounding subset construction still needed, and crash with a bare
//! `IndexOutOfBoundsException`. `wr_core::determinize::ExportRequest` hands its sink a
//! shared `&Automaton`, so this port's equivalent hook cannot mutate the automaton being
//! determinized even in principle, and `wr_cli::prover_helper::export_automata_to`'s `gv`
//! arm explicitly writes a deep clone before canonizing it (see that function's own
//! comment, unchanged by this file) — a divergence WB-040's entry already recorded as
//! *deliberate*, and confirmed (not merely defensive) by this very bug.
//!
//! `walnut-java` commit `0cf02d3` (branch `bugfix/wb-040`, stacked on `bugfix/wb-036`)
//! fixes the crash on the Java side, by the same shape the Rust port had already
//! independently converged on: `writeToGV` now canonizes a `clone()` of the automaton and
//! renders from the clone, leaving the caller's original object untouched. So **both
//! engines are now safe** on the `reverse`-during-`[export gv]` repro — this file is what
//! confirms that, rather than trusting the upstream commit message.
//!
//! # Two things this file checks, not one — read before skimming past either half
//!
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s task for this PR asked for the literal
//! `[export n gv]reverse ...::` sequence run through `wr-cli`'s real dispatch. Doing that
//! alone would be **misleading**: `wr_cli::prover::tests::
//! export_metacommands_on_a_non_eval_command_are_still_accepted_and_discarded` (unchanged,
//! pre-existing) already pins that `[export …]`/`[strategy …]` on `reverse` (and every
//! other non-`eval`/`def` command) is parsed and validated but **still silently
//! discarded** today — the determinizer's `DeterminizeContext` hook only fires from the
//! `eval`/`def` path (U32). So running the literal sequence through dispatch proves the
//! *sequence* doesn't crash, but proves nothing about `ExportRequest`'s clone-before-
//! canonize architecture, because that code path is never reached by `reverse` yet.
//! Verified live before writing anything below (see the module docs' "Capture recipe"
//! section): the current release binary reproduces this — three successful `reverse`s,
//! zero `_pre.gv`/`_pre.ba` files anywhere.
//!
//! So this file checks BOTH halves, independently:
//!
//! 1. [`wb040_reverse_export_gv_sequence_matches_fixed_java_and_export_is_still_a_noop_for_reverse`]
//!    — the literal repro, dispatched for real, checked against real fixed `walnut-java`'s
//!    own output by [`wr_core::equiv::automaton_language_equivalent`] (never byte identity
//!    as the primary check, per `CLAUDE.md`'s Prime Directive) — AND a check that the
//!    known wiring boundary above hasn't silently changed underneath this test (if it
//!    ever does, `export_metacommands_on_a_non_eval_command_are_still_accepted_and_discarded`
//!    is the one to invert, per its own doc comment — not this file).
//! 2. [`wb040_export_automata_gv_arm_does_not_mutate_the_original_automaton`] — exercises
//!    the actual primitive (`wr_cli::prover_helper::export_automata_to`'s `gv` arm) that
//!    `eval`/`def` (and, once wired, `reverse`) reach in production, on a fixture shaped
//!    exactly like real `walnut-java`'s own new regression test,
//!    `AutomatonWriterTest#testWriteToGV_doesNotMutateTheOriginalAutomaton` (a real,
//!    declared-but-unreachable-from-`q0` state) — the actual architectural claim WB-040's
//!    "Rust port" section makes, demonstrated directly rather than only inferred from the
//!    type signature.
//!
//! # Capture recipe (reproducible)
//!
//! `bugfix/wb-040` was already checked out at the main `~/dev/walnut-java` working tree
//! when this was captured (the same situation `java_bugfix_wb014.rs`/`wb021.rs`/`wb032.rs`/
//! `wb035.rs`/`wb036.rs`/`wb038.rs` hit), so the worktree below is added by commit hash
//! (detached), not by branch name:
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb040 0cf02d3
//! cd /tmp/walnut-java-wb040
//! export JAVA_HOME=/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1/Contents/Home
//! export PATH="$JAVA_HOME/bin:$PATH"
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! # The exact fixture from AutomatonWriterTest#testWriteToGV_doesNotMutateTheOriginalAutomaton /
//! # ReverseTest#testWB040_exportGvDuringReverseDoesNotDropAStateOrCrash (state 1 has no
//! # edge back to state 0, so post-reversal, stale q0=0 can only reach itself):
//! printf 'msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n' \
//!     > "Automata Library/wb040scratch_base.txt"
//!
//! cat > "Command Files/wb040scratch_capture.txt" <<'EOF'
//! reverse wb040scratch_revbNoExport $wb040scratch_base::
//! [export 0 ba]reverse wb040scratch_revbBa $wb040scratch_base::
//! [export 0 gv]reverse wb040scratch_revbGv $wb040scratch_base::
//! EOF
//! java -cp target/Walnut-all.jar Main.Prover wb040scratch_capture.txt \
//!     >stdout.txt 2>stderr.txt </dev/null
//!
//! rm -f "Automata Library/wb040scratch_base.txt" "Command Files/wb040scratch_capture.txt"
//! rm -rf Session
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb040 --force
//! ```
//!
//! `java`/`mvnw` above actually ran under a JDK 17+ toolchain (the shell's default `java`
//! resolves to a JDK 11 too old for this project's class file version, and `JAVA_HOME` must
//! point at the `Contents/Home` subdirectory or `mvnw` refuses to start). `</dev/null`
//! matters too: without it the process runs the command file and then blocks in the
//! interactive REPL.
//!
//! `stderr.txt` was empty. `stdout.txt` (captured 2026-08-22): all three `reverse`s
//! succeed, each reporting `Determinizing [#0, strategy: SC]: 2 states` /
//! `Minimized:2 states` — no state-count drop, no crash, matching the Java-side commit's
//! own claimed post-fix behavior. `Session/<timestamp>/Automata Library/` holds
//! `wb040scratch_revbNoExport.txt`/`wb040scratch_revbBa.txt`/`wb040scratch_revbGv.txt`, all
//! three **byte-identical**:
//!
//! ```text
//! lsd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 1
//!
//! 1 1
//! 0 -> 1
//! 1 -> 1
//! ```
//!
//! (`lsd_2`, not `msd_2`: `reverse` flips the numeration direction — expected, and
//! unrelated to WB-040.) Inlined below as [`CAPTURED_REVERSED`], the same "inline a small
//! captured fixture directly" convention `java_bugfix_wb014.rs`'s `WRTEST1_CAPTURED` uses.
//!
//! **Before writing any test, the current release build of `walnut-rs`
//! (`cargo build -p wr-cli --release`) was run live against the identical repro**
//! (`--home-dir=` pointed at a fresh scratch tree, same command file), to independently
//! confirm both this file's premise and WB-040's "Rust port" claim rather than trusting
//! either: all three `reverse`s succeeded, producing the identical `lsd_2` automaton shown
//! above (confirming no divergence from fixed Java on the shared observable) — and,
//! separately, no `_pre.gv`/`_pre.ba` file appeared anywhere under the scratch tree's
//! `Result/` directory for any of the three commands (confirming the existing
//! `export_metacommands_on_a_non_eval_command_are_still_accepted_and_discarded` tripwire's
//! claim still holds: `[export …]` on `reverse` is still a parsed-and-discarded no-op, not
//! yet wired to `ExportRequest`).

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::prover_helper::export_automata_to;
use wr_cli::session::Session;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;
use wr_io::reader::read_automaton_txt;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here per this `java_bugfix_*` file family's established
/// convention (see `java_bugfix_wb014.rs`'s matching note).
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
/// standing in for real stdout/stderr — same shape as `java_bugfix_wb014.rs`'s own helper.
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

/// WALNUT-BUGS.md's WB-040 trigger fixture, unchanged from the entry's own repro and from
/// real `walnut-java`'s new `AutomatonWriterTest`/`ReverseTest` regression tests: state 1
/// has no edge back to state 0, so post-`reverse` (which reverses every edge but never
/// updates `fa.q0`), the still-stale `q0 = 0` can only reach itself in the reversed graph
/// — exactly the shape `canonize()`'s BFS-from-`q0` needs to trim a real, still-needed
/// state.
const WB040_BASE: &str = "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n";

/// Real (fixed) `walnut-java`'s output for all three `reverse` variants (no export, `ba`
/// export, `gv` export) — byte-identical across all three, captured against `bugfix/wb-040`
/// (commit `0cf02d3`) per this file's own module docs.
const CAPTURED_REVERSED: &str = "lsd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n";

/// **Half 1**: the literal WB-040 repro (`reverse` with no/`ba`/`gv` export, `::`-suffixed),
/// dispatched through the real `Prover`. Before the Java-side fix, the third command would
/// have crashed with a bare `IndexOutOfBoundsException`; now (and always, on this port's
/// side — see the module docs) all three succeed and agree with fixed `walnut-java`. This
/// also re-pins the known, pre-existing wiring boundary (`[export …]` on `reverse` is still
/// a no-op here) so a future change silently narrowing what this test actually exercises
/// doesn't go unnoticed — see the module docs' "Two things this file checks" section.
#[test]
fn wb040_reverse_export_gv_sequence_matches_fixed_java_and_export_is_still_a_noop_for_reverse() {
    let (mut p, _console, err, dir) = prover("wb040-reverse");
    fs::write(
        dir.join("Automata Library/wb040scratch_base.txt"),
        WB040_BASE,
    )
    .unwrap();

    for (name, cmd) in [
        (
            "wb040scratch_revbNoExport",
            "reverse wb040scratch_revbNoExport $wb040scratch_base::",
        ),
        (
            "wb040scratch_revbBa",
            "[export 0 ba]reverse wb040scratch_revbBa $wb040scratch_base::",
        ),
        (
            "wb040scratch_revbGv",
            "[export 0 gv]reverse wb040scratch_revbGv $wb040scratch_base::",
        ),
    ] {
        p.dispatch(cmd)
            .unwrap_or_else(|e| panic!("`{cmd}` must succeed, matching fixed walnut-java: {e}"));

        let path = dir.join(format!("Automata Library/{name}.txt"));
        assert!(path.is_file(), "`{cmd}` must write {name}.txt");
        let ours_text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            ours_text, CAPTURED_REVERSED,
            "`{cmd}`'s result must be byte-identical to real fixed walnut-java's output \
             (captured against bugfix/wb-040, commit 0cf02d3)"
        );

        // The semantic-equivalence check CLAUDE.md's Prime Directive calls for as the
        // default comparison (never byte/structural identity) -- run alongside the byte
        // check above, not instead of it, since it costs nothing extra for this trivial
        // (already byte-identical) 2-state result.
        let ours = read_automaton_txt(&path).expect("the port's own output must parse");
        let captured_path = dir.join(format!("Automata Library/{name}_java_captured.txt"));
        fs::write(&captured_path, CAPTURED_REVERSED).unwrap();
        let java = read_automaton_txt(&captured_path).expect("the captured fixture must parse");
        assert_eq!(
            automaton_language_equivalent(&ours, &java),
            Ok(true),
            "`{cmd}`'s computed language must match real fixed walnut-java's"
        );
    }

    assert!(
        err.text().is_empty(),
        "no command here should write to stderr"
    );

    // Re-pin the known wiring boundary this file's module docs describe: `[export …]` on
    // `reverse` is still a parsed-and-discarded no-op today (`wr_cli::prover::tests::
    // export_metacommands_on_a_non_eval_command_are_still_accepted_and_discarded`), so no
    // `_pre.gv`/`_pre.ba` dump exists anywhere under `Result/` for any of the three
    // commands above. If this ever fails, `reverse`'s export arm has been wired -- that is
    // real progress, not a regression, but it means Half 2 below (the primitive-level
    // check) is no longer the only thing standing between `reverse` and WB-040's original
    // failure mode, and this assertion (not Half 2) is the one to remove/invert.
    let stray: Vec<String> = fs::read_dir(dir.join("Result"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("_pre"))
        .collect();
    assert!(
        stray.is_empty(),
        "`[export …]` on `reverse` is expected to still be a no-op; if these files \
         appeared, that arm has been wired -- see this test's own doc comment: {stray:?}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// **Half 2**: exercises the actual primitive WB-040's fix (and this port's own
/// pre-existing architecture) is about — `wr_cli::prover_helper::export_automata_to`'s
/// `gv` arm — directly, on a fixture shaped exactly like real `walnut-java`'s own new
/// regression test, `AutomatonWriterTest#testWriteToGV_doesNotMutateTheOriginalAutomaton`:
/// three states, one (`2`) genuinely declared but unreachable from `q0 = 0`, so
/// `canonize()`'s BFS-from-`q0` legitimately drops it from the *rendered* view but must
/// never be allowed to drop it from the *caller's own* automaton object.
///
/// This is what actually demonstrates the "Rust port" section's architectural claim
/// (`ExportRequest` hands a shared `&Automaton`; the `gv` arm clones before canonizing) —
/// Half 1 above cannot, today, since `reverse`'s own export arm isn't wired to reach this
/// function at all yet (see Half 1's own doc comment).
#[test]
fn wb040_export_automata_gv_arm_does_not_mutate_the_original_automaton() {
    // Mirrors AutomatonWriterTest's own fixture exactly (state 2 is real -- declared, with
    // its own self-loop transitions -- but unreachable from q0 = 0: q0 reaches {0, 1} via
    // symbols 0/1 respectively, and nothing reaches 2).
    const UNREACHABLE_STATE_FIXTURE: &str =
        "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n\n2 1\n0 -> 2\n1 -> 2\n";

    let dir = std::env::temp_dir().join(format!(
        "wr-differential-javabugfix-wb040-primitive-{}",
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
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);

    let fixture_path = dir.join("Automata Library/wb040scratch_unreachable.txt");
    fs::write(&fixture_path, UNREACHABLE_STATE_FIXTURE).unwrap();
    let original = read_automaton_txt(&fixture_path).expect("the fixture must parse");
    assert_eq!(original.fa.q, 3, "sanity: the fixture really has 3 states");

    let mut out: Vec<u8> = Vec::new();
    export_automata_to(
        session.paths(),
        Some("some predicate"),
        "wb040scratch_out",
        "gv",
        &original,
        false,
        &mut out,
    )
    .expect("the gv export must succeed");

    // The property this test exists to pin: the ORIGINAL automaton (a shared `&Automaton`
    // reference the whole way down, per the module docs) must be untouched -- still 3
    // states, not silently trimmed to 2 the way pre-fix Java's live-object `canonize()`
    // would have done to the very automaton `determinize` was about to consume.
    assert_eq!(
        original.fa.q, 3,
        "export_automata_to's gv arm must not mutate the caller's own automaton"
    );

    // The written file, meanwhile, legitimately reflects the canonized (unreachable-state-
    // trimmed) view -- that is canonize()'s documented job, just performed on a clone, not
    // the live object. Count the per-state node declarations (excluding the "qi" point
    // node), mirroring AutomatonWriterTest's own assertion shape exactly.
    let gv_path = dir.join("Result/wb040scratch_out.gv");
    assert!(gv_path.is_file(), "the gv export must write the file");
    let gv_content = fs::read_to_string(&gv_path).unwrap();
    let state_node_lines = gv_content
        .lines()
        .filter(|l| {
            l.starts_with("node [shape = circle,") || l.starts_with("node [shape = doublecircle,")
        })
        .count();
    assert_eq!(
        state_node_lines, 2,
        "the rendered .gv must show the canonized (unreachable-state-trimmed) view -- \
         2 states, not the original 3:\n{gv_content}"
    );

    fs::remove_dir_all(&dir).ok();
}
