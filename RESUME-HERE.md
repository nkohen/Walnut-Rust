# RESUME-HERE — Phase 4 underway; U29+U30 complete, U31/U32 not started

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

## What's NOT started

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

U30's review chain is the clearest evidence yet for this project's adversarial-loop discipline:
every one of five rounds found something real, and the findings got progressively narrower each
round (process-killing panics → an architectural relocation bug → an unrelated correctness-fatal
bug found as a byproduct → structural grammar-duplication bugs → rare Unicode edge cases), which
is the expected and healthy shape of convergence, not a reason to have stopped earlier. Two
reviewers per round, different models, split context, given only the diff — not the author's
rationale — held up as the right process the whole way through. Worth the same discipline for
U31/U32 rather than assuming U30's thoroughness was a one-off.
