// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **`convertNS`** (`wr_core::logicalops::convert_ns`, Phase 3b
//! U18), spot-checked against real `walnut-java` CLI output.
//!
//! # Why this file exists
//!
//! `convertNS` is the entire body of Walnut's `convert` command, and it is the one
//! `AutomatonLogicalOps` method whose result is not pinned by any existing unit test at the
//! *language* level: `AutomatonLogicalOpsTest`'s two `convertNS` cases both use the trivial
//! "accepts only the empty string" automaton, which every step of the pipeline maps to
//! itself, so they exercise the control flow and nothing else (their own comments say as
//! much). This file supplies the missing half — real, non-degenerate automata pushed
//! through every branch and compared against the real engine.
//!
//! Every case is checked three ways, because language equivalence alone is too weak here:
//!
//! 1. **the declared number system** (`alphabet` + `msd`) — a conversion that produced the
//!    right language in the *wrong base or direction* is the single most likely failure
//!    mode of this method, and several small automata (`x ≡ 0 mod 3` over `msd_2`, for one)
//!    are their own reversal, so an msd/lsd mix-up would otherwise pass silently;
//! 2. **semantic language equivalence** (`wr_core::equiv`), this project's standard bar;
//! 3. **per-state DFAO OUTPUT agreement over every word up to length 6** — because
//!    `convertNS` is documented (Java `:452`) as assuming a *word* automaton, and
//!    `wr_core::equiv` only compares `o[q] != 0` acceptance, which cannot tell a DFAO
//!    outputting `1` from one outputting `2`. The `mod3*` cases below have outputs
//!    `{0, 1, 2}` specifically to make that check bite.
//!
//! # Coverage map (every branch of `convertNS`, and which case reaches it)
//!
//! | branch (`AutomatonLogicalOps.java`) | case |
//! |---|---|
//! | equal bases, direction flip only (`:465-479`) | `even_msd2 -> lsd_2`, `mod3 -> lsd_2`, `lt5_msd4 -> lsd_2` |
//! | totalize on a partial table (`:470-471`, `:488-489`) | `lt5_msd4 -> *` (that fixture is not total) |
//! | source is lsd, pre-reversal (`:495-497`) | `lt5_lsd4 -> msd_2`, `lt5_lsd4 -> lsd_2` |
//! | ungroup `k^i -> k` (`:503-510`, `convertLsdBaseToRoot`) | `lt5_msd4 -> msd_2`, `lt5_lsd4 -> msd_2` |
//! | regroup `k -> k^j` (`:513-522`, `convertMsdBaseToExponent`) | `even -> msd_4`, `even -> msd_8`, `mod3 -> msd_4/msd_8` |
//! | **both** halves in one call (`fromBase != root != toBase`) | `lt5_msd4 -> msd_8/lsd_8`, `mod3msd4 -> msd_8/lsd_8` |
//! | final reversal (`:525-527`) | every `-> lsd_*` case |
//! | `exponent > 2` (three-digit grouping) | `even -> msd_8`, `mod3 -> msd_8` |
//! | WB-032's truncated exponent | `lt15_msd10 -> msd_1000` |
//!
//! # Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! # 1. plain automata (Automata Library)
//! cat > "Command Files/u18a.txt" <<'EOF'
//! def even "?msd_2 Ek x = 2*k";
//! convert $evenlsd2 lsd_2 $even;
//! convert $evenmsd4 msd_4 $even;
//! convert $evenmsd8 msd_8 $even;
//! convert $evenlsd4 lsd_4 $even;
//! def m4 "?msd_4 x < 5";
//! convert $m4msd2 msd_2 $m4;
//! convert $m4lsd2 lsd_2 $m4;
//! def base10 "?msd_10 x < 15";
//! convert $b10msd1000 msd_1000 $base10;
//! EOF
//! java -jar target/Walnut-all.jar u18a.txt < /dev/null
//!
//! cat > "Command Files/u18b.txt" <<'EOF'
//! def e4 "?lsd_4 x < 5";
//! convert $e4msd2 msd_2 $e4;
//! convert $e4lsd2 lsd_2 $e4;
//! EOF
//! java -jar target/Walnut-all.jar u18b.txt < /dev/null
//!
//! # 2. a genuine DFAO (outputs 0/1/2): x mod 3, msd_2. Hand-authored, then converted.
//! cat > "Word Automata Library/u18mod3.txt" <<'EOF'
//! msd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 1
//!
//! 1 1
//! 0 -> 2
//! 1 -> 0
//!
//! 2 2
//! 0 -> 1
//! 1 -> 2
//! EOF
//! cat > "Command Files/u18c.txt" <<'EOF'
//! convert mod3msd4 msd_4 u18mod3;
//! convert mod3lsd2 lsd_2 u18mod3;
//! convert mod3lsd4 lsd_4 u18mod3;
//! convert mod3msd8 msd_8 u18mod3;
//! EOF
//! java -jar target/Walnut-all.jar u18c.txt < /dev/null
//!
//! # 3. the BOTH-halves cases (fromBase != commonRoot != toBase). Feed back the already
//! #    captured msd_4 fixtures, so the sources are byte-identical to what this suite reads.
//! cp <repo>/tests/differential/fixtures/convert_ns/m4.txt       "Automata Library/u18m4.txt"
//! cp <repo>/tests/differential/fixtures/convert_ns/mod3msd4.txt "Word Automata Library/u18mod3msd4.txt"
//! cat > "Command Files/u18d.txt" <<'EOF'
//! convert $m4msd8 msd_8 $u18m4;
//! convert $m4lsd8 lsd_8 $u18m4;
//! convert mod3msd4to8 msd_8 u18mod3msd4;
//! convert mod3msd4tolsd8 lsd_8 u18mod3msd4;
//! EOF
//! java -jar target/Walnut-all.jar u18d.txt < /dev/null
//!
//! # 4. copy every produced .txt into fixtures/convert_ns/ under Walnut's own name
//! #    (kept verbatim so a re-capture is a straight overwrite). Note that this Walnut
//! #    writes into Session/<timestamp>/{Automata,Word Automata} Library/, not into the
//! #    top-level libraries, and prunes old session directories on the next start — so
//! #    copy the outputs out in the SAME shell invocation that produced them.
//! ```
//!
//! Captured 2026-08-13 against `walnut-java` at `Walnut v8.0-alpha`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logicalops::{convert_ns, ConvertNsError};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/convert_ns")
        .join(name)
}

fn read_fixture(name: &str) -> Automaton {
    wr_io::reader::read_automaton_txt(fixture_path(name))
        .unwrap_or_else(|e| panic!("fixture {name} must parse cleanly: {e:?}"))
}

/// Runs `a` from its (single-track) initial state over `word`, returning the output value
/// of the state it lands in, or `None` if the run falls off a partial transition table.
///
/// Digits index the track's alphabet directly, which for every automaton in this file is
/// `[0, 1, ..., base-1]`, so a digit IS its encoded symbol (single track ⇒ `encoder == [1]`).
fn run_word(a: &Automaton, word: &[i32]) -> Option<i32> {
    let mut q = a.fa.q0;
    for &digit in word {
        let dests: &Vec<usize> = a.fa.d[q].get(&digit)?;
        q = *dests.first()?;
    }
    Some(a.fa.o[q])
}

/// Every word of length `0..=max_len` over `0..base`, in a deterministic order.
fn all_words(base: i32, max_len: usize) -> Vec<Vec<i32>> {
    let mut words = vec![Vec::new()];
    let mut frontier = vec![Vec::new()];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for w in &frontier {
            for d in 0..base {
                let mut w2 = w.clone();
                w2.push(d);
                next.push(w2);
            }
        }
        words.extend(next.iter().cloned());
        frontier = next;
    }
    words
}

/// Asserts two automata assign the SAME OUTPUT VALUE to every word up to `max_len`, which
/// is strictly stronger than `wr_core::equiv`'s accept/reject comparison and is the check
/// that makes the DFAO cases meaningful. A word that falls off a partial table counts as
/// output `0` (this crate's "missing transition ⇒ reject" convention), which is exactly
/// how the two engines' own partial tables are read everywhere else.
fn assert_same_outputs(ours: &Automaton, theirs: &Automaton, base: i32, what: &str) {
    // `base^max_len` words are materialized, so the bound has to shrink as the base grows —
    // `CLAUDE.md`'s "generate SMALL" guardrail. An earlier draft used a flat `3` for every
    // base > 4, which meant 10^6 words for the base-100 WB-032 case: pure waste, since
    // every automaton here has at most 6 states and length-2 words already saturate their
    // reachable behaviour many times over.
    let max_len = if base <= 4 {
        6
    } else if base <= 16 {
        3
    } else {
        2
    };
    for word in all_words(base, max_len) {
        let a = run_word(ours, &word).unwrap_or(0);
        let b = run_word(theirs, &word).unwrap_or(0);
        assert_eq!(
            a, b,
            "{what}: outputs disagree on the digit string {word:?} \
             (ours {a}, real walnut-java {b})"
        );
    }
}

/// The whole comparison, for one conversion: read `input`, `convert_ns` it, and check the
/// result against the captured `expected` fixture on all three axes described in this
/// file's module docs.
fn check_conversion(input: &str, to_msd: bool, to_base: i32, expected: &str, expected_base: i32) {
    let mut ours = read_fixture(input);
    let what = format!(
        "{input} -> {}_{to_base}",
        if to_msd { "msd" } else { "lsd" }
    );

    convert_ns(&mut ours, to_msd, to_base)
        .unwrap_or_else(|e| panic!("{what}: convert_ns must succeed, got {e}"));

    let theirs = read_fixture(expected);

    // 1. the declared number system.
    let expected_alphabet: Vec<i32> = (0..expected_base).collect();
    assert_eq!(
        theirs.alphabet,
        vec![expected_alphabet.clone()],
        "{what}: the fixture's own alphabet must be 0..{expected_base}"
    );
    assert_eq!(
        theirs.msd,
        vec![Some(to_msd)],
        "{what}: the fixture's own header must declare the requested direction"
    );
    assert_eq!(
        ours.alphabet,
        vec![expected_alphabet],
        "{what}: the port must install the new base's alphabet"
    );
    assert_eq!(
        ours.msd,
        vec![Some(to_msd)],
        "{what}: the port must install the new direction"
    );
    assert_eq!(
        ours.fa.alphabet_size, expected_base as usize,
        "{what}: alphabet_size must follow the alphabet"
    );

    // 2. semantic language equivalence.
    let mut ours_total = ours.clone();
    let mut theirs_total = theirs.clone();
    ours_total.fa.totalize(0);
    theirs_total.fa.totalize(0);
    assert_eq!(
        automaton_language_equivalent(&ours_total, &theirs_total),
        Ok(true),
        "{what}: the converted language must match real walnut-java's"
    );

    // 3. DFAO output agreement.
    assert_same_outputs(&ours, &theirs, expected_base, &what);
}

// ---------------------------------------------------------------------------
// Same base, msd <-> lsd only (`:465-479`).
// ---------------------------------------------------------------------------

/// `x` even, `msd_2 -> lsd_2`. In msd this is "the last digit is 0"; in lsd it becomes
/// "the FIRST digit is 0", so the two automata genuinely differ — a no-op `convert_ns`
/// would fail this.
#[test]
fn even_msd2_to_lsd2_matches_real_walnut() {
    check_conversion("even.txt", false, 2, "evenlsd2.txt", 2);
}

/// `x < 5`, `msd_4 -> lsd_4`: the pure direction flip on a **partial** transition table
/// (`m4.txt`'s state 2 has no outgoing transitions at all), so it also exercises the
/// `if (!isDeterministicAndTotal()) totalize()` guard at `:470-471` — the one the Java
/// characterization test reaches only on a one-state automaton.
#[test]
fn lt5_msd4_to_lsd4_matches_real_walnut() {
    assert!(
        !read_fixture("m4.txt").fa.is_deterministic_and_total(),
        "this fixture must stay partial, or it stops exercising the totalize guard"
    );
    check_conversion("m4.txt", false, 4, "m4lsd4.txt", 4);
}

/// The `mod3` DFAO, `msd_2 -> lsd_2`. Outputs `{0, 1, 2}` — the case that makes the
/// output-agreement check bite, and the one where a `reverse` that dropped output values
/// would show up.
#[test]
fn mod3_dfao_msd2_to_lsd2_matches_real_walnut() {
    check_conversion("mod3.txt", false, 2, "mod3lsd2.txt", 2);
}

// ---------------------------------------------------------------------------
// Regrouping k -> k^j (`convertMsdBaseToExponent`).
// ---------------------------------------------------------------------------

#[test]
fn even_msd2_to_msd4_matches_real_walnut() {
    check_conversion("even.txt", true, 4, "evenmsd4.txt", 4);
}

/// `exponent == 3`, i.e. two extension rounds in `updateTransitionsFromMorphism` rather
/// than one — the case an off-by-one in its `for (i = 2; i <= exponent; i++)` loop would
/// break while leaving `msd_4` correct.
#[test]
fn even_msd2_to_msd8_matches_real_walnut() {
    check_conversion("even.txt", true, 8, "evenmsd8.txt", 8);
}

#[test]
fn even_msd2_to_lsd4_matches_real_walnut() {
    check_conversion("even.txt", false, 4, "evenlsd4.txt", 4);
}

#[test]
fn mod3_dfao_msd2_to_msd4_matches_real_walnut() {
    check_conversion("mod3.txt", true, 4, "mod3msd4.txt", 4);
}

#[test]
fn mod3_dfao_msd2_to_msd8_matches_real_walnut() {
    check_conversion("mod3.txt", true, 8, "mod3msd8.txt", 8);
}

#[test]
fn mod3_dfao_msd2_to_lsd4_matches_real_walnut() {
    check_conversion("mod3.txt", false, 4, "mod3lsd4.txt", 4);
}

// ---------------------------------------------------------------------------
// Ungrouping k^i -> k (`convertLsdBaseToRoot`).
// ---------------------------------------------------------------------------

/// `x < 5` over `msd_4 -> msd_2`. `commonRoot(4, 2) == 2 == toBase`, so only the ungrouping
/// half runs. The source is partial, so the `:488-489` totalize fires too.
#[test]
fn lt5_msd4_to_msd2_matches_real_walnut() {
    check_conversion("m4.txt", true, 2, "m4msd2.txt", 2);
}

#[test]
fn lt5_msd4_to_lsd2_matches_real_walnut() {
    check_conversion("m4.txt", false, 2, "m4lsd2.txt", 2);
}

/// Source is **lsd**, so `:495-497`'s pre-reversal runs before the ungrouping — the branch
/// `AutomatonLogicalOpsTest.testConvertNSBaseConversionFromLsd` reaches only on the
/// degenerate `{ε}` automaton.
#[test]
fn lt5_lsd4_to_msd2_matches_real_walnut() {
    check_conversion("e4.txt", true, 2, "e4msd2.txt", 2);
}

#[test]
fn lt5_lsd4_to_lsd2_matches_real_walnut() {
    check_conversion("e4.txt", false, 2, "e4lsd2.txt", 2);
}

// ---------------------------------------------------------------------------
// BOTH steps: ungroup k^i -> k AND regroup k -> k^j in one call.
//
// Every case above has one side equal to the common root, so exactly one of the two
// halves runs. These four are the only ones that chain the whole pipeline —
// pre-reversal (lsd source), reverse, `convertLsdBaseToRoot`, minimize, reverse-undo,
// `convertMsdBaseToExponent`, minimize, final reversal — with `currentlyReversed`
// actually toggling twice. `commonRoot(4, 8) == 2`, so `4 -> 2 -> 8`.
// ---------------------------------------------------------------------------

/// `x < 5`, `msd_4 -> msd_8`. Also a partial source table, so the `:488-489` totalize runs
/// ahead of both halves.
#[test]
fn lt5_msd4_to_msd8_matches_real_walnut() {
    check_conversion("m4.txt", true, 8, "m4msd8.txt", 8);
}

/// The same, ending in **lsd** — so the closing `if (toMsd == currentlyReversed)` reversal
/// at `:525-527` fires on a `currentlyReversed == false` automaton, the one arm of that
/// condition the single-step cases below `msd_4 -> msd_2` never combine with a regrouping.
#[test]
fn lt5_msd4_to_lsd8_matches_real_walnut() {
    check_conversion("m4.txt", false, 8, "m4lsd8.txt", 8);
}

/// The `mod3` DFAO already in `msd_4`, taken to `msd_8` — the both-steps path on genuine
/// `{0, 1, 2}` outputs, so a step that silently dropped or remapped output values (which
/// `wr_core::equiv` cannot see) is caught by the output-agreement check.
#[test]
fn mod3_dfao_msd4_to_msd8_matches_real_walnut() {
    check_conversion("mod3msd4.txt", true, 8, "mod3msd4to8.txt", 8);
}

/// The same DFAO to `lsd_8`: both halves *and* a direction flip, on a 6-state result — the
/// most-composed single call in this file.
#[test]
fn mod3_dfao_msd4_to_lsd8_matches_real_walnut() {
    check_conversion("mod3msd4.txt", false, 8, "mod3msd4tolsd8.txt", 8);
}

// ---------------------------------------------------------------------------
// WB-032: the truncated floating-point exponent.
// ---------------------------------------------------------------------------

/// `x < 15` over `msd_10`, asked to become **`msd_1000`** — and silently becoming
/// `msd_100` instead, in both engines.
///
/// `(int)(Math.log(1000) / Math.log(10))` is `2`, not `3`
/// (`docs/WALNUT-BUGS.md` WB-032), so `convertMsdBaseToExponent` groups two digits instead
/// of three. This test asserts the port reproduces the wrong answer **bit for bit against
/// the real engine's wrong answer** — it is the pin that would fail if someone "fixed" the
/// exponent computation in this port without going through the WALNUT-BUGS process, and it
/// would also fail on a platform whose `ln` is less accurate than the correctly-rounded one
/// Java and every mainstream libm provide.
#[test]
fn wb032_msd10_to_msd1000_silently_produces_msd100_in_both_engines() {
    // Note the `100`, not `1000`, as the expected base: that IS the bug.
    check_conversion("base10.txt", true, 1000, "b10msd1000.txt", 100);
}

// ---------------------------------------------------------------------------
// The three rejection paths, checked against the real CLI's own messages.
// ---------------------------------------------------------------------------

/// `convert $samens msd_2 $even` on an `msd_2` automaton prints
/// `New and old number systems are identical: msd_2` (captured live).
#[test]
fn identical_number_systems_is_rejected_with_javas_message() {
    let mut a = read_fixture("even.txt");
    let err = convert_ns(&mut a, true, 2).expect_err("must reject");
    assert_eq!(
        err,
        ConvertNsError::IdenticalNumberSystems {
            name: "msd_2".to_string()
        }
    );
    assert_eq!(
        err.to_string(),
        "New and old number systems are identical: msd_2"
    );
}

/// `convert $noroot msd_6 $even` prints
/// `New and old number systems must have bases k^i and k^j for some integer k.` (captured
/// live) — `commonRoot(2, 6)` has no common root even though `6 % 2 == 0`.
#[test]
fn no_common_root_is_rejected_with_javas_message() {
    let mut a = read_fixture("even.txt");
    let err = convert_ns(&mut a, true, 6).expect_err("must reject");
    assert_eq!(err, ConvertNsError::NoCommonRoot);
    assert_eq!(
        err.to_string(),
        "New and old number systems must have bases k^i and k^j for some integer k."
    );
}

/// `convert $multi msd_4 $two` on a two-track automaton prints
/// `Automaton must have exactly one input to be converted.` (captured live).
#[test]
fn multi_track_input_is_rejected_with_javas_message() {
    // Two tracks, both msd_2; the body is irrelevant, the guard fires on arity alone.
    let mut d0: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    d0.insert(0, vec![0]);
    let a_fa = wr_core::fa::Fa {
        true_false: None,
        q0: 0,
        q: 1,
        alphabet_size: 4,
        o: vec![1],
        d: vec![d0],
    };
    let mut a = Automaton::new(
        a_fa,
        vec![vec![0, 1], vec![0, 1]],
        vec!["x".to_string(), "y".to_string()],
        vec![Some(true), Some(true)],
    );
    let err = convert_ns(&mut a, true, 4).expect_err("must reject");
    assert_eq!(err, ConvertNsError::NotSingleInput);
    assert_eq!(
        err.to_string(),
        "Automaton must have exactly one input to be converted."
    );
}
