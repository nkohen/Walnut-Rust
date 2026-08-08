// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `walnut-rs` CLI — ports Walnut's `Main/Prover` command dispatch + `Session`.
//!
//! Refactor Walnut's global `static` state into an explicit `Session` context
//! (the one sanctioned deviation from mechanical fidelity). Port the ~20
//! regex-dispatched commands of `Prover.java` LAST, after `wr-core`/`wr-logic`
//! are differentially green, so divergences have a small suspect surface
//! (adversarial finding F9, DESIGN.md §6).

fn main() {
    eprintln!("walnut-rs: scaffold only — see docs/DESIGN.md for the build plan.");
}
