# RESUME-HERE — Phase 3a, unit-by-unit

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## Done and merged (master is green at these units) — 17 of 22 Phase 3a units

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
- **U9** — `RelationalOperator`/`ArithmeticOperator` `act()` semantics (`ad69512`). Both adversarial
  reviewers returned clean — merged directly, no fixer needed.
- **U8** — regex engine, Brics-dialect parser + Thompson construction (`2fb3f76`, current `master`
  HEAD). Two reviewers found no correctness-fatal defects but did find one real, previously-unlogged
  correctness-risk (a second face of the alphabet-offset wraparound bug, triggered by legitimate
  large alphabet sizes rather than out-of-alphabet digits — logged as **WB-025**) plus a false
  doc claim and a property-test gap; all three fixed by a fixer pass before merge.

`cargo test --workspace` was green on `master` as of `2fb3f76` (wr-cli 13, wr-core 434, wr-io 98,
wr-logic 225, wr-cts 22, differential 3, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **25**
genuine Walnut (Java) bugs found and logged so far (WB-001 through WB-025 — see
`docs/WALNUT-BUGS.md`). **WB-022 is U13's** (Rust-port NFAO-reader gap), **WB-023 is U9's**
(`RelationalOperator.act` mislabeled result string), **WB-024/WB-025 are both U8's** (the
`RichAlphabet`/`BricsConverter` `+128`-offset collision — WB-024 for out-of-alphabet digits,
WB-025 for the same offset overflowing on legitimate large alphabet sizes near 65535).

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
  lowercase `wbNNN`/`wb_NNN` spellings across the whole diff, not just the doc file, before
  considering a rebase conflict resolved). **U8 was a double collision** (it independently claimed
  BOTH "WB-022" and "WB-023" locally, both already taken by U13/U9 on master by the time it
  rebased) — resolved by renumbering both to WB-024/WB-025 via a small Python script that located
  the incoming diff hunk by its heading text and did a scoped find-replace, then verifying via
  grep across `docs/WALNUT-BUGS.md` AND the touched `.rs`/test files. This pattern has now
  recurred six times this phase; treat any future rebase touching `docs/WALNUT-BUGS.md` as
  needing this check by default, not as a surprise.
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time so far.

## In flight right now

- **U10** — `LogicalOperator.java` (162 LOC) — boolean-connective dispatch (mostly thin wrappers
  over already-shipped `wr-core::logicalops` primitives) **and the actual quantifier-elimination
  DRIVING logic**: `E`→`wr-core::quantify`'s ∃-projection, `A`→¬∃¬ (wiring the already-proven
  Tier-4 duality identity into real formula evaluation), `I`→leading-zero removal +
  `wr-core::infinite::infinite`. This is **the decision-procedure crux** of the whole port — Opus,
  isolated subagent, dispatched to worktree
  `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-ae9fba69726977be1` (branch
  `worktree-agent-ae9fba69726977be1`, based on `2fb3f76`). Not yet returned as of this checkpoint.
  Explicitly briefed to check U0's `TRUE_FALSE_AUTOMATON` short-circuit composes correctly with
  `A`/`I` quantifier driving (a re-review the Phase 3 plan flagged as needed once U0 landed — check
  whether it happened). **When it returns: this needs the FULL two-reviewer adversarial loop,
  reviewer model ≠ Opus (the author), given CLAUDE.md's explicit trust-critical/decision-procedure
  rule** — do not shortcut this one even if the diff looks clean, it's the highest-stakes unit
  remaining in Phase 3a.
- **U14** — `Session.java` (path-builder methods only) as an explicit context struct implementing
  U1's `PredicateEnv` trait for real (currently only an in-memory test double exists). Target
  `wr-cli` (`session.rs`, new file). Opus tier, isolated subagent, worktree
  `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-abb70bfad6d31e50d` (branch
  `worktree-agent-abb70bfad6d31e50d`, based on `ad69512` — now 2 commits behind `master`, will need
  rebase before merge). Not yet returned as of this checkpoint. **Done-when**: a `PredicateEnv`
  impl backed by real files works against a temp directory tree. Briefed to be aware of (not
  re-log) WB-005/WB-006, both directly about `Session.java`.

## Blocked until the above land

- **U11** (shared postfix executor — the Phase 3a checkpoint: literal predicate string → Automaton
  using only `wr-logic`+`wr-core`) needs U9 (**done**) AND U10 (in flight) — **the next dispatch
  once U10 lands**, and arguably the single most important remaining checkpoint in Phase 3a.
- **U15** (`EvalDef.java`) needs U11 AND U14 (in flight).
- **U16** (`Reg.java` + `alphabet` command) needs U8 (**done**) AND U14 (in flight).

See the plan file's "Phase 3a units" table for full details on each.

## Process notes for whoever resumes

- Follow the same loop used for every unit so far: author (worktree-isolated background agent) →
  two split-context adversarial reviewers (diff-only, model ≠ author where the file was authored
  by a modeled subagent) → fixer if either review finds `correctness-fatal`/`correctness-risk` →
  verify (`cargo test --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current
  `master` → fast-forward merge → `git worktree remove` + `git branch -d` to clean up.
- U9 was the first (and so far only) unit this phase where BOTH reviewers returned completely
  clean. U8's two reviewers together found 3 real issues (1 correctness-risk, 1 doc defect, 1
  test-gap) despite neither alone flagging a correctness-FATAL defect — a reminder that "no
  fatal finding" is not the same as "nothing to fix," and that reading BOTH reports in full before
  deciding whether a fixer pass is needed remains important even when the summary sounds clean.
- Watch for cross-unit file overlap when rebasing — U8 touched `crates/wr-core/src/lib.rs` and
  `product.rs` (a visibility change only) in addition to its own new `regex.rs`; U10 (in flight)
  will likely touch `crates/wr-logic/src/token.rs` again (same file U9 just extended) — check that
  rebase carefully when it lands, not just the WB-doc collision.
- This session hit two unrelated agent-infrastructure stalls (a 600s stream-watchdog timeout) on
  earlier units, both resolved by simply relaunching the same agent fresh — not a sign of a deeper
  problem, just retry if it recurs.
