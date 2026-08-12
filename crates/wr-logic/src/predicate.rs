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
//! # Scope: what this unit does NOT build (the U3/U4 boundary)
//!
//! `Predicate.java` also contains `putWord`/`putFunction`/`putMacro`/
//! `parseParenthesizedArguments`/`readMacroFile` — the three token kinds that reach out to
//! [`crate::predicate_env::PredicateEnv`]'s word/function/macro lookups and build nested
//! `Predicate`s. Those are **U4**, and this unit deliberately stops at the boundary:
//!
//! * the four patterns that *recognize* them (`PATTERN_FOR_WORD`,
//!   `PATTERN_FOR_WORD_WITH_DELIMITER`, `PATTERN_FOR_FUNCTION`, `PATTERN_FOR_MACRO`) are
//!   ported and sit at their exact Java priority in the scanning chain, so the *shape* of
//!   the token stream is already right — `T[i]` is recognized as a word rather than
//!   mis-lexed as the variable `T`, and `$f(x)`/`#m(x)` likewise;
//! * the `lastTokenWasOperator` adjacency check that Java performs **before** calling each
//!   of them is ported here too (it is part of the scanning loop, not of `putWord`), so all
//!   four of `PredicateTest`'s "operator is missing" characterization tests pass today;
//! * their *construction* is a stub — [`Predicate::put_word`]/[`Predicate::put_function`]/
//!   [`Predicate::put_macro`] each return [`LexError::NotYetImplemented`]. U4 replaces the
//!   three method bodies (and adds `Token::Word`/`Token::Function` to [`crate::token`]);
//!   **the scanning loop itself should not need to change**, which is the point of drawing
//!   the line at the method boundary rather than at the pattern-table boundary.
//!
//! `PATTERN_LEFT_BRACKET` (`:110`) is the one pattern-table entry not ported here: it is
//! used exclusively by `putWord`'s index-scanning loop, so it belongs to U4 alongside the
//! loop that reads it (an unused compiled pattern here would be dead code, not a head
//! start).
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
use crate::token::{symbols, AlphabetLetter, NumberLiteral, Operator, Token, TokenError, Variable};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Which of U4's three deferred token kinds a [`LexError::NotYetImplemented`] refers to.
///
/// Kept as an enum rather than a `&'static str` so tests can match on the kind without
/// asserting on prose, and so U4 can delete the variants one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredTokenKind {
    /// `PATTERN_FOR_WORD` (`Predicate.java:99`) — `T[i]`, handled by `putWord(ns, false)`.
    Word,
    /// `PATTERN_FOR_WORD_WITH_DELIMITER` (`:100`) — `.AUTOMATON[i]`, `putWord(ns, true)`.
    WordWithDelimiter,
    /// `PATTERN_FOR_FUNCTION` (`:101`) — `$name(...)`, handled by `putFunction`.
    Function,
    /// `PATTERN_FOR_MACRO` (`:102`) — `#name(...)`, handled by `putMacro`.
    Macro,
}

impl DeferredTokenKind {
    /// The Java method this port still has to grow, for the error message.
    fn java_method(self) -> &'static str {
        match self {
            DeferredTokenKind::Word | DeferredTokenKind::WordWithDelimiter => "Predicate.putWord",
            DeferredTokenKind::Function => "Predicate.putFunction",
            DeferredTokenKind::Macro => "Predicate.putMacro",
        }
    }

    /// The syntax as a user would have written it, for the error message.
    fn syntax(self) -> &'static str {
        match self {
            DeferredTokenKind::Word => "word/sequence index (`T[i]`)",
            DeferredTokenKind::WordWithDelimiter => "delimited word index (`.NAME[i]`)",
            DeferredTokenKind::Function => "function call (`$name(...)`)",
            DeferredTokenKind::Macro => "macro call (`#name(...)`)",
        }
    }
}

/// Every failure the tokenizer itself can report.
///
/// Module-local per this crate's established idiom ([`TokenError`], [`PredicateEnvError`],
/// `wr_core::numsys::NumSysError`) rather than one unified `WalnutError` — see the Phase 3
/// plan's resolved gap #8. Every variant except [`Self::NotYetImplemented`] renders
/// `WalnutException`'s verbatim message text, since Tier 1's `error*` fixtures compare it.
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
    /// Raised by [`Operator::push_onto`] (the shunting yard) or by the final
    /// operator-stack drain — in practice always [`TokenError::UnbalancedParenthesis`].
    Token(TokenError),
    /// A [`PredicateEnv`] lookup failed. Today only [`PredicateEnv::number_system`] is
    /// reachable from this unit (relational, arithmetic and number-literal tokens each
    /// resolve the *currently active* number system at construction time, exactly as
    /// Java's `NumberSystem.getComputeIfAbsent(currentNumberSystem)` does); U4 adds the
    /// word/function/macro lookups behind the same variant.
    Env(PredicateEnvError),
    /// Not a Java behavior: this port has recognized a token kind whose construction is
    /// U4's (see the module docs' U3/U4 boundary). Deliberately a hard, loud error rather
    /// than a silent skip or a placeholder token — a placeholder would let a later unit's
    /// test pass against a token stream real Walnut never produces.
    NotYetImplemented {
        kind: DeferredTokenKind,
        position: usize,
    },
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
            LexError::NotYetImplemented { kind, position } => write!(
                f,
                "{} is not implemented yet ({} lands in Phase 3a U4): char at {position}",
                kind.syntax(),
                kind.java_method()
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

    /// `Predicate.putWord(String, boolean)` (`:287-337`) — **deferred to U4**.
    ///
    /// The signature is already the one U4 needs: `default_number_system` is the
    /// `currentNumberSystem` Java passes (it becomes the nested index `Predicate`s'
    /// default), `caps` is the live word match (Java re-reads `MATCHER_FOR_WORD` /
    /// `MATCHER_FOR_WORD_WITH_DELIMITER`), and the return value is the offset to resume
    /// scanning at. The body is the only thing U4 has to replace.
    fn put_word(
        &mut self,
        _env: &dyn PredicateEnv,
        _default_number_system: &str,
        caps: &Captures,
        with_delimiter: bool,
    ) -> Result<usize, LexError> {
        let name = caps
            .get_group_by_name("name")
            .expect("group `name` matched");
        Err(LexError::NotYetImplemented {
            kind: if with_delimiter {
                DeferredTokenKind::WordWithDelimiter
            } else {
                DeferredTokenKind::Word
            },
            position: self.position(name.start),
        })
    }

    /// `Predicate.putFunction(String)` (`:448-491`) — **deferred to U4**. See
    /// [`Self::put_word`] on the signature.
    fn put_function(
        &mut self,
        _env: &dyn PredicateEnv,
        _default_number_system: &str,
        caps: &Captures,
    ) -> Result<usize, LexError> {
        let name = caps
            .get_group_by_name("name")
            .expect("group `name` matched");
        Err(LexError::NotYetImplemented {
            kind: DeferredTokenKind::Function,
            position: self.position(name.start),
        })
    }

    /// `Predicate.putMacro()` (`:419-446`) — **deferred to U4**.
    ///
    /// This is the one whose U4 body rewrites `self.predicate` in place and returns
    /// `matcher.start()` so the scanning loop re-lexes the expansion; `predicate_env.rs`'s
    /// Ruling 3 spells out the strategy. Nothing in this unit holds a borrow of
    /// `self.predicate` across the call, so that body change is local.
    fn put_macro(&mut self, _env: &dyn PredicateEnv, caps: &Captures) -> Result<usize, LexError> {
        let name = caps
            .get_group_by_name("name")
            .expect("group `name` matched");
        Err(LexError::NotYetImplemented {
            kind: DeferredTokenKind::Macro,
            position: self.position(name.start),
        })
    }
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
    /// not the variable `T` followed by an undefined `[`. (Construction is U4's; what this
    /// pins is that the *chain* routes it to the word arm.)
    #[test]
    fn word_pattern_outranks_variable_pattern() {
        match lex("T[i]").unwrap_err() {
            LexError::NotYetImplemented { kind, position } => {
                assert_eq!(kind, DeferredTokenKind::Word);
                assert_eq!(position, 0);
            }
            other => panic!("expected a deferred word token, got {other}"),
        }
    }

    /// Likewise `.EVEN[i]`: the delimited-word pattern claims it, and its leading `E` is
    /// never mistaken for a quantifier.
    #[test]
    fn delimited_word_is_not_lexed_as_a_quantifier() {
        match lex(".EVEN[i]").unwrap_err() {
            LexError::NotYetImplemented { kind, position } => {
                assert_eq!(kind, DeferredTokenKind::WordWithDelimiter);
                assert_eq!(position, 1, "the name starts after the delimiter dot");
            }
            other => panic!("expected a deferred delimited-word token, got {other}"),
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
    /// `$`/`#` would be undefined tokens.
    #[test]
    fn function_and_macro_patterns_outrank_the_left_parenthesis() {
        match lex("$phi(x)").unwrap_err() {
            LexError::NotYetImplemented { kind, position } => {
                assert_eq!(kind, DeferredTokenKind::Function);
                assert_eq!(position, 1, "the name starts after the `$`");
            }
            other => panic!("expected a deferred function token, got {other}"),
        }
        match lex("#m(x)").unwrap_err() {
            LexError::NotYetImplemented { kind, position } => {
                assert_eq!(kind, DeferredTokenKind::Macro);
                assert_eq!(position, 1, "the name starts after the `#`");
            }
            other => panic!("expected a deferred macro token, got {other}"),
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
}
