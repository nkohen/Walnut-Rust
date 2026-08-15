// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Reg.java` (77 LOC) — the `reg` command's dispatch logic: CLI-layer
//! glue around `wr_core::regex`'s already-shipped regex engine (U8).
//!
//! `Reg.determineEncodedRegex` (`Reg.java:42-76`) is not re-ported here — it already IS
//! [`wr_core::regex::determine_encoded_regex`] (see that function's own doc comment,
//! which cites `Reg.java` directly), including its WB-024 collision with dk.brics'
//! reserved-character range. This module is what's left once that piece is factored
//! out: parsing `listOfAlphabets` (via `crate::alphabet::determine_alphabets_and_ns`,
//! since `Reg.reg` itself calls `Alphabet.determineAlphabetsAndNS`, `Reg.java:20`),
//! building the automaton, installing its per-track number systems, and writing the
//! result.
//!
//! # `Reg.reg`'s throwaway `AutomatonDFA M`
//!
//! Java's `reg()` builds a short-lived `AutomatonDFA M` purely to reach
//! `M.richAlphabet`/`M.getAlphabetSize()` (`Reg.java:24-27`) before constructing the
//! real result `R`. [`wr_core::automaton::AutomatonDFA::from_encoded_regex`] already
//! collapses that: it takes the track list directly and derives the alphabet size
//! itself (see that function's own doc comment: "same end state, one fewer invalid
//! intermediate"), so this module never constructs an `M`-equivalent at all.
use wr_core::automaton::AutomatonDFA;
use wr_core::regex::{determine_encoded_regex, RegexError};

use crate::alphabet::{all_reps_from_ns, determine_alphabets_and_ns, AlphabetError};
use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure `reg` can produce.
#[derive(Debug)]
pub enum RegError {
    /// Propagated from `Alphabet.determineAlphabetsAndNS` (`crate::alphabet`).
    Alphabet(AlphabetError),
    /// `Reg.determineEncodedRegex`/the Brics engine (`wr_core::regex`).
    Regex(RegexError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for RegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegError::Alphabet(e) => write!(f, "{e}"),
            RegError::Regex(e) => write!(f, "{e}"),
            RegError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RegError {}

impl From<AlphabetError> for RegError {
    fn from(e: AlphabetError) -> Self {
        RegError::Alphabet(e)
    }
}

impl From<RegexError> for RegError {
    fn from(e: RegexError) -> Self {
        RegError::Regex(e)
    }
}

impl From<std::io::Error> for RegError {
    fn from(e: std::io::Error) -> Self {
        RegError::Io(e)
    }
}

/// `Reg.reg(String listOfAlphabets, String baseexp, String regName)` (`Reg.java:18-40`).
///
/// `list_of_alphabets` is a plain `&str`, not `Option<&str>`: unlike
/// `Alphabet.alphabetCommand` (`crate::alphabet::alphabet_command`), `Reg.reg` performs
/// no null/empty check before calling `determineAlphabetsAndNS` — ported faithfully,
/// i.e. this function does not add one either.
pub fn reg(
    session: &Session,
    list_of_alphabets: &str,
    baseexp: &str,
    reg_name: &str,
) -> Result<TestCase, RegError> {
    // `Alphabet.determineAlphabetsAndNS(listOfAlphabets, NS, alphabets);` (`Reg.java:20`).
    let (ns, alphabets) = determine_alphabets_and_ns(list_of_alphabets, session.predicate_env())?;

    // `String regex = determineEncodedRegex(baseexp, M.richAlphabet);` (`Reg.java:29`) --
    // `M.richAlphabet.getA()` is exactly `alphabets` here (see this module's docs on why
    // no `M` intermediate is built).
    let encoded_regex = determine_encoded_regex(baseexp, &alphabets)?;

    // `AutomatonDFA R = new AutomatonDFA(regex, M.getAlphabetSize());
    //  R.richAlphabet.setA(M.richAlphabet.getA()); R.determineAlphabetSize();`
    // (`Reg.java:31-33`).
    let msd: Vec<Option<bool>> = ns.iter().map(|o| o.as_ref().map(|n| n.is_msd())).collect();
    let dfa = AutomatonDFA::from_encoded_regex(&encoded_regex, alphabets, msd)?;
    let mut automaton = dfa.into_automaton();

    // `R.setNS(NS);` (`Reg.java:34`) -- the all-representations and NAME parts; `msd` (the
    // third) is already installed via `from_encoded_regex`'s `msd` parameter above. The
    // name matters downstream: `union`/`intersect`/`concat` compare number systems BY NAME
    // (`NumberSystem.isNSDiffering`), so a `reg`-built custom-base automaton that lost its
    // name would compare equal to a plain base with the same alphabet cardinality.
    automaton.set_all_reps(all_reps_from_ns(&ns));
    automaton.set_ns_names(
        ns.iter()
            .map(|o| o.as_ref().map(|n| n.name().to_string()))
            .collect(),
    );

    // `R.writeAutomata(regex, Session.getWriteAddressForAutomataLibrary(), regName,
    // false);` (`Reg.java:35`). Java's `regex` argument here is the ENCODED regex string
    // (private-use characters and all), not the user's original `baseexp` -- ported
    // verbatim; `String::from_utf16_lossy` is the one place this crate cannot losslessly
    // carry Java's arbitrary UTF-16 (possible unpaired surrogates from WB-024's negative
    // encodings) into a Rust `&str`, but this text is purely a cosmetic `.gv` label, not
    // compared by any equivalence oracle.
    let predicate = String::from_utf16_lossy(&encoded_regex);
    write_automata(
        session,
        &mut automaton,
        &predicate,
        &session.paths().write_address_for_automata_library(),
        reg_name,
        false,
    )?;

    Ok(TestCase::from_automaton(automaton))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-reg-{tag}-{}-{}",
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
        // `SessionPaths` does not append a trailing slash itself (Java's
        // `Prover.parseArgs` does that); supply one here, matching `session.rs`'s own
        // test convention (`walnut_tree`'s doc comment).
        let dir_str = format!("{}/", dir.to_str().unwrap());
        let session = Session::new(Some(&dir_str), Some(&dir_str), false);
        (session, dir)
    }

    #[test]
    fn reg_over_a_literal_set_alphabet_builds_and_writes_the_automaton() {
        let (session, dir) = temp_session("literal-set");
        let tc = reg(&session, "{0,1} ", "0*1", "r").unwrap();

        assert_eq!(tc.automaton_pairs().len(), 1);
        let a = tc.automaton_pairs()[0].automaton().unwrap();
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.msd, vec![None]);

        // The library copy must exist (`writeAutomata`'s `Files.copy`).
        assert!(dir.join("Automata Library").join("r.txt").is_file());
        assert!(dir.join("Result").join("r.gv").is_file());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reg_over_a_named_number_system_installs_its_msd_direction() {
        let (session, dir) = temp_session("named-ns");
        let tc = reg(&session, "lsd_3 ", "0*1", "r").unwrap();
        let a = tc.automaton_pairs()[0].automaton().unwrap();
        assert_eq!(a.alphabet, vec![vec![0, 1, 2]]);
        assert_eq!(a.msd, vec![Some(false)]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reg_multi_track_matches_the_vector_regex_dialect() {
        let (session, dir) = temp_session("multi-track");
        let tc = reg(&session, "{0,1} {0,1} ", "[0,0][1,1]*", "r").unwrap();
        let a = tc.automaton_pairs()[0].automaton().unwrap();
        assert_eq!(a.alphabet.len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reg_propagates_a_regex_parse_error() {
        let (session, dir) = temp_session("parse-error");
        let err = reg(&session, "{0,1} ", "0|", "r").unwrap_err();
        assert!(matches!(err, RegError::Regex(_)));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reg_propagates_an_unresolvable_number_system() {
        let (session, dir) = temp_session("bad-ns");
        let err = reg(&session, "msd_neg_2 ", "0*", "r").unwrap_err();
        assert!(matches!(err, RegError::Alphabet(_)));
        fs::remove_dir_all(&dir).ok();
    }
}
