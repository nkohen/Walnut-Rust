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

## Peak state count, and the one honest caveat

`CLAUDE.md` names state blow-up, not raw speed, as the decision procedure's dominant cost axis,
so wall clock alone would miss what actually decides whether a research query is tractable. Both
peaks are read out of each engine's own `details` trace (`::`-suffixed command, a **separate,
untimed** pass — writing the trace is real I/O and has no place in a timing loop) by one parser:
the largest `N state(s)` in the trace, skipping `Progress:` lines, which are a running counter
inside one traversal rather than the size of an automaton.

**The two traces are not equally complete, and the report says so.** Java's covers the whole
computation, including every `wr-core`-level step. The port's covers only the `wr-logic`-level
steps: threading `&mut Logging` through `wr-core`'s product/determinize/minimize/quantify is a
**known, pre-existing** gap — it is the root cause of seven of the golden corpus's nine
remaining divergences (`tests/golden/STATUS.md`, the "U28" follow-up) — and closing it is a
production change U32 deliberately does not make. So the JVM's number is the authoritative peak
and the port's is a lower bound; the report labels the columns that way rather than presenting a
logging gap as a difference in the computation. Both engines are separately proven to compute
the same language on every workload, so the authoritative peak characterises both.

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

## A note on the corpus's size distribution

Walnut's integration suite is designed to finish (675 fixtures inside its own
`MAX_TOTAL_SECS = 1800`), so it is not a worst-case-blowup benchmark and this crate does not
pretend otherwise. The set above deliberately spans four orders of magnitude — from a
sub-millisecond parse-and-dispatch to a multi-second decision procedure — so the report can
separate *per-command overhead* from *algorithmic throughput*, which are the two things a
"faster than Walnut" claim can mean.
