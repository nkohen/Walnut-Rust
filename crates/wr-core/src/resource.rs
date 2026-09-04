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
//! (`wr_cli::tracking_alloc::TrackingAllocator`, installed by the shipped `walnut-rs`
//! binary); it reports through [`memory_meter`]. An embedder that keeps its own allocator
//! wraps it the same way. **Without an installed meter a memory cap cannot be enforced**,
//! and this module refuses to pretend otherwise: [`Instrumentation::enter`] fails with
//! [`MemoryMeterMissing`] rather than silently skipping the check (fail closed — this is a
//! safety feature). The count is process-wide, so an embedder's own live data counts
//! toward the cap, exactly as it would toward a JVM heap ceiling; size the cap
//! accordingly (or compute it from [`memory_meter::live_bytes`] at scope entry).
//!
//! # What this does NOT bound
//!
//! Wall-clock time. A query can run for hours within both caps; the external watchdog
//! `docs/EMBEDDING-RESOURCE-SAFETY.md` prescribes is still required for that, and for the
//! in-process-cannot-be-interrupted case that document explains.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::determinize::Strategy;

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

/// A [`ResourceBudget`] with `max_bytes` set was entered while no [`memory_meter`] is
/// installed, so the memory cap could not be enforced. Refused rather than silently
/// skipped — see the module docs.
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
/// three-line wrapper around it. All three functions are `#[inline]` relaxed atomics —
/// one uncontended atomic add per allocation.
pub mod memory_meter {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
    static INSTALLED: AtomicBool = AtomicBool::new(false);

    /// Report an allocation of `bytes`. Also marks the meter installed (idempotent; a
    /// relaxed load on the fast path, a store only the first time).
    #[inline]
    pub fn allocated(bytes: usize) {
        LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed);
        if !INSTALLED.load(Ordering::Relaxed) {
            INSTALLED.store(true, Ordering::Relaxed);
        }
    }

    /// Report a deallocation of `bytes`.
    #[inline]
    pub fn freed(bytes: usize) {
        LIVE_BYTES.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Whether a tracking allocator has reported at least one allocation — i.e. whether
    /// [`live_bytes`] means anything. Any program has allocated long before it can call
    /// this, so it is reliable from `main` onward.
    pub fn is_installed() -> bool {
        INSTALLED.load(Ordering::Relaxed)
    }

    /// Live heap bytes right now, or `None` when no meter is installed.
    pub fn live_bytes() -> Option<usize> {
        if is_installed() {
            Some(LIVE_BYTES.load(Ordering::Relaxed))
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
    /// A subset construction finished with `states` metastates after `levels` levels.
    /// This is the **pre-minimization** size; compare it with the following
    /// [`Event::MinimizeFinished`]'s `after` to diagnose a transient explosion.
    SubsetConstructionFinished { states: usize, levels: usize },
    /// A cross product started between automata of `left_states` and `right_states`.
    CrossProductStarted {
        left_states: usize,
        right_states: usize,
    },
    /// A cross product finished with `states` pairs discovered.
    CrossProductFinished { states: usize },
    /// A minimization started on `states` states.
    MinimizeStarted { states: usize },
    /// A minimization finished: `before` states in, `after` out.
    MinimizeFinished { before: usize, after: usize },
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

    /// The largest live-heap reading taken at any check point, or `0` when no memory
    /// meter is installed. A check-point sample, not an allocator-level high-water mark.
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
    /// A `MinimizeFinished` is attributed to the most recent finished subset
    /// construction whose output size equals its `before` (Brzozowski's intermediate
    /// minimize pairs with its first step this way, and `determinize_and_minimize`'s
    /// with its only step). A minimization that follows no matching construction —
    /// e.g. of an already-deterministic automaton — is simply not a record here.
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
                Event::SubsetConstructionFinished { states, levels } => {
                    if let Some((input_states, _, _)) = open.take() {
                        out.push(DeterminizationRecord {
                            input_states,
                            levels: *levels,
                            peak_states: *states,
                            minimized: None,
                        });
                    }
                }
                Event::MinimizeFinished { before, after } => {
                    if let Some(last) = out.last_mut() {
                        if last.minimized.is_none() && last.peak_states == *before {
                            last.minimized = Some(*after);
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
            | Event::CrossProductFinished { states }
            | Event::MinimizeStarted { states } => *states,
            Event::Determinize { input_states, .. }
            | Event::SubsetConstructionStarted { input_states, .. } => *input_states,
            Event::CrossProductStarted {
                left_states,
                right_states,
            } => (*left_states).max(*right_states),
            Event::MinimizeFinished { before, after } => (*before).max(*after),
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
}

impl fmt::Debug for Instrumentation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Instrumentation")
            .field("budget", &self.budget)
            .field("observers", &self.observers.len())
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

    pub fn budget(&self) -> ResourceBudget {
        self.budget
    }

    pub fn observer_count(&self) -> usize {
        self.observers.len()
    }

    /// Whether entering this would do anything at all.
    pub fn is_inert(&self) -> bool {
        self.budget.is_unlimited() && self.observers.is_empty()
    }

    /// [`MemoryMeterMissing`] if the budget has a memory cap and no meter is installed.
    pub fn validate(&self) -> Result<(), MemoryMeterMissing> {
        if self.budget.max_bytes.is_some() && !memory_meter::is_installed() {
            return Err(MemoryMeterMissing);
        }
        Ok(())
    }

    /// Install this instrumentation on the current thread until the returned guard is
    /// dropped. Nested scopes replace the outer one for their duration and restore it
    /// afterwards.
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
}

impl Meter {
    /// Snapshot the current thread's active instrumentation.
    pub fn current() -> Meter {
        ACTIVE.with(|a| match &*a.borrow() {
            None => Meter::inert(),
            Some(i) => Meter {
                budget: i.budget,
                observers: i.observers.clone(),
            },
        })
    }

    /// A meter that checks and reports nothing.
    pub fn inert() -> Meter {
        Meter {
            budget: ResourceBudget::UNLIMITED,
            observers: Vec::new(),
        }
    }

    /// Whether any check can fire.
    #[inline]
    pub fn has_budget(&self) -> bool {
        !self.budget.is_unlimited()
    }

    /// Enforce both caps for `operation`, whose automaton currently has `states`
    /// states. Raises the typed exhaustion panic on a breach (see the module docs).
    #[inline]
    pub fn check(&self, operation: Operation, states: usize) {
        if let Some(limit) = self.budget.max_states {
            if states > limit {
                exhaust(Exhausted {
                    reason: ExhaustedReason::States,
                    operation,
                    at: states,
                    limit,
                });
            }
        }
        if let Some(limit) = self.budget.max_bytes {
            // `enter` refused a memory cap without a meter, so `None` cannot happen
            // here; treating it as "nothing live" is the only harmless reading.
            let live = memory_meter::live_bytes().unwrap_or(0);
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
        // No tracking allocator is installed in this test binary.
        if memory_meter::is_installed() {
            return;
        }
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
            });
            m.emit(|| Event::MinimizeStarted { states: 9 });
            m.emit(|| Event::MinimizeFinished {
                before: 9,
                after: 2,
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
        });
        t.observe(&Event::MinimizeFinished {
            before: 7,
            after: 1,
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
