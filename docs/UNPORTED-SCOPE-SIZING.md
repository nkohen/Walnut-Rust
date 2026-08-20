# Sizing the unported/dropped scope

DESIGN.md §3 drops Ostrowski/Fibonacci/Pell/negative-base numeration, CAS matrix exports, and
(per §10/§9 F3-F4) the OTF determinization family from the port. This note sizes what porting
each of those would actually cost, using the same process this project already follows for every
other unit: Phase 0 (drive/extend Java characterization-test coverage first) → mechanical port →
two-independent-adversarial-reviewer loop → Tier 1-4 integration (golden corpus, differential,
property tests). It exists so a future "should we port X" decision starts from real sizing data
instead of the original DROP list's one-line rationale.

Produced 2026-08-19 by a research agent reading `../walnut-java` (the oracle repo) directly —
`Ostrowski.java`/`NodeState.java`/`Ost.java`, `NumberSystem.java`'s `isNeg`-gated branches,
`Split.java`, the CAS `Writer/` classes, and `DeterminizationStrategies.java`'s OTF branches —
plus `docs/BOUNDARY-MAP.md` and `walnut-java/phase0-artifacts/`.

## Correcting a likely misreading of the DROP list

**Fibonacci and Pell numeration were never separate ported features and are not a gap.** There is
no native `Fibonacci`/`Pell` Java class anywhere in `walnut-java/src/main/java/` (grepped, zero
hits for either). `msd_fib`, `msd_pell`, etc. are ordinary user-supplied custom-base data files
that run through the generic custom-base loader Phase 3a already shipped
(`crates/wr-core/src/numsys.rs`) — already exercised by `tests/differential/tests/
lsd_custom_base.rs` and `phase3a_checkpoint`'s `fib_cmp`/`fib_reg`. DESIGN.md §3/§88's "Fibonacci/
Pell branches of NumberSystem" phrasing is accurate about *negative*-base branches but doesn't
apply to Fibonacci/Pell at all — nothing to port there.

The real dropped-item list is: CAS matrix export, negative-base numeration, `split`/`rsplit`,
Ostrowski, and the OTF determinization family.

## Ranking (smallest → largest task)

### 1. CAS matrix export — smallest

- **Java surface**: `Writer/AutomatonMatrixWriter.java` (188 LOC) + `MatrixEmitter.java` (26 LOC)
  + 4 format emitters (Maple 105, Sage 104, Matlab 105, Mathematica 102) ≈ **630 LOC total**,
  sole caller `EvalDef.writeMatrices`.
- **Complexity**: mechanical — walk the transition-incidence matrix, print in one of four text
  formats. No decision-procedure involvement; a pure writer, not trust-critical math.
- **Phase-0 status**: no dedicated JaCoCo coverage bucket yet, but ~28 golden fixtures already
  exercise it, giving a ready-made real-output oracle.
- **Review weight**: light — same tier as the `wr-io` line-splitting fix (item 5 of
  `BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md`), not the full trust-critical two-reviewer gate,
  since it doesn't touch `wr-core`/`wr-logic` decision-procedure code.

### 2. Ostrowski numeration

- **Java surface**: `Numeration/Ostrowski.java` (492 LOC) + `NodeState.java` (46 LOC) +
  `Commands/Ost.java` (25 LOC) ≈ **563 LOC**.
- **Complexity**: genuinely novel — continued-fraction-based numeration, structurally unrelated
  to base-*k* `NumberSystem` (`BOUNDARY-MAP.md` confirms zero shared logic). But it's cleanly
  additive: a wholly new module/command, no changes to existing hardened code paths, so it can't
  regress anything already shipped.
- **Phase-0 status**: `OstrowskiTest.java` (166 lines) exists but was deliberately not extended
  toward the ~100% bar since it was DROP-scope — real Phase-0 work needed before a port could
  start.
- **Demand signal**: thin — only 1 golden fixture (`drop_command:ost`) references it.

### 3. Negative-base numeration + `split`/`rsplit` — bundle these, they're one project

- **Java surface**: negative-base is not a separate file — ~100-150 LOC of `isNeg`-gated branches
  spread across `setLessThanAutomaton`/`setBaseChangeAutomaton`/`arithmetic()` in
  `NumberSystem.java`. `Split.java` (123 LOC, one handler, `isReverse` flag distinguishes
  `split`/`rsplit`) exists *solely* to convert into/out of negative-base representations, so it
  can't be ported independently of negative-base.
- **Complexity**: individually small by LOC, but negative-base was **deleted outright, not
  stubbed**, from `wr-core::numsys` (`BOUNDARY-MAP.md` §4.1, user decision 2026-08-08) — and that
  file has since been called out as "the largest, hardest-reasoning file" in the port (Phase 2
  U7) and built on top of across three more phases of property tests, differential generators,
  the golden corpus, and fuzz corpus, all of which currently assume no negative bases exist.
  Re-adding it means re-threading sign-handling through already-hardened call sites and
  re-validating that whole stack, not just adding ~150 lines.
- **Phase-0 status**: was covered pre-deletion (existing Java tests), so less fresh Phase-0 work
  than Ostrowski, but the re-integration surface in Rust is wider.
- **Demand signal**: the largest of any dropped item by fixture count — `msd_neg_2`=39,
  `msd_neg_fib`=14, `lsd_neg_fib`=11, `lsd_neg_2`=3, `msd_neg_10`=1 (68 fixtures) plus
  `split`=8/`rsplit`=7 (15 fixtures) ≈ **83 fixtures total**.

### 4. OTF determinization family (CCL/CCLS/BRZ_CCL/BRZ_CCLS/`OTF()`) — largest by a wide margin

- **Java surface**: branches within `DeterminizationStrategies.java`'s 326 LOC, plus an external,
  unvendored `io.github.jn1z:otf` Maven dependency (no local LOC visible — the real cost is
  outside this repo entirely).
- **Complexity**: very high — an unproven TACAS-2026 algorithm (antichain/simulation-relation
  machinery), no analogue anywhere in the current port. `BOUNDARY-MAP.md` and DESIGN.md §9/§10
  already size a full port at **~4,000-5,200 LOC**, with no viable smaller cut under
  ~2,000-3,000 LOC.
- **Phase-0 status**: resolved empirically, not by coverage — Phase 0 Item 7 ran a real-workload
  check and found no case in the actual golden corpus requires non-`SC`/`BRZ` strategies to
  terminate.
- **Demand signal**: zero — 0 fixtures require it, and the deferral decision (DESIGN.md §10) is
  already confirmed against real data, not just deferred for lack of time.
- **Recommendation**: this is the one item where the honest read is "don't," not "rank it
  low" — large, novel, externally-dependent, and the demand signal is a documented zero. Revisit
  only if a future research workload actually produces a query `SC`/`BRZ` can't finish.

## If picking one up next

CAS export is the easy win if there's a concrete research need for it. Ostrowski is the
interesting-but-isolated option. Negative-base+`split`/`rsplit` is the highest-value/highest-risk
pair (largest fixture footprint, but touches already-hardened trust-critical code). OTF should
stay deferred absent a new workload that demonstrably needs it.
