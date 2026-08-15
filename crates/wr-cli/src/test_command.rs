// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Test.java` (129 LOC) — the `test` command: "find the first N
//! shortlex-smallest non-empty inputs accepted by a given automaton", used to sanity
//! check an automaton by manual inspection.
//!
//! # `ProductBFS`, and what `wr_core::search` already did for this unit
//!
//! Java's `findNextAcceptedWord` (`:71-95`) drives `Automata.Search.ProductBFS`'s
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
//! `Test.findNextAcceptedWord`'s step closure (`:86`) just takes
//! `destinations.getInt(0)` with no determinism check at all, trusting its caller
//! (`Test.testCommand`, via `AutomatonDFA.readAutomatonDFAFromFile`) to have already
//! guaranteed a DFA. [`find_next_accepted_word`] below is a direct, mechanical port of
//! that literal indexing — it does not add the stricter guard, matching
//! `findAccepted`'s own public signature (`Automaton M`, not `AutomatonDFA M` —
//! `TestTest.java` calls it directly with plain `Automaton`s built by hand). The real
//! CLI path ([`test_command`]) still gets the determinism guarantee, the same way Java's
//! does: by loading through [`wr_core::automaton::AutomatonDFA::from`].

use std::io::{self, Write};

use wr_core::automaton::{Automaton, AutomatonDFA};
use wr_core::logicalops::{remove_leading_zeros, RemoveLeadingZerosError};
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
    /// Propagated from `AutomatonLogicalOps.removeLeadingZeros` (`Test.java:41-42`).
    RemoveLeadingZeros(RemoveLeadingZerosError),
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
/// `needed` inputs were found. Writes to the real process stdout; see
/// [`test_command_to`] for the injectable-sink form the CLI dispatch layer and this
/// module's own tests use.
pub fn test_command(session: &Session, test_name: &str, needed: i32) -> Result<bool, TestError> {
    test_command_to(session, test_name, needed, &mut io::stdout())
}

/// As [`test_command`], with an injectable sink for the console output — the same seam
/// `crate::prover_helper`'s `_to` functions use.
pub fn test_command_to(
    session: &Session,
    test_name: &str,
    needed: i32,
    stdout: &mut dyn Write,
) -> Result<bool, TestError> {
    // `AutomatonDFA M = AutomatonDFA.readAutomatonDFAFromFile(testName);` (`:23`).
    let address = session
        .paths()
        .read_file_for_automata_library(&format!("{test_name}{}", crate::prover::TXT_EXTENSION));
    let automaton = session.libraries().read_library_automaton(&address)?;
    let dfa = AutomatonDFA::from(automaton);

    let accepted = find_accepted(dfa.automaton(), needed)?;

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

/// `Test.findAccepted(Automaton M, int needed)` (`:34-59`) — public, like Java's, since
/// `TestTest.java` (and this module's own tests) call it directly with hand-built
/// [`Automaton`]s, not only through [`test_command`]'s file-loading path.
pub fn find_accepted(a: &Automaton, needed: i32) -> Result<Vec<String>, TestError> {
    if needed <= 0 {
        return Ok(Vec::new());
    }
    let needed = needed as usize;

    // "We do not want to count multiple representations of the same value as distinct
    // accepted values. This preserves the existing behavior that skips representations
    // beginning with 0 (or [0,0], etc., for higher-arity numeric inputs)." (`:39-42`).
    let mut m = a.clone();
    m.random_label();
    let labels = m.label.clone();
    let m = remove_leading_zeros(&m, &labels)?;

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

/// `Test.findNextAcceptedWord(Automaton M, Word<Integer> previous)` (`:71-95`) — see this
/// module's docs for why it is a direct 3-tuple port rather than a reuse of
/// [`wr_core::search::shortest_accepted_word`].
///
/// Product state layout, verbatim from Java's doc comment (`:63-68`):
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

    // `int[] start = { M.fa.getQ0(), 0, 0 };` (`:78`).
    let start = [m.fa.q0 as i32, 0, 0];
    let prev_len_plus_one = previous.map_or(1, |p| p.len() as i32 + 1);

    Ok(shortest_witness_word_int(
        &start,
        m.fa.alphabet_size,
        |state, symbol, out| {
            // `IntList destinations = M.fa.getT().getNfaStateDests(state[0], symbol); if
            // (destinations == null || destinations.isEmpty()) return false;` (`:82-84`).
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
        let a = Automaton::true_false(true);
        let err = find_accepted(&a, 3).unwrap_err();
        assert!(matches!(err, TestError::UnmaterializedTrueAutomaton));
        assert_eq!(
            err.to_string(),
            "Cannot enumerate accepted inputs of an unmaterialized true automaton."
        );
    }

    #[test]
    fn find_accepted_empty_on_false_automaton() {
        let a = Automaton::true_false(false);
        assert_eq!(find_accepted(&a, 3).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn find_accepted_needed_zero_or_negative_is_empty_without_touching_the_automaton() {
        // `needed <= 0` short-circuits before the TRUE automaton's TRUE_FALSE check would
        // ever run -- so this must NOT error, unlike `find_accepted(&true_automaton, 3)`.
        let a = Automaton::true_false(true);
        assert_eq!(find_accepted(&a, 0).unwrap(), Vec::<String>::new());
        assert_eq!(find_accepted(&a, -5).unwrap(), Vec::<String>::new());
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
        let a = finite_two_word_automaton();
        assert_eq!(find_accepted(&a, 5).unwrap(), vec!["0", "1"]);
    }

    #[test]
    fn find_accepted_returns_exactly_what_is_asked_for_when_enough_exists() {
        let a = finite_two_word_automaton();
        assert_eq!(find_accepted(&a, 1).unwrap(), vec!["0"]);
        assert_eq!(find_accepted(&a, 2).unwrap(), vec!["0", "1"]);
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
        let a = accept_everything_automaton();
        let expected: Vec<&str> =
            vec!["0", "1", "00", "01", "10", "11", "000", "001", "010", "011"];
        assert_eq!(find_accepted(&a, 10).unwrap(), expected);
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
        let a = Automaton::new(
            fa,
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![None, None],
        );
        let accepted = find_accepted(&a, 2).unwrap();
        assert_eq!(accepted.len(), 2);
        for s in &accepted {
            assert!(s.starts_with('[') && s.ends_with(']'), "{s}");
        }
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
        let ok = test_command_to(&session, "finiteTwoWord", 5, &mut out).unwrap();
        assert!(!ok, "only 2 inputs exist; asking for 5 must report false");
        let printed = String::from_utf8(out).unwrap();
        assert_eq!(
            printed,
            "finiteTwoWord only accepts 2 inputs, which are as follows: \n0\n1\n"
        );

        let mut out2 = Vec::new();
        let ok2 = test_command_to(&session, "finiteTwoWord", 1, &mut out2).unwrap();
        assert!(ok2, "asking for no more than what's accepted returns true");
        let printed2 = String::from_utf8(out2).unwrap();
        assert_eq!(printed2, "0\n", "no shortfall message when needed is met");

        fs::remove_dir_all(&dir).ok();
    }
}
