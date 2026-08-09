# WALNUT-BUGS.md — catalog of Walnut (Java) defects found while porting

**Purpose.** This project's Prime Directive #2 (`CLAUDE.md`) is *mechanical port first, preserve
quirks* — so finding a Walnut bug is never license to silently fix it in the Rust port. But "port
it verbatim and move on" would let real defects rot forever in both the oracle and (once ported)
the clone. This doc is the third option: **log every one here**, in enough detail to (a) fix
upstream in `walnut-java` and (b) make a deliberate replicate-vs-diverge call in `walnut-rs`, on
your schedule rather than as a side effect of whatever agent happened to notice it.

**When to add an entry.** Any time characterization testing, code reading, or porting turns up
Walnut (Java) behavior that's wrong, not just surprising — a real output-correctness defect, a
crash on a plausible input, or a load-bearing behavior that contradicts its own documentation. Pure
dead code and doc/comment-vs-implementation mismatches with no behavioral consequence don't need an
entry here (that's normal `PROGRESS.md` findings-log material); if you're unsure which bucket
something falls in, err toward adding it — a false positive here costs a few lines, a missed real
bug costs a silent wrong answer somewhere downstream.

**Status legend** — `Rust port`: `not yet reached` / `ported verbatim (quirk)` / `diverged
(fixed)`. `Upstream`: `not filed` / `filed <link>` / `fixed <link>`.

---

## WB-001 — Valmari minimize: `q0` silently aliases to the wrong block when not co-reachable to acceptance

- **Where:** `Automata/FA/ValmariDFA.java`, `replaceFields` (`f.setQ0(blocks.S[f.getQ0()])`), root
  cause in `minValmari`'s reachability pre-pass (`rem_unreachable`/`reach`) and the initial
  accept/non-accept `split()`.
- **What:** if `q0` cannot reach any accepting state *while some accepting state exists elsewhere
  in the automaton*, the minimized result's language is wrong — sometimes catastrophically so.
  Root cause: `rem_unreachable` computes the co-reachable-to-accepting set and parks everything
  else at element positions `>= rr`, outside every partition block's tracked `[F, P)` range — but
  those parked states keep the `S[q] = 0` (block id) they got from `Partition::init`'s default,
  because `split()` never revisits positions outside a block's own range. `replaceFields` then
  computes the new start state as `blocks.S[q0]` unconditionally, with no check that `q0` was ever
  actually inside a tracked block. If block `0` ends up being the *accepting* partition after the
  initial split (which side gets id `0` vs `1` depends on which half is smaller), a
  never-reaches-acceptance `q0` aliases onto it anyway.
- **Trigger (minimal, verified):** a 2-state automaton, alphabet size 1: state 0 = `q0`,
  non-accepting, self-loop on the only symbol; state 1 = accepting, self-loop on the only symbol,
  **no edge between them**. Correct minimized result: 1 state, non-accepting (language `∅`).
  Actual Java result: 1 state, **accepting** (language `Σ*`).
- **Verification:** hand-traced the exact algorithm on this input independently (not just run) —
  confirmed by three independent parties: the Rust-porting agent that first found it, the
  coordinator's own hand-trace against `ValmariDFA.java`/`ValmariPartition.java` source, and two
  separate adversarial code reviewers who each re-traced it against source before signing off.
- **Found:** Phase 1 spike, 2026-08-09, while porting `minimize` (`wr-core/src/minimize.rs`,
  commit `f9475e7`).
- **Rust port:** `ported verbatim (quirk)` — `wr_core::minimize::minimize` reproduces this exactly
  (documented at length in that module's doc comment, pinned by
  `minimize_q0_not_co_reachable_walnut_quirk`). **Was safe through the Phase-1 spike's own
  pipeline** (`trim` → `subset_construction` → `minimize`): `subset_construction`'s output only
  ever contains states forward-reachable from its own `q0` by construction, and `trim`'s own
  postcondition is exactly `minimize`'s precondition. **Update (Phase 2, U2, 2026-08-09): a live
  call site now exists** — `wr_core::automaton::Automaton::determinize_and_minimize` (faithfully
  porting `Automaton.java:383-398`) skips `trim` entirely whenever its input is ALREADY
  deterministic (matching Java's identical `!isDeterministic()` guard), so calling it on an
  already-deterministic automaton with a state unreachable from `q0` reaches WB-001 exactly —
  pinned by `determinize_and_minimize_reaches_wb_001_on_an_already_deterministic_input`
  (`automaton.rs`). Faithful to Java (same bug, same call site), so ported verbatim, not fixed with
  an undeclared extra `trim`. No guard exists at the `minimize` API boundary itself (flagged, not
  yet acted on, by the Phase-1 final integration review, commit `52e0bcf`'s discussion) — this
  finding makes that gap concretely reachable rather than hypothetical.
- **Upstream:** not filed. A ~3-line guard (`if blocks.loc[q0] >= rr` after the reachability pass,
  route to the canonical dead-automaton case) would fix it in Java too.
- **Severity:** **critical** — silent wrong answer (not a crash), in the automaton engine's most
  heavily-used operation, for an input shape that's plausible in real usage (any product/intersect
  whose current `q0` happens not to reach an accepting state, called without an intervening trim).

---

## WB-002 — `Infinite.infinite()` throws `NullPointerException` instead of returning "finite"

- **Where:** `Automata/FA/Infinite.java` (`infinite()`/`findPath`/`decode`), interacting with
  `Trimmer.trimAutomaton`'s `Q <= 1` no-op guard.
- **What:** on a degenerate single-state, non-accepting, self-looping automaton (language `∅`,
  trivially finite), `Trimmer.trimAutomaton` no-ops (its guard skips anything with `Q <= 1`), so
  `findPath`'s BFS exhausts immediately and returns `null` for the witness path. `decode(null, r)`
  then iterates over that `null` and throws `NullPointerException`, instead of `infinite()`
  returning `""` (Walnut's convention for "the language is finite, no cycle witness").
- **Trigger:** call `infinite`/the `inf` command on a 1-state, non-accepting automaton with only a
  self-loop.
- **Found:** Phase 0, Item 4 second wave, 2026-08-08 (`phase0-artifacts/PROGRESS.md`, second-wave
  findings). Confirmed by direct reading of the interacting methods, not run against a live crash
  (the characterization-test wave didn't force this exact degenerate case through the CLI; the
  *reasoning* for why it would throw was verified, the throw itself was not observed via `mvn
  test`).
- **Rust port:** `not yet reached` — `Infinite`/`inf` is not yet ported (Phase 2+ scope).
- **Upstream:** not filed.
- **Severity:** moderate — crash (not silent-wrong-answer) on a degenerate but real input shape;
  unclear how often a real query's automaton collapses to exactly this 1-state form before `inf` is
  invoked on it.

---

## WB-003 — `0 * x` short-circuits before the variable operand is bound or validated

- **Where:** arithmetic-expression evaluation (`Main/EvalComputations/Expressions`), the multiply
  path.
- **What:** `0*x`, `x*0`, and similar literal-zero multiplications short-circuit to the constant
  `0` without ever binding `x` into an automaton or checking it's a real, in-scope variable. A
  typo'd or nonexistent variable name silently passes validation as long as it's multiplied by a
  literal `0`.
- **Trigger:** any query containing `0 * <undeclared-or-misspelled-name>` (or the reverse order).
- **Found:** Phase 0, Item 4 second wave, 2026-08-08.
- **Rust port:** `not yet reached` — the parser/expression evaluator is `wr-logic` Phase 2+ scope.
- **Upstream:** not filed.
- **Severity:** minor — silently accepts a malformed query rather than erroring; doesn't produce a
  numerically wrong answer for any *valid* query (the constant-`0` result is correct on its own
  terms), but masks a real class of user typos.

---

## WB-004 — `EvalDef.toString()` NPEs if called before `compute()` populates its result

- **Where:** `Main/Commands/EvalDef.java`, `toString()`.
- **What:** dereferences the `result` field before checking it's been populated. Currently
  **unreachable dead code** (zero production callers), so not exploitable today — logged here
  rather than in the plain dead-code bucket because a future caller (e.g. a debugger/REPL
  convenience, or a refactor that starts calling `toString()` for logging) would hit a real NPE,
  not just inherit inert code.
- **Found:** Phase 0, Item 4 first wave, 2026-08-08.
- **Rust port:** `not yet reached`.
- **Upstream:** not filed.
- **Severity:** cosmetic today (dead code); would be minor if ever exercised.

---

## WB-005 — `Session` setup state goes stale on a second in-process call

- **Where:** `Main/Session.java`, `setPathsAndNames` (name/`sessionWalnutDir` drift) and
  `globalSession` (never reset to `false` once set `true`).
- **What:** two related issues in `Session`'s setup routine, both invisible in real Walnut usage
  (a fresh JVM process per CLI invocation calls setup exactly once) but real if the equivalent
  setup is ever called more than once in the same process — e.g. Rust port hygiene, or a future
  Java test harness / long-running service wrapper:
  1. `setPathsAndNames(sessionDir=null, ...)` recomputes the `name` timestamp field on every call,
     but only derives `sessionWalnutDir` from it on the *first* call — so `getName()`'s reported
     name can drift from the directory actually in use after a second call.
  2. `globalSession` is set `true` but never reset to `false` by a subsequent call that passes
     `globalSession=false` — sticky in one direction only.
- **Found:** Phase 0, Item 4 second wave (`globalSession`) and first wave (`setPathsAndNames`),
  2026-08-08.
- **Rust port:** `not yet reached` — this is exactly the kind of global mutable state `wr-cli`'s
  planned `Session` context struct refactor (`CLAUDE.md`'s "one sanctioned deviation from
  mechanical fidelity") is meant to make impossible by construction; tracked here mainly so the
  refactor's design explicitly accounts for "called more than once" as a real case to get right,
  not just "called once per process" as Java implicitly assumes.
- **Upstream:** not filed.
- **Severity:** minor — no impact on any real single-invocation CLI usage.

---

## WB-006 — `Session.createSubdirectories()` uses `mkdir()`, not `mkdirs()`

- **Where:** `Main/Session.java`, `createSubdirectories()`.
- **What:** single-level directory creation; silently assumes the parent of
  `mainWalnutDir`/`sessionWalnutDir` already exists. A `homeDir` whose parent doesn't exist throws
  rather than creating the full path.
- **Found:** Phase 0, Item 4 first wave, 2026-08-08.
- **Rust port:** `not yet reached` — worth carrying the `mkdir`-not-`mkdir -p` semantic into the
  Rust equivalent deliberately (matches a real install layout assumption), not silently upgrading
  to recursive creation.
- **Upstream:** not filed (arguably working-as-intended for the real install layout; logged for
  the Rust port design note, not as a fix request).
- **Severity:** cosmetic / non-issue in practice; kept for the Rust-port design note.

---

## WB-007 — `TestCase.getMatrixOutput()`'s "no matrices" default is a 4-element list of empty strings

- **Where:** `Main/TestCase.java`, `getMatrixOutput()` /
  `AutomatonMatrixWriter.EMPTY_MATRIX_TEST_CASES`.
- **What:** not a crash or wrong-answer bug — an API-contract surprise. Callers who assume "no
  matrices were written" means `List.of()` (empty list) will be wrong; the real sentinel is a
  4-element list of `""`.
- **Found:** Phase 0, Item 4 first wave, 2026-08-08. Now asserted explicitly in `EvalDefTest.java`
  so it can't regress silently.
- **Rust port:** `not yet reached` — this is CAS-matrix-export-adjacent (`TestCase`/test-harness
  plumbing), out of the KEEP subset per `docs/BOUNDARY-MAP.md`; only relevant if a Rust-side test
  harness ever needs an equivalent sentinel.
- **Upstream:** not filed.
- **Severity:** cosmetic — a documentation/API-clarity issue, not a behavior bug.

---

## WB-008 — `FA.concatStates` uses the second operand's state index 0, not its `q0`

- **Where:** `Automata/FA/FA.java`, `concatStates(FA other, FA N, int originalQ)` —
  `N.t.getEntriesNfaD(originalQ)` (the line that fetches "the second operand's initial-state
  transitions" to graft onto the first operand's final states).
- **What:** the intent is clearly "graft `other`'s initial state's outgoing transitions onto every
  final state of the first operand" (the epsilon-transition simulation a Kleene-style concatenation
  construction needs), but the code reads `other`'s transitions at shifted index `originalQ + 0`
  (state index 0), not `originalQ + other.q0`. This is silently correct only when `other.q0 == 0`
  — true for every automaton that's been round-tripped through Walnut's `.txt` writer (which always
  canonizes, forcing `q0` to `0`, before writing), but not guaranteed for an automaton built or
  mutated in-memory without an intervening canonicalize/write. Contrast `starStates` in the same
  file, which correctly uses `automaton.q0` (not a hardcoded `0`) for the analogous graft.
- **Trigger:** `concat(A, B)` where `B.q0 != 0` at the time of the call — the concatenation grafts
  the wrong state's transitions, producing a language that doesn't match either operand's actual
  continuation behavior at the seam. **This is reachable from the plain CLI, not just an in-memory
  corner case**: `AutomatonReader` sets `q0` to whichever state is declared FIRST in a `.txt` file,
  not necessarily state `0` — a hand-authored `.txt` with a non-zero state listed first reads back
  with `q0 != 0` with no canonicalize step in between. (An earlier version of this entry claimed
  every `.txt`-sourced automaton is `q0 == 0`-canonical; that's wrong — canonicalization only happens
  on *write*, via `Writer/AutomatonWriter`, not on read.)
- **Found:** Phase 2, U1 (`fa.rs`'s `FA` port), 2026-08-09, while porting `concatStates`. Confirmed
  by direct reading of `FA.java:107-124`, then **confirmed live** against the real `walnut-java` CLI
  (`Walnut-all.jar`) during adversarial review of the port: a hand-authored `.txt` with `q0 = 1`
  reproduces exactly the predicted wrong result end-to-end.
- **Rust port:** `ported verbatim (quirk)` — `wr_core::fa::Fa::concat_states` reproduces this exactly
  (documented in its doc comment), pinned by
  `concat_states_quirk_uses_others_state_zero_not_others_q0`.
- **Upstream:** not filed. Fix in Java would be reading `other`'s entries at its actual `q0`
  (shifted by `originalQ`), matching `starStates`'s pattern.
- **Severity:** moderate-to-significant (raised from "moderate" after live confirmation) — silent
  wrong answer, reachable via a real CLI workflow (hand-authored or externally-generated `.txt`
  files with a non-zero first-declared state are not exotic), not just a theoretical in-memory shape.

---

## WB-009 — `FA.concatStates` never clears the first operand's own accepting flags

- **Where:** `Automata/FA/FA.java`, `concatStates(FA other, FA N, int originalQ)`, via its reuse of
  the shared `mergeInTransitions` helper (also used by `starStates`, where this behavior is
  correct).
- **What:** `concatStates` grafts `other`'s (index-0, see WB-008) transitions onto every state of
  the first operand that's currently accepting — but never un-marks those states as accepting
  afterward. For Kleene-star (`starStates`), leaving the old final states accepting is exactly right
  (the starred language includes the empty repetition). For concatenation, it's wrong: whenever ε is
  **not** in `L(other)`, the raw NFA `concatStates` builds accepts `L(first) ∪ L(first)·L(other)`,
  not the documented `L(first)·L(other)` (`Help Documentation/Commands/Automata/concat.txt`:
  "accepts the concatenation of the inputs"). The first operand's language leaks into the result
  as spurious extra accepted strings.
- **Trigger:** `concat(A, B)` where `ε ∉ L(B)` and `L(A)` is nonempty — the result accepts every
  string in `L(A)` in addition to the intended `L(A)·L(B)`.
- **Found:** Phase 2, U1 (`fa.rs`'s `FA` port), 2026-08-09, alongside WB-008. Confirmed by direct
  reading of `FA.java`'s `concatStates`/`mergeInTransitions`, then **confirmed live** against the
  real `walnut-java` CLI during adversarial review of the port: `concat` of `reg "0"` and `reg "1"`
  (a plain, non-starred pair — the only such case in Walnut's own `IntegrationTest.java`,
  `test603`, asserts no language, so this never got caught upstream) produces a result whose second
  state is wrongly accepting, i.e. `L = {"0", "01"}` instead of the documented `{"01"}`.
- **Rust port:** `ported verbatim (quirk)` — `wr_core::fa::Fa::concat_states` reproduces this exactly
  (documented in its doc comment), pinned by
  `concat_states_quirk_leaks_first_operands_language_when_second_lacks_epsilon`.
- **Upstream:** not filed. Fix in Java would un-mark the first operand's states as accepting inside
  `concatStates` itself (or in a dedicated concat-only merge helper, not the shared
  `mergeInTransitions`) before/after the graft.
- **Severity:** significant — silent wrong answer on a very common shape (any `concat` whose second
  operand doesn't accept the empty string, which is the common case for non-star automata); worth
  prioritizing for upstream confirmation given how central `concat` is to word-automaton/morphism
  construction.

---

## Not-yet-confirmed / flagged as a question, not a finding

- **`Image.determineImageNumberSystemPrefix` returns `""`** when the referenced word automaton has
  no number system attached. Flagged during Phase 0 Item 4 as an open question (is indexing a
  non-arithmetic-numeration word automaton via `image()` an actually-supported scenario?), not a
  confirmed bug — needs a real answer before it earns a `WB-` entry either way.

---

## Dead code / doc-vs-implementation mismatches (tracked in `PROGRESS.md`, not duplicated here)

Several Phase 0 findings are confirmed-dead code or javadoc/implementation mismatches with **no
behavioral consequence** — `NumberSystem.getMultiplication(int)`/`getDivision(int)` (dead, zero
callers), `NumberSystem.setBaseChangeAutomaton`'s `isNeg==false` arms (dead, unreachable given the
only caller), `AutomatonLogicalOps.zeroReachableStates`'s undersold comment, and
`AutomatonLogicalOps.removeStatesWithOutputRebuild`'s javadoc overclaiming what it does. These stay
in `walnut-java/phase0-artifacts/PROGRESS.md`'s dated entries (searchable there) rather than being
duplicated here, per this doc's own scope note above — promote one to a `WB-` entry if it's ever
found to have a real behavioral edge after all.
