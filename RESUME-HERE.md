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

### Proposed units (post-U0, awaiting the user at the dispatch's re-baselining boundary)

Profile-derived ranking (evidence in `benches/baseline-perf-campaign.txt`'s profile section):
- **P1 — `subset_construction` internals** (dispatch lever 2; 59–89% of real work on all four
  profiled fixtures). Three sub-attacks, likely 2–3 units: (a) replace the per-(metastate,
  symbol) scratch-`Vec` `sort_unstable`+`dedup` with a reusable dense-marker/bitset union
  emitting the identical sorted-deduped sequence by construction (the sort alone is 34%/24% of
  230/179); (b) audit-then-swap the metastate `HashMap<Vec<usize>, usize>`'s SipHash for a
  faster hasher (8.3% on 230) — safe only if that map's iteration order is provably
  unobservable; (c) a function-LOCAL CSR snapshot of the input automaton's transitions for the
  duration of one `subset_construction` call (the "body" bucket, 47–60%, is dominated by
  per-member `d[state]` BTreeMap walks) — NOT the shelved Fa.d migration; builds per call,
  preserves ascending-symbol/insertion-order iteration. All S-gated + old-code-as-oracle
  structural probes (the U34 pattern), metastate discovery order provably identical.
- **P2 — Valmari `Partition` locality** (lever 3; 20.7% on 286, 12.2% on 261): layout/
  allocation-reuse only, algorithm+set-numbering frozen, S-gate mandatory.
- **P3 — heavier research-shaped opt-in workloads** (methodology rule 5): 2–4 `thm5`-class
  rows beside `sc637`, so >10× claims are made on research-shaped work, not only corpus-sized
  fixtures. Should land EARLY (before P1's A/B) to be part of the evidence base.
- **P4 — `Logging` zero-cost check + stale-docs touch-up** (lever 5 + the README/report
  peak-state caveat): small, foldable into any early unit.
- **NOT proposed: the Fa.d → TransitionRow migration** (lever 1) — 0.6–2.8% measured share;
  the dispatch's own gate ("IF a fresh profile shows Fa.d-rooted cost justifying it") fails.

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
