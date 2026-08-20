# Walnut-in-Rust: Feasibility Proposal & AI-Orchestrated Build Plan

*Draft v1 — 2026-08-08. Author: exploratory scoping pass. Status: proposal for review.*

> **Provenance:** this design doc originated as a scoping note in the `ct-research` repo
> (`notes/walnut-in-rust-proposal.md`) and is now the founding design doc of `walnut-rs`. This copy is
> canonical going forward. Paths like `libs/Walnut/` and `libs/RustConstantTermSequences/` refer to the
> **ct-research** checkout where the upstream Walnut and the Rust substrate are vendored.

---

## 1. Executive summary (read this first)

**The goal.** Build a Rust reimplementation of the [Walnut](https://walnut-theorem-prover.github.io/) automatic-theorem-prover — the Java tool that decides first-order statements about automatic sequences — that is *faster*, *owned by you* (freedom to modify), *extensible for your research*, and above all **at least as trustworthy as Walnut — ideally with fewer implementation bugs**.

**What we're actually cloning.** A **research-driven subset**, but that subset *includes Walnut's entire first-order-logic-over-automata decision engine* (this is the must-have core): the formula parser, quantifier elimination (∃ by projection+determinize, ∀ by ¬∃¬), boolean/product operations, determinization, minimization, and base-*k* (msd/lsd) numeration. We **drop** only the peripheral numeration systems (Ostrowski, Fibonacci/Pell, negative bases) and CAS-export conveniences. This cuts the port surface roughly in half while losing nothing your constant-term-sequences research uses.

**The correctness strategy — the heart of the plan.** Your top priority is correctness — concretely, **fewer bugs than Walnut** (the *implementation* must not produce wrong math answers; the underlying algorithms are already trusted). The single most important idea, which you supplied, drives the whole design:

> **Fork Walnut (Java) → drive its unit-test coverage to ~100% on the subset → *then* replicate that suite in Rust.**

This converts Walnut's weakest asset (moderate unit coverage — 92 tests) into the **executable specification** for the port. Layered on top, the assurance ladder is:

| Tier | What it certifies | Walnut-independent? |
|---|---|---|
| 0. Fork + ~100% Java unit coverage on the subset | Behavior is pinned as an executable spec (and finds latent Walnut bugs) | — |
| 1. Golden integration corpus (~689 golden-output files, subset-filtered) | Real theorem-proving workloads reproduce, compared by *semantic automaton-equivalence* (Walnut's own bar) | No (agreement w/ Walnut) |
| 2. Port every Java test → Rust test | Rust matches the pinned spec | No |
| 3. Differential testing (Rust vs forked Java, randomized/fuzzed queries) | No behavioral divergence on any generated input | No |
| 4. **Property-based invariants** (e.g. minimize preserves language; ¬∃¬ = ∀; determinize∘reverse twice = minimal; ported Valmari minimizer agrees with the existing Moore minimizer) | The output is *mathematically* right, independent of Walnut | **Yes** |
| 5. Fuzzing + coverage | The long tail of parser/reader edge cases | Partly |

**The risk we are actually managing is IMPLEMENTATION bugs, not algorithm correctness.** The algorithms (subset construction, Valmari minimization, Brzozowski, ∃-projection) are textbook and already trusted; the danger is a faithless port of them. So: Tiers 0–3 make the clone *behaviorally agree with Walnut*, which delivers "**at least as good as Walnut**" — a high floor, since Walnut is already good (it would only replicate a Walnut implementation bug, if one exists). **Tier 4 is what lets you go strictly better** ("fewer bugs than Walnut"): property invariants check the output against the mathematics directly, catching an implementation bug *even if Walnut shares it*. Tier 4 is therefore mandatory. *(Formal proof of the algorithms is explicitly out of scope — it proves what is already trusted and says nothing about the code. The only heavier "formal" step that would add anything is machine-checked verification of the Rust code itself (Kani/Creusot/Verus); it is unnecessary overkill for this goal — noted in §5 as a footnote, not a plan tier.)*

**How we build it.** Mechanical port first (faithful Java→Rust transliteration preserving behavior, à la the [Bun rewrite](https://bun.com/blog/bun-in-rust)), refactored toward idiomatic Rust later — because a faithful port is what makes tight differential testing against the Java original possible. Execution is AI-orchestrated using an adversarial-review loop (implementer → two split-context reviewers → fixer). *(NB: only `.claude/agents/adversarial-reviewer.md` exists in this repo today; the fleet launcher and any extra review-agent roles must be ported/authored — a Phase 0 work item, ROADMAP §3. Do not assume ct-research's agents/fleet are present here.)*

**Rough size & cost** *(order-of-magnitude, scaled from Bun — validate with the Phase 1 spike, do not treat as measured):* the port surface is ~8–10k LOC of the 14k-LOC Java, and you already have a **~3k-LOC well-tested Rust substrate** (`RustConstantTermSequences`: a `DFAO` type, Moore minimizer, GF(p) linear algebra, and a single-track LSD serializer) that provides useful *primitives* — though narrower than a drop-in kernel (it's deterministic single-track only; Walnut needs multi-track + NFA, so the substrate needs generalization, not just adoption — see §4). Bun was 535k LOC for ~$165k of API in 11 days, but that is a **pessimistic** anchor — cost should scale *sub*-linearly for us (our surface is ~1.7% of Bun's; Bun paid for 64-way redundancy and 6 platforms we don't need; most of the correctness pipeline is deterministic scripts costing *zero* model tokens; and we run cheap-model-by-default with escalation only on the hard ~20%). Expect **low thousands of dollars of API**, capped per phase (§7). The real investment is the correctness *weeks*, not spend. Realistic calendar: **a spike in days; a differentially-green FOL decider over base-k in a small number of weeks; hardened + property-verified in a couple of months**, mostly gated on review depth, not typing speed.

**Recommendation.** Approve a **Phase 0 + Phase 1**: fork Walnut and stand up JaCoCo coverage on the subset modules *(Phase 0)*, and build a thin end-to-end spike — base-*k* DFA + one quantified `eval` query — differentially tested against Walnut *(Phase 1)*. Those two de-risk every number in this document before committing to the full build.

---

## 2. Goals & non-goals (from your decisions)

**Goals**
- Rust reimplementation you own, faster than the JVM Walnut, extensible for constant-term-sequences research.
- **Must-have:** Walnut's first-order-logic-over-automata decision procedure.
- **Correctness is priority #1:** as-well-tested-as-Walnut, preferably better; *no implementation bugs that yield wrong math answers* (fewer bugs than Walnut is a success).
- Testing to best-practice standards: ~100% unit coverage (achieved first in the Java fork, then replicated), differential testing, property-based testing, golden tests, fuzzing.
- Mechanical-port fidelity first (preserve behavior, then idiomatize).
- Delivered as an AI-orchestrated build (Bun-style adversarial loops + agent fleet).
- Measurable speed-up over Walnut

**Non-goals (for the initial subset)**
- Ostrowski / Fibonacci / Pell / negative-base numeration systems.
- ~~CAS matrix exports (Maple/Mathematica/Matlab/Sage) — Sage export can be revisited later.~~
  **Revisited and reversed 2026-08-19** (`.claude/plans/amber-transcribing-ledger.md`): CAS
  matrix export is now KEEP scope and ported (`crates/wr-io/src/matrix_writer.rs`). See that
  plan and `tests/golden/STATUS.md`'s "CAS incidence-matrix export — CLOSED" entry.
- Drop-in file-format identity for *every* Walnut command; we match the subset's commands exactly and skip the rest.
- Idiomatic-Rust elegance in v1 (deliberately deferred behind the mechanical port).

---

## 3. What we're cloning: the subset, concretely

Walnut is 13,803 LOC of Java across 76 files in 11 packages. The subset keeps the decision engine and drops the peripheral numeration/export surface.

**KEEP (the FOL decider spine — must-have):**

| Walnut piece (Java) | Role | Notes for the port |
|---|---|---|
| `Main/Predicate` + `EvalComputations/{Token,Expressions}` | Formula lexer + shunting-yard parser → AST | Hand-written, no grammar file; reproduce exactly, pin with golden corpus |
| `Main/Prover` (command dispatch) + `Session` | REPL / command router | Refactor `public static` globals into an explicit `Session` context struct |
| `Main/Commands/{EvalDef, Reg, Combine, Concat, Intersect, Union, Reverse, Star, Quotient, Morphism, Image, Alphabet, Describe}` | The subset's commands | `eval`/`def`/`reg` are the core; morphism/`image` needed for building automatic sequences |
| `Automata/Automaton` + `Automata/FA/FA` | Semantic wrapper + raw NFA/DFA/DFAO engine | `FA` is mostly-`static` (clean free-function port); **`Automaton` is genuinely instance-stateful** (holds `List<NumberSystem> NS`, `RichAlphabet`) — not a trivial free-function port (kit-finding #17) |
| `Automata/NumberSystem` (base-*k* only) | base-*k* adder/comparator/constant automata | **Lives in `wr-core`, not a separate crate** — `Automaton`↔`NumberSystem` are bidirectionally coupled (kit #1) |
| `Automata/Morphism`, `WordAutomaton`, `RichAlphabet`, `AutomatonDFA` | morphic-sequence support, multi-track alphabet, DFA specialization (~745 LOC) | **Added — missed by the first draft** (kit #2); `Image` (settled KEEP) imports `Automata/Morphism`. All → `wr-core` |
| `AutomatonLogicalOps` | and/or/xor/imply/iff/not, reverse, quotient | Boolean layer over product |
| `AutomatonQuantification` | ∃ = projection+determinize; ∀ = ¬∃¬ | The decision-procedure crux |
| `FA/DeterminizationStrategies` | Subset construction (`SC` — the **default** strategy) + Brzozowski + opt-in OTF | **Ship `SC` (the default) + plain Brzozowski; defer the opt-in OTF variants.** Note Brz calls the minimizer mid-algorithm and its `BRZ_CCL/CCLS` variants route through OTF — so "ship Brz, defer OTF" separates cleanly only for *plain* Brz (§9 F4). **Deferral decision confirmed 2026-08-08, no longer open — see §10.** |
| `FA/ProductStrategies` | cross-product + minimize | Peak-memory-sensitive hotspot |
| `FA/ValmariDFA` + `ValmariPartition` + `Trimmer` | Minimization (Valmari) | Near-linear; invoked constantly |
| `Automata/NumberSystem` (base-*k* paths only) | msd/lsd base-*k* addition/comparison automata | **Slice out only base-*k***; drop Ostrowski/Fibonacci/Pell/negative |
| `AutomatonReader` + `ParseMethods` + `Automata/Writer/AutomatonWriter` | `.txt` automaton format + Graphviz | The interop bridge — your Rust substrate *already emits this format* |

**TO CLASSIFY — a set of commands the earlier draft missed** (adversarial pass, confirmed): `split`, `rsplit`, `join`, `transduce`, `convert`, `minimize`, `fixleadzero`, `fixtrailzero`, `promote`, `inf`, `export` are dispatched **inline in `Prover.java`** (regex-matched, no `Commands/` class each). They are *used by the golden corpus* and some look directly relevant to constant-term work — `convert` (msd↔lsd / base conversion) and `minimize` almost certainly **KEEP**; `transduce` (e.g. RUNSUM running-sum transduction) plausibly KEEP; `split`/`rsplit`/`join`/`fix*` need a decision. **These were absent from the 8–10k LOC estimate and the crate layout — a real scope gap to close in Phase 0** by classifying each against the research need. (This is adversarial-review finding F2/F9.)

**Broader gap (kit-review #2):** the same "missed files" problem applies beyond `Prover.java` — the first draft enumerated only some of the `Automata/` package. **Phase 0 must run a file-by-file inventory of the *entire* `Automata/` directory** (and its subpackages), assigning each file KEEP/DROP and a crate, and produce a **verified Java-file → Rust-crate boundary map** (the walnut-rs analogue of Bun's `LIFETIMES.tsv`) — adversarially reviewed **before Phase 2**. This is the single highest-value pre-build artifact: a crate-boundary decision discovered mid-port forces rework across everything already built on the wrong boundary.

**DROP (initially):** `Automata/Numeration/Ostrowski` (491 LOC) + `NodeState`; Fibonacci/Pell/negative-base branches of `NumberSystem`; CAS matrix writers (Maple/Mathematica/Matlab/Sage — ~28 golden files); `Transducer`-heavy paths *only if* `transduce` is dropped. This removes the messiest ~god-class surface and the exotic-numeration edge cases — the two hardest things to port *exactly*. (`Transducer` was later confirmed KEEP — see the file table above. Fibonacci/Pell turned out to be a non-issue: they're plain custom-base data files running through the already-KEEP generic loader, not separate code — see [`docs/UNPORTED-SCOPE-SIZING.md`](UNPORTED-SCOPE-SIZING.md), which also ranks the genuinely-dropped items — CAS export, negative-base, `split`/`rsplit`, Ostrowski, OTF — by porting effort should any of them ever be revisited. **CAS matrix export itself was later un-dropped — see the non-goals list above.**)

**Net port surface:** ~8–10k LOC of Java, of which the agent survey judges ~80% "straightforward" (clean, static, mechanically portable) and ~20% risky (parser edge cases, `NumberSystem`, the external OTF/Brics/AutomataLib dependencies — see §7).

### Compatibility: what runs unchanged vs. what's deferred

Drop-in compatibility with existing Walnut input files is **structurally guaranteed for the subset**, because the correctness strategy *depends* on it: differential testing (Tier 3) and the golden corpus (Tier 1) both require the clone to consume identical command scripts and emit automata that are **semantically equivalent** (same recognized language) to Walnut's — which is exactly how Walnut's *own* test suite compares results (`EqualityUtils.faEqual`, via Brics language-equivalence), **not** byte/structural identity. So *command-semantics and language* fidelity is load-bearing; exact state-numbering/canonicalization is **not** required (chasing it would be wasted effort — see §5).

- **Runs unchanged (from v1):** any Walnut command file within the subset — base-*k* (msd/lsd) numeration, the FOL decider, `eval`/`def`/`reg`/`morphism`/`image`/the boolean & quotient ops, and the `.txt` automaton format (read *and* written; the existing Rust substrate already emits it). **Your own research scripts are all here** (base-*p*, word-automata / morphic sequences like the Wcii5/7 certificates).
- **Deferred (fails cleanly, not silently):** files using a *dropped* feature — Ostrowski/Fibonacci/Pell/negative-base numeration (`msd_fib`, `msd_pell`, …), CAS matrix exports, or a non-KEEP command. These return an explicit "unsupported numeration/command" error and are additive later extensions, never a redesign.
- **The Java source itself stays executable** throughout: Phase 0 keeps the forked Java Walnut running as the coverage oracle. Only the *Rust product* has the subset boundary.

---

## 4. What already exists to leverage

**In-repo Rust substrate — `libs/RustConstantTermSequences` (~3k src LOC, well-tested, lean deps).** Useful *primitives*, but narrower than the earlier framing implied — an adversarial pass (below) found the reuse was oversold, so here it is honestly:
- `DFAO<A,S>` — a generic **deterministic** automaton-with-output type. Matches Walnut's *word-automaton* (DFAO) shape, but is deterministic-only: **no NFA**, which Walnut's ∃-projection produces and requires. So it's a base for the DFAO/value side, not for the NFA/projection side.
- **Moore partition-refinement `minimize`** — a correct minimizer, but hardcoded to `DFAO<ModInt,S>` (base-*p* single-track) and only ever run on BFS-constructed automata. Usable as the Tier-4 *independent second minimizer*, **but only after generalization** to `wr-core`'s multi-track/NFA automata — not free.
- **GF(p) linear algebra** — Gaussian elimination, rank, row-basis, null space, subspace intersection (Zassenhaus), coordinate solve. Genuinely reusable numeric primitives (least caveated).
- lsd/msd evaluation + forward/reverse machines — base-*p* digit conventions match Walnut's.
- **Serializer** — emits a **single-track** `lsd_p` format (msd is a `replacen` string-hack, not a real msd writer); Walnut's real format is **multi-track** (e.g. `lsd_2 lsd_2 lsd_2 lsd_2`) with NFA/`T`/`F` cases. So it's a *starting point* for I/O, **not** the "ready-made interop bridge" earlier claimed — it needs real multi-track/NFA support.
- Solid, tested `ModInt` / `LaurentPoly` (with parser) / matrix/vector arithmetic — genuinely reusable.

**Gap (must be built):** no FOL parser, no quantifier projection, no automaton product/cross-product/determinization, base-*p* only, no NFA/Büchi. i.e. the *logic engine* is absent — the substrate is the "automaton + linear algebra" kernel, and we build the decider on top.

**In-repo test assets from Walnut (the crown jewel for correctness):**
- **Golden integration corpus** (`src/test/resources/integrationTests/`, ~1,100 files incl. subdirs): **~689 golden-output files** (`automaton*`=635, `details*`=15, `error*`=39) driven by fixed command scripts over Thue–Morse, Rudin–Shapiro, paperfolding, period-doubling, Fibonacci, morphism-image suites, **plus ~28 CAS-export files we drop**. Language-agnostic and reusable — *but not "directly": it must be filtered against the subset boundary first.* Measured: **~22% of the command scripts use numeration systems we drop** (`msd_fib`/`msd_pell`/negative bases), and several drive commands (`transduce`/`convert`/`split`/…) not yet classified (see §3). So the honest reusable acceptance suite is the **subset-filtered** slice (~65–75% of the golden-output files), not all of it.
- 92 JUnit `@Test` methods + ~30 hand-crafted `.txt` automaton/numeration fixtures + deliberately-malformed `bogus*.txt` error-path inputs.

**External references (not dependencies):** the [Walnut paper (Mousavi, arXiv:1603.06017)](https://arxiv.org/abs/1603.06017) and Shallit's book are the algorithmic spec of record — the definitive statements of the algorithms we're porting (which are themselves not in doubt; the port is what we test).

---

## 5. The correctness architecture (priority #1)

This is where the project succeeds or fails. The design principle: **stack independent oracles so that a bug must fool *all* of them to survive.** Behavioral oracles (vs. Walnut) catch port errors; *mathematical* oracles (property/formal) catch errors Walnut itself might share.

### Tier 0 — Fork Walnut, drive ~100% unit coverage on the subset *(your key idea)*
- Fork the Java repo; add **JaCoCo** coverage measurement scoped to the KEEP modules (§3). Do **not** chase 100% on Ostrowski/CAS code we're dropping — coverage of dead-for-us code is wasted.
- Write **characterization tests** (Feathers-style: assert current behavior, not desired behavior) up toward ~100% line+branch on the subset. This:
  1. produces the **executable specification** the Rust port is judged against;
  2. **surfaces latent Walnut bugs** (uncovered branches are where they hide) — each one is a decision point: replicate-the-quirk vs. fix-and-diverge (log every divergence explicitly);
  3. is itself independently valuable — a hardened Java Walnut.
- Deliverable: a coverage report + a machine-readable test manifest that the Rust suite consumes. **Manifest schema (pin it in Phase 0, kit #11):** a directory of fixtures, each an entry `{ id, command_script (the .txt input), expected_kind: automaton|details|error, expected_path, number_system, commands_used[] }` as JSON/TSV — enough for the Tier-2 replicator to be near-mechanical and for the Tier-1 harness to filter by `number_system`/`commands_used` against the subset boundary.

### Tier 1 — Golden integration corpus as acceptance
- Wire the **subset-relevant** slice of the corpus into the Rust build: feed identical command scripts, and compare emitted automata by **semantic language-equivalence** (the `faEqual`/Brics bar Walnut itself uses), with `details`/`error` text compared after normalizing timing/progress lines (exactly as `IntegrationTest.assertEqualMessages` does — it strips `\d+ms` and `Progress:` lines). Green corpus = the port reproduces real theorem-proving workloads. **This requires a semantic-DFA-equivalence oracle in `wr-core`** (product + complement + emptiness-of-symmetric-difference) — a first-class deliverable, not an afterthought (it's also reused by the decision procedure itself).

### Tier 2 — Replicate the (now ~complete) Java test suite in Rust
- Each Java unit/characterization test → a Rust `#[test]`. The mechanical-port fidelity makes this near-mechanical. This is the "as-well-tested-as-Walnut" floor, by construction.

### Tier 3 — Differential testing (Rust vs forked Java)
- A **query generator** (grammar-based random FOL formulas over base-*k* + random automata) runs both engines and asserts identical verdicts/automata. Add **fuzzing** (`cargo-fuzz`/AFL on the parser and automaton readers) with the malformed-input corpus as seeds.
- This is the highest-leverage catch for port divergence — it explores inputs no human wrote.

### Tier 4 — Property-based invariants *(the first Walnut-independent oracle — mandatory)*
- `proptest`/`quickcheck` asserting algebraic laws that must hold *regardless of what Walnut does*:
  - `L(minimize(A)) == L(A)`; `minimize` is idempotent; output-invariant on all inputs.
  - `L(determinize(A)) == L(A)`; determinize∘reverse∘determinize∘reverse yields the minimal DFA (Brzozowski) — cross-check against the direct minimizer.
  - Quantifier duality: `∀x φ ≡ ¬∃x ¬φ`; De Morgan across product; projection soundness.
  - Number-system laws: the base-*k* adder automaton computes real addition; comparator is a total order; msd/lsd agree after reversal.
  - **Cross-oracle:** the existing substrate's Moore `minimize` and the ported Valmari `minimize` must produce the same language on every generated automaton — two independent implementations agreeing is strong evidence.
- **This is the tier that lets you exceed Walnut** — it can catch an implementation mistake Walnut and the port *both* make, which is exactly how you end up with *fewer* bugs than Walnut rather than merely equal.

### Tier 5 — Fuzzing + coverage
- `cargo-fuzz`/AFL on the parser and automaton readers, seeded with the malformed-input corpus (`bogus*.txt`); track coverage to keep the long tail of edge cases honest. Cheap, standard best practice.

### Not in scope: formal verification of the algorithms
- Proving the *algorithms* (determinize/minimize/projection are language-preserving) in Lean/Coq proves something **already trusted** and — critically — says **nothing about whether the Rust code implements them faithfully**, which is the only risk here. So it is out of scope.
- The *only* formal step that would target the real risk is machine-checked verification of the **Rust code itself** against a spec (Kani bounded model-checking, Creusot, Verus, Prusti). That is a heavy, separate undertaking and is **unnecessary overkill** given the goal is "fewer bugs than Walnut, well-tested" and Walnut is already good. Mentioned only for completeness; not a plan tier. *(If it were ever wanted, design the kernel as pure functions now so it stays cheap to bolt on later — a free hygiene choice.)*

### The assurance boundary, stated plainly
- Tiers 0–3 ⇒ **behaviorally equal to Walnut** (would replicate a Walnut implementation bug, if any — a high floor since Walnut is good).
- Tier 4 ⇒ **mathematically checked** on generated cases → **fewer bugs than Walnut** (catches bugs Walnut shares).
- Tier 5 ⇒ edge-case hardening.

All five are mandatory best practice; there is no expensive optional capstone.

### Test-performance guardrails (the superexponential-cost discipline)

Walnut's decision procedure is worst-case **superexponential**, and the real driver is **quantifier alternation**: each `∃` is a projection to an NFA followed by a `determinize` (subset construction — exponential when the NFA is genuinely nondeterministic), and alternation stacks these. (`∀` is compiled to `¬∃¬`; note the `not()` calls run `determinize` on already-*deterministic* input — cheap/linear, singleton metastates — so it's the projection-`determinize` per alternation level that explodes, not "2× per `∀`" as an earlier draft imprecisely stated.) Tests — **end-to-end tests most of all** — must be engineered so they actually finish. This repo's existing discipline (`bin/walnut-guard`, smoke-first, per-item caps, breadth-before-depth) transfers directly:

- **Bound the generators — generate *small*.** Differential (Tier 3) and property (Tier 4) tests generate automata with few states, small base/alphabet, and shallow quantifier-alternation depth. A minimizer/projection/quantifier bug manifests on a 5-state automaton exactly as on a 5000-state one, for a millionth of the cost. Small inputs are where bugs live, not a coverage compromise.
- **Differential testing needs *both* engines to finish.** The generator's size bounds are set by the slower engine (the JVM Walnut), and any case whose projected cost (state count / alternation depth) exceeds budget is **never emitted**. A divergence on a small case is fully diagnostic.
- **Per-test resource caps, never hangs.** Every end-to-end test carries an explicit wall-time + peak-state + memory cap and yields a `TIMEOUT`/`EXPLODED` verdict (mirroring `walnut-guard`) instead of hanging the suite. An over-budget case is recorded as `skip-too-big` **with its cap, visibly** — a silent drop reads as "covered" when it isn't (a logged repo lesson).
- **Two test tiers.** A **fast tier** (tiny inputs, seconds/commit — unit tests + most property tests) and a **gated slow tier** (heavier golden-corpus workloads + larger differential cases, each capped, sharded across background workers under the heartbeat harness, cost-modelled before launch). The golden corpus is inherently tractable (it's workloads Walnut actually completes), but we still cap and shard the heavy ones.
- **A performance-regression tier.** Since "faster than Walnut" is a goal, track per-query peak-state-count and wall-time against Walnut on a fixed bounded benchmark set — this simultaneously catches perf regressions *and* accidental state blowups introduced by a port bug.

---

## 6. Architecture of the Rust clone

**Crate layout (workspace):**

```
walnut-rs/
  crates/
    wr-core/        # WHOLE Automata/ package: FA, Automaton, NumberSystem (::numsys), Morphism, WordAutomaton,
                    # RichAlphabet; determinize, minimize, product, reverse, quotient, + language-equivalence oracle.
                    # numsys is a MODULE not a crate — Automaton<->NumberSystem are coupled (kit #1).
    wr-logic/       # Token/Expression AST, shunting-yard parser, quantifier elimination, boolean ops
    wr-io/          # AutomatonReader/Writer: Walnut .txt format (+ Graphviz); interop bridge
    wr-cli/         # Prover/Session command dispatch, REPL
    wr-cts/         # thin adapter re-using libs/RustConstantTermSequences (DFAO, GF(p) linalg, serializer)
  tests/
    golden/         # subset-filtered golden corpus harness (semantic-equivalence oracle)
    differential/   # Rust-vs-Java query generator + fuzz targets
    properties/     # proptest invariants
```

**Java package → Rust crate mapping:** the entire `Automata/` package (`FA`, `Automaton`, `NumberSystem` base-*k*, `Morphism`, `WordAutomaton`, `RichAlphabet`, `AutomatonDFA`) → **`wr-core`** (one crate, because of the coupling in kit #1/#2); `Main/EvalComputations` + `Predicate` → `wr-logic`; `AutomatonReader/Writer` → `wr-io`; `Main/Prover` + `Commands` (incl. the inline `split`/`transduce`/… ) → `wr-cli`. **Updated during Phase 2** (superseding this table's original `AutomatonQuantification`/`AutomatonLogicalOps` → `wr-logic` call, `docs/BOUNDARY-MAP.md` §4.2/§4.3): both landed in **`wr-core`** instead — `AutomatonQuantification` because `wr-core`'s own `NumberSystem` calls its `quantify` 10× (an incoming edge that would otherwise force `wr-core`→`wr-logic`, a cycle; `wr_logic::quantify::exists` is a thin wrapper over `wr_core::quantify::quantify`), and `AutomatonLogicalOps` because BOUNDARY-MAP §4.2's split-vs-monolithic question was settled in favor of keeping it monolithic in `wr-core` for the mechanical port.

**Dependency replacement table** (each Java dep needs a Rust answer — this is where port risk concentrates):

| Java dependency | Use in Walnut | Rust plan | Risk |
|---|---|---|---|
| `fastutil` (primitive collections) | Pervasive hot data structures | `std` `Vec`/`BTreeMap`/`HashMap` | Low (mechanical) |
| `dk.brics:automaton` | **two roles:** (a) regex → automaton for `reg`; (b) **the test-suite's automaton-equivalence oracle** (`EqualityUtils.faEqual` → Brics `.equals()` = language equivalence) | (a) `regex-automata` + converter; (b) a native `wr-core` semantic-equivalence check (product+complement+emptiness) — this is a **required deliverable**, not just a dep swap | Medium–High |
| **`io.github.jn1z:otf`** (on-the-fly determinization) | `DeterminizationStrategies` OTF path | **Defer** (confirmed 2026-08-08, §10): ship subset-construction + Brzozowski (both in-repo); reimplement OTF later only if scale demands. Sizing if ever revisited: full port ~4,000-5,200 LOC of an unproven TACAS 2026 algorithm; smallest cut (`CCL`/`BRZ_CCL` only) ~2,000-3,000 LOC | **Highest** — but avoidable in v1 |
| `net.automatalib` | product BFS, serialization utils | Hand-roll over `wr-core` (or `petgraph` where a graph helps) | Medium |
| `slf4j`/`logback` | logging | `tracing` | Trivial |

**Key architectural decisions:**
- **Refactor the global `static` state** (`Prover.mainProver`, `usingOTF`, the `Token` fresh-name counter, `Session` paths) into an explicit `Session` context threaded through calls — do *not* mirror Java's singleton style. This is the one place we deliberately deviate from mechanical fidelity, because global mutable state fights Rust and fights testability. **Caveat (F9):** `Prover.java` isn't *just* static state — it inlines ~20 regex-dispatched commands (several with no `Commands/` class, §3), so porting it already entails structural decisions indistinguishable from a refactor. The "clean bisection: port-bug vs refactor-bug" benefit is therefore **weakest exactly here**, the most command-semantics-central, most-golden-exercised file. Mitigation: port `Prover`'s dispatch last, after `wr-core`/`wr-logic` are differentially green, so divergences there have a small suspect surface.
- **Preserve Walnut's quirks** everywhere else (canonicalization order, state numbering, output formatting) — differential testing depends on it.
- **Compare by semantics, not text.** Tiers 1–3 compare automata via the `wr-core` language-equivalence oracle (matching Walnut's `faEqual`), so the port need **not** reproduce Walnut's exact state numbering/canonicalization — a large, otherwise-wasted engineering tax that even upstream Walnut doesn't pay against itself. Text output (`details`/`error`) is compared after the same timing/progress normalization Walnut uses.

---

## 7. The AI-orchestrated build (Bun-style, adapted to this repo)

The [Bun rewrite](https://bun.com/blog/bun-in-rust) ported 535k LOC in 11 days via loops of *implementer → two split-context adversarial reviewers → fixer*, peaking at 64 parallel Claude instances across 4 worktrees, with hard git-hygiene rules (no `stash`/`reset`, atomic commits) to avoid conflict and resource exhaustion. Zero tests deleted; the TypeScript test suite (language-independent) was the invariant that made it safe.

**We adapt the pattern — but this repo is NOT yet tooled for it** (correcting an earlier overclaim; ct-research's agents/fleet are a *different* repo). What exists here today: `.claude/agents/adversarial-reviewer.md` only. What must be built/ported (ROADMAP §3, Phase 0): the fleet launcher and any additional review-agent roles.

- **The adversarial loop.** Implementer → **two split-context adversarial reviewers** (`adversarial-reviewer.md`) → fixer. Reviewers are told "assume a mathematical/implementation mistake exists; find it," and see **only the diff** (split context prevents author bias — the Bun lesson). *(If a distinct math-honesty or exposition reviewer role proves useful, author it here — the ct-research `proof-referee`/`claim-auditor`/`red-team` agents are a reference, not present in this repo.)*
- **Reviewer ≠ author model (mandatory for trust-critical code).** A same-model reviewer shares the author's blind spots, so any math/decision-procedure code is never author-*and*-reviewed by the same model: a cheap-model author is paired with a *different*, stronger-model reviewer. This composes with split-context — different context *and* different model. (Non-critical glue can relax this.)
- **Model-tiering — start cheap, escalate on evidence.** Route each unit of work to the cheapest model that can do it, escalating only when it fails review or lands in a flagged-hard module:
  - **Cheap tier (e.g. Haiku):** mechanical transliteration of clean `static` methods, boilerplate Tier-2 test replication, compiler-error batches, deterministic glue.
  - **Mid tier (e.g. Sonnet):** most implementation and routine review.
  - **Expensive tier (e.g. Opus):** the genuinely hard reasoning — the hand-written parser + `NumberSystem` edge cases, quantifier-elimination correctness, **adversarial mathematical review**, and diagnosing differential-test divergences.
- **Sharding — MODEST on a subscription.** Concurrent agents share one quota (spend is additive; they only buy wall-clock and hit limits faster), so prefer in-session subagent delegation and a *few* agents, not a 64-way fleet (ROADMAP §0/§1). A `claude-box-agent`-style launcher (isolated `agent/<name>` clones, merged back) must be **built here** (ROADMAP W5) — it does not exist yet. The compiler is the work queue: fix errors crate-by-crate, batch similar errors.
- **Mechanical-first discipline.** Faithful transliteration commits first; idiomatic refactors are separate, later commits — so a differential-test regression bisects cleanly to either "port bug" or "refactor bug," never both.
- **The test suite is the safety rail** (Bun's central lesson): Tier 0's ~100%-covered Java fork means *every* ported unit has an oracle. No Rust code merges without its Tier-2 test green and the Tier-1 golden corpus still passing.
- **Fleet git hygiene + guardrails:** the concrete rules (no `git stash`/`reset`, atomic pathspec-scoped commits, foreground-only container runs, the merge gate) are stated in **this repo's `CLAUDE.md`** (§ "Fleet git hygiene"). The mechanical hooks that enforce them (commit gate, recursive-`rm`/backgrounded-job guard, pre-push `cargo test`) are **to be ported from ct-research** — a Phase 0 task; until then the rules hold by convention. *(Note: the ct-research `guard.py`/commit-gate hooks are not present in walnut-rs yet — do not assume mechanical enforcement.)*

**Cost model** *(rough, scaled from Bun — a planning ballpark, not a measurement; the Phase 1 spike replaces these with real numbers):*

Bun's ~$165k is a **pessimistic** anchor for us, and cost should scale *sub*-linearly, not linearly, for four reasons — this directly addresses the budget concern:
1. **Surface.** Port surface ≈ 8–10k LOC ≈ **~1.7% of Bun's 535k**; even strict linear scaling of the port loop is **low thousands of dollars**. We reuse the ~3k-LOC substrate's *primitives* (GF(p) linalg, arithmetic — the DFAO/serializer need generalization, §4) and drop the exotic-numeration surface — somewhat less to author than the raw LOC, though not the free head-start earlier implied.
2. **Bun bought things you don't need.** Its budget paid for 64-way parallel *redundancy* to compress a year into 11 days, and **6 platforms × full green**. You are not time-boxed and target **one** platform. Removing time-pressure and 5 platforms is a large multiplier off the top.
3. **Most of the correctness pipeline costs zero model tokens.** Coverage measurement, corpus diffing, running suites, the differential harness, fuzzing — all **deterministic scripts** (the repo's "deterministic work → script, not agent" rule). Only *authoring & reviewing* code/tests spends tokens; the heavy correctness *execution* is free.
4. **Model-tiering (per §7 bullets).** The bulk of the work — mechanical transliteration, boilerplate test replication, compiler-error batches — runs on the cheap tier; the expensive model is reserved for the ~20% hard-reasoning surface. Cheap-by-default with evidence-gated escalation is a multiplicative saving over an all-Opus run like Bun's.

**Net:** budget in the **low thousands of dollars of API**, controllable with a hard per-phase token ceiling; the real investment is the correctness *weeks* (mostly deterministic, human-in-the-loop-light), not API spend.

**Honest caveat on the estimate (adversarial-review F8):** the *dollar* number is genuinely uncertain, and the Phase 1 spike as scoped (one base-*k* DFA + one quantified query) touches **none** of the true cost centers — the hand-written parser, `NumberSystem` edge cases, and above all **diagnosing differential-test divergences** (open-ended by nature: you don't know how many a faithful-but-imperfect port produces or how deep each root-cause hunt goes). So the spike de-risks *feasibility and the equivalence oracle*, **not** the cost. The real cost control is therefore the **hard per-phase ceiling** (stop-and-reassess when hit), not a pre-committed total — and it's worth widening the spike to include one parser path + one deliberate divergence hunt if you want an early read on the expensive axis.

---

## 8. Phased roadmap

Each phase ends at a checkpoint you can stop and evaluate at.

| Phase | Goal | Exit criterion | Rough effort* |
|---|---|---|---|
| **0. Fork & cover + scope-gap closure** | Fork Walnut; JaCoCo on subset; characterization tests toward ~100%; export the **test manifest** (schema in §5 Tier 0); **classify the inline commands (`convert`/`transduce`/`split`/… — §3) KEEP/DROP**; **full `Automata/` file inventory → verified Java-file→crate boundary map** (kit #2, adversarially reviewed before Phase 2); **write + review `PORTING.md`** (Java→Rust idiom map, kit #6); **run the OTF empirical check (§9 F3)**; **filter the golden corpus to the subset**; **port the fleet hooks from ct-research** (commit gate, safety guard); log every latent-bug/quirk decision | Coverage green on KEEP modules; command set + crate boundary map finalized; `PORTING.md` reviewed; OTF question answered; subset-corpus enumerated; manifest usable by Rust | **1–4 wks** (F7: ~100% on the 873-LOC regex-dispatch `Prover.java` alone is a real effort; "days" was optimistic) |
| **1. Spike** | Thin end-to-end: base-*k* DFA + `minimize` + one quantified `eval` query + **the `wr-core` semantic-equivalence oracle**; differentially tested vs Walnut. *(Optionally widen to one parser path + one divergence hunt to sample the expensive cost axis — F8.)* | One real query returns a **semantically-equivalent** result to Walnut (via the oracle), both ways | days |
| **2. Core engine** | Mechanical port of `wr-core` (FA, Automaton, base-*k* `numsys`): determinize (subset/Brz), Valmari minimize, product, reverse, + the language-equivalence oracle | Tier-2 tests + Tier-4 core invariants green | 2–4 wks |
| **3. Logic layer** | `wr-logic` (parser, quantifiers, boolean) + `wr-io` + `wr-cli`; the full FOL decider | **Golden corpus (Tier 1) green**; `eval`/`def`/`reg` work | 3–6 wks |
| **4. Hardening** | Differential generator (Tier 3) + fuzzing (Tier 5) at scale; full property suite (Tier 4); performance vs JVM Walnut | No differential divergence over N≥10⁵ generated queries; property suite green; faster than Walnut on the research workloads | 2–4 wks |

*\*Effort figures are rough analogical estimates scaled from Bun + the surveyed LOC; they are **unverified** until the Phase 1 spike produces real per-unit throughput. Do not treat as commitments.*

---

## 9. Risks & mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| An implementation bug produces a wrong math answer, undetected | High (goal-defining) | Tier 4 property invariants mandatory (Walnut-independent); treat every Tier-0 uncovered branch as a bug hunt; two independent minimizers must agree |
| OTF opt-in determinization lib has no Rust equivalent — **is deferral a perf path or a capability cliff?** | **RESOLVED 2026-08-08 — deferral confirmed, see §10** | Phase 0 Item 7 ran the empirical check: no confirmed real case where `SC`+`BRZ` (both KEEP-scope) fail to *terminate*; the one real slow case found (`thm5`, a genuine Shallit-paper query) is ~300-700x faster under `BRZ` alone (already in scope) and `SC` still terminates (42s, not a hang). ct-research's real severe explosions (up to 2.06M states) were never once addressed with a `[strategy …]` directive — always query/numeration reformulation or a non-Walnut fallback instead. A follow-up scope-comparison (full detail: `walnut-java/phase0-artifacts/PROGRESS.md`'s OTF follow-up entry) sized the deferred cost — ~4,000-5,200 LOC of an unproven TACAS 2026 algorithm, no smaller cut under ~2,000-3,000 LOC — and the user decided to defer all of it, including that smaller cut. (F3) |
| Hand-written parser edge cases hard to match exactly | Medium | Golden corpus + differential fuzzing of the parser pin exact semantics |
| `NumberSystem` god-class messy to slice | Medium | Subset to base-*k* only; drop the exotic-numeration branches entirely |
| Global mutable state resists a faithful port | Low–Med | Deliberately refactor to a `Session` context (the one sanctioned deviation) |
| Effort/cost estimates are analogical, not measured | Medium | Phase 1 spike replaces every ballpark with real throughput before full commitment |
| Scope creep back toward full Walnut parity | Medium | Hold the subset line; peripheral numeration is a separate later project |
| Licensing of a derivative (GPLv3) | **None — aligned** | Walnut is GPLv3-or-later; a Rust port is a derivative work → stays GPLv3. This *matches* your open-source, non-commercial intent exactly: full freedom to modify/extend/distribute; obligations are only "keep it GPL + open + attributed." No action needed beyond adding your copyright line and preserving Walnut's. See §10. |

---

## 10. Open decisions & recommendation

**Settled (no longer open):**
- *Subset* — `morphism`/`image` are **in** the v1 subset (your word-automata / morphic-sequence work needs them).
- *Coverage scope* — Tier 0's ~100% coverage target is **scoped to the subset KEEP modules only**; the dropped Ostrowski/CAS surface is not coverage-tested (wasted on code we won't port).
- *Test performance* — end-to-end/differential/property tests run under per-test resource caps with bounded (small) generators, given Walnut's superexponential worst case (see §5 "Test-performance guardrails").
- *Cost control* — cheap-model-by-default with evidence-gated escalation; reviewer model ≠ author model for trust-critical code; deterministic correctness pipeline off the token budget (see §7).
- *Formal verification* — out of scope; it targets algorithm correctness, which isn't the risk (see §5).
- *Licensing (GPLv3)* — fully aligned with your open-source, freedom-to-modify intent; the mechanical-port approach carries **zero** licensing tension because staying GPL is exactly what you want. Ramifications, concretely: **you may** modify/extend/run/distribute/maintain your own fork freely (and even charge for copies); **you must** keep the clone GPLv3, provide source when you distribute, and preserve Walnut's copyright/license while adding your own; **you may not** relicense it permissively or make it closed — none of which you want. You own the copyright to the code you write; the combined work is GPL because it derives from Walnut. Simplest path: GPLv3 the whole `walnut-rs` workspace.
- *OTF deferral (§9 F3)* — **decided 2026-08-08: defer the entire OTF-family determinization surface**
  (`CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`/`OTF()`), confirming the original plan (KEEP only `SC`+`BRZ`) rather
  than leaving it an unverified risk. Basis: Phase 0 Item 7's empirical check found no real case where
  `SC`+`BRZ` fail to *terminate* (only a speed gap, and `BRZ` alone — already in scope — recovers nearly
  all of it on the one real example tested); ct-research's genuine severe explosions were never once
  addressed with a `[strategy …]` directive in practice. A follow-up scope comparison (prompted by the
  fair objection that "never used" isn't the same as "never needed") found the deferred surface is a real
  chunk of work either way — the full addition is ~4,000-5,200 LOC of an unproven **TACAS 2026** algorithm
  (not a decades-trusted textbook one like `SC`/`BRZ`/Valmari), and the smallest useful cut
  (`CCL`/`BRZ_CCL` only, skipping the simulation-relation machinery) is still ~2,000-3,000 LOC — no
  genuinely "small and easy" subset exists (`CCL`/`CCLS` share one method with a single boolean
  differentiating them; `BRZ_CCL`/`BRZ_CCLS` aren't separate algorithms, just Brzozowski's reverse step
  routed through `CCL`/`CCLS`). Full evidence and the size breakdown: `walnut-java/phase0-artifacts/
  PROGRESS.md`'s Item 7 entry + its same-day OTF follow-up entry. **Not permanently foreclosed** — if a
  concrete real need ever emerges, it's a self-contained later port (same pattern as the negative-base
  deletion decision, §3/BOUNDARY-MAP.md §4.1), not worse off for being deferred now.

**My recommendation:** approve **Phase 0 + Phase 1** now. They are cheap, they produce durable value regardless (a hardened Java Walnut + a proven-out spike), and — crucially, given the adversarial pass below — Phase 0 now also *settles the open scope questions* (missing commands, OTF cliff, true corpus size) and Phase 1 stands up the semantic-equivalence oracle. Everything downstream is gated on the spike being green.

---

## 11. Repository & environment structure

The build would generate a lot of history (thousands of AI-orchestration commits, a Java fork, large test corpora). None of it should land in `ct-research`. The clean shape mirrors how this repo *already* vendors `libs/Walnut` and `libs/RustConstantTermSequences` — **three separate repos, one deliberate seam:**

1. **`walnut-rs`** — a **new standalone GPLv3 repo** (the Rust clone). All build noise, CI, license, and issues live here; the AI-orchestration fleet clones *this* repo, so agent branches and guard hooks operate in its tree.
2. **`walnut-java`** — a **GitHub fork of upstream Walnut** for the Phase 0 coverage work. Keeping it a true fork (not a copy) lets you `git pull upstream` to track Walnut's updates and keeps the GPL lineage explicit. The ~100% coverage tests + exported test manifest live here.
3. **`ct-research`** (this repo) — consumes `walnut-rs` as a **git submodule under `libs/walnut-rs`**, pinned like the other vendored libs. It sees only a *single pointer-bump commit* when you deliberately run `git submodule update --remote` — precisely the discipline CLAUDE.md already mandates. **Zero day-to-day noise in your research history.**

**Why this shape:**
- **Total noise isolation** — research commits stay clean; the clone advances in its own repo until you choose to bump the pointer.
- **It's the pattern you already run** — one more pinned submodule; the existing submodule/union-merge conventions cover it.
- **Licensing stays contained** — GPLv3 lives in `walnut-rs`'s own repo, no entanglement with `ct-research`.
- **Substrate reuse without a fork** — `walnut-rs` pulls in `RustConstantTermSequences` as a Cargo **git dependency** (it stays its own crate; one source of truth).
- **Thin research invocation** — later, a `bin/walnut-rs` wrapper in `ct-research` (sibling to `bin/walnut`) calls the submodule's binary; experiment drivers switch engine by changing one wrapper.

**One Phase 0 line item:** the AI-orchestration tooling (agent fleet, `guard.py`, commit-gate hooks) is currently `ct-research`-specific — copy/adapt the relevant bits into `walnut-rs` so the fleet runs in *its* tree.

---

## 12. Adversarial-review record (v2)

A Sonnet subagent was run as a hostile reviewer against this proposal and the actual Walnut/substrate code; its load-bearing findings were **verified firsthand** and folded in above. Summary of what changed:

| # | Finding (verified) | Resolution in this doc |
|---|---|---|
| **F1** | Walnut compares automata by **semantic language-equivalence** (`EqualityUtils.faEqual` → Brics), **not** byte/text identity; details compared with timing normalized | Replaced every "byte-identical" bar with semantic-equivalence; added the **equivalence oracle as a required `wr-core` deliverable** + the second role of `dk.brics` to the dependency table. *Net: simpler, cheaper — no canonicalization chase.* (§1/§3/§5/§6/§8) |
| **F2** | Golden corpus is **~689** golden-output files (not ~1,000); **~22%** use dropped numeration; **~28** CAS files; and `split/rsplit/join/transduce/convert/minimize/…` are inline in `Prover.java`, absent from KEEP/DROP | Corrected the numbers; "corpus green" now means the **subset-filtered** slice; added a **TO-CLASSIFY command list** and made classification a Phase 0 task (§3/§4) |
| **F3** | `SC` (subset construction) is the **default** strategy; OTF is opt-in — so OTF deferral is a *hard-tail* question, not a general one | Reframed the risk as **UNVERIFIED**; added an **empirical Phase 0 check** (do your real queries need non-`SC` to terminate?) (§3/§9) |
| **F4** | Brzozowski calls the minimizer mid-algorithm and its CCL/CCLS variants route through OTF — not cleanly separable | Noted the entanglement; "ship Brz, defer OTF" now scoped to *plain* Brz (§3) |
| **F5** | The "∀ ⇒ two determinizations" cost story was imprecise; driver is projection-`determinize` per alternation level | Corrected the §5 description (testing strategy unchanged) |
| **F6** | Substrate reuse oversold — DFAO is deterministic-only (no NFA), serializer is single-track LSD (msd is a string-hack), minimizer is base-*p*-only | Rewrote §4 honestly ("primitives needing generalization," not a "ready-made bridge"); tempered the cost model's reliance on it |
| **F7** | ~100% coverage of the 873-LOC `Prover.java` in "days" is optimistic | Widened Phase 0 to **1–4 wks** |
| **F8** | Phase 1 spike touches none of the real cost centers, so it can't de-risk the dollar number | Added the honest cost caveat; real control is the **per-phase ceiling**; optionally widen the spike (§7) |
| **F9** | Mechanical-port-vs-refactor tension is worst at `Prover.java` (inlined commands) | Noted it; mitigation = port `Prover` dispatch **last** (§6) |
| **F10** | Pattern of slightly inflating asset sizes (LOC, corpus counts) | Corrected throughout; sizes now measured |

**The three highest-value pre-Phase-0 changes** (all now reflected): (1) semantic-equivalence oracle, not byte-identity; (2) reconcile the golden corpus + command set against the subset boundary; (3) settle the OTF cliff empirically on *your* query corpus. None is fatal; F1–F3 are scope/design corrections that make the plan *more* accurate and, in F1's case, *cheaper*.

---

## 13. Kit-review record (bootstrap scaffold)

A second Sonnet adversarial pass reviewed the **bootstrap kit itself** (CLAUDE.md, scaffold, agent def) as executable steering for the fleet, against the Bun methodology. Load-bearing findings were verified firsthand and applied to the repo.

| # | Finding (verified) | Resolution |
|---|---|---|
| **#1** | `Automaton`↔`NumberSystem` are bidirectionally coupled (Automaton→NS 19 refs incl. a `List<NumberSystem>` field; NS→Automaton **121** refs) — a `wr-core`/`wr-numsys` crate split is an impossible Cargo cycle | **Folded `numsys` into `wr-core`** (module, not crate); updated workspace, mappings, docstrings |
| **#2** | ~745 LOC of KEEP files missing from §3 (`Morphism`/`WordAutomaton`/`RichAlphabet`/`AutomatonDFA`); `Image` imports `Automata/Morphism` | Added to §3 KEEP → `wr-core`; made a **full `Automata/` inventory + boundary map** a Phase 0 deliverable |
| **#3** | Cargo dependency edges empty; `wr-cts` (mandatory Tier-4 minimizer) wired to nothing | Wired real edges (`wr-logic`/`wr-io`/`wr-cts`→`wr-core`; `wr-cli`→ all incl. `wr-cts`) |
| **#4** | CLAUDE.md lacked the fleet git-hygiene rules (no `stash`/`reset`, atomic commits, foreground containers); DESIGN falsely cited it as the enforcer | Added a **"Fleet git hygiene"** section to CLAUDE.md; corrected the §7 citation |
| **#5** | No merge/commit gate stated as an operating rule | Added the hard **merge gate** to CLAUDE.md (never commit red; two-reviewer loop on trust-critical crates) |
| **#6** | No `PORTING.md` (the top Bun prep artifact) | **Created `PORTING.md`** (Java→Rust idiom map incl. the HashMap iteration-order trap); made it a reviewed Phase 0 deliverable |
| **#7** | Reviewer agent had Bash but no resource-guard, in a superexponential project | Added a small-input/`walnut-guard`-style bound to the agent def |
| **#8/#9** | Split-context not protected; no two-reviewer reconciliation policy | CLAUDE.md: dispatch reviewers with diff-only; a single correctness finding from either blocks |
| **#10/#15** | No CI; no toolchain pin | Added `.github/workflows/ci.yml` (fmt+clippy+test) and `rust-toolchain.toml` |
| **#11** | No test-manifest schema | Pinned a schema in §5 Tier 0 |
| **#13/#16** | `NOTICE` upstream URL stale (`firetto/Walnut`); `wr-cts` substrate URL a placeholder | Corrected to `Walnut-Theorem-Prover/Walnut`; filled the real substrate URL |
| **#14/#17** | Architecture decisions unassigned to a model tier; `Automaton` "mostly-static" overclaim | Architecture/boundary calls → Opus tier; corrected the `Automaton` note |

**Verdict (accepted):** **Phase 0 can start as-is** (it's Java-side). **Phase 1 was gated on #1/#2/#3/#4/#6** — all now fixed in this commit, so that gate is cleared.

---

### Appendix A — Sources
- Bun-in-Rust methodology: <https://bun.com/blog/bun-in-rust>
- Walnut paper (Mousavi): <https://arxiv.org/abs/1603.06017>
- Walnut site: <https://walnut-theorem-prover.github.io/>
- Walnut source: `libs/Walnut/` (this repo, submodule)
- Existing Rust substrate: `libs/RustConstantTermSequences/`
