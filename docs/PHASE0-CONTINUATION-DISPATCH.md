# Kickoff prompt: Phase 0 continuation dispatcher (Items 6-7)

**Status: ready to hand to a fresh agent.** Paste the block below (between the `---PROMPT START---` and
`---PROMPT END---` markers) as the first message of a new session. Recommended working directory:
`~/dev/walnut-java` (the actual work happens there; the prompt tells the agent to also read `~/dev/walnut-rs`'s
docs for doctrine) — same convention as the original dispatch.

This **supersedes** `docs/PHASE0-COVERAGE-DISPATCH.md` for handoff purposes — that doc's own "where things
stand" section is now stale (it predates Items 1-5). Don't paste the old one into a fresh session; paste
this one. The old doc is left as-is for historical record (it's referenced by commit history).

Same model-tiering note as before: Items 6-7 are Java-side, mechanical-to-moderate work, a fine fit for a
cheap-to-mid model tier. Escalate only if Item 7's `[strategy ...]` investigation turns up something
architecturally surprising, or if you end up needing to read `NumberSystem.java`/`Predicate.java` deeply
again (both already have ~99%+ characterization coverage from Item 4 — you almost certainly won't need to).

---PROMPT START---

You are the dispatcher for the remaining Phase 0 work of the walnut-rs project: Items 6-7 of
ROADMAP-TO-AUTONOMY.md's W7 work item, in the **Java** fork at `~/dev/walnut-java`. You have no memory of
any prior conversation — everything you need is either in this prompt or in the repos below. Read before
doing anything else, in this order:

1. `~/dev/walnut-rs/CLAUDE.md` — the operating doctrine for this whole project (correctness ladder, token
   efficiency, model tiering, git/fleet hygiene, the merge gate). Governs how you should work even though
   today's work is Java, not Rust.
2. `~/dev/walnut-rs/docs/DESIGN.md` — §5 (correctness ladder, Tier 0/1), §8 (phased roadmap), §9 (risks,
   esp. **F3**, the OTF empirical check — this is literally Item 7).
3. `~/dev/walnut-rs/docs/ROADMAP-TO-AUTONOMY.md` — §1 (token efficiency), §2 (resumability), **W7** (your
   task list — Items 1-5 are DONE, see below; you're picking up at Item 6).
4. `~/dev/walnut-rs/docs/BOUNDARY-MAP.md` — the verified KEEP/DROP call for every file (§2) and every
   TO-CLASSIFY inline command (§6) in the subset. **Read §4.1 and §6.1 carefully** — the negative-base
   call is resolved (delete outright, confirmed `split`/`rsplit` DROP), and there's a **critical
   correction** you must not miss: `msd_fib`/`lsd_fib` are NOT a dropped feature (they're a KEEP-scope
   custom-base mechanism, unrelated to the DROP-scope Ostrowski algorithm) — get this wrong and Item 6
   will silently discard ~18% of the golden corpus that should stay in scope.
5. `~/dev/walnut-java/phase0-artifacts/PROGRESS.md` — the full append-only history of everything done so
   far (Items 1-5), including every finding, every bug caught, and the operational lessons from three
   waves of parallel-agent dispatch. **Read this instead of re-deriving anything it already covers.**
6. `~/dev/walnut-java/phase0-artifacts/RESUME-HERE.md` — the live one-page pointer to exact current state.
7. `~/dev/walnut-java/phase0-artifacts/test-manifest.json` — the Item 5 deliverable: 675 fixtures, each
   `{ id, command_script, expected_kind[], expected_path[], number_system[], commands_used[] }` (note:
   `expected_kind`/`expected_path`/`number_system` are **lists**, a deliberate deviation from DESIGN.md
   §5's singular-field schema — documented in `gen_test_manifest.py`'s docstring, don't "fix" this back to
   singular, it would silently drop real information). This is the input to Item 6.

## Where things stand (verified, don't re-check unless something looks wrong)

- **Items 1-5 of W7 are DONE** (commits `4685af7`..`c42bdd7` in `walnut-java`, plus a handful of
  `docs/BOUNDARY-MAP.md` commits in `walnut-rs`): TO-CLASSIFY commands classified, JaCoCo scoped to the
  KEEP subset, baseline coverage measured, characterization tests written (three waves — mechanical gaps,
  then trust-critical algorithmic files on heightened rigor / Opus tier for `NumberSystem`), test manifest
  exported. Overall KEEP-scoped coverage is **98.2% line / 94.0% branch** (`mvn test -Pcode-coverage`,
  `target/site/jacoco/jacoco.csv`) — a real remaining tail of ~35 classes with small (1-3 line) gaps
  exists but was deliberately not pursued further (diminishing returns, logged in `PROGRESS.md`).
- `./mvnw -q test` is green. JDK pinned via `.java-version` (17.0.20, jenv) — **do not delete this file**
  (it was accidentally deleted once this session while cleaning build debris and had to be restored;
  if you ever run a cleanup pass, check `git status` first and never blind-delete untracked files near
  the repo root).
- No open items block starting Item 6.

## Your mission: Items 6-7 (ROADMAP.md W7)

In order, phase-gated — stop and report after each, per the "how to work" section below.

### Item 6 — Filter the golden corpus against the subset

Using `test-manifest.json`'s `number_system[]`/`commands_used[]` fields per fixture, cross-reference
against `BOUNDARY-MAP.md` §2 (file-level KEEP/DROP) + §6 (command-level KEEP/DROP) to tag which of the 675
fixtures are subset-relevant vs. use a dropped feature. Concretely, a fixture is **DROP-relevant** (not
subset-relevant) if and only if:
- its `number_system[]` contains a genuine negative-base token (`msd_neg_*`/`lsd_neg_*` — there are 68
  such fixture-mentions across the corpus, per the manifest), **or**
- its `commands_used[]` contains `split`, `rsplit`, or `ost` (Ostrowski) — the three DROP-confirmed
  commands per BOUNDARY-MAP §6.
- **`msd_fib`/`lsd_fib` alone do NOT make a fixture DROP-relevant** — see the §4.1 correction above. Don't
  re-derive this; it's already investigated and verified.

Don't move/delete any corpus files — just produce the filter (e.g. add a `subset_relevant: true/false` +
`drop_reason` field to each manifest fixture, written to a new file or an updated `test-manifest.json` —
your call on the exact shape, but don't silently overwrite the Item-5 deliverable without a git commit
showing the diff clearly). Report the resulting counts (how many fixtures are subset-relevant vs. not, and
why) — this is what "corpus green" will mean for Phase 2's Tier-1 harness.

### Item 7 — Run the OTF empirical check (DESIGN.md §9 F3)

The question: does any of the **real research** usage of Walnut need a non-`SC` `[strategy ...]` directive
to terminate? `SC` (subset construction) is the default determinization strategy; OTF (on-the-fly) is the
opt-in alternative that walnut-rs's plan defers (not porting it initially). If real queries *need* OTF to
terminate, deferring it is a functional regression, not just a performance one — this determines whether
that deferral is safe.

- Grep this `walnut-java` checkout's own usage (test fixtures, `Command Files/`, `Custom Bases/`, the
  integration test corpus) for `[strategy` directives — does anything here explicitly request non-`SC`?
- `Main/MetaCommands.java` implements the `[strategy ...]` metacommand syntax (already characterized in
  Item 4's coverage work, see `src/test/java/Main/MetaCommandsTest.java` for how it's invoked) — read it to
  confirm exactly what directive syntax to grep for.
- If you have access to the sibling `ct-research` repo (check `~/dev/ct-research` or wherever it's
  checked out on this machine) and its own Walnut usage/research query history, check there too — that's
  the actual "real research queries" DESIGN.md means. **If you don't have access, say so explicitly in
  your report rather than silently skipping this half of the check** — it's the more important half, since
  this `walnut-java` checkout's own fixtures are deliberately small/synthetic, not representative research
  workloads.
- **Report findings; do NOT make the go/no-go call yourself.** ROADMAP.md §4 explicitly keeps this decision
  manual (it's a scope/risk judgment, not a mechanical fact). Your job is to answer "does the evidence show
  non-`SC` usage or not," clearly and with sources, and hand that to the user.

## How to work (per CLAUDE.md — this is not optional)

- **Token efficiency:** grep/locate before reading; never read a large file wholesale. Route long output to
  disk, read summaries not raw dumps.
- **Model tiering:** default/cheap tier is fine for both items — this is mechanical cross-referencing
  (Item 6) and a grep-driven investigation (Item 7), not architecture or math.
- **Phase-gated, not unattended.** Stop and report after Item 6, and again after Item 7. Don't barrel
  through both in one unreviewed pass — same discipline as Items 1-5.
- **Resumability.** Keep `phase0-artifacts/PROGRESS.md` append-only (add a dated entry per item, don't
  rewrite history) and `phase0-artifacts/RESUME-HERE.md` current (overwrite freely, it's a live pointer,
  not a log). Commit frequently in small, green, atomically-scoped commits — `git add` explicit paths,
  never `-A`; heredoc/`-F -` commit messages, never a double-quoted `-m`.
- **If you dispatch subagents for any part of this** (Item 6's cross-referencing is mechanical enough that
  parallelizing per some partition might tempt you): this session's hard-won lesson, from `PROGRESS.md`,
  is that **`isolation: "worktree"` did not actually isolate agents in this harness** — their changes
  landed directly in the shared checkout regardless, causing real build collisions. If you parallelize,
  either (a) forbid agents from running `mvn`/`git` entirely and have yourself, the coordinator, run the
  sole serialized verify/commit pass, or (b) just don't parallelize — Items 6-7 are small enough that
  solo work is probably faster than the coordination overhead anyway.
- **Verification discipline.** Don't take a claim (yours or a subagent's) about production-code behavior
  at face value — independently verify against source before committing/reporting it as fact. This
  session caught two real mistakes this way (a false "surprising finding" about parser behavior in Item 4,
  and a silent regex bug in Item 5's own manifest-generation script) — both by cross-checking against
  ground truth (re-reading source, or comparing a derived count against a raw `grep`) rather than trusting
  the first plausible-looking result.
- **A session usage limit can interrupt a subagent mid-task** ("failed" status, not "completed"). This is
  a pause, not data loss — resume it via `SendMessage` once the limit resets; it picks back up from its own
  transcript. Happened twice in Item 4's third wave, both resumed cleanly.

## Definition of done for this dispatch

Not "Phase 0 fully complete" — Items 6-7 finishing still leaves the phased roadmap's later gates (Phase 1
spike, etc.) as separate, human-gated decisions per `ROADMAP-TO-AUTONOMY.md` §4. Done for *this* dispatch
means: Item 6's filter is produced and committed with clear counts reported; Item 7's evidence is gathered
and reported (go/no-go left to the user); `PROGRESS.md`/`RESUME-HERE.md` reflect both; you've stopped to
report after each item rather than continuing unattended past Item 7 into Phase 1 or any Rust code without
the user's explicit go-ahead.

---PROMPT END---
