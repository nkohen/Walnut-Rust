// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Ost.java` — the `ost` command, which creates a new Ostrowski
//! numeration system from the continued-fraction expansion of a quadratic irrational
//! `alpha < 1`.
//!
//! Two files land in `Custom Bases/`: `msd_<name>.txt` (the representation automaton)
//! and `msd_<name>_addition.txt` (the adder). Nothing else in the port needs a new
//! consumption path for them — [`wr_core::numsys`]'s existing custom-base loader
//! (`CustomBaseCandidates`/`CustomBaseFiles`, keyed off
//! [`wr_core::numsys::MSD_UNDERSCORE`]/[`wr_core::numsys::UNDERSCORE_ADDITION_AUTOMATON`])
//! already reads exactly this pair; `msd_fib`/`msd_pell` in Walnut's own shipped
//! `Custom Bases/` are literally files this command produces.
//!
//! The construction itself lives in [`wr_core::ostrowski`]; this module is the command
//! wiring: parse, construct, write, and report.
//!
//! # Where the digit-list parsing happens
//!
//! Java's `Ostrowski` constructor calls `ParseMethods.parseList` on the two raw strings
//! itself. `ParseMethods` is `wr-io` here, and `wr-core` does not depend on `wr-io`, so
//! [`wr_core::ostrowski::Ostrowski::new`] takes already-parsed `&[i32]` and the parsing
//! moved up here — the same split `crate::morphism`/`crate::image` already use for
//! `parse_morphism`. See that module's docs, divergence 1.
//!
//! # Write ordering is observable
//!
//! `Ost.ostCommand` (`Ost.java:15-23`) is construct → build repr → **write repr** →
//! build adder → **write adder**, with `Ostrowski.writeAutomaton`'s
//! already-exists guard on each write individually. That interleaving is kept exactly:
//! if `msd_<name>_addition.txt` already exists but `msd_<name>.txt` does not, real
//! Walnut writes `msd_<name>.txt` to disk and only then throws — leaving one new file
//! behind. A "build both, then write both" version would leave nothing. Since this
//! command's whole point is its filesystem side effects, the sequencing is part of the
//! behavior, not an implementation detail.

use std::io::Write;

use wr_core::automaton::Automaton;
use wr_core::logging::Logging;
use wr_core::numsys::{MSD_UNDERSCORE, UNDERSCORE_ADDITION_AUTOMATON};
use wr_core::ostrowski::{Ostrowski, OstrowskiError};
use wr_io::parse_methods::ParseMethodsError;

use crate::prover::TXT_EXTENSION;
use crate::session::Session;
use crate::test_case::{AutomatonFilenamePair, TestCase, DEFAULT_TESTFILE, OST_REPR_TESTFILE};
use crate::walnut_exception as msg;

/// Everything the `ost` command can fail with.
#[derive(Debug)]
pub enum OstError {
    /// `ParseMethods.parseList`'s `UtilityMethods.parseInt` overflowing `int`
    /// (`ParseMethods.java:161`) — `ost o [99999999999] [1];` is regex-legal, and Java
    /// throws `NumberFormatException` there, caught by `Prover.readBuffer`.
    Parse(ParseMethodsError),
    /// `new Ostrowski(...)`'s own `WalnutException`s (`Ost.java:15`).
    Ostrowski(OstrowskiError),
    /// `Ostrowski.writeAutomaton`'s already-exists guard (`Ostrowski.java:231-233`).
    AlreadyExists { name: String },
    /// A genuine I/O failure writing one of the two `.txt` files. Java's
    /// `AutomatonWriter.writeToTxtFormat` swallows its `IOException` and only logs;
    /// `wr_io::writer` propagates instead (its own long-established idiom deviation —
    /// see that module's docs).
    Io(std::io::Error),
}

impl std::fmt::Display for OstError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OstError::Parse(e) => write!(f, "{e}"),
            OstError::Ostrowski(e) => write!(f, "{e}"),
            OstError::AlreadyExists { name } => {
                write!(f, "{}", msg::number_system_already_exists(name))
            }
            OstError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OstError {}

impl From<ParseMethodsError> for OstError {
    fn from(e: ParseMethodsError) -> Self {
        OstError::Parse(e)
    }
}

impl From<OstrowskiError> for OstError {
    fn from(e: OstrowskiError) -> Self {
        OstError::Ostrowski(e)
    }
}

impl From<std::io::Error> for OstError {
    fn from(e: std::io::Error) -> Self {
        OstError::Io(e)
    }
}

/// `Ost.ostCommand(String name, String preperiod, String period)` (`Ost.java:14-24`).
///
/// `out` receives `Ostrowski.writeAutomaton`'s `System.out.println("Writing to: …")`
/// (`Ostrowski.java:229`) — the one command in this crate that announces its own write
/// this way, matching Java (no other write call site in `Main/` prints a line like it).
pub fn ost_command(
    session: &Session,
    logging: &mut Logging,
    out: &mut dyn Write,
    name: &str,
    preperiod: &str,
    period: &str,
) -> Result<TestCase, OstError> {
    let preperiod = parse_digits(preperiod)?;
    let period = parse_digits(period)?;

    // `Ostrowski ostr = new Ostrowski(name, preperiod, period);` (`:15`)
    let mut ostr = Ostrowski::new(name, &preperiod, &period)?;

    // `Automaton repr = ostr.createRepresentationAutomaton();` (`:16`)
    let mut repr = ostr.create_representation_automaton(logging);
    // `String msdName = NumberSystem.MSD_UNDERSCORE + name;` (`:17`)
    let msd_name = format!("{MSD_UNDERSCORE}{name}");
    // `Ostrowski.writeAutomaton(name, msdName + Prover.TXT_EXTENSION, repr);` (`:18`)
    write_automaton(
        session,
        out,
        name,
        &format!("{msd_name}{TXT_EXTENSION}"),
        &mut repr,
    )?;

    // `Automaton adder = ostr.createAdderAutomaton();` (`:19`)
    let mut adder = ostr.create_adder_automaton(logging);
    // `Ostrowski.writeAutomaton(name, msdName + NumberSystem.UNDERSCORE_ADDITION_AUTOMATON,
    //  adder);` (`:20`)
    write_automaton(
        session,
        out,
        name,
        &format!("{msd_name}{UNDERSCORE_ADDITION_AUTOMATON}"),
        &mut adder,
    )?;

    // `:21-23` — the ADDER is the `DEFAULT_TESTFILE` pair and comes FIRST; the
    // representation automaton is the `OST_REPR_TESTFILE` pair and comes second. That is
    // also the order `IntegrationTest.loadTestCases` builds its expected pairs in
    // (`IntegrationTest.java:1018-1032`), so the golden harness compares positionally.
    Ok(TestCase::from_automaton_pairs(vec![
        AutomatonFilenamePair::new(adder, DEFAULT_TESTFILE),
        AutomatonFilenamePair::new(repr, OST_REPR_TESTFILE),
    ]))
}

/// `ParseMethods.parseList` plus the wildcard rejection Java reaches by crashing.
///
/// Java's `parseList` puts a boxed `null` in the list for a `*` element
/// (`ParseMethods.java:160`), and the very next thing `Ostrowski`'s constructor does —
/// `removeLeadingZeros`'s `it.next() == 0` (`Ostrowski.java:135`) — auto-unboxes it into
/// a `NullPointerException`, uncaught by `Ost.ostCommand` and recovered only by
/// `Prover.readBuffer`'s top-level `catch (RuntimeException)`.
///
/// **This is unreachable through the real `ost` grammar**: `Prover.RE_FOR_ost_CMD`'s two
/// bracketed groups accept `\d+` runs and whitespace only, never `*`
/// (`crate::prover::Patterns::ost`). So there is no golden or differential coverage to
/// match against, and rather than invent a bespoke error shape for an input the parser
/// cannot deliver, this reproduces Java's *behavior* — an uncaught runtime failure that
/// [`crate::prover::Prover`]'s panic-recovery boundary turns into a reported error and a
/// surviving session, exactly as `readBuffer` does. The message is necessarily this
/// workspace's own wording (Java's is a JVM-generated NPE text), which
/// [`wr_core::walnut_panic`]'s module docs already sanction for this case.
fn parse_digits(s: &str) -> Result<Vec<i32>, OstError> {
    let parsed = wr_io::parse_methods::parse_list(s)?;
    Ok(parsed
        .into_iter()
        .map(|d| d.expect("ost does not accept a wildcard digit"))
        .collect())
}

/// `Ostrowski.writeAutomaton(String name, String fullName, Automaton a)`
/// (`Ostrowski.java:227-235`).
///
/// The `Writing to: …` line is printed BEFORE the exists check, so a rejected write
/// still announces itself — kept in that order.
fn write_automaton(
    session: &Session,
    out: &mut dyn Write,
    name: &str,
    full_name: &str,
    a: &mut Automaton,
) -> Result<(), OstError> {
    let automaton_file_name = format!(
        "{}{full_name}",
        session.paths().write_address_for_custom_bases()
    );
    // `System.out.println(...)` (`:229`). The write result is dropped, not propagated —
    // `crate::macro_cmd`/`crate::eval_def`/`crate::prover`'s established convention for a
    // console line, because Java's `System.out.println` cannot fail and a sink-write
    // failure must not become an error the REPL treats as the command's outcome.
    let _ = writeln!(out, "Writing to: {automaton_file_name}");
    if std::path::Path::new(&automaton_file_name).exists() {
        return Err(OstError::AlreadyExists {
            name: name.to_string(),
        });
    }
    wr_io::writer::write_automaton_txt(a, &automaton_file_name)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    // ---------------------------------------------------------------------
    // Tier 2 — `Automata/Numeration/OstrowskiTest.java`, method for method.
    //
    // The eight `testAgainstFile` cases live HERE rather than beside the
    // construction code in `wr_core::ostrowski` because Java's own assertion is
    // `AutomatonWriter.writeTxtFormatToStream(actual) == <fixture bytes>` — a
    // byte-level write comparison, which needs `wr-io`'s writer as well as
    // `wr-core`'s constructor. `wr-cli` is the lowest crate that has both. The
    // constructor-only cases (`createOstrowski`, `testNode`, the three throw cases,
    // the rotation case) are in `wr_core::ostrowski`'s own test module.
    // ---------------------------------------------------------------------

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ostrowski")
    }

    /// `OstrowskiTest.testAgainstFile` (`:263-274`).
    fn test_against_file(expected_file: &str, actual: &mut Automaton) {
        let expected = fs::read_to_string(fixture_dir().join(expected_file))
            .unwrap_or_else(|e| panic!("reading {expected_file}: {e}"));
        let mut written: Vec<u8> = Vec::new();
        wr_io::writer::write_txt(actual, &mut written).expect("writing to a Vec cannot fail");
        assert_eq!(
            expected,
            String::from_utf8(written).expect("the writer emits UTF-8"),
            "{expected_file}"
        );
    }

    fn check(
        name: &str,
        preperiod: &[i32],
        period: &[i32],
        expect_pre: &[i32],
        expect_per: &[i32],
    ) {
        let mut ost = Ostrowski::new(name, preperiod, period).expect("valid continued fraction");
        assert_eq!(ost.preperiod, expect_pre, "{name} preperiod");
        assert_eq!(ost.period, expect_per, "{name} period");
        let mut logging = Logging::new();
        test_against_file(
            &format!("{MSD_UNDERSCORE}{name}{TXT_EXTENSION}"),
            &mut ost.create_representation_automaton(&mut logging),
        );
        test_against_file(
            &format!("{MSD_UNDERSCORE}{name}{UNDERSCORE_ADDITION_AUTOMATON}"),
            &mut ost.create_adder_automaton(&mut logging),
        );
    }

    /// `OstrowskiTest.createFib` (`:150-160`) — test against `msd_fib`.
    #[test]
    fn create_fib() {
        check("fib", &[0, 2], &[1], &[2], &[1]);
    }

    /// `OstrowskiTest.createNumSys` (`:165-175`) — the example from the Ostrowski
    /// documentation.
    #[test]
    fn create_num_sys() {
        check("numsys", &[0, 3, 1], &[1, 2], &[3, 1], &[1, 2]);
    }

    /// `OstrowskiTest.createPell` (`:180-190`).
    #[test]
    fn create_pell() {
        check("pell", &[0], &[2], &[2], &[2]);
    }

    /// `OstrowskiTest.createNs6` (`:195-205`).
    #[test]
    fn create_ns6() {
        check(
            "ns6",
            &[0, 1, 2, 1, 1],
            &[1, 1, 1, 2],
            &[3, 1, 1],
            &[1, 1, 1, 2],
        );
    }

    /// `OstrowskiTest.createNs7` (`:209-219`).
    #[test]
    fn create_ns7() {
        check("ns7", &[0, 1, 1, 3], &[1, 2, 1], &[2, 3], &[1, 2, 1]);
    }

    /// `OstrowskiTest.createNs8` (`:223-233`).
    #[test]
    fn create_ns8() {
        check("ns8", &[0, 1, 3, 1], &[2], &[4, 1], &[2]);
    }

    /// `OstrowskiTest.createNs9` (`:237-247`).
    #[test]
    fn create_ns9() {
        check("ns9", &[0, 1, 2, 3], &[2], &[3, 3], &[2]);
    }

    /// `OstrowskiTest.createNs10` (`:251-261`).
    #[test]
    fn create_ns10() {
        check("ns10", &[0, 1, 4, 2], &[3], &[5, 2], &[3]);
    }

    // ---------------------------------------------------------------------
    // `ost_command` — the write path, its exists-guard, and its `TestCase` shape.
    // ---------------------------------------------------------------------

    fn temp_session(tag: &str) -> (Session, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-ost-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_dir_all(&dir);
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

    fn run_ost(
        session: &Session,
        name: &str,
        preperiod: &str,
        period: &str,
    ) -> (Result<TestCase, OstError>, String) {
        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let mut out: Vec<u8> = Vec::new();
        let r = ost_command(session, &mut logging, &mut out, name, preperiod, period);
        (r, String::from_utf8(out).unwrap())
    }

    /// The happy path: both files land in `Custom Bases/`, byte-identical to what real
    /// `walnut-java` writes for the same command (the fixtures the Tier-2 tests above
    /// use), and the `TestCase` carries the adder first and the representation
    /// automaton second, per `Ost.java:21-23`.
    #[test]
    fn ost_writes_both_custom_base_files_and_returns_the_adder_first() {
        let (session, dir) = temp_session("happy");
        let (tc, printed) = run_ost(&session, "pell", "0", "2");
        let tc = tc.expect("ost pell [0] [2] succeeds");

        assert_eq!(tc.automaton_pairs().len(), 2);
        assert_eq!(tc.automaton_pairs()[0].filename(), DEFAULT_TESTFILE);
        assert_eq!(tc.automaton_pairs()[1].filename(), OST_REPR_TESTFILE);

        let bases = dir.join("Custom Bases");
        for (written, fixture) in [
            ("msd_pell.txt", "msd_pell.txt"),
            ("msd_pell_addition.txt", "msd_pell_addition.txt"),
        ] {
            assert_eq!(
                fs::read_to_string(bases.join(written)).unwrap(),
                fs::read_to_string(fixture_dir().join(fixture)).unwrap(),
                "{written}"
            );
        }

        // `Ostrowski.writeAutomaton`'s `System.out.println("Writing to: …")` (`:229`),
        // once per file, in write order.
        let lines: Vec<&str> = printed.lines().collect();
        assert_eq!(lines.len(), 2, "{printed:?}");
        assert!(lines[0].starts_with("Writing to: ") && lines[0].ends_with("msd_pell.txt"));
        assert!(
            lines[1].starts_with("Writing to: ") && lines[1].ends_with("msd_pell_addition.txt")
        );

        // The returned pairs really are the two DISTINCT automata, not the same one
        // twice: the adder is 3-track, the representation automaton 1-track.
        assert_eq!(
            tc.automaton_pairs()[0].automaton().unwrap().alphabet.len(),
            3
        );
        assert_eq!(
            tc.automaton_pairs()[1].automaton().unwrap().alphabet.len(),
            1
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// `OstrowskiTest.writeAutomatonThrowsWhenFileAlreadyExists` (`:78-94`) —
    /// `Ostrowski.writeAutomaton`'s `f.exists()` guard (`Ostrowski.java:231-233`).
    #[test]
    fn a_second_ost_with_the_same_name_hits_the_already_exists_guard() {
        let (session, dir) = temp_session("exists");
        run_ost(&session, "pell", "0", "2")
            .0
            .expect("first run succeeds");

        let (err, printed) = run_ost(&session, "pell", "0", "2");
        let err = err.expect_err("second run must refuse to overwrite");
        assert!(matches!(&err, OstError::AlreadyExists { name } if name == "pell"));
        assert_eq!(err.to_string(), "Error: number system pell already exists.");
        // The announcement is printed BEFORE the exists check (`:229` precedes `:231`),
        // so the refused write still announces itself -- and only once, because the
        // command stops at the first file.
        assert_eq!(printed.lines().count(), 1, "{printed:?}");

        fs::remove_dir_all(&dir).ok();
    }

    /// The write ordering is observable, and this pins it: with ONLY the second file
    /// (`msd_<name>_addition.txt`) already present, real Walnut still writes
    /// `msd_<name>.txt` to disk and only then throws — see this module's docs. A
    /// "build both, then write both, guard first" implementation would leave nothing
    /// behind and would pass every language-level check.
    #[test]
    fn a_pre_existing_addition_file_still_leaves_the_representation_file_on_disk() {
        let (session, dir) = temp_session("interleave");
        let bases = dir.join("Custom Bases");
        fs::write(bases.join("msd_pell_addition.txt"), "occupied").unwrap();

        let (err, printed) = run_ost(&session, "pell", "0", "2");
        assert!(matches!(err, Err(OstError::AlreadyExists { .. })));
        // Both announcements printed; the first write went through, the second did not.
        assert_eq!(printed.lines().count(), 2, "{printed:?}");
        assert_eq!(
            fs::read_to_string(bases.join("msd_pell.txt")).unwrap(),
            fs::read_to_string(fixture_dir().join("msd_pell.txt")).unwrap()
        );
        assert_eq!(
            fs::read_to_string(bases.join("msd_pell_addition.txt")).unwrap(),
            "occupied"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// The constructor's rejections reach the command layer with Java's exact text.
    #[test]
    fn constructor_rejections_surface_as_ost_errors() {
        let (session, dir) = temp_session("reject");
        let (err, _) = run_ost(&session, "bad", "1", "");
        assert_eq!(
            err.expect_err("empty period").to_string(),
            "The period cannot be empty."
        );
        let (err, _) = run_ost(&session, "bad", "2 0", "1");
        assert_eq!(
            err.expect_err("non-positive digit").to_string(),
            "Error: All digits of the continued fraction must be positive integers."
        );
        // Nothing was written for either.
        assert!(!dir.join("Custom Bases/msd_bad.txt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    /// `ParseMethods.parseList`'s `UtilityMethods.parseInt` on an `int`-overflowing
    /// digit run — regex-legal (`\d+` constrains shape, not magnitude), a
    /// `NumberFormatException` in Java.
    #[test]
    fn an_i32_overflowing_digit_is_a_parse_error_not_a_panic() {
        let (session, dir) = temp_session("overflow");
        let (err, _) = run_ost(&session, "big", "99999999999", "1");
        assert!(matches!(err, Err(OstError::Parse(_))), "{err:?}");
        fs::remove_dir_all(&dir).ok();
    }

    /// The `int`-width alphabet-size overflow guard, at the exact threshold real Walnut
    /// hits: `d_max + 1 = 1291` is the smallest cube exceeding `Integer.MAX_VALUE`
    /// (1291^3 = 2_151_685_171). Verified live — real `walnut-java` writes
    /// `msd_bigone.txt`, then throws `ArithmeticException: integer overflow` out of
    /// `createAdderAutomaton` and returns to the prompt.
    ///
    /// Before `wr_core::ostrowski::assert_alphabet_size_fits_in_an_int` existed this
    /// silently wrapped the `i32` transition-encode arithmetic in a RELEASE build (a
    /// debug build caught it only by accident, as `attempt to add with overflow`) and
    /// spent ~14 s writing a bogus `msd_bigone_addition.txt`. This test therefore runs
    /// the command through the same panic-recovery boundary `Prover::dispatch` uses, so
    /// it pins the *recovered error*, not the debug-only overflow check.
    #[test]
    fn an_alphabet_size_overflowing_int_errors_after_writing_only_the_first_file() {
        let (session, dir) = temp_session("bigalphabet");
        let caught =
            wr_core::walnut_panic::catch_walnut_panic(std::panic::AssertUnwindSafe(|| {
                run_ost(&session, "bigone", "1291", "1").0.map(|_| ())
            }));
        assert_eq!(caught.unwrap_err(), "integer overflow");

        // Java's on-disk outcome exactly: the representation automaton is already
        // written when `createAdderAutomaton` throws (`Ost.java:18` precedes `:19`).
        let bases = dir.join("Custom Bases");
        assert!(bases.join("msd_bigone.txt").is_file());
        assert!(!bases.join("msd_bigone_addition.txt").exists());

        // One below the threshold still builds (1290^3 = 2_146_689_000 < i32::MAX), so
        // the guard is at Java's boundary and not merely "large inputs are refused".
        let (session2, dir2) = temp_session("bigalphabet-ok");
        let mut ost = Ostrowski::new("justunder", &[1290], &[1]).expect("valid");
        assert_eq!(ost.d_max(), 1289);
        let mut logging = Logging::new();
        let _ = ost.create_representation_automaton(&mut logging);
        drop(session2);

        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&dir2).ok();
    }
}
