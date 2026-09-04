// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Valmari DFA minimization.
//!
//! Ports `Automata/FA/ValmariDFA.java` + `Automata/FA/ValmariPartition.java`, which are
//! themselves an adaptation of Antti Valmari, *"Fast brief practical DFA minimization"*,
//! Information Processing Letters 112.6 (2012): 213-217. The Java is a close
//! transliteration of the paper's own terse code; the naming here is expanded for
//! readability but the control flow, the shared mark/worklist scratch, the
//! co-reachability pre-pass, and the smaller-half-gets-the-new-index rule are preserved
//! exactly.
//!
//! # Preconditions
//!
//! 1. **Deterministic input** (a hard error, never a `debug_assert!`). Java's
//!    `FA.justMinimize` calls `convertNFAtoDFA()` first, which is a *storage* conversion
//!    that throws `"Unexpected NFA instead of DFA."` on genuine nondeterminism — it does
//!    not subset-construct. The algorithm itself is unsound on an NFA:
//!    [`Partition::mark`] has no double-mark guard, and two transitions sharing a tail
//!    *and* a label would land in the same cord and mark the same block element twice,
//!    corrupting the partition. Mirrored here as [`MinimizeError::NotDeterministic`].
//! 2. **Every state reachable from `q0` — for MINIMALITY only, no longer for
//!    correctness.** Walnut kept only Valmari's *backward* pruning (see the next
//!    section), so a state that is unreachable from `q0` but can still reach acceptance
//!    survives as one or more extra blocks and the result is language-correct but not
//!    minimal. Callers who need the genuinely minimal automaton should run
//!    [`crate::trim::trim`] first. Until the WB-001 fix below this precondition was
//!    load-bearing for *correctness* as well; it is not anymore.
//!
//! # Two reachability notions — do not conflate them
//!
//! Valmari's paper removes *both* states unreachable from `q0` (forward) and states from
//! which no final state is reachable (backward). **Walnut kept only the backward half**:
//! `ValmariDFA.reach` is seeded from every state with nonzero output and walks the
//! transition graph *backwards* (`make_adjacent(H)` buckets transitions by head, then
//! `reach(T[_A[j]])` steps to their tails), i.e. it computes exactly the
//! co-reachable-to-accepting set. It is therefore *not* a substitute for
//! [`crate::trim::trim`], which is the only place in this crate that checks
//! reachability from `q0` — and `minimize` deliberately does not do `trim`'s job.
//!
//! # The q0 aliasing bug (WB-001) — FIXED, matching `walnut-java` commit `14509f1`
//!
//! **The defect.** States found non-co-reachable are left parked at positions `>= rr` in
//! the element array, outside every block's `[F, P)` range, yet their set-id `S[q]` is
//! still the `0` that `init` defaulted it to — `Partition::split` only ever rewrites
//! `set_of` for positions inside a block's own `[F, P)` range, so it never revisits a
//! parked state. `replaceFields` then computed the new start state as `blocks.S[q0]`
//! unconditionally. So if `q0` itself cannot reach an accepting state *while some
//! accepting state exists*, the result's start state silently became block `0` — which
//! after the initial accepting/non-accepting split is not necessarily the dead block, and
//! can even be an accepting one. Concretely: `q0` self-looping and non-accepting, plus a
//! disjoint accepting self-loop, minimized to a 1-state *accepting* automaton (language
//! `∅` became `Σ*`).
//!
//! Note that `q0` parked ⟺ no accepting state is reachable from `q0` ⟺ the language is
//! empty. **Every** block holds only co-reachable states, so no block's language is ever
//! empty — which is why *both* polarities of the initial split produced a wrong answer,
//! not just the spectacular `Σ*` one.
//!
//! **The fix** (ported from `ValmariDFA.java`'s `numCoreachable` field + its
//! `replaceFields` guard + `replaceWithEmptyLanguage`): the co-reachable-set size is
//! recorded as a `num_coreachable` binding in [`minimize`] at the end of the
//! `rem_unreachable` pass,
//! and the rebuild checks `blocks.loc[q0] >= num_coreachable` before reading
//! `blocks.set_of[q0]`. When it holds, the result is the canonical minimal
//! empty-language automaton: **one non-accepting state, no transitions**. That shape is
//! not invented here — it is what this crate and Walnut already mean by "the empty
//! language" (Java's `Trimmer.quotient` empty-`statesToKeep` branch, and Valmari's own
//! output when the automaton has no accepting states at all). Both engines' DFAs are
//! partial, so a transition-less state is a well-formed sink; adding self-loops would
//! diverge from both precedents.
//!
//! The guard necessarily fires for **every** `q0` when there is no accepting state at all
//! (`num_coreachable == 0`), and there it is provably a no-op: the unguarded rebuild
//! already produced exactly `q = 1`, `q0 = 0`, `o = [0]`, no transitions on that input
//! (`blocks.z == 1` from `init`, block `0`'s range is `[0, 0)`, `num_final_states == 0`,
//! and every transition was dropped by the co-reachability filter). So the only inputs
//! whose result changes are the defective shape itself. Pinned both ways by
//! `minimize_wb_001_trigger_now_yields_the_empty_language` and
//! `the_guard_is_a_no_op_on_every_non_defective_input`.
//!
//! Cataloged as `docs/WALNUT-BUGS.md` WB-001 (upstream fix status, severity, the full
//! four-call-site inventory and verification history — this doc comment covers only the
//! Rust-port-relevant summary).
//!
//! # Scope note
//!
//! Like the Java, outputs are rebuilt as plain accept/reject bits, so DFAO word-output
//! values (`o > 1`) do not survive minimization. Matches this crate's current
//! predicate-automaton scope (see the `equiv` module docs).

use crate::fa::Fa;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinimizeError {
    /// Some `(state, symbol)` pair had more than one destination. Ports the
    /// `"Unexpected NFA instead of DFA."` throw in `FA.convertNFAtoDFA`.
    NotDeterministic,
    /// Two transitions out of the same final block on the same symbol disagreed on the
    /// destination block. Structurally impossible for a valid partition, but Walnut
    /// carries this as an unconditional production throw
    /// (`"Valmari minimization produced conflicting DFA transitions."`), so it is a real
    /// error here too — not an assertion that evaporates in release builds.
    ConflictingTransitions,
}

/// Mark counts + the pending-set worklist, shared by both partitions.
///
/// Java holds these as `static int[] M, W; static int w` on `ValmariPartition` — class
/// state deliberately shared between the `blocks` and `cords` instances. Reproduced here
/// as an explicit `&mut` context rather than per-partition fields, so the sharing stays
/// visible instead of being silently duplicated.
///
/// Fidelity note: the sharing turns out to be a memory optimization rather than a
/// semantic dependency. The refinement loop never has both partitions marked at once —
/// `split` drains `w` to empty and zeroes `m[s]` for every set it pops, so the scratch is
/// always clean when the other partition takes it over. Per-partition copies would
/// compute the same answer; they would just cost more memory. Kept shared to match Java.
struct Scratch {
    /// `M`: number of currently-marked elements of set `s`.
    m: Vec<usize>,
    /// `W`/`w`: stack of set ids with at least one mark pending.
    w: Vec<usize>,
}

/// Valmari's refinable partition over `0..n` (`ValmariPartition`).
///
/// Elements of a set occupy a contiguous run of `elems`; `mark` swaps marked elements to
/// the front of their run, and `split` cuts the run at the mark boundary.
struct Partition {
    /// `z`: number of sets.
    z: usize,
    /// `E`: elements, grouped by set.
    elems: Vec<usize>,
    /// `L`: location of each element within `elems`.
    loc: Vec<usize>,
    /// `S`: set id of each element.
    set_of: Vec<usize>,
    /// `F`: first index (inclusive) of each set's run.
    first: Vec<usize>,
    /// `P`: past-the-end index of each set's run.
    past: Vec<usize>,
}

impl Partition {
    fn init(n: usize) -> Self {
        let mut p = Partition {
            z: usize::from(n != 0),
            elems: (0..n).collect(),
            loc: (0..n).collect(),
            set_of: vec![0; n],
            first: vec![0; n],
            past: vec![0; n],
        };
        if p.z != 0 {
            p.past[0] = n;
        }
        p
    }

    /// Moves element `e` to the marked prefix of its set and, on the set's first mark,
    /// pushes it onto the shared worklist.
    ///
    /// Assumes `e` is not already marked — see the module docs on why determinism of the
    /// input is what guarantees that.
    fn mark(&mut self, e: usize, sc: &mut Scratch) {
        let s = self.set_of[e];
        let i = self.loc[e];
        let j = self.first[s] + sc.m[s];

        // Swap `e` (at `i`) with whatever sits at the mark boundary `j`.
        self.elems[i] = self.elems[j];
        self.loc[self.elems[i]] = i;
        self.elems[j] = e;
        self.loc[e] = j;

        if sc.m[s] == 0 {
            sc.w.push(s);
        }
        sc.m[s] += 1;
    }

    /// Splits every marked set at its mark boundary, draining the worklist.
    ///
    /// The **smaller** of the two halves always receives the freshly allocated set id
    /// `z`; the larger half keeps `s`. That is what makes the outer refinement loop's
    /// "walk the new set ids once" scan the small side only (Hopcroft's trick), so it
    /// must not be simplified into requeueing both halves.
    fn split(&mut self, sc: &mut Scratch) {
        while let Some(s) = sc.w.pop() {
            let j = self.first[s] + sc.m[s];
            if j == self.past[s] {
                // Everything in the set is marked: nothing to separate.
                sc.m[s] = 0;
                continue;
            }
            if sc.m[s] <= self.past[s] - j {
                // Marked half [first[s], j) is the smaller one.
                self.first[self.z] = self.first[s];
                self.past[self.z] = j;
                self.first[s] = j;
            } else {
                // Unmarked half [j, past[s]) is the smaller one.
                self.past[self.z] = self.past[s];
                self.first[self.z] = j;
                self.past[s] = j;
            }
            for i in self.first[self.z]..self.past[self.z] {
                self.set_of[self.elems[i]] = self.z;
            }
            sc.m[s] = 0;
            sc.m[self.z] = 0;
            self.z += 1;
        }
    }
}

/// Counting-sort the transition ids into buckets keyed by `key[t]`.
///
/// `adj_first[q]..adj_first[q + 1]` ends up indexing `adj` for the transitions whose key
/// is `q`. Ports `ValmariDFA.make_adjacent`; `adj_first` has length `num_states + 1` and
/// `adj` may be longer than `key` (it is allocated once at the original transition count
/// and reused after the arrays shrink, exactly as Java does).
fn make_adjacent(key: &[usize], num_states: usize, adj_first: &mut [usize], adj: &mut [usize]) {
    adj_first.fill(0);
    for &k in key {
        adj_first[k] += 1;
    }
    for q in 0..num_states {
        adj_first[q + 1] += adj_first[q];
    }
    for (t, &k) in key.iter().enumerate().rev() {
        adj_first[k] -= 1;
        adj[adj_first[k]] = t;
    }
}

/// Records `q` as co-reachable (`ValmariDFA.reach`): swaps it into the reached prefix
/// `elems[..rr]` of the block partition.
fn reach(blocks: &mut Partition, rr: &mut usize, q: usize) {
    let i = blocks.loc[q];
    if i >= *rr {
        blocks.elems[i] = blocks.elems[*rr];
        blocks.loc[blocks.elems[i]] = i;
        blocks.elems[*rr] = q;
        blocks.loc[q] = *rr;
        *rr += 1;
    }
}

/// Minimizes a deterministic automaton via Valmari's algorithm, returning a new [`Fa`]
/// (the crate convention — `determinize`/`trim` also build fresh values rather than
/// mutating in place).
///
/// Transitions whose head cannot reach an accepting state are dropped, so the result is
/// generally a *partial* DFA even if the input was total; totalize explicitly if a caller
/// needs totality (e.g. before the equivalence oracle). State numbering is not
/// canonical — compare results by language, never by structure (`CLAUDE.md` prime
/// directive #1).
///
/// See the module docs for the two preconditions: deterministic input (enforced), and
/// all-states-reachable-from-`q0` (documented; run [`crate::trim::trim`] first if unsure).
pub fn minimize(fa: &Fa) -> Result<Fa, MinimizeError> {
    if !fa.is_deterministic() {
        return Err(MinimizeError::NotDeterministic);
    }
    // Java would index `blocks.P[0]` on a zero-length array here; pass the degenerate
    // automaton through instead, matching `trim`'s handling of the same case.
    if fa.q == 0 {
        return Ok(fa.clone());
    }
    let num_states = fa.q;

    // walnut-rs instrumentation (`crate::resource`, no Java counterpart) -- inert when
    // nothing is installed. Minimization allocates in proportion to its INPUT (which was
    // itself built under the same budget), so one check at entry is the whole budget
    // story here; the two events are what a trajectory observer pairs with the
    // preceding subset construction to diagnose a transient explosion.
    let meter = crate::resource::Meter::current();
    meter.check(crate::resource::Operation::Minimize, num_states);
    meter.emit(|| crate::resource::Event::MinimizeStarted { states: num_states });

    // Flatten the transition table into Valmari's tail/label/head triple arrays. A
    // deterministic `Fa` has at most one destination per (state, symbol); empty
    // destination lists are skipped, mirroring `FA.convertNFAtoDFA`'s `isEmpty()` guard.
    let mut tail: Vec<usize> = Vec::new();
    let mut label: Vec<i32> = Vec::new();
    let mut head: Vec<usize> = Vec::new();
    for q in 0..num_states {
        for (&sym, dests) in &fa.d[q] {
            for &dest in dests {
                tail.push(q);
                label.push(sym);
                head.push(dest);
            }
        }
    }
    let original_transition_count = tail.len();

    let mut blocks = Partition::init(num_states);

    // Seed co-reachability from every accepting state, then close it backwards.
    let mut rr = 0usize;
    for q in 0..num_states {
        if fa.is_accepting(q) {
            reach(&mut blocks, &mut rr, q);
        }
    }
    // Positions `0..num_final_states` now hold exactly the accepting states, and nothing
    // that follows ever moves an element across that boundary — which is what makes the
    // `first[b] < num_final_states` output test below correct.
    let num_final_states = rr;

    let mut adj = vec![0usize; original_transition_count];
    let mut adj_first = vec![0usize; num_states + 1];

    // --- rem_unreachable ---
    make_adjacent(&head, num_states, &mut adj_first, &mut adj);
    // `rr` grows inside the loop; Java re-tests `i < rr` each iteration, so this is a
    // worklist walk, not a fixed-length scan.
    let mut i = 0usize;
    while i < rr {
        let q = blocks.elems[i];
        for j in adj_first[q]..adj_first[q + 1] {
            reach(&mut blocks, &mut rr, tail[adj[j]]);
        }
        i += 1;
    }
    // Keep only transitions whose head is co-reachable (their tails then are too).
    let mut kept = 0usize;
    for t in 0..tail.len() {
        if blocks.loc[head[t]] < rr {
            tail[kept] = tail[t];
            label[kept] = label[t];
            head[kept] = head[t];
            kept += 1;
        }
    }
    tail.truncate(kept);
    label.truncate(kept);
    head.truncate(kept);
    blocks.past[0] = rr;
    // `numCoreachable` (`ValmariDFA.java`, the WB-001 fix): the size of the co-reachable
    // set, i.e. the exclusive upper bound on the positions that any block will ever
    // track. Every state at a position `>= num_coreachable` was parked here and carries
    // a meaningless `set_of` entry forever after — see the module docs.
    //
    // Java has to stash this in a field because its `rr` is reset to `0` on this very
    // line and `blocks.P[0]`, which momentarily holds the same value, is subsequently
    // mutated by `split()`. This port's `rr` is a plain local that is never reset, so a
    // binding here is enough; it is taken all the same, so the value the guard reads is
    // fixed at the one moment it is meaningful rather than depending on `rr` staying
    // untouched through 70 more lines of refinement.
    let num_coreachable = rr;
    let num_transitions = tail.len();

    // Deviation from Java: `M`/`W` are sized `numTransitions + 1` there, but `M` is also
    // indexed by *block* id (up to `num_states`), so a state-heavy / transition-light
    // automaton could overrun it. Size for both users; `w` is a growable stack.
    let mut sc = Scratch {
        m: vec![0; num_states.max(num_transitions) + 1],
        w: Vec::new(),
    };

    // --- initial partition: accepting vs non-accepting ---
    sc.m[0] = num_final_states;
    if num_final_states != 0 {
        sc.w.push(0);
        blocks.split(&mut sc);
    }

    // --- transition partition: one set per label, later refined by head block ---
    let mut cords = Partition::init(num_transitions);
    if num_transitions != 0 {
        // Java uses `IntArrays.quickSort` (unstable). Ordering within a label group is
        // not load-bearing: it permutes elements inside a set, which can change the
        // final set *numbering* but never the set partition itself.
        cords.elems.sort_unstable_by_key(|&t| label[t]);
        cords.z = 0;
        sc.m[0] = 0;
        let mut current = label[cords.elems[0]];
        for i in 0..num_transitions {
            let t = cords.elems[i];
            if label[t] != current {
                current = label[t];
                cords.past[cords.z] = i;
                cords.z += 1;
                cords.first[cords.z] = i;
                sc.m[cords.z] = 0;
            }
            cords.set_of[t] = cords.z;
            cords.loc[t] = i;
        }
        cords.past[cords.z] = num_transitions;
        cords.z += 1;
    }

    // --- refinement fixpoint ---
    make_adjacent(&head, num_states, &mut adj_first, &mut adj);
    // `b` starts at 1: block 0 is the larger half of the initial split and never needs
    // scanning. `c` starts at 0: every label cord must be processed. Both `blocks.z` and
    // `cords.z` grow during the loop, and each new set id is visited exactly once.
    let mut b = 1usize;
    let mut c = 0usize;
    while c < cords.z {
        for i in cords.first[c]..cords.past[c] {
            blocks.mark(tail[cords.elems[i]], &mut sc);
        }
        blocks.split(&mut sc);
        c += 1;
        while b < blocks.z {
            for i in blocks.first[b]..blocks.past[b] {
                let q = blocks.elems[i];
                for &t in &adj[adj_first[q]..adj_first[q + 1]] {
                    cords.mark(t, &mut sc);
                }
            }
            cords.split(&mut sc);
            b += 1;
        }
    }

    // --- rebuild (`replaceFields` / `determineDfaD` / `determineO`) ---

    // `docs/WALNUT-BUGS.md` WB-001's guard, at Java's own placement (the first statement
    // of `replaceFields`). `q0` sitting at a parked position means no accepting state is
    // reachable from it at all, so the language is empty; `blocks.set_of[q0]` below would
    // read the stale `0` from `Partition::init` and alias `q0` onto whichever block
    // happens to hold id `0` — never an empty-language block. See the module docs.
    //
    // Reading `blocks.loc[q0]` this late is safe (and is what Java does): the refinement
    // above cannot move a parked element. `mark` is only ever called on `tail[…]` of a
    // *surviving* transition, and a surviving transition's head is co-reachable, hence so
    // is its tail — so every element `mark` touches, and every position `mark`/`split`
    // write to, lies inside some block's range and therefore below `num_coreachable`.
    //
    // An out-of-range `fa.q0` still panics identically: `blocks.loc` and `blocks.set_of`
    // are both `Partition::init`'s `num_states`-length vectors, and pre-fix the first
    // `q0` index in this function was `blocks.set_of[fa.q0]` a few lines below. Nothing
    // between the two points indexes by `q0`, so the guard moves neither the panic's
    // reachability nor its message (Java's `replaceFields` has the same property).
    if blocks.loc[fa.q0] >= num_coreachable {
        // `replaceWithEmptyLanguage`: the canonical minimal empty-language automaton —
        // one non-accepting state, no transitions (Walnut's DFAs are partial, so a
        // transition-less state is a well-formed sink). `alphabet_size` is carried over
        // untouched, exactly as Java leaves `FA.alphabetSize` alone here.
        return Ok(Fa::with_states(
            0,
            1,
            fa.alphabet_size,
            vec![0],
            vec![BTreeMap::new()],
        ));
    }

    let new_q = blocks.z;
    let new_q0 = blocks.set_of[fa.q0];
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); new_q];
    for (t, &tl) in tail.iter().enumerate() {
        // Take each block's transitions from its canonical first element only.
        if blocks.loc[tl] != blocks.first[blocks.set_of[tl]] {
            continue;
        }
        let src = blocks.set_of[tl];
        let sym = label[t];
        let dest = blocks.set_of[head[t]];
        if let Some(existing) = d[src].get(&sym) {
            if existing[0] != dest {
                return Err(MinimizeError::ConflictingTransitions);
            }
        }
        d[src].insert(sym, vec![dest]);
    }
    let o = (0..new_q)
        .map(|q| i32::from(blocks.first[q] < num_final_states))
        .collect();
    meter.emit(|| crate::resource::Event::MinimizeFinished {
        before: num_states,
        after: new_q,
    });

    Ok(Fa::with_states(new_q0, new_q, fa.alphabet_size, o, d))
}

/// [`minimize`], bracketed in `FA.justMinimize`'s own `Minimizing:`/`Minimized:` logging
/// (`FA.java:576-588`) — see [`crate::logging::Logging`]'s docs for the format. Java's
/// `justMinimize` is called from (at least) five independent Rust call sites
/// (`logicalops::just_minimize`, `Automaton::determinize_and_minimize_with_ctx`/
/// `_from_with_ctx`, `product::cross_product_and_minimize`, `determinize::brzozowski`),
/// every one of which logs unconditionally in Java — this wrapper exists so the pair
/// isn't duplicated five times.
///
/// The space differs between the two messages and is not a typo: `MINIMIZING + ": "` vs
/// `MINIMIZED + ":"` (no space) — both port verbatim.
pub fn minimize_with_logging(
    fa: &Fa,
    logging: &mut crate::logging::Logging,
) -> Result<Fa, MinimizeError> {
    let time_before = std::time::Instant::now();
    logging.log_message(&format!("{}: {} states.", crate::logging::MINIMIZING, fa.q));
    let result = minimize(fa)?;
    logging.log_message(&format!(
        "{}:{} states - {}ms.",
        crate::logging::MINIMIZED,
        result.q,
        time_before.elapsed().as_millis()
    ));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equiv::language_equivalent;
    use crate::trim::trim;
    use proptest::prelude::*;
    use std::collections::BTreeMap as Map;

    fn row(pairs: &[(i32, usize)]) -> Map<i32, Vec<usize>> {
        pairs.iter().map(|&(s, d)| (s, vec![d])).collect()
    }

    /// Convenience for the oracle, which demands total DFAs on both sides.
    fn totalized(fa: &Fa) -> Fa {
        let mut t = fa.clone();
        t.totalize(0);
        t
    }

    #[test]
    fn merges_two_equivalent_accepting_sinks() {
        // States 1 and 2 both accept every continuation (Σ*), by construction — they are
        // language-equivalent but structurally distinct, so a correct minimizer must
        // merge them. Minimal result: 2 states (recognizing "any nonempty word").
        let fa = Fa::with_states(
            0,
            3,
            2,
            vec![0, 1, 1],
            vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 2), (1, 2)]),
            ],
        );
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2, "the two equivalent sinks must collapse into one");
        assert_eq!(
            language_equivalent(&totalized(&fa), &totalized(&min)),
            Ok(true)
        );
        assert!(!min.accepts_word(&[]));
        assert!(min.accepts_word(&[1, 0, 1]));
    }

    #[test]
    fn already_minimal_dfa_is_unchanged_in_size() {
        // "contains at least one 1" — 2 states, provably minimal (ε is rejected, `1` is
        // accepted, so the two states are distinguishable).
        let fa = Fa::with_states(
            0,
            2,
            2,
            vec![0, 1],
            vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        );
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2, "no false merge on an already-minimal DFA");
        assert_eq!(
            language_equivalent(&totalized(&fa), &totalized(&min)),
            Ok(true)
        );
    }

    #[test]
    fn rejects_nondeterministic_input() {
        let mut fa = Fa::with_states(
            0,
            2,
            2,
            vec![0, 1],
            vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        );
        fa.d[0].insert(1, vec![0, 1]); // two destinations for symbol 1
        assert_eq!(minimize(&fa).unwrap_err(), MinimizeError::NotDeterministic);
    }

    #[test]
    fn no_accepting_states_collapses_to_one_dead_state() {
        let fa = Fa::with_states(
            0,
            3,
            2,
            vec![0, 0, 0],
            vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 2), (1, 0)]),
                row(&[(0, 0), (1, 1)]),
            ],
        );
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 1);
        assert!(min.is_language_empty());
        // Every transition was dropped: nothing had a co-reachable head.
        assert!(min.d[0].is_empty());
    }

    #[test]
    fn drops_states_that_cannot_reach_acceptance() {
        // State 2 is a non-accepting sink: reachable from q0 but not co-reachable, so
        // Valmari's own pre-pass removes it (and its transitions) even though `minimize`
        // does no q0-reachability pruning of its own.
        let fa = Fa::with_states(
            0,
            3,
            2,
            vec![0, 1, 0],
            vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 2), (1, 2)]),
            ],
        );
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2);
        assert!(min.accepts_word(&[0, 0]));
        assert!(!min.accepts_word(&[0, 1]));
        assert_eq!(
            language_equivalent(&totalized(&fa), &totalized(&min)),
            Ok(true)
        );
    }

    /// `docs/WALNUT-BUGS.md` WB-001's minimal verified trigger, now asserting the CORRECT
    /// answer — the fix landed here and upstream (`walnut-java` commit `14509f1`).
    ///
    /// This test formerly asserted the bug (`min.is_accepting(min.q0)`, language `Σ*`)
    /// under the name `minimize_q0_not_co_reachable_walnut_quirk`. It is flipped, not
    /// deleted, per `CLAUDE.md`'s merge gate; the pre-fix expectation is recorded in the
    /// message below so a regression reads as "WB-001 is back", not as an anonymous
    /// assertion failure.
    #[test]
    fn minimize_wb_001_trigger_now_yields_the_empty_language() {
        // q0 self-loops and is non-accepting; state 1 is a disjoint accepting self-loop
        // that q0 can never reach. The true language is ∅.
        let fa = Fa::with_states(0, 2, 1, vec![0, 1], vec![row(&[(0, 0)]), row(&[(0, 1)])]);
        assert!(fa.is_language_empty());

        let min = minimize(&fa).unwrap();
        // The canonical empty-language shape, asserted field by field rather than just as
        // "the language is empty": one non-accepting state, no transitions, start state 0.
        assert_eq!(min.q, 1);
        assert_eq!(min.q0, 0);
        assert_eq!(min.o, vec![0], "WB-001 regression: q0 aliased onto block 0");
        assert_eq!(min.d, vec![Map::new()]);
        assert_eq!(
            min.alphabet_size, fa.alphabet_size,
            "the guard must not disturb the alphabet"
        );
        assert!(min.is_language_empty());

        // Trimming first was the documented workaround while the bug was live; it now
        // reaches the identical answer by the other route, which is the point.
        let min_trimmed = minimize(&trim(&fa)).unwrap();
        assert!(min_trimmed.is_language_empty());
        assert_eq!(min_trimmed.q, 1);
    }

    /// The other half of WB-001's fix claim: the guard is a **no-op** on every input that
    /// does not hit the defect. Mirrors `walnut-java`'s
    /// `ordinaryMinimizationIsStructurallyUnchangedByTheGuard`.
    ///
    /// The pre-fix outputs below were captured by running the identical fixtures against
    /// this function with the guard removed, and are asserted **structurally** (state
    /// count, numbering, start state, exact transition table, exact output vector) rather
    /// than by language — a language-only check would not detect the guard perturbing
    /// state numbering, which is what "byte-for-byte identical" has to mean here given
    /// how central `minimize` is.
    #[test]
    fn the_guard_is_a_no_op_on_every_non_defective_input() {
        // (a) q0 IS co-reachable: "contains at least one 1", plus a stranded but
        //     co-reachable extra state so the input is genuinely untrimmed.
        let ordinary = Fa::with_states(
            0,
            3,
            2,
            vec![0, 1, 1],
            vec![
                row(&[(0, 0), (1, 1)]),
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 2), (1, 2)]), // unreachable from q0, but accepting
            ],
        );
        let min = minimize(&ordinary).unwrap();
        assert_eq!(min.q, 2, "the two Σ*-sinks merge; q0 stays separate");
        assert_eq!(min.o, vec![1, 0]);
        assert_eq!(min.q0, 1);
        assert_eq!(
            min.d,
            vec![row(&[(0, 0), (1, 0)]), row(&[(0, 1), (1, 0)])],
            "exact table, pre-fix capture"
        );

        // (b) num_coreachable == 0 (no accepting state anywhere): the guard fires for
        //     every q0, and the unguarded path already produced exactly this.
        let dead = Fa::with_states(
            1,
            3,
            2,
            vec![0, 0, 0],
            vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 2), (1, 0)]),
                row(&[(0, 0), (1, 1)]),
            ],
        );
        let min = minimize(&dead).unwrap();
        assert_eq!(
            (min.q, min.q0, &min.o, &min.d),
            (1, 0, &vec![0], &vec![Map::new()])
        );
    }

    // ---------------------------------------------------------------------------
    // WB-001: the exhaustive + randomized verification sweep
    //
    // Mirrors the 196,798-case differential sweep `walnut-java` commit `14509f1` ran
    // against live-built jars of itself and its parent. That approach (two builds, diff
    // the dumps) is not available inside one test binary, so the same two claims are
    // established here by different means, both of them checkable on every run:
    //
    //   1. **Correctness.** Every swept automaton's minimized language equals its own,
    //      decided by `same_language` below -- a from-scratch product BFS over PARTIAL
    //      DFAs, written for this check and calling nothing in this crate.
    //   2. **The fix is a no-op off the defect shape.** `NON_DEFECTIVE_DIGEST` is an
    //      FNV-1a digest of the exhaustive sweep's full output (state count, start
    //      state, output vector, transition table -- structure, not language) over every
    //      case where the guard does NOT fire. It was captured by running this very
    //      sweep against a build with the guard's condition forced to `false`, i.e.
    //      against the pre-fix code. A guard that perturbed any non-defective case --
    //      even only its state numbering -- changes this constant.
    //
    // ---------------------------------------------------------------------------

    /// Do `a` and `b` accept the same language? A product BFS over `(Option<usize>,
    /// Option<usize>)` pairs, where `None` means "already fell out of the automaton" --
    /// both engines' DFAs are partial and a missing transition is an implicit reject, so
    /// this decides equivalence directly, with no totalization and no word-length bound.
    ///
    /// Deliberately does NOT reuse [`crate::equiv`]: it is the oracle for a fix inside
    /// `minimize`, and `equiv` is a sibling module with its own conventions (it demands
    /// total DFAs, which would mean pre-processing the very automata under test).
    /// Terminates because the reachable pair set is finite (`(|A|+1)·(|B|+1)`).
    fn same_language(a: &Fa, b: &Fa) -> bool {
        assert_eq!(a.alphabet_size, b.alphabet_size, "different alphabets");
        let step = |fa: &Fa, s: Option<usize>, sym: i32| -> Option<usize> {
            let s = s?;
            fa.d[s].get(&sym).and_then(|dests| dests.first().copied())
        };
        let accepts = |fa: &Fa, s: Option<usize>| s.is_some_and(|s| fa.is_accepting(s));

        let start = (Some(a.q0), Some(b.q0));
        let mut seen: std::collections::BTreeSet<(Option<usize>, Option<usize>)> =
            [start].into_iter().collect();
        let mut stack = vec![start];
        while let Some((x, y)) = stack.pop() {
            if accepts(a, x) != accepts(b, y) {
                return false;
            }
            for sym in 0..a.alphabet_size as i32 {
                let next = (step(a, x, sym), step(b, y, sym));
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        true
    }

    /// Can `q0` reach any accepting state? Plain forward BFS.
    ///
    /// This is WB-001's trigger condition stated the other way round from the algorithm's
    /// own: Valmari parks `q0` exactly when no accepting state is *backward*-co-reachable
    /// to it, and the two are the same set. Written forwards on purpose, so the sweep's
    /// expectation is derived independently of the code it is checking.
    fn q0_reaches_acceptance(fa: &Fa) -> bool {
        let mut seen = vec![false; fa.q];
        seen[fa.q0] = true;
        let mut stack = vec![fa.q0];
        while let Some(s) = stack.pop() {
            if fa.is_accepting(s) {
                return true;
            }
            for dests in fa.d[s].values() {
                for &t in dests {
                    if !seen[t] {
                        seen[t] = true;
                        stack.push(t);
                    }
                }
            }
        }
        false
    }

    /// FNV-1a over the bytes of a stable structural rendering of `fa`. Hand-rolled rather
    /// than `DefaultHasher`, whose output std does not promise to keep stable across
    /// releases -- this digest is a captured constant and has to survive toolchain bumps.
    fn structural_digest(acc: &mut u64, fa: &Fa) {
        let rendered = format!(
            "{};{};{};{:?};{:?}",
            fa.q, fa.q0, fa.alphabet_size, fa.o, fa.d
        );
        for b in rendered.as_bytes() {
            *acc ^= u64::from(*b);
            *acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Every partial DFA with exactly `q` states over `alphabet_size` symbols: every
    /// transition table (each `(state, symbol)` pair independently absent or pointing at
    /// any state), every accepting set, and every start state -- the same three axes
    /// `walnut-java`'s `exhaustiveTinyAutomataKeepTheirLanguage` enumerates.
    fn for_each_small_partial_dfa(q: usize, alphabet_size: usize, f: &mut impl FnMut(Fa)) {
        let slots = q * alphabet_size;
        let radix = (q + 1) as u64; // 0 = no transition; k >= 1 = state k-1
        let tables = radix.pow(slots as u32);
        for table in 0..tables {
            let mut d: Vec<Map<i32, Vec<usize>>> = vec![Map::new(); q];
            let mut rest = table;
            for slot in 0..slots {
                let code = rest % radix;
                rest /= radix;
                if code > 0 {
                    d[slot / alphabet_size]
                        .insert((slot % alphabet_size) as i32, vec![(code - 1) as usize]);
                }
            }
            for o_mask in 0..(1u32 << q) {
                let o: Vec<i32> = (0..q).map(|s| i32::from(o_mask >> s & 1 == 1)).collect();
                for q0 in 0..q {
                    f(Fa::with_states(q0, q, alphabet_size, o.clone(), d.clone()));
                }
            }
        }
    }

    /// FNV-1a digest of the exhaustive sweep's structural output over every case whose
    /// `q0` DOES reach acceptance -- i.e. every case the WB-001 guard must not touch.
    ///
    /// Captured from a build of this file with the guard's condition forced to `false`
    /// (the pre-fix code path), running exactly the sweep below. It is therefore direct
    /// evidence, not a restatement of the current behaviour: if the guard perturbed even
    /// one non-defective case's state numbering, this constant would not match. The two
    /// runs agreed exactly, over all 73,926 non-defective cases.
    ///
    /// Reproducing the capture: force the `if blocks.loc[fa.q0] >= num_coreachable`
    /// condition in [`minimize`] to `false`, early-`return` from this sweep's closure on
    /// `!q0_reaches_acceptance(&fa)` (pre-fix, those cases genuinely compute the wrong
    /// language and would trip the oracle -- which is itself worth doing once, as
    /// independent confirmation that this sweep detects WB-001), and print `digest`.
    const NON_DEFECTIVE_DIGEST: u64 = 11_238_673_596_080_511_772;

    #[test]
    fn wb_001_exhaustive_small_sweep() {
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis
        let mut total = 0usize;
        let mut defective = 0usize;
        // `Fa` has no `PartialEq` (production type; not derived just for a test), so
        // the canonical shape is asserted field by field.
        let assert_canonical_empty = |min: &Fa, fa: &Fa| {
            assert_eq!(
                (min.q, min.q0, &min.o, &min.d, min.alphabet_size),
                (1, 0, &vec![0], &vec![Map::new()], fa.alphabet_size),
                "WB-001: {fa:?} has an empty language and must minimize to the \
                 canonical empty automaton"
            );
        };

        for q in 1..=3usize {
            for alphabet_size in 1..=2usize {
                for_each_small_partial_dfa(q, alphabet_size, &mut |fa| {
                    total += 1;
                    let min = minimize(&fa).expect("every generated table is deterministic");

                    // (1) Correctness, against the independent oracle.
                    assert!(
                        same_language(&fa, &min),
                        "minimize changed the language of {fa:?} -> {min:?}"
                    );
                    assert!(min.q <= fa.q, "minimize grew {fa:?} -> {min:?}");
                    assert_eq!(min.alphabet_size, fa.alphabet_size);

                    // (2) The guard fires on exactly the defect shape, and nowhere else.
                    if q0_reaches_acceptance(&fa) {
                        structural_digest(&mut digest, &min);
                    } else {
                        defective += 1;
                        assert_canonical_empty(&min, &fa);
                    }
                });
            }
        }

        assert_eq!(
            total, 100_572,
            "the sweep's own size, so it cannot silently shrink"
        );
        // Pinned exactly, so that a change which quietly stopped generating (or stopped
        // classifying) the WB-001 cases cannot leave this test green and vacuous.
        //
        // 26,646 of the 100,572 cases have a parked `q0`. Of those, 13,980 ALSO have an
        // accepting state somewhere and so genuinely computed the wrong language pre-fix;
        // the remaining 12,666 have no accepting state at all, where the guard is provably
        // a no-op (module docs). Both figures were measured on the pre-fix code path, and
        // the equivalence `wrong <=> (parked q0 AND some accepting state)` was asserted
        // case by case across all 100,572 -- WB-001's reach is exactly "the language is
        // empty and the automaton does not know it", nothing wider.
        assert_eq!(defective, 26_646, "guard-fires count");
        assert_eq!(
            digest, NON_DEFECTIVE_DIGEST,
            "the WB-001 guard perturbed a case it must not touch (digest captured from \
             the pre-fix code path -- see NON_DEFECTIVE_DIGEST)"
        );
    }

    /// The randomized half, over automata too large to enumerate: up to 6 states and 3
    /// symbols, partial, arbitrary start state. Mirrors `walnut-java`'s
    /// `randomLargerAutomataKeepTheirLanguage`.
    ///
    /// Uses a fixed-seed xorshift64* rather than `proptest` so the case set is identical
    /// on every run and on every machine -- a sweep whose job is to say "N cases, zero
    /// wrong" should sweep the same N cases each time.
    #[test]
    fn wb_001_randomized_larger_sweep() {
        let mut state: u64 = 0x2026_0820_0000_0001;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut defective = 0usize;
        const CASES: usize = 20_000;

        for _ in 0..CASES {
            let q = 1 + (next() % 6) as usize;
            let alphabet_size = 1 + (next() % 3) as usize;
            let o: Vec<i32> = (0..q).map(|_| i32::from(next() % 3 == 0)).collect();
            let mut d: Vec<Map<i32, Vec<usize>>> = vec![Map::new(); q];
            for (s, row) in d.iter_mut().enumerate() {
                let _ = s;
                for sym in 0..alphabet_size {
                    // ~1 in 4 transitions missing, so partial rows are common but the
                    // graph is still usually connected enough to be interesting.
                    if next() % 4 != 0 {
                        row.insert(sym as i32, vec![(next() % q as u64) as usize]);
                    }
                }
            }
            let fa = Fa::with_states((next() % q as u64) as usize, q, alphabet_size, o, d);

            let min = minimize(&fa).expect("generated tables are deterministic by construction");
            assert!(
                same_language(&fa, &min),
                "minimize changed the language of {fa:?} -> {min:?}"
            );
            assert!(min.q <= fa.q);
            if !q0_reaches_acceptance(&fa) {
                defective += 1;
                assert_eq!(min.q, 1);
                assert_eq!(min.o, vec![0]);
                assert!(min.d[0].is_empty());
            }
        }
        assert!(
            defective > CASES / 100,
            "the generator must actually produce the defect shape often enough to be \
             evidence: only {defective} of {CASES}"
        );
    }

    /// Random small total DFA (`q0 = 0`, every (state, symbol) pair mapped), same shape
    /// as `equiv`'s generator — totality keeps the oracle's precondition satisfiable on
    /// the input side without extra work. `alpha_max` may be 0 (empty-alphabet
    /// automata are a named edge case in `CLAUDE.md`'s correctness ladder, and are
    /// otherwise never generated by any range starting at `1..=`).
    fn arb_total_dfa(q_max: usize, alpha_max: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max, 0..=alpha_max).prop_flat_map(|(q, alphabet_size)| {
            let o_strategy = prop::collection::vec(0i32..=1, q);
            let trans_strategy =
                prop::collection::vec(prop::collection::vec(0usize..q, alphabet_size), q);
            (o_strategy, trans_strategy).prop_map(move |(o, trans)| {
                let d = trans
                    .into_iter()
                    .map(|r| {
                        r.into_iter()
                            .enumerate()
                            .map(|(sym, dest)| (sym as i32, vec![dest]))
                            .collect::<Map<i32, Vec<usize>>>()
                    })
                    .collect();
                Fa::with_states(0, q, alphabet_size, o, d)
            })
        })
    }

    /// Random small PARTIAL DFA (`q0 = 0`, each `(state, symbol)` pair independently
    /// present or absent) — unlike `arb_total_dfa`, this exercises `minimize`'s
    /// sparse-row rebuild path (`rem_unreachable`'s transition filter and
    /// `determineDfaD`'s possibly-missing-entry handling), which a total-only
    /// generator never reaches.
    fn arb_partial_dfa(q_max: usize, alpha_max: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max, 0..=alpha_max).prop_flat_map(|(q, alphabet_size)| {
            let o_strategy = prop::collection::vec(0i32..=1, q);
            let trans_strategy = prop::collection::vec(
                prop::collection::vec(prop::option::of(0usize..q), alphabet_size),
                q,
            );
            (o_strategy, trans_strategy).prop_map(move |(o, trans)| {
                let d = trans
                    .into_iter()
                    .map(|r| {
                        r.into_iter()
                            .enumerate()
                            .filter_map(|(sym, dest)| dest.map(|d| (sym as i32, vec![d])))
                            .collect::<Map<i32, Vec<usize>>>()
                    })
                    .collect();
                Fa::with_states(0, q, alphabet_size, o, d)
            })
        })
    }

    /// Independent Moore/Myhill-Nerode reference minimizer for a TOTAL DFA, sharing no
    /// code with Valmari: refine (acceptance, then transition signatures) to a fixpoint
    /// over the `q0`-reachable states, then count the resulting classes that can still
    /// reach an accepting class.
    ///
    /// That last restriction is what makes the count directly comparable to `minimize`'s
    /// output size: Valmari drops non-co-reachable states outright, so it yields the
    /// minimal *partial* DFA, which is the minimal total DFA minus its dead class.
    fn moore_co_reachable_class_count(fa: &Fa) -> usize {
        let alphabet: Vec<i32> = (0..fa.alphabet_size as i32).collect();

        let mut reachable = vec![false; fa.q];
        let mut stack = vec![fa.q0];
        reachable[fa.q0] = true;
        while let Some(s) = stack.pop() {
            for &sym in &alphabet {
                let dest = fa.d[s][&sym][0];
                if !reachable[dest] {
                    reachable[dest] = true;
                    stack.push(dest);
                }
            }
        }

        let mut class: Vec<usize> = fa.o.iter().map(|&o| usize::from(o != 0)).collect();
        let mut count = (0..fa.q)
            .filter(|&s| reachable[s])
            .map(|s| class[s])
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        loop {
            let mut sigs: std::collections::BTreeMap<(usize, Vec<usize>), usize> =
                std::collections::BTreeMap::new();
            let mut next_class = vec![0usize; fa.q];
            for s in 0..fa.q {
                if !reachable[s] {
                    continue;
                }
                let key = (
                    class[s],
                    alphabet
                        .iter()
                        .map(|&sym| class[fa.d[s][&sym][0]])
                        .collect(),
                );
                let fresh = sigs.len();
                next_class[s] = *sigs.entry(key).or_insert(fresh);
            }
            class = next_class;
            // Refinement only ever splits, so an unchanged class count is a fixpoint.
            if sigs.len() == count {
                break;
            }
            count = sigs.len();
        }

        // Backward closure from accepting classes over the quotient graph.
        let mut class_accepting = vec![false; count];
        let mut quotient_edges: Vec<Vec<usize>> = vec![Vec::new(); count];
        for s in 0..fa.q {
            if !reachable[s] {
                continue;
            }
            class_accepting[class[s]] |= fa.is_accepting(s);
            for &sym in &alphabet {
                quotient_edges[class[s]].push(class[fa.d[s][&sym][0]]);
            }
        }
        let mut reverse: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (src, dests) in quotient_edges.iter().enumerate() {
            for &dest in dests {
                reverse[dest].push(src);
            }
        }
        let mut co_reachable = class_accepting.clone();
        let mut stack: Vec<usize> = (0..count).filter(|&c| co_reachable[c]).collect();
        while let Some(c) = stack.pop() {
            for &p in &reverse[c] {
                if !co_reachable[p] {
                    co_reachable[p] = true;
                    stack.push(p);
                }
            }
        }
        co_reachable.iter().filter(|&&b| b).count()
    }

    proptest! {
        /// Tier-4 (CLAUDE.md §correctness ladder): the ported Valmari must agree with an
        /// independently-written Moore minimizer on the resulting state count. This is
        /// the property that catches *under*-merging — language preservation and
        /// idempotence are both satisfied by an identity function, this one is not.
        ///
        /// **This one keeps its `trim`, and NOT for the WB-001 reason its siblings above
        /// shed theirs.** `minimize` does no forward-reachability pruning of its own
        /// (module docs, "two reachability notions"), so on an untrimmed input a state
        /// that is unreachable from `q0` but still co-reachable survives as one or more
        /// extra blocks — the result is language-correct but larger than minimal, and an
        /// exact state-count comparison against a reference that *does* prune would fail
        /// for that reason alone. The trim establishes the minimality precondition, which
        /// outlives the WB-001 fix.
        ///
        /// `max(1, ...)` covers the empty language, where Valmari (via `trim`'s canonical
        /// empty automaton) returns a single dead state but zero classes are
        /// co-reachable.
        #[test]
        fn minimize_agrees_with_moore_reference(fa in arb_total_dfa(6, 3)) {
            let min = minimize(&trim(&fa)).unwrap();
            let expected = moore_co_reachable_class_count(&fa).max(1);
            prop_assert_eq!(min.q, expected);
        }

        /// Tier-4 property #3 (DESIGN.md §5): minimize preserves language, checked
        /// against the `equiv` oracle (both sides totalized, since minimize returns a
        /// partial DFA), on the RAW generated automaton.
        ///
        /// `trim` used to be applied first because the generator freely produces automata
        /// whose `q0` cannot reach acceptance — which was WB-001's trigger, and the one
        /// shape where `minimize` genuinely did not preserve the language. WB-001 is fixed
        /// (`walnut-java` commit `14509f1`; see the module docs), so the trim is removed
        /// and the property now covers exactly the case it used to have to exclude. The
        /// sibling `minimize_agrees_with_moore_reference` still trims, for an unrelated
        /// reason it explains itself.
        #[test]
        fn minimize_preserves_language(fa in arb_total_dfa(6, 3)) {
            let min = minimize(&fa).unwrap();
            prop_assert_eq!(
                language_equivalent(&totalized(&fa), &totalized(&min)),
                Ok(true)
            );
        }

        /// Tier-4 property #4 (DESIGN.md §5): minimize is idempotent — a second pass
        /// finds nothing further to merge, and does not disturb the language. The FIRST
        /// pass keeps its `trim` so that `once` really is the minimal automaton and
        /// `once.q == twice.q` is the sharp statement rather than a weaker one about an
        /// arbitrary fixpoint; no `trim` is needed on the second pass, since every state
        /// of a minimize output is co-reachable and reachable by construction.
        #[test]
        fn minimize_is_idempotent(fa in arb_total_dfa(6, 3)) {
            let once = minimize(&trim(&fa)).unwrap();
            let twice = minimize(&once).unwrap();
            prop_assert_eq!(once.q, twice.q);
            prop_assert_eq!(
                language_equivalent(&totalized(&once), &totalized(&twice)),
                Ok(true)
            );
        }

        /// Minimization must not *grow* the (co-reachable part of the) automaton, and
        /// its output is always a well-formed DFA.
        #[test]
        fn minimize_never_grows_and_stays_deterministic(fa in arb_total_dfa(6, 3)) {
            let trimmed = trim(&fa);
            let min = minimize(&trimmed).unwrap();
            prop_assert!(min.is_deterministic());
            prop_assert!(min.q <= trimmed.q);
            prop_assert_eq!(min.alphabet_size, fa.alphabet_size);
        }

        /// Same as `minimize_preserves_language`, but over PARTIAL DFAs — closes a real
        /// coverage gap the total-only generator leaves: `rem_unreachable`'s transition
        /// filter and `determineDfaD`'s rebuild both have to cope with rows that are
        /// already sparse going in, not just rows sparsified by dropping non-co-reachable
        /// heads. Also untrimmed, for the same reason as its sibling: WB-001 is fixed.
        #[test]
        fn minimize_preserves_language_on_partial_dfa(fa in arb_partial_dfa(6, 3)) {
            let min = minimize(&fa).unwrap();
            prop_assert_eq!(
                language_equivalent(&totalized(&fa), &totalized(&min)),
                Ok(true)
            );
        }
    }
}
