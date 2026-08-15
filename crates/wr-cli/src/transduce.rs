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
//! # The resource guard
//!
//! See [`check_transduce_size_guard`]'s own doc for exactly what it does, and — just as
//! importantly — does not, cover.

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

/// The largest input-automaton state count [`check_transduce_size_guard`] allows through.
/// Chosen generously above every shipped library fixture (the largest word automata in
/// `walnut-java`'s `Word Automata Library/` are tens of states, not hundreds) so it never
/// fires on ordinary usage, while still rejecting an obviously oversized input outright
/// rather than letting it reach the uncapped BFS.
pub const MAX_TRANSDUCE_INPUT_STATES: usize = 500;

/// As [`MAX_TRANSDUCE_INPUT_STATES`], for the transducer's own state count.
pub const MAX_TRANSDUCE_TRANSDUCER_STATES: usize = 500;

/// Every failure `transduce` can produce.
#[derive(Debug)]
pub enum TransduceCommandError {
    /// Reading `Transducer Library/<name>.txt` failed (`wr_io::reader::read_transducer_txt`).
    ReadTransducer(ReadError),
    /// Reading the input automaton failed — the same classification every other
    /// file-reading command in this crate gets (`crate::session::FileLibraries::
    /// read_library_automaton`).
    ReadAutomaton(PredicateEnvError),
    /// [`check_transduce_size_guard`] tripped before the transduction ever ran.
    TooBig {
        input_states: usize,
        transducer_states: usize,
    },
    /// `wr_core::transducer::TransduceError`, unchanged — including WB-034/WB-035's
    /// already-typed variants.
    Transduce(TransduceError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for TransduceCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransduceCommandError::ReadTransducer(e) => write!(f, "{e}"),
            TransduceCommandError::ReadAutomaton(e) => write!(f, "{e}"),
            TransduceCommandError::TooBig {
                input_states,
                transducer_states,
            } => write!(
                f,
                "transduce: input too large for the resource guard ({input_states} \
                 automaton states, {transducer_states} transducer states; limits are \
                 {MAX_TRANSDUCE_INPUT_STATES}/{MAX_TRANSDUCE_TRANSDUCER_STATES}) — \
                 see wr-core::transducer's module docs on why this is uncapped internally"
            ),
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

/// A size PREFLIGHT, not a true mid-execution kill-switch.
///
/// `CLAUDE.md`'s "per-test resource caps, never hangs" guardrail, and
/// `wr_core::transducer`'s own module doc ("Cost"), both ask for a wall-time/peak-state
/// guard around the call this unit wires up — the BFS itself stays uncapped, faithfully,
/// inside `wr-core`.
///
/// A *true* preemptive guard (kill the computation if it overruns a wall-clock deadline)
/// would need to run the call on a watchdog thread and abandon it if the deadline passes.
/// That is not achievable here without either modifying `wr_core::transducer`'s BFS to
/// accept a cancellation/step-budget hook (explicitly out of this unit's scope — the
/// primitive "stays uncapped per its own docs") or `unsafe impl Send`: `Automaton` (and
/// therefore `Transducer`, which wraps one) is unconditionally `!Send` — it carries
/// `all_reps: Vec<Option<Rc<Automaton>>>` — and `Rc`'s refcount is not atomic, so moving
/// one across a thread boundary is only sound if no other clone of the same `Rc` can be
/// touched concurrently from the thread it came from; this crate has no way to prove that
/// locally (an automaton's `all_reps` entries can, in general, alias a `NumberSystem`
/// still cached in this very `Session`). Scoped threads (`std::thread::scope`) don't
/// avoid this either: `SessionPaths` holds a `RefCell` (for the "Overriding global file
/// with session file" console notice), making it `!Sync`, so not even a *borrowed*
/// reference to it may cross into a scoped thread — and scoped threads block until the
/// spawned work finishes regardless, which defeats returning control to the caller on a
/// timeout anyway. Given all of that, this crate does not introduce `unsafe` code (it has
/// none today) to chase a guard that the type system this deep would make unsound in the
/// general case.
///
/// So instead: reject up front on the two numbers that are always cheaply available
/// *before* the BFS runs — `M`'s state count and the transducer's state count — past a
/// generous threshold (`MAX_TRANSDUCE_INPUT_STATES`/`MAX_TRANSDUCE_TRANSDUCER_STATES`).
/// This reliably catches "someone fed a genuinely huge automaton/transducer" without ever
/// touching the exponential search. It does **not** catch the module doc's other named
/// hazard — a transducer with only a *few* states but a rich enough transition monoid
/// that the periodicity search's lag/period still blows up — since noticing that requires
/// actually running the search. Documented honestly rather than overclaimed, matching
/// this crate's own precedent (`wr_core::equiv`'s U8 doc note on its own coverage gap).
fn check_transduce_size_guard(
    m: &Automaton,
    transducer: &Transducer,
) -> Result<(), TransduceCommandError> {
    let input_states = m.fa.q;
    let transducer_states = transducer.automaton.fa.q;
    if input_states > MAX_TRANSDUCE_INPUT_STATES
        || transducer_states > MAX_TRANSDUCE_TRANSDUCER_STATES
    {
        return Err(TransduceCommandError::TooBig {
            input_states,
            transducer_states,
        });
    }
    Ok(())
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
    let data =
        read_transducer_txt(&transducer_path).map_err(TransduceCommandError::ReadTransducer)?;
    let transducer = build_transducer(data);

    // `String inFileName = m.group(GROUP_TRANSDUCE_OLD_NAME) + TXT_EXTENSION;
    //  boolean isDFAO = !(m.group(GROUP_TRANSDUCE_DOLLAR_SIGN).equals("$"));
    //  Automaton M = new Automaton(ProverHelper.determineInLibrary(isDFAO, inFileName));`
    // (`:697-699`).
    let in_file_name = format!("{in_name}{TXT_EXTENSION}");
    let in_address = determine_in_library(session.paths(), is_dfao, &in_file_name);
    let mut m = session.libraries().read_library_automaton(&in_address)?;

    // Not a Java line -- the resource guard this unit adds. See its own doc for scope.
    check_transduce_size_guard(&m, &transducer)?;

    // `Automaton C = T.transduceNonDeterministic(M);` (`:701`).
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
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::fa::Fa;

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

    #[test]
    fn transduce_runsum2_over_lsd_paperfolding_reverses_and_back_through_the_real_command() {
        let (session, dir) = temp_session("runsum2-paperfolding-lsd");
        write_fixture(&dir, "Transducer Library", "RUNSUM2.txt", RUNSUM2_TXT);
        write_fixture(
            &dir,
            "Word Automata Library",
            "PR.txt",
            PAPERFOLDING_LSD_TXT,
        );

        // `transduce test529 RUNSUM2 PR;` -- an lsd_2 input, exercising
        // `transduce_non_deterministic`'s "Automaton number system is lsd, reversing"
        // branch through the real dispatch path (not calling `wr_core::transducer`
        // directly).
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
        // Sanity: the result is a genuine (non-trivial) automaton, not empty/degenerate.
        assert!(c.fa.q > 0);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_size_guard_does_not_fire_on_the_ordinary_runsum2_thue_morse_case() {
        let (session, dir) = temp_session("guard-normal");
        write_fixture(&dir, "Transducer Library", "RUNSUM2.txt", RUNSUM2_TXT);
        write_fixture(&dir, "Word Automata Library", "T.txt", THUE_MORSE_MSD_TXT);

        assert!(transduce_command(
            &session,
            "transduce out RUNSUM2 T",
            &mut Logging::new(),
            "RUNSUM2",
            true,
            "T",
            "out",
        )
        .is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    /// A hand-built, deliberately oversized (but otherwise trivial -- no exponential cost
    /// to even construct) input automaton, well past [`MAX_TRANSDUCE_INPUT_STATES`],
    /// confirms the guard trips with [`TransduceCommandError::TooBig`] rather than
    /// letting an enormous automaton reach the uncapped BFS.
    #[test]
    fn the_size_guard_rejects_an_oversized_input_automaton_before_transducing() {
        let (session, dir) = temp_session("guard-toobig");
        write_fixture(&dir, "Transducer Library", "RUNSUM2.txt", RUNSUM2_TXT);

        // A chain 0 -> 1 -> ... -> n-1 (capped) on symbol 1, self-looping on symbol 0,
        // with every state given a DISTINCT output. `read_library_automaton` (like
        // Java's `readAutomaton`) auto-determinizes + minimizes on load
        // (`AutomatonReader.readAutomaton`), so a construction whose states are all
        // Myhill-Nerode equivalent (e.g. all self-loops, all output 0) would collapse to
        // one state before this guard ever saw it -- distinct per-state outputs make
        // every state trivially distinguishable (differing on the empty suffix), so all
        // `n` states must survive minimization intact.
        let n = MAX_TRANSDUCE_INPUT_STATES + 1;
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(n);
        for q in 0..n {
            let mut row = BTreeMap::new();
            row.insert(0, vec![q]);
            row.insert(1, vec![(q + 1).min(n - 1)]);
            d.push(row);
        }
        let fa = Fa {
            q0: 0,
            q: n,
            alphabet_size: 2,
            o: (0..n as i32).collect(),
            d,
            true_false: None,
        };
        let mut big = Automaton::new(
            fa,
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        wr_io::writer::write_automaton_txt(
            &mut big,
            dir.join("Word Automata Library").join("BIG.txt"),
        )
        .unwrap();

        let err = transduce_command(
            &session,
            "transduce out RUNSUM2 BIG",
            &mut Logging::new(),
            "RUNSUM2",
            true,
            "BIG",
            "out",
        )
        .unwrap_err();
        assert!(
            matches!(err, TransduceCommandError::TooBig { input_states, .. } if input_states == n)
        );

        fs::remove_dir_all(&dir).ok();
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

    #[test]
    fn a_missing_transducer_file_is_reported_not_a_panic() {
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
        assert!(matches!(err, TransduceCommandError::ReadTransducer(_)));

        fs::remove_dir_all(&dir).ok();
    }
}
