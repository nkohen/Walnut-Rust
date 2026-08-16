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
| `wr_io_reader` | `MAX_INPUT_LEN`, `MAX_HEADER_LEN`, `MAX_HEADER_DIGIT_RUN`, `MAX_STATES_FOR_DOWNSTREAM`, `MAX_ALPHABET_SIZE_FOR_PRODUCT` | auto-determinize-on-load blowup; `msd_99999999`'s 400 MB alphabet; `usize` overflow in the mixed-radix `alphabet_size` product; and, for the downstream steps added in review round 2, the cross product's **squared** alphabet (`msd_92 msd_4 msd_55` is a 410-million-entry product alphabet — a 33-second libFuzzer timeout, textbook cost) |
| `wr_logic_parser` | `MAX_INPUT_LEN`, `MAX_BASE` | `NumberSystem::new`'s `O(k³)` construction (WB-032 documents real Walnut doing the same on `msd_1000`) |
| `wr_core_regex` | `MAX_INPUT_LEN`, `MAX_REPEAT_DIGIT_RUN`, `MAX_REPETITION_OPS`, `MAX_CONSECUTIVE_REPETITION_OPS`, `MAX_COMPLEMENT_OPS`, `MAX_ATOMS` | `e{0,99999999}`; `e++++…` (Brics' `e+` = `concat(e, star(e))`, so each `+` doubles); `Σ*·α₁…αₙ` determinizing to 2ⁿ |

**Known-crash bypasses.** These exist *only* because a target would otherwise die on a
known, already-reported finding within seconds and discover nothing else. They are
deliberately loud in the source — each is documented on its own constant/function with the
minimized reproducer and a **"delete this once the underlying defect is fixed"** note, in
the same spirit as `tests/golden`'s `KNOWN_DIVERGENCES` list. They are never a silent skip,
and they are **removed, not accumulated**: all three that this unit's findings needed are
gone, deleted along with the defects they masked (see "Findings" below). There are
currently none.

A fixed finding's minimized input is **not** thrown away: it moves from `regressions/` into
`seeds/<target>/` under the same name, so every future run replays it, and it is
additionally pinned as an ordinary `#[test]` in the owning crate (a fuzz-found bug gets a
normal regression test, not just a corpus entry). `regressions/` therefore does not exist
right now — recreate it when a new finding lands that cannot be fixed immediately.

## Findings

Three findings, all from the first 5-minute run of their target, all confirmed against the
real `walnut-java` CLI (`~/dev/walnut-java/target/Walnut-all.jar`). **All three are now
fixed**, together with a fourth instance of F1's root cause found while fixing it. Each
minimized input has moved into `seeds/<target>/` and is additionally pinned by an ordinary
`#[test]`; the three known-crash bypasses that masked them are deleted.

Every one of them turned out to be a **port defect, not a Walnut defect** — no
`docs/WALNUT-BUGS.md` entry was needed. The shared reason is `Prover.readBuffer`
(`Prover.java:387-392`), which wraps each command's `dispatch(s)` in
`catch (RuntimeException)` and prints a truncated stack trace: Java's *session survives*
every one of these inputs. Verified, not inferred — each reproducer was run through the
real jar with an ordinary `eval` after it, and that following command evaluated normally
in the same session every time. A Rust `panic!` has no equivalent boundary and kills the
process, so matching Java means returning a `Result`, never panicking.

### F1 — `wr_core::util::parse_int` panicked on `i32` overflow, from raw user input — FIXED

`parse_int`'s doc justified its panic on the grounds that *"every real call site in this
port passes text already validated by a regex-shaped matcher"*. A regex-shaped `\d+`
gate constrains a token's **shape** and says nothing about its **magnitude**, so that
reasoning was false at every site whose digits come from the user. Four, in the end:

| Call site | Reproducer | Now |
| --- | --- | --- |
| `wr_core::regex::parse_set_elements` (`reg`'s `[a,b,…]` vectors) | `reg r {0,1} "([8888888800])"` | `RegexError::NumberFormat` |
| `wr_logic`'s `@N` alphabet letter (`Predicate.java:220`) | `?msd_2 T[x] = @8888888888` | `LexError::NumberFormat` |
| `wr_cli::alphabet::parse_set_elements` (`Alphabet.java:56-59`) | `alphabet foo {8888888800} bar` | `AlphabetError::NumberFormat` |
| `wr_io::parse_methods`' state/transition/dest/output groups | `msd_2\n99999999999\n0 -> 0 / 0` (transducer reader) | `ReadError::ParseMethods(NumberFormat)` |

The last two were found by reading, not by the fuzzer — the third has no fuzz target at
all, and the fourth is on the `wr_io_reader` target's `read_transducer_from_str` half but
needs a 10+-digit run the 5-minute run never mutated its way to. All four now go through
the new `wr_core::util::try_parse_int`, which returns
`NumberFormatError` rendering `NumberFormatException.getMessage()` byte-for-byte
(`For input string: "8888888800"` — the exact text the real jar prints). `parse_int`
itself is unchanged and still panics, for the call sites where an overflow genuinely
would be an internal-invariant violation.

### F2 — `Automaton::encode` panicked on an out-of-alphabet digit read from a `.txt` file — FIXED

`read_automaton_str_impl` called `automaton.encode(&digits)` inside its parse loop, on
digits taken straight from an untrusted file, and `encode` panics on a digit absent from
its track's alphabet.

The original report split this into "undeclared destination state (Java = clean error)"
and "declared destination state (Java = `IndexOutOfBoundsException`)". Running the real
jar's classes directly showed both halves have the same cause and neither is a reader
error in Java: **`AutomatonReader` has no out-of-alphabet check at all.**
`RichAlphabet.encode` uses `List.indexOf`, gets `-1`, and stores the transition under
that bogus key; `new Automaton(" lsd_2\n0 1\n20 -> 0\n")` loads with no error and
reports 1 state. What happens next depends only on what the automaton is used for:

* **undeclared destination** — `validateDeclaredStates`, which runs *after* the whole
  parse loop, reports `State 11 is used but never declared anywhere in file: …`;
* **declared destination** — the file loads. The `IndexOutOfBoundsException: Index -1 out
  of bounds for length 2` the original report saw comes much later and from somewhere
  else entirely: `AutomatonWriter.writeToGV` → `RichAlphabet.decode(-1)`, reached from
  `EvalDef.evalDefCommand`'s result write, four frames outside the reader. `readBuffer`
  catches it and the session continues;
* **and often nothing at all** — for many shapes the `-1` key is simply dropped by the
  next pass that iterates `0..alphabetSize`, and real Walnut writes back the input minus
  the offending line, with no diagnostic whatsoever
  (`msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n` does exactly this).

So rejecting out-of-alphabet digits with a clean error would have *diverged* from Java on
that third bullet — the common case. The fix is instead the verbatim port: a new
`Automaton::encode_index_of` reproducing `List.indexOf`'s `-1` (the same treatment
`wr_core::regex`'s `encode_with_index_of` already carries for WB-024), used by both the
automaton and the transducer reader. The undeclared-state case then reaches this port's
existing `ReadError::UndeclaredDestState` exactly as Java reaches
`validateDeclaredStates`, and the declared case loads the same automaton Java loads.

**Follow-up (review round 2) — F2's first fix relocated a panic class instead of removing
it, and opened a silent-wrong-answer path. Both are now closed:**

* The `-1` key the reader now (faithfully) stores reaches raw `[sym as usize]` indexing in
  `wr_core::product`, `wr_core::automaton`, `wr_core::quantify` and
  `wr_core::word_automaton`, so `union`/`intersect`/`join`/`inf`/`test`/`combine`/… still
  killed the process on a file real Walnut merely errors on. Patched not at those five
  sites but at the layer Java itself guards: `wr_cli::prover`'s `Prover::caught`, a
  `catch_walnut_panic` boundary around `dispatch`/`dispatch_for_integration_test`, mirroring
  `Prover.readBuffer`'s `catch (RuntimeException)` — which wraps *every* command, which is
  why `eval`/`def` (already covered by `EvalDef.compute`'s own inner catch, ported in
  `wr_logic::eval`) never showed the problem during manual probing.
* The residual `decode` divergence this section used to report as untouched — `rem_euclid`/
  `div_euclid` where Java uses truncating `%`/`/`, so `decode(-1)` returned *a* digit where
  Java throws — is ported as well. It was not the "graceful direction": it made this port
  silently **write a fabricated automaton** where Java errors and writes nothing, which is
  worse than a crash. `Automaton::try_decode` is now bounds-checked and returns
  `DecodeError::IndexOutOfBounds` with the JDK's own text; `Automaton::decode` panics with
  exactly that message and the boundary above turns it into an ordinary command error.

This target now also **exercises downstream steps after a successful read** (writer
round-trip, `determinize_and_minimize`, `sort_label` + `cross_product`). Stopping at the
read is why 6M clean executions found neither of the two defects above: the reader itself
never touches the `-1` key it stores.

### F3 — `wr-io`'s header parser accepted a base below 2 — FIXED

`parse_ns_token` built `(0..base)` for **any** `i32` it could parse out of a
`msd_`/`lsd_` token, so `msd_1` yielded the one-symbol alphabet `{0}` and `msd_0`/`msd_-3`
the *empty* alphabet. Java's `NumberSystem` constructor rejects that outright
(`:322-332`: neither `isNumber(base) && parseInt(base) > 1` nor `parseNegNumber(base) > 1`
holds, and no `Custom Bases/msd_1*.txt` rescues it), and the real jar answers
`Number system msd_1 is not defined.` — session intact.

The new `numeric_base` helper ports that check, including its use of `isNumber`
(`^\d+$`) rather than `str::parse`, which Java's is and which `parse` is not (it would
accept a leading `+`/`-`). A base `<= 1` is now `ReadError::NumSys(NumSysError::NotDefined)`,
a `\d+` base overflowing `int` is `NumSysError::BaseNotAnI32` (Java's unchecked
`NumberFormatException` at `:325`), and a non-numeric rest still falls through to the
custom-base file lookup exactly as Java's does.

One message-level divergence, from a pre-existing tokenizer simplification rather than
from this fix: Java's `PATTERN_NEXT_ALPHABET_TOKEN` cannot consume a `-`, so real Walnut
reads `msd_-3` as the token `msd_` and says `Number system msd_ is not defined.`, whereas
this reader's whitespace-delimited tokenizer keeps the whole word and reports
`UnsupportedNumeration("msd_-3")`. Both are errors on the same input; only the name in
the text differs, and closing it means porting that regex.

## Run results at landing

Each target, 5 minutes, after its findings above were filtered out:

| Target | Executions | Result |
| --- | --- | --- |
| `wr_io_reader` | 8,655,725 | clean |
| `wr_logic_parser` | 4,984,611 | clean |
| `wr_core_regex` | 429,672 | clean |

(`wr_core_regex`'s much lower rate is expected: every iteration runs a full Thompson
construction → determinize → minimize → dead-transition prune, three times.)

## Run results after the F1/F2/F3 fixes

Same 5-minute budget per target, with the three known-crash bypasses deleted — so each
target now explores strictly *more* input than it did above (literal-set `.txt` headers
and arbitrary body digits for `wr_io_reader`, 10+-digit runs for `wr_logic_parser`,
multi-digit `[…]` vector elements for `wr_core_regex`), which is why the throughput drops:

| Target | Executions | Result |
| --- | --- | --- |
| `wr_io_reader` | 6,052,725 | clean |
| `wr_logic_parser` | 4,789,840 | clean |
| `wr_core_regex` | 100,681 | clean |

`wr_core_regex` also recorded one **slow unit** (not a crash; the run still exited 0):
`~.0..................` — 20 `.` atoms under a complement, i.e. the `Σ*`-concatenation
determinization blowup `MAX_ATOMS` already documents, sitting exactly at that limit.
Textbook algorithmic cost, identical in real Walnut/Brics; recorded rather than acted on.
Its much lower rate throughout is expected for the same reason: every iteration runs a
full Thompson construction → determinize → minimize → dead-transition prune, three times.

## Run results after the review-round-2 fixes (dispatch boundary + bounds-checked `decode`)

`wr_io_reader` now runs the downstream steps described in its module docs, so this is not
comparable to the numbers above — it does strictly more work per execution:

| Target | Budget | Executions | Result |
| --- | --- | --- | --- |
| `wr_io_reader` | 5 min | 1,993,643 | clean |
| `wr_logic_parser` | 2.5 min | 2,808,192 | clean |
| `wr_core_regex` | 2.5 min | 53,493 | clean (2 slow units) |

`wr_core_regex`'s two slow units are the same already-documented class as the one recorded at
landing — `~&0..................` / `~.0..................`, 20 `.` atoms under a complement,
i.e. the `Σ*`-concatenation determinization blowup `MAX_ATOMS` bounds. Not crashes; the run
still exited 0.

The extension paid for itself immediately: the first two runs of the new target ended in
crashes, both of them **harness** defects rather than port defects, and both worth recording
because they define what this target may and may not call —

* `true` (a trivial TRUE/FALSE automaton) reaching `cross_product`, whose first statement is
  a deliberately-ported Java guard every real caller checks `isTRUE_FALSE_AUTOMATON` before
  reaching. Now skipped for that step only.
* `msd_92 msd_4 msd_55` (a 410-million-entry product alphabet) timing out at seed-replay
  time — the squared-alphabet cost that `MAX_ALPHABET_SIZE_FOR_PRODUCT` now bounds.

It also re-found the WB-038 `-1`-key panic class in `wr_core::product` on
`{ 1}\n0 1\n0 -> 0` — the exact class the dispatch boundary was added for. That one is *not*
a harness defect and *not* a new finding: it is the ported behavior of a Java
`RuntimeException`, and it is what motivates the guarded/raw split in `exercise_downstream`
(see its call site). The `wr-cli` regression test
`a_corrupt_library_file_costs_one_command_not_the_process` pins the user-visible half.

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
