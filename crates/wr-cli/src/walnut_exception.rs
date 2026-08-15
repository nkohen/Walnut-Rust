// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/WalnutException.java` (139 LOC) — Walnut's centralized exception-message
//! factory, ported as a **message-string module**: one `pub fn` per Java `static`
//! factory, returning the exact `String` that factory's `WalnutException` carries.
//!
//! # Why a message module and not one crate-wide error enum
//!
//! `PORTING.md` maps `WalnutException` to "`Result<T, WalnutError>` with a real error
//! enum", and every crate here has done exactly that — *module-locally*. `wr-core`'s
//! `MinimizeError`/`ProductError`, `wr-logic`'s `LexError`/`ActError`/`EvalError`,
//! `wr-io`'s `ReadError`/`BaWriteError`, and this crate's `AlphabetError`/`RegError`/
//! `EvalDefError` each own the variants their own module can produce and spell the Java
//! text verbatim in their `Display`. That idiom is deliberate (see `predicate.rs`'s
//! `LexError` docs) and this module does **not** try to replace it with one god-enum.
//!
//! What this module adds is the piece that idiom lacks: **one place where every one of
//! Java's 33 factories' message text is written down, verbatim, and pinned by a test.**
//! Treat the functions below as the *reference* wording. If any other module in this
//! workspace ever renders one of these messages differently, that module is wrong.
//!
//! # Audit — where each factory's text is (or is not) already produced
//!
//! Taken 2026-08-13 by grepping every `crates/*/src` tree for the message's distinctive
//! substring. "not ported" means no Rust code in this workspace produces that text yet
//! (usually because the Java call site is in a still-unported command).
//!
//! | Java factory | already produced by | note |
//! |---|---|---|
//! | `alphabetExceedsSize` | `wr-core` `regex.rs` | |
//! | `alphabetIsEmpty` | `wr-core` `fa.rs` | |
//! | `arrayOverflow` | *not ported* | thrown from `FA`'s array sizing |
//! | `bricsNFA` | `wr-core` `regex.rs` | |
//! | `convertDFAOIntoFunction` | *not ported* | `convert` command — U24 |
//! | `divisionByZero` | `wr-core` `numsys.rs`, `word_automaton.rs`; `wr-logic` `token.rs` | |
//! | `errorCommand` | *not ported* | see the trailing-period quirk below |
//! | `fileDoesNotExist` | `wr-logic` `predicate_env.rs`; this crate's `session.rs` | |
//! | `fileEmpty` | `wr-logic` `predicate_env.rs` | |
//! | `fileHasConflict` | `wr-logic` `predicate_env.rs` | |
//! | `internalMacro` | `wr-logic` `predicate.rs` | |
//! | `invalidBind` | `wr-core` `automaton.rs` | |
//! | `invalidCommand` | *not ported before this unit* | now [`invalid_command`], used by `crate::prover` |
//! | `invalidCommandUse` | `wr-core` `product.rs` (its own `crossProduct` wording) | the *command* wording is new here |
//! | `invalidOperator` | `wr-logic` `token.rs` | |
//! | `invalidDualOperators` | `wr-logic` `token.rs` | |
//! | `morphismNegative` | *not ported* | `Morphism` — U24 |
//! | `morphismNotUniform` | *not ported* | `Morphism` — U24 |
//! | `negativeConstant` | `wr-core` `numsys.rs`; `wr-logic` `expr.rs` | |
//! | `noSuchCommand` | *not ported before this unit* | now [`no_such_command`] |
//! | `nonDeterministic` | `wr-core` `logicalops.rs` | |
//! | `nonDeterministicO` | `wr-core` `automaton.rs`; `wr-io` `reader.rs`; `wr-logic` `predicate_env.rs` | **double-period quirk**, preserved everywhere |
//! | `notFreeVariable` | `wr-core` `logicalops.rs`; `wr-logic` `token.rs` | |
//! | `numberSystemCannotCompare` | *not ported* | |
//! | `operatorMissing` | `wr-logic` `predicate.rs` | |
//! | `operatorTwoVariables` | `wr-core` `numsys.rs` | |
//! | `unbalancedBracket` | `wr-logic` `predicate.rs` | |
//! | `unbalancedParen` | `wr-logic` `token.rs`, `predicate.rs` | |
//! | `undefinedStatement` | `wr-logic` `predicate_env.rs` | |
//! | `undefinedToken` | `wr-logic` `predicate.rs` | |
//! | `unexpectedFormat` | *not ported before this unit* | now [`unexpected_format`], used by `crate::meta_commands`/`crate::prover_helper` |
//! | `unexpectedOperator` | `wr-core` `numsys.rs` | |
//!
//! # Quirks preserved verbatim (do **not** "fix" these)
//!
//! * [`non_deterministic_o`] ends in **two** periods — `"NFAOs are not supported.."`
//!   (`WalnutException.java:100`). Every existing port in this workspace already carries
//!   the double period; the test below pins it here too.
//! * [`unexpected_format`] and [`unexpected_operator`] have **no space** after the colon
//!   (`"Unexpected format:" + format`), unlike every other colon-carrying message here.
//! * [`error_command`] and [`invalid_command_use`] end in a period; [`invalid_command`]
//!   does not. Three neighbouring command-level messages, three different shapes.
//! * [`undefined_statement`] reads `"Undefined statement: line at N of file A"` — "line
//!   at", not "line N" — while [`file_has_conflict`] says "line N". Both verbatim.
//! * `negativeConstant(int)` (`:87-89`) simply delegates to `negativeConstant(String)`
//!   via `Integer.toString`, so there is one Rust function here, not two.
//!
//! # Signature deviations from Java
//!
//! Two factories take an `Expression` and read two things off it — `toString()` and
//! `getClass().getName()`. `Expression` is `wr-logic`'s type and this crate does not
//! depend on it for message text, so [`invalid_operator`]/[`invalid_dual_operators`] take
//! those two projections as `&str` instead of the object. `wr-logic`'s `token.rs`, which
//! actually raises these, builds the same text from the same two projections.

// ---------------------------------------------------------------------------
// The 33 factories, in `WalnutException.java`'s own declaration order
// ---------------------------------------------------------------------------

/// `WalnutException.alphabetExceedsSize(int)` (`:22-24`).
pub fn alphabet_exceeds_size(size: i32) -> String {
    format!("size of input alphabet exceeds the limit of {size}")
}

/// `WalnutException.alphabetIsEmpty()` (`:25-27`).
pub fn alphabet_is_empty() -> String {
    "Output alphabet is empty".to_string()
}

/// `WalnutException.arrayOverflow(String, long)` (`:29-31`).
pub fn array_overflow(v: &str, count: i64) -> String {
    format!("Array overflow: {v} is of size {count} which can't be handled by Java arrays")
}

/// `WalnutException.bricsNFA()` (`:33-35`). Note Walnut's own typo, `dk.bricks` (the
/// library is `dk.brics`) — preserved.
pub fn brics_nfa() -> String {
    "cannot set an automaton of type Automaton to a non-deterministic automaton of type \
     dk.bricks.automaton.Automaton"
        .to_string()
}

/// `WalnutException.convertDFAOIntoFunction()` (`:37-39`).
pub fn convert_dfao_into_function() -> String {
    "Cannot convert a Word Automaton into a function".to_string()
}

/// `WalnutException.divisionByZero()` (`:41-43`).
pub fn division_by_zero() -> String {
    "division by zero".to_string()
}

/// `WalnutException.errorCommand(String)` (`:45-47`).
pub fn error_command(cmd: &str) -> String {
    format!("Error using the {cmd} command.")
}

/// `WalnutException.fileDoesNotExist(String)` (`:49-51`).
pub fn file_does_not_exist(address: &str) -> String {
    format!("File does not exist: {address}")
}

/// `WalnutException.fileEmpty(String)` (`:52-54`).
pub fn file_empty(address: &str) -> String {
    format!("File is empty or contains only comments/whitespace: {address}")
}

/// `WalnutException.fileHasConflict(String, long)` (`:55-58`). Note the argument order:
/// Java takes `(address, lineNumber)` but interpolates the line number first.
pub fn file_has_conflict(address: &str, line_number: i64) -> String {
    format!(
        "A file that declares 'true'/'false' must not contain other statements: \
         line {line_number} of file {address}"
    )
}

/// `WalnutException.internalMacro(int)` (`:60-62`).
pub fn internal_macro(index: i32) -> String {
    format!("a function/macro cannot be called from inside another function/macro's argument list: char at {index}")
}

/// `WalnutException.invalidBind()` (`:64-66`).
pub fn invalid_bind() -> String {
    "invalid use of method bind".to_string()
}

/// `WalnutException.invalidCommand(String)` (`:68-70`) — no trailing period, unlike its
/// two neighbours.
pub fn invalid_command(command: &str) -> String {
    format!("Invalid command: {command}")
}

/// `WalnutException.invalidCommandUse(String)` (`:72-74`).
pub fn invalid_command_use(command: &str) -> String {
    format!("Invalid use of the {command} command.")
}

/// `WalnutException.invalidOperator(String, Expression)` (`:76-78`). See this module's
/// docs on why the `Expression` becomes two `&str` projections.
pub fn invalid_operator(op: &str, operand: &str, operand_type: &str) -> String {
    format!("operator {op} cannot be applied to the operand {operand} of type {operand_type}")
}

/// `WalnutException.invalidDualOperators(String, Expression, Expression)` (`:80-82`).
pub fn invalid_dual_operators(op: &str, a: &str, b: &str, a_type: &str, b_type: &str) -> String {
    format!(
        "operator {op} cannot be applied to operands {a} and {b} of types {a_type} and \
         {b_type} respectively"
    )
}

/// `WalnutException.morphismNegative()` (`:83`).
pub fn morphism_negative() -> String {
    "Cannot promote a morphism with negative values.".to_string()
}

/// `WalnutException.morphismNotUniform()` (`:85`).
pub fn morphism_not_uniform() -> String {
    "A morphism applied to a word automaton must be uniform.".to_string()
}

/// `WalnutException.negativeConstant(String)` (`:91-93`), which the `int` overload
/// (`:87-89`) delegates to.
pub fn negative_constant(a: &str) -> String {
    format!("negative constant {a}")
}

/// `WalnutException.noSuchCommand()` (`:95-97`).
pub fn no_such_command() -> String {
    "No such command exists.".to_string()
}

/// `WalnutException.nonDeterministic()` (`:99`).
pub fn non_deterministic() -> String {
    "NFA found when expecting a DFA.".to_string()
}

/// `WalnutException.nonDeterministicO()` (`:100`) — **two** trailing periods, verbatim.
pub fn non_deterministic_o() -> String {
    "NFAOs are not supported..".to_string()
}

/// `WalnutException.notFreeVariable(String)` (`:102-104`).
pub fn not_free_variable(s: &str) -> String {
    format!("Variable {s} in the list of quantified variables is not a free variable.")
}

/// `WalnutException.numberSystemCannotCompare()` (`:105-107`).
pub fn number_system_cannot_compare() -> String {
    "Number system cannot be compared.".to_string()
}

/// `WalnutException.operatorMissing(int)` (`:108-110`).
pub fn operator_missing(index: i32) -> String {
    format!("An operator is missing: char at {index}")
}

/// `WalnutException.operatorTwoVariables(String)` (`:112-114`).
pub fn operator_two_variables(operator: &str) -> String {
    format!("the operator {operator} cannot be applied to two variables")
}

/// `WalnutException.unbalancedBracket(int)` (`:116-118`).
pub fn unbalanced_bracket(index: i32) -> String {
    format!("unbalanced bracket: char at {index}")
}

/// `WalnutException.unbalancedParen(int)` (`:120-122`).
pub fn unbalanced_paren(index: i32) -> String {
    format!("unbalanced parenthesis: char at {index}")
}

/// `WalnutException.undefinedStatement(long, String)` (`:124-126`) — "line at N", not
/// "line N" (contrast [`file_has_conflict`]).
pub fn undefined_statement(line_number: i64, address: &str) -> String {
    format!("Undefined statement: line at {line_number} of file {address}")
}

/// `WalnutException.undefinedToken(int)` (`:128-130`).
pub fn undefined_token(position: i32) -> String {
    format!("Undefined token: char at {position}")
}

/// `WalnutException.unexpectedFormat(String)` (`:132-134`) — no space after the colon.
pub fn unexpected_format(format: &str) -> String {
    format!("Unexpected format:{format}")
}

/// `WalnutException.unexpectedOperator(String)` (`:136-138`) — no space after the colon.
pub fn unexpected_operator(op: &str) -> String {
    format!("Unexpected operator:{op}")
}

// ---------------------------------------------------------------------------
// Inline `new WalnutException(...)` sites this crate's U21 surface needs
// ---------------------------------------------------------------------------

/// `ProverHelper.exportAutomata`'s `txt` arm (`ProverHelper.java:29-30`) — not a
/// `WalnutException` *factory*, an inline `new WalnutException(...)`, but the same kind of
/// literal and it belongs with them.
pub fn exporting_to_txt_is_redundant() -> String {
    format!(
        "Exporting to {} is redundant; this is the input format",
        crate::prover::TXT_EXTENSION
    )
}

/// `MetaCommands.parseMetaCommands`'s inline throw (`MetaCommands.java:92`).
pub fn metacommands_require_double_colon() -> String {
    "Metacommands are currently only supported for commands ending in ::".to_string()
}

/// `Session.createSubdirectories`'s inline throw (`Session.java:132`) — no space after
/// the colon, like [`unexpected_format`].
pub fn could_not_create_directory(dir: &str) -> String {
    format!("Couldn't create directory:{dir}")
}

/// `DeterminizationStrategies.Strategy.fromString`'s failure (`:61`). Java throws
/// `IllegalArgumentException` here, not `WalnutException` — see
/// `crate::meta_commands::MetaCommandError`'s `LoggableError` triage.
pub fn no_strategy_found(name: &str) -> String {
    format!("No strategy found for: {name}")
}

/// `java.lang.NumberFormatException`'s own message, as `Integer.parseInt` builds it
/// (`NumberFormatException.forInputString`). Reachable from `MetaCommands.addStrategy`/
/// `addExport`, which call `Integer.parseInt` on an unvalidated metacommand token.
pub fn number_format_exception(input: &str) -> String {
    format!("For input string: \"{input}\"")
}

// ---------------------------------------------------------------------------
// Panic → catchable-error boundary
// ---------------------------------------------------------------------------

/// Runs `f`, converting a panic escaping it into `Err(panic message)`.
///
/// # Why this exists
///
/// Several `wr-core` primitives replicate a Java `RuntimeException` (usually a
/// `WalnutException`) as a `panic!`/`assert!` — see e.g.
/// `wr_core::logicalops::right_quotient`'s subset guard and
/// `wr_core::product`'s `create_basic_automaton` guards. In Java those are **caught**:
/// `Prover.dispatch`'s handler prints them and the REPL keeps going, so one bad
/// `rightquo`/`combine` costs you a command, not a session. In Rust a panic has no
/// `catch_unwind` boundary anywhere in this workspace, so the same input **kills the
/// process** — losing an entire `load`ed batch file. That is strictly less faithful than
/// Java, not a ported quirk.
///
/// Changing those `wr-core` primitives' signatures to return `Result` is a wider
/// cross-cutting decision (they have other, infallibility-assuming callers), so the
/// boundary is drawn here instead, at the CLI command that Java itself wraps in a
/// try/catch. The panic message is the Java exception message verbatim wherever the
/// `wr-core` guard ported one; where it is this crate's own wording (the WB-010 encode
/// panic, whose Java counterpart is an `ArrayIndexOutOfBoundsException` with a JVM-generated
/// message), the text necessarily differs — the *behavior* (report and continue) is what
/// is being matched.
///
/// # Console noise
///
/// Rust's default panic hook writes `thread '…' panicked at …` to stderr before
/// unwinding. That has no Java analogue, so the hook is silenced **for the duration of
/// this call on this thread only** (a thread-local flag consulted by a wrapper hook
/// installed once). Panics anywhere else — including on other threads running
/// concurrently, which matters for the test harness — still print normally.
pub fn catch_walnut_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    install_quiet_hook();
    let result = SILENCE_PANIC.with(|s| {
        let previous = s.get();
        s.set(true);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        s.set(previous);
        r
    });
    result.map_err(|payload| panic_payload_message(&payload))
}

/// The panic message, however it was raised: `panic!("literal")` yields a `&'static str`
/// payload, `panic!("{fmt}")`/`assert!(cond, "{fmt}")` a `String`. Anything else has no
/// message at all, which Java's `getMessage()` also models as `null`; the placeholder
/// matches what `Throwable.toString()` would show for a message-less exception.
fn panic_payload_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        String::new()
    }
}

thread_local! {
    static SILENCE_PANIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

static QUIET_HOOK: std::sync::Once = std::sync::Once::new();

/// Installs (once, process-wide) a panic hook that defers to whatever hook was in place
/// before, except on a thread that is currently inside [`catch_walnut_panic`].
fn install_quiet_hook() {
    QUIET_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if SILENCE_PANIC.with(|s| s.get()) {
                return;
            }
            previous(info);
        }));
    });
}

/// `Image.determineImageNumberSystemPrefix`'s inline throw (`Main/Commands/Image.java:50`).
pub fn image_requires_unary_word_automaton(word_name: &str) -> String {
    format!("Image requires a unary word automaton: {word_name}")
}

/// `Join.joinCommand`'s inline throw (`Main/Commands/Join.java:53`).
pub fn join_input_count_mismatch(automaton_name: &str) -> String {
    format!(
        "Number of inputs of word automata {automaton_name} does not match number of \
         inputs specified."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_walnut_panic_passes_a_success_through_untouched() {
        assert_eq!(catch_walnut_panic(|| 6 * 7), Ok(42));
    }

    #[test]
    fn catch_walnut_panic_recovers_both_panic_payload_shapes() {
        // `panic!("literal")` / `assert!(cond, "literal")` -> `&'static str` payload.
        assert_eq!(
            catch_walnut_panic(|| panic!("Second A's alphabet must be a subset")),
            Err::<(), _>("Second A's alphabet must be a subset".to_string())
        );
        // A formatted message -> `String` payload.
        let digit = 7;
        assert_eq!(
            catch_walnut_panic(|| panic!("digit {digit} not in track 0's alphabet")),
            Err::<(), _>("digit 7 not in track 0's alphabet".to_string())
        );
    }

    /// The silencing flag must be scoped to the guarded call: a panic raised AFTER one
    /// completes still reaches the default hook (otherwise a genuine bug elsewhere in the
    /// process would be swallowed silently).
    #[test]
    fn catch_walnut_panic_restores_the_silence_flag_on_both_paths() {
        let _ = catch_walnut_panic(|| panic!("inner"));
        assert!(!SILENCE_PANIC.with(|s| s.get()));
        let _ = catch_walnut_panic(|| ());
        assert!(!SILENCE_PANIC.with(|s| s.get()));
    }

    /// Every message, pinned verbatim against `WalnutException.java`. This is the whole
    /// point of the module: if one of these strings is ever "cleaned up", this test fails.
    #[test]
    fn every_message_matches_walnut_exception_verbatim() {
        assert_eq!(
            alphabet_exceeds_size(255),
            "size of input alphabet exceeds the limit of 255"
        );
        assert_eq!(alphabet_is_empty(), "Output alphabet is empty");
        assert_eq!(
            array_overflow("Q", 5_000_000_000),
            "Array overflow: Q is of size 5000000000 which can't be handled by Java arrays"
        );
        assert_eq!(
            brics_nfa(),
            "cannot set an automaton of type Automaton to a non-deterministic automaton of type dk.bricks.automaton.Automaton"
        );
        assert_eq!(
            convert_dfao_into_function(),
            "Cannot convert a Word Automaton into a function"
        );
        assert_eq!(division_by_zero(), "division by zero");
        assert_eq!(error_command("eval"), "Error using the eval command.");
        assert_eq!(
            file_does_not_exist("Automata Library/x.txt"),
            "File does not exist: Automata Library/x.txt"
        );
        assert_eq!(
            file_empty("a.txt"),
            "File is empty or contains only comments/whitespace: a.txt"
        );
        assert_eq!(
            file_has_conflict("a.txt", 7),
            "A file that declares 'true'/'false' must not contain other statements: line 7 of file a.txt"
        );
        assert_eq!(
            internal_macro(3),
            "a function/macro cannot be called from inside another function/macro's argument list: char at 3"
        );
        assert_eq!(invalid_bind(), "invalid use of method bind");
        assert_eq!(invalid_command("foo;"), "Invalid command: foo;");
        assert_eq!(
            invalid_command_use("reg"),
            "Invalid use of the reg command."
        );
        assert_eq!(
            invalid_operator("+", "y=1", "Main.EvalComputations.Expressions.VariableExpression"),
            "operator + cannot be applied to the operand y=1 of type Main.EvalComputations.Expressions.VariableExpression"
        );
        assert_eq!(
            invalid_dual_operators("<", "x", "y", "A", "B"),
            "operator < cannot be applied to operands x and y of types A and B respectively"
        );
        assert_eq!(
            morphism_negative(),
            "Cannot promote a morphism with negative values."
        );
        assert_eq!(
            morphism_not_uniform(),
            "A morphism applied to a word automaton must be uniform."
        );
        assert_eq!(negative_constant("-1"), "negative constant -1");
        assert_eq!(no_such_command(), "No such command exists.");
        assert_eq!(non_deterministic(), "NFA found when expecting a DFA.");
        assert_eq!(
            not_free_variable("x"),
            "Variable x in the list of quantified variables is not a free variable."
        );
        assert_eq!(
            number_system_cannot_compare(),
            "Number system cannot be compared."
        );
        assert_eq!(operator_missing(4), "An operator is missing: char at 4");
        assert_eq!(
            operator_two_variables("*"),
            "the operator * cannot be applied to two variables"
        );
        assert_eq!(unbalanced_bracket(2), "unbalanced bracket: char at 2");
        assert_eq!(unbalanced_paren(7), "unbalanced parenthesis: char at 7");
        assert_eq!(
            undefined_statement(3, "f.txt"),
            "Undefined statement: line at 3 of file f.txt"
        );
        assert_eq!(undefined_token(9), "Undefined token: char at 9");
        assert_eq!(unexpected_operator("&"), "Unexpected operator:&");
    }

    /// The double period is real (`WalnutException.java:100`). Called out separately from
    /// the bulk test so a future "typo fix" reads as deliberate vandalism, not an accident.
    #[test]
    fn non_deterministic_o_keeps_its_double_period() {
        assert_eq!(non_deterministic_o(), "NFAOs are not supported..");
        assert!(non_deterministic_o().ends_with(".."));
    }

    /// `unexpectedFormat` has no space after its colon — also verbatim.
    #[test]
    fn unexpected_format_has_no_space_after_the_colon() {
        assert_eq!(unexpected_format("pdf"), "Unexpected format:pdf");
    }

    #[test]
    fn inline_walnut_exception_sites_match_too() {
        assert_eq!(
            exporting_to_txt_is_redundant(),
            "Exporting to .txt is redundant; this is the input format"
        );
        assert_eq!(
            metacommands_require_double_colon(),
            "Metacommands are currently only supported for commands ending in ::"
        );
        assert_eq!(
            could_not_create_directory("Result/"),
            "Couldn't create directory:Result/"
        );
        assert_eq!(no_strategy_found("XYZ"), "No strategy found for: XYZ");
        assert_eq!(number_format_exception("x"), "For input string: \"x\"");
        assert_eq!(
            image_requires_unary_word_automaton("multi"),
            "Image requires a unary word automaton: multi"
        );
        assert_eq!(
            join_input_count_mismatch("unaryAuto"),
            "Number of inputs of word automata unaryAuto does not match number of inputs \
             specified."
        );
    }
}
