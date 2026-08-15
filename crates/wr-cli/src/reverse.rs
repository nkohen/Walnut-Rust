// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Reverse.java` (21 LOC) — U23, batch A. A thin dispatch over two
//! already-ported primitives, chosen by the command's own `$`-flag: a genuine word
//! automaton (DFAO) reverses through [`wr_core::word_automaton::reverse_with_output`],
//! a plain predicate automaton through [`wr_core::logicalops::reverse`]. Both always pass
//! `true` for the "reverse the msd/lsd direction too" flag (`Reverse.reverseCommand`
//! itself never varies it), matching Java's own two call sites exactly.

use wr_core::automaton::Automaton;
use wr_core::logicalops::reverse as reverse_predicate;
use wr_core::word_automaton::reverse_with_output;
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_output::write_automata;
use crate::prover_helper::{determine_in_library, determine_out_library};
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure [`reverse_command`] can produce.
#[derive(Debug)]
pub enum ReverseError {
    /// `new Automaton(ProverHelper.determineInLibrary(isDFAO, inFileName))` failed.
    Read(PredicateEnvError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for ReverseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReverseError::Read(e) => write!(f, "{e}"),
            ReverseError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ReverseError {}

impl From<std::io::Error> for ReverseError {
    fn from(e: std::io::Error) -> Self {
        ReverseError::Io(e)
    }
}

/// `Reverse.reverseCommand(String s, String inFileName, boolean isDFAO, String newName)`
/// (`Reverse.java:10-19`). `in_file_name` already carries the `.txt` extension (the
/// caller's dispatch arm builds it, matching `Prover.reverseCommand`'s own
/// `m.group(GROUP_REVERSE_OLD_NAME) + TXT_EXTENSION` — see `crate::alphabet::alphabet_command`
/// for the same established convention).
pub fn reverse_command(
    session: &Session,
    s: &str,
    in_file_name: &str,
    is_dfao: bool,
    new_name: &str,
) -> Result<TestCase, ReverseError> {
    let in_address = determine_in_library(session.paths(), is_dfao, in_file_name);
    let mut m: Automaton = session
        .libraries()
        .read_library_automaton(&in_address)
        .map_err(ReverseError::Read)?;

    if is_dfao {
        // `WordAutomaton.reverseWithOutput(M, true);` (`:13`).
        reverse_with_output(&mut m, true);
    } else {
        // `AutomatonLogicalOps.reverse(M, true);` (`:15`).
        reverse_predicate(&mut m, true);
    }

    let out_library = determine_out_library(session.paths(), is_dfao);
    write_automata(session, &mut m, s, &out_library, new_name, true)?;
    Ok(TestCase::from_automaton(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-reverse-{tag}-{}-{}",
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

    /// A single-track `msd_2` automaton accepting exactly `"01"` (q0 --0--> q1 --1--> q2,
    /// only q2 accepting).
    fn accepts_zero_one() -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(1, vec![2]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 0, 1],
                d: vec![d0, d1, BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    #[test]
    fn reverse_of_a_predicate_automaton_reverses_the_language() {
        let (session, dir) = temp_session("predicate");
        let mut a = accepts_zero_one();
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("A.txt"))
            .unwrap();

        let tc = reverse_command(&session, "reverse c $A;", "A.txt", false, "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[1, 0]), "reverse of \"01\" is \"10\"");
        assert!(!c.fa.accepts_word(&[0, 1]));

        assert!(dir.join("Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reverse_of_a_dfao_writes_into_the_word_library() {
        let (session, dir) = temp_session("dfao");
        let mut a = accepts_zero_one();
        // Give it a genuine word-automaton output (> 1) so `is_dfao` round-trips
        // meaningfully -- state 2's output becomes 5 instead of the plain 0/1 flag.
        a.fa.o[2] = 5;
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Word Automata Library").join("A.txt"))
            .unwrap();

        let tc = reverse_command(&session, "reverse c A;", "A.txt", true, "c").unwrap();
        assert!(tc.automaton_pairs()[0].automaton().is_some());
        assert!(dir.join("Word Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reverse_propagates_a_missing_file_read_error() {
        let (session, dir) = temp_session("missing");
        let err = reverse_command(&session, "reverse c $A;", "A.txt", false, "c").unwrap_err();
        assert!(matches!(err, ReverseError::Read(_)));
        fs::remove_dir_all(&dir).ok();
    }
}
