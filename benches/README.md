# `benches/` — performance vs JVM Walnut (Phase 4, U32)

`docs/DESIGN.md` §8's Phase-4 exit criterion ends with *"faster than Walnut on the research
workloads"*. This crate is how that clause is measured. It is **measurement infrastructure
only**: it adds no hooks to `wr-core`/`wr-logic`/`wr-cli`, and nothing here is reachable from a
shipped crate.

The checked-in results of the last run live in [`STATUS.md`](STATUS.md).

## Running it

```bash
# The head-to-head (the deliverable). Needs the sibling walnut-java checkout + its fat jar.
cargo run -p wr-bench --release --bin compare

# One or two workloads only, with a smaller sample, for a quick check.
WR_BENCH_ONLY=637,286 WR_BENCH_ITERS=3 WR_BENCH_WARMUP=2 \
  cargo run -p wr-bench --release --bin compare

# Criterion (Rust side only): confidence intervals, saved baselines, regression checking.
cargo bench -p wr-bench
cargo bench -p wr-bench -- fixture-286
```

Prerequisites, identical to `tests/golden` and `tests/differential-gen`:

* the sibling `walnut-java` checkout (or `WALNUT_JAVA_DIR`), for the corpus **and** the two
  Phase-0 manifests;
* its fat jar, `walnut-java/target/Walnut-all.jar`
  (`./mvnw -q clean package -DskipTests -Pfat-jar`);
* a JDK 17+ (`WR_BENCH_JAVA_HOME` / `JAVA_HOME` / `/usr/libexec/java_home -v 17` /
  `/opt/homebrew/opt/openjdk@17/bin` / `PATH`, in that order).

Missing any of them is a **loud failure**, never a silent skip or a silent pass —
`CLAUDE.md`'s absent-oracle contract.

`cargo bench` and `cargo run --bin compare` are separate invocations from `cargo test`: nothing
in this crate runs in the fast tier except its own unit tests (the peak-state parser, the
statistics, the wire decoder, the workload table's self-check).

## Files

| file | what it is |
|---|---|
| `src/lib.rs` | workload table, session-tree/prelude setup, the Rust engine, the JVM client, `peak_states`, statistics, the cross-engine answer check |
| `src/bin/compare.rs` | **the head-to-head**: one identical warm-up + fixed-iteration loop on both engines, and the report |
| `benches/dispatch.rs` | the Criterion benchmark of the Rust side alone |
| `java/BenchDriver.java` | the JVM half — a throwaway driver compiled fresh against the jar on every run |
| `STATUS.md` | the checked-in results of the last full run |

`tests/differential-gen/java/DiffGenDriver.java` is deliberately **not** modified or reused:
that driver answers one query per round trip and wraps every query in `eval "<formula>";`,
neither of which works here (a fixture's command script carries its own metacommand prefix and
`::` suffix, and this harness has to repeat one command inside one warm JVM and time it *there*,
not over the pipe). `BenchDriver.java` is a new file next to it, in the same capture-recipe
idiom.

## Two measurements, on purpose

**Criterion** (`cargo bench`) does the Rust side alone: warm-up, statistical sampling, outlier
detection, and saved baselines, so a future `wr-core` change can be regression-checked against
a stored baseline. It cannot do the Java side — driving a child JVM inside Criterion's sampling
loop would put the pipe round trip in the measured region.

**`compare`** does the head-to-head with one methodology applied to both engines: the same
warm-up count, the same iteration count, and mean/median/min/max computed by the same code.
Its Rust column calls the *same* `RustEngine::timed_dispatch` the Criterion benchmark times, so
the two cannot drift apart.

Criterion's defaults (3 s warm-up, 100 samples) are deliberately overridden for anything above
~50 ms/iteration (`benches/dispatch.rs`'s `sampling`): 100 samples of the 7-second workload
would be a twelve-minute benchmark of one fixture.

## What makes the comparison fair

* **Warm on both sides.** The JVM runs the identical command several times in the same process
  before the clock starts, and times the dispatch with `System.nanoTime()` *inside* the driver;
  the Rust side is a `--release` binary doing the same. **No process startup is in either
  number** — comparing a cold JVM against a warm Rust binary would be the meaningless
  comparison this unit's plan explicitly warns against.
* **The same session state.** Both engines replay Walnut's own `IntegrationTest.initialize`
  prelude (19 commands, the `PRELUDE` constant `tests/golden` already owns) into their **own**
  byte-identical copy of the corpus's `Global` + `Session` library trees, then dispatch the
  fixture's literal command script. Two copies, not one: both engines write library files as
  they run, and a shared tree would let each read the other's output.
* **One session-lifetime `Prover` on the Rust side.** This is the subtle one, and it points
  *against* the Rust side if you get it wrong. Java's expensive per-session state is `static` —
  `NumberSystem.numberSystemHash` (`Automata/NumberSystem.java:85`) is a JVM-global
  `HashMap<String, NumberSystem>` — so `new Prover()` there still gets `msd_17`'s
  adder/comparator automata for free after the first query. The port keeps that cache on the
  `Session`, which the `Prover` owns, so a fresh `Prover` per iteration would rebuild every
  iteration exactly what Java built once. Measured, on fixture 207 (`?msd_17 a=37`):
  **0.31 ms** warm vs **119.5 ms** with a fresh `Prover` per iteration — a 390× swing, against a
  Java side that measures 0.41 ms either way. One long-lived `Prover` is also what the real
  `wr-cli` REPL and Java's own `Prover.mainProver` do, and nothing accumulates across commands:
  `parse_setup` rebuilds `MetaCommands` per command on both sides. `WR_BENCH_COLD=1` measures
  the other way round, for diagnosis only — it prints a banner saying so, because it is not the
  fair comparison.
* **`Prover.mainProver` is assigned on the Java side.** `DeterminizationStrategies.determinize`
  (`Automata/FA/DeterminizationStrategies.java:99`) reaches the current command's metacommands
  through the **static** `Prover.mainProver`, so a driver that dispatches on an unpublished
  local `new Prover()` silently loses `[strategy 6 BRZ]` and falls back to subset construction —
  which turned fixture 637 from a 65 ms Brzozowski run into a 24-second, 155,153-state one
  during this unit's bring-up. That is a measurement of the harness, not of Walnut.
* **Console I/O is muted on both sides.** The JVM driver redirects `System.out` into a null
  stream for its whole lifetime; the Rust engine gives `Logging`, the `Prover` and
  `SessionPaths` all `io::sink()`.
* **Correctness is checked before speed is believed.** Every workload's answer is compared
  across the two engines by `wr_core::equiv` **semantic language equivalence** (never
  structurally — `CLAUDE.md`'s prime directive) before any timing is reported, with the same two
  normalizations `tests/golden` and `tests/differential-gen` use (`sort_label()` on the port's
  automaton, `totalize(0)` on both). A disagreement aborts the run: a benchmark of two engines
  computing different things is worse than no benchmark.
* **Engines run one at a time**, never concurrently, so neither is measured under the other's
  CPU load.
* **Each engine runs the allocator it actually ships with.** Since U33 the port registers
  `mimalloc` as its `#[global_allocator]` in `crates/wr-cli/src/main.rs` (the shipped binary),
  and `src/lib.rs` registers the same one so `compare` and the Criterion bench measure the
  configuration a user actually runs rather than a different one. `#[global_allocator]` is a
  per-binary, link-time choice, which is why it is declared twice and in neither library.
  The JVM keeps its own nursery + generational collector, which is the thing being compared
  against. Before/after numbers, and the profile that motivated the change, are in
  [`STATUS.md`](STATUS.md).

## Peak state count

`CLAUDE.md` names state blow-up, not raw speed, as the decision procedure's dominant cost axis,
so wall clock alone would miss what actually decides whether a research query is tractable. Both
peaks are read out of each engine's own `details` trace (`::`-suffixed command, a **separate,
untimed** pass — writing the trace is real I/O and has no place in a timing loop) by one parser:
the largest `N state(s)` in the trace, skipping `Progress:` lines, which are a running counter
inside one traversal rather than the size of an automaton.

**The two traces are equally complete since U28** (2026-08-17/19, the `Logging`-threading
unit): the port now threads `&mut Logging` through `wr-core`'s product/determinize/minimize/
quantify, so its `details` trace covers the same construction steps Java's does. Empirically,
the two columns name the same largest automaton on every benchmarked workload (verified across
the 2026-09-02 campaign-baseline runs, all 11 fixtures — `benches/baseline-perf-campaign.txt`).
When U32 built this harness that was NOT yet true — the port's trace was a lower bound, and the
report said so; the two columns are kept (rather than collapsed into one) precisely because
their agreement is now a free per-run cross-check that the two engines walked comparably-sized
intermediates. A disagreement between them is worth investigating, not labeling.

## Why these fixtures

The workloads are **real fixtures from Walnut's own integration corpus**, loaded through the
same two Phase-0 manifests `tests/golden` replays (`test-manifest.json` +
`subset-filter.json`). That loader is *included* from `tests/golden/tests/support/mod.rs`, not
copied, so a benchmark can never silently drift onto a different fixture than Tier 1 compares.

Ten fixtures span the corpus's real size range, plus fixture 637. The "ms" column is the Rust
side's measured warm dispatch mean (`STATUS.md`); the ordering was originally picked from
`tests/golden`'s per-fixture times, which are much larger for the same ids — see the note below
the table.

| id | ms | why it is in the set |
|---:|---:|---|
| 1 | 0.40 | floor: a tiny closed-form `lsd_2` conjunction. Measures per-command overhead — parse, dispatch, small automaton construction — not algorithmic throughput |
| 207 | 0.31 | a larger base (`?msd_17`): `NumberSystem::new` eagerly builds adder/comparator automata over a `k³` alphabet |
| 293 | 110 | the smallest genuine word-automaton factor-equality query (period-doubling `P`), one quantifier |
| 521 | 79 | the `I` (infinitely-often) quantifier over `msd_10` — a different elimination path (`wr_core::infinite`) from `E`/`A` |
| 179 | 440 | a **multi-track** word automaton (`PFmsd[f][i+k]`) under two quantifiers, plus two `reg`-defined prelude predicates |
| 266 | 112 | nested `A`/macro-call structure over Rudin-Shapiro — mid-sized alternation |
| 230 | 2202 | deep alternation (`Ei At …`) over Thue-Morse with a `3*n` coefficient — the first workload where the decision procedure, not the plumbing, dominates |
| 295 | 182 | paperfolding factor-equality: a large cross product under one quantifier |
| 261 | 310 | Rudin-Shapiro factor-equality — 295's shape over a bigger word automaton |
| 286 | 496 | **the slowest fixture in the whole corpus** (`?lsd_2` Rudin-Shapiro trapezoidal factor-equality) — the closest thing Walnut's own suite has to a research workload |
| 637 | 92 | `[strategy 6 BRZ]`: the corpus's one **strategy-sensitive** fixture. Its sixth determinization is a 1,790-state NFA that only Brzozowski makes tractable |

> **Why these are much smaller than `tests/golden`'s per-fixture times.** Golden's clock covers
> the whole job, including reading the recorded expectation and running the Tier-1
> `wr_core::equiv` comparison — which on a large result automaton costs far more than the query.
> Fixture 261 is ~0.31 s of dispatch inside ~5.2 s of golden-run wall clock; 286 is ~0.50 s
> inside ~7.3 s. Nothing is wrong with either number; they measure different things, and only
> this one is a benchmark of the decision procedure.

Selection rules, all enforced at run time rather than trusted:

* every id must exist in the manifest and be subset-relevant (a DROP-scope id aborts the run);
* every fixture must be reachable from the **prelude alone** — none depends on an earlier
  fixture's output, so the benchmark does not have to replay 636 other fixtures to set up;
* no `msd_fib`/custom-base workload, because the cross-engine answer check parses the JVM's
  serialized automaton and a custom base would drag a base-resolution surface into the
  comparison that has nothing to do with speed;
* fixture 637 keeps its `::` suffix, because Java gates `[strategy …]` on detail printing — a
  `;` variant would silently benchmark subset construction instead.

Benchmarking all 675 fixtures is deliberately **not** the goal: Criterion is for repeated-sample
statistical timing, and corpus-scale one-shot replay is what `tests/golden` and
`tests/differential-gen` already do.

### The opt-in `sc637` row

`WR_BENCH_SC_VARIANT=1` adds one extra, **non-fixture** row: 637's formula with the
`[strategy 6 BRZ]` prefix removed, so both engines decide it by plain subset construction. It is
opt-in because it is not part of Walnut's corpus and it takes minutes per engine. It exists to
answer the question U32's prerequisite unit was built to make askable — *what does the strategy
metacommand actually buy?* — and the answer (`STATUS.md`) is a far larger factor than anything
in the main table.

### Why fixture 637 is a fair comparison at all now

It was not, until U32's prerequisite unit landed. `[strategy N NAME]` used to be parsed and then
discarded, so the port always used `SC`; benchmarking 637 then would have compared Rust's `SC`
(does not finish) against Java's `BRZ` (~65 ms warm), a ~300× artifact of strategy choice rather
than a measurement of either engine. With the metacommand wired end-to-end, both engines take
the same Brzozowski path on the same determinization, and the comparison means something.

### The opt-in `WR_BENCH_HEAVY` rows

`WR_BENCH_HEAVY=1` adds three extra, **non-fixture** rows — same idea as `sc637`, but built to
answer a different honesty question: the 11 corpus fixtures above top out at fixture 230's
~2.2 s (JVM), and `docs/PERF-CAMPAIGN-DISPATCH.md`'s "workload honesty" rule is explicit that a
>10× claim measured only on sub-second corpus fixtures is not the >10× the user asked for. These
rows exist so the campaign's headline speedup claims have at least one measurement on genuinely
heavier, research-shaped decision-procedure work.

**These are NOT corpus fixtures.** They have no `automaton{id}.txt` recorded by `walnut-java`'s
own test suite, so the "vs corpus" fidelity check the ten `WORKLOADS` rows get reports `n/a` for
all three — the mandatory cross-engine `wr_core::equiv` answer check still runs and still gates
the timing exactly as it does for every other row (see "What makes the comparison fair" above);
there is simply no third, independent "does this match Walnut's own recorded answer" check to
run alongside it, because Walnut's own suite never computed these formulas.

Why not just use existing corpus fixtures that already exercise deep quantifier alternation
(richness, privileged words, etc. — see the `def`s `golden::PRELUDE` loads from `Command
Files/*.txt`)? Tried first, and rejected on measurement, not assumption: every corpus fixture
that wraps `rudin_rich`/`rudin_priv`/`period_doubling_rich`/paperfolding's equivalents in an
extra quantifier (ids 254, 264, 268, 298, 302, 325, 326) measured **sub-2 ms** on both engines.
That is a real property of the corpus, not a mistake in this note: Walnut's own integration suite
is built to finish all 675 fixtures inside its own 1800 s total budget (see "A note on the
corpus's size distribution" below), so no fixture reaches the heavy end research work actually
lives at — the two outliers that do (230, 286) are already in the default `WORKLOADS` table.
`HEAVY_WORKLOADS` (`src/lib.rs`) instead composes the same prelude `def`s and word automata into
NEW, deeper `eval` strings, the same way `sc637` composes fixture 637's own formula into a new
one.

| label | JVM (mean) | Rust (mean) | speedup | peak states | shape |
|---|---:|---:|---:|---:|---|
| `alt3` | 7.49 s | 1.76 s | 4.26× | 326,396 | genuine E-A-E alternation depth 3 over Thue-Morse (`Ei At (... Ej ...)`) — fixture 230 is E-A, depth 2 |
| `rsp1` | 12.95 s | 3.82 s | 3.39× | 524,748 | fixture 230's exact E-A shape, Rudin-Shapiro instead of Thue-Morse, palindromic (coefficient-1) second condition |
| `rsp2` | 20.18 s | 5.76 s | 3.50× | 649,748 | same as `rsp1` with the second condition's coefficient raised 1→2 — the set's heaviest row |

(Measured 2026-09-02, `WR_BENCH_HEAVY=1 WR_BENCH_ITERS=3 WR_BENCH_WARMUP=2`; the same run these
numbers and `src/lib.rs`'s `why` strings both quote, so the two never drift apart — not a
committed baseline in its own right, these rows are diagnostic, not part of the frozen campaign
table.)

**`alt3`'s formula was fixed during this table's review** (2026-09-02). The formula this row
originally shipped with,
`` Ei At ((t<n) => (Ej (j<t) & (T[i+j]=T[i+j+n]) & (T[i+t]=T[i+t+n]) & (T[i+t]=T[i+3*n-1-t]))) ``,
parses with the inner `Ej` scoping the ENTIRE conjunction (quantifiers are this grammar's lowest
precedence), so at `t=0` the guard `j<0` has an empty domain over the naturals and the whole
implication is vacuously false there — making `At` fail for every `n>=1` and the computed
language exactly `{n=0}`, confirmed against the real jar (a single-state automaton, `0 -> 0`).
Two candidate fixes that widen the inner guard (`j<=t`, `j<n`) were tried and rejected on
measurement: both let the witness `j=t` trivially satisfy the inner conjunct (since
`T[i+t]=T[i+t+n]` is already required separately), which collapses the whole formula to fixture
230's own E-A shape — confirmed by both candidates independently landing on the SAME 3-state
automaton and the SAME 115,802 peak states as fixture 230 itself, i.e. genuinely no E-A-E depth
left. The fix kept is narrower: `(0<t) & (t<n)` as the antecedent, excluding only `t=0` (the one
value where `j<t`'s domain is empty) and otherwise leaving the original `j<t` guard alone, which
is non-trivial for every `t>=1` precisely because it strictly excludes `j=t`. Verified against the
real jar: a genuine 4-state automaton (not one of the two degenerate one-state
all-accept/all-reject languages), engines agree, 326,396 peak states (matching this row's
already-recorded peak — the intermediate construction cost was never the trivial part), JVM mean
in the 3-30 s band. See `src/lib.rs`'s `HEAVY_WORKLOADS` for the exact formula and its full
`why` string.

`alt3`'s own execution model, stated precisely (an earlier draft of this note overclaimed):
the inner `Ej` elimination runs exactly ONCE, in the postfix evaluation of that one subformula —
it is not re-run per branch of anything. What makes the row expensive is what happens to that
one automaton AFTER: its determinized result feeds into the outer `At`'s `¬∃¬` complementation
(a further complement + project + complement + determinize), and that result feeds into the
outer `Ei`'s own projection + determinize. The peak-state count is the size of the largest
intermediate this two-more-determinizations chain touches, not a count of how many times `Ej`
itself was eliminated.

**Rejected candidates, and why** (recorded so the next person doesn't re-run the same expensive
experiments): for the `rsp1`/`rsp2` design space, fixture 230's shape with BOTH `T` and `RS`
conditions combined in one alternation, RS at fixture 230's own coefficient-3 offset, and RS
under the same 3-level E-A-E alternation `alt3` uses — all three ran past ~100 s of Rust dispatch
alone with no sign of terminating in this campaign's own time budget. That was a **selection
rule applied during calibration** (an informal "give up around 60 s of Rust-side dispatch and
try a smaller variant"), not a mechanism this crate enforces at run time — nothing in `src/lib.rs`
or `src/bin/compare.rs` imposes any such cutoff; a workload that is genuinely this slow would run
to completion (or to `java_deadline`'s much larger "never hang" cap) like any other row. The
pattern across all the candidates tried: swapping `T` for `RS` inside an already-heavy shape is
not a safe *linear* scaling knob — RS costs far more than proportionally more per unit of
alternation/arithmetic depth than T does, for reasons this campaign did not investigate further
(out of scope for a workload-selection unit; a real finding for anyone tuning `wr-core`'s
RS-heavy code paths later). No candidate ever produced a cross-engine answer *disagreement*;
every rejection was on measured time alone.

**Budget for a full default-settings `WR_BENCH_HEAVY=1` run**, computed honestly from this
section's own re-measured numbers rather than quoted from an earlier, uncomputed guess (a
previous draft of this README claimed "~10 minutes per engine," with no arithmetic behind it —
dropped). Each row makes `2 + warmup + iterations` real dispatches per engine: one correctness
check (`rust.dispatch` / `java.bench(_, 0, 1, _)`), one untimed detail pass for the peak-state
trace (also one dispatch each), plus `warmup` throwaway and `iterations` timed dispatches. With
the DEFAULT settings (`DEFAULT_WARMUP=3`, and `default_iters` picks 5 for every row here, since
all three have `approx_secs >= 1.0`) that is `2+3+5 = 10` dispatches per row per engine — double
the 5 *timed* dispatches alone, which is the number a quick mental estimate is likely to stop
at (it is easy to forget that the untimed correctness check and detail pass cost exactly as much
as any other dispatch). Using this section's own JVM means (7.49 s / 12.95 s / 20.18 s):
`10 × (7.49 + 12.95 + 20.18) ≈ 406 s ≈ 6.8 min` of JVM dispatch time alone under default
settings — almost exactly double the `5 × (7.49+12.95+20.18) ≈ 203 s ≈ 3.4 min` a timed-only
estimate would give, because 10 real dispatches per row is exactly double 5. Add the Rust side
(`10 × (1.76+3.82+5.76) ≈ 113 s ≈ 1.9 min`) and setup (JVM compile+boot+prelude ≈ 11 s, Rust
engine prep ≈ 4 s, both observed in this run) and a full default-settings `WR_BENCH_HEAVY=1` run
totals **≈ 8.9 minutes** wall clock, comfortably under 10 minutes but not by a wide margin — not
fast-tier, same as `sc637`.

## A note on the corpus's size distribution

Walnut's integration suite is designed to finish (675 fixtures inside its own
`MAX_TOTAL_SECS = 1800`), so it is not a worst-case-blowup benchmark and this crate does not
pretend otherwise. The set above deliberately spans four orders of magnitude — from a
sub-millisecond parse-and-dispatch to a multi-second decision procedure — so the report can
separate *per-command overhead* from *algorithmic throughput*, which are the two things a
"faster than Walnut" claim can mean.
