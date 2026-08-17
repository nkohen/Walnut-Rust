// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **Criterion benchmark of the Rust side alone** (Phase 4, U32).
//!
//! This is the statistical half of U32: repeated-sample timing of `wr-cli`'s real dispatch
//! path, with confidence intervals and saved baselines, so a future change to `wr-core` can be
//! regression-checked against a stored `cargo bench` baseline. It does **not** talk to the JVM
//! — the head-to-head lives in `src/bin/compare.rs`, which measures both engines with one
//! identical loop (see `benches/README.md` §"Two measurements, on purpose").
//!
//! ```bash
//! cargo bench -p wr-bench                       # all workloads
//! cargo bench -p wr-bench -- fixture-286        # one
//! ```
//!
//! Requires the sibling `walnut-java` checkout for the corpus, and says so loudly if it is
//! missing (`CLAUDE.md`'s absent-oracle contract) rather than benchmarking nothing.

use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use wr_bench::{golden, workloads, RustEngine, Workload};

/// Criterion's defaults (3 s warm-up, 100 samples, 5 s measurement, `Auto` sampling) are wrong
/// above ~50 ms per iteration — which the plan calls out explicitly, and which this crate's
/// workloads reach immediately.
///
/// Two things have to change together, not just the sample count:
///
/// * **`SamplingMode::Flat`.** `Auto` uses Criterion's linear scheme, where `n` samples cost
///   `n(n+1)/2` *iterations* — 465 of them for the default-ish `n = 30`. On fixture 637 (~90 ms)
///   that is 42 s of measurement for one fixture, and Criterion says so ("Unable to complete 30
///   samples in 30.0s"). `Flat` is Criterion's own recommendation for long-running benchmarks:
///   one iteration per sample, so the cost is `n × per-iteration` and stays predictable.
/// * **Time budgets derived from the workload's known cost** (`Workload::approx_secs`), rather
///   than fixed constants that are simultaneously too long for the microsecond workloads and too
///   short for the multi-second ones.
///
/// 10 is Criterion's own minimum sample size; it is the floor here for that reason.
fn sampling(approx_secs: f64) -> (usize, SamplingMode, Duration, Duration) {
    if approx_secs < 0.05 {
        // Sub-50 ms: Criterion's defaults are exactly right, linear sampling included.
        return (
            100,
            SamplingMode::Auto,
            Duration::from_secs(3),
            Duration::from_secs(5),
        );
    }
    let samples = if approx_secs < 1.0 { 30 } else { 10 };
    // ~3 iterations of warm-up, matching `compare`'s `DEFAULT_WARMUP`, clamped so a fast
    // workload still gets a full second and a slow one does not warm up for a minute.
    let warm_up = (approx_secs * 3.0).clamp(1.0, 10.0);
    // +30% headroom so Criterion never has to warn that it could not finish in the window.
    let measurement = (approx_secs * samples as f64 * 1.3).max(5.0);
    (
        samples,
        SamplingMode::Flat,
        Duration::from_secs_f64(warm_up),
        Duration::from_secs_f64(measurement),
    )
}

fn bench_workloads(c: &mut Criterion) {
    let Some(root) = golden::corpus_root() else {
        panic!(
            "the benchmark corpus was not found.\n\
             Looked for `src/test/resources/integrationTests/` under {}.\n\
             The workloads are real fixtures from the sibling `walnut-java` oracle repo and are\n\
             deliberately not vendored here; point WALNUT_JAVA_DIR at a walnut-java checkout.",
            golden::walnut_java_dir().display()
        );
    };
    let workloads: Vec<Workload> = workloads().unwrap_or_else(|e| panic!("loading workloads: {e}"));

    let dest = std::env::temp_dir().join(format!("wr-bench-rust-{}", std::process::id()));
    let engine = RustEngine::prepare(&root, &dest)
        .unwrap_or_else(|e| panic!("preparing the Rust engine (session tree + prelude): {e}"));

    let mut group = c.benchmark_group("dispatch");
    for w in &workloads {
        let (samples, mode, warm_up, measurement) = sampling(w.approx_secs);
        group
            .sample_size(samples)
            .sampling_mode(mode)
            .warm_up_time(warm_up)
            .measurement_time(measurement);
        group.bench_function(format!("fixture-{}", w.id), |b| {
            // `iter_custom`, not `iter`: the timed region is exactly `RustEngine::timed_dispatch`
            // — the same function the head-to-head's Rust column measures — so the two
            // measurements cannot drift apart.
            b.iter_custom(|iters| (0..iters).map(|_| engine.timed_dispatch(&w.command)).sum());
        });
    }
    group.finish();

    let _ = std::fs::remove_dir_all(&dest);
}

criterion_group!(benches, bench_workloads);
criterion_main!(benches);
