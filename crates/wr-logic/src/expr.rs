// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Expressions/Expression.java` (77 LOC) + its six subclasses (343 LOC total) —
//! the value objects the shunting-yard evaluator's operand stack holds, ported as
//! Phase 3a's U2.
//!
//! # One `enum`, not seven classes sharing one bag of nullable fields
//!
//! Java's `Expression` is a single concrete class with FOUR fields (`M`, `identifier`,
//! `constant`, `wordAutomaton`) that every subclass's constructor populates a *subset*
//! of, leaving the rest at Java's defaults (`null`/`0`). Every real read of those fields
//! is preceded by an `instanceof` check that narrows to a CLOSED SET of concrete
//! subclasses — usually exactly one, but sometimes a small disjunction of two or three
//! subclasses that all set the same field — before touching the field: never a *fully*
//! generic base-type read with no narrowing at all (verified across every call site in
//! `RelationalOperator`/`ArithmeticOperator`/`LogicalOperator`/`Word`/`Function` —
//! U9/U10's future territory, read ahead for exactly this reason). For example
//! `RelationalOperator.act` reads `a.identifier`/`b.identifier` after narrowing `a`/`b`
//! to `(ArithmeticExpression | VariableExpression)` (`RelationalOperator.java:135-137`,
//! `:124/126`'s `arithmetic.identifier` after the same two-way narrowing under a
//! `WordExpression`-vs-the-rest split), and `ArithmeticOperator.act`/`getIntConstantForWord`
//! read `e.constant` on a bare `Expression`-typed parameter after only a
//! `NumberLiteralExpression` early-return, i.e. narrowed to "anything else `isValidArithmeticOperator`
//! accepts" rather than one named subclass (`ArithmeticOperator.java:226`, `:264`, `:275`).
//! Nothing ever reads `identifier` off a value it knows is only ever an
//! `AutomatonExpression`, or `wordAutomaton` off a value it knows is only ever a
//! `VariableExpression` — so the four-nullable-fields-on-one-class shape still carries no
//! information any reader needs, and the Rust translation of an `instanceof`-narrowed
//! access is still a `match` arm (or, where the narrowing is a disjunction rather than a
//! single variant, an accessor like [`Expression::identifier`]/[`Expression::constant`]
//! that returns `Option` over exactly the variants that set the field) that already has
//! the right fields in scope — exactly what a sum type gives for free. This is the same
//! "prove the untaken state is unreachable AND unread, then use a precise type"
//! methodology `PORTING.md` already applies to `FA.TRUE_FALSE_AUTOMATON`/`TRUE_AUTOMATON`,
//! generalized to the first Java-`instanceof`-hierarchy this port has hit (a pattern
//! [`crate::token`] repeats for `Token`/`Operator`) — recorded as a `PORTING.md` ruling
//! alongside this unit (see that ruling's own text for why "narrows to one concrete
//! subclass" was corrected to "narrows to a closed disjunction of subclasses" after
//! adversarial review).
//!
//! # What's implemented here, and what's deferred (and why)
//!
//! Every concrete `act(...)` method below (`AutomatonExpression`, `ArithmeticExpression`,
//! `VariableExpression`, `NumberLiteralExpression`) is a *different* method from the
//! `Token::act` semantics deferred to U9/U10 — grep confirms all of Java's
//! `Expression::act(...)` overloads (besides the always-throwing base `act(String)`, ported
//! below) are called exclusively from `Word.java`/`Function.java` (U4 scope, not yet
//! ported), so nothing here is reachable from real code yet either — but nothing blocks
//! porting the *methods themselves* now:
//!
//! * [`NumberLiteralExpression::act`] (`NumberLiteralExpression.java:61-71`) was the one
//!   exception when U2 landed: it calls `this.base.getConstant(this.value)`, and
//!   `NumberSystem::get_constant` took `&mut self` at the time, while this crate's
//!   [`crate::predicate_env::PredicateEnv`] deliberately hands out `Rc<NumberSystem>` —
//!   shared, never `&mut` — so there was no sound signature for it to have. **Phase 3a's
//!   U5 delivered `predicate_env.rs`'s Ruling 1** (the memo tables are `RefCell`s and
//!   `get_constant` takes `&self`), which is exactly the signature this needed, so the
//!   method is ported now rather than leaving a doc comment describing a blocker that no
//!   longer exists.
//! * Every other `act(...)` below replaces `Token.getUniqueString()` with
//!   `&mut `[`FreshIdentifiers`]`::`[`next_identifier`](FreshIdentifiers::next_identifier),
//!   per `predicate_env.rs`'s Ruling 4 — a `Token` parameter is never threaded through.
//! * `Logging.indent()`/`Logging.dedent()` calls around some of these bodies in Java are
//!   *not* ported here. Ruling 4 states explicitly that the logging context joins
//!   `PredicateEnv`/`FreshIdentifiers` as something U11's postfix-token executor threads
//!   through, not something each individual `Token`/`Expression::act` signature grows
//!   piecemeal. Threading it through two call sites now and the rest later would be a
//!   false start, not a head start.
//! * `Operator.andThenQuantifyIfArithmetic` (`Operator.java:150-158`) is **not** ported
//!   in this unit. It is act()-support exclusively for `RelationalOperator`/
//!   `ArithmeticOperator::act` (both deferred to U9) and needs `Logging::indent/dedent`
//!   (deferred per the point above) — porting it here would be dead code with no test
//!   able to exercise it honestly. U9 should port it alongside the operators that call it.

use std::fmt;
use std::rc::Rc;

use num_bigint::BigInt;
use wr_core::automaton::Automaton;
use wr_core::logicalops::and;
use wr_core::numsys::{NumSysError, NumberSystem};

use crate::predicate_env::FreshIdentifiers;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure an [`Expression`]'s own methods can report, module-local per this
/// crate's established idiom (see `predicate_env.rs`'s `PredicateEnvError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    /// `AutomatonExpression.act`'s first check (`AutomatonExpression.java:33-35`):
    /// `"argument " + (i + 1) + " of function " + name + " cannot be an automaton
    /// with != 1 inputs"`. `position` is already `i + 1` (1-based, matching Java's
    /// message text, not the 0-based index callers pass in).
    AutomatonArgumentWrongArity { position: usize, name: String },
    /// `AutomatonExpression.act`'s second check (`:36-38`): `"argument " + (i + 1)
    /// + " of function " + name + " cannot be an automaton with unlabeled input"`.
    AutomatonArgumentUnlabeled { position: usize, name: String },
    /// The base `Expression.act(String)` fallback (`Expression.java:74-76`):
    /// `begin + " cannot be of type " + this.getClass().getName()`. `java_class_name`
    /// is the fully-qualified Java name (`Expression::java_class_name`), preserved
    /// verbatim because `error*` golden fixtures compare this text eventually.
    InvalidType {
        context: String,
        java_class_name: &'static str,
    },
    /// `VariableExpression.act`'s repeated-identifier branch (`VariableExpression.java:38`):
    /// `ns.equality.clone()` on a `null` `ns`. A real Java `NullPointerException`, not a
    /// `WalnutException` — reachable whenever the SAME variable indexes an
    /// explicit-alphabet-declared track (`Word.java:62`: `wordAutomaton.getNS().get(i)`
    /// is `null` for any `{...}`-declared track, per `ParseMethods.parseAlphabetDeclaration`
    /// `bases.add(null)`) more than once in one `eval`/`def` query — e.g. `T[i][i] = @1`
    /// where `T`'s second track is declared `{0,1}` rather than `msd_k`/`lsd_k`. Logged as
    /// WB-013 (`docs/WALNUT-BUGS.md`). Represented as a `Result::Err` here, deliberately
    /// **not** a `panic!`: Java's own NPE is an unchecked `RuntimeException` that
    /// `Prover.dispatch`'s top-level `catch (RuntimeException e)` recovers from (prints a
    /// stack trace, the session continues) — an uncaught Rust `panic!` here would unwind
    /// and, absent a `catch_unwind` boundary this port doesn't have yet, kill the whole
    /// process instead, the opposite of Java's actual behavior. (Same argument
    /// `wr_core::logging`'s module doc makes for `dedent()`'s `IllegalArgumentException`,
    /// applied here to a different unchecked-exception call site.)
    RepeatedIdentifierMissingNumberSystem { identifier: String },
    /// Propagated out of `this.base.getConstant(this.value)` in
    /// [`NumberLiteralExpression::act`] (`NumberLiteralExpression.java:62`). Java lets the
    /// `WalnutException` from `NumberSystem` escape unchanged; this wraps it so the caller
    /// still gets one error type, and [`fmt::Display`] renders the inner Java text verbatim
    /// (`wr_core::numsys::NumSysError` gained a `Display` in the same unit).
    NumberSystem(NumSysError),
}

impl From<NumSysError> for ExprError {
    fn from(e: NumSysError) -> Self {
        ExprError::NumberSystem(e)
    }
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprError::AutomatonArgumentWrongArity { position, name } => write!(
                f,
                "argument {position} of function {name} cannot be an automaton with != 1 inputs"
            ),
            ExprError::AutomatonArgumentUnlabeled { position, name } => write!(
                f,
                "argument {position} of function {name} cannot be an automaton with unlabeled input"
            ),
            ExprError::InvalidType {
                context,
                java_class_name,
            } => write!(f, "{context} cannot be of type {java_class_name}"),
            ExprError::RepeatedIdentifierMissingNumberSystem { identifier } => write!(
                f,
                "variable {identifier} was repeated under a track with no attached number \
                 system (Java: NullPointerException in VariableExpression.act, see WB-013)"
            ),
            ExprError::NumberSystem(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExprError {}

// ---------------------------------------------------------------------------
// The Expression sum type
// ---------------------------------------------------------------------------

/// `Main/EvalComputations/Expressions/Expression.java` (77 LOC) — see the module docs
/// for why this is a sum type rather than a mirror of Java's single base class.
///
/// Java's own doc comment (kept, lightly trimmed) on what each kind means:
/// - **automaton**: an expression with at least one reverse/logical/comparison
///   operator (`&`,`|`,`^`,`~`,`` ` ``,`E`,`A`,`=`,`!=`,`<`,`>`,`<=`,`>=`). Evaluates to
///   an automaton, e.g. `a+b>c` accepts iff `a+b>c`.
/// - **arithmetic**: has at least one of `+ - * /` and no operators of other kinds,
///   e.g. `a+b-c*2`. Evaluates to an automaton (in [`ArithmeticExpression::m`]) plus a
///   fresh identifier `x` such that the automaton accepts iff `x` equals the
///   expression's value; `x` must later be quantified away.
/// - **variable**: e.g. `a`, `b` in `a+2=b`. Its value is its name
///   ([`VariableExpression::identifier`]).
/// - **numberLiteral**: any positive integer that is not an alphabetLetter, e.g. the
///   `2` in `a+2=b`. Stores the value AND the number system it belongs to.
/// - **alphabetLetter**: an integer following `@`, e.g. `@-1` in `W[a] = @-1`
///   evaluates to `-1`.
/// - **word**: a `[]`-indexed name, e.g. `Thue[a+b]`. Evaluates to two automata: one
///   for `Thue[x]` (a fresh variable `x`), stored in
///   [`WordExpression::word_automaton`], and one for the arithmetic sub-expression
///   `x = a+b` (stored in [`WordExpression::m`]), since `x` must be eliminated later
///   via [`WordExpression::identifiers_to_quantify`].
#[derive(Debug, Clone)]
pub enum Expression {
    Automaton(AutomatonExpression),
    Arithmetic(ArithmeticExpression),
    Variable(VariableExpression),
    NumberLiteral(NumberLiteralExpression),
    AlphabetLetter(AlphabetLetterExpression),
    /// Boxed, unlike its five siblings: [`WordExpression`] holds TWO [`Automaton`]s where
    /// the next-largest variant holds one, and U5's addition of a per-track field to
    /// `Automaton` pushed that gap past `clippy::large_enum_variant`'s threshold. Pure
    /// indirection — `PORTING.md`'s "one enum variant per concrete subclass" ruling is
    /// unaffected, and nothing about the variant's meaning changes.
    Word(Box<WordExpression>),
}

impl Expression {
    /// `this.getClass().getName()` as read by [`ExprError::InvalidType`] and (later,
    /// U9) `WalnutException.invalidOperator`/`invalidDualOperators`, which both print
    /// the fully-qualified Java class name. Recorded here once so nothing downstream
    /// has to re-derive these literal strings.
    pub fn java_class_name(&self) -> &'static str {
        match self {
            Expression::Automaton(_) => "Main.EvalComputations.Expressions.AutomatonExpression",
            Expression::Arithmetic(_) => "Main.EvalComputations.Expressions.ArithmeticExpression",
            Expression::Variable(_) => "Main.EvalComputations.Expressions.VariableExpression",
            Expression::NumberLiteral(_) => {
                "Main.EvalComputations.Expressions.NumberLiteralExpression"
            }
            Expression::AlphabetLetter(_) => {
                "Main.EvalComputations.Expressions.AlphabetLetterExpression"
            }
            Expression::Word(_) => "Main.EvalComputations.Expressions.WordExpression",
        }
    }

    /// The Rust translation of an `instanceof (ArithmeticExpression | VariableExpression)`
    /// narrowing followed by a bare `.identifier` read, e.g. `RelationalOperator.act`'s
    /// `ns.comparison(a.identifier, b.identifier, opp)` (`RelationalOperator.java:137`)
    /// after its `(a instanceof ArithmeticExpression || a instanceof VariableExpression)`
    /// guard (`:135-136`), or `:124/126`'s `arithmetic.identifier` read on an `Expression`-
    /// typed local that the surrounding `if` has already narrowed the same way. `None`
    /// for every other variant, matching those call sites never being reached for them
    /// (verified: nothing in `RelationalOperator`/`ArithmeticOperator::act` reads
    /// `.identifier` off a value it hasn't already narrowed to this two-variant set).
    /// Added ahead of U9 (which ports those `act()` bodies) per the module docs' note on
    /// this file's own `PORTING.md` ruling, so U9 doesn't have to rediscover the pattern.
    pub fn identifier(&self) -> Option<&str> {
        match self {
            Expression::Arithmetic(e) => Some(&e.identifier),
            Expression::Variable(e) => Some(&e.identifier),
            _ => None,
        }
    }

    /// The Rust translation of an `instanceof (NumberLiteralExpression |
    /// AlphabetLetterExpression)` narrowing followed by a bare `.constant` read — e.g.
    /// `ArithmeticOperator.getIntConstantForWord`/`getConstantValue`'s `e.constant`
    /// fallback after a `NumberLiteralExpression` early-return (`ArithmeticOperator.java:264`,
    /// `:275`; the same pattern in `RelationalOperator`'s same-named helpers). `None` for
    /// every other variant. See [`Self::identifier`]'s docs for why this exists ahead of
    /// U9.
    pub fn constant(&self) -> Option<i32> {
        match self {
            Expression::NumberLiteral(e) => Some(e.constant),
            Expression::AlphabetLetter(e) => Some(e.constant),
            _ => None,
        }
    }

    /// `Expression.act(String begin)` (`Expression.java:74-76`) — the base class's
    /// default, which every kind here inherits unchanged (nothing overrides this exact
    /// 1-arg overload; see the module docs on Java overloading vs overriding).  Always
    /// fails; Java's declared `boolean` return is dead (the body only ever throws), kept
    /// here as `Result<bool, _>` rather than `Result<Infallible, _>` to mirror that
    /// signature shape rather than imply more static guarantee than Java's did.
    pub fn act(&self, begin: &str) -> Result<bool, ExprError> {
        Err(ExprError::InvalidType {
            context: begin.to_string(),
            java_class_name: self.java_class_name(),
        })
    }
}

impl fmt::Display for Expression {
    /// `Expression.toString()` (`Expression.java:70-72`): every kind's own
    /// `expressionInString`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expression::Automaton(e) => write!(f, "{}", e.expression_in_string),
            Expression::Arithmetic(e) => write!(f, "{}", e.expression_in_string),
            Expression::Variable(e) => write!(f, "{}", e.expression_in_string),
            Expression::NumberLiteral(e) => write!(f, "{}", e.expression_in_string),
            Expression::AlphabetLetter(e) => write!(f, "{}", e.expression_in_string),
            Expression::Word(e) => write!(f, "{}", e.expression_in_string),
        }
    }
}

// ---------------------------------------------------------------------------
// AutomatonExpression
// ---------------------------------------------------------------------------

/// `Expressions/AutomatonExpression.java` (45 LOC).
#[derive(Debug, Clone)]
pub struct AutomatonExpression {
    expression_in_string: String,
    pub m: Automaton,
}

impl AutomatonExpression {
    pub fn new(expression_in_string: impl Into<String>, m: Automaton) -> Self {
        AutomatonExpression {
            expression_in_string: expression_in_string.into(),
            m,
        }
    }

    /// `AutomatonExpression.act(String name, int i, Automaton M, List<String>
    /// identifiers)` (`:32-44`) — used when this expression appears as one argument of
    /// a `$name(...)`/word-index binding (`Word`/`Function`, U4). `i` is the 0-based
    /// argument index Java also takes; the 1-based `i + 1` in error text is computed
    /// here, not by the caller.
    ///
    /// `acc` is the caller's accumulator automaton (Java's parameter is confusingly
    /// also named `M`, shadowing this expression's own field of the same name — kept
    /// distinct here as `acc` vs [`Self::m`] to avoid that ambiguity in Rust, where
    /// `self.m` and a same-named parameter would otherwise be easy to conflate).
    pub fn act(
        &self,
        name: &str,
        i: usize,
        acc: Automaton,
        identifiers: &mut Vec<String>,
        logging: &mut wr_core::logging::Logging,
    ) -> Result<Automaton, ExprError> {
        if self.m.get_arity() != 1 {
            return Err(ExprError::AutomatonArgumentWrongArity {
                position: i + 1,
                name: name.to_string(),
            });
        }
        if !self.m.is_bound() {
            return Err(ExprError::AutomatonArgumentUnlabeled {
                position: i + 1,
                name: name.to_string(),
            });
        }
        identifiers.push(self.m.label[0].clone());
        // `Logging.indent()`/`dedent()` (`AutomatonExpression.java:40/42`) bracket the
        // `and` call -- no text of their own, matching every sibling `act` in this file.
        logging.indent();
        let result = and(&acc, &self.m, logging).into_automaton();
        logging.dedent();
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// ArithmeticExpression
// ---------------------------------------------------------------------------

/// `Expressions/ArithmeticExpression.java` (41 LOC).
#[derive(Debug, Clone)]
pub struct ArithmeticExpression {
    expression_in_string: String,
    pub m: Automaton,
    pub identifier: String,
}

impl ArithmeticExpression {
    pub fn new(
        expression_in_string: impl Into<String>,
        m: Automaton,
        identifier: impl Into<String>,
    ) -> Self {
        ArithmeticExpression {
            expression_in_string: expression_in_string.into(),
            m,
            identifier: identifier.into(),
        }
    }

    /// `ArithmeticExpression.act(List<String> identifiers, Automaton M, List<String>
    /// quantify)` (`:33-40`). `acc` is the caller's accumulator (see
    /// [`AutomatonExpression::act`]'s doc on the Java parameter-shadowing this avoids).
    pub fn act(
        &self,
        identifiers: &mut Vec<String>,
        acc: Automaton,
        quantify: &mut Vec<String>,
        logging: &mut wr_core::logging::Logging,
    ) -> Automaton {
        identifiers.push(self.identifier.clone());
        // `Logging.indent()`/`dedent()` (`ArithmeticExpression.java:34/38`) -- no text.
        logging.indent();
        let result = and(&acc, &self.m, logging).into_automaton();
        logging.dedent();
        quantify.push(self.identifier.clone());
        result
    }
}

// ---------------------------------------------------------------------------
// VariableExpression
// ---------------------------------------------------------------------------

/// `Expressions/VariableExpression.java` (49 LOC).
#[derive(Debug, Clone)]
pub struct VariableExpression {
    pub identifier: String,
    expression_in_string: String,
}

impl VariableExpression {
    pub fn new(identifier: impl Into<String>) -> Self {
        let identifier = identifier.into();
        VariableExpression {
            expression_in_string: identifier.clone(),
            identifier,
        }
    }

    /// `VariableExpression.act(Token t, NumberSystem ns, List<String> identifiers,
    /// Automaton M, List<String> quantify)` (`:34-48`). `t.getUniqueString()` becomes
    /// `fresh.next_identifier()` per Ruling 4; `ns` is read-only here (only
    /// `ns.equality` is consulted, never mutated), so `&NumberSystem` — not `&mut` —
    /// suffices even before U5's interior-mutability upgrade lands.
    ///
    /// `ns` is `Option<&NumberSystem>`, not `&NumberSystem`: Java's caller,
    /// `Word.java:62`, passes `wordAutomaton.getNS().get(i)`, which is a real `null`
    /// whenever track `i` was declared with an explicit alphabet (`{0,1}`, …) rather
    /// than `msd_k`/`lsd_k` (`ParseMethods.parseAlphabetDeclaration`'s `bases.add(null)`
    /// branch). `ns` is only ever dereferenced in the repeated-identifier branch below
    /// (the first-occurrence branch never touches it, in Java or here), so a first
    /// occurrence is safe with `ns = None`; a *repeated* occurrence with `ns = None` is
    /// exactly the shape that NPEs in Java — see [`ExprError::RepeatedIdentifierMissingNumberSystem`]'s
    /// docs and WB-013 for the full call chain and trigger.
    pub fn act(
        &self,
        fresh: &mut FreshIdentifiers,
        ns: Option<&NumberSystem>,
        identifiers: &mut Vec<String>,
        acc: Automaton,
        quantify: &mut Vec<String>,
        logging: &mut wr_core::logging::Logging,
    ) -> Result<Automaton, ExprError> {
        if !identifiers.contains(&self.identifier) {
            identifiers.push(self.identifier.clone());
            Ok(acc)
        } else {
            let ns = ns.ok_or_else(|| ExprError::RepeatedIdentifierMissingNumberSystem {
                identifier: self.identifier.clone(),
            })?;
            let new_identifier = format!("{}{}", self.identifier, fresh.next_identifier());
            let mut eq = ns.equality.clone();
            eq.bind(vec![self.identifier.clone(), new_identifier.clone()]);
            quantify.push(new_identifier.clone());
            identifiers.push(new_identifier);
            // `Logging.indent()`/`dedent()` (`VariableExpression.java:43/45`) -- only on
            // this (repeated-identifier) branch, matching Java's own placement inside the
            // `else`.
            logging.indent();
            let result = and(&acc, &eq, logging).into_automaton();
            logging.dedent();
            Ok(result)
        }
    }
}

// ---------------------------------------------------------------------------
// NumberLiteralExpression
// ---------------------------------------------------------------------------

/// `Expressions/NumberLiteralExpression.java` (72 LOC), complete as of U5 — see the module
/// docs for why its own `act(...)` had to wait for `NumberSystem::get_constant`'s `&self`
/// form.
#[derive(Debug, Clone)]
pub struct NumberLiteralExpression {
    expression_in_string: String,
    value: BigInt,
    base: Rc<NumberSystem>,
    /// `Expression.constant`, populated by the same try/fallback-to-zero conversion as
    /// Java's constructor (`:38-42`: `value.intValueExact()`, catching `ArithmeticException`
    /// to `0`). Ported for fidelity even though tracing every real read of a
    /// `NumberLiteralExpression`'s `.constant` (as opposed to [`Self::value`]/
    /// [`Self::int_value_exact`], which every located call site actually uses instead —
    /// see `RelationalOperator.getConstantValue`/`getIntConstantForWord`,
    /// `ArithmeticOperator`'s same-named helpers) found none; U9 should confirm this
    /// when it ports those call sites rather than this unit asserting it definitively.
    pub constant: i32,
}

impl NumberLiteralExpression {
    pub fn new(
        expression_in_string: impl Into<String>,
        value: BigInt,
        base: Rc<NumberSystem>,
    ) -> Self {
        // `value.intValueExact()` / catch ArithmeticException -> 0 (`:38-42`). `i32::
        // try_from(&BigInt)` is exactly `intValueExact`'s "exact fit or nothing" contract.
        let constant = i32::try_from(&value).unwrap_or(0);
        NumberLiteralExpression {
            expression_in_string: expression_in_string.into(),
            value,
            base,
            constant,
        }
    }

    /// `NumberLiteralExpression.value()` (`:45-47`).
    pub fn value(&self) -> &BigInt {
        &self.value
    }

    /// The number system this literal was lexed under (`NumberLiteralExpression.base`,
    /// `:32`, private in Java but needed by [`crate::token::NumberLiteral`], which holds
    /// the same field for the same reason).
    pub fn base(&self) -> &Rc<NumberSystem> {
        &self.base
    }

    /// `NumberLiteralExpression.isZero()` (`:49-51`): `value.signum() == 0`.
    pub fn is_zero(&self) -> bool {
        self.value == BigInt::from(0)
    }

    /// `NumberLiteralExpression.intValueExact(String context)` (`:53-59`): `context +
    /// " must fit in a Java int, found: " + value` on overflow. Returns a plain `i32`
    /// rather than routing through [`ExprError`] because the message needs the *caller's*
    /// context string, not a fixed shape this crate's error enum could carry generically
    /// without becoming stringly-typed for every other variant too; callers (U9) build
    /// their own error text the same way Java's `intValueExact` callers do
    /// (`"number literal " + ne + " used in word automaton output comparison"`, etc.).
    pub fn int_value_exact(&self, context: &str) -> Result<i32, String> {
        i32::try_from(&self.value)
            .map_err(|_| format!("{context} must fit in a Java int, found: {}", self.value))
    }

    /// `NumberLiteralExpression.act(Token t, List<String> identifiers, List<String> quantify,
    /// Automaton M)` (`NumberLiteralExpression.java:61-71`): intersect `acc` with "this
    /// track equals my value", under a fresh synthetic identifier that the caller then
    /// quantifies away.
    ///
    /// **Unblocked by U5, not by this unit's own design.** U2 ported the rest of this struct
    /// but deferred this one method, because it needs `NumberSystem::get_constant` and that
    /// took `&mut self` at the time, while [`crate::predicate_env::PredicateEnv`]
    /// deliberately hands out a shared `Rc<NumberSystem>`
    /// (`predicate_env.rs`'s Ruling 1). U5 made `get_constant` take `&self` behind interior
    /// mutability, which is exactly the signature this needed — so it lands here rather than
    /// leaving a doc comment claiming a blocker that no longer exists.
    ///
    /// `t.getUniqueString()` becomes `fresh.next_identifier()` (Ruling 4).
    /// `Logging.indent()`/`dedent()` around the `and` (`NumberLiteralExpression.java:67/69`)
    /// bracket it, no text of their own, matching every sibling `act` in this file.
    pub fn act(
        &self,
        fresh: &mut FreshIdentifiers,
        identifiers: &mut Vec<String>,
        quantify: &mut Vec<String>,
        acc: Automaton,
        logging: &mut wr_core::logging::Logging,
    ) -> Result<Automaton, ExprError> {
        let mut constant = self.base.get_constant(&self.value, logging)?;
        let id = fresh.next_identifier();
        constant.bind(vec![id.clone()]);
        identifiers.push(id.clone());
        quantify.push(id);
        logging.indent();
        let result = and(&acc, &constant, logging).into_automaton();
        logging.dedent();
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// AlphabetLetterExpression
// ---------------------------------------------------------------------------

/// `Expressions/AlphabetLetterExpression.java` (25 LOC). No `act(...)` of its own in
/// Java — a pure leaf value.
#[derive(Debug, Clone)]
pub struct AlphabetLetterExpression {
    expression_in_string: String,
    pub constant: i32,
}

impl AlphabetLetterExpression {
    pub fn new(expression_in_string: impl Into<String>, constant: i32) -> Self {
        AlphabetLetterExpression {
            expression_in_string: expression_in_string.into(),
            constant,
        }
    }
}

// ---------------------------------------------------------------------------
// WordExpression
// ---------------------------------------------------------------------------

/// `Expressions/WordExpression.java` (34 LOC). No `act(...)` of its own in Java either
/// — it is a pure value object built by `Predicate.putWord` (U4) and consumed by
/// `RelationalOperator`/`ArithmeticOperator::act` (U9).
#[derive(Debug, Clone)]
pub struct WordExpression {
    expression_in_string: String,
    pub word_automaton: Automaton,
    pub m: Automaton,
    pub identifiers_to_quantify: Vec<String>,
}

impl WordExpression {
    pub fn new(
        expression_in_string: impl Into<String>,
        word_automaton: Automaton,
        m: Automaton,
        identifiers_to_quantify: Vec<String>,
    ) -> Self {
        WordExpression {
            expression_in_string: expression_in_string.into(),
            word_automaton,
            m,
            identifiers_to_quantify,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wr_core::numsys::NumberSystem;

    fn ns(name: &str) -> Rc<NumberSystem> {
        Rc::new(NumberSystem::new(name).unwrap())
    }

    /// A genuinely non-trivial (not `TRUE_FALSE_AUTOMATON`) automaton with one track
    /// per entry of `labels`, each over `{0, 1}`. `get_arity()`/`is_bound()` are
    /// meaningless on `Automaton::true_false` (its `get_arity()` always short-circuits
    /// to `0`, per U0), so `AutomatonExpression::act`'s arity/bound checks need a real
    /// track-bearing automaton to exercise honestly.
    fn labeled_automaton(labels: &[&str]) -> Automaton {
        let alphabet: Vec<Vec<i32>> = labels.iter().map(|_| vec![0, 1]).collect();
        let alphabet_size: usize = alphabet.iter().map(|a| a.len()).product::<usize>().max(1);
        let fa = wr_core::fa::Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size,
            o: vec![1],
            d: vec![std::collections::BTreeMap::new()],
        };
        Automaton::new(
            fa,
            alphabet,
            labels.iter().map(|s| s.to_string()).collect(),
            labels.iter().map(|_| Some(true)).collect(),
        )
    }

    /// Arity 1 (one track) but genuinely unbound: `label` and `alphabet` have
    /// mismatched lengths, matching [`wr_core::automaton::Automaton::is_bound`]'s
    /// `label.len() == alphabet.len()` contract.
    fn unbound_arity_one_automaton() -> Automaton {
        let fa = wr_core::fa::Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1],
            d: vec![std::collections::BTreeMap::new()],
        };
        Automaton::new(fa, vec![vec![0, 1]], vec![], vec![Some(true)])
    }

    /// A 1-track `{0,1}` automaton, genuinely non-trivial AND total (unlike
    /// [`labeled_automaton`], whose empty transition map only accepts the empty word
    /// and is therefore not total) -- accepts exactly the words that are entirely `1`s
    /// (including the empty word). `label` lets callers give two instances of this
    /// family the SAME track name, so a cross product between them collapses to one
    /// shared track rather than two disjoint ones (mirrors `product.rs`'s own
    /// `cross_product_and_minimize_computes_and_over_a_shared_variable` fixture).
    /// Needed so `act()` tests can pass a real (not `Automaton::true_false`)
    /// accumulator: `and`'s TRUE-short-circuit returns the OTHER operand verbatim
    /// whenever either side is the `TRUE_FALSE_AUTOMATON` sentinel, which would let a
    /// test pass even if `and` were accidentally swapped for `or` -- see finding #6 of
    /// Phase 3a U2's adversarial review, pinned by the two tests below that use this.
    fn all_ones_automaton(label: &str) -> Automaton {
        let mut d0 = std::collections::BTreeMap::new();
        d0.insert(0, vec![1]);
        d0.insert(1, vec![0]);
        let mut d1 = std::collections::BTreeMap::new();
        d1.insert(0, vec![1]);
        d1.insert(1, vec![1]);
        let fa = wr_core::fa::Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 0],
            d: vec![d0, d1],
        };
        Automaton::new(
            fa,
            vec![vec![0, 1]],
            vec![label.to_string()],
            vec![Some(true)],
        )
    }

    /// Same 1-track `{0,1}` shape as [`all_ones_automaton`] (so it can share a track
    /// with one, per that function's docs) but accepts EVERY word -- a real, TOTAL
    /// automaton, deliberately NOT the `Automaton::true_false` sentinel, so `and`ing it
    /// with another real automaton must run a genuine cross product rather than
    /// short-circuiting on triviality.
    fn accepts_everything_automaton(label: &str) -> Automaton {
        let mut d0 = std::collections::BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        let fa = wr_core::fa::Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1],
            d: vec![d0],
        };
        Automaton::new(
            fa,
            vec![vec![0, 1]],
            vec![label.to_string()],
            vec![Some(true)],
        )
    }

    // ------------------------------------------------------------- Display / class name

    #[test]
    fn display_returns_expression_in_string_for_every_kind() {
        let cases: Vec<(Expression, &str)> = vec![
            (
                Expression::Automaton(AutomatonExpression::new(
                    "a+b>c",
                    Automaton::true_false(true),
                )),
                "a+b>c",
            ),
            (Expression::Variable(VariableExpression::new("x")), "x"),
            (
                Expression::AlphabetLetter(AlphabetLetterExpression::new("@-1", -1)),
                "@-1",
            ),
        ];
        for (expr, expected) in cases {
            assert_eq!(expr.to_string(), expected);
        }
    }

    #[test]
    fn java_class_names_are_fully_qualified() {
        assert_eq!(
            Expression::Word(Box::new(WordExpression::new(
                "Thue[a]",
                Automaton::true_false(true),
                Automaton::true_false(true),
                vec![]
            )))
            .java_class_name(),
            "Main.EvalComputations.Expressions.WordExpression"
        );
        assert_eq!(
            Expression::AlphabetLetter(AlphabetLetterExpression::new("@0", 0)).java_class_name(),
            "Main.EvalComputations.Expressions.AlphabetLetterExpression"
        );
    }

    #[test]
    fn base_act_always_rejects_with_the_java_message_shape() {
        let e = Expression::AlphabetLetter(AlphabetLetterExpression::new("@0", 0));
        let err = e.act("argument 1 of function phi").unwrap_err();
        assert_eq!(
            err.to_string(),
            "argument 1 of function phi cannot be of type Main.EvalComputations.Expressions.AlphabetLetterExpression"
        );
    }

    // ------------------------------------------------------------ AutomatonExpression

    #[test]
    fn automaton_expression_act_rejects_wrong_arity() {
        let ae = AutomatonExpression::new("phi", labeled_automaton(&["a", "b"]));
        let mut identifiers = vec![];
        let err = ae
            .act(
                "phi",
                0,
                Automaton::true_false(true),
                &mut identifiers,
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap_err();
        assert_eq!(
            err,
            ExprError::AutomatonArgumentWrongArity {
                position: 1,
                name: "phi".to_string()
            }
        );
        assert_eq!(
            err.to_string(),
            "argument 1 of function phi cannot be an automaton with != 1 inputs"
        );
    }

    #[test]
    fn automaton_expression_act_rejects_unlabeled_input() {
        let a = unbound_arity_one_automaton();
        assert_eq!(a.get_arity(), 1);
        assert!(
            !a.is_bound(),
            "mismatched label/alphabet lengths must not count as bound"
        );

        let ae = AutomatonExpression::new("phi", a);
        let err = ae
            .act(
                "phi",
                1,
                Automaton::true_false(true),
                &mut vec![],
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap_err();
        assert_eq!(
            err,
            ExprError::AutomatonArgumentUnlabeled {
                position: 2,
                name: "phi".to_string()
            }
        );
    }

    /// Finding #6 of Phase 3a U2's adversarial review: the earlier version of this test
    /// passed `Automaton::true_false(true)` as the accumulator, which makes `and`'s
    /// TRUE-short-circuit return `own` verbatim -- never running a real cross product,
    /// so the test would pass unchanged even if `and` were accidentally swapped for
    /// `or`. Both operands here are genuinely non-trivial, TOTAL automata sharing one
    /// track ("y"), so `and` must run its real cross-product path; the language checks
    /// below (via the equivalence oracle, not structural/label equality) confirm it did
    /// -- `own`'s language (all-1s) is a strict subset of `acc`'s (accept-everything),
    /// so `own ∪ acc == acc` but `own ∩ acc == own`, which distinguishes `and` from
    /// `or` outright.
    #[test]
    fn automaton_expression_act_ands_in_its_own_automaton_and_records_the_label() {
        let own = all_ones_automaton("y");
        let acc = accepts_everything_automaton("y");
        let ae = AutomatonExpression::new("phi", own.clone());
        let mut identifiers = vec![];
        let result = ae
            .act(
                "phi",
                0,
                acc.clone(),
                &mut identifiers,
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap();
        assert_eq!(identifiers, vec!["y".to_string()]);
        assert!(!result.is_true_false_automaton());
        assert_eq!(result.get_arity(), own.get_arity());
        assert_eq!(result.label, own.label);

        // `minimize` doesn't preserve totality (see `product.rs`'s own
        // `cross_product_and_minimize_computes_and_over_a_shared_variable` fixture for
        // the same caveat), so totalize defensively before handing these to the
        // oracle -- `own`/`acc` are already total by construction, so this is a no-op
        // for them.
        let mut result_fa = result.fa.clone();
        result_fa.totalize(0);
        let mut own_fa = own.fa.clone();
        own_fa.totalize(0);
        let mut acc_fa = acc.fa.clone();
        acc_fa.totalize(0);
        assert_eq!(
            wr_core::equiv::language_equivalent(&result_fa, &own_fa),
            Ok(true),
            "and(acc, own) must equal own's (all-1s) language exactly"
        );
        assert_eq!(
            wr_core::equiv::language_equivalent(&result_fa, &acc_fa),
            Ok(false),
            "must NOT silently equal the accept-everything accumulator's language \
             (would happen if `and` had been swapped for `or`)"
        );
    }

    // ------------------------------------------------------------ ArithmeticExpression

    /// Finding #6 of Phase 3a U2's adversarial review: the earlier version of this test
    /// passed `Automaton::true_false(true)` for BOTH `self.m` and the accumulator, so
    /// `and` short-circuited twice over and never ran a real cross product -- a
    /// hypothetical `and` -> `or` swap in the implementation would have passed
    /// unnoticed. See [`automaton_expression_act_ands_in_its_own_automaton_and_records_the_label`]'s
    /// docs for the same fixture shape and the same `own ⊆ acc` distinguishing
    /// argument, applied here to `ArithmeticExpression::act`.
    #[test]
    fn arithmetic_expression_act_conjoins_a_genuine_cross_product_with_the_accumulator() {
        let own = all_ones_automaton("y");
        let acc = accepts_everything_automaton("y");
        let ae = ArithmeticExpression::new("a+b", own.clone(), "x");
        let mut identifiers = vec![];
        let mut quantify = vec![];
        let result = ae.act(
            &mut identifiers,
            acc.clone(),
            &mut quantify,
            &mut wr_core::logging::Logging::new(),
        );
        assert_eq!(identifiers, vec!["x".to_string()]);
        assert_eq!(quantify, vec!["x".to_string()]);

        let mut result_fa = result.fa.clone();
        result_fa.totalize(0);
        let mut own_fa = own.fa.clone();
        own_fa.totalize(0);
        let mut acc_fa = acc.fa.clone();
        acc_fa.totalize(0);
        assert_eq!(
            wr_core::equiv::language_equivalent(&result_fa, &own_fa),
            Ok(true),
            "and(acc, self.m) must equal self.m's (all-1s) language exactly"
        );
        assert_eq!(
            wr_core::equiv::language_equivalent(&result_fa, &acc_fa),
            Ok(false),
            "must NOT silently equal the accept-everything accumulator's language \
             (would happen if `and` had been swapped for `or`, since own ∪ acc == acc)"
        );
    }

    // --------------------------------------------------- NumberLiteralExpression::act

    /// `NumberLiteralExpression.act` (`:61-71`), unblocked by U5's `&self` `get_constant`.
    /// The literal's automaton is bound to a FRESH synthetic name, which is both recorded as
    /// an identifier and queued for quantification, and intersected into the accumulator.
    #[test]
    fn number_literal_expression_act_binds_a_fresh_identifier_and_quantifies_it() {
        let base = ns("msd_2");
        let literal = NumberLiteralExpression::new("5", BigInt::from(5), Rc::clone(&base));
        let mut fresh = FreshIdentifiers::new();
        let mut identifiers = vec![];
        let mut quantify = vec![];
        let m = literal
            .act(
                &mut fresh,
                &mut identifiers,
                &mut quantify,
                Automaton::true_false(true),
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap();

        assert_eq!(fresh.issued(), 1, "exactly one synthetic name is minted");
        assert_eq!(identifiers, vec!["WALNUT_8AthA0PZZI_1".to_string()]);
        assert_eq!(quantify, identifiers, "the same name is queued for removal");
        // `and(TRUE, constant)` short-circuits to the constant automaton itself, bound to
        // the fresh name -- so the result is a one-track automaton over that name.
        assert_eq!(m.label, identifiers);
        assert_eq!(m.alphabet, vec![vec![0, 1]]);
    }

    /// The number system is consulted through a SHARED handle, so two literals over the same
    /// base hit one memo cache — `PORTING.md`'s Ruling 1 in action, and the reason this
    /// method could not be written before U5.
    #[test]
    fn number_literal_expression_act_shares_one_number_system_handle() {
        let base = ns("msd_2");
        let five = NumberLiteralExpression::new("5", BigInt::from(5), Rc::clone(&base));
        let also_five = NumberLiteralExpression::new("5", BigInt::from(5), Rc::clone(&base));
        assert_eq!(Rc::strong_count(&base), 3);

        let mut fresh = FreshIdentifiers::new();
        let mut identifiers = vec![];
        let mut quantify = vec![];
        let first = five
            .act(
                &mut fresh,
                &mut identifiers,
                &mut quantify,
                Automaton::true_false(true),
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap();
        let second = also_five
            .act(
                &mut fresh,
                &mut identifiers,
                &mut quantify,
                Automaton::true_false(true),
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap();
        assert_eq!(fresh.issued(), 2, "each act mints its own name");
        assert_eq!(first.fa.q, second.fa.q, "same cached constant automaton");
    }

    /// A rejected constant surfaces as an `Err`, wrapping `NumSysError`'s verbatim Walnut
    /// message rather than a fresh string.
    #[test]
    fn number_literal_expression_act_propagates_number_system_errors() {
        let base = ns("msd_2");
        let negative = NumberLiteralExpression::new("-1", BigInt::from(-1), base);
        let mut fresh = FreshIdentifiers::new();
        let err = negative
            .act(
                &mut fresh,
                &mut vec![],
                &mut vec![],
                Automaton::true_false(true),
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap_err();
        assert_eq!(err.to_string(), "negative constant -1");
    }

    // -------------------------------------------------------------- VariableExpression

    #[test]
    fn variable_expression_act_first_occurrence_just_records_identifier() {
        let ve = VariableExpression::new("a");
        let mut fresh = FreshIdentifiers::new();
        let n = ns("msd_2");
        let mut identifiers = vec![];
        let mut quantify = vec![];
        let m = ve
            .act(
                &mut fresh,
                Some(&n),
                &mut identifiers,
                Automaton::true_false(true),
                &mut quantify,
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap();
        assert_eq!(identifiers, vec!["a".to_string()]);
        assert!(quantify.is_empty());
        assert_eq!(
            fresh.issued(),
            0,
            "first occurrence must not mint a fresh name"
        );
        assert!(m.is_true_automaton());
    }

    /// A first occurrence never dereferences `ns` (matching Java: the repeated-identifier
    /// branch is the only place `ns` is touched), so `ns = None` must be safe here even
    /// though it would fail a repeated occurrence -- see
    /// [`variable_expression_act_repeated_occurrence_with_no_ns_reports_the_java_npe_shape`].
    #[test]
    fn variable_expression_act_first_occurrence_tolerates_missing_number_system() {
        let ve = VariableExpression::new("a");
        let mut fresh = FreshIdentifiers::new();
        let mut identifiers = vec![];
        let mut quantify = vec![];
        let m = ve
            .act(
                &mut fresh,
                None,
                &mut identifiers,
                Automaton::true_false(true),
                &mut quantify,
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap();
        assert_eq!(identifiers, vec!["a".to_string()]);
        assert!(m.is_true_automaton());
    }

    #[test]
    fn variable_expression_act_repeated_occurrence_mints_fresh_equality() {
        let ve = VariableExpression::new("a");
        let mut fresh = FreshIdentifiers::new();
        let n = ns("msd_2");
        let mut identifiers = vec!["a".to_string()]; // already seen once
        let mut quantify = vec![];
        let m = ve
            .act(
                &mut fresh,
                Some(&n),
                &mut identifiers,
                Automaton::true_false(true),
                &mut quantify,
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap();
        assert_eq!(fresh.issued(), 1);
        let expected_new_id = "aWALNUT_8AthA0PZZI_1".to_string();
        assert_eq!(identifiers, vec!["a".to_string(), expected_new_id.clone()]);
        assert_eq!(quantify, vec![expected_new_id]);
        // `and(TRUE, ns.equality.bind(...))`'s language is exactly `ns.equality`'s own,
        // which is non-trivial (not a TRUE/FALSE automaton) -- confirms the `and` really
        // ran rather than short-circuiting.
        assert!(!m.is_true_false_automaton());
    }

    /// Pins finding #1 from Phase 3a U2's adversarial review: `T[i][i] = @1` where track
    /// `i` is declared `{0,1}` (no `msd_k`/`lsd_k`) reaches Java's `ns.equality` on a
    /// `null` `ns` -- a real `NullPointerException`, not a `WalnutException`. This must
    /// surface as an explicit, documented error here, never a silently-wrong default.
    #[test]
    fn variable_expression_act_repeated_occurrence_with_no_ns_reports_the_java_npe_shape() {
        let ve = VariableExpression::new("i");
        let mut fresh = FreshIdentifiers::new();
        let mut identifiers = vec!["i".to_string()]; // already seen once, e.g. T[i][i]
        let mut quantify = vec![];
        let err = ve
            .act(
                &mut fresh,
                None,
                &mut identifiers,
                Automaton::true_false(true),
                &mut quantify,
                &mut wr_core::logging::Logging::new(),
            )
            .unwrap_err();
        assert_eq!(
            err,
            ExprError::RepeatedIdentifierMissingNumberSystem {
                identifier: "i".to_string()
            }
        );
        assert_eq!(fresh.issued(), 0, "must fail before minting a fresh name");
    }

    // ---------------------------------------------------------- NumberLiteralExpression

    #[test]
    fn number_literal_expression_stores_value_and_truncated_constant() {
        let n = ns("msd_2");
        let nl = NumberLiteralExpression::new("5", BigInt::from(5), Rc::clone(&n));
        assert_eq!(*nl.value(), BigInt::from(5));
        assert_eq!(nl.constant, 5);
        assert!(!nl.is_zero());
        assert!(Rc::ptr_eq(nl.base(), &n));
    }

    #[test]
    fn number_literal_expression_zero_and_int_value_exact() {
        let n = ns("msd_2");
        let zero = NumberLiteralExpression::new("0", BigInt::from(0), Rc::clone(&n));
        assert!(zero.is_zero());
        assert_eq!(zero.int_value_exact("ctx"), Ok(0));
    }

    /// Java: `value.intValueExact()` catches `ArithmeticException` down to `constant =
    /// 0` in the constructor (a value that does not fit in a Java `int`), but
    /// `intValueExact(String context)` (a DIFFERENT method, called explicitly rather
    /// than from the constructor) still reports the overflow as a real error rather
    /// than silently returning that same zero. Both behaviors are pinned here since
    /// they are easy to conflate.
    #[test]
    fn number_literal_expression_overflow_zeroes_constant_but_int_value_exact_still_errors() {
        let n = ns("msd_2");
        let huge = BigInt::from(i64::MAX) * BigInt::from(1000);
        let nl = NumberLiteralExpression::new(huge.to_string(), huge.clone(), n);
        assert_eq!(
            nl.constant, 0,
            "constructor silently zeroes an out-of-i32-range value, matching Java's catch"
        );
        assert_eq!(
            nl.int_value_exact("value"),
            Err(format!("value must fit in a Java int, found: {huge}"))
        );
    }

    // ------------------------------------------------------------------ round-trip

    #[test]
    fn expression_variants_round_trip_through_the_enum() {
        let word = WordExpression::new(
            "Thue[a]",
            Automaton::true_false(true),
            Automaton::true_false(false),
            vec!["x".to_string()],
        );
        let e = Expression::Word(Box::new(word));
        match &e {
            Expression::Word(w) => {
                assert_eq!(w.identifiers_to_quantify, vec!["x".to_string()]);
                assert!(w.word_automaton.is_true_automaton());
                assert!(!w.m.is_true_automaton());
            }
            other => panic!("expected Word, got {other:?}"),
        }
    }
}
