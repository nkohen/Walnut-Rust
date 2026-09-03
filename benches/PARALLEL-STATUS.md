# Parallelism in the engine — what was measured, what was built, what it is worth

Branch `agent/par-det`, built on `perf/beyond` @ `dfed86b`. The brief was to introduce
parallelism while keeping **all** observable behavior byte-for-byte identical to the
sequential engine: same state numbering, same `::`-details text in the same order, same
`.txt`/`.gv`/CAS bytes, same error messages, same golden-corpus tally.

Two candidate sites were considered. The first was measured and **rejected on the
evidence**; the second was built.

---

## 1. DAG-level parallelism across the postfix token stream — measured at 1.03x, not built

`wr_logic::eval::compute_with_ctx` walks a postfix token stream against one operand stack.
Each operation is a pure automaton construction whose output depends only on its inputs, so
running independent subtrees concurrently could not change any operation's result — the only
sequentially-observable artifact is `Logging` text order, which a per-task buffer plus an
in-postorder stitch could reproduce. That is a real design, and it is worth nothing here.

**Method.** A temporary env-gated per-token trace in `compute_with_ctx` (recoverable with
`git show 71f1bd0 -- crates/wr-logic/src/eval.rs`; deliberately reverted so `wr-logic`
carries no measurement code) plus `benches/src/bin/dagwidth.rs`, which rebuilds the
expression tree from the postfix arities and computes total work, the critical path, and the
infinite-processor ceiling `total / critical`.

**Result, over every `compute()` call the whole 675-fixture corpus makes (533 calls):**

| work floor | calls | aggregate ceiling | best single | calls with ceiling >= 1.1x |
|---|---|---|---|---|
| any | 533 | **1.030x** | 2.76x (fixture 62) | 224 |
| >= 1 ms | 119 | 1.025x | 2.09x (fixture 16) | 23 |
| >= 10 ms | 31 | 1.018x | 1.83x (fixture 362) | 3 |
| >= 100 ms | 3 | **1.001x** | 1.00x | **0** |

The ceiling is inversely correlated with the work — which is the whole finding. The
decision procedure's cost is superexponential in quantifier alternation, so the *outermost*
operation dwarfs its entire subtree. On the benchmark workloads a **single token carries
95–99.8% of the run**:

| workload | tokens | structural width | dominant token's share | ceiling |
|---|---|---|---|---|
| 230 | 35 | 16 | 99.8% | 1.00x |
| alt3 | 57 | 26 | 98.8% | 1.00x |
| 179 | 36 | 18 | 96.4% | 1.00x |
| 286 | 18 | 10 | 94.9% | 1.00x |

Note the "structural width" column: these DAGs are **not narrow**. Ten to twenty-six tokens
are simultaneously ready. They are just made of leaves and cheap operators. There is plenty
of independence and almost no work inside it.

**Conclusion:** DAG-level task parallelism would have bought ~0% for a large, byte-fidelity-
critical change (private `Logging` buffers, indent-state stitching, `DeterminizeContext`
index ordering). Not built. The measurement is kept and repeatable.

---

## 2. Intra-operation parallelism in `subset_construction` — built, 1.0–1.93x

That is where the work is (prior profiling put 59.5–89.4% of engine time in this one
function), so the question is whether its BFS has exploitable independence.

**Method.** A second temporary trace recorded, per `subset_construction` call, the frontier
width (`metastate_list.len() - cursor`) at every step.

**Result: the frontier is enormous.**

| workload | largest call | median lookahead | p90 | max |
|---|---|---|---|---|
| 230 | 103,216 states | 26,934 | 38,800 | 41,305 |
| alt3 | 326,396 states | 61,086 | 78,577 | 83,446 |
| 179 | 103,216 states | 26,934 | 38,800 | 41,305 |

Tens of thousands of metastates are discovered but not yet processed at any moment. So the
design is a **speculative pipeline**, not a fork-join.

### The design

The loop had two halves interleaved; they are now separated:

* **`key_rows`** — a *pure function of `fa` and one metastate's members*. It reads no id, no
  counter, and nothing another metastate produced. Computing it early, late, or on another
  thread cannot change its result.
* **`mint_row`** — the only place a state id is ever assigned. It stays on the calling
  thread, driven by the same `cursor`-ascending x symbol-ascending sequence over the same
  keys.

Worker threads run `key_rows` for metastates ahead of the cursor; the calling thread mints.
Because minting is untouched, the sequence of `metastate_to_id` probes — hence every id,
hence `metastate_list`, `d` and `o` — is identical to the sequential engine's **for any
scheduling**. This is a scheduling change over a pure function, not a change to the
construction.

Scheduling details: a FIFO work queue (`Mutex` + `Condvar`), an MPSC result channel, a
1,024-metastate dispatch window bounding the memory speculation holds, and a `CloseOnDrop`
guard so an unwind out of the mint loop cannot deadlock against workers parked on the
condvar. No new dependencies — `std::thread::scope` only.

Two behaviors are preserved deliberately, not incidentally:

* **Small inputs pay nothing.** Below 4,096 discovered metastates the pipeline never starts;
  the corpus's thousands of small determinizations run the identical sequential code with no
  threads spun up.
* **The malformed-`Fa` panic still fires on the calling thread at the same cursor.** Workers
  screen for it (`members_are_in_range`) rather than speculatively panicking on a metastate
  the sequential engine never reaches.

### Byte-identity evidence

| check | result |
|---|---|
| `subset_construction_reference` cross-check, forced through the parallel path (20,000 generated cases; `min_states: 0`, `window: 3`, 4 workers against a 1-metastate frontier — the worst case for the scheduler) | field-for-field identical |
| repeated-run determinism, 400 cases x 8 runs | identical |
| `::`-details vs the **sequential engine**: 4 workloads x 4 repeats x thread counts [0,1,2,4,7] = 80 runs | all identical after `tests/golden`'s own `normalize_message` |
| golden corpus | `675 \| 670 pass \| 1 fail (383) \| 4 skip \| 0 timeout` — the required tally, unchanged |
| differential-gen, 10,000 queries, fresh seed `0x7A3D91C4E5B20F68` | 10,000 match / 0 divergence / 0 skip |
| `cargo test --workspace` | green, zero tests modified or ignored |

Mutation-verified in both directions: minting whatever reply arrives first (instead of the
cursor's) fails both the reference cross-check and the determinism test in 0.02 s; removing
the panic screen fails `the_screen_keeps_speculation_from_panicking_on_a_worker`.

On the `::`-details normalization: the gate is `tests/golden`'s own comparator, a verbatim
port of Java's `IntegrationTest.assertEqualMessages`, whose `replaceAll("\\d+ms", "")`
removes elapsed-time text. That is not a convenience — a purely sequential engine also prints
different `- Xms` values run to run, so raw equality is not a property this code ever had.
Everything a scheduling change could actually alter (every state count, every line, their
order and indentation) survives the normalization and is compared, and the raw digest is
reported alongside so the normalization is visible rather than silent.

### Numbers

`benches/src/bin/scbench.rs`, `WR_SC_THREADS=0` = the pre-parallel path in the same binary.
Minimum of 4 iterations, best of 3 interleaved rounds.

**Machine load average 6.9–8.2 during this run, on 8 logical cores shared with two other
agents. These are indicative, not a measurement of record.**

| workload | t=0 | t=2 | t=4 (shipped) | t4 vs t0 |
|---|---|---|---|---|
| rsp1 | 3.461 s | 1.930 s | **1.791 s** | **1.93x** |
| alt3 | 1.531 s | 1.174 s | **947 ms** | **1.62x** |
| 230 | 457 ms | 385 ms | **313 ms** | **1.46x** |
| 179 | 106 ms | 89.8 ms | **80.6 ms** | **1.31x** |
| 293 | 30.9 ms | 28.5 ms | 27.3 ms | 1.13x |
| 261 | 87.4 ms | 83.0 ms | 79.3 ms | 1.10x |
| 286 | 156 ms | 155 ms | 155 ms | 1.01x |
| 521 | 20.0 ms | 20.2 ms | 19.8 ms | 1.01x |
| 637 | 18.7 ms | 18.6 ms | 18.7 ms | 1.00x |
| 295 | 57.0 ms | 55.5 ms | 60.9 ms | **0.94x** |

The shape is what the threshold was designed to produce: **the win scales with the work**,
and the sub-25 ms rows (521, 637) are flat rather than regressed, because they never start
the pipeline at all.

**Fixture 295 at 0.94x is reported as measured.** It is the one row that went backwards. It
is within this machine's run-to-run spread (295 measured 1.03x at t=2 in the same run and
1.31x at t=2 in an earlier one), so it is more likely noise than a real regression — but it
has not been shown to be noise, and a quiet-machine re-run should settle it.

### The thread-count default, and why it is not the fastest number measured

An earlier sweep, on the same machine at load average 11–42, went the *other* way:

| threads | 293 | 179 | 230 | 261 | 286 |
|---|---|---|---|---|---|
| 2 | 1.54x | 1.60x | 1.58x | 1.32x | 1.28x |
| 4 | 1.10x | 1.27x | 1.19x | 1.16x | 1.07x |
| 7 | 0.84x | 1.04x | 1.08x | 0.98x | **0.68x** |

Seven workers were slower than sequential on two of five workloads there. In the quieter run
above, 4 beat 2 on nearly every row — i.e. **the ordering reverses with machine load, and
neither sweep is a tuned optimum.** The default is `min(cores - 1, 4)`: the largest count
that was faster than sequential in *both* sweeps. It is deliberately conservative, and the
cap also keeps `wr-core` a reasonable library citizen for an embedder like `ct-research`
that runs its own parallel harness. `WR_SC_THREADS` overrides it; re-run `scbench` on an idle
machine before raising it.

---

## Honest limits of what was verified

* **The differential-gen gate does not exercise the pipeline.** Its generated queries are
  deliberately tiny and never reach 4,096 metastates, so all 10,000 ran the sequential path.
  It proves the refactor of `subset_construction` into `key_rows` + `mint_row` broke nothing;
  it says nothing about the threads. The **golden corpus is** the gate that exercises them:
  fixtures 179/230/261/286/293 each contain `subset_construction` calls of 15,697–115,802
  metastates, far above the threshold.
* **No cross-engine (vs JVM) claim is made here.** `scbench` is deliberately Rust-only,
  because `compare`'s JVM would contend for exactly the cores being measured. The Java
  comparison in `benches/STATUS.md` is untouched and was not re-run.
* **Every timing on this page was taken on a shared machine.** The coordinator's numbers
  supersede these.
* **`PIPELINE_MIN_STATES = 4096`, `PIPELINE_WINDOW = 1024`, `PIPELINE_MIN_LOOKAHEAD = 256`
  are reasoned, not swept.** They were chosen from the measured frontier widths (four to
  sixty times the window) and from thread spin-up cost, and no sensitivity analysis was run.

## What I would do next

1. **Re-sweep on an idle machine** — thread count, the three constants, and fixture 295.
2. **Shrink the sequential half.** The mint step was measured at 37–48% of
   `subset_construction`'s time, which is the Amdahl bound the pipeline is working against.
   Much of it is not really sequential: `metastate_to_id`'s hash is a pure function of the
   key and could be computed on the worker (needs `hashbrown`'s raw-entry API or an
   equivalent), as could the two key clones on the new-metastate path. That is the single
   highest-value follow-up.
3. **Recycle `KeyRows`.** Workers allocate two `Vec`s per metastate; a free-list back to the
   pool would remove ~200,000 allocations on a 100,000-state construction.
4. **Leave the DAG lane alone** unless a workload appears whose profile is genuinely wide —
   `dagwidth` is kept so that is a measurement, not a guess.
