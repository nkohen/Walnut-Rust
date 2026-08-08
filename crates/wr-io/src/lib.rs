// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `wr-io` — Walnut `.txt` automaton I/O + Graphviz.
//!
//! Ports `Automata/AutomatonReader` + `ParseMethods` + `Automata/Writer/AutomatonWriter`.
//! MUST support Walnut's real MULTI-TRACK format (e.g. header `lsd_2 lsd_2 lsd_2 lsd_2`,
//! one digit per track per transition) AND the NFA / `T` / `F` trivial cases —
//! the existing RustConstantTermSequences serializer is single-track LSD only and
//! is NOT sufficient here (adversarial finding F6, DESIGN.md §4).
//!
//! File-format fidelity is load-bearing: the clone must READ existing Walnut files
//! and WRITE files Walnut can load. (Comparison, however, is by semantic
//! equivalence in `wr-core`, not by textual identity.)

/// Placeholder so the crate compiles from the first commit; replace as the port lands.
pub fn placeholder() {}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        super::placeholder();
    }
}
