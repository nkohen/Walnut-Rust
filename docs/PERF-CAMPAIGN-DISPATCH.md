# Dispatch: the performance campaign (`perf/beyond`) — beat the JVM decisively, not incidentally

You are the coordinator of a performance-optimization campaign on walnut-rs. Read `CLAUDE.md`
in full first — every rule there binds you (merge gate, two-adversarial-reviewer loop for
trust-critical crates, fleet hygiene, token discipline). Then read
`docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md` (the behavioral guardrails; they bind every unit
here too) and `benches/STATUS.md` + `benches/README.md` (the measurement history and
methodology).

## Mission

Branch `perf/beyond` off `refactor/idiomatic` @ `cc64579` (do NOT rebase; nothing is pushed
without the user's explicit go-ahead). The port currently beats JVM Walnut on all 11 benchmark
workloads, but the margin on the engine-bound fixtures has measured as low as ~2× in some
runs. **The user's goal: >10× on most workloads.** Treat that as the aspiration that shapes
where you spend effort — NOT as a number to manufacture. This project's culture is honest
measurement: if, after real profiling, some workload's ceiling under the fidelity constraints
is 4×, the deliverable is that finding with evidence, stated plainly, plus the list of what
would have to be sacrificed to go further. Never soften a negative result; never claim a
speedup you did not measure on the same machine in the same session.

## Hard constraints (what "optimization" may never change)

Observable behavior is FROZEN, exactly as it was for the idiomatic refactor:
- Golden corpus stays exactly `675 | 670 pass | 1 fail (383, Details-text only) | 4 skip |
  0 timeout` — this includes `::`-details LOG TEXT (per-operation `Logging` lines, state
  counts, indentation), `.gv`/CAS bytes, and error messages. An optimization that changes
  state numbering, iteration order, metastate discovery order, or a logged intermediate count
  is a behavior change, not an optimization — the guardrails doc lists the order-load-bearing
  sites.
- `wr_core::equiv` and `wr-cts` are frozen oracles: never touched by any perf unit. If a hot
  path leads there, the finding is reported, not acted on.
- No `unsafe` without a dedicated unit, an explicit safety argument in the diff, and both
  reviewers signing off on the argument specifically. No parallelism/threads in the decision
  procedure without STOPPING and asking the user first (determinism and log-order risks are
  structural, and Java is single-threaded — a parallel speedup is not a like-for-like win).
- Every unit through the full loop: implementer → two split-context adversarial reviewers
  (models per CLAUDE.md's rules; different from the author) → fixer; `cargo test --workspace`
  green at every commit; zero tests deleted.

## Methodology (non-negotiable, learned from U32-U34)

1. **Re-baseline first, same session, before touching code.** Cross-run bench numbers in this
   repo have varied by >3× on identical code (fixture 521: 2.44× at the U0 baseline vs 9.60×
   in the 2026-09-02 sweep — same source both times). Run `cargo run -p wr-bench --release
   --bin compare` 3×, warm machine, record medians-of-medians into a committed
   `benches/baseline-perf-campaign.txt`. All later claims compare against THIS baseline, and
   any headline claim gets a same-session A/B (checkout old commit, re-run) before it ships.
2. **Profile before proposing.** `sample`/`samply` on the slowest engine-bound fixtures
   (286, 230, 179, 261 historically) per `benches/STATUS.md`'s repro commands. Every unit's
   plan names the profile bucket it attacks and its measured share of REAL work. No
   speculative optimization: if a candidate's bucket is <5% of samples, it is not a unit.
3. **Pre-registered decision rules** (the U34 pattern): each unit states, before implementing,
   what measurement outcome means "ship," "iterate," or "revert." A unit that helps one
   fixture and regresses another reverts unless the user rules otherwise.
4. **Fidelity gates per unit**: workspace tests; golden corpus (exact tally above);
   differential-gen soak ≥5,000 queries with a fresh VALID-HEX seed (digits/a-f only — an
   invalid seed panics the harness at parse); fuzz smoke for any unit touching
   reader/parser/regex; and the bench A/B. Structural-snapshot-first (the repo's S-gate
   pattern, see `crates/wr-core/tests/refactor_structural_snapshots.rs`) for any unit that
   could move state numbering.
5. **Workload honesty.** The 11 fixtures skew small. Add (as an early unit) 2-4 heavier
   research-shaped workloads to `wr-bench`'s opt-in rows — the `[strategy 6 BRZ]` sc637 row
   is precedent, and `thm5`-class queries are the stated reason that metacommand was wired.
   A 10× claim on sub-millisecond dispatch fixtures is not the 10× the user wants.

## Ranked candidate levers (start here, but the profile decides)

1. **Resume the `Fa.d` → `TransitionRow` migration** — `~/.claude/plans/
   glossy-compacting-lantern.md` §2: a COMPLETE, triple-adversarially-reviewed design
   (sorted `Vec<(i32, SmallVec<[usize;1]>)>` rows, `TransitionRowBuilder` with
   three-variant `DuplicatePolicy` mandatory at 9 sites, a 10-site order-sensitivity audit,
   P2a→P2c sequencing with bit-identical snapshots first). It was shelved twice: U34's
   pre-registered perf checkpoint said stop at 4.7% residual, and the idiomatic refactor's
   Phase 4 was declined on code-quality grounds. **This campaign's rationale is the one the
   design was originally built for, and the user has re-opened perf work — you are
   authorized to execute it IF a fresh profile shows `Fa.d`-rooted cost (BTreeMap
   navigation + allocator traffic attributable to transition rows) justifying it.** Execute
   the design verbatim; do not redesign. Note the design predates the U9 Track refactor and
   U10 constructors — re-verify its ~22-file site list against the current tree before
   starting; the plan's own instructions require exactly that.
2. **Subset-construction internals** — after U34's scratch-buffer fix, `BTreeSet::insert`
   inside `subset_construction` measured ~24% of fixture-286 samples. Metastate
   representation (sorted-vec/bitset keyed maps) can be swapped ONLY if metastate discovery
   order — and therefore output state numbering — is provably identical; S-gate mandatory.
3. **Valmari `Partition` locality** — `minimize.rs`'s `mark`/`split` machinery measured
   ~20.5%. The algorithm is frozen (WB-001 history; set numbering is observable); layout
   (SoA, index widths, allocation reuse across calls) is fair game under an S-gate.
4. **Per-operation allocation reuse** — `benches/STATUS.md`'s still-open candidate #3:
   scratch buffers threaded through `act()`-level operations instead of fresh maps per
   state. Compose with (1).
5. **`Logging` zero-cost check** — verify the non-`::` path (the overwhelmingly common case)
   does no formatting work; if `format!` runs before the enabled-check anywhere hot, fix the
   call shape without changing any emitted byte.
6. **Micro** (only if the profile insists): `encode`/`decode` paths, BigInt in hot parses,
   hash-map choices in non-order-observable spots (the guardrails list which `transducer.rs`
   maps are certified; everything else needs its own audit).

## Process

Work in units exactly like the refactor did (see `RESUME-HERE.md` for the ledger format —
reconcile its header to this campaign in your U0). One implementer at a time in the shared
tree; reviewers read-only. Sonnet implements mechanical units; escalate implementer model for
the TransitionRow flip and any Valmari-adjacent unit (CLAUDE.md's model-tiering doctrine —
those are the hard ~20%). Commit per unit with the full evidence trail in the message
(pathspec-scoped, Co-Authored-By trailer, single-quoted or heredoc -F). NOTHING is pushed
and no PR is opened without the user's explicit instruction. At natural boundaries (after
re-baselining; before starting TransitionRow; when the ranked list is exhausted), STOP and
report to the user with the numbers table: per-fixture median vs the campaign baseline vs
Java, and an honest "remaining ceiling" assessment.
