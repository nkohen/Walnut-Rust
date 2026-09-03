# Backlog: tune P5's parallel worker-count default (the deferred D3 sweep)

**Status: OPEN. Owed since P5 shipped (2026-09-03) with a provisional value.**

## What is unfinished

P5 (`perf/beyond`) parallelized `subset_construction` with a `std::thread::scope` design
whose total parallel degree defaults to `min(available_parallelism, DEFAULT_MAX_THREADS)`
with `DEFAULT_MAX_THREADS = 7` (`crates/wr-core/src/parallel.rs`). **That `7` is a
placeholder, not a measured value.** The P5 plan
(`~/.claude/plans/perf-beyond-p5-parallel-promotion.md`, deliverable D3) pre-registered a
worker-count sweep — measure the benchmark set at total degrees **2 / 4 / 6 / max** on a
quiet, AC-powered machine, min-of-2-medians, and set the shipped default from the data. P5
landed before that sweep could run because the machine would not stay quiet long enough
(the same environment problem that cost the campaign baseline three attempts — see
`benches/baseline-perf-campaign.txt` and the `bench-requires-quiet-machine` memory).

## Why it was safe to ship without it

The value is a **tuning knob, not a correctness parameter.** Every degree in
`[1, HARD_MAX_THREADS]` produces **bit-identical output** — proven by the cross-process
determinism gate (`P5-DIGEST` identical across `WR_CORE_THREADS` = 1/2/4/6/1024/default) and
the 50k differential soak. A wrong default can only cost some performance, never a wrong
answer. The 4P+4E core topology and P5's own review both hint the optimum may be nearer 4
than 7 (the E-cores are not P-cores; `agent/par-max`'s thread sweep was inconclusive under
contention), which is exactly what the sweep is for.

## How to do it (when a quiet machine is available)

1. Check fitness first (`uptime`; `ps -Ao pcpu,comm -r | head`; `pmset -g batt` — load < ~2.5,
   AC power, no foreign CPU-pinned process). The `bench-requires-quiet-machine` discipline.
2. For each total degree D in {2, 4, 6, `available_parallelism`}: run
   `WR_CORE_THREADS=D WR_BENCH_HEAVY=1 cargo run -p wr-bench --release --bin compare` (default
   rows + the three heavy rows), min-of-2-medians, interleaved across D to spread drift.
3. Pick the D with the best geomean over the engine-bound + heavy rows that does not regress
   any fixture; set `DEFAULT_MAX_THREADS` to it, update the doc comment to cite the measured
   run (date, numbers), and delete the "PROVISIONAL" wording.
4. Re-run the fast-tier `cargo test -p wr-core` (the value change touches only the default;
   the determinism gate already covers every degree) and commit.

No adversarial-review round is needed for a single-constant tuning change backed by the
sweep numbers, provided the determinism gate is re-run at the new default.

## Pointers

- Code: `crates/wr-core/src/parallel.rs` — `DEFAULT_MAX_THREADS`, `resolve_threads`.
- Plan: `~/.claude/plans/perf-beyond-p5-parallel-promotion.md` §D3.
- Gate script: session scratchpad `p5-impl/p5-thread-gate.sh` (the determinism digest sweep).
