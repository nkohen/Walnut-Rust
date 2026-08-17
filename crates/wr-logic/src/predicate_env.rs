// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `PredicateEnv` — the lexer/parser's **lookup context** (Phase 3a, U1).
//!
//! This module is a *design* unit, not a transliteration of one Java file: there is no
//! `PredicateEnv.java`. It exists because `Main/Predicate.java`'s tokenizer reaches out
//! to four pieces of global, file-backed state *while lexing*, and Rust has no faithful
//! equivalent of "a `static` map that any class may mutate from anywhere". Java's four
//! reach-outs, all inside `tokenizeAndComputePostOrder` and its helpers:
//!
//! | Java call site | What it needs |
//! |---|---|
//! | `NumberSystem.getComputeIfAbsent(currentNumberSystem)` (`Predicate.java:178`, `:185`, `:213`) | a `NumberSystem` by *already-normalized* name, memoized in a `static` map |
//! | `new Automaton(Session.getReadFileForWordsLibrary(name + ".txt"))` (`Predicate.java:295`) | an automaton for a named word/sequence (`T`, `F`, …) |
//! | `Automaton.readAutomatonFromFile(name)` → `Session.getReadFileForAutomataLibrary` (`Predicate.java:453`, `Automaton.java:148-150`) | an automaton for a user-defined `$name` |
//! | `readMacroFile(name)` → `Session.getReadFileForMacroLibrary` (`Predicate.java:495-511`) | the raw *text* of `#name`, for substitution |
//!
//! [`PredicateEnv`] is exactly those four, and deliberately nothing else. Everything the
//! lexer can do without touching the filesystem (connectives, `E`/`A`/`I` quantifiers,
//! variables, number literals, `@`-letters, parens, the `?msd_k` number-system token's
//! *syntax*) stays in the lexer. That split is what makes U2–U4 testable against literal
//! predicate strings today, years before `wr-cli`'s `Session` exists — see
//! [`InMemoryPredicateEnv`].
//!
//! # Ruling 1 — number-system memoization lives in `wr-core`, NOT here
//!
//! Java caches one `NumberSystem` per name in a `static HashMap`, hands the *same
//! instance* to every token in a formula, and then lets those tokens **mutate** it at
//! `act()` time: `getConstant`/`getMultiplication`/`getDivision` memoize into
//! `constantsDynamicTable` / `multiplicationsDynamicTable` / `divisionsDynamicTable`
//! (`NumberSystem.java:126-128`). Naively ported, "many tokens each needing `&mut` to one
//! shared object" forces an ownership choice (`Rc<RefCell<NumberSystem>>` vs. an
//! arena + index) that would infect the signature of every `Token::act`.
//!
//! The ruling (settled before any parser code was written, and recorded in `PORTING.md`):
//!
//! * **`wr_core::numsys::NumberSystem` owns its own memoization, behind interior
//!   mutability.** The three dynamic tables are `RefCell<BTreeMap<BigInt, Automaton>>` and
//!   `get_constant`/`get_multiplication`/`get_division` take `&self`. **Delivered by U5**
//!   (Phase 3a) — see `wr_core::numsys::NumberSystem`'s `constants_dynamic_table` doc
//!   comment for the implementation and its borrow discipline. Nothing in `wr-logic` may
//!   wrap the handle in a `RefCell` of its own; that would recreate the aliasing problem
//!   this ruling exists to prevent, and would put two independent memo caches behind one
//!   logical number system.
//! * **This trait therefore hands out a shared, immutable handle** — [`std::rc::Rc`] —
//!   and never a `&mut NumberSystem`, a `RefCell`, or an index into a caller-owned arena.
//!   [`Rc`] rather than [`std::sync::Arc`] because `NumberSystem`'s memo tables are about
//!   to become `RefCell`s (so the type is `!Sync` regardless), and Walnut is
//!   single-threaded throughout.
//! * **The name → `NumberSystem` cache** (Java's `numberSystemHash`, i.e.
//!   `getComputeIfAbsent` itself) is the *implementation's* business, not the trait's:
//!   the real implementation (U14's `Session`) must cache, because a cache miss there
//!   means custom-base file I/O. [`InMemoryPredicateEnv`] caches too, so the double and
//!   the real thing share observable behavior. Note the one Java detail worth matching:
//!   `computeIfAbsent` leaves the map **unmodified** when the mapping function throws, so
//!   a failed resolution is not negatively cached and a later identical lookup retries.
//!
//! Consequence for U5, as delivered: custom bases (`msd_fib`, `msd_pell`, …) slotted in
//! *underneath* [`PredicateEnv::number_system`] without changing its signature — the
//! implementation grows a "not a plain `msd_k`/`lsd_k`? then load the custom-base files"
//! branch (`wr_core::numsys::NumberSystem::with_custom_base_files`, fed by whatever the
//! implementor can read off disk), and the trait keeps returning `Rc<NumberSystem>`. The
//! in-memory double below still resolves only what `NumberSystem::new` can build, since it
//! has no filesystem to consult; U14's real `Session` is where the file probing lands.
//!
//! # Ruling 2 — `regex-automata`, for `\G`-anchored lexing
//!
//! Java's lexer is 15 `\G`-anchored `Pattern`s driven by `Matcher.find(index)`
//! (`Predicate.java:89-110`, `:157-242`). `Matcher.find(int)` resets the matcher and sets
//! the append position to `index`, so `\G` pins the match to start at *exactly* `index` —
//! the matcher never scans forward. That is load-bearing: the tokenizer's whole
//! priority-ordered `if/else` chain assumes "does token kind *k* start right here?", and
//! a forward-scanning match would silently accept a token from further down the string.
//!
//! The `regex` crate has no `\G` anchor, and its `find_at` still scans forward, so it
//! cannot express this at all. `regex-automata`'s
//! `Input::new(hay).span(index..).anchored(Anchored::Yes)` is the exact equivalent, and
//! `meta::Regex` keeps `regex`'s own syntax and engines. The dependency is declared in
//! the workspace root `Cargo.toml` with its justification, mirroring the `num-bigint`
//! precedent. [`tests::anchored_matching_is_java_g_anchoring`] pins the behavior U3 needs.
//!
//! ## Java-regex → Rust-regex dialect divergences U3 **must** handle
//!
//! Verified empirically against `regex-automata` 0.4 while wiring this dependency in —
//! these are not stylistic, each one silently changes what the lexer accepts:
//!
//! 1. **No look-behind.** `LOGICAL_OPERATORS` is `"(?<!\\.)(`|\\^|\\&|\\~|\\||=>|<=>|E|A|I|\u{02DC}|\u{0303})"`
//!    (`Predicate.java:80`); Rust's regex engines reject `(?<!…)` outright (confirmed: the
//!    pattern fails to compile). The negative look-behind is zero-width at the position
//!    just before group 1, so U3 should compile `\s*(op…)` **without** it and then reject
//!    the match when the byte immediately preceding `start(1)` is `'.'`. That is exactly
//!    equivalent, including at `start(1) == 0` (Java's look-behind succeeds there, since
//!    there is no preceding character — so the Rust check must treat "no preceding byte"
//!    as *pass*, not fail). This guard is what lets `.AUTOMATON[…]`-style word names
//!    beginning with `A`/`E`/`I` lex as words rather than as quantifiers.
//! 2. **`\s`/`\w`/`\d` are Unicode in Rust, ASCII in Java.** Java's `Pattern` defaults to
//!    `\s == [ \t\n\x0B\f\r]`, `\w == [a-zA-Z_0-9]`, `\d == [0-9]` unless
//!    `UNICODE_CHARACTER_CLASS` is set (it never is here). Rust's are Unicode-aware:
//!    confirmed that `\s+` matches U+00A0 (NBSP) while `(?-u:\s)+` does not. Every ported
//!    pattern must therefore wrap these classes as `(?-u:\s)`, `(?-u:\w)`, `(?-u:\d)` (or
//!    spell them out), or the Rust lexer will accept whitespace/identifiers real Walnut
//!    rejects with "undefined token".
//! 3. **Character-class intersection survives.** Java's `[a-zA-Z&&[^AEI]]` (the
//!    "alphanumeric but not starting with a reserved quantifier letter" class,
//!    `Predicate.java:72`) parses unchanged in Rust — `regex` supports the same `&&`
//!    nested-class syntax. No rewrite needed. (Pinned by a test below, so a future
//!    dependency bump that drops it fails loudly here rather than in U3.)
//! 4. **Group numbering must be re-derived, not assumed.** Java reads `group(1)` /
//!    `start(1)` / `end()` throughout, and `PATTERN_FOR_MACRO` reads `group(1)` *and*
//!    `group(2)` (`Predicate.java:102`, `:422`, `:442` — group 1 is the leading
//!    whitespace, which `putMacro` re-emits into the rewritten predicate). Rust numbers
//!    groups the same way (left-to-right by opening paren), but the ported patterns are
//!    assembled from shared `const` fragments, so a fragment that gains or loses a group
//!    renumbers every pattern built from it. U3 should use **named** groups.
//!
//! # Ruling 3 — the mutable-predicate / macro re-lex strategy (for U4)
//!
//! `Predicate.putMacro` (`Predicate.java:419-446`) does something a borrowing lexer
//! cannot copy literally: it **rewrites the string it is currently lexing**, then rebuilds
//! all 15 matchers over the new string (`initializeMatchers()`, `:444`) and returns
//! `matcher.start()` so the main loop resumes at the position where `#name(` began — now
//! occupied by the expansion. Concretely, with `s` the predicate, `m` the macro match:
//!
//! ```text
//! s := s[0 .. m.start()] + m.group(1) + expanded + s[endIndex + 1 ..]
//! index := m.start()
//! ```
//!
//! where `m.group(1)` is the whitespace the macro pattern consumed before `#`, `expanded`
//! is the macro file's text with `%0`, `%1`, … replaced by the parenthesized arguments
//! **in descending index order** (`:435-437` — descending matters: `%1`'s replacement must
//! not be re-scanned when `%0`… wait, descending order is what Java does and `%10` vs
//! `%1` is exactly why; port the loop direction verbatim), and `endIndex` is the offset of
//! the `)` closing the argument list.
//!
//! **The Rust strategy (do this, don't re-derive it):**
//!
//! * The lexer owns its buffer — `struct Lexer { src: String, index: usize, … }` with
//!   `src: String`, never `&'a str` borrowed from the caller. Callers pass `&str`; the
//!   lexer copies once at construction. The cost is one allocation per `Predicate`
//!   (including the nested ones built for word indices and function arguments), which is
//!   irrelevant next to the automaton work each one triggers.
//! * **Never hold a match across a buffer edit.** `regex-automata` returns `Span`s
//!   (byte-offset ranges), not borrows, so the natural style already avoids this: extract
//!   the `usize` offsets and any needed `String` copies first, *then* mutate `src`. Do not
//!   keep a `&str` slice of `src` alive across the `putMacro` rewrite — that is the
//!   borrowck error this ruling exists to pre-empt, and the fix is always "copy the
//!   captured text out before rewriting", not `unsafe` and not a second buffer.
//! * **Patterns are compiled once, globally; only the haystack changes.** Java rebuilds
//!   15 `Matcher`s because a `Matcher` binds a `Pattern` to a `CharSequence`. In Rust a
//!   `meta::Regex` binds to nothing — the haystack is a per-call `Input`. So
//!   `initializeMatchers()` has **no Rust counterpart at all**: put the 15 compiled
//!   patterns in a `OnceLock`/`LazyLock` static, and `putMacro` becomes "edit `src`, set
//!   `index = m_start`, continue the loop". Anything that *looks* like it needs matcher
//!   rebuilding is a sign of a borrowed-state design that should be corrected instead.
//! * **Byte offsets, not char offsets.** Java's positions are UTF-16 indices;
//!   `regex-automata`'s are UTF-8 byte offsets. These agree for ASCII input, which is
//!   every realistic Walnut predicate, but the two non-ASCII characters that *are* legal
//!   in the grammar — `\u{02DC}` (˜) and `\u{0303}` (◌̃), both spellings of negation
//!   (`Predicate.java:80`) — are 2 bytes each in UTF-8 and 1 unit each in UTF-16. Any
//!   position that reaches a user-visible message ("char at N", built from
//!   `realStartingPosition + index`) will therefore differ from Java's for a predicate
//!   containing one of them. U3/U4 must decide this explicitly (track a parallel UTF-16
//!   count, or accept and document the divergence) — do not let it be discovered by a
//!   Tier-1 `error*` fixture. `realStartingPosition` threading through nested
//!   `Predicate`s (word indices at `:310`, function arguments at `:462-463`) has the same
//!   unit.
//! * **No macro-expansion depth limit exists in Java.** `#a` expanding to `#a(…)` loops
//!   forever (well, until the string exhausts memory). Port that faithfully; if a guard
//!   is ever wanted it is a deliberate, signed-off divergence, not a silent one.
//!
//! # Ruling 4 — fresh identifiers: threaded state, not a `static`
//!
//! `Token.getUniqueString()` (`Token.java:33-40`) is a JVM-global `static long` counter
//! behind the prefix `WALNUT_8AthA0PZZI_`. It has four call sites —
//! `ArithmeticOperator.java:116` and `:155`, `VariableExpression.java:38` (which
//! *appends* the fresh string to the user's identifier), and
//! `NumberLiteralExpression.java:63` — all of which mint a name for a synthetic track
//! that is existentially quantified away in the same `act()`.
//!
//! [`FreshIdentifiers`] replaces it: a plain value type with an owned counter, to be held
//! as **one field of the evaluation context** that U11's postfix-token executor threads
//! through `Token::act` (alongside the `&dyn PredicateEnv` and, once U0a is wired
//! through, the logging context). That is the same conversion `Session`'s globals get in
//! U14 and `Logging`'s got in U0a — it is the sanctioned mechanical-fidelity deviation
//! (`PORTING.md`: Java `static` mutable → explicit context), so U2/U9's ports should
//! thread it from the start rather than "preserve then fix later".
//!
//! **Scope of the counter, and why per-evaluation is safe.** Java's counter is global and
//! monotonic for the whole process: two successive `eval` commands get *different* fresh
//! names, and a Rust context created per evaluation would restart at `_1`. That is only
//! observable if a fresh name reaches output — it does not: every one of the four call
//! sites adds its name to the `quantify` list in the same `act()`. Checked directly: the
//! literal `WALNUT_8AthA0PZZI` occurs nowhere in `walnut-java` except `Token.java` itself
//! — no golden fixture, no expected-output file. So per-evaluation scoping is safe today.
//! [`FreshIdentifiers`] is a plain owned value precisely so that raising its scope (to the
//! `Session`, if some future output ever *does* surface one) stays a one-line change.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use wr_core::automaton::Automaton;
use wr_core::determinize::DeterminizeContext;
use wr_core::numsys::{NumSysError, NumberSystem};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure a [`PredicateEnv`] lookup can report, as a module-local enum rather
/// than a stringly-typed error — Phase 1/2's established idiom (`NumSysError`,
/// `ReadError`, `ParseMethodsError`), which Phase 3's plan reaffirmed over a single
/// unified `WalnutError`.
///
/// Every variant corresponds to a real `WalnutException` thrown out of the Java call
/// sites tabulated in this module's docs. Tier 1's 38 `error*` fixtures compare
/// [`fmt::Display`] output (timing-normalized) against Walnut's real message text, so
/// verbatim wording is the *goal* for every variant — but **only
/// [`Self::FileDoesNotExist`] and [`Self::MacroDoesNotExist`] deliver on it today**.
/// [`Self::NumberSystem`] delivers on it as of U5 (`NumSysError` grew a `Display` emitting
/// Walnut's verbatim text). [`Self::MalformedAutomaton`] is still a self-described
/// placeholder in its own doc comment, pending a real `wr_io::ReadError` → Java-text
/// mapping (U13/U14).
#[derive(Debug)]
pub enum PredicateEnvError {
    /// `WalnutException.fileDoesNotExist` — `"File does not exist: " + address`
    /// (`WalnutException.java:49-51`), thrown from `AutomatonReader.readAutomaton`'s
    /// `catch (IOException)` (`AutomatonReader.java:99-102`) and therefore raised by
    /// both the word lookup (`Predicate.java:295`) and the function lookup
    /// (`Predicate.java:453` → `Automaton.java:149`).
    ///
    /// The payload is the address **as the implementation resolved it**, because that is
    /// what Java prints: a real, `Session`-backed implementation supplies an absolute
    /// path; [`InMemoryPredicateEnv`] supplies the library-relative path Walnut's own
    /// directory layout would have produced, so tests can assert on a stable string.
    FileDoesNotExist { address: String },

    /// `"Macro does not exist: " + filename` (`Predicate.java:508`). Note Java prints the
    /// *bare* macro name here, without the `.txt` extension and without the directory —
    /// unlike [`Self::FileDoesNotExist`], which prints the full address. Preserved.
    MacroDoesNotExist { name: String },

    /// The number-system name resolved to no usable `NumberSystem`. Wraps whatever
    /// `wr-core` reported (`"Number system X is not defined."` for an unknown base, and
    /// after U5 also the custom-base file failures and the explicit "unsupported
    /// numeration" answer for `msd_neg_*`).
    ///
    /// `name` is the *normalized* name that was looked up — see
    /// [`PredicateEnv::number_system`] — not the raw `?…` token from the query.
    NumberSystem { name: String, source: NumSysError },

    /// The word/function file was found and readable but did not parse as an automaton.
    ///
    /// This is a shape, not a port: in Java the failure surfaces as whichever
    /// `WalnutException` `AutomatonReader` threw, from inside the same constructor call.
    /// Here the parse lives in `wr-io`, which `wr-logic` must **not** depend on (that
    /// edge would invert the crate layering — `wr-io` reads `.txt` into `wr-core` types,
    /// and `wr-logic` sits above both), so the implementation stringifies its own reader
    /// error into `detail`. `address` is as in [`Self::FileDoesNotExist`].
    ///
    /// **Not fixture-ready — [`fmt::Display`]'s current rendering matches none of the
    /// real Java messages.** A malformed word/function `.txt` can raise (all from
    /// `Automata/AutomatonReader.java`, verified against `walnut-java` directly — cite
    /// method names, not line numbers, since those drift):
    ///
    /// * `firstParse`, no header line found → `WalnutException.fileEmpty`:
    ///   `"File is empty or contains only comments/whitespace: " + address`.
    /// * `firstParse`, content after a `true`/`false` line →
    ///   `WalnutException.fileHasConflict`:
    ///   `"A file that declares 'true'/'false' must not contain other statements: line "
    ///   + lineNumber + " of file " + address`.
    /// * `validateTransition`, a transition line before any state declaration → an
    ///   inline `WalnutException`: `"Must declare a state before declaring a list of
    ///   transitions: line " + lineNumber + " of file " + address"`.
    /// * `validateTransition`, input arity mismatch → an inline `WalnutException`:
    ///   `"This automaton requires a " + arity + "-tuple as input: line " + lineNumber
    ///   + " of file " + address"`.
    /// * `validateDeclaredStates`, a transition destination with no state block → an
    ///   inline `WalnutException`: `"State " + q + " is used but never declared
    ///   anywhere in file: " + address"`.
    /// * `readAutomaton`'s main loop, an unrecognized line →
    ///   `WalnutException.undefinedStatement`: `"Undefined statement: line at "
    ///   + lineNumber + " of file " + address"`.
    /// * `readAutomaton`, nondeterministic input on a DFAO →
    ///   `WalnutException.nonDeterministicO`: `"NFAOs are not supported.."` (no
    ///   `address` at all — out of scope for `wr-io` today per `reader.rs`'s module
    ///   docs, since this crate has no DFAO concept yet, but real Walnut can still
    ///   raise it).
    ///
    /// None of these are distinguished by `wr_io::ReadError` today — its variants carry
    /// line numbers or ids but (mostly) not the file address, and its
    /// [`fmt::Display`] is `{self:?}` (Rust `Debug` output), not Java text at all. So
    /// **this variant needs two things before it can be fixture-ready**: (1)
    /// `wr_io::ReadError` needs its own real, per-variant `Display` that reproduces the
    /// messages above (which likely means threading `address` into more of its
    /// variants), and (2) this arm needs to become a `{source}`-style passthrough
    /// instead of the current `"File does not parse: …"` wrapper. Both are real work
    /// against `wr-io`, which isn't wired into `wr-logic`'s actual file-loading path
    /// until U13–U15 — deliberately deferred rather than attempted here; tracked here so
    /// it isn't silently forgotten.
    MalformedAutomaton { address: String, detail: String },
}

impl fmt::Display for PredicateEnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Verbatim `WalnutException.java:50`.
            PredicateEnvError::FileDoesNotExist { address } => {
                write!(f, "File does not exist: {address}")
            }
            // Verbatim `Predicate.java:508`.
            PredicateEnvError::MacroDoesNotExist { name } => {
                write!(f, "Macro does not exist: {name}")
            }
            // `NumSysError` gained a `Display` emitting Walnut's verbatim message text in
            // U5, so this is now a plain pass-through, exactly as that note predicted.
            // Java does not wrap either: the `WalnutException` thrown by the
            // `NumberSystem(String)` constructor propagates out of
            // `NumberSystem.getComputeIfAbsent` unchanged, so re-stating the name here
            // would ADD text Walnut never prints. `name` is kept on the variant for
            // programmatic inspection (tests match on it) but is deliberately not printed.
            PredicateEnvError::NumberSystem { name: _, source } => write!(f, "{source}"),
            PredicateEnvError::MalformedAutomaton { address, detail } => {
                write!(f, "File does not parse: {address} ({detail})")
            }
        }
    }
}

impl std::error::Error for PredicateEnvError {}

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

/// The four things `Main/Predicate.java`'s tokenizer needs from the outside world.
///
/// Object-safe on purpose: U3/U4 should take `env: &dyn PredicateEnv`, not a generic
/// `<E: PredicateEnv>`. Both would let one lexer body serve [`InMemoryPredicateEnv`]
/// (tests, today) and U14's `Session`-backed implementation (the real CLI, later) —
/// that isn't the deciding factor. The deciding factor is that `Predicate` construction
/// is recursive: a word index (`T[i+1]`), a function argument (`$phi(x, y+1)`), and the
/// macro re-lex path (Ruling 3) all build a *nested* `Predicate` from inside an
/// in-progress lex, using the same environment the parent is using. A generic parameter
/// would have to be threaded through every one of those construction sites (and every
/// type that holds a `Predicate`, transitively) to stay monomorphized, for no benefit —
/// nothing here is on a hot enough path to need static dispatch, and dynamic dispatch
/// through one trait object keeps the recursive-construction sites simple.
///
/// **All four methods take `&self`.** Java's counterparts mutate global state — the
/// `numberSystemHash` cache above all — so implementations that cache use interior
/// mutability ([`RefCell`]), keeping the *caller's* signatures free of `&mut`. This is
/// what lets a nested `Predicate` (a word index, a function argument) be parsed with the
/// same shared `&dyn PredicateEnv` its parent is using, exactly as Java's statics allow,
/// with no borrow-splitting gymnastics in the lexer.
pub trait PredicateEnv {
    /// `NumberSystem.getComputeIfAbsent(name)` (`NumberSystem.java:211-213`).
    ///
    /// **`name` must already be normalized** — i.e. put through
    /// [`wr_core::numsys::normalize_number_system_token`], which maps `msd5` → `msd_5`,
    /// `msd` → `msd_2`, `fib` → `msd_fib`, `?msd_3` → `msd_3`, and `null`/empty →
    /// `msd_2`. That mirrors Java: `Predicate.java:224` normalizes at the point the
    /// `?…` token is lexed and pushes only normalized names onto its number-system
    /// stack, so every `getComputeIfAbsent` call downstream (`:178`, `:185`, `:213`)
    /// already sees a normalized name. Normalization is pure string work with no I/O, so
    /// it stays in the lexer rather than becoming this trait's problem — an
    /// implementation is free to normalize defensively, but must not *rely* on being
    /// handed a raw token.
    ///
    /// Implementations are expected to memoize (Java's `computeIfAbsent`), and to leave
    /// the cache untouched on failure so a later identical lookup retries. Custom bases
    /// (`msd_fib`, `msd_pell`, …) resolve through this same method once U5 lands: the
    /// *implementation* grows a file-loading fallback, and this signature does not move.
    ///
    /// The returned handle is shared and immutable — see this module's Ruling 1 for why
    /// it is not `&mut NumberSystem` and must not be wrapped in a `RefCell` here.
    fn number_system(&self, name: &str) -> Result<Rc<NumberSystem>, PredicateEnvError>;

    /// `new NumberSystem(name)` — the **unmemoized, freshly constructed** form, as opposed
    /// to [`Self::number_system`]'s `getComputeIfAbsent` cache lookup.
    ///
    /// Java has exactly one call site for this shape: `Token/Function.java:52`, the `$name(…)`
    /// token's own `ns` field (see [`crate::token::Function`]'s docs on why that non-sharing is
    /// preserved rather than "fixed"). It matters that this is a real *environment* method and
    /// not a direct `wr_core::numsys::NumberSystem::new` call, because Java's `NumberSystem`
    /// constructor reads `Custom Bases/` through `Session` — so `$fibonacci_factoreq(i,j,n)`
    /// under `?msd_fib` resolves the custom base in Java, and a port that bypassed the
    /// environment here could only ever build plain `msd_k`/`lsd_k` bases and would reject
    /// every custom-base `$`-call. (It did: this method was added in Phase 3b's U27 after the
    /// Tier-1 golden corpus caught exactly that — 15 of the corpus prelude's own `msd_fib`
    /// definitions failed with "Number system msd_fib is not defined.")
    ///
    /// The default implementation clones out of [`Self::number_system`]'s shared handle, which
    /// is correct for any environment (the two differ only in whether the memo table is
    /// populated as a side effect — not observable, since the table is a pure memo). An
    /// implementation backed by real files should override it with a genuinely unmemoized
    /// build, which is what Java does.
    fn fresh_number_system(&self, name: &str) -> Result<NumberSystem, PredicateEnvError> {
        Ok((*self.number_system(name)?).clone())
    }

    /// `new Automaton(Session.getReadFileForWordsLibrary(name + ".txt"))`
    /// (`Predicate.java:295`) — the automaton for a word/sequence occurrence like
    /// `T[i]` or `.AUTOMATON[i]`.
    ///
    /// `name` is the bare name as it appeared in the query: **no `.txt`**, and for the
    /// delimited form (`.NAME[…]`, `PATTERN_FOR_WORD_WITH_DELIMITER`) **no leading dot**
    /// either — Java captures only group 1 in both cases (`:99-100`, `:295`).
    ///
    /// Returns an **owned** automaton, not a shared handle, because that is what Java
    /// does: `putWord` constructs a fresh `Automaton` from the file on *every*
    /// occurrence, and `Word.act` then calls `wordAutomaton.bind(identifiers)`, mutating
    /// it. Two occurrences of `T` in one formula must therefore be two independent
    /// automata; handing out an `Rc` here would alias them and `bind` would corrupt both.
    ///
    /// The caller (U4's `Word` token) is responsible for the arity check Java does in
    /// `Word`'s constructor (`Word.java:39-45`: `validateArity(name, A.getArity())`);
    /// this method only produces the automaton.
    fn word(&self, name: &str) -> Result<Automaton, PredicateEnvError>;

    /// `Automaton.readAutomatonFromFile(name)` (`Predicate.java:453` →
    /// `Automaton.java:148-150`) — the automaton for a user-defined predicate invoked as
    /// `$name(…)`.
    ///
    /// `name` is the bare name **without the `$`** and without `.txt` (Java's
    /// `PATTERN_FOR_FUNCTION` captures group 1 after the literal `$`, `:101`).
    ///
    /// Owned, for the same reason as [`Self::word`] — `Function.act` calls
    /// `A.bind(identifiers)` and then reassigns `A` from `AutomatonLogicalOps.and`
    /// (`Function.java:91-93`), so the value must be the caller's alone.
    ///
    /// Separate from [`Self::word`] because the two read *different* libraries
    /// (`Word Automata Library/` vs `Automata Library/`), so the same bare name can
    /// legitimately resolve to two different automata — collapsing them into one method
    /// with a "kind" parameter would be a gratuitous divergence from the Java call graph.
    fn function(&self, name: &str) -> Result<Automaton, PredicateEnvError>;

    /// [`Self::word`] with the enclosing command's [`DeterminizeContext`], so that a
    /// **nondeterministic** library file's load-time `determinizeAndMinimize()`
    /// (`AutomatonReader.java:88-98`) consumes a `[strategy n …]`/`[export n …]` index the
    /// way Java's does.
    ///
    /// Java needs no such parameter: `DeterminizationStrategies.determinize` reads
    /// `Prover.mainProver.metaCommands` out of a global, so every determinization anywhere
    /// under a `::` command is counted — including one triggered by `Predicate`'s own
    /// lexer while it resolves `T[i]`/`$f(…)`. This port threads that global explicitly
    /// (`PORTING.md`'s standing ruling), which is why the context has to reach the
    /// environment at all.
    ///
    /// **The default implementation ignores `ctx`** and forwards to [`Self::word`]. That is
    /// correct for any environment that does not determinize on load — in particular
    /// [`InMemoryPredicateEnv`], whose automata are handed over already built, with no
    /// `AutomatonReader` step to count. A **file-backed** environment (`wr_cli::Session`)
    /// must override it.
    fn word_with_ctx(
        &self,
        name: &str,
        ctx: Option<&mut (dyn DeterminizeContext + '_)>,
    ) -> Result<Automaton, PredicateEnvError> {
        let _ = ctx;
        self.word(name)
    }

    /// [`Self::function`] with the enclosing command's [`DeterminizeContext`] — see
    /// [`Self::word_with_ctx`], which this mirrors exactly (same rationale, same default,
    /// same override obligation).
    fn function_with_ctx(
        &self,
        name: &str,
        ctx: Option<&mut (dyn DeterminizeContext + '_)>,
    ) -> Result<Automaton, PredicateEnvError> {
        let _ = ctx;
        self.function(name)
    }

    /// `Predicate.readMacroFile(name)` (`Predicate.java:495-511`) — the raw text of the
    /// macro invoked as `#name(…)`, before argument substitution.
    ///
    /// `name` is the bare name, no `#` and no `.txt` (`PATTERN_FOR_MACRO` group 2,
    /// `:102`, `:422`).
    ///
    /// **The returned text is the file's lines concatenated with *no* separator.** Java
    /// reads with `BufferedReader.readLine()` and `macro.append(line)` (`:503-505`), so
    /// every newline is dropped — a macro split across two lines is lexed as if the two
    /// halves were written adjacent, with no intervening space. That is a real, testable
    /// behavior (`a\nb` becomes the single token `ab`), not an artifact, so
    /// implementations must reproduce it; [`InMemoryPredicateEnv::with_macro_lines`]
    /// exists so tests can express it without a file. Argument substitution (`%0`, `%1`,
    /// …) is the *lexer's* job, not the environment's — see this module's Ruling 3.
    fn macro_text(&self, name: &str) -> Result<String, PredicateEnvError>;
}

// ---------------------------------------------------------------------------
// Fresh identifiers (Ruling 4)
// ---------------------------------------------------------------------------

/// `Token.getUniqueString()`'s counter (`Token.java:33-40`) as owned, threaded state
/// instead of a `static long`. See this module's Ruling 4 for the threading plan and for
/// why per-evaluation scoping is safe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FreshIdentifiers {
    /// `Token.uniqueCounter` (`Token.java:31`). Java's is a `long` starting at 0 and
    /// pre-incremented, so the first name ever handed out ends in `_1`, not `_0`.
    counter: u64,
}

impl FreshIdentifiers {
    /// `Token.WALNUT_UNIQUE_STRING` (`Token.java:32`), verbatim — "just a unique string",
    /// as Java's own comment puts it.
    pub const PREFIX: &'static str = "WALNUT_8AthA0PZZI_";

    /// A counter positioned exactly where Java's is at JVM start.
    pub fn new() -> Self {
        FreshIdentifiers::default()
    }

    /// `getUniqueString()` — increments, then returns `PREFIX + counter`.
    ///
    /// Named `next_identifier` rather than `next` so it isn't mistaken for (or linted
    /// against) [`Iterator::next`]; this is not an iterator, and making it one would
    /// invite `collect`-style bulk use that Java's per-`act()` call pattern never does.
    pub fn next_identifier(&mut self) -> String {
        self.counter += 1;
        format!("{}{}", Self::PREFIX, self.counter)
    }

    /// How many identifiers have been handed out. Exposed for tests and for the
    /// (currently hypothetical) day the counter has to become session-scoped.
    pub fn issued(&self) -> u64 {
        self.counter
    }
}

// ---------------------------------------------------------------------------
// The in-memory test double
// ---------------------------------------------------------------------------

/// An entirely in-memory [`PredicateEnv`], so U2–U4 can test the lexer/parser against
/// literal predicate strings with no filesystem, no `Session`, and no `wr-cli`.
///
/// This is the concrete thing that makes "the parser is testable before `wr-cli` exists"
/// true. It is **not** a mock in the assert-on-calls sense: it behaves like a real
/// environment whose libraries happen to have been populated by hand, including the
/// details that bite —
///
/// * number systems are memoized, and *not* negatively cached on failure, matching
///   `computeIfAbsent`;
/// * words and functions live in separate namespaces and are returned **by clone**, so a
///   formula mentioning the same word twice gets two independent automata;
/// * a missing word/function reports the library-relative address Walnut's own directory
///   layout would produce, so error assertions are stable and recognizable.
///
/// Build one with [`InMemoryPredicateEnv::new`] plus the `with_*` methods, which chain.
#[derive(Debug, Default)]
pub struct InMemoryPredicateEnv {
    /// Java's `NumberSystem.numberSystemHash` (`NumberSystem.java:211-213`). `RefCell`
    /// because the trait method takes `&self` — see the trait docs.
    number_systems: RefCell<BTreeMap<String, Rc<NumberSystem>>>,
    /// Stand-in for `Word Automata Library/`.
    words: BTreeMap<String, Automaton>,
    /// Stand-in for `Automata Library/`.
    functions: BTreeMap<String, Automaton>,
    /// Stand-in for `Macro Library/`, already line-joined per [`PredicateEnv::macro_text`].
    macros: BTreeMap<String, String>,
}

/// The library-relative paths this double reports in [`PredicateEnvError`] addresses.
/// They match `Session.java`'s own directory constants so a test's expected string looks
/// like the real thing minus the absolute prefix.
const WORD_AUTOMATA_LIB: &str = "Word Automata Library/";
const AUTOMATA_LIB: &str = "Automata Library/";

impl InMemoryPredicateEnv {
    /// An empty environment: no words, no functions, no macros, and no *preloaded*
    /// number systems.
    ///
    /// Note that "no preloaded number systems" does not mean number-system lookup fails —
    /// any plain `msd_k`/`lsd_k` name is still constructed on demand through
    /// [`wr_core::numsys::NumberSystem::new`], exactly as Java's `computeIfAbsent` would.
    /// Preloading via [`Self::with_number_system`] is only needed for names `wr-core`
    /// cannot build by itself (the custom bases, until U5).
    pub fn new() -> Self {
        InMemoryPredicateEnv::default()
    }

    /// Preloads a number system under an (already normalized) name, bypassing
    /// construction.
    ///
    /// Two uses: pinning a specific instance for a test, and — until U5's custom-base
    /// file loading lands — standing in for `msd_fib`-style bases that
    /// [`wr_core::numsys::NumberSystem::new`] cannot construct. Once U5 exists, this
    /// stays useful for the former and becomes optional for the latter.
    pub fn with_number_system(self, name: &str, ns: NumberSystem) -> Self {
        self.number_systems
            .borrow_mut()
            .insert(name.to_string(), Rc::new(ns));
        self
    }

    /// Adds a word/sequence automaton under `name` (no `.txt`, no leading `.`).
    pub fn with_word(mut self, name: &str, automaton: Automaton) -> Self {
        self.words.insert(name.to_string(), automaton);
        self
    }

    /// Adds a user-defined predicate automaton under `name` (no `$`, no `.txt`).
    pub fn with_function(mut self, name: &str, automaton: Automaton) -> Self {
        self.functions.insert(name.to_string(), automaton);
        self
    }

    /// Adds macro text under `name` (no `#`, no `.txt`), stored **verbatim**.
    ///
    /// Use this when the test already has the exact string it wants lexed. If the test is
    /// mirroring a multi-line macro *file*, use [`Self::with_macro_lines`] instead — see
    /// [`PredicateEnv::macro_text`] for why the difference is observable.
    pub fn with_macro(mut self, name: &str, text: &str) -> Self {
        self.macros.insert(name.to_string(), text.to_string());
        self
    }

    /// Adds macro text given as the *lines of a file*, joined the way
    /// `Predicate.readMacroFile` joins them: concatenated with no separator at all
    /// (`Predicate.java:503-505`).
    pub fn with_macro_lines(self, name: &str, lines: &[&str]) -> Self {
        self.with_macro(name, &lines.concat())
    }
}

impl PredicateEnv for InMemoryPredicateEnv {
    fn number_system(&self, name: &str) -> Result<Rc<NumberSystem>, PredicateEnvError> {
        if let Some(ns) = self.number_systems.borrow().get(name) {
            return Ok(Rc::clone(ns));
        }
        // Cache miss. `computeIfAbsent`'s contract on a throwing mapping function is that
        // the map is left unmodified, so the failure path below inserts nothing.
        let ns =
            Rc::new(
                NumberSystem::new(name).map_err(|source| PredicateEnvError::NumberSystem {
                    name: name.to_string(),
                    source,
                })?,
            );
        self.number_systems
            .borrow_mut()
            .insert(name.to_string(), Rc::clone(&ns));
        Ok(ns)
    }

    fn word(&self, name: &str) -> Result<Automaton, PredicateEnvError> {
        match self.words.get(name) {
            Some(a) => Ok(a.clone()),
            None => Err(PredicateEnvError::FileDoesNotExist {
                address: format!("{WORD_AUTOMATA_LIB}{name}.txt"),
            }),
        }
    }

    fn function(&self, name: &str) -> Result<Automaton, PredicateEnvError> {
        match self.functions.get(name) {
            Some(a) => Ok(a.clone()),
            None => Err(PredicateEnvError::FileDoesNotExist {
                address: format!("{AUTOMATA_LIB}{name}.txt"),
            }),
        }
    }

    fn macro_text(&self, name: &str) -> Result<String, PredicateEnvError> {
        match self.macros.get(name) {
            Some(t) => Ok(t.clone()),
            // Java prints the bare name here, not an address -- see the variant's docs.
            None => Err(PredicateEnvError::MacroDoesNotExist {
                name: name.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex_automata::{meta::Regex, Anchored, Input};
    use wr_core::numsys::normalize_number_system_token;

    /// A tiny 1-track automaton, distinguishable from another by its state count. The
    /// contents are irrelevant to every test here — what is under test is the
    /// environment's plumbing, not automaton semantics.
    fn stub_automaton(states: usize) -> Automaton {
        Automaton::true_false(states % 2 == 0)
    }

    // -- the trait object is usable as one -----------------------------------

    /// U3/U4 are meant to take `&dyn PredicateEnv`; if the trait ever stops being
    /// object-safe this fails to compile.
    #[test]
    fn trait_is_object_safe() {
        let env = InMemoryPredicateEnv::new();
        let dynamic: &dyn PredicateEnv = &env;
        assert!(dynamic.number_system("msd_2").is_ok());
    }

    // -- number systems ------------------------------------------------------

    #[test]
    fn number_system_builds_plain_bases_on_demand() {
        let env = InMemoryPredicateEnv::new();
        let ns = env.number_system("msd_3").expect("msd_3 is constructible");
        assert_eq!(ns.name(), "msd_3");
        assert!(ns.is_msd());
    }

    /// `computeIfAbsent` semantics: the same name yields the same instance, not a rebuilt
    /// one. Pointer equality is the observable that matters — it is what makes U5's
    /// per-`NumberSystem` memo tables shared across every token in a formula, which is
    /// the entire point of Ruling 1.
    #[test]
    fn number_system_is_memoized_by_name() {
        let env = InMemoryPredicateEnv::new();
        let a = env.number_system("msd_2").unwrap();
        let b = env.number_system("msd_2").unwrap();
        assert!(Rc::ptr_eq(&a, &b));

        let other = env.number_system("lsd_2").unwrap();
        assert!(!Rc::ptr_eq(&a, &other));
        assert!(!other.is_msd());
    }

    /// A failed resolution must not be negatively cached — Java's `computeIfAbsent`
    /// leaves the map unmodified when the mapping function throws, so a later lookup of
    /// a name that has since become resolvable succeeds. (This matters for real: U5's
    /// custom-base loading resolves against files a user can create between two
    /// commands.)
    #[test]
    fn failed_number_system_lookup_is_not_negatively_cached() {
        let env = InMemoryPredicateEnv::new();
        let err = env.number_system("msd_fib").unwrap_err();
        match &err {
            PredicateEnvError::NumberSystem { name, .. } => assert_eq!(name, "msd_fib"),
            other => panic!("expected a NumberSystem error, got {other:?}"),
        }

        // The direct, load-bearing assertion: nothing was inserted into the cache on
        // failure. Checked before any preload so a hypothetical regression that
        // negatively caches (e.g. inserting a sentinel/`None`) can't hide behind
        // `with_number_system`'s own unconditional `insert`, which would silently
        // overwrite any such sentinel and make the end-to-end assertion below pass
        // regardless.
        assert!(
            !env.number_systems.borrow().contains_key("msd_fib"),
            "a failed lookup must not populate the cache"
        );

        // End-to-end: simulate what U5's file loading (or a `load` command) would make
        // available, and confirm the now-resolvable name succeeds.
        let env = env.with_number_system("msd_fib", NumberSystem::new("msd_2").unwrap());
        assert_eq!(env.number_system("msd_fib").unwrap().name(), "msd_2");
    }

    /// The trait takes *normalized* names; this pins the split so U3's author sees where
    /// normalization belongs (in the lexer, at the `?…` token) rather than guessing.
    #[test]
    fn lookup_key_is_the_normalized_name() {
        let env = InMemoryPredicateEnv::new();
        assert_eq!(normalize_number_system_token(Some("?msd5")), "msd_5");
        let a = env.number_system("msd_5").unwrap();
        let b = env
            .number_system(&normalize_number_system_token(Some("?msd5")))
            .unwrap();
        assert!(Rc::ptr_eq(&a, &b));
    }

    // -- words / functions ---------------------------------------------------

    /// Words and functions are separate libraries in Walnut, so the same bare name may
    /// exist in both and must not collide.
    #[test]
    fn words_and_functions_are_separate_namespaces() {
        let env = InMemoryPredicateEnv::new()
            .with_word("T", stub_automaton(0))
            .with_function("T", stub_automaton(1));

        assert!(env.word("T").unwrap().is_true_automaton());
        assert!(!env.function("T").unwrap().is_true_automaton());

        // ... and a name present in only one library is missing from the other.
        let env = InMemoryPredicateEnv::new().with_word("only_word", stub_automaton(0));
        assert!(env.word("only_word").is_ok());
        assert!(env.function("only_word").is_err());
    }

    /// Two occurrences of one word in a formula must be two independent automata:
    /// `Word.act` calls `bind` on what it was handed, so aliasing them would corrupt
    /// both. Mutating one of the returned values must not be visible in the next lookup.
    #[test]
    fn word_lookup_returns_an_independent_copy() {
        let env = InMemoryPredicateEnv::new().with_word("T", stub_automaton(0));
        let mut first = env.word("T").unwrap();
        first.label = vec!["i".to_string()];

        let second = env.word("T").unwrap();
        assert!(second.label.is_empty(), "the stored automaton was mutated");
    }

    #[test]
    fn missing_word_and_function_report_walnut_message_text() {
        let env = InMemoryPredicateEnv::new();
        assert_eq!(
            env.word("T").unwrap_err().to_string(),
            "File does not exist: Word Automata Library/T.txt"
        );
        assert_eq!(
            env.function("phi").unwrap_err().to_string(),
            "File does not exist: Automata Library/phi.txt"
        );
    }

    // -- macros --------------------------------------------------------------

    #[test]
    fn macro_text_round_trips_verbatim() {
        let env = InMemoryPredicateEnv::new().with_macro("m", "Ex x=%0");
        assert_eq!(env.macro_text("m").unwrap(), "Ex x=%0");
    }

    /// `readMacroFile` drops newlines entirely (`macro.append(line)`), so a two-line
    /// macro file lexes as if its halves were written adjacent — `a` + `b` is the single
    /// token `ab`, not `a b`. The double reproduces that rather than papering over it.
    #[test]
    fn macro_lines_are_joined_with_no_separator() {
        let env = InMemoryPredicateEnv::new().with_macro_lines("m", &["Ex x", "=%0"]);
        assert_eq!(env.macro_text("m").unwrap(), "Ex x=%0");

        let env = InMemoryPredicateEnv::new().with_macro_lines("glued", &["a", "b"]);
        assert_eq!(env.macro_text("glued").unwrap(), "ab");
    }

    #[test]
    fn missing_macro_reports_walnut_message_text() {
        let env = InMemoryPredicateEnv::new();
        assert_eq!(
            env.macro_text("nope").unwrap_err().to_string(),
            "Macro does not exist: nope"
        );
    }

    // -- fresh identifiers ---------------------------------------------------

    /// Java pre-increments, so the first name ends in `_1`.
    #[test]
    fn fresh_identifiers_match_java_numbering() {
        let mut fresh = FreshIdentifiers::new();
        assert_eq!(fresh.issued(), 0);
        assert_eq!(fresh.next_identifier(), "WALNUT_8AthA0PZZI_1");
        assert_eq!(fresh.next_identifier(), "WALNUT_8AthA0PZZI_2");
        assert_eq!(fresh.issued(), 2);
    }

    /// Two contexts are independent — the whole point of threading the counter instead of
    /// keeping Java's `static`.
    #[test]
    fn fresh_identifier_counters_are_independent() {
        let mut a = FreshIdentifiers::new();
        let mut b = FreshIdentifiers::new();
        assert_eq!(a.next_identifier(), b.next_identifier());
        assert_eq!(a.next_identifier(), "WALNUT_8AthA0PZZI_2");
        assert_eq!(b.issued(), 1);
    }

    // -- regex-automata smoke tests (Ruling 2) -------------------------------

    /// `\G` + `Matcher.find(index)` == `Anchored::Yes` + `Input::span(index..)`.
    ///
    /// The load-bearing half is the *negative* case: at an offset where the token does
    /// not start, the match must FAIL rather than scan forward and find the token later
    /// in the string. `regex`'s `find_at` would have scanned; this is why U3 needs
    /// `regex-automata`.
    #[test]
    fn anchored_matching_is_java_g_anchoring() {
        // `PATTERN_FOR_RELATIONAL_OPERATORS` (`Predicate.java:93`), with Java's ASCII
        // `\s` spelled explicitly (see divergence 2 in this module's docs).
        let re = Regex::new(r"(?-u:\s)*(>=|<=|<|>|=|!=)").unwrap();
        let hay = "x  >= y";

        let anchored_at = |off: usize| {
            let mut caps = re.create_captures();
            let input = Input::new(hay).span(off..hay.len()).anchored(Anchored::Yes);
            re.search_captures(&input, &mut caps);
            caps.get_group(1)
                .map(|s| (s.start, s.end, caps.get_match().unwrap().end()))
        };

        // Offset 0 is `x` -- neither whitespace nor a relational operator. Java's
        // `find(0)` on a `\G`-anchored pattern fails here; so must this. A
        // forward-scanning engine would instead report the `>=` at 3..5.
        assert_eq!(anchored_at(0), None);

        // From offset 1 the leading `\s*` absorbs the two spaces and group 1 is the
        // operator at 3..5, with the overall match ending at 5 -- Java's `matcher.end()`,
        // which is what the tokenizer assigns back to `index`.
        assert_eq!(anchored_at(1), Some((3, 5, 5)));
        assert_eq!(anchored_at(3), Some((3, 5, 5)));

        // Alternation order is preserved: `>=` wins over `>` because it is listed first
        // (Rust's regex alternation is leftmost-first for the meta engine here, as
        // Java's is), so the tokenizer does not split `>=` into `>` then `=`.
        assert_eq!(&hay[3..5], ">=");
    }

    /// Java's `[a-zA-Z&&[^AEI]]` character-class intersection (`Predicate.java:72`)
    /// parses unchanged under Rust's regex syntax, so U3 can port the ALPHANUMERIC
    /// fragment literally. If a dependency bump ever drops `&&` support, this fails here
    /// rather than silently in the lexer.
    #[test]
    fn java_character_class_intersection_is_supported() {
        let alphanumeric = Regex::new(r"[a-zA-Z&&[^AEI]](?-u:\w)*").unwrap();
        let anchored = |hay: &str| {
            alphanumeric
                .find(Input::new(hay).anchored(Anchored::Yes))
                .map(|m| m.end())
        };
        assert_eq!(anchored("x1"), Some(2));
        assert_eq!(anchored("Ex"), None, "E is reserved for the quantifier");
        assert_eq!(anchored("Ax"), None, "A is reserved for the quantifier");
        assert_eq!(anchored("Ix"), None, "I is reserved for the quantifier");
        // Only the FIRST character is restricted; A/E/I are fine later on.
        assert_eq!(anchored("xAEI"), Some(4));
    }

    /// Rust's `\s`/`\w`/`\d` are Unicode-aware; Java's are ASCII. Ported patterns must
    /// say `(?-u:…)`. Pinned so the divergence is a documented, tested fact rather than
    /// a footnote U3 might not read.
    #[test]
    fn rust_default_classes_are_wider_than_javas() {
        let nbsp = "\u{00A0}x";
        let unicode_ws = Regex::new(r"\s+").unwrap();
        let ascii_ws = Regex::new(r"(?-u:\s)+").unwrap();
        assert!(unicode_ws
            .find(Input::new(nbsp).anchored(Anchored::Yes))
            .is_some());
        assert!(
            ascii_ws
                .find(Input::new(nbsp).anchored(Anchored::Yes))
                .is_none(),
            "(?-u:\\s) is what matches Java's ASCII-only \\s"
        );
    }

    /// Rust's regex engines reject look-around outright, so
    /// `LOGICAL_OPERATORS`'s `(?<!\.)` (`Predicate.java:80`) cannot be ported literally.
    /// Pinned as a test so the workaround in this module's docs (match without it, then
    /// check the preceding byte) is justified by a fact rather than a memory.
    #[test]
    fn lookbehind_is_unsupported_and_must_be_emulated() {
        assert!(
            Regex::new(r"(?<!\.)(E|A|I)").is_err(),
            "if this ever starts compiling, U3's manual preceding-byte check can go away"
        );

        // The equivalent: match without the look-behind, then reject when the byte
        // before group 1 is '.'. Zero-width look-behind at `start(1)` makes these agree,
        // including at start(1) == 0, where Java's look-behind succeeds (no preceding
        // character) and so must the emulation.
        let re = Regex::new(r"(?-u:\s)*(E|A|I)").unwrap();
        let lex_logical_op = |hay: &str| -> Option<usize> {
            let mut caps = re.create_captures();
            re.search_captures(&Input::new(hay).anchored(Anchored::Yes), &mut caps);
            let g1 = caps.get_group(1)?;
            let preceded_by_dot = g1.start > 0 && hay.as_bytes()[g1.start - 1] == b'.';
            if preceded_by_dot {
                None
            } else {
                Some(g1.start)
            }
        };
        assert_eq!(lex_logical_op("Ex"), Some(0), "no preceding byte => accept");
        assert_eq!(lex_logical_op("  Ex"), Some(2));
        assert_eq!(
            lex_logical_op(".EVEN[i]"),
            None,
            "a dotted word name must not lex its leading E as a quantifier"
        );
    }
}
