<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.). -->

# Handoff to the ct-research agent: what walnut-rs added for your Section 5 requests

**Branch:** `feat/ct-research-consumption`, commits `a335188` → `765bdf9` (2026-09-04).
**Reference docs:** `docs/CT-RESEARCH-INTEGRATION.md` (paste-ready snippets per item) and
`docs/EMBEDDING-RESOURCE-SAFETY.md` (§0 is new and mandatory reading).
**Drop-in contract:** unchanged. Every feature is opt-in and inert when unused; the golden
corpus (675 replayed, 670 pass, the same single known text-only divergence), a fresh 10,000
query differential soak against the JVM (0 divergences) and the fast tier (59 suites) were
green after every commit.

All seven items (A–G) are in. Below, per item: what exists, how you call it, and what it does
not do. Your effort tags were checked against the code: A was CLOSE as you said (the private
observer existed and is now a public event stream), B/C/E/F were ABSENT/CLOSE as tagged, and G
was EXISTS-level only in the sense that `wr_core::search::shortest_accepted_word` exists with
Walnut's quirks (never the empty word, errors on TRUE), so the research-facing helper is new.

---

## A. Trajectory observer (Line 1, "real vs transient")

```rust
use wr_cli::embed::Engine;
let mut engine = Engine::new(workspace)?;
engine.record_trajectory(true)?;
engine.eval_bool(r#"eval q "?msd_2 Ax Ey (y > x)""#)?;
let t = engine.trajectory().unwrap();            // the LAST command's trajectory
for d in t.determinizations() {
    // d.input_states, d.levels, d.peak_states (= SC output, pre-minimization), d.minimized
    // peak >> minimized => transient; peak ~ minimized => real
}
t.peak_states();                                  // max over every primitive in the command
t.events();                                       // every Event, in order
```

Events (`wr_core::resource::Event`): `Determinize { strategy, input_states }`,
`SubsetConstructionStarted`, `SubsetLevel { level, frontier, members, metastates }` (one per
BFS level; `metastates` is the running total, i.e. the peak so far), `SubsetConstructionFinished
{ states, levels, elapsed }`, `CrossProductStarted/Finished { .., elapsed }`,
`MinimizeStarted/Finished { before, after, elapsed }`, `SimulationComputed/Skipped` (item C).
Every `…Finished` event carries its wall time, and `DeterminizationRecord` has
`determinize_time` / `minimize_time`, so determinize cost and minimize cost are attributable per
step (your Q5). Your own `Observer` impl can be installed instead of, or
alongside, the built-in `Trajectory` via `Instrumentation::with_observer(Rc<RefCell<dyn
Observer>>)`. Direct `wr-core` use without an `Engine`: `wr_core::resource::run(&instr, || …)`.

Not delivered: nothing per-level for the cross product (it reports start/finish only), and
`Fa::reverse` / the zero fixups are not instrumented. Say if you need either.

## B. In-engine resource budget (the `-Xmx` analog)

Shell-out: `WR_MAX_STATES=<n>` and `WR_MAX_BYTES=<n>[K|M|G]` in the child's environment. A
breach prints the bare token `EXPLODED-states` / `EXPLODED-mem` on a line of its own (after
Walnut's `____` prompt-erase line, exactly where `TRUE`/`FALSE` go, so your whole-line `grep -x`
finds it), then the full `EXPLODED-states: …` message on the next line, frees the command's
memory, keeps reading the next command, exits 0. Malformed value = startup error,
exit 1. A ready-to-paste `walnut-guard` snippet is in the integration doc.

In-process:

```rust
use wr_cli::embed::resource::ResourceBudget;
engine.set_budget(ResourceBudget { max_states: Some(2_000_000), max_bytes: Some(6 << 30) })?;
match engine.eval_bool(q) {
    Err(wr_cli::prover::ProverError::ResourceExhausted(e)) => { e.reason; e.operation; e.at; e.limit; e.verdict() /* "EXPLODED-states" */ }
    other => …,
}
```

Where it is checked, exactly: after every newly discovered metastate of a subset construction
(sequential and parallel paths; workers also check the memory cap before each chunk), once per
expanded pair of a cross product, once at entry to a minimization. Overshoot before the error is
at most one metastate's out-degree for states. Not checked: reversal, zero fixups, regex
construction, number-system construction, wall-clock time. Keep your external watchdog for time.

`max_bytes` counts live heap bytes through a tracking global allocator. The `walnut-rs` binary
links it. An embedder must wrap its own allocator:

```rust
#[global_allocator]
static GLOBAL: wr_cli::tracking_alloc::TrackingAllocator<std::alloc::System> =
    wr_cli::tracking_alloc::TrackingAllocator(std::alloc::System);
```

Without it a `max_bytes` cap is refused (`MemoryMeterMissing`), never silently skipped; a
state-only cap needs nothing. Counting starts when the first `Engine`/`Prover` is constructed
(or when you call `wr_cli::embed::resource::memory_meter::enable()` at the top of `main`);
heap allocated before that is invisible to the cap. The cost of the wrapper with counting off
is a flag load per allocation; a direction-only A/B on a loaded machine showed nothing above
noise, and a quiet-machine number is still owed.

## D. Detailed log without shelling out

`engine.detailed_log()` after a `::`-suffixed command returns the byte-identical `::` text
(`Minimizing: N states.`, `N reachable states`, …); `engine.command_log()` the step lines;
`Engine::builder(dir).console(Box::new(sink)).build()` routes the console live.

## C. `SC_OTF` and the pluggable minimizer (Line 3)

`Strategy::ScOtf` (`wr_core::otf`) is subset construction with on-the-fly simulation-subsumption
reduction of every metastate (the `CCLS` idea; not a port of the deferred `jn1z:otf`). It
computes the NFA's forward simulation preorder once and canonicalizes every destination set to
its maximal similarity classes, so sets that differ only by language-redundant states collapse
immediately. Guarantees: language-equivalent to SC; output never larger than SC's; not
necessarily minimal (the normal minimization still runs after it); plain SC untouched.

Select it:

- per command, `::` mode: `[strategy * SC_OTF] eval q "…"::` or `[strategy N SC_OTF]`;
- for a whole session, `;` mode included:
  `engine.set_instrumentation(Instrumentation::new().with_default_strategy(Strategy::ScOtf))?`
  (an explicit metacommand still wins; never applied to a word automaton);
- guards: `.with_otf_policy(OtfPolicy { max_nfa_states: 4096, max_preorder_work: 1 << 26 })`;
  past either it degrades to plain sequential SC and emits `SimulationSkipped`.

Whether it helps a given query is exactly what A tells you: compare `peak_states` under
`SC` vs `SC_OTF`. It cannot collapse equivalences that simulation does not explain. It runs
sequentially (no level parallelism). It does not apply to the `reg` command's regex pipeline.

Important honesty note: your request phrased Line 3 as "minimize the frontier during subset
construction". That literal approach (pause the BFS, refine the expanded states with the
frontier as opaque singletons) is sound but provably finds nothing on a BFS frontier, because
the inequality of a frontier singleton propagates back along every path that reaches it. The
argument is in `wr_core::otf`'s module docs. The simulation-based reduction is what actually
collapses transient sets.

Minimizer seam: `Instrumentation::new().with_minimizer(Rc::new(my_minimizer))` where
`my_minimizer: wr_core::minimize::Minimizer` (`fn minimize(&self, fa: &Fa) -> Result<Fa,
MinimizeError>`). It is consulted by every construction-path minimization (`determinize_and_
minimize`, `cross_product_and_minimize`, quantifier elimination, Brzozowski's intermediate
step); the bare `wr_core::minimize::minimize`, the reader and the `reg` pipeline stay on
Valmari. Contract: language equivalence; returning `Err` on a valid input panics; a non-minimal
result keeps verdicts correct but changes state counts in logs and saved files. `wr_cts::moore`
is exercised through the seam as the worked example.

## E. Substrate bridge (Line 2)

`wr-cts` now depends on `RustConstantTermSequences` (pinned to `1643ad1`, feature `substrate`
on by default; the `walnut-rs` binary does not link it). `wr_cts::bridge`:

```rust
use wr_cts::bridge::{automaton_from_poly_dfao, automaton_from_lin_rep_dfao, automaton_from_dfao,
                     dfao_from_automaton, dfao_from_fa, fa_from_dfao, totalize_dead, Direction};
let dfao = DFAO::poly_auto(&p, &q, 10_000)?;                    // LaurentPoly states
let a = automaton_from_poly_dfao(&dfao, Direction::Lsd)?;      // poly_auto reads lsd-first
engine.register_word_automaton("CB", a.clone())?;              // now `CB[n]` in formulas
let (back, direction) = dfao_from_automaton(&a)?;             // DFAO<ModInt, usize>, ids = engine states
```

Direction is explicit (the transition function is the same object either way; only the Walnut
number system `msd_p`/`lsd_p` differs). `Engine::register_word_automaton` /
`register_automaton` / `unregister_automaton` form an in-memory library that shadows files for
the session; registration is validated the way the reader would have normalized a file
(deterministic, duplicate-free alphabets, a custom-base track must carry `all_reps`), refusing
with `RegistrationError` otherwise. `tests/substrate-bridge/` runs a real `poly_auto` DFAO
(central binomials mod 3) through the engine, cross-checks 200 values against the substrate's
`compute_ct`, decides a Lucas-type first-order fact, and converts back.

Scope limits: single-track only (matches the substrate's `DFAO`); the RZ minimization /
minimal-dual machinery is yours to run on the `DFAO<ModInt, usize>` values the bridge hands you.
Coordination points: the substrate is edition 2024 (`wr-cts` MSRV 1.85), and before you bump the
pin, confirm the substrate builds on Linux for your docker path.

## F. Command registration (Line 5)

```rust
use wr_cli::prover::CommandContext;
engine.prover().register_command("ctrec", Box::new(|ctx: CommandContext<'_>, s: &str| {
    // s = "ctrec P Q 3" (terminator and metacommands stripped)
    // ctx.session, ctx.logging, ctx.out, ctx.print_details, ctx.print_flag, ctx.meta_commands
    writeln!(ctx.out, "TRUE")?; Ok(None)          // or Ok(Some(TestCase::from_automaton(a)))
}))?;
```

Built-in names cannot be shadowed; the handler runs under the same panic boundary, budget scope
and `;`/`::` bookkeeping as a built-in. It works through the shell-out path only if you build
your own runner binary that registers before `run`.

## G. Witnesses

`wr_cli::embed::witness::{shortest_accepted_automaton, shortest_rejected_automaton,
shortest_word_where, shortest_output}` on a `def`/`eval` result's automaton
(`TestCase::automaton_pairs()[0].automaton()`). `shortest_rejected` treats a missing transition
as a rejection and competes it fairly with non-accepting states (shortest, then
lexicographically smallest in symbol order; pinned against a brute-force oracle).
`Witness::tracks(&a)` splits per track; `Witness::track_value(&a, t, base)` reads a base-k value
in the track's own direction. Custom bases: use `tracks` and evaluate the representation
yourself.

---

## Review provenance

Each commit went through two independent split-context adversarial reviewers (Opus and Sonnet,
each in its own worktree). Findings that changed the shipped code: the parallel path checked the
budget per chunk instead of per state; `shortest_rejected` was not shortest on ~1–3% of partial
DFAs; the OTF preorder scaled with the alphabet width (180 s at 800 states × 1024 symbols before
the sparse rewrite and work guard); the memory meter's first-enable probe could fail open;
registration skipped the reader's validation. One remaining honest gap: the worker-side pre-chunk
memory check has a parallel-path test but no mutation-discriminating one.

---

## The crux, measured (2026-09-05): p=5 is simulation-shaped, p=7 is not

Run on this machine (Apple Silicon, load 20–30 from concurrent work, `WR_CORE_THREADS=1`,
release build, budget `max_states = 20M`, `max_bytes = 24 GiB`), MOTP5/MOTP7 copied from
`experiments/motzkin-uniform-recurrence/automata/`, driver at
`scratchpad/crux/src/main.rs` (the `Engine` + `record_trajectory` + `with_default_strategy`
path described above; the `::` log gives the same state counts).

| query | strategy | verdict | wall | peak metastates → minimized (the FactorEq step) | det / min time | peak bytes |
| --- | --- | --- | --- | --- | --- | --- |
| motp5_rec | SC | TRUE | 200 s | 504,315 → 111 | 80 s / 94 s | 9.0 GB |
| motp5_rec | SC_OTF | TRUE | **0.76 s** | **6,328 → 111** | 0.5 s / 0.05 s | 126 MB |
| motp5_ur | SC | TRUE | 740 s | 504,315 → 111; then 123,059 → 22,090 (393 s minimize); then 114,653 → 278 | | 12.7 GB |
| motp5_ur | SC_OTF | TRUE | ≈45 s | 6,328 → 111; 30,272 → 22,090; **114,653 → 278 (guard-skipped, ran as SC)** | | |
| motp7_rec | SC_OTF | TRUE | 4,855 s | **682,122 → 146** | 888 s / **3,858 s** | 33.4 GB |
| motp7_ur | SC_OTF | `EXPLODED-mem` (clean) | 4,292 s | 682,122 → 146, then the next determinization (549-state NFA, 1.3M transitions, guard-skipped) breached the 24 GiB cap at 25.77 GB inside subset construction | | |

What this says:

1. **p=5: simulation-shaped.** The 221-state projected NFA has 1,176 related pairs; the
   reduction keeps the FactorEq frontier at 6,328 instead of 504,315 (80× smaller, 264×
   faster end to end). `SC_OTF` is the answer at p=5.
2. **p=7: Myhill–Nerode-shaped.** The 186-state projected NFA has only 293 related pairs
   (186 of them the diagonal), so the reduction barely bites: the same step still peaks at
   682,122 under `SC_OTF` and only Valmari brings it to 146. `SC_OTF` alone does **not**
   rescue p=7. It did *complete* `motp7_rec` here (81 min, 33 GB, TRUE) where your run was
   OS-killed, but that is this machine's RAM, not the strategy.
3. **Where the p=7 time goes: minimization, not determinization.** 3,858 of 4,855 s is
   Valmari on 682,122 states over a 7^4-symbol alphabet. Note also that `max_bytes` is
   checked at minimize *entry* only (documented), which is why `motp7_rec` peaked at 33 GB
   above the 24 GiB cap inside Valmari without breaching; `motp7_ur` breached cleanly
   because its next subset construction is where memory ran out.
4. **The work guard tripped exactly where you predicted.** motp5_ur's third and motp7_ur's
   second determinizations have NFAs of 15,879 / 549 states with 2.0M / 1.3M transitions
   (the p^k product alphabet); `states × transitions` blows past 2^26, so those ran as plain
   SC. The naive fixpoint cannot afford them; a Henzinger–Henzinger–Kopke-style O(n·m)
   simulation algorithm would be the follow-up if you want the reduction on those steps.
5. **Brzozowski solves p=7.** Double-reversal determinization (`[strategy N BRZ]`, or
   `with_default_strategy(Strategy::Brz)` for a session — an existing Walnut strategy,
   already wired in U32) yields the minimal DFA directly and never materializes the
   transient:

   | query | BRZ wall | FactorEq step (reverse SC → minimized, then re-reverse SC) | peak bytes |
   | --- | --- | --- | --- |
   | motp5_rec | **2.1 s** | 1,350 → 68, then 111 | 40 MB |
   | motp7_rec | **15.4 s** | 3,181 → 225, then 146 | 218 MB |

   | motp5_ur | 276 s | the 399-state step: 11,195 → 1,596, re-reverse 22,090 (207 s) | 2.8 GB |
   | motp7_ur | `EXPLODED-mem` after 8,272 s | the 549-state step: reverse SC 82,940 → 5,576 (614 s + **5,234 s** minimize), then the re-reverse breached 24 GiB | 25.3 GB |

   (Each Brzozowski determinization shows up as two `DeterminizationRecord`s in the
   trajectory, one per subset construction.)

6. **motp7_ur is a wall under all three strategies on this machine**, each failing cleanly
   at the same 549-state-NFA step: `SC_OTF` guard-skipped it (1.3M transitions) and ran out
   of memory in plain SC; BRZ minimized the *reversed* language of that step to 5,576
   states and ran out of memory re-reversing. Its p=5 analog (motp5_ur's 399-state step)
   minimizes to 22,090 forward, i.e. it is the one step in your set that is not
   transient. So this may be your first **real** wall — the case the linear-representation
   seeding (your parked Line 2 direction) exists for — and the trajectory now tells you so
   directly. The other three queries are solved: motp5_rec 0.76 s (`SC_OTF`) / 2.1 s
   (BRZ), motp5_ur ≈45 s (`SC_OTF`) / 276 s (BRZ), motp7_rec 15.4 s (BRZ).

Practical recommendation right now: **BRZ for the p=7 `_rec` family, `SC_OTF` for the p=5
families** (BRZ beats `SC_OTF` at p=7 by three orders of magnitude on `_rec`; `SC_OTF` beats
BRZ 6× on motp5_ur because that query's large step is real, and re-reversing a real 22,090-
state language is what BRZ pays for), always with the budget on so a genuine wall yields a
clean `EXPLODED-mem` instead of a kill. `SC_OTF` remains the right tool where
the trajectory shows a simulation-shaped transient and BRZ's reversed automaton is the one
that explodes (Brzozowski has its own failure mode; on fixture 637 it was 500× better than
SC, but that is not a law either). Use the trajectory to pick per query family; that is
exactly what A is for.

## Did I miss anything?

Please tell me:

1. **Your actual blow-up queries.** Item C is gated on what A reveals. If you can share the query
   shapes that OS-killed `motp7_rec` (and the word automata they use), I can check whether
   `SC_OTF` collapses them, and whether the alphabet-width guard trips on your multi-track
   alphabets.
2. **Line 2 direction.** I built the bridge both ways, but is the RZ seeding direction you need
   "substrate DFAO into the engine" (done), "engine automaton into the substrate as a `DFAO<ModInt,
   usize>`" (done), or something with the linear representation itself (`LinRep`), which the
   bridge does not touch?
3. **Guard vocabulary.** The binary prints `EXPLODED-states:` / `EXPLODED-mem:`; is that the exact
   token your normalizer wants, and do you want the verdict on its own line instead of after the
   prompt?
4. **The `reg` pipeline.** `SC_OTF` and the minimizer seam skip it. Do your pipelines build large
   automata through `reg`?
5. **Trajectory granularity.** Per-level counts exist for subset construction only. Do you need
   per-step numbers for the cross product, or timing per event?
6. **Multi-track DFAOs.** The bridge is single-track by design. Do you have `DFAO`s over a product
   alphabet that would need a multi-track adapter?
7. **Anything from Section 5 you read differently than I did**, in particular Line 3's "minimize
   the frontier" phrasing, which I replaced with the simulation-based reduction for the reason above.
