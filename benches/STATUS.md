# U32 — performance vs JVM Walnut: status

Snapshot of the last full `cargo run -p wr-bench --release --bin compare` run. Update it
whenever the numbers move. This is the U32 companion to `tests/golden/STATUS.md` and
`tests/differential-gen/STATUS.md`.

**Read [`README.md`](README.md) first.** It documents the methodology — what is warm, what is
timed, why the Rust column uses one session-lifetime `Prover`, why these eleven workloads, and
the one honest caveat about the peak-state column. The numbers below mean very little without
it.

## Headline

Measured 2026-08-16, Apple Silicon (darwin 24.6.0, macOS 15.7.3, ARM64), `--release`, JDK 17
(`-Xmx4096m`), `walnut-java` `Walnut-all.jar`. Both engines warm; process startup is in neither
number. Every workload's answer was checked to agree across the two engines by
`wr_core::equiv` semantic language equivalence **before** its timing was recorded, and every
automaton-valued one was additionally checked against the automaton `walnut-java` itself
recorded for that fixture.

```
fix   iters    rust mean  rust median    java mean  java median   speedup  peak states  rust trace
---------------------------------------------------------------------------------------------------------
1        50   397.382 µs   394.416 µs   688.856 µs   660.896 µs     1.73x            6           6
207      50   305.310 µs   303.625 µs   411.857 µs   409.396 µs     1.35x            3           3
293      20   110.022 ms   110.026 ms    77.215 ms    75.816 ms     0.70x        12334         129
521      20    79.408 ms    79.173 ms    48.054 ms    47.866 ms     0.61x            3           2
179      20   440.264 ms   439.811 ms   301.500 ms   300.227 ms     0.68x        33000         248
266      20   111.878 ms   111.671 ms    85.444 ms    85.035 ms     0.76x         7657         608
230       5      2.202 s      2.183 s      1.692 s      1.690 s     0.77x       115802         152
295      20   181.619 ms   180.231 ms   138.742 ms   138.863 ms     0.76x        11589        1361
261      20   309.853 ms   306.211 ms   231.937 ms   231.339 ms     0.75x        18674        1510
286      20   495.678 ms   493.230 ms   385.761 ms   384.869 ms     0.78x        34988        1818
637      20    91.746 ms    91.630 ms    61.277 ms    60.784 ms     0.67x         4965        1790
```

`speedup` is `java mean / rust mean`: above 1 means the Rust port is faster. `peak states` is the
largest automaton named in the JVM's **complete** `details` trace; `rust trace` is the largest the
port's **partial** trace names — a lower bound, not a different computation (see
§"The peak-state column" below).

**The verdict is mixed, and the interesting half is the bad half:**

| | |
|---|---|
| Rust faster | **2 of 11** — the two sub-millisecond workloads (1.35×–1.73×) |
| Rust slower | **9 of 11** — every workload where the decision procedure dominates (1.28×–1.65× slower) |
| answers disagreeing between the engines | **0** |
| workloads whose answer differs from the recorded corpus automaton | **0** |

## What that actually says

The two results are consistent with each other and say two different things:

* **Per-command overhead is lower in the port.** Fixtures 1 and 207 are dominated by parse,
  dispatch and small-automaton construction; the port is 1.35–1.73× faster there, which is the
  no-JVM, no-JIT-warm-up, no-GC advantage you would expect.
* **Algorithmic throughput is *worse* in the port, by a fairly flat ~1.3×.** Every workload above
  ~50 ms — cross products, subset construction, minimization — runs slower than JVM Walnut. The
  factor is remarkably stable (0.68–0.78 across seven very different queries), which points at a
  systematic per-operation cost rather than any one bad algorithm.

**`docs/DESIGN.md` §8's Phase-4 exit clause — "faster than Walnut on the research workloads" —
is therefore NOT met**, and this file records that plainly rather than reporting the two
favourable rows. See §"Root cause" for the measured reason and §"What would close the gap" for
what it would take. The correctness half of Phase 4's exit criterion (Tier-3 at 10⁵ with zero
divergences, Tier-4 green, Tier-5 clean) is unaffected and remains met; this is a performance
gap in a port that is, by every other tier's measure, correct.

## The strategy metacommand: what it buys (opt-in `sc637` row)

Fixture 637's formula, decided by **plain subset construction** — i.e. with `[strategy 6 BRZ]`
removed. Run with `WR_BENCH_SC_VARIANT=1 WR_BENCH_ONLY=99999 WR_BENCH_ITERS=2 WR_BENCH_WARMUP=1`.

```
fix   iters    rust mean  rust median    java mean  java median   speedup  peak states  rust trace
sc637     2     46.786 s     46.786 s     23.686 s     23.686 s     0.51x       155153        1790
```

| | with `[strategy 6 BRZ]` | plain `SC` | factor |
|---|---:|---:|---:|
| walnut-rs | 91.7 ms | 46.79 s | **510×** |
| walnut-java | 61.3 ms | 23.69 s | **387×** |
| peak states (JVM trace) | 4,965 | 155,153 | **31×** |

This is the payoff from U32's prerequisite unit (wiring `[strategy N NAME]` end-to-end) stated as
a measurement: on this query the metacommand is worth two and a half orders of magnitude, and
before that unit landed the port could not honour it at all. It is also the widest Rust-vs-Java
gap in the whole set (1.98× slower), which fits the root cause below — subset construction on a
1,790-state NFA is the most allocation-intensive thing either engine does.

## Root cause (best-effort, evidence-based)

A 20-second `sample(1)` profile of the Rust engine looping fixture 286, taking `sample`'s own
"sort by top of stack" summary and discarding the two blocked/idle frames (the JVM-pipe reader
thread's `read`, and `semaphore_timedwait_trap`), leaves 14,358 top-of-stack samples of real
work:

| bucket | samples | share |
|---|---:|---:|
| system allocator + `memmove`/`memset` (`tiny_malloc*`, `tiny_free*`, `szone_*`, `madvise`, …) | 7,394 | **51.5%** |
| `BTreeMap` node navigation (non-allocating) | 1,750 | 12.2% |
| engine code (`subset_construction`, `language_equivalent`, `Partition::mark`, `minimize`, …) | 5,214 | 36.3% |

So **over half of the port's CPU time on a representative workload is the macOS system
allocator**, and another eighth is walking B-tree nodes. That is a direct consequence of the
mechanical-port rule, not a bug: `wr_core::fa::Fa` stores transitions as
`Vec<BTreeMap<i32, Vec<usize>>>`, a faithful transliteration of Java's
`List<Int2ObjectRBTreeMap<IntList>>`, so every cross-product/determinize/minimize step allocates
and frees a large number of small, short-lived `Vec`s and B-tree nodes. The JVM services exactly
that pattern with a bump-pointer nursery allocator and a generational collector — the workload
its allocator is best at — while the port pays a general-purpose `malloc`/`free` round trip per
object. `CLAUDE.md`'s "mechanical port first, idiomatic Rust later in separate commits" is why
the port looks like this today; this measurement is the first concrete argument for scheduling
some of that "later".

This is a hypothesis supported by a profile, not a proven attribution — nobody has yet built the
counterfactual. See below.

## What would close the gap (not done here; U32 is measurement, not optimization)

In rough order of expected-return-per-risk, all of them **out of scope for this unit** (they
change `wr-core`, a trust-critical crate, and would each need the full two-reviewer loop):

1. **Swap the global allocator** (e.g. `mimalloc`/`jemalloc`). Zero algorithmic risk, one line,
   and it targets the 51.5% bucket directly. It is also the cheapest way to *test* the root-cause
   hypothesis above: if it moves the number a lot, the diagnosis is confirmed.
2. **Flatten the transition representation.** A sorted `Vec<(i32, SmallVec<[usize; 1]>)>` or a
   CSR-style arena per `Fa` would remove most of both the allocator and the B-tree-navigation
   buckets. Bigger change, real behavioural risk (iteration order is load-bearing in several
   ported algorithms — see `fa.rs`'s notes on `determine_permutation_map`), and it must not
   change any language.
3. **Reuse buffers across `act()` steps** instead of building a fresh map per state.
4. Build-profile tuning (`lto = "fat"`, `codegen-units = 1`). Untested here; the numbers above
   are the *default* `--release` profile, so this is free headroom of unknown size.

## The peak-state column

The port's peak is a **lower bound**, and the table labels it as such. Java's `details` trace
covers the whole computation; the port's covers only the `wr-logic`-level steps, because
threading `&mut Logging` through `wr-core`'s product/determinize/minimize/quantify is a known,
pre-existing gap — the same one behind seven of the golden corpus's nine remaining divergences
(`tests/golden/STATUS.md`, the "U28" follow-up). Both engines are separately proven to compute
the same language on every workload here, so the JVM's number characterises both computations.

Closing that gap would make this column a genuine two-engine comparison rather than one
authoritative number plus a lower bound, which is a second, independent reason to schedule the
U28 follow-up.

## Fidelity to the recorded corpus

Every automaton-valued workload's answer was compared against `automaton<id>.txt` as
`walnut-java` recorded it, and **all nine matched** (521 is `FALSE`, and `sc637` is not a corpus
fixture, so neither has a recorded automaton to compare). This is a check on the *harness*, not
on either engine: it proves the prelude-only session state this benchmark runs in reproduces the
corpus's own workloads, rather than some easier query the two engines happen to agree on.

## Criterion cross-check

`cargo bench -p wr-bench` measures the Rust column independently, through Criterion's own
sampling and analysis, calling the same `RustEngine::timed_dispatch`. Its point estimates agree
with the head-to-head's Rust means to within a few percent, which is the intended
cross-validation — two different timing harnesses, one measured quantity.

| fixture | Criterion (mean, 95% CI) | `compare` (mean) |
|---|---|---|
| 1 | 387.97 µs [380.83, 396.51] | 397.38 µs |
| 207 | 297.87 µs [293.25, 303.88] | 305.31 µs |
| 293 | 109.78 ms [109.00, 110.85] | 110.02 ms |
| 521 | 80.805 ms [80.327, 81.349] | 79.408 ms |
| 179 | 440.66 ms [437.41, 445.59] | 440.26 ms |
| 266 | 112.74 ms [112.20, 113.33] | 111.88 ms |
| 230 | 2.1626 s [2.1595, 2.1662] | 2.202 s |
| 295 | 181.79 ms [181.16, 182.57] | 181.62 ms |
| 261 | 299.83 ms [299.17, 300.72] | 309.85 ms |
| 286 | 492.50 ms [491.27, 493.98] | 495.68 ms |
| 637 | 90.901 ms [90.627, 91.232] | 91.746 ms |

## Reproducing

```bash
cargo run -p wr-bench --release --bin compare              # the table above
cargo bench -p wr-bench                                    # the Criterion column
WR_BENCH_SC_VARIANT=1 WR_BENCH_ONLY=99999 \
  WR_BENCH_ITERS=2 WR_BENCH_WARMUP=1 \
  cargo run -p wr-bench --release --bin compare            # the sc637 row (minutes)
WR_BENCH_COLD=1 WR_BENCH_ONLY=207 \
  cargo run -p wr-bench --release --bin compare            # the session-cache diagnostic
```

Every run also writes its report to `target/bench-report.txt`.

## Threats to validity, stated up front

* **One machine, one run each.** These are single-run means on one Apple Silicon laptop; nothing
  here is a cross-platform claim, and the µs-scale rows in particular carry real jitter (fixture
  1's Java max is 2.3× its min). The ~1.3× algorithmic gap is well outside that noise; the
  1.35× on fixture 207 is not far outside it.
* **`-Xmx4096m` on the JVM** — generous, and it favours *Java* (fewer collections), so it is a
  conservative setting for the conclusion actually drawn here.
* **The port's default `--release` profile** (no LTO, `codegen-units = 16`). See "What would
  close the gap" item 4.
* **The corpus is not a worst-case-blowup benchmark.** Walnut's own integration suite is designed
  to finish inside its `MAX_TOTAL_SECS = 1800`, so the heaviest ordinary workload here is ~2 s.
  The `sc637` row is the only genuinely large one, and it is the only one measured with n = 2.
