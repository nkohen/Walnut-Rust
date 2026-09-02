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
| PAR-EXP | Three-lane parallelization experiment (**user-authorized 2026-09-02**, lifting the dispatch's no-parallelism stop for three EXPERIMENT branches — none mergeable under current freeze rules without a further decision) | **in flight.** Three Opus agents in isolated worktrees: `agent/par-max` (outcome-correctness only — semantic equiv + exact verdicts/errors; byte-identity pins may be `#[ignore]`d, never deleted), `agent/par-det` (full byte-identity preserved — DAG-level parallelism across independent eval subtrees + buffered-log stitching; full gate ladder must stay green), `agent/par-posthoc` (racy compute + post-hoc reconstruction of Java-equivalent artifacts via the existing canonicalize/write path; three-tally golden report). Coordinator runs the final sequential benchmark comparison of all three branches + HEAD control after they land — concurrent timings are garbage by construction. |
| P2 | Valmari `Partition` locality | **planned, NOT started — awaiting the user at the ranked-list boundary.** Plan (Opus-authored, honest-ceiling investigation) at `~/.claude/plans/perf-beyond-p2-valmari-locality.md`; needs its plan-review round before execution. Its own math: expected geomean **+1–2.5%**, at/barely above harness noise; its Gate 0 (proceed only if mark+split ≥50% of the minimize bucket) passes marginally on 286 (53%) and fails on 261/179/230 (29%/20%/11%). WB-001 verified FIXED on this branch (`860475a` in ancestry — the frozen contract is the fixed behavior). S-gate reality: zero minimize coverage in the snapshot suite; one real structural pin capped at q≤3 (`wb_001_exhaustive_small_sweep`'s digest); a P1(a)-style prep commit (verbatim reference of the whole Partition machinery + ≥20k bit-identity) is mandatory if executed. |
| P1(a) | `subset_construction` internals: C1 (member-outer/row-once bucket fill) + C2 (u64-epoch dedup-at-drain) | **done — SHIPPED per the pre-registered rule** (geomean +23.1% on the 9 engine-bound fixtures vs the ≥10% ship bar; worst delta 521 +0.5%, inside every band; all gates green). Plan v2 (12 plan-review findings folded in) at `~/.claude/plans/perf-beyond-p1a-subset-construction.md`. Prep commit `06fc85c` (9 S-gate snapshots — incl. the (g) discovery that dest ids ≥ q PANIC today, v2's prediction was wrong; DO-NOT-TOUCH determinize entry; baseline-header spread fix). Opus implementer: C1's density hypothesis REFUTED on dense fixtures (230/179 density ≈1.0 — C1 alone regresses 230; C2 supplies that win; 286's density 0.46 — C1 supplies that one), honest decomposition reported; verbatim reference copy + 20k-case bit-identity test + 6-way mutation matrix. Fable+Sonnet split-context review (own worktrees): ZERO correctness defects; both independently found the same release-blind dropped-`dedup()` test-gap (Sonnet proved a genuine wrong-automaton divergence q=3-vs-2 on a two-route fixture; generator can't emit dest≥q by design) — fixed by coordinator with the two-symbol fixture, mutation-verified failing in release; + `Fa::clear` doc mischaracterization, i32::MAX doc qualifier, generator-counter overcount, all fixed. Gates: workspace 51 suites; golden EXACT `675|670|1(383)|4|0`; diffgen 50,000/50,000 match 0 divergence (seed 0xbeef1a7e5eed2026; NOTE: WR_DIFFGEN_SEED needs a `0x` prefix for hex); wr-core green debug+release. A/B (3-run medians, same environment class as the blessed baseline — one pinned python core, load ~3.5): engine-bound set now **2.49×–4.09×** vs Java (230 4.09×, 637 3.41×, 179 3.29×); heavy rows alt3 4.92×/rsp1 3.95×/rsp2 4.02× (~17-20% over their P3 single-run numbers). Fixtures 1/207 informational: −3.1%/−1.8%. **Next: fresh post-C1/C2 profile to re-rank P2 (Valmari) vs C3 (hasher) vs C4 (row build).** |
| P3+P4 | Heavy opt-in bench rows + peak-state docs touch-up + Logging audit | **done.** Sonnet impl; Opus+Fable split-context review — both found the stale per-row "logging gap" mismatch arm; Fable proved `t-alt3`'s formula semantically trivial (language exactly `{n=0}`, jar-verified single-state automaton — the vacuous `Ej (j<t)` guard at `t=0`); Opus proved the claimed typo-catch was a hole (`Error==Error` passes `same_answer`) plus label-column overflow + tautological table test. Fixer (Sonnet) shipped the `(0<t)`-guarded formula (non-trivial 4-state language, full 326,396-peak computation — `j<=t`/`j<n` variants were tried and REJECTED for collapsing to fixture 230's exact 115,802-peak computation), a hard-fail guard for Error/None on non-fixture rows, the loud `peak MISMATCH` arm, ≤5-char labels (`alt3`/`rsp1`/`rsp2`), a strengthened table test (42/42), unified single-provenance numbers (2026-09-02: alt3 7.49 s JVM/1.76 s Rust, rsp1 12.95/3.82, rsp2 20.18/5.76 — heavy rows sit at 3.4–4.3×), and the STATUS.md editor's note. Fast-tier parse check documented as infeasible without a Session (item-6 fallback). Gates: fmt/clippy clean, workspace green (coordinator-run), full heavy run + default-path run green; golden/diffgen not run — diff is provably benches/-only (`git diff --stat -- crates/ tests/ fuzz/` empty), no engine surface touched. **Lever-5 audit finding (P4): `log_message(&format!(...))` formats eagerly before the enabled-check everywhere incl. hot paths, but all fmt frames total ~1% of real work — below the dispatch's 5% floor; recorded, not acted on.** |
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
