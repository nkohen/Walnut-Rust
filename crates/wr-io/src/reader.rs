// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Reader for Walnut's `.txt` automaton format.
//!
//! Ports `Automata/AutomatonReader.java` + the alphabet/state/transition regexes of
//! `Automata/ParseMethods.java` — the parsing half only (see `Automata/Writer/
//! AutomatonWriter.java` for the writer, not yet ported: the Phase-1 spike only needs
//! to read real `walnut-java` output for differential comparison, see
//! `.claude/plans/fluttering-foraging-spindle.md`).
//!
//! # Grammar, verified against `ParseMethods`'s actual regexes (not guessed)
//!
//! - Comments (`^\s*#.*$`) and blank lines are skippable anywhere.
//! - A **trivial** file is just `true` or `false` and nothing else (except
//!   comments/whitespace), and yields the TRUE/FALSE automaton
//!   (`wr_core::automaton::Automaton::true_false`). Supported as of U0; it used to be
//!   a hard `UnsupportedTrivialAutomaton` error because `Automaton` had no such
//!   variant. **13% of Walnut's own golden `automaton*` corpus (85 of 638 fixtures)
//!   consists of exactly this**, so it is a mainline shape, not a curiosity.
//!   Anything other than comments/whitespace after the `true`/`false` line is
//!   [`ReadError::FileHasConflict`], matching `AutomatonReader.firstParse`
//!   (`:146-151`) — the trivial line is NOT merely a header the rest of the file may
//!   extend.
//! - The **header** line declares one token per track: either an explicit set
//!   `{v1, v2, ...}`, or a numeration spec. **This reader supports only `msd_<k>`,
//!   `lsd_<k>`, bare `msd` (= `msd_2`), and bare `lsd` (= `lsd_2`)** — real Walnut
//!   also accepts custom-base names (`msd_fib`, ...) via `NumberSystem.
//!   getComputeIfAbsent`, which constructs a whole `NumberSystem` (adder/comparator
//!   automata and all); out of scope here, so any other token is
//!   [`ReadError::UnsupportedNumeration`], never silently misread.
//! - Then repeated state blocks: `<id> <output>` (first declared block's `id` becomes
//!   `q0`, **not necessarily `0`**), each followed by zero or more transition lines
//!   `<sym1> <sym2> ... -> <dest1> [<dest2> ...]` (one token per track, each a signed
//!   integer or `*`; multiple dest ids, or repeated identical inputs across lines,
//!   both encode NFA nondeterminism via destination accumulation).
//! - **Declared state ids must be exactly the dense range `0..Q`** (some permutation
//!   of it — `FA.setFieldsFromFile` indexes `stateOutput.get(q)` for `q` in `0..Q`
//!   directly, so a real Walnut file satisfies this even though no single regex
//!   enforces it) — checked explicitly here as [`ReadError::NonDenseStateIds`]
//!   rather than inherited as a silent Java `NullPointerException`.
//! - Every destination id used in a transition must have its own state block
//!   ([`ReadError::UndeclaredDestState`]); every transition's input arity must equal
//!   the header's track count ([`ReadError::ArityMismatch`]).
//! - On load, if the parsed transition table is nondeterministic, Walnut
//!   auto-determinizes + minimizes (`AutomatonReader.readAutomaton`, mirrored here);
//!   this reader has no DFAO concept yet (see `wr_core::automaton` docs) so the
//!   corresponding Java "nondeterministic DFAO is a hard error" branch does not
//!   apply — every parsed automaton is a plain predicate automaton.
//!
//! # A deliberate grammar simplification
//!
//! Java's number regexes (`(\+|\-)?\s*\d+`) tolerate whitespace *between* a sign and
//! its digits (`"+  5"`). This reader does not — every real Walnut-*emitted* file
//! (the only files this spike's differential test reads) never inserts such
//! whitespace, so this is a gap only for hand-written files with unusual spacing, not
//! for round-tripping real output.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use wr_core::automaton::Automaton;
use wr_core::determinize::subset_construction;
use wr_core::fa::Fa;
use wr_core::minimize::{minimize, MinimizeError};
use wr_core::trim::trim;

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    /// The file contained nothing but comments/whitespace.
    EmptyFile,
    /// A `true`/`false` trivial file had further non-comment, non-blank content after
    /// the truth-value line. Ports `WalnutException.fileHasConflict`, thrown from
    /// `AutomatonReader.firstParse` (`:146-151`); the payload is the 1-based line
    /// number of the offending line, as in Java's message.
    FileHasConflict(usize),
    /// The header line couldn't be tokenized (unbalanced `{`, non-integer set element).
    MalformedHeader,
    /// A header token isn't `msd_<k>` / `lsd_<k>` / bare `msd` / bare `lsd`.
    UnsupportedNumeration(String),
    /// A line was neither a state declaration, a transition, blank, nor a comment.
    UnexpectedLine(usize),
    /// A transition line appeared before any state was declared.
    TransitionBeforeState(usize),
    /// A transition's input arity didn't match the header's track count.
    ArityMismatch {
        line: usize,
        expected: usize,
        got: usize,
    },
    /// A transition named a destination state with no `<id> <output>` block.
    UndeclaredDestState(usize),
    /// A header line was followed by no state declarations at all (a 0-state `Fa` is
    /// a valid, harmless value everywhere else in this crate — `trim`/`minimize`/
    /// `Fa::is_language_empty` all pass it through — but this reader has no `q0` to
    /// report for it, since Walnut's own file format has no way to declare one; the
    /// closest real Walnut behavior would be a file containing just `false`, which
    /// this reader now reads as the FALSE automaton).
    NoStates,
    /// Declared state ids weren't exactly `0..Q` (see module docs).
    NonDenseStateIds,
    /// Propagated from the auto-determinize-on-load step.
    Minimize(MinimizeError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ReadError {}

impl From<std::io::Error> for ReadError {
    fn from(e: std::io::Error) -> Self {
        ReadError::Io(e)
    }
}

impl From<MinimizeError> for ReadError {
    fn from(e: MinimizeError) -> Self {
        ReadError::Minimize(e)
    }
}

/// Reads a Walnut `.txt` automaton file into an [`Automaton`]. Track labels default to
/// placeholders (`"0"`, `"1"`, ...) — the file format itself carries no variable
/// names, matching Java (labels are a `Prover`/query-binding concept, not part of the
/// `.txt` grammar); relabel via `automaton.label` after loading if needed.
pub fn read_automaton_txt<P: AsRef<Path>>(path: P) -> Result<Automaton, ReadError> {
    let content = std::fs::read_to_string(path)?;

    let mut lines = content.lines().enumerate();
    let (_, header_line) = lines
        .by_ref()
        .find(|(_, l)| !should_skip(l))
        .ok_or(ReadError::EmptyFile)?;

    // `AutomatonReader.firstParse`'s trivial branch (`:141-153`): the `true`/`false`
    // test runs BEFORE the alphabet-declaration parse, and once it matches nothing but
    // comments/whitespace may follow.
    if let Some(truth) = parse_true_false(header_line) {
        for (i, raw_line) in lines {
            if !should_skip(raw_line) {
                return Err(ReadError::FileHasConflict(i + 1));
            }
        }
        // Java's result additionally carries `alphabetSize == 1` here, from the
        // unconditional `A.setAlphabetSize(1)` at `AutomatonReader.readAutomaton:23`
        // that runs before parsing; `Automaton::true_false` leaves it `0`. Not
        // replicated because nothing may read a trivial automaton's `alphabet_size`
        // (see `wr_core::fa`'s module docs) — noting it rather than leaving it silent.
        return Ok(Automaton::true_false(truth));
    }

    let trimmed_header = header_line.trim();
    let (alphabet, msd) = parse_header(trimmed_header)?;
    let num_tracks = alphabet.len();
    let alphabet_size: usize = alphabet.iter().map(|t| t.len()).product();
    let label: Vec<String> = (0..num_tracks).map(|i| i.to_string()).collect();

    // Placeholder `Fa`, replaced once every line is parsed — lets us reuse
    // `Automaton::encode` (mixed-radix, position-in-alphabet indexed) instead of
    // duplicating that formula here.
    let mut automaton = Automaton::new(
        Fa {
            true_false: None,
            q0: 0,
            q: 0,
            alphabet_size,
            o: vec![],
            d: vec![],
        },
        alphabet.clone(),
        label,
        msd,
    );

    let mut output: BTreeMap<usize, i32> = BTreeMap::new();
    let mut transitions: BTreeMap<usize, BTreeMap<i32, Vec<usize>>> = BTreeMap::new();
    let mut declaration_order: Vec<usize> = Vec::new();
    let mut current_state: Option<usize> = None;
    let mut dest_states_used: BTreeSet<usize> = BTreeSet::new();

    for (i, raw_line) in lines {
        let lineno = i + 1;
        if should_skip(raw_line) {
            continue;
        }

        if let Some((id, out)) = try_parse_state_decl(raw_line) {
            output.insert(id, out);
            transitions.entry(id).or_default();
            declaration_order.push(id);
            current_state = Some(id);
        } else if let Some((input_tokens, dests)) = try_parse_transition(raw_line) {
            let cur = current_state.ok_or(ReadError::TransitionBeforeState(lineno))?;
            if input_tokens.len() != num_tracks {
                return Err(ReadError::ArityMismatch {
                    line: lineno,
                    expected: num_tracks,
                    got: input_tokens.len(),
                });
            }
            dest_states_used.extend(dests.iter().copied());
            for digits in expand_wildcards(&input_tokens, &alphabet) {
                let sym = automaton.encode(&digits);
                transitions
                    .get_mut(&cur)
                    .expect("current_state always has a transitions entry")
                    .entry(sym)
                    .or_default()
                    .extend(dests.iter().copied());
            }
        } else {
            return Err(ReadError::UnexpectedLine(lineno));
        }
    }

    for &d in &dest_states_used {
        if !output.contains_key(&d) {
            return Err(ReadError::UndeclaredDestState(d));
        }
    }

    let q = declaration_order.len();
    if q == 0 {
        // A header with no state blocks at all — vacuously "dense" (both sides of
        // the check below are empty), but there is no q0 to report. Distinct from
        // NonDenseStateIds: this is a real Walnut file shape (a degenerate but
        // syntactically valid header-only file), not a corrupt one.
        return Err(ReadError::NoStates);
    }
    if output.len() != q || (0..q).any(|i| !output.contains_key(&i)) {
        return Err(ReadError::NonDenseStateIds);
    }
    let q0 = declaration_order[0];

    let mut o = vec![0i32; q];
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); q];
    for (id, out) in output {
        o[id] = out;
    }
    for (id, row) in transitions {
        d[id] = row;
    }
    automaton.fa = Fa {
        true_false: None,
        q0,
        q,
        alphabet_size,
        o,
        d,
    };

    // `AutomatonReader.readAutomaton`: auto-determinize + minimize non-deterministic
    // input (no DFAO branch here, see module docs).
    if !automaton.fa.is_deterministic() {
        let trimmed = trim(&automaton.fa);
        let initial: BTreeSet<usize> = [trimmed.q0].into_iter().collect();
        automaton.fa = minimize(&subset_construction(&trimmed, &initial))?;
    }

    Ok(automaton)
}

fn should_skip(line: &str) -> bool {
    let t = line.trim_start();
    t.is_empty() || t.starts_with('#')
}

/// `ParseMethods.parseTrueFalse(String, Boolean[])` (`ParseMethods.java:74-81`) against
/// `PATTERN_FOR_TRUE_FALSE = ^\s*(true|false)\s*$` (`:43`) — returns the parsed truth
/// value, or `None` when the line isn't a bare `true`/`false`.
///
/// Implemented inline here rather than as part of a `ParseMethods` port on purpose:
/// **`ParseMethods.java` as a whole is a separate, independently-landing unit (U0b),
/// which will eventually own all of this file's `.txt` grammar.** Keeping this as one
/// small private helper with the same name/semantics as the Java method means that
/// refactor is a mechanical "delete this and call `ParseMethods`" step with no merge
/// conflict against U0's other changes.
///
/// Regex-free by design: the pattern is anchored at both ends with only `\s*` padding,
/// so `str::trim` + equality is *exactly* equivalent — Java's `\s` and Rust's
/// `char::is_whitespace` differ on a handful of exotic code points, but the pattern
/// permits only whitespace there in either reading, so no input can be classified
/// differently. (Java uses `Matcher.find()`, not `matches()`, which is likewise
/// equivalent here because the pattern is `^...$`-anchored.)
fn parse_true_false(line: &str) -> Option<bool> {
    match line.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

enum HeaderToken {
    Set(Vec<i32>),
    Ns { msd: bool, base: i32 },
}

/// Per-track alphabet, and per-track msd/lsd (`None` for an explicit-set track).
type HeaderSpec = (Vec<Vec<i32>>, Vec<Option<bool>>);

fn parse_header(line: &str) -> Result<HeaderSpec, ReadError> {
    let mut alphabet = Vec::new();
    let mut msd = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let token = if let Some(after_brace) = rest.strip_prefix('{') {
            let end = after_brace.find('}').ok_or(ReadError::MalformedHeader)?;
            let inner = &after_brace[..end];
            let mut values = Vec::new();
            for part in inner.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    return Err(ReadError::MalformedHeader);
                }
                values.push(
                    part.parse::<i32>()
                        .map_err(|_| ReadError::MalformedHeader)?,
                );
            }
            rest = &after_brace[end + 1..];
            HeaderToken::Set(values)
        } else {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '{')
                .unwrap_or(rest.len());
            let word = &rest[..end];
            rest = &rest[end..];
            parse_ns_token(word)?
        };
        match token {
            HeaderToken::Set(values) => {
                alphabet.push(values);
                msd.push(None);
            }
            HeaderToken::Ns { msd: is_msd, base } => {
                alphabet.push((0..base).collect());
                msd.push(Some(is_msd));
            }
        }
    }
    if alphabet.is_empty() {
        return Err(ReadError::MalformedHeader);
    }
    Ok((alphabet, msd))
}

fn parse_ns_token(word: &str) -> Result<HeaderToken, ReadError> {
    if let Some(rest) = word.strip_prefix("msd_") {
        return rest
            .parse::<i32>()
            .map(|base| HeaderToken::Ns { msd: true, base })
            .map_err(|_| ReadError::UnsupportedNumeration(word.to_string()));
    }
    if let Some(rest) = word.strip_prefix("lsd_") {
        return rest
            .parse::<i32>()
            .map(|base| HeaderToken::Ns { msd: false, base })
            .map_err(|_| ReadError::UnsupportedNumeration(word.to_string()));
    }
    match word {
        "msd" => Ok(HeaderToken::Ns { msd: true, base: 2 }),
        "lsd" => Ok(HeaderToken::Ns {
            msd: false,
            base: 2,
        }),
        _ => Err(ReadError::UnsupportedNumeration(word.to_string())),
    }
}

/// `<id> <output>`, both integers, output optionally signed, nothing else on the
/// line — `ParseMethods.PATTERN_FOR_STATE_DECLARATION`.
fn try_parse_state_decl(line: &str) -> Option<(usize, i32)> {
    let mut parts = line.split_whitespace();
    let id_tok = parts.next()?;
    let out_tok = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let id: usize = id_tok.parse().ok()?;
    let out: i32 = out_tok.parse().ok()?;
    Some((id, out))
}

/// `<sym-or-*> ... -> <dest> ...` — `ParseMethods.PATTERN_FOR_TRANSITION`.
fn try_parse_transition(line: &str) -> Option<(Vec<Option<i32>>, Vec<usize>)> {
    let (lhs, rhs) = line.trim().split_once("->")?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }
    let mut input = Vec::new();
    for tok in lhs.split_whitespace() {
        if tok == "*" {
            input.push(None);
        } else {
            input.push(Some(tok.parse::<i32>().ok()?));
        }
    }
    let mut dest = Vec::new();
    for tok in rhs.split_whitespace() {
        dest.push(tok.parse::<usize>().ok()?);
    }
    if dest.is_empty() {
        return None;
    }
    Some((input, dest))
}

/// `RichAlphabet.expandWildcard`: cross-product-expands every `None` (`*`) position
/// against its own track's alphabet, one wildcard position at a time.
fn expand_wildcards(input: &[Option<i32>], alphabet: &[Vec<i32>]) -> Vec<Vec<i32>> {
    let mut results: Vec<Vec<i32>> = vec![input
        .iter()
        .map(|d| d.unwrap_or_default())
        .collect::<Vec<i32>>()];
    for (i, digit) in input.iter().enumerate() {
        if digit.is_some() {
            continue;
        }
        let mut expanded = Vec::with_capacity(results.len() * alphabet[i].len());
        for partial in &results {
            for &v in &alphabet[i] {
                let mut next = partial.clone();
                next[i] = v;
                expanded.push(next);
            }
        }
        results = expanded;
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn reads_automaton572_explicit_set_alphabet() {
        // {0, 1}; state 0 (q0, accepting) self-loops on both symbols to state 0.
        let a = read_automaton_txt(fixture("automaton572.txt")).unwrap();
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.msd, vec![None]);
        assert_eq!(a.fa.q, 1);
        assert!(a.fa.is_accepting(a.fa.q0));
        assert!(a
            .fa
            .accepts_word(&[a.encode(&[0]), a.encode(&[1]), a.encode(&[0])]));
    }

    #[test]
    fn reads_automaton2_msd3_four_track() {
        let a = read_automaton_txt(fixture("automaton2.txt")).unwrap();
        assert_eq!(a.alphabet, vec![vec![0, 1, 2]; 4]);
        assert_eq!(a.msd, vec![Some(true); 4]);
        assert_eq!(a.fa.q, 4);
        assert!(a.fa.is_deterministic());
        // Accepts exactly the one 3-transition path the file spells out:
        // state0 --(0,0,0,1)--> state1 --(1,1,2,2)--> state2 --(1,2,0,2)--> state3 (accept).
        let word = [
            a.encode(&[0, 0, 0, 1]),
            a.encode(&[1, 1, 2, 2]),
            a.encode(&[1, 2, 0, 2]),
        ];
        assert!(a.fa.accepts_word(&word));
        // The self-loop on state0 alone never reaches an accepting state.
        let wrong = [a.encode(&[0, 0, 0, 0])];
        assert!(!a.fa.accepts_word(&wrong));
    }

    #[test]
    fn bare_msd_defaults_to_base_2() {
        let (alphabet, msd) = parse_header("msd").unwrap();
        assert_eq!(alphabet, vec![vec![0, 1]]);
        assert_eq!(msd, vec![Some(true)]);
    }

    #[test]
    fn bare_lsd_defaults_to_base_2() {
        let (alphabet, msd) = parse_header("lsd").unwrap();
        assert_eq!(alphabet, vec![vec![0, 1]]);
        assert_eq!(msd, vec![Some(false)]);
    }

    #[test]
    fn msd_k_and_lsd_k_parse_explicit_bases() {
        let (alphabet, msd) = parse_header("msd_5 lsd_3").unwrap();
        assert_eq!(alphabet, vec![vec![0, 1, 2, 3, 4], vec![0, 1, 2]]);
        assert_eq!(msd, vec![Some(true), Some(false)]);
    }

    #[test]
    fn unsupported_numeration_is_explicit_not_silent() {
        assert!(matches!(
            parse_header("msd_fib"),
            Err(ReadError::UnsupportedNumeration(_))
        ));
        assert!(matches!(
            parse_header("msd5"), // no-underscore form — deliberately unsupported
            Err(ReadError::UnsupportedNumeration(_))
        ));
    }

    // --- trivial (TRUE/FALSE) automaton files (U0) ---
    //
    // Before U0 this shape was a hard `UnsupportedTrivialAutomaton` error; the test that
    // pinned that (`trivial_automaton_is_explicit_not_silent`) is retained below in
    // updated form, now asserting the real read, since the behavior it pinned was a
    // documented scope cut that this unit deliberately closes.

    #[test]
    fn reads_the_real_golden_true_fixture() {
        // `automaton189.txt` — the literal word `true`, no trailing newline, copied
        // byte-for-byte from walnut-java's corpus.
        let a = read_automaton_txt(fixture("automaton189.txt")).unwrap();
        assert!(a.is_true_false_automaton());
        assert!(a.is_true_automaton());
        assert!(!a.is_empty(), "the TRUE automaton's language is not empty");
        assert_eq!(a.get_arity(), 0);
        assert!(a.alphabet.is_empty());
        assert!(a.label.is_empty());
        assert!(a.msd.is_empty());
    }

    #[test]
    fn reads_the_real_golden_false_fixture() {
        let a = read_automaton_txt(fixture("automaton214.txt")).unwrap();
        assert!(a.is_true_false_automaton());
        assert!(!a.is_true_automaton());
        assert!(a.is_empty(), "the FALSE automaton's language is empty");
        assert_eq!(a.get_arity(), 0);
    }

    #[test]
    fn trivial_automaton_is_explicit_not_silent() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Trailing newline, leading/trailing whitespace and comment lines are all
        // tolerated -- `PATTERN_FOR_TRUE_FALSE` is `^\s*(true|false)\s*$` and
        // `firstParse` skips comments/blanks both before and after the match.
        let path = dir.join("true.txt");
        std::fs::write(&path, "# a comment\n\n   true  \n\n# trailing comment\n").unwrap();
        let a = read_automaton_txt(&path).unwrap();
        assert!(a.is_true_false_automaton() && a.is_true_automaton());

        let path = dir.join("false.txt");
        std::fs::write(&path, "false\n").unwrap();
        let a = read_automaton_txt(&path).unwrap();
        assert!(a.is_true_false_automaton() && !a.is_true_automaton());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn content_after_a_trivial_line_is_a_conflict_not_a_silent_ignore() {
        // `AutomatonReader.firstParse:146-151` — `WalnutException.fileHasConflict`.
        let dir = std::env::temp_dir().join(format!("wr-io-test-conflict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("conflict.txt");
        std::fs::write(&path, "true\n{0, 1}\n\n0 1\n").unwrap();
        assert!(matches!(
            read_automaton_txt(&path),
            // Line 2 (1-based) is the first offending line.
            Err(ReadError::FileHasConflict(2))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_true_like_word_that_is_not_a_bare_truth_value_is_not_trivial() {
        // Guards against a sloppier `starts_with`/`contains` implementation of
        // `PATTERN_FOR_TRUE_FALSE`: `true_2` is a header token, not a truth value, and
        // must fall through to the (rejecting) numeration parser.
        assert_eq!(parse_true_false("true"), Some(true));
        assert_eq!(parse_true_false("  false\t"), Some(false));
        assert_eq!(parse_true_false("true_2"), None);
        assert_eq!(parse_true_false("truefalse"), None);
        assert_eq!(parse_true_false("true false"), None);
        assert_eq!(parse_true_false("TRUE"), None);
        assert_eq!(parse_true_false("{0, 1}"), None);
    }

    #[test]
    fn undeclared_destination_state_is_an_error() {
        let dir =
            std::env::temp_dir().join(format!("wr-io-test-undeclared-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.txt");
        std::fs::write(&path, "{0, 1}\n\n0 0\n0 -> 5\n1 -> 0\n").unwrap();
        assert!(matches!(
            read_automaton_txt(&path),
            Err(ReadError::UndeclaredDestState(5))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn arity_mismatch_is_an_error() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-arity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.txt");
        std::fs::write(&path, "msd_2 msd_2\n\n0 0\n0 -> 0\n").unwrap();
        assert!(matches!(
            read_automaton_txt(&path),
            Err(ReadError::ArityMismatch {
                expected: 2,
                got: 1,
                ..
            })
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wildcard_expands_to_every_track_value() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-wildcard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wc.txt");
        // `*` on the only track from state 0 must reach state 1 on EVERY symbol.
        std::fs::write(&path, "msd_3\n\n0 0\n* -> 1\n\n1 1\n").unwrap();
        let a = read_automaton_txt(&path).unwrap();
        for digit in 0..3 {
            assert!(a.fa.accepts_word(&[a.encode(&[digit])]));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nondeterministic_input_is_auto_determinized() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-nfa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nfa.txt");
        // Two destinations for symbol 1 from state 0 — genuine nondeterminism.
        std::fs::write(
            &path,
            "msd_2\n\n0 0\n0 -> 0\n1 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();
        let a = read_automaton_txt(&path).unwrap();
        assert!(a.fa.is_deterministic());
        // Recognizes "contains a 1", same language as the hand-written NFA.
        assert!(!a.fa.accepts_word(&[a.encode(&[0]), a.encode(&[0])]));
        assert!(a
            .fa
            .accepts_word(&[a.encode(&[0]), a.encode(&[1]), a.encode(&[0])]));
    }

    #[test]
    fn non_dense_state_ids_are_rejected() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-dense-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gap.txt");
        // States 0 and 2 declared, but not 1 — not a dense 0..Q range.
        std::fs::write(
            &path,
            "msd_2\n\n0 0\n0 -> 2\n1 -> 2\n\n2 1\n0 -> 2\n1 -> 2\n",
        )
        .unwrap();
        assert!(matches!(
            read_automaton_txt(&path),
            Err(ReadError::NonDenseStateIds)
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn header_only_file_is_an_error_not_a_panic() {
        // Regression test for a reviewer-found panic: a header with no state blocks
        // at all used to index declaration_order[0] on an empty Vec.
        let dir = std::env::temp_dir().join(format!("wr-io-test-nostates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("header_only.txt");
        std::fs::write(&path, "msd_2\n").unwrap();
        assert!(matches!(
            read_automaton_txt(&path),
            Err(ReadError::NoStates)
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
}
