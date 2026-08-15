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
//!
//! Scope note: `wr-core` ports the WHOLE `Automata/` Java package — `FA`,
//! `Automaton`, `NumberSystem` (base-k, see [`numsys`]), plus `Morphism`,
//! `WordAutomaton`, `RichAlphabet`, `AutomatonDFA` — because these are mutually
//! coupled in one flat Java package and cannot be split across crates without
//! breaking mechanical fidelity (kit-findings #1/#2). A full file-by-file
//! KEEP/DROP + boundary map of `Automata/` is a Phase 0 deliverable.
//!
//! Also here (Phase 3a, U0a): [`logging`], the progress-reporting/log-buffer state
//! ported from Java's `Main/Logging.java`. Despite its Java package, 121 of its call
//! sites are inside `Automata/` (this crate's own scope), so a `wr-cli`-resident
//! placement would invert the crate dependency graph — see that module's docs for
//! the full crate-boundary reasoning.

pub mod automaton;
pub mod determinize;
pub mod equiv;
pub mod fa;
pub mod infinite;
pub mod logging;
pub mod logicalops;
pub mod minimize;
pub mod morphism;
pub mod numsys;
pub mod product;
pub mod quantify;
pub mod regex;
pub mod search;
pub mod transducer;
pub mod trim;
pub mod util;
pub mod walnut_panic;
pub mod word_automaton;
