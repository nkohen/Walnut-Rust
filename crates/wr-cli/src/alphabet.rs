// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Alphabet.java` (65 LOC) **and** `Automata/Automaton.setAlphabet`
//! (`Automaton.java:184-226`) — the `alphabet` command's full body, ported together in
//! Phase 3a's U16 per the plan's explicit note: an earlier draft only covered
//! `determineAlphabetsAndNS` (the alphabet-declaration *parsing* half) and missed the
//! automaton-mutation half that actually applies the new alphabet.
//!
//! `Alphabet.determineAlphabetsAndNS` is also `Reg.reg`'s (`crate::reg`) entry point for
//! parsing its own `listOfAlphabets` argument (`Reg.java:20`) — this module is the shared
//! home for both commands' alphabet-declaration grammar, matching Java's own dependency
//! direction (`Reg.java` imports `Main.Commands.Alphabet`, not the reverse).
//!
//! # `RE_FOR_AN_ALPHABET`, as a hand-written scanner
//!
//! Java's grammar (`Alphabet.java:14-15`):
//!
//! ```text
//! ((((msd|lsd)_(\d+|\w+))|((msd|lsd)(\d+|\w+))|(msd|lsd)|(\d+|\w+))
//!   |(\{(\s*(\+|\-)?\s*\d+)(\s*,\s*(\+|\-)?\s*\d+)*\s*\}))\s+
//! ```
//!
//! applied with `Matcher.find()` in a loop, exactly [`wr_core::regex`]'s own
//! `determine_encoded_regex` scanner (U8) does for a differently-shaped alphabet-vector
//! pattern — this module follows the same "hand-written scanner, not a `regex`/
//! `regex-automata` dependency" convention for the same reason: the grammar is small and
//! has no genuine backtracking ambiguity once analyzed.
//!
//! **The number-system alternative always reduces to "the maximal leading run of `\w`
//! characters."** All four of its Java sub-alternatives (`msd_2`, `msd2`, `msd`/`lsd`
//! bare, or a bare word/digit run) are built from `\w+`/`\d+` pieces with no whitespace
//! inside, so whichever one Java's backtracking picks, the captured text is always
//! exactly the maximal `\w+` run starting at the match position — *provided* that run is
//! immediately followed by the pattern's mandatory trailing `\s+` (if it is not, every
//! sub-alternative fails at that position for the same reason: the run's own interior has
//! no whitespace to satisfy `\s+` early, so there is nothing to backtrack into). This is
//! why [`match_number_system_token`] does not need to replicate Java's four-way
//! alternation explicitly.
//!
//! The `{…}`/set alternative (group 11, `Prover.R_SET`) is structurally the same
//! bracketed-list grammar `wr_core::regex`'s `RE_FOR_AN_ALPHABET_VECTOR` already scans
//! (just `{}` instead of `[]`, and a *mandatory* trailing `\s+` this module's caller
//! needs and that one doesn't) — [`match_set_token`]/[`parse_set_elements`] are a second,
//! independent copy of that same small piece of logic. Not shared with `wr_core::regex`:
//! that module's scanner internals are private to `wr-core`, and `Prover.java`'s own
//! `PAT_FOR_A_SINGLE_ELEMENT_OF_A_SET` this mirrors is itself already duplicated across
//! two Java files (`Reg.determineEncodedRegex` and `Alphabet.determineAlphabet`) sharing
//! one `Pattern` constant — the Rust port's crate boundary (`wr-core` vs `wr-cli`) turns
//! that Java-level sharing into two small, independently-testable copies instead. ~30
//! lines either way; not worth a new cross-crate dependency edge.
//!
//! Java's `"Alphabet at position N not recognized in alphabet command"` branch
//! (`Alphabet.java:47`) is not ported: it is unreachable in Java too — `Matcher.find()`
//! only ever calls this loop body after the OUTER `(A|B)\s+` pattern already matched, so
//! one of the two mutually-exclusive top-level alternatives always populated a group.
//! [`try_match_alphabet_token`] is structured the same way (branching on `{` vs. a word
//! character before attempting either alternative), so the "neither" case is likewise
//! not a reachable outcome here — nothing was silently dropped.
//!
//! # `Automaton.setAlphabet`: which existing primitives it assembles
//!
//! [`set_alphabet`] is exactly the free-function assembly `wr_core::automaton`'s own
//! module docs anticipated ("a future unit can call this once it has real `NumberSystem`
//! objects"): [`Automaton::is_in_new_alphabet`]/[`Automaton::rebuild_transitions_for_new_alphabet`]
//! (pre-positioned in U2), [`Automaton::apply_all_representations_with_output`] (U5),
//! and [`Automaton::encoder`] (added alongside this unit so the rebuild step can read
//! back the freshly recomputed encoder — see that method's doc comment). It stays a
//! `wr-cli` free function rather than an `Automaton` method: `NumberSystem` now lives in
//! `wr-core` too, so it *could* move there, but the plan assigns the whole method to this
//! unit and there is no other `wr-core` caller yet to justify relocating it.
//!
//! A faithfully-preserved quirk: both `Reg::reg` and this module's [`alphabet_command`]
//! call `write_automata` with `is_dfao_for_gv` hardcoded to `false` (`Alphabet.java:29`,
//! `Reg.java:35`), even when the command's own `is_dfao` is `true`. So a genuine DFAO
//! result from `alphabet $dfao-name ...` (no `$`) still gets rendered to `.gv` in the
//! plain accept/reject-circle style, not the `"q/output"` DFAO style — ported verbatim,
//! not a new divergence introduced here.

use std::rc::Rc;

use wr_core::automaton::Automaton;
use wr_core::numsys::{normalize_number_system_token, NumberSystem};
use wr_core::util::{parse_int, remove_duplicates};
use wr_core::word_automaton::minimize_self_with_output;
use wr_logic::predicate_env::{PredicateEnv, PredicateEnvError};

use crate::automaton_output::write_automata;
use crate::prover_helper::{determine_in_library, determine_out_library};
use crate::session::Session;
use crate::test_case::TestCase;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure `alphabet_command`/`determine_alphabets_and_ns`/`set_alphabet` can
/// produce. Mirrors this crate's established convention (see `wr_core::regex::RegexError`,
/// `wr_logic::predicate_env::PredicateEnvError`) of a plain enum with a `Display`
/// producing Walnut's exact message text where one exists, rather than pulling in a
/// `thiserror`-style dependency (none of `walnut-rs`'s crates use one).
#[derive(Debug)]
pub enum AlphabetError {
    /// A `WalnutException` whose message this module constructs directly — the two
    /// guards in `Automaton.setAlphabet` (`:185-190`) and `Alphabet.alphabetCommand`'s
    /// null check (`:19-21`).
    Walnut(String),
    /// `NumberSystem.getComputeIfAbsent` failed for a named number-system token (e.g.
    /// `msd_7` when nothing defines base 7, or a genuinely unresolvable custom base).
    NumberSystem(PredicateEnvError),
    /// `new Automaton(address)` failed reading the command's input automaton
    /// (`Alphabet.java:20`).
    Read(PredicateEnvError),
    /// Propagated from [`write_automata`] — see that function's docs for why this
    /// module does not swallow-and-log the way Java's `writeAutomata` does.
    Io(std::io::Error),
}

impl std::fmt::Display for AlphabetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlphabetError::Walnut(m) => f.write_str(m),
            AlphabetError::NumberSystem(e) => write!(f, "{e}"),
            AlphabetError::Read(e) => write!(f, "{e}"),
            AlphabetError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AlphabetError {}

impl From<std::io::Error> for AlphabetError {
    fn from(e: std::io::Error) -> Self {
        AlphabetError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// `Alphabet.determineAlphabetsAndNS` -- the shared alphabet-declaration grammar
// ---------------------------------------------------------------------------

enum AlphabetToken {
    NumberSystem(String),
    Set(Vec<i32>),
}

fn is(c: u16, ch: char) -> bool {
    c == ch as u16
}

/// Java's `\s` character class: `[ \t\n\x0B\f\r]` (ASCII-only, no
/// `UNICODE_CHARACTER_CLASS` flag), not Rust's Unicode-aware `char::is_whitespace`.
fn is_java_regex_space(c: u16) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D)
}

fn is_ascii_digit_unit(c: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&c)
}

/// Java's `\w`: `[a-zA-Z_0-9]`.
fn is_word_char(c: u16) -> bool {
    (u16::from(b'a')..=u16::from(b'z')).contains(&c)
        || (u16::from(b'A')..=u16::from(b'Z')).contains(&c)
        || is_ascii_digit_unit(c)
        || c == u16::from(b'_')
}

fn skip_spaces(units: &[u16], mut p: usize) -> usize {
    while p < units.len() && is_java_regex_space(units[p]) {
        p += 1;
    }
    p
}

/// `\s*(\+|\-)?\s*\d+` anchored at `p`.
fn match_signed_number(units: &[u16], p: usize) -> Option<usize> {
    let mut p = skip_spaces(units, p);
    if p < units.len() && (is(units[p], '+') || is(units[p], '-')) {
        p += 1;
    }
    p = skip_spaces(units, p);
    let start = p;
    while p < units.len() && is_ascii_digit_unit(units[p]) {
        p += 1;
    }
    (p > start).then_some(p)
}

/// `Prover.PAT_FOR_A_SINGLE_ELEMENT_OF_A_SET` (`"(\\+|\\-)?\\s*\\d+"`) applied with
/// `Matcher.find()` in a loop, as `Alphabet.determineAlphabet` does.
fn parse_set_elements(units: &[u16]) -> Vec<i32> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < units.len() {
        let with_sign = if is(units[i], '+') || is(units[i], '-') {
            let after = skip_spaces(units, i + 1);
            let mut p = after;
            while p < units.len() && is_ascii_digit_unit(units[p]) {
                p += 1;
            }
            (p > after).then_some(p)
        } else {
            None
        };
        let m = with_sign.or_else(|| {
            let mut p = i;
            while p < units.len() && is_ascii_digit_unit(units[p]) {
                p += 1;
            }
            (p > i).then_some(p)
        });
        match m {
            Some(end) => {
                let text = String::from_utf16_lossy(&units[i..end]);
                out.push(parse_int(&text));
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

/// Attempts to match `RE_FOR_AN_ALPHABET` starting EXACTLY at `at`. Returns the token
/// plus the offset just past the mandatory trailing `\s+`, or `None` if nothing matches
/// starting there (mirroring one failed `Matcher.find()` attempt at this position).
fn try_match_alphabet_token(units: &[u16], at: usize) -> Option<(AlphabetToken, usize)> {
    if at >= units.len() {
        return None;
    }
    if is(units[at], '{') {
        match_set_token(units, at)
    } else if is_word_char(units[at]) {
        match_number_system_token(units, at)
    } else {
        None
    }
}

fn match_number_system_token(units: &[u16], at: usize) -> Option<(AlphabetToken, usize)> {
    let mut end = at;
    while end < units.len() && is_word_char(units[end]) {
        end += 1;
    }
    let ws_end = skip_spaces(units, end);
    if ws_end == end {
        return None; // the mandatory trailing `\s+` did not match
    }
    let text = String::from_utf16_lossy(&units[at..end]);
    Some((AlphabetToken::NumberSystem(text), ws_end))
}

fn match_set_token(units: &[u16], at: usize) -> Option<(AlphabetToken, usize)> {
    let mut p = match_signed_number(units, at + 1)?; // past '{', first (mandatory) element
    loop {
        let q = skip_spaces(units, p);
        if q < units.len() && is(units[q], ',') {
            match match_signed_number(units, q + 1) {
                Some(r) => {
                    p = r;
                    continue;
                }
                None => break,
            }
        }
        break;
    }
    let close = skip_spaces(units, p);
    if !(close < units.len() && is(units[close], '}')) {
        return None;
    }
    let after_close = close + 1;
    let ws_end = skip_spaces(units, after_close);
    if ws_end == after_close {
        return None; // the mandatory trailing `\s+` did not match
    }
    // `s = s.substring(1, s.length() - 1)` (`Alphabet.java:56`) -- truncate the braces.
    let mut elems = parse_set_elements(&units[at + 1..close]);
    // `UtilityMethods.removeDuplicates(L)` (`Alphabet.java:59`).
    remove_duplicates(&mut elems);
    Some((AlphabetToken::Set(elems), ws_end))
}

/// `Alphabet.determineAlphabetsAndNS(String, List<NumberSystem>, List<List<Integer>>)`
/// (`Alphabet.java:33-49`), returning the two lists instead of writing into caller-owned
/// out-parameters. `env` resolves named number-system tokens (`NumberSystem
/// .getComputeIfAbsent`); a `{…}` literal set never touches it, matching Java's `NS.add
/// (null)` for that branch.
#[allow(clippy::type_complexity)]
pub fn determine_alphabets_and_ns(
    list_of_alphabets: &str,
    env: &dyn PredicateEnv,
) -> Result<(Vec<Option<Rc<NumberSystem>>>, Vec<Vec<i32>>), AlphabetError> {
    let units: Vec<u16> = list_of_alphabets.encode_utf16().collect();
    let mut ns = Vec::new();
    let mut alphabets = Vec::new();
    let mut i = 0usize;
    while i < units.len() {
        match try_match_alphabet_token(&units, i) {
            Some((AlphabetToken::NumberSystem(text), end)) => {
                let base = normalize_number_system_token(Some(&text));
                let resolved = env
                    .number_system(&base)
                    .map_err(AlphabetError::NumberSystem)?;
                alphabets.push(resolved.get_alphabet().to_vec());
                ns.push(Some(resolved));
                i = end;
            }
            Some((AlphabetToken::Set(elems), end)) => {
                alphabets.push(elems);
                ns.push(None);
                i = end;
            }
            None => i += 1,
        }
    }
    Ok((ns, alphabets))
}

/// The `all_reps` half of Java's `Automaton.setNS(numberSystems)` (a plain field
/// setter in Java, `this.NS = NS;`, whose two facts -- msd/lsd direction and the
/// custom-base valid-representation restriction -- this crate keeps as two parallel
/// per-track vectors, per [`wr_core::automaton::Automaton::all_reps`]'s struct docs).
/// Shared by [`set_alphabet`] and `crate::reg::reg` (both effectively call `setNS`).
pub(crate) fn all_reps_from_ns(
    number_systems: &[Option<Rc<NumberSystem>>],
) -> Vec<Option<Rc<Automaton>>> {
    number_systems
        .iter()
        .map(|o| {
            o.as_ref().and_then(|ns| {
                ns.use_all_representations().then(|| {
                    Rc::clone(ns.all_representations().expect(
                        "use_all_representations() true implies all_representations() is Some",
                    ))
                })
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// `Automaton.setAlphabet`
// ---------------------------------------------------------------------------

/// `Automaton.setAlphabet(boolean isDFAO, List<NumberSystem> numberSystems,
/// List<List<Integer>> alphabet)` (`Automaton.java:184-226`). Mutates `automaton` in
/// place (`copy(M)`, the final statement of the Java method).
///
/// Java's `Logging.indent()`/`logMessage`/`dedent()` calls throughout are not ported,
/// matching every other `wr-core`/`wr-cli` call site so far (`crate::product`'s "Progress
/// logging not ported" note) -- `crate::logging`'s context is not threaded into this call
/// graph yet.
pub fn set_alphabet(
    automaton: &mut Automaton,
    is_dfao: bool,
    number_systems: &[Option<Rc<NumberSystem>>],
    alphabet: Vec<Vec<i32>>,
) -> Result<(), AlphabetError> {
    // `:185-190`.
    if alphabet.len() != automaton.alphabet.len() {
        return Err(AlphabetError::Walnut(
            "The number of alphabets must match the number of alphabets in the input automaton."
                .to_string(),
        ));
    }
    if alphabet.len() != number_systems.len() {
        return Err(AlphabetError::Walnut(
            "The number of alphabets must match the number of number systems.".to_string(),
        ));
    }

    // `Automaton M = clone();` (`:202`) plus `M.richAlphabet.setA(alphabet)` /
    // `M.setNS(numberSystems)` (`:203-204`).
    let mut m = automaton.clone();
    m.alphabet = alphabet;
    m.msd = number_systems
        .iter()
        .map(|o| o.as_ref().map(|ns| ns.is_msd()))
        .collect();
    // `set_all_reps`/`set_ns_names` require `msd` already parallel to `alphabet` -- just
    // established above. The third part of the per-track `NumberSystem` stand-in is the
    // NAME (`wr_core::automaton::Automaton::ns_name`): a track switched to a literal
    // `{...}` set has no number system and therefore no name, and a track switched to a
    // custom base must carry that base's real name or `isNSDiffering` cannot later tell
    // it apart from the plain base with the same alphabet cardinality.
    m.set_all_reps(all_reps_from_ns(number_systems));
    m.set_ns_names(
        number_systems
            .iter()
            .map(|o| o.as_ref().map(|ns| ns.name().to_string()))
            .collect(),
    );

    // `M.determineAlphabetSize(); M.richAlphabet.setupEncoder();` (`:205-206`).
    m.determine_alphabet_size();
    m.setup_encoder();

    // `rebuildTransitions(this.getFa(), this.richAlphabet, M);` (`:208`) -- reads from
    // the STILL-UNCHANGED `automaton` (Java's `this`), writes into `m` (`M`).
    m.fa.d = Automaton::rebuild_transitions_for_new_alphabet(automaton, &m.alphabet, m.encoder());

    // `:210-214`.
    if is_dfao {
        minimize_self_with_output(&mut m);
    } else {
        m.determinize_and_minimize();
    }

    // `M.forceCanonize();` (`:216`).
    m.force_canonize();

    // `M.applyAllRepresentationsWithOutput();` (`:219`).
    m.apply_all_representations_with_output();

    // `copy(M);` (`:222`).
    *automaton = m;
    Ok(())
}

// ---------------------------------------------------------------------------
// `Alphabet.alphabetCommand`
// ---------------------------------------------------------------------------

// `ProverHelper.determineInLibrary`/`determineOutLibrary` (`ProverHelper.java:64-72`) used
// to be ported privately here, because U16 needed them before `ProverHelper` had a module.
// U21 gave them their real home; this file now calls that single copy rather than keeping a
// second one that could drift (the same call this crate already made for
// `Session::read_library_automaton`).

/// `Alphabet.alphabetCommand(String s, String listOfAlphabets, boolean isDFAO, String
/// inFileName, String newName)` (`Alphabet.java:17-30`).
///
/// `list_of_alphabets` is `Option<&str>` because Java's parameter is a nullable
/// `String` and the very first thing this method does is null-check it
/// (`:19-21`) -- unlike `Reg::reg` (`crate::reg::reg`), whose own `Alphabet
/// .determineAlphabetsAndNS` call has no such guard.
pub fn alphabet_command(
    session: &Session,
    s: &str,
    list_of_alphabets: Option<&str>,
    is_dfao: bool,
    in_file_name: &str,
    new_name: &str,
) -> Result<TestCase, AlphabetError> {
    let list_of_alphabets = list_of_alphabets.ok_or_else(|| {
        AlphabetError::Walnut(
            "List of alphabets for alphabet command must not be empty.".to_string(),
        )
    })?;

    let (ns, alphabets) = determine_alphabets_and_ns(list_of_alphabets, session.predicate_env())?;

    // `Automaton M = new Automaton(ProverHelper.determineInLibrary(isDFAO, inFileName));`
    // (`:20`).
    let in_address = determine_in_library(session.paths(), is_dfao, in_file_name);
    let mut automaton = session
        .libraries()
        .read_library_automaton(&in_address)
        .map_err(AlphabetError::Read)?;

    // `M.setAlphabet(isDFAO, NS, alphabets);` (`:23`).
    set_alphabet(&mut automaton, is_dfao, &ns, alphabets)?;

    // `M.writeAutomata(s, ProverHelper.determineOutLibrary(isDFAO), newName, false);`
    // (`:25`) -- note the hardcoded `false`, see this module's docs.
    let out_library = determine_out_library(session.paths(), is_dfao);
    write_automata(session, &mut automaton, s, &out_library, new_name, false)?;

    Ok(TestCase::from_automaton(automaton))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use wr_core::fa::Fa;
    use wr_logic::predicate_env::InMemoryPredicateEnv;

    // -- `determine_alphabets_and_ns` -------------------------------------------------

    #[test]
    fn a_literal_set_alphabet_resolves_to_no_number_system() {
        let env = InMemoryPredicateEnv::new();
        let (ns, alphabets) = determine_alphabets_and_ns("{0,1} ", &env).unwrap();
        assert!(ns.iter().all(Option::is_none));
        assert_eq!(ns.len(), 1);
        assert_eq!(alphabets, vec![vec![0, 1]]);
    }

    #[test]
    fn a_set_with_signs_and_whitespace_and_duplicates_parses_and_dedupes() {
        let env = InMemoryPredicateEnv::new();
        let (ns, alphabets) = determine_alphabets_and_ns("{ -1, +2 , 2, 3 } ", &env).unwrap();
        assert!(ns.iter().all(Option::is_none));
        assert_eq!(ns.len(), 1);
        assert_eq!(alphabets, vec![vec![-1, 2, 3]]);
    }

    #[test]
    fn a_named_number_system_resolves_through_the_env_and_carries_its_alphabet() {
        let env = InMemoryPredicateEnv::new();
        let (ns, alphabets) = determine_alphabets_and_ns("msd_3 ", &env).unwrap();
        assert_eq!(alphabets, vec![vec![0, 1, 2]]);
        assert!(ns[0]
            .as_ref()
            .is_some_and(|n| n.name() == "msd_3" && n.is_msd()));
    }

    #[test]
    fn bare_msd_normalizes_to_msd_2() {
        let env = InMemoryPredicateEnv::new();
        let (ns, alphabets) = determine_alphabets_and_ns("msd ", &env).unwrap();
        assert_eq!(alphabets, vec![vec![0, 1]]);
        assert_eq!(ns[0].as_ref().unwrap().name(), "msd_2");
    }

    #[test]
    fn lsd_without_underscore_normalizes_like_java() {
        let env = InMemoryPredicateEnv::new();
        let (ns, alphabets) = determine_alphabets_and_ns("lsd4 ", &env).unwrap();
        assert_eq!(alphabets, vec![vec![0, 1, 2, 3]]);
        assert!(ns[0].as_ref().is_some_and(|n| !n.is_msd()));
    }

    #[test]
    fn multiple_tracks_parse_in_order_mixing_sets_and_number_systems() {
        let env = InMemoryPredicateEnv::new();
        let (ns, alphabets) = determine_alphabets_and_ns("msd_2 {0,1,2} lsd_2 ", &env).unwrap();
        assert_eq!(ns.len(), 3);
        assert!(ns[0].is_some());
        assert!(ns[1].is_none());
        assert!(ns[2].is_some());
        assert_eq!(alphabets, vec![vec![0, 1], vec![0, 1, 2], vec![0, 1]]);
    }

    #[test]
    fn a_token_with_no_trailing_whitespace_does_not_match_at_all() {
        // The pattern's trailing `\s+` is mandatory; without it Java's `find()` never
        // matches this token anywhere in the string.
        let env = InMemoryPredicateEnv::new();
        let (ns, alphabets) = determine_alphabets_and_ns("{0,1}", &env).unwrap();
        assert!(ns.is_empty());
        assert!(alphabets.is_empty());
    }

    #[test]
    fn an_unresolvable_number_system_reports_a_number_system_error() {
        let env = InMemoryPredicateEnv::new();
        let err = determine_alphabets_and_ns("msd_neg_2 ", &env).unwrap_err();
        assert!(matches!(err, AlphabetError::NumberSystem(_)));
    }

    // -- `set_alphabet` ----------------------------------------------------------------

    fn digits(alphabet: &[i32]) -> Vec<i32> {
        alphabet.to_vec()
    }

    /// A tiny one-track automaton over `{0,1,2,3}`: state 0 (initial, non-accepting)
    /// transitions to state 1 (accepting) on every one of the four digits; state 1 has
    /// no outgoing transitions. Language: exactly the one-symbol words.
    fn one_track_over_four_digits() -> Automaton {
        let mut automaton = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 4,
                o: vec![0, 1],
                d: vec![BTreeMap::new(), BTreeMap::new()],
            },
            vec![digits(&[0, 1, 2, 3])],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        for d in 0..4 {
            let sym = automaton.encode(&[d]);
            automaton.fa.d[0].insert(sym, vec![1]);
        }
        automaton
    }

    #[test]
    fn set_alphabet_rejects_a_track_count_mismatch_against_the_automaton() {
        let mut a = one_track_over_four_digits();
        let err =
            set_alphabet(&mut a, false, &[None, None], vec![vec![0, 1], vec![0, 1]]).unwrap_err();
        assert!(matches!(err, AlphabetError::Walnut(m) if m.contains("input automaton")));
    }

    #[test]
    fn set_alphabet_rejects_an_alphabet_ns_length_mismatch() {
        let mut a = one_track_over_four_digits();
        let err = set_alphabet(&mut a, false, &[None, None], vec![vec![0, 1]]).unwrap_err();
        assert!(matches!(err, AlphabetError::Walnut(m) if m.contains("number systems")));
    }

    /// Mirrors the real capture behind `tests/differential/fixtures/alphabet/baseC*.txt`:
    /// restricting `{0,1,2,3}` down to `{0,1}` must prune the digit-2/3 transitions
    /// (which existed) and re-minimize, not merely relabel the header.
    #[test]
    fn set_alphabet_prunes_transitions_outside_the_new_alphabet() {
        let mut a = one_track_over_four_digits();
        set_alphabet(&mut a, false, &[None], vec![vec![0, 1]]).unwrap();

        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        // Only digits 0 and 1 can reach acceptance now; the automaton must still accept
        // exactly the one-symbol words drawn from {0,1} and reject everything else
        // (in particular, it must NOT still recognize a stray digit 2/3 as one-symbol
        // acceptance, since those transitions no longer exist to encode against).
        let accept = |w: &[i32]| -> bool {
            let mut state = a.fa.q0;
            for &d in w {
                let sym = a.encode(&[d]);
                match a.fa.d[state].get(&sym).and_then(|dests| dests.first()) {
                    Some(&next) => state = next,
                    None => return false,
                }
            }
            a.fa.o[state] != 0
        };
        assert!(accept(&[0]));
        assert!(accept(&[1]));
        assert!(!accept(&[]));
    }

    #[test]
    fn set_alphabet_installs_the_new_number_systems_msd_direction() {
        let ns2 = Rc::new(NumberSystem::new("lsd_2").unwrap());
        let mut a = one_track_over_four_digits();
        set_alphabet(&mut a, false, &[Some(Rc::clone(&ns2))], vec![vec![0, 1]]).unwrap();
        assert_eq!(a.msd, vec![Some(false)]);
    }

    /// A one-track word (DFAO) automaton over `{0,1,2,3}` with genuine non-boolean
    /// per-state outputs: the start state outputs `5`, and digit `0` steps to a second
    /// state outputting `7`. Every other digit has no outgoing transition.
    fn one_track_word_automaton_over_four_digits() -> Automaton {
        let mut automaton = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 4,
                o: vec![5, 7],
                d: vec![BTreeMap::new(), BTreeMap::new()],
            },
            vec![digits(&[0, 1, 2, 3])],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        let sym = automaton.encode(&[0]);
        automaton.fa.d[0].insert(sym, vec![1]);
        automaton
    }

    /// `set_alphabet`'s `is_dfao = true` branch (`minimize_self_with_output`, `:394-395`)
    /// has no differential-corpus coverage (`tests/differential/CAPTURE.md` documents why:
    /// building a real word automaton via the CLI needs `morphism`/`image`/`combine`, none
    /// ported yet) -- pin it directly here instead, so a regression in this specific branch
    /// still has a test to fail. Restricting `{0,1,2,3}` to `{0,1}` must survive digit 0's
    /// transition (2/3 were never used, so nothing here depends on pruning) and preserve
    /// both states' non-boolean outputs verbatim -- neither would happen if this branch
    /// silently fell through to the boolean `determinize_and_minimize` path instead.
    #[test]
    fn set_alphabet_preserves_dfao_outputs_through_the_is_dfao_branch() {
        let mut a = one_track_word_automaton_over_four_digits();
        set_alphabet(&mut a, true, &[None], vec![vec![0, 1]]).unwrap();

        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        let sym0 = a.encode(&[0]);
        let next = a.fa.d[a.fa.q0]
            .get(&sym0)
            .and_then(|dests| dests.first())
            .copied()
            .expect("digit 0's transition must survive restriction to {0,1}");
        assert_eq!(a.fa.o[a.fa.q0], 5, "start state's output must be preserved");
        assert_eq!(
            a.fa.o[next], 7,
            "digit 0's destination output must be preserved"
        );
    }
}
