// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **Tier-3 differential generator — Milestone 0** (`docs/DESIGN.md` §5, Tier 3; Phase 4 U29).
//!
//! Generates Walnut query strings from a recorded seed, evaluates each one on BOTH engines,
//! and compares the results:
//!
//! * the Rust port, **in-process** through `wr_cli::Prover::dispatch_for_integration_test`
//!   (never a subprocess — same discipline as `tests/golden`);
//! * real `walnut-java`, through ONE long-lived JVM child driven over a NUL-delimited pipe
//!   (`java/DiffGenDriver.java`; protocol and recipe in `../CAPTURE.md`). Java's `Session` is
//!   entirely `static`, so one JVM process *is* one Walnut session — hence one child for the
//!   whole batch rather than one per query, which would put JVM startup (~0.4s) on every
//!   single query.
//!
//! Automata are compared by `wr_core::equiv` **semantic language equivalence**, never
//! structurally (`CLAUDE.md`'s prime directive). Error messages are compared by **exact**
//! string equality, with no normalization — the port reproduces Java's exception text
//! byte-for-byte, and `tests/golden`'s own `compare_error` is an exact `assertEquals` too.
//!
//! **Collect, don't abort.** A divergence is recorded and the run continues; the final
//! assertion fires once, at the end, over the whole accumulated log. That is the difference
//! between "there are N divergences, here they are" and "there is at least one".
//!
//! # What Milestone 0 is for
//!
//! Three questions, none of which the plan could answer from static reading:
//! 1. what is the REAL throughput (the plan's "~0.27ms/query" was an unverified guess);
//! 2. does Java's static `Session` degrade over a long single-JVM lifetime;
//! 3. does the kill-restart-resync path actually fire correctly.
//!
//! # Running it
//!
//! ```text
//! cargo test -p wr-differential-gen --release -- --ignored --nocapture
//! WR_DIFFGEN_QUERIES=200 WR_DIFFGEN_SEED=0x1234 cargo test -p wr-differential-gen \
//!     --release -- --ignored --nocapture
//! ```
//!
//! It is `#[ignore]`d (gated-slow tier), and **fails loudly** — never silently skips — when
//! the `walnut-java` oracle is absent.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::time::{Duration, Instant};

use support::{
    compare_non_automaton, log_line, walnut_java_dir, Answer, Generator, JavaOracle, OracleError,
    SplitMix64, Verdict,
};
use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

/// Milestone 0's size. **Not** the plan's 10^5 — that comes after these results are reviewed.
const DEFAULT_QUERIES: u64 = 10_000;

/// The seed used when `WR_DIFFGEN_SEED` is unset. ASCII `WALNUTR0`, so a report that quotes
/// it is self-identifying.
const DEFAULT_SEED: u64 = 0x5741_4C4E_5554_5230;

/// Per-query wall-clock budget on the JVM side. Generously over the observed per-query cost
/// (see the report this run prints): it exists to catch a genuine hang, not to police jitter.
const JAVA_TIMEOUT: Duration = Duration::from_secs(20);
/// Per-query wall-clock budget on the Rust side. See [`rust_verdict`] for the (documented,
/// Milestone-0-only) limitation of this mechanism.
const RUST_TIMEOUT: Duration = Duration::from_secs(20);

/// Heap cap for the JVM child, so a state-exploding query becomes a prompt `OutOfMemoryError`
/// (reported as `skip-too-big`) rather than swapping the machine.
const JVM_HEAP_MB: u32 = 2048;

/// Stack for a query's Rust worker thread. `wr-logic`'s evaluator recurses over the parse
/// tree and `wr-core`'s product/determinize recurse over states; the default 2 MiB is enough
/// for the generator's shallow queries, but a generous stack keeps a stack overflow from being
/// mistaken for a port defect.
const WORKER_STACK: usize = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

#[test]
#[ignore = "gated-slow (DESIGN.md §5): needs a built walnut-java oracle jar and a JDK"]
fn milestone_0_soak() {
    let seed = env_u64("WR_DIFFGEN_SEED").unwrap_or(DEFAULT_SEED);
    let total = env_u64("WR_DIFFGEN_QUERIES").unwrap_or(DEFAULT_QUERIES);

    // The absent-oracle contract (`CLAUDE.md`; same as `tests/golden`): LOUD, never a silent
    // skip and never a silent pass.
    if let Err(why) = support::oracle_jar() {
        panic!(
            "Tier-3 differential generator cannot run without the walnut-java oracle.\n{why}\n\
             (The sibling checkout was resolved to {})",
            walnut_java_dir().display()
        );
    }

    let scratch = scratch_root(seed);
    let jvm_dir = scratch.join("jvm");
    let stage_dir = scratch.join("stage");
    let rust_home = scratch.join("rust");
    let rust_session = rust_home.join("Session");
    for d in [&jvm_dir, &stage_dir, &rust_home, &rust_session] {
        fs::create_dir_all(d).unwrap_or_else(|e| panic!("creating {}: {e}", d.display()));
    }
    // The Rust-side session tree, created once. Headless `eval "…";` writes nothing (Java's
    // `EvalDef.computeHeadless`, and the port's), but `Session` resolves paths against it.
    for sub in [
        "Automata Library",
        "Word Automata Library",
        "Custom Bases",
        "Macro Library",
        "Morphism Library",
        "Transducer Library",
        "Result",
    ] {
        for base in [&rust_home, &rust_session] {
            fs::create_dir_all(base.join(sub)).unwrap();
        }
    }

    let log_dir = target_dir().join("diffgen");
    fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join(format!("run-{seed:#018x}.log"));
    let mut log = fs::File::create(&log_path)
        .unwrap_or_else(|e| panic!("creating {}: {e}", log_path.display()));

    println!("=== Tier-3 differential generator — Milestone 0 ===");
    println!("seed        : {seed:#018x}");
    println!("queries     : {total}");
    println!("oracle      : {}", walnut_java_dir().display());
    println!("log         : {}", log_path.display());
    println!();

    let started = Instant::now();
    let mut oracle = JavaOracle::start(&jvm_dir, JVM_HEAP_MB)
        .unwrap_or_else(|e| panic!("starting the JVM oracle: {e}"));
    let jvm_startup = started.elapsed();
    println!(
        "jvm ready in {:.2}s (compile + spawn)",
        jvm_startup.as_secs_f64()
    );

    let mut rng = SplitMix64::new(seed);
    let mut matches = 0u64;
    let mut skips = 0u64;
    let mut divergences: Vec<(u64, String, String)> = Vec::new();
    let mut oracle_timeouts = 0u64;
    let mut oracle_protocol_faults = 0u64;
    // Per-kind counts of what the ORACLE answered. Without this, a run in which every query
    // happened to be rejected by both engines would report "10,000 match" and look like
    // strong evidence while having compared no automaton at all — a false green.
    let mut by_kind: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut oracle_fatals = 0u64;
    let mut fatal_restart_failures: Vec<String> = Vec::new();

    // BOTH paths carry a trailing separator, matching Java's own `Session.setPathsAndNames`
    // and `tests/golden`'s `build_session_tree` (which returns both that way). Without it on
    // `session_dir`, the port concatenates and writes into `…/rustSessionResult/` instead of
    // `…/rust/Session/Result/` — observed live before this fix.
    let session_dir = format!("{}/", rust_session.to_string_lossy());
    let home_dir = format!("{}/", rust_home.to_string_lossy());

    let query_loop = Instant::now();
    for index in 0..total {
        // `query_id` is a fresh monotone value, not the index, so a stale response from a
        // killed child can never coincidentally match a later request.
        let query_id = index + 1;
        let query = Generator::new(&mut rng).query();

        let java = match oracle.ask(query_id, &query, JAVA_TIMEOUT) {
            Ok(a) => a,
            Err(OracleError::Timeout) => {
                oracle_timeouts += 1;
                let v = Verdict::SkipTooBig(format!(
                    "the JVM oracle did not answer within {}s; killed and restarted",
                    JAVA_TIMEOUT.as_secs()
                ));
                skips += 1;
                write_log(&mut log, seed, index, query_id, &query, "<timeout>", &v);
                println!("[{index}] skip-too-big (jvm timeout): {query}");
                continue;
            }
            Err(OracleError::Protocol(why)) => {
                oracle_protocol_faults += 1;
                let v = Verdict::SkipTooBig(format!("oracle protocol fault: {why}"));
                skips += 1;
                write_log(&mut log, seed, index, query_id, &query, "<fault>", &v);
                println!("[{index}] skip-too-big (oracle protocol fault: {why}): {query}");
                continue;
            }
        };

        let oracle_kind = java.kind();
        *by_kind.entry(oracle_kind).or_default() += 1;

        // A JVM `Error` (OOM / StackOverflow) is a well-formed protocol response, so `ask`
        // does NOT restart the child for it — but `DiffGenDriver` catches `Throwable` broadly
        // and keeps looping, which leaves every later query answered by a JVM with a wounded
        // heap and a static `Session` of unknown state. Restart it here, before the next
        // query. A failed restart is recorded and reported, not panicked over: the run's own
        // gate below will notice if the oracle is genuinely gone.
        if matches!(java, Answer::Fatal(_)) {
            oracle_fatals += 1;
            if let Err(e) = oracle.respawn() {
                fatal_restart_failures.push(format!("index {index}: {e}"));
                println!("[{index}] WARNING: restart after a JVM error failed: {e}");
            }
        }

        let verdict = rust_verdict(&session_dir, &home_dir, &stage_dir, query_id, &query, java);
        match &verdict {
            Verdict::Match => matches += 1,
            Verdict::SkipTooBig(_) => skips += 1,
            Verdict::Divergence(d) => divergences.push((index, query.clone(), d.clone())),
        }
        write_log(
            &mut log,
            seed,
            index,
            query_id,
            &query,
            oracle_kind,
            &verdict,
        );
        if let Verdict::Divergence(d) = &verdict {
            println!("[{index}] DIVERGENCE\n  query: {query}\n{}", indent(d));
        }
        if let Verdict::SkipTooBig(d) = &verdict {
            println!("[{index}] skip-too-big ({d}): {query}");
        }
        if index > 0 && index % 500 == 0 {
            let rate = index as f64 / query_loop.elapsed().as_secs_f64();
            println!(
                "  … {index}/{total}  ({matches} match, {} divergence, {skips} skip)  {rate:.1} q/s",
                divergences.len()
            );
        }
    }
    let elapsed = query_loop.elapsed();
    let _ = log.flush();

    println!();
    println!("=== report ===");
    println!("queries          : {total}");
    println!("match            : {matches}");
    println!("divergence       : {}", divergences.len());
    println!("skip-too-big     : {skips}");
    println!("  jvm timeouts   : {oracle_timeouts}");
    println!("  protocol faults: {oracle_protocol_faults}");
    println!("  jvm errors     : {oracle_fatals}");
    println!("jvm restarts     : {}", oracle.restarts);
    println!("failed restarts  : {}", fatal_restart_failures.len());
    println!("oracle answers by kind:");
    for (kind, n) in &by_kind {
        println!("  {kind:<10}: {n}");
    }
    println!(
        "wall clock       : {:.1}s (+{:.1}s jvm startup)",
        elapsed.as_secs_f64(),
        jvm_startup.as_secs_f64()
    );
    println!(
        "throughput       : {:.1} queries/sec ({:.2} ms/query)",
        total as f64 / elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / total as f64
    );
    println!("log              : {}", log_path.display());

    if !divergences.is_empty() {
        let mut report = String::new();
        for (index, query, detail) in &divergences {
            report.push_str(&format!(
                "\n--- index {index} ---\n  query: {query}\n{}",
                indent(detail)
            ));
        }
        panic!(
            "{} of {total} generated queries diverged from real walnut-java (seed {seed:#018x}):{report}",
            divergences.len()
        );
    }

    assert_healthy_run(
        total,
        matches,
        skips,
        divergences.len() as u64,
        oracle_protocol_faults,
        &fatal_restart_failures,
        by_kind.get("automaton").copied().unwrap_or(0),
    );
}

/// The **anti-false-green gate**, run after the divergence check.
///
/// "No divergences" alone is not evidence of anything. If the oracle degrades mid-run — a JVM
/// that OOMs and stays wounded, a wedged child, a stale jar — every query becomes a
/// `skip-too-big`, the divergence list stays empty, and this test reports PASS having compared
/// literally nothing. So the run must also look like a run that DID the work.
///
/// **A failure here says the HARNESS or the ORACLE is broken, not that the port is.** It is a
/// statement about the shape of the run, not about walnut-rs's semantics — the port-defect
/// signal is the divergence panic above, which has already fired by this point if it applies.
///
/// The thresholds:
/// * **accounting is total** — every query ends up in exactly one bucket. Not a health
///   threshold but an invariant: if it fails, the loop's bookkeeping is wrong and every other
///   number printed above (including "0 divergences") is untrustworthy.
/// * **`matches > 0`** — the minimal proof that the comparison path ran at all.
/// * **at least half the oracle's answers were automata** — the point of a Tier-3 run is to
///   exercise `wr_core::equiv` against real automata; a run answered entirely in `true`/
///   `false`/`error` compares no transition table. Observed on healthy runs: ~99% (the
///   generator always keeps at least one free variable in scope, so an automaton is the
///   overwhelmingly common answer), so 50% is a floor with a very wide margin — it fires on a
///   degraded oracle, never on generator jitter.
/// * **skips below 5%** — a skip is a query that was NOT compared. Healthy runs observe 0
///   (the generator is held to `CLAUDE.md`'s "generate SMALL", so nothing should approach
///   either engine's 20s budget); 5% leaves room for machine-load jitter on a loaded CI box
///   while still catching the "the oracle died and everything became a skip" failure, which
///   drives this to ~100%.
/// * **zero protocol faults** — a desynchronized or malformed pipe is never normal. Unlike a
///   timeout it is not a resource verdict; it means the harness and the driver disagree about
///   the protocol, which invalidates the answers on BOTH sides of the boundary.
/// * **zero failed restarts** — a restart that could not be performed leaves the rest of the
///   run talking to a dead or wounded JVM.
#[allow(clippy::too_many_arguments)]
fn assert_healthy_run(
    total: u64,
    matches: u64,
    skips: u64,
    divergences: u64,
    oracle_protocol_faults: u64,
    fatal_restart_failures: &[String],
    automaton_answers: u64,
) {
    const BROKEN: &str = "This is a BROKEN-HARNESS/BROKEN-ORACLE signal, not necessarily a port \
                          defect: the run did not actually compare what it claims to have \
                          compared, so its `0 divergences` result is not evidence of anything.";

    assert_eq!(
        matches + skips + divergences,
        total,
        "verdict accounting does not add up: {matches} match + {skips} skip + {divergences} \
         divergence != {total} queries. {BROKEN}"
    );
    assert!(
        matches > 0,
        "not a single query MATCHED across {total} queries. {BROKEN}"
    );
    assert!(
        automaton_answers * 2 >= total,
        "the oracle answered with an automaton for only {automaton_answers} of {total} queries \
         (< 50%), so the semantic-equivalence oracle barely ran. {BROKEN}"
    );
    assert!(
        skips * 20 <= total,
        "{skips} of {total} queries were skipped (> 5%) — each one is a query that was NOT \
         compared. {BROKEN}"
    );
    assert_eq!(
        oracle_protocol_faults, 0,
        "{oracle_protocol_faults} oracle protocol faults (a desynchronized or malformed pipe). \
         {BROKEN}"
    );
    assert!(
        fatal_restart_failures.is_empty(),
        "the JVM could not be restarted after a JVM error, so the rest of the run was answered \
         by an untrusted child: {fatal_restart_failures:?}. {BROKEN}"
    );
}

// ---------------------------------------------------------------------------
// The Rust side + the comparison
// ---------------------------------------------------------------------------

/// Evaluates `query` on the Rust port and compares against the JVM's `java` answer.
///
/// # Timeout mechanism — a documented Milestone-0 stopgap
///
/// The evaluation runs on a spawned thread and is abandoned (not killed) if it overruns
/// [`RUST_TIMEOUT`]: Rust has no portable way to kill a running thread. That is acceptable
/// HERE because (a) the generator's queries are small by construction, (b) headless `eval`
/// writes nothing to disk, so an abandoned worker cannot corrupt the session tree that later
/// queries read (the taint hazard `tests/golden/STATUS.md` had to halt the run over), and (c)
/// each worker builds its own `Prover`, sharing no mutable state with the next query.
/// It is NOT production-grade: an abandoned worker keeps burning a core, so a run with many
/// timeouts degrades. **Scaling to 10^5 wants the plan's process-based cap** (`proptest`'s
/// `fork`+`timeout`, or a `Command`-spawn-per-query with `Child::kill`).
fn rust_verdict(
    session_dir: &str,
    home_dir: &str,
    scratch_dir: &Path,
    query_id: u64,
    query: &str,
    java: Answer,
) -> Verdict {
    let (tx, rx) = sync_channel::<Verdict>(1);
    let session_dir = session_dir.to_string();
    let home_dir = home_dir.to_string();
    let scratch_dir = scratch_dir.to_path_buf();
    let query = query.to_string();

    let spawned = std::thread::Builder::new()
        .stack_size(WORKER_STACK)
        .name(format!("diffgen:{query}"))
        .spawn(move || {
            let v = panic::catch_unwind(AssertUnwindSafe(|| {
                evaluate_and_compare(
                    &session_dir,
                    &home_dir,
                    &scratch_dir,
                    query_id,
                    &query,
                    &java,
                )
            }))
            .unwrap_or_else(|payload| {
                // A panic in the port is a real defect (the U26/U27 "unguarded `act()`"
                // precedent), never a `skip` — real Walnut throws a catchable exception.
                Verdict::Divergence(format!(
                    "the Rust port PANICKED where java answered {}: {}",
                    java_kind(&java),
                    panic_message(&*payload)
                ))
            });
            let _ = tx.send(v);
        });
    let handle = match spawned {
        Ok(h) => h,
        Err(e) => return Verdict::Divergence(format!("harness could not spawn a worker: {e}")),
    };

    match rx.recv_timeout(RUST_TIMEOUT) {
        Ok(v) => {
            let _ = handle.join();
            v
        }
        Err(RecvTimeoutError::Timeout) => Verdict::SkipTooBig(format!(
            "the Rust port did not finish within {}s (worker abandoned — see `rust_verdict`)",
            RUST_TIMEOUT.as_secs()
        )),
        Err(RecvTimeoutError::Disconnected) => {
            Verdict::Divergence("the Rust worker died without a verdict".to_string())
        }
    }
}

/// What the Rust port produced, before comparison. Deliberately mirrors the JVM driver's own
/// dispatch (`java/DiffGenDriver.java`) arm for arm, so a kind mismatch is a real
/// disagreement and not an artifact of the two harness halves classifying differently.
enum RustSide {
    Automaton(Box<Automaton>),
    True,
    False,
    Error(String),
    Malformed(String),
}

fn evaluate_and_compare(
    session_dir: &str,
    home_dir: &str,
    scratch_dir: &Path,
    query_id: u64,
    query: &str,
    java: &Answer,
) -> Verdict {
    // A JVM `Error` is never a semantic verdict — bail before spending Rust time on it.
    if let Answer::Fatal(why) = java {
        return Verdict::SkipTooBig(format!("JVM error: {why}"));
    }

    let session = Session::new(Some(session_dir), Some(home_dir), false);
    // Both consoles are sunk, mirroring the JVM driver muting `System.out` for its whole
    // lifetime: neither engine's incidental chatter belongs on the harness's own report
    // stream. (`Logging::new()` writes to the REAL process stdout/stderr, so passing it here
    // would interleave the port's console output with this run's verdicts.)
    let logging = Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink()));
    let mut prover = Prover::with_output(session, logging, Box::new(std::io::sink()));
    let command = format!("eval \"{query}\";");

    let rust = match prover.dispatch_for_integration_test(&command, "") {
        Err(e) => RustSide::Error(e.to_string()),
        Ok(None) => RustSide::Malformed("dispatch returned no TestCase".to_string()),
        Ok(Some(tc)) => {
            let pairs = tc.automaton_pairs();
            if pairs.len() != 1 {
                RustSide::Malformed(format!("unexpected automaton-pair count {}", pairs.len()))
            } else {
                match pairs[0].automaton() {
                    None => RustSide::Malformed("null automaton in pair".to_string()),
                    Some(a) if a.fa.is_true_false_automaton() => {
                        if a.fa.is_true_automaton() {
                            RustSide::True
                        } else {
                            RustSide::False
                        }
                    }
                    Some(a) => RustSide::Automaton(Box::new(a.clone())),
                }
            }
        }
    };

    // Route through the shared comparator for everything that is not automaton-vs-automaton,
    // so the harness has exactly ONE place that decides what counts as a match.
    let rust_answer = match &rust {
        RustSide::Automaton(a) => Answer::Automaton(render_txt(a)),
        RustSide::True => Answer::True,
        RustSide::False => Answer::False,
        RustSide::Error(m) => Answer::Error(m.clone()),
        RustSide::Malformed(m) => {
            return Verdict::Divergence(format!("the Rust port returned a malformed result: {m}"))
        }
    };
    if let Some(v) = compare_non_automaton(java, &rust_answer) {
        return v;
    }

    let (Answer::Automaton(java_txt), RustSide::Automaton(ours)) = (java, &rust) else {
        unreachable!("compare_non_automaton returns None only for automaton-vs-automaton");
    };
    compare_automata(java_txt, ours, scratch_dir, query_id)
}

/// Semantic language equivalence between the JVM's serialized automaton and the port's.
///
/// Two normalizations first, both established by `tests/golden`'s own comparator and by the
/// Tier-3 fixture suite — neither can hide a port defect:
/// * `sort_label()` on ours, because `AutomatonWriter.writeTxtFormatToStream` canonizes (and
///   therefore SORTS the track label) before writing, and `wr_core::equiv`'s track comparison
///   is positional (its own documented gap: a label permutation with matching per-position
///   alphabets is NOT detected, so the two must be brought into the same order first);
/// * `totalize(0)` on both, because Walnut `.txt` automata need not be total (a missing
///   transition means implicit rejection) while the equivalence oracle requires total DFAs.
///
/// # Why the staging file is named after `query_id`
///
/// The path MUST be unique per query. [`rust_verdict`]'s timeout abandons its worker rather
/// than killing it (documented stopgap), so a worker from an earlier, timed-out query can
/// still reach this code while a later query is running. On a single shared path the two race:
/// the loser reads the winner's serialized automaton and compares the port's answer for one
/// query against the ORACLE'S ANSWER FOR ANOTHER — which is a phantom divergence at best and,
/// if the two happen to agree, a real divergence silently recorded as a `Match`. `query_id` is
/// monotone and never reused within a run, so no two live workers can collide.
fn compare_automata(
    java_txt: &str,
    ours: &Automaton,
    scratch_dir: &Path,
    query_id: u64,
) -> Verdict {
    // `wr-io`'s reader is path-based only today (the plan's U30 notes this and proposes a
    // `&str` entry point); one small rewritten file per query is the Milestone-0 stopgap.
    let staged = scratch_dir.join(format!("java-result-{query_id}.txt"));
    if let Err(e) = fs::write(&staged, java_txt) {
        return Verdict::Divergence(format!("harness could not stage the JVM's automaton: {e}"));
    }
    let parsed = wr_io::reader::read_automaton_txt(&staged);
    // Removed as soon as it has been read, so a 10^4-query run does not leave 10^4 files
    // behind. Best-effort: a failed removal is not a verdict, only clutter.
    let _ = fs::remove_file(&staged);
    let mut theirs = match parsed {
        Ok(a) => a,
        Err(e) => {
            return Verdict::Divergence(format!(
                "the JVM's own serialized automaton did not parse: {e:?}\n{java_txt}"
            ))
        }
    };
    let mut ours = ours.clone();
    ours.sort_label();
    ours.fa.totalize(0);
    theirs.fa.totalize(0);
    match automaton_language_equivalent(&ours, &theirs) {
        Ok(true) => Verdict::Match,
        Ok(false) => Verdict::Divergence(format!(
            "automaton languages differ (semantic equivalence)\n  --- java ---\n{}\n  --- rust ---\n{}",
            indent(java_txt),
            indent(&render_txt(&ours))
        )),
        Err(e) => Verdict::Divergence(format!(
            "the equivalence oracle refused the comparison: {e:?}\n  --- java ---\n{}\n  --- rust ---\n{}",
            indent(java_txt),
            indent(&render_txt(&ours))
        )),
    }
}

/// `AutomatonWriter.writeTxtFormatToStream`'s Rust counterpart, to a `String` — used only for
/// divergence REPORTING. The comparison itself never touches this text.
fn render_txt(a: &Automaton) -> String {
    let mut a = a.clone();
    let mut buf: Vec<u8> = Vec::new();
    match wr_io::writer::write_txt(&mut a, &mut buf) {
        Ok(()) => String::from_utf8_lossy(&buf).into_owned(),
        Err(e) => format!("<could not serialize the Rust automaton: {e}>"),
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn java_kind(a: &Answer) -> &'static str {
    a.kind()
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

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("    {l}\n")).collect()
}

#[allow(clippy::too_many_arguments)]
fn write_log(
    log: &mut fs::File,
    seed: u64,
    index: u64,
    query_id: u64,
    query: &str,
    oracle_kind: &str,
    verdict: &Verdict,
) {
    let line = log_line(seed, index, query_id, query, oracle_kind, verdict);
    let _ = log.write_all(line.as_bytes());
}

/// `WR_DIFFGEN_*` overrides, accepting decimal or `0x`-prefixed hex.
fn env_u64(name: &str) -> Option<u64> {
    let raw = std::env::var(name).ok()?;
    let raw = raw.trim();
    let parsed = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => raw.parse::<u64>(),
    };
    Some(parsed.unwrap_or_else(|e| panic!("{name}={raw:?} is not a number: {e}")))
}

/// `<workspace>/target`, derived from this crate's manifest directory (which is
/// `<workspace>/tests/differential-gen`, or the worktree equivalent).
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .to_path_buf()
}

/// A per-seed scratch tree for both engines' session directories. Recreated from scratch on
/// every run so a previous run's leftovers cannot influence this one.
fn scratch_root(seed: u64) -> PathBuf {
    let root = std::env::temp_dir().join(format!("wr-diffgen-{seed:#018x}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|e| panic!("creating {}: {e}", root.display()));
    root
}

// ---------------------------------------------------------------------------
// Live-oracle self-checks
// ---------------------------------------------------------------------------

/// Proves the harness can FAIL, and that a hang is survivable — Milestone 0's goals (c) and
/// the "is this run a false green?" question.
///
/// Three checks, all against the real JVM:
/// 1. an automaton comparison of two genuinely different predicates must report a divergence
///    (i.e. `compare_automata` is not vacuously passing);
/// 2. an ask with an unsatisfiable deadline must time out, kill the child and restart it;
/// 3. the very next ask must answer correctly with a matching `query_id` echo — the resync
///    invariant that keeps a restart from turning into a flood of phantom divergences.
#[test]
#[ignore = "gated-slow (DESIGN.md §5): needs a built walnut-java oracle jar and a JDK"]
fn the_harness_detects_divergence_and_survives_a_hang() {
    if let Err(why) = support::oracle_jar() {
        panic!("Tier-3 differential generator cannot run without the walnut-java oracle.\n{why}");
    }
    let scratch = scratch_root(0xDEAD_BEEF_DEAD_BEEF);
    let jvm_dir = scratch.join("jvm");
    let stage_dir = scratch.join("stage");
    let rust_home = scratch.join("rust");
    let rust_session = rust_home.join("Session");
    for d in [&jvm_dir, &stage_dir, &rust_home, &rust_session] {
        fs::create_dir_all(d).unwrap();
    }
    // Trailing separator on BOTH, exactly as the run itself builds them (see `milestone_0_soak`).
    let session_dir = format!("{}/", rust_session.to_string_lossy());
    let home_dir = format!("{}/", rust_home.to_string_lossy());

    let mut oracle = JavaOracle::start(&jvm_dir, JVM_HEAP_MB).expect("starting the JVM oracle");

    // (1) A real mismatch must be reported as one.
    let java = oracle
        .ask(1, "?msd_2 x < 3", JAVA_TIMEOUT)
        .expect("the oracle should answer `?msd_2 x < 3`");
    assert!(
        matches!(java, Answer::Automaton(_)),
        "expected an automaton for a one-free-variable predicate, got {java:?}"
    );
    let same = evaluate_and_compare(
        &session_dir,
        &home_dir,
        &stage_dir,
        1,
        "?msd_2 x < 3",
        &java,
    );
    assert_eq!(same, Verdict::Match, "the identical predicate must match");
    let different = evaluate_and_compare(
        &session_dir,
        &home_dir,
        &stage_dir,
        2,
        "?msd_2 x < 4",
        &java,
    );
    assert!(
        matches!(different, Verdict::Divergence(_)),
        "comparing `x < 4` against the oracle's `x < 3` must diverge, got {different:?}"
    );

    // (2) A deadline the child cannot possibly meet must time out and restart it.
    let restarts_before = oracle.restarts;
    let hung = oracle.ask(2, "?msd_2 x < 3", Duration::from_nanos(1));
    assert!(
        matches!(hung, Err(OracleError::Timeout)),
        "a 1ns deadline must report a timeout, got {hung:?}"
    );
    assert_eq!(
        oracle.restarts,
        restarts_before + 1,
        "a timeout must kill and restart the JVM child"
    );

    // (3) The next query must be answered by the FRESH child, correctly and in sync — a stale
    // response from the killed child must not be mistaken for this one's answer.
    let after = oracle
        .ask(3, "?msd_2 x < 3", JAVA_TIMEOUT)
        .expect("the restarted oracle must answer");
    assert_eq!(
        after, java,
        "the restarted oracle must give the same answer"
    );
    let closed = oracle
        .ask(4, "?msd_2 Ax (x >= 0)", JAVA_TIMEOUT)
        .expect("the restarted oracle must keep answering");
    assert_eq!(closed, Answer::True, "`Ax (x >= 0)` is TRUE over msd_2");
}

// ---------------------------------------------------------------------------
// Fast-tier self-checks (no JVM, no oracle)
// ---------------------------------------------------------------------------

#[test]
fn env_u64_accepts_decimal_and_hex() {
    std::env::set_var("WR_DIFFGEN_TEST_ONLY", "0x1f");
    assert_eq!(env_u64("WR_DIFFGEN_TEST_ONLY"), Some(31));
    std::env::set_var("WR_DIFFGEN_TEST_ONLY", " 42 ");
    assert_eq!(env_u64("WR_DIFFGEN_TEST_ONLY"), Some(42));
    std::env::remove_var("WR_DIFFGEN_TEST_ONLY");
    assert_eq!(env_u64("WR_DIFFGEN_TEST_ONLY"), None);
}

/// The anti-false-green gate must accept a run that plainly did the work…
#[test]
fn the_health_gate_accepts_a_healthy_run() {
    assert_healthy_run(10_000, 10_000, 0, 0, 0, &[], 9_900);
    // A handful of skips and a lopsided-but-legitimate answer mix is still healthy.
    assert_healthy_run(10_000, 9_600, 400, 0, 0, &[], 5_000);
}

/// …and REJECT every shape of a degraded one. Each of these previously reported PASS, because
/// the only assertion was "the divergence list is empty" — which an oracle that answers
/// nothing satisfies perfectly.
#[test]
fn the_health_gate_rejects_a_degraded_run() {
    let cases: [(&str, fn()); 6] = [
        // The exact false green the reviewers found: the oracle died, every query became a
        // skip, zero divergences, PASS.
        ("every query skipped", || {
            assert_healthy_run(10_000, 0, 10_000, 0, 0, &[], 0)
        }),
        // Degraded but not dead: 90% skipped is still a run that compared almost nothing.
        ("most queries skipped", || {
            assert_healthy_run(10_000, 1_000, 9_000, 0, 0, &[], 1_000)
        }),
        // The oracle answered, but never with an automaton — the equivalence oracle never ran.
        ("no automata compared", || {
            assert_healthy_run(10_000, 10_000, 0, 0, 0, &[], 100)
        }),
        ("protocol faults", || {
            assert_healthy_run(10_000, 9_999, 1, 0, 1, &[], 9_900)
        }),
        ("a restart that failed", || {
            assert_healthy_run(
                10_000,
                10_000,
                0,
                0,
                0,
                &["index 5: no jvm".to_string()],
                9_900,
            )
        }),
        ("buckets that do not add up", || {
            assert_healthy_run(10_000, 9_000, 0, 0, 0, &[], 9_000)
        }),
    ];
    for (what, run) in cases {
        let outcome = panic::catch_unwind(AssertUnwindSafe(run));
        assert!(
            outcome.is_err(),
            "the health gate accepted a degraded run ({what}) — that is the false green this \
             gate exists to prevent"
        );
    }
}

#[test]
fn indent_prefixes_every_line() {
    assert_eq!(indent("a\nb"), "    a\n    b\n");
    assert_eq!(indent(""), "");
}

/// The absent-oracle contract is a hard failure, and the message must name the resolved
/// checkout — a silent skip is the one outcome `CLAUDE.md` forbids.
#[test]
fn the_absent_oracle_message_is_actionable() {
    let jar = walnut_java_dir().join("target/Walnut-all.jar");
    if jar.is_file() {
        // The oracle IS present here; the negative path is exercised by pointing the resolver
        // at a directory that certainly has no jar.
        std::env::set_var("WALNUT_JAVA_DIR", "/nonexistent-walnut-java");
        let err = support::oracle_jar().expect_err("a missing jar must be an error");
        std::env::remove_var("WALNUT_JAVA_DIR");
        assert!(err.contains("Walnut-all.jar"), "{err}");
        assert!(
            err.contains("mvnw"),
            "the message must say how to build it: {err}"
        );
    } else {
        let err = support::oracle_jar().expect_err("a missing jar must be an error");
        assert!(err.contains("Walnut-all.jar"), "{err}");
    }
}
