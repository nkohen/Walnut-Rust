// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Morphism.java` (18 LOC, the `morphism` command) and `Prover.java`'s
//! inline `promoteCommand` (`:637-650`, the `promote` command) — both operate on
//! [`wr_core::morphism::Morphism`], so they share this module. Not to be confused with
//! `Automata.Morphism` (the primitive itself, `wr_core::morphism`, U17/U24) or this
//! crate's own dispatch layer (`crate::prover`).
//!
//! # `morphism`: define a morphism, print its domain/range, write it twice
//!
//! [`morphism_command`] ports `Morphism.morphismCommand` verbatim, including its
//! console print's exact shape: FOUR `System.out.print` calls back to back with **no**
//! trailing newline and **no** separator beyond the two literal strings — see
//! [`print_domain_and_range`]'s doc comment for the domain/range bracket-style
//! asymmetry (`[…]` vs `{…}`) this reproduces. The morphism is written to BOTH
//! `Result/<name>.txt` and `Morphism Library/<name>.txt` (`Morphism.java:15-16`) —
//! two real file writes, not "write once, copy" the way ordinary automaton output
//! works (`crate::automaton_output::write_automata`'s copy-based shape doesn't apply
//! here at all; [`wr_core::morphism::Morphism::write_to_file`] is called twice
//! independently, matching Java's two `M.write(...)` calls exactly).
//!
//! # `promote`: no uniformity requirement (unlike `image`)
//!
//! [`promote_command`] ports `Prover.promoteCommand` verbatim. Unlike
//! `crate::image::image` (which calls `requirePositiveUniformLength()` first),
//! `promoteCommand` never does — [`wr_core::morphism::Morphism::to_word_automaton`]
//! has no uniformity precondition of its own either (see that method's own doc
//! comment), so a non-uniform morphism promotes just fine here. `promoteCommand` DOES
//! use `UtilityMethods.validateFile` (unlike `image`'s `UtilityMethods.readFromFile`,
//! which tolerates a missing file as `""`) — a missing morphism file is a real,
//! reported error here, not silently treated as an empty morphism.
//!
//! There IS one requirement `promote` does impose, easy to mistake for "none at all":
//! the morphism's LONGEST image must be at least two letters. That is not a check
//! `promoteCommand` writes — it falls out of `Morphism.java:90` constructing a real
//! `NumberSystem("msd_" + maxImageLength)`, whose constructor refuses base 0/1. So real
//! Walnut answers `promote P h;` on `0->1 1->0` with "Number system msd_1 is not
//! defined." and writes nothing; see
//! [`wr_core::morphism::MorphismError::NumberSystemNotDefined`].

use std::io::{self, Write};

use wr_core::morphism::{Morphism, MorphismError};
use wr_core::numsys::TXT_EXTENSION;
use wr_core::util::validate_file;
use wr_io::parse_methods::{parse_morphism, ParseMethodsError};

use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure [`morphism_command`]/[`promote_command`] can report.
#[derive(Debug)]
pub enum MorphismCommandError {
    /// `new Automata.Morphism(morphismDefinition)` / `new Morphism(mapString)`, via
    /// [`wr_io::parse_methods::parse_morphism`].
    Parse(ParseMethodsError),
    /// `UtilityMethods.validateFile(morphismAddress)` (`Prover.java:643`) —
    /// `promoteCommand` only; `morphismCommand` has no file-existence precondition
    /// (it defines a NEW morphism, it doesn't read one).
    InvalidFile(String),
    /// `h.toWordAutomaton()` (`Prover.java:646`) — `promoteCommand` only. See
    /// [`wr_core::morphism::MorphismError`] for its three reachable failure modes: a
    /// negative image value, `Morphism.java:90`'s `NumberSystem` validation
    /// (`maxImageLength < 2`, i.e. "Number system msd_1 is not defined."), and WB-036's
    /// domain/image-range mismatch — in that priority order, matching Java's own
    /// statement order.
    Promote(MorphismError),
    /// A real I/O failure writing the morphism (`morphismCommand`) or the promoted
    /// automaton (`promoteCommand`).
    Io(io::Error),
}

impl std::fmt::Display for MorphismCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MorphismCommandError::Parse(e) => write!(f, "{e}"),
            MorphismCommandError::InvalidFile(m) => write!(f, "{m}"),
            MorphismCommandError::Promote(e) => write!(f, "{e}"),
            MorphismCommandError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MorphismCommandError {}

impl From<io::Error> for MorphismCommandError {
    fn from(e: io::Error) -> Self {
        MorphismCommandError::Io(e)
    }
}

/// `Morphism.morphismCommand(String morphismDefinition, String name)`
/// (`Main/Commands/Morphism.java:9-17`), writing its console text to the real process
/// stdout.
pub fn morphism_command(
    session: &Session,
    morphism_definition: &str,
    name: &str,
) -> Result<(), MorphismCommandError> {
    morphism_command_to(session, morphism_definition, name, &mut io::stdout())
}

/// As [`morphism_command`], with an injectable stdout sink — this module's own tests'
/// seam, matching every other command in this crate that prints directly to the
/// console (`crate::eval_def`, `crate::prover_helper`).
pub fn morphism_command_to(
    session: &Session,
    morphism_definition: &str,
    name: &str,
    stdout: &mut dyn Write,
) -> Result<(), MorphismCommandError> {
    // `Automata.Morphism M = new Automata.Morphism(morphismDefinition);` (`:10`).
    let mapping = parse_morphism(morphism_definition).map_err(MorphismCommandError::Parse)?;
    let m = Morphism::from_mapping(mapping);

    // `System.out.print("Defined with domain "); System.out.print(M.mapping.keySet());
    //  System.out.print(" and range "); System.out.print(M.range);` (`:11-14`).
    print_domain_and_range(&m, stdout)?;

    // `M.write(Session.getAddressForResult() + name + Prover.TXT_EXTENSION);` (`:15`).
    let result_path = format!(
        "{}{name}{TXT_EXTENSION}",
        session.paths().address_for_result()
    );
    m.write_to_file(&result_path)?;

    // `M.write(Session.getWriteAddressForMorphismLibrary() + name + Prover.TXT_EXTENSION);`
    // (`:16`).
    let library_path = format!(
        "{}{name}{TXT_EXTENSION}",
        session.paths().write_address_for_morphism_library()
    );
    m.write_to_file(&library_path)?;

    Ok(())
}

/// `System.out.print("Defined with domain "); System.out.print(M.mapping.keySet());
///  System.out.print(" and range "); System.out.print(M.range);` (`Morphism.java:11-14`)
/// — four bare prints, no trailing newline.
///
/// **The two collections print in DIFFERENT bracket styles, faithfully reproduced:**
/// `M.mapping.keySet()` is a real `java.util.Set` (from a `TreeMap`), whose
/// `AbstractCollection.toString()` uses `[elem, elem, …]` (square brackets, SORTED —
/// `TreeMap`'s own iteration order, matching this port's `BTreeMap`/`BTreeSet`
/// exactly). `M.range` is a fastutil `IntOpenHashSet`, printed `{elem, elem, …}`
/// (curly braces) — see [`wr_core::morphism::Morphism::range`]'s own doc comment for
/// the accepted, display-only ordering divergence this reproduces (sorted here vs.
/// fastutil's unspecified hash order in real Java).
fn print_domain_and_range(m: &Morphism, out: &mut dyn Write) -> io::Result<()> {
    write!(out, "Defined with domain ")?;
    write!(
        out,
        "[{}]",
        m.mapping
            .keys()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    write!(out, " and range ")?;
    write!(
        out,
        "{{{}}}",
        m.range
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    Ok(())
}

/// `Prover.promoteCommand(String s)` (`Prover.java:637-650`).
pub fn promote_command(
    session: &Session,
    s: &str,
    morphism_name: &str,
    name: &str,
) -> Result<TestCase, MorphismCommandError> {
    // `String morphismAddress = Session.getReadFileForMorphismLibrary(
    //  m.group(GROUP_PROMOTE_MORPHISM) + TXT_EXTENSION);` (`:640-641`).
    let morphism_address = session
        .paths()
        .read_file_for_morphism_library(&format!("{morphism_name}{TXT_EXTENSION}"));

    // `String mapString = Files.readString(Paths.get(UtilityMethods.validateFile(
    //  morphismAddress).toURI()));` (`:642-643`) -- unlike `image`'s
    // `UtilityMethods.readFromFile` (tolerates a missing file as `""`), a missing
    // morphism file here is a real, reported error.
    validate_file(&morphism_address).map_err(MorphismCommandError::InvalidFile)?;
    let map_string = std::fs::read_to_string(&morphism_address)?;

    // `Morphism h = new Morphism(mapString); Automaton P = h.toWordAutomaton();`
    // (`:645-646`).
    let mapping = parse_morphism(&map_string).map_err(MorphismCommandError::Parse)?;
    let h = Morphism::from_mapping(mapping);
    let mut p = h
        .to_word_automaton()
        .map_err(MorphismCommandError::Promote)?;

    // `P.writeAutomata(s, Session.getWriteAddressForWordsLibrary(), m.group(
    //  GROUP_PROMOTE_NAME), true);` (`:648`).
    write_automata(
        session,
        &mut p,
        s,
        &session.paths().write_address_for_words_library(),
        name,
        true,
    )?;

    Ok(TestCase::from_automaton(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-morphism-{tag}-{}-{}",
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

    // -- morphism_command -------------------------------------------------------------

    #[test]
    fn morphism_command_prints_domain_and_range_with_no_trailing_newline() {
        let (session, dir) = temp_session("print");
        let mut out: Vec<u8> = Vec::new();
        morphism_command_to(&session, "0->01,1->10", "h", &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Defined with domain [0, 1] and range {0, 1}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn morphism_command_writes_both_the_result_and_the_library_copy() {
        let (session, dir) = temp_session("write");
        let mut out: Vec<u8> = Vec::new();
        morphism_command_to(&session, "0->01,1->10", "h", &mut out).unwrap();

        let result_text = fs::read_to_string(dir.join("Result").join("h.txt")).unwrap();
        let library_text = fs::read_to_string(dir.join("Morphism Library").join("h.txt")).unwrap();
        assert_eq!(result_text, "0 -> 01\n1 -> 10\n");
        assert_eq!(result_text, library_text);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn morphism_command_range_prints_with_curly_braces_domain_with_square_brackets() {
        // "0->0123 1->21 2->03 3->23" -- domain {0,1,2,3}, range {0,1,2,3} too, but a
        // morphism whose range genuinely differs from its domain pins the two bracket
        // styles independently. Use gam: "0->01 1->21 2->03 3->23" (domain and range
        // both {0,1,2,3} again) -- so instead pick a morphism whose domain is NOT its
        // full range: "0->5 1->5" (domain {0,1}, range {5}).
        let (session, dir) = temp_session("brackets");
        let mut out: Vec<u8> = Vec::new();
        morphism_command_to(&session, "0->5,1->5", "g", &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Defined with domain [0, 1] and range {5}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn morphism_command_rejects_an_empty_definition() {
        let (session, dir) = temp_session("empty");
        let mut out: Vec<u8> = Vec::new();
        let err = morphism_command_to(&session, "", "h", &mut out).unwrap_err();
        assert!(matches!(err, MorphismCommandError::Parse(_)));
        fs::remove_dir_all(&dir).ok();
    }

    // -- promote_command ---------------------------------------------------------------

    #[test]
    fn promote_command_thue_morse_end_to_end() {
        let (session, dir) = temp_session("promote-tm");
        fs::write(
            dir.join("Morphism Library").join("h.txt"),
            "0 -> 01\n1 -> 10\n",
        )
        .unwrap();

        let tc = promote_command(&session, "promote P h;", "h", "P").unwrap();
        let p = tc.automaton_pairs()[0].automaton().unwrap();

        // The whole transition table, not just the state count: the promoted automaton
        // is the Thue-Morse DFAO itself -- one state per domain letter (state q's output
        // IS q), one transition per image POSITION (`d[q][i] = h(q)[i]`). So from state 0
        // (image "01"): position 0 -> 0, position 1 -> 1; from state 1 (image "10"):
        // position 0 -> 1, position 1 -> 0.
        assert_eq!(p.fa.q, 2);
        assert_eq!(p.fa.q0, 0);
        assert_eq!(p.fa.o, vec![0, 1]);
        assert_eq!(p.alphabet, vec![vec![0, 1]]);
        assert_eq!(p.msd, vec![Some(true)]);
        assert_eq!(
            p.fa.d[0],
            std::collections::BTreeMap::from([(0, vec![0]), (1, vec![1])])
        );
        assert_eq!(
            p.fa.d[1],
            std::collections::BTreeMap::from([(0, vec![1]), (1, vec![0])])
        );

        assert!(dir.join("Result").join("P.txt").is_file());
        assert!(dir.join("Word Automata Library").join("P.txt").is_file());
        let written = fs::read_to_string(dir.join("Result").join("P.txt")).unwrap();
        assert!(
            written.starts_with("msd_2"),
            "the promoted automaton is read in base maxImageLength = 2: {written:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn promote_command_keeps_an_unreachable_domain_letter_state() {
        // The end-to-end counterpart of `wr_core::morphism`'s two `canonized`-flag
        // tests: `0->01 1->10 2->22` has a state (2) that nothing but itself transitions
        // into, so an ordinary `canonize()` inside the writer would drop it. Java sets
        // `setCanonized(true)` precisely to stop that, and the observable consequence is
        // the WRITTEN FILE -- which must still describe all three states.
        let (session, dir) = temp_session("promote-unreachable");
        fs::write(
            dir.join("Morphism Library").join("h.txt"),
            "0 -> 01\n1 -> 10\n2 -> 22\n",
        )
        .unwrap();

        let tc = promote_command(&session, "promote P h;", "h", "P").unwrap();
        let p = tc.automaton_pairs()[0].automaton().unwrap();
        assert_eq!(p.fa.q, 3);
        assert_eq!(p.fa.o, vec![0, 1, 2]);

        let written = fs::read_to_string(dir.join("Result").join("P.txt")).unwrap();
        // State 2 (output 2, self-looping on both digit positions) survived the write.
        assert!(
            written.contains("2 2"),
            "state 2 was pruned by canonicalization: {written:?}"
        );
        // And it really is a 3-state automaton on disk: three "<state> <output>" lines.
        let state_lines = written
            .lines()
            .filter(|l| {
                let mut parts = l.split_whitespace();
                matches!(
                    (parts.next(), parts.next(), parts.next()),
                    (Some(a), Some(b), None)
                        if a.parse::<i32>().is_ok() && b.parse::<i32>().is_ok()
                )
            })
            .count();
        assert_eq!(state_lines, 3, "expected 3 state lines in {written:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn promote_command_rejects_a_morphism_with_max_image_length_one() {
        // Real Walnut refuses this outright: `Morphism.java:90` builds a
        // `NumberSystem("msd_1")`, whose constructor throws "Number system msd_1 is not
        // defined." -- and, crucially, writes NO file.
        let (session, dir) = temp_session("promote-msd1");
        fs::write(
            dir.join("Morphism Library").join("h.txt"),
            "0 -> 1\n1 -> 0\n",
        )
        .unwrap();

        let err = promote_command(&session, "promote P h;", "h", "P").unwrap_err();
        assert!(matches!(
            err,
            MorphismCommandError::Promote(MorphismError::NumberSystemNotDefined(1))
        ));
        assert_eq!(err.to_string(), "Number system msd_1 is not defined.");
        assert!(
            !dir.join("Result").join("P.txt").exists(),
            "nothing may be written when promote is refused"
        );
        assert!(!dir.join("Word Automata Library").join("P.txt").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn promote_command_does_not_require_uniform_length() {
        // Unlike `image`, `promote` has no uniformity requirement -- "0->0 1->10" is
        // non-uniform (image lengths 1 and 2) but still promotes successfully.
        let (session, dir) = temp_session("promote-nonuniform");
        fs::write(
            dir.join("Morphism Library").join("h.txt"),
            "0 -> 0\n1 -> 10\n",
        )
        .unwrap();

        let tc = promote_command(&session, "", "h", "P").unwrap();
        let p = tc.automaton_pairs()[0].automaton().unwrap();
        assert_eq!(p.fa.q, 2); // max_entry(=1) + 1

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn promote_command_reports_a_missing_morphism_file() {
        let (session, dir) = temp_session("promote-missing");
        let err = promote_command(&session, "", "nope", "P").unwrap_err();
        assert!(matches!(err, MorphismCommandError::InvalidFile(_)));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn promote_command_wb036_domain_gap_surfaces_as_an_error_not_a_panic() {
        let (session, dir) = temp_session("promote-wb036");
        fs::write(
            dir.join("Morphism Library").join("badmor.txt"),
            "0 -> 05\n1 -> 10\n",
        )
        .unwrap();

        let err = promote_command(&session, "", "badmor", "P").unwrap_err();
        assert!(matches!(
            err,
            MorphismCommandError::Promote(MorphismError::DomainDoesNotCoverImageRange)
        ));

        fs::remove_dir_all(&dir).ok();
    }
}
