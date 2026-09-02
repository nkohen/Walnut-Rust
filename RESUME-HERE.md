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
| U0 | Branch, baselines, sibling assert, guardrails doc, this reconciliation | **done** (`f8b657f`) |
| U1 | matrix_writer loop/param idioms (bytes frozen) | **done** — Sonnet impl; Opus+Fable both "no correctness defect" (Opus proved byte-equality with an exhaustive old-vs-new differential harness incl. q==0/q==1/q0>=q); 3 non-blocking findings (helper placement+citation, q0-position pin test, doc nit) applied in a fixer pass. T0 1,723/0; T1 exactly 670/675 (383 only); wr-io 149/0 post-fix. |
| U2 | wr-cli error consolidation (4-tuple snapshot first) | **done** — snapshot committed first (`795c459`, 412 assertions, 2 pre-existing kind()-routing quirks pinned as-is: MorphismCommandError::InvalidFile falls to WalnutException; reg family unrouted — possible future follow-up, NOT refactor scope); then `simple_error_froms!` macro (18/19 markers, 29/31 Froms; prover.rs+walnut_exception.rs byte-untouched). Opus review: no correctness defect (exhaustive 29/29 mapping comparison); test-gap closed with direct From-routing assertions + doc fixes. T0 ~1,744/0; T1 exactly 670/675. |
| U3 | wr-cli bool flags → enums | **done** — `AutomatonKind`/`SplitDirection` introduced; 4 externally-called fns deliberately keep `bool` (ratified criterion: in-repo external caller in tests/benches, not pub-ness); print_flag/print_details left as correlated pair (3 valid states + WB-039 logging adjacency). Opus review: no polarity transposition across all 46 sites; 2 test-gaps closed (split-vs-rsplit dispatch tripwire, de-vacuumed library-selector assertions) + 2 doc fixes. T0 1,745/0; T1 exactly 670/675. |
| U4 | wr-logic ownership (token.rs, expr.rs; eval.rs excluded) | **done** — 5 clones removed in token.rs (32 left with airtight reasons; expr.rs 0 — all structurally required). Opus+Fable both "no defect" (interior-mutability loophole ruled out; Java originals cross-checked). Adjudication: adopted Opus's pre-format-message variant so both Logging-adjacent hunks keep Java's exact statement order. All 6 gates green: T0 1,746/0, T1 exactly 670/675, fuzz 1.5M execs/0 crashes, diffgen 5,000/5,000 match. |
| U5 | wr-core bool flags → enums (numsys/logicalops/automaton/…) | **done** — 6 enums (Direction, ComparisonOperands/Negation, MsdFlip, SubsetCheck, OperandOrder), ~60 sites; convert_ns kept bool (external caller); equiv/logging/search untouched. Opus+Fable both "no defect": Opus ran a normalizing back-substitution token-diff vs HEAD (nothing unaccounted); Fable grounded every polarity in the Java originals. 4 style fixes applied (3 stale docs, reverse→order rename). All 5 gates green: T0 1,746/0, fuzz-check clean, T1 exactly 670/675, diffgen 5,000/5,000. |
| U6 | wr-core curated loop→iterator (S-gated) | **done** — snapshots first (`567dd0f`, 13 tests); ~40 loops classified, 7 sites converted (search/minimize/ostrowski left wholesale). Review found a REAL blocking correctness-risk (both reviewers independently): two conversions dropped their `0..self.q` bound for container length, diverging from HEAD/Java panic behavior on the documented stale-`q` shape — both hunks reverted to literal HEAD form (verified byte-absent from the final diff by the coordinator); + 2 test-gaps closed (msd-vector assertions, output>1 collapse pin) and inline-traceability notes. T0 1,759/0; T1 exactly 670/675; diffgen 5,000/5,000. |
| U7 | wr-core clone reduction (equiv.rs excluded; S-gated) | **done** — 62 clones examined, only 6 removed (RefCell-scope/as_dfa/ownership-transfer all correctly left); snapshots first (`86e1877`). Opus+Fable both "no defect": deep-clone trap proven dodged (`(**n)` place-typed, `bind`'s `&mut self` makes an Rc-typed receiver uncompilable); refcount delta proven unobservable workspace-wide. Full ladder green incl. bench: ALL 11 fixtures faster than U0 baseline (0.4–7.8%), peak states bit-identical. |
| U8 | sentinel/cast hygiene (minimal) | **done** — 5 named `-1` constants + 1 real invariant-citing cast doc; deliberately skipped: transducer.rs (fragile marker area), fa.rs casts (no citable invariant — the gap is documented honestly), search.rs sign tests (rewriting to `==` would change semantics for negative start states — Opus confirmed the trap was correctly avoided). Opus: no correctness defect; 3 comment tweaks applied. T0 1,764/0; T1 exactly 670/675. **Phase 2 complete.** |
| U9 | Automaton Track struct (label stays separate; encoder decided) | **done** — 3 commits (A `632cf76` type+accessors, B `01a8ace` ~700-site migration, C the storage flip). Opus+Fable on the full 4,250-line unit: no correctness-fatal; ONE blocking correctness-risk (Automaton::new's zip silently truncated on contract-violating input where HEAD panicked — closed with a constructor assert, golden re-verified). Opus PROVED sort_label/reduce_dimension consolidations equivalent by exhaustive probe (110,592 + 3,672 cases vs independent oracles); encoder rebuild audited at every mutation point. equiv.rs: one compile-forced mechanical hunk, signed off. 2 WB-013 comment deletions signed off (scenario unrepresentable; WB-013 still referenced in-function). Full ladder green; doc warnings multiset-identical to baseline. |
| U10 | Fa named constructors | **done** — `Fa::with_states` (+ pre-existing `Fa::trivial`), ~247 sites converted, net −1,395 lines; 4 literals kept (2 quirk-shape tests + constructor bodies); a self-introduced infinite-recursion script bug self-caught via profiling before review; equiv.rs/wr-cts conversions reverted by coordinator direction (discretionary changes in frozen modules — the freeze stays bright-line). Opus review: no correctness defect — ALL 247 conversions machine-verified positionally vs HEAD (balanced-delimiter parser, 0 mismatches); the debug_assert pair proven a real q0/q-transposition tripwire; 3 style fixes applied. T0 green; T1 exactly 670/675. |
| U11 | wr-io reader/writer idioms (write_gv/export_to_ba frozen) | **done** — only 4 conversions (the files were already near-idiomatic); writer.rs zero-diff (entire non-trivial surface frozen). Opus+Fable both "no defect" (Opus brute-forced 4,681 read_comments cases + probed format outputs vs the removed code as oracle; Fable proved zip-truncation impossible via the arity guard's dominance). 2 findings applied (doc reword; a first-error-wins pin test). Full ladder green incl. 54k-exec fuzz smoke. **Phase 3 complete.** |
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
