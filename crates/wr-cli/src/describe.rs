// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Describe.java` (31 LOC) — U23, batch A. The one command in this batch
//! that writes NOTHING: it reads an already-saved automaton and logs five facts about
//! it, returning them via [`crate::test_case::TestCase`]'s log-only constructor shape
//! (`matrix_addresses`/`gv_address` both Java `null`, mapped the same way
//! `crate::eval_def`'s headless mode already establishes for "null" — an empty `Vec`/
//! empty `String`, since [`TestCase::graph_viz`]/[`TestCase::matrix_output`] treat both
//! the same as their real `null` checks).
//!
//! [`TestCase::graph_viz`]: crate::test_case::TestCase::graph_viz
//! [`TestCase::matrix_output`]: crate::test_case::TestCase::matrix_output

use wr_core::automaton::AutomatonDFA;
use wr_core::logging::Logging;
use wr_io::reader::{read_comments, ReadError};
use wr_logic::predicate_env::PredicateEnvError;

use crate::prover_helper::determine_in_library;
use crate::session::Session;
use crate::test_case::{AutomatonFilenamePair, TestCase, DEFAULT_TESTFILE};

/// Every failure [`describe`] can produce.
#[derive(Debug)]
pub enum DescribeError {
    /// `new AutomatonDFA(inLibrary)` failed reading/determinizing the input automaton.
    Read(PredicateEnvError),
    /// `AutomatonReader.readComments(inLibrary)` failed.
    Comments(ReadError),
}

impl std::fmt::Display for DescribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DescribeError::Read(e) => write!(f, "{e}"),
            DescribeError::Comments(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DescribeError {}

/// The `List<NumberSystem>.toString()` rendering Java's `"Number systems:" +
/// M.getNS()` produces — `[msd_2, null]`-style, via
/// [`wr_core::automaton::Automaton::track_ns_names`], which reports each track's real
/// `NumberSystem.getName()` (so a custom-base automaton prints `[msd_fib]`, not the
/// `[msd_2]` its alphabet cardinality alone would suggest).
fn ns_list_display(names: &[Option<String>]) -> String {
    let parts: Vec<String> = names
        .iter()
        .map(|n| n.clone().unwrap_or_else(|| "null".to_string()))
        .collect();
    format!("[{}]", parts.join(", "))
}

/// `Describe.describe(boolean isDFAO, String inFileName)` (`Describe.java:15-30`).
pub fn describe(
    session: &Session,
    logging: &mut Logging,
    is_dfao: bool,
    in_file_name: &str,
) -> Result<TestCase, DescribeError> {
    let in_library = determine_in_library(session.paths(), is_dfao, in_file_name);

    // `AutomatonDFA M = new AutomatonDFA(inLibrary);` (`:18`) -- read, then require DFA
    // storage (auto-determinize if needed), matching `AutomatonDFA`'s file constructor.
    let automaton = session
        .libraries()
        .read_library_automaton(&in_library)
        .map_err(DescribeError::Read)?;
    let dfa = AutomatonDFA::from(automaton);
    let m = dfa.automaton();

    logging.log_message_with(true, &format!("File location: {in_library}"));

    // `String comments = AutomatonReader.readComments(inLibrary);` (`:21`).
    let comments = read_comments(&in_library).map_err(DescribeError::Comments)?;
    logging.log_message_with(true, &format!("Comments: {comments}"));

    logging.log_message_with(true, &format!("State count:{}", m.fa.q));
    logging.log_message_with(
        true,
        &format!("Transition count:{}", m.fa.determine_transition_count()),
    );
    logging.log_message_with(true, &format!("Alphabet size:{}", m.fa.alphabet_size));
    logging.log_message_with(
        true,
        &format!("Number systems:{}", ns_list_display(&m.track_ns_names())),
    );

    Ok(TestCase::new(
        "",
        Vec::new(),
        "",
        logging.command_log(),
        vec![AutomatonFilenamePair::new(
            dfa.into_automaton(),
            DEFAULT_TESTFILE,
        )],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::automaton::Automaton;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-describe-{tag}-{}-{}",
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

    fn contains_one() -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![1]);
        d1.insert(1, vec![1]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![d0, d1],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    #[test]
    fn describe_logs_every_fact_and_returns_the_automaton() {
        let (session, dir) = temp_session("basic");
        let path = dir.join("Automata Library").join("A.txt");
        let mut a = contains_one();
        wr_io::writer::write_automaton_txt(&mut a, &path).unwrap();

        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let tc = describe(&session, &mut logging, false, "A.txt").unwrap();

        assert_eq!(tc.graph_viz().unwrap(), "", "describe writes no .gv file");
        assert_eq!(
            tc.matrix_output().unwrap(),
            vec![
                "".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string()
            ]
        );
        assert!(tc.automaton_pairs()[0].automaton().is_some());

        assert!(tc.details().contains("File location:"));
        assert!(tc.details().contains("Comments:"));
        assert!(tc.details().contains("State count:2"));
        // The one line fed by `Fa::determine_transition_count`: 2 states x 2 symbols,
        // every destination list of length 1, so 4 — NOT 2 (states) and not 2 (keys per
        // state), the two values a wrong summation would produce.
        assert!(
            tc.details().contains("Transition count:4"),
            "details were: {}",
            tc.details()
        );
        assert!(tc.details().contains("Alphabet size:2"));
        assert!(tc.details().contains("Number systems:[msd_2]"));

        fs::remove_dir_all(&dir).ok();
    }

    /// U23 review fix, finding #4: `"Number systems:"` reconstructed the name from
    /// `(msd, alphabet.len())`, so a custom base printed as the plain base with the same
    /// alphabet cardinality (`[msd_2]` for `msd_fib`). Same root cause as the fail-open
    /// `isNSDiffering` guard; fixed by threading the real `NumberSystem.getName()`.
    #[test]
    fn describe_prints_a_custom_bases_real_name_not_the_base_k_lookalike() {
        let (session, dir) = temp_session("custom-base");
        // The real shipped `msd_fib` files — see `crates/wr-io/tests/fixtures/ATTRIBUTION.md`.
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wr-io/tests/fixtures")
            .canonicalize()
            .unwrap();
        for name in ["msd_fib.txt", "msd_fib_addition.txt"] {
            fs::copy(fixtures.join(name), dir.join("Custom Bases").join(name)).unwrap();
        }
        fs::write(
            dir.join("Automata Library").join("F.txt"),
            "msd_fib\n\n0 1\n0 -> 0\n1 -> 0\n",
        )
        .unwrap();

        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let tc = describe(&session, &mut logging, false, "F.txt").unwrap();
        assert!(
            tc.details().contains("Number systems:[msd_fib]"),
            "details were: {}",
            tc.details()
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn describe_propagates_a_missing_file_read_error() {
        let (session, dir) = temp_session("missing");
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let err = describe(&session, &mut logging, false, "nope.txt").unwrap_err();
        assert!(matches!(err, DescribeError::Read(_)));
        fs::remove_dir_all(&dir).ok();
    }
}
