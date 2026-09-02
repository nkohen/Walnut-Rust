// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **`agent/par-max` EXPERIMENT — the one place that decides whether to go parallel.**
//!
//! This module exists only on the `agent/par-max` experiment branch. It is not part of the
//! mechanical port: real Walnut is single-threaded, so there is no Java original here.
//!
//! # What this is for
//!
//! Every parallel site in `wr-core` asks this module two questions and nothing else:
//!
//! * [`enabled`] — is parallelism switched on at all for this process?
//! * [`threads`] — how many workers is the pool allowed to use?
//!
//! …and every site additionally applies its OWN size threshold before handing work to the
//! pool, because the engine runs its hot loops over a huge number of *small* automata as well
//! as a few large ones. Measured on the benchmark corpus, a single fixture dispatch runs
//! ~190 separate `subset_construction` calls, of which one or two are large and the rest are
//! a few hundred metastates. Handing the small ones to a work-stealing pool costs more in
//! split/join bookkeeping than the work itself, so the thresholds are load-bearing, not
//! defensive.
//!
//! # Why this cannot change an answer
//!
//! Parallelism is applied only where the work being split is a **pure function of read-only
//! inputs** and the results are merged back **in the original sequential order**. See
//! `crate::determinize::subset_construction` for the concrete argument in the one place this
//! is currently used. Nothing here introduces a nondeterministic reduction (no floating point,
//! no order-dependent set iteration, no "first writer wins"), so the branch this module
//! selects is an implementation detail, not a semantic one.
//!
//! # Environment knobs (experiment-only)
//!
//! | variable | effect |
//! |---|---|
//! | `WR_PAR=0` | force every site onto its sequential path (the A/B control) |
//! | `WR_PAR_THREADS=N` | cap the worker count at `N` (`1` is equivalent to `WR_PAR=0` in effect, but still exercises the parallel code path — use it to separate "parallelism helps" from "the restructuring helps") |
//!
//! Both are read **once**, on first use, and cached: a mid-run change would make two halves of
//! one measurement incomparable.

use std::sync::OnceLock;

/// The resolved policy, computed once per process.
struct Policy {
    enabled: bool,
    threads: usize,
}

static POLICY: OnceLock<Policy> = OnceLock::new();

fn policy() -> &'static Policy {
    POLICY.get_or_init(|| {
        let enabled = !std::env::var("WR_PAR").is_ok_and(|v| v == "0");
        let requested: Option<usize> = std::env::var("WR_PAR_THREADS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .filter(|&n| n >= 1);
        // `rayon::current_num_threads()` reports the GLOBAL pool's size, which is what every
        // site here submits to. Reading it (rather than `available_parallelism`) means an
        // operator who has already configured `RAYON_NUM_THREADS` gets the number they asked
        // for, and `WR_PAR_THREADS` narrows it further rather than fighting it.
        let available = rayon::current_num_threads().max(1);
        let threads = requested.map_or(available, |n| n.min(available));
        Policy {
            enabled: enabled && threads > 1,
            threads,
        }
    })
}

/// Whether parallel paths are switched on for this process.
///
/// `false` when `WR_PAR=0`, or when only one worker is available — in both cases every site
/// must take its sequential path, which is the exact code that ran before this branch existed.
#[inline]
pub fn enabled() -> bool {
    policy().enabled
}

/// The number of workers a parallel site may plan for (>= 1, and 1 when [`enabled`] is false).
///
/// Used only to size work chunks. Submitting more chunks than this is fine and normal — rayon
/// load-balances by stealing — so this is a *planning* number, not a hard limit.
#[inline]
pub fn threads() -> usize {
    policy().threads
}

/// Splits `len` items into chunks sized so that every worker gets several, which is what lets
/// rayon's stealing balance a level whose items have very different costs (a metastate with
/// 3 members and one with 300 both live in the same BFS level).
///
/// Returns a chunk size of at least `min_chunk`, so a level that is barely over its site's
/// threshold does not fragment into single-item jobs.
#[inline]
pub fn chunk_size(len: usize, min_chunk: usize) -> usize {
    let target_chunks = threads().saturating_mul(8).max(1);
    (len / target_chunks).max(min_chunk).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The policy is a process-wide `OnceLock`, so these assertions have to hold for whatever
    /// environment the test binary happens to run under rather than setting the variables
    /// themselves (which would race every other test in the binary).
    #[test]
    fn the_policy_is_internally_consistent() {
        assert!(threads() >= 1);
        // `enabled()` implies more than one worker: a "parallel" run on one thread is a
        // sequential run with extra bookkeeping, and no site should pay for it.
        assert!(!enabled() || threads() > 1);
    }

    #[test]
    fn chunk_size_never_returns_zero_and_respects_its_floor() {
        for len in [0usize, 1, 7, 64, 1_000, 1_000_000] {
            for min in [1usize, 8, 64] {
                let c = chunk_size(len, min);
                assert!(c >= 1, "chunk_size({len}, {min}) == 0");
                assert!(c >= min, "chunk_size({len}, {min}) = {c} < floor {min}");
            }
        }
    }

    #[test]
    fn chunk_size_gives_every_worker_several_chunks_on_a_large_level() {
        // The property the balancing argument rests on: on a level far larger than the
        // worker count, the number of chunks is a multiple of `threads()`, not equal to it.
        let len = threads() * 8 * 100;
        let chunks = len.div_ceil(chunk_size(len, 1));
        assert!(
            chunks >= threads() * 4,
            "{chunks} chunks for {} workers is too coarse to steal against",
            threads()
        );
    }
}
