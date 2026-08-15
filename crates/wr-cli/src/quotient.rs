// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Quotient.java` (25 LOC) — U23, batch A. `rightquo`/`leftquo`, both a
//! read-read-compute-write triple over the already-ported
//! [`wr_core::logicalops::right_quotient`]/[`wr_core::logicalops::left_quotient`].

use wr_core::automaton::Automaton;
use wr_core::logicalops::{left_quotient, right_quotient};
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_ops::read_from_automata_library;
use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure [`right_quotient_command`]/[`left_quotient_command`] can produce.
#[derive(Debug)]
pub enum QuotientError {
    /// `Automaton.readAutomatonFromFile` failed for either operand.
    Read(PredicateEnvError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for QuotientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuotientError::Read(e) => write!(f, "{e}"),
            QuotientError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for QuotientError {}

impl From<std::io::Error> for QuotientError {
    fn from(e: std::io::Error) -> Self {
        QuotientError::Io(e)
    }
}

fn read_pair(
    session: &Session,
    old_name1: &str,
    old_name2: &str,
) -> Result<(Automaton, Automaton), QuotientError> {
    let m1 = read_from_automata_library(session, old_name1).map_err(QuotientError::Read)?;
    let m2 = read_from_automata_library(session, old_name2).map_err(QuotientError::Read)?;
    Ok((m1, m2))
}

/// `Quotient.rightQuotient(String s, String oldName1, String oldName2, String newName)`
/// (`Quotient.java:9-15`).
///
/// `right_quotient`'s third parameter (`skip_subset_check`) is hardcoded `false` here,
/// matching Java's own `AutomatonLogicalOps.rightQuotient(M1, M2, false)` call — the real,
/// non-`skip` subset-alphabet guard runs.
pub fn right_quotient_command(
    session: &Session,
    s: &str,
    old_name1: &str,
    old_name2: &str,
    new_name: &str,
) -> Result<TestCase, QuotientError> {
    let (m1, m2) = read_pair(session, old_name1, old_name2)?;
    let mut c = right_quotient(&m1, &m2, false);
    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(c))
}

/// `Quotient.leftQuotient(String s, String oldName1, String oldName2, String newName)`
/// (`Quotient.java:17-23`).
pub fn left_quotient_command(
    session: &Session,
    s: &str,
    old_name1: &str,
    old_name2: &str,
    new_name: &str,
) -> Result<TestCase, QuotientError> {
    let (m1, m2) = read_pair(session, old_name1, old_name2)?;
    let mut c = left_quotient(&m1, &m2);
    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-quotient-{tag}-{}-{}",
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

    /// A single-track `msd_2` automaton whose language is exactly `{word}` where `word`
    /// is the given single symbol.
    fn single_symbol_automaton(symbol: i32) -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(symbol, vec![1]);
        Automaton::new(
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
        )
    }

    /// A two-symbol-word automaton accepting exactly `"01"`.
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

    fn write_library_automaton(dir: &std::path::Path, name: &str, mut a: Automaton) {
        let path = dir.join("Automata Library").join(format!("{name}.txt"));
        wr_io::writer::write_automaton_txt(&mut a, &path).unwrap();
    }

    #[test]
    fn right_quotient_of_zero_one_by_one_gives_zero() {
        let (session, dir) = temp_session("right");
        write_library_automaton(&dir, "A", accepts_zero_one());
        write_library_automaton(&dir, "B", single_symbol_automaton(1));

        let tc = right_quotient_command(&session, "rightquo c A B;", "A", "B", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(
            c.fa.accepts_word(&[0]),
            "\"01\" with \"1\" quotiented off the right is \"0\""
        );
        assert!(!c.fa.accepts_word(&[0, 1]));
        assert!(dir.join("Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn left_quotient_of_zero_one_by_zero_gives_one() {
        // `left_quotient(a, b) = { z : ∃w ∈ L(b), wz ∈ L(a) }` (`wr_core::logicalops
        // ::left_quotient`'s own doc comment) -- so `a` must be the LARGER set ("01")
        // being stripped, `b` the prefix language ("0") stripped off it.
        let (session, dir) = temp_session("left");
        write_library_automaton(&dir, "A", accepts_zero_one());
        write_library_automaton(&dir, "B", single_symbol_automaton(0));

        let tc = left_quotient_command(&session, "leftquo c A B;", "A", "B", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(
            c.fa.accepts_word(&[1]),
            "\"01\" with \"0\" quotiented off the left is \"1\""
        );
        assert!(!c.fa.accepts_word(&[0, 1]));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn right_quotient_propagates_a_missing_file_read_error() {
        let (session, dir) = temp_session("missing");
        write_library_automaton(&dir, "A", accepts_zero_one());
        let err = right_quotient_command(&session, "rightquo c A B;", "A", "B", "c").unwrap_err();
        assert!(matches!(err, QuotientError::Read(_)));
        fs::remove_dir_all(&dir).ok();
    }
}
