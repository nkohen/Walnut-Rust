// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Test.java` (129 LOC) — the `test` command: "find the first N
//! shortlex-smallest non-empty inputs accepted by a given automaton", used to sanity
//! check an automaton by manual inspection.
//!
//! # `ProductBFS`, and what `wr_core::search` already did for this unit
//!
//! Java's `findNextAcceptedWord` (`:71-97`) drives `Automata.Search.ProductBFS`'s
//! generic `int[]`-tuple BFS ([`wr_core::search::shortest_witness_word_int`], ported by
//! U19) over a 3-field product state `[M-state, min(length read, |previous|+1),
//! lexicographic-comparison-with-previous]`. U19 also added
//! [`wr_core::search::shortest_accepted_word`] as a *faithful specialization* of that
//! same call to `previous == null` — but `findAccepted`'s loop (`:34-59`) calls
//! `findNextAcceptedWord` with a non-`null` `previous` on every iteration after the
//! first, which needs the full 3-field state that specialization deliberately folds
//! away (see that function's own doc comment, "Correspondence to `Test.java`'s
//! product-state layout"). So this module reimplements the general 3-tuple search
//! directly on top of the lower-level [`wr_core::search::shortest_witness_word_int`]
//! primitive, rather than layering on `shortest_accepted_word` for just the first call
//! — one code path handles every iteration, exactly like Java's single
//! `findNextAcceptedWord` method does.
//!
//! # No hard-error guard for non-determinism, unlike `search.rs`'s own entry points
//!
//! [`wr_core::search::shortest_accepted_word`]/[`wr_core::search::shortest_witness_word_product`]
//! reject a component with more than one destination per `(state, symbol)` as a hard
//! [`wr_core::search::SearchError`] — a deliberate strengthening *this port itself*
//! introduced (see that module's docs), not something Java's `ProductBFS`/`Test` do.
//! `Test.findNextAcceptedWord`'s step closure (`:90`) just takes
//! `destinations.getInt(0)` with no determinism check at all, trusting its caller
//! (`Test.testCommand`, via `AutomatonDFA.readAutomatonDFAFromFile`) to have already
//! guaranteed a DFA. [`find_next_accepted_word`] below is a direct, mechanical port of
//! that literal indexing — it does not add the stricter guard, matching
//! `findAccepted`'s own public signature (`Automaton M`, not `AutomatonDFA M` —
//! `TestTest.java` calls it directly with plain `Automaton`s built by hand).
//!
//! One consequence of keeping [`find_accepted`] `pub` and plain-`Automaton`-taking has
//! to be paid for explicitly, though: a hand-built **nondeterministic FAO** (a word
//! automaton with an output `> 1`) would reach `AutomatonDFA::require_dfa_storage`'s
//! `panic!("NFAOs are not supported..")` through the `removeLeadingZeros` pre-pass,
//! where Java throws a catchable `WalnutException.nonDeterministicO()`. A `pub` CLI API
//! that aborts the process where Java cleanly errors is the wrong shape for a crate
//! boundary, so [`find_accepted`] screens for exactly that case up front and returns
//! [`TestError::NonDeterministicO`] (Java's own message) instead. The `wr-core` panic
//! itself is left alone — it has other callers and is not this unit's to change.
//!
//! # What the CLI path actually guarantees about determinism (and WB-022)
//!
//! Java's `Test.testCommand` gets its DFA guarantee from
//! `AutomatonDFA.readAutomatonDFAFromFile`, whose `AutomatonReader.readAutomaton` has an
//! `isFAO()` check that **rejects** a genuinely nondeterministic FAO file outright with
//! `WalnutException.nonDeterministicO()`, and determinizes only the plain-NFA case.
//! [`test_command_to`] below does *not* reproduce that: `wr-io`'s reader (pre-existing
//! code, not this unit's) has no `isFAO()` guard and silently determinizes **any**
//! nondeterministic input, NFAO included, so a file Java would reject is instead
//! accepted here and enumerated as its determinized boolean projection. That gap is
//! already logged and deliberately deferred as **WB-022** in `docs/WALNUT-BUGS.md` (it
//! is a Rust-port scope gap, not a Java bug); this module neither widens nor closes it,
//! and the paragraph above is what stands in for it on the direct-Rust-caller path.
//!
//! # Resource cap: the handoff `wr_core::search`'s docs make to this unit
//!
//! `wr_core::search`'s module docs state that neither search function is resource-capped
//! (matching Java) and that "whoever wires U25 to real user-supplied automata is
//! therefore responsible for imposing the cap at that layer". [`test_command_to`] is
//! that layer: `needed` comes straight from user text (`Prover.java`'s `\d+` capture
//! group), so an unguarded `test foo 2147483647` would ask for a
//! `Vec::with_capacity(i32::MAX)` — a ~51 GB allocation, which in Rust aborts rather
//! than throwing Java's catchable `OutOfMemoryError`. [`MAX_NEEDED`] is that cap: a
//! **deliberate, port-specific deviation** from Java's unbounded `new
//! ArrayList<>(needed)`, rejecting absurd counts with a clean [`TestError`] instead.
//! It bounds the allocation and the outer loop's iteration count; it does *not* bound
//! any single [`find_next_accepted_word`] call, whose BFS is `O(Q · |Σ| · |previous|)`
//! per invocation and inherits `ProductBFS`'s own lack of a budget.

use std::io::{self, Write};

use wr_core::automaton::{Automaton, AutomatonDFA};
use wr_core::logging::Logging;
use wr_core::logicalops::{remove_leading_zeros_with_ctx, RemoveLeadingZerosError};
use wr_core::search::shortest_witness_word_int;
use wr_logic::predicate_env::PredicateEnvError;

use crate::session::Session;

/// Everything [`find_accepted`]/[`test_command`] can fail with.
#[derive(Debug)]
pub enum TestError {
    /// `Test.java:73-75`'s inline `throw new WalnutException("Cannot enumerate accepted
    /// inputs of an unmaterialized true automaton.")` — not one of
    /// `WalnutException.java`'s 33 named factories (`crate::walnut_exception`'s module
    /// docs), so the text is carried directly here rather than through that module.
    UnmaterializedTrueAutomaton,
    /// Propagated from `Session`'s library file resolution (`AutomatonDFA(String
    /// address)` -> `AutomatonReader.readAutomaton`).
    Read(PredicateEnvError),
    /// Propagated from `AutomatonLogicalOps.removeLeadingZeros` (`Test.java:43`).
    RemoveLeadingZeros(RemoveLeadingZerosError),
    /// `WalnutException.nonDeterministicO()` — a nondeterministic FAO handed to
    /// [`find_accepted`] directly. See this module's docs: Java throws this (catchable)
    /// from the same `removeLeadingZeros` pre-pass where `wr-core` would instead
    /// `panic!`, so this variant is the guard that keeps a `pub` API from aborting the
    /// process. Unreachable through [`test_command_to`] (see WB-022).
    NonDeterministicO,
    /// **Port-specific**, no Java analogue: `needed` exceeds [`MAX_NEEDED`]. See this
    /// module's "Resource cap" docs for why this deviation from Java's unbounded `new
    /// ArrayList<>(needed)` is deliberate.
    NeededTooLarge { needed: i32 },
    /// A real I/O failure while writing the command's console output. See
    /// `crate::automaton_output`'s module docs on why this crate propagates write
    /// failures rather than swallowing them the way Java's `System.out.println` does.
    Io(io::Error),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::UnmaterializedTrueAutomaton => write!(
                f,
                "Cannot enumerate accepted inputs of an unmaterialized true automaton."
            ),
            TestError::Read(e) => write!(f, "{e}"),
            TestError::RemoveLeadingZeros(e) => write!(f, "{e}"),
            TestError::NonDeterministicO => {
                write!(f, "{}", crate::walnut_exception::non_deterministic_o())
            }
            TestError::NeededTooLarge { needed } => write!(
                f,
                "The test command refuses to enumerate {needed} inputs; \
                 the limit is {MAX_NEEDED}."
            ),
            TestError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TestError {}

impl From<PredicateEnvError> for TestError {
    fn from(e: PredicateEnvError) -> Self {
        TestError::Read(e)
    }
}

impl From<RemoveLeadingZerosError> for TestError {
    fn from(e: RemoveLeadingZerosError) -> Self {
        TestError::RemoveLeadingZeros(e)
    }
}

impl From<io::Error> for TestError {
    fn from(e: io::Error) -> Self {
        TestError::Io(e)
    }
}

/// `Test.testCommand(String testName, int needed)` (`:22-32`).
///
/// Reads `testName` from the Automata Library (`AutomatonDFA.readAutomatonDFAFromFile`),
/// prints the first `needed` shortlex-smallest accepted inputs (or every accepted input,
/// with a shortfall message, if fewer than `needed` exist), and returns whether at least
/// `needed` inputs were found.
///
/// **Trap:** this writes to the real process stdout, so it escapes any output-capturing
/// harness. Nothing in this crate calls it — `crate::prover`'s `test` arm goes through
/// [`test_command_to`] with the `Prover`'s own sink, and so should any future caller;
/// this form exists only to keep `Test.testCommand`'s zero-argument-sink signature
/// traceable.
pub fn test_command(
    session: &Session,
    logging: &mut Logging,
    test_name: &str,
    needed: i32,
) -> Result<bool, TestError> {
    test_command_to(session, logging, test_name, needed, &mut io::stdout())
}

/// As [`test_command`], with an injectable sink for the console output — the same seam
/// `crate::prover_helper`'s `_to` functions use.
pub fn test_command_to(
    session: &Session,
    logging: &mut Logging,
    test_name: &str,
    needed: i32,
    stdout: &mut dyn Write,
) -> Result<bool, TestError> {
    // `AutomatonDFA M = AutomatonDFA.readAutomatonDFAFromFile(testName);` (`:23`).
    let address = session
        .paths()
        .read_file_for_automata_library(&format!("{test_name}{}", crate::prover::TXT_EXTENSION));
    let automaton = session.libraries().read_library_automaton(&address)?;
    // `AutomatonDFA` is consumed back into its `Automaton` here only because
    // `find_accepted` mutates its argument's `label` in place, matching Java (see its
    // doc). The determinism the wrapper asserted still holds — `randomLabel` touches
    // nothing but the track labels.
    let mut m = AutomatonDFA::from(automaton).into_automaton();

    let accepted = find_accepted(&mut m, logging, needed)?;

    // `if (accepted.size() < needed) { System.out.println(...); }` (`:25-27`).
    if (accepted.len() as i64) < needed as i64 {
        writeln!(
            stdout,
            "{test_name} only accepts {} inputs, which are as follows: ",
            accepted.len()
        )?;
    }
    for input in &accepted {
        writeln!(stdout, "{input}")?;
    }
    Ok(accepted.len() as i64 >= needed as i64)
}

/// The largest `needed` [`find_accepted`] will accept — see this module's "Resource cap"
/// docs. One million is far past any plausible manual-inspection use of `test` (Java's
/// own `TestTest`/`ProverHelperTest` cases ask for 10 at most) while still leaving the
/// `Vec::with_capacity` below a few megabytes rather than tens of gigabytes.
pub const MAX_NEEDED: i32 = 1_000_000;

/// `Test.findAccepted(Automaton M, int needed)` (`:34-59`) — public, like Java's, since
/// `TestTest.java` (and this module's own tests) call it directly with hand-built
/// [`Automaton`]s, not only through [`test_command`]'s file-loading path.
///
/// Takes `&mut Automaton` rather than `&Automaton` because Java's `M.randomLabel()`
/// (`:42`) mutates the **caller's** object — only the `M = removeLeadingZeros(M, ..)`
/// rebinding on the next line is local. A `&Automaton` + internal clone would silently
/// diverge from that, so the labelling is applied in place here too. (Java's own only
/// production caller, `testCommand`, uses a local variable and cannot observe it; a
/// direct Rust caller can, exactly as a direct Java caller can.)
pub fn find_accepted(
    a: &mut Automaton,
    logging: &mut Logging,
    needed: i32,
) -> Result<Vec<String>, TestError> {
    if needed <= 0 {
        return Ok(Vec::new());
    }
    // Port-specific, see [`MAX_NEEDED`]. Placed after the `needed <= 0` short-circuit so
    // the ordering of Java's own early return is untouched.
    if needed > MAX_NEEDED {
        return Err(TestError::NeededTooLarge { needed });
    }
    let needed = needed as usize;

    // "We do not want to count multiple representations of the same value as distinct
    // accepted values. This preserves the existing behavior that skips representations
    // beginning with 0 (or [0,0], etc., for higher-arity numeric inputs)." (`:39-41`).
    a.random_label();
    let labels = a.label.clone();
    // The `pub`-API panic guard described in this module's docs: `removeLeadingZeros`
    // folds its per-track constraints into `and(A, M)`, which routes through
    // `AutomatonDFA::from` -> `require_dfa_storage`, which `panic!`s (rather than
    // erroring) on a nondeterministic FAO. Mirrors that function's own precondition
    // exactly, including its `isTRUE_FALSE_AUTOMATON` short-circuit; skipped when the
    // label list is empty, since `removeLeadingZeros` then returns before the `and`.
    if !labels.is_empty()
        && !a.fa.is_true_false_automaton()
        && !a.fa.is_deterministic()
        && a.is_fao()
    {
        return Err(TestError::NonDeterministicO);
    }
    let m = remove_leading_zeros_with_ctx(a, &labels, None, logging)?;

    let mut accepted = Vec::with_capacity(needed);
    let mut previous: Option<Vec<i32>> = None;

    while accepted.len() < needed {
        let Some(next_word) = find_next_accepted_word(&m, previous.as_deref())? else {
            break;
        };
        accepted.push(format_accepted_word(&m, &next_word));
        previous = Some(next_word);
    }

    Ok(accepted)
}

/// `Test.findNextAcceptedWord(Automaton M, Word<Integer> previous)` (`:71-97`) — see this
/// module's docs for why it is a direct 3-tuple port rather than a reuse of
/// [`wr_core::search::shortest_accepted_word`].
///
/// Product state layout, verbatim from Java's doc comment (`:66-69`):
/// `[0]` state of `M`; `[1]` `min(length read so far, previous.length() + 1)`; `[2]`
/// lexicographic comparison with `previous` while the current length is
/// `<= previous.length()`.
fn find_next_accepted_word(
    m: &Automaton,
    previous: Option<&[i32]>,
) -> Result<Option<Vec<i32>>, TestError> {
    if m.fa.is_true_false_automaton() {
        return if m.fa.is_true_automaton() {
            // `:73-75`.
            Err(TestError::UnmaterializedTrueAutomaton)
        } else {
            // `:76`.
            Ok(None)
        };
    }

    // `int[] start = { M.fa.getQ0(), 0, 0 };` (`:79`).
    let start = [m.fa.q0 as i32, 0, 0];
    let prev_len_plus_one = previous.map_or(1, |p| p.len() as i32 + 1);

    Ok(shortest_witness_word_int(
        &start,
        m.fa.alphabet_size,
        |state, symbol, out| {
            // `IntList destinations = M.fa.getT().getNfaStateDests(state[0], symbol); if
            // (destinations == null || destinations.isEmpty()) return false;` (`:85-88`).
            // No determinism check — see this module's docs.
            let Some(dests) = m.fa.d[state[0] as usize].get(&symbol) else {
                return false;
            };
            if dests.is_empty() {
                return false;
            }
            let old_length = state[1];
            out[0] = dests[0] as i32;
            out[1] = (old_length + 1).min(prev_len_plus_one);
            out[2] = update_comparison(previous, old_length, state[2], symbol);
            true
        },
        |state| {
            state[1] != 0
                && m.fa.is_accepting(state[0] as usize)
                && is_after_previous(previous, state[1], state[2])
        },
    ))
}

/// `Test.updateComparison(Word<Integer> previous, int position, int comparison, int
/// symbol)` (`:99-104`).
fn update_comparison(previous: Option<&[i32]>, position: i32, comparison: i32, symbol: i32) -> i32 {
    let Some(p) = previous else {
        return comparison;
    };
    if comparison != 0 || (position as usize) >= p.len() {
        return comparison;
    }
    // `Integer.compare(symbol, previous.getSymbol(position))` — only the SIGN is ever
    // read by [`is_after_previous`], so `Ordering`'s three cases stand in for whatever
    // magnitude Java's `Integer.compare` happens to return.
    match symbol.cmp(&p[position as usize]) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `Test.isAfterPrevious(Word<Integer> previous, int length, int comparison)` (`:106-108`).
fn is_after_previous(previous: Option<&[i32]>, length: i32, comparison: i32) -> bool {
    match previous {
        None => true,
        Some(p) => {
            let prev_len = p.len() as i32;
            length > prev_len || (length == prev_len && comparison > 0)
        }
    }
}

/// `Test.formatAcceptedWord(Automaton M, Word<Integer> word)` (`:114-128`). "Keeps the
/// same user-facing formatting as `Automaton.findAcceptedHelper`: single-arity digits
/// 0..9 are printed without brackets, while vector symbols remain bracketed."
fn format_accepted_word(m: &Automaton, word: &[i32]) -> String {
    let single_arity = m.alphabet.len() == 1;
    let mut path = String::new();

    for &sym in word {
        let decoded = m.decode(sym);
        if single_arity && decoded[0] >= 0 && decoded[0] <= 9 {
            // `input.substring(1, input.length() - 1)` -- Java strips the `[`/`]` a
            // single-element `List.toString()` would otherwise print.
            path.push_str(&decoded[0].to_string());
        } else {
            path.push('[');
            for (i, digit) in decoded.iter().enumerate() {
                if i > 0 {
                    path.push_str(", ");
                }
                path.push_str(&digit.to_string());
            }
            path.push(']');
        }
    }

    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-test-command-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
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

    fn write_library_automaton(dir: &std::path::Path, name: &str, contents: &str) {
        fs::write(
            dir.join("Automata Library").join(format!("{name}.txt")),
            contents,
        )
        .unwrap();
    }

    // --- find_accepted: trivial automata (TestTest.java's
    //     testFindAcceptedThrowsOnUnmaterializedTrueAutomaton /
    //     testFindAcceptedEmptyOnFalseAutomaton) ---

    #[test]
    fn find_accepted_throws_on_unmaterialized_true_automaton() {
        let mut a = Automaton::true_false(true);
        let err = find_accepted(&mut a, &mut Logging::new(), 3).unwrap_err();
        assert!(matches!(err, TestError::UnmaterializedTrueAutomaton));
        assert_eq!(
            err.to_string(),
            "Cannot enumerate accepted inputs of an unmaterialized true automaton."
        );
    }

    #[test]
    fn find_accepted_empty_on_false_automaton() {
        let mut a = Automaton::true_false(false);
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 3).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn find_accepted_needed_zero_or_negative_is_empty_without_touching_the_automaton() {
        // `needed <= 0` short-circuits before the TRUE automaton's TRUE_FALSE check would
        // ever run -- so this must NOT error, unlike `find_accepted(&true_automaton, 3)`.
        let mut a = Automaton::true_false(true);
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 0).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), -5).unwrap(),
            Vec::<String>::new()
        );
    }

    // --- the two port-specific guards: the `pub`-API panic screen and the resource cap ---

    #[test]
    fn find_accepted_rejects_a_nondeterministic_fao_instead_of_panicking() {
        // A word automaton (some output > 1, so `is_fao`) that is nondeterministic: state
        // 0 goes to BOTH states on symbol 0. Feeding this through `removeLeadingZeros`'
        // `and` would reach `AutomatonDFA::require_dfa_storage`'s
        // `panic!("NFAOs are not supported..")`; Java throws a catchable
        // `WalnutException.nonDeterministicO()` there, so this must be an `Err`, never an
        // abort.
        let mut d = vec![BTreeMap::new(), BTreeMap::new()];
        d[0].insert(0, vec![0, 1]);
        d[1].insert(0, vec![1]);
        let fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 7],
            d,
            true_false: None,
        };
        let mut a = Automaton::new(fa, vec![vec![0, 1]], vec!["x".to_string()], vec![None]);
        assert!(a.is_fao() && !a.fa.is_deterministic(), "sanity");
        let err = find_accepted(&mut a, &mut Logging::new(), 1).unwrap_err();
        assert!(matches!(err, TestError::NonDeterministicO));
        assert_eq!(err.to_string(), "NFAOs are not supported..");
    }

    #[test]
    fn find_accepted_rejects_an_absurd_needed_rather_than_allocating_51gb() {
        // `Vec::with_capacity(i32::MAX as usize)` is a ~51GB allocation, which Rust
        // ABORTS (unlike Java's catchable OutOfMemoryError). See `MAX_NEEDED`.
        let mut a = accept_everything_automaton();
        let err = find_accepted(&mut a, &mut Logging::new(), i32::MAX).unwrap_err();
        assert!(matches!(
            err,
            TestError::NeededTooLarge { needed: i32::MAX }
        ));
        assert!(err.to_string().contains("1000000"), "{err}");
        // The boundary itself is accepted (it just runs, so only check it isn't rejected).
        assert!(matches!(
            find_accepted(&mut a, &mut Logging::new(), MAX_NEEDED + 1),
            Err(TestError::NeededTooLarge { .. })
        ));
    }

    // --- find_accepted: a real (non-trivial) automaton, both the shortfall and the
    //     "asks for exactly what's available" cases (mirrors
    //     TestTest.java's TestTestCommand / testFindAcceptedStopsWhenLanguageIsExhausted) ---

    /// A single-track, base-2 automaton whose language is exactly `{"0", "1"}`: state 0
    /// (non-accepting) reads either symbol to the dead-end accepting state 1, which has
    /// no outgoing transitions at all -- so `findAccepted` must exhaust the search
    /// (destinations absent once state 1 is reached) rather than loop.
    fn finite_two_word_automaton() -> Automaton {
        let mut d = vec![BTreeMap::new(), BTreeMap::new()];
        d[0].insert(0, vec![1]);
        d[0].insert(1, vec![1]);
        let fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d,
            true_false: None,
        };
        Automaton::new(fa, vec![vec![0, 1]], vec!["x".to_string()], vec![None])
    }

    #[test]
    fn find_accepted_stops_when_the_language_is_exhausted_rather_than_looping() {
        let mut a = finite_two_word_automaton();
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 5).unwrap(),
            vec!["0", "1"]
        );
    }

    #[test]
    fn find_accepted_returns_exactly_what_is_asked_for_when_enough_exists() {
        let mut a = finite_two_word_automaton();
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 1).unwrap(),
            vec!["0"]
        );
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 2).unwrap(),
            vec!["0", "1"]
        );
    }

    #[test]
    fn find_accepted_labels_the_callers_own_automaton_in_place_like_javas_random_label() {
        // Java's `M.randomLabel()` (`:42`) mutates the caller's object; only the
        // `removeLeadingZeros` rebinding is local. Pin that this port does the same.
        let mut a = finite_two_word_automaton();
        a.label = Vec::new();
        find_accepted(&mut a, &mut Logging::new(), 1).unwrap();
        assert_eq!(a.label, vec!["0".to_string()]);
    }

    /// A 1-state automaton that self-loops on its only symbol and accepts nothing. Its
    /// reachable state space is a single state, but the search would revisit it at ever
    /// growing lengths forever without `find_next_accepted_word`'s
    /// `out[1] = min(old_length + 1, |previous| + 1)` length cap (`Test.java:91`), which
    /// collapses every length past `|previous| + 1` onto one product state. So this test
    /// is a TERMINATION test: the assertion matters far less than the fact that it
    /// returns at all.
    #[test]
    fn find_accepted_terminates_on_a_self_looping_automaton_with_no_accepting_state() {
        let mut d = vec![BTreeMap::new()];
        d[0].insert(0, vec![0]);
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 1,
            o: vec![0],
            d,
            true_false: None,
        };
        let mut a = Automaton::new(fa, vec![vec![0]], vec!["x".to_string()], vec![None]);
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 3).unwrap(),
            Vec::<String>::new()
        );
    }

    /// A single-track, explicit-alphabet (`msd: None`, not an arithmetic `msd_2` track)
    /// total automaton over `{0,1}` whose one state both accepts and self-loops on every
    /// symbol: it accepts every string over `{0,1}`. Because the track has no number
    /// system, `removeLeadingZeros`' fixup is a no-op here (it only restricts arithmetic
    /// tracks), so the shortlex enumeration is simply every binary string in length-then-
    /// lexicographic order -- `0, 1, 00, 01, 10, 11, 000, ...` -- which is exactly
    /// `TestTest.testFindAcceptedRegression`'s own expected list (confirming its real
    /// `findAcceptedRegression.txt` fixture is this same non-arithmetic shape, not an
    /// `msd_2` one).
    fn accept_everything_automaton() -> Automaton {
        let mut d = vec![BTreeMap::new()];
        d[0].insert(0, vec![0]);
        d[0].insert(1, vec![0]);
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1],
            d,
            true_false: None,
        };
        Automaton::new(fa, vec![vec![0, 1]], vec!["x".to_string()], vec![None])
    }

    #[test]
    fn find_accepted_regression_matches_javas_shortlex_enumeration() {
        // `TestTest.testFindAcceptedRegression`'s own expected list, verbatim -- see
        // `accept_everything_automaton`'s doc comment for why a hand-built equivalent
        // reproduces it exactly without shipping the real `.txt` fixture.
        let mut a = accept_everything_automaton();
        let expected: Vec<&str> =
            vec!["0", "1", "00", "01", "10", "11", "000", "001", "010", "011"];
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 10).unwrap(),
            expected
        );
    }

    // --- the `removeLeadingZeros` pre-pass (`Test.java:43`), the one part of
    //     `findAccepted` that every `msd: None` test above leaves as a no-op ---

    #[test]
    fn find_accepted_runs_the_leading_zero_pre_pass_on_an_arithmetic_track() {
        // `accept_everything_automaton`'s transition table exactly, but with the track
        // declared msd (an arithmetic base-2 track) instead of an explicit alphabet. The
        // language is still every binary string, but `removeLeadingZeros` now restricts
        // the enumeration to one representation per VALUE: no listed word may start with
        // 0 (`ε`, the leading-zero-free encoding of 0, is excluded separately by the
        // search's own `state[1] != 0` non-empty requirement). Deleting the pre-pass
        // turns this back into `accept_everything_automaton`'s `0, 1, 00, ...` list,
        // which is what makes this the regression test for it.
        let mut a = accept_everything_automaton();
        a.msd = vec![Some(true)];
        let expected: Vec<&str> = vec!["1", "10", "11", "100", "101", "110", "111", "1000"];
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 8).unwrap(),
            expected
        );
    }

    #[test]
    fn find_accepted_on_hard_inf_test_matches_javas_expected_words() {
        // `TestTest.TestTestCommand`, ported verbatim -- the ONLY case in Java's own
        // suite that exercises `removeLeadingZeros` through `findAccepted`, on its real
        // 284-state `msd_2` fixture. That fixture is copied BYTE-FOR-BYTE from
        // `walnut-java/src/test/resources/unitTests/hardInfTest.txt` (GPLv3, Walnut --
        // see this repo's `NOTICE`) to `crates/wr-cli/tests/fixtures/hardInfTest.txt`;
        // do not hand-edit it, or the expected words below stop being Java's.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("hardInfTest.txt");
        let read = || {
            AutomatonDFA::from(wr_io::reader::read_automaton_txt(&path).unwrap()).into_automaton()
        };
        assert_eq!(
            find_accepted(&mut read(), &mut Logging::new(), 0).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            find_accepted(&mut read(), &mut Logging::new(), 1).unwrap(),
            vec!["101"]
        );
        assert_eq!(
            find_accepted(&mut read(), &mut Logging::new(), 2).unwrap(),
            vec!["101", "1010"]
        );
    }

    // --- find_accepted: multi-track (bracket-preserving) formatting, mirrors
    //     TestTest.testFindAcceptedKeepsBracketsForMultiArityInput ---

    #[test]
    fn find_accepted_keeps_brackets_for_multi_arity_input() {
        // 2-track automaton, single accepting state, total self-loop on all 4 symbol
        // pairs (`MULTI_ARITY_AUTOMATON` in TestTest.java).
        let mut d = vec![BTreeMap::new()];
        for sym in 0..4 {
            d[0].insert(sym, vec![0]);
        }
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 4,
            o: vec![1],
            d,
            true_false: None,
        };
        let mut a = Automaton::new(
            fa,
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![None, None],
        );
        let accepted = find_accepted(&mut a, &mut Logging::new(), 2).unwrap();
        assert_eq!(accepted.len(), 2);
        for s in &accepted {
            assert!(s.starts_with('[') && s.ends_with(']'), "{s}");
        }
        // Java's own assertion (above) only checks the brackets, which leaves the tuple
        // SEPARATOR unpinned -- `List.toString()` joins with ", ", and dropping the space
        // passes every bracket-only check. These exact values are the real
        // `walnut-java` output for this automaton.
        assert_eq!(accepted, vec!["[0, 0]", "[1, 0]"]);
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 4).unwrap(),
            vec!["[0, 0]", "[1, 0]", "[0, 1]", "[1, 1]"]
        );
    }

    #[test]
    fn find_accepted_brackets_single_arity_digits_outside_zero_through_nine() {
        // `formatAcceptedWord`'s guard is `singleArity && 0 <= d && d <= 9` (`:122`), not
        // just `singleArity`: a single-track alphabet may legally contain negative or
        // multi-digit values, and those keep their brackets so the output stays
        // unambiguous (`[10]`, not a `1` followed by a `0`). With an all-0..9 alphabet
        // the guard's two bounds are dead, which is what leaves them untested elsewhere.
        let mut d = vec![BTreeMap::new()];
        for sym in 0..3 {
            d[0].insert(sym, vec![0]);
        }
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 3,
            o: vec![1],
            d,
            true_false: None,
        };
        let mut a = Automaton::new(fa, vec![vec![-1, 0, 10]], vec!["x".to_string()], vec![None]);
        assert_eq!(
            find_accepted(&mut a, &mut Logging::new(), 3).unwrap(),
            vec!["[-1]", "0", "[10]"]
        );
    }

    // --- test_command_to: end to end through Session, mirrors
    //     TestTest.testTestCommandReportsShortfallAndSuccess ---

    #[test]
    fn test_command_reports_shortfall_and_success() {
        let (session, dir) = temp_session("shortfall-and-success");
        write_library_automaton(
            &dir,
            "finiteTwoWord",
            "{0,1}\n\n0 0\n0 -> 1\n1 -> 1\n\n1 1\n",
        );

        let mut out = Vec::new();
        let ok =
            test_command_to(&session, &mut Logging::new(), "finiteTwoWord", 5, &mut out).unwrap();
        assert!(!ok, "only 2 inputs exist; asking for 5 must report false");
        let printed = String::from_utf8(out).unwrap();
        assert_eq!(
            printed,
            "finiteTwoWord only accepts 2 inputs, which are as follows: \n0\n1\n"
        );

        let mut out2 = Vec::new();
        let ok2 =
            test_command_to(&session, &mut Logging::new(), "finiteTwoWord", 1, &mut out2).unwrap();
        assert!(ok2, "asking for no more than what's accepted returns true");
        let printed2 = String::from_utf8(out2).unwrap();
        assert_eq!(printed2, "0\n", "no shortfall message when needed is met");

        fs::remove_dir_all(&dir).ok();
    }
}
