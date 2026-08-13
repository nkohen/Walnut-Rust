// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/ProverHelper.java` (74 LOC) — the five free helpers `Prover.java`'s command arms
//! share: export-to-format dispatch, the `matchOrFail` regex guard, the screen clear, the
//! `inf` command's two halves, and the DFAO/predicate library selectors.
//!
//! Java's class is `static`-only; this is a module of free functions, with the `Session`
//! statics it reads threaded in as a [`SessionPaths`] parameter (`PORTING.md`'s standing
//! ruling, already applied by `crate::session`).
//!
//! # `matchOrFail` lives in `crate::prover`
//!
//! `ProverHelper.matchOrFail` (`:35-41`) is the one helper NOT re-exported from here: it
//! is inseparable from the regex plumbing (`Pattern`/`Matcher` → `regex_automata`
//! `Regex`/`Captures`) that `crate::prover` owns, so it is
//! [`crate::prover::match_or_fail`]. Everything else in `ProverHelper.java` is below.
//!
//! # `determineInLibrary`/`determineOutLibrary` moved here from `crate::alphabet`
//!
//! U16 needed these two before `ProverHelper` had a home, so it ported them privately
//! inside `crate::alphabet`. This unit gives them their real one and `crate::alphabet` now
//! calls them here — one copy, not two drifting ones (the same call this crate already
//! made for `Session::read_library_automaton`).

use std::io::{self, Write};

use wr_core::automaton::Automaton;
use wr_core::infinite::{infinite, InfiniteError};
use wr_core::logicalops::{remove_leading_zeros, RemoveLeadingZerosError};
use wr_io::writer::{export_automaton_to_ba, write_automaton_gv, BaWriteError};

use wr_logic::predicate_env::PredicateEnvError;

use crate::prover::{BA_EXTENSION, BA_STRING, GV_EXTENSION, GV_STRING, TXT_STRING};
use crate::session::{Session, SessionPaths};
use crate::walnut_exception as msg;

/// Everything `ProverHelper`'s helpers can fail with.
#[derive(Debug)]
pub enum ProverHelperError {
    /// `ProverHelper.exportAutomata`'s `txt` arm (`:29-30`).
    ExportingToTxtIsRedundant,
    /// `WalnutException.unexpectedFormat` (`:31`).
    UnexpectedFormat(String),
    /// Propagated from `AutomatonWriter.exportToBA` (`wr_io::writer`).
    BaWrite(BaWriteError),
    /// A real I/O failure while writing. See `crate::automaton_output`'s module docs on
    /// why this crate propagates write failures rather than swallowing them the way Java
    /// does.
    Io(io::Error),
    /// `new Automaton(address)` failed inside `infFromAddress` (`:49`).
    Read(PredicateEnvError),
    /// Propagated from `AutomatonLogicalOps.removeLeadingZeros` (`:52`).
    RemoveLeadingZeros(RemoveLeadingZerosError),
    /// Propagated from `Infinite.infinite` (`:57`) — WB-002's crash trigger, reported as
    /// an error here rather than a panic.
    Infinite(InfiniteError),
}

impl std::fmt::Display for ProverHelperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProverHelperError::ExportingToTxtIsRedundant => {
                write!(f, "{}", msg::exporting_to_txt_is_redundant())
            }
            ProverHelperError::UnexpectedFormat(x) => write!(f, "{}", msg::unexpected_format(x)),
            ProverHelperError::BaWrite(e) => write!(f, "{e}"),
            ProverHelperError::Io(e) => write!(f, "{e}"),
            ProverHelperError::Read(e) => write!(f, "{e}"),
            ProverHelperError::RemoveLeadingZeros(e) => write!(f, "{e}"),
            ProverHelperError::Infinite(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProverHelperError {}

impl From<BaWriteError> for ProverHelperError {
    fn from(e: BaWriteError) -> Self {
        ProverHelperError::BaWrite(e)
    }
}

impl From<io::Error> for ProverHelperError {
    fn from(e: io::Error) -> Self {
        ProverHelperError::Io(e)
    }
}

impl From<PredicateEnvError> for ProverHelperError {
    fn from(e: PredicateEnvError) -> Self {
        ProverHelperError::Read(e)
    }
}

impl From<RemoveLeadingZerosError> for ProverHelperError {
    fn from(e: RemoveLeadingZerosError) -> Self {
        ProverHelperError::RemoveLeadingZeros(e)
    }
}

impl From<InfiniteError> for ProverHelperError {
    fn from(e: InfiniteError) -> Self {
        ProverHelperError::Infinite(e)
    }
}

/// `ProverHelper.exportAutomata(String s, String filename, String exportType, Automaton M,
/// boolean isDFAO)` (`:17-33`), writing its one console line to the real process stdout.
///
/// `s` is `Option<&str>` for Java's nullable predicate argument (`String predicate = s ==
/// null ? "" : s;`, `:19`) — the `[export …]` metacommand call site really does pass
/// `Prover.currentEvalName`, which is `null` in headless mode.
pub fn export_automata(
    paths: &SessionPaths,
    s: Option<&str>,
    filename: &str,
    export_type: &str,
    m: &Automaton,
    is_dfao: bool,
) -> Result<(), ProverHelperError> {
    export_automata_to(
        paths,
        s,
        filename,
        export_type,
        m,
        is_dfao,
        &mut io::stdout(),
    )
}

/// As [`export_automata`], but with an injectable sink for the `Writing to …` line — the
/// same seam `crate::eval_def`'s `_with_stdout` variant uses, for the same reason.
pub fn export_automata_to(
    paths: &SessionPaths,
    s: Option<&str>,
    filename: &str,
    export_type: &str,
    m: &Automaton,
    is_dfao: bool,
    stdout: &mut dyn Write,
) -> Result<(), ProverHelperError> {
    let export_type_lower = export_type.to_lowercase();
    let predicate = s.unwrap_or("");
    let result_file = format!("{}{filename}", paths.address_for_result());

    // "currently only a few types are supported" (`:22`).
    match export_type_lower.as_str() {
        BA_STRING => {
            export_automaton_to_ba(&m.fa, format!("{result_file}{BA_EXTENSION}"), is_dfao)?;
        }
        GV_STRING => {
            writeln!(stdout, "Writing to {result_file}{GV_EXTENSION}")?;
            // `wr_io::writer::write_automaton_gv` takes `&mut Automaton` (Java's
            // `writeToGV` likewise mutates `M` in passing, via `canonize`); this helper
            // is handed a shared reference by `DeterminizeContext::export_pre_determinization`,
            // so it writes a clone. Cloning is deep (`PORTING.md`), so the caller's
            // automaton is genuinely untouched — a small, deliberate divergence from Java,
            // in the safe direction.
            let mut copy = m.clone();
            write_automaton_gv(
                &mut copy,
                format!("{result_file}{GV_EXTENSION}"),
                predicate,
                is_dfao,
            )?;
        }
        TXT_STRING => return Err(ProverHelperError::ExportingToTxtIsRedundant),
        _ => return Err(ProverHelperError::UnexpectedFormat(export_type.to_string())),
    }
    Ok(())
}

/// `ProverHelper.clearScreen()` (`:43-46`) — the ANSI "cursor home + erase screen" pair,
/// written to the real process stdout and flushed.
pub fn clear_screen() {
    clear_screen_to(&mut io::stdout());
}

/// As [`clear_screen`], with an injectable sink.
pub fn clear_screen_to(stdout: &mut dyn Write) {
    // Java: `System.out.print("\033[H\033[2J"); System.out.flush();` -- both failures are
    // silently ignored there (`print` swallows I/O errors), so they are here too.
    let _ = write!(stdout, "\u{1b}[H\u{1b}[2J");
    let _ = stdout.flush();
}

/// `ProverHelper.infFromAddress(String)` (`:48-54`).
///
/// `address` is the bare automaton NAME, despite the parameter's name: Java passes it to
/// `Automaton.readAutomatonFromFile`, which appends `.txt` and resolves it against the
/// Automata Library (`Automaton.java:148-150`), and then re-uses the same string as the
/// automaton's display name in the printed line.
pub fn inf_from_address(session: &Session, address: &str) -> Result<bool, ProverHelperError> {
    inf_from_address_to(session, address, &mut io::stdout())
}

/// As [`inf_from_address`], with an injectable sink.
pub fn inf_from_address_to(
    session: &Session,
    address: &str,
    stdout: &mut dyn Write,
) -> Result<bool, ProverHelperError> {
    // `Automaton M = Automaton.readAutomatonFromFile(address);` (`:49`).
    let resolved = session
        .paths()
        .read_file_for_automata_library(&format!("{address}{}", crate::prover::TXT_EXTENSION));
    let mut m = session.libraries().read_library_automaton(&resolved)?;

    // "we don't want to count multiple representations of the same value as distinct
    // accepted values" (`:50-52`).
    m.random_label();
    let labels = m.label.clone();
    let m = remove_leading_zeros(&m, &labels)?;
    inf_from_automaton_to(address, &m, stdout)
}

/// `ProverHelper.infFromAutomaton(String automatonName, Automaton M)` (`:56-62`) —
/// returns whether the language is infinite, after printing Java's one-line verdict.
pub fn inf_from_automaton(automaton_name: &str, m: &Automaton) -> Result<bool, ProverHelperError> {
    inf_from_automaton_to(automaton_name, m, &mut io::stdout())
}

/// As [`inf_from_automaton`], with an injectable sink.
pub fn inf_from_automaton_to(
    automaton_name: &str,
    m: &Automaton,
    stdout: &mut dyn Write,
) -> Result<bool, ProverHelperError> {
    // `String infReg = Infinite.infinite(M.fa, M.richAlphabet);` (`:57`). This port
    // returns `Option<String>` where Java returns `""`-for-finite, so `is_some()` is
    // Java's `!infReg.isEmpty()` -- see `wr_core::infinite`'s docs.
    let inf_reg = infinite(m)?;
    match &inf_reg {
        Some(reg) => writeln!(
            stdout,
            "Automaton accepts infinite values, including regex:{reg}"
        )?,
        None => writeln!(
            stdout,
            "Automaton {automaton_name} accepts finitely many values."
        )?,
    }
    Ok(inf_reg.is_some())
}

/// `ProverHelper.determineInLibrary(boolean, String)` (`:64-67`).
pub fn determine_in_library(paths: &SessionPaths, is_dfao: bool, in_file_name: &str) -> String {
    if is_dfao {
        paths.read_file_for_words_library(in_file_name)
    } else {
        paths.read_file_for_automata_library(in_file_name)
    }
}

/// `ProverHelper.determineOutLibrary(boolean)` (`:69-72`).
pub fn determine_out_library(paths: &SessionPaths, is_dfao: bool) -> String {
    if is_dfao {
        paths.write_address_for_words_library()
    } else {
        paths.write_address_for_automata_library()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wr_core::numsys::less_than_msd;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-prover-helper-{tag}-{}-{}",
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
        (Session::new(Some(&dir_str), Some(&dir_str), false), dir)
    }

    #[test]
    fn export_to_gv_writes_the_file_and_announces_it() {
        let (session, dir) = temp_session("gv");
        let a = less_than_msd(2);
        let mut out: Vec<u8> = Vec::new();
        export_automata_to(session.paths(), Some("x<y"), "e", "GV", &a, false, &mut out).unwrap();

        let gv = dir.join("Result").join("e.gv");
        assert!(gv.is_file());
        assert!(fs::read_to_string(&gv).unwrap().contains("x<y"));
        assert!(String::from_utf8(out).unwrap().starts_with("Writing to "));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_to_ba_writes_the_file() {
        let (session, dir) = temp_session("ba");
        let a = less_than_msd(2);
        let mut out: Vec<u8> = Vec::new();
        export_automata_to(session.paths(), None, "e", "ba", &a, false, &mut out).unwrap();
        assert!(dir.join("Result").join("e.ba").is_file());
        // The `ba` arm prints nothing.
        assert!(out.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exporting_to_txt_is_rejected_as_redundant() {
        let (session, dir) = temp_session("txt");
        let a = less_than_msd(2);
        let err = export_automata_to(
            session.paths(),
            None,
            "e",
            "TXT",
            &a,
            false,
            &mut io::sink(),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Exporting to .txt is redundant; this is the input format"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unknown_export_format_reports_the_original_casing() {
        let (session, dir) = temp_session("badformat");
        let a = less_than_msd(2);
        let err = export_automata_to(
            session.paths(),
            None,
            "e",
            "PDF",
            &a,
            false,
            &mut io::sink(),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "Unexpected format:PDF");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_does_not_mutate_the_caller_s_automaton() {
        let (session, dir) = temp_session("nomutate");
        let a = less_than_msd(2);
        let before = format!("{:?}", a.fa);
        export_automata_to(session.paths(), None, "e", "gv", &a, false, &mut io::sink()).unwrap();
        assert_eq!(before, format!("{:?}", a.fa));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_screen_emits_the_two_ansi_escapes() {
        let mut out: Vec<u8> = Vec::new();
        clear_screen_to(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "\u{1b}[H\u{1b}[2J");
    }

    #[test]
    fn the_library_selectors_pick_the_right_directory() {
        let (session, dir) = temp_session("libs");
        let p = session.paths();
        assert!(determine_in_library(p, true, "T.txt").contains("Word Automata Library"));
        assert!(determine_in_library(p, false, "T.txt").contains("Automata Library/T.txt"));
        assert!(determine_out_library(p, true).contains("Word Automata Library"));
        assert!(determine_out_library(p, false).contains("Automata Library"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inf_from_automaton_reports_an_infinite_language() {
        // `x < y` over msd_2 has infinitely many accepted pairs.
        let a = less_than_msd(2);
        let mut out: Vec<u8> = Vec::new();
        assert!(inf_from_automaton_to("lt", &a, &mut out).unwrap());
        assert!(String::from_utf8(out)
            .unwrap()
            .starts_with("Automaton accepts infinite values, including regex:"));
    }

    #[test]
    fn inf_from_automaton_reports_a_finite_language() {
        // The TRUE automaton short-circuits to "finite" in `wr_core::infinite`.
        let a = Automaton::true_false(true);
        let mut out: Vec<u8> = Vec::new();
        assert!(!inf_from_automaton_to("t", &a, &mut out).unwrap());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Automaton t accepts finitely many values.\n"
        );
    }

    #[test]
    fn inf_from_address_reads_the_automata_library() {
        let (session, dir) = temp_session("inf");
        let mut a = less_than_msd(2);
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("lt.txt"))
            .unwrap();

        let mut out: Vec<u8> = Vec::new();
        assert!(inf_from_address_to(&session, "lt", &mut out).unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inf_from_address_reports_a_missing_file() {
        let (session, dir) = temp_session("infmissing");
        let err = inf_from_address_to(&session, "nope", &mut io::sink()).unwrap_err();
        assert!(err.to_string().starts_with("File does not exist: "));
        fs::remove_dir_all(&dir).ok();
    }
}
