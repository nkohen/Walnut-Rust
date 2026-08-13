# RESUME-HERE — Phase 3a, unit-by-unit

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## Done and merged (master is green at these units) — 15 of 22 Phase 3a units

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
- **U13** — custom-base reader headers + `readTransducer`/`readComments`/`AutomatonDFA(String)`
  (`63e0927`, current `master` HEAD)

`cargo test --workspace` was green on `master` as of `63e0927` (wr-cli 13, wr-core 379, wr-io 98,
wr-logic 189, wr-cts 22, differential 1, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **22**
genuine Walnut (Java) bugs found and logged so far (WB-001 through WB-022 — see
`docs/WALNUT-BUGS.md`), several found empirically via real `walnut-java` CLI/jar reproduction
during this phase alone (WB-011 through WB-021; **WB-022 in `docs/WALNUT-BUGS.md` is U13's**
— see the WB-number-collision note below, it is NOT the same WB-022 U9 found independently in
its own worktree, which will need renumbering to WB-023 on merge).

**Process notes for whoever resumes:**
- **Dependency-order slip (already happened once, don't repeat it)**: check each unit's REAL
  "Depends on" column in the plan before starting it, not just the unit-number order — U3 turned
  out to depend on U5, not just U2, which wasn't caught until after U2 was already done.
- **WB-number collisions across parallel units are routine, not a sign of a problem**: when
  units are authored in parallel worktrees, they each number new `docs/WALNUT-BUGS.md` entries
  starting from whatever the LOCAL worktree's file showed at branch time — collisions on `git
  rebase` are expected. Resolve by keeping the earlier-merging unit's number(s) as-is and
  renumbering the later unit's entries to continue the sequence (heading text AND every in-body
  self-reference — the entry's own prose, AND any `wbNNN_test_name` test function names /
  `WB-NNN`-citing doc comments in the corresponding `.rs` file(s) — grep for both the `WB-NNN` and
  lowercase `wbNNN` spellings across the whole diff, not just the doc file, before considering a
  rebase conflict resolved). This has now happened at least four times in this phase (U6/U4/U12
  all landed a "WB-016"; U13 just landed "WB-022" and U9's in-flight worktree independently
  claimed the same number for a different bug — U9 will need renumbering to WB-023 when it
  merges).
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time so far.

## In flight right now

- **U9** — `RelationalOperator`/`ArithmeticOperator` `act()` semantics. Authoring complete in
  worktree `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-a93904928fd2f6b87` (branch
  `worktree-agent-a93904928fd2f6b87`, based on `2e0befb` — one commit behind current `master`,
  will need a rebase onto `63e0927` before merge, which will surface the WB-022→WB-023
  renumbering). Two split-context adversarial reviewers dispatched (background), not yet
  returned. Found and logged (locally) **WB-022** (its own numbering): `RelationalOperator.act`'s
  word-vs-arithmetic arm mislabels its result string, dropping the operator and other operand —
  confirmed live against the real CLI. Also updated WB-003's status from "not yet reached" to
  "ported verbatim (quirk)". `cargo test --workspace` reported green in-worktree at author-done
  time (wr-core 384, wr-logic 225). **Next action when reviews return**: reconcile findings,
  dispatch a fixer if needed, rebase onto `63e0927`, resolve the WB-022→WB-023 collision (rename
  in both `docs/WALNUT-BUGS.md` and `crates/wr-logic/src/token.rs`/`crates/wr-core/src/numsys.rs`
  wherever `WB-022`/`wb022` appears in U9's diff), merge.
- **U8** — regex engine. Still authoring in background (task `a0ec2c14b70a1ab1a`), worktree
  `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-a0ec2c14b70a1ab1a` (currently locked, based
  on `2e0befb`). No notification received yet as of this checkpoint. Opus-authored, isolated
  subagent, genuinely self-contained (unblocks nothing else until U16) — largest remaining unit
  in Phase 3a (~800–1,100 LOC estimate). When it completes: dispatch the two-reviewer loop same
  as U9, watch for the `RichAlphabet`/`BricsConverter` alphabet-offset collision bug the plan
  pre-identified (must be logged to `docs/WALNUT-BUGS.md`, another likely WB-number collision
  candidate), and confirm hand-written differential cases for `&`/`~`/`[^…]` exist (zero
  golden-corpus coverage for these).

## Blocked until the above land

- **U10** (LogicalOperator + quantifier-elimination driving) needs U9 done first.
- **U11** (shared postfix executor — the Phase 3a checkpoint: literal predicate string → Automaton
  using only `wr-logic`+`wr-core`) needs U9 AND U10.
- **U14** (`Session.java` as explicit context) needs U13 done (**done**, plus U1/U5/U12, already
  done) — **U14 is now unblocked and ready to dispatch** once U9/U8 free up review bandwidth.
- **U15** (`EvalDef.java`) needs U11 AND U14.
- **U16** (`Reg.java` + `alphabet` command) needs U8 AND U14.

See the plan file's "Phase 3a units" table for full details on each.

## Process notes for whoever resumes

- Follow the same loop used for every unit so far: author (worktree-isolated background agent) →
  two split-context adversarial reviewers (diff-only, model ≠ author where the file was authored
  by a modeled subagent) → fixer if either review finds `correctness-fatal`/`correctness-risk` →
  verify (`cargo test --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current
  `master` → fast-forward merge → `git worktree remove` + `git branch -d` to clean up.
- Watch for cross-unit file overlap when rebasing (U0/U0b both touched `automaton.rs`/`numsys.rs`
  and rebased clean, but always verify post-rebase — don't assume).
- This session hit two unrelated agent-infrastructure stalls (a 600s stream-watchdog timeout) on
  earlier units, both resolved by simply relaunching the same agent fresh — not a sign of a deeper
  problem, just retry if it recurs.
- **U14 is now unblocked** (its last dependency, U13, just landed) — a good next dispatch once
  U9/U8 stop needing review-bandwidth attention.
