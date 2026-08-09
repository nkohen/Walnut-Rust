// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `parse_methods` — static regex-based line-parsers for Walnut's `.txt` automaton
//! format, ported from `Automata/ParseMethods.java` (194 LOC).
//!
//! Lives in `wr-io` (not `wr-core`, despite Java's `ParseMethods` sitting in the
//! `Automata` package) because everything here is pure text-format parsing with no
//! automaton-algorithm content — matching this project's existing crate-boundary
//! docs and the Phase-3a plan (`.claude/plans/synthetic-prancing-aurora.md`, unit
//! U0b). [`crate::reader`] independently implements a small inline
//! trivial-automaton-detection helper (`parseTrueFalse`'s job) since it landed
//! first and couldn't wait for this module — a future refactor can dedupe the two,
//! not done here per this unit's own scope.
//!
//! # No `regex` crate dependency
//!
//! Every pattern here is `\G`-anchored (contiguous re-tokenization from a moving
//! cursor) or otherwise needs precise control over *which* alternative wins and
//! exactly how many bytes it consumes — behavior the `regex` crate's leftmost-longest
//! semantics don't reproduce, and Java's own backtracking alternation-order
//! semantics are exactly what several of these grammars depend on (see
//! [`match_ns_token`]'s doc comment). So, matching [`crate::reader`]'s existing
//! precedent, every pattern is a small hand-written matcher instead — no new
//! workspace dependency needed for this unit (`wr-logic`'s lexer, Phase 3a's U1,
//! separately needs `regex-automata` for its own `\G`-anchored formula lexer; that's
//! a different, larger grammar and not shared with this one).
//!
//! # A deliberate grammar simplification (matching [`crate::reader`]'s precedent)
//!
//! [`parse_transition`]/[`parse_transducer_transition`] use a GREEDY, non-backtracking
//! scan of their `^...$`-anchored line patterns rather than a full backtracking regex
//! simulation. This is observably different from Java only on pathological inputs
//! where greedily consuming an LHS number/`*` element would need to be *undone* for
//! the rest of the line to match (e.g. a literal `-` immediately followed by
//! whitespace then `>`, so `"5 - > 1"` — Java backtracks and ultimately rejects it
//! too, since `-` needs a digit immediately after ANY amount of whitespace to be a
//! valid element and greedy matching never even attempts giving back the `-` it
//! already consumed as a sign). No real Walnut-emitted `.txt` file or hand-written
//! fixture in the golden corpus exercises this edge; flagged here rather than
//! silently assumed away, consistent with `reader.rs`'s "deliberate grammar
//! simplification" precedent for the same reason.
//!
//! # The `determineBase`/`Prover` coupling this unit's brief warned about
//!
//! The brief for this unit (`.claude/plans/synthetic-prancing-aurora.md`, U0b) flagged
//! a `ParseMethods.java` -> `Main.Prover.determineBase(m)` call as a real coupling to
//! resolve. Re-reading the actual current `ParseMethods.java` source found no such
//! call anywhere in the file (verified: zero occurrences of `Prover` in
//! `ParseMethods.java`). `determineBase` is instead a `private static` method
//! *inside* `Automata/NumberSystem.java` (`:265-267`), already ported in Phase 2's U7
//! as `wr_core::numsys::determine_base` (and `normalize_number_system_token`,
//! `NumberSystem::new`, `NumberSystem::get_alphabet`, all already available). So
//! [`parse_alphabet_declaration`] (the one function here that resolves numeration
//! names, mirroring `ParseMethods.parseAlphabetDeclaration`'s call to
//! `NumberSystem.getComputeIfAbsent`) calls straight into `wr_core::numsys` with no
//! forward reference to unported `wr-cli`/`Prover` code at all — the coupling this
//! unit was meant to resolve turned out not to exist in the current source. Flagged
//! here rather than silently ignored, since the brief was explicit about it.
//!
//! One real, smaller divergence remains, worth naming: Java's
//! `NumberSystem.getComputeIfAbsent` caches instances in a static,
//! process-lifetime map keyed by name, so two identical numeration tokens in one
//! declaration (or across declarations) share the SAME `NumberSystem` object. The
//! caching/session layer that would host an equivalent cache doesn't exist yet in
//! this port (deferred to `wr-cli`'s `Session`, Phase 3a's U14) —
//! [`parse_alphabet_declaration`] always constructs a fresh [`NumberSystem`]. The
//! VALUES produced (alphabet contents) are identical either way; only object
//! identity — unused anywhere in this port today — would differ.

use std::collections::BTreeMap;

use wr_core::numsys::{normalize_number_system_token, NumSysError, NumberSystem};
use wr_core::util::{is_java_whitespace, parse_int};

/// Recoverable parse failures. Mirrors this project's established idiom (module-local
/// error enums for recoverable cases; see e.g. `wr_core::numsys::NumSysError`) rather
/// than a shared `WalnutError` type.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseMethodsError {
    /// `WalnutException("Morphism has no valid mappings.")` (`ParseMethods.java:190`,
    /// `parseMorphism`).
    NoValidMorphismMappings,
    /// **Genuine Walnut (Java) bug, logged as `docs/WALNUT-BUGS.md` WB-011, ported
    /// verbatim.** `parseMorphism` calls plain `Integer.parseInt` on a morphism
    /// symbol's bracketed text (`ParseMethods.java:172-174,182-184`) — NOT
    /// `UtilityMethods.parseInt`, which strips whitespace — even though the pattern
    /// backing it (`\[(?:[+\-])?\s*\d+\]`) explicitly permits whitespace between the
    /// sign and the digits. `"[+ 5]"` therefore matches the regex but then throws an
    /// uncaught `NumberFormatException` in real Walnut. Also reachable from plain
    /// `i32` overflow (same underlying `Integer.parseInt` call, an ordinary and
    /// already-precedented divergence elsewhere in this port, e.g.
    /// `NumSysError::BaseNotAnI32`). The `String` is the exact text Java would have
    /// handed to `Integer.parseInt`.
    IntegerParseFailure(String),
}

impl std::fmt::Display for ParseMethodsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseMethodsError::NoValidMorphismMappings => {
                write!(f, "Morphism has no valid mappings.")
            }
            ParseMethodsError::IntegerParseFailure(text) => {
                write!(f, "For input string: \"{text}\"")
            }
        }
    }
}

impl std::error::Error for ParseMethodsError {}

// ---------------------------------------------------------------------------
// Low-level shared matchers (no direct Java counterpart -- internal plumbing
// shared by several of the public functions below, the way Java's Matcher/Pattern
// machinery is shared implicitly by the regex engine).
// ---------------------------------------------------------------------------

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && is_java_whitespace(bytes[i]) {
        i += 1;
    }
    i
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `(\+|\-)?\s*\d+`, applied at byte offset `pos`: an optional sign, then any amount
/// of whitespace (whether or not a sign was present), then one or more digits.
/// Returns the captured `(start, end)` byte SPAN and the end offset — deliberately
/// NOT the parsed value. Java's regex engine matches `\d+` purely syntactically (it
/// doesn't care how many digits there are); `Integer.parseInt`/`UtilityMethods.parseInt`
/// is only ever called by real Walnut AFTER the ENCLOSING pattern (state declaration,
/// transition line, ...) has fully matched — a candidate digit run that's part of an
/// ultimately-failing larger match never reaches `parseInt` at all in Java. So every
/// caller here must do the same: collect spans while probing, and defer
/// [`parse_int`]/[`parse_plain_i32`] to a final step that runs only once the caller's
/// own full pattern is confirmed. (Getting this backwards is exactly
/// `docs/WALNUT-BUGS.md`-adjacent territory in spirit, though it's a port bug, not a
/// Java one: an i32-overflowing digit run inside a probe that Java's own backtracking
/// would have abandoned used to abort the whole parse here instead of being quietly
/// dropped like Java drops it.)
fn match_signed_int_span(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    let mut i = pos;
    if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
        i += 1;
    }
    i = skip_ws(bytes, i);
    let digit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digit_start {
        None
    } else {
        Some((pos, i))
    }
}

/// [`parse_int`] applied to an already-CONFIRMED [`match_signed_int_span`] capture.
fn parse_int_span(s: &str, span: (usize, usize)) -> i32 {
    parse_int(&s[span.0..span.1])
}

/// [`match_signed_int_span`] plus immediate parsing — safe ONLY where a successful
/// sub-match is never subject to further right-context that could still fail (i.e.
/// nothing downstream can invalidate this match once found). [`parse_list`] is the
/// one caller for which that's true: each `PATTERN_ELEMENT` match Java finds via
/// `m.find(index)` is self-contained (the pattern has no trailing requirement beyond
/// the element itself), so Java itself calls `parseInt` immediately on each one, with
/// no backtracking risk — mirrored here the same way. Every OTHER caller in this file
/// has real trailing context (an arrow, a mandatory whitespace run, a closing brace,
/// end-of-string) that a plain digit match doesn't yet guarantee, and must use
/// [`match_signed_int_span`] instead.
fn match_signed_int(bytes: &[u8], pos: usize) -> Option<(i32, usize)> {
    let (start, end) = match_signed_int_span(bytes, pos)?;
    let text = std::str::from_utf8(&bytes[start..end]).expect("regex-shaped input is ASCII");
    Some((parse_int(text), end))
}

// ---------------------------------------------------------------------------
// parseTrueFalse
// ---------------------------------------------------------------------------

/// `ParseMethods.parseTrueFalse(String, Boolean[])` (`:74-81`) — `^\s*(true|false)\s*$`.
/// Java's out-parameter becomes a return value: `Some(true/false)` on a match,
/// `None` on no match (Java's `false` return, singleton left untouched).
pub fn parse_true_false(s: &str) -> Option<bool> {
    let bytes = s.as_bytes();
    let start = skip_ws(bytes, 0);
    let mut end = bytes.len();
    while end > start && is_java_whitespace(bytes[end - 1]) {
        end -= 1;
    }
    match &s[start..end] {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// parseAlphabetDeclaration
// ---------------------------------------------------------------------------

/// The result of [`parse_alphabet_declaration`], mirroring
/// `ParseMethods.parseAlphabetDeclaration`'s three out-facing values (Java: a `bool`
/// return, plus two `List` out-parameters).
#[derive(Debug)]
pub struct AlphabetDeclaration {
    /// Java's return value: whether the ENTIRE input string was consumed by
    /// contiguous token matches (`index >= s.length()`).
    pub fully_consumed: bool,
    /// One entry per declared track: either an explicit `{v1, v2, ...}` set, or a
    /// numeration's alphabet (`NumberSystem::get_alphabet`).
    pub alphabet: Vec<Vec<i32>>,
    /// `None` for an explicit-set track, `Some(ns)` for a numeration track — mirrors
    /// Java's `bases` list (`null` entries included).
    pub number_systems: Vec<Option<NumberSystem>>,
}

enum AlphabetToken {
    Set(Vec<i32>),
    Ns(String),
}

/// `(\d+|\w+)`: a greedy ASCII-digit run, falling back to a greedy word-char run
/// (`[a-zA-Z0-9_]`, Java's default non-`UNICODE_CHARACTER_CLASS` `\w`). Mirrors
/// Java's ordered-alternative backtracking directly: `\d+` is tried first and, since
/// digits are also word characters, a leading digit always takes that branch;
/// `\w+` only gets a chance when the very first character isn't a digit at all.
fn match_digit_or_word_run(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 {
        return Some(i);
    }
    while i < bytes.len() && is_word_byte(bytes[i]) {
        i += 1;
    }
    if i > 0 {
        Some(i)
    } else {
        None
    }
}

/// Matches group 2 (`ALPHABET_NUMBER_SYSTEM`) of `PATTERN_NEXT_ALPHABET_TOKEN`:
/// `((msd|lsd)_(\d+|\w+))|((msd|lsd)(\d+|\w+))|(msd|lsd)|(\d+|\w+)`, tried in that
/// alternation order (Java regex alternation is leftmost-first with backtracking,
/// not longest-match — order matters here, e.g. for `"msd_"` with nothing usable
/// after the `_`, which falls all the way through to matching bare `"msd"`). Only
/// the OVERALL matched text is used downstream (`normalizeNumberSystemToken`
/// consumes the whole string and re-derives msd/lsd/base itself), so the individual
/// inner Java subgroups have no Rust counterpart here.
fn match_ns_token(s: &str) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    let has_prefix = bytes.starts_with(b"msd") || bytes.starts_with(b"lsd");
    if has_prefix {
        let prefix_len = 3;
        // Alt 1: prefix + literal '_' + (\d+|\w+). Note \w includes '_' itself, so
        // e.g. "msd_" with nothing else following can still fall through to Alt 2
        // matching \w+ = "_" if Alt 1's OWN \d+|\w+ (which starts AFTER the literal
        // '_') finds nothing -- handled naturally here since Alt 1 only succeeds
        // when match_digit_or_word_run finds >=1 char past the '_'.
        if bytes.get(prefix_len) == Some(&b'_') {
            if let Some(len) = match_digit_or_word_run(&bytes[prefix_len + 1..]) {
                let total = prefix_len + 1 + len;
                return Some((&s[..total], total));
            }
        }
        // Alt 2: prefix + (\d+|\w+) directly (no literal '_' required here).
        if let Some(len) = match_digit_or_word_run(&bytes[prefix_len..]) {
            let total = prefix_len + len;
            return Some((&s[..total], total));
        }
        // Alt 3: bare prefix, zero-length continuation.
        return Some((&s[..prefix_len], prefix_len));
    }
    // Alt 4: bare (\d+|\w+), reached only when the input doesn't start with a
    // literal "msd"/"lsd" at all (Alt 3 above always succeeds whenever it does).
    let len = match_digit_or_word_run(bytes)?;
    Some((&s[..len], len))
}

/// Matches group 12 (`ALPHABET_SET`)'s grammar,
/// `(\+|\-)?\s*\d+\s*(\s*,\s*(\+|\-)?\s*\d+)*`, applied at the start of `s`. Returns
/// the end offset of the captured list text — NOT including any trailing whitespace
/// or the closing `}` (those are the caller's job, matching Java's
/// `\{\s*(GROUP12)\s*\}` structure). A trailing comma with nothing valid after it
/// (e.g. `"0,1,"`) is a real parse failure at the OUTER level: the repeated group
/// simply stops (backtracks to zero extra repetitions) without consuming the
/// dangling comma, so the caller's subsequent `\s*\}` requirement fails on the
/// leftover `",...}"`.
fn match_alphabet_set_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    // Span-only: this function's job is purely to find where the list-shaped text
    // ENDS (mirroring the Java regex engine's own purely-syntactic `\d+` matching,
    // which doesn't evaluate anything). The actual values only get parsed once the
    // caller ([`match_next_alphabet_token`]) confirms the surrounding `{...}` fully
    // matched, via a separate re-parse with [`parse_list`] -- exactly mirroring
    // Java's own two-stage capture-then-`parseList` design for this same group. Using
    // the value-returning [`match_signed_int`] here used to panic on an
    // overflow-sized element even when the set was never going to close (e.g. no
    // closing `}` at all), which Java would never even attempt to `parseInt`.
    let (_, mut pos) = match_signed_int_span(bytes, 0)?;
    loop {
        let mut j = skip_ws(bytes, pos);
        if bytes.get(j) != Some(&b',') {
            break;
        }
        j += 1;
        j = skip_ws(bytes, j);
        match match_signed_int_span(bytes, j) {
            Some((_, end)) => pos = end,
            None => break,
        }
    }
    Some(pos)
}

/// `PATTERN_NEXT_ALPHABET_TOKEN`, applied at the START of `s` — mirrors `\G`
/// anchoring (Java re-anchors `find(index)` at the current cursor on each loop
/// iteration in `parseAlphabetDeclaration`, so a non-match here means the WHOLE
/// alphabet-declaration parse stops there, not just this one token). Returns the
/// token and the total number of bytes consumed, including surrounding whitespace.
fn match_next_alphabet_token(s: &str) -> Option<(AlphabetToken, usize)> {
    let bytes = s.as_bytes();
    let i = skip_ws(bytes, 0);
    if i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'{' {
        // Java's pattern is `\{\s*(...)\s*\}` (`ParseMethods.java:46`) -- there IS a
        // `\s*` right after the opening brace. Skipping it only "worked" by accident
        // when the first set element has no sign, because `match_signed_int`'s own
        // internal whitespace-skip absorbs it; a signed first element (`{ -1, 0, 1}`)
        // was silently rejected without this.
        let inner_start = skip_ws(bytes, i + 1);
        let list_end = match_alphabet_set_end(&s[inner_start..])?;
        let list_text = &s[inner_start..inner_start + list_end];
        let mut j = inner_start + list_end;
        j = skip_ws(bytes, j);
        if bytes.get(j) != Some(&b'}') {
            return None;
        }
        j += 1;
        j = skip_ws(bytes, j);
        // Mirrors Java's two-stage capture-then-reparse: group 12 captures this raw
        // text, then `parseList` re-tokenizes it with the more permissive
        // PATTERN_ELEMENT grammar (identical result here, since group 12's grammar
        // is a strict subset of PATTERN_ELEMENT's — no `*`, no leading comma).
        let values: Vec<i32> = parse_list(list_text)
            .into_iter()
            .map(|v| v.expect("group 12's grammar never produces a '*' element"))
            .collect();
        return Some((AlphabetToken::Set(values), j));
    }
    let (ns_text, ns_len) = match_ns_token(&s[i..])?;
    let ns_text = ns_text.to_string();
    let mut j = i + ns_len;
    j = skip_ws(bytes, j);
    Some((AlphabetToken::Ns(ns_text), j))
}

/// `ParseMethods.parseAlphabetDeclaration(String, List<List<Integer>>, List<NumberSystem>)`
/// (`:84-109`). See the module docs for how the `Main.Prover.determineBase` coupling
/// flagged in this unit's brief was resolved (it doesn't exist in the current
/// source) and the one real remaining divergence (no cross-call NumberSystem cache).
///
/// Propagates [`NumSysError`] exactly where Java's `NumberSystem.getComputeIfAbsent`
/// (via the `NumberSystem(String)` constructor) would throw an uncaught
/// `WalnutException` — e.g. a non-numeric custom-base name like `msd_fib`, since
/// file-backed custom bases are out of scope in this port (`wr_core::numsys`'s
/// module docs).
pub fn parse_alphabet_declaration(s: &str) -> Result<AlphabetDeclaration, NumSysError> {
    let mut alphabet: Vec<Vec<i32>> = Vec::new();
    let mut number_systems: Vec<Option<NumberSystem>> = Vec::new();
    let mut index = 0usize;
    while let Some((token, consumed)) = match_next_alphabet_token(&s[index..]) {
        match token {
            AlphabetToken::Set(values) => {
                alphabet.push(values);
                number_systems.push(None);
            }
            AlphabetToken::Ns(ns_text) => {
                let base = normalize_number_system_token(Some(&ns_text));
                let ns = NumberSystem::new(&base)?;
                alphabet.push(ns.get_alphabet().to_vec());
                number_systems.push(Some(ns));
            }
        }
        index += consumed;
        if consumed == 0 {
            // Defensive: every real alternative of PATTERN_NEXT_ALPHABET_TOKEN
            // consumes >=1 byte (even bare "msd"/"lsd" is 3 bytes), so this never
            // fires on real input -- guards against an infinite loop if that
            // invariant is ever violated by a future edit.
            break;
        }
    }
    Ok(AlphabetDeclaration {
        fully_consumed: index >= s.len(),
        alphabet,
        number_systems,
    })
}

// ---------------------------------------------------------------------------
// State / transition declarations
// ---------------------------------------------------------------------------

/// `ParseMethods.parseStateDeclaration(String, int[])` (`:111-120`) —
/// `^\s*(\d+)\s+((\+|\-)?\s*\d+)\s*$`. Note the MANDATORY `\s+` between the state
/// name and its output (at least one whitespace char) — unlike most other patterns
/// here, which tolerate zero. Returns `Some((state_name, output))`.
///
/// Integer parsing is deferred until the WHOLE pattern (mandatory `\s+`, the output
/// number, trailing `\s*$`) is confirmed to match — mirroring Java, where
/// `UtilityMethods.parseInt` is only ever called on `m.group(k)` after `m.find()`
/// already succeeded for the entire line. An i32-overflow-sized digit run with no
/// following whitespace at all (e.g. `"9999999999"`, one token, no `\s+`) is not a
/// state declaration in Java — `m.find()` fails outright and `parseStateDeclaration`
/// returns `false` — so it must return `None` here too, not panic mid-probe.
pub fn parse_state_declaration(s: &str) -> Option<(i32, i32)> {
    let bytes = s.as_bytes();
    let mut i = skip_ws(bytes, 0);
    let name_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name_span = (name_start, i);
    let ws_start = i;
    i = skip_ws(bytes, i);
    if i == ws_start {
        return None; // \s+ needs at least one
    }
    let output_span = match_signed_int_span(bytes, i)?;
    i = skip_ws(bytes, output_span.1);
    if i != bytes.len() {
        return None;
    }
    // Fully matched -- now, and only now, safe to parse (matching where Java's own
    // parseInt calls happen).
    Some((parse_int_span(s, name_span), parse_int_span(s, output_span)))
}

/// `ParseMethods.parseTransducerStateDeclaration(String, int[])` (`:122-130`) —
/// `^\s*(\d+)\s*$`. Same deferred-parsing discipline as [`parse_state_declaration`]:
/// an overflow-sized digit run followed by non-whitespace trailing garbage isn't a
/// match in Java at all (`\s*$` fails), so it must not panic here either.
pub fn parse_transducer_state_declaration(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    let mut i = skip_ws(bytes, 0);
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return None;
    }
    let val_span = (start, i);
    i = skip_ws(bytes, i);
    if i != bytes.len() {
        return None;
    }
    Some(parse_int_span(s, val_span))
}

/// One LHS input-symbol element, as a SPAN (deferred parse) rather than a value —
/// see [`match_signed_int_span`]'s doc for why: neither [`match_transition_lhs`]'s
/// nor [`match_dest_list`]'s callers know yet whether the surrounding line will end
/// up matching (a mandatory `->`, a mandatory `/`+output, or end-of-string may still
/// fail), so no element's digits may be parsed until the caller confirms the whole
/// pattern.
enum InputElemSpan {
    Num(usize, usize),
    Wildcard,
}

/// Matches one repetition of `(((\+|\-)?\s*\d+\s*)|(\s*\*\s*))` at byte offset `i`,
/// used by both [`parse_transition`] and [`parse_transducer_transition`]'s LHS.
/// Returns `(element, new_offset)`.
fn match_transition_input_element_span(bytes: &[u8], i: usize) -> Option<(InputElemSpan, usize)> {
    if let Some((start, end)) = match_signed_int_span(bytes, i) {
        let end = skip_ws(bytes, end);
        return Some((InputElemSpan::Num(start, end), end));
    }
    let j = skip_ws(bytes, i);
    if bytes.get(j) == Some(&b'*') {
        let k = skip_ws(bytes, j + 1);
        return Some((InputElemSpan::Wildcard, k));
    }
    None
}

/// Greedily matches one-or-more [`match_transition_input_element_span`]s, then
/// requires literal `->` (with `\s*` on each side — already partly absorbed by each
/// element's own trailing whitespace). Returns `(input_spans, offset_after_arrow)`.
fn match_transition_lhs(s: &str, start: usize) -> Option<(Vec<InputElemSpan>, usize)> {
    let bytes = s.as_bytes();
    let mut i = start;
    let mut input = Vec::new();
    while let Some((v, end)) = match_transition_input_element_span(bytes, i) {
        input.push(v);
        i = end;
    }
    if input.is_empty() {
        return None; // the "+" repetition requires >= 1
    }
    i = skip_ws(bytes, i);
    if !s[i..].starts_with("->") {
        return None;
    }
    i += 2;
    i = skip_ws(bytes, i);
    Some((input, i))
}

/// Parses a confirmed [`InputElemSpan`] list into the public `Vec<Option<i32>>`
/// shape — only called after the caller's full pattern has matched.
fn parse_input_spans(s: &str, spans: Vec<InputElemSpan>) -> Vec<Option<i32>> {
    spans
        .into_iter()
        .map(|e| match e {
            InputElemSpan::Num(start, end) => Some(parse_int_span(s, (start, end))),
            InputElemSpan::Wildcard => None,
        })
        .collect()
}

/// Greedily matches `(\d+\s*)+` at byte offset `i` (the destination-state-id list
/// shared by [`parse_transition`]/[`parse_transducer_transition`]'s RHS, before the
/// literal `->`'s target). Returns `(dest_spans, new_offset)`; `None` if no digit run
/// is found at all (the `+` repetition requires >= 1). Spans, not values, for the
/// same deferred-parsing reason as [`match_transition_input_element_span`] — a huge
/// destination id followed by trailing garbage (for `parseTransition`) or a missing
/// `/` (for `parseTransducerTransition`) is not a match in Java at all.
fn match_dest_list(s: &str, start: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    let bytes = s.as_bytes();
    let mut i = start;
    let mut dest = Vec::new();
    loop {
        let d_start = i;
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == d_start {
            break;
        }
        dest.push((d_start, j));
        i = skip_ws(bytes, j);
    }
    if dest.is_empty() {
        None
    } else {
        Some((dest, i))
    }
}

fn parse_dest_spans(s: &str, spans: Vec<(usize, usize)>) -> Vec<i32> {
    spans.into_iter().map(|sp| parse_int_span(s, sp)).collect()
}

/// `ParseMethods.parseTransition(String, List<Integer>, List<Integer>)` (`:132-140`)
/// — `^\s*((((\+|\-)?\s*\d+\s*)|(\s*\*\s*))+)\s*\->\s*((\d+\s*)+)\s*$`. Returns
/// `Some((input_symbols, dest_state_ids))`; `None` entries in `input_symbols` are `*`
/// wildcards.
///
/// Integer parsing is deferred until the whole line (LHS, arrow, dest list, trailing
/// `\s*$`) is confirmed to match — mirroring Java's `parseList(m.group(k), ...)`,
/// which only ever runs after `m.find()` already succeeded for the entire line. An
/// overflow-sized digit run that's part of a line failing to match for some OTHER
/// reason (missing `->`, trailing garbage, …) must return `None` here, not panic.
pub fn parse_transition(s: &str) -> Option<(Vec<Option<i32>>, Vec<i32>)> {
    let bytes = s.as_bytes();
    let start = skip_ws(bytes, 0);
    let (input_spans, i) = match_transition_lhs(s, start)?;
    let (dest_spans, i) = match_dest_list(s, i)?;
    if i != bytes.len() {
        return None;
    }
    // Fully matched -- now, and only now, safe to parse.
    Some((
        parse_input_spans(s, input_spans),
        parse_dest_spans(s, dest_spans),
    ))
}

/// One transducer transition line's parsed fields, mirroring
/// `ParseMethods.parseTransducerTransition`'s three out-parameters
/// (`List<Integer> input, dest, output`).
pub struct TransducerTransition {
    /// `None` entries are `*` wildcards.
    pub input: Vec<Option<i32>>,
    pub dest: Vec<i32>,
    /// Always exactly one element — Java's `parseList` is called on this group too,
    /// which for this pattern's shape (a single signed number) always produces
    /// exactly one entry.
    pub output: Vec<i32>,
}

/// `ParseMethods.parseTransducerTransition(String, List<Integer>, List<Integer>,
/// List<Integer>)` (`:142-153`) —
/// `^\s*((((\+|\-)?\s*\d+\s*)|(\s*\*\s*))+)\s*\->\s*((\d+\s*)+)\s*\/\s*((\+|\-)?\s*\d+)\s*$`.
/// Same deferred-parsing discipline as [`parse_transition`].
pub fn parse_transducer_transition(s: &str) -> Option<TransducerTransition> {
    let bytes = s.as_bytes();
    let start = skip_ws(bytes, 0);
    let (input_spans, i) = match_transition_lhs(s, start)?;
    let (dest_spans, i) = match_dest_list(s, i)?;
    if bytes.get(i) != Some(&b'/') {
        return None;
    }
    let i = skip_ws(bytes, i + 1);
    let output_span = match_signed_int_span(bytes, i)?;
    let i = skip_ws(bytes, output_span.1);
    if i != bytes.len() {
        return None;
    }
    // Fully matched -- now, and only now, safe to parse.
    Some(TransducerTransition {
        input: parse_input_spans(s, input_spans),
        dest: parse_dest_spans(s, dest_spans),
        output: vec![parse_int_span(s, output_span)],
    })
}

/// `ParseMethods.parseList(String, List<Integer>)` (`:155-164`) — repeatedly matches
/// `PATTERN_ELEMENT` (`\G\s*,?\s*(((\+|\-)?\s*\d+)|\*)`) from the current position,
/// appending each matched value (`None` for `*`) until no more match. Unlike
/// [`parse_alphabet_declaration`], Java's `parseList` has no "fully consumed" check
/// at all — any trailing non-matching remainder is silently ignored.
pub fn parse_list(s: &str) -> Vec<Option<i32>> {
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut index = 0usize;
    loop {
        let mut i = skip_ws(bytes, index);
        if bytes.get(i) == Some(&b',') {
            i += 1;
        }
        i = skip_ws(bytes, i);
        if bytes.get(i) == Some(&b'*') {
            result.push(None);
            index = i + 1;
            continue;
        }
        match match_signed_int(bytes, i) {
            Some((v, end)) => {
                result.push(Some(v));
                index = end;
            }
            None => break,
        }
    }
    result
}

// ---------------------------------------------------------------------------
// parseMorphism
// ---------------------------------------------------------------------------

/// `text.parse::<i32>()` with no whitespace stripping — the Rust stand-in for
/// Java's plain `Integer.parseInt(String)` as called directly (not via
/// `UtilityMethods.parseInt`) at `ParseMethods.java:187` / `:185`. See
/// [`ParseMethodsError::IntegerParseFailure`] and `docs/WALNUT-BUGS.md` WB-011.
fn parse_plain_i32(text: &str) -> Result<i32, ParseMethodsError> {
    text.parse::<i32>()
        .map_err(|_| ParseMethodsError::IntegerParseFailure(text.to_string()))
}

/// Matches `\[(?:[+\-])?\s*\d+\]` syntactically — the bracket form shared by
/// `MORPHISM_INPUT_SYMBOL` and `MORPHISM_IMAGE_SYMBOL` (`MORPHISM_COMMON_SYMBOL`).
/// Returns the raw inner text's byte range (sign + whitespace + digits, NOT
/// stripped — see [`parse_plain_i32`]) and the offset just past the closing `]`.
fn match_bracket_syntax(bytes: &[u8], pos: usize) -> Option<(usize, usize, usize)> {
    if bytes.get(pos) != Some(&b'[') {
        return None;
    }
    let mut i = pos + 1;
    let inner_start = i;
    if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
        i += 1;
    }
    i = skip_ws(bytes, i);
    let digit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digit_start {
        return None;
    }
    let inner_end = i;
    if bytes.get(i) != Some(&b']') {
        return None;
    }
    Some((inner_start, inner_end, i + 1))
}

/// `MORPHISM_INPUT_SYMBOL` = `\[(?:[+\-])?\s*\d+\]|\d+`. The unbracketed form is
/// `\d+` only — no sign — unlike the unbracketed IMAGE symbol, which is a single
/// bare digit ([`match_morphism_image_symbol_span`]).
///
/// Returns the captured text SPAN (bracket-inner text for the bracketed form, the
/// bare digit run otherwise) and the offset just past the whole symbol —
/// deliberately NOT the parsed value. See [`match_signed_int_span`]'s doc for why:
/// [`try_match_morphism_mapping`] doesn't yet know whether the mandatory `->` (and
/// beyond) will actually follow, and Java's own `Integer.parseInt` is only called
/// once `m1.find()` has confirmed the WHOLE `INPUT -> IMAGE*` mapping matches, not
/// merely this one symbol. Getting this backwards used to make
/// `parse_morphism("x9999999999 0 -> 1")` abort entirely on the doomed
/// `"9999999999"` probe (no `->` follows it) instead of succeeding with `{0: [1]}`
/// like Java does, once the scan reaches the real `"0 -> 1"` mapping.
fn match_morphism_input_symbol_span(bytes: &[u8], pos: usize) -> Option<((usize, usize), usize)> {
    if let Some((inner_start, inner_end, total_end)) = match_bracket_syntax(bytes, pos) {
        return Some(((inner_start, inner_end), total_end));
    }
    let start = pos;
    let mut i = pos;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        None
    } else {
        Some(((start, i), i))
    }
}

/// `MORPHISM_IMAGE_SYMBOL` = `\[(?:[+\-])?\s*\d+\]|\d` — the unbracketed form is
/// exactly ONE digit (not `\d+`): each unbracketed character in a morphism's image
/// is its own separate symbol (`parseMorphism("0 -> 0010")` yields four image
/// symbols `[0,0,1,0]`, not one four-digit number). Span-only, deferred parse, same
/// reasoning as [`match_morphism_input_symbol_span`] (the unbracketed single-digit
/// form can never overflow, but the bracketed form can, and there's no reason to
/// treat the two forms differently here).
fn match_morphism_image_symbol_span(bytes: &[u8], pos: usize) -> Option<((usize, usize), usize)> {
    if let Some((inner_start, inner_end, total_end)) = match_bracket_syntax(bytes, pos) {
        return Some(((inner_start, inner_end), total_end));
    }
    if let Some(&b) = bytes.get(pos) {
        if b.is_ascii_digit() {
            return Some(((pos, pos + 1), pos + 1));
        }
    }
    None
}

/// Tries to match one `INPUT_SYMBOL \s* -> \s* (IMAGE_SYMBOL)*` mapping starting
/// EXACTLY at `pos` (not a scan — the caller does the scanning, mirroring
/// `Matcher.find()`'s own leftmost-search behavior). `Ok(None)` = no syntactic
/// match at this exact position (keep scanning); `Err` = the syntax matched but a
/// symbol's `Integer.parseInt` failed (propagates immediately, matching Java's
/// uncaught exception aborting the whole `parseMorphism` call).
///
/// Symbols are matched as SPANS first and parsed only once the whole mapping (input
/// symbol, arrow, zero-or-more image symbols) is confirmed — see
/// [`match_morphism_input_symbol_span`]'s doc. Note this doesn't need to replicate
/// Java's regex-engine digit-run BACKTRACKING to get the same overall result: the
/// caller ([`parse_morphism`]) already retries at every subsequent byte position on a
/// failed attempt here, which has the same net effect as backtracking a greedy `\d+`
/// down to shorter runs one position at a time.
fn try_match_morphism_mapping(
    s: &str,
    pos: usize,
) -> Result<Option<(i32, Vec<i32>, usize)>, ParseMethodsError> {
    let bytes = s.as_bytes();
    let Some((key_span, mut i)) = match_morphism_input_symbol_span(bytes, pos) else {
        return Ok(None);
    };
    i = skip_ws(bytes, i);
    if !s[i..].starts_with("->") {
        return Ok(None);
    }
    i += 2;
    i = skip_ws(bytes, i);
    let mut image_spans = Vec::new();
    while let Some((sp, end)) = match_morphism_image_symbol_span(bytes, i) {
        image_spans.push(sp);
        i = end;
    }
    // Fully matched -- now, and only now, safe to parse (mirrors Java, where
    // Integer.parseInt is only called on m1.group(1) / each m2 piece AFTER m1.find()
    // has already confirmed the whole INPUT -> IMAGE* mapping).
    let key = parse_plain_i32(&s[key_span.0..key_span.1])?;
    let mut image = Vec::with_capacity(image_spans.len());
    for sp in image_spans {
        image.push(parse_plain_i32(&s[sp.0..sp.1])?);
    }
    Ok(Some((key, image, i)))
}

/// `ParseMethods.parseMorphism(String)` (`:166-193`). Scans `mapString` for
/// non-overlapping `INPUT -> IMAGE*` mappings ANYWHERE in the string (Java's
/// `Matcher.find()` loop, not `\G`-anchored — unmatched characters between mappings
/// are silently skipped), collecting them into a sorted map (Java's `TreeMap`, here
/// `BTreeMap`). Errors with [`ParseMethodsError::NoValidMorphismMappings`] if no
/// mapping was found at all.
pub fn parse_morphism(map_string: &str) -> Result<BTreeMap<i32, Vec<i32>>, ParseMethodsError> {
    let mut mapping: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    let bytes = map_string.as_bytes();
    let mut pos = 0usize;
    while pos <= bytes.len() {
        match try_match_morphism_mapping(map_string, pos)? {
            Some((key, image, end)) => {
                mapping.insert(key, image);
                pos = end;
            }
            None => {
                if pos >= bytes.len() {
                    break;
                }
                pos += 1;
            }
        }
    }
    if mapping.is_empty() {
        return Err(ParseMethodsError::NoValidMorphismMappings);
    }
    Ok(mapping)
}

// ---------------------------------------------------------------------------
// PATTERN_WHITESPACE / PATTERN_COMMENT
// ---------------------------------------------------------------------------

/// `ParseMethods.PATTERN_WHITESPACE` (`:70`) — `^\s*$`.
pub fn is_whitespace_line(s: &str) -> bool {
    s.bytes().all(is_java_whitespace)
}

/// `ParseMethods.PATTERN_COMMENT` (`:71`) — `^\s*#.*$`, matched via `Matcher.matches()`
/// (the WHOLE string must match, not just a prefix). Java's `.` does NOT match line
/// terminators (no `DOTALL` flag), so — verified against the real compiled
/// `Pattern`/`Matcher.matches()` — any `\n`/`\r` anywhere at or after the `#` makes the
/// whole thing fail to match (`"#x\ny"` and even a bare trailing `"#x\n"` are both
/// `false`; empirically `matches()` does NOT extend `$`'s usual "before a final line
/// terminator" leniency here — a trailing terminator is simply never consumed by
/// `.*`, so it's never `matches()`-clean). Leading whitespace (via `\s*`) DOES still
/// tolerate embedded newlines before the `#`, since `\s` includes `\n`/`\r`.
pub fn is_comment_line(s: &str) -> bool {
    let bytes = s.as_bytes();
    let i = skip_ws(bytes, 0);
    if bytes.get(i) != Some(&b'#') {
        return false;
    }
    !bytes[i + 1..].iter().any(|&b| b == b'\n' || b == b'\r')
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- parseTrueFalse ------------------------------------------------------

    #[test]
    fn parse_true_false_examples() {
        assert_eq!(parse_true_false("true"), Some(true));
        assert_eq!(parse_true_false("  false  "), Some(false));
        assert_eq!(parse_true_false("\ttrue\n"), Some(true));
        assert_eq!(parse_true_false("truee"), None);
        assert_eq!(parse_true_false("True"), None); // case-sensitive
        assert_eq!(parse_true_false(""), None);
        assert_eq!(parse_true_false("msd_2"), None);
    }

    // -- parseAlphabetDeclaration ---------------------------------------------

    #[test]
    fn alphabet_declaration_explicit_set() {
        let d = parse_alphabet_declaration("{0, 1, 2}").unwrap();
        assert!(d.fully_consumed);
        assert_eq!(d.alphabet, vec![vec![0, 1, 2]]);
        assert!(d.number_systems[0].is_none());
    }

    #[test]
    fn alphabet_declaration_msd_k() {
        let d = parse_alphabet_declaration("msd_3").unwrap();
        assert!(d.fully_consumed);
        assert_eq!(d.alphabet, vec![vec![0, 1, 2]]);
        assert!(d.number_systems[0].is_some());
        assert_eq!(d.number_systems[0].as_ref().unwrap().name(), "msd_3");
    }

    #[test]
    fn alphabet_declaration_bare_msd_defaults_to_base_2() {
        let d = parse_alphabet_declaration("msd").unwrap();
        assert_eq!(d.alphabet, vec![vec![0, 1]]);
        assert_eq!(d.number_systems[0].as_ref().unwrap().name(), "msd_2");
    }

    #[test]
    fn alphabet_declaration_dangling_underscore_quirk() {
        // Verified against the real javac'd `PATTERN_NEXT_ALPHABET_TOKEN`: "msd_"
        // alone matches ENTIRELY via Alt 2 (`(msd|lsd)(\d+|\w+)`, not Alt 1), because
        // `\w+` (tried after `\d+` fails) matches the literal '_' itself (it's a
        // word character) even though Alt 1's explicit-literal-'_' branch failed to
        // find anything USABLE after it. `normalizeNumberSystemToken("msd_")`
        // returns `"msd_"` unchanged (starts with `MSD_UNDERSCORE`), producing the
        // unusable base name `""` -- this becomes a clean `NotDefined` error here
        // rather than a silent misparse.
        let d = parse_alphabet_declaration("msd_");
        assert!(matches!(d, Err(NumSysError::NotDefined(_))));
    }

    #[test]
    fn alphabet_declaration_underscore_then_space_splits_into_two_tokens() {
        // Verified against real javac'd output: "msd_ 10" tokenizes as "msd_ "
        // (Alt 2's \w+ = "_", then the pattern's own trailing \s* eats the space)
        // followed by a SEPARATE token "10" (Alt 4, bare number) -- NOT "msd_10" as
        // one token, since a space sits between the '_' and the digits. Checked at
        // the raw-tokenizer level (not through `parse_alphabet_declaration`) because
        // the first token normalizes to the same unusable `"msd_"` name as the
        // previous test and would abort the loop with `NotDefined` before a second
        // token is ever reached -- exactly like real Java's
        // `NumberSystem.getComputeIfAbsent("msd_")` throwing immediately there too.
        let (tok1, len1) = match_next_alphabet_token("msd_ 10").unwrap();
        assert!(matches!(tok1, AlphabetToken::Ns(ref t) if t == "msd_"));
        let (tok2, len2) = match_next_alphabet_token(&"msd_ 10"[len1..]).unwrap();
        assert!(matches!(tok2, AlphabetToken::Ns(ref t) if t == "10"));
        assert_eq!(len1 + len2, "msd_ 10".len());
    }

    #[test]
    fn alphabet_declaration_no_underscore_form() {
        // "msd3" -- Alt 2 (prefix + \d+ directly, no literal '_').
        let d = parse_alphabet_declaration("msd3").unwrap();
        assert_eq!(d.alphabet, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn alphabet_declaration_multi_track_mixed() {
        let d = parse_alphabet_declaration("msd_2 {5, -3, 7} lsd_4").unwrap();
        assert!(d.fully_consumed);
        assert_eq!(d.alphabet.len(), 3);
        assert_eq!(d.alphabet[0], vec![0, 1]);
        assert_eq!(d.alphabet[1], vec![5, -3, 7]);
        assert_eq!(d.alphabet[2], vec![0, 1, 2, 3]);
        assert!(d.number_systems[0].is_some());
        assert!(d.number_systems[1].is_none());
        assert!(d.number_systems[2].is_some());
    }

    #[test]
    fn alphabet_declaration_bare_number_defaults_to_msd() {
        let d = parse_alphabet_declaration("10").unwrap();
        assert_eq!(d.alphabet, vec![(0..10).collect::<Vec<i32>>()]);
        assert_eq!(d.number_systems[0].as_ref().unwrap().name(), "msd_10");
    }

    #[test]
    fn alphabet_declaration_custom_base_name_errors_not_silent() {
        // File-backed custom bases (msd_fib, ...) are out of scope in wr-core's
        // NumberSystem -- this must be a clean Err, not a silent misparse.
        let err = parse_alphabet_declaration("msd_fib").unwrap_err();
        assert!(matches!(err, NumSysError::NotDefined(_)));
    }

    #[test]
    fn alphabet_declaration_unbalanced_brace_not_fully_consumed() {
        let d = parse_alphabet_declaration("{0, 1").unwrap();
        assert!(!d.fully_consumed);
        // The unmatched leading token contributes nothing.
        assert!(d.alphabet.is_empty());
    }

    #[test]
    fn alphabet_declaration_empty_set_is_not_fully_consumed() {
        // `{}` has no digits at all -- group 12 requires >=1 number, so the whole
        // token fails to match; the parse stops immediately.
        let d = parse_alphabet_declaration("{}").unwrap();
        assert!(!d.fully_consumed);
        assert!(d.alphabet.is_empty());
    }

    #[test]
    fn alphabet_declaration_trailing_comma_in_set_fails_to_close() {
        // "{0,1,}" -- group 12 backtracks to "0,1" (can't consume the dangling
        // comma), then the required `\s*}` fails against the leftover ",}"
        let d = parse_alphabet_declaration("{0,1,}").unwrap();
        assert!(!d.fully_consumed);
    }

    // Regression tests: Java's pattern is `\{\s*(...)\s*\}` (`ParseMethods.java:46`)
    // -- there IS a `\s*` right after the opening brace. Skipping it (as this port
    // used to) only "worked" by accident for an UNSIGNED first element (whose own
    // internal whitespace-skip absorbed the leading space); a signed first element
    // was silently rejected. The existing test suite systematically only exercised
    // shapes that would NOT have caught this (no leading whitespace at all, or a
    // non-first negative like `{5, -3, 7}` above).

    #[test]
    fn alphabet_declaration_set_with_leading_whitespace_and_signed_first_element() {
        let d = parse_alphabet_declaration("{ -1, 0, 1 }").unwrap();
        assert!(d.fully_consumed);
        assert_eq!(d.alphabet, vec![vec![-1, 0, 1]]);
    }

    #[test]
    fn alphabet_declaration_set_leading_whitespace_plus_sign() {
        let d = parse_alphabet_declaration("{ +1}").unwrap();
        assert!(d.fully_consumed);
        assert_eq!(d.alphabet, vec![vec![1]]);
    }

    #[test]
    fn alphabet_declaration_set_leading_tab_negative() {
        let d = parse_alphabet_declaration("{\t-1}").unwrap();
        assert!(d.fully_consumed);
        assert_eq!(d.alphabet, vec![vec![-1]]);
    }

    #[test]
    fn alphabet_declaration_multi_track_with_leading_whitespace_signed_set() {
        // Previously this silently truncated to a SINGLE track (the "msd_2" token),
        // since the malformed `{ -1, 0, 1 }` token failed to match at all and the
        // parse just stopped there instead of continuing.
        let d = parse_alphabet_declaration("msd_2 { -1, 0, 1 }").unwrap();
        assert!(d.fully_consumed);
        assert_eq!(d.alphabet.len(), 2);
        assert_eq!(d.alphabet[0], vec![0, 1]);
        assert_eq!(d.alphabet[1], vec![-1, 0, 1]);
    }

    // -- parseStateDeclaration / parseTransducerStateDeclaration -------------

    #[test]
    fn state_declaration_examples() {
        assert_eq!(parse_state_declaration("0 1"), Some((0, 1)));
        assert_eq!(parse_state_declaration("  12   -3  "), Some((12, -3)));
        assert_eq!(parse_state_declaration("5"), None); // missing output
        assert_eq!(parse_state_declaration("5-3"), None); // missing mandatory \s+
        assert_eq!(parse_state_declaration("5 -3 extra"), None);
    }

    /// Review finding: `"9999999999"` (10 digits, overflows i32) has no mandatory
    /// `\s+` after it at all, so Java's own regex never even matches (`m.find()`
    /// returns `false`, the caller reports `undefinedStatement`) -- `Integer.parseInt`
    /// is never reached in Java. This port used to eagerly parse the state-name digit
    /// run BEFORE checking for the mandatory whitespace, so it panicked here instead
    /// of returning `None` like Java's clean non-match.
    #[test]
    fn state_declaration_overflow_digit_run_with_no_match_returns_none_not_panic() {
        assert_eq!(parse_state_declaration("9999999999"), None);
        // Overflowing OUTPUT field, trailing garbage after it (also not a match).
        assert_eq!(parse_state_declaration("0 9999999999 x"), None);
    }

    #[test]
    fn transducer_state_declaration_examples() {
        assert_eq!(parse_transducer_state_declaration("  7  "), Some(7));
        assert_eq!(parse_transducer_state_declaration("7 8"), None);
        assert_eq!(parse_transducer_state_declaration(""), None);
    }

    // -- parseTransition / parseTransducerTransition --------------------------

    #[test]
    fn transition_single_symbol() {
        let (input, dest) = parse_transition("0 -> 1").unwrap();
        assert_eq!(input, vec![Some(0)]);
        assert_eq!(dest, vec![1]);
    }

    #[test]
    fn transition_multi_track_with_wildcard() {
        let (input, dest) = parse_transition("0 * -1 -> 2 3").unwrap();
        assert_eq!(input, vec![Some(0), None, Some(-1)]);
        assert_eq!(dest, vec![2, 3]);
    }

    #[test]
    fn transition_requires_arrow() {
        assert_eq!(parse_transition("0 1"), None);
    }

    #[test]
    fn transition_rejects_trailing_garbage() {
        assert_eq!(parse_transition("0 -> 1 x"), None);
    }

    /// Review finding: neither of these is a match in Java at all (no `->` for the
    /// bare digit run; the dest id itself overflows once `->` IS present) --
    /// `Integer.parseInt`/`UtilityMethods.parseInt` is never reached in real Walnut.
    /// This port used to parse eagerly while still probing the LHS/dest list, so it
    /// panicked instead of returning `None`.
    #[test]
    fn transition_overflow_digit_run_with_no_match_returns_none_not_panic() {
        assert_eq!(parse_transition("9999999999"), None); // no "->" anywhere
        assert_eq!(parse_transition("2147483648"), None); // ditto, i32::MAX + 1
        assert_eq!(parse_transition("0 -> 9999999999 x"), None); // dest overflow + trailing garbage
    }

    #[test]
    fn transducer_transition_example() {
        let t = parse_transducer_transition("0 -> 1 / -2").unwrap();
        assert_eq!(t.input, vec![Some(0)]);
        assert_eq!(t.dest, vec![1]);
        assert_eq!(t.output, vec![-2]);
    }

    #[test]
    fn transducer_transition_requires_slash_output() {
        assert!(parse_transducer_transition("0 -> 1").is_none());
    }

    /// Review finding: no `/`-output present at all, so this isn't a
    /// `parseTransducerTransition` match in Java regardless of the overflow-sized
    /// trailing digit run -- must return `None`, not panic.
    #[test]
    fn transducer_transition_overflow_dest_with_no_slash_returns_none_not_panic() {
        assert!(parse_transducer_transition("0 -> 1 9999999999").is_none());
    }

    // -- parseList -------------------------------------------------------------

    #[test]
    fn parse_list_examples() {
        assert_eq!(parse_list("0, 1, 2"), vec![Some(0), Some(1), Some(2)]);
        assert_eq!(parse_list("5 -3 *"), vec![Some(5), Some(-3), None]);
        assert_eq!(parse_list(""), Vec::<Option<i32>>::new());
    }

    #[test]
    fn parse_list_ignores_trailing_unparseable_remainder() {
        // Unlike parse_alphabet_declaration, parse_list has no "fully consumed"
        // check -- trailing junk after the last valid element is silently dropped.
        assert_eq!(parse_list("0, 1, abc"), vec![Some(0), Some(1)]);
    }

    // -- parseMorphism -----------------------------------------------------------

    #[test]
    fn morphism_matches_java_test_fixture() {
        // Mirrors `ParseMethodsTest.testParseMorphism` in walnut-java verbatim.
        let map = parse_morphism("0 -> 0010").unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&0), Some(&vec![0, 0, 1, 0]));
    }

    #[test]
    fn morphism_multiple_mappings_sorted_by_key() {
        let map = parse_morphism("1 -> 01\n0 -> 10").unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&0), Some(&vec![1, 0]));
        assert_eq!(map.get(&1), Some(&vec![0, 1]));
        // BTreeMap iterates in key order -- 0 before 1.
        assert_eq!(map.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn morphism_empty_image_is_allowed() {
        let map = parse_morphism("0 ->").unwrap();
        assert_eq!(map.get(&0), Some(&vec![]));
    }

    #[test]
    fn morphism_bracketed_symbols() {
        let map = parse_morphism("[5] -> [-3][2]1").unwrap();
        assert_eq!(map.get(&5), Some(&vec![-3, 2, 1]));
    }

    #[test]
    fn morphism_no_valid_mappings_errors() {
        let err = parse_morphism("this has no arrows in it").unwrap_err();
        assert_eq!(err, ParseMethodsError::NoValidMorphismMappings);
    }

    #[test]
    fn morphism_bracket_whitespace_quirk_wb_011() {
        // docs/WALNUT-BUGS.md WB-011: the bracket regex allows whitespace between
        // the sign and the digits, but Java's plain Integer.parseInt on that
        // captured text then throws. Ported verbatim as an Err here.
        let err = parse_morphism("[+ 5] -> 1").unwrap_err();
        assert!(matches!(err, ParseMethodsError::IntegerParseFailure(_)));
    }

    // -- MorphismTest (walnut-java, Automata/MorphismTest.java) mirrors -------
    //
    // `Morphism`'s constructor (`Morphism.java:53-54`) is a thin wrapper that calls
    // `ParseMethods.parseMorphism(mapString)` straight into its `mapping` field, so
    // these three cases exercise this crate's `parse_morphism` directly by checking
    // the same resulting map Java's `h.mapping` would hold. `Morphism` itself
    // (`length`/`range` derivation, `toWordAutomaton`) is out of this unit's scope
    // (deferred, per the Phase-3a plan) so those Morphism-level fields aren't
    // asserted here -- only the parse result this unit IS responsible for.
    // `testGamMorphism` additionally needs file-backed `Session`/morphism-library
    // reading, unrelated to `parse_methods` itself, so it's not mirrored here.

    #[test]
    fn morphism_test_uniform_image_lengths() {
        // MorphismTest.testImageLength, first half: "0->01 1->21 2->03 3->23".
        let map = parse_morphism("0->01 1->21 2->03 3->23").unwrap();
        assert_eq!(map.get(&0), Some(&vec![0, 1]));
        assert_eq!(map.get(&1), Some(&vec![2, 1]));
        assert_eq!(map.get(&2), Some(&vec![0, 3]));
        assert_eq!(map.get(&3), Some(&vec![2, 3]));
    }

    #[test]
    fn morphism_test_non_uniform_image_lengths() {
        // MorphismTest.testImageLength, second half: "0->0123 1->21 2->03 3->23" --
        // Java asserts h.length == -1 there (non-uniform), which is a Morphism-level
        // derivation this unit doesn't port; the parse result itself is still
        // checked here.
        let map = parse_morphism("0->0123 1->21 2->03 3->23").unwrap();
        assert_eq!(map.get(&0), Some(&vec![0, 1, 2, 3]));
        assert_eq!(map.get(&1), Some(&vec![2, 1]));
        assert_eq!(map.get(&2), Some(&vec![0, 3]));
        assert_eq!(map.get(&3), Some(&vec![2, 3]));
    }

    #[test]
    fn morphism_test_big_alphabet() {
        // MorphismTest.testBigAlphabet: "0->01 [11]->012 [12]->02". Java additionally
        // asserts h.range.size() == 3 (the Morphism-level UNION of all image values,
        // {0,1,2} -- not the same thing as the number of map keys, and not ported
        // here, out of this unit's scope); the underlying mapping is checked below.
        let map = parse_morphism("0->01 [11]->012 [12]->02").unwrap();
        assert_eq!(map.get(&0), Some(&vec![0, 1]));
        assert_eq!(map.get(&10), None);
        assert_eq!(map.get(&11), Some(&vec![0, 1, 2]));
        assert_eq!(map.get(&12), Some(&vec![0, 2]));
    }

    // -- Deferred-parsing correctness (review findings, both reviewers) -------

    #[test]
    fn morphism_abandons_unrelated_overflowing_digit_run() {
        // An i32-overflowing digit run that's part of a doomed candidate (no "->"
        // follows it) must be silently abandoned, exactly like Java's regex engine
        // backtracks away from it without ever calling Integer.parseInt on it --
        // NOT propagate an IntegerParseFailure and abort the whole parse.
        let map = parse_morphism("x9999999999 0 -> 1").unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&0), Some(&vec![1]));
    }

    #[test]
    fn morphism_fully_unparseable_input_is_no_valid_mappings_not_integer_error() {
        // A huge digit run that never gets a chance to pair with "->" anywhere in
        // the string must still surface as NoValidMappings (Java's WalnutException),
        // not an integer-parse-failure variant.
        let err = parse_morphism("garbage 99999999999999 garbage").unwrap_err();
        assert_eq!(err, ParseMethodsError::NoValidMorphismMappings);
    }

    // -- is_whitespace_line / is_comment_line ----------------------------------

    #[test]
    fn whitespace_and_comment_line_detection() {
        assert!(is_whitespace_line(""));
        assert!(is_whitespace_line("   \t  "));
        assert!(!is_whitespace_line("  x "));
        assert!(is_comment_line("# a comment"));
        assert!(is_comment_line("   # indented"));
        assert!(!is_comment_line("not a comment"));
    }

    /// Verified against the real compiled `PATTERN_COMMENT`/`Matcher.matches()`: `.`
    /// doesn't match line terminators and `matches()` requires the whole string
    /// consumed, so an embedded OR merely-trailing `\n`/`\r` anywhere at/after the `#`
    /// makes the whole line fail to match as a comment -- not just embedded newlines.
    #[test]
    fn comment_line_rejects_any_newline_at_or_after_the_hash() {
        assert!(!is_comment_line("#x\ny"));
        assert!(!is_comment_line("#x\n")); // trailing newline alone is NOT forgiven
        assert!(!is_comment_line("#a\r\n"));
        assert!(!is_comment_line("#a\rb"));
        assert!(is_comment_line("#x"));
        assert!(is_comment_line("#"));
        // Leading whitespace (before the '#') still tolerates embedded newlines,
        // since \s includes \n/\r.
        assert!(is_comment_line(" \n #x"));
        assert!(!is_comment_line(" \n #x\n"));
    }
}
