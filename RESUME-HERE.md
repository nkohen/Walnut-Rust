# RESUME-HERE — Phase 3a, unit-by-unit

The plan being executed is `/Users/nkohen/.claude/plans/synthetic-prancing-aurora.md` (Phase 3,
full unit breakdown — read it before doing anything else if this is a cold start). This file is
updated at each unit-merge checkpoint so a fresh session (after a pause, e.g. a usage-limit reset)
can resume immediately without re-deriving state.

## Done and merged (master is green at these units) — 21 of 22 Phase 3a units

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
- **U11** — the shared postfix executor + final `Predicate` assembly, **the Phase 3a integration
  checkpoint** (`f2aa6ce`, current `master` HEAD). Two Opus-tier reviewers found the core
  postfix-evaluation math correct (confirmed via a 40+-predicate differential sweep against the
  real CLI, including cross-feature compositions no single earlier unit's tests exercised), but
  found and a fixer resolved: a FALSE documentation claim that Java's operand-token `act()` calls
  have no `Logging` side effects worth porting (they do — verified live, 6 of 10 expected log
  lines were missing; now honestly documented as a deferred gap needed before Phase 3b's golden-
  corpus unit, not before this unit, and pinned by a test asserting current behavior), plus a
  differential-test methodology bug that could have silently produced a wrong verdict on the first
  multi-track fixture (the port's raw track order vs. Walnut's always-`canonize()`'d fixture
  output — fixed and verified load-bearing), plus several weaker/tautological tests strengthened.

`cargo test --workspace` was green on `master` as of `f2aa6ce` (wr-cli 58, wr-core 444, wr-io 99,
wr-logic 270, wr-cts 22, differential 8, wr-core-integration-tests 2 — all passing). `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets` clean. **27**
genuine Walnut (Java) bugs found and logged so far (WB-001 through WB-027 — see
`docs/WALNUT-BUGS.md`; U11/U16 found no new genuine Java bugs, only pre-existing/deferred gaps,
correctly documented honestly rather than logged as Java bugs since they aren't Java defects).

**Process notes for whoever resumes:**
- **WB-number collisions remain routine when a unit adds new entries** — none this round (neither
  U16 nor U11 added any). Always grep `^## WB-` for duplicate/gapped numbers after ANY rebase
  touching `docs/WALNUT-BUGS.md`, conflict-reported or not.
- **U11's rebase had a real (non-WB) conflict** in `tests/differential/CAPTURE.md` — both U16 and
  U11 appended a new capture-recipe section to the same file, at the same insertion point. Trivial
  to resolve (both sections are independent prose, no actual content conflict) — just remove the
  three-way markers and keep both sections in sequence. Same "grep for markers, don't assume a
  reported conflict means real content conflict" discipline as the WB-number collisions.
- **A real coordinator process-hygiene lesson from the U16 round is worth restating**: after any
  sequence of commands that `cd`s into a worktree for inspection, explicitly `cd` back to the
  intended target directory before the next stateful command (`git add`/`commit`/`merge`) — a
  stale shell cwd from several tool calls back caused one misdirected (but non-destructive) commit
  attempt that round.
- Several agent runs stalled on a 600s stream-watchdog timeout (infra hiccup, not a task problem)
  — relaunching the same unit fresh has resolved it every time so far.

## In flight right now — the LAST Phase 3a unit

- **U15** — `EvalDef.java` (~185 LOC) as a `wr-cli` library function, the actual `eval`/`def`
  command implementation, wired to the real `Session` (U14) and calling `wr-logic`'s
  `evaluate`/`evaluate_with_logging` (U11). Needs to get right: Java's headless/non-headless split,
  the `TRUE`/`FALSE` print branch (now real via U0's trivial-automaton support), and the CAS
  matrix-export path per the plan's ALREADY-USER-DECIDED sign-off item #1 (write the automaton
  output normally, never produce matrix side-files, don't reach the DROP-scope CAS machinery at
  all). Sonnet tier, isolated subagent, worktree not yet known at this checkpoint (dispatched just
  before this file was last written — check `git worktree list` if resuming cold). Depends on U11
  (**done**) and U14 (**done**) — the last blocker just cleared, so this is the final unit standing
  between here and the **Phase 3a exit checkpoint**.

## After U15 lands: the Phase 3a exit checkpoint

Per the plan: extend `tests/differential` with `eval`/`def`/`reg` cases (literal strings via
`wr-cli`'s library API, using U0c's no-op strategy/export context) compared against real
`walnut-java` CLI output through `wr_core::equiv`. `cargo test --workspace` green throughout; every
genuine divergence logged to `docs/WALNUT-BUGS.md`, not silently resolved either way. This is
**the actual completion of Phase 3a** — a real, meaningful milestone (the full FOL decider's
engine + parser + CLI wiring, working end-to-end) worth flagging clearly to the user when reached,
distinct from Phase 3b (the remaining `wr-core` primitives, the real `Prover` dispatch/REPL, the
Tier-1 golden-corpus harness) which per `docs/ROADMAP-TO-AUTONOMY.md`'s phase-gating doctrine
still needs the user's explicit go-ahead before starting, same as Phase 3 itself did.

## Process notes for whoever resumes

- Follow the same loop used for every unit so far: author (worktree-isolated background agent) →
  two split-context adversarial reviewers (diff-only, model ≠ author — U11's author ran on the
  session's default/Sonnet tier per the plan's own tiering, so its reviewers used an explicit Opus
  override; check what tier U15's author actually runs on before picking reviewer models) → fixer
  if either review finds `correctness-fatal`/`correctness-risk` → verify (`cargo test
  --workspace`/`fmt`/`clippy`) → commit in the worktree → rebase onto current `master` → grep for
  WB-number duplicates/gaps AND check for non-WB content conflicts (e.g. `CAPTURE.md`) regardless
  of whether the rebase reported conflicts → fast-forward merge → `git worktree remove` +
  `git branch -d` to clean up.
- This is the home stretch of Phase 3a — U8/U14/U16/U11 all found real, fixable issues even in
  units that looked clean on first read. Don't relax review rigor for U15 just because it's last.
