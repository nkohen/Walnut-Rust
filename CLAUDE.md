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
minimize, reverse, quotient; `eval`/`def`/`reg`/`morphism`/`image`; the `.txt` automaton format (multi-track + NFA);
CAS matrix export (`AutomatonMatrixWriter` + the Maple/MATLAB/Mathematica/Sage emitters, `wr_io::matrix_writer`,
ported 2026-08-19 per `docs/CAS-EXPORT-DISPATCH.md`); **Ostrowski numeration** (`ost` — `wr_core::ostrowski` +
`wr_cli::ost`, ported 2026-08-20 per `docs/OSTROWSKI-DISPATCH.md`) — see "Current status" below for both.
**DROP:** Fibonacci / Pell / negative-base numeration. (Fibonacci/Pell were never separate code — they are
ordinary custom-base data files running through the already-KEEP loader; see `docs/UNPORTED-SCOPE-SIZING.md`.)
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

**U32 prerequisite complete — `[strategy N NAME]`/`[export N FORMAT]` now actually work.** The
user explicitly chose the larger of U32's two documented benchmark-sourcing options (wiring
`[strategy N NAME]` through, to unlock `thm5`-class fixtures for a genuine slow-workload
comparison rather than relying only on the golden corpus's uniformly-fast fixtures), anticipating
increased library usage upon performance upgrades. Previously these metacommands were fully
*parsed* by `MetaCommands` (which already implemented `wr_core::determinize::DeterminizeContext`)
but never *threaded* anywhere — every determinization silently used `SC` regardless of what a
query asked for. Now a real `Option<&mut dyn DeterminizeContext>` flows from `Prover`'s dispatch
through `wr-logic`'s eval path, `wr-cli`'s `PredicateEnv`/library-automaton loading, down to every
`wr_core::determinize::determinize` call site — gated exactly on Java's real `printDetails`
(`::`-suffix) condition, with `printEnabled` modeled structurally (`wr_core::numsys` never gets a
context, matching Java's stated intent that internal `NumberSystem` constructions stay silent).
Commits `63e7e46`→`e07beb3`/`3a39ce4`→`6e7ce6f`. This went through **three full adversarial-review
rounds**, each finding real issues, converging cleanly:
- **Round 1** found two live-reproduced correctness-risk bugs beyond the initial implementation:
  loading a library automaton (`$name(...)`) inside a `::`-suffixed query desynced every later
  metacommand index (Java counts the load-time determinize, the port didn't); and the commit had
  deleted a U21-era tripwire test that used to catch "parses and silently discards" bugs, silently
  reintroducing that exact failure mode for the (deliberately still out-of-scope) non-`eval`/`def`
  commands. Also corrected **WB-039**'s documented scope (the underlying Java bug — nested
  `disablePrint`/`enablePrint` not being save/restore — also fires in `NumberSystem.multiplication`,
  not just `constant`; the user's sign-off on keeping the port's stable indices stood unchanged).
- **Round 2** fixed both via real `PredicateEnv` context-threading (not a workaround) and a
  restored tripwire, plus surfaced **WB-040** (Java's `[export gv]` mutates the automaton
  mid-determinize — confirmed unreachable in the port's current call shape) — but its own review
  found the golden-corpus harness's just-tightened `KNOWN_DIVERGENCES` gate still tagged several
  early-return failure paths as "text-only" without the automaton comparison having actually run,
  undermining the very invariant that gate exists to enforce.
- **Round 3** closed that gate-laundering gap (mutation-verified in both directions), corrected a
  parity nuance in WB-039's multiplication trigger (one leaked index vs two, depending on `n`'s
  parity), and hardened `PredicateEnv`'s trait shape so a future implementor can't silently stop
  counting load-time determinizations.

Verified against the real jar throughout (WB-039/WB-040 both confirmed live; fixture 637's full
Brzozowski-path state-count trace matches Java's bit-for-bit, completing in well under a second
where plain `SC` doesn't finish in 60s). **Golden corpus: 577/586 pass** (9 known divergences, all
independently re-confirmed genuinely text-only — 7 are the pre-existing `Logging`-threading gap,
637/660 join that same class), up from 576/583 (the `MetacommandNotWired` exclusion category is
gone). No net-new `WB-` entries beyond WB-039/WB-040 (both logged during this unit, both handled
per CLAUDE.md's rule — WB-039 with explicit user sign-off on a deliberate divergence, WB-040
confirmed unreachable in the port). `cargo test --workspace` green (1425 tests); `fmt`/`clippy`
clean.

**U32 complete — Phase 4, and DESIGN.md's original roadmap, are now fully executed.** New
`benches/` crate (a normal workspace member — unlike `fuzz/`, Criterion needs no separate
toolchain): `src/lib.rs` (workload table, session/prelude setup, the Rust engine, the JVM
client, the peak-state parser, the cross-engine answer check), `src/bin/compare.rs` (the
head-to-head), `benches/dispatch.rs` (Criterion, Rust side only), `java/BenchDriver.java` (a new
throwaway driver beside — never modifying — U29's `DiffGenDriver`), plus `README.md`
(methodology) and `STATUS.md` (the numbers). Workloads are 11 **real corpus fixtures** loaded
through the same Phase-0 manifests Tier 1 uses (`tests/golden`'s loader is `#[path]`-included,
not copied), spanning 0.3 ms to 2.2 s, plus an opt-in non-fixture row. Both engines are warm,
timed on their own side of the pipe, run over their own copy of the corpus library trees with
Walnut's own 19-command prelude, and every workload's answer is checked to agree by
`wr_core::equiv` **before** its timing is believed (and, for the nine automaton-valued ones,
against the automaton `walnut-java` itself recorded — all nine match).

**The result is mixed, and DESIGN.md §8's "faster than Walnut on the research workloads" clause
is NOT met.** The port is 1.35-1.73× **faster** on the two sub-millisecond workloads
(per-command overhead: parse/dispatch/small-automaton construction), and **1.28-1.65× slower on
all nine workloads where the decision procedure dominates** — a strikingly flat factor across
seven very different queries, which points at a systematic per-operation cost, not one bad
algorithm. A `sample(1)` profile says what it is: **51.5% of the port's CPU time on fixture 286
is the system allocator** (`tiny_malloc*`/`tiny_free*`/`madvise`/memmove) and another 12.2% is
`BTreeMap` node navigation, against 36.3% in actual engine code. That is the mechanical-port
rule showing its price — `Fa`'s `Vec<BTreeMap<i32, Vec<usize>>>` is a faithful transliteration of
Java's `List<Int2ObjectRBTreeMap<IntList>>`, and the JVM's bump-allocator nursery services that
allocate-many-short-lived-objects pattern far better than a general-purpose `malloc`.
`benches/STATUS.md` records the full numbers, the profile, four ranked candidate fixes (global
allocator swap first — cheapest, and it would *test* the diagnosis), and the threats to validity.
**This is reported, not buried: it is a real finding about the port, and the first concrete
argument for scheduling some of CLAUDE.md's "idiomatic Rust later, in separate commits".**

Two side results worth keeping: (1) the `[strategy 6 BRZ]` metacommand this unit's prerequisite
wired up is worth **510× on the port and 387× on Java** (91.7 ms vs 46.8 s; 4,965 vs 155,153
peak states) on fixture 637's query — the opt-in `sc637` row; (2) `tests/golden`'s per-fixture
times are NOT dispatch times (they include the Tier-1 `equiv` comparison, which dominates on a
large result: fixture 261 is ~0.31 s of dispatch inside ~5.2 s of golden wall clock), a
distinction that misled this unit's first workload sizing and is now documented in both files.

Two harness bugs found and fixed during bring-up, both of which would have produced meaningless
numbers: the JVM driver must assign the **static** `Prover.mainProver` (which
`DeterminizationStrategies.determinize` reads for the current command's metacommands) or
`[strategy 6 BRZ]` is silently ignored; and the Rust column must use **one session-lifetime
`Prover`**, because Java caches number systems in a JVM-global static while the port caches them
on the `Session` — measured at 0.31 ms vs 119.5 ms on fixture 207, a 390× artifact pointing the
wrong way (`WR_BENCH_COLD=1` reproduces it). `cargo test --workspace` green (1452 tests);
`fmt`/`clippy` clean; `cargo bench` and the head-to-head are separate invocations that never run
in the fast tier.

**U33 (unplanned, user-requested follow-up to U32's negative finding) — the two cheapest ranked
fixes from `benches/STATUS.md`'s "what would close the gap" list, both zero algorithmic risk:
swap the global allocator to `mimalloc`, and enable `lto = "fat"` / `codegen-units = 1` in
`[profile.release]`.** Neither touches `wr-core`/`wr-logic`/decision-procedure code — a
`#[global_allocator]` static registered in `wr-cli`'s and `wr-bench`'s binaries only (not the
library, so an embedder like `ct-research` isn't forced onto it), plus a Cargo profile change.
Commit `09020db`. **Result: 9-of-11-slower became 10-of-11-faster.** The allocator swap alone
accounts for essentially the whole effect (1.22×–3.62× per fixture); LTO/codegen-units add a
uniform but small 1.08×–1.16× on top. A repeat of U32's own CPU profile on the same fixture
(286) confirms the diagnosis directly: the system-allocator share of CPU time dropped from
**51.5% to 13.4%** (~248ms→~38ms per iteration); `BTreeSet::insert` inside `subset_construction`
is now the single largest frame at 24% — i.e. exactly `benches/STATUS.md`'s candidate #2
(flattening the transition representation), untouched here and correctly out of scope for this
pass (real correctness risk — iteration order is load-bearing in several ported algorithms).

**DESIGN.md §8's exit criterion is now met on most, not all, workloads — reported without
softening, same as U32's negative finding was.** Five fixtures (521/295/261/286/293) are genuine
wins, 1.20×–2.47× faster than Java. Two (179/230) are ties within the harness's own measurement
spread (~1.04×), not real wins either direction. **Fixture 637 — the most allocation-intensive
one, the Brzozowski/`thm5`-class query `[strategy 6 BRZ]` was wired up to unlock — is still
1.16× slower than Java.** The remaining gap sits exactly where the profile predicts. Golden
corpus unchanged (577/586, 0 regression); a 5,000-query Tier-3 spot check (fresh seed) also
clean. `benches/STATUS.md` keeps U32's original baseline verbatim alongside this unit's numbers
for comparison, not overwritten.

**U34 — `subset_construction`'s hot loop flattened; fixture 637 closed, and the whole 11-of-11
target now met.** Investigation of "flatten `Fa.d`'s `Vec<BTreeMap<i32, Vec<usize>>>`
representation" (U33's candidate #2) found the attribution was more specific than the original
framing implied: ~87% of U33's whole "B-tree navigation" bucket (27.6% of real work) was one
local `BTreeSet<usize>` inside `subset_construction`'s per-`(metastate, symbol)` union
(`determinize.rs`), not `Fa.d`'s own storage. Plan at
`~/.claude/plans/glossy-compacting-lantern.md` (outside this repo), adversarially reviewed
**three rounds** before any code landed — round 2 found a genuine blocking defect in round 1's
own proposed fix (a `TransitionRowBuilder` design that would have silently dropped destinations
from every nondeterministic cross-product), an unusually deep review chain reflecting the size of
the deferred `Fa.d` migration it was designing. The plan split into two phases with an explicit,
pre-registered go/no-go checkpoint: **Phase 1** — a small, one-function fix to
`subset_construction`'s own hot loop (a reusable scratch `Vec<usize>` + a borrowed `HashMap`
lookup replacing the per-iteration `BTreeSet` allocation-and-clone), independently proven a pure
representation swap by both code reviewers (Opus, Fable — different from the authoring model),
each running their own large-scale structural-equivalence probe (404,000 and 20,000 cases,
respectively, using the removed pre-change code as an oracle) rather than just reading the diff.
**Phase 1 alone closed the whole gap**: fixture 637 went from 1.16× slower than Java to **2.65×
faster**, and all 11 benchmark workloads are now faster in Rust than in Java (up from U33's
10-of-11) — `DESIGN.md` §8's Phase-4 exit clause is now met across the board. The pre-registered
checkpoint rule (proceed to the larger `Fa.d` migration only if 637 is still >5% slower than
Java, AND a fresh profile still attributes ≥8% of real work to `Fa.d`-rooted `BTreeMap`/allocator
frames outside `subset_construction`) evaluated to **stop** on both counts (637 is now faster,
not slower; residual `Fa.d`-attributable share measured at 4.7%) — so **Phase 2, the larger
`Fa.d`/`TransitionRow` representation change, was not implemented**, per the plan's own
pre-committed decision rule rather than a post-hoc call. The plan's fully-designed §2 (the
`TransitionRow`/`SmallVec`/`TransitionRowBuilder` design, its order-sensitivity audit across
~15 files, and the `DuplicatePolicy` semantics) remains available to resume from if a future
workload's profile looks different — not thrown away, just not currently justified. Golden corpus
unchanged (577/586, 0 regression); differential-gen 22,000 queries (0 divergences) across the
checkpoint spot-check and a follow-up run; `cargo test --workspace` green (1453 tests, +1 —
a regression test pinning `subset_construction`'s exact output structure on an
unsorted/duplicated destination list, added after adversarial review found the sort+dedup
invariant had no clean-failure tripwire, only a hang). Full numbers, the checkpoint's profile
breakdown, and the reproduction commands are in `benches/STATUS.md`'s new §U34.

**U28 (retroactive numbering — the Phase 3b `per-act()` `Logging` gap `wr_logic::eval`'s own
docs flagged since U27, but which never actually landed "before Phase 3b's U27" as that note
claimed it must — closed now, 2026-08-17, after U29-U34) complete.** Plan at
`~/.claude/plans/frosty-tumbling-nectarine.md` (outside this repo), adversarially reviewed
before execution. Threaded `&mut Logging` into every `Token::act`/`Operator::act`/`Word::act`/
`Function::act` body (`wr-logic`) and into every `wr-core`-level construction primitive they
call (`product`/`determinize`/`minimize`/`quantify`/`logicalops`/`numsys`/`word_automaton`) —
the actual per-`act()` COMPUTING/COMPUTED/quantifying/Minimizing/Determinizing detail Java logs
that this port had only ever logged the top-level per-operator state-count summary for. Went
through the full implementer → two-independent-adversarial-reviewer → fixer loop, several
rounds, plus extensive live-jar verification (a throwaway `CaptureLog.java` driver, this
project's `phase0-artifacts/CAPTURE.md` convention) rather than hand-deriving expected text.

Two real findings along the way, both logged:
- **WB-041** (new): `RelationalOperator.act`, `LogicalOperator.actQuantifier`, and
  `Operator.andThenQuantifyIfArithmetic` all call `Logging.indent()` unconditionally but
  `dedent()` only on success, leaking `+1` indent into whatever logs next after a failing
  operation — a genuine Walnut (Java) log-text-only quirk (the decision procedure itself is
  unaffected), invisible to any fixture since both engines' integration-test harnesses reset
  indent per fixture. Ported verbatim (the same `?`-skips-`dedent()` shape at `wr-logic`'s
  three matching call sites), not fixed.
- **WB-039 widened**: the same `Logging.disablePrint()`/`enablePrint()` non-save/restore bug
  U32 had already logged for its `[strategy …]`/`[export …]` index-instability consequence
  turned out to have a second, independent consequence for LOG TEXT — `disablePrint`/
  `enablePrint` gate whether `NumberSystem`-internal `and`/`quantify` construction logging is
  visible at all (`Logging.logMessage`'s full `printEnabled && printDetails` gate, not just
  console output — an earlier draft of this unit's own docs had this backwards and was
  corrected after a live-jar capture proved it). `wr_core::numsys`'s query-time construction
  methods (`comparison_const_b`, `arithmetic_const_a`/`b`/`c`, `constant`, `multiplication`,
  `division`) now call `Logging::disable_print`/`enable_print` at Java's exact bracket
  placements, which — because both are plain non-nesting field writes, matching Java's own
  buggy `static boolean` — naturally reproduces the leak rather than requiring it to be
  specially emulated.

**Round 1's "6 of 9 closed, 3 unfixable" conclusion was itself wrong, and a second
adversarial-review round (both live-jar-verified) is why it didn't ship that way.** After
round 1 (375, 376, 377, 378, 628, 637 closed; 379/383/660 written off as one shared
"warm-vs-cold `NumberSystem` cache" harness limitation), two fresh reviewers — different
models from the author and from each other, given only the diff — independently found the
same set of real, live-reproduced gaps this port's own "CLOSED" claim had missed:
`wr_core::logicalops::reverse` and `remove_leading_zeros` were missing their own Java logging
pairs and indent brackets entirely; the word⊗word arithmetic arm was missing an
`indent`/`dedent` bracket; `quantify_helper`'s new `Trimmed to:` line could fire where Java's
never would; and — the big one — `wr_core::product::cross_product` was simply missing Java's
own `computing cross product:` line (a separate call site from `crossProductAndMinimize`'s
own, which this port already had). One reviewer also caught that the "379 needs a warm cache"
claim rested on a misread diff-context window, not the fixture's actual first line.

Fixing the `cross_product` gap closed **660** outright — it was never a cache issue. Chasing
the corrected 379 evidence found a real distinction: `PredicateEnv` has two number-system
lookups with different caching contracts (`number_system`, memoized; `fresh_number_system`,
Java's `Function` constructor calling `new NumberSystem(name)` directly, deliberately
unmemoized). The FIRST fix built from that distinction — a throwaway `Logging` at
`number_system`'s three call sites, keeping `fresh_number_system` real — closed 379 (and kept
375-378 closed) by construction-time coincidence. Also added, from the same review round: a
genuinely missing `Logging.disablePrint()`/`enablePrint()` bracket in `NumberSystem`'s own
constructor; `right_quotient`/`left_quotient`'s own missing logging (currently latent —
`wr-cli` still hasn't wired real `Logging` into non-`eval`/`def` commands, a separately-scoped
follow-up); and a corrected `totalize` doc comment that had falsely claimed `reverse` was one
of its callers.

**That throwaway fix was then reverted, after a THIRD adversarial-review pass — verifying
round 2's own fixes — found it was the wrong call, not a shortcut worth keeping.** The
reviewers pointed out the throwaway made a genuinely fresh, single-query Walnut session (the
normal case for a real user's very first `eval` in a new session, real or ported — equally
cold either way) log LESS than real Walnut actually would, purely to make this harness's
specific cold-start artifact match fixtures captured deep inside Java's own already-warm
fixture-generation session. That is backwards from `CLAUDE.md`'s own Prime Directive —
fidelity to real Walnut, not to one test harness's quirk. `number_system` now threads the
caller's real `Logging` again (unconditionally, same as `fresh_number_system`), and **375-379
join 383 as one honestly-documented, understood harness limitation** — a genuine
warm-vs-cold `NumberSystem` session-cache mismatch between this harness (fresh `Prover` per
fixture, always cold) and Java's own continuous, long-running fixture-generation session
(warm by the time these particular fixtures ran). `wr_logic::predicate`'s
`Predicate::tokenize_and_compute_post_order` docs, `wr_logic::eval`'s module docs, and
`tests/golden/tests/golden_corpus.rs`'s `KNOWN_DIVERGENCES` entries for 375-379/383 have the
full three-round account; `tests/golden/STATUS.md` §1b is the narrative version.

**Golden corpus: 580/586 pass (99.0%)**, up from 577/586 — 0 timeouts, 0 not-run, both
automaton AND text compared for all six remaining divergences (genuinely text-only, not a
hidden automaton regression). **This is a smaller number than the 585/586 an intermediate
round reported, and that is the correct, honest result, not a regression to explain away** —
the extra five "passes" were bought by making production code quieter than real Walnut on a
path real users can actually hit. A 5,000-query differential-gen spot check (fresh seed)
against the real jar: 0 divergences. All three fuzz targets re-smoke-tested after every
round's fixes (hundreds of thousands of executions, 0 crashes). `cargo test --workspace`
green throughout; `fmt`/`clippy` clean (workspace and `fuzz/`'s own toolchain, both of which
needed the same signature threading this unit did everywhere else it touched a
determinization/logging call site).

**U28's code sat uncommitted after being written up above — committed and pushed 2026-08-19**,
in the same session that landed the fix below. (No commit hash was ever recorded for U28
itself in this log, unlike every other unit; that gap is now closed.)

**Golden corpus, closing 375-379 for real (2026-08-19): 585/586 pass (99.8%), via a
harness-only mechanism, not a change to what gets logged.** Round 2's reverted throwaway-
`Logging` fix (above) tried to make production code quieter — the wrong lever, per Prime
Directive. This instead teaches the golden-corpus *comparator* to identify, with certainty,
exactly which `details` text one specific call produced, and exclude only that. `wr_core::
logging::Logging` gained a side-channel recorder (`begin`/`end`/`discard_construction_
recording`, `construction_recordings` — no Java analogue) bracketing only `PredicateEnv::
number_system`'s memoized-lookup call site (`crates/wr-cli/src/session.rs`) — deliberately
NOT the shared `load_number_system` helper `fresh_number_system` also calls, since that
construction is genuinely reproducible in real Java too, not session-warmth noise (bracketing
the shared helper was tried first and broke fixture 379, whose `$fibmr(…)` calls go through
`fresh_number_system`). `tests/golden`'s comparator (`support::strip_construction_recordings`)
removes exactly that verbatim, line-anchored text before diffing — the same shape of fix as
`PathRewrite`.

Two independent adversarial reviewers (Opus, Fable), given only the diff, both found real
correctness-risk gaps — most seriously that the mechanism's "cannot mask a construction-time
regression" claim was **false, and live-verified as false**: injecting a bogus extra line into
`NumberSystem::with_custom_base_files` and re-running the corpus showed 375-378 still passing,
because a recorded span is copied from the same call that wrote it and so always matches
itself. Fixed by adding the positive coverage that was missing — a new pinned-line-sequence
regression test in `numsys.rs` (`a_cold_msd_fib_construction_logs_exactly_these_seven_lines`)
— and by correcting the doc claims across `logging.rs`/`golden_corpus.rs`/`support/mod.rs` to
state the real property: this cannot mask a bug in QUERY-COMPUTATION text (never inside a
recorded span), but is NOT independent verification of construction's OWN text — that
responsibility stays in `wr-core`'s own tests. Also fixed: the burst match is now anchored to
line starts (a plain substring search could match mid-line against a differently-indented,
unrelated line); a construction that logs some lines and then fails is discarded rather than
filed (not memoized, so it would re-log real signal on every later retry, exactly like
`fresh_number_system`'s always-genuine case). Every fix mutation-verified (reverted, confirmed
the corresponding new test fails, reapplied).

**383 stays open, deliberately.** It is a different root cause from 375-379 — `NumberSystem`'s
own recursive constant-building cache (`constants_dynamic_table`, exercised via `get_constant`)
leaking WB-039's non-nesting `disablePrint`/`enablePrint` bug, not `PredicateEnv::
number_system`'s cache. A similar recorder could technically be built for it, but was
deliberately not: `get_constant` is exercised across a large fraction of the corpus (any query
comparing against a constant ≥2), unlike `NumberSystem` construction (rare, at most once per
custom base per session) — a general recorder there risks silently stripping real, currently-
verified construction-detail text from many OTHER fixtures where it's also Java's own first
time building that constant, converting genuine matches into unverified no-ops corpus-wide.
Scoping it narrowly enough to be safe (in effect, keyed to fixture 383 specifically) would
buy only one fixture's worth of log-text verification for a real generalization risk — a
worse trade than 375-379's fix, where the affected text is provably rare and narrow. Left as
the documented `KNOWN_DIVERGENCES` exception it already was; automaton comparison still fully
enforced.

`cargo test --workspace`, `fmt`, and `clippy` all clean throughout. Both changes landed in one
commit on `master` (`2563ec3`, on top of `7660a35`) and pushed.

**Item 1 of `docs/BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md` (`I`-over-`lsd`) resolved, as a
negative-hypothesis finding, not a bug fix (2026-08-19).** The backlog note hypothesized
`act_quantifier`'s `I` (infinitely-often) arm might apply `remove_leading_zeros`'s msd fixup
unconditionally regardless of msd/lsd, mirroring a bug Phase 3b's L1 fixed in the sibling
`wr_core::quantify` path (used by `E`/`A`, not `I`). A dedicated Opus investigation subagent —
per this backlog's own model-tiering instruction — read the real Java source
(`LogicalOperator.actQuantifier`'s `I` branch, `AutomatonLogicalOps.removeLeadingZeros`/
`removeLeadingZerosHelper`, `Infinite.infinite`) and ran 47 live queries against a real,
freshly-verified-current `walnut-java` jar. **The hypothesis was refuted: 0 divergences.**
`remove_leading_zeros` was already msd/lsd-aware on both sides (it reverses its per-track
helper automaton exactly when the track's own `!msd`, matching Java's
`removeLeadingZerosHelper` line-for-line), and `act_quantifier`'s `I` arm already called it
unconditionally on the identifier list — i.e. the fixup itself, not the dispatch, carries the
msd/lsd awareness, and it already did. `Infinite.infinite`/`wr_core::infinite::infinite` is
confirmed msd/lsd-agnostic by construction (a pure `prefix·cycle*·suffix` graph search with no
`NumberSystem` input at all) — directionality is handled entirely upstream. No `WB-` entry
warranted.

The real gap the hypothesis pointed at was **missing test coverage**, not a bug: no unit test,
differential case, or Tier-3 generator coverage (U29's generator explicitly never emits `I`
queries) existed for `I` over `lsd` anywhere in the repo, and the one piece of coverage that did
exist by coincidence — Tier-1 golden fixture 520 (`?lsd_10 Ix x > 0`) — turns out to be
direction-INsensitive (cofinite either way) and could not have caught the hypothesized bug.
Added: three unit tests in `wr-logic/src/token.rs`
(`infinite_quantifier_over_lsd_matches_msd_shape`,
`infinite_quantifier_lsd_two_variables_or_fold`,
`infinite_quantifier_mixed_numeration_system_selects_correct_direction` — the last a hand-built
mixed-`lsd_2`/`msd_2` two-track automaton whose `Ix`/`Iy` verdicts must come out opposite,
the strongest pin), and a new differential file
(`tests/differential/tests/infinite_quantifier.rs`, 5 tests) including one case built through
the real `wr_cli::reg::reg` command dispatch rather than a hand-rolled `Fa` table. Every pinned
verdict was independently re-confirmed against a fresh `walnut-java` CLI run (not just trusted
from the investigation subagent's report).

This unit's own two-independent-adversarial-reviewer round (Opus, Fable — split context, diff
only) is itself worth recording: **both reviewers independently found, and mutation-proved,
that the first draft's two plain msd/lsd-pair unit tests were vacuous** — they built their
predicate operand via a helper (`predicate_operand`) that hardcodes the relational operator's
own number system to `msd_2` regardless of what base the caller's `NumberLiteralExpression`
operand carries (`RelationalOperator::act`'s dispatch reads the operator's own `ns` field, only
the literal's `BigInt` value), so both "lsd" tests silently re-ran the same `msd_2` computation
twice. Confirmed by mutating away `remove_leading_zeros_helper`'s `if !msd { reverse_with_ctx
(...) }` branch: both tests stayed green. Fixed with a new NS-parameterized
`predicate_operand_ns` helper; re-mutation-tested, now correctly fails. Opus additionally found
the differential file's original doc claim of "zero prior coverage" was itself inaccurate
(fixture 520 exists, just isn't discriminating) and that the file's verdict polarity was
lopsided (6 FALSE / 2 direction-insensitive TRUE) — both fixed, the latter by adding the
`reg`-based mixed-NS differential test (independently re-verified live: `Ix $mixr(x,y)` TRUE,
`Iy $mixr(x,y)` FALSE). Several doc-accuracy nits also fixed (a self-contradicting mid-sentence
claim in the mixed-NS unit test's doc, a stale function-name reference, an internally
inconsistent capture-provenance numbering). `cargo test --workspace`, `fmt`, and `clippy` all
clean throughout. Uncommitted as of this write-up — commit is on the user's explicit request per
this project's git-hygiene rule, not implied by "read and follow" the backlog.

**Item 2 of the same backlog (custom-base `lsd` verification) resolved — also a coverage
addition, not a bug fix (2026-08-19).** `walnut-java`'s `Custom Bases/` ships no `lsd_fib*`
files at all (only `msd_fib`/other `msd_*` bases) — `?lsd_fib` resolves on both engines solely
through `NumberSystem`'s opposite-direction-complement fallback (`msd_fib_addition.txt`,
language-reversed). An Opus investigation subagent captured eight queries over `lsd_fib`
(comparison, `∃`, three-track addition, open and closed `∀`, both `I` polarities, `def`-then-
`$token` reuse) through `wr-cli`'s real `eval`/`def`, all matching real `walnut-java` on the
first run with zero production changes. Added `tests/differential/tests/lsd_custom_base.rs`
(7 tests) plus 5 captured fixtures; every fixture and verdict independently re-verified against
a fresh `walnut-java` jar run (byte-identical). Mutation matrix (3 targeted mutations, each
applied/confirmed/reverted): `numsys::CustomBaseCandidates::resolve`'s reversal, `quantify`'s
lsd arm, and `remove_leading_zeros_helper`'s `!msd` branch — each caught by the expected subset
of the 7 tests, none by the closed-formula-verdict test (documented as a known, deliberate
weak spot, not an oversight).

Two-independent-adversarial-reviewer round (Sonnet, Fable — different from the Opus author,
split context): **no correctness defect in the tests or mutation matrix** — both reviewers
independently re-ran all three mutations themselves rather than trusting the report, and both
matched the claimed matrix exactly. But Fable caught a real doc-accuracy overclaim: the
original draft's "never been exercised" framing was false — Java's own gated-slow Tier-1
golden corpus already exercises `∃` and open `∀` over `lsd_fib` extensively (44 fixtures
referencing `lsd_fib` in `phase0-artifacts/test-manifest.json`, including fixtures 65/110-115/
135, all subset-relevant and currently passing — independently confirmed by inspecting the
manifest directly, not just trusting the reviewer). What is genuinely new here: fast-tier
presence (the golden corpus is `#[ignore]`d), `I` over a custom base (zero prior fixtures), and
`def`-then-`$token` reuse over one (also zero prior fixtures). Fixed the overclaim across all
three places it appeared (`lsd_custom_base.rs`'s module docs, `eval.rs`'s addendum,
`CAPTURE.md`'s new entry) rather than just softening the headline — the M1 mutation-matrix
bullet had inherited the same overclaim ("nothing outside this file can see this" — the golden
corpus can, just not every commit).

**Process lesson, worth keeping**: Fable's review also caught a live fleet-hygiene near-miss —
running two adversarial reviewers concurrently in the SAME shared working tree, when at least
one (unprompted) chose to do its own live mutation-verification (temporarily editing production
files, testing, reverting) rather than only reading, can transiently poison a concurrent
reviewer's test runs (`CLAUDE.md`'s existing fleet-hygiene section already warns about this for
concurrent implementers; it applies equally to concurrent reviewers that self-elect to mutate).
No harm resulted here — both reviewers' mutations were reverted before conflicting, and both
converged on the identical matrix — but it was luck, not design. Future dispatches of >1
concurrent reviewer should either instruct read-only review (no live mutation) or give each an
isolated worktree if mutation-verification is wanted from more than one at once.

`cargo test --workspace`, `fmt`, `clippy`, and `cargo doc` all clean throughout (no new doc
warnings). Uncommitted, same as item 1 — commit on explicit user request.

**Item 5 of the same backlog (the lone-`\r` line-splitting gap in `wr-io`) resolved — genuinely
easy, as the existing doc comment predicted (2026-08-19).** Only 3 call sites in
`crates/wr-io/src/reader.rs` used `str::lines()` (`\n`-only), and every line-number computation
in the file was the same one-line `i + 1`-off-`.enumerate()` pattern — no hidden fan-out. Added
`split_lines_java`, a hand-rolled, byte-level `BufferedReader.readLine()`-equivalent splitter
(`\n`/`\r`/`\r\n` each a terminator, `\r\n` counts once, unterminated final line kept, no
trailing empty line after a terminator), and swapped all three call sites onto it. 9 new tests
(3 on the splitter directly, covering every terminator kind and edge case; 3 end-to-end per call
site on lone-`\r` content; 1 confirming line-number reporting stays correct). Mutation-verified
(reverted to `str::lines()`, confirmed all 5 gap-pinning tests fail, restored). Checked the fuzz
corpus for `\r`-only seeds — only one seed contains `\r` at all and it's pure CRLF, so no
existing seed's behavior changes; re-ran the `wr_io_reader` fuzz target live (~1.5M executions,
0 crashes). No genuine Walnut (Java) bug involved — pure port-side gap, no `WB-` entry needed.
Reviewed directly by the coordinator (not the full two-adversarial-reviewer loop — `wr-io` isn't
a trust-critical crate per this file's own merge-gate scope, and the change is pure string-
splitting logic, not decision-procedure math); independently traced the trickiest edge cases
(a lone `\r` immediately followed by an unrelated `\r\n`) by hand before merging.
`cargo test --workspace`, `fmt`, `clippy` all clean.

**Item 3 of the same backlog (wiring real `Logging` into non-`eval`/`def` commands) resolved
(2026-08-19) — and turned out substantially larger than its own enumeration once two rounds of
adversarial review ran.** The starting premise: `Prover` already has a real `logging: Logging`
field, correctly `configure_for_command`-d on EVERY dispatch (not just `eval`/`def` — this
infrastructure already existed), but a number of command-handler functions constructed their own
throwaway `Logging::new()` instead of receiving it, so `<command>;::` silently printed nothing
for those commands even though the `wr-core` primitives they call have logged since U28. The
backlog's own enumeration named 8 call sites; investigation found 2 more in the same shape
(`fix_lead_zero_command`/`minimize_command`, both confirmed live to log in real Java) before any
review ran, landing an initial fix across quotient/convert/combine/union/intersect/alphabet/
fixleadzero/fixtrailzero/minimize/reg — 10 sites, plus a first differential test file
(`tests/differential/tests/cli_command_logging.rs`) checking real captured text per command
family, not just "the code compiles."

**Two-independent-adversarial-reviewer round (Opus, Fable — split context, diff only, instructed
read-only per this session's own fleet-hygiene lesson above) both found real, live-verified gaps
beyond that first pass** — the enumeration itself was still incomplete:
- **`reverse` was missed entirely** (Opus) — a whole command family, not in the original list,
  confirmed live to have gone from correctly logging nothing pre-fix... to still logging nothing
  post-fix, because nobody had touched it.
- **`concat`/`star` had throwaway `Logging::new()` INSIDE the very file this diff was already
  editing** (Opus) — `automaton_ops.rs`'s `concat_pair`/`star` already received the real
  `logging` as a parameter and used it on surrounding lines, but called the plain
  `determinize_and_minimize()` instead of the `_with_ctx` sibling one line away.
- **`inf`/`test` shared the same gap** via `remove_leading_zeros` (Opus), in `prover_helper.rs`/
  `test_command.rs`.
- **`union`/`intersect` are missing Java's `computed =>:Q states - Tms` line entirely**
  (BOTH reviewers, independently) — `Union.java:76` logs it once per fold iteration; the port
  never did. The new test's own transcript of real captured output silently omitted this exact
  line, so the test itself couldn't have caught it — reviewers caught the missing line in
  production code AND the doctored-looking transcript in the same pass.
- **The `reg` test's whole premise was factually wrong** (Opus) — it observed real Walnut's
  CONSOLE printing nothing for a cold custom-base `reg` and concluded "real Walnut logs nothing
  here," settling for a weak smoke test. The console and `detailedLog()` are different channels
  in Java (`NumberSystem`'s `disablePrint()` bracket, WB-039, suppresses the console via
  `printEnabled` but `Logging.logDetail` gates `detailedLog` on `printDetails` alone, and
  `Automaton.applyAllRepresentations`'s `"Applying valid representation #i"` is a `logAndPrint`
  that never re-checks `printEnabled`) — so `detailedLog()` DOES carry seven real lines here,
  which is exactly what this fix was supposed to prove reaches through, and the test had settled
  for asserting nothing about it.
- **The one genuine behavioral change this diff made** (a new `Logging.indent()`/`dedent()`
  bracket in `alphabet.rs`'s `set_alphabet`, verified against real Java's `Automaton.java:218-220`
  — accurate) **was exercised by no test at all**, because the existing test's operand (a literal
  `{0,1}` alphabet) has no `NumberSystem`, so the bracketed code path never runs and the bracket
  is a no-op either way.
- Both reviewers independently confirmed the two gaps the diff DID correctly leave out of scope
  (the `Determinizing […]` line's `ctx.is_some()` gating — genuinely needs a `DeterminizeContext`,
  U32-scoped; `convertNS`'s unported `CONVERTING`/`CONVERTED` announcement lines, pre-existing and
  already flagged in `logicalops.rs`'s own module docs) — so the reviewers weren't just finding
  more work, they were also confirming the deliberate boundary was drawn in the right place.

**Fixer round closed every finding, live-verified against the real jar, and — while it was in
there — found the SAME missed-announcement-line pattern in `Concat`/`Star` too** (their own
`concat:`/`concat complete:`/`concatenated =>:`/`star:`/`star complete:` lines,
`Concat.java:54/61/80`, `Star.java:23/33`, never ported at all, same class as the `union`/
`intersect` gap above but not explicitly named in either review). Final tally: **15 call sites**
across 7 `wr-cli` files threaded with the real `Logging`, 5 previously-unported announcement-line
classes added (`union`/`intersect`'s `computed =>:`, `concat`'s three lines, `star`'s two),
`tests/differential/tests/cli_command_logging.rs` grown from 9 to 16 tests (new: `reverse`,
`concat`, `star`, `intersect`, `inf`, `test`, the alphabet-indent case; the `reg` test rewritten
from a weak smoke test to an exact 7-line `assert_eq!` on `detailedLog()`, matching
`crates/wr-core/src/numsys.rs`'s existing `a_cold_msd_fib_construction_logs_exactly_these_seven_lines`
pin for the direct-construction path). `fresh_prover` fixed to sink its console output like every
other test file, instead of polluting real `cargo test` stdout. Six mutations applied/confirmed-
failing/restored (one per fixed finding). Every fix independently re-verified by the coordinator
against the real Java source directly (not just the review/fixer text) for at least the highest-
risk claims: `Concat.java`/`Star.java`/`Union.java`'s exact log lines, `Automaton.java:218-220`'s
indent/dedent bracket. `git diff --stat -- crates/wr-core/` confirmed empty throughout every
round — this entire unit is `wr-cli`-only parameter threading and new test coverage, no
`wr-core`/`wr-logic` change at any point, though the coordinator still ran it through the full
two-reviewer loop given the size and real-behavior-change nature of the diff.

**Golden corpus unchanged (585/586, the same single known-open fixture 383), 0 regression** — the
only `::`-suffixed fixture on any affected command in the whole 675-fixture manifest
(`alphabet test617 msd_4 T::`) compares an automaton, not text, so was never at risk.
`cargo test --workspace` green (1532+ tests), `fmt`/`clippy` clean throughout. Uncommitted, same
as items 1/2/5 above — commit on explicit user request.

**This closes `docs/BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md` in full — all four items (1, 2, 3,
5) are resolved.**

**CAS matrix export ported (`docs/CAS-EXPORT-DISPATCH.md`, 2026-08-19) — CAS export is no longer
DROP scope.** Investigation first: Phase-0 coverage on the six Java files
(`AutomatonMatrixWriter`/`MatrixEmitter`/the four emitters) turned out already high (93.5-100%
line, freshly measured via an unrestricted JaCoCo run — the files are excluded from the tracked
report), so no separate coverage-driving pass was needed; and the dispatch doc's one open
architectural question (headless vs. interactive `EvalDef` dispatch) resolved to "no gap" —
both `wr-cli`'s real dispatch and `tests/golden`'s harness already sit on Java's interactive
`compute()` path, not `computeHeadless()`. Plan at `.claude/plans/amber-transcribing-ledger.md`,
adversarially reviewed before execution (fable — caught a mischaracterized test-coverage claim,
a golden-corpus known-divergence laundering hole, a missing mutation-verification step, an
unflagged test needing a flip, an under-scoped review tier, and a stale WB-007 status line; all
fixed in the plan before any code landed). Implemented: a new `wr-io` module
(`crates/wr-io/src/matrix_writer.rs`) mechanically porting `AutomatonMatrixWriter` + the Maple/
MATLAB/Mathematica/Sage emitters; the `wr-cli` wiring (`eval_def.rs`) that was previously a
`Vec::new()` stub; `tests/golden`'s harness comparison (`Expected::has_cas_matrices`/the
`cas-matrix-skipped` branch deleted — all 586 fixtures now compared symmetrically). **WB-042**
logged (`MathematicaEmitter` uses `#`, invalid Wolfram Language syntax, as its comment prefix —
ported verbatim, not fixed).

Two independent adversarial reviewers (opus, fable — split context, diff only) both converged on
the same real correctness-risk finding neither the implementer's own first-pass verification nor
the plan's pre-registered check had actually closed: fixture 383 is both a matrix fixture and the
sole surviving `KNOWN_DIVERGENCES` entry (a `details`-text-only WB-039 gap), and the gate's
"every failure reason is text" check could not tell a documented `details` divergence apart from
an undocumented `matrix` one — so a real matrix regression on 383 specifically would have been
silently excused. The plan's own mutation check (corrupting `MapleEmitter::begin`) hadn't
actually exercised this, since it only proved the OTHER six fixtures fail loudly, which they
always would have. Fixed by giving `FailedHalf::Text` a `TextField` discriminant
(`Details`/`Matrix`/`Graphviz`/`Error`) and scoping each `KNOWN_DIVERGENCES` entry to the
field(s) it actually declares (`Verdict::is_excused_by`, `tests/golden/tests/golden_corpus.rs`)
— live mutation-verified in the correct direction this time (corrupting `MapleEmitter::begin`
and checking the GATE's own verdict, not just the report text, now fails the run with an
`UNDECLARED TEXT FIELD` diagnosis on 383; reverting restores green). Also fixed from the same
review round: a weak fast-tier differential assertion (a substring check that would have
survived a wrong matrix order/fixup/separator in any of the four formats) replaced with a
byte-exact comparison against a fresh `walnut-java` capture (`tests/differential/fixtures/
cas_export/`, `tests/differential/CAPTURE.md`); the two `eval_def.rs` free-variable validation-
error tests extended to assert the same zero-byte-first-file-then-stop quirk `wr-io`'s own test
already pinned at the primitive level; a new test locking `wr_io::matrix_writer::EMITTERS`'
extension order to `tests/golden`'s `MATRIX_EXTENSIONS` (two independent literals in different
crates that must stay in sync, previously untested); and a silent `as i32` truncation on
`alphabet_size` replaced with the same panic-on-overflow idiom `Automaton::
determine_alphabet_size` already established for this exact conversion. Golden corpus:
585/586 unchanged (0 regression; 383 remains the one open, deliberately excused divergence,
correctly excused with the tightened gate), 28 new byte-for-byte matrix comparisons now
genuinely run instead of being skipped. `cargo test --workspace` green throughout, `fmt`/
`clippy` clean. Uncommitted — commit on explicit user request, per this project's standing
git-hygiene rule.

**Ostrowski numeration ported (`docs/OSTROWSKI-DISPATCH.md`, 2026-08-20) — `ost` is no
longer DROP scope, and `Automata/` now has no DROP files left at all.** Plan at
`~/.claude/plans/dusky-braiding-compass.md` (outside this repo; a v2, revised after a full
adversarial review of v1 that found four blocking defects — a `wr-core`→`wr-io` crate
dependency that would not have compiled, a missing `state_transitions` density fill that
would have produced malformed automata, a vacuous property test, and a factual mix-up
between two different `continue` statements — plus four more). Phase-0 coverage was driven
first: a fresh JaCoCo run measured `Ostrowski.java` at 96% line / 87% branch, and five added
`@Test` methods in `walnut-java`'s `OstrowskiTest.java` took it to 99.6%/91.7% (both
remaining misses are one bytecode-instrumentation artifact of how a specific `||` compiles
against a fixed table — not dead code).

Landed: **`crates/wr-core/src/ostrowski.rs`** (`NodeState`, the fixed 7-state 4-input adder
table, `Ostrowski` + its constructor, both BFS builders and every shared helper,
`populate_automaton`/`init_automaton`/`handle_zero_state`) and **`crates/wr-cli/src/ost.rs`**
(`Ost.ostCommand`: parse → construct → write repr → write adder, each write with its own
already-exists guard, returning Java's two-`AutomatonFilenamePair` `TestCase`), plus the real
`OST` dispatch arm in `prover.rs` (the last `ProverError::UnsupportedCommand` producer is
gone) and `walnut_exception::number_system_already_exists`. Six deliberate divergences from
the Java source, each stated in the module docs rather than left implicit: `Ostrowski::new`
takes already-parsed `&[i32]` (`wr-core` cannot depend on `wr-io`'s `ParseMethods`, the same
split `crate::morphism` already uses); `Option<(i32, i32)>` instead of the `99`/`NONE`
sentinel; an unordered `HashMap` for `nodeToIndex` (audited: neither map is ever iterated, so
`NodeState.compareTo` has no observable effect — `Ord` deliberately not implemented); a `Vec`
for `indexToNode` (the keys really are dense, so `isReprFinal`'s `node != null` guard is
provably dead); `state_transitions` kept dense **continuously** at every node allocation
rather than by `populateAutomaton`'s blanket `putIfAbsent` sweep (that sweep is load-bearing,
not filler — a node's row is created when it is pointed *to*, and `seenIndex == 1` nodes
early-`continue` before ever being dequeued); and an explicit `set_canonized(true)` after
`handle_zero_state`, which has no literal Java counterpart but *reproduces* one
(`FA.canonizeInternal` sets the flag, `handleZeroState` then mutates `nfaD`/`O`/`Q` without
resetting it, so `AutomatonWriter`'s own `canonize()` is a no-op — measured honestly: on all
18 automata this port currently builds, re-canonicalizing would have been byte-identical
anyway, so the flag is fidelity insurance, not a currently-observable fix, and its test says
exactly that).

**One genuine port defect found and fixed (not a Walnut bug, so no `WB-` entry — highest
remains WB-042).** `d_max` is user-controlled through a `\d+`-only regex, and the adder's
alphabet is `(d_max+1)^3`, so `ost bigone [1291] [1];` (the smallest cube exceeding
`Integer.MAX_VALUE`) reaches the long-documented gap in `Automaton::determine_alphabet_size`
— Java's `Math.multiplyExact` checks at **`int`** width, this port's at `usize`. Verified
live against the real jar: Java writes `msd_bigone.txt`, then throws `ArithmeticException:
integer overflow` and returns to the prompt. The port silently WRAPPED the `i32`
transition-encode arithmetic in a **release** build (debug caught it only by accident, as
`attempt to add with overflow`) and spent ~14 s writing a bogus `msd_bigone_addition.txt`.
Fixed with an `int`-width check at exactly Java's call site
(`assert_alphabet_size_fits_in_an_int`), scoped to this module on purpose — widening
`determine_alphabet_size`'s own check is a separate cross-cutting decision. Mutation-verified
in release, where it actually matters.

**Testing, all four tiers.** *Tier 2*: every one of `OstrowskiTest.java`'s 15 methods
replicated — the eight `testAgainstFile` cases as **byte-for-byte** comparisons of
`wr_io::writer::write_txt` output against the very `.txt` fixtures Java's suite compares
against (copied into `crates/wr-cli/tests/fixtures/ostrowski/`), which passed on the first
run; the constructor/`NodeState`/throw cases in `wr-core` (`compareTo` deliberately not
ported — it is unreachable API, and the inequality it stood for is asserted directly).
*Tier 4*: a real, Walnut-independent oracle, replacing the plan's own vacuous first draft (a
track-swap test that would have passed with the transition table zeroed out, since
`addTransitions` is symmetric in `x`/`y` by construction) — the Ostrowski place values are
derived from the continued fraction itself (`q[i] = a_i·q[i-1] + q[i-2]`, anchored by
asserting they reproduce literal Fibonacci and Pell numbers), and four systems are swept
exhaustively: the representation automaton accepts **exactly** the valid representations, the
canonical words of each length enumerate **exactly** `0..q[len]`, and the adder accepts a
canonical triple **iff** the values add up. Two findings from building it, both recorded in
the test's own docs: the adder's language is deliberately WIDER than the sum relation (state
0's `(0,0)` self-loop accepts `(0, w, w)` for any `w`) because Walnut always intersects it
with the representation automaton via `all_reps` — so restricting the sweep to canonical
triples is the real property, not a weakening; and mutating one `adder_transitions` entry or
dropping the oracle's `b_i == a_i ⇒ b_{i-1} == 0` clause both fail the sweep loudly.
*Tier 3*: `tests/differential/tests/ostrowski.rs`, four cases driven through the real
`Prover::dispatch`, against a freshly captured `walnut-java` run (recipe in
`tests/differential/CAPTURE.md`; captured twice independently, all nine files byte-identical)
— covering both `preperiod[0] == 1` rotation branches, which no golden fixture reaches, and
follow-up `eval`s over the freshly created bases. *Tier 1*: **golden fixture 625 now compares,
and passes** — corpus moved from 586 compared / 585 pass to **587 compared / 586 pass
(99.8%)**, 88 skipped, 0 timeouts, with the single pre-existing 383 divergence unchanged.

That last one was two real work items, not a flag flip. **Harness side**: 625 is the corpus's
only fixture with TWO recorded automaton pairs (`IntegrationTest.loadTestCases`' "hack for
repr files"), so `support::Expected` gained an `automaton_repr_path` + `automaton_pair_count()`
and the comparator a genuine positional two-pair branch mirroring Java's own `runSpecificTest`
— including a per-pair LABEL check, since returning the right two automata under swapped
labels would otherwise pass, and the expected count comes from what the corpus recorded rather
than a hardcoded id. **Exclusion side (cross-repo)**: `walnut-java/phase0-artifacts/
subset-filter.json`'s row for 625 flipped to `subset_relevant`, along with the three aggregate
counts `support::load_fixtures` self-checks (`subset_relevant_count` 591→592,
`drop_relevant_count` 84→83, `drop_reason_counts.drop_command` 16→15) and its `schema_note`.

`cargo test --workspace` green (1580+ tests), `cargo fmt`/`clippy` clean, `cargo doc` clean of
new warnings. `docs/BOUNDARY-MAP.md`, `docs/DESIGN.md`, `docs/UNPORTED-SCOPE-SIZING.md`,
`docs/PHASE0-CONTINUATION-DISPATCH.md`, `docs/OSTROWSKI-DISPATCH.md` and
`tests/golden/STATUS.md` all updated to match. **This unit has NOT yet been through the
two-independent-adversarial-reviewer loop** (`wr-core` construction code — it is required
before merge). Uncommitted in both repos, per this project's standing git-hygiene rule.
