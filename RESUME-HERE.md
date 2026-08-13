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

**RESOLVED (Phase 3b, L1) — was the one real open item Phase 3a left behind — scope corrected
below, read it before assuming this covers more than it does.** `eval`/`def` over `lsd_*`
numeration used to fail with `QuantifyError::UnsupportedLsdFixup` (`wr-core::quantify`),
pre-existing Phase-2 scope debt confirmed live by Phase 3a's exit checkpoint. It was reported as
"lsd + a quantifier", but the real blast radius was wider: because `wr_core::numsys` calls
`quantify` to build its own automata, *any* `lsd_k` comparison or arithmetic against a constant
`>= 2` failed too — `?lsd_2 x >= 2`, with no user-written quantifier anywhere. L1 wired
`AutomatonLogicalOps.fixTrailingZerosProblem` into `quantify`'s `Some(false)` arm (the plain port
of Java's `AutomatonQuantification.java:46`, which U5 had already made available and U6 chose not
to call), deleted the now-unconstructible `UnsupportedLsdFixup` variant, flipped the four tests
that pinned the rejection into tests of the computed language, and added the positive coverage
that had never existed: `wr-core`'s `quantify_on_an_lsd_automaton_runs_the_trailing_zero_fixup`
(msd/lsd contrast on one transition table), `numsys`'s
`lsd_composed_constructions_compute_the_right_language` +
`msd_and_lsd_composed_constructions_agree_after_reversal` (Tier 4), `wr-logic`'s
`lsd_numeration_evaluates_end_to_end`, and `tests/differential/tests/lsd_numeration.rs` (five
cases against real `walnut-java` output, incl. a three-track `lsd_3` one).
**Scope, precisely** (a first draft of this note overclaimed and was corrected on review): verified
for `lsd_k` only (not `lsd_fib`/custom-base) — `∃` and closed `∀` (`¬∃¬`) both confirmed correct.
**Not verified**: the `I` quantifier over `lsd` (dispatches through `wr_core::infinite::infinite`,
which never calls `quantify` — this fix neither touched nor tested it), and `reg` over `lsd_*`
(also never routes through `quantify`; has its own separate, pre-existing coverage in
`reg_and_alphabet_commands.rs`). Phase 3b's golden-corpus harness can take `lsd_k` `eval`/`def`
fixtures now, but an `lsd`+`I` or custom-base-`lsd` fixture is still untested ground.

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
