// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Writer for Walnut's `.txt` automaton format, Graphviz `.gv`, and Brics-style `.ba`.
//!
//! Ports `Automata/Writer/AutomatonWriter.java` (175 LOC) — the writer half of
//! `wr-io`'s I/O surface; see [`crate::reader`] for the `.txt` reader this module's
//! `.txt` output must round-trip through.
//!
//! # `.txt` format
//!
//! [`write_txt`]/[`write_automaton_txt`] port `writeTxtFormatToStream`/
//! `writeToTxtFormat`. A trivial (TRUE/FALSE) automaton writes just the bare word
//! `true`/`false`, no trailing newline (`FA.trueFalseString`) — matching
//! [`crate::reader::read_automaton_txt`]'s trivial-file branch and the 85-of-638
//! golden `automaton*` fixtures that consist of exactly this. Otherwise: `canonize()`
//! (sorts the label, canonicalizes the `Fa`), then a header line (per-track either an
//! explicit `{v1, v2, ...}` set or a `msd_<k>`/`lsd_<k>` numeration token), then one
//! blank-line-separated block per state (`<id> <output>` followed by its transition
//! lines, grouped by distinct input symbol with all destinations on one line — NFA
//! nondeterminism is multiple destinations per line, not multiple lines per symbol).
//!
//! **A faithfully-preserved Java quirk**: [`write_alphabet`]'s per-track spacing is
//! asymmetric between the two header-token kinds. An explicit-set token always
//! appends its own trailing space (`"{...} "`); a numeration token only prepends a
//! space when it is NOT the first track (`if (i > 0) out.write(" ")`). So an
//! explicit-set track immediately followed by a numeration track prints **two**
//! spaces between them (`"{0, 1}  msd_3"`), not one — mechanically ported rather than
//! "cleaned up," per `CLAUDE.md`'s mechanical-port-first rule. Harmless for
//! round-tripping: [`crate::reader::parse_header`] treats any run of whitespace as a
//! single separator.
//!
//! **Number-system names**: Java writes `numberSystem.toString()` (`AutomatonWriter.java:72`),
//! i.e. `NumberSystem.getName()`. Since U23's review fixes, `wr_core::Automaton` carries
//! that name per track ([`wr_core::automaton::Automaton::ns_name`], surfaced through
//! `track_ns_names()`), so this writer emits it verbatim — a custom base (`msd_fib`, ...)
//! now round-trips through write→read instead of being flattened to the `msd_2` its
//! alphabet cardinality alone suggests. Where no name was recorded (an automaton this
//! crate built in memory, always on a plain base), it falls back to the canonical
//! `msd_<base>`/`lsd_<base>` reconstruction, which is exactly what Java's name would be
//! (`NumberSystem.normalizeNumberSystemToken` maps bare `msd`/`lsd` to `msd_2`/`lsd_2`),
//! so no previously-correct output changes.
//!
//! # `.gv` (Graphviz) format
//!
//! [`write_gv`]/[`write_automaton_gv`] port `writeToGV`. Straightforward text
//! formatting (no external crate) reproduced byte-for-byte against real
//! `walnut-java` output (see below) — including a real Java inconsistency between
//! its trivial and non-trivial branches: the trivial branch's initial-state arrow is
//! `"qi ->0;"` (no space before the hardcoded literal `0`), the non-trivial branch's
//! is `"qi -> " + q0 + ";"` (a space, and the REAL `q0`, not hardcoded). Graphviz
//! doesn't care about the whitespace and the hardcoded `0` only matters for a
//! (nonexistent, since a trivial automaton has no real states) alternate `q0`, so this
//! is a cosmetic quirk, not a `docs/WALNUT-BUGS.md`-worthy defect — ported verbatim as
//! two distinct literal strings, not unified.
//!
//! # `.ba` (Brics-style AutomataLib `BAWriter`) format
//!
//! [`export_to_ba`]/[`export_automaton_to_ba`] port `exportToBA`. **This is
//! AutomataLib's own `net.automatalib.serialization.ba.BAWriter` format, not a Rust
//! crate dependency** — the plan (`.claude/plans/synthetic-prancing-aurora.md`, U12's
//! row) explicitly retiered this unit because of it: no spec document exists, only
//! real output to match. Derived here from two ground-truth sources, not guessed:
//!
//! 1. **`javap -c` on the real, already-resolved
//!    `automata-serialization-ba-0.12.1.jar`** (present in this machine's local Maven
//!    cache, `~/.m2/repository/net/automatalib/automata-serialization-ba/0.12.1/`) —
//!    `BAWriter.writeAutomaton`'s bytecode is short and completely unambiguous (three
//!    straight-line helpers: `writeInitialState`/`writeTransitions`/
//!    `writeFinalStates`, no branches beyond the ones described below).
//! 2. **Empirical runs of the real `walnut-java` CLI's own compiled classes**
//!    (`target/Walnut-all.jar`, built from this checkout) against hand-built
//!    `Automaton`/`CompactNFA` inputs, to nail down every ordering guarantee the
//!    bytecode alone doesn't settle (Java `Set`/`Alphabet` iteration order is not
//!    specified by the bytecode) and to confirm the trivial-automaton edge case (see
//!    WB-021 below) actually runs rather than crashing. Three probes, each run via a
//!    small driver class compiled and executed against the real jars:
//!    - a real multi-track deterministic automaton read from
//!      `src/test/resources/integrationTests/automaton2.txt` (msd_3, 4 tracks) via
//!      `new Automaton(address)` then `AutomatonWriter.exportToBA` — confirmed the
//!      **symbol** written per transition is the RAW ENCODED integer key from `FA.t`
//!      (`FA.FAtoCompactNFA`'s `entry.getIntKey()`, not the decoded per-track digit
//!      tuple `.txt`/`.gv` use), by hand-computing the expected mixed-radix values
//!      (track 0 fastest-varying, matching [`wr_core::automaton::Automaton::encode`])
//!      and matching them against the real output exactly (symbols `0`, `27`, `76`,
//!      `61` for the file's four transitions).
//!    - a hand-built `CompactNFA` with destinations added out of numeric order under
//!      one (state, symbol) pair, and states added in an order that doesn't match
//!      their eventual accept/non-accept partition — confirmed **ascending** order on
//!      all three axes real `BAWriter` output can vary on: source state, input
//!      symbol, and destination-state list (all backed by array/bitset-shaped
//!      AutomataLib collections, not a Java `HashSet`'s unspecified order).
//!    - a hand-built `CompactNFA` with transitions added from out-of-order SOURCE
//!      states specifically (to isolate that axis from the symbol/dest one above) —
//!      confirmed source-state iteration is also ascending numeric order, not
//!      insertion order.
//!
//! Confidence: **high** — every format detail below is either read directly off
//! unambiguous bytecode, or confirmed by a real run whose output is reproduced in
//! this module's tests (`crates/wr-io/tests/fixtures/*.ba`, copied byte-for-byte from
//! the real `walnut-java` runs described above).
//!
//! Format, precisely:
//! - **Initial state** (`writeInitialState`): exactly one line, the initial state's
//!   integer id, IF there is exactly one initial state (`Set.size() == 1`); if there
//!   are zero, nothing is written for this section (this crate's [`Fa`] always has
//!   exactly one `q0` field, so the "zero initial states" case never actually arises
//!   here — only Java's more general `FiniteStateAcceptor`, which can have zero,
//!   needs it); if there are more than one, Java throws
//!   `IllegalArgumentException("The BA format only allows for at most one initial
//!   state")` — likewise unreachable from this crate's single-`q0` [`Fa`].
//! - **Transitions** (`writeTransitions`): for each state in ascending id order, for
//!   each symbol `0..alphabet_size` in ascending order, for each destination in
//!   ascending id order: one line `<symbol>,<source>-><dest>`. `<symbol>` is the raw
//!   encoded transition key (see above) — **not** decoded into per-track digits, even
//!   for a multi-track automaton; a `.ba` file has no notion of tracks at all.
//! - **Final states** (`writeFinalStates`): a real, faithfully-ported AutomataLib
//!   behavior that looks surprising in isolation — if EVERY state is accepting (a
//!   vacuous truth when there are zero states), **nothing is written for this
//!   section at all** (not "every state," just nothing); otherwise, one line per
//!   accepting state, ascending id order.
//!
//! ## WB-021 (logged to `docs/WALNUT-BUGS.md`): `exportToBA` has no
//! `TRUE_FALSE_AUTOMATON` guard, unlike its two siblings
//!
//! Both `writeToTxtFormat` and `writeToGV` special-case `FA.isTRUE_FALSE_AUTOMATON()`
//! before touching `Q`/alphabet/transition state (which are meaningless/stale on a
//! trivial automaton — see [`crate::reader`]'s and `wr_core::fa`'s own docs on this).
//! `exportToBA` has no such guard. Empirically confirmed (not just read) against the
//! real `walnut-java` CLI: it does not crash, but for BOTH the TRUE and the FALSE
//! automaton it silently produces **byte-identical** output — just `"0\n"` (the
//! default-valued, never-actually-existing "state 0" as the sole initial-state line;
//! zero transitions since `FA.t` is empty; the final-states section elided by the
//! "vacuously all states accept" rule above, since there are zero states to disagree).
//! The TRUE and FALSE automata are indistinguishable in `.ba` output, and neither
//! carries any information about which automaton it was. Reachable from the plain
//! CLI: `[export ... BA]` on any query whose result happens to be the trivial
//! TRUE/FALSE automaton, which the golden corpus shows is common (13% of
//! `automaton*` fixtures per [`crate::reader`]'s docs). This crate's
//! [`export_to_ba`] reproduces the same behavior verbatim (it never special-cases
//! [`Fa::is_true_false_automaton`], exactly matching Java's omission), per
//! `CLAUDE.md`'s mechanical-port rule — see `docs/WALNUT-BUGS.md`'s WB-021 entry.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use wr_core::automaton::Automaton;
use wr_core::fa::Fa;
use wr_core::util::{generic_list_string, to_transition_label, to_tuple};

// ---------------------------------------------------------------------------
// `.txt` format
// ---------------------------------------------------------------------------

/// `AutomatonWriter.writeTxtFormatToStream` (`:48-58`). Mutates `automaton` via
/// `canonize()` on the non-trivial branch, matching Java exactly (`canonize` sorts
/// the label and canonicalizes the `Fa` in place).
pub fn write_txt<W: Write>(automaton: &mut Automaton, out: &mut W) -> io::Result<()> {
    if automaton.fa.is_true_false_automaton() {
        write!(out, "{}", automaton.fa.true_false_string())
    } else {
        automaton.canonize();
        write_alphabet(automaton, out)?;
        for q in 0..automaton.fa.q {
            write_state(automaton, out, q)?;
        }
        Ok(())
    }
}

/// `AutomatonWriter.writeToTxtFormat` (`:40-46`) — opens `path` and writes through
/// [`write_txt`].
///
/// **Deliberate idiom deviation from Java, matching this crate's existing
/// convention (see [`crate::reader::read_automaton_txt`]):** Java catches the
/// `IOException` and only logs it (`Logging.printTruncatedStackTrace`), silently
/// continuing rather than propagating. This crate follows `wr-io`'s established
/// Result-propagating idiom instead (the reader never swallows I/O errors either) —
/// a platform-plumbing idiom choice, not a divergence in the actual `.txt` format or
/// algorithm.
pub fn write_automaton_txt<P: AsRef<Path>>(automaton: &mut Automaton, path: P) -> io::Result<()> {
    let file = File::create(path)?;
    let mut out = BufWriter::new(file);
    write_txt(automaton, &mut out)?;
    out.flush()
}

/// `AutomatonWriter.writeAlphabet` (`:60-76`) — see this module's docs for the
/// faithfully-preserved asymmetric-spacing quirk.
fn write_alphabet<W: Write>(automaton: &Automaton, out: &mut W) -> io::Result<()> {
    // `numberSystem.toString()` (`:72`) == `NumberSystem.getName()`; see this module's docs.
    let names = automaton.track_ns_names();
    for (i, name) in names.iter().enumerate() {
        match name {
            Some(name) => {
                if i > 0 {
                    write!(out, " ")?;
                }
                out.write_all(name.as_bytes())?;
            }
            None => {
                write!(
                    out,
                    "{{{}}} ",
                    generic_list_string(&automaton.alphabet[i], ", ")
                )?;
            }
        }
    }
    writeln!(out)
}

/// `AutomatonWriter.writeState` (`:78-92`).
///
/// # A transition key this cannot render
///
/// `automaton.fa.d`'s keys are not guaranteed to be valid symbols: a `.txt` file with an
/// out-of-alphabet body digit stores one under `-1` (WB-038, faithfully ported — see
/// [`Automaton::encode_index_of`]). [`Automaton::decode`] **panics** on such a key rather
/// than inventing digits for it, and that is deliberate on both counts:
///
/// * Java does the same thing here — `RichAlphabet.decode`'s `ArrayList.get(-1)` throws
///   `IndexOutOfBoundsException`, `writeToTxtFormat`'s try/catch covers only `IOException`,
///   so the exception escapes to `Prover.readBuffer`'s `catch (RuntimeException)` and the
///   file is left containing whatever had already been written (the `PrintWriter` is
///   closed by try-with-resources, not rolled back). This port's boundary is
///   `wr_cli::prover`'s `Prover::caught`, with the same partially-written-file outcome.
/// * The alternative — mapping it into this function's `io::Result` — would be actively
///   wrong: `wr_cli`'s `is_io_class_error` routes an I/O failure to Java's OUTER
///   `catch (IOException)`, which **ends** the read loop. A `RuntimeException` must not
///   end the session.
fn write_state<W: Write>(automaton: &Automaton, out: &mut W, q: usize) -> io::Result<()> {
    write!(out, "\n{} {}\n", q, automaton.fa.o[q])?;
    for (&sym, dests) in &automaton.fa.d[q] {
        let digits = automaton.decode(sym);
        for d in &digits {
            write!(out, "{} ", d)?;
        }
        write!(out, "->")?;
        for &dest in dests {
            write!(out, " {}", dest)?;
        }
        writeln!(out)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `.gv` (Graphviz) format
// ---------------------------------------------------------------------------

/// `AutomatonWriter.writeToGV`'s stream half (`:101-159`, minus the file-open
/// try/catch — see [`write_automaton_gv`] for that). `predicate` is the label text
/// (Java's `String predicate` parameter — e.g. the query text that produced this
/// automaton); `is_dfao` selects the `"q/output"` node-label form over plain
/// accept/reject circles, matching Java's `boolean isDFAO` parameter (an external
/// fact the caller supplies, not derived from the automaton here, exactly as in
/// Java).
pub fn write_gv<W: Write>(
    automaton: &mut Automaton,
    out: &mut W,
    predicate: &str,
    is_dfao: bool,
) -> io::Result<()> {
    if automaton.fa.is_true_false_automaton() {
        writeln!(out, "digraph G {{")?;
        writeln!(out, "label = \"(): {predicate}\";")?;
        writeln!(out, "rankdir = LR;")?;
        if automaton.fa.is_true_automaton() {
            writeln!(
                out,
                "node [shape = doublecircle, label=\"0\", fontsize=12]0;"
            )?;
        } else {
            writeln!(out, "node [shape = circle, label=\"0\", fontsize=12]0;")?;
        }
        writeln!(out, "node [shape = point ]; qi")?;
        // Faithful quirk: hardcoded `0`, no space before it -- see module docs.
        writeln!(out, "qi ->0;")?;
        if automaton.fa.is_true_automaton() {
            writeln!(out, "0 -> 0[ label = \"*\"];")?;
        }
        writeln!(out, "}}")
    } else {
        automaton.canonize();
        writeln!(out, "digraph G {{")?;
        writeln!(
            out,
            "label = \"{}: {predicate}\";",
            to_tuple(&automaton.label)
        )?;
        writeln!(out, "rankdir = LR;")?;

        let q_count = automaton.fa.q;
        for q in 0..q_count {
            if is_dfao {
                writeln!(
                    out,
                    "node [shape = circle, label=\"{q}/{}\", fontsize=12]{q};",
                    automaton.fa.o[q]
                )?;
            } else if automaton.fa.is_accepting(q) {
                writeln!(
                    out,
                    "node [shape = doublecircle, label=\"{q}\", fontsize=12]{q};"
                )?;
            } else {
                writeln!(out, "node [shape = circle, label=\"{q}\", fontsize=12]{q};")?;
            }
        }

        writeln!(out, "node [shape = point ]; qi")?;
        writeln!(out, "qi -> {};", automaton.fa.q0)?;

        // `TreeMap<Integer, TreeMap<Integer, List<String>>> transitions` -- `BTreeMap`
        // gives the same ascending-key iteration Java's `TreeMap` does.
        let mut transitions: std::collections::BTreeMap<
            usize,
            std::collections::BTreeMap<usize, Vec<String>>,
        > = std::collections::BTreeMap::new();
        for q in 0..q_count {
            let dest_map = transitions.entry(q).or_default();
            for (&sym, dests) in &automaton.fa.d[q] {
                let digits = automaton.decode(sym);
                let label = to_transition_label(&digits);
                for &dest in dests {
                    dest_map.entry(dest).or_default().push(label.clone());
                }
            }
        }
        for q in 0..q_count {
            if let Some(dest_map) = transitions.get(&q) {
                for (&dest, labels) in dest_map {
                    let joined = labels.join(", ");
                    writeln!(out, "{q} -> {dest}[ label = \"{joined}\"];")?;
                }
            }
        }

        writeln!(out, "}}")
    }
}

/// `AutomatonWriter.writeToGV` (`:101-159`) — opens `path` and writes through
/// [`write_gv`]. See [`write_automaton_txt`]'s doc comment for the same
/// Result-propagating idiom deviation from Java's log-and-swallow.
pub fn write_automaton_gv<P: AsRef<Path>>(
    automaton: &mut Automaton,
    path: P,
    predicate: &str,
    is_dfao: bool,
) -> io::Result<()> {
    let file = File::create(path)?;
    let mut out = BufWriter::new(file);
    write_gv(automaton, &mut out, predicate, is_dfao)?;
    out.flush()
}

// ---------------------------------------------------------------------------
// `.ba` (Brics-style AutomataLib `BAWriter`) format
// ---------------------------------------------------------------------------

/// `WalnutException`'s payload for `AutomatonWriter.exportToBA`'s `isDFAO` guard
/// (`:162-164`: `"Can't export DFAO to BA format"`), plus the I/O errors Java's
/// `PrintWriter`/`BufferedOutputStream` machinery would otherwise swallow (see
/// [`write_automaton_txt`]'s doc comment on this crate's Result-propagating idiom).
#[derive(Debug)]
pub enum BaWriteError {
    /// `WalnutException("Can't export DFAO to BA format")`.
    DfaoNotSupported,
    Io(io::Error),
}

impl std::fmt::Display for BaWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaWriteError::DfaoNotSupported => write!(f, "Can't export DFAO to BA format"),
            BaWriteError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BaWriteError {}

impl From<io::Error> for BaWriteError {
    fn from(e: io::Error) -> Self {
        BaWriteError::Io(e)
    }
}

/// `AutomatonWriter.exportToBA`'s stream half (`:161-173`, minus the file-open
/// try/catch — see [`export_automaton_to_ba`] for that, and this module's docs for
/// the format derivation and WB-021). Takes the raw [`Fa`] (not a track-aware
/// [`Automaton`]), matching Java's `FA a` parameter: `.ba` has no track concept, so
/// every transition symbol is written as its raw encoded integer, never decoded.
pub fn export_to_ba<W: Write>(fa: &Fa, out: &mut W, is_dfao: bool) -> Result<(), BaWriteError> {
    if is_dfao {
        return Err(BaWriteError::DfaoNotSupported);
    }

    // `writeInitialState`: this crate's `Fa` always has exactly one `q0`, the
    // `Set.size() == 1` branch -- the 0-or->1-initial-states branches are
    // unreachable here (see module docs).
    writeln!(out, "{}", fa.q0)?;

    // `writeTransitions`: source state ascending, symbol ascending (BTreeMap key
    // order matches both), destination ascending -- all three confirmed by direct
    // empirical probes against the real BAWriter (see module docs).
    for (q, row) in fa.d.iter().enumerate() {
        for (&sym, dests) in row {
            let mut sorted_dests = dests.clone();
            sorted_dests.sort_unstable();
            for dest in sorted_dests {
                writeln!(out, "{sym},{q}->{dest}")?;
            }
        }
    }

    // `writeFinalStates`: vacuously "all accepting" (and hence WRITE NOTHING) when
    // `fa.q == 0`, faithfully reproducing the trivial-automaton edge case WB-021
    // describes.
    let all_accepting = (0..fa.q).all(|q| fa.is_accepting(q));
    if !all_accepting {
        for q in 0..fa.q {
            if fa.is_accepting(q) {
                writeln!(out, "{q}")?;
            }
        }
    }

    Ok(())
}

/// `AutomatonWriter.exportToBA` (`:161-173`) — opens `path` and writes through
/// [`export_to_ba`].
pub fn export_automaton_to_ba<P: AsRef<Path>>(
    fa: &Fa,
    path: P,
    is_dfao: bool,
) -> Result<(), BaWriteError> {
    if is_dfao {
        return Err(BaWriteError::DfaoNotSupported);
    }
    let file = File::create(path)?;
    let mut out = BufWriter::new(file);
    export_to_ba(fa, &mut out, is_dfao)?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::read_automaton_txt;
    use std::collections::BTreeMap;
    use wr_core::equiv::automaton_language_equivalent;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn fixture_bytes(name: &str) -> Vec<u8> {
        std::fs::read(fixture(name)).unwrap()
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wr-io-writer-test-{}-{}", std::process::id(), name));
        dir
    }

    // ------------------------------------------------------------------
    // `.txt` round-trip: write -> read back -> semantic equivalence.
    // ------------------------------------------------------------------

    /// Round-trips a real golden fixture through `write_automaton_txt` then
    /// `read_automaton_txt`, asserting the result is language-equivalent to the
    /// original (per `CLAUDE.md`'s "compare by semantic equivalence" rule).
    fn assert_txt_round_trips(fixture_name: &str, out_name: &str) {
        let mut original = read_automaton_txt(fixture(fixture_name)).unwrap();
        let out_path = temp_path(out_name);
        write_automaton_txt(&mut original.clone(), &out_path).unwrap();
        let mut round_tripped = read_automaton_txt(&out_path).unwrap();
        std::fs::remove_file(&out_path).ok();

        // The equivalence oracle requires a TOTAL DFA; a `.txt` file may legitimately
        // leave transitions partial (an undeclared symbol implicitly rejects) -- not
        // this writer's concern, so totalize both sides identically before comparing
        // (same pattern `determinize.rs`'s own cross-strategy tests use).
        if !original.fa.is_true_false_automaton() {
            original.fa.totalize(0);
        }
        if !round_tripped.fa.is_true_false_automaton() {
            round_tripped.fa.totalize(0);
        }

        assert!(
            automaton_language_equivalent(&original, &round_tripped).unwrap(),
            "round-tripped {fixture_name} through write_automaton_txt is not \
             language-equivalent to the original"
        );
    }

    #[test]
    fn multi_track_deterministic_automaton_round_trips() {
        assert_txt_round_trips("automaton2.txt", "roundtrip-automaton2.txt");
    }

    #[test]
    fn explicit_set_alphabet_automaton_round_trips() {
        assert_txt_round_trips("automaton572.txt", "roundtrip-automaton572.txt");
    }

    #[test]
    fn true_automaton_round_trips() {
        assert_txt_round_trips("automaton189.txt", "roundtrip-automaton189.txt");
    }

    #[test]
    fn false_automaton_round_trips() {
        assert_txt_round_trips("automaton214.txt", "roundtrip-automaton214.txt");
    }

    #[test]
    fn trivial_true_writes_bare_word_no_trailing_newline() {
        let mut a = wr_core::automaton::Automaton::true_false(true);
        let mut buf = Vec::new();
        write_txt(&mut a, &mut buf).unwrap();
        assert_eq!(buf, b"true");
    }

    #[test]
    fn trivial_false_writes_bare_word_no_trailing_newline() {
        let mut a = wr_core::automaton::Automaton::true_false(false);
        let mut buf = Vec::new();
        write_txt(&mut a, &mut buf).unwrap();
        assert_eq!(buf, b"false");
    }

    #[test]
    fn explicit_set_header_matches_real_walnut_byte_for_byte() {
        // automaton572.txt's own header line is real walnut-java output
        // (`{0, 1} ` -- trailing space, single blank line after). Round-tripping it
        // through our writer must reproduce exactly that header.
        let mut a = read_automaton_txt(fixture("automaton572.txt")).unwrap();
        let mut buf = Vec::new();
        write_txt(&mut a, &mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let header_line = text.lines().next().unwrap();
        assert_eq!(header_line, "{0, 1} ");
    }

    /// Every other `.txt` round-trip test in this module goes through
    /// `read_automaton_txt`, which auto-determinizes on load (see its own doc
    /// comment/`nondeterministic_input_is_auto_determinized`) -- so every `Automaton`
    /// those tests ever hand to the writer is already deterministic, and `write_state`'s
    /// multi-destination code path (`for &dest in dests { write!(out, " {}", dest)?; }`)
    /// has never actually been exercised with more than one destination per symbol.
    ///
    /// This test bypasses `read_automaton_txt` entirely: it directly constructs a
    /// genuine NFA (state 0 on symbol 0 nondeterministically reaches BOTH states 1 and
    /// 2), writes it via `write_automaton_txt`, reads the written file back via
    /// `read_automaton_txt` (which DOES already support parsing multiple
    /// space-separated destinations on one line -- see
    /// `nondeterministic_input_is_auto_determinized` in `reader.rs`, and
    /// `try_parse_transition`'s `rhs.split_whitespace()`), and confirms the two are
    /// semantically equivalent. `original` is determinized+totalized independently
    /// (via `determinize_and_minimize`, not by piggy-backing on the write path under
    /// test) purely so the equivalence oracle's total-DFA precondition is met on both
    /// sides -- the WRITE itself still runs against the genuine NFA.
    #[test]
    fn multi_destination_nfa_round_trips_through_write_then_read() {
        let mut d = vec![BTreeMap::new(); 3];
        d[0].insert(0, vec![1, 2]);
        d[0].insert(1, vec![2]);
        let nfa_fa = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 1],
            d,
            true_false: None,
        };
        let original = wr_core::automaton::Automaton::new(
            nfa_fa,
            vec![vec![0, 1]],
            vec!["a".to_string()],
            vec![None],
        );
        assert!(
            !original.fa.is_deterministic(),
            "test setup: state 0 on symbol 0 must have 2 destinations -- a genuine NFA"
        );

        let out_path = temp_path("multi-dest-nfa.txt");
        write_automaton_txt(&mut original.clone(), &out_path).unwrap();
        let round_tripped = read_automaton_txt(&out_path).unwrap();
        std::fs::remove_file(&out_path).ok();

        // The equivalence oracle (see `equiv.rs`) requires both sides be total DFAs.
        // `round_tripped` already is (the reader auto-determinizes); `original` is
        // still the genuine NFA that was WRITTEN (that's the point of this test), so
        // determinize+totalize an independent clone of it just for the comparison.
        let mut original_dfa = original.clone();
        original_dfa.determinize_and_minimize();
        original_dfa.fa.totalize(0);
        let mut round_tripped_total = round_tripped;
        round_tripped_total.fa.totalize(0);

        assert!(
            automaton_language_equivalent(&original_dfa, &round_tripped_total).unwrap(),
            "a genuine NFA with multi-destination transitions must round-trip through \
             write_automaton_txt -> read_automaton_txt with the same language"
        );
    }

    #[test]
    fn ns_header_matches_real_walnut_byte_for_byte() {
        let mut a = read_automaton_txt(fixture("automaton2.txt")).unwrap();
        let mut buf = Vec::new();
        write_txt(&mut a, &mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let header_line = text.lines().next().unwrap();
        assert_eq!(header_line, "msd_3 msd_3 msd_3 msd_3");
    }

    // ------------------------------------------------------------------
    // `.gv`: byte-for-byte against real `walnut-java` output.
    // ------------------------------------------------------------------

    #[test]
    fn gv_matches_real_walnut_output_for_multi_track_automaton() {
        let mut a = read_automaton_txt(fixture("automaton2.txt")).unwrap();
        // `read_automaton_txt` deliberately labels tracks with placeholders
        // (`"0"`, `"1"`, ...) as a documented simplification (see `reader.rs`'s
        // module docs) -- real Java's `new Automaton(address)` leaves a freshly
        // loaded automaton UNLABELED (`getLabel()` empty), which is what produced
        // the real `"(): some predicate"` label line this fixture pins. Unlabel here
        // to compare against the real shape rather than the reader's convenience
        // default.
        a.unlabel();
        let mut buf = Vec::new();
        write_gv(&mut a, &mut buf, "some predicate", false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_sample.gv"));
    }

    #[test]
    fn gv_matches_real_walnut_output_for_true_automaton() {
        let mut a = wr_core::automaton::Automaton::true_false(true);
        let mut buf = Vec::new();
        write_gv(&mut a, &mut buf, "true predicate", false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_true.gv"));
    }

    #[test]
    fn gv_matches_real_walnut_output_for_false_automaton() {
        let mut a = wr_core::automaton::Automaton::true_false(false);
        let mut buf = Vec::new();
        write_gv(&mut a, &mut buf, "false predicate", false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_false.gv"));
    }

    /// `is_dfao=true` was previously untested despite `write_gv`'s own doc comment
    /// claiming byte-for-byte parity with real `walnut-java` output covers it. This
    /// pins that branch (`"{q}/{output}"` node labels) against a REAL captured run.
    ///
    /// `writer_sample_dfao.gv` was captured by compiling a small driver
    /// (`Automata.Automaton` + `Automata.Writer.AutomatonWriter.writeToGV(a, ...,
    /// "some predicate", true)`) against this checkout's real, already-built
    /// `walnut-java` jar (`target/Walnut-all.jar`, JDK 17), run against a hand-written
    /// `.txt` input describing a 3-state, single explicit-set-track (`{0, 1}`)
    /// automaton with real (non-boolean) per-state DFAO output values `5`, `3`, `7` —
    /// see `ATTRIBUTION.md` for the exact input and command. The automaton built here
    /// is the identical structure (states/transitions/outputs), constructed directly
    /// rather than round-tripped through the reader so this test is not coupled to
    /// `read_automaton_txt`'s own correctness.
    #[test]
    fn gv_matches_real_walnut_output_for_dfao_true_branch() {
        let mut d = vec![BTreeMap::new(); 3];
        d[0].insert(0, vec![1]);
        d[0].insert(1, vec![2]);
        d[1].insert(0, vec![2]);
        d[1].insert(1, vec![0]);
        d[2].insert(0, vec![1]);
        d[2].insert(1, vec![1]);
        let fa = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![5, 3, 7],
            d,
            true_false: None,
        };
        // Unbound (empty) label -- matches real `walnut-java`'s freshly-loaded
        // `new Automaton(address)` shape (`getLabel()` empty -> `to_tuple` prints
        // "()"), same convention the multi-track `.gv` test above uses via
        // `unlabel()`.
        let mut a =
            wr_core::automaton::Automaton::new(fa, vec![vec![0, 1]], Vec::new(), vec![None]);
        let mut buf = Vec::new();
        write_gv(&mut a, &mut buf, "some predicate", true).unwrap();
        assert_eq!(buf, fixture_bytes("writer_sample_dfao.gv"));
    }

    // ------------------------------------------------------------------
    // `.ba`: byte-for-byte against real `walnut-java`/`BAWriter` output.
    // ------------------------------------------------------------------

    #[test]
    fn ba_matches_real_walnut_output_for_multi_track_automaton() {
        let a = read_automaton_txt(fixture("automaton2.txt")).unwrap();
        let mut buf = Vec::new();
        export_to_ba(&a.fa, &mut buf, false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_sample.ba"));
    }

    #[test]
    fn ba_matches_real_walnut_output_for_true_automaton_wb021() {
        // WB-021: the TRUE automaton's `.ba` export is just "0\n" -- see module docs.
        let a = wr_core::automaton::Automaton::true_false(true);
        let mut buf = Vec::new();
        export_to_ba(&a.fa, &mut buf, false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_true.ba"));
    }

    #[test]
    fn ba_matches_real_walnut_output_for_false_automaton_wb021() {
        // WB-021: byte-identical to the TRUE automaton's output above -- that's the
        // bug.
        let a = wr_core::automaton::Automaton::true_false(false);
        let mut buf = Vec::new();
        export_to_ba(&a.fa, &mut buf, false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_false.ba"));
        assert_eq!(
            fixture_bytes("writer_true.ba"),
            fixture_bytes("writer_false.ba"),
            "WB-021: real walnut-java's exportToBA is genuinely indistinguishable \
             between TRUE and FALSE -- if this ever fails, WB-021 needs re-verifying \
             against a fresh real-jar run, not this test relaxed"
        );
    }

    /// Real `BAWriter` output for a hand-built NFA whose destinations were added
    /// grossly out of numeric order under one (state, symbol) pair -- confirms
    /// ascending destination-id ordering (see module docs' derivation section).
    #[test]
    fn ba_orders_destinations_ascending_matching_real_bawriter() {
        let mut d = vec![BTreeMap::new(); 5];
        d[0].insert(0, vec![2]);
        d[0].insert(1, vec![1, 2, 3, 4]);
        let fa = Fa {
            q0: 0,
            q: 5,
            alphabet_size: 3,
            o: vec![0, 1, 1, 0, 1],
            d,
            true_false: None,
        };
        let mut buf = Vec::new();
        export_to_ba(&fa, &mut buf, false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_order.ba"));
    }

    /// Real `BAWriter` output for a hand-built NFA whose transitions were added from
    /// out-of-order SOURCE states -- confirms ascending source-state ordering,
    /// independent of the symbol/destination axis above (see module docs).
    #[test]
    fn ba_orders_source_states_ascending_matching_real_bawriter() {
        let mut d = vec![BTreeMap::new(); 4];
        d[0].insert(0, vec![1]);
        d[2].insert(1, vec![3]);
        d[3].insert(0, vec![0]);
        let fa = Fa {
            q0: 0,
            q: 4,
            alphabet_size: 2,
            o: vec![0, 0, 0, 0],
            d,
            true_false: None,
        };
        let mut buf = Vec::new();
        export_to_ba(&fa, &mut buf, false).unwrap();
        assert_eq!(buf, fixture_bytes("writer_order2.ba"));
    }

    #[test]
    fn ba_rejects_dfao_matching_walnut_exception() {
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 1,
            o: vec![1],
            d: vec![BTreeMap::new()],
            true_false: None,
        };
        let mut buf = Vec::new();
        let err = export_to_ba(&fa, &mut buf, true).unwrap_err();
        assert!(matches!(err, BaWriteError::DfaoNotSupported));
        assert!(
            buf.is_empty(),
            "must not write anything before the guard fires"
        );
    }

    #[test]
    fn write_automaton_txt_and_gv_and_ba_to_real_files() {
        // Exercises the file-opening wrappers (not just the in-memory stream
        // functions), matching Java's address-taking entry points.
        let a = read_automaton_txt(fixture("automaton572.txt")).unwrap();
        let txt_path = temp_path("file-wrapper.txt");
        let gv_path = temp_path("file-wrapper.gv");
        let ba_path = temp_path("file-wrapper.ba");

        write_automaton_txt(&mut a.clone(), &txt_path).unwrap();
        write_automaton_gv(&mut a.clone(), &gv_path, "p", false).unwrap();
        export_automaton_to_ba(&a.fa, &ba_path, false).unwrap();

        assert!(std::fs::metadata(&txt_path).unwrap().len() > 0);
        assert!(std::fs::metadata(&gv_path).unwrap().len() > 0);
        assert!(std::fs::metadata(&ba_path).unwrap().len() > 0);

        // The .txt output must itself still read back correctly.
        let read_back = read_automaton_txt(&txt_path).unwrap();
        assert!(automaton_language_equivalent(&a, &read_back).unwrap());

        std::fs::remove_file(&txt_path).ok();
        std::fs::remove_file(&gv_path).ok();
        std::fs::remove_file(&ba_path).ok();
    }
}
