# RESUME-HERE — Phase 4 underway; U29 complete, U30/U31/U32 not started

**2026-08-15 checkpoint.** Phase 4 (Hardening) is approved and in progress. Plan at
`~/.claude/plans/purrfect-doodling-muffin.md` (outside this repo — not checked in; if that path
is ever unavailable, the plan's full text (as revised after adversarial review) is preserved in
this session's conversation transcript and summarized in `CLAUDE.md`'s "Current status" section).

## What's done: U29 (Tier-3 differential-generator harness)

`tests/differential-gen/` — a two-sided harness comparing the Rust port against a live
`walnut-java` JVM oracle over a large stream of randomly generated small KEEP-subset queries.
Three commits:

1. `4ca773c` — Milestone 0 build: Java driver (`tests/differential-gen/java/DiffGenDriver.java`,
   documented recipe per `tests/differential-gen/CAPTURE.md`, not committed into `walnut-java`'s
   tracked source) + Rust harness crate (`wr-differential-gen`), 10,000-query soak clean.
2. `b6e9b3a` — the mandatory two-independent-reviewer round on commit 1 (Opus + Sonnet,
   split-context). Both independently found the same headline bug (a false-green pass/fail gate
   that didn't check for a degraded/dead oracle); Opus additionally found a missed JVM-restart
   case (`Answer::Fatal`/OOM), a temp-file race that could mask a real divergence as a match, and
   zero test coverage on the query-ID echo check ("the load-bearing invariant" — mutation-tested
   proof the existing suite couldn't detect its removal). All fixed here.
3. `3c3d852` — scaled to 120,000 generated queries across 4 seeds: **120,000 match, 0
   divergences, 0 skips**, meeting U29's exit criterion (N≥10⁵, zero unresolved divergences).
   Full numbers and methodology in `tests/differential-gen/STATUS.md`.

**No new genuine Walnut (Java) bug found** — `docs/WALNUT-BUGS.md` unchanged at 37 entries
(WB-001–WB-037).

**What this run does NOT cover, by construction** (documented, not an oversight — see
`tests/differential-gen/STATUS.md`'s exclusions section): `::`-detail-printing queries,
`transduce`/`def`/`reg`/other non-`eval` `Commands/*`, the `I` (infinitely-often) quantifier,
custom bases, word/macro/function tokens, formula depth >3, quantifier nesting >2. So this run
neither confirms nor clears the two pre-existing Phase-3b follow-ups (still open, still separately
scoped, unchanged since before Phase 4 started):

1. **`details`-fixture `Logging` threading** (7 of `tests/golden`'s 10 known divergences) —
   `wr-core`'s product/determinize/minimize/quantify don't thread a `Logging` handle through yet;
   `crates/wr-logic/src/eval.rs` already flags this as owed. Tentatively "U28" if picked up.
2. **lsd-direction `transduce` divergence** (3 of `tests/golden`'s 10 known divergences, fixtures
   532-534) — `Transducer::transduceNonDeterministic`'s reverse-input/reverse-result branch, not
   yet root-caused past that isolation. `tests/golden/STATUS.md` has the fixture IDs.

Neither is part of Phase 4's plan as scoped (the plan's U31 #7 explicitly keeps the
lsd-`transduce` hunt separate, in U29's now-built live-JVM bisection infra, should it be picked up
later — but U29 as executed didn't reach for it, since the generator doesn't emit `transduce`
commands at all). Pick either up as a deliberate follow-up, same phase-gating convention as
everything else.

## What's NOT started

- **U30** — Tier-5 fuzzing (`cargo-fuzz`, 3 targets: `wr-io` reader, `wr-logic` parser, `wr-core`
  regex engine). Fully independent of U29/U31; can start any time. First step per the plan: a
  ~30-min spike confirming `cargo-fuzz` builds/runs at all on this machine (darwin 24.6.0 arm64,
  ASAN has known rough edges there) before committing to the full design.
- **U31** — Tier-4 property-suite completion (quotients, `convertNS`, `Morphism`,
  `fixleadzero`/`fixtrailzero`, `NumberSystem::Div`, lsd trailing-zero fixup, `Transducer`). Full
  gap list + the plan's corrected oracle designs (notably: `convertNS` needs a captured sweep +
  a powers-of-2-restricted property, NOT a naive property test, because WB-032 is a
  ported-verbatim quirk a naive oracle would misflag) are in the plan file's U31 section.
- **U32** — Performance vs JVM Walnut. Wants U29's JVM-batching infra (done, reusable) and
  ideally U31 complete first. Benchmark-sourcing needs a decision at execution time between the
  plan's two documented options (golden-corpus throughput as the exit proxy, vs. first wiring
  `[strategy N NAME]` to unlock `thm5`-class fixtures — default is the former, smaller option).

## Process note worth keeping

The plan-review step (an independent Opus agent adversarially reviewing the *plan document*
itself, not just the code, before any implementation started) caught real architectural defects
that would otherwise have surfaced mid-implementation or, worse, produced a silently-wrong 10⁵-
query "pass." Worth repeating for U30/U31/U32 if their scope turns out to need more than the
plan file's existing detail once execution starts.
