# Tier-1 golden corpus — status

Snapshot of the last full run of `tests/golden`. Update it whenever the numbers move; the
harness itself asserts the *failing set* is exactly `KNOWN_DIVERGENCES` in
`tests/golden_corpus.rs`, so this file and that table must stay in step.

The gate matches an entry by **id AND kind**, not by id alone: every reason a fixture failed
with is tagged with the comparison it came from (`FailedHalf::{Text, Automaton, Harness}`),
and a `KNOWN_DIVERGENCES` entry is honored only when *every* reason is a `Text` one. That
turns "all of these are text-only, their automata already match" from a claim this file makes
into an invariant the harness enforces on every run — a regression that broke fixture 660's
*automaton* is reported as `NOT TEXT-ONLY`, where an id-only match would have waved it through.
`only_a_text_only_failure_can_be_excused_by_a_known_divergence` (a fast-tier test, no corpus
run needed) covers the rule itself.

**The tagging was initially applied incompletely, and that is now fixed.** Five comparison
sites tagged their failure `Text` while returning *before* the automaton comparison ran, which
is exactly the "did not run ≠ passed" case the tagging exists to catch — a listed fixture that
started erroring outright, or whose `.gv`/matrix output diverged, would have been reported as
"text-only" with zero automaton evidence behind the claim. Two of the five were live (the
`compare_error` branch for "the corpus records success but the port errored", and
`compare_test_case`'s mirror image "the port succeeded but the corpus records an error"); the
other three (matrix size, matrix content, graphviz) were latent, since none of the nine listed
fixtures has a `gv*.gv` expectation and the six with CAS matrices take the DROP-scope skip.
All five are corrected: the two error-vs-success shapes are now `FailedHalf::Automaton` (no
comparison happened, and none *can* — the corpus records no automaton on an error fixture),
and the three text comparisons now collect into `failures` and fall through to step 4 the way
the `details` check already did, so the automaton comparison genuinely runs and carries its own
tag. `an_error_where_the_corpus_records_success_is_not_a_text_only_failure` and
`a_text_mismatch_no_longer_pre_empts_the_automaton_comparison` (both fast-tier) pin this;
both were mutation-verified — reverting either fix makes its test fail.

## How to run it

```bash
cargo test -p wr-golden --release -- --ignored --nocapture   # the full corpus
cargo test -p wr-golden                                      # the cheap checks only
```

The corpus test is `#[ignore]`d by default — it is `docs/DESIGN.md` §5's **gated slow tier**,
not the fast one (see "Runtime" below). Everything else in the crate (the
`assertEqualMessages` normalization tests, the classifier tests, the
`no_later_fixture_depends_on_an_unexecuted_one` self-check) runs on every `cargo test
--workspace`, in milliseconds.

The corpus lives in the sibling `walnut-java` oracle repo and is **not** vendored here; set
`WALNUT_JAVA_DIR` if the checkout is not beside this one. A run with no corpus fails loudly
rather than passing silently.

## Headline numbers (measured 2026-08-19, release build)

| | |
|---|---|
| fixtures replayed | **675** (the whole `IntegrationTest.L` list, in order) |
| compared | **586** |
| **pass** | **585** (99.8% of compared) |
| **fail** | **1** (in `KNOWN_DIVERGENCES`, a distinct root cause from the five closed below) |
| skipped (excluded, each with a recorded reason) | **89** |
| timed out / not-run | **0** |

**375, 376, 377, 378 and 379 closed (2026-08-19), unrelated to any change in what the port
computes or logs.** These five were the "warm-vs-cold `PredicateEnv::number_system`
session-cache mismatch" bucket described below in "root cause 1b" — real text this harness's
fresh-`Prover`-per-fixture design produces that Java's own already-warm fixture-generation
session never showed. The fix identifies EXACTLY which lines a fixture's own cold
`NumberSystem` construction produced, via a new harness-only side-channel tap,
`wr_core::logging::Logging::construction_recordings` (no Java analogue, bracketing only
`PredicateEnv::number_system`'s memoized-lookup call site in `wr-cli/src/session.rs` — see
that module's docs), and removes exactly that verbatim text before comparing
(`tests/golden/tests/support::strip_construction_recordings`), the same way `PathRewrite`
already removes exactly the harness's own temp-directory strings.

**The nuance, found by two independent adversarial reviews before this landed**: a recorded
span is copied from the very call that wrote it, so it always matches ITSELF — it does NOT
provide independent verification that construction's own logged text is correct (a bug that
changed what construction logs would be stripped just as cleanly as correct text). What it
DOES still guarantee: `fresh_number_system` (the UNMEMOIZED `$name(…)`/`Function`-token
sibling — 379's own `$fibmr(…)` calls, which genuinely reconstruct on every call in real Java
too) is never recorded, so its logging stays fully compared, byte-for-byte; and a
QUERY-COMPUTATION `apply_all_representations` call is never inside a recorded span either,
so a real bug in that text is still fully caught. Construction's own correctness is
`wr-core`'s responsibility, closed by a new pinned-line-sequence regression test in
`numsys.rs` (`a_cold_msd_fib_construction_logs_exactly_these_seven_lines`) rather than by
this comparator. Two more real, adversarial-review-found gaps were also fixed before this
landed: the burst match is now anchored to line starts (a plain substring search could match
mid-line against a differently-indented, textually unrelated line) and a construction that
logs some lines and then fails is discarded rather than filed (it is not memoized, so — like
`fresh_number_system` — it would re-log real signal on every later retry).

Regression tests for the mechanism itself live in `wr_core::logging`'s and
`wr-cli/src/session.rs`'s test modules (mutation-verified: deliberately recording
`fresh_number_system`'s construction too was confirmed, by reverting the fix, to break
fixture 379; dropping the indent prefix from a recorded line, filing a failed construction,
and reverting the line-anchoring were all confirmed, the same way, to break their respective
tests) and in `tests/golden/tests/support`'s `strip_construction_recordings` unit tests.

**383 remains open — a genuinely different root cause, not touched by the above fix.** See
"root cause 1b" below for why: it is WB-039's `disablePrint`/`enablePrint` leak during
recursive constant construction, captured from Java's WARM session, which is a harness
limitation this fix does not (and should not) address.

Previous snapshot: 586 compared / 577 pass / 9 fail / 89 skipped. U28 (2026-08-17 through
2026-08-19) closed two of the nine original `details` log-text divergences outright (628,
637) and turned the remaining seven (375-379, 383) into ONE clearly-understood,
honestly-documented root cause instead of the vaguer original nine — see "root cause 1" below
for the closed pair and "root cause 1b" for the seven (now five closed, as described above,
and one — 383 — still open). This took three rounds to land
correctly, and the swings between rounds are worth recording plainly rather than smoothing
over:

* **Round 1** threaded `&mut Logging` through every `act()` body and `wr-core`'s
  product/determinize/minimize/quantify/numsys call sites, closing six fixtures (375-378,
  628, 637) and misdiagnosing the remaining three (379, 383, 660) as one shared
  harness-cold-cache limitation.
* **Round 2** (a two-independent-adversarial-reviewer pass, both live-jar-verified) found
  that diagnosis was only really true for 383. It found several genuinely unported
  `wr-core` logging call sites (`reverse`, `remove_leading_zeros`, `cross_product`'s own
  missing log line, a missing `indent`/`dedent` bracket, an over-firing `Trimmed to:`
  line, a missing `disablePrint`/`enablePrint` bracket in `NumberSystem`'s constructor) —
  fixing these closed 660 outright and, combined with a corrected understanding of
  `PredicateEnv`'s two differently-cacheable number-system lookups (`number_system`,
  memoized; `fresh_number_system`, deliberately not), closed 375-379 too, by giving
  `number_system`'s three call sites a throwaway `Logging` so a genuinely fresh per-fixture
  session wouldn't show `msd_fib`'s construction-time detail that Java's own already-warm
  fixture-generation session doesn't show either.
* **A second review pass on that fix** (same two reviewers, verifying the round-2 changes
  specifically) found the round-2 fix itself was the wrong call: the throwaway made a
  GENUINELY FRESH real session (the normal case for a real user's first `eval` in a new
  Walnut session) log LESS than real Walnut actually would, purely to make this harness's
  specific cold-start artifact match a captured-mid-warm-session fixture. That is backwards
  from this project's Prime Directive (fidelity to real Walnut, not to one test harness's
  quirk). Reverted — `number_system` threads the caller's real `Logging` again, matching
  what a truly cold Walnut session (real or ported) actually logs, and 375-379 are back in
  `KNOWN_DIVERGENCES` alongside 383, all under the same honestly-stated root cause. The
  round-2 fixes that were genuine improvements (`reverse`/`remove_leading_zeros`/
  `cross_product`'s logging, the `Trimmed to:` gating, the `NumberSystem` constructor
  bracket) all stayed; only the `number_system`-throwaway experiment was undone.

**580/586 is a smaller passing count than round 2's 585/586 — that is the correct, honest
number, not a regression to be explained away.** The extra five "passes" round 2 reported
were bought by making production code quieter than real Walnut on a path real users can
actually exercise, in order to match one test harness's cold-vs-warm-session artifact.

Skip breakdown:

| count | reason |
|---|---|
| 68 | `skip-drop-scope[negative_base_number_system]` |
| 16 | `skip-drop-scope[drop_command]` (`split`/`rsplit`/`ost`) |
| 4 | `skip-otf-deferred[CCL / CCLS / BRZ_CCL / BRZ_CCLS]` |
| 1 | `skip-transitive-drop-dep[test444,test445,test446,test447]` (fixture 448) |

Partial comparisons (compared, but not in full — recorded per id in the run report):

* fixtures **638-641** — `not executed`. They ask for a deferred OTF strategy on the same
  1,790-state query as 637, which without a working `[strategy …]` would blow the per-fixture
  budget for no information; `no_later_fixture_depends_on_an_unexecuted_one` proves nothing
  downstream reads their output. (637, 659 and 660 ARE executed and compared now.)

## CAS incidence-matrix export — **CLOSED (2026-08-19)**

Fixtures **374, 375, 376, 377, 378, 379, 383** used to be `cas-matrix-skipped`: their
recorded expectation includes CAS incidence matrices (`.mpl`/`.m`/`.wl`/`.sage`), and the
CAS writer was confirmed DROP scope, so only automaton/details/error were compared and
the matrix files were explicitly excluded. CAS export is no longer DROP scope
(`.claude/plans/amber-transcribing-ledger.md`, `crates/wr-io/src/matrix_writer.rs`) — the
skip branch and `Expected::has_cas_matrices` are gone, and all 7 fixtures now get their
matrix output compared exactly like every other text field (`compare_messages`, trimmed).
All 7 pass on all four extensions (28 comparisons); 383 still fails, but only on its
pre-existing `details` divergence (§1b below).

**A real gate-laundering hole here, found independently by two adversarial reviewers, not
by this unit's own first-pass verification.** 383 is both a matrix fixture and the sole
surviving `KNOWN_DIVERGENCES` entry, whose gate condition (before this fix) was "every
failure reason is `FailedHalf::Text`" with no regard for WHICH text field diverged — so a
genuine matrix regression on 383 specifically would have been silently excused by the
entry's `details`-only justification, both being tagged `Text`. The first-pass mutation
check (corrupting `MapleEmitter::begin` and observing 383 report both reasons together)
did not actually exercise this: it proved the OTHER six fixtures (not in
`KNOWN_DIVERGENCES`) fail loudly, which they always would have; it said nothing about
whether the gate itself would still pass 383. Statically confirmed the gate would have
excused 383 alone.

**Fixed by giving `FailedHalf::Text` a `TextField` discriminant** (`Details`/`Matrix`/
`Graphviz`/`Error`) and scoping each `KNOWN_DIVERGENCES` entry to the field(s) it actually
declares (`Verdict::is_excused_by`, `golden_corpus.rs`) — 383's entry now declares
`&[TextField::Details]` only. Mutation-verified correctly this time, with the corruption
still in place while checking the GATE's verdict specifically: corrupting
`MapleEmitter::begin`'s output makes the full corpus run **FAIL** the gate, reporting
fixture 383 as `UNDECLARED TEXT FIELD` (a real, new divergence, not the documented one) —
not silently passed. Reverting the corruption restores a clean, green run with 383's
matrix comparison passing on its own and only its documented `details` divergence
remaining. Two new unit tests pin the field-scoping directly:
`a_known_divergence_only_excuses_its_own_declared_text_field` (hand-built verdicts) and
the existing `only_a_text_only_failure_can_be_excused_by_a_known_divergence` (updated for
the `TextField` parameter).

## The 1 remaining divergence (5 of the former 6 closed 2026-08-19)

### 1. `details` log-text fidelity — **CLOSED (U28, 2026-08-17)** for 2 of the original 9

**Was never a decision-procedure defect.** Every *state count* in these fixtures already
matched real Walnut exactly, pre-minimization ones included — only log LINES were missing:
the `computing X` / `computed X` pairs Java emits from inside each token's `act()` body
(`RelationalOperator.java:96-97,175-176`, `LogicalOperator.java:78-79,…`, `Word.java:54,78`,
`Function.java:64,97`), and every `wr-core`-level line (`computing cross product:N states`,
`Minimizing: N states.`, `Determinizing [#k, strategy: SC]`, `quantifying:N states`,
`fixing leading zeros:N states`).

Closed by threading `&mut Logging` through every `Token::act`/`Operator::act`/`Word::act`/
`Function::act` body (`wr-logic`) and through `wr-core`'s `product`/`determinize`/`minimize`/
`quantify`/`numsys` call sites — see `crates/wr-logic/src/eval.rs`'s module docs ("Per-`act()`
`Logging` calls — CLOSED (U28)") for the full account. Along the way this surfaced a real,
verified instance of WB-039 (`docs/WALNUT-BUGS.md`) — a `NumberSystem`-internal
`disablePrint`/`enablePrint` non-nesting leak — and corrected an earlier misunderstanding of
what `disablePrint`/`enablePrint` actually gate (they DO gate `logMessage`-based internal
construction logging, not just console output, contrary to an initial reading of
`Logging.java`).

**628 and 637 now pass cleanly** (both automaton and `details` text). 637 in particular is
worth calling out because its earlier `[strategy 6 BRZ]eval test637
"E x,y,z (n=x+y+z)&(QQ[x]=@1)&(QQ[y]=@1)&(QQ[z]=@1)"::` failure was never a metacommand
failure — its seventh determinization is a 1,790-state NFA that only finishes inside the 60s
cap once `[strategy 6 BRZ]` actually takes effect (matching real Walnut's 130ms); the `details`
gap was purely the missing `Determinizing [#6, strategy: Brzozowski]` log line, now emitted.
**660 also closed** (see root cause 1b — it turned out to be a different, closeable bug that
had nothing to do with the shared root cause below).

(The other two `details` fixtures whose text mentions a file path, 656 and 668 —
`describe GG;` and `describe $diffbyone;` — DO pass. Their only divergence was the absolute
library directory, which is the harness's own temp path rather than the
`src/test/resources/integrationTests/…` one Java recorded; see `PathRewrite` for the exact,
narrow substitution that reconciles it and why it cannot hide a port defect.)

### 1b. Warm-vs-cold `NumberSystem` cache — CLOSED for 375-379 (2026-08-19); 383 remains open

375-379, 383 and 660 were all originally (mis)diagnosed as one shared "warm-vs-cold
`NumberSystem` cache" limitation. Two further adversarial-review rounds (both live-jar-
verified, different models from the author and from each other) corrected that diagnosis
twice over — first narrowing it, then widening it back — and the final state is genuinely
different from either intermediate one:

* **660** was never a cache issue at all — `wr_core::product::cross_product` was simply
  missing Java's own `computing cross product:` line (`ProductStrategies.crossProduct`'s
  `printAndUpdateIndex`, a call site separate from `crossProductAndMinimize`'s own, which
  this port already had, plus a log-before-validate ordering bug in the first attempt at
  this exact fix, caught by a THIRD review pass). Fixed. **660 passes cleanly and stays
  that way** — this is the one fixture from the original nine that a plain logging fix,
  not a cache-warmth accounting, actually closes.
* **375-379 and 383 are genuinely one root cause: `PredicateEnv::number_system` is a
  MEMOIZED lookup** (`?ns`-directive-driven — only actually builds a `NumberSystem` the
  first time a name is referenced in a session) whose construction-time logging (custom
  bases like `msd_fib` with their own all-representations file trigger real,
  correctly-threaded `apply_all_representations` logging — see
  `wr_core::numsys::NumberSystem::with_custom_base_files`'s docs) genuinely depends on
  Java's session-wide cache warmth. Real Java's fixture-generation run is ONE long,
  continuous session; by the time fixtures 375+ ran, `msd_fib` (and the constants 383
  needs) were already warm from earlier fixtures in that SAME session. This harness
  dispatches every fixture through a fresh `Prover` (deliberate, for
  isolation/timeout-safety — see "Runtime" below), so it is always cold where Java's
  original session was warm.

  **A throwaway `Logging` at `number_system`'s three call sites was tried (2026-08-17-19)
  specifically to make this harness's cold fixtures match Java's warm captures, closing
  375-379 and appearing to leave only 383 — and then reverted after a further
  adversarial-review pass**, because it traded harness cosmetics for real-world
  correctness: it made a genuinely fresh, single-query Walnut session (the normal case for
  a real user's very first `eval` in a new session — real or ported, equally cold) log
  LESS than real Walnut actually would. `PredicateEnv::fresh_number_system` (the
  UNMEMOIZED sibling lookup `$name(…)`/`Function` tokens use — Java's own
  `new NumberSystem(name)` in `Function`'s constructor, reconstructs every time regardless
  of session state) is unaffected either way and stays correctly, unconditionally
  threaded. See `wr_logic::predicate::Predicate::tokenize_and_compute_post_order`'s docs
  for the full account of the back-and-forth.

**Fixing 375-379/383 by sharing `NumberSystem`-cache state across fixtures** — matching
whatever order Java's original session ran them in — was considered and rejected: it would
be a genuine harness redesign (threading shared, mutable cross-fixture state through the
existing per-fixture timeout/halt-on-timeout machinery this file's "Runtime" section
documents at length, specifically to AVOID exactly this kind of shared mutable state), with
its own real risks, for a problem that turned out to have a much narrower fix available.

**375-379 CLOSED (2026-08-19), by a fix that changes nothing about what is logged.** The
insight: `PredicateEnv::number_system`'s cold-construction text isn't merely SIMILAR to noise
that could be pattern-matched away — the production code already knows, precisely, which
lines it just emitted from inside that one call. `wr_core::logging::Logging` gained a small
side-channel recorder with no Java analogue
(`begin_construction_recording`/`end_construction_recording`/`construction_recordings` — see
that module's docs), bracketing ONLY `PredicateEnv::number_system`'s own call into
`load_number_system` (`crates/wr-cli/src/session.rs`) — deliberately not the shared
`load_number_system` helper itself, since `fresh_number_system` also calls that helper and
its construction is NOT a cache artifact (it reconstructs on every call, in real Java too, so
its logging is real signal, not noise — see the mutation-verified regression test
`only_the_memoized_lookups_first_cold_call_is_recorded` in `session.rs`, which pins exactly
this distinction after an initial, broader version of the bracket briefly broke fixture 379).
The golden-corpus comparator then removes exactly that verbatim, exact-match text before
diffing (`support::strip_construction_recordings`) — the same shape of fix as `PathRewrite`,
just driven by what the production code reports it did rather than by a fixed set of known
strings. It cannot mask a real defect: a span that does not appear verbatim in the actual
output (e.g. a future construction-time regression) is simply left in place, and the ordinary
text diff reports whatever is genuinely different. **375-379 now pass cleanly, both automaton
and text.**

**383 remains open — it is a DIFFERENT root cause from 375-379**, not merely the hardest
member of the same bucket, so the fix above does not touch it: it is WB-039's
`disablePrint`/`enablePrint` leak during RECURSIVE constant construction inside
`get_constant`, not `NumberSystem`'s own top-level construction. This port's gating there is
independently verified correct (byte-for-byte against a COLD-session real-jar capture of the
same query, leak included), but 383's originally recorded fixture text was captured from
Java's WARM session — the same class of harness limitation as 375-379 was, but arising from a
different code path with no equivalent "the production code already knows exactly what it
emitted from one bracketed call" structure to hook a recorder onto. Fixing it for real would
still require the cross-fixture cache-sharing redesign described above. **Every automaton
still matches**; only the log text differs, and the divergence is well-understood and pinned,
not silently skipped.

### 2. `transduce` over a reversed (lsd) custom-base word automaton — **CLOSED (2026-08-16)**

Fixtures 532, 533 and 534 used to fail here. `transduce test532 RUNSUM2 test531;` (where
`test531 = reverse test531 F;` and `F` is the Fibonacci word over `msd_fib`) was the one
`transduce` fixture whose input is an **`lsd_fib`** DFAO, i.e. the one taking
`Transducer.transduceNonDeterministic`'s reverse-input/reverse-result branch, and it was the
one that failed; 533 and 534 are downstream of it.

The root cause was not in `Transducer` at all — this file's earlier suggestion to bisect
`word_automaton::reverse_with_output`'s number-system flip was right about the location, and
the bug was one level down in it. `wr_core::logicalops::flip_ns` flipped the per-track
`Automaton::msd` direction flag but left the recorded `Automaton::ns_name` untouched, where
Java's `NumberSystem.flipNS` replaces the whole `NumberSystem` object and so changes its
`getName()` too. Since `Automaton::track_ns_names` deliberately prefers the recorded name
(a custom base's name is not derivable from its alphabet), everything downstream of a
`reverse` saw `msd_fib` on an automaton that was really `lsd_fib`.

The same defect had two other live symptoms outside the corpus, both confirmed against the
real jar: `reverse rv $ok;` wrote an `msd_2` header where Walnut writes `lsd_2`, and
`union mixed rv two;` — which real Walnut refuses with `Automata must have the same number
system(s).` — silently succeeded, writing a union of an lsd automaton with an msd one.
Fixed in `flip_ns`; see its docs and
`logicalops::tests::flip_ns_flips_the_recorded_number_system_name_not_just_the_direction`.

## What U27 fixed along the way

Three genuine port bugs, each found by this harness and each with its own regression test:

1. **`$name(…)` could not use a custom base** (`crates/wr-logic/src/predicate.rs`,
   `predicate_env.rs`, `crates/wr-cli/src/session.rs`). `Predicate.putFunction` built the
   token's number system with `wr_core::numsys::NumberSystem::new`, which by design does no
   file I/O — so every `$`-call under `?msd_fib` failed with "Number system msd_fib is not
   defined.", including 15 of the corpus prelude's own definitions. Now routed through a new
   `PredicateEnv::fresh_number_system`, matching Java's `new NumberSystem(name)` (which reads
   `Custom Bases/` through `Session`). Test:
   `eval_def::tests::a_function_token_resolves_a_custom_base_number_system`.
2. **A custom-base library file lost its valid-representation restriction**
   (`crates/wr-io/src/reader.rs`). The reader left `Automaton::all_reps` empty even for an
   `msd_fib` header, so `Automaton::apply_all_representations` — which `~`, `=>` and `A` all
   run — silently did nothing, and complementing an `msd_fib` automaton admitted words with a
   `11` substring, i.e. non-Zeckendorf strings. 12 fixtures (352-371) diverged. Verified fixed
   by byte-comparing six probe predicates against the real `walnut-java` CLI. Test:
   `reader::tests::a_custom_base_header_carries_its_valid_representation_restriction`.
3. **`convert` read the source base from the alphabet instead of the name**
   (`crates/wr-core/src/logicalops.rs`). Java's `NumberSystem.parseBase()` parses the name;
   this port used the track's alphabet size, which for `msd_fib` is 2 — so
   `convert x msd_2 FTM;` reported "New and old number systems are identical: msd_2" instead
   of "Base of automaton's number system must be > 1 and int, found: fib" (fixture 554), and
   `convert x lsd_2 FTM;` would have *succeeded*, reversing a Zeckendorf automaton as if it
   were binary. Tests:
   `logicalops::tests::convert_ns_parses_the_base_from_the_name_not_the_alphabet_size`,
   `…::parse_base_matches_javas_pattern_number_and_greater_than_one_guard`.

Plus one behavioral fix that is a fidelity restoration rather than a wrong answer:

4. **A `wr-core` guard panic inside `act()` killed the process** (`crates/wr-core/src/product.rs`,
   `crates/wr-core/src/walnut_panic.rs`, `crates/wr-logic/src/eval.rs`). Java's
   `EvalDef.compute` catches *any* `RuntimeException` a token's `act()` throws and rethrows it
   with the token's position appended; this port had no such boundary, so
   `eval test190 "Ez,x,y $func(z,x,y,17)"` aborted instead of printing
   `in computing cross product … must have the same alphabet\n\t: char at 8`. Test:
   `eval_def::tests::a_wr_core_guard_panic_inside_act_becomes_a_positioned_error`.

**No new `docs/WALNUT-BUGS.md` entry was needed** — every divergence this unit found was a
defect in the *port*, not in Walnut. (The highest existing entry remains WB-037.)

## Runtime

| | release | debug |
|---|---|---|
| prelude (19 commands) | 13 s | 120 s (measured) |
| 675 fixtures | 35 s | ~6 min (extrapolated at the measured 10× ratio) |

Slowest single fixture, release: well under the `MAX_FIXTURE_SECS` = 60 s cap; nothing was
recorded `TIMEOUT` or `not-run`. The 10× debug/release ratio is why the corpus test is
`#[ignore]`d: at ~7 minutes it does not belong in the every-commit fast tier
(`docs/DESIGN.md` §5, "Two test tiers").

That cap is **enforced, not merely measured**: each fixture (and each prelude command) runs on
a worker thread that the harness waits on with a timeout, so a port regression that turns one
fixture superexponential is recorded `TIMEOUT` instead of wedging `cargo test` indefinitely
(`CLAUDE.md`, "per-test resource caps, never hangs").
`the_per_fixture_cap_is_enforced_rather_than_measured_afterwards` (fast tier) pins the
enforcement itself.

**A timeout halts the run.** The known tradeoff of the cap is that a timed-out worker is
*abandoned* rather than killed — Rust has no thread-kill primitive — so it holds a `Prover`
over the same on-disk session tree that every later fixture builds a fresh `Prover` against,
and may keep writing into it for the rest of the run. A `PASS`/`FAIL` computed while that is
happening is not evidence about the port and is indistinguishable, in the report, from one
computed cleanly. So the harness stops attempting fixtures at the first timeout — a prelude
command's, a fixture's, or one run only for its session state — and records every remaining
fixture `NOT-RUN` with the reason, naming the fixture that poisoned the tree. The
`MAX_TOTAL_SECS` budget uses the same mechanism. Both `TIMEOUT` and `NOT-RUN` fail the gate,
so this can never make a broken run look green; what it buys is that the report says "one
timeout, N never attempted" instead of "one timeout, N verdicts of unknown worth".
`a_timeout_halts_the_run_and_later_fixtures_are_not_run` (fast tier) pins the policy.

The one fixture that would not finish under plain subset construction, 637, now runs in well
under a second because its `[strategy 6 BRZ]` metacommand is wired through to
`wr_core::determinize` — exactly as it is in real Walnut. Its four OTF siblings (638-641) name
strategies this port deliberately does not implement, so they would fall back to `SC` on the
same 1,790-state NFA; they stay unexecuted (`skips_execution`).
