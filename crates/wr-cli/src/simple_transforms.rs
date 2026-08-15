// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Prover.java`'s inline `minimizeCommand` (`:712-722`), `fixLeadZeroCommand`
//! (`:747-753`), `fixTrailZeroCommand` (`:756-762`) — U23, batch A. Three read-mutate-write
//! triples, each over a single already-ported primitive:
//! [`wr_core::word_automaton::minimize_self_with_output`],
//! [`wr_core::logicalops::fix_leading_zeros_problem`],
//! [`wr_core::logicalops::fix_trailing_zeros_problem`]. Grouped into one module (rather
//! than three) because, unlike the `Main/Commands/*.java` files, these never had their
//! own Java class or file to begin with — they are inline `Prover` methods, and this
//! module is their equally small Rust home.

use wr_core::logicalops::{fix_leading_zeros_problem, fix_trailing_zeros_problem};
use wr_core::numsys::TXT_EXTENSION;
use wr_core::word_automaton::minimize_self_with_output;
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_ops::read_from_automata_library;
use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure the three commands in this module can produce.
#[derive(Debug)]
pub enum SimpleTransformError {
    /// The input automaton couldn't be read.
    Read(PredicateEnvError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for SimpleTransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimpleTransformError::Read(e) => write!(f, "{e}"),
            SimpleTransformError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SimpleTransformError {}

impl From<std::io::Error> for SimpleTransformError {
    fn from(e: std::io::Error) -> Self {
        SimpleTransformError::Io(e)
    }
}

/// `Prover.minimizeCommand(String s)` (`Prover.java:712-722`). Reads/writes
/// `Word Automata Library/`, not `Automata Library/` — the one command in this module
/// that does, matching Java's `Session.getReadFileForWordsLibrary`/
/// `getWriteAddressForWordsLibrary` exactly.
pub fn minimize_command(
    session: &Session,
    s: &str,
    old_name: &str,
    new_name: &str,
) -> Result<TestCase, SimpleTransformError> {
    let address = session
        .paths()
        .read_file_for_words_library(&format!("{old_name}{TXT_EXTENSION}"));
    let mut m = session
        .libraries()
        .read_library_automaton(&address)
        .map_err(SimpleTransformError::Read)?;

    // `WordAutomaton.minimizeSelfWithOutput(M);` (`:718`).
    minimize_self_with_output(&mut m);

    write_automata(
        session,
        &mut m,
        s,
        &session.paths().write_address_for_words_library(),
        new_name,
        true,
    )?;
    Ok(TestCase::from_automaton(m))
}

/// `Prover.fixLeadZeroCommand(String s)` (`Prover.java:747-753`).
pub fn fix_lead_zero_command(
    session: &Session,
    s: &str,
    old_name: &str,
    new_name: &str,
) -> Result<TestCase, SimpleTransformError> {
    let mut m =
        read_from_automata_library(session, old_name).map_err(SimpleTransformError::Read)?;

    // `AutomatonLogicalOps.fixLeadingZerosProblem(M);` (`:750`).
    fix_leading_zeros_problem(&mut m);

    write_automata(
        session,
        &mut m,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(m))
}

/// `Prover.fixTrailZeroCommand(String s)` (`Prover.java:756-762`).
pub fn fix_trail_zero_command(
    session: &Session,
    s: &str,
    old_name: &str,
    new_name: &str,
) -> Result<TestCase, SimpleTransformError> {
    let mut m =
        read_from_automata_library(session, old_name).map_err(SimpleTransformError::Read)?;

    // `AutomatonLogicalOps.fixTrailingZerosProblem(M);` (`:759`).
    fix_trailing_zeros_problem(&mut m);

    write_automata(
        session,
        &mut m,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(m))
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
            "wr-cli-simple-transforms-{tag}-{}-{}",
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

    /// Non-minimal (two equivalent accepting states) `msd_2` predicate automaton
    /// accepting exactly the words containing a `1`.
    fn non_minimal_contains_one() -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        d1.insert(1, vec![1]);
        let mut d2 = BTreeMap::new();
        d2.insert(0, vec![2]);
        d2.insert(1, vec![1]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 1, 1],
                d: vec![d0, d1, d2],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    #[test]
    fn minimize_command_shrinks_a_redundant_word_automaton() {
        let (session, dir) = temp_session("minimize");
        let path = dir.join("Word Automata Library").join("A.txt");
        let mut a = non_minimal_contains_one();
        wr_io::writer::write_automaton_txt(&mut a, &path).unwrap();

        let tc = minimize_command(&session, "minimize c A;", "A", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.q <= 2, "states 1 and 2 are language-equivalent");
        assert!(c.fa.accepts_word(&[0, 1, 0]));
        assert!(!c.fa.accepts_word(&[0, 0, 0]));
        assert!(dir.join("Word Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fix_lead_zero_command_closes_under_prepended_zeros() {
        let (session, dir) = temp_session("fixlead");
        // Accepts exactly "1" -- a leading-zero fixup must additionally accept "01",
        // "001", etc.
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![1]);
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![d0, BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("A.txt"))
            .unwrap();

        let tc = fix_lead_zero_command(&session, "fixleadzero c A;", "A", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[1]));
        assert!(c.fa.accepts_word(&[0, 1]));
        assert!(c.fa.accepts_word(&[0, 0, 1]));
        assert!(!c.fa.accepts_word(&[1, 0]));
        assert!(dir.join("Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fix_trail_zero_command_admits_a_word_whose_zero_padding_was_already_accepted() {
        // `fix_trailing_zeros_problem` is NOT a forward "append a zero, stay accepted"
        // closure (that is `fix_leading_zeros_problem`'s prepend-zero shape, on the
        // other end) -- it marks a state accepting whenever it can REACH an already-
        // accepting state via a chain of zero transitions, i.e. `L(fixed) = { w : ∃k≥0,
        // w·0^k ∈ L(original) }` (a right quotient by `0*`). So build an automaton
        // accepting exactly "10"; the fixup must then ALSO accept "1" (since
        // "1"+"0" = "10" ∈ L(original)), while leaving "10" itself accepted and adding
        // nothing else.
        let (session, dir) = temp_session("fixtrail");
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        let mut a = Automaton::new(
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
        );
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("A.txt"))
            .unwrap();

        let tc = fix_trail_zero_command(&session, "fixtrailzero c A;", "A", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[1, 0]), "\"10\" stays accepted");
        assert!(
            c.fa.accepts_word(&[1]),
            "\"1\" must gain acceptance: \"1\"+\"0\" = \"10\" was already accepted"
        );
        assert!(
            !c.fa.accepts_word(&[]),
            "the empty word must still be rejected"
        );
        assert!(!c.fa.accepts_word(&[0, 1]), "\"01\" must still be rejected");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fix_lead_zero_command_propagates_a_missing_file_read_error() {
        let (session, dir) = temp_session("missing");
        let err = fix_lead_zero_command(&session, "fixleadzero c A;", "A", "c").unwrap_err();
        assert!(matches!(err, SimpleTransformError::Read(_)));
        fs::remove_dir_all(&dir).ok();
    }
}
