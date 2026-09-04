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

Memory/time/state limits are enforced **externally** by the caller's watchdog, exactly
as ct-research's `bin/walnut-guard` already does around the JVM — no `-Xmx` analog is
needed, and the watchdog works unchanged against this process.

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

---

## Mechanism 2 — in-process embedding

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
- `Engine` sends the detailed (`::`) log to a sink; for the byte-identical detailed log
  (e.g. a state-count watchdog), use the shell-out binary instead.
- `wr-cli` deliberately does **not** set a `#[global_allocator]`; only the `walnut-rs`
  binary does (`mimalloc`). An embedder picks its own allocator.

---

## Licensing

walnut-rs is GPLv3-or-later, contained in its own repo. Consuming it as a submodule
(shell-out or library link) does not entangle ct-research's own licensing beyond the
usual GPL obligations for the linked/invoked component. See `LICENSE` / `NOTICE`.
