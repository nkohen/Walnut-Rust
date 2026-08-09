// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Progress-reporting / log-buffer state, ported from `Main/Logging.java` (344 LOC).
//!
//! # Why this lives in `wr-core`, not `wr-cli` (U0a's crate-boundary call)
//!
//! `Logging.java` sits in Java's `Main/` package, which on its face reads as CLI/driver
//! scope. It is not: 121 of its call sites are inside `Automata/` (`Automaton`,
//! `AutomatonLogicalOps`, `AutomatonQuantification`, `AutomatonReader`,
//! `FA/DeterminizationStrategies`, `FA/FA`, `FA/ProductStrategies`, `NumberSystem`,
//! `Transducer`, `WordAutomaton`, `Writer/AutomatonWriter`) — i.e. `wr-core`'s own
//! algorithmic code calls it to emit exactly the `computing cross product:…`/
//! `Minimizing:…` progress lines that the golden `details*` fixtures assert on
//! verbatim. `wr-core` cannot depend on a `wr-cli`-resident version of this module
//! without inverting the crate DAG (`wr-cli` already depends on `wr-core`) — the same
//! incoming-edge-coupling mistake Phase 2's U6 found and fixed once already for
//! `AutomatonQuantification` (see `quantify.rs`'s module docs for that precedent).
//! `wr-logic` (`Operator`/`Word`/`Function`/`RelationalOperator`/`ArithmeticOperator`/
//! `LogicalOperator`/`NumberLiteralExpression`/`VariableExpression`) and `wr-cli`
//! (`EvalDef`, `Session`) both already depend on `wr-core` per the workspace
//! `Cargo.toml`, so placing this here (rather than a new shared leaf crate) costs
//! nothing and adds no new crate: every future caller already has a path to it.
//!
//! # Global statics → an owned struct (the sanctioned mechanical-fidelity deviation)
//!
//! Java's `Logging` is a `static`-only class: every field above is a class-level
//! static, mutated from wherever any of ~15 files happen to be running. Per
//! `CLAUDE.md`'s sanctioned deviation for exactly this shape (originally scoped for
//! `Session`, generalized here since `Session` hasn't landed yet but the doctrine is
//! the same), this is ported as an **owned context struct** ([`Logging`]) rather than
//! a Rust `static`/`static mut` — a later unit will thread one instance through as
//! part of a session/evaluation context (mirroring how `Session` will own the rest of
//! Java's other static-heavy classes). No call sites are wired up in this unit; that
//! is deliberate scope (see the unit's own Done-when) — this module only needs to be
//! correct and testable in isolation.
//!
//! # Two real behavioral asymmetries, preserved exactly (read before wiring call sites)
//!
//! These are easy to "fix" by accident during later units because they look like
//! bugs; they are not this unit's call to make (mechanical-port-first) and are pinned
//! by dedicated tests below so a later refactor can't silently erase them:
//!
//! 1. **[`Logging::log_message`] vs [`Logging::log_and_print`] differ in more than
//!    "does it print."** `logMessage`'s own guard (`printEnabled && print`) wraps the
//!    ENTIRE call into the shared `logDetail` helper — when it's false, `logDetail`
//!    never runs at all, so nothing is recorded anywhere: not the global log file, not
//!    the in-memory `commandLog`/`detailedLog` buffers. `logAndPrint` always calls
//!    `logDetail` and only gates the final console write inside it. So a
//!    disabled/non-printing `log_message` call is a complete no-op; the equivalent
//!    `log_and_print` call still records to the global log and (when applicable)
//!    the in-memory buffers, and only suppresses the console line.
//! 2. **[`Logging::log_detail`] (private; backs `log_message`/`log_and_print`) and
//!    [`Logging::log_evaluation_step`] disagree about the eval-log-files-active file
//!    write — but only for the COMMAND log, not the detailed log.** `log_detail`
//!    only appends to `command_log`/writes to the command-log file when
//!    `!eval_log_files_active`; its `detailed_log`/detailed-log-file write is gated
//!    solely by `print_details`, with no `eval_log_files_active` check at all. So
//!    while a [`CommandLogContext`] is open (i.e. during a `writeEvalLogsTo` block —
//!    Java's sole real call site is `EvalDef.evalDefCommand`'s
//!    `try (var ignored = Logging.writeEvalLogsTo(resultName)) { ... }`, whose body
//!    calls `c.compute(predicate)`, which is where the actual per-step `indent`/
//!    `logMessage`/`logAndPrint`/`logEvaluationStep` calls happen — both methods
//!    play a role: `evalDefCommand` opens the context, `compute` is what runs
//!    inside it), `log_message`/
//!    `log_and_print` calls made by nested automaton code reach the *detailed* log
//!    (buffer + file, if `print_details` is on) exactly as normal, but do **not**
//!    reach the per-eval *command*-log file or its in-memory buffer — only the
//!    global log file and (if gated) the console still receive them via that path.
//!    `log_evaluation_step` has no such guard on either buffer: it always appends to
//!    `command_log` (and, if `print_details`, `detailed_log`) and always writes both
//!    corresponding files, active or not. This reads as intentional (only top-level
//!    "step" lines belong in the per-eval *command* file; verbose nested detail
//!    lines would bloat it, but are exactly what the *detailed* file is for) rather
//!    than a bug — preserved verbatim either way.
//!
//! # `console`/`err_console`: modeling `slf4j`/`logback`'s observable behavior directly
//!
//! Java routes "print to the user" through three `slf4j` loggers configured by
//! `src/main/resources/logback.xml`. Read literally (confirmed from that file, not
//! assumed): `Walnut.Console` has a `ConsoleAppender` with pattern `%msg%n` — i.e.
//! `consoleLogger.info(msg)` is observably *exactly* `msg` followed by a newline on
//! stdout, nothing more (no timestamp/level/logger-name — that pattern is genuinely
//! bare). `Walnut.CommandLog`/`Walnut.DetailedLog` have **no default appender at all**
//! (the XML's own comment: "File appenders for these loggers are installed per
//! command by Main.Logging.") — so `commandLogger.info`/`detailedLogger.info` are
//! pure no-ops unless [`Logging::write_eval_logs_to`] has attached a file for the
//! duration. `addFileAppender` uses the same bare `%msg%n` pattern for those files
//! too. None of this needs `slf4j`/`logback` in Rust — it needs exactly two
//! behaviors: "write `msg\n` to an injectable stdout-like sink, unconditionally when
//! called" (`console`, backing every `consoleLogger.info` call site here) and "write
//! `msg\n` to a file if one is currently attached, else do nothing"
//! ([`Logging::command_log_file`]/[`Logging::detailed_log_file`]). `console`/
//! `err_console` are injectable (not hardcoded to `io::stdout()`/`io::stderr()`) so
//! tests can pin the literal text without capturing real process streams.
//!
//! # Line separator: hardcoded to `\n`, a documented Windows fidelity gap
//!
//! Java's `BufferedWriter.newLine()` (used by every global-log write) and
//! `System.lineSeparator()` (used by `append`/`appendLine`, i.e. the in-memory
//! `command_log`/`detailed_log` buffers) both resolve to `\r\n` on Windows and `\n`
//! on Unix-likes. This port hardcodes `\n` everywhere (`write_all(b"\n")`,
//! `format!("...{command}\n")`, `String::push('\n')`) rather than branching on
//! `cfg(windows)`. This is a deliberate, documented limitation, not a silent gap:
//! `walnut-rs`'s own dev/CI environment is Unix-only in practice (see `CLAUDE.md`),
//! so a `cfg(windows)` branch here would be untested scope creep with no current
//! consumer. If Windows support is ever actually needed, every hardcoded `b"\n"`/
//! `'\n'` in this file's I/O and buffer-append paths (not the plain in-message
//! `\n`s that originate from caller-supplied text, e.g. inside `rendered` in
//! [`Logging::print_truncated_stack_trace_with_length`], which Java also never
//! platform-varies) would need to become `cfg`-gated.
//!
//! # `indent()`/`dedent()`: Java's `String.repeat` crashes on a negative count
//!
//! `indentCount` is a bare Java `int` with no floor guard in `dedent()`. If a caller
//! ever dedents more than it indented, `" ".repeat(indentCount)` — called from
//! `logDetail`/`logEvaluationStep` on the next log line — throws
//! `IllegalArgumentException("count is negative: " + count)`. [`Logging::indent_prefix`]
//! matches that trigger condition exactly (a Rust `panic!`, message text included,
//! on the next logged line rather than at `dedent()` itself) rather than silently
//! clamping to zero, per `CLAUDE.md`'s "port quirks/crashes verbatim" rule —
//! clamping would be a real, silent behavioral divergence (Java crashes the command;
//! a clamped Rust port would instead silently keep going with less indentation).
//!
//! **This is NOT actually behaviorally verbatim, though, and an earlier draft of
//! this doc incorrectly claimed it was.** Java's `IllegalArgumentException` is a
//! `RuntimeException` thrown from inside `Prover.dispatch(s)`, which
//! `Prover.readBuffer`'s command loop wraps in its own `catch (RuntimeException e)`
//! (`Prover.java:387-390`) — so in real Java, an unbalanced `dedent()` prints a
//! truncated stack trace via `Logging.printTruncatedStackTrace` and the interactive
//! session survives to read the next command. A Rust `panic!` here unwinds the
//! whole stack instead, and — absent a `catch_unwind` (or `Result`-based error) at
//! a CLI command-dispatch boundary that does not exist anywhere in this port yet —
//! that unwind is not caught anywhere and kills the process. That is the opposite of
//! Java's actual recovery behavior, not an equivalent crash.
//!
//! As it stands today this divergence is latent, not live: no call site wired up
//! in any unit through Phase 2 calls `dedent()` more times than `indent()` for a
//! given command (verified by tracing every `Logging.dedent`/`Logging.indent` call
//! site in `walnut-java`), and this unit wires up no call sites at all (see above).
//! So this is not logged as a Walnut bug (the underlying Java behavior — recovering
//! via an exception handler — is correct and intentional; nothing in Java is wrong
//! here) and not fixed here (fixing it means deciding how command dispatch reports
//! errors in `wr-cli`, which does not exist yet — premature to design against). It
//! is flagged here so that whichever future unit wires `indent`/`dedent` into real
//! automaton code, or builds `wr-cli`'s command dispatch loop, treats
//! "`indent_prefix` can panic" as a real open problem needing a `catch_unwind` or
//! `Result`-based error boundary at that call site, not as an already-solved one.
//!
//! # `print_truncated_stack_trace`: an intentional, documented fidelity limit
//!
//! Java's two-argument form mutates the exception's own `StackTraceElement[]` to its
//! first `length` frames, then calls `e.printStackTrace()` **twice** — once into a
//! `StringWriter` (captured to the global log) and once to the real
//! `System.err` (note: **stderr**, unlike every other console write in this file,
//! which goes to `System.out`). The single-argument overload (the only one any real
//! call site uses — verified: all 18 Java call sites in `src/main` pass exactly 1
//! argument; recount with `grep -rn "Logging.printTruncatedStackTrace" src/main`)
//! hardcodes `length = 1`. The *routing* (handled-vs-not, global-log capture, stdout-vs-stderr
//! split, frame-count truncation) is mechanical and is ported exactly. The *frame
//! text itself* (`ClassName.method(File.java:line)`, tied to walnut-java's actual
//! JVM bytecode/call stack) has no meaningful Rust analogue — a Rust panic/backtrace
//! has different frames entirely, and golden fixtures cannot plausibly assert JVM
//! stack-frame text against a Rust binary. So the "is this a handled
//! `WalnutException` or not" triage is exposed as the [`LoggableError`] trait (a
//! future `WalnutException`-equivalent Rust error type — Phase 3b's `U21` — should
//! implement it) rather than requiring an unported Java exception hierarchy here, and
//! the "not handled" branch renders whatever `stack_trace_lines` the caller supplies
//! instead of reproducing JVM frame syntax. This is a documented, unavoidable
//! environment-fidelity limit, not a silently-resolved divergence.
//!
//! # `CommandLogContext`: simplified relative to Java's multi-appender model
//!
//! Java's `addFileAppender` *adds* an appender to a shared logger (SLF4J loggers can
//! carry more than one); closing detaches only the one it added, so nested
//! `writeEvalLogsTo` calls would, in real Java, leave both files receiving writes
//! simultaneously. Verified (grep): the only real call site is
//! `EvalDef.evalDefCommand`'s single, non-nested
//! `try (var ignored = Logging.writeEvalLogsTo(resultName)) { ... }` block, whose
//! body calls `c.compute(predicate)` for the actual per-step logging. This port
//! models "current eval-log files" as two plain `Option<BufWriter<File>>` fields on
//! [`Logging`] itself (swapped out and restored by the guard), which is exactly
//! equivalent to Java's behavior for the real non-nested case and a documented,
//! deliberate simplification for the hypothetical (never-exercised) nested case.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

// ---------------------------------------------------------------------------------
// Constants — verbatim from `Logging.java`'s `public static final String` fields.
// ---------------------------------------------------------------------------------

pub const APPLIED: &str = "applied";
pub const APPLYING: &str = "applying";

pub const COMPARED: &str = "compared";
pub const COMPARING: &str = "comparing";

pub const COMPUTED: &str = "computed";
pub const COMPUTING: &str = "computing";

pub const FIXED: &str = "fixed";
pub const FIXING: &str = "fixing";

pub const QUANTIFIED: &str = "quantified";
pub const QUANTIFYING: &str = "quantifying";

pub const REMOVED: &str = "removed";
pub const REMOVING: &str = "removing";

pub const REVERSED: &str = "reversed";
pub const REVERSING: &str = "reversing";

pub const TOTALIZED: &str = "totalized";
pub const TOTALIZING: &str = "totalizing";

// note caps. This is historical, and just to avoid changing lots of code. (Java's own
// comment, ported verbatim along with the inconsistency it explains.)

pub const CONVERTED: &str = "Converted";
pub const CONVERTING: &str = "Converting";

pub const DETERMINIZED: &str = "Determinized";
pub const DETERMINIZING: &str = "Determinizing";

pub const MINIMIZED: &str = "Minimized";
pub const MINIMIZING: &str = "Minimizing";

pub const GLOBAL_LOG_FILENAME: &str = "global_log.txt";

/// Ported analogue of Java's `Exception` parameter to `printTruncatedStackTrace`. A
/// future `WalnutException`-equivalent Rust error type should implement this; see the
/// module docs' section on this function for why the frame text itself is not
/// mechanically portable.
pub trait LoggableError {
    /// Java: `e instanceof WalnutException` — the message-only branch.
    fn is_handled(&self) -> bool;
    /// Java: `e.getMessage()`, which is nullable — a `WalnutException(null)` is a
    /// real constructible Java value, and `null` prints/records differently on the
    /// two sinks `printTruncatedStackTrace` uses (see [`Logging::console_write`]'s
    /// caller here: `None` renders as the literal `"null"` on the console, matching
    /// `System.out.println` on a null argument, but as an empty line in the global
    /// log, matching `writeGlobalLogLine`'s own `if (msg == null) msg = "";` guard).
    /// `None` here corresponds exactly to a null Java message; do not conflate it
    /// with `Some(String::new())` (an empty-but-non-null message), which Java (and
    /// this port) render identically on both sinks.
    fn message(&self) -> Option<String>;
    /// Java: `e.getClass().getName()` — the class-name half of `Throwable.toString()`
    /// (`getClass().getName() + ": " + getLocalizedMessage()`), which is the header
    /// line `Throwable.printStackTrace()` emits before any frames. Only consulted
    /// when `!is_handled()`; see [`Logging::print_truncated_stack_trace_with_length`].
    fn kind(&self) -> String;
    /// Java: `e.getStackTrace()`, one rendered frame per entry, deepest-first (as
    /// `Throwable.printStackTrace()` would emit them). Only consulted when
    /// `!is_handled()`.
    fn stack_trace_lines(&self) -> Vec<String>;
}

/// Owned analogue of `Main.Logging`'s static state. See the module docs for the
/// crate-placement and global-statics-to-struct rationale.
pub struct Logging {
    // `Box<dyn Write>`, not `Option<BufWriter<File>>`: this is the injection seam
    // that lets tests simulate the writer itself failing mid-write (Java's
    // `ThrowingWriter`-backed `ioExceptionsFromTheWriter_areSwallowedEverywhere`
    // test) without needing a real unwritable file. Every real caller still gets a
    // `BufWriter<File>` boxed up by [`Logging::initialize_global_log`].
    global_log_writer: Option<Box<dyn Write>>,
    global_log_has_content: bool,

    print_steps: bool,
    print_details: bool,
    eval_log_files_active: bool,
    command_log: String,
    detailed_log: String,
    indent_count: i32,
    print_enabled: bool,

    command_log_file: Option<BufWriter<File>>,
    detailed_log_file: Option<BufWriter<File>>,

    console: Box<dyn Write>,
    err_console: Box<dyn Write>,
}

impl Default for Logging {
    fn default() -> Self {
        Self::new()
    }
}

impl Logging {
    /// Fresh state matching Java's static-field initializers: printing disabled,
    /// empty buffers, zero indent, printing globally *enabled* (`printEnabled =
    /// true` in Java — printing is gated by `printSteps`/`printDetails`, not by this
    /// flag, which exists only for [`Logging::disable_print`]/
    /// [`Logging::enable_print`]'s "temporarily disable for helper calls" use), no
    /// global log file. `console`/`err_console` default to the real process streams.
    pub fn new() -> Self {
        Self::with_writers(Box::new(io::stdout()), Box::new(io::stderr()))
    }

    /// As [`Logging::new`], but with injectable console/stderr sinks — the seam this
    /// module's own tests use to pin literal output text without capturing real
    /// process streams; also usable later by `wr-cli` (e.g. a headless/non-TTY mode).
    pub fn with_writers(console: Box<dyn Write>, err_console: Box<dyn Write>) -> Self {
        Self {
            global_log_writer: None,
            global_log_has_content: false,
            print_steps: false,
            print_details: false,
            eval_log_files_active: false,
            command_log: String::new(),
            detailed_log: String::new(),
            indent_count: 0,
            print_enabled: true,
            command_log_file: None,
            detailed_log_file: None,
            console,
            err_console,
        }
    }

    // ------------------------------------------------------------- global log file

    /// Java: `initializeGlobalLog`. Closes any previously-open global log, creates
    /// `filename`'s parent directories if it has any, then (re)creates `filename`
    /// truncated for writing. On any I/O failure, prints
    /// `"Could not create global log file {filename}"` to the console — matching
    /// Java's direct (ungated) `System.out.println`, not the `printEnabled`-gated
    /// path other methods use.
    pub fn initialize_global_log(&mut self, filename: &str) {
        self.close_global_log_writer();
        let path = Path::new(filename);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                // Java's createDirectories failure and the newBufferedWriter failure
                // share one catch block below; a mkdir failure here just falls
                // through to the same File::create failure and the same message.
                let _ = fs::create_dir_all(parent);
            }
        }
        match File::create(path) {
            Ok(f) => {
                self.global_log_writer = Some(Box::new(BufWriter::new(f)));
                self.global_log_has_content = false;
            }
            Err(_) => {
                self.console_write(&format!("Could not create global log file {filename}"));
            }
        }
    }

    fn close_global_log_writer(&mut self) {
        // Dropping the boxed writer (a real `BufWriter<File>`, best-effort-flushed
        // and discarded on `Drop`) stands in for Java's `catch (IOException ignored)`
        // around `writer.close()`: any failure here is unobservable either way.
        self.global_log_writer = None;
    }

    /// Test-only injection seam: install an arbitrary [`Write`] implementation (e.g.
    /// one whose `write`/`flush` always fail) as the global log "writer", so tests
    /// can exercise the "an I/O failure on the writer itself is swallowed" paths in
    /// [`Logging::log_command`]/[`Logging::write_global_log_line`] (Java's
    /// `ioExceptionsFromTheWriter_areSwallowedEverywhere`, backed by a
    /// `ThrowingWriter`) without needing a real unwritable file on disk.
    #[cfg(test)]
    fn set_global_log_writer_for_test(&mut self, writer: Box<dyn Write>) {
        self.global_log_writer = Some(writer);
        self.global_log_has_content = false;
    }

    /// Java: `logCommand`. A no-op if no global log is open. Otherwise writes a
    /// blank-line separator (if the log already has content), then `>>> {command}`
    /// on its own line, flushing immediately. `command = None` matches Java's
    /// `command == null` → `""` fallback.
    pub fn log_command(&mut self, command: Option<&str>) {
        let command = command.unwrap_or("");
        if self.global_log_writer.is_none() {
            return;
        }
        let has_content = self.global_log_has_content;
        let result: io::Result<()> = (|| {
            let writer = self.global_log_writer.as_mut().unwrap();
            if has_content {
                writer.write_all(b"\n")?;
            }
            writer.write_all(format!(">>> {command}\n").as_bytes())?;
            writer.flush()
        })();
        if result.is_ok() {
            self.global_log_has_content = true;
        }
    }

    /// Java: `writeGlobalLogLine` (private). A no-op if no global log is open;
    /// otherwise writes `msg` plus one trailing newline and flushes. On success,
    /// marks the log as having content (matches Java exactly: a failed write does
    /// not flip `globalLogHasContent`).
    fn write_global_log_line(&mut self, msg: &str) {
        if self.global_log_writer.is_none() {
            return;
        }
        let result: io::Result<()> = (|| {
            let writer = self.global_log_writer.as_mut().unwrap();
            writer.write_all(msg.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()
        })();
        if result.is_ok() {
            self.global_log_has_content = true;
        }
    }

    // --------------------------------------------------------------- command setup

    /// Java: `configureForCommand`. Resets the per-command printing flags and clears
    /// both in-memory log buffers. Does **not** touch `indent_count` (Java's
    /// `Session.resetIndent()` is a separate, deliberate call) or `print_enabled`
    /// (only `disable_print`/`enable_print` touch that), and does not close any
    /// currently-open eval-log files — it just flips the flag, matching Java's own
    /// unconditional `evalLogFilesActive = false` reassignment (which likewise
    /// doesn't detach any lingering file appender).
    pub fn configure_for_command(&mut self, should_print_steps: bool, should_print_details: bool) {
        self.print_steps = should_print_steps;
        self.print_details = should_print_details;
        self.eval_log_files_active = false;
        self.command_log.clear();
        self.detailed_log.clear();
    }

    /// Java: `shouldPrintDetails`.
    pub fn should_print_details(&self) -> bool {
        self.print_enabled && self.print_details
    }

    /// Java: `shouldPrintStepsOrDetails`.
    pub fn should_print_steps_or_details(&self) -> bool {
        self.print_enabled && (self.print_steps || self.print_details)
    }

    /// Java: `getCommandLog`.
    pub fn command_log(&self) -> String {
        self.command_log.clone()
    }

    /// Java: `getDetailedLog` — returns `""` unless `print_details` is set, even if
    /// the buffer itself has content (matches Java exactly: the buffer can be
    /// written to internally regardless, `getDetailedLog` just hides it).
    pub fn detailed_log(&self) -> String {
        if self.print_details {
            self.detailed_log.clone()
        } else {
            String::new()
        }
    }

    // -------------------------------------------------------------- eval log files

    /// Java: `EvalDef.evalDefCommand`'s
    /// `try (var ignored = Logging.writeEvalLogsTo(resultName)) { ... }` block,
    /// expressed as a scoped closure rather than a bare guard value — see
    /// [`Logging::write_eval_logs_to`]'s docs for why the guard form alone is a
    /// footgun for real callers. `f` runs with the eval-log files installed and
    /// receives `&mut Logging` so it can make the dozens of nested `log_message`/
    /// `log_and_print`/`log_evaluation_step` calls `EvalDef.compute` makes over the
    /// course of one evaluation; the files are closed (and the previous eval-log
    /// state restored) when `f` returns, mirroring Java's try-with-resources exactly
    /// — including on an early return via `?`/panic unwinding through `f`, since
    /// closing is still handled by [`CommandLogContext`]'s `Drop` in that case.
    pub fn with_eval_logs_to<R>(
        &mut self,
        result_name: &str,
        f: impl FnOnce(&mut Logging) -> R,
    ) -> R {
        let mut ctx = self.write_eval_logs_to(result_name);
        f(ctx.logging_mut())
    }

    /// Java: `writeEvalLogsTo`. Opens `{result_name}_log.txt` (always) and, if
    /// `print_details` is currently set, `{result_name}_detailed_log.txt` too;
    /// installs both as this `Logging`'s active eval-log files for the returned
    /// guard's lifetime (or until [`CommandLogContext::close`] is called explicitly).
    /// A file that fails to open (I/O error) is treated the same as Java's silent
    /// appender-attach failure: logging to it becomes a no-op rather than a panic
    /// (see the "logging failures must not interfere with Walnut commands" pattern
    /// used throughout this file).
    ///
    /// Returns a guard, not `()` — dropping it immediately (e.g. the natural
    /// bare-statement translation `logging.write_eval_logs_to(&name);`) closes the
    /// eval-log files on the very next line, silently producing empty log files, so
    /// this is `#[must_use]`. Prefer [`Logging::with_eval_logs_to`] unless you
    /// specifically need to hold the guard across control flow that a closure can't
    /// express; use [`CommandLogContext::logging_mut`] to reach `self` through it.
    #[must_use = "dropping this immediately closes the eval-log files; hold the guard \
                  for the scope of the evaluation, or use `with_eval_logs_to` instead"]
    pub fn write_eval_logs_to(&mut self, result_name: &str) -> CommandLogContext<'_> {
        let command_file = File::create(format!("{result_name}_log.txt"))
            .ok()
            .map(BufWriter::new);
        let detailed_file = if self.print_details {
            File::create(format!("{result_name}_detailed_log.txt"))
                .ok()
                .map(BufWriter::new)
        } else {
            None
        };

        let previous_eval_log_files_active = self.eval_log_files_active;
        let previous_command_log_file = self.command_log_file.take();
        let previous_detailed_log_file = self.detailed_log_file.take();

        self.command_log_file = command_file;
        self.detailed_log_file = detailed_file;
        self.eval_log_files_active = true;

        CommandLogContext {
            logging: self,
            previous_eval_log_files_active,
            previous_command_log_file,
            previous_detailed_log_file,
            closed: false,
        }
    }

    // ------------------------------------------------------------------- indent

    /// Java: `indent`.
    pub fn indent(&mut self) {
        self.indent_count += 1;
    }

    /// Java: `dedent`. See the module docs: an unbalanced `dedent()` can drive
    /// `indent_count` negative, which panics on the *next* logged line rather than
    /// here (matching Java, where `dedent()` itself never touches
    /// `" ".repeat(...)`).
    pub fn dedent(&mut self) {
        self.indent_count -= 1;
    }

    /// Java: `resetIndent` ("useful for integration tests").
    pub fn reset_indent(&mut self) {
        self.indent_count = 0;
    }

    fn indent_prefix(&self) -> String {
        assert!(
            self.indent_count >= 0,
            "count is negative: {}",
            self.indent_count
        );
        " ".repeat(self.indent_count as usize)
    }

    // -------------------------------------------------------------- print toggle

    /// Java: `disablePrint` ("temporarily disable print for helper calls").
    pub fn disable_print(&mut self) {
        self.print_enabled = false;
    }

    /// Java: `enablePrint`.
    pub fn enable_print(&mut self) {
        self.print_enabled = true;
    }

    // ------------------------------------------------------------------ logging

    /// Java: `logMessage(String)` — `logMessage(printDetails, msg)`.
    pub fn log_message(&mut self, msg: &str) {
        let print = self.print_details;
        self.log_message_with(print, msg);
    }

    /// Java: `logMessage(boolean, String)`. See the module docs' asymmetry #1: when
    /// `!(print_enabled && print)`, this is a **complete no-op** — nothing is
    /// recorded anywhere, not even the global log.
    pub fn log_message_with(&mut self, print: bool, msg: &str) {
        if self.print_enabled && print {
            self.log_detail(msg, true);
        }
    }

    /// Java: `logAndPrint(String)` — `logAndPrint(printDetails, msg)`.
    pub fn log_and_print(&mut self, msg: &str) {
        let print = self.print_details;
        self.log_and_print_with(print, msg);
    }

    /// Java: `logAndPrint(boolean, String)`. Unlike [`Logging::log_message_with`],
    /// always records to the global log (and, subject to the usual gates, the
    /// buffers) — `print` only gates the final console write. See asymmetry #1.
    pub fn log_and_print_with(&mut self, print: bool, msg: &str) {
        self.log_detail(msg, print);
    }

    /// Java: `logEvaluationStep`. Unlike [`Logging::log_detail`], always appends to
    /// `command_log` and always writes the command-log file if one is open,
    /// regardless of `eval_log_files_active` — see the module docs' asymmetry #2.
    ///
    /// `final_line` (Java: `append(log, msg, finalLine)`) is easy to misread:
    /// `final_line = false` terminates `msg` with a trailing newline in the
    /// in-memory buffers (a normal, complete log line — used for every
    /// intermediate step); `final_line = true` appends `msg` with **no** trailing
    /// newline, leaving the buffer ending mid-line — used for the one truly last
    /// call for a given command (`EvalDef.compute`'s real usage: each loop
    /// iteration logs a step with `final_line = false`, then the closing "Total
    /// computation time: …ms." line uses `final_line = true`). It is not a
    /// "keep extending this one line across calls" flag. The file write and the
    /// console write are unaffected either way — both always end in a newline,
    /// since each call is its own independent write/log event on those sinks.
    pub fn log_evaluation_step(&mut self, msg: &str, final_line: bool) {
        let msg_with_indent = format!("{}{msg}", self.indent_prefix());

        Self::append(&mut self.command_log, &msg_with_indent, final_line);
        self.command_logger_write(&msg_with_indent);
        self.write_global_log_line(&msg_with_indent);

        if self.print_details {
            Self::append(&mut self.detailed_log, &msg_with_indent, final_line);
            self.detailed_logger_write(&msg_with_indent);
        }

        if self.should_print_steps_or_details() {
            self.console_write(&msg_with_indent);
        }
    }

    /// Java: `logDetail` (private, backs `logMessage`/`logAndPrint`).
    fn log_detail(&mut self, msg: &str, print: bool) {
        let msg_with_indent = format!("{}{msg}", self.indent_prefix());
        self.write_global_log_line(&msg_with_indent);

        if self.print_details {
            Self::append(&mut self.detailed_log, &msg_with_indent, false);
            self.detailed_logger_write(&msg_with_indent);
        }

        if !self.eval_log_files_active {
            Self::append(&mut self.command_log, &msg_with_indent, false);
            self.command_logger_write(&msg_with_indent);
        }

        if self.print_enabled && print {
            self.console_write(&msg_with_indent);
        }
    }

    /// Java: `append`/`appendLine` (private). `appendLine(log, msg)` is
    /// `append(log, msg, false)`.
    fn append(log: &mut String, msg: &str, final_line: bool) {
        log.push_str(msg);
        if !final_line {
            log.push('\n');
        }
    }

    /// Analogue of `commandLogger.info(msg)`: a no-op unless
    /// [`Logging::write_eval_logs_to`] currently has a command-log file open (see
    /// the module docs on why `logback`'s no-default-appender behavior is modeled
    /// this way). Errors are swallowed, matching `logback`'s own internal error
    /// handling never propagating to the caller.
    fn command_logger_write(&mut self, msg: &str) {
        if let Some(w) = self.command_log_file.as_mut() {
            let _ = writeln!(w, "{msg}");
            let _ = w.flush(); // Java's FileAppender: setImmediateFlush(true).
        }
    }

    /// Analogue of `detailedLogger.info(msg)`. See [`Logging::command_logger_write`].
    fn detailed_logger_write(&mut self, msg: &str) {
        if let Some(w) = self.detailed_log_file.as_mut() {
            let _ = writeln!(w, "{msg}");
            let _ = w.flush();
        }
    }

    /// Analogue of `consoleLogger.info(msg)` (and of Java's direct, ungated
    /// `System.out.println(msg)` call sites — both land on the same physical stdout
    /// stream with the same `%msg%n`-equivalent text; see the module docs). Always
    /// writes when called; every caller here has already applied whatever gate
    /// Java's own code applies before calling in.
    fn console_write(&mut self, msg: &str) {
        let _ = writeln!(self.console, "{msg}");
    }

    // ------------------------------------------------------- exception reporting

    /// Java: `printTruncatedStackTrace(Exception)` — `printTruncatedStackTrace(e, 1)`.
    pub fn print_truncated_stack_trace(&mut self, e: &dyn LoggableError) {
        self.print_truncated_stack_trace_with_length(e, 1);
    }

    /// Java: `printTruncatedStackTrace(Exception, int)`. See the module docs' section
    /// on this method for the documented fidelity limit (JVM frame text has no Rust
    /// analogue) and the stdout/stderr split this preserves exactly.
    pub fn print_truncated_stack_trace_with_length(
        &mut self,
        e: &dyn LoggableError,
        length: usize,
    ) {
        if e.is_handled() {
            // "handled Walnut exception; only print message" (Java's own comment).
            let message = e.message();
            // System.out.println(e.getMessage()): println on a null argument prints
            // the literal 4-character string "null" (Java's PrintStream.println(Object)
            // routes a null reference through String.valueOf, which special-cases null
            // to "null" rather than throwing).
            let console_text = message.clone().unwrap_or_else(|| "null".to_string());
            self.console_write(&console_text);
            // writeGlobalLogLine has its own `if (msg == null) msg = "";` guard —
            // a null message becomes an empty logged line here, NOT "null". This
            // asymmetry with the console line above is real and intentional (see the
            // module docs).
            let global_text = message.unwrap_or_default();
            self.write_global_log_line(&global_text);
        } else {
            let lines = e.stack_trace_lines();
            let keep = length.min(lines.len());
            // Java: `Throwable.printStackTrace()`'s header line is
            // `Throwable.toString()` = `getClass().getName() + ": " + getLocalizedMessage()`
            // when the message is non-null, or just `getClass().getName()` when it is
            // null (no trailing ": null" — Throwable.toString() special-cases this,
            // unlike the println-on-null case in the handled branch above).
            let mut rendered = match e.message() {
                Some(m) => format!("{}: {m}", e.kind()),
                None => e.kind(),
            };
            for line in &lines[..keep] {
                rendered.push('\n');
                rendered.push_str("\tat ");
                rendered.push_str(line);
            }
            self.write_global_log_line(rendered.trim_end());
            // Java: `e.printStackTrace()` (no-arg) — System.err, not System.out.
            let _ = writeln!(self.err_console, "{rendered}");
        }
    }
}

/// Java: `Logging.CommandLogContext` (an `AutoCloseable`). Returned by
/// [`Logging::write_eval_logs_to`]; restores the previous eval-log-file state either
/// when [`CommandLogContext::close`] is called explicitly or when this value is
/// dropped (Rust's RAII stands in for Java's try-with-resources), whichever happens
/// first — `close` is idempotent, matching Java's `if (closed) return;` guard.
pub struct CommandLogContext<'a> {
    logging: &'a mut Logging,
    previous_eval_log_files_active: bool,
    previous_command_log_file: Option<BufWriter<File>>,
    previous_detailed_log_file: Option<BufWriter<File>>,
    closed: bool,
}

impl CommandLogContext<'_> {
    /// Reach the underlying [`Logging`] while the eval-log-file context is open —
    /// the accessor a real caller needs (Java's sole real call site is
    /// `EvalDef.evalDefCommand`'s `try (var ignored = Logging.writeEvalLogsTo(...))`
    /// block, whose body calls `compute`, which makes dozens of `Logging` calls over
    /// the course of one evaluation). The field itself is private specifically so
    /// nothing outside this module can reach `self` without going through here (or
    /// [`Logging::with_eval_logs_to`]).
    pub fn logging_mut(&mut self) -> &mut Logging {
        self.logging
    }

    /// Java: `CommandLogContext.close`.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        // Dropping the current Some(...) here (by overwriting the field) flushes and
        // closes it, standing in for Java's `appender.stop()`.
        self.logging.command_log_file = self.previous_command_log_file.take();
        self.logging.detailed_log_file = self.previous_detailed_log_file.take();
        self.logging.eval_log_files_active = self.previous_eval_log_files_active;
        self.closed = true;
    }
}

impl Drop for CommandLogContext<'_> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::{Arc, Mutex};

    /// A `Write` sink that hands its bytes back out through a shared buffer, so tests
    /// can inspect exactly what `Logging` wrote to "the console" without touching the
    /// real process stdout/stderr.
    #[derive(Clone)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn logging_with_capture() -> (Logging, SharedBuf, SharedBuf) {
        let out = SharedBuf::new();
        let err = SharedBuf::new();
        let logging = Logging::with_writers(Box::new(out.clone()), Box::new(err.clone()));
        (logging, out, err)
    }

    fn read_file(path: &str) -> String {
        let mut s = String::new();
        File::open(path).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    /// A per-invocation-unique tag: `std::process::id()` alone is not enough for
    /// tests that assert a file/dir does NOT exist, since OS PIDs get reused across
    /// separate `cargo test` process runs — two runs landing on the same PID and
    /// requesting `tmp_path` with the same `name` would otherwise collide on the same
    /// path. A monotonic in-process counter plus a wall-clock nanosecond timestamp
    /// makes collisions practically impossible even under PID reuse.
    fn unique_suffix() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{nanos}-{n}")
    }

    fn tmp_path(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "wr-core-logging-test-{}-{}-{}",
            std::process::id(),
            unique_suffix(),
            name
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name).to_str().unwrap().to_string()
    }

    /// Like [`tmp_path`], but returns a path that is itself an already-existing
    /// *directory* (not a file path inside one) — used to exercise
    /// `initialize_global_log`'s I/O-failure branch (`File::create` on a path that is
    /// a directory fails, unlike a plain missing file).
    fn tmp_existing_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "wr-core-logging-test-dir-{}-{}-{}",
            std::process::id(),
            unique_suffix(),
            name
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.to_str().unwrap().to_string()
    }

    // ------------------------------------------------------------------- constants

    #[test]
    fn constants_match_java_verbatim() {
        assert_eq!(APPLIED, "applied");
        assert_eq!(APPLYING, "applying");
        assert_eq!(COMPARED, "compared");
        assert_eq!(COMPARING, "comparing");
        assert_eq!(COMPUTED, "computed");
        assert_eq!(COMPUTING, "computing");
        assert_eq!(FIXED, "fixed");
        assert_eq!(FIXING, "fixing");
        assert_eq!(QUANTIFIED, "quantified");
        assert_eq!(QUANTIFYING, "quantifying");
        assert_eq!(REMOVED, "removed");
        assert_eq!(REMOVING, "removing");
        assert_eq!(REVERSED, "reversed");
        assert_eq!(REVERSING, "reversing");
        assert_eq!(TOTALIZED, "totalized");
        assert_eq!(TOTALIZING, "totalizing");
        // Historically capitalized, ported verbatim (see the module's const-block comment).
        assert_eq!(CONVERTED, "Converted");
        assert_eq!(CONVERTING, "Converting");
        assert_eq!(DETERMINIZED, "Determinized");
        assert_eq!(DETERMINIZING, "Determinizing");
        assert_eq!(MINIMIZED, "Minimized");
        assert_eq!(MINIMIZING, "Minimizing");
        assert_eq!(GLOBAL_LOG_FILENAME, "global_log.txt");
    }

    // --------------------------------------------------------------- default state

    #[test]
    fn default_state_prints_nothing_and_indents_zero() {
        let (logging, _out, _err) = logging_with_capture();
        assert!(!logging.should_print_details());
        assert!(!logging.should_print_steps_or_details());
        assert_eq!(logging.command_log(), "");
        assert_eq!(logging.detailed_log(), "");
    }

    #[test]
    fn configure_for_command_sets_flags_and_clears_buffers() {
        let (mut logging, _out, _err) = logging_with_capture();
        logging.log_and_print("stale"); // populate command_log
        assert_ne!(logging.command_log(), "");

        logging.configure_for_command(true, true);
        assert!(logging.should_print_steps_or_details());
        assert!(logging.should_print_details());
        assert_eq!(logging.command_log(), "");
        assert_eq!(logging.detailed_log(), "");
    }

    #[test]
    fn print_enabled_gates_both_should_print_queries() {
        let (mut logging, _out, _err) = logging_with_capture();
        logging.configure_for_command(true, true);
        assert!(logging.should_print_details());
        assert!(logging.should_print_steps_or_details());

        logging.disable_print();
        assert!(!logging.should_print_details());
        assert!(!logging.should_print_steps_or_details());

        logging.enable_print();
        assert!(logging.should_print_details());
        assert!(logging.should_print_steps_or_details());
    }

    // ----------------------------------------------------------------------- indent

    #[test]
    fn indent_prefixes_logged_lines_with_matching_spaces() {
        let (mut logging, out, _err) = logging_with_capture();
        // print_details=true: log_and_print's implicit `print` flag is
        // `print_details` (see log_message-vs-log_and_print asymmetry docs), so this
        // is what makes the console actually receive these lines.
        logging.configure_for_command(false, true);

        logging.log_and_print("top");
        logging.indent();
        logging.log_and_print("nested");
        logging.indent();
        logging.log_and_print("deeper");
        logging.dedent();
        logging.dedent();
        logging.log_and_print("back to top");

        assert_eq!(
            out.text(),
            "top\n nested\n  deeper\nback to top\n",
            "each indent level should add exactly one space"
        );
    }

    #[test]
    fn reset_indent_zeroes_the_counter() {
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(false, true); // print_details=true, see above
        logging.indent();
        logging.indent();
        logging.reset_indent();
        logging.log_and_print("x");
        assert_eq!(out.text(), "x\n");
    }

    #[test]
    #[should_panic(expected = "count is negative: -1")]
    fn unbalanced_dedent_panics_on_the_next_logged_line_like_javas_string_repeat() {
        let (mut logging, _out, _err) = logging_with_capture();
        logging.configure_for_command(true, false);
        logging.dedent(); // indent_count now -1; dedent() itself does not panic...
        logging.log_and_print("boom"); // ...but the next logged line does.
    }

    // --------------------------------------------------- log_message vs log_and_print

    #[test]
    fn log_message_is_a_complete_noop_when_not_gated_on() {
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(false, false); // print_steps=false, print_details=false
        logging.log_message("silent"); // printDetails is false -> gate is false
        assert_eq!(out.text(), "");
        assert_eq!(
            logging.command_log(),
            "",
            "log_message must record nothing at all when gated off"
        );
    }

    #[test]
    fn log_and_print_still_records_to_buffer_when_console_is_suppressed() {
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(false, false); // printDetails=false suppresses console
        logging.log_and_print("recorded but silent");
        assert_eq!(out.text(), "", "console output should be suppressed");
        assert_eq!(
            logging.command_log(),
            "recorded but silent\n",
            "unlike log_message, log_and_print still records to the command buffer"
        );
    }

    #[test]
    fn log_message_prints_and_records_when_gated_on() {
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(false, true); // printDetails=true
        logging.log_message("hello");
        assert_eq!(out.text(), "hello\n");
        assert_eq!(logging.command_log(), "hello\n");
        assert_eq!(logging.detailed_log(), "hello\n");
    }

    #[test]
    fn log_message_with_explicit_print_false_records_nothing_even_if_print_details_true() {
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(false, true);
        logging.log_message_with(false, "explicitly suppressed");
        assert_eq!(out.text(), "");
        assert_eq!(logging.command_log(), "");
    }

    // ------------------------------------------------------------ log_evaluation_step

    #[test]
    fn log_evaluation_step_final_line_only_affects_the_buffers_trailing_newline() {
        // Mirrors EvalDef.compute's real usage: a loop of `final_line = false` step
        // lines, then one closing `final_line = true` summary line.
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(true, false);

        logging.log_evaluation_step("step 1", false);
        logging.log_evaluation_step("step 2", false);
        logging.log_evaluation_step("Total computation time: 5ms.", true);

        assert_eq!(
            logging.command_log(),
            "step 1\nstep 2\nTotal computation time: 5ms.",
            "final_line=false lines are newline-terminated; the one final_line=true \
             call leaves the buffer without a trailing newline"
        );
        assert_eq!(
            out.text(),
            "step 1\nstep 2\nTotal computation time: 5ms.\n",
            "the console write always ends its own line, regardless of final_line"
        );
    }

    #[test]
    fn log_evaluation_step_console_gate_is_steps_or_details() {
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(true, false); // print_steps=true, print_details=false
        logging.log_evaluation_step("visible", true);
        assert_eq!(out.text(), "visible\n");

        let (mut logging2, out2, _err2) = logging_with_capture();
        logging2.configure_for_command(false, false); // neither flag set
        logging2.log_evaluation_step("hidden", true);
        assert_eq!(out2.text(), "");
        assert_eq!(
            logging2.command_log(),
            "hidden", // final_line=true omits the trailing newline in the buffer
            "the buffer still records even when the console is suppressed"
        );
    }

    // --------------------------------------------------------------------- global log

    #[test]
    fn global_log_records_lines_from_every_logging_call_and_prefixes_commands() {
        let path = tmp_path("global.txt");
        let (mut logging, _out, _err) = logging_with_capture();
        logging.initialize_global_log(&path);

        logging.log_command(Some("eval foo \"?msd_2 x=x\":"));
        logging.configure_for_command(false, false); // console/buffers suppressed...
        logging.log_and_print("still reaches the global log");
        logging.log_command(None); // null -> ""

        let contents = read_file(&path);
        assert_eq!(
            contents, ">>> eval foo \"?msd_2 x=x\":\nstill reaches the global log\n\n>>> \n",
            "log_command separates repeated commands with a blank line; global log \
             capture is independent of the console/buffer gates"
        );
    }

    #[test]
    fn log_command_and_write_global_log_line_are_noops_without_an_open_global_log() {
        let (mut logging, _out, _err) = logging_with_capture();
        // No initialize_global_log call at all: global_log_writer is None.
        logging.log_command(Some("ignored"));
        assert!(
            !logging.global_log_has_content_for_test(),
            "log_command must not mark the (nonexistent) global log as having content"
        );

        logging.log_and_print("also fine, just not recorded to any file");
        assert!(
            !logging.global_log_has_content_for_test(),
            "write_global_log_line (via log_and_print) must not mark the (nonexistent) \
             global log as having content either"
        );

        // Now open a real global log and confirm it starts genuinely empty -- i.e.
        // the calls above really wrote nothing anywhere, they didn't just target a
        // path we forgot to check.
        let path = tmp_path("was_never_written.txt");
        logging.initialize_global_log(&path);
        assert_eq!(read_file(&path), "");
    }

    // ---------------------------------------------------------------- eval log files

    #[test]
    fn write_eval_logs_to_excludes_nested_log_and_print_from_the_command_file_only() {
        // Demonstrates asymmetry #2 precisely: a nested `log_and_print` call made
        // while eval log files are active reaches the *detailed* log file (gated
        // only by print_details) but NOT the *command* log file (gated by
        // `!eval_log_files_active`); `log_evaluation_step` reaches both,
        // unconditionally.
        let base = tmp_path("evalcase");
        let (mut logging, out, _err) = logging_with_capture();
        logging.configure_for_command(false, true); // print_details=true

        {
            let mut ctx = logging.write_eval_logs_to(&base);
            ctx.logging_mut().log_evaluation_step("step one", true);
            ctx.logging_mut()
                .log_and_print("nested detail, should reach detailed but not command");
        }

        assert_eq!(
            read_file(&format!("{base}_log.txt")),
            "step one\n",
            "the command-log file only ever receives log_evaluation_step's line"
        );
        assert_eq!(
            read_file(&format!("{base}_detailed_log.txt")),
            "step one\nnested detail, should reach detailed but not command\n",
            "the detailed-log file receives BOTH calls: print_details alone gates it, \
             with no eval_log_files_active guard"
        );
        assert_eq!(
            logging.command_log(),
            "step one", // log_evaluation_step("step one", true): final_line=true, no trailing \n
            "the in-memory command_log buffer only reflects log_evaluation_step's \
             append; log_and_print's append was suppressed while eval log files were active"
        );
        assert_eq!(
            out.text(),
            "step one\nnested detail, should reach detailed but not command\n",
            "console output is unaffected by eval-log-file gating either way"
        );
    }

    #[test]
    fn write_eval_logs_to_skips_detailed_file_when_print_details_is_false() {
        let base = tmp_path("nodetail");
        let (mut logging, _out, _err) = logging_with_capture();
        logging.configure_for_command(true, false); // print_details=false

        {
            let mut ctx = logging.write_eval_logs_to(&base);
            ctx.logging_mut()
                .log_evaluation_step("only command log", true);
        }

        assert_eq!(read_file(&format!("{base}_log.txt")), "only command log\n");
        assert!(!Path::new(&format!("{base}_detailed_log.txt")).exists());
    }

    #[test]
    fn closing_the_context_restores_prior_state_and_stops_further_file_writes() {
        let base = tmp_path("restore");
        let (mut logging, _out, _err) = logging_with_capture();
        logging.configure_for_command(false, false);
        assert!(!logging.eval_log_files_active_for_test());

        {
            let mut ctx = logging.write_eval_logs_to(&base);
            assert!(ctx.logging_mut().eval_log_files_active_for_test());
        } // dropped here -> close() runs

        assert!(!logging.eval_log_files_active_for_test());
        logging.log_evaluation_step("after close", true);
        // The file should contain only what was written before the context closed.
        assert_eq!(read_file(&format!("{base}_log.txt")), "");
        assert_eq!(logging.command_log(), "after close"); // final_line=true: no trailing \n
    }

    #[test]
    fn explicit_close_is_idempotent_and_only_restores_state_once() {
        // Java: `commandLogContext_secondClose_isNoOp`. Unlike a bare "does not
        // panic" check, this pins WHAT the first close() actually does (restores
        // `eval_log_files_active` and stops further writes to the eval-log file) and
        // that a second close() genuinely changes nothing further -- neither
        // re-flipping the flag nor reopening/rewriting the file behind the caller's
        // back.
        let base = tmp_path("idempotent");
        let (mut logging, _out, _err) = logging_with_capture();
        logging.configure_for_command(false, false);
        assert!(!logging.eval_log_files_active_for_test());

        {
            let mut ctx = logging.write_eval_logs_to(&base);
            assert!(ctx.logging_mut().eval_log_files_active_for_test());
            ctx.logging_mut().log_evaluation_step("first", true);

            ctx.close();
            assert!(
                !ctx.logging_mut().eval_log_files_active_for_test(),
                "the first close() must restore the pre-context eval_log_files_active state"
            );

            ctx.close(); // must not panic or double-restore incorrectly
            assert!(!ctx.logging_mut().eval_log_files_active_for_test());
        }

        assert_eq!(
            read_file(&format!("{base}_log.txt")),
            "first\n",
            "only the write made before close() should have reached the file"
        );
        logging.log_evaluation_step("after both closes", true);
        assert_eq!(
            read_file(&format!("{base}_log.txt")),
            "first\n",
            "neither close() call may leave the eval-log file open for further writes"
        );
    }

    // Small test-only accessors so the assertions above don't need public getters
    // that no real call site needs yet.
    impl Logging {
        fn eval_log_files_active_for_test(&self) -> bool {
            self.eval_log_files_active
        }
        fn global_log_has_content_for_test(&self) -> bool {
            self.global_log_has_content
        }
    }

    // ------------------------------------------------------ print_truncated_stack_trace

    struct FakeHandled(Option<&'static str>);
    impl LoggableError for FakeHandled {
        fn is_handled(&self) -> bool {
            true
        }
        fn message(&self) -> Option<String> {
            self.0.map(|s| s.to_string())
        }
        fn kind(&self) -> String {
            "should not be consulted".to_string()
        }
        fn stack_trace_lines(&self) -> Vec<String> {
            vec!["should not be consulted".to_string()]
        }
    }

    struct FakeUnhandled {
        kind: &'static str,
        message: Option<&'static str>,
        frames: Vec<&'static str>,
    }
    impl LoggableError for FakeUnhandled {
        fn is_handled(&self) -> bool {
            false
        }
        fn message(&self) -> Option<String> {
            self.message.map(|s| s.to_string())
        }
        fn kind(&self) -> String {
            self.kind.to_string()
        }
        fn stack_trace_lines(&self) -> Vec<String> {
            self.frames.iter().map(|s| s.to_string()).collect()
        }
    }

    #[test]
    fn handled_error_prints_message_only_to_stdout_and_global_log() {
        let path = tmp_path("handled.txt");
        let (mut logging, out, err) = logging_with_capture();
        logging.initialize_global_log(&path);

        logging.print_truncated_stack_trace(&FakeHandled(Some("bad query: unknown variable x")));

        assert_eq!(out.text(), "bad query: unknown variable x\n");
        assert_eq!(err.text(), "", "handled errors must not touch stderr");
        assert_eq!(read_file(&path), "bad query: unknown variable x\n");
    }

    /// Java: `printTruncatedStackTrace_walnutExceptionWithNullMessage_writesEmptyLine`
    /// (`LoggingTest.java:154`). Pins the real asymmetry between the two sinks a
    /// null/`None` message hits: `System.out.println(null)` prints the literal
    /// string `"null"`, but `writeGlobalLogLine`'s own null guard normalizes it to an
    /// empty line. A `LoggableError::message` returning `None` must reproduce both
    /// halves exactly, not collapse them to the same text.
    #[test]
    fn handled_error_with_null_message_prints_the_literal_null_but_logs_an_empty_line() {
        let path = tmp_path("handled_null.txt");
        let (mut logging, out, err) = logging_with_capture();
        logging.initialize_global_log(&path);

        logging.print_truncated_stack_trace(&FakeHandled(None));

        assert_eq!(
            out.text(),
            "null\n",
            "System.out.println(e.getMessage()) on a null message prints the literal \"null\""
        );
        assert_eq!(err.text(), "");
        assert_eq!(
            read_file(&path),
            "\n",
            "writeGlobalLogLine normalizes a null message to an empty logged line, not \"null\""
        );
    }

    #[test]
    fn unhandled_error_truncates_frames_and_writes_to_stderr_not_stdout() {
        let (mut logging, out, err) = logging_with_capture();
        let e = FakeUnhandled {
            kind: "Main.WalnutRuntimeException",
            message: Some("unexpected null"),
            frames: vec!["Frame::one", "Frame::two", "Frame::three"],
        };

        logging.print_truncated_stack_trace(&e); // default length = 1

        assert_eq!(out.text(), "", "unhandled errors must not touch stdout");
        assert_eq!(
            err.text(),
            "Main.WalnutRuntimeException: unexpected null\n\tat Frame::one\n",
            "the rendered text must start with the `kind: message` header line \
             (Throwable.toString()), not the bare message"
        );
    }

    /// Pins finding #3: the header line has no meaningful Rust analogue for the
    /// *frame text*, but the `kind: message` header itself does, and dropping it
    /// silently loses real information a Walnut user's stack dump would show.
    #[test]
    fn unhandled_error_header_line_is_kind_colon_message() {
        let (mut logging, _out, err) = logging_with_capture();
        let e = FakeUnhandled {
            kind: "java.lang.NullPointerException",
            message: Some("Cannot invoke \"Object.toString()\" because \"x\" is null"),
            frames: vec!["Frame::only"],
        };
        logging.print_truncated_stack_trace(&e);
        assert_eq!(
            err.text(),
            "java.lang.NullPointerException: Cannot invoke \"Object.toString()\" because \"x\" is null\n\tat Frame::only\n"
        );
    }

    /// Java: `Throwable.toString()` special-cases a null message to just the class
    /// name (no trailing `": null"`) -- unlike the handled-branch println-on-null
    /// case above, which really does print the literal text "null". Both are real,
    /// distinct behaviors from the same nullable `getMessage()`; this pins the
    /// unhandled-branch one specifically.
    #[test]
    fn unhandled_error_with_null_message_header_is_just_the_kind_no_null_suffix() {
        let (mut logging, _out, err) = logging_with_capture();
        let e = FakeUnhandled {
            kind: "java.lang.NullPointerException",
            message: None,
            frames: vec!["Frame::only"],
        };
        logging.print_truncated_stack_trace(&e);
        assert_eq!(
            err.text(),
            "java.lang.NullPointerException\n\tat Frame::only\n"
        );
    }

    #[test]
    fn unhandled_error_with_explicit_length_keeps_that_many_frames() {
        let (mut logging, _out, err) = logging_with_capture();
        let e = FakeUnhandled {
            kind: "Kind",
            message: Some("boom"),
            frames: vec!["a", "b", "c"],
        };
        logging.print_truncated_stack_trace_with_length(&e, 2);
        assert_eq!(err.text(), "Kind: boom\n\tat a\n\tat b\n");
    }

    #[test]
    fn unhandled_error_length_longer_than_available_frames_keeps_them_all() {
        let (mut logging, _out, err) = logging_with_capture();
        let e = FakeUnhandled {
            kind: "Kind",
            message: Some("boom"),
            frames: vec!["only"],
        };
        logging.print_truncated_stack_trace_with_length(&e, 5);
        assert_eq!(err.text(), "Kind: boom\n\tat only\n");
    }

    // ---------------------------------------------------- initialize_global_log edge cases

    /// Java: `initializeGlobalLog_pathIsADirectory_ioExceptionIsCaughtAndPrinted`
    /// (`LoggingTest.java:87`). `File::create` on a path that is itself an existing
    /// directory fails; the failure must be caught and reported to the console with
    /// the exact Java message text, leaving no writer open.
    #[test]
    fn initialize_global_log_path_is_a_directory_prints_exact_java_message_and_opens_no_writer() {
        let dir = tmp_existing_dir("as_a_directory");
        let (mut logging, out, _err) = logging_with_capture();

        logging.initialize_global_log(&dir);

        assert_eq!(
            out.text(),
            format!("Could not create global log file {dir}\n")
        );

        // globalLogWriter is still null/None: log_command must no-op, not panic.
        logging.log_command(Some("should be dropped"));
        assert!(!logging.global_log_has_content_for_test());
    }

    /// Java: `initializeGlobalLog_bareFilenameWithNoParent_stillOpensWriter`
    /// (`LoggingTest.java:70`). A bare filename with no directory component makes
    /// `Path::parent()` return `Some("")` in Rust (Java's `Path.getParent()` returns
    /// `null` for the same input) -- this exercises the
    /// `!parent.as_os_str().is_empty()` guard that exists specifically to treat that
    /// `Some("")` the same as Java's null.
    #[test]
    fn initialize_global_log_bare_filename_with_no_parent_still_opens_writer() {
        let filename = format!("wr_core_logging_bare_{}.txt", unique_suffix());
        let (mut logging, _out, _err) = logging_with_capture();

        logging.initialize_global_log(&filename);
        logging.log_command(Some("marker"));

        let contents = read_file(&filename);
        fs::remove_file(&filename).unwrap();
        assert_eq!(contents, ">>> marker\n");
    }

    // ------------------------------------------------- global-log writer I/O failures

    /// A `Write` whose `write`/`flush` always fail, to exercise the "ignored" I/O
    /// failure paths -- Java's `ThrowingWriter` + `ioExceptionsFromTheWriter_areSwallowedEverywhere`.
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("simulated write failure"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("simulated flush failure"))
        }
    }

    /// Java: `ioExceptionsFromTheWriter_areSwallowedEverywhere` (`LoggingTest.java:129`).
    #[test]
    fn io_failures_from_the_global_log_writer_are_swallowed_everywhere() {
        let (mut logging, _out, err) = logging_with_capture();
        logging.set_global_log_writer_for_test(Box::new(FailingWriter));

        // log_command()'s write fails; caught and ignored -- no panic, and the
        // failed write must not be recorded as having succeeded.
        logging.log_command(Some("anything"));
        assert!(!logging.global_log_has_content_for_test());

        // write_global_log_line() (via print_truncated_stack_trace) also fails;
        // likewise swallowed.
        logging.print_truncated_stack_trace(&FakeHandled(Some("boom")));
        assert_eq!(
            err.text(),
            "",
            "handled errors never touch stderr, failing or not"
        );
        assert!(!logging.global_log_has_content_for_test());

        // initialize_global_log() closes the (still-failing) previous writer first
        // -- dropping it swallows any close-time failure the same way Java's ignored
        // catch does -- and leaves a genuinely working writer behind afterward.
        let path = tmp_path("after_failure.txt");
        logging.initialize_global_log(&path);
        logging.log_command(Some("now it works"));
        assert!(read_file(&path).contains("now it works"));
    }
}
