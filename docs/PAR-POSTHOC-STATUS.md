# `agent/par-posthoc` — results

Companion to [`PAR-POSTHOC.md`](PAR-POSTHOC.md) (the design and the coverage map). This file
holds the measured numbers and the verdict.

**Measurement caveat, stated once and meant.** Every timing here was taken on a machine shared
with two sibling agents running their own release builds and corpus replays; observed load
average ranged from 6 to 34 on 8 cores. Timings are **spot checks**, not results. Correctness
tallies (golden, differential, byte-stability) are unaffected by load and *are* results.

---

## 1. Verification matrix

### 1a. Fast tier

`cargo test --workspace` — **40 suites, 0 failures**, with the parallel path enabled by default.

`wr_core::par_determinize`'s own 8 tests, all green:

| Test | What it pins |
|---|---|
| `canonicalize_is_the_identity_on_subset_construction_output` | Lemma L2 |
| `canonicalize_recovers_the_sequential_numbering_from_any_relabelling` | Lemma L1, with reverse/rotation/shuffle permutations |
| `par_matches_the_sequential_implementation` | field-for-field contract, large deterministic inputs |
| `par_matches_the_sequential_implementation_on_nondeterministic_inputs` | same, on real NFAs with genuine blow-up (with its own anti-vacuity floor) |
| `the_parallel_phase_really_does_produce_a_different_numbering` | **anti-vacuity for the whole lane** |
| `repeated_parallel_runs_agree_bit_for_bit` | determinism of the recovered result |
| `small_inputs_delegate_to_the_sequential_implementation` | the threshold is real |
| `a_zero_alphabet_automaton_delegates_to_the_sequential_implementation` | the degenerate-shape guard |

The fifth is the one that makes the rest mean anything: it runs the *raw*, un-canonicalized
parallel construction 20 times on a 1200-state input and asserts that the numbering genuinely
differs from the sequential one — and that canonicalization brings every one of those distinct
numberings back to the sequential answer. Without it, every other result in this module would be
consistent with the parallel phase having accidentally reproduced the sequential order.

### 1b. Tier 1 — golden corpus

*(filled in below from the runs)*

### 1c. Tier 3 — differential vs the real JVM

*(filled in below)*

### 1d. Determinism of written artifacts (the 5× check)

*(filled in below)*

---

## 2. Timings

*(filled in below)*

---

## 3. Ignored tests

*(filled in below)*

---

## 4. Verdict

*(filled in below)*
