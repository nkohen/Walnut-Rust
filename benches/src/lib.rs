// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Shared machinery for **U32 — performance vs JVM Walnut** (`docs/DESIGN.md` §8's Phase-4
//! exit clause, "faster than Walnut on the research workloads").
//!
//! Two consumers sit on top of this module:
//!
//! * `benches/dispatch.rs` — the Criterion benchmark of the Rust side alone (statistical
//!   sampling, confidence intervals, `target/criterion` reports);
//! * `src/bin/compare.rs` — the head-to-head, which runs the **identical** warm-up +
//!   fixed-iteration loop against `wr-cli` in-process and against one long-lived warm
//!   `walnut-java` JVM, and prints the table `benches/STATUS.md` records.
//!
//! Three properties are load-bearing and are why this file exists rather than two ad hoc
//! scripts:
//!
//! 1. **Both engines run the same command over the same session state.** The workloads are
//!    real fixtures out of Walnut's own integration corpus, loaded through the same Phase-0
//!    manifests `tests/golden` uses (that crate's loader is *included*, not copied — see
//!    [`golden`]), and each engine gets its own byte-identical copy of the corpus's `Global`
//!    + `Session` library trees plus the same 19-command prelude.
//! 2. **Neither engine's startup is in the measurement.** The Rust side is a `--release`
//!    binary that has already run the same query several times; the JVM side is warmed with
//!    the same number of throwaway iterations of the same query inside the same process, and
//!    times the dispatch itself with `System.nanoTime()`. Comparing a cold JVM against a warm
//!    Rust binary would be a meaningless comparison.
//! 3. **Correctness is checked before speed is believed.** Every workload's answer is compared
//!    across the two engines by `wr_core::equiv` semantic language equivalence before any
//!    timing is reported, so the table can never be "fast but wrong vs slow but right".

use std::cell::RefCell;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use wr_cli::prover::Prover;
use wr_cli::session::{Session, SessionPaths};
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

/// `tests/golden`'s harness support module, **included** rather than copied.
///
/// It already owns everything this crate needs from the corpus — the Phase-0 manifest loader
/// and its self-checks, `build_session_tree` (the exact `Global`/`Session` layout
/// `Session.setPathsAndNamesIntegrationTests` leaves behind), and `PRELUDE` (Walnut's own
/// `IntegrationTest.initialize` prologue, transcribed verbatim). A copy here would be a
/// staleness trap: the manifests are machine-generated in the sibling repo and are
/// re-exported whenever Phase 0's tooling is re-run, and a benchmark that silently loaded a
/// *different* fixture than Tier 1 compares would be worse than no benchmark.
///
/// `walnut_java_dir()` resolves by walking UP from `CARGO_MANIFEST_DIR` looking for a sibling
/// `walnut-java`, so it works unchanged from this crate's directory.
#[allow(dead_code)]
#[path = "../../tests/golden/tests/support/mod.rs"]
pub mod golden;

// ---------------------------------------------------------------------------
// The workload set
// ---------------------------------------------------------------------------

/// One benchmarked workload: a real fixture from Walnut's integration corpus.
#[derive(Debug, Clone)]
pub struct Workload {
    pub id: usize,
    /// The literal command script, exactly as `tests/golden` replays it — metacommand prefix
    /// and `;`/`::` suffix included.
    pub command: String,
    /// Why this fixture is in the set (printed in the report, so the selection is auditable).
    pub why: &'static str,
    /// Roughly how long one warm Rust dispatch of this workload takes, in seconds — the mean
    /// measured by the last full `compare` run. Used ONLY to pick sample counts and timeout
    /// budgets; never reported as a result (the report always prints freshly measured numbers).
    ///
    /// Note this is **not** `tests/golden`'s per-fixture time for the same id, which is
    /// typically much larger: that harness's clock also covers reading the recorded expectation
    /// and running the Tier-1 `wr_core::equiv` comparison, which on a large result automaton
    /// costs far more than the query itself (fixture 261: ~0.30 s of dispatch inside ~5.2 s of
    /// golden-run wall clock).
    pub approx_secs: f64,
}

/// The chosen fixtures, roughly smallest-first. See `benches/README.md` §"Why these fixtures"
/// for the full rationale; the one-liners here are what the report prints.
///
/// Every entry is verified at run time to (a) exist in the manifest, (b) be reachable from the
/// prelude alone (no dependency on an earlier fixture's output), and (c) produce the same
/// language on both engines — a workload that fails any of those aborts the run rather than
/// being quietly dropped.
pub const WORKLOADS: [(usize, &str, f64); 10] = [
    (
        1,
        "floor: a tiny closed-form lsd_2 conjunction — measures per-command overhead \
         (parse + dispatch + number-system construction), not algorithmic throughput",
        0.0004,
    ),
    (
        207,
        "a larger base (msd_17): NumberSystem::new eagerly builds adder/comparator automata \
         over a k^3 alphabet, so this is dominated by number-system construction",
        0.0003,
    ),
    (
        293,
        "the smallest genuine word-automaton factor-equality query (period-doubling `P`), \
         one quantifier",
        0.109,
    ),
    (
        521,
        "the `I` (infinitely-often) quantifier over msd_10 — a different elimination path \
         (`wr_core::infinite`) from `E`/`A`",
        0.081,
    ),
    (
        179,
        "a MULTI-track word automaton (`PFmsd[f][i+k]`) under two quantifiers, with two \
         `reg`-defined predicates from the prelude",
        0.432,
    ),
    (
        266,
        "nested `A`/macro-call structure over Rudin-Shapiro — mid-sized alternation",
        0.111,
    ),
    (
        230,
        "deep alternation (`Ei At …`) over Thue-Morse with a `3*n` coefficient — the first \
         workload where the decision procedure, not the plumbing, dominates",
        2.153,
    ),
    (
        295,
        "paperfolding factor-equality: a large cross product under one quantifier",
        0.18,
    ),
    (
        261,
        "Rudin-Shapiro factor-equality — the same shape as 295 over a bigger word automaton",
        0.301,
    ),
    (
        286,
        "the SLOWEST fixture in the whole corpus (lsd_2 Rudin-Shapiro trapezoidal \
         factor-equality) — the closest thing Walnut's own suite has to a research workload",
        0.494,
    ),
];

/// Fixture 637, kept separate because it is the one workload whose *algorithm* is chosen by a
/// metacommand: `[strategy 6 BRZ]` (gated on the `::` detail suffix) switches the sixth
/// determinization off subset construction, which real Walnut needs to answer it at all.
///
/// It is a genuine slow-workload comparison only because U32's prerequisite unit wired
/// `[strategy N NAME]` through to `wr_core::determinize` — before that, benchmarking it would
/// have compared Rust's `SC` (does not finish) against Java's `BRZ`, an artifact of strategy
/// choice rather than a measurement of either engine.
pub const STRATEGY_WORKLOAD: (usize, &str, f64) = (
    637,
    "`[strategy 6 BRZ]`: the sixth determinization is a 1,790-state NFA that only Brzozowski \
     makes tractable — the corpus's one strategy-sensitive fixture",
    0.09,
);

/// Fixture 637's formula with the `[strategy 6 BRZ]` metacommand **removed**, so both engines
/// take the default subset construction on that sixth, 1,790-state determinization.
///
/// Not a corpus fixture and not part of the default set — it is opt-in (`WR_BENCH_SC_VARIANT=1`)
/// and reported separately, because it answers a different question: *what does the strategy
/// metacommand actually buy?* That is the question U32's prerequisite unit was built to make
/// askable, and the answer is a much larger factor than anything in the main table.
///
/// Deliberately given a fresh result name so it cannot collide with fixture 637's own library
/// entry, and kept `::`-suffixed so its `details` trace is comparable with 637's.
pub const SC_VARIANT: (&str, &str) = (
    "eval benchsc637 \"E x,y,z (n=x+y+z)&(QQ[x]=@1)&(QQ[y]=@1)&(QQ[z]=@1)\"::",
    "fixture 637's formula WITHOUT `[strategy 6 BRZ]` — the same query decided by plain subset \
     construction, i.e. what both engines do when the metacommand is not honoured",
);

/// Loads every benchmarked workload from the Phase-0 manifests, in the order they are
/// benchmarked (the ten ordinary fixtures smallest-first, then fixture 637).
pub fn workloads() -> Result<Vec<Workload>, String> {
    let fixtures = golden::load_fixtures()?;
    let mut out = Vec::with_capacity(WORKLOADS.len() + 1);
    for (id, why, approx_secs) in WORKLOADS.iter().chain(std::iter::once(&STRATEGY_WORKLOAD)) {
        let f = fixtures
            .get(*id)
            .ok_or_else(|| format!("fixture {id} is not in test-manifest.json"))?;
        if f.id != *id {
            return Err(format!("manifest row {} is not fixture {id}", f.id));
        }
        if !f.subset_relevant {
            return Err(format!(
                "fixture {id} is DROP-scope ({:?}); it cannot be a benchmark workload",
                f.drop_reason
            ));
        }
        out.push(Workload {
            id: *id,
            command: f.command_script.clone(),
            why,
            approx_secs: *approx_secs,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Peak state count
// ---------------------------------------------------------------------------

/// The largest automaton either engine reports building, read out of the `details` trace
/// (`Logging`'s "… : N states", "Determinized: N states", "Minimizing: N states." …).
///
/// `CLAUDE.md` names state blow-up, not raw speed, as the decision procedure's dominant cost
/// axis, so a wall-clock-only comparison would miss the thing that actually decides whether a
/// research query is tractable. The same parser is applied to BOTH engines' `details` text, so
/// the two numbers are commensurable even though neither is instrumentation *inside* the
/// engine — this deliberately adds no production-code hooks (U32 is measurement, not a change
/// to `wr-core`/`wr-logic`/`wr-cli`).
///
/// Lines containing `Progress:` are skipped: `Progress: Added 100 states` is a running counter
/// of states added *so far in one traversal*, not the size of an automaton, and Java's own
/// `IntegrationTest.assertEqualMessages` strips those lines before comparing details for the
/// same reason.
///
/// # The port's number is a LOWER BOUND, and the report says so
///
/// The two traces are not equally complete, and pretending otherwise would be the dishonest
/// version of this measurement. Java's `details` covers the whole computation, including every
/// `wr-core`-level step (`computing cross product:N states`, `Determinizing […]: N states`,
/// `Minimized:N states`). The port's covers only the `wr-logic`-level steps: threading
/// `&mut Logging` through `wr-core`'s product/determinize/minimize/quantify is the *known,
/// pre-existing* gap behind seven of the golden corpus's nine remaining divergences
/// (`tests/golden/STATUS.md`, the "U28" follow-up), and closing it is a production change that
/// U32 deliberately does not make.
///
/// So: Java's peak is the authoritative one; the port's is the largest automaton its partial
/// trace happens to name, i.e. a lower bound. `src/bin/compare.rs` labels the two columns
/// accordingly rather than reporting a "difference" that is really a logging gap. Both engines
/// are separately proven to compute the *same language* on every workload, so the authoritative
/// peak characterises both computations up to that verified-identical answer.
pub fn peak_states(details: &str) -> Option<u64> {
    let mut peak: Option<u64> = None;
    for line in details.lines() {
        if line.contains("Progress:") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if !bytes[i].is_ascii_digit() {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // Only a digit run immediately followed by ` state`/` states` counts.
            let rest = &line[i..];
            if rest.starts_with(" state") {
                if let Ok(n) = line[start..i].parse::<u64>() {
                    peak = Some(peak.map_or(n, |p: u64| p.max(n)));
                }
            }
        }
    }
    peak
}

/// The detail-printing form of a command: `…;` → `…::`, `…:` → `…::`, `…::` unchanged.
///
/// The peak-state measurement needs the `details` trace, which Walnut only produces for a
/// `::`-suffixed command. Detail printing does not change *which* automata get built (the one
/// exception is a `[strategy …]` metacommand, which Java gates on the same flag — and the one
/// workload that has one, fixture 637, is already `::` in the corpus), so the peak measured
/// this way is the peak of the timed computation. It is nevertheless measured in a **separate,
/// untimed** pass: writing the trace is real I/O and has no place inside a timing loop.
pub fn detail_variant(command: &str) -> String {
    let trimmed = command.trim_end();
    if let Some(stem) = trimmed.strip_suffix("::") {
        format!("{stem}::")
    } else if let Some(stem) = trimmed.strip_suffix(';') {
        format!("{stem}::")
    } else if let Some(stem) = trimmed.strip_suffix(':') {
        format!("{stem}::")
    } else {
        format!("{trimmed}::")
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// The summary both engines are reported with — deliberately the same shape Criterion prints,
/// computed the same way on both sides so the table compares like with like.
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub n: usize,
    pub mean: Duration,
    pub median: Duration,
    pub min: Duration,
    pub max: Duration,
}

impl Stats {
    pub fn of(samples: &[Duration]) -> Option<Stats> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let total: Duration = sorted.iter().sum();
        let median = if sorted.len() % 2 == 1 {
            sorted[sorted.len() / 2]
        } else {
            (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2
        };
        Some(Stats {
            n: sorted.len(),
            mean: total / sorted.len() as u32,
            median,
            min: sorted[0],
            max: sorted[sorted.len() - 1],
        })
    }
}

/// Human-readable duration, in the unit Criterion would have chosen.
pub fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s >= 1.0 {
        format!("{s:.3} s")
    } else if s >= 1e-3 {
        format!("{:.3} ms", s * 1e3)
    } else {
        format!("{:.3} µs", s * 1e6)
    }
}

// ---------------------------------------------------------------------------
// The answer both engines are checked against each other on
// ---------------------------------------------------------------------------

/// What one dispatch produced, in the one form both engines can be compared in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// `AutomatonWriter.writeTxtFormatToStream`'s output.
    Automaton(String),
    True,
    False,
    Error(String),
    /// The command produced no `TestCase` at all.
    None,
}

impl Answer {
    pub fn kind(&self) -> &'static str {
        match self {
            Answer::Automaton(_) => "automaton",
            Answer::True => "true",
            Answer::False => "false",
            Answer::Error(_) => "error",
            Answer::None => "none",
        }
    }
}

/// `Ok(())` iff the two engines computed the same thing.
///
/// Automata compare by `wr_core::equiv` **semantic language equivalence**, never structurally
/// (`CLAUDE.md`'s prime directive), with the same two normalizations `tests/differential-gen`
/// and `tests/golden` both use: `sort_label()` on the port's automaton (Java's writer canonizes
/// and therefore sorts the track label, and the equivalence oracle's track comparison is
/// positional) and `totalize(0)` on both (Walnut `.txt` automata need not be total).
pub fn same_answer(java: &Answer, rust: &Answer) -> Result<(), String> {
    match (java, rust) {
        (Answer::True, Answer::True)
        | (Answer::False, Answer::False)
        | (Answer::None, Answer::None) => Ok(()),
        (Answer::Error(a), Answer::Error(b)) if a == b => Ok(()),
        (Answer::Automaton(java_txt), Answer::Automaton(rust_txt)) => {
            let mut theirs = wr_io::reader::read_automaton_from_str(java_txt)
                .map_err(|e| format!("the JVM's own serialized automaton did not parse: {e:?}"))?;
            let mut ours = wr_io::reader::read_automaton_from_str(rust_txt)
                .map_err(|e| format!("the port's serialized automaton did not parse: {e:?}"))?;
            ours.sort_label();
            ours.fa.totalize(0);
            theirs.fa.totalize(0);
            match automaton_language_equivalent(&ours, &theirs) {
                Ok(true) => Ok(()),
                Ok(false) => Err("the two engines' automata accept DIFFERENT languages".into()),
                Err(e) => Err(format!(
                    "the equivalence oracle refused the comparison: {e:?}"
                )),
            }
        }
        (j, r) => Err(format!(
            "result kind differs: java={} rust={}",
            j.kind(),
            r.kind()
        )),
    }
}

// ---------------------------------------------------------------------------
// The Rust engine: `wr-cli` in-process
// ---------------------------------------------------------------------------

/// A prepared Rust-side engine: a private copy of the corpus's library trees with Walnut's own
/// `IntegrationTest.initialize` prelude already replayed into it, and **one session-lifetime
/// `Prover`** that every dispatch goes through.
///
/// # Why one `Prover`, when `IntegrationTest` uses a fresh one per fixture
///
/// Because that is what makes the two engines comparable, not what makes the Rust side look
/// good. Java's expensive per-session state is `static`, so a `new Prover()` there keeps it:
/// `NumberSystem.numberSystemHash` (`Automata/NumberSystem.java:85`) is a JVM-global
/// `HashMap<String, NumberSystem>`, so the adder/comparator/equality automata for `msd_17` are
/// built once per JVM and every later query in that JVM gets them free. The port puts the same
/// cache on the `Session` (`wr_cli::session::FileLibraries::number_systems`), which the
/// `Prover` owns — so a fresh `Prover` per iteration would rebuild, every single iteration,
/// exactly the automata Java built once.
///
/// That is not a theoretical worry; it is **measured**. On fixture 207 (`?msd_17 a=37`, whose
/// cost is almost entirely `msd_17`'s `k³ = 4,913`-symbol adder/comparator construction) the
/// same query measures **0.31 ms** with one session-lifetime `Prover` and **119.5 ms** with a
/// fresh one per iteration — a 390× swing, against a Java side that measures 0.41 ms either
/// way because its cache is a JVM-global static. Reproduce with
/// `WR_BENCH_COLD=1 WR_BENCH_ONLY=207 cargo run -p wr-bench --release --bin compare`.
///
/// One long-lived `Prover` is also what the real `wr-cli` REPL does, and what Java's own REPL
/// does (`Prover.mainProver`, a static initialized once). Nothing accumulates across commands:
/// `Prover::parse_setup` rebuilds `MetaCommands` per command, exactly as Java's does, and
/// `current_eval_name` deliberately survives on both sides (`prover.rs:1042-1053`).
pub struct RustEngine {
    home_dir: String,
    session_dir: String,
    prover: RefCell<Prover>,
}

impl RustEngine {
    /// Builds the tree under `dest` and replays the prelude.
    pub fn prepare(corpus_root: &Path, dest: &Path) -> Result<RustEngine, String> {
        let (home_dir, session_dir) = golden::build_session_tree(corpus_root, dest)
            .map_err(|e| format!("building the session tree at {}: {e}", dest.display()))?;
        let engine = RustEngine {
            prover: RefCell::new(new_prover(&session_dir, &home_dir)),
            home_dir,
            session_dir,
        };
        for (i, command) in golden::PRELUDE.iter().enumerate() {
            engine
                .dispatch(command)
                .map_err(|e| format!("prelude[{i}] `{command}`: {e}"))?;
        }
        Ok(engine)
    }

    pub fn session_dir(&self) -> &str {
        &self.session_dir
    }

    pub fn home_dir(&self) -> &str {
        &self.home_dir
    }

    /// One dispatch, timing NOT taken. `Err` only for a malformed `TestCase`; a command that
    /// Walnut itself rejects comes back as [`Answer::Error`] on both sides, so the two are
    /// compared rather than one of them aborting the run.
    pub fn dispatch(&self, command: &str) -> Result<Answer, String> {
        let mut prover = self.prover.borrow_mut();
        match prover.dispatch_for_integration_test(command, "") {
            Err(e) => Ok(Answer::Error(e.to_string())),
            Ok(None) => Ok(Answer::None),
            Ok(Some(tc)) => {
                let pairs = tc.automaton_pairs();
                if pairs.len() != 1 {
                    return Err(format!("unexpected automaton-pair count {}", pairs.len()));
                }
                match pairs[0].automaton() {
                    None => Err("null automaton in pair".to_string()),
                    Some(a) if a.fa.is_true_false_automaton() => Ok(if a.fa.is_true_automaton() {
                        Answer::True
                    } else {
                        Answer::False
                    }),
                    Some(a) => Ok(Answer::Automaton(render_txt(a))),
                }
            }
        }
    }

    /// One dispatch, returning the `details` trace (for [`peak_states`]). Untimed.
    pub fn details(&self, command: &str) -> Result<String, String> {
        let mut prover = self.prover.borrow_mut();
        match prover.dispatch_for_integration_test(command, "") {
            Err(e) => Err(e.to_string()),
            Ok(None) => Ok(String::new()),
            Ok(Some(tc)) => Ok(tc.details().to_string()),
        }
    }

    /// The measured region, in exactly the shape the JVM driver measures on its side:
    /// `warmup` throwaway iterations, then `measure` timed ones, all in this process.
    pub fn bench(&self, command: &str, warmup: usize, measure: usize) -> Vec<Duration> {
        for _ in 0..warmup {
            self.timed_dispatch(command);
        }
        (0..measure).map(|_| self.timed_dispatch(command)).collect()
    }

    /// One dispatch, timed. Public so the Criterion benchmark measures byte-identical work to
    /// the head-to-head's Rust column.
    pub fn timed_dispatch(&self, command: &str) -> Duration {
        let mut prover = self.prover.borrow_mut();
        let t0 = Instant::now();
        let _ = prover.dispatch_for_integration_test(command, "");
        t0.elapsed()
    }

    /// As [`RustEngine::bench`], but building a **fresh `Prover`** (and therefore a fresh,
    /// empty `Session` number-system cache) inside every timed iteration.
    ///
    /// This is NOT the head-to-head's measurement — it would charge the port for a cache Java
    /// keeps in a JVM-global static, see this type's docs. It exists so the report can *quantify*
    /// that difference instead of asserting it, and because it is what `tests/golden` does per
    /// fixture, which is why that harness's per-fixture times are much larger than this
    /// benchmark's for the same query.
    pub fn bench_cold(&self, command: &str, warmup: usize, measure: usize) -> Vec<Duration> {
        let run = || {
            let mut prover = new_prover(&self.session_dir, &self.home_dir);
            let t0 = Instant::now();
            let _ = prover.dispatch_for_integration_test(command, "");
            t0.elapsed()
        };
        for _ in 0..warmup {
            run();
        }
        (0..measure).map(|_| run()).collect()
    }
}

/// This engine's one `Prover`, over its on-disk session.
///
/// All three sinks are `io::sink()`: the JVM driver mutes `System.out` for its whole process,
/// so neither engine pays for console I/O inside the timed region. (`SessionPaths`' console is
/// the one that prints `Overriding global file with session file:…` — real output, but output
/// the muted JVM does not produce either.)
fn new_prover(session_dir: &str, home_dir: &str) -> Prover {
    let paths = SessionPaths::with_console(
        Some(session_dir),
        Some(home_dir),
        false,
        Box::new(io::sink()),
    );
    let session = Session::from_paths(paths);
    let logging = Logging::with_writers(Box::new(io::sink()), Box::new(io::sink()));
    Prover::with_output(session, logging, Box::new(io::sink()))
}

/// `AutomatonWriter.writeTxtFormatToStream`'s Rust counterpart, to a `String`.
fn render_txt(a: &wr_core::automaton::Automaton) -> String {
    // `write_txt` canonizes in place, exactly as Java's writer does, hence the clone.
    let mut a = a.clone();
    let mut buf: Vec<u8> = Vec::new();
    match wr_io::writer::write_txt(&mut a, &mut buf) {
        Ok(()) => String::from_utf8_lossy(&buf).into_owned(),
        Err(e) => format!("<could not serialize the Rust automaton: {e}>"),
    }
}

// ---------------------------------------------------------------------------
// The Java engine: one long-lived, warm JVM
// ---------------------------------------------------------------------------

/// The sibling `walnut-java` checkout. Same resolution rule as `tests/golden` and
/// `tests/differential-gen`: `WALNUT_JAVA_DIR`, else the first ancestor with a `walnut-java`
/// beside it.
pub fn walnut_java_dir() -> PathBuf {
    golden::walnut_java_dir()
}

/// The fat jar the driver compiles and runs against. `Err` (never a silent skip) when it is
/// missing — `CLAUDE.md`'s CI/absent-oracle contract.
pub fn oracle_jar() -> Result<PathBuf, String> {
    let jar = walnut_java_dir().join("target/Walnut-all.jar");
    if jar.is_file() {
        Ok(jar)
    } else {
        Err(format!(
            "the walnut-java oracle jar is missing: {}\n\
             Build it with:  cd {} && ./mvnw -q clean package -DskipTests -Pfat-jar\n\
             (or point WALNUT_JAVA_DIR at a checkout that has one).",
            jar.display(),
            walnut_java_dir().display()
        ))
    }
}

/// Absolute path to a JDK tool, resolved the same way `tests/differential-gen` resolves it
/// (`Walnut-all.jar` is class-file major 61, so an older `javac` refuses it outright):
/// `WR_BENCH_JAVA_HOME`, `JAVA_HOME`, `/usr/libexec/java_home -v 17`,
/// `/opt/homebrew/opt/openjdk@17/bin`, then bare `PATH`.
pub fn jdk_tool(name: &str) -> PathBuf {
    for var in ["WR_BENCH_JAVA_HOME", "WR_DIFFGEN_JAVA_HOME", "JAVA_HOME"] {
        if let Some(home) = std::env::var_os(var) {
            let candidate = PathBuf::from(home).join("bin").join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    if Path::new("/usr/libexec/java_home").is_file() {
        if let Ok(out) = Command::new("/usr/libexec/java_home")
            .args(["-v", "17"])
            .output()
        {
            if out.status.success() {
                let home = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let candidate = PathBuf::from(home).join("bin").join(name);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    let brew = PathBuf::from("/opt/homebrew/opt/openjdk@17/bin").join(name);
    if brew.is_file() {
        return brew;
    }
    PathBuf::from(name)
}

/// One measured reply from the JVM: the per-iteration timings plus the last iteration's
/// answer and `details` trace.
#[derive(Debug, Clone)]
pub struct JavaReply {
    pub samples: Vec<Duration>,
    pub answer: Answer,
    pub details: String,
}

/// One long-lived `walnut-java` JVM running `java/BenchDriver.java`.
///
/// Deliberately a separate driver from `tests/differential-gen`'s `DiffGenDriver`, which this
/// crate does not modify: that one answers one query per round trip and wraps every query in
/// `eval "<formula>";`, neither of which works here (a fixture's command script carries its own
/// metacommand prefix and `::` suffix, and the whole point of this harness is repeating one
/// command inside one warm JVM and timing it *there*, not over the pipe).
pub struct JavaEngine {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Vec<u8>>,
}

impl JavaEngine {
    /// Compiles `BenchDriver.java` against the oracle jar and starts the JVM pointed at
    /// `session_dir`/`home_dir` — which must be the JVM's OWN copy of the tree, not the Rust
    /// engine's (both write library files as they run).
    ///
    /// `heap_mb` bounds the child so a state explosion becomes a prompt `OutOfMemoryError`
    /// rather than swapping the machine.
    pub fn start(
        scratch: &Path,
        session_dir: &str,
        home_dir: &str,
        heap_mb: u32,
    ) -> Result<JavaEngine, String> {
        let jar = oracle_jar()?;
        let classes = scratch.join("classes");
        std::fs::create_dir_all(&classes).map_err(|e| format!("creating {classes:?}: {e}"))?;

        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("java/BenchDriver.java");
        let javac = jdk_tool("javac");
        let out = Command::new(&javac)
            .arg("-cp")
            .arg(&jar)
            .arg("-d")
            .arg(&classes)
            .arg(&src)
            .output()
            .map_err(|e| format!("running {} (is a JDK 17+ installed?): {e}", javac.display()))?;
        if !out.status.success() {
            return Err(format!(
                "compiling {} with {} failed:\n{}{}\n\
                 (Walnut-all.jar is built for Java 17; set WR_BENCH_JAVA_HOME or JAVA_HOME to a \
                 JDK 17+ if the above says `class file has wrong version 61.0`.)",
                src.display(),
                javac.display(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }

        let classpath = format!("{}:{}", jar.display(), classes.display());
        // The child's stderr is kept, not discarded: an `OutOfMemoryError`'s trace and a
        // classpath refusal are only visible there.
        let stderr = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(scratch.join("jvm-stderr.log"))
        {
            Ok(f) => Stdio::from(f),
            Err(_) => Stdio::null(),
        };
        let mut child = Command::new(jdk_tool("java"))
            .arg(format!("-Xmx{heap_mb}m"))
            .arg("-cp")
            .arg(&classpath)
            .arg("BenchDriver")
            .arg(session_dir)
            .arg(home_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .map_err(|e| format!("spawning the JVM (is `java` on PATH?): {e}"))?;

        let stdin = child.stdin.take().ok_or("the JVM child has no stdin")?;
        let stdout = child.stdout.take().ok_or("the JVM child has no stdout")?;
        let (tx, rx) = channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) | Err(_) => return, // EOF or a dead pipe: the child is gone.
                    Ok(_) => {
                        if byte[0] == 0 {
                            if tx.send(std::mem::take(&mut buf)).is_err() {
                                return;
                            }
                        } else {
                            buf.push(byte[0]);
                        }
                    }
                }
            }
        });
        Ok(JavaEngine { child, stdin, rx })
    }

    /// `warmup` throwaway iterations then `measure` timed ones, all inside the warm JVM.
    ///
    /// `deadline` bounds the WHOLE request (every iteration together): the JVM is a child
    /// process with no other supervision here, and `CLAUDE.md`'s guardrail is that a harness
    /// never hangs. A timeout is fatal to the run rather than a skip — with one shared
    /// long-lived JVM there is no way to know the child is not still mutating its session tree,
    /// which is exactly the taint `tests/golden` halts on.
    pub fn bench(
        &mut self,
        command: &str,
        warmup: usize,
        measure: usize,
        deadline: Duration,
    ) -> Result<JavaReply, String> {
        assert!(
            !command.as_bytes().contains(&0),
            "a command must never contain NUL (it is the record separator)"
        );
        self.send(command, warmup, measure)
            .map_err(|e| format!("writing the request to the JVM failed: {e}"))?;

        let status = self.recv(deadline)?;
        let nanos = self.recv(deadline)?;
        let kind = self.recv(deadline)?;
        let payload = self.recv(deadline)?;
        let details = self.recv(deadline)?;

        if status != "ok" {
            return Err(format!(
                "the JVM reported a fatal error (its heap/stack is no longer trustworthy): \
                 {payload}"
            ));
        }
        let samples = parse_nanos(&nanos, measure)?;
        let answer = match kind.as_str() {
            "automaton" => Answer::Automaton(payload),
            "true" => Answer::True,
            "false" => Answer::False,
            "error" => Answer::Error(payload),
            "none" => Answer::None,
            other => return Err(format!("unknown response kind from the JVM: {other:?}")),
        };
        Ok(JavaReply {
            samples,
            answer,
            details,
        })
    }

    fn send(&mut self, command: &str, warmup: usize, measure: usize) -> io::Result<()> {
        for record in [command, &warmup.to_string(), &measure.to_string()] {
            self.stdin.write_all(record.as_bytes())?;
            self.stdin.write_all(&[0])?;
        }
        self.stdin.flush()
    }

    fn recv(&mut self, deadline: Duration) -> Result<String, String> {
        match self.rx.recv_timeout(deadline) {
            Ok(bytes) => String::from_utf8(bytes)
                .map_err(|e| format!("non-UTF-8 response record from the JVM: {e}")),
            Err(RecvTimeoutError::Timeout) => Err(format!(
                "the JVM did not answer within {:.0}s",
                deadline.as_secs_f64()
            )),
            Err(RecvTimeoutError::Disconnected) => {
                Err("the JVM exited (its stdout closed)".to_string())
            }
        }
    }
}

impl Drop for JavaEngine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Decodes the driver's `nanos_csv` record, insisting on exactly `expected` samples — a short
/// list would silently average fewer iterations than the Rust side ran.
pub fn parse_nanos(csv: &str, expected: usize) -> Result<Vec<Duration>, String> {
    let mut out = Vec::with_capacity(expected);
    if !csv.is_empty() {
        for tok in csv.split(',') {
            let n: u64 = tok
                .trim()
                .parse()
                .map_err(|e| format!("the JVM sent a malformed timing {tok:?}: {e}"))?;
            out.push(Duration::from_nanos(n));
        }
    }
    if out.len() != expected {
        return Err(format!(
            "the JVM timed {} iterations, the harness asked for {expected}",
            out.len()
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_states_takes_the_largest_state_count_in_the_trace() {
        let details = "\
 computing &:2 states - 2 states
  computed cross product:4 states - 0ms
  Minimizing: 4 states.
   Determinizing [#5, strategy: BRZ]: 1790 states
   Determinized: 496 states - 12ms
 quantified:3 states - 0ms";
        assert_eq!(peak_states(details), Some(1790));
    }

    /// `Progress: Added N states` is a running counter inside ONE traversal, not the size of
    /// an automaton — Java's own `assertEqualMessages` strips those lines, and counting them
    /// would report a peak that no automaton ever reached.
    #[test]
    fn progress_lines_do_not_contribute_to_the_peak() {
        let details = "\
  computing cross product:4 states
      Progress: Added 999999 states - 1ms
  computed cross product:166 states";
        assert_eq!(peak_states(details), Some(166));
    }

    #[test]
    fn a_digit_run_not_followed_by_states_is_not_a_state_count() {
        assert_eq!(peak_states("Total computation time: 4321ms."), None);
        assert_eq!(peak_states("[strategy 6 BRZ]"), None);
        assert_eq!(peak_states("Minimized:32 states - 0ms."), Some(32));
        // Singular, as `Logging` prints for one state.
        assert_eq!(peak_states("Determinized: 1 state - 0ms"), Some(1));
    }

    #[test]
    fn peak_states_is_none_for_a_trace_with_no_counts() {
        assert_eq!(peak_states(""), None);
        assert_eq!(peak_states("computing x+y\ncomputed x+y"), None);
    }

    #[test]
    fn detail_variant_switches_any_terminator_to_the_double_colon() {
        assert_eq!(detail_variant("eval t \"x=x\";"), "eval t \"x=x\"::");
        assert_eq!(detail_variant("eval t \"x=x\":"), "eval t \"x=x\"::");
        assert_eq!(detail_variant("eval t \"x=x\"::"), "eval t \"x=x\"::");
        assert_eq!(
            detail_variant("[strategy 6 BRZ]eval test637 \"x=x\"::"),
            "[strategy 6 BRZ]eval test637 \"x=x\"::"
        );
        // Trailing whitespace is trimmed, never left between the formula and the suffix.
        assert_eq!(detail_variant("eval t \"x=x\"; \n"), "eval t \"x=x\"::");
    }

    #[test]
    fn stats_are_computed_over_the_whole_sample() {
        let s = Stats::of(&[
            Duration::from_millis(10),
            Duration::from_millis(30),
            Duration::from_millis(20),
        ])
        .expect("non-empty");
        assert_eq!(s.n, 3);
        assert_eq!(s.mean, Duration::from_millis(20));
        assert_eq!(s.median, Duration::from_millis(20));
        assert_eq!(s.min, Duration::from_millis(10));
        assert_eq!(s.max, Duration::from_millis(30));
        assert!(Stats::of(&[]).is_none());
    }

    #[test]
    fn an_even_sample_takes_the_midpoint_of_the_two_middle_values() {
        let s = Stats::of(&[
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_millis(30),
            Duration::from_millis(50),
        ])
        .expect("non-empty");
        assert_eq!(s.median, Duration::from_millis(25));
    }

    /// A short (or long) timing list must be loud, never quietly averaged: it would mean the
    /// JVM measured a different number of iterations than the Rust side did.
    #[test]
    fn parse_nanos_insists_on_the_requested_sample_count() {
        assert_eq!(
            parse_nanos("1000,2000,3000", 3).expect("well formed"),
            vec![
                Duration::from_nanos(1000),
                Duration::from_nanos(2000),
                Duration::from_nanos(3000)
            ]
        );
        assert!(parse_nanos("1000,2000", 3).is_err());
        assert!(parse_nanos("1000,2000,3000,4000", 3).is_err());
        assert!(parse_nanos("", 0).expect("empty is fine for 0").is_empty());
        assert!(parse_nanos("1000,oops", 2).is_err());
    }

    #[test]
    fn same_answer_matches_only_like_for_like() {
        assert!(same_answer(&Answer::True, &Answer::True).is_ok());
        assert!(same_answer(&Answer::True, &Answer::False).is_err());
        assert!(same_answer(&Answer::Error("a".into()), &Answer::Error("a".into())).is_ok());
        assert!(same_answer(&Answer::Error("a".into()), &Answer::Error("b".into())).is_err());
        assert!(same_answer(&Answer::None, &Answer::True).is_err());
    }

    /// The workload table must name real, subset-relevant fixtures — and each exactly once.
    /// A typo here would silently benchmark a different query than the report claims.
    #[test]
    fn the_workload_ids_are_distinct() {
        let mut ids: Vec<usize> = WORKLOADS.iter().map(|(id, _, _)| *id).collect();
        ids.push(STRATEGY_WORKLOAD.0);
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "a fixture id is listed twice in WORKLOADS");
    }

    /// Skipped (not failed) without the sibling oracle checkout, exactly like every other
    /// corpus-dependent check in this repo's fast tier — the LOUD failure belongs to the
    /// benchmark run itself, not to a unit test that cannot see the corpus.
    #[test]
    fn every_workload_resolves_against_the_real_manifest() {
        if golden::corpus_root().is_none() {
            eprintln!("skipping: no walnut-java corpus beside this checkout");
            return;
        }
        let loaded = workloads().expect("the workload table must resolve");
        assert_eq!(loaded.len(), WORKLOADS.len() + 1);
        for w in &loaded {
            assert!(
                w.command.contains("eval") || w.command.contains("def"),
                "fixture {} is not an eval/def workload: {}",
                w.id,
                w.command
            );
        }
        // Fixture 637 must still be the strategy-sensitive one; if the corpus is regenerated
        // and that stops being true, the benchmark's one slow-workload claim is stale.
        let strategy = loaded.last().expect("non-empty");
        assert_eq!(strategy.id, STRATEGY_WORKLOAD.0);
        assert!(
            strategy.command.starts_with("[strategy 6 BRZ]"),
            "fixture {} no longer carries `[strategy 6 BRZ]`: {}",
            strategy.id,
            strategy.command
        );
        assert!(
            strategy.command.trim_end().ends_with("::"),
            "fixture {} must stay `::`-suffixed — `[strategy …]` is gated on detail printing",
            strategy.id
        );
    }
}
