# walnut-rs

A Rust reimplementation of a **research-driven subset** of the
[Walnut](https://walnut-theorem-prover.github.io/) automatic-theorem-prover — the tool that decides
first-order-logic statements about automatic sequences.

**Goals:** faster than the JVM Walnut; owned and extensible for constant-term-sequences research; and above
all **correct** — as-well-tested as Walnut, ideally with *fewer implementation bugs*. The underlying algorithms
are trusted; correctness effort targets the faithfulness of the *port*, verified by differential testing against
Java Walnut, property-based invariants, a golden corpus, and fuzzing.

Status: **complete and in use.** The full first-order-logic decider is implemented — parser, quantifier
elimination, boolean/product ops, determinize (`SC` + Brzozowski), Valmari minimize, reverse, quotient;
`eval`/`def`/`reg`/`morphism`/`image`/`transduce`/… and the `.txt` automaton format; base-k, negative-base,
custom-base (Fibonacci/Pell), and Ostrowski numeration; CAS matrix export. Correctness is verified across all
tiers (Java unit-test replicas, a semantic-equivalence golden corpus of Walnut's own integration fixtures,
large-scale differential testing against the real Java engine, property-based invariants, and fuzzing), and on
the benchmarked research workloads it is faster than the JVM engine. It is consumed downstream (ct-research) as
a drop-in for the Java Walnut engine.

- **Consuming it from another project:** [`docs/CT-RESEARCH-INTEGRATION.md`](docs/CT-RESEARCH-INTEGRATION.md).
- **The plan & record:** [`docs/DESIGN.md`](docs/DESIGN.md) — scope, correctness ladder, roadmap, review record.
- **How agents work here:** [`CLAUDE.md`](CLAUDE.md).

## Layout

```
crates/
  wr-core     FA engine: DFA/NFA/DFAO, determinize, minimize, product, reverse, quotient, + language-equivalence
              oracle; and the number systems (module `numsys`: base-k/negative-base/custom-base msd/lsd adders,
              comparators, constant automata) — kept inside wr-core because Automaton<->NumberSystem are coupled
  wr-logic    formula lexer/parser (AST), quantifier elimination, boolean ops — the FOL decider
  wr-io       Walnut .txt automaton reader/writer (multi-track + NFA), Graphviz, CAS matrix export
  wr-cli      Prover/Session command dispatch + REPL (binary: `walnut-rs`); embedding facade (module `embed`)
  wr-cts      adapter over RustConstantTermSequences primitives + a from-scratch cross-check minimizer
tests/        golden corpus (Tier 1), differential (Tier 3), differential-gen (Tier 3 at scale)
benches/      Criterion benchmarks + the head-to-head vs a warm walnut-java JVM
fuzz/         cargo-fuzz targets (Tier 5; separate nightly toolchain)
```

## Build & test

```bash
cargo build --workspace
cargo test  --workspace          # fast tier
cargo fmt --all && cargo clippy --workspace --all-targets
```

## Run

```bash
./build.sh                                  # release binary at target/release/walnut-rs
printf 'eval q "?msd_2 Ex x=x";\nquit;\n' | ./bin/walnut-rs
```

The `walnut-rs` binary is a stdin REPL that resolves (and writes) its library directories relative to its cwd,
matching the Java engine. `bin/walnut-rs` is the launcher a downstream consumer runs; see the integration doc.

## License

GPLv3-or-later. walnut-rs is a derivative work of Walnut (GPLv3); see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).
