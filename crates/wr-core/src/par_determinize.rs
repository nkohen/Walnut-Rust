// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **EXPERIMENT — the `agent/par-posthoc` lane.** A subset construction that computes
//! *racily* (metastate ids minted by whichever thread wins a lock, so the numbering is
//! genuinely nondeterministic run to run) and then reconstructs, POST HOC, the exact
//! state numbering the sequential [`crate::determinize::subset_construction`] would have
//! produced — by calling production [`Fa::canonicalize`] on the result.
//!
//! This module is **not** part of the ported Walnut surface. It adds no new behavior:
//! [`subset_construction_par`] is contracted to return a `Fa` that is **field-for-field
//! identical** to `subset_construction`'s, and is proven so by
//! [`par_matches_the_sequential_implementation`] over generated automata. Nothing
//! downstream can observe that it ran.
//!
//! # The Canonical Recovery Lemma — why post-hoc reconstruction works here
//!
//! Two facts, together, are the whole thesis of this lane for `determinize`:
//!
//! **(L1) `canonicalize` is a permutation-invariant.** For an `Fa` `A` and any bijection
//! `π` on its states, write `π(A)` for the relabelled automaton (`π(A).d[π(q)][s] =
//! [π(x) for x in A.d[q][s]]`, `π(A).o[π(q)] = A.o[q]`, `π(A).q0 = π(A.q0)`). Then
//! `canonicalize(π(A)) == canonicalize(A)`, field for field.
//!
//! *Proof.* [`Fa::determine_permutation_map`] is a BFS from `q0` that pops states FIFO
//! and, at each popped state, walks `d[q].values()` — i.e. the state's symbols in
//! ascending `BTreeMap` key order, and within a symbol, the destination list in its
//! stored order. Relabelling preserves both orders exactly (the key set of `d[π(q)]` is
//! the key set of `d[q]`; the destination list is mapped element-wise, so its length and
//! position-by-position correspondence are preserved). So the BFS on `π(A)` visits
//! `π(q)` exactly when the BFS on `A` visits `q`, in the same step. Hence it assigns
//! `π(q)` the same new id it assigns `q`, and the rebuilt `o`/`d`/`q0` — which are read
//! only through that map — coincide. The reachability pruning and the empty-destination-
//! list pruning are likewise defined purely in terms of the BFS and the list contents,
//! both preserved. ∎
//!
//! **(L2) `canonicalize` is the identity on any `subset_construction` output.** The
//! sequential construction mints ids in exactly BFS-from-`q0`, ascending-symbol order:
//! its worklist cursor walks metastates in id order (and ids are dense and minted in
//! increasing order, so id order *is* FIFO discovery order), and for each metastate it
//! walks `0..alphabet_size` ascending, minting a new id the first time a union key is
//! seen. That is, symbol for symbol, the order `determine_permutation_map` walks. Every
//! metastate is reachable from metastate 0 by construction, so nothing is pruned, and
//! `subset_construction` never records an empty destination list. So the permutation map
//! it computes is the identity, and `canonicalize` rewrites nothing. ∎
//!
//! **Corollary.** For *any* numbering `π` a parallel engine happens to produce,
//! `canonicalize(π(SC(fa, init))) == SC(fa, init)`. The parallel phase is therefore free
//! to mint ids in whatever order threads happen to race to — a freedom a
//! *deterministic*-parallelization lane (which must reproduce the sequential order
//! during compute) does not have.
//!
//! Both lemmas are load-bearing and both are tested here rather than asserted:
//! [`canonicalize_is_the_identity_on_subset_construction_output`] pins L2, and
//! [`canonicalize_recovers_the_sequential_numbering_from_any_relabelling`] pins L1 with
//! adversarial permutations (including the reverse permutation, which is the worst case
//! for a BFS renumberer).
//!
//! # What is *not* claimed
//!
//! Only `subset_construction`'s numbering is recovered here. `minimize` (Valmari) is
//! **not** numbering-invariant in the same way, and neither is `product`; see
//! `docs/PAR-POSTHOC.md` for the coverage map. That is why this function canonicalizes
//! internally and hands back the sequential answer, rather than letting a racy numbering
//! escape into the rest of the engine.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Barrier, Mutex, RwLock};

use crate::determinize::subset_construction;
use crate::fa::Fa;

/// Number of independently-locked hash-cons shards. Sized well above the thread count so
/// that two threads interning unrelated metastates rarely contend on the same lock.
const SHARDS: usize = 256;

/// Below this many NFA states the parallel path is not attempted at all — the sequential
/// implementation is called instead.
///
/// Two reasons, one of them a correctness reason and therefore load-bearing:
///
/// 1. *Performance.* Thread spawn plus per-level barriers cost tens of microseconds; the
///    sequential implementation completes a small automaton in less than that.
/// 2. *Fidelity on malformed input.* `subset_construction` is `pub` and `Fa` carries no
///    invariant forbidding a destination id `>= q`, an `alphabet_size` of 0, or a `d`
///    shorter than the seed set. Those shapes *panic*, and the sequential implementation's
///    panic site/message is pinned by existing tests. Delegating small inputs keeps every
///    such shape on the exact sequential path. (Large malformed inputs still panic, just
///    from inside a scoped thread — documented on [`subset_construction_par`].)
const MIN_STATES_FOR_PARALLEL: usize = 64;

/// One thread's reusable scratch, allocated once per run rather than once per level.
struct Bufs {
    /// `buckets[sym]` accumulates symbol `sym`'s raw union for the metastate currently
    /// being processed — the same C1 bucket table `subset_construction` uses.
    buckets: Vec<Vec<usize>>,
    /// Which symbols the current metastate actually filled, so the clear-down is
    /// proportional to what was written rather than to `alphabet_size`.
    touched: Vec<usize>,
    /// C2's dedup marker: `seen[dest] == epoch` means `dest` is already in `scratch`.
    seen: Vec<u64>,
    epoch: u64,
    scratch: Vec<usize>,
}

impl Bufs {
    fn new(alphabet_size: usize, q: usize) -> Bufs {
        Bufs {
            buckets: vec![Vec::new(); alphabet_size],
            touched: Vec::new(),
            seen: vec![0; q],
            epoch: 0,
            scratch: Vec::new(),
        }
    }
}

/// A sharded, lock-per-shard hash-cons table mapping a canonical metastate key to its id.
///
/// Ids come from a single [`AtomicUsize`], so **which** id a metastate gets depends on
/// the order threads reach their shard locks — i.e. on thread scheduling. That is the
/// point: this lane deliberately does not constrain the numbering during computation.
struct Interner {
    shards: Vec<Mutex<HashMap<Vec<usize>, usize>>>,
    next_id: AtomicUsize,
}

impl Interner {
    fn new() -> Interner {
        Interner {
            shards: (0..SHARDS).map(|_| Mutex::new(HashMap::new())).collect(),
            next_id: AtomicUsize::new(0),
        }
    }

    /// FNV-1a over the key's words. Only used to pick a shard, so its only requirements
    /// are determinism for a given key and a reasonable spread; it never influences the
    /// automaton (which is renumbered post hoc regardless).
    fn shard_of(key: &[usize]) -> usize {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &w in key {
            h ^= w as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        (h % SHARDS as u64) as usize
    }

    /// Returns `(id, is_new)`. Exactly one caller ever sees `is_new == true` for a given
    /// key, so exactly one thread enqueues it for processing.
    fn intern(&self, key: &[usize]) -> (usize, bool) {
        let mut shard = self.shards[Self::shard_of(key)].lock().unwrap();
        if let Some(&id) = shard.get(key) {
            return (id, false);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        shard.insert(key.to_vec(), id);
        (id, true)
    }
}

/// What one worker produced for one metastate it processed.
type Processed = (usize, i32, BTreeMap<i32, Vec<usize>>);

/// A metastate that has been assigned an id but not yet processed.
type Pending = (usize, Vec<usize>);

/// Determinizes `fa` via a **parallel** subset construction, then reconstructs the
/// sequential state numbering post hoc.
///
/// Contract: returns exactly what [`subset_construction`] returns, field for field, for
/// every input on which `subset_construction` returns at all. See this module's docs for
/// the proof sketch (L1 + L2) and
/// [`par_matches_the_sequential_implementation`] for the differential test.
///
/// # Panics
///
/// On a malformed `Fa` (a destination id `>= fa.q`, or a `d` shorter than a metastate's
/// members) this panics, as `subset_construction` does — but for inputs at or above
/// [`MIN_STATES_FOR_PARALLEL`] the panic is raised inside a scoped worker thread and
/// resurfaced by `thread::scope`, so the *message* is wrapped rather than identical.
/// Below that threshold the call is delegated and the panic is bit-identical.
pub fn subset_construction_par(fa: &Fa, initial: &BTreeSet<usize>, threads: usize) -> Fa {
    subset_construction_par_with_threshold(fa, initial, threads, min_states())
}

/// The effective delegation threshold: [`MIN_STATES_FOR_PARALLEL`], or whatever
/// `WR_PAR_MIN_STATES` overrides it to.
///
/// The override exists for VERIFICATION, not tuning. At the production threshold only the
/// few genuinely large determinizations in Walnut's corpus take the parallel path, so a
/// golden-corpus run would exercise the racy code on a handful of fixtures and the
/// sequential delegation on the rest — weak evidence for a lane whose whole claim is that
/// racy numbering is unobservable. `WR_PAR_MIN_STATES=1` forces essentially every
/// determinization in the corpus down the parallel path instead, turning the same run into
/// a much stronger test. It is slower, deliberately.
fn min_states() -> usize {
    use std::sync::OnceLock;
    static MIN: OnceLock<usize> = OnceLock::new();
    *MIN.get_or_init(|| {
        std::env::var("WR_PAR_MIN_STATES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map_or(MIN_STATES_FOR_PARALLEL, |v| v.max(1))
    })
}

/// [`subset_construction_par`] with the delegation threshold supplied explicitly.
///
/// Exists so the tests can drive genuinely NONDETERMINISTIC automata down the parallel
/// path. Real NFAs whose determinization stays small enough to sweep in the fast tier are
/// necessarily tiny (a handful of nondeterministic edges over `q` states already makes the
/// reachable metastate count grow like the number of small subsets of `q`, not like `q`),
/// so an NFA that is above [`MIN_STATES_FOR_PARALLEL`] *and* cheap to determinize
/// essentially does not exist. Lowering the threshold instead lets the same worker code be
/// exercised on real nondeterminism, which is the shape where the per-symbol union, the
/// epoch dedup marker and the hash-cons race all actually do something.
pub fn subset_construction_par_with_threshold(
    fa: &Fa,
    initial: &BTreeSet<usize>,
    threads: usize,
    min_states: usize,
) -> Fa {
    if threads <= 1
        || fa.q < min_states
        || fa.alphabet_size == 0
        || !is_safe_to_parallelize(fa, initial)
    {
        let t = std::time::Instant::now();
        let out = subset_construction(fa, initial);
        stats::record_delegated(t.elapsed());
        return out;
    }
    let t = std::time::Instant::now();
    let mut result = build_racy(fa, initial, threads);
    let compute = t.elapsed();
    // The whole thesis, in one call: production canonicalization maps the racy numbering
    // back onto the sequential one (module docs, L1 + L2). This is the "post-hoc
    // reconstruction phase" whose cost the report accounts for separately from the
    // parallel compute win.
    let t = std::time::Instant::now();
    result.canonicalize();
    stats::record_parallel(compute, t.elapsed());
    result
}

/// Whether `fa` is well-formed enough that no worker can panic — and therefore whether the
/// parallel path may be entered at all.
///
/// **This is a liveness guard, not a defensive style choice.** `build_racy`'s workers
/// synchronize on a [`Barrier`] sized to the worker count, so a worker that panics never
/// arrives at its next `wait()` and every other worker blocks there forever: an input that
/// makes the SEQUENTIAL implementation panic cleanly would make the parallel one **hang**.
/// CLAUDE.md's superexponential-cost guardrail is explicit that every path must yield a
/// diagnosable verdict and never hang, so the malformed shapes are detected up front and
/// routed to the sequential implementation, which panics at exactly the site and with
/// exactly the message its existing tests pin (`subset_construction_panics_on_a_
/// destination_id_out_of_range_of_fa_q`,
/// `subset_construction_with_zero_alphabet_size_and_an_out_of_bounds_initial_member_panics`).
///
/// The two panic sources inside [`process_metastate`] are `fa.d[q]` and `fa.is_accepting(q)`,
/// so the bound is `min(d.len(), o.len())` and the ids that must respect it are the seed
/// members plus every destination in every row.
///
/// Deliberately CONSERVATIVE: it scans every row, including ones no metastate ever reaches,
/// so it can route to the sequential implementation for an automaton the parallel path would
/// in fact have survived. That costs performance on a malformed input, never correctness —
/// and a well-formed `Fa`, which is every automaton this engine actually builds, always
/// passes.
fn is_safe_to_parallelize(fa: &Fa, initial: &BTreeSet<usize>) -> bool {
    let bound = fa.d.len().min(fa.o.len());
    if initial.iter().any(|&s| s >= bound) {
        return false;
    }
    fa.d.iter()
        .all(|row| row.values().all(|dests| dests.iter().all(|&x| x < bound)))
}

/// Phase A on its own: the racy parallel computation, returning an `Fa` whose state
/// numbering depends on thread scheduling.
fn build_racy(fa: &Fa, initial: &BTreeSet<usize>, threads: usize) -> Fa {
    let interner = Interner::new();
    let seed: Vec<usize> = initial.iter().copied().collect();
    // The seed is interned single-threaded, so it is always id 0 and hence `q0`. Every
    // OTHER id is minted under a race.
    let (seed_id, _) = interner.intern(&seed);
    debug_assert_eq!(seed_id, 0);

    let frontier: RwLock<Vec<Pending>> = RwLock::new(vec![(seed_id, seed)]);
    let next_frontier: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
    let processed: Mutex<Vec<Processed>> = Mutex::new(Vec::new());
    let barrier = Barrier::new(threads);

    std::thread::scope(|scope| {
        for t in 0..threads {
            let interner = &interner;
            let frontier = &frontier;
            let next_frontier = &next_frontier;
            let processed = &processed;
            let barrier = &barrier;
            scope.spawn(move || {
                let mut bufs = Bufs::new(fa.alphabet_size, fa.q);
                let mut local_processed: Vec<Processed> = Vec::new();
                let mut local_new: Vec<Pending> = Vec::new();
                loop {
                    {
                        // Every worker holds a READ guard for the whole level; the level's
                        // frontier is immutable while it is being consumed. Discovered
                        // metastates go to `next_frontier`, never to this one.
                        let level = frontier.read().unwrap();
                        if level.is_empty() {
                            break;
                        }
                        // Static block-cyclic split. Which worker gets which metastate
                        // does not matter for the result (the numbering is recovered
                        // afterwards); it only matters for load balance.
                        for (id, members) in level.iter().skip(t).step_by(threads) {
                            let (out, row) =
                                process_metastate(fa, members, &mut bufs, interner, &mut local_new);
                            local_processed.push((*id, out, row));
                        }
                    }
                    // One lock acquisition per worker per level, not per metastate.
                    processed.lock().unwrap().append(&mut local_processed);
                    next_frontier.lock().unwrap().append(&mut local_new);

                    // Everyone has published before the swap, and nobody reads the new
                    // level before the swap is visible: two barriers, one for each edge.
                    barrier.wait();
                    if t == 0 {
                        let mut level = frontier.write().unwrap();
                        level.clear();
                        level.append(&mut next_frontier.lock().unwrap());
                    }
                    barrier.wait();
                }
            });
        }
    });

    // ---- Assembly: id-indexed tables from the workers' unordered output ----------
    let processed = processed.into_inner().unwrap();
    let q = interner.next_id.load(Ordering::Relaxed);
    debug_assert_eq!(
        processed.len(),
        q,
        "every discovered metastate must be processed"
    );

    let mut o = vec![0i32; q];
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); q];
    for (id, out, row) in processed {
        o[id] = out;
        d[id] = row;
    }

    Fa::with_states(seed_id, q, fa.alphabet_size, o, d)
}

/// Computes one metastate's outgoing row and accepting output.
///
/// The union logic is a transliteration of `subset_construction`'s own C1/C2 loop
/// (member-outer bucket fill, then an ascending-symbol drain with an epoch dedup marker),
/// so the *set* of destinations per symbol is identical; only the id each destination
/// metastate is assigned differs, and that is undone in phase B.
fn process_metastate(
    fa: &Fa,
    members: &[usize],
    bufs: &mut Bufs,
    interner: &Interner,
    local_new: &mut Vec<Pending>,
) -> (i32, BTreeMap<i32, Vec<usize>>) {
    for &q in members {
        for (&sym, dests) in &fa.d[q] {
            // Out-of-alphabet keys contribute nothing and are silently dropped, matching
            // the sequential implementation (and Java — WB-038 outcome (b)).
            if sym < 0 || sym as usize >= fa.alphabet_size {
                continue;
            }
            if dests.is_empty() {
                continue;
            }
            let bucket = &mut bufs.buckets[sym as usize];
            if bucket.is_empty() {
                bufs.touched.push(sym as usize);
            }
            bucket.extend(dests.iter().copied());
        }
    }

    let mut row = BTreeMap::new();
    for sym in 0..fa.alphabet_size as i32 {
        let bucket = &bufs.buckets[sym as usize];
        if bucket.is_empty() {
            // Subset construction does not totalize: no entry is recorded at all.
            continue;
        }
        bufs.epoch += 1;
        bufs.scratch.clear();
        for &dest in bucket {
            if dest < fa.q {
                if bufs.seen[dest] == bufs.epoch {
                    continue;
                }
                bufs.seen[dest] = bufs.epoch;
            }
            bufs.scratch.push(dest);
        }
        bufs.scratch.sort_unstable();
        bufs.scratch.dedup();
        let (id, is_new) = interner.intern(&bufs.scratch);
        if is_new {
            local_new.push((id, bufs.scratch.clone()));
        }
        row.insert(sym, vec![id]);
    }
    for &sym in &bufs.touched {
        bufs.buckets[sym].clear();
    }
    bufs.touched.clear();

    let out = i32::from(members.iter().any(|&q| fa.is_accepting(q)));
    (out, row)
}

// ---------------------------------------------------------------------------
// Instrumentation: the parallel compute win vs the post-hoc reconstruction cost.
// ---------------------------------------------------------------------------

/// Process-global counters splitting [`subset_construction_par`]'s wall time into its
/// racy-compute phase and its post-hoc reconstruction phase.
///
/// The experiment's central question is whether reconstruction eats the parallel win, so
/// the two are measured separately rather than inferred. Counters are `Relaxed` atomics —
/// they are a measurement aid, never read by the engine, and a lost update under
/// contention would cost a few nanoseconds of accuracy, not correctness.
pub mod stats {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static COMPUTE_NS: AtomicU64 = AtomicU64::new(0);
    static RECOVER_NS: AtomicU64 = AtomicU64::new(0);
    static DELEGATED_NS: AtomicU64 = AtomicU64::new(0);
    static PAR_CALLS: AtomicU64 = AtomicU64::new(0);
    static DELEGATED_CALLS: AtomicU64 = AtomicU64::new(0);

    pub(super) fn record_parallel(compute: Duration, recover: Duration) {
        COMPUTE_NS.fetch_add(compute.as_nanos() as u64, Ordering::Relaxed);
        RECOVER_NS.fetch_add(recover.as_nanos() as u64, Ordering::Relaxed);
        PAR_CALLS.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_delegated(elapsed: Duration) {
        DELEGATED_NS.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
        DELEGATED_CALLS.fetch_add(1, Ordering::Relaxed);
    }

    /// A snapshot of the counters.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Snapshot {
        /// Wall time inside the racy parallel phase.
        pub compute: Duration,
        /// Wall time inside post-hoc canonicalization — the reconstruction overhead.
        pub recover: Duration,
        /// Wall time inside calls that fell below the threshold and ran sequentially.
        pub delegated: Duration,
        pub parallel_calls: u64,
        pub delegated_calls: u64,
    }

    pub fn snapshot() -> Snapshot {
        Snapshot {
            compute: Duration::from_nanos(COMPUTE_NS.load(Ordering::Relaxed)),
            recover: Duration::from_nanos(RECOVER_NS.load(Ordering::Relaxed)),
            delegated: Duration::from_nanos(DELEGATED_NS.load(Ordering::Relaxed)),
            parallel_calls: PAR_CALLS.load(Ordering::Relaxed),
            delegated_calls: DELEGATED_CALLS.load(Ordering::Relaxed),
        }
    }

    pub fn reset() {
        COMPUTE_NS.store(0, Ordering::Relaxed);
        RECOVER_NS.store(0, Ordering::Relaxed);
        DELEGATED_NS.store(0, Ordering::Relaxed);
        PAR_CALLS.store(0, Ordering::Relaxed);
        DELEGATED_CALLS.store(0, Ordering::Relaxed);
    }
}

/// How many workers [`subset_construction_par`] should use by default.
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

// ---------------------------------------------------------------------------
// Runtime configuration for the experiment.
// ---------------------------------------------------------------------------

/// Whether the parallel path is used at all, and whether its post-hoc reconstruction
/// happens eagerly (inside the primitive) or is deferred to the write path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// `WR_PAR=0` — the stock sequential engine. The baseline.
    Sequential,
    /// Default. Compute racily in parallel, then canonicalize *inside*
    /// [`subset_construction_par`], so the racy numbering never escapes and every
    /// downstream artifact (including `::` details text and `.ba` exports) is
    /// bit-identical to the sequential engine's.
    ParallelEagerRecovery,
    /// `WR_PAR_DEFER=1` — the aggressive variant. Compute racily and let the arbitrary
    /// numbering flow into `minimize`/`product`/the rest of the engine, relying on
    /// `write_txt`/`write_gv`'s own `canonize()` to normalize only at the very end.
    ///
    /// This is the experiment's real question, and it is **not** contract-preserving:
    /// see `docs/PAR-POSTHOC.md` for the enumerated surfaces it can and cannot recover
    /// (`export_to_ba` never canonicalizes; the `canonized = true` suppressions in
    /// `morphism`/`ostrowski` deliberately disable the writer's normalization).
    ParallelDeferredRecovery,
}

fn mode() -> Mode {
    use std::sync::OnceLock;
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| {
        let off = std::env::var("WR_PAR").is_ok_and(|v| v == "0");
        if off {
            return Mode::Sequential;
        }
        if std::env::var("WR_PAR_DEFER").is_ok_and(|v| v == "1") {
            return Mode::ParallelDeferredRecovery;
        }
        Mode::ParallelEagerRecovery
    })
}

/// The engine's single entry point for subset construction, so that one env-var switch
/// moves every determinization in the port between the sequential and the parallel
/// implementation. Called from [`crate::determinize::determinize`]'s `SC` arm.
pub fn dispatch_subset_construction(fa: &Fa, initial: &BTreeSet<usize>) -> Fa {
    match mode() {
        Mode::Sequential => subset_construction(fa, initial),
        Mode::ParallelEagerRecovery => subset_construction_par(fa, initial, default_threads()),
        Mode::ParallelDeferredRecovery => {
            subset_construction_par_raw(fa, initial, default_threads())
        }
    }
}

/// [`subset_construction_par`] without the post-hoc canonicalization — the racy numbering
/// escapes. Isomorphic to the sequential result, but **not** equal to it.
pub fn subset_construction_par_raw(fa: &Fa, initial: &BTreeSet<usize>, threads: usize) -> Fa {
    if threads <= 1
        || fa.q < min_states()
        || fa.alphabet_size == 0
        || !is_safe_to_parallelize(fa, initial)
    {
        let t = std::time::Instant::now();
        let out = subset_construction(fa, initial);
        stats::record_delegated(t.elapsed());
        return out;
    }
    let t = std::time::Instant::now();
    let out = build_racy(fa, initial, threads);
    stats::record_parallel(t.elapsed(), std::time::Duration::ZERO);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relabels `a` by `perm` (`perm[old] = new`), producing an isomorphic automaton with
    /// a different, adversarially chosen numbering — the `π(A)` of the module docs' L1.
    fn relabel(a: &Fa, perm: &[usize]) -> Fa {
        let q = a.q;
        assert_eq!(perm.len(), q);
        let mut o = vec![0i32; q];
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); q];
        for old in 0..q {
            o[perm[old]] = a.o[old];
            d[perm[old]] = a.d[old]
                .iter()
                .map(|(&s, dests)| (s, dests.iter().map(|&x| perm[x]).collect()))
                .collect();
        }
        Fa::with_states(perm[a.q0], q, a.alphabet_size, o, d)
    }

    fn assert_same_fa(actual: &Fa, expected: &Fa, context: &str) {
        assert_eq!(actual.q, expected.q, "q -- {context}");
        assert_eq!(actual.q0, expected.q0, "q0 -- {context}");
        assert_eq!(
            actual.alphabet_size, expected.alphabet_size,
            "alphabet_size -- {context}"
        );
        assert_eq!(actual.o, expected.o, "o -- {context}");
        assert_eq!(actual.d, expected.d, "d -- {context}");
        assert_eq!(
            actual.true_false, expected.true_false,
            "true_false -- {context}"
        );
    }

    /// SplitMix64, matching `determinize.rs`'s own test PRNG so a failing seed is
    /// reproducible without a dependency.
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// A random NFA with every destination in range (out-of-range ids are a panic class,
    /// covered by `determinize.rs`'s own snapshot tests, not here).
    ///
    /// `nondet_states` bounds the blow-up so these tests stay in the fast tier: only
    /// states `< nondet_states` may have more than one destination on a symbol, so the
    /// determinized size is at most `q * 2^nondet_states` rather than `2^q`. (A first
    /// draft generated unrestricted nondeterminism at `q = 40` and produced automata whose
    /// subset construction does not terminate in any practical time — the sequential
    /// implementation hung too, so this is a property of the generator, not of the code
    /// under test.)
    fn random_nfa(
        rng: &mut Rng,
        q: usize,
        alphabet_size: usize,
        nondet_states: usize,
    ) -> (Fa, BTreeSet<usize>) {
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(q);
        for state in 0..q {
            let mut row: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
            for sym in 0..alphabet_size as i32 {
                // Deliberately sparse: a state with no transition on a symbol is the
                // shape that makes `subset_construction` skip a row entry entirely.
                if rng.below(5) == 0 {
                    continue;
                }
                let len = if state < nondet_states {
                    1 + rng.below(3)
                } else {
                    1
                };
                let dests: Vec<usize> = (0..len).map(|_| rng.below(q)).collect();
                row.insert(sym, dests);
            }
            d.push(row);
        }
        let o: Vec<i32> = (0..q).map(|_| rng.below(2) as i32).collect();
        let fa = Fa::with_states(0, q, alphabet_size, o, d);
        let mut initial = BTreeSet::new();
        initial.insert(0);
        for _ in 0..rng.below(3) {
            initial.insert(rng.below(q.min(nondet_states.max(1))));
        }
        (fa, initial)
    }

    /// L2 of the module docs: `canonicalize` rewrites nothing on a `subset_construction`
    /// output. If this ever fails, the post-hoc thesis is broken at its root — the
    /// parallel path would return a *canonical* automaton that is not the *sequential*
    /// one.
    #[test]
    fn canonicalize_is_the_identity_on_subset_construction_output() {
        for case in 0..250 {
            let mut rng = Rng(0x1A2B_3C4D_5E6F_0001 ^ case as u64);
            let q = 1 + rng.below(7);
            let alphabet_size = 1 + rng.below(4);
            let (fa, initial) = random_nfa(&mut rng, q, alphabet_size, q);
            let sequential = subset_construction(&fa, &initial);
            let mut canonicalized = sequential.clone();
            canonicalized.canonicalize();
            assert_same_fa(&canonicalized, &sequential, &format!("case {case}"));
        }
    }

    /// L1 of the module docs, with adversarial permutations rather than whatever
    /// numbering a race happened to produce: for ANY relabelling of a subset-construction
    /// output, canonicalization recovers the sequential numbering exactly.
    ///
    /// This is the deterministic proof of the post-hoc thesis. The parallel
    /// implementation's own numbering is one arbitrary member of the family swept here.
    #[test]
    fn canonicalize_recovers_the_sequential_numbering_from_any_relabelling() {
        for case in 0..250 {
            let mut rng = Rng(0x9F8E_7D6C_5B4A_0001 ^ case as u64);
            let q = 1 + rng.below(7);
            let alphabet_size = 1 + rng.below(4);
            let (fa, initial) = random_nfa(&mut rng, q, alphabet_size, q);
            let sequential = subset_construction(&fa, &initial);
            let n = sequential.q;

            // Three permutation families: the reverse (worst case for a BFS renumberer —
            // it inverts discovery order), a random shuffle, and a rotation.
            let mut perms: Vec<Vec<usize>> = Vec::new();
            perms.push((0..n).rev().collect());
            perms.push((0..n).map(|i| (i + n / 2) % n).collect());
            let mut shuffled: Vec<usize> = (0..n).collect();
            for i in (1..n).rev() {
                shuffled.swap(i, rng.below(i + 1));
            }
            perms.push(shuffled);

            for (k, perm) in perms.iter().enumerate() {
                let mut relabelled = relabel(&sequential, perm);
                relabelled.canonicalize();
                assert_same_fa(
                    &relabelled,
                    &sequential,
                    &format!("case {case}, permutation {k}"),
                );
            }
        }
    }

    /// The contract test: the parallel implementation returns the sequential `Fa` field
    /// for field, on inputs big enough to actually take the parallel path.
    #[test]
    fn par_matches_the_sequential_implementation() {
        let threads = default_threads().max(2);
        for case in 0..40 {
            let mut rng = Rng(0x0BAD_F00D_0000_0001 ^ case as u64);
            // Deterministic inputs, above the real threshold: the determinized size is then
            // bounded by `q` (each metastate is a singleton), so this sweeps genuinely
            // large automata without the combinatorial blow-up an NFA of this size has.
            let q = 400 + rng.below(1600);
            let alphabet_size = 1 + rng.below(4);
            let (fa, initial) = random_nfa(&mut rng, q, alphabet_size, 0);
            assert!(
                fa.q >= MIN_STATES_FOR_PARALLEL,
                "case {case} must take the parallel path"
            );
            let expected = subset_construction(&fa, &initial);
            let actual = subset_construction_par(&fa, &initial, threads);
            assert_same_fa(&actual, &expected, &format!("case {case}"));
        }
    }

    /// The same contract on genuinely NONDETERMINISTIC inputs, forced down the parallel
    /// path by lowering the delegation threshold (see
    /// [`subset_construction_par_with_threshold`] for why an NFA cannot be both large and
    /// cheap here). This is the sweep that exercises multi-member metastates, the epoch
    /// dedup marker, and hash-cons contention on keys longer than one element.
    #[test]
    fn par_matches_the_sequential_implementation_on_nondeterministic_inputs() {
        let threads = default_threads().max(2);
        let mut multi_member_metastates = 0;
        for case in 0..250 {
            let mut rng = Rng(0x5EED_1234_ABCD_0001 ^ case as u64);
            let q = 2 + rng.below(6);
            let alphabet_size = 1 + rng.below(4);
            let (fa, initial) = random_nfa(&mut rng, q, alphabet_size, q);
            let expected = subset_construction(&fa, &initial);
            let actual = subset_construction_par_with_threshold(&fa, &initial, threads, 1);
            assert_same_fa(&actual, &expected, &format!("case {case}"));
            if expected.q > fa.q {
                multi_member_metastates += 1;
            }
        }
        // Anti-vacuity: the sweep must actually produce automata the subset construction
        // GREW, i.e. metastates with more than one member. Without this, a generator
        // regression back to deterministic inputs would silently make this test a
        // duplicate of the one above.
        assert!(
            multi_member_metastates >= 40,
            "only {multi_member_metastates} of 250 cases had a genuinely nondeterministic \
             blow-up -- the generator has degenerated"
        );
    }

    /// Determinism of the *final* artifact under a racy compute: the same input run many
    /// times must yield the identical `Fa`, even though which thread mints which id
    /// varies. (This is the in-crate half of the report's 5x byte-stability check.)
    #[test]
    fn repeated_parallel_runs_agree_bit_for_bit() {
        let threads = default_threads().max(2);
        let mut rng = Rng(0xFEED_BEEF_0000_0001);
        let (fa, initial) = random_nfa(&mut rng, 1200, 3, 0);
        let expected = subset_construction(&fa, &initial);
        for run in 0..12 {
            let actual = subset_construction_par(&fa, &initial, threads);
            assert_same_fa(&actual, &expected, &format!("run {run}"));
        }
    }

    /// **Anti-vacuity for the whole lane.** If the parallel phase happened to reproduce the
    /// sequential numbering anyway, every "post-hoc recovery" result in this module would be
    /// true but vacuous. This asserts the opposite directly: over repeated runs of the RAW
    /// (un-canonicalized) parallel construction, the numbering genuinely differs from the
    /// sequential one — and canonicalization brings every one of those distinct numberings
    /// back to the sequential answer.
    ///
    /// The `differed > 0` assertion is the load-bearing half. It is a statement about thread
    /// interleaving, so it is in principle schedulable away; with two or more workers over a
    /// frontier of hundreds of metastates it is not, in practice, close.
    #[test]
    fn the_parallel_phase_really_does_produce_a_different_numbering() {
        let threads = default_threads().max(2);
        let mut rng = Rng(0xC0FF_EE00_1234_5678);
        let (fa, initial) = random_nfa(&mut rng, 1200, 3, 0);
        let sequential = subset_construction(&fa, &initial);
        assert!(
            sequential.q > 100,
            "the fixture must be big enough to race over"
        );

        let mut differed = 0;
        for run in 0..20 {
            let raw = build_racy(&fa, &initial, threads);
            // Isomorphic in size and alphabet, whatever the numbering.
            assert_eq!(
                raw.q, sequential.q,
                "run {run}: state count is numbering-independent"
            );
            if raw.d != sequential.d || raw.o != sequential.o || raw.q0 != sequential.q0 {
                differed += 1;
            }
            let mut recovered = raw;
            recovered.canonicalize();
            assert_same_fa(
                &recovered,
                &sequential,
                &format!("run {run} after recovery"),
            );
        }
        assert!(
            differed > 0,
            "all 20 raw parallel runs happened to reproduce the sequential numbering exactly, \
             so this module's recovery tests prove nothing -- investigate before trusting them"
        );
    }

    /// A malformed automaton — one with a destination id outside `0..q` — must PANIC (as
    /// the sequential implementation does), not hang.
    ///
    /// The test is written with an explicit spawned-thread + `recv_timeout` watchdog rather
    /// than a bare `should_panic`, because the failure mode being guarded against is a
    /// DEADLOCK: without [`is_safe_to_parallelize`], the panicking worker never reaches its
    /// next `Barrier::wait()` and the others block there forever, so a `should_panic` test
    /// would hang the whole `cargo test` run instead of failing. Mutation-checking this
    /// (deleting the guard) reproduces exactly that, which is why the watchdog is here.
    #[test]
    fn a_malformed_automaton_panics_rather_than_deadlocking_the_workers() {
        let q = MIN_STATES_FOR_PARALLEL + 40;
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); q];
        for (i, row) in d.iter_mut().enumerate() {
            row.insert(0, vec![(i + 1) % q]);
        }
        // The poison: a destination id past the end of the state set.
        d[0].insert(1, vec![q + 7]);
        let fa = Fa::with_states(0, q, 2, vec![0; q], d);
        let initial: BTreeSet<usize> = [0usize].into_iter().collect();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                subset_construction_par(&fa, &initial, default_threads().max(2))
            }));
            let _ = tx.send(outcome.is_err());
        });

        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(true) => {}
            Ok(false) => panic!(
                "a destination id outside 0..q must panic, as the sequential implementation does"
            ),
            Err(_) => panic!(
                "subset_construction_par neither returned nor panicked within 30s on a \
                 malformed automaton -- the workers are deadlocked on the barrier, which is \
                 exactly what is_safe_to_parallelize exists to prevent"
            ),
        }
    }

    /// The guard must not fire on well-formed input, or it would silently disable the whole
    /// parallel path (and every other test here would still pass, vacuously).
    #[test]
    fn the_liveness_guard_admits_well_formed_automata() {
        let mut rng = Rng(0x11FF_2200_3344_5566);
        let (fa, initial) = random_nfa(&mut rng, 200, 3, 2);
        assert!(
            is_safe_to_parallelize(&fa, &initial),
            "a well-formed generated automaton must be admitted to the parallel path"
        );
        let mut poisoned = fa.clone();
        poisoned.d[3].insert(0, vec![fa.q]);
        assert!(
            !is_safe_to_parallelize(&poisoned, &initial),
            "an out-of-range destination must be rejected"
        );
        let bad_seed: BTreeSet<usize> = [fa.q + 1].into_iter().collect();
        assert!(
            !is_safe_to_parallelize(&fa, &bad_seed),
            "an out-of-range seed member must be rejected"
        );
    }

    /// Isolated micro-measurement of the primitive: sequential vs racy-parallel vs the
    /// post-hoc recovery phase, on one large synthetic determinization.
    ///
    /// `#[ignore]`d — it is a measurement, not an assertion, and the machine it runs on is
    /// shared. Run with
    /// `cargo test -p wr-core --release par_determinize::tests::micro -- --ignored --nocapture`.
    ///
    /// It exists because the end-to-end `benches` numbers cannot separate "the parallel
    /// phase is slow" from "the corpus never reaches the parallel path"; this can.
    #[test]
    #[ignore = "measurement, not an assertion; machine-shared timings"]
    fn micro_sequential_vs_parallel() {
        let threads = default_threads().max(2);
        for (q, alphabet_size) in [(2_000usize, 4usize), (20_000, 4), (100_000, 8)] {
            let mut rng = Rng(0xABCD_0000_0000_0001 ^ q as u64);
            let (fa, initial) = random_nfa(&mut rng, q, alphabet_size, 0);

            let t = std::time::Instant::now();
            let sequential = subset_construction(&fa, &initial);
            let seq = t.elapsed();

            // Warm, then measure phase A and phase B separately.
            let _ = build_racy(&fa, &initial, threads);
            let t = std::time::Instant::now();
            let racy = build_racy(&fa, &initial, threads);
            let compute = t.elapsed();
            let mut recovered = racy;
            let t = std::time::Instant::now();
            recovered.canonicalize();
            let recover = t.elapsed();

            assert_same_fa(&recovered, &sequential, &format!("q={q}"));
            let total = compute + recover;
            println!(
                "q={q:>7} alpha={alphabet_size} out={out:>7} threads={threads} | \
                 seq {seq:>10.3?} | par-compute {compute:>10.3?} | recover {recover:>10.3?} | \
                 par-total {total:>10.3?} | speedup {ratio:.2}x (compute-only {conly:.2}x)",
                out = sequential.q,
                ratio = seq.as_secs_f64() / total.as_secs_f64(),
                conly = seq.as_secs_f64() / compute.as_secs_f64(),
            );
        }
    }

    /// The threshold delegation is real, not decorative: a tiny input must go down the
    /// sequential path (which is what keeps the malformed-input panic sites identical).
    #[test]
    fn small_inputs_delegate_to_the_sequential_implementation() {
        let mut rng = Rng(0x0102_0304_0506_0708);
        let (fa, initial) = random_nfa(&mut rng, MIN_STATES_FOR_PARALLEL - 1, 2, 0);
        let expected = subset_construction(&fa, &initial);
        let actual = subset_construction_par(&fa, &initial, 8);
        assert_same_fa(&actual, &expected, "below-threshold delegation");
    }

    /// A zero-alphabet automaton is the one shape whose sequential path never reads
    /// `fa.d` at all; it must stay on that path regardless of size.
    #[test]
    fn a_zero_alphabet_automaton_delegates_to_the_sequential_implementation() {
        let q = MIN_STATES_FOR_PARALLEL + 10;
        let fa = Fa::with_states(0, q, 0, vec![0; q], vec![BTreeMap::new(); q]);
        let initial: BTreeSet<usize> = [0usize].into_iter().collect();
        let expected = subset_construction(&fa, &initial);
        let actual = subset_construction_par(&fa, &initial, 8);
        assert_same_fa(&actual, &expected, "zero alphabet");
    }
}
