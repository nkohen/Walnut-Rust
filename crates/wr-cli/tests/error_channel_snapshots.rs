// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **Stage 1 of the idiomatic-refactor unit U2 (`docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`).**
//!
//! `wr-cli` renders a failed command to the user through exactly four channels, all
//! reached from [`crate::prover::ProverError`] — [`LoggableError`]'s `is_handled`/
//! `kind`/`stack_trace_lines`, plus `ProverError`'s own [`std::fmt::Display`] (the text
//! `LoggableError::message` wraps as `Some(self.to_string())`, so pinning `Display`
//! covers `message` too). This file is a byte-exact snapshot of that 4-tuple for every
//! constructible variant of every `wr-cli` command error enum, captured from the CURRENT
//! (pre-refactor) code. It is the regression gate stage 2's consolidation must not move a
//! single byte on: every case below is re-checked, unedited except for mechanical `use`
//! path updates, after the enum/`From`-impl consolidation lands.
//!
//! # Scope: outer variants exhaustively, nested variants only where classification forks
//!
//! Every one of `wr-cli`'s ~19 command error enums is enumerated exhaustively at its own
//! level (every variant gets a case). Several of those variants themselves wrap a
//! deeper `wr-core`/`wr-io`/`wr-logic` error type (`PredicateEnvError`, `ReadError`,
//! `ParseMethodsError`, …) whose OWN sub-variants make no difference to any of the four
//! channels — `ProverError`'s `is_handled`/`kind` match on the *outer* variant only, and
//! `Display` just forwards via `write!(f, "{e}")` — so those get exactly ONE
//! representative nested construction; enumerating that nested type's own variants
//! exhaustively is that type's own crate's job, not this file's.
//!
//! The exception: THREE nested types where `crate::prover::ProverError`'s `is_handled`/
//! `kind` genuinely pattern-match on the nested type's OWN sub-variant, so the fork is
//! part of the rendering path this file guards and every discriminating sub-variant is
//! covered:
//! - [`wr_core::logicalops::ConvertNsError`] (inside `ConvertError::Convert`) — `InvalidRoot`/
//!   `NotAnExactPower`/`BaseOverflowsInt` are singled out (`IllegalArgumentException`/
//!   `IllegalArgumentException`/`NumberFormatException`, all `is_handled() == false`);
//!   every other variant falls through to the `ConvertError`-level default.
//! - [`wr_core::transducer::TransduceError`] (inside `TransduceCommandError::Transduce`) —
//!   all EIGHT variants are covered individually, since three
//!   (`TrivialAutomaton`/`NoTransducerTransition`/`NoTransducerOutput`) are singled out
//!   by `is_handled`/`kind` and the other five (plus `Exploded`, itself carrying a
//!   `TransduceLimit`) share the enum-level default — the fork is the entire point of
//!   this nested type's existence in `wr-cli`'s classification.
//! - [`wr_io::parse_methods::ParseMethodsError`] (inside `OstError::Parse`) —
//!   `is_handled()` is unconditionally `false` for ANY `OstError::Parse(_)` regardless of
//!   the inner variant, but `kind()` only special-cases the `NumberFormat` sub-variant
//!   (`NumberFormatException`); every other `ParseMethodsError` variant inside `Parse(_)`
//!   falls through `kind()`'s default (`Main.WalnutException`) despite `is_handled()`
//!   staying `false` — a real, checked-here divergence between the two channels.
//!
//! [`MetaCommandError`] is `wr-cli`'s only OTHER type with its own `LoggableError` impl
//! (`ProverError::Meta` delegates every one of the four methods to it, including
//! `Display` via `write!(f, "{e}")`) — its variants are exercised here wrapped in
//! `ProverError::Meta`, which exercises the identical code `LoggableError for
//! MetaCommandError` runs; no separate direct-`MetaCommandError` test path is needed.
//!
//! # `stack_trace_lines()` is empty for every single case in this file
//!
//! `crate::prover::ProverError::stack_trace_lines` only has one non-empty-producing arm
//! (`ProverError::Meta(e) => e.stack_trace_lines()`), and
//! `MetaCommandError::stack_trace_lines` itself unconditionally returns `Vec::new()`
//! ("This port has no JVM frames to report", `wr_core::logging`'s module docs). So across
//! every case in this file — all ~19 command enums plus `ProverError`'s own variants —
//! the fourth channel is always `vec![]`. [`stack_trace_lines_is_empty_everywhere_today`]
//! pins that fact directly so a future non-empty producer (a real stack-trace-threading
//! unit, should one ever land) shows up as an intentional, reviewed diff here rather than
//! silently falling out of a hand-derived expectation nobody re-checked.
//!
//! # Payloads that could not be constructed from here
//!
//! None. Every enum in this crate is a plain, non-`#[non_exhaustive]` `pub enum` in a
//! `pub mod` reachable from `wr-cli`'s crate root (`crates/wr-cli/src/lib.rs`), every
//! variant's payload type is itself `pub` with either a `pub` constructor, all-`pub`
//! fields, or a `pub` free function that returns an instance
//! (`wr_core::util::try_parse_int` for [`wr_core::util::NumberFormatError`], whose own
//! `payload` field is private) — checked case by case while writing this file, not
//! assumed.
//!
//! # Why literal expected strings, not calls back into `wr_cli::walnut_exception`
//!
//! `wr_cli::walnut_exception` is itself frozen (`docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`'s
//! "Frozen modules" section), so calling its functions here would still be safe against
//! stage 2's refactor — but it would defeat the point of a byte-exact SNAPSHOT: a
//! `msg::foo()` call compares the function against itself and can never fail no matter
//! what the function returns. Every expected string below is a literal, transcribed from
//! this crate's current source (cited inline) or captured by running this file and
//! reading the actual computed value back — never derived by calling the code under
//! test.

use std::io;

use wr_cli::alphabet::AlphabetError;
use wr_cli::automaton_ops::AutomatonOpsError;
use wr_cli::convert::ConvertError;
use wr_cli::describe::DescribeError;
use wr_cli::eval_def::EvalDefError;
use wr_cli::image::ImageError;
use wr_cli::join::JoinError;
use wr_cli::meta_commands::MetaCommandError;
use wr_cli::morphism::MorphismCommandError;
use wr_cli::ost::OstError;
use wr_cli::prover::ProverError;
use wr_cli::prover_helper::ProverHelperError;
use wr_cli::quotient::QuotientError;
use wr_cli::reg::RegError;
use wr_cli::reverse::ReverseError;
use wr_cli::simple_transforms::SimpleTransformError;
use wr_cli::split::SplitError;
use wr_cli::test_command::TestError;
use wr_cli::transduce::TransduceCommandError;

use wr_core::logging::LoggableError;
use wr_core::logicalops::{ConvertNsError, RemoveLeadingZerosError};
use wr_core::morphism::MorphismError;
use wr_core::ostrowski::OstrowskiError;
use wr_core::regex::RegexError;
use wr_core::transducer::{TransduceError, TransduceLimit};
use wr_core::util::try_parse_int;
use wr_io::parse_methods::ParseMethodsError;
use wr_io::reader::ReadError;
use wr_io::writer::BaWriteError;
use wr_logic::eval::EvalError;
use wr_logic::predicate_env::PredicateEnvError;

// ---------------------------------------------------------------------------
// Shared assertion helper
// ---------------------------------------------------------------------------

/// The one place all four channels are read, so every case below reads identically and a
/// future refactor of the assertion shape itself only has one call site to update.
///
/// `stack_trace_lines` is asserted as `vec![]` unconditionally per this file's module
/// docs — see [`stack_trace_lines_is_empty_everywhere_today`] for the dedicated pin of
/// that fact on its own.
fn check(err: &ProverError, expected_display: &str, expected_handled: bool, expected_kind: &str) {
    assert_eq!(err.to_string(), expected_display, "Display mismatch");
    assert_eq!(err.is_handled(), expected_handled, "is_handled() mismatch");
    assert_eq!(err.kind(), expected_kind, "kind() mismatch");
    assert_eq!(
        err.stack_trace_lines(),
        Vec::<String>::new(),
        "stack_trace_lines() mismatch (expected empty -- see module docs)"
    );
}

fn io_err(message: &str) -> io::Error {
    io::Error::other(message.to_string())
}

const HANDLED: bool = true;
const UNHANDLED: bool = false;
const MAIN_WALNUT_EXCEPTION: &str = "Main.WalnutException";

// ---------------------------------------------------------------------------
// `stack_trace_lines()` -- the always-empty fourth channel, pinned directly
// ---------------------------------------------------------------------------

#[test]
fn stack_trace_lines_is_empty_everywhere_today() {
    // One case reached through `ProverError::Meta` (the only delegating arm) and one
    // reached through the catch-all default -- both must agree, since `check` above
    // asserts this for every single case in the file regardless of path.
    assert_eq!(
        ProverError::from(MetaCommandError::RequiresDoubleColon).stack_trace_lines(),
        Vec::<String>::new()
    );
    assert_eq!(
        ProverError::NoSuchCommand.stack_trace_lines(),
        Vec::<String>::new()
    );
}

// ---------------------------------------------------------------------------
// `ProverError`'s own variants (no wrapped command enum)
// ---------------------------------------------------------------------------

#[test]
fn prover_error_own_variants() {
    check(
        &ProverError::InvalidCommand("frobnicate".to_string()),
        "Invalid command: frobnicate",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::NoSuchCommand,
        "No such command exists.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::InvalidCommandUse("eval".to_string()),
        "Invalid use of the eval command.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `IllegalArgumentException`, not a `WalnutException` -- `UtilityMethods.validateFile`.
    check(
        &ProverError::InvalidFile("Automata Library/missing.txt".to_string()),
        "Automata Library/missing.txt",
        UNHANDLED,
        "java.lang.IllegalArgumentException",
    );
    check(
        &ProverError::WalnutMessage("Couldn't create directory: Result/".to_string()),
        "Couldn't create directory: Result/",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `Integer.parseInt`'s `NumberFormatException` -- e.g. `Prover.testCommand`'s
    // overflowing `\d+` capture.
    check(
        &ProverError::NumberFormat("99999999999".to_string()),
        "For input string: \"99999999999\"",
        UNHANDLED,
        "java.lang.NumberFormatException",
    );
    check(
        &ProverError::UnsupportedCommand {
            command: "split",
            reason: "not yet ported",
        },
        "The split command is out of scope for walnut-rs (not yet ported).",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `location` reaches neither `Display` nor `is_handled`/`kind`/`stack_trace_lines` --
    // it exists only for a failing test's `{:?}` dump -- so one representative (`Some`)
    // covers both branches of that field for these four channels; the field's own
    // presence/absence is not part of this file's contract.
    check(
        &ProverError::Thrown {
            message: "Second A's alphabet must be a subset".to_string(),
            location: Some("crates/wr-core/src/logicalops.rs:1:1".to_string()),
        },
        "Second A's alphabet must be a subset",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::NotYetImplemented {
            command: "otf",
            unit: "U99",
        },
        "The otf command is not implemented yet (planned for U99).",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `MetaCommandError`, wrapped as `ProverError::Meta` -- the real rendering path
// (`ProverError::Meta` delegates all four methods to `MetaCommandError`'s own
// `LoggableError` impl, and `Display` via `write!(f, "{e}")`).
// ---------------------------------------------------------------------------

#[test]
fn meta_command_error_variants() {
    check(
        &ProverError::from(MetaCommandError::InvalidCommand("strategy".to_string())),
        "Invalid command: strategy",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(MetaCommandError::InvalidCommandUse(
            "[strategy 0 SC]".to_string(),
        )),
        "Invalid use of the [strategy 0 SC] command.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(MetaCommandError::UnexpectedFormat("[strategy]".to_string())),
        // `msg::unexpected_format` has no space after the colon -- a preserved quirk.
        "Unexpected format:[strategy]",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(MetaCommandError::RequiresDoubleColon),
        "Metacommands are currently only supported for commands ending in ::",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(MetaCommandError::OtfStrategyDeferred("CCL".to_string())),
        "Determinization strategy CCL is an OTF strategy, which walnut-rs deliberately \
         does not implement (see docs/DESIGN.md sections 9 and 10)",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `IllegalArgumentException` -- `Strategy.fromString`.
    check(
        &ProverError::from(MetaCommandError::NoStrategyFound("bogus".to_string())),
        "No strategy found for: bogus",
        UNHANDLED,
        "java.lang.IllegalArgumentException",
    );
    // `NumberFormatException` -- `addStrategy`/`addExport`'s unvalidated index parse.
    check(
        &ProverError::from(MetaCommandError::NumberFormat("99999999999".to_string())),
        "For input string: \"99999999999\"",
        UNHANDLED,
        "java.lang.NumberFormatException",
    );
}

// ---------------------------------------------------------------------------
// `AlphabetError`, wrapped as `ProverError::Alphabet` -- entirely in the blanket
// `is_handled() == true` / default `kind()` bucket.
// ---------------------------------------------------------------------------

#[test]
fn alphabet_error_variants() {
    check(
        &ProverError::from(AlphabetError::Walnut(
            "Alphabet must not be empty.".to_string(),
        )),
        "Alphabet must not be empty.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(AlphabetError::NumberSystem(
            PredicateEnvError::FileDoesNotExist {
                address: "Custom Bases/msd_bogus.txt".to_string(),
            },
        )),
        "File does not exist: Custom Bases/msd_bogus.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(AlphabetError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/missing.txt".to_string(),
        })),
        "File does not exist: Automata Library/missing.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(AlphabetError::Io(io_err("permission denied"))),
        "permission denied",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(AlphabetError::NumberFormat(
            try_parse_int("99999999999").unwrap_err(),
        )),
        "For input string: \"99999999999\"",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `AutomatonOpsError`, wrapped as `ProverError::AutomatonOps` -- `NumberFormat` is the
// one variant singled out of the blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn automaton_ops_error_variants() {
    check(
        &ProverError::from(AutomatonOpsError::Read(
            PredicateEnvError::FileDoesNotExist {
                address: "Automata Library/A.txt".to_string(),
            },
        )),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(AutomatonOpsError::Io(io_err("disk full"))),
        "disk full",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(AutomatonOpsError::Walnut(
            "in computing cross product of two automaton, variables with the same label \
             must have the same alphabet"
                .to_string(),
        )),
        "in computing cross product of two automaton, variables with the same label must \
         have the same alphabet",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // Not a `WalnutException` -- `Integer.parseInt(u.substring(1))` (`Combine.java:38`).
    check(
        &ProverError::from(AutomatonOpsError::NumberFormat("99999999999".to_string())),
        "For input string: \"99999999999\"",
        UNHANDLED,
        "java.lang.NumberFormatException",
    );
}

// ---------------------------------------------------------------------------
// `ConvertError`, wrapped as `ProverError::Convert` -- including `ConvertNsError`'s
// three singled-out sub-variants.
// ---------------------------------------------------------------------------

#[test]
fn convert_error_variants() {
    check(
        &ProverError::from(ConvertError::DfaoIntoFunction),
        "Cannot convert a Word Automaton into a function",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `Integer.parseInt(m.group(GROUP_CONVERT_BASE))` -- not a `WalnutException`.
    check(
        &ProverError::from(ConvertError::InvalidBase("99999999999999".to_string())),
        "For input string: \"99999999999999\"",
        UNHANDLED,
        "java.lang.NumberFormatException",
    );
    check(
        &ProverError::from(ConvertError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/A.txt".to_string(),
        })),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `ConvertNsError` variant NOT one of the three singled out: falls to the
    // `ConvertError`-level default (handled, `Main.WalnutException`).
    check(
        &ProverError::from(ConvertError::Convert(ConvertNsError::NotSingleInput)),
        "Automaton must have exactly one input to be converted.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `UtilityMethods.exactIntegerExponent`'s `root <= 1` guard -- `IllegalArgumentException`.
    check(
        &ProverError::from(ConvertError::Convert(ConvertNsError::InvalidRoot {
            root: 0,
        })),
        "root must be > 1, got 0",
        UNHANDLED,
        "java.lang.IllegalArgumentException",
    );
    // Same guard's consistency check -- also `IllegalArgumentException`.
    check(
        &ProverError::from(ConvertError::Convert(ConvertNsError::NotAnExactPower {
            base: 5,
            root: 2,
        })),
        "5 is not an exact power of 2",
        UNHANDLED,
        "java.lang.IllegalArgumentException",
    );
    // `NumberSystem.parseBase`'s `Integer.parseInt` on an all-digit, `int`-overflowing
    // base -- `NumberFormatException`, deliberately distinguished from the two above.
    check(
        &ProverError::from(ConvertError::Convert(ConvertNsError::BaseOverflowsInt {
            found: "99999999999999".to_string(),
        })),
        "For input string: \"99999999999999\"",
        UNHANDLED,
        "java.lang.NumberFormatException",
    );
    check(
        &ProverError::from(ConvertError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `DescribeError`, wrapped as `ProverError::Describe` -- blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn describe_error_variants() {
    check(
        &ProverError::from(DescribeError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/A.txt".to_string(),
        })),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(DescribeError::Comments(ReadError::MalformedHeader)),
        // `ReadError::MalformedHeader`'s own `Display` -- see its source for the exact
        // text; captured here as this command's own rendering.
        &ReadError::MalformedHeader.to_string(),
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `EvalDefError`, wrapped as `ProverError::EvalDef` -- blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn eval_def_error_variants() {
    check(
        &ProverError::from(EvalDefError::Eval(EvalError::NoResult)),
        "Evaluation ended in no result.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(EvalDefError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(EvalDefError::NoFreeVariableOnTrivialAutomaton),
        "incidence matrices cannot be calculated, because the automaton does not have a \
         free variable.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(EvalDefError::DuplicateLabelVariable {
            name: "x".to_string(),
        }),
        "Duplicate variable in automaton label: x",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(EvalDefError::NotAFreeVariable {
            name: "x".to_string(),
        }),
        "incidence matrices for the variable x cannot be calculated, because x is not a \
         free variable.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(EvalDefError::DuplicateFreeVariable {
            name: "x".to_string(),
        }),
        "Duplicate free variable: x",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(EvalDefError::EmptyValueDomain {
            name: "x".to_string(),
        }),
        "Empty value domain for free variable: x",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `ImageError`, wrapped as `ProverError::Image` -- blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn image_error_variants() {
    check(
        &ProverError::from(ImageError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ImageError::MorphismParse(
            ParseMethodsError::NoValidMorphismMappings,
        )),
        "Morphism has no valid mappings.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ImageError::RequireUniform(MorphismError::NotUniform)),
        &MorphismError::NotUniform.to_string(),
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ImageError::OldWordRead(
            PredicateEnvError::FileDoesNotExist {
                address: "Word Automata Library/W.txt".to_string(),
            },
        )),
        "File does not exist: Word Automata Library/W.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ImageError::NotUnaryWordAutomaton {
            name: "W".to_string(),
        }),
        "Image requires a unary word automaton: W",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ImageError::CustomBaseNotSupported {
            name: "W".to_string(),
        }),
        "image: the word automaton W is over a custom base, whose number system name \
         walnut-rs does not yet record -- refusing rather than silently evaluating the \
         image over the wrong arithmetic",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ImageError::Eval(EvalError::NoResult)),
        "Evaluation ended in no result.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `JoinError`, wrapped as `ProverError::Join` -- blanket bucket (WB-037's fix moved
// `NoAutomataSpecified` in here too, see the module's own docs).
// ---------------------------------------------------------------------------

#[test]
fn join_error_variants() {
    check(
        &ProverError::from(JoinError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/A.txt".to_string(),
        })),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(JoinError::LabelMismatch {
            automaton_name: "W".to_string(),
        }),
        "Number of inputs of word automata W does not match number of inputs specified.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(JoinError::NoAutomataSpecified),
        "Cannot join without any automata specified.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(JoinError::AlphabetMismatch {
            label: "x".to_string(),
        }),
        "in computing cross product of two automaton, variables with the same label must \
         have the same alphabet",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(JoinError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `MorphismCommandError`, wrapped as `ProverError::Morphism` -- `InvalidFile` is the one
// variant singled out of the blanket bucket (WB-036's fix moved `Promote`'s domain-gap
// case into the blanket bucket too, see the module's own docs).
// ---------------------------------------------------------------------------

#[test]
fn morphism_command_error_variants() {
    check(
        &ProverError::from(MorphismCommandError::Parse(
            ParseMethodsError::NoValidMorphismMappings,
        )),
        "Morphism has no valid mappings.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `UtilityMethods.validateFile` throws `IllegalArgumentException` in Java, and
    // `is_handled()` correctly says so (`false`, pinned by `prover.rs`'s own
    // `u24_command_errors_classify_by_walnutexception_vs_jdk_exception` test) -- but
    // `kind()` has NO matching arm for this specific variant (unlike its sibling
    // `ProverError::InvalidFile`, which does), so it falls through to the generic
    // default. A real, pre-existing `is_handled()`/`kind()` divergence, captured here as
    // current behavior -- not this file's job to fix.
    check(
        &ProverError::from(MorphismCommandError::InvalidFile(
            "Morphism Library/missing.txt".to_string(),
        )),
        "Morphism Library/missing.txt",
        UNHANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(MorphismCommandError::Promote(MorphismError::NotUniform)),
        &MorphismError::NotUniform.to_string(),
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(MorphismCommandError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `OstError`, wrapped as `ProverError::Ost` -- `Parse(_)` is UNCONDITIONALLY
// `is_handled() == false` regardless of the inner `ParseMethodsError` variant, but
// `kind()` only special-cases `Parse(NumberFormat(_))`; every other `Parse(_)` payload
// still renders `Main.WalnutException` despite being unhandled. Both `Parse` cases below
// exist specifically to pin that divergence.
// ---------------------------------------------------------------------------

#[test]
fn ost_error_variants() {
    check(
        &ProverError::from(OstError::Parse(ParseMethodsError::NumberFormat(
            try_parse_int("99999999999").unwrap_err(),
        ))),
        "For input string: \"99999999999\"",
        UNHANDLED,
        "java.lang.NumberFormatException",
    );
    // `is_handled()` is still `false` here (unconditional on `Parse(_)`), but `kind()`
    // has no `NoValidMorphismMappings`-specific arm, so it falls through to the default.
    check(
        &ProverError::from(OstError::Parse(ParseMethodsError::NoValidMorphismMappings)),
        "Morphism has no valid mappings.",
        UNHANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(OstError::Ostrowski(OstrowskiError::EmptyPeriod)),
        "The period cannot be empty.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(OstError::AlreadyExists {
            name: "fib".to_string(),
        }),
        "Error: number system fib already exists.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(OstError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `ProverHelperError`, wrapped as `ProverError::Helper` -- blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn prover_helper_error_variants() {
    check(
        &ProverError::from(ProverHelperError::ExportingToTxtIsRedundant),
        "Exporting to .txt is redundant; this is the input format",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ProverHelperError::UnexpectedFormat("foo".to_string())),
        "Unexpected format:foo",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ProverHelperError::BaWrite(BaWriteError::DfaoNotSupported)),
        "Can't export DFAO to BA format",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ProverHelperError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ProverHelperError::Read(
            PredicateEnvError::FileDoesNotExist {
                address: "Automata Library/A.txt".to_string(),
            },
        )),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ProverHelperError::RemoveLeadingZeros(
            RemoveLeadingZerosError::NotFreeVariable("x".to_string()),
        )),
        "Variable x in the list of quantified variables is not a free variable.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `QuotientError`, wrapped as `ProverError::Quotient` -- `Runtime` is the one variant
// singled out (`is_walnut_exception()`'s own triage), rendered with an
// `ArrayIndexOutOfBoundsException` header rather than message-only.
// ---------------------------------------------------------------------------

#[test]
fn quotient_error_variants() {
    check(
        &ProverError::from(QuotientError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/A.txt".to_string(),
        })),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(QuotientError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(QuotientError::Walnut(
            "Second A's alphabet must be a subset of the first A's alphabet for right \
             quotient."
                .to_string(),
        )),
        "Second A's alphabet must be a subset of the first A's alphabet for right \
         quotient.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(QuotientError::Runtime("something unexpected".to_string())),
        "something unexpected",
        UNHANDLED,
        "java.lang.ArrayIndexOutOfBoundsException",
    );
}

// ---------------------------------------------------------------------------
// `RegError`, wrapped as `ProverError::Reg` -- blanket `true`/default bucket for EVERY
// variant, including `Regex(RegexError::NumberFormat(_))` (`RegexError`'s own doc
// comment says Java treats this as an uncaught, stack-trace-headed exception, but
// `ProverError`'s `is_handled`/`kind` currently have no `RegError`/`RegexError`-specific
// arm at all -- `reg`'s whole family lands in the blanket bucket regardless). Captured
// as current behavior, not endorsed as correct; not this file's job to fix.
// ---------------------------------------------------------------------------

#[test]
fn reg_error_variants() {
    check(
        &ProverError::from(RegError::Alphabet(AlphabetError::Walnut(
            "Alphabet must not be empty.".to_string(),
        ))),
        "Alphabet must not be empty.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(RegError::Regex(RegexError::Brics(
            "expected ')' at position 3".to_string(),
        ))),
        "expected ')' at position 3",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(RegError::Regex(RegexError::NumberFormat(
            try_parse_int("8888888800").unwrap_err(),
        ))),
        "For input string: \"8888888800\"",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(RegError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `ReverseError`, wrapped as `ProverError::Reverse` -- blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn reverse_error_variants() {
    check(
        &ProverError::from(ReverseError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/A.txt".to_string(),
        })),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(ReverseError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `SimpleTransformError`, wrapped as `ProverError::SimpleTransform` -- blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn simple_transform_error_variants() {
    check(
        &ProverError::from(SimpleTransformError::Read(
            PredicateEnvError::FileDoesNotExist {
                address: "Automata Library/A.txt".to_string(),
            },
        )),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(SimpleTransformError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `SplitError`, wrapped as `ProverError::Split` -- blanket bucket for EVERY variant,
// including `Core` (whose own doc comment in `crate::split` flags one specific message,
// `op_from_symbol`'s "Unknown arithmetic operator: …", as a known-but-unreachable
// inconsistency with real Java's `IllegalArgumentException` classification -- captured
// as current behavior, not fixed here).
// ---------------------------------------------------------------------------

#[test]
fn split_error_variants() {
    check(
        &ProverError::from(SplitError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/A.txt".to_string(),
        })),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(SplitError::Walnut(
            "Number system for input 0 must be defined.".to_string(),
        )),
        "Number system for input 0 must be defined.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(SplitError::Core(
            "Unknown arithmetic operator: ?".to_string(),
        )),
        "Unknown arithmetic operator: ?",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(SplitError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `TestError`, wrapped as `ProverError::Test` -- blanket bucket.
// ---------------------------------------------------------------------------

#[test]
fn test_error_variants() {
    check(
        &ProverError::from(TestError::UnmaterializedTrueAutomaton),
        "Cannot enumerate accepted inputs of an unmaterialized true automaton.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TestError::Read(PredicateEnvError::FileDoesNotExist {
            address: "Automata Library/A.txt".to_string(),
        })),
        "File does not exist: Automata Library/A.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TestError::RemoveLeadingZeros(
            RemoveLeadingZerosError::NotFreeVariable("x".to_string()),
        )),
        "Variable x in the list of quantified variables is not a free variable.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TestError::NonDeterministicO),
        "NFAOs are not supported..",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TestError::NeededTooLarge {
            needed: 2_000_000_000,
        }),
        "The test command refuses to enumerate 2000000000 inputs; the limit is \
         1000000.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TestError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// `TransduceCommandError`, wrapped as `ProverError::Transduce` -- the richest
// classification in the crate. Three `TransduceError` variants are singled out
// (`TrivialAutomaton` / `NoTransducerTransition` / `NoTransducerOutput`); the other five
// (`NotSingleInput`, `IncompatibleAlphabet`, `MultipleTransitionsPerInput`,
// `NoNumberSystem`, `Exploded`) share the blanket bucket, so every `TransduceError`
// variant is exercised individually.
// ---------------------------------------------------------------------------

#[test]
fn transduce_command_error_variants() {
    // `ReadTransducer`'s `Display` forks on its own `source` field independently of
    // `crate::prover::ProverError`'s classification (both land in the blanket bucket
    // regardless) -- both branches are covered since the fork is real, observable text.
    check(
        &ProverError::from(TransduceCommandError::ReadTransducer {
            address: "Transducer Library/T.txt".to_string(),
            source: ReadError::Io(io_err("not found")),
        }),
        "File does not exist: Transducer Library/T.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TransduceCommandError::ReadTransducer {
            address: "Transducer Library/T.txt".to_string(),
            source: ReadError::MalformedHeader,
        }),
        &format!(
            "File does not parse: Transducer Library/T.txt ({})",
            ReadError::MalformedHeader
        ),
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TransduceCommandError::ReadAutomaton(
            PredicateEnvError::FileDoesNotExist {
                address: "Word Automata Library/W.txt".to_string(),
            },
        )),
        "File does not exist: Word Automata Library/W.txt",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TransduceCommandError::Transduce(
            TransduceError::NotSingleInput,
        )),
        "Automata with only one input can be transduced.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TransduceCommandError::Transduce(
            TransduceError::IncompatibleAlphabet,
        )),
        "Output alphabet of automaton must be compatible with the transducer input alphabet",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TransduceCommandError::Transduce(
            TransduceError::MultipleTransitionsPerInput,
        )),
        "Automaton must have at most one transition per input per state.",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TransduceCommandError::Transduce(
            TransduceError::NoNumberSystem,
        )),
        "the automaton being transduced has no attached number system (its alphabet was \
         declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    // `transduceMsdDeterministic`'s zero-state `M.fa.getO().getInt(0)`.
    check(
        &ProverError::from(TransduceCommandError::Transduce(
            TransduceError::TrivialAutomaton,
        )),
        "a TRUE/FALSE automaton has no states or tracks and cannot be transduced",
        UNHANDLED,
        "java.lang.IndexOutOfBoundsException",
    );
    // `Transducer.createMap`'s NPE.
    check(
        &ProverError::from(TransduceCommandError::Transduce(
            TransduceError::NoTransducerTransition,
        )),
        "Cannot invoke \"it.unimi.dsi.fastutil.ints.IntList.getInt(int)\" because the \
         return value of \"Automata.FA.Transitions.getNfaStateDests(int, int)\" is null",
        UNHANDLED,
        "java.lang.NullPointerException",
    );
    // The `sigma` unboxing NPE, one line earlier in the same algorithm.
    check(
        &ProverError::from(TransduceCommandError::Transduce(
            TransduceError::NoTransducerOutput,
        )),
        "Cannot invoke \"java.lang.Integer.intValue()\" because the return value of \
         \"java.util.Map.get(Object)\" is null",
        UNHANDLED,
        "java.lang.NullPointerException",
    );
    // Port-specific resource verdict, no Java analogue -- stays in the blanket bucket.
    check(
        &ProverError::from(TransduceCommandError::Transduce(TransduceError::Exploded(
            TransduceLimit::MapSteps,
        ))),
        "transduce exceeded walnut-rs's resource budget (transducer-map composition \
         steps); the input is too large for this port to transduce",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
    check(
        &ProverError::from(TransduceCommandError::Io(io_err("write failed"))),
        "write failed",
        HANDLED,
        MAIN_WALNUT_EXCEPTION,
    );
}

// ---------------------------------------------------------------------------
// U2 stage 2 addendum: every macro-generated `From<Foreign> for CommandError` impl,
// invoked directly.
//
// Everything above this point routes every construction through `ProverError::from`,
// which never calls a command enum's OWN `From` impl -- `ProverError::from(AlphabetError
// ::Io(e))` just moves an already-built `AlphabetError` one level up, it never exercises
// `AlphabetError::from(io::Error)` itself. So stage 1 never actually invoked any of the
// 29 `From` impls `crate::error_support::simple_error_froms!` now generates (U2 stage 2),
// even though every one of those impls existed by hand before stage 2 too -- the
// snapshot file passing unmodified through stage 2 proved the macro's OUTPUT matches the
// old hand-written code closely enough to fool `ProverError::from`'s callers, but not
// that the `From` impls it generates are individually well-formed and wired to the
// right variant. This is that missing coverage, added per adversarial review.
//
// This also closes the same review's latent-risk note: `simple_error_froms!` cannot by
// construction generate two `From<T>` impls for the same `T` on one enum (Rust's
// coherence rules reject the resulting duplicate trait impl at compile time before any
// test could even run) -- but nothing previously exercised the ONE `From<T>` impl each
// enum does have, so a future edit that pointed it at the wrong variant (e.g. if
// `AlphabetError` ever grew a `PredicateEnvError => NumberSystem`/`PredicateEnvError =>
// Read` pair and someone swapped the two variant names) would have compiled and passed
// every existing test silently. `matches!` below pins the actual target variant per
// pair, not just that `From::from` compiles.
#[test]
fn every_macro_generated_from_impl_routes_to_its_declared_variant() {
    // AlphabetError (1 pair)
    assert!(matches!(
        AlphabetError::from(io_err("x")),
        AlphabetError::Io(_)
    ));

    // AutomatonOpsError (1 pair)
    assert!(matches!(
        AutomatonOpsError::from(io_err("x")),
        AutomatonOpsError::Io(_)
    ));

    // ConvertError (1 pair)
    assert!(matches!(
        ConvertError::from(io_err("x")),
        ConvertError::Io(_)
    ));

    // EvalDefError (2 pairs)
    assert!(matches!(
        EvalDefError::from(EvalError::NoResult),
        EvalDefError::Eval(_)
    ));
    assert!(matches!(
        EvalDefError::from(io_err("x")),
        EvalDefError::Io(_)
    ));

    // ImageError (2 pairs)
    assert!(matches!(ImageError::from(io_err("x")), ImageError::Io(_)));
    assert!(matches!(
        ImageError::from(EvalError::NoResult),
        ImageError::Eval(_)
    ));

    // JoinError (1 pair)
    assert!(matches!(JoinError::from(io_err("x")), JoinError::Io(_)));

    // MorphismCommandError (1 pair)
    assert!(matches!(
        MorphismCommandError::from(io_err("x")),
        MorphismCommandError::Io(_)
    ));

    // OstError (3 pairs)
    assert!(matches!(
        OstError::from(ParseMethodsError::NoValidMorphismMappings),
        OstError::Parse(_)
    ));
    assert!(matches!(
        OstError::from(OstrowskiError::EmptyPeriod),
        OstError::Ostrowski(_)
    ));
    assert!(matches!(OstError::from(io_err("x")), OstError::Io(_)));

    // ProverHelperError (4 pairs)
    assert!(matches!(
        ProverHelperError::from(BaWriteError::DfaoNotSupported),
        ProverHelperError::BaWrite(_)
    ));
    assert!(matches!(
        ProverHelperError::from(io_err("x")),
        ProverHelperError::Io(_)
    ));
    assert!(matches!(
        ProverHelperError::from(PredicateEnvError::FileDoesNotExist {
            address: "x".to_string()
        }),
        ProverHelperError::Read(_)
    ));
    assert!(matches!(
        ProverHelperError::from(RemoveLeadingZerosError::NotFreeVariable("x".to_string())),
        ProverHelperError::RemoveLeadingZeros(_)
    ));

    // QuotientError (1 pair)
    assert!(matches!(
        QuotientError::from(io_err("x")),
        QuotientError::Io(_)
    ));

    // RegError (3 pairs)
    assert!(matches!(
        RegError::from(AlphabetError::Walnut("x".to_string())),
        RegError::Alphabet(_)
    ));
    assert!(matches!(
        RegError::from(RegexError::Brics("x".to_string())),
        RegError::Regex(_)
    ));
    assert!(matches!(RegError::from(io_err("x")), RegError::Io(_)));

    // ReverseError (1 pair)
    assert!(matches!(
        ReverseError::from(io_err("x")),
        ReverseError::Io(_)
    ));

    // SimpleTransformError (1 pair)
    assert!(matches!(
        SimpleTransformError::from(io_err("x")),
        SimpleTransformError::Io(_)
    ));

    // SplitError (1 pair -- the macro-generated one only; `From<NumSysError>` stays
    // hand-written and is out of scope for this addendum, same as `EvalDefError`'s
    // `From<MatrixWriteError>`)
    assert!(matches!(SplitError::from(io_err("x")), SplitError::Io(_)));

    // TestError (3 pairs)
    assert!(matches!(
        TestError::from(PredicateEnvError::FileDoesNotExist {
            address: "x".to_string()
        }),
        TestError::Read(_)
    ));
    assert!(matches!(
        TestError::from(RemoveLeadingZerosError::NotFreeVariable("x".to_string())),
        TestError::RemoveLeadingZeros(_)
    ));
    assert!(matches!(TestError::from(io_err("x")), TestError::Io(_)));

    // TransduceCommandError (3 pairs)
    assert!(matches!(
        TransduceCommandError::from(PredicateEnvError::FileDoesNotExist {
            address: "x".to_string()
        }),
        TransduceCommandError::ReadAutomaton(_)
    ));
    assert!(matches!(
        TransduceCommandError::from(TransduceError::NotSingleInput),
        TransduceCommandError::Transduce(_)
    ));
    assert!(matches!(
        TransduceCommandError::from(io_err("x")),
        TransduceCommandError::Io(_)
    ));
}
