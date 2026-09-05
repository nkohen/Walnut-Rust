// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! On-the-fly reduced subset construction — [`crate::determinize::Strategy::ScOtf`],
//! the port's answer to "tame a *transient* determinization explosion" without
//! porting Walnut's deferred `io.github.jn1z:otf` family (`docs/OTF-DETERMINIZATION-SIZING.md`).
//!
//! **No Java counterpart; opt-in; never runs unless selected** (a `[strategy N SC_OTF]`
//! metacommand, or a scope-level default via
//! [`crate::resource::Instrumentation::with_default_strategy`]). Plain `SC` stays
//! bit-identical. Reached only through [`crate::determinize::determinize`]'s dispatcher —
//! every `eval`/`def` construction path — and therefore NOT by [`crate::regex`]'s `reg`
//! pipeline, which calls `subset_construction` directly.
//!
//! # What "transient" means, and what this does about it
//!
//! Subset construction hash-conses metastates by their NFA state *set*. A transient
//! explosion is many distinct sets with the *same language* — the minimizer that runs
//! afterwards collapses them, but only after they have all been materialized. This
//! strategy canonicalizes sets on the fly using the NFA's **forward simulation
//! preorder** (`q` simulates `p` ⇒ `L(p) ⊆ L(q)`): every destination set is reduced to
//! its simulation-maximal members before it is looked up, so sets that differ only by
//! simulated (language-redundant) states collapse to one metastate immediately. It is
//! the idea behind Walnut's `CCLS` ("S" = simulation) and the antichain literature
//! (De Wulf–Doyen–Henzinger–Raskin), in its simplest form.
//!
//! **Sound by construction.** `L(S) = ⋃ L(s)`; replacing a member by a mutually-similar
//! state (same language) and dropping `p` when some other member simulates it both leave
//! that union unchanged. The reduced set's acceptance is unchanged too (the preorder's
//! seed requires `acc(p) ⇒ acc(q)`). Since `a⁻¹L(S)` depends only on `L(S)`, every state
//! the reduced construction reaches on a word `w` has exactly the language the plain
//! construction's state has, so the two DFAs are language-equivalent — pinned by the
//! property tests below through the frozen `crate::equiv` oracle.
//!
//! **Canonical, so never larger than plain `SC`.** The reduction of a set is its
//! antichain of ⊑-maximal *similarity classes*, each class written as its smallest state
//! id — a function of the set's downward closure `cl(S) = {p : p ⊑ s for some s ∈ S}`,
//! and `cl(δ(S, a)) = cl(δ(cl(S), a))` for a simulation preorder, so the reduced
//! construction's states are in bijection with the images of the plain construction's
//! states under `cl`. Mapping to class representatives first is load-bearing: this
//! module's first draft dropped only strictly-subsumed members and kept "the smaller id"
//! of a mutually-similar pair, and its own property test caught the resulting growth —
//! `{p, q}` with `p ≡ q` reduced to `{p}` on one path while a reduced predecessor's
//! successor `{q}` stayed `{q}` on another, two states for one language.
//!
//! **Not complete.** Two sets can be language-equivalent without simulation explaining
//! it; those still materialize separately and are left to the final minimization. The
//! output is at most as large as plain `SC`'s and not necessarily minimal — callers
//! always minimize afterwards exactly as for `SC`.
//!
//! # The approach that was NOT taken, and why
//!
//! "Minimize the frontier during subset construction" in the literal sense — pause the
//! BFS every so often and run a partition refinement over the already-expanded states,
//! treating each unexpanded frontier state as an opaque singleton block — is sound, but
//! provably finds nothing on a BFS frontier: a block can only merge states whose
//! successors are in equal blocks, a frontier singleton equals nothing but itself, and
//! that inequality propagates backwards along every path that leads to the frontier.
//! In a BFS every explored state that is not inside a *closed* region (one from which
//! the frontier is unreachable) lies on such a path, so the pass splits everything back
//! apart. It was prototyped on paper against the transient fixtures below and rejected;
//! the simulation-based reduction is the version that actually collapses them.
//!
//! # Cost, and the size guards
//!
//! The preorder is computed once per determinization by the textbook refinement — a
//! bit matrix of `q²` pairs, iterated to a fixpoint. One round costs, for every related
//! pair `(p, q)`, a walk over `p`'s **present** symbols (a sparse successor table; the
//! first draft iterated `0..alphabet_size` for every pair, which an adversarial review
//! measured at 180 s for 800 states over a 1024-symbol alphabet — Walnut's multi-track
//! `RichAlphabet`s are routinely that wide — and that is now a merge over the symbols
//! that actually occur), so a round is `O(q · m)` for `m` effective `(state, symbol)`
//! transition entries, and there are at most `q²` rounds in theory and a handful in
//! practice. Two guards in [`OtfPolicy`] keep this off the critical path: the preorder is
//! not computed when `q > max_nfa_states` **or** when `q · m > max_preorder_work`; in
//! either case the strategy degrades to plain sequential `SC` (bit-identical to it) and
//! the observer is told ([`crate::resource::Event::SimulationSkipped`]). The preorder's
//! memory (`q²` bits plus the sparse table) is allocated in one piece and the resource
//! budget's memory cap is checked right after it is built. The construction itself runs
//! sequentially — the level-parallel machinery of [`crate::determinize`] is not used
//! here (the reduction would have to move into the workers; a straightforward follow-up
//! if the strategy earns it).

use std::collections::{BTreeSet, HashMap};

use crate::determinize::{expand_metastate, merge_expansion, ExpandOut, ExpandScratch};
use crate::fa::Fa;
use crate::resource::{Event, Meter, Operation};

/// Tunables for [`subset_construction_otf`]. Installed for a scope through
/// [`crate::resource::Instrumentation::with_otf_policy`]; the default applies otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OtfPolicy {
    /// Largest NFA (state count) for which the simulation preorder is computed. Above
    /// this the strategy is plain sequential subset construction. See the module docs
    /// for the cost model behind the default.
    pub max_nfa_states: usize,
    /// Largest `q · m` (states times effective transition entries — one refinement
    /// round's cost) for which the preorder is computed; the alphabet-width guard the
    /// state-count guard alone cannot provide. Default `2^26`.
    pub max_preorder_work: usize,
}

impl Default for OtfPolicy {
    fn default() -> Self {
        OtfPolicy {
            max_nfa_states: 4096,
            max_preorder_work: 1 << 26,
        }
    }
}

impl OtfPolicy {
    /// Whether the guards admit an NFA of `q` states and `m` effective transition entries.
    pub fn admits(&self, q: usize, m: usize) -> bool {
        q <= self.max_nfa_states && q.saturating_mul(m) <= self.max_preorder_work
    }
}

/// The effective `(state, symbol)` transition entries of `fa` — in-range symbols with a
/// non-empty destination list — i.e. the `m` of the cost model.
pub fn effective_transitions(fa: &Fa) -> usize {
    fa.d.iter()
        .take(fa.q)
        .map(|row| {
            row.iter()
                .filter(|(&sym, dests)| {
                    sym >= 0 && (sym as usize) < fa.alphabet_size && !dests.is_empty()
                })
                .count()
        })
        .sum()
}

/// The forward simulation preorder of an NFA, as a bit matrix: `simulates(q, p)` holds
/// iff `q` simulates `p`, which guarantees `L(p) ⊆ L(q)`.
///
/// Computed over the same effective NFA subset construction reads — a transition whose
/// symbol is outside `0..alphabet_size` is ignored, exactly as `expand_metastate` ignores
/// it (WB-038 outcome (b)) — so the languages the preorder speaks about are the ones the
/// construction computes.
pub struct Simulation {
    n: usize,
    words: usize,
    /// Row `p`, bit `q`: `q` simulates `p`.
    bits: Vec<u64>,
    /// `rep[p]`: the smallest state mutually similar to `p` (its similarity class's
    /// canonical representative; `rep[p] == p` for a class's smallest member).
    rep: Vec<usize>,
}

impl Simulation {
    /// The greatest simulation relation of `fa`, by fixpoint refinement from
    /// `{(p, q) : acc(p) ⇒ acc(q)}`.
    ///
    /// # Panics
    ///
    /// On a destination id `>= fa.q` (a malformed `Fa`; plain subset construction
    /// panics on the same input, at a later point).
    pub fn compute(fa: &Fa) -> Simulation {
        let n = fa.q;
        let words = n.div_ceil(64).max(1);
        let mut sim = Simulation {
            n,
            words,
            bits: vec![0u64; n * words],
            rep: (0..n).collect(),
        };
        // Effective successor lists, SPARSE: per state, the in-range symbols that occur
        // (ascending, as `BTreeMap` yields them) with their deduplicated destinations.
        let mut succ: Vec<Vec<(i32, Vec<usize>)>> = vec![Vec::new(); n];
        for (p, row) in fa.d.iter().enumerate().take(n) {
            for (&sym, dests) in row {
                if sym < 0 || sym as usize >= fa.alphabet_size || dests.is_empty() {
                    continue;
                }
                let mut bucket: Vec<usize> = Vec::with_capacity(dests.len());
                for &dest in dests {
                    assert!(
                        dest < n,
                        "subset_construction_otf: destination id {dest} out of range for {n} states"
                    );
                    bucket.push(dest);
                }
                bucket.sort_unstable();
                bucket.dedup();
                succ[p].push((sym, bucket));
            }
        }
        for p in 0..n {
            for q in 0..n {
                if !(fa.is_accepting(p) && !fa.is_accepting(q)) {
                    sim.set(p, q);
                }
            }
        }
        loop {
            let mut changed = false;
            for p in 0..n {
                for q in 0..n {
                    if p == q || !sim.simulates(q, p) {
                        continue;
                    }
                    // For every symbol `p` can read, `q` must read it too and every
                    // `p`-successor must be simulated by some `q`-successor. Symbols `p`
                    // cannot read impose nothing. Both lists are sorted by symbol, so this
                    // is a merge, not an alphabet scan.
                    let qs = &succ[q];
                    let keep = succ[p].iter().all(|(a, p_succ)| {
                        match qs.binary_search_by_key(a, |(b, _)| *b) {
                            Err(_) => false,
                            Ok(i) => p_succ
                                .iter()
                                .all(|&p2| qs[i].1.iter().any(|&q2| sim.simulates(q2, p2))),
                        }
                    });
                    if !keep {
                        sim.clear(p, q);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for p in 0..n {
            // The smallest `q` with `q ≡ p`; scanning upward, the first hit is it.
            sim.rep[p] = (0..=p)
                .find(|&q| sim.simulates(q, p) && sim.simulates(p, q))
                .unwrap_or(p);
        }
        sim
    }

    fn set(&mut self, p: usize, q: usize) {
        self.bits[p * self.words + q / 64] |= 1u64 << (q % 64);
    }

    fn clear(&mut self, p: usize, q: usize) {
        self.bits[p * self.words + q / 64] &= !(1u64 << (q % 64));
    }

    /// Whether `q` simulates `p` (so `L(p) ⊆ L(q)`).
    #[inline]
    pub fn simulates(&self, q: usize, p: usize) -> bool {
        (self.bits[p * self.words + q / 64] >> (q % 64)) & 1 == 1
    }

    /// Number of NFA states the preorder was computed over.
    pub fn states(&self) -> usize {
        self.n
    }

    /// Number of ordered pairs `(p, q)` with `q` simulating `p`, the diagonal included.
    pub fn related_pairs(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// The canonical representative of `p`'s similarity class (its smallest member).
    #[inline]
    pub fn representative(&self, p: usize) -> usize {
        self.rep[p]
    }

    /// Reduce a state set to its canonical form, in place: every member is replaced by
    /// its similarity-class representative, duplicates are dropped, and then every
    /// member some *other* member simulates is dropped (distinct representatives are
    /// never mutually similar, so that is a strict subsumption). The result is sorted,
    /// duplicate-free, never empty for a non-empty input, and — the property the
    /// construction relies on — a function of the input's downward closure.
    pub fn reduce(&self, set: &mut Vec<usize>) {
        for p in set.iter_mut() {
            *p = self.rep[*p];
        }
        set.sort_unstable();
        set.dedup();
        if set.len() < 2 {
            return;
        }
        let mut write = 0;
        for i in 0..set.len() {
            let p = set[i];
            let subsumed = set
                .iter()
                .enumerate()
                .any(|(j, &q)| j != i && self.simulates(q, p));
            if !subsumed {
                set[write] = p;
                write += 1;
            }
        }
        set.truncate(write);
        debug_assert!(!set.is_empty(), "a maximal element always survives");
    }
}

/// Reduce every destination-set key an expansion produced, compacting `out` in place.
fn reduce_expand_out(out: &mut ExpandOut, sim: &Simulation, scratch: &mut Vec<usize>) {
    let mut read = 0usize;
    let mut write = 0usize;
    for span in out.spans.iter_mut() {
        let len = span.1 as usize;
        scratch.clear();
        scratch.extend_from_slice(&out.flat[read..read + len]);
        read += len;
        sim.reduce(scratch);
        out.flat[write..write + scratch.len()].copy_from_slice(scratch);
        write += scratch.len();
        // Fits: the reduced key is never longer than the original, which fit in `u32`.
        span.1 = scratch.len() as u32;
    }
    out.flat.truncate(write);
}

/// Subset construction with on-the-fly simulation-subsumption reduction of every
/// metastate. Same contract as [`crate::determinize::subset_construction`] (starts from
/// the metastate `initial`, does not totalize, `q0 = 0`), with an output that is
/// language-equivalent to it and at most as large.
///
/// With `policy.max_nfa_states < fa.q` no preorder is computed and the result is
/// **bit-identical** to sequential plain subset construction.
pub fn subset_construction_otf(fa: &Fa, initial: &BTreeSet<usize>, policy: &OtfPolicy) -> Fa {
    let meter = Meter::current();
    meter.emit(|| Event::SubsetConstructionStarted {
        input_states: fa.q,
        initial_size: initial.len(),
    });
    let transitions = effective_transitions(fa);
    let sim = if policy.admits(fa.q, transitions) {
        let sim = Simulation::compute(fa);
        // The preorder's whole allocation is live now: one memory-cap look before the
        // construction starts (`crate::resource`'s docs state this exactly).
        meter.budget().check_memory(Operation::SubsetConstruction);
        meter.emit(|| Event::SimulationComputed {
            nfa_states: sim.states(),
            related_pairs: sim.related_pairs(),
        });
        Some(sim)
    } else {
        meter.emit(|| Event::SimulationSkipped {
            nfa_states: fa.q,
            transitions,
            policy: *policy,
        });
        None
    };

    let mut first: Vec<usize> = initial.iter().copied().collect();
    if let Some(sim) = &sim {
        sim.reduce(&mut first);
    }
    let mut metastate_to_id: HashMap<Vec<usize>, usize> = HashMap::new();
    metastate_to_id.insert(first.clone(), 0);
    meter.check(Operation::SubsetConstruction, 1);
    let mut metastate_list: Vec<Vec<usize>> = vec![first];
    let mut d = Vec::new();
    let mut scratch = ExpandScratch::new(fa);
    let mut out = ExpandOut::default();
    let mut key_scratch: Vec<usize> = Vec::new();
    let mut cursor = 0usize;
    let mut levels = 0usize;

    loop {
        let end = metastate_list.len();
        if cursor >= end {
            break;
        }
        meter.emit(|| Event::SubsetLevel {
            level: levels,
            frontier: end - cursor,
            members: metastate_list[cursor..end].iter().map(Vec::len).sum(),
            metastates: end,
        });
        levels += 1;
        for i in cursor..end {
            let current = metastate_list[i].clone();
            out.clear();
            expand_metastate(fa, &current, &mut scratch, &mut out);
            if let Some(sim) = &sim {
                reduce_expand_out(&mut out, sim, &mut key_scratch);
            }
            merge_expansion(
                &out,
                &mut metastate_list,
                &mut metastate_to_id,
                &mut d,
                &meter,
            );
        }
        cursor = end;
    }

    let o = metastate_list
        .iter()
        .map(|ms| i32::from(ms.iter().any(|&q| fa.is_accepting(q))))
        .collect();
    meter.emit(|| Event::SubsetConstructionFinished {
        states: metastate_list.len(),
        levels,
    });
    Fa::with_states(0, metastate_list.len(), fa.alphabet_size, o, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::determinize::subset_construction;
    use crate::equiv::language_equivalent;
    use crate::minimize::minimize;
    use std::collections::BTreeMap;

    fn row(pairs: &[(i32, &[usize])]) -> BTreeMap<i32, Vec<usize>> {
        pairs.iter().map(|&(s, d)| (s, d.to_vec())).collect()
    }

    fn totalized(fa: &Fa) -> Fa {
        let mut t = fa.clone();
        t.totalize(0);
        t
    }

    fn from_zero() -> BTreeSet<usize> {
        [0].into_iter().collect()
    }

    /// Σ* recognized wastefully: state 0 accepts and loops on everything, and on `1`
    /// also fans into a chain `c1 → … → ck` of accepting states. Every reachable set is
    /// `{0} ∪ C` for some `C ⊆ chain`, so plain subset construction builds 2^k
    /// metastates that all minimize to ONE — the transient explosion in its purest
    /// form. State 0 simulates every chain state, so the reduction collapses every set
    /// to `{0}`.
    fn sigma_star_via_chain(k: usize) -> Fa {
        let n = k + 1;
        let mut d = Vec::with_capacity(n);
        d.push(row(&[(0, &[0]), (1, &[0, 1])]));
        for i in 1..k {
            d.push(row(&[(0, &[i + 1]), (1, &[i + 1])]));
        }
        d.push(row(&[]));
        Fa::with_states(0, n, 2, vec![1; n], d)
    }

    /// "The k-th symbol from the end is 1": 2^k metastates, ALL of them necessary
    /// (the minimal DFA has 2^k states). No chain state is simulated by anything, so
    /// the reduction must change nothing here.
    fn kth_from_end(k: usize) -> Fa {
        let mut d = Vec::new();
        d.push(row(&[(0, &[0]), (1, &[0, 1])]));
        for i in 1..k {
            d.push(row(&[(0, &[i + 1]), (1, &[i + 1])]));
        }
        d.push(row(&[]));
        let mut o = vec![0; k + 1];
        o[k] = 1;
        Fa::with_states(0, k + 1, 2, o, d)
    }

    #[test]
    fn a_transient_explosion_collapses_on_the_fly() {
        let fa = sigma_star_via_chain(6);
        let plain = subset_construction(&fa, &from_zero());
        assert_eq!(plain.q, 64, "plain SC materializes every subset");
        let otf = subset_construction_otf(&fa, &from_zero(), &OtfPolicy::default());
        assert_eq!(otf.q, 1, "every set reduces to {{0}}");
        assert!(language_equivalent(&totalized(&plain), &totalized(&otf)).unwrap());
    }

    #[test]
    fn a_real_explosion_is_left_alone() {
        let fa = kth_from_end(5);
        let plain = subset_construction(&fa, &from_zero());
        let otf = subset_construction_otf(&fa, &from_zero(), &OtfPolicy::default());
        assert_eq!(plain.q, 32);
        assert_eq!(otf.q, 32);
        assert!(language_equivalent(&totalized(&plain), &totalized(&otf)).unwrap());
        assert_eq!(minimize(&otf).unwrap().q, 32);
    }

    #[test]
    fn above_the_size_guard_it_is_plain_sequential_subset_construction() {
        let fa = sigma_star_via_chain(4);
        let plain = subset_construction(&fa, &from_zero());
        let policy = OtfPolicy {
            max_nfa_states: 0,
            ..OtfPolicy::default()
        };
        let otf = subset_construction_otf(&fa, &from_zero(), &policy);
        assert_eq!(otf.q, plain.q);
        assert_eq!(otf.o, plain.o);
        assert_eq!(otf.d, plain.d);
        // The work guard degrades the same way: this NFA has 5 states and 8 effective
        // entries (the chain's end has none), so a work cap of 39 refuses it and 40
        // admits it.
        assert_eq!(effective_transitions(&fa), 8);
        let tight = OtfPolicy {
            max_preorder_work: 39,
            ..OtfPolicy::default()
        };
        assert!(!tight.admits(5, 8));
        let otf = subset_construction_otf(&fa, &from_zero(), &tight);
        assert_eq!(otf.q, plain.q);
        let loose = OtfPolicy {
            max_preorder_work: 40,
            ..OtfPolicy::default()
        };
        assert!(loose.admits(5, 8));
        assert_eq!(subset_construction_otf(&fa, &from_zero(), &loose).q, 1);
    }

    /// The review's wide-alphabet case: the preorder must not scale with the alphabet
    /// width, only with the transitions that occur. 400 chain states over a 1024-symbol
    /// alphabet, each state using two symbols: sparse work `q·m = 400·800`, admitted by
    /// the default policy, and fast.
    #[test]
    fn a_wide_alphabet_costs_only_its_present_symbols() {
        let q = 400;
        let mut d = Vec::with_capacity(q);
        for i in 0..q {
            let next = (i + 1) % q;
            d.push(row(&[(0, &[next]), (1000, &[next, 0])]));
        }
        let fa = Fa::with_states(
            0,
            q,
            1024,
            (0..q).map(|i| i32::from(i % 7 == 0)).collect(),
            d,
        );
        assert_eq!(effective_transitions(&fa), 2 * q);
        assert!(OtfPolicy::default().admits(q, 2 * q));
        let started = std::time::Instant::now();
        let sim = Simulation::compute(&fa);
        assert!(sim.related_pairs() >= q);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn the_preorder_is_reflexive_and_seeded_by_acceptance() {
        let fa = kth_from_end(3);
        let sim = Simulation::compute(&fa);
        for p in 0..fa.q {
            assert!(sim.simulates(p, p));
        }
        // The accepting dead-end state 3 is simulated by nothing else (nothing else
        // accepts the empty word), and it simulates nothing with a future.
        for p in 0..3 {
            assert!(!sim.simulates(p, 3));
            assert!(!sim.simulates(3, p));
        }
        assert!(sim.related_pairs() >= fa.q);
    }

    #[test]
    fn reduce_maps_a_mutually_similar_class_to_its_smallest_member() {
        // Two copies of the same accepting sink: mutually similar.
        let fa = Fa::with_states(
            0,
            3,
            1,
            vec![0, 1, 1],
            vec![row(&[(0, &[1, 2])]), row(&[(0, &[1])]), row(&[(0, &[2])])],
        );
        let sim = Simulation::compute(&fa);
        assert!(sim.simulates(1, 2) && sim.simulates(2, 1));
        let mut set = vec![1, 2];
        sim.reduce(&mut set);
        assert_eq!(set, vec![1]);
        assert_eq!(sim.representative(2), 1);
        let mut set = vec![0, 1, 2];
        sim.reduce(&mut set);
        // 1 simulates 0 (0 is non-accepting, so the seed admits it, and 0's successors
        // {1,2} are each simulated by 1's successor 1) while 0 does not simulate 1 (1
        // accepts ε, 0 does not): 0 is strictly subsumed.
        assert_eq!(set, vec![1]);
        // The growth case the first draft got wrong: `{2}` alone must canonicalize to
        // the same set `{1, 2}` does.
        let mut lone = vec![2];
        sim.reduce(&mut lone);
        assert_eq!(lone, vec![1]);
    }

    /// Deterministic generator of small NFAs — the same LCG shape `determinize.rs`'s
    /// tests use, kept local so this module is self-contained.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> usize {
            (self.next() % n) as usize
        }
    }

    fn random_nfa(rng: &mut Rng) -> Fa {
        let q = 1 + rng.below(7);
        let alphabet = 1 + rng.below(3);
        let mut d = Vec::with_capacity(q);
        for _ in 0..q {
            let mut r = BTreeMap::new();
            for a in 0..alphabet as i32 {
                let k = rng.below(3);
                let mut dests: Vec<usize> = (0..k).map(|_| rng.below(q as u64)).collect();
                dests.sort_unstable();
                dests.dedup();
                if !dests.is_empty() {
                    r.insert(a, dests);
                }
            }
            d.push(r);
        }
        let o = (0..q).map(|_| rng.below(2) as i32).collect();
        Fa::with_states(rng.below(q as u64), q, alphabet, o, d)
    }

    #[test]
    fn reduced_and_plain_constructions_are_language_equivalent_on_random_nfas() {
        let mut rng = Rng(0x5eed_0f7f);
        let mut reduced_somewhere = false;
        for _ in 0..3000 {
            let fa = random_nfa(&mut rng);
            let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
            let plain = subset_construction(&fa, &initial);
            let otf = subset_construction_otf(&fa, &initial, &OtfPolicy::default());
            assert!(otf.q <= plain.q, "reduction never grows the construction");
            reduced_somewhere |= otf.q < plain.q;
            assert!(
                language_equivalent(&totalized(&plain), &totalized(&otf)).unwrap(),
                "language differs on {fa:?}"
            );
            assert!(otf.is_deterministic());
        }
        assert!(
            reduced_somewhere,
            "the generator must exercise an actual reduction"
        );
    }

    #[test]
    fn simulation_implies_language_inclusion_on_random_nfas() {
        // `q` simulates `p`  ⇒  L(p) ⊆ L(q), checked exactly through the oracle:
        // L(p) ∩ ¬L(q) = ∅.
        let mut rng = Rng(0xabcd_1234);
        let mut checked = 0usize;
        for _ in 0..1500 {
            let fa = random_nfa(&mut rng);
            let sim = Simulation::compute(&fa);
            for p in 0..fa.q {
                for q in 0..fa.q {
                    if p == q || !sim.simulates(q, p) {
                        continue;
                    }
                    let lp = totalized(&subset_construction(&fa, &[p].into_iter().collect()));
                    let lq = totalized(&subset_construction(&fa, &[q].into_iter().collect()));
                    let diff = crate::equiv::product_dfa(&lp, &lq, |x, y| x && !y).unwrap();
                    let empty = Fa::with_states(0, 1, fa.alphabet_size, vec![0], {
                        let mut r = BTreeMap::new();
                        for a in 0..fa.alphabet_size as i32 {
                            r.insert(a, vec![0]);
                        }
                        vec![r]
                    });
                    assert!(
                        language_equivalent(&diff, &empty).unwrap(),
                        "{q} simulates {p} but L({p}) ⊄ L({q}) in {fa:?}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "only {checked} non-trivial pairs checked");
    }
}
