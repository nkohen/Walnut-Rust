// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! The consumer-facing instrumentation surface (`docs/CT-RESEARCH-INTEGRATION.md`,
//! "Observability and resource budgets"): [`wr_cli::embed::Engine`]'s trajectory
//! recorder, resource budget and detailed-log access, and the binary's
//! `WR_MAX_STATES`/`WR_MAX_BYTES` environment knobs.
//!
//! Each assertion is exact where the contract is exact (the `EXPLODED-…` verdict token,
//! the structured error variant) so a regression in the seam fails here rather than in
//! a consumer's driver.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use std::rc::Rc;

use wr_cli::embed::resource::{
    memory_meter, Event, ExhaustedReason, Instrumentation, ResourceBudget,
};
use wr_cli::embed::Engine;
use wr_cli::prover::ProverError;
use wr_core::determinize::Strategy;
use wr_core::fa::Fa;
use wr_core::minimize::{MinimizeError, Minimizer};

const BIN: &str = env!("CARGO_BIN_EXE_walnut-rs");

const LIB_DIRS: &[&str] = &[
    "Automata Library",
    "Word Automata Library",
    "Custom Bases",
    "Command Files",
    "Result",
    "Session",
    "Macro Library",
    "Morphism Library",
    "Transducer Library",
    "Test Results",
];

fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wr-embed-instr-{tag}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    for sub in LIB_DIRS {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// A query with a real quantifier alternation, so `eval` runs a projection, a subset
/// construction and a minimization: "every x has a strictly larger y" — TRUE.
const QUANTIFIED_TRUE: &str = r#"eval q "?msd_2 Ax Ey (y > x)""#;

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Buf {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for Buf {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ------------------------------------------------------------------ trajectory

#[test]
fn trajectory_records_each_commands_determinizations() {
    let ws = workspace("traj");
    let mut engine = Engine::new(&ws).unwrap();
    assert!(engine.trajectory().is_none(), "off by default");
    engine.record_trajectory(true).unwrap();

    assert_eq!(engine.eval_bool(QUANTIFIED_TRUE).unwrap(), Some(true));
    let t = engine.trajectory().expect("recording is on");
    assert!(t.peak_states() > 0);
    let recs = t.determinizations();
    assert!(
        !recs.is_empty(),
        "a quantified eval must run at least one subset construction; events: {:?}",
        t.events()
    );
    assert!(
        recs.iter().all(|r| r.minimized.is_some()),
        "every eval-path determinization is followed by its minimization: {recs:?}"
    );
    assert!(recs.iter().all(|r| r.minimized.unwrap() <= r.peak_states));

    // The next command starts a fresh trajectory (a quantifier-free eval runs no
    // subset construction at all).
    assert_eq!(
        engine.eval_bool(r#"eval p "?msd_2 1 < 2""#).unwrap(),
        Some(true)
    );
    let t2 = engine.trajectory().unwrap();
    assert!(t2.determinizations().is_empty(), "{:?}", t2.events());
    assert_ne!(t2.events().len(), t.events().len());

    engine.record_trajectory(false).unwrap();
    assert!(engine.trajectory().is_none());
    fs::remove_dir_all(&ws).ok();
}

// ---------------------------------------------------------------------- budget

#[test]
fn a_state_budget_yields_a_structured_error_and_the_engine_survives() {
    let ws = workspace("budget");
    let mut engine = Engine::new(&ws).unwrap();
    engine.set_budget(ResourceBudget::states(1)).unwrap();

    let err = engine.eval_bool(QUANTIFIED_TRUE).unwrap_err();
    match &err {
        ProverError::ResourceExhausted(e) => {
            assert_eq!(e.reason, ExhaustedReason::States);
            assert_eq!(e.limit, 1);
            assert!(e.at > 1);
            assert!(err.to_string().starts_with("EXPLODED-states: "), "{err}");
        }
        other => panic!("expected ResourceExhausted, got {other:?}"),
    }

    // Lifting the cap on the same engine: the identical query now decides. Nothing
    // from the aborted command leaked into the session.
    engine.set_budget(ResourceBudget::UNLIMITED).unwrap();
    assert_eq!(engine.eval_bool(QUANTIFIED_TRUE).unwrap(), Some(true));
    fs::remove_dir_all(&ws).ok();
}

#[test]
fn the_builder_accepts_a_budget_up_front() {
    let ws = workspace("builder-budget");
    let mut engine = Engine::builder(&ws)
        .budget(ResourceBudget::states(1))
        .build()
        .unwrap();
    assert!(matches!(
        engine.eval_bool(QUANTIFIED_TRUE),
        Err(ProverError::ResourceExhausted(_))
    ));
    fs::remove_dir_all(&ws).ok();
}

#[test]
fn a_memory_cap_without_a_tracking_allocator_is_refused() {
    // This test binary links no tracking allocator (only the `walnut-rs` binary does),
    // so a memory cap cannot be enforced here and must be refused, not ignored.
    assert!(!memory_meter::is_enabled());
    let ws = workspace("no-meter");
    let mut engine = Engine::new(&ws).unwrap();
    let cap = ResourceBudget {
        max_states: None,
        max_bytes: Some(1 << 30),
    };
    assert!(engine.set_budget(cap).is_err());
    // Nothing was installed: the engine still runs unbudgeted.
    assert_eq!(engine.eval_bool(QUANTIFIED_TRUE).unwrap(), Some(true));
    assert_eq!(
        Engine::builder(&ws)
            .budget(cap)
            .build()
            .err()
            .map(|e| e.kind()),
        Some(io::ErrorKind::Unsupported)
    );
    assert!(
        !memory_meter::is_enabled(),
        "a failed enable must not leave counting on"
    );
    fs::remove_dir_all(&ws).ok();
}

// ------------------------------------------------------------------ detailed log

#[test]
fn detailed_log_is_the_double_colon_text_and_the_builder_can_capture_the_console() {
    let ws = workspace("detailed");
    let console = Buf::default();
    let mut engine = Engine::builder(&ws)
        .console(Box::new(console.clone()))
        .build()
        .unwrap();

    // `;` mode: no detailed log.
    engine.run(r#"eval q "?msd_2 Ax Ey (y > x)";"#).unwrap();
    assert_eq!(engine.detailed_log(), "");

    // `::` mode: the same lines the binary prints, available in-process.
    let out = engine.run(r#"eval q "?msd_2 Ax Ey (y > x)"::"#).unwrap();
    assert!(out.lines().any(|l| l == "TRUE"), "{out}");
    let log = engine.detailed_log();
    assert!(
        log.contains("Minimizing:") && log.contains("states"),
        "detailed log should carry the construction trace, got:\n{log}"
    );
    // The console sink received it too (the `::` trace is console output in Walnut).
    let seen = console.text();
    assert!(seen.contains("Minimizing:"), "console got:\n{seen}");
    // And the per-step command log is non-empty in `::` mode as well.
    assert!(!engine.command_log().is_empty());
    fs::remove_dir_all(&ws).ok();
}

// ------------------------------------------------------------------ the binary

struct Run {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_binary_env(ws: &Path, env: &[(&str, &str)], commands: &str) -> Run {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(ws)
        .env_remove("WR_MAX_STATES")
        .env_remove("WR_MAX_BYTES")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn walnut-rs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(commands.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    Run {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

const SCRIPT: &str = "eval q \"?msd_2 Ax Ey (y > x)\";\neval p \"?msd_2 1 < 2\";\nquit;\n";

#[test]
fn binary_state_budget_prints_the_verdict_token_and_keeps_reading() {
    let ws = workspace("bin-states");
    let run = run_binary_env(&ws, &[("WR_MAX_STATES", "1")], SCRIPT);
    assert!(run.status.success(), "{:?}\n{}", run.status, run.stderr);
    let lines: Vec<&str> = run.stdout.lines().collect();
    // On stdout, like every Walnut error message; in interactive (stdin) mode it
    // follows the `[Walnut]$ ` prompt on the same line, exactly as Java's exception
    // messages do -- hence `grep -o 'EXPLODED-[a-z]*'` rather than an anchored match.
    assert!(
        lines.iter().any(|l| l.contains("EXPLODED-states: ")),
        "stdout:\n{}",
        run.stdout
    );
    // The consumer's guard extracts verdicts with a whole-line `grep -x`: the bare
    // token stands on its own line (after Walnut's `____` prompt-erase line), exactly
    // like `TRUE`/`FALSE`.
    assert_eq!(
        lines.iter().filter(|l| **l == "EXPLODED-states").count(),
        1,
        "stdout:\n{}",
        run.stdout
    );
    // The budgeted command printed no verdict; the next command still ran.
    assert_eq!(
        lines.iter().filter(|l| **l == "TRUE").count(),
        1,
        "{}",
        run.stdout
    );
    fs::remove_dir_all(&ws).ok();
}

#[test]
fn binary_memory_budget_is_enforced_by_the_installed_tracking_allocator() {
    let ws = workspace("bin-mem");
    // One byte: any check point sees more than that live, so the first construction
    // of the first command breaches.
    let run = run_binary_env(&ws, &[("WR_MAX_BYTES", "1")], SCRIPT);
    assert!(run.status.success(), "{:?}\n{}", run.status, run.stderr);
    assert!(
        run.stdout.lines().any(|l| l.contains("EXPLODED-mem: ")),
        "stdout:\n{}\nstderr:\n{}",
        run.stdout,
        run.stderr
    );
    assert!(
        run.stdout.lines().any(|l| l == "EXPLODED-mem"),
        "{}",
        run.stdout
    );
    // A generous cap changes nothing: both verdicts print.
    let run = run_binary_env(&ws, &[("WR_MAX_BYTES", "4G")], SCRIPT);
    assert_eq!(
        run.stdout.lines().filter(|l| *l == "TRUE").count(),
        2,
        "{}",
        run.stdout
    );
    fs::remove_dir_all(&ws).ok();
}

#[test]
fn binary_rejects_a_malformed_budget_at_startup() {
    let ws = workspace("bin-bad");
    let run = run_binary_env(&ws, &[("WR_MAX_STATES", "lots")], SCRIPT);
    assert!(!run.status.success());
    assert!(run.stderr.contains("WR_MAX_STATES"), "{}", run.stderr);
    assert!(!run.stdout.contains("TRUE"));
    fs::remove_dir_all(&ws).ok();
}

#[test]
fn binary_without_budget_variables_is_unchanged() {
    let ws = workspace("bin-none");
    let run = run_binary_env(&ws, &[], SCRIPT);
    assert!(run.status.success());
    assert_eq!(
        run.stdout.lines().filter(|l| *l == "TRUE").count(),
        2,
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("EXPLODED"));
    fs::remove_dir_all(&ws).ok();
}

// ------------------------------------------------------------- item C: SC_OTF + seam

#[test]
fn the_sc_otf_strategy_is_selectable_by_metacommand_and_by_scope_default() {
    let ws = workspace("otf");
    let mut engine = Engine::new(&ws).unwrap();
    engine.record_trajectory(true).unwrap();

    // Baseline: plain SC everywhere.
    assert_eq!(engine.eval_bool(QUANTIFIED_TRUE).unwrap(), Some(true));
    let strategies = |t: &wr_cli::embed::resource::Trajectory| {
        t.events()
            .iter()
            .filter_map(|e| match e {
                Event::Determinize { strategy, .. } => Some(*strategy),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let base = engine.trajectory().unwrap();
    assert!(!strategies(&base).is_empty());
    assert!(strategies(&base).iter().all(|s| *s == Strategy::Sc));

    // `[strategy * SC_OTF]` (metacommands are read in `::` mode, like Java): every
    // dispatcher-level determinization now runs SC_OTF, and the verdict is unchanged.
    let out = engine
        .run(r#"[strategy * SC_OTF] eval q "?msd_2 Ax Ey (y > x)"::"#)
        .unwrap();
    assert!(out.lines().any(|l| l == "TRUE"), "{out}");
    let t = engine.trajectory().unwrap();
    assert!(!strategies(&t).is_empty());
    assert!(
        strategies(&t).iter().all(|s| *s == Strategy::ScOtf),
        "{:?}",
        strategies(&t)
    );
    assert!(
        engine.detailed_log().contains("strategy: SC_OTF]"),
        "{}",
        engine.detailed_log()
    );
    // Java's alias normalization: underscores/dashes dropped, case-insensitive.
    engine
        .run(r#"[strategy * scotf] eval q "?msd_2 Ax Ey (y > x)"::"#)
        .unwrap();
    assert!(strategies(&engine.trajectory().unwrap())
        .iter()
        .all(|s| *s == Strategy::ScOtf));

    // Scope default: applies in `;` mode too (no metacommand parsed there), and an
    // explicit metacommand still wins over it.
    engine
        .set_instrumentation(Instrumentation::new().with_default_strategy(Strategy::ScOtf))
        .unwrap();
    assert_eq!(engine.eval_bool(QUANTIFIED_TRUE).unwrap(), Some(true));
    let t = engine.trajectory().unwrap();
    assert!(
        strategies(&t).iter().all(|s| *s == Strategy::ScOtf),
        "{:?}",
        strategies(&t)
    );
    engine
        .run(r#"[strategy * SC] eval q "?msd_2 Ax Ey (y > x)"::"#)
        .unwrap();
    let t = engine.trajectory().unwrap();
    assert!(
        strategies(&t).iter().all(|s| *s == Strategy::Sc),
        "{:?}",
        strategies(&t)
    );
    fs::remove_dir_all(&ws).ok();
}

/// `wr_cts::moore` — the Tier-4 independent minimizer — installed through the seam. It
/// returns the minimal COMPLETE DFA (keeps a dead class Valmari drops), which is a
/// legal, non-identical, language-equivalent minimizer: exactly what the seam must
/// tolerate without changing any verdict.
struct Moore(std::cell::Cell<usize>);

impl Minimizer for Moore {
    fn minimize(&self, fa: &Fa) -> Result<Fa, MinimizeError> {
        self.0.set(self.0.get() + 1);
        if fa.q == 0 {
            return Ok(fa.clone());
        }
        if !fa.is_deterministic() {
            return Err(MinimizeError::NotDeterministic);
        }
        // Moore wants a total DFA; the construction path hands it partial ones.
        let mut total = wr_core::trim::trim(fa);
        total.totalize(0);
        wr_cts::moore::minimize(&total)
            .map_err(|e| panic!("moore rejected a construction-path automaton: {e:?}"))
    }
    fn name(&self) -> &str {
        "moore"
    }
}

#[test]
fn a_second_minimizer_through_the_seam_decides_the_same_queries() {
    let ws = workspace("moore-seam");
    let mut engine = Engine::new(&ws).unwrap();
    let moore = Rc::new(Moore(std::cell::Cell::new(0)));
    engine
        .set_instrumentation(Instrumentation::new().with_minimizer(moore.clone()))
        .unwrap();
    let queries = [
        (QUANTIFIED_TRUE, Some(true)),
        (r#"eval f "?msd_2 Ax Ey (y < x)""#, Some(false)),
        (r#"eval g "?msd_3 Ax Ay (x + y = y + x)""#, Some(true)),
        (r#"eval h "?msd_2 Ex Ay (x <= y)""#, Some(true)),
    ];
    for (q, expected) in queries {
        assert_eq!(engine.eval_bool(q).unwrap(), expected, "{q}");
    }
    assert!(moore.0.get() > 0, "the seam must actually have been used");
    // A `def` result built through Moore still saves and is reusable.
    engine.run(r#"def lt "?msd_2 x < y""#).unwrap();
    assert_eq!(
        engine
            .eval_bool(r#"eval k "?msd_2 Ax Ey $lt(x, y)""#)
            .unwrap(),
        Some(true)
    );
    fs::remove_dir_all(&ws).ok();
}
