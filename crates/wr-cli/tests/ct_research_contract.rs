// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Pins the **consumption contract** a downstream consumer (ct-research) relies
//! on, for both mechanisms (`docs/CT-RESEARCH-INTEGRATION.md`):
//!
//! * the **shell-out binary** run as a stdin REPL with libraries resolved
//!   relative to its cwd — the exact shape ct-research runs the JVM Walnut in
//!   (`bin/walnut` → `java -jar … "$@"`, cwd = workspace, commands on stdin);
//! * the **in-process facade** [`wr_cli::embed::Engine`].
//!
//! Semantic/cross-engine fidelity is proven elsewhere (golden corpus, differential
//! generation, fuzzing). What is load-bearing *here*, and untested until now, is the
//! I/O contract the consumer's shell drivers actually parse:
//!
//! 1. `eval` of a closed formula prints a line that is **exactly** `TRUE`/`FALSE`
//!    (the consumer greps `^(TRUE|FALSE)$`).
//! 2. `def NAME "…"` writes `NAME.txt` in native format into the session's
//!    `Automata Library/`, from where the consumer reads it back and counts states
//!    with `grep -cE '^[0-9]+ [0-9]+$'`.
//!
//! Each assertion is exact (a line equals `"TRUE"`, not merely contains it) so it
//! fails if the contract shifts — e.g. if a future change wrapped the verdict in
//! other text, or moved the `def` output, the consumer's greps would break and so
//! would these tests.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use wr_cli::embed::Engine;

/// The `walnut-rs` binary Cargo built for this integration test.
const BIN: &str = env!("CARGO_BIN_EXE_walnut-rs");

/// The library directories the binary/facade resolve relative to a workspace —
/// the ones a consumer stages (ct-research's `bin/walnut-workspace` mirror).
const LIB_DIRS: &[&str] = &[
    "Automata Library",
    "Word Automata Library",
    "Custom Bases",
    "Command Files",
    "Result",
    "Session",
    "Macro Library",
    "Morphism Library",
    "Transducer Library",
    "Test Results",
];

/// A fresh workspace directory with the standard library subdirectories.
fn workspace(tag: &str) -> PathBuf {
    // `tag` is unique per test, so the directory name is too, even with tests
    // running in parallel in one process.
    let dir = std::env::temp_dir().join(format!("wr-ct-contract-{tag}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    for sub in LIB_DIRS {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// Run the binary in `ws` with `commands` on stdin (as a consumer's driver does),
/// returning its stdout.
fn run_binary(ws: &Path, commands: &str) -> String {
    let mut child = Command::new(BIN)
        .current_dir(ws)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn walnut-rs binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(commands.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait for walnut-rs");
    assert!(
        out.status.success(),
        "binary exited non-zero: {:?}",
        out.status
    );
    String::from_utf8(out.stdout).expect("stdout is UTF-8")
}

/// The consumer's `grep -cE '^[0-9]+ [0-9]+$'` state-count: lines that are exactly
/// two all-digit runs joined by a single ASCII space (a `<state> <output>` header).
/// Transition lines (`0 0 -> 0`) are excluded (the `->` breaks the pattern).
fn state_header_count(txt: &str) -> usize {
    // Split on `\n` (not `str::lines`, which would strip a trailing `\r`) so this
    // is byte-for-byte what `grep -cE '^[0-9]+ [0-9]+$'` matches: two non-empty
    // all-ASCII-digit runs joined by exactly one ASCII space, and nothing else —
    // a tab, a second space, a leading/trailing space, or a CRLF `\r` all fail.
    txt.split('\n')
        .filter(|line| match line.split_once(' ') {
            Some((a, b)) => {
                !a.is_empty()
                    && !b.is_empty()
                    && a.bytes().all(|c| c.is_ascii_digit())
                    && b.bytes().all(|c| c.is_ascii_digit())
            }
            None => false,
        })
        .count()
}

/// The newest `NAME.txt` under `ws/Session/*/Automata Library/`, the location a
/// consumer `ls -t`'s after a `def` (verified against the real JVM engine).
fn session_automaton(ws: &Path, name: &str) -> Option<PathBuf> {
    let mut hits: Vec<PathBuf> = Vec::new();
    let sessions = fs::read_dir(ws.join("Session")).ok()?;
    for entry in sessions.flatten() {
        let candidate = entry
            .path()
            .join("Automata Library")
            .join(format!("{name}.txt"));
        if candidate.is_file() {
            hits.push(candidate);
        }
    }
    hits.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
    hits.pop()
}

// ---------------------------------------------------------------- shell-out binary

#[test]
fn binary_eval_prints_exact_true_false_lines() {
    let ws = workspace("eval-tf");
    let out = run_binary(
        &ws,
        // `Ex x=x` is satisfiable (TRUE); `Ex x<x` is not (FALSE).
        "eval t \"?msd_2 Ex x = x\";\neval f \"?msd_2 Ex (x < x)\";\nquit;\n",
    );
    // Exactly one `TRUE` line and exactly one `FALSE` line — a bare token on its
    // own line, which is what `grep -xE '(TRUE|FALSE)'` matches.
    assert_eq!(
        out.lines().filter(|l| *l == "TRUE").count(),
        1,
        "want exactly one bare TRUE line; stdout was:\n{out}"
    );
    assert_eq!(
        out.lines().filter(|l| *l == "FALSE").count(),
        1,
        "want exactly one bare FALSE line; stdout was:\n{out}"
    );
    // Order preserved: TRUE (first query) before FALSE (second).
    let true_at = out.lines().position(|l| l == "TRUE").unwrap();
    let false_at = out.lines().position(|l| l == "FALSE").unwrap();
    assert!(true_at < false_at, "verdict order not preserved:\n{out}");
}

#[test]
fn binary_def_writes_native_txt_into_session_tree() {
    let ws = workspace("def-file");
    run_binary(&ws, "def lt \"?msd_2 x < y\";\nquit;\n");

    let file = session_automaton(&ws, "lt")
        .expect("def must write lt.txt under Session/<ts>/Automata Library/");
    let txt = fs::read_to_string(&file).unwrap();

    // Native Walnut serialization: a numeration-system header line, then
    // `<state> <output>` blocks with `<input> -> <dest>` transition lines.
    let first = txt.lines().next().unwrap_or("");
    assert!(
        first.contains("msd_2"),
        "first line should be the numeration header; got {first:?}"
    );
    // `x < y` over msd_2 minimizes to two states — the exact count the consumer's
    // `grep -cE '^[0-9]+ [0-9]+$'` yields.
    assert_eq!(
        state_header_count(&txt),
        2,
        "unexpected state-header count in:\n{txt}"
    );
    assert!(
        txt.contains("->"),
        "serialization must contain transition lines:\n{txt}"
    );

    // The top-level `Automata Library/` is NOT written (matches the JVM engine —
    // both write only under the session tree). A consumer that wants it there
    // copies it itself, as ct-research's sync-maxgap.sh does.
    assert!(
        !ws.join("Automata Library").join("lt.txt").exists(),
        "top-level Automata Library must stay empty (JVM parity)"
    );
}

#[test]
fn binary_resolves_libraries_relative_to_cwd() {
    // A def'd automaton referenced by a later `$name(...)` in the same session
    // proves in-session resolution through the cwd-rooted library tree.
    let ws = workspace("cwd-lib");
    let out = run_binary(
        &ws,
        "def lt \"?msd_2 x < y\";\neval q \"?msd_2 Ex Ey $lt(x, y)\";\nquit;\n",
    );
    assert_eq!(
        out.lines().filter(|l| *l == "TRUE").count(),
        1,
        "in-session $lt reference should evaluate TRUE; stdout was:\n{out}"
    );
}

// ---------------------------------------------------------------- in-process facade

#[test]
fn facade_eval_bool_matches_verdicts() {
    let ws = workspace("facade-bool");
    let mut engine = Engine::new(&ws).unwrap();
    assert_eq!(
        engine.eval_bool("eval t \"?msd_2 Ex x = x\"").unwrap(),
        Some(true)
    );
    assert_eq!(
        engine.eval_bool("eval f \"?msd_2 Ex (x < x)\"").unwrap(),
        Some(false)
    );
}

#[test]
fn facade_run_returns_exact_verdict_text() {
    let ws = workspace("facade-run");
    let mut engine = Engine::new(&ws).unwrap();
    // Faithful REPL text: the `____` prompt-erase line then a bare `TRUE` line —
    // exactly what a shell consumer greps line-wise.
    let text = engine.run("eval t \"?msd_2 Ex x = x\"").unwrap();
    assert!(
        text.lines().any(|l| l == "TRUE"),
        "run() should print a bare TRUE line; text was {text:?}"
    );
    assert!(
        !text.lines().any(|l| l == "FALSE"),
        "run() should not print FALSE here; text was {text:?}"
    );

    // A second command on the same engine gets only its own output (the buffer is
    // drained per call), not the first command's residue.
    let text2 = engine.run("eval f \"?msd_2 Ex (x < x)\"").unwrap();
    assert!(
        text2.lines().any(|l| l == "FALSE"),
        "second run() should print a bare FALSE line; text was {text2:?}"
    );
    assert!(
        !text2.lines().any(|l| l == "TRUE"),
        "second run() leaked first verdict: {text2:?}"
    );
}

#[test]
fn facade_def_is_visible_to_later_commands_in_session() {
    let ws = workspace("facade-def");
    let mut engine = Engine::new(&ws).unwrap();
    engine.run("def lt \"?msd_2 x < y\"").unwrap();
    assert_eq!(
        engine
            .eval_bool("eval q \"?msd_2 Ex Ey $lt(x, y)\"")
            .unwrap(),
        Some(true),
        "def'd $lt should be visible to a later command on the same engine"
    );
}

#[test]
fn facade_eval_structured_returns_an_automaton() {
    let ws = workspace("facade-struct");
    let mut engine = Engine::new(&ws).unwrap();
    // A formula with a free variable yields an automaton, not a bare verdict.
    let tc = engine
        .eval_structured("eval q \"?msd_2 x < y\"")
        .unwrap()
        .expect("a value-producing eval yields a TestCase");
    assert!(
        !tc.automaton_pairs().is_empty(),
        "structured eval should carry at least one automaton"
    );
    // And eval_bool of the same (non-closed) formula is None, not a false verdict.
    let mut engine2 = Engine::new(workspace("facade-struct2")).unwrap();
    assert_eq!(engine2.eval_bool("eval q \"?msd_2 x < y\"").unwrap(), None);
}

#[test]
fn facade_accepts_already_terminated_and_trailing_whitespace_commands() {
    // A consumer reuses the exact `;\n`-terminated command strings it pipes to the
    // binary. The facade must accept them, not just bare unterminated commands —
    // `terminate` normalizes trailing whitespace so `parse_setup` does not reject
    // an already-`;`/`::`-terminated line whose raw final byte is a newline.
    let ws = workspace("facade-term");
    let mut engine = Engine::new(&ws).unwrap();
    let closed = r#"?msd_2 Ex x = x"#;
    for form in [
        format!("eval q \"{closed}\""),     // bare
        format!("eval q \"{closed}\";"),    // already terminated
        format!("eval q \"{closed}\";\n"),  // terminated + trailing newline
        format!("eval q \"{closed}\"  "),   // trailing whitespace, no terminator
        format!("eval q \"{closed}\"::\n"), // detailed mode + newline
    ] {
        assert_eq!(
            engine.eval_bool(&form).unwrap(),
            Some(true),
            "eval_bool should accept command form {form:?}"
        );
    }
}
