// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/EvalDef.java`'s shared postfix-token executor + the final `Predicate`
//! assembly — Phase 3a's U11.
//!
//! # This is the Phase-3a integration checkpoint
//!
//! Every previous Phase 3a unit ported one *piece* of the decision procedure in
//! isolation, each proven correct only against its own unit tests: the lexer/parser
//! (U2–U4, `predicate.rs`/`token.rs`), `RelationalOperator`/`ArithmeticOperator`'s
//! semantics (U9), and `LogicalOperator`'s boolean connectives + quantifier-elimination
//! driving logic (U10). None of those units, on their own, prove the pieces actually
//! *compose* — that a real predicate string lexes to a postfix token stream whose tokens,
//! walked left to right against one operand stack, drive `wr-core`'s already-shipped
//! engine (determinize/minimize/product/∃-projection) to a genuine final `Automaton`.
//! This module is that proof: [`compute`] is the postfix-token executor Java's
//! `EvalDef.compute` is, and this module's own tests are the first place in this whole
//! phase a literal predicate string goes in and a real [`wr_core::automaton::Automaton`]
//! comes out, using only `wr-logic` + `wr-core` — no `wr-cli`, no `wr-io`.
//!
//! # `EvalDef.java`'s shape, and what actually needed porting here
//!
//! `EvalDef` has four public entry points; only one (`compute`, `:105-158`) is genuine
//! shared logic. The other three are thin, `wr-cli`-scope wrappers around it that this
//! unit does **not** port (per the plan's U11/U11b/U15 split):
//!
//! * `evalDefCommand` (`:50-77`) — the real `eval`/`def` CLI command. Resolves a result
//!   file name from `Session`, opens `Logging.writeEvalLogsTo`, calls `compute`, then
//!   writes the resulting automaton to disk (`M.writeAutomata`), prints a `TRUE`/`FALSE`
//!   line, optionally exports CAS matrix files, and wraps everything in a `TestCase`
//!   (U11b). Every one of those extra steps needs `Session`/`wr-io`/`TestCase` — U14/U15
//!   territory.
//! * `computeHeadless` (`:79-91`) — the no-result-name `eval`/`def` branch: parses,
//!   calls `compute`, prints the `TRUE`/`FALSE` line, wraps in a (mostly-empty)
//!   `TestCase`. This unit's [`evaluate`]/[`evaluate_with_logging`] port everything
//!   *except* the `System.out.println`/`TestCase` wrapping — the literal-predicate-
//!   string-to-`Automaton` pipeline is the useful, `wr-cli`-independent core of this
//!   method, generalized per the plan's note that `image` reuses the same shape.
//! * `getImageEval` (`:93-99`) — `image`'s own call site (`Main.Commands.Image`, out of
//!   scope for this unit and not deep-read per this port's token-efficiency discipline):
//!   parse, `compute`, return `c.result.M`. This is *exactly* [`evaluate`]/
//!   [`evaluate_with_logging`]'s shape already — `image` differs from `eval`/`def` only
//!   in what it does with the returned `Automaton` afterward (no result-file write, no
//!   per-intermediate `Logging` context swap — `getImageEval` never calls
//!   `Logging.writeEvalLogsTo`, unlike `evalDefCommand`), which is exactly why this
//!   module exposes the pipeline as a plain function returning `Automaton` rather than
//!   wrapping it in anything `eval`/`def`-specific. No over-generalization beyond that
//!   was needed: `image`'s real call site needs nothing this module doesn't already do.
//!
//! # `compute`'s own shape (`EvalDef.compute`, `:105-158`)
//!
//! A `Stack<Expression>` operand stack, walked once against `predicate.getPostOrder()`:
//! call `t.act(expressions)`, catch any failure and re-throw it with `": char at " +
//! t.getPositionInPredicate()` appended (after logging the ORIGINAL exception's
//! truncated stack trace), then — after the whole postorder sequence has run — validate
//! the stack holds exactly one operand and that operand is an `AutomatonExpression`.
//! [`compute`] below is a direct, faithful translation: [`EvalError::Act`] is the
//! caught-and-rethrown branch, [`EvalError::TooManyResults`]/[`EvalError::NoResult`]/
//! [`EvalError::ResultNotAutomaton`] are the three post-loop validation branches.
//!
//! `Token::act` here never takes `&dyn PredicateEnv` (only `Predicate` construction —
//! U3/U4's lexer — does), so unlike [`evaluate_with_logging`]/[`evaluate`], [`compute`]
//! itself takes no environment parameter at all.
//!
//! # `Logging` threading (per `predicate_env.rs`'s Ruling 4 and this crate's own
//! established practice)
//!
//! `predicate_env.rs`'s Ruling 4 and this crate's U9/U10 module docs both say the same
//! thing in different words: `Logging.indent()`/`dedent()`/`logEvaluationStep(...)` calls
//! sprinkled through individual `act()` bodies are deliberately NOT ported piecemeal —
//! "the logging context joins `PredicateEnv`/`FreshIdentifiers` as something U11's
//! postfix-token executor threads through." This module is that thread: [`compute`]
//! takes `&mut Logging` and reproduces `EvalDef.compute`'s own three `Logging` call
//! sites (`logEvaluationStep` per completed operator step, `indent()` after each,
//! `resetIndent()` + a final timing line at the end, `printTruncatedStackTrace` on
//! failure).
//!
//! Timing text (`"...ms"`) is a direct, mechanical translation of Java's
//! `System.currentTimeMillis()` deltas via [`std::time::Instant`]; per `CLAUDE.md`'s
//! Prime Directive #1, Walnut's own test suite normalizes timing out of compared text,
//! so exact millisecond values are never fixture-significant — only the call *shape*
//! (which lines get logged, in what order, at what indent) is.
//!
//! ## DEFERRED GAP: the per-`act()` `Logging` calls are NOT ported (log-text fidelity)
//!
//! **This is an honest, known, tested-as-currently-incomplete gap — not a claim that
//! those calls don't matter.** Java's individual `act()` bodies *do* call `Logging`, and
//! those calls *do* have captured effects:
//!
//! * `RelationalOperator.java:96-97` (`logAndPrint("computing " + …)` + `Logging.indent()`)
//!   and `:175-176` (`Logging.dedent()` + `logAndPrint("computed " + …)`).
//! * `LogicalOperator.java:78-79`, `:95-96`, `:105-106`, `:112-113`, `:123-124`, `:160`.
//! * `Word.java:54`, `:78`.
//!
//! `Logging.logDetail` (`Main/Logging.java:204-221`) appends to `commandLog` whenever
//! `!evalLogFilesActive` — which is exactly the `computeHeadless`/`getImageEval` path this
//! module ports — and to `detailedLog` when `printDetails`. `getDetailedLog()` is what
//! `TestCase` captures, i.e. exactly what Tier-1's `details*` golden fixtures compare.
//!
//! **Concrete, verified divergence.** For `?msd_2 x<5 & x>1`, real Walnut's
//! `Session/<ts>/Result/global_log.txt` holds ten lines:
//!
//! ```text
//! computing x<5
//! computed x<5
//! x<5:4 states - 28ms
//!  computing x>1
//!  computed x>1
//!  x>1:3 states - 1ms
//!   computing x<5&x>1
//!   computed x<5&x>1
//!   (x<5&x>1):4 states - 0ms
//! Total computation time: 30ms.
//! ```
//!
//! This port's [`Logging::command_log`] for the same query holds only four of them — the
//! three `X:N states - Nms` summaries plus the total, which come from [`compute`]'s own
//! per-iteration logging and are correct. Every `computing X`/`computed X` pair, which in
//! Java comes from *inside* the token's `act()` call, is absent. A second, related
//! divergence: Java's `RelationalOperator.act` calls `indent()` unconditionally but
//! `dedent()` only on success, so a *failing* relational op leaks `+1` indent into the
//! next command's log (five spaces vs. this port's four) — also not replicated here.
//!
//! **Why it is not fixed in this unit.** The fix is threading `&mut Logging` into every
//! `Token::act`/`Operator::act`/`Word::act` body across the already-reviewed U4/U9/U10
//! code — a substantial unit of engineering in its own right, and one that touches code
//! outside this unit's diff. It is **not** needed for Phase 3a's exit criterion (which is
//! automaton-level semantic equivalence, per `CLAUDE.md`'s Prime Directive #1), but it
//! **must land before Phase 3b's U27**, the golden-corpus unit that compares the
//! `details*` fixtures — the one place `CLAUDE.md` signs off on chasing exact
//! traversal-order/log-text parity rather than semantic equivalence.
//!
//! [`tests::command_log_pins_the_currently_incomplete_per_act_logging_gap`] pins the
//! current (four-line) output verbatim, so closing this gap is a deliberate, visible test
//! change rather than something that drifts unnoticed.
//!
//! ## Related, smaller logging divergence: a `∀`-closed formula's logged state count
//!
//! For `?msd_2 Ax (x >= 5)` real Walnut logs `(A x x>=5):4 states`; this port logs
//! `0 states`. Both agree on the FALSE verdict — this is logging fidelity, not a
//! decision-procedure defect. Root cause is **pre-existing Phase-2 `wr-core` code**, not
//! this unit: [`crate::logicalops`]'s Java counterpart flips the `TRUE_FALSE` flag on an
//! already-trivial automaton *in place* (leaving `Q` untouched), whereas
//! `wr_core::logicalops::not` materializes a fresh trivial automaton with `q == 0`. This
//! module is simply the first thing in the port that *prints* that count, which is why it
//! is documented (and pinned by
//! [`tests::forall_closed_formula_logs_a_zero_state_count_a_known_divergence`]) here
//! rather than fixed here.
//!
//! ## Scope note (RESOLVED in Phase 3b's L1): `lsd_k` numeration
//!
//! When this module landed, "composes end-to-end" meant **`msd` numeration only**: every
//! `lsd_k` query beyond a bare variable-to-variable comparison failed at
//! [`wr_core::quantify`] with a `QuantifyError::UnsupportedLsdFixup`, because Phase 2
//! never wired `fixTrailingZerosProblem` into `quantify`. That was pre-existing `wr-core`
//! scope debt this module's own unit could neither introduce nor fix, and it was much
//! wider than "lsd + an explicit quantifier": `wr_core::numsys` calls `quantify` to build
//! its own automata, so `?lsd_2 x >= 2` — no user-written quantifier anywhere — failed
//! too.
//!
//! Phase 3b's L1 wired the branch up (see [`wr_core::quantify`]'s "The lsd fixup"
//! section). [`tests::lsd_numeration_evaluates_end_to_end`] is the flipped regression
//! test — it used to assert the rejection and now asserts the computed language.
//!
//! # `LoggableError for ActError`
//!
//! [`wr_core::logging::LoggableError`] is the seam `Logging::print_truncated_stack_trace`
//! needs, and [`ActError`] (this crate's single union of every failure `Token::act` can
//! report) is the natural — and, as of this unit, first — implementer. Per that trait's
//! own docs, `is_handled()` mirrors Java's `e instanceof WalnutException` triage: almost
//! every [`ActError`] variant corresponds to a real, deliberately-thrown
//! `WalnutException` and is `handled` (message-only console/log line, no stack frames).
//! The two documented exceptions are genuine Walnut (Java) bugs already logged in
//! `docs/WALNUT-BUGS.md` — real, UNCAUGHT `NullPointerException`s, not `WalnutException`s
//! — reported here as `is_handled() == false` with the closest honest `kind()` text
//! (`"java.lang.NullPointerException"`) and an empty `stack_trace_lines()` (this port has
//! no JVM frames to report; see `wr_core::logging`'s own module docs on why frame text
//! has no Rust analogue and is a documented, not silent, fidelity limit):
//!
//! * [`crate::expr::ExprError::RepeatedIdentifierMissingNumberSystem`] — WB-013.
//! * [`ActError::Infinite`] (wrapping [`wr_core::infinite::InfiniteError`]) — WB-002.

use std::fmt;
use std::time::Instant;

use wr_core::automaton::Automaton;
use wr_core::determinize::DeterminizeContext;
use wr_core::logging::{LoggableError, Logging};
use wr_core::numsys::NumSysError;
use wr_core::walnut_panic::catch_walnut_panic;

use crate::expr::{ExprError, Expression};
use crate::predicate::{LexError, Predicate};
use crate::predicate_env::{FreshIdentifiers, PredicateEnv};
use crate::token::{ActError, Token};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure [`compute`]/[`evaluate_with_logging`]/[`evaluate`] can report.
///
/// Module-local, per this crate's established idiom (see `predicate.rs`'s `LexError`
/// docs on the same point) rather than one unified `WalnutError`.
#[derive(Debug)]
pub enum EvalError {
    /// Lexing/tokenizing the predicate string itself failed — a [`Predicate::new`]/
    /// [`Predicate::with_context`] failure. Never reaches [`compute`] (that function
    /// only ever sees an already-successfully-tokenized [`Predicate`]); only
    /// [`evaluate_with_logging`]/[`evaluate`] (the "final `Predicate` assembly" half of
    /// this unit) can produce this variant.
    Lex(LexError),
    /// `EvalDef.compute`'s `catch (RuntimeException e)` (`:123-128`): one token's
    /// `act()` failed. `position` is `t.getPositionInPredicate()`, matching Java's
    /// `message += lineSeparator + "\t: char at " + t.getPositionInPredicate()` — see
    /// this type's [`fmt::Display`] impl for the exact reproduced text.
    Act { source: ActError, position: usize },
    /// `EvalDef.compute`'s `expressions.size() > 1` branch (`:135-149`): more than one
    /// operand remained on the stack after the whole postorder sequence ran (a
    /// malformed predicate missing an operator). `leftover` holds each remaining
    /// operand's `Display` text, in the same order Java's message lists them — see this
    /// type's `Display` impl's doc comment on why that order is "original push order,"
    /// not stack (LIFO) order, despite Java building it via a double `Stack` reversal.
    TooManyResults { leftover: Vec<String> },
    /// `EvalDef.compute`'s `expressions.isEmpty()` branch (`:150-151`): the postorder
    /// sequence was empty (an all-whitespace, or otherwise vacuous, predicate string).
    NoResult,
    /// `EvalDef.compute`'s final `else` branch (`:153-156`): the lone remaining operand
    /// was not an `AutomatonExpression` — e.g. a bare `T[i]` with no relational operator
    /// to resolve the [`Expression::Word`], or a bare arithmetic/variable expression
    /// with no comparison. `description` is that operand's `Display` text, kept for
    /// programmatic inspection even though (matching Java) it is not part of the
    /// rendered message.
    ResultNotAutomaton { description: String },
}

impl From<LexError> for EvalError {
    fn from(e: LexError) -> Self {
        EvalError::Lex(e)
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Lex(e) => write!(f, "{e}"),
            // Verbatim `EvalDef.compute`'s rethrow text (`:126`): the original
            // exception's message, a line separator, then a tab and "char at N".
            EvalError::Act { source, position } => {
                writeln!(f, "{source}")?;
                write!(f, "\t: char at {position}")
            }
            // Verbatim `EvalDef.compute`'s multi-result message (`:136-148`). Java
            // builds `tmp` by draining `expressions` into it (reversing LIFO order to
            // FIFO) and then draining `tmp` back out (reversing again) — net effect,
            // traced by hand: each remaining operand prints in the order it was
            // originally pushed onto the stack (bottom of stack first), which is
            // exactly `leftover`'s own iteration order here (index 0 = first pushed).
            EvalError::TooManyResults { leftover } => {
                writeln!(f, "Cannot evaluate the following into a single automaton:")?;
                for item in leftover {
                    writeln!(f, "{item}")?;
                }
                write!(f, "Probably some operators are missing.")
            }
            // Verbatim `EvalDef.compute` (`:151`).
            EvalError::NoResult => write!(f, "Evaluation ended in no result."),
            // Verbatim `EvalDef.compute` (`:155`) — Java's message does not include the
            // offending operand's text, only its own fixed wording.
            EvalError::ResultNotAutomaton { description: _ } => {
                write!(
                    f,
                    "The final result of the evaluation is not of type automaton"
                )
            }
        }
    }
}

impl std::error::Error for EvalError {}

/// The [`ExprError`] half of [`ActError`]'s `is_handled` triage. Exhaustive (no `_` arm)
/// for the reason given on [`LoggableError::is_handled`]'s impl below.
fn expr_error_is_handled(e: &ExprError) -> bool {
    match e {
        // Real, deliberately-thrown `WalnutException`s — see each variant's own docs for
        // the Java throw site.
        ExprError::AutomatonArgumentWrongArity { .. }
        | ExprError::AutomatonArgumentUnlabeled { .. }
        | ExprError::InvalidType { .. } => true,
        // WB-013: a real, UNCAUGHT `NullPointerException` in Java, not a
        // `WalnutException`.
        ExprError::RepeatedIdentifierMissingNumberSystem { .. } => false,
        ExprError::NumberSystem(e) => num_sys_error_is_handled(e),
    }
}

/// The [`NumSysError`] half of [`ActError`]'s `is_handled` triage — reachable both
/// directly ([`ActError::NumberSystem`]) and nested under [`ExprError::NumberSystem`], so
/// both routes share this one classification rather than drifting apart. Exhaustive (no
/// `_` arm) for the reason given on [`LoggableError::is_handled`]'s impl below.
///
/// The three `false` arms are *not* currently reachable through [`Token::act`] (the first
/// two can only fire at lex time, when the number-system name is first resolved; the third
/// needs `WordAutomaton`'s per-state DFAO arithmetic, which this port has not ported), so
/// this classification has no live behavioral effect today. It is written correctly anyway:
/// getting it right once, against the Java throw sites, is cheaper than rediscovering it
/// the day one of the three does become reachable.
fn num_sys_error_is_handled(e: &NumSysError) -> bool {
    match e {
        // Java throws a bare `StringIndexOutOfBoundsException` out of
        // `determineMsdOrLsd` (`NumberSystem.java:268-270`) — no guard, no
        // `WalnutException` (`NumberSystemTest.testBogusNS` asserts exactly that).
        NumSysError::MalformedName(_) => false,
        // Java's `Integer.parseInt` (`NumberSystem.java:325`, `:242`) throws an unchecked
        // `NumberFormatException`.
        NumSysError::BaseNotAnI32(_) => false,
        // Java's `BigInteger.intValueExact()` (`ArithmeticOperator.java:237`) throws an
        // unchecked `ArithmeticException`.
        NumSysError::ArithmeticIntOverflow(_) => false,
        // Everything else is a real, deliberately-thrown `WalnutException`…
        NumSysError::NotDefined(_)
        | NumSysError::InvalidBase(_)
        | NumSysError::NegativeConstant(_)
        | NumSysError::OperatorTwoVariables(_)
        | NumSysError::UnexpectedArithmeticOperator(_)
        | NumSysError::UnexpectedOperator(_)
        | NumSysError::ConstantDividedByVariable
        | NumSysError::DivisionByZero
        | NumSysError::MultiplicationByZero
        | NumSysError::AdditionInputCount(_)
        | NumSysError::AdditionAlphabetMissingZero(_)
        | NumSysError::AdditionAlphabetMissingOne(_)
        | NumSysError::AdditionAlphabetsDiffer(_)
        | NumSysError::LessThanInputCount(_)
        | NumSysError::LessThanAlphabetMismatch(_) => true,
        // …with two port-local exceptions to that sentence, both reported as `handled`
        // (message-only) since neither has any JVM frames to report: `Quantify` wraps
        // `wr_core::quantify`'s own internal surfaces (see `ActError::Quantify` below),
        // and `UnsupportedNegativeBase` is this port's declared, deliberate divergence
        // (Phase 3a U5) rather than anything Java throws.
        NumSysError::Quantify(_) | NumSysError::UnsupportedNegativeBase(_) => true,
    }
}

/// [`wr_core::logging::LoggableError`] for [`ActError`] — see this module's docs on the
/// `is_handled()` triage. This is the first (and, as of this unit, only) implementer of
/// that trait in the workspace; a future `wr-cli`-scope `WalnutException`-equivalent
/// type (Phase 3b's U21, per `wr_core::logging`'s own module docs) may supersede it for
/// call sites above this crate, but every failure `Token::act` itself can produce is an
/// `ActError`, so this is the correct (and only) implementer `compute` needs.
impl LoggableError for ActError {
    /// **Exhaustive on purpose** — no `_` arm, and none in the two helpers below either.
    /// This triage answers "would Java have caught this as a `WalnutException`, or does it
    /// escape as an uncaught runtime exception?", which is a per-variant judgement about a
    /// specific Java throw site. A fail-open `!matches!(self, A | B)` default would silently
    /// classify every future [`ActError`] variant as `handled`; written as a total match, a
    /// new variant is a compile error until someone decides its classification deliberately.
    fn is_handled(&self) -> bool {
        match self {
            // Every `TokenError`/`RemoveLeadingZerosError` variant is a deliberately-thrown
            // `WalnutException` in Java (see each type's own per-variant docs, which cite
            // the throw site).
            ActError::Token(_) | ActError::RemoveLeadingZeros(_) => true,
            ActError::Expr(e) => expr_error_is_handled(e),
            ActError::NumberSystem(e) => num_sys_error_is_handled(e),
            // `wr_core::quantify`'s errors are one real `WalnutException`
            // (`NotFreeVariable`) plus this port's own internal-invariant surface
            // (`Minimize`), which has no Java exception behind it at all. Reported as
            // `handled` so they render as a message-only line rather than inventing JVM
            // frames this port cannot produce.
            ActError::Quantify(_) => true,
            // WB-002: a real, UNCAUGHT `NullPointerException` in Java, not a
            // `WalnutException`.
            ActError::Infinite(_) => false,
            // A recovered `wr-core` guard panic. Every guard this variant can currently carry
            // ports a deliberately-thrown Java `WalnutException` (`wr_core::product`'s
            // same-label/different-alphabet guard, `wr_core::logicalops`'s quotient subset
            // guard, …), so it is `handled`: a message-only line, no invented JVM frames.
            // Note the classification is about the JAVA throw site, not about the fact that
            // this port used `panic!` to model it.
            ActError::Thrown(_) => true,
        }
    }

    fn message(&self) -> Option<String> {
        // No `ActError` variant models a null Java message (every one of this crate's
        // module-local error enums renders real, non-empty text — see e.g.
        // `predicate_env.rs`'s docs on `PredicateEnvError`), so this is always `Some`.
        Some(self.to_string())
    }

    fn kind(&self) -> String {
        match self {
            ActError::Expr(ExprError::RepeatedIdentifierMissingNumberSystem { .. })
            | ActError::Infinite(_) => "java.lang.NullPointerException".to_string(),
            // The three `NumSysError`s `num_sys_error_is_handled` classifies as unhandled
            // each name a *different* uncaught JVM exception — see that function's own
            // per-variant comments for the throw site each one stands in for.
            ActError::NumberSystem(NumSysError::MalformedName(_))
            | ActError::Expr(ExprError::NumberSystem(NumSysError::MalformedName(_))) => {
                "java.lang.StringIndexOutOfBoundsException".to_string()
            }
            ActError::NumberSystem(NumSysError::BaseNotAnI32(_))
            | ActError::Expr(ExprError::NumberSystem(NumSysError::BaseNotAnI32(_))) => {
                "java.lang.NumberFormatException".to_string()
            }
            ActError::NumberSystem(NumSysError::ArithmeticIntOverflow(_))
            | ActError::Expr(ExprError::NumberSystem(NumSysError::ArithmeticIntOverflow(_))) => {
                "java.lang.ArithmeticException".to_string()
            }
            // Every other variant corresponds to a real `WalnutException` in Java;
            // `kind()` is only ever consulted by `print_truncated_stack_trace` when
            // `is_handled()` is false, so this arm is unreachable in practice but kept
            // honest rather than a `panic!`/`unreachable!` — some future variant added
            // to `ActError` without updating `is_handled()` above would otherwise panic
            // deep inside logging rather than fail loudly at the actual bug.
            _ => "Main.WalnutException".to_string(),
        }
    }

    fn stack_trace_lines(&self) -> Vec<String> {
        // Only consulted when `!is_handled()`. See this module's docs and
        // `wr_core::logging`'s own module docs: JVM stack-frame text has no Rust
        // analogue, so this is an honest empty list, not a silently-wrong guess.
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// compute — `EvalDef.compute` (`EvalDef.java:105-158`)
// ---------------------------------------------------------------------------

/// The shared postfix-token executor: walks `post_order` (a [`Predicate`]'s already-
/// tokenized postorder sequence — see [`Predicate::post_order`]) against one operand
/// stack, calling each [`crate::token::Token::act`] in turn, and returns the single
/// resulting [`Expression::Automaton`] — or the first `Result::Err` any of them, or the
/// stack's own final shape, reports.
///
/// Takes `&[Token]` rather than `&Predicate`: Java's own `compute(Predicate predicate)`
/// immediately does `List<Token> postOrder = predicate.getPostOrder();` (`:107`) and
/// never touches `predicate` again — every other read in the method body is of
/// `postOrder`, `expressions`, or `t`. Taking the slice directly is therefore the more
/// precise translation of what this function actually depends on, not a looser one; it
/// also means this executor is testable against a hand-built token sequence with no
/// [`PredicateEnv`]/lexer involved at all.
///
/// `fresh` is the caller-owned [`FreshIdentifiers`] counter, threaded in rather than
/// constructed here — `predicate_env.rs`'s Ruling 4 is explicit that it is "a plain value
/// type with an owned counter, to be held as one field of the evaluation context that
/// U11's postfix-token executor threads through". It is passed as a sibling parameter of
/// `logging` rather than wrapped in a new `EvalContext` struct because those two *are*
/// the whole evaluation context today; a struct holding exactly them would be pure
/// indirection. (U14's `Session` port is the natural moment to introduce one, and to
/// raise this counter's scope to process-lifetime, which is what Java's `static long`
/// actually has.)
///
/// See this module's docs for the full mapping to `EvalDef.compute`'s Java source,
/// including the `Logging` threading and the exact rethrown-message text.
pub fn compute(
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    post_order: &[Token],
) -> Result<Expression, EvalError> {
    compute_with_ctx(logging, fresh, post_order, None)
}

/// [`compute`] with an explicit [`DeterminizeContext`] — Walnut's `[strategy …]`/
/// `[export …]` metacommand state; see [`Token::act_with_ctx`] for the contract and for
/// why the caller owes the `shouldPrintDetails()` gate.
///
/// The context is threaded across the WHOLE postorder loop, not rebuilt per token:
/// `MetaCommands`' automata counter is per-COMMAND state in Java, which is exactly what
/// makes `[strategy 6 BRZ]` mean "the seventh determinization this command performs".
pub fn compute_with_ctx(
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    post_order: &[Token],
    mut ctx: Option<&mut (dyn DeterminizeContext + '_)>,
) -> Result<Expression, EvalError> {
    let mut stack: Vec<Expression> = Vec::new();
    let time_beginning = Instant::now();

    for t in post_order {
        let time_before = Instant::now();
        // `try { ... t.act(expressions); ... } catch (RuntimeException e)` (`:112-128`).
        // Java's catch covers **every** unchecked exception `act()` can throw, and several
        // `wr-core` guards port a `WalnutException` as a `panic!`/`assert!` rather than as an
        // `Err` (`wr_core::product`'s same-label/different-alphabet guard is the one the
        // Tier-1 corpus exercises, via `error190.txt`). Without this boundary such a guard
        // would kill the process instead of producing Walnut's positioned error message —
        // strictly less faithful than Java. See `wr_core::walnut_panic`.
        let outcome =
            match catch_walnut_panic(|| t.act_with_ctx(fresh, &mut stack, ctx.as_deref_mut())) {
                Ok(inner) => inner,
                Err(message) => Err(ActError::Thrown(message)),
            };
        if let Err(source) = outcome {
            // `Logging.printTruncatedStackTrace(e)` (`:124`) on the ORIGINAL exception,
            // before the position-appending wrapper below is built.
            logging.print_truncated_stack_trace(&source);
            return Err(EvalError::Act {
                position: t.position_in_predicate(),
                source,
            });
        }
        let elapsed_ms = time_before.elapsed().as_millis();

        // `if (t.isOperator() && nextExpression instanceof AutomatonExpression)`
        // (`:117`). `stack.last()` is `expressions.peek()`; `act()` above always
        // leaves at least one operand on success (every `act()` either pushes exactly
        // one result or fails before popping — see each `Token`/`Operator` variant's
        // own docs), so this is never `None` on the success path, but a `None` here is
        // handled as "no log line" rather than panicking, matching the spirit of
        // "the postorder sequence is well-formed" being an input precondition, not
        // something this executor must itself re-prove.
        if t.is_operator() {
            if let Some(top @ Expression::Automaton(ae)) = stack.last() {
                let step = format!("{top}:{} states - {elapsed_ms}ms", ae.m.fa.q);
                logging.log_evaluation_step(&step, false);
                logging.indent();
            }
        }
    }

    // `Logging.resetIndent()` + the final timing line (`:132-133`).
    logging.reset_indent();
    logging.log_evaluation_step(
        &format!(
            "Total computation time: {}ms.",
            time_beginning.elapsed().as_millis()
        ),
        true,
    );

    // The three post-loop validation branches (`:135-157`).
    if stack.len() > 1 {
        return Err(EvalError::TooManyResults {
            leftover: stack.iter().map(ToString::to_string).collect(),
        });
    }
    let result = stack.pop().ok_or(EvalError::NoResult)?;
    if !matches!(result, Expression::Automaton(_)) {
        return Err(EvalError::ResultNotAutomaton {
            description: result.to_string(),
        });
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// The final `Predicate` assembly — lex, then execute, then unwrap
// ---------------------------------------------------------------------------

/// Lexes `predicate_str` (via [`Predicate::new`], U3/U4's tokenizer) and then runs
/// [`compute`] over the result, returning the final [`Automaton`] — the shared shape of
/// `EvalDef.computeHeadless`/`getImageEval` (see this module's docs), generalized just
/// far enough for both without pulling in either's `wr-cli`-scope surroundings
/// (`Session`, result-file writes, `TestCase`, the `TRUE`/`FALSE` console line).
///
/// `logging` is the caller's own [`Logging`] context — real callers (a future `wr-cli`
/// `EvalDef`/`image`) configure it via [`Logging::configure_for_command`] first and
/// inspect [`Logging::command_log`]/[`Logging::detailed_log`] afterward, exactly as
/// Java's `EvalDef` constructor (`Logging.configureForCommand(printSteps,
/// printDetails)`) and `TestCase` construction (`Logging.getDetailedLog()`) do. Tests
/// (and any caller with no interest in step/detail logs) should prefer [`evaluate`].
///
/// `fresh` is threaded through to [`compute`] for the reason given in that function's own
/// docs (`predicate_env.rs`'s Ruling 4). Java's counter is process-global and monotonic
/// across successive `eval` commands; owning it here would restart it per evaluation.
/// That is unobservable *today* — every fresh name Java mints is existentially quantified
/// away inside the same `act()` call, and this codebase has no nested evaluation (no
/// macro/`def` expansion re-entering [`compute`]) — but taking it as a parameter is what
/// lets a caller that does need process-lifetime scoping simply hold one.
pub fn evaluate_with_logging(
    env: &dyn PredicateEnv,
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    predicate_str: &str,
) -> Result<Automaton, EvalError> {
    evaluate_with_logging_and_ctx(env, logging, fresh, predicate_str, None)
}

/// [`evaluate_with_logging`] with an explicit [`DeterminizeContext`] — see
/// [`compute_with_ctx`].
///
/// **Scope note (deliberate, and narrower than Java's):** the context covers the
/// postorder execution only, not [`Predicate::new`]'s lexing. Java's singleton is global,
/// so a `.txt` library automaton that happens to be an NFA also advances the counter when
/// `AutomatonReader` determinizes it (`AutomatonReader.java:92`). Threading a context
/// through [`PredicateEnv`] to reproduce that is a separate decision; no fixture in
/// Walnut's own corpus loads a nondeterministic library automaton under a metacommand, so
/// the gap is recorded here rather than closed blind.
pub fn evaluate_with_logging_and_ctx(
    env: &dyn PredicateEnv,
    logging: &mut Logging,
    fresh: &mut FreshIdentifiers,
    predicate_str: &str,
    ctx: Option<&mut (dyn DeterminizeContext + '_)>,
) -> Result<Automaton, EvalError> {
    let predicate = Predicate::new(env, predicate_str)?;
    match compute_with_ctx(logging, fresh, predicate.post_order(), ctx)? {
        Expression::Automaton(ae) => Ok(ae.m),
        // `compute` already validated this above (`ResultNotAutomaton` otherwise), so
        // any other variant here is an internal-invariant violation, not a real input.
        other => unreachable!(
            "compute() only ever returns Expression::Automaton on success, got {other:?}"
        ),
    }
}

/// [`evaluate_with_logging`] with a throwaway, non-printing [`Logging`] context — for
/// callers (this module's own tests foremost) that only want the final [`Automaton`]
/// and have no interest in step/detail logs or console output. The writers are
/// [`std::io::sink`], not the real process streams: nothing about the "generalize `image`
/// reuses this" scope needs a real caller to see console text from this specific
/// convenience wrapper (a real `wr-cli` caller uses [`evaluate_with_logging`] with its
/// own [`Logging`] instead, exactly as `EvalDef.getImageEval`'s Java call site does with
/// the process's real `System.out`).
pub fn evaluate(env: &dyn PredicateEnv, predicate_str: &str) -> Result<Automaton, EvalError> {
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    // A throwaway counter alongside the throwaway `Logging`, for exactly the same reason:
    // this wrapper's whole point is "I want only the final automaton". See
    // [`evaluate_with_logging`] on why per-evaluation scoping is currently unobservable.
    let mut fresh = FreshIdentifiers::new();
    evaluate_with_logging(env, &mut logging, &mut fresh, predicate_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use wr_core::fa::Fa;

    // ------------------------------------------------------------------------
    // Test helpers
    // ------------------------------------------------------------------------

    /// Runs a single-track word (msd-first digits) through `a`, NFA-style, and reports
    /// whether some run ends in an accepting state. Mirrors `numsys.rs`'s own private
    /// `accepts_single_track_word` test helper (not reusable from here — it's
    /// `wr-core`-test-private — so this is a small, deliberate duplicate, not a new
    /// shared abstraction).
    fn accepts_single_track_msd(a: &Automaton, digits: &[i32]) -> bool {
        assert_eq!(a.alphabet.len(), 1, "single-track helper");
        let mut current = std::collections::BTreeSet::from([a.fa.q0]);
        for &d in digits {
            let sym = a.encode(&[d]);
            let mut next = std::collections::BTreeSet::new();
            for &s in &current {
                if let Some(dests) = a.fa.d[s].get(&sym) {
                    next.extend(dests.iter().copied());
                }
            }
            current = next;
            if current.is_empty() {
                return false;
            }
        }
        current.iter().any(|&s| a.fa.is_accepting(s))
    }

    /// The classic Thue–Morse-parity DFAO: 1 track, `msd_2`, 2 states. State 0 (start)
    /// is "even number of 1-bits seen so far" (output 0); state 1 is "odd" (output 1).
    /// Reading order doesn't matter for a pure XOR/parity walk, so this is well-defined
    /// msd-first (Walnut's convention) despite being usually described lsd-first.
    fn thue_morse_word_automaton() -> Automaton {
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![
                BTreeMap::from([(0, vec![0]), (1, vec![1])]),
                BTreeMap::from([(0, vec![1]), (1, vec![0])]),
            ],
        };
        Automaton::new(fa, vec![vec![0, 1]], Vec::new(), vec![Some(true)])
    }

    // ------------------------------------------------------------------------
    // THE checkpoint test: a literal predicate string -> a real Automaton,
    // using only wr-logic + wr-core. No wr-cli, no wr-io.
    // ------------------------------------------------------------------------

    /// Exactly the plan's own example: `Ex` binds `x`, the body compares that same bound
    /// `x` against two relational operators joined by `&`, so this exercises the parser
    /// (U2-U4), `RelationalOperator`/boolean-`&`/quantifier-elimination (U9/U10) and
    /// `wr-core`'s ∃-projection+determinize engine all in one pass, collapsing to a
    /// closed TRUE/FALSE result (witnessed by x=2, 3, or 4). This is the single test the
    /// unit's Done-when criterion names.
    #[test]
    fn end_to_end_predicate_string_to_automaton_no_wr_cli_no_wr_io() {
        let env = wr_logic_test_env();
        let result = evaluate(&env, "?msd_2 Ex (x < 5 & x >= 2)")
            .expect("a real predicate string must evaluate end-to-end to an Automaton");

        assert!(
            result.fa.is_true_false_automaton(),
            "a fully-quantified closed formula must collapse to a trivial automaton"
        );
        assert!(
            result.fa.is_true_automaton(),
            "witnessed by x=2 (or 3, or 4): the formula is TRUE"
        );
    }

    fn wr_logic_test_env() -> crate::predicate_env::InMemoryPredicateEnv {
        crate::predicate_env::InMemoryPredicateEnv::new()
    }

    // ------------------------------------------------------------------------
    // Boolean connectives + relational/arithmetic comparisons, free-variable form
    // (no quantifier) -- a non-trivial resulting Automaton, checked by direct
    // simulation rather than a trivial TRUE/FALSE collapse.
    // ------------------------------------------------------------------------

    #[test]
    fn boolean_and_relational_composition_yields_the_expected_free_variable_language() {
        let env = wr_logic_test_env();
        let result = evaluate(&env, "x >= 2 & x < 5").expect("boolean+relational must compose");

        assert!(!result.fa.is_true_false_automaton());
        assert_eq!(result.label, vec!["x".to_string()]);

        for i in 0..8 {
            let digits = msd_2_digits(i);
            let expected = (2..5).contains(&i);
            assert_eq!(
                accepts_single_track_msd(&result, &digits),
                expected,
                "i={i}"
            );
        }
    }

    #[test]
    fn arithmetic_operator_composes_through_the_full_pipeline() {
        let env = wr_logic_test_env();
        // x + 1 = 5  =>  x = 4, the only witness.
        let result = evaluate(&env, "x + 1 = 5").expect("arithmetic+relational must compose");
        assert!(!result.fa.is_true_false_automaton());

        for i in 0..8 {
            let digits = msd_2_digits(i);
            assert_eq!(accepts_single_track_msd(&result, &digits), i == 4, "i={i}");
        }
    }

    /// `i`'s msd-first (most-significant-bit-first) binary digits, zero-padded to at
    /// least one digit, with no extraneous leading zero beyond what's needed to
    /// represent `0` itself -- exactly how a `NumberSystem::comparison`-built automaton
    /// expects a query word (matches this crate's other tests' informal convention).
    fn msd_2_digits(i: u32) -> Vec<i32> {
        if i == 0 {
            return vec![0];
        }
        let mut bits = Vec::new();
        let mut n = i;
        while n > 0 {
            bits.push((n & 1) as i32);
            n >>= 1;
        }
        bits.reverse();
        bits
    }

    // ------------------------------------------------------------------------
    // Quantifiers alone (composed with a relational comparison, since a bare
    // quantified variable list needs a body).
    // ------------------------------------------------------------------------

    #[test]
    fn exists_quantifier_eliminates_the_bound_variable() {
        let env = wr_logic_test_env();
        // Ex (x < 3 & y = x): for the given y, does some x < 3 satisfy y = x?
        // True exactly for y in {0, 1, 2}.
        let result = evaluate(&env, "Ex (x < 3 & y = x)")
            .expect("quantifier elimination must compose end-to-end");
        assert!(!result.fa.is_true_false_automaton());
        assert_eq!(result.label, vec!["y".to_string()]);

        for i in 0..8 {
            let digits = msd_2_digits(i);
            let expected = (0..3).contains(&i);
            assert_eq!(
                accepts_single_track_msd(&result, &digits),
                expected,
                "y={i}"
            );
        }
    }

    #[test]
    fn forall_quantifier_composes_via_not_exists_not() {
        let env = wr_logic_test_env();
        // Ax (x >= 5) is FALSE (x=0 is a counterexample) -- exercises A's
        // not-exists-not driving logic (U10) through the full pipeline.
        let result =
            evaluate(&env, "Ax (x >= 5)").expect("forall must compose end-to-end via not/exists");
        assert!(result.fa.is_true_false_automaton());
        assert!(!result.fa.is_true_automaton());
    }

    /// The companion the test above needs: `FALSE` is the verdict a *broken* `¬∃¬` chain
    /// produces most easily (an over-eager complement, an empty projection), so a `∀` suite
    /// that only ever asserts `FALSE` proves very little. `Ax (x >= 0)` is `TRUE` in real
    /// Walnut (every base-2 representation denotes a non-negative integer).
    #[test]
    fn forall_composes_to_a_true_verdict_too() {
        let env = wr_logic_test_env();
        let result = evaluate(&env, "?msd_2 Ax (x >= 0)").expect("forall must compose");
        assert!(result.fa.is_true_false_automaton());
        assert!(
            result.fa.is_true_automaton(),
            "every x satisfies x >= 0, so this is TRUE"
        );
    }

    /// A `∀` that does NOT close the formula: `y` survives as a free variable, so the
    /// result is a real automaton rather than a TRUE/FALSE collapse. `Ax (x < 5 => x < y)`
    /// says "every x below 5 is below y", i.e. exactly `y >= 5`.
    #[test]
    fn forall_with_a_surviving_free_variable_yields_a_real_automaton() {
        let env = wr_logic_test_env();
        let result = evaluate(&env, "?msd_2 Ax (x < 5 => x < y)").expect("forall must compose");
        assert!(!result.fa.is_true_false_automaton());
        assert_eq!(result.label, vec!["y".to_string()]);

        for i in 0..12u32 {
            let digits = msd_2_digits(i);
            assert_eq!(accepts_single_track_msd(&result, &digits), i >= 5, "y={i}");
        }
    }

    // ------------------------------------------------------------------------
    // Word / Function tokens, via InMemoryPredicateEnv -- the environment
    // lookups U3/U4 built specifically so this composition is testable with no
    // filesystem/wr-cli.
    // ------------------------------------------------------------------------

    #[test]
    fn word_token_composes_with_a_relational_operator() {
        let env = crate::predicate_env::InMemoryPredicateEnv::new()
            .with_word("T", thue_morse_word_automaton());
        let result = evaluate(&env, "T[i] = 1").expect("word+relational must compose");

        assert!(!result.fa.is_true_false_automaton());
        assert_eq!(result.label, vec!["i".to_string()]);

        for i in 0..8u32 {
            let digits = msd_2_digits(i);
            let expected = i.count_ones() % 2 == 1;
            assert_eq!(
                accepts_single_track_msd(&result, &digits),
                expected,
                "i={i}"
            );
        }
    }

    /// The captured `def zphi "?msd_2 a < b"` automaton, transcribed verbatim from
    /// `tests/differential/fixtures/u11/zphi_a_lt_b.txt` (captured from the real
    /// `walnut-java` CLI — recipe in `tests/differential/CAPTURE.md`). Two `msd_2` tracks
    /// labeled `a`, `b`; state 0 rejecting, state 1 accepting; note the deliberately
    /// ASYMMETRIC transition table — state 0 has no `(1,0)` edge at all (implicit
    /// rejection), which is exactly what makes a swapped-argument bug detectable.
    ///
    /// Hand-transcribed rather than read from the fixture file because `wr-logic` must
    /// stay free of a `wr-io` dependency; `tests/differential/tests/u11_eval_composition.rs`
    /// checks this same predicate against the fixture file itself.
    fn zphi_a_less_than_b() -> Automaton {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 4,
                o: vec![0, 1],
                d: vec![BTreeMap::new(), BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            Vec::new(),
            vec![Some(true), Some(true)],
        );
        // state 0: (0,0)->0, (0,1)->1, (1,1)->0  [no (1,0) edge]
        for (digits, dest) in [([0, 0], 0), ([0, 1], 1), ([1, 1], 0)] {
            let sym = a.encode(&digits);
            a.fa.d[0].insert(sym, vec![dest]);
        }
        // state 1 (accepting): every input self-loops.
        for digits in [[0, 0], [1, 0], [0, 1], [1, 1]] {
            let sym = a.encode(&digits);
            a.fa.d[1].insert(sym, vec![1]);
        }
        a
    }

    /// Runs a 2-track msd-first word through `a` and reports acceptance. `a`'s tracks are
    /// taken in `a.label` order, so a caller passes digits in whatever order the result's
    /// own labels say — which is exactly what makes argument-order bugs visible.
    fn accepts_two_track_msd(a: &Automaton, w0: &[i32], w1: &[i32]) -> bool {
        assert_eq!(a.alphabet.len(), 2, "two-track helper");
        assert_eq!(
            w0.len(),
            w1.len(),
            "tracks must be equal-length (zero-padded)"
        );
        let mut current = std::collections::BTreeSet::from([a.fa.q0]);
        for (&d0, &d1) in w0.iter().zip(w1.iter()) {
            let sym = a.encode(&[d0, d1]);
            let mut next = std::collections::BTreeSet::new();
            for &s in &current {
                if let Some(dests) = a.fa.d[s].get(&sym) {
                    next.extend(dests.iter().copied());
                }
            }
            current = next;
            if current.is_empty() {
                return false;
            }
        }
        current.iter().any(|&s| a.fa.is_accepting(s))
    }

    /// `Function::act` must bind the call's arguments to the callee's tracks **in order**.
    /// The previous version of this test used a symmetric 1-state, zero-transition
    /// stand-in and asserted only `!is_true_false_automaton()`, which a swapped-argument
    /// bug would have passed; this one uses the real, asymmetric `a < b` automaton and
    /// asserts the two orderings differ.
    #[test]
    fn function_token_binds_its_arguments_in_order() {
        let env = crate::predicate_env::InMemoryPredicateEnv::new()
            .with_function("zphi", zphi_a_less_than_b());

        let mut forward = evaluate(&env, "?msd_2 $zphi(x,y)").expect("function must compose");
        assert!(!forward.fa.is_true_false_automaton());
        assert_eq!(forward.label, vec!["x".to_string(), "y".to_string()]);
        // 01 < 10 holds; 10 < 01 does not.
        assert!(accepts_two_track_msd(&forward, &[0, 1], &[1, 0]), "1 < 2");
        assert!(
            !accepts_two_track_msd(&forward, &[1, 0], &[0, 1]),
            "!(2 < 1)"
        );

        // Swapping the call's arguments must produce a genuinely different language, not
        // the same one. Both are sorted to `[x, y]` track order first so the comparison is
        // over the same track positions -- `wr_core::equiv`'s oracle does NOT detect a
        // label permutation on its own (U8's documented limitation), so without this the
        // check below would be meaningless.
        let mut swapped = evaluate(&env, "?msd_2 $zphi(y,x)").expect("function must compose");
        assert_eq!(swapped.label, vec!["y".to_string(), "x".to_string()]);
        forward.sort_label();
        swapped.sort_label();
        forward.fa.totalize(0);
        swapped.fa.totalize(0);
        assert_eq!(
            wr_core::equiv::automaton_language_equivalent(&forward, &swapped),
            Ok(false),
            "`$zphi(x,y)` (x<y) and `$zphi(y,x)` (y<x) must NOT be the same language — \
             they would be if `Function::act` bound its arguments symmetrically"
        );
    }

    // ------------------------------------------------------------------------
    // The three post-loop validation branches, end-to-end (not just compute()'s
    // own unit shape) -- and the "char at N" position-wrapping on a real act()
    // failure.
    // ------------------------------------------------------------------------

    #[test]
    fn too_many_results_reports_javas_exact_message_shape() {
        let env = wr_logic_test_env();
        // Two juxtaposed relational expressions with no connecting operator.
        let err = evaluate(&env, "x < 5 y < 5").unwrap_err();
        // The LEXER itself catches "an operator is missing" before `compute` ever runs
        // (Predicate.java's own `lastTokenWasOperator` check) -- so this is actually a
        // `Lex` error, not `TooManyResults`. Documented here (not just asserted) because
        // it is easy to assume this input reaches `compute`'s stack-size check when it
        // does not: the executor's `TooManyResults` branch is reachable only by
        // constructing a `Predicate` directly from a hand-built token list.
        assert!(matches!(err, EvalError::Lex(_)), "{err:?}");
    }

    #[test]
    fn compute_reports_too_many_results_for_a_hand_built_postorder_with_no_connective() {
        // `compute` takes a bare `&[Token]` (see its own docs on why), so this bypasses
        // the lexer's adjacency check entirely: two bare NumberLiteral tokens, no
        // operator between them, is not a sequence the real tokenizer would ever
        // produce (see `too_many_results_reports_javas_exact_message_shape` for why
        // this branch appears to be unreachable through real lexing), but `compute`
        // itself must still handle it exactly as `EvalDef.compute` does.
        use crate::token::NumberLiteral;
        use num_bigint::BigInt;
        use std::rc::Rc;
        let ns = Rc::new(wr_core::numsys::NumberSystem::new("msd_2").unwrap());
        let t0 = Token::NumberLiteral(NumberLiteral::new(0, BigInt::from(1), Rc::clone(&ns)));
        let t1 = Token::NumberLiteral(NumberLiteral::new(1, BigInt::from(2), ns));
        let post_order = vec![t0, t1];

        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut fresh = FreshIdentifiers::new();
        let err = compute(&mut logging, &mut fresh, &post_order).unwrap_err();
        match &err {
            EvalError::TooManyResults { leftover } => {
                assert_eq!(leftover, &vec!["1".to_string(), "2".to_string()]);
            }
            other => panic!("expected TooManyResults, got {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "Cannot evaluate the following into a single automaton:\n1\n2\nProbably some \
             operators are missing."
        );
    }

    #[test]
    fn empty_predicate_reports_no_result() {
        let env = wr_logic_test_env();
        let err = evaluate(&env, "   ").unwrap_err();
        assert!(matches!(err, EvalError::NoResult), "{err:?}");
        assert_eq!(err.to_string(), "Evaluation ended in no result.");
    }

    #[test]
    fn act_failure_is_wrapped_with_the_tokens_position() {
        let env = wr_logic_test_env();
        // `y` is never bound to a number system's comparison target in a way that
        // fails at the LEXER level, but comparing two automata-typed operands with `<`
        // is invalid at the `act()` level once both operands already collapsed to
        // automata (`&` between two well-formed sub-predicates, then `<` against a
        // third automaton-typed value) -- constructed here as `(x<5)<(y<5)`, an
        // invalid dual-operand relational comparison.
        //
        // The expected text below is HARDCODED, deliberately: an earlier version of this
        // test read `position` out of the error and then asserted it appeared in that same
        // error's own rendering, which is a tautology — it would have passed even if
        // `position_in_predicate()` always returned `0`. Both literal positions here (`5`
        // with no `?msd_2` prefix, `12` with it) were verified against the real
        // `walnut-java` CLI.
        const MESSAGE: &str = "operator < cannot be applied to operands x<5 and y<5 of types \
             Main.EvalComputations.Expressions.AutomatonExpression and \
             Main.EvalComputations.Expressions.AutomatonExpression respectively";

        let err = evaluate(&env, "(x<5)<(y<5)").unwrap_err();
        assert!(matches!(err, EvalError::Act { .. }), "{err:?}");
        assert_eq!(err.to_string(), format!("{MESSAGE}\n\t: char at 5"));

        // The same formula behind a `?msd_2` prefix: every token's position shifts by the
        // prefix's 7 characters, so the reported position must too.
        let err = evaluate(&env, "?msd_2 (x<5)<(y<5)").unwrap_err();
        assert!(matches!(err, EvalError::Act { .. }), "{err:?}");
        assert_eq!(err.to_string(), format!("{MESSAGE}\n\t: char at 12"));
    }

    /// `EvalDef.compute`'s third post-loop branch (`:153-156`) — the one the other two
    /// tests above do NOT cover. A bare variable is a well-formed postorder sequence that
    /// leaves exactly one operand on the stack, but that operand is a
    /// `VariableExpression`, not an `AutomatonExpression`.
    #[test]
    fn a_bare_variable_reports_result_not_automaton() {
        let env = wr_logic_test_env();
        let err = evaluate(&env, "?msd_2 x").unwrap_err();
        match &err {
            EvalError::ResultNotAutomaton { description } => assert_eq!(description, "x"),
            other => panic!("expected ResultNotAutomaton, got {other:?}"),
        }
        // Byte-identical to the real `walnut-java` CLI's own message for this input.
        assert_eq!(
            err.to_string(),
            "The final result of the evaluation is not of type automaton"
        );
    }

    // ------------------------------------------------------------------------
    // `Logging` output. See this module's docs' "DEFERRED GAP" section: these two
    // tests pin what the port CURRENTLY logs, which is deliberately less than what
    // real Walnut logs. They are here so that closing that gap is a visible,
    // conscious test change rather than silent drift.
    // ------------------------------------------------------------------------

    /// Replaces every `<digits>ms` with `Nms`. `CLAUDE.md`'s Prime Directive #1 notes
    /// Walnut's own test suite normalizes timing out of compared text; wall-clock
    /// milliseconds are the one part of these lines that is not reproducible.
    fn normalize_timing(log: &str) -> String {
        let mut out = String::with_capacity(log.len());
        let mut rest = log;
        while let Some(pos) = rest.find("ms") {
            let head = &rest[..pos];
            let digits_start =
                head.len() - head.chars().rev().take_while(char::is_ascii_digit).count();
            out.push_str(&head[..digits_start]);
            if digits_start < head.len() {
                out.push('N');
            }
            out.push_str("ms");
            rest = &rest[pos + 2..];
        }
        out.push_str(rest);
        out
    }

    /// **Pins a KNOWN-INCOMPLETE output.** Real Walnut's `global_log.txt` for this exact
    /// query has TEN lines; this port produces the four below. The six missing ones are
    /// the `computing X`/`computed X` pairs that Java emits from inside each token's
    /// `act()` body — see this module's docs' "DEFERRED GAP" section for the full listing,
    /// the Java call sites, and why the fix is a separate unit that must land before Phase
    /// 3b's U27.
    #[test]
    fn command_log_pins_the_currently_incomplete_per_act_logging_gap() {
        let env = wr_logic_test_env();
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        // `printDetails = true` so `detailed_log()` is non-empty too (Java's
        // `getDetailedLog` returns "" unless that flag is set) — that buffer is what
        // `TestCase`, and therefore Tier 1's `details*` fixtures, actually compare.
        logging.configure_for_command(false, true);
        let mut fresh = FreshIdentifiers::new();
        evaluate_with_logging(&env, &mut logging, &mut fresh, "?msd_2 x<5 & x>1")
            .expect("the query itself must evaluate fine — only its logging is incomplete");

        let expected = "x<5:4 states - Nms\n \
                        x>1:3 states - Nms\n  \
                        (x<5&x>1):4 states - Nms\n\
                        Total computation time: Nms.";
        assert_eq!(normalize_timing(&logging.command_log()), expected);
        assert_eq!(normalize_timing(&logging.detailed_log()), expected);
    }

    /// **Pins a KNOWN DIVERGENCE, whose root cause is pre-existing Phase-2 `wr-core`
    /// code, not this unit.** Real Walnut logs `(A x x>=5):4 states`; this port logs
    /// `0 states`, because `wr_core::logicalops::not` materializes a fresh trivial
    /// automaton (`q == 0`) where Java flips the `TRUE_FALSE` flag in place and leaves `Q`
    /// alone. The VERDICT is identical either way (both FALSE) — this is logging fidelity
    /// only. See this module's docs for the full note.
    #[test]
    fn forall_closed_formula_logs_a_zero_state_count_a_known_divergence() {
        let env = wr_logic_test_env();
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        logging.configure_for_command(false, false);
        let mut fresh = FreshIdentifiers::new();
        let result = evaluate_with_logging(&env, &mut logging, &mut fresh, "?msd_2 Ax (x >= 5)")
            .expect("must evaluate");

        // The decision procedure agrees with real Walnut …
        assert!(result.fa.is_true_false_automaton());
        assert!(!result.fa.is_true_automaton());
        // … only the logged state count for the `A` step differs (Walnut: 4).
        assert_eq!(
            normalize_timing(&logging.command_log()),
            "x>=5:5 states - Nms\n (A x x>=5):0 states - Nms\nTotal computation time: Nms."
        );
    }

    // ------------------------------------------------------------------------
    // `lsd_k` numeration. See this module's docs' "Scope note".
    // ------------------------------------------------------------------------

    /// `i`'s lsd-first binary digits in exactly `width` positions (least-significant
    /// digit first), or `None` if `i` doesn't fit. Deliberately NOT `msd_2_digits`
    /// reversed-on-the-fly at the call site: writing the reversal once, here, and
    /// asserting its own convention in `lsd_2_digits_writes_the_least_significant_digit_first`
    /// is what stops the whole lsd section from silently agreeing on a flipped
    /// convention with itself.
    fn lsd_2_digits(mut i: u32, width: usize) -> Option<Vec<i32>> {
        let mut out = Vec::with_capacity(width);
        for _ in 0..width {
            out.push((i & 1) as i32);
            i >>= 1;
        }
        if i == 0 {
            Some(out)
        } else {
            None
        }
    }

    #[test]
    fn lsd_2_digits_writes_the_least_significant_digit_first() {
        assert_eq!(lsd_2_digits(6, 4), Some(vec![0, 1, 1, 0]));
        assert_eq!(lsd_2_digits(1, 3), Some(vec![1, 0, 0]));
        assert_eq!(lsd_2_digits(0, 2), Some(vec![0, 0]));
        assert_eq!(lsd_2_digits(9, 3), None);
    }

    /// **This test used to pin a REJECTION.** `?lsd_2 x >= 2` evaluates fine in real
    /// `walnut-java`, but until Phase 3b's L1 it failed here with
    /// `ActError::NumberSystem(NumSysError::Quantify(QuantifyError::UnsupportedLsdFixup))`,
    /// because Phase 2 never wired `fixTrailingZerosProblem` into `wr_core::quantify`.
    /// Note the query contains no user-written quantifier at all — `x >= 2` is a
    /// comparison against a constant `>= 2`, which `NumberSystem` builds by quantifying a
    /// bound copy of the constant away, so the gap reached far past "lsd + `E`/`A`/`I`".
    /// L1 closed it; this now checks the computed language.
    #[test]
    fn lsd_numeration_evaluates_end_to_end() {
        let env = wr_logic_test_env();

        // No user-written quantifier: the `quantify` call is `NumberSystem`'s own.
        let result = evaluate(&env, "?lsd_2 x >= 2").expect("lsd comparison must evaluate");
        assert!(!result.fa.is_true_false_automaton());
        assert_eq!(result.label, vec!["x".to_string()]);
        for i in 0..12u32 {
            let digits = lsd_2_digits(i, 4).unwrap();
            assert_eq!(
                accepts_single_track_msd(&result, &digits),
                i >= 2,
                "x = {i} (lsd digits {digits:?})"
            );
        }
        // The msd spelling of a value the two directions disagree on: 4 is lsd "0010"
        // and msd "0100". Feeding "0100" asks about the value 2 in lsd (>= 2, accepted)
        // while feeding "0010" asks about 4. An engine that silently built the msd
        // automaton would answer these the other way round for `x = 1` ("1000" lsd = 1,
        // rejected; read msd it is 8, which would be accepted) -- already covered by the
        // loop above, and restated here because it is the assertion that fails first if
        // the trailing-zero fixup is swapped for the leading-zero one.
        assert!(!accepts_single_track_msd(&result, &[1, 0, 0, 0]));

        // A user-written quantifier over an lsd variable, which is what the gap was
        // originally reported as. `Ex (x < 5 & x >= 2 & y = x)` is `y in {2, 3, 4}`.
        let result = evaluate(&env, "?lsd_2 Ex (x < 5 & x >= 2 & y = x)")
            .expect("lsd quantifier elimination must evaluate");
        assert_eq!(result.label, vec!["y".to_string()]);
        for i in 0..12u32 {
            let digits = lsd_2_digits(i, 4).unwrap();
            assert_eq!(
                accepts_single_track_msd(&result, &digits),
                (2..5).contains(&i),
                "y = {i} (lsd digits {digits:?})"
            );
        }

        // And a closed lsd formula, so the `A`/`¬∃¬` path is exercised too.
        let result = evaluate(&env, "?lsd_2 Ax (x >= 0)").expect("lsd forall must evaluate");
        assert!(result.fa.is_true_false_automaton() && result.fa.is_true_automaton());
        let result = evaluate(&env, "?lsd_2 Ax (x >= 5)").expect("lsd forall must evaluate");
        assert!(result.fa.is_true_false_automaton() && !result.fa.is_true_automaton());
    }
}
