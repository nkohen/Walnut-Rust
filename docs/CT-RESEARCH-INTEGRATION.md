<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.). -->

# Consuming walnut-rs from ct-research

walnut-rs is a behavioral drop-in for the JVM Walnut engine. This guide is for the
downstream consumer (`ct-research`) that vendors walnut-rs as a pinned submodule and
wants to run queries through it — as a **shell-out binary** (exactly how it runs the
Java jar today) or **in-process** (linking the Rust library).

Everything a consumer needs lives here; nothing is written into ct-research. The
snippets below are ready to paste into ct-research when you deliberately wire it up.

The verified contract (checked against the real jar, JDK 19, and pinned by
`crates/wr-cli/tests/ct_research_contract.rs`):

| Behavior | walnut-rs | matches JVM Walnut |
| --- | --- | --- |
| stdin REPL; `;` quiet / `::` detailed; `eval`/`def`/`reg`/`quit`; libraries resolved relative to cwd | yes | yes |
| `eval` of a closed formula prints a bare `TRUE`/`FALSE` line (after a `____` prompt-erase line) — `grep -xE '(TRUE\|FALSE)'` matches it | yes | yes |
| `def NAME "…"` writes native `NAME.txt` into `Session/<timestamp>/Automata Library/` (and `Result/`); `grep -cE '^[0-9]+ [0-9]+$'` counts its states | yes | yes (neither writes the top-level `Automata Library/`) |
| `::` detailed log is line-for-line identical (state counts, `N reachable states`), only `Xms` timings differ | yes | yes |
| exit code 0 on a decided query | yes | yes |

---

## Build

```bash
cd libs/walnut-rs
./build.sh                       # cargo build --release -p wr-cli --bin walnut-rs
# produces libs/walnut-rs/target/release/walnut-rs
```

The release profile uses fat LTO + `codegen-units=1`, so a from-scratch build is slow
but yields the engine the perf campaign measured. Build once; `bin/walnut-rs` reuses it.

---

## Mechanism 1 — shell-out drop-in (recommended, matches today's usage)

`libs/walnut-rs/bin/walnut-rs` is the launcher, the analog of ct-research's `bin/walnut`:
it locates the built binary and `exec`s it with cwd = `$WALNUT_DIR` (or the current dir),
passing stdin/args/env straight through.

Environment knobs:

| Var | Effect |
| --- | --- |
| `WALNUT_DIR` | Workspace holding the library dirs; the launcher `cd`s here first. Default: cwd. |
| `WALNUT_RS_BIN` | Explicit binary path (overrides discovery). |
| `WR_CORE_THREADS` | Parallel degree. `WR_CORE_THREADS=1` = deterministic, single-threaded (recommended for reproducible research runs). Default is a small auto-picked value. |
| `WR_MAX_STATES` | In-engine cap on the state count of any single automaton under construction. Unset = unlimited. See "Observability and resource budgets". |
| `WR_MAX_BYTES` | In-engine cap on live heap bytes (`K`/`M`/`G` suffix allowed) — the `-Xmx` analog. Unset = unlimited. |

Time limits are enforced **externally** by the caller's watchdog, exactly as
ct-research's `bin/walnut-guard` already does around the JVM; the watchdog works unchanged
against this process. State/memory limits can additionally be enforced **inside** the
engine (`WR_MAX_STATES`/`WR_MAX_BYTES`), which — unlike a sampling watchdog — cannot miss a
fast spike; see below.

### ct-research wiring (paste-in)

1. Add the submodule (`.gitmodules` gets this entry automatically):

   ```bash
   git submodule add <walnut-rs-remote-url> libs/walnut-rs
   git -C libs/walnut-rs checkout <pinned-commit>
   git add .gitmodules libs/walnut-rs
   ```

   Resulting `.gitmodules` stanza:

   ```
   [submodule "libs/walnut-rs"]
   	path = libs/walnut-rs
   	url = <walnut-rs-remote-url>
   ```

2. `bin/walnut-rs` in ct-research — a sibling to `bin/walnut` that calls the submodule's
   launcher (and honors the same `WALNUT_DIR` the drivers already export):

   ```bash
   #!/bin/bash
   # Run walnut-rs (the Rust engine) as a drop-in for bin/walnut.
   set -euo pipefail
   REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
   BIN="$REPO/libs/walnut-rs/target/release/walnut-rs"
   if [[ ! -x "$BIN" ]]; then
     echo "walnut-rs not built. Run: (cd libs/walnut-rs && ./build.sh)" >&2
     exit 1
   fi
   # The submodule launcher cd's to $WALNUT_DIR (if set) and execs the binary.
   exec "$REPO/libs/walnut-rs/bin/walnut-rs" "$@"
   ```

   `chmod +x bin/walnut-rs`.

3. Existing drivers switch engine by changing **one word** — the wrapper they invoke:

   ```bash
   # before:  WOUT="$("$REPO/bin/walnut"    < "$HERE/walnut_commands.txt" 2>&1)"
   # after:   WOUT="$("$REPO/bin/walnut-rs" < "$HERE/walnut_commands.txt" 2>&1)"
   ```

   The `grep -E '^(TRUE|FALSE)$'` verdict parsing and the
   `ls -t Session/*/Automata\ Library/NAME.txt` read-back are unchanged.

4. `bin/walnut-workspace` can be reused as-is — its `/tmp/walnut_work[.tag]` mirror holds
   the library dirs walnut-rs needs (the extra `target/`/`src` symlinks are harmless). Or
   stage a minimal workspace with just the library subdirectories.

### Resource-guarded single queries (`walnut-guard`)

`bin/walnut-guard`'s watchdog is process-external (it samples wall time / RSS / state
count and kills the JVM), so it ports by copying it to `bin/walnut-rs-guard` and swapping
only the launch line:

```bash
# in the guard, replace the JVM launch:
#   "$JAVA_HOME/bin/java" -Xmx${XMX_MB}m -jar "$JAR" >"$raw" 2>&1 < <(printf '%s' "$cmd") &
# with the Rust launcher (WALNUT_DIR already exported by the caller):
"$REPO/bin/walnut-rs" >"$raw" 2>&1 < <(printf '%s' "$cmd") &
```

The RSS/time/state sampling, the `TRUE|FALSE|TIMEOUT|EXPLODED-*|ERROR` normalization, and
the `grep -oE '[0-9]+ reachable states'` STATE_MONITOR (fed by `::` mode) all work
unchanged — walnut-rs's `::` output carries the same `N reachable states` lines.

Add the in-engine budget to the launch environment so a blow-up the sampler would miss
still yields a clean verdict, and let the engine's own verdict win when it fires:

```bash
# alongside the RSS/time sampler: the engine caps itself, per inserted state
WR_MAX_STATES="${STATE_LIMIT:-2000000}" WR_MAX_BYTES="${XMX_MB}M" \
  "$REPO/bin/walnut-rs" >"$raw" 2>&1 < <(printf '%s' "$cmd") &
# ...
# in the normalizer, before the TRUE/FALSE grep:
if v=$(grep -oE 'EXPLODED-(states|mem)' "$raw" | head -1); then echo "$v"; exit 0; fi
```

The engine prints the verdict line on stdout after the `[Walnut]$ ` prompt (like every
Walnut error message), frees the command's memory, keeps reading the next command, and
exits 0 — so `grep -o` (not an anchored match) finds it, and a script's later commands
still run.

---

## Observability and resource budgets (both mechanisms)

Added 2026-09 for the research lines in ct-research's feature request; all opt-in and
inert when unused, so results stay bit-identical.

| Need | Shell-out | In-process |
| --- | --- | --- |
| **Real vs. transient explosion** — did the subset construction's peak exceed the minimized output? | `::` mode: the `Determinizing`/`Minimizing:`/`Minimized:` lines | `engine.record_trajectory(true)`; after a command, `engine.trajectory().unwrap().determinizations()` gives one `DeterminizationRecord { input_states, levels, peak_states, minimized }` per subset construction; `.peak_states()` / `.events()` for the per-level `SubsetLevel { level, frontier, members, metastates }` trajectory |
| **Clean exhaustion instead of an OS kill** | `WR_MAX_STATES` / `WR_MAX_BYTES` → an `EXPLODED-states:` / `EXPLODED-mem:` line | `Engine::builder(dir).budget(ResourceBudget { max_states, max_bytes })` (or `set_budget`) → `Err(ProverError::ResourceExhausted(Exhausted { reason, operation, at, limit }))` |
| **The `::` detailed log without shelling out** | n/a | `engine.detailed_log()` after a `::`-suffixed command (byte-identical to the binary's lines), or route the console with `Engine::builder(dir).console(Box::new(sink))` |

```rust
use wr_cli::embed::resource::{ResourceBudget, Trajectory};
use wr_cli::embed::Engine;
use wr_cli::prover::ProverError;

let mut engine = Engine::builder("/path/to/workspace")
    .budget(ResourceBudget { max_states: Some(2_000_000), max_bytes: None })
    .build()?;
engine.record_trajectory(true)?;

match engine.eval_bool(r#"eval q "?msd_2 Ax Ey (y > x)""#) {
    Ok(verdict) => {
        let t: Trajectory = engine.trajectory().unwrap();
        for d in t.determinizations() {
            // peak >> minimized  =>  transient; peak ~ minimized  =>  real
            println!("{} -> {} (min {:?})", d.input_states, d.peak_states, d.minimized);
        }
        println!("peak states this command: {}", t.peak_states());
    }
    Err(ProverError::ResourceExhausted(e)) => println!("{}", e.verdict()), // EXPLODED-states
    Err(e) => return Err(e.into()),
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`max_bytes` counts live heap bytes process-wide through a tracking global allocator. The
`walnut-rs` binary installs one; an embedder installs its own (any allocator can be wrapped):

```rust
#[global_allocator]
static GLOBAL: wr_cli::tracking_alloc::TrackingAllocator<std::alloc::System> =
    wr_cli::tracking_alloc::TrackingAllocator(std::alloc::System);
```

Without it, a `max_bytes` cap is refused (`MemoryMeterMissing`) rather than silently
ignored; a state-only cap needs nothing. Neither cap bounds wall-clock time — keep the
external watchdog (`docs/EMBEDDING-RESOURCE-SAFETY.md`). The cost of the allocator wrapper
in the shipped binary is one relaxed atomic add per allocation; see the measurement note in
`docs/EMBEDDING-RESOURCE-SAFETY.md`'s companion section of `CLAUDE.md`'s status log.

For a direct `wr-core` user (no `Engine`): `wr_core::resource::run(&Instrumentation::new()
.with_budget(..).with_observer(rc_refcell_trajectory), || { ... })` brackets any code that
calls the primitives.

---

## Mechanism 2 — in-process embedding

> **Resource safety — required reading: [`EMBEDDING-RESOURCE-SAFETY.md`](EMBEDDING-RESOURCE-SAFETY.md).**
> The decision procedure is worst-case superexponential and walnut-rs has **no `-Xmx`-style
> memory ceiling**. A query run directly on your own thread **cannot be interrupted** (Rust
> cannot safely kill a running thread), so a blow-up can freeze the host. Only run
> provably-small queries directly in-process; isolate anything else in a child process under a
> memory cap **and** a wall-clock watchdog (e.g. `bin/walnut-guard`). Read the doc before
> embedding.

Link `wr-cli` directly and drive the engine with `wr_cli::embed::Engine` — no subprocess,
no stdin pipe, no `.txt` round-trip.

`Cargo.toml` (path into the submodule):

```toml
[dependencies]
wr-cli = { path = "libs/walnut-rs/crates/wr-cli" }
```

```rust
use wr_cli::embed::{Engine, set_thread_count};

// Optional: pin the parallel degree for bit-reproducible runs. Must precede the
// first query; the shell path's WR_CORE_THREADS=1 is the equivalent.
let _ = set_thread_count(1);

// A workspace holding "Automata Library/", "Word Automata Library/", etc.
let mut engine = Engine::new("/path/to/workspace")?;

// Closed formula -> a TRUE/FALSE verdict.
let verdict: Option<bool> = engine.eval_bool(r#"eval q "?lsd_2 An (n >= 1 => Ex x < n)""#)?;

// `def` builds and saves a named automaton, visible to later commands on this engine.
engine.run(r#"def lt "?msd_2 x < y""#)?;

// Structured result (the automaton object) instead of console text.
let tc = engine.eval_structured(r#"eval a "?msd_2 x < y""#)?; // Option<TestCase>
# Ok::<(), Box<dyn std::error::Error>>(())
```

Notes:

- `Engine::run` returns exactly the REPL's console text for the command (a closed `eval`
  is `"____\nTRUE\n"`); `eval_bool` parses it with the same per-line semantics as the
  shell `grep`. Commands may omit the trailing `;` — the facade adds it.
- `Engine::run` has full file-writing side effects (a `def` saves under the session tree).
  `Engine::eval_structured` returns the automaton in memory.
- `Engine` sends the detailed (`::`) log to a sink by default; read it back with
  `engine.detailed_log()` after a `::`-suffixed command, or route it live with
  `Engine::builder(dir).console(..)`. Structured per-operation counts:
  `engine.record_trajectory(true)` — see "Observability and resource budgets".
- `wr-cli` deliberately does **not** set a `#[global_allocator]`; only the `walnut-rs`
  binary does (`mimalloc`). An embedder picks its own allocator.

---

## Licensing

walnut-rs is GPLv3-or-later, contained in its own repo. Consuming it as a submodule
(shell-out or library link) does not entangle ct-research's own licensing beyond the
usual GPL obligations for the linked/invoked component. See `LICENSE` / `NOTICE`.
