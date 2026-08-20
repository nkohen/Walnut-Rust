// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/EvalDef.java` (185 LOC) — the real `eval`/`def` command implementation,
//! Phase 3a's U15: the final integration of U11's `wr_logic::eval::evaluate_with_logging`,
//! U14's real `Session`, and U11b's `TestCase`.
//!
//! # Scope: `evalDefCommand`/`computeHeadless` only
//!
//! `EvalDef` has four public entry points (see `wr_logic::eval`'s own module docs, which
//! already map all four). Two are `wr-logic`-scope and already shipped (U11): `compute`
//! (`:105-158`, the postfix-token executor) and the "lex → compute → unwrap" shape shared
//! by `computeHeadless`/`getImageEval`, generalized there as
//! [`wr_logic::eval::evaluate_with_logging`]/[`wr_logic::eval::evaluate`]. This module
//! ports the two that are genuinely `wr-cli` scope:
//!
//! * [`eval_def_command`] — `EvalDef.evalDefCommand` (`:50-77`), the real `eval`/`def`
//!   command Java's `Prover` dispatches to.
//! * `compute_headless` (private below) — `EvalDef.computeHeadless` (`:79-91`), the
//!   no-result-name branch `eval_def_command` itself calls.
//!
//! `getImageEval` (`:93-99`, `image`'s call site) is out of scope here too — it is
//! `Main.Commands.Image`'s own territory, not yet ported, and per `wr_logic::eval`'s docs
//! needs nothing beyond what that module already provides.
//!
//! # The headless/non-headless split (`evalName == null || evalName.isBlank()`)
//!
//! `headless` means "no result name was given" — `eval query.txt` without a trailing
//! `evalName`, vs. `eval myresult "query.txt"`. It changes THREE things, all preserved
//! here:
//!
//! 1. **Whether `Session`/file I/O is touched at all for the RESULT.** Headless calls
//!    [`wr_logic::eval::evaluate_with_logging`] directly, with no `Session`-derived result
//!    name and no [`crate::automaton_output::write_automata`] call — nothing is written to
//!    `Result/` or a library directory, and [`TestCase::gv_address`]'s caller-visible
//!    `graph_viz()` reads back as `""` (see [`crate::test_case::TestCase::graph_viz`]).
//!    Non-headless wraps the same evaluation in [`Logging::with_eval_logs_to`] (Java's
//!    `try (var ignored = Logging.writeEvalLogsTo(resultName))`), then always calls
//!    `write_automata`.
//! 2. **What `TestCase`'s `matrix_addresses`/`gv_address` fields hold.** Headless: `vec![]`
//!    / `""` (`List.of()` / `""` in Java). Non-headless: (see the CAS section below) /
//!    `{result_name}.gv`.
//! 3. **Nothing else.** The `TRUE`/`FALSE` print (below) and the postfix-evaluation logic
//!    itself are identical in both branches — a common, easy-to-assume-wrong shape this
//!    module's tests pin explicitly.
//!
//! `evalName.isBlank()` (Java's `String.isBlank()`, Unicode-`Character.isWhitespace`-based:
//! empty or all-whitespace) is approximated here as `s.trim().is_empty()` (Rust's
//! `char::is_whitespace`, Unicode `White_Space`-property-based) — the two whitespace
//! classes disagree on a handful of rare code points (e.g. U+180E MONGOLIAN VOWEL
//! SEPARATOR), not a Walnut behavioral bug, just a std-library-semantics gap. It has no
//! observable effect through the real grammar: `Main/Prover.java`'s `eval`/`def` regex
//! (`RE_START + "(eval|def)(?:\\s+(" + RE_IDENTIFIER + ")...` — not yet ported, Phase 3b's
//! U21) only ever captures `evalName` as `RE_IDENTIFIER = [a-zA-Z]\w*`, which by
//! construction is never blank-but-non-empty; a real dispatch call therefore only ever
//! reaches this function with `eval_name` either wholly absent or a genuine non-blank
//! identifier.
//!
//! # The `TRUE`/`FALSE` print branch (now real via U0's `TRUE_FALSE_AUTOMATON` retrofit)
//!
//! Both branches, after computing the result automaton, do:
//!
//! ```java
//! if (M.fa.isTRUE_FALSE_AUTOMATON()) {
//!   System.out.println("____\n" + M.fa.trueFalseString().toUpperCase());
//! }
//! ```
//!
//! — a **raw `System.out.println`**, not routed through `Logging` (`Logging`'s own
//! `console` field, per `wr_core::logging`'s module docs, models a *specific* slf4j
//! `"console"` logger `Logging`'s own methods write to, not a blanket stdout redirect; this
//! print is a separate, unrelated call site in Java, so this port keeps it separate too
//! rather than folding it into `Logging`). [`eval_def_command`]/`compute_headless` take an
//! explicit `stdout: &mut dyn Write` for exactly this line, mirroring the injectable-writer
//! seam `wr_core::logging::Logging::with_writers`/`crate::session::SessionPaths::with_console`
//! already establish for the same reason (a real, user-visible `println!` that tests must
//! be able to assert on without capturing the process's actual stdout).
//!
//! **Important, easy-to-get-backwards nuance verified against the real Java source (not
//! just this unit's own task description): a `TRUE`/`FALSE` result does NOT mean "no
//! automaton file is written" in the non-headless branch.** `M.writeAutomata(...)`
//! (`:67`) runs unconditionally, BEFORE the `isTRUE_FALSE_AUTOMATON()` check — and
//! `Automaton.writeAutomata`/`AutomatonWriter.writeToGV`/`writeToTxtFormat` themselves
//! already special-case `TRUE_FALSE_AUTOMATON` (writing the literal string `"true"`/
//! `"false"` as the `.txt` body, and a degenerate one-node `.gv`), already ported and
//! reviewed at U12/U16 (`wr_io::writer`, `crate::automaton_output`). So in non-headless
//! mode, a TRUE/FALSE result gets BOTH: a (degenerate) automaton file AND the console
//! print. It is HEADLESS mode — regardless of TRUE/FALSE — that never writes any file at
//! all, simply because there is no result name to write one under. Pinned by
//! [`tests::non_headless_true_result_writes_a_degenerate_automaton_file_and_prints`] below.
//!
//! # CAS matrix-export handling — now real (`.claude/plans/amber-transcribing-ledger.md`)
//!
//! `EvalDef.evalDefCommand`'s non-headless branch unconditionally calls
//! `writeMatrices(M, freeVarStr, resultName)` (`:72`), which (`:160-172`) calls
//! `AutomatonMatrixWriter.writeAll` whenever `determineFreeVariables(freeVarStr)`
//! (`:174-184`, scanning `freeVarStr` for `RE_IDENTIFIER` matches) finds at least one free
//! variable — printing `"Matrix files:"` plus one line per CAS emitter, and returning the
//! resulting file addresses for `TestCase`'s `matrix_addresses` field.
//!
//! **`free_var_str` is a separate, EXPLICIT command-line argument, not auto-detected from
//! the predicate.** Verified against the real `walnut-java` CLI (not just this unit's own
//! task description): `Main/Prover.java`'s `eval`/`def` regex captures it as `evalName`'s
//! trailing `(?:\s+RE_IDENTIFIER)*` group — i.e. `eval r x y "x < 3 & y = x";` passes
//! `freeVarStr = "x y"` and DOES trigger real Walnut's CAS export (confirmed live:
//! produces `r.m`/`r.mpl`/`r.wl`/`r.sage` plus the `"Matrix files:"` print), while `eval r
//! "x < 3 & y = x";` — same predicate, same genuinely-free `x`/`y`, but no extra
//! identifiers on the command line — passes `freeVarStr = ""` and triggers nothing,
//! confirmed live too. So "does the predicate have free variables" and "does this call
//! attempt CAS export" are Java's two independently-controllable inputs, not the same
//! question.
//!
//! `AutomatonMatrixWriter` (the CAS/Maple/MATLAB/Mathematica/Sage incidence-matrix
//! exporter) was previously confirmed DROP scope, per a since-superseded Phase 3
//! sign-off ("keep writing the automaton output normally, but never produce the matrix
//! side-files"). That sign-off has been revisited and reversed: the actual
//! CAS-file-writing machinery is now ported, in `wr_io::matrix_writer` (see that
//! module's own docs for the full mechanical-port account, including WB-042 and the two
//! confirmed-unreachable-but-ported validation checks). This function now calls
//! [`wr_io::matrix_writer::write_all`] for real whenever `free_var_str` names at least
//! one identifier, and prints `"Matrix files:"` plus one line per emitter
//! (`EvalDef.java:165-169`, raw `System.out.println` — routed through this function's
//! own `stdout` seam, same as the `TRUE`/`FALSE` print above) exactly as Java does.
//! `matrix_addresses` is now the real `.mpl`/`.m`/`.wl`/`.sage` paths on the success
//! path, matching Java's own value — there is no longer a divergence here.
//!
//! `AutomatonMatrixWriter.writeMatrix`'s five real `WalnutException`s (see
//! `wr_io::matrix_writer::MatrixWriteError`'s docs for the exact line numbers and which
//! two are confirmed unreachable but ported anyway) are NOT caught by
//! `AutomatonMatrixWriter.writeAll`'s `catch (IOException e)` (`:180`), so they
//! propagate UNCAUGHT out of `evalDefCommand` entirely — the whole `eval`/`def` command
//! fails, even though `M.writeAutomata(...)` (`:67`) and the TRUE/FALSE console print
//! (`:68-70`) have ALREADY run as side effects by that point (`writeMatrices` is called
//! at `:72`, after both). [`wr_io::matrix_writer::write_all`]'s `Result` is converted to
//! an [`EvalDefError`] at the exact same point in the control flow Java has the
//! (uncaught) exception — after `write_automata`/the print, matching Java's real
//! statement order. A call whose free-variable list is empty (the common case) never
//! reaches `write_all` at all, matching `writeMatrices`'s own
//! `!freeVariables.isEmpty()` guard (`:163`).
//!
//! [`TestCase::gv_address`]: crate::test_case::TestCase
use std::io::Write;

use wr_core::determinize::DeterminizeContext;
use wr_core::logging::Logging;
use wr_logic::eval::{evaluate_with_logging_and_ctx, EvalError};
use wr_logic::predicate_env::{FreshIdentifiers, PredicateEnv};

use crate::automaton_output::{write_automata, GV_EXTENSION};
use crate::session::Session;
use crate::test_case::{AutomatonFilenamePair, TestCase, DEFAULT_TESTFILE};

/// Every failure [`eval_def_command`] can report: propagated from either
/// [`wr_logic::eval::evaluate_with_logging`] (parse/postfix-evaluation failures — Java's
/// uncaught `WalnutException`s and the two documented uncaught-`RuntimeException` cases,
/// see that module's `LoggableError for ActError` docs), from
/// [`crate::automaton_output::write_automata`] (a real I/O failure — see that function's
/// own docs on why this propagates rather than being swallowed-and-logged the way Java's
/// `Automaton.writeAutomata` is), or from [`wr_io::matrix_writer::write_all`]'s own port
/// of `AutomatonMatrixWriter.writeMatrix`'s validation `WalnutException`s (see this
/// module's docs, "CAS matrix-export handling", and `wr_io::matrix_writer`'s own docs for
/// the full validation order).
#[derive(Debug)]
pub enum EvalDefError {
    Eval(EvalError),
    Io(std::io::Error),
    /// `AutomatonMatrixWriter.writeMatrix` (`:32-34`): a non-empty free-variable list
    /// against a trivial TRUE/FALSE result automaton.
    NoFreeVariableOnTrivialAutomaton,
    /// `AutomatonMatrixWriter.writeMatrix` (`:44-45`): two entries of the result
    /// automaton's own label are identical. Confirmed unreachable in practice (this
    /// crate's own invariants keep track labels unique) — ported anyway, see
    /// `wr_io::matrix_writer`'s module docs.
    DuplicateLabelVariable {
        name: String,
    },
    /// `AutomatonMatrixWriter.writeMatrix` (`:52-55`): `name` is not one of the result
    /// automaton's track labels (`Automaton.getLabel()`).
    NotAFreeVariable {
        name: String,
    },
    /// `AutomatonMatrixWriter.writeMatrix` (`:56-58`): `name` appears more than once in
    /// the caller's free-variable list.
    DuplicateFreeVariable {
        name: String,
    },
    /// `AutomatonMatrixWriter.writeMatrix` (`:68-69`): a free variable's track has an
    /// empty domain. Confirmed unreachable in practice — ported anyway, see
    /// `wr_io::matrix_writer`'s module docs.
    EmptyValueDomain {
        name: String,
    },
}

impl std::fmt::Display for EvalDefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalDefError::Eval(e) => write!(f, "{e}"),
            EvalDefError::Io(e) => write!(f, "{e}"),
            // Verbatim `AutomatonMatrixWriter.java:33`.
            EvalDefError::NoFreeVariableOnTrivialAutomaton => write!(
                f,
                "incidence matrices cannot be calculated, because the automaton does not \
                 have a free variable."
            ),
            // Verbatim `AutomatonMatrixWriter.java:45`.
            EvalDefError::DuplicateLabelVariable { name } => {
                write!(f, "Duplicate variable in automaton label: {name}")
            }
            // Verbatim `AutomatonMatrixWriter.java:53-54`.
            EvalDefError::NotAFreeVariable { name } => write!(
                f,
                "incidence matrices for the variable {name} cannot be calculated, because \
                 {name} is not a free variable."
            ),
            // Verbatim `AutomatonMatrixWriter.java:57`.
            EvalDefError::DuplicateFreeVariable { name } => {
                write!(f, "Duplicate free variable: {name}")
            }
            // Verbatim `AutomatonMatrixWriter.java:69`.
            EvalDefError::EmptyValueDomain { name } => {
                write!(f, "Empty value domain for free variable: {name}")
            }
        }
    }
}

impl std::error::Error for EvalDefError {}

impl From<EvalError> for EvalDefError {
    fn from(e: EvalError) -> Self {
        EvalDefError::Eval(e)
    }
}

impl From<std::io::Error> for EvalDefError {
    fn from(e: std::io::Error) -> Self {
        EvalDefError::Io(e)
    }
}

impl From<wr_io::matrix_writer::MatrixWriteError> for EvalDefError {
    fn from(e: wr_io::matrix_writer::MatrixWriteError) -> Self {
        use wr_io::matrix_writer::MatrixWriteError as M;
        match e {
            M::NoFreeVariableOnTrivialAutomaton => EvalDefError::NoFreeVariableOnTrivialAutomaton,
            M::DuplicateLabelVariable { name } => EvalDefError::DuplicateLabelVariable { name },
            M::NotAFreeVariable { name } => EvalDefError::NotAFreeVariable { name },
            M::DuplicateFreeVariable { name } => EvalDefError::DuplicateFreeVariable { name },
            M::EmptyValueDomain { name } => EvalDefError::EmptyValueDomain { name },
        }
    }
}

/// `EvalDef.evalDefCommand(boolean printFlag, boolean printDetails, String predicateStr,
/// String evalName, String freeVarStr)` (`EvalDef.java:50-77`), writing the `TRUE`/`FALSE`
/// console line (see this module's docs) to the real process stdout.
///
/// `eval_name`/`free_var_str` are `Option<&str>` for Java's nullable `String` parameters.
/// See this module's docs for the headless/non-headless split, the `TRUE`/`FALSE` print,
/// and why `free_var_str` drives only the two `AutomatonMatrixWriter` validation checks
/// (not the matrix-writing itself, which stays out of scope).
///
/// `clippy::too_many_arguments`: Java's own `evalDefCommand` takes 5 parameters and reads
/// `Session`/`Logging`/the fresh-identifier counter off `static` state for free; this port's
/// "explicit context instead of statics" is `CLAUDE.md`'s one sanctioned mechanical-fidelity
/// deviation (see `wr_core::logging`'s and `crate::session`'s own module docs for the same
/// point), so the 3 extra parameters here are that deviation's direct, expected cost.
#[allow(clippy::too_many_arguments)]
pub fn eval_def_command(
    session: &Session,
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    print_flag: bool,
    print_details: bool,
    predicate_str: &str,
    eval_name: Option<&str>,
    free_var_str: Option<&str>,
) -> Result<TestCase, EvalDefError> {
    eval_def_command_with_stdout(
        session,
        logging,
        fresh,
        print_flag,
        print_details,
        predicate_str,
        eval_name,
        free_var_str,
        &mut std::io::stdout(),
    )
}

/// As [`eval_def_command`], but with an injectable stdout sink — the seam this module's
/// own tests use to assert on the `TRUE`/`FALSE` console line without capturing the
/// process's real stdout. See this module's docs on why this print is a separate,
/// `Logging`-independent call site in Java.
///
/// `clippy::too_many_arguments`: see [`eval_def_command`]'s docs — this variant adds
/// exactly one more (`stdout`) on top of that already-justified count.
#[allow(clippy::too_many_arguments)]
pub fn eval_def_command_with_stdout(
    session: &Session,
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    print_flag: bool,
    print_details: bool,
    predicate_str: &str,
    eval_name: Option<&str>,
    free_var_str: Option<&str>,
    stdout: &mut dyn Write,
) -> Result<TestCase, EvalDefError> {
    eval_def_command_with_stdout_and_ctx(
        session,
        logging,
        fresh,
        print_flag,
        print_details,
        predicate_str,
        eval_name,
        free_var_str,
        stdout,
        None,
    )
}

/// As [`eval_def_command_with_stdout`], plus the `[strategy …]`/`[export …]` metacommand
/// context (`crate::meta_commands::MetaCommands` implements
/// [`DeterminizeContext`]) — the single point where Walnut's parsed metacommands actually
/// reach the determinization dispatcher.
///
/// # The `shouldPrintDetails()` gate lives at this call's CALLER
///
/// Java wraps its whole metacommand block in `if (Logging.shouldPrintDetails())`
/// (`DeterminizationStrategies.java:95`), so the automata counter must not move for a
/// command that is not printing details. `MetaCommands.parseMetaCommands` already refuses
/// any metacommand on a command that does not end in `::`
/// (`MetaCommands.java:91-93`), which makes `print_details` the exact Rust-side spelling
/// of that gate — [`crate::prover::Prover::eval_def_commands`] passes `Some(ctx)` only
/// when `print_details` holds. The `printEnabled` half of Java's flag (flipped off by
/// `Logging.disablePrint()` around every automaton `NumberSystem` builds for itself) is
/// modelled structurally instead: `wr_core::numsys` never receives a context at all.
///
/// `clippy::too_many_arguments`: see [`eval_def_command`]'s docs.
#[allow(clippy::too_many_arguments)]
pub fn eval_def_command_with_stdout_and_ctx(
    session: &Session,
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    print_flag: bool,
    print_details: bool,
    predicate_str: &str,
    eval_name: Option<&str>,
    free_var_str: Option<&str>,
    stdout: &mut dyn Write,
    ctx: Option<&mut (dyn DeterminizeContext + '_)>,
) -> Result<TestCase, EvalDefError> {
    // `boolean headless = evalName == null || evalName.isBlank();` (`:52`) -- see this
    // module's docs on the `isBlank`/`trim().is_empty()` approximation.
    let headless = eval_name.map(|s| s.trim().is_empty()).unwrap_or(true);

    // `EvalDef c = new EvalDef(printFlag, printDetails);` (`:56`) -- the constructor's
    // entire body is `Logging.configureForCommand(printSteps, printDetails)` (`:46-48`).
    // Runs unconditionally before the headless/non-headless branch, matching Java's own
    // statement order exactly (though the branch's own purity means the order has no
    // observable effect either way).
    logging.configure_for_command(print_flag, print_details);

    let env = session.predicate_env();

    if headless {
        return compute_headless(env, logging, fresh, predicate_str, stdout, ctx);
    }
    // `evalName == null || evalName.isBlank()` was false, so a real, non-blank name exists.
    let eval_name =
        eval_name.expect("non-headless implies eval_name is Some (see `headless` above)");

    // `String resultName = Session.getAddressForResult() + evalName;` (`:61`).
    let result_name = format!("{}{eval_name}", session.paths().address_for_result());

    // `try (Logging.CommandLogContext ignored = Logging.writeEvalLogsTo(resultName)) { ... }`
    // (`:62-76`).
    logging.with_eval_logs_to(&result_name, |logging| {
        // `Predicate predicate = new Predicate(predicateStr); c.compute(predicate);
        //  Automaton M = c.result.M;` (`:63-65`).
        let mut automaton = evaluate_with_logging_and_ctx(env, logging, fresh, predicate_str, ctx)?;

        // `M.writeAutomata(predicateStr, Session.getWriteAddressForAutomataLibrary(),
        //  evalName, false);` (`:67`) -- UNCONDITIONAL, before the TRUE/FALSE check below.
        // See this module's docs: this still writes a (degenerate) file for a TRUE/FALSE
        // result, because `writeAutomata`'s own writers special-case that case internally.
        write_automata(
            session,
            &mut automaton,
            predicate_str,
            &session.paths().write_address_for_automata_library(),
            eval_name,
            false,
        )?;

        // `if (M.fa.isTRUE_FALSE_AUTOMATON()) { System.out.println("____\n" +
        //  M.fa.trueFalseString().toUpperCase()); }` (`:68-70`).
        if automaton.fa.is_true_false_automaton() {
            let _ = writeln!(
                stdout,
                "____\n{}",
                automaton.fa.true_false_string().to_uppercase()
            );
        }

        // `List<String> matrixAddresses = writeMatrices(M, freeVarStr, resultName);`
        // (`:72`), inlining `EvalDef.writeMatrices` (`:160-172`) itself: a no-op
        // (`vec![]`, matching `writeMatrices`'s own `!freeVariables.isEmpty()` guard,
        // `:163`) when the caller named no free variables; otherwise a real call into
        // [`wr_io::matrix_writer::write_all`] (see this module's docs, "CAS
        // matrix-export handling") followed by Java's own `"Matrix files:"` print
        // (`:165-169`, raw `System.out.println`, routed through `stdout` like the
        // `TRUE`/`FALSE` print above).
        let free_variables = determine_free_variables(free_var_str);
        let matrix_addresses: Vec<String> = if free_variables.is_empty() {
            Vec::new()
        } else {
            wr_io::matrix_writer::write_all(&mut automaton, &result_name, &free_variables)
                .map_err(EvalDefError::from)?;
            let _ = writeln!(stdout, "Matrix files:");
            wr_io::matrix_writer::EMITTERS
                .iter()
                .map(|spec| {
                    let path = format!("{result_name}{}", spec.extension);
                    let _ = writeln!(stdout, "  {}: {path}", spec.intro);
                    path
                })
                .collect()
        };

        // `return new TestCase("", matrixAddresses, resultName + Prover.GV_EXTENSION,
        //  Logging.getDetailedLog(), List.of(new TestCase.AutomatonFilenamePair(M,
        //  DEFAULT_TESTFILE)));` (`:73-75`).
        Ok(TestCase::new(
            "",
            matrix_addresses,
            format!("{result_name}{GV_EXTENSION}"),
            logging.detailed_log(),
            vec![AutomatonFilenamePair::new(automaton, DEFAULT_TESTFILE)],
        ))
    })
}

/// `EvalDef.computeHeadless(EvalDef c, String predicateStr)` (`EvalDef.java:79-91`). Not
/// `pub`: Java's is `private static`, reached only through [`eval_def_command`]'s headless
/// branch (mirrors that call shape exactly, so no external caller needs this directly).
///
/// Unlike Java's `computeHeadless(EvalDef c, ...)` (which reads `c`'s already-configured
/// `Logging` state implicitly via the static class), this takes `env`/`logging`/`fresh`
/// explicitly -- `eval_def_command` above has already run
/// `Logging.configureForCommand`'s equivalent before calling this, matching Java's own
/// statement order.
fn compute_headless(
    env: &dyn PredicateEnv,
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    predicate_str: &str,
    stdout: &mut dyn Write,
    ctx: Option<&mut (dyn DeterminizeContext + '_)>,
) -> Result<TestCase, EvalDefError> {
    // `Predicate predicate = new Predicate(predicateStr); c.compute(predicate);
    //  Automaton M = c.result.M;` (`:80-82`).
    let automaton = evaluate_with_logging_and_ctx(env, logging, fresh, predicate_str, ctx)?;

    // `if (M.fa.isTRUE_FALSE_AUTOMATON()) { System.out.println("____\n" +
    //  M.fa.trueFalseString().toUpperCase()); }` (`:84-86`) -- identical to the
    // non-headless branch's print (see this module's docs point 3).
    if automaton.fa.is_true_false_automaton() {
        let _ = writeln!(
            stdout,
            "____\n{}",
            automaton.fa.true_false_string().to_uppercase()
        );
    }

    // `return new TestCase("", List.of(), "", Logging.getDetailedLog(),
    //  List.of(new TestCase.AutomatonFilenamePair(M, DEFAULT_TESTFILE)));` (`:88-90`) --
    // no Session, no `writeAutomata` call: headless mode never writes a result file.
    Ok(TestCase::new(
        "",
        Vec::new(),
        "",
        logging.detailed_log(),
        vec![AutomatonFilenamePair::new(automaton, DEFAULT_TESTFILE)],
    ))
}

/// `EvalDef.determineFreeVariables(String freeVariablesStr)` (`EvalDef.java:174-184`):
/// scans `free_var_str` for every maximal match of `RE_IDENTIFIER = [a-zA-Z]\w*`
/// (`PAT_FOR_A_FREE_VARIABLE_IN_eval_def_CMDS`, Java's default non-`UNICODE_CHARACTER_CLASS`
/// `\w` = `[a-zA-Z_0-9]`), left to right, exactly matching `Matcher.find()`'s
/// findall-style scanning rather than a naive `split_whitespace` (a caller-supplied
/// `free_var_str` need not be pre-tokenized on spaces the way the real grammar's
/// `(?:\s+RE_IDENTIFIER)*` capture happens to produce it -- see this module's docs on
/// `free_var_str` being a library entry point, not just the grammar's output). A manual
/// scan rather than a `regex`/`regex-automata` match: `wr-cli` has no regex dependency
/// today and this ASCII-only, two-character-class pattern needs none.
fn determine_free_variables(free_var_str: Option<&str>) -> Vec<String> {
    let Some(s) = free_var_str else {
        return Vec::new();
    };
    let chars: Vec<char> = s.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            result.push(chars[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-eval-def-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
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
        // `SessionPaths` does not append a trailing slash itself (Java's
        // `Prover.parseArgs` does that); supply one here, matching `reg.rs`'s/
        // `automaton_output.rs`'s own test convention.
        let dir_str = format!("{}/", dir.to_str().unwrap());
        let session = Session::new(Some(&dir_str), Some(&dir_str), false);
        (session, dir)
    }

    fn stdout_text(buf: &[u8]) -> String {
        String::from_utf8(buf.to_vec()).unwrap()
    }

    // ------------------------------------------------------------------------
    // Headless mode: no Session I/O, TestCase carries the automaton directly.
    // ------------------------------------------------------------------------

    #[test]
    fn headless_mode_writes_no_files_and_returns_a_populated_test_case() {
        let (session, dir) = temp_session("headless-basic");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 x < 5 & x >= 2",
            None,
            None,
            &mut stdout,
        )
        .unwrap();

        assert_eq!(
            tc.graph_viz().unwrap(),
            "",
            "headless mode writes no .gv file"
        );
        assert_eq!(
            tc.matrix_output().unwrap(),
            vec![
                "".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string()
            ],
            "empty matrix_addresses short-circuits to the four-empty-string placeholder"
        );
        assert_eq!(tc.automaton_pairs().len(), 1);
        assert_eq!(tc.automaton_pairs()[0].filename(), DEFAULT_TESTFILE);
        assert!(tc.automaton_pairs()[0].automaton().is_some());

        // No result file exists anywhere -- headless mode never calls `write_automata`.
        assert!(
            fs::read_dir(dir.join("Result")).unwrap().next().is_none(),
            "headless mode must not write anything under Result/"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// Blank-but-`Some` `eval_name` is still headless, matching Java's
    /// `evalName == null || evalName.isBlank()` -- exercised even though (per this
    /// module's docs) the real grammar never produces this shape, since this function is
    /// a library entry point future callers besides `Prover`'s regex dispatch could use.
    #[test]
    fn blank_eval_name_is_still_headless() {
        let (session, dir) = temp_session("headless-blank-name");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 x < 5",
            Some("   "),
            None,
            &mut stdout,
        )
        .unwrap();

        assert_eq!(tc.graph_viz().unwrap(), "");
        assert!(
            fs::read_dir(dir.join("Result")).unwrap().next().is_none(),
            "a blank (whitespace-only) eval_name must also take the headless path"
        );

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------------
    // Non-headless mode: Session-backed file writes.
    // ------------------------------------------------------------------------

    #[test]
    fn non_headless_mode_writes_result_and_library_files() {
        let (session, dir) = temp_session("non-headless-basic");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "x < 3 & y = x",
            Some("myresult"),
            None,
            &mut stdout,
        )
        .unwrap();

        assert!(dir.join("Result").join("myresult.gv").is_file());
        assert!(dir.join("Result").join("myresult.txt").is_file());
        assert!(dir.join("Automata Library").join("myresult.txt").is_file());
        assert_eq!(
            tc.graph_viz().unwrap(),
            fs::read_to_string(dir.join("Result").join("myresult.gv"))
                .unwrap()
                .trim_end()
        );
        // Not a TRUE/FALSE result (a free variable `y` survives) -- no print.
        assert_eq!(stdout_text(&stdout), "");

        fs::remove_dir_all(&dir).ok();
    }

    /// `_log.txt` is opened unconditionally by `Logging.writeEvalLogsTo`, regardless of
    /// `printDetails` -- only the SECOND (`_detailed_log.txt`) file is gated.
    #[test]
    fn non_headless_mode_opens_the_eval_command_log_file() {
        let (session, dir) = temp_session("non-headless-logfile");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "x < 3",
            Some("r"),
            None,
            &mut stdout,
        )
        .unwrap();

        let result_name = format!("{}r", session.paths().address_for_result());
        assert!(std::path::Path::new(&format!("{result_name}_log.txt")).is_file());
        assert!(
            !std::path::Path::new(&format!("{result_name}_detailed_log.txt")).exists(),
            "printDetails=false must not open the detailed-log file"
        );

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------------
    // TRUE/FALSE print branch -- both headless and non-headless.
    // ------------------------------------------------------------------------

    #[test]
    fn headless_true_result_prints_the_true_line_and_the_automaton_is_marked_trivial() {
        let (session, dir) = temp_session("headless-true");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        // `Ax (x >= 0)`: every x satisfies x >= 0, a closed TRUE result (see
        // `wr_logic::eval`'s own test of the same predicate).
        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 Ax (x >= 0)",
            None,
            None,
            &mut stdout,
        )
        .unwrap();

        assert_eq!(stdout_text(&stdout), "____\nTRUE\n");
        assert!(tc.automaton_pairs()[0]
            .automaton()
            .unwrap()
            .is_true_false_automaton());
        assert!(tc.automaton_pairs()[0]
            .automaton()
            .unwrap()
            .is_true_automaton());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn headless_false_result_prints_the_false_line() {
        let (session, dir) = temp_session("headless-false");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        // `Ax (x >= 5)`: x=0 is a counterexample, a closed FALSE result.
        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 Ax (x >= 5)",
            None,
            None,
            &mut stdout,
        )
        .unwrap();

        assert_eq!(stdout_text(&stdout), "____\nFALSE\n");
        let a = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(a.is_true_false_automaton());
        assert!(!a.is_true_automaton());

        fs::remove_dir_all(&dir).ok();
    }

    /// The nuance this module's docs call out explicitly: a non-headless TRUE/FALSE
    /// result still gets a (degenerate) automaton file written, in addition to the print
    /// -- `writeAutomata` runs unconditionally, before the TRUE/FALSE check.
    #[test]
    fn non_headless_true_result_writes_a_degenerate_automaton_file_and_prints() {
        let (session, dir) = temp_session("non-headless-true");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 Ax (x >= 0)",
            Some("truthy"),
            None,
            &mut stdout,
        )
        .unwrap();

        assert_eq!(stdout_text(&stdout), "____\nTRUE\n");
        let txt_path = dir.join("Result").join("truthy.txt");
        assert!(
            txt_path.is_file(),
            "a TRUE/FALSE result still gets a .txt file"
        );
        assert_eq!(
            fs::read_to_string(&txt_path).unwrap(),
            "true",
            "the degenerate .txt body is the bare string \"true\" (`Fa::true_false_string`)"
        );
        assert!(dir.join("Result").join("truthy.gv").is_file());
        assert_eq!(
            tc.graph_viz().unwrap(),
            fs::read_to_string(dir.join("Result").join("truthy.gv"))
                .unwrap()
                .trim_end()
        );

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------------
    // CAS matrix-export handling (sign-off item #1): a call that passes a non-empty
    // free-variable list still writes the automaton output normally, with no attempt at
    // matrix side-files.
    // ------------------------------------------------------------------------

    #[test]
    fn free_variable_result_writes_automaton_normally_and_the_real_matrix_side_files() {
        let (session, dir) = temp_session("free-var");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        // `free_var_str = Some("x")`: in real Walnut, `eval r x "x < 5";` (an explicit
        // free-variable list on the command line -- see this module's docs on why that,
        // not "does the predicate happen to have a free variable", is what actually
        // triggers `writeMatrices`'s CAS export; verified live against `walnut-java`)
        // passes both validation checks ("x" is a real track label on a non-trivial
        // result), so real Walnut additionally prints "Matrix files:" and writes
        // `r.m`/`r.mpl`/`r.wl`/`r.sage` -- CAS export is no longer DROP scope
        // (`.claude/plans/amber-transcribing-ledger.md`), so this test now asserts that
        // real behavior instead of the signed-off divergence it used to pin.
        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 x < 5",
            Some("r"),
            Some("x"),
            &mut stdout,
        )
        .unwrap();

        // The automaton output is written completely normally.
        assert!(dir.join("Result").join("r.txt").is_file());
        assert!(dir.join("Result").join("r.gv").is_file());
        assert!(dir.join("Automata Library").join("r.txt").is_file());
        assert!(!tc.automaton_pairs()[0]
            .automaton()
            .unwrap()
            .is_true_false_automaton());

        // All four real matrix side-files exist, in `wr_io::matrix_writer::EMITTERS`
        // order (Maple/MATLAB/Mathematica/Sage), each with real, non-empty content.
        let result_dir = dir.join("Result");
        for ext in [".mpl", ".m", ".wl", ".sage"] {
            assert!(
                result_dir.join(format!("r{ext}")).is_file(),
                "expected r{ext} to exist"
            );
        }
        let matrix_output = tc.matrix_output().unwrap();
        assert_eq!(matrix_output.len(), 4);
        for m in &matrix_output {
            assert!(
                !m.trim().is_empty(),
                "expected real matrix content, got empty"
            );
        }
        // Free variable "x" has domain {0, 1} (msd_2), which contains 0 -- the fix-up
        // representative is `M_x_0` (`AutomatonMatrixWriter.java:76-78`'s "prefer 0"
        // branch), and the Maple emitter's fixup comment hardcodes "v" (see
        // `wr_io::matrix_writer::MapleEmitter`'s doc comment on that specific quirk).
        assert!(
            matrix_output[0].contains("M_x_0"),
            "Maple output should reference M_x_0:\n{}",
            matrix_output[0]
        );

        assert_eq!(
            stdout_text(&stdout),
            format!(
                "Matrix files:\n  Maple: {result_dir}/r.mpl\n  MATLAB/Octave: {result_dir}/r.m\n  \
                 Mathematica: {result_dir}/r.wl\n  Sage: {result_dir}/r.sage\n",
                result_dir = result_dir.to_str().unwrap()
            )
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// The real, live-verified divergence this unit's review found: a non-empty
    /// free-variable list against a TRUE/FALSE (trivial) result is a genuine Java
    /// failure (`AutomatonMatrixWriter.writeMatrix`'s `isTRUE_FALSE_AUTOMATON` guard),
    /// not silent success. `write_automata`/the `TRUE`/`FALSE` print must still have
    /// happened as side effects BEFORE the error, matching Java's real statement order
    /// (`writeMatrices` is called at `EvalDef.java:72`, after `writeAutomata` at `:67`
    /// and the print at `:68-70`).
    #[test]
    fn free_variable_list_on_a_trivial_result_is_an_error_after_the_side_effects_ran() {
        let (session, dir) = temp_session("free-var-trivial");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        // `?msd_2 Ax (x >= 0)` is a closed TRUE result (no free variables at all), so
        // real Walnut's `eval r x "?msd_2 Ax (x >= 0)";` throws.
        let err = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 Ax (x >= 0)",
            Some("r"),
            Some("x"),
            &mut stdout,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            EvalDefError::NoFreeVariableOnTrivialAutomaton
        ));
        assert_eq!(
            err.to_string(),
            "incidence matrices cannot be calculated, because the automaton does not \
             have a free variable."
        );

        // The `write_automata`/`TRUE` print side effects already happened, exactly as in
        // real Java -- the error does not roll them back.
        assert_eq!(stdout_text(&stdout), "____\nTRUE\n");
        assert!(dir.join("Result").join("r.gv").is_file());
        assert!(dir.join("Result").join("r.txt").is_file());
        assert!(dir.join("Automata Library").join("r.txt").is_file());

        // The SAME partial-write quirk `wr_io::matrix_writer`'s own
        // `write_all_validation_error_leaves_a_zero_byte_first_file_and_no_others` pins at
        // the primitive level, confirmed here to survive the real `eval_def` wiring: Java's
        // `writeAll` opens each emitter's `FileWriter` before calling `writeMatrix`, one
        // emitter at a time, so the FIRST emitter (Maple, `EMITTERS`' order) leaves a
        // zero-byte file behind when validation throws, and the remaining three are never
        // attempted.
        let maple = dir.join("Result").join("r.mpl");
        assert!(maple.is_file(), "Maple's file should still be created");
        assert_eq!(
            fs::metadata(&maple).unwrap().len(),
            0,
            "but left at zero bytes"
        );
        for ext in ["m", "wl", "sage"] {
            assert!(
                !dir.join("Result").join(format!("r.{ext}")).exists(),
                "extension .{ext} should never have been attempted"
            );
        }

        fs::remove_dir_all(&dir).ok();
    }

    /// The second real Java validation check: a name in the caller's free-variable list
    /// that is not actually a track label on the (non-trivial) result automaton.
    #[test]
    fn free_variable_name_that_is_not_a_track_label_is_an_error() {
        let (session, dir) = temp_session("free-var-unknown");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        // The result automaton's only real track label is "x"; "z" is not one.
        let err = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 x < 5",
            Some("r"),
            Some("z"),
            &mut stdout,
        )
        .unwrap_err();

        assert!(matches!(err, EvalDefError::NotAFreeVariable { ref name } if name == "z"));
        assert_eq!(
            err.to_string(),
            "incidence matrices for the variable z cannot be calculated, because z is \
             not a free variable."
        );

        // The side effects still ran first, exactly as in real Java.
        assert!(dir.join("Result").join("r.gv").is_file());
        assert!(dir.join("Result").join("r.txt").is_file());

        // Same zero-byte-first-file-then-stop quirk as the trivial-automaton case above --
        // this validation check also fires inside `writeMatrix`, after Maple's `FileWriter`
        // is already open.
        let maple = dir.join("Result").join("r.mpl");
        assert!(maple.is_file(), "Maple's file should still be created");
        assert_eq!(
            fs::metadata(&maple).unwrap().len(),
            0,
            "but left at zero bytes"
        );
        for ext in ["m", "wl", "sage"] {
            assert!(
                !dir.join("Result").join(format!("r.{ext}")).exists(),
                "extension .{ext} should never have been attempted"
            );
        }

        fs::remove_dir_all(&dir).ok();
    }

    /// A name repeated in the caller's free-variable list is its own distinct Java
    /// validation failure, checked after the membership check for each name in list
    /// order (`AutomatonMatrixWriter.java:52-58`).
    #[test]
    fn duplicate_free_variable_name_is_an_error() {
        let (session, dir) = temp_session("free-var-dup");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        let err = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_2 x < 5",
            Some("r"),
            Some("x x"),
            &mut stdout,
        )
        .unwrap_err();

        assert!(matches!(err, EvalDefError::DuplicateFreeVariable { ref name } if name == "x"));
        assert_eq!(err.to_string(), "Duplicate free variable: x");

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------------
    // Error propagation: a real evaluation failure surfaces as `Err`, not a panic.
    // ------------------------------------------------------------------------

    #[test]
    fn a_malformed_predicate_propagates_as_an_eval_error_not_a_panic() {
        let (session, dir) = temp_session("malformed");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        let err = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "x < ",
            None,
            None,
            &mut stdout,
        )
        .unwrap_err();
        assert!(matches!(err, EvalDefError::Eval(_)));

        fs::remove_dir_all(&dir).ok();
    }

    /// The headless case above never exercises [`Logging::with_eval_logs_to`]'s closure
    /// at all -- the non-headless path is a distinct control-flow shape (an early
    /// `?`-return happens INSIDE the closure, and the eval-log file is opened
    /// unconditionally BEFORE the closure runs, per Java's real
    /// `Logging.writeEvalLogsTo`/try-with-resources behavior). Pins: (1) the log file
    /// still gets opened even though evaluation fails, (2) no automaton file is ever
    /// produced (evaluation never got far enough to call `write_automata`), and (3) a
    /// subsequent, independent call reusing the same `Logging` still succeeds normally --
    /// proving the RAII/log-file state was correctly restored on the error path rather
    /// than left corrupted for the next command.
    #[test]
    fn a_malformed_predicate_in_non_headless_mode_propagates_and_leaves_logging_reusable() {
        let (session, dir) = temp_session("malformed-non-headless");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        let err = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "x < ",
            Some("badresult"),
            None,
            &mut stdout,
        )
        .unwrap_err();
        assert!(matches!(err, EvalDefError::Eval(_)));

        let result_name = format!("{}badresult", session.paths().address_for_result());
        assert!(
            std::path::Path::new(&format!("{result_name}_log.txt")).is_file(),
            "the eval-log file is opened unconditionally before the closure runs, even \
             though evaluation inside it failed"
        );
        assert!(
            !dir.join("Result").join("badresult.txt").exists(),
            "no automaton was ever produced, so no .txt should be written"
        );
        assert!(
            !dir.join("Result").join("badresult.gv").exists(),
            "no automaton was ever produced, so no .gv should be written"
        );

        // A subsequent, independent call using the SAME `Logging` instance still
        // succeeds normally -- the error path didn't leave any log-file/RAII state
        // corrupted for the next command.
        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "x < 3",
            Some("goodresult"),
            None,
            &mut stdout,
        )
        .unwrap();
        assert!(dir.join("Result").join("goodresult.txt").is_file());
        assert!(dir.join("Result").join("goodresult.gv").is_file());
        assert_eq!(tc.automaton_pairs().len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------------
    // `$name(…)` over a CUSTOM base (regression, Phase 3b U27)
    // ------------------------------------------------------------------------

    /// `Predicate.putFunction` builds the `$name(…)` token's own number system with Java's
    /// `new NumberSystem(number_system)` (`Token/Function.java:52`) — a constructor that
    /// reads `Custom Bases/` through `Session`. This port originally called
    /// `wr_core::numsys::NumberSystem::new` directly there, which by design has no file
    /// access, so **every** `$name(…)` call under a custom base failed with "Number system
    /// msd_fib is not defined." even though the very same query's relational tokens
    /// resolved it fine.
    ///
    /// The Tier-1 golden corpus caught this: 15 of `IntegrationTest.initialize()`'s own
    /// prelude definitions (`fibonacci_occurs`, `fibonacci_border`, … — every `?msd_fib`
    /// definition that calls another one) failed, silently leaving the library files 60+
    /// later fixtures read from missing. Fixed by routing the construction through
    /// [`wr_logic::predicate_env::PredicateEnv::fresh_number_system`], which `FileLibraries`
    /// implements with the same unmemoized, file-resolving build Java's constructor does.
    #[test]
    fn a_function_token_resolves_a_custom_base_number_system() {
        let (session, dir) = temp_session("fn-custom-base");
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for f in ["msd_fib.txt", "msd_fib_addition.txt"] {
            fs::copy(fixtures.join(f), dir.join("Custom Bases").join(f)).unwrap();
        }
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        // Writes `Automata Library/fibhelper.txt`, whose own header declares `msd_fib`.
        eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_fib i >= 1",
            Some("fibhelper"),
            None,
            &mut stdout,
        )
        .unwrap();

        // The `$`-call: this is the path that used to fail.
        let tc = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "?msd_fib $fibhelper(i) & i <= 5",
            Some("usefib"),
            None,
            &mut stdout,
        )
        .expect("a $name(...) call under a custom base must resolve that base");
        assert_eq!(tc.automaton_pairs().len(), 1);
        assert!(tc.automaton_pairs()[0].automaton().is_some());

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------------
    // A `wr-core` guard panic inside `act()` is an ERROR, not a crash (regression)
    // ------------------------------------------------------------------------

    /// `Main/Commands/EvalDef.compute` wraps each token's `act()` in
    /// `catch (RuntimeException e)` and rethrows with the token's position appended
    /// (`EvalDef.java:123-128`). Several `wr-core` guards port a Java `WalnutException` as a
    /// `panic!`, so without a `catch_unwind` at that same point the port **killed the
    /// process** where Walnut prints a message and carries on.
    ///
    /// This is golden-corpus fixture 190 verbatim — its `func` predicate mixes `msd_3`,
    /// `msd_2` and `msd_10` tracks, so calling it with a repeated argument makes
    /// `wr_core::product`'s cross product see two same-labeled tracks with different
    /// alphabets. `walnut-java` records the expected message in
    /// `src/test/resources/integrationTests/error190.txt`; this test pins the same two lines.
    #[test]
    fn a_wr_core_guard_panic_inside_act_becomes_a_positioned_error() {
        let (session, dir) = temp_session("act-guard-panic");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut stdout = Vec::new();

        eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "(?msd_3 c < 5) & (a = b+1) & (?msd_10 e = 17)",
            Some("func"),
            None,
            &mut stdout,
        )
        .unwrap();

        let err = eval_def_command_with_stdout(
            &session,
            &mut logging,
            &mut fresh,
            false,
            false,
            "Ez,x,y $func(z,x,y,17)",
            Some("mixed"),
            None,
            &mut stdout,
        )
        .expect_err("the cross product's same-label/different-alphabet guard must fire");
        assert_eq!(
            err.to_string(),
            "in computing cross product of two automaton, variables with the same label \
             must have the same alphabet\n\t: char at 8"
        );

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------------
    // `[strategy …]`/`[export …]`: the `DeterminizeContext` threading
    // ------------------------------------------------------------------------

    /// A [`DeterminizeContext`] that records every interaction, so a test can assert
    /// WHICH automaton each `[strategy n …]`/`[export n …]` index actually names.
    ///
    /// `MetaCommands` itself deliberately records nothing (Java's does not either), so the
    /// observability lives here in the test rather than as production state.
    #[derive(Default)]
    struct RecordingCtx {
        next_index: usize,
        /// The strategy to hand back per index; absent entries fall back to `SC`, exactly
        /// as `MetaCommands.getStrategy`'s `getOrDefault` does.
        strategies: std::collections::BTreeMap<usize, wr_core::determinize::Strategy>,
        /// `(index, strategy answered, |Q| of the PRE-determinization automaton)`, in
        /// order. The state count comes from the export hook, which Java offers `A`
        /// before any strategy runs — the same value Walnut prints as
        /// `Determinizing [#n, strategy: S]: <Q> states`.
        seen: Vec<(usize, wr_core::determinize::Strategy, usize)>,
    }

    impl DeterminizeContext for RecordingCtx {
        fn next_automaton_index(&mut self) -> usize {
            let i = self.next_index;
            self.next_index += 1;
            i
        }
        fn strategy(&mut self, automaton_index: usize) -> wr_core::determinize::Strategy {
            self.strategies
                .get(&automaton_index)
                .copied()
                .unwrap_or(wr_core::determinize::Strategy::Sc)
        }
        fn export_pre_determinization(&mut self, request: wr_core::determinize::ExportRequest<'_>) {
            let idx = request.automaton_index;
            let strategy = self.strategy(idx);
            self.seen.push((idx, strategy, request.automaton.fa.q));
        }
    }

    /// Runs `predicate` headlessly with `print_details = true` and the given context.
    fn eval_with_ctx(
        session: &Session,
        predicate: &str,
        ctx: &mut dyn DeterminizeContext,
    ) -> wr_core::automaton::Automaton {
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut out: Vec<u8> = Vec::new();
        let tc = eval_def_command_with_stdout_and_ctx(
            session,
            &mut logging,
            &mut fresh,
            true,
            true,
            predicate,
            None,
            None,
            &mut out,
            Some(ctx),
        )
        .unwrap();
        tc.automaton_pairs()[0].automaton().unwrap().clone()
    }

    /// **The index-alignment pin, anchored on real `walnut-java` output.**
    ///
    /// `[strategy n …]`/`[export n …]` name an automaton by the ORDER of the
    /// determinizations a command performs, so this port is only correct if it performs
    /// exactly the same determinizations, in the same order, as Java. Nothing about the
    /// final language can detect a mistake here (both strategies are correct), so the
    /// only real check is against Walnut's own `Determinizing [#n, strategy: S]: <Q>
    /// states` trace.
    ///
    /// The expectations below are transcribed from `<name>_detailed_log.txt` files
    /// produced by the real `Walnut-all.jar` for these exact six queries (captured the
    /// same way `tests/differential/CAPTURE.md` describes; the same lines are visible in
    /// the committed corpus fixture `details637.txt`). Each tuple is
    /// `(index, |Q| before determinizing)`.
    ///
    /// Four of these six are load-bearing beyond mere counting:
    ///
    /// * `q2`/`q5` — the arithmetic sub-formula's `quantifying:` block gets **no**
    ///   `Determinizing` line at all in Java, because `Automaton.determinizeAndMinimize()`
    ///   skips the dispatcher on an already-deterministic table (`Automaton.java:385`).
    ///   This port used to determinize unconditionally there; every later index would be
    ///   off by one.
    /// * `q3` — `A` is `~E~`, and neither `not(asDFA())` consumes an index (the operand is
    ///   already deterministic), so a `∀` costs the same two indices a `∃` does.
    /// * `q4` — on `lsd`, `fixTrailingZerosProblem` closes with `justMinimize`, not
    ///   `determinizeAndMinimize`, so an `lsd` quantification costs ONE index, not two.
    /// * `q5`/`q6` — the two confirmed WB-039 sites, one per `NumberSystem` call site
    ///   (`constant()` and `multiplication()`); see each one's own comment below.
    ///
    /// A seventh shape — a **nondeterministic library automaton**, whose load-time
    /// determinization Java counts as index `#0` — lives in
    /// [`a_nondeterministic_library_automatons_load_consumes_index_zero_like_java`],
    /// because it needs library files this loop's shared session does not have.
    #[test]
    fn the_automata_index_sequence_matches_real_walnut_java() {
        let (session, dir) = temp_session("idxseq");
        for (predicate, expected) in [
            ("?msd_2 Ei i < x", vec![(0, 2), (1, 2)]),
            (
                "?msd_2 Ex Ey (x + y = z) & (x < y)",
                vec![(0, 2), (1, 4), (2, 3), (3, 3), (4, 2)],
            ),
            ("?msd_2 Ax (x < y) => (x < z)", vec![(0, 4), (1, 2)]),
            ("?lsd_2 Ei i < x", vec![(0, 2)]),
            // WB-039: Java has SIX indices here, not five —
            // `[(0, 2), (1, 2), (2, 5), (3, 6), (4, 6), (5, 3)]`. Its extra `(1, 2)` is
            // `NumberSystem.constant(2)`'s own internal `quantify`, which
            // `NumberSystem.java:936` brackets in `Logging.disablePrint()` precisely so it
            // will NOT be counted — except `disablePrint`/`enablePrint` are plain
            // assignments, so the nested `getConstant(1)` inside that bracket turns
            // printing back on and the rest of `constant(2)` runs counted. It also makes
            // Java's sequence depend on the constant memo: rerun the same command and the
            // leak vanishes. This port never hands `wr_core::numsys` a context, so its
            // sequence is stable and matches Java's stated intent rather than Java's
            // behavior — it is exactly Java's list with the leaked `(1, 2)` entry removed
            // and the survivors renumbered, which is the whole of the divergence. See WB-039 — this divergence is logged and awaiting an explicit
            // decision, not silently taken.
            (
                "?msd_3 Ei Ej (i + j = x) & (i > 2)",
                vec![(0, 2), (1, 5), (2, 6), (3, 6), (4, 3)],
            ),
            // WB-039's SECOND confirmed site, and the one that costs TWO indices rather
            // than one: `NumberSystem.multiplication(n)` for `n > 2` brackets its recursive
            // `multiplication(n-1)`/`multiplication(2)` calls in `Logging.disablePrint()`
            // (`NumberSystem.java:983`, `:1000`, `:1021`) the same non-reentrant way
            // `constant()` does, so the inner bracket's `enablePrint()` re-enables printing
            // for the remainder of the outer one. Java's sequence here is
            // `[(0, 4), (1, 3), (2, 3), (3, 3)]` — its leading `(0, 4)` and `(1, 3)` are
            // both `multiplication`-internal — while this port, which never hands
            // `wr_core::numsys` a context at all, records only the two the query itself
            // performs. Captured live from `Walnut-all.jar` (2026-08-16) on
            // `eval r "?msd_2 Ex (x*3 = y)"::`. Same signed-off divergence as `q5`.
            ("?msd_2 Ex (x*3 = y)", vec![(0, 3), (1, 3)]),
            // WB-039's second site again, this time on its EVEN branch — the parity
            // distinction the entry originally missed. `multiplication(n)`'s `n > 2` arm
            // splits on `n.mod(2)` (`NumberSystem.java:1006`): the odd half quantifies TWO
            // variables (`quantify(P, Set.of(b, c))`, `:1016`) and therefore leaks two
            // indices, but the even half quantifies ONE (`quantify(P, b)`, `:1009`) and
            // leaks exactly one. Java's sequence for `x*4` is `[(0, 4), (1, 4), (2, 3)]`,
            // three entries to this port's two — one leaked, not two. Captured live from
            // `Walnut-all.jar` (2026-08-16) on `eval m1 "?msd_2 Ex (x*4 = y)"::` (three
            // determinizations) versus a byte-identical rerun in the same session (two,
            // the memo having swallowed the leak). Same signed-off divergence as `q5`/`q6`.
            ("?msd_2 Ex (x*4 = y)", vec![(0, 4), (1, 3)]),
        ] {
            let mut ctx = RecordingCtx::default();
            eval_with_ctx(&session, predicate, &mut ctx);
            let actual: Vec<(usize, usize)> = ctx.seen.iter().map(|(i, _, q)| (*i, *q)).collect();
            assert_eq!(actual, expected, "index sequence for `{predicate}`");
        }
        fs::remove_dir_all(&dir).ok();
    }

    /// A `$name(…)` whose library file is a genuine **NFA** costs an index — the FIRST
    /// one — and one whose file is already deterministic costs none.
    ///
    /// This is the lexer half of the index-accounting model. `Predicate.putFunction`
    /// (`Predicate.java:451`) reads the library file while *tokenizing*, before any
    /// operator's `act()` runs, and `AutomatonReader.readAutomaton` (`:88-98`) determinizes
    /// it on the spot if the transition table is nondeterministic. Java's counter is a
    /// global, so that determinization is counted like any other — which makes it index
    /// `#0` of the whole command and shifts every later `[strategy n …]`/`[export n …]`
    /// target by one.
    ///
    /// Both expectations were captured live from the real `Walnut-all.jar` (2026-08-16),
    /// with the exact two library files built below:
    ///
    /// ```text
    /// eval r1 "?msd_2 Ei $mynfa(i) & (i < x)"::   ->  #0: 2, #1: 2, #2: 4, #3: 3
    /// eval r2 "?msd_2 Ei $mydfa(i) & (i < x)"::   ->  #0: 2, #1: 4, #2: 3
    /// ```
    ///
    /// The second case is what keeps the fix narrow, and it is a real distinction rather
    /// than an assumed one: Java reaches `determinizeAndMinimize()` only inside
    /// `if (!A.fa.getT().isDeterministic())`, so a DFA file never touches the dispatcher
    /// and never advances the counter. A port that counted every library load would be
    /// wrong in exactly the opposite direction.
    #[test]
    fn a_nondeterministic_library_automatons_load_consumes_index_zero_like_java() {
        let (session, dir) = temp_session("libnfa");
        let lib = dir.join("Automata Library");
        // State 0's symbol `1` goes to BOTH 0 and 1 — genuinely nondeterministic.
        fs::write(
            lib.join("mynfa.txt"),
            "msd_2\n\n0 0\n0 -> 0\n1 -> 0 1\n\n1 1\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();
        // The same shape with that one extra edge removed: already deterministic.
        fs::write(
            lib.join("mydfa.txt"),
            "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();

        for (predicate, expected) in [
            (
                "?msd_2 Ei $mynfa(i) & (i < x)",
                vec![(0, 2), (1, 2), (2, 4), (3, 3)],
            ),
            (
                "?msd_2 Ei $mydfa(i) & (i < x)",
                vec![(0, 2), (1, 4), (2, 3)],
            ),
        ] {
            let mut ctx = RecordingCtx::default();
            eval_with_ctx(&session, predicate, &mut ctx);
            let actual: Vec<(usize, usize)> = ctx.seen.iter().map(|(i, _, q)| (*i, *q)).collect();
            assert_eq!(actual, expected, "index sequence for `{predicate}`");
        }
        fs::remove_dir_all(&dir).ok();
    }

    /// The context reaches a library load **nested one level down**, inside a word
    /// token's own index sub-predicate — `TD[NU[i]]`, where `NU` is a nondeterministic
    /// word file.
    ///
    /// The lexer recurses to lex a word index (`Predicate.putWord`, `:302-303`, builds a
    /// fresh `Predicate` from the index text) and to lex a function argument
    /// (`putFunction`, `:459-462`). Both recursion points must pass the context down, or a
    /// load performed while lexing the *inner* predicate silently fails to advance the
    /// counter and every later `[strategy n …]`/`[export n …]` in the command targets the
    /// wrong automaton. The existing coverage only exercised the top level
    /// (`a_nondeterministic_library_automatons_load_consumes_index_zero_like_java`), so
    /// the recursion was correct-by-reading, not correct-by-test.
    ///
    /// **Why this shape, and why it ends in an error.** Walnut has no *successful* query
    /// in which a nested sub-predicate performs a library load: a word index and a
    /// function argument are both TERM positions, and every token that loads a library
    /// file yields a `WordExpression` (a word) or a boolean automaton (a function), which
    /// `Word.act`/`Function.act` refuse there. Confirmed live against `Walnut-all.jar`
    /// (2026-08-16) for all four combinations — `TD[NU[i]]`, `TD[NU[i]+0]`, `$gfun(NU[i])`
    /// and (as the reviewer of this unit noted) `$f($g(i))`, which the grammar rejects
    /// outright. So the nested load is reachable only on a path that then fails, and the
    /// observable this test pins is precisely that: **the nested load happens, and is
    /// counted, BEFORE the type error**. Real Java on
    /// `eval n1 "?msd_2 Ei TD[NU[i]] = @1"::` prints, in order:
    ///
    /// ```text
    /// NFA input: determinizing.
    ///  Determinizing [#0, strategy: SC]: 2 states
    /// computing NU[...] / computed NU[i] / computing TD[...]
    /// argument 1 of function TD cannot be of type Main.EvalComputations.Expressions.WordExpression
    /// ```
    ///
    /// — one determinization, index `#0`, the `NU` load, and then the failure. This
    /// asserts the same sequence and the same message.
    #[test]
    fn the_context_reaches_a_load_nested_inside_a_word_index() {
        let (session, dir) = temp_session("nestedctx");
        let words = dir.join("Word Automata Library");
        // Nondeterministic (state 0's symbol `1` goes to both 0 and 1) and NOT an
        // `isFAO` — every output is 0 or 1 — so `AutomatonReader.java:87-96` determinizes
        // it on load rather than throwing `nonDeterministicO`. That distinction is what
        // makes a *word* file able to consume an index at all.
        fs::write(
            words.join("NU.txt"),
            "msd_2\n\n0 0\n0 -> 0\n1 -> 0 1\n\n1 1\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();
        // The outer word: deterministic, and a genuine DFAO (output 2), so its own load
        // consumes nothing and cannot be confused with `NU`'s.
        fs::write(
            words.join("TD.txt"),
            "msd_2\n\n0 1\n0 -> 0\n1 -> 1\n\n1 2\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();

        let mut ctx = RecordingCtx::default();
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let mut out: Vec<u8> = Vec::new();
        let err = eval_def_command_with_stdout_and_ctx(
            &session,
            &mut logging,
            &mut fresh,
            true,
            true,
            "?msd_2 Ei TD[NU[i]] = @1",
            None,
            None,
            &mut out,
            Some(&mut ctx),
        )
        .expect_err("Walnut rejects a word expression in an index position");

        // The nested load was counted — the whole point. Java: `Determinizing [#0,
        // strategy: SC]: 2 states`, and nothing else before the error.
        let seen: Vec<(usize, usize)> = ctx.seen.iter().map(|(i, _, q)| (*i, *q)).collect();
        assert_eq!(
            seen,
            vec![(0, 2)],
            "the `NU` load inside `TD[...]`'s index must consume index #0, as it does in Java"
        );
        // ...and the failure that follows is Java's, verbatim.
        assert!(
            err.to_string().contains(
                "argument 1 of function TD cannot be of type \
                 Main.EvalComputations.Expressions.WordExpression"
            ),
            "{err}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// `[strategy n BRZ]` really switches the algorithm for THAT determinization and no
    /// other, and the answer is unchanged.
    ///
    /// The proxy for "Brzozowski actually ran" is not the answer (both strategies compute
    /// the same language — that is the point of the equivalence assertion below) but the
    /// recorded strategy the dispatcher asked for and got at each index, plus the fact
    /// that only the targeted index differs.
    #[test]
    fn a_strategy_metacommand_switches_exactly_the_named_determinization() {
        let (session, dir) = temp_session("stratsel");
        let predicate = "?msd_2 Ex Ey (x + y = z) & (x < y)";

        let mut sc_ctx = RecordingCtx::default();
        let sc = eval_with_ctx(&session, predicate, &mut sc_ctx);

        let mut brz_ctx = RecordingCtx {
            strategies: [(3, wr_core::determinize::Strategy::Brz)]
                .into_iter()
                .collect(),
            ..RecordingCtx::default()
        };
        let brz = eval_with_ctx(&session, predicate, &mut brz_ctx);

        use wr_core::determinize::Strategy;
        assert_eq!(
            sc_ctx.seen.iter().map(|(_, s, _)| *s).collect::<Vec<_>>(),
            vec![Strategy::Sc; 5]
        );
        assert_eq!(
            brz_ctx.seen.iter().map(|(_, s, _)| *s).collect::<Vec<_>>(),
            vec![
                Strategy::Sc,
                Strategy::Sc,
                Strategy::Sc,
                Strategy::Brz,
                Strategy::Sc
            ],
            "only index 3 may be answered with Brzozowski"
        );
        // Same automata offered at every index, i.e. switching the strategy at index 3
        // did not perturb WHICH automaton later indices name.
        assert_eq!(
            sc_ctx
                .seen
                .iter()
                .map(|(i, _, q)| (*i, *q))
                .collect::<Vec<_>>(),
            brz_ctx
                .seen
                .iter()
                .map(|(i, _, q)| (*i, *q))
                .collect::<Vec<_>>()
        );

        // ... and the answer is the same language (`CLAUDE.md`'s prime directive #1:
        // semantic equivalence, never structural identity).
        let mut a = sc.clone();
        let mut b = brz.clone();
        a.fa.totalize(0);
        b.fa.totalize(0);
        assert_eq!(wr_core::equiv::language_equivalent(&a.fa, &b.fa), Ok(true));
        fs::remove_dir_all(&dir).ok();
    }

    /// The no-metacommand case must be bit-for-bit what it was before the context existed:
    /// passing `None` runs plain subset construction everywhere and touches nothing else.
    #[test]
    fn no_context_computes_exactly_what_the_default_sc_context_does() {
        let (session, dir) = temp_session("noctx");
        for predicate in [
            "?msd_2 Ei i < x",
            "?msd_2 Ex Ey (x + y = z) & (x < y)",
            "?lsd_2 Ei i < x",
        ] {
            let mut logging =
                Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
            let mut fresh = FreshIdentifiers::new();
            let mut out: Vec<u8> = Vec::new();
            let plain = eval_def_command_with_stdout(
                &session,
                &mut logging,
                &mut fresh,
                false,
                false,
                predicate,
                None,
                None,
                &mut out,
            )
            .unwrap();

            let mut ctx = RecordingCtx::default();
            let with_ctx = eval_with_ctx(&session, predicate, &mut ctx);

            let plain = plain.automaton_pairs()[0].automaton().unwrap();
            assert_eq!(plain.fa.q, with_ctx.fa.q, "state count for `{predicate}`");
            assert_eq!(plain.fa.o, with_ctx.fa.o, "outputs for `{predicate}`");
            assert_eq!(plain.fa.d, with_ctx.fa.d, "transitions for `{predicate}`");
        }
        fs::remove_dir_all(&dir).ok();
    }
}
