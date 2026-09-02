# The post-hoc parallelization lane (`agent/par-posthoc`)

**Status: experiment.** Branch `agent/par-posthoc`, based on `dfed86b` (top of `perf/beyond`).
Nothing here is proposed for `master` as-is; the deliverable is the working prototype plus the
map below.

## The bet

Do **not** constrain execution order during computation so that outputs come out right by
construction. Instead: let the parallel engine compute racily — arbitrary metastate discovery
order, arbitrary state numbering, whatever parallelizes best — and then **reconstruct the
sequential-equivalent artifacts afterwards** by canonicalization.

The question the experiment answers: *which* sequential artifacts are recoverable that way, which
are not, and does the reconstruction cost eat the parallel win.

## 1. Why post-hoc recovery works at all — the Canonical Recovery Lemma

Two facts about the port's existing `Fa::canonicalize` (`crates/wr-core/src/fa.rs:593`), a
verbatim port of Java's `FA.canonizeInternal`:

**L1 — `canonicalize` is a permutation invariant.** For an `Fa` `A` and any bijection `π` on its
states, `canonicalize(π(A)) == canonicalize(A)` field for field.
`determine_permutation_map` (`fa.rs:643`) is a FIFO BFS from `q0` that, at each popped state,
walks `d[q].values()` — the state's symbols in ascending `BTreeMap` order, and within a symbol the
destination list in stored order. Relabelling preserves both orders exactly (keys are untouched;
destination lists are mapped element-wise, so length and position correspondence survive). So the
BFS on `π(A)` visits `π(q)` at the same step the BFS on `A` visits `q`, and assigns it the same new
id. Reachability pruning and empty-list pruning are defined purely in terms of that BFS and the
list contents, both preserved.

**L2 — `canonicalize` is the identity on any `subset_construction` output.** The sequential
construction (`determinize.rs:310`) mints ids in exactly BFS-from-`q0`, ascending-symbol order: its
worklist cursor walks metastates in id order, ids are dense and minted increasing (so id order *is*
FIFO discovery order), and for each metastate it walks `0..alphabet_size` ascending. Every metastate
is reachable from metastate 0 by construction, and no empty destination list is ever recorded.

**Corollary.** `canonicalize(π(SC(fa, init))) == SC(fa, init)` for any `π`. A parallel subset
construction is therefore free to mint ids in whatever order threads race to — the freedom the
deterministic-parallelization lane does not have.

Both lemmas are *tested*, not just argued:
`par_determinize::tests::canonicalize_is_the_identity_on_subset_construction_output` (L2) and
`canonicalize_recovers_the_sequential_numbering_from_any_relabelling` (L1, with the reverse
permutation — the worst case for a BFS renumberer — plus rotations and shuffles).

## 2. Coverage map — what is and is not recoverable post hoc

### 2a. Recoverable (numbering is genuinely unobservable)

| Surface | Why |
|---|---|
| `subset_construction` output | L1 + L2 above. Exact, proven, tested. |
| `.txt` final artifact | `write_txt` calls `automaton.canonize()` first (`wr-io/src/writer.rs:171`). |
| `.gv` final artifact | `write_gv` likewise (`writer.rs:297`). |
| CAS matrix export | `matrix_writer.rs:577` likewise. |
| `right_quotient`, `reverse_and_canonize`, `combine` results | each ends in `force_canonize()` (`logicalops.rs:906`, `:991`, `:1595`). |
| `Automaton::set_alphabet` path | `force_canonize()` (`automaton.rs:1600`). |
| Per-operation **state counts** in `::` details text | A count is a cardinality; relabelling cannot change it. Applies to `Trimmed to:`, `Minimizing:`/`Minimized:`, `Determinizing`/`DETERMINIZED`, `computed cross product:`. |

The `.txt`/`.gv` row is the strongest single result: because `minimize` produces a minimal DFA and
the writer canonicalizes, **the final artifact is canonical up to isomorphism regardless of every
intermediate numbering**. That is why the deferred mode (§3) is even worth testing.

### 2b. NOT recoverable by `canonicalize` — and why

| Surface | Obstruction |
|---|---|
| **`export_to_ba` (`.ba` files)** | `writer.rs:409` takes `&Fa`, not `&mut Automaton`, and never canonicalizes. Its emitted `q0`, `{sym},{q}->{dest}` lines and accepting-state list carry raw ids. Any renumbering is visible in the bytes. Sole production caller: `wr-cli/src/prover_helper.rs:175`. |
| **`morphism::to_word_automaton` / `ostrowski` outputs** | Both deliberately `set_canonized(true)` (`morphism.rs:443`, `ostrowski.rs:472`) precisely so the writer's `canonize()` is a **no-op** and unreachable states survive into the emitted file. Post-hoc canonicalization there would *change* the output, not restore it. |
| **NFA-shaped intermediates** | `canonicalize` preserves destination-list order rather than sorting it (`fa.rs:617-620`), and that order feeds the BFS itself. So for a nondeterministic automaton, canonicalization is *not* a complete normal form: a parallel engine must reproduce `product`'s A-outer/B-inner list order (`product.rs:341`) and `quantify`'s union order exactly. |
| **`sort_label`'s `label_sorted` memo** | Set `true` before the early returns (`automaton.rs:1765`), so track/symbol order is path-dependent, not a pure function of the final automaton. |
| **`[strategy n]` / `[export n]` metacommand indices** | The counter advances once per *actual* determinization (`determinize.rs:251`). Any parallel engine that adds, skips, or reorders determinizations desynchronizes every later index — and `[export n]` writes files. Numbering-independent, order-*dependent*. |
| **`::` details text ORDER and indentation** | `Logging` is a linear buffer with an indent counter (`logging.rs`). A DAG-parallel evaluator interleaves two subtrees' lines. Recoverable *in principle* by buffering per-node and splicing in post-order — see §5 — but not by canonicalization. |

### 2c. Numbering-dependent by construction, but harmless *because* something canonicalizes later

These are the intermediates a deferred-recovery engine perturbs. They are listed to be explicit that
the deferred mode is not "safe" — it is "safe **if** nothing observes them before the write path":

- `minimize` (Valmari): output numbering is block-id numbering derived from input positions
  (`minimize.rs:191-217`, `:371`), and `new_q0 = blocks.set_of[fa.q0]` is generally **not** 0
  (`:450`). No caller re-canonicalizes.
- `trim`: order-preserving compaction of input ids, `new_q0` not necessarily 0 (`trim.rs:87`).
- `product`: discovery order is A-symbol-major, which is **not** ascending *result*-symbol order
  whenever the second operand contributes an extra track (the result symbol is
  `a_sym + (b's extra digits) * a.alphabet_size`). So `canonicalize` on a cross-product result is
  **not** an identity in general.
- `quantify`: ends in `minimize`, so its output is block numbering.

## 3. The two implemented modes

`crates/wr-core/src/par_determinize.rs`, selected by env var, dispatched from the engine's single
`SC` entry point (`determinize.rs`'s `Strategy::Sc` arm):

| Mode | Env | Behavior |
|---|---|---|
| `Sequential` | `WR_PAR=0` | Stock engine. The baseline. |
| `ParallelEagerRecovery` | *(default)* | Compute racily; canonicalize **inside** the primitive. Returns the sequential `Fa` field for field, so nothing downstream — including `.ba` exports and details text — can observe that it ran. |
| `ParallelDeferredRecovery` | `WR_PAR_DEFER=1` | Compute racily and let the arbitrary numbering flow into `minimize`/`product`/the rest, relying only on the writer's own `canonize()`. This is the aggressive variant the experiment exists to test. |

### How the parallel phase works

Level-synchronous BFS over metastates, workers spawned once for the whole call
(`std::thread::scope` + two `Barrier`s per level, so per-level thread spawn is not paid):

1. All workers hold a read guard on the current level and take a block-cyclic slice of it.
2. Each worker computes its metastates' per-symbol unions with **thread-local** scratch buffers —
   a transliteration of the sequential C1/C2 loop (member-outer bucket fill, ascending-symbol drain,
   epoch dedup marker), so the destination *set* per symbol is identical.
3. Each union key is hash-consed into a 256-way sharded lock table; ids come from one
   `AtomicUsize`. **Which id a metastate gets depends on which thread reaches its shard lock
   first** — the numbering is genuinely nondeterministic run to run.
4. Barrier; worker 0 swaps the next level in; barrier; repeat.

Then phase B: assemble id-indexed `o`/`d` tables and call `Fa::canonicalize`.

Inputs below `MIN_STATES_FOR_PARALLEL = 64`, or with `alphabet_size == 0`, delegate to the
sequential implementation. That threshold is not only a performance guard: `subset_construction` is
`pub` and `Fa` has no invariant forbidding a destination id `>= q`, so several *panic* shapes are
pinned by existing tests, and delegating small inputs keeps those panic sites bit-identical.

## 4. Honest limits of the prototype

- **`brzozowski` and `regex.rs` still call the sequential `subset_construction` directly**
  (`determinize.rs`'s `Strategy::Brz` arm, `regex.rs:1017`, `:1144`). Only the `SC` arm is wired.
  This is deliberate — `brzozowski`'s internal calls are on reversed automata whose panic behavior
  is pinned separately — but it means `[strategy 6 BRZ]` workloads get no parallel win.
- **Malformed large inputs panic from inside a scoped thread**, so the panic *message* is wrapped
  rather than identical to the sequential one. Below the threshold it is bit-identical.
- **`minimize` is untouched.** Valmari is sequential-hard (its refinement is inherently a
  dependent chain), and its numbering is not recoverable by canonicalization anyway. What the
  post-hoc lane buys here is *upstream* freedom, not a parallel minimizer: because `minimize`'s
  input is canonicalized before it (eager mode), or its output is canonicalized at the writer
  (deferred mode), the numbering of everything feeding it stops being load-bearing.

## 5. What DAG-level parallelism would additionally require (not implemented)

Independent `eval` subtrees are genuinely independent computations, but three shared, ordered
side-effects make naive parallelism observable:

1. **`Logging`** — one linear buffer with an indent counter. Reconstructible post hoc: give each
   subtree its own `Logging`, then splice the buffers in the sequential post-order at join. The
   *content* of each subtree's lines is order-independent (state counts, not ids); only the
   concatenation order and the indent base need fixing. Timing (`- Nms`) is already normalized out
   by both engines' harnesses.
2. **The metacommand automaton index** — `next_automaton_index()` must be assigned in sequential
   order, not in the order determinizations actually finish. Reconstructible: count
   determinizations per subtree, then renumber at join. This is real work, not a flag flip, and
   `[export n]` writes a *file* named after the index.
3. **`NumberSystem` caching on `Session`** — memoized construction is a shared mutable map, and
   *whether* a construction happens at all is already observable in details text (this is fixture
   383's whole story, and the 375-379 harness limitation). Parallel subtrees race on it.

None of these is a canonicalization problem; all three are *ordering* problems, which is exactly
the boundary between this lane and the deterministic one.
