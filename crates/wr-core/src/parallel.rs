// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Parallel-coordination primitives, built on `std` alone.
//!
//! **There is no Java original here.** Real Walnut is single-threaded; this module is not
//! part of the mechanical port. It exists for one reason: the campaign profile puts
//! 49.4-87.8% of the engine's real work inside [`crate::determinize::subset_construction`]'s
//! BFS, whose per-level frontier is a set of metastates that can all be expanded at once.
//! `wr_core::determinize` is this module's only caller.
//!
//! The name is historical: an earlier draft ran the parallel levels on a persistent global
//! *worker pool*. What survives here is not a pool but the pieces the current design shares
//! with that draft — the per-level work counter and result collector [`Task`], the chunk
//! dispenser [`drain`] and its [`Direction`], the wait/finish guards, and the thread-count
//! policy ([`threads`]/[`worker_count`]/[`chunk_size`]/[`enabled`]).
//!
//! # The scoped design, and the alternative it replaced
//!
//! `determinize` drives its level-parallel subset construction with a `std::thread::scope`
//! opened once per over-threshold `subset_construction` call: workers are spawned into the
//! scope, park between BFS levels on the primitives below, and are joined by the scope on the
//! way out. It uses no raw pointers and nothing the compiler cannot check.
//!
//! It was chosen over a persistent process-wide worker pool. That alternative spawned its
//! workers once per process and parked them forever, which cost less per call — but handing a
//! parked worker a job that borrows the submitting caller's stack needed a raw-pointer region
//! the compiler could not verify. The scoped formulation pays an OS thread spawn+join per
//! over-threshold call (lazily — the ~188-of-~190 small calls in a benchmark dispatch never
//! spawn a thread) and, because scoped threads and the main thread cannot hold `&` and `&mut`
//! to the same data, puts the BFS worklist behind an `RwLock`. In exchange the whole crate
//! keeps to compiler-checked code with no raw pointers. The pool alternative and the
//! head-to-head measurement that rejected it are preserved in git history; this module is the
//! decided end state.
//!
//! # Why this cannot change an answer
//!
//! A parallel level is a *map*, not a reduction: it evaluates one chunk-expansion function
//! `f(0)..f(n-1)` in an unspecified order on an unspecified thread and collects the results
//! into a [`Task`] **indexed by chunk position `i`**, never by completion order. There is no
//! floating point, no order-dependent set iteration and no first-writer-wins anywhere in it.
//! A caller whose `f` is a pure function of shared immutable data therefore gets results that
//! do not depend on the schedule, the worker count, or the machine — which is exactly the
//! contract `subset_construction`'s bit-identity argument rests on. `determinize.rs`'s
//! `the_parallel_schedule_matches_the_pre_p1a_reference_implementation` and
//! `a_forced_out_of_order_completion_still_produces_the_sequential_output` are the tests that
//! hold it to that.
//!
//! Panic attribution is likewise index-ordered, not arrival-ordered: if several chunks panic,
//! the one with the **lowest index** is the panic re-raised to the caller (see
//! [`Task::take_results`]). Since the caller's chunks are ordered the way a sequential loop
//! would visit them, the panic a caller sees is the panic the sequential path would have
//! raised. Achieving that costs one thing, stated rather than hidden: a panicking run does
//! **not** abort early, so every chunk of the offending level is evaluated before the panic
//! surfaces — work done only on malformed input, on a query that is being aborted anyway.
//!
//! # Worker count
//!
//! Read once per process, on first use, and cached — a mid-run change would make two halves
//! of one measurement incomparable. See [`resolve_threads`] for the exact rules and
//! `WR_CORE_THREADS` for the override.

use std::sync::OnceLock;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread;

/// The default ceiling on the total parallel degree (workers + the calling thread), used
/// when `WR_CORE_THREADS` is not set.
///
/// **PROVISIONAL — NOT YET TUNED. Tracked follow-up: `docs/BACKLOG-D3-THREAD-TUNING.md`.**
/// P5 shipped with this placeholder (the plan's `min(available_parallelism - 1, 6)` workers,
/// i.e. `7` total) because the machine would not go quiet enough for the D3 worker-count
/// sweep (2/4/6/max on a quiet machine) that is meant to set it. This is a tuning knob, not a
/// correctness parameter — every value in `[1, HARD_MAX_THREADS]` produces bit-identical
/// output (the determinism gate proves it) — so shipping a placeholder is safe, but the sweep
/// still owes a measured value. Do not cite this as a tuned number until that backlog item is
/// closed.
///
/// One thing it is NOT: "leave a core for the JVM". That is the benchmark harness's
/// reasoning — the harness runs `walnut-java` alongside — and has nothing to do with an
/// embedder, which is why this is expressed as a plain ceiling rather than as
/// `available_parallelism() - 1`.
const DEFAULT_MAX_THREADS: usize = 7;

/// The ceiling on an explicit `WR_CORE_THREADS` override.
///
/// An explicit request is honoured up to this; beyond it the value is clamped rather than
/// obeyed literally, so a fat-fingered `WR_CORE_THREADS=100000` costs a clamp instead of a
/// thread-exhaustion failure deep inside a query. Stated as a deliberate divergence from a
/// literal reading of "override": the knob exists to bound parallelism, and every value it
/// accepts has defined behavior.
const HARD_MAX_THREADS: usize = 64;

/// The environment override for the total parallel degree. Read once, at first use.
const THREADS_VAR: &str = "WR_CORE_THREADS";

/// Locks `m`, treating a poisoned mutex as an ordinary one.
///
/// Poisoning cannot help here and can only hurt: nothing in this module panics while holding
/// a lock (every chunk-function call and every panic payload move happens outside the
/// critical section), so a poisoned flag would only ever be a second-order consequence of a
/// panic the caller is already being told about — and turning it into an `unwrap()` panic
/// *inside a `Drop` guard* would upgrade that to a double-panic abort. See [`WaitGuard`],
/// whose soundness argument requires that it cannot panic.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// The process-wide resolved parallel degree, fixed on first use (or by an explicit
/// [`set_thread_count`] call before then). Hoisted to module scope — rather than a local
/// `static` inside [`threads`] — so an embedder can pin it programmatically.
static THREADS: OnceLock<usize> = OnceLock::new();

/// Returned by [`set_thread_count`] when the parallel degree is already fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParallelismAlreadyStarted;

impl std::fmt::Display for ParallelismAlreadyStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "wr-core's parallel degree is already fixed; set_thread_count must be called \
             before the first determinization",
        )
    }
}

impl std::error::Error for ParallelismAlreadyStarted {}

/// Pin `wr-core`'s parallel degree programmatically — the in-process opt-out an embedder
/// (e.g. `ct-research`) needs when it does not want this crate spawning worker threads and
/// cannot, or will not, set the `WR_CORE_THREADS` environment variable.
///
/// `total_degree` is the calling thread plus workers, same meaning as `WR_CORE_THREADS`:
/// **`1` is fully sequential** (no worker is ever spawned — the pre-P1(a) production path),
/// `N` caps at [`HARD_MAX_THREADS`]. This is a hard override: when set, it wins over both
/// `WR_CORE_THREADS` and the CPU-count default.
///
/// Must be called before the first determinization; the degree is a write-once global.
/// Returns [`ParallelismAlreadyStarted`] if a query has already fixed it (or a prior call
/// did), leaving the existing value unchanged — an embedder that must guarantee sequential
/// execution should call this at startup and treat the error as "someone already ran a
/// query," not ignore it.
pub fn set_thread_count(total_degree: usize) -> Result<(), ParallelismAlreadyStarted> {
    THREADS
        .set(total_degree.clamp(1, HARD_MAX_THREADS))
        .map_err(|_| ParallelismAlreadyStarted)
}

/// The total parallel degree: the calling thread plus [`worker_count`] scoped workers.
///
/// `1` means "fully sequential" — [`enabled`] is false and no worker is ever spawned.
fn threads() -> usize {
    *THREADS.get_or_init(|| {
        let requested = std::env::var(THREADS_VAR).ok();
        let available = thread::available_parallelism().ok().map(|n| n.get());
        resolve_threads(requested.as_deref(), available)
    })
}

/// Resolves the total parallel degree from the raw environment value and the machine's
/// reported parallelism. Split out as a pure function so every edge case
/// (`~/.claude/plans/perf-beyond-p5-parallel-promotion.md`'s "0, 1, 2, absurd (1024), and
/// `available_parallelism` failure") is testable in-process, without the separate-process
/// dance the *end-to-end* determinism gates need.
///
/// | input | result |
/// |---|---|
/// | `WR_CORE_THREADS=0` or `=1` | `1` — fully sequential |
/// | `WR_CORE_THREADS=N` | `N`, clamped to [`HARD_MAX_THREADS`] |
/// | `WR_CORE_THREADS` unparseable/empty | ignored; the default applies |
/// | unset, `available_parallelism` = `P` | `min(P, DEFAULT_MAX_THREADS)` |
/// | unset, `available_parallelism` failed | `1` — fully sequential |
///
/// The last row is the conservative direction on purpose: if the platform cannot say how
/// many cores it has, this crate does not guess, it stays on the path that has been the
/// production path since P1(a).
fn resolve_threads(requested: Option<&str>, available: Option<usize>) -> usize {
    if let Some(raw) = requested {
        if let Ok(n) = raw.trim().parse::<usize>() {
            return n.clamp(1, HARD_MAX_THREADS);
        }
        // Unparseable (including empty): fall through to the default rather than failing a
        // query over an environment typo. There is no `Logging` down here to report it on.
    }
    match available {
        Some(p) => p.clamp(1, DEFAULT_MAX_THREADS),
        None => 1,
    }
}

/// Whether parallel execution is switched on for this process.
///
/// False when the total degree is 1 — a "parallel" run on one thread is a sequential run
/// with extra bookkeeping, and no caller should pay for it.
pub(crate) fn enabled() -> bool {
    threads() >= 2
}

/// How many worker threads a parallel level spawns (the total degree minus the calling
/// thread, which drains chunks itself).
pub(crate) fn worker_count() -> usize {
    threads() - 1
}

/// Splits `len` items into chunks sized so every drainer gets several.
///
/// Chunks are dispensed one at a time from a shared counter, so a level whose items have
/// very different costs (a metastate with 3 members and one with 300 live in the same BFS
/// level) balances itself: a drainer that draws a cheap chunk comes back for another. That
/// is what the `* 8` is for. `min_chunk` keeps a level barely over its caller's threshold
/// from fragmenting into single-item jobs, each of which would allocate its own scratch.
pub(crate) fn chunk_size(len: usize, min_chunk: usize) -> usize {
    let target_chunks = threads().saturating_mul(8).max(1);
    (len / target_chunks).max(min_chunk).max(1)
}

// ---------------------------------------------------------------------------
// The indexed work counter and result collector
// ---------------------------------------------------------------------------

/// Which end of the index range a drainer takes work from.
///
/// Scoped workers take from the front, the calling thread from the back. That is not a load-
/// balancing trick — with one shared counter it would make no difference to throughput —
/// it is about which chunk the CALLER evaluates inline, and therefore which panics travel
/// through the record-and-replay path rather than simply unwinding the caller directly.
/// With a single front-to-back dispenser the caller reliably wins the race for chunk 0
/// (it starts draining microseconds after publishing the level, before any worker has woken),
/// so the lowest-indexed panic was almost always one the caller raised inline — and a
/// mutation that deleted `drain`'s per-chunk `catch_unwind` entirely went undetected, because
/// the caller's inline panic happened to carry the same message. Draining from the back
/// removes that coincidence: the low-index chunks are the workers', so
/// `determinize.rs`'s `the_parallel_schedule_reports_the_panic_the_sequential_schedule_would_have_raised`
/// now genuinely exercises cross-thread panic recording.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Front,
    Back,
}

struct TaskInner<T> {
    /// The next chunk index to hand out from the front. The un-dispensed range is
    /// `next..end`; a drainer is done when it is empty.
    next: usize,
    /// One past the next chunk index to hand out from the back.
    end: usize,
    /// Submitted jobs that have not yet finished. The calling thread is deliberately NOT
    /// counted: it waits for this to reach zero.
    outstanding: usize,
    /// Results, **indexed by chunk position** — never by completion order. This is the
    /// mechanism the caller's ordering guarantee rests on, and the reason this is a
    /// pre-sized `Vec<Option<T>>` rather than the `Vec<T>` a channel-based collector would
    /// push results onto as they arrive.
    results: Vec<Option<T>>,
    /// The lowest-indexed panic seen so far, with its payload. Lowest rather than first-to-
    /// arrive so that panic attribution does not depend on thread scheduling.
    panic: Option<(usize, Box<dyn std::any::Any + Send>)>,
}

/// A single parallel level's work counter and result collector. One [`Task`] is kept alive
/// for a whole `subset_construction` call and [`re-armed`](Task::rearm) per BFS level, because
/// the scoped workers are spawned once and can only reach data that outlives the scope.
pub(crate) struct Task<T> {
    inner: Mutex<TaskInner<T>>,
    /// Signalled when `outstanding` reaches zero.
    idle: Condvar,
}

impl<T> Task<T> {
    /// A task with `n` chunks and no jobs outstanding yet.
    pub(crate) fn new(n: usize) -> Task<T> {
        Task {
            inner: Mutex::new(TaskInner {
                next: 0,
                end: n,
                outstanding: 0,
                results: (0..n).map(|_| None).collect(),
                panic: None,
            }),
            idle: Condvar::new(),
        }
    }

    /// Re-arms this task for a fresh batch of `n` chunks with `outstanding` jobs expected.
    ///
    /// The scoped backend keeps ONE task alive for a whole `subset_construction` call and
    /// re-arms it per BFS level, because its workers are spawned once and can only reach data
    /// that outlives the scope — a task created per level would not.
    ///
    /// # Panics
    ///
    /// If any job from the previous batch is still outstanding. Re-arming under a live job
    /// would hand it a different `results` vector than the one it took its index from; the
    /// caller must have waited ([`Task::wait_idle`]) first.
    pub(crate) fn rearm(&self, n: usize, outstanding: usize) {
        let mut inner = lock(&self.inner);
        assert_eq!(
            inner.outstanding, 0,
            "wr-core parallel: Task::rearm while a job is still running"
        );
        inner.next = 0;
        inner.end = n;
        inner.outstanding = outstanding;
        inner.results.clear();
        inner.results.resize_with(n, || None);
        inner.panic = None;
    }

    /// Records one job as finished, waking the waiter when the last one lands. This is
    /// [`DoneGuard`]'s whole body, exposed so the scoped backend's workers can use the
    /// identical accounting.
    pub(crate) fn finish_one(&self) {
        let mut inner = lock(&self.inner);
        inner.outstanding -= 1;
        if inner.outstanding == 0 {
            self.idle.notify_all();
        }
    }

    /// Blocks until no job is outstanding. Must not be able to panic: it runs inside
    /// [`WaitGuard`]'s `Drop`, possibly while already unwinding.
    pub(crate) fn wait_idle(&self) {
        let mut inner = lock(&self.inner);
        while inner.outstanding > 0 {
            inner = self
                .idle
                .wait(inner)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Takes the batch's results in **index order**, or re-raises the lowest-indexed panic.
    ///
    /// # Panics
    ///
    /// Re-raises a chunk's panic with its original payload, or reports an internal
    /// invariant break if a chunk produced neither a result nor a recorded panic.
    pub(crate) fn take_results(&self, n: usize) -> Vec<T> {
        let (panic, results) = {
            let mut inner = lock(&self.inner);
            (inner.panic.take(), std::mem::take(&mut inner.results))
        };
        // Resumed with the lock released, so a `Drop` running during the unwind cannot
        // deadlock against a guard this thread still holds.
        if let Some((_, payload)) = panic {
            std::panic::resume_unwind(payload);
        }
        results
            .into_iter()
            .enumerate()
            .map(|(i, slot)| match slot {
                Some(value) => value,
                // Unreachable by construction: `drain` writes either a result or a panic for
                // every index it takes, and it takes every index before returning. Reaching
                // here would mean a panic escaped `drain` itself and was swallowed by a
                // worker's own boundary — a bug in this module, made loud rather than left
                // as an `unwrap()` with no story.
                None => panic!(
                    "wr-core parallel: chunk {i} of {n} produced neither a result nor a \
                     recorded panic -- a panic escaped the per-chunk boundary"
                ),
            })
            .collect()
    }
}

/// Blocks, on drop, until every submitted job has finished.
///
/// This is the anchor of the scoped backend's liveness argument, so it must not be able to
/// panic — a panic here while the stack is already unwinding from a job panic would abort the
/// process. Hence [`lock`]'s poison-tolerance, and hence no `unwrap` anywhere below.
pub(crate) struct WaitGuard<'a, T>(pub(crate) &'a Task<T>);

impl<T> Drop for WaitGuard<'_, T> {
    fn drop(&mut self) {
        self.0.wait_idle();
    }
}

/// Marks one job finished. Placed as the outermost binding in a worker's per-level closure so
/// it fires after everything else in the job, including while unwinding.
pub(crate) struct DoneGuard<'a, T>(pub(crate) &'a Task<T>);

impl<T> Drop for DoneGuard<'_, T> {
    fn drop(&mut self) {
        self.0.finish_one();
    }
}

/// Takes chunk indices from `task` until they run out, evaluating `f` on each.
///
/// Runs on the calling thread and on every worker alike — one body, so the sequential and
/// parallel arms cannot drift apart. Every `f` call gets its own [`catch_unwind`], which is
/// what keeps one bad chunk from taking down a drainer that still owes results for other
/// chunks.
pub(crate) fn drain<T, F>(task: &Task<T>, f: &F, from: Direction)
where
    F: Fn(usize) -> T,
{
    loop {
        let i = {
            let mut inner = lock(&task.inner);
            if inner.next >= inner.end {
                return;
            }
            match from {
                Direction::Front => {
                    let i = inner.next;
                    inner.next += 1;
                    i
                }
                Direction::Back => {
                    inner.end -= 1;
                    inner.end
                }
            }
        };
        // Evaluated OUTSIDE the lock: `f` is the expensive part, and holding the mutex
        // across it would serialize every drainer.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(i)));
        let mut inner = lock(&task.inner);
        match outcome {
            Ok(value) => inner.results[i] = Some(value),
            Err(payload) => {
                let keep = match &inner.panic {
                    Some((seen, _)) => i < *seen,
                    None => true,
                };
                if keep {
                    inner.panic = Some((i, payload));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test-only introspection
// ---------------------------------------------------------------------------

/// The resolved total parallel degree for this process (workers + the calling thread).
/// Reported by the cross-process determinism gate so each run's configuration is visible
/// beside its digest.
#[cfg(test)]
pub(crate) fn configured_threads() -> usize {
    threads()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- policy ----------------------------------------------------------

    /// The thread-count edge-case list from the plan, as a pure-function table. The
    /// end-to-end behavior of each value is checked separately, one configuration per
    /// PROCESS (`determinize.rs`'s determinism gate) — the worker count is resolved once,
    /// lazily, per process, so an in-process sweep would only ever re-test whichever value
    /// initialized it first.
    /// `set_thread_count` writes the process-global degree, so its success path cannot be
    /// tested in the normal suite without pinning `THREADS` for every other test in the
    /// process (the same OnceLock-is-global reason `resolve_threads`'s end-to-end behavior is
    /// gated per-process). This is `#[ignore]`d and run ALONE, in its own process, by
    /// `scratchpad`'s `p5-thread-gate.sh` — never by a plain `cargo test`.
    #[test]
    #[ignore = "pins the process-global thread degree; run in isolation via p5-thread-gate.sh"]
    fn set_thread_count_pins_a_sequential_degree_before_first_use() {
        // First call wins: 0 clamps to 1 (fully sequential).
        assert_eq!(set_thread_count(0), Ok(()));
        assert_eq!(threads(), 1, "pinned to sequential");
        assert!(!enabled(), "no parallelism");
        assert_eq!(worker_count(), 0, "no workers spawned");
        // Second call loses — the degree is write-once — and leaves the value unchanged.
        assert_eq!(set_thread_count(8), Err(ParallelismAlreadyStarted));
        assert_eq!(
            threads(),
            1,
            "still sequential after the rejected second call"
        );
    }

    #[test]
    fn resolve_threads_covers_every_documented_edge_case() {
        // An explicit override wins over the machine.
        assert_eq!(resolve_threads(Some("0"), Some(8)), 1, "0 is sequential");
        assert_eq!(resolve_threads(Some("1"), Some(8)), 1, "1 is sequential");
        assert_eq!(resolve_threads(Some("2"), Some(8)), 2);
        assert_eq!(
            resolve_threads(Some("4"), Some(2)),
            4,
            "not capped by cores"
        );
        assert_eq!(
            resolve_threads(Some("1024"), Some(8)),
            HARD_MAX_THREADS,
            "absurd values clamp rather than exhaust the thread table"
        );
        assert_eq!(
            resolve_threads(Some(" 3 "), Some(8)),
            3,
            "whitespace trimmed"
        );

        // Unparseable values are ignored, not fatal.
        for junk in ["", "  ", "yes", "-1", "3.5", "0x4"] {
            assert_eq!(
                resolve_threads(Some(junk), Some(8)),
                DEFAULT_MAX_THREADS.min(8),
                "{junk:?} should fall through to the default"
            );
        }

        // No override: the machine's parallelism, under the provisional ceiling.
        assert_eq!(resolve_threads(None, Some(4)), 4);
        assert_eq!(resolve_threads(None, Some(64)), DEFAULT_MAX_THREADS);
        assert_eq!(resolve_threads(None, Some(1)), 1);
        // `available_parallelism()` failed: stay on the pre-P5 production path.
        assert_eq!(resolve_threads(None, None), 1, "unknown parallelism = 1");
    }

    #[test]
    fn the_policy_is_internally_consistent() {
        assert!(threads() >= 1);
        assert_eq!(worker_count(), threads() - 1);
        assert_eq!(enabled(), worker_count() >= 1);
    }

    #[test]
    fn chunk_size_never_returns_zero_and_respects_its_floor() {
        for len in [0usize, 1, 7, 64, 1_000, 1_000_000] {
            for min in [1usize, 8, 64] {
                let c = chunk_size(len, min);
                assert!(c >= min.max(1), "chunk_size({len}, {min}) = {c}");
            }
        }
    }

    #[test]
    fn chunk_size_gives_every_drainer_several_chunks_on_a_large_level() {
        let len = threads() * 8 * 100;
        let chunks = len.div_ceil(chunk_size(len, 1));
        assert!(
            chunks >= threads() * 4,
            "{chunks} chunks for {} drainers is too coarse to balance",
            threads()
        );
    }
}
