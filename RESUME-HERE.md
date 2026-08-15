# RESUME-HERE — Phase 3b complete; two open follow-ups, Phase 4 scope undecided

**2026-08-15 checkpoint.** U27 (the Tier-1 golden-corpus harness) is merged to `master` at
`2a00443`, closing out Phase 3b's actual DESIGN.md exit criterion. `cargo test --workspace` is
green, `cargo fmt`/`clippy` clean. `docs/WALNUT-BUGS.md` is unchanged at **37 entries**
(WB-001–WB-037) — U27 found zero new genuine Walnut bugs; every divergence it surfaced was a
port defect, now fixed. Full narrative in `CLAUDE.md`'s "Current status" section and
`tests/golden/STATUS.md`.

**Phase 3b is done.** No unit remains started-but-incomplete. What's open is two small,
well-scoped follow-ups U27 surfaced (neither blocks anything else) and an undecided Phase 4 scope
— all three need the user's go-ahead before work starts, per this project's standing
phase-gating convention.

## What U27 built and its process, briefly

`tests/golden/` replays all 675 of Walnut's own integration-test fixtures through
`Prover::dispatch_for_integration_test` (the real library API, never a subprocess), comparing
against Java's own recorded golden output: automata by `wr_core::equiv` semantic equivalence,
`details`/`error` fixtures by a verbatim port of Java's `assertEqualMessages` normalization /
exact match. Result: **573/583 compared fixtures pass (98.3%)**, 92 excluded with a recorded
reason, 0 over-budget/not-run, 10 known divergences (two root causes, see below) pinned rather
than silently skipped.

Went through the implementer → two-independent-reviewer → fixer loop **twice** — the second
review round (scoped to the first round's fix commit) found two more real correctness-risk gaps
in the fixes themselves, which is exactly what the loop is for. Nothing here is a lesson so much
as confirmation the process works: five commits total (`b9d2bd5..2a00443`), each review round
narrower and cheaper than the last, zero correctness-fatal findings at any point, and the coordinator's own final read of the last diff plus a from-scratch `cargo test --workspace` +
`fmt --check` run (not just trusting the fixer's self-report) before merging — worth keeping as
the default verification bar for any unit this size, not just when a reviewer flags something.

## The two open follow-ups (neither started)

1. **`details`-fixture `Logging` threading** (affects 7 of the 10 known divergences). Every
   state count already matches real Walnut exactly, pre-minimization ones included — only the
   per-`act()` progress/log-line trace is missing, because `wr-core`'s product/determinize/
   minimize/quantify don't thread a `Logging` handle through yet. `crates/wr-logic/src/eval.rs`
   already flags this as owed. Tentatively "U28" if picked up — check current `WB-`/unit numbering
   live before assuming that label is still free.
2. **`transduce` over a reversed (lsd) custom-base DFAO** (3 of the 10 known divergences, fixture
   532 root + 2 downstream). Every other `transduce` fixture passes, including the *same*
   transducer's **msd**-direction case — so the defect is specifically in
   `Transducer::transduceNonDeterministic`'s reverse-input/reverse-result branch, not the Dekking
   construction generally. Not yet root-caused past that isolation.

Both are documented in `tests/golden/STATUS.md` with the exact fixture IDs and are pinned (not
silently passing) in `KNOWN_DIVERGENCES` in `tests/golden/tests/golden_corpus.rs` — if either
gets fixed, remove its entries there and confirm the corpus count moves from 573/583 toward
583/583, don't just patch the underlying bug and leave the pin stale.

## Phase 4

Not scoped yet. `docs/DESIGN.md` should have the original phase breakdown if one was planned;
otherwise this is a fresh planning conversation with the user before any code moves — same
gating as every phase boundary so far in this project.
