// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `split` / `rsplit` — `Main/Commands/Split.java` (123 LOC), Layer B of
//! `docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`.
//!
//! Both commands reinterpret an automaton's inputs across the base-`k` / base-`(-k)`
//! boundary; `isReverse` is the only difference between them, which is why Java gives them
//! one handler and so does this.
//!
//! # Why this is a `wr-cli` module and not a `wr-core` one
//!
//! `processSplit` is composition, not new algorithm: every primitive it uses already
//! exists in `wr-core` ([`wr_core::word_automaton::uncombine`],
//! [`wr_core::logicalops::combine`] / [`wr_core::logicalops::and`],
//! [`wr_core::quantify::quantify`], `Automaton::{bind, sort_label, random_label}`). What
//! it *adds* is two things `wr-core` cannot do on its own:
//!
//! * **Resolving a number system by NAME.** Java reads `automaton.getNS().get(i)`, an
//!   actual `NumberSystem` object carried on the automaton. This crate's `Automaton`
//!   carries only `msd: Vec<Option<bool>>` + `ns_name: Vec<Option<String>>` (see that
//!   type's docs for why), so the object has to be re-resolved — through the session's
//!   [`wr_logic::predicate_env::PredicateEnv`], which is also what makes a custom base
//!   like `msd_neg_fib` work here. [`Automaton::track_ns_names`] is the faithful stand-in
//!   for `getNS()`: its `None` entries are exactly Java's `null` ones, which is what
//!   `Split.java:96-97`'s "Number system for input i must be defined." tests.
//! * **File I/O for the base-change automaton.** `setBaseChangeAutomaton` probes
//!   `Custom Bases/*_base_change.txt`, and `wr-core` performs no file I/O — so the probe
//!   lives here and hands [`NumberSystem::set_base_change_automaton`] its result, exactly
//!   as [`crate::session`] already does for the adder / comparator / all-representations
//!   trio.
//!
//! # The three `reverse` ternaries
//!
//! `processSplit`'s two arms contain three independent `reverse ?` choices with different
//! polarities, and they are the easiest thing in this file to transcribe wrongly
//! (`Split.java:104`, `:108`, `:112`):
//!
//! | arm | base-change binding | extra conjunct |
//! |-----|---------------------|----------------|
//! | `PLUS` | `bind(reverse ? [b, a] : [a, b])` | — |
//! | `MINUS` | `bind([reverse ? b : a, c])` | `arithmetic(reverse ? a : b, c, 0, PLUS)` |
//!
//! The `MINUS` arm's `arithmetic` call is the constant-RESULT overload with `0`/`PLUS`,
//! evaluated in the **negative** number system — i.e. "`c` is the additive inverse of the
//! other name". That is where Layer A's restored negative-base arithmetic is actually
//! load-bearing for this command: in a positive base the same call would only ever be
//! satisfiable by `0`.
//!
//! The quantifier set accumulates across the whole loop (`b` in the `PLUS` arm, both `b`
//! and `c` in the `MINUS` arm) and `quantify` runs ONCE at the end, not per track.
//!
//! # Java's own `getNS()` aliasing, and why this port re-resolves instead
//!
//! In Java, `ns.determineNegativeNS()` returns `this` when `ns` is already negative, so a
//! second `split` on the same automaton reuses the same cached `baseChange`. When `ns` is
//! positive it constructs a brand-new `NumberSystem` every call and throws it away — no
//! caching at all. This port sits between the two: it resolves the negative system's NAME
//! through the session's memoized [`wr_logic::predicate_env::PredicateEnv`] and then
//! builds the base-change automaton into a local clone, memoized per name for the duration
//! of one `processSplit` call. Same automaton either way — `set_base_change_automaton` is a
//! pure function of the name plus the `Custom Bases/` files — so the difference is only how
//! often it is rebuilt.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

use wr_core::automaton::Automaton;
use wr_core::logging::Logging;
use wr_core::logicalops::{and, combine};
use wr_core::numsys::{
    self, ArithmeticOp, CustomBaseCandidates, NumSysError, NumberSystem, TXT_EXTENSION,
};
use wr_core::quantify::{quantify_with_ctx, QuantifyError};
use wr_core::util::remove_duplicates;
use wr_core::word_automaton::uncombine;

use crate::prover_helper::determine_out_library;
use crate::session::Session;
use crate::test_case::TestCase;
use wr_logic::predicate_env::{PredicateEnv, PredicateEnvError};

/// Every failure `split`/`rsplit` can produce.
#[derive(Debug)]
pub enum SplitError {
    /// `new Automaton(address)` failed, or a number system named on one of the operand's
    /// tracks could not be resolved through the session.
    Read(PredicateEnvError),
    /// A `WalnutException` whose message `Split.java` constructs directly, preserved
    /// verbatim.
    Walnut(String),
    /// Propagated from `wr-core`: `NumberSystem` construction, `set_base_change_automaton`,
    /// `arithmetic`, or `quantify`.
    Core(String),
    /// See `crate::automaton_output::write_automata`'s docs for why a write failure
    /// propagates here rather than being swallowed-and-logged the way Java's
    /// `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for SplitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitError::Read(e) => write!(f, "{e}"),
            SplitError::Walnut(m) | SplitError::Core(m) => f.write_str(m),
            SplitError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SplitError {}

impl From<std::io::Error> for SplitError {
    fn from(e: std::io::Error) -> Self {
        SplitError::Io(e)
    }
}

impl From<NumSysError> for SplitError {
    fn from(e: NumSysError) -> Self {
        SplitError::Core(e.to_string())
    }
}

/// `ArithmeticOperator.Ops.fromSymbol(String)` (`ArithmeticOperator.java:64-71`), which
/// throws `IllegalArgumentException` on an unknown symbol.
///
/// `Split.processSplitCommand` (`:40`) calls it on each captured bracket group, so this
/// port needs the full table even though `Prover`'s `RE_FOR_INPUT_IN_split_CMD` can only
/// ever hand it `""`, `"+"` or `"-"`. The `None` return plays the `IllegalArgumentException`
/// rather than a `null`: Java's `null` case is the EMPTY string, checked before this is
/// called.
fn op_from_symbol(symbol: &str) -> Option<ArithmeticOp> {
    [
        ArithmeticOp::Plus,
        ArithmeticOp::Minus,
        ArithmeticOp::Div,
        ArithmeticOp::Mult,
        ArithmeticOp::UnaryNegative,
    ]
    .into_iter()
    .find(|op| op.symbol() == symbol)
}

/// `Split.processSplitCommand(String s, boolean isReverse, String automatonName, String
/// name, Matcher inputPattern)` (`:14-62`).
///
/// `input_group` is the raw text `Prover` captured for the whole bracket list (Java hands
/// over a `Matcher` already positioned on it); this function runs the same
/// `RE_FOR_INPUT_IN_split_CMD` over it, so the `find()`-loop semantics are Java's.
pub fn process_split_command(
    session: &Session,
    logging: &mut Logging,
    s: &str,
    is_reverse: bool,
    automaton_name: &str,
    name: &str,
    input_group: &str,
) -> Result<TestCase, SplitError> {
    // `Session.getReadFileForWordsLibrary(automatonName + TXT_EXTENSION)` (`:17-24`): the
    // WORD library first; only if that file is absent is the automata library consulted,
    // and which one hit decides `isDFAO` (and so where the result is written).
    let words_address = session
        .paths()
        .read_file_for_words_library(&format!("{automaton_name}{TXT_EXTENSION}"));
    let (m, is_dfao) = if Path::new(&words_address).is_file() {
        (read(session, &words_address)?, true)
    } else {
        let automata_address = session
            .paths()
            .read_file_for_automata_library(&format!("{automaton_name}{TXT_EXTENSION}"));
        if Path::new(&automata_address).is_file() {
            (read(session, &automata_address)?, false)
        } else {
            // `throw new WalnutException("Automaton " + automatonName + " does not
            // exist.")` (`:32`).
            return Err(SplitError::Walnut(format!(
                "Automaton {automaton_name} does not exist."
            )));
        }
    };

    // `while (inputPattern.find())` (`:38-46`).
    let mut plus_minus_inputs: Vec<Option<ArithmeticOp>> = Vec::new();
    let mut has_input = false;
    for caps in crate::prover::patterns()
        .input_in_split
        .captures_iter(input_group)
    {
        let t = caps.get_group(1).map_or("", |span| &input_group[span]);
        // `t.isEmpty() ? null : Ops.fromSymbol(t)` (`:40`).
        let t_op = if t.is_empty() {
            None
        } else {
            match op_from_symbol(t) {
                Some(op) => Some(op),
                // `fromSymbol`'s own `IllegalArgumentException`. Unreachable through
                // `Prover`'s dispatch (its regex captures only `""`/`"+"`/`"-"`), ported
                // for completeness.
                None => {
                    return Err(SplitError::Core(format!(
                        "Unknown arithmetic operator: {t}"
                    )))
                }
            }
        };
        // `if (tOp != null && tOp != PLUS && tOp != MINUS) throw invalidCommand(t)`
        // (`:41-43`).
        if let Some(op) = t_op {
            if op != ArithmeticOp::Plus && op != ArithmeticOp::Minus {
                return Err(SplitError::Walnut(format!("Invalid command: {t}")));
            }
        }
        has_input = has_input || t_op.is_some();
        plus_minus_inputs.push(t_op);
    }
    // `if (!hasInput || plusMinusInputs.isEmpty())` (`:47-49`). The second disjunct is
    // unreachable on its own (an empty list cannot have set `hasInput`), ported as Java
    // wrote it.
    if !has_input || plus_minus_inputs.is_empty() {
        return Err(SplitError::Walnut(
            "Cannot split without inputs.".to_string(),
        ));
    }

    // `IntList outputs = new IntArrayList(M.fa.getO()); removeDuplicates(outputs);`
    // (`:51-52`) -- FIRST-occurrence order, which `combine` below relies on.
    let mut outputs: Vec<i32> = m.fa.o.clone();
    remove_duplicates(&mut outputs);

    // `WordAutomaton.uncombine(M, outputs)` (`:53`), then `replaceAll(processSplit)`
    // (`:55`).
    let mut subautomata = uncombine(&m, &outputs);
    let mut split_subautomata = Vec::with_capacity(subautomata.len());
    for a in subautomata.drain(..) {
        split_subautomata.push(process_split(
            session,
            logging,
            &a,
            &plus_minus_inputs,
            is_reverse,
        )?);
    }

    // `Automaton N = subautomata.remove(0); N = combine(N, rest, outputs);` (`:57-58`).
    let first = split_subautomata.remove(0);
    let mut n = combine(&first, split_subautomata, &outputs, logging);

    // `N.writeAutomata(s, ProverHelper.determineOutLibrary(isDFAO), name, isDFAO);`
    // (`:60`).
    crate::automaton_output::write_automata(
        session,
        &mut n,
        s,
        &determine_out_library(session.paths(), is_dfao),
        name,
        is_dfao,
    )?;
    Ok(TestCase::from_automaton(n))
}

/// `QuantifyError` rendered the way Walnut renders it, not via `Debug`.
///
/// `wr_logic::token` already implements this text for the same variant; duplicating the
/// one reachable arm here (rather than exporting it) keeps `wr-cli` from depending on
/// `wr-logic`'s error-rendering internals for a case that is, in this command,
/// unreachable by construction: every `b_i`/`c_i` in `quantifiers` is a track of `m` —
/// `m` starts labelled `b0..bk` and each `and` unions in the `a_i`/`c_i` the same
/// iteration just bound.
fn quantify_error_message(e: &QuantifyError) -> String {
    match e {
        QuantifyError::NotFreeVariable(name) => {
            format!("Variable {name} in the list of quantified variables is not a free variable.")
        }
        other => format!("{other:?}"),
    }
}

fn read(session: &Session, address: &str) -> Result<Automaton, SplitError> {
    session
        .libraries()
        .read_library_automaton(address)
        .map_err(SplitError::Read)
}

/// `Split.processSplit(Automaton automaton, List<ArithmeticOperator.Ops> inputs, boolean
/// reverse)` (`:72-122`).
///
/// See this module's docs for the three `reverse` ternaries and for why the number system
/// is re-resolved by name here rather than read off the automaton.
pub fn process_split(
    session: &Session,
    logging: &mut Logging,
    automaton: &Automaton,
    inputs: &[Option<ArithmeticOp>],
    reverse: bool,
) -> Result<Automaton, SplitError> {
    // `if (automaton.getAlphabetSize() == 0)` (`:73-75`).
    if automaton.fa.alphabet_size == 0 {
        return Err(SplitError::Walnut(
            "Cannot process split automaton with no inputs.".to_string(),
        ));
    }
    // `if (inputs.size() != automaton.richAlphabet.getA().size())` (`:76-78`).
    if inputs.len() != automaton.alphabet.len() {
        return Err(SplitError::Walnut(
            "Split automaton has incorrect number of inputs.".to_string(),
        ));
    }

    let mut m = automaton.clone();
    let mut quantifiers: BTreeSet<String> = BTreeSet::new();
    // `M.setLabel(names)` with `names = [b0, b1, …]` (`:82-87`) -- a PLAIN field
    // assignment in Java, not `bind()`: no `removeSameInputs`, no `labelSorted`/`canonized`
    // reset. The names are distinct by construction, so the only observable difference
    // would be those two flags; assigning directly keeps the port faithful anyway.
    m.label = (0..automaton.alphabet.len())
        .map(|i| format!("b{i}"))
        .collect();

    // Java's `getNS().get(i)` stand-in -- see this module's docs.
    let ns_names = automaton.track_ns_names();
    // Per-name memo for the negative system's base-change automaton (this module's docs).
    let mut base_changes: BTreeMap<String, Automaton> = BTreeMap::new();

    for (i, input) in inputs.iter().enumerate() {
        // `if (input == null) continue;` (`:92-94`) -- the `[]` slot.
        let Some(input) = *input else { continue };
        // `NumberSystem ns = automaton.getNS().get(i); if (ns == null) throw …` (`:95-97`).
        let Some(ns_name) = ns_names[i].as_deref() else {
            return Err(SplitError::Walnut(format!(
                "Number system for input {i} must be defined."
            )));
        };
        // `NumberSystem negativeNumberSystem = ns.determineNegativeNS();` (`:98`) --
        // `negative_ns_name` plus `set_base_change_automaton`, which together are Java's
        // `:219-229`.
        let negative_name = numsys::negative_ns_name(ns_name)?;
        let base_change = match base_changes.get(&negative_name) {
            Some(bc) => bc.clone(),
            None => {
                let bc = build_base_change(session, logging, &negative_name)?;
                base_changes.insert(negative_name.clone(), bc.clone());
                bc
            }
        };

        // `Automaton baseChange = negativeNumberSystem.baseChange.clone();` (`:100`).
        let mut base_change = base_change;
        let (a, b, c) = (format!("a{i}"), format!("b{i}"), format!("c{i}"));

        if input == ArithmeticOp::Plus {
            // `baseChange.bind(reverse ? List.of(b, a) : List.of(a, b));` (`:104`).
            base_change.bind(if reverse {
                vec![b.clone(), a.clone()]
            } else {
                vec![a.clone(), b.clone()]
            });
            m = and(&m, &base_change, logging).into_automaton();
            quantifiers.insert(b);
        } else {
            // `baseChange.bind(List.of(reverse ? b : a, c));` (`:108`).
            base_change.bind(vec![if reverse { b.clone() } else { a.clone() }, c.clone()]);
            m = and(&m, &base_change, logging).into_automaton();
            // `negativeNumberSystem.arithmetic(reverse ? a : b, c, 0, PLUS)` (`:112`) --
            // the constant-RESULT overload, evaluated in the NEGATIVE number system.
            let negative_ns = resolve(session, logging, &negative_name)?;
            let sum_is_zero = negative_ns.arithmetic_const_c(
                if reverse { &a } else { &b },
                &c,
                &num_bigint::BigInt::from(0),
                ArithmeticOp::Plus,
                logging,
            )?;
            m = and(&m, &sum_is_zero, logging).into_automaton();
            quantifiers.insert(b);
            quantifiers.insert(c);
        }
    }

    // `AutomatonQuantification.quantify(M, quantifiers); M.sortLabel(); M.randomLabel();`
    // (`:118-120`) -- ONE quantify over the whole accumulated set, after the loop.
    //
    // `_with_ctx` with the CALLER's real `logging`, not the plain `quantify()` wrapper:
    // that wrapper substitutes a throwaway `Logging::new()`
    // (`wr_core::quantify::quantify`), which would silently swallow the whole
    // `quantifying:` / `Determinizing […]` / `Minimizing:` / `quantified:` /
    // `fixing leading zeros:` / `fixed leading zeros:` block Java prints here for every
    // subautomaton — 23 lines on a two-track `split …::` in a live comparison against the
    // real jar. That is exactly the `details`-text class U28 and
    // `docs/BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md` item 3 closed for every other
    // command family, and `split` landing after them must not reintroduce it. No golden
    // fixture would have caught it: none of the corpus's 15 `split`/`rsplit` fixtures
    // carries the `::` suffix (checked against `phase0-artifacts/test-manifest.json`), so
    // `tests/differential/tests/cli_command_logging.rs` is where it is pinned.
    //
    // The `ctx` stays `None`, deliberately and consistently with every other non-`eval`/
    // `def` command: threading a real `DeterminizeContext` is U32-scoped work gated on
    // Java's `printDetails` condition, and `wr-cli`'s command handlers have never had one.
    // The consequence is stated rather than left implicit: Java consumes automata indices
    // inside split's own `quantify`, so a `[strategy N …]`/`[export N …]` metacommand
    // paired with a `::`-suffixed `split` indexes differently there than here. That is the
    // port's existing, documented boundary (see `crate::meta_commands`), not a new gap.
    quantify_with_ctx(&mut m, &quantifiers, None, logging)
        .map_err(|e| SplitError::Core(quantify_error_message(&e)))?;
    m.sort_label();
    m.random_label();
    Ok(m)
}

/// `NumberSystem.setBaseChangeAutomaton()`'s file half (`:445-453`): probe
/// `Custom Bases/` for the two candidates [`numsys::base_change_candidate_names`] names,
/// hand them to `wr-core`, and return the resulting automaton.
fn build_base_change(
    session: &Session,
    logging: &mut Logging,
    negative_name: &str,
) -> Result<Automaton, SplitError> {
    let mut ns = (*resolve(session, logging, negative_name)?).clone();
    let (main_name, complement_name) = numsys::base_change_candidate_names(negative_name)?;
    // Java's `else if` is preserved rather than collapsed into two independent probes: the
    // complement file is stat'd only when the main file is absent (`:311-313`), the same
    // shape `Session::probe_custom_base` already uses for the other three.
    let main_address = session.paths().read_address_for_custom_bases(&main_name);
    let main = if Path::new(&main_address).is_file() {
        Some(read(session, &main_address)?)
    } else {
        None
    };
    let complement = if main.is_none() {
        let complement_address = session
            .paths()
            .read_address_for_custom_bases(&complement_name);
        if Path::new(&complement_address).is_file() {
            Some(read(session, &complement_address)?)
        } else {
            None
        }
    } else {
        None
    };
    ns.set_base_change_automaton(CustomBaseCandidates { main, complement }, logging)?;
    Ok(ns
        .base_change()
        .expect("set_base_change_automaton either errors or installs one")
        .clone())
}

fn resolve(
    session: &Session,
    logging: &mut Logging,
    name: &str,
) -> Result<Rc<NumberSystem>, SplitError> {
    session
        .libraries()
        .number_system(name, logging)
        .map_err(SplitError::Read)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ---------------------------------------------------------------------
    // Tier 2 — `Main/Commands/SplitTest.java`, method for method.
    //
    // That Java file was written for this port (2026-08-20, walnut-java commit
    // a2cfb30): before it, `Split.java` had NO unit test at all, only 15
    // integration fixtures that take the happy path exclusively. Every test below
    // is the twin of one of its methods, using the same fixtures and the same
    // reasoning; the two that have no Java twin say so.
    // ---------------------------------------------------------------------

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-split-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::remove_dir_all(&dir).ok();
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

    fn write(dir: &std::path::Path, sub: &str, name: &str, body: &str) {
        fs::write(dir.join(sub).join(name), body).unwrap();
    }

    /// The same `x != 0` operand `SplitTest` uses: one `msd_2` input, two distinct
    /// outputs, so `uncombine` really does produce two subautomata.
    const X_NOT_ZERO: &str = "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n";

    fn read_automaton(dir: &std::path::Path, sub: &str, name: &str) -> Automaton {
        wr_io::reader::read_automaton_txt(dir.join(sub).join(name)).unwrap()
    }

    fn log() -> Logging {
        Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()))
    }

    /// `SplitTest.testSplitFallsBackToTheAutomataLibraryAndWritesAPlainAutomaton`.
    /// The `isDFAO == false` branch (`Split.java:26-30`), which no golden fixture
    /// reaches — every one of the corpus's 15 `split`/`rsplit` operands is a word
    /// automaton.
    #[test]
    fn split_falls_back_to_the_automata_library_and_writes_a_plain_automaton() {
        let (session, dir) = temp_session("plain");
        write(&dir, "Automata Library", "plain.txt", X_NOT_ZERO);
        let tc = process_split_command(
            &session,
            &mut log(),
            "split out plain[+];",
            false,
            "plain",
            "out",
            "[+]",
        )
        .unwrap();
        assert_eq!(tc.automaton_pairs().len(), 1);
        assert!(dir.join("Automata Library/out.txt").is_file());
        assert!(!dir.join("Word Automata Library/out.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    /// The other half of the same branch, with no Java twin because Java's own
    /// fixtures cover it: the WORD library wins when both files exist, and the
    /// result is written back as a DFAO.
    #[test]
    fn split_prefers_the_word_library_and_writes_a_dfao() {
        let (session, dir) = temp_session("dfao");
        write(&dir, "Word Automata Library", "w.txt", X_NOT_ZERO);
        write(&dir, "Automata Library", "w.txt", X_NOT_ZERO);
        process_split_command(
            &session,
            &mut log(),
            "split out w[+];",
            false,
            "w",
            "out",
            "[+]",
        )
        .unwrap();
        assert!(dir.join("Word Automata Library/out.txt").is_file());
        assert!(!dir.join("Automata Library/out.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    /// `SplitTest.testMissingAutomatonThrows` (`Split.java:32`).
    #[test]
    fn a_missing_operand_reports_walnuts_own_message() {
        let (session, dir) = temp_session("missing");
        let err = process_split_command(
            &session,
            &mut log(),
            "split out nope[+];",
            false,
            "nope",
            "out",
            "[+]",
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "Automaton nope does not exist.");
        fs::remove_dir_all(&dir).ok();
    }

    /// `SplitTest.testNonPlusMinusOperatorThrows` (`Split.java:41-43`). Java needs a
    /// laxer `Matcher` than `Prover`'s own regex to reach this; here the equivalent
    /// is passing an input group the real dispatch could never capture. Both engines
    /// agree it is unreachable from the CLI — this pins the guard, not a live path.
    #[test]
    fn an_operator_that_is_neither_plus_nor_minus_is_an_invalid_command() {
        let (session, dir) = temp_session("badop");
        write(&dir, "Automata Library", "plain.txt", X_NOT_ZERO);
        // The real `input_in_split` regex only matches `[+]`/`[-]`/`[]`, so a `[*]`
        // group is simply not matched at all and the command fails one check later,
        // at "Cannot split without inputs." -- which is itself worth pinning, because
        // it is what a user actually gets.
        let err = process_split_command(
            &session,
            &mut log(),
            "split out plain[*];",
            false,
            "plain",
            "out",
            "[*]",
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "Cannot split without inputs.");
        // The guard itself, reached directly with a symbol the regex cannot produce.
        assert_eq!(op_from_symbol("*"), Some(ArithmeticOp::Mult));
        assert_eq!(op_from_symbol("+"), Some(ArithmeticOp::Plus));
        assert_eq!(op_from_symbol("-"), Some(ArithmeticOp::Minus));
        assert_eq!(op_from_symbol("?"), None);
        fs::remove_dir_all(&dir).ok();
    }

    /// `SplitTest.testNoInputsThrowsForBothSubConditions` (`Split.java:47-49`).
    #[test]
    fn no_inputs_is_an_error_for_both_sub_conditions() {
        let (session, dir) = temp_session("noinput");
        write(&dir, "Automata Library", "plain.txt", X_NOT_ZERO);
        for group in ["", "[][]"] {
            let err = process_split_command(
                &session,
                &mut log(),
                "split out plain;",
                false,
                "plain",
                "out",
                group,
            )
            .unwrap_err();
            assert_eq!(
                err.to_string(),
                "Cannot split without inputs.",
                "group {group:?}"
            );
        }
        fs::remove_dir_all(&dir).ok();
    }

    /// `SplitTest.testProcessSplitRejectsAnAutomatonWithNoInputs` (`Split.java:73-75`).
    #[test]
    fn process_split_rejects_an_automaton_with_no_inputs() {
        let (session, dir) = temp_session("noinputs");
        // Java's bare `new Automaton()`: no tracks, `FA.alphabetSize` still its `int`
        // default of 0 because `determineAlphabetSize()` was never called.
        let empty = Automaton::new(
            wr_core::fa::Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 0,
                o: vec![0],
                d: vec![Default::default()],
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(empty.fa.alphabet_size, 0);
        let err = process_split(
            &session,
            &mut log(),
            &empty,
            &[Some(ArithmeticOp::Plus)],
            false,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Cannot process split automaton with no inputs."
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// `SplitTest.testProcessSplitRejectsAnInputCountMismatch` (`Split.java:76-78`).
    #[test]
    fn process_split_rejects_an_input_count_mismatch() {
        let (session, dir) = temp_session("mismatch");
        write(
            &dir,
            "Automata Library",
            "one.txt",
            "msd_2\n\n0 1\n0 -> 0\n1 -> 0\n",
        );
        let one = read_automaton(&dir, "Automata Library", "one.txt");
        let err = process_split(
            &session,
            &mut log(),
            &one,
            &[Some(ArithmeticOp::Plus), Some(ArithmeticOp::Minus)],
            false,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Split automaton has incorrect number of inputs."
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// `SplitTest.testProcessSplitRequiresANumberSystemOnEverySplitInput`
    /// (`Split.java:96-97`). A `{0,1}` declaration carries no number system, which
    /// this crate models as `msd[i] == None` and so `track_ns_names()[i] == None` —
    /// exactly Java's `getNS().get(i) == null`.
    #[test]
    fn process_split_requires_a_number_system_on_every_converted_input() {
        let (session, dir) = temp_session("nons");
        write(
            &dir,
            "Automata Library",
            "nons.txt",
            "{0,1}\n\n0 1\n0 -> 0\n1 -> 0\n",
        );
        let no_ns = read_automaton(&dir, "Automata Library", "nons.txt");
        assert_eq!(no_ns.track_ns_names(), vec![None]);
        let err = process_split(
            &session,
            &mut log(),
            &no_ns,
            &[Some(ArithmeticOp::Plus)],
            false,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Number system for input 0 must be defined."
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// `SplitTest.testProcessSplitSkipsNullInputSlots` (`Split.java:92-94`) — the
    /// `[]` slot, which is what makes golden fixtures 449/450 (`split test449
    /// T2[+][]`) work: a track with no number system is fine as long as nothing asks
    /// to convert it.
    #[test]
    fn process_split_skips_null_input_slots() {
        let (session, dir) = temp_session("nullslot");
        write(
            &dir,
            "Automata Library",
            "two.txt",
            "msd_2 {0,1}\n\n0 1\n0 0 -> 0\n1 1 -> 0\n",
        );
        let two = read_automaton(&dir, "Automata Library", "two.txt");
        let out = process_split(
            &session,
            &mut log(),
            &two,
            &[Some(ArithmeticOp::Plus), None],
            false,
        )
        .unwrap();
        assert_eq!(out.alphabet.len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    /// `docs/WALNUT-BUGS.md` **WB-044**, pinned rather than fixed: a trivial
    /// (`true`/`false`) operand has an empty output vector, so `outputs` is empty,
    /// `uncombine` returns nothing, and `Split.java:57`'s `subautomata.remove(0)` blows up
    /// — in Java with `IndexOutOfBoundsException: Index 0 out of bounds for length 0`, here
    /// with `Vec::remove`'s own panic. Both engines recover (Java's
    /// `Prover.readBuffer` catch, this port's `Prover::caught`) and keep the session alive;
    /// only the message text differs, which is `ProverError::Thrown`'s pre-existing
    /// documented divergence.
    ///
    /// Verified by running the SAME command file through both engines, not inferred from
    /// the source. This test asserts the panic reaches the caller as a panic (so
    /// `Prover::caught` is what handles it) rather than being quietly swallowed here.
    #[test]
    fn split_on_a_true_automaton_recovers_like_java_does() {
        let (session, dir) = temp_session("truefalse");
        write(&dir, "Automata Library", "t.txt", "true\n");
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_split_command(
                &session,
                &mut log(),
                "split out t[+];",
                false,
                "t",
                "out",
                "[+]",
            )
        }));
        assert!(
            caught.is_err(),
            "WB-044: the unguarded remove(0) must still panic, for `Prover::caught` to \
             recover the way Java's own catch does"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// No Java twin — this is the port's own check that `split` and `rsplit` are
    /// genuinely different computations rather than one silently reused for the
    /// other (the three `reverse` ternaries in `processSplit`, see the module docs).
    ///
    /// `rsplit(split(A))` is the corpus's own round-trip shape (fixtures 440-447), so
    /// the two must at minimum disagree with each other on a non-symmetric operand.
    #[test]
    fn split_and_rsplit_are_different_computations() {
        let (session, dir) = temp_session("polarity");
        write(&dir, "Automata Library", "plain.txt", X_NOT_ZERO);
        let plain = read_automaton(&dir, "Automata Library", "plain.txt");
        let fwd = process_split(
            &session,
            &mut log(),
            &plain,
            &[Some(ArithmeticOp::Minus)],
            false,
        )
        .unwrap();
        let rev = process_split(
            &session,
            &mut log(),
            &plain,
            &[Some(ArithmeticOp::Minus)],
            true,
        )
        .unwrap();
        let mut a = fwd.clone();
        let mut b = rev.clone();
        a.fa.totalize(0);
        b.fa.totalize(0);
        assert!(
            !wr_core::equiv::automaton_language_equivalent(&a, &b).unwrap(),
            "split and rsplit must not produce the same language on `x != 0`"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
