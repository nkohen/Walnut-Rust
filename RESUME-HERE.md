# RESUME-HERE — Phase 4 COMPLETE (U29–U34); DESIGN.md's original roadmap fully executed, all exit criteria met

**2026-08-17 checkpoint.** Phase 4 (Hardening) is done — correctness AND performance exit
criteria both met. Plan at `~/.claude/plans/purrfect-doodling-muffin.md` (outside this repo — not
checked in; if that path is ever unavailable, the plan's full text is summarized in `CLAUDE.md`'s
"Current status" section). U33/U34 were unplanned follow-ups the user explicitly requested after
U32's benchmark surfaced a negative finding — not in the original plan file, fully narrated in
`CLAUDE.md` instead.

## What's done: U29 (Tier-3 differential-generator harness)

`tests/differential-gen/` — 120,000 generated queries across 4 seeds vs a live `walnut-java` JVM
oracle, 0 divergences, meeting the N≥10⁵ exit criterion. Went through the full two-reviewer loop
twice (Milestone 0 build, then the scale-up). Full detail in `CLAUDE.md`'s "Current status" and
`tests/differential-gen/STATUS.md`. No new genuine Walnut (Java) bug found by this unit.

## What's done: U30 (Tier-5 fuzzing)

`fuzz/` — three `cargo-fuzz` targets (`wr_io_reader`, `wr_logic_parser`, `wr_core_regex`), real
seed corpora, all clean at scale. This unit ran an unusually deep review chain — **five rounds**,
each finding a real bug — because the very first fix attempt turned out to relocate a panic
rather than eliminate it, which is exactly the kind of thing a second (and third, and fourth...)
independent look is for. Commits: `4fc968f` → `b230db4` → `9a26f37`/`f60f086` → `4ea178b` →
`e8258c7`. Full narrative in `CLAUDE.md`'s "Current status" section. Headline outcomes:

- 4 fuzz-discovered process-killing panics fixed, all confirmed genuine port bugs (not Walnut
  bugs) against the real jar.
- **WB-038 logged** — `AutomatonReader` silently accepts an out-of-alphabet transition digit,
  ported verbatim per the mechanical-port rule.
- A real architectural fix (a top-level panic-recovery boundary at command dispatch, mirroring
  Java's `Prover.readBuffer` catch) after review found the first attempt only relocated the panic
  to six other commands.
- **An unrelated correctness-fatal bug found and fixed as a byproduct**: `reverse` wrote a stale
  number system into its output (`flip_ns` didn't update `Automaton::ns_name`), which could let a
  genuinely mixed-numeration `union` silently succeed. Fixing it **closed the previously-open
  lsd-`transduce` golden-corpus divergence** (fixtures 532-534) — that was never a `Transducer`
  bug. **Golden corpus: 573/583 → 576/583.**
- `wr-io`'s header parser unified onto shared grammar primitives, closing several real
  reader-fidelity divergences (alphabet dedup, set-grammar whitespace, ASCII-vs-Unicode
  whitespace, Java regex `$`/`.` leniency around rare Unicode line terminators, an
  `alphabet_size` overflow panic).

**Both pre-existing Phase-3b follow-ups this checkpoint used to track are now resolved or
narrowed:**
1. ~~lsd-direction `transduce` divergence~~ — **CLOSED** by U30's `reverse`/`flip_ns` fix (see
   above). Removed from `tests/golden`'s `KNOWN_DIVERGENCES`.
2. **`details`-fixture `Logging` threading** — still open, still the only golden-corpus gap (7 of
   583 fixtures). `wr-core`'s product/determinize/minimize/quantify don't thread a `Logging`
   handle through yet; `crates/wr-logic/src/eval.rs` already flags this as owed. Tentatively "U28"
   if picked up — check current numbering is still free before assuming.

## What's done: U31 (Tier-4 property-suite completion)

~17 new property tests across `wr-core`/`wr-logic`/`wr-cli` (quotients incl. WB-010, `convertNS`
via a widened JVM-captured sweep + one non-circular power-of-2 property, `Morphism` constrained
away from WB-036, `fixleadzero`/`fixtrailzero`, `NumberSystem::Div` + `msd_fib` round-trips, the
lsd trailing-zero fixup, `Transducer` constrained away from WB-035). Commits `f754a91`→`16a62bc`→
`a54e33e`→`76e5ff9`→`b740b27`. **Zero new genuine bugs found** — WB-038 remains the highest entry.
Full narrative in `CLAUDE.md`'s "Current status" section — its two review rounds are worth reading
even though they're done, because the findings were about test-strength itself (property tests
that passed against a deliberately-broken implementation), not production logic, which is a
distinct and easy-to-miss failure mode worth remembering for any future property-test unit.

## What's done: U32 (performance vs JVM Walnut) — a negative finding, then closed by U33+U34

`benches/` — a Criterion benchmark of the Rust side plus a warm-JVM head-to-head over 11 real
corpus fixtures. Full numbers, methodology and threats to validity in `benches/STATUS.md` and
`benches/README.md`; narrative in `CLAUDE.md`'s "Current status".

U32 itself found DESIGN.md §8's *"faster than Walnut on the research workloads"* clause **NOT
met**: faster on sub-millisecond overhead, but **1.28-1.65× slower on every workload where the
decision procedure dominates** — profiled to 51.5% of CPU time in the system allocator, 12.2% in
`BTreeMap` navigation, the price of the mechanical-port rule's faithful
`Vec<BTreeMap<i32, Vec<usize>>>` transition representation against a JVM nursery allocator. This
was reported honestly rather than softened (two harness artifacts *were* found and fixed first —
see `CLAUDE.md` — this is what was left after removing them).

**The user asked to attack the gap. Two follow-up units closed it completely:**
- **U33** — the two cheapest ranked candidates (swap the global allocator to `mimalloc`, enable
  `lto = "fat"`/`codegen-units = 1`), zero `wr-core` changes, zero algorithmic risk. Result:
  9-of-11-slower became 10-of-11-faster; a repeat profile confirmed the diagnosis directly
  (allocator's CPU share 51.5% → 13.4%). One holdout: fixture 637, the most
  allocation-intensive workload, still 1.16× slower.
- **U34** — closed 637 too. Investigation found the actual remaining bottleneck was narrower than
  U33's framing implied: not `Fa.d`'s storage itself, but one local `BTreeSet<usize>` inside
  `subset_construction`'s hot loop. A properly plan-first, three-round-adversarially-reviewed
  process (round 2 caught a genuine blocking defect in round 1's own proposed fix) replaced it
  with a reusable scratch `Vec` + `HashMap` lookup — a pure representation swap, proven so by two
  reviewers each independently running large-scale structural-equivalence probes (404,000 and
  20,000 cases) against the removed code as an oracle. **Result: 637 went from 1.16× slower to
  2.65× faster. All 11 benchmark workloads are now faster in Rust than in Java.** The plan had a
  pre-registered go/no-go checkpoint for a larger, riskier `Fa.d`/`TransitionRow` migration
  (Phase 2) — evaluated to **stop** on both of its own pre-committed conditions (637 no longer
  slow; residual `Fa.d`-attributable profile share measured at 4.7%, below the 8% threshold), so
  Phase 2 was correctly not implemented, per the plan's own decision rule rather than a post-hoc
  call. Its full design remains available to resume from if a future workload's profile differs.

**DESIGN.md §8's Phase-4 exit criterion — both correctness AND performance — is now fully met.**
Golden corpus unchanged throughout U33/U34 (577/586, 0 regression); no new genuine Walnut bugs
found by either unit.

## What's open

1. **`details`-fixture `Logging` threading** (the long-standing "U28"): still open, still 7 of 586
   golden fixtures — all confirmed text-only divergences (state counts already match exactly),
   not automaton-level. `wr-core`'s product/determinize/minimize/quantify don't thread a
   `Logging` handle through yet.
2. **`I`-over-`lsd`** — still the user-deprioritized backlog item from Phase 3b.
3. **U34's Phase 2** (the larger `Fa.d`/`TransitionRow` representation migration) — fully
   designed, deliberately not implemented per its own checkpoint's stop decision. Resume from
   `~/.claude/plans/glossy-compacting-lantern.md` (outside this repo) only if a future workload's
   profile shows `Fa.d`-rooted cost above the plan's own thresholds again.

**Phase 4, and DESIGN.md's entire original roadmap (Phases 0-4), are now fully executed with
every exit criterion met — correctness (Tier 0-5 all green) and performance (faster than Walnut
on all 11 benchmarked research workloads).** Per this project's phase-gating convention this is a
real stop-and-reassess point with the user, not a licence to continue into undefined further
work. Items 1-3 above are tracked backlog, not scheduled work.

## Process note worth keeping

Both U30 and U31 ran multi-round adversarial review chains that each found real, non-trivial
issues on every round until they converged — U30 took five rounds (production-logic bugs,
narrowing to rare Unicode edge cases), U31 took two (test-strength gaps, not production bugs).
The pattern holding across both: two reviewers per round, different models, split context, given
only the diff — never the author's rationale — reliably surfaces real problems that a single
self-review pass did not. Treat this as the standing default for any future trust-critical unit
in this project, not a one-off born of U30 specifically. Worth noting for U32 too, though
performance work has a different risk profile (correctness bugs there would more likely show up
as a differential/golden-corpus regression than something only an adversarial reviewer would spot
by reading the diff) — use judgment on how many rounds it actually needs rather than assuming the
same five-round depth applies by default.
