# RESUME-HERE — Phase 3a, unit-by-unit

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## Done and merged (master is green at these units) — 18 of 22 Phase 3a units

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
- **U8** — regex engine, Brics-dialect parser + Thompson construction (`2fb3f76`). Fixer resolved
  one correctness-risk (WB-025, a second face of the alphabet-offset wraparound) plus a doc defect
  and a property-test gap.
- **U14** — `Session.java` as an explicit context struct, the first real file-backed `PredicateEnv`
  impl (`3e1df51`, current `master` HEAD). Reviewer #2 found a real correctness-risk: the author's
  own flagged divergence (nested custom-base header resolution using only the global directory)
  rested on a factually wrong justification — Java resolves nested-header lookups through the same
  session-aware path as top-level queries. Fixer added a proper `CustomBaseResolver` trait seam in
  `wr-io` (not a workaround) so `wr-cli` can supply real session-then-global precedence, verified
  with a falsification test (fails against the old behavior, passes against the fix). Also logged
  **WB-024** locally (renumbered to **WB-026** on merge — see collision note below): a real,
  live-confirmed Java bug where `Prover.parseArgs` validates the command file before
  `Session.setPathsAndNames` runs, silently ignoring `--home-dir=`; deferred to U21 (not yet
  ported) since the defect lives in `Prover`, not `Session`.

`cargo test --workspace` was green on `master` as of `3e1df51` (wr-cli 38, wr-core 434, wr-io 99,
wr-logic 225, wr-cts 22, differential 3, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **26**
genuine Walnut (Java) bugs found and logged so far (WB-001 through WB-026 — see
`docs/WALNUT-BUGS.md`). Recent numbering: **WB-022** = U13 (Rust-port NFAO-reader gap), **WB-023**
= U9 (`RelationalOperator.act` mislabeled result string), **WB-024/WB-025** = U8 (the
`RichAlphabet`/`BricsConverter` `+128`-offset collision, two trigger conditions), **WB-026** = U14
(`Prover.parseArgs`'s `--home-dir=`-ignoring bug, deferred to U21).

**Process notes for whoever resumes:**
- **Dependency-order slip (already happened once, don't repeat it)**: check each unit's REAL
  "Depends on" column in the plan before starting it, not just the unit-number order — U3 turned
  out to depend on U5, not just U2, which wasn't caught until after U2 was already done.
- **WB-number collisions across parallel units are routine, not a sign of a problem** — see the
  detailed resolution procedure below. **New wrinkle this round**: U14's rebase onto master
  produced **no conflict markers at all** in `docs/WALNUT-BUGS.md` (git's 3-way merge silently
  interleaved U14's new entry into the file without flagging it, because the surrounding context
  lines didn't textually overlap with U8/U9's insertions) — yet it still silently created a
  duplicate `## WB-024` heading. **Lesson: a clean, conflict-free rebase of a unit that touches
  `docs/WALNUT-BUGS.md` is NOT proof the numbering is still collision-free — always grep
  `^## WB-` for duplicate/gapped numbers after ANY rebase that touches this file, even one that
  reported no conflicts.** (`grep -oP '^## WB-\K\d+' docs/WALNUT-BUGS.md | sort -n | uniq -c` to
  spot duplicates; a short awk one-liner to spot gaps — see shell history if needed.)
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time so far.

## In flight right now

- **U10** — `LogicalOperator.java` (162 LOC) — boolean-connective dispatch (mostly thin wrappers
  over already-shipped `wr-core::logicalops` primitives) **and the actual quantifier-elimination
  DRIVING logic**: `E`→`wr-core::quantify`'s ∃-projection, `A`→¬∃¬ (wiring the already-proven
  Tier-4 duality identity into real formula evaluation), `I`→leading-zero removal +
  `wr-core::infinite::infinite`. This is **the decision-procedure crux** of the whole port — Opus,
  isolated subagent, worktree `/Users/nkohen/dev/walnut-rs/.claude/worktrees/agent-ae9fba69726977be1`
  (branch `worktree-agent-ae9fba69726977be1`, based on `2fb3f76` — now 2 commits behind `master`,
  will need a rebase touching `crates/wr-logic/src/token.rs` again, the same file U9 just extended
  — check carefully, and also `docs/WALNUT-BUGS.md` per the note above). Not yet returned as of
  this checkpoint. **When it returns: this needs the FULL two-reviewer adversarial loop, reviewer
  model ≠ Opus (the author)** — do not shortcut this one even if the diff looks clean, it's the
  highest-stakes unit remaining in Phase 3a. Explicitly briefed to check U0's
  `TRUE_FALSE_AUTOMATON` short-circuit composes correctly with `A`/`I` quantifier driving.

## Blocked until the above lands

- **U11** (shared postfix executor — the Phase 3a checkpoint: literal predicate string → Automaton
  using only `wr-logic`+`wr-core`) needs U9 (**done**) AND U10 (in flight) — **the next dispatch
  once U10 lands**, and arguably the single most important remaining checkpoint in Phase 3a.
- **U15** (`EvalDef.java`) needs U11 AND U14 (**done**).
- **U16** (`Reg.java` + `alphabet` command) needs U8 (**done**) AND U14 (**done**) —
  **U16 is now unblocked and ready to dispatch** once review bandwidth allows.

See the plan file's "Phase 3a units" table for full details on each.

## Process notes for whoever resumes

- Follow the same loop used for every unit so far: author (worktree-isolated background agent) →
  two split-context adversarial reviewers (diff-only, model ≠ author where the file was authored
  by a modeled subagent) → fixer if either review finds `correctness-fatal`/`correctness-risk` →
  verify (`cargo test --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current
  `master` → **grep for WB-number duplicates/gaps regardless of whether the rebase reported
  conflicts** → fast-forward merge → `git worktree remove` + `git branch -d` to clean up.
- Both U8 and U14 are reminders that "no reviewer flagged correctness-fatal" is not the same as
  "nothing to fix" — U8's two reviewers together found 3 real issues despite neither alone calling
  anything fatal; U14's reviewer #2 found a real correctness-risk (with a concrete failing
  scenario) that reviewer #1 entirely missed, on a claim the AUTHOR had already flagged for a
  second opinion but under an incorrect justification. Read both full reports before deciding a
  unit is clean, not just the summary line.
- **U16 is now unblocked** (both its dependencies, U8 and U14, are done) — a good next dispatch
  once U10's review loop is running and doesn't need full attention.
- This session hit two unrelated agent-infrastructure stalls (a 600s stream-watchdog timeout) on
  earlier units, both resolved by simply relaunching the same agent fresh — not a sign of a deeper
  problem, just retry if it recurs.
