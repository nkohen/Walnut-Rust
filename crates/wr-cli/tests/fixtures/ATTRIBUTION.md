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

No `walnut-java` source code is included.
