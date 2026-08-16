# RESUME-HERE — Phase 4 underway; U29+U30+U31 complete, U32 not started

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

## What's NOT started

- **U32** — Performance vs JVM Walnut. The last unit in Phase 4's plan. Wants U29's JVM-batching
  infra (done, reusable) and benefits from U31 being complete (it is). Benchmark-sourcing needs a
  decision at execution time between the plan's two documented options (golden-corpus throughput
  as the exit proxy, vs. first wiring `[strategy N NAME]` to unlock `thm5`-class fixtures — default
  is the former, smaller option). Once U32 lands, Phase 4 — and the original DESIGN.md roadmap —
  is complete; worth a real stop-and-reassess with the user at that point, not an automatic
  continuation into undefined further work.

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
