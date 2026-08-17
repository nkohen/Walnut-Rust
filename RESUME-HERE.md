# RESUME-HERE — Phase 4 COMPLETE (U29+U30+U31+U32); roadmap fully executed

**2026-08-16 checkpoint.** Phase 4 (Hardening) is approved and in progress. Plan at
`~/.claude/plans/purrfect-doodling-muffin.md` (outside this repo — not checked in; if that path
is ever unavailable, the plan's full text is summarized in `CLAUDE.md`'s "Current status"
section).

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

## What's done: U32 (performance vs JVM Walnut)

`benches/` — a Criterion benchmark of the Rust side plus a warm-JVM head-to-head over 11 real
corpus fixtures. Full numbers, methodology and threats to validity in `benches/STATUS.md` and
`benches/README.md`; narrative in `CLAUDE.md`'s "Current status".

**Read this part before anything else in this section:** DESIGN.md §8's *"faster than Walnut on
the research workloads"* clause is **NOT met**. The port is 1.35-1.73× faster on sub-millisecond
per-command overhead and **1.28-1.65× slower on every workload where the decision procedure
dominates**. A profile attributes 51.5% of the port's CPU time to the system allocator and 12.2%
to `BTreeMap` navigation — the price of the mechanical-port rule's faithful
`Vec<BTreeMap<i32, Vec<usize>>>` transition representation against a JVM nursery allocator. This
is a genuine finding, not a harness artifact (two harness artifacts *were* found and fixed first
— see `CLAUDE.md`); it is documented rather than dropped, per the plan's explicit instruction.

## What's open

1. **The performance gap above.** `benches/STATUS.md` §"What would close the gap" ranks four
   candidates; the cheapest (swap the global allocator) is also the one that would *confirm or
   refute* the allocator diagnosis, so it is the natural first step if this is picked up. All of
   them touch `wr-core` and would need the full two-reviewer loop. **Nothing here is scheduled** —
   it needs a user decision, not an automatic continuation.
2. **`details`-fixture `Logging` threading** (the long-standing "U28"): still open, still 7 of 586
   golden fixtures. U32 gives it a second, independent motivation — it is also why the
   benchmark's Rust-side peak-state column is only a lower bound.
3. **`I`-over-`lsd`** — still the user-deprioritized backlog item from Phase 3b.

**Phase 4, and DESIGN.md's original roadmap, are now fully executed.** Per this project's
phase-gating convention this is a real stop-and-reassess point with the user, not a licence to
continue into undefined further work.

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
