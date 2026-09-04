<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.). -->

# ct-research consumption wrapper — dispatch / plan

**Status: DONE (2026-09-04), uncommitted** — commit on the user's explicit request per the
standing git-hygiene rule. All six deliverables landed; `cargo test --workspace` green (the
new `crates/wr-cli/tests/ct_research_contract.rs` passes, 8 tests), `fmt`/`clippy`/doctest
clean, `wr-core`/`wr-logic` untouched (`git diff --stat` empty on both).

**Adversarial review outcome (one reviewer, read-only during a concurrent workspace test —
`wr-cli` is not a trust-critical crate, so one pass, not the two-reviewer math loop):** found
**one real correctness bug** and three test-strength gaps, all fixed:
- **`terminate()` (bug):** returned the original command with trailing whitespace intact in
  the already-`;`/`::`-terminated branch, so `Engine::run("eval …;\n")` — the exact strings a
  shell consumer pipes — hit `InvalidCommand` (`parse_setup` checks the raw final byte). Fixed
  to normalize in both branches; the new `facade_accepts_already_terminated_and_trailing_whitespace_commands`
  test pins it and is mutation-verified (reverting the fix fails it).
- **`eval_bool`:** now returns `None` on conflicting verdict lines instead of last-wins.
- **`state_header_count` test helper:** now mirrors `grep -cE '^[0-9]+ [0-9]+$'` byte-for-byte
  (single ASCII space, no tab/leading/trailing/`\r`), not a whitespace-tolerant token split.
- **test gap:** added the terminator/whitespace-form coverage above.
Reviewer confirmed clean: `SharedBuf` drain (no lost-output race), `dir_arg` vs `SessionPaths`,
verdict routing through the captured `out`, and the launcher/`build.sh` scripts.

**Original plan below (2026-09-04).** Built on `perf/beyond` (the top branch: 18-PR bug
fixes → idiomatic refactor → perf campaign P1–P5). User authorized "build the ct-research
consumption wrapper on the now super-fast and fully correct top-level branch"; scope decided
via `AskUserQuestion` — **both** consumption mechanisms (shell-out launcher **and** in-process
embedding facade), with all ct-research-side artifacts delivered as **ready-to-paste snippets
in walnut-rs docs** (nothing written into ct-research, per CLAUDE.md's "do not reach into it").

## What ct-research actually needs (verified empirically, not assumed)

An `Explore` agent read `../ct-research` (read-only) and the current binary was run against
the real invocation shape. Findings:

ct-research invokes Java Walnut through `bin/walnut` →
`java -Xmx8192m -jar libs/Walnut/target/Walnut-all.jar` with **cwd = a workspace dir**, feeding
commands on **stdin**. Drivers then either (a) `grep -E '^(TRUE|FALSE)$'` the stdout verdict
lines, (b) take `bin/walnut-guard`'s single normalized token (`TRUE|FALSE|TIMEOUT|EXPLODED-*|
ERROR`, with time/RSS/state watchdog *external* to the JVM), or (c) read a `def`-generated
`.txt` back from `Session/<ts>/Automata Library/<NAME>.txt`. `bin/walnut-workspace` stages a
writable mirror at `/tmp/walnut_work[.tag]`. The intended future seam (DESIGN.md §8, and
ct-research `notes/walnut-in-rust-proposal.md` §11) is a `bin/walnut-rs` sibling to `bin/walnut`
that "calls the submodule's binary." No `libs/walnut-rs` submodule or `bin/walnut-rs` exists yet.

**The engine is already a faithful behavioral drop-in** — verified against the real jar (JDK 19):

| Contract point | Verified |
| --- | --- |
| stdin REPL, `;`/`::` terminators, `eval`/`def`/`reg`/`quit`, cwd-relative libraries | ✅ |
| `eval` prints an exact `TRUE`/`FALSE` line (`grep -xE '(TRUE\|FALSE)'`) | ✅ |
| `def` writes native `.txt` to `Session/<ts>/Automata Library/` + `Result/` (matches Java; **neither** writes top-level `Automata Library/` — corrects the investigation's parenthetical) | ✅ |
| `::` detailed output line-for-line identical to Java incl. every state count and the `reachable states` lines `walnut-guard`'s `STATE_MONITOR` greps (only normalized `Xms` timings differ) | ✅ |
| exit code 0 on a decided query | ✅ |
| deterministic thread control for a shell caller: `WR_CORE_THREADS=1` (P5 default is 7, provisional per `docs/BACKLOG-D3-THREAD-TUNING.md`) | ✅ |

So this unit is **packaging + verification + docs**, not engine work. Only `wr_cli::embed`
carries any correctness surface, and it is a thin facade over already-verified entry points
(`Prover::dispatch` / `Prover::dispatch_for_integration_test`, `TestCase`,
`wr_core::set_thread_count`).

## Deliverables

1. **`bin/walnut-rs`** — shell-out launcher (the thing ct-research's `bin/walnut-rs` execs).
   Locates the built binary (`WALNUT_RS_BIN` → `target/release/walnut-rs` → `target/debug`),
   `cd`s to `$WALNUT_DIR` if set (else stays in cwd, matching Java's cwd-relative behavior),
   `exec`s it with `"$@"` and inherited stdin/env. Errors with the exact build command if no
   binary is found (mirrors `bin/walnut`'s jar-missing behavior). No auto-build by default (a
   research pipeline must not eat a multi-minute LTO build mid-experiment).
2. **`build.sh`** — `cargo build --release -p wr-cli --bin walnut-rs` helper (analog of
   Walnut's `build.sh`).
3. **`crates/wr-cli/src/embed.rs`** — curated in-process facade + a runnable doctest.
   `pub use wr_core::set_thread_count`; a small `Engine` over `Prover` exposing structured
   (`TestCase`) and text (`TRUE`/`FALSE` captured) evaluation. No new behavior; re-exports and
   thin glue only. `pub mod embed;` in `lib.rs`.
4. **`crates/wr-cli/tests/ct_research_contract.rs`** — pins the seam against regression: runs
   the **built binary** in a temp workspace, asserts exact `TRUE`/`FALSE` stdout and
   `def`→session-tree `.txt` placement + `grep -cE '^[0-9]+ [0-9]+$'` state-count readability;
   exercises the `embed` facade the same way. Written to FAIL if the contract breaks (guard
   against the repo's recurring "vacuous test" finding — each assertion mutation-checked).
5. **`docs/CT-RESEARCH-INTEGRATION.md`** — consumer guide: `.gitmodules` + `git submodule add`
   lines; the ready-to-paste ct-research `bin/walnut-rs`; how `bin/walnut-guard` works unchanged
   (process-external watchdog); the embedding-facade Cargo path-dep + code example; env knobs
   (`WR_CORE_THREADS`, `WALNUT_DIR`, `WALNUT_RS_BIN`); and the verified-contract table above.
6. **README refresh** — the current README falsely says "scaffold / Phase 0 … nothing is
   implemented yet." Consumers read it. Update status + layout (crate names, `wr-numsys` is
   inside `wr-core`) to reality.

## Process

- Semantic/cross-engine fidelity is already proven (golden 670/675, differential-gen, fuzz);
  the **new** risk is the I/O contract (stdout shape, file placement) and a vacuous compat
  test. So the compat test is self-contained (Rust-driving-the-binary), and each assertion is
  mutation-verified.
- `wr-core`/`wr-logic` are untouched (`git diff --stat` on them must stay empty). `wr-cli`'s
  public API grows only by `embed`. Per the merge gate, `wr-cli` is not a trust-critical crate,
  but `embed.rs` + the compat test will still go through an adversarial-reviewer pass (the
  facade is public API; the test is correctness-load-bearing).
- Gates before "done": `cargo test --workspace` green; `fmt`/`clippy` clean; the launcher
  exercised end-to-end; docs' paste-in snippets dry-checked.
- **No commit/push without the user's explicit request** (standing git-hygiene rule).
