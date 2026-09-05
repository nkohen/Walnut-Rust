// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! Section 5 item E end to end: a constant-term DFAO computed on the
//! `RustConstantTermSequences` substrate enters the engine as an object
//! (`wr_cts::bridge` → `Engine::register_word_automaton`), is queried with real
//! first-order formulas, and the verdicts agree with the substrate's own evaluation of
//! the sequence. Then the engine's automaton goes back to the substrate.

use std::fs;
use std::path::PathBuf;

use rust_constant_term_sequences::dfao::DFAO;
use rust_constant_term_sequences::laurent_poly::LaurentPoly;
use rust_constant_term_sequences::mod_int::ModInt;
use wr_cli::embed::Engine;
use wr_cts::bridge::{automaton_from_poly_dfao, dfao_from_automaton, Direction};

fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wr-substrate-{tag}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    for sub in [
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
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// Central binomial coefficients mod 2: `CT((x + 1/x)^n)` = `binom(n, n/2)` for even
/// `n`, `0` for odd `n`. Mod 2 that is `1` exactly when `n = 0`... no: `binom(2m, m)` is
/// even for every `m >= 1`, so the sequence is `1, 0, 0, 0, …` — a degenerate check.
/// Mod 3 is the interesting one: `binom(2m, m) mod 3` is nonzero iff the base-3 digits
/// of `m` are all `0` or `1` (Lucas), which is exactly what a base-3 DFAO decides.
fn central_binomial_mod_3() -> DFAO<ModInt, LaurentPoly> {
    let p = LaurentPoly::from_vec(vec![(1, 1), (-1, 1)], 3);
    let q = LaurentPoly::one(3);
    DFAO::poly_auto(&p, &q, 10_000).expect("a small automaton")
}

#[test]
fn a_substrate_dfao_answers_first_order_queries_in_the_engine() {
    let dfao = central_binomial_mod_3();
    // The substrate reads digits least-significant-first (`compute_ct` = `compute_lsd`).
    let a = automaton_from_poly_dfao(&dfao, Direction::Lsd).unwrap();
    assert_eq!(a.fa.q, dfao.states.len());

    let ws = workspace("ct3");
    let mut engine = Engine::new(&ws).unwrap();
    engine.register_word_automaton("CB", a.clone());

    // Every value the engine reads off the registered automaton matches the substrate.
    for n in 0..200u64 {
        let expected = dfao.compute_ct(n).value;
        let verdict = engine
            .eval_bool(&format!(r#"eval v "?lsd_3 CB[{n}] = {expected}""#))
            .unwrap();
        assert_eq!(verdict, Some(true), "n = {n}, substrate says {expected}");
    }
    // Odd n: the constant term of an odd power of (x + 1/x) is 0.
    assert_eq!(
        engine
            .eval_bool(r#"eval odd "?lsd_3 An CB[2*n+1] = 0""#)
            .unwrap(),
        Some(true)
    );
    // Not identically zero.
    assert_eq!(
        engine
            .eval_bool(r#"eval nz "?lsd_3 En CB[n] = 2""#)
            .unwrap(),
        Some(true)
    );
    // A genuinely first-order fact (Lucas): CB[2m] != 0 iff CB[6m] != 0 (appending a
    // base-3 zero digit to m keeps its digits in {0,1}).
    assert_eq!(
        engine
            .eval_bool(r#"eval lucas "?lsd_3 Am ((CB[2*m] = 0) <=> (CB[6*m] = 0))""#)
            .unwrap(),
        Some(true)
    );
    assert!(!ws.join("Word Automata Library").join("CB.txt").exists());

    // Back to the substrate: the engine's automaton, as a substrate DFAO, still
    // computes the sequence.
    let (back, direction) = dfao_from_automaton(&a).unwrap();
    assert_eq!(direction, Direction::Lsd);
    for n in 0..200u64 {
        let via_engine = back.compute_lsd(n, 3, |&s| a.fa.o[s]);
        assert_eq!(via_engine as u64, dfao.compute_ct(n).value, "n = {n}");
    }
    fs::remove_dir_all(&ws).ok();
}
