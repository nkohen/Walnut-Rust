# RESUME-HERE — Phase 3a, unit-by-unit

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## Done and merged (master is green at these units) — 20 of 22 Phase 3a units

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
  (`19c2c28`) — **the decision-procedure crux of the whole port**, cleared by both reviewers with
  zero correctness findings
- **U16** — `Reg.java` + the full `alphabet` command body (`991e8f4`, current `master` HEAD). Both
  reviewers cleared it (no correctness-fatal/-risk); one hand-traced `Automaton.setAlphabet`'s full
  assembly order line-by-line and empirically probed the one code path with zero test coverage
  (the `is_dfao=true` branch) before signing off. Coordinator added a permanent unit test for that
  branch (previously only covered by the reviewer's own temporary probe) and fixed a doc comment
  this unit's own new call site made stale, both directly rather than via a fixer agent.

`cargo test --workspace` was green on `master` as of `991e8f4` (wr-cli 58, wr-core 444, wr-io 99,
wr-logic 253, wr-cts 22, differential 4, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **27**
genuine Walnut (Java) bugs found and logged so far (WB-001 through WB-027 — see
`docs/WALNUT-BUGS.md`; U16 found no new genuine bug, only two faithfully-preserved quirks
documented in code comments, correctly not logged since neither is a defect).

**Process notes for whoever resumes:**
- **WB-number collisions remain routine when a unit adds new entries** — none this round (U16
  didn't add any). Always grep `^## WB-` for duplicate/gapped numbers after ANY rebase touching
  `docs/WALNUT-BUGS.md`, conflict-reported or not.
- **A real coordinator mistake this round, caught before damage**: while investigating what looked
  like a fleet-hygiene incident (U16's agent appearing to leave stray fixture files in the main
  worktree), the coordinator's own shell had a STALE `cd` left over from a previous command and
  ran a `git add`/`git commit` for `RESUME-HERE.md` from INSIDE a different worktree — the commit
  silently no-op'd there (nothing destructive happened; `git add`/`commit`/`log` are non-destructive
  even when misdirected) but had to be redone from the correct directory. **Lesson: after any
  sequence of commands that `cd`s into a worktree for inspection, explicitly `cd` back to the
  intended target directory before the next stateful command (`git add`/`commit`/`merge`) — don't
  rely on remembering the last `cd` several tool calls back.** Relatedly, the "stray fixture files"
  themselves turned out to be a false alarm on re-inspection (the main worktree was clean once
  checked from the correct directory) — likely the coordinator's own earlier stale-cwd read, not a
  real agent misbehavior. No actual fleet-hygiene violation occurred this round.
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time so far.

## In flight right now

- **U11** — the shared postfix-token executor (`EvalDef.compute`'s core, generalized) + final
  `Predicate` assembly. Target `wr-logic` (`eval.rs`, new). **This is the Phase 3a integration
  checkpoint** — its Done-when is literally "a test evaluates a literal predicate string end-to-end
  to an `Automaton` using only `wr-logic`+`wr-core` — no `wr-cli`/`wr-io`," i.e. the first real
  proof the whole parser (U2/U3/U4) + quantifier-elimination (U10) + operand-semantics (U9) stack
  actually composes, not just that each piece passes its own isolated tests. Worktree
  `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-a7991d97d30d1af49` (branch
  `worktree-agent-a7991d97d30d1af49`, based on `ba7389f` — now 1 commit behind `master`, will need
  a rebase). Not yet returned as of this checkpoint. Sonnet tier per the plan (integration work
  over already-reviewed primitives, not new algorithm design) — **still needs the full
  two-reviewer loop** since it touches `wr-logic` (trust-critical).

## Blocked until the above lands

- **U15** (`EvalDef.java`, the actual `wr-cli`-level command) needs U11 AND U14 (**done**) — the
  last remaining Phase 3a unit after U11 lands.

Once U11 and U15 both land, the **Phase 3a exit checkpoint** is next: extend `tests/differential`
with `eval`/`def`/`reg` cases (literal strings via `wr-cli`'s library API, using U0c's no-op
strategy/export context) compared against real `walnut-java` CLI output through `wr_core::equiv`.
See the plan file's "Phase 3a exit checkpoint" section for the full exit criteria.

## Process notes for whoever resumes

- Follow the same loop used for every unit so far: author (worktree-isolated background agent) →
  two split-context adversarial reviewers (diff-only, model ≠ author where the file was authored
  by a modeled subagent) → fixer if either review finds `correctness-fatal`/`correctness-risk` →
  verify (`cargo test --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current
  `master` → grep for WB-number duplicates/gaps regardless of whether the rebase reported conflicts
  → fast-forward merge → `git worktree remove` + `git branch -d` to clean up.
- **Always `cd` explicitly to the intended directory before a stateful git command** — see the
  process note above. Don't trust a `cd` from several tool calls back.
- This is the closest Phase 3a has been to done — only U11 (in flight) and U15 (blocked on it)
  remain before the sub-phase's own exit checkpoint. Don't relax review rigor for the home stretch;
  U8/U14/U16 all found real, fixable issues even in units that looked clean on first read.
