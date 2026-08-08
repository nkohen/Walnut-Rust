// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `numsys` — base-k numeration systems (msd/lsd only).
//!
//! Ports the base-k paths of Walnut's `Automata/NumberSystem`. Lives INSIDE
//! `wr-core` (not a separate crate) on purpose: in Walnut, `Automaton` holds a
//! `List<NumberSystem>` field (19 refs) and `NumberSystem` references `Automaton`
//! 121 times and constructs it — genuine bidirectional coupling that a crate
//! boundary cannot express (adversarial-review kit-finding #1). The adder,
//! comparator, and constant automata over base-k that the FOL decider composes
//! all live here, alongside the automaton types they produce.
//!
//! DROPPED: Ostrowski / Fibonacci / Pell / negative bases (DESIGN.md §3).
//! Property targets (Tier 4): the adder automaton computes real addition; the
//! comparator is a total order; msd and lsd agree after reversal.

/// Placeholder so the module compiles from the first commit; replace as the port lands.
pub fn placeholder() {}
