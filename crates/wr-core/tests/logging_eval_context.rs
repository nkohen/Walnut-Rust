// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Integration test for `wr_core::logging`'s eval-log-file context, run from OUTSIDE
//! the `logging` module on purpose: from here, `Logging`'s fields are genuinely
//! private, so this test can only reach it through the crate's real public API -- no
//! cheating via private-field access the way an in-module unit test could.
//!
//! This mirrors the shape of Java's sole real call site,
//! `EvalDef.evalDefCommand`'s `try (var ignored = Logging.writeEvalLogsTo(resultName)) { ... }`
//! block, whose body (`EvalDef.compute`) makes dozens of nested `Logging` calls
//! *while the context is open*. That is exactly the case the original
//! `CommandLogContext` (a bare guard with a private `&mut Logging` field and no
//! accessor) made impossible: the only thing a real caller outside the `logging`
//! module could do with the returned guard was let it drop immediately, silently
//! producing empty log files. `Logging::with_eval_logs_to` is the fix -- a scoped
//! closure that gets `&mut Logging` for the duration of the open context.

use std::fs;
use std::io::{self, Read};
use wr_core::logging::Logging;

fn read_file(path: &str) -> String {
    let mut s = String::new();
    fs::File::open(path)
        .unwrap_or_else(|e| panic!("failed to open {path}: {e}"))
        .read_to_string(&mut s)
        .unwrap();
    s
}

/// A base path unique to this test invocation, inside a freshly-created scratch
/// directory (avoids collisions with any other test/process).
fn unique_base(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "wr-core-logging-eval-context-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_str().unwrap().to_string()
}

/// Mirrors `EvalDef.evalDefCommand`'s try-with-resources block: open the eval-log
/// context, then -- from *inside* the closure, i.e. while the context is open --
/// make the two real call shapes `EvalDef.compute` makes over the course of one
/// evaluation: `log_evaluation_step` for each top-level step, and a nested
/// `log_and_print` call standing in for deeper automaton code logging a detail.
#[test]
fn with_eval_logs_to_lets_a_real_external_caller_log_from_inside_the_open_context() {
    let base = unique_base("compute");
    // Route console/stderr to io::sink() so this test doesn't spam the real test
    // runner's stdout; the assertions below are entirely file-based.
    let mut logging = Logging::with_writers(Box::new(io::sink()), Box::new(io::sink()));
    logging.configure_for_command(false, true); // printSteps=false, printDetails=true

    let result = logging.with_eval_logs_to(&base, |l| {
        l.log_evaluation_step("x=x:1 states - 0ms", false);
        // A nested call from deeper automaton code, made while the context is open.
        // The natural-but-wrong bare-statement translation of Java's
        // try-with-resources (`logging.write_eval_logs_to(&name);`) would have
        // closed the context before this line could ever run.
        l.log_and_print("computing cross product: ...");
        l.log_evaluation_step("Total computation time: 5ms.", true);
        42
    });
    assert_eq!(
        result, 42,
        "the closure's return value must propagate out of with_eval_logs_to"
    );

    let command_log = read_file(&format!("{base}_log.txt"));
    let detailed_log = read_file(&format!("{base}_detailed_log.txt"));

    assert!(
        !command_log.is_empty(),
        "the command-log file must be non-empty -- this is the bug the natural bare-\
         statement translation produced (a silently-empty log file)"
    );
    assert!(
        !detailed_log.is_empty(),
        "the detailed-log file must be non-empty"
    );

    assert_eq!(
        command_log, "x=x:1 states - 0ms\nTotal computation time: 5ms.\n",
        "the command-log file should contain only the two log_evaluation_step lines \
         -- the nested log_and_print call must NOT reach the command file while the \
         context is open (see the module docs' asymmetry #2)"
    );
    assert_eq!(
        detailed_log,
        "x=x:1 states - 0ms\ncomputing cross product: ...\nTotal computation time: 5ms.\n",
        "the detailed-log file should contain all three calls: print_details alone \
         gates it, with no eval-log-files-active guard"
    );

    // After the closure returns, the context is closed: further calls on `logging`
    // must not reach the eval-log files anymore.
    logging.log_evaluation_step("after close", true);
    assert_eq!(
        read_file(&format!("{base}_log.txt")),
        command_log,
        "no further writes may land in the eval-log file once the context has closed"
    );
}

/// Same shape, but via the lower-level guard API ([`Logging::write_eval_logs_to`] +
/// [`wr_core::logging::CommandLogContext::logging_mut`]) instead of the closure
/// convenience -- confirms that path is *also* usable by a real external caller
/// (not just internally cheat-able via private-field access), for callers who need
/// to hold the guard across control flow a plain closure can't express.
#[test]
fn write_eval_logs_to_guard_is_usable_via_logging_mut_from_outside_the_module() {
    let base = unique_base("guard_form");
    let mut logging = Logging::with_writers(Box::new(io::sink()), Box::new(io::sink()));
    logging.configure_for_command(false, false); // printDetails=false: no detailed file

    {
        let mut ctx = logging.write_eval_logs_to(&base);
        ctx.logging_mut()
            .log_evaluation_step("only command log", true);
        ctx.close();
    }

    assert_eq!(read_file(&format!("{base}_log.txt")), "only command log\n");
}
