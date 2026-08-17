# Performance vs JVM Walnut: status

Snapshot of the last full `cargo run -p wr-bench --release --bin compare` run. Update it
whenever the numbers move. This is the U32 companion to `tests/golden/STATUS.md` and
`tests/differential-gen/STATUS.md`.

**Read [`README.md`](README.md) first.** It documents the methodology — what is warm, what is
timed, why the Rust column uses one session-lifetime `Prover`, why these eleven workloads, and
the one honest caveat about the peak-state column. The numbers below mean very little without
it.

This file now records **two** measurements, in chronological order:

* [§U33](#u33--the-allocator-swap--build-profile-tuning-current) — the current state, after the
  two zero-algorithm changes U32's own "what would close the gap" list ranked first;
* [§U32](#u32--the-original-baseline-default-release-profile-system-allocator) — the original
  baseline, kept verbatim so the delta is auditable rather than asserted.

---

# U33 — the allocator swap + build-profile tuning (current)

Two changes, neither of which touches a single line of decision-procedure code:

1. **`mimalloc` as the `#[global_allocator]`** — registered in `crates/wr-cli/src/main.rs` (the
   shipped binary) and `benches/src/lib.rs` (both `compare` and the Criterion bench). No
   library in the workspace registers one, so an embedder keeps its own choice.
2. **`[profile.release] lto = "fat"`, `codegen-units = 1`** in the root `Cargo.toml` —
   `[profile.bench]` inherits it, so `cargo bench` and `cargo test --release` pick it up too.

Measured 2026-08-17 on the same machine, same jar, same eleven workloads, same harness. Every
answer was re-checked by `wr_core::equiv` before its timing was believed, and all nine
automaton-valued workloads still match the recorded corpus automaton.

```
fix   iters    rust mean  rust median    java mean  java median   speedup  peak states  rust trace
---------------------------------------------------------------------------------------------------------
1        50   352.401 µs   343.291 µs   629.115 µs   612.521 µs     1.79x            6           6
207      50   365.027 µs   361.083 µs   417.818 µs   401.854 µs     1.14x            3           3
293      20    63.804 ms    63.779 ms    76.798 ms    76.096 ms     1.20x        12334         129
521      20    19.407 ms    19.330 ms    47.897 ms    47.440 ms     2.47x            3           2
179      20   285.454 ms   285.460 ms   298.183 ms   297.652 ms     1.04x        33000         248
266      20    70.480 ms    70.509 ms    83.740 ms    83.351 ms     1.19x         7657         608
230       5      1.612 s      1.611 s      1.671 s      1.673 s     1.04x       115802         152
295      20    99.745 ms    98.414 ms   134.968 ms   134.944 ms     1.35x        11589        1361
261      20   180.291 ms   179.974 ms   228.381 ms   229.136 ms     1.27x        18674        1510
286      20   285.813 ms   284.295 ms   376.730 ms   375.393 ms     1.32x        34988        1818
637      20    68.620 ms    68.561 ms    58.909 ms    58.809 ms     0.86x         4965        1790
```

| | U32 baseline | now |
|---|---|---|
| Rust faster | 2 of 11 | **10 of 11** |
| Rust slower | **9 of 11** (1.28×–1.65×) | **1 of 11** — fixture 637, 1.16× slower |
| answers disagreeing between the engines | 0 | **0** |
| workloads whose answer differs from the recorded corpus automaton | 0 | **0** |

**Honest reading of that table.** The 9-of-11-slower result is gone, but "faster than Walnut on
the research workloads" is *still not uniformly* true:

* **637 is still 1.16× slower** — the Brzozowski-strategy fixture, i.e. the most
  allocation-intensive determinization in the set. It improved 1.31× but did not overtake Java.
* **179 (1.04×) and 230 (1.04×) are ties, not wins.** They are inside this harness's own
  run-to-run spread; calling them Rust victories would be exactly the softening this project's
  discipline forbids.
* The genuine wins are 293/295/261/286/521 (1.20×–2.47×) and the two per-command-overhead rows.

So: **the gap is closed on most of the set and reversed on half of it, but DESIGN.md §8's clause
is met in spirit rather than across the board.** The remaining deficit is concentrated exactly
where U32's profile said it would be — the heaviest subset construction — and item 2 of "what
would close the gap" (flattening the transition representation) is still the change that would
address it.

## Attribution: which change bought what

Each configuration was measured as its own full `compare` run, so the two effects are separated
rather than conflated. The middle column is the *same source tree* built with
`CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, which reproduces the
default `--release` profile exactly.

| fix | U32 baseline | + mimalloc | + mimalloc + LTO | mimalloc alone | LTO increment | **total** |
|---:|---:|---:|---:|---:|---:|---:|
| 293 | 107.671 ms | 72.831 ms | 63.804 ms | 1.48× | 1.14× | **1.69×** |
| 521 | 78.058 ms | 21.571 ms | 19.407 ms | 3.62× | 1.11× | **4.02×** |
| 179 | 429.390 ms | 315.022 ms | 285.454 ms | 1.36× | 1.10× | **1.50×** |
| 266 | 110.000 ms | 77.304 ms | 70.480 ms | 1.42× | 1.10× | **1.56×** |
| 230 | 2.173 s | 1.735 s | 1.612 s | 1.25× | 1.08× | **1.35×** |
| 295 | 177.054 ms | 112.548 ms | 99.745 ms | 1.57× | 1.13× | **1.78×** |
| 261 | 292.377 ms | 209.752 ms | 180.291 ms | 1.39× | 1.16× | **1.62×** |
| 286 | 481.674 ms | 317.851 ms | 285.813 ms | 1.52× | 1.11× | **1.69×** |
| 637 | 90.166 ms | 73.872 ms | 68.620 ms | 1.22× | 1.08× | **1.31×** |

**The allocator is the whole story; the build profile is a consistent but small bonus.** mimalloc
alone is worth 1.22×–3.62× (median ~1.42×) on the nine decision-procedure workloads; `lto = "fat"`
+ `codegen-units = 1` adds a further 1.08×–1.16× on top, remarkably uniformly. Build cost of the
profile change: a from-scratch `--release` build of `wr-bench` went from ~7 s to ~25 s, which is
paid by `cargo bench`/`cargo run --release`/`cargo test --release` and **not** by
`cargo test --workspace` (a `dev`/`test`-profile build, untouched).

The two sub-millisecond rows (1, 207) are deliberately left out of that table: at ~300 µs they
carry more jitter than either change's effect. Re-measured at `WR_BENCH_ITERS=300`, the two
configurations are indistinguishable (fixture 1: 329 µs vs 315 µs median; fixture 207: 272 µs vs
277 µs median), and the 365 µs the main table shows for 207 is an artifact of that jitter, not a
regression — the U32 baseline's own fixture-1 row had the same problem in the other direction
(633 µs mean against a 386 µs median).

## The allocator hypothesis: confirmed

U32's root-cause section attributed 51.5% of the port's CPU time to the system allocator and
called the allocator swap "the cheapest way to *test* the root-cause hypothesis: if it moves the
number a lot, the diagnosis is confirmed." It moved the number a lot.

Same profile, taken the same way — a 20-second `sample(1)` of the Rust engine looping fixture
286, its "sort by top of stack" summary, the two blocked/idle frames (the JVM-pipe reader
thread's `read`, `semaphore_timedwait_trap`) discarded — leaving 14,853 top-of-stack samples of
real work:

| bucket | U32 baseline | now | |
|---|---:|---:|---|
| system/`mimalloc` allocator + `memmove`/`memset` | **51.5%** | **13.4%** | ↓ 3.8× as a share |
| `BTreeMap` node navigation (non-allocating) | 12.2% | 27.6% | ↑ (same absolute work, smaller denominator) |
| engine code (`subset_construction`, `Partition::mark`, `minimize`, …) | 36.3% | 59.0% | ↑ |

Since the workload also got 1.69× faster in wall clock, the *absolute* allocator time per
iteration fell from ~0.515 × 481.7 ms ≈ **248 ms** to ~0.134 × 285.8 ms ≈ **38 ms** — roughly a
6.5× reduction. The diagnosis was right: the port was not slow because of its algorithms, it was
slow because `Vec<BTreeMap<i32, Vec<usize>>>` asks a general-purpose `malloc` to do what a JVM
nursery does for free, and a nursery-shaped allocator recovers most of it.

Two caveats stated up front. (a) The after-profile is of the **combined** mimalloc + LTO binary,
so it is not a clean isolation of the allocator alone; it is, however, sound in its attribution,
because mimalloc's fast path lives in a separately-compiled C library that Rust LTO does not
inline into (`mi_malloc*`/`mi_free` remain distinct top-of-stack symbols). (b) The largest single
remaining frame is now `BTreeSet::insert` (3,576 samples, 24% of real work) inside
`subset_construction` — which is precisely candidate 2 below, and the reason it is still on the
list.

## Correctness, re-verified rather than assumed

An allocator swap and a codegen-flag change should be behaviourally invisible. They were checked
anyway:

| gate | result |
|---|---|
| `cargo fmt --all` / `cargo clippy --workspace --all-targets` | clean |
| `cargo test --workspace` (fast tier) | green, 1452 tests |
| Tier 1 golden corpus (`cargo test -p wr-golden --release -- --ignored`) | **577 pass / 9 known divergences / 89 excluded / 0 timeout / 0 not-run** — unchanged |
| Tier 3 differential-gen spot check (5,000 generated queries, fresh seed) | 5,000 match, 0 divergence, 0 skip |
| the benchmark's own per-workload `wr_core::equiv` answer check | agrees on all 11, and all 9 automaton-valued ones still match the recorded corpus automaton |

## Criterion cross-check (re-run)

`cargo bench -p wr-bench` on the new configuration. Its agreement with the `compare` column is
also the empirical proof that `[profile.bench]` really did inherit the new `[profile.release]`
settings — these are the LTO numbers, not the mimalloc-only ones.

| fixture | Criterion (mean, 95% CI) | `compare` (mean) |
|---|---|---|
| 1 | 326.98 µs [325.87, 328.44] | 352.40 µs |
| 207 | 278.50 µs [273.83, 285.00] | 365.03 µs (see the jitter note above) |
| 293 | 63.104 ms [62.447, 63.972] | 63.804 ms |
| 521 | 19.411 ms [19.386, 19.438] | 19.407 ms |
| 179 | 279.75 ms [279.31, 280.24] | 285.45 ms |
| 266 | 69.906 ms [69.710, 70.127] | 70.480 ms |
| 230 | 1.6199 s [1.6098, 1.6369] | 1.612 s |
| 295 | 97.619 ms [97.271, 97.982] | 99.745 ms |
| 261 | 180.45 ms [176.74, 185.94] | 180.29 ms |
| 286 | 285.62 ms [281.94, 290.75] | 285.81 ms |
| 637 | 68.798 ms [68.420, 69.407] | 68.620 ms |

---

# U32 — the original baseline (default release profile, system allocator)

*Everything below this line is the original U32 measurement, kept unmodified except for the two
"what would close the gap" items U33 executed, which are marked done in place.*

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
favourable rows. *(U33 has since moved this to 10-of-11-faster without touching an algorithm —
see the top of this file. The paragraph below is the state as U32 measured it.)* See §"Root cause" for the measured reason and §"What would close the gap" for
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

## What would close the gap (U32 is measurement, not optimization — items 1 and 4 were done in U33)

In rough order of expected-return-per-risk:

1. ~~**Swap the global allocator** (e.g. `mimalloc`/`jemalloc`). Zero algorithmic risk, one line,
   and it targets the 51.5% bucket directly. It is also the cheapest way to *test* the root-cause
   hypothesis above: if it moves the number a lot, the diagnosis is confirmed.~~
   **DONE (U33) — the single largest win, and it confirmed the diagnosis.** `mimalloc`, worth
   **1.22×–3.62×** on the nine decision-procedure workloads on its own, and it dropped the
   allocator's share of CPU time from 51.5% to 13.4% (≈248 ms → ≈38 ms per iteration of fixture
   286). It **partially, not fully, closed the gap**: 9-of-11-slower became 1-of-11-slower, but
   fixture 637 is still 1.16× slower and 179/230 are ties. See §U33 above.
2. **Flatten the transition representation.** A sorted `Vec<(i32, SmallVec<[usize; 1]>)>` or a
   CSR-style arena per `Fa` would remove most of both the allocator and the B-tree-navigation
   buckets. Bigger change, real behavioural risk (iteration order is load-bearing in several
   ported algorithms — see `fa.rs`'s notes on `determine_permutation_map`), and it must not
   change any language.
   **Still open, and now the top candidate**: after U33, `BTreeSet::insert` inside
   `subset_construction` is the single largest frame in the profile (24% of real work) and the
   B-tree bucket is 27.6% — the two buckets this item targets are what is left.
3. **Reuse buffers across `act()` steps** instead of building a fresh map per state.
   Still open.
4. ~~Build-profile tuning (`lto = "fat"`, `codegen-units = 1`). Untested here; the numbers above
   are the *default* `--release` profile, so this is free headroom of unknown size.~~
   **DONE (U33) — real but small: a further 1.08×–1.16×**, uniformly, on top of the allocator
   swap. The headroom was not free after all (a from-scratch `--release` build of `wr-bench` went
   ~7 s → ~25 s), but the price is only paid by the release/bench/gated-slow builds, never by
   `cargo test --workspace`.

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
CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  cargo run -p wr-bench --release --bin compare            # U33's "mimalloc alone" column
```

Every run also writes its report to `target/bench-report.txt`.

## Threats to validity, stated up front

* **One machine, one run each.** These are single-run means on one Apple Silicon laptop; nothing
  here is a cross-platform claim, and the µs-scale rows in particular carry real jitter (fixture
  1's Java max is 2.3× its min). The ~1.3× algorithmic gap is well outside that noise; the
  1.35× on fixture 207 is not far outside it.
* **`-Xmx4096m` on the JVM** — generous, and it favours *Java* (fewer collections), so it is a
  conservative setting for the conclusion actually drawn here.
* **The port's default `--release` profile** (no LTO, `codegen-units = 16`) *and the system
  allocator*. Both were changed in U33 — see the top of this file for the re-measurement; the
  U32 table below this line is the un-tuned baseline, on purpose.
* **The corpus is not a worst-case-blowup benchmark.** Walnut's own integration suite is designed
  to finish inside its `MAX_TOTAL_SECS = 1800`, so the heaviest ordinary workload here is ~2 s.
  The `sc637` row is the only genuinely large one, and it is the only one measured with n = 2.
