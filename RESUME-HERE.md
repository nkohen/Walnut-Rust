# RESUME-HERE — Phase 3a, unit-by-unit

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## Done and merged (master is green at these units) — 16 of 22 Phase 3a units

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
- **U9** — `RelationalOperator`/`ArithmeticOperator` `act()` semantics (`ad69512`, current `master` HEAD).
  Both adversarial reviewers returned clean (no correctness-fatal/correctness-risk findings) —
  merged directly, no fixer needed.

`cargo test --workspace` was green on `master` as of `ad69512` (wr-cli 13, wr-core 384, wr-io 98,
wr-logic 225, wr-cts 22, differential 1, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **23**
genuine Walnut (Java) bugs found and logged so far (WB-001 through WB-023 — see
`docs/WALNUT-BUGS.md`), several found empirically via real `walnut-java` CLI/jar reproduction
during this phase alone (WB-011 through WB-023). **WB-022 is U13's** (Rust-port NFAO-reader gap);
**WB-023 is U9's** (`RelationalOperator.act`'s word-vs-arithmetic mislabeled result string) — U9's
diff originally numbered its own finding "WB-022" too (parallel-worktree collision, resolved
during U9's rebase per the established pattern: renumbered every `WB-022`/`wb022` occurrence in
U9's diff, both in `docs/WALNUT-BUGS.md` and in `crates/wr-logic/src/token.rs`, to `WB-023`/`wb023`).

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
  rebase conflict resolved). This has now happened at least five times in this phase (U6/U4/U12
  all landed a "WB-016"; U13 landed "WB-022"; U9 independently claimed the same number and was
  renumbered to WB-023 on merge). **U8 (in flight, see below) reports its own local "WB-022" too
  — this WILL collide again and need renumbering to WB-024 (or higher, if anything else lands
  first) when U8 merges.**
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time so far.

## In flight right now

- **U8** — regex engine (hand-rolled Brics-dialect recursive-descent parser + Thompson
  construction, transliterated from the real `dk.brics:automaton` 1.12-4 sources jar). Authoring
  complete in worktree `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-a0ec2c14b70a1ab1a`
  (branch `worktree-agent-a0ec2c14b70a1ab1a`, based on `2e0befb` — now 3 commits behind `master`,
  will need a rebase before merge, which will surface the WB-022→WB-024 renumbering, and should
  be re-diffed against current `master`/`numsys.rs`/`token.rs` for unrelated drift since U9 also
  touched `wr-core`/`wr-logic` files this unit might brush against — check for conflicts, not
  just the WB doc). Author reported: `crates/wr-core/src/regex.rs` (~1549 LOC) +
  `crates/wr-core/src/regex/tests.rs` (~866 LOC, 49 tests incl. property tests) +
  `tests/differential/tests/reg_brics_regex.rs` (60-case differential gate against real
  `walnut-java` output, all matching) + a new WB-022 (its own numbering) for the
  `RichAlphabet`/`BricsConverter` `+128`-offset collision the plan pre-identified, ported verbatim.
  `cargo test --workspace` reported green in-worktree at author-done time (wr-core 428 = 379+49,
  fmt/clippy clean). Two split-context adversarial reviewers dispatched (background, task ids not
  recorded here — check `/workflows` or agent list if resuming cold), not yet returned. **Next
  action when reviews return**: reconcile findings, dispatch a fixer if needed, rebase onto
  current `master`, resolve the WB-022→WB-024 collision (rename in `docs/WALNUT-BUGS.md` AND
  `crates/wr-core/src/regex.rs`/`regex/tests.rs` wherever `WB-022`/`wb022` appears — same
  grep-both-spellings discipline as every prior collision), re-verify
  `cargo test --workspace`/`fmt`/`clippy` post-rebase, merge, clean up worktree.
- **U14** — `Session.java` (path-builder methods only; `Logging` already out of scope, ported as
  U0a) as an explicit context struct implementing U1's `PredicateEnv` trait for real (currently
  only an in-memory test double exists). Target `wr-cli` (`session.rs`, new file). Depends on U1,
  U5, U12, U13 — all done, so this was dispatched now to keep the pipeline moving while U8's
  reviews run. Opus tier per the plan (the plan's "2nd sanctioned mechanical-fidelity deviation" —
  Java's `Session` uses static/global path state, the Rust port uses an explicit struct instead,
  matching the same idiom `PredicateEnv`/`FreshIdentifiers` already established). Isolated
  subagent, self-contained. **Done-when** (per the plan): a `PredicateEnv` impl backed by real
  files works against a temp directory tree. Not yet returned as of this checkpoint.

## Blocked until the above land

- **U10** (LogicalOperator + quantifier-elimination driving) — U9 is done, so **U10 is now
  unblocked**; not yet dispatched (review/merge bandwidth was on U9/U8/U13 this round). Good next
  dispatch once U8/U14 free up.
- **U11** (shared postfix executor — the Phase 3a checkpoint: literal predicate string → Automaton
  using only `wr-logic`+`wr-core`) needs U9 (**done**) AND U10 (not started).
- **U15** (`EvalDef.java`) needs U11 AND U14 (in flight).
- **U16** (`Reg.java` + `alphabet` command) needs U8 (in flight) AND U14 (in flight).

See the plan file's "Phase 3a units" table for full details on each.

## Process notes for whoever resumes

- Follow the same loop used for every unit so far: author (worktree-isolated background agent) →
  two split-context adversarial reviewers (diff-only, model ≠ author where the file was authored
  by a modeled subagent) → fixer if either review finds `correctness-fatal`/`correctness-risk` →
  verify (`cargo test --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current
  `master` → fast-forward merge → `git worktree remove` + `git branch -d` to clean up.
- U9 was the first unit this phase where BOTH reviewers returned completely clean (no findings at
  all, not even minor) — straight to merge, no fixer round needed. Don't assume this is typical;
  keep dispatching the full two-reviewer loop for every remaining trust-critical unit regardless.
- Watch for cross-unit file overlap when rebasing (U0/U0b both touched `automaton.rs`/`numsys.rs`
  and rebased clean, but always verify post-rebase — don't assume). U8's rebase in particular now
  needs checking against U9's `numsys.rs` changes, not just the WB-doc collision.
- This session hit two unrelated agent-infrastructure stalls (a 600s stream-watchdog timeout) on
  earlier units, both resolved by simply relaunching the same agent fresh — not a sign of a deeper
  problem, just retry if it recurs.
- **U10 is now unblocked** (its last dependency, U9, just landed) — dispatch once U8/U14 stop
  needing review-bandwidth attention. After U10, U11 (the Phase 3a integration checkpoint) becomes
  reachable.
