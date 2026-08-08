# BOUNDARY-MAP.md — verified Java-file → Rust-crate boundary map

**Status: Phase-0 deliverable, DRAFT — not yet adversarially reviewed.** Covers every file in
`Automata/` and its subpackages (`FA/`, `Numeration/`, `Search/`, `Writer/`) in the `walnut-java`
checkout at `../ct-research/libs/Walnut` (32 files, 8,568 LOC). Produced per DESIGN.md §8 Phase 0 and
kit-review finding #2. **Must be adversarially reviewed before Phase 2** (DESIGN.md's own rule for this
doc), and the open items in §4 need your call before it's final.

Methodology: import graph built by direct grep (cross-package `import` statements + same-package
bare-name reference counts, since same-Java-package files need no `import`); per-file responsibility /
KEEP-DROP-DEFER / target-crate judgment produced by 4 parallel subagents reading class docs + method
signatures (not full-file reads), one per file group. Findings below were then cross-checked against
the raw import data, not taken on faith.

---

## 1. Summary

| | |
|---|---|
| Files scanned | 32 |
| Total LOC | 8,568 |
| KEEP | 24 files (~6,460 LOC) |
| DROP | 8 files (~1,616 LOC — Ostrowski/NodeState + all 5 CAS-export files + AutomatonMatrixWriter) |
| DEFER (partial, within one file) | `FA/DeterminizationStrategies.java` — OTF-dependent strategy variants only |
| New KEEP files not in DESIGN.md's original list | `Transducer.java`, `Search/ProductBFS.java`, `FA/BricsConverter.java`, `FA/Infinite.java`, `FA/Transitions*.java`, `FA/ValmariPartition.java` |
| Cross-crate cycle beyond the known Automaton↔NumberSystem one | **Yes — a systemic one, see §3** |

---

## 2. Full file table

### Top-level `Automata/`

| File | LOC | Responsibility | KEEP/DROP | Target crate | Coupling out of `Automata/` | Ambiguous |
|---|---|---|---|---|---|---|
| `Automaton.java` | 741 | Core automaton type: NFA/DFA/DFAO representation, `RichAlphabet` + `List<NumberSystem>` fields, clone/star/concat/join/bind/canonize, file I/O entry points | KEEP | wr-core | `Main.Prover` (op-mode constants `COMBINE`/`FIRST_OP`/`IF_OTHER_OP`, `TXT_EXTENSION`/`GV_EXTENSION`), `Main.Session` (`getAddressForResult`/`getReadFileForAutomataLibrary` — file path resolution), `Main.EvalComputations.Token.{ArithmeticOperator,LogicalOperator}` | **Yes** — see §3 |
| `AutomatonDFA.java` | 75 | `Automaton` subtype for (by-convention, unenforced) deterministic automata; builds from regex+alphabet via `BricsConverter` | KEEP | wr-core | `Automata.FA.BricsConverter` (in-crate), `Main.UtilityMethods`/`WalnutException` (benign) | No — but Rust can enforce the determinism invariant statically where Java only TODOs it; a PORTING.md note, not a scope question |
| `AutomatonLogicalOps.java` | 774 | Static ops: and/or/xor/imply/iff/not, left/right quotient, reverse, `convertNS` (msd↔lsd/base-power conversion, confirmed base-k only), leading/trailing-zero fixups, `combine` | KEEP | wr-core (bulk); boolean connectives arguably wr-logic | `Main.EvalComputations.Token.{LogicalOperator,Operator}`, `Main.Prover` (`COMBINE` constant), `Main.Logging`/`UtilityMethods`/`WalnutException` | **Yes** — DESIGN.md itself flags this file as possibly wr-logic-conceptual; content confirms a real split is plausible (and/or/xor/imply/iff/not = logic layer; quotient/reverse/convertNS/combine = automaton-algorithm layer) — human call on split-vs-monolithic |
| `AutomatonQuantification.java` | 126 | ∃-elimination: removes quantified tracks, permutes/collapses transitions, re-determinizes/minimizes, applies zero-fixup | KEEP | **wr-logic** (physically in package `Automata`, but content is pure ∃-projection semantics — matches DESIGN.md's own hint) | `Main.Logging`/`UtilityMethods`/`WalnutException` only (benign); calls `Automaton`, `AutomatonLogicalOps`, `NumberSystem` — if moved to wr-logic these become a normal forward wr-logic→wr-core dependency, not a cycle | No on KEEP; flag placement (recommend wr-logic, confirmed by content not just DESIGN's hint) |
| `AutomatonReader.java` | 294 | Parses the `.txt` automaton format + transducer format + comments; zero Ostrowski/Fibonacci/Pell/negative-base references | KEEP | wr-io | `Main.Logging`/`UtilityMethods`/`WalnutException` (benign), `Automata.ParseMethods` (in-crate) | Mild — `readTransducer`'s `Transducer`-object construction is arguably wr-core domain vs. wr-io's line-tokenizing; same split pattern as reader/writer elsewhere |
| `Morphism.java` | 196 | Parses `k→k*` letter-to-word morphism maps, builds resulting `WordAutomaton`/`Automaton` image, validates "image morphism" | KEEP — core to `morphism`/`image` | wr-core | `Main.UtilityMethods`/`WalnutException` (benign) | No |
| `NumberSystem.java` | 1027 | base-k (msd/lsd, **incl. negative-base**) numeration: addition/less-than/equality/base-change/constant automata; exposes op enums for eval | KEEP the positive-base msd/lsd machinery; **DROP negative-base** (`baseNegN*`, `isNeg`-gated branches) | wr-core | `Main.*` wildcard (`Prover.TXT_EXTENSION`), `Main.EvalComputations.Token.{ArithmeticOperator,RelationalOperator}` | **Yes** — see §4.1 (negative-base excision is not a clean file-level cut) and §3 (Token coupling) |
| `ParseMethods.java` | 195 | Static regex line-parsers for `.txt` format: alphabet/state/transition/morphism declarations | KEEP — the `.txt` format parser | **wr-io** (pure text-format parsing, despite package location) | `Main.Prover.determineBase(m)` — a real method-call dependency, not just a constant; file even has a `// TODO - look at redundancy with Prover.determineAlphabetsAndNS` admitting the tangle | **Yes** — needs `determineBase` relocated (likely into wr-core's NumberSystem-naming helpers) before this cleanly lands in wr-io — see §3 |
| `RichAlphabet.java` | 239 | Encodes/decodes n-tuple multi-track alphabet symbols to/from single integers; wildcard expansion, subset checks | KEEP — required by multi-track format + product/determinize | wr-core | `Main.WalnutException` only (benign) | No |
| `Transducer.java` | 438 | DFST (deterministic finite-state transducer, all-final, 1-uniform output); MSD-deterministic + nondeterministic transduction of an `Automaton` | **KEEP — closes a real DESIGN.md gap.** Engine behind the `transduce` command; base-k/word-automaton recoding, squarely research-relevant, not CAS/exotic-numeration | wr-core | `Automata.FA.FA` (in-crate), `Main.Logging`/`WalnutException` (benign) — no Prover/Token coupling | No |
| `WordAutomaton.java` | 235 | Static helpers on word automata (per-state output vs. constant via relational op; arithmetic op applied to per-state outputs) | KEEP — needed for `eval`/`def` with output automata | wr-core | `Main.EvalComputations.Token.{ArithmeticOperator,RelationalOperator}` directly, calls `RelationalOperator.compare`/`ArithmeticOperator.arith`; `Automata.FA.ProductStrategies` (in-crate) | **Yes** — same Token-coupling issue as NumberSystem — see §3 |

### `Automata/FA/`

| File | LOC | Responsibility | KEEP/DROP/DEFER | Target crate | Coupling out of `Automata/FA/` | Ambiguous |
|---|---|---|---|---|---|---|
| `FA.java` | 737 | Core NFA/DFA/DFAO engine: state/transition storage, canonicalize, totalize, reverse, star/concat state-building. **Not "mostly-static"** (corrects DESIGN.md's note) — a stateful `Cloneable` object with instance fields (`q0,Q,alphabetSize,O,t,canonized`) plus some static helpers | KEEP | wr-core | `Main.Logging`/`WalnutException` (benign, trivially replaced by `tracing`/error types); `net.automatalib` (`Alphabets`,`CompactDFA`,`CompactNFA`) for interop | No |
| `ProductStrategies.java` | 341 | Boolean product-automaton construction driving multi-automaton ops (cross-product + minimize) | KEEP | wr-core | `Main.EvalComputations.Token.{ArithmeticOperator,LogicalOperator,RelationalOperator}`, `Main.Prover` (`COMBINE`/`FIRST_OP`/`IF_OTHER_OP`) | **Yes** — same Token/Prover coupling as §3 |
| `DeterminizationStrategies.java` | 327 | `Strategy` enum (SC/BRZ/CCL/CCLS/BRZ_CCL/BRZ_CCLS) + `determinize()` dispatch: subset construction, Brzozowski double-reversal, OTF-based determinization | **KEEP `SC`+`BRZ`; DEFER `CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`/`OTF()`** — confirmed structurally clean split (see below) | wr-core | `OTF.OTFDeterminization`, `OTF.Registry.*`, `OTF.Model.*`, `OTF.NFATrim`, `OTF.Simulation.ParallelSimulation` — **confined entirely to the `OTF(...)` method and the CCL/CCLS-routed path**; plain `SC`/`BRZ` never reach these imports | No — DESIGN §3's "ship Brz, defer OTF, entanglement only for CCL/CCLS" is now **empirically confirmed**: `Strategy.removeBrzozowski()` maps `BRZ→SC`, and `brzStep()` calls `SC(...)`, never `OTF(...)` |
| `ValmariDFA.java` | 209 | Valmari DFA-minimization algorithm | KEEP | wr-core | None beyond fastutil | No |
| `Trimmer.java` | 155 | Removes unreachable/dead states | KEEP | wr-core | None | No |
| `BricsConverter.java` | 166 | Converts `FA` ↔ `dk.brics.automaton`: regex→automaton (feeds `reg` via `AutomatonDFA`) and automaton→Brics export | **KEEP — closes a real DESIGN.md gap.** `reg`'s regex compilation needs *some* engine; this is not merely a test-oracle detail | wr-core (or a thin wr-core/wr-io adapter if regex parsing is factored separately) | `dk.brics.automaton.{RegExp,State,Transition}` — genuine external dep, **no Rust equivalent chosen yet** (DESIGN §6 names `regex-automata` as the plan for the regex-parsing half; this file confirms the automaton-conversion half is also needed) | **Yes** — regex→automaton engine choice is a real open item, not just a dependency swap |
| `Infinite.java` | 162 | DFS cycle detection producing a `prefix(cycle)*suffix` witness proving a language is infinite; wired into eval-result reporting via `ProverHelper`/`LogicalOperator` | **KEEP — closes a real DESIGN.md gap** | wr-core | `Automata.RichAlphabet` only (in-crate) | No |
| `TransitionsNFA.java` | 143 | NFA transition-table representation, backs `FA.t` | KEEP | wr-core | `Main.WalnutException` only | No |
| `TransitionsDFA.java` | 106 | DFA transition-table representation | KEEP | wr-core | `Main.WalnutException` only | No |
| `Transitions.java` | 58 | Shared base for `TransitionsDFA`/`TransitionsNFA` | KEEP | wr-core | None | No |
| `ValmariPartition.java` | 76 | Partition-refinement structure backing `ValmariDFA` (package-private, no `public`) | KEEP | wr-core | None — fully self-contained | No |

### `Automata/Numeration/`, `Automata/Search/`, `Automata/Writer/`

| File | LOC | Responsibility | KEEP/DROP | Target crate | Coupling out of subpackage | Ambiguous |
|---|---|---|---|---|---|---|
| `Numeration/NodeState.java` | 46 | `(state, startIndex, seenIndex)` search-node key, only consumed by `Ostrowski.java` | DROP | N/A | None | No |
| `Numeration/Ostrowski.java` | 491 | Ostrowski (continued-fraction) numeration adder/comparison automata | DROP — confirmed; only callers (`DeterminizationStrategies`'s Ostrowski path, `Main/Commands/Ost.java`) are Ostrowski-only and drop with it | N/A | `Automata.*`, `Automata.FA.FA`, `Automata.Writer.AutomatonWriter`, `Main.Session`/`WalnutException` (all moot) | No |
| `Search/ProductBFS.java` | 406 | Generic shortest-witness BFS over `int[]`-tuple product states (pluggable step/accept functors) + a specialized DFA-product variant with reverse-reachability pruning | **KEEP — closes a real DESIGN.md gap.** Not a `ProductStrategies` alternative (that builds a full product; this does on-the-fly witness search); sole consumer is `Main/Commands/Test.java`'s `test` command ("first N accepted inputs") — a real research/verification primitive | wr-core | None beyond `net.automatalib` (`CompactDFA`,`Word`) in the specialized variant | No functionally; **port-design flag**: uses **static mutable fields** as a de facto singleton (`idOf`,`states`,`prevId`,`prevSym`,`q`) — not thread-safe/reentrant, must become owned local state in Rust, not a global |
| `Writer/AutomatonWriter.java` | 175 | Writes `.txt` format, Graphviz `.gv`, Brics-style `.ba` | KEEP | wr-io | `Automata.Automaton`/`FA.FA`/`NumberSystem` (in-crate), `Main.Logging`/`UtilityMethods`/`WalnutException` (benign), `net.automatalib` (`CompactNFA`,`BAWriter`) | No |
| `Writer/AutomatonMatrixWriter.java` | 188 | Walks transition table into a generic incidence-matrix rep, streams through a pluggable `MatrixEmitter`; drives all 4 CAS emitters | **DROP — confirmed CAS-export-only.** Sole caller across the whole codebase is `EvalDef.writeMatrices` (the `export` CAS-matrix feature); no internal algorithm (determinize/minimize/product) touches it | N/A | `Main.Logging`/`WalnutException` (moot) | No |
| `Writer/MapleEmitter.java` | 105 | Maple `.mpl` `MatrixEmitter` impl | DROP | N/A | `Main.Prover` (moot) | No |
| `Writer/MathematicaEmitter.java` | 102 | Mathematica `MatrixEmitter` impl | DROP | N/A | `Main.Prover` (moot) | No |
| `Writer/MatlabEmitter.java` | 105 | MATLAB `.m` `MatrixEmitter` impl | DROP | N/A | `Main.Prover` (moot) | No |
| `Writer/SageEmitter.java` | 104 | Sage/Python `MatrixEmitter` impl | DROP | N/A | `Main.Prover` (moot) | No |
| `Writer/MatrixEmitter.java` | 26 | Shared interface + `EmitterSpec` for the 4 CAS emitters | DROP | N/A | None | No |

---

## 3. NEW coupling cycle beyond the known Automaton↔NumberSystem one

This is the single highest-value finding of this pass. **Multiple `wr-core`-bound files reach "upward" into
what DESIGN.md's crate layout assigns to `wr-logic` and `wr-cli`** — the reverse of the intended dependency
direction (`wr-logic`/`wr-cli` → `wr-core`, never back). Two distinct patterns, both systemic (not one-offs):

**3a. `Main.EvalComputations.Token.{ArithmeticOperator, LogicalOperator, RelationalOperator, Operator}`** —
the parser's AST/Token operator-enum hierarchy (assigned to `wr-logic`) is imported and used directly by:
- `Automaton.java` (`processSplit`, `determineCombineOutVal`)
- `AutomatonLogicalOps.java`
- `FA/ProductStrategies.java`
- `NumberSystem.java`
- `WordAutomaton.java`

If these enum *types* live in `wr-logic`, then `wr-core` depends on `wr-logic` — but `wr-logic` (quantifier
elimination, boolean ops) fundamentally operates *on* `Automaton`, so it must depend on `wr-core`. Under
mechanical translation this is a real Cargo cycle, not a stylistic nit.

**3b. `Main.Prover` / `Main.Session`** — several files reach into `Prover.java` (→ `wr-cli`) for small pieces
that aren't really CLI/REPL concerns:
- `Automaton.java`: `Prover.{COMBINE,FIRST_OP,IF_OTHER_OP}` (op-mode int constants), `Prover.{TXT_EXTENSION,GV_EXTENSION}`, and `Session.getAddressForResult()`/`getReadFileForAutomataLibrary()` (file-path resolution)
- `AutomatonLogicalOps.java`, `FA/ProductStrategies.java`: same `COMBINE`/`FIRST_OP`/`IF_OTHER_OP` constants
- `ParseMethods.java`: **`Prover.determineBase(m)`** — an actual method call (numeration-name parsing logic), not just a constant; the Java source itself has a `// TODO - look at redundancy with Prover.determineAlphabetsAndNS` acknowledging the tangle

**Recommended resolution (same pattern DESIGN.md already used for Automaton↔NumberSystem — kit finding #1):**
identify the minimal shared primitive and relocate it *downward* into `wr-core`/`wr-io` rather than accepting
the cycle:
- Define a bare operator-kind enum (no AST/Token baggage) in `wr-core`; have `wr-logic`'s `Token`/`Operator`
  types wrap or re-export it.
- Hoist the `COMBINE`/`FIRST_OP`/`IF_OTHER_OP` mode constants and `TXT_EXTENSION`/`GV_EXTENSION` into `wr-core`
  as local constants.
- Move `determineBase`'s numeration-name-parsing logic into `wr-core` (next to `NumberSystem`'s own naming
  helpers) and have `wr-io`'s `ParseMethods` call it there, or inline it in `wr-io` if it turns out to be
  purely string-pattern logic with no `NumberSystem` dependency — worth a 10-minute read before deciding.
- `Automaton`'s dependency on `Session` for file-path resolution should become an injected path-provider
  (trait/closure) passed in from `wr-cli`, matching CLAUDE.md's "explicit `Session` context, not globals"
  principle already adopted for the state-refactor.

This is not fatal — it's exactly the kind of thing Phase 0's boundary map exists to catch before Phase 2
code gets built on the wrong assumption. But it's bigger than the single Automaton↔NumberSystem case
DESIGN.md already documents, so DESIGN.md §6/§13 should be updated to record it.

---

## 4. Items needing your call

### 4.1 `NumberSystem.java` negative-base excision — RESOLVED 2026-08-08
Ostrowski/Fibonacci/Pell are **not actually in this file** — Ostrowski lives separately in
`Numeration/Ostrowski.java` (already cleanly DROP-able), and **Fibonacci/Pell numeration doesn't exist
anywhere in this checkout** (only a stray Javadoc example mentioning "Fibonacci[n]" as a *word automaton*
name, unrelated to numeration — DESIGN.md's premise here was slightly off, in the direction of less work
than expected). What `NumberSystem.java` actually mixes is **positive-base with negative-base** (`msd_neg_10`
etc., an `isNeg` boolean field). Negative-base logic is ~10-15% of the file: partly siloed in dedicated
methods (`baseNegNAddition`, `baseNegNLessThan`, `validateNeg`), but `isNeg` also branches inside shared
methods (`setLessThanAutomaton`, `setBaseChangeAutomaton`, `arithmetic()`).

**Decision (user, 2026-08-08): delete negative-base code outright during the port** — do not port it
structurally-present-but-`unimplemented!()`. Rationale (session discussion, not a re-derivable code fact,
recorded here since it's the actual reasoning behind the call): the Java source in `walnut-java` is a
permanent oracle regardless of when the Rust port happens, so a future revival starts from the same
translation step either way — pre-translating now saves nothing there. The genuinely expensive part of
"add negative-base support" is the correctness pipeline (coverage, golden corpus, differential testing,
the mandatory two-reviewer adversarial loop for `wr-core`/`wr-logic` code) — a dormant stub skips none of
that and, in the meantime, is untested/unreviewed dead code sitting in a trust-critical crate, which is
exactly the "half-finished implementation" this project's own conventions say to avoid. If negative-base
is ever wanted, it's a self-contained later port (read Java → translate → test → review), not worse off
for having been deleted now. This also **confirms `split`/`rsplit` as DROP** (§6) rather than
DROP-contingent — the coupling in §6.1 is now settled the same direction.

### 4.2 Split `AutomatonLogicalOps.java` across wr-core/wr-logic, or keep it monolithic in wr-core?
Boolean connectives (`and/or/xor/imply/iff/not`) read as logic-layer semantics; quotient/reverse/`convertNS`/
`combine` read as automaton-algorithm-layer. DESIGN.md itself flagged this file as ambiguous; content confirms
it. Splitting is architecturally cleaner but adds porting/review overhead for uncertain benefit at this
stage — my instinct is **keep it monolithic in wr-core for the mechanical port**, revisit at the idiomatic-
refactor pass, but this is your call since it's a crate-boundary decision (CLAUDE.md routes those to Opus-tier
review either way).

### 4.3 `AutomatonQuantification.java` — move to wr-logic now, or leave in wr-core with DESIGN.md just noting the conceptual mismatch?
Content-confirmed as pure ∃-projection logic despite its `Automata` package location. Recommend actually
targeting `wr-logic` for the port (not just noting it) since it only imports benign utilities from `Main` and
calls forward into `Automaton`/`AutomatonLogicalOps`/`NumberSystem` — a clean forward dependency, no cycle.

### 4.4 `FA/BricsConverter.java`'s regex→automaton engine
Confirms `reg` needs a real regex-to-automaton conversion, not just a test-oracle nicety. DESIGN.md's
dependency table already names `regex-automata` as the intended replacement for the regex-*parsing* half; this
file is the automaton-*construction* half (Brics `RegExp`/`State`/`Transition` → Walnut `FA`). Worth confirming
during Phase 1 spike scoping whether `regex-automata`'s own automaton representation can feed `wr-core`
directly, or whether a hand-rolled Thompson construction (mirroring this file) is still needed.

### 4.5 `Search/ProductBFS.java`'s static mutable state
Not a scope question (KEEP is clear), but a port-design flag: `idOf`/`states`/`prevId`/`prevSym`/`q` are
static fields used as a de facto singleton — not reentrant. Trivial to fix in the Rust port (owned local
state) but flagging so it's not accidentally ported faithfully as `static`/`OnceLock` globals under the
"preserve quirks" mechanical-port rule — this is implementation plumbing, not behavior, so idiomatizing it
immediately (rather than deferring to the later idiomatic pass) seems right. Your call if you want it treated
as a "quirk to preserve then fix later" instead, for strict mechanical-port discipline.

---

## 5. No new files needed for the DROP side
`Numeration/NodeState.java`, `Numeration/Ostrowski.java`, and all 5 `Writer/` CAS-export files
(`AutomatonMatrixWriter`, `MapleEmitter`, `MathematicaEmitter`, `MatlabEmitter`, `SageEmitter`,
`MatrixEmitter`) were investigated (not just assumed) and confirmed genuinely DROP — single-purpose,
single-caller, no internal algorithm depends on them.

---

## 6. TO-CLASSIFY inline commands (DESIGN.md §3 / ROADMAP W7 item 1)

These are regex-dispatched inline in `Main/Prover.java` (`switch (commandName)`, ~L486-574; handler
methods ~L620-814), not routed through a `Commands/` class the way `eval`/`reg`/`combine`/etc. are —
except `split`/`rsplit`/`join`, which *do* have thin `Main/Commands/{Split,Join}.java` handlers (new
files, not yet in §2's table since they live outside `Automata/`). Classified by reading each handler
+ its `Help Documentation/Commands/**/*.txt` entry (the doc tree's own categorization — e.g. `promote`/
`join`/`minimize`/`split`/`rsplit` filed under "Morphisms And Word Automata" — turned out to be an
accurate KEEP signal on its own). All target crate `wr-cli` (dispatch lives in `Prover.java` per
DESIGN.md §6), backed by the `wr-core`/`wr-io` engine noted per row.

| Command | What it does | Backing engine | Verdict | Target crate | Notes |
|---|---|---|---|---|---|
| `promote` | Converts a morphism (`Automata/Morphism.java`) into its equivalent word automaton (DFAO) — e.g. builds the Thue-Morse DFAO from the morphism `0->01 1->10` | `Morphism.toWordAutomaton()` (already KEEP) | **KEEP** | wr-cli | Core to morphic-sequence research — this is literally how an automatic sequence gets turned into a queryable automaton. High-confidence KEEP, not ambiguous. |
| `join` | Combines N DFAOs into one: output = first non-zero among `N1[a][b], N2[b][c], N3[a][c], …` | new `Main/Commands/Join.java` (93 LOC) → `Automata/Automaton`/`WordAutomaton` (in-crate) | **KEEP** | wr-cli | General word-automaton combinator, no numeration-system coupling; useful research primitive. New file to record — not yet in §2's table. |
| `minimize` | Minimizes a **Word Automaton** (DFAO) in place | `WordAutomaton.minimizeSelfWithOutput` (already KEEP) | **KEEP** | wr-cli | Thin CLI wrapper over already-KEEP minimization machinery. DESIGN.md's "almost certainly KEEP" guess confirmed. |
| `convert` | Converts an automaton/word-automaton's number system between `k^i` and `k^j` bases (msd↔lsd, base-power change) | `AutomatonLogicalOps.convertNS` (already KEEP, "confirmed base-*k* only") | **KEEP** | wr-cli | DESIGN.md's "almost certainly KEEP" guess confirmed; no negative-base/Ostrowski involvement. |
| `transduce` | Applies a DFST transducer (e.g. `RUNSUM` running-sum) to an automaton/word-automaton | `Automata/Transducer.java` (already KEEP, "closes a real DESIGN.md gap") | **KEEP** | wr-cli | This command *is* the CLI entry point for the already-KEEP `Transducer` engine — DESIGN.md's "plausibly KEEP" guess confirmed. |
| `fixleadzero` | Strips leading zeros from an **msd** automaton's accepted language (`0* x'` iff old accepts `x`) | `AutomatonLogicalOps.fixLeadingZerosProblem` (already KEEP) | **KEEP** | wr-cli | Base-*k* representation-correctness fixup, not negative-base-specific; needed for correct msd automatic-sequence automata. |
| `fixtrailzero` | Same as above, for **lsd** trailing zeros | `AutomatonLogicalOps.fixTrailingZerosProblem` (already KEEP) | **KEEP** | wr-cli | Same reasoning as `fixleadzero`. |
| `inf` | Decides whether an automaton accepts infinitely many inputs; if so, returns a `prefix(cycle)*suffix` witness regex | `ProverHelper.infFromAddress` → `Automata/FA/Infinite.java` (already KEEP, "closes a real DESIGN.md gap") | **KEEP** | wr-cli | CLI entry point for the already-KEEP `Infinite` engine. |
| `export` | Exports an automaton/word-automaton to **Graphviz (`.gv`) or BA (`.ba`) format only** — `.txt` is refused as "redundant" | `ProverHelper.exportAutomata` → `Automata/Writer/AutomatonWriter.java` (already KEEP: `exportToBA`, `writeToGV`) | **KEEP — scope correction** | wr-cli | **Surprising finding:** this command is *not* the CAS-matrix export DESIGN.md's phrasing worried about. `ProverHelper.exportAutomata`'s `switch` has exactly 3 cases (`BA`, `GV`, and `TXT`→throws) — **no path to `AutomatonMatrixWriter`/the 4 CAS emitters at all**. Those are wired to a *different* trigger (`EvalDef.writeMatrices`, per §2's `AutomatonMatrixWriter.java` row) that isn't reachable from this `export` command. So DESIGN.md's TO-CLASSIFY listing of `export` alongside CAS concerns was imprecise — the inline `export` command is a plain already-KEEP-backed automaton writer, unrelated to the DROP-confirmed CAS path. |
| `split` | Given a DFAO defined over a **negative base** (`msd_neg_k`), splits it into DFAO(s) over the corresponding **positive base**, handling signed inputs separately | new `Main/Commands/Split.java` (123 LOC) → `Automata/Automaton` + `EvalComputations.Token.ArithmeticOperator` | **DROP — confirmed** (§4.1) | N/A | Entirely negative-base-numeration machinery — the whole point of `split` is converting *out of* `msd_neg_k`. §4.1 resolved 2026-08-08: negative-base is deleted outright, not stubbed, so `split` has no positive-base-only purpose left and drops with it. |
| `rsplit` | Inverse of `split`: given a DFAO over a positive base, produces the corresponding negative-base DFAO | same `Main/Commands/Split.java` (`processSplitCommand(..., isReverse=true, ...)`) | **DROP — confirmed** (§4.1) | N/A | Same reasoning as `split` — they share one implementation (`Split.processSplitCommand`) gated by an `isReverse` flag, so they were always a single porting/dropping unit. |

**Net effect on DESIGN.md §3's "TO CLASSIFY" list:** 9 of 11 are KEEP (high confidence — `promote`/`join`/
`minimize`/`convert`/`transduce`/`fixleadzero`/`fixtrailzero`/`inf`/`export`), all backed by engines
already independently confirmed KEEP in §2 above (no new engine-level scope work, only CLI-dispatch
porting). 2 of 11 (`split`/`rsplit`) are **DROP — confirmed**, per §4.1's resolution. Two new
`wr-cli`-target files recorded here since they live outside `Automata/` and so weren't in scope for
§1-§5's file-by-file pass: `Main/Commands/Join.java` (KEEP), `Main/Commands/Split.java` (DROP — not
ported at all, per §4.1).

### 6.1 Resolved 2026-08-08
`split`/`rsplit` are decided together with §4.1 (`NumberSystem.java` negative-base excision) — both DROP,
confirmed. No longer an open item.

---

## 7. `Main/` package file inventory (Phase 0 W7 item 2 — JaCoCo scoping)

Out of this doc's original stated scope (§0: "Covers every file in `Automata/`"), but needed to build a
JaCoCo include/exclude list, since coverage scoping is codebase-wide, not `Automata/`-only. Classified by
comparing against DESIGN.md §3's explicit KEEP list (`Main/Predicate` + `EvalComputations/{Token,
Expressions}`, `Main/Prover`+`Session`, and the named `Main/Commands/*` classes) plus this session's Item-1
findings, with one new file read to confirm (`Main/Commands/Ost.java`, below). Not a full per-file
responsibility table like §2 — just the KEEP/DROP call needed for coverage scoping.

**KEEP (everything under `Main/` except the two below):** `Prover.java`, `ProverHelper.java`,
`Session.java`, `Predicate.java`, `HelpMessages.java`, `Logging.java`, `MetaCommands.java`,
`TestCase.java`, `UtilityMethods.java`, `WalnutException.java`, all of `EvalComputations/{Token,
Expressions}/*`, and all of `Commands/*` except `Ost.java`/`Split.java` (incl. `Join.java`, confirmed KEEP
in §6, and `Test.java` — kept because it's the sole consumer of the already-KEEP `Search/ProductBFS.java`,
§2). `MetaCommands.java` is worth a specific note: it implements the `[strategy …]`/`[export …]` bracket
metacommand syntax (DESIGN.md §9 F3's subject) — the *mechanism* is KEEP (used for in-scope `SC`/`BRZ`
strategy selection too), even though it can also select DEFERRED strategies (`CCL`/`CCLS`/OTF variants),
same file-level-can't-cleanly-split situation as `DeterminizationStrategies.java` (§2).

**DROP (new finding):** `Main/Commands/Ost.java` (confirmed by reading it — constructs `Ostrowski`
representation/adder automata directly, no non-Ostrowski logic). Not previously listed anywhere (DESIGN.md
§3's KEEP table doesn't mention it, and it's outside `Automata/` so §1-§5 didn't cover it) — it was found
only because building the JaCoCo exclude list required enumerating all of `Main/Commands/`. Drops
consistently with `Numeration/Ostrowski.java` (§2, already DROP).

**JaCoCo excludes wired in `walnut-java/pom.xml`'s `code-coverage` profile** (`report` execution): the 8
already-confirmed-DROP `Automata/` files (§2/§5) + `Main/Commands/Ost.java` + `Main/Commands/Split.java`
(§6). **Deliberately NOT excluded** (can't be cleanly cut at file granularity, noise accepted per the
dispatch mission's own instruction): `NumberSystem.java` (negative-base branches, §4.1) and
`FA/DeterminizationStrategies.java` (deferred `CCL`/`CCLS`/OTF strategy paths, §2) — both will show
less-than-100% coverage from code outside the subset; that's expected, not a gap to chase toward 100% on.
