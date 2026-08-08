# Roadmap: from the current scaffold to (semi-)autonomous, token-efficient development

*Draft v1 — 2026-08-08. The gap between "the kit exists" and "an agent can develop here (semi-)autonomously
without wasting your subscription tokens." Read after `docs/DESIGN.md` and `CLAUDE.md`.*

---

## 0. What "autonomous" realistically means on a Claude subscription

You are on a **subscription**, not API pay-per-token. That changes the whole frame from the dollar-based cost
model in DESIGN §7 (which used the Bun rewrite's ~$165k API bill as a magnitude anchor):

- **The constraint is tokens against your session/usage limits, not a dollar bill.** The goal is to get the most
  *useful work per token*, and to **pause cleanly** when a limit is hit.
- **Session limits are PAUSES, not failures.** A limit with a reset time is a break to resume from, not the end
  of a run. So the setup's #1 job is **resumability** — a pause must lose zero progress.
- **The Bun 64-way parallel fleet economics do NOT transfer.** That massive parallelism was API-billed; on a
  subscription, dozens of concurrent agents just hit your rate/session limit almost immediately. Here, "autonomous"
  means a **modest, resumable, phase-gated loop that spends tokens efficiently** — not unattended mass parallelism.

**Target posture for this project:** *semi-autonomous, phase-gated, resumable.* An agent (or a small handful) works
a phase, checkpoints continuously, pauses cleanly at session limits, and stops at phase boundaries for you to review.
You remain the governor at phase edges; within a phase it runs on its own.

---

## 1. Token efficiency — the thing you actually asked about

**Three levers plus one hygiene item**, in rough order of impact. These are the difference between finishing a phase
in one session vs. several. *(These subscription-specific claims are reasoned, not measured for your exact tier —
treat them as tendencies, like DESIGN's own hedged estimates.)*

### 1a. Model tiering — real, but NOT free per unit (know the mechanism)
Use a cheaper model where you can — **but a single session runs on ONE model, and switching it mid-session
invalidates the prompt cache and re-sends the whole accumulated context uncached (expensive).** So tiering is not
"start every unit cheap, escalate per unit"; it is two deliberate moves:
- **Run the session on the tier that fits the bulk of the current phase.** A mechanical-port stretch → launch on
  **Haiku/Sonnet**; it stays there. Reserve an **Opus** session for a genuinely hard stretch (parser / `NumberSystem`
  edge cases / quantifier correctness / differential-divergence diagnosis / architecture calls).
- **Delegate a large ISOLABLE unit** (a whole file or test class — *not* a single method) to a fresh `Agent`
  subagent with a `model` override. This avoids the cache tax (no cache to miss) but pays a **cold-start** cost
  (re-establishing `PORTING.md` + the relevant context). It only pays off when the unit is big enough that the
  cheaper per-token rate outweighs that cold start — so **escalate/switch in batches, not per small unit.**
- Directionally, cheaper tiers stretch your session budget further and run faster; do not treat that as a fixed
  multiplier for your account.

### 1b. Context management (keep the coordinator thin) — the most reliable lever
The largest hidden cost is a bloated main context re-sent every turn. Doctrine (inherited from ct-research, now also
in `CLAUDE.md`):
- **Delegate heavy-in / small-out to fresh subagents** and keep only the conclusion. Reading a big Java file,
  running a coverage pass, sweeping the corpus → a subagent returns the verdict, not the file dump. *(This does not
  reduce total tokens — the subagent's tokens hit the same subscription — it keeps the **main** context small so it
  isn't re-sent every subsequent turn, which is the recurring cost.)*
- **Never read a large Java source file wholesale into the coordinator.** Use `Grep`/`Explore` to locate, read only
  the slice you port. (`NumberSystem.java` is 1,027 lines — reading it whole is a large avoidable cost.)
- **Route long output to files, not context** — test logs, coverage reports, diffs go to disk; grep them.
- **Trigger delegation on task SHAPE, not on "context feels large"** — by the time it feels large you've already
  paid and are re-paying every turn.

### 1b. Context management (keep the coordinator thin)
The largest hidden token cost is a bloated main context re-sent every turn. Doctrine (inherited from ct-research):
- **Delegate heavy-in / small-out to fresh subagents** and keep only the conclusion. Reading a big Java file,
  running a coverage pass, sweeping the corpus → a subagent returns the verdict, not the file dump.
- **Never read a large Java source file wholesale into the coordinator.** Use `Grep`/`Explore` to locate, read only
  the slice you port. (Walnut has 1000+-line files — reading `NumberSystem.java` whole is a large avoidable cost.)
- **Route long output to files, not context** — test logs, coverage reports, diffs go to disk; grep them.
- **Trigger delegation on task SHAPE, not on "context feels large"** — by the time it feels large you've already
  paid and are re-paying every turn.

### 1c. MCP / tooling surface (a minor hygiene item — do it, don't overrate it)
- Tool **schemas load on demand** (deferred) — so an unused connected server does **not** cost per-turn schema
  tokens (this session is a live example: the Lean tools appear only as deferred names). What an MCP server *does*
  cost is a **one-time per-session "instructions" block**. Small, and paid once — not the recurring per-turn cost
  that makes §1b big.
- So: **don't connect servers you won't use here** (walnut-rs is Rust — it does NOT need ct-research's Lean
  `lean-lsp` MCP), but treat this as hygiene, not a major lever. §1a and §1b dwarf it.
- If you add a Rust LSP/analyzer later, prefer plain `cargo`/CLI unless a heavy always-on MCP genuinely earns its
  one-time instructions cost.

### 1d. The deterministic pipeline is FREE
Running test suites, diffing the golden corpus, the differential harness, fuzzing, coverage measurement — all
**deterministic scripts with zero model tokens**. Keep correctness *execution* in scripts; spend model tokens only on
*authoring and reviewing*. Never drive a deterministic loop with a model.

---

## 2. Resumability across session-limit pauses (makes autonomy survivable)

A session limit must lose nothing. **Key reality: a usage limit can cut off mid-turn with no warning** — unlike a
context-window ceiling, you cannot reliably measure-and-pre-empt it. So an "orderly checkpoint-then-stop" is a
*best-effort fallback*, NOT the plan. The plan is that state is always already on disk:
- **PRIMARY: commit working progress frequently** (green, small commits — the merge gate enforces "never commit
  red"). This is what actually survives an abrupt cut-off; everything below is secondary.
- **A live `RESUME-HERE.md`** — a short doc updated every checkpoint: what's done, what's in flight, the exact next
  step. Written to disk continuously (not only at a graceful stop, which you may not get).
- **A per-phase append-only progress log** — a mid-phase pause is then a clean restart point.
- **No long-lived in-memory-only state** — anything needed after a pause lives on disk (manifest, boundary map, log),
  not just in the conversation.
- **Resume is NOT automatic** (see §5): after the reset, *something must start a new session and point it at
  `RESUME-HERE.md`* — a human, or an explicit scheduled-wake work item (W6b). Do NOT treat a limit as "done," but do
  not assume the run restarts itself either.

---

## 3. The work items (current scaffold → semi-autonomous), sequenced

Each item: **what**, **who** (you vs. agent), **why it's needed for autonomy**, **done-when**.

| # | Work item | Who | Why | Done-when |
|---|---|---|---|---|
| **W1** | **Fork upstream Walnut → `walnut-java`** (`gh repo fork Walnut-Theorem-Prover/Walnut`), clone as `~/dev/walnut-java` | **You** (outward-facing) | Phase 0's actual work (coverage/oracle) lives here; agent can't create your fork | Fork exists, cloned, builds (`bin/walnut`-style JDK17) |
| **W2** | **Confirm the local environment** — Rust 1.83 toolchain, `gh` auth, JDK 17 + JaCoCo for `walnut-java`, `cargo-fuzz` | You + agent | Agents can't proceed if the toolchain/CI can't run | `cargo test` green here; `mvn test` + JaCoCo run in `walnut-java` |
| **W3** | **`GETTING-STARTED.md` kickoff** — the exact Phase 0 task list, the cheap-model/phase-gate/resume posture, the fork step flagged as yours | Agent (I can draft) | So the first agent starts correctly instead of guessing | Doc committed; a fresh agent can follow it unaided |
| **W4** | **Port the enforcement hooks from ct-research** — commit gate on `wr-core`/`wr-logic` (reviewer loop ran), recursive-`rm`/backgrounded-job guard, pre-push `cargo test`; follow `AUTHORING.md` (self-test all branches) | Agent | Turns the convention-only merge gate + git hygiene into *mechanical* enforcement — required before trusting unattended stretches | Hooks self-tested + wired in `.claude/`; a red-suite commit is blocked |
| **W4b** | **Author the review-agent role(s)** the loop needs — today only `adversarial-reviewer.md` exists (DESIGN §7's `proof-referee`/`claim-auditor`/`red-team` are ct-research, NOT here). Decide which extra roles, if any, this port needs and author them | Agent | The loop can't dispatch agents that don't exist | Needed roles authored under `.claude/agents/` |
| **W5** | **Orchestration harness (only if in-session delegation proves insufficient — see note).** A *modest* launcher adapted from `claude-box-agent`: per-agent model-tier, the implementer→2-reviewers→fixer loop, diff-only dispatch, per-agent token caps, isolated `agent/<name>` clones, **429/rate-limit backoff + an empirically-tuned concurrency cap**, and **disk/`target/` pruning** across clones. Sized for a subscription (a few agents, not 64) | Agent (design) + You (approve) | Parallelism on a subscription does NOT save tokens (spend is additive; it only buys wall-clock and hits limits faster) — so build this **only if** the lighter in-session-subagent-delegation pattern proves insufficient | Justified vs. the delegation alternative; one implement→review→fix cycle runs end-to-end |
| **W6** | **Resumability scaffolding** — a `RESUME-HERE.md` template + per-phase append-only progress-log convention (§2) | Agent | A mid-turn session-limit cut-off must not lose work | Template committed; updated every checkpoint |
| **W6b** | *(optional, for true unattended resume)* **Scheduled re-wake** — a cron/launchd (or `CronCreate`) wake for `reset + ε` that starts a fresh session pointed at `RESUME-HERE.md`. Without it, resume is **manual** | Agent (wire) + You (decide) | `RESUME-HERE.md` is passive; something must re-invoke a session after the reset | A pause auto-resumes once, from disk state |
| **W7** | **Phase 0 deliverables** (DESIGN §8): JaCoCo coverage on the subset; full `Automata/` inventory → verified crate-boundary map; adversarial review of `PORTING.md`; the **OTF empirical check**; golden-corpus subset filter; test-manifest export | Agent (drive) + You (phase-gate review) | These settle the open scope questions and produce the spec the Rust port is built against | All six done + reviewed; Phase 1 gate clear |

**Merge-back authority (resolve before W5):** at L2, pre-authorize merge-back of **green, hook-passed** diffs that do
*not* touch `wr-core`/`wr-logic`; anything hook-failing or touching a trust-critical crate waits for your explicit
go-ahead (reconciles the fleet's "merge deliberately" with CLAUDE.md's "commit on the user's request").

**Suggested order (corrected):** **W1→W2→W3 → W7 starts immediately.** Phase 0 is Java-side (DESIGN §10: "can start
as-is") and touches **zero** Rust code — so it does **not** need W4/W4b/W5, which gate/parallelize Rust-port work on
crates that are still empty scaffolds. Land **W6** early (cheap, generically useful for any long run). Defer
**W4/W4b/W5/W6b to just before Phase 2**, when `wr-core` actually starts filling in and there's multi-unit work to
gate and (maybe) parallelize. Building the harness before Phase 0 is days of infrastructure for empty crates.

---

## 4. What stays manual (your call, by design)

These do not get automated away — they're either outward-facing or genuine judgment forks:
- **The `gh repo fork`** and any **push to a public remote** (outward-facing).
- **Phase-boundary go/no-go** — you review at each checkpoint; the phases are designed as stop points.
- **Model-tier ceiling for a session** — you pick the model you launch with; the agent tiers *down* from there via
  subagents but won't silently escalate the whole session to Opus.
- **The OTF go/no-go** (does deferral hold for your real queries — DESIGN §9 F3) — a scope decision.
- **Promoting a phase's conclusions** into DESIGN/known-results.

---

## 5. Autonomy readiness ladder

| Level | Description | Requires |
|---|---|---|
| **L0 — Interactive** (today) | You drive an agent turn-by-turn in the repo | Nothing more; kit is ready |
| **L1 — Interactive, phase-gated** | Agent runs a phase's steps; you review at checkpoints | W3 (kickoff doc) |
| **L2 — Semi-autonomous within a phase** | Agent runs the implement→review→fix loop unattended *inside* a phase, stops at the boundary | W4 (hooks) + W4b + (W5 only if delegation insufficient) |
| **L3 — Resumable across pauses** | A pause is survived: **by default a HUMAN restarts** and points the session at `RESUME-HERE.md`; auto-restart needs W6b | W6 (resume scaffolding) + W4; **W6b for auto-resume** |

**L2 and L3 are one milestone, not two.** Every phase's effort estimate (DESIGN §8) is 1–6 weeks, i.e. it *will* span
multiple session-limit windows — so L2 "unattended within a phase" is not usable without L3's resumability (it would
run until the first pause and stop). Treat **L2+L3 together as the realistic ceiling**: semi-autonomous within a
phase, resumable across limits, you-gated at phase boundaries. And be precise about "resumable": with W6 only, resume
is **manual** (you start the next session); genuine hands-off auto-resume requires **W6b** (a scheduled re-wake).
Unattended cross-*phase* autonomy is not recommended — each phase boundary is a real review point where the biggest
scope/design calls live.

---

## 6. Bottom line

- **Can you start today?** Yes — at **L0/L1**, interactively, with a cheap model, phase-gated. That's productive now.
  The immediate path is **W1→W2→W3, then W7 (Phase 0) starts** — no harness needed first.
- **Efficient token use** comes from §1 — the two big levers are **model tiering done right** (§1a: pick the session
  tier for the phase + delegate large isolable units; don't switch per-unit) and **a thin coordinator** (§1b). The
  §1a/§1b/§1d doctrine is now folded into `CLAUDE.md` (§ "Token efficiency & context management" + the model-tiering
  bullet), which is where a live agent actually looks; MCP hygiene (§1c) is a minor add-on. This roadmap is linked
  from `CLAUDE.md`'s header so an agent can find it.
- **Before any unattended stretch:** land W4 (hooks) + W6 (resume) + W4b (the reviewer role), and W6b if you want
  hands-off auto-resume; build W5 only if in-session delegation proves insufficient. Until then you are the governor
  at every phase edge — which, on a subscription with mid-turn session-limit cut-offs, is the right amount of
  supervision anyway.
