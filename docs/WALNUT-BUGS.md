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
- **Trigger (exact, empirically confirmed — not an approximation):** the sole state must be BOTH
  non-accepting AND have at least one outgoing transition (necessarily a self-loop, the only
  possible destination when `Q == 1`). Working through `findCycle`/`findPath`/`decode` for all four
  `Q == 1` sub-cases (and confirming each empirically — `InfiniteTest.testSingleStateSelfLoop-
  WithNoAcceptingStateThrowsNPE` is a *passing* JUnit test for the crashing row; the other three
  rows were checked with a standalone driver calling `Infinite.infinite` directly against
  `target/classes`, Phase 3a U7 review pass):

  | state 0 accepting? | self-loop present? | Java's `infinite()` result |
  |---|---|---|
  | yes | yes | `"([0])*"` (genuinely infinite) |
  | yes | no | `""` (finite — language is `{ε}`) |
  | **no** | **yes** | **`NullPointerException`** |
  | no | no | `""` (finite — language is `∅`) |

  A `Q == 1` automaton whose sole state is either accepting, or has no transitions at all, does
  **not** crash. Also confirmed empirically: a completely empty-language automaton with `Q > 1`
  does not crash either — `Trimmer.trimAutomaton`'s `Q > 1` path genuinely runs, and
  `Trimmer.quotient`'s `statesToKeep.isEmpty()` branch collapses the automaton to a single state
  with **zero** transitions (not a self-loop), so `findCycle` finds nothing and cleanly returns
  `""`. So the crash needs `Q == 1` (trimming skipped) specifically, not merely "empty language."
- **Found:** Phase 0, Item 4 second wave, 2026-08-08 (`phase0-artifacts/PROGRESS.md`, second-wave
  findings); trigger condition narrowed to the exact table above and empirically confirmed (both
  the crashing row via the pre-existing passing JUnit test, and the three non-crashing rows via a
  standalone driver against `target/classes`) during Phase 3a's U7 refinement pass, 2026-08-12.
- **Rust port:** `diverged (fixed)`, precisely — not a blanket "every empty-language input"
  guard (an earlier draft of this fix over-broadened the trigger; corrected here). `wr_core::
  infinite::infinite` (`crates/wr-core/src/infinite.rs`) reproduces Java's exact crash trigger as a
  recoverable `Result::Err(InfiniteError::DegenerateSelfLoop)`, checked on the **untrimmed** input
  (`a.fa.q == 1 && !is_accepting(q0) && a.fa.d[q0].values().any(|dests| !dests.is_empty())`) before
  `crate::trim::trim` ever runs — matching the WB-011/WB-012/WB-013 precedent that a genuine Java
  `RuntimeException` `Prover.dispatch`'s top-level `catch (RuntimeException e)` recovers from is
  more faithfully ported as a `Result` than an uncaught `panic!` (which would unwind and kill the
  process, not "print a stack trace and keep the session alive" the way Java's crash actually
  behaves).

  **Post-landing correction (Phase 3a U7 review, 2026-08-12):** two independent adversarial reviews
  of the original guard, `!d[q0].is_empty()` (a `BTreeMap<i32, Vec<usize>>` non-emptiness check),
  each separately found it tests only whether the map has *any symbol key present*, not whether that
  key's destination `Vec` is actually non-empty. A `Fa` can have a symbol key mapped to an *empty*
  destination list (constructible via direct `Fa` manipulation — `fa.rs`'s
  `canonicalize_prunes_entries_with_empty_destination_list` test builds exactly this shape,
  precisely because `canonicalize()` exists to prune it); that shape has no real transition at all,
  so the original guard was a false positive, converting a correct `Ok(None)` (finite) into a
  spurious `Err`. Fixed to check for an actual non-empty destination (`.values().any(|dests|
  !dests.is_empty())`), matching how `find_cycle`/`find_path` in the same file already traverse
  `dests` without checking map non-emptiness. Pinned by
  `infinite::tests::single_state_empty_destination_list_is_finite_not_an_error`.

  A second, separate guard remains **after** trimming
  (`trimmed.is_language_empty()` → `Ok(None)`), but it is *not* part of the WB-002 fix and is *not*
  a Java divergence: it exists purely because this port's own `crate::trim::trim` (shipped in Phase
  2, out of U7's scope) collapses **every** empty-language input, any `Q`, to a canonical
  **fully self-looping** 1-state automaton (`trim.rs`'s own module docs), unlike Java's
  `Trimmer.quotient`, whose analogous collapse leaves the canonical state **transition-less**.
  Without this second guard, the `Q > 1` empty-language case would trip an internal invariant this
  port's own DFS relies on (a newly-introduced Rust-side failure mode Java never has, since Java's
  `Q > 1` empty-language path never re-hits `findCycle`/`findPath` at all). With the guard, `Q > 1`
  empty language answers `Ok(None)` (finite) — the same answer real Java gives, just reached by a
  structurally different mechanism on both sides, exactly like Java's own `Q > 1` collapse differs
  structurally from its `Q <= 1` case. **This was never actually a divergence** to begin with; an
  earlier draft of this entry incorrectly described it as one.

  Pinned by `infinite::tests::single_state_self_loop_with_no_accepting_state_errors` (the real
  crash trigger, now asserting `Err`, direct port of `InfiniteTest.testSingleStateSelfLoop-
  WithNoAcceptingStateThrowsNPE`), `infinite::tests::single_state_no_self_loop_is_finite_not_an_
  error`, `infinite::tests::single_accepting_state_with_self_loop_is_infinite_not_an_error`, and
  `infinite::tests::single_accepting_state_no_self_loop_is_finite_not_an_error` (the three
  non-crashing `Q == 1` sub-cases, proving the guard doesn't over-fire), and
  `infinite::tests::empty_language_is_finite_regardless_of_state_count` (the `Q > 1` case, proving
  the pre-trim `DegenerateSelfLoop` guard correctly does *not* fire there, and the port still
  answers "finite").
- **Upstream:** not filed.
- **Severity:** moderate — crash (not silent-wrong-answer) on a degenerate but real input shape;
  unclear how often a real query's automaton collapses to exactly this 1-state, non-accepting,
  self-looping form before `inf` is invoked on it.
- **Note (audit trail, not a separate bug):** `wr_core::infinite::infinite`'s guard-reordering fix
  above also introduced a second, harmless, previously-uncalled-out divergence: its pre-trim
  `a.fa.q == 0 || a.fa.q0 >= a.fa.q` check is intentionally *more* defensive than Java's real
  behavior. Java's `Trimmer.trimAutomaton` only tolerates an out-of-range `q0` incidentally, via its
  `getQ() <= 1` no-op guard — for `Q > 1`, an invalid `q0` would crash Java's own `Trimmer`
  differently (not via WB-002's `NullPointerException` path) rather than being caught cleanly. This
  port's guard catches an invalid `q0` for *every* `Q`, not just `Q <= 1`, which changes no answer
  for any well-formed automaton and is strictly safer, but is worth recording for completeness since
  it's a second, distinct place this same guard-reordering diverges from literal Java behavior.

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

## WB-010 — `AutomatonLogicalOps.leftQuotient` checks the subset containment in the wrong direction

- **Where:** `Automata/AutomatonLogicalOps.java`, `leftQuotient` (`:237-256`), specifically the
  guard at `:242` (`isSubsetA(A, B)`, i.e. "A's alphabet ⊆ B's alphabet") followed by the call to
  `rightQuotient(reverse(A), reverse(B), skipSubsetCheck=true)` at `:248`.
- **What:** `rightQuotient` re-encodes the SECOND operand's transition symbols under the FIRST
  operand's alphabet (`RichAlphabet.encode` inside `rightQuotient`, `:198`) — which requires the
  *opposite* containment (second's alphabet ⊆ first's) to be safe. `leftQuotient` checks "A ⊆ B"
  but then calls `rightQuotient` with A reversed as the FIRST argument and B reversed as the
  SECOND — so the containment `rightQuotient` actually needs is "B ⊆ A", not "A ⊆ B". It then
  passes `skipSubsetCheck=true`, disabling `rightQuotient`'s own (correct-direction) guard
  entirely. The two checks coincide only when the alphabets are equal as sets — which is not
  guaranteed and not checked.
- **Trigger:** `leftquo` (or the equivalent CLI/API call) with A's alphabet a proper subset of B's
  alphabet as a SET — e.g. A over `{0,1}` and B over `{0,1,2}`. `isSubsetA(A, B)` passes (A ⊆ B is
  true), but the actual re-encoding inside `rightQuotient` needs B's digits to all appear in A's
  alphabet, which fails for digit `2`. In Java this manifests as `RichAlphabet.encode` hitting
  `indexOf == -1` and silently producing a corrupt (possibly negative) symbol id — a silent
  wrong-automaton result, not a crash. Reachable from the plain CLI: `Main/Commands/Quotient.java`
  reads both operands straight from `.txt` files with no alphabet normalization in between.
- **Found:** Phase 2, U5 (`logicalops.rs`'s `AutomatonLogicalOps` port), 2026-08-09, while porting
  `leftQuotient`/`rightQuotient`. Confirmed by direct reading of the guard logic and the
  re-encoding direction it's meant to protect — not yet run against a live Java reproduction.
- **Rust port:** `ported verbatim (quirk)` — `wr_core::logicalops::left_quotient` reproduces the
  same wrong-direction guard. Note the FAILURE MODE differs from Java's, faithfully inheriting an
  earlier, deliberate crate-wide improvement (not something introduced for this bug):
  `Automaton::encode` already panics on an out-of-alphabet digit (`PORTING.md`'s error-mapping
  table calls for a hard error over Java's silent `List.indexOf`-returns-`-1` corruption), so this
  port surfaces the same underlying guard defect as a clean panic instead of a silently wrong
  automaton.
- **Upstream:** not filed. Fix in Java would be checking `isSubsetA(B, A)` (or, more robustly,
  requiring the alphabets be equal as sets, matching how same-labeled-track merges elsewhere in
  this codebase are guarded) before the `rightQuotient(reverse(A), reverse(B), true)` call.
- **Severity:** moderate — silent wrong answer in Java (this port turns it into a clean panic, not
  a fix); reachable whenever `leftquo`'s two operands have genuinely different (non-equal-as-sets)
  alphabets, which is a plausible real usage shape, not a contrived corner case.

---

## WB-011 — `ParseMethods.parseMorphism` crashes on a bracketed symbol its own regex accepts

- **Where:** `Automata/ParseMethods.java`, `parseMorphism` (`:166-193`), specifically the
  `Integer.parseInt(input)` / `Integer.parseInt(imagePiece)` calls at `:187`/`:185` on text stripped
  of its surrounding `[`/`]` by hand (`input.substring(1, input.length() - 1)`,
  `imagePiece.substring(1, imagePiece.length() - 1)`) — NOT `UtilityMethods.parseInt`, which strips
  whitespace before parsing (used everywhere else in this same file).
- **What:** the bracket grammar shared by `MORPHISM_INPUT_SYMBOL`/`MORPHISM_IMAGE_SYMBOL`
  (`MORPHISM_COMMON_SYMBOL = "\\[(?:[+\\-])?\\s*\\d+\\]|"`) explicitly permits whitespace between
  the optional sign and the digits — e.g. `[+ 5]` matches the pattern. But the two call sites above
  hand that raw (unstripped) inner text straight to plain `Integer.parseInt`, which does NOT accept
  embedded whitespace and throws `NumberFormatException` on exactly the input the regex just
  accepted. A self-contradiction between what the grammar allows and what the parser that consumes
  its own capture can handle, not a mere quirk.
- **Trigger (verified against the real regex, javac'd standalone):** `parseMorphism("[+ 5] -> 1")`
  — the pattern matches `input = "[+ 5]"` cleanly (`m1.group(1)`), but
  `Integer.parseInt("+ 5")` (after bracket-stripping) throws
  `NumberFormatException: For input string: "+ 5"`, uncaught, aborting the whole call. Any morphism
  definition using bracketed symbol notation with a space after the sign hits this. **Correction
  (Phase 3a, U0b review pass):** an earlier version of this entry claimed no test anywhere exercises
  bracket notation at all — that overclaimed. `ParseMethodsTest` (in `walnut-java`) indeed has no
  bracket-notation coverage, but `Automata/MorphismTest.testBigAlphabet` (`:34-41`) DOES exercise
  bracket notation directly (`new Morphism("0->01 [11]->012 [12]->02")`) — it just never happens to
  hit THIS specific whitespace-after-sign shape (`[11]`/`[12]` have no sign at all, let alone a space
  after one), so it doesn't cover this bug. The accurate claim is narrower: no existing test (in
  `ParseMethodsTest`, `MorphismTest`, or any `Morphism Library/*.txt` fixture) exercises a bracketed
  symbol with whitespace between its sign and digits — that specific shape has no coverage on either
  side, but it is real, reproducible, user-triggerable syntax, not a contrived construction.
- **Found:** Phase 3a, U0b (`wr-io::parse_methods` port), 2026-08-09, while porting `parseMorphism`.
  Verified directly against the actual compiled Java `Pattern`/`Integer.parseInt` behavior (not just
  read) via a standalone `javac`'d reproduction using the real regex strings copied from
  `ParseMethods.java`.
- **Rust port:** `ported verbatim (quirk)` — `wr_io::parse_methods::parse_morphism` reproduces the
  same crash, surfaced as `Err(ParseMethodsError::IntegerParseFailure)` rather than Java's uncaught
  `NumberFormatException` (a `Result` rather than a panic, following this port's convention of
  keeping anything reachable straight from raw user-typed command text recoverable — see
  `NumSysError::BaseNotAnI32` for the same convention applied elsewhere). Pinned by
  `morphism_bracket_whitespace_quirk_wb_011` in `crates/wr-io/src/parse_methods.rs`.
- **Upstream:** not filed. Fix in Java would be using `UtilityMethods.parseInt` (whitespace-stripping)
  at both call sites, matching every other numeric-text-to-`int` conversion in the same file.
- **Severity:** low — reachable only via bracket-notation morphism symbols with an internal space
  after the sign, a narrow and untested corner of an already narrow (no golden-corpus coverage at
  all, per the Phase-3a plan's gap #11) feature; flagged per `CLAUDE.md`'s bug-logging rule (a crash
  on syntax the code's own grammar declares valid) rather than silently ported or silently "fixed."

---

## WB-012 — `UtilityMethods.commonRoot(a, 0)` recurses forever for negative `a`

- **Where:** `Main/UtilityMethods.java`, `commonRoot` (`:123-134`), specifically the final branch:
  `return (b % a == 0) ? commonRoot(a, b / a) : NO_COMMON_ROOT;`.
- **What:** for `a < 0` and `b == 0` (reached either directly, or via the `a > b` swap one level up
  for `a == 0, b < 0`), `b % a` is `0 % a == 0` for any nonzero `a` in Java, so the method recurses
  into `commonRoot(a, b / a)` — and `0 / a == 0` for any nonzero `a`, so the recursive call is
  `commonRoot(a, 0)`, i.e. **exactly the same arguments as the current call**. Every subsequent
  recursion is identical to the one before it; there is no base case that terminates this shape.
- **Trigger:** `commonRoot(-3, 0)` (or the symmetric `commonRoot(0, -3)`, which swaps into the same
  shape via the `a > b` branch). In Java this recurses until `StackOverflowError` — a crash, and a
  loud one, but not silent. The positive-`a`/`b == 0` shape (e.g. `commonRoot(3, 0)`) does NOT hang:
  it swaps to `commonRoot(0, 3)`, then hits `3 % 0`, Java's integer `%` on a zero divisor, which
  throws `ArithmeticException: / by zero` immediately — a different, non-hanging crash for what's
  really the same "no coherent common root" input shape.
- **Found:** Phase 3a, U0b review pass (`crates/wr-core/src/util.rs`'s `common_root`, the port of
  this method), 2026-08-09. Confirmed by direct trace of the recursion (`0 % a == 0` and `0 / a ==
  0` for any nonzero integer `a` in Java's semantics) — not yet run against a live
  `StackOverflowError` reproduction, but the arithmetic is unambiguous.
- **Rust port:** **deliberate divergence, not a verbatim port.** `wr_core::util::common_root` adds an
  explicit `a == 0 || b == 0` guard (placed after the pre-existing `a == b` check, so
  `commonRoot(0, 0) == 0` is unaffected and still matches Java) that returns `NO_COMMON_ROOT`
  cleanly instead of recursing. This is the one function in this crate's Tier-4 discipline
  (`CLAUDE.md`: "yields a `TIMEOUT`/`EXPLODED` verdict... instead of hanging") where faithfully
  porting Java's behavior is not a coherent goal: Java's own `StackOverflowError` is a crash, but the
  identical-arguments self-recursion is exactly the shape a Rust release build's tail-call
  optimization turns into a **genuinely infinite loop with no stack growth at all** — a silent hang,
  which this project's stated discipline treats as strictly worse than a crash. Reproducing either of
  Java's two failure modes (a slow stack-overflow crash for negative `a`, an immediate
  divide-by-zero crash for positive `a`) for what's really one degenerate input shape isn't worth it
  either, so both guard the same way. Verified the guard changes nothing for any `(a, b)` pair where
  neither operand is zero (provable by construction — the new check is a no-op unless
  `a == 0 || b == 0` — and spot-checked directly against an unguarded reference reimplementation over
  the full `[-6, 20]²` grid in `common_root_matches_unguarded_reference_away_from_zero`).
- **Upstream:** not filed. Fix in Java would be an explicit `a == 0 || b == 0` early return (mirroring
  the Rust guard), placed before the `a > b` swap so both the negative-`a` hang and the
  positive-`a`/zero-`b` `ArithmeticException` are replaced with a clean, documented `NO_COMMON_ROOT`.
- **Severity:** low in Java today — `commonRoot`'s only caller is
  `AutomatonLogicalOps.java:482` inside `convertNS`, which is out of scope for this port
  (`docs/BOUNDARY-MAP.md`), so this isn't reachable from any code path this port currently exposes.
  Logged now (rather than deferred) because the Rust port's own `common_root` was being touched in
  this same review pass and the divergence needed to be a deliberate, documented choice per
  `CLAUDE.md`, not a silent one.

---

## WB-013 — `VariableExpression.act` NPEs when the same variable indexes a `{...}`-declared track twice

- **Where:** `Main/EvalComputations/Expressions/VariableExpression.java`, `act` (`:34-48`),
  specifically `ns.equality.clone()` (`:39`) in the repeated-identifier branch.
- **What:** `VariableExpression.act`'s `ns` parameter is whatever `Word.act` passed in from
  `wordAutomaton.getNS().get(i)` (`Word.java:62`). `Automaton.NS` is populated one entry per track by
  `ParseMethods.parseAlphabetDeclaration` (`Automata/ParseMethods.java:84-109`): a `msd_k`/`lsd_k`
  token adds a real `NumberSystem` (`:98-103`), but an explicit-alphabet token like `{0,1}` adds a
  literal `null` (`:91-96`, `bases.add(null)`) — Walnut deliberately supports tracks with no attached
  number system. `VariableExpression.act`'s FIRST occurrence of a variable never touches `ns` (it
  just records the identifier), but a REPEATED occurrence of the same variable dereferences
  `ns.equality` unconditionally. So a word automaton with an explicit-alphabet-declared track,
  referenced by the SAME index variable more than once in one `eval`/`def` query — e.g. `T[i][i] =
  @1` where `T`'s second track is declared `{0,1}` rather than `msd_k`/`lsd_k` — throws a real
  `NullPointerException`, not a `WalnutException`. This is a genuine, user-triggerable crash on
  syntactically valid input, not dead code or a doc mismatch.
- **Trigger:** an `eval`/`def` query indexing a `{...}`-declared word-automaton track with a
  repeated variable, e.g. `T[i][i] = @1` for a `T` whose relevant track has no `msd_k`/`lsd_k` base.
- **Found:** Phase 3a, U2 adversarial review (`crates/wr-logic/src/expr.rs`'s
  `VariableExpression::act`), 2026-08-12. Confirmed by direct trace of the call chain
  (`VariableExpression.java:34-48` -> `Word.java:62` -> `ParseMethods.java:84-109`'s
  `bases.add(null)` branch), not yet run against a live crash reproduction — the arithmetic/control
  flow is unambiguous (an unconditional dereference of a value provably `null` on this path).
- **Rust port:** `ported verbatim (quirk)`, but represented as an explicit, documented
  `Result::Err` rather than a `panic!`. `VariableExpression::act`'s `ns` parameter is
  `Option<&NumberSystem>` (`None` standing in for Java's `null`); the repeated-identifier branch
  returns `Err(ExprError::RepeatedIdentifierMissingNumberSystem)` instead of dereferencing. A
  `panic!` was deliberately rejected here (unlike, e.g., `relational_op_from_symbol`'s panic for an
  unreachable-by-construction internal invariant): Java's NPE is an unchecked `RuntimeException`
  that `Prover.dispatch`'s top-level `catch (RuntimeException e)` (`Prover.java:390`) recovers from
  — prints a stack trace, the session continues — so an uncaught Rust `panic!` here would be *less*
  faithful, not more: absent a `catch_unwind` boundary this port doesn't have yet, it would unwind
  and kill the whole process, the opposite of Java's actual recoverable behavior. (Same reasoning
  `wr_core::logging`'s module doc already applies to `dedent()`'s `IllegalArgumentException`, and
  the same "recoverable crash on raw user-typed input stays a `Result`" convention WB-011's entry
  above documents for `parse_morphism`.) Pinned by
  `variable_expression_act_repeated_occurrence_with_no_ns_reports_the_java_npe_shape` in
  `expr.rs`. Not yet wired to any real caller — `Word`/`Function` (the only real source of a `None`
  `ns`) are deferred to U4, so this is a signature-level fix ahead of its first live call site, not
  something exercised through the CLI yet.
- **Upstream:** not filed. A ~3-line guard in `VariableExpression.act` (throw a real
  `WalnutException` naming the offending variable/track when `ns == null` in the repeated-identifier
  branch, rather than falling through to the NPE) would fix it in Java too.
- **Severity:** moderate — a real crash (not silent-wrong-answer) on syntactically valid, plausible
  input (any multi-track word automaton mixing an `msd_k`/`lsd_k` track with an explicit-alphabet
  track, indexed by a repeated variable) rather than a contrived construction; not yet reachable
  through walnut-rs's own CLI (no lexer/`Word`/`Function` yet), so no *live* user impact in this port
  today.

---

## WB-014 — `NumberSystem.getComputeIfAbsent` is reentrant, so loading a custom base whose files declare a number system throws `ConcurrentModificationException`

- **Where:** `Automata/NumberSystem.java:211-213`
  (`numberSystemHash.computeIfAbsent(base, NumberSystem::new)`), reached recursively through
  `NumberSystem(String)` -> `setAdditionAutomaton` (`:322`) -> `loadAutomatonOrNull` (`:299-319`) ->
  `new Automaton(mainName)` -> `AutomatonReader` -> `ParseMethods.parseAlphabetDeclaration`
  (`Automata/ParseMethods.java:99-100`), which calls `NumberSystem.getComputeIfAbsent` **again**.
- **What:** `java.util.HashMap.computeIfAbsent` forbids its mapping function from structurally
  modifying the map, and detects the violation (since JDK 9) by throwing
  `ConcurrentModificationException`. Here the mapping function is the `NumberSystem` constructor
  itself, and for a **custom base** that constructor reads `Custom Bases/<name>_addition.txt` (and
  `<name>_less_than.txt`, and `<name>.txt`). If any of those files declares its alphabet with a
  number-system token (`msd_2 msd_2 msd_2`) rather than an explicit set (`{0,1} {0,1} {0,1}`), the
  reader resolves that token through `getComputeIfAbsent`, inserting into the very map the outer
  `computeIfAbsent` is still executing over. Result: the whole command dies with a bare
  `java.util.ConcurrentModificationException` and a one-line stack trace, with no indication that the
  problem is the custom-base file's header.
  Both header forms are fully legal in the `.txt` format, and Walnut's own `Word Automata Library`
  files routinely use the `msd_k` form — so this is a plausible authoring choice, not a contrived one.
  (Nothing in `walnut-java/Custom Bases/` happens to use it today, which is why the bug has gone
  unnoticed: all nine shipped bases declare `{0, 1}`-style explicit alphabets.)
- **Trigger (minimal, verified against the real CLI):** create
  `Custom Bases/msd_wrtest_addition.txt` containing
  ```
  msd_2 msd_2 msd_2

  0 1
  0 0 0 -> 0
  ```
  then run `eval wrtest1 "?msd_wrtest x=x";`. Actual output:
  `java.util.ConcurrentModificationException at java.base/java.util.HashMap.computeIfAbsent(HashMap.java:1221)`.
  (Run against `target/Walnut-all.jar`, JDK 17, 2026-08-12. A *self*-referential header — a
  `msd_foo_addition.txt` declaring `msd_foo` — instead recurses until the stack overflows; that half
  is arguably user error, the cross-base case above is not.)
- **Found:** Phase 3a, U5 (custom-base `NumberSystem` loading), 2026-08-12, while tracing what the
  I/O-free builder's callers would have to do.
- **Rust port:** `not reached` — and the port's architecture makes it unreachable by construction
  rather than by accident, which is worth recording explicitly so nobody "restores fidelity" later.
  `wr_core::numsys::NumberSystem::with_custom_base_files` takes ALREADY-PARSED automata and performs
  no I/O, so the constructor cannot re-enter the name -> `NumberSystem` cache. The cache itself lives
  outside `wr-core` (`wr_logic::predicate_env`'s `InMemoryPredicateEnv`, and later U14's `Session`),
  behind a `RefCell<BTreeMap<..>>` whose lookup borrow is released before the constructor runs — so
  a reentrant resolution would be a plain nested lookup, not a panic. **Consequence for U13** (the
  `wr-io` reader's custom-base header support): reading a custom-base file whose header names another
  number system must resolve that name through the same cache, and will *succeed* where Java throws.
  That is a divergence, and it is the one this entry exists to make deliberate — recorded here rather
  than replicated, since replicating it would mean deliberately engineering a self-modification panic
  into the resolver.
- **Upstream:** not filed. Two independent fixes exist in Java: (a) replace `computeIfAbsent` with an
  explicit `get`/construct/`put` (which is reentrancy-safe and preserves the existing
  "failed construction is not negatively cached" behavior), or (b) resolve the alphabet declaration's
  number systems lazily, after the outer construction completes.
- **Severity:** moderate — a hard crash with a useless message (not a silent wrong answer), on
  syntactically valid input, but only for a user authoring their own custom base with an `msd_k`-style
  alphabet header. Nothing in the shipped corpus triggers it, so no golden fixture covers it.

---

## WB-015 — `Predicate`'s tokenizer records three classes of wrong `positionInPredicate`, all surfaced in user-visible error text

- **Where:** `Main/Predicate.java`, `tokenizeAndComputePostOrder` (`:228-237`, the two parenthesis
  arms) and `handleQuantifier` (`:276-285`). Every other arm of the same scanning loop uses the
  form `realStartingPosition + matcher.start(1)`; these three deviate from it, each differently.
- **What:** `Token.positionInPredicate` is Walnut's "char at N" pointer into the user's query. It is
  read in exactly two places — `Predicate.java:248`'s `unbalancedParen(op.getPositionInPredicate())`
  and `EvalDef.compute`'s catch block (`Main/Commands/EvalDef.java:126`, which appends
  `System.lineSeparator() + "\t: char at " + t.getPositionInPredicate()` to the message of ANY
  `RuntimeException` a token's `act()` throws) — so a wrong value is a wrong diagnostic, printed to
  the user, not dead data. Three distinct defects:
  1. **Both parenthesis tokens record the pre-whitespace cursor, not the parenthesis.**
     `new LeftParenthesis(realStartingPosition + index)` / `new RightParenthesis(realStartingPosition
     + index)` use the loop's `index`, i.e. the offset where scanning *began*, while the pattern
     itself is `\G\s*\(` — so any whitespace before the parenthesis shifts the reported position
     backwards onto that whitespace (or onto the end of the previous token).
  2. **A quantifier operator and its quantified variables omit `realStartingPosition` entirely.**
     `handleQuantifier` passes the bare `MATCHER_FOR_LOGICAL_OPERATORS.start()` and
     `MATCHER_FOR_LIST_OF_QUANTIFIED_VARIABLES.start()`. Both are offsets into *this* `Predicate`'s
     own string, which for a nested `Predicate` (a word index, `:310`, or a function argument,
     `:462-463`) is a fragment of the real query — so the reported position is in the wrong
     coordinate space, and points at an unrelated character of the query (or, for a short query,
     off the front of the construct entirely). It is also a whole-match start rather than
     `start(1)`, so it has defect 1's leading-whitespace problem on top.
  3. **Every variable in one quantifier's list shares one position** — the list match's start —
     instead of its own offset, so `E x, y, z` reports `x`'s position for a complaint about `z`.
- **Trigger (empirically confirmed against the real `walnut-java`, via a probe that constructs
  `new Predicate(line)` and prints each token's `getPositionInPredicate()`, run against
  `target/Walnut-all.jar`, JDK 19, 2026-08-12):**
  - defect 1: `new Predicate("a=1 & ( b=2")` throws `unbalanced parenthesis: char at 5`; the `(` is
    at offset 6, and offset 5 is a space. (This one reaches the user through Walnut's own error
    text with no `eval` needed.)
  - defect 2: `new Predicate("T[  Ex (x=1)  ]=@1")` (with a real `T` in the word library) yields
    token positions `x=3, x=8, 1=10, ==9, E=0, T=0, 1=17, ==15`. The nested index predicate's
    `realStartingPosition` is 2, and its ordinary tokens correctly show it (the inner `x` at nested
    offset 6 reports 8) — but the `E` reports **0** and its quantified `x` reports **3**, both
    missing the +2. With a longer prefix the gap is larger: `"xxxxxxxxxx & T[Ey (y=1)]=@1"` reports
    the quantified `y` at **1** where the correct position is 16.
  - defect 3: `new Predicate("E x, y, z (x=y)")` yields positions `1,1,1` for the three quantified
    variables.
  - The `EvalDef.compute` half of the surface (defects 2/3's route to the user) is a one-line trace
    rather than a live reproduction: a quantifier's `act()` really can throw on plausible input
    (`WalnutException.notFreeVariable`, "Variable X in the list of quantified variables is not a
    free variable."), and `EvalDef.compute` unconditionally appends that token's position — so e.g.
    `T[Ey (x=1)]` prints a "char at" pointing into the wrong coordinate space. Not run end-to-end
    through the CLI, unlike the three token/error observations above.
- **Found:** Phase 3a, U3 (`crates/wr-logic/src/predicate.rs`, the `Predicate` tokenizer port),
  2026-08-12.
- **Rust port:** `ported verbatim (quirk)` — all three. `predicate.rs`'s scanning loop uses
  `self.position(index)` (i.e. `realStartingPosition + offset`) for the parenthesis arms exactly as
  Java does, and `handle_quantifier` uses `self.java_offset(...)` (no `realStartingPosition`) for
  both the operator and every variable, with the list's start reused for all of them. Each is
  flagged in a code comment naming this entry, and pinned by a dedicated test asserting the
  *defective* values: `parenthesis_positions_are_the_pre_whitespace_cursor_wb015`,
  `quantifier_token_positions_omit_real_starting_position_wb015`,
  `all_quantified_variables_share_one_position_wb015`. Note the port fixes a *different*, unrelated
  position problem that Java does not have: `predicate.rs` converts its UTF-8 byte cursor to UTF-16
  code units before recording any position, so the two legal non-ASCII grammar characters
  (˜ U+02DC, ◌̃ U+0303) do not shift positions relative to Java's — see that module's docs.
- **Upstream:** not filed. All three are small, independent fixes: use `matcher.start()` for the
  parenthesis position (or `matcher.end() - 1`); add `realStartingPosition` and switch to `start(1)`
  in `handleQuantifier`; and compute each variable's own offset while splitting the list (the split
  currently discards offsets, so this one needs a `Matcher`-based split rather than
  `String.split`).
- **Severity:** low — diagnostics only. No automaton, language, or accept/reject decision depends on
  `positionInPredicate`; the damage is a misleading "char at N" in an error message, which is worst
  for defect 2 (a quantifier error inside a function argument or word index points somewhere
  unrelated). **Checked, so U27 does not have to guess:** 11 golden fixtures print a "char at"
  (`error190`, `error398`–`error403`, `error459`, `error476`, `error669`, `error670`), and **none of
  them encodes a WB-015-affected value** — the two `unbalanced parenthesis` ones (`def test669
  "((("`, `def test670 ")))"`) have no whitespace before their parentheses, so defect 1 is
  invisible there, and the other nine report function/arithmetic/relational tokens, which use the
  correct `realStartingPosition + start(1)` form. So Tier 1 neither pins nor contradicts this
  entry, and a future upstream fix would not look like a golden-corpus regression.

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
