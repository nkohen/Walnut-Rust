// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `wr-core` — the finite-automaton engine.
//!
//! Ports Walnut's `Automata/FA/*` and the automaton-level operations of
//! `Automata/Automaton` + `AutomatonLogicalOps` + `AutomatonQuantification`.
//!
//! Responsibilities:
//! - DFA / NFA / DFAO representation (multi-track digit alphabets).
//! - `determinize` (subset construction `SC` — the default; plain Brzozowski).
//! - `minimize` (Valmari), `product`/cross-product, `reverse`, `quotient`.
//! - `∃`-projection (produces an NFA → determinize) — the decision-procedure crux.
//! - **The semantic language-equivalence oracle** (`product + complement +
//!   emptiness of symmetric difference`) — this is the comparison bar for all
//!   differential/golden testing (Walnut's own suite uses `faEqual`/Brics
//!   language equivalence, NOT byte identity). Required deliverable — see
//!   DESIGN.md §5 and adversarial finding F1.
//!
//! Port discipline: mechanical transliteration first, idiomatic later. Compare
//! by SEMANTICS, never by exact state numbering.

/// Placeholder so the crate compiles from the first commit; replace as the port lands.
pub fn placeholder() {}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        super::placeholder();
    }
}
