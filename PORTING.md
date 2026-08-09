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
| `Token.getUniqueString()` `static long` fresh-name counter | a counter field on `Session` (or a passed `&mut FreshNames`) | Global mutable counter → threaded state. |
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

## Open questions to resolve during Phase 0 (add rulings here)

- Multi-track alphabet representation in `wr-core` (Walnut's `RichAlphabet`) — pick the Rust rep once.
- NFA transition representation (Walnut: `Int2ObjectRBTreeMap<IntList>`), and how the equivalence
  oracle consumes it.
- `dk.brics` regex→automaton replacement (`regex-automata` + converter) — pin the converter's contract.
