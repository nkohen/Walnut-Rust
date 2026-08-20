# Fixture provenance

`hardInfTest.txt` is copied verbatim from the `walnut-java` oracle's own unit-test
resources (`src/test/resources/unitTests/hardInfTest.txt`), part of
[Walnut](https://walnut-theorem-prover.github.io/) (GPLv3, Mousavi et al.) — see `NOTICE`
at the repo root.

`msd_fib.txt` and `msd_fib_addition.txt` are copied verbatim from `walnut-java`'s
`Custom Bases/msd_fib.txt` / `Custom Bases/msd_fib_addition.txt` — the real
Zeckendorf-representation number system Walnut ships (same provenance and licence). They
are byte-identical to the copies at `crates/wr-io/tests/fixtures/`; each crate keeps its
own copy rather than reaching across a crate boundary for a test resource, matching this
workspace's existing convention.

They exist here so `eval_def.rs`'s regression test can build a session whose
`Custom Bases/` really resolves a non-`msd_k` numeration — the shape that
`fresh_number_system_resolves_a_custom_base_for_a_function_token` pins.

## Ostrowski fixtures (`ostrowski/msd_*.txt`)

The sixteen files under `ostrowski/` are copied verbatim from `walnut-java`'s own
unit-test resources (`src/test/resources/unitTests/msd_{fib,numsys,pell,ns6,ns7,ns8,ns9,
ns10}{,_addition}.txt`), the exact files `Automata/Numeration/OstrowskiTest.java`'s
`testAgainstFile` helper compares its constructed automata against — same provenance and
licence. `crates/wr-cli/src/ost.rs`'s Tier-2 tests use them the same way, so the Rust
port is pinned to the same byte-for-byte expectations the Java suite is, against files
this port did not generate.

(`msd_fib.txt`/`msd_fib_addition.txt` appear both here and under `ostrowski/`; the two
copies are byte-identical, and Walnut's shipped `Custom Bases/msd_fib*.txt` are in turn
byte-identical to its `unitTests/` copies — `ost fib [0 2] [1];` really does reproduce
the shipped file. The duplication is kept so each consumer's fixture set reads
self-contained.)

No `walnut-java` source code is included.
