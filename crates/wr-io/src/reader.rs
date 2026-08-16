// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Reader for Walnut's `.txt` automaton format.
//!
//! Ports `Automata/AutomatonReader.java` + the alphabet/state/transition regexes of
//! `Automata/ParseMethods.java` — the parsing half only (see `Automata/Writer/
//! AutomatonWriter.java` for the writer, `wr-io`'s `writer` module, Phase 3a's U12).
//!
//! # U13 additions: custom-base headers, `readTransducer`/`readComments`, `AutomatonDFA(String)`
//!
//! Three things `AutomatonReader.java` also owns, folded into this unit
//! (`.claude/plans/synthetic-prancing-aurora.md`):
//!
//! - [`read_automaton_txt_with_custom_bases`] — the `msd_fib`-style header support
//!   [`read_automaton_txt`] itself still declines (see the header bullet below).
//! - [`read_transducer_txt`]/[`read_comments`] — `AutomatonReader.readTransducer`/
//!   `readComments` (`:196-294`). `readTransducer`'s DFST parsing logic belongs here
//!   (it is pure `.txt`-format I/O), even though nothing consumes its output yet — the
//!   `Transducer` automaton type itself, and `transduce`, are Phase 3b's U20/U26.
//! - [`read_automaton_dfa_txt`] — `AutomatonDFA(String address)` (`AutomatonDFA.java:27-32`).
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
//!   `{v1, v2, ...}`, or a numeration spec. Plain `msd_<k>`/`lsd_<k>`/bare `msd`
//!   (= `msd_2`)/bare `lsd` (= `lsd_2`) are always supported. **As of U13, custom-base
//!   names (`msd_fib`, ...) are supported too, but only via
//!   [`read_automaton_txt_with_custom_bases`]** (or, inside a session, its
//!   [`CustomBaseResolver`]-injecting twin
//!   [`read_automaton_txt_with_custom_base_resolver`], which is what lets `wr-cli` apply
//!   Java's per-file session-overrides-global precedence to a NESTED header token, exactly
//!   as Java does) — plain [`read_automaton_txt`] has no way to resolve `Custom Bases/*.txt`
//!   at all (this crate has no `Session` concept and deliberately never will; that's
//!   `wr-cli`'s Phase-3a U14, so the caller supplies the directory or the resolver) and
//!   still rejects them with [`ReadError::UnsupportedNumeration`]. See
//!   [`read_automaton_txt_with_custom_bases`]'s own doc for the exact file-resolution
//!   order (`wr_core::numsys::NumberSystem::with_custom_base_files`, Phase 3a's U5, owns
//!   the *decision* logic; this module supplies the actual file reads). Any OTHER
//!   unrecognized token is still [`ReadError::UnsupportedNumeration`], never silently
//!   misread.
//!
//!   Java stores the resolved `NumberSystem` objects themselves in `A.getNS()`; this
//!   crate's `Automaton` keeps a decomposed stand-in, and the reader populates two of its
//!   three parts — the msd/lsd direction (`Automaton::msd`) and, as of U23's review fixes,
//!   the number system's NAME (`Automaton::ns_name`, load-bearing because
//!   `NumberSystem.isNSDiffering` compares by name and `msd_fib` is otherwise
//!   indistinguishable from `msd_2`) — and, as of U27, the third: `Automaton::all_reps`,
//!   the custom base's valid-representation restriction (`msd_fib.txt`'s "no `11`
//!   substring"). That third one is load-bearing, not bookkeeping:
//!   `Automaton::apply_all_representations` — which `not`, `=>` and the `A` quantifier all
//!   run — reads exactly `all_reps`, so while it was empty, complementing an `msd_fib`
//!   automaton loaded from a library file admitted words that are not valid Zeckendorf
//!   representations at all. The Tier-1 golden corpus caught it (12 fixtures, 352-371); see
//!   `a_custom_base_header_carries_its_valid_representation_restriction`.
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
//! # One grammar, not two (Phase 4, U30 review round 2)
//!
//! Both readers now tokenize state declarations and transition lines through
//! [`crate::parse_methods`] — the verbatim ports of `ParseMethods`' own regexes,
//! including their deferred-`parseInt` discipline. This file used to carry a second,
//! hand-rolled `split_whitespace` grammar for the *automaton* reader only (the
//! transducer reader always used `parse_methods`), which was a Phase-1 leftover and
//! disagreed with Java in both directions: it accepted a `+`-signed state id Java's
//! `\d+` rejects, rejected the sign/digit spacing (`"0 + 1"`) Java's `(\+|\-)?\s*\d+`
//! accepts, and — the reason it was found — silently dropped an `i32`-overflowing state
//! or destination id into the wrong error class instead of Java's
//! `NumberFormatException`, while typing ids as `usize` so a value too large for Java's
//! `int` "parsed" fine.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use wr_core::automaton::{Automaton, AutomatonDFA};
use wr_core::fa::Fa;
use wr_core::minimize::{minimize, MinimizeError};
use wr_core::numsys::{self, CustomBaseCandidates, CustomBaseFiles, NumSysError, NumberSystem};
use wr_core::trim::trim;
use wr_core::util::is_number;

use crate::parse_methods::{self, ParseMethodsError};

/// Every way reading a `.txt` automaton/transducer can fail.
///
/// # Message fidelity
///
/// Tier-1 `error*` fixtures compare rendered text, so [`fmt::Display`] reproduces the
/// real `WalnutException` message **verbatim** wherever this reader's failure corresponds
/// to a Java throw site whose inputs it actually has (the whole per-line family below,
/// plus the delegating [`Self::NumSys`]/[`Self::ParseMethods`]/[`Self::Io`] arms). Until
/// this round the impl was a blanket `write!(f, "{self:?}")` — a Debug dump — so a user
/// reading a library file headed `msd_1` was shown `NumSys(NotDefined("msd_1"))` instead
/// of `Number system msd_1 is not defined.`.
///
/// **Still port-specific text, deliberately** (each has no Java counterpart with the
/// information needed, and closing it means porting more than this file):
/// [`Self::MalformedHeader`] (Java's `WalnutException.undefinedStatement(lineNumber,
/// address)`, thrown from `firstParse` — this reader's `parse_header` is handed the
/// header *text* alone, with neither the line number nor the address in scope),
/// [`Self::UnsupportedNumeration`] (renders `NumberSystem`'s real "is not defined."
/// text, which is what Java answers for the `msd_fib`-without-a-resolver case this
/// variant is normally raised for — but NOT for a name with no `_` at all, e.g. `msd5`,
/// where Java instead throws `StringIndexOutOfBoundsException` from
/// `determineMsdOrLsd`), [`Self::NoStates`]/[`Self::NonDenseStateIds`]
/// (shapes real Walnut has no check for at all — it fails later with a
/// `NullPointerException`/`IndexOutOfBoundsException`, see this module's docs) and
/// [`Self::CustomBaseCycle`] (a guard this port added; Java stack-overflows).
///
/// One layer above, `wr_cli::session::read_library_automaton` still wraps whatever this
/// renders in `PredicateEnvError::MalformedAutomaton`'s `"File does not parse: {address}
/// ({detail})"`, which Java does not do — a separate, already-documented gap on that
/// type, not this one.
#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    /// `WalnutException.fileEmpty` (`WalnutException.java:52-54`), thrown from
    /// `AutomatonReader.firstParse`'s `if (!sawHeader)` (`:176-178`): the file contained
    /// nothing but comments/whitespace.
    EmptyFile {
        address: String,
    },
    /// A `true`/`false` trivial file had further non-comment, non-blank content after
    /// the truth-value line. Ports `WalnutException.fileHasConflict`, thrown from
    /// `AutomatonReader.firstParse` (`:146-151`); `line` is the 1-based line number of
    /// the offending line, as in Java's message.
    FileHasConflict {
        line: usize,
        address: String,
    },
    /// The header line couldn't be tokenized (unbalanced `{`, non-integer set element).
    MalformedHeader,
    /// A header token isn't `msd_<k>` / `lsd_<k>` / bare `msd` / bare `lsd`.
    UnsupportedNumeration(String),
    /// A line was neither a state declaration, a transition, blank, nor a comment —
    /// `WalnutException.undefinedStatement` (`WalnutException.java:124-126`), thrown from
    /// `AutomatonReader.readAutomaton` (`:77`)/`readTransducer` (`:259`).
    UnexpectedLine {
        line: usize,
        address: String,
    },
    /// A transition line appeared before any state was declared —
    /// `AutomatonReader.validateTransition`'s first throw (`:116-120`).
    TransitionBeforeState {
        line: usize,
        address: String,
    },
    /// A transition's input arity didn't match the header's track count —
    /// `AutomatonReader.validateTransition`'s second throw (`:123-125`). `got` is kept
    /// for programmatic inspection but is deliberately NOT printed: Java's message names
    /// only the required arity.
    ArityMismatch {
        line: usize,
        expected: usize,
        got: usize,
        address: String,
    },
    /// A transition named a destination state with no `<id> <output>` block —
    /// `AutomatonReader.validateDeclaredStates` (`:189-193`). `state` is `i32`, matching
    /// the `int` Java parses it as (see [`parse_methods::parse_transition`]).
    ///
    /// **Newly reachable** as of U30's F2 fix on a file real Walnut accepts: an
    /// out-of-alphabet body digit is encoded to the bogus key `-1` rather than rejected
    /// (WB-038), so a file whose only offense is that digit now runs the same
    /// declared-state validation Java runs, and reports through here.
    UndeclaredDestState {
        state: i32,
        address: String,
    },
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
    /// Propagated from custom-base [`NumberSystem`] construction (U13): a header token
    /// named a syntactically-plausible custom base whose files failed to resolve into a
    /// valid number system (missing files falling back to a non-numeric name, a
    /// malformed name, a `_neg_` name, or a structurally invalid loaded automaton — see
    /// [`NumSysError`]). Only reachable through
    /// [`read_automaton_txt_with_custom_bases`]/[`read_automaton_dfa_txt_with_custom_bases`];
    /// plain [`read_automaton_txt`] never attempts a custom-base load at all, so it can
    /// never produce this variant.
    NumSys(NumSysError),
    /// A custom-base name was re-entered while its own resolution was still in progress —
    /// e.g. `Custom Bases/msd_fib_addition.txt`'s own header declares `msd_fib` again (or,
    /// transitively, some other custom base whose own chain leads back to `msd_fib`). Guards
    /// [`load_custom_base`]'s recursion into [`read_automaton_txt_impl`] against unbounded
    /// (stack-overflowing) recursion on a malformed self-referential custom-base file; see
    /// that function's own doc for why this is a real, distinct-from-WB-014 gap this port
    /// needed to close. The payload is the re-entered name.
    CustomBaseCycle(String),
    /// Propagated from [`parse_methods`]: a `.txt` line matched its pattern but one of
    /// its `\d+` groups overflows `i32` (a state id, a transition digit, a destination
    /// id, an output value). Java's `UtilityMethods.parseInt` throws an unchecked
    /// `NumberFormatException` there, which `Prover.readBuffer`'s `catch
    /// (RuntimeException)` (`Prover.java:390-392`) recovers from, so the read has to be
    /// a recoverable failure rather than the process-fatal panic this port used to
    /// raise (Phase 4 U30's fuzz finding F1, same root cause, reached here through the
    /// transducer reader).
    ParseMethods(ParseMethodsError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // `AutomatonReader`'s own `catch (IOException e)` turns this into
            // `WalnutException.fileDoesNotExist`, but only at the CALLER's level
            // (`wr_cli::session::read_library_automaton` ports exactly that mapping), so
            // this arm renders the underlying I/O message, as everywhere else in this
            // workspace.
            ReadError::Io(e) => write!(f, "{e}"),
            // Verbatim `WalnutException.java:53`.
            ReadError::EmptyFile { address } => write!(
                f,
                "File is empty or contains only comments/whitespace: {address}"
            ),
            // Verbatim `WalnutException.java:56-57`.
            ReadError::FileHasConflict { line, address } => write!(
                f,
                "A file that declares 'true'/'false' must not contain other statements: \
                 line {line} of file {address}"
            ),
            // Port-specific (see this type's docs): Java's `undefinedStatement` needs the
            // line number and address, neither of which `parse_header` is given.
            ReadError::MalformedHeader => write!(f, "Malformed alphabet declaration."),
            ReadError::UnsupportedNumeration(name) => {
                write!(f, "Number system {name} is not defined.")
            }
            // Verbatim `WalnutException.java:125`.
            ReadError::UnexpectedLine { line, address } => {
                write!(f, "Undefined statement: line at {line} of file {address}")
            }
            // Verbatim `AutomatonReader.java:118-120`.
            ReadError::TransitionBeforeState { line, address } => write!(
                f,
                "Must declare a state before declaring a list of transitions: \
                 line {line} of file {address}"
            ),
            // Verbatim `AutomatonReader.java:124-125`.
            ReadError::ArityMismatch {
                line,
                expected,
                got: _,
                address,
            } => write!(
                f,
                "This automaton requires a {expected}-tuple as input: line {line} of file {address}"
            ),
            // Verbatim `AutomatonReader.java:191`.
            ReadError::UndeclaredDestState { state, address } => write!(
                f,
                "State {state} is used but never declared anywhere in file: {address}"
            ),
            // Port-specific (see this type's docs); Java has no check for either shape.
            ReadError::NoStates => write!(f, "The automaton declares no states."),
            ReadError::NonDenseStateIds => {
                write!(f, "The declared state ids must be exactly 0..Q.")
            }
            // `MinimizeError` has no `Display` (it is this port's own
            // internal-invariant surface, not a Java throw site); Debug is what there is
            // to show, and it is named as such rather than pretending otherwise.
            ReadError::Minimize(e) => write!(f, "Minimization failed: {e:?}"),
            // `NumSysError`/`ParseMethodsError` already render Walnut's verbatim text
            // (`Number system msd_1 is not defined.`, `For input string: "…"`), and Java
            // does not wrap either — the exception thrown inside `NumberSystem`'s
            // constructor / `UtilityMethods.parseInt` propagates out of the reader
            // unchanged — so these are plain pass-throughs.
            ReadError::NumSys(e) => write!(f, "{e}"),
            ReadError::ParseMethods(e) => write!(f, "{e}"),
            // Port-specific: this port's own recursion guard (Java stack-overflows).
            ReadError::CustomBaseCycle(name) => {
                write!(f, "Custom base {name} is defined in terms of itself.",)
            }
        }
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

impl From<ParseMethodsError> for ReadError {
    fn from(e: ParseMethodsError) -> Self {
        ReadError::ParseMethods(e)
    }
}

impl From<NumSysError> for ReadError {
    fn from(e: NumSysError) -> Self {
        ReadError::NumSys(e)
    }
}

/// Resolves one `Custom Bases/` file **name** (`"msd_fib_addition.txt"`) to the address this
/// reader should try to open — the injected half of custom-base header resolution.
///
/// Java needs no such abstraction: `NumberSystem`'s constructor reaches the static
/// `Session.getReadAddressForCustomBases(fileName)` directly (`NumberSystem.java:299-319`
/// → `Session.java:164-166` → `Session.globalOrSessionFile`), so its resolution is
/// hard-wired to the ambient session and is therefore **session-aware for nested library
/// headers exactly as much as it is for a top-level `?msd_fib` query token — there is no
/// split between the two in Java.**
///
/// This crate deliberately has no `Session` concept of its own (that is `wr-cli`'s
/// `session` module, Phase 3a's U14, and `wr-io` must not grow a second copy of it), so the
/// *policy* — which directory, and whether a session copy shadows the global one — is
/// injected through this trait, while the *file I/O* stays here. `wr-cli` implements it
/// with `SessionPaths::read_address_for_custom_bases`, i.e. real `globalOrSessionFile`
/// precedence; standalone callers use [`CustomBasesDir`].
///
/// The address is returned whether or not it exists (Java's `globalOrSessionFile` likewise
/// returns a non-existent global address as its fallback); the caller stats it.
pub trait CustomBaseResolver {
    /// `filename` is a bare file name, never a path — the reader composes it from the
    /// custom-base name and one of `_addition.txt`/`_less_than.txt`/`.txt`
    /// (`wr_core::numsys::custom_base_candidate_names`).
    fn resolve(&self, filename: &str) -> PathBuf;
}

/// The degenerate [`CustomBaseResolver`]: every custom-base file is looked up in one fixed
/// directory, with no session override. What [`read_automaton_txt_with_custom_bases`] uses,
/// and what a caller outside a session (this crate's own tests, a one-off tool) wants.
pub struct CustomBasesDir<'a>(pub &'a Path);

impl CustomBaseResolver for CustomBasesDir<'_> {
    fn resolve(&self, filename: &str) -> PathBuf {
        self.0.join(filename)
    }
}

/// Reads a Walnut `.txt` automaton file into an [`Automaton`]. Track labels default to
/// placeholders (`"0"`, `"1"`, ...) — the file format itself carries no variable
/// names, matching Java (labels are a `Prover`/query-binding concept, not part of the
/// `.txt` grammar); relabel via `automaton.label` after loading if needed.
///
/// A custom-base header (`msd_fib`, ...) is [`ReadError::UnsupportedNumeration`] here —
/// use [`read_automaton_txt_with_custom_bases`] to resolve those.
pub fn read_automaton_txt<P: AsRef<Path>>(path: P) -> Result<Automaton, ReadError> {
    read_automaton_txt_impl(path.as_ref(), None, &mut BTreeSet::new())
}

/// Like [`read_automaton_txt`], but a header token that isn't `msd_<k>`/`lsd_<k>`/bare
/// `msd`/bare `lsd` is resolved as a **custom-base** numeration name (`msd_fib`, ...)
/// against `custom_bases_dir`, instead of failing with
/// [`ReadError::UnsupportedNumeration`].
///
/// # File resolution, matching `NumberSystem`'s constructor exactly (`NumberSystem.java:132-163`)
///
/// For each of the three probes a `NumberSystem` constructor makes — the adder
/// (`<name>_addition.txt`), the comparator (`<name>_less_than.txt`), and the "set of all
/// representations" restriction (`<name>.txt`) — in that order:
///
/// 1. If `custom_bases_dir/<name><extension>` exists, it is read (recursively, through
///    this same function, with the same `custom_bases_dir`) and used AS-IS — never
///    reversed, even for an lsd system.
/// 2. Otherwise, if `custom_bases_dir/<opposite>_<base><extension>` exists (the SAME base
///    under the OPPOSITE msd/lsd direction — Java's "when the number system does not
///    exist, we try its complement", `NumberSystem.java:305-306`), it is read and its
///    language **reversed** (declared direction left alone).
/// 3. Otherwise the probe is absent, and [`NumberSystem::with_custom_base_files`] falls
///    back to Java's usual "no file → error" behavior for that piece (`NotDefined` for a
///    missing adder; a missing comparator/all-representations file just means "build it
///    programmatically" / "no restriction").
///
/// File-name resolution and the precedence/fallback logic above are
/// [`wr_core::numsys::custom_base_candidate_names`]/[`CustomBaseCandidates::resolve`]
/// (Phase 3a's U5) — this function supplies only the actual `Path::is_file`/file-read
/// steps, matching `wr-core`'s "no file I/O" boundary (see that module's docs). A `_neg_`
/// name (`msd_neg_fib`) is rejected up front, before any file is even probed, mirroring
/// `NumberSystem`'s own line order (`isNeg` checked at `:137`, before `setAdditionAutomaton`
/// at `:142`) — see [`load_custom_base`].
///
/// # Recursion and `docs/WALNUT-BUGS.md` WB-014
///
/// A loaded custom-base file's OWN header may itself declare a numeration (standard or
/// another custom base) — handled by recursing through [`read_automaton_txt_impl`] with
/// the same `custom_bases_dir`. Real Walnut's equivalent path (`NumberSystem.
/// getComputeIfAbsent` re-entering its own static cache mid-`computeIfAbsent`) crashes
/// with `ConcurrentModificationException` on exactly this shape (WB-014). This port has
/// no shared mutable cache to re-enter here — each recursive call is a plain, independent
/// function call — so THAT SPECIFIC crash mode is provably unreachable by construction, not
/// by luck; WB-014's own entry already names this as the intended, recorded divergence for
/// this unit.
///
/// That does not mean this recursion is unconditionally safe, though: a self-referential
/// (or mutually-referential) malformed custom-base file — one whose own header names itself,
/// or a cycle of names — would recurse with no depth/cycle limit, an uncatchable Rust stack
/// overflow, a DIFFERENT crash mode from WB-014's. [`load_custom_base`] closes that gap with
/// an explicit in-progress-name guard ([`ReadError::CustomBaseCycle`]) — see its own doc for
/// the mechanism.
pub fn read_automaton_txt_with_custom_bases<P: AsRef<Path>>(
    path: P,
    custom_bases_dir: &Path,
) -> Result<Automaton, ReadError> {
    read_automaton_txt_with_custom_base_resolver(path, &CustomBasesDir(custom_bases_dir))
}

/// As [`read_automaton_txt_with_custom_bases`], but each custom-base file name is resolved to
/// an address by `resolver` instead of being joined onto one fixed directory.
///
/// This is the form a **session** needs: Java resolves every `Custom Bases/` file through
/// `Session.globalOrSessionFile` (session copy shadows the global one, per file), and it does
/// so for a nested header token inside a library `.txt` exactly as it does for a top-level
/// `?msd_fib` query token — `ParseMethods.parseAlphabetDeclaration` (reached from
/// `AutomatonReader.firstParse` while reading ANY library file) and `Predicate`'s query
/// tokenizer both land in the same `NumberSystem.getComputeIfAbsent(name)` →
/// `NumberSystem(String)` → `Session.getReadAddressForCustomBases` path. A caller that
/// resolved nested headers against a bare global directory would silently apply the WRONG
/// number system's semantics to a session-overridden base. See [`CustomBaseResolver`].
pub fn read_automaton_txt_with_custom_base_resolver<P: AsRef<Path>>(
    path: P,
    resolver: &dyn CustomBaseResolver,
) -> Result<Automaton, ReadError> {
    read_automaton_txt_impl(path.as_ref(), Some(resolver), &mut BTreeSet::new())
}

/// [`read_automaton_txt`]'s parse, reading the `.txt` grammar out of an in-memory string
/// instead of off disk.
///
/// Byte-for-byte the same parser: [`read_automaton_txt`] is `std::fs::read_to_string` +
/// this function, and the only observable difference is that a [`ReadError`] message names
/// [`STRING_ADDRESS`] where the file entry point names the real path. It exists because a
/// caller that already *has* the text (a fuzz harness, an in-memory test, anything driving
/// the reader from a buffer) would otherwise be forced through a temp file per call — pure
/// overhead for no behavioral
/// difference.
///
/// Custom-base headers (`msd_fib`, …) are [`ReadError::UnsupportedNumeration`] here, the
/// same as in [`read_automaton_txt`] — resolving them needs a file resolver, so use
/// [`read_automaton_from_str_with_custom_base_resolver`] for that.
pub fn read_automaton_from_str(content: &str) -> Result<Automaton, ReadError> {
    read_automaton_str_impl(content, STRING_ADDRESS, None, &mut BTreeSet::new())
}

/// [`read_automaton_from_str`] with [`read_automaton_txt_with_custom_base_resolver`]'s
/// custom-base support: the top-level text comes from memory, while any custom-base file
/// its header names is still resolved (and read from disk) through `resolver`.
pub fn read_automaton_from_str_with_custom_base_resolver(
    content: &str,
    resolver: &dyn CustomBaseResolver,
) -> Result<Automaton, ReadError> {
    read_automaton_str_impl(
        content,
        STRING_ADDRESS,
        Some(resolver),
        &mut BTreeSet::new(),
    )
}

fn read_automaton_txt_impl(
    path: &Path,
    custom_bases: Option<&dyn CustomBaseResolver>,
    in_progress: &mut BTreeSet<String>,
) -> Result<Automaton, ReadError> {
    let content = std::fs::read_to_string(path)?;
    read_automaton_str_impl(
        &content,
        &path.display().to_string(),
        custom_bases,
        in_progress,
    )
}

/// The `address` every [`ReadError`] message names when the text came from memory rather
/// than from a file — the string entry points ([`read_automaton_from_str`],
/// [`read_transducer_from_str`]) have no Java counterpart at all (Java's reader always
/// takes a path), so there is no real address to report and no Java text to match.
const STRING_ADDRESS: &str = "<string>";

fn read_automaton_str_impl(
    content: &str,
    address: &str,
    custom_bases: Option<&dyn CustomBaseResolver>,
    in_progress: &mut BTreeSet<String>,
) -> Result<Automaton, ReadError> {
    let mut lines = content.lines().enumerate();
    let (_, header_line) = lines
        .by_ref()
        .find(|(_, l)| !should_skip(l))
        .ok_or_else(|| ReadError::EmptyFile {
            address: address.to_string(),
        })?;

    // `AutomatonReader.firstParse`'s trivial branch (`:141-153`): the `true`/`false`
    // test runs BEFORE the alphabet-declaration parse, and once it matches nothing but
    // comments/whitespace may follow.
    if let Some(truth) = parse_true_false(header_line) {
        for (i, raw_line) in lines {
            if !should_skip(raw_line) {
                return Err(ReadError::FileHasConflict {
                    line: i + 1,
                    address: address.to_string(),
                });
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
    let (alphabet, msd, ns_names, all_reps) =
        parse_header(trimmed_header, custom_bases, in_progress)?;
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
    // Java's `AutomatonReader` stores the resolved `NumberSystem` objects themselves in
    // `A.getNS()`; this crate keeps the two facts it needs plus the name (see
    // `Automaton::ns_name`). Without the name, `isNSDiffering` cannot tell `msd_fib` from
    // `msd_2`.
    automaton.set_ns_names(ns_names);
    // The third part: the custom base's valid-representation restriction. Java gets this for
    // free (it stores the `NumberSystem` objects themselves), and it is load-bearing —
    // `Automaton::apply_all_representations`, which every `~`/`=>`/`A` runs, consults exactly
    // this. Without it, complementing an `msd_fib` automaton read from a library file admits
    // words that are not valid Zeckendorf representations. See this module's docs.
    automaton.set_all_reps(all_reps);

    let mut output: BTreeMap<usize, i32> = BTreeMap::new();
    let mut transitions: BTreeMap<usize, BTreeMap<i32, Vec<usize>>> = BTreeMap::new();
    let mut declaration_order: Vec<usize> = Vec::new();
    let mut current_state: Option<usize> = None;
    // `i32`, matching the `int` Java parses a destination id as — see the
    // `parse_methods` delegation below.
    let mut dest_states_used: BTreeSet<i32> = BTreeSet::new();

    for (i, raw_line) in lines {
        let lineno = i + 1;
        if should_skip(raw_line) {
            continue;
        }

        // `ParseMethods.parseStateDeclaration` / `parseTransition`, i.e. the SAME two
        // ports the transducer reader already used. Until this round the automaton
        // reader had its own hand-rolled `split_whitespace` copies instead, which (a)
        // swallowed an i32-overflowing state/dest id with `.parse().ok()?` and fell
        // through to the wrong error class (`UnexpectedLine`/`UndeclaredDestState`)
        // where Java raises `NumberFormatException`, (b) parsed ids as `usize`, so an id
        // too large for Java's `int` "succeeded" instead of throwing, and (c) diverged on
        // Java's own grammar in both directions (it accepted a `+`-signed id Java's
        // `\d+` rejects, and rejected the `0 + 1`/`0 - 1` spacing Java's
        // `(\+|\-)?\s*\d+` accepts). One shared port, no second grammar.
        if let Some((id, out)) = parse_methods::parse_state_declaration(raw_line)? {
            let id = id as usize; // `\d+`: never negative
            output.insert(id, out);
            transitions.entry(id).or_default();
            declaration_order.push(id);
            current_state = Some(id);
        } else if let Some((input_tokens, dests)) = parse_methods::parse_transition(raw_line)? {
            let cur = current_state.ok_or_else(|| ReadError::TransitionBeforeState {
                line: lineno,
                address: address.to_string(),
            })?;
            if input_tokens.len() != num_tracks {
                return Err(ReadError::ArityMismatch {
                    line: lineno,
                    expected: num_tracks,
                    got: input_tokens.len(),
                    address: address.to_string(),
                });
            }
            dest_states_used.extend(dests.iter().copied());
            for digits in expand_wildcards(&input_tokens, &alphabet) {
                // `Automaton::encode_index_of`, NOT `encode`: these digits come straight
                // out of an untrusted file, and Java's `AutomatonReader` has no
                // out-of-alphabet check at all — `RichAlphabet.encode`'s `List.indexOf`
                // just returns `-1` and the transition is stored under that bogus key
                // (`AutomatonReader.java:71-72`). Reproducing the key rather than
                // rejecting the file is what keeps this port's observable behavior equal
                // to Java's on both shapes: an undeclared destination still reports the
                // clean `UndeclaredDestState` below (Java's `validateDeclaredStates`,
                // which runs after this loop), and a declared one still loads. Using
                // `encode` here was a process-fatal panic on a plausible file, found by
                // Tier-5 fuzzing (Phase 4, U30, finding F2); see `encode_index_of`'s doc
                // for the real-`walnut-java` evidence.
                let sym = automaton.encode_index_of(&digits);
                transitions
                    .get_mut(&cur)
                    .expect("current_state always has a transitions entry")
                    .entry(sym)
                    .or_default()
                    .extend(dests.iter().map(|&d| d as usize));
            }
        } else {
            return Err(ReadError::UnexpectedLine {
                line: lineno,
                address: address.to_string(),
            });
        }
    }

    for &d in &dest_states_used {
        if !output.contains_key(&(d as usize)) {
            return Err(ReadError::UndeclaredDestState {
                state: d,
                address: address.to_string(),
            });
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
    //
    // Routed through `wr_core::determinize::determinize` (the U0c dispatcher) rather
    // than calling `subset_construction` directly, so Phase 3b's `[strategy …]`/
    // `[export …]` metacommands apply to `.txt`-load-triggered determinizations too --
    // see `wr_core::automaton`'s corrected module-level note on U0c's actual call-graph
    // coverage. `ctx = None` is bit-for-bit identical to the pre-dispatcher direct
    // call (`wr_core::determinize`'s `no_context_is_exactly_plain_subset_construction`
    // pins this), and with `ctx = None` the dispatcher's only fallible arm is
    // unreachable -- the same reasoning `Automaton::determinize_and_minimize`'s
    // `NO_CONTEXT_CANNOT_FAIL` already documents, so it is `.expect()`ed here rather
    // than propagated -- unlike the `minimize` call below, which stays a propagated
    // `Result` (`ReadError::Minimize`) exactly as before.
    if !automaton.fa.is_deterministic() {
        automaton.fa = trim(&automaton.fa);
        let initial: BTreeSet<usize> = [automaton.fa.q0].into_iter().collect();
        wr_core::determinize::determinize(&mut automaton, &initial, None).expect(
            "determinize with no metacommand context always takes the SC arm, which is infallible",
        );
        automaton.fa = minimize(&automaton.fa)?;
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
    /// `alphabet` is the track's full alphabet — `0..base` for a standard `msd_<k>`/
    /// `lsd_<k>` base, or [`NumberSystem::get_alphabet`]'s value for a custom base (not
    /// necessarily contiguous-from-zero in general, though every shipped custom base
    /// declares its alphabet as a `{...}` set counting up from `0` — most are `{0, 1}`,
    /// but not all: `msd_kim`/`msd_pell` are `{0, 1, 2}`, `msd_ns`/`msd_tib` are
    /// `{0, 1, 2, 3}` — verified against the real `walnut-java` `Custom Bases/*.txt`
    /// files, not assumed).
    Ns {
        msd: bool,
        alphabet: Vec<i32>,
        /// `NumberSystem.getName()` for this track, already normalized the way Java's
        /// `NumberSystem.normalizeNumberSystemToken` (`:273-295`) would: bare `msd`/`lsd`
        /// become `msd_2`/`lsd_2`, an `msd_<k>`/`lsd_<k>` token is itself, and a
        /// custom-base token is the resolved [`NumberSystem`]'s own name. Kept because
        /// `NumberSystem.isNSDiffering` compares number systems BY NAME, and a custom
        /// base is otherwise indistinguishable from the plain base with the same alphabet
        /// cardinality — see [`wr_core::automaton::Automaton::ns_name`].
        name: String,
        /// [`NumberSystem::all_representations`] for this track: the "set of all valid
        /// representations" restriction a custom base declares in its `<name>.txt` file
        /// (`msd_fib.txt` = "no `11` substring"). `None` for every standard `msd_<k>`/
        /// `lsd_<k>` base, and for a custom base that ships no `<name>.txt`.
        ///
        /// Populating this is what makes `~`/`=>`/`A` behave correctly on an automaton
        /// loaded from a library file — see [`read_automaton_txt_with_custom_bases`]'s
        /// "valid representations" note.
        all_reps: Option<Rc<Automaton>>,
    },
}

/// Per-track alphabet, per-track msd/lsd (`None` for an explicit-set track), and per-track
/// number-system name (`None` for an explicit-set track, which has no `NumberSystem`).
type HeaderSpec = (
    Vec<Vec<i32>>,
    Vec<Option<bool>>,
    Vec<Option<String>>,
    Vec<Option<Rc<Automaton>>>,
);

fn parse_header(
    line: &str,
    custom_bases: Option<&dyn CustomBaseResolver>,
    in_progress: &mut BTreeSet<String>,
) -> Result<HeaderSpec, ReadError> {
    let mut alphabet = Vec::new();
    let mut msd = Vec::new();
    let mut ns_names: Vec<Option<String>> = Vec::new();
    let mut all_reps: Vec<Option<Rc<Automaton>>> = Vec::new();
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
            parse_ns_token(word, custom_bases, in_progress)?
        };
        match token {
            HeaderToken::Set(values) => {
                alphabet.push(values);
                msd.push(None);
                ns_names.push(None);
                all_reps.push(None);
            }
            HeaderToken::Ns {
                msd: is_msd,
                alphabet: track_alphabet,
                name,
                all_reps: track_all_reps,
            } => {
                alphabet.push(track_alphabet);
                msd.push(Some(is_msd));
                ns_names.push(Some(name));
                all_reps.push(track_all_reps);
            }
        }
    }
    if alphabet.is_empty() {
        return Err(ReadError::MalformedHeader);
    }
    Ok((alphabet, msd, ns_names, all_reps))
}

fn parse_ns_token(
    word: &str,
    custom_bases: Option<&dyn CustomBaseResolver>,
    in_progress: &mut BTreeSet<String>,
) -> Result<HeaderToken, ReadError> {
    if let Some(rest) = word.strip_prefix("msd_") {
        return match numeric_base(word, rest)? {
            Some(base) => Ok(HeaderToken::Ns {
                msd: true,
                alphabet: (0..base).collect(),
                name: word.to_string(),
                all_reps: None,
            }),
            None => custom_base_token(word, custom_bases, in_progress),
        };
    }
    if let Some(rest) = word.strip_prefix("lsd_") {
        return match numeric_base(word, rest)? {
            Some(base) => Ok(HeaderToken::Ns {
                msd: false,
                alphabet: (0..base).collect(),
                name: word.to_string(),
                all_reps: None,
            }),
            None => custom_base_token(word, custom_bases, in_progress),
        };
    }
    // `NumberSystem.normalizeNumberSystemToken` (`:284-286`): a bare `msd`/`lsd` token IS
    // named `msd_2`/`lsd_2`, so that — not the bare word — is what `isNSDiffering` would
    // compare.
    match word {
        "msd" => Ok(HeaderToken::Ns {
            msd: true,
            alphabet: vec![0, 1],
            name: format!("{}2", numsys::MSD_UNDERSCORE),
            all_reps: None,
        }),
        "lsd" => Ok(HeaderToken::Ns {
            msd: false,
            alphabet: vec![0, 1],
            name: format!("{}2", numsys::LSD_UNDERSCORE),
            all_reps: None,
        }),
        _ => Err(ReadError::UnsupportedNumeration(word.to_string())),
    }
}

/// The base-validation half of `NumberSystem`'s constructor
/// (`NumberSystem.setAdditionAutomaton`, `:322-332`), applied to the `rest` of a
/// `msd_`/`lsd_`-prefixed header token whose full text is `name`:
///
/// * `Ok(Some(k))` — `rest` is `\d+` and `k >= 2`: an ordinary base-*k* track;
/// * `Ok(None)` — `rest` is not `\d+` at all: Java falls through to the custom-base
///   file lookup (`loadAutomatonOrNull`), so this port hands it to
///   [`custom_base_token`];
/// * `Err(NotDefined)` — `rest` IS `\d+` but `<= 1`: Java throws
///   `"Number system " + name + " is not defined."` (`:330`), because neither
///   `isNumber(base) && parseInt(base) > 1` nor `parseNegNumber(base) > 1` holds and
///   there is no `Custom Bases/msd_1*.txt` to rescue it. Confirmed against
///   `walnut-java/target/Walnut-all.jar`: a library file headed `msd_1` (or `msd_0`)
///   answers exactly that, and the session continues;
/// * `Err(BaseNotAnI32)` — `rest` is `\d+` but overflows `int`, where Java's
///   `Integer.parseInt` (`:325`) throws an unchecked `NumberFormatException`;
///   `NumSysError`'s existing stand-in for that.
///
/// Before this, the whole check was a bare `rest.parse::<i32>()`, so `msd_1` built the
/// one-symbol alphabet `{0}` and `msd_0`/`msd_-3` built the *empty* alphabet — a missing
/// validation found by Tier-5 fuzzing (Phase 4, U30, finding F3), whose first body digit
/// then tripped a panic in `encode`.
///
/// The membership test is [`is_number`] (`^\d+$`), not `str::parse`, precisely as Java's
/// is: `parse` would accept a leading `+`/`-` that Java's `isNumber` rejects. One
/// residual, pre-existing message divergence that this deliberately does not chase:
/// Java's alphabet-token regex (`ParseMethods.PATTERN_NEXT_ALPHABET_TOKEN`) cannot
/// consume a `-`, so real Walnut reads `msd_-3` as the token `msd_` and reports
/// `Number system msd_ is not defined.`, whereas this reader's whitespace-delimited
/// tokenizer keeps the whole word and reports it under `msd_-3`. Both are errors on the
/// same input; only the name in the text differs, and closing it means porting that
/// regex, not this function.
///
/// # Why the `<= 1` case errors here rather than falling through to the file lookup
///
/// Java's order is file-FIRST: `setAdditionAutomaton` calls
/// `loadAutomatonOrNull(name, "_addition", base)` before it ever looks at the base's
/// value, so a `Custom Bases/msd_1_addition.txt` would genuinely make `msd_1` legal in
/// real Walnut. This function reports the error instead — deliberately, and consistently
/// with a **pre-existing** simplification right above it: [`parse_ns_token`] shortcuts
/// every numeric base straight to `(0..k)` and never probes for
/// `Custom Bases/msd_<k>_addition.txt` either, so no numeric base consults files in this
/// reader. Making only `k <= 1` consult them would be the inconsistent choice. Nothing in
/// the real `walnut-java` corpus ships a numerically-named custom base (all of them are
/// `msd_fib`-style names, verified), so neither branch of the simplification is live; if
/// that ever changes, both belong to the same fix, not this one.
fn numeric_base(name: &str, rest: &str) -> Result<Option<i32>, ReadError> {
    if !is_number(rest) {
        return Ok(None);
    }
    let base = rest
        .parse::<i32>()
        .map_err(|_| ReadError::NumSys(NumSysError::BaseNotAnI32(rest.to_string())))?;
    if base <= 1 {
        return Err(ReadError::NumSys(NumSysError::NotDefined(name.to_string())));
    }
    Ok(Some(base))
}

/// The non-numeric-base fallback shared by both `msd_`/`lsd_`-prefixed branches of
/// [`parse_ns_token`]: without a resolver, preserves the pre-U13
/// [`ReadError::UnsupportedNumeration`] behavior exactly (every existing caller/test).
/// With one, attempts [`load_custom_base`] and propagates any failure as
/// [`ReadError::NumSys`] rather than downgrading it back to `UnsupportedNumeration` —
/// once a resolver is supplied, "this looked like a custom-base name but failed to
/// load" is a real, reportable error, not silent unsupported-numeration territory.
fn custom_base_token(
    word: &str,
    custom_bases: Option<&dyn CustomBaseResolver>,
    in_progress: &mut BTreeSet<String>,
) -> Result<HeaderToken, ReadError> {
    let Some(resolver) = custom_bases else {
        return Err(ReadError::UnsupportedNumeration(word.to_string()));
    };
    let ns = load_custom_base(word, resolver, in_progress)?;
    Ok(HeaderToken::Ns {
        msd: ns.is_msd(),
        alphabet: ns.get_alphabet().to_vec(),
        name: ns.name().to_string(),
        // `Automaton.getNS().get(i).getAllRepresentations()` — Java keeps the resolved
        // `NumberSystem` object itself, so this comes for free there. See this module's docs.
        all_reps: ns.all_representations().cloned(),
    })
}

/// Builds the [`NumberSystem`] named `name` by reading the `Custom Bases/`-style files
/// `resolver` points each candidate name at — see [`read_automaton_txt_with_custom_bases`]'s
/// doc for the exact resolution order this implements.
///
/// # Recursion guard
///
/// This recurses back into [`read_automaton_txt_impl`] (via [`probe_custom_base_candidate`])
/// to load `name`'s own `_addition.txt`/`_less_than.txt`/`.txt` files, and THOSE files' own
/// headers may themselves name a custom base — including, on a malformed self-referential
/// input, `name` itself again. `in_progress` tracks every custom-base name currently being
/// resolved on the current call stack (pushed on entry, popped on every exit path via the
/// closure below); re-entering a name already in progress is [`ReadError::CustomBaseCycle`]
/// instead of unbounded recursion. No real shipped `walnut-java` `Custom Bases/*.txt` file
/// triggers this (all use explicit-set `{...}` headers, verified) — so it is not live
/// against the real corpus — but a malformed hand-written custom-base file could otherwise
/// recurse without limit, an uncatchable Rust stack overflow. This is a DIFFERENT crash mode
/// from WB-014 (Java's `ConcurrentModificationException` from `NumberSystem.
/// getComputeIfAbsent` re-entering its own static cache): this port has no shared mutable
/// cache to re-enter, so WB-014's specific crash is provably unreachable here — but that
/// alone does not rule out a plain unbounded-recursion stack overflow for the same
/// malformed-input shape, which is what this guard closes.
fn load_custom_base(
    name: &str,
    resolver: &dyn CustomBaseResolver,
    in_progress: &mut BTreeSet<String>,
) -> Result<NumberSystem, ReadError> {
    // Mirrors `NumberSystem`'s own constructor order (`isNeg` checked at `:137`, BEFORE
    // any file is consulted at `:142`): avoids probing/reading `Custom Bases/msd_neg_*`
    // files for a name that's going to be rejected anyway.
    if name.contains(numsys::UNDERSCORE_NEG_UNDERSCORE) {
        return Err(NumSysError::UnsupportedNegativeBase(name.to_string()).into());
    }
    if !in_progress.insert(name.to_string()) {
        return Err(ReadError::CustomBaseCycle(name.to_string()));
    }
    let result = (|| {
        let addition = probe_custom_base_candidate(
            name,
            numsys::UNDERSCORE_ADDITION_AUTOMATON,
            resolver,
            in_progress,
        )?;
        let less_than = probe_custom_base_candidate(
            name,
            numsys::UNDERSCORE_LESS_THAN_AUTOMATON,
            resolver,
            in_progress,
        )?;
        let all_representations =
            probe_custom_base_candidate(name, numsys::TXT_EXTENSION, resolver, in_progress)?;
        let files = CustomBaseFiles {
            addition,
            less_than,
            all_representations,
        };
        Ok(NumberSystem::with_custom_base_files(name, files)?)
    })();
    in_progress.remove(name);
    result
}

/// One `NumberSystem.loadAutomatonOrNull` probe (`:299-319`), minus the decision logic
/// (that's [`CustomBaseCandidates::resolve`], called INSIDE
/// [`NumberSystem::with_custom_base_files`], not here): stats and, if present, reads the
/// main file; stats and reads the complement file ONLY if the main one is absent (Java's
/// `else if`, not two independent probes) — matching `loadAutomatonOrNull`'s exact
/// short-circuit rather than reading both unconditionally.
fn probe_custom_base_candidate(
    name: &str,
    extension: &str,
    resolver: &dyn CustomBaseResolver,
    in_progress: &mut BTreeSet<String>,
) -> Result<CustomBaseCandidates, ReadError> {
    let (main_name, complement_name) = numsys::custom_base_candidate_names(name, extension)?;
    let main_path = resolver.resolve(&main_name);
    let main = if main_path.is_file() {
        Some(read_automaton_txt_impl(
            &main_path,
            Some(resolver),
            in_progress,
        )?)
    } else {
        None
    };
    let complement = if main.is_none() {
        let complement_path = resolver.resolve(&complement_name);
        if complement_path.is_file() {
            Some(read_automaton_txt_impl(
                &complement_path,
                Some(resolver),
                in_progress,
            )?)
        } else {
            None
        }
    } else {
        None
    };
    Ok(CustomBaseCandidates { main, complement })
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

// ---------------------------------------------------------------------------
// readTransducer (U13)
// ---------------------------------------------------------------------------

/// The result of [`read_transducer_txt`] — everything `AutomatonReader.readTransducer`
/// (`:196-274`) parses out of a `.txt` DFST file. Not `wr_core`'s eventual `Transducer`
/// type (that type, and `transduce` itself, are Phase 3b's U20/U26) — a plain data
/// struct capturing the parse result, matching this unit's scope ("the parsing logic
/// belongs here", per the Phase 3a plan's U13 row).
#[derive(Debug, Clone)]
pub struct TransducerData {
    /// Per-track alphabet (same shape as [`Automaton::alphabet`]).
    pub alphabet: Vec<Vec<i32>>,
    /// Per-track msd/lsd (`None` for an explicit-set track) — same shape as
    /// [`Automaton::msd`].
    pub msd: Vec<Option<bool>>,
    /// The declared alphabet's total encoded size (product of per-track sizes).
    pub alphabet_size: usize,
    /// The first declared state's id (Java: `q0`, not necessarily `0`).
    pub q0: usize,
    /// The number of declared states.
    pub q: usize,
    /// Per-state transition table, keyed by encoded input symbol
    /// ([`Automaton::encode`]) — `Transducer.fa.d`'s shape. Transducers are read AS-IS,
    /// with no auto-determinize step (`readTransducer` has none, unlike `readAutomaton`
    /// — DFSTs are used directly, determinism is the file author's responsibility).
    pub d: Vec<BTreeMap<i32, Vec<usize>>>,
    /// Per-state, per-encoded-input-symbol OUTPUT value — `Transducer.sigma`
    /// (`currentStateTransitionOutputs`, `:211-253`). State output itself is meaningless
    /// for a transducer ("state output does not matter for transducers", `:237`) and is
    /// not represented here at all, matching Java discarding it (`currentStateOutput = 0`
    /// unconditionally).
    pub sigma: Vec<BTreeMap<i32, i32>>,
}

/// `AutomatonReader.readTransducer(Transducer, String)` (`:196-274`).
///
/// Shares [`parse_header`]'s grammar (`firstParse`'s alphabet-declaration half) but,
/// matching Java's `trueFalseSingleton == null` call (`:203`), **never** treats a bare
/// `true`/`false` header line specially — transducers have no trivial-automaton
/// shortcut, so such a line is parsed (and rejected) as an ordinary numeration token
/// instead. No custom-base directory parameter: no shipped `Transducer Library/*.txt`
/// fixture declares one (every real transducer header this port has seen is an explicit
/// set or a plain `msd_k`/`lsd_k`), so this is scoped to [`read_automaton_txt`]'s
/// custom-base-free default; a future caller that needs one can extend this the same way
/// [`read_automaton_txt_with_custom_bases`] extends the automaton reader.
pub fn read_transducer_txt<P: AsRef<Path>>(path: P) -> Result<TransducerData, ReadError> {
    let path = path.as_ref();
    let content = std::fs::read_to_string(path)?;
    read_transducer_str_impl(&content, &path.display().to_string())
}

/// [`read_transducer_txt`]'s parse, reading the transducer grammar out of an in-memory
/// string instead of off disk — the transducer counterpart of
/// [`read_automaton_from_str`], and identical to [`read_transducer_txt`] minus the
/// `std::fs::read_to_string`. Same motivation: a caller that already holds the text
/// should not have to round-trip it through a temp file.
pub fn read_transducer_from_str(content: &str) -> Result<TransducerData, ReadError> {
    read_transducer_str_impl(content, STRING_ADDRESS)
}

/// [`read_transducer_from_str`]'s body, with the `address` every [`ReadError`] message
/// names threaded in (a real path from [`read_transducer_txt`], [`STRING_ADDRESS`] from
/// the in-memory entry point).
fn read_transducer_str_impl(content: &str, address: &str) -> Result<TransducerData, ReadError> {
    let mut lines = content.lines().enumerate();
    let (_, header_line) = lines
        .by_ref()
        .find(|(_, l)| !should_skip(l))
        .ok_or_else(|| ReadError::EmptyFile {
            address: address.to_string(),
        })?;

    let trimmed_header = header_line.trim();
    // The NS names go unused here: a transducer's own header names never reach an
    // `Automaton` (`TransducerData` carries only the alphabet/msd it needs), and Java's
    // `readTransducer` likewise keeps the `NumberSystem` list only inside its scratch
    // parse state.
    let (alphabet, msd, _ns_names, _all_reps) =
        parse_header(trimmed_header, None, &mut BTreeSet::new())?;
    let num_tracks = alphabet.len();
    let alphabet_size: usize = alphabet.iter().map(|t| t.len()).product();
    let label: Vec<String> = (0..num_tracks).map(|i| i.to_string()).collect();

    // Scratch `Automaton`, used only for its `encode` (mixed-radix, position-in-alphabet)
    // — same precedent as `read_automaton_txt_impl`'s placeholder `Fa`.
    let scratch = Automaton::new(
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
        msd.clone(),
    );

    let mut d: BTreeMap<usize, BTreeMap<i32, Vec<usize>>> = BTreeMap::new();
    let mut sigma: BTreeMap<usize, BTreeMap<i32, i32>> = BTreeMap::new();
    let mut declaration_order: Vec<usize> = Vec::new();
    let mut current_state: Option<usize> = None;
    // `i32`, matching the `int` Java parses a destination id as.
    let mut dest_states_used: BTreeSet<i32> = BTreeSet::new();

    for (i, raw_line) in lines {
        let lineno = i + 1;
        if should_skip(raw_line) {
            continue;
        }

        if let Some(id) = parse_methods::parse_transducer_state_declaration(raw_line)? {
            let id = id as usize;
            d.entry(id).or_default();
            sigma.entry(id).or_default();
            declaration_order.push(id);
            current_state = Some(id);
        } else if let Some(t) = parse_methods::parse_transducer_transition(raw_line)? {
            let cur = current_state.ok_or_else(|| ReadError::TransitionBeforeState {
                line: lineno,
                address: address.to_string(),
            })?;
            if t.input.len() != num_tracks {
                return Err(ReadError::ArityMismatch {
                    line: lineno,
                    expected: num_tracks,
                    got: t.input.len(),
                    address: address.to_string(),
                });
            }
            // `parseTransducerTransition`'s output group always yields exactly one
            // element by construction (see `parse_methods::TransducerTransition`'s own
            // doc) -- Java's `if (output.size() != 1) throw` guard is dead code, not
            // ported as a reachable error.
            debug_assert_eq!(t.output.len(), 1);
            let output = t.output[0];
            let dest: Vec<usize> = t.dest.iter().map(|&x| x as usize).collect();
            dest_states_used.extend(t.dest.iter().copied());
            for digits in expand_wildcards(&t.input, &alphabet) {
                // Same untrusted-input reasoning as the automaton reader above;
                // `readTransducer` (`AutomatonReader.java:245-247`) encodes with the very
                // same `richAlphabet.encode`.
                let sym = scratch.encode_index_of(&digits);
                // `AutomatonReader.readTransducer` (`:249-250`):
                // `currentStateTransitions.put(encode(i), dest)` — Java `Map.put`
                // REPLACES any prior entry for the same encoded symbol, it does not
                // accumulate. So when two separate transition lines from the same
                // state declare the same input symbol, the LATER line's destination
                // silently wins and the earlier one is discarded — unlike
                // `read_automaton_txt_impl`'s `d` table (which genuinely accumulates,
                // matching `readAutomaton`'s own `computeIfAbsent(...).addAll(dest)` at
                // `:66-67`). Nondeterminism in a transducer's `d` is still fully
                // expressible — just WITHIN one line's `dest` list (`"0 -> 0 1 / 0"`),
                // not across repeated-symbol lines. Overwrite here, matching `sigma`'s
                // existing (correct) overwrite semantics right below.
                d.get_mut(&cur)
                    .expect("current_state always has a d entry")
                    .insert(sym, dest.clone());
                sigma
                    .get_mut(&cur)
                    .expect("current_state always has a sigma entry")
                    .insert(sym, output);
            }
        } else {
            return Err(ReadError::UnexpectedLine {
                line: lineno,
                address: address.to_string(),
            });
        }
    }

    for &dst in &dest_states_used {
        if !d.contains_key(&(dst as usize)) {
            return Err(ReadError::UndeclaredDestState {
                state: dst,
                address: address.to_string(),
            });
        }
    }

    let q = declaration_order.len();
    if q == 0 {
        return Err(ReadError::NoStates);
    }
    if d.len() != q || (0..q).any(|i| !d.contains_key(&i)) {
        return Err(ReadError::NonDenseStateIds);
    }
    let q0 = declaration_order[0];

    let mut d_vec: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); q];
    let mut sigma_vec: Vec<BTreeMap<i32, i32>> = vec![BTreeMap::new(); q];
    for (id, row) in d {
        d_vec[id] = row;
    }
    for (id, row) in sigma {
        sigma_vec[id] = row;
    }

    Ok(TransducerData {
        alphabet,
        msd,
        alphabet_size,
        q0,
        q,
        d: d_vec,
        sigma: sigma_vec,
    })
}

// ---------------------------------------------------------------------------
// readComments (U13)
// ---------------------------------------------------------------------------

/// `AutomatonReader.readComments(String address)` (`:279-294`): "Usually we skip
/// comments. Here we skip everything else and return them." Collects every comment line
/// (`ParseMethods.PATTERN_COMMENT`, [`parse_methods::is_comment_line`]) anywhere in the
/// file, in original order, joined by `\n` (Java: `System.lineSeparator()` — this port
/// targets `\n`-emitting platforms, matching every other line-oriented function in this
/// module), then trimmed top and bottom (`String.strip()` -> `str::trim`).
pub fn read_comments<P: AsRef<Path>>(path: P) -> Result<String, ReadError> {
    let content = std::fs::read_to_string(path)?;
    let mut out = String::new();
    for line in content.lines() {
        if parse_methods::is_comment_line(line) {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out.trim().to_string())
}

// ---------------------------------------------------------------------------
// AutomatonDFA(String) / readAutomatonDFAFromFile (U13)
// ---------------------------------------------------------------------------

/// `AutomatonDFA(String address)` (`AutomatonDFA.java:27-32`): `readAutomaton(this,
/// address); requireDfaStorage();`.
///
/// In practice a thin wrapper over [`read_automaton_txt`]: `readAutomaton` (this reader's
/// own auto-determinize-on-load step) already guarantees the parsed automaton is
/// deterministic — or, for a genuine NFAO, see the gap noted below — before
/// `requireDfaStorage` ([`AutomatonDFA::from`]) ever runs, so the only Java-observable
/// difference from a bare [`read_automaton_txt`] call is the TYPE-LEVEL promise the
/// caller gets back (see [`AutomatonDFA`]'s own doc comment for why that promise is
/// enforced more strongly here than in Java).
///
/// `AutomatonDFA.readAutomatonDFAFromFile(String automataName)`'s extra step — resolving
/// `automataName` to a path via `Session.getReadFileForAutomataLibrary` — is `wr-cli`'s
/// job (`Session`, Phase 3a's U14): this function, like every other one in this module,
/// takes an already-resolved path.
///
/// # `docs/WALNUT-BUGS.md` WB-022: a real gap, now logged (not just noted inline)
///
/// Real Walnut's `readAutomaton` throws `WalnutException.nonDeterministicO` when a file
/// describes a genuine NFAO (nondeterministic transitions AND some state output `> 1`) —
/// checked BEFORE auto-determinizing, since a DFAO's per-state output values cannot be
/// soundly merged by subset construction. [`read_automaton_txt_impl`] has no such check
/// and unconditionally auto-determinizes every nondeterministic input regardless of its
/// outputs, collapsing them to plain 0/1 acceptance (`wr_core::determinize::
/// subset_construction`'s own docs confirm this is exactly what it does) — a silently
/// DIFFERENT, wrong automaton, not the error Java would produce. So `AutomatonDFA::from`'s
/// matching `is_fao()` guard (`"NFAOs are not supported.."` panic) is PROVABLY UNREACHABLE
/// through this specific call path — by the time it runs, `read_automaton_txt` has already
/// forced determinism. No real custom-base or golden-corpus fixture this port has
/// encountered exercises the genuine-NFAO shape, so this is not live against the real
/// corpus — but it is a plausible hand-written input, hence WB-022 (`docs/WALNUT-BUGS.md`),
/// pinned by
/// [`tests::read_automaton_dfa_txt_on_a_genuine_nfao_file_silently_determinizes_instead_of_erroring_wb022`].
pub fn read_automaton_dfa_txt<P: AsRef<Path>>(path: P) -> Result<AutomatonDFA, ReadError> {
    Ok(AutomatonDFA::from(read_automaton_txt(path)?))
}

/// Like [`read_automaton_dfa_txt`], but with [`read_automaton_txt_with_custom_bases`]'s
/// custom-base header support — Java's `readAutomaton` is shared code between the
/// `Automaton(String)` and `AutomatonDFA(String)` constructors, so custom-base
/// resolution applies equally to both.
pub fn read_automaton_dfa_txt_with_custom_bases<P: AsRef<Path>>(
    path: P,
    custom_bases_dir: &Path,
) -> Result<AutomatonDFA, ReadError> {
    Ok(AutomatonDFA::from(read_automaton_txt_with_custom_bases(
        path,
        custom_bases_dir,
    )?))
}

/// [`read_automaton_dfa_txt_with_custom_bases`]'s resolver-injecting form — see
/// [`read_automaton_txt_with_custom_base_resolver`] for why a session needs it.
pub fn read_automaton_dfa_txt_with_custom_base_resolver<P: AsRef<Path>>(
    path: P,
    resolver: &dyn CustomBaseResolver,
) -> Result<AutomatonDFA, ReadError> {
    Ok(AutomatonDFA::from(
        read_automaton_txt_with_custom_base_resolver(path, resolver)?,
    ))
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

    // =======================================================================
    // Phase 4 U30 Tier-5 fuzz regressions (findings F1/F2/F3)
    // =======================================================================

    /// F2, sub-case A. ` lsd_2\n0 1\n20-> 11` — the body digit `20` is outside the
    /// header's `{0,1}` alphabet AND state `11` is never declared. Real `walnut-java`
    /// on this exact file reports `State 11 is used but never declared anywhere in
    /// file: …` (verified on `target/Walnut-all.jar`), because `RichAlphabet.encode`'s
    /// `List.indexOf` silently yields `-1` and the undeclared-state validation only
    /// runs after the whole parse loop. This port used to panic inside `encode` before
    /// ever reaching that check.
    #[test]
    fn an_out_of_alphabet_digit_with_an_undeclared_dest_reports_the_undeclared_state() {
        let e = read_automaton_from_str(" lsd_2\n0 1\n20-> 11").expect_err("undeclared state");
        assert!(
            matches!(e, ReadError::UndeclaredDestState { state: 11, .. }),
            "{e:?}"
        );
        // A negative digit reaches the same place (`-1 1 -> 0` under `msd_2`).
        let e = read_automaton_from_str("msd_2\n0 1\n-1 -> 9").expect_err("undeclared state");
        assert!(
            matches!(e, ReadError::UndeclaredDestState { state: 9, .. }),
            "{e:?}"
        );
    }

    /// F2, sub-case B. Same shape but with a DECLARED destination. Real `walnut-java`
    /// **loads this file with no error at all** — `new Automaton(path)` returns a
    /// 1-state automaton, keeping the transition under the bogus encoded key `-1`
    /// (verified directly against `Walnut-all.jar`'s classes; the
    /// `IndexOutOfBoundsException: Index -1 out of bounds for length 2` the fuzz report
    /// saw comes later, from `AutomatonWriter.writeToGV`'s `decode(-1)`, and
    /// `Prover.readBuffer`'s `catch (RuntimeException)` recovers from that too).
    ///
    /// So the faithful port is to REPRODUCE the key, not to reject the file: rejecting
    /// it would diverge on every file Java accepts (see the test below). Any later pass
    /// that iterates `0..alphabet_size` drops the `-1` entry, which is exactly what
    /// real Walnut's own written-back output shows.
    #[test]
    fn an_out_of_alphabet_digit_with_a_declared_dest_loads_with_javas_minus_one_key() {
        let a = read_automaton_from_str(" lsd_2\n0 1\n20 -> 0\n").expect("Java loads this too");
        assert_eq!(a.fa.q, 1);
        assert_eq!(a.fa.d[0].get(&-1), Some(&vec![0usize]));
        assert!(
            a.fa.d[0].keys().all(|&k| k < 0),
            "no valid symbol is stored"
        );
        // `encode_index_of` is what produces that key; `encode` would have panicked.
        assert_eq!(a.encode_index_of(&[20]), -1);
    }

    /// F2, sub-case B, the case that decides the fix: real `walnut-java` accepts this
    /// file and writes it back out as exactly itself minus the out-of-alphabet line
    /// (`5 -> 1`), confirmed by running `eval`/`def` over it on `Walnut-all.jar`. A
    /// port that rejected out-of-alphabet digits outright would diverge here.
    #[test]
    fn an_out_of_alphabet_digit_leaves_every_in_alphabet_transition_intact() {
        let a = read_automaton_from_str("msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n")
            .expect("Java accepts this file");
        assert_eq!(a.fa.q, 2);
        assert_eq!(a.fa.o, vec![0, 1]);
        // State 0's two real transitions, unaffected.
        assert_eq!(a.fa.d[0].get(&0), Some(&vec![0usize]));
        assert_eq!(a.fa.d[0].get(&1), Some(&vec![1usize]));
        // State 1 keeps its real `0 -> 0` and parks `5 -> 1` under the bogus key.
        assert_eq!(a.fa.d[1].get(&0), Some(&vec![0usize]));
        assert_eq!(a.fa.d[1].get(&1), None);
        assert_eq!(a.fa.d[1].get(&-1), Some(&vec![1usize]));
    }

    /// F2, sub-case C — the **aliasing** case, and the reason `encode_index_of` is a
    /// faithfulness point rather than merely a crash-avoidance one.
    ///
    /// With more than one track, `RichAlphabet.encode`'s `-1` terms can cancel against
    /// the other tracks' real terms and land on a **valid** key: under `msd_2 msd_2`
    /// (`encoder = [1, 2]`), the line `5 1 -> 0` encodes to `1*(-1) + 2*1 == 1`, i.e.
    /// exactly the key the legitimate input `1 0` would have. Real Walnut therefore reads
    /// this file as an automaton that accepts `(1, 0)` — silently, with no diagnostic and
    /// no crash to give it away (WB-038's outcome (b)). This port must agree digit for
    /// digit, so the test asserts the ALIASED language, not the file's apparent one.
    ///
    /// It is pinned precisely because it is intentional and easy to "fix" by accident: an
    /// out-of-alphabet check anywhere in this path would reject a file Java accepts, and
    /// clamping the index would change which valid key it aliases onto.
    ///
    /// **Confirmed live** against `walnut-java/target/Walnut-all.jar` (2026-08-16): this
    /// exact file, evaluated as `eval wralias1 "?msd_2 $wralias(x,y)";`, loads with no
    /// diagnostic and writes a result whose only non-zero-input transition is `1 0 -> 0`
    /// — the aliased tuple, never the `5 1` the file spells.
    #[test]
    fn an_out_of_alphabet_digit_can_alias_onto_a_valid_key_exactly_as_java_does() {
        let a = read_automaton_from_str("msd_2 msd_2\n0 1\n5 1 -> 0\n").expect("Java loads this");
        assert_eq!(a.fa.q, 1);
        // The bogus digit tuple is stored under the VALID key 1, not under a negative
        // one -- so unlike the single-track case it survives every later pass.
        assert_eq!(a.fa.d[0].get(&1), Some(&vec![0usize]));
        assert_eq!(a.encode_index_of(&[5, 1]), 1);
        assert_eq!(a.encode(&[1, 0]), 1);
        // Which means the automaton's language is over `(1, 0)`, NOT over the `(5, 1)`
        // the file appears to declare.
        assert_eq!(a.decode(1), vec![1, 0]);
    }

    /// F3. Java's `NumberSystem` constructor rejects a base `<= 1` outright
    /// (`:322-332`): a library file headed `msd_1` answers `Number system msd_1 is not
    /// defined.` on the real CLI, and the session continues. This reader used to build
    /// `(0..base)` for any `i32` it could parse, so `msd_1` produced the one-symbol
    /// alphabet `{0}` and `msd_0` an EMPTY one.
    #[test]
    fn a_header_base_below_two_is_not_a_defined_number_system() {
        for (header, name) in [("msd_1", "msd_1"), ("msd_0", "msd_0"), ("lsd_1", "lsd_1")] {
            let e = parse_header(header, None, &mut BTreeSet::new()).expect_err("base <= 1");
            match e {
                ReadError::NumSys(NumSysError::NotDefined(ref n)) => assert_eq!(n, name),
                ref other => panic!("{other:?}"),
            }
            // The MESSAGE the user actually sees, asserted on the error `parse_header`
            // really returned. (An earlier version of this assertion built its own
            // `ReadError` value and compared that to a `Debug`-shaped string it also
            // constructed — tautological twice over: it could not fail whatever
            // `parse_header` returned, and it pinned the Debug dump this type's
            // `Display` used to emit instead of Walnut's real text.)
            assert_eq!(
                e.to_string(),
                format!("Number system {name} is not defined.")
            );
        }
        // Reached through the whole-file entry point too, not just `parse_header`.
        let e = read_automaton_from_str("# .\nmsd_1\n\n1 1\n1 -> 0\n").expect_err("base <= 1");
        assert!(
            matches!(e, ReadError::NumSys(NumSysError::NotDefined(_))),
            "{e:?}"
        );
        // Base 2 and up is of course still fine, and a signed base is not a number at
        // all to Java's `isNumber`, so it falls through to the custom-base branch
        // (which, with no resolver, is `UnsupportedNumeration`) instead of silently
        // building an empty alphabet.
        assert!(parse_header("msd_2", None, &mut BTreeSet::new()).is_ok());
        let e = parse_header("msd_-3", None, &mut BTreeSet::new()).expect_err("not a base");
        assert!(matches!(e, ReadError::UnsupportedNumeration(_)), "{e:?}");
    }

    /// F1's class, at the two `parse_methods` call sites the transducer reader uses:
    /// a `\d+` group that matches syntactically but overflows `i32`. Java's
    /// `UtilityMethods.parseInt` throws `NumberFormatException` there and
    /// `Prover.readBuffer` recovers; this port used to panic, which was process-fatal.
    #[test]
    fn a_transducer_integer_that_overflows_i32_reports_instead_of_panicking() {
        for content in [
            "msd_2\n99999999999\n0 -> 0 / 0\n", // state declaration
            "msd_2\n0\n99999999999 -> 0 / 0\n", // input digit
            "msd_2\n0\n0 -> 99999999999 / 0\n", // destination id
            "msd_2\n0\n0 -> 0 / 99999999999\n", // output value
        ] {
            let e = read_transducer_from_str(content).expect_err("overflows i32");
            match e {
                ReadError::ParseMethods(ref p) => {
                    assert_eq!(p.to_string(), "For input string: \"99999999999\"")
                }
                ref other => panic!("{other:?}"),
            }
            // The wrapper renders the same text: Java does not wrap a
            // `NumberFormatException` from `parseInt` either.
            assert_eq!(e.to_string(), "For input string: \"99999999999\"");
        }
    }

    /// F1's class at the fifth site, the one the first fix round missed: the ORDINARY
    /// automaton reader's own state-declaration/transition parsing. It used to be a
    /// second, hand-rolled grammar in this file whose `.parse().ok()?` silently turned an
    /// `i32`-overflowing digit run into "this line is not a state declaration", so the
    /// read failed as `UnexpectedLine`/`UndeclaredDestState` where Java throws
    /// `NumberFormatException` from `UtilityMethods.parseInt`. Both readers now share the
    /// `parse_methods` port, so all four groups report the same way.
    #[test]
    fn an_automaton_integer_that_overflows_i32_reports_a_number_format_error() {
        for content in [
            "msd_2\n99999999999 0\n0 -> 0\n",   // state id
            "msd_2\n0 99999999999\n0 -> 0\n",   // state output
            "msd_2\n0 0\n99999999999 -> 0\n",   // transition digit
            "msd_2\n0 0\n0 -> 99999999999\n",   // destination id
            "msd_2\n0 0\n0 -> 0 99999999999\n", // second destination id
        ] {
            let e = read_automaton_from_str(content).expect_err("overflows i32");
            assert!(
                matches!(e, ReadError::ParseMethods(_)),
                "{content:?} -> {e:?}"
            );
            assert_eq!(e.to_string(), "For input string: \"99999999999\"");
        }
        // And a digit run that is huge but still a valid `i32` is NOT a parse failure --
        // it is an ordinary undeclared destination, reported with Java's own text.
        let e = read_automaton_from_str("msd_2\n0 0\n0 -> 2000000000\n").expect_err("undeclared");
        assert_eq!(
            e.to_string(),
            "State 2000000000 is used but never declared anywhere in file: <string>"
        );
    }

    /// The grammar the automaton reader inherited by sharing `parse_methods`: Java's
    /// `(\+|\-)?\s*\d+` tolerates whitespace between a sign and its digits, and its
    /// `\d+` state/destination ids do NOT tolerate a sign at all. The deleted
    /// hand-rolled parser had both backwards.
    #[test]
    fn state_and_transition_lines_follow_javas_own_regexes() {
        // `0 - 1`: a state whose OUTPUT is -1, spelled with a space after the sign.
        let a = read_automaton_from_str("msd_2\n0 - 1\n0 -> 0\n").expect("Java accepts this");
        assert_eq!(a.fa.o, vec![-1]);
        // A `+`-signed state id is not `\d+`, so the line is not a state declaration --
        // and not a transition either, so it is an undefined statement.
        let e = read_automaton_from_str("msd_2\n+0 1\n0 -> 0\n").expect_err("not `\\d+`");
        assert!(
            matches!(e, ReadError::UnexpectedLine { line: 2, .. }),
            "{e:?}"
        );
        // Likewise a `+`-signed destination id.
        let e = read_automaton_from_str("msd_2\n0 1\n0 -> +0\n").expect_err("not `\\d+`");
        assert!(
            matches!(e, ReadError::UnexpectedLine { line: 3, .. }),
            "{e:?}"
        );
    }

    /// `ReadError`'s `Display` renders Walnut's own message text, not the `Debug` dump it
    /// emitted before this round (`NumSys(NotDefined("msd_1"))`, …) — the text is what
    /// Tier-1's `error*` fixtures compare and what the user reads. Each expected string
    /// below is copied from the cited `walnut-java` throw site.
    #[test]
    fn read_errors_render_walnuts_own_message_text() {
        // `WalnutException.fileEmpty` (`WalnutException.java:53`).
        assert_eq!(
            read_automaton_from_str("# nothing but a comment\n")
                .expect_err("empty")
                .to_string(),
            "File is empty or contains only comments/whitespace: <string>"
        );
        // `WalnutException.fileHasConflict` (`:56-57`).
        assert_eq!(
            read_automaton_from_str("true\n{0, 1}\n")
                .expect_err("conflict")
                .to_string(),
            "A file that declares 'true'/'false' must not contain other statements: \
             line 2 of file <string>"
        );
        // `WalnutException.undefinedStatement` (`:125`).
        assert_eq!(
            read_automaton_from_str("msd_2\n0 0\nnonsense\n")
                .expect_err("undefined statement")
                .to_string(),
            "Undefined statement: line at 3 of file <string>"
        );
        // `AutomatonReader.validateTransition`'s two throws (`:116-125`).
        assert_eq!(
            read_automaton_from_str("msd_2\n0 -> 0\n0 0\n")
                .expect_err("transition first")
                .to_string(),
            "Must declare a state before declaring a list of transitions: \
             line 2 of file <string>"
        );
        assert_eq!(
            read_automaton_from_str("msd_2 msd_2\n0 0\n0 -> 0\n")
                .expect_err("arity")
                .to_string(),
            "This automaton requires a 2-tuple as input: line 3 of file <string>"
        );
        // `AutomatonReader.validateDeclaredStates` (`:191`).
        assert_eq!(
            read_automaton_from_str("msd_2\n0 0\n0 -> 5\n")
                .expect_err("undeclared")
                .to_string(),
            "State 5 is used but never declared anywhere in file: <string>"
        );
        // A real path is named as itself, not as the in-memory placeholder.
        let dir = std::env::temp_dir().join(format!("wr-io-test-msgaddr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.txt");
        std::fs::write(&path, "msd_2\n0 0\n0 -> 5\n").unwrap();
        assert_eq!(
            read_automaton_txt(&path)
                .expect_err("undeclared")
                .to_string(),
            format!(
                "State 5 is used but never declared anywhere in file: {}",
                path.display()
            )
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bare_msd_defaults_to_base_2() {
        let (alphabet, msd, _, _) = parse_header("msd", None, &mut BTreeSet::new()).unwrap();
        assert_eq!(alphabet, vec![vec![0, 1]]);
        assert_eq!(msd, vec![Some(true)]);
    }

    #[test]
    fn bare_lsd_defaults_to_base_2() {
        let (alphabet, msd, _, _) = parse_header("lsd", None, &mut BTreeSet::new()).unwrap();
        assert_eq!(alphabet, vec![vec![0, 1]]);
        assert_eq!(msd, vec![Some(false)]);
    }

    #[test]
    fn msd_k_and_lsd_k_parse_explicit_bases() {
        let (alphabet, msd, _, _) =
            parse_header("msd_5 lsd_3", None, &mut BTreeSet::new()).unwrap();
        assert_eq!(alphabet, vec![vec![0, 1, 2, 3, 4], vec![0, 1, 2]]);
        assert_eq!(msd, vec![Some(true), Some(false)]);
    }

    #[test]
    fn unsupported_numeration_is_explicit_not_silent() {
        // Without a `custom_bases_dir`, `msd_fib` is still `UnsupportedNumeration`,
        // exactly as before U13 -- see `custom_base_header_is_resolved_with_a_dir` below
        // for the WITH-a-directory case.
        assert!(matches!(
            parse_header("msd_fib", None, &mut BTreeSet::new()),
            Err(ReadError::UnsupportedNumeration(_))
        ));
        assert!(matches!(
            parse_header("msd5", None, &mut BTreeSet::new()), // no-underscore form — deliberately unsupported
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
            Err(ReadError::FileHasConflict { line: 2, .. })
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
            Err(ReadError::UndeclaredDestState { state: 5, .. })
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

    // --- custom-base header parsing (U13) ------------------------------------

    #[test]
    fn reads_custom_base_header_msd_fib_end_to_end() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-fib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cb_dir = dir.join("Custom Bases");
        std::fs::create_dir_all(&cb_dir).unwrap();
        std::fs::copy(fixture("msd_fib.txt"), cb_dir.join("msd_fib.txt")).unwrap();
        std::fs::copy(
            fixture("msd_fib_addition.txt"),
            cb_dir.join("msd_fib_addition.txt"),
        )
        .unwrap();

        let path = dir.join("main.txt");
        // Content is irrelevant to header resolution -- a single-track automaton over
        // the (real, {0,1}-alphabet) msd_fib numeration, self-looping on everything.
        std::fs::write(&path, "msd_fib\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();

        let a = read_automaton_txt_with_custom_bases(&path, &cb_dir).unwrap();
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.msd, vec![Some(true)]);
        assert!(a.fa.is_accepting(a.fa.q0));

        // Same file through the plain (no-directory) entry point is unaffected --
        // U13 is additive, not a behavior change to the pre-existing one.
        assert!(matches!(
            read_automaton_txt(&path),
            Err(ReadError::UnsupportedNumeration(ref s)) if s == "msd_fib"
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// U27 fix, found by the Tier-1 golden corpus. The reader used to leave
    /// [`wr_core::automaton::Automaton::all_reps`] empty even for a custom-base header, so an
    /// automaton loaded from `Automata Library/foo.txt` carried no valid-representation
    /// restriction — and `Automaton::apply_all_representations` (which every `~`/`=>`/`A`
    /// runs) therefore silently did nothing to it. Observable effect: complementing an
    /// `msd_fib` library automaton admitted words with a `11` substring, i.e. strings that are
    /// not Zeckendorf representations at all. `?msd_fib ~$fibonacci_in(m,1,n)` came out with
    /// 10 states instead of real Walnut's 5, and 12 corpus fixtures (352-371) diverged.
    #[test]
    fn a_custom_base_header_carries_its_valid_representation_restriction() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-reps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cb_dir = dir.join("Custom Bases");
        std::fs::create_dir_all(&cb_dir).unwrap();
        std::fs::copy(fixture("msd_fib.txt"), cb_dir.join("msd_fib.txt")).unwrap();
        std::fs::copy(
            fixture("msd_fib_addition.txt"),
            cb_dir.join("msd_fib_addition.txt"),
        )
        .unwrap();

        let path = dir.join("two_track.txt");
        std::fs::write(
            &path,
            "msd_fib msd_fib\n\n0 1\n0 0 -> 0\n0 1 -> 0\n1 0 -> 0\n1 1 -> 0\n",
        )
        .unwrap();
        let a = read_automaton_txt_with_custom_bases(&path, &cb_dir).unwrap();
        assert_eq!(a.all_reps.len(), 2, "one entry per track");
        assert!(
            a.all_reps.iter().all(|r| r.is_some()),
            "every msd_fib track must carry the base's all-representations automaton"
        );

        // A standard base declares no such restriction (Java: `allRepresentations` stays
        // null unless a `Custom Bases/<name>.txt` exists), and an explicit `{...}` track
        // has no number system at all.
        let plain = dir.join("plain.txt");
        std::fs::write(&plain, "msd_2 {0,1}\n\n0 1\n0 0 -> 0\n").unwrap();
        let p = read_automaton_txt_with_custom_bases(&plain, &cb_dir).unwrap();
        assert_eq!(p.all_reps.len(), 2);
        assert!(p.all_reps.iter().all(|r| r.is_none()));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// U23 review fix, finding #1. The reader used to discard the resolved
    /// [`NumberSystem`]'s NAME, keeping only `(alphabet, msd)` — which for `msd_fib` is
    /// `([0, 1], true)`, i.e. byte-identical to `msd_2`'s. Downstream,
    /// `NumberSystem.isNSDiffering` compares by name, so `union`/`intersect`/`concat`
    /// silently accepted `msd_fib` and `msd_2` operands as "the same number system".
    #[test]
    fn a_custom_base_header_records_its_real_name_not_the_base_k_lookalike() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cb_dir = dir.join("Custom Bases");
        std::fs::create_dir_all(&cb_dir).unwrap();
        std::fs::copy(fixture("msd_fib.txt"), cb_dir.join("msd_fib.txt")).unwrap();
        std::fs::copy(
            fixture("msd_fib_addition.txt"),
            cb_dir.join("msd_fib_addition.txt"),
        )
        .unwrap();

        let fib_path = dir.join("fib.txt");
        std::fs::write(&fib_path, "msd_fib\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();
        let fib = read_automaton_txt_with_custom_bases(&fib_path, &cb_dir).unwrap();

        let two_path = dir.join("two.txt");
        std::fs::write(&two_path, "msd_2\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();
        let two = read_automaton_txt_with_custom_bases(&two_path, &cb_dir).unwrap();

        // Identical by every fact the pre-fix `Automaton` carried...
        assert_eq!(fib.alphabet, two.alphabet);
        assert_eq!(fib.msd, two.msd);
        // ...and distinguishable only by the name, which is now kept.
        assert_eq!(fib.track_ns_names(), vec![Some("msd_fib".to_string())]);
        assert_eq!(two.track_ns_names(), vec![Some("msd_2".to_string())]);

        // The writer emits `numberSystem.toString()` (`AutomatonWriter.java:72`), so the
        // custom base round-trips instead of being flattened to `msd_2`.
        let mut fib_out = fib;
        let out_path = dir.join("fib_out.txt");
        crate::writer::write_automaton_txt(&mut fib_out, &out_path).unwrap();
        let text = std::fs::read_to_string(&out_path).unwrap();
        assert_eq!(text.lines().next().unwrap(), "msd_fib");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `NumberSystem.normalizeNumberSystemToken` (`:284-286`) maps a bare `msd`/`lsd` to
    /// `msd_2`/`lsd_2`, so those — not the bare words — are the names Java compares. This
    /// also keeps the writer's output stable: a bare-`msd` header still writes back as
    /// `msd_2`, exactly as before this unit.
    #[test]
    fn a_bare_msd_or_lsd_header_is_named_msd_2_or_lsd_2() {
        let (_, _, names, _) = parse_header("msd lsd {0, 1}", None, &mut BTreeSet::new()).unwrap();
        assert_eq!(
            names,
            vec![Some("msd_2".to_string()), Some("lsd_2".to_string()), None]
        );
    }

    /// The [`CustomBaseResolver`] seam really is consulted **per file name**, not collapsed
    /// back into one directory — the property `wr-cli`'s session needs in order to give a
    /// nested header the same session-shadows-global precedence Java's `globalOrSessionFile`
    /// gives a top-level query token.
    ///
    /// Modelled on that layout: two directories, the "session" one holding only ONE of the two
    /// files `msd_fib` needs. A resolver that preferred a single directory wholesale (either
    /// one) would fail — the pieces must be picked up from different places.
    #[test]
    fn a_resolver_is_consulted_per_file_name_not_per_directory() {
        struct SplitResolver {
            preferred: PathBuf,
            fallback: PathBuf,
        }
        impl CustomBaseResolver for SplitResolver {
            fn resolve(&self, filename: &str) -> PathBuf {
                let p = self.preferred.join(filename);
                if p.is_file() {
                    p
                } else {
                    self.fallback.join(filename)
                }
            }
        }

        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-split-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let (global, session) = (dir.join("global"), dir.join("session"));
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(&session).unwrap();
        // The adder only in "global", the all-representations file only in "session".
        std::fs::copy(
            fixture("msd_fib_addition.txt"),
            global.join("msd_fib_addition.txt"),
        )
        .unwrap();
        std::fs::copy(fixture("msd_fib.txt"), session.join("msd_fib.txt")).unwrap();

        let path = dir.join("main.txt");
        std::fs::write(&path, "msd_fib\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();

        let resolver = SplitResolver {
            preferred: session.clone(),
            fallback: global.clone(),
        };
        let a = read_automaton_txt_with_custom_base_resolver(&path, &resolver).unwrap();
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.msd, vec![Some(true)]);

        // Neither directory alone is sufficient: the adder is mandatory, so the
        // session-only view cannot resolve `msd_fib` at all.
        assert!(matches!(
            read_automaton_txt_with_custom_bases(&path, &session),
            Err(ReadError::NumSys(NumSysError::NotDefined(_)))
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_base_header_round_trips_an_alphabet_larger_than_two() {
        // Not every shipped custom base is `{0, 1}` -- `msd_kim`/`msd_pell` are
        // `{0, 1, 2}`, `msd_ns`/`msd_tib` are `{0, 1, 2, 3}` (verified against the real
        // `walnut-java` `Custom Bases/*.txt` files). This proves
        // `HeaderToken::Ns::alphabet` (and therefore the resulting `Automaton::alphabet`)
        // genuinely round-trips a size-3 custom-base alphabet, not just the `{0, 1}`
        // shape every OTHER fixture in this test module happens to use. A hand-authored
        // (not walnut-java-sourced) 3-track adder is enough -- `NumberSystem::
        // with_custom_base_files` only structurally validates the adder (exactly 3
        // tracks, alphabet contains 0 and 1, all three tracks' alphabets set-equal), it
        // never checks the transition table actually computes addition.
        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-wide-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("msd_wide_addition.txt"),
            "{0, 1, 2} {0, 1, 2} {0, 1, 2}\n\n0 1\n* * * -> 0\n",
        )
        .unwrap();

        let path = dir.join("main.txt");
        std::fs::write(&path, "msd_wide\n\n0 1\n0 -> 0\n1 -> 0\n2 -> 0\n").unwrap();

        let a = read_automaton_txt_with_custom_bases(&path, &dir).unwrap();
        assert_eq!(a.alphabet, vec![vec![0, 1, 2]]);
        assert_eq!(a.msd, vec![Some(true)]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_automaton_dfa_txt_with_custom_bases_resolves_the_header_too() {
        // `readAutomaton` (and therefore custom-base resolution) is shared between the
        // `Automaton(String)` and `AutomatonDFA(String)` constructors in Java.
        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-dfa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cb_dir = dir.join("Custom Bases");
        std::fs::create_dir_all(&cb_dir).unwrap();
        std::fs::copy(fixture("msd_fib.txt"), cb_dir.join("msd_fib.txt")).unwrap();
        std::fs::copy(
            fixture("msd_fib_addition.txt"),
            cb_dir.join("msd_fib_addition.txt"),
        )
        .unwrap();

        let path = dir.join("main.txt");
        std::fs::write(&path, "msd_fib\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();

        let dfa = read_automaton_dfa_txt_with_custom_bases(&path, &cb_dir).unwrap();
        assert!(dfa.automaton().fa.is_deterministic());
        assert_eq!(dfa.automaton().alphabet, vec![vec![0, 1]]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_base_complement_fallback_reverses_the_opposite_direction_file() {
        // Only the MSD-direction file exists; loading "lsd_test" must fall back to the
        // "msd_test" complement and reverse it (`NumberSystem.loadAutomatonOrNull`'s
        // "try the complement" branch, `:313-316`).
        let dir =
            std::env::temp_dir().join(format!("wr-io-test-cb-complement-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(
            fixture("msd_fib_addition.txt"),
            dir.join("msd_test_addition.txt"),
        )
        .unwrap();

        let ns = load_custom_base("lsd_test", &CustomBasesDir(&dir), &mut BTreeSet::new()).unwrap();
        // `isMsd` comes from the NAME passed in, independent of which file was actually
        // loaded (Java: `isMsd = msdOrLsd.equals(MSD)`, computed before any file I/O).
        assert!(!ns.is_msd());
        assert_eq!(ns.name(), "lsd_test");
        assert_eq!(ns.get_alphabet(), &[0, 1]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_base_negative_name_is_rejected_before_any_file_io() {
        // An empty directory: if the `_neg_` check ran AFTER file probing, this would
        // fail with a `NotDefined`/file-not-found error instead of the intended
        // `UnsupportedNegativeBase` -- pins the ordering, not just the outcome.
        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-neg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            load_custom_base("msd_neg_fib", &CustomBasesDir(&dir), &mut BTreeSet::new()),
            Err(ReadError::NumSys(NumSysError::UnsupportedNegativeBase(_)))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_base_name_with_no_matching_files_is_not_defined() {
        let dir =
            std::env::temp_dir().join(format!("wr-io-test-cb-missing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap(); // empty: no files at all
        assert!(matches!(
            load_custom_base(
                "msd_nonexistent",
                &CustomBasesDir(&dir),
                &mut BTreeSet::new()
            ),
            Err(ReadError::NumSys(NumSysError::NotDefined(_)))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_base_self_referential_addition_file_is_a_clean_cycle_error_not_a_stack_overflow() {
        // `Custom Bases/msd_selfref_addition.txt`'s OWN header names `msd_selfref` again --
        // resolving `msd_selfref` would otherwise recurse into resolving `msd_selfref`
        // resolving `msd_selfref`... forever. No real shipped `walnut-java` custom-base
        // file does this (verified while porting U13), but a malformed hand-written one
        // could, and without a guard that's an uncatchable Rust stack overflow rather than
        // a normal `Result`. The cycle is caught before the recursively-read file's own
        // BODY is ever parsed (the re-entrant `load_custom_base` call happens while parsing
        // that file's header), so the body content here is irrelevant to the test.
        let dir = std::env::temp_dir().join(format!("wr-io-test-cb-cycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("msd_selfref_addition.txt"),
            "msd_selfref\n\n0 0\n0 0 0 -> 0\n",
        )
        .unwrap();

        assert!(matches!(
            load_custom_base("msd_selfref", &CustomBasesDir(&dir), &mut BTreeSet::new()),
            Err(ReadError::CustomBaseCycle(ref s)) if s == "msd_selfref"
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_base_cycle_guard_does_not_reject_the_same_name_resolved_twice_sequentially() {
        // The guard tracks names currently IN PROGRESS on the call stack, not every name
        // ever seen -- resolving `msd_fib` twice in a row (e.g. two header tracks that both
        // name it) must succeed both times, not fail the second time as a false-positive
        // "cycle".
        let dir =
            std::env::temp_dir().join(format!("wr-io-test-cb-cycle-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(fixture("msd_fib.txt"), dir.join("msd_fib.txt")).unwrap();
        std::fs::copy(
            fixture("msd_fib_addition.txt"),
            dir.join("msd_fib_addition.txt"),
        )
        .unwrap();

        let mut in_progress = BTreeSet::new();
        assert!(load_custom_base("msd_fib", &CustomBasesDir(&dir), &mut in_progress).is_ok());
        assert!(in_progress.is_empty(), "guard must clean up after success");
        assert!(load_custom_base("msd_fib", &CustomBasesDir(&dir), &mut in_progress).is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- readTransducer / readComments (U13) ---------------------------------

    #[test]
    fn reads_the_real_runsum2_transducer_fixture() {
        // `Transducer Library/RUNSUM2.txt`: {0,1}, two states, each transition negates
        // the input bit as its output on a self/cross loop:
        //   0: 0 -> 0 / 0, 1 -> 1 / 1
        //   1: 0 -> 1 / 1, 1 -> 0 / 0
        // The alphabet is exactly {0, 1} in that order, so encoded symbol == digit value
        // (position-in-alphabet), and destination/state ids are exactly 0 and 1 as
        // declared -- both asserted directly below rather than re-deriving an encoder.
        let t = read_transducer_txt(fixture("RUNSUM2.txt")).unwrap();
        assert_eq!(t.alphabet, vec![vec![0, 1]]);
        assert_eq!(t.msd, vec![None]); // explicit-set alphabet, not a numeration
        assert_eq!(t.q, 2);
        assert_eq!(t.q0, 0);

        assert_eq!(t.d[0].get(&0), Some(&vec![0]));
        assert_eq!(t.d[0].get(&1), Some(&vec![1]));
        assert_eq!(t.d[1].get(&0), Some(&vec![1]));
        assert_eq!(t.d[1].get(&1), Some(&vec![0]));

        assert_eq!(t.sigma[0].get(&0), Some(&0));
        assert_eq!(t.sigma[0].get(&1), Some(&1));
        assert_eq!(t.sigma[1].get(&0), Some(&1));
        assert_eq!(t.sigma[1].get(&1), Some(&0));
    }

    #[test]
    fn transducer_transitions_are_not_auto_determinized() {
        // Unlike `read_automaton_txt`, `readTransducer` has no auto-determinize step --
        // MULTIPLE destinations declared on a SINGLE transition line survive as genuine
        // nondeterminism in `d` (`AutomatonReader.readTransducer`'s `dest` list, `:249`,
        // is stored as-is via `Map.put`; a line's own dest list is never collapsed).
        let dir =
            std::env::temp_dir().join(format!("wr-io-test-transducer-nfa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nfa_transducer.txt");
        std::fs::write(
            &path,
            "{0, 1}\n\n0\n0 -> 0 1 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 0\n",
        )
        .unwrap();
        let t = read_transducer_txt(&path).unwrap();
        assert_eq!(t.d[0].get(&0), Some(&vec![0, 1]));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transducer_repeated_symbol_across_lines_is_last_line_wins_not_a_union() {
        // `AutomatonReader.readTransducer` (`:249-250`):
        // `currentStateTransitions.put(encode(i), dest)` -- Java `Map.put` REPLACES,
        // it does not accumulate. Two separate transition lines from the same state
        // declaring the SAME input symbol must leave only the LATER line's destination
        // in `d`, discarding the earlier one -- unlike `read_automaton_txt`'s NFA `d`
        // table, which genuinely unions repeated-symbol lines.
        let dir = std::env::temp_dir().join(format!(
            "wr-io-test-transducer-last-wins-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("last_wins_transducer.txt");
        std::fs::write(
            &path,
            "{0, 1}\n\n0\n0 -> 0 / 0\n0 -> 1 / 1\n\n1\n0 -> 1 / 0\n",
        )
        .unwrap();
        let t = read_transducer_txt(&path).unwrap();
        // The later line ("0 -> 1 / 1") wins outright: destination AND output.
        assert_eq!(t.d[0].get(&0), Some(&vec![1]));
        assert_eq!(t.sigma[0].get(&0), Some(&1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transducer_arity_mismatch_is_an_error() {
        let dir = std::env::temp_dir().join(format!(
            "wr-io-test-transducer-arity-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.txt");
        std::fs::write(&path, "{0, 1} {0, 1}\n\n0\n0 -> 0 / 0\n").unwrap();
        assert!(matches!(
            read_transducer_txt(&path),
            Err(ReadError::ArityMismatch {
                expected: 2,
                got: 1,
                ..
            })
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_comments_collects_every_comment_line_in_order() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-comments-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commented.txt");
        std::fs::write(
            &path,
            "# first comment\nmsd_2\n\n0 0\n# a mid-file comment\n0 -> 0\n1 -> 0\n#trailing, no space\n",
        )
        .unwrap();
        let comments = read_comments(&path).unwrap();
        assert_eq!(
            comments,
            "# first comment\n# a mid-file comment\n#trailing, no space"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_comments_on_a_file_with_no_comments_is_empty() {
        let dir =
            std::env::temp_dir().join(format!("wr-io-test-comments-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plain.txt");
        std::fs::write(&path, "msd_2\n\n0 0\n0 -> 0\n1 -> 0\n").unwrap();
        assert_eq!(read_comments(&path).unwrap(), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- AutomatonDFA(String) / readAutomatonDFAFromFile (U13) ---------------

    #[test]
    fn read_automaton_dfa_txt_on_an_already_deterministic_fixture() {
        let dfa = read_automaton_dfa_txt(fixture("automaton2.txt")).unwrap();
        assert!(dfa.automaton().fa.is_deterministic());
        assert_eq!(dfa.automaton().alphabet, vec![vec![0, 1, 2]; 4]);
    }

    #[test]
    fn read_automaton_dfa_txt_auto_determinizes_an_nfa_file() {
        let dir = std::env::temp_dir().join(format!("wr-io-test-dfa-nfa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nfa.txt");
        std::fs::write(
            &path,
            "msd_2\n\n0 0\n0 -> 0\n1 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();
        let dfa = read_automaton_dfa_txt(&path).unwrap();
        assert!(dfa.automaton().fa.is_deterministic());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_automaton_dfa_txt_on_a_genuine_nfao_file_silently_determinizes_instead_of_erroring_wb022(
    ) {
        // `docs/WALNUT-BUGS.md` WB-022: real Java's `readAutomaton` would throw
        // `WalnutException.nonDeterministicO` for this file -- nondeterministic
        // transitions (state 0 has two destinations for symbol 0) AND a genuine DFAO
        // output (state 1's declared output is 2, not just 0/1). This port has no
        // `is_fao` guard in `read_automaton_txt_impl` and silently determinizes instead,
        // collapsing the real output 2 down to plain boolean acceptance. This test pins
        // the CURRENT (divergent) behavior explicitly, so a future fix that adds the
        // missing guard is a visible, intentional change (this test will fail and need
        // updating), not something nobody notices moving.
        let dir = std::env::temp_dir().join(format!("wr-io-test-nfao-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nfao.txt");
        std::fs::write(
            &path,
            "msd_2\n\n0 0\n0 -> 0\n0 -> 1\n1 -> 0\n\n1 2\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();

        // Sanity: the parsed shape really is a genuine NFAO before determinizing --
        // confirms the test fixture actually exercises the gap, not some other shape.
        // (Read directly via the plain, non-DFA-typed entry point is not possible here
        // since `read_automaton_txt` itself already auto-determinizes; instead this
        // just documents the fixture's intent above and relies on the DFA-typed
        // result's boolean-only acceptance below as the observable symptom.)

        let dfa = read_automaton_dfa_txt(&path).unwrap();
        assert!(dfa.automaton().fa.is_deterministic());
        // The real output value 2 is gone -- collapsed to plain 0/1 acceptance. Real
        // Walnut would have thrown `nonDeterministicO` instead of reaching this point.
        assert!(dfa.automaton().fa.o.iter().all(|&o| o == 0 || o == 1));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_automaton_dfa_txt_true_false_fixtures() {
        let dfa_true = read_automaton_dfa_txt(fixture("automaton189.txt")).unwrap();
        assert!(dfa_true.automaton().is_true_false_automaton());
        assert!(dfa_true.automaton().is_true_automaton());

        let dfa_false = read_automaton_dfa_txt(fixture("automaton214.txt")).unwrap();
        assert!(dfa_false.automaton().is_true_false_automaton());
        assert!(!dfa_false.automaton().is_true_automaton());
    }

    /// The string-taking entry points added for the Tier-5 fuzz harness must be the
    /// *same* parse as the path-taking ones, not a second implementation that can drift.
    /// Checked on the real fixtures, both the ordinary and the trivial-automaton shapes,
    /// and on an error case (so the error path is confirmed shared too).
    #[test]
    fn read_automaton_from_str_matches_the_path_entry_point() {
        for name in ["automaton2.txt", "automaton189.txt", "automaton214.txt"] {
            let path = fixture(name);
            let from_path = read_automaton_txt(&path).unwrap();
            let from_str =
                read_automaton_from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            // Compared by `Debug` rather than `PartialEq`: neither `Automaton` (a
            // `wr-core` type) nor `TransducerData` derives `Eq`, and widening either
            // type's public trait surface just to write this assertion would be a
            // gratuitous API change. `Debug` is total over both and is exactly as
            // discriminating for "same parse result" purposes here.
            assert_eq!(format!("{from_path:?}"), format!("{from_str:?}"), "{name}");
        }

        // Error parity: an empty input is `EmptyFile` either way, and a custom-base
        // header is `UnsupportedNumeration` either way (the resolver-free forms).
        assert!(matches!(
            read_automaton_from_str(""),
            Err(ReadError::EmptyFile { .. })
        ));
        assert!(matches!(
            read_automaton_from_str("msd_fib\n\n0 0\n"),
            Err(ReadError::UnsupportedNumeration(_))
        ));
    }

    #[test]
    fn read_transducer_from_str_matches_the_path_entry_point() {
        let path = fixture("RUNSUM2.txt");
        let from_path = read_transducer_txt(&path).unwrap();
        let from_str = read_transducer_from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(format!("{from_path:?}"), format!("{from_str:?}"));
    }
}
