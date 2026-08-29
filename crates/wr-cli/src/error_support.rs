// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Shared codegen for `wr-cli`'s ~19 per-command error enums — U2 of the idiomatic-Rust
//! refactor, stage 2 (`docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`).
//!
//! Every command error enum in this crate repeats the same two-part boilerplate: a
//! trivial `impl std::error::Error for X {}` marker, and — for however many of its
//! variants do nothing but wrap a foreign error type in a single field — an `impl
//! From<Foreign> for X` that is just `X::Variant(e)`. [`simple_error_froms`] generates
//! both from one declarative call site, replacing 18 of the 19 hand-written marker
//! impls (`ProverError`'s own marker stays hand-written — it lives in the frozen
//! `crate::prover`, see `docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`) and 29 of the 31
//! hand-written `From` blocks `crates/wr-cli/tests/error_channel_snapshots.rs` (stage
//! 1's commit) pinned before this refactor touched anything. Every one of this crate's
//! `Display` impls, and every doc comment (including `WB-0xx` cross-references), is
//! untouched — only the constructor plumbing around them moves.
//!
//! # What is deliberately NOT covered
//!
//! A `From` impl whose body is more than "wrap `e` in one variant" is real logic, not
//! duplication, and stays exactly where it was, hand-written:
//! - `EvalDefError`'s `From<wr_io::matrix_writer::MatrixWriteError>` — a real multi-arm
//!   match distributing several source variants across several differently-shaped target
//!   variants.
//! - `SplitError`'s `From<wr_core::numsys::NumSysError>` — converts the source error to
//!   a `String` via `.to_string()` before wrapping, not a bare wrap.
//!
//! # Why a macro, not a shared error type
//!
//! Each enum's variant list, `Display` text, and doc comments are genuinely distinct per
//! command — collapsing them into one shared type would either lose that per-command
//! shape or just relocate the same ~19 variant lists into one giant enum, without
//! actually removing any duplication. The real duplication is entirely in the
//! constructor plumbing that surrounds those variants, which is exactly what this
//! declarative macro removes without touching a single `Display` arm, `is_handled`/
//! `kind` classification in `crate::prover`, or doc comment.
//!
//! # Prior art: `crate::prover`'s own `prover_error_from!`
//!
//! `crate::prover` already has a macro doing the same single-wrap `From` dedup for
//! `ProverError` itself (`prover_error_from! { MetaCommandError => Meta, EvalDefError =>
//! EvalDef, … }`) — this crate had already reached for exactly this pattern once before,
//! just scoped to one enum. [`simple_error_froms`] is a strict superset: it also emits
//! the `std::error::Error` marker `prover_error_from!` doesn't need (`ProverError`'s own
//! marker was always separate), and it is reusable across every OTHER command enum
//! rather than hardcoded to `ProverError`. Unifying the two — e.g. rewriting
//! `prover_error_from!` in terms of `simple_error_froms!`, or vice versa — is a
//! deliberate follow-up for whenever `crate::prover` is unfrozen; it is out of scope
//! here because `prover.rs` is one of this unit's own frozen files
//! (`docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`), and this stage's mandate was reducing
//! the per-COMMAND duplication the ~19 satellite modules carried, not touching
//! `prover.rs` itself.

/// `simple_error_froms!(SomeError);` alone emits `impl std::error::Error for SomeError
/// {}`. Each optional trailing `Foreign => Variant` pair additionally emits `impl
/// From<Foreign> for SomeError { fn from(e: Foreign) -> Self { SomeError::Variant(e) } }`
/// — the exact shape every hand-written instance of this pattern already had, so
/// expanding a call site here is semantically identical to the code it replaces (the
/// expansion spells the constructor as `<SomeError>::Variant`, a type-relative path, not
/// necessarily the literal original spelling — e.g. a file that used to write the bare
/// `SomeError::Variant` unqualified still behaves identically, but is not a byte-for-byte
/// match of the removed source text).
macro_rules! simple_error_froms {
    ($ty:ty $(, $from:ty => $variant:ident)* $(,)?) => {
        impl std::error::Error for $ty {}
        $(
            impl From<$from> for $ty {
                fn from(e: $from) -> Self {
                    <$ty>::$variant(e)
                }
            }
        )*
    };
}

pub(crate) use simple_error_froms;
