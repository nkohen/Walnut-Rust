// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! Scoped, opt-in instrumentation of the decision procedure: a **construction
//! observer** (the peak-state / subset-construction trajectory a consumer needs to tell
//! a *real* state explosion from a *transient* one) and a **resource budget** (the
//! in-engine `-Xmx` analog `docs/EMBEDDING-RESOURCE-SAFETY.md` says this engine lacks),
//! reported as a clean, structured [`Exhausted`] error instead of allocating until the
//! OS kills the process.
//!
//! **No Java counterpart.** Walnut has neither; this module is walnut-rs's own, written
//! for the downstream research consumer (`docs/CT-RESEARCH-INTEGRATION.md`).
//!
//! # Design: a thread-local scope, not a threaded parameter
//!
//! Every construction primitive that can blow up — [`crate::determinize`]'s subset
//! construction, [`crate::product`]'s cross-product BFS, [`crate::minimize`] — is
//! reached through a deep, already-reviewed call chain (`wr-logic`'s `act()` bodies,
//! `wr-cli`'s command handlers, `NumberSystem`'s own internal constructions). Threading
//! a new parameter through all of it would touch every signature on the drop-in path
//! for a feature that is off by default. Instead, [`Instrumentation::enter`] installs
//! the budget and observers in a **thread-local** slot for the duration of a scope, and
//! each primitive snapshots that slot once, at entry ([`Meter::current`]). When nothing
//! is installed the snapshot is empty and every per-state check is one predictable
//! branch — the existing behavior, bit for bit, which is what the drop-in contract
//! requires.
//!
//! Thread-local (not a process-wide static) for the same reason `determinize.rs`'s
//! private `Schedule` parameter is not an environment variable: `cargo test` runs many
//! tests concurrently in one process, and a global would race them. The parallel
//! subset-construction workers (`std::thread::scope`) never consult the slot — every
//! check and every observer call happens on the thread that entered the scope, at the
//! points where that thread merges the workers' results.
//!
//! # How exhaustion propagates
//!
//! A breached cap raises a Rust panic whose payload is the [`Exhausted`] value
//! (`std::panic::panic_any`). That is deliberate, and it is this port's established idiom
//! for "an exception that unwinds out of the decision procedure" —
//! [`crate::walnut_panic`] models Java's `WalnutException`s the same way, and Java's own
//! `OutOfMemoryError` is exactly an unwinding `Throwable`. Unwinding is also what frees
//! the memory: every partially-built automaton on the stack is dropped on the way out.
//!
//! The payload is typed so the boundaries can recognize it:
//! [`crate::walnut_panic::catch_walnut_panic`] (the inner boundary `wr-logic`'s eval loop
//! and several `wr-cli` commands draw) **re-raises** it untouched — an `OutOfMemoryError`
//! is not a `RuntimeException`, so Java's `catch (RuntimeException)` would not have
//! absorbed it either — and [`run`] / `wr-cli`'s dispatch boundary turn it into a
//! `Result`. A consumer never sees a raw panic: [`run`] returns
//! `Err(BudgetError::Exhausted(..))`, and `wr_cli::prover::Prover::dispatch` returns the
//! corresponding `ProverError`.
//!
//! # What the state cap measures
//!
//! `max_states` bounds the state count of **any single automaton under construction**:
//! the metastate list of a subset construction, the pair list of a cross product, the
//! input of a minimization. It is checked at every insertion point on the constructing
//! thread (after each merged metastate / each expanded pair), so the overshoot before
//! the error is at most one metastate's out-degree — a fast spike cannot slip between
//! checks the way it can slip between an external sampler's ticks.
//!
//! # What the memory cap measures, and why it needs an allocator hook
//!
//! `max_bytes` bounds the process's **live heap bytes** as counted by a tracking global
//! allocator — the same quantity `-Xmx` bounds. `wr-core` is `unsafe`-free and a
//! `GlobalAlloc` impl is inherently `unsafe`, so the allocator wrapper lives in `wr-cli`
//! (`wr_cli::tracking_alloc::TrackingAllocator`, linked by the shipped `walnut-rs`
//! binary); it reports through [`memory_meter`]. An embedder that keeps its own allocator
//! wraps it the same way. The wrapper **counts only once a memory cap asks it to**
//! ([`memory_meter::enable`], called by [`Instrumentation::validate`] when `max_bytes` is
//! set): until then every allocation pays one relaxed load of a never-written flag, so a
//! session that sets no memory cap runs the allocator exactly as before. **Without a
//! linked wrapper a memory cap cannot be enforced**, and this module refuses to pretend
//! otherwise: `enable` proves the wrapper is there by allocating and watching the counter
//! move, and [`Instrumentation::enter`] fails with [`MemoryMeterMissing`] rather than
//! silently skipping the check (fail closed — this is a safety feature). The count is
//! the net of allocations and frees **since counting started**: `wr_cli::prover::Prover`
//! (hence every `Engine`) starts it at construction when a wrapper is linked, and the
//! shipped binary does so before its first command, so for those the count is effectively
//! process-wide and an embedder's own live data counts toward the cap, exactly as it would
//! toward a JVM heap ceiling. Heap that was already live before counting started is
//! invisible to it — an embedder that builds large data *before* its first `Engine` and
//! wants a true process ceiling calls [`memory_meter::enable`] itself at the top of `main`.
//! Size the cap accordingly (or compute it from [`memory_meter::live_bytes`] once counting
//! is on).
//!
//! # Where the checks are, exactly, and what they cannot see
//!
//! Three primitives are budgeted: [`crate::determinize::subset_construction`] (also
//! `SC_OTF` and both halves of Brzozowski) checks **both caps after every newly discovered
//! metastate** on the sequential and the parallel path alike, so the state overshoot is at
//! most one metastate's out-degree; on the parallel path the workers additionally check
//! the memory cap before expanding each chunk, so the memory overshoot there is bounded by
//! one chunk's expansion output plus what the other workers produce concurrently, not by
//! a whole BFS level. [`crate::product::cross_product_internal`] checks both caps once per
//! expanded pair. [`crate::minimize::minimize`] checks both **once, at entry**, on its
//! input's state count — its own working set is proportional to that input (which was
//! itself built under the same caps), and it is not checked again while it runs.
//! Everything else on an `eval` path — `Fa::reverse`, the zero fixups and quotients in
//! `logicalops`, the regex Thompson construction, `NumberSystem` construction, `search`,
//! `infinite` — is **unbudgeted**; each is linear in an automaton that one of the three
//! checked primitives produced, which is why they are not checked, but a cap is a bound on
//! what those three build, not on the process. Two more precise notes: the initial
//! metastate of a subset construction is checked like every other one (so a cap of `0`
//! breaches at `at = 1`); and `SC_OTF`'s simulation preorder ([`crate::otf`]) is
//! allocated in one piece (a `q²`-bit matrix plus a sparse successor table, both bounded
//! by [`OtfPolicy`]'s work guard) and the memory cap is checked once right after it is
//! built, before the construction starts — not while it is being built. Wall-clock time is not bounded at all: a
//! query can run for hours within both caps. The external watchdog
//! `docs/EMBEDDING-RESOURCE-SAFETY.md` prescribes is still required for that, and for the
//! in-process-cannot-be-interrupted case that document explains.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use crate::determinize::Strategy;
use crate::minimize::Minimizer;
use crate::otf::OtfPolicy;

// ---------------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------------

/// Caps on what one scope may build. `None` = unlimited. `Default` is fully unlimited,
/// i.e. no check ever fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceBudget {
    /// Maximum state count of any single automaton under construction. See the module
    /// docs for exactly where this is checked.
    pub max_states: Option<usize>,
    /// Maximum live heap bytes, process-wide, as counted by the installed
    /// [`memory_meter`]. Requires a meter; see the module docs.
    pub max_bytes: Option<usize>,
}

impl ResourceBudget {
    /// No caps at all.
    pub const UNLIMITED: ResourceBudget = ResourceBudget {
        max_states: None,
        max_bytes: None,
    };

    /// A budget capping only the state count.
    pub fn states(max_states: usize) -> Self {
        ResourceBudget {
            max_states: Some(max_states),
            max_bytes: None,
        }
    }

    /// Whether any cap is set.
    pub fn is_unlimited(&self) -> bool {
        self.max_states.is_none() && self.max_bytes.is_none()
    }

    /// Enforce both caps for `operation`, whose automaton currently has `states` states.
    /// Raises the typed exhaustion panic on a breach (see the module docs). This is the
    /// `Copy`, `Send` form of [`Meter::check`], for a worker thread that holds no
    /// [`Meter`].
    #[inline]
    pub fn check(&self, operation: Operation, states: usize) {
        if let Some(limit) = self.max_states {
            if states > limit {
                exhaust(Exhausted {
                    reason: ExhaustedReason::States,
                    operation,
                    at: states,
                    limit,
                });
            }
        }
        self.check_memory(operation);
    }

    /// Enforce only the memory cap (for a point where the state count is not at hand).
    #[inline]
    pub fn check_memory(&self, operation: Operation) {
        if let Some(limit) = self.max_bytes {
            // `enter` refused a memory cap unless the meter was enabled, and the meter is
            // never switched off, so a `None` here is a broken invariant -- fail LOUDLY
            // rather than read it as "nothing live" (which would be fail-open).
            let live = memory_meter::live_bytes()
                .expect("a memory-capped scope requires the memory meter to be enabled");
            if live > limit {
                exhaust(Exhausted {
                    reason: ExhaustedReason::Memory,
                    operation,
                    at: live,
                    limit,
                });
            }
        }
    }
}

/// Which cap of a [`ResourceBudget`] was breached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExhaustedReason {
    /// `max_states`.
    States,
    /// `max_bytes`.
    Memory,
}

/// The construction primitive that was running when a cap was breached, or that an
/// [`Event`] describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    /// [`crate::determinize::subset_construction`] (also the two inner steps of
    /// [`crate::determinize::brzozowski`]).
    SubsetConstruction,
    /// [`crate::product::cross_product_internal`] — every boolean connective.
    CrossProduct,
    /// [`crate::minimize::minimize`].
    Minimize,
}

impl Operation {
    /// Human-readable name, as used in [`Exhausted`]'s message.
    pub fn name(self) -> &'static str {
        match self {
            Operation::SubsetConstruction => "subset construction",
            Operation::CrossProduct => "cross product",
            Operation::Minimize => "minimization",
        }
    }
}

/// A [`ResourceBudget`] cap was breached. The structured verdict a consumer asked for
/// in place of an OS kill: which cap, the measured value at the breach, the limit, and
/// which primitive was running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exhausted {
    pub reason: ExhaustedReason,
    /// The primitive that was constructing when the cap was breached.
    pub operation: Operation,
    /// The measured value (states, or live bytes) at the moment of the breach.
    pub at: usize,
    /// The cap it breached.
    pub limit: usize,
}

impl Exhausted {
    /// The verdict token a shell consumer's watchdog vocabulary uses:
    /// `EXPLODED-states` or `EXPLODED-mem`. This is the first word of
    /// [`Exhausted`]'s `Display` output, so `grep -o 'EXPLODED-[a-z]*'` finds it.
    pub fn verdict(&self) -> &'static str {
        match self.reason {
            ExhaustedReason::States => "EXPLODED-states",
            ExhaustedReason::Memory => "EXPLODED-mem",
        }
    }
}

impl fmt::Display for Exhausted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            ExhaustedReason::States => write!(
                f,
                "{}: {} reached {} states (limit {})",
                self.verdict(),
                self.operation.name(),
                self.at,
                self.limit
            ),
            ExhaustedReason::Memory => write!(
                f,
                "{}: live heap reached {} bytes (limit {}) during {}",
                self.verdict(),
                self.at,
                self.limit,
                self.operation.name()
            ),
        }
    }
}

impl std::error::Error for Exhausted {}

/// A [`ResourceBudget`] with `max_bytes` set was entered while no tracking allocator is
/// linked ([`memory_meter::enable`] found no counter movement), so the memory cap could
/// not be enforced. Refused rather than silently skipped — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryMeterMissing;

impl fmt::Display for MemoryMeterMissing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a memory budget (max_bytes) was requested but no tracking allocator is \
             installed (see wr_core::resource::memory_meter); the cap cannot be enforced"
        )
    }
}

impl std::error::Error for MemoryMeterMissing {}

/// Everything [`run`] can fail with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetError {
    Exhausted(Exhausted),
    MemoryMeterMissing(MemoryMeterMissing),
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BudgetError::Exhausted(e) => write!(f, "{e}"),
            BudgetError::MemoryMeterMissing(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BudgetError {}

impl From<Exhausted> for BudgetError {
    fn from(e: Exhausted) -> Self {
        BudgetError::Exhausted(e)
    }
}

impl From<MemoryMeterMissing> for BudgetError {
    fn from(e: MemoryMeterMissing) -> Self {
        BudgetError::MemoryMeterMissing(e)
    }
}

// ---------------------------------------------------------------------------------
// Memory meter (the hook a tracking allocator reports through)
// ---------------------------------------------------------------------------------

/// The process-wide live-heap counter a tracking global allocator maintains, and this
/// module's memory cap reads.
///
/// `wr-core` deliberately contains no `GlobalAlloc` impl (it is `unsafe`-free). The
/// wrapper that calls [`allocated`](memory_meter::allocated) /
/// [`freed`](memory_meter::freed) from its `alloc`/`dealloc`/`realloc` lives in
/// `wr_cli::tracking_alloc`; an embedder with its own allocator writes the same
/// three-line wrapper around it.
///
/// **Counting is off until [`enable`](memory_meter::enable) turns it on** — until then the
/// wrapper costs one relaxed load of a flag per allocation, which is what keeps a process
/// that never enables it on the allocator's own fast path. Once enabled, the counter is
/// the net of allocations and frees *since enabling*: heap live before that moment is
/// never counted, and a block allocated before and freed after pushes the net below zero,
/// which [`live_bytes`](memory_meter::live_bytes) clamps to `0` — an underestimate bounded
/// by the heap that was live at enable time, never a spurious overestimate. So enable
/// **early**: `wr_cli::prover::Prover::with_output` (every `Engine`, the binary) does it at
/// construction; a memory-capped [`Instrumentation`] does it on `validate`/`enter` as a
/// backstop; an embedder wanting a true process ceiling calls it first thing in `main`.
pub mod memory_meter {
    use super::MemoryMeterMissing;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);
    /// Set by the wrapper on its first `alloc`, never cleared: "a tracking allocator is
    /// this process's global allocator". Any program has allocated long before it can
    /// call [`enable`], so this is reliable from `main` onward — and unlike a probe it
    /// cannot race a concurrent `enable` or a concurrent free (an earlier draft proved
    /// linkage by allocating 64 KiB and watching the counter, which a second thread's
    /// simultaneous first `enable` could observe as "unmoved" and switch counting OFF
    /// under a scope that had just been told it was on).
    static LINKED: AtomicBool = AtomicBool::new(false);
    static ENABLED: AtomicBool = AtomicBool::new(false);

    /// Report an allocation of `bytes`. Until enabled, two relaxed loads and (once) a
    /// store of the link flag.
    #[inline]
    pub fn allocated(bytes: usize) {
        if !LINKED.load(Ordering::Relaxed) {
            LINKED.store(true, Ordering::Relaxed);
        }
        if ENABLED.load(Ordering::Relaxed) {
            LIVE_BYTES.fetch_add(bytes as isize, Ordering::Relaxed);
        }
    }

    /// Report a deallocation of `bytes`. A no-op (one relaxed load) until enabled.
    #[inline]
    pub fn freed(bytes: usize) {
        if ENABLED.load(Ordering::Relaxed) {
            LIVE_BYTES.fetch_sub(bytes as isize, Ordering::Relaxed);
        }
    }

    /// Start counting. [`MemoryMeterMissing`] — and nothing changes — if no wrapper has
    /// ever reported an allocation, i.e. no `TrackingAllocator` is the global allocator,
    /// so a memory cap can never be fail-open. Idempotent; once on, never off.
    pub fn enable() -> Result<(), MemoryMeterMissing> {
        if !LINKED.load(Ordering::SeqCst) {
            return Err(MemoryMeterMissing);
        }
        ENABLED.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Whether a tracking allocator is linked (it has reported at least one allocation).
    pub fn is_linked() -> bool {
        LINKED.load(Ordering::Relaxed)
    }

    /// Whether counting is on, i.e. whether [`live_bytes`] means anything.
    pub fn is_enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    /// Live heap bytes right now (net since [`enable`]; clamped at `0`), or `None` when
    /// counting is off.
    pub fn live_bytes() -> Option<usize> {
        if is_enabled() {
            Some(LIVE_BYTES.load(Ordering::Relaxed).max(0) as usize)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------------
// Observer
// ---------------------------------------------------------------------------------

/// One step of a construction, reported to every installed [`Observer`] on the
/// constructing thread, in order. State counts are exact; nothing here is sampled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// [`crate::determinize::determinize`]'s dispatcher is about to run `strategy` on
    /// an automaton of `input_states` states. (Not emitted for direct
    /// [`crate::determinize::subset_construction`] calls, which bypass the dispatcher.)
    Determinize {
        strategy: Strategy,
        input_states: usize,
    },
    /// A subset construction started on an NFA of `input_states` states from an initial
    /// metastate of `initial_size` NFA states.
    SubsetConstructionStarted {
        input_states: usize,
        initial_size: usize,
    },
    /// One BFS level of a subset construction is about to be expanded. `level` counts
    /// from 0; `frontier` is the number of metastates in this level; `members` is the
    /// total number of NFA states across them (the level's "metastate set size");
    /// `metastates` is the total number of distinct metastates discovered so far —
    /// the running peak, since subset construction never discards one.
    SubsetLevel {
        level: usize,
        frontier: usize,
        members: usize,
        metastates: usize,
    },
    /// A subset construction finished with `states` metastates after `levels` levels,
    /// taking `elapsed` wall-clock time. This is the **pre-minimization** size; compare it
    /// with the following [`Event::MinimizeFinished`]'s `after` to diagnose a transient
    /// explosion.
    SubsetConstructionFinished {
        states: usize,
        levels: usize,
        elapsed: Duration,
    },
    /// A cross product started between automata of `left_states` and `right_states`.
    CrossProductStarted {
        left_states: usize,
        right_states: usize,
    },
    /// A cross product finished with `states` pairs discovered, in `elapsed`.
    CrossProductFinished { states: usize, elapsed: Duration },
    /// A minimization started on `states` states.
    MinimizeStarted { states: usize },
    /// A minimization finished: `before` states in, `after` out, in `elapsed`.
    MinimizeFinished {
        before: usize,
        after: usize,
        elapsed: Duration,
    },
    /// [`crate::otf`]: the NFA's simulation preorder was computed (`related_pairs`
    /// ordered pairs, diagonal included) before an `SC_OTF` subset construction.
    SimulationComputed {
        nfa_states: usize,
        related_pairs: usize,
    },
    /// [`crate::otf`]: the NFA exceeded the policy's size guard
    /// ([`OtfPolicy::max_nfa_states`], or [`OtfPolicy::max_preorder_work`] against
    /// `nfa_states * transitions`), so this `SC_OTF` subset construction ran as plain
    /// sequential `SC`.
    SimulationSkipped {
        nfa_states: usize,
        /// Effective `(state, symbol)` transition entries of the NFA.
        transitions: usize,
        policy: OtfPolicy,
    },
}

/// A sink for [`Event`]s. Install one with [`Instrumentation::with_observer`].
///
/// Called synchronously on the constructing thread, so it must not re-enter the engine.
/// [`Trajectory`] is the batteries-included implementation.
pub trait Observer {
    fn observe(&mut self, event: &Event);
}

/// One determinization as reconstructed by [`Trajectory::determinizations`]: the
/// real-vs-transient diagnosis in one record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeterminizationRecord {
    /// NFA states going in.
    pub input_states: usize,
    /// BFS levels the subset construction ran.
    pub levels: usize,
    /// Metastates at the end — the intermediate peak (subset construction never
    /// shrinks, so its final count is its peak).
    pub peak_states: usize,
    /// States after the minimization that immediately followed, if one did. A `peak`
    /// far above `minimized` is a *transient* explosion; `peak ≈ minimized` is *real*.
    pub minimized: Option<usize>,
    /// Wall-clock time of the subset construction itself.
    pub determinize_time: Duration,
    /// Wall-clock time of the minimization that followed, if one did — so the cost of a
    /// transient explosion can be attributed to building it vs collapsing it.
    pub minimize_time: Option<Duration>,
}

/// An [`Observer`] that records every event and keeps the running peaks. Share it with
/// a scope through an `Rc<RefCell<Trajectory>>` and read it back afterwards.
#[derive(Debug, Default, Clone)]
pub struct Trajectory {
    events: Vec<Event>,
    peak_states: usize,
    peak_bytes: usize,
}

impl Trajectory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every event observed, in order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The largest state count any single automaton under construction reached, across
    /// every primitive in the scope.
    pub fn peak_states(&self) -> usize {
        self.peak_states
    }

    /// The largest live-heap reading taken at any **event** (level / primitive
    /// boundary), or `0` when the memory meter is not enabled. Coarser than the budget's
    /// own per-state checks, and not an allocator-level high-water mark.
    pub fn peak_bytes(&self) -> usize {
        self.peak_bytes
    }

    /// Forget everything recorded so far (the peaks included).
    pub fn clear(&mut self) {
        self.events.clear();
        self.peak_states = 0;
        self.peak_bytes = 0;
    }

    /// Pair each subset construction with the minimization that followed it.
    ///
    /// A `MinimizeFinished` is attributed to the **last** finished subset construction,
    /// and only if that record has no minimization yet and its output size equals the
    /// minimization's `before` (Brzozowski's intermediate minimize pairs with its first
    /// step this way, and `determinize_and_minimize`'s with its only step); otherwise the
    /// minimization is not attributed to anything. A minimization that follows no
    /// construction at all — e.g. of an already-deterministic automaton — is likewise not
    /// a record here.
    pub fn determinizations(&self) -> Vec<DeterminizationRecord> {
        let mut out: Vec<DeterminizationRecord> = Vec::new();
        let mut open: Option<(usize, usize, usize)> = None; // (input, levels, peak)
        for e in &self.events {
            match e {
                Event::SubsetConstructionStarted { input_states, .. } => {
                    open = Some((*input_states, 0, 0));
                }
                Event::SubsetLevel { metastates, .. } => {
                    if let Some(o) = open.as_mut() {
                        o.1 += 1;
                        o.2 = o.2.max(*metastates);
                    }
                }
                Event::SubsetConstructionFinished {
                    states,
                    levels,
                    elapsed,
                } => {
                    if let Some((input_states, _, _)) = open.take() {
                        out.push(DeterminizationRecord {
                            input_states,
                            levels: *levels,
                            peak_states: *states,
                            minimized: None,
                            determinize_time: *elapsed,
                            minimize_time: None,
                        });
                    }
                }
                Event::MinimizeFinished {
                    before,
                    after,
                    elapsed,
                } => {
                    if let Some(last) = out.last_mut() {
                        if last.minimized.is_none() && last.peak_states == *before {
                            last.minimized = Some(*after);
                            last.minimize_time = Some(*elapsed);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

impl Observer for Trajectory {
    fn observe(&mut self, event: &Event) {
        let states = match event {
            Event::SubsetLevel { metastates, .. } => *metastates,
            Event::SubsetConstructionFinished { states, .. }
            | Event::CrossProductFinished { states, .. }
            | Event::MinimizeStarted { states } => *states,
            Event::Determinize { input_states, .. }
            | Event::SubsetConstructionStarted { input_states, .. } => *input_states,
            Event::CrossProductStarted {
                left_states,
                right_states,
            } => (*left_states).max(*right_states),
            Event::MinimizeFinished { before, after, .. } => (*before).max(*after),
            Event::SimulationComputed { nfa_states, .. }
            | Event::SimulationSkipped { nfa_states, .. } => *nfa_states,
        };
        self.peak_states = self.peak_states.max(states);
        if let Some(live) = memory_meter::live_bytes() {
            self.peak_bytes = self.peak_bytes.max(live);
        }
        self.events.push(event.clone());
    }
}

// ---------------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------------

/// A budget plus zero or more observers, installed for the duration of a scope. Cheap
/// to clone (the observers are shared handles).
#[derive(Clone, Default)]
pub struct Instrumentation {
    budget: ResourceBudget,
    observers: Vec<Rc<RefCell<dyn Observer>>>,
    minimizer: Option<Rc<dyn Minimizer>>,
    default_strategy: Option<Strategy>,
    otf_policy: Option<OtfPolicy>,
}

impl fmt::Debug for Instrumentation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Instrumentation")
            .field("budget", &self.budget)
            .field("observers", &self.observers.len())
            .field(
                "minimizer",
                &self.minimizer.as_ref().map(|m| m.name().to_string()),
            )
            .field("default_strategy", &self.default_strategy)
            .field("otf_policy", &self.otf_policy)
            .finish()
    }
}

impl Instrumentation {
    /// No budget, no observers — entering this is a no-op.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_budget(mut self, budget: ResourceBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Add an observer. Several may be installed; each sees every event, in the order
    /// they were added.
    pub fn with_observer(mut self, observer: Rc<RefCell<dyn Observer>>) -> Self {
        self.observers.push(observer);
        self
    }

    /// Route every construction-path minimization (`minimize_with_logging`) through
    /// `minimizer` instead of the ported Valmari. See [`Minimizer`] for the contract.
    pub fn with_minimizer(mut self, minimizer: Rc<dyn Minimizer>) -> Self {
        self.minimizer = Some(minimizer);
        self
    }

    /// The determinization strategy to use wherever no `[strategy …]` metacommand chose
    /// one explicitly (`determinize`'s dispatcher consults
    /// [`crate::determinize::DeterminizeContext::has_explicit_strategy`]). Never applied
    /// to a word automaton (DFAO), which only `SC` handles. The intended value is
    /// [`Strategy::ScOtf`].
    pub fn with_default_strategy(mut self, strategy: Strategy) -> Self {
        self.default_strategy = Some(strategy);
        self
    }

    /// Tunables for [`Strategy::ScOtf`] ([`crate::otf`]); the default applies otherwise.
    pub fn with_otf_policy(mut self, policy: OtfPolicy) -> Self {
        self.otf_policy = Some(policy);
        self
    }

    pub fn budget(&self) -> ResourceBudget {
        self.budget
    }

    pub fn observer_count(&self) -> usize {
        self.observers.len()
    }

    pub fn default_strategy(&self) -> Option<Strategy> {
        self.default_strategy
    }

    pub fn otf_policy(&self) -> Option<OtfPolicy> {
        self.otf_policy
    }

    pub fn minimizer(&self) -> Option<&Rc<dyn Minimizer>> {
        self.minimizer.as_ref()
    }

    /// Whether entering this would do anything at all.
    pub fn is_inert(&self) -> bool {
        self.budget.is_unlimited()
            && self.observers.is_empty()
            && self.minimizer.is_none()
            && self.default_strategy.is_none()
            && self.otf_policy.is_none()
    }

    /// Switches the memory meter on if the budget has a memory cap
    /// ([`memory_meter::enable`]); [`MemoryMeterMissing`] if that finds no tracking
    /// allocator linked. Otherwise a no-op.
    pub fn validate(&self) -> Result<(), MemoryMeterMissing> {
        if self.budget.max_bytes.is_some() {
            memory_meter::enable()?;
        }
        Ok(())
    }

    /// Install this instrumentation on the current thread until the returned guard is
    /// dropped. Nested scopes replace the outer one for their duration and restore it
    /// afterwards.
    ///
    /// A breached cap inside the scope is a **panic** carrying [`Exhausted`] (see the
    /// module docs); a caller of `enter` must draw its own boundary
    /// ([`crate::walnut_panic::catch_walnut_panic_detailed`] +
    /// [`crate::walnut_panic::CaughtPanic::exhausted`]) or use [`run`], which does exactly
    /// that. Left uncaught, it reaches the default panic hook like any other panic.
    pub fn enter(&self) -> Result<Scope, MemoryMeterMissing> {
        self.validate()?;
        let previous = ACTIVE.with(|a| a.replace(Some(self.clone())));
        Ok(Scope { previous })
    }
}

thread_local! {
    static ACTIVE: RefCell<Option<Instrumentation>> = const { RefCell::new(None) };
}

/// The guard returned by [`Instrumentation::enter`]; restores the previously active
/// instrumentation (if any) on drop.
#[must_use = "the instrumentation is uninstalled as soon as this guard is dropped"]
pub struct Scope {
    previous: Option<Instrumentation>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ACTIVE.with(|a| *a.borrow_mut() = previous);
    }
}

/// Run `f` under `instrumentation`, converting a breached cap into
/// `Err(BudgetError::Exhausted(..))`. Any *other* panic escaping `f` is re-raised
/// unchanged. This is the `Result`-shaped entry point for a direct `wr-core` caller;
/// `wr_cli::prover::Prover::dispatch` does the equivalent for a command.
pub fn run<R>(instrumentation: &Instrumentation, f: impl FnOnce() -> R) -> Result<R, BudgetError> {
    let _scope = instrumentation.enter()?;
    match crate::walnut_panic::catch_walnut_panic_detailed(f) {
        Ok(r) => Ok(r),
        Err(caught) => match caught.exhausted() {
            Some(e) => Err(BudgetError::Exhausted(e.clone())),
            None => caught.resume(),
        },
    }
}

// ---------------------------------------------------------------------------------
// Meter — what the primitives use
// ---------------------------------------------------------------------------------

/// A primitive's snapshot of the active [`Instrumentation`], taken once at entry.
///
/// Inert (no budget, no observers) when nothing is installed: [`Meter::check`] is then
/// one branch, [`Meter::emit`] never evaluates its event.
pub struct Meter {
    budget: ResourceBudget,
    observers: Vec<Rc<RefCell<dyn Observer>>>,
    minimizer: Option<Rc<dyn Minimizer>>,
    default_strategy: Option<Strategy>,
    otf_policy: Option<OtfPolicy>,
}

impl Meter {
    /// Snapshot the current thread's active instrumentation.
    pub fn current() -> Meter {
        ACTIVE.with(|a| match &*a.borrow() {
            None => Meter::inert(),
            Some(i) => Meter {
                budget: i.budget,
                observers: i.observers.clone(),
                minimizer: i.minimizer.clone(),
                default_strategy: i.default_strategy,
                otf_policy: i.otf_policy,
            },
        })
    }

    /// A meter that checks and reports nothing.
    pub fn inert() -> Meter {
        Meter {
            budget: ResourceBudget::UNLIMITED,
            observers: Vec::new(),
            minimizer: None,
            default_strategy: None,
            otf_policy: None,
        }
    }

    /// The scope's caller-supplied minimizer, if any.
    pub fn minimizer(&self) -> Option<&Rc<dyn Minimizer>> {
        self.minimizer.as_ref()
    }

    /// The scope's default determinization strategy, if any.
    pub fn default_strategy(&self) -> Option<Strategy> {
        self.default_strategy
    }

    /// The scope's `SC_OTF` policy, if any.
    pub fn otf_policy(&self) -> Option<OtfPolicy> {
        self.otf_policy
    }

    /// Whether any check can fire.
    #[inline]
    pub fn has_budget(&self) -> bool {
        !self.budget.is_unlimited()
    }

    /// The budget alone — `Copy` and `Send`, for a worker thread that cannot hold the
    /// observers.
    #[inline]
    pub fn budget(&self) -> ResourceBudget {
        self.budget
    }

    /// Enforce both caps for `operation`, whose automaton currently has `states`
    /// states. Raises the typed exhaustion panic on a breach (see the module docs).
    #[inline]
    pub fn check(&self, operation: Operation, states: usize) {
        self.budget.check(operation, states)
    }

    /// Report an event to every observer. `event` is only evaluated when there is at
    /// least one, so a caller may compute level statistics inside it for free when
    /// nobody is listening.
    #[inline]
    pub fn emit(&self, event: impl FnOnce() -> Event) {
        if self.observers.is_empty() {
            return;
        }
        let event = event();
        for observer in &self.observers {
            observer.borrow_mut().observe(&event);
        }
    }
}

#[cold]
#[inline(never)]
fn exhaust(e: Exhausted) -> ! {
    std::panic::panic_any(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_trajectory() -> Rc<RefCell<Trajectory>> {
        Rc::new(RefCell::new(Trajectory::new()))
    }

    #[test]
    fn an_inert_meter_checks_nothing_and_evaluates_no_event() {
        let m = Meter::current();
        assert!(!m.has_budget());
        m.check(Operation::Minimize, usize::MAX);
        let mut evaluated = false;
        m.emit(|| {
            evaluated = true;
            Event::MinimizeStarted { states: 1 }
        });
        assert!(!evaluated, "no observer => the event closure must not run");
    }

    #[test]
    fn a_state_cap_raises_the_typed_payload_and_run_returns_it_structured() {
        let instr = Instrumentation::new().with_budget(ResourceBudget::states(10));
        let result = run(&instr, || {
            let m = Meter::current();
            m.check(Operation::CrossProduct, 10); // at the limit: fine
            m.check(Operation::CrossProduct, 11); // over: raises
            unreachable!()
        });
        assert_eq!(
            result,
            Err(BudgetError::Exhausted(Exhausted {
                reason: ExhaustedReason::States,
                operation: Operation::CrossProduct,
                at: 11,
                limit: 10,
            }))
        );
    }

    #[test]
    fn exhausted_displays_the_verdict_token_first() {
        let e = Exhausted {
            reason: ExhaustedReason::States,
            operation: Operation::SubsetConstruction,
            at: 1_000_001,
            limit: 1_000_000,
        };
        assert_eq!(
            e.to_string(),
            "EXPLODED-states: subset construction reached 1000001 states (limit 1000000)"
        );
        let e = Exhausted {
            reason: ExhaustedReason::Memory,
            operation: Operation::Minimize,
            at: 7,
            limit: 5,
        };
        assert_eq!(
            e.to_string(),
            "EXPLODED-mem: live heap reached 7 bytes (limit 5) during minimization"
        );
    }

    #[test]
    fn run_re_raises_a_panic_that_is_not_an_exhaustion() {
        let instr = Instrumentation::new();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(&instr, || panic!("unrelated"))
        }));
        let payload = outcome.expect_err("must re-raise");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"unrelated"));
    }

    #[test]
    fn the_scope_is_restored_on_exit_including_after_exhaustion() {
        assert!(!Meter::current().has_budget());
        let outer = Instrumentation::new().with_budget(ResourceBudget::states(5));
        let _g = outer.enter().unwrap();
        assert_eq!(Meter::current().budget.max_states, Some(5));
        {
            let inner = Instrumentation::new().with_budget(ResourceBudget::states(1));
            let r = run(&inner, || Meter::current().check(Operation::Minimize, 2));
            assert!(matches!(r, Err(BudgetError::Exhausted(_))));
        }
        assert_eq!(
            Meter::current().budget.max_states,
            Some(5),
            "the inner scope must restore the outer one, even when it unwound"
        );
        drop(_g);
        assert!(!Meter::current().has_budget());
    }

    #[test]
    fn a_memory_cap_without_a_meter_is_refused_not_ignored() {
        // No tracking allocator is linked into this test binary, so `enable` must find
        // the counter unmoved and refuse.
        assert_eq!(memory_meter::enable(), Err(MemoryMeterMissing));
        assert!(!memory_meter::is_enabled());
        assert_eq!(memory_meter::live_bytes(), None);
        let instr = Instrumentation::new().with_budget(ResourceBudget {
            max_states: None,
            max_bytes: Some(1),
        });
        assert_eq!(instr.validate(), Err(MemoryMeterMissing));
        assert!(instr.enter().is_err());
        assert_eq!(
            run(&instr, || 1),
            Err(BudgetError::MemoryMeterMissing(MemoryMeterMissing))
        );
    }

    #[test]
    fn observers_see_events_in_order_and_the_trajectory_tracks_the_peak() {
        let t = shared_trajectory();
        let instr = Instrumentation::new().with_observer(t.clone());
        run(&instr, || {
            let m = Meter::current();
            m.emit(|| Event::SubsetConstructionStarted {
                input_states: 3,
                initial_size: 1,
            });
            m.emit(|| Event::SubsetLevel {
                level: 0,
                frontier: 1,
                members: 1,
                metastates: 4,
            });
            m.emit(|| Event::SubsetLevel {
                level: 1,
                frontier: 3,
                members: 7,
                metastates: 9,
            });
            m.emit(|| Event::SubsetConstructionFinished {
                states: 9,
                levels: 2,
                elapsed: Duration::from_millis(5),
            });
            m.emit(|| Event::MinimizeStarted { states: 9 });
            m.emit(|| Event::MinimizeFinished {
                before: 9,
                after: 2,
                elapsed: Duration::from_millis(3),
            });
        })
        .unwrap();
        let t = t.borrow();
        assert_eq!(t.events().len(), 6);
        assert_eq!(t.peak_states(), 9);
        assert_eq!(
            t.determinizations(),
            vec![DeterminizationRecord {
                input_states: 3,
                levels: 2,
                peak_states: 9,
                minimized: Some(2),
                determinize_time: Duration::from_millis(5),
                minimize_time: Some(Duration::from_millis(3)),
            }]
        );
    }

    #[test]
    fn a_minimization_of_an_unrelated_size_is_not_attributed_to_a_determinization() {
        let mut t = Trajectory::new();
        t.observe(&Event::SubsetConstructionStarted {
            input_states: 2,
            initial_size: 1,
        });
        t.observe(&Event::SubsetConstructionFinished {
            states: 5,
            levels: 1,
            elapsed: Duration::ZERO,
        });
        t.observe(&Event::MinimizeFinished {
            before: 7,
            after: 1,
            elapsed: Duration::ZERO,
        });
        assert_eq!(t.determinizations()[0].minimized, None);
        t.clear();
        assert!(t.events().is_empty());
        assert_eq!(t.peak_states(), 0);
    }

    #[test]
    fn two_observers_both_see_every_event() {
        let a = shared_trajectory();
        let b = shared_trajectory();
        let instr = Instrumentation::new()
            .with_observer(a.clone())
            .with_observer(b.clone());
        assert_eq!(instr.observer_count(), 2);
        assert!(!instr.is_inert());
        run(&instr, || {
            Meter::current().emit(|| Event::MinimizeStarted { states: 4 });
        })
        .unwrap();
        assert_eq!(a.borrow().events(), b.borrow().events());
        assert_eq!(a.borrow().events().len(), 1);
    }

    #[test]
    fn instrumentation_is_inert_by_default() {
        assert!(Instrumentation::new().is_inert());
        assert!(ResourceBudget::default().is_unlimited());
        assert!(Instrumentation::new().validate().is_ok());
    }
}
