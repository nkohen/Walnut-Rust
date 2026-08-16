# Tier-3 differential generator — status

Snapshot of the last full scale-up run of `tests/differential-gen` (Phase 4, U29). Update it
whenever the numbers move. This is the Tier-3 companion to `tests/golden/STATUS.md`: same
spirit, but the artifact under test is a *generated* query stream compared against a **live
`walnut-java` JVM oracle**, not a fixed corpus of recorded fixtures.

The harness itself is described in `CAPTURE.md` (wire protocol, JVM recipe, comparator rules)
and `tests/soak.rs`'s module docs. **Read those before reading the numbers here** — in
particular, what counts as a `match` is decided in exactly one place, and automata are compared
by `wr_core::equiv` semantic language equivalence, never structurally (`CLAUDE.md`'s prime
directive).

## How to run it

```bash
# One batch: 30,000 queries on a named seed.
WR_DIFFGEN_QUERIES=30000 WR_DIFFGEN_SEED=0x57414c4e55545231 \
  cargo test -p wr-differential-gen --release -- --ignored --nocapture milestone_0_soak

# The harness's own live self-check (it must be able to REPORT a divergence, and it must
# survive a JVM that does not answer in time).
cargo test -p wr-differential-gen --release -- --ignored --nocapture \
  the_harness_detects_divergence_and_survives_a_hang

# The fast tier (no JVM, no oracle): PRNG, generator invariants, decoder, comparator, log format.
cargo test -p wr-differential-gen
```

Both live tests are `#[ignore]`d — `docs/DESIGN.md` §5's **gated-slow tier**. The oracle jar is
**not** vendored here; the sibling `walnut-java` checkout is resolved beside this repo or named
by `WALNUT_JAVA_DIR`, and a run with no oracle **fails loudly** rather than passing silently
(the same CI/absent-oracle contract as `tests/golden`).

The scale-up run is deliberately **several sequential single-JVM batches on different seeds**,
not a multi-process sharding launcher. Single-JVM throughput (~1,000 q/s, measured in
Milestone 0 and reconfirmed here) clears 10⁵ queries in ~2 minutes of wall clock, so sharding
would buy nothing but a second aggregation-correctness surface to get wrong. Distinct seeds buy
what sharding would not: genuine stream diversity.

## Headline numbers (measured 2026-08-15, Apple Silicon, darwin 24.6.0, `--release`)

| | |
|---|---|
| batches | **4** (4 distinct seeds, sequential, one JVM each) |
| queries generated and compared | **120,000** |
| **match** | **120,000** (100%) |
| **divergence** | **0** |
| skip-too-big | **0** |
| JVM timeouts / protocol faults / JVM errors | **0 / 0 / 0** |
| JVM restarts (and failed restarts) | **0 / 0** |
| queries never compared, for any reason | **0** |
| total query-loop wall clock | **134.6 s** (+1.7 s JVM startup across the 4 batches) |

**`docs/DESIGN.md` §8's Phase 4 exit criterion for U29 — "N ≥ 10⁵ generated queries logged with
zero unresolved divergences" — is met** at 120,000, with 0 divergences of any kind: none
resolved, none outstanding, none suppressed. There is no `KNOWN_DIVERGENCES` table in this
harness because there is nothing to put in it.

(Milestone 0's two earlier 10,000-query runs — seeds `0x57414c4e55545230` and
`0x00000000a5a5a5a5`, both 10,000/10,000 match — are recorded separately in `CAPTURE.md` and are
*not* counted in the 120,000 above.)

### Per batch

| seed | queries | match | divergence | skip | JVM restarts | wall clock | throughput |
|---|---|---|---|---|---|---|---|
| `0x57414c4e55545231` | 30,000 | 30,000 | **0** | 0 | 0 | 29.5 s | 1,016.7 q/s (0.98 ms/q) |
| `0x57414c4e55545232` | 30,000 | 30,000 | **0** | 0 | 0 | 29.6 s | 1,012.4 q/s (0.99 ms/q) |
| `0x00000000deadbeef` | 30,000 | 30,000 | **0** | 0 | 0 | 33.1 s | 907.2 q/s (1.10 ms/q) |
| `0x0123456789abcdef` | 30,000 | 30,000 | **0** | 0 | 0 | 42.4 s | 708.0 q/s (1.41 ms/q) |

Throughput drifts downward across the four batches purely from ambient machine load (each batch
is an independent JVM; the *within*-batch rate rises monotonically as the JIT warms, exactly as
Milestone 0 observed). It is not evidence of session degradation: no batch restarted its JVM,
and every batch compared every one of its queries.

### What the oracle actually answered

This is the run's evidential weight, and it is asserted rather than merely reported (see
"Anti-false-green gate" below). "120,000 match" would mean very little if they were matched
rejections.

| oracle `kind` | count | share |
|---|---|---|
| `automaton` | **110,977** | 92.5% |
| `true` | 4,707 | 3.9% |
| `false` | 4,298 | 3.6% |
| `error` | 18 | 0.015% |
| `fatal` (JVM `Error`) | 0 | — |

So ~111k of the 120k comparisons are genuine `wr_core::equiv` language-equivalence checks
against a real transition table serialized by Java's own `AutomatonWriter`, not truth-value or
string comparisons.

All 18 `error` answers were matched **byte-for-byte** by the port (the comparator uses exact
string equality, with no normalization — `CAPTURE.md` explains why). Every one of them is the
same Walnut error, confirmed by asking the JVM driver directly:

```
Variable i in the list of quantified variables is not a free variable.
	: char at 8
```

They arise where the generator textually mentions the bound variable but Walnut's evaluation
folds it out of the computed automaton anyway — e.g.
`?msd_3 (Ei ((1 > 3 => 1*i != y)) <=> y + 5 = 3)`, where `1 > 3 => …` is trivially true, so `i`
never becomes a free variable of the subformula. That surface is genuinely exercised, not
merely nominally reachable, and it agrees exactly.

### Stream diversity

| | |
|---|---|
| distinct query strings, all 4 batches pooled | **91,157** of 120,000 (76.0%) |
| distinct query strings within a batch | ~25,500 of 30,000 (~85%) each |
| numeration systems | `msd_2` 24,159 · `msd_3` 24,023 · `msd_4` 23,867 · `lsd_2` 23,967 · `lsd_3` 23,984 |
| queries with at least one quantifier | 46,840 (39.0%) |
| queries with two nested quantifiers | 8,527 (7.1%) |

Repetition is expected and is not padding: the generator's grammar is deliberately small
(`CLAUDE.md`'s "generate SMALL" guardrail), so a 30,000-query stream necessarily revisits
shapes. The honest reading is "~91k distinct formulas, each verified", with the duplicates
costing time rather than adding evidence. The near-uniform numeration split matters
independently: `lsd_*` is the port's least-exercised numeration surface (Phase 3b L1), and it
took ~48,000 of these queries.

## What this harness does NOT cover (exclusions, by construction)

`tests/golden`'s exclusions are per-fixture and recorded per id. Here the equivalent is the
**shape of the grammar**: nothing is skipped at run time (0 skips, 0 not-run), so the exclusions
are entirely up front, in what the generator can emit. Listed explicitly so the 120,000 is not
read as broader evidence than it is.

* **Only `eval "…";`** — one headless FOL query per record, `;`-terminated. No `def`, `reg`,
  `morphism`, `image`, `combine`, `transduce`, `convert`, `split`, `join`, `promote`, or any
  other `Commands/*` surface. Those are Tier-1's territory (`tests/golden`, 675 fixtures).
* **Never `::` detail-printing.** `details` log text is a *known-divergent* surface pending the
  separate `Logging`-threading follow-up (U28, `tests/golden/STATUS.md` §1), so generating it
  here would manufacture guaranteed-false divergences.
* **Only `msd_2` / `msd_3` / `msd_4` / `lsd_2` / `lsd_3`.** No custom/file-backed bases
  (`msd_fib` and friends), and none of the DROP-scope numerations (negative base, Ostrowski,
  Pell) — those are out of this port's subset entirely (`docs/BOUNDARY-MAP.md`).
* **Only the `E` and `A` quantifiers.** The `I` (infinitely-often) quantifier is not generated,
  so this run says nothing about `I`-over-`lsd`, which remains the tracked backlog item
  `CLAUDE.md` records at "could be nice to have" priority.
* **No word-automaton / DFAO tokens (`F[i]`), no macro or `$function(…)` tokens, no regexes.**
* **No variable shadowing** — `FREE_VARS` (`x`, `y`) is kept disjoint from `BOUND_VARS`
  (`i`, `j`, `k`). Shadowing is a real and interesting surface; it is deliberately deferred so
  it does not confound the first scale-up run.
* **Small by construction** — formula depth ≤ 3, quantifier nesting ≤ 2, constants in `0..=5`,
  coefficients in `1..=4`, one or two free variables. This is `CLAUDE.md`'s
  superexponential-cost discipline, not an accident: the point is many small queries, not few
  large ones.

**Consequence worth naming:** the open lsd-direction `transduce` divergence
(`tests/golden/STATUS.md` §2, fixtures 532-534) is **not reachable by this generator** — it needs
a `transduce` command over a custom base, and neither is emitted. This run neither confirms nor
clears it; it remains open, and the plan's intent that U29's live-JVM infra be the *tool* used to
bisect it stands as separate work.

## Skips, timeouts, and caps

Zero of everything, across all 120,000 queries: no JVM timeout, no Rust-side timeout, no JVM
`OutOfMemoryError`, no protocol fault, no restart. The caps are nevertheless real and enforced,
not merely declared:

| cap | value | mechanism |
|---|---|---|
| per-query JVM budget | 20 s | read deadline on the pipe; on expiry the child is **killed, restarted, and resynced**, and the query is recorded `skip-too-big` |
| per-query Rust budget | 20 s | worker thread + `recv_timeout`; the worker is *abandoned*, not killed (documented stopgap — see `rust_verdict`'s docs) |
| JVM heap | 2048 MB | `-Xmx`, so a state-exploding query becomes a prompt `OutOfMemoryError` (`skip-too-big`) rather than swapping the machine |
| Rust worker stack | 256 MiB | so a deep recursion is not mistaken for a port defect |

The abandoned-worker hazard that forced `tests/golden` to **halt** its run on the first timeout
does not apply here: headless `eval` writes nothing to disk, each query builds its own `Prover`,
and each query's staging file is named after its unique monotone `query_id`, so no two live
workers can collide. That reasoning is only load-bearing if timeouts actually occur — and none
did.

**A panic in the port is recorded as a divergence, never a skip** (real Walnut throws a
catchable exception; a process-killing panic on a plausible input is a port defect — the
U26/U27 unguarded-`act()` precedent). Zero panics occurred.

## Anti-false-green gate

`assert_healthy_run` fires after the divergence check on every batch and fails the run unless it
actually did the work: total verdict accounting, ≥ 1 match, ≥ 50% of oracle answers `automaton`,
≤ 5% skipped, zero protocol faults, zero failed restarts. All four batches passed it with wide
margins (92.5% automaton answers against a 50% floor; 0% skips against a 5% ceiling).

A failure of that gate is a **broken-harness / broken-oracle** signal, not a port defect. It
exists because "no divergences" is not on its own evidence of anything: a degraded oracle turns
every query into a skip, leaves the divergence list empty, and reports PASS having compared
nothing.

Independently, `the_harness_detects_divergence_and_survives_a_hang` was re-run against the live
JVM alongside this scale-up and passed. It proves the comparator can **fail** (the port's
`x < 4` against the oracle's `x < 3` must be reported as a divergence) and that the
kill/restart/resync path works. Without it, a clean 120,000 would be consistent with a
comparator that says "match" unconditionally.

## Divergences

**None.** Zero divergences across 120,000 generated queries on 4 seeds — nothing fixed, nothing
deferred, nothing logged as a Walnut (Java) bug, no new `docs/WALNUT-BUGS.md` entry needed (the
highest existing entry remains WB-037).

Should a future run find one, `CLAUDE.md`'s standing triage rule applies unchanged: root-cause
it; a genuine Walnut (Java) defect is logged in `docs/WALNUT-BUGS.md` and ported **verbatim**
(never silently fixed, never silently replicated), while a genuine port defect is fixed only
through the full implementer → two-independent-reviewer → fixer loop. Reproduction is cheap by
construction — the query stream is a pure function of the seed (a hand-rolled SplitMix64 with
every constant written out, not `rand`), and every run writes
`target/diffgen/run-<seed>.log` with one tab-separated line per query:

```
<seed>	<index>	<query_id>	<query>	<oracle_kind>	<verdict>[	<detail>]
```

so a divergence at index *N* of seed *S* replays verbatim, and an interrupted run restarts from
the last logged index on the same seed.
