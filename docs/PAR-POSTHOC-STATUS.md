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

*(filled in below)*

### 1d. Determinism of written artifacts (the 5× check)

*(filled in below)*

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

At the production threshold the parallel path is reached by only a handful of the corpus's
determinizations — most inputs to `determinize` are far below 64 states — so the end-to-end
picture is dominated by workloads that never take it. That is itself the finding: **Walnut's own
recorded corpus contains almost nothing big enough for this to help.**

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

## 4. Verdict

*(filled in below)*
