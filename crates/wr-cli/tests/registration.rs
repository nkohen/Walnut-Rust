// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! Section 5 items E (in-memory automaton registration — the engine half of the
//! substrate bridge), F (command registration) and G (witness extraction) through the
//! public `wr_cli::embed::Engine` surface.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use wr_cli::embed::witness;
use wr_cli::embed::Engine;
use wr_cli::prover::{CommandContext, ProverError, RegisterCommandError};
use wr_cli::test_case::TestCase;
use wr_core::automaton::Automaton;
use wr_core::fa::Fa;

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
    let dir = std::env::temp_dir().join(format!("wr-registration-{tag}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    for sub in LIB_DIRS {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// The (single) automaton a `def`/`eval` `TestCase` carries.
fn first(tc: &TestCase) -> Automaton {
    tc.automaton_pairs()[0]
        .automaton()
        .expect("an automaton-valued result")
        .clone()
}

fn row(pairs: &[(i32, usize)]) -> BTreeMap<i32, Vec<usize>> {
    pairs.iter().map(|&(s, d)| (s, vec![d])).collect()
}

/// Thue–Morse as a hand-built msd_2 word automaton in exactly the shape the reader
/// gives a `Word Automata Library/` file (one track, label `"0"`, `msd_2` named).
fn thue_morse() -> Automaton {
    let fa = Fa::with_states(
        0,
        2,
        2,
        vec![0, 1],
        vec![row(&[(0, 0), (1, 1)]), row(&[(0, 1), (1, 0)])],
    );
    let mut a = Automaton::new(
        fa,
        vec![vec![0, 1]],
        vec!["0".to_string()],
        vec![Some(true)],
    );
    a.set_track_ns_name(0, Some("msd_2".to_string()));
    a
}

// ------------------------------------------------------------------- item E

#[test]
fn a_registered_word_automaton_is_usable_in_formulas_without_a_file() {
    let ws = workspace("word");
    let mut engine = Engine::new(&ws).unwrap();
    // Nothing on disk: the name does not resolve.
    assert!(engine
        .eval_bool(r#"eval t "?msd_2 An TM[n] = TM[2*n]""#)
        .is_err());

    engine.register_word_automaton("TM", thue_morse()).unwrap();
    // t(2n) = t(n) for every n: TRUE. t(n) = t(n+1) for every n: FALSE.
    assert_eq!(
        engine
            .eval_bool(r#"eval t "?msd_2 An TM[n] = TM[2*n]""#)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        engine
            .eval_bool(r#"eval f "?msd_2 An TM[n] = TM[n+1]""#)
            .unwrap(),
        Some(false)
    );
    // Thue–Morse is overlap-free but has squares: `Ei En (n >= 1 & Aj (j < n => TM[i+j] = TM[i+j+n]))`.
    assert_eq!(
        engine
            .eval_bool(r#"eval sq "?msd_2 Ei En (n >= 1 & Aj (j < n => TM[i+j] = TM[i+j+n]))""#)
            .unwrap(),
        Some(true)
    );
    assert!(
        !ws.join("Word Automata Library").join("TM.txt").exists(),
        "registration must not write a file"
    );
    // Unregistering restores the file lookup (which fails, as at the start).
    engine.unregister_automaton("TM");
    assert!(engine
        .eval_bool(r#"eval t "?msd_2 An TM[n] = TM[2*n]""#)
        .is_err());
    fs::remove_dir_all(&ws).ok();
}

#[test]
fn a_registered_predicate_automaton_shadows_the_library_and_a_def_result_is_a_valid_source() {
    let ws = workspace("function");
    let mut engine = Engine::new(&ws).unwrap();
    // Build `x < y` through the engine itself, take the automaton object, and register
    // it under a name that has no file.
    let tc = engine
        .eval_structured(r#"def lt "?msd_2 x < y""#)
        .unwrap()
        .expect("a def yields a test case");
    let lt = first(&tc);
    engine.register_automaton("lt_mem", lt).unwrap();
    assert_eq!(
        engine
            .eval_bool(r#"eval a "?msd_2 Ax Ey $lt_mem(x, y)""#)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        engine
            .eval_bool(r#"eval b "?msd_2 Ey Ax $lt_mem(x, y)""#)
            .unwrap(),
        Some(false)
    );
    // The registration shadows the on-disk `lt.txt` the `def` wrote: register a
    // DIFFERENT automaton under `lt` and watch the verdict flip.
    let gt = first(
        &engine
            .eval_structured(r#"def gt "?msd_2 x > y""#)
            .unwrap()
            .unwrap(),
    );
    assert_eq!(
        engine
            .eval_bool(r#"eval c "?msd_2 Ax Ey $lt(x, y)""#)
            .unwrap(),
        Some(true)
    );
    engine.register_automaton("lt", gt).unwrap();
    assert_eq!(
        engine
            .eval_bool(r#"eval d "?msd_2 Ax Ey $lt(x, y)""#)
            .unwrap(),
        Some(false),
        "x > y has no y for x = 0"
    );
    engine.unregister_automaton("lt");
    assert_eq!(
        engine
            .eval_bool(r#"eval e "?msd_2 Ax Ey $lt(x, y)""#)
            .unwrap(),
        Some(true)
    );
    fs::remove_dir_all(&ws).ok();
}

#[test]
fn registration_refuses_what_the_reader_would_have_normalized() {
    use wr_cli::session::RegistrationError;
    let ws = workspace("validate");
    let mut engine = Engine::new(&ws).unwrap();
    // An NFA (two destinations on one symbol).
    let mut nfa = thue_morse();
    nfa.fa.d[0].insert(0, vec![0, 1]);
    assert_eq!(
        engine.register_word_automaton("bad", nfa),
        Err(RegistrationError::NotDeterministic)
    );
    // A duplicated alphabet entry.
    let dup = Automaton::new(
        thue_morse().fa,
        vec![vec![0, 1, 1]],
        vec!["0".to_string()],
        vec![Some(true)],
    );
    assert_eq!(
        engine.register_word_automaton("bad", dup),
        Err(RegistrationError::DuplicateAlphabetEntry { track: 0 })
    );
    // A custom base without its valid-representation automaton.
    let mut fib = thue_morse();
    fib.set_track_ns_name(0, Some("msd_fib".to_string()));
    assert_eq!(
        engine.register_word_automaton("bad", fib),
        Err(RegistrationError::CustomBaseWithoutAllReps {
            track: 0,
            ns_name: "msd_fib".to_string()
        })
    );
    // Plain bases, negative bases and the TRUE/FALSE automaton need nothing extra.
    let mut neg = thue_morse();
    neg.set_track_ns_name(0, Some("msd_neg_2".to_string()));
    assert_eq!(engine.register_word_automaton("ok", neg), Ok(()));
    assert_eq!(
        engine.register_automaton("t", Automaton::true_false(true)),
        Ok(())
    );
    fs::remove_dir_all(&ws).ok();
}

// ------------------------------------------------------------------- item F

#[test]
fn a_registered_command_dispatches_like_a_built_in() {
    let ws = workspace("command");
    let mut engine = Engine::new(&ws).unwrap();
    engine
        .prover()
        .register_command(
            "greet",
            Box::new(|ctx: CommandContext<'_>, s: &str| {
                let arg = s.trim_start_matches("greet").trim();
                writeln!(ctx.out, "HELLO {arg} details={}", ctx.print_details).unwrap();
                ctx.logging.log_message("greet ran");
                Ok(None)
            }),
        )
        .unwrap();
    assert_eq!(engine.prover().registered_commands(), vec!["greet"]);

    let out = engine.run("greet world;").unwrap();
    assert!(out.contains("HELLO world details=false"), "{out}");
    let out = engine.run("greet again::").unwrap();
    assert!(out.contains("HELLO again details=true"), "{out}");
    assert!(engine.detailed_log().contains("greet ran"));

    // A structured result comes back through the structured path.
    engine
        .prover()
        .register_command(
            "mk",
            Box::new(|_ctx: CommandContext<'_>, _s: &str| {
                Ok(Some(TestCase::from_automaton(thue_morse())))
            }),
        )
        .unwrap();
    let tc = engine.eval_structured("mk;").unwrap().expect("a test case");
    assert_eq!(first(&tc).fa.q, 2);

    // A handler may access the session — here, resolving a registered word automaton.
    engine.register_word_automaton("TM", thue_morse()).unwrap();
    engine
        .prover()
        .register_command(
            "count_states",
            Box::new(|ctx: CommandContext<'_>, s: &str| {
                let name = s.trim_start_matches("count_states").trim();
                let a = ctx
                    .session
                    .predicate_env()
                    .word(name)
                    .map_err(|e| ProverError::WalnutMessage(e.to_string()))?;
                writeln!(ctx.out, "{name} has {} states", a.fa.q).unwrap();
                Ok(None)
            }),
        )
        .unwrap();
    assert!(engine
        .run("count_states TM;")
        .unwrap()
        .contains("TM has 2 states"));

    // A panicking handler is recovered like a built-in guard, and the engine survives.
    engine
        .prover()
        .register_command(
            "boom",
            Box::new(|_ctx: CommandContext<'_>, _s: &str| panic!("custom guard fired")),
        )
        .unwrap();
    match engine.run("boom;") {
        Err(ProverError::Thrown { message, .. }) => assert_eq!(message, "custom guard fired"),
        other => panic!("expected a recovered panic, got {other:?}"),
    }
    assert!(engine.run("greet still;").unwrap().contains("HELLO still"));

    // Unknown names are still `NoSuchCommand`; built-ins cannot be replaced; bad names
    // are refused.
    assert!(matches!(
        engine.run("nosuch;"),
        Err(ProverError::NoSuchCommand)
    ));
    assert_eq!(
        engine
            .prover()
            .register_command("eval", Box::new(|_, _| Ok(None)))
            .unwrap_err(),
        RegisterCommandError::BuiltIn("eval".to_string())
    );
    assert_eq!(
        engine
            .prover()
            .register_command("1st", Box::new(|_, _| Ok(None)))
            .unwrap_err(),
        RegisterCommandError::InvalidName("1st".to_string())
    );
    assert!(engine
        .prover()
        .register_command("has-dash", Box::new(|_, _| Ok(None)))
        .is_err());
    // And the built-ins are untouched.
    assert_eq!(
        engine.eval_bool(r#"eval q "?msd_2 1 < 2""#).unwrap(),
        Some(true)
    );
    fs::remove_dir_all(&ws).ok();
}

// ------------------------------------------------------------------- item G

#[test]
fn witnesses_come_straight_off_a_decided_automaton() {
    let ws = workspace("witness");
    let mut engine = Engine::new(&ws).unwrap();
    // `x + y = 5 & x > y` over msd_2: the shortest satisfying assignment.
    let tc = engine
        .eval_structured(r#"def w "?msd_2 x + y = 5 & x > y""#)
        .unwrap()
        .unwrap();
    let a = &first(&tc);
    let w = witness::shortest_accepted_automaton(a)
        .unwrap()
        .expect("satisfiable");
    let x = w.track_value(a, 0, 2).unwrap();
    let y = w.track_value(a, 1, 2).unwrap();
    assert_eq!(x + y, 5);
    assert!(x > y, "x = {x}, y = {y}");
    // The shortest counterexample to "x + y = 5 => x > y" is the smallest pair with
    // x <= y that the automaton rejects.
    let tc = engine
        .eval_structured(r#"def c "?msd_2 (x + y = 5) => (x > y)""#)
        .unwrap()
        .unwrap();
    let a = &first(&tc);
    let r = witness::shortest_rejected_automaton(a)
        .unwrap()
        .expect("not valid");
    let x = r.track_value(a, 0, 2).unwrap();
    let y = r.track_value(a, 1, 2).unwrap();
    assert_eq!(x + y, 5);
    assert!(x <= y, "x = {x}, y = {y}");
    // A word automaton: the first index where Thue–Morse is 1 is 1.
    engine.register_word_automaton("TM", thue_morse()).unwrap();
    let tc = engine
        .eval_structured(r#"def one "?msd_2 TM[n] = 1""#)
        .unwrap()
        .unwrap();
    let a = &first(&tc);
    let w = witness::shortest_accepted_automaton(a).unwrap().unwrap();
    assert_eq!(w.track_value(a, 0, 2), Some(1));
    fs::remove_dir_all(&ws).ok();
}
