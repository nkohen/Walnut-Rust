// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **`agent/par-max` experiment tool — the Rust column ONLY, no JVM in the loop.**
//!
//! `src/bin/compare.rs` is the real deliverable: it starts a warm `walnut-java` JVM, checks
//! every answer across the two engines before believing a timing, and prints the table
//! `STATUS.md` records. That is the right shape for a *reported* number and the wrong shape
//! for a *steering* number: starting the JVM, replaying the 19-command prelude and running
//! every workload twice costs ~90 s before any Rust code is timed, which makes an A/B loop
//! over a parallelization prototype unusably slow.
//!
//! This binary reuses the identical [`RustEngine`] setup (same corpus, same session tree,
//! same prelude, same warm `Prover`) and the identical [`RustEngine::bench`] loop, and prints
//! only the Rust median/mean. It deliberately does NOT print a speedup: there is no oracle
//! here, so nothing it prints is a head-to-head claim.
//!
//! It still checks the answer against the corpus's own recorded automaton where one exists —
//! that check needs no JVM (the file is on disk) and it is the cheap half of `compare`'s
//! correctness gate, so there is no reason to drop it.
//!
//! ```text
//! WR_BENCH_ONLY=230,286 WR_BENCH_ITERS=3 WR_BENCH_WARMUP=2 \
//!   cargo run -p wr-bench --release --bin rustonly
//! ```
//!
//! Env knobs are a subset of `compare`'s and mean the same things: `WR_BENCH_ONLY`,
//! `WR_BENCH_ITERS`, `WR_BENCH_WARMUP`, `WR_BENCH_HEAVY`.

use std::time::Instant;

use wr_bench::{fmt_dur, golden, is_non_fixture_row, label, same_answer, workloads, RustEngine};

fn main() {
    if let Err(e) = run() {
        eprintln!("\nRUSTONLY FAILED: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = golden::corpus_root()
        .ok_or_else(|| "the benchmark corpus was not found; set WALNUT_JAVA_DIR".to_string())?;

    let only: Option<Vec<usize>> = std::env::var("WR_BENCH_ONLY").ok().map(|s| {
        s.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().parse().expect("WR_BENCH_ONLY: not a fixture id"))
            .collect()
    });
    let iters: usize = std::env::var("WR_BENCH_ITERS")
        .ok()
        .map(|s| s.trim().parse().expect("WR_BENCH_ITERS: not a number"))
        .unwrap_or(5);
    let warmup: usize = std::env::var("WR_BENCH_WARMUP")
        .ok()
        .map(|s| s.trim().parse().expect("WR_BENCH_WARMUP: not a number"))
        .unwrap_or(2);

    let mut all = workloads()?;
    if std::env::var("WR_BENCH_HEAVY").is_ok_and(|v| v != "0") {
        all.extend(wr_bench::heavy_workloads());
    }
    let selected: Vec<_> = match &only {
        Some(ids) => all.into_iter().filter(|w| ids.contains(&w.id)).collect(),
        None => all,
    };
    if selected.is_empty() {
        return Err("WR_BENCH_ONLY selected no workloads".to_string());
    }

    let scratch = std::env::temp_dir().join(format!("wr-rustonly-{}", std::process::id()));
    eprintln!("preparing the Rust engine ...");
    let t0 = Instant::now();
    let rust = RustEngine::prepare(&root, &scratch.join("rust"))?;
    eprintln!("  ready in {:.1}s", t0.elapsed().as_secs_f64());

    println!("fix        iters      rust mean    rust median   vs recorded corpus automaton");
    println!("-----------------------------------------------------------------------------");
    for w in &selected {
        eprintln!("=== DISPATCH {}", w.id);
        let answer = rust.dispatch(&w.command)?;
        let recorded = match &answer {
            _ if is_non_fixture_row(w.id) => "n/a (not a corpus fixture)".to_string(),
            wr_bench::Answer::Automaton(_) => {
                let path = root.join(format!("automaton{}.txt", w.id));
                match std::fs::read_to_string(&path) {
                    Ok(txt) => match same_answer(&wr_bench::Answer::Automaton(txt), &answer) {
                        Ok(()) => "matches".to_string(),
                        Err(e) => format!("DIFFERS ({e})"),
                    },
                    Err(_) => "none on disk".to_string(),
                }
            }
            other => format!("n/a ({})", other.kind()),
        };
        if recorded.starts_with("DIFFERS") {
            return Err(format!("fixture {}: {recorded}", w.id));
        }
        let samples = rust.bench(&w.command, warmup, iters);
        let stats = wr_bench::Stats::of(&samples).ok_or("no samples")?;
        println!(
            "{:<10} {:>5}   {:>12}   {:>12}   {}",
            label(w.id),
            iters,
            fmt_dur(stats.mean),
            fmt_dur(stats.median),
            recorded
        );
    }
    Ok(())
}
