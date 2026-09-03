# The three-lane parallelization experiment (2026-09-02, user-authorized)

Three Opus agents, isolated worktrees, one question each. Branches (none merged; the
no-parallelism freeze still stands for `perf/beyond` until the user promotes a lane):
`agent/par-max` (outcome-correctness bar), `agent/par-det` (byte-identity bar),
`agent/par-posthoc` (racy compute + post-hoc reconstruction). Per-branch evidence lives in
each branch's own STATUS docs; this file records the cross-lane findings and the
coordinator's clean four-config benchmark.

## Convergent findings (each reached independently)

1. **The eval-DAG axis is dead.** Measured ceiling with infinite processors: 1.030× across
   all 533 corpus `compute()` calls (1.001× on the ≥100 ms calls). The DAGs are wide but
   empty — superexponential alternation concentrates 95–99.8% of each heavy query in its
   outermost elimination. Independently corroborated via the `!Send` type structure and
   `DeterminizeContext` index-ordering hazards.
2. **`subset_construction`'s frontier is the real axis** (25k–61k pending metastates on the
   big fixtures), and `Fa` is `Sync` for free.
3. **Byte-identity costs almost nothing on this axis.** All three lanes — including the two
   allowed to break it — shipped bit-identical output (ordered merge / sequential minting /
   post-hoc renumber). Zero tests ignored on A and B; C's deferred mode ran the whole corpus
   with unrepaired racy numbering and the write-path `canonize()` still reproduced every
   artifact — strong evidence the writer is the true fidelity boundary.

## The clean benchmark (coordinator-run, sequential, quiet machine, min of 2 run-medians;
## heavy rows single run at the P3 provenance settings; control = perf/beyond HEAD 9525894)

| fix | control | par-max | par-det | posthoc | control vs Java |
|---:|---:|---:|---:|---:|---:|
| 293 | 28.5ms | **1.70×** | 1.19× | 1.42× | 2.98× |
| 179 | 95.0ms | **1.87×** | 1.41× | 1.65× | 3.29× |
| 266 | 27.4ms | **1.36×** | 1.12× | 1.18× | 3.23× |
| 230 | 472.5ms | 2.07× | 1.80× | **2.60×** | 3.88× |
| 295 | 54.6ms | **1.33×** | 1.10× | 1.02× | 2.82× |
| 261 | 82.1ms | **1.48×** | 1.15× | 1.04× | 3.15× |
| 286 | 146.2ms | **1.39×** | 1.06× | 0.95× | 2.90× |
| alt3 | 1.49s | **2.03×** | 1.61× | 1.70× | 5.05× |
| rsp1 | 3.17s | 2.02× | 2.07× | **2.29×** | 3.91× |
| rsp2 | 4.89s | 2.16× | 2.24× | **2.90×** | 4.06× |

(521/637/1/207 flat everywhere — no big SC on their paths. Java-side cross-config sanity:
worst drift +15% on µs-scale fixture 1 only; ms-scale Java columns agree.)

**Vs Java, the parallel branches reach 7–12× on the research-shaped heavy rows** (alt3 on
par-max: 10.3×; rsp2 on posthoc: 11.8×; 230 on posthoc: 10.1×) — the campaign's >10×
aspiration becomes reachable on exactly the workload class it was aimed at, with the
explicit caveat that this is parallel-Rust vs sequential-Java (Java Walnut has zero
threading constructs in its main tree) and must always be reported as such.

## Cross-lane verdict

- **par-max (A)** is the best all-rounder (broadest corpus wins, 2.0–2.2× heavy) — rayon
  level-parallel expansion, ordered merge, bit-identical, zero relaxations used. 28%
  work-stealing spin observed → tuning headroom.
- **par-det (B)** proves the zero-dependency shape works but its speculative pipeline
  (sequential minting = 37–48% of SC) trails A on corpus; matches A on the heaviest rows.
  Its DAG-width measurement is the experiment's most decisive negative result.
- **posthoc (C)** wins only where frontiers are huge (230/rsp1/rsp2 best-in-class; 286
  regresses 0.95×) — per-call spawn + reconstruction overhead needs big work to amortize.
  Its lemma pair (canonicalize is permutation-invariant; identity on sequential SC output)
  is the lasting scientific contribution.

**Recommendation:** promote A's design as the basis for a real, fully-reviewed unit
(threshold-gated like A's `Schedule::Auto` so small queries never pay), stealing C's
biggest-frontier scheduling observations for the giant-construction regime; keep B's
measurement harness and its DAG-death evidence as the record of why operation-level
parallelism is not pursued. Open questions for the real unit: rayon vs a persistent
in-crate pool; pool sizing on 4P+4E; the `wr-cli`-embedder story (a library consumer may
not want a global pool).
