<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.). -->

# `fuzz/` — Tier-5 fuzzing (Phase 4, U30)

`docs/DESIGN.md` §5's Tier 5: **`cargo-fuzz` + coverage, seeded with a malformed-input
corpus**. Three targets, one per hand-written parser in the port. Each target's job is
**crash-freedom only** — no panic, no OOM, no hang. Semantic correctness belongs to
Tier 1 (`tests/golden`) and Tier 3 (`tests/differential-gen`); a fuzz target that tried to
assert language equivalence would be a much worse oracle than either, so none of them do.

## Layout and toolchain

`fuzz/` is **its own workspace** (`[workspace]` in `Cargo.toml`) and is `exclude`d from the
repo root's. That is not tidiness — it is required:

* `cargo-fuzz` needs **nightly** (`-Z sanitizer`, `-C llvm-args=-sanitizer-coverage-*`),
  while the repo root pins stable 1.97.1 (`../rust-toolchain.toml`). Toolchain files
  resolve by walking *up* from the cwd, so this directory carries its own dated pin in
  `rust-toolchain.toml`.
* Being outside the root workspace also keeps it out of `cargo test --workspace` and
  `cargo clippy --workspace`. **It therefore needs its own gate** (see below) — the root
  invocations do not reach it.

```
fuzz/
  fuzz_targets/       the three targets
  seeds/<target>/     curated, COMMITTED seed inputs (read-only corpus)
  corpus/<target>/    libFuzzer's working corpus — gitignored, run output
  regressions/        minimized crashing inputs from findings not yet fixed
  artifacts/          libFuzzer crash/OOM/timeout artifacts — gitignored
```

## Running

`cargo-fuzz` is installed against the nightly pin
(`cargo +nightly-2026-05-01 install cargo-fuzz`).

**This machine needs a linker override** (see "Environment notes"), so every invocation is
prefixed with:

```sh
export RUSTFLAGS="-Clinker-features=+lld -Clink-self-contained=+linker -Zunstable-options"
```

Then, from `fuzz/` (create `corpus/<target>/` once — libFuzzer requires the writable
corpus directory to exist):

```sh
mkdir -p corpus/wr_io_reader corpus/wr_logic_parser corpus/wr_core_regex

cargo fuzz run wr_io_reader    corpus/wr_io_reader    seeds/wr_io_reader \
  -- -max_len=512 -timeout=25 -rss_limit_mb=4096 -max_total_time=300

cargo fuzz run wr_logic_parser corpus/wr_logic_parser seeds/wr_logic_parser \
  -- -max_len=256 -timeout=25 -rss_limit_mb=4096 -max_total_time=300

cargo fuzz run wr_core_regex   corpus/wr_core_regex   seeds/wr_core_regex \
  -- -max_len=128 -timeout=25 -rss_limit_mb=4096 -max_total_time=300
```

The first directory is the **writable** corpus, the second is the read-only seed set — so
a run never mutates what is committed, and a clean checkout always starts from the same
inputs.

**Time budget.** `-max_total_time=300` (5 minutes each) is the *validation* budget used
when this unit landed, enough to confirm a target is not crashing on trivial input. A real
hardening pass wants hours per target; nothing here caps that, just raise
`-max_total_time` (or drop it and interrupt). The `-max_len` values match the in-harness
`MAX_INPUT_LEN` constants, which are enforced in the target too so a committed seed or a
replayed artifact cannot bypass them.

### Gate (this crate is not covered by `cargo …--workspace`)

```sh
cd fuzz
cargo +nightly-2026-05-01 fmt --all -- --check
RUSTFLAGS="-Clinker-features=+lld -Clink-self-contained=+linker -Zunstable-options" \
  cargo clippy --all-targets
RUSTFLAGS="-Clinker-features=+lld -Clink-self-contained=+linker -Zunstable-options" \
  cargo fuzz build
```

### Reproducing a crash

```sh
cargo fuzz run   <target> artifacts/<target>/crash-…        # replay
cargo fuzz tmin  <target> artifacts/<target>/crash-…        # minimize
```

## The targets

### `wr_io_reader` — `wr-io`'s Walnut `.txt` reader

Drives `read_automaton_from_str` **and** `read_transducer_from_str` from the same input:
the two grammars share `parse_header` and differ only in the body, so one corpus exercises
both. Both string entry points were added for this unit (`crates/wr-io/src/reader.rs`) —
the reader was path-only before, which would have meant a temp file per iteration.

Seeds (116): every `.txt` under `tests/differential/fixtures/` below 512 bytes (111 of
113), one transducer (`crates/wr-io/tests/fixtures/RUNSUM2.txt`), and the four malformed
files `docs/DESIGN.md` §5 names by name, copied from
`walnut-java/src/test/resources/unitTests/` — `bogusTransitionDeclaredFirst.txt`,
`bogusAlphabetNotMatch.txt`, `bogusTrueWithConflict.txt`, `bogusInvalidSyntax.txt`.

No custom-base resolver is supplied: resolving one means real file I/O, which a fuzz
target must not do. A custom-base header still reaches `parse_header` and returns
`UnsupportedNumeration`, so the header parse itself is fully under test.

### `wr_logic_parser` — `wr-logic`'s FOL lexer/parser

Drives `Predicate::new`. `PORTING.md` flags this tokenizer as the port's highest
hand-written-parser risk: Java drives 15 `\G`-anchored `Matcher` patterns by index, which
the `regex` crate cannot express at all, so the port re-expresses it with
`regex-automata`'s anchored `Input::span`. Mis-anchoring, UTF-16 offset drift, and
unguarded indexing into multi-byte characters are exactly what a fuzzer finds and a
fixture corpus does not.

`Predicate::new` is **not** a pure parse — it takes a `PredicateEnv`, and lexing `?msd_k`
eagerly builds that number system's `O(k³)` addition/comparison automata. The target
supplies a `BoundedEnv` (no file I/O, base capped at 8, one word `T`, one function `f`,
one macro `m` so those token branches lex past the library lookup) so the run explores
*lexing and parsing* rather than automaton-construction cost.

Seeds (49): query strings extracted from `Predicate::new`/`with_context` call sites and
`eval`/`def` command strings across `crates/*/src` and `tests/*/tests`.

### `wr_core_regex` — `wr-core`'s Brics-dialect regex engine

Drives both public entry points, which are different parsers rather than one wrapping the
other: `convert_from_brics` (single-track, two alphabets — `{0,1}` and the
non-contiguous `{2,4,1}`, which exercises the `List.indexOf`-shaped dense-symbol map), and
`determine_encoded_regex` + `AutomatonDFA::from_encoded_regex` (the multi-track path `reg`
actually uses, whose `[a,b,…]` alphabet-vector pre-scan has no single-track equivalent).

Seeds (58): the regex bodies from `tests/differential/tests/reg_brics_regex.rs`'s corpus
table — 60 automaton cases plus the pinned parse-error cases — which are the strings
behind `tests/differential/fixtures/reg/*.txt` (those files are the *resulting automata*,
not regexes).

## In-harness input filters — read this before adding one

Each target rejects some inputs up front. They fall into two kinds, and the distinction
matters:

**Budget filters ("cost, not bug").** The decision procedure is superexponential by
construction and real Walnut blows up identically on the same inputs, so a libFuzzer OOM
or timeout there is textbook algorithmic cost, not a defect — but it stalls the run on a
non-finding. These implement `CLAUDE.md`'s "generate SMALL" guardrail:

| Target | Filter | What it bounds |
| --- | --- | --- |
| `wr_io_reader` | `MAX_INPUT_LEN`, `MAX_HEADER_LEN`, `MAX_HEADER_DIGIT_RUN` | auto-determinize-on-load blowup; `msd_99999999`'s 400 MB alphabet; `usize` overflow in the mixed-radix `alphabet_size` product |
| `wr_logic_parser` | `MAX_INPUT_LEN`, `MAX_BASE` | `NumberSystem::new`'s `O(k³)` construction (WB-032 documents real Walnut doing the same on `msd_1000`) |
| `wr_core_regex` | `MAX_INPUT_LEN`, `MAX_REPEAT_DIGIT_RUN`, `MAX_REPETITION_OPS`, `MAX_CONSECUTIVE_REPETITION_OPS`, `MAX_COMPLEMENT_OPS`, `MAX_ATOMS` | `e{0,99999999}`; `e++++…` (Brics' `e+` = `concat(e, star(e))`, so each `+` doubles); `Σ*·α₁…αₙ` determinizing to 2ⁿ |

**Known-crash bypasses.** These exist *only* because a target would otherwise die on a
known, already-reported finding within seconds and discover nothing else. They are
deliberately loud in the source — each is documented on its own constant/function with the
minimized reproducer and a **"delete this once the underlying defect is fixed"** note, in
the same spirit as `tests/golden`'s `KNOWN_DIVERGENCES` list. They are never a silent skip.

## Findings

Three findings, all from the first 5-minute run of their target, all confirmed against the
real `walnut-java` CLI (`~/dev/walnut-java/target/Walnut-all.jar`). **None was fixed here**
— all three live in `crates/wr-core`/`crates/wr-io`, which need the project's
implementer → two-independent-reviewer → fixer loop. Each minimized input is committed
under `regressions/`.

### F1 — `wr_core::util::parse_int` panics on `i32` overflow, from raw user input, at two call sites

`parse_int` panics (`"For input string: \"…\""`) when its argument overflows `i32`. Its
doc comment justifies that: *"every real call site in this port passes text already
validated by a regex-shaped matcher, so the only realistic failure is `i32` overflow"* —
and reaches for a panic rather than a `Result` on the reasoning that this is an
internal-invariant violation, not untrusted input. **That reasoning does not hold at two
call sites**, both reachable straight from what a user types:

* `regex.rs`'s `parse_set_elements` (the `reg` command's `[a,b,…]` alphabet vectors) —
  `reg r {0,1} "([8888888800])"`.
  Reproducer: `regressions/wr_core_regex/f1-parse-int-i32-overflow-in-alphabet-vector`
* `wr-logic`'s `@N` alphabet-letter token (`Predicate.java:220`) —
  `?msd_2 T[x] = @8888888888`.
  Reproducer: `regressions/wr_logic_parser/f1b-parse-int-i32-overflow-in-alphabet-letter`

**Real Walnut on the same inputs**: throws `java.lang.NumberFormatException: For input
string: "8888888800"` — i.e. the port reproduces Java's *message* byte-for-byte, which is
what makes this a port defect rather than a `WALNUT-BUGS.md` entry. What diverges is
**recoverability**: Java's REPL catches the exception and returns to the prompt, while the
Rust panic is process-fatal. That is exactly the class the U17–U26 review round already
fixed several instances of ("multiple process-killing panics on ordinary mismatched-input
commands, now guarded").

### F2 — `Automaton::encode` panics on an out-of-alphabet digit read from a `.txt` file

`read_automaton_str_impl` calls `automaton.encode(&digits)` **inside** its parse loop, on
digits taken straight from an untrusted file. `encode`'s own doc says it *"Panics if a
digit isn't present in its track's alphabet (a caller bug, not a data error)"* — again a
premise that does not hold at this call site.

Reproducer (18 bytes, `regressions/wr_io_reader/f2-encode-panics-on-out-of-alphabet-digit`):

```
 lsd_2
0 1
20-> 11
```

**Real Walnut on the same file**: `State 11 is used but never declared anywhere in file:
Automata Library/fz.txt` — a clean `WalnutException`. Java gets *past* the encode step
(`RichAlphabet.encode` uses `List.indexOf`, which returns `-1` and silently corrupts the
encoding rather than throwing) and then hits its undeclared-state validation, which the
port performs only *after* the loop. Two sub-cases worth separating for whoever fixes it:

* undeclared destination state (above): Java = clean error, Rust = **panic**;
* declared destination state (` lsd_2\n0 1\n20 -> 0\n`): Java = uncaught
  `IndexOutOfBoundsException: Index -1 out of bounds for length 2`, REPL recovers; Rust =
  panic, process-fatal. Java's own unhelpful exception here looks like a genuine Walnut
  defect and may deserve a `WB-` entry in its own right — flagged, not filed, since
  filing is the coordinator's call.

Negative digits (`-1 1 -> 0` under `msd_2`) reach the same panic.

### F3 — `wr-io`'s header parser accepts a base below 2

`parse_ns_token` (`crates/wr-io/src/reader.rs`) builds `(0..base).collect()` for **any**
`i32` it can parse out of a `msd_`/`lsd_` token. So `msd_1` yields the one-symbol alphabet
`{0}`, and `msd_0`/`msd_-3` yield the *empty* alphabet; the first body digit then trips
F2's panic. Java's `NumberSystem.parseBase` rejects `<= 1` outright
(`if (!isNumber(baseStr) || Integer.parseInt(baseStr) <= 1) throw`).

Reproducer (`regressions/wr_io_reader/f3-header-accepts-base-below-2`):

```
# .
msd_1

1 1
1 -> 0
```

**Real Walnut on the same file**: `Number system msd_1 is not defined.` This is a distinct
defect from F2 — a missing *validation*, not a missing *guard* — and fixing it does not
fix F2 (an out-of-alphabet digit under a legitimate `msd_2` still panics).

## Run results at landing

Each target, 5 minutes, after its findings above were filtered out:

| Target | Executions | Result |
| --- | --- | --- |
| `wr_io_reader` | 8,655,725 | clean |
| `wr_logic_parser` | 4,984,611 | clean |
| `wr_core_regex` | 429,672 | clean |

`wr_core_regex`'s much lower rate is expected: every iteration runs a full Thompson
construction → determinize → minimize → dead-transition prune, three times.

## Environment notes

**This machine's system linker cannot link an ASAN binary.** The Xcode Command Line Tools
here ship `ld64-650.9` (Apple clang 12.0.5, ~2021), which hits
`Assertion failed: (_mode == modeFinalAddress) … ld.hpp:1161` on ASAN's thread-local
fixups. Building with the nightly toolchain's bundled `rust-lld` instead resolves it
completely — that is what the `RUSTFLAGS` above do, and ASAN is genuinely linked (no
`__sanitizer_*` dlsym warnings at startup). Updating the Command Line Tools would also fix
it and make the override unnecessary; the override is harmless either way, since
`rust-lld` ships with the toolchain.

The flags cannot move into `.cargo/config.toml`: `cargo-fuzz` sets `RUSTFLAGS` itself
before invoking cargo, and the environment variable takes precedence over
`build.rustflags`/`target.<triple>.rustflags`, so a config-file value would be silently
ignored.

`cargo fuzz run --sanitizer=none` also works and needs no override, at the cost of losing
ASAN — worth knowing, but ASAN is on by default here because it is available.
