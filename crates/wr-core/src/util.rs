// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `util` — generic static helpers, ported from `Main/UtilityMethods.java` (185 LOC).
//!
//! This file is a grab-bag of small pure functions used by ≥22 Java files across the
//! whole Walnut codebase (`Automata/*`, `Main/*`, `Main/Commands/*`) — confirmed by
//! grepping `UtilityMethods\.` across `src/main/java`. It lands here, in `wr-core`
//! rather than a separate crate, because its widest and earliest use is inside
//! `wr-core` itself (`NumberSystem`/`ParseMethods`-adjacent number parsing); other
//! crates (`wr-io`, `wr-logic`, `wr-cli`) call into it via `wr-core`'s public API,
//! matching this project's Phase-3a plan (`.claude/plans/synthetic-prancing-aurora.md`,
//! unit U0b).
//!
//! # What's deliberately NOT ported
//!
//! `ADDRESS_FOR_UNIT_TEST_INTEGRATION_TEST_RESULTS` (`UtilityMethods.java:34`) is a
//! non-`final` `static String` holding a Java-test-resource path
//! (`"src/test/resources/integrationTests/"`), reassigned only by Java's own
//! integration-test harness to redirect where golden output gets written during a
//! test run. It has no role outside that harness (not a generic utility, unlike
//! everything else in this file) and this Rust port's own test/fixture layout is
//! unrelated (`tests/fixtures`, `tests/golden`, ...), so there is nothing here to
//! port it *to* — carried as this doc note instead of dead code.
//!
//! # A pre-existing duplicate, now de-duplicated
//!
//! [`crate::numsys`] already had its own private `is_number` (added in Phase 2's U7,
//! before this module existed) — it now delegates to [`is_number`] here instead of
//! keeping a second copy of the same one-line regex port.

use num_bigint::BigInt;
use std::collections::HashSet;
use std::fmt::{self, Display};
use std::hash::Hash;

/// `UtilityMethods.MISSING_ELT` (`:31`) — "useful for IntMaps especially."
pub const MISSING_ELT: i32 = -1;
/// `UtilityMethods.NO_COMMON_ROOT` (`:32`).
pub const NO_COMMON_ROOT: i32 = -1;

/// Java's default (non-`UNICODE_CHARACTER_CLASS`) `\s`: `[ \t\n\x0B\f\r]`. Used
/// throughout this crate's hand-rolled regex ports (here and in `wr-io::parse_methods`)
/// instead of Rust's broader Unicode-aware `char::is_whitespace`, so whitespace
/// handling matches Java's regex engine exactly, not just "close enough."
pub fn is_java_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// `UtilityMethods.isNumber(String)` (`:42-44`) — matches `^\d+$` exactly: no sign,
/// at least one digit, ASCII digits only. Deliberately NOT `s.parse::<i32>().is_ok()`:
/// this accepts leading zeros and arbitrarily long digit strings that would overflow
/// `i32` (Walnut's own `Integer.parseInt` on such a string throws — see
/// `NumSysError::BaseNotAnI32` in [`crate::numsys`]) and rejects a leading `+`/`-`.
pub fn is_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `UtilityMethods.parseNegNumber(String)` (`:50-55`) — matches `^neg_\d+$`; on a
/// match, returns [`parse_int`] of the text after `"neg_"` (Java does NOT itself
/// negate the value — the caller's own naming convention, e.g. `msd_neg_5`, is what
/// encodes the sign). On no match, returns `0` (not an `Option`/error — this is
/// Java's actual signature and callers rely on the `0` sentinel, e.g.
/// `NumberSystem.setAdditionAutomaton`'s `parseNegNumber(base) > 1` guard).
pub fn parse_neg_number(s: &str) -> i32 {
    match s.strip_prefix("neg_") {
        Some(rest) if is_number(rest) => parse_int(rest),
        _ => 0,
    }
}

/// `UtilityMethods.toTuple(List<T>)` (`:60-62`) — `"(" + genericListString(l, ",") + ")"`.
pub fn to_tuple<T: Display>(l: &[T]) -> String {
    format!("({})", generic_list_string(l, ","))
}

/// `UtilityMethods.toTransitionLabel(List<T>)` (`:64-71`) — a single element is
/// printed bare (no brackets); anything else (including the empty list, `""`) is
/// bracketed and comma-joined.
pub fn to_transition_label<T: Display>(l: &[T]) -> String {
    if l.len() == 1 {
        return l[0].to_string();
    }
    format!("[{}]", generic_list_string(l, ","))
}

/// `UtilityMethods.removeDuplicates(List<T>)` (`:76-81`) — dedups in place,
/// preserving FIRST-occurrence order (Java's `LinkedHashSet`). A no-op on `null`/
/// length-`<=1` in Java; the `null` half doesn't apply here (Rust has no nullable
/// `Vec`), so only the length check remains.
pub fn remove_duplicates<T: Eq + Hash + Clone>(l: &mut Vec<T>) {
    if l.len() <= 1 {
        return;
    }
    let mut seen: HashSet<T> = HashSet::new();
    let mut out = Vec::with_capacity(l.len());
    for item in l.drain(..) {
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    *l = out;
}

/// `UtilityMethods.areEqual(List<T>, List<T>)` (`:86-90`) — "Checks if the SET of L
/// and R are equal. L and R do not have duplicates" (per that doc comment; this
/// function does not itself enforce that precondition, matching Java). `None`/`None`
/// is equal (mirrors Java's `L == null && R == null`); `None`/`Some` is not.
pub fn are_equal<T: Eq + Hash>(l: Option<&[T]>, r: Option<&[T]>) -> bool {
    match (l, r) {
        (None, None) => true,
        (None, Some(_)) | (Some(_), None) => false,
        (Some(l), Some(r)) => {
            let ls: HashSet<&T> = l.iter().collect();
            let rs: HashSet<&T> = r.iter().collect();
            ls == rs
        }
    }
}

/// `UtilityMethods.removeIndices(List<T>, List<Integer>)` (`:95-103`) — removes the
/// elements at the given (0-based) positions, preserving relative order of what's
/// kept. Java does `!indices.contains(i)` (a linear scan per element, not a set) —
/// behaviorally identical to a set-based check here (order/dedup of `indices` doesn't
/// matter either way), just faster.
pub fn remove_indices<T: Clone>(l: &mut Vec<T>, indices: &[usize]) {
    let idx: HashSet<usize> = indices.iter().copied().collect();
    let mut out = Vec::with_capacity(l.len());
    for (i, item) in l.iter().enumerate() {
        if !idx.contains(&i) {
            out.push(item.clone());
        }
    }
    *l = out;
}

/// `UtilityMethods.parseInt(String)` (`:108-110`) — strips ALL whitespace
/// (`PATTERN_WHITESPACE = "\\s"`, i.e. every whitespace char anywhere in the string,
/// not just leading/trailing) then `Integer.parseInt`. Java lets a malformed/
/// overflowing result throw an uncaught `NumberFormatException`; every real call site
/// in this port passes text already validated by a regex-shaped matcher (so the only
/// realistic failure is `i32` overflow), so this mirrors that with a panic carrying
/// Java's own message shape rather than inventing a `Result` for an
/// internal-invariant violation, consistent with this crate's existing panic idiom.
/// (`NumSysError::BaseNotAnI32` in [`crate::numsys`] took the recoverable-`Result`
/// path instead because it's genuinely reachable straight from raw user input at the
/// `NumberSystem` boundary; this one is one layer further removed, already past a
/// regex gate.)
///
/// # That reasoning is only true of the call sites that remain
///
/// Tier-5 fuzzing found three call sites where it was false — a regex gate that
/// matches `\d+` says nothing about magnitude, so any of them could be handed a digit
/// run that overflows `i32` straight from user input. Those now call
/// [`try_parse_int`]; see its doc for the reproducers and the real-`walnut-java`
/// comparison. Do not add a new `parse_int` call on text whose *length* is not itself
/// bounded by the caller.
pub fn parse_int(s: &str) -> i32 {
    match try_parse_int(s) {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    }
}

/// `java.lang.NumberFormatException` as a recoverable value: the failure
/// [`try_parse_int`] reports instead of panicking.
///
/// [`Display`](fmt::Display) renders `NumberFormatException.getMessage()` verbatim —
/// `For input string: "8888888800"` — because the call sites that surface it
/// (`wr_core::regex::determine_encoded_regex`'s `[a,b,…]` alphabet vectors,
/// `wr_logic`'s `@N` alphabet-letter token, `wr_cli::alphabet`'s `{…}` sets, and
/// `wr_io::parse_methods`' state/transition groups) reproduce that text byte-for-byte
/// against the real `walnut-java` CLI, and Tier 1's `error*` fixtures compare message
/// text.
/// `payload` is the WHITESPACE-STRIPPED input, matching what Java's
/// `UtilityMethods.parseInt` actually hands `Integer.parseInt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberFormatError {
    payload: String,
}

impl NumberFormatError {
    /// The whitespace-stripped text that failed to parse.
    pub fn payload(&self) -> &str {
        &self.payload
    }
}

impl fmt::Display for NumberFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "For input string: \"{}\"", self.payload)
    }
}

impl std::error::Error for NumberFormatError {}

/// [`parse_int`]'s non-panicking form, for the call sites where the argument is **raw
/// user input** rather than something already past a regex gate.
///
/// `parse_int`'s own doc reasons that only an internal-invariant violation can make it
/// fail. Tier-5 fuzzing (Phase 4, U30) disproved that at four call sites, all reachable
/// straight from what a user types or from a `.txt` file: `reg r {0,1}
/// "([8888888800])"`, `?msd_2 T[x] = @8888888888`, `alphabet foo {8888888800} bar`, and
/// a `.txt` transducer state declaration whose id overflows `i32`. Real `walnut-java`
/// throws `NumberFormatException` on all of them
/// and `Prover.readBuffer`'s `catch (RuntimeException)` (`Prover.java:390-392`) returns
/// to the prompt — verified by running `target/Walnut-all.jar` on each: the failing
/// command reports the exception and the very next command in the same session
/// evaluates normally. A Rust `panic!` there is process-fatal, i.e. a port defect, not
/// Walnut behavior; those call sites use this function and propagate the failure as an
/// ordinary command-level error.
pub fn try_parse_int(s: &str) -> Result<i32, NumberFormatError> {
    let stripped = strip_whitespace(s);
    stripped
        .parse::<i32>()
        .map_err(|_| NumberFormatError { payload: stripped })
}

/// `UtilityMethods.parseBigInteger(String)` (`:115-117`) — same whitespace-stripping
/// convention as [`parse_int`], arbitrary precision.
///
/// The panic message on failure does NOT attempt to match `java.math.BigInteger`'s
/// constructor exactly (e.g. `new BigInteger("")` throws `"Zero length BigInteger"`,
/// `new BigInteger("+ 5")` throws `"Illegal embedded sign character"` — distinct
/// messages per failure shape that `num_bigint`'s `FromStr` doesn't expose). Every
/// real call site here already passes text validated by a regex-shaped matcher, so
/// this is reachable only on internal-invariant violations, not raw untrusted input
/// (unlike [`crate::numsys::NumSysError::BaseNotAnI32`]); left as a generic message
/// rather than hand-rolling `BigInteger`'s validation to reproduce its exact text.
pub fn parse_big_integer(s: &str) -> BigInt {
    let stripped = strip_whitespace(s);
    stripped
        .parse::<BigInt>()
        .unwrap_or_else(|_| panic!("For input string: \"{stripped}\""))
}

/// Iterates `.chars()`, not `.bytes()` — every byte of a multi-byte UTF-8 sequence
/// would individually fail [`is_java_whitespace`]'s ASCII-range check and get kept,
/// but reinterpreting each surviving byte `as char` (Latin-1-style) instead of
/// re-assembling the original codepoint is mojibake on any non-ASCII input. Every
/// real call site ([`parse_int`], [`parse_big_integer`]) only ever receives text
/// already regex-gated to ASCII digits/sign/whitespace, so this was latent, not live.
fn strip_whitespace(s: &str) -> String {
    s.chars()
        .filter(|&c| !c.is_ascii() || !is_java_whitespace(c as u8))
        .collect()
}

/// `UtilityMethods.commonRoot(int, int)` (`:123-134`) — the largest `r` such that both
/// `a` and `b` are powers of `r` (returns [`NO_COMMON_ROOT`] if none exists, `1`s
/// excluded per Java's explicit guard). Ported as the same tail-recursive shape Java
/// uses (`commonRoot(b, a)` / `commonRoot(a, b / a)`) — the recursion depth is
/// `O(log b)`, no real stack-depth concern for any input this port ever sees.
///
/// **Deliberate divergence, logged as `docs/WALNUT-BUGS.md` WB-012.** Java's
/// `commonRoot(a, 0)` for negative `a` (or the symmetric `commonRoot(0, a)`, which
/// swaps into the same shape) recurses forever: `0 % a == 0` in Java for any nonzero
/// `a`, so it calls `commonRoot(a, 0 / a)` = `commonRoot(a, 0)` again — identical
/// arguments every call. In Java this eventually throws `StackOverflowError` (loud,
/// if slow); in Rust the self-recursive call with unchanged arguments is exactly the
/// shape LLVM turns into a genuinely infinite loop in a release build — a silent
/// hang, which this project's stated discipline treats as strictly worse than a
/// crash. `a == 0` xor `b == 0` (with the other nonzero) also has no coherent
/// "common root" answer in Java either way — it degenerates to a `/ by zero`
/// `ArithmeticException` one recursion level up the `a > b` swap instead of hanging,
/// but faithfully reproducing either failure mode (a crash-shaped one and a
/// hang-shaped one, for what's really the same input case) isn't a coherent goal for
/// a hang-intolerant test suite. So both guard cleanly to [`NO_COMMON_ROOT`] here
/// instead of recursing — this is the sole intentional behavioral divergence in this
/// function, not fixing anything about the a,b != 0 arithmetic itself. `a == b == 0`
/// is unaffected (already returned by the `a == b` check above, matching Java, which
/// returns `0` for `commonRoot(0, 0)` without ever reaching the problematic branch).
pub fn common_root(a: i32, b: i32) -> i32 {
    if a == 1 || b == 1 {
        return NO_COMMON_ROOT;
    }
    if a == b {
        return a;
    }
    if a == 0 || b == 0 {
        return NO_COMMON_ROOT;
    }
    if a > b {
        return common_root(b, a);
    }
    if b % a == 0 {
        common_root(a, b / a)
    } else {
        NO_COMMON_ROOT
    }
}

/// `UtilityMethods.genericListString(List<?>, String)` (`:140-142`) — `Object::toString`
/// joined by `separator`.
pub fn generic_list_string<T: Display>(objects: &[T], separator: &str) -> String {
    objects
        .iter()
        .map(|o| o.to_string())
        .collect::<Vec<_>>()
        .join(separator)
}

/// `UtilityMethods.isSorted(List<String>)` (`:144-151`) — non-decreasing per
/// `String.compareTo` (lexicographic by UTF-16 code unit; Rust's `&str` `Ord` is
/// lexicographic by byte, which agrees with Java's ordering for the ASCII track
/// labels this is actually called on).
pub fn is_sorted(label: &[String]) -> bool {
    label.windows(2).all(|w| w[0] <= w[1])
}

/// `UtilityMethods.validateFile(String)` (`:153-159`) — Java returns the `File`
/// handle (used for its path elsewhere); this port has no equivalent "validated
/// handle" type worth inventing for a single boolean check, so it returns `()` on
/// success and the same message Java's `IllegalArgumentException` carries on
/// failure.
pub fn validate_file(path: &str) -> Result<(), String> {
    if std::path::Path::new(path).is_file() {
        Ok(())
    } else {
        Err(format!(
            "File does not exist or is not a valid file: {path}"
        ))
    }
}

/// `UtilityMethods.readFromFile(String)` (`:161-176`) — a missing file is NOT an
/// error, it silently returns `""` (only genuine I/O errors on an EXISTING file
/// propagate). Lines are rejoined with `\n` between them (Java uses
/// `System.lineSeparator()`; every platform this port targets/tests on is `\n`-native,
/// flagged here rather than silently assumed if this ever needs to run on Windows).
///
/// Two further edge-case divergences from `BufferedReader.readLine()`, both narrow
/// enough to leave as a doc note rather than a full behavioral port:
/// - Java's `readLine()` also treats a lone `\r` (old Mac line endings, no following
///   `\n`) as its own terminator; Rust's `str::lines()` does not (a lone `\r` stays
///   embedded in the line it appears in, or at the end of it).
/// - Java decodes malformed byte sequences leniently, substituting U+FFFD per the
///   platform default charset's replacement behavior; `std::fs::read_to_string`
///   requires strictly valid UTF-8 and returns `Err` on anything else.
pub fn read_from_file(file_path: &str) -> std::io::Result<String> {
    let path = std::path::Path::new(file_path);
    if !path.is_file() {
        return Ok(String::new());
    }
    let content = std::fs::read_to_string(path)?;
    // `BufferedReader.readLine()` strips line terminators; Java then rejoins with
    // `System.lineSeparator()` between lines (none after the last). `str::lines()`
    // matches `readLine`'s terminator-stripping behavior (splits on `\n`, `\r\n`) —
    // but not the lone-`\r` case, see the doc note above.
    Ok(content.lines().collect::<Vec<_>>().join("\n"))
}

/// `UtilityMethods.intRangeList(int)` (`:178-184`) — `[0, endExclusive)`. Java
/// pre-sizes the result with `new ArrayList<>(endExclusive)`, whose constructor
/// throws `IllegalArgumentException("Illegal Capacity: " + ...)` for a negative
/// argument *before* the loop (which itself would just execute zero times and be
/// silently harmless) ever runs — so a negative `end_exclusive` is a genuine Java
/// crash on this path, not a "return nothing" case. Panics here to match, rather
/// than the empty-`Vec` this used to silently return.
pub fn int_range_list(end_exclusive: i32) -> Vec<i32> {
    assert!(end_exclusive >= 0, "Illegal Capacity: {end_exclusive}");
    (0..end_exclusive).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_number_matches_digit_only() {
        assert!(is_number("0"));
        assert!(is_number("007"));
        assert!(!is_number(""));
        assert!(!is_number("-1"));
        assert!(!is_number("1a"));
    }

    #[test]
    fn parse_neg_number_only_matches_neg_prefix() {
        assert_eq!(parse_neg_number("neg_5"), 5);
        assert_eq!(parse_neg_number("neg_0"), 0);
        assert_eq!(parse_neg_number("5"), 0);
        assert_eq!(parse_neg_number("negative_5"), 0);
        assert_eq!(parse_neg_number("neg_-5"), 0); // "-5" fails is_number
    }

    #[test]
    fn to_tuple_and_transition_label() {
        assert_eq!(to_tuple(&[1, 2, 3]), "(1,2,3)");
        assert_eq!(to_tuple::<i32>(&[]), "()");
        assert_eq!(to_transition_label(&[5]), "5");
        assert_eq!(to_transition_label(&[1, 2]), "[1,2]");
        assert_eq!(to_transition_label::<i32>(&[]), "[]");
    }

    #[test]
    fn remove_duplicates_preserves_first_occurrence_order() {
        let mut v = vec![1, 3, 2, 1, 3];
        remove_duplicates(&mut v);
        assert_eq!(v, vec![1, 3, 2]);
    }

    #[test]
    fn remove_duplicates_noop_on_short_lists() {
        let mut v: Vec<i32> = vec![];
        remove_duplicates(&mut v);
        assert_eq!(v, Vec::<i32>::new());
        let mut v = vec![7];
        remove_duplicates(&mut v);
        assert_eq!(v, vec![7]);
    }

    #[test]
    fn are_equal_is_set_equality() {
        assert!(are_equal(Some(&[1, 2, 3][..]), Some(&[3, 2, 1][..])));
        assert!(!are_equal(Some(&[1, 2][..]), Some(&[1, 2, 3][..])));
        assert!(are_equal::<i32>(None, None));
        assert!(!are_equal(None, Some(&[1][..])));
    }

    #[test]
    fn remove_indices_drops_by_position() {
        let mut v = vec!["X", "Y", "Z", "W"];
        remove_indices(&mut v, &[1, 3]);
        assert_eq!(v, vec!["X", "Z"]);
    }

    #[test]
    fn parse_int_strips_internal_whitespace() {
        assert_eq!(parse_int("  5  "), 5);
        assert_eq!(parse_int("+  5"), 5);
        assert_eq!(parse_int("-\t5"), -5);
    }

    #[test]
    #[should_panic(expected = "For input string")]
    fn parse_int_panics_like_javas_number_format_exception() {
        parse_int("not a number");
    }

    /// Phase 4 U30 fuzz finding F1. `parse_int`'s panic is only defensible where the
    /// caller genuinely cannot pass an overflowing digit run; `try_parse_int` is the
    /// form the raw-user-input call sites use, and its message is
    /// `NumberFormatException.getMessage()` byte-for-byte (real `walnut-java` on
    /// `reg r {0,1} "([8888888800])"` prints exactly
    /// `java.lang.NumberFormatException: For input string: "8888888800"`).
    #[test]
    fn try_parse_int_reports_overflow_instead_of_panicking() {
        assert_eq!(try_parse_int("  -12  "), Ok(-12));
        let e = try_parse_int("8888888800").unwrap_err();
        assert_eq!(e.payload(), "8888888800");
        assert_eq!(e.to_string(), "For input string: \"8888888800\"");
        // Whitespace is stripped BEFORE parsing, so the reported payload is the
        // stripped text -- exactly what Java's `UtilityMethods.parseInt` hands
        // `Integer.parseInt`.
        assert_eq!(
            try_parse_int(" 88 888 888 00 ").unwrap_err().payload(),
            "8888888800"
        );
        // `parse_int` still panics with the identical text, for the call sites where
        // an overflow really would be an internal-invariant violation.
        assert!(std::panic::catch_unwind(|| parse_int("8888888800")).is_err());
    }

    #[test]
    fn parse_big_integer_handles_arbitrary_precision() {
        assert_eq!(
            parse_big_integer("99999999999999999999"),
            "99999999999999999999".parse::<BigInt>().unwrap()
        );
    }

    #[test]
    fn common_root_examples() {
        assert_eq!(common_root(8, 4), 2);
        assert_eq!(common_root(9, 27), 3);
        assert_eq!(common_root(5, 5), 5);
        assert_eq!(common_root(1, 5), NO_COMMON_ROOT);
        assert_eq!(common_root(6, 4), NO_COMMON_ROOT);
    }

    /// WB-012: `common_root(a, 0)` for negative `a` (and the symmetric `common_root(0,
    /// a)`) infinitely recurses in Java's own algorithm (`0 % a == 0` for nonzero `a`,
    /// recursing back into `commonRoot(a, 0 / a)` = `commonRoot(a, 0)`, unchanged).
    /// Must terminate cleanly here rather than hang — this test would itself hang (or
    /// stack-overflow) forever without the `a == 0 || b == 0` guard.
    #[test]
    fn common_root_zero_guard_terminates_cleanly() {
        assert_eq!(common_root(-3, 0), NO_COMMON_ROOT);
        assert_eq!(common_root(0, -3), NO_COMMON_ROOT);
        assert_eq!(common_root(-1, 0), NO_COMMON_ROOT);
        assert_eq!(common_root(0, -1), NO_COMMON_ROOT);
        // The positive-a/zero-b shape doesn't hang in Java (it hits a `/ by zero`
        // ArithmeticException one swap later instead), but it's the same
        // no-coherent-answer input shape, so it guards the same way here.
        assert_eq!(common_root(3, 0), NO_COMMON_ROOT);
        assert_eq!(common_root(0, 3), NO_COMMON_ROOT);
        // a == b == 0 is unaffected by the new guard -- it's caught by the earlier
        // `a == b` check, exactly like Java's `commonRoot(0, 0) == 0`.
        assert_eq!(common_root(0, 0), 0);
    }

    /// Spot-check (per the review brief: "all pairs with a,b in [-6,20]") that the new
    /// `a == 0 || b == 0` guard changed nothing for any pair where NEITHER operand is
    /// zero — reimplements Java's original (unguarded) recursive shape directly and
    /// compares against [`common_root`] over the full grid, skipping only the zero
    /// pairs the guard was added for (those are covered separately above, and the
    /// unguarded reference would hang/overflow on them by construction).
    #[test]
    fn common_root_matches_unguarded_reference_away_from_zero() {
        fn reference(a: i32, b: i32) -> i32 {
            if a == 1 || b == 1 {
                return NO_COMMON_ROOT;
            }
            if a == b {
                return a;
            }
            if a > b {
                return reference(b, a);
            }
            if b % a == 0 {
                reference(a, b / a)
            } else {
                NO_COMMON_ROOT
            }
        }
        for a in -6..=20 {
            for b in -6..=20 {
                if a == 0 || b == 0 {
                    continue;
                }
                assert_eq!(
                    common_root(a, b),
                    reference(a, b),
                    "common_root({a}, {b}) diverged from the unguarded reference"
                );
            }
        }
    }

    #[test]
    fn generic_list_string_joins_with_separator() {
        assert_eq!(generic_list_string(&[1, 2, 3], ","), "1,2,3");
        assert_eq!(generic_list_string::<i32>(&[], ","), "");
    }

    #[test]
    fn is_sorted_checks_non_decreasing() {
        let sorted = vec!["a".to_string(), "b".to_string(), "b".to_string()];
        assert!(is_sorted(&sorted));
        let unsorted = vec!["b".to_string(), "a".to_string()];
        assert!(!is_sorted(&unsorted));
        assert!(is_sorted(&[]));
    }

    #[test]
    fn validate_file_reports_missing_files() {
        assert!(validate_file("/nonexistent/path/definitely-not-here.txt").is_err());
    }

    #[test]
    fn read_from_file_returns_empty_string_for_missing_file() {
        assert_eq!(
            read_from_file("/nonexistent/path/definitely-not-here.txt").unwrap(),
            ""
        );
    }

    #[test]
    fn read_from_file_rejoins_lines_with_newline() {
        let dir = std::env::temp_dir().join(format!("wr-core-util-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.txt");
        std::fs::write(&path, "line1\nline2\nline3").unwrap();
        assert_eq!(
            read_from_file(path.to_str().unwrap()).unwrap(),
            "line1\nline2\nline3"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn int_range_list_examples() {
        assert_eq!(int_range_list(0), Vec::<i32>::new());
        assert_eq!(int_range_list(3), vec![0, 1, 2]);
    }

    #[test]
    #[should_panic(expected = "Illegal Capacity")]
    fn int_range_list_panics_on_negative_like_javas_illegal_argument_exception() {
        int_range_list(-1);
    }

    #[test]
    fn strip_whitespace_handles_non_ascii_without_mojibake() {
        // A non-ASCII char must survive unmangled -- the old byte-wise implementation
        // would split its UTF-8 encoding into individual bytes and reinterpret each
        // one `as char` (Latin-1-style), corrupting it (e.g. 'é' -- U+00E9, encoded as
        // the two UTF-8 bytes 0xC3 0xA9 -- would come back out as "Ã©").
        assert_eq!(strip_whitespace(" é 5 "), "é5");
    }
}
