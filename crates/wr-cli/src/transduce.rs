// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Prover.transduceCommand(String)` (`Prover.java:693-704`) — the `transduce` command's
//! CLI-layer glue: read a `.txt` DFST from the Transducer Library, read the input word
//! automaton (DFAO/non-DFAO per the `$` sigil, same convention `alphabet`/`convert` use),
//! run it through the already-ported `wr_core::transducer::Transducer::
//! transduce_non_deterministic` (U20), and write the result into the Word Automata
//! Library — Java always writes a DFAO here (`writeAutomata(..., true)`, `:702`),
//! regardless of the input's own `$` flag.
//!
//! # The msd/non-deterministic "dispatch" is already inside `transduce_non_deterministic`
//!
//! This unit's own brief asks to "check `Prover.java` for how it decides between
//! `transduce_msd_deterministic` and `transduce_non_deterministic`" — it doesn't decide:
//! Java's `transduceCommand` unconditionally calls `T.transduceNonDeterministic(M)`
//! (`:701`), full stop. `transduceNonDeterministic` (already ported, U20) is what makes
//! the real msd/lsd-reversal and totalized-vs-partial-automaton dispatch, internally,
//! calling `transduceMsdDeterministic` itself once the input has been normalized. So this
//! module has nothing left to decide — it calls the one entry point Java's command layer
//! calls and lets `wr_core::transducer` do the rest.
//!
//! # Converting `TransducerData` -> `Transducer`
//!
//! `wr_io::reader::read_transducer_txt` (Phase 3a's U13) returns a plain data struct;
//! `wr_core::transducer`'s module doc spells out the exact field-mapping recipe this
//! module follows below (`build_transducer`) — `wr-core` cannot depend on `wr-io`, so the
//! conversion has to live on this side, exactly like `wr_core::morphism::Morphism::
//! from_mapping` already does for `ParseMethods.parseMorphism`.
//!
//! # The resource guard is in `wr-core`, not here
//!
//! `CLAUDE.md`'s "per-test resource caps, never hangs" guardrail applies to this command:
//! the transduction is exponential. It is **not** enforced at this layer, because nothing
//! at this layer can enforce it — the cost is exponential in the BFS *depth* inside
//! `wr_core::transducer`, not in either automaton's state count, so a precheck on the two
//! numbers available here (`M.fa.q` and the transducer's `fa.q`) is off by orders of
//! magnitude on the wrong axis, and a wall-clock watchdog is impossible because
//! `Automaton` is `!Send`. The cap therefore lives inside the primitive's own loops as
//! `wr_core::transducer::TransduceBudget`, and reaches this module as an ordinary
//! `TransduceError::Exploded` through [`TransduceCommandError::Transduce`]. See that
//! module's "Cost" docs for the numbers and for why the divergence from Java is
//! deliberate.

use wr_core::automaton::Automaton;
use wr_core::fa::Fa;
use wr_core::logging::Logging;
use wr_core::numsys::TXT_EXTENSION;
use wr_core::transducer::{TransduceError, Transducer};
use wr_io::reader::{read_transducer_txt, ReadError, TransducerData};

use crate::automaton_output::write_automata;
use crate::prover_helper::determine_in_library;
use crate::session::Session;
use crate::test_case::TestCase;
use wr_logic::predicate_env::PredicateEnvError;

/// Every failure `transduce` can produce.
#[derive(Debug)]
pub enum TransduceCommandError {
    /// Reading `Transducer Library/<name>.txt` failed
    /// (`wr_io::reader::read_transducer_txt`).
    ///
    /// Carries the address and is rendered exactly the way
    /// `crate::session::FileLibraries::read_library_automaton` renders the *input*
    /// automaton's read failures — Java raises the same two `WalnutException`s from the
    /// same `AutomatonReader` code for both files (`readTransducer` shares
    /// `readAutomaton`'s `catch (IOException) -> fileDoesNotExist` and its per-line
    /// parse errors), so the two paths must not print differently. See
    /// [`fmt::Display`](std::fmt::Display).
    ReadTransducer { address: String, source: ReadError },
    /// Reading the input automaton failed — the same classification every other
    /// file-reading command in this crate gets (`crate::session::FileLibraries::
    /// read_library_automaton`).
    ReadAutomaton(PredicateEnvError),
    /// `wr_core::transducer::TransduceError`, unchanged — including WB-034/WB-035's
    /// already-typed variants and the port-specific
    /// `TransduceError::Exploded` resource verdict (see the module docs).
    Transduce(TransduceError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for TransduceCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Deliberately identical in shape to `PredicateEnvError::FileDoesNotExist` /
            // `MalformedAutomaton`, which is what `read_library_automaton` produces for
            // the input-automaton half of this very command: an `Io` failure is Java's
            // `WalnutException.fileDoesNotExist` (`"File does not exist: " + address`),
            // everything else is a parse failure named by address. Printing the bare
            // `ReadError` instead would emit a raw Rust `Debug` dump with no filename in
            // it — worse than a rough Java match, and inconsistent with this crate's own
            // other file-reading paths.
            TransduceCommandError::ReadTransducer { address, source } => match source {
                ReadError::Io(_) => write!(f, "File does not exist: {address}"),
                other => write!(f, "File does not parse: {address} ({other})"),
            },
            TransduceCommandError::ReadAutomaton(e) => write!(f, "{e}"),
            TransduceCommandError::Transduce(e) => write!(f, "{e}"),
            TransduceCommandError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TransduceCommandError {}

impl From<PredicateEnvError> for TransduceCommandError {
    fn from(e: PredicateEnvError) -> Self {
        TransduceCommandError::ReadAutomaton(e)
    }
}

impl From<TransduceError> for TransduceCommandError {
    fn from(e: TransduceError) -> Self {
        TransduceCommandError::Transduce(e)
    }
}

impl From<std::io::Error> for TransduceCommandError {
    fn from(e: std::io::Error) -> Self {
        TransduceCommandError::Io(e)
    }
}

/// `wr_core::transducer`'s module doc, "Constructing one (the recipe U26's `transduce`
/// command needs)" — the mechanical `TransducerData` -> `Transducer` conversion, spelled
/// out there specifically so this unit didn't have to re-derive it:
///
/// ```text
/// Fa { q0: data.q0, q: data.q, alphabet_size: data.alphabet_size,
///      o: vec![0; data.q],           // readTransducer's discarded state outputs
///      d: data.d, true_false: None }
/// Transducer::new(Automaton::new(fa, data.alphabet, label, data.msd), data.sigma)
/// ```
///
/// `label` is only ever copied onto the *result* automaton, and only from the **input**
/// automaton `M` — never from the transducer — so any per-track placeholder matching
/// `readTransducer`'s own `"0"`, `"1"`, … will do; this uses the identical convention
/// `read_transducer_txt` itself already applies to its scratch automaton.
fn build_transducer(data: TransducerData) -> Transducer {
    let label: Vec<String> = (0..data.alphabet.len()).map(|i| i.to_string()).collect();
    let fa = Fa {
        q0: data.q0,
        q: data.q,
        alphabet_size: data.alphabet_size,
        o: vec![0; data.q],
        d: data.d,
        true_false: None,
    };
    let automaton = Automaton::new(fa, data.alphabet, label, data.msd);
    Transducer::new(automaton, data.sigma)
}

/// `Prover.transduceCommand(String)` (`:693-704`).
///
/// `transducer_name`/`in_name`/`new_name` are `PAT_FOR_transduce_CMD`'s
/// `GROUP_TRANSDUCE_TRANSDUCER`/`GROUP_TRANSDUCE_OLD_NAME`/`GROUP_TRANSDUCE_NEW_NAME`
/// capture groups; `is_dfao` is `!(m.group(GROUP_TRANSDUCE_DOLLAR_SIGN).equals("$"))`
/// (`:698`), computed by the caller exactly as `alphabet_command`'s own `is_dfao`
/// parameter already is.
pub fn transduce_command(
    session: &Session,
    s: &str,
    logging: &mut Logging,
    transducer_name: &str,
    is_dfao: bool,
    in_name: &str,
    new_name: &str,
) -> Result<TestCase, TransduceCommandError> {
    // `Transducer T = new Transducer(Session.getTransducerFile(m.group(
    // GROUP_TRANSDUCE_TRANSDUCER) + TXT_EXTENSION));` (`:696`).
    let transducer_path = session
        .paths()
        .transducer_file(&format!("{transducer_name}{TXT_EXTENSION}"));
    let data = read_transducer_txt(&transducer_path).map_err(|source| {
        TransduceCommandError::ReadTransducer {
            address: transducer_path.clone(),
            source,
        }
    })?;
    let transducer = build_transducer(data);

    // `String inFileName = m.group(GROUP_TRANSDUCE_OLD_NAME) + TXT_EXTENSION;
    //  boolean isDFAO = !(m.group(GROUP_TRANSDUCE_DOLLAR_SIGN).equals("$"));
    //  Automaton M = new Automaton(ProverHelper.determineInLibrary(isDFAO, inFileName));`
    // (`:697-699`).
    let in_file_name = format!("{in_name}{TXT_EXTENSION}");
    let in_address = determine_in_library(session.paths(), is_dfao, &in_file_name);
    let mut m = session.libraries().read_library_automaton(&in_address)?;

    // `Automaton C = T.transduceNonDeterministic(M);` (`:701`). The resource cap is
    // inside this call (`wr_core::transducer::TransduceBudget`) -- see the module docs.
    let mut c = transducer.transduce_non_deterministic(&mut m, logging)?;

    // `C.writeAutomata(s, Session.getWriteAddressForWordsLibrary(),
    // m.group(GROUP_TRANSDUCE_NEW_NAME), true);` (`:702`) -- `true` here (unlike
    // `reg`/`alphabet`, which always pass `false`): the result of a transduction is
    // always written as a DFAO, regardless of the *input*'s own `$` flag.
    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_words_library(),
        new_name,
        true,
    )?;

    // `return new TestCase(C);` (`:703`).
    Ok(TestCase::from_automaton(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-transduce-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
        for sub in [
            "Result",
            "Automata Library",
            "Word Automata Library",
            "Transducer Library",
            "Custom Bases",
            "Macro Library",
            "Morphism Library",
        ] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let dir_str = format!("{}/", dir.to_str().unwrap());
        (Session::new(Some(&dir_str), Some(&dir_str), false), dir)
    }

    /// `Transducer Library/RUNSUM2.txt`, transcribed verbatim from `walnut-java`'s copy
    /// (the running-sum-mod-2 transducer):
    ///
    /// ```text
    /// {0, 1}
    ///
    /// 0
    /// 0 -> 0 / 0
    /// 1 -> 1 / 1
    ///
    /// 1
    /// 0 -> 1 / 1
    /// 1 -> 0 / 0
    /// ```
    const RUNSUM2_TXT: &str = "{0, 1}\n\n0\n0 -> 0 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 1\n1 -> 0 / 0\n";

    /// `Word Automata Library/T.txt`, transcribed verbatim (the Thue-Morse sequence,
    /// `msd_2`):
    ///
    /// ```text
    /// # The Thue-Morse sequence.
    /// msd_2
    ///
    /// 0 0
    /// 0 -> 0
    /// 1 -> 1
    ///
    /// 1 1
    /// 0 -> 1
    /// 1 -> 0
    /// ```
    const THUE_MORSE_MSD_TXT: &str =
        "# The Thue-Morse sequence.\nmsd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n";

    /// `Word Automata Library/PR.txt`, transcribed verbatim (the regular paperfolding
    /// word, `lsd_2`):
    ///
    /// ```text
    /// # Paperfolding word, LSD representation
    /// lsd_2
    /// 0 0
    /// 0 -> 1
    /// 1 -> 0
    /// 1 0
    /// 0 -> 2
    /// 1 -> 3
    /// 2 0
    /// 0 -> 2
    /// 1 -> 2
    /// 3 1
    /// 0 -> 3
    /// 1 -> 3
    /// ```
    const PAPERFOLDING_LSD_TXT: &str = "# Paperfolding word, LSD representation\nlsd_2\n0 0\n0 -> 1\n1 -> 0\n1 0\n0 -> 2\n1 -> 3\n2 0\n0 -> 2\n1 -> 2\n3 1\n0 -> 3\n1 -> 3\n";

    fn write_fixture(dir: &std::path::Path, sub: &str, name: &str, content: &str) {
        fs::write(dir.join(sub).join(name), content).unwrap();
    }

    /// Walks a deterministic word automaton and returns the output at the state reached
    /// by `word`, msd-first.
    fn word_output(a: &Automaton, word: &[i32]) -> Option<i32> {
        let mut state = a.fa.q0;
        for sym in word {
            state = *a.fa.d[state].get(sym)?.first()?;
        }
        Some(a.fa.o[state])
    }

    #[test]
    fn transduce_runsum2_over_thue_morse_through_the_real_command_matches_the_known_output() {
        let (session, dir) = temp_session("runsum2-thue-morse");
        write_fixture(&dir, "Transducer Library", "RUNSUM2.txt", RUNSUM2_TXT);
        write_fixture(&dir, "Word Automata Library", "T.txt", THUE_MORSE_MSD_TXT);

        let tc = transduce_command(
            &session,
            "transduce test527 RUNSUM2 T",
            &mut Logging::new(),
            "RUNSUM2",
            true,
            "T",
            "test527",
        )
        .unwrap();

        let c = tc.automaton_pairs()[0].automaton().unwrap();

        // The transduced sequence is the running sum mod 2 of Thue-Morse:
        // t(n) = popcount(n) mod 2, running = sum_{k<=n} t(k) mod 2.
        let mut running = 0i32;
        for n in 0u32..32 {
            running = (running + (n.count_ones() % 2) as i32) % 2;
            let word: Vec<i32> = format!("{n:b}")
                .bytes()
                .map(|b| (b - b'0') as i32)
                .collect();
            assert_eq!(
                word_output(c, &word),
                Some(running),
                "running sum mod 2 at n = {n}"
            );
        }

        // The library copy and the Result/ files must exist, matching `writeAutomata`'s
        // effects (`is_dfao_for_gv = true` for `transduce`, unlike `reg`/`alphabet`).
        assert!(dir
            .join("Word Automata Library")
            .join("test527.txt")
            .is_file());
        assert!(dir.join("Result").join("test527.gv").is_file());

        fs::remove_dir_all(&dir).ok();
    }

    /// `transduce test529 RUNSUM2 PR;` (`IntegrationTest.java:674`) end-to-end — an
    /// `lsd_2` input, exercising `transduce_non_deterministic`'s "Automaton number system
    /// is lsd, reversing" branch through the real dispatch path.
    ///
    /// The assertion is the actual transduction semantics, derived from the fixture
    /// itself rather than from any hardcoded golden constant: RUNSUM2 is the running sum
    /// mod 2, so the result's output at `n` must equal `sum_{k<=n} PR(k) mod 2`, where
    /// `PR` is the very automaton this test wrote into the library. Both automata are
    /// read lsd-first (the reversal is undone on the way out, so the result is `lsd_2`
    /// again). This is the same shape the msd-side test above uses for its own semantic
    /// assertion.
    #[test]
    fn transduce_runsum2_over_lsd_paperfolding_computes_the_running_sum_of_the_fixture() {
        let (session, dir) = temp_session("runsum2-paperfolding-lsd");
        write_fixture(&dir, "Transducer Library", "RUNSUM2.txt", RUNSUM2_TXT);
        write_fixture(
            &dir,
            "Word Automata Library",
            "PR.txt",
            PAPERFOLDING_LSD_TXT,
        );

        // The oracle: the input automaton itself, read back the same way the command
        // reads it (so `word_output` walks a comparable, minimized copy).
        let pr = session
            .libraries()
            .read_library_automaton(
                dir.join("Word Automata Library")
                    .join("PR.txt")
                    .to_str()
                    .unwrap(),
            )
            .unwrap();

        let tc = transduce_command(
            &session,
            "transduce test529 RUNSUM2 PR",
            &mut Logging::new(),
            "RUNSUM2",
            true,
            "PR",
            "test529",
        )
        .unwrap();

        let c = tc.automaton_pairs()[0].automaton().unwrap();
        // Still an lsd_2 result (the reversal is undone on the way out).
        assert_eq!(c.msd, vec![Some(false)]);

        let mut running = 0i32;
        for n in 0u32..40 {
            // lsd representation: binary, least-significant digit first.
            let lsd_rep: Vec<i32> = format!("{n:b}")
                .bytes()
                .rev()
                .map(|b| (b - b'0') as i32)
                .collect();
            running = (running + word_output(&pr, &lsd_rep).expect("PR is total")) % 2;
            assert_eq!(
                word_output(c, &lsd_rep),
                Some(running),
                "running sum mod 2 of the paperfolding word at n = {n}"
            );
        }

        fs::remove_dir_all(&dir).ok();
    }

    /// The `$` sigil — the one branching decision this command layer makes. Java:
    /// `boolean isDFAO = !(m.group(GROUP_TRANSDUCE_DOLLAR_SIGN).equals("$"))` (`:698`),
    /// feeding `ProverHelper.determineInLibrary`, so `transduce N T $M;` reads `M` from
    /// the **`Automata Library/`** (a plain predicate automaton) rather than the
    /// `Word Automata Library/`.
    ///
    /// The asymmetry Java hardcodes is pinned here too: whatever the *input*'s `$` flag
    /// was, the *output* is written with `isDFAO = true` (`writeAutomata(..., true)`,
    /// `:702`), i.e. into the Word Automata Library — so this test's result lands beside
    /// word automata even though its input did not.
    #[test]
    fn the_dollar_sigil_reads_a_plain_automaton_but_the_result_is_still_written_as_a_dfao() {
        let (session, dir) = temp_session("dollar");
        write_fixture(&dir, "Transducer Library", "RUNSUM2.txt", RUNSUM2_TXT);
        // A plain (non-DFAO) automaton, in `Automata Library/`: msd_2, accepting the
        // words with an even number of 1s. Its `.txt` shape is identical to a DFAO's --
        // Walnut's format does not distinguish them; only the *directory* does, which is
        // exactly what `$` selects.
        write_fixture(
            &dir,
            "Automata Library",
            "EVEN.txt",
            "msd_2\n\n0 1\n0 -> 0\n1 -> 1\n\n1 0\n0 -> 1\n1 -> 0\n",
        );
        // A same-named decoy in the Word Automata Library, so a test that accidentally
        // read the DFAO path would produce a different answer rather than silently pass.
        write_fixture(
            &dir,
            "Word Automata Library",
            "EVEN.txt",
            THUE_MORSE_MSD_TXT,
        );

        let tc = transduce_command(
            &session,
            "transduce outd RUNSUM2 $EVEN",
            &mut Logging::new(),
            "RUNSUM2",
            false, // `is_dfao == false` <=> the `$` was present
            "EVEN",
            "outd",
        )
        .unwrap();

        // Read from `Automata Library/EVEN.txt`, not the decoy: the running sum mod 2 of
        // [n has an even number of 1s], not of Thue-Morse (which is its complement).
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        let mut running = 0i32;
        for n in 0u32..32 {
            running = (running + i32::from(n.count_ones() % 2 == 0)) % 2;
            let word: Vec<i32> = format!("{n:b}")
                .bytes()
                .map(|b| (b - b'0') as i32)
                .collect();
            assert_eq!(word_output(c, &word), Some(running), "at n = {n}");
        }

        // ... and the OUTPUT went to the Word Automata Library regardless.
        assert!(dir.join("Word Automata Library").join("outd.txt").is_file());
        assert!(
            !dir.join("Automata Library").join("outd.txt").is_file(),
            "Java writes the transduction result as a DFAO even for a `$` input"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// A **partial transducer**: `Transducer Library/PARTIAL.txt` declares `{0, 1}` but
    /// state `1` has no transition on letter `1`. The file is perfectly well-formed --
    /// `read_transducer_txt` accepts it, and `transduce_non_deterministic`'s only
    /// compatibility guard looks at the transducer's state `0` (WB-035), which is total
    /// here -- so nothing rejects it before the BFS reaches the hole.
    ///
    /// Real Walnut throws `NullPointerException` there (`Transducer.java:400`), which
    /// `Prover.dispatch`'s `catch (RuntimeException)` prints before returning to the
    /// prompt. This is the shape U26 makes reachable from an ordinary user-supplied
    /// `.txt`, and `wr-cli` has no `catch_unwind` anywhere in `read_buffer`/`dispatch`, so
    /// a Rust `panic!` would kill the whole session: it must be a clean error.
    #[test]
    fn a_partial_transducer_file_is_a_clean_error_not_a_process_killing_panic() {
        let (session, dir) = temp_session("partial-transducer");
        write_fixture(
            &dir,
            "Transducer Library",
            "PARTIAL.txt",
            // State 0 total; state 1 missing its `1` transition.
            "{0, 1}\n\n0\n0 -> 0 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 1\n",
        );
        write_fixture(&dir, "Word Automata Library", "T.txt", THUE_MORSE_MSD_TXT);

        let err = transduce_command(
            &session,
            "transduce out PARTIAL T",
            &mut Logging::new(),
            "PARTIAL",
            true,
            "T",
            "out",
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                TransduceCommandError::Transduce(TransduceError::NoTransducerTransition)
            ),
            "expected a clean NoTransducerTransition, got {err:?}"
        );
        // Java's own NPE text, so REPL output still matches.
        assert!(err.to_string().contains("getNfaStateDests"), "{err}");

        fs::remove_dir_all(&dir).ok();
    }

    /// The `wr-core` resource budget surfaces through this command as an ordinary error,
    /// with a user-facing message that does not point at Rust internals.
    ///
    /// The *triggering* of the budget is covered where it lives
    /// (`wr_core::transducer`'s `a_tiny_but_exponential_input_is_rejected_...`); running
    /// a genuinely exponential input through the default budget here would cost seconds
    /// per test run for no extra signal, since this layer only propagates with `?`.
    #[test]
    fn a_budget_exhaustion_verdict_propagates_with_a_user_facing_message() {
        let err = TransduceCommandError::Transduce(TransduceError::Exploded(
            wr_core::transducer::TransduceLimit::MapSteps,
        ));
        let text = err.to_string();
        assert!(text.contains("resource budget"), "{text}");
        assert!(
            !text.contains("::") && !text.contains("docs"),
            "user-facing text must not cite Rust modules or doc comments: {text}"
        );
    }

    #[test]
    fn wb034_a_track_with_no_number_system_is_rejected_through_the_real_command() {
        let (session, dir) = temp_session("wb034");
        write_fixture(&dir, "Transducer Library", "RUNSUM2.txt", RUNSUM2_TXT);
        // The same shape as `T.txt` but with an explicit-set header instead of `msd_2`,
        // so the reader leaves `msd[0] == None` (WB-034's trigger).
        write_fixture(
            &dir,
            "Word Automata Library",
            "TSET.txt",
            "{0, 1}\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n",
        );

        let err = transduce_command(
            &session,
            "transduce out RUNSUM2 TSET",
            &mut Logging::new(),
            "RUNSUM2",
            true,
            "TSET",
            "out",
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TransduceCommandError::Transduce(TransduceError::NoNumberSystem)
        ));

        fs::remove_dir_all(&dir).ok();
    }

    /// A missing transducer file must print Java's `WalnutException.fileDoesNotExist`
    /// text (`"File does not exist: " + address`), naming the file — the same message
    /// `Session::read_library_automaton` already produces for the *input* automaton half
    /// of this same command. Asserting the message STRING, not just the variant: the
    /// variant matched fine while the rendering was a raw Rust `Debug` dump
    /// (`Io(Os { code: 2, ... })`) with no filename in it at all.
    #[test]
    fn a_missing_transducer_file_names_the_file_the_way_every_other_read_path_does() {
        let (session, dir) = temp_session("missing-transducer");
        write_fixture(&dir, "Word Automata Library", "T.txt", THUE_MORSE_MSD_TXT);

        let err = transduce_command(
            &session,
            "transduce out NOPE T",
            &mut Logging::new(),
            "NOPE",
            true,
            "T",
            "out",
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TransduceCommandError::ReadTransducer {
                source: ReadError::Io(_),
                ..
            }
        ));
        let expected = format!(
            "File does not exist: {}",
            dir.join("Transducer Library").join("NOPE.txt").display()
        );
        assert_eq!(err.to_string(), expected);

        fs::remove_dir_all(&dir).ok();
    }

    /// The malformed half of the same convention: the file exists but does not parse, so
    /// the message is the `MalformedAutomaton`-shaped one, again naming the file. (This
    /// crate's `ReadError` detail text is still its `Debug` form — that gap is
    /// `PredicateEnvError::MalformedAutomaton`'s own documented, pre-existing one, shared
    /// verbatim by both read paths; what this test pins is that the *transducer* path is
    /// no longer worse than the automaton path.)
    #[test]
    fn a_malformed_transducer_file_names_the_file_the_way_every_other_read_path_does() {
        let (session, dir) = temp_session("malformed-transducer");
        write_fixture(&dir, "Word Automata Library", "T.txt", THUE_MORSE_MSD_TXT);
        write_fixture(
            &dir,
            "Transducer Library",
            "EMPTY.txt",
            "# only a comment\n",
        );

        let err = transduce_command(
            &session,
            "transduce out EMPTY T",
            &mut Logging::new(),
            "EMPTY",
            true,
            "T",
            "out",
        )
        .unwrap_err();
        let address = dir.join("Transducer Library").join("EMPTY.txt");
        let text = err.to_string();
        assert!(
            text.starts_with(&format!("File does not parse: {}", address.display())),
            "{text}"
        );
        assert!(
            !text.starts_with("Io(") && !text.starts_with("EmptyFile"),
            "must not be a bare Debug dump of the reader error: {text}"
        );

        fs::remove_dir_all(&dir).ok();
    }
}
