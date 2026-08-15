// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Join.java` (93 LOC) — the `join` command: reads a list of named
//! automata (each with its own explicit `[x][y]…` input labeling), cross-products them
//! pairwise ("first non-zero output wins" among the chained DFAOs, `Prover.FIRST_OP`),
//! and writes the result.
//!
//! # Double-parsing: `crate::prover`'s `join` pattern only validates + captures a blob
//!
//! `Prover.RE_FOR_join_CMD` (already ported, `crate::prover::patterns().join`) captures
//! the ENTIRE `name[x][y] name2[z] …` text as one big group (`GROUP_JOIN_AUTOMATA`) —
//! it does not itself split it into individual `(name, labels)` pairs. `Join.java` has
//! its OWN two private patterns for that (`PAT_FOR_AN_AUTOMATON_IN_join_CMD`/
//! `PAT_FOR_AN_AUTOMATON_INPUT_IN_join_CMD`), applied via `Matcher.find()` loops to
//! RE-SCAN that captured blob — [`join_command`] below does the same, using
//! `regex_automata::meta::Regex::captures_iter` for the same non-overlapping,
//! left-to-right `find()`-loop semantics. The two patterns' text is IDENTICAL to a
//! substring already proven to compile inside `crate::prover`'s own `join` pattern
//! (the `[a-zA-Z&&[^AE]]` Java character-class-intersection syntax genuinely survives
//! the port to `regex-automata`, per `PORTING.md`'s Ruling 2), so no new dialect risk
//! here — only the usual `]`-escaping/`\s`\`\w` ASCII-class translation.
//!
//! # `join`, not `combine`: NO relabeling step
//!
//! Unlike `AutomatonLogicalOps.combine` (`wr_core::logicalops::combine`, U24's sibling
//! primitive for `image`), which forces every operand to share ONE label set via
//! `randomLabel()`/`setLabel()` (because `combine`'s automata all describe outputs of
//! the SAME sequence), `Join.join` (`:72-92`) does **no** relabeling at all — each
//! automaton keeps whatever label the caller assigned via `M.setLabel(label)`
//! (`joinCommand`'s own per-automaton `[x]`/`[y]` argument parsing, `:55`), and
//! [`wr_core::product::cross_product`]'s own label/alphabet-union bookkeeping handles
//! automata over genuinely DIFFERENT variable sets — the entire point of `join`.
//!
//! # `FIRST_OP`: "first non-zero output wins"
//!
//! `ProductStrategies.determineOutput`'s `Prover.FIRST_OP` arm (`ProductStrategies.java:186`):
//! `aP == 0 ? mQ : aP` — if the accumulated automaton's output at this state pair is
//! `0`, take the next automaton's; otherwise keep the accumulated one. [`join`] passes
//! exactly this closure to [`wr_core::product::cross_product`].
//!
//! # `totalize()`, not `totalize_relaxed` at the call site — but the RELAXED underlying `Fa` op
//!
//! `first.fa.totalize(); next.fa.totalize();` (`:82-83`) is Java's real, non-asserting
//! `FA.totalize()` — [`wr_core::logicalops::totalize`] (widened to `pub` by this unit
//! for exactly this call site; see its own doc comment), not the stricter
//! [`wr_core::fa::Fa::totalize`], which `logicalops.rs`'s own module docs already flag
//! as unsafe to use here (`Fa::totalize` asserts determinism; Java's real `FA.totalize()`
//! does not, and a `join` operand read straight from a library file could in principle
//! be non-deterministic before whatever earlier step produced it).
//!
//! # WB-037 (`docs/WALNUT-BUGS.md`): a `join` with zero automata crashes
//!
//! `Join.joinCommand`'s `Automaton N = subautomata.remove(0);` (`:58`) has no empty-list
//! guard: `join J;` (syntactically valid — `Prover.RE_FOR_join_CMD`'s automata group is
//! `(...)*`, zero-or-more) reaches this line with an empty `subautomata` and throws
//! `IndexOutOfBoundsException: Index 0 out of bounds for length 0`, confirmed live
//! against real `walnut-java`. [`join_command`] reproduces this as
//! [`JoinError::NoAutomataSpecified`] — a `Result::Err`, not a `panic!`, per the
//! WB-002/033/034/035/036 precedent for a genuine, unguarded Java `RuntimeException`
//! that `Prover.dispatch`'s top-level catch recovers from.

use std::path::Path;

use regex_automata::meta::Regex;
use regex_automata::util::captures::Captures;
use regex_automata::Input;

use wr_core::automaton::Automaton;
use wr_core::logicalops::totalize;
use wr_core::numsys::TXT_EXTENSION;
use wr_core::product::cross_product;
use wr_core::word_automaton::minimize_with_output;
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_output::write_automata;
use crate::prover::RE_WORD_OF_CMD_NO_SPC;
use crate::prover_helper::determine_out_library;
use crate::session::Session;
use crate::test_case::TestCase;
use crate::walnut_exception as msg;

/// Every failure [`join_command`] can report.
#[derive(Debug)]
pub enum JoinError {
    /// `new Automaton(address)` failing to read one of the named automata
    /// (`Join.java:37`/`:42`).
    Read(PredicateEnvError),
    /// `Join.joinCommand`'s inline throw (`:53`): the number of `[x]`/`[y]` labels
    /// supplied for an automaton doesn't match its real track count.
    LabelMismatch { automaton_name: String },
    /// WB-037 (`docs/WALNUT-BUGS.md`, see module docs): `join` was given zero
    /// automata.
    NoAutomataSpecified,
    /// A real I/O failure writing the joined result.
    Io(std::io::Error),
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinError::Read(e) => write!(f, "{e}"),
            JoinError::LabelMismatch { automaton_name } => {
                write!(f, "{}", msg::join_input_count_mismatch(automaton_name))
            }
            JoinError::NoAutomataSpecified => write!(
                f,
                "join requires at least one automaton (WB-037: real Walnut throws \
                 IndexOutOfBoundsException for this shape)"
            ),
            JoinError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for JoinError {}

impl From<std::io::Error> for JoinError {
    fn from(e: std::io::Error) -> Self {
        JoinError::Io(e)
    }
}

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("join pattern failed to compile: {pattern}: {e}"))
}

fn group<'h>(caps: &Captures, hay: &'h str, index: usize) -> Option<&'h str> {
    caps.get_group(index).map(|span| &hay[span])
}

/// `Join.PAT_FOR_AN_AUTOMATON_IN_join_CMD` (`:21-22`): `RE_WORD_OF_CMD_NO_SPC +
/// "((\s*\[\s*[a-zA-Z&&[^AE]]\w*\s*])+)"`. Group 1 = the automaton's name, group 2 =
/// the whole run of `[x][y]…` bracket groups (unsplit — see
/// [`RE_FOR_AN_AUTOMATON_INPUT_IN_JOIN_CMD`] for the per-label re-scan).
fn re_for_an_automaton_in_join_cmd() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        compile(&format!(
            r"{RE_WORD_OF_CMD_NO_SPC}(((?-u:\s)*\[(?-u:\s)*[a-zA-Z&&[^AE]](?-u:\w)*(?-u:\s)*\])+)"
        ))
    })
}

/// `Join.PAT_FOR_AN_AUTOMATON_INPUT_IN_join_CMD` (`:24-25`): `"\[\s*([a-zA-Z&&[^AE]]\w*)\s*]"`
/// — note the bare `]` Java allows but Rust requires escaped (`PORTING.md`'s dialect
/// note). Group 1 = one label.
fn re_for_an_automaton_input_in_join_cmd() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| compile(r"\[(?-u:\s)*([a-zA-Z&&[^AE]](?-u:\w)*)(?-u:\s)*\]"))
}

/// `Join.joinCommand(String s, String joinAutomata, String joinName)` (`:27-63`).
pub fn join_command(
    session: &Session,
    s: &str,
    join_automata: &str,
    join_name: &str,
) -> Result<TestCase, JoinError> {
    let mut subautomata: Vec<Automaton> = Vec::new();
    let mut is_dfao = false;

    let re1 = re_for_an_automaton_in_join_cmd();
    for caps in re1.captures_iter(Input::new(join_automata)) {
        let automaton_name = group(&caps, join_automata, 1).unwrap_or("").to_string();

        // `String addressForWordAutomaton = Session.getReadFileForWordsLibrary(
        //  automatonName + TXT_EXTENSION);` (`:33-34`) -- Java's read-family builder
        // has no session-file-precedence check here (`getReadFileForWordsLibrary`
        // DOES apply `globalOrSessionFile` internally, same as every other library
        // read), only a plain `isFile()` existence probe on the resolved address.
        let word_address = session
            .paths()
            .read_file_for_words_library(&format!("{automaton_name}{TXT_EXTENSION}"));
        let (m, from_word_library) = if Path::new(&word_address).is_file() {
            let m = session
                .libraries()
                .read_library_automaton(&word_address)
                .map_err(JoinError::Read)?;
            (m, true)
        } else {
            let address = session
                .paths()
                .read_file_for_automata_library(&format!("{automaton_name}{TXT_EXTENSION}"));
            let m = session
                .libraries()
                .read_library_automaton(&address)
                .map_err(JoinError::Read)?;
            (m, false)
        };
        if from_word_library {
            is_dfao = true;
        }
        let mut m = m;

        let automaton_inputs = group(&caps, join_automata, 2).unwrap_or("");
        let re2 = re_for_an_automaton_input_in_join_cmd();
        let mut label: Vec<String> = Vec::new();
        for icaps in re2.captures_iter(Input::new(automaton_inputs)) {
            if let Some(t) = group(&icaps, automaton_inputs, 1) {
                label.push(t.to_string());
            }
        }
        if label.len() != m.alphabet.len() {
            return Err(JoinError::LabelMismatch { automaton_name });
        }
        m.label = label;
        subautomata.push(m);
    }

    // `Automaton N = subautomata.remove(0);` (`:58`) -- see module docs, WB-037.
    if subautomata.is_empty() {
        return Err(JoinError::NoAutomataSpecified);
    }
    let mut iter = subautomata.into_iter();
    let first = iter.next().expect("checked non-empty above");
    let rest: Vec<Automaton> = iter.collect();

    // `N = join(N, new LinkedList<>(subautomata));` (`:59`).
    let mut joined = join(&first, rest);

    // `N.writeAutomata(s, ProverHelper.determineOutLibrary(isDFAO), joinName, isDFAO);`
    // (`:61`).
    let out_library = determine_out_library(session.paths(), is_dfao);
    write_automata(session, &mut joined, s, &out_library, join_name, is_dfao)?;

    Ok(TestCase::from_automaton(joined))
}

/// `Join.join(Automaton automaton, Queue<Automaton> subautomata)` (`:72-92`) — see
/// module docs for why this does NOT relabel its operands (unlike `combine`), and for
/// the `FIRST_OP`/`totalize` choices.
pub fn join(automaton: &Automaton, subautomata: Vec<Automaton>) -> Automaton {
    let mut first = automaton.clone();
    for mut next in subautomata {
        // `first.fa.totalize(); next.fa.totalize();` (`:82-83`).
        totalize(&mut first.fa);
        totalize(&mut next.fa);

        // `first = ProductStrategies.crossProduct(first, next, Prover.FIRST_OP);`
        // (`:84`) -- `determineOutput`'s `FIRST_OP` arm: `aP == 0 ? mQ : aP`.
        first = cross_product(&first, &next, |a, b| if a == 0 { b } else { a });

        // `first = WordAutomaton.minimizeWithOutput(first);` (`:85`).
        first = minimize_with_output(&first);
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-join-{tag}-{}-{}",
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

    fn word_automaton(output0: i32, output1: i32) -> Automaton {
        // 1-track msd_2 DFAO: q0 outputs `output0`, self-loops on 0, goes to q1
        // (outputting `output1`) on 1; q1 self-loops on both digits.
        let mut d = vec![BTreeMap::new(), BTreeMap::new()];
        d[0].insert(0, vec![0]);
        d[0].insert(1, vec![1]);
        d[1].insert(0, vec![1]);
        d[1].insert(1, vec![1]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![output0, output1],
                d,
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    // -- join (the primitive) -------------------------------------------------------

    #[test]
    fn join_first_non_zero_output_wins() {
        // `a` outputs 0 everywhere except state 1 (output 5); `b` outputs 3
        // everywhere. Joined: wherever `a` is 0, `b`'s 3 wins; wherever `a` is
        // nonzero (5), `a` wins.
        let mut a = word_automaton(0, 5);
        let mut b = word_automaton(3, 3);
        a.label = vec!["x".to_string()];
        b.label = vec!["x".to_string()];

        let joined = join(&a, vec![b]);

        // Word "" (state q0): a=0, b=3 -> 3.
        assert_eq!(joined.fa.o[joined.fa.q0], 3);
        // Word "1" (state q1 of both, since same transition shape): a=5 -> 5.
        let sym1 = joined.encode(&[1]);
        let next = joined.fa.d[joined.fa.q0][&sym1][0];
        assert_eq!(joined.fa.o[next], 5);
    }

    #[test]
    fn join_with_no_subautomata_is_a_clone() {
        let a = word_automaton(1, 2);
        let joined = join(&a, vec![]);
        assert_eq!(joined.fa.o, a.fa.o);
    }

    // -- join_command (end-to-end) ---------------------------------------------------

    #[test]
    fn join_command_reads_from_both_word_and_automata_libraries() {
        // Direct port of JoinTest.testJoinCommandReadsFromBothWordAndAutomataLibraries.
        let (session, dir) = temp_session("libs");

        let mut word_auto = word_automaton(5, 5);
        wr_io::writer::write_automaton_txt(
            &mut word_auto,
            dir.join("Word Automata Library").join("wordAuto.txt"),
        )
        .unwrap();

        let mut plain_auto = word_automaton(3, 3);
        wr_io::writer::write_automaton_txt(
            &mut plain_auto,
            dir.join("Automata Library").join("plainAuto.txt"),
        )
        .unwrap();

        let tc = join_command(
            &session,
            "join test wordAuto[x] plainAuto[x];",
            " wordAuto[x] plainAuto[x]",
            "joined",
        )
        .unwrap();

        assert_eq!(tc.automaton_pairs().len(), 1);
        assert!(tc.automaton_pairs()[0].automaton().is_some());
        // The output was written as a DFAO (isDFAO stays true once any sub-automaton
        // came from the word library), so it lands in the Word Automata Library.
        assert!(dir
            .join("Word Automata Library")
            .join("joined.txt")
            .is_file());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn join_command_rejects_a_label_count_mismatch() {
        // Direct port of JoinTest.testJoinCommandThrowsWhenLabelCountDoesNotMatchAlphabetSize.
        let (session, dir) = temp_session("label-mismatch");

        let mut unary_auto = word_automaton(5, 5);
        wr_io::writer::write_automaton_txt(
            &mut unary_auto,
            dir.join("Word Automata Library").join("unaryAuto.txt"),
        )
        .unwrap();

        let err = join_command(
            &session,
            "join test unaryAuto[x][y];",
            " unaryAuto[x][y]",
            "shouldNotBeWritten",
        )
        .unwrap_err();
        assert!(
            matches!(err, JoinError::LabelMismatch { ref automaton_name } if automaton_name == "unaryAuto")
        );
        assert_eq!(
            err.to_string(),
            "Number of inputs of word automata unaryAuto does not match number of \
             inputs specified."
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn join_command_wb037_zero_automata_is_rejected_not_a_panic() {
        let (session, dir) = temp_session("empty");
        let err = join_command(&session, "join J ;", "", "J").unwrap_err();
        assert!(matches!(err, JoinError::NoAutomataSpecified));
        fs::remove_dir_all(&dir).ok();
    }
}
