// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for a **real custom base's `lsd` direction** (`lsd_fib`, i.e.
//! Zeckendorf representation read least-significant-digit-first), spot-checked against real
//! `walnut-java` CLI output — `docs/BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md` item 2.
//!
//! # Why this file exists
//!
//! **Correction (adversarial review):** an earlier draft of this module doc claimed the
//! composition below had "never been exercised". That overclaimed. Real Java's own Tier-1
//! integration-test corpus already exercises `∃` and open `∀` over `lsd_fib` extensively
//! and correctly — e.g. `?lsd_fib Ex,y x < y` (fixture 65), `?lsd_fib Eb a < b & b = 4`
//! (fixture 112), `?lsd_fib Ax y < x+5` (fixture 135) — all subset-relevant, all currently
//! passing through `tests/golden`'s Tier-1 replay (`phase0-artifacts/test-manifest.json`
//! has 44 fixtures referencing `lsd_fib`). What is actually true, and is this file's real
//! reason to exist:
//!
//! * That coverage is **gated-slow** (`tests/golden` is `#[ignore]`d, run only on demand —
//!   DESIGN.md §5's Tier 1, not the fast tier every commit runs).
//! * It has **zero coverage of the `I` quantifier** over `lsd_fib` (no `Ix …` fixture
//!   exists over any custom base) and **zero coverage of `def`-then-`$token` reuse** over
//!   `lsd_fib` (no `def` command over `lsd_fib` exists in the manifest at all) — both
//!   genuinely new here.
//! * It never explicitly checks the `.txt` round trip through `Result/` the way this file's
//!   `comparison_against_a_constant` test does.
//!
//! Two neighbouring things were also already verified, and neither of them is this one:
//!
//! * **`lsd_k` (plain bases) end-to-end** — Phase 3b's L1, covered by
//!   `lsd_numeration.rs` (`∃`, closed `∀`) and by `infinite_quantifier.rs` (`I`). Every
//!   case there is `lsd_2`/`lsd_3`, i.e. a base this crate constructs *programmatically*
//!   (`wr_core::numsys::base_n_addition_automaton`/`lexicographic_less_than`).
//! * **Custom-base `lsd` CONSTRUCTION** — `crates/wr-core/src/numsys.rs`'s
//!   `lsd_custom_base_built_from_the_msd_files_reverses_exactly_once`, which pins that a
//!   base shipped only with `msd_*` files builds a correct `lsd_*` adder by reversing the
//!   opposite-direction complement candidate exactly once
//!   (`NumberSystem.java`'s `loadAutomatonOrNull` fallback, ported as
//!   `wr_core::numsys::custom_base_candidate_names` + `set_addition_automaton`).
//!
//! So this file's job is: give the fast/differential tier its OWN direct coverage of an
//! `lsd` custom base composing with quantifier elimination through `eval`/`def` — not
//! introduce that composition for the first time — and close the two real gaps (`I`,
//! `def`-then-reuse) the gated-slow corpus leaves open. Three things could go wrong in
//! this composition that a plain `lsd_k` test can't see (all of which the golden corpus's
//! passing `lsd_fib` fixtures already give indirect, gated-slow evidence against, and
//! which the tests below re-confirm directly):
//!
//! 1. `quantify`'s lsd arm (`fixTrailingZerosProblem`) running against an alphabet whose
//!    words are restricted by `all_reps` — an lsd-`fib` word may not contain `11`, so the
//!    trailing-zero right-quotient interacts with a restriction `lsd_2` never imposes.
//! 2. The reversed adder being reversed a second time (or zero times) somewhere downstream
//!    of construction, which `lsd_custom_base_built_from_the_msd_files_reverses_exactly_once`
//!    cannot see because it never leaves `NumberSystem`.
//! 3. The `.txt` round trip: `eval` writes an `lsd_fib` header, and a later `def`/`$token`
//!    reference reads it back through `wr-io`'s custom-base resolver, which must re-run the
//!    *same* opposite-direction fallback. A base name that survives in memory but is
//!    written or re-parsed as `msd_fib` (or as the `lsd_2` its alphabet cardinality alone
//!    suggests) would be invisible to every in-memory test.
//!
//! **Result: no divergence.** All seven cases below match real `walnut-java` exactly, on
//! the first run, with no production change of any kind. The deliverable is this coverage.
//!
//! ## Every case goes through `wr-cli`'s real `eval`/`def`, not `wr_logic::eval::evaluate`
//!
//! Unlike `lsd_numeration.rs`, which can use `InMemoryPredicateEnv` for its library-level
//! cases, there is no in-memory option here: resolving `lsd_fib` *requires* file access to
//! `Custom Bases/`, so every test builds a real [`Session`] over a temp Walnut home tree
//! with `msd_fib.txt`/`msd_fib_addition.txt` installed. That is also the point — see (3)
//! above.
//!
//! Note that `Custom Bases/` is deliberately given **only** the `msd_fib` files, never an
//! `lsd_fib*.txt`: real Walnut ships no `lsd_fib` files either (check
//! `walnut-java`'s `Custom Bases/` — it is `msd_*` throughout), so the
//! opposite-direction-complement fallback is the only way `?lsd_fib` resolves at all, on
//! either engine. A test that hand-wrote `lsd_fib_addition.txt` would silently stop
//! exercising the mechanism this file exists to check.
//!
//! ## Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Command Files/lsdfib_capture.txt" <<'EOF'
//! eval lfge2 "?lsd_fib x >= 2";
//! eval lfquant "?lsd_fib Ex (x < 5 & x >= 2 & y = x)";
//! eval lfclosed "?lsd_fib Ax (x >= 5)";
//! eval lfclosedtrue "?lsd_fib Ax Ey (y > x)";
//! eval lfinffalse "?lsd_fib Ix x < 5";
//! eval lfinftrue "?lsd_fib Ix x >= 5";
//! eval lfadd "?lsd_fib x + y = z & z < 4";
//! eval lfforall "?lsd_fib Ay (y < 3 => y < x)";
//! EOF
//! java -jar target/Walnut-all.jar lsdfib_capture.txt < /dev/null
//! # stdout prints "____\nFALSE" for lfclosed and lfinffalse, "____\nTRUE" for
//! # lfclosedtrue and lfinftrue -- no fixture files needed for those four (an `I`- or
//! # closed-`A`-quantified result is always a trivial automaton with no meaningful body,
//! # the same convention `lsd_numeration.rs`/`infinite_quantifier.rs` already use).
//! S="Session/<timestamp>/Automata Library"
//! cp "$S/lfge2.txt"  .../fixtures/lsd_custom_base/ge_two.txt
//! cp "$S/lfquant.txt" .../fixtures/lsd_custom_base/exists_quantified.txt
//! cp "$S/lfadd.txt"  .../fixtures/lsd_custom_base/addition_three_track.txt
//! cp "$S/lfforall.txt" .../fixtures/lsd_custom_base/forall_open.txt
//!
//! cat > "Command Files/lsdfib_capture2.txt" <<'EOF'
//! def lfge2d "?lsd_fib x >= 2";
//! eval lfusedef "?lsd_fib $lfge2d(y) & y < 5";
//! EOF
//! java -jar target/Walnut-all.jar lsdfib_capture2.txt < /dev/null
//! cp "Session/<timestamp2>/Automata Library/lfusedef.txt" \
//!    .../fixtures/lsd_custom_base/def_then_reuse.txt
//! ```
//!
//! The command files and `Session/<timestamp>/` directories were deleted from the
//! `walnut-java` checkout afterward, matching `../CAPTURE.md`'s existing recipes.
//!
//! ## What these tests actually catch (mutation-verified, not asserted)
//!
//! Every claim below was checked by breaking the named primitive, re-running this file,
//! and restoring it — `CLAUDE.md`'s "mutation-verify anything you claim a regression test
//! catches" rule. `C` = caught (test fails), `.` = survives:
//!
//! ```text
//!                                            M1   M2   M3
//! comparison_against_a_constant               C    C    .
//! existential_quantification                  C    C    .
//! addition (three-track)                      C    C    .
//! open_universal_quantification               C    C    .
//! def_then_reuse                              C    C    .
//! infinite_quantifier                         C    C    C
//! closed_formulae (verdicts only)             .    .    .
//! ```
//!
//! * **M1** — `wr_core::numsys::CustomBaseCandidates::resolve` returns the
//!   opposite-direction complement *without* reversing it (i.e. the custom base's
//!   reversed-adder fallback is silently a no-op). In the FAST tier, nothing outside this
//!   file and `numsys.rs`'s own construction unit test can see this — the gated-slow
//!   golden corpus's ~44 `lsd_fib` fixtures would also catch it (they exercise the same
//!   reversed adder), but that tier is `#[ignore]`d, not run every commit.
//! * **M2** — `wr_core::quantify`'s `Some(false)` arm calls
//!   `fix_leading_zeros_problem_with_ctx` instead of `fix_trailing_zeros_problem`, i.e.
//!   the msd fixup applied to an lsd system. This is the exact shape of the pre-L1 gap,
//!   here over a custom base rather than `lsd_k`.
//! * **M3** — `wr_core::logicalops::remove_leading_zeros_helper`'s `if !msd { reverse… }`
//!   branch never fires, i.e. the `I` quantifier's own direction handling. Only the `I`
//!   test sees this, which is why it is here at all: it is a genuinely different code
//!   path from `quantify` (see `wr_core::infinite`'s module docs).
//!
//! The one row that catches nothing is honest, not an oversight: a *closed* formula
//! collapses to one bit, and `x >= 5` / `Ey (y > x)` stay cofinite under all three
//! mutations. It is kept as cheap both-polarity coverage of the `¬∃¬` dispatch itself;
//! the discriminating `∀` evidence is the `open_universal_quantification` row, which
//! compares a real automaton.
//!
//! ## Normalizations, both inherited from `lsd_numeration.rs` and both load-bearing
//!
//! * **`sort_label()`** — a captured `.txt` went through Java's
//!   `AutomatonWriter.writeToTxtFormat` -> `canonize` -> `sortLabel`, so its tracks are in
//!   alphabetical label order. `wr_core::equiv::automaton_language_equivalent` does not
//!   detect a label permutation whose per-position alphabets match (U8's documented
//!   limitation — it returns a silently WRONG verdict), which would matter for the
//!   three-track addition case.
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

/// Same scaffolding convention as `lsd_numeration.rs`/`infinite_quantifier.rs`: a
/// process-scoped directory laid out as a full Walnut home tree, additionally seeded with
/// the real `msd_fib` custom-base files (and *only* those — see this file's module docs).
///
/// The two files are read from this crate's existing `fixtures/phase3a_checkpoint/` copies
/// rather than duplicated a third time: `crates/wr-cli/tests/fixtures/ATTRIBUTION.md`'s
/// convention is one copy per *crate*, and `tests/differential` already has its pair.
fn temp_session(tag: &str) -> (Session, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-lsd-custom-base-{tag}-{}",
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

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/phase3a_checkpoint");
    for name in ["msd_fib.txt", "msd_fib_addition.txt"] {
        fs::copy(src.join(name), dir.join("Custom Bases").join(name)).unwrap_or_else(|e| {
            panic!("must be able to install fixtures/phase3a_checkpoint/{name}: {e}")
        });
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

/// Compares `ours` against the captured `walnut-java` fixture at
/// `fixtures/lsd_custom_base/<fixture>` by semantic language equivalence.
///
/// Three structural assertions ride along, each of which a *language*-only comparison
/// would miss:
///
/// * the alphabet, on both sides;
/// * `msd == [Some(false); tracks]`, i.e. the header really says `lsd` — several small
///   automata here have reversal-symmetric languages, so a direction lost during
///   projection would otherwise still compare equivalent;
/// * `track_ns_names() == ["lsd_fib"; tracks]` — the assertion that is specific to THIS
///   file. `lsd_fib`'s alphabet is `{0, 1}`, exactly `lsd_2`'s, so a result that silently
///   fell back to the programmatic base-2 number system would have the right alphabet, the
///   right direction, and (for a query like `x >= 2`) a *different* language — but for a
///   query whose language happened to coincide it would pass everything else here.
fn compare_to_fixture(
    mut ours: Automaton,
    custom_bases_dir: &Path,
    fixture: &str,
    expected_alphabet: &[Vec<i32>],
    what: &str,
) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/lsd_custom_base")
        .join(fixture);
    // The fixture's own header is `lsd_fib`, so reading it re-runs the very
    // opposite-direction-complement fallback under test -- `wr-io` resolves
    // `lsd_fib_addition.txt` (absent), then `msd_fib_addition.txt` (present), and reverses.
    let mut ground_truth: Automaton =
        wr_io::reader::read_automaton_txt_with_custom_bases(&path, custom_bases_dir)
            .expect("fixture must parse cleanly, resolving lsd_fib through Custom Bases/");

    ours.sort_label();
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
    let want_names = vec![Some("lsd_fib".to_string()); tracks];
    assert_eq!(
        ground_truth.track_ns_names(),
        want_names,
        "the fixture's tracks must carry the CUSTOM base's name, not lsd_2"
    );
    assert_eq!(
        ours.track_ns_names(),
        want_names,
        "the port must keep every surviving track bound to lsd_fib, not silently fall back \
         to the programmatic lsd_2 its {{0, 1}} alphabet alone would suggest"
    );
    assert_eq!(
        automaton_language_equivalent(&ours, &ground_truth),
        Ok(true),
        "the Rust pipeline's result for `{what}` must be language-equivalent to real \
         walnut-java's output (see this file's module docs for the capture)"
    );
}

/// Asserts a closed `?lsd_fib` formula collapses to real Walnut's printed TRUE/FALSE.
fn assert_verdict(session: &Session, predicate: &str, name: &str, expected: bool) {
    let ours = run_eval_or_def(session, predicate, name, false)
        .unwrap_or_else(|e| panic!("`{predicate}` must evaluate end-to-end through wr-cli: {e}"));
    assert!(
        ours.fa.is_true_false_automaton(),
        "`{predicate}` must collapse to a trivial automaton"
    );
    assert_eq!(
        ours.fa.is_true_automaton(),
        expected,
        "`{predicate}` must match real walnut-java's printed verdict"
    );
}

/// `?lsd_fib x >= 2` — **no user-written quantifier.** The only `quantify` call is
/// `NumberSystem::comparison_const_b`'s own (it binds the constant to a fresh name and
/// quantifies it away), running against a `less_than` built programmatically over an
/// `all_reps`-restricted custom base whose adder came from a reversal. This is the
/// `lsd_k` analogue of `lsd_numeration.rs`'s headline `lsdge2` case, and the one that
/// proves the composition was never really about explicit quantifiers.
///
/// The result is also read back out of `Result/` after `eval` wrote it, so the writer's
/// `lsd_fib` header and the reader's custom-base resolution of it are in the loop too —
/// exactly the round trip a later `$token` reference or a research script depends on.
#[test]
fn lsd_custom_base_comparison_against_a_constant_matches_real_walnut_output() {
    let (session, dir) = temp_session("ge2");
    let bases = dir.join("Custom Bases");

    let returned = run_eval_or_def(&session, "?lsd_fib x >= 2", "lfge2", false)
        .expect("`eval` over lsd_fib must succeed through wr-cli");
    compare_to_fixture(
        returned,
        &bases,
        "ge_two.txt",
        &[vec![0, 1]],
        "eval lfge2 \"?lsd_fib x >= 2\"",
    );

    let written = dir.join("Result").join("lfge2.txt");
    let round_tripped = wr_io::reader::read_automaton_txt_with_custom_bases(&written, &bases)
        .expect("the file `eval` wrote must parse back cleanly, lsd_fib header and all");
    compare_to_fixture(
        round_tripped,
        &bases,
        "ge_two.txt",
        &[vec![0, 1]],
        "eval lfge2, after a .txt round trip",
    );

    fs::remove_dir_all(&dir).ok();
}

/// `?lsd_fib Ex (x < 5 & x >= 2 & y = x)` — a genuine `∃` over an `lsd` custom-base
/// variable, leaving one free variable. Language: `y in {2, 3, 4}`.
#[test]
fn lsd_custom_base_existential_quantification_matches_real_walnut_output() {
    let (session, dir) = temp_session("exists");
    let bases = dir.join("Custom Bases");

    let ours = run_eval_or_def(
        &session,
        "?lsd_fib Ex (x < 5 & x >= 2 & y = x)",
        "lfquant",
        false,
    )
    .expect("an existential over lsd_fib must succeed through wr-cli");
    compare_to_fixture(
        ours,
        &bases,
        "exists_quantified.txt",
        &[vec![0, 1]],
        "eval lfquant \"?lsd_fib Ex (x < 5 & x >= 2 & y = x)\"",
    );

    fs::remove_dir_all(&dir).ok();
}

/// `?lsd_fib x + y = z & z < 4` — **three tracks**, and the case that actually uses the
/// custom base's own adder (the one that exists only because
/// `msd_fib_addition.txt` was reversed). A doubled or missing reversal anywhere
/// downstream of `NumberSystem` shows up here and essentially nowhere else in this file:
/// the single-track cases above route through `less_than`, which for `msd_fib` is built
/// programmatically (`msd_fib` ships no `_less_than` file).
#[test]
fn lsd_custom_base_addition_matches_real_walnut_output() {
    let (session, dir) = temp_session("addition");
    let bases = dir.join("Custom Bases");

    let ours = run_eval_or_def(&session, "?lsd_fib x + y = z & z < 4", "lfadd", false)
        .expect("addition over lsd_fib must succeed through wr-cli");
    compare_to_fixture(
        ours,
        &bases,
        "addition_three_track.txt",
        &[vec![0, 1], vec![0, 1], vec![0, 1]],
        "eval lfadd \"?lsd_fib x + y = z & z < 4\"",
    );

    fs::remove_dir_all(&dir).ok();
}

/// The closed `∀` (`¬∃¬`) path over an `lsd` custom base, both polarities — a suite that
/// only ever asserts `FALSE` proves very little, since `FALSE` is what a broken `¬∃¬`
/// chain produces most easily.
///
/// Both verdicts here are nonetheless *weak* evidence on their own, and deliberately
/// labelled as such: `x >= 5` and `Ey (y > x)` are robust to a fair amount of damage
/// (a cofinite set stays cofinite), which mutation testing confirmed — this is the one
/// row of this file's module-doc mutation matrix that catches none of the three
/// mutations. Kept as cheap both-polarity coverage of the `¬∃¬` dispatch itself; the `∀`
/// case that actually discriminates is
/// [`lsd_custom_base_open_universal_quantification_matches_real_walnut_output`] below,
/// which compares a real automaton rather than a boolean.
#[test]
fn lsd_custom_base_closed_formulae_match_real_walnut_verdicts() {
    let (session, dir) = temp_session("closed");

    // walnut-java: `eval lfclosed "?lsd_fib Ax (x >= 5)";` prints "____\nFALSE".
    assert_verdict(&session, "?lsd_fib Ax (x >= 5)", "lfclosed", false);
    // walnut-java: `eval lfclosedtrue "?lsd_fib Ax Ey (y > x)";` prints "____\nTRUE".
    assert_verdict(&session, "?lsd_fib Ax Ey (y > x)", "lfclosedtrue", true);

    fs::remove_dir_all(&dir).ok();
}

/// `?lsd_fib Ay (y < 3 => y < x)` — an **open** `∀`, i.e. the `¬∃¬` path with a free
/// variable surviving, so the result is a real automaton (`x >= 3`) compared by language
/// equivalence rather than a single TRUE/FALSE bit.
///
/// This is the discriminating half of the `∀` coverage: unlike the two closed verdicts
/// above, it fails under both of the mutations that damage the `E`/`A` pipeline (M1 and
/// M2 in this file's module-doc matrix). It does not catch M3 — nothing routed through
/// `quantify` does, since the `I` quantifier's fixup is a separate code path.
#[test]
fn lsd_custom_base_open_universal_quantification_matches_real_walnut_output() {
    let (session, dir) = temp_session("forall");
    let bases = dir.join("Custom Bases");

    let ours = run_eval_or_def(&session, "?lsd_fib Ay (y < 3 => y < x)", "lfforall", false)
        .expect("an open universal over lsd_fib must succeed through wr-cli");
    compare_to_fixture(
        ours,
        &bases,
        "forall_open.txt",
        &[vec![0, 1]],
        "eval lfforall \"?lsd_fib Ay (y < 3 => y < x)\"",
    );

    fs::remove_dir_all(&dir).ok();
}

/// The `I` (infinitely-often) quantifier over an `lsd` custom base — the third arm of the
/// backlog item, and the one that routes through
/// `wr_core::logicalops::remove_leading_zeros` and then `wr_core::infinite::infinite`,
/// rather than through `wr_core::quantify` at all
/// (`Token::act_quantifier`'s `I` arm). `infinite_quantifier.rs` covers this for `lsd_k`;
/// nothing covered it for a custom base, where `remove_leading_zeros`'s reversal branch
/// runs over an `all_reps`-restricted alphabet.
///
/// Both polarities again, for the same reason as the closed-`∀` case above.
#[test]
fn lsd_custom_base_infinite_quantifier_matches_real_walnut_verdicts() {
    let (session, dir) = temp_session("infinite");

    // walnut-java: `eval lfinffalse "?lsd_fib Ix x < 5";` prints "____\nFALSE"
    // (five witnesses: 0..4).
    assert_verdict(&session, "?lsd_fib Ix x < 5", "lfinffalse", false);
    // walnut-java: `eval lfinftrue "?lsd_fib Ix x >= 5";` prints "____\nTRUE".
    assert_verdict(&session, "?lsd_fib Ix x >= 5", "lfinftrue", true);

    fs::remove_dir_all(&dir).ok();
}

/// `def lfge2d "?lsd_fib x >= 2";` then
/// `eval lfusedef "?lsd_fib $lfge2d(y) & y < 5";` — the `def`-then-reuse path, where the
/// saved automaton is re-read from `Automata Library/` as a `Function` token and combined
/// with a fresh custom-base comparison. Language: `y in {2, 3, 4}`.
///
/// This is the case that exercises the full `.txt` round trip of a custom-base *`lsd`*
/// header through `wr-io`'s reader **and** the `$name(…)`-token number-system construction
/// (`PredicateEnv::fresh_number_system`, whose custom-base resolution was itself a real
/// port bug fixed in Phase 3b U27 — for `msd_fib`; the `lsd` direction of it was never
/// checked until now).
#[test]
fn lsd_custom_base_def_then_reuse_matches_real_walnut_output() {
    let (session, dir) = temp_session("def");
    let bases = dir.join("Custom Bases");

    run_eval_or_def(&session, "?lsd_fib x >= 2", "lfge2d", true)
        .expect("`def` over lsd_fib must succeed through wr-cli");
    let ours = run_eval_or_def(&session, "?lsd_fib $lfge2d(y) & y < 5", "lfusedef", false)
        .expect("reusing a def'd lsd_fib automaton must succeed");

    compare_to_fixture(
        ours,
        &bases,
        "def_then_reuse.txt",
        &[vec![0, 1]],
        "def lfge2d; eval lfusedef",
    );

    fs::remove_dir_all(&dir).ok();
}
