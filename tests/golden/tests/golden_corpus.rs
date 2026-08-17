// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **Tier 1 — the golden-corpus harness** (`docs/DESIGN.md` §5; Phase 3b's U27, the actual
//! Phase-3 exit criterion "Tier 1 green; eval/def/reg, and now everything else, work").
//!
//! This replays Walnut's own recorded integration-test corpus — the 675 command scripts
//! `walnut-java/src/test/java/Main/IntegrationTest.java` holds in its `L` list, filtered to
//! the 591 that `phase0-artifacts/subset-filter.json` marks subset-relevant — through this
//! port's real dispatch loop, and compares each result against the output real Walnut
//! recorded for it.
//!
//! # Tier 1 is not Tier 3
//!
//! `tests/differential/` (crate `wr-differential`) is **Tier 3**: queries this project chose,
//! captured from a live `walnut-java` CLI run, checked in as fixtures. This crate is
//! **Tier 1**: Walnut's *own* fixtures, its own recorded expectations, replayed wholesale. The
//! two share a comparison philosophy (semantic equivalence, never bytes) and some scaffolding
//! shape, but not their inputs, their scale, or their failure meaning — a Tier-1 divergence
//! says "the port does not reproduce a workload Walnut itself ships a golden answer for."
//!
//! # What is compared, and how (a port of `IntegrationTest.runSpecificTest`, `:920-966`)
//!
//! Field for field, in Java's own order:
//!
//! | field | Java | here |
//! |---|---|---|
//! | thrown exception | `assertEquals(expected.getError(), e.getMessage())` | exact string equality on `ProverError`'s `Display`, **no normalization** |
//! | `getMatrixOutput()` | size + `assertEqualMessages` per entry | same, except the 7 CAS fixtures (below) |
//! | `getGraphViz()` | `assertEqualMessages`, only when the expected file is non-empty | same |
//! | `getDetails()` | `assertEqualMessages` | same — see below, this is the strict one |
//! | `getAutomatonPairs()` | size + `EqualityUtils.faEqual(actual.fa, expected.fa)` | size + [`wr_core::equiv::automaton_language_equivalent`] |
//!
//! `assertEqualMessages`' normalization is ported exactly in [`support::normalize_message`]
//! (strip; one non-overlapping doubled-space pass; drop `\d+ms`; drop `\s*Progress:.*`) and
//! nothing else. **Every state count survives, including pre-minimization ones** — a
//! `details` fixture therefore pins intermediate automaton sizes and the cross-product
//! traversal shape exactly. That strictness is deliberate: a `details` divergence on a
//! fixture whose command runs `wr_core::product`'s BFS is a correctness signal about that
//! traversal, not a normalization gap to paper over.
//!
//! Automata are compared by **semantic language equivalence only** — `CLAUDE.md`'s prime
//! directive #1. State numbering, canonicalization order and file bytes are all free to
//! differ.
//!
//! # Session model
//!
//! The corpus is **stateful and ordered**: fixture 448 consumes automata fixtures 444-447
//! built, fixture 534 compares two automata earlier fixtures defined, and so on. Java runs
//! every fixture against one shared on-disk session (a fresh `Prover` per fixture, but the
//! same `Session` statics), seeded by `IntegrationTest.initialize()`'s prelude. This harness
//! does the same:
//!
//! * `Global/` is copied to a temp directory and used as the home (read-only seed) library;
//! * a fresh `Session/` is created with the six library subdirectories and the five word
//!   automata `Session.cleanPathsAndNamesIntegrationTest` explicitly keeps;
//! * [`support::PRELUDE`]'s 19 commands run first, on a fresh `Prover` each;
//! * then all 675 fixtures run **in id order**, each on a fresh `Prover`.
//!
//! Fixtures that are excluded from *assertion* are still **executed**, exactly as Java runs
//! them, so the session state every later fixture depends on is the same one Walnut recorded
//! against. Excluding a fixture here means "do not compare its result", never "do not run it".
//!
//! # Exclusions (each recorded per-id in the report, never silent)
//!
//! 1. `subset-filter.json`'s 84 DROP-scope fixtures (negative-base numeration; `split`/
//!    `rsplit`/`ost`).
//! 2. The deferred **OTF** determinization strategies `CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`
//!    (`docs/DESIGN.md` §9/§10 — an already-decided deferral). `SC` and plain `BRZ` are in
//!    scope and are compared normally.
//! 3. **Transitive DROP dependency** — a subset-relevant fixture whose command consumes a
//!    library name that only a DROP-scope fixture could have produced. Computed structurally
//!    from the manifest ([`support::transitively_dropped`]), not hard-coded.
//!
//! (There used to be a fourth: `[strategy …]`/`[export …]` metacommands, parsed but not
//! threaded into determinization. That is now wired through the `eval`/`def` path — see
//! `crates/wr-cli/src/prover.rs`'s module docs — so fixtures 637, 659 and 660 are compared
//! normally. 638-641 remain excluded, but under (2): they ask for OTF strategies.)
//!
//! Plus one *partial* exclusion: the fixtures whose recorded expectation includes CAS
//! incidence-matrix files (`.mpl`/`.m`/`.wl`/`.sage`). The CAS matrix writer is confirmed
//! DROP scope for this port, so those fixtures have their automaton/details/error compared
//! and **only** their matrix comparison skipped — reported per id as `cas-matrix-skipped`.
//!
//! # Resource discipline — an ENFORCED per-fixture cap
//!
//! `CLAUDE.md`'s test-performance guardrail is "per-test resource caps, **never hangs**", so
//! [`MAX_FIXTURE_SECS`] is enforced, not merely measured afterwards: every fixture's whole
//! body — build the `Prover`, dispatch, compare — runs on a worker thread
//! ([`run_with_timeout`]), and the harness thread waits on a channel with a timeout rather
//! than joining. A fixture that does not answer inside the cap is recorded `TIMEOUT`.
//!
//! `Prover`/`Session` hold `Rc` state and are **not** `Send`, which is why nothing but plain
//! data crosses the thread boundary: the worker is handed `String` paths + the fixture's
//! recorded [`Expected`], constructs its own `Prover` inside the thread, and sends back only
//! the [`Verdict`] and its notes.
//!
//! **The known tradeoff, and what the harness does about it:** Rust has no thread-kill
//! primitive, so a timed-out worker is *abandoned*, not stopped. It keeps running — and may
//! keep writing into the shared on-disk session tree that every later fixture builds its own
//! `Prover` against. A verdict computed while an unbounded, unkillable writer is mutating that
//! tree is not evidence about the port: at best it is a `PASS`/`FAIL` nobody can trust, at
//! worst it is a false `PASS` off a torn write. So the run **halts** on the first timeout: the
//! remaining fixtures are neither dispatched nor compared, and each is recorded [`Verdict::NotRun`]
//! naming the fixture that poisoned the tree ([`Halt`]). The same halt covers a prelude command
//! that times out, and a run whose *total* exceeds [`MAX_TOTAL_SECS`].
//!
//! That is still strictly better than the alternative it replaces (the whole harness hanging
//! forever), and it keeps the report honest: one `TIMEOUT` plus N explicit `NOT-RUN`s, rather
//! than one `TIMEOUT` plus N unmarked verdicts that raced an abandoned writer. Both verdicts
//! fail the gate — a timeout at all is a port regression to fix, not a state to run in.
//!
//! # Running it
//!
//! The corpus test is `#[ignore]`d — it is `docs/DESIGN.md` §5's **gated slow tier**; see
//! [`tier1_golden_corpus`]'s own doc and `tests/golden/STATUS.md`.
//!
//! ```bash
//! cargo test -p wr-golden --release -- --ignored --nocapture   # the whole corpus
//! cargo test -p wr-golden                                      # the cheap checks only
//! WR_GOLDEN_ONLY=0,1,375,653 cargo test -p wr-golden --release -- --ignored --nocapture
//! WR_GOLDEN_STOP_AFTER=100  ...   # stop after fixture 100 (triage; disables the gate)
//! WR_GOLDEN_KEEP_TREE=1     ...   # leave the temp library tree behind for inspection
//! WALNUT_JAVA_DIR=/path/to/walnut-java cargo test -p wr-golden --release -- --ignored
//! ```
//!
//! The full per-fixture report is also written to `target/golden-corpus-report.txt`, and
//! `tests/golden/STATUS.md` holds the checked-in summary of the last full run (pass/fail/skip
//! counts, every remaining divergence and its root cause, and the bugs this unit fixed).

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::time::{Duration, Instant};

use support::{
    build_session_tree, compare_messages, corpus_root, deferred_otf_strategy, global_library_names,
    load_expected, load_fixtures, transitively_dropped, walnut_java_dir, Excluded, Expected,
    Fixture, PathRewrite, PRELUDE,
};
use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_cli::test_case::TestCase;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

/// Per-fixture wall-time cap (`CLAUDE.md`, test-performance guardrails), **enforced** by
/// [`run_with_timeout`] rather than measured afterwards. Chosen from a measured full run: see
/// `tests/golden/STATUS.md` for the observed distribution.
const MAX_FIXTURE_SECS: u64 = 60;

/// Whole-run wall-time cap. The corpus is Walnut's own workload set, so it is inherently
/// tractable — but a port bug that turns one fixture superexponential must not be able to
/// wedge `cargo test --workspace` indefinitely.
const MAX_TOTAL_SECS: u64 = 1800;

/// Stack for a fixture's worker thread. Deliberately generous: `libtest` gives its own test
/// threads 8 MiB while `thread::spawn`'s default is 2 MiB, and this port's regex/automaton
/// code recurses, so a plain `spawn` would trade a hang for a stack overflow on exactly the
/// deep fixtures the cap exists for. (Virtual reservation — it costs nothing untouched.)
const WORKER_STACK_BYTES: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Verdicts
// ---------------------------------------------------------------------------

/// Which comparison a failure reason came from.
///
/// This exists for one reason: [`KNOWN_DIVERGENCES`]' whole legitimacy rests on the claim
/// that every entry in it is a **text-only** divergence whose automaton already matches.
/// Matching a known entry by fixture id alone would let a future regression break the
/// AUTOMATON comparison for one of those ids and still report green — the id is in the
/// list, so the gate would wave it through. Tagging each reason turns "these are text-only"
/// from a one-time manual observation into an invariant the gate enforces on every run
/// (see [`Verdict::is_text_only_failure`] and its use at the gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FailedHalf {
    /// A TEXT comparison: `details`, the error message, matrix output, graphviz. The only
    /// half a [`KNOWN_DIVERGENCES`] entry may excuse.
    Text,
    /// The `wr_core::equiv` automaton comparison — **including** the cases where it could
    /// not be run at all: an unreadable recorded automaton, a missing/extra automaton, the
    /// wrong number of pairs, the command **erroring** where the corpus records a successful
    /// result, and the command **succeeding** where the corpus records an error (so the
    /// corpus recorded no automaton to compare against). "Did not run" is not "passed", so
    /// those are tagged here deliberately rather than as [`FailedHalf::Text`] — tagging them
    /// `Text` is exactly how a listed fixture that stopped producing any automaton at all
    /// could be waved through with zero automaton evidence.
    Automaton,
    /// Neither comparison: the harness itself, the command's own dispatch, or a port panic.
    /// Never excusable by a known entry.
    Harness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Pass,
    /// Every reason collected for this fixture, each tagged with the comparison it came
    /// from. More than one entry means [`fold_failures`] combined a text mismatch with an
    /// automaton one — which is exactly the case the gate must never treat as "known".
    Fail(Vec<(FailedHalf, String)>),
    Skipped(Excluded),
    /// The fixture did not answer within [`MAX_FIXTURE_SECS`] and its worker thread was
    /// abandoned (carries the cap, not a measured time — there is no measured time, that is
    /// the point). Counts as a failure at the gate, and [`Halt`]s the rest of the run.
    Timeout(Duration),
    /// The fixture was never attempted: the run had already halted (see [`Halt`]). Counts as a
    /// failure at the gate — a corpus run that did not finish is not a green corpus run.
    NotRun,
}

impl Verdict {
    fn tag(&self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail(_) => "FAIL",
            Verdict::Skipped(_) => "SKIP",
            Verdict::Timeout(_) => "TIMEOUT",
            Verdict::NotRun => "NOT-RUN",
        }
    }

    /// One failure reason, tagged with the comparison it came from.
    fn fail(half: FailedHalf, why: impl Into<String>) -> Verdict {
        Verdict::Fail(vec![(half, why.into())])
    }

    /// The reasons as one human-readable block — what the report and the gate print.
    fn message(&self) -> String {
        match self {
            Verdict::Fail(reasons) => reasons
                .iter()
                .map(|(_, why)| why.as_str())
                .collect::<Vec<_>>()
                .join("\n  --- and ---\n"),
            _ => String::new(),
        }
    }

    /// Whether EVERY collected reason is a [`FailedHalf::Text`] one — i.e. the automaton
    /// comparison ran and matched, and the harness/dispatch were fine.
    ///
    /// A [`KNOWN_DIVERGENCES`] entry is honored only when this is true. A `Timeout`/
    /// `NotRun`/`Pass` verdict is not a text-only failure (there is no failure, or no
    /// comparison happened at all), so this is `false` for those by construction.
    fn is_text_only_failure(&self) -> bool {
        match self {
            Verdict::Fail(reasons) => {
                !reasons.is_empty() && reasons.iter().all(|(half, _)| *half == FailedHalf::Text)
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// The enforced cap
// ---------------------------------------------------------------------------

/// What became of a body handed to [`run_with_timeout`].
#[derive(Debug)]
enum Worker<T> {
    /// The body finished inside the cap; here is its value.
    Done(T),
    /// The body did not finish inside the cap. Its thread is still running — see this file's
    /// "Resource discipline" section for why that is the accepted tradeoff.
    TimedOut,
    /// The body panicked *outside* its own `catch_unwind` (i.e. the harness itself broke, not
    /// the port under test). Carries the panic message.
    Panicked(String),
}

/// Runs `body` on a worker thread and gives up on it after `cap`.
///
/// This is the mechanism behind `CLAUDE.md`'s "per-test resource caps, **never hangs**": the
/// calling thread blocks on `recv_timeout`, never on `join`, so it always regains control at
/// `cap` whatever the body is doing. `body` must own everything it needs — nothing that is
/// not `Send` may cross the boundary, which is why callers pass path `String`s and build the
/// (`Rc`-holding, `!Send`) `Prover` *inside* the closure.
///
/// A timed-out thread is abandoned, not killed; Rust has no primitive for the latter.
fn run_with_timeout<T: Send + 'static>(
    what: &str,
    cap: Duration,
    body: impl FnOnce() -> T + Send + 'static,
) -> Worker<T> {
    let (tx, rx) = sync_channel::<T>(1);
    let handle = std::thread::Builder::new()
        .name(what.to_string())
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || {
            // The receiver is gone only when the harness already gave up on this worker.
            let _ = tx.send(body());
        })
        .unwrap_or_else(|e| panic!("spawning a worker thread for {what}: {e}"));

    match rx.recv_timeout(cap) {
        Ok(value) => {
            // The value arrived, so the closure has run to completion; this join is a
            // formality that cannot block for meaningful time.
            let _ = handle.join();
            Worker::Done(value)
        }
        Err(RecvTimeoutError::Timeout) => Worker::TimedOut,
        // The sender was dropped without sending: the body unwound past its own
        // `catch_unwind` (or there was none). The thread is finished, so `join` is safe.
        Err(RecvTimeoutError::Disconnected) => Worker::Panicked(match handle.join() {
            Err(payload) => panic_message(payload.as_ref()),
            Ok(()) => "<worker exited without sending a result>".to_string(),
        }),
    }
}

struct Outcome {
    id: usize,
    command: String,
    verdict: Verdict,
    elapsed: Duration,
    notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Halting the run
// ---------------------------------------------------------------------------

/// Why the corpus run stopped attempting fixtures — and the *only* thing that decides it.
///
/// Two events halt a run, and both mean the same thing operationally: from here on, this
/// harness can no longer produce a verdict that says anything about the port.
///
/// * **A timeout** (a fixture's or a prelude command's). The worker is abandoned, not killed
///   (see this file's "Resource discipline" section), so an unbounded thread may still be
///   writing into the shared on-disk session tree that every later fixture's `Prover` reads.
///   Continuing would emit `PASS`/`FAIL` verdicts that raced a live writer — indistinguishable
///   in the report from cleanly computed ones, which is precisely the diagnostic trap this
///   type exists to close.
/// * **The total budget** ([`MAX_TOTAL_SECS`]). Nothing is corrupt here; the run is simply out
///   of time.
///
/// Once halted, every remaining fixture is recorded [`Verdict::NotRun`] carrying the reason,
/// and is neither dispatched nor compared. The first reason wins — a later event must not
/// overwrite the explanation of why the run actually stopped.
#[derive(Debug, Default)]
struct Halt(Option<String>);

impl Halt {
    fn reason(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Records `reason` unless the run has already halted for an earlier one.
    fn halt(&mut self, reason: String) {
        if self.0.is_none() {
            self.0 = Some(reason);
        }
    }

    /// The timeout rule, in one place so the prelude, the `WR_GOLDEN_ONLY` state-only path and
    /// the main loop cannot drift apart.
    fn halt_on_abandoned_worker(&mut self, what: &str) {
        self.halt(format!(
            "not run: {what} timed out after {MAX_FIXTURE_SECS}s and its worker thread was \
             abandoned (Rust cannot kill a thread), so it may still be writing into the shared \
             session tree — no later fixture's verdict would mean anything"
        ));
    }
}

/// The `NotRun` entry a halted run records for a fixture it will not attempt, or `None` while
/// the run is still live. Called once per fixture, at the top of the corpus loop.
fn halted_outcome(halt: &Halt, id: usize, command: &str) -> Option<Outcome> {
    halt.reason().map(|reason| Outcome {
        id,
        command: command.to_string(),
        verdict: Verdict::NotRun,
        elapsed: Duration::ZERO,
        notes: vec![reason.to_string()],
    })
}

// ---------------------------------------------------------------------------
// Known divergences
// ---------------------------------------------------------------------------

/// Fixtures that do **not** currently reproduce Walnut's recorded output, each with the reason
/// it is accepted rather than fixed. The harness asserts the failing set is EXACTLY this set:
/// a new failure fails the run, and so does a fixture here that starts passing (which means
/// the entry is stale and must be deleted).
///
/// An entry here is only legitimate when it names a `docs/WALNUT-BUGS.md` entry, an already-
/// signed-off scope decision, or an open follow-up unit — never "this one is hard".
///
/// **An entry excuses a TEXT divergence only.** Every id below is accepted specifically
/// because its automaton comparison already passes and only the log text differs — so the
/// gate honors an entry only when [`Verdict::is_text_only_failure`] holds for that run's
/// verdict ([`FailedHalf`]). If a future regression breaks fixture 637's or 660's
/// *automaton*, being on this list will not save it: it is reported as `NOT TEXT-ONLY`.
/// Before that check existed, matching was by id alone, and exactly that regression would
/// have reported green.
const KNOWN_DIVERGENCES: &[(usize, &str)] = &[
    // -- root cause 1: `details` log-text fidelity (see tests/golden/STATUS.md §1) --------
    //
    // Every STATE COUNT in these fixtures already matches real Walnut exactly,
    // pre-minimization ones included. What is missing is log LINES: the `computing X`/
    // `computed X` pairs that Java emits from inside each token's `act()` body, and every
    // `wr-core`-level line (`computing cross product:N states`, `Minimizing: N states.`,
    // `Determinizing [#k, strategy: SC]`, `quantifying:N states`, …).
    //
    // Both are pre-existing, explicitly documented deferrals — `wr-logic`'s `eval.rs`
    // module docs ("DEFERRED GAP: the per-`act()` `Logging` calls are NOT ported", which
    // itself says the gap "must land before Phase 3b's U27") and `wr-core`'s `product.rs`
    // ("Progress logging not ported"). Closing them means threading `&mut Logging` through
    // every `act()` body AND through `wr-core`'s product/determinize/minimize/quantify call
    // sites: a follow-up unit, not a harness concern.
    (
        375,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    (
        376,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    (
        377,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    (
        378,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    (
        379,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    (
        383,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    (
        628,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    // 637 and 660 joined this list the moment `[strategy …]`/`[export …]` were wired through
    // to `wr_core::determinize` — before that they were not compared at all (they were the
    // whole of the old `Excluded::MetacommandNotWired` class). They are the same root cause,
    // not a new one, and specifically NOT a metacommand failure:
    //
    //   * 637's `[strategy 6 BRZ]` demonstrably takes effect. Its sixth determinization is a
    //     1,790-state NFA that does not finish inside this harness's 60s cap under `SC`; with
    //     the metacommand wired the whole fixture completes in well under a second, exactly as
    //     real Walnut does (130ms). Its `details` expectation additionally spells
    //     `Determinizing [#6, strategy: Brzozowski]` — a log LINE this port does not emit yet,
    //     which is the same gap 375-379/383/628 have.
    //   * 660's `[export 1 BA]` writes its `_pre` dump; Java's export is a side-effecting file
    //     write that never touches the returned automaton, so it cannot affect this comparison
    //     either way.
    //
    // Both now compare their AUTOMATON too (the `details` check no longer short-circuits —
    // see `fold_failures`), and both pass it; only the log text differs.
    (
        637,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    (
        660,
        "details: per-act()/wr-core logging not threaded (wr-logic eval.rs DEFERRED GAP)",
    ),
    // -- root cause 2 (CLOSED): `transduce` over a reversed (lsd) custom-base DFAO --------
    //
    // 532/533/534 used to live here: `test531 = reverse F` is an `lsd_fib` DFAO, so
    // `transduce test532 RUNSUM2 test531` was the ONE `transduce` fixture taking
    // `Transducer.transduceNonDeterministic`'s reverse-input/reverse-result branch, and it
    // was the ONE that failed. The root cause turned out not to be in `Transducer` at all:
    // `wr_core::logicalops::flip_ns` flipped `Automaton::msd` but left `Automaton::ns_name`
    // stale, so `reverse F` produced an automaton whose recorded number system still SAID
    // `msd_fib` while it was really `lsd_fib`. Fixed there (see that function's docs); these
    // three entries went stale and are deleted rather than left as a false claim.
];

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// `#[ignore]` = `docs/DESIGN.md` §5's **gated slow tier**, not a disabled test.
///
/// The corpus is Walnut's own workload set, and it is tractable — 12 s of prelude plus 28 s
/// for all 675 fixtures in a release build. In a debug build the same run is ~7 minutes (a
/// measured 10× ratio; see `tests/golden/STATUS.md`), which does not belong in the
/// every-commit fast tier. Everything else in this crate — the `assertEqualMessages`
/// normalization tests, the classifier tests, and
/// [`no_later_fixture_depends_on_an_unexecuted_one`] — runs by default in milliseconds.
///
/// ```bash
/// cargo test -p wr-golden --release -- --ignored --nocapture
/// ```
#[test]
#[ignore = "Tier-1 gated slow tier: run with --release -- --ignored (see tests/golden/STATUS.md)"]
fn tier1_golden_corpus() {
    let Some(root) = corpus_root() else {
        panic!(
            "Tier-1 golden corpus not found.\n\
             Looked for `src/test/resources/integrationTests/` under {}.\n\
             The corpus lives in the sibling `walnut-java` oracle repo and is deliberately not\n\
             vendored here (see tests/golden/tests/support/mod.rs). Point WALNUT_JAVA_DIR at a\n\
             walnut-java checkout to run Tier 1.",
            walnut_java_dir().display()
        );
    };
    let fixtures = load_fixtures().unwrap_or_else(|e| panic!("loading the Phase-0 manifests: {e}"));

    let dest = std::env::temp_dir().join(format!("wr-golden-corpus-{}", std::process::id()));
    let (home_dir, session_dir) =
        build_session_tree(&root, &dest).unwrap_or_else(|e| panic!("building session tree: {e}"));

    let seeded = global_library_names(&home_dir);
    let transitive = transitively_dropped(&fixtures, &seeded);
    let rewrite = PathRewrite {
        home_dir: home_dir.clone(),
        session_dir: session_dir.clone(),
    };

    // Two triage knobs, both of which disable the pass/fail gate at the end: a run that
    // deliberately looks at part of the corpus must never be able to report the corpus green.
    let only: Option<BTreeSet<usize>> = std::env::var("WR_GOLDEN_ONLY").ok().map(|s| {
        s.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().parse().expect("WR_GOLDEN_ONLY: not a fixture id"))
            .collect()
    });
    let stop_after: Option<usize> = std::env::var("WR_GOLDEN_STOP_AFTER").ok().map(|s| {
        s.trim()
            .parse()
            .expect("WR_GOLDEN_STOP_AFTER: not a fixture id")
    });
    let triage = only.is_some() || stop_after.is_some();

    // -- the prelude (`IntegrationTest.initialize`, `:43-78`) -----------------
    let prelude_started = Instant::now();
    let mut prelude_failures = Vec::new();
    let mut halt = Halt::default();
    for (i, command) in PRELUDE.iter().enumerate() {
        // Once an earlier prelude command's worker has been abandoned, the rest of the prelude
        // would build its state on a tree that thread may still be writing to — the same
        // reasoning that halts the corpus loop below.
        if let Some(reason) = halt.reason() {
            prelude_failures.push(format!("prelude[{i}] `{command}`: {reason}"));
            continue;
        }
        // Same enforced cap as a fixture: a prelude command that hangs would wedge the run
        // before a single fixture had reported anything.
        let what = format!("prelude:{i}");
        let (sd, hd, cmd, msg) = (
            session_dir.clone(),
            home_dir.clone(),
            command.to_string(),
            what.clone(),
        );
        let outcome = run_with_timeout(&what, Duration::from_secs(MAX_FIXTURE_SECS), move || {
            let mut prover = fresh_prover(&sd, &hd);
            // `ProverError` is not `Send`; only its rendered message crosses back.
            match panic::catch_unwind(AssertUnwindSafe(|| {
                prover.dispatch_for_integration_test(&cmd, &msg)
            })) {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(e.to_string()),
                Err(payload) => Err(format!("PANICKED: {}", panic_message(payload.as_ref()))),
            }
        });
        match outcome {
            Worker::Done(Ok(())) => {}
            Worker::Done(Err(e)) => prelude_failures.push(format!("prelude[{i}] `{command}`: {e}")),
            Worker::Panicked(m) => {
                prelude_failures.push(format!("prelude[{i}] `{command}`: PANICKED: {m}"))
            }
            Worker::TimedOut => {
                prelude_failures.push(format!(
                    "prelude[{i}] `{command}`: TIMED OUT after {MAX_FIXTURE_SECS}s (thread abandoned)"
                ));
                // Same reasoning as a fixture timeout, one step earlier: the abandoned worker
                // owns a `Prover` over the shared session tree, so the seed state the whole
                // corpus is replayed against is no longer trustworthy.
                halt.halt_on_abandoned_worker(&format!("prelude command {i} (`{command}`)"));
            }
        }
    }

    // -- the corpus -----------------------------------------------------------
    let prelude_elapsed = prelude_started.elapsed();
    eprintln!("prelude finished in {:.1}s", prelude_elapsed.as_secs_f64());
    let run_started = Instant::now();
    let mut outcomes: Vec<Outcome> = Vec::with_capacity(fixtures.len());

    for fixture in &fixtures {
        if stop_after.is_some_and(|last| fixture.id > last) {
            break;
        }
        // The run has halted (a timeout, or the total budget): record the fixture as `NotRun`
        // and attempt nothing — see [`Halt`].
        if let Some(outcome) = halted_outcome(&halt, fixture.id, &fixture.command_script) {
            outcomes.push(outcome);
            continue;
        }
        if only.as_ref().is_some_and(|ids| !ids.contains(&fixture.id)) {
            // Still EXECUTED (the session is stateful) but not compared, and not reported as
            // a skip — `WR_GOLDEN_ONLY` is a triage tool, not a scope decision. Capped like
            // every other execution, so a triage run cannot hang on a fixture it is not even
            // looking at.
            let (sd, hd, cmd, id) = (
                session_dir.clone(),
                home_dir.clone(),
                fixture.command_script.clone(),
                fixture.id,
            );
            let outcome = run_with_timeout(
                &format!("fixture {id} (WR_GOLDEN_ONLY: state only)"),
                Duration::from_secs(MAX_FIXTURE_SECS),
                move || {
                    let mut prover = fresh_prover(&sd, &hd);
                    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
                        prover.dispatch_for_integration_test(&cmd, &id.to_string())
                    }));
                },
            );
            if let Worker::TimedOut = outcome {
                eprintln!(
                    "[TIMEOUT] {id:>3} (run for its session state only) exceeded the \
                     {MAX_FIXTURE_SECS}s cap; the run halts here"
                );
                halt.halt_on_abandoned_worker(&format!(
                    "fixture {id} (run for its session state only)"
                ));
            }
            continue;
        }

        let exclusion = classify(fixture, &transitive);
        let expected = load_expected(&root, fixture.id)
            .unwrap_or_else(|e| panic!("fixture {}: reading expectations: {e}", fixture.id));

        // The ONE class of fixture that is not even executed. Everything else runs, because
        // the session is stateful and later fixtures read what earlier ones wrote — see this
        // file's module docs. An unwired `[strategy …]` fixture is different in kind: running
        // it does not reproduce any state Walnut recorded (the strategy is ignored, so the
        // computation is a different one), and for the corpus's five `[strategy 6 …]` fixtures
        // it is also ruinously expensive — Walnut answers `test637` in 130 ms via Brzozowski,
        // where the default `SC` subset-constructs the same 1,790-state NFA and does not
        // finish. `no_later_fixture_depends_on_an_unexecuted_one` below proves nothing
        // downstream reads their output, so skipping execution cannot perturb any other
        // fixture's verdict.
        let execute = !skips_execution(fixture);

        let job = FixtureJob {
            id: fixture.id,
            command: fixture.command_script.clone(),
            session_dir: session_dir.clone(),
            home_dir: home_dir.clone(),
            execute,
            exclusion,
            expected,
            rewrite: rewrite.clone(),
        };
        let started = Instant::now();
        let outcome = run_with_timeout(
            &format!("fixture {}", fixture.id),
            Duration::from_secs(MAX_FIXTURE_SECS),
            move || job.run(),
        );
        let elapsed = started.elapsed();

        let (verdict, notes) = match outcome {
            Worker::Done(done) => (done.verdict, done.notes),
            Worker::Panicked(message) => (
                // The job catches the port's own panics itself, so reaching here means the
                // HARNESS unwound (a comparison helper, or `fresh_prover`). Reported against
                // the fixture rather than aborting the run.
                Verdict::fail(
                    FailedHalf::Harness,
                    format!("harness thread panicked: {message}"),
                ),
                Vec::new(),
            ),
            Worker::TimedOut => {
                halt.halt_on_abandoned_worker(&format!("fixture {}", fixture.id));
                (
                    Verdict::Timeout(Duration::from_secs(MAX_FIXTURE_SECS)),
                    vec![format!(
                        "TIMED OUT after {MAX_FIXTURE_SECS}s; the worker thread is abandoned, \
                         not stopped (Rust cannot kill a thread), so it may still be writing \
                         into the shared session tree — the run halts here and every later \
                         fixture is reported NOT-RUN rather than compared against a tree a \
                         live writer may still be mutating"
                    )],
                )
            }
        };

        // Progress, one line per fixture. Captured by libtest unless `--nocapture`, so this
        // costs nothing in a normal run and makes a long triage run watchable.
        eprintln!(
            "[{:>5}] {:>3} {:>7.2}s {}{}",
            verdict.tag(),
            fixture.id,
            elapsed.as_secs_f64(),
            truncate(&fixture.command_script, 70),
            match &verdict {
                Verdict::Fail(_) => format!(
                    "\n         >> {}",
                    truncate(verdict.message().lines().next().unwrap_or(""), 160)
                ),
                _ => String::new(),
            }
        );
        outcomes.push(Outcome {
            id: fixture.id,
            command: fixture.command_script.clone(),
            verdict,
            elapsed,
            notes,
        });

        if run_started.elapsed() > Duration::from_secs(MAX_TOTAL_SECS) {
            halt.halt(format!(
                "not run: the run stopped early after exceeding the {MAX_TOTAL_SECS}s total budget"
            ));
        }
    }

    // -- report ---------------------------------------------------------------
    let report = build_report(
        &outcomes,
        &prelude_failures,
        prelude_elapsed,
        run_started.elapsed(),
    );
    let report_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join("golden-corpus-report.txt");
    if let Some(parent) = report_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&report_path, &report);
    println!("{report}");
    println!("full report written to {}", report_path.display());

    // `WR_GOLDEN_KEEP_TREE=1` leaves the temp home/session tree in place so a triage run can
    // inspect exactly what the corpus built (which library files exist, what a `load` wrote).
    if std::env::var_os("WR_GOLDEN_KEEP_TREE").is_none() {
        let _ = std::fs::remove_dir_all(&dest);
    } else {
        println!("session tree kept at {}", dest.display());
    }

    // -- gate -----------------------------------------------------------------
    if triage {
        // A triage run compares a hand-picked subset; it must not be able to "pass" the gate.
        println!("WR_GOLDEN_ONLY/WR_GOLDEN_STOP_AFTER set: gate skipped (triage run).");
        return;
    }

    let known: BTreeMap<usize, &str> = KNOWN_DIVERGENCES.iter().copied().collect();
    let failing: BTreeMap<usize, String> = outcomes
        .iter()
        .filter_map(|o| match &o.verdict {
            Verdict::Fail(_) => Some((o.id, o.verdict.message())),
            Verdict::Timeout(cap) => Some((
                o.id,
                format!(
                    "timed out: no answer within the {:.0}s per-fixture cap",
                    cap.as_secs_f64()
                ),
            )),
            Verdict::NotRun => Some((
                o.id,
                if o.notes.is_empty() {
                    "not run".to_string()
                } else {
                    o.notes.join("; ")
                },
            )),
            _ => None,
        })
        .collect();

    // A halted run turns hundreds of fixtures into `NotRun` at once; listing them one per line
    // would bury the ONE entry that explains why the run stopped. They still block the gate.
    let not_run: BTreeSet<usize> = outcomes
        .iter()
        .filter(|o| o.verdict == Verdict::NotRun)
        .map(|o| o.id)
        .collect();

    // Which fixtures diverged in a way a `KNOWN_DIVERGENCES` entry is even *allowed* to
    // excuse: a failure every one of whose reasons is a TEXT one (see `FailedHalf`). An id
    // in `KNOWN_DIVERGENCES` whose automaton comparison broke — or that timed out, or that
    // panicked the harness — is a NEW FAILURE, not a known one, however long it has been on
    // the list. Matching by id alone (what this gate used to do) would let exactly that
    // regression report green.
    let text_only: BTreeSet<usize> = outcomes
        .iter()
        .filter(|o| o.verdict.is_text_only_failure())
        .map(|o| o.id)
        .collect();

    let mut problems = String::new();
    for (id, why) in &failing {
        if !not_run.contains(id) && !(known.contains_key(id) && text_only.contains(id)) {
            if known.contains_key(id) {
                let _ = writeln!(
                    problems,
                    "  NOT TEXT-ONLY fixture {id} IS in KNOWN_DIVERGENCES, but this run's \
                     divergence is not a text-only one — every entry there is accepted \
                     *because* its automaton already matches, so this does not qualify: {why}"
                );
            } else {
                let _ = writeln!(problems, "  NEW FAILURE   fixture {id}: {why}");
            }
        }
    }
    if !not_run.is_empty() {
        let first = not_run.iter().next().copied().unwrap_or_default();
        let _ = writeln!(
            problems,
            "  NOT RUN       {} fixtures were never attempted (from fixture {first} on): {}",
            not_run.len(),
            failing.get(&first).map(String::as_str).unwrap_or("not run")
        );
    }
    for (id, why) in &known {
        if !failing.contains_key(id) {
            let _ = writeln!(
                problems,
                "  STALE ENTRY   fixture {id} is listed in KNOWN_DIVERGENCES ({why}) but now passes \
                 — delete the entry"
            );
        }
    }
    if !prelude_failures.is_empty() {
        for f in &prelude_failures {
            let _ = writeln!(problems, "  PRELUDE       {f}");
        }
    }
    assert!(
        problems.is_empty(),
        "Tier-1 golden corpus is not green:\n{problems}\nfull report: {}",
        report_path.display()
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Everything one fixture's worker thread needs, as owned `Send` data.
///
/// The `Prover`/`Session`/`TestCase` themselves never cross the thread boundary — they hold
/// `Rc` state and are not `Send`, so the worker builds and consumes them entirely inside
/// itself and sends back only a [`Completed`]. This is what makes [`run_with_timeout`]'s
/// enforced cap possible at all; see this file's "Resource discipline" section.
struct FixtureJob {
    id: usize,
    command: String,
    session_dir: String,
    home_dir: String,
    execute: bool,
    exclusion: Option<Excluded>,
    expected: Expected,
    rewrite: PathRewrite,
}

/// A worker's answer for one fixture: exactly the two fields the harness thread records.
struct Completed {
    verdict: Verdict,
    notes: Vec<String>,
}

impl FixtureJob {
    /// The body that used to run inline in the corpus loop, unchanged in substance: build a
    /// fresh `Prover`, dispatch (unless the fixture is one of the never-executed ones), and
    /// compare against the recorded expectation.
    fn run(self) -> Completed {
        let mut prover = fresh_prover(&self.session_dir, &self.home_dir);
        let dispatched = self.execute.then(|| {
            panic::catch_unwind(AssertUnwindSafe(|| {
                prover.dispatch_for_integration_test(&self.command, &self.id.to_string())
            }))
        });

        let mut notes = Vec::new();
        if !self.execute {
            notes.push("not executed (see skip reason)".to_string());
        }
        let verdict = match self.exclusion {
            Some(reason) => Verdict::Skipped(reason),
            // `execute` is only ever false for a fixture that `classify` already excluded
            // (both are driven by `deferred_otf_strategy`), so this arm is unreachable — but
            // it is a real `Option`, and a future exclusion class that forgets to keep the two
            // in step should say so loudly rather than silently compare a result that was
            // never computed.
            None if dispatched.is_none() => Verdict::fail(
                FailedHalf::Harness,
                "harness bug: the fixture was not executed but is not excluded either",
            ),
            None => match dispatched.expect("checked by the arm above") {
                Err(payload) => Verdict::fail(
                    FailedHalf::Harness,
                    format!("PANICKED: {}", panic_message(payload.as_ref())),
                ),
                Ok(Err(error)) => {
                    compare_error(&self.expected, &self.rewrite.apply(&error.to_string()))
                }
                Ok(Ok(None)) => Verdict::fail(
                    FailedHalf::Harness,
                    "dispatch returned no TestCase, but the corpus records an expectation \
                     (Java's harness fails on a null TestCase too)",
                ),
                Ok(Ok(Some(actual))) => compare_test_case(
                    prover.session(),
                    &self.expected,
                    &actual,
                    &self.rewrite,
                    &mut notes,
                ),
            },
        };
        Completed { verdict, notes }
    }
}

fn fresh_prover(session_dir: &str, home_dir: &str) -> Prover {
    // `Prover.mainProver = new Prover();` (`IntegrationTest.java:926`) — a fresh prover per
    // fixture, over the same on-disk session.
    let session = Session::new(Some(session_dir), Some(home_dir), false);
    Prover::with_output(session, Logging::new(), Box::new(std::io::sink()))
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// The one "do not even run it" rule: a fixture asking for a deferred **OTF** strategy
/// (`CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`, fixtures 638-641).
///
/// Those four are the same query as 637 — `E x,y,z (n=x+y+z)&(QQ[x]=@1)&(QQ[y]=@1)&(QQ[z]=@1)`,
/// whose sixth determinization is a 1,790-state NFA that real Walnut only survives because the
/// metacommand switches it off subset construction. 637 itself is now executed and compared
/// (its `[strategy 6 BRZ]` is wired through, so it takes the same cheap Brzozowski path Java
/// does), but 638-641 name strategies this port deliberately does not implement, so they would
/// fall back to `SC` on that NFA and blow the per-fixture budget for no information.
///
/// Keyed on the *command text* rather than on which [`Excluded`] variant `classify` picked, so
/// the two cannot drift apart silently — `no_later_fixture_depends_on_an_unexecuted_one` checks
/// that skipping them cannot change any other fixture's answer.
fn skips_execution(fixture: &Fixture) -> bool {
    deferred_otf_strategy(&fixture.command_script).is_some()
}

/// Nothing later in the corpus may read a library file that a not-executed fixture would have
/// written — otherwise skipping execution would silently change some *other* fixture's answer,
/// which is exactly the kind of quiet corruption this harness exists to catch.
///
/// Checked at run time rather than assumed, so it stays true if `walnut-java` regenerates the
/// manifest with new fixtures.
#[test]
fn no_later_fixture_depends_on_an_unexecuted_one() {
    let Ok(fixtures) = load_fixtures() else {
        // The corpus/manifests are absent; `tier1_golden_corpus` reports that loudly on its
        // own and this check has nothing to check.
        return;
    };
    let mut produced: Vec<(usize, String)> = Vec::new();
    for f in &fixtures {
        if skips_execution(f) {
            if let Some(name) = support::defined_name(&f.command_script) {
                produced.push((f.id, name));
            }
        }
    }
    assert!(
        !produced.is_empty(),
        "the rule is supposed to apply to the corpus's `[strategy …]`/`[export …]` fixtures; \
         finding none means the classifier stopped matching"
    );
    for f in &fixtures {
        for (id, name) in &produced {
            if f.id > *id {
                assert!(
                    !support::references(&f.command_script, name),
                    "fixture {} references `{name}`, which fixture {id} would have written — \
                     that fixture is not executed, so skipping it would change fixture {}'s \
                     result. Either execute it after all, or exclude {} too.",
                    f.id,
                    f.id,
                    f.id
                );
            }
        }
    }
}

/// The per-fixture cap must be **enforced**, not measured after the fact — `CLAUDE.md`'s
/// "per-test resource caps, never hangs". Proved here on a stand-in body rather than on a real
/// runaway fixture: the corpus deliberately contains none (the one query that would not finish,
/// 637, is excluded for an unrelated reason and is not executed), and a harness that could only
/// demonstrate its cap by shipping a superexponential fixture would be a worse harness.
///
/// The first case's thread outlives the assertion by design; that is the documented tradeoff
/// (Rust cannot kill a thread) and it is why the assertion is that *the harness* moved on.
#[test]
fn the_per_fixture_cap_is_enforced_rather_than_measured_afterwards() {
    let started = Instant::now();
    let outcome = run_with_timeout(
        "stand-in for a runaway fixture",
        Duration::from_millis(200),
        || {
            std::thread::sleep(Duration::from_secs(120));
            "the body ran to completion, which this test's cap must have prevented".to_string()
        },
    );
    let waited = started.elapsed();
    assert!(
        matches!(outcome, Worker::TimedOut),
        "a body that outruns its cap must be reported TimedOut, got {outcome:?}"
    );
    assert!(
        waited < Duration::from_secs(30),
        "the harness blocked for {waited:?} on a 200ms cap — the cap is being measured, not \
         enforced, which is the exact defect this test exists for"
    );

    // A body that finishes inside its cap still returns its value, unchanged.
    assert!(matches!(
        run_with_timeout("fast body", Duration::from_secs(60), || 7u32),
        Worker::Done(7)
    ));

    // A body that unwinds is reported as a harness panic, NOT as a timeout — the two mean
    // different things at the gate and must not be conflated.
    let panicked = run_with_timeout("panicking body", Duration::from_secs(60), || -> () {
        // The worker's own panic message goes to stderr; libtest captures it.
        panic!("deliberate: worker died");
    });
    match panicked {
        Worker::Panicked(message) => assert!(
            message.contains("deliberate: worker died"),
            "the panic payload must survive: {message}"
        ),
        other => panic!("expected Worker::Panicked, got {other:?}"),
    }
}

/// A timeout must **taint the rest of the run**, not just its own fixture.
///
/// The timed-out worker is abandoned rather than killed, and it holds a `Prover` over the same
/// on-disk session tree every later fixture builds a fresh `Prover` against — so a verdict
/// computed after a timeout raced a live, unbounded writer and cannot be told apart, in the
/// report, from one computed cleanly. This drives the corpus loop's exact halt policy
/// ([`Halt`] + [`halted_outcome`], the only two things the loop consults) over a stand-in
/// sequence of verdicts, for the same reason the cap test above uses a stand-in body: the
/// corpus deliberately contains no fixture that would time out.
#[test]
fn a_timeout_halts_the_run_and_later_fixtures_are_not_run() {
    let ids = [10usize, 11, 12, 13];
    let mut halt = Halt::default();
    let mut attempted: Vec<usize> = Vec::new();
    let mut recorded: Vec<(usize, Verdict)> = Vec::new();

    for (i, id) in ids.iter().copied().enumerate() {
        if let Some(outcome) = halted_outcome(&halt, id, "eval t \"x=x\";") {
            assert!(
                !outcome.notes.is_empty(),
                "a NotRun outcome must carry the reason the run halted"
            );
            recorded.push((id, outcome.verdict));
            continue;
        }
        // The fixture is attempted for real; fixture 11 is the one that does not answer.
        attempted.push(id);
        let verdict = if i == 1 {
            Verdict::Timeout(Duration::from_secs(MAX_FIXTURE_SECS))
        } else {
            Verdict::Pass
        };
        if matches!(verdict, Verdict::Timeout(_)) {
            halt.halt_on_abandoned_worker(&format!("fixture {id}"));
        }
        recorded.push((id, verdict));
    }

    assert_eq!(
        attempted,
        vec![10, 11],
        "nothing after the timed-out fixture may be dispatched or compared"
    );
    assert_eq!(
        recorded
            .iter()
            .map(|(id, v)| (*id, v.tag()))
            .collect::<Vec<_>>(),
        vec![
            (10, "PASS"),
            (11, "TIMEOUT"),
            (12, "NOT-RUN"),
            (13, "NOT-RUN")
        ]
    );
    let reason = halt.reason().expect("the run must be halted");
    assert!(
        reason.contains("fixture 11") && reason.contains("abandoned"),
        "the halt reason must name the fixture that poisoned the session tree: {reason}"
    );

    // The first reason wins: a later event must not rewrite why the run actually stopped.
    halt.halt("not run: the run stopped early after exceeding the total budget".to_string());
    assert!(halt.reason().is_some_and(|r| r.contains("fixture 11")));

    // And a live run halts nothing.
    assert!(Halt::default().reason().is_none());
    assert!(halted_outcome(&Halt::default(), 1, "eval t \"x=x\";").is_none());
}

/// [`KNOWN_DIVERGENCES`]' gate condition, tested directly rather than only through a
/// 15-minute corpus run.
///
/// The invariant it enforces: an entry there excuses a **text-only** divergence and nothing
/// else. A fixture on the list whose automaton comparison ALSO broke must not be waved
/// through — which is precisely the regression the id-only match could not see, so it gets a
/// test of its own rather than a comment.
#[test]
fn only_a_text_only_failure_can_be_excused_by_a_known_divergence() {
    let text = |why: &str| Verdict::fail(FailedHalf::Text, why);
    let automaton = |why: &str| Verdict::fail(FailedHalf::Automaton, why);

    // A pure `details` mismatch — the shape all nine current entries have.
    assert!(text("details differs").is_text_only_failure());

    // The automaton half broke: never excusable, alone or alongside a text mismatch.
    assert!(!automaton("language differs").is_text_only_failure());
    assert!(
        !fold_failures(
            vec![(FailedHalf::Text, "details differs".to_string())],
            automaton("language differs"),
        )
        .is_text_only_failure(),
        "a text mismatch must not launder an automaton mismatch into a 'known' one"
    );
    // ...and folding still reports BOTH reasons, so the report says why.
    let both = fold_failures(
        vec![(FailedHalf::Text, "details differs".to_string())],
        automaton("language differs"),
    );
    assert!(both.message().contains("details differs"));
    assert!(both.message().contains("language differs"));

    // Harness-level failures are never text-only either.
    assert!(!Verdict::fail(FailedHalf::Harness, "PANICKED: boom").is_text_only_failure());

    // Neither is anything that is not a failure at all — in particular a `Timeout`, which the
    // gate also treats as failing.
    assert!(!Verdict::Pass.is_text_only_failure());
    assert!(!Verdict::Timeout(Duration::from_secs(MAX_FIXTURE_SECS)).is_text_only_failure());
    assert!(!Verdict::NotRun.is_text_only_failure());
    assert!(
        !Verdict::Fail(Vec::new()).is_text_only_failure(),
        "an empty reason list is a harness bug, not a text-only divergence"
    );

    // And a clean fold is still a pass.
    assert_eq!(fold_failures(Vec::new(), Verdict::Pass), Verdict::Pass);
}

/// The same invariant, driven through the **real comparison functions** rather than through
/// hand-built [`Verdict`]s — because the tagging is only worth anything if the comparators
/// actually apply it, and the hand-built test above cannot see a comparator that tags the
/// wrong half.
///
/// The shape under test is the one that matters most in practice: a fixture the corpus
/// records a SUCCESSFUL result for, where the port errors (or panics, which `FixtureJob::run`
/// turns into a `Harness` failure, or fails at dispatch, which arrives here). No automaton
/// comparison happens on that path at all, so it must never be excusable by a
/// `KNOWN_DIVERGENCES` entry.
#[test]
fn an_error_where_the_corpus_records_success_is_not_a_text_only_failure() {
    let success = Expected {
        automaton_path: None,
        error: String::new(),
        details: String::new(),
        graph_viz: String::new(),
        matrix_output: vec![String::new(); 4],
    };

    // The port errored where the corpus says the command succeeds: nothing was compared.
    let verdict = compare_error(&success, "Undefined variable: index out of bounds");
    assert!(matches!(verdict, Verdict::Fail(_)));
    assert!(
        !verdict.is_text_only_failure(),
        "an outright port error on a corpus-expects-success fixture has zero automaton \
         evidence behind it, so `KNOWN_DIVERGENCES` must not be able to excuse it: {verdict:?}"
    );

    // Both sides errored and only the wording differs: that IS a text comparison.
    let errored = Expected {
        error: "Undefined variable: x".to_string(),
        ..success.clone()
    };
    assert!(compare_error(&errored, "Undefined variable: y").is_text_only_failure());
    assert_eq!(
        compare_error(&errored, "Undefined variable: x"),
        Verdict::Pass
    );
}

/// [`compare_test_case`]'s own tagging, driven end to end on a real [`TestCase`] and a real
/// recorded automaton file.
///
/// Two things are pinned here, and the second is the one the earlier short-circuiting code
/// got wrong:
///  1. a text mismatch (here graphviz) no longer pre-empts step 4 — the automaton comparison
///     still runs, so when the automaton ALSO differs the verdict carries both reasons and is
///     not text-only;
///  2. the same text mismatch with a MATCHING automaton is still text-only, so the
///     tightening does not turn any genuine text-only divergence into a hard failure.
///
/// Under the old code case 1 returned `FailedHalf::Text` from the graphviz check and the
/// automaton was never looked at — indistinguishable from case 2 at the gate, i.e. a listed
/// fixture whose language silently broke would have reported green.
#[test]
fn a_text_mismatch_no_longer_pre_empts_the_automaton_comparison() {
    let dir = std::env::temp_dir().join(format!(
        "wr-golden-tagging-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // `x = x` over msd_2: accepts everything.
    let everything = dir.join("everything.txt");
    std::fs::write(&everything, "msd_2\n\n0 1\n0 -> 0\n1 -> 0\n").unwrap();
    // A different language: `x = 0`.
    let only_zero = dir.join("only_zero.txt");
    std::fs::write(
        &only_zero,
        "msd_2\n\n0 1\n0 -> 0\n1 -> 1\n\n1 0\n0 -> 1\n1 -> 1\n",
    )
    .unwrap();

    let read = |p: &Path| wr_io::reader::read_automaton_txt(p.to_str().unwrap()).unwrap();
    let session = Session::new(
        Some(dir.join("Session").to_str().unwrap()),
        Some(dir.to_str().unwrap()),
        false,
    );
    let rewrite = PathRewrite {
        home_dir: dir.display().to_string(),
        session_dir: dir.join("Session").display().to_string(),
    };
    // The port's answer, in both cases: the `accepts everything` automaton. `gv_address` is
    // empty, so `TestCase::graph_viz()` yields `""` — a mismatch against any recorded one.
    let actual = || TestCase::from_automaton(read(&everything));

    let expected_with_gv = |automaton_path: &Path| Expected {
        automaton_path: Some(automaton_path.to_path_buf()),
        error: String::new(),
        details: String::new(),
        graph_viz: "digraph G { rankdir = LR; }".to_string(),
        matrix_output: vec![String::new(); 4],
    };

    // 1. graphviz differs AND the automaton differs.
    let mut notes = Vec::new();
    let verdict = compare_test_case(
        &session,
        &expected_with_gv(&only_zero),
        &actual(),
        &rewrite,
        &mut notes,
    );
    assert!(
        verdict.message().contains("graphviz"),
        "the graphviz mismatch must still be reported: {verdict:?}"
    );
    assert!(
        verdict.message().contains("automaton language differs"),
        "the automaton comparison must have RUN despite the graphviz mismatch: {verdict:?}"
    );
    assert!(
        !verdict.is_text_only_failure(),
        "a text mismatch must not launder a broken automaton into an excusable one: {verdict:?}"
    );

    // 2. the same graphviz mismatch with a MATCHING automaton is still text-only.
    let mut notes = Vec::new();
    let verdict = compare_test_case(
        &session,
        &expected_with_gv(&everything),
        &actual(),
        &rewrite,
        &mut notes,
    );
    assert!(
        verdict.is_text_only_failure(),
        "a genuine text-only divergence must stay excusable: {verdict:?}"
    );

    // 3. the corpus records an ERROR but the command succeeded: no automaton was recorded,
    //    so the automaton comparison cannot run — never excusable.
    let mut notes = Vec::new();
    let verdict = compare_test_case(
        &session,
        &Expected {
            automaton_path: None,
            error: "Undefined variable: x".to_string(),
            details: String::new(),
            graph_viz: String::new(),
            matrix_output: vec![String::new(); 4],
        },
        &actual(),
        &rewrite,
        &mut notes,
    );
    assert!(
        !verdict.is_text_only_failure(),
        "the corpus recorded no automaton for an error fixture, so nothing was compared: \
         {verdict:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

fn classify(fixture: &Fixture, transitive: &BTreeMap<usize, Vec<String>>) -> Option<Excluded> {
    if !fixture.subset_relevant {
        return Some(Excluded::DropScope(fixture.drop_reason.clone()));
    }
    if let Some(strategy) = deferred_otf_strategy(&fixture.command_script) {
        return Some(Excluded::OtfStrategy(strategy));
    }
    if let Some(names) = transitive.get(&fixture.id) {
        return Some(Excluded::TransitiveDropDependency(names.clone()));
    }
    None
}

/// The `catch(Exception e)` arm of `runSpecificTest` (`:963-965`):
/// `assertEquals(expected.getError(), e.getMessage())` — an EXACT comparison, with none of
/// `assertEqualMessages`' normalization.
///
/// **The two mismatch branches are tagged differently, and that difference is the point.**
/// Only one of them is a text comparison at all:
///   * the corpus recorded an error too, and the two error strings differ — a genuine TEXT
///     divergence, and the only shape a [`KNOWN_DIVERGENCES`] entry may excuse;
///   * the corpus recorded a **successful** result and the port errored (or panicked into an
///     error) instead — here the automaton comparison never ran, and "did not run" is not
///     "passed" ([`FailedHalf::Automaton`]'s own contract). Tagging this `Text` would let a
///     listed fixture that starts erroring outright be waved through as "text-only" with zero
///     automaton evidence, which is precisely what the tagging exists to prevent.
fn compare_error(expected: &Expected, actual: &str) -> Verdict {
    if expected.error == actual {
        Verdict::Pass
    } else if expected.error.is_empty() {
        Verdict::fail(
            FailedHalf::Automaton,
            format!(
                "command failed, but the corpus records a successful result — so the automaton \
                 comparison never ran.\n  actual error: {actual}"
            ),
        )
    } else {
        Verdict::fail(
            FailedHalf::Text,
            format!(
                "error message differs (exact comparison, as in Java)\n  expected: {}\n  actual:   {actual}",
                expected.error
            ),
        )
    }
}

fn compare_test_case(
    session: &Session,
    expected: &Expected,
    actual: &TestCase,
    rewrite: &PathRewrite,
    notes: &mut Vec<String>,
) -> Verdict {
    if !expected.error.is_empty() {
        // Tagged `Automaton`, not `Text`, even though the strings being contrasted are text:
        // the corpus recorded an ERROR for this fixture, so it recorded no automaton (nor
        // details/matrix/graphviz) to compare against. The automaton comparison therefore
        // cannot run at all — and "did not run" is not "passed", so no `KNOWN_DIVERGENCES`
        // entry may excuse it. This is also why this one check still short-circuits where the
        // three below now fall through: there is genuinely nothing left to compare, and
        // running the remaining steps against absent expectations would only manufacture
        // cascading phantom mismatches.
        return Verdict::fail(
            FailedHalf::Automaton,
            format!(
                "command succeeded, but the corpus records an error (so no automaton was \
                 recorded to compare against): {}",
                expected.error
            ),
        );
    }

    // Every mismatch from here on is COLLECTED, not returned. It used to short-circuit, which
    // meant a fixture whose matrix/graphviz/`details` text diverged never had its AUTOMATON
    // compared at all — so an automaton divergence could hide behind a text one indefinitely,
    // AND (worse, once `KNOWN_DIVERGENCES` started being gated on `FailedHalf`) a text-tagged
    // early return would advertise "the automaton half is fine" about a comparison that never
    // ran. Every remaining `details` divergence in `KNOWN_DIVERGENCES` is a log-text gap whose
    // state counts already match; that claim is only checkable if the automaton comparison
    // still runs, so all three text comparisons push into `failures` and execution continues
    // to step 4.
    let mut failures: Vec<(FailedHalf, String)> = Vec::new();

    // 1. matrix output (`:933-938`)
    let actual_matrix = match actual.matrix_output() {
        Ok(m) => m,
        Err(e) => {
            return Verdict::fail(
                FailedHalf::Harness,
                format!("reading actual matrix output: {e}"),
            )
        }
    };
    if expected.has_cas_matrices() {
        // CAS incidence-matrix export is DROP scope for this port; compared automaton/details
        // still apply. Recorded, not silent.
        notes.push("cas-matrix-skipped (CAS export is DROP scope)".to_string());
    } else if expected.matrix_output.len() != actual_matrix.len() {
        failures.push((
            FailedHalf::Text,
            format!(
                "matrix output size differs: expected {}, actual {}",
                expected.matrix_output.len(),
                actual_matrix.len()
            ),
        ));
    } else {
        for (i, (e, a)) in expected
            .matrix_output
            .iter()
            .zip(actual_matrix.iter())
            .enumerate()
        {
            if let Err(why) = compare_messages(e.trim(), a.trim(), &format!("matrix output[{i}]")) {
                failures.push((FailedHalf::Text, why));
            }
        }
    }

    // 2. graphviz (`:940-945`) — only when the corpus recorded one. A `.gv` rendering
    // mismatch says nothing about the automaton's language (it is a view of the same object),
    // so it is collected like the others rather than pre-empting step 4.
    if !expected.graph_viz.trim().is_empty() {
        match actual.graph_viz() {
            Ok(actual_gv) => {
                if let Err(why) = compare_messages(
                    expected.graph_viz.trim(),
                    actual_gv.trim(),
                    "graphviz output",
                ) {
                    failures.push((FailedHalf::Text, why));
                }
            }
            // A harness-level read failure is never excusable, but it must not discard the
            // matrix reasons collected above — hence the fold rather than a bare return.
            Err(e) => {
                return fold_failures(
                    failures,
                    Verdict::fail(FailedHalf::Harness, format!("reading actual graphviz: {e}")),
                )
            }
        }
    }

    // 3. details (`:946`). `rewrite` maps this harness's own throwaway library directories
    // back to the ones `walnut-java` recorded — see [`PathRewrite`] for why that is the one
    // extra normalization and why it cannot hide a port defect.
    if let Err(why) = compare_messages(
        &expected.details,
        &rewrite.apply(actual.details()),
        "details",
    ) {
        failures.push((FailedHalf::Text, why));
    }

    // 4. automaton pairs (`:947-961`). `loadTestCases` always records exactly one pair for a
    // subset-relevant fixture — the `automaton_repr` second pair only ever appears on the one
    // Ostrowski fixture (id 625), which is DROP scope and never compared here.
    let actual_pairs = actual.automaton_pairs().len();
    if actual_pairs != 1 {
        return fold_failures(
            failures,
            Verdict::fail(
                FailedHalf::Automaton,
                format!(
                    "expected exactly one automaton pair (the corpus records one), got {actual_pairs}"
                ),
            ),
        );
    }
    let actual_automaton = actual.automaton_pairs()[0].automaton();
    let automaton_verdict = match (&expected.automaton_path, actual_automaton) {
        (None, None) => Verdict::Pass,
        (None, Some(_)) => Verdict::fail(
            FailedHalf::Automaton,
            "the corpus records no automaton for this fixture, but the port produced one",
        ),
        (Some(_), None) => Verdict::fail(
            FailedHalf::Automaton,
            "the corpus records an automaton for this fixture, but the port produced none",
        ),
        (Some(path), Some(ours)) => {
            let mut expected_automaton =
                match wr_io::reader::read_automaton_txt_with_custom_base_resolver(
                    path,
                    session.paths(),
                ) {
                    Ok(a) => a,
                    Err(e) => {
                        return fold_failures(
                            failures,
                            Verdict::fail(
                                FailedHalf::Automaton,
                                format!(
                                    "the recorded automaton {} could not be read: {e:?}",
                                    path.display()
                                ),
                            ),
                        )
                    }
                };
            // `EqualityUtils.faEqual(actualA.fa, expectedA.fa)` (`:958`). Two normalizations
            // apply before that call can mean anything, both established by the Tier-3 suite:
            //   * `sort_label()` on ours — the recorded file was written through Java's
            //     `AutomatonWriter`, which canonizes (and therefore sorts the label) first, so
            //     the two must be brought into the same track order before a POSITIONAL
            //     comparison is meaningful (`wr_core::equiv`'s own documented gap);
            //   * `totalize(0)` on both — Walnut's `.txt` automata need not be total (a
            //     missing transition means implicit rejection) and the equivalence oracle
            //     requires total DFAs.
            let mut ours = ours.clone();
            ours.sort_label();
            ours.fa.totalize(0);
            expected_automaton.fa.totalize(0);
            match automaton_language_equivalent(&ours, &expected_automaton) {
                Ok(true) => Verdict::Pass,
                Ok(false) => Verdict::fail(
                    FailedHalf::Automaton,
                    "automaton language differs from the recorded one (semantic equivalence)",
                ),
                Err(e) => Verdict::fail(
                    FailedHalf::Automaton,
                    format!("equivalence oracle refused the comparison: {e:?}"),
                ),
            }
        }
    };
    fold_failures(failures, automaton_verdict)
}

/// Combines the mismatches collected so far with the automaton comparison's own verdict.
///
/// `Verdict::Pass` only survives when nothing was collected; otherwise every reason is
/// reported together, so a `details` divergence can never hide an automaton one (which is
/// exactly what the old short-circuit did).
///
/// The per-reason [`FailedHalf`] tags are carried through rather than flattened into one
/// string, because the gate needs them: a [`KNOWN_DIVERGENCES`] entry is honored only for a
/// failure whose every reason is [`FailedHalf::Text`]. Combining a text reason with an
/// automaton one therefore produces a failure no known entry can excuse — which is the
/// whole point of collecting both instead of short-circuiting on the first.
fn fold_failures(mut failures: Vec<(FailedHalf, String)>, last: Verdict) -> Verdict {
    if let Verdict::Fail(reasons) = last {
        failures.extend(reasons);
    }
    if failures.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Fail(failures)
    }
}

fn build_report(
    outcomes: &[Outcome],
    prelude_failures: &[String],
    prelude: Duration,
    total: Duration,
) -> String {
    let mut s = String::new();
    let count = |tag: &str| outcomes.iter().filter(|o| o.verdict.tag() == tag).count();
    let _ = writeln!(s, "=== Tier-1 golden corpus ===");
    let _ = writeln!(
        s,
        "prelude ({} commands): {:.1}s",
        PRELUDE.len(),
        prelude.as_secs_f64()
    );
    let _ = writeln!(
        s,
        "fixtures: {} | pass {} | fail {} | skip {} | timeout {} | not-run {} | {:.1}s",
        outcomes.len(),
        count("PASS"),
        count("FAIL"),
        count("SKIP"),
        count("TIMEOUT"),
        count("NOT-RUN"),
        total.as_secs_f64()
    );
    if !prelude_failures.is_empty() {
        let _ = writeln!(s, "\n-- prelude failures --");
        for f in prelude_failures {
            let _ = writeln!(s, "  {f}");
        }
    }

    let mut skip_reasons: BTreeMap<String, usize> = BTreeMap::new();
    for o in outcomes {
        if let Verdict::Skipped(r) = &o.verdict {
            let key = match r {
                Excluded::DropScope(reasons) => format!(
                    "skip-drop-scope[{}]",
                    reasons
                        .iter()
                        .map(|r| r.split(':').next().unwrap_or(r).to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                other => other.to_string(),
            };
            *skip_reasons.entry(key).or_default() += 1;
        }
    }
    let _ = writeln!(s, "\n-- skip breakdown --");
    for (reason, n) in &skip_reasons {
        let _ = writeln!(s, "  {n:>4}  {reason}");
    }

    let notes: Vec<&Outcome> = outcomes.iter().filter(|o| !o.notes.is_empty()).collect();
    if !notes.is_empty() {
        let _ = writeln!(s, "\n-- partial comparisons (compared, but not in full) --");
        for o in notes {
            let _ = writeln!(s, "  fixture {:>3}: {}", o.id, o.notes.join("; "));
        }
    }

    let mut slow: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| o.elapsed > Duration::from_secs(5))
        .collect();
    slow.sort_by_key(|o| std::cmp::Reverse(o.elapsed));
    if !slow.is_empty() {
        let _ = writeln!(
            s,
            "\n-- slowest fixtures (>5s; per-fixture cap {MAX_FIXTURE_SECS}s) --"
        );
        for o in slow.iter().take(25) {
            let _ = writeln!(
                s,
                "  fixture {:>3}: {:>7.1}s  {}",
                o.id,
                o.elapsed.as_secs_f64(),
                truncate(&o.command, 90)
            );
        }
    }

    let failures: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| matches!(o.verdict, Verdict::Fail(_) | Verdict::Timeout(_)))
        .collect();
    if !failures.is_empty() {
        let _ = writeln!(s, "\n-- divergences --");
        for o in &failures {
            let why = match &o.verdict {
                Verdict::Fail(_) => o.verdict.message(),
                Verdict::Timeout(cap) => format!(
                    "TIMED OUT: no answer within the {:.0}s cap (worker thread abandoned)",
                    cap.as_secs_f64()
                ),
                _ => unreachable!(),
            };
            let _ = writeln!(
                s,
                "\n[{}] fixture {}\n  command: {}\n  {}",
                o.verdict.tag(),
                o.id,
                truncate(&o.command, 200),
                why.replace('\n', "\n  ")
            );
        }
    }
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}
