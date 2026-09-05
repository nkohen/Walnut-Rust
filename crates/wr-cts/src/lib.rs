// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `wr-cts` — the Tier-4 **cross-check** crate: independent second implementations
//! used to corroborate `wr-core`'s ported algorithms.
//!
//! # Status of the original "reuse the RustConstantTermSequences substrate" plan
//!
//! `docs/DESIGN.md` §4 proposed reusing the user's existing `RustConstantTermSequences`
//! crate here — its GF(p) linear algebra, `ModInt`/`LaurentPoly` arithmetic, and (once
//! generalized) its DFAO Moore minimizer as the Tier-4 independent second minimizer.
//!
//! For the minimizer specifically, that plan **did not work out**: direct investigation
//! of the substrate found no usable `minimize` entry point for this crate's automata (it
//! is deterministic single-track / `DFAO<ModInt, S>`-specific, matching the caveat
//! already recorded in DESIGN.md §4 and adversarial finding F6). Per the user's explicit
//! decision, [`moore`] therefore contains a small **standalone** Moore
//! partition-refinement minimizer written from the textbook definition, operating
//! directly on [`wr_core::fa::Fa`]. It has no dependency on the substrate, so no git
//! dependency is wired for it.
//!
//! The substrate's *numeric* primitives remain a candidate for later reuse; nothing here
//! forecloses that.
//!
//! # The substrate bridge (2026-09, feature `substrate`, on by default)
//!
//! [`bridge`] converts between [`wr_core::fa::Fa`] / [`wr_core::automaton::Automaton`]
//! and the substrate's `DFAO<ModInt, S>` in both directions, so an already-minimal
//! constant-term DFAO computed on the substrate can be registered directly into an
//! engine session (`wr_cli::embed::Engine::register_word_automaton`) and the engine's
//! automata can be handed to the substrate's linear-representation machinery — no
//! `.txt` round trip. The dependency is a pinned git commit and optional; `wr-cli`
//! disables it so the shipped binary never links it.

pub mod moore;

#[cfg(feature = "substrate")]
pub mod bridge;
