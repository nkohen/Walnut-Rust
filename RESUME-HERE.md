# RESUME-HERE — Phase 3a: all 22 units merged, exit checkpoint in flight

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## ALL 22 Phase 3a units done and merged — master is green

- **U0** — `TRUE_FALSE_AUTOMATON`/`TRUE_AUTOMATON` retrofit (`4758462`)
- **U0a** — `Logging.java` port (`5d9f714`)
- **U0b** — `ParseMethods.java`/`UtilityMethods.java` port (`1b02587`)
- **U0c** — `MetaCommands` strategy/export hook in `wr-core::determinize` (`3297581`)
- **U1** — `PredicateEnv` design (`1cfd760`)
- **U2** — `Token`/`Operator`/`Expressions` + symbol tables (`1524f9a`)
- **U11b** — `TestCase.java`, gave `wr-cli` its first `[lib]` target (`90541a5`)
- **U5** — custom-base `NumberSystem` loading + `applyAllRepresentations` (`7e9f3b1`)
- **U3** — Predicate lexer (`82a2b7a`)
- **U7** — `Infinite.java` (`df7cc8f`)
- **U6** — `WordAutomaton.java` (`2bd25a7`)
- **U4** — Word/Function/macro token construction (`83410c0`)
- **U12** — `AutomatonWriter` `.txt`/`.gv`/`.ba` (`2e0befb`)
- **U13** — custom-base reader headers + `readTransducer`/`readComments`/`AutomatonDFA(String)` (`63e0927`)
- **U9** — `RelationalOperator`/`ArithmeticOperator` `act()` semantics (`ad69512`)
- **U8** — regex engine, Brics-dialect parser + Thompson construction (`2fb3f76`)
- **U14** — `Session.java` as an explicit context struct, first real file-backed `PredicateEnv` impl (`3e1df51`)
- **U10** — `LogicalOperator`'s connective dispatch + the quantifier-elimination driving logic
  (`19c2c28`) — the decision-procedure crux of the whole port
- **U16** — `Reg.java` + the full `alphabet` command body (`991e8f4`)
- **U11** — the shared postfix executor + final `Predicate` assembly, the Phase 3a integration
  checkpoint (`f2aa6ce`)
- **U15** — `EvalDef.java`, the actual `eval`/`def` command (`fa9488e`, current `master` HEAD).
  Reviewer #2 found and a fixer resolved a real correctness-risk: the CAS-matrix-export drop
  (a plan-approved DROP-scope decision) had been implemented as a silent no-op, but real Walnut
  actually ABORTS `eval`/`def` (throws, after already writing the automaton output and printing
  the TRUE/FALSE verdict) when a TRUE/FALSE result is asked for free variables, or when a named
  free variable doesn't exist — the port was silently reporting success for the same input. Fixed
  by porting the two cheap input-validation checks (not the CAS-file-writing itself), preserving
  the sign-off's actual intent while closing the wrong-success divergence.

`cargo test --workspace` was green on `master` as of `fa9488e` (wr-cli 71, wr-core 444, wr-io 99,
wr-logic 270, wr-cts 22, differential 8, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **27**
genuine Walnut (Java) bugs found and logged across Phase 3 so far (WB-001 through WB-027 — see
`docs/WALNUT-BUGS.md`). U11/U15/U16 found no NEW genuine Java bugs, only pre-existing/deferred
gaps or port-side defects, all correctly handled per CLAUDE.md's protocol.

**Process notes for whoever resumes:**
- **WB-number collisions are routine when a unit adds new entries; always grep `^## WB-` for
  duplicate/gapped numbers after ANY rebase touching `docs/WALNUT-BUGS.md`, conflict-reported or
  not** — this has now been the single most recurring process wrinkle across the whole phase.
- **"No reviewer flagged correctness-fatal" is not the same as "nothing to fix."** Nearly every
  unit in the back half of this phase (U8, U14, U16, U11, U15) had at least one reviewer surface a
  real, fixable issue despite the other reviewer (or even both, on first pass) finding nothing —
  always read BOTH full reports, not just the summary line, before deciding a unit is clean.
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time this phase.
- **A coordinator process-hygiene lesson from mid-phase**: after any sequence of commands that
  `cd`s into a worktree for inspection, explicitly `cd` back to the intended target directory
  before the next stateful command (`git add`/`commit`/`merge`) — don't rely on remembering the
  last `cd` several tool calls back.

## In flight right now — the Phase 3a EXIT CHECKPOINT itself

Per the plan: extend `tests/differential` with `eval`/`def`/`reg` cases (literal strings, called
via **`wr-cli`'s real library API** — not the individual units' own lower-layer tests, and not yet
the full `Prover` dispatch loop — using U0c's no-op strategy/export context), compared against
real `walnut-java` CLI output through `wr_core::equiv`. This is the first time the ENTIRE Phase 3a
stack (parser → quantifier-elimination → operand semantics → `wr-cli` command wiring) gets
exercised together through the actual public API, not each unit's own isolated tests.

Dispatched to worktree `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-a9437470573d60839`
(branch `worktree-agent-a9437470573d60839`, based on `fa9488e`). Not yet returned as of this
checkpoint. Briefed to: survey what differential coverage already exists (several units, e.g. U16,
already built real `wr-cli`-layer differential tests; U11/U8's tests stop at `wr-logic`/`wr-core`
and don't fully satisfy this checkpoint's "through `wr-cli`" bar on their own), build a new
consolidated checkpoint suite covering boolean connectives + quantifiers in combination, a
custom-base query, word/function tokens with quantifiers, `reg`/`alphabet` cases, a TRUE/FALSE
`eval` result, and a `def` free-variable case exercising U15's just-fixed CAS-validation path —
and to log, not silently resolve, any genuine divergence found.

**This is a real, significant milestone once it lands** — the completion of Phase 3a (the full FOL
decider's engine + parser + CLI wiring, working end-to-end). **Flag this clearly to the user when
reached.** Phase 3b (the remaining `wr-core` primitives — `Morphism`/`convertNS`/`ProductBFS`/
`Transducer`, the real `Prover` dispatch/REPL/`MetaCommands`, all remaining `Commands/*`, and the
Tier-1 golden-corpus harness) still needs the user's **explicit go-ahead** before starting, per
`docs/ROADMAP-TO-AUTONOMY.md`'s phase-gating doctrine — same as Phase 3 itself needed at the start
of this whole stretch. Do not begin Phase 3b work without that explicit signal, even if "continue"
is said in a context that doesn't obviously address the phase boundary — the phase-gating rule is
about the PHASE transition specifically, not ordinary unit-to-unit continuation within a phase.

## Process notes for whoever resumes

- Same loop as every unit this phase: author (worktree-isolated background agent) → two
  split-context adversarial reviewers (diff-only, model ≠ author for trust-critical crates) →
  fixer if either review finds `correctness-fatal`/`correctness-risk` → verify (`cargo test
  --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current `master` → grep for
  WB-number duplicates/gaps AND check for non-WB content conflicts regardless of whether the
  rebase reported conflicts → fast-forward merge → `git worktree remove` + `git branch -d`.
- The exit-checkpoint task itself may or may not need a two-reviewer loop depending on what it
  actually touches (a NEW differential test file only touches `tests/differential`, not
  `wr-core`/`wr-logic` — check CLAUDE.md's trust-critical-crate scoping before deciding whether the
  full loop is mandatory here, vs. a single careful read given it's test-only, no production code).
