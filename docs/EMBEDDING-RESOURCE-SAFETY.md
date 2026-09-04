<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.). -->

# Resource safety when embedding walnut-rs (READ BEFORE running a query in-process)

**Audience:** anything that consumes walnut-rs — a human, or an automated agent in a
downstream repo (e.g. ct-research). This is a hard requirement, not advice. If you run a
walnut-rs query, you must build in the protection described here. You are free to embed the
library in-process rather than shelling out to `bin/walnut-rs` — but embedding does **not**
excuse you from the protection; it *raises* the bar, for the reason below.

## The threat

walnut-rs decides first-order logic over automatic sequences. That decision procedure is
worst-case **superexponential** — a single innocent-looking query (a deep quantifier
alternation, a large base, a state-exploding intermediate automaton) can try to build an
automaton with millions to billions of states. Walnut has the same blowup; the JVM contains
it with a per-process heap ceiling (`java -Xmx8192m …`), so a runaway query dies with an
`OutOfMemoryError` instead of exhausting the host's RAM and freezing the machine.

**walnut-rs is a native binary/library; its `-Xmx` analog is opt-in and must be switched
on.** Left unset, a runaway query allocates until the OS OOM-kills the process — or, worse,
until the machine swap-thrashes to a halt and takes everything else on it down with it. The
in-engine budget (`wr_core::resource`, 2026-09; `WR_MAX_STATES` / `WR_MAX_BYTES` for the
binary, `ResourceBudget` for an embedder — see §0 below) caps the state count of any
automaton under construction and the process's live heap, checked at every state insertion
inside `determinize` / `product` / `quantify` / `minimize`, and turns a breach into a clean
`EXPLODED-states` / `EXPLODED-mem` error with the partial automata freed. It bounds **no**
wall-clock time, and it does nothing unless you set it. Do not rely on the engine to stop
itself unless you have configured it to.

## Why in-process is *harder* to protect than a subprocess

A walnut-rs query runs synchronously on the calling thread, and the core has no cooperative
"check the budget" points you can interrupt at. **Rust has no safe way to kill a running
thread.** So if you call the engine directly on a thread of your own process:

- a watchdog thread that notices "this is taking too long / using too much RAM" **cannot stop
  the worker** — it keeps running and keeps allocating;
- there is no point at which you can reclaim the memory it has taken short of tearing down the
  whole process;
- by the time you decide to abort, the host may already be swap-thrashing.

This is the crux: **an in-process query you cannot bound in advance is a query you cannot make
safe.** The only mechanism that can actually stop a runaway walnut-rs computation and reclaim
its memory is the operating system killing the **process** it runs in.

## What you MUST do

Classify every query before you run it, and protect accordingly.

### 0. Always set the in-engine budget — it is the only thing that catches a fast spike

The external watchdogs below sample; a state explosion can go from "fine" to "OS-killed"
between two samples (observed: `motp7_rec` killed at a peak a 3 s RSS sampler never saw).
The in-engine budget is checked **per inserted state**, on the constructing thread, so it
cannot be outrun. Set it in addition to — never instead of — the process-level protection:

- **Shell-out:** `WR_MAX_STATES=<n>` and `WR_MAX_BYTES=<n>[K|M|G]` in the child's
  environment (`bin/walnut-rs` passes the environment through). The binary installs the
  tracking allocator `WR_MAX_BYTES` needs. A breach prints one line containing
  `EXPLODED-states: …` / `EXPLODED-mem: …` on stdout (after the `[Walnut]$ ` prompt, like
  every Walnut error message — match it with `grep -o 'EXPLODED-[a-z]*'`), the command's
  memory is freed, and the REPL reads the next command; exit code stays 0. A malformed value
  is a startup error (exit 1), never silently ignored.
- **In-process:** `Engine::builder(dir).budget(ResourceBudget { max_states: Some(n),
  max_bytes: Some(bytes) })` (or `engine.set_budget(..)`). A breach makes that command return
  `Err(ProverError::ResourceExhausted(Exhausted { reason, operation, at, limit }))`; the
  `Engine` stays usable. `max_bytes` counts **live heap bytes process-wide** (the same
  quantity `-Xmx` bounds), and needs a tracking global allocator in *your* binary:

  ```rust
  #[global_allocator]
  static GLOBAL: wr_cli::tracking_alloc::TrackingAllocator<std::alloc::System> =
      wr_cli::tracking_alloc::TrackingAllocator(std::alloc::System);
  ```

  Without it a `max_bytes` cap is **refused** (`MemoryMeterMissing`), not silently skipped —
  a state-only budget still works. Size the byte cap below the host's real RAM, and remember
  it includes your own process's live data.

What the budget does **not** do: bound wall-clock time (a query can run for hours inside both
caps), or interrupt anything between check points inside a single primitive — the check
points are per state, so in practice a breach lands within one metastate's out-degree of the
cap, but the process-level cap in point 1 remains the backstop for everything this cannot
see (a third-party allocation, a hang).

### 1. Any query that is not *provably* small → run it in an isolated, resource-capped child process

"Embedding" does not have to mean "the shipped `bin/walnut-rs`." You may write your own small
runner binary that links `wr-cli` and calls `wr_cli::embed::Engine` — that is fully supported.
But run that runner (or `bin/walnut-rs`) as a **separate process** that you can cap and kill,
so the OS is the backstop. Concretely, the child must have all three of:

- **A hard address-space / memory cap — the real `-Xmx` analog.**
  - **Linux / Linux containers (ct-research's docker path):** the strong, *instantaneous*
    option. Either `setrlimit(RLIMIT_AS, …)` in the child before it runs the query (allocation
    then fails and the child aborts cleanly; the parent is untouched), or a cgroup / container
    memory limit (`docker run --memory=…`, a systemd `MemoryMax=`, or a v2 `memory.max`). Size
    it below the host's real RAM so the OOM lands on the child, not the machine.
  - **macOS:** `RLIMIT_AS` is **not reliably enforced** — do not trust it. Fall back to the
    RSS-sampling watchdog in point 3, which is what actually protects a Mac.
- **A CPU-time cap.** `setrlimit(RLIMIT_CPU, …)` (SIGXCPU when exceeded) bounds runaway
  compute even when memory stays flat.
- **A wall-clock watchdog that kills the child on breach.** Wall time, not just CPU time (a
  swapping process burns wall clock without burning much CPU).

The already-built, reusable version of all of this is ct-research's `bin/walnut-guard`: it
samples wall time + RSS (`ps -o rss=`) + optional state count and kills the child, normalizing
the outcome to `TRUE | FALSE | TIMEOUT | EXPLODED-mem | EXPLODED-states | ERROR`. It is
**process-external and engine-agnostic**, so it works against a walnut-rs child exactly as it
works against the JVM — point it at `bin/walnut-rs` (or your own runner) and you are done.
See `docs/CT-RESEARCH-INTEGRATION.md`.

### 2. State-count watchdog (optional, for early blow-up detection)

walnut-rs's `::` (detailed) output emits `<N> reachable states` lines byte-identically to the
JVM engine, so a `STATE_MONITOR`-style guard that greps those and kills on a threshold works
unchanged. Use it when you want to catch a blow-up *before* it exhausts memory rather than
after.

### 3. Only run a query directly in-process (parent thread) when it is provably bounded

Direct `wr_cli::embed::Engine` use on your own process's thread is appropriate **only** for
queries whose size you control and know to be small: your own hand-authored fixtures, small
bases, shallow quantifier alternation, automata of at most a few hundred states — **and always
with a `ResourceBudget` set (§0)**, which turns the one failure mode this section is about (a
blow-up you cannot interrupt) into an `Err` you get back with the memory already freed. If you
cannot state a concrete bound, treat it as unbounded and go through point 1. When you do run
in-process directly, still wrap it in a wall-clock check so a runaway-but-within-budget query
is *noticed* (noticing cannot stop it — it tells you to fix the query or move it to a
subprocess).

### 4. Reproducibility (not a safety control, but do it anyway)

Pin the parallel degree: `WR_CORE_THREADS=1` (shell) or `wr_cli::embed::set_thread_count(1)`
before the first query (in-process). walnut-rs is bit-identical at every thread count; this
just removes a nondeterminism source from research runs. It does **not** bound memory.

## What NOT to do

- **Do not** call the engine on a thread and assume a timeout thread can rescue you — it
  cannot stop the worker or free its memory.
- **Do not** rely on `-Xmx`-style intuition: the heap ceiling exists only if you set
  `WR_MAX_BYTES` / `ResourceBudget::max_bytes` (and, in-process, install the tracking
  allocator). Unset, there is none.
- **Do not** trust `RLIMIT_AS` on macOS; use RSS sampling there.
- **Do not** run an unclassified or externally-supplied query directly in-process. Isolate it.

## The one-line rule

> Always set the in-engine budget (`WR_MAX_STATES`/`WR_MAX_BYTES`, or `ResourceBudget`).
> If you cannot prove the query is small, run it in a child process under a memory cap
> (`RLIMIT_AS` / cgroup on Linux, RSS-sampling watchdog on macOS) **and** a wall-clock
> watchdog that kills it — `bin/walnut-guard` already does all of this. Only provably-small,
> budgeted queries may run directly in-process.
