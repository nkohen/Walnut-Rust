// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **EXPERIMENT — the `agent/par-posthoc` lane.** A Rust-only head-to-head between the
//! three `wr_core::par_determinize` modes, over the same real corpus fixtures
//! `benches/src/bin/compare.rs` uses against the JVM.
//!
//! There is no JVM here on purpose: this measures the *lane's own* question — does the
//! parallel compute win survive the post-hoc reconstruction it pays for — which is a
//! Rust-vs-Rust question. The cross-engine comparison is `compare.rs`'s job and is
//! unaffected (the eager mode returns the sequential automaton field for field).
//!
//! The determinization mode is a process-wide `OnceLock` read from the environment, so
//! this binary measures ONE mode per invocation and the driver runs it three times:
//!
//! ```text
//! WR_PAR=0        cargo run --release -p wr-bench --bin par_compare   # sequential baseline
//!                 cargo run --release -p wr-bench --bin par_compare   # eager recovery
//! WR_PAR_DEFER=1  cargo run --release -p wr-bench --bin par_compare   # deferred recovery
//! ```
//!
//! Each run prints, per workload, a machine-readable line:
//! `RESULT <id> <mean_ns> <median_ns> <compute_ns> <recover_ns> <par_calls> <artifact_hash>`
//!
//! `artifact_hash` is a hash of the **written `.txt` bytes** (`RustEngine::dispatch` renders
//! through the real writer, so `canonize()` has run). Comparing it across modes is the
//! byte-exact final-artifact check; comparing it across repeated runs of the SAME mode is
//! the determinism check.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use wr_bench::{fmt_dur, workloads, Answer, RustEngine, Stats};
use wr_core::par_determinize::stats as par_stats;

fn artifact_hash(answer: &Answer) -> u64 {
    let mut h = DefaultHasher::new();
    match answer {
        Answer::Automaton(txt) => {
            "automaton".hash(&mut h);
            txt.hash(&mut h);
        }
        Answer::True => "true".hash(&mut h),
        Answer::False => "false".hash(&mut h),
        Answer::None => "none".hash(&mut h),
        Answer::Error(e) => {
            "error".hash(&mut h);
            e.hash(&mut h);
        }
    }
    h.finish()
}

fn mode_label() -> &'static str {
    if std::env::var("WR_PAR").is_ok_and(|v| v == "0") {
        "sequential"
    } else if std::env::var("WR_PAR_DEFER").is_ok_and(|v| v == "1") {
        "parallel-deferred-recovery"
    } else {
        "parallel-eager-recovery"
    }
}

fn main() {
    let corpus_root = wr_bench::walnut_java_dir();
    let dest = std::env::temp_dir().join(format!("wr-par-compare-{}", std::process::id()));
    let engine = match RustEngine::prepare(&corpus_root, &dest) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("could not prepare the Rust engine: {e}");
            eprintln!("(set WALNUT_JAVA_DIR to the walnut-java checkout)");
            std::process::exit(1);
        }
    };
    let workloads = match workloads() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("could not load the workload table: {e}");
            std::process::exit(1);
        }
    };

    let mode = mode_label();
    println!("# mode: {mode}");
    println!(
        "# {:<6} {:>12} {:>12} {:>12} {:>12} {:>8}  artifact",
        "id", "mean", "median", "par-compute", "recover", "par-calls"
    );

    for w in &workloads {
        // Sample counts scale with the workload's own cost, matching `compare.rs`'s policy:
        // enough repetitions to be meaningful on a fast row, not so many that a 2 s row
        // takes minutes.
        let (warmup, measure) = if w.approx_secs > 0.5 {
            (1, 3)
        } else if w.approx_secs > 0.05 {
            (2, 8)
        } else {
            (5, 40)
        };

        // The answer is taken OUTSIDE the timed region so rendering the `.txt` never lands
        // in a timing sample.
        let answer = match engine.dispatch(&w.command) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("fixture {}: dispatch failed: {e}", w.id);
                std::process::exit(1);
            }
        };
        let hash = artifact_hash(&answer);

        // `WR_PAR_HASH_ONLY=1`: emit the written-artifact hash and skip timing entirely.
        // This is the mode the 5x determinism check runs in — it needs the BYTES to be
        // stable across repeated racy runs, and timing 11 workloads five times over would
        // cost minutes for numbers the check never looks at.
        if std::env::var("WR_PAR_HASH_ONLY").is_ok_and(|v| v == "1") {
            println!("RESULT {:<6} {:>12} {:>12} {:>12} {:>12} {:>8}  {hash:016x}", w.id, 0, 0, 0, 0, 0);
            continue;
        }

        par_stats::reset();
        let samples = engine.bench(&w.command, warmup, measure);
        let snap = par_stats::snapshot();
        let stats = match Stats::of(&samples) {
            Some(s) => s,
            None => {
                eprintln!("fixture {}: no samples", w.id);
                std::process::exit(1);
            }
        };

        // The counters accumulate over `warmup + measure` dispatches, so scale them to a
        // single dispatch to be comparable with `mean`.
        let iters = (warmup + measure) as u32;
        let compute = snap.compute / iters;
        let recover = snap.recover / iters;

        println!(
            "RESULT {:<6} {:>12} {:>12} {:>12} {:>12} {:>8}  {hash:016x}",
            w.id,
            stats.mean.as_nanos(),
            stats.median.as_nanos(),
            compute.as_nanos(),
            recover.as_nanos(),
            snap.parallel_calls,
        );
        println!(
            "#      {:<6} {:>12} {:>12} {:>12} {:>12} {:>8}  {}",
            w.id,
            fmt_dur(stats.mean),
            fmt_dur(stats.median),
            fmt_dur(compute),
            fmt_dur(recover),
            snap.parallel_calls,
            w.why,
        );
    }

    let _ = std::fs::remove_dir_all(&dest);
}
