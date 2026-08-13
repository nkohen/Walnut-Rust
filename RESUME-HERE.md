# RESUME-HERE — Phase 3a, unit-by-unit

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## Done and merged (master is green at these units) — 19 of 22 Phase 3a units

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
- **U14** — `Session.java` as an explicit context struct, first real file-backed `PredicateEnv`
  impl (`3e1df51`)
- **U10** — `LogicalOperator`'s connective dispatch + the actual quantifier-elimination driving
  logic (`19c2c28`, current `master` HEAD). **This was the decision-procedure crux of the whole
  port.** Both adversarial reviewers independently reproduced its new bug finding (WB-027, `I`
  silently drops a free variable) live against the real CLI and found no correctness-fatal or
  correctness-risk defects. Two minor items (a doc nit, a test-gap on `remove_leading_zeros`
  interacting with a custom-base restriction) were fixed directly by the coordinator rather than
  via a fixer agent, given their triviality — same judgment call as earlier small nits in U5.

`cargo test --workspace` was green on `master` as of `19c2c28` (wr-cli 38, wr-core 444, wr-io 99,
wr-logic 253, wr-cts 22, differential 3, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **27**
genuine Walnut (Java) bugs found and logged so far (WB-001 through WB-027 — see
`docs/WALNUT-BUGS.md`). Recent numbering: **WB-024/WB-025** = U8, **WB-026** = U14
(`Prover.parseArgs`'s `--home-dir=`-ignoring bug), **WB-027** = U10 (the `I` quantifier silently
discarding free variables — confirmed independently by both reviewers against the live CLI).

**Process notes for whoever resumes:**
- **WB-number collisions across parallel units remain routine** — this round (U10) collided again
  with the already-merged WB-026 (U14's), resolved the same well-established way: renumber the
  later-merging unit's entry (and its `.rs` source citations) to continue the sequence. Always grep
  `^## WB-` for duplicate/gapped numbers after ANY rebase touching `docs/WALNUT-BUGS.md`, even a
  conflict-free one (a lesson from the U14 round, still worth repeating).
- **NEW this round — a real fleet-hygiene incident, low severity, worth flagging for the next
  session**: while U10 was merging, U16's background agent (dispatched with `isolation: "worktree"`
  into `.claude/worktrees/agent-abc04a6c5725e2533`) left several UNTRACKED capture-fixture files
  directly in the MAIN worktree (`tests/differential/fixtures/alphabet/{baseB,baseB_asSet,baseC,
  baseC_restricted}.txt`, `tests/differential/fixtures/reg/u16_{mixed_ns,msd3,set}.txt`) instead of
  its own isolated worktree — almost certainly an absolute-path slip in an empirical-verification
  script run against the real `walnut-java` CLI, not a shared-tree/git-level collision (U16's own
  worktree's git status shows no corresponding files, and nothing TRACKED was touched, so no data
  was lost or corrupted — CLAUDE.md's "container agents run foreground" and "atomic pathspec-scoped
  commits" rules were both still honored at the git level). **Left in place, untouched, pending
  reconciliation once U16 completes** — check whether U16's own commit expects these exact files
  (in which case move/copy them into U16's worktree before its commit) or whether they were just
  scratch verification output already consumed (in which case they can be deleted). Do not delete
  them speculatively before checking.
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time so far.

## In flight right now

- **U16** — `Reg.java` + the full `alphabet` command body (`determineAlphabetsAndNS` AND
  `Automaton.setAlphabet`, folded together per the plan's correction that a first draft only
  covered half). Target `wr-cli`. Sonnet tier, isolated subagent, worktree
  `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-abc04a6c5725e2533` (branch
  `worktree-agent-abc04a6c5725e2533`, based on `364c15b` — now 1 commit behind `master`, will need
  a rebase). Not yet returned as of this checkpoint. Already has real, substantive progress
  in-worktree (`crates/wr-cli/src/{alphabet,automaton_output,reg}.rs` new, plus edits to
  `session.rs`/`lib.rs`/`wr-core::automaton.rs`) — see the stray-fixture-files note above for one
  loose end to check at merge time, separate from the actual code review.

## Blocked until the above lands

- **U11** — shared postfix-token executor (`EvalDef.compute`, generalized, also reused by `image`)
  + final `Predicate` assembly. Target `wr-logic` (`eval.rs`, new). Depends on U9 (**done**) AND
  U10 (**done**) — **fully unblocked, the single most important remaining checkpoint in Phase 3a**:
  its Done-when is literally "a test evaluates a literal predicate string end-to-end to an
  `Automaton` using only `wr-logic`+`wr-core` — no `wr-cli`/`wr-io`," i.e. the first real proof the
  whole parser+quantifier+operand-semantics stack actually composes. **Dispatch this next**,
  independent of U16's status (different crate, no file overlap expected).
- **U15** (`EvalDef.java`) needs U11 AND U14 (**done**).

See the plan file's "Phase 3a units" table for full details on each.

## Process notes for whoever resumes

- Follow the same loop used for every unit so far: author (worktree-isolated background agent) →
  two split-context adversarial reviewers (diff-only, model ≠ author where the file was authored
  by a modeled subagent) → fixer if either review finds `correctness-fatal`/`correctness-risk` →
  verify (`cargo test --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current
  `master` → grep for WB-number duplicates/gaps regardless of whether the rebase reported conflicts
  → fast-forward merge → `git worktree remove` + `git branch -d` to clean up.
- U10 (the highest-stakes unit in this phase) came through both reviews with zero correctness
  findings — a good sign for the port's overall trajectory, but keep running the full loop on
  U15/U16 regardless; U8/U14 already showed "no fatal finding" isn't the same as "nothing to fix."
- This session hit two unrelated agent-infrastructure stalls (a 600s stream-watchdog timeout) on
  earlier units, both resolved by simply relaunching the same agent fresh — not a sign of a deeper
  problem, just retry if it recurs.
