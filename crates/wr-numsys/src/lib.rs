// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `wr-numsys` — base-k numeration systems (msd/lsd only).
//!
//! Ports the base-k paths of Walnut's `Automata/NumberSystem` (the largest,
//! messiest Java file). DROPPED: Ostrowski / Fibonacci / Pell / negative bases
//! (DESIGN.md §3). Provides the adder, comparator, and constant automata over
//! base-k that the FOL decider composes.
//!
//! Property targets (Tier 4): the adder automaton computes real addition; the
//! comparator is a total order; msd and lsd agree after reversal.

/// Placeholder so the crate compiles from the first commit; replace as the port lands.
pub fn placeholder() {}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        super::placeholder();
    }
}
