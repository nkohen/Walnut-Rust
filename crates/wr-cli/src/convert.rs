// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Prover.java`'s inline `convertCommand` (`:724-745`) — the `convert` command, a
//! thin CLI wrapper around U18's already-ported [`wr_core::logicalops::convert_ns`].
//! No separate `Main/Commands/Convert.java` class exists (`convert` is one of
//! `PORTING.md`'s ~20 genuinely inline `Prover.java` commands), so this module is its
//! home, per that ruling ("port them as modules too").
//!
//! # The DFAO->function guard, and the hardcoded `true` GV-style quirk
//!
//! Two behaviors ported verbatim, both easy to get backwards:
//! - `if (oldIsDFAO && !newIsDFAO) throw WalnutException.convertDFAOIntoFunction();`
//!   (`:731-733`) — converting a word automaton (DFAO) into a plain predicate
//!   automaton is rejected; every other combination (predicate->predicate,
//!   predicate->DFAO, DFAO->DFAO) is fine.
//! - `M.writeAutomata(s, ProverHelper.determineOutLibrary(newIsDFAO), …, true)` (`:743`)
//!   — the FOURTH argument (which library-copy directory to use) correctly depends on
//!   `newIsDFAO`, but the `.gv` rendering style (`write_automata`'s `is_dfao_for_gv`
//!   parameter) is a **hardcoded `true`**, regardless of `newIsDFAO`. So converting
//!   INTO a plain predicate automaton still renders its `.gv` in the `"q/output"` DFAO
//!   node-label style, not the plain accept/reject-circle style. The same class of
//!   quirk `crate::alphabet`'s module docs already document for `alphabet`/`reg`
//!   (there hardcoded `false`; here hardcoded `true`) — ported verbatim, not "fixed"
//!   to track `newIsDFAO`.
//!
//! # Base parsing: real Java `Integer.parseInt`, not `UtilityMethods.parseInt`
//!
//! `Integer.parseInt(m.group(GROUP_CONVERT_BASE))` (`:740`) is the STRICT parser (no
//! whitespace stripping) — but the capturing group backing it
//! (`RE_FOR_convert_CMD`'s `((?-u:\d)+)`) can only ever contain plain ASCII digits, so
//! the two parsers agree on every string this pattern can produce; the only realistic
//! failure mode is `i32` overflow on an implausibly long digit run, reported as
//! [`ConvertError::InvalidBase`] (a `Result::Err`, matching how this crate treats
//! every other genuine-but-rarely-reachable Java exception, rather than the panic
//! `wr_core::util::parse_int` would give for the *whitespace-tolerant* Java parser).

use wr_core::logicalops::{convert_ns, ConvertNsError};
use wr_core::numsys::{self, TXT_EXTENSION};
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_output::write_automata;
use crate::prover_helper::{determine_in_library, determine_out_library};
use crate::session::Session;
use crate::test_case::TestCase;
use crate::walnut_exception as msg;

/// Every failure [`convert_command`] can report.
#[derive(Debug)]
pub enum ConvertError {
    /// `WalnutException.convertDFAOIntoFunction()` (`:732`).
    DfaoIntoFunction,
    /// `Integer.parseInt(m.group(GROUP_CONVERT_BASE))` (`:740`) — see module docs.
    InvalidBase(String),
    /// `new Automaton(inLibrary)` (`:737`) failing to read the input automaton.
    Read(PredicateEnvError),
    /// `AutomatonLogicalOps.convertNS` (`:739-741`) itself — see
    /// [`wr_core::logicalops::ConvertNsError`] for its five failure modes (including
    /// WB-032's silent-wrong-answer float-log case, which is NOT an `Err` here either,
    /// matching Java: it doesn't throw, it just computes the wrong exponent).
    Convert(ConvertNsError),
    /// A real I/O failure writing the result.
    Io(std::io::Error),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::DfaoIntoFunction => write!(f, "{}", msg::convert_dfao_into_function()),
            ConvertError::InvalidBase(text) => write!(f, "For input string: \"{text}\""),
            ConvertError::Read(e) => write!(f, "{e}"),
            ConvertError::Convert(e) => write!(f, "{e}"),
            ConvertError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<std::io::Error> for ConvertError {
    fn from(e: std::io::Error) -> Self {
        ConvertError::Io(e)
    }
}

/// `Prover.convertCommand(String s)` (`Prover.java:724-745`). Every argument below is
/// a raw captured group's text, mirroring Java's own `m.group(...)` call shape at each
/// use site (the `$`-comparisons and `Integer.parseInt` happen here, not in
/// `crate::prover`, matching where Java's own statements live).
#[allow(clippy::too_many_arguments)]
pub fn convert_command(
    session: &Session,
    s: &str,
    new_dollar_sign: &str,
    new_name: &str,
    msd_or_lsd: &str,
    base: &str,
    old_dollar_sign: &str,
    old_name: &str,
) -> Result<TestCase, ConvertError> {
    // `boolean newIsDFAO = !newDollarSign.equals("$");` / `boolean oldIsDFAO =
    //  !oldDollarSign.equals("$");` (`:727-730`).
    let new_is_dfao = new_dollar_sign != "$";
    let old_is_dfao = old_dollar_sign != "$";
    if old_is_dfao && !new_is_dfao {
        return Err(ConvertError::DfaoIntoFunction);
    }

    // `String inFileName = m.group(GROUP_CONVERT_OLD_NAME) + TXT_EXTENSION; String
    //  inLibrary = ProverHelper.determineInLibrary(oldIsDFAO, inFileName); Automaton M
    //  = new Automaton(inLibrary);` (`:735-737`).
    let in_file_name = format!("{old_name}{TXT_EXTENSION}");
    let in_library = determine_in_library(session.paths(), old_is_dfao, &in_file_name);
    let mut m = session
        .libraries()
        .read_library_automaton(&in_library)
        .map_err(ConvertError::Read)?;

    // `Integer.parseInt(m.group(GROUP_CONVERT_BASE))` (`:740`).
    let base_value: i32 = base
        .parse()
        .map_err(|_| ConvertError::InvalidBase(base.to_string()))?;

    // `AutomatonLogicalOps.convertNS(M, m.group(GROUP_CONVERT_MSD_OR_LSD).equals(
    //  NumberSystem.MSD), Integer.parseInt(m.group(GROUP_CONVERT_BASE)));` (`:739-741`).
    convert_ns(&mut m, msd_or_lsd == numsys::MSD, base_value).map_err(ConvertError::Convert)?;

    // `M.writeAutomata(s, ProverHelper.determineOutLibrary(newIsDFAO), m.group(
    //  GROUP_CONVERT_NEW_NAME), true);` (`:743`) -- the trailing `true` is a hardcoded
    // GV-style quirk, see module docs.
    let out_library = determine_out_library(session.paths(), new_is_dfao);
    write_automata(session, &mut m, s, &out_library, new_name, true)?;

    Ok(TestCase::from_automaton(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wr_core::automaton::Automaton;
    use wr_core::numsys::less_than_msd;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-convert-{tag}-{}-{}",
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

    #[test]
    fn convert_rejects_dfao_into_function() {
        let (session, dir) = temp_session("dfao-guard");
        // No automaton needs to actually exist -- the guard fires before any read.
        let err = convert_command(&session, "", "$", "out", "msd", "4", "", "in").unwrap_err();
        assert!(matches!(err, ConvertError::DfaoIntoFunction));
        assert_eq!(
            err.to_string(),
            "Cannot convert a Word Automaton into a function"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// A single-track msd_2 automaton accepting exactly the numbers whose LAST digit
    /// read is `0` (i.e. even numbers, in the mundane "state after the last symbol
    /// determines parity" sense) -- `convertNS` requires a single-track input
    /// (`ConvertNsError::NotSingleInput` otherwise), unlike `less_than_msd`'s two
    /// tracks.
    fn even_numbers_msd(base: i32) -> Automaton {
        use std::collections::BTreeMap;
        use wr_core::fa::Fa;
        let alphabet: Vec<i32> = (0..base).collect();
        let mut d0 = BTreeMap::new();
        let mut d1 = BTreeMap::new();
        for &digit in &alphabet {
            let dest = if digit == 0 { 0 } else { 1 };
            d0.insert(digit, vec![dest]);
            d1.insert(digit, vec![dest]);
        }
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: alphabet.len(),
                o: vec![1, 0],
                d: vec![d0, d1],
            },
            vec![alphabet],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    #[test]
    fn convert_msd_to_msd_base4_round_trips_the_language() {
        // A single-track msd_2 automaton converted to msd_4 must still recognize the
        // same numbers, re-expressed in base 4.
        let (session, dir) = temp_session("msd4");
        let mut a = even_numbers_msd(2);
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("lt.txt"))
            .unwrap();

        let tc = convert_command(&session, "", "$", "lt4", "msd", "4", "$", "lt").unwrap();
        let converted = tc.automaton_pairs()[0].automaton().unwrap();
        assert_eq!(converted.alphabet, vec![vec![0, 1, 2, 3]]);
        assert!(dir.join("Result").join("lt4.txt").is_file());
        assert!(dir.join("Automata Library").join("lt4.txt").is_file());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn convert_rejects_a_malformed_base() {
        let (session, dir) = temp_session("badbase");
        let mut a = less_than_msd(2);
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("lt.txt"))
            .unwrap();

        let err = convert_command(
            &session,
            "",
            "$",
            "out",
            "msd",
            "999999999999999999999",
            "$",
            "lt",
        )
        .unwrap_err();
        assert!(matches!(err, ConvertError::InvalidBase(_)));

        fs::remove_dir_all(&dir).ok();
    }
}
