# RESUME-HERE — Performance campaign on `perf/beyond` (docs/PERF-CAMPAIGN-DISPATCH.md)

**Reconciled 2026-09-02 (campaign U0).** This file's previous contents were the idiomatic-refactor
closeout (Phases 0–3 complete on `refactor/idiomatic`, Phase R4 — the `Fa.d` → `TransitionRow`
migration — declined by the user at the boundary). That narrative is preserved in `CLAUDE.md`'s
"Current status" section, the git log, and commit `cc64579`'s version of this file; it is not
duplicated here. The R4 design remains on the shelf at
`~/.claude/plans/glossy-compacting-lantern.md` and is explicitly a ranked candidate (lever 1) for
THIS campaign, gated on a fresh profile.

## Current work: the performance campaign

- **Dispatch:** `docs/PERF-CAMPAIGN-DISPATCH.md` — read it in full; every rule binds
  (frozen observable behavior incl. `::`-details text and state numbering, frozen
  `wr_core::equiv`/`wr-cts` oracles, no `unsafe` without a dedicated unit, no parallelism without
  asking, full two-adversarial-reviewer loop per unit, pre-registered decision rules, fidelity
  gates per unit).
- **Guardrails:** `docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md` binds every unit verbatim.
- **Branch:** `perf/beyond`, cut from `refactor/idiomatic` @ `cc64579`. Never rebased. **Nothing
  is pushed without the user's explicit go-ahead.**
- **Goal:** >10× vs JVM Walnut on most workloads as the aspiration that directs effort — never a
  number to manufacture. Honest ceilings, stated plainly, are a valid deliverable.
- **Oracle:** sibling `walnut-java` at `14509f1` (`bugfix/wb-001`), fat jar
  `target/Walnut-all.jar` (2026-08-22 build — same jar the refactor-U0 baseline used).

### Unit ledger

| Unit | Scope | Status |
|---|---|---|
| U0 | Branch, dispatch committed, ledger reconciliation, campaign re-baseline | **done.** Branch cut at `cc64579`; docs landed. Baseline took THREE attempts: 1 (battery 16% + two 99%-CPU processes, load 10–17) and 2 (AC but concurrent Claude/Lean sessions, load 7→34) both harness-green yet **discarded** — swings up to 19× on identical code; attempt 3 (load 2.5–5.8, one pinned python core, otherwise quiet) **blessed**: Rust medians tight to ~2% (worst 7%), reproduces the historical quiet-machine tables. Campaign baseline table in `benches/baseline-perf-campaign.txt`: engine-bound fixtures **2.18×–3.45×** vs Java (230: 3.45×, 286: 2.29×, 295: 2.18×). >10× aspiration ⇒ ~3–4× more off the port's own times. Check machine fitness before EVERY bench/profile run (attempts 1–2 are the cautionary appendix). |

### Findings so far (campaign)

- The port's `::`-details trace now names the same peak state count as Java's on **all 11**
  bench fixtures (U28's Logging threading closed the gap) — `benches/README.md`'s "the port's
  trace is a lower bound" caveat and the compare report's banner are stale; fold a docs/report
  touch-up into an early unit rather than leaving the report mislabeling its own column.

## Recovery procedure

Read this file + `docs/PERF-CAMPAIGN-DISPATCH.md` + `git log --oneline refactor/idiomatic..HEAD`
on `perf/beyond`. Every landed commit must be independently green (T0 minimum). The unit ledger
says what's next. Before ANY bench/profile work: check machine load and power state first (see
U0's row above — attempt 1 was burned by skipping that check).

## Process note (standing default, inherited from the refactor)

Two reviewers per round, different models from the author and each other, split context, given
only the diff — never the author's rationale. Reviewers are read-only in the shared tree
(concurrent-reviewer mutation hazard, see CLAUDE.md's fleet-hygiene section). One implementer at
a time in the shared tree.
