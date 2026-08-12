# PORTING.md — Java (Walnut) → Rust (walnut-rs) idiom map

The Bun rewrite's top-billed preparation artifact was a reviewed pattern-map created *before* any
port code, so that many differently-tiered agents don't each invent a different answer to the same
recurring question. This is that map for walnut-rs. **It is a Phase-0 deliverable and must be
adversarially reviewed before Phase 2 begins.** Treat every entry as the *default* — deviations must
be justified in the diff.

> Status: **DRAFT / incomplete.** Seeded with the known mappings; the fleet extends it as new patterns
> recur. When you hit a Java idiom not covered here, add the ruling here *before* porting the third
> occurrence.

## Core discipline (repeat of CLAUDE.md, because it governs every entry)

- **Mechanical first.** Preserve Walnut's *behavior*, including quirks and dead code (port stale
  `TODO`s / no-op calls verbatim; note them, don't "fix" them). Idiomatic refactors are *separate,
  later* commits.
- **Compare by semantics.** Never reproduce Walnut's exact state numbering/canonicalization; the
  comparison bar is language-equivalence (`wr-core` oracle).

## Structural mappings

| Java (Walnut) | Rust (walnut-rs) | Notes |
|---|---|---|
| `public static` global state (`Prover.mainProver`, `usingOTF`, `Session` paths) | fields of an explicit `Session` context struct, threaded through calls | The one sanctioned deviation from mechanical fidelity. Do NOT use `static mut`/globals. |
| `Token.getUniqueString()` `static long` fresh-name counter | `wr_logic::predicate_env::FreshIdentifiers`, one instance per evaluation, threaded through `Token::act` alongside `&dyn PredicateEnv` | Global mutable counter → threaded state. Scoped per-evaluation, not per-session, since the literal counter value never reaches any observable output (verified: the fresh-name prefix appears nowhere in `walnut-java`'s golden fixtures) — see `predicate_env.rs`'s "Ruling 4" module doc for the full argument. |
| `implements Cloneable` / `.clone()` deep copies of `FA`/`Automaton` | `#[derive(Clone)]` with **deep** semantics | Call sites assume deep copies — verify no accidental shared mutability. |
| One class = one file (`Main/Commands/*`) | one module per command under `wr-cli` | The ~20 **inline** `Prover.java` commands (`split`/`rsplit`/`join`/`transduce`/`convert`/…) have no class — port them as modules too, LAST (kit F9). |

## Type & error mappings

| Java | Rust | Notes |
|---|---|---|
| `null` return / sentinel | `Option<T>` | |
| checked/unchecked exception (`WalnutException`) | `Result<T, WalnutError>` with a real error enum | Do **not** stringly-type errors; the Java code sometimes does — improve it here. |
| `int`/`long` state & digit indices | `usize`/`u32`/`i64` chosen per range | Watch for silent `as` truncation; Walnut assumes wide ints in places. |
| `java.math.BigInteger` | `num-bigint` (add deliberately) or bounded int if provably small | Confirm the value can't overflow before choosing a fixed width. |
| two coupled `boolean` fields where one gates the other (`FA.TRUE_FALSE_AUTOMATON` gating `FA.TRUE_AUTOMATON`) | a single `Option<bool>` | **Only after proving the fourth combination is unreachable AND unread** — enumerate every writer and every reader in the Java tree and record the audit in the field's doc comment (see `wr-core`'s `fa.rs` for the worked example). If any reader consults the gated flag without checking the gate, keep two `bool`s instead. |
| `boolean[]`/bitset outputs | `Vec<bool>` / a bitset crate | |

## Collections — and the ITERATION-ORDER CORRECTNESS TRAP

| Java (fastutil) | Rust | Notes |
|---|---|---|
| `IntList` / `IntArrayList` | `Vec<i32>` | |
| `Int2ObjectRBTreeMap` (**sorted**) | `BTreeMap<K, V>` | **Ordered** — preserves iteration order. Use where Walnut used an RB/tree map. |
| `Object2IntOpenHashMap` (**unordered**) | `HashMap<K, V>` | See trap ↓ |

**The trap (must-read):** Java `HashMap`/fastutil-open-hash iteration order ≠ Rust `HashMap` iteration
order (Rust randomizes seeds per process). Walnut's output automaton *numbering* can depend on
iteration order. Because we compare by **language-equivalence, not structure** (kit F1), this is
usually harmless — **but** if any ported algorithm's *result language* depends on iteration order,
that is a real bug in the port (or in Walnut). Rule: where a Java structure is iterated and the order
could affect a result, use an **ordered** map (`BTreeMap`/`IndexMap`) to make the port deterministic,
and add a property test that the result is order-independent.

## Known regression classes (from the Bun rewrite — watch for these)

- **`debug_assert!` erasing side effects.** Never put a side-effecting expression inside
  `debug_assert!`/`assert!`; it vanishes in release builds. (Bun hit this.)
- **Bounds checks.** Rust keeps bounds checks that Java's JIT / a `ReleaseFast` build might elide;
  behavior can differ at the edges — don't `get_unchecked` to "match" Java.
- **Slice length semantics** — off-by-one at boundaries when translating `a.length`/substring math.

## Phase 3 rulings (added as `wr-logic`'s parser/quantifier layer landed)

- **`NumberSystem` memoization lives in `wr-core`, never in `wr-logic`.** Java hands the
  *same* cached `NumberSystem` instance to every token in a formula and lets `act()`-time
  code mutate it (`getConstant`/`getMultiplication`/`getDivision`'s dynamic tables). Rather
  than let that force `Rc<RefCell<NumberSystem>>`-style sharing into every `Token`/
  `Expression::act` signature, `wr_core::numsys::NumberSystem` owns its own memoization
  behind interior mutability (U5); `wr-logic`'s `PredicateEnv` trait hands out a
  read-only `Rc<NumberSystem>` and nothing wraps it in a second `RefCell`. Full argument:
  `crates/wr-logic/src/predicate_env.rs`'s module doc, "Ruling 1".
- **`\G`-anchored lexing needs `regex-automata`, not `regex`.** Java's lexer is 15
  `\G`-anchored `Matcher.find(index)` calls (match starts *exactly* at the cursor, never
  scans forward); the `regex` crate has no `\G` anchor. `regex-automata`'s
  `Input::new(hay).span(index..).anchored(Anchored::Yes)` is the equivalent, added to
  `crates/wr-logic/Cargo.toml` with the same justification style as the workspace root's
  `num-bigint` entry. Four empirically-verified Java-regex → Rust-regex dialect
  divergences the lexer (U3) must handle (no look-behind, ASCII-vs-Unicode `\s`/`\w`/`\d`,
  character-class intersection *does* survive, group numbering must be re-derived per
  pattern) are catalogued in `predicate_env.rs`'s "Ruling 2" — read it before porting
  `Predicate.java`'s regex table, don't re-derive these from scratch. (This is a
  *different* `regex-automata` use than the still-open `dk.brics` question below — see
  that entry, don't conflate the two.)
- **A lexer never borrows the string it's lexing; it owns a `String` buffer.** Java's
  `Predicate.putMacro` rewrites the predicate string mid-lex and rebuilds all its
  matchers over the new string. The Rust lexer instead owns `src: String` (not `&'a str`),
  never holds a match `Span`/slice across a buffer edit (extract offsets/copies first,
  *then* mutate), and needs no matcher-rebuild step at all — `regex-automata`'s `Input` is
  per-call, so the 15 patterns are compiled once into a `OnceLock`/`LazyLock` static.
  Byte-vs-UTF-16 offset units diverge for the two non-ASCII grammar characters (˜ U+02DC,
  ◌̃ U+0303); U3/U4 must decide explicitly how to handle that in position-reporting error
  text, not let a golden `error*` fixture discover it. Full argument: `predicate_env.rs`'s
  "Ruling 3".
- **`PredicateEnv` (`&self`) must be implemented on a narrow `Session` sub-struct, never
  on `Session` as a whole.** `PredicateEnv`'s four methods deliberately take `&self`
  (`predicate_env.rs`'s trait docs), because the four file-library lookups it wraps are
  read-only/memoizing. `crate::determinize::DeterminizeContext` (landed U0c, `wr-core`) is
  the opposite shape: its methods take `&mut self` (`next_automaton_index` must advance;
  `strategy`/`export_pre_determinization` are implementor-defined and may too). Nothing
  implements both today, so this is not a live bug — but if U14's `Session` implemented
  `PredicateEnv` directly on the whole struct, a nested evaluation needing
  `&mut dyn DeterminizeContext` from `Session` at the same time it holds
  `&dyn PredicateEnv` borrowed from `Session` would hit ordinary Rust aliasing rules.
  Ruling, settled now so U14 doesn't have to re-derive it under pressure: `PredicateEnv`
  is implemented on a narrow field/sub-struct of `Session` that holds *only* the four
  file-library lookups (the word/function/macro libraries and the number-system cache),
  kept disjoint from whatever field(s) later hold `MetaCommands`/determinize-context
  state. `Session` as a whole should never itself be the `impl PredicateEnv for _` target.

## Open questions to resolve during Phase 0 (add rulings here)

- Multi-track alphabet representation in `wr-core` (Walnut's `RichAlphabet`) — pick the Rust rep once.
- NFA transition representation (Walnut: `Int2ObjectRBTreeMap<IntList>`), and how the equivalence
  oracle consumes it.
- `dk.brics` regex→automaton replacement (`regex-automata` + converter) — pin the converter's contract.
  **Unrelated to the "Phase 3 rulings" `regex-automata`-for-lexing entry above**: that entry settles
  `\G`-anchored *lexing* of predicate strings (`Predicate.java`'s tokenizer); this one is about
  converting a `reg`-command regex *string* into an automaton (replacing `dk.brics.automaton`). Same
  crate, unrelated use — this question is still open even though lexing's is settled.
