# `agent/par-posthoc` — results

Companion to [`PAR-POSTHOC.md`](PAR-POSTHOC.md) (the design and the coverage map). This file
holds the measured numbers and the verdict.

**Measurement caveat, stated once and meant.** Every timing here was taken on a machine shared
with two sibling agents running their own release builds and corpus replays; observed load
average ranged from 6 to 34 on 8 cores. Timings are **spot checks**, not results. Correctness
tallies (golden, differential, byte-stability) are unaffected by load and *are* results.

---

## 1. Verification matrix

### 1a. Fast tier

`cargo test --workspace` — **40 suites, 0 failures**, with the parallel path enabled by default.

`wr_core::par_determinize`'s own 8 tests, all green:

| Test | What it pins |
|---|---|
| `canonicalize_is_the_identity_on_subset_construction_output` | Lemma L2 |
| `canonicalize_recovers_the_sequential_numbering_from_any_relabelling` | Lemma L1, with reverse/rotation/shuffle permutations |
| `par_matches_the_sequential_implementation` | field-for-field contract, large deterministic inputs |
| `par_matches_the_sequential_implementation_on_nondeterministic_inputs` | same, on real NFAs with genuine blow-up (with its own anti-vacuity floor) |
| `the_parallel_phase_really_does_produce_a_different_numbering` | **anti-vacuity for the whole lane** |
| `repeated_parallel_runs_agree_bit_for_bit` | determinism of the recovered result |
| `small_inputs_delegate_to_the_sequential_implementation` | the threshold is real |
| `a_zero_alphabet_automaton_delegates_to_the_sequential_implementation` | the degenerate-shape guard |

The fifth is the one that makes the rest mean anything: it runs the *raw*, un-canonicalized
parallel construction 20 times on a 1200-state input and asserts that the numbering genuinely
differs from the sequential one — and that canonicalization brings every one of those distinct
numberings back to the sequential answer. Without it, every other result in this module would be
consistent with the parallel phase having accidentally reproduced the sequential order.

### 1b. Tier 1 — golden corpus

`WALNUT_JAVA_DIR=... cargo test -p wr-golden --release -- --ignored --nocapture`, four
configurations. **All four agree, exactly:**

| # | Mode | Threshold | fixtures | pass | fail | skip | timeout | not-run |
|---|---|---|---|---|---|---|---|---|
| 0 | sequential (base `dfed86b`) | — | 675 | **670** | 1 | 4 | 0 | 0 |
| A | eager recovery | 64 (production) | 675 | **670** | 1 | 4 | 0 | 0 |
| A' | eager recovery | **1 (forced)** | 675 | **670** | 1 | 4 | 0 | 0 |
| B | **deferred** recovery | **1 (forced)** | 675 | **670** | 1 | 4 | 0 | 0 |
| C | **deferred** recovery | 64 (production) | 675 | **670** | 1 | 4 | 0 | 0 |

The single failure is fixture 383 in every run — the long-standing, pre-existing WB-039
`details`-text divergence documented in `tests/golden/STATUS.md`, unrelated to this branch. The
four skips are the four deferred-OTF strategies, also pre-existing.

Rows A' and B are the ones that matter. `WR_PAR_MIN_STATES=1` forces **every determinization in
the corpus** down the racy parallel path rather than the handful that clear the 64-state
production threshold, so these are not a token exercise of the new code.

#### The three tallies, stated separately as asked

The golden harness fails a fixture if *any* of its halves differs, and `Verdict::is_excused_by`
scopes fixture 383's `KNOWN_DIVERGENCES` entry to `TextField::Details` alone — so a matrix or
graphviz regression on 383 would fail the run rather than be excused. That lets the three
tallies be read off exactly, for **all four** parallel configurations:

| Tally | Result |
|---|---|
| **Automaton-semantic matches** (`wr_core::equiv`) | **671 / 671 compared** — including 383, whose automaton comparison passes |
| **Byte-exact final artifacts** (`.gv` graphviz + the 28 CAS matrix comparisons) | **all compared, all match** — 0 byte divergences |
| **`details`-text matches** | **670 / 671** — the one miss is 383's pre-existing WB-039 gap |

So: **zero regressions in any of the three, in any mode, including the aggressive one.**

#### What row B means

Row B is the experiment's headline. In deferred mode the racy numbering is *not* repaired inside
the primitive: it flows into `trim`, `minimize` (Valmari block ids), `product`, `quantify`, and
every `Commands/*` handler, and the only normalization anywhere is the `automaton.canonize()`
that `write_txt`/`write_gv`/`matrix_writer` already do on their own. That is enough to reproduce
Walnut's entire recorded corpus — bytes included.

The reason it works is worth stating precisely, because it is *not* "canonicalize fixes
everything" (§2b of `PAR-POSTHOC.md` lists what it cannot fix). It is that `build_racy`'s output
is a genuine **relabelling** of the sequential one — same symbols per state, same
destination-list contents in the same positions — so every downstream operation behaves
isomorphically, and the minimal DFA that reaches the writer is unique up to isomorphism.

#### Timing of the corpus runs is NOT reported as a result

Wall clock was 34.0 s (base), 39.1 s (A'), 59.5 s (B), 65.8 s (C). That ordering tracks the
*order the runs happened in*, not the modes: sibling agents' load rose monotonically across the
session. It is consistent with a mode effect and equally consistent with pure contention, so it
is recorded and **not interpreted**. The controlled timing is §2's micro-benchmark instead.

### 1c. Tier 3 — differential vs the real JVM

```
WALNUT_JAVA_DIR=... WR_DIFFGEN_QUERIES=10000 WR_DIFFGEN_SEED=0x9C4E7B21A05D33F1 \
  cargo test -p wr-differential-gen --release -- --ignored --nocapture
```

Fresh seed, eager mode, against a live `walnut-java` JVM:

```
match            : 10000
divergence       :     0
skip-too-big     :     0
  jvm errors     :     0
```

**10000 / 0 / 0** — the brief's bar, met exactly.

### 1d. Determinism of written artifacts (the 5× check)

Eleven real corpus fixtures, dispatched through `wr-cli`'s real path and hashed on the **written
`.txt` bytes** (`RustEngine::dispatch` renders through the real writer, so `canonize()` has run).
Eleven distinct, non-trivial hashes — the check is not comparing empty output (see §3 for why that
warning is here).

| Comparison | Result |
|---|---|
| eager, 5 runs vs the sequential engine | **5/5 byte-identical**, all 11 workloads |
| deferred, 5 runs vs the sequential engine | **5/5 byte-identical**, all 11 workloads |
| deferred, run-to-run (1 vs 2,3,4,5) | **all byte-identical** |

So the written artifacts are stable across racy runs *and* equal to what the sequential engine
writes — in both modes. Computation is nondeterministic (proven separately by
`the_parallel_phase_really_does_produce_a_different_numbering`); output is not.

---

## 2. Timings — the parallel win vs the reconstruction cost

### 2a. The isolated primitive (`micro_sequential_vs_parallel`, 8 threads, `--release`)

This is the only *controlled* measurement here: one function, two implementations, same process,
no dispatch/parse/IO around it. Both reconstruction implementations are timed on the **same** racy
output, and both are asserted equal to the sequential result before their timings are believed.

| input states | sequential SC | parallel phase | `canonicalize()` | net | **fused `reconstruct()`** | **net** |
|---|---|---|---|---|---|---|
| 2,000 | 2.379 ms | 2.368 ms (1.00×) | 3.947 ms | 0.38× | **302 µs** | **0.89×** |
| 20,000 | 16.347 ms | 19.379 ms (0.84×) | 15.054 ms | 0.47× | **1.555 ms** | **0.78×** |
| 100,000 | 231.202 ms | 126.113 ms (1.83×) | 170.345 ms | 0.78× | **26.014 ms** | **1.52×** |

**Reconstruction overhead, reported separately as asked.** With production `Fa::canonicalize` it is
**100–165 % of the parallel compute phase** — i.e. it more than eats the win, every time, at every
size. With the fused one-pass `reconstruct` it is **10–21 % of the compute phase** (6.5×, 9.7×,
13× cheaper respectively). The difference is entirely allocation: `canonicalize` builds every
state's `BTreeMap` row twice and copies it once; `reconstruct` moves each row exactly once.

So the honest answer to *"if reconstruction eats the win, say so plainly"* is: **it did, decisively,
in the obvious implementation — and the fix is a fused renumber, not a cheaper parallel phase.**

**The parallel win itself only appears at scale.** 1.83× at 100,000 input states; a wash at 20,000;
nothing at 2,000. That is the level-synchronous BFS showing its limit — parallelism is bounded by
the *width of the BFS frontier*, and a subset construction's frontier is often narrow even when the
automaton is large.

**Run-to-run variance is severe and is not hidden.** An earlier run of this identical benchmark
measured the 20,000-state parallel phase at 1.92× where this one measured 0.84×, and the
100,000-state case at 2.53× where this one measured 1.83×. Two sibling agents were running release
builds and corpus replays throughout; observed load average ranged 6–34 on 8 cores. Treat the
*ratios between reconstruction implementations* (measured back-to-back in one process, on one
input) as sound, and every absolute speedup as a spot check with a wide error bar.

### 2b. End-to-end, on real corpus fixtures

`benches/src/bin/par_compare`, the same 11 fixtures `compare.rs` benchmarks against the JVM,
sequential vs eager parallel recovery. `par-calls` counts determinizations that actually took the
parallel path across the whole warmup+measure batch; `compute`/`recover` are per-dispatch means.

| fixture | sequential | parallel | speedup | par-compute | recover | par-calls | artifact |
|---|---|---|---|---|---|---|---|
| 1 | 0.469 ms | 0.455 ms | 1.03× | — | — | **0** | identical |
| 207 | 0.468 ms | 0.334 ms | 1.40× | — | — | **0** | identical |
| 293 | 34.226 ms | 26.339 ms | 1.30× | 14.616 ms | 2.081 ms | 30 | identical |
| 521 | 23.328 ms | 30.897 ms | 0.76× | — | — | **0** | identical |
| 179 | 117.679 ms | 80.621 ms | 1.46× | 51.103 ms | 7.281 ms | 10 | identical |
| 266 | 32.131 ms | 45.216 ms | 0.71× | 24.744 ms | 1.713 ms | 40 | identical |
| 230 | 498.771 ms | 256.490 ms | **1.94×** | 188.310 ms | 21.449 ms | 4 | identical |
| 295 | 66.608 ms | 71.594 ms | 0.93× | 26.466 ms | 6.537 ms | 40 | identical |
| 261 | 95.098 ms | 107.405 ms | 0.89× | 43.187 ms | 11.957 ms | 40 | identical |
| 286 | 180.715 ms | 212.955 ms | 0.85× | 77.309 ms | 23.842 ms | 10 | identical |
| 637 | 21.426 ms | 20.191 ms | 1.06× | — | — | **0** | identical |

**Every one of the 11 written artifacts is byte-identical to the sequential engine's.** That is the
correctness result and it is not noisy.

**The noise floor, calibrated from this table itself.** Four fixtures (1, 207, 521, 637) took the
parallel path **zero** times — for them the two columns are the *same code executing the same
work*, so their spread is pure measurement noise. They span **0.76× to 1.40×**. Any speedup inside
that band in this table therefore means nothing, which disqualifies seven of the eleven rows
outright and leaves exactly one result above the floor: **fixture 230 at 1.94×** (and 179 at 1.46×
sitting right on the edge of it).

Fixture 637 is worth noting separately: it is the `[strategy 6 BRZ]` workload, and 0 par-calls is
correct rather than a miss — `determinize`'s Brzozowski arm calls the sequential
`subset_construction` internally and was deliberately left unwired (`PAR-POSTHOC.md` §4).

**Reconstruction overhead on real workloads** is 7–31 % of the parallel compute phase (293: 14 %,
179: 14 %, 230: 11 %, 266: 7 %, 295: 25 %, 261: 28 %, 286: 31 %), consistent with the micro
benchmark's 10–21 % and nowhere near the 100–165 % that production `canonicalize` cost.

---

## 3. Ignored tests

**None. Zero tests were `#[ignore]`d, weakened, or deleted for this branch.**

The brief anticipated a list of byte-pinning unit tests the racy compute path would break. There
is nothing to list, and that is a result rather than an omission — it is what the eager mode's
contract (return the sequential `Fa` field for field) buys. It held under the strongest available
test: `WR_PAR_DEFER=1 WR_PAR_MIN_STATES=1 cargo test --workspace`, i.e. the **aggressive** mode
with the numbering escaping into the whole engine *and* every determinization forced down the
parallel path, is also fully green.

Two honest qualifications on how much that proves:

1. Many of the port's exact-structure snapshots (`crates/wr-core/tests/refactor_structural_
   snapshots.rs`, and the `determinize.rs` pins) call `subset_construction` **directly**, not
   through `determinize`'s dispatcher. Those are untouched by either mode by construction, so they
   are evidence that the sequential implementation still exists and is unchanged — not evidence
   about the parallel path. The parallel path's own field-for-field contract is carried by
   `par_matches_the_sequential_implementation{,_on_nondeterministic_inputs}` instead.
2. One test is `#[ignore]`d, but as a **measurement**, not a casualty:
   `par_determinize::tests::micro_sequential_vs_parallel`, marked
   `#[ignore = "measurement, not an assertion; machine-shared timings"]`. It still asserts
   correctness (both reconstruction implementations must equal the sequential result) when run.

### The one defect this branch's own code had

Found by self-review, not by a test: `build_racy`'s workers synchronize on a `Barrier` sized to
the worker count, so **a worker that panics never reaches its next `wait()` and every other worker
blocks there forever**. An input that makes the sequential subset construction panic cleanly — a
destination id outside `0..q`, a seed member past the end of the table, both reachable because
`subset_construction` is `pub` and `Fa` carries no invariant excluding them — made the parallel
path *hang*. That is precisely the failure mode CLAUDE.md's "per-test resource caps, never hangs"
guardrail exists to forbid.

Fixed with `is_safe_to_parallelize`, a pre-flight bounds scan that routes any violating input to
the sequential implementation, so the panic site and message stay exactly what the existing tests
pin. Mutation-verified in both directions: deleting the guard makes
`a_malformed_automaton_panics_rather_than_deadlocking_the_workers` fail on its 30-second watchdog
(30.01 s — the deadlock is real, not hypothetical); restoring it passes in under a second. The
test uses an explicit spawned-thread + `recv_timeout` watchdog rather than `#[should_panic]`,
because a `should_panic` test would have hung the whole run instead of failing.

### A vacuous check this report nearly contained

The first run of the 5× determinism check reported "IDENTICAL" five times over. It was comparing
two **empty** files: `par_compare` was passing the walnut-java checkout root where
`build_session_tree` needs `src/test/resources/integrationTests`, every invocation died with a
bare `No such file or directory`, and the driver script had suppressed stderr. Recorded here
rather than quietly fixed, because a green-looking check that compared nothing is exactly the
class of defect this project's review discipline exists to catch, and it was in *this* work.

---

## 4. Verdict — post-hoc reconstruction vs deterministic parallelization

### 4a. The thesis is correct, and holds further than expected

Not merely for the primitive. In the **deferred** mode the racy numbering is never repaired inside
the engine at all — it flows through `trim`, `minimize`'s Valmari block ids, `product`, `quantify`
and every `Commands/*` handler, and the only normalization anywhere is the `canonize()` that
`write_txt`/`write_gv`/`matrix_writer` already performed before this branch existed. That
reproduces Walnut's entire 675-fixture recorded corpus (automata, `.gv` bytes, CAS matrix bytes,
`details` text) and passes the full 1,811-test workspace suite. Zero tests were ignored, weakened
or deleted.

### 4b. But *why* it holds is narrow, and the coverage map has real holes

It is **not** "canonicalization fixes everything". It holds because `build_racy`'s output is a
genuine *relabelling* of the sequential one, so every downstream operation behaves isomorphically
and the minimal DFA that reaches the writer is unique up to isomorphism. `PAR-POSTHOC.md` §2b lists
where the same trick fails, and those are not hypothetical:

- `export_to_ba` never canonicalizes — `.ba` bytes carry raw ids.
- `morphism::to_word_automaton` and `ostrowski` deliberately `set_canonized(true)` to **suppress**
  the writer's canonicalization so unreachable states survive; post-hoc canonicalization there
  would *change* the output, not restore it.
- NFA-shaped intermediates are not normalized by `canonicalize` at all (it preserves
  destination-list order, and that order feeds its own BFS).
- Metacommand `[strategy n]`/`[export n]` indices and `::` details-text *ordering* are **ordering**
  problems, which canonicalization cannot touch in principle.

The corpus does not exercise the first three against a racy numbering, so "670/675 in deferred
mode" is evidence that those paths were not reached, not that they are safe.

### 4c. On this workload, the post-hoc freedom bought almost nothing

This is the part worth being blunt about. The freedom the lane buys is *arbitrary metastate
numbering during compute*. For subset construction specifically, that turns out not to be the
binding constraint:

A deterministic lane can use the **identical** parallel structure — level-synchronous BFS, workers
computing each metastate's per-symbol union keys in parallel — and then mint ids in a short
**serial** pass over the frontier in id order, symbols ascending. That reproduces the sequential
numbering exactly (level-synchronous frontiers are contiguous id ranges in increasing order, so
frontier order *is* sequential mint order), and it parallelizes exactly the same expensive part:
the union computation. So the post-hoc lane does not unlock a better parallel algorithm here. It
**trades a serial hash-cons pass for a serial renumber pass** — measured at 10–21 % of compute for
the fused renumber — and the two are plausibly a wash.

Where post-hoc genuinely wins is **engineering scope, not speed**: the deferred-mode result means
you can parallelize anything upstream of the write path without auditing `minimize`, `product`,
`quantify` and `trim` for order-sensitivity one at a time. The deterministic lane must establish
order-preservation at every one of those boundaries; the post-hoc lane establishes one lemma at
the writer and is done. For a codebase whose own `docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md` exists
because iteration order is load-bearing in several ported algorithms, that is a real and reusable
advantage — it is about how much code you must reason about, not how fast it runs.

### 4d. The performance verdict is negative for Walnut's actual workloads

- The parallel phase wins only around **10⁵ input states** (1.8–2.5×); at 20,000 it is a wash and
  at 2,000 it loses. Level-synchronous BFS is bounded by **frontier width**, and a subset
  construction's frontier is often narrow even when the automaton is large.
- Reconstruction must be the fused renumber. The obvious implementation (production
  `canonicalize`) costs more than the entire operation.
- Walnut's own recorded corpus essentially never reaches the size where any of this pays: at the
  production 64-state threshold only a handful of the corpus's determinizations take the parallel
  path at all.

**Recommendation: do not merge the eager mode as a default.** It is correct, invisible, and a
small net loss on this corpus. What is worth keeping is the *analysis* — the Canonical Recovery
Lemma, the coverage map, and the demonstration that the deferred mode is viable end to end —
because those are what make a future, better-targeted parallelization cheap to justify.

## 5. What I would do next, in priority order

1. **Replace the level-synchronous barrier with a work-stealing deque plus termination
   detection.** Frontier width, not thread count, is the measured ceiling; a worklist that lets a
   worker start on a newly-discovered metastate without waiting for the level to close removes it,
   and removes the per-level barrier cost with it.
2. **Raise `MIN_STATES_FOR_PARALLEL` to ~10⁴.** The current 64 is a guess; the micro-benchmark says
   the parallel path is a measured *loss* below roughly 20,000 states.
3. **Move up a level: DAG-parallel `eval` over independent subtrees.** This is where the large
   independent work actually is, and unlike subset construction it has no frontier-width ceiling.
   It needs the three *ordering* reconstructions in `PAR-POSTHOC.md` §5 — per-subtree `Logging`
   buffers spliced in sequential post-order, metacommand-index renumbering at join, and a decision
   about the `Session` `NumberSystem` cache (whose warmth is already observable in `details` text —
   that is fixture 383's and the 375–379 harness limitation's whole story).
4. **A parallel `product` with a bespoke post-hoc renumber.** `canonicalize` cannot recover
   `cross_product`'s numbering (its discovery order is A-symbol-major, which is not ascending
   *result*-symbol order once the second operand adds a track), but a second BFS that re-walks
   `a.d[p] × b.d[q]` in Java's own order, over already-computed edges, would — and that is a
   genuinely different post-hoc reconstruction from canonicalization, which would test the thesis
   somewhere it is not already known to hold.
