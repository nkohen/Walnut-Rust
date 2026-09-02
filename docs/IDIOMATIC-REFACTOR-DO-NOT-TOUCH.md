# Idiomatic-refactor DO-NOT-TOUCH list (U0, mechanically derived 2026-08-29)

Built as the union of (1) `grep -rin "load-bearing\|iteration order\|insertion order\|
order-sensitive" crates/ --include='*.rs'` (81 hits, curated below), (2) the U34 plan's 10-site
behavior-critical order audit (`~/.claude/plans/glossy-compacting-lantern.md` §1), and (3) the
refactor plan's hand list (`~/.claude/plans/robust-seeking-flamingo.md`). The grep alone is NOT
sufficient — several sites phrase their constraint differently ("order of minting is observable",
"BFS order") — which is why this is a curated union, not raw grep output. **This file goes
verbatim into every implementer and reviewer prompt. Cite by symbol; line numbers rot.**

An entry here means: the named code's *representation, iteration order, call order, or text* is
observable behavior. It may only change under an S-gate explicitly scoped to it by the plan —
for most entries, not at all in this refactor.

## Frozen modules (entire)

- **`wr_core::equiv`** — the semantic-equivalence oracle every verification tier (golden,
  differential-gen, bench answer-check) compares through. A defect here makes every gate MORE
  permissive, undetectably. Refactoring it would need its own unit gated on `wr-cts`'s Moore
  minimizer + the brute-force Myhill–Nerode oracle. Out of scope for this whole refactor.
- **`wr-cts`** (both files) — the independent cross-check oracle; same reasoning.
- **`wr_cli::walnut_exception`** — all 33 Java message texts verbatim, including every quirk
  (double period, missing spaces after colons — the named typos are EXAMPLES, not an exhaustive
  list; the categorical rule is every byte frozen).

## Order-load-bearing sites (symbol order / insertion order / numbering)

- `wr_core::determinize` — `subset_construction`'s metastate discovery order (BFS over
  `metastate_list` in index order × symbols ascending `0..alphabet_size`; first-seen assigns the
  next id) IS the output automaton's state numbering, observable in every `.txt`/`.gv` byte and
  `::`-details state count. The `0..alphabet_size` probe range is itself load-bearing: a
  transition-table key outside that range (negative, or `>= alphabet_size`) is silently dropped,
  matching Java (WB-038 outcome (b)) — not a bounds check to "fix."
- `wr_core::fa` — `determine_permutation_map` → `canonicalize`: consumes `d`'s ascending
  `BTreeMap` symbol order AND each destination list's insertion order; feeds state numbering for
  every `.txt`/`.gv` write.
- `wr_core::product` — `cross_product_internal`'s BFS and its destination loops: A-outer/B-inner,
  each list in **insertion order, never sorted** (pinned by exact-structure tests).
- `wr_core::infinite` — `find_cycle`/`find_path`: witness construction visits symbols in
  ascending order; pinned by an exact-string test. (`wr_core::trim::trim` is a documented
  precondition of these.)
- `wr_core::minimize` — the unstable-sort tie-break: permutes elements inside a set, which can
  change final set numbering. Do not "stabilize" or reorder it.
- `wr_core::search` — BFS order, pruning, tie-breaking, and reconstruction are all observable
  (shortlex witness output through the `test` command).
- `wr_core::logicalops` — `update_transitions_from_morphism` (ascending symbol order used as a
  digit value); `remove_states_with_output_rebuild` (reads `dests[0]`); the totalization
  precondition (that file's own "single most load-bearing thing"); the `||` short-circuit in the
  quotient guard; the documented load-bearing `fa` mutation in the fix-trailing-zeros path.
- `wr_core::transducer` — the state-key shape and marker value (ported exactly; mutate-by-one
  breaks four tests); `add_first_entries` (symbol order used as a word position);
  `transduce_msd_deterministic`'s BFS. Its module-doc "# Iteration order" section certifies which
  maps may be `HashMap` — but no swap happens in this refactor regardless.
- `wr_core::quantify` — the `Vec<BTreeMap<i32, BTreeSet<usize>>>` reverse-map stays a `BTreeMap`
  UNCONDITIONALLY (non-monotone insertion on the hottest ∃ path; a sorted Vec measurably
  regresses); the list-union helper preserves insertion order; the `!A.isEmpty()`-before-flags
  ordering is reproduced, not paraphrased.
- `wr_core::numsys` — the constructor's exact-Java-sequence block ("the order is load-bearing");
  `lexicographic_less_than`'s sort; the swapped comparison spelling (cosmetic everywhere except
  state 0, where it is load-bearing); the `is_neg` coupling; the name distinction that keeps
  `msd_fib` distinguishable from base files.
- `wr_core::ostrowski` — the continuous dense `state_transitions` fill discipline (the
  `putIfAbsent`-sweep equivalent is load-bearing, not defensive filler).
- `wr_core::regex` — the deliberately-truncating cast (load-bearing for WB-024's negative-digit
  behavior).
- `wr_core::automaton` — `#[derive(Clone)]`'s stale-`q` preservation is LOAD-BEARING for
  `as_dfa` (directly constrains U7 clone work); the trivial-`Fa` branch checks; the
  already-deterministic gating before `determinize_and_minimize`.
- `wr_core::morphism` — `mapping`'s `BTreeMap`/`BTreeSet` sorted iteration order is read
  directly into `Morphism::write`'s file output and image construction.
- `wr_logic::token` — `get_unique_string`'s minting order ("the order of minting is
  observable"); the two dispatch blocks documented as "Java's exact order (order matters /
  order-sensitive)"; the `(`-handling match arm marked load-bearing.
- `wr_logic::predicate` — the DESCENDING `%N` substitution order (`%1` is a substring of `%10`).
- `wr_logic::predicate_env` — the `\G`-anchored non-forward-scanning matcher.
- `wr_logic::eval` — the leftover-stack iteration order (index 0 = first pushed).
- `wr_io::writer` — `write_gv`'s label-grouping insertion order and `export_to_ba`'s emission
  (the `.gv` fixture is the corpus's ONLY byte-exact automaton coverage). Callers touchable,
  emission loops not.
- `wr_io::reader` — `ns_name` threading (load-bearing for numeration-mismatch guards); the
  number-system memo cache identity; the index-accounting order in DFA library-file reads.
- `wr_io::matrix_writer` — the `EMITTERS` array order; the `domains[0]`-varies-slowest
  (outermost) loop nesting; every emitted byte.
- `wr_cli::morphism` — output in `TreeMap` (= `BTreeMap`/`BTreeSet`) iteration order.
- `wr_cli::test_command` — `find_next_accepted_word` reads `dests[0]`.

## Behavioral quirks that must survive verbatim

- `Fa::clear` deliberately leaves `q` stale; `Fa::set_fields` deliberately never sets `q0`
  (WB-016 — fixed at the call site only; the primitive's quirk is still live).
- `Prover::caught` + `wr_core::walnut_panic` — the panic-recovery boundary. Guard panics use
  `panic!`/`assert!(cond, "msg")`, never `assert_eq!` (payload text reaches the user). No
  guard-panic → `Result` conversion in this refactor.
- `wr_core::logging` — its bools are WB-039's faithfully-buggy non-nesting
  `printEnabled`/`printDetails` machinery; excluded from flag-enum conversion. `Logging` CALL
  ORDER anywhere is observable `::`-details text — any refactor that can reorder, add, or drop a
  `Logging` call is out of scope everywhere (no tier reliably catches it; see U6 criterion (d)).
- `Automaton`'s deep `Clone` is a sanctioned aliasing contract — call sites assume deep copies.
- `Automaton::label` is deliberately NOT parallel to the track vectors:
  `label.len() != alphabet.len()` encodes Java's null-label "unbound" state (`is_bound`,
  `unlabel`, product's unbound panics, quantify's early-return all depend on it).
- All `WB-0xx` cross-reference comments (~583 mentions) survive verbatim, moving only with their
  code. Every `Display`/log/panic message string is frozen. The three quoted Java `TODO`s in
  `numsys.rs` stay (they are quotations, not debt).
- **Zero tests deleted, ever.** A refactor that breaks a test fixes the refactor.
