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
use wr_core::infinite::infinite;
use wr_core::logging::Logging;
use wr_core::logicalops::{remove_leading_zeros_with_ctx, RemoveLeadingZerosError};
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
        }
    }
}

use crate::error_support::simple_error_froms;
simple_error_froms!(
    ProverHelperError,
    BaWriteError => BaWrite,
    io::Error => Io,
    PredicateEnvError => Read,
    RemoveLeadingZerosError => RemoveLeadingZeros,
);

/// Java's `boolean isDFAO`, wherever it selects which of Walnut's two file libraries an
/// automaton belongs to: the plain "Automata Library" (a predicate automaton, no output)
/// or the "Word Automata Library" (a DFAO — deterministic finite automaton with output,
/// aka a "word automaton"). Threaded through most `isDFAO`-shaped call sites in this
/// crate (`determine_in_library`/`determine_out_library`/`write_automata`'s
/// `is_dfao_for_gv`, `set_alphabet`, and most of the commands that call them) — one
/// shared type rather than a per-file bool, since it is the same concept everywhere.
///
/// **Exceptions, deliberate:** `export_automata_to`, `export_automata`, and
/// `alphabet_command` keep a plain `bool` `is_dfao` parameter instead.
/// `export_automata_to` is the one with the real external caller —
/// `tests/differential` calls it directly with a literal `false` — and it does NOT
/// convert to `AutomatonKind` at an internal boundary: its `is_dfao` threads straight
/// into `wr_io`'s `export_automaton_to_ba`/`write_automaton_gv`, which already take a
/// plain `bool`, so there is nothing to convert. `export_automata` merely forwards to
/// `export_automata_to` (`As export_automata_to`, `wr-cli`'s own established
/// `_to`-suffix seam convention) and keeps `bool` purely so the two signatures match —
/// it has no external caller of its own. `alphabet_command` DOES convert at its own
/// entry (see its doc comment) before calling the `AutomatonKind`-typed primitives
/// below; its own `is_dfao` stays `bool` only because `tests/differential` calls it
/// directly too, with a literal `false`, three times.
///
/// **The carve-out criterion, ratified by the coordinator:** a `bool` stays `bool`
/// exactly when it has an in-repo external caller — a real call site in
/// `tests/golden`, `tests/differential`, or `benches/` (outside `crates/wr-cli/`, off
/// limits for this unit, idiomatic-refactor U3) — not merely because the parameter's
/// owning function is `pub`. Plain `pub`-ness does not freeze a signature here: this
/// refactor branch is declared source-breaking for embedders per its own plan, so a
/// `pub` function with no real in-repo caller outside this crate is free to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomatonKind {
    /// A word automaton (DFAO) — read/written via the Word Automata Library.
    WordAutomaton,
    /// A plain automaton (DFA/NFA, no output) — read/written via the Automata Library.
    PlainAutomaton,
}

/// `ProverHelper.exportAutomata(String s, String filename, String exportType, Automaton M,
/// boolean isDFAO)` (`:17-33`), writing its one console line to the real process stdout.
///
/// `s` is `Option<&str>` for Java's nullable predicate argument (`String predicate = s ==
/// null ? "" : s;`, `:19`) — the `[export …]` metacommand call site really does pass
/// `Prover.currentEvalName`, which is `null` in headless mode.
///
/// `is_dfao` stays `bool` rather than [`AutomatonKind`] (idiomatic-refactor U3): this
/// function only forwards it to [`export_automata_to`], whose own `bool` is fixed by a
/// real external caller — see [`AutomatonKind`]'s doc comment for the full criterion.
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
///
/// `is_dfao` stays `bool` rather than [`AutomatonKind`] (idiomatic-refactor U3):
/// `tests/differential` (outside `crates/wr-cli/`, off limits for this unit) calls this
/// function directly with a literal `false`. Unlike `alphabet_command`, there is no
/// internal boundary conversion here either — `is_dfao` threads straight into
/// `wr_io`'s `export_automaton_to_ba`/`write_automaton_gv` below, which already take a
/// plain `bool`, so `bool` is this function's natural type end to end, not a carve-out
/// forced against an otherwise-`AutomatonKind` body.
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
            // in the safe direction. `docs/WALNUT-BUGS.md` WB-040 is the confirmation that
            // it is not merely defensive: in real Walnut this call site is reached from
            // `DeterminizationStrategies`' `[export n gv]` block, mid-`determinize`, so the
            // `canonize()` mutates the automaton about to be determinized — verified live
            // to drop a state and kill an ordinary `reverse` command. Do NOT "fix" this
            // divergence by matching Java.
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
pub fn inf_from_address(
    session: &Session,
    logging: &mut Logging,
    address: &str,
) -> Result<bool, ProverHelperError> {
    inf_from_address_to(session, logging, address, &mut io::stdout())
}

/// As [`inf_from_address`], with an injectable sink.
///
/// `logging` is the caller's real, already-`configure_for_command`-d [`Logging`]
/// (`Prover`'s `self.logging`), not a throwaway one:
/// `AutomatonLogicalOps.removeLeadingZeros` logs its own `removing leading zeros for:`/
/// `removed:` pair through Java's global static `Logging`, and `Prover.parseSetup`'s
/// `Logging.configureForCommand` is universal rather than `eval`/`def`-specific, so
/// `inf A;::` really does print those two lines in real Walnut -- confirmed live against
/// `target/Walnut-all.jar`.
pub fn inf_from_address_to(
    session: &Session,
    logging: &mut Logging,
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
    let m = remove_leading_zeros_with_ctx(&m, &labels, None, logging)?;
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
    let inf_reg = infinite(m);
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
pub fn determine_in_library(
    paths: &SessionPaths,
    is_dfao: AutomatonKind,
    in_file_name: &str,
) -> String {
    match is_dfao {
        AutomatonKind::WordAutomaton => paths.read_file_for_words_library(in_file_name),
        AutomatonKind::PlainAutomaton => paths.read_file_for_automata_library(in_file_name),
    }
}

/// `ProverHelper.determineOutLibrary(boolean)` (`:69-72`).
pub fn determine_out_library(paths: &SessionPaths, is_dfao: AutomatonKind) -> String {
    match is_dfao {
        AutomatonKind::WordAutomaton => paths.write_address_for_words_library(),
        AutomatonKind::PlainAutomaton => paths.write_address_for_automata_library(),
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
        assert!(
            determine_in_library(p, AutomatonKind::WordAutomaton, "T.txt")
                .contains("Word Automata Library")
        );
        // `"Word Automata Library/T.txt"` itself CONTAINS `"Automata Library/T.txt"` as a
        // substring, so a plain `.contains` here would pass even if `PlainAutomaton`
        // silently resolved to the WORD library -- the trailing `!....contains("Word ...")`
        // is load-bearing, not decorative.
        let plain_in = determine_in_library(p, AutomatonKind::PlainAutomaton, "T.txt");
        assert!(
            plain_in.contains("Automata Library/T.txt")
                && !plain_in.contains("Word Automata Library")
        );
        assert!(determine_out_library(p, AutomatonKind::WordAutomaton)
            .contains("Word Automata Library"));
        let plain_out = determine_out_library(p, AutomatonKind::PlainAutomaton);
        assert!(
            plain_out.contains("Automata Library") && !plain_out.contains("Word Automata Library")
        );
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
        assert!(inf_from_address_to(&session, &mut Logging::new(), "lt", &mut out).unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inf_from_address_reports_a_missing_file() {
        let (session, dir) = temp_session("infmissing");
        let err = inf_from_address_to(&session, &mut Logging::new(), "nope", &mut io::sink())
            .unwrap_err();
        assert!(err.to_string().starts_with("File does not exist: "));
        fs::remove_dir_all(&dir).ok();
    }
}
