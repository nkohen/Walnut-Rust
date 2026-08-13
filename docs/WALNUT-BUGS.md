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
  finding makes that gap concretely reachable rather than hypothetical. **Update (Phase 3b, L1):
  one further reach path, added by wiring the lsd branch of `wr_core::quantify::quantify` up to
  `fix_trailing_zeros_problem`.** That fixup closes with `justMinimize` (which never trims),
  unlike its msd sibling `fixLeadingZerosProblem`, which closes with
  `determinizeAndMinimize(IntSet)` and so re-establishes the reachability precondition via subset
  construction. It is unreachable on the ordinary path (`quantifyHelper`'s own
  determinize+minimize leaves every state reachable from `q0`) and only opens up when the helper
  short-circuits — empty label set, or a label-less automaton — on an input that already had an
  unreachable state. Java has the identical shape at the identical call site
  (`AutomatonQuantification.java:46`), so ported verbatim, not guarded; recorded here so the
  call-site inventory above stays complete.
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

- **Where:** `Main/EvalComputations/Token/ArithmeticOperator.java`, `processBinaryOperator`
  (`:201-224`) — four structurally identical `if (… isZero()/constant == 0 …) && opp.equals(Ops.MULT)`
  early returns, one per (left/right) × (number-literal/`@`-letter) combination. *(Located precisely
  in Phase 3a's U9; the original Phase-0 entry pointed at `Main/EvalComputations/Expressions`, which
  is where the operand types live, not where the short-circuit is.)* A **fifth** instance of the same
  rule lives in the word-automaton rewrite arm of the same method (`:183-185`, `o == 0 && opp.equals(
  Ops.MULT)`), where the per-output conjunct for a zero output is `c = 0` and likewise never mentions
  the other operand's identifier.
- **What:** `0*x`, `x*0`, and similar literal-zero multiplications short-circuit to the constant
  `0` without ever binding `x` into an automaton or checking it's a real, in-scope variable. A
  typo'd or nonexistent variable name silently passes validation as long as it's multiplied by a
  literal `0`.
- **Trigger:** any query containing `0 * <undeclared-or-misspelled-name>` (or the reverse order).
- **Found:** Phase 0, Item 4 second wave, 2026-08-08.
- **Rust port:** `ported verbatim (quirk)` as of Phase 3a's U9 — `Operator::process_binary_operator`
  (`crates/wr-logic/src/token.rs`) reproduces all four early returns (and the `o == 0 && MULT`
  word-rewrite arm) at Java's exact positions, including the fact that the synthetic identifier `c` is
  minted at `ArithmeticOperator.java:155` *before* the short-circuit throws it away, so the
  fresh-name counter still advances. Pinned by
  `wb003_zero_times_a_variable_short_circuits_and_wastes_a_fresh_name`, which covers all four
  operand shapes, with `a_nonzero_constant_times_a_variable_builds_a_real_automaton` as the
  contrasting non-short-circuited case.
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
- **Rust port:** `diverged (structurally inapplicable)` — resolved by Phase 3a's **U14**
  (`crates/wr-cli/src/session.rs`), the `Session` context-struct refactor this entry anticipated
  (`CLAUDE.md`'s sanctioned deviation from mechanical fidelity). `SessionPaths` has no setter and
  no `&mut self` method: `name`, `main_walnut_dir`, `session_walnut_dir` and `global_session` are
  all computed once in `SessionPaths::new` from that call's own arguments and are immutable
  afterwards, so "a second setup call" is "a second, independent value" and there is no surviving
  state for either defect to act on — `name` and `session_walnut_dir` are always derived together
  (defect 1), and `global_session` is a plain constructor argument (defect 2). Not merely unlikely:
  a mutator would have to be *added* back to reintroduce it, and the guarantee is structural (no
  `&mut self` method exists on `SessionPaths`) rather than test-enforced —
  `session.rs`'s `a_second_session_is_fully_independent_wb_005` pins the consequence (two
  independently built values never share or drift), but, since it only ever constructs, it would
  not by itself catch a newly added setter. Note this is
  a divergence in the port's *shape*, not a behavior fix — every single-setup invocation (i.e. all
  real Walnut usage) behaves identically.
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

## WB-016 — `WordAutomaton.reverseWithOutput` never updates `q0` after rebuilding the automaton, silently corrupting the result whenever the input's initial state isn't already numbered `0`

- **Where:** `Automata/WordAutomaton.java`, `reverseWithOutput` (`:109-192`), specifically the
  `wordA.fa.setFields(newStates.size(), newO, newD)` call at `:175`.
- **What:** the reversal construction (Theorem 4.3.3, Allouche & Shallit) rebuilds the automaton's
  states from scratch as a subset-of-functions construction: `newStates.get(0)` is ALWAYS the
  correct new initial state by construction (it's the BFS root, added before the loop at `:136`,
  and `newO`/`newD` are built in BFS-discovery order so index `0` in the rebuilt arrays always
  corresponds to it). But `FA.setFields` (`FA.java:547-551`) only assigns `Q`/`O`/the transition
  table — it does **not** touch `q0` — and `reverseWithOutput` never calls `setQ0(0)` (or anything
  else) afterward. So `wordA.fa.q0` keeps whatever value it had **before** reversal, which after a
  complete state renumbering is almost always a reference to the wrong state (or, if the old value
  happens to be `>=` the new state count, out of bounds). The very next call,
  `minimizeSelfWithOutput` → `minimizeWithOutput` → `uncombine` → each sub-automaton's
  `determinizeAndMinimize()`, starts subset construction from this stale `q0`, so the corruption
  propagates through the entire reversed result — this is the same failure shape as WB-001 (a
  wrong `q0` after a state rebuild silently producing a wrong language), independently introduced.
  The same missing-`setQ0` pattern also appears in the structurally identical BFS-rebuild-via-
  `setFields` at `AutomatonLogicalOps.java:645` (`convertLsdBaseToRoot`, Phase 3b/U18 scope, not
  re-verified here but flagged for whoever lands that unit).
- **Why it's usually invisible:** `FA.canonizeInternal` (`FA.java:148-191`) always renumbers so
  `q0` becomes `0`, and most word automata reaching `reverseWithOutput` arrive via a
  `determinizeAndMinimize` + canonicalize pipeline, so `q0 == 0` is the common case — masking the
  bug by coincidence (new state `0` IS the correct new initial state, so the stale, unset `q0`
  happens to already equal the right value). The bug is live whenever that coincidence doesn't
  hold, e.g. a hand-authored `.txt` word-automaton file (Walnut's documented, supported way to
  introduce a DFAO — the `Word Automata Library` directory) whose first listed state block is not
  numbered `0`; `AutomatonReader` sets `q0` to whatever state ID is listed first
  (`AutomatonReader.java:41-57`), with no requirement that it be `0`.
- **Trigger (minimal, empirically confirmed against the real `walnut-java` CLI, `target/
  Walnut-all.jar`, 2026-08-12):** a 2-state, alphabet-`{0,1}` (`msd_2`) word automaton, total and
  deterministic. Isomorphic pair, differing only in which state is listed (hence numbered) first:
  - `q0` numbered `1`: `1 10 / 0->1, 1->0` then `0 20 / 0->0, 1->1`. Running `reverse` on this
    produces a result whose (canonically renumbered-on-write) initial state has **output `20`**.
  - The literal same automaton with states renumbered so `q0` is `0`: `0 10 / 0->0, 1->1` then
    `1 20 / 0->1, 1->0`. Running `reverse` on this produces a result whose initial state has
    **output `10`**.
  - By Theorem 4.3.3, a DFAO's reversal evaluated at the empty string always equals the original
    evaluated at the empty string (both are "", its own reversal) — i.e. `O_new(q0_new)` must equal
    `O_old(q0_old)` regardless of state numbering. The original automaton's `q0` has output `10` in
    BOTH cases (same automaton, just relabeled) — so `20` is the wrong answer, and it appears
    *only* in the `q0 != 0` run. Confirmed reproducible, not a one-off: reran both cases via a
    `reverse newname oldname;` command file against the real jar with a fresh session each time.
- **Found:** Phase 3a, U6 (`crates/wr-core/src/word_automaton.rs`, the `WordAutomaton` port),
  2026-08-12, while working out `reverseWithOutput`'s subset-construction dependencies (it needed
  `FA.setFields`, not yet ported) — reading the method line-by-line to port it surfaced the missing
  `setQ0` call, then confirmed empirically per above before logging.
- **Rust port:** `ported verbatim (quirk)` — `word_automaton::reverse_with_output` calls
  `Fa::set_fields` (itself a faithful, no-`q0`-touching port of `FA.setFields`) and likewise never
  assigns `fa.q0` afterward. Flagged inline in both `Fa::set_fields`'s and
  `word_automaton::reverse_with_output`'s doc comments (citing this entry), and pinned by a
  dedicated test, `reverse_with_output_wb016_wrong_q0_on_non_zero_initial_state`, which reproduces
  the exact empirical shape above inside the Rust port and asserts it reproduces the SAME wrong
  answer Java gives (not the corrected one) — per the mechanical-port rule, this is a case where
  the test's job is to pin the bug, not catch a regression from it.
- **Upstream:** not filed. A one-line fix: `wordA.fa.setQ0(0);` immediately after the `setFields`
  call at `:175` (new state `0` is always the correct new initial state, by the BFS-root argument
  above — no further computation needed, just setting the field). The `AutomatonLogicalOps.
  convertLsdBaseToRoot` sibling instance would need the same fix, separately verified.
- **Severity:** **critical** — silent wrong answer (not a crash) in `reverse`'s core algorithm, for
  an input shape (a hand-authored word-automaton `.txt` file whose initial state isn't numbered
  `0`) that is plausible under Walnut's own documented usage, not an adversarial or degenerate
  input.
---

## WB-017 — `putWord`/`putFunction`'s empty-index/empty-argument errors omit `realStartingPosition`

- **Where:** `Main/Predicate.java`, `putWord`'s per-index emptiness check (`:326-328`) and
  `putFunction`'s per-argument emptiness check (`:479-483`). Both are inline `new
  WalnutException(...)` calls (no `WalnutException.` static helper), and both use the bare
  `matcher.start(1)` — the WORD's/FUNCTION's own name-group offset within *this* `Predicate`'s
  string — instead of `realStartingPosition + matcher.start(1)`, the form every other position in
  this file uses. Same defect CLASS as WB-015 (a raw, un-adjusted matcher offset reaching a
  user-visible "char at N"), found at two different call sites while porting a different unit (U4,
  not U3) — logged separately since it is a different pair of methods, not a re-occurrence of an
  already-fixed bug.
- **What:** for a TOP-LEVEL word/function occurrence (`realStartingPosition == 0`), the bug is
  invisible — the correct and the buggy position coincide. It becomes observable the moment the
  word/function occurrence is inside a NESTED `Predicate` (a function argument, a word index, a
  macro expansion) with a non-zero `realStartingPosition`: the reported position is the offset
  *within the nested predicate's own fragment*, not the true absolute offset into the user's query.
- **Trigger (empirically confirmed against the real `walnut-java`, via the same kind of probe
  WB-015 used — `new Predicate(line)`, printing the thrown message — run against
  `target/Walnut-all.jar`, JDK 17, 2026-08-12; `endsIn2Zeros` and `F` are real fixtures in the
  Global `Automata Library`/`Word Automata Library`):**
  - word side: `new Predicate("$endsIn2Zeros(F[])")` throws `"index 1 of the word F cannot be
    empty: char at 0"`. `F`'s true offset in the query is 14 (inside the function argument, which
    has `realStartingPosition = 14`); the correct message would say `char at 14`.
  - function side: `new Predicate("F[$endsIn2Zeros(a,)]")` throws `"argument 2 of the function
    endsIn2Zeros cannot be empty: char at 1"`. `endsIn2Zeros`'s true offset in the query is 3
    (inside the word index, `realStartingPosition = 2`); the correct message would say `char at 3`.
- **Found:** Phase 3a, U4 (`crates/wr-logic/src/predicate.rs`, `Predicate::put_word`/
  `Predicate::put_function`), 2026-08-12.
- **Rust port:** `ported verbatim (quirk)`. `LexError::EmptyWordIndex`/`LexError::EmptyFunctionArgument`
  both store `self.java_offset(name_span.start)` — UTF-16-converted but deliberately NOT run
  through `self.position` (which would add `real_starting_position`) — matching Java's omission
  exactly. Pinned by `wb017_empty_word_index_position_is_not_real_starting_position_adjusted` and
  `wb017_empty_function_argument_position_is_not_real_starting_position_adjusted` (both reproduce
  the two triggers above verbatim, including the expected *wrong* position), plus the two top-level
  (non-distinguishing, `realStartingPosition == 0`) cases straight from `PredicateTest.java`
  (`wordWithEmptyIndexThrows`, `functionCallWithEmptyArgumentAmongMultipleThrows`).
- **Upstream:** not filed. Fix is mechanical at both sites: `realStartingPosition +
  matcher.start(1)`, matching every sibling position in the same file.
- **Severity:** low — diagnostics only, same class of damage as WB-015. Requires authoring a
  malformed (empty) word index or function argument nested inside another construct to observe;
  none of the golden corpus's `error*` fixtures exercise a nested empty index/argument (checked by
  grepping the corpus's `reg`/`eval`/`def` command lines for `[]`/`(,`/`(,)`-shaped text inside a
  `$`/word-index context — none found), so Tier 1 neither pins nor contradicts this entry.

---

## WB-018 — a word occurrence with a trailing, never-closed chained bracket silently drops the rest of the query

- **Where:** `Main/Predicate.java`, `putWord` (`:296-333`). Once an index bracket closes and
  `indices` already holds enough entries to satisfy the word's declared arity, `putWord` still
  *tries* one more chained index (`m_leftBracket.find(i + 1)`, `:305`) — and if that next `[` is
  found but is never closed, the enclosing `while (i < predicate.length())` loop (`:299`) simply
  runs off the end of the string with `i == predicate.length()`, `indices` unchanged (the unclosed
  attempt is never finalized/added). `putWord` then returns `i + 1` — one PAST the end of the
  string — and the `Word` constructor's arity check passes, because `indices.size()` already
  matched before the doomed extra bracket was even opened. No exception fires anywhere.
- **What:** any syntactically-broken trailing text that starts with `[` immediately after a
  word occurrence whose own arity is already satisfied is silently discarded in its entirety, with
  no diagnostic — not a parse error, not a warning, nothing. This is a real "wrong output" bug in
  the CLAUDE.md sense: a clearly malformed query (an unclosed bracket, or worse, unrelated trailing
  predicate text that happens to start with `[`) is silently ACCEPTED as if the garbage were never
  there.
- **Trigger (empirically confirmed against `target/Walnut-all.jar`, JDK 17, 2026-08-12; `F` is a
  real msd_fib, arity-1 fixture in the Global Word Automata Library):**
  - `new Predicate("F[a][b")` succeeds, producing the SAME token stream as `new Predicate("F[a]")`
    — the trailing `[b` (a syntactically dangling, unclosed bracket) vanishes with no error.
  - `new Predicate("F[a][b=1")` succeeds identically — even though `[b=1` looks like it might be
    intended as a second, differently-shaped construct, it too is silently dropped.
- **Found:** Phase 3a, U4 (`crates/wr-logic/src/predicate.rs`, `Predicate::put_word`), 2026-08-12.
- **Rust port:** `ported verbatim (quirk)`. `put_word`'s bracket-scanning loop returns `i + 1` in
  BOTH the "closed cleanly, no more brackets" case and the "ran off the end without closing"
  case, exactly mirroring Java's single `return i + 1;` — no extra check was added to distinguish
  them, and none should be (the whole point is Java doesn't distinguish them either). Pinned by
  `wb018_trailing_unclosed_bracket_after_satisfied_arity_is_silently_dropped`, reproducing both
  triggers above and asserting the post-order is identical to the un-suffixed `F[a]` query (not an
  error).
- **Upstream:** not filed. A real fix needs a design decision (should a further unclosed bracket
  after a satisfied word occurrence be an error, and if so which one — unbalanced bracket? operator
  missing on whatever follows?), not just a one-line change, so this is flagged for a deliberate
  upstream decision rather than a mechanical patch.
- **Severity:** moderate — a real "wrong output" class defect (input a user would expect to be
  rejected is silently accepted, and a suffix of their query is silently ignored), but requires an
  already-malformed query (an unclosed bracket) to trigger, and the shipped golden corpus contains
  no unclosed-bracket fixtures (grepped: no `reg`/`eval`/`def` command line has an unmatched `[`
  after a word occurrence), so Tier 1 does not exercise it either way.

---

## WB-019 — `putMacro`'s `%N` argument substitution inherits `Matcher.appendReplacement`'s `$`/`\` escaping, crashing on a trailing backslash

- **Where:** `Main/Predicate.java`, `putMacro` (`:435-437`): `macro =
  new StringBuilder(macro.toString().replaceAll("%" + arg, arguments.get(arg)));`. Java's
  `String.replaceAll(regex, replacement)` compiles `regex` as a pattern (harmless here — `"%" + arg`
  is always plain digits, no metacharacters) but ALSO parses `replacement` through
  `java.util.regex.Matcher.appendReplacement`'s own mini-language, where `$` introduces a group
  reference and `\` escapes the next character. `replacement` here is `arguments.get(arg)` — the
  RAW, VERBATIM text of a macro call's argument, exactly as the user typed it, with no escaping
  applied before being handed to `replaceAll`.
- **What:** a macro-call argument containing a literal `\` is not passed through as literal text.
  Concretely: `\X` (any non-`\`/`$` character `X`) silently becomes just `X` (the backslash is
  consumed as an escape, matching neither Java's regex dialect NOR Walnut's own predicate grammar,
  which has no escape syntax at all); a LONE trailing `\` (nothing after it) makes the entire
  substitution throw an uncaught `IllegalArgumentException` — not a `WalnutException`, so it
  bypasses every one of Walnut's own error-formatting conventions and surfaces as a raw Java stack
  trace. The `$`-group-reference half of the same underlying issue (`$1`..`$9` would throw
  `IndexOutOfBoundsException`, since the pattern `"%" + arg` has zero capturing groups) turns out to
  be UNREACHABLE in practice: `parseParenthesizedArguments` (`:356-358`) already rejects any `$`
  (or `#`) appearing anywhere in a macro/function call's argument text, before `putMacro`'s
  substitution ever runs — confirmed empirically (see triggers below).
- **Trigger (empirically confirmed against `target/Walnut-all.jar`, JDK 17, 2026-08-12; using the
  real `my_macro0.txt` fixture, body `%0`):**
  - `new Predicate("#my_macro0(\\)")` (a single backslash as the sole argument) throws
    `java.lang.IllegalArgumentException: character to be escaped is missing` — uncaught, not a
    `WalnutException`.
  - `new Predicate("#my_macro0(\\x)")` succeeds, producing the predicate text `"x"` — the backslash
    silently vanished, and `\x` was NOT preserved as the two literal characters `\` and `x`.
  - `new Predicate("#my_macro0($5)")` and `new Predicate("#my_macro0($0)")` BOTH throw
    `"a function/macro cannot be called from inside another function/macro's argument list: char at
    11"` (`WalnutException.internalMacro`) — confirming the `$`-group-reference half of this same
    Java quirk is blocked upstream and never reaches `replaceAll` at all.
- **Found:** Phase 3a, U4 (`crates/wr-logic/src/predicate.rs`, `Predicate::put_macro`), 2026-08-12.
- **Rust port:** `ported verbatim (quirk)`. `java_replace_all_literal`/`expand_java_replacement`
  reproduce `Matcher.appendReplacement`'s replacement-string parsing (specialized to the
  zero-capturing-group case every real call here has), including the trailing-backslash failure and
  the backslash-escapes-the-next-character behavior. Ported as a recoverable
  `LexError::MacroArgumentReplacementError` (a `Result::Err`), not a Rust `panic!`: like
  `ExprError`'s WB-013 entry, this is a real Java UNCHECKED exception that `Prover`'s top-level
  `catch (RuntimeException)` recovers from (prints a stack trace, session continues) — a Rust
  `panic!` here would abort the whole process instead, with no `catch_unwind` boundary yet to
  mirror that recovery. Pinned by `wb019_macro_argument_trailing_backslash_reports_javas_exception_text`
  and `wb019_macro_argument_backslash_escapes_the_following_character`, reproducing both empirical
  triggers above; `macro_call_argument_containing_dollar_is_also_blocked` pins that the
  `$`-group-reference half stays unreachable through this port's own call graph too (same ordering:
  `parse_parenthesized_arguments`'s `#`/`$` check runs before substitution).
- **Upstream:** not filed. Fix is mechanical: escape `arguments.get(arg)` for `replaceAll`'s
  replacement-string dialect (`Matcher.quoteReplacement(...)`) before substituting, or switch to a
  literal (non-regex) replace entirely, e.g. `StringBuilder`-based splicing.
- **Severity:** low-moderate — an uncaught, unformatted crash on a plausible-if-unusual input (a
  macro argument containing a stray trailing backslash), plus a silent, undocumented
  character-dropping transformation on any argument containing `\` followed by another character.
  Walnut's predicate grammar has no legitimate use for a literal `\` in a macro argument, so this
  is unlikely to bite an ordinary user, but it is a real crash on input that is otherwise
  syntactically unremarkable. No golden fixture uses a backslash in a macro-call argument.

---

## WB-020 — `Function.act`'s string representation has a spurious extra closing parenthesis

- **Where:** `Main/EvalComputations/Token/Function.java`, `act` (`:72`): `stringValue +=
  UtilityMethods.genericListString(expressions, ",") + "))";` — TWO closing parens, where the
  surrounding `"("` (`:63`) and normal `name(args)` call syntax both call for exactly one.
  `Word.act`'s equivalent string-building (`Word.java:60`, `"[" + expression + "]"` per argument)
  has no such defect — this is specific to `Function`.
- **What:** every `AutomatonExpression` produced by evaluating a `$name(...)` call carries a
  DOUBLE closing paren in its printable form (`Expression.toString()`/`expressionInString`), e.g.
  `phi(a,b))` instead of `phi(a,b)`. This is silent under normal evaluation (nothing rejects it),
  but surfaces verbatim in any later error message that prints the operand — e.g.
  `WalnutException.invalidOperator`/`invalidDualOperators`, raised whenever a function-call result
  is combined with an operator it doesn't support.
- **Trigger (empirically confirmed live through the real `walnut-java` CLI —
  `java -cp target/Walnut-all.jar Main.Prover` with `--session-dir`, a temporary `Automata
  Library/endsIn2Zeros.txt`, and the command file `eval trigger_bug "$endsIn2Zeros(a)=1";` — JDK
  17, 2026-08-12):** the CLI prints `operator = cannot be applied to operands endsIn2Zeros(a)) and
  1 of types Main.EvalComputations.Expressions.AutomatonExpression and
  Main.EvalComputations.Expressions.NumberLiteralExpression respectively` — note `endsIn2Zeros(a))`,
  the double paren, in real, user-visible CLI output (relational comparison rejects an
  `AutomatonExpression` operand, exactly the kind `$`-calls produce).
- **Found:** Phase 3a, U4 (`crates/wr-logic/src/token.rs`, `Function::act`), 2026-08-12.
- **Rust port:** `ported verbatim (quirk)`. `Function::act` builds `string_value` as
  `format!("{}({}))", self.name, joined)` — the double `)` is written explicitly and flagged at the
  call site as WB-020, not a typo to fix silently. Pinned by
  `function_act_conjoins_and_quantifies_producing_an_automaton_expression`
  (`crates/wr-logic/src/token.rs`), which asserts the exact string `"phi(a,b))"`.
- **Upstream:** not filed. Trivial one-character fix (`")"` instead of `"))"`).
- **Severity:** low — cosmetic, diagnostics-only (no automaton/language decision reads this
  string), but genuinely reaches real user-visible CLI output whenever a function-call result hits
  a type-mismatched operator. No golden `error*` fixture happens to combine a `$`-call result with
  an incompatible operator (checked: none of the corpus's `error*` fixtures invoke a `$`-defined
  function at all), so Tier 1 neither pins nor contradicts this entry.

---

## WB-021 — `AutomatonWriter.exportToBA` has no `TRUE_FALSE_AUTOMATON` guard, unlike its two siblings; TRUE and FALSE export byte-identically

- **Where:** `Automata/Writer/AutomatonWriter.java`, `exportToBA` (`:161-173`).
- **What:** `writeToTxtFormat` (`:48-58`) and `writeToGV` (`:101-159`) both explicitly check
  `automaton.fa.isTRUE_FALSE_AUTOMATON()` first and handle the trivial automaton specially, because a trivial `FA`'s
  `Q`/alphabet/transition-table fields are meaningless/stale (see `wr_core::fa`'s and `crate::reader`'s own docs on
  this shape). `exportToBA` has no such guard — it unconditionally calls `a.FAtoCompactNFA()` on whatever `FA` it's
  given. Empirically confirmed (not just read) against the real `walnut-java` CLI (`target/Walnut-all.jar`, JDK 19,
  small driver classes calling `AutomatonWriter.exportToBA` directly on both `new Automaton(true).fa` and
  `new Automaton(false).fa`): it does not crash — `FAtoCompactNFA` builds a 0-state `CompactNFA` over an empty
  alphabet, `setInitial(0, true)` records an initial-state id that doesn't correspond to any real state, and
  `BAWriter` happily writes it — but the two calls produce **byte-identical** output, just `"0\n"`: one initial-state
  line (the phantom id `0`), zero transition lines (`FA.t` is empty), and zero final-state lines (`BAWriter`'s
  "if every state is accepting, write no final-state section at all" rule — see `crates/wr-io/src/writer.rs`'s module
  docs for the full derivation — is vacuously true when there are zero states to disagree). The TRUE and FALSE
  automata are indistinguishable in `.ba` output, and neither carries any information about which one it was.
- **Trigger:** `[export <name> BA]` (or any direct `exportToBA` call) on an automaton that is currently the trivial
  TRUE or FALSE automaton — plausible and common: the golden corpus shows 13% of `automaton*` fixtures are exactly
  this shape (`crates/wr-io/src/reader.rs`'s docs).
- **Found:** Phase 3a, U12 (`wr-io`'s `AutomatonWriter` port), 2026-08-12, while deriving the `.ba` format from real
  `walnut-java` output (the plan's `.ba`-format-fidelity retiering explicitly called for empirical verification, not
  guessing). Confirmed live: both the no-crash result and the byte-identical output were run, not inferred.
- **Rust port:** `ported verbatim (quirk)` — `wr_io::writer::export_to_ba` never special-cases
  `Fa::is_true_false_automaton`, exactly matching Java's omission; the trivial-`Fa` shape (`q0 = 0, q = 0`) naturally
  falls through the same general algorithm to the same `"0\n"` output. Pinned by
  `ba_matches_real_walnut_output_for_true_automaton_wb016` and `ba_matches_real_walnut_output_for_false_automaton_wb016`
  in `crates/wr-io/src/writer.rs`, the latter also asserting the two real fixture files are themselves byte-identical
  (so the test can't silently pass if a future re-verification run shows Java's behavior has changed).
- **Upstream:** not filed. A ~3-line fix would add the same `isTRUE_FALSE_AUTOMATON()` guard `writeToTxtFormat`/
  `writeToGV` already have — e.g. writing a single well-known sentinel state (accepting for TRUE, non-accepting for
  FALSE) instead of falling through to `FAtoCompactNFA` on stale/empty fields.
- **Severity:** moderate — silent, information-losing wrong output (not a crash) reachable from the plain CLI export
  command, on an automaton shape (trivial TRUE/FALSE) the golden corpus shows is common, not a contrived corner case;
  bounded by the fact that a `.ba` export of a trivial automaton is presumably a rare real workflow (most `export`
  usage is on a non-trivial query result), which is why this wasn't caught before.

---

## WB-022 — `read_automaton_txt_impl` (Rust) has no `isFAO`/`nonDeterministicO` guard, unlike the Java it ports; a genuine-NFAO `.txt` file is silently determinized to boolean instead of rejected

- **Note on this entry's shape:** unlike WB-001–WB-021, the Java side here is *correct* — this
  entry tracks a **Rust-port scope gap that diverges from correct Java behavior**, not a Java
  defect. Logged under this doc's "log every deliberate replicate-vs-diverge decision" charter
  anyway (per `CLAUDE.md`'s hard rule and this unit's own review), since the alternative — an
  agent quietly deciding the gap doesn't matter — is exactly what this doc exists to prevent.
- **Where:**
  - Java (correct): `Automata/AutomatonReader.java`, `readAutomaton` (`:88-98`) — after
    `A.fa.setFieldsFromFile(...)`, if `!A.fa.getT().isDeterministic()`, Java branches: if
    `!A.getFa().isFAO()` it determinizes; otherwise ("unexpected case — NFAO") it throws
    `WalnutException.nonDeterministicO()`. The `isFAO()` check runs **before** any determinizing
    attempt, since a DFAO's per-state output values cannot be soundly merged by subset
    construction.
  - Rust (gap): `crates/wr-io/src/reader.rs`, `read_automaton_txt_impl`'s auto-determinize step
    (`if !automaton.fa.is_deterministic() { ... determinize ... minimize }`) — no `is_fao()` check
    anywhere in this function. It unconditionally auto-determinizes ANY nondeterministic parsed
    automaton, DFAO or not.
- **What:** a hand-authored `.txt` file whose transition table is genuinely nondeterministic (some
  state has more than one destination for the same input symbol) AND carries real DFAO output
  (some state's declared output is `> 1`, not just plain 0/1 accept/reject — `wr_core::automaton::
  Automaton::is_fao`) is, in Java, a hard, correct rejection (`nonDeterministicO`). In this port,
  the same file is silently accepted, subset-constructed, and minimized — which collapses every
  state's output to plain boolean acceptance (`wr_core::determinize::subset_construction`'s own
  docs confirm this is exactly what determinizing does to output). The result is not an error, it
  is a silently DIFFERENT, wrong automaton: the real per-state DFAO outputs the file declared are
  gone, replaced by 0/1 acceptance.
  - Note the check is present, correctly, one layer up: `wr_core::automaton::AutomatonDFA::
    require_dfa_storage` DOES have an `is_fao()` guard that panics with Java's exact
    `nonDeterministicO` message. But by the time `AutomatonDFA::from` (and therefore
    `require_dfa_storage`) ever sees the automaton coming out of `read_automaton_dfa_txt`,
    `read_automaton_txt` has already forced it deterministic — so that guard's
    `!automaton.fa.is_deterministic()` branch, and the `is_fao()` check inside it, are **provably
    unreachable** through this call path. The real gap is one level down, in
    `read_automaton_txt_impl` itself, which has no equivalent check before it determinizes.
- **Trigger:** a hand-authored genuine-NFAO `.txt` file, fed through either `read_automaton_txt` or
  (more visibly, since the DFA-typed wrapper's whole contract is "guaranteed deterministic, real
  DFAO inputs should be rejected, not silently reinterpreted") `read_automaton_dfa_txt`. No shipped
  `walnut-java` golden-corpus fixture or custom-base file this port has encountered exercises this
  shape (checked while porting this unit) — so it is not live against the real corpus, but it is a
  plausible hand-written input.
- **Found:** Phase 3a, U13 review (adversarial-reviewer pass), 2026-08-12, while checking a prior
  draft's in-code claim that this was "a known, already-documented gap" — that claim was false (no
  `WALNUT-BUGS.md` entry existed for it before this one; confirmed by grepping the file for
  `NFAO`/`is_fao`/`nonDeterministicO` and finding nothing). Confirmed by reading
  `AutomatonReader.java:88-98` directly, not inferred.
- **Rust port:** `not yet reached` — the underlying gap is in `read_automaton_txt_impl` (pre-dates
  this unit; U13 only added a new, more visible caller in `read_automaton_dfa_txt`), and is
  currently unaddressed. Pinned, not silently left unnoticed, by
  `read_automaton_dfa_txt_on_a_genuine_nfao_file_silently_determinizes_instead_of_erroring_wb022`
  in `crates/wr-io/src/reader.rs`, which asserts the CURRENT (divergent) behavior explicitly, so
  a future fix (adding the `is_fao()` guard to `read_automaton_txt_impl`, before the
  auto-determinize step) is a visible, intentional behavior change — the test will fail and have
  to be updated — rather than something nobody notices moving.
- **Upstream:** not applicable — Java's behavior here is the one to replicate, not fix.
- **Severity:** moderate — silent, information-losing wrong output (not a crash) on a plausible
  hand-written input shape; bounded by the fact that no real corpus fixture hits it and that
  `AutomatonDFA`'s own `is_fao()` guard is one `read_automaton_txt_impl` fix away from covering
  this too (the check already exists correctly in one place, it's just not reached first).

---

## WB-023 — `RelationalOperator.act`'s word-vs-arithmetic arm labels its result with only the WORD, dropping the operator and the other operand

- **Where:** `Main/EvalComputations/Token/RelationalOperator.java`, `act(Stack<Expression>)` (`:134`):
  `S.push(new AutomatonExpression(word.toString(), M));` — the arm that handles a word automaton
  compared against an arithmetic/variable expression (`:99-134`).
- **What:** all **seven** other arms of the same method label their result `a + op + b`
  (`:93`, `:140`, `:146`, `:152`, `:159`, `:165`, `:171`) — the full comparison as written. This one
  arm alone uses `word.toString()`, i.e. just the word occurrence's own text, silently discarding
  the operator symbol and the entire right-hand operand. So `T[i]<x` evaluates to an
  `AutomatonExpression` whose `expressionInString` is the literal `T[i]`. Nothing in the automaton
  or the decision procedure reads that string, but every later `WalnutException` that prints the
  operand does — `invalidOperator`/`invalidDualOperators` (`WalnutException.java:76-82`) — so the
  wrong text reaches real, user-visible CLI output. Same defect *class* as WB-020 (a wrong
  `expressionInString` surfacing through an operand-printing error message), different site and
  different mechanism (dropped operands, not a stray character).
- **Trigger (empirically confirmed live through the real `walnut-java` CLI —
  `java -cp target/Walnut-all.jar Main.Prover`, home dir seeded with the stock
  `Word Automata Library/T.txt` (Thue–Morse); JDK 17, 2026-08-12):** the command file

  ```
  eval wb023a "(T[i]<x)+1=y";
  eval wb023b "(x<z)+1=y";
  ```

  prints, respectively,

  ```
  operator + cannot be applied to the operand T[i] of type Main.EvalComputations.Expressions.AutomatonExpression
  operator + cannot be applied to the operand x<z of type Main.EvalComputations.Expressions.AutomatonExpression
  ```

  — note the first says `T[i]` where it should say `T[i]<x`, while the second (an all-variable
  comparison, i.e. a *sibling* arm) correctly names the whole comparison `x<z`. The explicit
  parentheses are needed only to make the comparison the operand of `+`: `+` has priority 20 and
  `<` has 40, so `T[i] < x + 1` would otherwise parse as `T[i] < (x+1)`.
- **Found:** Phase 3a, U9 (`crates/wr-logic/src/token.rs`, `Operator::act_relational`), 2026-08-12,
  while porting `RelationalOperator.act`'s eight-arm operand dispatch — the inconsistency is visible
  by reading the eight `S.push` calls side by side.
- **Rust port:** `ported verbatim (quirk)`. `Operator::act_relational`'s word-vs-arithmetic arm
  pushes `AutomatonExpression::new(word_expr.to_string(), m)` with an inline `WB-023` comment, while
  every sibling arm builds `format!("{a}{op}{b}")`. Pinned by
  `wb023_word_vs_arithmetic_result_string_drops_the_operator_and_operand`
  (`crates/wr-logic/src/token.rs`), which asserts the wrong string `"T[i]"` for the affected arm AND
  the right string `"T[i]<@1"` for a sibling arm, so the test cannot pass if the two ever converge.
- **Upstream:** not filed. One-line fix: `new AutomatonExpression(a + op + b, M)`, matching the
  other seven arms. Note the correct text is `a + op + b` (the ORIGINAL operand order), not
  `word + op + arithmetic` — the arm handles both operand orders via its `reverse` flag, so
  reconstructing from `word`/`arithmetic` would print `x<T[i]` as `T[i]<x`.
- **Severity:** low — cosmetic, diagnostics-only (no automaton, language, or output-file content
  depends on this string), but genuinely reaches user-visible CLI error text, and is actively
  misleading there: it names an operand that is not the one that failed. No golden `error*` fixture
  combines a word-vs-arithmetic comparison result with a type-mismatched operator (checked), so
  Tier 1 neither pins nor contradicts this entry.
## WB-024 — `reg`'s digit encoding collides with dk.brics' reserved characters, so an out-of-alphabet digit's effect depends on where it appears

- **Where:** `Main/Commands/Reg.determineEncodedRegex` (`Reg.java:42-76`, specifically `:63`'s
  `BricsConverter.convertEncodingForBrics(r.encode(L))`), acting on `Automata/RichAlphabet.encode`
  (`RichAlphabet.java:109-115`) and `Automata/FA/BricsConverter.convertEncodingForBrics`
  (`BricsConverter.java:158-165`).
- **What:** `reg` rewrites every alphabet vector and every bare digit in the user's regex into a single
  character standing for that input vector's *encoding*, so that `dk.brics` — which only understands
  characters — can parse a multi-track regex at all. The two halves of that rewrite disagree about their
  contract:
  * `RichAlphabet.encode` computes `Σ encoder[i] * A.get(i).indexOf(l.get(i))`. `List.indexOf` returns
    **`-1`** for a digit that is not in that track's alphabet, and nothing checks for it — so an
    out-of-alphabet digit silently produces a **negative** encoding rather than an error.
  * `convertEncodingForBrics` then does `vectorEncoding += 128; (char) vectorEncoding`, with the explicit
    comment that `+128` is enough "to ensure that we have no conflicts" with dk.brics' reserved
    characters, "All of these reserved characters have UTF-16 values between 0 and 127". That reasoning is
    sound only for **non-negative** encodings; a negative one lands right back inside `0..127`, i.e. inside
    the reserved range the offset exists to escape, and is then read as regex *syntax*.

  The result is that whether an out-of-alphabet digit is harmless, silently wrong, or a hard crash depends
  entirely on which reserved character its encoding happens to hit and on where in the regex it sits:
  ```
  reg foo {0,1,2,3} {0,1} "[9,9][0,0]";   ->  Set from brics:1 states   (empty language, no diagnostic)
  reg foo {0,1,2,3} {0,1} "[0,0][9,9]";   ->  java.lang.IllegalArgumentException: integer expected at position 3
  ```
  Both regexes contain exactly the same two vectors; only their order differs. `[9,9]` encodes to
  `1*(-1) + 4*(-1) = -5`, and `-5 + 128 = 123 = '{'`. In the first regex the `{` is a leading literal and
  the language quietly comes out empty; in the second it *follows* an expression, where dk.brics'
  `parseRepeatExp` reads it as the start of a `{n,m}` repeat count and throws. The reported position
  (`3`) is an index into the internally-wrapped `"(" + regex + ")&[…]*"` string, so it does not even point
  at the offending vector in the user's own input.
  Two further faces of the same root cause, both confirmed by reading the code and reproduced by the port's
  own tests: an encoding of `-119` yields character `9` (TAB), which
  `determineEncodedRegex`'s closing `replaceAll("\\s", "")` then **deletes**, so the character disappears
  with no diagnostic at all; and `[10]`-shaped input is swallowed by `RE_FOR_AN_ALPHABET_VECTOR` as the
  one-element vector holding the integer `10` (not as a character class), which on a `{0,1}` alphabet takes
  the same `-1` path and silently becomes the empty language.
- **Trigger:** any `reg` command whose regex mentions a digit or vector component that is not in the
  declared alphabet for that track — a plain user typo (`reg r {0,1} "[2,0]*"`), and, more insidiously, any
  bracketed multi-digit run like `[10]` that a user reasonably reads as a character class.
- **Found:** Phase 3a, U8 (`wr-core`'s regex engine), 2026-08-12. Pre-identified during the Phase-3 planning
  research and re-confirmed live here against the real `walnut-java` CLI (`target/Walnut-all.jar`): both
  commands above were run and produced exactly the quoted outputs.
- **Rust port:** `ported verbatim (quirk)`. `wr_core::regex::encode_with_index_of` reproduces
  `List.indexOf`'s `-1` deliberately (it exists *because* `Automaton::encode` panics on an out-of-alphabet
  digit and therefore cannot be reused here), and `convert_encoding_for_brics` reproduces Java's truncating
  `(char)` cast rather than range-checking. Pinned by `wb_024_*` in `crates/wr-core/src/regex/tests.rs`
  (four tests: the encoding itself, the order-dependence end to end, the "every negative encoding lands in
  the reserved range" invariant plus the whitespace-deletion face, and the `[10]` face) and by
  `wb_024_alphabet_offset_collision_is_order_dependent` in
  `tests/differential/tests/reg_brics_regex.rs`, which asserts the same two commands against the same
  behavior the real jar produced.
- **Upstream:** not filed. The minimal fix is a guard in `Reg.determineEncodedRegex` (or in
  `RichAlphabet.encode`): reject a digit whose `indexOf` is `-1` with a real message naming the digit and
  the track, instead of letting a negative encoding reach `convertEncodingForBrics`. Widening the offset
  would not be sufficient on its own — the wrong-answer case (`[9,9][0,0]` quietly yielding the empty
  language) is a missing *validation*, not a character-range problem.
- **Severity:** moderate-to-high — this is a **silently wrong answer** on plausible user input in the most
  common branch (a mistyped digit produces the empty language, and an empty-language `reg` result then
  propagates into every `eval` that uses it), with the crash branch as a bonus. It is bounded only by
  needing an out-of-alphabet digit in the first place; the `[10]`-as-a-vector face makes that easier to hit
  than it looks, since nothing in the syntax warns that bracketed digit runs are vectors rather than
  character classes.

---

## WB-025 — `BricsConverter.convertEncodingForBrics`'s `+128` offset itself overflows `char` for large, validator-legal alphabets, wrapping a legitimate symbol back into dk.brics' reserved range

- **Where:** `Automata/FA/BricsConverter.convertEncodingForBrics` (`BricsConverter.java:158-165`)
  together with its own guard, `BricsConverter.validateBricsAlphabetSize`/`MAX_BRICS_CHARACTER`
  (`BricsConverter.java:151-156`), reached from `Reg.determineEncodedRegex` (`Reg.java:63`) and from
  `BricsConverter.setFromBricsAutomaton` (`BricsConverter.java:54-80`).
- **What:** This is the same underlying mechanism as WB-024 — the truncating `(char)(128 +
  vectorEncoding)` cast — but triggered by a different, narrower condition: a perfectly legitimate,
  in-alphabet symbol index rather than an out-of-alphabet digit. `validateBricsAlphabetSize` only
  rejects `alphabetSize > MAX_BRICS_CHARACTER` where `MAX_BRICS_CHARACTER == (1<<16)-1 == 65535`, so
  any `alphabetSize` up to and including `65535` passes validation, making every symbol index `x` in
  `0..alphabetSize` (i.e. up to `65534`) validator-legal. But `char` is a 16-bit unsigned type in
  Java, so `128 + x` for `x >= 65408` is `>= 65536` and wraps (via the narrowing cast, exactly as
  WB-024's entry describes) back into `0..127` — dk.brics' own reserved range, the very range the
  `+128` offset exists to escape. Concretely: `x = 65408` encodes to `(char) 65536 == 0` (the NUL
  character), and `x = 65534` (the largest symbol index a `65535`-alphabet ever assigns) encodes to
  `(char) 65662 == '~'` (`~`, dk.brics' complement operator). Every symbol index in
  `[65408, 65534]` collides with a reserved character the same way WB-024's out-of-alphabet digits
  do — the regex silently reads as different syntax than intended, or throws a Brics parse error
  unrelated to the user's actual mistake — except here there IS no mistake: the alphabet and the
  symbol index are both exactly what the validator was supposed to guarantee are safe.
- **Trigger:** any `reg` command (or other caller of `setFromBricsAutomaton`) declaring a track
  alphabet whose size is in `(65408, 65535]` — reachable, if implausible in ordinary hand-written
  queries, since nothing before `validateBricsAlphabetSize` rejects it, and generated/large-alphabet
  automata are exactly the kind of input this code is otherwise supposed to tolerate up to its
  documented `65535` limit.
- **Found:** Phase 3a, U8 adversarial review (`wr-core`'s regex engine), 2026-08-12. Same
  `(1<<16)-1` bound and same truncating cast confirmed live against the real `BricsConverter.java`
  source (`:151-165`); not a hypothetical reading, the arithmetic is unconditional once
  `alphabetSize` is in the affected range.
- **Rust port:** `ported verbatim (quirk)`. `wr_core::regex::convert_encoding_for_brics` reproduces
  Java's `(char)` narrowing cast exactly via `vector_encoding.wrapping_add(128) as u16`, and
  `validate_brics_alphabet_size` reproduces `MAX_BRICS_CHARACTER == (1<<16)-1` verbatim, so the same
  wraparound reproduces at the same boundary. Pinned by
  `wb_025_a_legitimate_large_alphabet_symbol_wraps_into_the_reserved_range` in
  `crates/wr-core/src/regex/tests.rs`, right next to the WB-024 tests.
- **Upstream:** not filed. The minimal fix is tightening `MAX_BRICS_CHARACTER` (or
  `validateBricsAlphabetSize`'s bound) to `65535 - 128 = 65407`, so every validator-accepted symbol
  index's `+128` offset stays inside `char`'s range — narrower than WB-024's fix (which is a missing
  *validation* of the encoding's sign), this is a missing validation of the encoding's *magnitude*.
- **Severity:** low-to-moderate — same silently-wrong-answer/spurious-parse-error shape as WB-024,
  but gated behind an alphabet size (`> 65408`) far outside any plausible hand-written Walnut query;
  realistic exposure is through generated/fuzzed or programmatically-constructed large alphabets, not
  everyday use.

---

## WB-026 — `Prover.parseArgs` validates the command file BEFORE `Session.setPathsAndNames` runs, so `--home-dir=` is ignored and a valid invocation crashes

- **Where:** `Main/Prover.java`, `parseArgs(String[])` — the `else if (filename == null)` arm's
  `UtilityMethods.validateFile(Session.getReadAddressForCommandFiles(filename))` (`:318`), which
  runs inside the argument loop, i.e. **before** `Session.setPathsAndNames(sessionDir, homeDir,
  globalSession)` at `:321`.
- **What:** `Session.mainWalnutDir` is still its static initializer's `""` (`Session.java:42`) when
  `:318` calls `getReadAddressForCommandFiles`, so the command file is resolved as
  `"Command Files/<name>"` — relative to the process's working directory — regardless of what
  `--home-dir=` said. `validateFile` throws an unchecked `IllegalArgumentException` when that path
  is not a file (`UtilityMethods.java:153-158`), and nothing catches it, so Walnut dies with a
  stack trace before it ever reads a command. The validation is also **redundant**: `run(filename)`
  re-runs the identical `validateFile(getReadAddressForCommandFiles(filename))` at `:326`, that
  time *after* setup, i.e. correctly. So `:318` can only ever produce a false negative (reject a
  file that is really there) or a false positive (accept a same-named file in the working
  directory that is not the one that will actually be run) — it never adds a check `:326` does not
  already make.
- **Trigger (empirically confirmed live through the real `walnut-java` CLI —
  `java -cp target/Walnut-all.jar Main.Prover`; JDK 17, 2026-08-12):** with a home directory
  `whome/` containing `Command Files/probe.txt`, running from `whome`'s *parent*:

  ```
  $ java -cp target/Walnut-all.jar Main.Prover --home-dir=whome probe.txt
  Exception in thread "main" java.lang.IllegalArgumentException: File does not exist or is not a
    valid file: Command Files/probe.txt
        at Main.UtilityMethods.validateFile(UtilityMethods.java:156)
        at Main.Prover.parseArgs(Prover.java:318)
        at Main.Prover.main(Prover.java:289)
  ```

  — note the reported path has no `whome/` prefix at all. The same command file runs fine when
  invoked from inside `whome` with no `--home-dir` (verified: it evaluates and prints `TRUE`), so
  the file is genuinely present and readable; only the premature resolution is at fault. Argument
  order does not help — the loop always reaches `:318` before `:321`.
- **Found:** Phase 3a, U14 (`crates/wr-cli/src/session.rs`, the `Session.java` port), 2026-08-12,
  while reading `parseArgs` to establish who is responsible for appending the trailing `/` to
  `--home-dir=`/`--session-dir=` values (`Prover.java:304-313`) that `Session`'s string
  concatenation assumes.
- **Rust port:** `ported verbatim (bug)` **as of Phase 3b's U21** (was `not yet reached` while the
  defect's owning file, `Prover.parseArgs`, was unported; U14 ports only `Session`'s path builders,
  and the builder involved, `SessionPaths::read_address_for_command_files`, is faithful).
  `crates/wr-cli/src/prover.rs`'s `parse_args` replicates the ordering exactly, including the odd
  part this entry predicted: it constructs a **throwaway** `SessionPaths::new(Some(""), Some(""),
  false)` — an explicitly empty home directory, matching Java's still-uninitialized
  `Session.mainWalnutDir` static — purely to run a validation whose result is then discarded by
  `run`'s correct re-validation. The explicit `Some("")` (rather than `None`) matters: `None` would
  apply `setPathsAndNames`' "working directory ends in `bin` → `../`" rule, which has *not* run at
  this point in Java. Pinned by `run_command_file_validation_ignores_home_dir_wb_026`, which
  asserts the reported path has no home-directory prefix. **The user's sign-off this entry asked
  for was not obtained**; U21 applied `CLAUDE.md`'s stated default (replicate) rather than deciding
  the divergence on its own authority. Flipping to the one-line fix later is a two-line change here
  plus that test.
- **Upstream:** not filed. Fix is a one-line deletion: drop `:318` entirely and let `run`'s `:326`
  do the validation (it already does, correctly). If an early "unknown file" diagnostic is wanted,
  it has to move below `:321`.
- **Severity:** moderate — a hard crash with a misleading path in the message, on a documented,
  plausible invocation (`--home-dir=` is one of only three flags Walnut accepts, and it exists
  precisely to run against a home tree that is not the working directory). No golden fixture
  exercises it (the corpus harness runs command files from the repo root, where the working
  directory *is* the home directory), which is why Phase 0's coverage work did not surface it.
## WB-027 — the `I` quantifier silently discards the body's FREE variables, answering one global TRUE/FALSE instead of a predicate over them

- **Where:** `Main/EvalComputations/Token/LogicalOperator.actQuantifier`'s `I` branch
  (`LogicalOperator.java:149-153`), specifically `M = new Automaton(!infReg.isEmpty())` at `:153`.
- **What:** `E` and `A` both *project away only the named variables* and leave a real automaton over
  the body's remaining (free) tracks. `I` does not: it runs `removeLeadingZeros` over the named
  variables, asks `Infinite.infinite` whether the **whole multi-track language** is infinite, and
  then replaces the automaton with a TRUE/FALSE automaton carrying no tracks at all. So when the
  body has a free variable, the verdict is computed over the free variable's values too — "are
  there infinitely many *(x, y)* pairs" rather than "for this *y*, are there infinitely many *x*" —
  and the free variable then vanishes from the result rather than the answer being a predicate over
  it. There is no guard, no warning, and no error: the wrong answer is indistinguishable from a
  right one.
  - Concretely, `Ix x < y` over `msd_2` evaluates to `TRUE`. For every fixed `y` there are only
    finitely many `x < y` (at most `y` of them), so the correct answer is `FALSE` for every `y`;
    Walnut answers `TRUE` because the *set of pairs* `{(x, y) : x < y}` is infinite. `Ix x = y`
    likewise answers `TRUE` where the correct answer is `FALSE` for every `y` (exactly one witness
    each).
  - Even under the most charitable reading — that `I` is only *meant* for closed formulas, so a free
    variable is user error — the defect stands: the correct behavior for unsupported input is an
    error, not a confidently-printed `TRUE`. Note also that `E`/`A` accept free variables perfectly
    well, so nothing in the surface syntax signals that `I` is different.
- **Trigger:** any `I`-quantified formula whose body mentions a variable the `I` does not quantify,
  e.g. `eval t "?msd_2 Ix x < y";`.
- **Found:** Phase 3a, U10 (`LogicalOperator.act`), 2026-08-12. Confirmed live against the real
  `walnut-java` CLI (`Walnut-all.jar`): both `Ix x < y` and `Ix x = y` print `TRUE`. The
  free-variable-less cases in the same probe are all correct (`Ix x < 5` → `FALSE`, `Ix x >= 5` →
  `TRUE`, `Ix x = 3` → `FALSE`, `Ix,y x = y` → `TRUE`, `Ix,y (x = y & x < 5)` → `FALSE`), so this is
  specifically about free variables, not about `I` generally.
- **Rust port:** `ported verbatim (bug)`. `Operator::act_quantifier`'s `Infinite` arm in
  `crates/wr-logic/src/token.rs` reproduces `:150-153` exactly, including replacing the automaton
  with `Automaton::true_false(...)`. Pinned by
  `wb_027_infinite_quantifier_silently_drops_a_free_variable` in the same file, which asserts the
  wrong-but-faithful `TRUE` and states the correct answer in its own comment.
- **Upstream:** not filed. Two candidate fixes, neither obviously the intended one, which is exactly
  why this is logged rather than resolved here: (a) reject an `I` whose body has free tracks left
  after the quantified ones are removed, or (b) give `I` a genuinely per-free-variable semantics
  (for each valuation of the free tracks, is the fibre infinite?) — a real construction, not a
  one-line change.
- **Severity:** moderate-to-high where it applies — a silently wrong TRUE/FALSE from the decision
  procedure itself, which is the worst failure shape for a theorem prover — but narrow: `I` is a
  niche quantifier and the overwhelmingly common use is on a closed formula, where the answer is
  correct.

---

## WB-030 — two shipped help files are Windows-1252 but `HelpMessages` reads them as UTF-8, so `help reg;` and `help image;` print `�` where upstream wrote `…`, `’` and `–`

- **Where:** `Main/HelpMessages.java`, `printHelpFile` (`:217`) — `new BufferedReader(new
  FileReader(file))`, the charset-less `FileReader` constructor, i.e. `Charset.defaultCharset()`.
  The data is `Help Documentation/Commands/Automata/reg.txt` and
  `Help Documentation/Commands/Morphisms And Word Automata/image.txt`.
- **What:** those two files are the only non-ASCII ones in the 34-file help tree, and they are
  encoded in **Windows-1252**, not UTF-8: `reg.txt` has `0x85` (an intended `…`) on line 3 and two
  `0x92`s (intended `’`) on line 13; `image.txt` has `0x96` (an intended en dash `–`, in
  "Math. Systems Theory 6, 164–192") on line 7. None of the four is valid UTF-8, so on any JVM
  whose default charset is UTF-8 — which is every JVM since JEP 400 (Java 18+), and the
  oracle's JDK 17 on macOS as well — `InputStreamReader`'s `CodingErrorAction.REPLACE` turns each
  into `U+FFFD`. The user sees mojibake in the middle of the `reg` command's own syntax line. The
  output is also **platform-dependent** on JDK ≤17: the same build on a Windows JVM whose default
  charset *is* cp1252 prints the intended punctuation.
- **Trigger (minimal, verified):** `help reg;` or `help image;` at the Walnut prompt. Verified two
  ways against the real `walnut-java`: (a) a standalone `BufferedReader`/`FileReader` probe over
  both files on JDK 17.0.20 reported `defaultCharset=UTF-8` and exactly four `U+FFFD`s at
  (line 3, idx 37), (line 13, idx 411), (line 13, idx 614) and (line 7, idx 120); (b) the real
  `Main.Prover` CLI, whose `help reg;` / `help image;` transcript decodes as UTF-8 with `U+FFFD`
  at precisely those positions.
- **Found:** Phase 3b, U22 review, 2026-08-13, while porting `HelpMessages.java`.
- **Rust port:** `ported verbatim (quirk)` — `crates/wr-cli/data/help/` stores `U+FFFD` at those
  four positions, so `wr_cli::help_messages::help_command` reproduces real Walnut's *observed*
  output byte for byte rather than the punctuation upstream evidently meant. Deliberately **not**
  decoded as cp1252: that would be a silent divergence from the oracle, and this catalog's whole
  point is that the fix is a scheduled decision, not an agent's side effect. Pinned by
  `help_reg_output_is_exact_including_the_wb_030_replacement_chars` and
  `help_image_output_is_exact_including_the_wb_030_replacement_char`, both exact-string goldens
  cross-checked against the real CLI's transcript.
- **Upstream:** `not filed`. The clean fix is upstream and one-line-ish either way: re-save both
  files as UTF-8 with the intended `…`/`’`/`–` (best), or pin the reader with
  `new InputStreamReader(new FileInputStream(file), StandardCharsets.UTF_8)` *after* re-saving.
  Note the first alone is sufficient and also fixes the JDK-≤17 platform dependence.
- **Severity:** low — cosmetic, confined to help text, no effect on any decision-procedure result.
  Worth logging because it is a *data*-side defect the port would otherwise have silently
  "corrected", and because it is the reason two of this repo's data files are not byte-identical
  to their upstream originals.

---

## WB-031 — `parseHelpArguments` splits on whitespace with no quoting, so the `Morphisms And Word Automata` group can never be named: its group-level and per-group help are unreachable

- **Where:** `Main/HelpMessages.java`, `parseHelpArguments` (`:77-97`, the terminal
  `s.split("\\s+")`) against `helpCommand`'s token-count dispatch (`:30-62`). The offending datum
  is the directory name `Help Documentation/Commands/Morphisms And Word Automata/`.
- **What:** `helpCommand` routes purely on `tokens.length`: 1 → group listing or command search,
  2 → `help <group> <command>`, **≥3 → `Too many arguments.`**. The tokenizer has no quoting or
  escaping of any kind, so a group name containing spaces cannot be delivered as one token. One
  of Walnut's four help groups is named `Morphisms And Word Automata` — four whitespace tokens by
  itself. Consequently `help Morphisms And Word Automata;` (4 tokens) and
  `help Morphisms And Word Automata promote;` (5 tokens) both hit `Too many arguments`, and there
  is **no** input that reaches either the group-listing or the two-token mode for that group. The
  eight commands in it (`alphabet`, `image`, `join`, `minimize`, `morphism`, `promote`, `rsplit`,
  `split`) are reachable only through the one-token whole-tree search (`help promote;`), and only
  because no command name is duplicated across groups — `showCommandHelpAcrossAllGroups` takes the
  first `listFiles` hit in unspecified filesystem order and would otherwise be nondeterministic.
- **Trigger (minimal, verified):** `help Morphisms And Word Automata promote;` against the real
  `walnut-java` CLI prints `Too many arguments. Usage: help [group] [command];`, not `promote`'s
  help. Same for the 4-token `help Morphisms And Word Automata;`.
- **Found:** Phase 3b, U22 review, 2026-08-13, alongside WB-030. Logged separately because the
  root cause is code (the tokenizer's contract), not the help data, and because fixing WB-030 does
  nothing for this.
- **Rust port:** `ported verbatim (quirk)` — `wr_cli::help_messages::help_command` takes the raw
  command line and runs the same tokenizer, so it answers `Too many arguments` identically. Pinned
  by `test_help_promote_from_morphisms_group`, which asserts the error for both token counts and
  additionally exercises the (correct but unreachable) two-token dispatch through the internal
  helper so the dead path stays covered and visibly labelled as dead.
- **Upstream:** `not filed`. Cheapest real fix is to greedily match the longest known group name
  before splitting the remainder, or to accept a quoted group name; renaming the directory to a
  single token (e.g. `Morphisms`) also works but changes documented output.
- **Severity:** low — a discoverability defect, not a wrong answer; the affected help text is
  still reachable via `help <command>;`.

---

## WB-028 — the `earlyExistTermination` metacommand is accepted, documented in-code, and does nothing: its only reader was deleted

- **Where:** `Main/Prover.java:254` (`public static boolean earlyExistTermination = false;
  // earlyExistTermination metacommand`) and `Main/MetaCommands.java:115-117`, the switch arm that
  sets it. `MetaCommands`' constructor resets it (`:24`).
- **What:** the flag is **written twice and read nowhere**. Grepping the whole of `src/` for
  `earlyExistTermination` returns exactly four sites: the constant `EARLY_EXIST_TERMINATION`
  (`Prover.java:234`), the field declaration (`:254`), the constructor reset
  (`MetaCommands.java:24`), and the metacommand arm that sets it to `true` (`:116`) — plus the
  arity check at `:98` that lets a one-token metacommand block through *only* for this name. So
  `[earlyExistTermination] eval …::` parses successfully, takes a dedicated code path, and has
  precisely zero effect on the evaluation. There is no warning, no log line, and no error: the
  user gets the ordinary answer and no indication the metacommand was ignored.
  - This is not a feature that was never wired up. `git log -S earlyExistTermination` shows the
    reader existed and was removed: commit `e013dd7` ("Test of earlyExistTermination.",
    2025-04-08) added both the flag and its consumers in `Main/EvalComputer` /
    `EvalComputations/Token/LogicalOperator` — `Prover.earlyExistTermination &&
    postOrder.getLast().toString().equals(Operator.EXISTS)`, a short-circuit for a trailing `E`
    quantifier — and commit `b1aa9ab` ("Remove all remaining prefixes.", 2026-05-23) deleted those
    consumer lines while leaving the flag, the constant, the parser arm and the special-case arity
    check in place. The metacommand is the orphaned half of a reverted experiment.
- **Trigger:** `[earlyExistTermination] eval x "?msd_2 Ey y > x"::` — accepted, no effect.
- **Found:** Phase 3b, U21 (`Main/MetaCommands.java` port), 2026-08-13, by tracing the field to its
  (nonexistent) readers while deciding how to port it.
- **Rust port:** `ported verbatim (quirk)`. `crates/wr-cli/src/meta_commands.rs`'s `MetaCommands`
  keeps the field, parses the token, and exposes it through
  `MetaCommands::early_exist_termination()` — stored, observable, and consulted by nothing, exactly
  as in Java. Dropping it would have made a currently-accepted command string an error, which is a
  behavior change; that is why it is inert rather than deleted. Pinned by
  `wb_028_early_exist_termination_parses_and_sets_an_inert_flag`.
- **Upstream:** not filed. Two clean fixes, and the choice is a product decision, not a mechanical
  one: (a) delete the flag, the constant, the switch arm and the `:98` arity special-case, so the
  metacommand is rejected like any other unknown one; or (b) restore the consumer `b1aa9ab`
  removed. Note that (b) needs care — the deleted short-circuit was itself only ever described as a
  "test".
- **Severity:** low. No wrong answers; the cost is a silently-ignored user request and four pieces
  of misleading dead code (including a `// earlyExistTermination metacommand` comment that reads as
  if the flag were live).

---

## WB-029 — `Strategy.fromString` strips dashes from its input but not from its alias table, so a strategy's own printed name never parses

- **Where:** `Automata/FA/DeterminizationStrategies.java`, `Strategy.fromString` (`:52-63`),
  interacting with the enum constructor's `this.aliases.add(name)` (`:48`).
- **What:** `fromString` normalizes its argument with `name.replace("_","-").replace("-","")` —
  i.e. **delete every underscore and dash** — and then compares the normalized string, case
  -insensitively, against each strategy's alias list. But the alias list is built from the declared
  aliases *plus the strategy's own `name`*, and two of those names contain dashes
  (`"Brzozowski-CCLS"`, `"Brzozowski-CCL"`). Since the alias side is never normalized, those two
  entries are unreachable: the input `Brzozowski-CCL` normalizes to `BrzozowskiCCL`, which equals
  neither `BRZCCL` nor `Brzozowski-CCL`. So `[strategy 1 Brzozowski-CCL]` throws
  `IllegalArgumentException: No strategy found for: Brzozowski-CCL` even though
  `Brzozowski-CCL` is exactly the string Walnut itself prints for that strategy (via
  `Strategy.outputName`, `:69-71`, in every `Determinizing …` details line).
- **Trigger:** `[strategy 1 Brzozowski-CCL] eval x "?msd_2 x = 1"::`, or the same with
  `Brzozowski-CCLS`. The undashed aliases (`BRZCCL`, `BRZ_CCL`, `BRZ-CCL`, `brzccl`, …) all work.
- **Found:** Phase 3b, U21 (`Main/MetaCommands.java` port), 2026-08-13, while porting the alias
  table.
- **Rust port:** `ported verbatim (quirk)`. `crates/wr-cli/src/meta_commands.rs`'s
  `strategy_from_string` reproduces the normalize-input-only comparison and the same alias table,
  so the two dashed names likewise fail to resolve; pinned by
  `wb_029_dashed_strategy_names_are_unreachable_aliases`. Note the practical impact **in this
  port is nil**: both affected names are OTF strategies, which `docs/DESIGN.md` §9/§10 defer and
  which `strategy_from_string` rejects with `MetaCommandError::OtfStrategyDeferred` on the
  reachable spellings anyway. The two in-scope strategies (`SC`, `Brzozowski`) have no dash in
  their names and round-trip correctly — pinned by
  `the_two_in_scope_strategy_names_round_trip_including_aliases`.
- **Upstream:** not filed. One-line fix: normalize the alias the same way as the input inside the
  comparison loop (`alias.replace("_","-").replace("-","")`), which also makes the redundant
  `List.of("BRZCCLS")`-style aliases unnecessary.
- **Severity:** low, and low *here* — it only bites the OTF family, which this project does not
  implement. Logged because the shape of the defect ("the printed name is not a parseable name")
  is exactly the kind that survives a port unnoticed, and because it would become user-visible the
  day the OTF deferral is revisited.

---

## WB-035 — `Transducer.transduceNonDeterministic`'s dead-state marker `minOutput` is used un-encoded as a transducer INPUT symbol *and* as a marker in the RESULT's output alphabet, silently deleting real states (and crashing outright on a shifted alphabet)

- **Where:** `Automata/Transducer.java`, `transduceNonDeterministic`'s partial-automaton branch
  (`:303-323`) — specifically `Tnew.fa.getT().setNfaDTransition(q, minOutput, newList)` (`:315`),
  `Tnew.sigma.get(q).put(minOutput, minOutput)` (`:316`), and
  `AutomatonLogicalOps.removeStatesWithOutputRebuild(N.fa, minOutput)` (`:321`).
- **What:** when the input automaton `M` has undefined transitions, Walnut totalizes it with a
  distinguished dead state (`FA.addDistinguishedDeadState`, output `min(M.O) - 1`), extends the
  transducer so that reading that dead letter loops in place and emits `minOutput`, transduces, and
  finally deletes every result state whose output is `minOutput`. `minOutput` is a value from **`M`'s
  output alphabet**. It is then used in two places where it is *not* a value in that space, with no
  conversion at either:
  1. **As an encoded transducer INPUT symbol.** Every other site in the file goes through
     `richAlphabet.encode(List.of(v))` (`:188`, `:277`, `:397`), i.e. `A[0].indexOf(v)` — a *position*
     in the transducer's input alphabet, not the value itself. `:315-316` skip that indirection
     entirely and key the transducer's transition table and `sigma` on the raw `minOutput`.
  2. **As a marker in the RESULT's OUTPUT alphabet.** `N`'s outputs come from `sigma`, i.e. the
     *transducer's* output alphabet, which has nothing to do with `M`'s. If the transducer can
     legitimately emit `minOutput` from a real state, `removeStatesWithOutputRebuild` deletes those
     real states along with the intended dead ones.

  (1) happens to be harmless in the single common case — `A[0] = [0, 1, …, k-1]` makes `indexOf(v) ==
  v`, and `min(M.O) == 0` makes `minOutput == -1`, which `List.indexOf` also returns for an absent
  value — so the two spaces coincide by coincidence. Shift either and it breaks.
- **Trigger (both manifestations empirically confirmed against the real `walnut-java` CLI,
  `target/Walnut-all.jar`, 2026-08-13):**
  - **(2) silent wrong answer.** A partial `msd_2` word automaton with outputs `{0, 1}` (so
    `minOutput == -1`), and a one-state transducer over `{0, 1}` that emits `-1` on letter `0`:
    - `Word Automata Library/PARTIAL.txt`: `0 0 / 0 -> 1` then `1 1 / 0 -> 1, 1 -> 0` (state `0` has
      no transition on symbol `1`, and only that one).
    - `Transducer Library/NEG.txt`: `{0, 1}` then state `0` with `0 -> 0 / -1`, `1 -> 0 / 1`.
    - `transduce OUTP NEG PARTIAL;` produces `0 -1 / 0 -> 1` and `1 1 / 0 -> 1` — **two**
      transitions. The input's `1 -> 0` transition (perfectly well-defined in `PARTIAL`) is gone.
    - Control, identical in every respect except the one colliding output value (`-1` becomes `5`):
      `Transducer Library/POS.txt` with `0 -> 0 / 5`, `1 -> 0 / 1`. `transduce CTRL POS PARTIAL;`
      produces `0 5 / 0 -> 1` and `1 1 / 0 -> 1, 1 -> 0` — **three** transitions, i.e. the correct
      answer, keeping `1 -> 0`. Second control: running `NEG` against a *totalized* version of the
      same automaton (add `1 -> 1` to state `0`) also keeps `1 -> 0`, confirming it is the
      dead-state path plus the collision, not the `-1` output or the partiality alone.
  - **(1) crash.** `M` with outputs `{1, 2}` and a transducer whose input alphabet is `{1, 2}` — the
    compatibility check at `:276-281` passes, since both of `M`'s output values are in the alphabet.
    `minOutput` is then `0`: `:315` writes encoded symbol `0` (which *means the letter `1`*,
    clobbering a real transition of the transducer for every state), while `createMap` looks the dead
    state up as `encode([0]) == -1`, which was never written. Real Walnut throws
    `java.lang.NullPointerException: Cannot invoke "…IntList.getInt(int)" because the return value of
    "Automata.FA.Transitions.getNfaStateDests(int, int)" is null at Automata.Transducer.createMap
    (Transducer.java:400)`. Files used: `PARTIAL12.txt` = `0 1 / 0 -> 1` then `1 2 / 0 -> 1, 1 -> 0`;
    `SHIFT.txt` = `{1, 2}` then state `0` with `1 -> 0 / 7`, `2 -> 0 / 8`.
- **Why it's usually invisible:** the shipped `Transducer Library` transducers (`RUNSUM2`/`RUNSUM3`/
  `RUNSUM4`) all declare `{0, 1, …}` alphabets and emit only non-negative outputs, and the word
  automata they are used on (Thue-Morse and friends) are total, so the dead-state branch is never
  taken at all. Both halves need a partial input automaton *plus* either a non-`0`-based input
  alphabet or a transducer output equal to `min(M.O) - 1`.
- **Found:** Phase 3b, U20 (`crates/wr-core/src/transducer.rs`, the `Transducer` port), 2026-08-13,
  while working out why the port could not use `Automaton::encode` (which panics on an
  out-of-alphabet digit) for `:188`/`:277`/`:397` — tracing that dependency on Java's silent
  `indexOf` → `-1` surfaced the un-encoded `minOutput` two lines away. Both manifestations were then
  reproduced against the real CLI before logging.
- **Rust port:** `ported verbatim (bug)`. `Transducer::transduce_non_deterministic` writes
  `min_output` straight into `t_new.automaton.fa.d[q]`/`t_new.sigma[q]` and passes it straight to
  `logicalops::remove_states_with_output_rebuild`, with both halves flagged inline citing this entry;
  `Transducer::encode_input` is a deliberate local port of `RichAlphabet.encode`'s `indexOf` → `-1`
  fallback (see this module's docs) rather than a call to `Automaton::encode`, precisely so half (1)
  reproduces rather than being masked by a different panic. Pinned by three tests in that module:
  `wb035_partial_automaton_loses_states_whose_output_collides_with_the_marker` (asserts the WRONG
  two-transition result), `partial_automaton_transduces_through_the_dead_state_path` (the
  one-value-different control, asserting the correct three-transition result), and
  `wb035_shifted_alphabet_panics_where_java_npes` (a `#[should_panic]` at the same point Java NPEs).
- **Upstream:** not filed. The fix is not one line, which is part of why this is logged rather than
  resolved here. Half (1) is mechanical — encode before use (`richAlphabet.encode(List.of(minOutput))`
  at `:315-316`, matching `:397`) — but that alone is not enough, because the encoded symbol may
  still collide with a real letter; the dead letter really wants to be a *fresh* symbol appended to
  the transducer's input alphabet. Half (2) needs a marker outside the transducer's output alphabet
  (e.g. `min(sigma values) - 1`, or a parallel "is dead" flag rather than an output sentinel) instead
  of reusing `M`'s.
- **Severity:** **high** where it applies — half (2) is a silent wrong answer in `transduce`'s core
  construction, half (1) is an uncaught crash, and both are reachable from ordinary hand-authored
  library files with no adversarial shape. Narrow in practice: only partial (non-total) input
  automata reach the branch at all.

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
