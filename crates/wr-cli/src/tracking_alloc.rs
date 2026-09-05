// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! A global-allocator wrapper that reports live heap bytes to
//! [`wr_core::resource::memory_meter`], which is what makes a
//! [`wr_core::resource::ResourceBudget::max_bytes`] cap enforceable — the in-engine
//! `-Xmx` analog (`docs/EMBEDDING-RESOURCE-SAFETY.md`).
//!
//! It lives here rather than in `wr-core` because a `GlobalAlloc` impl is inherently
//! `unsafe` and `wr-core` is kept `unsafe`-free. The shipped `walnut-rs` binary installs
//! `TrackingAllocator<mimalloc::MiMalloc>`; an embedder that links `wr-cli` picks its
//! own allocator and may wrap it the same way:
//!
//! ```ignore
//! #[global_allocator]
//! static GLOBAL: wr_cli::tracking_alloc::TrackingAllocator<std::alloc::System> =
//!     wr_cli::tracking_alloc::TrackingAllocator(std::alloc::System);
//! ```
//!
//! Cost: one relaxed **load** of a flag per allocation/free while no memory cap has
//! enabled counting (the shipped binary's default), and one relaxed atomic add on a
//! shared counter per allocation/free once `WR_MAX_BYTES` / `ResourceBudget::max_bytes`
//! has enabled it (`memory_meter::enable`). The counting path is the price of the
//! `-Xmx` analog and is paid only by sessions that ask for one; a direction-only A/B of
//! the two paths is recorded in `docs/CT-RESEARCH-INTEGRATION.md`.

use std::alloc::{GlobalAlloc, Layout};

use wr_core::resource::memory_meter;

/// Wraps any [`GlobalAlloc`], forwarding every call and keeping
/// [`memory_meter::live_bytes`] current.
pub struct TrackingAllocator<A: GlobalAlloc>(pub A);

// SAFETY: every method forwards to the wrapped allocator with the same arguments and
// returns its result unchanged; the only addition is bookkeeping on two atomics, which
// allocates nothing and cannot fail. The wrapped allocator's own contract is therefore
// preserved exactly.
unsafe impl<A: GlobalAlloc> GlobalAlloc for TrackingAllocator<A> {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = self.0.alloc(layout);
        if !p.is_null() {
            memory_meter::allocated(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.dealloc(ptr, layout);
        memory_meter::freed(layout.size());
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = self.0.alloc_zeroed(layout);
        if !p.is_null() {
            memory_meter::allocated(layout.size());
        }
        p
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = self.0.realloc(ptr, layout, new_size);
        if !p.is_null() {
            // The old block is gone and a `new_size` one is live; report the net
            // change in the order that never lets the counter underflow.
            memory_meter::allocated(new_size);
            memory_meter::freed(layout.size());
        }
        p
    }
}
