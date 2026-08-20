# Dispatch prompt: port CAS matrix export

**Status: RESOLVED (2026-08-19).** Executed via `.claude/plans/amber-transcribing-ledger.md`
(adversarially reviewed before execution; the plan's own review found and fixed six real gaps
in the first draft). `crates/wr-io/src/matrix_writer.rs` ports `AutomatonMatrixWriter` + the
four emitters; `crates/wr-cli/src/eval_def.rs` wires it in for real; the golden-corpus harness
now compares all 7 matrix fixtures (374-379, 383) in full — all pass on all four extensions.
WB-042 (`MathematicaEmitter`'s `#`-vs-`(* *)` comment bug) logged and ported verbatim; WB-007's
status line updated to "reached." Sizing rationale (now historical) is in
[`docs/UNPORTED-SCOPE-SIZING.md`](UNPORTED-SCOPE-SIZING.md) (ranked #1, smallest of the
then-currently-dropped items). The original dispatch prompt below is kept for the record, not
because it is still an open task.

---

## Prompt

You're picking up a new unit of work on **walnut-rs**, a Rust port of a research subset of the
Walnut theorem prover. Before anything else, read `CLAUDE.md` in full — it is this project's
operating contract (correctness ladder, mechanical-port-first rule, the adversarial-review loop,
git/commit discipline, token-efficiency practices) and everything below assumes you're following
it. Also read `docs/DESIGN.md` (the overall plan) and `PORTING.md` (the Java→Rust idiom map).

**The task**: port Walnut's CAS matrix export feature — currently dropped scope. When a real
(interactive, non-headless) `eval`/`def` command finishes, Java's `EvalDef.compute()` calls
`writeMatrices(M, freeVarStr, resultName)`, which writes the resulting automaton's transition
matrix out in four computer-algebra-system formats via `Automata/Writer/AutomatonMatrixWriter.java`
(188 LOC) + `MatrixEmitter.java` (26 LOC) + one emitter class each for Maple (105 LOC), Sage
(104 LOC), Matlab (105 LOC), and Mathematica (102 LOC) — ~630 LOC total in `../walnut-java`
(the oracle repo, a sibling directory to this one). Read those six files directly before planning
anything; do not guess their behavior from names.

Full sizing context — Java LOC, complexity assessment, existing fixture count (~28 golden files
reference CAS export), and why this ranked as the smallest of the currently-dropped items — is in
[`docs/UNPORTED-SCOPE-SIZING.md`](UNPORTED-SCOPE-SIZING.md). Read it before starting; don't
re-derive what's already there.

### What to actually do, in order

1. **Phase 0 first, per this project's own rule.** Check current Java coverage on the six files
   above (`../walnut-java`'s JaCoCo setup, `phase0-artifacts/`). The sizing note found no
   dedicated coverage bucket for this feature yet. If coverage is thin, drive it up in
   `walnut-java` first (characterization tests against real Walnut output) *before* porting — the
   whole point of Tier 0 is that the Java tests are the executable spec you port against, not
   something you infer from reading code once.

2. **Resolve the headless-vs-interactive question before writing any Rust.** Java's
   `EvalDef.compute()` (interactive path, calls `writeMatrices`) and `computeHeadless()`
   (used by integration tests, does *not* call `writeMatrices`) diverge exactly here — confirmed
   by reading `EvalDef.java` directly (lines ~55-90). Figure out: does `wr-cli`'s real dispatch
   path (the one actual users hit) correspond to Java's interactive `compute`, and does the
   existing Tier-1 golden-corpus harness (`tests/golden/`, which uses
   `Prover::dispatch_for_integration_test` — check whether that's wired to the headless or
   interactive equivalent) currently exercise this feature at all, or silently skip it? If it's
   silently skipped, the harness needs a real extension (a headless variant of `computeHeadless`
   won't do — you need the interactive path's behavior specifically), not a workaround. This is
   the one genuinely open architectural question in this unit; don't paper over it.

3. **Write a plan** (this project's convention for a unit this size: a plan file, adversarially
   reviewed by an independent agent *before* any code lands — see the Phase 2/3/4 plans referenced
   throughout `CLAUDE.md`'s "Current status" section for the pattern). The plan should cover:
   - Target crate: almost certainly `wr-io` (this is pure output-formatting, no decision-procedure
     math — it mirrors the existing `.txt`/`.gv`/`.ba` writer in shape, not `wr-core`/`wr-logic`).
   - The wiring point in `wr-cli` (where Java's `EvalDef.compute()` calls `writeMatrices` — find
     and use the real equivalent, not a new ad hoc hook).
   - Answer to the headless-vs-interactive question from step 2, and what it means for golden-
     corpus coverage.
   - The four emitters + `AutomatonMatrixWriter`'s `EMITTERS`/`writeAll` dispatch structure —
     mechanical port, preserve exact output formatting (whitespace, ordering) since these are
     text-comparison-tested via the golden corpus, not equivalence-tested like automata.
   - Test plan: golden-corpus comparison against the ~28 existing fixtures (add coverage if step 2
     found a harness gap), plus new Rust unit tests per emitter.

4. **Execute the plan** through the implementer → review → fixer loop. This feature does **not**
   touch `wr-core`/`wr-logic` (the trust-critical crates `CLAUDE.md`'s merge gate names), so it
   does not require the full two-independent-adversarial-reviewer gate that math/decision-
   procedure code needs — the lighter single-reviewer treatment this project used for the
   `wr-io` lone-`\r` line-splitting fix (see "Item 5" in `CLAUDE.md`'s history) is the right
   precedent, *unless* step 2 turns up wiring changes in `wr-cli`'s real command dispatch path
   that could regress already-shipped `eval`/`def` behavior — if so, treat that specific wiring
   change with the same care as any other dispatch-path change and get a second look on it.

5. **If you find a genuine Walnut (Java) bug** while reading/porting these six files — not a
   quirk, an actual wrong-output or crash-on-plausible-input defect — log it in
   `docs/WALNUT-BUGS.md` per `CLAUDE.md`'s rule and port it verbatim; do not silently fix or
   silently replicate it without logging.

6. **Merge gate**: `cargo test --workspace` green, `cargo fmt --all`/`cargo clippy --workspace
   --all-targets` clean, before considering this done. Do not delete any test. Do not commit
   without the user's explicit go-ahead (this project's standing git-hygiene rule) — leave the
   work staged/described and say so.

Report back with: what you found in step 2 (the headless/interactive question) and how it
resolved, the plan's adversarial-review outcome, final golden-corpus pass count, and any
`WALNUT-BUGS.md` entries added.
