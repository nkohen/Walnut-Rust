// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **`lsd_k` numeration**, spot-checked against real
//! `walnut-java` CLI output.
//!
//! # Why this file exists
//!
//! Until Phase 3b's L1, `wr_core::quantify`'s lsd arm returned a hard
//! `QuantifyError::UnsupportedLsdFixup` instead of calling
//! `AutomatonLogicalOps.fixTrailingZerosProblem` — a Phase-2 scope cut. The blast radius
//! was much wider than "an `lsd_k` query with an explicit `E`/`A`/`I`": `wr_core::numsys`
//! calls `quantify` to build its *own* automata (`getConstant(n)` for `n >= 2`,
//! comparison/arithmetic against a constant, multiplication, division), so `?lsd_2 x >= 2`
//! — a query containing no user-written quantifier at all — failed too. Roughly all of
//! `lsd_k` beyond `x < y` / `a + b = c` was unreachable, and the port had **zero**
//! positive `lsd_k` end-to-end coverage: every existing test pinned the rejection.
//!
//! L1 wired the branch up. These are the differential tests that were missing, in the same
//! shape as `u11_eval_composition.rs`'s msd ones and captured the same one-time way (see
//! the recipe below and `../CAPTURE.md`) — no live JVM shellout from the test.
//!
//! Seven cases, chosen so that each one fails for a *different* reason if the fixup is
//! wired up wrongly:
//!
//! * `lsdge2` — `x >= 2`, **no user-written quantifier**. Exercises exactly the
//!   `NumberSystem`-internal `quantify` call (`comparison(String, BigInteger, ...)` binds
//!   the constant to a fresh name and quantifies it away). This is the case that proves
//!   the gap was never really about explicit quantifiers.
//! * `lsdquant` — `Ex (x < 5 & x >= 2 & y = x)`, a genuine `∃` over an `lsd` variable
//!   composed with two constant comparisons, leaving one free variable.
//! * `lsdmult` — `y = 3*x`, the multiplication family (recursive doubling, two nested
//!   `quantify` calls per level).
//! * `lsd3addcmp` — `x + y = z & z < 4` over **`lsd_3`**, so the base is not 2 and the
//!   case is three-track (a digit-order or per-track-alphabet bug that base 2 hides
//!   shows up here).
//! * `lsdclosed`/`lsdclosedtrue` — closed `∀` formulae over `lsd`, i.e. the `¬∃¬` path,
//!   checked against real Walnut's printed `FALSE`/`TRUE` verdicts rather than a fixture
//!   file (a trivial automaton has no meaningful `.txt` body — same convention as
//!   `u11_eval_composition.rs`'s `quantifier_elimination_composition_matches_real_walnut_verdict`).
//! * The last two go through **`wr-cli`'s real `eval`/`def`**, not just
//!   `wr_logic::eval::evaluate`, so the `.txt` writer's `lsd_2` header and the reader's
//!   parse of it are in the loop as well — `eval`/`def` over `lsd_*` is the user-stated
//!   requirement, so it is checked directly rather than inferred. `lsdusedef` additionally
//!   reuses a `def`'d lsd automaton as a `$token`. **`reg` is not exercised here** — it
//!   doesn't route through `wr_core::quantify` at all (Thompson construction +
//!   determinize, no quantifier elimination), so it was never affected by this fix; its
//!   own, pre-existing `lsd` coverage lives in `reg_and_alphabet_commands.rs`'s
//!   `"msd_2 lsd_2 "` alphabet case.
//!
//! ## Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! Two command-file runs (`def` must land in `Automata Library/` before the query that
//! references it — a single file would work too, but the two were captured separately):
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Command Files/lsd_capture.txt" <<'EOF'
//! eval lsdge2 "?lsd_2 x >= 2";
//! eval lsdquant "?lsd_2 Ex (x < 5 & x >= 2 & y = x)";
//! eval lsdmult "?lsd_2 y = 3*x";
//! eval lsdclosed "?lsd_2 Ax (x >= 5)";
//! eval lsdclosedtrue "?lsd_2 Ax Ey (y > x)";
//! eval lsd3addcmp "?lsd_3 x + y = z & z < 4";
//! EOF
//! java -jar target/Walnut-all.jar lsd_capture.txt < /dev/null
//! # stdout prints "____\nFALSE" for lsdclosed and "____\nTRUE" for lsdclosedtrue --
//! # no fixture files needed for those two, see above.
//! S="Session/<timestamp>/Automata Library"
//! cp "$S/lsdge2.txt"     .../fixtures/lsd/ge_two.txt
//! cp "$S/lsdquant.txt"   .../fixtures/lsd/exists_quantified.txt
//! cp "$S/lsdmult.txt"    .../fixtures/lsd/mult_two_track.txt
//! cp "$S/lsd3addcmp.txt" .../fixtures/lsd/base3_addition_and_compare.txt
//!
//! cat > "Command Files/lsd_capture2.txt" <<'EOF'
//! def lsdge2d "?lsd_2 x >= 2";
//! eval lsdusedef "?lsd_2 $lsdge2d(y) & y < 5";
//! EOF
//! java -jar target/Walnut-all.jar lsd_capture2.txt < /dev/null
//! cp "Session/<timestamp2>/Automata Library/lsdusedef.txt" \
//!    .../fixtures/lsd/def_then_reuse.txt
//! ```
//!
//! The command files and `Session/<timestamp>/` directories were deleted from the
//! `walnut-java` checkout afterward, matching `../CAPTURE.md`'s existing recipes.
//!
//! ## Two normalizations, both load-bearing (inherited from `u11_eval_composition.rs`)
//!
//! * **`sort_label()`** — every captured `.txt` went through Java's
//!   `AutomatonWriter.writeToTxtFormat` -> `canonize` -> `sortLabel`, so its tracks are in
//!   alphabetical label order; the port returns whatever order the pipeline produced.
//!   `wr_core::equiv::automaton_language_equivalent` does **not** detect a label
//!   permutation whose per-position alphabets match (U8's documented limitation — it
//!   returns a silently WRONG verdict), so without this the three-track `lsd_3` case would
//!   read as a decision-procedure bug.
//! * **`totalize(0)`** — real Walnut's automata for free-variable predicates are partial
//!   (a missing transition is an implicit rejection).

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::eval_def::{eval_def_command_with_stdout, EvalDefError};
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;
use wr_logic::predicate_env::{FreshIdentifiers, InMemoryPredicateEnv};

/// Evaluates `predicate` through the Rust pipeline and compares it, by semantic language
/// equivalence, against the captured `walnut-java` fixture at `fixtures/lsd/<fixture>`.
fn compare_to_fixture(predicate: &str, fixture: &str, expected_alphabet: &[Vec<i32>]) {
    let env = InMemoryPredicateEnv::new();
    let mut ours = wr_logic::eval::evaluate(&env, predicate)
        .expect("the Rust pipeline must evaluate this lsd predicate end-to-end");
    ours.sort_label();
    compare_automaton_to_fixture(ours, fixture, expected_alphabet, predicate);
}

/// The comparison half of [`compare_to_fixture`], split out so the `wr-cli`-level tests
/// below can reuse it against an already-computed automaton.
///
/// The `msd` vectors are asserted on both sides, not only the languages: an `lsd` fixture
/// whose header had been parsed as `msd` (or a port result that silently lost the
/// direction during projection) would otherwise still compare equivalent whenever the
/// language happens to be reversal-symmetric, which several small automata are.
fn compare_automaton_to_fixture(
    mut ours: Automaton,
    fixture: &str,
    expected_alphabet: &[Vec<i32>],
    what: &str,
) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/lsd")
        .join(fixture);
    let mut ground_truth: Automaton =
        wr_io::reader::read_automaton_txt(&path).expect("fixture must parse cleanly");

    ours.fa.totalize(0);
    ground_truth.fa.totalize(0);

    let tracks = expected_alphabet.len();
    assert_eq!(ground_truth.alphabet, expected_alphabet);
    assert_eq!(ours.alphabet, expected_alphabet);
    assert_eq!(
        ground_truth.msd,
        vec![Some(false); tracks],
        "the fixture's header must actually say lsd on every track"
    );
    assert_eq!(
        ours.msd,
        vec![Some(false); tracks],
        "the port must keep every surviving track marked lsd"
    );
    assert_eq!(
        automaton_language_equivalent(&ours, &ground_truth),
        Ok(true),
        "the Rust pipeline's result for `{what}` must be language-equivalent to real \
         walnut-java's output (see this file's module docs for the capture)"
    );
}

/// `?lsd_2 x >= 2` — the headline case. No user-written quantifier: the only `quantify`
/// call is `NumberSystem::comparison_const_b`'s own, which is why this query failed with
/// `UnsupportedLsdFixup` before L1.
#[test]
fn lsd_comparison_against_a_constant_matches_real_walnut_output() {
    compare_to_fixture("?lsd_2 x >= 2", "ge_two.txt", &[vec![0, 1]]);
}

/// `?lsd_2 Ex (x < 5 & x >= 2 & y = x)` — a real `∃` over an `lsd` variable, one free
/// variable surviving. Language: `y in {2, 3, 4}`.
#[test]
fn lsd_existential_quantification_matches_real_walnut_output() {
    compare_to_fixture(
        "?lsd_2 Ex (x < 5 & x >= 2 & y = x)",
        "exists_quantified.txt",
        &[vec![0, 1]],
    );
}

/// `?lsd_2 y = 3*x` — the multiplication family, built by recursive doubling with a
/// `quantify` call per level.
#[test]
fn lsd_multiplication_matches_real_walnut_output() {
    compare_to_fixture(
        "?lsd_2 y = 3*x",
        "mult_two_track.txt",
        &[vec![0, 1], vec![0, 1]],
    );
}

/// `?lsd_3 x + y = z & z < 4` — **base 3, three tracks.** The case that catches a
/// per-track alphabet or digit-order bug that base 2 (where a digit and its index
/// coincide, and where `0`/`1` are the only symbols) can hide.
#[test]
fn lsd_base_three_addition_and_comparison_matches_real_walnut_output() {
    compare_to_fixture(
        "?lsd_3 x + y = z & z < 4",
        "base3_addition_and_compare.txt",
        &[vec![0, 1, 2], vec![0, 1, 2], vec![0, 1, 2]],
    );
}

/// The closed-formula (`∀`, i.e. `¬∃¬`) path over `lsd`, against real Walnut's printed
/// verdicts. Both polarities, because `FALSE` is what a broken `¬∃¬` chain produces most
/// easily — a suite that only ever asserts `FALSE` proves very little.
#[test]
fn lsd_closed_formulae_match_real_walnut_verdicts() {
    let env = InMemoryPredicateEnv::new();

    // walnut-java: `eval lsdclosed "?lsd_2 Ax (x >= 5)";` prints "____\nFALSE".
    let ours = wr_logic::eval::evaluate(&env, "?lsd_2 Ax (x >= 5)")
        .expect("the Rust pipeline must evaluate this lsd predicate end-to-end");
    assert!(ours.fa.is_true_false_automaton());
    assert!(
        !ours.fa.is_true_automaton(),
        "must match real walnut-java's printed FALSE verdict"
    );

    // walnut-java: `eval lsdclosedtrue "?lsd_2 Ax Ey (y > x)";` prints "____\nTRUE".
    let ours = wr_logic::eval::evaluate(&env, "?lsd_2 Ax Ey (y > x)")
        .expect("the Rust pipeline must evaluate this lsd predicate end-to-end");
    assert!(ours.fa.is_true_false_automaton());
    assert!(
        ours.fa.is_true_automaton(),
        "must match real walnut-java's printed TRUE verdict"
    );
}

// ---------------------------------------------------------------------------
// The same thing through `wr-cli`'s real `eval`/`def`, not just `wr_logic::eval`
// ---------------------------------------------------------------------------
//
// The cases above call `wr_logic::eval::evaluate` directly, which is the engine but not
// the whole user-facing path: `eval NAME "..."` additionally writes the result through
// `wr-io`'s writer, and `def NAME "..."` makes it re-readable as a `$NAME(...)` token in a
// later query. Both of those steps carry the per-track `msd`/`lsd` marking across a `.txt`
// round-trip, so they are exactly where an lsd-specific regression could still hide after
// `quantify` itself is correct. `eval`/`def` over `lsd_*` is the user-stated requirement
// this whole unit exists to serve, so it gets direct coverage rather than being inferred
// from the library-level cases. (`reg` is out of scope for this file -- see this file's
// module docs.)

/// Same scaffolding convention as `phase3a_checkpoint.rs`/`reg_and_alphabet_commands.rs`:
/// a process-scoped directory laid out as a full Walnut home tree.
fn temp_session(tag: &str) -> (Session, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("wr-differential-lsd-{tag}-{}", std::process::id()));
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
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    (session, dir)
}

fn run_eval_or_def(
    session: &Session,
    predicate: &str,
    name: &str,
    is_def: bool,
) -> Result<Automaton, EvalDefError> {
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut fresh = FreshIdentifiers::new();
    let mut stdout = Vec::new();
    let tc = eval_def_command_with_stdout(
        session,
        &mut logging,
        &mut fresh,
        is_def,
        false,
        predicate,
        Some(name),
        None,
        &mut stdout,
    )?;
    Ok(tc.automaton_pairs()[0].automaton().unwrap().clone())
}

/// `eval lsdge2 "?lsd_2 x >= 2";` through `wr-cli` — and then the `.txt` file it wrote is
/// read back and compared to real Walnut's, so the writer's `lsd_2` header and the
/// reader's parse of it are both in the loop, not just the decision procedure.
#[test]
fn lsd_eval_command_writes_a_file_that_matches_real_walnut_output() {
    let (session, dir) = temp_session("eval");

    let returned = run_eval_or_def(&session, "?lsd_2 x >= 2", "lsdge2", false)
        .expect("`eval` over lsd_2 must succeed through wr-cli");
    compare_automaton_to_fixture(returned, "ge_two.txt", &[vec![0, 1]], "eval lsdge2");

    // ... and the same automaton after a full `.txt` round trip through this port's own
    // writer, which is what a later `def`/`$token` reference or a research script reads.
    let written = dir.join("Result").join("lsdge2.txt");
    let round_tripped = wr_io::reader::read_automaton_txt(&written)
        .expect("the file `eval` wrote must parse back cleanly");
    compare_automaton_to_fixture(
        round_tripped,
        "ge_two.txt",
        &[vec![0, 1]],
        "eval lsdge2, after a .txt round trip",
    );

    fs::remove_dir_all(&dir).ok();
}

/// `def lsdge2d "?lsd_2 x >= 2";` followed by
/// `eval lsdusedef "?lsd_2 $lsdge2d(y) & y < 5";` — the `def`-then-reuse path, where the
/// saved automaton is re-read from `Automata Library/` as a `Function` token and combined
/// with a fresh lsd comparison. Language: `y in {2, 3, 4}`.
#[test]
fn lsd_def_then_reuse_matches_real_walnut_output() {
    let (session, dir) = temp_session("def");

    run_eval_or_def(&session, "?lsd_2 x >= 2", "lsdge2d", true)
        .expect("`def` over lsd_2 must succeed through wr-cli");
    let ours = run_eval_or_def(&session, "?lsd_2 $lsdge2d(y) & y < 5", "lsdusedef", false)
        .expect("reusing a def'd lsd automaton must succeed");

    compare_automaton_to_fixture(
        ours,
        "def_then_reuse.txt",
        &[vec![0, 1]],
        "def lsdge2d; eval lsdusedef",
    );

    fs::remove_dir_all(&dir).ok();
}
