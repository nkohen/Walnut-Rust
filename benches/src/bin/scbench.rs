// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! A/B harness for `subset_construction`'s speculative key pipeline: the same release
//! binary, the same warm session, the same workloads, run once per `WR_SC_THREADS` value.
//!
//! ```bash
//! WALNUT_JAVA_DIR=… cargo run -p wr-bench --release --bin scbench
//! WR_SC_SWEEP=0,1,2,4,7 WR_BENCH_ONLY=230,286 … --bin scbench
//! ```
//!
//! `WR_SC_THREADS=0` restores the pre-parallel code path exactly, so column `0` is the
//! honest baseline rather than a separately-built binary.
//!
//! **Deliberately Rust-only.** `src/bin/compare.rs` drives a `walnut-java` JVM on the same
//! machine, which is fine for a single-threaded comparison and actively misleading for this
//! one: the JVM's own threads would compete for exactly the cores being measured. Nothing
//! here is a cross-engine claim; `compare` remains the only thing that makes those.
//!
//! Each `WR_SC_THREADS` value gets its OWN process, because `default_workers()` memoizes
//! the env var in a `OnceLock` — one process can only ever observe one setting. This binary
//! re-execs itself per value; the child does the timing and prints one row.

use std::time::Duration;

use wr_bench::{fmt_dur, golden, heavy_workloads, label, workloads, RustEngine, Stats, Workload};

fn selected() -> Vec<Workload> {
    let mut all: Vec<Workload> = workloads().expect("workloads");
    all.extend(heavy_workloads());
    let only: Option<Vec<usize>> = std::env::var("WR_BENCH_ONLY").ok().map(|s| {
        s.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().parse().expect("WR_BENCH_ONLY: not a fixture id"))
            .collect()
    });
    match only {
        Some(ids) => all.into_iter().filter(|w| ids.contains(&w.id)).collect(),
        None => all,
    }
}

/// The child half: time every selected workload in this process and print `id=nanos` rows.
fn child() {
    let root = golden::corpus_root().expect("corpus root (set WALNUT_JAVA_DIR)");
    let tmp = std::env::temp_dir().join(format!(
        "wr-scbench-{}",
        std::env::var("WR_SC_THREADS").unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let engine = RustEngine::prepare(&root, &tmp).expect("session");

    let warmup: usize = env_usize("WR_BENCH_WARMUP", 1);
    let iters: usize = env_usize("WR_BENCH_ITERS", 3);
    for w in selected() {
        let samples = engine.bench(&w.command, warmup, iters);
        let stats = Stats::of(&samples).expect("at least one sample");
        // MINIMUM, not mean. This machine is shared with other build/test jobs, and a
        // contended sample measures the contention, not the engine. The minimum over
        // several iterations is the sample that got closest to running alone, which is the
        // quantity being compared across thread counts; the mean here is dominated by
        // whatever else happened to be scheduled.
        println!("ROW {} {}", w.id, stats.min.as_nanos());
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// One recorded run: workload id, repeat index, normalized-details digest, raw-details
/// digest, normalized length.
type DetRow = (usize, usize, String, String, usize);

/// A stable digest of one details trace, so the parent can compare traces produced by
/// different child PROCESSES without shipping megabytes of text between them. FNV-1a, which
/// needs no dependency; this is a comparison aid, not a security boundary.
fn digest(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// The child half of the determinism check: run each workload's `::` variant `WR_SC_REPEATS`
/// times in THIS process and print a digest of each run's normalized details text.
fn determinism_child() {
    let root = golden::corpus_root().expect("corpus root (set WALNUT_JAVA_DIR)");
    let tmp = std::env::temp_dir().join(format!(
        "wr-scdet-{}",
        std::env::var("WR_SC_THREADS").unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let engine = RustEngine::prepare(&root, &tmp).expect("session");
    let repeats = env_usize("WR_SC_REPEATS", 5);

    for w in selected() {
        let command = wr_bench::detail_variant(&w.command);
        for repeat in 0..repeats {
            match engine.details(&command) {
                Ok(text) => {
                    // The gate is `normalize_message` — `tests/golden`'s own comparator, a
                    // verbatim port of Java's `IntegrationTest.assertEqualMessages`, whose
                    // `replaceAll("\\d+ms", "")` removes elapsed-time text. That is not a
                    // convenience: a purely sequential engine also prints different `- Xms`
                    // values run to run, so raw equality is not a property this code ever
                    // had. Everything a scheduling change could actually alter — every state
                    // count, every line, their order and indentation — survives it and is
                    // compared. The RAW digest is reported too, so the normalization is
                    // visible rather than load-bearing-but-silent.
                    let norm = wr_bench::golden::normalize_message(&text);
                    println!(
                        "DET {} {repeat} {:016x} {:016x} {}",
                        w.id,
                        digest(&norm),
                        digest(&text),
                        norm.len()
                    );
                }
                Err(e) => println!("DET {} {repeat} ERROR ERROR 0 {e}", w.id),
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// `WR_SC_DETERMINISM=1`: the pipeline's primary failure-mode check.
///
/// A scheduling race would not usually corrupt the automaton — ids are minted on one thread
/// — so the way it would surface is `::`-details TEXT. This checks the two things that must
/// both hold, and the second is the stronger one:
///
/// 1. **Run to run.** The same query, run `WR_SC_REPEATS` times in one process with the
///    pipeline live, must produce the same details every time.
/// 2. **Against the sequential engine.** The pipeline's details must equal what
///    `WR_SC_THREADS=0` — the pre-parallel code path, in the same binary — produces. This
///    is the real property; (1) alone would be satisfied by an engine that is consistently
///    and identically wrong.
///
/// Each thread count runs in its own child process because `default_workers()` memoizes the
/// env var in a `OnceLock`, so one process can only observe one setting.
///
/// Workloads are chosen for size on purpose: below `PIPELINE_MIN_STATES` the pipeline never
/// starts, so a small fixture would pass this vacuously. The normalized length is printed so
/// an empty or truncated comparison is visible rather than counted as a pass.
fn determinism() {
    let sweep: Vec<String> = std::env::var("WR_SC_SWEEP")
        .unwrap_or_else(|_| "0,2,4,7".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    assert!(
        sweep.first().map(String::as_str) == Some("0"),
        "the sweep must start at 0: the sequential engine is the reference every other \
         column is compared against"
    );

    let exe = std::env::current_exe().expect("current exe");
    let mut seen: Vec<(String, Vec<DetRow>)> = Vec::new();
    for threads in &sweep {
        let out = std::process::Command::new(&exe)
            .env("WR_SC_CHILD", "1")
            .env("WR_SC_DETERMINISM", "1")
            .env("WR_SC_THREADS", threads)
            .output()
            .expect("re-exec");
        if !out.status.success() {
            eprintln!(
                "WR_SC_THREADS={threads} failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            std::process::exit(1);
        }
        let mut rows: Vec<DetRow> = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let Some(rest) = line.strip_prefix("DET ") else {
                continue;
            };
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 5 {
                rows.push((
                    f[0].parse().unwrap_or(0),
                    f[1].parse().unwrap_or(0),
                    f[2].to_string(),
                    f[3].to_string(),
                    f[4].parse().unwrap_or(0),
                ));
            }
        }
        seen.push((threads.clone(), rows));
    }

    let (_, reference) = &seen[0];
    let mut failures = 0;
    println!(
        "{:>9}  {:>9}  {:>18}  {:>7}",
        "workload", "threads", "normalized digest", "bytes"
    );
    for (threads, rows) in &seen {
        for (i, row) in rows.iter().enumerate() {
            let (id, repeat, norm, raw, len) = row;
            let expected = reference
                .get(i)
                .map(|r| (r.0, r.2.clone(), r.4))
                .unwrap_or((0, String::from("MISSING"), 0));
            let ok = *id == expected.0 && *norm == expected.1 && *len == expected.2;
            if !ok {
                failures += 1;
            }
            if *repeat == 0 || !ok {
                println!(
                    "{:>9}  {:>9}  {:>18}  {:>7}  {}{}",
                    label(*id),
                    threads,
                    norm,
                    len,
                    if ok { "OK" } else { "MISMATCH vs t=0" },
                    if *raw == reference.get(i).map(|r| r.3.clone()).unwrap_or_default() {
                        ""
                    } else {
                        "  (raw digest differs: ms text only)"
                    },
                );
            }
        }
    }
    if failures > 0 {
        eprintln!("\nDETERMINISM CHECK FAILED: {failures} mismatching run(s)");
        std::process::exit(1);
    }
    println!(
        "\nall runs identical: every repeat of every workload, at every thread count in \
         [{}], matches the sequential (t=0) engine's normalized details exactly",
        sweep.join(", ")
    );
}

fn main() {
    if std::env::var("WR_SC_CHILD").is_ok() {
        if std::env::var("WR_SC_DETERMINISM").is_ok() {
            determinism_child();
        } else {
            child();
        }
        return;
    }
    if std::env::var("WR_SC_DETERMINISM").is_ok() {
        determinism();
        return;
    }

    let sweep: Vec<String> = std::env::var("WR_SC_SWEEP")
        .unwrap_or_else(|_| "0,2,4,7".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let exe = std::env::current_exe().expect("current exe");
    // Each sweep value is measured `rounds` times, interleaved, and the best of those
    // rounds is kept per workload -- so a burst of load during one round degrades every
    // column's that round, rather than permanently penalising whichever column ran during
    // it. With a quiet machine one round is enough.
    let rounds: usize = env_usize("WR_SC_ROUNDS", 1);
    let mut columns: Vec<(String, Vec<(usize, u128)>)> = Vec::new();
    for threads in &sweep {
        let out = std::process::Command::new(&exe)
            .env("WR_SC_CHILD", "1")
            .env("WR_SC_THREADS", threads)
            .output()
            .expect("re-exec");
        if !out.status.success() {
            eprintln!(
                "WR_SC_THREADS={threads} failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            std::process::exit(1);
        }
        let rows: Vec<(usize, u128)> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let mut it = l.strip_prefix("ROW ")?.split_whitespace();
                Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
            })
            .collect();
        columns.push((threads.clone(), rows));
    }
    for _ in 1..rounds {
        for (threads, best) in columns.iter_mut() {
            let out = std::process::Command::new(&exe)
                .env("WR_SC_CHILD", "1")
                .env("WR_SC_THREADS", threads.as_str())
                .output()
                .expect("re-exec");
            for (i, line) in String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| l.strip_prefix("ROW "))
                .enumerate()
            {
                if let Some(ns) = line.split_whitespace().nth(1).and_then(|t| t.parse().ok()) {
                    if i < best.len() && ns < best[i].1 {
                        best[i].1 = ns;
                    }
                }
            }
        }
    }

    let baseline = &columns[0].1;
    print!("{:>9}", "workload");
    for (threads, _) in &columns {
        print!("  {:>12}", format!("t={threads}"));
    }
    for (threads, _) in columns.iter().skip(1) {
        print!("  {:>8}", format!("t{threads} vs t0"));
    }
    println!();
    for (i, (id, base)) in baseline.iter().enumerate() {
        print!("{:>9}", label(*id));
        for (_, rows) in &columns {
            print!("  {:>12}", fmt_dur(Duration::from_nanos(rows[i].1 as u64)));
        }
        for (_, rows) in columns.iter().skip(1) {
            print!("  {:>7.2}x", *base as f64 / rows[i].1 as f64);
        }
        println!();
    }
    println!(
        "\nSHARED MACHINE -- these numbers are indicative, not a measurement of record.\n\
         Column t=0 is the pre-parallel code path in the same binary."
    );
}
