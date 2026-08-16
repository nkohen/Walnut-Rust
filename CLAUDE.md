# walnut-rs — Agent Operating Guide

A Rust reimplementation of a **research-driven subset** of the [Walnut](https://walnut-theorem-prover.github.io/)
automatic-theorem-prover. This file tells Claude how to work here. **Read [`docs/DESIGN.md`](docs/DESIGN.md) first**
(the full plan), then [`PORTING.md`](PORTING.md) (Java→Rust idiom map) and
[`docs/ROADMAP-TO-AUTONOMY.md`](docs/ROADMAP-TO-AUTONOMY.md) (how development is meant to run on a subscription —
token efficiency, resumability, and how far to automate). This is a **derivative work of Walnut (GPLv3)**; keep it
GPLv3 and preserve attribution (see `NOTICE`).

## Prime directive: correctness

The goal is **fewer implementation bugs than Walnut**, not raw feature parity. The underlying *algorithms*
(subset construction, Valmari minimization, Brzozowski, ∃-projection) are textbook and trusted — **the PORT is what
we test.** Two rules dominate everything else:

1. **Compare by SEMANTIC LANGUAGE-EQUIVALENCE, never by byte/structural identity.** Walnut's own test suite compares
   automata with `EqualityUtils.faEqual` (Brics language equivalence), and normalizes timing out of text output. So
   the clone need **not** reproduce Walnut's exact state numbering / canonicalization — chasing that is wasted effort.
   The equivalence oracle lives in `wr-core` (product + complement + emptiness of symmetric difference) and is the
   bar for all differential + golden testing. *(This corrects the original proposal — adversarial finding F1.)*
2. **Faithful behavior, including quirks.** Mechanical port first (preserve Walnut's behavior, port its quirks/dead
   code verbatim), idiomatic Rust later in **separate** commits — so a differential regression bisects cleanly to
   "port bug" vs "refactor bug."

**When you find a genuine Walnut (Java) bug while porting — wrong output, a crash on a plausible input, not just a
quirk or dead code — do NOT silently fix or silently replicate it. Log it in [`docs/WALNUT-BUGS.md`](docs/WALNUT-BUGS.md)**
(location, trigger, how it's currently handled in the Rust port, upstream-fix status), then port it verbatim per rule
2 above unless the user has explicitly signed off on a deliberate divergence for that specific entry. This is what
makes "fix it upstream in walnut-java AND decide how to handle it in walnut-rs" a deliberate, scheduled decision
instead of something an agent quietly resolves mid-port.

## The three-repo world

- **`walnut-java`** (sibling repo, a fork of upstream Walnut) — the **oracle**: the source of the ~100%-coverage Java
  test suite (Phase 0), the differential-testing counterpart, and the golden corpus. Drive Phase 0 agents there.
- **`walnut-rs`** (here) — the port. Drive Phases 1–4 coordinator + fleet agents here.
- **`ct-research`** (downstream) — consumes this repo as a pinned `libs/walnut-rs` submodule. **Do not reach into it**;
  it only ever bumps the submodule pointer deliberately.

## Scope — the subset

**KEEP:** the full first-order-logic decider over **base-k** numeration — parser, quantifier elimination
(∃ projection+determinize, ∀ = ¬∃¬), boolean/product ops, determinize (`SC` default + plain Brzozowski), Valmari
minimize, reverse, quotient; `eval`/`def`/`reg`/`morphism`/`image`; the `.txt` automaton format (multi-track + NFA).
**DROP:** Ostrowski / Fibonacci / Pell / negative-base numeration; CAS matrix exports.
**TO CLASSIFY (Phase 0):** `split`/`rsplit`/`join`/`transduce`/`convert`/`minimize`/`fixleadzero`/… — inline commands
in `Prover.java` with no `Commands/` class; classify each KEEP/DROP against research need. See DESIGN.md §3.

## Correctness ladder (all tiers mandatory — DESIGN.md §5)

- **Tier 0** — ~100% Java unit coverage on the subset, in `walnut-java` (the executable spec).
- **Tier 1** — golden corpus (subset-filtered), compared by **semantic equivalence** (+ normalized text for details/errors).
- **Tier 2** — every Java test replicated as a Rust `#[test]`.
- **Tier 3** — differential testing vs `walnut-java` on generated queries.
- **Tier 4** — **property-based invariants** (Walnut-independent — this is what beats Walnut): `L(minimize(A))=L(A)`,
  `∀=¬∃¬`, Brzozowski double-reversal = minimal, ported-Valmari agrees with the substrate Moore minimizer.
- **Tier 5** — fuzzing + coverage (`cargo-fuzz`, seeded with malformed-input corpus).

Formal verification of the *algorithms* is **out of scope** (proves what's already trusted; says nothing about the code).

## Test-performance guardrails (superexponential-cost discipline)

The decision procedure is worst-case **superexponential**; the driver is quantifier **alternation** (each ∃ is a
projection→NFA→`determinize`). So:
- **Generate SMALL** — few states, small base/alphabet, shallow alternation. Bugs show up on 5-state automata.
- **Both engines must finish** — differential generators are size-bounded by the slower (JVM) engine; over-budget
  cases are never emitted.
- **Per-test resource caps, never hangs** — every end-to-end test has a wall-time + peak-state + memory cap and yields
  a `TIMEOUT`/`EXPLODED` verdict (mirror ct-research's `walnut-guard`), recorded visibly as `skip-too-big`.
- **Two tiers** — fast (tiny inputs, every commit) + gated-slow (heavier corpus/differential, capped & sharded).

## AI-orchestration (Bun-style adversarial loop)

- **Follow [`PORTING.md`](PORTING.md)** — the reviewed Java→Rust idiom map. Its entries are defaults; deviations must
  be justified in the diff. Hit a pattern it doesn't cover? Add the ruling there before porting the third occurrence.
- **Loop:** implementer → **two split-context adversarial reviewers** (`.claude/agents/adversarial-reviewer.md`, given
  only the diff, told "assume a mathematical/implementation bug exists; find it") → fixer.
- **Protect the split context.** Dispatch each reviewer with **only the diff + file paths** — never the author's
  commit message, rationale, or "why this is correct." Handing over the author's reasoning silently defeats
  independent review (the Bun lesson).
- **Reviewer model ≠ author model** for any trust-critical (math / decision-procedure) code — a same-model reviewer
  shares the author's blind spots.
- **Reconciliation:** a single `correctness-fatal` or `correctness-risk` from **either** reviewer blocks the merge until
  resolved; "signed off" means **both** reviewers returned no unresolved correctness finding. If the two disagree on a
  load-bearing *fact* (not just wording), the coordinator adjudicates the math firsthand — do not average them.
- **Model-tiering — but know the MECHANISM (it is not free per-unit).** A single session runs on ONE model; you
  cannot cheaply switch it per unit — a mid-session model switch invalidates the prompt cache and re-sends the whole
  accumulated context uncached (expensive). So tiering is done two ways, and you must pick deliberately:
  (1) **run the session on the tier that fits the bulk of the current phase** (Haiku/Sonnet for a mechanical-port
  stretch; you launched it, it stays); (2) **delegate a genuinely isolable large unit** (a whole file/test class, not
  a single method) **to a fresh `Agent` subagent with a `model` override** — this avoids the cache tax but pays a
  cold-start context cost, so it only pays off for units big enough that the cheaper per-token rate outweighs
  re-establishing context. **Escalate in batches, not per-unit.** Reserve Opus for the hard ~20%: parser +
  `NumberSystem` edge cases, quantifier-elimination correctness, adversarial math review, diagnosing differential
  divergences, and architecture/crate-boundary calls. (Directionally, cheaper tiers stretch your session budget
  further — but treat that as a tendency, not a measured multiplier for your account.)
- **Compiler as work queue** — fix errors crate-by-crate, batch similar errors. Mechanical commits before idiomatic ones.
- **THE MERGE GATE (hard rule):** never commit or merge with `cargo test --workspace` **red**, and never merge
  trust-critical-crate (`wr-core`/`wr-logic`) code without the two-reviewer loop having run. Zero tests deleted, ever
  (a failing ported test is a real signal — fix the port, don't delete the test).
- **Cost control** — a per-phase budget ceiling; the deterministic correctness pipeline (running suites, corpus
  diffing, fuzzing) costs zero model tokens — keep it there.

## Token efficiency & context management (this is a SUBSCRIPTION — tokens/session-limits, not $)

The constraint here is your subscription's token/session budget, not a dollar bill; session limits are *pauses*, not
failures. The biggest avoidable cost is a **bloated main context re-sent every turn**. Keep the coordinator thin:

- **Delegate heavy-in / small-out work to a fresh subagent and keep only the conclusion** — reading a 1000-line Java
  file, running a coverage pass, sweeping the corpus. (This does not *reduce* total tokens — the subagent's tokens
  count against the same subscription — but it keeps the *main* context small so it isn't re-sent every subsequent
  turn, which is the real recurring cost.)
- **Never read a large Java source file wholesale into the coordinator.** `Grep`/`Explore` to locate; read only the
  slice you port. (`NumberSystem.java` is 1,027 lines — reading it whole is a large avoidable cost.)
- **Route long output to files, not context** — test logs, coverage reports, diffs → disk; grep them.
- **Trigger delegation on task SHAPE, not on "context feels large"** — by then you've already paid and re-pay each turn.
- **MCP hygiene (a minor lever — do it, don't overrate it).** Tool *schemas* load on demand (deferred), so an unused
  server does NOT cost per-turn schema tokens. What it *does* cost is a one-time per-session "instructions" block.
  So: don't connect servers you won't use here (walnut-rs is Rust — it does NOT need ct-research's Lean `lean-lsp`
  MCP), but this is hygiene, not a major lever — §the two bullets above dwarf it.

Full treatment (incl. the resumability plan and the autonomy ladder) is in
[`docs/ROADMAP-TO-AUTONOMY.md`](docs/ROADMAP-TO-AUTONOMY.md).

## Fleet / concurrency hygiene

On a **subscription**, concurrent agents share ONE quota — running N at once does not save tokens (spend is additive)
and hits your session/rate limit ~N× faster; it only buys wall-clock. So prefer **in-session subagent delegation**
and **modest** concurrency (a few agents), not a large fleet. Whenever >1 agent DOES run concurrently against a shared
tree, git collisions corrupt work — hard rules (learned the hard way in
the Bun rewrite AND in this operator's `ct-research` fleet):

- **NEVER `git stash` or `git reset`** during fleet operation — they silently discard or rewrite another agent's work.
- **Atomic, pathspec-scoped commits** — `git commit -F - -- <explicit paths>`; never `git add -A` then a bare commit
  (it can sweep a concurrent agent's staged files under your message). One logical change per commit.
- **Each agent works in its own clone / worktree on an `agent/<name>` branch**; merge back deliberately. Do not share
  a working tree between agents.
- **Container agents run FOREGROUND, in-turn** — a `claude -p` run that backgrounds a job and ends its turn gets the
  job killed when the `--rm` container tears down. Verify the *deliverable* (commit landed), not the exit code.
- **Watch disk/IOPS** — many parallel `target/` builds + clones exhaust disk and I/O at scale; cap concurrency.

## Guardrails to adapt (not yet mechanical)

The rules above are enforced by convention until the hooks in [`.claude/hooks/README.md`](.claude/hooks/README.md) are
ported from ct-research (a Phase-0 task): a commit gate on the trust-critical crates, a recursive-`rm`/backgrounded-job
safety guard, and a pre-push `cargo test` check. Follow the AUTHORING discipline (self-test all branches, fail-diagnosable)
when wiring them.

## Running the tools

```bash
cargo build --workspace
cargo test  --workspace          # fast tier
cargo fmt --all && cargo clippy --workspace --all-targets
# cargo fuzz run <target>        # Tier 5 (once fuzz targets exist)
```

## Git & licensing

- **GPLv3-or-later.** Every new source file carries the SPDX header
  `// SPDX-License-Identifier: GPL-3.0-or-later` + the derivative-of-Walnut line (see existing stubs). Preserve
  Walnut's copyright where code is ported from it; add your own copyright for new code.
- Commit messages: single-quoted `-m` or `-F`/heredoc (never a double-quoted `-m` — zsh expands backticks). End with:
  `Co-Authored-By: Claude <noreply@anthropic.com>`.
- Commit/push on the user's request; scope commits with explicit pathspecs.

## Current status

Phase 0, W7 items 1-7 **done** (see `walnut-java/phase0-artifacts/PROGRESS.md` for full history):
TO-CLASSIFY commands classified (`docs/BOUNDARY-MAP.md` §6), JaCoCo scoped to the KEEP subset, baseline +
characterization-test coverage driven to 98.2% line / 94.0% branch across three waves (incl. the
trust-critical algorithmic files), test manifest exported (`walnut-java/phase0-artifacts/test-manifest.json`,
675 fixtures; 591 of them subset-relevant per `phase0-artifacts/subset-filter.json`), and the **OTF
empirical check** (DESIGN.md §9 F3) run and resolved — **decided: defer the entire OTF-family
determinization surface** (`CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`/`OTF()`), confirming the original KEEP-only-
`SC`+`BRZ` plan; full evidence + a follow-up scope-comparison (sizing what a smaller cut would cost, in
case the deferral is ever revisited) in `walnut-java/phase0-artifacts/PROGRESS.md`'s Item 7 entries and
`docs/DESIGN.md` §9/§10. **Phase 0 is complete.**

**Phase 1 spike done** (7 commits, `e36e8f7..52e0bcf`; plan at `.claude/plans/fluttering-foraging-spindle.md`):
`wr-core`'s `Fa` representation, the semantic-equivalence oracle, subset-construction `determinize`,
`Trimmer`, a faithful port of Valmari minimization (authored by an Opus subagent per the model-tiering
doctrine — found and documented a genuine Walnut bug in the process, the "q0 aliasing quirk", see
`minimize.rs`'s module docs), a minimal `Automaton` wrapper + `RichAlphabet` encoding + `less_than_msd`,
`wr-logic`'s `exists` (∃-quantification + the mandatory leading-zero fixup, also Opus-authored — two
adversarial-reviewer passes each for the two Opus-authored commits caught and fixed a real reachable
panic), and a minimal `wr-io` `.txt` reader. **Exit criterion met**: `tests/differential` differentially
checks `∃i (i<x)` over msd base-2 against real `walnut-java` output via the Rust-native equivalence
oracle — green. `cargo test --workspace` is green throughout; `cargo fmt`/`clippy` clean.

**Phase 2 complete** (plan at `.claude/plans/toasty-napping-wall.md`, adversarially reviewed before
execution — the first draft's unit ordering was dependency-backwards in several places, corrected before
any code landed). Toolchain bumped to current stable first (U0, `206413a`). All units landed, each
through the full implementer → two-independent-reviewer (model always different from the author) →
fixer loop:
- **U1** (`23e030d`): `fa.rs` completion — `reverse`, `star_states`/`concat_states`, `canonicalize`. Found
  and logged two genuine Walnut bugs in `concatStates` (WB-008/WB-009, both confirmed live against the
  real `walnut-java` CLI during review).
- **U2** (`3524b95`): `automaton.rs`'s self-contained surface — `bind`/`sort_label`/`canonize`/
  `determinize_and_minimize`/`AutomatonDFA`. Surfaced a live call site for WB-001 (the Phase-1
  Valmari-minimize quirk) through `determinize_and_minimize`'s already-deterministic branch — faithfully
  ported (not fixed), pinned by a dedicated test, WB-001's entry updated.
- **U3** (`e220cda`): `product.rs` — the cross-product BFS behind the boolean connectives, via a local
  `BooleanOp` enum (not a full `Token`/`LogicalOperator` port) generic over the output-combining function
  so later units (`combine`/`join`/eval) can reuse the same primitives.
- **U4** (`f12aea1`): Brzozowski double-reversal determinize (`BRZ`) in `determinize.rs`.
- **U5** (`504acb8`): `logicalops.rs` — `AutomatonLogicalOps`'s boolean ops, quotients, zero-fixups,
  `reverse` (Opus-authored — flagged as subtle as `NumberSystem`; found a genuine Walnut bug in
  `leftQuotient`'s alphabet-subset guard, WB-010). `convertNS` stays out of scope (genuine, unconditional
  `WordAutomaton` dependency, not a DFAO-only branch).
- **U6** (`23c2e09`): an architecture unit — moved `AutomatonQuantification`'s ∃-projection primitive
  from `wr-logic` down into `wr-core::quantify` (with `wr_logic::quantify::exists` now a thin wrapper),
  because `wr-core`'s own `NumberSystem` needs to call it 10× and `wr-logic` must depend on `wr-core`, not
  the reverse — a real incoming-edge coupling `docs/BOUNDARY-MAP.md`'s original analysis missed (now
  corrected there and in `docs/DESIGN.md`'s crate-mapping table).
- **U7** (`0fe14f7`): `numsys.rs` — `NumberSystem`'s positive-base msd/lsd surface (addition, comparison,
  base-change/constant automata, arithmetic/multiplication/division), Opus-authored as the largest,
  hardest-reasoning file in this phase. Negative-base logic deleted outright (`docs/BOUNDARY-MAP.md`
  §4.1's already-made call), base-change dropped entirely (its sole real caller is the DROP-scope `split`
  command), file-backed custom bases (`msd_fib`, …) confirmed out of scope and deferred to `wr-io`.
- **U8** (`7abb42a`): hardened the equivalence oracle — added `Automaton`-level track-structure checking
  and De Morgan property tests. Adversarial review found the new function's own doc overclaimed what it
  caught (label-permuted tracks with identical per-position alphabets are NOT detected and get a silently
  wrong verdict — a real, common-case gap, now honestly documented and pinned by a dedicated test rather
  than the closed claim an earlier draft made).
- **U9** (`0e0be83`): completed the Tier-4 core property suite — audited what already existed (most of
  DESIGN.md §5's named properties landed incidentally during U1–U8) and filled the one gap, quantifier
  duality (`∀y φ ≡ ¬∃y ¬φ`), checked against an independent brute-force oracle.
- **U9a** (`4ddc930`): the cross-oracle minimizer check. DESIGN.md's plan to reuse
  `RustConstantTermSequences`'s "Moore minimizer" for this turned out to rest on a function that doesn't
  exist in that repo (verified against its full source and git history) — and a deeper mismatch besides
  (its `DFAO` type is single-track only, `wr-core`'s automata are not). Per the user's explicit decision,
  authored a small standalone Moore minimizer from scratch in `wr-cts` instead (fresh code, not a port —
  `CLAUDE.md`'s mechanical-port rule doesn't apply), cross-checked against the ported Valmari minimizer
  and a from-scratch brute-force Myhill–Nerode oracle.

**Phase 2 exit criterion met**: Tier-2 tests and Tier-4 core invariants are green across `wr-core`
(fa/automaton/product/determinize/logicalops/quantify/numsys/equiv) and the new `wr-cts` cross-check.
Four new genuine Walnut (Java) bugs found and logged during the port (WB-008–WB-010, all in
`docs/WALNUT-BUGS.md`, all ported verbatim per the mechanical-port rule, not fixed). `cargo test
--workspace` is green throughout; `cargo fmt`/`clippy` clean.

**Phase 3a complete** (all 22 units landed on `master`, plan at
`.claude/plans/synthetic-prancing-aurora.md`, adversarially reviewed before execution per this
project's now-standard practice; execution history and process notes in `RESUME-HERE.md`). Built
the full FOL decider: `wr-logic`'s parser/precedence/lexer (`Token`/`Operator`/`Expressions`,
macro/word/function token construction), the quantifier-elimination driving logic
(`LogicalOperator`'s `E`/`A`/`I` dispatch — **the decision-procedure crux of the whole port**),
relational/arithmetic operand semantics, the shared postfix-token executor (the Phase 3a
*integration* checkpoint — the first proof the parser+quantifier-elimination+operand-semantics
stack genuinely composes, not just that each piece passes its own isolated tests); `wr-core`'s
regex engine (a hand-rolled Brics-dialect parser + Thompson construction, transliterated from the
real `dk.brics:automaton` sources, the largest single unit this phase), `WordAutomaton`,
`Infinite`, custom-base `NumberSystem` file loading, and the `TRUE_FALSE_AUTOMATON`/`TRUE_AUTOMATON`
retrofit (closed a blocking Phase-2 gap — 13% of golden fixtures are literally `true`/`false`);
`wr-io`'s writer (`.txt`/`.gv`/`.ba`) and reader extensions (custom-base headers, transducers,
comments); and `wr-cli`'s `Session` (the first real file-backed `PredicateEnv` impl), `eval`/`def`
(`EvalDef`), `reg`/`alphabet`. **16 new genuine Walnut (Java) bugs found and logged this phase**
(WB-011 through WB-021 and WB-023 through WB-027; WB-022 is a Rust-port scope gap, not a Java
bug — see `docs/WALNUT-BUGS.md`), all ported verbatim per the mechanical-port rule. **Exit criterion
met**: a consolidated differential suite (`tests/differential/tests/phase3a_checkpoint.rs`)
exercises `eval`/`def`/`reg`/`alphabet` together through `wr-cli`'s real public library API —
boolean connectives + quantifiers in combination, a custom base, word/function tokens with
quantifiers, TRUE/FALSE results, the CAS-validation success path, a `reg`-then-`alphabet`
pipeline — compared against real `walnut-java` CLI output via `wr_core::equiv`, zero genuine
divergences found. `cargo test --workspace` green throughout (920+ tests); `cargo fmt`/`clippy`
clean.

**Phase 3b, L1 — `lsd_*` numeration (the one limitation Phase 3a left open) is RESOLVED.**
Phase 3a's exit checkpoint surfaced it as "`eval`/`def`/`reg` over `lsd_*` combined with any
quantifier fails with `QuantifyError::UnsupportedLsdFixup`"; investigation for this unit found
the blast radius was substantially wider than that framing. `wr_core::numsys` calls
`quantify` ten times to build its *own* automata, so on an `lsd_k` system every comparison or
arithmetic against a constant `>= 2`, and every `get_constant`/`get_multiplication`/
`get_division`, failed as well — `?lsd_2 x >= 2`, containing no user-written quantifier at all,
was already broken. Root cause was a single Phase-2 (U6) scope cut: `quantify`'s `Some(false)`
arm returned an error instead of calling `AutomatonLogicalOps.fixTrailingZerosProblem`, which
U5 had already ported. That module's own docs flagged the flip as "an explicit, separately
reviewed change" and justified the deferral partly on "no lsd numeration system exists in
`crate::numsys` to exercise it against yet" — a clause that went stale one unit later, when U7
landed `NumberSystem`'s full msd/lsd surface. L1 is that separately-reviewed change: the arm is
now the plain port of Java's `AutomatonQuantification.java:46`, the now-unconstructible
`UnsupportedLsdFixup` variant is deleted, the four tests that pinned the rejection are flipped
to assert the computed language (none deleted), and the positive coverage that never existed is
added — `wr-core`'s `quantify_on_an_lsd_automaton_runs_the_trailing_zero_fixup` (the two fixups
contrasted on one transition table, since they are genuinely *not* mirror images), `numsys`'s
`lsd_composed_constructions_compute_the_right_language` and the new Tier-4
`msd_and_lsd_composed_constructions_agree_after_reversal`, `wr-logic`'s
`lsd_numeration_evaluates_end_to_end`, and a new differential suite
(`tests/differential/tests/lsd_numeration.rs`, five cases incl. a three-track `lsd_3` one)
against freshly captured real `walnut-java` output — zero divergences. No new Walnut (Java) bug
found: Java's asymmetry between the two fixups (leading closes under prepending zeros; trailing
only right-quotients) is correct in context and is ported verbatim.

**Scope of what "RESOLVED" actually covers** (narrowed after adversarial review found the first
draft of this note overclaimed): verified for `lsd_k` (not custom-base `lsd_fib`/similar) through
`eval`/`def` — `∃` (genuine quantifier elimination) and closed `∀` (the `¬∃¬` path) both confirmed
correct against real `walnut-java`. **Not verified**: the `I` (infinitely-often) quantifier over
`lsd` — it dispatches through `wr_core::infinite::infinite`, a code path that never calls
`quantify` at all, so this fix neither touched nor tested it; and `reg` over `lsd_*`, which also
never routes through `quantify` (Thompson construction + determinize only) and has its own,
separate, pre-existing `lsd` coverage in `reg_and_alphabet_commands.rs`. **User priority signal
(2026-08-13): `I`-over-`lsd` is explicitly "could be nice to have," not a hard requirement like the
`lsd_k` fix above was — the user chose to leave it as a tracked backlog item rather than fix it
immediately. Revisit when picking up related work or if it starts blocking something, not before.**

**Phase 3b, U17–U26 complete** (11 of 12 units; plan re-verified against current code before
starting per `docs/ROADMAP-TO-AUTONOMY.md`'s phase-gating — corrected-plan addendum at
`~/.claude/plans/zany-sauteeing-pudding.md`, outside this repo; execution history and process
notes in `RESUME-HERE.md`). Landed the remaining `wr-core` primitives (`Morphism`, `convertNS`,
`Search/ProductBFS`, `Transducer`), the real `Prover.java` dispatch/REPL/`MetaCommands` (wiring
real values into Phase 3a's U0c hook), and all remaining `Commands/*`
(combine/concat/union/intersect/star/reverse/quotient/describe/minimize/fixleadzero/fixtrailzero/
macro/morphism/image/promote/join/convert/inf/export/test/transduce/help). Each unit went through
the full implementer → two-independent-reviewer → fixer loop; **two correctness-fatal bugs found
and fixed** (`convertNS`'s exponent computation used Rust's correctly-rounded `f64::ln` where
Java's `Math.log` is not correctly rounded, silently diverging on ~150-340 ordinary bases; and
`Morphism::to_word_automaton` dropped Java's `NumberSystem` validation, letting `promote` silently
succeed on inputs — e.g. any 1-uniform morphism — where real Java cleanly errors), plus several
correctness-risk bugs (silent fail-open on custom-base number-system mismatches in
`union`/`concat`/`intersect`, now fixed via real per-track NS-name threading through the
reader/`Automaton`/writer; multiple process-killing panics on ordinary mismatched-input commands,
now guarded; an ineffective resource cap on `transduce` replaced with a measured, deterministic
budget inside the primitive itself). **10 new genuine Walnut (Java) bugs found and logged this
phase** (WB-028–WB-037), all ported verbatim per the mechanical-port rule. `cargo test --workspace`
green throughout; `cargo fmt`/`clippy` clean.

**Phase 3b, U27 complete — Phase 3b is now fully done (12 of 12 units).** Built the Tier-1
golden-corpus harness (`tests/golden/`, plan/prompt at the session's own scratchpad, execution
history in this section and `tests/golden/STATUS.md`), the actual DESIGN.md Phase-3 exit
criterion: replays all 675 of Walnut's own integration-test fixtures through the real
`wr-cli` dispatch path (`Prover::dispatch_for_integration_test`, never a subprocess) against a
throwaway copy of the corpus's own library trees, comparing automata by `wr_core::equiv`
semantic equivalence (never structural/byte identity) and `details`/`error` fixtures by a
verbatim port of Java's own `IntegrationTest.assertEqualMessages` normalization / exact string
match. Went through the full implementer → two-independent-reviewer → fixer loop **twice**
(a second review round on the first fix commit surfaced two more real, if low-blast-radius,
correctness-risk gaps — see `tests/golden/STATUS.md` and the commit trail `b9d2bd5..2a00443`
for the complete finding-by-finding history). **Four genuine port bugs found and fixed**
(custom-base `$name(...)` resolution skipped `Session`-backed NS construction; the reader
dropped a custom-base library file's valid-representation restriction, `all_reps`; `convert`
parsed its source base from the alphabet instead of the name; an unguarded `act()` panic could
kill the process on a plausible input) — **no new `WB-` entries were needed, all four were port
defects, not Walnut defects** (highest entry remains WB-037). Review rounds also fixed: a
harness exclusion-classifier gap that could silently miscount a read-only (`describe`/`inf`/
`test`) or `$`-prefixed (`convert $name`) fixture's dependency on DROP-scope output; a
per-fixture timeout that was measured but not actually enforced (now a real spawned-thread +
`recv_timeout` mechanism per CLAUDE.md's "never hangs" guardrail); a silent divergence from
Java's `NumberFormatException` crash on an (unreachable-in-practice) integer-overflow base name,
now a distinguishable `ConvertNsError::BaseOverflowsInt`; and a timeout-taint gap where a
timed-out fixture's abandoned, unkillable worker thread could keep mutating the shared session
tree that every later fixture reads — the run now **halts** on the first timeout, marking every
remaining fixture `NotRun` rather than reporting an untrustworthy verdict. **Result: 573 of 583
compared fixtures pass (98.3%)**; 92 fixtures excluded with a per-id recorded reason (DROP-scope,
deferred-OTF, transitive-drop-dependency, unwired metacommands); 0 over-budget/not-run. The
**10 remaining known divergences** (pinned in `KNOWN_DIVERGENCES`, not silently skipped) are two
root causes, both pre-existing and already flagged before this unit, not new: (1) **7 `details`
fixtures** — every state count already matches Java exactly, including pre-minimization counts;
only the per-`act()` `Logging` call trace isn't threaded through `wr-core`'s product/determinize/
minimize/quantify yet (`wr-logic/src/eval.rs` already flagged this as owed; suggest a follow-up
unit, tentatively "U28", to thread `&mut Logging` through); (2) **3 `transduce` fixtures** — a
reversed (**lsd**) custom-base DFAO diverges specifically in `Transducer::transduceNonDeterministic`'s
reverse-input/reverse-result branch (the same-transducer **msd**-direction case passes), open
and not yet root-caused past that. `cargo test --workspace` green throughout (20+ suites);
`cargo fmt`/`clippy` clean. The corpus replay itself is `#[ignore]`d (gated-slow tier per
DESIGN.md §5) — run it with `cargo test -p wr-golden --release -- --ignored --nocapture`.

**Phase 3b is complete.** Everything DESIGN.md's Phase 3 exit criterion asked for
("eval/def/reg, and now everything else, work," verified Tier 1 green) is now true.

**Phase 4 (Hardening) is underway** — the user gave the go-ahead 2026-08-15. Plan at
`~/.claude/plans/purrfect-doodling-muffin.md` (outside this repo), adversarially reviewed by an
independent Opus agent before any code landed (caught 3 blocking design defects — a fatal
thread/halt timeout mismatch, an unhandled JVM-hang pipe-desync hazard, a weakened error
comparator — plus several significant scope errors, all fixed in the plan before execution
started). Four units: **U29** (Tier-3 differential-generator harness, at scale), U30 (Tier-5
fuzzing), U31 (Tier-4 property-suite completion), U32 (performance vs JVM Walnut).

**U29 complete.** A two-sided harness (`tests/differential-gen/`) drives one long-lived
`walnut-java` JVM through `Prover.dispatchForIntegrationTest` (headless `eval`, writes nothing to
`Automata Library`) over a NUL-delimited pipe protocol, and compares its answers against the
Rust port's own `Prover::dispatch_for_integration_test` for each of a large stream of randomly
generated small KEEP-subset queries (base 2-4 msd/lsd, quantifier depth ≤2, formula depth ≤3) —
automata via `wr_core::equiv` semantic equivalence, errors by exact string match, with a
query-ID echo check that turns any pipe desync into a loud failure instead of a phantom
divergence. Milestone 0 (10,000 queries, commit `4ca773c`) went through the full
two-independent-reviewer loop (Opus + Sonnet, split-context); both reviewers independently found
the same headline defect — the soak test's pass/fail gate only checked for divergences, so a
degraded/dead oracle would report a false "PASS" having compared nothing — plus Opus found two
more real, evidence-backed bugs (`Answer::Fatal`/OOM never triggered the JVM restart the code's
own docs claimed it did; a shared non-unique temp-file path that could race an abandoned timed-out
worker against a later query and silently mask a real divergence as a match) and a coverage gap
(the query-ID echo check — described as "the load-bearing invariant" — had zero regression tests;
mutation-testing proved the existing suite couldn't detect its removal). All fixed in the
follow-up commit `b6e9b3a`. Scaled to **120,000 generated queries across 4 seeds: 120,000 match,
0 divergences, 0 skips** (commit `3c3d852`, `tests/differential-gen/STATUS.md`), meeting U29's
exit criterion (N≥10⁵, zero unresolved divergences) — ~92.5% of comparisons were real
`wr_core::equiv` automaton checks (not vacuous error/error matches), ~76% distinct query strings,
near-uniform numeration-system spread including heavy `lsd_*` coverage. No new genuine Walnut
(Java) bug found (`docs/WALNUT-BUGS.md` unchanged at 37 entries). **Known, explicit non-coverage**
(by construction, not oversight): this generator never emits `::`-detail-printing queries,
`transduce`/`def`/`reg`/other `Commands/*`, the `I` quantifier, custom bases, or word/macro/
function tokens — so it neither confirms nor clears the two pre-existing Phase-3b follow-ups
(`details`-fixture `Logging` threading; the lsd-`transduce` divergence,
`tests/golden/STATUS.md`), which remain open, separately-scoped work. No multi-process sharding
launcher was built — measured single-JVM throughput (~1,000 q/s) clears 10⁵ in ~2 minutes, so
distinct seeds gave run diversity without adding an aggregation-correctness surface for no
throughput benefit.

**U30 complete** — Tier-5 fuzzing (`fuzz/`, `cargo-fuzz` + ASAN, working on Apple Silicon via an
`rust-lld` linker workaround documented in `fuzz/README.md`). Three targets (`wr_io_reader`,
`wr_logic_parser`, `wr_core_regex`) with real seed corpora, all clean at millions of executions.
This unit ran an unusually long implementer → adversarial-review chain — **five review rounds**,
each finding a real bug, which is the process working as intended (CLAUDE.md's merge gate exists
precisely so this class of finding doesn't ship silently) rather than a sign anything was
rushed. Cumulative result across commits
`4fc968f`→`b230db4`→`9a26f37`/`f60f086`→`4ea178b`→`e8258c7`:
- **4 fuzz-discovered process-killing panics fixed** (i32-overflow parses, an undeclared-dest-state
  guard, an out-of-alphabet-digit encode guard, an unvalidated numeration base) — all confirmed
  genuine PORT bugs (Java's `Prover.readBuffer` catches and recovers; the Rust port didn't), not
  Walnut bugs, verified against the real jar rather than inferred.
- **WB-038 logged**: `AutomatonReader` silently accepts an out-of-alphabet transition digit,
  encoding it to a bogus `-1` key that either silently corrupts the automaton's language (no
  diagnostic) or crashes on a later write — a genuine Walnut (Java) bug, ported verbatim.
- **A real architectural fix**, not a patch: a top-level panic-recovery boundary at command
  dispatch (`Prover::caught`, mirroring `Prover.readBuffer`'s `catch (RuntimeException)`), after
  review found the first attempt at guarding individual `encode`/`decode` call sites had merely
  *relocated* the panic to six other commands (`union`/`intersect`/`join`/`inf`/`test`/`reverse`).
  `Automaton::decode` made Java-faithful (bounds-checked truncating division) in the same pass.
- **An unrelated correctness-fatal bug found and fixed as a byproduct**: `reverse`'s `flip_ns`
  flipped the `msd`/`lsd` boolean but left `Automaton::ns_name` stale, so the output header (and,
  worse, a later `union`'s numeration-mismatch guard) used the wrong number system — confirmed
  live to let a genuinely mixed-numeration `union` silently succeed where real Walnut correctly
  refuses. Fixing this **closed the previously-open lsd-`transduce` golden-corpus divergence**
  (fixtures 532-534, `tests/golden/STATUS.md` §2, tracked since Phase 3b) — it was never a
  `Transducer` bug, it was this. **Golden corpus moved from 573/583 to 576/583.**
- **`wr-io`'s header parser unified** onto the same `parse_methods` grammar primitives the rest of
  the reader already used, closing three real divergences the unification itself surfaced (missing
  alphabet dedup, an over-restrictive `{...}` set grammar, Unicode-vs-Java's-ASCII-only `\s`
  whitespace handling) plus, in the final round, Java's regex `$`/`.` leniency around rare Unicode
  line terminators (NEL/LS/PS) and an `alphabet_size` overflow that panicked in debug and diverged
  from Java's `Math.multiplyExact`-throws behavior in release.
- Golden corpus, differential (20,000+ queries), and all three fuzz targets re-run clean after
  every round; `cargo test --workspace` green throughout (1300+ tests); `fmt`/`clippy` clean
  (workspace and `fuzz/`'s own nightly toolchain). Known remaining gaps documented in code, not
  silently dropped: a lone-`\r` line-splitting divergence (no corpus/fuzz reach), the `details`-
  fixture `Logging`-threading gap (7 known golden divergences, unrelated, pre-existing).

**U31 complete** — Tier-4 property-suite completion (`f754a91`→`16a62bc`→`a54e33e`→`76e5ff9`→
`b740b27`), ~17 new property tests across `wr-core`/`wr-logic`/`wr-cli` covering the confirmed
gaps: quotients (with WB-010's guard correctly modeled, not asserted away), `convertNS` (a
JVM-captured sweep widened to `root ≤ 46340`/48,036 pairs, plus one genuine non-circular
power-of-2 property — a naive property test here would have been actively wrong, per the plan's
explicit correction, since WB-032 is a deliberately-ported-verbatim quirk), `Morphism` (generator
constrained away from WB-036's trigger, validation-order checked against real Java source),
`fixleadzero`/`fixtrailzero`, `NumberSystem::Div` + `msd_fib` custom-base round-trips, the lsd
trailing-zero fixup (finally property-tested, not just example-tested — the msd/lsd asymmetry
confirmed real, not a mirror image), and `Transducer` (constrained away from WB-035, oracle
independently re-derived from real Java source rather than from the port's own logic). **Zero new
genuine bugs found** — WB-038 remains the highest entry.

This unit's own review chain (two rounds, both with real findings) is worth recording because the
findings were about **test strength itself**, not production logic — the first round found two
property tests that passed even when the primitive under test was replaced with a trivially wrong
implementation (asserting only one-directional closure, never an upper bound — confirmed by
literally swapping in a broken implementation and watching the tests stay green), a WB-001 skip
predicate that was sound but silently over-broad with an inaccurate doc claim, and a WB-010 panic
check that swallowed *any* panic across ~37-39% of its generated cases, not just the documented
one. Fixed with exact-characterization oracles (independently re-derived, mutation-verified) and a
correctly-tightened WB-001 precondition (rejection rate dropped from 30% to ~11% with zero cases
wrongly admitted, verified exhaustively over thousands of cases). The second round caught a small
irony: the fix for "stop silently skipping cases" itself left one more instance of exactly that
pattern unconverted (the highest-skip-rate one, ~37%) — fixed, plus a coverage gap where
`fixtrailzero`'s WB-001 path lost its only test after an unrelated trimming fix, closed with a
hand-built pin verified live against the real jar. `cargo test --workspace` green throughout
(1434 tests); `fmt`/`clippy` clean.

Next: **U32** (performance vs JVM Walnut) — the last unit in Phase 4's plan. See the plan file for
full scope; needs an execution-time decision on benchmark sourcing (the plan's two documented
options — golden-corpus throughput as the exit proxy, vs. first wiring `[strategy N NAME]` to
unlock `thm5`-class fixtures; default is the former).
