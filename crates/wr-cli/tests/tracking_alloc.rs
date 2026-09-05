// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! `wr_cli::tracking_alloc::TrackingAllocator` as this test binary's global allocator:
//! pins that `memory_meter::live_bytes()` measures **live** heap (rises on allocation,
//! falls back on free, does not drift under `realloc` growth), that counting is off until
//! enabled, and that a memory-capped `Engine` in a process with the wrapper linked really
//! fires `EXPLODED-mem`. Its own binary because `#[global_allocator]` is per program.

use std::alloc::System;
use std::fs;
use std::hint::black_box;

use wr_cli::embed::resource::{memory_meter, ExhaustedReason, ResourceBudget};
use wr_cli::embed::Engine;
use wr_cli::prover::ProverError;
use wr_cli::tracking_alloc::TrackingAllocator;

#[global_allocator]
static GLOBAL: TrackingAllocator<System> = TrackingAllocator(System);

/// All assertions live in one test: the meter is process-global and `cargo test` runs
/// tests concurrently, so separate tests would race on the counter.
#[test]
fn live_bytes_rise_fall_and_do_not_drift() {
    assert_eq!(
        memory_meter::live_bytes(),
        None,
        "counting must be off until something enables it"
    );
    // Constructing a `Prover`/`Engine` in a process with the wrapper linked starts
    // counting by itself (review finding: a cap installed later in a session must bound
    // the whole session's heap, not just what follows the cap), so no explicit
    // `enable()` is needed here.
    let ws = std::env::temp_dir().join(format!("wr-tracking-alloc-{}", std::process::id()));
    fs::remove_dir_all(&ws).ok();
    for sub in [
        "Automata Library",
        "Word Automata Library",
        "Custom Bases",
        "Command Files",
        "Result",
        "Session",
        "Macro Library",
        "Morphism Library",
        "Transducer Library",
        "Test Results",
    ] {
        fs::create_dir_all(ws.join(sub)).unwrap();
    }
    let mut engine = Engine::new(&ws).unwrap();
    assert!(
        memory_meter::is_enabled(),
        "constructing an Engine must start the meter when a wrapper is linked"
    );
    assert_eq!(memory_meter::enable(), Ok(()), "idempotent once on");
    let baseline = memory_meter::live_bytes().unwrap();

    // Rises by (at least) the allocation, falls back on free.
    const BIG: usize = 32 << 20;
    let block = black_box(vec![1u8; BIG]);
    let during = memory_meter::live_bytes().unwrap();
    assert!(
        during >= baseline + BIG,
        "baseline {baseline}, during {during}"
    );
    drop(block);
    let after = memory_meter::live_bytes().unwrap();
    assert!(
        after < baseline + BIG / 2,
        "freeing must bring the count back down: baseline {baseline}, after {after}"
    );

    // Growth through `realloc` (Vec doubling) leaves exactly the final capacity live,
    // not the sum of every intermediate capacity.
    let base2 = memory_meter::live_bytes().unwrap();
    let mut grown: Vec<u64> = Vec::new();
    for i in 0..(1 << 20) {
        grown.push(i);
    }
    let cap_bytes = grown.capacity() * std::mem::size_of::<u64>();
    let during2 = memory_meter::live_bytes().unwrap();
    assert!(
        during2 >= base2 + cap_bytes,
        "{during2} vs {base2} + {cap_bytes}"
    );
    assert!(
        during2 < base2 + 2 * cap_bytes + (1 << 16),
        "realloc must not double-count: {during2} vs base {base2}, capacity {cap_bytes}"
    );
    drop(grown);
    assert!(memory_meter::live_bytes().unwrap() < base2 + cap_bytes);

    // And the whole thing end to end: a memory cap below what a real query needs fires
    // `EXPLODED-mem` in-process, with the structured reason.
    let live_now = memory_meter::live_bytes().unwrap();
    engine
        .set_budget(ResourceBudget {
            max_states: None,
            max_bytes: Some(live_now.saturating_sub(1).max(1)),
        })
        .unwrap();
    match engine.eval_bool(r#"eval q "?msd_2 Ax Ey (y > x)""#) {
        Err(ProverError::ResourceExhausted(e)) => {
            assert_eq!(e.reason, ExhaustedReason::Memory);
            assert!(e.at > e.limit);
        }
        other => panic!("expected EXPLODED-mem, got {other:?}"),
    }
    engine
        .set_budget(ResourceBudget {
            max_states: None,
            max_bytes: Some(4 << 30),
        })
        .unwrap();
    assert_eq!(
        engine
            .eval_bool(r#"eval q "?msd_2 Ax Ey (y > x)""#)
            .unwrap(),
        Some(true)
    );
    fs::remove_dir_all(&ws).ok();

    // The PARALLEL subset-construction path under a memory cap, in the one binary that
    // can enforce one: a 2^13-metastate construction whose wide levels clear the
    // level-parallel thresholds, capped just above the live heap at entry, must breach
    // with the structured reason and leave the process usable. Honest limit: a breach
    // here can come from a worker's pre-chunk check or from the coordinating thread's
    // per-metastate check, and nothing observable says which -- so this pins that the
    // parallel path IS memory-capped and recovers cleanly, not that the worker-side
    // check in particular fired (the review that asked for this test noted deleting
    // that check survives the suite; it still does).
    use std::collections::{BTreeMap, BTreeSet};
    use wr_cli::embed::resource::{run, BudgetError, Instrumentation, Operation};
    use wr_core::determinize::subset_construction;
    use wr_core::fa::Fa;
    let k = 13;
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
    let mut first = BTreeMap::new();
    first.insert(0, vec![0]);
    first.insert(1, vec![0, 1]);
    d.push(first);
    for i in 1..k {
        let mut row = BTreeMap::new();
        row.insert(0, vec![i + 1]);
        row.insert(1, vec![i + 1]);
        d.push(row);
    }
    d.push(BTreeMap::new());
    let mut o = vec![0; k + 1];
    o[k] = 1;
    let fa = Fa::with_states(0, k + 1, 2, o, d);
    let initial: BTreeSet<usize> = [0usize].into_iter().collect();
    let cap = memory_meter::live_bytes().unwrap() + (256 << 10);
    let instr = Instrumentation::new().with_budget(ResourceBudget {
        max_states: None,
        max_bytes: Some(cap),
    });
    match run(&instr, || subset_construction(&fa, &initial)) {
        Err(BudgetError::Exhausted(e)) => {
            assert_eq!(e.reason, ExhaustedReason::Memory);
            assert_eq!(e.operation, Operation::SubsetConstruction);
            assert_eq!(e.limit, cap);
            assert!(e.at > cap);
        }
        other => panic!("expected EXPLODED-mem from the parallel path, got {other:?}"),
    }
    // Uncapped, the same construction completes: nothing was left behind.
    assert_eq!(subset_construction(&fa, &initial).q, 1 << k);
}
