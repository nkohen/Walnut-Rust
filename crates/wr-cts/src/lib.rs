// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `wr-cts` — adapter over the RustConstantTermSequences primitives.
//!
//! Reuses the genuinely-reusable primitives from the user's existing
//! `RustConstantTermSequences` crate — GF(p) linear algebra, ModInt / LaurentPoly
//! arithmetic — and, once generalized, the DFAO Moore minimizer as the Tier-4
//! INDEPENDENT second minimizer (cross-check vs the ported Valmari).
//!
//! NOTE: the substrate is deterministic single-track only; its DFAO/serializer are
//! NOT drop-in for `wr-core`/`wr-io` (adversarial finding F6). Wire it as a Cargo
//! git dependency when ready:
//!   # [dependencies]
//!   # rust-constant-term-sequences = { git = "<substrate repo url>" }

/// Placeholder so the crate compiles from the first commit; replace as the port lands.
pub fn placeholder() {}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        super::placeholder();
    }
}
