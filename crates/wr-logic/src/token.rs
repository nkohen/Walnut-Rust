// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Token/Token.java` (71 LOC) + `Token/Operator.java` (159 LOC) + the parenthesis/
//! leaf-token subclasses (`LeftParenthesis`, `RightParenthesis`, `Variable`,
//! `NumberLiteral`, `AlphabetLetter`, 205 LOC combined) + the bare symbol tables of
//! `LogicalOperator`/`RelationalOperator`/`ArithmeticOperator` — Phase 3a's U2.
//!
//! # Why one `Token` enum, not eight Java classes
//!
//! Same rationale as [`crate::expr`]'s module docs (read those first — this file
//! repeats the pattern rather than re-deriving it): Java's `Token` hierarchy is
//! `Token` (abstract) -> `Operator` (abstract) -> `{LeftParenthesis, RightParenthesis,
//! LogicalOperator, RelationalOperator, ArithmeticOperator}`, plus **five** more direct
//! `Token` subclasses: `Variable`, `NumberLiteral`, `AlphabetLetter` — ported below — and
//! `Word`/`Function`, which are **not** ported in this unit (deferred to U4; see
//! `crate::expr`'s module docs for the corresponding deferral of the `act(...)` overloads
//! `Word`/`Function` call). An earlier draft of this doc omitted `Word`/`Function`
//! entirely, undercounting the hierarchy at 3 leaf subclasses instead of 5 — corrected
//! during Phase 3a U2's adversarial review. This matters beyond bookkeeping: `Word`'s and
//! `Function`'s constructors are `Token.validateArity`'s and `Token::arity`'s only real
//! non-zero-arity, non-`Operator` callers/setters in Java (`Word.java:38-44`,
//! `Function.java`'s constructor) — see [`Token::act`]'s and [`Token::arity`]'s own docs
//! for exactly what that means for this unit's scope. Every place that branches on
//! *which* concrete class a `Token`/`Operator` is (the tokenizer's `instanceof Operator`
//! dispatch; `Operator.put`'s "is this a left-paren/quantifier" check;
//! `Operator.setPriority`'s symbol switch) narrows to a closed, known set — there is no
//! plugin point, no third-party subclass. [`Token`] and [`OperatorKind`] below are that
//! closed set as Rust `enum`s, restricted to the leaf kinds this unit actually ports.
//!
//! # Scope boundary: symbol tables, not `act()` semantics
//!
//! Per this unit's brief, `LogicalOperator`/`RelationalOperator`/`ArithmeticOperator`
//! are ported ONLY as far as `Operator::set_priority`'s dispatch needs them — the
//! `Ops` enum / symbol-table declarations, reused directly from
//! [`wr_core::numsys::RelationalOp`]/[`wr_core::numsys::ArithmeticOp`] rather than
//! duplicated (see [`relational_op_from_symbol`]/[`arithmetic_op_from_symbol`]'s docs
//! for why reusing those types is correct here, not merely convenient). Each
//! operator's real `act()` behavior — the automaton-building logic in
//! `LogicalOperator.act`/`RelationalOperator.act`/`ArithmeticOperator.act` — is
//! deliberately NOT ported here; it lands in U10 (`LogicalOperator`) and U9
//! (`RelationalOperator`/`ArithmeticOperator`). [`Token::act`] therefore has no arm for
//! [`Token::Operator`] beyond the inherited no-op default, exactly mirroring Java:
//! `Token.act(Stack<Expression>)` is `{}` by default, and `Operator` itself never
//! overrides it (only its `LogicalOperator`/`RelationalOperator`/`ArithmeticOperator`
//! subclasses do, and none of those are ported yet). [`Operator`] is designed so U9/U10
//! can add real behavior — most naturally as a `match &self.kind` inside a new
//! `Operator::act` — without restructuring anything here: [`OperatorKind::Relational`]/
//! [`OperatorKind::Arithmetic`] already carry the right operator enum, and
//! [`Operator::ns`] already carries the `NumberSystem` handle both need.
//!
//! Two small `Operator.java` pieces are ALSO deliberately deferred alongside those,
//! for the same reason (they exist purely to support `act()`, and porting them here
//! would be untestable dead code): `Operator.andThenQuantifyIfArithmetic` (needs
//! `Logging::indent/dedent`, itself deferred per Ruling 4 — see [`crate::expr`]'s
//! module docs) is documented there, not here, since it is a free function rather than
//! a `Token`/`Operator` method. `Operator.validateArity(Stack<Expression>)` — the
//! two-argument overload Java's `Operator` itself declares — IS ported below
//! ([`Operator::validate_arity`]), since unlike the other two it needs nothing this
//! unit doesn't already have.
//!
//! # Fresh identifiers: not needed by anything ported here
//!
//! Unlike [`crate::expr`]'s `VariableExpression`/`NumberLiteralExpression::act`,
//! nothing in this file calls `Token.getUniqueString()` in Java — that only happens
//! inside the deferred `RelationalOperator`/`ArithmeticOperator::act` bodies (via
//! `Operator.andThenQuantifyIfArithmetic` and `ArithmeticOperator`'s own unary-negate
//! path) and `Expression`'s own `act()` methods (already ported using
//! [`crate::predicate_env::FreshIdentifiers`]). So no [`crate::predicate_env`] type
//! appears in this file's public API at all — U9/U10 will need to add it when they add
//! `Operator::act`, and nothing here forecloses that (a `&mut FreshIdentifiers`
//! parameter added to a new method costs nothing today).

use std::collections::BTreeSet;
use std::fmt;
use std::rc::Rc;

use num_bigint::BigInt;
use wr_core::automaton::Automaton;
use wr_core::logicalops::{and, imply};
use wr_core::numsys::{ArithmeticOp, NumSysError, NumberSystem, RelationalOp};
use wr_core::quantify::{quantify, QuantifyError};
use wr_core::word_automaton::{
    apply_word_arith_operator, apply_word_operator, compare_word_automata, compare_word_automaton,
};

use crate::expr::{
    AlphabetLetterExpression, ArithmeticExpression, AutomatonExpression, ExprError, Expression,
    NumberLiteralExpression, VariableExpression, WordExpression,
};
use crate::predicate_env::FreshIdentifiers;

// ---------------------------------------------------------------------------
// Operator symbol constants — `Operator`/`LogicalOperator`'s `public static final`
// fields (`Operator.java:34-38`, `LogicalOperator.java:38-42`).
// ---------------------------------------------------------------------------

pub mod symbols {
    /// `Operator.REVERSE` (`` ` ``).
    pub const REVERSE: &str = "`";
    /// `Operator.EXISTS`.
    pub const EXISTS: &str = "E";
    /// `Operator.FORALL`.
    pub const FORALL: &str = "A";
    /// `Operator.INFINITE`.
    pub const INFINITE: &str = "I";
    /// `Operator.NEGATE` — the ASCII tilde spelling. [`super::is_negation`] additionally
    /// recognizes ˜ (U+02DC) and ◌̃ (U+0303); see its docs.
    pub const NEGATE: &str = "~";
    /// `LogicalOperator.AND`.
    pub const AND: &str = "&";
    /// `LogicalOperator.OR`.
    pub const OR: &str = "|";
    /// `LogicalOperator.XOR`.
    pub const XOR: &str = "^";
    /// `LogicalOperator.IMPLY`.
    pub const IMPLY: &str = "=>";
    /// `LogicalOperator.IFF`.
    pub const IFF: &str = "<=>";
    /// `LeftParenthesis.LEFT_PAREN`.
    pub const LEFT_PAREN: &str = "(";
}

/// `Operator.isNegation(String)` (`Operator.java:134-144`). To allow for multiple
/// spellings of "tilde" (`~`, ˜ U+02DC, the combining ◌̃ U+0303), this must be used
/// instead of a plain string comparison against [`symbols::NEGATE`] — ported verbatim,
/// including the Java-`char`-based (i.e. UTF-16-code-unit-based) `length() == 1` check:
/// all three spellings are single UTF-16 code units (all in the BMP), so checking "is
/// this exactly one Rust `char`" is the faithful equivalent, not merely close enough.
pub fn is_negation(op: &str) -> bool {
    let mut chars = op.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        // Java: `Integer.toHexString(op.charAt(0))`, compared against "2dc"/"303".
        if matches!(c as u32, 0x2dc | 0x303) {
            return true;
        }
    }
    op == symbols::NEGATE
}

// ---------------------------------------------------------------------------
// RelationalOperator / ArithmeticOperator symbol tables
// ---------------------------------------------------------------------------
//
// `wr_core::numsys::RelationalOp`/`ArithmeticOp` already exist (added in Phase 2's
// U7), but that module's own docs are explicit that they are "a local enum, not a
// port of that Token class" (`numsys.rs`, `RelationalOp`'s doc comment) — built for
// `NumberSystem::comparison`/`arithmetic`'s internal dispatch, with no symbol<->enum
// mapping of their own (only `ArithmeticOp::symbol()`, no `from_symbol` on either).
// This unit is the actual `Token`-layer port `RelationalOperator.Ops`/
// `ArithmeticOperator.Ops` correspond to, so it reuses those enums directly — a new,
// isomorphic-but-distinct pair of enums here would just be a second copy of the same
// six/five-way classification with a conversion layer U9 would have to write and
// maintain, for no benefit (nothing downstream needs `RelationalOperator`'s `Ops` to
// be a DIFFERENT type from `NumberSystem::comparison`'s parameter — they were always
// meant to be the same six relations). This module only adds what's missing: the
// symbol<->enum mapping (`RelationalOperator.RELATIONAL_OPERATORS`/`Ops.fromSymbol`/
// `getSymbol`, `ArithmeticOperator.ARITHMETIC_OPERATORS`/`Ops.fromSymbol`).

/// `RelationalOperator.Ops.getSymbol()` (`RelationalOperator.java:53-61`).
pub fn relational_op_symbol(op: RelationalOp) -> &'static str {
    match op {
        RelationalOp::Equal => "=",
        RelationalOp::NotEqual => "!=",
        RelationalOp::LessThan => "<",
        RelationalOp::GreaterThan => ">",
        RelationalOp::LessEqThan => "<=",
        RelationalOp::GreaterEqThan => ">=",
    }
}

/// `RelationalOperator.Ops.fromSymbol(String)` (`:63-70`). Java throws
/// `IllegalArgumentException("Unknown comparison operator: " + symbol)` — an unchecked
/// exception around an internal invariant (every real call site passes text a
/// `PATTERN_FOR_RELATIONAL_OPERATORS` match already guarantees is one of these six
/// symbols; U3's lexer is the only caller `Operator::relational` will ever have).
/// Ported as a panic with matching text, per this crate's established idiom for that
/// class of Java exception (`predicate_env.rs`, `wr_core::numsys`'s `unexpectedOperator`
/// handling) rather than threading a `Result` every caller would have to unwrap anyway.
pub fn relational_op_from_symbol(symbol: &str) -> RelationalOp {
    match symbol {
        "=" => RelationalOp::Equal,
        "!=" => RelationalOp::NotEqual,
        "<" => RelationalOp::LessThan,
        ">" => RelationalOp::GreaterThan,
        "<=" => RelationalOp::LessEqThan,
        ">=" => RelationalOp::GreaterEqThan,
        _ => panic!("Unknown comparison operator: {symbol}"),
    }
}

/// `ArithmeticOperator.Ops.fromSymbol(String)` (`ArithmeticOperator.java:64-71`). See
/// [`relational_op_from_symbol`]'s docs for why this panics rather than returning a
/// `Result`. [`ArithmeticOp::symbol`] already exists (`wr_core::numsys`) and is reused
/// as-is; only the reverse direction (symbol -> enum) is missing there.
pub fn arithmetic_op_from_symbol(symbol: &str) -> ArithmeticOp {
    match symbol {
        "+" => ArithmeticOp::Plus,
        "-" => ArithmeticOp::Minus,
        "/" => ArithmeticOp::Div,
        "*" => ArithmeticOp::Mult,
        "_" => ArithmeticOp::UnaryNegative,
        _ => panic!("Unknown arithmetic operator: {symbol}"),
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure a [`Token`]'s own methods can report, module-local per this crate's
/// established idiom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// `WalnutException.unbalancedParen(int)` (`WalnutException.java:120-122`), thrown
    /// from `RightParenthesis.put` (`:41`) when the operator stack is exhausted before
    /// a matching left parenthesis is found.
    UnbalancedParenthesis { position: usize },
    /// `Token.validateArity(Stack<Expression>, String, String)` (`Token.java:56-58`):
    /// `name1 + this + " requires " + arity + name2`. Stored as components (not a
    /// pre-formatted `String`) so [`fmt::Display`] can reproduce the exact concatenation
    /// without this crate losing the pieces a test might want to check individually.
    InsufficientStackOperands {
        name1: String,
        token_display: String,
        arity: usize,
        name2: String,
    },
    /// `Token.validateArity(String, int)` (`:60-63`): `"function " + name + " requires
    /// " + otherArity + " arguments: char at " + positionInPredicate`.
    WrongArgumentArity {
        name: String,
        expected_arity: usize,
        position: usize,
    },
    /// `Operator.validateArity(Stack<Expression>)` (`Operator.java:146-148`):
    /// `"operator " + op + " requires " + arity + " operands"`.
    InsufficientOperands { op: String, arity: usize },
    /// `WalnutException.invalidOperator(String, Expression)` (`WalnutException.java:76-78`):
    /// `"operator " + op + " cannot be applied to the operand " + a + " of type " +
    /// a.getClass().getName()`. Thrown by `ArithmeticOperator.act`/`processBinaryOperator`
    /// when an operand is not one of the five kinds `isValidArithmeticOperator` accepts
    /// (i.e. it is an [`Expression::Automaton`]) — Phase 3a's U9.
    InvalidOperator {
        op: String,
        operand: String,
        operand_type: &'static str,
    },
    /// `WalnutException.invalidDualOperators(String, Expression, Expression)`
    /// (`:80-82`): `"operator " + op + " cannot be applied to operands " + a + " and " +
    /// b + " of types " + a.getClass().getName() + " and " + b.getClass().getName() +
    /// " respectively"`. `RelationalOperator.act`'s final `else`
    /// (`RelationalOperator.java:172-174`) — Phase 3a's U9.
    InvalidDualOperators {
        op: String,
        a: String,
        b: String,
        a_type: &'static str,
        b_type: &'static str,
    },
    /// Propagated out of [`NumberLiteralExpression::int_value_exact`] — the
    /// `WalnutException` `RelationalOperator.getIntConstantForWord` /
    /// `ArithmeticOperator.getIntConstantForWord` let escape when a number literal used
    /// against a word automaton's per-state OUTPUT doesn't fit a Java `int`
    /// (`RelationalOperator.java:195-200`, `ArithmeticOperator.java:260-265`). Carries the
    /// already-formatted message (that helper builds the whole string, including the
    /// caller's context prefix) rather than its components — see
    /// [`NumberLiteralExpression::int_value_exact`]'s own docs on why the context string
    /// is the caller's to supply.
    NumberLiteralOverflow(String),
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenError::UnbalancedParenthesis { position } => {
                write!(f, "unbalanced parenthesis: char at {position}")
            }
            TokenError::InsufficientStackOperands {
                name1,
                token_display,
                arity,
                name2,
            } => write!(f, "{name1}{token_display} requires {arity}{name2}"),
            TokenError::WrongArgumentArity {
                name,
                expected_arity,
                position,
            } => write!(
                f,
                "function {name} requires {expected_arity} arguments: char at {position}"
            ),
            TokenError::InsufficientOperands { op, arity } => {
                write!(f, "operator {op} requires {arity} operands")
            }
            TokenError::InvalidOperator {
                op,
                operand,
                operand_type,
            } => write!(
                f,
                "operator {op} cannot be applied to the operand {operand} of type {operand_type}"
            ),
            TokenError::InvalidDualOperators {
                op,
                a,
                b,
                a_type,
                b_type,
            } => write!(
                f,
                "operator {op} cannot be applied to operands {a} and {b} of types {a_type} \
                 and {b_type} respectively"
            ),
            TokenError::NumberLiteralOverflow(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TokenError {}

/// Every failure [`Token::act`] can report — U4 upgrades `act()` from an infallible
/// method (U2's Variable/NumberLiteral/AlphabetLetter never fail) to a fallible one,
/// because `Word.act`/`Function.act` (`Word.java:50-79`, `Function.java`'s own `act`)
/// each open with `Token.validateArity(Stack, ...)` (a real, user-reachable
/// `WalnutException` — an arity mismatch between the number of operands on the postfix
/// stack and the token's own declared arity) and call straight through to the
/// `Expression::act` overloads U2/U5 already ported in [`crate::expr`], several of which
/// are themselves fallible ([`ExprError`]). [`Function::act`] additionally calls
/// `AutomatonQuantification.quantify` (`wr_core::quantify::quantify`), so its own error
/// type joins the union too.
///
/// A thin union, per this crate's established idiom (module-local enums, not one
/// unified `WalnutError` — see `predicate.rs`'s `LexError` docs on the same point) rather
/// than flattening every case into a single flat variant set.
#[derive(Debug)]
pub enum ActError {
    Token(TokenError),
    Expr(ExprError),
    Quantify(QuantifyError),
    /// Every `WalnutException` `NumberSystem`'s automaton builders raise, surfaced
    /// directly rather than through [`ExprError::NumberSystem`]. Added by U9: unlike
    /// [`crate::expr`]'s `act(...)` methods (which only ever call `getConstant`, so
    /// wrapping through `ExprError` cost nothing there), `RelationalOperator::act`/
    /// `ArithmeticOperator::act` call `comparison`/`arithmetic`/`getConstant` from a
    /// dozen sites and never build an [`ExprError`] of their own — routing those through
    /// `ExprError` would imply an expression-level failure that never happened.
    /// [`fmt::Display`] renders `NumSysError`'s verbatim Walnut text either way.
    NumberSystem(NumSysError),
}

impl From<TokenError> for ActError {
    fn from(e: TokenError) -> Self {
        ActError::Token(e)
    }
}

impl From<ExprError> for ActError {
    fn from(e: ExprError) -> Self {
        ActError::Expr(e)
    }
}

impl From<QuantifyError> for ActError {
    fn from(e: QuantifyError) -> Self {
        ActError::Quantify(e)
    }
}

impl From<NumSysError> for ActError {
    fn from(e: NumSysError) -> Self {
        ActError::NumberSystem(e)
    }
}

impl fmt::Display for ActError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActError::Token(e) => write!(f, "{e}"),
            ActError::Expr(e) => write!(f, "{e}"),
            // Verbatim `WalnutException.notFreeVariable` for the one variant reachable in
            // practice through `Function::act` (a name in the quantify list that isn't
            // actually a track of the result -- shouldn't happen given how `quantify` is
            // built here, but `wr_core::quantify::quantify` still reports it rather than
            // assuming). The other two variants (`UnsupportedLsdFixup`, `Minimize`) are
            // internal-invariant surfaces `wr_core::quantify` itself has no Java-text
            // `Display` for yet, so they fall back to their `Debug` shape rather than this
            // crate inventing wording Walnut never prints.
            ActError::Quantify(QuantifyError::NotFreeVariable(s)) => write!(
                f,
                "Variable {s} in the list of quantified variables is not a free variable."
            ),
            ActError::Quantify(other) => write!(f, "{other:?}"),
            ActError::NumberSystem(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ActError {}

// ---------------------------------------------------------------------------
// OperatorKind — the closed set `Operator.setPriority()` dispatches on
// ---------------------------------------------------------------------------

/// Which concrete Java `Operator` subclass (and, for quantifiers, which constructor
/// overload) this operator token stands in for. See the module docs for why this
/// replaces Java's class hierarchy rather than mirroring it 1:1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorKind {
    /// `LeftParenthesis`.
    LeftParen,
    /// `RightParenthesis`.
    RightParen,
    /// `LogicalOperator(position, EXISTS, quantifiedVariableCount)`.
    Exists { quantified_variable_count: usize },
    /// `LogicalOperator(position, FORALL, quantifiedVariableCount)`.
    Forall { quantified_variable_count: usize },
    /// `LogicalOperator(position, INFINITE, quantifiedVariableCount)`.
    Infinite { quantified_variable_count: usize },
    /// `LogicalOperator(position, op)` where `op` satisfies [`is_negation`] (any of the
    /// three tilde spellings).
    Negate,
    /// `LogicalOperator(position, Operator.REVERSE)`.
    Reverse,
    /// `LogicalOperator(position, LogicalOperator.AND)`.
    And,
    /// `LogicalOperator(position, LogicalOperator.OR)`.
    Or,
    /// `LogicalOperator(position, LogicalOperator.XOR)`.
    Xor,
    /// `LogicalOperator(position, LogicalOperator.IMPLY)`.
    Imply,
    /// `LogicalOperator(position, LogicalOperator.IFF)`.
    Iff,
    /// `RelationalOperator`.
    Relational(RelationalOp),
    /// `ArithmeticOperator`.
    Arithmetic(ArithmeticOp),
}

/// `Operator.setPriority()` (`Operator.java:78-123`) — the precedence table. Ported as
/// a free function of [`OperatorKind`] rather than a method that mutates a `priority`
/// field in place (Java's own shape), since every [`Operator`] constructor below needs
/// the value at construction time anyway and an `Operator` is never rebuilt with a
/// different `kind` after construction.
///
/// | Kind | Priority |
/// |---|---|
/// | [`OperatorKind::Relational`] (any) | 40 |
/// | [`OperatorKind::Arithmetic`]\([`ArithmeticOp::UnaryNegative`]) | 5 |
/// | [`OperatorKind::Arithmetic`]\([`ArithmeticOp::Mult`] \| [`ArithmeticOp::Div`]) | 10 |
/// | [`OperatorKind::Arithmetic`]\([`ArithmeticOp::Plus`] \| [`ArithmeticOp::Minus`]) | 20 |
/// | [`OperatorKind::Negate`] \| [`OperatorKind::Reverse`] | 80 |
/// | [`OperatorKind::And`] \| [`OperatorKind::Or`] \| [`OperatorKind::Xor`] | 90 |
/// | [`OperatorKind::Imply`] | 100 |
/// | [`OperatorKind::Iff`] | 110 |
/// | [`OperatorKind::Exists`] \| [`OperatorKind::Forall`] \| [`OperatorKind::Infinite`] | 150 |
/// | [`OperatorKind::LeftParen`] | 200 |
/// | [`OperatorKind::RightParen`] | 0 (see below) |
///
/// `RightParen`'s 0 is not a value Java's `setPriority()` ever computes — Java's
/// `RightParenthesis` constructor never calls `setPriority()` at all, leaving
/// `priority` at its `int` default (`0`). This is a genuine Java quirk (see
/// [`Operator::right_paren`]'s docs for the full picture, including the related
/// never-initialized `op` field), not a value this port invents; it is also never
/// read, since [`Operator::push_onto`] special-cases `RightParen` before consulting
/// priority at all (matching `RightParenthesis.put`'s total override of
/// `Operator.put`). Java's `setPriority()` ALSO has a `default:` switch arm reachable
/// only by a symbol matching none of the named constants — unreachable here by
/// construction, since every [`Operator`] constructor below classifies `op_text` into
/// one of the closed [`OperatorKind`] variants (or panics) rather than ever holding an
/// unclassified symbol; see [`Operator::logical_connective`]'s docs for why that
/// divergence from Java's silent-sentinel fallback is safe.
fn compute_priority(kind: &OperatorKind) -> i32 {
    if let OperatorKind::Relational(_) = kind {
        return 40;
    }
    if let OperatorKind::Arithmetic(op) = kind {
        return match op {
            ArithmeticOp::UnaryNegative => 5,
            ArithmeticOp::Mult | ArithmeticOp::Div => 10,
            ArithmeticOp::Plus | ArithmeticOp::Minus => 20,
        };
    }
    match kind {
        OperatorKind::Negate | OperatorKind::Reverse => 80,
        OperatorKind::And | OperatorKind::Or | OperatorKind::Xor => 90,
        OperatorKind::Imply => 100,
        OperatorKind::Iff => 110,
        OperatorKind::Exists { .. }
        | OperatorKind::Forall { .. }
        | OperatorKind::Infinite { .. } => 150,
        OperatorKind::LeftParen => 200,
        OperatorKind::RightParen => 0,
        OperatorKind::Relational(_) | OperatorKind::Arithmetic(_) => {
            unreachable!("handled by the early returns above")
        }
    }
}

// ---------------------------------------------------------------------------
// Operator
// ---------------------------------------------------------------------------

/// `Token/Operator.java` (159 LOC) — the common shape of every operator-kind token.
/// See the module docs for the scope boundary (`act()` semantics deferred to U9/U10).
#[derive(Debug, Clone)]
pub struct Operator {
    pub kind: OperatorKind,
    /// The exact operator text as lexed (`Operator.op`, `:41`) — preserved verbatim
    /// rather than re-derived from `kind`, because [`OperatorKind::Negate`] alone
    /// stands for three distinct spellings (`~`, ˜ U+02DC, ◌̃ U+0303) that must still
    /// round-trip through [`fmt::Display`]/future error text identically to Java's
    /// `op` field. Exception: [`Operator::right_paren`]'s Java field is genuinely
    /// uninitialized (`null`); see its docs for how this port represents that.
    op_text: String,
    position_in_predicate: usize,
    arity: usize,
    priority: i32,
    /// `RelationalOperator.ns`/`ArithmeticOperator.ns` — `Some` only for
    /// [`OperatorKind::Relational`]/[`OperatorKind::Arithmetic`]. An `Rc`, matching
    /// [`crate::predicate_env::PredicateEnv::number_system`]'s shared handle (Ruling 1),
    /// so constructing a `RelationalOperator`/`ArithmeticOperator` token never needs to
    /// clone or otherwise duplicate the formula's shared `NumberSystem`.
    ns: Option<Rc<NumberSystem>>,
}

impl Operator {
    /// `LeftParenthesis(int position)` (`LeftParenthesis.java:26-31`): `op = "("`,
    /// `setPriority()` (-> 200), `leftParenthesis = true`. `arity` is never set in
    /// Java (int default `0`), matching every other operator that isn't a quantifier.
    pub fn left_paren(position: usize) -> Self {
        let kind = OperatorKind::LeftParen;
        Operator {
            priority: compute_priority(&kind),
            kind,
            op_text: symbols::LEFT_PAREN.to_string(),
            position_in_predicate: position,
            arity: 0,
            ns: None,
        }
    }

    /// `RightParenthesis(int position)` (`RightParenthesis.java:28-30`). Two genuine
    /// Java quirks, both preserved *in effect* rather than literally (Rust has no
    /// `null`):
    ///
    /// 1. Java's constructor never sets `op` (`Operator.op` stays `null`) or calls
    ///    `setPriority()` (`priority` stays `0`). Nothing ever observes either: a
    ///    `RightParenthesis` is never pushed onto the operator stack (its own
    ///    [`Operator::push_onto`]/`RightParenthesis.put` pops and discards, it never
    ///    calls `S.push(this)`), so its `priority`/`toString()` are never read by
    ///    anything reachable — confirmed by tracing every caller of
    ///    `Operator.getPriority()`/`toString()`/`rightAssociativity()` (the last of
    ///    which would NPE on a `null` `op` if it were ever called on a
    ///    `RightParenthesis`, since `op.equals(REVERSE)` cannot be evaluated on `null`
    ///    — also unreachable for the same reason). This port gives `op_text` the value
    ///    `")"` for a sane [`fmt::Display`] rather than modeling `Option<String>`
    ///    everywhere else in this file to preserve an unreachable `null`.
    /// 2. `priority` is `0`, matching [`compute_priority`]'s `RightParen` arm — again,
    ///    never read (see [`Operator::push_onto`]).
    pub fn right_paren(position: usize) -> Self {
        let kind = OperatorKind::RightParen;
        Operator {
            priority: compute_priority(&kind),
            kind,
            op_text: ")".to_string(),
            position_in_predicate: position,
            arity: 0,
            ns: None,
        }
    }

    /// `LogicalOperator(int position, String op, int quantifiedVariableCount)`
    /// (`LogicalOperator.java:53-60`) for `op` in `{E, A, I}`. `arity =
    /// quantifiedVariableCount + 1` (the variables plus the final automaton operand).
    /// Panics if `op_text` is not one of [`symbols::EXISTS`]/[`symbols::FORALL`]/
    /// [`symbols::INFINITE`] — see [`Operator::logical_connective`]'s docs for why an
    /// unrecognized symbol here is an internal-invariant violation, not a real input.
    pub fn quantifier(
        position: usize,
        op_text: impl Into<String>,
        quantified_variable_count: usize,
    ) -> Self {
        let op_text = op_text.into();
        let kind = match op_text.as_str() {
            s if s == symbols::EXISTS => OperatorKind::Exists {
                quantified_variable_count,
            },
            s if s == symbols::FORALL => OperatorKind::Forall {
                quantified_variable_count,
            },
            s if s == symbols::INFINITE => OperatorKind::Infinite {
                quantified_variable_count,
            },
            _ => panic!("Operator::quantifier: unrecognized quantifier symbol {op_text:?}"),
        };
        Operator {
            priority: compute_priority(&kind),
            kind,
            op_text,
            position_in_predicate: position,
            arity: quantified_variable_count + 1,
            ns: None,
        }
    }

    /// `LogicalOperator(int position, String op)` (`:45-51`) for every `op` that is
    /// NOT a quantifier: `~`/˜/◌̃ (any spelling), `` ` ``, `&`, `|`, `^`, `=>`, `<=>`.
    /// `arity = (isNegation(op) || op.equals(REVERSE)) ? 1 : 2`.
    ///
    /// Panics on any other `op_text`. Java itself does NOT throw here — an
    /// unrecognized symbol silently produces `priority = Integer.MAX_VALUE` (via
    /// `setPriority()`'s `default:` arm) and `arity = 2`, a value nothing downstream
    /// is set up to interpret meaningfully. That fallback is reachable in Java only if
    /// some caller constructs a `LogicalOperator` with text `Predicate.java`'s own
    /// `PATTERN_FOR_LOGICAL_OPERATORS` could never match — i.e. never, via the only
    /// real construction path (the lexer, U3). This mirrors the same
    /// "prove-unreachable-then-simplify" reasoning `PORTING.md` already applies to
    /// `TRUE_FALSE_AUTOMATON`/`TRUE_AUTOMATON`: rather than model a `priority =
    /// i32::MAX` sentinel state every match arm in this crate would have to keep
    /// handling for no test to ever exercise, this constructor makes the invariant
    /// ("only ever called with an already-known-good symbol") a precondition enforced
    /// at the one call site (the future lexer) instead of a silently-succeeding
    /// fallback value threaded through everything after it.
    pub fn logical_connective(position: usize, op_text: impl Into<String>) -> Self {
        let op_text = op_text.into();
        let kind = if is_negation(&op_text) {
            OperatorKind::Negate
        } else if op_text == symbols::REVERSE {
            OperatorKind::Reverse
        } else if op_text == symbols::AND {
            OperatorKind::And
        } else if op_text == symbols::OR {
            OperatorKind::Or
        } else if op_text == symbols::XOR {
            OperatorKind::Xor
        } else if op_text == symbols::IMPLY {
            OperatorKind::Imply
        } else if op_text == symbols::IFF {
            OperatorKind::Iff
        } else {
            panic!("Operator::logical_connective: unrecognized operator symbol {op_text:?}");
        };
        let arity = if matches!(kind, OperatorKind::Negate | OperatorKind::Reverse) {
            1
        } else {
            2
        };
        Operator {
            priority: compute_priority(&kind),
            kind,
            op_text,
            position_in_predicate: position,
            arity,
            ns: None,
        }
    }

    /// `RelationalOperator(int position, String type, NumberSystem ns)`
    /// (`RelationalOperator.java:73-81`). `arity` is always 2. Panics on an
    /// unrecognized `op_text` — see [`relational_op_from_symbol`]'s docs.
    pub fn relational(position: usize, op_text: impl Into<String>, ns: Rc<NumberSystem>) -> Self {
        let op_text = op_text.into();
        let kind = OperatorKind::Relational(relational_op_from_symbol(&op_text));
        Operator {
            priority: compute_priority(&kind),
            kind,
            op_text,
            position_in_predicate: position,
            arity: 2,
            ns: Some(ns),
        }
    }

    /// `ArithmeticOperator(int position, String op, NumberSystem ns)`
    /// (`ArithmeticOperator.java:74-82`). `arity = 1` for unary negative (`_`), `2`
    /// otherwise. Panics on an unrecognized `op_text` — see
    /// [`arithmetic_op_from_symbol`]'s docs.
    pub fn arithmetic(position: usize, op_text: impl Into<String>, ns: Rc<NumberSystem>) -> Self {
        let op_text = op_text.into();
        let op = arithmetic_op_from_symbol(&op_text);
        let kind = OperatorKind::Arithmetic(op);
        let arity = if op == ArithmeticOp::UnaryNegative {
            1
        } else {
            2
        };
        Operator {
            priority: compute_priority(&kind),
            kind,
            op_text,
            position_in_predicate: position,
            arity,
            ns: Some(ns),
        }
    }

    pub fn arity(&self) -> usize {
        self.arity
    }

    pub fn position_in_predicate(&self) -> usize {
        self.position_in_predicate
    }

    pub fn priority(&self) -> i32 {
        self.priority
    }

    /// `Operator.isLeftParenthesis()` (`:70-72`): `this.leftParenthesis`, `true` only
    /// for a value built by [`Operator::left_paren`].
    pub fn is_left_parenthesis(&self) -> bool {
        matches!(self.kind, OperatorKind::LeftParen)
    }

    /// `Operator.rightAssociativity()` (`:74-76`): `op.equals(REVERSE) ||
    /// isNegation(op)`. Since `kind` already closes over that classification (see the
    /// module docs), this is exactly `kind == Negate || kind == Reverse` — no need to
    /// re-derive from `op_text`.
    pub fn right_associativity(&self) -> bool {
        matches!(self.kind, OperatorKind::Negate | OperatorKind::Reverse)
    }

    /// `Operator.validateArity(Stack<Expression>)` (`:146-148`).
    pub fn validate_arity(&self, stack_len: usize) -> Result<(), TokenError> {
        if stack_len < self.arity {
            return Err(TokenError::InsufficientOperands {
                op: self.op_text.clone(),
                arity: self.arity,
            });
        }
        Ok(())
    }

    /// `Operator.put(List<Token> postOrder, Stack<Operator> operatorStack)`
    /// (`:47-64`) for every kind except [`OperatorKind::RightParen`], which instead
    /// runs `RightParenthesis.put` (`RightParenthesis.java:32-42`) — Java expresses
    /// these as two methods reached via different static types at the tokenizer's call
    /// site (`instanceof Operator` picks the 2-arg `put`, dynamic dispatch on that
    /// picks `RightParenthesis`'s override); here both live behind one entry point
    /// since [`Token::push_onto`] already knows which [`OperatorKind`] it has.
    ///
    /// The shunting-yard algorithm itself, ported line-for-line:
    /// ```text
    /// if op is LEFT_PAREN, EXISTS, FORALL, or INFINITE:
    ///     push self onto operator_stack; return
    /// while operator_stack is not empty:
    ///     if top.priority <= self.priority:
    ///         if self.right_associative() && top.priority == self.priority:
    ///             break
    ///         pop top, append it to post_order
    ///     else:
    ///         break
    /// push self onto operator_stack
    /// ```
    /// The immediate-push special case for parens/quantifiers is NOT an optimization —
    /// without it, two adjacent same-priority quantifiers (`Ex Ey ...`) would
    /// incorrectly pop each other via the ordinary equal-priority/left-associative
    /// path below (quantifiers are left-associative — `right_associativity()` is
    /// `false` for them — so the loop WOULD pop an equal-priority top if this check
    /// didn't skip the loop entirely first).
    pub fn push_onto(
        self,
        post_order: &mut Vec<Token>,
        operator_stack: &mut Vec<Operator>,
    ) -> Result<(), TokenError> {
        if self.kind == OperatorKind::RightParen {
            return self.push_right_paren(post_order, operator_stack);
        }
        if matches!(
            self.kind,
            OperatorKind::LeftParen
                | OperatorKind::Exists { .. }
                | OperatorKind::Forall { .. }
                | OperatorKind::Infinite { .. }
        ) {
            operator_stack.push(self);
            return Ok(());
        }
        while let Some(top) = operator_stack.last() {
            if top.priority <= self.priority {
                if self.right_associativity() && top.priority == self.priority {
                    break;
                }
                let popped = operator_stack.pop().expect("just peeked via .last()");
                post_order.push(Token::Operator(popped));
            } else {
                break;
            }
        }
        operator_stack.push(self);
        Ok(())
    }

    /// `RightParenthesis.put(List<Token> postOrder, Stack<Operator> operatorStack)`
    /// (`RightParenthesis.java:32-42`): pop and emit every operator down to (and
    /// including, but NOT emitting) the nearest left parenthesis. Errors with
    /// [`TokenError::UnbalancedParenthesis`] — Java's `WalnutException.unbalancedParen`
    /// — if the stack empties out without finding one.
    fn push_right_paren(
        self,
        post_order: &mut Vec<Token>,
        operator_stack: &mut Vec<Operator>,
    ) -> Result<(), TokenError> {
        while let Some(top) = operator_stack.last() {
            if !top.is_left_parenthesis() {
                let popped = operator_stack.pop().expect("just peeked via .last()");
                post_order.push(Token::Operator(popped));
            } else {
                operator_stack.pop();
                return Ok(());
            }
        }
        Err(TokenError::UnbalancedParenthesis {
            position: self.position_in_predicate,
        })
    }
}

// ---------------------------------------------------------------------------
// U9 — `RelationalOperator.act` / `ArithmeticOperator.act` semantics
// ---------------------------------------------------------------------------
//
// U2 ported these two classes only as far as `Operator.setPriority` needed (the `Ops`
// symbol tables and the constructors above). This section is the rest: the
// automaton-BUILDING half, i.e. `RelationalOperator.java:87-222` and
// `ArithmeticOperator.java:88-280`, plus `Operator.andThenQuantifyIfArithmetic`
// (`Operator.java:150-158`) which exists solely to support them and which U2 therefore
// deferred to here by name.
//
// Two cross-cutting decisions, both continuations of rulings already made in this crate
// rather than new calls:
//
// * **No `Logging` calls.** Java sprinkles `Logging.logAndPrint(COMPUTING/COMPUTED …)`
//   and `Logging.indent()`/`dedent()` through both `act()` bodies (and through
//   `andThenQuantifyIfArithmetic` itself). Per `predicate_env.rs`'s Ruling 4, the logging
//   context is threaded by U11's postfix-token executor, not grown piecemeal into each
//   `act()` signature — the same reason `Word::act`/`Function::act`/every
//   `Expression::act` above omits them. Nothing else in these bodies depends on the log
//   state, so omitting the calls changes no automaton.
// * **`Token.getUniqueString()` becomes `&mut FreshIdentifiers`** (Ruling 4 again).
//   `ArithmeticOperator` is the source of two of that ruling's four Java call sites
//   (`:116` in `processUnaryOperator`, `:155` in `processBinaryOperator`); both are minted
//   at exactly the point Java mints them, because the *order* of minting is observable in
//   the synthetic names and one of the two (`:155`) is minted on paths that then discard
//   it (see [`Operator::process_binary_operator`]'s zero-multiplication note).
//
// Java reaches all of this through `Operator`'s abstract-class polymorphism
// (`RelationalOperator extends Operator`, `ArithmeticOperator extends Operator`, each
// overriding `act(Stack<Expression>)`); here it is a `match` on [`OperatorKind`] inside
// [`Operator::act`], exactly as U2's module docs predicted this unit would do it.

/// `Operator.andThenQuantifyIfArithmetic(Expression a, Automaton M)`
/// (`Operator.java:150-158`) — if (and only if) the operand was an
/// [`Expression::Arithmetic`], fold in its own defining automaton and existentially
/// eliminate the synthetic identifier that stands for its value.
///
/// This is what makes an [`ArithmeticExpression`] "an automaton plus a fresh variable
/// `x` that must later be quantified away" (see [`Expression`]'s own docs) actually
/// collapse to a plain predicate. Every other operand kind is returned untouched —
/// including [`Expression::Word`], whose analogous cleanup its callers do inline instead
/// (`and` with `word.M`, then `quantify` over `word.identifiersToQuantify`).
///
/// `Logging.indent()`/`dedent()` around the body are not ported (see this section's
/// header). Java's `quantify(M, a.identifier)` is the single-`String` overload
/// (`AutomatonQuantification.java:16-19`), which is just `quantify(A, Set.of(label))`.
///
/// `pub(crate)`, matching Java's package-private `static` — its only callers are the two
/// `act()` bodies below, and U10's `LogicalOperator.act` does not use it either.
pub(crate) fn and_then_quantify_if_arithmetic(
    a: &Expression,
    m: Automaton,
) -> Result<Automaton, ActError> {
    if let Expression::Arithmetic(ae) = a {
        let mut m = and(&m, &ae.m).into_automaton();
        quantify(&mut m, &BTreeSet::from([ae.identifier.clone()]))?;
        return Ok(m);
    }
    Ok(m)
}

/// `RelationalOperator.isConstantExpression` (`:202-204`) / `ArithmeticOperator.
/// isConstantExpression` (`:267-269`) — byte-identical private helpers in both classes.
fn is_constant_expression(e: &Expression) -> bool {
    matches!(
        e,
        Expression::NumberLiteral(_) | Expression::AlphabetLetter(_)
    )
}

/// `RelationalOperator.getConstantValue` (`:206-211`) / `ArithmeticOperator.
/// getConstantValue` (`:271-276`) — again identical in both classes.
///
/// The `unwrap_or(0)` reproduces Java's *field default* rather than adding a check Java
/// doesn't have: `e.constant` is a plain `int` field on the shared `Expression` base
/// class, `0` for every subclass whose constructor never assigns it. Both call sites are
/// guarded by [`is_constant_expression`], so only the two variants that DO assign it can
/// reach here — the fallback is unreachable, not a silent default (see
/// [`Expression::constant`]'s own docs on the narrowing this mirrors).
fn get_constant_value(e: &Expression) -> BigInt {
    match e {
        Expression::NumberLiteral(ne) => ne.value().clone(),
        _ => BigInt::from(e.constant().unwrap_or(0)),
    }
}

/// `a instanceof ArithmeticExpression || a instanceof VariableExpression` — the
/// two-variant narrowing both `act()` bodies test over and over (six times in
/// `RelationalOperator.act` alone). Exactly the set [`Expression::identifier`] answers
/// `Some` for.
fn is_arithmetic_or_variable(e: &Expression) -> bool {
    matches!(e, Expression::Arithmetic(_) | Expression::Variable(_))
}

/// `ArithmeticOperator.isValidArithmeticOperator(Expression)` (`:278-280`) — everything
/// except [`Expression::Automaton`]. Java spells out the five accepted subclasses rather
/// than excluding the one rejected one; kept as an explicit five-way `matches!` for the
/// same reason (if a seventh `Expression` kind ever appears, this must fail closed).
fn is_valid_arithmetic_operand(e: &Expression) -> bool {
    matches!(
        e,
        Expression::AlphabetLetter(_)
            | Expression::Word(_)
            | Expression::Arithmetic(_)
            | Expression::Variable(_)
            | Expression::NumberLiteral(_)
    )
}

/// `RelationalOperator.getIntConstantForWord` (`:195-200`) /
/// `ArithmeticOperator.getIntConstantForWord` (`:260-265`) — the same helper in both
/// classes except for ONE word of the error message, which is why `usage` is a parameter
/// here instead of the string being inlined: `RelationalOperator` says "used in word
/// automaton output **comparison**", `ArithmeticOperator` says "… output
/// **arithmetic**". Both are user-visible `WalnutException` text.
///
/// A word automaton's per-state output is a Java `int`, so a number literal compared or
/// combined against one must fit in an `int` — unlike every other constant path in these
/// two files, which keeps the unbounded `BigInteger`.
fn get_int_constant_for_word(e: &Expression, usage: &str) -> Result<i32, TokenError> {
    match e {
        Expression::NumberLiteral(ne) => ne
            .int_value_exact(&format!(
                "number literal {e} used in word automaton output {usage}"
            ))
            .map_err(TokenError::NumberLiteralOverflow),
        // `return e.constant;` — reachable only for `AlphabetLetterExpression` (both call
        // sites narrow to `is_constant_expression` first); see [`get_constant_value`]'s
        // note on the `0` fallback.
        _ => Ok(e.constant().unwrap_or(0)),
    }
}

/// `((WordExpression) e)` — the unchecked cast both `act()` bodies perform after an
/// `instanceof WordExpression` test. Panics rather than returning an `Option` for the
/// same reason Java's cast would `ClassCastException`: every call site below has already
/// tested the variant on the immediately preceding line, so a failure here is an internal
/// invariant violation, not user input (`PORTING.md`'s panic-for-precondition rule).
fn as_word(e: &Expression) -> &WordExpression {
    match e {
        Expression::Word(w) => w,
        other => panic!("as_word: expected a WordExpression, got {other:?}"),
    }
}

/// `a.identifier` read off an `Expression`-typed local the surrounding `if` has already
/// narrowed to `ArithmeticExpression | VariableExpression` — see
/// [`Expression::identifier`]'s docs, which were added by U2's fixer specifically for
/// these call sites. Panics for the same reason [`as_word`] does.
fn identifier_of(e: &Expression) -> &str {
    e.identifier().unwrap_or_else(|| {
        panic!("identifier_of: expected an arithmetic/variable expression, got {e:?}")
    })
}

/// `new HashSet<>(list)` — `AutomatonQuantification.quantify(Automaton, List<String>)`'s
/// own first line (`AutomatonQuantification.java:21-23`). A `BTreeSet` rather than a
/// `HashSet` per `PORTING.md`'s iteration-order rule; `quantify` itself is
/// order-insensitive (it maps each label to a track index independently), so this is a
/// determinism win with no behavior change.
fn label_set(labels: &[String]) -> BTreeSet<String> {
    labels.iter().cloned().collect()
}

impl Operator {
    /// `RelationalOperator.ns` / `ArithmeticOperator.ns`. Panics for a kind that has
    /// none — unreachable by construction, since only [`Operator::relational`] and
    /// [`Operator::arithmetic`] produce those two kinds and both demand an `Rc`.
    fn number_system(&self) -> &Rc<NumberSystem> {
        self.ns.as_ref().expect(
            "Operator::number_system: only Relational/Arithmetic kinds have one, and both \
             constructors require it",
        )
    }

    /// The `act(Stack<Expression>)` override each concrete `Operator` subclass supplies.
    ///
    /// Two of the three are ported here (U9): [`OperatorKind::Relational`] ->
    /// `RelationalOperator.act`, [`OperatorKind::Arithmetic`] -> `ArithmeticOperator.act`.
    /// `LogicalOperator.act` (every remaining kind that has one: the connectives, negate,
    /// reverse, and the three quantifiers) is **U10**, so those kinds still fall through
    /// to the inherited `Token.act` no-op — exactly as they did before this unit, and
    /// exactly as `LeftParenthesis`/`RightParenthesis` do permanently (neither ever
    /// reaches an operand stack: the tokenizer consumes them during the shunting yard,
    /// see [`Operator::push_onto`]).
    pub fn act(
        &self,
        fresh: &mut FreshIdentifiers,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        match self.kind {
            OperatorKind::Relational(opp) => self.act_relational(opp, stack),
            OperatorKind::Arithmetic(opp) => self.act_arithmetic(opp, fresh, stack),
            _ => Ok(()),
        }
    }

    /// `RelationalOperator.act(Stack<Expression> S)` (`RelationalOperator.java:87-177`).
    ///
    /// The operand-type dispatch, in Java's exact order (the order matters: several
    /// later arms would also match an earlier arm's operands):
    ///
    /// | # | `a` | `b` | Result |
    /// |---|---|---|---|
    /// | 0 | const | const | **constant-fold** to a TRUE/FALSE automaton, no `NumberSystem` touched |
    /// | 1 | word | arith\|var | word-output rewrite `⋀ᵢ (T[…] = @i ⇒ i op b)`, both operand orders |
    /// | 1 | arith\|var | word | (same arm, `reverse = true`) |
    /// | 2 | arith\|var | arith\|var | `ns.comparison(a.identifier, b.identifier, op)` |
    /// | 3 | const | arith\|var | `ns.comparison(constant, b.identifier, op)` |
    /// | 4 | arith\|var | const | `ns.comparison(a.identifier, constant, op)` |
    /// | 5 | word | word | `WordAutomaton.compareWordAutomata` |
    /// | 6 | word | const | `WordAutomaton.compareWordAutomaton(a.wordAutomaton, k, op)` |
    /// | 7 | const | word | same, with the relation **reversed** |
    /// | — | anything else | | `WalnutException.invalidDualOperators` |
    ///
    /// "const" is [`is_constant_expression`] (number literal or `@`-letter); "arith\|var"
    /// is [`is_arithmetic_or_variable`]. The only combinations that fall through to the
    /// final `else` are the ones involving an [`Expression::Automaton`] operand, e.g.
    /// `(x=1) < y`.
    fn act_relational(
        &self,
        opp: RelationalOp,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        // `super.validateArity(S)` (`:88`).
        self.validate_arity(stack.len())?;
        let ns = self.number_system();
        let mut b = stack.pop().expect("validated arity above");
        let mut a = stack.pop().expect("validated arity above");
        let op = &self.op_text;

        // ---- 0: constant folding (`:92-95`) --------------------------------
        // No automaton and no `NumberSystem` involved: `new Automaton(boolean)` is Java's
        // TRUE/FALSE automaton constructor. Note the comparison runs at FULL BigInteger
        // width (`getConstantValue` never narrows), so a literal far outside `int` still
        // folds exactly.
        if is_constant_expression(&a) && is_constant_expression(&b) {
            let truth = opp.compare_big_int(&get_constant_value(&a), &get_constant_value(&b));
            stack.push(Expression::Automaton(AutomatonExpression::new(
                format!("{a}{op}{b}"),
                Automaton::true_false(truth),
            )));
            return Ok(());
        }

        // ---- 1: word vs arithmetic/variable (`:99-134`) ---------------------
        if (matches!(a, Expression::Word(_)) && is_arithmetic_or_variable(&b))
            || (is_arithmetic_or_variable(&a) && matches!(b, Expression::Word(_)))
        {
            // Java's comment, kept: "We rewrite T[a] < b as
            // (T[a] = @0 => 0 < b) & (T[a] = @1 => 1 < b)", with one conjunct per output.
            let (word_expr, arith_expr, reverse) = if matches!(a, Expression::Word(_)) {
                (&a, &b, false)
            } else {
                (&b, &a, true)
            };
            let word = as_word(word_expr);
            let identifier = identifier_of(arith_expr);

            let mut m = Automaton::true_false(true);
            // `for (int o : word.wordAutomaton.fa.getO())` — Java iterates the raw
            // per-STATE output list, so a value shared by k states contributes k
            // identical conjuncts. Redundant, not wrong (`and` is idempotent); ported
            // verbatim rather than deduplicated, since deduplicating would change the
            // intermediate state counts the `details*` golden fixtures compare exactly.
            // Cloned up front only because the loop body clones `word.word_automaton`;
            // the list itself is never mutated here, in either language.
            for o in word.word_automaton.fa.o.clone() {
                let mut n = word.word_automaton.clone();
                compare_word_automaton(&mut n, o, RelationalOp::Equal);
                let mut c = if reverse {
                    ns.comparison_const_b(identifier, &BigInt::from(o), opp)?
                } else {
                    ns.comparison_const_a(&BigInt::from(o), identifier, opp)?
                };
                let n = imply(&mut n, &mut c).into_automaton();
                m = and(&m, &n).into_automaton();
            }
            m = and(&m, &word.m).into_automaton();
            quantify(&mut m, &label_set(&word.identifiers_to_quantify))?;
            m = and_then_quantify_if_arithmetic(arith_expr, m)?;
            // WB-023: `word.toString()`, NOT `a + op + b` like every sibling arm.
            stack.push(Expression::Automaton(AutomatonExpression::new(
                word_expr.to_string(),
                m,
            )));
            return Ok(());
        }

        // ---- 2: arithmetic/variable vs arithmetic/variable (`:135-140`) -----
        if is_arithmetic_or_variable(&a) && is_arithmetic_or_variable(&b) {
            let mut m = ns.comparison(identifier_of(&a), identifier_of(&b), opp);
            m = and_then_quantify_if_arithmetic(&a, m)?;
            m = and_then_quantify_if_arithmetic(&b, m)?;
            stack.push(Expression::Automaton(AutomatonExpression::new(
                format!("{a}{op}{b}"),
                m,
            )));
            return Ok(());
        }

        // ---- 3: constant vs arithmetic/variable (`:141-146`) ----------------
        if is_constant_expression(&a) && is_arithmetic_or_variable(&b) {
            let identifier = identifier_of(&b);
            let m = match &a {
                Expression::NumberLiteral(ne) => {
                    ns.comparison_const_a(ne.value(), identifier, opp)?
                }
                _ => ns.comparison_const_a(&get_constant_value(&a), identifier, opp)?,
            };
            let m = and_then_quantify_if_arithmetic(&b, m)?;
            stack.push(Expression::Automaton(AutomatonExpression::new(
                format!("{a}{op}{b}"),
                m,
            )));
            return Ok(());
        }

        // ---- 4: arithmetic/variable vs constant (`:147-152`) ----------------
        if is_arithmetic_or_variable(&a) && is_constant_expression(&b) {
            let identifier = identifier_of(&a);
            let m = match &b {
                Expression::NumberLiteral(ne) => {
                    ns.comparison_const_b(identifier, ne.value(), opp)?
                }
                _ => ns.comparison_const_b(identifier, &get_constant_value(&b), opp)?,
            };
            let m = and_then_quantify_if_arithmetic(&a, m)?;
            stack.push(Expression::Automaton(AutomatonExpression::new(
                format!("{a}{op}{b}"),
                m,
            )));
            return Ok(());
        }

        // ---- 5: word vs word (`:153-159`) -----------------------------------
        if matches!(a, Expression::Word(_)) && matches!(b, Expression::Word(_)) {
            let string_value = format!("{a}{op}{b}");
            let aw = as_word(&a);
            let bw = as_word(&b);
            // Java passes the raw symbol string here (`compareWordAutomata(…, op)`), which
            // `ProductStrategies` immediately maps back through `RELATIONAL_OPERATORS` —
            // i.e. to exactly `opp`. Passing `opp` skips a lossless round trip.
            let mut m = compare_word_automata(&aw.word_automaton, &bw.word_automaton, opp);
            m = and(&m, &aw.m).into_automaton();
            m = and(&m, &bw.m).into_automaton();
            // Two separate `quantify` calls, not one merged set — each one re-runs the
            // leading-zero fixup. Ported as-is (`:157-158`).
            quantify(&mut m, &label_set(&aw.identifiers_to_quantify))?;
            quantify(&mut m, &label_set(&bw.identifiers_to_quantify))?;
            stack.push(Expression::Automaton(AutomatonExpression::new(
                string_value,
                m,
            )));
            return Ok(());
        }

        // ---- 6: word vs constant (`:160-165`) -------------------------------
        if matches!(a, Expression::Word(_)) && is_constant_expression(&b) {
            // Evaluated before the mutation below, matching Java's argument-evaluation
            // order: an out-of-`int` literal must fail with `a.wordAutomaton` untouched.
            let k = get_int_constant_for_word(&b, "comparison")?;
            let string_value = format!("{a}{op}{b}");
            let aw = match &mut a {
                Expression::Word(w) => w,
                _ => unreachable!("matched above"),
            };
            // Mutates `a.wordAutomaton` IN PLACE (`compareWordAutomaton` returns void);
            // `a` is discarded right after, so the aliasing Java's `Automaton M =
            // a.wordAutomaton` sets up is unobservable.
            compare_word_automaton(&mut aw.word_automaton, k, opp);
            let mut m = and(&aw.word_automaton, &aw.m).into_automaton();
            quantify(&mut m, &label_set(&aw.identifiers_to_quantify))?;
            stack.push(Expression::Automaton(AutomatonExpression::new(
                string_value,
                m,
            )));
            return Ok(());
        }

        // ---- 7: constant vs word (`:166-171`) -------------------------------
        if is_constant_expression(&a) && matches!(b, Expression::Word(_)) {
            let k = get_int_constant_for_word(&a, "comparison")?;
            let string_value = format!("{a}{op}{b}");
            let bw = match &mut b {
                Expression::Word(w) => w,
                _ => unreachable!("matched above"),
            };
            // `k op T[…]` is `T[…] reverse(op) k` — hence `reverseOperator(opp)`.
            compare_word_automaton(&mut bw.word_automaton, k, opp.reverse_operator());
            let mut m = and(&bw.word_automaton, &bw.m).into_automaton();
            quantify(&mut m, &label_set(&bw.identifiers_to_quantify))?;
            stack.push(Expression::Automaton(AutomatonExpression::new(
                string_value,
                m,
            )));
            return Ok(());
        }

        // ---- else (`:172-174`) ----------------------------------------------
        Err(TokenError::InvalidDualOperators {
            op: op.clone(),
            a: a.to_string(),
            b: b.to_string(),
            a_type: a.java_class_name(),
            b_type: b.java_class_name(),
        }
        .into())
    }

    /// `ArithmeticOperator.act(Stack<Expression> S)` (`ArithmeticOperator.java:88-98`) —
    /// validate, pop `b`, reject an [`Expression::Automaton`] operand, then split on
    /// unary-vs-binary.
    fn act_arithmetic(
        &self,
        opp: ArithmeticOp,
        fresh: &mut FreshIdentifiers,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        self.validate_arity(stack.len())?;
        let b = stack.pop().expect("validated arity above");
        if !is_valid_arithmetic_operand(&b) {
            return Err(TokenError::InvalidOperator {
                op: self.op_text.clone(),
                operand: b.to_string(),
                operand_type: b.java_class_name(),
            }
            .into());
        }
        if opp == ArithmeticOp::UnaryNegative {
            self.process_unary_operator(b, fresh, stack)
        } else {
            self.process_binary_operator(opp, b, fresh, stack)
        }
    }

    /// `ArithmeticOperator.processUnaryOperator(Expression b, Stack<Expression> S)`
    /// (`:100-123`) — the `_` (unary minus) operator.
    ///
    /// Four cases, in Java's order: a number literal negates its `BigInteger` outright; an
    /// `@`-letter negates its `int`; a word automaton has its per-state OUTPUTS negated in
    /// place (as `0 - output`) and is pushed back unchanged otherwise; anything else
    /// (arithmetic/variable) gets an automaton for `b + c = 0` under a fresh `c`.
    fn process_unary_operator(
        &self,
        mut b: Expression,
        fresh: &mut FreshIdentifiers,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        let ns = self.number_system();
        let op = &self.op_text;

        if let Expression::NumberLiteral(ne) = &b {
            // `ne.value().negate()` — note the new literal takes THIS OPERATOR's number
            // system (`ns`), not the literal's own `base`, exactly as Java does.
            let value = -ne.value();
            stack.push(Expression::NumberLiteral(NumberLiteralExpression::new(
                value.to_string(),
                value,
                Rc::clone(ns),
            )));
            return Ok(());
        }
        if let Expression::AlphabetLetter(ae) = &b {
            let value = -ae.constant;
            stack.push(Expression::AlphabetLetter(AlphabetLetterExpression::new(
                format!("@{value}"),
                value,
            )));
            return Ok(());
        }
        if matches!(b, Expression::Word(_)) {
            {
                let bw = match &mut b {
                    Expression::Word(w) => w,
                    _ => unreachable!("matched above"),
                };
                // `applyWordArithOperator(b.wordAutomaton, 0, Ops.MINUS, false)`: with
                // `reverse == false` the per-state computation is `arith(MINUS, o=0,
                // thisP)`, i.e. `0 - output`. Note MINUS, not the `_` this operator IS —
                // `ArithmeticOp::UnaryNegative` is precisely the value
                // `ArithmeticOp::arith` rejects, so the rewrite is load-bearing.
                apply_word_arith_operator(&mut bw.word_automaton, 0, ArithmeticOp::Minus, false)?;
            }
            // The expression string is NOT updated, so `_T[i]` still displays as `T[i]`.
            stack.push(b);
            return Ok(());
        }

        // Arithmetic / variable: `b + c = 0` with a fresh `c` standing for `-b`.
        let c = fresh.next_identifier();
        let m =
            ns.arithmetic_const_c(identifier_of(&b), &c, &BigInt::from(0), ArithmeticOp::Plus)?;
        let m = and_then_quantify_if_arithmetic(&b, m)?;
        stack.push(Expression::Arithmetic(ArithmeticExpression::new(
            format!("({op}{b})"),
            m,
            c,
        )));
        Ok(())
    }

    /// `ArithmeticOperator.processBinaryOperator(Expression b, Stack<Expression> S)`
    /// (`:125-234`) — `+`, `-`, `*`, `/`.
    ///
    /// Dispatch order, again exactly Java's (and again order-sensitive):
    ///
    /// | # | `a` | `b` | Result |
    /// |---|---|---|---|
    /// | 0 | word | word | outputs combined pointwise; `a` mutated and re-pushed as a word |
    /// | 1 | word | const | `applyWordArithOperator(a.wordAutomaton, k, op, reverse=true)` |
    /// | 2 | const | word | same with `reverse=false` |
    /// | 3 | const | const | **constant-fold** to a new number literal (full `BigInteger`) |
    /// | 4 | word involved | | word-output rewrite `⋀ᵢ (T[…] = @i ⇒ i op b = c)` |
    /// | 5 | otherwise | | `ns.arithmetic(…, c, op)`, constant on either side or neither |
    ///
    /// Arms 0–3 return a non-[`Expression::Arithmetic`] value and never mint a synthetic
    /// name; arms 4–5 both produce `ArithmeticExpression("(a op b)", M, c)`.
    fn process_binary_operator(
        &self,
        opp: ArithmeticOp,
        mut b: Expression,
        fresh: &mut FreshIdentifiers,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        let ns = self.number_system();
        let op = &self.op_text;
        let mut a = stack.pop().expect("validated arity in act_arithmetic");
        if !is_valid_arithmetic_operand(&a) {
            return Err(TokenError::InvalidOperator {
                op: op.clone(),
                operand: a.to_string(),
                operand_type: a.java_class_name(),
            }
            .into());
        }

        // ---- 0: word ⊕ word (`:130-138`) ------------------------------------
        if matches!(a, Expression::Word(_)) && matches!(b, Expression::Word(_)) {
            {
                let bw = as_word(&b);
                let aw = match &mut a {
                    Expression::Word(w) => w,
                    _ => unreachable!("matched above"),
                };
                aw.word_automaton =
                    apply_word_operator(&aw.word_automaton, &bw.word_automaton, opp);
                aw.m = and(&aw.m, &bw.m).into_automaton();
                aw.identifiers_to_quantify
                    .extend(bw.identifiers_to_quantify.iter().cloned());
            }
            // `a` is pushed back with its ORIGINAL `expressionInString`, so `T[i]+U[i]`
            // still displays as `T[i]` — ported verbatim (cosmetic, same family as the
            // unary word case above).
            stack.push(a);
            return Ok(());
        }

        // ---- 1: word ⊕ constant (`:139-143`) --------------------------------
        if matches!(a, Expression::Word(_)) && is_constant_expression(&b) {
            let k = get_int_constant_for_word(&b, "arithmetic")?;
            {
                let aw = match &mut a {
                    Expression::Word(w) => w,
                    _ => unreachable!("matched above"),
                };
                // `reverse = true` -> per-state `arith(op, output, k)`, i.e. `T[…] op k`.
                apply_word_arith_operator(&mut aw.word_automaton, k, opp, true)?;
            }
            stack.push(a);
            return Ok(());
        }

        // ---- 2: constant ⊕ word (`:144-148`) --------------------------------
        if is_constant_expression(&a) && matches!(b, Expression::Word(_)) {
            let k = get_int_constant_for_word(&a, "arithmetic")?;
            {
                let bw = match &mut b {
                    Expression::Word(w) => w,
                    _ => unreachable!("matched above"),
                };
                // `reverse = false` -> per-state `arith(op, k, output)`, i.e. `k op T[…]`.
                apply_word_arith_operator(&mut bw.word_automaton, k, opp, false)?;
            }
            stack.push(b);
            return Ok(());
        }

        // ---- 3: constant ⊕ constant (`:150-154`) ----------------------------
        if is_constant_expression(&a) && is_constant_expression(&b) {
            // Full-width `BigInteger` arithmetic with no narrowing step — the result goes
            // straight into a new literal, so `DIV`'s floor-toward-negative-infinity
            // rounding is whatever `ArithmeticOp::arith_big_int` computes. Division by
            // zero surfaces here as `NumSysError::DivisionByZero`.
            let value = opp.arith_big_int(&get_constant_value(&a), &get_constant_value(&b))?;
            stack.push(Expression::NumberLiteral(NumberLiteralExpression::new(
                value.to_string(),
                value,
                Rc::clone(ns),
            )));
            return Ok(());
        }

        // `String c = getUniqueString();` (`:155`) — minted HERE, before the branch
        // split, which means the zero-multiplication short-circuits below discard it
        // after it has already advanced the counter. Ported at Java's exact position
        // because the counter's value is observable in later synthetic names.
        let c = fresh.next_identifier();
        let m: Automaton;

        // ---- 4: a word is still involved (`:158-198`) ------------------------
        // `a instanceof WordExpression || ((a is arith|var) && b instanceof
        // WordExpression)` — Java's `&&`-binds-tighter grouping. Reachable for
        // (word, arith|var) and (arith|var, word) only; every other word combination was
        // consumed by arms 0-2 above.
        if matches!(a, Expression::Word(_))
            || (is_arithmetic_or_variable(&a) && matches!(b, Expression::Word(_)))
        {
            // Java's comment, kept: "We rewrite T[a] * 5 = z as
            // (T[a] = @0 => 0 * 5 = z) & (T[a] = @1 => 1 * 5 = z)".
            let (word_expr, arith_expr, reverse) = if matches!(a, Expression::Word(_)) {
                (&a, &b, false)
            } else {
                (&b, &a, true)
            };
            let word = as_word(word_expr);
            let identifier = identifier_of(arith_expr);

            let mut acc = Automaton::true_false(true);
            for o in word.word_automaton.fa.o.clone() {
                let mut n = word.word_automaton.clone();
                compare_word_automaton(&mut n, o, RelationalOp::Equal);
                let mut cc = if o == 0 && opp == ArithmeticOp::Mult {
                    // `0 * anything = 0`, asserted directly on `c` — this is the ONLY
                    // conjunct that never mentions `arithmetic.identifier`, i.e. WB-003's
                    // "the other operand is never bound or validated" short-circuit, in
                    // its per-word-output form.
                    let mut k = ns.get_constant(&BigInt::from(0))?;
                    k.bind(vec![c.clone()]);
                    k
                } else if reverse {
                    ns.arithmetic_const_b(identifier, &BigInt::from(o), &c, opp)?
                } else {
                    ns.arithmetic_const_a(&BigInt::from(o), identifier, &c, opp)?
                };
                let n = imply(&mut n, &mut cc).into_automaton();
                acc = and(&acc, &n).into_automaton();
            }
            acc = and(&acc, &word.m).into_automaton();
            quantify(&mut acc, &label_set(&word.identifiers_to_quantify))?;
            m = and_then_quantify_if_arithmetic(arith_expr, acc)?;
        } else {
            // ---- 5: no word operand (`:200-231`) -----------------------------
            // Exactly one of `a`/`b` may be a constant here (arm 3 consumed the
            // both-constant case), so the four constant arms are mutually exclusive and
            // the final `else` sees two arithmetic/variable operands.
            let built = if let Expression::NumberLiteral(ne) = &a {
                // WB-003: `0 * x` folds to the literal `0` without ever building an
                // automaton for `x` — so an undeclared/misspelled `x` passes silently.
                // Ported verbatim, incl. the wasted `c` minted just above.
                if ne.is_zero() && opp == ArithmeticOp::Mult {
                    stack.push(Expression::NumberLiteral(NumberLiteralExpression::new(
                        "0",
                        BigInt::from(0),
                        Rc::clone(ns),
                    )));
                    return Ok(());
                }
                ns.arithmetic_const_a(ne.value(), identifier_of(&b), &c, opp)?
            } else if let Expression::AlphabetLetter(ae) = &a {
                if ae.constant == 0 && opp == ArithmeticOp::Mult {
                    stack.push(Expression::NumberLiteral(NumberLiteralExpression::new(
                        "0",
                        BigInt::from(0),
                        Rc::clone(ns),
                    )));
                    return Ok(());
                }
                ns.arithmetic_const_a(&BigInt::from(ae.constant), identifier_of(&b), &c, opp)?
            } else if let Expression::NumberLiteral(ne) = &b {
                if ne.is_zero() && opp == ArithmeticOp::Mult {
                    stack.push(Expression::NumberLiteral(NumberLiteralExpression::new(
                        "0",
                        BigInt::from(0),
                        Rc::clone(ns),
                    )));
                    return Ok(());
                }
                ns.arithmetic_const_b(identifier_of(&a), ne.value(), &c, opp)?
            } else if let Expression::AlphabetLetter(ae) = &b {
                if ae.constant == 0 && opp == ArithmeticOp::Mult {
                    stack.push(Expression::NumberLiteral(NumberLiteralExpression::new(
                        "0",
                        BigInt::from(0),
                        Rc::clone(ns),
                    )));
                    return Ok(());
                }
                ns.arithmetic_const_b(identifier_of(&a), &BigInt::from(ae.constant), &c, opp)?
            } else {
                ns.arithmetic(identifier_of(&a), identifier_of(&b), &c, opp)?
            };
            let built = and_then_quantify_if_arithmetic(&a, built)?;
            m = and_then_quantify_if_arithmetic(&b, built)?;
        }

        stack.push(Expression::Arithmetic(ArithmeticExpression::new(
            format!("({a}{op}{b})"),
            m,
            c,
        )));
        Ok(())
    }
}

impl fmt::Display for Operator {
    /// `Operator.toString()` (`:66-68`): `return op;` for every kind except
    /// `RelationalOperator`/`ArithmeticOperator`, which override it as `op + "_" + ns`
    /// (`RelationalOperator.java:83-85`, `ArithmeticOperator.java:84-86`) — and since
    /// `NumberSystem.toString()` (`NumberSystem.java:652-654`) is just `return name;`,
    /// that's `op_text + "_" + ns.name()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.ns {
            Some(ns) => write!(f, "{}_{}", self.op_text, ns.name()),
            None => write!(f, "{}", self.op_text),
        }
    }
}

// ---------------------------------------------------------------------------
// Leaf tokens: Variable, NumberLiteral, AlphabetLetter
// ---------------------------------------------------------------------------

/// `Token/Variable.java` (42 LOC).
#[derive(Debug, Clone)]
pub struct Variable {
    position_in_predicate: usize,
    name: String,
}

impl Variable {
    pub fn new(position: usize, name: impl Into<String>) -> Self {
        Variable {
            position_in_predicate: position,
            name: name.into(),
        }
    }

    /// `Variable.act(Stack<Expression> S)` (`:39-41`).
    pub fn act(&self, stack: &mut Vec<Expression>) {
        stack.push(Expression::Variable(VariableExpression::new(
            self.name.clone(),
        )));
    }
}

/// `Token/NumberLiteral.java` (46 LOC).
#[derive(Debug, Clone)]
pub struct NumberLiteral {
    position_in_predicate: usize,
    value: BigInt,
    base: Rc<NumberSystem>,
}

impl NumberLiteral {
    pub fn new(position: usize, value: BigInt, base: Rc<NumberSystem>) -> Self {
        NumberLiteral {
            position_in_predicate: position,
            value,
            base,
        }
    }

    /// `NumberLiteral.act(Stack<Expression> S)` (`:43-45`): pushes a
    /// [`NumberLiteralExpression`], nothing more — the `getConstant`-calling `act(...)`
    /// deferred in [`crate::expr`] belongs to `NumberLiteralExpression`, a completely
    /// different method this one never calls.
    pub fn act(&self, stack: &mut Vec<Expression>) {
        stack.push(Expression::NumberLiteral(NumberLiteralExpression::new(
            self.value.to_string(),
            self.value.clone(),
            Rc::clone(&self.base),
        )));
    }
}

/// `Token/AlphabetLetter.java` (42 LOC).
#[derive(Debug, Clone)]
pub struct AlphabetLetter {
    position_in_predicate: usize,
    value: i32,
}

impl AlphabetLetter {
    pub fn new(position: usize, value: i32) -> Self {
        AlphabetLetter {
            position_in_predicate: position,
            value,
        }
    }

    /// `AlphabetLetter.act(Stack<Expression> S)` (`:39-41`).
    pub fn act(&self, stack: &mut Vec<Expression>) {
        stack.push(Expression::AlphabetLetter(AlphabetLetterExpression::new(
            format!("@{}", self.value),
            self.value,
        )));
    }
}

// ---------------------------------------------------------------------------
// Word / Function — Phase 3a's U4
// ---------------------------------------------------------------------------

/// `wordAutomaton.getNS().get(i)` (`Word.java:62`), as honestly as this port can answer
/// it today.
///
/// `wr-core`'s [`Automaton`] does not retain [`NumberSystem`] objects per track — only
/// the two DERIVED facts `PORTING.md`'s "parallel vector" ruling settled on,
/// [`Automaton::msd`] (direction) and [`Automaton::all_reps`] (custom-base restriction) —
/// so `getNS().get(i)` cannot be a field read here. What it CAN be:
///
/// * a plain `msd_k`/`lsd_k` track (`all_reps[i]` is `None`, the common case) is
///   reconstructed on the fly from its direction ([`Automaton::msd`]) and its alphabet
///   size (`k`), via [`NumberSystem::new`]. This is semantically identical to Java's
///   cached instance (same name, same automata) — it just isn't the SAME object, so it
///   does not share [`NumberSystem`]'s U5 memoization with anything else in the formula.
///   That divergence is invisible to a caller: [`VariableExpression::act`] only ever
///   reads `ns.equality` here, never mutates or re-looks-up through it.
/// * a `{...}`-declared track (`msd[i]` is `None`) has no numeration in Java either —
///   correctly `None`, matching `getNS().get(i) == null` (see WB-013).
/// * a CUSTOM-base track (`all_reps[i]` is `Some`, e.g. `msd_fib`) **cannot** be
///   reconstructed this way: `wr-core` records only the valid-representations
///   restriction automaton, not which named custom base produced it. This function
///   conservatively returns `None` for that case too — a real, documented gap (not a
///   Walnut bug; this is a port limitation, not something `docs/WALNUT-BUGS.md` covers).
///   Consequence: a variable that indexes the SAME custom-base track twice
///   (`Fib[i][i] = @1` for some custom-base word `Fib`) hits
///   [`ExprError::RepeatedIdentifierMissingNumberSystem`] here even though real Walnut's
///   `getNS().get(i)` is non-null there and would succeed. Flagged for whichever later
///   unit gives `Automaton`/`PredicateEnv` a way to recover a custom base's identity.
fn track_number_system(automaton: &Automaton, i: usize) -> Option<NumberSystem> {
    if automaton.all_reps.get(i).and_then(|r| r.as_ref()).is_some() {
        return None; // custom base -- known gap, see docs above
    }
    let is_msd = automaton.msd.get(i).copied().flatten()?;
    let base = automaton.alphabet.get(i)?.len();
    let name = if is_msd {
        format!("msd_{base}")
    } else {
        format!("lsd_{base}")
    };
    NumberSystem::new(&name).ok()
}

/// `Token/Word.java` (80 LOC) — `T[i]`/`.NAME[i]`-style word/sequence occurrences.
#[derive(Debug, Clone)]
pub struct Word {
    position_in_predicate: usize,
    name: String,
    word_automaton: Automaton,
    /// `Word.arity` (via `Token.arity`, set from the constructor's `indexCount`
    /// parameter, `Word.java:42`) — the number of index brackets this occurrence had,
    /// NOT necessarily [`Self::word_automaton`]'s own arity (the constructor requires
    /// them to be equal, but keeps both concepts distinct exactly as Java does).
    arity: usize,
}

impl Word {
    /// `Word(int position, String name, Automaton wordAutomaton, int indexCount)`
    /// (`:38-44`). Fails with [`TokenError::WrongArgumentArity`] when `index_count`
    /// doesn't match `word_automaton`'s real arity — `Token.validateArity(name,
    /// wordAutomaton.getArity())`, an unchecked `WalnutException` in Java (called from
    /// the constructor itself), so this is a `Result` here, not a panic: it is
    /// user-triggered by any `T[i][j]...` whose bracket count doesn't match `T`'s
    /// declared alphabet (`PredicateTest.wordWithMissingClosingBracketConsumesToEndOfInput`).
    pub fn new(
        position: usize,
        name: impl Into<String>,
        word_automaton: Automaton,
        index_count: usize,
    ) -> Result<Self, TokenError> {
        let name = name.into();
        let actual_arity = word_automaton.get_arity();
        if actual_arity != index_count {
            return Err(TokenError::WrongArgumentArity {
                name,
                expected_arity: actual_arity,
                position,
            });
        }
        Ok(Word {
            position_in_predicate: position,
            name,
            word_automaton,
            arity: index_count,
        })
    }

    pub fn position_in_predicate(&self) -> usize {
        self.position_in_predicate
    }

    pub fn arity(&self) -> usize {
        self.arity
    }

    /// `Word.toString()` (`:46-48`): `return name;`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `Word.act(Stack<Expression> S)` (`:50-79`). `Logging.logAndPrint` calls are not
    /// ported (Ruling 4 in `predicate_env.rs`: the logging context is threaded by U11's
    /// executor, not grown piecemeal per `act()`), and the Java `expression == null`
    /// branch has no Rust equivalent (an [`Expression`] can never be null).
    pub fn act(
        &self,
        fresh: &mut FreshIdentifiers,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        // `super.validateArity(S, "word ", " indices")` (`Token.java:56-58`).
        if stack.len() < self.arity {
            return Err(TokenError::InsufficientStackOperands {
                name1: "word ".to_string(),
                token_display: self.name.clone(),
                arity: self.arity,
                name2: " indices".to_string(),
            }
            .into());
        }
        // `reverseStack(S)`: pop `arity` operands (rightmost first) onto a scratch
        // stack, whose OWN top is then the leftmost operand -- so popping IT one at a
        // time (below) yields the original left-to-right order.
        let mut temp: Vec<Expression> = Vec::with_capacity(self.arity);
        for _ in 0..self.arity {
            temp.push(stack.pop().expect("validated arity above"));
        }

        let mut string_value = self.name.clone();
        let mut identifiers: Vec<String> = Vec::new();
        let mut quantify: Vec<String> = Vec::new();
        let mut m = Automaton::true_false(true);
        let mut word_automaton = self.word_automaton.clone();

        for i in 0..self.arity {
            let expression = temp.pop().expect("temp has exactly `arity` elements");
            string_value.push('[');
            string_value.push_str(&expression.to_string());
            string_value.push(']');
            match &expression {
                Expression::Variable(ve) => {
                    let ns = track_number_system(&word_automaton, i);
                    m = ve.act(fresh, ns.as_ref(), &mut identifiers, m, &mut quantify)?;
                }
                Expression::Arithmetic(ae) => {
                    m = ae.act(&mut identifiers, m, &mut quantify);
                }
                Expression::NumberLiteral(ne) => {
                    m = ne.act(fresh, &mut identifiers, &mut quantify, m)?;
                }
                Expression::Automaton(ae) => {
                    m = ae.act(&self.name, i, m, &mut identifiers)?;
                }
                // `AlphabetLetterExpression`/`WordExpression`: the Java `else` arm,
                // `expression.act("argument " + (i + 1) + " of function " + this)` --
                // always fails ([`Expression::act`]'s base-class fallback). Note Java's
                // own hardcoded "function" wording even though `this` is a WORD, not a
                // function -- a cosmetic Java quirk, ported verbatim (not a WB-worthy
                // bug: the message is imprecise, never wrong about WHAT failed).
                _ => {
                    expression.act(&format!("argument {} of function {}", i + 1, self.name))?;
                }
            }
        }
        word_automaton.bind(identifiers);
        stack.push(Expression::Word(Box::new(WordExpression::new(
            string_value,
            word_automaton,
            m,
            quantify,
        ))));
        Ok(())
    }
}

/// `Token/Function.java` (99 LOC) — `$name(arg1, arg2, ...)` user-defined predicate
/// calls.
#[derive(Debug, Clone)]
pub struct Function {
    position_in_predicate: usize,
    name: String,
    automaton: Automaton,
    arity: usize,
    /// `Function.ns` (`Function.java:44`). Java builds this via `new
    /// NumberSystem(number_system)` directly (`:52`) -- NOT through the shared
    /// `NumberSystem.getComputeIfAbsent` cache every relational/arithmetic/number-literal
    /// token resolves through. So, unlike everywhere else
    /// [`crate::predicate_env::PredicateEnv::number_system`]'s `Rc`-shared handle is
    /// used (Ruling 1), a `Function`'s own number system is genuinely a FRESH,
    /// unmemoized instance in Java -- ported as an owned [`NumberSystem`] here, not an
    /// `Rc`, to make that non-sharing visible in the type instead of silently "fixing"
    /// it into sharing Java never does.
    ns: NumberSystem,
}

impl Function {
    /// `Function(String number_system, int position, String name, Automaton A, int
    /// argCount)` (`:47-54`). See [`Word::new`]'s docs on why this is fallible, not a
    /// panic. `ns` is the already-constructed [`NumberSystem`] Java's constructor builds
    /// inline (`new NumberSystem(number_system)`) -- passed in here rather than built
    /// from a bare name because building it can itself fail (an unresolvable `?ns`), and
    /// [`crate::predicate::Predicate::put_function`] already has the machinery to map
    /// that failure into a [`crate::predicate::LexError`] uniformly with every other
    /// number-system resolution in this crate.
    pub fn new(
        position: usize,
        name: impl Into<String>,
        automaton: Automaton,
        arg_count: usize,
        ns: NumberSystem,
    ) -> Result<Self, TokenError> {
        let name = name.into();
        let actual_arity = automaton.get_arity();
        if actual_arity != arg_count {
            return Err(TokenError::WrongArgumentArity {
                name,
                expected_arity: actual_arity,
                position,
            });
        }
        Ok(Function {
            position_in_predicate: position,
            name,
            automaton,
            arity: arg_count,
            ns,
        })
    }

    pub fn position_in_predicate(&self) -> usize {
        self.position_in_predicate
    }

    pub fn arity(&self) -> usize {
        self.arity
    }

    /// `Function.toString()` (`:56-58`): `return name;`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `Function.act(Stack<Expression> S)` (Java's own `act`, ~lines 60-98). Unlike
    /// [`Word::act`], a `Function` immediately collapses to a single automaton (via
    /// `AutomatonLogicalOps.and` + `AutomatonQuantification.quantify`) rather than
    /// deferring to a later comparison -- see [`WordExpression`] vs [`AutomatonExpression`].
    pub fn act(
        &self,
        fresh: &mut FreshIdentifiers,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        // `super.validateArity(S, "function ", " arguments")` (`Token.java:56-58`).
        if stack.len() < self.arity {
            return Err(TokenError::InsufficientStackOperands {
                name1: "function ".to_string(),
                token_display: self.name.clone(),
                arity: self.arity,
                name2: " arguments".to_string(),
            }
            .into());
        }
        let mut temp: Vec<Expression> = Vec::with_capacity(self.arity);
        for _ in 0..self.arity {
            temp.push(stack.pop().expect("validated arity above"));
        }
        // `Function.act`'s OWN second pop loop (`:69-71`), unlike `Word.act`'s single
        // pass over `temp`: `expressions` is read TWICE below (once to build
        // `stringValue`, once for the type-dispatch loop), so it must be a plain,
        // left-to-right, re-readable list rather than something drained once.
        let mut expressions: Vec<Expression> = Vec::with_capacity(self.arity);
        for _ in 0..self.arity {
            expressions.push(temp.pop().expect("temp has exactly `arity` elements"));
        }

        // `stringValue = this + "(" + genericListString(expressions, ",") + "))"`
        // (`:72`) -- WB-020, ported verbatim: note the DOUBLE closing paren.
        let joined = expressions
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let string_value = format!("{}({}))", self.name, joined);

        let mut m = Automaton::true_false(true);
        let mut identifiers: Vec<String> = Vec::new();
        let mut quantify: Vec<String> = Vec::new();
        for (i, expression) in expressions.iter().enumerate() {
            match expression {
                Expression::Variable(ve) => {
                    m = ve.act(fresh, Some(&self.ns), &mut identifiers, m, &mut quantify)?;
                }
                Expression::Arithmetic(ae) => {
                    m = ae.act(&mut identifiers, m, &mut quantify);
                }
                Expression::NumberLiteral(ne) => {
                    m = ne.act(fresh, &mut identifiers, &mut quantify, m)?;
                }
                Expression::Automaton(ae) => {
                    m = ae.act(&self.name, i, m, &mut identifiers)?;
                }
                // See `Word::act`'s matching arm docs -- same base-class fallback,
                // same verbatim (if imprecise for a genuinely-word argument) wording.
                _ => {
                    expression.act(&format!("argument {} of function {}", i + 1, self.name))?;
                }
            }
        }

        let mut bound = self.automaton.clone();
        bound.bind(identifiers);
        let mut anded = and(&bound, &m).into_automaton();
        let quantify_set: BTreeSet<String> = quantify.into_iter().collect();
        wr_core::quantify::quantify(&mut anded, &quantify_set)?;

        stack.push(Expression::Automaton(AutomatonExpression::new(
            string_value,
            anded,
        )));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// `Token/Token.java` (71 LOC) — the closed set of concrete token kinds. See the
/// module docs for why this is a sum type.
#[derive(Debug, Clone)]
pub enum Token {
    Operator(Operator),
    Variable(Variable),
    NumberLiteral(NumberLiteral),
    AlphabetLetter(AlphabetLetter),
    Word(Word),
    /// Boxed: `Function` (`NumberSystem` + `Automaton` + `String` + two `usize`s) makes
    /// this variant far larger than its siblings, tripping `clippy::large_enum_variant`
    /// — the same "box the large field" fix `PORTING.md`'s ruling already applied to
    /// `Expression::Word` for the identical reason. Pure indirection; every call site
    /// still reads through it via `Deref`.
    Function(Box<Function>),
}

impl Token {
    /// `Token.arity` (`protected int arity`, `:29`) — `0` for `Variable`/`NumberLiteral`/
    /// `AlphabetLetter` (never set by their constructors, matching Java's `int`
    /// default), [`Operator::arity`] for an operator, and — as of U4 — [`Word::arity`]/
    /// [`Function::arity`] for the two remaining direct `Token` subclasses (`Word.java:42`:
    /// `this.arity = indexCount`; `Function.java`'s constructor sets it from the declared
    /// argument count).
    pub fn arity(&self) -> usize {
        match self {
            Token::Operator(op) => op.arity(),
            Token::Variable(_) | Token::NumberLiteral(_) | Token::AlphabetLetter(_) => 0,
            Token::Word(w) => w.arity(),
            Token::Function(f) => f.arity(),
        }
    }

    /// `Token.getPositionInPredicate()` (`:52-54`).
    pub fn position_in_predicate(&self) -> usize {
        match self {
            Token::Operator(op) => op.position_in_predicate(),
            Token::Variable(t) => t.position_in_predicate,
            Token::NumberLiteral(t) => t.position_in_predicate,
            Token::AlphabetLetter(t) => t.position_in_predicate,
            Token::Word(w) => w.position_in_predicate(),
            Token::Function(f) => f.position_in_predicate(),
        }
    }

    /// `Token.isOperator()` (`:48-50`), overridden by `Operator` to `true` (`:43-45`).
    pub fn is_operator(&self) -> bool {
        matches!(self, Token::Operator(_))
    }

    /// `Token.act(Stack<Expression> S)` (`:46`, no-op default) — overridden by
    /// `Variable`/`NumberLiteral`/`AlphabetLetter` (U2), by `Word`/`Function` (U4,
    /// `Word.java:50-79`, `Function.java`'s own `act`), and — as of U9 — by
    /// `RelationalOperator`/`ArithmeticOperator` via [`Operator::act`]. `Operator` itself
    /// still never overrides the inherited no-op directly; `LogicalOperator.act` remains
    /// deferred to U10, so those operator kinds fall through [`Operator::act`]'s own
    /// catch-all to `Ok(())`.
    ///
    /// `fresh` is [`FreshIdentifiers`] (`predicate_env.rs`'s Ruling 4) — needed
    /// transitively by `Word`/`Function::act` via the `Expression::act` overloads they
    /// call, and now threaded through every arm even though the U2 leaf arms below
    /// still ignore it, so this signature does not have to change again when U9/U10 add
    /// `Operator::act` (which needs it too).
    pub fn act(
        &self,
        fresh: &mut FreshIdentifiers,
        stack: &mut Vec<Expression>,
    ) -> Result<(), ActError> {
        match self {
            Token::Variable(t) => t.act(stack),
            Token::NumberLiteral(t) => t.act(stack),
            Token::AlphabetLetter(t) => t.act(stack),
            Token::Operator(op) => return op.act(fresh, stack),
            Token::Word(w) => return w.act(fresh, stack),
            Token::Function(f) => return f.act(fresh, stack),
        }
        Ok(())
    }

    /// The tokenizer's own dispatch (`instanceof Operator` -> the 2-arg `put`, else
    /// the inherited 1-arg `Token.put(List<Token> postOrder)` which is just
    /// `postOrder.add(this)`, `Token.java:42-44`) collapsed into one entry point,
    /// since a [`Token`] here already knows which case it is. Not itself a literal
    /// Java method — `Token.put`/`Operator.put`/`RightParenthesis.put` combined — but
    /// exactly what any real caller (the future U3 lexer) needs to call per token.
    pub fn push_onto(
        self,
        post_order: &mut Vec<Token>,
        operator_stack: &mut Vec<Operator>,
    ) -> Result<(), TokenError> {
        match self {
            Token::Operator(op) => op.push_onto(post_order, operator_stack),
            leaf => {
                post_order.push(leaf);
                Ok(())
            }
        }
    }

    /// `Token.validateArity(Stack<Expression>, String, String)` (`Token.java:56-58`).
    /// `stack_len` stands in for Java's `S.size()` (the caller's operand stack length).
    pub fn validate_arity_stack(
        &self,
        stack_len: usize,
        name1: &str,
        name2: &str,
    ) -> Result<(), TokenError> {
        if stack_len < self.arity() {
            return Err(TokenError::InsufficientStackOperands {
                name1: name1.to_string(),
                token_display: self.to_string(),
                arity: self.arity(),
                name2: name2.to_string(),
            });
        }
        Ok(())
    }

    /// `Token.validateArity(String, int)` (`:60-63`).
    pub fn validate_arity(&self, name: &str, other_arity: usize) -> Result<(), TokenError> {
        if other_arity != self.arity() {
            return Err(TokenError::WrongArgumentArity {
                name: name.to_string(),
                expected_arity: other_arity,
                position: self.position_in_predicate(),
            });
        }
        Ok(())
    }

    /// `Token.reverseStack(Stack<Expression> S)` (`:65-71`): pops `arity()` elements
    /// off `stack` into a fresh `Vec` via the exact same push/pop sequence Java's
    /// `Stack`-based version uses (Rust's `Vec::push`/`Vec::pop` are the same LIFO
    /// operations as `java.util.Stack`'s, so this is a direct 1:1 translation, not a
    /// reimplementation) — so that popping the RESULT (via [`Vec::pop`], mirroring
    /// Java's `temp.pop()`) yields elements in their original left-to-right push order
    /// rather than LIFO order. Callers (U10's `LogicalOperator.actQuantifier`, once
    /// ported) must call [`Token::validate_arity_stack`] or [`Operator::validate_arity`]
    /// first — this panics, matching Java's unchecked `EmptyStackException`, if `stack`
    /// holds fewer than `arity()` elements.
    pub fn reverse_stack(&self, stack: &mut Vec<Expression>) -> Vec<Expression> {
        let arity = self.arity();
        let mut temp = Vec::with_capacity(arity);
        for _ in 0..arity {
            temp.push(
                stack
                    .pop()
                    .expect("caller must validate_arity before reverse_stack"),
            );
        }
        temp
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Operator(op) => write!(f, "{op}"),
            Token::Variable(t) => write!(f, "{}", t.name),
            Token::NumberLiteral(t) => write!(f, "{}", t.value),
            Token::AlphabetLetter(t) => write!(f, "{}", t.value),
            Token::Word(w) => write!(f, "{}", w.name()),
            Token::Function(func) => write!(f, "{}", func.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(name: &str) -> Rc<NumberSystem> {
        Rc::new(NumberSystem::new(name).unwrap())
    }

    // ------------------------------------------------------------- is_negation

    #[test]
    fn is_negation_recognizes_all_three_tilde_spellings() {
        assert!(is_negation("~")); // ASCII, U+007E
        assert!(is_negation("\u{02dc}")); // ˜
        assert!(is_negation("\u{0303}")); // combining ◌̃
    }

    #[test]
    fn is_negation_rejects_other_single_and_multi_char_strings() {
        assert!(!is_negation("`"));
        assert!(!is_negation("&"));
        assert!(!is_negation("<=>"));
        assert!(!is_negation(""));
    }

    // ------------------------------------------------------- symbol tables

    #[test]
    fn relational_symbol_round_trips() {
        for op in [
            RelationalOp::Equal,
            RelationalOp::NotEqual,
            RelationalOp::LessThan,
            RelationalOp::GreaterThan,
            RelationalOp::LessEqThan,
            RelationalOp::GreaterEqThan,
        ] {
            assert_eq!(relational_op_from_symbol(relational_op_symbol(op)), op);
        }
    }

    /// The round-trip test above only checks the two new functions against EACH OTHER —
    /// a reviewer showed that swapping BOTH functions' mapping for `NotEqual`<->`"<>"`
    /// (an invalid pairing no real symbol uses) still passes it, since the round trip is
    /// self-consistent either way. `"="`/`">="` are independently pinned elsewhere in
    /// this file (`display_appends_ns_name_for_relational_and_arithmetic`,
    /// `priority_relational_is_40`), but the other four symbols were not. This pins all
    /// six literal symbols from `RelationalOperator.Ops` (`RelationalOperator.java:46-51`)
    /// against `RelationalOp`'s variants directly, in both directions, mirroring
    /// [`arithmetic_symbol_round_trips`]'s stronger pattern (which already pins against
    /// `wr_core::numsys::ArithmeticOp::symbol()`, a pre-existing independent source).
    #[test]
    fn relational_symbol_table_matches_java_literal_symbols_both_directions() {
        let table = [
            (RelationalOp::Equal, "="),
            (RelationalOp::NotEqual, "!="),
            (RelationalOp::LessThan, "<"),
            (RelationalOp::GreaterThan, ">"),
            (RelationalOp::LessEqThan, "<="),
            (RelationalOp::GreaterEqThan, ">="),
        ];
        for (op, symbol) in table {
            assert_eq!(
                relational_op_symbol(op),
                symbol,
                "{op:?} -> symbol mismatch"
            );
            assert_eq!(
                relational_op_from_symbol(symbol),
                op,
                "{symbol:?} -> op mismatch"
            );
        }
    }

    #[test]
    fn arithmetic_symbol_round_trips() {
        for op in [
            ArithmeticOp::Plus,
            ArithmeticOp::Minus,
            ArithmeticOp::Div,
            ArithmeticOp::Mult,
            ArithmeticOp::UnaryNegative,
        ] {
            assert_eq!(arithmetic_op_from_symbol(op.symbol()), op);
        }
    }

    #[test]
    #[should_panic(expected = "Unknown comparison operator: ~=")]
    fn relational_from_symbol_panics_on_garbage() {
        relational_op_from_symbol("~=");
    }

    #[test]
    #[should_panic(expected = "Unknown arithmetic operator: %")]
    fn arithmetic_from_symbol_panics_on_garbage() {
        arithmetic_op_from_symbol("%");
    }

    // -------------------------------------------------- the exact precedence table

    #[test]
    fn priority_relational_is_40() {
        let op = Operator::relational(0, "=", ns("msd_2"));
        assert_eq!(op.priority(), 40);
        let op = Operator::relational(0, ">=", ns("msd_2"));
        assert_eq!(op.priority(), 40);
    }

    #[test]
    fn priority_arithmetic_unary_negative_is_5() {
        let op = Operator::arithmetic(0, "_", ns("msd_2"));
        assert_eq!(op.priority(), 5);
        assert_eq!(op.arity(), 1);
    }

    #[test]
    fn priority_arithmetic_mult_div_is_10() {
        assert_eq!(Operator::arithmetic(0, "*", ns("msd_2")).priority(), 10);
        assert_eq!(Operator::arithmetic(0, "/", ns("msd_2")).priority(), 10);
    }

    #[test]
    fn priority_arithmetic_plus_minus_is_20() {
        assert_eq!(Operator::arithmetic(0, "+", ns("msd_2")).priority(), 20);
        assert_eq!(Operator::arithmetic(0, "-", ns("msd_2")).priority(), 20);
    }

    #[test]
    fn priority_negate_and_reverse_is_80() {
        assert_eq!(Operator::logical_connective(0, "~").priority(), 80);
        assert_eq!(Operator::logical_connective(0, "\u{02dc}").priority(), 80);
        assert_eq!(Operator::logical_connective(0, "\u{0303}").priority(), 80);
        assert_eq!(Operator::logical_connective(0, "`").priority(), 80);
    }

    #[test]
    fn priority_and_or_xor_is_90() {
        assert_eq!(Operator::logical_connective(0, "&").priority(), 90);
        assert_eq!(Operator::logical_connective(0, "|").priority(), 90);
        assert_eq!(Operator::logical_connective(0, "^").priority(), 90);
    }

    #[test]
    fn priority_imply_is_100() {
        assert_eq!(Operator::logical_connective(0, "=>").priority(), 100);
    }

    #[test]
    fn priority_iff_is_110() {
        assert_eq!(Operator::logical_connective(0, "<=>").priority(), 110);
    }

    #[test]
    fn priority_quantifiers_is_150() {
        assert_eq!(Operator::quantifier(0, "E", 1).priority(), 150);
        assert_eq!(Operator::quantifier(0, "A", 2).priority(), 150);
        assert_eq!(Operator::quantifier(0, "I", 1).priority(), 150);
    }

    #[test]
    fn priority_left_paren_is_200() {
        assert_eq!(Operator::left_paren(0).priority(), 200);
    }

    #[test]
    fn arity_matches_java_exactly() {
        assert_eq!(Operator::left_paren(0).arity(), 0);
        assert_eq!(Operator::right_paren(0).arity(), 0);
        assert_eq!(Operator::quantifier(0, "E", 3).arity(), 4);
        assert_eq!(Operator::logical_connective(0, "~").arity(), 1);
        assert_eq!(Operator::logical_connective(0, "`").arity(), 1);
        assert_eq!(Operator::logical_connective(0, "&").arity(), 2);
        assert_eq!(Operator::relational(0, "=", ns("msd_2")).arity(), 2);
        assert_eq!(Operator::arithmetic(0, "_", ns("msd_2")).arity(), 1);
        assert_eq!(Operator::arithmetic(0, "+", ns("msd_2")).arity(), 2);
    }

    // ------------------------------------------------------------ associativity

    #[test]
    fn negation_and_reverse_are_right_associative_both_tilde_spellings() {
        assert!(Operator::logical_connective(0, "~").right_associativity());
        assert!(Operator::logical_connective(0, "\u{02dc}").right_associativity());
        assert!(Operator::logical_connective(0, "\u{0303}").right_associativity());
        assert!(Operator::logical_connective(0, "`").right_associativity());
    }

    #[test]
    fn everything_else_is_left_associative() {
        assert!(!Operator::logical_connective(0, "&").right_associativity());
        assert!(!Operator::logical_connective(0, "=>").right_associativity());
        assert!(!Operator::relational(0, "<", ns("msd_2")).right_associativity());
        assert!(!Operator::arithmetic(0, "+", ns("msd_2")).right_associativity());
        assert!(!Operator::quantifier(0, "E", 1).right_associativity());
        assert!(!Operator::left_paren(0).right_associativity());
    }

    // --------------------------------------------------------------------- Display

    #[test]
    fn display_is_bare_op_text_for_non_relational_arithmetic_kinds() {
        assert_eq!(Operator::logical_connective(0, "&").to_string(), "&");
        assert_eq!(Operator::quantifier(0, "E", 1).to_string(), "E");
        assert_eq!(Operator::left_paren(0).to_string(), "(");
    }

    #[test]
    fn display_appends_ns_name_for_relational_and_arithmetic() {
        assert_eq!(
            Operator::relational(0, "=", ns("msd_2")).to_string(),
            "=_msd_2"
        );
        assert_eq!(
            Operator::arithmetic(0, "+", ns("lsd_3")).to_string(),
            "+_lsd_3"
        );
    }

    #[test]
    fn leaf_token_display_matches_java_tostring() {
        let v = Token::Variable(Variable::new(0, "x"));
        assert_eq!(v.to_string(), "x");
        let n = Token::NumberLiteral(NumberLiteral::new(0, BigInt::from(42), ns("msd_2")));
        assert_eq!(n.to_string(), "42");
        let a = Token::AlphabetLetter(AlphabetLetter::new(0, -1));
        assert_eq!(a.to_string(), "-1");
    }

    // ------------------------------------------------------------ shunting yard

    /// `a & b | c` — `&`/`|` share priority 90 and are left-associative, so the second
    /// operator (`|`) must pop the first (`&`) before pushing itself, yielding
    /// postfix `a b & c |`.
    #[test]
    fn equal_priority_left_associative_pops_before_pushing() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();

        Token::Variable(Variable::new(0, "a"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(1, "&"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(2, "b"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(3, "|"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(4, "c"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        // Drain the remaining operator stack, as the tokenizer's final flush would.
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        assert_eq!(
            post_order.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            vec!["a", "b", "&", "c", "|"]
        );
    }

    /// `~~a` — negation is right-associative, so the SECOND `~` must NOT pop the
    /// first one (equal priority, right-associative -> the `break` fires), yielding
    /// postfix `a ~ ~` (both negations after the operand, still nested outer-first).
    #[test]
    fn right_associative_equal_priority_does_not_pop() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();

        Token::Operator(Operator::logical_connective(0, "~"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(1, "~"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(2, "a"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        assert_eq!(
            post_order.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            vec!["a", "~", "~"]
        );
    }

    /// `Ex Ey (x=y)` — two adjacent quantifiers must NOT pop each other despite equal
    /// priority and left-associativity; the immediate-push special case is what
    /// prevents that (see [`Operator::push_onto`]'s docs).
    #[test]
    fn adjacent_quantifiers_do_not_pop_each_other() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();

        Token::Operator(Operator::quantifier(0, "E", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::quantifier(1, "E", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();

        assert_eq!(
            operator_stack.len(),
            2,
            "both quantifiers must still be on the stack"
        );
        assert!(post_order.is_empty());
    }

    /// `a + b * c` — `*` (priority 10) binds tighter than `+` (priority 20); since `*`
    /// is encountered while `+` is on the stack with a STRICTLY LOWER priority number
    /// (higher precedence in this table -- lower numeric priority binds first, see the
    /// `<=` comparison in `push_onto`), `+` must NOT be popped when `*` arrives.
    #[test]
    fn higher_precedence_operator_does_not_pop_lower_priority_number() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();
        let n = ns("msd_2");

        Token::Variable(Variable::new(0, "a"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::arithmetic(1, "+", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(2, "b"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::arithmetic(3, "*", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(4, "c"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        // Postfix: a b c * +  (multiplication evaluated first)
        assert_eq!(
            post_order.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            vec!["a", "b", "c", "*_msd_2", "+_msd_2"]
        );
    }

    /// `(a & b) | c` — a left parenthesis is pushed immediately (never triggers a
    /// pop), and the matching right parenthesis pops everything down to (and
    /// consuming) it without emitting the paren itself.
    #[test]
    fn parentheses_group_without_being_emitted() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();

        Token::Operator(Operator::left_paren(0))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(1, "a"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(2, "&"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(3, "b"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::right_paren(4))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(5, "|"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(6, "c"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        assert_eq!(
            post_order.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            vec!["a", "b", "&", "c", "|"]
        );
    }

    // The five regression tests below pin `push_onto`'s "immediate push, skip the pop
    // loop" special case for `(`/`E`/`A`/`I` (see its own doc comment) — a mutation
    // test proved every OTHER test in this module stays green even if that special case
    // is deleted from the match arms, despite it being load-bearing (without it, `(`
    // at priority 200 would pop the entire operator stack instead of just being pushed,
    // and adjacent same-priority quantifiers would incorrectly pop each other via the
    // ordinary left-associative equal-priority path). Expected postfix sequences are
    // Java's real `Predicate.toString()` output (`UtilityMethods.genericListString(
    // postOrder, ":")`, `Predicate.java:517-519`) against the real `walnut-java` jar —
    // the `:`-joined shape (not space-joined) is Java's own `toString`, not a Rust
    // convention, so the assertions below join with `:` to match it directly rather
    // than reformatting into the other tests' bare `vec![...]` shape.

    /// `x=1 & (y=2)` — the immediate-push case for `(` is what stops priority-200 `(`
    /// from popping `=` (priority 40) off the stack when it's pushed.
    #[test]
    fn parenthesized_operand_is_not_popped_by_the_open_paren_itself() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();
        let n = ns("msd_2");

        Token::Variable(Variable::new(0, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(1, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::NumberLiteral(NumberLiteral::new(2, BigInt::from(1), Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(3, "&"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::left_paren(4))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(5, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(6, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::NumberLiteral(NumberLiteral::new(7, BigInt::from(2), Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::right_paren(8))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        let rendered = post_order
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(rendered, "x:1:=_msd_2:y:2:=_msd_2:&");
    }

    /// `~(x=1) & y=2` — the immediate-push case for `(` matters here too: without it,
    /// `(` (priority 200) arriving while `~` (priority 80) sits on the stack would still
    /// not pop `~` (200 > 80), so this case alone wouldn't distinguish the two
    /// implementations — the real distinguishing power is `(` never popping anything
    /// AND never itself being subject to `~`'s right-associativity check. Included
    /// anyway as the exact case a reviewer's mutation test flagged.
    #[test]
    fn negated_parenthesized_operand_conjoined_with_a_second_comparison() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();
        let n = ns("msd_2");

        Token::Operator(Operator::logical_connective(0, "~"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::left_paren(1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(2, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(3, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::NumberLiteral(NumberLiteral::new(4, BigInt::from(1), Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::right_paren(5))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(6, "&"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(7, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(8, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::NumberLiteral(NumberLiteral::new(9, BigInt::from(2), Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        let rendered = post_order
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(rendered, "x:1:=_msd_2:~:y:2:=_msd_2:&");
    }

    /// `E x A y (x=y)` — without the immediate-push case, `A` (priority 150,
    /// left-associative) arriving while `E` (also 150) is on top would incorrectly pop
    /// `E` via the ordinary equal-priority/left-associative path (see
    /// [`equal_priority_left_associative_pops_before_pushing`]); the immediate-push
    /// case for quantifiers skips that loop entirely so adjacent quantifiers nest
    /// instead of popping each other.
    #[test]
    fn adjacent_exists_forall_quantifiers_nest_rather_than_pop() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();
        let n = ns("msd_2");

        Token::Operator(Operator::quantifier(0, "E", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(1, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::quantifier(2, "A", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(3, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::left_paren(4))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(5, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(6, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(7, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::right_paren(8))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        let rendered = post_order
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(rendered, "x:y:x:y:=_msd_2:A:E");
    }

    /// `E x I y (x=y)` — same shape as the `E`/`A` case above, with `I` (infinite
    /// quantifier) in the immediate-push set instead of `A`.
    #[test]
    fn adjacent_exists_infinite_quantifiers_nest_rather_than_pop() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();
        let n = ns("msd_2");

        Token::Operator(Operator::quantifier(0, "E", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(1, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::quantifier(2, "I", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(3, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::left_paren(4))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(5, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(6, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(7, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::right_paren(8))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        let rendered = post_order
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(rendered, "x:y:x:y:=_msd_2:I:E");
    }

    /// `E x (x=1) & A y (y=2)` — combines both special cases: `E`'s low binding power
    /// (priority 150, higher than `&`'s 90) must NOT be popped when `&` arrives (`E`
    /// stays on the stack, extending its scope over the whole rest of the formula,
    /// matching Walnut's `E x φ & ψ` ≡ `E x (φ & ψ)` semantics), and `A` immediately
    /// after `&` must push directly rather than being compared against `&`/`E`'s
    /// priorities at all.
    #[test]
    fn quantifier_scope_extends_across_a_following_conjunction() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();
        let n = ns("msd_2");

        Token::Operator(Operator::quantifier(0, "E", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(1, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::left_paren(2))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(3, "x"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(4, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::NumberLiteral(NumberLiteral::new(5, BigInt::from(1), Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::right_paren(6))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::logical_connective(7, "&"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::quantifier(8, "A", 1))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(9, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::left_paren(10))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Variable(Variable::new(11, "y"))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::relational(12, "=", Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::NumberLiteral(NumberLiteral::new(13, BigInt::from(2), Rc::clone(&n)))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        Token::Operator(Operator::right_paren(14))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap();
        while let Some(op) = operator_stack.pop() {
            post_order.push(Token::Operator(op));
        }

        let rendered = post_order
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(rendered, "x:x:1:=_msd_2:y:y:2:=_msd_2:A:&:E");
    }

    #[test]
    fn unmatched_right_paren_reports_walnut_message_text() {
        let mut post_order = Vec::new();
        let mut operator_stack = Vec::new();
        let err = Token::Operator(Operator::right_paren(7))
            .push_onto(&mut post_order, &mut operator_stack)
            .unwrap_err();
        assert_eq!(err, TokenError::UnbalancedParenthesis { position: 7 });
        assert_eq!(err.to_string(), "unbalanced parenthesis: char at 7");
    }

    // -------------------------------------------------------------- act() dispatch

    #[test]
    fn variable_token_act_pushes_variable_expression() {
        let mut stack = Vec::new();
        let mut fresh = FreshIdentifiers::new();
        Token::Variable(Variable::new(0, "x"))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(stack.len(), 1);
        match &stack[0] {
            Expression::Variable(v) => assert_eq!(v.identifier, "x"),
            other => panic!("expected Variable, got {other:?}"),
        }
    }

    #[test]
    fn number_literal_token_act_pushes_number_literal_expression_without_mutating_ns() {
        let mut stack = Vec::new();
        let mut fresh = FreshIdentifiers::new();
        let n = ns("msd_2");
        Token::NumberLiteral(NumberLiteral::new(0, BigInt::from(7), Rc::clone(&n)))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(stack.len(), 1);
        match &stack[0] {
            Expression::NumberLiteral(nl) => {
                assert_eq!(*nl.value(), BigInt::from(7));
                assert!(Rc::ptr_eq(nl.base(), &n));
            }
            other => panic!("expected NumberLiteral, got {other:?}"),
        }
    }

    #[test]
    fn alphabet_letter_token_act_pushes_alphabet_letter_expression() {
        let mut stack = Vec::new();
        let mut fresh = FreshIdentifiers::new();
        Token::AlphabetLetter(AlphabetLetter::new(0, -1))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(stack.len(), 1);
        match &stack[0] {
            Expression::AlphabetLetter(al) => assert_eq!(al.constant, -1),
            other => panic!("expected AlphabetLetter, got {other:?}"),
        }
    }

    #[test]
    fn operator_token_act_is_a_noop_pending_u9_u10() {
        let mut stack = Vec::new();
        let mut fresh = FreshIdentifiers::new();
        Token::Operator(Operator::logical_connective(0, "&"))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert!(stack.is_empty());
    }

    // ------------------------------------------------------------- Word / Function

    /// A 2-track `msd_2` word automaton, unlabeled (as every word occurrence's automaton
    /// is before `Word`/`Function::act` binds it). The transition table is empty --
    /// irrelevant to what these tests check (arity/binding/dispatch, never the
    /// automaton's language) — mirroring `expr.rs`'s own `labeled_automaton` helper.
    fn stub_word_automaton() -> Automaton {
        let fa = wr_core::fa::Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 4,
            o: vec![1],
            d: vec![std::collections::BTreeMap::new()],
        };
        Automaton::new(
            fa,
            vec![vec![0, 1], vec![0, 1]],
            Vec::new(),
            vec![Some(true), Some(true)],
        )
    }

    #[test]
    fn word_new_rejects_an_index_count_mismatch_with_the_automatons_own_arity() {
        let err = Word::new(0, "T", stub_word_automaton(), 1).unwrap_err();
        assert_eq!(
            err.to_string(),
            "function T requires 2 arguments: char at 0"
        );
    }

    #[test]
    fn word_act_binds_variables_and_pushes_a_word_expression() {
        let word = Word::new(0, "T", stub_word_automaton(), 2).unwrap();
        let mut fresh = FreshIdentifiers::new();
        // Push in source order (a, b) -- `Word::act` reverses internally, same contract
        // as `Token::reverse_stack`.
        let mut stack = vec![
            Expression::Variable(VariableExpression::new("a")),
            Expression::Variable(VariableExpression::new("b")),
        ];
        Token::Word(word).act(&mut fresh, &mut stack).unwrap();
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].to_string(), "T[a][b]");
        match &stack[0] {
            Expression::Word(w) => {
                assert_eq!(
                    w.word_automaton.label,
                    vec!["a".to_string(), "b".to_string()]
                );
                assert!(w.identifiers_to_quantify.is_empty());
            }
            other => panic!("expected Word, got {other:?}"),
        }
    }

    #[test]
    fn function_new_rejects_an_arg_count_mismatch_with_the_automatons_own_arity() {
        let err = Function::new(
            1,
            "phi",
            stub_word_automaton(),
            1,
            NumberSystem::new("msd_2").unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "function phi requires 2 arguments: char at 1"
        );
    }

    #[test]
    fn function_act_conjoins_and_quantifies_producing_an_automaton_expression() {
        let func = Function::new(
            0,
            "phi",
            stub_word_automaton(),
            2,
            NumberSystem::new("msd_2").unwrap(),
        )
        .unwrap();
        let mut fresh = FreshIdentifiers::new();
        let mut stack = vec![
            Expression::Variable(VariableExpression::new("a")),
            Expression::Variable(VariableExpression::new("b")),
        ];
        Token::Function(Box::new(func))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(stack.len(), 1);
        // WB-020: the double closing paren is Java's own text, ported verbatim.
        assert_eq!(stack[0].to_string(), "phi(a,b))");
        assert!(matches!(stack[0], Expression::Automaton(_)));
    }

    #[test]
    fn word_and_function_act_reject_an_insufficient_stack() {
        let word = Word::new(0, "T", stub_word_automaton(), 2).unwrap();
        let mut fresh = FreshIdentifiers::new();
        let mut stack = vec![Expression::Variable(VariableExpression::new("a"))];
        let err = Token::Word(word).act(&mut fresh, &mut stack).unwrap_err();
        assert_eq!(err.to_string(), "word T requires 2 indices");

        let func = Function::new(
            0,
            "phi",
            stub_word_automaton(),
            2,
            NumberSystem::new("msd_2").unwrap(),
        )
        .unwrap();
        let mut stack = vec![Expression::Variable(VariableExpression::new("a"))];
        let err = Token::Function(Box::new(func))
            .act(&mut fresh, &mut stack)
            .unwrap_err();
        assert_eq!(err.to_string(), "function phi requires 2 arguments");
    }

    // --------------------------------------------------------------- validate_arity

    // NOTE on the next two tests: `Token::validate_arity_stack`/`Token::validate_arity`
    // exist in Java (`Token.java:56-63`) purely to support `Word`/`Function`'s
    // constructors and `act()` bodies -- the ONLY real Java callers. `Word`/`Function`
    // now exist (U4) and inline their OWN copy of the `Stack`-overload's logic directly
    // in `act()` (see `Word::act`/`Function::act`'s docs) rather than calling through
    // `Token::validate_arity_stack` on a not-yet-constructed `Token` -- so these two
    // tests still exercise the shared mechanism generically against an `&` operator and
    // a bare `Variable`, shapes real Java code never actually calls these two `Token`
    // methods on. The real `Word`/`Function` call-site coverage for arity/message
    // shape lives in the "Word / Function" tests further down instead.
    #[test]
    fn validate_arity_stack_reports_java_message_shape() {
        let t = Token::Variable(Variable::new(0, "T"));
        // arity() is 0 for a leaf token, so this can only be exercised via an
        // Operator; build a 2-arity `&` and call the Token-level (not Operator-level)
        // overload to pin its distinct message shape.
        let op_token = Token::Operator(Operator::logical_connective(0, "&"));
        let err = op_token
            .validate_arity_stack(1, "word ", " indices")
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "word & requires 2 indices",
            "name1 + token.toString() + \" requires \" + arity + name2, verbatim"
        );
        assert!(t.validate_arity_stack(0, "word ", " indices").is_ok());
    }

    #[test]
    fn validate_arity_reports_java_message_shape() {
        let t = Token::Variable(Variable::new(5, "x"));
        let err = t.validate_arity("phi", 2).unwrap_err();
        assert_eq!(
            err.to_string(),
            "function phi requires 2 arguments: char at 5"
        );
        assert!(t.validate_arity("phi", 0).is_ok());
    }

    #[test]
    fn operator_validate_arity_reports_java_message_shape() {
        let op = Operator::logical_connective(0, "&");
        let err = op.validate_arity(1).unwrap_err();
        assert_eq!(err.to_string(), "operator & requires 2 operands");
        assert!(op.validate_arity(2).is_ok());
    }

    // ---------------------------------------------------------------- reverse_stack

    /// Mirrors `LogicalOperator.actQuantifier`'s usage: push operands in left-to-right
    /// source order, then `reverse_stack` + repeated `.pop()` must yield them back in
    /// that SAME left-to-right order (not LIFO order) -- see `Token::reverse_stack`'s
    /// docs for why the direct push/pop translation already guarantees this.
    #[test]
    fn reverse_stack_yields_original_left_to_right_order_via_pop() {
        let quantifier = Operator::quantifier(0, "E", 2); // arity 3: var, var, automaton
        let mut stack = vec![
            Expression::Variable(VariableExpression::new("x")),
            Expression::Variable(VariableExpression::new("y")),
            Expression::Automaton(crate::expr::AutomatonExpression::new(
                "phi",
                wr_core::automaton::Automaton::true_false(true),
            )),
        ];
        let token = Token::Operator(quantifier);
        let mut temp = token.reverse_stack(&mut stack);
        assert!(stack.is_empty());

        // Popping `temp` must reproduce x, y, phi -- the ORIGINAL push order.
        let first = temp.pop().unwrap();
        assert!(matches!(first, Expression::Variable(ref v) if v.identifier == "x"));
        let second = temp.pop().unwrap();
        assert!(matches!(second, Expression::Variable(ref v) if v.identifier == "y"));
        let third = temp.pop().unwrap();
        assert!(matches!(third, Expression::Automaton(_)));
        assert!(temp.is_empty());
    }

    // =====================================================================
    // U9 — `RelationalOperator.act` / `ArithmeticOperator.act`
    // =====================================================================

    use std::collections::BTreeMap as Map;
    use wr_core::fa::Fa;

    // ------------------------------------------------------- fixtures

    fn number_literal(value: i64, base: &Rc<NumberSystem>) -> Expression {
        big_literal(BigInt::from(value), base)
    }

    fn big_literal(value: BigInt, base: &Rc<NumberSystem>) -> Expression {
        Expression::NumberLiteral(NumberLiteralExpression::new(
            value.to_string(),
            value,
            Rc::clone(base),
        ))
    }

    fn alphabet_letter(value: i32) -> Expression {
        Expression::AlphabetLetter(AlphabetLetterExpression::new(format!("@{value}"), value))
    }

    fn variable(name: &str) -> Expression {
        Expression::Variable(VariableExpression::new(name))
    }

    /// A deterministic, total, single-track msd-base-2 DFAO with the given per-state
    /// outputs and transition table (`d[state][digit] = destination`), bound to `label`.
    fn dfao(label: &str, outputs: &[i32], d: &[[usize; 2]]) -> Automaton {
        let q = outputs.len();
        assert_eq!(q, d.len());
        let mut table: Vec<Map<i32, Vec<usize>>> = Vec::with_capacity(q);
        for row in d {
            let mut m = Map::new();
            m.insert(0, vec![row[0]]);
            m.insert(1, vec![row[1]]);
            table.push(m);
        }
        Automaton::new(
            Fa {
                q0: 0,
                q,
                alphabet_size: 2,
                o: outputs.to_vec(),
                d: table,
                true_false: None,
            },
            vec![vec![0, 1]],
            vec![label.to_string()],
            vec![Some(true)],
        )
    }

    /// Thue–Morse: the output is the parity of the number of `1` digits, so
    /// `T[i] == 1` exactly for odd-popcount `i` (leading zeros don't change it).
    fn thue_morse(label: &str) -> Automaton {
        dfao(label, &[0, 1], &[[0, 1], [1, 0]])
    }

    /// The constant-`1` DFAO — its single state outputs `1`.
    fn always_one(label: &str) -> Automaton {
        dfao(label, &[1], &[[0, 0]])
    }

    /// A [`WordExpression`] shaped the way `Word::act` leaves one for a plain,
    /// first-occurrence variable index: the bound word automaton, a TRUE accumulator,
    /// and nothing pending quantification.
    fn word_expression(display: &str, word_automaton: Automaton) -> Expression {
        Expression::Word(Box::new(WordExpression::new(
            display,
            word_automaton,
            Automaton::true_false(true),
            vec![],
        )))
    }

    // ------------------------------------------------- semantic oracle

    /// msd base-`base` digits of `value`, zero-padded on the left to `width`.
    fn msd_digits(value: u32, base: u32, width: usize) -> Vec<i32> {
        let mut digits = Vec::new();
        let mut v = value;
        while v > 0 {
            digits.push((v % base) as i32);
            v /= base;
        }
        assert!(
            digits.len() <= width,
            "{value} needs more than {width} base-{base} digits"
        );
        while digits.len() < width {
            digits.push(0);
        }
        digits.reverse();
        digits
    }

    /// Does `a` accept the given per-track values, each written msd base-`base` and
    /// zero-padded to a common `width`?
    ///
    /// A deliberately INDEPENDENT oracle: it walks `a.fa`'s transitions directly over
    /// symbols built with `a`'s own `RichAlphabet` encoding, rather than comparing `a`
    /// against a second automaton built from the same `NumberSystem` primitives the code
    /// under test used (where both sides could share one mistake). Tracks are looked up
    /// BY NAME, so it is also immune to the track-permutation gap
    /// `wr_core::equiv::automaton_language_equivalent` documents.
    fn accepts(a: &Automaton, base: u32, width: usize, values: &[(&str, u32)]) -> bool {
        assert_eq!(
            a.label.len(),
            values.len(),
            "expected one value per track; automaton tracks are {:?}",
            a.label
        );
        let mut per_track: Vec<Vec<i32>> = vec![Vec::new(); a.label.len()];
        for (name, value) in values {
            let idx = a
                .label
                .iter()
                .position(|l| l == name)
                .unwrap_or_else(|| panic!("no track named {name:?} in {:?}", a.label));
            per_track[idx] = msd_digits(*value, base, width);
        }
        let word: Vec<i32> = (0..width)
            .map(|pos| {
                let digits: Vec<i32> = per_track.iter().map(|d| d[pos]).collect();
                a.encode(&digits)
            })
            .collect();
        a.fa.accepts_word(&word)
    }

    /// Runs one operator against a prepared operand stack and returns the single
    /// [`Expression`] it leaves behind.
    fn act_once(
        op: &Operator,
        fresh: &mut FreshIdentifiers,
        mut stack: Vec<Expression>,
    ) -> Expression {
        op.act(fresh, &mut stack)
            .unwrap_or_else(|e| panic!("act failed: {e}"));
        assert_eq!(stack.len(), 1, "act must leave exactly one operand");
        stack.pop().unwrap()
    }

    fn as_automaton_expression(e: Expression) -> AutomatonExpression {
        match e {
            Expression::Automaton(ae) => ae,
            other => panic!("expected an AutomatonExpression, got {other:?}"),
        }
    }

    // =====================================================================
    // RelationalOperator: constant folding (`RelationalOperator.java:92-95`)
    // =====================================================================

    #[test]
    fn relational_constant_folding_of_two_number_literals() {
        let n = ns("msd_2");
        for (a, b, op, expected) in [
            (5i64, 7i64, "<", true),
            (7, 5, "<", false),
            (5, 5, "<", false),
            (5, 5, "<=", true),
            (5, 5, "=", true),
            (5, 7, "!=", true),
            (7, 5, ">", true),
            (5, 5, ">=", true),
        ] {
            let operator = Operator::relational(0, op, Rc::clone(&n));
            let mut fresh = FreshIdentifiers::new();
            let result = act_once(
                &operator,
                &mut fresh,
                vec![number_literal(a, &n), number_literal(b, &n)],
            );
            let ae = as_automaton_expression(result);
            assert!(
                ae.m.is_true_false_automaton(),
                "constant folding must produce a trivial automaton, never a real one"
            );
            assert_eq!(
                ae.m.is_true_automaton(),
                expected,
                "{a} {op} {b} should fold to {expected}"
            );
            assert_eq!(
                fresh.issued(),
                0,
                "constant folding mints no synthetic names"
            );
        }
    }

    /// `a + op + b`, using the operator's RAW symbol (`Operator.op`), not its
    /// `toString()` (which would append `_msd_2`).
    #[test]
    fn relational_constant_folding_expression_string_is_a_op_b() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, ">=", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![number_literal(7, &n), number_literal(5, &n)],
        );
        assert_eq!(result.to_string(), "7>=5");
    }

    /// `@`-letters fold too — and, unlike every path that goes through `NumberSystem`,
    /// NEGATIVE ones are fine here, because constant folding never touches the number
    /// system and so never reaches `validateNeg`.
    #[test]
    fn relational_constant_folding_accepts_negative_alphabet_letters() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![alphabet_letter(-1), alphabet_letter(0)],
        );
        assert!(as_automaton_expression(result).m.is_true_automaton());
    }

    /// Why `RelationalOp::compare_big_int` exists: `getConstantValue` keeps the
    /// literal's unbounded `BigInteger`, so a value far outside `i32` must still fold
    /// exactly rather than overflow or truncate.
    #[test]
    fn relational_constant_folding_is_exact_far_outside_i32() {
        let n = ns("msd_2");
        let huge: BigInt = BigInt::from(10).pow(30);
        let operator = Operator::relational(0, ">", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![big_literal(huge.clone(), &n), number_literal(5, &n)],
        );
        assert!(as_automaton_expression(result).m.is_true_automaton());

        let operator = Operator::relational(0, "=", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![big_literal(huge.clone(), &n), big_literal(&huge + 1, &n)],
        );
        assert!(
            !as_automaton_expression(result).m.is_true_automaton(),
            "two distinct 31-digit literals must not compare equal"
        );
    }

    // =====================================================================
    // RelationalOperator: the general (automaton-building) arms
    // =====================================================================

    #[test]
    fn relational_variable_vs_variable_builds_the_real_relation() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![variable("x"), variable("y")],
        );
        assert_eq!(result.to_string(), "x<y");
        let m = as_automaton_expression(result).m;
        assert_eq!(m.label.len(), 2);
        for (x, y) in [(0u32, 1u32), (1, 2), (2, 5)] {
            assert!(
                accepts(&m, 2, 5, &[("x", x), ("y", y)]),
                "{x} < {y} must be accepted"
            );
        }
        for (x, y) in [(1u32, 1u32), (2, 1), (5, 0)] {
            assert!(
                !accepts(&m, 2, 5, &[("x", x), ("y", y)]),
                "{x} < {y} must be rejected"
            );
        }
    }

    #[test]
    fn relational_variable_vs_number_literal_and_the_mirror_image() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "=", Rc::clone(&n));

        // x = 5
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![variable("x"), number_literal(5, &n)],
        ))
        .m;
        assert!(accepts(&m, 2, 5, &[("x", 5)]));
        assert!(!accepts(&m, 2, 5, &[("x", 4)]));

        // 5 = x — the constant now on the LEFT, which Java routes through
        // `comparison(BigInteger, String, op)` by REVERSING the relation.
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![number_literal(5, &n), variable("x")],
        ))
        .m;
        assert!(accepts(&m, 2, 5, &[("x", 5)]));
        assert!(!accepts(&m, 2, 5, &[("x", 4)]));
    }

    /// Reversal is only observable for an ASYMMETRIC relation: `3 < x` must not be
    /// `x < 3`. This is the arm that would silently pass if `comparison_const_a` were
    /// wired straight to `comparison_const_b` without the `reverse_operator()` step.
    #[test]
    fn relational_constant_on_the_left_reverses_the_relation() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![number_literal(3, &n), variable("x")],
        ))
        .m;
        assert!(accepts(&m, 2, 5, &[("x", 4)]), "3 < 4");
        assert!(!accepts(&m, 2, 5, &[("x", 3)]), "3 < 3 is false");
        assert!(!accepts(&m, 2, 5, &[("x", 2)]), "3 < 2 is false");

        // ... and the alphabet-letter form of the same arm.
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![alphabet_letter(3), variable("x")],
        ))
        .m;
        assert!(accepts(&m, 2, 5, &[("x", 4)]));
        assert!(!accepts(&m, 2, 5, &[("x", 2)]));
    }

    #[test]
    fn relational_variable_vs_alphabet_letter_constant() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![variable("x"), alphabet_letter(3)],
        ))
        .m;
        assert!(accepts(&m, 2, 5, &[("x", 2)]));
        assert!(!accepts(&m, 2, 5, &[("x", 3)]));
    }

    /// `RelationalOperator.act`'s final `else` (`:172-174`). The message text is
    /// `error*`-fixture material, so it is pinned verbatim.
    #[test]
    fn relational_rejects_automaton_operands_with_walnuts_exact_message() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let mut stack = vec![
            Expression::Automaton(AutomatonExpression::new("x=1", Automaton::true_false(true))),
            Expression::Automaton(AutomatonExpression::new("y=2", Automaton::true_false(true))),
        ];
        let err = operator
            .act(&mut FreshIdentifiers::new(), &mut stack)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "operator < cannot be applied to operands x=1 and y=2 of types \
             Main.EvalComputations.Expressions.AutomatonExpression and \
             Main.EvalComputations.Expressions.AutomatonExpression respectively"
        );
    }

    #[test]
    fn relational_validates_arity_before_popping() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let mut stack = vec![variable("x")];
        let err = operator
            .act(&mut FreshIdentifiers::new(), &mut stack)
            .unwrap_err();
        assert_eq!(err.to_string(), "operator < requires 2 operands");
        assert_eq!(stack.len(), 1, "a failed act must not consume the stack");
    }

    // =====================================================================
    // ArithmeticOperator: constant folding + DIV's floor semantics
    // =====================================================================

    fn fold_arithmetic(op: &str, a: Expression, b: Expression, n: &Rc<NumberSystem>) -> BigInt {
        let operator = Operator::arithmetic(0, op, Rc::clone(n));
        let result = act_once(&operator, &mut FreshIdentifiers::new(), vec![a, b]);
        match result {
            Expression::NumberLiteral(ne) => {
                assert!(
                    Rc::ptr_eq(ne.base(), n),
                    "the folded literal takes the OPERATOR's number system"
                );
                ne.value().clone()
            }
            other => panic!("expected a folded NumberLiteralExpression, got {other:?}"),
        }
    }

    #[test]
    fn arithmetic_constant_folding_of_two_literals() {
        let n = ns("msd_2");
        assert_eq!(
            fold_arithmetic("+", number_literal(3, &n), number_literal(4, &n), &n),
            BigInt::from(7)
        );
        assert_eq!(
            fold_arithmetic("-", number_literal(9, &n), number_literal(4, &n), &n),
            BigInt::from(5)
        );
        assert_eq!(
            fold_arithmetic("*", number_literal(6, &n), number_literal(7, &n), &n),
            BigInt::from(42)
        );
    }

    /// **This layer's** use of `ArithmeticOp::arith_big_int` must round toward NEGATIVE
    /// INFINITY, not toward zero — checked end-to-end through `act()` on `@`-letter
    /// operands (the only constants that can be negative without first going through
    /// unary minus). `-7 / 2` is `-4` in Walnut, not the `-3` that Java's and Rust's
    /// native `/` would give.
    #[test]
    fn arithmetic_div_floors_toward_negative_infinity_through_act() {
        let n = ns("msd_2");
        for (a, b, expected) in [
            (7i32, 2i32, 3i64),
            (-7, 2, -4),
            (7, -2, -4),
            (-7, -2, 3),
            // Exact division: no floor correction even though the signs differ.
            (-8, 2, -4),
            (8, -2, -4),
            (0, 5, 0),
            (-1, 5, -1),
        ] {
            assert_eq!(
                fold_arithmetic("/", alphabet_letter(a), alphabet_letter(b), &n),
                BigInt::from(expected),
                "floor({a} / {b})"
            );
        }
    }

    /// The same floor rounding reached from a genuinely negative NUMBER LITERAL (via the
    /// unary-minus path) rather than an `@`-letter: `(_7) / 2 == -4`.
    #[test]
    fn arithmetic_div_floors_a_negated_number_literal() {
        let n = ns("msd_2");
        let negate = Operator::arithmetic(0, "_", Rc::clone(&n));
        let negated = act_once(
            &negate,
            &mut FreshIdentifiers::new(),
            vec![number_literal(7, &n)],
        );
        assert_eq!(negated.to_string(), "-7");
        assert_eq!(
            fold_arithmetic("/", negated, number_literal(2, &n), &n),
            BigInt::from(-4)
        );
    }

    #[test]
    fn arithmetic_constant_folding_division_by_zero_is_walnuts_error() {
        let n = ns("msd_2");
        let operator = Operator::arithmetic(0, "/", Rc::clone(&n));
        let mut stack = vec![number_literal(5, &n), number_literal(0, &n)];
        let err = operator
            .act(&mut FreshIdentifiers::new(), &mut stack)
            .unwrap_err();
        assert!(matches!(
            err,
            ActError::NumberSystem(NumSysError::DivisionByZero)
        ));
        assert_eq!(err.to_string(), "division by zero");
    }

    /// Constant folding pushes the raw `BigInteger` into a new literal with no
    /// `intValueExact` step, so a product far outside `i32` must survive intact — the
    /// `ArithmeticIntOverflow` that `ArithmeticOp::arith`'s `int` form raises must NOT be
    /// reachable from here.
    #[test]
    fn arithmetic_constant_folding_never_narrows_to_i32() {
        let n = ns("msd_2");
        let big = BigInt::from(2).pow(40);
        assert_eq!(
            fold_arithmetic(
                "*",
                big_literal(big.clone(), &n),
                big_literal(big.clone(), &n),
                &n
            ),
            BigInt::from(2).pow(80)
        );
    }

    // =====================================================================
    // ArithmeticOperator: unary minus (`processUnaryOperator`, `:100-123`)
    // =====================================================================

    #[test]
    fn unary_minus_negates_an_alphabet_letter() {
        let n = ns("msd_2");
        let operator = Operator::arithmetic(0, "_", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![alphabet_letter(1)],
        );
        match &result {
            Expression::AlphabetLetter(ae) => assert_eq!(ae.constant, -1),
            other => panic!("expected an AlphabetLetterExpression, got {other:?}"),
        }
        assert_eq!(result.to_string(), "@-1");
    }

    /// A word automaton's per-state OUTPUTS are negated in place (`0 - output`, via
    /// `Ops.MINUS` — `ArithmeticOp::UnaryNegative` is exactly the value
    /// `ArithmeticOp::arith` refuses, so the rewrite is load-bearing), and the same
    /// `WordExpression` is pushed back.
    #[test]
    fn unary_minus_negates_a_word_automatons_outputs_in_place() {
        let n = ns("msd_2");
        let operator = Operator::arithmetic(0, "_", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![word_expression("T[i]", thue_morse("i"))],
        );
        match &result {
            Expression::Word(we) => {
                let mut outputs = we.word_automaton.fa.o.clone();
                outputs.sort_unstable();
                assert_eq!(outputs, vec![-1, 0], "outputs {{0,1}} negate to {{0,-1}}");
            }
            other => panic!("expected a WordExpression, got {other:?}"),
        }
        // Ported quirk: the displayed expression is NOT updated.
        assert_eq!(result.to_string(), "T[i]");
    }

    /// The general arm: `_x` becomes an [`ArithmeticExpression`] over a fresh `c` with
    /// `x + c = 0`. Over a positive base that pins `x == c == 0`, which is exactly what
    /// unary minus of a variable means in Walnut there.
    #[test]
    fn unary_minus_of_a_variable_builds_x_plus_c_equals_zero() {
        let n = ns("msd_2");
        let operator = Operator::arithmetic(0, "_", Rc::clone(&n));
        let mut fresh = FreshIdentifiers::new();
        let result = act_once(&operator, &mut fresh, vec![variable("x")]);
        assert_eq!(fresh.issued(), 1, "exactly one synthetic name is minted");
        assert_eq!(result.to_string(), "(_x)");
        let ae = match result {
            Expression::Arithmetic(ae) => ae,
            other => panic!("expected an ArithmeticExpression, got {other:?}"),
        };
        let c = ae.identifier.clone();
        assert!(c.starts_with(FreshIdentifiers::PREFIX));
        assert!(accepts(&ae.m, 2, 4, &[("x", 0), (&c, 0)]));
        assert!(!accepts(&ae.m, 2, 4, &[("x", 1), (&c, 1)]));
        assert!(!accepts(&ae.m, 2, 4, &[("x", 1), (&c, 0)]));
    }

    // =====================================================================
    // Fresh-variable generation + quantification
    // =====================================================================

    /// The headline behavior for this unit: a nested arithmetic sub-expression mints a
    /// synthetic temporary, and the ENCLOSING relational operator must quantify it away
    /// in the same `act()` — via `Operator.andThenQuantifyIfArithmetic`.
    ///
    /// `x + y = z` must therefore end up as a three-track relation over exactly
    /// `{x, y, z}` (no `WALNUT_…` track surviving), denoting real addition.
    #[test]
    fn nested_arithmetic_subexpression_is_quantified_away_by_the_enclosing_relation() {
        let n = ns("msd_2");
        let mut fresh = FreshIdentifiers::new();
        let mut stack = vec![variable("x"), variable("y")];

        Operator::arithmetic(0, "+", Rc::clone(&n))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(fresh.issued(), 1, "`x+y` mints exactly one temporary");
        let temporary = match &stack[0] {
            Expression::Arithmetic(ae) => ae.identifier.clone(),
            other => panic!("expected an ArithmeticExpression, got {other:?}"),
        };
        assert_eq!(stack[0].to_string(), "(x+y)");
        assert!(temporary.starts_with(FreshIdentifiers::PREFIX));

        stack.push(variable("z"));
        Operator::relational(0, "=", Rc::clone(&n))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(stack.len(), 1);
        let m = as_automaton_expression(stack.pop().unwrap()).m;

        let mut labels = m.label.clone();
        labels.sort();
        assert_eq!(
            labels,
            vec!["x".to_string(), "y".to_string(), "z".to_string()],
            "the synthetic temporary {temporary} must have been quantified away"
        );

        for (x, y, z) in [(0u32, 0u32, 0u32), (1, 2, 3), (3, 4, 7), (5, 2, 7)] {
            assert!(
                accepts(&m, 2, 5, &[("x", x), ("y", y), ("z", z)]),
                "{x} + {y} == {z} must be accepted"
            );
        }
        for (x, y, z) in [(1u32, 2u32, 4u32), (3, 4, 6), (0, 0, 1)] {
            assert!(
                !accepts(&m, 2, 5, &[("x", x), ("y", y), ("z", z)]),
                "{x} + {y} != {z} must be rejected"
            );
        }

        // Padding-independent, because `quantify` applies the msd leading-zero fixup
        // after eliminating the temporary. If that fixup had been skipped, the same
        // tuple would be accepted at one width and rejected at another.
        for width in [2usize, 3, 8, 12] {
            assert!(
                accepts(&m, 2, width, &[("x", 1), ("y", 2), ("z", 3)]),
                "1 + 2 == 3 must be accepted at padding width {width}"
            );
        }
    }

    /// Two nested levels: `(x + y) + w = z`. Both temporaries must vanish, and the
    /// counter must have advanced exactly twice.
    #[test]
    fn two_nested_arithmetic_temporaries_are_both_quantified_away() {
        let n = ns("msd_2");
        let mut fresh = FreshIdentifiers::new();
        let mut stack = vec![variable("x"), variable("y")];
        Operator::arithmetic(0, "+", Rc::clone(&n))
            .act(&mut fresh, &mut stack)
            .unwrap();
        stack.push(variable("w"));
        Operator::arithmetic(0, "+", Rc::clone(&n))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(fresh.issued(), 2);
        assert_eq!(stack[0].to_string(), "((x+y)+w)");

        stack.push(variable("z"));
        Operator::relational(0, "=", Rc::clone(&n))
            .act(&mut fresh, &mut stack)
            .unwrap();
        let m = as_automaton_expression(stack.pop().unwrap()).m;
        let mut labels = m.label.clone();
        labels.sort();
        assert_eq!(
            labels,
            vec![
                "w".to_string(),
                "x".to_string(),
                "y".to_string(),
                "z".to_string()
            ]
        );
        assert!(accepts(&m, 2, 5, &[("x", 1), ("y", 2), ("w", 4), ("z", 7)]));
        assert!(!accepts(
            &m,
            2,
            5,
            &[("x", 1), ("y", 2), ("w", 4), ("z", 6)]
        ));
    }

    /// The constant-operand arm of the same story: `x + 1 = z`.
    #[test]
    fn arithmetic_with_a_constant_operand_then_quantified_away() {
        let n = ns("msd_2");
        let mut fresh = FreshIdentifiers::new();
        let mut stack = vec![variable("x"), number_literal(1, &n)];
        Operator::arithmetic(0, "+", Rc::clone(&n))
            .act(&mut fresh, &mut stack)
            .unwrap();
        assert_eq!(stack[0].to_string(), "(x+1)");
        stack.push(variable("z"));
        Operator::relational(0, "=", Rc::clone(&n))
            .act(&mut fresh, &mut stack)
            .unwrap();
        let m = as_automaton_expression(stack.pop().unwrap()).m;
        let mut labels = m.label.clone();
        labels.sort();
        assert_eq!(labels, vec!["x".to_string(), "z".to_string()]);
        assert!(accepts(&m, 2, 5, &[("x", 2), ("z", 3)]));
        assert!(!accepts(&m, 2, 5, &[("x", 2), ("z", 4)]));
    }

    /// `and_then_quantify_if_arithmetic` must be a strict no-op for every operand kind
    /// that is not an [`Expression::Arithmetic`] — otherwise it would quantify tracks out
    /// of an unrelated automaton.
    #[test]
    fn and_then_quantify_if_arithmetic_is_a_no_op_for_non_arithmetic_operands() {
        let m = thue_morse("i");
        for operand in [
            variable("x"),
            alphabet_letter(1),
            number_literal(3, &ns("msd_2")),
            word_expression("T[i]", thue_morse("i")),
        ] {
            let out = and_then_quantify_if_arithmetic(&operand, m.clone()).unwrap();
            assert_eq!(out.label, m.label);
            assert_eq!(out.fa.q, m.fa.q);
            assert_eq!(out.fa.o, m.fa.o);
        }
    }

    // =====================================================================
    // Word-automaton comparisons (`RelationalOperator.act` arms 1, 5, 6, 7)
    // =====================================================================

    /// Arm 6: `T[i] = @1` over Thue–Morse accepts exactly the odd-popcount indices.
    #[test]
    fn word_vs_constant_comparison_selects_the_matching_outputs() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "=", Rc::clone(&n));
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![word_expression("T[i]", thue_morse("i")), alphabet_letter(1)],
        ))
        .m;
        assert_eq!(m.label, vec!["i".to_string()]);
        for i in [1u32, 2, 4, 7, 8] {
            assert!(
                accepts(&m, 2, 5, &[("i", i)]),
                "T[{i}] should be 1 (popcount {} is odd)",
                i.count_ones()
            );
        }
        for i in [0u32, 3, 5, 6, 9] {
            assert!(
                !accepts(&m, 2, 5, &[("i", i)]),
                "T[{i}] should be 0 (popcount {} is even)",
                i.count_ones()
            );
        }
    }

    /// Arm 7: the constant on the LEFT reverses the relation, so `@0 < T[i]` must be
    /// `T[i] > 0`, i.e. again the odd-popcount indices. Without `reverse_operator()` this
    /// would compute `T[i] < 0` and accept nothing, so both outcomes are checked.
    #[test]
    fn constant_vs_word_comparison_reverses_the_relation() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![alphabet_letter(0), word_expression("T[i]", thue_morse("i"))],
        ))
        .m;
        assert!(accepts(&m, 2, 5, &[("i", 1)]), "0 < T[1] == 1");
        assert!(!accepts(&m, 2, 5, &[("i", 3)]), "0 < T[3] == 0 is false");
        assert!(!accepts(&m, 2, 5, &[("i", 0)]), "0 < T[0] == 0 is false");
    }

    /// Arm 5: `T[i] = U[i]` where `U` is the constant-`1` DFAO — again the odd-popcount
    /// indices, but reached through `compareWordAutomata`'s cross product rather than the
    /// per-state `compareWordAutomaton` loop.
    #[test]
    fn word_vs_word_comparison_compares_outputs_pointwise() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "=", Rc::clone(&n));
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![
                word_expression("T[i]", thue_morse("i")),
                word_expression("U[i]", always_one("i")),
            ],
        ))
        .m;
        assert_eq!(m.label, vec!["i".to_string()]);
        assert!(accepts(&m, 2, 5, &[("i", 1)]));
        assert!(accepts(&m, 2, 5, &[("i", 2)]));
        assert!(!accepts(&m, 2, 5, &[("i", 0)]));
        assert!(!accepts(&m, 2, 5, &[("i", 3)]));
    }

    /// Arm 1, the word-vs-arithmetic rewrite: `T[i] < x` becomes
    /// `(T[i] = @0 => 0 < x) & (T[i] = @1 => 1 < x)`, a two-track relation over `{i, x}`.
    #[test]
    fn word_vs_variable_comparison_rewrites_over_every_output() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![word_expression("T[i]", thue_morse("i")), variable("x")],
        ))
        .m;
        let mut labels = m.label.clone();
        labels.sort();
        assert_eq!(labels, vec!["i".to_string(), "x".to_string()]);
        // T[0] == 0, T[1] == 1.
        assert!(accepts(&m, 2, 5, &[("i", 0), ("x", 1)]), "T[0]=0 < 1");
        assert!(!accepts(&m, 2, 5, &[("i", 0), ("x", 0)]), "0 < 0 is false");
        assert!(accepts(&m, 2, 5, &[("i", 1), ("x", 2)]), "T[1]=1 < 2");
        assert!(!accepts(&m, 2, 5, &[("i", 1), ("x", 1)]), "1 < 1 is false");
    }

    /// The mirror direction of arm 1 (`reverse = true`): `x < T[i]`.
    #[test]
    fn variable_vs_word_comparison_keeps_the_operand_order() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let m = as_automaton_expression(act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![variable("x"), word_expression("T[i]", thue_morse("i"))],
        ))
        .m;
        assert!(accepts(&m, 2, 5, &[("i", 1), ("x", 0)]), "0 < T[1] == 1");
        assert!(!accepts(&m, 2, 5, &[("i", 1), ("x", 1)]), "1 < 1 is false");
        assert!(!accepts(&m, 2, 5, &[("i", 0), ("x", 0)]), "0 < 0 is false");
    }

    /// WB-023 (`docs/WALNUT-BUGS.md`): arm 1 alone labels its result with just
    /// `word.toString()`, dropping the operator and the other operand — every sibling arm
    /// uses `a + op + b`. Ported verbatim; this pins the (wrong) Walnut text.
    #[test]
    fn wb023_word_vs_arithmetic_result_string_drops_the_operator_and_operand() {
        let n = ns("msd_2");
        let operator = Operator::relational(0, "<", Rc::clone(&n));
        let result = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![word_expression("T[i]", thue_morse("i")), variable("x")],
        );
        assert_eq!(
            result.to_string(),
            "T[i]",
            "WB-023: Walnut drops '<x' here; a corrected port would say 'T[i]<x'"
        );

        // Every sibling arm DOES include the operator and both operands — the contrast
        // that makes this a defect rather than a house style.
        let sibling = act_once(
            &operator,
            &mut FreshIdentifiers::new(),
            vec![word_expression("T[i]", thue_morse("i")), alphabet_letter(1)],
        );
        assert_eq!(sibling.to_string(), "T[i]<@1");
    }

    // =====================================================================
    // Word-automaton arithmetic (`processBinaryOperator` arms 0, 1, 2, 4)
    // =====================================================================

    /// Arm 1 (`reverse = true`) computes `output - 1`; arm 2 (`reverse = false`) computes
    /// `1 - output`. The two differ, so this is the test that catches a flipped `reverse`.
    #[test]
    fn word_arithmetic_against_a_constant_respects_operand_order() {
        let n = ns("msd_2");
        let minus = Operator::arithmetic(0, "-", Rc::clone(&n));

        let result = act_once(
            &minus,
            &mut FreshIdentifiers::new(),
            vec![word_expression("T[i]", thue_morse("i")), alphabet_letter(1)],
        );
        let mut outputs = match &result {
            Expression::Word(we) => we.word_automaton.fa.o.clone(),
            other => panic!("expected a WordExpression, got {other:?}"),
        };
        outputs.sort_unstable();
        assert_eq!(outputs, vec![-1, 0], "T[i] - 1 maps {{0,1}} to {{-1,0}}");

        let result = act_once(
            &minus,
            &mut FreshIdentifiers::new(),
            vec![alphabet_letter(1), word_expression("T[i]", thue_morse("i"))],
        );
        let mut outputs = match &result {
            Expression::Word(we) => we.word_automaton.fa.o.clone(),
            other => panic!("expected a WordExpression, got {other:?}"),
        };
        outputs.sort_unstable();
        assert_eq!(outputs, vec![0, 1], "1 - T[i] maps {{0,1}} to {{1,0}}");
    }

    /// Arm 0: two word automata combine pointwise, `a` is mutated and re-pushed, and
    /// `b`'s pending-quantification list is appended to `a`'s.
    #[test]
    fn word_plus_word_combines_outputs_and_merges_quantification_lists() {
        let n = ns("msd_2");
        let plus = Operator::arithmetic(0, "+", Rc::clone(&n));
        let left = Expression::Word(Box::new(WordExpression::new(
            "T[i]",
            thue_morse("i"),
            Automaton::true_false(true),
            vec!["tmpA".to_string()],
        )));
        let right = Expression::Word(Box::new(WordExpression::new(
            "U[i]",
            always_one("i"),
            Automaton::true_false(true),
            vec!["tmpB".to_string()],
        )));
        let result = act_once(&plus, &mut FreshIdentifiers::new(), vec![left, right]);
        match &result {
            Expression::Word(we) => {
                let mut outputs = we.word_automaton.fa.o.clone();
                outputs.sort_unstable();
                assert_eq!(outputs, vec![1, 2], "{{0,1}} + 1 == {{1,2}}");
                assert_eq!(
                    we.identifiers_to_quantify,
                    vec!["tmpA".to_string(), "tmpB".to_string()]
                );
            }
            other => panic!("expected a WordExpression, got {other:?}"),
        }
        // Ported quirk: the displayed expression is still just the LEFT operand's.
        assert_eq!(result.to_string(), "T[i]");
    }

    /// Arm 4, including the `o == 0 && MULT` special case (`:183-185`): `T[i] * x` yields
    /// `c = T[i] * x`, which for a Thue–Morse `T` means `c == 0` on even-popcount indices
    /// and `c == x` on odd ones.
    #[test]
    fn word_times_variable_rewrites_per_output_including_the_zero_shortcut() {
        let n = ns("msd_2");
        let times = Operator::arithmetic(0, "*", Rc::clone(&n));
        let mut fresh = FreshIdentifiers::new();
        let result = act_once(
            &times,
            &mut fresh,
            vec![word_expression("T[i]", thue_morse("i")), variable("x")],
        );
        assert_eq!(fresh.issued(), 1);
        assert_eq!(result.to_string(), "(T[i]*x)");
        let ae = match result {
            Expression::Arithmetic(ae) => ae,
            other => panic!("expected an ArithmeticExpression, got {other:?}"),
        };
        let c = ae.identifier.clone();
        let mut labels = ae.m.label.clone();
        labels.sort();
        let mut expected = vec!["i".to_string(), "x".to_string(), c.clone()];
        expected.sort();
        assert_eq!(labels, expected);

        // i = 1: T[1] == 1, so c == x.
        assert!(accepts(&ae.m, 2, 5, &[("i", 1), ("x", 3), (&c, 3)]));
        assert!(!accepts(&ae.m, 2, 5, &[("i", 1), ("x", 3), (&c, 2)]));
        // i = 3: T[3] == 0, so c == 0 whatever x is — the `o == 0 && MULT` arm.
        assert!(accepts(&ae.m, 2, 5, &[("i", 3), ("x", 3), (&c, 0)]));
        assert!(!accepts(&ae.m, 2, 5, &[("i", 3), ("x", 3), (&c, 3)]));
    }

    // =====================================================================
    // Errors and pinned quirks
    // =====================================================================

    #[test]
    fn arithmetic_rejects_an_automaton_operand_with_walnuts_exact_message() {
        let n = ns("msd_2");
        let plus = Operator::arithmetic(0, "+", Rc::clone(&n));

        // Rejected as the RIGHT operand (`act`, `:91-92`).
        let mut stack = vec![
            variable("x"),
            Expression::Automaton(AutomatonExpression::new("y=1", Automaton::true_false(true))),
        ];
        let err = plus
            .act(&mut FreshIdentifiers::new(), &mut stack)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "operator + cannot be applied to the operand y=1 of type \
             Main.EvalComputations.Expressions.AutomatonExpression"
        );

        // ... and as the LEFT operand (`processBinaryOperator`, `:127-128`).
        let mut stack = vec![
            Expression::Automaton(AutomatonExpression::new("y=1", Automaton::true_false(true))),
            variable("x"),
        ];
        let err = plus
            .act(&mut FreshIdentifiers::new(), &mut stack)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "operator + cannot be applied to the operand y=1 of type \
             Main.EvalComputations.Expressions.AutomatonExpression"
        );
    }

    /// `getIntConstantForWord`'s overflow path. The wording differs between the two
    /// classes ("comparison" vs "arithmetic"), so both are pinned.
    #[test]
    fn a_number_literal_too_large_for_a_word_output_reports_walnuts_message() {
        let n = ns("msd_2");
        let huge = BigInt::from(i64::MAX);

        let eq = Operator::relational(0, "=", Rc::clone(&n));
        let mut stack = vec![
            word_expression("T[i]", thue_morse("i")),
            big_literal(huge.clone(), &n),
        ];
        let err = eq
            .act(&mut FreshIdentifiers::new(), &mut stack)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "number literal {huge} used in word automaton output comparison must fit in a \
                 Java int, found: {huge}"
            )
        );

        let plus = Operator::arithmetic(0, "+", Rc::clone(&n));
        let mut stack = vec![
            word_expression("T[i]", thue_morse("i")),
            big_literal(huge.clone(), &n),
        ];
        let err = plus
            .act(&mut FreshIdentifiers::new(), &mut stack)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "number literal {huge} used in word automaton output arithmetic must fit in a \
                 Java int, found: {huge}"
            )
        );
    }

    /// WB-003 (`docs/WALNUT-BUGS.md`): `0 * x` short-circuits to the literal `0` without
    /// ever building or validating an automaton for `x`. Ported verbatim — including the
    /// synthetic name minted at `ArithmeticOperator.java:155` and then thrown away.
    #[test]
    fn wb003_zero_times_a_variable_short_circuits_and_wastes_a_fresh_name() {
        let n = ns("msd_2");
        let times = Operator::arithmetic(0, "*", Rc::clone(&n));

        for operands in [
            vec![number_literal(0, &n), variable("never_declared")],
            vec![variable("never_declared"), number_literal(0, &n)],
            vec![alphabet_letter(0), variable("never_declared")],
            vec![variable("never_declared"), alphabet_letter(0)],
        ] {
            let mut fresh = FreshIdentifiers::new();
            let result = act_once(&times, &mut fresh, operands);
            match &result {
                Expression::NumberLiteral(ne) => {
                    assert_eq!(*ne.value(), BigInt::from(0));
                    assert!(Rc::ptr_eq(ne.base(), &n));
                }
                other => panic!("expected the folded literal 0, got {other:?}"),
            }
            assert_eq!(
                fresh.issued(),
                1,
                "WB-003: `c` is minted at :155 before the short-circuit discards it"
            );
        }
    }

    /// `*` between a NON-zero constant and a variable takes the ordinary path and does
    /// build a real automaton — the contrast that keeps the WB-003 test above honest.
    #[test]
    fn a_nonzero_constant_times_a_variable_builds_a_real_automaton() {
        let n = ns("msd_2");
        let times = Operator::arithmetic(0, "*", Rc::clone(&n));
        let mut fresh = FreshIdentifiers::new();
        let result = act_once(
            &times,
            &mut fresh,
            vec![number_literal(3, &n), variable("x")],
        );
        let ae = match result {
            Expression::Arithmetic(ae) => ae,
            other => panic!("expected an ArithmeticExpression, got {other:?}"),
        };
        let c = ae.identifier.clone();
        assert!(accepts(&ae.m, 2, 5, &[("x", 4), (&c, 12)]));
        assert!(!accepts(&ae.m, 2, 5, &[("x", 4), (&c, 11)]));
    }

    /// Every non-relational, non-arithmetic operator kind still falls through to the
    /// inherited no-op (`LogicalOperator.act` is U10) — pinned so U10's author sees this
    /// test flip rather than silently changing behavior.
    #[test]
    fn logical_operator_kinds_are_still_a_no_op_pending_u10() {
        let operators = [
            Operator::logical_connective(0, "&"),
            Operator::logical_connective(0, "~"),
            Operator::logical_connective(0, "`"),
            Operator::quantifier(0, "E", 1),
            Operator::left_paren(0),
            Operator::right_paren(0),
        ];
        for operator in operators {
            let mut fresh = FreshIdentifiers::new();
            let mut stack = vec![variable("x"), variable("y")];
            operator.act(&mut fresh, &mut stack).unwrap();
            assert_eq!(stack.len(), 2, "{operator} must not touch the stack yet");
            assert_eq!(fresh.issued(), 0);
        }
    }
}
