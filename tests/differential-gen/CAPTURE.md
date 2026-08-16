# Live-oracle capture: the Tier-3 differential generator (U29)

This crate does not replay *recorded* fixtures the way `tests/differential/` does. It drives a
**live `walnut-java` JVM** as an oracle and compares it against the Rust port, query by query,
over a stream of generated queries. This file is the reproducible recipe for the JVM half —
same discipline as `tests/differential/CAPTURE.md`'s `U8Probe.java` entry.

Everything below is what `tests/support/mod.rs`'s `JavaOracle` automates; it is written out so
a human can reproduce, debug, or replace the oracle by hand.

## Prerequisites

* A built oracle jar:

  ```bash
  cd ~/dev/walnut-java
  ./mvnw -q clean package -DskipTests -Pfat-jar        # produces target/Walnut-all.jar
  ```

* **A JDK 17 or newer.** `Walnut-all.jar`'s classes are class-file major version 61 (Java 17);
  an older `javac` refuses them outright with `class file has wrong version 61.0, should be
  55.0`. A `jenv`/`asdf` shim pointing at Java 11 is the usual cause. The harness resolves the
  toolchain in this order — `$WR_DIFFGEN_JAVA_HOME/bin`, `$JAVA_HOME/bin`,
  `/usr/libexec/java_home -v 17`, `/opt/homebrew/opt/openjdk@17/bin`, then bare `PATH` — and
  says so in its failure message if none of them works.

* The sibling checkout must be findable: either beside this repo, or named by
  `WALNUT_JAVA_DIR`. **If it is missing the harness fails loudly** — it never silently skips
  or silently passes (`CLAUDE.md`'s CI/absent-oracle contract, same as `tests/golden`).

## The driver

`java/DiffGenDriver.java` in this directory. It deliberately lives **here, not in
`walnut-java`'s tracked source**: it is a capture recipe, not production Walnut code, and it is
compiled fresh against the jar on every run.

```bash
JDK=$(/usr/libexec/java_home -v 17)          # or /opt/homebrew/opt/openjdk@17
JAR=~/dev/walnut-java/target/Walnut-all.jar
OUT=/tmp/diffgen-classes

"$JDK/bin/javac" -cp "$JAR" -d "$OUT" ~/dev/walnut-rs/tests/differential-gen/java/DiffGenDriver.java
"$JDK/bin/java"  -Xmx2048m -cp "$JAR:$OUT" DiffGenDriver /tmp/diffgen-scratch
```

The single argument is a scratch directory; the driver points Java's (entirely `static`)
`Session` at it, so nothing lands in the user's real working tree. Headless
`eval "…";` writes no automaton file at all (`EvalDef.computeHeadless`,
`Main/Commands/EvalDef.java:79-91`), so the scratch tree stays essentially empty.

### Why one long-lived JVM

Java's `Session` is entirely `static` (`Main/Session.java:41-60`), so **one JVM process is one
Walnut session**. Spawning a JVM per query would put ~0.4-0.7s of startup on every query and
make the run take longer than the decision procedure does. The driver therefore loops on stdin
for its whole lifetime, constructing a fresh `Prover` per query — exactly what
`IntegrationTest.runSpecificTest` (`:926`) does per fixture: fresh dispatch state, one shared
session.

### Wire protocol

All records are UTF-8 and **NUL-terminated**; NUL is the only reserved byte, and a Walnut query
string can never contain one (the harness asserts this before sending).

```
request   := query_id "\0" query_string "\0"
response  := query_id "\0" kind "\0" payload "\0"

query_id  := decimal ASCII, e.g. "417"
kind      := "automaton" | "true" | "false" | "error" | "fatal"
```

| `kind`      | `payload`                                                                       |
|-------------|---------------------------------------------------------------------------------|
| `automaton` | the automaton serialized by `AutomatonWriter.writeTxtFormatToStream` (`Automata/Writer/AutomatonWriter.java:48`) — the exact `.txt` format `wr-io`'s reader consumes |
| `true`      | empty. A closed formula that evaluated to the TRUE automaton — the `u11closed`/`lsdclosed` convention already used by `tests/differential/CAPTURE.md` |
| `false`     | empty. Likewise for FALSE                                                        |
| `error`     | the thrown `Exception`'s exact `getMessage()` (or its class name when the message is null) |
| `fatal`     | a JVM `Error` (OOM / StackOverflow), as `<class>: <message>`. **Not** a semantic answer — the harness records `skip-too-big` and restarts the child |

A `fatal` is a well-formed response on a still-synchronized pipe, so `JavaOracle::ask` does not
restart the child for it (unlike a timeout or a protocol fault, which it handles itself). The
**query loop** does, on every `Answer::Fatal`: the driver catches `Throwable` broadly and keeps
looping, so without that restart every later query would be answered by a JVM with a wounded
heap and a static `Session` of unknown state. The child's stderr is appended to
`<scratch>/jvm/jvm-stderr.log` across restarts, which is where an `OutOfMemoryError`'s own
trace lands.

The driver sends `eval "<query>";` — always `;`-terminated, never `::`. Detail-printing
(`::`) output is a *known-divergent* surface pending the separate `details`-logging follow-up
(`RESUME-HERE.md`), so generating it here would manufacture guaranteed-false divergences.
The generator never emits a `"` either, so the wrapper can never be escaped out of.

Two properties are load-bearing:

* **`query_id` is echoed on every response**, and the harness asserts
  `response.query_id == request.query_id` **before** treating the response as an answer (the
  check is `support::check_query_id_echo`, kept a standalone function so the mismatch branch
  has direct unit coverage — see `a_desynchronized_query_id_echo_is_a_protocol_fault`). A
  desynchronized pipe then fails loudly and immediately, instead of producing a flood of
  phantom divergences across the rest of the run.
* **stdout is flushed after every response.** This is a streaming pipe protocol; Java's
  default buffering would otherwise batch responses and deadlock the harness.

And one that is easy to miss: the driver grabs the process's **real** stdout
(`new FileOutputStream(FileDescriptor.out)`) before redirecting `System.out` into a bit
bucket. Walnut prints `____\nTRUE` on `System.out` for a closed formula — letting that reach
the pipe would corrupt every subsequent record.

## The comparison (the Rust half)

* **Automata: `wr_core::equiv` semantic language equivalence, never structural/byte identity**
  (`CLAUDE.md`'s prime directive). Two normalizations first, both inherited from
  `tests/golden`'s comparator: `sort_label()` on the port's automaton (Java's writer canonizes,
  and therefore sorts the track label, before writing — and `wr_core::equiv`'s track comparison
  is positional), and `totalize(0)` on both (Walnut `.txt` automata need not be total).
* **`true`/`false`: direct equality.**
* **Errors: EXACT string equality, with no normalization.** The port has been deliberately
  engineered to reproduce Java's exception text byte-for-byte (WB-013/WB-033/WB-034/WB-035),
  `tests/golden`'s own `compare_error` is an exact `assertEquals`, and this project's golden
  run has zero error-fixture divergences. `assertEqualMessages`-style normalization is
  reserved for `details` output, which this crate does not generate.
* **A panic in the port is a divergence, never a skip** — real Walnut throws a catchable
  exception; a process-killing panic on a plausible input is a port defect (the U26/U27
  unguarded-`act()` precedent).

## Running it

```bash
# Milestone 0: 10,000 queries, default seed.
cargo test -p wr-differential-gen --release -- --ignored --nocapture

# A short reproduction of one seed.
WR_DIFFGEN_QUERIES=200 WR_DIFFGEN_SEED=0x1234 \
  cargo test -p wr-differential-gen --release -- --ignored --nocapture milestone_0_soak

# The harness's own live self-check: it must be able to REPORT a divergence, and it must
# survive (kill, restart, resync) a JVM that does not answer in time.
cargo test -p wr-differential-gen --release -- --ignored --nocapture \
  the_harness_detects_divergence_and_survives_a_hang
```

Both are `#[ignore]`d (gated-slow tier, `docs/DESIGN.md` §5). The rest of this crate's tests —
the PRNG, the generator's well-formedness invariants, the response decoder, the comparator's
match/divergence rules, the log format — need no JVM and run in the ordinary fast tier.

### Reproducibility

The query stream is a pure function of the seed: the generator is driven by a hand-rolled
SplitMix64 with every constant written out (see `tests/support/mod.rs`), not by `rand`, whose
own docs do not promise a stable stream across minor versions. Every run writes
`target/diffgen/run-<seed>.log`, one tab-separated line per query:

```
<seed>	<index>	<query_id>	<query>	<oracle_kind>	<verdict>[	<detail>]
```

`<oracle_kind>` is what the JVM answered (`automaton`/`true`/`false`/`error`/`fatal`), so a
run's evidential weight is auditable after the fact: "10,000 match" means something very
different if 9,999 of them were `error`-vs-`error`.

so a divergence at index *N* of seed *S* replays verbatim, and an interrupted run can be
restarted from the last logged index with the same seed.

## Milestone 0 results (2026-08-15, Apple Silicon, darwin 24.6.0, `--release`)

Two independent 10,000-query runs:

| seed | match | divergence | skip-too-big | jvm restarts | wall clock | throughput |
|---|---|---|---|---|---|---|
| `0x57414c4e55545230` | 10,000 | **0** | 0 | 0 | 10.3s | 971 q/s (1.03 ms/query) |
| `0x00000000a5a5a5a5` | 10,000 | **0** | 0 | 0 | 10.2s | 978 q/s (1.02 ms/query) |

Oracle answers by kind (first run): 9,199 `automaton`, 424 `true`, 375 `false`, 2 `error` —
i.e. the bulk of the evidence really is semantic-equivalence comparisons of real automata, not
matched rejections.

That distribution is not just reported, it is **asserted**: `assert_healthy_run` fires after
the divergence check and fails the run unless it actually did the work — every query accounted
for in exactly one bucket, at least one match, ≥ 50% of the oracle's answers `automaton`,
≤ 5% skipped, zero protocol faults, zero failed restarts. Without it a degraded oracle (an
OOMing JVM, a wedged child, a stale jar) turns every query into a `skip-too-big`, leaves the
divergence list empty, and reports **PASS having compared nothing**. A failure of that gate is
a broken-harness/broken-oracle signal, not a port defect.

This **replaces the plan's unverified "~0.27ms/query" guess with a measurement**: ~1 ms/query
end-to-end for *both* engines, including the JVM round trip. Extrapolating, 10^5 queries of
this size distribution is ~2 minutes of wall clock on one shard, so the plan's sharding
machinery is a robustness feature (isolating a hang), not a throughput requirement.

Milestone 0's three questions, answered:

1. **Throughput** — ~1,000 queries/sec, as above.
2. **Does Java's static `Session` degrade over a long single-JVM lifetime?** No sign of it: the
   instantaneous rate *rises* monotonically across the run (479 q/s at query 500 → 1,010 q/s at
   query 9,500), which is JIT warm-up dominating, with no counteracting drift. One JVM served
   all 10,000 queries with zero restarts.
3. **Does the kill/restart/resync path fire correctly?** Yes — proven by
   `the_harness_detects_divergence_and_survives_a_hang`, which forces a timeout with an
   unsatisfiable deadline, asserts the child was killed and respawned, and then asserts the
   *next* two queries are answered correctly and in `query_id` sync by the fresh child. The
   same test also proves the automaton comparator can FAIL (comparing the port's `x < 4`
   against the oracle's `x < 3` must report a divergence), so a clean run is not vacuous.
