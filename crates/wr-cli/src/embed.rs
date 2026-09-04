// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! In-process embedding facade — the second of the two consumption mechanisms a
//! downstream research consumer (e.g. `ct-research`) can use.
//!
//! The first mechanism is the **shell-out drop-in**: run the `walnut-rs` binary
//! (via `bin/walnut-rs`) as a stdin REPL, exactly how the JVM Walnut is run today
//! (`docs/CT-RESEARCH-INTEGRATION.md`). This module is the alternative for a
//! consumer that would rather link `wr-cli` directly and drive the engine in the
//! same process — no subprocess, no stdin pipe, no `.txt` round-trip through disk.
//!
//! It is a **thin, curated facade over already-verified entry points**, adding no
//! new decision-procedure behavior of its own:
//!
//! * [`Engine`] owns a [`Prover`] plus a captured stdout sink, so a single
//!   [`Engine::run`] call reproduces exactly what one REPL line would print
//!   (crucially, the `TRUE`/`FALSE` verdict line a shell consumer greps for) but
//!   hands it back as a `String` instead of writing it to a terminal.
//! * [`Engine::eval_structured`] exposes the structured
//!   [`Prover::dispatch_for_integration_test`] path, returning the resulting
//!   automaton/verdict as an in-memory [`TestCase`] rather than the console text.
//! * [`set_thread_count`] is re-exported so an embedder can pin walnut-rs's
//!   parallel degree (e.g. `set_thread_count(1)` for bit-reproducible,
//!   single-threaded runs) **before** the first decision-procedure call, without
//!   the `WR_CORE_THREADS` environment variable the shell path uses. It must be
//!   called before any query runs; afterward it returns
//!   [`ParallelismAlreadyStarted`]. See `wr_core::parallel`.
//!
//! Everything here writes into a session tree under the engine's `home_dir`, the
//! same as the binary — `run` has full REPL file-writing semantics (`def NAME`
//! saves `NAME.txt` under `Session/<timestamp>/Automata Library/`). Nothing in
//! this module reaches outside that `home_dir`.
//!
//! ```no_run
//! use wr_cli::embed::{Engine, set_thread_count};
//!
//! // Deterministic, single-threaded — optional; must precede the first query.
//! let _ = set_thread_count(1);
//!
//! // A workspace holding "Automata Library/", "Word Automata Library/", etc.
//! let mut engine = Engine::new("/path/to/workspace")?;
//!
//! // A closed formula evaluates to a TRUE/FALSE verdict.
//! assert_eq!(engine.eval_bool(r#"eval q "?msd_2 Ex x = x""#)?, Some(true));
//!
//! // `def` builds and saves a named automaton for later reference in-session.
//! engine.run(r#"def lt "?msd_2 x < y""#)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use wr_core::logging::Logging;

use crate::prover::{Prover, ProverError};
use crate::session::{Session, SessionPaths};
use crate::test_case::TestCase;

pub use wr_core::{set_thread_count, ParallelismAlreadyStarted};

/// A cloneable, in-memory stdout sink. One clone is handed to the [`Prover`] as
/// its `out`; [`Engine`] keeps the other to read back and drain per command.
#[derive(Clone, Default)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl SharedBuf {
    /// Remove and return everything written since the last drain, as UTF-8
    /// (lossily — the engine only ever writes UTF-8, but this never panics on a
    /// hypothetical non-UTF-8 byte).
    fn drain_text(&self) -> String {
        let mut guard = self.0.lock().expect("SharedBuf mutex poisoned");
        let text = String::from_utf8_lossy(&guard).into_owned();
        guard.clear();
        text
    }
}

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("SharedBuf mutex poisoned")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// An embedded walnut-rs engine: a single long-lived [`Prover`] over a session
/// rooted at a caller-chosen `home_dir`, with its console output captured so each
/// command's text (including the `TRUE`/`FALSE` verdict) is returned rather than
/// printed.
///
/// Construct one with [`Engine::new`] (an explicit workspace) or
/// [`Engine::in_current_dir`], then drive it with [`Engine::run`] /
/// [`Engine::eval_bool`] / [`Engine::eval_structured`]. Reuse one engine across a
/// sequence of commands to get REPL semantics — a `def`'d name is visible to
/// later commands in the same engine, exactly as within one piped session.
///
/// `Logging`'s own console/detail streams are sent to a sink, so the captured
/// text is just what Java prints with a bare `System.out.print` (the verdict, the
/// command echo) — not the `::`-style detailed step log. A consumer that needs the
/// detailed log (e.g. for a state-count watchdog) should use the shell-out binary,
/// whose `::` output is byte-identical to the JVM engine's.
pub struct Engine {
    prover: Prover,
    out: SharedBuf,
}

impl Engine {
    /// An engine whose library directories (`Automata Library/`, `Word Automata
    /// Library/`, `Custom Bases/`, ...) and session/result output live under
    /// `home_dir`. The directory tree is created if missing (the same
    /// `Session.createSubdirectories` the binary runs at startup).
    ///
    /// The session directory is auto-generated (`Session/<timestamp>/` under
    /// `home_dir`), matching the binary's default; pass an explicit session
    /// directory via [`Engine::with_session_dir`] if you need a fixed location.
    pub fn new(home_dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::build(None, Some(home_dir.as_ref()))
    }

    /// As [`Engine::new`], but rooted at the process's current working directory —
    /// the binary's own no-`--home-dir` default.
    pub fn in_current_dir() -> io::Result<Self> {
        Self::build(None, None)
    }

    /// As [`Engine::new`], but with an explicit `session_dir` (no auto-generated
    /// timestamp subdirectory) — the analog of the binary's `--session-dir`.
    pub fn with_session_dir(
        home_dir: impl AsRef<Path>,
        session_dir: impl AsRef<Path>,
    ) -> io::Result<Self> {
        Self::build(Some(session_dir.as_ref()), Some(home_dir.as_ref()))
    }

    fn build(session_dir: Option<&Path>, home_dir: Option<&Path>) -> io::Result<Self> {
        let session_dir = session_dir.map(dir_arg);
        let home_dir = home_dir.map(dir_arg);
        let paths = SessionPaths::new(session_dir.as_deref(), home_dir.as_deref(), false);
        // `Session.createSubdirectories` (`Session.java:132`) — the same directory
        // setup the binary does; a failure here is an I/O error, not a query error.
        paths.create_subdirectories().map_err(io::Error::other)?;
        let out = SharedBuf::default();
        // Route `Logging`'s own console/detail streams to a sink; only the
        // command's `System.out.print` output (the verdict) is captured in `out`.
        let logging = Logging::with_writers(Box::new(io::sink()), Box::new(io::sink()));
        let prover =
            Prover::with_output(Session::from_paths(paths), logging, Box::new(out.clone()));
        Ok(Engine { prover, out })
    }

    /// Run one command with full REPL semantics and return exactly the text the
    /// REPL would have printed for it. For a closed `eval` that is a bare `TRUE`
    /// or `FALSE` **line**, preceded by Walnut's own `____` prompt-erase line
    /// (i.e. `"____\nTRUE\n"`) — faithful to the JVM engine, and the same shape a
    /// shell consumer greps with `grep -xE '(TRUE|FALSE)'` (the `____` is a
    /// separate line, so it does not match). Use [`Engine::eval_bool`] for the
    /// parsed verdict.
    ///
    /// File-writing side effects happen as in the binary: `def NAME "..."` saves
    /// `NAME.txt` under the session's `Automata Library/`, `reg`/`alphabet` create
    /// their automata, etc.
    ///
    /// Returns the underlying [`ProverError`] on a malformed or failing command,
    /// the same error the binary would have reported.
    pub fn run(&mut self, command: &str) -> Result<String, ProverError> {
        // Drop any residue from a previous call, then dispatch and take what this
        // command printed. `dispatch` returns whether the REPL would continue
        // (`false` only for `exit`/`quit`), which an embedder does not need.
        let _ = self.out.drain_text();
        self.prover.dispatch(&terminate(command))?;
        Ok(self.out.drain_text())
    }

    /// Evaluate a **closed** formula and return its verdict: `Some(true)` for
    /// `TRUE`, `Some(false)` for `FALSE`, or `None` if the command produced
    /// something that is not a bare boolean verdict (e.g. an `eval`/`def` with a
    /// free variable, which yields an automaton, or a non-`eval` command).
    ///
    /// Reads [`Engine::run`]'s text exactly as a shell consumer does — scanning
    /// for a **line** that is precisely `TRUE` or `FALSE` (`grep -xE '(TRUE|FALSE)'`)
    /// — so Walnut's leading `____` prompt-erase line is ignored, and any other
    /// output (an automaton save, a details dump) yields `None`. A closed `eval`
    /// prints exactly one verdict line; if the output somehow carried two
    /// *conflicting* verdict lines it is treated as ambiguous (`None`) rather than
    /// silently preferring one.
    pub fn eval_bool(&mut self, command: &str) -> Result<Option<bool>, ProverError> {
        let text = self.run(command)?;
        let mut verdict = None;
        for line in text.lines() {
            let this = match line {
                "TRUE" => Some(true),
                "FALSE" => Some(false),
                _ => continue,
            };
            match verdict {
                None => verdict = this,
                Some(prev) if prev == this.unwrap() => {}
                // A second, differing verdict line: ambiguous.
                Some(_) => return Ok(None),
            }
        }
        Ok(verdict)
    }

    /// Evaluate a command through the structured, in-memory dispatch path
    /// ([`Prover::dispatch_for_integration_test`]) and return its result as a
    /// [`TestCase`] (the resulting automaton, details, or error text) rather than
    /// console output. `Ok(None)` for a command that yields no test case (a blank
    /// line, a comment, `clear`, ...).
    ///
    /// This is the path the golden-corpus harness uses; prefer it when you want the
    /// resulting automaton object directly instead of parsing the binary's `.txt`.
    pub fn eval_structured(&mut self, command: &str) -> Result<Option<TestCase>, ProverError> {
        self.prover
            .dispatch_for_integration_test(&terminate(command), "")
    }

    /// The underlying [`Prover`], for commands the facade does not wrap directly.
    pub fn prover(&mut self) -> &mut Prover {
        &mut self.prover
    }
}

/// Ensure a command carries the `;`/`:` terminator [`Prover::dispatch`] requires
/// (`parse_setup` rejects an unterminated command). A consumer may pass a bare
/// `eval q "…"`; a trailing `;` is appended if — ignoring trailing ASCII
/// whitespace — the command is not already terminated. An explicit `::` (detailed
/// mode) or `;` the caller supplied is left untouched.
fn terminate(command: &str) -> String {
    // Trim trailing ASCII whitespace FIRST: `parse_setup` (via `dispatch`) checks
    // the raw final byte, so `"eval q \"…\";\n"` — a command string a shell
    // consumer pipes verbatim — must have its trailing newline removed, else the
    // already-`;`-terminated command is rejected as `InvalidCommand`. Returning the
    // trimmed form in BOTH branches is what makes "ignoring trailing whitespace"
    // true rather than just claimed.
    let trimmed = command.trim_end();
    if trimmed.ends_with(';') || trimmed.ends_with(':') {
        trimmed.to_string()
    } else {
        format!("{trimmed};")
    }
}

/// Normalize a directory path to the trailing-slash string form
/// [`SessionPaths::new`] expects (it concatenates, rather than path-joins, the
/// directory constants — see the `session` module docs), matching `parse_args`.
fn dir_arg(p: &Path) -> String {
    let mut s = p.to_string_lossy().into_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}
