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
//! # Resource safety (READ THIS)
//!
//! The decision procedure is worst-case **superexponential**. A query run directly
//! on the calling thread via [`Engine`] **cannot be interrupted from outside** —
//! Rust cannot safely kill a running thread, so an unbudgeted state blow-up
//! allocates until the OS OOM-kills the process or the host freezes, and a
//! watchdog thread cannot stop it. **Always set a budget** ([`Engine::set_budget`]
//! / [`EngineBuilder::budget`]): the engine then checks the cap at every inserted
//! state and returns [`ProverError::ResourceExhausted`] with the partial automata
//! freed, instead of dying. A `max_bytes` cap additionally needs a tracking
//! allocator in your binary (`crate::tracking_alloc`). Even so, drive [`Engine`]
//! directly **only** for queries you control; run anything unbounded or externally
//! supplied in a **separate, resource-capped child process** (an `RLIMIT_AS` /
//! cgroup memory cap on Linux, an RSS-sampling watchdog on macOS, plus a
//! wall-clock watchdog that kills it — nothing in-engine bounds wall time). Full
//! guidance and the rationale: `docs/EMBEDDING-RESOURCE-SAFETY.md`.
//!
//! # Observability
//!
//! [`Engine::record_trajectory`] + [`Engine::trajectory`] give the structured
//! per-operation state counts of the last command (every subset construction's
//! per-level metastate counts and its pre-/post-minimization sizes — the
//! real-vs-transient explosion diagnosis); [`Engine::detailed_log`] gives the
//! `::`-level text, and [`EngineBuilder::console`] routes it live.
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

use std::cell::RefCell;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use wr_core::logging::Logging;
use wr_core::resource::{Instrumentation, MemoryMeterMissing, ResourceBudget, Trajectory};

use crate::prover::{Prover, ProverError};
use crate::session::{Session, SessionPaths};
use crate::test_case::TestCase;

/// The instrumentation surface an embedder drives through [`Engine::set_budget`] /
/// [`Engine::set_instrumentation`] / [`Engine::record_trajectory`], re-exported so a
/// consumer that only links `wr-cli` needs no direct `wr-core` dependency.
pub use wr_core::resource;
/// Witness / counterexample extraction from a decided automaton (the `TestCase`
/// [`Engine::eval_structured`] returns) — `shortest_accepted`, `shortest_rejected`,
/// per-track decoding. Re-exported for the same reason as [`resource`].
pub use wr_core::witness;
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
    /// What the embedder asked for (budget + its own observers); the prover gets this
    /// plus, when [`Engine::record_trajectory`] is on, the engine's own recorder.
    base_instrumentation: Instrumentation,
    /// The per-command recorder behind [`Engine::trajectory`], when enabled.
    trajectory: Option<Rc<RefCell<Trajectory>>>,
}

/// Construction options for an [`Engine`] beyond the two-argument constructors:
/// where the detailed (`::`) log's console stream goes, and an initial
/// instrumentation. Obtained from [`Engine::builder`].
pub struct EngineBuilder {
    home_dir: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    console: Option<Box<dyn Write>>,
    err_console: Option<Box<dyn Write>>,
    instrumentation: Instrumentation,
}

impl EngineBuilder {
    /// An explicit session directory (no auto-generated timestamp subdirectory) — the
    /// analog of the binary's `--session-dir`.
    pub fn session_dir(mut self, session_dir: impl AsRef<Path>) -> Self {
        self.session_dir = Some(session_dir.as_ref().to_path_buf());
        self
    }

    /// Where `Logging`'s console stream goes — the stream that carries the `::`
    /// detailed log (`Minimizing: N states.`, `N reachable states`, …) and the
    /// `;`-mode step lines, byte-identical to what the binary prints. The default is a
    /// sink; pass e.g. a shared buffer to capture it, or `Box::new(std::io::stdout())`
    /// to see it. Independent of the command output [`Engine::run`] returns.
    pub fn console(mut self, console: Box<dyn Write>) -> Self {
        self.console = Some(console);
        self
    }

    /// Where `Logging`'s error stream goes (Java's `System.err`: non-Walnut exception
    /// traces). Default: a sink.
    pub fn err_console(mut self, err_console: Box<dyn Write>) -> Self {
        self.err_console = Some(err_console);
        self
    }

    /// An initial [`Instrumentation`]; same as calling [`Engine::set_instrumentation`]
    /// right after construction.
    pub fn instrumentation(mut self, instrumentation: Instrumentation) -> Self {
        self.instrumentation = instrumentation;
        self
    }

    /// A state/memory budget; same as [`Engine::set_budget`] after construction.
    pub fn budget(mut self, budget: ResourceBudget) -> Self {
        self.instrumentation = self.instrumentation.with_budget(budget);
        self
    }

    /// Build the engine. An I/O error is a failure to create the session tree; a
    /// [`MemoryMeterMissing`] (as an `io::Error` of kind `Unsupported`) means the
    /// requested memory cap cannot be enforced in this process — see
    /// `wr_core::resource`.
    pub fn build(self) -> io::Result<Engine> {
        let mut engine = Engine::build(
            self.session_dir.as_deref(),
            self.home_dir.as_deref(),
            self.console,
            self.err_console,
        )?;
        engine
            .set_instrumentation(self.instrumentation)
            .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e))?;
        Ok(engine)
    }
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
        Self::build(None, Some(home_dir.as_ref()), None, None)
    }

    /// As [`Engine::new`], but rooted at the process's current working directory —
    /// the binary's own no-`--home-dir` default.
    pub fn in_current_dir() -> io::Result<Self> {
        Self::build(None, None, None, None)
    }

    /// As [`Engine::new`], but with an explicit `session_dir` (no auto-generated
    /// timestamp subdirectory) — the analog of the binary's `--session-dir`.
    pub fn with_session_dir(
        home_dir: impl AsRef<Path>,
        session_dir: impl AsRef<Path>,
    ) -> io::Result<Self> {
        Self::build(
            Some(session_dir.as_ref()),
            Some(home_dir.as_ref()),
            None,
            None,
        )
    }

    /// The full set of construction options — console sinks for the detailed log, an
    /// initial budget/instrumentation, an explicit session directory. `home_dir` is as
    /// for [`Engine::new`].
    pub fn builder(home_dir: impl AsRef<Path>) -> EngineBuilder {
        EngineBuilder {
            home_dir: Some(home_dir.as_ref().to_path_buf()),
            session_dir: None,
            console: None,
            err_console: None,
            instrumentation: Instrumentation::new(),
        }
    }

    fn build(
        session_dir: Option<&Path>,
        home_dir: Option<&Path>,
        console: Option<Box<dyn Write>>,
        err_console: Option<Box<dyn Write>>,
    ) -> io::Result<Self> {
        let session_dir = session_dir.map(dir_arg);
        let home_dir = home_dir.map(dir_arg);
        let paths = SessionPaths::new(session_dir.as_deref(), home_dir.as_deref(), false);
        // `Session.createSubdirectories` (`Session.java:132`) — the same directory
        // setup the binary does; a failure here is an I/O error, not a query error.
        paths.create_subdirectories().map_err(io::Error::other)?;
        let out = SharedBuf::default();
        // Route `Logging`'s own console/detail streams to a sink unless the builder
        // supplied one; only the command's `System.out.print` output (the verdict) is
        // captured in `out`.
        let logging = Logging::with_writers(
            console.unwrap_or_else(|| Box::new(io::sink())),
            err_console.unwrap_or_else(|| Box::new(io::sink())),
        );
        let prover =
            Prover::with_output(Session::from_paths(paths), logging, Box::new(out.clone()));
        Ok(Engine {
            prover,
            out,
            base_instrumentation: Instrumentation::new(),
            trajectory: None,
        })
    }

    // ------------------------------------------------------------ instrumentation

    /// Install a resource budget (the in-engine `-Xmx` analog) for every later command.
    /// A breached cap makes that command return
    /// [`ProverError::ResourceExhausted`](crate::prover::ProverError::ResourceExhausted)
    /// with its partial automata already freed; the engine stays usable. Fails (and
    /// installs nothing) when a memory cap is requested but no tracking allocator is
    /// present — `wr_cli::tracking_alloc` for how to install one.
    pub fn set_budget(&mut self, budget: ResourceBudget) -> Result<(), MemoryMeterMissing> {
        let base = self.base_instrumentation.clone().with_budget(budget);
        self.set_instrumentation(base)
    }

    /// Install a full [`Instrumentation`] (budget plus the embedder's own observers).
    /// Replaces what was installed before; the engine's own trajectory recorder
    /// ([`Engine::record_trajectory`]) is kept alongside it.
    pub fn set_instrumentation(
        &mut self,
        instrumentation: Instrumentation,
    ) -> Result<(), MemoryMeterMissing> {
        instrumentation.validate()?;
        self.base_instrumentation = instrumentation;
        self.reinstall()
    }

    /// The embedder-supplied instrumentation currently installed.
    pub fn instrumentation(&self) -> &Instrumentation {
        &self.base_instrumentation
    }

    /// Record every construction event of each command into a [`Trajectory`] readable
    /// via [`Engine::trajectory`] — the peak-state / subset-construction trajectory of
    /// the *last* command run (cleared at the start of each). Off by default.
    pub fn record_trajectory(&mut self, on: bool) -> Result<(), MemoryMeterMissing> {
        if on && self.trajectory.is_none() {
            self.trajectory = Some(Rc::new(RefCell::new(Trajectory::new())));
        } else if !on {
            self.trajectory = None;
        }
        self.reinstall()
    }

    /// The trajectory of the last command, if [`Engine::record_trajectory`] is on. A
    /// snapshot: reading it does not clear it.
    pub fn trajectory(&self) -> Option<Trajectory> {
        self.trajectory.as_ref().map(|t| t.borrow().clone())
    }

    fn reinstall(&mut self) -> Result<(), MemoryMeterMissing> {
        let mut composed = self.base_instrumentation.clone();
        if let Some(t) = &self.trajectory {
            composed = composed.with_observer(t.clone());
        }
        self.prover.set_instrumentation(composed)
    }

    fn begin_command(&mut self) {
        if let Some(t) = &self.trajectory {
            t.borrow_mut().clear();
        }
    }

    // ------------------------------------------------------- in-memory automata

    /// Register an in-memory word automaton as `name` (usable as `name[i]` in every
    /// later formula on this engine), shadowing any `Word Automata Library/name.txt`.
    /// No `.txt` is written or parsed — this is how an already-minimal DFAO built on the
    /// ct-research substrate (`wr_cts::bridge::automaton_from_dfao`) enters the engine.
    /// The automaton must have the shape the reader produces for a word automaton
    /// (deterministic, one track per variable, `msd`/`ns_name` set on each track, a
    /// custom base's `all_reps` attached); the parts of that the engine relies on are
    /// checked and refused with a [`crate::session::RegistrationError`] rather than
    /// assumed.
    pub fn register_word_automaton(
        &mut self,
        name: &str,
        automaton: wr_core::automaton::Automaton,
    ) -> Result<(), crate::session::RegistrationError> {
        self.prover
            .session()
            .libraries()
            .register_word(name, automaton)
    }

    /// Register an in-memory predicate automaton as `name` (usable as `$name(…)`),
    /// shadowing any `Automata Library/name.txt`, with the same checks as
    /// [`Engine::register_word_automaton`]. A `def` result obtained through
    /// [`Engine::eval_structured`] is the typical source.
    pub fn register_automaton(
        &mut self,
        name: &str,
        automaton: wr_core::automaton::Automaton,
    ) -> Result<(), crate::session::RegistrationError> {
        self.prover
            .session()
            .libraries()
            .register_function(name, automaton)
    }

    /// Forget an in-memory registration of either kind.
    pub fn unregister_automaton(&mut self, name: &str) {
        self.prover.session().libraries().unregister(name);
    }

    // --------------------------------------------------------------------- logs

    /// The `::`-level detailed log of the last command, as text — every
    /// `computing cross product:`/`Minimizing:`/`N reachable states` line the binary
    /// would have printed for `<command>::`, without shelling out. Empty unless the
    /// command was run with the `::` suffix (the same gate as the binary), since that
    /// is what turns detailed logging on. For structured counts use
    /// [`Engine::record_trajectory`] instead.
    pub fn detailed_log(&self) -> String {
        self.prover.logging().detailed_log()
    }

    /// The per-step command log of the last command (the `;`-mode step lines).
    pub fn command_log(&self) -> String {
        self.prover.logging().command_log()
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
        self.begin_command();
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
        self.begin_command();
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
