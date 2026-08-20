# Dispatch plan: upstream Walnut (Java) bug-fix PRs, then port the fixes

Status: **DRAFT — plan only, nothing executed, nothing filed upstream.** Written 2026-08-20 in
response to the user's request to (1) tag the current tree as a milestone, (2) fix the 44
documented `docs/WALNUT-BUGS.md` entries **upstream in `walnut-java` first**, each in a way that
reviews and audits cleanly as a standalone PR, and (3) once a fix is upstream, port it into
`walnut-rs` and re-point the relevant differential tests at the fixed `walnut-java` branch/commit,
closing the loop CLAUDE.md's "log it, don't silently fix or replicate it" rule opened for each
entry.

This doc is the plan, not the execution log. Nothing here should be treated as done until a
corresponding PR/commit exists and this doc (or its per-batch successor) is updated to say so —
per this project's own standing lesson about not writing a completion claim ahead of its evidence
(see CLAUDE.md's "two process failures" note under the negative-base/split unit).

**Nine of the 44 entries need your explicit sign-off before they're queued at all** — see
"Entries requiring your decision" below. Everything else is a batching *proposal*, not a
commitment; sanity-check the groupings before any agent starts writing Java patches.

---

## Why this is safe to do at all, and where it isn't

CLAUDE.md's prime directive is correctness via **semantic equivalence**, not byte-identical
behavior — so "fix a Java bug" is a legitimate, planned exception to "port quirks verbatim," not a
violation of it, *exactly because* each one already went through the log-first discipline that
rule demands. The risk isn't in fixing the bugs; it's in three things this plan has to manage
explicitly:

1. **Differential/golden fixtures may currently *depend on* the buggy behavior.** A handful of WB
   entries are pinned by dedicated Rust tests that assert the *quirk* (e.g. WB-001's
   `minimize_q0_not_co_reachable_walnut_quirk`, `convert_ns_reaches_wb_001_when_regrouping_
   strands_a_state`). Fixing the bug upstream and porting the fix means those tests flip from
   asserting the bug to asserting the correct answer — not deleting a test (never allowed), but
   *changing what it asserts*, which needs the same adversarial-review weight as any other
   `wr-core`/`wr-logic` change.
2. **A few entries are "latent"** (unreachable from any real call site today, in Java, in Rust, or
   both — e.g. WB-004, WB-012, WB-043). Fixing these upstream is still worth doing for the
   community, but the Rust-side "port" may be nothing more than updating the WB entry's status
   line, since there's no live behavior to differentially test.
3. **A couple of entries have already had user-facing decisions made about them** (WB-039's index-
   stability divergence was explicitly signed off 2026-08-16 to be *kept* on the Rust side). Fixing
   the Java bug upstream doesn't automatically undo that sign-off — it needs to be re-examined
   once the upstream fix exists, not assumed.

---

## Entries requiring your decision before they're queued

CLAUDE.md is explicit that this class of call belongs to you, not to an agent mid-port. Recorded
here so you can answer them in one pass rather than nine separate check-ins later.

| ID | What's actually being asked | My recommendation |
|---|---|---|
| **WB-005** | `Session` static state goes stale on a 2nd in-process setup call — invisible in the real single-invocation CLI. Worth an upstream fix at all? | **Skip.** No real-world trigger; not worth a PR's review overhead. Leave logged. |
| **WB-018** | Trailing unclosed `[` after a satisfied-arity word occurrence is silently dropped. Needs a real design decision (what error to raise), not a mechanical fix. | **Defer** — bundle into a future "parser diagnostics" pass rather than this bugfix sweep; it's a feature (a new diagnostic), not a bug fix. |
| **WB-026** | `--home-dir=` silently ignored, crashes on a valid invocation, because arg validation runs before session setup. Real, mechanical, reproduced live. | **Fix.** Straightforward: move (or drop, since it's redundant with the later correct check) the premature `validateFile` call. Low risk. |
| **WB-027** | `I` quantifier silently drops the body's free variables — two candidate fixes, neither obviously the "intended" semantics. | **Needs your call on semantics**, not mine — this changes what `I` *means* for free-variable bodies, which is a language-design decision, not a bug fix with one obviously-correct answer. Recommend a short investigation-only pass (read the Walnut paper/docs, check if any published Walnut example relies on either behavior) before deciding, separate from this sweep. |
| **WB-028** | Orphaned `earlyExistTermination` metacommand — half of a reverted experiment. Delete the dead flag, or wire it up for real? | **Delete the dead code** (simplest, lowest-risk) unless you specifically want early-∃-termination as a feature — that's a feature request, not a bug fix. |
| **WB-031** | `help` can't reach commands under "Morphisms And Word Automata" because arg parsing has no quoting. Multiple fix shapes, one of which changes documented output (renaming the group). | **Fix via least-invasive option**: accept an underscore/hyphen alias for the group name in `parseHelpArguments` rather than renaming the group itself. Low review risk, doesn't touch documented output. |
| **WB-039** | `disablePrint`/`enablePrint` non-restore. You already signed off (2026-08-16) to **keep** the Rust port's diverged, stable index numbering rather than replicate Java's cache-order-dependent instability. | **The Java-side fix (save/restore semantics) is still worth proposing upstream on its own merits** — real bug, real community value. But **do not port that fix into Rust** without re-confirming: your existing sign-off was to diverge *because* Java's behavior is unstable; if Java gets fixed to be stable too, the reason for the divergence may disappear and porting the fix could let Rust drop a deliberate difference cleanly. Flagging for you to decide once the upstream fix exists, not now. |
| **WB-042** | `MathematicaEmitter` uses `#` (invalid Wolfram syntax) instead of `(* ... *)` as its comment prefix. Fixing it requires updating 7 checked-in golden `.wl` fixtures on the Rust side too. | **Fix.** Real, unambiguous bug (generated files don't even load in real Mathematica) — worth the fixture churn. Flagging only because the fixture update means this can't be a Java-only PR review; the Rust-side companion change touches test data, not logic. |
| **WB-043** | `arithmetic(...,MINUS)`'s negative-constant rewrite computes `b=a-\|c\|` where the algebra needs `b=a+\|c\|`. Currently latent (no production caller reaches it), but `NumberSystemTest.testNegArithmeticOrdering` **asserts the buggy result as expected** — fixing it means updating that test to assert the opposite. | **Fix**, but flag it clearly in the PR description as "this existing test was asserting the bug; here's why the new assertion is the correct one" so a Java-side reviewer doesn't mistake it for a test regression. Low behavioral risk since it's latent today. |

I'd like your answers on WB-026/028/031/039/042/043's recommendations (accept/adjust) and
WB-005/018/027's disposition before any agent starts writing patches. Doesn't need to be a formal
sign-off ceremony — a quick "yes to all" or specific overrides is enough.

---

## Excluded entirely (not a fix candidate)

- **WB-022** — not a Java bug at all; Java is correct, it's a Rust-port scope gap
  (`wr-io`'s reader missing an `isFAO` guard). Not in scope for this sweep — track separately if
  you want it closed, it's a Rust-only fix.
- **WB-006, WB-007** — each entry's own text concludes it's arguably working-as-intended /
  a cosmetic API-contract note, not worth the review overhead of a PR. Left logged, not queued.

---

## Batching plan for the remaining ~32 entries

Grouped for **PR-sized reviewability**: each PR gets its own branch, its own description stating
the bug/trigger/fix/why, and (per this project's Phase-0 practice) a **new failing-then-passing
JUnit test per bug** as the auditable proof, not just a prose claim. Bugs sharing a root cause or
file are bundled into one PR as separate commits (still individually revertable/auditable); bugs
that are architecturally unrelated get their own PR even if small, so a reviewer/maintainer can
merge or reject each independently. Ordered highest-impact first within each tier.

### Tier 1 — silent wrong answers (highest priority; these corrupt output with no diagnostic)

| PR | Entries | Why grouped | Notes |
|---|---|---|---|
| PR-1 | **WB-001** | Standalone — the highest-severity entry in the whole catalog (`FA/ValmariDFA.java`, minimize corrupts language on a q0-not-co-reachable input) | **See "WB-001 needs special handling" below before starting — the Rust-side port has FOUR live call sites and several tests that currently assert the bug.** Do not treat this as a normal-weight PR. |
| PR-2 | WB-008 + WB-009 | Same method (`FA.concatStates`), two independent defects (wrong q0 grafted, first operand's accept flags never cleared) | Natural single PR, combined regression test exercises both. |
| PR-3 | WB-016 | Standalone (`WordAutomaton.reverseWithOutput`, `q0` never updated after BFS rebuild) | |
| PR-4 | WB-010 | Standalone (`AutomatonLogicalOps.leftQuotient`'s alphabet-subset check runs backwards, silently disabling `rightQuotient`'s own guard) | |
| PR-5 | WB-024 + WB-025 | Same root mechanism — a `+128` truncating cast in `BricsConverter`'s alphabet encoding, triggered two ways (out-of-alphabet digit vs. alphabet size in `(65408,65535]`) | |
| PR-6 | WB-032 | Standalone (`convertNS`'s `Math.log`-ratio exponent is truncated float, wrong for 343 `(root,exp)` pairs) | Needs an exact-integer fix (reuse/extend `commonRoot`), not a one-liner — size accordingly. |
| PR-7 | WB-021 | Standalone (`exportToBA` missing the TRUE/FALSE guard its two siblings have) | |
| PR-8 | WB-038 | Standalone (`AutomatonReader` accepts an out-of-alphabet transition digit, encodes to a bogus `-1` key) | |
| PR-9 | WB-035 | Standalone (`Transducer.transduceNonDeterministic`'s dead-state marker collides with real input/output values) | Flagged in the catalog as needing a **fresh marker design**, not a mechanical patch — size as the largest PR in this tier. |

### Tier 2 — crashes on plausible input

| PR | Entries | Why grouped | Notes |
|---|---|---|---|
| PR-10 | WB-002, WB-012, WB-037, WB-044 | Each a small (1-3 line), unrelated, independent guard — bundling keeps PR count down without hiding any individual fix (one commit per bug) | Lowest-risk PR in the whole sweep; good candidate to land first as a process dry run. |
| PR-11 | WB-013 + WB-033 + WB-034 | Same defect class across three call sites: NPE when a track is `{...}`-declared (no `NumberSystem`) instead of `msd_k`/`lsd_k` | Shared root cause, share a common guard helper if Java's structure allows it. |
| PR-12 | WB-011 | Standalone (`parseMorphism` rejects bracket notation its own regex accepts) | |
| PR-13 | WB-014 | Standalone (`NumberSystem.getComputeIfAbsent` reentrant `ConcurrentModificationException`) | |
| PR-14 | WB-036 | Standalone (`Morphism.toWordAutomaton`'s `Q` vs. transition-table-size mismatch) | |
| PR-15 | WB-040 | Standalone (`[export gv]` mutates the automaton mid-determinize) | |
| PR-16 | WB-019 | Standalone (`putMacro`'s `%N` substitution inherits regex-replacement escaping) | |

### Tier 3 — log/diagnostic text only (no behavioral risk; safe to batch aggressively)

| PR | Entries | Why grouped |
|---|---|---|
| PR-17 | WB-015, WB-017, WB-020, WB-023, WB-041 | All diagnostic/error-text-only fixes across different files — zero decision-procedure risk, bundle freely (one commit per bug) |
| PR-18 | WB-029, WB-030 | Cosmetic: a strategy-alias round-trip bug (only affects the deferred-OTF strategies, no Rust impact either way) + two help files saved in the wrong encoding |

**32 entries queued across 18 PRs** (assuming all nine "needs your decision" entries land as
recommended above except WB-005/018/027, which are deferred/skipped rather than fixed) — a mix of
1-bug and small-bundle PRs, biased toward small so each reviews fast.

---

## Special case: WB-001 needs careful sequencing, not just "fix and port"

This is the one entry where "port the fix" is a real project, not a follow-up. Current Rust state
(see `docs/WALNUT-BUGS.md`'s WB-001 entry and CLAUDE.md's Phase 2/3b history):

- `wr_core::minimize::minimize` reproduces the quirk, pinned by
  `minimize_q0_not_co_reachable_walnut_quirk`.
- **Four live call sites** currently reach it faithfully: `Automaton::determinize_and_minimize`'s
  already-deterministic branch, `quantify`'s lsd trailing-zero fixup's short-circuit path,
  `convertNS`'s `k -> k^j` regrouping (the easiest to hit from the CLI), each with its own pinning
  test asserting the **buggy** output.
- U31's property-suite work already had to build an **exact-characterization WB-001 skip
  predicate** for its own property tests, specifically so they wouldn't wrongly flag the quirk as
  a violation — that predicate's existence is itself a load-bearing artifact of the bug being live.

So porting the upstream fix means, at minimum: flipping 3-4 pinned unit tests from "asserts the
bug" to "asserts the correct minimal automaton," re-deriving whatever the property-suite's WB-001
skip predicate was protecting against (it may become dead code, or may still be needed for some
narrower residual case — don't assume either way, check), and confirming none of the 675 golden
fixtures or the differential-gen corpus silently depended on the wrong answer anywhere. Treat this
as its own planned unit (its own plan doc, its own two-adversarial-reviewer round on the Rust
side, same as Phase 2-4 units), not a one-line follow-up to the Java PR. Land every other PR in
this sweep first — WB-001 is the one place where being the last, most-carefully-sequenced fix is
correct, not procrastination.

---

## Per-PR workflow (walnut-java side)

For each PR:

1. **Reproduce live** against the real `Walnut-all.jar` first — don't trust the WB entry's
   trigger description alone; re-confirm it, the same way every WB entry in this catalog was
   originally verified.
2. **Write a new failing JUnit test** in the relevant `*Test.java` file demonstrating the bug
   (or, for WB-043-style cases, update the test that currently asserts the buggy behavior — call
   this out explicitly in the PR description).
3. **Fix the Java source**, minimal diff, matching the WB entry's own suggested fix where one is
   recorded.
4. **Confirm the test passes**, run the full existing suite for the touched file(s) to check for
   regressions.
5. **Open the PR** with a description in this shape: bug location, trigger, root cause, fix,
   before/after behavior, link back to the `docs/WALNUT-BUGS.md` entry ID for provenance.

## Downstream port workflow (walnut-rs side), once a PR is merged (or stable on review)

1. Point the relevant differential-test target at the fixed `walnut-java` commit/branch (pin the
   SHA explicitly — don't float against a moving branch while other PRs from this same sweep are
   still in flight, or a differential run could silently mix fixed and unfixed behavior across
   different queries).
2. Implement the matching Rust-side fix through the normal implementer → two-independent-
   adversarial-reviewer → fixer loop (unconditionally for anything touching `wr-core`/`wr-logic`,
   per CLAUDE.md's merge gate).
3. Update the WB entry: status line moves from `ported verbatim (bug)`/`(quirk)` to `fixed,
   matches walnut-java as of commit <sha>`; flip any test that was pinning the old buggy behavior.
4. For **latent** entries (no live call site today, e.g. WB-004, WB-012, WB-043-once-fixed) — the
   "port" may just be a doc update plus a regression test proving the now-fixed behavior, since
   there's no existing live path to differentially test.
5. Re-run the golden corpus + a differential-gen spot check before closing out each entry, same
   bar as every other unit in this project's history.

---

## Suggested sequencing

1. Get your sign-off on the nine ambiguous entries above.
2. PR-10 first (Tier 2's small-guards bundle) as a low-risk dry run of the whole upstream-PR
   process end to end, including the first Rust-side port-back, before committing to the full
   18-PR sweep.
3. Tier 1 (silent-wrong-answer) next, WB-001 held for last within that tier per the sequencing
   note above.
4. Tier 2 remainder, then Tier 3 (batch freely, low risk).
5. Re-tag the repo (or extend the milestone tag's annotation) once a meaningful slice has landed —
   doesn't need to wait for all 44.
