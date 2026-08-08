# walnut-rs

A Rust reimplementation of a **research-driven subset** of the
[Walnut](https://walnut-theorem-prover.github.io/) automatic-theorem-prover — the tool that decides
first-order-logic statements about automatic sequences.

**Goals:** faster than the JVM Walnut; owned and extensible for constant-term-sequences research; and above
all **correct** — as-well-tested as Walnut, ideally with *fewer implementation bugs*. The underlying algorithms
are trusted; correctness effort targets the faithfulness of the *port*, verified by differential testing against
Java Walnut, property-based invariants, a golden corpus, and fuzzing.

Status: **scaffold / Phase 0.** This is a workspace skeleton plus its operating discipline. Nothing is
implemented yet.

- **The plan:** [`docs/DESIGN.md`](docs/DESIGN.md) — scope, correctness ladder, roadmap, adversarial-review record.
- **How agents work here:** [`CLAUDE.md`](CLAUDE.md).

## Layout

```
crates/
  wr-core     FA engine: DFA/NFA/DFAO, determinize, minimize, product, reverse, + language-equivalence oracle
  wr-numsys   base-k msd/lsd number systems (adder, comparator, constant automata)
  wr-logic    formula parser (AST), quantifier elimination, boolean ops — the FOL decider
  wr-io       Walnut .txt automaton reader/writer (multi-track + NFA), Graphviz
  wr-cli      Prover/Session command dispatch + REPL  (binary: `walnut-rs`)
  wr-cts      adapter over RustConstantTermSequences primitives
```

## Build

```bash
cargo build --workspace
cargo test  --workspace
```

## License

GPLv3-or-later. walnut-rs is a derivative work of Walnut (GPLv3); see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).
