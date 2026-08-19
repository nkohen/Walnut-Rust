// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Combine.java`, `Union.java`, `Intersect.java`, `Concat.java`, `Star.java`
//! (U23, batch A) — five "thin wrapper" commands that all share the same shape: parse a
//! whitespace-separated list of automaton names out of an already-regex-captured
//! substring, read each named automaton from `Automata Library/`, fold them together with
//! an already-ported `wr-core` primitive, and write the result. Grouped into one module
//! because none of the five is more than the assembly Java's own originals are — no new
//! algorithmic logic, just CLI-layer glue, matching this crate's `eval_def.rs`/`reg.rs`
//! precedent of one module per closely-related command family.
//!
//! # The shared name-list scanner
//!
//! `Union`/`Intersect`/`Concat` all build their own private `Pattern
//! PAT_FOR_AN_AUTOMATON_IN_{union,intersect,concat}_CMD` from the exact same text,
//! `RE_WORD_OF_CMD_NO_SPC = "([a-zA-Z]\\w*)"` (`Prover.java:40`), and scan their captured
//! argument list with `Matcher.find()` in a loop. [`scan_automaton_names`] is the one
//! Rust copy all three call, written as a manual ASCII scan rather than a
//! `regex`/`regex-automata` match — this crate's already-established idiom for exactly
//! this pattern shape (see `crate::eval_def::determine_free_variables`'s docs, the same
//! reasoning applies verbatim: no genuine backtracking ambiguity, and `wr-cli` gains
//! nothing from a regex engine for a single character class).
//!
//! `Combine`'s own scanner adds the optional `(=-?\d+)?` output suffix
//! (`RE_EQ_INT_OPTIONAL`, `Prover.java:43`) immediately after the identifier with no
//! intervening whitespace — [`scan_combine_tokens`] is a second, separate scan rather
//! than a generalization of [`scan_automaton_names`], matching how the two Java patterns
//! are textually unrelated (one is a strict prefix of the other's *shape*, not a
//! parameterization of the same `Pattern` constant).
//!
//! # NS-compatibility checks: `Union`/`Concat`'s `NumberSystem.isNSDiffering`
//!
//! Both call sites use [`wr_core::numsys::is_ns_differing`], which (per that function's
//! own docs) needed a real `Automaton` wired in as of this unit — done via
//! [`wr_core::automaton::Automaton::track_ns_names`]. That method reports each track's
//! REAL `NumberSystem.getName()`, threaded through from `wr-io`'s reader
//! ([`wr_core::automaton::Automaton::ns_name`]); an earlier draft of this unit
//! reconstructed the name from `(msd, alphabet.len())` instead, which made the guard
//! **fail open** on custom bases — `union u fib1 two1;` with an `msd_fib` operand and an
//! `msd_2` one silently produced a mixed-numeration result where real Walnut refuses with
//! `"Automata must have the same number system(s)."`.
//!
//! # Panics recovered at this boundary
//!
//! `combine` reaches `wr-core` guards that replicate Java `WalnutException`s as `assert!`s;
//! see [`crate::walnut_exception::catch_walnut_panic`] for why they are caught here rather
//! than being allowed to kill the process.

use wr_core::automaton::Automaton;
use wr_core::fa::Fa;
use wr_core::logging::Logging;
use wr_core::logicalops::{and, combine, or};
use wr_core::numsys::{is_ns_differing, TXT_EXTENSION};
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure the five commands in this module can produce.
#[derive(Debug)]
pub enum AutomatonOpsError {
    /// `Automaton.readAutomatonFromFile`/`new Automaton(address)` failed reading a named
    /// input automaton.
    Read(PredicateEnvError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
    /// A `WalnutException` whose message this module constructs directly — every one of
    /// the ad hoc `throw new WalnutException("...")` calls in `Combine`/`Union`/
    /// `Intersect`/`Concat`, preserved verbatim.
    Walnut(String),
    /// `Integer.parseInt(u.substring(1))`'s `NumberFormatException` (`Combine.java:38`).
    /// `PAT_FOR_AN_AUTOMATON_IN_combine_CMD`'s `(=-?\d+)?` group constrains the token to
    /// digits but NOT to `i32`'s range, so `combine c A=99999999999;` reaches `parseInt`
    /// with an out-of-range literal and throws — the command then fails outright, writing
    /// no output file. NOT a `WalnutException`, hence its own variant rather than
    /// [`AutomatonOpsError::Walnut`]: `Prover`'s handler renders it with a stack-trace
    /// header (`crate::prover::ProverError`'s `LoggableError` triage), exactly as Java's
    /// uncaught `NumberFormatException` is.
    NumberFormat(String),
}

impl std::fmt::Display for AutomatonOpsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutomatonOpsError::Read(e) => write!(f, "{e}"),
            AutomatonOpsError::Io(e) => write!(f, "{e}"),
            AutomatonOpsError::Walnut(m) => f.write_str(m),
            AutomatonOpsError::NumberFormat(input) => {
                write!(
                    f,
                    "{}",
                    crate::walnut_exception::number_format_exception(input)
                )
            }
        }
    }
}

impl std::error::Error for AutomatonOpsError {}

impl From<std::io::Error> for AutomatonOpsError {
    fn from(e: std::io::Error) -> Self {
        AutomatonOpsError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Shared scanners
// ---------------------------------------------------------------------------

/// `PAT_FOR_AN_AUTOMATON_IN_{union,intersect,concat}_CMD` = `RE_WORD_OF_CMD_NO_SPC =
/// "([a-zA-Z]\\w*)"`, scanned with `Matcher.find()`'s findall semantics: every maximal
/// run starting with an ASCII letter and continuing with `\w` (Java's ASCII-only class:
/// letters, digits, underscore), left to right, skipping everything else. See this
/// module's docs.
fn scan_automaton_names(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            result.push(chars[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    result
}

/// `PAT_FOR_AN_AUTOMATON_IN_combine_CMD` = `RE_WORD_OF_CMD_NO_SPC + "(" +
/// RE_EQ_INT_OPTIONAL + ")"` = `([a-zA-Z]\w*)((=-?\d+)?)`. Each match is `(name,
/// explicit_output)`, `explicit_output` being `None` when the optional suffix did not
/// participate (Java: `u.isEmpty()`) — [`combine_command`] applies the "default to the
/// 1-based argument position" rule Java's caller does, not this scanner.
///
/// Fallible because Java's `Integer.parseInt` is: the pattern admits an arbitrarily long
/// digit run, so an out-of-`i32`-range `=N` throws `NumberFormatException` rather than
/// being silently ignored. Returning `Option`-and-discarding-the-error here would demote
/// that to "no `=N` was given", which silently substitutes the positional-index default
/// and produces a DIFFERENT, wrong DFAO instead of failing.
fn scan_combine_tokens(s: &str) -> Result<Vec<(String, Option<i32>)>, AutomatonOpsError> {
    let chars: Vec<char> = s.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();

            let mut explicit_output = None;
            if i < chars.len() && chars[i] == '=' {
                let mut j = i + 1;
                if j < chars.len() && chars[j] == '-' {
                    j += 1;
                }
                let digits_start = j;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                if j > digits_start {
                    // A valid `=-?\d+` suffix: `u.substring(1)` in Java drops only the
                    // leading `=`, keeping a leading `-` if present.
                    let text: String = chars[i + 1..j].iter().collect();
                    explicit_output = Some(
                        text.parse::<i32>()
                            .map_err(|_| AutomatonOpsError::NumberFormat(text.clone()))?,
                    );
                    i = j;
                }
            }
            result.push((name, explicit_output));
        } else {
            i += 1;
        }
    }
    Ok(result)
}

/// `Automaton.readAutomatonFromFile(String automataName)` (`Automaton.java:148-150`) —
/// shared by every command in this module, all of which read exclusively from
/// `Automata Library/` (never `Word Automata Library/`, unlike `reverse`/quotient/etc.,
/// which take a `$`-flagged DFAO argument). `pub(crate)` since `crate::quotient` and
/// `crate::simple_transforms`'s `fixleadzero`/`fixtrailzero` are the exact same Java
/// `readAutomatonFromFile` call shape and reuse this one copy rather than a second,
/// independently-drifting one.
pub(crate) fn read_from_automata_library(
    session: &Session,
    name: &str,
) -> Result<Automaton, PredicateEnvError> {
    let address = session
        .paths()
        .read_file_for_automata_library(&format!("{name}{TXT_EXTENSION}"));
    session.libraries().read_library_automaton(&address)
}

/// `NumberSystem.isNSDiffering` applied to two already-loaded automata, via
/// [`Automaton::track_ns_names`] (see this module's docs).
fn ns_differs(a: &Automaton, b: &Automaton) -> bool {
    let a_names = a.track_ns_names();
    let b_names = b.track_ns_names();
    let a_ref: Vec<Option<&str>> = a_names.iter().map(|o| o.as_deref()).collect();
    let b_ref: Vec<Option<&str>> = b_names.iter().map(|o| o.as_deref()).collect();
    is_ns_differing(&a_ref, &b_ref, &a.alphabet, &b.alphabet)
}

// ---------------------------------------------------------------------------
// `combine`
// ---------------------------------------------------------------------------

/// `Combine.combineCommand(String s, String combineAutomata, String combineName)`
/// (`Combine.java:27-65`).
pub fn combine_command(
    session: &Session,
    s: &str,
    combine_automata: &str,
    combine_name: &str,
) -> Result<TestCase, AutomatonOpsError> {
    let tokens = scan_combine_tokens(combine_automata)?;
    let mut automata_names = Vec::with_capacity(tokens.len());
    let mut outputs: Vec<i32> = Vec::with_capacity(tokens.len());
    for (index, (name, explicit_output)) in tokens.into_iter().enumerate() {
        // `argumentCounter` is 1-based and incremented once per match (`:34`); the
        // default when no `=N` suffix was given is that same 1-based position (`:39`).
        let argument_counter = index as i32 + 1;
        outputs.push(explicit_output.unwrap_or(argument_counter));
        automata_names.push(name);
    }

    if automata_names.is_empty() {
        return Err(AutomatonOpsError::Walnut(
            "Combine requires at least one automaton as input.".to_string(),
        ));
    }
    let first_name = automata_names.remove(0);
    let first =
        read_from_automata_library(session, &first_name).map_err(AutomatonOpsError::Read)?;

    let mut subautomata = Vec::with_capacity(automata_names.len());
    for name in &automata_names {
        subautomata
            .push(read_from_automata_library(session, name).map_err(AutomatonOpsError::Read)?);
    }

    // `AutomatonLogicalOps.combine(first, subautomata, outputs)` (`:61`).
    //
    // Wrapped because `combine` reaches `wr_core::product::create_basic_automaton`, whose
    // two `WalnutException` guards ("...cannot be true or false automata." /
    // "...must have labeled inputs.") and whose alphabet-consistency check are `assert!`s
    // here. Both are reachable from ordinary CLI input — `combine c A B;` with operands of
    // different arity, or with the same variable declared over different alphabets — and
    // Java catches the resulting `WalnutException` in `Prover.dispatch`, prints it, and
    // keeps the REPL alive. See `crate::walnut_exception::catch_walnut_panic`.
    // A throwaway logger, not the caller's real one. NOTE this is a genuine, documented
    // gap, not "out of scope by design": real Walnut's `::` suffix works on EVERY
    // command via `Prover.parseSetup`'s universal `Logging.configureForCommand`, not just
    // `eval`/`def` -- so `combine c A B;::` really does print detailed logs in real
    // Walnut. This port's `wr-cli` dispatch does not yet thread a real `Logging` (with
    // the command's own `:`/`::`-derived `configure_for_command` state) into non-`eval`/
    // `def` commands at all, so even a correctly-instrumented `wr-core` primitive (as
    // `combine` now is) has nothing real to log into here. Wiring that up is a
    // `Prover`-dispatch-wide follow-up, not specific to `combine`.
    let mut c = crate::walnut_exception::catch_walnut_panic(|| {
        combine(
            &first,
            subautomata,
            &outputs,
            &mut wr_core::logging::Logging::new(),
        )
    })
    .map_err(AutomatonOpsError::Walnut)?;

    // `C.writeAutomata(s, Session.getWriteAddressForWordsLibrary(), combineName, true);`
    // (`:63`).
    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_words_library(),
        combine_name,
        true,
    )?;
    Ok(TestCase::from_automaton(c))
}

// ---------------------------------------------------------------------------
// `union` / `intersect`
// ---------------------------------------------------------------------------

/// Which of the two boolean folds [`union_or_intersect`] performs — Java's shared
/// `Union.unionOrIntersect(Automaton, List<String>, String op)` dispatches on a raw
/// `String` (`Prover.UNION`/`Prover.INTERSECT`); this port uses a real enum instead
/// (`PORTING.md`'s "typed enum over a string-tag switch" default), with no behavioral
/// difference since both call sites are fixed at compile time anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnionOrIntersect {
    Union,
    Intersect,
}

/// `Union.unionOrIntersect(Automaton automaton, List<String> automataNames, String op)`
/// (`Union.java:51-79`) — shared by [`union_command`] and [`intersect_command`], exactly
/// as Java's own `Intersect.intersect` calls `Union.unionOrIntersect` directly
/// (`Intersect.java:36`) rather than duplicating the fold.
fn union_or_intersect(
    session: &Session,
    automaton: Automaton,
    automata_names: &[String],
    op: UnionOrIntersect,
) -> Result<Automaton, AutomatonOpsError> {
    let mut first = automaton;
    for name in automata_names {
        let mut n = read_from_automata_library(session, name).map_err(AutomatonOpsError::Read)?;

        // `NumberSystem.isNSDiffering(N.getNS(), first.getNS(), N.richAlphabet.getA(),
        // first.richAlphabet.getA())` (`:58`).
        if ns_differs(&n, &first) {
            return Err(AutomatonOpsError::Walnut(
                "Automata must have the same number system(s).".to_string(),
            ));
        }

        // `first.randomLabel(); N.setLabel(first.getLabel());` (`:64-65`).
        first.random_label();
        n.label = first.label.clone();

        // Not an `eval`/`def` command -- a throwaway logger (see `combine_command`'s
        // matching note above).
        let mut logging = wr_core::logging::Logging::new();
        first = match op {
            UnionOrIntersect::Union => or(&mut first, &mut n, &mut logging).into_automaton(),
            UnionOrIntersect::Intersect => and(&first, &n, &mut logging).into_automaton(),
        };
    }
    Ok(first)
}

/// `Union.union(String s, String unionAutomata, String unionName)` (`Union.java:22-41`).
pub fn union_command(
    session: &Session,
    s: &str,
    union_automata: &str,
    union_name: &str,
) -> Result<TestCase, AutomatonOpsError> {
    let mut names = scan_automaton_names(union_automata);
    if names.is_empty() {
        return Err(AutomatonOpsError::Walnut(
            "Union requires at least one automaton as input.".to_string(),
        ));
    }
    let first_name = names.remove(0);
    let first =
        read_from_automata_library(session, &first_name).map_err(AutomatonOpsError::Read)?;

    let mut c = union_or_intersect(session, first, &names, UnionOrIntersect::Union)?;

    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        union_name,
        true,
    )?;
    Ok(TestCase::from_automaton(c))
}

/// `Intersect.intersect(String s, String intersectAutomata, String intersectName)`
/// (`Intersect.java:21-40`).
pub fn intersect_command(
    session: &Session,
    s: &str,
    intersect_automata: &str,
    intersect_name: &str,
) -> Result<TestCase, AutomatonOpsError> {
    let mut names = scan_automaton_names(intersect_automata);
    if names.is_empty() {
        return Err(AutomatonOpsError::Walnut(
            "Intersect requires at least one automaton as input.".to_string(),
        ));
    }
    let first_name = names.remove(0);
    let first =
        read_from_automata_library(session, &first_name).map_err(AutomatonOpsError::Read)?;

    let mut c = union_or_intersect(session, first, &names, UnionOrIntersect::Intersect)?;

    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        intersect_name,
        true,
    )?;
    Ok(TestCase::from_automaton(c))
}

// ---------------------------------------------------------------------------
// `concat`
// ---------------------------------------------------------------------------

/// `Concat.concat(Automaton automaton, Automaton other)` (`Concat.java:59-83`, private) —
/// the pairwise fold [`concat_command`] applies left to right.
fn concat_pair(
    automaton: Automaton,
    other: &Automaton,
    logging: &mut Logging,
) -> Result<Automaton, AutomatonOpsError> {
    // `NumberSystem.isNSDiffering(other.getNS(), automaton.getNS(),
    // automaton.richAlphabet.getA(), other.richAlphabet.getA())` (`:64`) -- note Java's
    // own argument order pairs `other`'s NS names with `automaton`'s alphabet; harmless,
    // since [`is_ns_differing`]'s name-list comparison and its separate alphabet
    // (in)equality check are each independently symmetric, but preserved here for
    // traceability back to the exact Java statement.
    if ns_differs(other, &automaton) {
        return Err(AutomatonOpsError::Walnut(
            "Automata to be concatenated must have the same number system(s).".to_string(),
        ));
    }

    // `Automaton N = automaton.clone(); int originalQ = automaton.fa.getQ();
    // FA.concatStates(other.fa, N.fa, originalQ);` (`:68-72`).
    let mut n = automaton.clone();
    let original_q = automaton.fa.q;
    Fa::concat_states(&other.fa, &mut n.fa, original_q);

    n.normalize_number_systems(logging);
    n.determinize_and_minimize();
    n.apply_all_representations(logging);
    Ok(n)
}

/// `Concat.concat(String s, String concatAutomata, String concatName)`
/// (`Concat.java:22-41`).
pub fn concat_command(
    session: &Session,
    logging: &mut Logging,
    s: &str,
    concat_automata: &str,
    concat_name: &str,
) -> Result<TestCase, AutomatonOpsError> {
    let names = scan_automaton_names(concat_automata);
    if names.len() < 2 {
        return Err(AutomatonOpsError::Walnut(
            "Concatenation requires at least two automata as input.".to_string(),
        ));
    }
    let mut iter = names.into_iter();
    let first_name = iter.next().expect("length checked above");
    let mut c =
        read_from_automata_library(session, &first_name).map_err(AutomatonOpsError::Read)?;

    for name in iter {
        let n = read_from_automata_library(session, &name).map_err(AutomatonOpsError::Read)?;
        c = concat_pair(c, &n, logging)?;
    }

    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        concat_name,
        true,
    )?;
    Ok(TestCase::from_automaton(c))
}

// ---------------------------------------------------------------------------
// `star`
// ---------------------------------------------------------------------------

/// `Star.star(Automaton automaton)` (`Star.java:21-36`).
fn star(automaton: &Automaton, logging: &mut Logging) -> Automaton {
    let mut n = automaton.clone();
    Fa::star_states(&automaton.fa, &mut n.fa);
    n.normalize_number_systems(logging);
    n.force_canonize();
    n.determinize_and_minimize();
    n.apply_all_representations(logging);
    n
}

/// `Star.star(String s, String oldName, String newName)` (`Star.java:11-19`).
pub fn star_command(
    session: &Session,
    logging: &mut Logging,
    s: &str,
    old_name: &str,
    new_name: &str,
) -> Result<TestCase, AutomatonOpsError> {
    let m = read_from_automata_library(session, old_name).map_err(AutomatonOpsError::Read)?;
    let mut c = star(&m, logging);
    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wr_io::writer::write_automaton_txt;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-automaton-ops-{tag}-{}-{}",
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

    /// A single-track `msd_2` automaton accepting exactly the words with a `1` symbol at
    /// some position (`less_than_msd(2)`'s two-track relation, restricted to one variable
    /// isn't handy here, so a small hand-built "contains a 1" automaton is used instead).
    fn contains_one() -> Automaton {
        let mut d0 = std::collections::BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = std::collections::BTreeMap::new();
        d1.insert(0, vec![1]);
        d1.insert(1, vec![1]);
        Automaton::new(
            wr_core::fa::Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![d0, d1],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    fn write_library_automaton(dir: &std::path::Path, name: &str, mut a: Automaton) {
        let path = dir.join("Automata Library").join(format!("{name}.txt"));
        write_automaton_txt(&mut a, &path).unwrap();
    }

    // ------------------------------------------------------------------ scan_*

    #[test]
    fn scan_automaton_names_finds_every_identifier_in_order() {
        assert_eq!(
            scan_automaton_names("  A B  C\tD"),
            vec!["A", "B", "C", "D"]
        );
    }

    #[test]
    fn scan_combine_tokens_defaults_and_explicit_outputs() {
        assert_eq!(
            scan_combine_tokens(" A B=5 C=-3 ").unwrap(),
            vec![
                ("A".to_string(), None),
                ("B".to_string(), Some(5)),
                ("C".to_string(), Some(-3)),
            ]
        );
    }

    // ------------------------------------------------------------------ combine

    #[test]
    fn combine_rejects_an_empty_automata_list() {
        let (session, dir) = temp_session("combine-empty");
        let err = combine_command(&session, "combine c;", "", "c").unwrap_err();
        assert!(matches!(err, AutomatonOpsError::Walnut(_)));
        assert_eq!(
            err.to_string(),
            "Combine requires at least one automaton as input."
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn combine_single_automaton_rewrites_accepting_states_to_the_given_output() {
        let (session, dir) = temp_session("combine-single");
        write_library_automaton(&dir, "A", contains_one());

        let tc = combine_command(&session, "combine c A=7;", " A=7 ", "c").unwrap();
        let a = tc.automaton_pairs()[0].automaton().unwrap();
        // Every state that used to be accepting (output 1) now outputs 7; the whole
        // thing must still accept exactly the same LANGUAGE (a word containing a 1).
        assert!(a.fa.accepts_word(&[0, 1, 0]));
        assert!(!a.fa.accepts_word(&[0, 0, 0]));

        let library_txt =
            fs::read_to_string(dir.join("Word Automata Library").join("c.txt")).unwrap();
        assert!(
            library_txt.contains('7'),
            "the combined output 7 must appear in the DFAO output"
        );
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------ union / intersect

    #[test]
    fn union_of_two_automata_matches_the_union_language() {
        let (session, dir) = temp_session("union");
        // A: accepts exactly "0"; B: accepts exactly "1" (both msd_2, one track).
        let accepts_zero = single_symbol_automaton(0);
        let accepts_one = single_symbol_automaton(1);
        write_library_automaton(&dir, "A", accepts_zero);
        write_library_automaton(&dir, "B", accepts_one);

        let tc = union_command(&session, "union c A B;", " A B ", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[0]));
        assert!(c.fa.accepts_word(&[1]));
        assert!(!c.fa.accepts_word(&[0, 0]));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn intersect_of_two_automata_matches_the_intersection_language() {
        let (session, dir) = temp_session("intersect");
        let accepts_one_a = single_symbol_automaton(1);
        let accepts_one_b = single_symbol_automaton(1);
        write_library_automaton(&dir, "A", accepts_one_a);
        write_library_automaton(&dir, "B", accepts_one_b);

        let tc = intersect_command(&session, "intersect c A B;", " A B ", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[1]));
        assert!(!c.fa.accepts_word(&[0]));
        fs::remove_dir_all(&dir).ok();
    }

    /// A single-track `msd_2` automaton whose language is exactly `{symbol}` (one
    /// letter, one word).
    fn single_symbol_automaton(symbol: i32) -> Automaton {
        let mut d0 = std::collections::BTreeMap::new();
        d0.insert(symbol, vec![1]);
        Automaton::new(
            wr_core::fa::Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![d0, std::collections::BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    // ------------------------------------------------------------------ concat

    #[test]
    fn concat_requires_at_least_two_automata() {
        let (session, dir) = temp_session("concat-too-few");
        let err =
            concat_command(&session, &mut Logging::new(), "concat c A;", "A", "c").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Concatenation requires at least two automata as input."
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn concat_of_two_automata_matches_the_concatenation_language() {
        // NOTE: `docs/WALNUT-BUGS.md` WB-009 (`FA.concatStates` never un-marks the first
        // operand's own accepting states) means the real, ported-verbatim language here
        // is `L(A) ∪ L(A)·L(B)`, not the textbook `L(A)·L(B)` -- so "0" alone (from
        // `L(A)` leaking through) is correctly ACCEPTED, not rejected. See
        // `wr_core::fa::Fa::concat_states`'s own doc comment for the same quirk pinned
        // at the `wr-core` layer.
        let (session, dir) = temp_session("concat");
        let accepts_zero = single_symbol_automaton(0);
        let accepts_one = single_symbol_automaton(1);
        write_library_automaton(&dir, "A", accepts_zero);
        write_library_automaton(&dir, "B", accepts_one);

        let tc =
            concat_command(&session, &mut Logging::new(), "concat c A B;", "A B", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[0, 1]), "\"01\" must be accepted");
        assert!(!c.fa.accepts_word(&[1, 0]), "\"10\" must be rejected");
        assert!(
            c.fa.accepts_word(&[0]),
            "WB-009: \"0\" alone leaks through from L(A) and must be accepted too"
        );
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------ star

    #[test]
    fn star_of_a_single_symbol_language_matches_the_kleene_star() {
        let (session, dir) = temp_session("star");
        write_library_automaton(&dir, "A", single_symbol_automaton(1));

        let tc = star_command(&session, &mut Logging::new(), "star c A;", "A", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[]), "empty word must be accepted");
        assert!(c.fa.accepts_word(&[1, 1, 1]));
        assert!(!c.fa.accepts_word(&[0]));
        assert!(!c.fa.accepts_word(&[1, 0]));
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------ NS-differing guard

    #[test]
    fn union_rejects_differing_number_systems() {
        let (session, dir) = temp_session("union-ns-diff");
        // Same alphabet size but a different base direction (msd vs lsd) -- `is_ns_differing`
        // must catch this even though the raw alphabet lists are identical.
        let mut msd_a = single_symbol_automaton(1);
        msd_a.msd = vec![Some(true)];
        let mut lsd_b = single_symbol_automaton(1);
        lsd_b.msd = vec![Some(false)];
        write_library_automaton(&dir, "A", msd_a);
        write_library_automaton(&dir, "B", lsd_b);

        let err = union_command(&session, "union c A B;", " A B ", "c").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Automata must have the same number system(s)."
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// The real `walnut-java` `Custom Bases/msd_fib.txt` / `msd_fib_addition.txt`, read
    /// straight out of `wr-io`'s fixture directory rather than duplicated here — see
    /// `crates/wr-io/tests/fixtures/ATTRIBUTION.md` for their provenance (GPLv3, Mousavi
    /// et al.). Copied into `dir/Custom Bases/` so the session resolves the base exactly
    /// as Java's `Session.getReadAddressForCustomBases` would.
    fn install_msd_fib(dir: &std::path::Path) {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wr-io/tests/fixtures")
            .canonicalize()
            .expect("wr-io fixture directory exists in this workspace");
        for name in ["msd_fib.txt", "msd_fib_addition.txt"] {
            fs::copy(fixtures.join(name), dir.join("Custom Bases").join(name)).unwrap();
        }
    }

    /// U23 review fix, finding #1 — the fail-open guard, at the command level.
    ///
    /// `msd_fib`'s alphabet is `{0, 1}` in the msd direction, i.e. indistinguishable from
    /// `msd_2`'s by everything except the NAME. Real Walnut refuses this pairing with
    /// `"Automata must have the same number system(s)."`; before this fix walnut-rs
    /// accepted it and silently wrote a meaningless mixed-numeration result.
    #[test]
    fn union_rejects_a_custom_base_paired_with_the_base_k_that_shares_its_alphabet() {
        let (session, dir) = temp_session("union-custom-base");
        install_msd_fib(&dir);
        let lib = dir.join("Automata Library");
        fs::write(lib.join("FIB.txt"), "msd_fib\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();
        fs::write(lib.join("TWO.txt"), "msd_2\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();

        let err = union_command(&session, "union c FIB TWO;", " FIB TWO ", "c").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Automata must have the same number system(s)."
        );
        assert!(
            !lib.join("c.txt").exists(),
            "the guard must fire before any output is written"
        );

        // ...and the same-base pairing still goes through, so the guard is not merely
        // rejecting everything.
        fs::write(lib.join("TWO2.txt"), "msd_2\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();
        union_command(&session, "union d TWO TWO2;", " TWO TWO2 ", "d").unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    /// Same root cause, through `intersect` and `concat` — both consume the same
    /// `ns_differs` primitive, with `concat`'s own (differently-worded) message.
    #[test]
    fn intersect_and_concat_reject_a_custom_base_paired_with_base_k_too() {
        let (session, dir) = temp_session("mixed-custom-base");
        install_msd_fib(&dir);
        let lib = dir.join("Automata Library");
        fs::write(lib.join("FIB.txt"), "msd_fib\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();
        fs::write(lib.join("TWO.txt"), "msd_2\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();

        let err =
            intersect_command(&session, "intersect c FIB TWO;", " FIB TWO ", "c").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Automata must have the same number system(s)."
        );

        let err = concat_command(
            &session,
            &mut Logging::new(),
            "concat c FIB TWO;",
            " FIB TWO ",
            "c",
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Automata to be concatenated must have the same number system(s)."
        );
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------ combine hardening

    /// U23 review fix, finding #3. `(=-?\d+)?` constrains the suffix to digits but not to
    /// `i32`'s range, so Java's `Integer.parseInt` throws `NumberFormatException` and the
    /// command fails outright. Discarding the parse error (the pre-fix `.ok()`) demoted
    /// that to "no `=N` was given" and silently used the positional-index default — a
    /// different, wrong DFAO written to disk instead of an error.
    #[test]
    fn combine_rejects_an_out_of_i32_range_output_value_instead_of_defaulting() {
        let (session, dir) = temp_session("combine-overflow");
        write_library_automaton(&dir, "A", contains_one());

        let err = combine_command(&session, "combine c A=99999999999;", " A=99999999999 ", "c")
            .unwrap_err();
        assert!(matches!(err, AutomatonOpsError::NumberFormat(_)));
        assert_eq!(err.to_string(), "For input string: \"99999999999\"");
        assert!(
            !dir.join("Word Automata Library").join("c.txt").exists(),
            "Java writes no output file when parseInt throws"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// U23 review fix, finding #2. `wr_core::product::create_basic_automaton`'s
    /// `WalnutException` guards are `assert!`s; unwrapped, they killed the whole process
    /// (no `catch_unwind` boundary exists in this workspace), losing an entire `load`ed
    /// batch-file session where Java loses only the one command.
    #[test]
    fn combine_reports_a_wr_core_guard_as_an_error_instead_of_killing_the_process() {
        let (session, dir) = temp_session("combine-arity");
        // Arity 1 vs arity 2: `cross_product`'s "must have labeled inputs" guard.
        write_library_automaton(&dir, "A", contains_one());
        let two_track = Automaton::new(
            wr_core::fa::Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 4,
                o: vec![1],
                d: vec![std::collections::BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![Some(true), Some(true)],
        );
        write_library_automaton(&dir, "P2", two_track);

        let err = combine_command(&session, "combine c A P2;", " A P2 ", "c").unwrap_err();
        assert!(matches!(err, AutomatonOpsError::Walnut(_)));
        assert!(
            err.to_string().contains("crossProduct"),
            "the recovered message is Java's own guard text, got {err}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
