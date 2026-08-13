# RESUME-HERE — Phase 3a: COMPLETE

Phase 3a (`.claude/plans/synthetic-prancing-aurora.md`) is finished: all 22 units plus the exit
checkpoint are merged to `master`, `cargo test --workspace` is green, `cargo fmt`/`clippy` are
clean. `CLAUDE.md`'s "Current status" section now carries the authoritative summary of what
landed — read that first if you're resuming cold. This file's job going forward is process notes
for whoever picks up Phase 3b (which needs the user's explicit go-ahead per
`docs/ROADMAP-TO-AUTONOMY.md`'s phase-gating doctrine before any code starts).

## Immediate next step if the user gives the Phase 3b go-ahead

Per the plan's "Phase 3b units" table: `Morphism`, `convertNS`, `Search/ProductBFS`, `Transducer`
(the remaining `wr-core` primitives, all previously deferred), then `Prover.java`'s real
dispatch/REPL/command-file reading/`MetaCommands` (wiring real parsed values into Phase 3a's U0c
no-op `DeterminizeContext` hook), all remaining `Commands/*` (batched — `Combine`/`Concat`/
`Union`/`Intersect`/`Star`/`Reverse`/`Quotient`/`Describe`/inline `minimize`/`fixleadzero`/
`fixtrailzero`, `morphism`/`image`/`promote`/`join`/`convert`/`inf`/`export`, `test`,
`transduce`), `HelpMessages`, and finally the Tier-1 golden-corpus harness (`tests/golden`,
consuming `walnut-java/phase0-artifacts/subset-filter.json`) — the actual DESIGN.md Phase-3 exit
criterion ("Tier 1 green; eval/def/reg work").

**A real, open item from Phase 3a to fold into Phase 3b's planning**: `eval`/`def`/`reg` over
`lsd_*` numeration combined with any quantifier (`E`/`A`/`I`) currently fails
(`QuantifyError::UnsupportedLsdFixup`, `wr-core::quantify`) — pre-existing Phase-2 scope debt, not
new, but now confirmed live and worth a deliberate fix-now-vs-schedule decision before Phase 3b's
golden-corpus harness hits an `lsd`+quantifier fixture (it will — 18%+ of golden fixtures use
custom/alternate bases per earlier phase counts, and `lsd` is common among them).

## Process lessons from this phase, worth carrying into Phase 3b

- **The implementer → two-reviewer → fixer loop caught a real, material issue in nearly every
  back-half unit** (U8, U14, U16, U11, U15, and the exit checkpoint itself) — never skip it, and
  always read both full reports, not just the summary line ("no correctness-fatal" ≠ "nothing to
  fix"). U9/U10 (the two highest-stakes trust-critical units) came through clean on the first
  pass — encouraging, but not a reason to relax rigor elsewhere.
- **WB-number collisions across parallel worktrees are routine** when units land concurrently —
  resolve by keeping the earlier-merging unit's number(s) and renumbering the later one's (heading
  + every in-body self-reference + any `.rs` test-name/doc-comment citations, both `WB-NNN` and
  lowercase `wbNNN` spellings). Always grep `^## WB-` for duplicate/gapped numbers after ANY
  rebase touching `docs/WALNUT-BUGS.md`, whether or not the rebase itself reported a conflict —
  git can silently interleave non-overlapping insertions without flagging a real numbering clash.
- **A background agent hitting a session-limit error mid-task is a pause, not a failure** — its
  worktree and partial progress survive; resume it via `SendMessage` to its agent ID rather than
  relaunching fresh, so accumulated context/work isn't thrown away.
- **Coordinator self-caution**: after `cd`-ing into a worktree for inspection, explicitly `cd`
  back to the intended target directory before the next stateful git command — a stale shell `cwd`
  from several tool calls back caused one misdirected (non-destructive) commit attempt this phase.
  Separately, this session's own attempt to fix a reviewer-flagged test-gap directly (rather than
  delegating) via careless global `sed` replacements corrupted unrelated lines sharing the same
  literal text (a predicate string reused in both a test body and its own module-doc capture
  recipe) — caught by rerunning the full suite and a deliberate falsification check before
  committing, not by the edit itself. Prefer precise `Edit`-tool replacements with enough
  surrounding context to be unique over `sed` when a file has repeated literal substrings.
- **A test that's supposed to prove a normalization step matters should be falsified before being
  trusted** — temporarily disable the step and confirm the specific test(s) that should fail
  actually do, and that everything else still passes. This caught (twice this phase — U11's
  reviewer, then again while fixing the exit checkpoint's own review finding) a normalization
  call that looked load-bearing in a comment but wasn't exercised by any test that would actually
  fail without it.
