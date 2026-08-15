// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Image.java` (58 LOC) — the `image` command: applies a morphism to a
//! word automaton to produce its "image" under that morphism.
//!
//! # Shape: one intermediate evaluation per value in the morphism's range
//!
//! [`image`] mirrors `Image.image` exactly: read the morphism (by name, from the
//! Morphism Library) and the "old" word automaton (by name, from the Word Automata
//! Library), require the morphism be uniform-length (unlike `promote`'s
//! [`crate::morphism::promote_command`], which has no such requirement — see that
//! module's docs), then for every distinct value `v` in the morphism's range (sorted
//! ascending — matches `h.range`'s own `BTreeSet` order, so `range.sort()`'s Java
//! statement is a no-op here, not skipped), build an "intermediary predicate" via
//! [`wr_core::morphism::Morphism::make_inter_predicate`] and evaluate it — one real
//! `eval`-shaped computation per range value, exactly as `EvalDef.getImageEval` does.
//! The per-value automata are then folded together with
//! [`wr_core::logicalops::combine`] (Java's `AutomatonLogicalOps.combine`), producing
//! one DFAO whose output at each accepted word is the range value that "won".
//!
//! # `getImageEval`, generalized as `evaluate_with_logging` — but NOT `computeHeadless`
//!
//! `EvalDef.getImageEval` (`EvalDef.java:93-99`) constructs `new EvalDef(printFlag,
//! false)` (i.e. `Logging.configureForCommand(printFlag, false)`, printDetails always
//! `false`) and computes, but — unlike `computeHeadless`/`evalDefCommand`'s two
//! branches — does **not** print the `TRUE`/`FALSE` console line at all. So this
//! module calls [`wr_logic::eval::evaluate_with_logging`] directly (after configuring
//! `Logging` itself, matching Java's per-call `new EvalDef(...)` constructor), not
//! `crate::eval_def`'s higher-level wrappers, which both print that line.
//!
//! `Logging.configureForCommand`/a fresh [`FreshIdentifiers`] are re-done on EVERY
//! loop iteration, matching Java's `new EvalDef(...)`/`new Predicate(...)` being
//! constructed fresh inside the loop body (`getImageEval` is called once per range
//! value) — not hoisted out as a one-time setup.
//!
//! # The empty-range case is provably unreachable here (not defensively handled)
//!
//! If `h.range` were empty, Java's `AutomatonLogicalOps.combine(null, …)` would NPE
//! immediately (`Automaton first = A.clone();` on a `null` `A`) — but that shape
//! cannot occur through this command: [`Morphism::require_positive_uniform_length`]
//! already rejects `length <= 0` before the range loop ever runs, and a uniform,
//! POSITIVE length forces at least one letter's image to be non-empty, which forces
//! `range` to be non-empty too. [`image`] therefore takes the first per-value
//! automaton via `.expect(..)` (a loud, diagnosable panic if this invariant is ever
//! violated) rather than returning a defensive `Result::Err` for a state proven
//! unreachable, matching this crate's convention elsewhere (e.g. `infinite.rs`'s
//! `find_path().expect(..)`).
//!
//! # `determineImageNumberSystemPrefix`: the pre-existing representation-gap note
//!
//! [`determine_image_number_system_prefix`] ports `Image.determineImageNumberSystemPrefix`
//! (`:48-57`) using the same canonical-name reconstruction `wr-io`'s `writer.rs` already
//! uses for the identical representation gap (`wr_core::automaton::Automaton` carries only
//! `msd: Option<bool>` + the track's alphabet length, not a full `NumberSystem` object with
//! its original name string) — `docs/WALNUT-BUGS.md`'s "Not-yet-confirmed" section already
//! flags this exact method as a pre-existing open question (is indexing a
//! non-arithmetic-numeration word automaton via `image()` even a supported scenario?), not
//! a confirmed bug; nothing here resolves that question, it only reproduces Java's `ns ==
//! null -> ""` / `"?" + ns` shape as faithfully as this crate's representation allows.

use wr_core::automaton::Automaton;
use wr_core::logging::Logging;
use wr_core::logicalops::combine;
use wr_core::morphism::{Morphism, MorphismError};
use wr_core::util::read_from_file;
use wr_io::parse_methods::{parse_morphism, ParseMethodsError};
use wr_logic::eval::{evaluate_with_logging, EvalError};
use wr_logic::predicate_env::{FreshIdentifiers, PredicateEnvError};

use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;
use crate::walnut_exception as msg;

/// Every failure [`image`] can report.
#[derive(Debug)]
pub enum ImageError {
    /// A real I/O failure reading the morphism file or writing the result. The
    /// morphism-file read itself (`UtilityMethods.readFromFile`) does NOT error on a
    /// merely-missing file (see [`wr_core::util::read_from_file`]'s own docs) — this
    /// variant is only a genuine, unexpected I/O failure on either side.
    Io(std::io::Error),
    /// `new Automata.Morphism(...)` (`Image.java:20`), via
    /// [`wr_io::parse_methods::parse_morphism`] — e.g. the named morphism file has no
    /// valid mappings (including "file did not exist", since a missing file reads as
    /// `""`, which parses to zero mappings).
    MorphismParse(ParseMethodsError),
    /// `h.requirePositiveUniformLength()` (`Image.java:21`).
    RequireUniform(MorphismError),
    /// `new Automaton(Session.getReadFileForWordsLibrary(...))` (`Image.java:23`)
    /// failing to read the "old word" automaton.
    OldWordRead(PredicateEnvError),
    /// `determineImageNumberSystemPrefix`'s inline throw (`Image.java:50`): the old
    /// word automaton is not unary (single-track).
    NotUnaryWordAutomaton { name: String },
    /// A per-value intermediary evaluation (`EvalDef.getImageEval`) failed.
    Eval(EvalError),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Io(e) => write!(f, "{e}"),
            ImageError::MorphismParse(e) => write!(f, "{e}"),
            ImageError::RequireUniform(e) => write!(f, "{e}"),
            ImageError::OldWordRead(e) => write!(f, "{e}"),
            ImageError::NotUnaryWordAutomaton { name } => {
                write!(f, "{}", msg::image_requires_unary_word_automaton(name))
            }
            ImageError::Eval(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl From<std::io::Error> for ImageError {
    fn from(e: std::io::Error) -> Self {
        ImageError::Io(e)
    }
}

impl From<EvalError> for ImageError {
    fn from(e: EvalError) -> Self {
        ImageError::Eval(e)
    }
}

/// `Image.image(String s, String morphismFileName, String imageOldName, String
/// imageNewName, boolean printFlag)` (`Image.java:17-46`).
///
/// `s` is the raw command text (Java's nullable `String`, threaded straight into
/// [`write_automata`]'s `.gv` label, same as every other command's `s` parameter in
/// this crate).
#[allow(clippy::too_many_arguments)]
pub fn image(
    session: &Session,
    logging: &mut Logging,
    s: &str,
    morphism_file_name: &str,
    image_old_name: &str,
    image_new_name: &str,
    print_flag: bool,
) -> Result<TestCase, ImageError> {
    // `String morphismAddress = Session.getReadFileForMorphismLibrary(morphismFileName +
    //  TXT_EXTENSION); Automata.Morphism h = new Morphism(UtilityMethods.readFromFile(
    //  morphismAddress));` (`:18-20`).
    let morphism_address = session
        .paths()
        .read_file_for_morphism_library(&format!("{morphism_file_name}{}", TXT_EXTENSION));
    let map_string = read_from_file(&morphism_address)?;
    let mapping = parse_morphism(&map_string).map_err(ImageError::MorphismParse)?;
    let h = Morphism::from_mapping(mapping);

    // `h.requirePositiveUniformLength();` (`:21`).
    h.require_positive_uniform_length()
        .map_err(ImageError::RequireUniform)?;

    // `Automaton oldWord = new Automaton(Session.getReadFileForWordsLibrary(imageOldName +
    //  TXT_EXTENSION));` (`:23`).
    let old_word_address = session
        .paths()
        .read_file_for_words_library(&format!("{image_old_name}{}", TXT_EXTENSION));
    let old_word = session
        .libraries()
        .read_library_automaton(&old_word_address)
        .map_err(ImageError::OldWordRead)?;
    let num_sys_name = determine_image_number_system_prefix(&old_word, image_old_name)?;

    // `List<Integer> range = new ArrayList<>(h.range); range.sort(Integer::compareTo);`
    // (`:26-27`) -- `h.range` is already a `BTreeSet`, i.e. already sorted; see module
    // docs.
    let mut per_value_automata: Vec<Automaton> = Vec::with_capacity(h.range.len());
    let mut outputs: Vec<i32> = Vec::with_capacity(h.range.len());
    for value in h.range.iter().copied() {
        let predicate_str = h.make_inter_predicate(value, image_old_name, &num_sys_name);

        // `EvalDef.getImageEval(predicateStr, printFlag)` (`:34`) -- see module docs:
        // NOT `computeHeadless` (no TRUE/FALSE print), and both the `Logging`
        // configuration and the fresh-identifier counter are re-done every iteration,
        // matching Java's per-call `new EvalDef(...)`/`new Predicate(...)`.
        logging.configure_for_command(print_flag, false);
        let mut fresh = FreshIdentifiers::new();
        let value_automaton =
            evaluate_with_logging(session.predicate_env(), logging, &mut fresh, &predicate_str)?;

        per_value_automata.push(value_automaton);
        outputs.push(value);
    }

    // `Automaton image = AutomatonLogicalOps.combine(first, subautomata, outputs);`
    // (`:43`) -- see module docs on why `per_value_automata` is provably non-empty here.
    let mut iter = per_value_automata.into_iter();
    let first = iter.next().expect(
        "range is non-empty: require_positive_uniform_length already rejected length <= 0, \
         and a uniform positive length forces at least one non-empty image, which forces \
         h.range to be non-empty -- see module docs",
    );
    let mut image = combine(&first, iter.collect(), &outputs);

    // `image.writeAutomata(s, Session.getWriteAddressForWordsLibrary(), imageNewName,
    //  true);` (`:44`).
    write_automata(
        session,
        &mut image,
        s,
        &session.paths().write_address_for_words_library(),
        image_new_name,
        true,
    )?;

    Ok(TestCase::from_automaton(image))
}

/// Java's `TXT_EXTENSION` (`Prover.TXT_EXTENSION`) — aliased the same way
/// `crate::prover`/`crate::automaton_output` alias it, to avoid a second independently
/// spelled `".txt"` literal.
const TXT_EXTENSION: &str = wr_core::numsys::TXT_EXTENSION;

/// `Image.determineImageNumberSystemPrefix(Automaton, String)` (`:48-57`) — see module
/// docs on the canonical-name reconstruction this crate's representation gap forces.
fn determine_image_number_system_prefix(
    word: &Automaton,
    word_name: &str,
) -> Result<String, ImageError> {
    if word.arity() != 1 || word.msd.len() != 1 {
        return Err(ImageError::NotUnaryWordAutomaton {
            name: word_name.to_string(),
        });
    }
    match word.msd[0] {
        // `ns == null` -- this crate's `None` stand-in.
        None => Ok(String::new()),
        Some(is_msd) => {
            let base = word.alphabet[0].len();
            Ok(format!("?{}_{base}", if is_msd { "msd" } else { "lsd" }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-image-{tag}-{}-{}",
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

    // -- determine_image_number_system_prefix --------------------------------------

    fn unary_automaton(alphabet: Vec<i32>, msd: Option<bool>) -> Automaton {
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: alphabet.len(),
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![alphabet],
            vec!["x".to_string()],
            vec![msd],
        )
    }

    #[test]
    fn number_system_prefix_is_empty_when_no_number_system() {
        // Mirrors ImageTest.testDetermineImageNumberSystemPrefixEmptyWhenNoNumberSystem.
        let word = unary_automaton(vec![0, 1], None);
        assert_eq!(
            determine_image_number_system_prefix(&word, "plain").unwrap(),
            ""
        );
    }

    #[test]
    fn number_system_prefix_reconstructs_the_canonical_msd_name() {
        let word = unary_automaton(vec![0, 1, 2], Some(true));
        assert_eq!(
            determine_image_number_system_prefix(&word, "w").unwrap(),
            "?msd_3"
        );
    }

    #[test]
    fn number_system_prefix_reconstructs_the_canonical_lsd_name() {
        let word = unary_automaton(vec![0, 1], Some(false));
        assert_eq!(
            determine_image_number_system_prefix(&word, "w").unwrap(),
            "?lsd_2"
        );
    }

    #[test]
    fn number_system_prefix_rejects_non_unary_arity() {
        // Mirrors ImageTest.testDetermineImageNumberSystemPrefixThrowsForNonUnaryArity
        // (there via the TRUE automaton, arity 0; here via a genuine 2-track automaton
        // -- both are "arity != 1").
        let word = Automaton::true_false(true);
        let err = determine_image_number_system_prefix(&word, "trueAuto").unwrap_err();
        assert!(matches!(err, ImageError::NotUnaryWordAutomaton { name } if name == "trueAuto"));
    }

    // -- image (end-to-end) ---------------------------------------------------------

    fn write_morphism(dir: &std::path::Path, name: &str, text: &str) {
        fs::write(
            dir.join("Morphism Library").join(format!("{name}.txt")),
            text,
        )
        .unwrap();
    }

    #[test]
    fn image_thue_morse_end_to_end() {
        // Thue-Morse morphism h: 0->01, 1->10, applied to the Thue-Morse word T itself
        // (T[n] = parity of the number of 1s in n's base-2 representation, msd_2) --
        // h's image of T should be language-equivalent to T again (a classical
        // Thue-Morse identity: h(T) = T).
        let (session, dir) = temp_session("thue-morse");
        write_morphism(&dir, "h", "0->01 1->10");

        // T: msd_2, output 0 at even parity, 1 at odd parity -- a small 2-state DFAO.
        // state 0 (parity even, output 0) --0--> 0, --1--> 1
        // state 1 (parity odd,  output 1) --0--> 1, --1--> 0
        let mut d = vec![BTreeMap::new(), BTreeMap::new()];
        d[0].insert(0, vec![0]);
        d[0].insert(1, vec![1]);
        d[1].insert(0, vec![1]);
        d[1].insert(1, vec![0]);
        let t = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d,
            },
            vec![vec![0, 1]],
            vec!["n".to_string()],
            vec![Some(true)],
        );
        let mut t_for_write = t.clone();
        wr_io::writer::write_automaton_txt(
            &mut t_for_write,
            dir.join("Word Automata Library").join("T.txt"),
        )
        .unwrap();

        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let tc = image(
            &session,
            &mut logging,
            "image FS h T;",
            "h",
            "T",
            "FS",
            false,
        )
        .unwrap();

        assert!(dir.join("Result").join("FS.txt").is_file());
        assert!(dir.join("Word Automata Library").join("FS.txt").is_file());
        let result = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(!result.fa.is_true_false_automaton());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn image_requires_uniform_length() {
        let (session, dir) = temp_session("non-uniform");
        write_morphism(&dir, "h", "0->0 1->10");

        let mut t_for_write = unary_automaton(vec![0, 1], Some(true));
        wr_io::writer::write_automaton_txt(
            &mut t_for_write,
            dir.join("Word Automata Library").join("T.txt"),
        )
        .unwrap();

        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let err = image(&session, &mut logging, "", "h", "T", "FS", false).unwrap_err();
        assert!(matches!(err, ImageError::RequireUniform(_)));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn image_rejects_a_non_unary_old_word() {
        let (session, dir) = temp_session("non-unary");
        write_morphism(&dir, "h", "0->01 1->10");

        // A 2-track word automaton: `image` requires a unary (single-track) old word.
        let mut multi = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 4,
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["a".to_string(), "b".to_string()],
            vec![Some(true), Some(true)],
        );
        wr_io::writer::write_automaton_txt(
            &mut multi,
            dir.join("Word Automata Library").join("multi.txt"),
        )
        .unwrap();

        let mut logging =
            Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
        let err = image(
            &session,
            &mut logging,
            "",
            "h",
            "multi",
            "shouldNotBeWritten",
            false,
        )
        .unwrap_err();
        assert!(matches!(err, ImageError::NotUnaryWordAutomaton { name } if name == "multi"));

        fs::remove_dir_all(&dir).ok();
    }
}
