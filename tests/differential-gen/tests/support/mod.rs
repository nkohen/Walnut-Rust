// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Shared machinery for the **Tier-3 differential generator** (`docs/DESIGN.md` §5, Tier 3;
//! Phase 4's U29). See `../soak.rs` for the harness itself.
//!
//! This module holds four separable pieces, each unit-testable without a JVM:
//!
//! 1. [`SplitMix64`] — a hand-rolled, fully specified PRNG, so a recorded seed replays the
//!    exact same query stream forever (see `Cargo.toml` for why this is not `rand`).
//! 2. [`Generator`] — a recursive query-string builder over the KEEP-subset grammar, held to
//!    `CLAUDE.md`'s "generate SMALL" guardrail (small bases, shallow alternation).
//! 3. [`JavaOracle`] — one long-lived `walnut-java` JVM child, driven over a NUL-delimited
//!    pipe protocol (documented in `../../CAPTURE.md`), with kill-and-restart on a hang.
//! 4. [`compare`] — the verdict function: automata by `wr_core::equiv` semantic equivalence,
//!    errors by EXACT string equality (never normalized — see `../../CAPTURE.md`).

use std::fmt::Write as _;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Locating the oracle
// ---------------------------------------------------------------------------

/// The sibling `walnut-java` checkout that owns the oracle (`CLAUDE.md`, "The three-repo
/// world"). Overridable with `WALNUT_JAVA_DIR`. Same resolution rule as
/// `tests/golden/tests/support/mod.rs`'s `walnut_java_dir` — walk *up* from this crate rather
/// than hard-coding `..` hops, so an agent working in a git worktree still finds it.
pub fn walnut_java_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WALNUT_JAVA_DIR") {
        return PathBuf::from(dir);
    }
    let start = Path::new(env!("CARGO_MANIFEST_DIR"));
    for ancestor in start.ancestors() {
        let candidate = ancestor.join("walnut-java");
        if candidate.join("src/main/java/Main/Prover.java").is_file() {
            return candidate;
        }
    }
    start.join("../../..").join("walnut-java")
}

/// The fat jar the driver compiles and runs against.
///
/// `Err` (never a silent skip) when it is missing — `CLAUDE.md`'s CI/absent-oracle contract,
/// matching `tests/golden`'s: a checkout without the oracle must fail LOUDLY saying why.
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

/// The `DiffGenDriver.java` source, compiled fresh on every run (never committed into
/// `walnut-java`'s tracked source — see `../../CAPTURE.md`).
pub fn driver_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("java/DiffGenDriver.java")
}

/// Absolute path to a JDK tool (`javac` / `java`), or the bare name to be resolved on `PATH`.
///
/// `Walnut-all.jar` is built for Java 17 (class-file major version 61), so a `PATH` `javac`
/// from an older JDK rejects it outright (`class file has wrong version 61.0`) — which is
/// exactly what a `jenv`/`asdf` shim pointing at Java 11 produces on this machine. Resolution
/// order, most explicit first:
///
/// 1. `WR_DIFFGEN_JAVA_HOME` — this harness's own override;
/// 2. `JAVA_HOME` — the conventional one;
/// 3. `/usr/libexec/java_home -v 17` — macOS's canonical JDK locator;
/// 4. `/opt/homebrew/opt/openjdk@17/bin` — the checkout `tests/differential/CAPTURE.md`'s
///    existing recipes already name;
/// 5. bare `javac`/`java` on `PATH`.
pub fn jdk_tool(name: &str) -> PathBuf {
    for var in ["WR_DIFFGEN_JAVA_HOME", "JAVA_HOME"] {
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

// ---------------------------------------------------------------------------
// 1. Deterministic PRNG
// ---------------------------------------------------------------------------

/// SplitMix64 (Steele/Lea/Flood, "Fast Splittable Pseudorandom Number Generators", 2014) —
/// the standard seeding mixer, used here as the generator itself.
///
/// Every constant is written out, so this crate's query stream is a pure function of the seed
/// on any machine and any future toolchain. That is the whole point: a divergence found at
/// index 47,213 of seed `0xDEADBEEF` must be replayable verbatim months later.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`. Uses the high bits (SplitMix64's low bits are fine too, but the
    /// widening-multiply form avoids modulo bias without a rejection loop).
    pub fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "below(0) has no value to return");
        ((self.next_u64() as u128 * n as u128) >> 64) as usize
    }

    /// Uniform element of a non-empty slice.
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

// ---------------------------------------------------------------------------
// 2. The query generator
// ---------------------------------------------------------------------------

/// The numeration systems generated. `CLAUDE.md`'s "generate SMALL": base 2-4 only, because
/// the alphabet of a `NumberSystem`'s addition automaton is `k^3` and the decision procedure
/// is superexponential in alternation — bugs show up on 5-state automata, not 500-state ones.
/// `lsd_*` is included deliberately: Phase 3b's L1 note flags it as the least-exercised
/// numeration surface in the port.
pub const NUMBER_SYSTEMS: [&str; 5] = ["msd_2", "msd_3", "msd_4", "lsd_2", "lsd_3"];

/// Free variables. Kept disjoint from [`BOUND_VARS`] so no generated formula ever shadows a
/// name — shadowing is a real (and interesting) surface, but it would confound a
/// *first* soak run, where the goal is measuring throughput and validating the plumbing.
pub const FREE_VARS: [&str; 2] = ["x", "y"];
/// Quantifier-introduced variables.
pub const BOUND_VARS: [&str; 3] = ["i", "j", "k"];

/// Relational operators (`Main/Predicate.java`'s relational token set, KEEP subset).
pub const RELATIONS: [&str; 6] = ["=", "!=", "<", "<=", ">", ">="];

/// Maximum formula nesting depth. 3 keeps every generated query inside the "few states, small
/// base, shallow alternation" budget.
pub const MAX_DEPTH: u32 = 3;
/// Maximum number of nested quantifiers on any root-to-leaf path.
pub const MAX_QUANTIFIER_DEPTH: u32 = 2;

/// Builds one Walnut query string per call, from a [`SplitMix64`] stream.
pub struct Generator<'a> {
    rng: &'a mut SplitMix64,
}

impl<'a> Generator<'a> {
    pub fn new(rng: &'a mut SplitMix64) -> Self {
        Generator { rng }
    }

    /// One complete `?<ns> <formula>` query body (no surrounding `eval "…";` — both sides
    /// wrap it themselves, so the two wrappings can never drift apart).
    pub fn query(&mut self) -> String {
        let ns = *self.rng.pick(&NUMBER_SYSTEMS);
        // Start with one or two free variables in scope.
        let n_free = 1 + self.rng.below(FREE_VARS.len());
        let scope: Vec<&str> = FREE_VARS[..n_free].to_vec();
        let body = self.formula(MAX_DEPTH, MAX_QUANTIFIER_DEPTH, &scope);
        format!("?{ns} {body}")
    }

    fn formula(&mut self, depth: u32, quant_budget: u32, scope: &[&str]) -> String {
        if depth == 0 {
            return self.atom(scope);
        }
        // Weights, out of 16: atoms stay common so the tree does not always bottom out at
        // maximum depth (which would make every query the same shape).
        let roll = self.rng.below(16);
        match roll {
            0..=4 => self.atom(scope),
            5..=7 => {
                let op = *self.rng.pick(&["&", "|"]);
                let a = self.formula(depth - 1, quant_budget, scope);
                let b = self.formula(depth - 1, quant_budget, scope);
                format!("({a} {op} {b})")
            }
            8..=9 => {
                let op = *self.rng.pick(&["=>", "<=>"]);
                let a = self.formula(depth - 1, quant_budget, scope);
                let b = self.formula(depth - 1, quant_budget, scope);
                format!("({a} {op} {b})")
            }
            10..=11 => {
                let a = self.formula(depth - 1, quant_budget, scope);
                format!("~({a})")
            }
            _ => {
                if quant_budget == 0 {
                    return self.atom(scope);
                }
                // Bind the next unused name, so nesting never shadows.
                let used = scope.iter().filter(|v| BOUND_VARS.contains(v)).count();
                if used >= BOUND_VARS.len() {
                    return self.atom(scope);
                }
                let v = BOUND_VARS[used];
                let mut inner: Vec<&str> = scope.to_vec();
                inner.push(v);
                let body = self.formula(depth - 1, quant_budget - 1, &inner);
                if !mentions(&body, v) {
                    // Walnut rejects a quantifier over a variable the body never uses; emitting
                    // one would only manufacture matched-error pairs, so drop the quantifier
                    // instead of the (already valid) body.
                    return body;
                }
                let q = *self.rng.pick(&["E", "A"]);
                format!("{q}{v} ({body})")
            }
        }
    }

    fn atom(&mut self, scope: &[&str]) -> String {
        let lhs = self.term(scope);
        let rel = *self.rng.pick(&RELATIONS);
        let rhs = self.term(scope);
        format!("{lhs} {rel} {rhs}")
    }

    fn term(&mut self, scope: &[&str]) -> String {
        // Constants stay in 0..=5 and coefficients in 1..=4: `NumberSystem.getConstant` /
        // `getMultiplication` build an automaton per constant, so large ones buy state count,
        // not coverage.
        match self.rng.below(8) {
            0 => format!("{}", self.rng.below(6)),
            1..=3 => (*self.rng.pick(scope)).to_string(),
            4 => format!("{} + {}", self.rng.pick(scope), self.rng.below(6)),
            5 => format!("{} + {}", self.rng.pick(scope), self.rng.pick(scope)),
            6 => format!("{} - {}", self.rng.pick(scope), self.rng.below(6)),
            _ => format!("{}*{}", 1 + self.rng.below(4), self.rng.pick(scope)),
        }
    }
}

/// Whether `formula` mentions `var` as a whole identifier (not as a substring of a longer
/// name). All generated names are single ASCII letters, so "whole identifier" reduces to
/// "not adjacent to another alphanumeric".
pub fn mentions(formula: &str, var: &str) -> bool {
    let bytes = formula.as_bytes();
    let v = var.as_bytes();
    if v.is_empty() || v.len() > bytes.len() {
        return false;
    }
    for start in 0..=(bytes.len() - v.len()) {
        if &bytes[start..start + v.len()] != v {
            continue;
        }
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after = start + v.len();
        let after_ok = after == bytes.len() || !is_ident_byte(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------------------------------------------------------------------------
// 3. The JVM oracle
// ---------------------------------------------------------------------------

/// What either side computed for one query.
///
/// `Automaton` carries the Java side's serialized `.txt`; the Rust side keeps a real
/// `wr_core::Automaton` instead and never builds this variant, so there is no risk of the
/// comparison silently degrading into a structural (byte) one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Automaton(String),
    True,
    False,
    Error(String),
    /// A JVM `Error` (OOM / StackOverflow). Not a semantic answer: the harness records
    /// `skip-too-big` and restarts the child, because the JVM's state is no longer trusted.
    Fatal(String),
}

impl Answer {
    pub fn kind(&self) -> &'static str {
        match self {
            Answer::Automaton(_) => "automaton",
            Answer::True => "true",
            Answer::False => "false",
            Answer::Error(_) => "error",
            Answer::Fatal(_) => "fatal",
        }
    }
}

/// Decodes one `(kind, payload)` response pair into an [`Answer`].
pub fn decode_answer(kind: &str, payload: &str) -> Result<Answer, String> {
    match kind {
        "automaton" => Ok(Answer::Automaton(payload.to_string())),
        "true" => Ok(Answer::True),
        "false" => Ok(Answer::False),
        "error" => Ok(Answer::Error(payload.to_string())),
        "fatal" => Ok(Answer::Fatal(payload.to_string())),
        other => Err(format!(
            "unknown response kind {other:?} (payload: {payload:?})"
        )),
    }
}

/// What went wrong talking to the JVM.
#[derive(Debug)]
pub enum OracleError {
    /// The child produced no response within the deadline. The child has already been killed
    /// and a fresh one spawned by the time this is returned.
    Timeout,
    /// The child died, the protocol desynchronized, or the response was malformed. Also
    /// already restarted.
    Protocol(String),
}

/// One long-lived `walnut-java` JVM, driven over the pipe protocol in `../../CAPTURE.md`.
pub struct JavaOracle {
    jar: PathBuf,
    classes: PathBuf,
    scratch: PathBuf,
    heap: String,
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Vec<u8>>,
    /// How many times the child had to be killed and respawned.
    pub restarts: usize,
}

impl JavaOracle {
    /// Compiles `DiffGenDriver.java` against the oracle jar and starts the first child.
    ///
    /// `scratch` is where the JVM-side `Session` tree lands; `heap_mb` bounds the child so a
    /// state-exploding query becomes a prompt `OutOfMemoryError` (reported as
    /// [`Answer::Fatal`]) instead of swapping the machine.
    pub fn start(scratch: &Path, heap_mb: u32) -> Result<Self, String> {
        let jar = oracle_jar()?;
        let classes = scratch.join("classes");
        std::fs::create_dir_all(&classes).map_err(|e| format!("creating {classes:?}: {e}"))?;

        let src = driver_source();
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
                 (Walnut-all.jar is built for Java 17; set WR_DIFFGEN_JAVA_HOME or JAVA_HOME to \
                 a JDK 17+ if the above says `class file has wrong version 61.0`.)",
                src.display(),
                javac.display(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }

        let heap = format!("-Xmx{heap_mb}m");
        let (child, stdin, rx) = spawn_child(&jar, &classes, scratch, &heap)?;
        Ok(JavaOracle {
            jar,
            classes,
            scratch: scratch.to_path_buf(),
            heap,
            child,
            stdin,
            rx,
            restarts: 0,
        })
    }

    /// Kills the current child and starts a fresh one, with a fresh reader thread.
    ///
    /// The old reader thread is dropped, not joined: killing the child closes its stdout, so
    /// the thread observes EOF and exits on its own. Its channel is dropped with it, which is
    /// what makes a restart safe — a stale response from the dead child can never be read as
    /// an answer to a later query.
    pub fn respawn(&mut self) -> Result<(), String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let (child, stdin, rx) = spawn_child(&self.jar, &self.classes, &self.scratch, &self.heap)?;
        self.child = child;
        self.stdin = stdin;
        self.rx = rx;
        self.restarts += 1;
        Ok(())
    }

    /// Asks the oracle one query and waits at most `deadline` for the whole response.
    ///
    /// On any timeout or protocol fault the child is killed and respawned before returning, so
    /// the caller can simply continue with the next query.
    pub fn ask(&mut self, id: u64, query: &str, deadline: Duration) -> Result<Answer, OracleError> {
        if let Err(e) = self.send(id, query) {
            let restart = self.respawn();
            return Err(OracleError::Protocol(format!(
                "writing the request failed: {e}{}",
                restart
                    .err()
                    .map(|r| format!(" (restart also failed: {r})"))
                    .unwrap_or_default()
            )));
        }

        let echoed = match self.recv(deadline) {
            Ok(r) => r,
            Err(e) => return Err(self.fail(e)),
        };
        let kind = match self.recv(deadline) {
            Ok(r) => r,
            Err(e) => return Err(self.fail(e)),
        };
        let payload = match self.recv(deadline) {
            Ok(r) => r,
            Err(e) => return Err(self.fail(e)),
        };

        // THE load-bearing invariant (plan, review finding B2): a desynchronized pipe must be
        // loud and immediate, never a flood of phantom divergences over the rest of the run.
        if echoed != id.to_string() {
            let e = OracleError::Protocol(format!(
                "pipe desynchronized: asked query_id {id}, the oracle echoed {echoed:?}"
            ));
            let _ = self.respawn();
            return Err(e);
        }

        match decode_answer(&kind, &payload) {
            Ok(a) => Ok(a),
            Err(why) => {
                let _ = self.respawn();
                Err(OracleError::Protocol(why))
            }
        }
    }

    fn fail(&mut self, e: OracleError) -> OracleError {
        let _ = self.respawn();
        e
    }

    fn send(&mut self, id: u64, query: &str) -> io::Result<()> {
        assert!(
            !query.as_bytes().contains(&0),
            "a generated query must never contain NUL (it is the record separator)"
        );
        self.stdin.write_all(id.to_string().as_bytes())?;
        self.stdin.write_all(&[0])?;
        self.stdin.write_all(query.as_bytes())?;
        self.stdin.write_all(&[0])?;
        self.stdin.flush()
    }

    fn recv(&mut self, deadline: Duration) -> Result<String, OracleError> {
        match self.rx.recv_timeout(deadline) {
            Ok(bytes) => String::from_utf8(bytes)
                .map_err(|e| OracleError::Protocol(format!("non-UTF-8 response record: {e}"))),
            Err(RecvTimeoutError::Timeout) => Err(OracleError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(OracleError::Protocol(
                "the JVM oracle exited (its stdout closed)".to_string(),
            )),
        }
    }
}

impl Drop for JavaOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts one JVM child plus the reader thread that turns its stdout into a stream of
/// NUL-delimited records.
fn spawn_child(
    jar: &Path,
    classes: &Path,
    scratch: &Path,
    heap: &str,
) -> Result<(Child, ChildStdin, Receiver<Vec<u8>>), String> {
    let classpath = format!("{}:{}", jar.display(), classes.display());
    let mut child = Command::new(jdk_tool("java"))
        .arg(heap)
        .arg("-cp")
        .arg(&classpath)
        .arg("DiffGenDriver")
        .arg(scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawning the JVM oracle (is `java` on PATH?): {e}"))?;

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
                            return; // the harness moved on (restart); stop reading.
                        }
                    } else {
                        buf.push(byte[0]);
                    }
                }
            }
        }
    });
    Ok((child, stdin, rx))
}

// ---------------------------------------------------------------------------
// 4. Verdicts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Match,
    Divergence(String),
    /// Over budget on either side, or a JVM `Error`. Recorded visibly, never silently dropped
    /// (`CLAUDE.md`, test-performance guardrails).
    SkipTooBig(String),
}

impl Verdict {
    pub fn tag(&self) -> &'static str {
        match self {
            Verdict::Match => "match",
            Verdict::Divergence(_) => "divergence",
            Verdict::SkipTooBig(_) => "skip-too-big",
        }
    }
}

/// The `true`/`false`/`error` half of the comparison — everything that does not need a real
/// `Automaton`, so it can be unit-tested without a JVM or a session tree.
///
/// Returns `None` when the pair is `(Automaton, automaton)`, i.e. when the caller must run the
/// semantic-equivalence oracle instead.
pub fn compare_non_automaton(java: &Answer, rust: &Answer) -> Option<Verdict> {
    match (java, rust) {
        (Answer::Fatal(why), _) => Some(Verdict::SkipTooBig(format!("JVM error: {why}"))),
        (Answer::True, Answer::True) | (Answer::False, Answer::False) => Some(Verdict::Match),
        // EXACT equality, deliberately: the port reproduces Java's exception text
        // byte-for-byte (WB-013/033/034/035), and `tests/golden`'s own `compare_error` is an
        // exact `assertEquals` too. Normalizing here would blind this harness to exactly the
        // class of bug it exists to find.
        (Answer::Error(a), Answer::Error(b)) => {
            if a == b {
                Some(Verdict::Match)
            } else {
                Some(Verdict::Divergence(format!(
                    "error message differs (exact comparison, as in Java)\n  java: {a}\n  rust: {b}"
                )))
            }
        }
        (Answer::Automaton(_), Answer::Automaton(_)) => None,
        (j, r) => Some(Verdict::Divergence(format!(
            "result KIND differs: java={} rust={}\n  java payload: {}\n  rust payload: {}",
            j.kind(),
            r.kind(),
            summarize(j),
            summarize(r)
        ))),
    }
}

fn summarize(a: &Answer) -> String {
    match a {
        Answer::Automaton(t) => format!("<automaton, {} bytes>", t.len()),
        Answer::True => "TRUE".to_string(),
        Answer::False => "FALSE".to_string(),
        Answer::Error(m) | Answer::Fatal(m) => m.clone(),
    }
}

// ---------------------------------------------------------------------------
// The run log
// ---------------------------------------------------------------------------

/// One append-only log line: `(seed, index, query_id, query, verdict)`.
///
/// Written BEFORE the verdict is known would be ideal for crash-resumability; this milestone
/// writes one line per completed query instead (see `../soak.rs`'s note), which is enough to
/// replay a divergence and to restart a run from the last logged index.
pub fn log_line(
    seed: u64,
    index: u64,
    query_id: u64,
    query: &str,
    oracle_kind: &str,
    verdict: &Verdict,
) -> String {
    let mut s = String::new();
    let detail = match verdict {
        Verdict::Match => String::new(),
        Verdict::Divergence(d) | Verdict::SkipTooBig(d) => {
            format!("\t{}", d.replace('\n', "\\n").replace('\t', " "))
        }
    };
    let _ = writeln!(
        s,
        "{seed:#018x}\t{index}\t{query_id}\t{}\t{oracle_kind}\t{}{detail}",
        query.replace('\t', " "),
        verdict.tag()
    );
    s
}

// ===========================================================================
// Unit tests — everything here runs in the FAST tier (no JVM, no oracle).
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- SplitMix64 -----------------------------------------------------------

    /// Pinned against the reference SplitMix64 (`golang.org/x/exp/rand`, Vigna's C, and
    /// java.util.SplittableRandom all agree on these): seed 0's first three outputs.
    #[test]
    fn splitmix64_matches_the_reference_stream() {
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(r.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn the_same_seed_replays_the_same_query_stream() {
        let render = |seed| {
            let mut r = SplitMix64::new(seed);
            let mut g = Generator::new(&mut r);
            (0..200).map(|_| g.query()).collect::<Vec<_>>()
        };
        assert_eq!(render(0x5EED), render(0x5EED));
        assert_ne!(render(0x5EED), render(0x5EEE));
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut r = SplitMix64::new(7);
        let mut seen = [false; 5];
        for _ in 0..2000 {
            let v = r.below(5);
            assert!(v < 5);
            seen[v] = true;
        }
        assert!(
            seen.iter().all(|s| *s),
            "below(5) never produced some value"
        );
    }

    // -- the generator --------------------------------------------------------

    #[test]
    fn generated_queries_are_shaped_like_walnut_queries() {
        let mut r = SplitMix64::new(0xC0FFEE);
        let mut g = Generator::new(&mut r);
        for _ in 0..5000 {
            let q = g.query();
            assert!(q.starts_with('?'), "missing numeration prefix: {q}");
            let ns = q[1..].split_whitespace().next().unwrap();
            assert!(
                NUMBER_SYSTEMS.contains(&ns),
                "unexpected number system in {q}"
            );
            assert!(!q.as_bytes().contains(&0), "NUL in a generated query: {q}");
            assert_eq!(
                q.matches('(').count(),
                q.matches(')').count(),
                "unbalanced parentheses: {q}"
            );
            assert!(!q.contains("::"), "generated a detail-printing query: {q}");
            assert!(
                !q.contains('"'),
                "a quote would break the eval wrapper: {q}"
            );
        }
    }

    /// The generator must never bind a quantifier over a variable its body does not use —
    /// Walnut rejects those, and a stream of matched errors would drown real signal.
    #[test]
    fn every_quantifier_binds_a_variable_its_body_actually_uses() {
        let mut r = SplitMix64::new(0xBEEF);
        let mut g = Generator::new(&mut r);
        for _ in 0..5000 {
            let q = g.query();
            for v in BOUND_VARS {
                // `E<v> (` / `A<v> (` is the only shape the generator emits.
                for quant in ["E", "A"] {
                    let needle = format!("{quant}{v} (");
                    if let Some(at) = q.find(&needle) {
                        let body = balanced_suffix(&q[at + needle.len() - 1..]);
                        assert!(
                            mentions(body, v),
                            "quantifier {quant}{v} binds an unused variable in {q}"
                        );
                    }
                }
            }
        }
    }

    /// The parenthesized group starting at `s[0] == '('`, without its outer parentheses.
    fn balanced_suffix(s: &str) -> &str {
        let b = s.as_bytes();
        assert_eq!(b[0], b'(');
        let mut depth = 0usize;
        for (i, c) in b.iter().enumerate() {
            match c {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return &s[1..i];
                    }
                }
                _ => {}
            }
        }
        &s[1..]
    }

    #[test]
    fn quantifier_nesting_stays_within_budget() {
        let mut r = SplitMix64::new(11);
        let mut g = Generator::new(&mut r);
        for _ in 0..5000 {
            let q = g.query();
            let quantifiers = BOUND_VARS
                .iter()
                .filter(|v| q.contains(&format!("E{v} (")) || q.contains(&format!("A{v} (")))
                .count();
            assert!(
                quantifiers as u32 <= MAX_QUANTIFIER_DEPTH,
                "{quantifiers} quantifiers exceeds the budget in {q}"
            );
        }
    }

    // -- `mentions` -----------------------------------------------------------

    #[test]
    fn mentions_matches_whole_identifiers_only() {
        assert!(mentions("i < 3", "i"));
        assert!(mentions("(x = 2 & i > 1)", "i"));
        assert!(mentions("2*i", "i"));
        assert!(!mentions("x < 3", "i"));
        // Not a substring match: `ij` is a different identifier (the generator only emits
        // single letters, but the guard must not depend on that).
        assert!(!mentions("ij < 3", "i"));
        assert!(!mentions("x_i < 3", "i"));
        assert!(!mentions("", "i"));
    }

    // -- response decoding ----------------------------------------------------

    #[test]
    fn decode_answer_covers_every_protocol_kind() {
        assert_eq!(decode_answer("true", "").unwrap(), Answer::True);
        assert_eq!(decode_answer("false", "").unwrap(), Answer::False);
        assert_eq!(
            decode_answer("automaton", "msd_2\n").unwrap(),
            Answer::Automaton("msd_2\n".to_string())
        );
        assert_eq!(
            decode_answer("error", "boom").unwrap(),
            Answer::Error("boom".to_string())
        );
        assert_eq!(
            decode_answer("fatal", "java.lang.OutOfMemoryError").unwrap(),
            Answer::Fatal("java.lang.OutOfMemoryError".to_string())
        );
        assert!(decode_answer("automation", "typo").is_err());
    }

    // -- the comparator -------------------------------------------------------

    #[test]
    fn matching_truth_values_and_identical_errors_are_matches() {
        assert_eq!(
            compare_non_automaton(&Answer::True, &Answer::True),
            Some(Verdict::Match)
        );
        assert_eq!(
            compare_non_automaton(&Answer::False, &Answer::False),
            Some(Verdict::Match)
        );
        assert_eq!(
            compare_non_automaton(&Answer::Error("a".into()), &Answer::Error("a".into())),
            Some(Verdict::Match)
        );
    }

    /// Error comparison must be EXACT — no trimming, no case folding, no whitespace
    /// collapsing. This test is the guard on the plan's B3 correction.
    #[test]
    fn error_comparison_is_exact_not_normalized() {
        let v = compare_non_automaton(
            &Answer::Error("expected ')' at position 10".into()),
            &Answer::Error("expected ')'  at position 10".into()),
        );
        assert!(matches!(v, Some(Verdict::Divergence(_))), "got {v:?}");
        let v = compare_non_automaton(&Answer::Error("Boom".into()), &Answer::Error("boom".into()));
        assert!(matches!(v, Some(Verdict::Divergence(_))), "got {v:?}");
    }

    #[test]
    fn a_true_false_flip_is_a_divergence_not_a_match() {
        let v = compare_non_automaton(&Answer::True, &Answer::False);
        assert!(matches!(v, Some(Verdict::Divergence(_))), "got {v:?}");
    }

    #[test]
    fn one_side_erroring_is_a_divergence() {
        let v = compare_non_automaton(&Answer::True, &Answer::Error("nope".into()));
        assert!(matches!(v, Some(Verdict::Divergence(_))), "got {v:?}");
        let v = compare_non_automaton(&Answer::Error("nope".into()), &Answer::True);
        assert!(matches!(v, Some(Verdict::Divergence(_))), "got {v:?}");
        // An automaton on one side and a truth value on the other is also a divergence, not a
        // silent fall-through to the equivalence oracle.
        let v = compare_non_automaton(&Answer::Automaton("msd_2\n".into()), &Answer::True);
        assert!(matches!(v, Some(Verdict::Divergence(_))), "got {v:?}");
    }

    #[test]
    fn two_automata_defer_to_the_equivalence_oracle() {
        assert_eq!(
            compare_non_automaton(
                &Answer::Automaton("a".into()),
                &Answer::Automaton("b".into())
            ),
            None
        );
    }

    /// A JVM `Error` is never a semantic verdict, whatever the Rust side said.
    #[test]
    fn a_jvm_fatal_is_always_skip_too_big() {
        for rust in [
            Answer::True,
            Answer::False,
            Answer::Error("x".into()),
            Answer::Automaton("y".into()),
        ] {
            let v =
                compare_non_automaton(&Answer::Fatal("java.lang.OutOfMemoryError".into()), &rust);
            assert!(matches!(v, Some(Verdict::SkipTooBig(_))), "got {v:?}");
        }
    }

    // -- the log --------------------------------------------------------------

    #[test]
    fn log_lines_are_single_line_and_tab_separated() {
        let line = log_line(
            0xABC,
            7,
            7,
            "?msd_2 x\t> 1",
            "automaton",
            &Verdict::Divergence("two\nlines".into()),
        );
        assert_eq!(line.matches('\n').count(), 1, "not one line: {line:?}");
        assert!(line.ends_with('\n'));
        let fields: Vec<&str> = line.trim_end().split('\t').collect();
        assert_eq!(fields.len(), 7, "unexpected field count in {line:?}");
        assert_eq!(fields[0], "0x0000000000000abc");
        assert_eq!(fields[1], "7");
        assert_eq!(
            fields[3], "?msd_2 x > 1",
            "an embedded tab must be neutralized"
        );
        assert_eq!(fields[4], "automaton");
        assert_eq!(fields[5], "divergence");
        assert_eq!(fields[6], "two\\nlines");
    }
}
