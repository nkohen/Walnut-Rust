# OTF determinization: a from-first-principles sizing (2026-08-20)

## Why this document exists, and how it differs from the existing note

`docs/UNPORTED-SCOPE-SIZING.md` §4 already sizes OTF at "~4,000-5,200 LOC full port... no smaller
cut under ~2,000-3,000 LOC" and recommends "don't." That figure was produced 2026-08-19 by reading
the external library's GitHub file tree and `walnut-java`'s dispatcher source in one pass. It was
not wrong, but every other item on that same list — Ostrowski, CAS export, negative-base + `split`
— turned out smaller or more tractable than its own pre-port sizing predicted once someone actually
opened the source and started porting. The user's ask here is explicit: OTF is now the *only*
remaining unported thing, so before deciding whether it's worth doing, size it with the same rigor
this project gives to something it's about to actually build — not a one-pass LOC estimate, but a
real read of the algorithm, its dependency graph, and what it would cost against *this* codebase
specifically.

**Method**: this is not a repeat of the prior sizing pass. It (1) pulled the real GitHub source tree
of `jn1z/OTF` (the actual upstream of the `io.github.jn1z:otf` Maven artifact `walnut-java` depends
on, confirmed via `pom.xml` + the jar present in `~/.m2`) and measured exact line counts file-by-file
rather than a bytes-based estimate; (2) read the actual algorithm bodies (`DeterminizationStrategies
.java` in full, plus `OTFDeterminization.java`, `NFATrim.java`, `Registry.java`,
`AntichainForestRegistry.java`, `AntichainForest.java`'s structure, `Threshold.java`,
`ParallelSimulation.java`'s header) rather than inferring from package names; (3) traced what those
files import, which surfaced a dependency layer the prior note didn't discuss at all (below); and
(4) cross-checked against this repo's current `wr-core`/`wr-cli` state (`determinize.rs`,
`minimize.rs`, `meta_commands.rs`, `Cargo.toml`) to say concretely what already exists vs. what's
net-new. Every number below is either a direct measurement (stated as such) or an estimate reasoned
from a measurement (stated as such) — nothing here is a guess presented as a fact.

**Headline finding, up front**: the prior estimate's *library-LOC* number was, if anything, very
slightly high once dead and demo-only code are excluded (see §2) — that dimension is genuinely not
inflated, but it also turns out not to be the dominant cost. The real correction this document makes
is discovering a **second, unsized dependency layer** inside `net.automatalib` that the prior note
never surfaced (§3), and a **correctness-maturity problem** that has no precedent among anything else
this project has ported (§4). Net effect: the true engineering surface is *wider* than "port a
5,000-line jar," even though the jar itself measures slightly *smaller* than previously estimated.
The recommendation (§8) is unchanged — don't, absent a real demand signal — but it is now backed by
a materially deeper investigation than the one that produced the original number.

## 1. What OTF actually is, in three layers

`walnut-java`'s own `Automata/FA/DeterminizationStrategies.java` (326 LOC, already fully read for
this document, reproduced in outline below) is small and mechanical. It is *not* the cost center —
it's a thin dispatcher. The real cost is what it calls out to:

| Layer | What it is | Size (measured or estimated) | Rust equivalent today |
|---|---|---|---|
| **1. `DeterminizationStrategies.java`** | Strategy enum (`SC`/`BRZ`/`CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`), dispatch switch, the `Brz()`/`SC()` helpers | 326 LOC, already fully ported for `SC`/`BRZ` | The `Strategy` enum + dispatch switch already exist in `wr_core::determinize`; only the `CCL`/`CCLS` arm is missing (§7) |
| **2. `io.github.jn1z:otf` v1.1.1** | The actual OTF-CCL/OTF-CCLS algorithm (antichain-forest equivalence registry, on-the-fly periodic re-minimization) | **5,216 LOC measured exactly** across 32 files (§2) | None |
| **3. `net.automatalib`'s partition-refinement internals**, reached *through* layer 2 | `Hopcroft`/`Block`/`FullIntAbstraction` (online re-minimization of a *partial*, still-under-construction DFA) and a second, NFA-oriented use of `Valmari` (bisimulation reduction, forward *and* backward) | **Not independently sized by anyone yet** — see §3 | Partial: `wr-core` already has a Valmari DFA-minimizer (Phase 1), but for *complete* automata as a one-shot post-pass, not this |

There's a **layer 4**, minor but real: `OTF.Compress.SimAccelerate` (part of the core antichain
machinery, not simulation-only despite its name and package — see §2) pulls in
`com.github.benmanes.caffeine` for an LRU-style cache. This one is easy — a small hand-rolled cache
or the `lru` crate covers it — but it's one more dependency the prior note didn't mention, and it's
worth recording so nobody is surprised mid-port.

## 2. Layer 2, measured exactly

The prior note's "183,513 bytes ≈ 4,000-5,200 LOC" was a density estimate. This pass fetched all 32
main-source files from `github.com/jn1z/OTF`'s real tree (commit `b537cdf`, the current `main`) and
counted actual lines. **Exact total: 5,216 LOC** — landing at the very top of the prior range, not
beyond it, so that estimate holds up. But "5,216 LOC total" is not "5,216 LOC you'd need to port,"
because a meaningful fraction is dead or integration-irrelevant:

| Group | Files | LOC | Needed for a walnut-rs port? |
|---|---|---|---|
| **Confirmed dead** — three alternate antichain-forest implementations, superseded, never instantiated (the prior note already confirmed this by tracing `AntichainForestRegistry`'s constructor — it only ever builds `Compress.AntichainForest`, never `2`/`5`/`5Idx`) | `AntichainForest2.java`, `AntichainForest5.java`, `AntichainForest5Idx.java` | 322+380+438 = **1,140** | No |
| **Demo/CLI-only**, not reached from `DeterminizationStrategies.java` at all — a standalone command-line tool for running OTF against `.ba` files outside Walnut | `OTFCommandLine.java`, `BAFormat.java` | 301+42 = **343** | No — `wr-io` already has its own `.txt`/`.ba` readers |
| **Possibly unused** — an alternative determinizer class not referenced anywhere in `DeterminizationStrategies.java`'s call path (not confirmed dead the way the antichain variants were — flagging as "needs a real grep-the-call-graph check before assuming," not asserting) | `PowersetDeterminizer.java` | 101 | Probably not, unconfirmed |
| **Simulation-only** — needed for `CCLS`/`BRZ_CCLS`, not for plain `CCL`/`BRZ_CCL` | `Simulation/{FixedBitSet,NaiveSimulation,ParSimTask,ParallelSimulation}.java` | 95+217+86+284 = **682** | Only if simulation variants are wanted |
| **Core** — antichain-forest registry, its bitset backbone, the OTF main loop, the pre-reduction bisimulation step, the periodic-minimization scheduler | `SmartBitSet.java` (755), `Compress/{AntichainForest,ACElts,ACGlobals,ACPlus,InvertedIndex,SimAccelerate}.java` (284+197+134+65+233+117=1,030), `Registry/{Registry,AntichainForestRegistry,NoOpRegistry,AddressRegistry}.java` (50+57+23+53=183), `Model/{Threshold,DeterminizeRecord,CompactImpl,FastImpl,SupportsCompactPowerset,Cancellation}.java` (173+8+42+76+11+55=365), `PTInitializers.java` (232), `NFATrim.java` (168), `OTFDeterminization.java` (182), `BitSetUtils.java` (18), `module-info.java` (17) | **2,933** | Yes — this is the floor, needed even for the smallest useful cut |

**Corrected sizing**:
- **Smallest useful cut (`CCL`/`BRZ_CCL` only, no simulation)**: core (2,933) — that's it, since the
  simulation package isn't reached. **≈2,900 LOC**, essentially matching the prior note's
  2,000-3,000 estimate, at the top of that range.
- **Full four-strategy family (`CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`)**: core + simulation = 2,933 + 682
  = **≈3,600 LOC**, genuinely *below* the prior note's 4,000-5,200 headline number, because that
  number counted the dead antichain variants and the CLI/demo code that a library-only integration
  never needs. **This is the one place this investigation found "smaller than feared," consistent
  with the pattern the user asked about** — but, as the rest of this document shows, it's a minor
  correction next to §3.

One genuinely new structural fact from actually reading `AntichainForest`, `ACGlobals`, and
`InvertedIndex` rather than just sizing them: this is a real, custom **antichain disjoint-set forest
with a rebuildable inverted index for subsumption search** — `compress()` sorts equivalence classes
by size, flags which are "searchable," and rebuilds an inverted index over their unions; `put`/`get`
walk that structure to find whether a new subset is subsumed by (or subsumes) an existing
equivalence class. There is no off-the-shelf Rust crate for this — it is genuinely bespoke research
code, and porting it faithfully (not just "an antichain-like thing that seems to work") means
understanding and replicating that subsumption-search structure exactly, index rebuild included.

## 3. Layer 3 — the dependency the prior sizing never surfaced

Reading `OTFDeterminization.otfMinimization` and `NFATrim.bisim` line-by-line (not just their
signatures) surfaced two calls into `net.automatalib.util.partitionrefinement` that neither this
project's prior OTF note nor its DESIGN.md §9/§10 discussion mentions:

1. **`otfMinimization`** — the periodic re-minimization step that runs mid-construction, on a
   *partial*, still-being-explored DFA — builds a `net.automatalib.util.partitionrefinement.Hopcroft`
   instance via `PTInitializers.initDeterministic`, calls `computeCoarsestStablePartition()`, then
   directly manipulates that instance's internal index arrays (`pt.blockData`, `pt.predOfsData`,
   `pt.predData`, `Block.low`/`Block.high`) to merge equivalent states, redirect their incoming
   transitions to a representative, and free the merged state IDs for reuse — all *while* the
   subset-construction exploration is still in flight, treating not-yet-explored states as an
   "artificial sink block" to be skipped. This is real, and it is not the same problem `wr-core`'s
   existing Valmari minimizer solves: that one runs once, after determinization is complete, on a
   finished DFA, and returns a fresh minimized automaton (Phase 1, `minimize.rs`, 814 LOC). This
   needs a partition-refinement structure that's *queryable and mutable mid-construction*, with a
   live notion of "finished" vs. "unfinished" states — a genuinely different shape of problem, even
   though the underlying partition-refinement math (Hopcroft/Valmari-family) is a close cousin of
   what's already been ported once.
2. **`NFATrim.bisim`** — a pre-reduction step, run unconditionally at the top of `OTF()` regardless
   of `CCL` vs `CCLS`, that computes NFA *bisimulation* equivalence (both forward and backward —
   `bisimReduce(..., forward=true)` then `bisimReduce(..., forward=false)` on the reversed NFA) via
   `net.automatalib.util.partitionrefinement.Valmari` + `ValmariInitializers`/`ValmariExtractors`.
   This is a *different application* of the same underlying algorithm family from `wr-core`'s
   existing DFA minimizer — bisimulation reduction on a nondeterministic automaton, not
   language-equivalence minimization of a deterministic one. No code in this repo does this today.

Neither of these has a visible LOC count from outside `net.automatalib` (it's a large, general
automata-learning library — porting *it* wholesale is obviously off the table and not what's being
proposed). What's actually needed is a Rust reimplementation of just these two specific
partition-refinement variants, scoped to what `OTFDeterminization`/`NFATrim` actually call. Given
`wr-core`'s own Valmari minimizer (a full, working, adversarially-reviewed port of a closely related
algorithm) is 814 LOC, a reasonable range for both variants combined — accounting for the added
complexity of "mid-construction, partial-automaton-aware" for the first one and "forward+backward,
NFA-shaped" for the second — is **roughly 600-1,200 LOC of new `wr-core` partition-refinement code**,
estimated by analogy to the one comparable thing already built here, not measured (there is nothing
to directly measure — that's the point of this finding). The existing Valmari port is a genuine
asset here (the team has done this style of algorithm once, adversarially reviewed, and knows its
sharp edges — see the "q0 aliasing quirk" already documented in `minimize.rs`'s module docs) — but
"we've done something adjacent before" is not the same as "this is small."

One open engineering question worth flagging rather than resolving here: `NFATrim.bisim` is called
unconditionally, but reading its own logic, it appears to be a pure **size-reduction** pre-pass —
bisimulation-equivalent NFAs accept the same language, so skipping it should still produce a correct
(if potentially larger/slower) OTF run. If that's confirmed correct by whoever eventually does this
port, an initial spike could stub `bisim` as identity, deferring the entire NFA-bisimulation half of
layer 3 and leaving only the Hopcroft-on-a-partial-DFA piece (which is *not* skippable — it's the
algorithm's actual mechanism, not an optimization). That would meaningfully shrink layer 3, but it's
a hypothesis from reading the source, not something this investigation verified by testing — flag it
for whoever picks this up, don't bank on it.

## 4. The correctness-maturity problem — a risk dimension no LOC estimate captures

This project's Prime Directive rests on one load-bearing assumption (`CLAUDE.md`, verbatim): *"the
underlying algorithms... are textbook and trusted — the PORT is what we test."* That assumption is
true for subset construction, Valmari, Brzozowski, and ∃-projection — decades old, independently
scrutinized, the basis for every differential-testing and mechanical-port decision this project has
made. **It is not true for OTF-CCL/CCLS.** Concretely, from the library's own `CHANGELOG.md`:

> **[OTF 1.1.0] - 2025-10-29** — Fixed: "memory performance bug in InvertedIndex (potentially could
> have consumed 10x memory)"; "AC Union performance bug (conservative AC unioning sometimes lost
> equivalence information); testing reveals occasional 10x speedups."

That's real bugfix history from four months ago in the exact subsumption-search machinery §2
describes, in a library whose README points to a paper explicitly marked **"revised version, to
appear in TACAS 2026"** — i.e., not yet presented at the time of this investigation. This is, by a
wide margin, the youngest and least battle-tested piece of math this project would ever consider
porting into a trust-critical crate. Every other genuine Walnut (Java) bug this project has found and
logged (`docs/WALNUT-BUGS.md`, 44 entries) was found in code that's been in production for years,
where "port bug vs. algorithm bug" has a clear prior (algorithm bugs are rare, port bugs are common).
For OTF, that prior doesn't hold — a divergence found while porting this could just as easily be an
undiscovered bug in a four-month-old research implementation as a porting mistake, and there is no
large, independent, "trusted" oracle to lean on the way `walnut-java`'s `SC`/`BRZ`/Valmari paths serve
as oracles for everything else in this project. Differential testing against `walnut-java`'s OTF path
would still work mechanically, but it would be verifying **port-fidelity to an early-stage reference
implementation**, not **correctness against a trusted algorithm** — a materially weaker guarantee
than every other differential suite in this repo currently provides, and worth naming as such rather
than let it blend into "differential testing, same as everything else."

This also raises a live policy question this document deliberately does not answer, because it's the
user's call, not an implementation detail: `CLAUDE.md` rule 2 says port Walnut's quirks/bugs verbatim
and log genuine bugs rather than silently fixing them — a rule calibrated for a mature, external,
independently-maintained project. Does that rule still make sense applied to a four-month-old
research prototype, where "faithful to the Java reference" carries much less epistemic weight than
"faithful to the paper's stated algorithm"? Reasonable people could land either way; it should be
decided deliberately if this is ever picked up, not inherited by default from a rule written with
`walnut-java` proper in mind.

## 5. Testing burden — the Phase-0 discipline has no natural home here

Every other DROP-scope item this project has ported (Ostrowski, CAS export, negative-base + `split`)
followed the same first move: drive `walnut-java`'s own JaCoCo coverage on the target file toward
~100% *before* porting, then differential-test against real `walnut-java` output. That motion doesn't
transfer cleanly to OTF, because the code being ported doesn't live in `walnut-java` — it's a
third-party GitHub repo (`jn1z/OTF`) this project has no relationship to and no standing to modify.
There's no "add five `@Test` methods to `OstrowskiTest.java`, land it in `walnut-java` first"
equivalent available.

What *does* exist: `jn1z/OTF` ships its own test suite, measured the same way as §2 —
**3,530 LOC across 17 files** (`src/test/java/OTF/`), including `BenchmarkIT.java`,
`SimulationTest.java`, `AntichainForestTest.java`, `NFATrimTest.java`,
`OTFDeterminizationTest.java`, `RegistryTest.java`, `ACEltsTest.java`, `PaigeTarjanNFAIT.java`, and
`TabakovVardiRandomNFA.java` (a random-NFA generator implementing the Tabakov-Vardi model, a
standard tool in the automata-research literature for stress-testing determinization/minimization
algorithms against structurally varied inputs — its presence is itself a signal of how seriously the
library's own author treats correctness testing, which is reassuring about the library, but doesn't
change that this project would need to read and re-derive equivalent Rust coverage from scratch,
comparable in scale to reading and porting a real Java test class — work this project's own sizing
conventions would normally count and didn't, because the prior note never got this deep). Realistic
treatment: **read `jn1z/OTF`'s own tests as the closest thing to a spec, and port the load-bearing
ones as Rust unit/property tests** — comparable-scale work to §2's "core" 2,933 LOC, not a rounding
error.

Tier 4 (property-based invariants) is also harder to design here than for anything else this project
has done. `L(minimize(A)) = L(A)` is a clean, obviously-correct property to check against an
already-complete automaton. OTF's correctness claim is about an *incremental, partial-automaton*
process — the useful property is closer to "at every periodic-minimization checkpoint, completing
exploration of the current partial DFA yields the same language as if no minimization checkpoints had
happened at all," which is a real property but a meaningfully harder one to state and check than
anything in this project's existing Tier 4 suite (`wr_core::equiv`'s current invariants, listed in
`CLAUDE.md`'s correctness ladder, are all properties of *complete* automata).

## 6. What's already built, and is forward-compatible

This is the genuinely good news, and worth stating plainly so the rest of this document doesn't read
as pure discouragement: **U32's `[strategy]`/`[export]` metacommand work already built the front door
for this**, deliberately, even while the algorithm itself stayed deferred. Confirmed by reading
`crates/wr-cli/src/meta_commands.rs` directly:

- All six `Strategy` names (`SC`, `Brzozowski`, `CCLS`, `Brzozowski-CCLS`, `CCL`, `Brzozowski-CCL`)
  and their aliases (including the dash-stripping quirk, `WB-029`, already faithfully reproduced —
  `"BRZCCL"` etc.) parse correctly today. A user (or a future implementer) asking for `CCL` gets a
  clean, distinguishable `MetaCommandError::OtfStrategyDeferred`, not a silent misbehavior or a
  generic "unknown strategy" error — the module's own docs say exactly why this split exists.
- `wr_core::determinize::{DeterminizeContext, ExportRequest, Strategy}` and the real dispatch switch
  in `determinize.rs` already have the shape a `CCL`/`CCLS` arm would slot into — U32 threaded a real
  `Option<&mut dyn DeterminizeContext>` through `eval`/`def` and library-automaton loading precisely
  so metacommands could eventually drive real strategy selection, not just parse it.
- Net effect: **adding OTF later would mean deleting one deferred-error arm and one Rust-side
  algorithm module, not touching the parser, the metacommand plumbing, or any existing call site.**
  That scoping work already happened; it doesn't need to happen again.

The gap is purely in `wr_core::determinize`'s missing `OTF`/`CCL`/`CCLS` implementation and (§3) the
partition-refinement primitives it would need — not in getting a user's `[strategy 1 CCL]` request to
the right place.

One more concrete, easy-to-overlook fact: **`wr-core` currently has zero external crate dependencies**
beyond `num-bigint` (confirmed via `Cargo.toml` — the project's own U33/U34 history notes even its
bitsets are hand-rolled `BTreeMap`/`Vec`, deliberately, for fidelity and to avoid a dependency
surface). OTF would be the first thing in this project's history to break that norm, one way or
another — either a `SmartBitSet`-equivalent gets hand-rolled (755 LOC in Java, so not small either
way) or a bitset crate gets pulled in for the first time, which is a real precedent-setting decision,
not just an implementation detail.

## 7. Revised effort estimate

| Component | Estimate | Basis |
|---|---|---|
| Layer 2 core (smallest cut, `CCL`/`BRZ_CCL`) | ~2,900 LOC | Direct measurement, §2 |
| Layer 2 simulation add-on (`CCLS`/`BRZ_CCLS`) | +~700 LOC | Direct measurement, §2 |
| Layer 3 (partition-refinement primitives `wr-core` doesn't have) | ~600-1,200 LOC, unmeasured | Estimated by analogy to the existing 814-LOC Valmari port, §3 |
| Ported/derived test coverage | comparable to ~2,900-3,500 LOC of source read+translated, plus new Tier-4 property design | Measured source (jn1z's own suite, §5) + qualitative difficulty (§5) |
| Review overhead | the mandatory two-independent-adversarial-reviewer loop, likely **more** rounds than this project's typical unit given §4 | This project's own history: routine plumbing (U28) still took 3 rounds; nothing this project has ported carries OTF's combination of algorithmic novelty *and* unverified-upstream risk |
| `wr-cli` integration | small — the dispatch surface already exists (§6) | Direct read of current code |

**All-in**, for even the smallest defensible cut (`CCL`/`BRZ_CCL`, skipping simulation, and
tentatively skipping the bisimulation half of layer 3 per §3's open question): comparable in scope to
this project's largest completed multi-unit batches (e.g. Phase 3b's U17-U26, or standing up the
differential-generator + fuzz infrastructure in Phase 4) — realistically multiple work-sessions, not
a single-session unit the way Ostrowski or CAS export were. The full four-strategy family with both
layer-3 pieces is larger still. This is *not* a case where "it looked big but was actually small once
someone opened the source," the pattern the other DROP items showed. The library-LOC dimension
specifically showed a mild version of that pattern (§2); the total picture didn't, because of what
§3 and §4 add that no LOC count captures.

## 8. Demand signal (unchanged — restated, not re-derived)

Nothing in this investigation touched the demand-signal question; it's already been answered with
real evidence and doesn't need re-litigating. Full detail in `walnut-java/phase0-artifacts/
PROGRESS.md`'s 2026-08-08 "Item 7" and "OTF follow-up" entries, summarized: zero golden fixtures
require a non-`SC`/`BRZ` strategy to terminate; the one real heavy example found (`thm5`, fixtures
637-641) is slow-but-terminating under plain `SC` (42s) and the *already-shipped* `BRZ` alone closes
essentially the whole gap (130ms) without needing OTF at all; and `ct-research`'s real, documented,
severe explosions (up to 2.06M states) were never once addressed with a `[strategy ...]` directive —
always query/numeration reformulation or a non-Walnut fallback. That evidence is unchanged by
anything found here.

## 9. Recommendation

**Still don't — and this investigation sharpens that call rather than reopening it.** The one
dimension that looked smaller under close inspection (§2's corrected LOC count) is a minor,
second-order correction next to two things the original sizing missed entirely: a real,
currently-unsized second dependency layer with no Rust equivalent (§3), and a correctness-maturity
profile unlike anything else this project has ever ported (§4) — a still-changing, not-yet-presented
research algorithm, where this project's entire testing philosophy (trust the algorithm, test the
port) doesn't hold. Combined with an unchanged, already-rigorously-confirmed zero demand signal
(§8), the honest read is the same as before, now for better reasons: this is large, still maturing
upstream, and solves a problem nothing in this project's real usage has ever actually had.

**Trigger for revisiting** (unchanged from `DESIGN.md` §10, restated here as the single place to
check): a real, reproducible query from actual research work where both `SC` and `BRZ` fail to
*terminate* in practice — not "would be faster under OTF," a genuine hang or resource exhaustion
neither in-scope strategy resolves. If that happens, start from **this** document, not the original
one-pass estimate: begin with the `CCL`/`BRZ_CCL`-only cut (§2), investigate whether `NFATrim.bisim`
can be safely stubbed as identity for a first spike (§3's open question — verify before relying on
it), and treat the layer-3 partition-refinement work as its own reviewed sub-unit before attempting
the full algorithm, exactly the way this project has sequenced every other multi-part port (negative-
base's Layer A before Layer B is the closest precedent).

## Sources

- `walnut-java/src/main/java/Automata/FA/DeterminizationStrategies.java` — read in full.
- `walnut-java/pom.xml` — confirmed `io.github.jn1z:otf:1.1.1` and `net.automatalib.distribution:
  automata-distribution:0.12.1`.
- `~/.m2/repository/io/github/jn1z/otf/1.1.1/otf-1.1.1.jar` — confirmed present locally, class list
  enumerated.
- `github.com/jn1z/OTF` (commit `b537cdf`, branch `main`) — full source tree fetched via the GitHub
  API (`git/trees/main?recursive=1`), all 32 main-source files and their exact line counts measured
  directly (not byte-density-estimated); `OTFDeterminization.java`, `NFATrim.java`,
  `Registry/Registry.java`, `Registry/AntichainForestRegistry.java`, `Model/Threshold.java`,
  `Compress/AntichainForest.java` (structure), `Compress/SimAccelerate.java` (header),
  `Simulation/ParallelSimulation.java` (header) read in full or substantial part; `README.md` and
  `CHANGELOG.md` read in full.
- `arxiv.org/abs/2505.10319`, "Deconstructing Subset Construction: Reducing While Determinizing"
  (the TACAS 2026 paper this library implements) — abstract and framing read via web fetch.
- `walnut-java/phase0-artifacts/PROGRESS.md`'s 2026-08-08 Item 7 + OTF-follow-up entries — read in
  full for the demand-signal evidence (§8), not re-derived.
- `walnut-rs/crates/wr-cli/src/meta_commands.rs`, `crates/wr-core/src/{determinize,minimize}.rs`,
  `crates/wr-core/Cargo.toml` — read/grepped directly for current-state claims in §6.
