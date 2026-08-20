# Plan — Layer A: restore negative-base numeration in `wr-core::numsys`

Dispatch: `docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`, step 2/3. Layer B (`baseChange` /
`determineNegativeNS` / `split` / `rsplit`) is a SEPARATE plan and a separate commit; nothing
in this plan may depend on it.

## 0. What Layer A is, exactly

`NumberSystem`'s `isNeg`-gated arithmetic surface, and nothing else. Concretely: a user can
write `?msd_neg_2 x < y`, `?lsd_neg_2 Ex x + 3 = y`, `?msd_neg_fib …` in `eval`/`def`/`reg`
and get the same automaton real Walnut gives. The `baseChange` family
(`setBaseChangeAutomaton` `:443-468`, `baseNBaseChange` `:568-601`, `determineNegativeNS`
`:219-230`, the `baseChange` field, `UNDERSCORE_BASE_CHANGE_AUTOMATON`) stays absent —
`determineNegativeNS`'s own javadoc says "Currently used ONLY in split command", and Layer A
must be green and committed before that lands.

Deliberately unchanged in Layer A (verified, not assumed):
* `NumberSystem.parseBase()` (`:237-243`) keeps Java's positive-only guard and its `TODO`
  verbatim. It throws for `msd_neg_2` in Java too. Rust call-site audit: `parse_base_of` /
  `NumberSystem::parse_base` have **zero** non-test callers in this workspace
  (`wr_core::logicalops`'s `convertNS` has its own private `parse_base`, already pinned by
  `parse_base_matches_javas_pattern_number_and_greater_than_one_guard`, which already asserts
  `msd_neg_2` is rejected — Java-faithful, stays).
* `crate::quantify`'s leading/trailing-zero fixups. Java's `AutomatonQuantification.quantify`
  and `AutomatonLogicalOps.removeLeadingZeros`/`fixTrailingZerosProblem` never read `isNeg`;
  they branch only on msd/lsd. Nothing to widen.
* `less_than_msd` (the Phase-1 spike helper) — positive-base only, and only used by its own
  cross-check test against `lexicographic_less_than`.

## 1. The undo-list, method by method

Source of truth: the real Java at
`/Users/nkohen/dev/walnut-java/src/main/java/Automata/NumberSystem.java` (read directly, not
from the module doc's summary). Mechanical port first per `CLAUDE.md` — quirks preserved,
idiomatic cleanup deferred to a separate commit if ever.

| # | Java | Rust target |
|---|------|-------------|
| A1 | `UtilityMethods.parseNegNumber` (`UtilityMethods.java:47-55`) | new `crate::util::parse_neg_number` beside the existing `is_number` |
| A2 | `isNeg` field (`:99`), assigned `name.contains("_neg_")` (`:137`) | new `NumberSystem::is_neg` field; `with_custom_base_files` stops returning `UnsupportedNegativeBase` |
| A3 | `baseNegNAddition` (`:503-533`) | new free fn `base_neg_n_addition(n, is_msd)` |
| A4 | `baseNegNLessThan` (`:541-561`) | new free fn `base_neg_n_less_than(n, is_msd)` |
| A5 | `setAdditionAutomaton`'s `else if (parseNegNumber(base) > 1)` (`:327-328`) | restored in `set_addition_automaton` |
| A6 | `setLessThanAutomaton`'s `if (parseNegNumber(base) > 1)` (`:372-373`) | restored in `set_less_than_automaton` (needs a new `base: &str` parameter — today it only takes the alphabet) |
| A7 | `validateNeg` (`:1026-1028`) | `validate_non_negative` becomes `validate_neg`: `if !self.is_neg && *n < 0 { Err(NegativeConstant) }` |
| A8 | `comparison(String, BigInteger, Ops)`'s `b.signum() < 0` arm (`:700-702`) | `comparison_const_b` |
| A9 | `arithmetic(String, BigInteger, String, Ops)`'s (`:809-813`) | `arithmetic_const_b` |
| A10 | `arithmetic(BigInteger, String, String, Ops)`'s (`:861-864`) | `arithmetic_const_a` |
| A11 | `arithmetic(String, String, BigInteger, Ops)`'s (`:910-913`) | `arithmetic_const_c` |
| A12 | `constant`'s `n.signum() < 0` arm (`:944-951`) | `constant` |
| A13 | `multiplication`'s `n.signum() < 0` arm (`:986-994`) | `multiplication` |
| A14 | `division`'s two `n.signum() < 0` operand selections (`:1046-1048`) | `division` |

### Exact shapes that are easy to get wrong (call these out in the diff)

* **A5/A6 order.** Java's adder is `if (isNumber(base) && parseInt(base) > 1) baseN…
  else if (parseNegNumber(base) > 1) baseNegN… else throw`. Java's comparator is
  `if (parseNegNumber(base) > 1) baseNegNLessThan… else lexicographicLessThan(getAlphabet())`
  — note it does **not** consult `isNumber` and has no "not defined" throw at all (an unknown
  base has already thrown inside `setAdditionAutomaton`). Port both literally.
* **A5/A6 reverse placement.** `if (!isMsd) reverse(…)` sits INSIDE the "no file found"
  branch in both setters and applies to the negative-base construction too. `lsd_neg_2` is
  therefore `reverse(base_neg_n_addition(2))`. Do not hoist.
* **A3's state 1.** `baseNegNAddition` has **three** states with outputs `[1, 0, 0]`, and the
  only edge into state 1 is `(0,0,n-1)` from state 1 itself: `if (i == 0 && j == 0 && k == n
  - 1) addNewTransition(1, 2, l)` — i.e. state 1 has exactly ONE outgoing transition, to
  state 2. State 1 is reachable only as the initial-carry state via `O = [1,0,0]`… no: state
  1 is not reachable at all from state 0. Port it verbatim including the unreachable-looking
  structure; `Trimmer` is not applied here in Java either.
* **A4's `l` counter.** `baseNegNLessThan` runs `i` fastest inside `j` (two tracks), and its
  state-0 arm uses `l` directly, not the swapped `i*size + j` spelling
  `lexicographicLessThan` uses. Ports as a plain counter.
* **A12/A13's recursion.** `constant(n)` for `n < 0` recurses into `getConstant(-n)` and
  `arithmetic(a, b, 0, PLUS)`; `multiplication(n)` for `n < 0` recurses into
  `getMultiplication(-n)` and `arithmetic(b, c, 0, PLUS)`. Both go through the memo table and
  both sit inside the existing `disable_print()`/`enable_print()` bracket — keep the bracket
  boundaries exactly where Java has them (WB-039's non-nesting leak is a ported quirk).
* **A14.** `division`'s guard flips to `n < r <= 0` for negative `n`, i.e.
  `comparison(r, 0, LESS_EQ_THAN)` and `comparison(r, n, GREATER_THAN)`. Note `comparison(r,
  n, …)` is then called with a NEGATIVE constant, which only survives `validateNeg` because
  `is_neg` is true — this is the load-bearing coupling between A7 and A14.

## 2. `NumSysError::UnsupportedNegativeBase` — deliberate removal

Its entire reason to exist is gone. Every reference (grepped, 9 production + 4 test sites):

| Site | Action |
|------|--------|
| `wr-core/src/numsys.rs:387` (variant), `:425`, `:467` (Display) | delete the variant and its `Display` arm |
| `wr-core/src/numsys.rs:1232-1234` (the constructor gate) | replaced by `let is_neg = …` |
| `wr-core/src/numsys.rs:95`, `:276`, `:1199` (docs) | rewritten to describe the restored surface |
| `wr-core/src/numsys.rs:4560-4585` (two tests pinning rejection) | **flipped, not deleted** — they become tests that the named negative base now constructs and computes the right language (`CLAUDE.md`: "a test that specifically pinned 'we reject this' is allowed to become a test that pins the new correct behavior once the feature is real") |
| `wr-io/src/reader.rs:1285` (`load_custom_base` gate) | deleted; a `_neg_` header token now resolves like any other custom base |
| `wr-io/src/reader.rs:2776-2782` (test pinning the ordering) | flipped: assert `msd_neg_2` now loads, and that a genuinely malformed `msd_neg_fib_addition.txt` produces the *parse* error (which is what the ordering test was really protecting) |
| `wr-cli/src/session.rs:655-657` (`load_number_system` gate) | deleted |
| `wr-cli/src/session.rs:1650-1662` (`a_negative_base_is_rejected_before_its_files_are_read`) | flipped to assert the files ARE now read (garbage content ⇒ a read/parse error, not `UnsupportedNegativeBase`), plus a positive case with real content |
| `wr-logic/src/eval.rs:405-407` (classifies it as "port-declared divergence, report as error") | the match arm loses that variant; `NumSysError::Quantify(_)` stays |
| `tests/golden/tests/support/mod.rs:406` (doc comment only) | doc updated |

Zero tests deleted.

## 3. Regression risk to the existing suites — audited, per the dispatch's explicit ask

The question is not "would the invariant also hold for negative bases" but "does any generator
bake positive-only into its INPUT SPACE as an assumption".

* `tests/differential-gen` — its base generator emits `2..=4` with `msd_`/`lsd_` prefixes only.
  It never *could* produce a `_neg_` name, so it is neither invalidated nor widened by this
  change. **Not widened in Layer A** (it is a soak harness with its own STATUS.md accounting;
  widening its input space is a change to a Tier-3 exit criterion and belongs in its own unit).
  A fresh-seed spot run is still part of the merge gate, to prove no *positive*-base regression.
* `fuzz/` corpora — `wr_io_reader`'s seeds contain no `_neg_` names; the target's assertion is
  "does not panic", which is unchanged in kind. Re-smoke all three targets in the merge gate.
* `wr-core/src/numsys.rs` property tests — `comparison_automata_agree_with_the_integer_order`,
  `addition_automaton_computes_real_addition`, `get_constant_accepts_exactly_that_value`,
  `multiplication_automaton_computes_real_multiplication`, `msd_and_lsd_agree_after_reversal`.
  Each generates a base and enumerates non-negative integer tuples. **These extend to negative
  bases naturally and must be extended in place, not shadowed by a parallel set** (the
  dispatch says this explicitly). The extension needs a real base-(-k) value↔word oracle
  (§4.1).
* `wr-cli`/`wr-io` tests asserting `UnsupportedNegativeBase` — §2.
* Golden corpus — 68 fixtures un-excluded (§5).

## 4. Test plan

### 4.1 The Walnut-independent oracle (Tier 4) — the load-bearing new test asset

A word `d_{m-1} … d_1 d_0` over `{0..n-1}` denotes `Σ d_i · (-n)^i`. Implement
`neg_base_value(digits_msd_first, n) -> i64` in the test module and use it to drive the SAME
property tests the positive path uses, parameterised on the base's sign:

* comparator: accepts `(x, y)` iff `value(x) < value(y)`, over ALL equal-length digit pairs
  up to length 4 for `n ∈ {2, 3}` — exhaustive, not sampled.
* adder: accepts `(x, y, z)` iff `value(x) + value(y) == value(z)`, exhaustively for `n = 2`
  up to length 4 and `n = 3` up to length 3.
* `get_constant(v)` for `v` in a range that **includes negatives** (which is the whole point):
  accepts exactly the words with `value == v`.
* `multiplication(k)`/`division(k)` for negative `k` — accepts `(x, y)` iff
  `value(y) == k·value(x)` / `value(y) == value(x) / k` with Java's truncation, cross-checked
  against the `0 <= r < n` vs `n < r <= 0` remainder convention A14 restores.

Mutation-verify at least: deleting A3's `(1,2)` edge, swapping A4's `i<j`/`j<i` arms, and
flipping A14's `n.signum() < 0` operand selection must each fail loudly. Record the matrix in
the test module's docs.

### 4.2 Tier 2 — replicate `NumberSystemTest`'s negative-base methods
Whatever Phase 0 (step 1 of the dispatch, running now) reports as existing Java coverage on
these paths gets a Rust `#[test]` twin. If Phase 0 finds the Java coverage thin, extend
`NumberSystemTest.java` **first**, in `walnut-java`, and commit that separately.

### 4.3 Tier 3 — a new differential file
`tests/differential/tests/negative_base.rs`, driven through `wr-cli`'s real `Prover::dispatch`
against freshly captured `walnut-java` output (recipe appended to
`tests/differential/CAPTURE.md`, per this repo's convention). Minimum coverage: `msd_neg_2`
comparison, `lsd_neg_2` comparison, a negative constant on each side of `+`/`-`, `*` and `/`
by a negative constant, a two-quantifier formula, and `msd_neg_fib` (the file-backed case,
which is a genuinely different construction path — file load, not programmatic).

### 4.4 Tier 1 — un-exclude the 68 fixtures
`walnut-java/phase0-artifacts/subset-filter.json`: flip every fixture whose only
`drop_reason` is `negative_base_number_system:*` (68 of them: 39 `msd_neg_2`, 14
`msd_neg_fib`, 11 `lsd_neg_fib`, 3 `lsd_neg_2`, 1 `msd_neg_10`) to `subset_relevant: true`,
and update `subset_relevant_count` 592→660, `drop_relevant_count` 83→15,
`drop_reason_counts.negative_base_number_system` 68→0 (drop the key), plus `schema_note`.
`tests/golden/tests/support/mod.rs`'s `load_fixtures` self-check constants move with them.
Cross-repo, so it is a `walnut-java` commit of its own.

Expected corpus movement: 587 compared / 586 pass → 655 compared, with 654+ passing. Any
fixture that does not pass gets a real `KNOWN_DIVERGENCES` entry with a real justification —
never a silent re-exclusion.

## 5. Order of work

1. (running) Phase-0 Java coverage report; extend `NumberSystemTest.java` if thin → commit in
   `walnut-java`.
2. `crate::util::parse_neg_number` + A3 + A4 + their unit tests (pure construction, no
   plumbing) — smallest independently-checkable slice.
3. A2 + A5 + A6 (constructor plumbing) + §2's de-gating in `wr-io`/`wr-cli`.
4. A7-A14 (the `signum` arms).
5. §4.1's oracle + property extensions; §4.2; §4.3.
6. §4.4's fixture flip (both repos) + golden run.
7. Docs: `crates/wr-core/src/numsys.rs` module doc (the undo-list section is now a
   *restored*-list), `CLAUDE.md`'s KEEP/DROP line, `docs/BOUNDARY-MAP.md` §4.1,
   `docs/DESIGN.md` §3, `docs/UNPORTED-SCOPE-SIZING.md` item 3, `tests/golden/STATUS.md`.
8. Two independent adversarial reviewers on the whole diff (split context, diff only, at least
   one on a different model than the author), fix, re-verify, commit + push.

## 6. Merge gate

`cargo test --workspace` green; `cargo fmt --all` / `cargo clippy --workspace --all-targets`
clean; `cargo doc` no new warnings; golden corpus re-run with the 68 new fixtures compared;
a fresh-seed differential-gen spot run (≥5,000 queries, positive bases — proves no
regression); all three fuzz targets re-smoked. Zero tests deleted. Any genuine Java bug found
→ `docs/WALNUT-BUGS.md` + ported verbatim.
