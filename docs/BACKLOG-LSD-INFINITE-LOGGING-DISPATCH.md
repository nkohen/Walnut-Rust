# Kickoff prompt: `I`-over-`lsd`, custom-base `lsd` verification, non-`eval`/`def` `Logging` wiring

**Status:** ready to hand to a fresh agent. Paste the block below (between the `---PROMPT START---` and
`---PROMPT END---` markers) as the first message of a new session. Recommended working directory:
`~/dev/walnut-rs` (this is Rust-side work; `~/dev/walnut-java` is read-only reference/oracle throughout).

This session mixes two very different kinds of work. Item 1 (`I`-over-`lsd`) is genuine quantifier-elimination
correctness work — CLAUDE.md's model-tiering doctrine reserves this class of problem for the hard ~20%, so run
the session on a strong tier (Opus) for its sake, since a mid-session model switch invalidates the prompt cache
and re-sends everything uncached. Item 3 (`Logging` wiring across CLI commands) is comparatively mechanical —
if you want the cheaper-per-token rate for it, delegate it wholesale to a fresh `Agent` subagent with a `model`
override rather than downgrading the whole session, per CLAUDE.md's "escalate/delegate in batches" guidance.

---PROMPT START---

You are picking up four items from `walnut-rs`'s tracked backlog. You have no memory of any prior
conversation — everything you need is either in this prompt or in the repo. Read before doing anything else,
in this order:

1. `CLAUDE.md` — the full operating doctrine (Prime Directive, correctness ladder, mechanical-port-first rule,
   the `WALNUT-BUGS.md` logging discipline, the two-independent-adversarial-reviewer merge gate for
   `wr-core`/`wr-logic`, model tiering, token efficiency, git hygiene). Everything below is governed by it.
2. `PORTING.md` — the Java→Rust idiom map. If you hit a pattern it doesn't cover, add the ruling there before
   porting the third occurrence.
3. `docs/DESIGN.md` §5 (the correctness ladder — Tier 1 golden corpus, Tier 3 differential, Tier 4 property
   tests) and §3 (scope: `lsd_k` is KEEP, custom bases are KEEP, Ostrowski/Fibonacci-as-a-NEGATIVE-base is
   DROP — `msd_fib`/`lsd_fib` themselves are ordinary positive custom bases and are NOT the dropped surface,
   don't confuse the two).
4. `tests/golden/STATUS.md` and `tests/golden/tests/golden_corpus.rs`'s `KNOWN_DIVERGENCES` — the current,
   accurate state of Tier 1 (585/586 passing as of 2026-08-19; fixture 383 is a known, deliberately-left
   exception unrelated to anything in this dispatch — don't touch it).

## Where things stand (verified when this prompt was written — 2026-08-19; re-check anything that looks stale)

- **`crates/wr-core/src/infinite.rs`**'s `pub fn infinite(a: &Automaton) -> Result<Option<String>, InfiniteError>`
  is the whole port of `Automata/FA/Infinite.java`. Its own module docs say it backs BOTH the `inf` command
  AND the `I` (infinitely-often) quantifier's pipeline in Java's `LogicalOperator.actQuantifier`
  (`AutomatonLogicalOps.removeLeadingZeros` + `Infinite.infinite`). It never calls `wr_core::quantify::quantify`
  — it is a structurally different algorithm (a `prefix · cycle* · suffix` witness search over the trimmed
  automaton graph), not projection+determinize.
- The msd/lsd asymmetry that mattered for Phase 3b's L1 unit (closing `∃`/closed-`∀` over `lsd_k`) was: msd
  needs `removeLeadingZeros` (closure under prepending zeros), lsd needs the DIFFERENT
  `fixTrailingZerosProblem`/right-quotient fixup — genuinely not mirror images of each other, both ported and
  both property-tested (see `wr_core::quantify`'s module docs and
  `msd_and_lsd_composed_constructions_agree_after_reversal`). **Check whether `actQuantifier`'s `I` branch
  (`wr-logic`'s `LogicalOperator`, wherever it dispatches the `I` quantifier — grep for it) applies the SAME
  `removeLeadingZeros`-shaped fixup unconditionally regardless of msd/lsd**, the way `quantify`'s lsd branch
  used to before L1. If so, that is very likely the actual gap — the same shape of bug L1 fixed, in a sibling
  code path L1 never touched. Do not assume this is the bug; verify it by reading the real Java source
  (`Automata/FA/Infinite.java`, `Main/EvalComputations/Token/LogicalOperator.java`'s `actQuantifier`) and by
  constructing a small `lsd_2` (or `lsd_3`) query using `I` and comparing against a real `walnut-java` CLI run
  before writing any fix.
- **Tier-3 coverage gap, not just Tier-1**: U29's differential-generator harness (`tests/differential-gen/`)
  explicitly never emits the `I` quantifier (documented in its own "Known, explicit non-coverage" section) —
  so there is close to zero automated coverage of `I` at all right now, `lsd` or otherwise. Closing item 1
  correctly will likely require adding NEW differential and/or property coverage for `I`, not just fixing one
  bug and hoping the existing suite catches it — the existing suite mostly can't see this code path.
- **Custom-base `lsd` (item 2, e.g. `lsd_fib`) construction is already correct and tested** —
  `crates/wr-core/src/numsys.rs`'s
  `lsd_custom_base_built_from_the_msd_files_reverses_exactly_once` pins that a custom base supplied only with
  `msd_*` files (as the reverse-direction complement candidates) builds a correct `lsd_*` adder. The gap is
  END-TO-END: does `eval`/`def` over `?lsd_fib` (or another real custom base) compose correctly through
  quantifiers (`∃`, closed `∀`, and — once item 1 lands — `I`)? This has never been exercised; L1's own
  positive coverage was `lsd_k` (plain bases) only, explicitly not custom bases (see `wr_logic::eval`'s module
  docs, the L1 section, "Scope of what RESOLVED actually covers").
- **Item 3 — non-`eval`/`def` commands still pass a throwaway `Logging::new()` instead of the real session's
  logging**, so `::`-suffixed detail output is silently empty/wrong for these even though the underlying
  primitives now support it (post-U28). Confirmed, current, real call sites (not test code — these are inside
  the actual command handlers):
  - `crates/wr-cli/src/quotient.rs:120` (`right_quotient`) and `:147` (`left_quotient`)
  - `crates/wr-cli/src/convert.rs:132`
  - `crates/wr-cli/src/automaton_ops.rs:279`, `:340`
  - `crates/wr-cli/src/simple_transforms.rs:120` (`fix_trailing_zeros_problem`)
  - `crates/wr-cli/src/alphabet.rs:332`, `:445`
  Grep `Logging::new()` across `crates/wr-cli/src/` yourself for the authoritative, current list — this one
  may have shifted. Compare each command's real Java source (`Main/Prover.java`'s dispatch,
  `Commands/*.java`) for whether it's even supposed to print details at all (some commands genuinely don't;
  don't wire logging into ones Java itself never threads it through) before changing anything.
- **Item 5 (only if it turns out easy — see below) — `crates/wr-io/src/reader.rs`'s own module docs
  (search for "Known remaining gap: line SPLITTING")** already document this precisely: both readers split on
  `str::lines()` (breaks on `\n` only), while Java's `BufferedReader.readLine()` treats `\n`, `\r`, AND `\r\n`
  each as a terminator, so a lone-`\r`-delimited file reads as one giant line here instead of several. **The
  same doc comment already states the honest cost of fixing it: replacing `lines()` with a hand-rolled
  splitter in BOTH readers, plus every line-number computation that hangs off them** — this reads like a
  multi-file, careful-but-mechanical change, not a one-line fix. Treat "if it is easy" literally: spend a
  time-boxed pass (recommend: no more than 30-45 minutes of investigation) confirming or refuting that
  estimate for yourself by reading both readers' line-number tracking, and if it's genuinely as contained as
  the doc suggests, do it (mechanical-port discipline: match `BufferedReader.readLine()`'s three-terminator
  behavior exactly, don't invent a fourth). If it turns out bigger (e.g. line-number tracking is threaded
  through many more call sites than expected, or fixing it risks the fuzz corpus / existing golden fixtures),
  stop, report why, and leave `docs/WALNUT-BUGS.md`/the existing doc comment as-is rather than grinding on it —
  this item is explicitly lowest priority and was flagged as likely-not-easy when this prompt was written.

## Your mission, in priority order

1. **`I`-over-`lsd`** (the hard one — this is why the session should run on a strong model tier).
   a. Read the real Java source for `Infinite.infinite` and `LogicalOperator.actQuantifier`'s `I` branch.
      Determine precisely what fixup (if any) Java applies before/after calling `infinite()`, and whether it
      is msd/lsd-aware.
   b. Reproduce the current Rust behavior on a small `lsd_2`/`lsd_3` `I`-quantifier query and compare against
      a real `walnut-java` CLI run (the `phase0-artifacts/CAPTURE.md` convention other units used for exactly
      this). Confirm there IS a divergence before fixing anything — don't assume the backlog note was
      precisely correct about the mechanism.
   c. Port the fix mechanically, per CLAUDE.md's rules — if you find a genuine Walnut (Java) bug along the way
      (not just a port gap), log it in `docs/WALNUT-BUGS.md` per the established format and port it verbatim,
      don't silently fix or silently replicate it without logging.
   d. Add real coverage: unit tests in `wr-core`/`wr-logic` for the fixed code path, AND new differential
      and/or golden coverage exercising `I` over both `msd` and `lsd`, since the existing Tier-3 generator
      doesn't emit `I` queries at all. Don't rely on "the existing suite went green" as evidence — check what
      it actually exercises.
   e. This touches `wr-core`/`wr-logic` (trust-critical crates) — it MUST go through the full
      implementer → two-independent-adversarial-reviewer (`.claude/agents/adversarial-reviewer.md`, split
      context — diff only, no rationale, reviewer model ≠ author model) → fixer loop before you consider it
      done, per CLAUDE.md's hard merge gate.

2. **Custom-base `lsd` verification** (naturally sequenced after 1, since you'll want `I`-over-`lsd` working
   before claiming end-to-end custom-`lsd` coverage is complete).
   a. Build differential and/or golden coverage exercising a real custom base's `lsd` direction (`lsd_fib` is
      the natural choice — the files already exist in `walnut-java`'s `Custom Bases/`) through `eval`/`def`
      with `∃`, closed `∀`, and `I` (once 1 is fixed).
   b. If you find a genuine divergence, treat it with the same rigor as item 1 (log genuine Walnut bugs,
      don't silently patch either engine, two-reviewer loop for any `wr-core`/`wr-logic` fix).
   c. If everything already composes correctly (plausible — L1's fix and the addition/quantifier primitives
      are general, not `lsd_k`-specific), the deliverable is the new coverage itself plus an explicit note
      updating `wr_logic::eval`'s module docs (the L1 section) that custom-base `lsd` is now verified, not
      just `lsd_k`.

3. **Wire real `Logging` into the non-`eval`/`def` commands** (the mechanical one — good candidate to
   delegate to a fresh `Agent` subagent with a cheaper `model` override if the session is running on Opus for
   item 1's sake).
   a. Enumerate every command handler currently constructing a throwaway `wr_core::logging::Logging::new()`
      instead of threading the real `Prover`'s own logging (start from the grep above, but re-derive the list
      yourself — it may have moved). For each, check the real Java command handler to confirm it's actually
      supposed to support `::`-suffixed detail printing before wiring it up.
   b. Thread the real `&mut Logging` through, mirroring exactly how U28 already did this for `eval`/`def`
      (see `crates/wr-logic/src/eval.rs`'s module docs for the account of that unit, and
      `crates/wr-cli/src/session.rs`'s `PredicateEnv::number_system`/`fresh_number_system` for the pattern of
      "pass the real session logging down, don't construct a fresh one").
   c. Add or extend golden-corpus / differential coverage for at least one `::`-suffixed non-`eval`/`def`
      command per command family (`union`/`intersect`/`quotient`/`convert`/etc.) to prove the wiring actually
      produces real detail text, not just that it compiles. Check whether the existing corpus already has
      such fixtures before assuming you need to invent new ones from scratch.
   d. This also touches `wr-core`/`wr-cli` — apply the same merge-gate discipline as item 1 for anything that
      touches `wr-core` specifically (the CLI-layer wiring itself is lower-risk and doesn't strictly require
      it, but don't skip review on any `wr-core` change this surfaces).

5. **Only if step 5's easy-or-not assessment above comes back genuinely easy**, close the lone-`\r`
   line-splitting gap in `crates/wr-io/src/reader.rs`. Otherwise explicitly report why it isn't easy and stop
   there — do not let this item expand scope or eat the budget items 1-3 need.

## How to work (per CLAUDE.md — not optional)

- **Mechanical-port-first.** Preserve Walnut's behavior including quirks; log genuine Java bugs to
  `docs/WALNUT-BUGS.md` rather than silently fixing or silently replicating them without a paper trail.
- **The merge gate is a hard rule.** Never commit with `cargo test --workspace` red. Never merge
  `wr-core`/`wr-logic` changes without the two-independent-adversarial-reviewer loop actually running — split
  context (diff + file paths only, never your own rationale/commit message), reviewer model different from
  yours. Zero tests deleted, ever.
- **Mutation-verify anything you claim a regression test catches** — revert your fix, confirm the new test
  fails, reapply. This project's own history (see `CLAUDE.md`'s U31, and this session's own construction-
  recording work) has repeatedly found tests that looked like they covered something but didn't; don't add to
  that pile.
- **Token efficiency**: don't read a large Java or Rust source file wholesale — grep/locate, read the slice.
  Route long output (test logs, diffs, capture sessions) to disk. Delegate heavy-in/small-out work to a fresh
  subagent and keep only the conclusion in your own context.
- **Phase-gated, not unattended.** Stop and report after each numbered item, or sooner if you hit something
  needing a human call (an ambiguous scope question, a discovered bug significant enough to need a
  replicate-vs-fix decision the user should make explicitly, or item 1 turning out to need a design bigger
  than a single-unit fix).
- **Update the record.** When each item lands, add its account to `CLAUDE.md`'s "Current status" section in
  the same style as the existing entries (what was done, what was found, commit hashes), and update
  `tests/golden/STATUS.md`/`docs/WALNUT-BUGS.md` if either changed.
- **Commit discipline**: atomic, pathspec-scoped commits (never `git add -A`), one logical change per commit.
  Never run `git checkout`/`reset`/`stash` without running `git status` first and being certain what you'd be
  discarding — if genuinely unsure, stop and ask rather than guessing. Commit/push only on explicit user
  request unless the user has told you otherwise for this session.

## Definition of done for this dispatch

Not "all four items closed" in one unattended pass — items 1 and 2 in particular may reveal more than
expected (L1's own experience: the blast radius was "substantially wider than framing" going in). Done for a
given checkpoint means: the item is genuinely finished (verified against real Walnut, tested, reviewed if it
touched `wr-core`/`wr-logic`) OR you've hit a wall worth reporting, `cargo test --workspace` is green,
`CLAUDE.md`/`STATUS.md` reflect what happened, and you've stopped to report rather than continuing unattended
into the next item without the user's go-ahead.

---PROMPT END---
