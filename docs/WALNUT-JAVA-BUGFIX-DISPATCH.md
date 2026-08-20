# Dispatch plan: upstream Walnut (Java) bug-fix PRs, then port the fixes

Status: **DRAFT — plan only, nothing executed, nothing filed upstream.** Written 2026-08-20,
revised the same day after scoping feedback: **this round targets functionality only** — a bug
qualifies if and only if it makes the program compute a wrong answer or crash on reachable input.
Dead code, discoverability gaps, diagnostic/log-text-only differences, and export-format cosmetics
are explicitly OUT, regardless of how they were labeled in `docs/WALNUT-BUGS.md`'s original
severity tags. This is not a technical-debt cleanup pass.

Once a fix is upstream, it gets ported into `walnut-rs` and the relevant differential tests get
re-pointed at the fixed `walnut-java` branch/commit — closing the loop CLAUDE.md's "log it, don't
silently fix or replicate it" rule opened for each entry.

This doc is the plan, not the execution log. Nothing here should be treated as done until a
corresponding PR/commit exists and this doc is updated to say so.

---

## The filter, applied to all 44 entries

**In scope (27 entries) — the computed answer is wrong, or the program crashes on input a real
command can produce:**

- Wrong output: WB-001, WB-008, WB-009, WB-010, WB-016, WB-021, WB-024, WB-025, WB-032, WB-035,
  WB-038, WB-043
- Crashes: WB-002, WB-011, WB-012, WB-013, WB-014, WB-019, WB-026, WB-033, WB-034, WB-036, WB-037,
  WB-040, WB-044
- Wrong output via silent truncation of a malformed query, needs a design decision before it's a
  mechanical fix: WB-018
- Wrong output (drops free variables from an `I`-quantified body), needs a semantics decision
  before it's a mechanical fix: WB-027

**Out of scope (17 entries) — not a functionality defect, regardless of original severity label:**

| ID | Why it's out |
|---|---|
| WB-003 | `0*x` skips validating `x`, but the computed answer is always correct (`0 * anything = 0`) — a missing check, never a wrong result. |
| WB-004 | Dead code — `EvalDef.toString()` has no caller today. |
| WB-005 | No trigger through the real single-invocation CLI; already settled skip. |
| WB-006, WB-007 | Each entry's own text already concludes not worth fixing. |
| WB-015, WB-017, WB-020, WB-023, WB-041 | Diagnostic/log text only — the computed answer is identical either way. |
| WB-022 | Not a Java bug at all (Java is correct; it's a Rust-port gap). |
| WB-028 | Dead code — the metacommand's only reader was already deleted upstream. |
| WB-029 | Only affects the deferred-OTF strategy family, which is out of scope entirely already. |
| WB-030, WB-031 | Help-text encoding / discoverability, not computation. |
| WB-039 | Its only non-log-text consequence (metacommand index instability) has zero Rust-side work either way — you already settled the Rust divergence on its own merits, independent of the Java bug. Nothing for this round's port-back workflow to do even if fixed upstream. |
| WB-042 | The exported matrix *values* are correct; only the Mathematica comment-prefix syntax is wrong. Export-format cosmetics, not a wrong computed answer. |

---

## Batching plan for the 25 mechanically-fixable entries

(27 in-scope minus WB-018 and WB-027, which need a design/semantics decision first — see below.)

Each PR gets its own branch, its own description (bug/trigger/root cause/fix), and a **new
failing-then-passing JUnit test** per bug as the auditable proof. Bugs sharing a root cause or file
are bundled into one PR as separate commits; everything else gets its own PR so it can be merged or
rejected independently.

### Tier 1 — wrong computed output (highest priority)

| PR | Entries | Why grouped | Notes |
|---|---|---|---|
| PR-1 | **WB-001** | Standalone — the highest-severity entry in the catalog (Valmari minimize corrupts language on a `q0`-not-co-reachable input) | **See "WB-001 needs special handling" below — four live Rust call sites, several tests currently assert the bug. Land last, as its own careful unit, not a normal-weight PR.** |
| PR-2 | WB-008 + WB-009 | Same method (`FA.concatStates`): wrong `q0` grafted, first operand's accept flags never cleared | Combined regression test exercises both. |
| PR-3 | WB-016 | Standalone (`WordAutomaton.reverseWithOutput`, `q0` never updated after BFS rebuild) | |
| PR-4 | WB-010 | Standalone (`leftQuotient`'s alphabet-subset check runs backwards, disabling `rightQuotient`'s own guard) | |
| PR-5 | WB-024 + WB-025 | Same root mechanism — a truncating cast in `BricsConverter`'s alphabet encoding | |
| PR-6 | WB-032 | Standalone (`convertNS`'s exponent is a truncated float, wrong for 343 `(root,exp)` pairs) | Needs an exact-integer fix, not a one-liner. |
| PR-7 | WB-021 | Standalone (`exportToBA` missing the TRUE/FALSE guard its two siblings have) | |
| PR-8 | WB-038 | Standalone (`AutomatonReader` accepts an out-of-alphabet transition digit) | |
| PR-9 | WB-035 | Standalone (`Transducer`'s dead-state marker collides with real input/output values) | Needs a fresh marker design — the largest PR in this tier. |
| PR-10 | WB-043 | Standalone (`arithmetic(...,MINUS)`'s sign error) | The PR must also flip `NumberSystemTest.testNegArithmeticOrdering`, which currently asserts the buggy result — call this out explicitly so a reviewer doesn't mistake it for a test regression. |

### Tier 2 — crashes on reachable input

| PR | Entries | Why grouped | Notes |
|---|---|---|---|
| PR-11 | WB-002, WB-012, WB-037, WB-044 | Each a small, unrelated, independent guard | Lowest-risk PR in the sweep — good candidate to land first as a process dry run. |
| PR-12 | WB-013 + WB-033 + WB-034 | Same defect class: NPE when a track is `{...}`-declared instead of `msd_k`/`lsd_k` | |
| PR-13 | WB-011 | Standalone (`parseMorphism` rejects bracket notation its own regex accepts) | |
| PR-14 | WB-014 | Standalone (`NumberSystem` reentrant `ConcurrentModificationException`) | |
| PR-15 | WB-026 | Standalone (`--home-dir=` ignored, valid invocation crashes — arg validation runs before session setup) | |
| PR-16 | WB-036 | Standalone (`Morphism.toWordAutomaton`'s `Q` vs. transition-table-size mismatch) | |
| PR-17 | WB-040 | Standalone (`[export gv]` mutates the automaton mid-determinize) | |
| PR-18 | WB-019 | Standalone (`putMacro`'s `%N` substitution inherits regex-replacement escaping) | |

**25 entries across 18 PRs.**

### Held for a design/semantics decision before they're mechanical fixes

- **WB-018** — a trailing unclosed `[` after a satisfied-arity word occurrence silently drops the
  rest of the query. Real wrong-output bug, but the fix needs a decision on what error to raise.
  Short investigation pass first, then its own PR.
- **WB-027** — `I` silently drops the body's free variables, answering a global TRUE/FALSE instead
  of a per-free-variable predicate. Real wrong-output bug (already reproduced live, already ported
  verbatim in Rust with a pinned test asserting the wrong answer), but the fix is a genuine
  language-semantics call — reject free variables as an error, vs. build real per-free-variable
  semantics (a new construction, not a patch). Needs its own investigation before it's scoped as a
  PR.

---

## Special case: WB-001 needs careful sequencing, not just "fix and port"

This is the one entry where "port the fix" is a project in itself, not a follow-up:

- `wr_core::minimize::minimize` reproduces the quirk, pinned by
  `minimize_q0_not_co_reachable_walnut_quirk`.
- **Four live call sites** currently reach it faithfully: `Automaton::determinize_and_minimize`'s
  already-deterministic branch, `quantify`'s lsd trailing-zero fixup's short-circuit path, and
  `convertNS`'s `k -> k^j` regrouping (the easiest to hit from the CLI) — each with its own pinning
  test asserting the **buggy** output.
- U31's property-suite work already had to build an **exact-characterization WB-001 skip
  predicate** so its own property tests wouldn't wrongly flag the quirk as a violation.

Porting the upstream fix means: flipping 3-4 pinned unit tests from "asserts the bug" to "asserts
the correct minimal automaton," re-deriving whatever the skip predicate was protecting against, and
confirming none of the 675 golden fixtures or the differential-gen corpus silently depended on the
wrong answer. Treat it as its own planned unit (own plan doc, own two-adversarial-reviewer round),
landed last in this sweep — the one place where going last is correct sequencing, not delay.

---

## Per-PR workflow (walnut-java side)

1. **Reproduce live** against the real `Walnut-all.jar` first — re-confirm the trigger, don't trust
   the WB entry's description alone.
2. **Write a new failing JUnit test** demonstrating the bug (or, for WB-043-style cases, update the
   test that currently asserts the buggy behavior, calling this out explicitly in the PR).
3. **Fix the Java source**, minimal diff.
4. **Confirm the test passes**, run the full existing suite for the touched file(s).
5. **Open the PR**: bug location, trigger, root cause, fix, before/after behavior, and a link back
   to the `docs/WALNUT-BUGS.md` entry ID.

## Downstream port workflow (walnut-rs side), once a PR is merged

1. Pin the differential-test target at the fixed `walnut-java` commit/branch explicitly.
2. Implement the matching Rust-side fix through the implementer → two-independent-adversarial-
   reviewer → fixer loop (mandatory for anything touching `wr-core`/`wr-logic`).
3. Update the WB entry: `ported verbatim (bug)` → `fixed, matches walnut-java as of commit <sha>`;
   flip any test that was pinning the old buggy behavior.
4. For latent entries (no live Rust call site today, e.g. WB-012, WB-043) the "port" may just be a
   doc update plus a regression test proving the now-fixed behavior.
5. Re-run the golden corpus + a differential-gen spot check before closing out each entry.

---

## Suggested sequencing

1. PR-11 first (Tier 2's small-guards bundle) as a low-risk dry run of the whole upstream-PR
   process end to end, including the first Rust-side port-back.
2. Tier 1 next, WB-001 held for last.
3. Tier 2 remainder.
4. WB-018 and WB-027 investigated and scoped separately, once there's bandwidth for the semantics
   work they actually need.
