# Tier-1 golden corpus — status

Snapshot of the last full run of `tests/golden`. Update it whenever the numbers move; the
harness itself asserts the *failing set* is exactly `KNOWN_DIVERGENCES` in
`tests/golden_corpus.rs`, so this file and that table must stay in step.

The gate matches an entry by **id AND kind**, not by id alone: every reason a fixture failed
with is tagged with the comparison it came from (`FailedHalf::{Text, Automaton, Harness}`),
and a `KNOWN_DIVERGENCES` entry is honored only when *every* reason is a `Text` one. That
turns "all nine of these are text-only, their automata already match" from a claim this file
makes into an invariant the harness enforces on every run — a regression that broke fixture
637's or 660's *automaton* is reported as `NOT TEXT-ONLY`, where an id-only match would have
waved it through. `only_a_text_only_failure_can_be_excused_by_a_known_divergence` (a fast-tier
test, no corpus run needed) covers the rule itself.

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

## Headline numbers (measured 2026-08-16, release build)

| | |
|---|---|
| fixtures replayed | **675** (the whole `IntegrationTest.L` list, in order) |
| compared | **586** |
| **pass** | **577** (98.5% of compared) |
| **fail** | **9** (all in `KNOWN_DIVERGENCES`, one root cause — below) |
| skipped (excluded, each with a recorded reason) | **89** |
| timed out / not-run | **0** |

Previous snapshot: 583 compared / 576 pass / 7 fail / 92 skipped. The three extra compared
fixtures are 637, 659 and 660, which used to be excluded as `skip-metacommand-not-wired`;
wiring `[strategy …]`/`[export …]` through to `wr_core::determinize` made them comparable.
659 (an `error` fixture — a metacommand on a `;` command) now **passes**; 637 and 660 land in
the pre-existing `details` log-text class below, their automata compared and matching.

Skip breakdown:

| count | reason |
|---|---|
| 68 | `skip-drop-scope[negative_base_number_system]` |
| 16 | `skip-drop-scope[drop_command]` (`split`/`rsplit`/`ost`) |
| 4 | `skip-otf-deferred[CCL / CCLS / BRZ_CCL / BRZ_CCLS]` |
| 1 | `skip-transitive-drop-dep[test444,test445,test446,test447]` (fixture 448) |

Partial comparisons (compared, but not in full — recorded per id in the run report):

* fixtures **374, 375, 376, 377, 378, 379, 383** — `cas-matrix-skipped`. Their recorded
  expectation includes CAS incidence matrices (`.mpl`/`.m`/`.wl`/`.sage`); the CAS writer is
  confirmed DROP scope for this port, so automaton/details/error are compared and only the
  matrix files are not.
* fixtures **638-641** — `not executed`. They ask for a deferred OTF strategy on the same
  1,790-state query as 637, which without a working `[strategy …]` would blow the per-fixture
  budget for no information; `no_later_fixture_depends_on_an_unexecuted_one` proves nothing
  downstream reads their output. (637, 659 and 660 ARE executed and compared now.)

## The 9 remaining divergences — one root cause

### 1. `details` log-text fidelity (9 fixtures: 375, 376, 377, 378, 379, 383, 628, 637, 660)

**Not a decision-procedure defect.** Every *state count* in these fixtures already matches
real Walnut exactly, pre-minimization ones included — the recorded
`j<i:6 states`, `…:51 states`, `…:137 states`, `…:23 states`, `…:25 states` chain of
`details375.txt` is reproduced verbatim. What is missing is log LINES:

* the `computing X` / `computed X` pairs, which in Java come from inside each token's
  `act()` body (`RelationalOperator.java:96-97,175-176`, `LogicalOperator.java:78-79,…`,
  `Word.java:54,78`);
* every `wr-core`-level line — `computing cross product:N states`, `Minimizing: N states.`,
  `Determinizing [#k, strategy: SC]`, `quantifying:N states`, `fixing leading zeros:N states`.

Both are **pre-existing, documented deferrals**, not something this unit introduced:
`crates/wr-logic/src/eval.rs`'s module docs ("DEFERRED GAP: the per-`act()` `Logging` calls
are NOT ported") and `crates/wr-core/src/product.rs`'s "Progress logging not ported" note.
`eval.rs` already says the gap "**must land before Phase 3b's U27**" — it did not, and this is
the honest accounting of what that costs: 9 of 586 compared fixtures.

**637 and 660 joined this class when the metacommands were wired**, and are worth calling out
because their failure is emphatically NOT a metacommand failure:

* 637 is `[strategy 6 BRZ]eval test637 "E x,y,z (n=x+y+z)&(QQ[x]=@1)&(QQ[y]=@1)&(QQ[z]=@1)"::`.
  Its seventh determinization (index 6) is a 1,790-state NFA whose subset construction does not
  finish inside the 60s per-fixture cap; with the metacommand wired the whole fixture completes
  in well under a second, matching real Walnut's 130ms. That is itself the proof the index
  arithmetic is right — target index 5 or 7 instead and the fixture times out. Its `details`
  expectation additionally spells `Determinizing [#6, strategy: Brzozowski]`, a log LINE this
  port does not emit yet: the same gap as the seven above.
* 660 is `[export 1 BA]eval test660 "TH[a+c] + TH[b-d] = 0"::`. The export is a side-effecting
  file write (`<name>_1_pre.ba`) that never touches the returned automaton, so it cannot move
  this comparison either way; only the log text differs.

**Both now have their AUTOMATON compared and matching** — as do all seven older members of this
class. That is newly checkable: `compare_test_case` used to `return` on the first `details`
mismatch, so an automaton divergence could hide behind a text one indefinitely. It now collects
mismatches (`fold_failures`) and reports them together; the run report shows no `--- and ---`
separator anywhere, i.e. all nine are text-only.

(The other two `details` fixtures whose text mentions a file path, 656 and 668 —
`describe GG;` and `describe $diffbyone;` — DO pass. Their only divergence was the absolute
library directory, which is the harness's own temp path rather than the
`src/test/resources/integrationTests/…` one Java recorded; see `PathRewrite` for the exact,
narrow substitution that reconciles it and why it cannot hide a port defect.)

Closing it means threading a `&mut Logging` through every `Token::act`/`Operator::act`/
`Word::act` body **and** through `wr-core`'s `product`/`determinize`/`minimize`/`quantify`
call sites. That is a unit of engineering in its own right, touching already-reviewed
Phase-2/3a code, and is the recommended follow-up (call it U28).

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
