# `agent/par-max` — parallelizing the decision procedure

**Status: experiment, one parallel site landed, fully verified. NOT for merge under current
project rules** (see "Relaxations" below — every one is stated, none is silent).

Branch point: `dfed86b` (tip of `perf/beyond`). Machine: Apple Silicon, 8 cores (4P + 4E).

---

## 1. What was parallelized

One site: **`wr_core::determinize::subset_construction`'s BFS**, which the campaign profile
(`baseline-perf-campaign.txt`, post-P1(a) section) puts at **49.4 % – 87.8 %** of the engine's
real work on the four profiled fixtures. Nothing else in `wr-core`/`wr-logic` was touched.

### Design

`subset_construction`'s worklist is a queue, so the frontier `[cursor, metastate_list.len())`
is a set of metastates that can all be expanded at once. The loop body was split in two —
**verbatim**, not rewritten:

| function | what it does | purity |
|---|---|---|
| `expand_metastate` | P1(a)'s bucket-fill + epoch-dedup + sort/dedup canonicalization | **pure** function of `fa` (shared, immutable) and one metastate's member list. Reads no id, no worklist, no map. |
| `merge_expansion` | mints ids via `metastate_to_id`, builds the `Fa.d` row | the single-threaded half; makes every observable ordering decision |

On top of that sits a per-level **schedule**:

* **sequential** — expand-then-merge, one metastate at a time (the pre-branch interleaving);
* **level-parallel** — snapshot the frontier, expand it with `rayon::par_chunks`, then merge
  the chunk results back **in frontier order**, single-threaded.

`Schedule::Auto` picks per level, on two thresholds (`PAR_MIN_LEVEL = 256` metastates **and**
`PAR_MIN_MEMBERS = 4096` total members). Those thresholds are load-bearing, not defensive: a
single fixture dispatch runs ~190 separate `subset_construction` calls and only one or two are
large. `crates/wr-core/src/par.rs` holds the policy (`WR_PAR=0`, `WR_PAR_THREADS=N`).

### Why the output is bit-identical

Because the worklist is a queue and the merge preserves frontier order, ids are minted in the
same order as the sequential loop, so **state numbering does not move**:

1. *Discovery order* — the merge walks the frontier ascending and, within a metastate, symbols
   ascending: the identical `metastate_to_id` probe/insert sequence.
2. *Key computation* — `expand_metastate` cannot observe which schedule it is running under.
3. *Frontier membership* — a metastate discovered during a parallel level lands at an index
   `>= end`, so it is expanded next level, exactly as a later index would have been anyway.

This is why **no test had to be ignored**, which was not the expected outcome — the brief
budgeted for marking byte/structure pins `#[ignore]`, and none needed it.

### Level structure (the measurement that justified the design)

Instrumented BFS level widths for each fixture's own dispatch (prelude excluded):

| fixture | metastates in the dispatch's SC calls | share in levels of size >= 64 |
|---|---|---|
| 230 | 115,802 (widest level 31,034) | **99.9 %** |
| 286 | 34,988 (widest level 9,795) | **100.0 %** |
| 179 | 33,000 (widest level 16,388) | **99.9 %** |
| 261 | 20,392 across 3 calls | **99.2 %** |
| 637 | 496 (`[strategy 6 BRZ]`, so SC is not its hot path) | 49.0 % |

---

## 2. Verification

| gate | result |
|---|---|
| `cargo test --workspace` | **52 suites green, 0 failures. No test ignored by this branch** — the 3 the run reports as ignored (`milestone_0_soak`, `the_harness_detects_divergence_and_survives_a_hang`, `tier1_golden_corpus`) are the pre-existing gated-slow tier, `#[ignore]`d on `dfed86b` too, and two of the three are run explicitly below. |
| Tier 3 — `wr-differential-gen`, 10,000 queries, fresh seed `0x9E3779B97F4A7C15` | **10,000 match / 0 divergence / 0 skip**; 9,239 automaton comparisons, 405 TRUE, 353 FALSE, 3 error |
| Tier 1 — golden corpus, 675 fixtures | **670 pass / 1 fail / 4 skip / 0 timeout / 0 not-run** — byte-identical to the pre-change baseline. The one failure is fixture 383, the long-standing text-only WB-039 divergence; the 4 skips are the deferred-OTF strategies. **Zero regression.** |
| Determinism | 7 workloads x 3 processes x 3 reps: **exactly one answer digest per workload**. The digest is over the answer's *exact text*, not a semantic class — strictly stronger than this branch's bar. |
| Cross-engine (`compare`, warm JVM) | all 11 workloads: answers agree by `wr_core::equiv`, all 9 automaton-valued ones match the automaton `walnut-java` itself recorded, and **every peak-state trace equals Java's exactly** — independent evidence no intermediate automaton changed size. |

New unit gates in `determinize.rs`:

* `the_parallel_schedule_matches_the_pre_p1a_reference_implementation` — the frozen pre-P1(a)
  reference vs the **parallel** schedule, field-for-field, over the same 20,000 generated
  automata the sequential schedule is checked against. Forcing via `Schedule::Parallel` is what
  makes it load-bearing: the generator's `q <= 6` inputs are three orders of magnitude below
  `Auto`'s thresholds, so without forcing it would silently re-run the sequential path.
* `auto_selects_the_parallel_schedule_on_a_large_input_and_agrees_with_sequential` — the other
  half: a blow-up NFA that trips `Auto`'s *real* thresholds, with an anti-vacuity assertion so a
  future threshold retune fails the test instead of hollowing it out.
* `should_parallelize_requires_both_a_wide_level_and_real_work_in_it`.

---

## 3. Timings

> **These numbers are unreliable and are reported as such.** The machine was shared with two
> sibling agents throughout; 1-minute load average ranged 5–16, with sibling `rustc` processes
> at 76–98 % of a core. Cross-run spread on identical binaries reached 2x. Every table below is
> **interleaved** (arms alternate round by round so drift hits them equally) and reports the
> **minimum** across rounds, which is the least-contended sample and the most robust estimator
> available here. Treat ratios as directional. Final numbers belong to the coordinator.

### Corpus fixtures — `rustonly`, 3 rounds interleaved, min-of-rounds

| fixture | pre-change | new, `WR_PAR=0` | new, parallel | speedup |
|---|---|---|---|---|
| 179 | 218 ms | 175 ms | **95 ms** | **2.30x** |
| 230 | 843 ms | 980 ms | **444 ms** | **1.90x** |
| 261 | 143 ms | 151 ms | **110 ms** | **1.30x** |
| 286 | 368 ms | 292 ms | **288 ms** | **1.27x** |

The `WR_PAR=0` column is the control that matters: it runs the **new** code on the sequential
schedule and lands within noise of pre-change. So the win is parallelism, not the restructuring.

### `WR_BENCH_HEAVY=1` research-shaped rows — 2 rounds interleaved, min-of-rounds

| row | pre-change | new, parallel | speedup |
|---|---|---|---|
| alt3 | 2.201 s | **862 ms** | **2.55x** |
| rsp1 | 4.063 s | **2.204 s** | **1.84x** |
| rsp2 | 6.628 s | **2.558 s** | **2.59x** |

The heavy rows benefit most, which is the useful direction: they are the closest thing in the
harness to a real research workload.

### Thread-count sweep — inconclusive

`WR_PAR_THREADS` 1/2/4/6/8, two rounds. Contention swamped the signal: min-of-rounds moved
non-monotonically (e.g. 230 read 562/254/298/417/279 ms). The only defensible reading is that
most of the win is present by 2 workers and the 4E cores add little. **This sweep should be
re-run on a quiet machine before anyone draws a scaling curve from it.**

---

## 4. The Amdahl ceiling actually observed

`sample(1)`, 15 s strictly inside `RustEngine::bench`'s timed loop. This matters: `rustonly`'s
own untimed answer check calls `wr_core::equiv::language_equivalent`, which on a large result
costs more than the dispatch — a first attempt sampled across it and mis-attributed 23 % of
"engine" time to the harness. Every capture below is asserted free of that frame.

Top-of-stack samples, idle-worker frames (`__psynch_cvwait`) excluded:

| bucket | fixture 230 | fixture 286 |
|---|---|---|
| `expand_metastate` (**parallel**) | 10,200 | 6,302 |
| `sort_unstable` on the key (**parallel**, child of the above) | 4,967 | ~250 |
| `merge_expansion` (sequential) | 1,695 | 1,853 |
| metastate `HashMap` hashing + `memcmp` (sequential) | 1,375 | ~610 |
| Valmari `minimize` — `mark`/`split`/`make_adjacent`/sort (sequential, **untouched**) | 204 | 4,532 |
| allocator + `memmove` + `madvise` (sequential) | ~810 | ~1,694 |
| rayon work-stealing sync (`swtch_pri`, `psynch_mutex*`) | ~7,660 | ~3,120 |

Parallelizable share of *useful* work: **~78 % on 230**, **~41 % on 286**.

Ideal 8-worker speedups from those fractions are 3.2x and 1.6x; observed are ~2.0x and ~1.3x.
Two things account for the gap, and both are visible in the table:

1. **Pool synchronization is real overhead** — 28 % of 230's samples sit in `swtch_pri` /
   `psynch_mutex*`, i.e. work-stealing spin. On a machine already running two other agents, an
   8-worker pool spends a lot of time contending for cores it does not own.
2. **The E-cores are not P-cores.** 8 logical workers on 4P+4E is nowhere near 8x of capacity.

The per-fixture spread tracks the profile almost exactly: 179/230 are 83–88 % subset
construction and gain ~2x; 261/286 carry 16–26 % in Valmari minimize, which this branch does not
touch, and gain ~1.3x. **This is Amdahl, not a bad schedule.**

---

## 5. Relaxations of the project's normal rules

Stated, per the experiment's own terms:

1. **A new dependency (`rayon`) in a trust-critical crate**, justified in the workspace
   manifest. `std::thread::scope` was tried first as instructed and rejected on evidence: a
   fixture dispatch runs ~190 `subset_construction` calls with 25–30 BFS levels inside the big
   ones, so spawn+join per level is tens of thousands of OS thread creations per query — the
   same order as the work being parallelized.
2. **Panic attribution on a malformed `Fa` is no longer deterministic.** A transition naming a
   state outside `0..fa.q` still panics with the same message, but raised by whichever worker
   reaches it first. Every such test input is far below the thresholds and stays sequential.
3. **No two-reviewer adversarial round was run**, which project rules require for a `wr-core`
   diff. This is an experiment branch; the substitute evidence is the bit-identity gates above.

Not relaxed, because they turned out not to need to be: **no test was ignored**, no byte
freeze was broken, and the golden corpus is unchanged.

---

## 6. Operation-level (DAG) parallelism: investigated and REJECTED, with evidence

This was the brief's first-ranked hypothesis — run independent subtrees of the postfix token
stream concurrently. It was investigated rather than assumed, and the answer is **no**. Five
independent findings, each verified against the source:

1. **The Amdahl ceiling is 2–3, and the parallel part is the cheap part.** Every benchmark
   workload has the same shape: a small conjunction of word-automaton comparisons under a
   *sequential* quantifier-elimination spine that owns essentially all the runtime. Fixture 230
   (`Ei At ((t<n) => ((T[i+t]=T[i+t+n]) & (T[i+t]=T[i+3*n-1-t])))`) has **2** balanced legs;
   286 and 261 have **effectively 1**; the heavy rows have 2–3. The legs finish long before the
   `At` complementation starts. Realistic gain: well under 1.2x.
2. **`Expression`/`Token`/`Automaton` are `!Send`.** `Automaton` holds `Option<Rc<Automaton>>`
   (`automaton.rs:545`) and `Expression` holds `Rc<NumberSystem>` (`expr.rs:636`); the shared
   `NumberSystem` carries `RefCell` memo tables that `PORTING.md`'s Ruling 1 deliberately shares
   across every token in a formula. No subtree can cross a thread boundary without an
   `Rc`→`Arc` + `RefCell`→`RwLock` conversion across two crates.
3. **`DeterminizeContext` cannot be made safe at all.** Its per-command index selects the
   *algorithm* — `determinize.rs:252-253` looks up the strategy under the index and `:281-284`
   switches `subset_construction` vs `brzozowski` on it. Fixture 637 is `[strategy 6 BRZ]`
   precisely because subset construction on that sixth determinization does not finish.
   Reordering the index stream is a hang, not a cosmetic difference. And a `quantify` consumes
   one *or two* indices depending on whether its projection happened to be deterministic
   (`quantify.rs:214-219`), so the count is data-dependent per subtree.
4. **Error ordering is observable byte-for-byte.** `compute_with_ctx` reports the *first*
   failing token's position, and the position is embedded in the message text. Both harnesses
   compare error strings by exact equality, with a dedicated test saying so
   (`tests/golden/.../support/mod.rs`'s `compare_error`, and
   `tests/differential-gen`'s `error_comparison_is_exact_not_normalized`).
5. `Token::arity()` already exists (`token.rs:2773`), so subtree *extraction* is the easy part —
   which is worth stating, because it is the part that looks hard and isn't.

**This is a useful negative result, not a dead end**: it confirms the axis chosen in §1 is the
right one. Splitting over *metastates* works precisely because `Fa` is a plain
`Vec`/`BTreeMap`/`usize` structure with no `Rc` anywhere, so it is `Sync` for free — the
parallelism sits *inside* the expensive determinizations, which is where the time actually is,
rather than *between* cheap leaves.

## 7. What I would do next

Ranked by the profile in §4, not by guesswork:

1. **Reduce the sequential merge** — the biggest contained win left, and it is sequential work
   that this branch's own parallelization has now promoted to the critical path. Two pieces:
   hoist key hashing into the parallel workers (needs a hash table accepting a precomputed
   hash), and stop allocating each new metastate's member list *twice* —
   `metastate_to_id.insert(key.to_vec(), _)` plus `metastate_list.push(key.to_vec())` could
   share one `Arc<[usize]>` (`Arc<[T]>: Borrow<[T]>`, so slice lookups still work). Together
   ~10 % of 230's useful work.
2. **Parallel Valmari minimize** — the right and only target for 286/261 specifically (27 % of
   286's useful work, and the pool sits **idle** throughout it: `__psynch_cvwait` dominates that
   capture). Also the hardest; not attempted, per the brief.
3. **Tune the pool.** 28 % of fixture 230's samples are work-stealing spin (`swtch_pri`,
   `psynch_mutex*`). A larger `PAR_MIN_CHUNK`, or capping the pool to the 4 P-cores, is cheap to
   try and the sweep in §3 hints it may cost nothing.
4. **Re-run everything on a quiet machine.** The single biggest improvement available to the
   *evidence*, as opposed to the code.

## 8. Reproducing

```bash
export WALNUT_JAVA_DIR=/path/to/walnut-java

# A/B the two schedules (same binary; WR_PAR=0 is the sequential control).
WR_BENCH_ONLY=230,286,179,261 WR_BENCH_ITERS=3 WR_BENCH_WARMUP=1 \
  cargo run -p wr-bench --release --bin rustonly
WR_PAR=0 WR_BENCH_ONLY=230,286,179,261 WR_BENCH_ITERS=3 WR_BENCH_WARMUP=1 \
  cargo run -p wr-bench --release --bin rustonly

# Determinism (expect exactly one digest per workload, across several processes).
WR_RUSTONLY_DIGEST=3 WR_BENCH_ONLY=230,286 cargo run -p wr-bench --release --bin rustonly

# The gates.
cargo test --workspace
WR_DIFFGEN_QUERIES=10000 WR_DIFFGEN_SEED=0x<fresh> \
  cargo test -p wr-differential-gen --release -- --ignored --nocapture
cargo test -p wr-golden --release -- --ignored --nocapture
```
