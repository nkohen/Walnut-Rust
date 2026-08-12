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
use wr_core::logicalops::and;
use wr_core::numsys::{ArithmeticOp, NumberSystem, RelationalOp};
use wr_core::quantify::QuantifyError;

use crate::expr::{
    AlphabetLetterExpression, AutomatonExpression, ExprError, Expression, NumberLiteralExpression,
    VariableExpression, WordExpression,
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
    /// `Variable`/`NumberLiteral`/`AlphabetLetter` (U2) and, as of U4, by `Word`/
    /// `Function` (`Word.java:50-79`, `Function.java`'s own `act`). `Operator` itself
    /// never overrides the inherited no-op directly — only its `LogicalOperator`/
    /// `RelationalOperator`/`ArithmeticOperator` subclasses do (deferred to U9/U10), so
    /// [`Token::Operator`] still falls through to `Ok(())` here.
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
            Token::Operator(_) => {}
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
}
