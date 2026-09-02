// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! TEMPORARY measurement scaffold (branch `agent/par-det`): how much genuine subtree
//! independence exists in the eval DAG of each benchmark workload?
//!
//! Reads the `WR_DAG_TRACE` file that `wr_logic::eval::compute_with_ctx` writes when that
//! env var is set (one line per `compute` call, `arity:nanos` per postorder token), rebuilds
//! the expression tree from the arities, and reports:
//!
//! * total work (sum of all token times),
//! * the **critical path** (the longest root-to-leaf chain by time, i.e. the wall clock an
//!   infinitely-wide machine would still have to pay),
//! * the resulting Amdahl ceiling `total / critical`,
//! * the maximum number of tokens simultaneously ready (the raw "width").
//!
//! **The `WR_DAG_TRACE` writer is not in the tree.** It is a five-line, env-gated patch to
//! `compute_with_ctx`, deliberately reverted after the study so `wr-logic` carries no
//! measurement code; recover it with
//! `git show 71f1bd0 -- crates/wr-logic/src/eval.rs`. This binary is kept because the
//! measurement is worth being able to repeat, not because it runs as-is.

use std::path::Path;

use wr_bench::{golden, heavy_workloads, workloads, RustEngine};

struct Node {
    arity: usize,
    nanos: u128,
    children: Vec<usize>,
}

/// Rebuilds the postfix stream into a forest; returns (nodes, roots).
fn build(tokens: &[(usize, u128)]) -> (Vec<Node>, Vec<usize>) {
    let mut nodes: Vec<Node> = Vec::with_capacity(tokens.len());
    let mut stack: Vec<usize> = Vec::new();
    for &(arity, nanos) in tokens {
        let take = arity.min(stack.len());
        let children: Vec<usize> = stack.split_off(stack.len() - take);
        nodes.push(Node {
            arity,
            nanos,
            children,
        });
        stack.push(nodes.len() - 1);
    }
    (nodes, stack)
}

/// Longest-path-to-completion for every node (its own cost plus the max over its children).
fn finish_times(nodes: &[Node]) -> Vec<u128> {
    let mut f = vec![0u128; nodes.len()];
    // Postfix order guarantees every child index is < its parent's, so one forward pass works.
    for i in 0..nodes.len() {
        let deepest = nodes[i].children.iter().map(|&c| f[c]).max().unwrap_or(0);
        f[i] = deepest + nodes[i].nanos;
    }
    f
}

/// Greedy level-by-level schedule with unbounded workers: how many nodes are ever ready at once.
fn max_ready(nodes: &[Node]) -> usize {
    let mut depth = vec![0usize; nodes.len()];
    let mut by_depth = std::collections::BTreeMap::<usize, usize>::new();
    for i in 0..nodes.len() {
        depth[i] = nodes[i]
            .children
            .iter()
            .map(|&c| depth[c] + 1)
            .max()
            .unwrap_or(0);
        *by_depth.entry(depth[i]).or_default() += 1;
    }
    by_depth.values().copied().max().unwrap_or(0)
}

fn ms(n: u128) -> f64 {
    n as f64 / 1e6
}

/// `WR_DAG_CORPUS=1`: replay every golden fixture in id order through one session (the
/// Tier-1 harness's own shape) and report the work-weighted DAG shape of every `compute`
/// call the whole corpus makes — so the ceiling finding rests on the corpus, not on the
/// 14 hand-picked benchmark rows.
fn corpus_sweep(root: &Path) {
    let tmp = std::env::temp_dir().join("wr-dagwidth-corpus");
    let _ = std::fs::remove_dir_all(&tmp);
    let trace = tmp.join("trace.txt");
    std::fs::create_dir_all(&tmp).unwrap();
    let engine = RustEngine::prepare(root, &tmp.join("session")).expect("session");

    let fixtures = golden::load_fixtures().expect("fixtures");
    std::env::set_var("WR_DAG_TRACE", &trace);
    let mut per_fixture: Vec<(usize, Vec<(usize, u128)>)> = Vec::new();
    for f in &fixtures {
        let before = std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0);
        let _ = engine.dispatch(&f.command_script);
        let text = std::fs::read_to_string(&trace).unwrap_or_default();
        for line in text[before as usize..]
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let toks: Vec<(usize, u128)> = line
                .split(',')
                .filter_map(|t| {
                    let (a, n) = t.split_once(':')?;
                    Some((a.parse().ok()?, n.parse().ok()?))
                })
                .collect();
            if !toks.is_empty() {
                per_fixture.push((f.id, toks));
            }
        }
    }
    std::env::remove_var("WR_DAG_TRACE");

    let mut rows: Vec<(usize, usize, u128, u128, usize, f64)> = Vec::new();
    for (id, toks) in &per_fixture {
        let (nodes, roots) = build(toks);
        let f = finish_times(&nodes);
        let total: u128 = toks.iter().map(|t| t.1).sum();
        let crit = roots.iter().map(|&r| f[r]).max().unwrap_or(0);
        let ceiling = if crit == 0 {
            1.0
        } else {
            total as f64 / crit as f64
        };
        rows.push((*id, nodes.len(), total, crit, max_ready(&nodes), ceiling));
    }
    println!("compute() calls traced: {}", rows.len());

    // The only rows that could matter: those with enough absolute work to be worth
    // parallelizing at all. A 2x ceiling on a 0.2 ms query buys nothing.
    for floor_ms in [0.0f64, 1.0, 10.0, 100.0] {
        let sel: Vec<_> = rows.iter().filter(|r| ms(r.2) >= floor_ms).collect();
        if sel.is_empty() {
            println!("work >= {floor_ms:>6.0} ms:  (none)");
            continue;
        }
        let best = sel
            .iter()
            .fold(sel[0], |a, b| if b.5 > a.5 { b } else { a });
        let over_11 = sel.iter().filter(|r| r.5 >= 1.1).count();
        let work: f64 = sel.iter().map(|r| ms(r.2)).sum();
        let crit: f64 = sel.iter().map(|r| ms(r.3)).sum();
        println!(
            "work >= {floor_ms:>6.0} ms:  n={:<5} aggregate ceiling={:.3}x  best={:.2}x (fixture {})  n(ceiling>=1.1x)={over_11}",
            sel.len(),
            work / crit,
            best.5,
            best.0,
        );
    }

    rows.sort_by_key(|r| std::cmp::Reverse(r.2));
    println!(
        "\nthe 15 costliest compute() calls in the corpus:\n{:>8}  {:>6}  {:>10}  {:>10}  {:>8}  {:>5}",
        "fixture", "tokens", "work(ms)", "crit(ms)", "ceiling", "width"
    );
    for r in rows.iter().take(15) {
        println!(
            "{:>8}  {:>6}  {:>10.2}  {:>10.2}  {:>7.3}x  {:>5}",
            r.0,
            r.1,
            ms(r.2),
            ms(r.3),
            r.5,
            r.4
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

fn main() {
    let root = golden::corpus_root().expect("corpus root (set WALNUT_JAVA_DIR)");
    if std::env::var("WR_DAG_CORPUS").is_ok() {
        corpus_sweep(&root);
        return;
    }
    let tmp = std::env::temp_dir().join("wr-dagwidth");
    let _ = std::fs::remove_dir_all(&tmp);
    let trace = tmp.join("trace.txt");
    std::fs::create_dir_all(&tmp).unwrap();

    let engine = RustEngine::prepare(&root, &tmp.join("session")).expect("session");

    let mut all: Vec<_> = workloads().expect("workloads");
    all.extend(heavy_workloads());
    let only: Option<Vec<usize>> = std::env::var("WR_BENCH_ONLY").ok().map(|s| {
        s.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().parse().unwrap())
            .collect()
    });

    println!(
        "{:>9}  {:>6}  {:>10}  {:>10}  {:>7}  {:>5}  {:>5}",
        "workload", "tokens", "work(ms)", "crit(ms)", "ceiling", "width", "roots"
    );
    for w in &all {
        if let Some(ids) = &only {
            if !ids.contains(&w.id) {
                continue;
            }
        }
        let _ = std::fs::remove_file(&trace);
        std::env::set_var("WR_DAG_TRACE", &trace);
        let answer = engine.dispatch(&w.command);
        std::env::remove_var("WR_DAG_TRACE");
        if let Err(e) = answer {
            println!("{:>9}  ERROR {e}", wr_bench::label(w.id));
            continue;
        }
        let text = std::fs::read_to_string(&trace).unwrap_or_default();
        // A command can run `compute` more than once (e.g. a macro); take the costliest line.
        let mut best: Option<(Vec<(usize, u128)>, u128)> = None;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let toks: Vec<(usize, u128)> = line
                .split(',')
                .filter_map(|t| {
                    let (a, n) = t.split_once(':')?;
                    Some((a.parse().ok()?, n.parse().ok()?))
                })
                .collect();
            let total: u128 = toks.iter().map(|t| t.1).sum();
            if best.as_ref().map_or(true, |(_, b)| total > *b) {
                best = Some((toks, total));
            }
        }
        let Some((toks, total)) = best else {
            println!("{:>9}  (no trace)", wr_bench::label(w.id));
            continue;
        };
        let (nodes, roots) = build(&toks);
        let f = finish_times(&nodes);
        let crit = roots.iter().map(|&r| f[r]).max().unwrap_or(0);
        println!(
            "{:>9}  {:>6}  {:>10.2}  {:>10.2}  {:>6.2}x  {:>5}  {:>5}",
            wr_bench::label(w.id),
            nodes.len(),
            ms(total),
            ms(crit),
            if crit == 0 {
                0.0
            } else {
                total as f64 / crit as f64
            },
            max_ready(&nodes),
            roots.len(),
        );
        // The top few costliest tokens and where they sit, for a feel of the shape.
        if std::env::var("WR_DAG_VERBOSE").is_ok() {
            let mut idx: Vec<usize> = (0..nodes.len()).collect();
            idx.sort_by_key(|&i| std::cmp::Reverse(nodes[i].nanos));
            for &i in idx.iter().take(6) {
                println!(
                    "        #{i} arity={} self={:.2}ms finish={:.2}ms",
                    nodes[i].arity,
                    ms(nodes[i].nanos),
                    ms(f[i])
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = Path::new(".");
}
