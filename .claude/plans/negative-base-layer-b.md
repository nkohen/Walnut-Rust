<!--
PROCESS NOTE, written after the fact and deliberately not softened:

This plan was NOT adversarially reviewed before its code was written, unlike
`negative-base-layer-a.md`. `docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`'s step 4 asks for a
Layer-B plan "same rigor as step 2", i.e. reviewed by an independent agent before any code
lands. That did not happen: the plan was drafted into a session scratchpad while Layer A's
diff reviewers were running, and execution started from it directly.

It is filed here now so the design reasoning is on the record and reviewable, not to imply
it went through the gate. The two diff-level adversarial reviewers dispatched for Layer B
are not a substitute — they see the code, not the design choices behind it (e.g. the
decision to re-resolve number systems by name rather than carry the object, and the choice
to put the base-change file probe in `wr-cli`).
-->

# Plan — Layer B: the base-change surface + `split`/`rsplit`

Dispatch: `docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`, steps 4-5. **Depends on Layer A** (committed
separately): the negative number system `determineNegativeNS` builds is a real, working
`NumberSystem`, and `processSplit`'s `arithmetic(…, 0, PLUS)` call runs on its adder.

## 1. What Layer B is

Three `NumberSystem` methods U7 deleted, plus one command:

| Java | Rust target |
|------|-------------|
| `baseNBaseChange(int n)` (`NumberSystem.java:568-601`) | new free fn `base_n_base_change(n, is_msd, base_name_underscore)` in `wr_core::numsys` |
| `setBaseChangeAutomaton()` (`:443-468`) | `NumberSystem::set_base_change_automaton(&mut self, candidates: CustomBaseCandidates)`, I/O-free like `with_custom_base_files` |
| `determineNegativeNS()` (`:219-230`) | `numsys::negative_ns_name(name) -> Result<String>` (the pure name computation) + resolution through `PredicateEnv` at the `wr-cli` layer |
| `UNDERSCORE_BASE_CHANGE_AUTOMATON` (`:82`), `NEG_UNDERSCORE` (`:77`) | new consts |
| `Main/Commands/Split.java` (123 LOC) | new `crates/wr-cli/src/split.rs` |
| `Prover.splitCommand`/`rsplitCommand` (`:663-675`) | replace the two `NotYetImplemented` arms in `crates/wr-cli/src/prover.rs` |

### `baseNBaseChange` is verified correct, and is LSD-oriented

Worked out independently from the Java before porting, because the state numbering is opaque.
Track 0 is the base-`n` (positive) numeral, track 1 the base-`(-n)` one; reading
**least-significant digit first**, the state is `(c, parity)` where `y_k - x_k = c·n^k` and
parity is `k mod 2`:

| state | meaning | Java's transitions |
|-------|---------|--------------------|
| 0 (accept) | `c = 0`, even `k` | `i == j` → 1 |
| 1 (accept) | `c = 0`, odd `k` | `i == 0 && j == 0` → 0; `i + j == n` → 2 |
| 2 | `c = -1`, even `k` | `i + 1 == j` → 1; `i == n-1 && j == 0` → 3 |
| 3 | `c = -1`, odd `k` | `i + j == n - 1` → 2 |

with the recurrence `c' = (c + b·(-1)^k - a) / n` and acceptance at `c = 0`, i.e. outputs
`[1,1,0,0]` — exactly Java's `IntList.of(1,1,0,0)`. Every one of Java's six
`addNewTransition` lines is accounted for and none collide on a `(state, symbol)` pair
(checked: state 1's two arms need `i+j == 0` vs `i+j == n`; state 2's need `i+1 == j` vs
`i == n-1 && j == 0`).

Because it is built lsd-first, `setBaseChangeAutomaton` reverses it when `isMsd` — the
**opposite** of the `if (!isMsd) reverse(...)` every other construction uses. Getting that
backwards is the single most likely silent bug in this layer, so it gets its own test.

### Two structural facts that shape the port

* `baseNBaseChange` uses the **one-argument** `initBasicAutomaton(IntList)` overload — no
  alphabets, no number systems — then adds two of each by hand and calls
  `determineAlphabetSize()`. So the two tracks carry the names `<msd|lsd>_n` and
  `<msd|lsd>_neg_n`, taken from the NEGATIVE system's own `determineBaseNameUnderscore()`.
  In this port that is `ns_name = [Some("msd_3"), Some("msd_neg_3")]`, `msd = [Some(true); 2]`.
* `setBaseChangeAutomaton` is only reached with `parseNegNumber(base) > 1` OR a shipped
  `_base_change.txt`. `walnut-java` ships exactly one: `Custom Bases/msd_neg_fib_base_change.txt`
  (header `msd_fib msd_neg_fib`). So the programmatic path never has to construct a
  file-backed `NumberSystem` recursively — `NumberSystem::new` suffices there, and the
  file-backed path needs no programmatic construction. No new I/O in `wr-core`.

## 2. `Split.java`'s composition — confirmed, no new algorithmic risk

Every primitive `processSplitCommand`/`processSplit` uses already exists in `wr-core`
(verified by reading both sides): `word_automaton::uncombine`, `logicalops::combine`,
`logicalops::and`, `quantify::quantify`, `Automaton::{bind, set_label, sort_label,
random_label, clone}`, `util::remove_duplicates`, and `wr_cli`'s `write_automata` /
`prover_helper::determine_out_library` / `TestCase::from_automaton`. The genuinely new code
is the wiring plus §1's three `NumberSystem` methods.

Two things in `processSplit` that are easy to get wrong and get named tests:
* The `PLUS` arm binds `(a, b)` or, when `reverse`, `(b, a)`; the `MINUS` arm binds
  `(reverse ? b : a, c)` and ANDs in `arithmetic(reverse ? a : b, c, 0, PLUS)` — three
  independent `reverse` ternaries with different polarities.
* `quantifiers` accumulates `b` in the `PLUS` arm and BOTH `b` and `c` in the `MINUS` arm,
  and `quantify` runs ONCE at the end over the whole set, after the loop.

## 3. Java-side Phase 0 first (a separate `walnut-java` commit)

The coverage sweep run for this unit found `Split.java` is the thin one (`NumberSystem`'s
negative-base paths are already at 100% line / 100% branch):

* `pom.xml`'s `code-coverage` profile still excludes `Main/Commands/Split*.class` — a stale
  DROP-scope exclude, exactly like the one the Ostrowski unit had to remove. Delete it.
* `Split.java` measures 87.9% line / 81.0% branch, and **every error path is dead**, plus
  one real load path: the `isDFAO = false` branch (`:26-30`) — every corpus fixture's operand
  is a word automaton, so `split` on a plain automaton is untested on either engine. Add
  `src/test/java/Main/Commands/SplitTest.java` covering: the Automata-Library load branch,
  `invalidCommand` for a non-`+`/`-` operator, "Cannot split without inputs." (both
  sub-conditions), "Cannot process split automaton with no inputs.", "Split automaton has
  incorrect number of inputs.", and "Number system for input i must be defined."
* Only then port against it (the Rust tests replicate whatever `SplitTest` pins).

## 4. Test plan

* **Tier 2** — a Rust `#[test]` twin of each new `SplitTest` method, plus
  `NumberSystemTest.testMakeNeg` (`:116-127`, including the "double negative remains
  negative" quirk) and `testLsdNegativeNumberSystemGetsABaseChangeAutomaton` (`:362`) and
  `testNegativeFibonacciBaseChangeComesFromACustomBaseFile` (`:377`).
* **Tier 4** — a Walnut-independent sweep of `base_n_base_change`: for `n ∈ {2, 3}` and every
  pair of words up to length 5, the automaton accepts iff `value_base_n(x) == value_base_neg_n(y)`,
  reusing Layer A's `value_msd_neg` oracle. Both directions (`msd` and `lsd`), which is what
  pins the inverted-reverse hazard from §1.
* **Tier 3** — `tests/differential/tests/split_command.rs`, driven through the real
  `Prover::dispatch`, against freshly captured `walnut-java` output for the four sign
  combinations plus a `rsplit` round-trip.
* **Tier 1** — flip the remaining 15 `drop_command:split|rsplit` fixtures in
  `subset-filter.json` (592+68=660 → 675, `drop_relevant_count` 15 → 0), plus the
  `skip-transitive-drop-dep[test444..447]` entry the harness currently records for fixture 448.
  Target: 675 compared, 674 pass (383 remains the one known text-only divergence).

## 5. Merge gate

Same as Layer A: `cargo test --workspace` green, `fmt`/`clippy`/`doc` clean, golden corpus
re-run, differential spot check, zero tests deleted, two independent adversarial reviewers
(at least one on a different model than the author) with no unresolved correctness finding.
Any genuine Java bug found → `docs/WALNUT-BUGS.md`, ported verbatim.

## 6. Stop conditions

If the base-change surface turns out to need file I/O inside `wr-core` (it should not — §1),
or if `processSplit`'s `quantify` composition diverges from Java in a way that is not a simple
port defect, **stop and write it up** rather than forcing a merge. Layer A is already
committed and green, so a Layer-B blocker costs nothing already banked.
