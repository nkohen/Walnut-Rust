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
//! # Preconditions (both are hard errors / documented, never `debug_assert!`)
//!
//! 1. **Deterministic input.** Java's `FA.justMinimize` calls `convertNFAtoDFA()` first,
//!    which is a *storage* conversion that throws `"Unexpected NFA instead of DFA."` on
//!    genuine nondeterminism — it does not subset-construct. The algorithm itself is
//!    unsound on an NFA: [`Partition::mark`] has no double-mark guard, and two
//!    transitions sharing a tail *and* a label would land in the same cord and mark the
//!    same block element twice, corrupting the partition. Mirrored here as
//!    [`MinimizeError::NotDeterministic`].
//! 2. **Every state should be reachable from `q0`.** See the "q0 aliasing quirk" section
//!    below — this is a genuine, faithfully-ported Walnut behavior, not a port artifact.
//!    Callers should run [`crate::trim::trim`] first if they cannot guarantee it.
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
//! # The q0 aliasing quirk (ported verbatim)
//!
//! States found non-co-reachable are left parked at positions `>= rr` in the element
//! array, outside every block's `[F, P)` range, yet their set-id `S[q]` is still `0` from
//! `init`. `replaceFields` then computes the new start state as `blocks.S[q0]`. So if
//! `q0` itself cannot reach an accepting state *while some accepting state exists*, the
//! result's start state silently becomes block `0` — which after the initial
//! accepting/non-accepting split is not necessarily the dead block, and can even be an
//! accepting one. Concretely: `q0` self-looping and non-accepting, plus a disjoint
//! accepting self-loop, minimizes to a 1-state *accepting* automaton (language `∅`
//! becomes `Σ*`). This cannot happen when every state is reachable from `q0`: a final
//! state then exists only if `q0` can reach it. Pinned by
//! `minimize_q0_not_co_reachable_walnut_quirk`. Cataloged as `docs/WALNUT-BUGS.md` WB-001
//! (upstream fix status, severity, full verification history — this doc comment covers
//! only the Rust-port-relevant summary).
//!
//! # Scope note
//!
//! Like the Java, outputs are rebuilt as plain accept/reject bits, so DFAO word-output
//! values (`o > 1`) do not survive minimization. Matches this crate's current
//! predicate-automaton scope (see the `equiv` module docs).

use crate::fa::Fa;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
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

    Ok(Fa {
        true_false: None,
        q0: new_q0,
        q: new_q,
        alphabet_size: fa.alphabet_size,
        o,
        d,
    })
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
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 1],
            d: vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 1), (1, 1)]),
                row(&[(0, 2), (1, 2)]),
            ],
        };
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
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2, "no false merge on an already-minimal DFA");
        assert_eq!(
            language_equivalent(&totalized(&fa), &totalized(&min)),
            Ok(true)
        );
    }

    #[test]
    fn rejects_nondeterministic_input() {
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 1)])],
        };
        fa.d[0].insert(1, vec![0, 1]); // two destinations for symbol 1
        assert_eq!(minimize(&fa).unwrap_err(), MinimizeError::NotDeterministic);
    }

    #[test]
    fn no_accepting_states_collapses_to_one_dead_state() {
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 0, 0],
            d: vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 2), (1, 0)]),
                row(&[(0, 0), (1, 1)]),
            ],
        };
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
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 0],
            d: vec![
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 1), (1, 2)]),
                row(&[(0, 2), (1, 2)]),
            ],
        };
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 2);
        assert!(min.accepts_word(&[0, 0]));
        assert!(!min.accepts_word(&[0, 1]));
        assert_eq!(
            language_equivalent(&totalized(&fa), &totalized(&min)),
            Ok(true)
        );
    }

    #[test]
    fn minimize_q0_not_co_reachable_walnut_quirk() {
        // q0 self-loops and is non-accepting; state 1 is a disjoint accepting self-loop
        // that q0 can never reach. Walnut's `replaceFields` maps the new start state to
        // `blocks.S[q0]`, and non-co-reachable states keep the `S = 0` they got from
        // `init` — so q0 aliases onto block 0, which here is the accepting block. The
        // language flips from ∅ to Σ*. Ported verbatim (mechanical-port discipline);
        // pinned here so the deviation is visible rather than latent.
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![row(&[(0, 0)]), row(&[(0, 1)])],
        };
        assert!(fa.is_language_empty());
        let min = minimize(&fa).unwrap();
        assert_eq!(min.q, 1);
        assert!(
            min.is_accepting(min.q0),
            "documents Walnut's q0-aliasing quirk, see module docs"
        );

        // Trimming first restores the expected precondition and the correct answer.
        let min_trimmed = minimize(&trim(&fa)).unwrap();
        assert!(min_trimmed.is_language_empty());
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
                Fa {
                    true_false: None,
                    q0: 0,
                    q,
                    alphabet_size,
                    o,
                    d,
                }
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
                Fa {
                    true_false: None,
                    q0: 0,
                    q,
                    alphabet_size,
                    o,
                    d,
                }
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
        /// partial DFA).
        ///
        /// `trim` is applied first because the generator freely produces automata whose
        /// q0 cannot reach acceptance, which is outside minimize's documented
        /// precondition (see the q0-aliasing quirk in the module docs). `trim` is itself
        /// property-tested as language-preserving, so this remains an end-to-end check
        /// of minimize against the *original* automaton.
        #[test]
        fn minimize_preserves_language(fa in arb_total_dfa(6, 3)) {
            let min = minimize(&trim(&fa)).unwrap();
            prop_assert_eq!(
                language_equivalent(&totalized(&fa), &totalized(&min)),
                Ok(true)
            );
        }

        /// Tier-4 property #4 (DESIGN.md §5): minimize is idempotent — a second pass
        /// finds nothing further to merge, and does not disturb the language. No `trim`
        /// is needed on the second pass: every state of a minimize output is
        /// co-reachable by construction, so the q0-aliasing quirk cannot trigger there.
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
        /// heads.
        #[test]
        fn minimize_preserves_language_on_partial_dfa(fa in arb_partial_dfa(6, 3)) {
            let min = minimize(&trim(&fa)).unwrap();
            prop_assert_eq!(
                language_equivalent(&totalized(&fa), &totalized(&min)),
                Ok(true)
            );
        }
    }
}
