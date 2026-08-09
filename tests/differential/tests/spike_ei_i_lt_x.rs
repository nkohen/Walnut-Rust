// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! **The Phase 1 spike's exit criterion** (`docs/DESIGN.md` §8: "one real query
//! returns a semantically-equivalent result to Walnut (via the oracle), both ways").
//!
//! Builds `∃i (i < x)` over msd base-2 using only the ported pieces
//! (`less_than_msd` and `exists`), reads the real `walnut-java` output for the
//! identical query (`eval spike "?msd_2 Ei i<x";`, captured in `../CAPTURE.md`),
//! and asserts language equivalence via `wr_core`'s own oracle — never
//! textual/structural comparison, per this project's Prime Directive.

use std::collections::BTreeSet;
use std::path::Path;

use wr_core::equiv::automaton_language_equivalent;
use wr_core::numsys::less_than_msd;
use wr_logic::quantify::exists;

#[test]
fn ei_i_lt_x_matches_real_walnut_output() {
    let mut ours = less_than_msd(2);
    ours.label = vec!["i".to_string(), "x".to_string()];

    let mut labels = BTreeSet::new();
    labels.insert("i".to_string());
    exists(&mut ours, &labels).expect("quantifying a real free variable must succeed");

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/spike_ei_i_lt_x.txt");
    let mut ground_truth =
        wr_io::reader::read_automaton_txt(&fixture).expect("fixture must parse cleanly");

    // Sanity on the reduced automaton's shape before the language check, so a
    // failure here doesn't get misdiagnosed as an oracle bug.
    assert_eq!(ours.label, vec!["x".to_string()]);
    assert_eq!(ground_truth.alphabet, vec![vec![0, 1]]);

    // `wr_core::equiv::automaton_language_equivalent` (U8) checks `Automaton::alphabet`
    // for exact positional equality itself before ever touching the underlying `Fa`s —
    // this used to be a manual `assert_eq!(ours.alphabet, ground_truth.alphabet, ...)`
    // work-around here (see git history), needed because the raw `Fa`-level oracle
    // (`language_equivalent`) only ever checks `alphabet_size`, never track content or
    // order. That work-around is no longer needed: the oracle call below now enforces
    // the same guarantee (returning `Err(MismatchedTrackStructure)` instead of a
    // confidently wrong verdict) as an integral part of the comparison itself.
    ours.fa.totalize(0);
    ground_truth.fa.totalize(0);

    assert_eq!(
        automaton_language_equivalent(&ours, &ground_truth),
        Ok(true),
        "the Rust pipeline's result must be language-equivalent to real walnut-java's \
         output for `eval spike \"?msd_2 Ei i<x\";` (see ../CAPTURE.md)"
    );
}
