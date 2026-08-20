// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **Phase 3a exit checkpoint** (per `.claude/plans/synthetic-prancing-aurora.md`, the section
//! right after the U16 row): the first place `eval`/`def`/`reg`/`alphabet` are exercised
//! together, through `wr-cli`'s REAL public library API (not an individual unit's own isolated
//! test), compared against real `walnut-java` CLI output via [`wr_core::equiv`]'s
//! semantic-equivalence oracle -- `CLAUDE.md`'s prime directive #1 (never byte/structural
//! comparison).
//!
//! # Relationship to existing differential coverage
//!
//! Three files already exercise pieces of this surface differentially:
//!
//! * `u11_eval_composition.rs` (U11) calls `wr_logic::eval::evaluate` directly -- **not**
//!   through `wr-cli`. Real, valuable coverage of the parser/quantifier-elimination/operand
//!   layer, but it never touches `wr_cli::eval_def::eval_def_command`, `Session`, or the
//!   `TestCase`/CAS-validation surface U15 added on top.
//! * `reg_brics_regex.rs` (U8) calls `wr_core::regex` directly -- **not** through `wr-cli`.
//! * `reg_and_alphabet_commands.rs` (U16) already calls `wr_cli::reg::reg`/
//!   `wr_cli::alphabet::alphabet_command` through a real [`Session`] -- this one already meets
//!   the checkpoint's "through wr-cli" bar for `reg`/`alphabet`, so this file does not
//!   re-duplicate its 60-case regex corpus; it adds a small number of NEW `reg`/`alphabet`
//!   cases chosen to compose with other features (a custom-base regex, a two-command
//!   reg-then-alphabet pipeline) rather than repeating single-feature coverage.
//!
//! **What was still completely unexercised through `wr-cli`'s real API before this file:**
//! `wr_cli::eval_def::eval_def_command`/`eval_def_command_with_stdout` (U15) -- the actual
//! `eval`/`def` command implementation, wired to a real [`Session`], including the
//! headless/non-headless split, the `TRUE`/`FALSE` console print, and the CAS
//! free-variable-list validation paths. This file's primary job is closing that gap, and
//! doing it with cases that combine MULTIPLE features at once (quantifiers + connectives,
//! word/function tokens + quantifiers, a custom base + relational comparison, etc.) since
//! that composition -- not any single feature in isolation -- is this checkpoint's actual
//! point.
//!
//! # `eval` vs `def`: confirmed identical dispatch
//!
//! `Main/Prover.java`'s command switch has `case DEF, EVAL -> { ... alphabetCommand ... }`
//! (verified directly against `Prover.java:495` in the real `walnut-java` checkout) -- **one
//! shared arm**, calling the exact same `evalDefCommand`. There is no parameter or behavior
//! that differs based on which of the two command names was typed; `wr_cli::eval_def`'s single
//! `eval_def_command`/`eval_def_command_with_stdout` already reflects this (see that module's
//! own docs). So "exercise both eval and def at the library-call level" cannot mean "find two
//! different functions" -- there is only one. What DOES genuinely vary from call to call is the
//! *shape* of usage each name conventionally signals: `eval` for a query with no interesting
//! free variables (often collapsing to `TRUE`/`FALSE`), `def` for a query that names free
//! variables for later reuse (often paired with an explicit free-variable list, which is what
//! actually drives the CAS-validation paths U15 added). This file's cases are named/grouped
//! along that convention and [`eval_and_def_are_the_identical_dispatch_call`] pins the "same
//! function, same result either way" claim directly rather than just asserting it in prose.
//!
//! # `wr-core::determinize`'s no-op context (U0c)
//!
//! The plan asks this checkpoint to exercise `eval`/`def`/`reg` "using U0c's no-op
//! strategy/export context." No test-side wiring is needed for that: every call path from
//! `wr_logic::quantify::exists` down to `wr_core::determinize::determinize` already passes
//! `None` for the `ctx: Option<&mut dyn DeterminizeContext>` parameter (see
//! `crates/wr-core/src/quantify.rs`'s `determinize(&mut projected, &initial, None)`), which is
//! exactly U0c's documented no-op default -- there is no real `MetaCommands` parser yet
//! (that's Phase 3b's U21) to wire a non-default context in from `wr-cli` even if we wanted to.
//! So simply calling the public `wr-cli` API, as every test below does, already exercises the
//! no-op path end-to-end.
//!
//! # Capture recipe (reproducible, same discipline as `../CAPTURE.md`/`u11_eval_composition.rs`)
//!
//! Three separate command-file runs (the CLI's `reg` grammar takes an UNQUOTED alphabet
//! token list, so `msd_fib` cannot share a line with the quoted-regex cases; `func_quant`
//! needed a second pass after an initial `lsd_2` attempt hit the then-open
//! `UnsupportedLsdFixup` gap documented at
//! [`function_token_combined_with_quantifier_matches_real_walnut`] and was switched to
//! `msd_2` — that gap is closed as of Phase 3b's L1, and `tests/lsd_numeration.rs` is
//! the differential suite that now covers `lsd_k`, but this file's captures were not
//! re-taken):
//!
//! ```bash
//! cd ~/dev/walnut-java
//! cp "src/test/resources/integrationTests/Global/Automata Library/T_2.txt" \
//!    "Automata Library/func_target.txt"   # temporarily, for the capture run only
//! cat > "Command Files/phase3a_checkpoint_capture.txt" <<'EOF'
//! eval bool_quant_combo "?msd_2 (Ex x < z) & Ay (y >= 0 | y < z)";
//! eval i_quant_combo "?msd_2 (Ix (x=x)) & y<5";
//! eval fib_cmp "?msd_fib x < y";
//! eval word_quant "?msd_2 Eb T[a] = T[b] & b = a+1";
//! eval arith_cmp "?msd_2 x + y = 2*z & z < 5";
//! eval triv_true "?msd_2 Ax Ey (y > x)";
//! eval triv_false "?msd_2 Ex (x < 0)";
//! eval def_freevars_ok "?msd_2 x < 5 & y = x";
//! eval func_quant "?msd_2 Ei (i < n & $func_target(i))";
//! reg baseR {0,1,2,3} "(0|1|2|3)*";
//! alphabet baseR_restricted {0,1} $baseR;
//! EOF
//! java -jar target/Walnut-all.jar phase3a_checkpoint_capture.txt < /dev/null
//! cat > "Command Files/phase3a_checkpoint_capture2.txt" <<'EOF'
//! reg fib_reg msd_fib "0*1";
//! EOF
//! java -jar target/Walnut-all.jar phase3a_checkpoint_capture2.txt < /dev/null
//! cp "Session/<timestamp1>/Result/"{bool_quant_combo,i_quant_combo,fib_cmp,word_quant,\
//!    func_quant,arith_cmp,def_freevars_ok,baseR,baseR_restricted}.txt \
//!    ~/dev/walnut-rs/tests/differential/fixtures/phase3a_checkpoint/
//! cp "Session/<timestamp2>/Result/fib_reg.txt" \
//!    ~/dev/walnut-rs/tests/differential/fixtures/phase3a_checkpoint/
//! # triv_true/triv_false print "____\nTRUE"/"____\nFALSE" to stdout -- no fixture file, see
//! # module docs above and u11_eval_composition.rs's own precedent for the same shape.
//! cp "Custom Bases/msd_fib.txt" "Custom Bases/msd_fib_addition.txt" \
//!    "Word Automata Library/T.txt" \
//!    "Automata Library/func_target.txt" \
//!    ~/dev/walnut-rs/tests/differential/fixtures/phase3a_checkpoint/
//! ```
//!
//! The command files, the temporary `Automata Library/func_target.txt` copy, and both
//! `Session/<timestamp>/` directories were deleted from the `walnut-java` checkout afterward,
//! matching `../CAPTURE.md`'s existing recipes. `msd_fib.txt`/`msd_fib_addition.txt` were
//! confirmed byte-identical to the copies already vendored at
//! `crates/wr-io/tests/fixtures/msd_{fib,fib_addition}.txt` before being re-copied here (a
//! self-contained copy, not a cross-crate fixture reference, matching this crate's existing
//! `fixtures/alphabet`/`fixtures/reg` convention).

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::alphabet::alphabet_command;
use wr_cli::eval_def::{eval_def_command_with_stdout, EvalDefError};
use wr_cli::reg::reg;
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;
use wr_logic::predicate_env::FreshIdentifiers;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/phase3a_checkpoint/{name}.txt"))
}

/// Same scaffolding convention as `reg_and_alphabet_commands.rs`/`eval_def.rs`'s own tests --
/// a process-scoped directory laid out as a full Walnut home tree so [`Session::new`] can
/// resolve every library address.
fn temp_session(tag: &str) -> (Session, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-phase3a-checkpoint-{tag}-{}",
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
    // `SessionPaths` does not append a trailing slash itself (Java's `Prover.parseArgs`
    // does that) -- supply one, matching every other differential test's convention.
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    (session, dir)
}

/// Copies a captured library fixture (`msd_fib.txt`, `T.txt`, `endsIn2Zeros.txt`, ...) from
/// this crate's own `fixtures/phase3a_checkpoint/` into `dir`'s `sub_dir`, under `dest_name`
/// -- the same "hand-place a file where `Session` will look for it" shape
/// `reg_and_alphabet_commands.rs` already uses for `baseC.txt`/`baseB.txt`.
fn install_library_file(dir: &Path, sub_dir: &str, src_name: &str, dest_name: &str) {
    fs::copy(fixture(src_name), dir.join(sub_dir).join(dest_name)).unwrap_or_else(|e| {
        panic!("must be able to install fixtures/phase3a_checkpoint/{src_name}.txt into {sub_dir}/{dest_name}: {e}")
    });
}

/// Compares `ours` against the captured ground-truth fixture at
/// `fixtures/phase3a_checkpoint/<name>.txt` by semantic language equivalence -- never
/// byte/structural identity, per `CLAUDE.md`'s prime directive #1.
///
/// `apply_sort_label`: `true` for `eval`/`def` results (named-variable tracks, whose order the
/// port's pipeline does not canonicalize the way Java's `AutomatonWriter.writeToTxtFormat`'s
/// `canonize()`/`sortLabel()` does before writing). `false` for `reg`/`alphabet` results, whose
/// tracks are positional (no named-label permutation possible), matching
/// `reg_and_alphabet_commands.rs`'s own comparison helper.
///
/// For every `eval`/`def` case in THIS file, `run_eval` calls `eval_def_command_with_stdout`
/// in non-headless mode, whose `write_automata` call (`crates/wr-cli/src/eval_def.rs`)
/// already canonicalizes `ours` as a side effect (`wr_io::writer`'s `write_automaton_txt`/
/// `_gv` both call `Automaton::canonize()`, which is `sort_label()` + `fa.canonicalize()`) --
/// so THIS function's own `sort_label()` call here is defense-in-depth, not the thing
/// actually doing the work for those cases (confirmed: none of this file's non-headless
/// multi-track tests fail if this call is removed). It only becomes load-bearing for a
/// HEADLESS multi-track result, which skips `write_automata` entirely and therefore never
/// canonicalizes on its own -- see `arithmetic_comparison_via_headless_mode_is_still_sorted`
/// below, which exercises exactly that path and genuinely requires this call to pass. See
/// `u11_eval_composition.rs`'s `compare_to_fixture` for the same normalization applied to a
/// call into `wr_logic::eval::evaluate` directly (bypassing `wr-cli`, so never canonicalized
/// by anything else), where it is unconditionally load-bearing.
fn assert_equivalent_to_fixture(
    mut ours: Automaton,
    fixture_name: &str,
    apply_sort_label: bool,
    msg: &str,
) {
    if apply_sort_label {
        ours.sort_label();
    }
    // Some fixtures (`fib_cmp.txt`, `fib_reg.txt`) declare a custom-base header
    // (`msd_fib`) rather than a plain `msd_k`/`lsd_k` one -- the plain `read_automaton_txt`
    // cannot parse those (`ReadError::UnsupportedNumeration`, by design: it has no custom-base
    // directory to resolve against). This crate's own `fixtures/phase3a_checkpoint/` directory
    // doubles as that directory (it also holds `msd_fib.txt`/`msd_fib_addition.txt`, see this
    // file's module docs), so every fixture in this file is read the same way, custom-base or
    // not -- `read_automaton_txt_with_custom_bases` falls back to the plain path for an
    // ordinary `msd_k`/`lsd_k` header.
    let custom_bases_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/phase3a_checkpoint");
    let mut ground_truth = wr_io::reader::read_automaton_txt_with_custom_bases(
        fixture(fixture_name),
        &custom_bases_dir,
    )
    .unwrap_or_else(|e| panic!("{msg}: fixture must parse cleanly, got {e:?}"));
    // Real Walnut's automaton for a free-variable predicate is not necessarily total (missing
    // transitions mean implicit rejection) -- totalize both sides before comparing, same
    // reasoning `u11_eval_composition.rs`/`spike_ei_i_lt_x.rs` already established.
    ours.fa.totalize(0);
    ground_truth.fa.totalize(0);
    assert_eq!(
        automaton_language_equivalent(&ours, &ground_truth),
        Ok(true),
        "{msg}: language must match real walnut-java's output"
    );
}

/// One `eval`/`def`-style call: fresh `Logging`/`FreshIdentifiers`, a throwaway stdout sink
/// (individual tests that care about the `TRUE`/`FALSE` print capture their own).
fn run_eval(
    session: &Session,
    predicate: &str,
    name: &str,
    free_var_str: Option<&str>,
) -> Result<Automaton, EvalDefError> {
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
        free_var_str,
        &mut stdout,
    )?;
    Ok(tc.automaton_pairs()[0].automaton().unwrap().clone())
}

/// Like [`run_eval`], but headless (`eval_name: None`) -- skips `write_automata` entirely,
/// so nothing canonicalizes the result's track order except `assert_equivalent_to_fixture`'s
/// own `sort_label()` call. Needed to make that call genuinely load-bearing in at least one
/// test in this file; see `assert_equivalent_to_fixture`'s docs.
fn run_eval_headless(session: &Session, predicate: &str) -> Result<Automaton, EvalDefError> {
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
        None,
        None,
        &mut stdout,
    )?;
    Ok(tc.automaton_pairs()[0].automaton().unwrap().clone())
}

// ---------------------------------------------------------------------------
// Boolean connectives + quantifiers (E/A) in combination
// ---------------------------------------------------------------------------

/// `?msd_2 (Ex x < z) & Ay (y >= 0 | y < z)` -- `Ex`/`Ay` and `&`/`|` all in one formula, one
/// free variable (`z`) surviving quantifier elimination.
#[test]
fn boolean_connectives_and_quantifiers_combine_correctly() {
    let (session, dir) = temp_session("bool-quant");
    let a = run_eval(
        &session,
        "?msd_2 (Ex x < z) & Ay (y >= 0 | y < z)",
        "bool_quant_combo",
        None,
    )
    .unwrap();
    assert_equivalent_to_fixture(a, "bool_quant_combo", true, "bool_quant_combo");
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_2 (Ix (x=x)) & y<5` -- the `I` (infinite) quantifier combined with `&`. `Ix (x=x)`'s
/// body has no free variable besides the bound `x`, so this does NOT trigger WB-027 (`I`
/// silently discarding a free variable it doesn't quantify) -- it is a clean, closed-body `I`
/// usage that correctly evaluates to a genuine `TRUE`, then combines with the free `y<5` via
/// `&`. One free variable (`y`) survives.
#[test]
fn infinite_quantifier_combined_with_boolean_connective() {
    let (session, dir) = temp_session("i-quant");
    let a = run_eval(&session, "?msd_2 (Ix (x=x)) & y<5", "i_quant_combo", None).unwrap();
    assert_equivalent_to_fixture(a, "i_quant_combo", true, "i_quant_combo");
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Relational/arithmetic comparisons, including a custom-base query
// ---------------------------------------------------------------------------

/// `?msd_fib x < y` -- a genuine custom-base (Fibonacci numeration) two-track relational
/// comparison, exercising U5's file-backed `NumberSystem` loading through the real `Session`
/// (needs `Custom Bases/msd_fib.txt` + `msd_fib_addition.txt`; `msd_fib` has no
/// `_less_than.txt` file in real Walnut, so this also exercises the main/complement-with-
/// reverse fallback `numsys.rs`'s module docs describe).
#[test]
fn custom_base_relational_comparison_matches_real_walnut() {
    let (session, dir) = temp_session("fib-cmp");
    install_library_file(&dir, "Custom Bases", "msd_fib", "msd_fib.txt");
    install_library_file(
        &dir,
        "Custom Bases",
        "msd_fib_addition",
        "msd_fib_addition.txt",
    );

    let a = run_eval(&session, "?msd_fib x < y", "fib_cmp", None).unwrap();
    // U23's review fixes made `Automaton` carry each track's real `NumberSystem.getName()`
    // (Java's `getNS().set(i, this)`), so a result built out of `msd_fib`'s own comparison
    // automaton reports `msd_fib` — not the `msd_2` its `{0, 1}` alphabet alone suggests.
    // That name is what `NumberSystem.isNSDiffering` compares and what
    // `AutomatonWriter.writeAlphabet` emits, so getting it wrong here would let
    // `union`/`intersect`/`concat` silently mix this result with a genuine base-2 one.
    assert_eq!(
        a.track_ns_names(),
        vec![Some("msd_fib".to_string()), Some("msd_fib".to_string())]
    );
    assert_equivalent_to_fixture(a, "fib_cmp", true, "fib_cmp");
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_2 x + y = 2*z & z < 5` -- arithmetic (addition, multiplication-by-constant) combined
/// with relational comparison and a boolean `&`, three free tracks (`x`, `y`, `z`) surviving.
#[test]
fn arithmetic_and_relational_comparison_matches_real_walnut() {
    let (session, dir) = temp_session("arith-cmp");
    let a = run_eval(&session, "?msd_2 x + y = 2*z & z < 5", "arith_cmp", None).unwrap();
    assert_equivalent_to_fixture(a, "arith_cmp", true, "arith_cmp");
    fs::remove_dir_all(&dir).ok();
}

/// Same LANGUAGE as `def_style_free_variable_list_passes_validation_like_real_walnut`'s
/// `?msd_2 x < 5 & y = x` (AND is commutative, so `def_freevars_ok.txt` is still valid
/// ground truth), but through the HEADLESS path (`eval_name: None`, and operands written in
/// the opposite order to guarantee a non-alphabetical RAW track order: confirmed empirically
/// -- `y = x & x < 5` evaluates to raw label order `["y", "x"]`, where `x < 5 & y = x` happens
/// to already come out `["x", "y"]` by coincidence and would not have exercised anything).
/// Headless mode never calls `write_automata` and therefore never canonicalizes the result on
/// its own. This is the one case in this file where `assert_equivalent_to_fixture`'s own
/// `sort_label()` call is genuinely load-bearing, not defense-in-depth -- see that function's
/// docs. Deleting the `sort_label()` call there makes THIS test (and only this test) fail;
/// verified directly before landing this test.
#[test]
fn arithmetic_comparison_via_headless_mode_is_still_sorted() {
    let (session, dir) = temp_session("def-freevars-headless");
    let a = run_eval_headless(&session, "?msd_2 y = x & x < 5").unwrap();
    assert_eq!(
        a.label,
        vec!["y".to_string(), "x".to_string()],
        "raw (pre-sort) track order must be non-alphabetical, or this test isn't testing what it claims"
    );
    assert_equivalent_to_fixture(a, "def_freevars_ok", true, "def_freevars_headless");
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Word/function tokens combined with quantifiers
// ---------------------------------------------------------------------------

/// `?msd_2 Eb T[a] = T[b] & b = a+1` -- the Thue-Morse word automaton (`T.txt`) indexed twice
/// (`T[a]`, `T[b]`) and compared, combined with `Eb` and `&`/arithmetic (`b = a+1`). This
/// exact predicate shape (modulo the `?msd_2` prefix and result name) is real Walnut's own
/// `test164`/`test165` (`phase0-artifacts/test-manifest.json`), so this is a well-trodden,
/// already-validated-by-Walnut's-own-suite pattern, now additionally checked against a live
/// capture through the port's real `wr-cli` dispatch. One free variable (`a`) survives.
#[test]
fn word_token_combined_with_quantifier_matches_real_walnut() {
    let (session, dir) = temp_session("word-quant");
    install_library_file(&dir, "Word Automata Library", "T", "T.txt");

    let a = run_eval(
        &session,
        "?msd_2 Eb T[a] = T[b] & b = a+1",
        "word_quant",
        None,
    )
    .unwrap();
    assert_equivalent_to_fixture(a, "word_quant", true, "word_quant");
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_2 Ei (i < n & $func_target(i))` -- a `Function` token (`$func_target(i)`, a plain
/// small `msd_2` DFA -- `Automata Library/T_2.txt` from `walnut-java`'s own integration-test
/// fixtures, renamed `func_target.txt` here to avoid clashing with the Thue-Morse `T.txt`
/// word automaton used above -- read as a one-argument boolean predicate) combined with `Ei`
/// and `&`. One free variable (`n`) survives.
///
/// **Why `msd_2`, not `lsd_2`:** an earlier draft of this test used the real Walnut example
/// `$endsIn2Zeros(i)` (an `lsd_2` automaton, from `IntegrationTest.java`) with an `Ei`
/// quantifier. That combination -- `∃` elimination over an `lsd`-numeration variable --
/// returned `QuantifyError::UnsupportedLsdFixup` in this port when this checkpoint was
/// written: `wr_core::quantify`'s post-∃-projection zero fixup was wired up for `msd`
/// systems only, a pre-existing Phase-2 scope gap predating this checkpoint -- **not** a
/// new finding from this file, and not a `WALNUT-BUGS.md` entry (it was the Rust port's own
/// known-incomplete surface, not a Java behavioral divergence). This test was switched to an
/// `msd_2` function target rather than chasing that gap, since exercising "function token +
/// quantifier" is this case's actual point.
///
/// **That gap is now closed** (Phase 3b's L1 wired `fixTrailingZerosProblem` in; see
/// `wr_core::quantify`'s "The lsd fixup" section). `lsd_k` differential coverage lives in
/// `tests/lsd_numeration.rs`, deliberately as its own file with its own captures rather
/// than by re-taking this checkpoint's fixtures -- this file is a *Phase 3a* exit record
/// and is left as captured.
#[test]
fn function_token_combined_with_quantifier_matches_real_walnut() {
    let (session, dir) = temp_session("func-quant");
    install_library_file(&dir, "Automata Library", "func_target", "func_target.txt");

    let a = run_eval(
        &session,
        "?msd_2 Ei (i < n & $func_target(i))",
        "func_quant",
        None,
    )
    .unwrap();
    assert_equivalent_to_fixture(a, "func_quant", true, "func_quant");
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// TRUE/FALSE (trivial-automaton) results for `eval`
// ---------------------------------------------------------------------------

/// `?msd_2 Ax Ey (y > x)` -- closed (no free variables), real Walnut prints `____\nTRUE`
/// (naturals are unbounded, so every `x` has a larger `y`).
#[test]
fn closed_formula_evaluates_to_true_matching_real_walnut_verdict() {
    let (session, dir) = temp_session("triv-true");
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut fresh = FreshIdentifiers::new();
    let mut stdout = Vec::new();

    let tc = eval_def_command_with_stdout(
        &session,
        &mut logging,
        &mut fresh,
        false,
        false,
        "?msd_2 Ax Ey (y > x)",
        Some("triv_true"),
        None,
        &mut stdout,
    )
    .unwrap();

    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        "____\nTRUE\n",
        "must match real walnut-java's printed verdict"
    );
    let a = tc.automaton_pairs()[0].automaton().unwrap();
    assert!(a.is_true_false_automaton());
    assert!(a.is_true_automaton());
    fs::remove_dir_all(&dir).ok();
}

/// `?msd_2 Ex (x < 0)` -- closed, real Walnut prints `____\nFALSE` (the domain is the
/// nonnegative integers, so no `x` satisfies `x < 0`).
#[test]
fn closed_formula_evaluates_to_false_matching_real_walnut_verdict() {
    let (session, dir) = temp_session("triv-false");
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut fresh = FreshIdentifiers::new();
    let mut stdout = Vec::new();

    let tc = eval_def_command_with_stdout(
        &session,
        &mut logging,
        &mut fresh,
        false,
        false,
        "?msd_2 Ex (x < 0)",
        Some("triv_false"),
        None,
        &mut stdout,
    )
    .unwrap();

    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        "____\nFALSE\n",
        "must match real walnut-java's printed verdict"
    );
    let a = tc.automaton_pairs()[0].automaton().unwrap();
    assert!(a.is_true_false_automaton());
    assert!(!a.is_true_automaton());
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// `def`-style free-variable cases, including the CAS-matrix validation success path
// ---------------------------------------------------------------------------

/// `?msd_2 x < 5 & y = x` with an explicit, VALID free-variable list (`"x y"`) -- both names
/// are real track labels on a non-trivial result, so real Walnut's
/// `AutomatonMatrixWriter.writeMatrix` validation SUCCEEDS and proceeds to real CAS export.
/// CAS matrix export is no longer DROP scope (`.claude/plans/amber-transcribing-ledger.md`,
/// `wr_io::matrix_writer`) -- this now asserts the real matrix content is produced, not the
/// signed-off divergence it used to pin.
///
/// The matrix comparison is BYTE-EXACT against real `walnut-java` output captured for this
/// exact query (`fixtures/cas_export/`, see this crate's `CAPTURE.md`) -- not a substring
/// check. A substring check (`m.contains("M_x_y_")`) would survive a wrong matrix order, a
/// wrong fix-up representative, wrong separators, wrong brace nesting, wrong `Q`, or wrong
/// `q0` in any of the four formats; this is the fast-tier's only end-to-end byte-exact pin of
/// the `eval`/`def` -> matrix-file pipeline (the golden corpus's own byte-exact coverage of
/// the same pipeline is `#[ignore]`d, gated-slow). Byte-exact text comparison is correct here
/// -- `CLAUDE.md`'s "compare by semantic equivalence, never structural identity" rule is about
/// AUTOMATA, not CAS text output, which real Walnut's own `IntegrationTest` also compares
/// exactly (mod normalization).
#[test]
fn def_style_free_variable_list_passes_validation_and_writes_real_matrix_files() {
    let (session, dir) = temp_session("def-freevars-ok");
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut fresh = FreshIdentifiers::new();
    let mut stdout = Vec::new();

    let tc = eval_def_command_with_stdout(
        &session,
        &mut logging,
        &mut fresh,
        false,
        false,
        "?msd_2 x < 5 & y = x",
        Some("def_freevars_ok"),
        Some("x y"),
        &mut stdout,
    )
    .unwrap_or_else(|e| panic!("a valid free-variable list must succeed, got {e}"));

    let matrix_output = tc.matrix_output().unwrap();
    assert_eq!(matrix_output.len(), 4);
    // `EMITTERS`/`matrix_output()` order: Maple, MATLAB, Mathematica, Sage
    // (`AutomatonMatrixWriter.java:16-18`; `crates/wr-io/src/matrix_writer.rs`).
    let expected_exts = ["mpl", "m", "wl", "sage"];
    for (m, ext) in matrix_output.iter().zip(expected_exts) {
        let expected = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("fixtures/cas_export/def_freevars_ok.{ext}")),
        )
        .unwrap_or_else(|e| panic!("reading captured cas_export/def_freevars_ok.{ext}: {e}"));
        assert_eq!(
            m.trim(),
            expected.trim(),
            "matrix output for .{ext} must match real walnut-java byte-for-byte"
        );
    }
    let a = tc.automaton_pairs()[0].automaton().unwrap().clone();
    assert_equivalent_to_fixture(a, "def_freevars_ok", true, "def_freevars_ok");
    fs::remove_dir_all(&dir).ok();
}

/// Direct evidence for this file's module-docs claim: `eval` and `def` reach the exact same
/// function with the exact same observable result (automaton, stdout print, free-variable
/// validation) -- there is nothing named "def" to test separately from "eval" at the
/// library-call level, only a different result name/free-variable-list on the same call.
#[test]
fn eval_and_def_are_the_identical_dispatch_call() {
    let (session, dir) = temp_session("eval-def-identical");
    let mut logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut fresh = FreshIdentifiers::new();
    let mut stdout_eval = Vec::new();
    let mut stdout_def = Vec::new();

    // "eval"-flavored call: no free-variable list, a name that suggests a query.
    let eval_tc = eval_def_command_with_stdout(
        &session,
        &mut logging,
        &mut fresh,
        false,
        false,
        "?msd_2 y = x & x < 5",
        Some("as_eval"),
        None,
        &mut stdout_eval,
    )
    .unwrap();
    // "def"-flavored call: same predicate, a name that suggests a reusable definition, PLUS a
    // free-variable list -- exactly what distinguishes conventional `def` usage in practice,
    // even though (per this file's module docs) `Prover.java` dispatches both identically.
    let def_tc = eval_def_command_with_stdout(
        &session,
        &mut logging,
        &mut fresh,
        false,
        false,
        "?msd_2 y = x & x < 5",
        Some("as_def"),
        Some("x y"),
        &mut stdout_def,
    )
    .unwrap();

    let mut a = eval_tc.automaton_pairs()[0].automaton().unwrap().clone();
    let mut b = def_tc.automaton_pairs()[0].automaton().unwrap().clone();
    a.fa.totalize(0);
    b.fa.totalize(0);
    assert_eq!(
        automaton_language_equivalent(&a, &b),
        Ok(true),
        "eval-style and def-style calls to the identical function must agree"
    );
    // Neither is a TRUE/FALSE result, so neither prints the `____` line -- the free-variable
    // list alone doesn't change whether THAT print happens. It DOES, however, now (CAS export
    // is no longer DROP scope) trigger the SEPARATE "Matrix files:" print for the `def`-style
    // call, since that call alone supplied a non-empty free-variable list. Pinned exactly
    // (all four emitter lines, not just the leading "Matrix files:\n"), matching
    // `eval_def.rs`'s own `free_variable_result_writes_automaton_normally_and_the_real_
    // matrix_side_files` convention -- a `starts_with` check alone would not notice a
    // wrong/missing/reordered emitter line.
    assert_eq!(stdout_eval, Vec::<u8>::new());
    let result_dir = dir.join("Result");
    assert_eq!(
        String::from_utf8(stdout_def.clone()).unwrap(),
        format!(
            "Matrix files:\n  Maple: {rd}/as_def.mpl\n  MATLAB/Octave: {rd}/as_def.m\n  \
             Mathematica: {rd}/as_def.wl\n  Sage: {rd}/as_def.sage\n",
            rd = result_dir.to_str().unwrap()
        ),
        "the def-style call's free-variable list should trigger the real CAS export print now"
    );

    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// `reg`: a named-number-system regex (composed with a custom base) + a literal-set case
// reused from U8/U16's already-captured corpus
// ---------------------------------------------------------------------------

/// `reg fib_reg msd_fib "0*1"` -- a regex evaluated directly over the CUSTOM base `msd_fib`'s
/// alphabet (not a literal `{...}` set, and not one of U16's own already-covered named-NS
/// cases, which only used `msd_2`/`msd_3` and set literals) -- new composition: regex engine
/// (U8) + custom-base `NumberSystem` (U5) + the `reg` command (U16), together.
#[test]
fn reg_over_a_custom_base_number_system_matches_real_walnut() {
    let (session, dir) = temp_session("fib-reg");
    install_library_file(&dir, "Custom Bases", "msd_fib", "msd_fib.txt");
    install_library_file(
        &dir,
        "Custom Bases",
        "msd_fib_addition",
        "msd_fib_addition.txt",
    );

    let tc = reg(&session, &mut Logging::new(), "msd_fib ", "0*1", "fib_reg")
        .unwrap_or_else(|e| panic!("reg over msd_fib must build, got {e}"));
    let a = tc.automaton_pairs()[0].automaton().unwrap().clone();
    // Same U23 review-fix property as `custom_base_relational_comparison_matches_real_walnut`,
    // through the OTHER producer that installs number systems (`Reg.java:34`'s `R.setNS(NS)`).
    assert_eq!(a.track_ns_names(), vec![Some("msd_fib".to_string())]);
    assert_equivalent_to_fixture(a, "fib_reg", false, "fib_reg (msd_fib)");
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// `reg` followed by `alphabet`: a two-command pipeline through the same Session
// ---------------------------------------------------------------------------

/// `reg baseR {0,1,2,3} "(0|1|2|3)*"` (accepts every string over all four digits) then
/// `alphabet baseR_restricted {0,1} $baseR` (restricts to `{0,1}`, pruning the transitions on
/// digits 2/3) -- two commands chained through the SAME `Session`, the `reg` command's own
/// `Automata Library` write feeding the `alphabet` command's read. Exercises genuine
/// transition pruning (every state has real outgoing transitions on all four digits before
/// restriction), not just a header/metadata rewrite.
#[test]
fn reg_then_alphabet_pipeline_matches_real_walnut() {
    let (session, dir) = temp_session("reg-then-alphabet");

    let base_tc = reg(
        &session,
        &mut Logging::new(),
        "{0,1,2,3} ",
        "(0|1|2|3)*",
        "baseR",
    )
    .unwrap_or_else(|e| panic!("reg baseR must build, got {e}"));
    let base_a = base_tc.automaton_pairs()[0].automaton().unwrap().clone();
    assert_equivalent_to_fixture(base_a, "baseR", false, "baseR (pre-restriction)");

    let restricted_tc = alphabet_command(
        &session,
        &mut Logging::new(),
        "alphabet baseR_restricted {0,1} $baseR",
        Some("{0,1} "),
        false,
        "baseR.txt",
        "baseR_restricted",
    )
    .unwrap_or_else(|e| panic!("alphabet command must succeed, got {e}"));
    let restricted_a = restricted_tc.automaton_pairs()[0]
        .automaton()
        .unwrap()
        .clone();
    assert_equivalent_to_fixture(restricted_a, "baseR_restricted", false, "baseR_restricted");

    fs::remove_dir_all(&dir).ok();
}
