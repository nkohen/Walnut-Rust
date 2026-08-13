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
  behind interior mutability; `wr-logic`'s `PredicateEnv` trait hands out a
  read-only `Rc<NumberSystem>` and nothing wraps it in a second `RefCell`. Full argument:
  `crates/wr-logic/src/predicate_env.rs`'s module doc, "Ruling 1".
  **Delivered by U5**: the three dynamic tables are `RefCell<BTreeMap<BigInt, Automaton>>`
  and `get_constant`/`get_multiplication`/`get_division` take `&self`. Two follow-on rules
  that fell out of implementing it, and that any future `RefCell` memo in this port should
  copy: (a) **never hold a cell borrow across a call back into `self`** — every one of
  those three builders recurses, so each does a scoped `borrow()` + `clone()` for the
  lookup and a fresh `borrow_mut()` for the insert, after all recursion has finished
  (violating this is a runtime `already borrowed` panic, not a compile error); (b) a
  method that Java writes as "return the cached instance by reference, and let the public
  wrapper `.clone()` it" collapses into a single clone-returning method here, because a
  `RefCell` cannot lend a reference out past its borrow — verify first that no *internal*
  caller bypassed the public wrapper, or that collapse changes behavior.
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
- **A Java `Pattern` ported to `regex`/`regex-automata`: `\>` and `\<` are NOT literals in
  Rust, and two other spellings must change.** Added in Phase 3b's U21, which ported
  `Prover.java`'s ~30 command-argument patterns and lost a debugging round to the first
  entry below. These are *in addition to* Ruling 2's four divergences (which are about the
  lexer's `\G` anchoring and ASCII classes); they apply to any Java pattern ported anywhere
  in this workspace:
  1. **`\>` is an end-of-word boundary in Rust** (`\<`/`\>` were added to the `regex` crate
     in 1.10), while Java has no such escape and reads `\>` as the literal `>`. The trap is
     that the Java spelling **compiles clean** and then silently never matches:
     `RE_FOR_morphism_CMD`'s `\d+\s*\-\>\s*` matched nothing at all until the `>` was
     unescaped. Port `\>` as `>` (and `\<` as `<`). Escaping `-` outside a character class
     *is* a plain literal in both dialects, so `\-` can stay as-is.
  2. **`]` must be escaped in Rust** even where Java allows it bare — Java's `[^]]`
     ("anything but `]`") becomes `[^\]]`, and a literal `]` outside a class becomes `\]`.
     This one fails loudly (a compile error), so it costs nothing but a spelling change.
  3. **`$` is not the same anchor.** Java's `$` also matches immediately *before* a final
     line terminator; Rust's matches only at the very end of the haystack. Check that the
     strings reaching a `$`-anchored ported pattern are already stripped (`Prover.java`'s
     are, via `readBuffer`), or the divergence is real.
  Capture-group NUMBERING, by contrast, does agree: both engines number by opening
  parenthesis and skip `(?:…)`, so Java's `static int GROUP_… = 20` constants port as
  literal group indices — but pin each one with a test against a real input rather than
  re-counting parens by eye (`crates/wr-cli/src/prover.rs`'s
  `*_group_numbers_match_javas_constants` /
  `every_command_pattern_pins_its_group_indices` tests).
- **`String.strip()` is NOT `str::trim()`, and `String.split("\\s+")` is not
  `str::split_whitespace()`.** Two separate Java string APIs that each look like they have
  an obvious Rust twin, and each of which is wrong in a way that only shows up on input
  nobody writes by hand. Added in Phase 3b's U21 (`Prover.java` `.strip()`s three command
  strings, `MetaCommands.java` two more).
  1. **`.strip()` vs `.trim()` — they disagree in BOTH directions**, because `.strip()`
     is defined by `Character.isWhitespace` and `str::trim` by `char::is_whitespace`.
     Java does *not* count the three non-breaking spaces `U+00A0`/`U+2007`/`U+202F` (nor
     `U+0085` NEL) as whitespace; Rust strips all four. Java *does* count the four
     information separators `U+001C`–`U+001F`; Rust leaves them. Use
     `wr_cli::prover::java_strip` (whose docs carry the full rule) rather than `.trim()`
     wherever the Java said `.strip()`. Note Java's `.trim()` is a *third* function again
     (everything `<= U+0020`) — check which one the source actually called.
  2. **`\s` inside a Java REGEX is a different set again**: ASCII-only
     `[ \t\n\x0B\f\r]`, per Ruling 2. So `split("\\s+")` is neither `.strip()`'s notion
     nor `split_whitespace`'s. It also keeps a leading empty field, drops *all* trailing
     empty ones (limit `0`), and consequently has three lengths worth memorizing:
     `"a b"` → 2, `""` → **1** (`[""]`), `"   "` → **0** (empty array). The `""` and
     `"   "` cases go opposite ways; `split_whitespace` gives 0 for both.
     `crates/wr-cli/src/meta_commands.rs`'s `split_java` is the reference implementation.
- **A lexer never borrows the string it's lexing; it owns a `String` buffer.** Java's
  `Predicate.putMacro` rewrites the predicate string mid-lex and rebuilds all its
  matchers over the new string. The Rust lexer instead owns `src: String` (not `&'a str`),
  never holds a match `Span`/slice across a buffer edit (extract offsets/copies first,
  *then* mutate), and needs no matcher-rebuild step at all — `regex-automata`'s `Input` is
  per-call, so the 15 patterns are compiled once into a `OnceLock`/`LazyLock` static.
  Byte-vs-UTF-16 offset units diverge for the two non-ASCII grammar characters (˜ U+02DC,
  ◌̃ U+0303); U3/U4 must decide explicitly how to handle that in position-reporting error
  text, not let a golden `error*` fixture discover it. Full argument: `predicate_env.rs`'s
  "Ruling 3". **Decided by U3, and the rule for every later port of a Java `Matcher`
  offset: the cursor stays a UTF-8 byte offset, and every offset that reaches a token,
  an error message, or a nested construct's `realStartingPosition` is converted to
  UTF-16 code units first** (`predicate.rs`'s `Predicate::java_offset`, a
  `char_indices().map(len_utf16)` walk — panic-free, and linear in a string whose length
  is irrelevant next to the automaton work each token triggers). Converting makes the
  divergence *zero* rather than "documented", which is worth ~6 lines here because the
  alternative silently shifts every reported position after a `˜` in a query;
  `predicate.rs`'s `token_positions_after_a_non_ascii_operator_match_javas` pins it
  against real Walnut's own output (`a˜=1` -> positions `0,3,2,1`, whose byte offsets
  would be `0,4,3,1`). Corollary for U4: a nested `Predicate`'s `real_starting_position`
  argument is in UTF-16 units, so the offset handed to it must be converted, not a raw
  byte index.
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
- **A closed Java `instanceof`-dispatched class hierarchy ports to one Rust `enum`, not
  one struct per subclass.** Hit for the first time in U2: `Token`/`Operator` (8 concrete
  subclasses) and `Expression` (6 concrete subclasses) are each a Java abstract base
  class whose every real field/behavior access is gated by an `instanceof` check
  narrowing to a **closed disjunction of concrete subclasses** first — sometimes exactly
  one subclass, but not always: e.g. `RelationalOperator.act`'s `ns.comparison(a.identifier,
  b.identifier, opp)` narrows `a`/`b` to `(ArithmeticExpression | VariableExpression)`
  before reading `.identifier` off the still-`Expression`-typed local
  (`RelationalOperator.java:135-137`), and `ArithmeticOperator`'s `getIntConstantForWord`/
  `getConstantValue` read a bare `Expression` parameter's `.constant` after only a
  `NumberLiteralExpression` early-return (`ArithmeticOperator.java:264`, `:275`) — never a
  *fully generic*, unnarrowed base-type read with no `instanceof` at all (verified for
  both hierarchies by tracing every call site before applying this ruling, the same
  "prove the untaken state is unreachable AND unread" bar `PORTING.md`'s
  `TRUE_FALSE_AUTOMATON`→`Option<bool>` entry above already sets — the earlier draft of
  this ruling overclaimed "always narrows to ONE concrete subclass," corrected during
  Phase 3a U2's adversarial review once these multi-subclass call sites were found). For
  the single-subclass case, one Rust `enum` variant (holding only the fields that
  subclass actually sets) per concrete Java subclass is the direct translation of
  `instanceof`-narrowed access — a `match` arm already has exactly the fields in scope
  that an `instanceof`-then-cast block would. For the closed-disjunction case, add a
  small `Option`-returning accessor on the enum (e.g. `Expression::identifier`/
  `Expression::constant` in `expr.rs`) covering exactly the variants that set the field,
  rather than either a `match` at every call site or a field promoted onto the enum
  itself. Either way, with no runtime cost and no risk of reading a field a Java subclass
  left at its default. Do **not** model this as one Rust struct with every field
  `Option`-wrapped (that's Java's problem shape, not a fix for it) or as a trait object
  per concrete type (no dynamic-dispatch need has been found; a `match`/accessor suffices
  everywhere this has come up so far). Where the hierarchy also has its own
  *sub*-hierarchy of "abstract middle class" (`Operator` between `Token` and its five
  operator subclasses), fold the middle class's shared fields into a payload struct held
  by the relevant outer variant (`Token::Operator(Operator)`) rather than adding a second
  enum layer, unless a later unit finds a real reason to split it further. Full worked
  examples: `crates/wr-logic/src/token.rs` and `crates/wr-logic/src/expr.rs`'s module
  docs.

- **A Java field whose per-element type is a whole object, where the port only keeps a
  DERIVED fact, becomes one PARALLEL VECTOR PER FACT — and every mutation site must move
  all of them together.** Hit twice now on the same field: `Automaton.NS`
  (`List<NumberSystem>`, one entry per track). `wr-core`'s `Automaton` does not hold
  `NumberSystem` objects (that would be circular — a `NumberSystem` *owns* three
  `Automaton`s — and would force every hand-built test automaton to construct a real
  numeration system), so it keeps the two facts any ported code actually reads off one:
  `msd: Vec<Option<bool>>` (the msd/lsd direction, Phase 1) and
  `all_reps: Vec<Option<Rc<Automaton>>>` (the custom-base valid-representation restriction,
  U5). The hazard this creates is real and is the reason for the ruling: Java's single
  `getNS().set(i, ns)` / `removeIndices(getNS(), I)` / `permute(getNS(), p)` moves both
  facts at once, so a port that updates one vector and forgets the other silently
  describes two different number systems on one track. Rules: (1) document the invariant
  on the *added* field, including which combinations are impossible and why; (2) update
  every parallel vector in the SAME statement as the original (`automaton.rs`'s
  `clear`/`sort_label`/`reduce_dimension`, `product.rs`'s `update_axb_fields`,
  `quantify.rs`'s track removal, `logicalops.rs`'s `right_quotient`/`flip_ns` are the
  full current list); (3) gate the only public bulk setter on an assertion, and add a
  `debug_assert`-backed invariant check the operations that consume the fields call. Do
  **not** merge them into one `Vec<Option<TrackInfo>>` retroactively unless a unit is
  already touching every `msd` call site for another reason — the churn across
  trust-critical code costs more than the invariant check.
- **Adding a field to a widely-embedded struct can trip `clippy::large_enum_variant`
  somewhere else entirely.** U5's one extra `Vec` on `Automaton` (24 bytes) pushed
  `wr_logic::expr::Expression`'s largest/second-largest variant gap past clippy's 200-byte
  default, because `WordExpression` holds two `Automaton`s where its siblings hold one.
  Apply clippy's own suggestion (box the large variant) rather than `#[allow]` — this
  codebase has no clippy allows and shouldn't grow its first one for a size lint.

## Open questions to resolve during Phase 0 (add rulings here)

- Multi-track alphabet representation in `wr-core` (Walnut's `RichAlphabet`) — pick the Rust rep once.
- NFA transition representation (Walnut: `Int2ObjectRBTreeMap<IntList>`), and how the equivalence
  oracle consumes it.
- `dk.brics` regex→automaton replacement (`regex-automata` + converter) — pin the converter's contract.
  **Unrelated to the "Phase 3 rulings" `regex-automata`-for-lexing entry above**: that entry settles
  `\G`-anchored *lexing* of predicate strings (`Predicate.java`'s tokenizer); this one is about
  converting a `reg`-command regex *string* into an automaton (replacing `dk.brics.automaton`). Same
  crate, unrelated use — this question is still open even though lexing's is settled.
