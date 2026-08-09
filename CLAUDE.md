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

Not yet started: Phase 2 (mechanical port of the rest of `wr-core`) — needs the user's explicit go-ahead
per `docs/ROADMAP-TO-AUTONOMY.md`'s phase-gating.
