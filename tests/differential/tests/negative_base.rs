// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **negative-base numeration** (`msd_neg_k`, `lsd_neg_k`,
//! `msd_neg_fib`, `lsd_neg_fib`), spot-checked against real `walnut-java` CLI output —
//! `docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`, Layer A.
//!
//! # Why this file exists, stated honestly
//!
//! It is **not** the primary evidence that the negative-base port is correct, and should
//! not be read as if it were. That evidence is Tier 1: `walnut-java`'s own integration
//! corpus contains **68 negative-base fixtures** (39 `msd_neg_2`, 14 `msd_neg_fib`, 11
//! `lsd_neg_fib`, 3 `lsd_neg_2`, 1 `msd_neg_10` — see
//! `walnut-java/phase0-artifacts/subset-filter.json`), which this port now replays green
//! through `tests/golden`. They cover comparison, all six relations, boolean connectives,
//! `∃`/`∀`, multiplication, division by both signs, and word tokens.
//!
//! What this file adds over that:
//!
//! * **The fast tier.** `tests/golden` is `#[ignore]`d (DESIGN.md §5's gated-slow tier);
//!   these run on every `cargo test --workspace`.
//! * **A localized failure.** A corpus regression reports "fixture 431 diverges"; these
//!   report which *construction* diverged — comparator, adder, negative constant,
//!   multiplication by a negative constant, division by a negative constant, or the
//!   file-backed custom base.
//! * **`lsd_neg_fib`'s complement fallback, checked directly.** `walnut-java` ships
//!   `Custom Bases/msd_neg_fib{,_addition,_less_than}.txt` and **no** `lsd_neg_fib*` file
//!   at all, so `?lsd_neg_fib` resolves ONLY through
//!   `NumberSystem.loadAutomatonOrNull`'s opposite-direction-complement-plus-reverse
//!   fallback (`NumberSystem.java:304-319`) — on both engines. That is two independent
//!   mechanisms stacked (negative base ⨯ reversed custom base) and it deserves a named
//!   test rather than being inferred from 11 corpus fixtures passing.
//!
//! **Result: no divergence.** All nine automaton cases and both closed-formula verdicts
//! below match real `walnut-java` exactly, on the first run.
//!
//! # Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Command Files/negcap.txt" <<'EOF'
//! eval nblt          "?msd_neg_2 x < y";
//! eval nbltlsd       "?lsd_neg_2 x < y";
//! eval nbadd         "?msd_neg_2 x + y = z";
//! eval nbconst       "?msd_neg_2 x = _5";
//! eval nbquant       "?msd_neg_3 Ex (x + x = y & y < 5)";
//! eval nbdiv         "?msd_neg_2 y = x / _3";
//! eval nbmul         "?msd_neg_2 y = _2 * x";
//! eval nbfiblsd      "?lsd_neg_fib x >= 2";
//! eval nbfibmsd      "?msd_neg_fib Ex (x < 5 & y = x)";
//! eval nbclosed      "?msd_neg_2 Ax Ey (y > x)";
//! eval nbclosedfalse "?msd_neg_2 Ax (x >= 0)";
//! EOF
//! java -jar target/Walnut-all.jar negcap.txt < /dev/null
//! # stdout prints "____\nTRUE" for nbclosed and "____\nFALSE" for nbclosedfalse -- no
//! # fixture file is needed for a closed formula (the same convention
//! # `lsd_custom_base.rs`/`infinite_quantifier.rs` already use).
//! S="Session/<timestamp>/Automata Library"
//! cp "$S/nblt.txt"     .../fixtures/negative_base/less_than.txt
//! cp "$S/nbltlsd.txt"  .../fixtures/negative_base/less_than_lsd.txt
//! cp "$S/nbadd.txt"    .../fixtures/negative_base/addition_three_track.txt
//! cp "$S/nbconst.txt"  .../fixtures/negative_base/constant_minus_five.txt
//! cp "$S/nbquant.txt"  .../fixtures/negative_base/exists_negative_constant.txt
//! cp "$S/nbdiv.txt"    .../fixtures/negative_base/division_by_minus_three.txt
//! cp "$S/nbmul.txt"    .../fixtures/negative_base/times_minus_two.txt
//! cp "$S/nbfiblsd.txt" .../fixtures/negative_base/neg_fib_lsd_ge_two.txt
//! cp "$S/nbfibmsd.txt" .../fixtures/negative_base/neg_fib_msd_exists.txt
//! # …plus the three shipped custom-base files the two `neg_fib` cases need:
//! cp "Custom Bases/msd_neg_fib.txt" "Custom Bases/msd_neg_fib_addition.txt" \
//!    "Custom Bases/msd_neg_fib_less_than.txt" .../fixtures/negative_base/
//! ```
//!
//! The command file and `Session/<timestamp>/` directory were deleted from the
//! `walnut-java` checkout afterward, matching `../CAPTURE.md`'s existing recipes. The
//! three `Custom Bases/*.txt` files are Walnut's own data files, carried under the same
//! GPLv3 attribution as every other fixture copied from that repo.
//!
//! # What these tests catch (mutation-verified, not asserted)
//!
//! Each mutation below was really applied to `crates/wr-core/src/numsys.rs`, this file
//! re-run, and the mutation reverted. `C` = caught, `.` = survives.
//!
//! ```text
//!                                        M1   M2   M3   M4
//! comparator (msd)                        .    C    .    .
//! comparator (lsd)                        .    C    .    C
//! addition (three-track)                  C    .    .    .
//! constant _5                             C    .    .    .
//! exists over the negative-base adder     C    C    .    .
//! division by _3                          C    C    C    .
//! times _2                                C    .    .    .
//! neg_fib (lsd, complement fallback)      .    .    .    .
//! neg_fib (msd)                           .    .    .    .
//! closed verdicts                         .    .    .    .
//! ```
//!
//! * **M1** — drop `base_neg_n_addition`'s `1 -> 2` edge (`i==0 && j==0 && k==n-1`).
//! * **M2** — swap `base_neg_n_less_than`'s `i < j` / `j < i` arms.
//! * **M3** — flip `division`'s `n.signum() < 0` operand selection (`:1046-1048`).
//! * **M4** — drop `set_less_than_automaton`'s `if (!isMsd) reverse(...)` step, i.e. an
//!   `lsd_*` comparator that is never reversed.
//!
//! Three rows catch nothing, and each is honest rather than an oversight:
//!
//! * **Both `neg_fib` rows.** With all three `Custom Bases/msd_neg_fib*` files present,
//!   both cases are pure FILE loads — Java's `loadAutomatonOrNull` returns before the
//!   programmatic fallback, and its own reverse (for the `lsd` direction) lives inside
//!   `CustomBaseCandidates::resolve`, not in the `if (!isMsd)` step M4 breaks. So no
//!   mutation to a programmatic negative-base construction can reach them by
//!   construction. They are here to pin the file-backed path itself, and to pin that the
//!   two directions resolve to genuinely DIFFERENT automata rather than one silently
//!   being reused for the other. (A mutation inside `resolve` would catch the `lsd` one —
//!   `lsd_custom_base.rs` already keeps that mutation in its own matrix, over `lsd_fib`.)
//! * **The closed verdicts.** A closed formula collapses to one bit, and `Ax Ey (y > x)`
//!   stays TRUE / `Ax (x >= 0)` stays FALSE under all four mutations. Kept as cheap
//!   both-polarity coverage of the `¬∃¬` dispatch over a negative base — the
//!   discriminating `∀`/`∃` evidence is the `exists_over_the_negative_base_adder` row.
//!
//! # Normalizations, both inherited from `lsd_custom_base.rs` and both load-bearing
//!
//! * **`sort_label()`** — a captured `.txt` went through Java's `writeToTxtFormat` ->
//!   `canonize` -> `sortLabel`, so its tracks are in alphabetical label order.
//!   `wr_core::equiv::automaton_language_equivalent` does not detect a label permutation
//!   whose per-position alphabets match (U8's documented limitation), which would matter
//!   for the three-track addition case.
//! * **`totalize(0)`** — real Walnut's automata for free-variable predicates are partial
//!   (a missing transition is an implicit rejection).

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::eval_def::{eval_def_command_with_stdout, EvalDefError};
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;
use wr_logic::predicate_env::FreshIdentifiers;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/negative_base")
}

/// A process-scoped directory laid out as a full Walnut home tree, seeded with the three
/// shipped `msd_neg_fib*` custom-base files — and *only* those, never an `lsd_neg_fib*`,
/// because real Walnut ships none either (see this file's module docs).
fn temp_session(tag: &str) -> (Session, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-negative-base-{tag}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    for sub in [
        "Result",
        "Automata Library",
        "Word Automata Library",
        "Custom Bases",
        "Macro Library",
        "Morphism Library",
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    for name in [
        "msd_neg_fib.txt",
        "msd_neg_fib_addition.txt",
        "msd_neg_fib_less_than.txt",
    ] {
        fs::copy(
            fixtures_dir().join(name),
            dir.join("Custom Bases").join(name),
        )
        .unwrap_or_else(|e| panic!("must be able to install fixtures/negative_base/{name}: {e}"));
    }
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    (session, dir)
}

fn run_eval(session: &Session, predicate: &str, name: &str) -> Result<Automaton, EvalDefError> {
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut fresh = FreshIdentifiers::new();
    let mut stdout = Vec::new();
    let tc = eval_def_command_with_stdout(
        session,
        &mut logging,
        &mut fresh,
        false,
        false,
        predicate,
        Some(name),
        None,
        &mut stdout,
    )?;
    Ok(tc.automaton_pairs()[0].automaton().unwrap().clone())
}

/// Compares `ours` against the captured `walnut-java` fixture by semantic language
/// equivalence, plus three structural assertions a language-only comparison would miss:
/// the alphabet on both sides, the msd/lsd direction on every track, and — the one that
/// is specific to a negative base — the per-track NUMBER SYSTEM NAME.
///
/// That last one is not decoration. `msd_neg_2`'s alphabet is `{0, 1}`, byte-identical to
/// `msd_2`'s, so a result that silently fell back to the positive base would have the
/// right alphabet, the right direction, and — for a query whose language happens to
/// coincide — could pass everything else here. Java writes the real name into the header
/// (`AutomatonWriter.writeAlphabet` reads `NumberSystem.getName()`), so the fixture is a
/// genuine oracle for it.
fn compare_to_fixture(
    mut ours: Automaton,
    custom_bases_dir: &Path,
    fixture: &str,
    expected_alphabet: &[Vec<i32>],
    expected_ns_name: &str,
    what: &str,
) {
    let path = fixtures_dir().join(fixture);
    let mut ground_truth: Automaton =
        wr_io::reader::read_automaton_txt_with_custom_bases(&path, custom_bases_dir)
            .unwrap_or_else(|e| panic!("{what}: fixture {fixture} must parse cleanly: {e:?}"));

    ours.sort_label();
    ours.fa.totalize(0);
    ground_truth.fa.totalize(0);

    let tracks = expected_alphabet.len();
    let is_msd = expected_ns_name.starts_with("msd");
    assert_eq!(ground_truth.alphabet, expected_alphabet, "{what}: fixture");
    assert_eq!(ours.alphabet, expected_alphabet, "{what}: ours");
    assert_eq!(
        ground_truth.msd,
        vec![Some(is_msd); tracks],
        "{what}: the fixture's header must actually say {expected_ns_name}"
    );
    assert_eq!(
        ours.msd,
        vec![Some(is_msd); tracks],
        "{what}: the port must keep every surviving track's direction"
    );
    let want_names = vec![Some(expected_ns_name.to_string()); tracks];
    assert_eq!(
        ground_truth.track_ns_names(),
        want_names,
        "{what}: the fixture's tracks must carry the NEGATIVE base's name"
    );
    assert_eq!(
        ours.track_ns_names(),
        want_names,
        "{what}: the port's tracks must carry the NEGATIVE base's name"
    );
    assert!(
        automaton_language_equivalent(&ours, &ground_truth)
            .expect("both sides are total DFAs after totalize"),
        "{what}: diverges from real walnut-java"
    );
}

/// `?msd_neg_2 x < y` — `baseNegNLessThan` itself, the construction that has no
/// positive-base analogue at all (a negative base's order is NOT lexicographic).
#[test]
fn comparator_msd() {
    let (session, dir) = temp_session("lt");
    let ours = run_eval(&session, "?msd_neg_2 x < y", "nblt").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "less_than.txt",
        &[vec![0, 1], vec![0, 1]],
        "msd_neg_2",
        "msd_neg_2 comparator",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?lsd_neg_2 x < y` — the same comparator after `setLessThanAutomaton`'s
/// `if (!isMsd) reverse(...)` step, which sits inside the no-file-found branch and so
/// applies to the negative-base construction too.
#[test]
fn comparator_lsd() {
    let (session, dir) = temp_session("ltlsd");
    let ours = run_eval(&session, "?lsd_neg_2 x < y", "nbltlsd").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "less_than_lsd.txt",
        &[vec![0, 1], vec![0, 1]],
        "lsd_neg_2",
        "lsd_neg_2 comparator",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_neg_2 x + y = z` — `baseNegNAddition`'s three-state carry table, three tracks,
/// no quantifier. The case most sensitive to a wrong transition.
#[test]
fn addition_three_track() {
    let (session, dir) = temp_session("add");
    let ours = run_eval(&session, "?msd_neg_2 x + y = z", "nbadd").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "addition_three_track.txt",
        &[vec![0, 1], vec![0, 1], vec![0, 1]],
        "msd_neg_2",
        "msd_neg_2 addition",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_neg_2 x = _5` — a NEGATIVE constant, which is the whole point of `validateNeg`'s
/// restored `!isNeg` conjunct plus `constant`'s `n.signum() < 0` arm. Unrepresentable in
/// a positive base (real Walnut throws "negative constant -5" there).
#[test]
fn negative_constant() {
    let (session, dir) = temp_session("const");
    let ours = run_eval(&session, "?msd_neg_2 x = _5", "nbconst").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "constant_minus_five.txt",
        &[vec![0, 1]],
        "msd_neg_2",
        "msd_neg_2 constant -5",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_neg_3 Ex (x + x = y & y < 5)` — a genuine quantifier elimination composed on top
/// of the negative-base adder AND comparator, over a base whose alphabet (`{0,1,2}`) is
/// not `{0,1}`. The result is the 8-state "y is even and y < 5" automaton, deliberately
/// NOT a trivial one: an earlier draft of this test used `Ex (x + y = _1)`, whose answer
/// is "every y" (a negative base has an additive inverse for everything), and which
/// therefore survived every mutation in the matrix above.
#[test]
fn exists_over_the_negative_base_adder() {
    let (session, dir) = temp_session("quant");
    let ours = run_eval(&session, "?msd_neg_3 Ex (x + x = y & y < 5)", "nbquant").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "exists_negative_constant.txt",
        &[vec![0, 1, 2]],
        "msd_neg_3",
        "msd_neg_3 exists",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_neg_2 y = x / _3` — `division`'s restored `n.signum() < 0` operand selection
/// (`n < r <= 0` instead of `0 <= r < n`), which is the only thing that arm changes.
#[test]
fn division_by_a_negative_constant() {
    let (session, dir) = temp_session("div");
    let ours = run_eval(&session, "?msd_neg_2 y = x / _3", "nbdiv").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "division_by_minus_three.txt",
        &[vec![0, 1], vec![0, 1]],
        "msd_neg_2",
        "msd_neg_2 division by -3",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_neg_2 y = _2 * x` — `multiplication`'s restored `n.signum() < 0` arm
/// (`Ec, b + c = 0 & c = (-n)*a`).
#[test]
fn multiplication_by_a_negative_constant() {
    let (session, dir) = temp_session("mul");
    let ours = run_eval(&session, "?msd_neg_2 y = _2 * x", "nbmul").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "times_minus_two.txt",
        &[vec![0, 1], vec![0, 1]],
        "msd_neg_2",
        "msd_neg_2 times -2",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?lsd_neg_fib x >= 2` — the two mechanisms stacked: a FILE-backed negative base, in the
/// direction for which no file is shipped, so `loadAutomatonOrNull` must find
/// `msd_neg_fib_addition.txt`/`msd_neg_fib_less_than.txt` and reverse them.
#[test]
fn neg_fib_lsd_resolves_through_the_complement_fallback() {
    let (session, dir) = temp_session("fiblsd");
    let ours = run_eval(&session, "?lsd_neg_fib x >= 2", "nbfiblsd").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "neg_fib_lsd_ge_two.txt",
        &[vec![0, 1]],
        "lsd_neg_fib",
        "lsd_neg_fib x >= 2",
    );
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_neg_fib Ex (x < 5 & y = x)` — the same base in the direction its files ARE
/// shipped for, with a quantifier on top. Together with the test above this also pins
/// that the two directions produce genuinely different automata rather than one being
/// silently reused for the other.
#[test]
fn neg_fib_msd_loads_from_its_own_files() {
    let (session, dir) = temp_session("fibmsd");
    let ours = run_eval(&session, "?msd_neg_fib Ex (x < 5 & y = x)", "nbfibmsd").unwrap();
    compare_to_fixture(
        ours,
        &dir.join("Custom Bases"),
        "neg_fib_msd_exists.txt",
        &[vec![0, 1]],
        "msd_neg_fib",
        "msd_neg_fib exists",
    );
    fs::remove_dir_all(&dir).ok();
}

/// Both closed-formula polarities over a negative base. Real `walnut-java` prints
/// `TRUE` for the first and `FALSE` for the second — the latter only because a negative
/// base genuinely contains negative numbers, so `Ax (x >= 0)` is false there where it is
/// vacuously true in every positive base this port supports.
#[test]
fn closed_formulae_over_a_negative_base() {
    let (session, dir) = temp_session("closed");
    let t = run_eval(&session, "?msd_neg_2 Ax Ey (y > x)", "nbclosed").unwrap();
    assert_eq!(t.fa.true_false, Some(true), "Ax Ey (y > x) must be TRUE");
    let f = run_eval(&session, "?msd_neg_2 Ax (x >= 0)", "nbclosedfalse").unwrap();
    assert_eq!(
        f.fa.true_false,
        Some(false),
        "Ax (x >= 0) must be FALSE in a negative base"
    );
    fs::remove_dir_all(&dir).ok();
}
