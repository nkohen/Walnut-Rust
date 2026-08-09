# Fixture provenance

`automaton572.txt`, `automaton2.txt`, `automaton189.txt` and `automaton214.txt` are
copied verbatim from the `walnut-java` oracle's own golden integration-test corpus
(`src/test/resources/integrationTests/{automaton572,automaton2,automaton189,automaton214}.txt`),
part of [Walnut](https://walnut-theorem-prover.github.io/) (GPLv3, Mousavi et al.) — see
`NOTICE` at the repo root. Used here as `.txt` format parser fixtures only; no
walnut-java source code is included.

`automaton189.txt` / `automaton214.txt` are the trivial TRUE / FALSE automaton files —
literally the single word `true` / `false`, with no trailing newline. They are two of
the 85 such fixtures in the corpus, kept byte-for-byte (rather than hand-written) so the
reader is exercised against the real no-trailing-newline shape Walnut's writer emits.
