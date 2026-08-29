# RESUME-HERE — Idiomatic-Rust refactor (Phase R) in progress on `refactor/idiomatic`

**Reconciled 2026-08-29.** This file's previous contents were the 2026-08-17 "Phase 4 COMPLETE"
checkpoint, which predated everything that landed after U34: U28 (Logging threading), the four
backlog items, CAS matrix export, Ostrowski, negative-base + `split`/`rsplit`, and the entire
18-PR bug-fix-then-port sweep. That narrative is recorded in full in `CLAUDE.md`'s "Current
status" section (the canonical history) and the git log; it is not duplicated here. The three
"What's open" items it tracked are all since closed or absorbed: `details`-Logging threading
closed by U28 (golden now 671 compared / 670 pass, sole divergence fixture 383); `I`-over-`lsd`
resolved as a negative-hypothesis finding with new coverage; U34's Phase 2 is now Phase R4 below.

## Current work: the idiomatic-Rust refactor

- **Plan:** `~/.claude/plans/robust-seeking-flamingo.md` (v2 — adversarially plan-reviewed by
  independent Sonnet + Opus agents 2026-08-29; all findings folded in).
- **Branch:** `refactor/idiomatic`, cut from `bugfix/wb-001` @ `8fc95eb` (top of the 18-PR
  bug-fix stack). Behavior-preserving; quirks/WB entries/message text/`.gv`+CAS bytes/state
  numbering all survive. **Nothing gets pushed without the user's explicit go-ahead.**
- **Execution model:** coordinator dispatches only; Sonnet implements (strictly sequential in the
  shared tree); adversarial review = one Opus + one Fable (split context, diff only) for U1 and
  U4–U11 and all of Phase R4, single Opus for U2/U3/U8/U10.
- **Guardrails:** `docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md` (U0's mechanically-derived list —
  goes verbatim into every implementer and reviewer prompt).

### Unit ledger

| Unit | Scope | Status |
|---|---|---|
| U0 | Branch, baselines, sibling assert, guardrails doc, this reconciliation | **in progress** |
| U1 | matrix_writer loop/param idioms (bytes frozen) | pending |
| U2 | wr-cli error consolidation (4-tuple snapshot first) | pending |
| U3 | wr-cli bool flags → enums | pending |
| U4 | wr-logic ownership (token.rs, expr.rs; eval.rs excluded) | pending |
| U5 | wr-core bool flags → enums (numsys/logicalops/automaton/…) | pending |
| U6 | wr-core curated loop→iterator (S-gated) | pending |
| U7 | wr-core clone reduction (equiv.rs excluded; S-gated) | pending |
| U8 | sentinel/cast hygiene (minimal) | pending |
| U9 | Automaton Track struct (label stays separate; encoder decided) | pending |
| U10 | Fa named constructors | pending |
| U11 | wr-io reader/writer idioms (write_gv/export_to_ba frozen) | pending |
| R4 (U12a–d) | OPTIONAL Fa.d → TransitionRow — **user go/no-go at Phase 3→4 boundary** | not decided |

### U0 baseline (2026-08-29)

- Sibling oracle: `walnut-java` checked out at `bugfix/wb-001` = `14509f1` (was on
  `bugfix/wb-019`; switched 2026-08-29). Untracked in sibling, recorded as benign: `.java-version`,
  `src/test/resources/integrationTests/Global/Result/global_log.txt` (leftover live-jar session
  log; `build_session_tree` copies `Global/` but comparisons never read that path).
- **T0**: `cargo test --workspace` **1,723 passed / 0 failed** across 49 suites; `fmt --check`,
  `clippy --workspace --all-targets`, and `cargo +nightly check` on the fuzz workspace all clean.
- **T1**: golden corpus **675 fixtures | 670 pass | 1 fail (383, the known `Details`-only
  divergence) | 4 skip (deferred-OTF) | 0 timeout | 0 not-run** — exactly the plan's bar.
- **T4**: all 11 workloads RUST FASTER (1.56×–3.37×); full per-fixture report snapshot committed
  at `benches/baseline-refactor-u0.txt` (the no-regression comparison anchor for later units).

## Recovery procedure

Read this file + `git log --oneline master..HEAD` on `refactor/idiomatic`. Every landed commit is
independently green (T0 minimum). The unit ledger above says what's next; the plan file says how.
Mid-unit state, reviewer verdicts, and baseline deltas are appended per unit below the ledger.

## Process note worth keeping (from the Phase-4 checkpoint, still the standing default)

Both U30 and U31 ran multi-round adversarial review chains that each found real, non-trivial
issues on every round until they converged — U30 took five rounds (production-logic bugs,
narrowing to rare Unicode edge cases), U31 took two (test-strength gaps, not production bugs).
The pattern holding across both: two reviewers per round, different models, split context, given
only the diff — never the author's rationale — reliably surfaces real problems that a single
self-review pass did not. Treat this as the standing default for any future trust-critical unit
in this project, not a one-off born of U30 specifically.
