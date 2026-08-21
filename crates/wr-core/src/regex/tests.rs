// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Unit tests for [`crate::regex`].
//!
//! Ground truth for every "real Walnut does X" claim below was captured from the actual
//! `walnut-java` CLI (`target/Walnut-all.jar`) or, for the constructor with no CLI
//! command behind it, from a small Java driver compiled against that same jar. The
//! *corpus-scale* differential comparison (60 regexes, compared by language equivalence
//! against committed `walnut-java` output files) lives in
//! `tests/differential/tests/reg_brics_regex.rs`; this file pins the pieces that are
//! easier to state directly — AST shapes, every parse-error message and position, the
//! `Reg.determineEncodedRegex` rewriting rules, WB-024, and Walnut-independent Tier-4
//! invariants.

use super::*;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `Main/Commands/Reg.reg` (`Reg.java:18-40`) minus the file writing — the exact path
/// the `reg` command takes.
fn reg(alphabets: Vec<Vec<i32>>, baseexp: &str) -> Result<AutomatonDFA, RegexError> {
    let encoded = determine_encoded_regex(baseexp, &alphabets)?;
    let msd = vec![None; alphabets.len()];
    AutomatonDFA::from_encoded_regex(&encoded, alphabets, msd)
}

/// Parses a *bare* regex (no alphabet wrapping) with Brics' full flag set.
fn ast(s: &str) -> Result<RegexNode, RegexError> {
    Parser::parse(&u16s(s), FLAG_ALL)
}

fn ch(c: char) -> RegexNode {
    RegexNode::Char(c as u16)
}

fn err_message(s: &str) -> String {
    ast(s).expect_err("must not parse").message()
}

/// Every word over `0..alphabet_size` of length `<= max_len`, shortlex.
fn all_words(alphabet_size: usize, max_len: usize) -> Vec<Vec<i32>> {
    let mut out = vec![Vec::new()];
    let mut frontier = vec![Vec::new()];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for w in &frontier {
            for s in 0..alphabet_size as i32 {
                let mut w2 = w.clone();
                w2.push(s);
                next.push(w2);
            }
        }
        out.extend(next.iter().cloned());
        frontier = next;
    }
    out
}

/// Unwraps a built automaton down to its owned [`Fa`], so call sites can chain without
/// borrowing a temporary.
fn fa_of(m: AutomatonDFA) -> Fa {
    m.into_automaton().fa
}

/// The words of length `<= max_len` accepted by `fa`, shortlex.
fn language_up_to(fa: &Fa, alphabet_size: usize, max_len: usize) -> Vec<Vec<i32>> {
    all_words(alphabet_size, max_len)
        .into_iter()
        .filter(|w| fa.accepts_word(w))
        .collect()
}

// ---------------------------------------------------------------------------
// `convertEncodingForBrics`
// ---------------------------------------------------------------------------

#[test]
fn convert_encoding_for_brics_offsets_by_128_and_truncates_like_javas_char_cast() {
    assert_eq!(convert_encoding_for_brics(0), 128);
    assert_eq!(convert_encoding_for_brics(7), 135);
    // WB-024's core: a NEGATIVE encoding lands back inside dk.brics' reserved range
    // instead of above it. `-5 + 128 == 123 == '{'`.
    assert_eq!(convert_encoding_for_brics(-5), '{' as u16);
    assert_eq!(convert_encoding_for_brics(-1), 127);
    // Java's `(char) int` truncates to the low 16 bits rather than range-checking.
    assert_eq!(convert_encoding_for_brics(65536 - 128), 0);
}

// ---------------------------------------------------------------------------
// Parser: AST shape
// ---------------------------------------------------------------------------

#[test]
fn parse_reproduces_brics_precedence_union_loosest_then_intersection_then_concat() {
    // `a|b&cd` == a | (b & (cd)) — `parseUnionExp` -> `parseInterExp` ->
    // `parseConcatExp`.
    assert_eq!(
        ast("a|b&cd").expect("parses"),
        RegexNode::Union(
            Box::new(ch('a')),
            Box::new(RegexNode::Intersection(
                Box::new(ch('b')),
                // `makeConcatenation` fuses two adjacent chars into a string.
                Box::new(RegexNode::Str(u16s("cd"))),
            )),
        )
    );
}

#[test]
fn parse_binds_postfix_repetition_tighter_than_complement() {
    // `~a*` is `(~a)*`, NOT `~(a*)`: `parseRepeatExp` calls `parseComplExp` first and
    // only then consumes the `*`. Empirically confirmed against the real CLI —
    // `reg r {0,1} "~0*"` yields 3 states, `reg r {0,1} "~(0*)"` yields 2.
    assert_eq!(
        ast("~a*").expect("parses"),
        RegexNode::Repeat(Box::new(RegexNode::Complement(Box::new(ch('a')))))
    );
}

#[test]
fn parse_desugars_negated_char_class_into_anychar_and_complement() {
    assert_eq!(
        ast("[^ab]").expect("parses"),
        RegexNode::Intersection(
            Box::new(RegexNode::AnyChar),
            Box::new(RegexNode::Complement(Box::new(RegexNode::Union(
                Box::new(ch('a')),
                Box::new(ch('b')),
            )))),
        )
    );
}

#[test]
fn parse_treats_a_trailing_hyphen_in_a_class_as_a_literal() {
    // `RegExp.parseCharClass:813-814`.
    assert_eq!(
        ast("[a-]").expect("parses"),
        RegexNode::Union(Box::new(ch('a')), Box::new(ch('-')))
    );
    assert_eq!(
        ast("[a-c]").expect("parses"),
        RegexNode::CharRange('a' as u16, 'c' as u16)
    );
}

#[test]
fn parse_backslash_escapes_exactly_one_code_unit_with_no_interpretation() {
    // `parseCharExp` is `match('\\'); return next();` — `\n` is the LETTER n.
    assert_eq!(ast("\\n").expect("parses"), ch('n'));
    assert_eq!(ast("\\*").expect("parses"), ch('*'));
    assert_eq!(ast("\\\\").expect("parses"), ch('\\'));
}

#[test]
fn parse_recognizes_the_optional_syntax_leaves() {
    assert_eq!(ast(".").expect("parses"), RegexNode::AnyChar);
    assert_eq!(ast("#").expect("parses"), RegexNode::Empty);
    assert_eq!(ast("@").expect("parses"), RegexNode::AnyString);
    assert_eq!(ast("()").expect("parses"), RegexNode::Str(Vec::new()));
    assert_eq!(ast("\"ab\"").expect("parses"), RegexNode::Str(u16s("ab")));
    assert_eq!(
        ast("<foo>").expect("parses"),
        RegexNode::NamedAutomaton("foo".to_string())
    );
    assert_eq!(
        ast("<1-25>").expect("parses"),
        RegexNode::Interval(1, 25, 0)
    );
    // Equal-length endpoints record the digit count (`RegExp.java:865-868`), and
    // `imin > imax` swaps (`:869-873`).
    assert_eq!(
        ast("<05-12>").expect("parses"),
        RegexNode::Interval(5, 12, 2)
    );
    assert_eq!(
        ast("<12-05>").expect("parses"),
        RegexNode::Interval(5, 12, 2)
    );
}

#[test]
fn parse_maps_plus_and_brace_counts_onto_the_repeat_nodes() {
    assert_eq!(
        ast("a+").expect("parses"),
        RegexNode::RepeatMin(Box::new(ch('a')), 1)
    );
    assert_eq!(
        ast("a{3}").expect("parses"),
        RegexNode::RepeatMinMax(Box::new(ch('a')), 3, 3)
    );
    assert_eq!(
        ast("a{3,}").expect("parses"),
        RegexNode::RepeatMin(Box::new(ch('a')), 3)
    );
    assert_eq!(
        ast("a{2,4}").expect("parses"),
        RegexNode::RepeatMinMax(Box::new(ch('a')), 2, 4)
    );
}

#[test]
fn parse_empty_input_is_the_empty_string_language_not_an_error() {
    // `RegExp.java:202-203`. Unreachable through either Walnut entry point (both wrap
    // the regex), ported anyway.
    assert_eq!(
        Parser::parse(&[], FLAG_ALL).expect("parses"),
        RegexNode::Str(Vec::new())
    );
}

#[test]
fn parse_honours_the_syntax_flags_it_is_given() {
    // With INTERSECTION disabled, `&` is just a character, so `a&b` concatenates three
    // literals instead of intersecting two.
    assert_eq!(
        Parser::parse(&u16s("a&b"), FLAG_ALL & !FLAG_INTERSECTION).expect("parses"),
        RegexNode::Str(u16s("a&b"))
    );
    // With INTERVAL disabled, a `<n-m>`-shaped reference is an illegal identifier.
    assert_eq!(
        Parser::parse(&u16s("<1-2>"), FLAG_ALL & !FLAG_INTERVAL)
            .expect_err("must fail")
            .message(),
        "illegal identifier at position 4"
    );
    // ...and with AUTOMATON disabled, a plain identifier is an interval error.
    assert_eq!(
        Parser::parse(&u16s("<ab>"), FLAG_ALL & !FLAG_AUTOMATON)
            .expect_err("must fail")
            .message(),
        "interval syntax error at position 3"
    );
    // COMPLEMENT/EMPTY/ANYSTRING off: all three become plain characters.
    let flags = FLAG_ALL & !(FLAG_COMPLEMENT | FLAG_EMPTY | FLAG_ANYSTRING);
    assert_eq!(
        Parser::parse(&u16s("~#@"), flags).expect("parses"),
        RegexNode::Str(u16s("~#@"))
    );
}

// ---------------------------------------------------------------------------
// Parser: every throw site, with Java's exact message and position
// ---------------------------------------------------------------------------

#[test]
fn parse_errors_match_dk_brics_messages_and_positions() {
    assert_eq!(err_message("a{}"), "integer expected at position 2");
    assert_eq!(err_message("a{2"), "expected '}' at position 3");
    assert_eq!(err_message("a{2,3"), "expected '}' at position 5");
    assert_eq!(err_message("[ab"), "expected ']' at position 3");
    assert_eq!(err_message("\"ab"), "expected '\"' at position 3");
    assert_eq!(err_message("(ab"), "expected ')' at position 3");
    assert_eq!(err_message("<ab"), "expected '>' at position 3");
    assert_eq!(err_message("a)"), "end-of-string expected at position 1");
    assert_eq!(err_message("a\\"), "unexpected end-of-string");
    // Interval endpoints that aren't parseable integers (`RegExp.java:857-877`).
    assert_eq!(err_message("<-5>"), "interval syntax error at position 3");
    assert_eq!(err_message("<5->"), "interval syntax error at position 3");
    assert_eq!(
        err_message("<1-2-3>"),
        "interval syntax error at position 6"
    );
    assert_eq!(err_message("<a-b>"), "interval syntax error at position 4");
}

#[test]
fn a_named_automaton_reference_always_fails_at_construction_the_way_walnut_reaches_it() {
    // Walnut calls `RE.toAutomaton()` with a null automaton map AND a null provider, so
    // `dk.brics` itself throws `'<id>' not found` (`RegExp.java:394-395`). Confirmed
    // live: `reg r {0,1} "<abc>"` prints
    // `java.lang.IllegalArgumentException: 'abc' not found`.
    let err = reg(vec![vec![0, 1]], "<abc>").expect_err("must fail");
    assert_eq!(err, RegexError::AutomatonNotFound("abc".to_string()));
    assert_eq!(err.message(), "'abc' not found");
}

#[test]
fn a_numerical_interval_is_a_documented_scope_exclusion_not_a_silent_wrong_answer() {
    // Unreachable via `reg` (its digits are rewritten first — see the next test), so
    // this goes through the only other entry point, which keeps raw digits.
    let err = AutomatonDFA::from_regex_over_alphabet("<1-5>", &[0, 1], None)
        .expect_err("intervals are not constructed");
    assert_eq!(err, RegexError::UnsupportedInterval);
}

#[test]
fn intervals_and_brace_counts_are_unreachable_through_the_reg_command() {
    // Both die in dk.brics' own parser, because `determineEncodedRegex` has already
    // replaced every digit with a private-use character. Originally verified against
    // real (pre-WB-024-fix) `walnut-java` over `{0,1}`; widened to `{0,1,2,3,4,5}` here
    // so every digit these two regexes mention (`1`/`5`, `0`/`2`/`3`) is genuinely in the
    // declared alphabet -- otherwise WB-024's fix would reject them before dk.brics' own
    // parser ever runs, which is a real (and separately pinned, see `wb_024_*` above)
    // behavior but not what THIS test is about. Each digit still becomes exactly one
    // replacement character regardless of alphabet size, so the expected message
    // positions are unchanged from the original `{0,1}` capture -- and this reasoning
    // was independently re-verified live by two adversarial reviewers of this unit
    // against a freshly built fixed jar, not just trusted (see
    // `tests/differential/tests/reg_brics_regex.rs`'s matching case for the live
    // re-capture record).
    assert_eq!(
        reg(vec![vec![0, 1, 2, 3, 4, 5]], "<1-5>")
            .expect_err("must fail")
            .message(),
        "interval syntax error at position 5"
    );
    assert_eq!(
        reg(vec![vec![0, 1, 2, 3, 4, 5]], "0{2,3}")
            .expect_err("must fail")
            .message(),
        "integer expected at position 3"
    );
}

// ---------------------------------------------------------------------------
// `Reg.determineEncodedRegex`
// ---------------------------------------------------------------------------

#[test]
fn determine_encoded_regex_replaces_bare_digits_with_their_encodings() {
    // One track `{0,1}`: digit 0 -> encoding 0 -> char 128; digit 1 -> 1 -> 129.
    assert_eq!(
        determine_encoded_regex("01*", &[vec![0, 1]]).expect("encodes"),
        vec![128, 129, '*' as u16]
    );
}

#[test]
fn determine_encoded_regex_replaces_bracketed_vectors_and_respects_alphabet_order() {
    // Two tracks: encoder = [1, 2]. `[1,0]` -> 1*1 + 2*0 = 1 -> char 129.
    // `[0,1]` -> 1*0 + 2*1 = 2 -> char 130.
    let alphabet = vec![vec![0, 1], vec![0, 1]];
    assert_eq!(
        determine_encoded_regex("[1,0][0,1]", &alphabet).expect("encodes"),
        vec![129, 130]
    );
    // A track whose digits are listed out of order encodes by INDEX, not by value:
    // `{2,4,1}` puts digit 4 at index 1.
    assert_eq!(
        determine_encoded_regex("4", &[vec![2, 4, 1]]).expect("encodes"),
        vec![129]
    );
}

#[test]
fn determine_encoded_regex_strips_whitespace_and_accepts_signs_and_spacing_in_vectors() {
    // encoder = [1, 2]; `[ +1 , -1 ]` -> 1*1 + 2*0 = 1 -> 129.
    let alphabet = vec![vec![0, 1], vec![-1, 1]];
    assert_eq!(
        determine_encoded_regex("[ +1 , -1 ] *", &alphabet).expect("encodes"),
        vec![129, '*' as u16]
    );
}

#[test]
fn determine_encoded_regex_rejects_a_vector_whose_arity_misses_the_track_count() {
    let err = determine_encoded_regex("[0,1]", &[vec![0, 1]]).expect_err("arity mismatch");
    assert_eq!(
        err.message(),
        "Mismatch between vector length in regex and specified number of inputs to automaton"
    );
}

/// Phase 4 U30 fuzz finding F1. `reg r {0,1} "([8888888800])"` is a plausible
/// command line, and the `\d+` behind `PAT_FOR_A_SINGLE_ELEMENT_OF_A_SET` bounds the
/// element's shape but not its magnitude — so `UtilityMethods.parseInt` overflows.
/// Real `walnut-java` prints
/// `java.lang.NumberFormatException: For input string: "8888888800"` and
/// `Prover.readBuffer`'s `catch (RuntimeException)` returns to the prompt (verified
/// on `target/Walnut-all.jar`: the next command in the same session still evaluates).
/// This port used to `panic!` here, which was process-fatal.
#[test]
fn determine_encoded_regex_reports_an_i32_overflowing_vector_element_instead_of_panicking() {
    let err = determine_encoded_regex("([8888888800])", &[vec![0, 1]]).expect_err("overflows i32");
    assert!(matches!(err, RegexError::NumberFormat(_)));
    assert_eq!(err.message(), "For input string: \"8888888800\"");
    // A bare (unbracketed) digit run is NOT this call site: `RE_FOR_AN_ALPHABET_VECTOR`
    // matches one bare digit at a time, so `8888888800` is ten separate one-element
    // vectors and nothing overflows -- only the bracketed form can carry a multi-digit
    // element. Over an alphabet that actually contains both digits used (`{0,8}`, not
    // `{0,1}` -- `8 ∉ {0,1}` would instead hit WB-024's guard, a DIFFERENT failure mode
    // covered separately below), this must succeed and produce exactly ten matches,
    // positively pinning the "ten separate vectors" parse shape rather than just
    // asserting some error occurred.
    assert_eq!(
        determine_encoded_regex("8888888800", &[vec![0, 8]])
            .expect("all ten digits are in {0,8}; must not overflow")
            .len(),
        10
    );
    // The WB-024 guard case: 8 is NOT in {0,1}, so this must be REJECTED post-WB-024-fix
    // (caught by the alphabet-membership guard, not the overflow path) -- pins the two
    // failure modes (`NumberFormat` vs `Walnut`) stay distinguishable rather than
    // conflated.
    let err2 = determine_encoded_regex("8888888800", &[vec![0, 1]])
        .expect_err("8 is not in {0,1}, caught by the WB-024 guard, not the overflow path");
    assert!(matches!(err2, RegexError::Walnut(_)));
    // No panic when the value merely does not exist in the alphabet -- that is WB-024's
    // fix (`docs/WALNUT-BUGS.md`): a clean, digit-naming error rather than the pre-fix
    // silent negative-encoding path.
    let err3 = determine_encoded_regex("9", &[vec![0, 1]]).expect_err("9 is not in {0,1}");
    assert_eq!(
        err3.message(),
        "digit 9 in position 0 of a regular-expression vector is not in that input's \
         alphabet: [0, 1]"
    );
}

#[test]
fn determine_encoded_regex_swallows_bracketed_digit_runs_that_look_like_char_classes() {
    // `[01]` matches `RE_FOR_AN_ALPHABET_VECTOR`'s FIRST alternative, so it is the
    // one-element vector holding the integer 1 — not a character class. Confirmed
    // against the real CLI: `reg r {0,1} "[01][01]"` builds the 3-state automaton for
    // the single word `11`.
    assert_eq!(
        determine_encoded_regex("[01]", &[vec![0, 1]]).expect("encodes"),
        vec![129]
    );
    // `[0-]`, by contrast, does NOT match (no `]` right after the digits), so only the
    // `0` is replaced and the brackets survive as a real character class.
    assert_eq!(
        determine_encoded_regex("[0-]", &[vec![0, 1]]).expect("encodes"),
        vec!['[' as u16, 128, '-' as u16, ']' as u16]
    );
    // A `^` likewise defeats the vector pattern, which is what keeps `[^01]` a class.
    assert_eq!(
        determine_encoded_regex("[^01]", &[vec![0, 1]]).expect("encodes"),
        vec!['[' as u16, '^' as u16, 128, 129, ']' as u16]
    );
}

#[test]
fn determine_encoded_regex_leaves_non_digit_syntax_untouched() {
    let encoded = determine_encoded_regex("(0|1)*&~(.)", &[vec![0, 1]]).expect("encodes");
    let expected: Vec<u16> = vec![
        '(' as u16, 128, '|' as u16, 129, ')' as u16, '*' as u16, '&' as u16, '~' as u16,
        '(' as u16, '.' as u16, ')' as u16,
    ];
    assert_eq!(encoded, expected);
}

// ---------------------------------------------------------------------------
// WB-024
// ---------------------------------------------------------------------------

#[test]
fn wb_024_out_of_alphabet_digits_are_now_cleanly_rejected() {
    // `docs/WALNUT-BUGS.md` WB-024, fixed in `walnut-java` commit `59eda64`: `9` is in
    // neither track, so this used to reach `RichAlphabet.encode`'s `List.indexOf` `-1`
    // path (encoder = [1, 4]; 1*(-1) + 4*(-1) = -5; -5 + 128 = 123 = '{') and silently
    // build a one-character-class regex instead of erroring. Now the digit is checked
    // against its track's alphabet BEFORE encoding. Message verified live against the
    // fixed real jar (see `tests/differential/tests/java_bugfix_wb024_wb025.rs`).
    let alphabet = vec![vec![0, 1, 2, 3], vec![0, 1]];
    let err = determine_encoded_regex("[9,9]", &alphabet).expect_err("9 is in neither track");
    assert_eq!(
        err.message(),
        "digit 9 in position 0 of a regular-expression vector is not in that input's \
         alphabet: [0, 1, 2, 3]"
    );
}

#[test]
fn wb_024_the_same_two_vectors_in_either_order_now_reject_identically() {
    // WB-024's headline symptom was ORDER-dependence -- the same two vectors behaved
    // completely differently depending on which one came first, with no diagnostic
    // connecting the failure to the actual out-of-alphabet digit. The fix removes the
    // order-dependence entirely: both orders now reject with the SAME message, naming the
    // first out-of-alphabet digit encountered (position 0 in both vectors here). Both
    // halves confirmed live against the real, fixed `walnut-java` CLI.
    let alphabet = vec![vec![0, 1, 2, 3], vec![0, 1]];
    let expected = "digit 9 in position 0 of a regular-expression vector is not in that \
                     input's alphabet: [0, 1, 2, 3]";

    let err_first = reg(alphabet.clone(), "[9,9][0,0]").expect_err("9 not in track 0");
    assert_eq!(err_first.message(), expected);

    let err_second = reg(alphabet, "[0,0][9,9]").expect_err("9 not in track 0, second vector");
    assert_eq!(err_second.message(), expected);
}

#[test]
fn wb_024_encode_with_index_of_still_produces_negative_encodings_directly() {
    // WB-024's fix is at `determine_encoded_regex`'s caller boundary (guard added before
    // `encode_with_index_of` runs), NOT inside `encode_with_index_of`/
    // `convert_encoding_for_brics` themselves -- exactly like Java's fix, which changed
    // `Reg.determineEncodedRegex` and left `RichAlphabet.encode` untouched (every other
    // Java call site already passes an in-range index). So calling these lower-level
    // functions directly, bypassing the new guard, still reproduces the underlying
    // mechanism -- this is a property of the (unchanged) primitives, not an observable
    // `reg` command outcome any more.
    for enc in [-1i32, -5, -119, -128] {
        assert!(
            convert_encoding_for_brics(enc) <= 127,
            "encoding {enc} must land inside dk.brics' reserved range"
        );
    }
    assert!(is_java_regex_space(convert_encoding_for_brics(-119)));
}

#[test]
fn wb_024_a_bracketed_integer_outside_the_alphabet_is_now_rejected() {
    // `[10]` reads as the one-element vector holding 10, which is in no track -- before
    // the fix this silently built a one-state, empty-language automaton (real
    // pre-fix Walnut: `reg r {0,1} "[10]"` reported `Set from brics:1 states`); now it is
    // rejected the same way any other out-of-alphabet digit is.
    let err = reg(vec![vec![0, 1]], "[10]").expect_err("10 is not in {0,1}");
    assert_eq!(
        err.message(),
        "digit 10 in position 0 of a regular-expression vector is not in that input's \
         alphabet: [0, 1]"
    );
}

#[test]
fn wb_024_an_in_alphabet_vector_is_unaffected_by_the_new_guard() {
    // Sanity check mirroring `walnut-java`'s own `RegTest.testInAlphabetVectorsStillEncodeNormally`
    // (added by the same fix commit): a regex that never mentions an out-of-alphabet
    // digit must still encode exactly as before.
    // encoder = [1, 4]; [0,0] -> 1*0 + 4*0 = 0 -> 128; [1,1] -> 1*1 + 4*1 = 5 -> 133.
    let alphabet = vec![vec![0, 1, 2, 3], vec![0, 1]];
    assert_eq!(
        determine_encoded_regex("[0,0][1,1]*", &alphabet).expect("all digits in range"),
        vec![128, 133, '*' as u16]
    );
}

// ---------------------------------------------------------------------------
// WB-025
// ---------------------------------------------------------------------------

#[test]
fn wb_025_the_tightened_guard_rejects_exactly_what_the_offset_cannot_encode() {
    // `docs/WALNUT-BUGS.md` WB-025, fixed in `walnut-java` commit `446dab2` on
    // `bugfix/wb-024-025`: unlike WB-024 (an out-of-alphabet digit producing a negative
    // encoding), this is a validator-legal symbol index whose `+128` offset itself
    // overflows `u16` (Java `char`). Before the fix, `validate_brics_alphabet_size`'s
    // `65535` bound (`MAX_BRICS_CHARACTER == (1 << 16) - 1`, the full `char`/`u16`
    // range) let every symbol index up to 65534 through, of which [65408, 65534] all
    // wrapped into dk.brics' reserved range. The fix tightens the guard to
    // `MAX_OFFSET_ENCODABLE_ALPHABET_SIZE == 65535 - 127 == 65408` (an alphabet of size
    // N uses indices `0..N-1`, so the largest index a size-N alphabet assigns is
    // `N - 1`, not `N` -- the bound is on SIZE, one more than the largest safe INDEX).
    // An earlier revision of both the Java fix and this port used `65407`, one less
    // than correct (alphabet size `65408`, max index `65407`, encoded char `65535` --
    // still safe, so it was being rejected unnecessarily) -- found by adversarial
    // review, corrected upstream first, then here to match.
    //
    // Boundary verified live against the fixed real jar (direct
    // `BricsConverter.setFromBricsAutomaton` invocation, since driving this size through
    // the full `reg` CLI's textual alphabet syntax is impractically slow/parser-hostile
    // at this scale -- see `tests/differential/tests/java_bugfix_wb024_wb025.rs`'s module
    // docs): `65409` throws `"size of input alphabet exceeds the limit of 65408"`.
    assert!(validate_offset_encodable_alphabet_size(65408).is_ok());
    let err = validate_offset_encodable_alphabet_size(65409).expect_err("one past the boundary");
    assert_eq!(
        err.message(),
        "size of input alphabet exceeds the limit of 65408"
    );

    // The OLD bound (65535) is now correctly rejected too -- every symbol index in
    // [65408, 65534] that used to wrap into the reserved range is unreachable through
    // `set_from_brics_automaton` any more.
    assert!(validate_offset_encodable_alphabet_size(65535).is_err());
}

#[test]
fn wb_025_convert_encoding_for_brics_still_wraps_the_same_way_when_called_directly() {
    // Exactly like WB-024's `encode_with_index_of` test above: the fix is in the GUARD
    // (`validate_offset_encodable_alphabet_size`), not in `convert_encoding_for_brics`
    // itself -- Java's fix didn't touch `convertEncodingForBrics` either. So the
    // truncating-cast wraparound mechanism is unchanged; it's just no longer reachable
    // through `set_from_brics_automaton` for a symbol index a validator-accepted
    // alphabet size would ever assign.
    // 65408 + 128 == 65536, which truncates (`as u16`) to 0 -- collides with dk.brics'
    // reserved NUL.
    assert_eq!(convert_encoding_for_brics(65408), 0);
    // 65534 + 128 == 65662, which truncates to 126 -- collides with '~' (dk.brics'
    // complement operator).
    assert_eq!(convert_encoding_for_brics(65534), '~' as u16);
    for x in 65408i32..=65534 {
        assert!(
            convert_encoding_for_brics(x) <= 127,
            "symbol index {x} must still wrap into dk.brics' reserved range when this \
             function is called directly, bypassing the guard"
        );
    }
    // Symbols just below the wraparound boundary do NOT collide -- pins the boundary
    // itself, not just "somewhere in this range". `MAX_OFFSET_ENCODABLE_ALPHABET_SIZE`
    // is `65408` (an accepted alphabet SIZE), so the largest INDEX such an alphabet ever
    // assigns is `MAX_OFFSET_ENCODABLE_ALPHABET_SIZE - 1 == 65407` -- exactly the value
    // tested here.
    assert!(convert_encoding_for_brics(65407) > 127);
}

// ---------------------------------------------------------------------------
// Construction: the `reg` (multi-track) entry point
// ---------------------------------------------------------------------------

#[test]
fn reg_builds_the_expected_language_for_a_single_track_regex() {
    let m = reg(vec![vec![0, 1]], "0*1").expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 2);
    assert_eq!(
        language_up_to(fa, 2, 3),
        vec![vec![1], vec![0, 1], vec![0, 0, 1]]
    );
}

#[test]
fn reg_builds_the_expected_language_for_a_multi_track_regex() {
    // encoder = [1, 2], so `[0,1]` is symbol 2 and `[1,0]` is symbol 1.
    let m = reg(vec![vec![0, 1], vec![0, 1]], "[0,1][1,0]*").expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 2);
    assert!(fa.accepts_word(&[2]));
    assert!(fa.accepts_word(&[2, 1, 1]));
    assert!(!fa.accepts_word(&[1]));
    assert!(!fa.accepts_word(&[2, 2]));
}

#[test]
fn reg_yields_a_one_state_transition_free_automaton_for_the_empty_language() {
    // Brics' `reduce()` keeps the initial state even when it is dead, which is why real
    // Walnut reports `Set from brics:1 states` for `#` rather than 0 — and why this port
    // uses its own dead-state prune instead of `crate::trim::trim` (which would
    // substitute a fully self-looping sink).
    let m = reg(vec![vec![0, 1]], "#").expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 1);
    assert!(fa.d[0].is_empty());
    assert!(!fa.is_accepting(0));
}

#[test]
fn reg_yields_a_one_state_accepting_automaton_for_the_empty_string_language() {
    let m = reg(vec![vec![0, 1]], "()").expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 1);
    assert!(fa.d[0].is_empty());
    assert!(fa.is_accepting(0));
}

#[test]
fn reg_installs_the_declared_tracks_and_alphabet_size_on_the_result() {
    let m = reg(vec![vec![0, 1], vec![0, 1, 2]], "[1,2]*").expect("builds");
    let a = m.automaton();
    assert_eq!(a.alphabet, vec![vec![0, 1], vec![0, 1, 2]]);
    assert_eq!(a.fa.alphabet_size, 6);
    assert_eq!(a.msd, vec![None, None]);
}

#[test]
fn from_encoded_regex_requires_one_number_system_per_track() {
    let encoded = determine_encoded_regex("0*", &[vec![0, 1]]).expect("encodes");
    let panicked = std::panic::catch_unwind(move || {
        let _ = AutomatonDFA::from_encoded_regex(&encoded, vec![vec![0, 1]], Vec::new());
    });
    assert!(
        panicked.is_err(),
        "a track/NS length mismatch must not pass silently"
    );
}

// ---------------------------------------------------------------------------
// Hand-written coverage for `&`, `~` and `[^…]`
//
// Zero of the golden corpus's 37 `reg` fixtures exercise these three operators, so the
// corpus alone would pass with them unimplemented. Each case below states the expected
// language explicitly rather than deferring to a fixture.
// ---------------------------------------------------------------------------

#[test]
fn intersection_operator_accepts_exactly_the_words_both_sides_accept() {
    let m = reg(vec![vec![0, 1]], "(0*1*)&(1*0*)").expect("builds");
    let fa = &m.automaton().fa;
    // 0*1* ∩ 1*0* = 0* ∪ 1*.
    for w in all_words(2, 4) {
        let all_zero = w.iter().all(|&s| s == 0);
        let all_one = w.iter().all(|&s| s == 1);
        assert_eq!(
            fa.accepts_word(&w),
            all_zero || all_one,
            "0*1* & 1*0* on {w:?}"
        );
    }
}

#[test]
fn intersection_operator_can_produce_the_empty_language_and_a_long_cycle() {
    let fa = fa_of(reg(vec![vec![0, 1]], "0&1").expect("builds"));
    assert!(fa.is_language_empty());

    // (00)* ∩ (000)* = (000000)*.
    let fa = fa_of(reg(vec![vec![0, 1]], "(00)*&(000)*").expect("builds"));
    assert_eq!(fa.q, 6);
    for len in 0..=12usize {
        assert_eq!(fa.accepts_word(&vec![0i32; len]), len % 6 == 0, "0^{len}");
    }
}

#[test]
fn intersection_operator_chains_right_associatively_over_three_operands() {
    // `((0)|(1))*&~(0*)&~(1*)`: all words containing at least one 0 AND at least one 1.
    let fa = fa_of(reg(vec![vec![0, 1]], "((0)|(1))*&~(0*)&~(1*)").expect("builds"));
    for w in all_words(2, 4) {
        let mixed = w.contains(&0) && w.contains(&1);
        assert_eq!(fa.accepts_word(&w), mixed, "on {w:?}");
    }
}

#[test]
fn complement_operator_accepts_exactly_the_words_the_operand_rejects() {
    let plain = reg(vec![vec![0, 1]], "0*1*").expect("builds");
    let negated = reg(vec![vec![0, 1]], "~(0*1*)").expect("builds");
    for w in all_words(2, 5) {
        assert_ne!(
            plain.automaton().fa.accepts_word(&w),
            negated.automaton().fa.accepts_word(&w),
            "~(0*1*) must disagree with 0*1* on {w:?}"
        );
    }
}

#[test]
fn complement_operator_is_involutive_and_handles_the_two_extremes() {
    let once = reg(vec![vec![0, 1]], "~(0*)").expect("builds");
    let twice = reg(vec![vec![0, 1]], "~(~(0*))").expect("builds");
    for w in all_words(2, 5) {
        assert_ne!(
            once.automaton().fa.accepts_word(&w),
            twice.automaton().fa.accepts_word(&w)
        );
    }
    // `~#` is everything over the alphabet; `~@` is nothing.
    let everything = reg(vec![vec![0, 1]], "~#").expect("builds");
    let nothing = reg(vec![vec![0, 1]], "~@").expect("builds");
    for w in all_words(2, 4) {
        assert!(everything.automaton().fa.accepts_word(&w), "~# on {w:?}");
        assert!(!nothing.automaton().fa.accepts_word(&w), "~@ on {w:?}");
    }
}

#[test]
fn complement_is_taken_over_the_declared_alphabet_only() {
    // `~(.)` is "every word except the one-character ones" — over `{0,1}` that includes
    // ε and everything of length >= 2, and nothing outside the alphabet leaks in.
    let fa = fa_of(reg(vec![vec![0, 1]], "~(.)").expect("builds"));
    for w in all_words(2, 4) {
        assert_eq!(fa.accepts_word(&w), w.len() != 1, "~(.) on {w:?}");
    }
}

#[test]
fn negated_character_class_accepts_exactly_the_alphabet_minus_the_listed_digits() {
    // Over `{0,1,2}`, `[^1]` is `{0,2}` — one symbol, not "any of the other 65535
    // characters" (the top-level `& [alphabet]*` cuts it down).
    let m = reg(vec![vec![0, 1, 2]], "[^1]").expect("builds");
    let fa = &m.automaton().fa;
    assert!(fa.accepts_word(&[0]));
    assert!(!fa.accepts_word(&[1]));
    assert!(fa.accepts_word(&[2]));
    assert!(!fa.accepts_word(&[]));
    assert!(!fa.accepts_word(&[0, 0]));
}

#[test]
fn negated_character_class_over_the_whole_alphabet_is_the_empty_language() {
    let fa = fa_of(reg(vec![vec![0, 1]], "[^01]").expect("builds"));
    assert!(fa.is_language_empty());
}

#[test]
fn negated_character_class_composes_with_repetition() {
    let fa = fa_of(reg(vec![vec![0, 1, 2]], "[^1]*").expect("builds"));
    for w in all_words(3, 4) {
        assert_eq!(
            fa.accepts_word(&w),
            w.iter().all(|&s| s != 1),
            "[^1]* on {w:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Construction: the single-track `convertFromBrics` entry point
//
// Cross-checked against a small Java driver run directly against
// `target/Walnut-all.jar` (`new AutomatonDFA(regex, alphabet, null)`), since this
// constructor has no CLI command behind it.
// ---------------------------------------------------------------------------

#[test]
fn from_regex_over_alphabet_matches_the_java_constructors_state_counts_and_languages() {
    // Java driver `p01`: Q=2.
    let m = AutomatonDFA::from_regex_over_alphabet("01*", &[0, 1, 2], Some(true)).expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 2);
    assert_eq!(
        language_up_to(fa, 3, 3),
        vec![vec![0], vec![0, 1], vec![0, 1, 1]]
    );

    // Java driver `p08`: digits map to their INDEX in the alphabet, so `4` over
    // `{2,4,1}` is symbol 1.
    let m = AutomatonDFA::from_regex_over_alphabet("4*", &[2, 4, 1], None).expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 1);
    assert_eq!(language_up_to(fa, 3, 2), vec![vec![], vec![1], vec![1, 1]]);

    // Java driver `p17`: `removeDuplicates` turns `{1,1,0,0,0}` into `[1,0]`, so digit 1
    // is symbol 0 and digit 0 is symbol 1.
    let m = AutomatonDFA::from_regex_over_alphabet("10*", &[1, 1, 0, 0, 0], None).expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 2);
    assert_eq!(language_up_to(fa, 2, 2), vec![vec![0], vec![0, 1]]);
}

#[test]
fn from_regex_over_alphabet_supports_brace_repeat_counts_the_reg_path_cannot_reach() {
    // Java driver `p09`/`p10`/`p11`/`p20`/`p21`.
    let q_of = |re: &str| {
        AutomatonDFA::from_regex_over_alphabet(re, &[0, 1], None)
            .expect("builds")
            .automaton()
            .fa
            .q
    };
    assert_eq!(q_of("0{2,3}"), 4);
    assert_eq!(q_of("0{2,}"), 3);
    assert_eq!(q_of("0{3}"), 4);
    assert_eq!(q_of("0{0,2}"), 3);
    // `min > max` is the empty language (`BasicOperations.repeat:204-205`).
    assert_eq!(q_of("0{3,2}"), 1);

    let fa =
        fa_of(AutomatonDFA::from_regex_over_alphabet("0{2,3}", &[0, 1], None).expect("builds"));
    for len in 0..=5usize {
        assert_eq!(fa.accepts_word(&vec![0i32; len]), (2..=3).contains(&len));
    }
}

#[test]
fn from_regex_over_alphabet_supports_quoted_string_literals() {
    // Java driver `p15`: `"01"` is a two-character string literal, Q=3.
    let m = AutomatonDFA::from_regex_over_alphabet("\"01\"", &[0, 1], None).expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 3);
    assert_eq!(language_up_to(fa, 2, 3), vec![vec![0, 1]]);
}

#[test]
fn from_regex_over_alphabet_handles_intersection_and_complement_too() {
    // Java driver `p22`: `(0|1)*&~(11)` — everything except the single word `11`.
    let m = AutomatonDFA::from_regex_over_alphabet("(0|1)*&~(11)", &[0, 1], None).expect("builds");
    let fa = &m.automaton().fa;
    assert_eq!(fa.q, 4);
    for w in all_words(2, 3) {
        assert_eq!(fa.accepts_word(&w), w != vec![1, 1], "on {w:?}");
    }
}

#[test]
fn from_regex_over_alphabet_rejects_an_empty_or_out_of_range_alphabet() {
    // Java driver `p18`/`p19`.
    assert_eq!(
        AutomatonDFA::from_regex_over_alphabet("0*", &[], None)
            .expect_err("must fail")
            .message(),
        "empty alphabet is not accepted"
    );
    assert_eq!(
        AutomatonDFA::from_regex_over_alphabet("0*", &[0, 10], None)
            .expect_err("must fail")
            .message(),
        "the input alphabet of an automaton generated from a regular expression must be a subset of {0,1,...,9}"
    );
}

#[test]
fn set_from_brics_automaton_rejects_an_alphabet_wider_than_a_java_char() {
    // Post-WB-025-fix: the limit named in the message is the tightened
    // `MAX_OFFSET_ENCODABLE_ALPHABET_SIZE` (65408), not the plain `char`/`u16` range
    // (65535) -- this alphabet size exceeds both, so it was already rejected before the
    // fix, but now with the tighter number in the message.
    assert_eq!(
        set_from_brics_automaton(65_536, &u16s("x"))
            .expect_err("must fail")
            .message(),
        "size of input alphabet exceeds the limit of 65408"
    );
}

/// WB-025's exact boundary (`docs/WALNUT-BUGS.md`), through the real public entry point
/// `set_from_brics_automaton` rather than the private guard directly -- confirms the
/// guard is actually wired up at the one call site that matters, not just present.
/// `65409` verified live against the fixed real jar (direct `BricsConverter.
/// setFromBricsAutomaton` invocation -- see
/// `tests/differential/tests/java_bugfix_wb024_wb025.rs`); `65535` (the OLD, too-wide
/// bound) is a size that used to be validator-legal and must now also be rejected.
#[test]
fn set_from_brics_automaton_rejects_an_alphabet_in_wb025s_former_danger_zone() {
    let err = set_from_brics_automaton(65_409, &u16s("x")).expect_err("one past the boundary");
    assert_eq!(
        err.message(),
        "size of input alphabet exceeds the limit of 65408"
    );
    assert!(set_from_brics_automaton(65_535, &u16s("x")).is_err());
}

// ---------------------------------------------------------------------------
// Tier-4 invariants (Walnut-independent)
// ---------------------------------------------------------------------------

/// A tiny generator of Brics regexes over the two digits `0` and `1`, produced as source
/// text so the whole pipeline (encoding, parsing, construction) is exercised.
fn arb_regex_text(depth: u32) -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        Just("0".to_string()),
        Just("1".to_string()),
        Just(".".to_string()),
        Just("()".to_string()),
        Just("#".to_string()),
        Just("@".to_string()),
    ];
    leaf.prop_recursive(depth, 24, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}|{b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}{b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}&{b})")),
            inner.clone().prop_map(|a| format!("({a})*")),
            inner.clone().prop_map(|a| format!("({a})?")),
            inner.prop_map(|a| format!("~({a})")),
        ]
    })
}

/// Alphabet sizes to sweep in the size-parameterized properties below. `1` is a
/// genuinely different code path for `AnyChar`/totalize/complement (a single-symbol
/// alphabet), and `> 2` exercises the multi-digit-leaf case that [`arb_regex_text`]'s
/// hardcoded `{0,1}` never reaches. Kept small (`1..=4`) per `CLAUDE.md`'s "generate
/// SMALL" test-performance guardrail -- this drives `regex_to_fa`'s full pipeline
/// (Thompson construction, subset construction, Valmari minimize) once per case.
fn arb_alphabet_size() -> impl Strategy<Value = usize> {
    1usize..=4
}

/// As [`arb_regex_text`], but the digit leaves range over `0..alphabet_size` instead of
/// being hardcoded to `{0,1}`. Digits outside the declared alphabet now hit WB-024's fix
/// (a clean rejection, `RegexError::Walnut`) instead of the property being tested here,
/// so this generator only ever emits digits that are actually in the track's alphabet.
fn arb_regex_text_over_alphabet(depth: u32, alphabet_size: usize) -> impl Strategy<Value = String> {
    let digit = (0..alphabet_size as i32).prop_map(|d| d.to_string());
    let leaf = prop_oneof![
        digit,
        Just(".".to_string()),
        Just("()".to_string()),
        Just("#".to_string()),
        Just("@".to_string()),
    ];
    leaf.prop_recursive(depth, 24, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}|{b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}{b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}&{b})")),
            inner.clone().prop_map(|a| format!("({a})*")),
            inner.clone().prop_map(|a| format!("({a})?")),
            inner.prop_map(|a| format!("~({a})")),
        ]
    })
}

/// Pairs a generated `alphabet_size` (see [`arb_alphabet_size`]) with a regex text drawn
/// from that same alphabet (see [`arb_regex_text_over_alphabet`]) -- the two must be
/// generated together (`prop_flat_map`, not two independent `in` clauses) since the
/// digits the regex generator is allowed to emit depend on the chosen size.
fn arb_regex_with_alphabet_size(depth: u32) -> impl Strategy<Value = (usize, String)> {
    arb_alphabet_size().prop_flat_map(move |size| {
        arb_regex_text_over_alphabet(depth, size).prop_map(move |re| (size, re))
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// `L(~E)` is exactly the complement of `L(E)` over the declared alphabet — the
    /// property the module docs' `(Σ* \ L) ∩ A* = A* \ (L ∩ A*)` argument rests on,
    /// checked against the operand itself rather than against a second derivation.
    ///
    /// Swept over `alphabet_size in 1..=4` (not just the fixed `{0,1}` every other
    /// property in this file uses): `alphabet_size == 1` is a genuinely different code
    /// path for `AnyChar`/totalize/complement, and this is the cheapest place to catch a
    /// regression there since complement is exactly the operation most likely to get a
    /// single-symbol alphabet's `AnyChar` wrong.
    #[test]
    fn complement_is_exactly_set_complement((alphabet_size, re) in arb_regex_with_alphabet_size(3)) {
        let alphabet = vec![(0..alphabet_size as i32).collect::<Vec<_>>()];
        let plain = reg(alphabet.clone(), &re).expect("generated regexes always parse");
        let negated = reg(alphabet, &format!("~({re})"))
            .expect("generated regexes always parse");
        for w in all_words(alphabet_size, 4) {
            prop_assert_ne!(
                plain.automaton().fa.accepts_word(&w),
                negated.automaton().fa.accepts_word(&w),
                "on word {:?} of `{}` (alphabet_size {})", w, re, alphabet_size
            );
        }
    }

    /// `L(E1 & E2) = L(E1) ∩ L(E2)` and `L(E1 | E2) = L(E1) ∪ L(E2)`.
    #[test]
    fn intersection_and_union_agree_with_set_operations(
        a in arb_regex_text(2),
        b in arb_regex_text(2),
    ) {
        let alphabet = vec![vec![0, 1]];
        let fa_a = reg(alphabet.clone(), &a).expect("parses");
        let fa_b = reg(alphabet.clone(), &b).expect("parses");
        let fa_and = reg(alphabet.clone(), &format!("({a})&({b})")).expect("parses");
        let fa_or = reg(alphabet, &format!("({a})|({b})")).expect("parses");
        for w in all_words(2, 4) {
            let in_a = fa_a.automaton().fa.accepts_word(&w);
            let in_b = fa_b.automaton().fa.accepts_word(&w);
            prop_assert_eq!(fa_and.automaton().fa.accepts_word(&w), in_a && in_b);
            prop_assert_eq!(fa_or.automaton().fa.accepts_word(&w), in_a || in_b);
        }
    }

    /// `L(E*)` is the Kleene star of `L(E)`, checked against an independent
    /// dynamic-programming decomposition rather than against the construction.
    #[test]
    fn star_is_exactly_kleene_star(re in arb_regex_text(2)) {
        let alphabet = vec![vec![0, 1]];
        let base = reg(alphabet.clone(), &re).expect("parses");
        let starred = reg(alphabet, &format!("({re})*")).expect("parses");
        let base_fa = &base.automaton().fa;
        for w in all_words(2, 4) {
            // `splittable[i]` == "w[..i] decomposes into words of L(base)".
            let mut splittable = vec![false; w.len() + 1];
            splittable[0] = true;
            for end in 1..=w.len() {
                splittable[end] = (0..end)
                    .any(|start| splittable[start] && base_fa.accepts_word(&w[start..end]));
            }
            prop_assert_eq!(
                starred.automaton().fa.accepts_word(&w),
                splittable[w.len()],
                "star of `{}` on {:?}", re, w
            );
        }
    }

    /// Every automaton this module builds is deterministic, minimal and dead-state-free
    /// — the shape `AutomatonDFA` promises and the shape real Walnut's
    /// `Set from brics:N states` count assumes.
    ///
    /// Swept over `alphabet_size in 1..=4` for the same reason as
    /// `complement_is_exactly_set_complement` above — `alphabet_size == 1` and
    /// `alphabet_size > 2` are both untouched by the fixed `{0,1}` alphabet every other
    /// property in this file uses.
    #[test]
    fn built_automata_are_deterministic_minimal_and_trimmed((alphabet_size, re) in arb_regex_with_alphabet_size(3)) {
        let alphabet = vec![(0..alphabet_size as i32).collect::<Vec<_>>()];
        let m = reg(alphabet, &re).expect("parses");
        let fa = &m.automaton().fa;
        prop_assert!(fa.is_deterministic());
        prop_assert!(fa.q >= 1);
        // No dead state survives except a lone dead `q0` (Brics' `reduce()` keeps it).
        for q in 0..fa.q {
            if q == fa.q0 {
                continue;
            }
            let mut probe = fa.clone();
            probe.q0 = q;
            prop_assert!(
                !probe.is_language_empty(),
                "state {} of `{}` cannot reach acceptance", q, re
            );
        }
        // Re-minimizing changes nothing about the state count.
        let mut total = fa.clone();
        total.totalize(0);
        let remin = crate::minimize::minimize(&total).expect("deterministic");
        prop_assert_eq!(remove_dead_states(&remin).q, fa.q, "`{}` was not minimal", re);
    }
}
