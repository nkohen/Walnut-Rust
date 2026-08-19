# Kickoff prompt: Phase 0 coverage/characterization dispatcher

**Status:** ready to hand to a fresh agent. Paste the block below (between the `---PROMPT START---` and
`---PROMPT END---` markers) as the first message of a new session. Recommended working directory:
`~/dev/walnut-java` (the actual work happens there; the prompt tells the agent to also read `~/dev/walnut-rs`'s
docs for doctrine).

This is *not* a Rust-porting session — it's Java-only, mechanical-leaning, and a good fit for a cheaper model
tier per CLAUDE.md's model-tiering doctrine. Escalate only if it lands on genuinely hard characterization
(parser edge cases, `NumberSystem`) or a scope judgment call (the inline-command classification).

---PROMPT START---

You are the dispatcher for the remaining Phase 0 work of the walnut-rs project: driving JaCoCo coverage
and characterization tests toward ~100% on the ported subset, in the **Java** fork at `~/dev/walnut-java`.
You have no memory of any prior conversation — everything you need is either in this prompt or in the repos
below. Read before doing anything else, in this order:

1. `~/dev/walnut-rs/CLAUDE.md` — the operating doctrine for this whole project (correctness ladder, token
   efficiency, model tiering, git/fleet hygiene, the merge gate). It governs how you should work even though
   today's work is Java, not Rust.
2. `~/dev/walnut-rs/docs/DESIGN.md` — §3 (the KEEP/DROP subset), §5 (the correctness ladder, esp. Tier 0's
   test-manifest schema), §8 (phased roadmap), §9 (risks, esp. the OTF empirical check F3).
3. `~/dev/walnut-rs/docs/ROADMAP-TO-AUTONOMY.md` — §1 (token efficiency), §2 (resumability), work item **W7**
   (this is literally your task list, see below).
4. `~/dev/walnut-rs/docs/BOUNDARY-MAP.md` — the verified KEEP/DROP call for every file in `Automata/` and its
   subpackages, already done. **Do not re-derive this** — treat it as ground truth unless you find it's
   factually wrong (in which case flag the discrepancy, don't silently override it).

## Where things stand (verified, don't re-check unless something looks wrong)

- `~/dev/walnut-java` is **your** fork (`git@github.com:nkohen/Walnut.git`), cloned, with `upstream` wired to
  `https://github.com/Walnut-Theorem-Prover/Walnut.git` for `git pull upstream` later.
- JDK is pinned via `jenv` — a `.java-version` file in `~/dev/walnut-java` pins it to `17.0.20` (installed via
  Homebrew, registered with `jenv add`). `./mvnw` picks this up automatically with no manual `JAVA_HOME`
  export needed. (Global `jenv` default elsewhere on this machine is still JDK 11 — untouched, don't change it.)
- `./mvnw -q test` is green out of the box (existing upstream suite, ~92 JUnit tests across 25 files under
  `src/test/java`). No characterization tests have been added yet.
- **JaCoCo is already wired into `pom.xml`**, but behind a Maven profile, not bound to the default `test`
  phase: `mvn test -Pcode-coverage` generates the report (`jacoco-maven-plugin` 0.8.14). Nobody has run it
  scoped to the subset yet — no coverage report exists in `target/` right now.
- The golden integration corpus is at `src/test/resources/integrationTests/` (`Global/` and `Session/`
  subdirs), ~666 `automaton*` files confirmed present, roughly matching DESIGN.md's ~635 estimate. Not yet
  filtered against the subset boundary.
- `docs/BOUNDARY-MAP.md` covers the `Automata/` package's **classes** (KEEP/DROP/crate target). It does
  **NOT** cover the inline commands dispatched by regex-match inside `Main/Prover.java` — `split`, `rsplit`,
  `join`, `transduce`, `convert`, `minimize`, `fixleadzero`, `fixtrailzero`, `promote`, `inf`, `export` — those
  are a **separate, still-open** classification task (DESIGN.md §3's "TO CLASSIFY" list). Don't confuse the
  two; both matter, only one is done.

## Your mission (ROADMAP.md W7, the parts not already done)

In priority order — treat each as a checkpoint, not a single unattended run:

1. **Classify the TO-CLASSIFY inline commands.** For each of `split`/`rsplit`/`join`/`transduce`/`convert`/
   `minimize`/`fixleadzero`/`fixtrailzero`/`promote`/`inf`/`export` (grep `Main/Prover.java` for their
   regex dispatch), read enough to understand what it does, then judge KEEP (needed for automatic-
   sequence/constant-term-sequence research — base-k automata, word automata, morphisms) or DROP (CAS-export
   or exotic-numeration-only). Write the result into `docs/BOUNDARY-MAP.md` in `walnut-rs` as a new section
   (don't touch the existing file-level table). Flag genuinely ambiguous calls for the user rather than
   guessing.
2. **Scope JaCoCo to the subset KEEP modules.** Use `docs/BOUNDARY-MAP.md`'s per-file KEEP/DROP calls plus
   your own command classification from step 1 to build an include/exclude list (JaCoCo supports
   include/exclude globs in the plugin config). Do **not** chase coverage on Ostrowski, the CAS-export
   `Writer/*Emitter.java` files, or `NumberSystem.java`'s negative-base branches (BOUNDARY-MAP.md §4.1 flags
   these aren't cleanly separable at the file level — scope at the line/branch level if the tool allows, or
   accept some noise and note it, don't burn time perfecting the exclude list).
3. **Run a baseline coverage report** (`mvn test -Pcode-coverage`) and identify the real gaps in KEEP-scoped
   code. Route the raw report to disk, not your context — read the summary, not the line-by-line HTML.
4. **Write characterization tests** (Feathers-style: assert current behavior, not desired behavior) to drive
   toward ~100% line+branch coverage on the KEEP modules. This is the bulk of the work and the most
   mechanical — a good candidate for delegating whole classes to fresh subagents with a `model` override per
   CLAUDE.md's tiering doctrine (delegate a whole test class at a time, not method-by-method). For every
   branch you cover that reveals surprising/undocumented behavior, **log it explicitly** (a short entry: what,
   where, why it's surprising) rather than silently asserting it — these are candidate latent Walnut bugs and
   the project's stated policy is "log every divergence explicitly," not silently normalize or fix them.
   Zero tests deleted, ever, per CLAUDE.md's merge gate — this applies here too even though it's Java, not Rust.
5. **Export the test manifest** — the schema is pinned in DESIGN.md §5 Tier 0: a directory of fixtures, each
   `{ id, command_script, expected_kind: automaton|details|error, expected_path, number_system,
   commands_used[] }` as JSON or TSV. This is what lets Phase 2's Tier-1 harness filter the corpus by subset
   membership later. Write it to `~/dev/walnut-java/phase0-artifacts/test-manifest.json` (create the
   directory) — cross-repo wiring into `walnut-rs/tests/golden/` is a later phase's job, not yours.
6. **Filter the golden corpus against the subset.** Using the manifest from step 5, tag which of the ~666
   golden files are subset-relevant vs. use a dropped feature. Don't move/delete anything — just produce the
   filter (e.g. a list of relevant fixture IDs) alongside the manifest.
7. **Run the OTF empirical check** (DESIGN.md §9, risk F3). Grep this Walnut checkout's own usage and any
   sibling `ct-research` research scripts for `[strategy` directives — does any *real* query need a non-`SC`
   strategy to terminate? This determines whether deferring OTF (DESIGN's plan) is safe. Report findings, do
   not make the go/no-go call yourself — that's explicitly called out in ROADMAP.md §4 as staying manual.

## How to work (per CLAUDE.md — this is not optional)

- **Token efficiency:** never read a large Java file wholesale into your own context — grep/locate, read only
  the slice you need. Delegate heavy-in/small-out work (scanning a big file, running the full test suite,
  sweeping the corpus) to a fresh subagent and keep only its conclusion. Route long output (test logs,
  coverage HTML, diffs) to disk, not context.
- **Model tiering:** run this session on the cheap-to-mid tier (this is mechanical characterization-test
  authoring, not architecture or math). Delegate a large isolable unit (a whole test class, not a single
  method) to a fresh `Agent` subagent with a `model` override if you want to parallelize. Escalate only for
  genuinely hard cases: parser edge cases, `NumberSystem` behavior, or an ambiguous KEEP/DROP call on an
  inline command.
- **Phase-gated, not unattended.** Stop and report after each of the 7 numbered items above, or sooner if you
  hit something needing a human call (an ambiguous classification, a coverage target that seems unreachable,
  a discovered Walnut bug significant enough to need a replicate-vs-fix decision). Don't barrel through all
  seven in one unreviewed pass.
- **Resumability.** This will span multiple sessions. Maintain
  `~/dev/walnut-java/phase0-artifacts/PROGRESS.md` — append-only, updated continuously (not just at a clean
  stop, since a session-limit cutoff can happen mid-turn with no warning): what's done, what's in flight, the
  exact next step. Commit working progress frequently in small, green commits (never commit with the test
  suite red). On resume, a fresh agent should be able to read `PROGRESS.md` and continue without you
  re-explaining anything.
- **Git hygiene.** This is your own fork, not shared with other agents concurrently, so the fleet-concurrency
  rules in CLAUDE.md are lower-stakes here — but still: atomic, scoped commits (explicit pathspecs, not
  `git add -A`), never `git stash`/`git reset` if you're not certain what's in the working tree, never force-push.
- **Don't touch `~/dev/walnut-rs`'s Rust code.** This phase is Java-only. If you update
  `docs/BOUNDARY-MAP.md` (step 1) that's the one sanctioned write into the walnut-rs repo; everything else
  stays in `walnut-java`.

## Definition of done for this dispatch

Not "100% coverage achieved" — that may take several sessions. Done for *a given checkpoint* means: the
numbered item is genuinely finished (or you've hit a wall worth reporting), `PROGRESS.md` reflects it,
relevant commits are green, and you've stopped to report rather than continuing unattended into the next
phase (Phase 1 spike, or any Rust code) without the user's go-ahead.

---PROMPT END---
