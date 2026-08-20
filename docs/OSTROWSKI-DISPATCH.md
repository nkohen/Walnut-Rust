# Dispatch prompt: port Ostrowski numeration

Status: **DONE (2026-08-20).** Executed via the plan at `~/.claude/plans/dusky-braiding-compass.md`
(v2, itself adversarially reviewed before execution). Landed:
`crates/wr-core/src/ostrowski.rs` (the `NodeState`/adder-table/BFS/`Ostrowski` port),
`crates/wr-cli/src/ost.rs` (the `Ost.ostCommand` wiring, the `Custom Bases/` write + exists-guard,
and the Tier-2 byte-for-byte replication of `OstrowskiTest.java`'s eight `testAgainstFile` cases),
the real `OST` dispatch arm in `crates/wr-cli/src/prover.rs`, a new differential suite
(`tests/differential/tests/ostrowski.rs`) against fresh `walnut-java` captures, a Tier-4
property suite built on an independent from-the-continued-fraction place-value oracle, and the
golden-corpus two-pair `Expected`/comparator branch that lets fixture 625 (the corpus's only
`ost` fixture, and its only two-automaton-pair fixture) be compared — **it passes**; corpus went
from 586 compared / 585 pass to 587 compared / 586 pass, with the single pre-existing 383
divergence unchanged. No new `docs/WALNUT-BUGS.md` entry was warranted (highest remains WB-042):
nothing found in `Ostrowski.java` produced wrong output or crashed on a plausible input.

The prompt below is preserved as the record of what was asked for.

Sizing rationale is in [`docs/UNPORTED-SCOPE-SIZING.md`](UNPORTED-SCOPE-SIZING.md) (ranked #2 —
CAS matrix export, ranked #1, is now done; see that doc's item 1 and
[`docs/CAS-EXPORT-DISPATCH.md`](CAS-EXPORT-DISPATCH.md) for the precedent this unit should follow
procedurally, adjusted for the differences below).

---

## Prompt

You're picking up a new unit of work on **walnut-rs**, a Rust port of a research subset of the
Walnut theorem prover. Before anything else, read `CLAUDE.md` in full — it is this project's
operating contract (correctness ladder, mechanical-port-first rule, the adversarial-review loop,
git/commit discipline, token-efficiency practices) and everything below assumes you're following
it. Also read `docs/DESIGN.md` (the overall plan), `PORTING.md` (the Java→Rust idiom map), and
`docs/UNPORTED-SCOPE-SIZING.md`'s Ostrowski section — don't re-derive what's already there.

**The task**: port Walnut's `ost` command and the Ostrowski numeration system it builds —
currently dropped scope. The Java surface is small and self-contained: `Automata/Numeration/
Ostrowski.java` (492 LOC) + `Automata/Numeration/NodeState.java` (46 LOC, a BFS-state key type) +
`Main/Commands/Ost.java` (25 LOC, the command handler) in `../walnut-java` (the oracle repo, a
sibling directory to this one). Read all three files directly before planning anything — the
notes below are load-bearing context from reading them, not a substitute for reading them
yourself.

### What reading the Java source already established (verify, don't re-derive)

- **`ost <name> <preperiod> <period>` builds a continued-fraction numeration system via BFS**
  (`performReprBfs`/`performAdderBfs` in `Ostrowski.java`) and writes out exactly two automaton
  files: a digit-representation automaton (`msd_<name>.txt`) and an addition/adder automaton
  (`msd_<name>_addition.txt`) — see `Ost.ostCommand` and `NumberSystem.MSD_UNDERSCORE`/
  `UNDERSCORE_ADDITION_AUTOMATON`. **This is the same file-naming convention the already-ported
  generic custom-base loader (`crates/wr-core/src/numsys.rs`, shipped in Phase 3a) already
  consumes for `msd_fib`/`msd_pell`/etc.** So the scope here is narrower than it first looks:
  you're porting the *construction* algorithm (BFS over `(preperiod, period)` → two `Automaton`s),
  not a new numeration-system-consumption path — once the two files exist, the existing
  custom-base machinery should be able to read them back with **no changes**. Confirm this by
  hand-tracing one small `ost` case end-to-end in real `walnut-java` (generate the files, then
  feed the resulting `msd_<name>` base into an already-supported command like a comparison query)
  before assuming it, and call it out explicitly in your plan if it turns out not to hold.
- **It's genuinely isolated, not just claimed to be.** The one other reference to `Ostrowski`/
  `NodeState` outside their own package is a comment in `DeterminizationStrategies.java` — not a
  real call. Porting this cannot regress any already-shipped code path; it's pure addition.
- **Phase-0 coverage is likely already adequate — this is a correction to
  `docs/UNPORTED-SCOPE-SIZING.md`'s claim that it needs real Phase-0 work.** A fresh JaCoCo run in
  `../walnut-java` (regenerate via `mvn test jacoco:report` if the checked-in report under
  `target/site/jacoco/Automata.Numeration/` looks stale) shows `Ostrowski.java` at **96% line /
  89% branch** and `NodeState.java` at **100% line / 95% branch**, from Walnut's own original
  `OstrowskiTest.java` (166 lines, 10 `@Test` methods, last touched 2026-06-08 — this predates
  this project's own coverage-driving work, so it's upstream Walnut's authors' coverage, not
  something Phase 0 built). **Don't skip step 1 below on the strength of this note** — actually
  regenerate and inspect the report yourself, and specifically look at what the missed ~4-13% of
  branches are (JaCoCo's `Ostrowski.java.html` marks them inline) before deciding whether they're
  worth closing pre-port. But go in expecting a small top-up, not the ground-up characterization
  effort CAS export needed.
- **The BFS transition-table shape uses `fastutil`'s `Int2ObjectRBTreeMap<IntList>`** (Java) —
  the same idiom already established for `Fa.d` (`Vec<BTreeMap<i32, Vec<usize>>>` in `wr-core`).
  Follow `PORTING.md`'s existing ruling for that shape. One thing worth deliberately deciding
  rather than defaulting on: this is **new** code, not a port of an already-differentially-tested
  existing structure, and U33/U34 (see `CLAUDE.md`'s history) found that exact
  `BTreeMap`-per-transition pattern to be a real, measured performance liability in
  `subset_construction`'s hot loop when ported naively. You have more freedom here than a
  strict mechanical port normally allows to start with a flatter representation if the BFS
  shape supports it cleanly — note the decision and rationale in your plan either way, don't
  silently default to the heaviest structure out of pure mechanical-port habit.

### What to actually do, in order

1. **Confirm/close the Phase-0 coverage gap** (see above — likely small, but verify against the
   live JaCoCo report, don't assume).

2. **Write a plan**, adversarially reviewed by an independent agent before any code lands (this
   project's standing convention for a unit this size — see the Phase 2/3/4 plans referenced in
   `CLAUDE.md`'s "Current status", and `.claude/plans/amber-transcribing-ledger.md` for the CAS
   precedent). Unlike CAS export, **this unit is genuinely math/construction logic** (a new
   automaton-construction algorithm, not text formatting), so treat it with the weight
   `CLAUDE.md` gives `wr-core` decision-procedure code, not the lighter tier CAS qualified for.
   The plan should cover:
   - Target crate/module: almost certainly `wr-core` (it constructs real `Automaton`/`Fa` values
     via BFS, same tier as `NumberSystem`/`Valmari` — not a `wr-io` formatting concern). Pick a
     module name and confirm it doesn't collide with or entangle `numsys.rs`'s existing custom-base
     surface, given the file-format overlap noted above.
   - The wiring point in `wr-cli` (find the Prover-dispatch equivalent — Java's `Prover.java` has
     `OST = "ost"` at line 108 and dispatches to `Ost.ostCommand` at line 690; use whatever pattern
     the already-ported commands in `wr-cli`'s command-dispatch module use, not a new ad hoc hook).
   - The `NodeState`/BFS transition-table representation decision from above.
   - Confirmation (or correction) of the "no new NumberSystem consumption path needed" claim
     above, and what changes, if any, that implies for scope.
   - Test plan: the existing Java `OstrowskiTest.java` is your Tier-2 replication target; the
     1 golden fixture referencing `ost` (`drop_command:ost` in the manifest — confirm its ID and
     current exclusion reason in `tests/golden/`) should be un-excluded once this lands, mirroring
     how CAS export's fixtures were un-excluded in the same unit that ported it. Given the thin
     existing golden/differential signal (only 1 fixture), also plan targeted new tests: hand-built
     small `(preperiod, period)` cases checked against real `walnut-java` output, plus at least one
     property test if a natural invariant exists (e.g. round-tripping a value through the
     representation automaton and back, or the adder automaton agreeing with the representation
     automaton's own arithmetic on small cases) — this is exactly the kind of Walnut-independent
     check Tier 4 asks for, and this feature currently has none.

3. **Execute the plan** through the full implementer → two-independent-adversarial-reviewer →
   fixer loop (model different from the author for at least one reviewer, per `CLAUDE.md`'s
   trust-critical-code rule) — this is `wr-core` construction logic, not a formatting-only change,
   so it gets the full gate CAS export didn't need.

4. **If you find a genuine Walnut (Java) bug** while reading/porting these three files — not a
   quirk, an actual wrong-output or crash-on-plausible-input defect — log it in
   `docs/WALNUT-BUGS.md` per `CLAUDE.md`'s rule and port it verbatim; do not silently fix or
   silently replicate it without logging.

5. **Merge gate**: `cargo test --workspace` green, `cargo fmt --all`/`cargo clippy --workspace
   --all-targets` clean. Do not delete any test. Do not commit without the user's explicit
   go-ahead (this project's standing git-hygiene rule) — leave the work staged/described and say
   so.

Report back with: whether the "no new NumberSystem consumption path needed" architectural claim
held up, what the Phase-0 coverage top-up (if any) actually was, the plan's adversarial-review
outcome, final test/golden-corpus status, and any `WALNUT-BUGS.md` entries added.
