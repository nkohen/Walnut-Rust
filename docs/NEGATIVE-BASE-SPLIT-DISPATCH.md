# Dispatch prompt: port negative-base numeration + `split`/`rsplit`

> **Status: LAYER A DONE (2026-08-20). Layer B in progress / see the status section at the
> bottom of this file for where it stands.**
>
> Layer A — negative-base numeration — is landed, reviewed and committed. `wr_core::numsys`
> has `base_neg_n_addition` (`NumberSystem.java:503-533`), `base_neg_n_less_than`
> (`:541-561`), the two fallback arms that select them, the full
> `validateNeg` (`!isNeg && n.signum() < 0`), and every restored `n.signum() < 0` arm in
> `comparison`/`arithmetic` (three overloads)/`constant`/`multiplication`/`division`; the
> `_neg_` rejection gates in `wr-io`'s reader and `wr-cli`'s session are gone, along with
> `NumSysError::UnsupportedNegativeBase`. **Tier 1 moved from 587 compared / 586 pass to
> 655 compared / 654 pass** — all 68 negative-base fixtures green on the first run, the
> single remaining failure being the pre-existing, documented fixture 383. **WB-043**
> logged (a genuine, latent Java operator bug in
> `arithmetic(String, String, BigInteger, MINUS)`'s negative-constant rewrite — ported
> verbatim, pinned by a test). Two independent adversarial reviewers (different models,
> split context, diff only) each ran the full corpus themselves and returned **no
> correctness finding**; their doc-accuracy findings are fixed.

Status: Layer A done; Layer B open. This is the prompt to hand to a fresh agent/session to build **fully
autonomously, without further check-ins** — the user will not be available to answer questions
while this runs. Sizing rationale is in
[`docs/UNPORTED-SCOPE-SIZING.md`](UNPORTED-SCOPE-SIZING.md) (ranked #3 of the originally-dropped
items — CAS export and Ostrowski, ranked #1 and #2, are both done; see
[`docs/CAS-EXPORT-DISPATCH.md`](CAS-EXPORT-DISPATCH.md) and
[`docs/OSTROWSKI-DISPATCH.md`](OSTROWSKI-DISPATCH.md) for the procedural precedent this unit
should follow, adjusted for the differences below — this one is materially bigger and riskier than
either).

---

## Prompt

You're picking up a new unit of work on **walnut-rs**, a Rust port of a research subset of the
Walnut theorem prover, **running fully autonomously**: the user who dispatched this has explicitly
authorized you to carry it all the way through — planning, implementation, the full adversarial-
review loop, fixing, verification, and **committing and pushing in both `walnut-rs` and its
sibling oracle repo `../walnut-java`** — without pausing to ask for confirmation, because they will
be asleep/unavailable for the duration. This is a deliberate, explicit exception to this project's
normal "never commit without the user's go-ahead" rule, granted for this specific dispatched task
only — it does not change how you should treat any other repo or task. **It does not relax any
correctness bar.** Where you would normally ask the user a judgment call, make the most defensible
choice, write down your reasoning where a future reader will find it (plan file, commit message,
code comment if it's a real invariant), and keep moving. Where you hit something you cannot safely
resolve — tests you cannot get green, a design contradiction, evidence the scope is bigger than
this document assumes — **stop, leave both repos in a clean, buildable, all-green state** (revert
your own in-progress change if needed, on a branch if that helps), and write up the blocker clearly
instead of forcing a merge. The one rule that never bends, autonomous or not: never commit or merge
with `cargo test --workspace` red, and never merge `wr-core`/`wr-logic` code without the real
two-independent-adversarial-reviewer loop having run.

Before anything else, read `CLAUDE.md` in full — it is this project's operating contract
(correctness ladder, mechanical-port-first rule, the adversarial-review loop, model-tiering
doctrine, git/commit discipline) and everything below assumes you're following it. Also read
`docs/DESIGN.md`, `PORTING.md`, `docs/UNPORTED-SCOPE-SIZING.md`'s item 3, and
`docs/BOUNDARY-MAP.md` §4.1 (the original 2026-08-08 decision to delete negative-base outright
rather than stub it — read this section directly, it explains *why* the deletion was clean and
explicitly anticipates being revisited: *"If negative-base is ever wanted, it's a self-contained
later port (read Java → translate → test → review), not worse off for having been deleted now."*
That's what's happening now.).

### The task, and what's already been established by reading the source directly

Two Java surfaces, genuinely coupled but separable in sequence:

**A. Negative-base numeration** — `isNeg`-gated branches spread through
`../walnut-java/src/main/java/Automata/NumberSystem.java` (1027 LOC total; negative-base is
~10-15% of it). This is **not a separate file** — it was deleted from `wr-core/src/numsys.rs`
during Phase 3a's U7, and unusually well-documented at the deletion site: **read
`crates/wr-core/src/numsys.rs`'s module doc (lines 1-135) before touching anything else in that
file.** It gives you, with exact Java line numbers, every method removed
(`baseNegNAddition` java:503-533, `baseNegNLessThan` java:541-561, `baseNBaseChange` java:568-601,
`setBaseChangeAutomaton` java:443-468, `determineNegativeNS` java:219-230) and every branch
simplified away inside surviving methods (`comparison` java:701-702, `arithmetic`
java:809-813/861-864/910-913, `constant` java:944-951, `multiplication` java:986-994, `division`
java:1047-1048) — plus where the current Rust code rejects negative bases on purpose
(`NumSysError::UnsupportedNegativeBase`, currently thrown from `NumberSystem::with_custom_base_files`
around line 1233, and `validate_non_negative` around line 1524 gates the `_const_*` methods
around lines 1619-1900). This is your literal undo-list — go through it method by method against
the real Java source, don't just grep for "neg" and improvise.

**Key scoping fact, worth sequencing around:** `determineNegativeNS`'s own Java doc comment says
*"Currently used ONLY in split command."* So standalone negative-base numeration (a user writing
`?msd_neg_2 x < y` directly in `eval`/`def`/`reg`) needs only `baseNegNAddition`/
`baseNegNLessThan`/the restored `n.signum() < 0` branches — **not** `baseChange`/
`determineNegativeNS`/`baseNBaseChange`, which exist solely to support `split`. That means:
- **Layer A** (negative-base numeration alone) unlocks the large majority of the fixture footprint
  — `docs/UNPORTED-SCOPE-SIZING.md` counts 68 golden fixtures using `msd_neg_2`/`msd_neg_fib`/
  `lsd_neg_2`/`lsd_neg_fib`/`msd_neg_10` directly, none of which touch `split`.
- **Layer B** (`baseChange`/`determineNegativeNS`/`baseNBaseChange` + `Main/Commands/Split.java`,
  123 LOC) is what unlocks `split`/`rsplit` (15 fixtures: 8 `split` + 7 `rsplit`).
- Do Layer A first, get it fully green and reviewed, **then** Layer B on top. Don't build them as
  one inseparable diff — Layer A is meaningfully lower-risk (pure arithmetic-automaton
  construction, no new composition) than Layer B (which composes `quantify`/`combine`/`bind` in a
  new way).

**Layer B is more composition than construction — verify this claim, but it should hold.** Reading
`Split.java` directly: `processSplitCommand`/`processSplit` build on `WordAutomaton.uncombine`,
`AutomatonLogicalOps.combine`, `AutomatonQuantification.quantify`, `Automaton.bind`/`sortLabel`/
`randomLabel` — **every one of these already exists in `wr-core`** (confirmed by grep:
`word_automaton.rs::uncombine`, `logicalops.rs::combine`, `quantify.rs::quantify`,
`automaton.rs::{bind, sort_label, random_label}`). The genuinely new code for Layer B is
`NumberSystem::determine_negative_ns`/`base_change`/`base_n_base_change` (from Layer A's undo-list)
plus a `split.rs`-equivalent command handler wiring those existing primitives together the way
`Split.java` does. Confirm this by reading `Split.java` and the four `wr-core` files named above
side by side before assuming it — but go in expecting composition, not new algorithmic risk, for
this half.

**One more thing already found**: `setBaseChangeAutomaton`'s `isNeg == false` arms were *already*
confirmed dead code by Phase 0 (only reachable via reflection in the Java test suite,
`docs/WALNUT-BUGS.md`'s dead-code section) — don't resurrect them, the numsys.rs module doc already
explains why they don't matter.

### What to actually do, in order

1. **Phase 0 first.** Check current Java characterization-test coverage on `NumberSystem.java`'s
   negative-base paths and on `Split.java` (`../walnut-java`'s JaCoCo — regenerate via
   `mvn -Pcode-coverage test jacoco:report` if the checked-in report looks stale; note the
   `code-coverage` profile in `pom.xml` may still list stale excludes from earlier DROP-scope
   decisions — check it, the way the Ostrowski unit had to fix a leftover exclude there). Extend
   `NumberSystemTest.java`/add a `SplitTest.java` in `walnut-java` if coverage on these specific
   paths is thin, before porting against them.

2. **Write a plan for Layer A**, adversarially reviewed by an independent agent before any code
   lands (this project's standing convention — see `.claude/plans/amber-transcribing-ledger.md`
   and the Ostrowski plan referenced in `docs/OSTROWSKI-DISPATCH.md` for precedent). This is
   `wr-core` decision-procedure code touching an already-hardened, heavily-built-upon file
   (`numsys.rs` — three phases of property tests, differential generators, the golden corpus, and
   the fuzz corpus currently all assume no negative bases exist), so treat it with full
   trust-critical weight. The plan must specifically address:
   - Reversing the module-doc's undo-list method by method, restoring each deleted branch/method
     faithfully from the real Java source (mechanical port first, per `CLAUDE.md`).
   - What happens to `NumSysError::UnsupportedNegativeBase` and its call sites/tests — this
     variant's whole reason to exist goes away; find every place it's asserted in tests and update
     them deliberately, don't just delete failing tests (`CLAUDE.md`: zero tests deleted, ever —
     a failing ported test is real signal, but a test that specifically pinned "we reject this" is
     allowed to become a test that pins the new correct behavior once the feature is real).
   - Regression risk to the existing property/differential/fuzz suites: do any existing invariants
     implicitly assume positive-only bases in a way that would need widening (not just "would now
     also need to hold for negative bases" — check if any generator explicitly excludes negative
     bases as an assumption baked into its input space, vs. one that just never produced them by
     chance).
   - Test plan: Tier-2 replication of `NumberSystemTest`'s negative-base cases, new differential
     cases against real `walnut-java` output, new Tier-4 property tests (the existing
     `comparison_automata_agree_with_the_integer_order`/`addition_automaton_computes_real_addition`
     style properties in `numsys.rs`'s test module should extend naturally to negative bases — do
     that rather than writing a parallel, disconnected set), and un-excluding the 68 golden
     fixtures once green.

3. **Execute Layer A** through the full implementer → two-independent-adversarial-reviewer → fixer
   loop (model different from the author for at least one reviewer, per `CLAUDE.md`'s
   trust-critical-code rule; this project's model-tiering doctrine calls this exact class of work
   — `NumberSystem` edge cases — out as Opus-tier work, follow that if you have the option). **You
   have tool access to spawn your own subagents — use it.** Once your implementation diff exists,
   dispatch two independent `adversarial-reviewer` subagents (`.claude/agents/adversarial-
   reviewer.md`), each given **only the diff and file paths**, never your own rationale or commit
   message (protecting the split-context review is the whole point — see `CLAUDE.md`'s
   AI-orchestration section), and at least one on a model different from whatever you're authoring
   with. Reviewing your own diff yourself is not a substitute for this and does not satisfy
   `CLAUDE.md`'s merge gate. **Commit Layer A once it's fully green, reviewed, and fixed** — don't
   hold it hostage to Layer B's completion; a clean intermediate commit is much easier for the user
   to review in the morning than one giant diff.

4. **Write a plan for Layer B** (same rigor as step 2), covering: the `determine_negative_ns`/
   `base_change`/`base_n_base_change` port, the `split`/`rsplit` command handler and its wiring
   into `wr-cli`'s real dispatch (find `Prover.java`'s `split`/`rsplit` regex dispatch and mirror
   whatever pattern already-ported commands use — do not build a new ad hoc hook), and confirmation
   or correction of the "pure composition, no new algorithmic risk" claim above.

5. **Execute Layer B** through the same full review loop (same split-context mechanics as step 3),
   and commit once green.

6. **Merge gate for both layers**: `cargo test --workspace` green, `cargo fmt --all`/`cargo clippy
   --workspace --all-targets` clean, golden corpus re-run (`cargo test -p wr-golden --release --
   --ignored --nocapture`) showing the 83 fixtures (68 + 15) now compared and passing (or, if a
   fixture genuinely can't pass, documented in `tests/golden/STATUS.md`'s `KNOWN_DIVERGENCES` with
   real justification, the same honesty standard every prior unit held to — not silently excluded),
   and a differential-testing spot check against real `walnut-java` on freshly generated
   negative-base/`split`/`rsplit` queries. Zero tests deleted.

7. **If you find a genuine Walnut (Java) bug** while reading/porting — not a quirk, an actual
   wrong-output or crash-on-plausible-input defect — log it in `docs/WALNUT-BUGS.md` per
   `CLAUDE.md`'s rule and port it verbatim; do not silently fix or silently replicate it without
   logging.

8. **Commit and push, in both repos, at each safe checkpoint** — after Layer A lands, again after
   Layer B lands, and separately for any `walnut-java` Phase-0 coverage work (mirroring how the
   Ostrowski unit split its `walnut-java` coverage commit from its `walnut-rs` port commit). Use
   explicit pathspecs, not `git add -A` (`CLAUDE.md`'s fleet-hygiene rule applies even solo — it's
   good hygiene generally). Write commit messages a human catching up cold tomorrow morning can
   follow without more context than the message itself gives them.

9. **Update the tracking docs the way CAS and Ostrowski's units did**: mark this dispatch doc's own
   status line DONE with a summary (mirroring the pattern already at the top of
   `docs/CAS-EXPORT-DISPATCH.md`/`docs/OSTROWSKI-DISPATCH.md`), and update
   `docs/UNPORTED-SCOPE-SIZING.md`'s item 3 the same way item 1 was updated when CAS landed.

### If you get stuck

If Layer A turns out fine but Layer B reveals a real design problem, or vice versa, land and commit
whichever layer is genuinely done and green, and write up exactly what's blocking the other one —
partial, honest progress the user can pick up from is much more valuable than either forcing a
merge that isn't ready or silently doing nothing. Do not delete a test to make the suite green, and
do not weaken a property test's assertion to dodge a real finding — if a property test you write
turns up a genuine bug in your own new code, that is the process working, not a problem to route
around.
