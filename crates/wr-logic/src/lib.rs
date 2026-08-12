// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `wr-logic` — the first-order-logic layer (the must-have decider spine).
//!
//! Ports Walnut's `Main/Predicate` + `Main/EvalComputations/{Token,Expressions}`
//! (hand-written lexer + shunting-yard parser → AST), plus the driving of
//! `AutomatonLogicalOps` (and/or/xor/imply/iff/not) and `AutomatonQuantification`
//! (∃ = projection+determinize; ∀ = ¬∃¬).
//!
//! HIGH-RISK port surface: the parser has no grammar file — its exact semantics
//! are pinned by the golden corpus. Cost driver of the decision procedure is
//! quantifier ALTERNATION (each ∃ is a projection→NFA→determinize); see
//! DESIGN.md §5.

pub mod predicate_env;
pub mod quantify;
