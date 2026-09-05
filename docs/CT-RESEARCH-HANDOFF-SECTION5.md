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
{ states, levels }`, `CrossProductStarted/Finished`, `MinimizeStarted/Finished { before, after }`,
`SimulationComputed/Skipped` (item C). Your own `Observer` impl can be installed instead of, or
alongside, the built-in `Trajectory` via `Instrumentation::with_observer(Rc<RefCell<dyn
Observer>>)`. Direct `wr-core` use without an `Engine`: `wr_core::resource::run(&instr, || …)`.

Not delivered: nothing per-level for the cross product (it reports start/finish only), and
`Fa::reverse` / the zero fixups are not instrumented. Say if you need either.

## B. In-engine resource budget (the `-Xmx` analog)

Shell-out: `WR_MAX_STATES=<n>` and `WR_MAX_BYTES=<n>[K|M|G]` in the child's environment. A
breach prints one line containing `EXPLODED-states: …` or `EXPLODED-mem: …` on stdout (after the
`[Walnut]$ ` prompt like every Walnut error, so match with `grep -o 'EXPLODED-[a-z]*'`), frees the
command's memory, keeps reading the next command, exits 0. Malformed value = startup error,
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
