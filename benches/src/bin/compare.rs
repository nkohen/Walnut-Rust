// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **The head-to-head: `wr-cli` in-process vs a warm `walnut-java` JVM** (Phase 4, U32).
//!
//! ```bash
//! cargo run -p wr-bench --release --bin compare
//! WR_BENCH_ONLY=637,286 cargo run -p wr-bench --release --bin compare
//! ```
//!
//! The table this prints (also written to `target/bench-report.txt`) is U32's deliverable; the
//! checked-in copy of the last run is `benches/STATUS.md`.
//!
//! # What makes the comparison fair
//!
//! * **One methodology, both engines.** Each workload gets the same `warmup` throwaway
//!   iterations and the same `measure` timed ones, and each timed iteration is one
//!   `dispatchForIntegrationTest` over an already-built session tree. Neither side is measured
//!   cold: the JVM has run the identical command several times in the same process before the
//!   clock starts (JIT warm-up), and the Rust side is a `--release` binary that has done the
//!   same. Process startup appears in neither number.
//! * **Equivalent per-session caching.** Java keeps its number systems in a JVM-global static,
//!   so a `new Prover()` there is nearly free; the port keeps the same cache on the `Session`
//!   its `Prover` owns, so the Rust column uses one session-lifetime `Prover` (what the real
//!   REPL does). See [`wr_bench::RustEngine`]'s docs — getting this wrong charges the port for
//!   a cache Java never rebuilds. `WR_BENCH_COLD=1` measures the other way, for diagnosis only.
//! * **Timed on its own side of the pipe.** The JVM times with `System.nanoTime()` *inside*
//!   the driver and reports per-iteration nanoseconds, so the pipe round trip and the
//!   automaton serialization are outside the measured region on both sides.
//! * **Same command, same state.** Both engines replay Walnut's own
//!   `IntegrationTest.initialize` prelude into their own byte-identical copy of the corpus's
//!   library trees, then dispatch the fixture's literal command script — metacommand prefix and
//!   `::` suffix included.
//! * **Checked before believed.** Every workload's answer is compared across the two engines by
//!   `wr_core::equiv` semantic language equivalence *before* any timing is reported. A
//!   divergence aborts the run: a benchmark of two engines computing different things is worse
//!   than no benchmark.
//! * **Peak state count, not just wall clock.** `CLAUDE.md` names state blow-up as the dominant
//!   cost axis. Both peaks are read out of each engine's own `details` trace by one parser, in
//!   a separate untimed pass.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use wr_bench::{
    detail_variant, fmt_dur, golden, peak_states, same_answer, workloads, JavaEngine, RustEngine,
    Stats, Workload,
};

/// How many throwaway iterations each engine runs before the clock starts. Three is enough for
/// the JVM's C2 compiler to have promoted the hot path on these workloads (the differential
/// harness measured Java's instantaneous rate plateauing well inside 10 iterations) without
/// making the ~7 s workloads take an extra half-minute.
const DEFAULT_WARMUP: usize = 3;

/// Timed iterations per workload, chosen from its rough cost so the whole run stays inside a
/// few minutes. Overridable wholesale with `WR_BENCH_ITERS`.
fn default_iters(approx_secs: f64) -> usize {
    if approx_secs < 0.05 {
        50
    } else if approx_secs < 1.0 {
        20
    } else {
        5
    }
}

/// The per-request cap on the JVM. Generous — this is a "never hang" guardrail
/// (`CLAUDE.md`'s test-performance discipline), not a performance budget: a workload that
/// blows it is reported as a failure of the run, never silently dropped.
fn java_deadline(approx_secs: f64, iterations: usize) -> Duration {
    Duration::from_secs_f64(60.0 + 20.0 * approx_secs.max(0.5) * iterations as f64)
}

struct Row {
    workload: Workload,
    iterations: usize,
    rust: Stats,
    java: Stats,
    rust_peak: Option<u64>,
    java_peak: Option<u64>,
    answer_kind: &'static str,
    /// Whether the port's answer here equals the automaton `walnut-java` recorded for this
    /// fixture — a harness-fidelity check, see `benches/STATUS.md`.
    recorded: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("\nBENCHMARK FAILED: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = golden::corpus_root().ok_or_else(|| {
        format!(
            "the benchmark corpus was not found.\n\
             Looked for `src/test/resources/integrationTests/` under {}.\n\
             The workloads are real fixtures from the sibling `walnut-java` oracle repo and are\n\
             deliberately not vendored here; point WALNUT_JAVA_DIR at a walnut-java checkout.",
            golden::walnut_java_dir().display()
        )
    })?;

    let only: Option<Vec<usize>> = std::env::var("WR_BENCH_ONLY").ok().map(|s| {
        s.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().parse().expect("WR_BENCH_ONLY: not a fixture id"))
            .collect()
    });
    let forced_iters: Option<usize> = std::env::var("WR_BENCH_ITERS")
        .ok()
        .map(|s| s.trim().parse().expect("WR_BENCH_ITERS: not a number"));
    let warmup: usize = std::env::var("WR_BENCH_WARMUP")
        .ok()
        .map(|s| s.trim().parse().expect("WR_BENCH_WARMUP: not a number"))
        .unwrap_or(DEFAULT_WARMUP);

    let cold = std::env::var("WR_BENCH_COLD").is_ok_and(|v| v != "0");
    if cold {
        eprintln!(
            "WR_BENCH_COLD: the Rust column builds a FRESH Prover per iteration. This is a \n\
             diagnostic mode, not the fair comparison -- see benches/README.md."
        );
    }

    let all = workloads()?;
    let mut selected: Vec<Workload> = all
        .into_iter()
        // Not `is_none_or`: that is stable only since 1.82 and the workspace declares
        // `rust-version = "1.75"`.
        .filter(|w| match &only {
            None => true,
            Some(ids) => ids.contains(&w.id),
        })
        .collect();
    // Opt-in extra row: fixture 637's query with the strategy metacommand removed. Off by
    // default because it is not a corpus fixture and it is minutes long on both engines.
    if std::env::var("WR_BENCH_SC_VARIANT").is_ok_and(|v| v != "0") {
        selected.push(Workload {
            // Not a manifest id; `usize::MAX` keeps it out of the `WR_BENCH_ONLY` id space and
            // makes it obvious in the report that this row is not fixture-numbered.
            id: usize::MAX,
            command: wr_bench::SC_VARIANT.0.to_string(),
            why: wr_bench::SC_VARIANT.1,
            // A deliberately huge budget: this is the one workload expected to take minutes.
            approx_secs: 600.0,
        });
    }
    if selected.is_empty() {
        return Err("WR_BENCH_ONLY selected no workloads".to_string());
    }

    // Two SEPARATE copies of the corpus's library trees: both engines write library files as
    // they run, and sharing one tree would let each engine read the other's output.
    let scratch = std::env::temp_dir().join(format!("wr-bench-{}", std::process::id()));
    let rust_tree = scratch.join("rust");
    let java_tree = scratch.join("java");

    eprintln!("preparing the Rust engine (session tree + 19-command prelude) ...");
    let t0 = Instant::now();
    let rust = RustEngine::prepare(&root, &rust_tree)?;
    eprintln!("  ready in {:.1}s", t0.elapsed().as_secs_f64());

    eprintln!("preparing the JVM engine (compile driver, start JVM, replay prelude) ...");
    let t0 = Instant::now();
    let (java_home, java_session) = golden::build_session_tree(&root, &java_tree)
        .map_err(|e| format!("building the JVM's session tree: {e}"))?;
    let mut java = JavaEngine::start(&scratch, &java_session, &java_home, 4096)?;
    for (i, command) in golden::PRELUDE.iter().enumerate() {
        java.bench(command, 0, 1, Duration::from_secs(120))
            .map_err(|e| format!("JVM prelude[{i}] `{command}`: {e}"))?;
    }
    eprintln!("  ready in {:.1}s", t0.elapsed().as_secs_f64());

    let mut rows = Vec::with_capacity(selected.len());
    for w in selected {
        let iterations = forced_iters.unwrap_or_else(|| default_iters(w.approx_secs));
        let deadline = java_deadline(w.approx_secs, iterations + warmup);
        eprintln!(
            "\nfixture {:>3}  ({} warm-up + {} timed iterations per engine)\n  {}",
            label(w.id),
            warmup,
            iterations,
            truncate(&w.command, 100)
        );

        // -- 1. correctness, before any timing is believed ---------------------
        let rust_answer = rust.dispatch(&w.command)?;
        let java_answer = java
            .bench(&w.command, 0, 1, deadline)
            .map_err(|e| format!("fixture {}: {e}", w.id))?
            .answer;
        same_answer(&java_answer, &rust_answer).map_err(|e| {
            format!(
                "fixture {}: the two engines do not agree, so their timings are meaningless: {e}\n\
                 command: {}",
                w.id, w.command
            )
        })?;
        // ... and, separately, against the automaton `walnut-java` RECORDED for this fixture.
        // This is a check on the HARNESS, not on either engine: it proves the prelude-only
        // session state this benchmark runs in reproduces the corpus's own workload, rather
        // than some easier query that happens to agree between the two engines. Reported, not
        // fatal — see `benches/STATUS.md` §"Fidelity to the recorded corpus".
        let recorded = match &rust_answer {
            _ if w.id == usize::MAX => "n/a (not a corpus fixture)".to_string(),
            wr_bench::Answer::Automaton(_) => {
                let path = root.join(format!("automaton{}.txt", w.id));
                match std::fs::read_to_string(&path) {
                    Ok(txt) => match same_answer(&wr_bench::Answer::Automaton(txt), &rust_answer) {
                        Ok(()) => "matches the recorded corpus automaton".to_string(),
                        Err(e) => format!("DIFFERS from the recorded corpus automaton ({e})"),
                    },
                    Err(_) => "no recorded automaton on disk".to_string(),
                }
            }
            _ => "n/a (not an automaton result)".to_string(),
        };
        eprintln!("  vs corpus: {recorded}");

        let answer_kind = match rust_answer {
            wr_bench::Answer::Automaton(_) => "automaton",
            wr_bench::Answer::True => "TRUE",
            wr_bench::Answer::False => "FALSE",
            wr_bench::Answer::Error(_) => "error",
            wr_bench::Answer::None => "none",
        };
        eprintln!("  answers agree ({answer_kind}, semantic equivalence)");

        // -- 2. peak state count, untimed --------------------------------------
        let detail_cmd = detail_variant(&w.command);
        let rust_peak = peak_states(&rust.details(&detail_cmd)?);
        let java_peak = peak_states(
            &java
                .bench(&detail_cmd, 0, 1, deadline)
                .map_err(|e| format!("fixture {} (detail pass): {e}", w.id))?
                .details,
        );
        eprintln!(
            "  peak states: rust {}  java {}",
            rust_peak.map_or("?".to_string(), |n| n.to_string()),
            java_peak.map_or("?".to_string(), |n| n.to_string())
        );

        // -- 3. the timings, one engine at a time (never concurrently) ---------
        // `WR_BENCH_COLD=1` swaps the Rust column to a fresh `Prover` per iteration. That is
        // deliberately NOT the default (see `RustEngine`'s docs: Java keeps the equivalent
        // cache in a JVM-global static, so it would be an unfair comparison) -- it exists to
        // quantify the session-cache effect, which is also what makes `tests/golden`'s
        // per-fixture times much larger than this benchmark's for the same query.
        let rust_samples = if cold {
            rust.bench_cold(&w.command, warmup, iterations)
        } else {
            rust.bench(&w.command, warmup, iterations)
        };
        let rust_stats = Stats::of(&rust_samples).ok_or("no Rust samples")?;
        eprintln!(
            "  rust: mean {}  median {}",
            fmt_dur(rust_stats.mean),
            fmt_dur(rust_stats.median)
        );
        let java_reply = java
            .bench(&w.command, warmup, iterations, deadline)
            .map_err(|e| format!("fixture {}: {e}", w.id))?;
        let java_stats = Stats::of(&java_reply.samples).ok_or("no Java samples")?;
        eprintln!(
            "  java: mean {}  median {}",
            fmt_dur(java_stats.mean),
            fmt_dur(java_stats.median)
        );

        rows.push(Row {
            workload: w,
            iterations,
            rust: rust_stats,
            java: java_stats,
            rust_peak,
            java_peak,
            answer_kind,
            recorded,
        });
    }

    let report = render(&rows, warmup, cold);
    println!("\n{report}");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/bench-report.txt");
    match std::fs::write(&out, &report) {
        Ok(()) => eprintln!("report written to {}", out.display()),
        Err(e) => eprintln!("(could not write {}: {e})", out.display()),
    }

    // The engines are dropped (JVM killed) before the trees are removed.
    drop(java);
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

fn render(rows: &[Row], warmup: usize, cold: bool) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "== U32: walnut-rs vs walnut-java, {} workloads, {warmup} warm-up iterations each ==\n",
        rows.len()
    );
    if cold {
        let _ = writeln!(
            s,
            "!! WR_BENCH_COLD: the Rust column rebuilt its Prover (and its number-system cache)\n\
             !! on every iteration, which Java's JVM-global static cache never does. DIAGNOSTIC\n\
             !! MODE -- not the fair comparison.\n"
        );
    }
    let _ = writeln!(
        s,
        "peak states = the largest automaton named in the JVM's COMPLETE `details` trace.\n\
         rust trace  = the largest the port's PARTIAL trace names (a lower bound -- the port\n\
         \x20             does not yet thread `Logging` through wr-core; see benches/README.md)."
    );
    let _ = writeln!(
        s,
        "\n{:<5} {:>5} {:>12} {:>12} {:>12} {:>12} {:>9} {:>12} {:>11}",
        "fix",
        "iters",
        "rust mean",
        "rust median",
        "java mean",
        "java median",
        "speedup",
        "peak states",
        "rust trace"
    );
    let _ = writeln!(s, "{}", "-".repeat(105));
    for r in rows {
        let speedup = r.java.mean.as_secs_f64() / r.rust.mean.as_secs_f64();
        let _ = writeln!(
            s,
            "{:<5} {:>5} {:>12} {:>12} {:>12} {:>12} {:>8.2}x {:>12} {:>11}",
            label(r.workload.id),
            r.iterations,
            fmt_dur(r.rust.mean),
            fmt_dur(r.rust.median),
            fmt_dur(r.java.mean),
            fmt_dur(r.java.median),
            speedup,
            r.java_peak.map_or("?".to_string(), |n| n.to_string()),
            r.rust_peak.map_or("?".to_string(), |n| n.to_string()),
        );
    }

    let _ = writeln!(s, "\n-- per-workload detail --");
    for r in rows {
        let speedup = r.java.mean.as_secs_f64() / r.rust.mean.as_secs_f64();
        let verdict = if speedup >= 1.10 {
            format!("RUST FASTER ({speedup:.2}x)")
        } else if speedup <= 0.91 {
            format!("RUST SLOWER ({:.2}x slower)", 1.0 / speedup)
        } else {
            format!("COMPARABLE ({speedup:.2}x)")
        };
        let _ = writeln!(s, "\nfixture {} -- {verdict}", label(r.workload.id));
        let _ = writeln!(s, "  command : {}", r.workload.command);
        let _ = writeln!(s, "  why     : {}", r.workload.why);
        let _ = writeln!(s, "  answer  : {} (engines agree)", r.answer_kind);
        let _ = writeln!(s, "  corpus  : {}", r.recorded);
        let _ = writeln!(
            s,
            "  rust    : mean {}  median {}  min {}  max {}  (n={})",
            fmt_dur(r.rust.mean),
            fmt_dur(r.rust.median),
            fmt_dur(r.rust.min),
            fmt_dur(r.rust.max),
            r.rust.n
        );
        let _ = writeln!(
            s,
            "  java    : mean {}  median {}  min {}  max {}  (n={})",
            fmt_dur(r.java.mean),
            fmt_dur(r.java.median),
            fmt_dur(r.java.min),
            fmt_dur(r.java.max),
            r.java.n
        );
        let peaks = match (r.java_peak, r.rust_peak) {
            (Some(j), Some(p)) if j == p => {
                format!("{j} states (both traces name the same largest automaton)")
            }
            (Some(j), Some(p)) => format!(
                "{j} states (JVM, complete trace); the port's partial trace names {p} \
                 -- a logging gap, not a different computation"
            ),
            (j, p) => format!(
                "java {} / rust {}",
                j.map_or("?".to_string(), |n| n.to_string()),
                p.map_or("?".to_string(), |n: u64| n.to_string())
            ),
        };
        let _ = writeln!(s, "  peak    : {peaks}");
    }

    let slower: Vec<&Row> = rows
        .iter()
        .filter(|r| r.java.mean.as_secs_f64() / r.rust.mean.as_secs_f64() <= 0.91)
        .collect();
    let _ = writeln!(s, "\n-- verdict --");
    if slower.is_empty() {
        let _ = writeln!(
            s,
            "The Rust port is faster than (or comparable to) JVM Walnut on every workload."
        );
    } else {
        let _ = writeln!(
            s,
            "The Rust port is SLOWER on {} of {} workloads: {}",
            slower.len(),
            rows.len(),
            slower
                .iter()
                .map(|r| label(r.workload.id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    s
}

/// A workload's name in the report: its fixture id, or `sc637` for the opt-in non-fixture row.
fn label(id: usize) -> String {
    if id == usize::MAX {
        "sc637".to_string()
    } else {
        id.to_string()
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}
