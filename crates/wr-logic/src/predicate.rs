// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Predicate.java`'s tokenizer — Phase 3a's U3.
//!
//! Ports the scanning half of Walnut's `Predicate` class: the pattern table
//! (`Predicate.java:72-108`), the constructor's whitespace-only early return (`:143`),
//! [`Predicate::tokenize_and_compute_post_order`] (`:148-253`), the shunting-yard drain
//! that follows it, [`Predicate::handle_quantifier`] (`:276-285`), and
//! [`find_current_number_system`] (`:255-274`). The result is a [`Token`] stream in
//! **postfix (post-)order**, exactly like Java's `postOrder` field — the tokenizer and the
//! shunting yard are one pass in Walnut, not two, and [`crate::token::Operator::push_onto`]
//! (U2) already ports the yard itself.
//!
//! # `putWord`/`putFunction`/`putMacro` (U4)
//!
//! `Predicate.java` also contains `putWord`/`putFunction`/`putMacro`/
//! `parseParenthesizedArguments`/`readMacroFile` — the three token kinds that reach out to
//! [`crate::predicate_env::PredicateEnv`]'s word/function/macro lookups and build nested
//! `Predicate`s. U3 ported everything *around* them (the four recognizing patterns
//! `PATTERN_FOR_WORD`/`PATTERN_FOR_WORD_WITH_DELIMITER`/`PATTERN_FOR_FUNCTION`/
//! `PATTERN_FOR_MACRO` at their exact scanning-chain priority, and the
//! `lastTokenWasOperator` adjacency check Java performs *before* calling each of them) and
//! stubbed the three construction methods; U4 (this section) replaces those three method
//! bodies — [`Predicate::put_word`], [`Predicate::put_function`], [`Predicate::put_macro`]
//! — plus [`Predicate::parse_parenthesized_arguments`] (shared by the latter two) and the
//! macro `%N`-substitution helpers, without changing the scanning loop itself.
//!
//! **Nested `Predicate` construction** (word indices, function arguments, and — via the
//! re-lex, not a nested `Predicate` — macro expansions) all go through
//! [`Predicate::with_context`], the same public entry point U3 exposed for exactly this;
//! each index/argument's `real_starting_position` is computed via [`Self::position`] on
//! its own start offset within *this* predicate's (unmutated, for `put_word`/
//! `put_function`) buffer, so [`Self::java_offset`]'s UTF-16 conversion and
//! `real_starting_position` threading both compose correctly through arbitrary nesting
//! depth (Ruling 3's "corollary for U4").
//!
//! **The macro re-lex** ([`Predicate::put_macro`]) is the one case that does NOT build a
//! nested `Predicate`: per Ruling 3, it rewrites `self.predicate` in place (splicing the
//! macro's `%N`-substituted expansion text in at the call site, preserving the leading
//! whitespace Java's own pattern captures) and returns the byte offset the *main* scanning
//! loop should resume at — the expansion is then lexed as part of the SAME `Predicate`,
//! not a child one. No borrow of `self.predicate` is held across that rewrite (every
//! offset/string this method needs is copied out of the `Captures`/cloned first — see
//! [`Predicate::put_macro`]'s own body).
//!
//! **No macro-expansion depth or cycle guard exists**, ported faithfully per Ruling 3's
//! explicit instruction: `#a` expanding to `#a(...)` loops forever in real Walnut, and
//! does here too (pinned by
//! [`tests::macro_expansion_has_no_depth_guard_and_will_recurse_until_the_environment_stops_it`],
//! which proves it via a call-counting [`PredicateEnv`] rather than actually hanging).
//!
//! Four genuine Walnut (Java) bugs found while porting this section are logged in
//! `docs/WALNUT-BUGS.md` (WB-017 through WB-020) and ported verbatim, not fixed — see each
//! one's doc comment at its `LexError`/behavior site below for the empirical evidence and
//! the exact trigger.
//!
//! # Ruling 2 (`predicate_env.rs`) — the four Java→Rust regex-dialect divergences
//!
//! All four are handled here, none by accident:
//!
//! 1. **No look-behind.** `LOGICAL_OPERATORS`'s `(?<!\.)` (`:80`) does not compile under
//!    Rust's regex engines at all. [`find_logical_operator`] compiles the pattern without
//!    it and then rejects a match whose operator character is immediately preceded by `.`,
//!    treating "no preceding byte" as a pass (Java's zero-width look-behind succeeds at
//!    offset 0). See that function's docs for why rejecting outright is *exactly*
//!    equivalent to Java's backtracking here rather than merely close.
//! 2. **ASCII vs Unicode classes.** Every `\s`/`\w`/`\d` below is written `(?-u:\s)`/
//!    `(?-u:\w)`/`(?-u:\d)`, because Java's `Pattern` is ASCII-only without
//!    `UNICODE_CHARACTER_CLASS` (never set in Walnut) while Rust's defaults are
//!    Unicode-aware. Without this, e.g. a NBSP between two tokens would be silently
//!    accepted here and rejected by real Walnut with "Undefined token"
//!    ([`tests::non_ascii_whitespace_is_not_whitespace`] pins it).
//! 3. **Character-class intersection survives.** [`ALPHANUMERIC`] keeps Java's
//!    `[a-zA-Z&&[^AEI]]` verbatim — Rust's regex syntax supports the same nested-class
//!    `&&` intersection.
//! 4. **Group numbering is re-derived via named groups.** Java reads `group(1)` (and, for
//!    the macro pattern, `group(2)`) off patterns assembled from shared fragments; since
//!    the fragments here are shared too, every group this port reads is **named**
//!    (`op`/`list`/`tok`/`name`/`num`/`val`/`ws`) and the fragments' own inner groups are
//!    non-capturing. A future edit to a fragment therefore cannot silently renumber a
//!    reader.
//!
//! # Ruling 3 (`predicate_env.rs`) — owned buffer, byte cursor, Java-compatible positions
//!
//! [`Predicate`] owns `predicate: String` (never a borrowed `&'a str`), and the 15 patterns
//! are compiled **once** into a process-wide [`OnceLock`] rather than rebuilt per haystack
//! — `Predicate.initializeMatchers()` (`:116-132`) has no Rust counterpart at all, because
//! a `regex_automata::meta::Regex` binds to no haystack. Nothing holds a borrow of
//! `predicate` across a statement that could mutate it: every arm of the scanning loop
//! copies the offsets (and any matched text) it needs out of the [`Captures`] before doing
//! anything else. That is what makes U4's `putMacro` — which rewrites `self.predicate`
//! mid-scan and resumes at the rewritten text — a drop-in method-body change.
//!
//! **Position units, decided here rather than left for a golden `error*` fixture to find
//! (Ruling 3 asks U3 to make this call explicitly): the cursor is a UTF-8 byte offset, but
//! every position that reaches a [`Token`] or an error message is converted to a UTF-16
//! code-unit offset**, via [`Predicate::java_offset`]. Java's `Matcher` offsets are UTF-16
//! indices, and Walnut prints them (`"char at " + position`) in `error*` fixture text and in
//! `EvalDef.compute`'s catch block. The two units agree for ASCII — every realistic Walnut
//! predicate — and diverge for exactly the two non-ASCII characters the grammar allows
//! (˜ U+02DC and ◌̃ U+0303, both negation, 2 bytes / 1 UTF-16 unit each). Converting is a
//! handful of lines and makes the divergence zero rather than "documented"; the alternative
//! (report byte offsets, document the drift) would silently shift every position after a
//! `˜` in a query. `real_starting_position` is therefore also a UTF-16 offset, which U4's
//! nested-`Predicate` construction sites must respect.
//!
//! # Ported quirks (see `docs/WALNUT-BUGS.md`, WB-015)
//!
//! Three token-position defects in these methods are ported **verbatim**, not fixed:
//! the parenthesis tokens record the pre-whitespace cursor rather than their own offset;
//! quantifier operators and their quantified variables omit `realStartingPosition`
//! entirely (so a quantifier inside a function argument/macro reports a position in the
//! wrong coordinate space); and every variable in one quantifier's list shares the
//! list's start position. All three are observable in Walnut's own error text. Each is
//! flagged at its call site below and pinned by a test.

use std::sync::OnceLock;

use regex_automata::meta::Regex;
use regex_automata::util::captures::Captures;
use regex_automata::{Anchored, Input};

use wr_core::numsys::{normalize_number_system_token, MSD_2};
use wr_core::util::{generic_list_string, is_java_whitespace, parse_big_integer, parse_int};

use crate::predicate_env::{PredicateEnv, PredicateEnvError};
use crate::token::{
    symbols, AlphabetLetter, Function, NumberLiteral, Operator, Token, TokenError, Variable, Word,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure the tokenizer itself can report.
///
/// Module-local per this crate's established idiom ([`TokenError`], [`PredicateEnvError`],
/// `wr_core::numsys::NumSysError`) rather than one unified `WalnutError` — see the Phase 3
/// plan's resolved gap #8. Every variant renders `WalnutException`'s verbatim message
/// text, since Tier 1's `error*` fixtures compare it.
#[derive(Debug)]
pub enum LexError {
    /// `Predicate.java:163-166` — thrown when `E`/`A`/`I` is not followed by something
    /// `PATTERN_FOR_LIST_OF_QUANTIFIED_VARIABLES` accepts. Note Java reports
    /// `realStartingPosition + index`, i.e. the position where scanning for the operator
    /// *began* (before any leading whitespace the pattern consumed), not the operator's
    /// own offset — preserved.
    QuantifierRequiresVariableList { op: String, position: usize },
    /// `WalnutException.operatorMissing(int)` — "An operator is missing: char at N".
    /// Raised when a value token (word/function/macro/variable/number/alphabet letter)
    /// follows another value token with no operator between them.
    OperatorMissing { position: usize },
    /// `WalnutException.undefinedToken(int)` — "Undefined token: char at N". The scanning
    /// loop's final `else`: nothing matches at the cursor.
    UndefinedToken { position: usize },
    /// Raised by [`Operator::push_onto`] (the shunting yard), the final operator-stack
    /// drain, or (as of U4) [`Word::new`]/[`Function::new`]'s arity check — in practice
    /// always [`TokenError::UnbalancedParenthesis`] or
    /// [`TokenError::WrongArgumentArity`].
    Token(TokenError),
    /// A [`PredicateEnv`] lookup failed: [`PredicateEnv::number_system`] (relational,
    /// arithmetic and number-literal tokens each resolve the *currently active* number
    /// system at construction time, exactly as Java's
    /// `NumberSystem.getComputeIfAbsent(currentNumberSystem)` does), or, as of U4,
    /// [`PredicateEnv::word`]/[`PredicateEnv::function`]/[`PredicateEnv::macro_text`].
    Env(PredicateEnvError),
    /// `WalnutException.unbalancedBracket(int)` (`putWord`, `:298`) — "unbalanced
    /// bracket: char at N". Ported for fidelity with Java's defensive check, but this
    /// port's implementation of the bracket-matching loop (a depth counter over a
    /// pre-sliced index text, rather than Java's character-by-character `Stack<Character>`)
    /// makes the check's own precondition ("a `]` was seen while the bracket stack was
    /// already empty") structurally unreachable, exactly as it is in Java: the moment
    /// depth would reach zero, the scan finalizes and stops looking at more characters
    /// from the same bracket run, so a second, deeper pop can never observe an empty
    /// stack either. Traced by hand (both here and in Java) rather than asserted away.
    UnbalancedBracket { position: usize },
    /// `WalnutException.internalMacro(int)` (`parseParenthesizedArguments`, `:353-355`)
    /// — "a function/macro cannot be called from inside another function/macro's
    /// argument list: char at N". Raised the instant a `#`/`$` is seen anywhere in a
    /// macro/function call's argument text (not just at the start).
    InternalMacroInArgument { position: usize },
    /// `putWord`'s per-index emptiness check (`:326-328`, no `WalnutException.` helper —
    /// an inline `new WalnutException(...)`): "index N of the word NAME cannot be empty:
    /// char at POS". **WB-017** (`docs/WALNUT-BUGS.md`): `POS` is `matcher.start(1)`,
    /// i.e. the WORD's own name-group offset within *this* predicate — NOT adjusted by
    /// `realStartingPosition`, unlike every other position in this file (confirmed
    /// empirically: nesting the empty index inside a function argument reports the
    /// position *within the nested predicate's own coordinate space*, not the true
    /// absolute offset). `position` here is therefore [`Self::java_offset`]-converted
    /// but NOT [`Self::position`]-adjusted — ported verbatim, not fixed.
    EmptyWordIndex {
        index: usize,
        name: String,
        position: usize,
    },
    /// `putFunction`'s per-argument emptiness check (`:479-483`): "argument N of the
    /// function NAME cannot be empty: char at POS". Same WB-017 position defect as
    /// [`Self::EmptyWordIndex`] (`POS` is the un-adjusted `matcher.start(1)`), a
    /// different call site of the identical bug.
    EmptyFunctionArgument {
        index: usize,
        name: String,
        position: usize,
    },
    /// **WB-019** (`docs/WALNUT-BUGS.md`): `putMacro`'s `%N` argument substitution
    /// (`Predicate.java:436`) is `String.replaceAll("%" + arg, arguments.get(arg))`, so
    /// the REPLACEMENT text — a macro call's raw argument, verbatim user input — is run
    /// through `java.util.regex.Matcher`'s replacement-string parsing, which gives `$`
    /// and `\` special meaning `WalnutException`'s machinery never sees or catches: a
    /// real, UNCAUGHT `IllegalArgumentException`/`IndexOutOfBoundsException`, confirmed
    /// empirically against `walnut-java`. `$`/`#` can never reach this point (blocked
    /// earlier by [`Self::InternalMacroInArgument`]), so `message` in practice is always
    /// the backslash-escaping half of the quirk (a lone trailing `\` in an argument).
    /// Ported as a recoverable `Result::Err` — like [`crate::expr::ExprError`]'s WB-013
    /// entry, a real Java unchecked exception that `Prover`'s top-level `catch
    /// (RuntimeException)` recovers from, not a Rust `panic!` that would abort this
    /// process outright with no equivalent boundary (yet).
    MacroArgumentReplacementError { message: &'static str },
}

impl From<TokenError> for LexError {
    fn from(e: TokenError) -> Self {
        LexError::Token(e)
    }
}

impl From<PredicateEnvError> for LexError {
    fn from(e: PredicateEnvError) -> Self {
        LexError::Env(e)
    }
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Verbatim `Predicate.java:163-165`.
            LexError::QuantifierRequiresVariableList { op, position } => write!(
                f,
                "Operator {op} requires a list of variables: char at {position}"
            ),
            // Verbatim `WalnutException.operatorMissing`.
            LexError::OperatorMissing { position } => {
                write!(f, "An operator is missing: char at {position}")
            }
            // Verbatim `WalnutException.undefinedToken`.
            LexError::UndefinedToken { position } => {
                write!(f, "Undefined token: char at {position}")
            }
            LexError::Token(e) => write!(f, "{e}"),
            LexError::Env(e) => write!(f, "{e}"),
            // Verbatim `WalnutException.unbalancedBracket`.
            LexError::UnbalancedBracket { position } => {
                write!(f, "unbalanced bracket: char at {position}")
            }
            // Verbatim `WalnutException.internalMacro`.
            LexError::InternalMacroInArgument { position } => write!(
                f,
                "a function/macro cannot be called from inside another function/macro's \
                 argument list: char at {position}"
            ),
            // Verbatim inline `WalnutException` text from `putWord` (WB-017).
            LexError::EmptyWordIndex {
                index,
                name,
                position,
            } => write!(
                f,
                "index {index} of the word {name} cannot be empty: char at {position}"
            ),
            // Verbatim inline `WalnutException` text from `putFunction` (WB-017).
            LexError::EmptyFunctionArgument {
                index,
                name,
                position,
            } => write!(
                f,
                "argument {index} of the function {name} cannot be empty: char at {position}"
            ),
            // Not a `WalnutException` at all -- an uncaught Java `RuntimeException` from
            // `Matcher.appendReplacement` (WB-019); `message` is that exception's own
            // verbatim text.
            LexError::MacroArgumentReplacementError { message } => write!(
                f,
                "{message} (uncaught Java exception from Predicate.putMacro's %N \
                 substitution, see docs/WALNUT-BUGS.md WB-019)"
            ),
        }
    }
}

impl std::error::Error for LexError {}

// ---------------------------------------------------------------------------
// The pattern table (`Predicate.java:72-108`)
// ---------------------------------------------------------------------------

/// `Predicate.ALPHANUMERIC` (`:72`) — "Alphanumeric, but not starting with reserved
/// letters A,E,I". Java's capturing group is dropped (every pattern that embeds this
/// fragment names its own group instead — Ruling 2, divergence 4); the class
/// intersection `&&` and the ASCII-only `\w` are otherwise verbatim.
const ALPHANUMERIC: &str = r"[a-zA-Z&&[^AEI]](?-u:\w)*";

/// `Predicate.WHITESPACE` (`:74`) — `\s*`, ASCII-only (Ruling 2, divergence 2).
const WHITESPACE: &str = r"(?-u:\s)*";

/// The 15 compiled patterns. Java holds 15 `Pattern` statics **plus** 15 per-instance
/// `Matcher`s rebound to each new predicate string; a `meta::Regex` binds to no haystack,
/// so the `Matcher` half (and `initializeMatchers()` with it) simply does not exist here.
struct Patterns {
    logical_operators: Regex,
    list_of_quantified_variables: Regex,
    relational_operators: Regex,
    arithmetic_operators: Regex,
    number_system: Regex,
    word: Regex,
    word_with_delimiter: Regex,
    function: Regex,
    macro_call: Regex,
    variable: Regex,
    number_literal: Regex,
    alphabet_letter: Regex,
    left_parenthesis: Regex,
    right_parenthesis: Regex,
    whitespace: Regex,
    /// `PATTERN_LEFT_BRACKET` (`Predicate.java:110`) — used exclusively by `putWord`'s
    /// index-scanning loop to detect a CHAINED index bracket (`T[i][j]`) immediately
    /// after one closes. Not part of the main scanning chain (U3's table), so it is not
    /// tried by [`Predicate::tokenize_and_compute_post_order`]'s `if`/`else if` chain.
    left_bracket: Regex,
}

/// Compiles one pattern, panicking on failure: these are compile-time-constant strings,
/// so a failure is an internal-invariant violation, not user input (this crate's
/// established idiom — see `token.rs`'s `relational_op_from_symbol`).
fn compile(pattern: &str) -> Regex {
    Regex::new(pattern)
        .unwrap_or_else(|e| panic!("Predicate pattern {pattern:?} must compile: {e}"))
}

impl Patterns {
    fn compile_all() -> Patterns {
        Patterns {
            // `PATTERN_FOR_LOGICAL_OPERATORS` (`:89-90`) + `LOGICAL_OPERATORS` (`:80`),
            // minus the `(?<!\.)` look-behind (Ruling 2, divergence 1 — emulated in
            // `find_logical_operator`). The alternation order is Java's, verbatim.
            // Built by concatenation rather than `format!` so the two Unicode tilde
            // spellings can be written as Rust escapes without going through format-string
            // parsing.
            logical_operators: compile(
                &(WHITESPACE.to_string() + "(?<op>`|\\^|&|~|\\||=>|<=>|E|A|I|\u{02dc}|\u{0303})"),
            ),
            // `PATTERN_FOR_LIST_OF_QUANTIFIED_VARIABLES` (`:91-92`). Java's inner groups
            // are unused; only the outer one (its `group(1)`) is read, so it alone is
            // named. Note additional variables need a COMMA — the pattern has no
            // whitespace-only separator — even though `handleQuantifier` then splits on
            // `(\s|,)+`.
            list_of_quantified_variables: compile(&format!(
                "{ws}(?<list>(?:{ws}{alnum}{ws})(?:{ws},{ws}{alnum}{ws})*)",
                ws = WHITESPACE,
                alnum = ALPHANUMERIC
            )),
            // `PATTERN_FOR_RELATIONAL_OPERATORS` (`:93`) + `RELATIONAL_OPERATORS` (`:78`).
            // Alternation order matters: `>=` before `>` and `<=` before `<`, so a
            // leftmost-first engine never splits a two-character operator.
            relational_operators: compile(&format!("{ws}(?<op>>=|<=|<|>|=|!=)", ws = WHITESPACE)),
            // `PATTERN_FOR_ARITHMETIC_OPERATORS` (`:94`) + `ARITHMETIC_OPERATORS` (`:79`).
            arithmetic_operators: compile(&format!(r"{ws}(?<op>[_/*+\-])", ws = WHITESPACE)),
            // `PATTERN_FOR_NUMBER_SYSTEM` (`:96`) + `NUMBER_SYSTEM` (`:82-85`). Java's
            // `group(1)` (`R_NUMBER_SYSTEM_TOKEN`, `:97`) is the whole token after `?`,
            // named `tok` here. The first alternative cannot match a bare `msd`/`lsd` (it
            // requires at least one `\d+`/`\w+` after the optional `_`), so those fall
            // through to the second alternative and normalize to `msd_2`/`lsd_2`.
            number_system: compile(&format!(
                r"{ws}\?(?<tok>(?:(?:msd|lsd)(?:_?(?:(?-u:\d)+|(?-u:\w)+))|(?:(?-u:\d)+|(?-u:\w)+)))",
                ws = WHITESPACE
            )),
            // `PATTERN_FOR_WORD` (`:99`).
            word: compile(&format!(
                r"{ws}(?<name>{alnum}){ws}\[",
                ws = WHITESPACE,
                alnum = ALPHANUMERIC
            )),
            // `PATTERN_FOR_WORD_WITH_DELIMITER` (`:100`) — note its name class is
            // `[a-zA-Z]\w*`, NOT `ALPHANUMERIC`: a leading `.` is exactly what lets a word
            // name start with the reserved `A`/`E`/`I` (`.AUTOMATON[..]`, `.EVEN[..]`).
            word_with_delimiter: compile(&format!(
                r"{ws}\.(?<name>[a-zA-Z](?-u:\w)*){ws}\[",
                ws = WHITESPACE
            )),
            // `PATTERN_FOR_FUNCTION` (`:101`).
            function: compile(&format!(
                r"{ws}\$(?<name>{alnum}){ws}\(",
                ws = WHITESPACE,
                alnum = ALPHANUMERIC
            )),
            // `PATTERN_FOR_MACRO` (`:102`) — the ONE pattern whose leading whitespace is
            // captured (Java's `group(1)`, re-emitted by `putMacro` into the rewritten
            // predicate, `:442`); its name is Java's `group(2)`.
            macro_call: compile(&format!(
                r"(?<ws>{ws})#(?<name>{alnum}){ws}\(",
                ws = WHITESPACE,
                alnum = ALPHANUMERIC
            )),
            // `PATTERN_FOR_VARIABLE` (`:103`).
            variable: compile(&format!(
                "{ws}(?<name>{alnum})",
                ws = WHITESPACE,
                alnum = ALPHANUMERIC
            )),
            // `PATTERN_FOR_NUMBER_LITERAL` (`:104`).
            number_literal: compile(&format!(r"{ws}(?<num>(?-u:\d)+)", ws = WHITESPACE)),
            // `PATTERN_FOR_ALPHABET_LETTER` (`:105`) — group 1 (`val`) deliberately
            // includes the sign AND any whitespace around it; `UtilityMethods.parseInt`
            // strips the whitespace back out.
            alphabet_letter: compile(&format!(
                r"{ws}@(?<val>{ws}(?:[+\-])?{ws}(?-u:\d)+)",
                ws = WHITESPACE
            )),
            // `PATTERN_FOR_LEFT_PARENTHESIS`/`PATTERN_FOR_RIGHT_PARENTHESIS` (`:106-107`).
            left_parenthesis: compile(&format!(r"{ws}\(", ws = WHITESPACE)),
            right_parenthesis: compile(&format!(r"{ws}\)", ws = WHITESPACE)),
            // `PATTERN_FOR_WHITESPACE` (`:108`) — `\s+`, one or more (unlike the `\s*`
            // fragment every other pattern is prefixed with).
            whitespace: compile(r"(?-u:\s)+"),
            // `PATTERN_LEFT_BRACKET` (`:110`).
            left_bracket: compile(&format!(r"{ws}\[", ws = WHITESPACE)),
        }
    }
}

/// The process-wide compiled pattern table. [`OnceLock`] rather than a `LazyLock` static
/// because this workspace's declared `rust-version` is 1.75 and `LazyLock` stabilized in
/// 1.80.
fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(Patterns::compile_all)
}

/// `Matcher.find(int index)` on a `\G`-anchored pattern: match starting at **exactly**
/// `at`, never scanning forward (`predicate_env.rs`'s Ruling 2).
///
/// Java's `find(int)` resets the matcher and sets `\G`'s anchor to the search start, so
/// `\G` pins the match to `at`; `Input::span(at..).anchored(Anchored::Yes)` is the exact
/// equivalent. Returns the [`Captures`], which hold only byte offsets — no borrow of
/// `hay` survives the call, which is what lets U4's `putMacro` rewrite the buffer.
fn find_at(re: &Regex, hay: &str, at: usize) -> Option<Captures> {
    let mut caps = re.create_captures();
    let input = Input::new(hay).span(at..hay.len()).anchored(Anchored::Yes);
    re.search_captures(&input, &mut caps);
    if caps.is_match() {
        Some(caps)
    } else {
        None
    }
}

/// `MATCHER_FOR_LOGICAL_OPERATORS.find(index)`, with `LOGICAL_OPERATORS`'s `(?<!\.)`
/// look-behind (`Predicate.java:80`) emulated — Rust's regex engines reject look-around
/// outright (pinned by `predicate_env.rs`'s
/// `lookbehind_is_unsupported_and_must_be_emulated`).
///
/// The emulation is "match without the look-behind, then reject if the byte immediately
/// before the operator is `.`", with *no* preceding byte counting as a pass (Java's
/// zero-width look-behind succeeds at offset 0). Rejecting outright is **exactly**
/// equivalent to Java's backtracking, not merely close: the only freedom Java's engine has
/// after a failed look-behind is to let the leading `\s*` consume a different number of
/// characters, and for any non-zero count the character before the operator would be
/// whitespace — so if `\s*` consuming nothing put the operator right after a `.`, that `.`
/// sits at the cursor itself, and no longer `\s*` run can start there.
///
/// Note that the guard inspects `hay` *before* `at`, exactly as Java's look-behind does:
/// Java's `find(int)` only moves the search start, it does not restrict the region, so the
/// character before the cursor is still visible to `(?<!\.)`.
fn find_logical_operator(hay: &str, at: usize) -> Option<Captures> {
    let caps = find_at(&patterns().logical_operators, hay, at)?;
    let op = caps.get_group_by_name("op")?;
    if op.start > 0 && hay.as_bytes()[op.start - 1] == b'.' {
        return None;
    }
    Some(caps)
}

// ---------------------------------------------------------------------------
// Predicate
// ---------------------------------------------------------------------------

/// `Main/Predicate.java` — a predicate string, tokenized into postfix order.
///
/// Construction *is* tokenization, as in Java (the constructor calls
/// `tokenizeAndComputePostOrder`), so a `Predicate` value always holds a fully scanned
/// token stream — or construction failed with a [`LexError`].
#[derive(Debug, Clone)]
pub struct Predicate {
    /// `Predicate.predicate` (`:50`). **Owned**, never borrowed, per Ruling 3: U4's
    /// `putMacro` rewrites this string mid-scan.
    predicate: String,
    /// `Predicate.postOrder` (`:51`).
    post_order: Vec<Token>,
    /// `Predicate.operatorStack` (`:52`). Kept as a field (rather than a local of the
    /// scanning loop) to mirror Java; it is empty once construction returns, because the
    /// final drain empties it.
    operator_stack: Vec<Operator>,
    /// `Predicate.realStartingPosition` (`:53`) — the offset of this predicate's text
    /// within the user's original query, added to every reported position. A **UTF-16**
    /// offset (see the module docs).
    real_starting_position: usize,
    /// `Predicate.defaultNumberSystem` (`:54`) — already normalized (`msd_2`, …).
    default_number_system: String,
}

impl Predicate {
    /// `Predicate(String predicate)` (`:112-114`): `this(MSD_2, predicate, 0)`.
    pub fn new(env: &dyn PredicateEnv, predicate: &str) -> Result<Predicate, LexError> {
        Predicate::with_context(env, MSD_2, predicate, 0)
    }

    /// `Predicate(String defaultNumberSystem, String predicate, int realStartingPosition)`
    /// (`:134-146`).
    ///
    /// Java's is private, reached only from `putWord`/`putFunction`'s nested-`Predicate`
    /// construction (U4); it is public here because it is also the honest way to test the
    /// number-system default and position threading directly.
    ///
    /// `real_starting_position` must be a **UTF-16 code-unit** offset into the enclosing
    /// query, matching what Java's `Matcher` offsets are — see the module docs.
    pub fn with_context(
        env: &dyn PredicateEnv,
        default_number_system: &str,
        predicate: &str,
        real_starting_position: usize,
    ) -> Result<Predicate, LexError> {
        let mut p = Predicate {
            predicate: predicate.to_string(),
            post_order: Vec::new(),
            operator_stack: Vec::new(),
            real_starting_position,
            default_number_system: default_number_system.to_string(),
        };
        // `if (PATTERN_WHITESPACE.matcher(predicate).matches()) return;` (`:143`) --
        // `ParseMethods.PATTERN_WHITESPACE` is `^\s*$`, i.e. "every character is (ASCII)
        // whitespace", which the empty string also satisfies. An all-whitespace predicate
        // is NOT an error: it yields an empty post-order, which `putWord`/`putFunction`
        // (U4) then diagnose as an empty index/argument.
        if p.predicate.bytes().all(is_java_whitespace) {
            return Ok(p);
        }
        p.tokenize_and_compute_post_order(env)?;
        Ok(p)
    }

    /// `Predicate.getPostOrder()` (`:513-515`).
    pub fn post_order(&self) -> &[Token] {
        &self.post_order
    }

    /// The token stream, by value — for callers (U11's executor) that consume it.
    pub fn into_post_order(self) -> Vec<Token> {
        self.post_order
    }

    /// `Predicate.predicate` (`:50`) — the (possibly macro-rewritten, once U4 lands)
    /// text this predicate was tokenized from. `PredicateTest` asserts on it directly
    /// after a macro expansion, which is why it is exposed.
    pub fn predicate(&self) -> &str {
        &self.predicate
    }

    /// The offset this predicate's text starts at within the enclosing query
    /// (`Predicate.realStartingPosition`, `:53`).
    pub fn real_starting_position(&self) -> usize {
        self.real_starting_position
    }

    /// `Predicate.defaultNumberSystem` (`:54`).
    pub fn default_number_system(&self) -> &str {
        &self.default_number_system
    }

    /// A byte offset into [`Self::predicate`], converted to the **UTF-16 code-unit**
    /// offset a Java `Matcher` would have reported (see the module docs for why this port
    /// converts rather than documenting a drift).
    ///
    /// Deliberately implemented by walking `char_indices` rather than slicing
    /// `predicate[..byte_offset]`: every offset this is called with comes from a regex
    /// match boundary and is therefore a `char` boundary, but a panic-free implementation
    /// costs nothing and cannot turn a future off-by-one into a crash. Linear in the
    /// prefix length — irrelevant next to the automaton work each token triggers.
    fn java_offset(&self, byte_offset: usize) -> usize {
        self.predicate
            .char_indices()
            .take_while(|(i, _)| *i < byte_offset)
            .map(|(_, c)| c.len_utf16())
            .sum()
    }

    /// Java's `realStartingPosition + <matcher offset>`, the form nearly every token
    /// construction and error message in `tokenizeAndComputePostOrder` uses.
    fn position(&self, byte_offset: usize) -> usize {
        self.real_starting_position + self.java_offset(byte_offset)
    }

    /// `Predicate.tokenizeAndComputePostOrder()` (`:148-253`) — the scanning loop and the
    /// shunting-yard drain that follows it.
    ///
    /// The `if`/`else if` chain below is Java's **exact** priority order; it is not an
    /// arbitrary ordering and several pairs of patterns overlap: `<=>` (logical) before
    /// `<=` (relational); `T[i]` (word) before `T` (variable); `$f(`/`#m(` before the bare
    /// `(`; `@5` (alphabet letter) is only reached because `@` is not a digit, so the
    /// number-literal pattern above it cannot match first.
    fn tokenize_and_compute_post_order(&mut self, env: &dyn PredicateEnv) -> Result<(), LexError> {
        let p = patterns();
        // `Stack<String> numberSystems` (`:149-151`): the bottom entry is the default,
        // and a literal "(" is pushed as a scope marker by every left parenthesis.
        let mut number_systems: Vec<String> = vec![self.default_number_system.clone()];
        let mut current_number_system = self.default_number_system.clone();
        let mut index = 0usize;
        let mut last_token_was_operator = true;

        while index < self.predicate.len() {
            if let Some(caps) = find_logical_operator(&self.predicate, index) {
                // `:157-174`
                last_token_was_operator = true;
                let whole = caps.get_match().expect("matched").range();
                let op_span = caps.get_group_by_name("op").expect("group `op` matched");
                let op_str = self.predicate[op_span.range()].to_string();
                if op_str == symbols::EXISTS
                    || op_str == symbols::FORALL
                    || op_str == symbols::INFINITE
                {
                    // `:162-167`: the variable list must start EXACTLY where the
                    // quantifier ended. Note the reported position is the pre-whitespace
                    // cursor (`index`), not the operator's own offset.
                    let vars = find_at(&p.list_of_quantified_variables, &self.predicate, whole.end)
                        .ok_or_else(|| LexError::QuantifierRequiresVariableList {
                            op: op_str.clone(),
                            position: self.position(index),
                        })?;
                    let list_span = vars
                        .get_group_by_name("list")
                        .expect("group `list` matched");
                    let list_match = vars.get_match().expect("matched").range();
                    index = self.handle_quantifier(
                        &op_str,
                        whole.start,
                        list_match.start,
                        list_span.range(),
                        list_match.end,
                    );
                } else {
                    let op = Operator::logical_connective(self.position(op_span.start), op_str);
                    Token::Operator(op)
                        .push_onto(&mut self.post_order, &mut self.operator_stack)?;
                    index = whole.end;
                }
            } else if let Some(caps) = find_at(&p.relational_operators, &self.predicate, index) {
                // `:175-181`
                last_token_was_operator = true;
                let whole = caps.get_match().expect("matched").range();
                let op_span = caps.get_group_by_name("op").expect("group `op` matched");
                let op_str = self.predicate[op_span.range()].to_string();
                // `NumberSystem.getComputeIfAbsent(currentNumberSystem)` (`:178`) -- the
                // number system is resolved HERE, at token-construction time, not at
                // `act()` time, and against whatever `?ns` directive is currently in scope.
                let ns = env.number_system(&current_number_system)?;
                let op = Operator::relational(self.position(op_span.start), op_str, ns);
                Token::Operator(op).push_onto(&mut self.post_order, &mut self.operator_stack)?;
                index = whole.end;
            } else if let Some(caps) = find_at(&p.arithmetic_operators, &self.predicate, index) {
                // `:182-188`
                last_token_was_operator = true;
                let whole = caps.get_match().expect("matched").range();
                let op_span = caps.get_group_by_name("op").expect("group `op` matched");
                let op_str = self.predicate[op_span.range()].to_string();
                let ns = env.number_system(&current_number_system)?;
                let op = Operator::arithmetic(self.position(op_span.start), op_str, ns);
                Token::Operator(op).push_onto(&mut self.post_order, &mut self.operator_stack)?;
                index = whole.end;
            } else if let Some(caps) = find_at(&p.word, &self.predicate, index) {
                // `:189-192`
                if !last_token_was_operator {
                    return Err(LexError::OperatorMissing {
                        position: self.position(index),
                    });
                }
                last_token_was_operator = false;
                index = self.put_word(env, &current_number_system, &caps, false)?;
            } else if let Some(caps) = find_at(&p.word_with_delimiter, &self.predicate, index) {
                // `:193-196`
                if !last_token_was_operator {
                    return Err(LexError::OperatorMissing {
                        position: self.position(index),
                    });
                }
                last_token_was_operator = false;
                index = self.put_word(env, &current_number_system, &caps, true)?;
            } else if let Some(caps) = find_at(&p.function, &self.predicate, index) {
                // `:197-200`
                if !last_token_was_operator {
                    return Err(LexError::OperatorMissing {
                        position: self.position(index),
                    });
                }
                last_token_was_operator = false;
                index = self.put_function(env, &current_number_system, &caps)?;
            } else if let Some(caps) = find_at(&p.macro_call, &self.predicate, index) {
                // `:201-203`. QUIRK, ported verbatim: unlike the word and function arms
                // above, this one never sets `lastTokenWasOperator = false` on its own
                // success path -- a macro expands into text that is re-scanned, so
                // whatever that text ends with will (or won't) set the flag itself.
                if !last_token_was_operator {
                    return Err(LexError::OperatorMissing {
                        position: self.position(index),
                    });
                }
                index = self.put_macro(env, &caps)?;
            } else if let Some(caps) = find_at(&p.variable, &self.predicate, index) {
                // `:204-209`
                if !last_token_was_operator {
                    return Err(LexError::OperatorMissing {
                        position: self.position(index),
                    });
                }
                last_token_was_operator = false;
                let whole = caps.get_match().expect("matched").range();
                let name_span = caps
                    .get_group_by_name("name")
                    .expect("group `name` matched");
                let name = self.predicate[name_span.range()].to_string();
                let t = Token::Variable(Variable::new(self.position(name_span.start), name));
                t.push_onto(&mut self.post_order, &mut self.operator_stack)?;
                index = whole.end;
            } else if let Some(caps) = find_at(&p.number_literal, &self.predicate, index) {
                // `:210-216`
                if !last_token_was_operator {
                    return Err(LexError::OperatorMissing {
                        position: self.position(index),
                    });
                }
                last_token_was_operator = false;
                let whole = caps.get_match().expect("matched").range();
                let num_span = caps.get_group_by_name("num").expect("group `num` matched");
                let text = self.predicate[num_span.range()].to_string();
                // Same eager `getComputeIfAbsent` as the operator arms (`:213`).
                let ns = env.number_system(&current_number_system)?;
                let t = Token::NumberLiteral(NumberLiteral::new(
                    self.position(num_span.start),
                    parse_big_integer(&text),
                    ns,
                ));
                t.push_onto(&mut self.post_order, &mut self.operator_stack)?;
                index = whole.end;
            } else if let Some(caps) = find_at(&p.alphabet_letter, &self.predicate, index) {
                // `:217-222`
                if !last_token_was_operator {
                    return Err(LexError::OperatorMissing {
                        position: self.position(index),
                    });
                }
                last_token_was_operator = false;
                let whole = caps.get_match().expect("matched").range();
                let val_span = caps.get_group_by_name("val").expect("group `val` matched");
                let text = self.predicate[val_span.range()].to_string();
                // `UtilityMethods.parseInt` strips the whitespace the pattern allowed
                // between `@`, the sign, and the digits.
                let t = Token::AlphabetLetter(AlphabetLetter::new(
                    self.position(val_span.start),
                    parse_int(&text),
                ));
                t.push_onto(&mut self.post_order, &mut self.operator_stack)?;
                index = whole.end;
            } else if let Some(caps) = find_at(&p.number_system, &self.predicate, index) {
                // `:223-227`. A `?ns` directive emits NO token: it only updates the
                // scanning state. `lastTokenWasOperator` is deliberately left alone (Java
                // does not touch it here), so `a ?msd_3 b` still reports a missing operator.
                let whole = caps.get_match().expect("matched").range();
                let tok_span = caps.get_group_by_name("tok").expect("group `tok` matched");
                let tmp = normalize_number_system_token(Some(&self.predicate[tok_span.range()]));
                number_systems.push(tmp.clone());
                current_number_system = tmp;
                index = whole.end;
            } else if let Some(caps) = find_at(&p.left_parenthesis, &self.predicate, index) {
                // `:228-232`. QUIRK (WB-015), ported verbatim: the position recorded is
                // `realStartingPosition + index` -- the cursor BEFORE the pattern's leading
                // `\s*` -- not the parenthesis's own offset. It is observable: an unclosed
                // `(` reports this position in "unbalanced parenthesis: char at N".
                let op = Operator::left_paren(self.position(index));
                Token::Operator(op).push_onto(&mut self.post_order, &mut self.operator_stack)?;
                number_systems.push("(".to_string());
                index = caps.get_match().expect("matched").end();
            } else if let Some(caps) = find_at(&p.right_parenthesis, &self.predicate, index) {
                // `:233-237`. Same pre-whitespace position quirk as the left parenthesis.
                let op = Operator::right_paren(self.position(index));
                Token::Operator(op).push_onto(&mut self.post_order, &mut self.operator_stack)?;
                current_number_system =
                    find_current_number_system(&mut number_systems, &self.default_number_system);
                index = caps.get_match().expect("matched").end();
            } else if let Some(caps) = find_at(&p.whitespace, &self.predicate, index) {
                // `:238-239`
                index = caps.get_match().expect("matched").end();
            } else {
                // `:241`
                return Err(LexError::UndefinedToken {
                    position: self.position(index),
                });
            }
        }

        // `:245-252` -- drain the operator stack; a surviving left parenthesis was never
        // closed.
        while let Some(op) = self.operator_stack.pop() {
            if op.is_left_parenthesis() {
                return Err(LexError::Token(TokenError::UnbalancedParenthesis {
                    position: op.position_in_predicate(),
                }));
            }
            self.post_order.push(Token::Operator(op));
        }
        Ok(())
    }

    /// `Predicate.handleQuantifier()` (`:276-285`): emit one multi-arity quantifier
    /// operator plus one [`Variable`] token per quantified variable, and return the offset
    /// just past the variable list.
    ///
    /// Parameters are the byte offsets Java reads off its two live matchers:
    /// `logical_match_start` is `MATCHER_FOR_LOGICAL_OPERATORS.start()`,
    /// `list_match_start`/`list_match_end` are the variable-list match's own bounds, and
    /// `list_group` is its `group(1)`.
    ///
    /// **Three position quirks, all ported verbatim (WB-015).** Java uses the raw matcher
    /// offsets here, so — unlike every other token in the scanning loop — (a)
    /// `realStartingPosition` is NOT added, putting the position in the wrong coordinate
    /// space for any nested predicate (a function argument, a macro expansion, a word
    /// index); (b) the offsets used are whole-match starts, i.e. they include the leading
    /// whitespace the patterns consumed; and (c) every variable in the list is given the
    /// SAME position (the list's start), not its own. All three are observable:
    /// `EvalDef.compute` appends `"\t: char at " + t.getPositionInPredicate()` to any error a
    /// token's `act()` raises, and a quantifier's `act()` can raise (`"Variable X in the list
    /// of quantified variables is not a free variable."`).
    ///
    /// The variables are pushed straight onto `post_order` rather than through
    /// [`Token::push_onto`], which is exactly what Java's inherited `Token.put(List<Token>)`
    /// (`Token.java:42-44`, `postOrder.add(this)`) does for a non-operator token — the
    /// difference is only that this avoids an infallible `Result` at the call site.
    fn handle_quantifier(
        &mut self,
        op_str: &str,
        logical_match_start: usize,
        list_match_start: usize,
        list_group: std::ops::Range<usize>,
        list_match_end: usize,
    ) -> usize {
        let list_text = self.predicate[list_group].to_string();
        let variables = split_variable_list(&list_text);
        // Quirks (a)+(b): whole-match start, and no `realStartingPosition`.
        let op = Operator::quantifier(
            self.java_offset(logical_match_start),
            op_str,
            variables.len(),
        );
        // `op.put(...)` cannot fail: a quantifier is pushed straight onto the operator
        // stack by the shunting yard (`Operator.put`'s immediate-push special case), and
        // only `RightParenthesis.put` can error.
        Token::Operator(op)
            .push_onto(&mut self.post_order, &mut self.operator_stack)
            .expect("a quantifier is pushed unconditionally and cannot fail");
        // Quirks (a)+(c): every variable gets the list's start position.
        let variable_position = self.java_offset(list_match_start);
        for var in variables {
            self.post_order
                .push(Token::Variable(Variable::new(variable_position, var)));
        }
        list_match_end
    }

    /// `Predicate.putWord(String, boolean)` (`:287-337`).
    ///
    /// `default_number_system` is the `currentNumberSystem` Java passes (it becomes each
    /// index `Predicate`'s own default number system); `caps` is the live word match
    /// (`MATCHER_FOR_WORD`/`MATCHER_FOR_WORD_WITH_DELIMITER`); the return value is the
    /// offset to resume scanning at.
    ///
    /// Scans against a CLONE of `self.predicate` (Ruling 3's "one allocation per
    /// `Predicate`" cost is irrelevant next to the automaton work each index triggers),
    /// so nothing here needs to juggle a borrow of `self.predicate` alongside the `&mut
    /// self.post_order`/`&mut self.operator_stack` writes at the end — `put_word` never
    /// mutates `self.predicate` at all (only [`Self::put_macro`] does).
    fn put_word(
        &mut self,
        env: &dyn PredicateEnv,
        default_number_system: &str,
        caps: &Captures,
        with_delimiter: bool,
    ) -> Result<usize, LexError> {
        let name_span = caps
            .get_group_by_name("name")
            .expect("group `name` matched");
        let name = self.predicate[name_span.range()].to_string();
        // `matcher.end()` (`:296`) -- the offset right after the opening `[`.
        let mut i = caps.get_match().expect("matched").end();

        // `new Automaton(Session.getReadFileForWordsLibrary(matcher.group(1) +
        // TXT_EXTENSION))` (`:295`). Java resolves the word/sequence automaton BEFORE
        // scanning for indices; a missing file is reported before any index-syntax
        // error would be, and this ordering matches that.
        let word_automaton = env.word(&name)?;
        let _ = with_delimiter; // only affects which pattern matched `caps`, not the body

        let text = self.predicate.clone();
        let mut indices: Vec<Predicate> = Vec::new();
        let mut depth: i32 = 1;
        let mut starting_position = i;

        while i < text.len() {
            let ch = text[i..].chars().next().expect("i < text.len()");
            if ch == ']' {
                // `bracketStack.isEmpty()` (`:298`) -- see `LexError::UnbalancedBracket`'s
                // docs on why this is structurally unreachable, ported anyway.
                if depth == 0 {
                    return Err(LexError::UnbalancedBracket {
                        position: self.position(i),
                    });
                }
                depth -= 1;
                if depth == 0 {
                    // `:302-303` -- finalize this index as a nested `Predicate`.
                    let index_text = &text[starting_position..i];
                    indices.push(Predicate::with_context(
                        env,
                        default_number_system,
                        index_text,
                        self.position(starting_position),
                    )?);
                    // `:305-311` -- a chained index (`T[i][j]`)?
                    if let Some(next) = find_at(&patterns().left_bracket, &text, i + 1) {
                        depth = 1;
                        i = next.get_match().expect("matched").end();
                        starting_position = i;
                        continue;
                    } else {
                        break;
                    }
                }
                // `:313` -- a nested `]` within a still-open index: just text.
            } else if ch == '[' {
                depth += 1;
            }
            i += ch.len_utf8();
        }
        // If the loop ran off the end without closing the LAST bracket (`i ==
        // text.len()`), that final unclosed attempt contributes nothing to `indices`
        // (WB-018: this can silently leave `indices.len()` matching the word's real
        // arity despite trailing garbage — see that entry).

        for (idx, p) in indices.iter().enumerate() {
            if p.post_order().is_empty() {
                // WB-017: `matcher.start(1)`, un-adjusted by `realStartingPosition`.
                return Err(LexError::EmptyWordIndex {
                    index: idx + 1,
                    name: name.clone(),
                    position: self.java_offset(name_span.start),
                });
            }
        }
        let index_count = indices.len();
        for p in indices {
            self.post_order.extend(p.into_post_order());
        }

        // `:333`: `new Word(realStartingPosition + matcher.start(1), matcher.group(1),
        // A, indices.size())` -- correctly `realStartingPosition`-adjusted, unlike the
        // empty-index check just above.
        let w = Word::new(
            self.position(name_span.start),
            name,
            word_automaton,
            index_count,
        )?;
        self.post_order.push(Token::Word(w));
        // `:335`: `return i + 1;`.
        Ok(i + 1)
    }

    /// `Predicate.putFunction(String)` (`:448-491`). See [`Self::put_word`] on the
    /// clone-and-scan strategy.
    fn put_function(
        &mut self,
        env: &dyn PredicateEnv,
        default_number_system: &str,
        caps: &Captures,
    ) -> Result<usize, LexError> {
        let name_span = caps
            .get_group_by_name("name")
            .expect("group `name` matched");
        let function_name = self.predicate[name_span.range()].to_string();
        let match_end = caps.get_match().expect("matched").end();

        // `Automaton.readAutomatonFromFile(functionName)` (`:451`).
        let automaton = env.function(&function_name)?;

        let parse_result = self.parse_parenthesized_arguments(match_end)?;

        let mut arguments: Vec<Predicate> = Vec::with_capacity(parse_result.arguments.len());
        for arg in &parse_result.arguments {
            arguments.push(Predicate::with_context(
                env,
                default_number_system,
                &arg.text,
                self.position(arg.start_pos),
            )?);
        }

        // `:466-468`: a single, empty (or whitespace-only) argument means zero
        // arguments, e.g. `$foo()`.
        if arguments.len() == 1 && arguments[0].post_order().is_empty() {
            arguments.remove(0);
        }

        let total_arguments = arguments.len();
        for (idx, p) in arguments.iter().enumerate() {
            if p.post_order().is_empty() && total_arguments > 1 {
                // WB-017: `matcher.start(1)`, un-adjusted by `realStartingPosition`.
                return Err(LexError::EmptyFunctionArgument {
                    index: idx + 1,
                    name: function_name.clone(),
                    position: self.java_offset(name_span.start),
                });
            }
        }
        for p in arguments {
            self.post_order.extend(p.into_post_order());
        }

        // `new NumberSystem(number_system)` (`Function.java:52`) — a FRESH, unmemoized
        // instance, deliberately NOT resolved through `env.number_system` (Ruling 1's
        // shared-`Rc` cache); see `Function`'s own doc comment in `token.rs` for why
        // that non-sharing is preserved rather than "fixed".
        let ns = wr_core::numsys::NumberSystem::new(default_number_system).map_err(|source| {
            PredicateEnvError::NumberSystem {
                name: default_number_system.to_string(),
                source,
            }
        })?;

        // `:485-487`: `new Function(defaultNumberSystem, realStartingPosition +
        // matcher.start(1), matcher.group(1), A, arguments.size())`.
        let f = Function::new(
            self.position(name_span.start),
            function_name,
            automaton,
            total_arguments,
            ns,
        )?;
        self.post_order.push(Token::Function(Box::new(f)));
        // `:490`: `return parseResult.endIndex + 1;`.
        Ok(parse_result.end_index + 1)
    }

    /// `Predicate.parseParenthesizedArguments(int)` (`:348-395`) — shared by
    /// [`Self::put_function`] and [`Self::put_macro`]. `start_index` is the byte offset
    /// right after the `(` that opens the argument list (already consumed by the
    /// caller's match). Read-only (`&self`): nothing here mutates the buffer, so both
    /// callers can hold `&mut self` around the call without any special-casing.
    fn parse_parenthesized_arguments(&self, start_index: usize) -> Result<ParseResult, LexError> {
        let text = &self.predicate;
        let mut depth: i32 = 1;
        let mut buf = String::new();
        let mut current_arg_start = start_index;
        let mut arguments = Vec::new();
        let mut i = start_index;

        while i < text.len() {
            let ch = text[i..].chars().next().expect("i < text.len()");
            // `:356-358` -- checked before `)`/`,` handling, exactly as Java orders it.
            if ch == '#' || ch == '$' {
                return Err(LexError::InternalMacroInArgument {
                    position: self.position(i),
                });
            }
            if ch == ')' {
                // `:363-365` -- see `LexError::UnbalancedBracket`'s docs for why this
                // "already empty" defensive check is structurally unreachable here too
                // (same shape, different delimiter): finalize-and-return happens the
                // instant `depth` reaches zero, so no later character in this call can
                // ever observe it already at zero.
                if depth == 0 {
                    return Err(LexError::Token(TokenError::UnbalancedParenthesis {
                        position: self.position(i),
                    }));
                }
                depth -= 1;
                if depth == 0 {
                    // `:369-372`.
                    arguments.push(ParsedArgument {
                        text: std::mem::take(&mut buf),
                        start_pos: current_arg_start,
                    });
                    return Ok(ParseResult {
                        arguments,
                        end_index: i,
                    });
                }
                buf.push(')');
            } else if ch == ',' {
                if depth == 1 {
                    // `:379-382` -- top-level comma: finalize one argument.
                    arguments.push(ParsedArgument {
                        text: std::mem::take(&mut buf),
                        start_pos: current_arg_start,
                    });
                    current_arg_start = i + 1;
                } else {
                    // `:384-385` -- nested comma: just text.
                    buf.push(',');
                }
            } else {
                buf.push(ch);
                if ch == '(' {
                    depth += 1;
                }
            }
            i += ch.len_utf8();
        }
        // `:391-393` -- ran off the end without closing the top-level `(`.
        Err(LexError::Token(TokenError::UnbalancedParenthesis {
            position: self.position(i),
        }))
    }

    /// `Predicate.putMacro()` (`:419-446`) — rewrites `self.predicate` in place and
    /// resumes scanning from the (preserved) leading whitespace, per `predicate_env.rs`'s
    /// Ruling 3. Every offset/string this needs is extracted from `caps`/computed before
    /// `self.predicate` is reassigned, so nothing holds a borrow of the OLD buffer across
    /// the rewrite.
    fn put_macro(&mut self, env: &dyn PredicateEnv, caps: &Captures) -> Result<usize, LexError> {
        let name_span = caps
            .get_group_by_name("name")
            .expect("group `name` matched");
        let name = self.predicate[name_span.range()].to_string();
        let ws_span = caps.get_group_by_name("ws").expect("group `ws` matched");
        let leading_ws = self.predicate[ws_span.range()].to_string();
        let whole = caps.get_match().expect("matched").range();
        let match_start = whole.start; // == ws_span.start
        let match_end = whole.end; // right after '('

        // `readMacroFile(matcher.group(2))` (`:422`).
        let mut macro_text = env.macro_text(&name)?;

        let parse_result = self.parse_parenthesized_arguments(match_end)?;

        // `:435-437`: `for (int arg = arguments.size() - 1; arg >= 0; arg--)` --
        // DESCENDING order, ported verbatim. This is load-bearing, not stylistic: `%1`
        // is a literal-text substring of `%10`, so substituting `%1` first would mangle
        // any later `%10` before it is ever reached, and — for the same reason —
        // `%10`'s own replacement text is scanned again on `%1`'s pass, so a
        // replacement that happens to CONTAIN `%1` gets a second, unintended
        // substitution. Both are Java's real behavior, reproduced by literally
        // replaying its loop rather than trying to special-case around it.
        for (arg_index, arg) in parse_result.arguments.iter().enumerate().rev() {
            macro_text =
                java_replace_all_literal(&macro_text, &format!("%{arg_index}"), &arg.text)?;
        }

        // `:441-442`: `predicate = predicate.substring(0, matcher.start()) +
        // matcher.group(1) + macro + predicate.substring(parseResult.endIndex + 1);`
        let tail = &self.predicate[parse_result.end_index + 1..];
        let mut new_predicate =
            String::with_capacity(match_start + leading_ws.len() + macro_text.len() + tail.len());
        new_predicate.push_str(&self.predicate[..match_start]);
        new_predicate.push_str(&leading_ws);
        new_predicate.push_str(&macro_text);
        new_predicate.push_str(tail);
        self.predicate = new_predicate;

        // `:445`: `return matcher.start();` -- resume scanning from the (re-inserted)
        // leading whitespace, so the SAME buffer is re-tokenized from there. No
        // `initializeMatchers()` counterpart is needed (see the module docs' Ruling 2):
        // `regex_automata`'s `Input` is per-call, not bound to a haystack.
        Ok(match_start)
    }
}

/// `Predicate.ParsedArgument` (`record ParsedArgument(String text, int startPos)`,
/// `:344-346`). `start_pos` is a **byte** offset into the predicate the argument was
/// parsed from — converted to a UTF-16, `real_starting_position`-adjusted offset only at
/// the point of use ([`Predicate::position`]), matching every other raw offset in this
/// file (Ruling 3's "corollary for U4").
struct ParsedArgument {
    text: String,
    start_pos: usize,
}

/// `Predicate.ParseResult` (`:412-415`).
struct ParseResult {
    arguments: Vec<ParsedArgument>,
    end_index: usize,
}

/// `String.replaceAll(literalPattern, replacement)` as `Predicate.putMacro` uses it
/// (`:436`): `literal_pattern` is always `"%" + N` for a non-negative `N` — plain
/// digits, no regex metacharacters — so no real regex compilation is needed here. But
/// the REPLACEMENT text (`arguments.get(arg)`, a macro call's raw argument) is still run
/// through `java.util.regex.Matcher`'s replacement-string parsing, which gives `$`/`\`
/// special meaning — see [`expand_java_replacement`] and **WB-019**
/// (`docs/WALNUT-BUGS.md`).
fn java_replace_all_literal(
    haystack: &str,
    literal_pattern: &str,
    replacement: &str,
) -> Result<String, LexError> {
    let mut out = String::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(idx) = rest.find(literal_pattern) {
        out.push_str(&rest[..idx]);
        out.push_str(&expand_java_replacement(replacement, literal_pattern)?);
        rest = &rest[idx + literal_pattern.len()..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `Matcher.appendReplacement`'s replacement-string parsing (OpenJDK's
/// `java.util.regex.Matcher`), specialized to a pattern with ZERO capturing groups —
/// every pattern [`java_replace_all_literal`] is ever called with is `"%" + N`, plain
/// digits — so group 0 (`whole_match`, i.e. the entire `%N` token) is the only valid
/// group reference. `$1`..`$9` always fail (`groupCount() == 0 < 1`), matching a real,
/// empirically-confirmed `IndexOutOfBoundsException`; a lone trailing `\` also fails
/// (`IllegalArgumentException`), also confirmed empirically. **WB-019**, ported
/// verbatim: `$`/`#` can never reach this function in practice (blocked earlier by
/// [`LexError::InternalMacroInArgument`]), so the `\`-escaping arm is the one real
/// callers exercise; the `$`-group-reference arm is implemented anyway, in the same
/// spirit as `putWord`'s unreachable-but-ported [`LexError::UnbalancedBracket`] check,
/// and is unreachable through this crate's own call graph today. **Its fidelity is
/// narrower than that comparison implies**, flagged during adversarial review: real
/// `Matcher.appendReplacement` extends a `$NNN` group reference digit-by-digit for as
/// long as the accumulated number stays a valid group index (here, `groupCount()==0`,
/// so it can extend through any number of leading `0`s before failing on the first
/// nonzero digit), then treats any REMAINING digits as literal text — e.g. real Java's
/// `"$007"` consumes both `0`s into the group reference and appends `whole_match+"7"`.
/// This arm consumes exactly one digit after `$` (`"$007"` here would append
/// `whole_match+"07"` instead). Since the whole arm is provably dead code, this
/// divergence has zero live effect — recorded here rather than silently left
/// undocumented, since a future change that makes this arm reachable must fix this
/// first.
fn expand_java_replacement(replacement: &str, whole_match: &str) -> Result<String, LexError> {
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::with_capacity(replacement.len());
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i += 1;
                let c = *chars
                    .get(i)
                    .ok_or(LexError::MacroArgumentReplacementError {
                        message: "character to be escaped is missing",
                    })?;
                out.push(c);
                i += 1;
            }
            '$' => {
                i += 1;
                let d = *chars
                    .get(i)
                    .ok_or(LexError::MacroArgumentReplacementError {
                        message: "Illegal group reference: group index is missing",
                    })?;
                if !d.is_ascii_digit() {
                    return Err(LexError::MacroArgumentReplacementError {
                        message: "Illegal group reference",
                    });
                }
                // `groupCount() == 0`: any FIRST digit other than `0` already exceeds
                // it (Java's own digit-extension loop can only ever shrink the
                // candidate group number back down to this same first digit, never
                // grow past it while staying <= 0), so only `$0` (the whole match) is
                // ever valid here.
                if d != '0' {
                    return Err(LexError::MacroArgumentReplacementError {
                        message: "No group",
                    });
                }
                i += 1;
                out.push_str(whole_match);
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

impl std::fmt::Display for Predicate {
    /// `Predicate.toString()` (`:517-519`):
    /// `UtilityMethods.genericListString(postOrder, ":")`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", generic_list_string(&self.post_order, ":"))
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// `Predicate.findCurrentNumberSystem(Stack<String>)` (`:255-274`) — what number system is
/// in scope again now that a `)` has closed a parenthesized group?
///
/// The stack interleaves number-system names with literal `"("` scope markers. Ported
/// step for step:
///
/// 1. Pop until a `"("` marker is popped, **discarding** everything popped on the way —
///    those are the `?ns` directives that were declared inside the group now closing, and
///    they go out of scope with it.
/// 2. Then keep popping into a scratch stack until a non-`"("` entry is found; that entry
///    is the answer (skipping over `"("` markers is what makes `((a=1))` resolve to the
///    default rather than to a marker).
/// 3. Push the scratch stack back, restoring the enclosing scopes untouched.
///
/// If step 1 empties the stack without finding a marker, the default is returned and the
/// stack is left empty. That is unreachable through the tokenizer (a `)` with no matching
/// `(` on the *operator* stack raises `unbalancedParen` from `RightParenthesis.put` first,
/// and the two stacks gain their parentheses in lockstep), but it is Java's behavior and is
/// ported rather than asserted away.
fn find_current_number_system(number_systems: &mut Vec<String>, default: &str) -> String {
    let mut current = default.to_string();
    while let Some(popped) = number_systems.pop() {
        if popped == "(" {
            let mut tmp: Vec<String> = Vec::new();
            while let Some(next) = number_systems.pop() {
                let is_marker = next == "(";
                tmp.push(next);
                if !is_marker {
                    current = tmp.last().expect("just pushed").clone();
                    break;
                }
            }
            while let Some(restore) = tmp.pop() {
                number_systems.push(restore);
            }
            break;
        }
    }
    current
}

/// `String.split("(\\s|,)+")` as applied to a quantified-variable list
/// (`Predicate.java:277`), with Java's `split` semantics preserved:
///
/// * separators are runs of ASCII whitespace and/or commas (`\s` is ASCII in Java — see
///   Ruling 2, divergence 2);
/// * a **leading** empty field is kept, **trailing** empty fields are dropped (Java's
///   `limit == 0` behavior);
/// * if no separator matches at all, the result is the whole input as a single field —
///   including when the input is empty, where Java returns `[""]` (length 1), not an empty
///   array. Unreachable here (the pattern guarantees at least one identifier), but ported
///   because `listOfVars.length` feeds the operator's arity.
///
/// The `\s` half of the separator is effectively vestigial in Walnut: the pattern that
/// produced this text only accepts *comma*-separated identifiers, so whitespace can only
/// appear around a comma or trailing. Ported anyway, since it is what determines the count.
fn split_variable_list(list: &str) -> Vec<&str> {
    let is_separator = |b: u8| b == b',' || is_java_whitespace(b);
    let bytes = list.as_bytes();
    let mut fields: Vec<&str> = Vec::new();
    let mut found_separator = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if is_separator(bytes[i]) {
            found_separator = true;
            fields.push(&list[start..i]);
            while i < bytes.len() && is_separator(bytes[i]) {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }
    if !found_separator {
        // Java: "If there is no match, the result is an array with a single element,
        // namely this string" -- the trailing-empty trim does not apply.
        return vec![list];
    }
    fields.push(&list[start..]);
    while fields.last().is_some_and(|s| s.is_empty()) {
        fields.pop();
    }
    fields
}

/// # Provenance of this module's expectations
///
/// Every `post(...)`/`err(...)`/position expectation below was checked against the **real
/// `walnut-java` `Predicate` class**, not derived by reading the Java source: a throwaway
/// probe (`new Predicate(line)`, printing `toString()` plus each token's
/// `getPositionInPredicate()`) was run against `walnut-java/target/Walnut-all.jar` over
/// every case in this file. That is a Tier-3-style differential check performed at
/// authoring time rather than a standing harness, which is why the expected strings are
/// inlined here as literals — they are Walnut's own output, not this port's.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::predicate_env::InMemoryPredicateEnv;
    use std::rc::Rc;
    use wr_core::automaton::Automaton;
    use wr_core::numsys::NumberSystem;

    fn env() -> InMemoryPredicateEnv {
        InMemoryPredicateEnv::new()
    }

    /// Tokenize with a bare in-memory environment (nothing this unit can lex needs a
    /// word/function/macro entry).
    fn lex(s: &str) -> Result<Predicate, LexError> {
        Predicate::new(&env(), s)
    }

    /// The `":"`-joined post-order, i.e. Java's `Predicate.toString()`.
    fn post(s: &str) -> String {
        lex(s).expect("must tokenize").to_string()
    }

    fn err(s: &str) -> String {
        lex(s).expect_err("must fail").to_string()
    }

    // =======================================================================
    // Tier 2 -- ports of `walnut-java`'s own `PredicateTest` cases that do not
    // need a `Session` (i.e. everything but the word/function/macro bodies).
    // =======================================================================

    /// `PredicateTest.basicTest`.
    #[test]
    fn basic_test() {
        let p = lex("blah").unwrap();
        assert_eq!(p.post_order().len(), 1);
        assert_eq!(p.to_string(), "blah");

        let p = lex("?msd_3 (a=1 )").unwrap();
        assert_eq!(p.post_order().len(), 3);
        assert_eq!(p.to_string(), "a:1:=_msd_3");
    }

    /// `PredicateTest.quantifierMissingVariableListThrows`.
    #[test]
    fn quantifier_missing_variable_list_throws() {
        assert_eq!(
            err("E1=1"),
            "Operator E requires a list of variables: char at 0"
        );
    }

    /// `PredicateTest.undefinedTokenThrows`.
    #[test]
    fn undefined_token_throws() {
        assert_eq!(err("%"), "Undefined token: char at 0");
    }

    /// `PredicateTest`'s six adjacency characterization tests. All six are reachable in
    /// this unit even though four of them name deferred token kinds: Java performs the
    /// `lastTokenWasOperator` check BEFORE calling `putWord`/`putFunction`/`putMacro`, so
    /// the check is scanning-loop code (ported here), not construction code (U4's).
    #[test]
    fn value_token_adjacent_to_variable_reports_operator_missing() {
        for input in [
            "a myword[i]",   // wordAdjacentToVariableThrowsOperatorMissing
            "a .myword[i]",  // wordWithDelimiterAdjacentToVariableThrowsOperatorMissing
            "a $myfunc(b)",  // functionAdjacentToVariableThrowsOperatorMissing
            "a #mymacro(b)", // macroAdjacentToVariableThrowsOperatorMissing
            "a 5",           // numberLiteralAdjacentToVariableThrowsOperatorMissing
            "a @5",          // alphabetLetterAdjacentToVariableThrowsOperatorMissing
        ] {
            assert_eq!(
                err(input),
                "An operator is missing: char at 1",
                "input {input:?}"
            );
        }
    }

    /// `PredicateTest.numberSystemDeclaredInsideParenRequiresExtraStackSearch` — the
    /// `?ns` sits INSIDE the parentheses, so `findCurrentNumberSystem`'s outer loop has to
    /// pop a non-`"("` entry before it reaches the marker.
    #[test]
    fn number_system_declared_inside_paren_requires_extra_stack_search() {
        let p = lex("(?msd_3 a=1)").unwrap();
        assert_eq!(p.post_order().len(), 3);
        assert_eq!(p.to_string(), "a:1:=_msd_3");
    }

    /// `PredicateTest.doublyNestedParenWithoutNumberSystemRequiresExtraInnerSearch` —
    /// the inner loop must skip a `"("` marker to reach the default underneath it.
    #[test]
    fn doubly_nested_paren_without_number_system_requires_extra_inner_search() {
        let p = lex("((a=1))").unwrap();
        assert_eq!(p.post_order().len(), 3);
        assert_eq!(p.to_string(), "a:1:=_msd_2");
    }

    /// `PredicateTest.runPredicateTests` case 9's *expanded* predicate and its expected
    /// post-order, verbatim from Java (the macro expansion itself is U4's; what the
    /// expansion lexes to is this unit's). Note `Ea b`: the variable list is just `a`,
    /// because additional quantified variables require a COMMA — `b` is the left operand
    /// of the `=` that follows.
    #[test]
    fn java_macro_case_9_expansion_lexes_to_javas_expected_post_order() {
        assert_eq!(
            post("?msd_2 Ea b = a + 1 & a = 5"),
            "a:b:a:1:+_msd_2:=_msd_2:a:5:=_msd_2:&:E"
        );
    }

    /// `PredicateTest.runPredicateTests` case 13's expanded predicate, with `msd_fib`
    /// swapped for `msd_3` (custom-base files are U14 territory for the in-memory double;
    /// the *structure* under test — two parenthesized groups, each with its own number
    /// system, one introduced inside the parentheses — is unchanged). The expected
    /// post-order is Java's own, with the same substitution.
    #[test]
    fn java_macro_case_13_expansion_lexes_to_javas_expected_post_order() {
        assert_eq!(
            post("?msd_3 (Ea b = a + 1 & a = 5) =>(?lsd_3   Ef g = f + 1 & f = 6)"),
            "a:b:a:1:+_msd_3:=_msd_3:a:5:=_msd_3:&:E:f:g:f:1:+_lsd_3:=_lsd_3:f:6:=_lsd_3:&:E:=>"
        );
    }

    // =======================================================================
    // Quantifiers and variable lists
    // =======================================================================

    #[test]
    fn single_variable_quantifier() {
        assert_eq!(post("Ex (x=1)"), "x:x:1:=_msd_2:E");
    }

    /// A multi-variable list: one quantifier token of arity `n + 1`, followed by `n`
    /// variable tokens.
    #[test]
    fn multi_variable_quantifier_list() {
        let p = lex("E x, y, z (x=y)").unwrap();
        assert_eq!(p.to_string(), "x:y:z:x:y:=_msd_2:E");
        let quantifier = p.post_order().last().unwrap();
        assert_eq!(
            quantifier.arity(),
            4,
            "arity is quantifiedVariableCount + 1 (3 variables + the operand)"
        );
    }

    /// All three quantifier letters take a variable list, and all three are recognized by
    /// the same branch.
    #[test]
    fn all_three_quantifiers_take_variable_lists() {
        assert_eq!(post("Ex (x=1)"), "x:x:1:=_msd_2:E");
        assert_eq!(post("Ax (x=1)"), "x:x:1:=_msd_2:A");
        assert_eq!(post("Ix (x=1)"), "x:x:1:=_msd_2:I");
    }

    /// Irregular whitespace inside the list, including around the commas and a trailing
    /// run before the operand — all absorbed by the pattern and by Java's
    /// `split("(\\s|,)+")`.
    #[test]
    fn quantifier_list_tolerates_irregular_whitespace() {
        let p = lex("E   x ,y   ,  z   (x=1)").unwrap();
        assert_eq!(p.to_string(), "x:y:z:x:1:=_msd_2:E");
        assert_eq!(p.post_order().last().unwrap().arity(), 4);
    }

    /// Adjacent quantifiers must not pop each other: they share priority 150 and are
    /// left-associative, so only `Operator.put`'s immediate-push special case keeps
    /// `Ex Ey ...` in the right order (both end up at the very end, innermost first).
    #[test]
    fn adjacent_quantifiers_nest_rather_than_popping_each_other() {
        assert_eq!(post("Ex Ey (x=y)"), "x:y:x:y:=_msd_2:E:E");
    }

    /// A quantified variable may not start with `A`/`E`/`I` (`ALPHANUMERIC`'s class
    /// intersection), so `E Ax` does not lex as "quantify `Ax`" — the list match fails and
    /// Java's "requires a list of variables" error fires.
    #[test]
    fn quantified_variable_may_not_start_with_a_reserved_letter() {
        assert_eq!(
            err("E Ax (x=1)"),
            "Operator E requires a list of variables: char at 0"
        );
    }

    #[test]
    fn split_variable_list_matches_java_split_semantics() {
        assert_eq!(split_variable_list("x"), vec!["x"]);
        assert_eq!(split_variable_list("x "), vec!["x"]);
        assert_eq!(split_variable_list("x, y, z "), vec!["x", "y", "z"]);
        assert_eq!(split_variable_list("x ,   y"), vec!["x", "y"]);
        // Leading separator => leading empty field is KEPT (Java's `split` only trims
        // trailing empties).
        assert_eq!(split_variable_list(" x"), vec!["", "x"]);
        // No separator at all => the whole (even empty) string, as one field.
        assert_eq!(split_variable_list(""), vec![""]);
    }

    // =======================================================================
    // Number-system tracking (`?msd_k` directives + `findCurrentNumberSystem`)
    // =======================================================================

    /// A directive at the top level applies to every later operator.
    #[test]
    fn number_system_directive_applies_to_later_operators() {
        assert_eq!(post("?msd_3 a=1 & b=2"), "a:1:=_msd_3:b:2:=_msd_3:&");
    }

    /// The load-bearing nesting case: a directive inside parentheses is discarded when the
    /// group closes, and the enclosing directive comes back into scope.
    #[test]
    fn number_system_scope_is_restored_when_a_paren_group_closes() {
        assert_eq!(
            post("?lsd_3 (?msd_2 a=1) & b=2"),
            "a:1:=_msd_2:b:2:=_lsd_3:&"
        );
    }

    /// Two sibling groups, each with its own directive, and a third operator after both —
    /// which must be back on the outermost number system.
    #[test]
    fn sibling_paren_groups_do_not_leak_number_systems() {
        assert_eq!(
            post("(?msd_3 a=1) & (?lsd_2 b=2) & c=3"),
            "a:1:=_msd_3:b:2:=_lsd_2:&:c:3:=_msd_2:&"
        );
    }

    /// Nested groups: the inner `)` restores the *middle* system, the outer `)` the
    /// outermost one.
    #[test]
    fn nested_paren_groups_restore_one_level_at_a_time() {
        assert_eq!(
            post("?msd_5 ((?msd_3 (?msd_2 a=1) & b=2) & c=3)"),
            "a:1:=_msd_2:b:2:=_msd_3:&:c:3:=_msd_5:&"
        );
    }

    /// Every `normalizeNumberSystemToken` shorthand the `?…` pattern can deliver.
    #[test]
    fn number_system_token_shorthands_normalize() {
        assert_eq!(post("?msd5 a=1"), "a:1:=_msd_5");
        assert_eq!(post("?msd a=1"), "a:1:=_msd_2");
        assert_eq!(post("?lsd a=1"), "a:1:=_lsd_2");
        assert_eq!(post("?lsd_3 a=1"), "a:1:=_lsd_3");
        // A bare number: `?3` normalizes to `msd_3`.
        assert_eq!(post("?3 a=1"), "a:1:=_msd_3");
    }

    /// `find_current_number_system` on a stack that never contains a `"("` marker leaves
    /// the stack empty and answers with the default (Java's behavior; unreachable through
    /// the tokenizer, since the operator stack raises `unbalancedParen` first).
    #[test]
    fn find_current_number_system_without_a_marker_falls_back_to_the_default() {
        let mut stack = vec!["msd_2".to_string(), "msd_3".to_string()];
        assert_eq!(find_current_number_system(&mut stack, "msd_2"), "msd_2");
        assert!(stack.is_empty());
    }

    // =======================================================================
    // Number systems really are resolved through `PredicateEnv`
    // =======================================================================

    /// The relational token carries a `NumberSystem` obtained from the environment, not a
    /// name: `Operator`'s `Display` prints `op + "_" + ns.name()`, so a preloaded system
    /// registered under a DIFFERENT name than it reports proves the handle came from the
    /// environment's cache rather than from the token text.
    #[test]
    fn number_system_is_resolved_through_the_environment() {
        let env = InMemoryPredicateEnv::new()
            .with_number_system("msd_fib", NumberSystem::new("lsd_3").unwrap());
        let p = Predicate::new(&env, "?msd_fib a=1").unwrap();
        assert_eq!(p.to_string(), "a:1:=_lsd_3");
    }

    /// An unresolvable number system surfaces as the environment's own error, verbatim —
    /// and it is raised at *token construction* time (Java's
    /// `NumberSystem.getComputeIfAbsent` inside the scanning loop), not deferred to
    /// evaluation.
    #[test]
    fn unresolvable_number_system_fails_at_token_construction() {
        let e = lex("?msd_fib a=1").unwrap_err();
        match &e {
            LexError::Env(PredicateEnvError::NumberSystem { name, .. }) => {
                assert_eq!(name, "msd_fib")
            }
            other => panic!("expected an Env error, got {other:?}"),
        }
        assert_eq!(e.to_string(), "Number system msd_fib is not defined.");
    }

    /// Number LITERALS resolve a number system too (`Predicate.java:213`), so the lookup
    /// happens even with no relational/arithmetic operator anywhere in the predicate.
    #[test]
    fn number_literal_also_resolves_the_current_number_system() {
        let env = InMemoryPredicateEnv::new()
            .with_number_system("msd_x", NumberSystem::new("lsd_2").unwrap());
        // No operator at all: the only environment lookup comes from the literal.
        let p = Predicate::new(&env, "?msd_x 5").unwrap();
        assert_eq!(p.to_string(), "5");
        assert!(Predicate::new(&env, "?msd_nope 5").is_err());
    }

    // =======================================================================
    // `\G` anchoring / priority order
    // =======================================================================

    /// The direct test of Ruling 2's core mechanism: at an offset where a token does not
    /// begin, the match must FAIL rather than scan forward and find it later.
    #[test]
    fn anchoring_never_scans_forward_to_a_later_token() {
        let hay = "1+2=3";
        // A forward-scanning engine would report the `=` at offset 3 here.
        assert!(find_at(&patterns().relational_operators, hay, 0).is_none());
        assert!(find_at(&patterns().relational_operators, hay, 3).is_some());
        // ... which is exactly why the literal `1` is the first token, not the `=`.
        assert_eq!(post(hay), "1:2:+_msd_2:3:=_msd_2");
    }

    /// `<=>` is a logical operator (priority level 1 of the chain), `<=` a relational one
    /// (level 2). A scan that consulted the relational pattern first would split `<=>`
    /// into `<=` followed by an undefined `>`.
    #[test]
    fn iff_wins_over_less_equal_by_priority_order() {
        assert_eq!(post("a<=>b"), "a:b:<=>");
        assert_eq!(post("a<=b"), "a:b:<=_msd_2");
        assert_eq!(post("a=>b"), "a:b:=>");
        assert_eq!(post("a>=b"), "a:b:>=_msd_2");
    }

    /// `NUMBER_SYSTEM`'s inner `(\d+|\w+)` alternation must prefer `\d+`, exactly as Java's
    /// backtracking engine does — otherwise the number-system token would swallow trailing
    /// letters. `?msd_3x` therefore lexes as the directive `msd_3` followed by the variable
    /// `x`, which (being a value token adjacent to another value token) is what produces
    /// real Walnut's "An operator is missing: char at 7" rather than a
    /// "Number system msd_3x is not defined." — a directly distinguishing observation, since
    /// the two engines would disagree loudly here if Rust preferred `\w+`.
    #[test]
    fn number_system_token_prefers_the_digit_alternative() {
        assert_eq!(err("?msd_3x a=1"), "An operator is missing: char at 7");
    }

    /// `normalizeNumberSystemToken` is case-sensitive: `MSD_3` is not the `msd` prefix, so it
    /// falls through to the "prepend `msd_`" branch and produces the unusable name
    /// `msd_MSD_3`. Confirmed against real Walnut, message text included.
    #[test]
    fn number_system_normalization_is_case_sensitive() {
        assert_eq!(err("?MSD_3 a=1"), "Number system msd_MSD_3 is not defined.");
    }

    /// Degenerate but pattern-legal directives, with the message text real Walnut prints.
    /// (`?msd_` is reachable because `\w` includes `_`, so the `\w+` alternative matches the
    /// trailing underscore itself; `?0`/`?1` normalize to bases the constructor rejects.)
    #[test]
    fn degenerate_number_system_directives_report_walnut_message_text() {
        assert_eq!(err("?msd_ a=1"), "Number system msd_ is not defined.");
        assert_eq!(err("?0 a=1"), "Number system msd_0 is not defined.");
        assert_eq!(err("?1 a=1"), "Number system msd_1 is not defined.");
    }

    /// `msd`/`lsd` are reserved only as *prefixes*, not as identifiers: `Emsd (x=1)`
    /// quantifies a variable literally named `msd`.
    #[test]
    fn msd_is_a_legal_variable_name() {
        assert_eq!(post("Emsd (x=1)"), "msd:x:1:=_msd_2:E");
    }

    /// `ALPHANUMERIC` requires a *letter* first, so `_x` is not a variable name — and `_` is
    /// the unary-negative arithmetic operator, which the variable-list pattern will not
    /// accept.
    #[test]
    fn underscore_cannot_start_a_quantified_variable() {
        assert_eq!(
            err("E_x (x=1)"),
            "Operator E requires a list of variables: char at 0"
        );
    }

    /// A lone `!` is not a relational operator (only `!=` is), so it is undefined.
    #[test]
    fn a_lone_bang_is_an_undefined_token() {
        assert_eq!(err("a=1 ! b=2"), "Undefined token: char at 4");
    }

    /// Leading zeros survive `parseBigInteger` as a value, not as text.
    #[test]
    fn number_literals_with_leading_zeros_lex_to_their_value() {
        assert_eq!(post("x = 007"), "x:7:=_msd_2");
    }

    /// `!=`, and an alphabet letter `@0`.
    #[test]
    fn not_equal_and_zero_alphabet_letter() {
        assert_eq!(post("a!=b"), "a:b:!=_msd_2");
        assert_eq!(post("a=@0"), "a:0:=_msd_2");
    }

    /// `&&` is not an operator of its own — it lexes as two `&`s, and the second pops the
    /// first (equal priority, left-associative), so the conjunction appears twice.
    /// Confirmed against real Walnut.
    #[test]
    fn a_doubled_ampersand_lexes_as_two_conjunctions() {
        assert_eq!(post("a=1&&b=2"), "a:1:=_msd_2:&:b:2:=_msd_2:&");
    }

    /// A base with a leading zero keeps its literal name (`msd_02`, base 2).
    #[test]
    fn a_number_system_name_is_not_canonicalized() {
        assert_eq!(post("?msd_02 a=1"), "a:1:=_msd_02");
    }

    /// The word pattern outranks the variable pattern, so `T[i]` is a word occurrence and
    /// not the variable `T` followed by an undefined `[`. With `T` unregistered, this
    /// surfaces as [`PredicateEnvError::FileDoesNotExist`] rather than "undefined token"
    /// or "operator missing" — proof the chain routed it to the word arm at all, since
    /// neither of those errors is reachable once it has.
    #[test]
    fn word_pattern_outranks_variable_pattern() {
        match lex("T[i]").unwrap_err() {
            LexError::Env(PredicateEnvError::FileDoesNotExist { address }) => {
                assert_eq!(address, "Word Automata Library/T.txt")
            }
            other => panic!("expected a FileDoesNotExist error, got {other}"),
        }
    }

    /// Likewise `.EVEN[i]`: the delimited-word pattern claims it, and its leading `E` is
    /// never mistaken for a quantifier — `EVEN` (no leading dot) is looked up, not
    /// `.EVEN`.
    #[test]
    fn delimited_word_is_not_lexed_as_a_quantifier() {
        match lex(".EVEN[i]").unwrap_err() {
            LexError::Env(PredicateEnvError::FileDoesNotExist { address }) => {
                assert_eq!(address, "Word Automata Library/EVEN.txt")
            }
            other => panic!("expected a FileDoesNotExist error, got {other}"),
        }
    }

    /// Ruling 2's divergence 1, tested against the emulation directly: an operator
    /// character immediately preceded by `.` is not a logical operator, and one at offset
    /// 0 (no preceding byte at all) is.
    #[test]
    fn logical_operator_lookbehind_emulation_rejects_a_dotted_name() {
        assert!(find_logical_operator(".EVEN[i]", 1).is_none());
        assert!(find_logical_operator("EVEN[i]", 0).is_some());
        // The guard is about the byte before the OPERATOR, not before the cursor: leading
        // whitespace the pattern consumes moves the operator past the dot.
        assert!(find_logical_operator("x. E y", 2).is_some());
        assert!(find_logical_operator("x.E y", 2).is_none());
    }

    /// `$f(` and `#m(` are consulted before the bare-`(` pattern; without that ordering the
    /// `$`/`#` would be undefined tokens. With `phi`/`m` unregistered, the routing shows up
    /// as each construction's own environment lookup instead.
    #[test]
    fn function_and_macro_patterns_outrank_the_left_parenthesis() {
        match lex("$phi(x)").unwrap_err() {
            LexError::Env(PredicateEnvError::FileDoesNotExist { address }) => {
                assert_eq!(address, "Automata Library/phi.txt")
            }
            other => panic!("expected a FileDoesNotExist error, got {other}"),
        }
        match lex("#m(x)").unwrap_err() {
            LexError::Env(PredicateEnvError::MacroDoesNotExist { name }) => {
                assert_eq!(name, "m")
            }
            other => panic!("expected a MacroDoesNotExist error, got {other}"),
        }
    }

    // =======================================================================
    // Leaf tokens
    // =======================================================================

    #[test]
    fn alphabet_letters_carry_their_sign_and_tolerate_whitespace() {
        assert_eq!(post("x=@1"), "x:1:=_msd_2");
        assert_eq!(post("x=@-1"), "x:-1:=_msd_2");
        assert_eq!(post("x=@+2"), "x:2:=_msd_2");
        // The pattern deliberately allows whitespace around the sign; `parseInt` strips it.
        assert_eq!(post("x=@ - 3"), "x:-3:=_msd_2");
    }

    /// The literal keeps its full `BigInteger` value (`UtilityMethods.parseBigInteger`),
    /// not a truncated `int`.
    #[test]
    fn number_literals_are_arbitrary_precision() {
        assert_eq!(
            post("x=123456789012345678901234567890"),
            "x:123456789012345678901234567890:=_msd_2"
        );
    }

    /// Every arithmetic symbol, including the unary-negative `_`.
    #[test]
    fn arithmetic_operators_are_recognized() {
        assert_eq!(post("x+1=y"), "x:1:+_msd_2:y:=_msd_2");
        assert_eq!(post("x-1=y"), "x:1:-_msd_2:y:=_msd_2");
        assert_eq!(post("2*x=y"), "2:x:*_msd_2:y:=_msd_2");
        assert_eq!(post("x/2=y"), "x:2:/_msd_2:y:=_msd_2");
        assert_eq!(post("x=_1"), "x:1:__msd_2:=_msd_2");
    }

    /// All three tilde spellings of negation, plus the reverse operator.
    #[test]
    fn negation_spellings_and_reverse() {
        assert_eq!(post("~(a=1)"), "a:1:=_msd_2:~");
        assert_eq!(post("\u{02dc}(a=1)"), "a:1:=_msd_2:\u{02dc}");
        assert_eq!(post("\u{0303}(a=1)"), "a:1:=_msd_2:\u{0303}");
        assert_eq!(post("`(a=1)"), "a:1:=_msd_2:`");
    }

    #[test]
    fn boolean_connectives_respect_the_precedence_table() {
        // `&` (90) binds tighter than `=>` (100), which binds tighter than `<=>` (110).
        assert_eq!(
            post("a=1 & b=2 => c=3 <=> d=4"),
            "a:1:=_msd_2:b:2:=_msd_2:&:c:3:=_msd_2:=>:d:4:=_msd_2:<=>"
        );
    }

    // =======================================================================
    // Whitespace, empty input, and unbalanced parentheses
    // =======================================================================

    /// `PATTERN_WHITESPACE.matcher(predicate).matches()` short-circuit (`:143`): an empty
    /// or all-whitespace predicate is not an error, it is an empty token stream.
    #[test]
    fn whitespace_only_predicates_yield_an_empty_post_order() {
        for input in ["", " ", "\t\n ", "\r\n"] {
            let p = lex(input).expect("whitespace-only input must not fail");
            assert!(p.post_order().is_empty(), "input {input:?}");
            assert_eq!(p.to_string(), "");
        }
    }

    /// Java's `\s` is ASCII-only (Ruling 2, divergence 2): a non-breaking space is NOT
    /// whitespace, and must be rejected as an undefined token rather than silently skipped.
    #[test]
    fn non_ascii_whitespace_is_not_whitespace() {
        assert_eq!(err("a\u{00a0}= 1"), "Undefined token: char at 1");
        // ... and it does not satisfy the all-whitespace short circuit either.
        assert_eq!(err("\u{00a0}"), "Undefined token: char at 0");
    }

    #[test]
    fn unclosed_left_parenthesis_is_reported_by_the_final_drain() {
        assert_eq!(err("(a=1"), "unbalanced parenthesis: char at 0");
    }

    #[test]
    fn unmatched_right_parenthesis_is_reported_by_the_shunting_yard() {
        assert_eq!(err("a=1)"), "unbalanced parenthesis: char at 3");
    }

    /// The two golden-corpus fixtures that exercise nothing but parentheses: `def test669
    /// "((("` -> `error669.txt` and `def test670 ")))"` -> `error670.txt`. The drain reports
    /// the LAST unclosed parenthesis (offset 2), while a stray `)` reports the first one.
    #[test]
    fn golden_corpus_paren_only_predicates_match_their_error_fixtures() {
        assert_eq!(err("((("), "unbalanced parenthesis: char at 2");
        assert_eq!(err(")))"), "unbalanced parenthesis: char at 0");
    }

    // =======================================================================
    // Positions: UTF-16 units, `realStartingPosition`, and WB-015's quirks
    // =======================================================================

    /// The position-unit decision (module docs): reported positions are UTF-16 code-unit
    /// offsets, as Java's are. `˜` (U+02DC) is 2 bytes but 1 UTF-16 unit, so the `(` that
    /// follows it is at Java-position 1 — a byte-offset port would say 2.
    #[test]
    fn positions_are_utf16_units_not_bytes() {
        assert_eq!(err("\u{02dc}(a=1"), "unbalanced parenthesis: char at 1");
        // Two of them: 4 bytes, 2 UTF-16 units.
        assert_eq!(
            err("\u{02dc}\u{0303}(a=1"),
            "unbalanced parenthesis: char at 2"
        );
    }

    /// `realStartingPosition` is added to ordinary token/error positions.
    #[test]
    fn real_starting_position_offsets_reported_positions() {
        let e = Predicate::with_context(&env(), MSD_2, "(a=1", 100).unwrap_err();
        assert_eq!(e.to_string(), "unbalanced parenthesis: char at 100");
        let e = Predicate::with_context(&env(), MSD_2, "%", 100).unwrap_err();
        assert_eq!(e.to_string(), "Undefined token: char at 100");
    }

    /// The default number system is honored for a nested predicate.
    #[test]
    fn default_number_system_is_honored() {
        let p = Predicate::with_context(&env(), "lsd_3", "a=1", 0).unwrap();
        assert_eq!(p.to_string(), "a:1:=_lsd_3");
        assert_eq!(p.default_number_system(), "lsd_3");
    }

    /// **WB-015**, pinned: a quantifier operator and its quantified variables omit
    /// `realStartingPosition` and use the whole-match start (leading whitespace included),
    /// while every sibling token in the same predicate uses `realStartingPosition +
    /// start(1)`. Ported verbatim, so this test asserts the *defective* values.
    #[test]
    fn quantifier_token_positions_omit_real_starting_position_wb015() {
        let p = Predicate::with_context(&env(), MSD_2, " Ex (x=1)", 100).unwrap();
        // post-order: x (quantified variable), x, 1, =, E
        let positions: Vec<usize> = p
            .post_order()
            .iter()
            .map(|t| t.position_in_predicate())
            .collect();
        let quantified_variable = positions[0];
        let ordinary_variable = positions[1];
        let e_operator = *positions.last().unwrap();

        assert_eq!(
            e_operator, 0,
            "the quantifier records the whole-match start (0, i.e. before the leading \
             space) with NO realStartingPosition -- WB-015"
        );
        assert_eq!(
            quantified_variable, 2,
            "the quantified variable records the variable-list match start (2, just past \
             the `E`), also with no realStartingPosition -- WB-015"
        );
        assert_eq!(
            ordinary_variable, 105,
            "every non-quantifier token DOES add realStartingPosition, which is what makes \
             the two defects above observable"
        );
    }

    /// **WB-015**, second part: every variable in one quantifier's list shares the list's
    /// start position instead of carrying its own.
    #[test]
    fn all_quantified_variables_share_one_position_wb015() {
        let p = lex("E x, y, z (x=1)").unwrap();
        let positions: Vec<usize> = p.post_order()[..3]
            .iter()
            .map(|t| t.position_in_predicate())
            .collect();
        assert_eq!(positions, vec![1, 1, 1]);
    }

    /// **WB-015**, third part: the parenthesis tokens record the pre-whitespace cursor,
    /// not their own offset — observable in the unbalanced-parenthesis message.
    #[test]
    fn parenthesis_positions_are_the_pre_whitespace_cursor_wb015() {
        // The `(` is at offset 6; the cursor when it was scanned was 5 (just past `&`).
        assert_eq!(err("a=1 & ( b=2"), "unbalanced parenthesis: char at 5");
    }

    /// Token positions — not just error text — are UTF-16 units. Confirmed against the
    /// real `walnut-java` `Predicate` (see this test module's header note): `a˜=1` yields
    /// positions `0,3,2,1`, i.e. the `=` is at 2 and the `1` at 3, while their BYTE offsets
    /// are 3 and 4. A byte-offset port would be off by one on every token after the `˜`.
    #[test]
    fn token_positions_after_a_non_ascii_operator_match_javas() {
        let p = lex("a\u{02dc}=1").unwrap();
        assert_eq!(p.to_string(), "a:1:=_msd_2:\u{02dc}");
        let positions: Vec<usize> = p
            .post_order()
            .iter()
            .map(|t| t.position_in_predicate())
            .collect();
        assert_eq!(positions, vec![0, 3, 2, 1]);
    }

    // =======================================================================
    // `lastTokenWasOperator` quirks (all confirmed against real `walnut-java`)
    // =======================================================================

    /// The left-parenthesis arm does NOT set `lastTokenWasOperator = true` — Java only
    /// ever sets it in the logical/relational/arithmetic arms. So a value token, a `(`,
    /// and another value token in a row still report a missing operator, at the position
    /// of the token AFTER the parenthesis.
    ///
    /// `E x y (x=1)` is the natural way to hit it: only `x` is quantified (a second
    /// quantified variable would need a comma), `y` is then an ordinary variable which
    /// clears the flag, and the `(` does not restore it.
    #[test]
    fn left_parenthesis_does_not_clear_the_operator_missing_flag() {
        assert_eq!(err("E x y (x=1)"), "An operator is missing: char at 7");
    }

    /// A `?ns` directive does not set the flag either, so it cannot be used to separate
    /// two value tokens.
    #[test]
    fn number_system_directive_does_not_clear_the_operator_missing_flag() {
        assert_eq!(err("a ?msd_3 b"), "An operator is missing: char at 8");
    }

    /// A right-associative unary chain: `~ ~ b=2` keeps both negations, innermost last.
    #[test]
    fn repeated_negation_is_right_associative() {
        assert_eq!(post("a=1 & ~ ~ b=2"), "a:1:=_msd_2:b:2:=_msd_2:~:~:&");
    }

    // =======================================================================
    // Malformed input that is NOT an error, and near-misses that are
    // =======================================================================

    /// A quantifier with no operand at all is not a lexing error — the operator simply
    /// drains onto the end of the post-order (arity checking happens at evaluation time).
    #[test]
    fn quantifier_without_an_operand_still_lexes() {
        assert_eq!(post("Ex"), "x:E");
    }

    /// A bare `?` does not satisfy the number-system pattern (which requires at least one
    /// `\d+`/`\w+` after the optional `msd`/`lsd`), so it is an undefined token — the
    /// `normalizeNumberSystemToken` quirk that turns `"?"` into the unusable name `"msd_"`
    /// is therefore unreachable from the tokenizer.
    #[test]
    fn a_bare_question_mark_is_an_undefined_token() {
        assert_eq!(err("?"), "Undefined token: char at 0");
        assert_eq!(err("??msd_3 a=1"), "Undefined token: char at 0");
    }

    /// A trailing comma is not part of the variable list (the repetition needs an
    /// identifier after the comma), so the comma itself becomes an undefined token.
    #[test]
    fn a_trailing_comma_in_a_variable_list_is_an_undefined_token() {
        assert_eq!(err("E x, (x=1)"), "Undefined token: char at 3");
        // ... and a LEADING comma makes the list match fail outright.
        assert_eq!(
            err("E,x (x=1)"),
            "Operator E requires a list of variables: char at 0"
        );
    }

    /// The delimited-word pattern needs a letter immediately after the `.`, so a stray
    /// `.` between two identifiers is an undefined token rather than a word.
    #[test]
    fn a_bare_dot_is_an_undefined_token() {
        assert_eq!(err("a.b"), "Undefined token: char at 1");
    }

    /// A comma-separated list attached directly to the quantifier letter.
    #[test]
    fn quantifier_list_may_start_immediately_after_the_letter() {
        assert_eq!(post("Ex, y (x=y)"), "x:y:x:y:=_msd_2:E");
    }

    /// Two directives in a row: both are pushed, and the later one wins.
    #[test]
    fn back_to_back_number_system_directives_take_the_last() {
        assert_eq!(post("?msd_3?lsd_2 a=1"), "a:1:=_lsd_2");
    }

    /// An inner group with no directive of its own must resolve back to the enclosing
    /// group's directive, not to the default.
    #[test]
    fn inner_group_without_a_directive_inherits_the_enclosing_one() {
        assert_eq!(post("(?msd_3 (a=1) & b=2)"), "a:1:=_msd_3:b:2:=_msd_3:&");
    }

    // =======================================================================
    // Larger end-to-end shapes
    // =======================================================================

    #[test]
    fn a_realistic_query_lexes_to_the_expected_post_order() {
        assert_eq!(
            post("?msd_3 Ei (i >= 1 & Aj (j < i => x + j = y))"),
            "i:i:1:>=_msd_3:j:j:i:<_msd_3:x:j:+_msd_3:y:=_msd_3:=>:A:&:E"
        );
    }

    /// One environment serves several predicates (and, per `PredicateEnv`'s `&self`
    /// contract, will serve nested ones in U4).
    #[test]
    fn one_environment_serves_many_predicates() {
        let env = env();
        assert_eq!(
            Predicate::new(&env, "a=1").unwrap().to_string(),
            "a:1:=_msd_2"
        );
        assert_eq!(
            Predicate::new(&env, "?msd_3 b=2").unwrap().to_string(),
            "b:2:=_msd_3"
        );
    }

    // =======================================================================
    // Word / Function / Macro (U4)
    // =======================================================================

    /// Lex against a caller-supplied environment (the four tests above and everything
    /// before them only ever needed the empty [`env`]).
    fn lex_with(env: &InMemoryPredicateEnv, s: &str) -> Result<Predicate, LexError> {
        Predicate::new(env, s)
    }

    fn post_with(env: &InMemoryPredicateEnv, s: &str) -> String {
        lex_with(env, s).expect("must tokenize").to_string()
    }

    fn err_with(env: &InMemoryPredicateEnv, s: &str) -> String {
        lex_with(env, s).expect_err("must fail").to_string()
    }

    /// An unlabeled, `arity`-track automaton with an empty transition table — standing
    /// in for a word/function file. The transition content is irrelevant to every test
    /// below (none of them execute `act()`; that's U9/U10/U11 territory), only
    /// `get_arity()` and `bind`'s arity check matter here.
    fn stub_automaton(arity: usize) -> Automaton {
        let fa = wr_core::fa::Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2usize.saturating_pow(arity as u32).max(1),
            o: vec![1],
            d: vec![std::collections::BTreeMap::new()],
        };
        Automaton::new(
            fa,
            vec![vec![0, 1]; arity],
            Vec::new(),
            vec![Some(true); arity],
        )
    }

    // -- `PredicateTest.runPredicateTests` (all 15 cases), ported to real `#name(...)`
    // -- macro CALLS now that `putMacro` is implemented (U3 could only test the
    // -- already-expanded text; see `java_macro_case_9`/`_13` above). Cases 12/13 swap
    // -- `msd_fib` for `msd_3`, matching the substitution `java_macro_case_13` already
    // -- established (custom-base files are `wr-cli`'s `Session` territory, not this
    // -- in-memory double's).

    fn macro_test_env() -> InMemoryPredicateEnv {
        InMemoryPredicateEnv::new()
            .with_macro("my_macro0", "%0")
            .with_macro("my_macro1", "%0")
            .with_macro("my_macro2", "%0")
            .with_macro("my_macro3", "%0=1")
            .with_macro("my_macro4", "%0=1")
            .with_macro("my_macro5", "%0=1")
            .with_macro("my_macro6", "%0=1")
            .with_macro("my_macro7", "a+b=2")
            .with_macro("my_macro8", "a+b=2")
            .with_macro("my_macro9", "%0 E%1 %2 = %1 + 1 & %1 = 5")
            .with_macro("my_macro10", "%0 E%1 %2 = %1 + 1 & %1 = 5")
            .with_macro("my_macro11", "%0 E%1 %2 = %1 + 1 &")
            .with_macro("my_macro12", "E%0 %1 = %0 + 1 &")
            .with_macro("my_macro13", "E%0 %1 = %0 + 1 &")
            .with_macro("my_macro14", "%10=%1")
    }

    /// Regression guard for the DESCENDING substitution order `put_macro` uses
    /// (`Predicate.java:435`'s `for (int arg = arguments.size()-1; arg >= 0; arg--)`,
    /// mirrored by this port's `.rev()` iterator). With only single-digit placeholders
    /// present, ascending vs. descending order is unobservable (no `%N` is a prefix of
    /// another `%M`) — every OTHER macro test in this file tops out at `%2`, so none of
    /// them can catch a regression that silently flips the iteration direction. This
    /// test uses 11 placeholders (`%0`..`%10`) specifically so `%1` is a strict prefix
    /// of `%10`: a naive ASCENDING substitution would replace the `%1` inside `%10`
    /// first, corrupting it (`"%10=%1"` -> `"1"+"0=%1"` = `"10=%1"` instead of the
    /// correct `"2=1"`), before `%10` ever gets its own turn.
    #[test]
    fn macro_call_with_eleven_placeholders_substitutes_in_descending_order() {
        let env = macro_test_env();
        let p = Predicate::new(&env, "#my_macro14(z,1,z,z,z,z,z,z,z,z,2)").expect("must tokenize");
        assert_eq!(p.predicate(), "2=1");
        assert_eq!(p.to_string(), "2:1:=_msd_2");
    }

    #[test]
    fn java_macro_calls_expand_and_lex_to_javas_expected_post_order() {
        let env = macro_test_env();
        let cases: &[(&str, &str, &str)] = &[
            // (predicate, expected rewritten `predicate` buffer, expected post-order)
            ("#my_macro0(a)=1", "a=1", "a:1:=_msd_2"),
            ("#my_macro1(a)=1", "a=1", "a:1:=_msd_2"),
            ("#my_macro2(a)=1", "a=1", "a:1:=_msd_2"),
            ("?msd_3 (#my_macro3(a))", "?msd_3 (a=1)", "a:1:=_msd_3"),
            ("?msd_3 (#my_macro4(a))", "?msd_3 ( a=1)", "a:1:=_msd_3"),
            ("?msd_3 (#my_macro5(a) )", "?msd_3 (a=1 )", "a:1:=_msd_3"),
            (
                "?msd_3 (#my_macro6(a) => #my_macro6(b))",
                "?msd_3 (a=1 => b=1)",
                "a:1:=_msd_3:b:1:=_msd_3:=>",
            ),
            ("#my_macro7()", "a+b=2", "a:b:+_msd_2:2:=_msd_2"),
            (
                "#my_macro8() & #my_macro8()",
                "a+b=2 & a+b=2",
                "a:b:+_msd_2:2:=_msd_2:a:b:+_msd_2:2:=_msd_2:&",
            ),
            (
                "#my_macro9(?msd_2,a,b)",
                "?msd_2 Ea b = a + 1 & a = 5",
                "a:b:a:1:+_msd_2:=_msd_2:a:5:=_msd_2:&:E",
            ),
            (
                "#my_macro10(?msd_3,a,b)",
                "?msd_3 Ea b = a + 1 & a = 5",
                "a:b:a:1:+_msd_3:=_msd_3:a:5:=_msd_3:&:E",
            ),
            (
                "#my_macro11(?msd_2,a,b) a = 5",
                "?msd_2 Ea b = a + 1 & a = 5",
                "a:b:a:1:+_msd_2:=_msd_2:a:5:=_msd_2:&:E",
            ),
            (
                // `msd_fib` -> `msd_3` (see this section's header note).
                "?msd_3 #my_macro12(a,b) a = 5",
                "?msd_3 Ea b = a + 1 & a = 5",
                "a:b:a:1:+_msd_3:=_msd_3:a:5:=_msd_3:&:E",
            ),
            (
                "?msd_3 (#my_macro13(a,b) a = 5) =>(?lsd_3   #my_macro13(f,g) f = 6)",
                "?msd_3 (Ea b = a + 1 & a = 5) =>(?lsd_3   Ef g = f + 1 & f = 6)",
                "a:b:a:1:+_msd_3:=_msd_3:a:5:=_msd_3:&:E:f:g:f:1:+_lsd_3:=_lsd_3:f:6:=_lsd_3:&:E:=>",
            ),
            // Case 14: a nested, parenthesized top-level comma inside the ONE argument
            // exercises `parse_parenthesized_arguments`'s "nested comma" branch. `%0` is
            // never referenced by `my_macro7`'s body, so the argument is parsed and
            // discarded, unused -- identical result to case 7.
            ("#my_macro7((x,y))", "a+b=2", "a:b:+_msd_2:2:=_msd_2"),
        ];
        for (predicate, expected_buffer, expected_post) in cases {
            let p = Predicate::new(&env, predicate)
                .unwrap_or_else(|e| panic!("{predicate:?} must tokenize: {e}"));
            // `PredicateTest.test(PredTest)` compares BOTH strings with every space
            // stripped (`t.expected_predicate.strip().replace(" ","")`) — not exact
            // equality — so this mirrors that exact (weaker, deliberately
            // whitespace-insensitive) invariant rather than a stronger one this port
            // invented. `assertion left == right` failures should be trusted, but a
            // difference in whitespace ALONE is not one Java's own suite checks either
            // (the Java literal fixtures themselves have at least one such inconsistency
            // between structurally-identical cases 3/4, confirmed by hand-tracing the
            // algorithm against `putMacro`'s source — not a Rust-port defect).
            let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(
                strip(p.predicate()),
                strip(expected_buffer),
                "input {predicate:?}"
            );
            assert_eq!(
                strip(&p.to_string()),
                strip(expected_post),
                "input {predicate:?}"
            );
        }
    }

    /// `PredicateTest.wordWithMissingClosingBracketConsumesToEndOfInput`: an unclosed
    /// `[` means zero indices were ever finalized, mismatching `F`'s real arity (1), so
    /// `Word::new`'s arity check fires — with `F`'s own (correctly `real_starting_position`-
    /// adjusted) position, not WB-017's defect.
    #[test]
    fn word_with_missing_closing_bracket_consumes_to_end_of_input() {
        let env = InMemoryPredicateEnv::new().with_word("F", stub_automaton(1));
        assert_eq!(
            err_with(&env, "F[i"),
            "function F requires 1 arguments: char at 0"
        );
    }

    /// `PredicateTest.wordWithEmptyIndexThrows`.
    #[test]
    fn word_with_empty_index_throws() {
        let env = InMemoryPredicateEnv::new().with_word("F", stub_automaton(1));
        assert_eq!(
            err_with(&env, "F[]"),
            "index 1 of the word F cannot be empty: char at 0"
        );
    }

    /// `PredicateTest.macroCallInternalMacroInArgumentThrows`.
    #[test]
    fn macro_call_internal_macro_in_argument_throws() {
        let env = macro_test_env();
        assert_eq!(
            err_with(&env, "#my_macro0(#x)"),
            "a function/macro cannot be called from inside another function/macro's \
             argument list: char at 11"
        );
    }

    /// Same check, triggered by `$` rather than `#` — confirms `internalMacro` fires on
    /// EITHER reserved character, not just the one that happens to start a nested macro
    /// call, and rules out (see WB-019's doc comment) the `$`-group-reference half of
    /// that bug: `$` can never reach `putMacro`'s substitution step at all.
    #[test]
    fn macro_call_argument_containing_dollar_is_also_blocked() {
        let env = macro_test_env();
        assert_eq!(
            err_with(&env, "#my_macro0($5)"),
            "a function/macro cannot be called from inside another function/macro's \
             argument list: char at 11"
        );
    }

    /// `PredicateTest.macroCallWithMissingClosingParenThrowsUnbalancedParen`.
    #[test]
    fn macro_call_with_missing_closing_paren_throws_unbalanced_paren() {
        let env = macro_test_env();
        assert_eq!(
            err_with(&env, "#my_macro0(a"),
            "unbalanced parenthesis: char at 12"
        );
    }

    /// `PredicateTest.undefinedMacroFileThrows`.
    #[test]
    fn undefined_macro_file_throws() {
        assert_eq!(
            err("#no_such_macro_xyz(a)"),
            "Macro does not exist: no_such_macro_xyz"
        );
    }

    /// `PredicateTest.functionCallWithSingleEmptyArgumentIsTreatedAsZeroArgs`.
    #[test]
    fn function_call_with_single_empty_argument_is_treated_as_zero_args() {
        let env = InMemoryPredicateEnv::new().with_function("endsIn2Zeros", stub_automaton(1));
        assert_eq!(
            err_with(&env, "$endsIn2Zeros()"),
            "function endsIn2Zeros requires 1 arguments: char at 1"
        );
    }

    /// `PredicateTest.functionCallWithEmptyArgumentAmongMultipleThrows`.
    #[test]
    fn function_call_with_empty_argument_among_multiple_throws() {
        let env = InMemoryPredicateEnv::new().with_function("endsIn2Zeros", stub_automaton(1));
        assert_eq!(
            err_with(&env, "$endsIn2Zeros(a,)"),
            "argument 2 of the function endsIn2Zeros cannot be empty: char at 1"
        );
    }

    // -- Nested word-index / function-argument sub-`Predicate` construction --------

    /// Each index of `T[i][j+1]` is its own sub-`Predicate`, including one with a real
    /// arithmetic sub-expression (`j+1`, not just a bare variable) — proof that a
    /// bracket's contents are tokenized as an arbitrary predicate, not special-cased to
    /// a single identifier.
    #[test]
    fn word_index_brackets_are_independently_tokenized_sub_predicates() {
        let env = InMemoryPredicateEnv::new().with_word("T", stub_automaton(2));
        assert_eq!(post_with(&env, "T[i][j+1]"), "i:j:1:+_msd_2:T");
    }

    /// A word index containing NESTED brackets of its own (`T[a[0]]`, i.e. the index
    /// expression is itself `a[0]` — a word occurrence of `a` indexed by the literal
    /// `0`) exercises `put_word`'s inner-bracket-depth counting, not just the top-level
    /// open/close pair.
    #[test]
    fn word_index_may_itself_contain_nested_brackets() {
        let env = InMemoryPredicateEnv::new()
            .with_word("T", stub_automaton(1))
            .with_word("a", stub_automaton(1));
        assert_eq!(post_with(&env, "T[a[0]]"), "0:a:T");
    }

    /// Multi-argument function calls: each comma-separated argument is its own
    /// sub-`Predicate`, including one with a real arithmetic sub-expression.
    #[test]
    fn function_call_arguments_are_independently_tokenized_sub_predicates() {
        let env = InMemoryPredicateEnv::new().with_function("phi", stub_automaton(3));
        assert_eq!(post_with(&env, "$phi(a, b+1, 2)"), "a:b:1:+_msd_2:2:phi");
    }

    /// The `real_starting_position`/UTF-16 threading (Ruling 3's "corollary for U4")
    /// through a nested function argument: the argument `x=1` starts at byte offset 5 in
    /// `"$phi(x=1)"`, so its own tokens must report position 5, not 0.
    #[test]
    fn function_argument_position_is_correctly_offset() {
        let env = InMemoryPredicateEnv::new().with_function("phi", stub_automaton(1));
        let p = Predicate::new(&env, "$phi(x=1)").unwrap();
        let positions: Vec<usize> = p
            .post_order()
            .iter()
            .map(|t| t.position_in_predicate())
            .collect();
        // post-order: x, 1, =_msd_2, phi -- `x` is at byte/char offset 5.
        assert_eq!(positions[0], 5);
    }

    // -- WB-017: `putWord`/`putFunction`'s empty-index/argument position defect -----

    /// **WB-017**, direct case (`real_starting_position == 0`, so this alone doesn't
    /// distinguish the bug from correct behavior — see the NESTED case right below for
    /// that): `PredicateTest.wordWithEmptyIndexThrows`/
    /// `functionCallWithEmptyArgumentAmongMultipleThrows` already pin the top-level
    /// shape above. This one adds the FUNCTION side at top level for symmetry.
    #[test]
    fn wb017_empty_function_argument_position_at_top_level() {
        let env = InMemoryPredicateEnv::new().with_function("endsIn2Zeros", stub_automaton(1));
        assert_eq!(
            err_with(&env, "$endsIn2Zeros(a,)"),
            "argument 2 of the function endsIn2Zeros cannot be empty: char at 1"
        );
    }

    /// **WB-017**, the case that actually distinguishes the bug from correct behavior:
    /// nesting the empty word index inside a function argument gives it a non-zero
    /// `real_starting_position` (14, `F`'s own byte offset within the full query). The
    /// CORRECT position would be 14; Java (and this port, ported verbatim) reports 0 —
    /// confirmed empirically against real `walnut-java`
    /// (`$endsIn2Zeros(F[])` -> `"index 1 of the word F cannot be empty: char at 0"`).
    #[test]
    fn wb017_empty_word_index_position_is_not_real_starting_position_adjusted() {
        let env = InMemoryPredicateEnv::new()
            .with_word("F", stub_automaton(1))
            .with_function("endsIn2Zeros", stub_automaton(1));
        assert_eq!(
            err_with(&env, "$endsIn2Zeros(F[])"),
            "index 1 of the word F cannot be empty: char at 0"
        );
    }

    /// **WB-017**, the FUNCTION-side analogue of the test above: an empty function
    /// argument nested inside a WORD index likewise reports the un-adjusted local
    /// position (1, `endsIn2Zeros`' own offset within the nested index text) instead of
    /// the correct absolute position (3). Confirmed empirically against real
    /// `walnut-java` (`F[$endsIn2Zeros(a,)]` -> `char at 1`, not `char at 3`).
    #[test]
    fn wb017_empty_function_argument_position_is_not_real_starting_position_adjusted() {
        let env = InMemoryPredicateEnv::new()
            .with_word("F", stub_automaton(1))
            .with_function("endsIn2Zeros", stub_automaton(1));
        assert_eq!(
            err_with(&env, "F[$endsIn2Zeros(a,)]"),
            "argument 2 of the function endsIn2Zeros cannot be empty: char at 1"
        );
    }

    // -- WB-018: a satisfied word occurrence silently swallows trailing garbage -----

    /// **WB-018**: once a word's declared arity is satisfied, `putWord` still checks for
    /// ONE more chained `[`, and if it finds one but that bracket is never closed, the
    /// whole unclosed remainder is silently discarded — NOT an error, matching real
    /// Walnut exactly (`F[a][b` and `F[a][b=1` both succeed there, confirmed empirically,
    /// dropping `[b`/`[b=1` with no diagnostic at all).
    #[test]
    fn wb018_trailing_unclosed_bracket_after_satisfied_arity_is_silently_dropped() {
        let env = InMemoryPredicateEnv::new().with_word("F", stub_automaton(1));
        assert_eq!(post_with(&env, "F[a][b"), "a:F");
        assert_eq!(post_with(&env, "F[a][b=1"), "a:F");
    }

    // -- WB-019: `putMacro`'s `%N` substitution inherits Java's replacement-string quirks

    /// **WB-019**: a macro-call argument ending in a lone, unescaped backslash makes the
    /// substitution step fail with the exact (uncaught, non-`WalnutException`) message
    /// Java's `Matcher.appendReplacement` reports for a dangling escape.
    #[test]
    fn wb019_macro_argument_trailing_backslash_reports_javas_exception_text() {
        let env = InMemoryPredicateEnv::new().with_macro("echo", "%0");
        match lex_with(&env, "#echo(\\)").unwrap_err() {
            LexError::MacroArgumentReplacementError { message } => {
                assert_eq!(message, "character to be escaped is missing");
            }
            other => panic!("expected MacroArgumentReplacementError, got {other}"),
        }
    }

    /// **WB-019**: `\x` in an argument silently becomes literal `x` (the backslash is
    /// swallowed as an escape character), rather than passing the two-character
    /// sequence through as literal argument text.
    #[test]
    fn wb019_macro_argument_backslash_escapes_the_following_character() {
        let env = InMemoryPredicateEnv::new().with_macro("echo", "%0");
        let p = Predicate::new(&env, "#echo(\\x)").unwrap();
        assert_eq!(p.predicate(), "x");
        assert_eq!(p.to_string(), "x");
    }

    // -- Ruling 3: no macro-expansion depth/cycle guard ------------------------------

    /// A [`PredicateEnv`] wrapper that counts `macro_text` calls and fails once a cap is
    /// reached — the mechanism this crate's other "must not hang" tests use (per this
    /// unit's brief: "use a cycle-bounded macro definition ... before you assert the
    /// expansion truly doesn't self-limit"). `#loop` expands to the byte-identical text
    /// `#loop(x)` (its body never references `%0`, so the argument's own content is
    /// irrelevant), so each re-lex pass is O(1) work — genuinely unbounded recursion,
    /// made observable without ever actually running unbounded.
    struct CountingMacroEnv<'a> {
        inner: &'a InMemoryPredicateEnv,
        calls: std::cell::Cell<u32>,
        cap: u32,
    }

    impl PredicateEnv for CountingMacroEnv<'_> {
        fn number_system(&self, name: &str) -> Result<Rc<NumberSystem>, PredicateEnvError> {
            self.inner.number_system(name)
        }
        fn word(&self, name: &str) -> Result<Automaton, PredicateEnvError> {
            self.inner.word(name)
        }
        fn function(&self, name: &str) -> Result<Automaton, PredicateEnvError> {
            self.inner.function(name)
        }
        fn macro_text(&self, name: &str) -> Result<String, PredicateEnvError> {
            let n = self.calls.get() + 1;
            self.calls.set(n);
            if n > self.cap {
                return Err(PredicateEnvError::MacroDoesNotExist {
                    name: name.to_string(),
                });
            }
            self.inner.macro_text(name)
        }
    }

    #[test]
    fn macro_expansion_has_no_depth_guard_and_will_recurse_until_the_environment_stops_it() {
        let inner = InMemoryPredicateEnv::new().with_macro("loop", "#loop(x)");
        let capped = CountingMacroEnv {
            inner: &inner,
            calls: std::cell::Cell::new(0),
            cap: 50,
        };
        let err = Predicate::new(&capped, "#loop(x)").unwrap_err();
        // If the lexer had ANY self-imposed recursion limit, `calls` would stop well
        // short of `cap + 1` -- it doesn't, proving `put_macro` really does keep calling
        // `macro_text` with no limit of its own, matching Ruling 3's explicit
        // instruction to port Java's total absence of one.
        assert_eq!(
            capped.calls.get(),
            51,
            "the lexer must keep re-expanding with no self-imposed limit"
        );
        assert!(matches!(
            err,
            LexError::Env(PredicateEnvError::MacroDoesNotExist { .. })
        ));
    }
}
