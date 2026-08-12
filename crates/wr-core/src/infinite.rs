// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Proves a language is infinite by finding a `prefix(cycle)*suffix`-shaped witness.
//!
//! Ports `Automata/FA/Infinite.java` (backs the `inf` command and the `I` quantifier's
//! `AutomatonLogicalOps.removeLeadingZeros` + `Infinite.infinite` pipeline in
//! `Main/EvalComputations/Token/LogicalOperator.actQuantifier`, ported later in U10).
//! The witness regex is deliberately **not** a description of the whole accepted
//! language — it is one reachable `prefix · cycle* · suffix` shape that proves
//! infinitude and nothing more.
//!
//! # Return type: `Result<Option<String>, InfiniteError>`, not Java's `""`-sentinel `String`
//!
//! Java's `infinite()` returns `""` for "the language is finite, no witness" and a
//! non-empty regex otherwise. `PORTING.md`'s "`null`/sentinel return -> `Option<T>`"
//! ruling applies directly to this exact idiom, so this port returns `None`/`Some(re)`
//! instead — same information, no reliance on `.is_empty()` at call sites. The outer
//! `Result` is a separate concern (see "WB-002" below): on one narrow input shape,
//! Java's `infinite()` doesn't return `""` at all, it throws `NullPointerException`.
//!
//! # Trivial (`TRUE`/`FALSE`) automaton input (U0)
//!
//! Java's own guard, `infiniteTrimmed`'s `fa.getQ() == 0 || fa.getQ0() < 0 ||
//! fa.getQ0() >= fa.getQ()` (`Infinite.java:43-45`), already happens to catch a
//! trivial automaton cleanly: `new Automaton(boolean)` sets `Q == 0`
//! (`Automaton.java:106-110`), so both `TRUE_FALSE_AUTOMATON` values fall through to
//! `return ""`. This is not a hypothetical path guarded defensively for its own
//! sake — it is genuinely reachable in Java: `ProverHelper.infFromAddress` reads an
//! automaton straight from a `.txt` file (which may be one of the 85 trivial
//! `automaton*` golden fixtures, U0's own headline finding), calls
//! `AutomatonLogicalOps.removeLeadingZeros(M, M.getLabel())` — whose `listOfLabels`
//! is empty for a label-less trivial automaton, so it early-returns `A.clone()`
//! unchanged — and then calls `Infinite.infinite(M.fa, M.richAlphabet)` directly on
//! the still-trivial `M.fa`. So [`infinite`] below checks
//! [`Automaton::is_true_false_automaton`] explicitly and up front, before doing
//! anything else, matching that real behavior (both `TRUE` and `FALSE` report
//! "finite" — sensible on its own terms too: a 0-arity automaton has no notion of
//! "infinitely many accepted values" to begin with, there being no input to vary).
//!
//! This port's explicit check is *stronger* than relying on `trimmed.q == 0` the way
//! Java's `fa.getQ() == 0` guard does, because [`Fa`] has a second trivial shape
//! Java's `FA` object model can't expose across this function boundary:
//! `Automaton::clear()`'s stale-but-nonzero `q` with an emptied `d`/`o`
//! (`crate::fa`'s module docs, `crate::automaton::Automaton::clear`). Relying on
//! `q == 0` alone would let that shape fall through into the DFS below and index an
//! empty `d`/`o` out of bounds; checking [`Automaton::is_true_false_automaton`]
//! directly (mirroring how [`crate::trim::trim`] itself guards, per its own module
//! docs) rules that out regardless of `q`.
//!
//! # WB-002: reproducing Java's *exact* crash trigger as a `Result::Err`
//!
//! `docs/WALNUT-BUGS.md`'s WB-002 documents a genuine Walnut (Java) bug:
//! `Infinite.infinite` throws `NullPointerException` on a narrow, precise input shape
//! — not, as an earlier draft of this file assumed, on "any empty-language input".
//! Working out the exact shape means following `Trimmer.trimAutomaton`'s own guard:
//! `if (a.isTRUE_FALSE_AUTOMATON() || a.getQ() <= 1) return;` (`Trimmer.java:31-33`) —
//! trimming is skipped **entirely** when the automaton has at most one state, so a
//! 1-state input reaches `infiniteTrimmed` completely untrimmed.
//!
//! Within that untrimmed `Q == 1` case, tracing `findCycle`/`findPath`/`decode` by hand
//! gives four sub-cases for the sole state, only one of which crashes:
//!
//! | state 0 accepting? | self-loop present? | Java's `infinite()` result            |
//! |---------------------|---------------------|---------------------------------------|
//! | yes                  | yes                  | `"([0])*"` (genuinely infinite)       |
//! | yes                  | no                   | `""` (finite — language is `{ε}`)     |
//! | **no**                | **yes**               | **`NullPointerException`**            |
//! | no                   | no                   | `""` (finite — language is `∅`)       |
//!
//! The crash needs BOTH a non-accepting sole state AND at least one outgoing
//! transition (with `Q == 1`, any transition necessarily targets the state itself, so
//! "has a transition" and "has a self-loop" coincide): `findCycle` immediately hits
//! the self-loop as a back edge (state 0 is still `ON_STACK` when it revisits itself)
//! and reports a "cycle" starting and ending at state 0; `findPath`'s BFS for a suffix
//! from there to an accepting state then exhausts (there is none reachable) and
//! returns Java `null`; `decode(null, r)` iterates that null list and NPEs. *Without*
//! a self-loop, `findCycle` has no transitions to traverse at all, reports no cycle,
//! and `infiniteTrimmed` returns `""` before ever reaching `findPath`/`decode` — so
//! "empty language, `Q == 1`" does *not* automatically crash; it specifically needs
//! the self-loop.
//!
//! **Empirically confirmed, not just reasoned from source.** `walnut-java`'s own
//! `InfiniteTest.testSingleStateSelfLoopWithNoAcceptingStateThrowsNPE` (the
//! not-accepting + self-loop row) is a *passing* JUnit characterization test that
//! asserts the NPE (`./mvnw -Dtest=Automata.FA.InfiniteTest test`: 7/7 green,
//! including that one, confirmed while authoring this fix). The other three rows were
//! checked with a small standalone driver (`InfProbe.java`, compiled and run against
//! the same `target/classes`, calling `Infinite.infinite` directly on hand-built `FA`s
//! for each shape) — its output matched the table exactly:
//! ```text
//! Q1 accepting+selfloop -> OK, result = "([0])*"
//! Q1 accepting+noloop -> OK, result = ""
//! Q1 nonaccepting+selfloop -> THREW java.lang.NullPointerException: Cannot invoke "java.util.List.iterator()" because "symbols" is null
//! Q1 nonaccepting+noloop -> OK, result = ""
//! ```
//!
//! ## `Q > 1` empty language was never a divergence
//!
//! A completely empty-language automaton with `Q > 1` does **not** reach this crash in
//! Java at all — also confirmed empirically (a hand-built 3-state automaton,
//! `q0 -> 1 <-> 2`, no accepting state anywhere: `Infinite.infinite` returns `""`
//! cleanly). `Trimmer.trimAutomaton`'s `Q > 1` path actually *runs* (unlike the
//! `Q <= 1` no-op above), and `Trimmer.quotient`'s `statesToKeep.isEmpty()` branch
//! collapses the whole automaton down to a single state with **zero** outgoing
//! transitions (`a.setNfaTransitions(new ArrayList<>())`, `Trimmer.java:44-51`) — not a
//! self-loop. So `findCycle` finds nothing to traverse and cleanly reports `""`: the
//! same "not accepting, no self-loop" row from the table above, just reached via real
//! trimming instead of trimming being skipped.
//!
//! This port's own [`crate::trim::trim`] handles the empty-language case differently
//! — a previously-shipped, previously-reviewed, out-of-this-unit's-scope choice
//! (`trim.rs`'s own module docs): its `keep.is_empty()` branch rebuilds a canonical
//! **fully self-looping** 1-state sink instead of Java's zero-transition one, and it
//! runs that same collapse for *every* `Q`, not just `Q <= 1` (this port's `trim` has
//! no `Q <= 1` no-op at all — see `trim.rs`'s docs on why not, a deliberate and
//! separately-reviewed choice unrelated to this file). Left unguarded, that would mean
//! the DFS below "finds" a self-loop cycle on *any* empty-language input post-trim and
//! then fails the suffix search exactly like Java's real `Q == 1` crash case — a shape
//! [`find_path`] would hit as a `None` this file would otherwise have to `.expect(..)`
//! away incorrectly. So [`infinite`] still keeps a `trimmed.is_language_empty()` guard
//! *after* trimming (returning `Ok(None)`), purely to route around this port's own
//! trim's self-looping-canonical-shape choice — not to emulate anything about Java,
//! which never reaches its DFS at all for `Q > 1` empty-language input. That guard is
//! not a divergence: it changes no answer Java gives (Java already answers `""` there
//! too, just via a different mechanism), it only prevents this port's differently-shaped
//! trim result from tripping an unrelated internal invariant below.
//!
//! ## The fix: `Result<Option<String>, InfiniteError>`
//!
//! [`infinite`] therefore does two separate things, in this order:
//! 1. **Before** calling [`crate::trim::trim`], checks the untrimmed input directly
//!    against Java's *exact* trigger (`q == 1`, state `0` not accepting, at least one
//!    outgoing transition) and returns [`InfiniteError::DegenerateSelfLoop`] — a
//!    `Result::Err`, not a `panic!`, matching the WB-011/WB-012/WB-013 precedent that a
//!    genuine Java `RuntimeException` `Prover.dispatch`'s top-level
//!    `catch (RuntimeException e)` recovers from (prints a stack trace, the session
//!    continues) is more faithfully ported as a recoverable `Result` than an uncaught
//!    Rust `panic!` (which would unwind and, absent a `catch_unwind` boundary this port
//!    doesn't have yet, kill the whole process — the opposite of Java's actual
//!    behavior).
//! 2. **After** trimming, keeps the `trimmed.is_language_empty()` guard described
//!    above, strictly to route around this port's own trim's self-looping-canonical-
//!    shape choice for `Q > 1` (and, harmlessly, any other `Q`) — `Ok(None)`, matching
//!    Java's real "finite" answer for that input, just reached by a different
//!    mechanism on both sides.
//!
//! Every case that reaches neither guard behaves exactly as it did before this fix.
//!
//! `docs/WALNUT-BUGS.md`'s WB-002 entry is updated alongside this module to describe
//! exactly this: not a blanket "every empty-language input" divergence, but Java's
//! real, narrow trigger reproduced precisely as a `Result::Err`.
//!
//! Once both guards above have passed, `find_path`'s "no target found" case (Java's
//! `null`) is provably unreachable. The pre-trim guard rules out the one untrimmed
//! shape ([`crate::trim::trim`] no-op-equivalent: `q == 1`) where a non-empty-language
//! trim result could still leave a state unable to reach acceptance; every other `q`
//! either collapses via `keep.is_empty()` (caught by the post-trim guard) or survives
//! trim's real reachability computation, which by construction keeps only states that
//! are both forward-reachable from `q0` and backward-co-reachable to an accepting
//! state (`trim.rs`'s own module docs). `find_cycle` only ever visits states
//! reachable from `q0` in the trimmed automaton, so `cycle.start` is
//! backward-co-reachable to acceptance by construction, and a BFS from a
//! backward-co-reachable state is guaranteed to discover *some* accepting state. This
//! is asserted via `.expect(..)`, not silently `unwrap`ped, so a violation (should
//! either invariant above ever regress) is a loud, diagnosable panic rather than a
//! silent wrong answer.
//!
//! # Witness string format (Tier-1-fixture-comparable — get this exact)
//!
//! Java's `decode` helper (`Infinite.java:153-159`) appends `RichAlphabet.decode(int)`
//! — which returns a `List<Integer>` digit tuple, one entry per track — directly to a
//! `StringBuilder`, so each encoded symbol renders via `List.toString()`:
//! `"[d0, d1, ..., dn]"` (comma-space-separated, square-bracketed), with **no**
//! separator between consecutive symbols in the same run. So for a single-track
//! automaton the witness for the self-loop-at-`q0` shape used by
//! `InfiniteTest.testMultiHopSuffixSearchBuildsRegex` is exactly `"([0])*[1][2]"` (no
//! prefix, cycle body `[0]`, suffix `[1][2]`) — reproduced verbatim by
//! [`tests::multi_hop_suffix_search_builds_regex`] below. [`decode_symbols`] hand-formats
//! this (rather than relying on `{:?}` on `Vec<i32>`, which happens to coincide today
//! but isn't a documented format-stability contract for this specific external-facing
//! use).

use crate::automaton::Automaton;
use crate::fa::Fa;
use crate::trim;
use std::collections::VecDeque;
use std::fmt;

/// Everything [`infinite`] can fail with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfiniteError {
    /// Reproduces `Infinite.infinite`'s `NullPointerException` (`docs/WALNUT-BUGS.md`
    /// WB-002) on Java's *exact* trigger shape: a single state (`Q == 1`, so
    /// `Trimmer.trimAutomaton`'s `Q <= 1` guard no-ops and trimming never runs), not
    /// accepting, with at least one outgoing transition (necessarily a self-loop, the
    /// only possible destination when `Q == 1`). See this module's docs for the full
    /// derivation and empirical confirmation, and the table of the three sibling
    /// `Q == 1` shapes that do *not* crash.
    ///
    /// Represented as a `Result::Err` here, deliberately **not** a `panic!`: Java's own
    /// NPE is an unchecked `RuntimeException` that `Prover.dispatch`'s top-level
    /// `catch (RuntimeException e)` recovers from (prints a stack trace, the session
    /// continues) — the same WB-011/WB-012/WB-013 precedent `wr_logic::expr::ExprError::
    /// RepeatedIdentifierMissingNumberSystem` follows for its own Java NPE.
    DegenerateSelfLoop,
}

impl fmt::Display for InfiniteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InfiniteError::DegenerateSelfLoop => write!(
                f,
                "automaton has a single, non-accepting, self-looping state (Java: \
                 NullPointerException in Infinite.decode via findPath returning null, \
                 see WB-002)"
            ),
        }
    }
}

impl std::error::Error for InfiniteError {}

/// DFS visit state for [`find_cycle`]. Matches Java's `UNSEEN`/`ON_STACK`/`DONE` `int`
/// constants (`Infinite.java:20-22`) one-for-one; a Rust enum purely for readability,
/// no behavior change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Unseen,
    OnStack,
    Done,
}

/// A DFS-discovered directed cycle: `start` is the state the back edge closes onto
/// (the first state, in DFS order, found already `OnStack`), and `symbols` are the
/// encoded transition symbols along `start -> ... -> start`, already including the
/// closing back edge. Mirrors Java's private `Cycle` record (`Infinite.java:161`).
struct Cycle {
    start: usize,
    symbols: Vec<i32>,
}

/// Returns a witness regex `prefix(cycle)*suffix` proving `a`'s language is infinite,
/// `Ok(None)` if it's finite, or [`InfiniteError::DegenerateSelfLoop`] on the one input
/// shape where Java's `infinite()` itself throws `NullPointerException` (WB-002).
/// Ports `Infinite.infinite`/`infiniteTrimmed` (`Infinite.java:29-61`) — see this
/// module's docs for the `Option<String>` payload, the trivial-automaton short-circuit,
/// and the two guards (one pre-trim, one post-trim) that together reproduce Java's
/// exact crash trigger without either under- or over-firing it.
pub fn infinite(a: &Automaton) -> Result<Option<String>, InfiniteError> {
    if a.fa.is_true_false_automaton() {
        return Ok(None);
    }

    // Checked BEFORE trimming, unlike Java's `infinite`/`infiniteTrimmed` (which trims
    // first, then guards) -- see this module's docs, "Guarding an invalid `q0` before
    // trim, not after". `crate::trim::trim` (already shipped, out of this unit's scope)
    // assumes `q0 < q` and indexes `fa.d`/`fa.o` at `q0` unconditionally; Java's
    // `Trimmer.trimAutomaton` happens to be safe against an out-of-range `q0` too, but
    // only incidentally, via its `getQ() <= 1` no-op guard (absent from this port's
    // `trim`, per that module's own documented divergence) -- for `Q > 1` an invalid
    // `q0` crashes Java's `Trimmer` as well, just differently. Guarding here, for every
    // `Q`, is strictly more robust than Java (which only tolerates this for `Q <= 1`)
    // and changes no answer for any well-formed automaton.
    if a.fa.q == 0 || a.fa.q0 >= a.fa.q {
        return Ok(None);
    }

    // Java's *exact* WB-002 trigger, checked on the UNTRIMMED automaton (matching
    // `Trimmer.trimAutomaton`'s `Q <= 1` no-op -- a 1-state automaton never actually
    // gets trimmed in Java). See this module's docs for the full derivation and the
    // empirically-confirmed table of the four `Q == 1` sub-cases; only "not accepting,
    // has an outgoing transition" crashes. `a.fa.q == 1` forces `a.fa.q0 == 0` (the
    // `q0 >= q` guard above already ruled out anything else), so `a.fa.d[a.fa.q0]` is
    // always in range here.
    //
    // NOTE: `a.fa.d[a.fa.q0]` is a `BTreeMap<i32, Vec<usize>>`; `.is_empty()` on the
    // map only tests whether any SYMBOL KEY is present, not whether that key's
    // destination `Vec` is actually non-empty. A `Fa` can have a symbol key mapped to
    // an empty `Vec` (constructible via direct `Fa` manipulation -- `fa.rs`'s
    // `canonicalize_prunes_entries_with_empty_destination_list` test builds exactly
    // this shape, precisely because `canonicalize()` exists to prune it). That shape
    // has NO real transition -- no self-loop at all -- so the guard must check for an
    // actual non-empty destination, matching how `find_cycle`/`find_path` below already
    // traverse `dests` (they don't check map non-emptiness either).
    if a.fa.q == 1
        && !a.fa.is_accepting(a.fa.q0)
        && a.fa.d[a.fa.q0].values().any(|dests| !dests.is_empty())
    {
        return Err(InfiniteError::DegenerateSelfLoop);
    }

    let trimmed = trim::trim(&a.fa);

    // `infiniteTrimmed`'s own guard (`Infinite.java:43-45`) is checked again here for
    // fidelity to Java's structure, even though `trim`'s postcondition should make the
    // second half unreachable now that the pre-trim checks above have run. Also: Java
    // additionally checks `fa.getQ0() < 0`; `q0: usize` makes that sub-case
    // unrepresentable in this port, so there is nothing left to check for it.
    if trimmed.q == 0 || trimmed.q0 >= trimmed.q {
        return Ok(None);
    }

    // NOT a Java divergence -- see this module's docs, "`Q > 1` empty language was
    // never a divergence". Purely routes around this port's OWN `trim`'s
    // self-looping-canonical-shape choice for an empty language (any `Q`), which Java
    // reaches cleanly via a structurally different (zero-transition) mechanism.
    if trimmed.is_language_empty() {
        return Ok(None);
    }

    let mut visit_state = vec![VisitState::Unseen; trimmed.q];
    let mut previous: Vec<Option<usize>> = vec![None; trimmed.q];
    let mut input: Vec<Option<i32>> = vec![None; trimmed.q];
    previous[trimmed.q0] = Some(trimmed.q0);

    let Some(cycle) = find_cycle(
        &trimmed,
        trimmed.q0,
        &mut visit_state,
        &mut previous,
        &mut input,
    ) else {
        return Ok(None);
    };

    let prefix = symbols_on_path(trimmed.q0, cycle.start, &previous, &input);
    let suffix = find_path(&trimmed, cycle.start, |s| trimmed.is_accepting(s)).expect(
        "unreachable: the pre-trim DegenerateSelfLoop guard plus trim's postcondition \
         together guarantee a path to acceptance from any state the (non-empty-language) \
         DFS can reach cycle.start through -- see this module's docs",
    );

    Ok(Some(format!(
        "{}({})*{}",
        decode_symbols(&prefix, a),
        decode_symbols(&cycle.symbols, a),
        decode_symbols(&suffix, a)
    )))
}

/// Runs DFS from `current` and returns the first directed cycle found. Ports
/// `findCycle` (`Infinite.java:69-93`) literally: same traversal order (transitions
/// visited in ascending symbol order, matching `Fa::d`'s `BTreeMap` iteration order
/// against Java's `Int2ObjectRBTreeMap`; destinations within one symbol visited in
/// their stored order, matching Java's `IntList`), same back-edge detection (a
/// transition into an `OnStack` state), same early-return-on-first-cycle-found
/// short-circuit.
///
/// # Known limitation: unbounded native recursion (flagged for U10/U15/U16)
///
/// This mirrors Java's own recursive `findCycle` faithfully (per this crate's
/// mechanical-port rule — Java is recursive here too, so a rewrite would be a
/// *refactor*, not a port), but the failure mode differs across languages on a
/// deep/degenerate automaton: Java's `StackOverflowError` is an unchecked but
/// *catchable* `Error` (`Prover.dispatch`'s top-level catch can still recover), while
/// Rust's native stack overflow aborts the process outright — no `Result`, no
/// `panic!` to catch, no recovery. Rewriting this as an iterative DFS would remove the
/// risk but isn't attempted here: it would touch currently-correct, twice-adversarially-
/// reviewed traversal logic for a robustness concern with no reproducing input yet, and
/// risks introducing a new bug in exchange. Left as a **known, deliberate, flagged
/// limitation**: whoever wires [`infinite`] into a real CLI call site later (U10/U15/
/// U16) should consider running it on a thread with an explicitly larger stack
/// (`std::thread::Builder::stack_size`, the common Rust pattern for this exact problem)
/// for large/adversarial automata, rather than assuming the default thread stack is
/// always enough.
fn find_cycle(
    fa: &Fa,
    current: usize,
    visit_state: &mut [VisitState],
    previous: &mut [Option<usize>],
    input: &mut [Option<i32>],
) -> Option<Cycle> {
    visit_state[current] = VisitState::OnStack;

    for (&symbol, dests) in &fa.d[current] {
        for &next in dests {
            match visit_state[next] {
                VisitState::Unseen => {
                    previous[next] = Some(current);
                    input[next] = Some(symbol);
                    if let Some(cycle) = find_cycle(fa, next, visit_state, previous, input) {
                        return Some(cycle);
                    }
                }
                VisitState::OnStack => {
                    // next -> ... -> current, closed by current --symbol--> next.
                    let mut cycle_symbols = symbols_on_path(next, current, previous, input);
                    cycle_symbols.push(symbol);
                    return Some(Cycle {
                        start: next,
                        symbols: cycle_symbols,
                    });
                }
                VisitState::Done => {}
            }
        }
    }

    // No cycle was found through `current`; future DFS branches don't need to
    // re-inspect it.
    visit_state[current] = VisitState::Done;
    None
}

/// Finds a shortest path from `start` to any state satisfying `is_target`, returning
/// the encoded symbols along it (not states). Ports `findPath` (`Infinite.java:100-131`)
/// literally: plain BFS, `previous[q].is_some()` standing in for Java's
/// `previous[next] != MISSING_ELT` "already visited" check.
fn find_path(fa: &Fa, start: usize, is_target: impl Fn(usize) -> bool) -> Option<Vec<i32>> {
    if is_target(start) {
        return Some(Vec::new());
    }

    let mut previous: Vec<Option<usize>> = vec![None; fa.q];
    let mut input: Vec<Option<i32>> = vec![None; fa.q];

    let mut queue: VecDeque<usize> = VecDeque::new();
    previous[start] = Some(start);
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        for (&symbol, dests) in &fa.d[current] {
            for &next in dests {
                if previous[next].is_some() {
                    continue;
                }
                previous[next] = Some(current);
                input[next] = Some(symbol);
                if is_target(next) {
                    return Some(symbols_on_path(start, next, &previous, &input));
                }
                queue.push_back(next);
            }
        }
    }

    None
}

/// Reconstructs the encoded symbols on a known path from `start` to `end`. Ports
/// `symbolsOnPath` (`Infinite.java:144-151`): walk backward from `end` to `start` via
/// `previous`/`input`, then reverse. Returns the empty list when `start == end`
/// (matches Java's `for` loop, which never executes in that case).
fn symbols_on_path(
    start: usize,
    end: usize,
    previous: &[Option<usize>],
    input: &[Option<i32>],
) -> Vec<i32> {
    let mut symbols = Vec::new();
    let mut q = end;
    while q != start {
        symbols.push(input[q].expect("symbols_on_path: `input` missing for a state on the path"));
        q = previous[q].expect("symbols_on_path: `previous` missing for a state on the path");
    }
    symbols.reverse();
    symbols
}

/// Ports `decode` (`Infinite.java:153-159`): decodes each encoded symbol into its
/// per-track digit tuple and renders it Java-`List.toString()`-style
/// (`"[d0, d1, ..., dn]"`), concatenating the per-symbol strings with no separator
/// between them. See this module's docs for why the exact format matters (Tier-1
/// fixture text) and why this doesn't just delegate to `{:?}`.
fn decode_symbols(symbols: &[i32], a: &Automaton) -> String {
    let mut result = String::new();
    for &symbol in symbols {
        let digits = a.decode(symbol);
        result.push('[');
        for (i, d) in digits.iter().enumerate() {
            if i > 0 {
                result.push_str(", ");
            }
            result.push_str(&d.to_string());
        }
        result.push(']');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Builds a single-track `Automaton` over the given alphabet (e.g. `&[0, 1, 2]`
    /// for msd/lsd base 3) wrapping a hand-built `Fa`.
    fn single_track_automaton(fa: Fa, alphabet: &[i32]) -> Automaton {
        Automaton::new(
            fa,
            vec![alphabet.to_vec()],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    fn fa_with(q0: usize, q: usize, alphabet_size: usize, o: Vec<i32>) -> Fa {
        Fa {
            true_false: None,
            q0,
            q,
            alphabet_size,
            o,
            d: vec![BTreeMap::new(); q],
        }
    }

    fn add_transition(fa: &mut Fa, from: usize, symbol: i32, to: usize) {
        fa.d[from].entry(symbol).or_default().push(to);
    }

    // --- Trivial (TRUE/FALSE) automaton: U0 short-circuit ---

    #[test]
    fn trivial_true_automaton_is_finite() {
        let a = Automaton::true_false(true);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn trivial_false_automaton_is_finite() {
        let a = Automaton::true_false(false);
        assert_eq!(infinite(&a), Ok(None));
    }

    // --- Direct ports of `InfiniteTest.java`'s guard-clause characterization tests ---

    #[test]
    fn empty_automaton_is_finite() {
        // Infinite.java's Q == 0 guard, reached directly (mirrors
        // InfiniteTest.testEmptyAutomatonIsFinite, minus the TRUE_FALSE_AUTOMATON
        // flag -- exercised separately above).
        let fa = fa_with(0, 0, 0, vec![]);
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn out_of_range_initial_state_is_finite() {
        // Mirrors InfiniteTest.testOutOfRangeInitialStateIsTreatedAsFinite (Q0 >= Q).
        // (Java also covers Q0 < 0, which `q0: usize` makes unrepresentable here.)
        let fa = fa_with(1, 1, 1, vec![0]);
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn acyclic_automaton_is_finite() {
        // Mirrors InfiniteTest.testAcyclicAutomatonIsFinite: 0 --0--> 1(accepting), no
        // cycle anywhere.
        let mut fa = fa_with(0, 2, 1, vec![0, 1]);
        add_transition(&mut fa, 0, 0, 1);
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn multi_hop_suffix_search_builds_regex() {
        // Direct port of InfiniteTest.testMultiHopSuffixSearchBuildsRegex: a self-loop
        // at q0 (cycle found immediately, no DFS needed to reach it) plus a two-hop
        // BFS suffix search (0 -> 1 -> 2) to the accepting state, exercising both the
        // immediate-cycle path and the multi-hop BFS "not yet found, keep looking"
        // branch.
        let mut fa = fa_with(0, 3, 3, vec![0, 0, 1]);
        add_transition(&mut fa, 0, 0, 0);
        add_transition(&mut fa, 0, 1, 1);
        add_transition(&mut fa, 1, 2, 2);
        let a = single_track_automaton(fa, &[0, 1, 2]);

        // prefix = "" (q0 == cycle.start), cycle = "0" (the self-loop symbol),
        // suffix = "1","2" (the two BFS hops) -- byte-for-byte Java's expected output.
        assert_eq!(infinite(&a), Ok(Some("([0])*[1][2]".to_string())));
    }

    // --- WB-002: Java's exact NPE trigger, reproduced as a `Result::Err` (see module docs) ---

    #[test]
    fn single_state_self_loop_with_no_accepting_state_errors() {
        // Java's own trigger, byte-for-byte: `Q == 1`, state 0 not accepting, with a
        // self-loop. `InfiniteTest.testSingleStateSelfLoopWithNoAcceptingStateThrowsNPE`
        // asserts the real `NullPointerException` for this exact shape (confirmed
        // passing, `./mvnw -Dtest=Automata.FA.InfiniteTest test`, while authoring this
        // fix); this port reproduces it as `InfiniteError::DegenerateSelfLoop` instead
        // of an uncaught panic, per this module's docs.
        let mut fa = fa_with(0, 1, 1, vec![0]); // single non-accepting state
        add_transition(&mut fa, 0, 0, 0); // self loop
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Err(InfiniteError::DegenerateSelfLoop));
    }

    #[test]
    fn single_state_no_self_loop_is_finite_not_an_error() {
        // Sibling of the case above with the self-loop removed: `Q == 1`, not
        // accepting, but ZERO outgoing transitions. Empirically confirmed this does
        // NOT crash in Java (`findCycle` has nothing to traverse, reports no cycle,
        // `infiniteTrimmed` returns `""` before ever reaching `findPath`/`decode`) --
        // proving the port's guard fires on "has a self-loop", not merely "Q == 1 and
        // not accepting". The language here is literally empty (`∅`), correctly finite.
        let fa = fa_with(0, 1, 1, vec![0]); // single non-accepting state, no transitions
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn single_state_empty_destination_list_is_finite_not_an_error() {
        // Regression test for a false-positive found by two independent adversarial
        // reviews of the pre-trim guard above: `Q == 1`, not accepting, with a symbol
        // key present in `fa.d[q0]` but mapped to an EMPTY destination `Vec` (the same
        // shape `fa.rs`'s `canonicalize_prunes_entries_with_empty_destination_list`
        // test builds, via direct `Fa` construction -- `canonicalize()` exists
        // specifically to prune this). This is NOT a self-loop: there is no real
        // transition at all, so `BTreeMap::is_empty()` (which only tests key
        // presence, not destination non-emptiness) previously misfired here and
        // returned `Err(DegenerateSelfLoop)` instead of the Java-correct `Ok(None)`
        // ("" / finite -- `findCycle`'s inner loop over the empty destination list
        // never executes, so it reports no cycle, same as the sibling
        // `single_state_no_self_loop_is_finite_not_an_error` case above).
        let mut fa = fa_with(0, 1, 1, vec![0]); // single non-accepting state
        fa.d[0].insert(0, vec![]); // symbol key present, but destination list empty
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn single_accepting_state_with_self_loop_is_infinite_not_an_error() {
        // `Q == 1`, state 0 IS accepting, WITH a self-loop: empirically confirmed Java
        // returns `"([0])*"` cleanly (`isTarget(cycle.start)` is true immediately, so
        // `findPath` never runs its BFS at all) -- proving the guard also requires
        // non-acceptance, not just "Q == 1 with a self-loop".
        let mut fa = fa_with(0, 1, 1, vec![1]); // single accepting state
        add_transition(&mut fa, 0, 0, 0); // self loop
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(Some("([0])*".to_string())));
    }

    #[test]
    fn single_accepting_state_no_self_loop_is_finite_not_an_error() {
        // `Q == 1`, accepting, no self-loop: language is `{ε}`, empirically confirmed
        // Java returns `""` (no cycle for `findCycle` to find).
        let fa = fa_with(0, 1, 1, vec![1]); // single accepting state, no transitions
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn empty_language_is_finite_regardless_of_state_count() {
        // NOT a Java divergence (see module docs, "`Q > 1` empty language was never a
        // divergence") -- Java's own `Trimmer.quotient` collapses this to a
        // zero-transition single state and answers "" cleanly too, just via a
        // different mechanism than this port's `trim`. `Q > 1` here, unlike the
        // `DegenerateSelfLoop`-triggering cases above, so the pre-trim guard must NOT
        // fire; only the post-trim `is_language_empty` guard should.
        let mut fa = fa_with(0, 3, 1, vec![0, 0, 0]); // no state is accepting
        add_transition(&mut fa, 0, 0, 1);
        add_transition(&mut fa, 1, 0, 2);
        add_transition(&mut fa, 2, 0, 1); // 1 <-> 2 is a live cycle, but unreachable to acceptance
        let a = single_track_automaton(fa, &[0]);
        assert_eq!(infinite(&a), Ok(None));
    }

    // --- Task-requested edge cases ---

    #[test]
    fn cycle_unreachable_from_any_accepting_path_is_finite() {
        // q0 --0--> 1 (accepting, dead end); q0 --1--> 2 (non-accepting, self-loops on
        // 0 forever, never reaches acceptance). Trim removes state 2 entirely (it's
        // forward-reachable from q0 but not backward-co-reachable to acceptance), so
        // the cycle at state 2 never reaches the DFS at all -- the language is finite
        // ({"0"}) even though the untrimmed automaton has a real cycle in it.
        let mut fa = fa_with(0, 3, 2, vec![0, 1, 0]);
        add_transition(&mut fa, 0, 0, 1);
        add_transition(&mut fa, 0, 1, 2);
        add_transition(&mut fa, 2, 0, 2); // dead self-loop, never reaches state 1
        let a = single_track_automaton(fa, &[0, 1]);
        assert_eq!(infinite(&a), Ok(None));
    }

    #[test]
    fn language_size_equal_to_state_count_is_finite() {
        // A `Q`-state "star": q0 (accepting, so epsilon is in the language) plus
        // `Q - 1` branches, each a single non-epsilon symbol read straight into a
        // distinct accepting leaf with no further transitions. Exactly `Q` words are
        // accepted (`Q - 1` one-symbol words, plus epsilon) by exactly `Q` states --
        // an acyclic boundary case where "language size" and "state count" coincide,
        // rather than one dwarfing the other.
        const Q: usize = 5;
        let mut o = vec![1]; // q0 itself accepts (epsilon)
        o.extend(std::iter::repeat_n(1, Q - 1));
        let mut fa = fa_with(0, Q, Q - 1, o);
        for leaf in 1..Q {
            add_transition(&mut fa, 0, (leaf - 1) as i32, leaf);
        }
        let alphabet: Vec<i32> = (0..(Q - 1) as i32).collect();
        let a = single_track_automaton(fa, &alphabet);
        assert_eq!(infinite(&a), Ok(None));

        // Sanity: this really is the claimed language (Q accepted words, all distinct).
        assert!(a.fa.accepts_word(&[]));
        for sym in 0..(Q - 1) as i32 {
            assert!(a.fa.accepts_word(&[sym]));
        }
        assert!(!a.fa.accepts_word(&[0, 0]));
    }

    #[test]
    fn infinite_language_with_nonempty_prefix_and_suffix() {
        // q0 --a--> 1 (cycle start, self-loop on b) --c--> 2 (accepting). Unlike
        // `multi_hop_suffix_search_builds_regex`, both the prefix (q0 -> cycle.start)
        // and the suffix (cycle.start -> accepting) are non-empty, exercising the
        // general shape rather than the q0-is-the-cycle-start special case.
        let mut fa = fa_with(0, 3, 3, vec![0, 0, 1]);
        add_transition(&mut fa, 0, 0, 1); // a
        add_transition(&mut fa, 1, 1, 1); // b (self-loop)
        add_transition(&mut fa, 1, 2, 2); // c
        let a = single_track_automaton(fa, &[0, 1, 2]);

        assert_eq!(infinite(&a), Ok(Some("[0]([1])*[2]".to_string())));
    }

    // --- Multi-track witness format ---

    #[test]
    fn multi_track_witness_uses_bracketed_digit_tuples() {
        // `Infinite.infinite` genuinely runs on multi-track automata in real usage (the
        // `I`-quantifier path, `LogicalOperator.actQuantifier` -> `Infinite.infinite`,
        // and `ProverHelper.infFromAddress`) and `decode_symbols` is written generically
        // over N tracks, but every test above this one is single-track. This pins the
        // N > 1 path.
        //
        // Two tracks, each alphabet `[0, 1]`, so `Automaton::compute_encoder` gives
        // `encoder = [1, 2]` (hand-verified against `automaton.rs`'s `encode`/`decode`):
        // encoded symbol `2` decodes to digit tuple `[0, 1]` (track0 idx 0, track1 idx
        // 1: `0*1 + 1*2 == 2`) and encoded symbol `1` decodes to `[1, 0]` (track0 idx 1,
        // track1 idx 0: `1*1 + 0*2 == 1`).
        //
        // q0 (non-accepting) self-loops on encoded symbol 2 (digits `[0, 1]`); q0 --1-->
        // 1 (accepting) on encoded symbol 1 (digits `[1, 0]`). The cycle is found
        // immediately at q0 (empty prefix), cycle body is the self-loop's digits, and
        // the one-hop BFS suffix is the edge into the accepting state.
        let mut fa = fa_with(0, 2, 4, vec![0, 1]);
        add_transition(&mut fa, 0, 2, 0); // self-loop, digits [0, 1]
        add_transition(&mut fa, 0, 1, 1); // digits [1, 0], reaches acceptance
        let a = Automaton::new(
            fa,
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![Some(true), Some(true)],
        );

        assert_eq!(infinite(&a), Ok(Some("([0, 1])*[1, 0]".to_string())));
    }
}
