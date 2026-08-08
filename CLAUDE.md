# walnut-rs — Agent Operating Guide

A Rust reimplementation of a **research-driven subset** of the [Walnut](https://walnut-theorem-prover.github.io/)
automatic-theorem-prover. This file tells Claude how to work here. **Read `docs/DESIGN.md` first** — it is the
full plan (scope, correctness ladder, roadmap, and the adversarial-review record). This is a **derivative work of
Walnut (GPLv3)**; keep it GPLv3 and preserve attribution (see `NOTICE`).

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
- **Model-tiering — cheap by default, escalate on evidence.** Cheap tier (Haiku) for mechanical transliteration,
  boilerplate test replication, compiler-error batches; mid (Sonnet) for most implementation/review; expensive (Opus)
  ONLY for the hard ~20%: the parser + `NumberSystem` edge cases, quantifier-elimination correctness, adversarial math
  review, diagnosing differential-test divergences, **and any architecture / crate-boundary / mechanical-vs-refactor
  judgment call** (e.g. resolving a coupling cycle — these need the *most* scrutiny, not the least).
- **Compiler as work queue** — fix errors crate-by-crate, batch similar errors. Mechanical commits before idiomatic ones.
- **THE MERGE GATE (hard rule):** never commit or merge with `cargo test --workspace` **red**, and never merge
  trust-critical-crate (`wr-core`/`wr-logic`) code without the two-reviewer loop having run. Zero tests deleted, ever
  (a failing ported test is a real signal — fix the port, don't delete the test).
- **Cost control** — a hard per-phase token/$ ceiling; the deterministic correctness pipeline (running suites, corpus
  diffing, fuzzing) costs zero model tokens — keep it there.

## Fleet git hygiene (mandatory when >1 agent runs concurrently)

At the target scale (dozens of parallel agents), git collisions corrupt work. Hard rules (learned the hard way in
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

Phase 0 (bootstrap): scaffold committed. **Next steps** (DESIGN.md §8 Phase 0): fork upstream Walnut → `walnut-java`;
stand up JaCoCo coverage on the subset; classify the TO-CLASSIFY commands; run the **OTF empirical check** (do the
real research queries need a non-`SC` `[strategy …]` to terminate? — DESIGN.md §9 F3); filter the golden corpus to the
subset. Then Phase 1 spike: base-k DFA + `minimize` + one quantified `eval`, plus the `wr-core` equivalence oracle,
differentially checked vs `walnut-java`.
