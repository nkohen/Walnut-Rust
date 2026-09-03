// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Subset-construction determinization (`SC`, Walnut's default strategy) and
//! Brzozowski double-reversal determinization (`BRZ`).
//!
//! Ports `Automata/FA/DeterminizationStrategies.java`'s `SC` and `Brz`/`brzStep`
//! methods — the deferred `CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`/`OTF` strategies (see
//! `docs/DESIGN.md` §9 F3) are out of scope for this crate.
//!
//! **`SC` does NOT totalize.** A metastate/symbol pair with an empty union of
//! destinations is simply omitted from the output, matching Java's `SC` exactly —
//! totalization is a separate, explicit operation ([`crate::fa::Fa::totalize`]), never
//! conflated with determinize. Callers that need a total DFA (e.g. the equivalence
//! oracle) must totalize explicitly after determinizing.
//!
//! # U0c: the strategy/export dispatcher and its per-call context
//!
//! [`subset_construction`] and [`brzozowski`] are the two *strategies*; Java reaches
//! them only through `DeterminizationStrategies.determinize(Automaton, IntSet)`
//! (`:90-131`), the dispatcher that decides which one to run and can write out an
//! intermediate automaton on the way. Both of those decisions come from Walnut's
//! `[strategy …]`/`[export …]` bracket-prefix metacommands, which the Java dispatcher
//! reads out of the process-wide singleton `Prover.mainProver.metaCommands` (`:99-107`).
//!
//! [`determinize`] is that dispatcher, with the singleton replaced by an explicit,
//! caller-supplied [`DeterminizeContext`] — `PORTING.md`'s standing ruling for Java
//! static/global mutable state, and the same call this project already made for
//! `Session`/`Logging`. **`None` means "no metacommands in effect"** and is
//! bit-for-bit the pre-U0c behavior: strategy `SC`, no export, no counter movement.
//!
//! The real `MetaCommands` parser that supplies the values landed in Phase 3b (`U21`), and
//! Phase 4 threads it down the `eval`/`def` call chain into this dispatcher
//! (`wr_cli::prover` → `wr_cli::eval_def` → `wr_logic::eval` → `wr_core::quantify`/
//! `logicalops`/`word_automaton`). In particular this
//! dispatcher does **not** yet emit Java's two `Logging` lines (`DETERMINIZING …`/
//! `DETERMINIZED …`, `:111-112` and `:129-130`) — no `wr-core` algorithm threads
//! [`crate::logging::Logging`] yet (see `product.rs`'s identical note). The format
//! string those lines need is ported and pinned regardless, as
//! [`Strategy::output_name`].
//!
//! ## The `shouldPrintDetails()` gate is the caller's job
//!
//! Java's whole metacommand block is wrapped in `if (Logging.shouldPrintDetails())`,
//! with an explicit comment saying why: the automata counter must NOT advance for the
//! "several silent automata creations for NS, Ostrowski, and other caches" (`:95-99`).
//! That gate is *not* re-implemented inside [`determinize`], because it has no
//! `Logging` to consult. Instead **the caller must pass `None` whenever
//! `should_print_details()` is false** — otherwise indices shift and `[strategy 6 …]`
//! /`[export 1 …]` select the wrong automaton. `Some(ctx)` here means exactly what
//! `shouldPrintDetails() == true` means in Java.
//!
//! Both halves of Java's flag are honoured, in two different places:
//!
//! * `printDetails` — `wr_cli::prover::Prover::eval_def_commands` passes `Some(ctx)` only
//!   when the command ended in `::`.
//! * `printEnabled` — Java flips this off with `Logging.disablePrint()` around every
//!   automaton `NumberSystem` builds for itself. This port models that structurally:
//!   `crate::numsys` is never handed a context, so nothing it builds can move the counter.
//!   That is *not* bit-for-bit Java, because Java's `disablePrint`/`enablePrint` are not
//!   save/restore and a nested pair re-enables early — see `docs/WALNUT-BUGS.md` WB-039,
//!   which is logged and awaiting an explicit replicate-vs-diverge decision.

use crate::automaton::Automaton;
use crate::fa::Fa;
use crate::minimize::MinimizeError;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Condvar, Mutex, RwLock};

/// `DeterminizationStrategies.Strategy` (`:34-81`), restricted to its two in-scope
/// members.
///
/// Java's enum has six: `SC`, `BRZ`, and the four OTF variants (`CCL`, `CCLS`,
/// `BRZ_CCL`, `BRZ_CCLS`). The OTF family is deliberately **deferred** — `docs/DESIGN.md`
/// §9 F3/§10 — so it is not represented here at all rather than carried as
/// unimplemented variants. Consequence for Phase 3b's `MetaCommands` port: since
/// `[strategy 6 CCLS]` cannot even be *named* through this type, that parser must
/// reject the four OTF aliases with a clean error of its own; there is no
/// `Strategy::from_string` here to do it (Java's `Strategy.fromString`, `:52-63`, with
/// its underscore/dash-insensitive alias matching, belongs to that unit).
///
/// Java's two other enum members are likewise absent by design: `doSimulation` is
/// OTF-only, and `removeBrzozowski()` (`:73-80`) is folded into [`brzozowski`], which
/// hard-codes the one mapping that survives the OTF cut (`BRZ -> SC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// Subset construction — Java's default, and this port's ([`subset_construction`]).
    #[default]
    Sc,
    /// Brzozowski double reversal ([`brzozowski`]).
    Brz,
}

impl Strategy {
    /// The enum's `name` field (`:35-36`) — the spelling Walnut prints, which is NOT
    /// the variant name for `BRZ` (`"Brzozowski"`, not `"BRZ"`).
    pub fn name(self) -> &'static str {
        match self {
            Strategy::Sc => "SC",
            Strategy::Brz => "Brzozowski",
        }
    }

    /// `Strategy.outputName(int currentIdx)` (`:69-71`) — the `[#3, strategy: SC]`
    /// fragment of Java's `Determinizing …` log line, emitted by [`determinize`]'s
    /// `Some(ctx)` arm below; it is ported and pinned here because the `details*`
    /// golden fixtures compare that line verbatim.
    pub fn output_name(self, automaton_index: usize) -> String {
        format!("[#{}, strategy: {}]", automaton_index, self.name())
    }
}

/// What [`DeterminizeContext::export_pre_determinization`] is handed: everything Java's
/// export block (`:103-109`) passes to `ProverHelper.exportAutomata`, minus the two
/// pieces that live in the context's own state.
///
/// Java:
/// ```text
/// String exportName = mc.getExportName(automataIdx);
/// if (exportName != null) {
///   String exportFormat = mc.getExportFormat(automataIdx);
///   ProverHelper.exportAutomata(Prover.currentEvalName,
///       exportName + "_" + automataIdx + "_pre", exportFormat, A, fa.isFAO());
/// }
/// ```
///
/// The file name and format are derived entirely from `MetaCommands`/`Prover` state, so
/// they are the *sink's* to compute — an implementor of [`DeterminizeContext`] is the
/// port of `MetaCommands` and already holds them. What only the dispatcher can supply
/// is what this struct carries: which automaton index this determinization is,
/// the automaton itself, and its `isFAO` flag.
///
/// **This is the PRE-determinization automaton** (hence Java's `_pre` filename suffix):
/// the hook fires before any strategy runs, so `automaton` is exactly the input.
pub struct ExportRequest<'a> {
    /// Java's `automataIdx` — the value returned by
    /// [`DeterminizeContext::next_automaton_index`] for this call, which Java splices
    /// into the export file name.
    pub automaton_index: usize,
    /// Java's `A` — the whole [`Automaton`], not just its [`Fa`], because the `.txt`/
    /// `.gv`/`.ba` writers need the track alphabets and labels too.
    pub automaton: &'a Automaton,
    /// Java's `fa.isFAO()` argument ([`Fa::is_fao`]): whether to write this out as a
    /// word automaton (DFAO) rather than a predicate automaton.
    pub is_fao: bool,
}

/// The per-call replacement for Java's `Prover.mainProver.metaCommands` singleton read
/// inside `DeterminizationStrategies.determinize` (`:99-107`).
///
/// One implementor is expected in Phase 3b: the port of `Main/MetaCommands.java`, whose
/// three methods this trait mirrors one-for-one (`incrementAutomataIndex`, `getStrategy`,
/// and the `getExportName`/`getExportFormat` pair collapsed into a single sink call).
/// The methods take `&mut self` because Java's counter is mutable state that must
/// survive across determinizations within one command — the context is threaded, not
/// rebuilt per call.
///
/// Both defaulted methods reproduce `MetaCommands`' own defaults, so a partial
/// implementation degrades exactly the way an empty `MetaCommands` does: `getStrategy`
/// falls back to `SC` (`MetaCommands.java:47`) and a missing export entry means
/// `getExportName` returned `null`, i.e. nothing is written (`:66-71`).
pub trait DeterminizeContext {
    /// `MetaCommands.incrementAutomataIndex()` (`MetaCommands.java:27-29`) — a
    /// POST-increment: the value returned is this determinization's index, and the
    /// first non-silent determinization of a command is `0`.
    ///
    /// Called exactly once per [`determinize`] call. Note in particular that a
    /// `Brz` determinization runs subset construction twice internally and still
    /// consumes ONE index — Java's `brzStep` never re-reads the metacommands.
    fn next_automaton_index(&mut self) -> usize;

    /// `MetaCommands.getStrategy(int)` (`MetaCommands.java:43-48`), including its
    /// `alwaysOnStrategy` wildcard (`[strategy * …]`) and its `SC` fallback — all of
    /// which is the implementor's business, not the dispatcher's.
    fn strategy(&mut self, automaton_index: usize) -> Strategy {
        let _ = automaton_index;
        Strategy::Sc
    }

    /// The `[export …]` hook: `getExportName`/`getExportFormat` + the
    /// `ProverHelper.exportAutomata` call they guard (`:103-109`). Doing nothing is the
    /// port of `exportName == null` (no export registered for this index).
    ///
    /// The dispatcher ignores anything the sink might want to report — Java's
    /// `exportAutomata` returns `void` and an I/O failure there throws out of the whole
    /// command rather than altering determinization.
    fn export_pre_determinization(&mut self, request: ExportRequest<'_>) {
        let _ = request;
    }
}

/// Everything [`determinize`] can fail with.
#[derive(Debug, PartialEq, Eq)]
pub enum DeterminizeError {
    /// `WalnutException("DFAOs are not supported for non-SC strategies.")` (`:115-119`).
    /// Only reachable through a context that overrides the strategy: the default `SC`
    /// path never checks. Carries the offending strategy (always [`Strategy::Brz`]
    /// while the OTF family is deferred).
    DfaoWithNonScStrategy(Strategy),
    /// Propagated from the intermediate `justMinimize()` inside [`brzozowski`]; see its
    /// docs for why neither [`MinimizeError`] variant can actually occur there.
    Minimize(MinimizeError),
}

impl From<MinimizeError> for DeterminizeError {
    fn from(e: MinimizeError) -> Self {
        DeterminizeError::Minimize(e)
    }
}

/// `DeterminizationStrategies.determinize(Automaton A, IntSet initialState)`
/// (`:90-131`) — the strategy/export dispatcher, and the only entry point Java's two
/// `Automaton.determinizeAndMinimize` overloads (`Automaton.java:394`, `:404`) use.
///
/// Determinizes `a` **in place** (Java mutates `A.getFa()` through `FA.setQ`/`setQ0`/
/// `calculateNewStateOutput`/`setDfaTransitions`; this port reassigns `a.fa`, the crate
/// convention). `initial` is the starting metastate, exactly as for
/// [`subset_construction`].
///
/// # `ctx`
///
/// `None` — no metacommands in effect — runs plain [`subset_construction`] and touches
/// nothing else, which is precisely what every pre-U0c caller did. `Some(ctx)` is the
/// port of Java's `Logging.shouldPrintDetails()` branch: take the next automaton index,
/// ask the context for this index's strategy, offer the context the pre-determinization
/// automaton to export, then determinize. See this module's docs on why the
/// print-details gate itself is the caller's responsibility.
///
/// The three context interactions happen in Java's order, which is observable: the
/// index is consumed *before* the strategy lookup (so both see the same index), and the
/// export is offered *before* the DFAO guard below — so a `[strategy] [export]` pair on
/// a DFAO still writes its `_pre` file and only then errors, matching Java.
///
/// # Errors
///
/// [`DeterminizeError::DfaoWithNonScStrategy`] ports Java's `:115-119` guard: a
/// non-`SC` strategy on a word automaton is refused rather than silently flattening the
/// DFAO's outputs to accept/reject bits (which is what [`brzozowski`] would otherwise
/// do — see its own note, and its matching `debug_assert!`). Reachable only via a
/// strategy-overriding context.
pub fn determinize(
    a: &mut Automaton,
    initial: &BTreeSet<usize>,
    ctx: Option<&mut (dyn DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) -> Result<(), DeterminizeError> {
    let time_before = std::time::Instant::now();
    let mut strategy = Strategy::Sc;
    if let Some(ctx) = ctx {
        // Java `:100-101`: the counter advances once per non-silent determinization,
        // and the strategy is looked up under that same index.
        let automaton_index = ctx.next_automaton_index();
        strategy = ctx.strategy(automaton_index);

        // Java `:103-109`: `A` is offered to the export sink BEFORE determinizing
        // (Walnut names the file `..._<idx>_pre` for exactly that reason).
        let is_fao = a.fa.is_fao();
        ctx.export_pre_determinization(ExportRequest {
            automaton_index,
            automaton: &*a,
            is_fao,
        });

        // Java `:111-112` logs `DETERMINIZING <outputName>: <Q> states`, inside the
        // `if (Logging.shouldPrintDetails())` block this whole arm already stands in
        // for (`ctx.is_some()` IS that gate, per this module's own docs).
        logging.log_message(&format!(
            "{} {}: {} states",
            crate::logging::DETERMINIZING,
            strategy.output_name(automaton_index),
            a.fa.q
        ));
    }

    // Java `:115-119`.
    if strategy != Strategy::Sc && a.fa.is_fao() {
        return Err(DeterminizeError::DfaoWithNonScStrategy(strategy));
    }

    // Java `:121-125`'s switch, minus the deferred OTF arm.
    a.fa = match strategy {
        Strategy::Sc => subset_construction(&a.fa, initial),
        Strategy::Brz => brzozowski(&a.fa, initial, logging)?,
    };
    // In Java this is a brand-new `FA` object, so its `canonized` memo is `false` by
    // construction; this port's flag lives on the `Automaton` wrapper and survives the
    // `fa` swap, so it is reset by hand. See `Automaton::canonized`'s doc comment for
    // the exhaustive list of sites that owe this.
    a.set_canonized(false);

    // Java `:127-130` logs `DETERMINIZED: <Q> states - <ms>ms` -- unlike the block
    // above, unconditionally (no `shouldPrintDetails` guard; `Logging.logMessage` does
    // its own filtering, so a no-op call here is cheap and correct either way).
    logging.log_message(&format!(
        "{}: {} states - {}ms",
        crate::logging::DETERMINIZED,
        a.fa.q,
        time_before.elapsed().as_millis()
    ));
    Ok(())
}

/// Determinizes `fa` via subset construction, starting from the metastate `initial`
/// (a *set* of NFA states, matching Java's generalized multi-initial-state entry point
/// used e.g. by Brzozowski's algorithm — for an ordinary single-initial-state NFA,
/// pass `[fa.q0].into_iter().collect()`).
///
/// Metastates are hash-consed (deduplicated) via `metastate_to_id`, and processed as a
/// worklist that grows by appending newly-discovered metastates — the same
/// array-append-as-worklist shape as the Java `metastateList`, not a separate queue.
pub fn subset_construction(fa: &Fa, initial: &BTreeSet<usize>) -> Fa {
    subset_construction_scheduled(fa, initial, Schedule::Auto)
}

/// Which BFS schedule [`subset_construction_scheduled`] uses.
///
/// Production callers always get [`Schedule::Auto`]. The three forcing variants exist so
/// tests can drive the parallel path directly, on inputs far below `Auto`'s size
/// thresholds — **without them the parallel path has near-zero fast-tier coverage**, since
/// neither the unit generators nor the differential-gen queries build automata anywhere
/// near [`PAR_MIN_LEVEL`], and its equivalence claim would rest entirely on the gated-slow
/// tiers.
///
/// This is a private *parameter*, deliberately: it is passed down the call chain,
/// unreachable from outside `#[cfg(test)]`, and — unlike an environment variable or a
/// mutable static — cannot race the rest of the test binary, which `cargo test` runs
/// concurrently in one process.
#[derive(Clone, Copy)]
// The forcing variants are constructed only by this crate's tests. Keeping them in the
// non-test build (rather than `#[cfg(test)]`-gating the enum) means the release build
// type-checks and optimizes the exact `match` the tests exercise.
#[cfg_attr(not(test), allow(dead_code))]
enum Schedule<'h> {
    /// Per-level: parallel when the frontier clears [`should_parallelize`], else sequential.
    Auto,
    /// `Auto`'s real thresholds, plus an observer called once per BFS level with
    /// `(went_parallel, level_len, member_count)`.
    ///
    /// This is how a test proves `Auto` genuinely took the parallel branch instead of
    /// passing vacuously on the sequential one — the failure mode a future retune of
    /// [`PAR_MIN_LEVEL`]/[`PAR_MIN_MEMBERS`] would otherwise cause silently. The observer
    /// runs on the calling thread, inside the schedule decision, so it needs no `Sync`.
    AutoObserved(&'h dyn Fn(bool, usize, usize)),
    /// Always sequential.
    Sequential,
    /// Always parallel with production chunk sizing, thresholds ignored. Slower than
    /// `Sequential` on small inputs by construction; a correctness probe, not a
    /// performance mode.
    Parallel,
    /// Always parallel with a forced chunk size and an optional per-chunk observer, so a
    /// test can produce maximum fragmentation (`chunk_size: 1`) and can delay one chunk
    /// past another to force out-of-order completion.
    Tuned {
        /// Metastates per chunk. Clamped to at least 1.
        chunk_size: usize,
        /// Called as `hook(chunk_index, chunk_count, phase)` on whichever thread runs the
        /// chunk — hence `Sync`.
        hook: Option<&'h (dyn Fn(usize, usize, ChunkPhase) + Sync)>,
    },
}

// A parallel BFS level runs on `std::thread::scope` (see [`subset_construction_scheduled`]),
// the single design this crate ships. It was chosen over a persistent process-wide worker
// pool — that alternative cost less per call but needed a raw-pointer region the compiler
// could not verify to hand a parked worker a stack-borrowing job, so the whole crate stays on
// compiler-checked code by not using it. The rejected alternative and the head-to-head
// measurement behind the choice are preserved in git history; `crate::parallel`'s module docs
// carry the summary.

impl<'h> Schedule<'h> {
    /// Whether this level goes parallel, reporting to the observer when there is one.
    fn wants_parallel(self, level: &[Vec<usize>]) -> bool {
        match self {
            Schedule::Auto => should_parallelize(level),
            Schedule::AutoObserved(observe) => {
                let decision = should_parallelize(level);
                observe(decision, level.len(), level.iter().map(Vec::len).sum());
                decision
            }
            Schedule::Sequential => false,
            Schedule::Parallel | Schedule::Tuned { .. } => true,
        }
    }

    /// Metastates per chunk for a level of `level_len`.
    ///
    /// Clamped to the level's own length, which is behavior-preserving — a chunk larger
    /// than the level already produced exactly one chunk covering all of it — and is what
    /// keeps `start + chunk` from overflowing in a backend that indexes the worklist
    /// ABSOLUTELY rather than through a level-relative slice. `Schedule::Tuned` accepts an
    /// arbitrary `usize` from a test, `usize::MAX` included, and
    /// `every_schedule_agrees_on_the_blow_up_input_at_every_chunk_size` passes exactly
    /// that; without the clamp the scoped backend overflowed on every level past the
    /// first, which is how this was found.
    fn chunk_for(self, level_len: usize) -> usize {
        let requested = match self {
            Schedule::Tuned { chunk_size, .. } => chunk_size.max(1),
            _ => crate::parallel::chunk_size(level_len, PAR_MIN_CHUNK),
        };
        requested.min(level_len.max(1))
    }

    /// The per-chunk observer, if this schedule carries one.
    fn hook(self) -> Option<&'h (dyn Fn(usize, usize, ChunkPhase) + Sync)> {
        match self {
            Schedule::Tuned { hook, .. } => hook,
            _ => None,
        }
    }
}

/// Where in a chunk's evaluation a [`Schedule::Tuned`] hook is being called.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
enum ChunkPhase {
    /// Before any metastate in the chunk is expanded.
    Before,
    /// After the last one is.
    After,
}

/// The smallest BFS frontier worth expanding in parallel.
///
/// Below this the split/join bookkeeping dominates. Deliberately generous: the engine runs
/// ~190 `subset_construction` calls per benchmark-fixture dispatch, almost all of them
/// small, and a threshold that lets those onto the parallel path makes the *whole dispatch*
/// slower even though the one big call gets faster.
const PAR_MIN_LEVEL: usize = 256;

/// …and the smallest total member count in that frontier. A frontier of 1,000 singleton
/// metastates over a 2-symbol alphabet is 2,000 destination-list appends in total — real
/// work, but not enough of it to pay for a parallel round trip.
const PAR_MIN_MEMBERS: usize = 4_096;

/// The floor on a chunk's size, so a frontier barely over [`PAR_MIN_LEVEL`] does not
/// fragment into per-metastate jobs (each of which would allocate its own scratch).
const PAR_MIN_CHUNK: usize = 32;

/// Whether this BFS frontier is big enough to be worth expanding in parallel.
fn should_parallelize(level: &[Vec<usize>]) -> bool {
    if !crate::parallel::enabled() || level.len() < PAR_MIN_LEVEL {
        return false;
    }
    // O(level.len()) and branch-free — cheap next to the expansion it gates. `.len()` is a
    // field read; nothing is dereferenced past the `Vec` headers.
    let members: usize = level.iter().map(Vec::len).sum();
    members >= PAR_MIN_MEMBERS
}

// ---------------------------------------------------------------------------
// The level-parallel subset construction — `std::thread::scope`.
// ---------------------------------------------------------------------------
//
// [`subset_construction_scheduled`] decides HOW the per-metastate expansions are SCHEDULED;
// it does not change P1(a)'s union/canonicalization logic, which was lifted verbatim into
// [`expand_metastate`]. Two schedules exist:
//
//   * sequential — one metastate at a time, expand-then-merge, exactly the pre-P5
//     interleaving;
//   * level-parallel — snapshot the current BFS frontier `[cursor, end)`, expand every
//     metastate in it concurrently on `std::thread::scope` workers (a pure read-only function
//     of `fa` and the metastate's own member list), then merge the results back **in frontier
//     order**, single-threaded.
//
// **The two schedules produce the same `Fa`, bit for bit**, which is why the structural
// snapshot tests and `the_parallel_schedule_matches_the_pre_p1a_reference_implementation`
// pass. The argument, in the three places it could break:
//
//   1. *Discovery order.* The worklist IS a queue: expanding metastate `i` appends new
//      metastates at the end, and `i` is only ever expanded after every `j < i`. Snapshotting
//      `end = metastate_list.len()` and expanding `[cursor, end)` before merging changes WHEN
//      a metastate is expanded relative to others' merges, but not the ORDER in which merges
//      happen — `merge_expansion` walks the frontier in ascending index and, within one
//      metastate, ascending symbol, the identical `metastate_to_id` probe sequence the
//      sequential loop performs. So ids are minted in the same order and `metastate_list`
//      grows identically. [`crate::parallel::Task`] guarantees the input to that walk: it
//      returns chunk results **indexed by chunk position, never by completion order** — the
//      one property this argument cannot survive without (a completion-order collector would
//      silently scramble the numbering) — pinned by
//      `a_forced_out_of_order_completion_still_produces_the_sequential_output`.
//   2. *Key computation.* `expand_metastate` reads only `fa` (shared, immutable) and one
//      metastate's member list; it writes only into scratch buffers it owns. It never
//      observes `metastate_to_id`, `metastate_list` or any id, so it cannot see the
//      difference between the two schedules.
//   3. *Frontier membership.* A metastate discovered *during* the parallel level is appended
//      by the merge, i.e. at an index `>= end`, so it is expanded in the NEXT level. The
//      sequential schedule would have expanded it later too. Neither schedule can expand a
//      metastate before it exists.
//
// Panic behavior is preserved too: on a MALFORMED `Fa` whose transition table names a state
// outside `0..fa.q`, the `fa.d[garbage]` panic is raised by whichever worker reaches it
// first, but `Task::take_results` re-raises the panic of the **lowest-indexed** chunk, and
// chunks are indexed in frontier order — so the panic the caller sees, payload included, is
// the panic the sequential schedule would have raised. `PAR_MIN_*` keeps every small
// automaton on the sequential path anyway.
//
// # What the scoped formulation costs, stated up front
//
// `thread::scope` guarantees the spawned threads are joined before the scope returns, which
// is exactly the liveness property this design needs. In exchange:
//
//   1. **The worklist has to go behind an `RwLock`.** Scoped threads may only borrow data
//      that outlives the scope, and while the scope is open the main thread holds only `&`
//      to that data — so anything both sides touch needs interior mutability. This is
//      inherent to the formulation, not a design choice: there is no way to express
//      "these borrows are temporally disjoint" across a scope boundary. Workers take ONE
//      read guard per chunk; the main thread takes ONE write guard per level (for the merge,
//      or for a whole sequential level). The two never overlap in time — the merge runs only
//      after every worker has gone idle — so the lock is uncontended, but it is not free.
//   2. **A spawn+join per over-threshold call.** Threads are spawned LAZILY, on the first
//      level that actually clears `should_parallelize`, so the ~188-of-~190 small
//      `subset_construction` calls in a benchmark dispatch never create one. An empty
//      `thread::scope` costs essentially nothing.
//
// # Why this cannot deadlock
//
// Two hazards, both handled explicitly, because prototypes of this unit hit them:
//
//   * **Unwinding out of the scope with workers parked.** `thread::scope` joins its threads
//     even while the closure is unwinding, so a panic re-raised by `Task::take_results`
//     would hang forever against workers waiting for the next generation.
//     [`ScopedShutdown`]'s `Drop` sets the shutdown flag and wakes them first, and it is
//     created before anything that can panic.
//   * **A worker missing a generation.** The main thread never bumps the generation until
//     the previous level's `outstanding` has reached zero, so no worker can be a generation
//     behind and leave a level permanently un-decremented.
//
// Lock ordering is total and shallow: nothing is ever held across the acquisition of
// anything else. `drain` takes `task.inner`, releases it, calls the chunk function (which
// takes `metastate_list.read()` and releases it), then re-takes `task.inner`.

/// The per-BFS-level parameters a scoped worker needs, plus the generation handshake.
///
/// Plain `Copy` scalars: a worker reads a snapshot under the lock, releases it, and works
/// from the copy. Nothing here borrows anything.
#[derive(Clone, Copy)]
struct ScopedLevel {
    /// Bumped once per level. A worker runs a level exactly when this differs from the
    /// generation it last saw, which is what makes a missed notification impossible.
    generation: u64,
    /// Set once, after the last level, so the workers can leave and the scope can join.
    shutdown: bool,
    /// Index of the level's first metastate in the worklist.
    cursor: usize,
    /// One past its last.
    level_end: usize,
    /// Metastates per chunk.
    chunk: usize,
    /// How many chunks the level was split into.
    chunk_count: usize,
}

/// Everything the scoped workers borrow. Declared in [`subset_construction_scheduled`]'s
/// frame, OUTSIDE `thread::scope`, because that is the only place a scoped thread may borrow
/// from.
struct ScopedShared<'fa, 'h> {
    fa: &'fa Fa,
    /// The BFS worklist. See this section's docs for why it is behind a lock.
    metastate_list: RwLock<Vec<Vec<usize>>>,
    level: Mutex<ScopedLevel>,
    /// Signalled when a new level is published, and when shutdown is requested.
    wake: Condvar,
    /// Re-armed per level rather than rebuilt, since the workers can only reach a task that
    /// outlives the scope.
    task: crate::parallel::Task<ExpandOut>,
    hook: Option<&'h (dyn Fn(usize, usize, ChunkPhase) + Sync)>,
}

impl ScopedShared<'_, '_> {
    /// Expands chunk `i` of the level described by `level`. This is the chunk function both
    /// the workers and the main thread run — one body, so they cannot diverge.
    fn expand_chunk(&self, level: ScopedLevel, i: usize) -> ExpandOut {
        if let Some(hook) = self.hook {
            hook(i, level.chunk_count, ChunkPhase::Before);
        }
        let start = level.cursor + i * level.chunk;
        // `saturating_add` as well as `chunk_for`'s clamp: this indexes the worklist
        // absolutely, so a chunk bound is `cursor`-offset and nothing here should be able
        // to wrap even if a future caller reintroduces an unclamped chunk size.
        let stop = start.saturating_add(level.chunk).min(level.level_end);
        let mut scratch = ExpandScratch::new(self.fa);
        let mut out = ExpandOut::default();
        {
            let list = self
                .metastate_list
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for current in &list[start..stop] {
                expand_metastate(self.fa, current, &mut scratch, &mut out);
            }
        }
        if let Some(hook) = self.hook {
            hook(i, level.chunk_count, ChunkPhase::After);
        }
        out
    }
}

/// Tells the scoped workers to leave, on the way out of the scope — including while
/// unwinding. Without this, a panic re-raised inside the scope hangs on the join.
struct ScopedShutdown<'a, 'fa, 'h>(&'a ScopedShared<'fa, 'h>);

impl Drop for ScopedShutdown<'_, '_, '_> {
    fn drop(&mut self) {
        {
            let mut level = self
                .0
                .level
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            level.shutdown = true;
        }
        self.0.wake.notify_all();
    }
}

/// A scoped worker: park until a level is published, drain its chunks, repeat until
/// shutdown.
fn scoped_worker(shared: &ScopedShared<'_, '_>) {
    let mut seen: u64 = 0;
    loop {
        let level = {
            let mut level = shared
                .level
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            loop {
                if level.shutdown {
                    return;
                }
                if level.generation != seen {
                    break;
                }
                level = shared
                    .wake
                    .wait(level)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            seen = level.generation;
            *level
        };
        // The panic boundary is per JOB and INSIDE the loop: a boundary around the loop would
        // let one panicking level unwind this worker out of existence. A dead scoped thread
        // still gets joined, but it would silently stop draining every LATER level of the same
        // call. `DoneGuard` is inside, so the accounting is correct whichever way the body
        // leaves.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _done = crate::parallel::DoneGuard(&shared.task);
            let chunk_fn = |i: usize| shared.expand_chunk(level, i);
            crate::parallel::drain(&shared.task, &chunk_fn, crate::parallel::Direction::Front);
        }));
    }
}

/// Runs subset construction under `schedule`, using `std::thread::scope` for any level that
/// goes parallel. See this section's docs for the bit-identity and deadlock arguments.
fn subset_construction_scheduled(fa: &Fa, initial: &BTreeSet<usize>, schedule: Schedule<'_>) -> Fa {
    let first: Vec<usize> = initial.iter().copied().collect();
    let mut metastate_to_id: HashMap<Vec<usize>, usize> = HashMap::new();
    metastate_to_id.insert(first.clone(), 0);
    let shared = ScopedShared {
        fa,
        metastate_list: RwLock::new(vec![first]),
        level: Mutex::new(ScopedLevel {
            generation: 0,
            shutdown: false,
            cursor: 0,
            level_end: 0,
            chunk: 1,
            chunk_count: 0,
        }),
        wake: Condvar::new(),
        task: crate::parallel::Task::new(0),
        hook: schedule.hook(),
    };

    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
    let mut seq_scratch = ExpandScratch::new(fa);
    let mut seq_out = ExpandOut::default();
    let mut cursor = 0;

    std::thread::scope(|scope| {
        // Created FIRST: everything below can panic, and every one of those paths has to
        // release the workers before `thread::scope` tries to join them.
        let _shutdown = ScopedShutdown(&shared);
        let mut workers = 0usize;

        loop {
            let end = shared
                .metastate_list
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len();
            if cursor >= end {
                break;
            }
            let parallel = {
                let list = shared
                    .metastate_list
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                schedule.wants_parallel(&list[cursor..end])
            };

            if parallel {
                let level_len = end - cursor;
                let chunk = schedule.chunk_for(level_len);
                let chunk_count = level_len.div_ceil(chunk);

                // Spawned lazily, on the first level that actually wants them, so a small
                // `subset_construction` call never creates a thread. `crate::parallel`'s
                // policy decides how many.
                if workers == 0 {
                    for _ in 0..crate::parallel::worker_count() {
                        // A scoped `spawn` panics rather than returning `Err` on thread
                        // exhaustion, so there is nothing to recover from
                        // here; `_shutdown` above makes that panic exit cleanly instead of
                        // hanging the join.
                        scope.spawn(|| scoped_worker(&shared));
                        workers += 1;
                    }
                }

                // Re-arm BEFORE publishing the generation, so a worker that wakes instantly
                // finds a task that is already consistent. `outstanding` counts the workers
                // only: this thread drains too, but waits on the count rather than being in
                // it.
                shared.task.rearm(chunk_count, workers);
                let published = {
                    let mut level = shared
                        .level
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    level.generation += 1;
                    level.cursor = cursor;
                    level.level_end = end;
                    level.chunk = chunk;
                    level.chunk_count = chunk_count;
                    *level
                };
                shared.wake.notify_all();

                {
                    // Blocks on drop until every worker has finished this level, so the
                    // write lock below cannot race a reader. Held across this thread's own
                    // draining so that a panic in it still waits.
                    let _wait = crate::parallel::WaitGuard(&shared.task);
                    let chunk_fn = |i: usize| shared.expand_chunk(published, i);
                    crate::parallel::drain(
                        &shared.task,
                        &chunk_fn,
                        crate::parallel::Direction::Back,
                    );
                }

                // Re-raises the lowest-indexed panic, if any -- `_shutdown` releases the
                // workers on the way out.
                let chunk_outs = shared.task.take_results(chunk_count);
                let mut list = shared
                    .metastate_list
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for out in &chunk_outs {
                    merge_expansion(out, &mut list, &mut metastate_to_id, &mut d);
                }
            } else {
                // One write guard for the whole sequential level rather than one per
                // metastate. The workers are parked throughout, so it is uncontended.
                let mut list = shared
                    .metastate_list
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for i in cursor..end {
                    let current = list[i].clone();
                    seq_out.clear();
                    expand_metastate(fa, &current, &mut seq_scratch, &mut seq_out);
                    merge_expansion(&seq_out, &mut list, &mut metastate_to_id, &mut d);
                }
            }
            cursor = end;
        }
    });

    let metastate_list = shared
        .metastate_list
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let o = metastate_list
        .iter()
        .map(|ms| i32::from(ms.iter().any(|&q| fa.is_accepting(q))))
        .collect();

    Fa::with_states(0, metastate_list.len(), fa.alphabet_size, o, d)
}

/// The per-drainer reusable buffers behind [`expand_metastate`] — P1(a)'s `buckets`/
/// `touched`/`seen`/`epoch`, unchanged, moved into a struct so each chunk can own a set.
///
/// Nothing here is shared: a chunk's scratch is created inside the chunk's own closure and
/// dropped with it, so a panic mid-chunk cannot leave a marker, bucket or epoch behind for
/// a later chunk or a later `subset_construction` call to trip over (the failure mode
/// `a_panic_cannot_corrupt_a_later_run_through_reused_scratch` pins). The sequential arm
/// reuses ONE set across the whole call, exactly as the pre-P5 loop did — and a panic there
/// unwinds out of `subset_construction` entirely, taking the scratch with it.
struct ExpandScratch {
    /// C1's bucket table: `buckets[s]` accumulates symbol `s`'s raw union for the metastate
    /// currently being expanded, and is emptied again before the next one.
    buckets: Vec<Vec<usize>>,
    /// The symbols this metastate actually touched, so the clear-down is proportional to
    /// what was filled rather than to `alphabet_size`. No duplicates: a symbol is recorded
    /// only on the fill that takes its bucket from empty to non-empty.
    touched: Vec<usize>,
    /// C2's dedup marker. `seen[dest] == epoch` means `dest` is already in the key being
    /// built for the (metastate, symbol) currently being drained. A `u64` epoch cannot wrap
    /// in practice and so needs no reset/wraparound branch: 2^64 bumps at an implausible
    /// 10^8 drains/second is ~5,800 years of continuous running.
    seen: Vec<u64>,
    epoch: u64,
}

impl ExpandScratch {
    fn new(fa: &Fa) -> ExpandScratch {
        ExpandScratch {
            buckets: vec![Vec::new(); fa.alphabet_size],
            touched: Vec::new(),
            seen: vec![0; fa.q],
            epoch: 0,
        }
    }
}

/// The output of expanding one or more metastates: their canonical destination-set keys,
/// concatenated, plus the index needed to walk back over them.
///
/// Flat rather than `Vec<(i32, Vec<usize>)>` on purpose — see the allocation note at the
/// parallel call site.
#[derive(Default)]
struct ExpandOut {
    /// Every key's members, concatenated in emission order.
    flat: Vec<usize>,
    /// `(symbol, key length)` for each key in `flat`, in emission order. Reconstructing a
    /// key means walking `spans` with a running offset into `flat`.
    spans: Vec<(i32, u32)>,
    /// How many `spans` entries belong to each expanded metastate, in expansion order. A
    /// metastate with no outgoing transitions at all contributes a `0`, so this vector's
    /// length is always the number of metastates expanded — that is what keeps the merge's
    /// `d.push(row)` in step with `metastate_list`.
    per_metastate: Vec<u32>,
}

impl ExpandOut {
    fn clear(&mut self) {
        self.flat.clear();
        self.spans.clear();
        self.per_metastate.clear();
    }
}

/// Expands ONE metastate: computes, for every symbol with a non-empty destination union,
/// the canonical (sorted, deduplicated) union, and appends it to `out`.
///
/// This is P1(a)'s loop body, verbatim apart from writing into `out.flat` instead of a
/// local `scratch` vector. It is a **pure function of `fa` and `current`** — it reads no id,
/// no worklist and no map — which is the whole reason the caller is free to run many copies
/// of it concurrently.
fn expand_metastate(fa: &Fa, current: &[usize], sc: &mut ExpandScratch, out: &mut ExpandOut) {
    // `alphabet_size == 0` is the one shape where the pre-P1(a) code never read `fa.d[q]`
    // at all (its `for sym in 0..0` body never ran), so neither may this one: on a
    // malformed `Fa` whose `d` is shorter than `initial`'s members, that code reached the
    // `o`-build and panicked there, and moving that panic earlier — into a member walk that
    // today does not happen — would be a behavior change. With `alphabet_size >= 1` both
    // shapes index `fa.d[q]` for the same first offending member, so the panic site and
    // message already coincide.
    if fa.alphabet_size > 0 {
        for &q in current {
            for (&sym, dests) in &fa.d[q] {
                // The mechanical equivalent of the pre-P1(a) `for sym in
                // 0..fa.alphabet_size as i32` probe range, which never LOOKED UP a key
                // outside it: a negative or `>= alphabet_size` key contributes nothing and
                // is silently dropped with no diagnostic, matching Java (WB-038 outcome
                // (b)). This is that load-bearing drop, not a defensive bounds check — `Fa`
                // has no invariant excluding such keys and `subset_construction` is `pub`.
                if sym < 0 || sym as usize >= fa.alphabet_size {
                    continue;
                }
                if dests.is_empty() {
                    continue;
                }
                let bucket = &mut sc.buckets[sym as usize];
                if bucket.is_empty() {
                    sc.touched.push(sym as usize);
                }
                bucket.extend(dests.iter().copied());
            }
        }
    }
    let mut spans_here: u32 = 0;
    for sym in 0..fa.alphabet_size as i32 {
        let bucket = &sc.buckets[sym as usize];
        if bucket.is_empty() {
            // SC does not totalize: no transition is recorded here at all.
            continue;
        }
        // Exactly one bump per drained (metastate, symbol) pair, so the marker never
        // carries a destination's membership across symbols.
        sc.epoch += 1;
        let start = out.flat.len();
        for &dest in bucket {
            if dest < fa.q {
                if sc.seen[dest] == sc.epoch {
                    continue;
                }
                sc.seen[dest] = sc.epoch;
            } else {
                // A destination id outside `0..fa.q` has no marker slot. Push it unmarked
                // rather than growing/bounds-checking `seen`: the sort/dedup below
                // canonicalizes such ids exactly as the pre-P1(a) code did, giving the
                // identical key, and — load-bearing — leaving the resulting `fa.d[garbage]`
                // panic at the same later BFS iteration, with the same message, that
                // `subset_construction_panics_on_a_destination_id_out_of_range_of_fa_q`
                // pins. Indexing `seen` here instead would move that panic earlier.
            }
            out.flat.push(dest);
        }
        let key = &mut out.flat[start..];
        key.sort_unstable();
        // Deduplicate in place. A provable no-op on the marker-deduped run above, except
        // for the `>= fa.q` ids it deliberately does not mark. Kept because it is what
        // canonicalizes those, and because it is cheap on an already-sorted slice.
        let mut write = 1;
        for read in 1..key.len() {
            if key[read] != key[write - 1] {
                key[write] = key[read];
                write += 1;
            }
        }
        // `write` is only meaningful for a non-empty key; `bucket.is_empty()` above
        // guarantees at least one push, so `key.len() >= 1` here.
        out.flat.truncate(start + write);
        // Adversarial review found breaking this invariant (e.g. dropping the sort or the
        // dedup) has NO clean test tripwire: a non-canonical key makes every metastate look
        // "new" to `metastate_to_id`, so `while cursor < metastate_list.len()` never
        // terminates and the test process is killed by its resource cap rather than failing
        // an assertion -- exactly what CLAUDE.md's "never hangs, always a diagnosable
        // verdict" guardrail exists to prevent. This turns that failure mode into an
        // immediate, located panic.
        debug_assert!(
            out.flat[start..].windows(2).all(|w| w[0] < w[1]),
            "subset_construction: metastate key must be sorted with no duplicates"
        );
        // ... but that invariant is blind to OVER-dedup: dropping a destination that
        // belongs in the union leaves a shorter key that is still sorted and still
        // duplicate-free, so it passes the check above and silently builds a different
        // automaton. This is the tripwire for that class (the cross-symbol suppression of
        // C2's marker being the concrete way to cause it): the canonicalized RAW bucket
        // must equal what the epoch-dedup produced.
        #[cfg(debug_assertions)]
        {
            let mut canonical_raw = bucket.clone();
            canonical_raw.sort_unstable();
            canonical_raw.dedup();
            assert!(
                canonical_raw.as_slice() == &out.flat[start..],
                "subset_construction: the epoch-deduped union for symbol {sym} differs \
                 from the canonicalized raw union ({:?} vs {canonical_raw:?}) -- the dedup \
                 marker dropped or kept the wrong destinations",
                &out.flat[start..]
            );
        }
        let len = out.flat.len() - start;
        // `ExpandOut` packs span lengths and per-metastate counts as `u32` for compactness;
        // both are bounded by `fa.q`, which is `usize` with no type-level cap. Guard the
        // narrowing in release too (same discipline as `ostrowski`'s
        // `assert_alphabet_size_fits_in_an_int`) — a >4-billion-state `Fa` is impractical
        // today, but a silent truncation here would be a wrong automaton, not a clean error.
        let len =
            u32::try_from(len).expect("subset_construction: destination-set span exceeds u32");
        out.spans.push((sym, len));
        spans_here += 1;
    }
    for &sym in &sc.touched {
        sc.buckets[sym].clear();
    }
    sc.touched.clear();
    out.per_metastate.push(spans_here);
}

/// Turns [`ExpandOut`]'s keys into ids and rows — the single-threaded half.
///
/// This is where every observable ordering decision is made: it walks `out`'s metastates in
/// expansion order and, within each, its symbols in ascending order, performing exactly the
/// `metastate_to_id` probe/insert sequence the pre-P5 loop performed inline. Running it
/// after a parallel expansion rather than interleaved with a sequential one is what makes
/// the two schedules agree bit for bit.
fn merge_expansion(
    out: &ExpandOut,
    metastate_list: &mut Vec<Vec<usize>>,
    metastate_to_id: &mut HashMap<Vec<usize>, usize>,
    d: &mut Vec<BTreeMap<i32, Vec<usize>>>,
) {
    let mut span_at = 0usize;
    let mut flat_at = 0usize;
    for &n_spans in &out.per_metastate {
        let mut row = BTreeMap::new();
        for _ in 0..n_spans {
            let (sym, len) = out.spans[span_at];
            span_at += 1;
            let key = &out.flat[flat_at..flat_at + len as usize];
            flat_at += len as usize;
            let id = if let Some(&id) = metastate_to_id.get(key) {
                id
            } else {
                let next_id = metastate_list.len();
                metastate_to_id.insert(key.to_vec(), next_id);
                metastate_list.push(key.to_vec());
                next_id
            };
            row.insert(sym, vec![id]);
        }
        d.push(row);
    }
    debug_assert_eq!(
        span_at,
        out.spans.len(),
        "merge_expansion: per_metastate does not account for every span"
    );
    debug_assert_eq!(
        flat_at,
        out.flat.len(),
        "merge_expansion: spans do not account for every key element"
    );
}

// ---------------------------------------------------------------------------
// P1(a)'s frozen reference implementation.
//
// Everything from the `/// Determinizes ...` line to this function's closing brace is a
// VERBATIM copy of `subset_construction`'s pre-P1(a) body at commit `06fc85c`, with the
// single edit of the `pub fn subset_construction` signature line to `fn
// subset_construction_reference` (verify with
// `git show 06fc85c:crates/wr-core/src/determinize.rs | sed -n '302,398p'`). It exists so
// `new_matches_the_pre_p1a_reference_implementation` can compare the two implementations'
// output `Fa` field-for-field over 20,000 generated automata, and it is deliberately NOT
// kept in sync with anything: if a future change to `subset_construction` makes this
// comparison fail, the change altered observable output.
// ---------------------------------------------------------------------------
#[cfg(test)]
/// Determinizes `fa` via subset construction, starting from the metastate `initial`
/// (a *set* of NFA states, matching Java's generalized multi-initial-state entry point
/// used e.g. by Brzozowski's algorithm — for an ordinary single-initial-state NFA,
/// pass `[fa.q0].into_iter().collect()`).
///
/// Metastates are hash-consed (deduplicated) via `metastate_to_id`, and processed as a
/// worklist that grows by appending newly-discovered metastates — the same
/// array-append-as-worklist shape as the Java `metastateList`, not a separate queue.
fn subset_construction_reference(fa: &Fa, initial: &BTreeSet<usize>) -> Fa {
    // U34-P1 (`~/.claude/plans/glossy-compacting-lantern.md` §3): the per-(metastate,
    // symbol) union used to be a fresh `BTreeSet<usize>`, heap-allocated on every
    // iteration, then unconditionally `.clone()`d just to probe `metastate_to_id` via
    // the `Entry` API even when the metastate already existed (the common case
    // post-startup). Profiling found this was the single largest hot frame in the
    // engine post-U33 (`BTreeSet::insert`, 24% of real work on a representative
    // workload, `benches/STATUS.md`'s U33 section).
    //
    // This is a pure representation swap, not a behavior change: `scratch` is filled
    // with the exact same set of destination ids a `BTreeSet<usize>` union would
    // contain, then `sort_unstable()` + `dedup()` produces the identical canonical
    // sorted-deduped sequence `BTreeSet<usize>`'s own iteration would give — so it's
    // usable as an equivalent hash-map key. `metastate_to_id`/`metastate_list`/
    // `current` all move from `BTreeSet<usize>` to `Vec<usize>` (sorted, deduped) in
    // lockstep: `current` is a *clone of a `metastate_list` element*
    // (`metastate_list[cursor].clone()`), so its type necessarily follows
    // `metastate_list`'s, and a sorted-deduped `Vec` iterates in the identical
    // ascending order a `BTreeSet` does, so `for &q in &current` walks the same
    // sequence either way — `subset_construction`'s exact state-discovery order (hence
    // the output `Fa`'s exact numbering) is unchanged. `metastate_list`'s only other
    // use, the order-insensitive `.any()` below, is likewise unaffected.
    //
    // The borrowed lookup (`metastate_to_id.get(scratch.as_slice())`, via the standard
    // `Vec<T>: Borrow<[T]>` impl) runs BEFORE any clone, so the already-known-metastate
    // path — the overwhelmingly common one after the initial exploration burst — pays
    // no allocation at all beyond filling the reused `scratch` buffer. That path is
    // where nearly all the removed `BTreeSet::insert`/clone cost lived, so that's where
    // this change wins. On the far rarer genuinely-new-metastate path, `scratch` is
    // cloned twice (once for `metastate_to_id`'s stored key, once more into
    // `metastate_list`) — one MORE clone than the old code's single
    // clone-for-the-key-then-move-into-the-list, an honest cost, not a further win, on
    // that specific rare path (adversarial review caught an earlier draft of this
    // comment claiming otherwise).
    let mut metastate_list: Vec<Vec<usize>> = vec![initial.iter().copied().collect()];
    let mut metastate_to_id: HashMap<Vec<usize>, usize> = HashMap::new();
    metastate_to_id.insert(metastate_list[0].clone(), 0);

    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
    let mut scratch: Vec<usize> = Vec::new();
    let mut cursor = 0;
    while cursor < metastate_list.len() {
        let current = metastate_list[cursor].clone();
        let mut row = BTreeMap::new();
        for sym in 0..fa.alphabet_size as i32 {
            scratch.clear();
            for &q in &current {
                if let Some(dests) = fa.d[q].get(&sym) {
                    scratch.extend(dests.iter().copied());
                }
            }
            if scratch.is_empty() {
                // SC does not totalize: no transition is recorded here at all.
                continue;
            }
            scratch.sort_unstable();
            scratch.dedup();
            // Adversarial review found breaking this invariant (e.g. dropping the sort
            // or the dedup) has NO clean test tripwire: a non-canonical key makes every
            // metastate look "new" to `metastate_to_id`, so `while cursor <
            // metastate_list.len()` never terminates and the test process is killed by
            // its resource cap rather than failing an assertion -- exactly what
            // CLAUDE.md's "never hangs, always a diagnosable verdict" guardrail exists
            // to prevent. This turns that failure mode into an immediate, located panic.
            debug_assert!(
                scratch.windows(2).all(|w| w[0] < w[1]),
                "subset_construction: metastate key must be sorted with no duplicates"
            );
            let id = if let Some(&id) = metastate_to_id.get(scratch.as_slice()) {
                id
            } else {
                let next_id = metastate_list.len();
                metastate_to_id.insert(scratch.clone(), next_id);
                metastate_list.push(scratch.clone());
                next_id
            };
            row.insert(sym, vec![id]);
        }
        d.push(row);
        cursor += 1;
    }

    let o = metastate_list
        .iter()
        .map(|ms| i32::from(ms.iter().any(|&q| fa.is_accepting(q))))
        .collect();

    Fa::with_states(0, metastate_list.len(), fa.alphabet_size, o, d)
}

/// `DeterminizationStrategies.Brz`/`brzStep` (`Brz`: `DeterminizationStrategies.java:140-149`;
/// `brzStep`: `:151-163`), restricted to the `BRZ`->`SC` path (`strategy.removeBrzozowski()`
/// maps `BRZ` to `SC`; the `BRZ_CCL`/`BRZ_CCLS`->OTF paths are out of scope, see module
/// docs and `docs/BOUNDARY-MAP.md`'s confirmation that this split is clean: `brzStep`
/// never calls `OTF(...)` on the `SC`-mapped path). `initial` is the ORIGINAL
/// automaton's initial-state set (ordinarily `{fa.q0}`; Brzozowski's algorithm and
/// other callers may pass a genuine multi-state seed, same as [`subset_construction`]).
///
/// # Precondition NOT enforced here (it lives in the dispatcher, [`determinize`])
///
/// Java's `DeterminizationStrategies.determinize` refuses `BRZ` (and every non-`SC`
/// strategy) on a DFAO: `if (strategy != SC) { if (fa.isFAO()) throw ... }`
/// (`:115-118`, `isFAO` = "some state's output is `> 1`", `FA.java:65-71`). This
/// function has no such guard and will silently collapse a DFAO's real output values
/// to plain 0/1 acceptance (via [`subset_construction`]'s/[`crate::minimize::minimize`]'s
/// binary-output handling) instead of erroring. As of U0c the guard is ported, in the
/// same place Java keeps it — [`determinize`], which returns
/// [`DeterminizeError::DfaoWithNonScStrategy`]. Callers that reach this function
/// directly, bypassing the dispatcher, still owe it the same check (hence the
/// `debug_assert!` below).
///
/// # Sequence
///
/// Matching Java's `Brz` body exactly: reverse -> `SC` -> `justMinimize()` -> reverse
/// (from the MINIMIZED result's own `q0`, not the original `initial` parameter —
/// Java's comment: "Note that initial state is now q0") -> `SC`. **The final `SC` is
/// NOT followed by another minimize** — Java's `Brz` calls `fa.justMinimize()` exactly
/// once, between the two `brzStep`s, not after the second one.
///
/// # Why this still yields the minimal DFA without a final minimize
///
/// The classical theorem is: `SC(reverse(A))` is the MINIMAL DFA of `L(A)`-reversed
/// *whenever `A` is deterministic and every one of `A`'s states is reachable from its
/// initial state* — minimization of the intermediate is NOT the hypothesis (an
/// earlier version of this doc claimed the theorem holds "regardless of whether the
/// intermediate was minimized", which is true here but for a different, unstated
/// reason, and is false in general: reverse-then-determinize a deterministic but
/// UNREACHABLE-states-having automaton and the result need not be minimal). What
/// actually discharges the hypothesis on both applications here: [`subset_construction`]
/// always emits only the part reachable from its own `q0` (state `0`) by construction,
/// and [`crate::minimize::minimize`] preserves that reachability on an
/// already-reachable input (Valmari's partition refinement never reintroduces a
/// pruned-away state) — so both the first and second `SC` call always receive a
/// reachable, deterministic input, satisfying the real hypothesis. That same argument
/// is why the `minimize.rs` WB-001 bug (`q0` not co-reachable to acceptance) could
/// never fire on the intermediate even while it was live: a reachable automaton with any
/// accepting state has that state reachable from `q0`, hence `q0` is co-reachable to it.
/// (WB-001 is now fixed — `walnut-java` commit `14509f1` — so this is a note about why
/// this path was never affected, not a live constraint.) The mid-sequence minimize
/// is therefore a genuine performance optimization only (it shrinks what feeds the
/// potentially-exponential SECOND subset construction — note it's the FIRST one, on
/// the raw reversed input, that has no such shrinking and is the more surprising cost
/// center) — never a correctness requirement. Pinned by
/// `brzozowski_yields_the_minimal_dfa_cross_checked_against_direct_minimize` below
/// (DESIGN.md §5 Tier 4's named "Brzozowski double-reversal = minimal DFA,
/// cross-checked against the direct minimizer" property).
///
/// # Errors and panics
///
/// Returns `Err` if the intermediate `justMinimize()` call's preconditions somehow
/// fail — see [`crate::minimize::minimize`]'s documented `MinimizeError` variants
/// (both `NotDeterministic` and `ConflictingTransitions` are possible in principle,
/// though neither can actually occur here: [`subset_construction`]'s output is always
/// deterministic and reachable by construction, so the first `SC`'s result always
/// satisfies `minimize`'s preconditions). Like [`crate::minimize::minimize`] and
/// [`crate::trim::trim`], this crate generally guards the degenerate `fa.q == 0`
/// case — this function does NOT (matching Java, which throws
/// `IndexOutOfBoundsException` at the equivalent spot): a 0-state `fa` with a
/// non-empty `initial` panics inside [`crate::fa::Fa::reverse`].
pub fn brzozowski(
    fa: &Fa,
    initial: &BTreeSet<usize>,
    logging: &mut crate::logging::Logging,
) -> Result<Fa, MinimizeError> {
    debug_assert!(
        fa.o.iter().all(|&o| o <= 1),
        "brzozowski: caller must reject DFAOs first, matching DeterminizationStrategies.\
         determinize's dispatcher-level guard (DeterminizationStrategies.java:115-118) \
         -- this function has no way to error cleanly on one itself"
    );
    // Step 1 (`brzStep(fa, initialStates, SC, "Reverse")`): reverse, then SC. `brzStep`
    // (`DeterminizationStrategies.java:150-160`) has no indent/dedent of its own -- both
    // log calls sit at whatever level the caller is already at. `strategy.name` is
    // always `"SC"` here: Java's `Brz` passes `strategy.removeBrzozowski()`
    // (`:140-141`) to the FIRST `brzStep` and a hardcoded `Strategy.SC` to the second
    // (`:148`), and this port's `Strategy` enum already hard-codes the one mapping that
    // survives the OTF cut (`BRZ -> SC`) -- see this module's own docs.
    let time_before = std::time::Instant::now();
    let mut reversed = fa.clone();
    let new_initial = reversed.reverse(initial);
    logging.log_message("Reverse -- Determinizing with strategy:SC.");
    let determinized = subset_construction(&reversed, &new_initial);
    logging.log_message(&format!(
        "Reverse: {} states - {}ms",
        determinized.q,
        time_before.elapsed().as_millis()
    ));

    // `fa.justMinimize()`.
    let minimized = crate::minimize::minimize_with_logging(&determinized, logging)?;

    // Step 2 (`brzStep(fa, IntSet.of(fa.getQ0()), SC, "Reverse of reverse")`): seed
    // with the NEW automaton's `q0` (Java's comment: "Note that initial state is now
    // q0" — i.e. after minimizing, re-seed from the minimized result's own `q0`, not
    // the original `initial` parameter), reverse again, then SC. No minimize after
    // this one (see doc comment above).
    let time_before2 = std::time::Instant::now();
    let mut reversed_again = minimized.clone();
    let seed2: BTreeSet<usize> = [reversed_again.q0].into_iter().collect();
    let new_initial2 = reversed_again.reverse(&seed2);
    logging.log_message("Reverse of reverse -- Determinizing with strategy:SC.");
    let result = subset_construction(&reversed_again, &new_initial2);
    logging.log_message(&format!(
        "Reverse of reverse: {} states - {}ms",
        result.q,
        time_before2.elapsed().as_millis()
    ));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    // `proptest::prelude` glob-exports its own `Strategy` TRAIT, which collides with
    // this module's `Strategy` ENUM (both arrive by glob, so a bare `Strategy` is
    // ambiguous). Explicit imports beat globs: `Strategy` keeps meaning proptest's
    // trait, as the generator signatures below have always assumed, and the U0c enum
    // is spelled `DetStrategy` in this module only.
    use super::Strategy as DetStrategy;
    use proptest::strategy::Strategy;

    /// A genuinely nondeterministic 2-state NFA over {0,1}: state 0 (start,
    /// non-accepting) has TWO destinations on symbol 1 (self-loop and to state 1);
    /// state 1 (accepting) self-loops on everything. Recognizes "contains a 1".
    fn contains_one_nfa() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0, 1]); // nondeterministic choice
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![1]);
        d1.insert(1, vec![1]);
        Fa::with_states(0, 2, 2, vec![0, 1], vec![d0, d1])
    }

    /// Adversarial-review-requested regression test (both independent reviewers of
    /// U34-P1 flagged this as untested): `subset_construction`'s new `scratch: Vec`
    /// path relies on `sort_unstable()+dedup()` to canonicalize a metastate key that
    /// used to be canonicalized automatically by `BTreeSet`'s type. `Fa::d` carries no
    /// invariant that a destination list is sorted or duplicate-free (`fa.rs`'s own
    /// docs), so a hand-built `Fa` with an unsorted, duplicated destination list is a
    /// legitimate input, not a contrived one -- this pins the exact expected output
    /// structure (not just language) against one, hand-traced by the algorithm.
    #[test]
    fn subset_construction_canonicalizes_an_unsorted_duplicated_destination_list() {
        // q0 = 0 (non-accepting), 1 (accepting). Single symbol 0.
        // d[0][0] = [1, 0, 1] -- unsorted AND duplicated (1 appears twice).
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1, 0, 1]);
        let fa = Fa::with_states(0, 2, 1, vec![0, 1], vec![d0, BTreeMap::new()]);
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let dfa = subset_construction(&fa, &initial);

        // Hand-traced: metastate {0} discovers {0,1} on symbol 0 (canonicalized from
        // the unsorted/duplicated [1, 0, 1]); metastate {0,1} maps to itself on symbol
        // 0 (same canonicalization). Two states total, both with a single self-row.
        assert_eq!(dfa.q, 2);
        assert_eq!(dfa.q0, 0);
        assert_eq!(
            dfa.o,
            vec![0, 1],
            "state 0 = {{0}} non-accepting, state 1 = {{0,1}} accepting"
        );
        let mut expected_row = BTreeMap::new();
        expected_row.insert(0, vec![1]);
        assert_eq!(dfa.d[0], expected_row, "{{0}} --0--> {{0,1}}");
        assert_eq!(dfa.d[1], expected_row, "{{0,1}} --0--> {{0,1}} (self-loop)");

        // Language cross-check, independent of the structural trace above: "contains
        // symbol 0" -- unaffected by the canonicalization detail either way.
        for word in [vec![], vec![0], vec![0, 0]] {
            assert_eq!(
                fa.accepts_word(&word),
                dfa.accepts_word(&word),
                "mismatch on {word:?}"
            );
        }
    }

    // --- P1(a): new vs the frozen pre-P1(a) reference implementation ---------

    /// SplitMix64 — a deterministic, self-contained PRNG so a failure reported by
    /// [`new_matches_the_pre_p1a_reference_implementation`] reproduces exactly (the
    /// failing case's seed is printed, and re-running it re-generates the same
    /// automaton). Deliberately not `proptest`: this test wants a fixed, large,
    /// cheap-to-run case count with hand-controlled class coverage, not shrinking.
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        /// Uniform in `0..n`. `n == 0` is never passed.
        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }

        fn one_in(&mut self, n: usize) -> bool {
            self.below(n) == 0
        }
    }

    /// What one generated case looked like, so the test can prove its generator really
    /// did emit every input class the comparison is supposed to cover rather than
    /// silently degenerating into 20,000 copies of the easy shape.
    #[derive(Default)]
    struct GeneratorCoverage {
        out_of_range_key: usize,
        negative_key: usize,
        empty_dest_list: usize,
        unsorted_dest_list: usize,
        duplicated_dest_list: usize,
        empty_row: usize,
        alphabet_zero: usize,
        alphabet_one: usize,
        single_state: usize,
        multi_state_initial: usize,
        empty_initial: usize,
        unreachable_initial_member: usize,
        nonempty_output: usize,
    }

    /// Builds one random `Fa` + `initial` pair.
    ///
    /// **Destination ids are always `< q`, deliberately.** An id `>= fa.q` makes BOTH
    /// implementations panic (pinned by
    /// `refactor_structural_snapshots.rs`'s
    /// `subset_construction_panics_on_a_destination_id_out_of_range_of_fa_q`), which
    /// would abort this comparison loop rather than compare anything; that shape is
    /// covered by that snapshot test on both sides of the change instead. Symbol keys,
    /// by contrast, ARE generated out of range and negative, because those are silently
    /// dropped rather than fatal, so the two implementations must agree on them.
    fn random_case(rng: &mut Rng, cov: &mut GeneratorCoverage) -> (Fa, BTreeSet<usize>) {
        let q = 1 + rng.below(6);
        // Weighted so the two degenerate alphabet sizes the plan calls out are common,
        // not a once-in-20,000 accident.
        let alphabet_size = match rng.below(8) {
            0 => 0,
            1 | 2 => 1,
            _ => 2 + rng.below(3),
        };
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(q);
        for _ in 0..q {
            let mut row: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
            let keys = rng.below(alphabet_size + 3);
            for _ in 0..keys {
                let sym: i32 = match rng.below(10) {
                    // Out of range (>= alphabet_size, including when it is 0).
                    0 => (alphabet_size + rng.below(3)) as i32,
                    // Negative.
                    1 => -1 - rng.below(3) as i32,
                    // In range -- unless the alphabet is empty, in which case every
                    // key is out of range by construction.
                    _ if alphabet_size == 0 => rng.below(3) as i32,
                    _ => rng.below(alphabet_size) as i32,
                };
                let len = rng.below(4);
                let dests: Vec<usize> = (0..len).map(|_| rng.below(q)).collect();
                row.insert(sym, dests);
            }
            // Coverage is counted from the FINAL row: a later duplicate `sym` overwrites
            // an earlier entry via `row.insert`, so counting at generation time would
            // credit shapes that never appear in any tested `Fa` (adversarial-review
            // finding on this test's first draft).
            for (&sym, dests) in &row {
                if sym < 0 {
                    cov.negative_key += 1;
                } else if sym as usize >= alphabet_size {
                    cov.out_of_range_key += 1;
                }
                if dests.is_empty() {
                    cov.empty_dest_list += 1;
                }
                if dests.windows(2).any(|w| w[0] > w[1]) {
                    cov.unsorted_dest_list += 1;
                }
                let mut sorted = dests.clone();
                sorted.sort_unstable();
                if sorted.windows(2).any(|w| w[0] == w[1]) {
                    cov.duplicated_dest_list += 1;
                }
            }
            if row.is_empty() {
                cov.empty_row += 1;
            }
            d.push(row);
        }
        let o: Vec<i32> = (0..q).map(|_| rng.below(2) as i32).collect();
        let fa = Fa::with_states(0, q, alphabet_size, o, d);

        let mut initial: BTreeSet<usize> = BTreeSet::new();
        if !rng.one_in(20) {
            let members = 1 + rng.below(q);
            for _ in 0..members {
                initial.insert(rng.below(q));
            }
        }
        if alphabet_size == 0 {
            cov.alphabet_zero += 1;
        } else if alphabet_size == 1 {
            cov.alphabet_one += 1;
        }
        if q == 1 {
            cov.single_state += 1;
        }
        match initial.len() {
            0 => cov.empty_initial += 1,
            1 => {}
            _ => cov.multi_state_initial += 1,
        }
        // A seed member other than `q0` is a member the ordinary `{fa.q0}` seeding could
        // never reach -- the "q0-unreachable member" class.
        if initial.iter().any(|&s| s != fa.q0) {
            cov.unreachable_initial_member += 1;
        }
        (fa, initial)
    }

    /// The P1(a) contract test: the restructured [`subset_construction`] must produce a
    /// field-for-field identical `Fa` to the frozen pre-change implementation
    /// ([`subset_construction_reference`]) on every input, not merely an equivalent
    /// language. State numbering here is observable output (see
    /// `docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`), so "same language" is not the bar.
    #[test]
    fn new_matches_the_pre_p1a_reference_implementation() {
        const CASES: usize = 20_000;
        let mut cov = GeneratorCoverage::default();
        for case in 0..CASES {
            // Per-case seeding: the printed seed alone reproduces the failing input.
            let seed = 0x5C_01A0_5EED_0001_u64 ^ case as u64;
            let mut rng = Rng(seed);
            let (fa, initial) = random_case(&mut rng, &mut cov);
            let expected = subset_construction_reference(&fa, &initial);
            let actual = subset_construction(&fa, &initial);
            if actual.q > 1 || !actual.d[0].is_empty() {
                cov.nonempty_output += 1;
            }
            let context =
                format!("case {case} (seed {seed:#x}): fa = {fa:?}, initial = {initial:?}");
            assert_eq!(actual.q, expected.q, "q -- {context}");
            assert_eq!(actual.q0, expected.q0, "q0 -- {context}");
            assert_eq!(
                actual.alphabet_size, expected.alphabet_size,
                "alphabet_size -- {context}"
            );
            assert_eq!(actual.o, expected.o, "o -- {context}");
            assert_eq!(actual.d, expected.d, "d -- {context}");
            assert_eq!(
                actual.true_false, expected.true_false,
                "true_false -- {context}"
            );
        }

        // The generator's own coverage, asserted rather than assumed: a comparison over
        // 20,000 inputs that all look alike proves nothing.
        for (name, count, min) in [
            ("out-of-range symbol keys", cov.out_of_range_key, 1000),
            ("negative symbol keys", cov.negative_key, 1000),
            ("empty destination lists", cov.empty_dest_list, 1000),
            ("unsorted destination lists", cov.unsorted_dest_list, 1000),
            (
                "duplicated destination lists",
                cov.duplicated_dest_list,
                1000,
            ),
            ("empty rows", cov.empty_row, 1000),
            ("alphabet_size == 0", cov.alphabet_zero, 1000),
            ("alphabet_size == 1", cov.alphabet_one, 1000),
            ("single-state automata", cov.single_state, 1000),
            ("multi-state initial seeds", cov.multi_state_initial, 1000),
            ("empty initial seeds", cov.empty_initial, 100),
            (
                "initial members other than q0",
                cov.unreachable_initial_member,
                1000,
            ),
            // Without this one the whole comparison could be 20,000 trivial
            // one-state-no-transitions results agreeing vacuously.
            (
                "results with a real transition or >1 state",
                cov.nonempty_output,
                10_000,
            ),
        ] {
            assert!(
                count >= min,
                "generator coverage: only {count} case(s) of {name} in {CASES} \
                 (expected at least {min})"
            );
        }
    }

    // --- P5: the level-parallel schedule -------------------------------------
    //
    // Every test below drives `subset_construction_scheduled` through its private
    // `Schedule` parameter. That parameter is the load-bearing test lever for this unit:
    // the production thresholds (`PAR_MIN_LEVEL`/`PAR_MIN_MEMBERS`) keep every automaton
    // the generators above build — and every differential-gen query — on the sequential
    // path, so without forcing, the parallel arm would have essentially no fast-tier
    // coverage at all.

    /// A blow-up NFA: `q` states in a ring where symbol 0 is deterministic
    /// (`i -> i + 1 mod q`) and symbol 1 branches from every state to two others, which is
    /// what makes the reachable metastate set exponential rather than linear. Its
    /// metastates have LARGE member lists, which is what clears `PAR_MIN_MEMBERS` — a
    /// merely wide frontier of singletons would not.
    fn blow_up_nfa(q: usize) -> (Fa, BTreeSet<usize>) {
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(q);
        for i in 0..q {
            let mut row = BTreeMap::new();
            row.insert(0, vec![(i + 1) % q]);
            row.insert(1, vec![(i + 1) % q, (2 * i + 3) % q]);
            d.push(row);
        }
        let o: Vec<i32> = (0..q).map(|i| i32::from(i % 5 == 0)).collect();
        let fa = Fa::with_states(0, q, 2, o, d);
        let initial: BTreeSet<usize> = [0usize].into_iter().collect();
        (fa, initial)
    }

    /// A malformed `Fa` on which EVERY metastate of BFS level 1 panics.
    ///
    /// `fa.d` has one row but `d[0]` sends each of the 8 symbols to a distinct
    /// out-of-range state `5..13`, so level 0 expands cleanly (the out-of-range ids are
    /// canonicalized into keys, exactly as
    /// `an_unmarkable_destination_id_is_canonicalized_exactly_as_the_reference_does`
    /// describes) and level 1 is 8 metastates, each of which indexes `fa.d[>= 5]` and
    /// panics. With `chunk_size: 1` that is 8 independently-panicking chunks, which is what
    /// makes it a pool test rather than a single-panic test.
    ///
    /// Chunk `i` blames state `5 + i`, so the LOWEST-indexed chunk blames state 5 — the
    /// same state the sequential schedule would reach first. That is what
    /// `the_parallel_schedule_reports_the_panic_the_sequential_schedule_would_have_raised` asserts.
    fn every_level_one_metastate_panics() -> (Fa, BTreeSet<usize>) {
        let mut d0 = BTreeMap::new();
        for sym in 0..8i32 {
            d0.insert(sym, vec![5 + sym as usize]);
        }
        let fa = Fa::with_states(0, 1, 8, vec![0], vec![d0]);
        let initial: BTreeSet<usize> = [0usize].into_iter().collect();
        (fa, initial)
    }

    fn assert_same_automaton(actual: &Fa, expected: &Fa, context: &str) {
        assert_eq!(actual.q, expected.q, "q -- {context}");
        assert_eq!(actual.q0, expected.q0, "q0 -- {context}");
        assert_eq!(
            actual.alphabet_size, expected.alphabet_size,
            "alphabet_size -- {context}"
        );
        assert_eq!(actual.o, expected.o, "o -- {context}");
        assert_eq!(actual.d, expected.d, "d -- {context}");
        assert_eq!(
            actual.true_false, expected.true_false,
            "true_false -- {context}"
        );
    }

    /// **P5's primary unit gate.** The level-parallel schedule must produce a
    /// field-for-field identical `Fa` to the frozen pre-P1(a) implementation on exactly the
    /// same 20,000 generated inputs the sequential schedule is checked against in
    /// [`new_matches_the_pre_p1a_reference_implementation`] — not merely an equivalent
    /// language. State numbering is observable output (see
    /// `docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`), so "same language" is not the bar.
    ///
    /// It runs under [`Schedule::Parallel`], which ignores `should_parallelize`'s size
    /// thresholds, because the generator's automata (`q <= 6`) are three orders of
    /// magnitude too small to trip them. Without that forcing this test would silently
    /// re-run the sequential path and assert nothing new.
    ///
    /// What it does NOT prove on its own: at these sizes, production chunk sizing puts the
    /// whole level in ONE chunk, which the calling thread runs inline — so this test
    /// exercises the chunk/collect/ordered-merge restructuring, not the worker threads.
    /// [`maximum_fragmentation_matches_the_pre_p1a_reference_implementation`] is the
    /// variant that forces real cross-thread submission on the same corpus.
    #[test]
    fn the_parallel_schedule_matches_the_pre_p1a_reference_implementation() {
        const CASES: usize = 20_000;
        let mut cov = GeneratorCoverage::default();
        for case in 0..CASES {
            let seed = 0x5C_01A0_5EED_0001_u64 ^ case as u64;
            let mut rng = Rng(seed);
            let (fa, initial) = random_case(&mut rng, &mut cov);
            let expected = subset_construction_reference(&fa, &initial);
            let actual = subset_construction_scheduled(&fa, &initial, Schedule::Parallel);
            let context =
                format!("case {case} (seed {seed:#x}): fa = {fa:?}, initial = {initial:?}");
            assert_same_automaton(&actual, &expected, &context);
        }
        // The generator's coverage assertions are not repeated here; they are properties of
        // `random_case`, already asserted over the identical seed sequence above.
    }

    /// **Mandated concurrency test (d): maximum fragmentation.** The same comparison as
    /// above with the chunk size forced to 1, so a level of `n` metastates becomes `n`
    /// separate chunks and the scoped workers really do run chunks on inputs this small.
    /// Fewer cases than the 20,000 above only because each one now pays real
    /// spawn/wakeup cost; the input classes are identical (same generator, same seed
    /// sequence prefix).
    ///
    /// This is the test that would catch a schedule that dropped, duplicated or misordered a
    /// chunk's result — at chunk size 1, "chunk index" and "metastate index within the
    /// level" are the same thing, so any scrambling shows up directly as a different `d`.
    #[test]
    fn maximum_fragmentation_matches_the_pre_p1a_reference_implementation() {
        const CASES: usize = 3_000;
        let mut cov = GeneratorCoverage::default();
        for case in 0..CASES {
            let seed = 0x5C_01A0_5EED_0001_u64 ^ case as u64;
            let mut rng = Rng(seed);
            let (fa, initial) = random_case(&mut rng, &mut cov);
            let expected = subset_construction_reference(&fa, &initial);
            let actual = subset_construction_scheduled(
                &fa,
                &initial,
                Schedule::Tuned {
                    chunk_size: 1,
                    hook: None,
                },
            );
            let context =
                format!("case {case} (seed {seed:#x}): fa = {fa:?}, initial = {initial:?}");
            assert_same_automaton(&actual, &expected, &context);
        }
    }

    /// The complement of the forced tests: an input big enough that `Schedule::Auto` picks
    /// the parallel path **on its own production thresholds**, checked against
    /// `Schedule::Sequential` on the same input.
    ///
    /// The anti-vacuity assertion is direct rather than inferred: `AutoObserved` reports
    /// what every level decided, so a future retune of `PAR_MIN_*` that quietly puts this
    /// case back on the sequential path turns the test red instead of hollowing it out.
    /// It is skipped, loudly, only when the process has no pool at all (a one-core machine
    /// or `WR_CORE_THREADS=1`), where "`Auto` stays sequential" is the correct answer.
    #[test]
    fn auto_selects_the_parallel_schedule_on_a_large_input_and_agrees_with_sequential() {
        let (fa, initial) = blow_up_nfa(17);
        let sequential = subset_construction_scheduled(&fa, &initial, Schedule::Sequential);

        let decisions = std::cell::RefCell::new(Vec::new());
        let observe = |parallel: bool, level: usize, members: usize| {
            decisions.borrow_mut().push((parallel, level, members));
        };
        let auto = subset_construction_scheduled(&fa, &initial, Schedule::AutoObserved(&observe));

        assert_same_automaton(&auto, &sequential, "Auto vs Sequential on the blow-up NFA");

        let decisions = decisions.into_inner();
        let parallel_levels = decisions.iter().filter(|(p, _, _)| *p).count();
        if crate::parallel::enabled() {
            assert!(
                parallel_levels > 0,
                "Auto never took the parallel branch -- the comparison above proved \
                 nothing about the parallel path. Level (len, members) histogram: {:?}",
                decisions
                    .iter()
                    .map(|(_, l, m)| (*l, *m))
                    .collect::<Vec<_>>()
            );
        } else {
            assert_eq!(
                parallel_levels, 0,
                "the pool is disabled, so no level may go parallel"
            );
        }
    }

    /// `should_parallelize` is the whole `Auto` policy, so its two independent conditions
    /// get a direct test rather than only being covered through the schedule comparison.
    #[test]
    fn should_parallelize_requires_both_a_wide_level_and_real_work_in_it() {
        // Wide enough, but every metastate is a singleton: `PAR_MIN_LEVEL` passes,
        // `PAR_MIN_MEMBERS` does not.
        let thin: Vec<Vec<usize>> = (0..PAR_MIN_LEVEL + 10).map(|i| vec![i]).collect();
        assert!(!should_parallelize(&thin));

        // Heavy, but only a handful of metastates: the other way round.
        let narrow: Vec<Vec<usize>> = (0..4).map(|_| (0..4_000).collect()).collect();
        assert!(!should_parallelize(&narrow));

        // Both conditions met. Only asserted true when the process actually has a pool to
        // submit to -- with `WR_CORE_THREADS=1` or on a single-core machine `enabled()` is
        // false and the sequential path is the only correct answer.
        let big: Vec<Vec<usize>> = (0..PAR_MIN_LEVEL + 10).map(|_| (0..64).collect()).collect();
        assert_eq!(should_parallelize(&big), crate::parallel::enabled());
    }

    /// **Mandated concurrency test (c): forced out-of-order completion.**
    ///
    /// A collector that delivered results in COMPLETION order rather than by chunk position
    /// would still pass every test above whenever chunks happen to finish in order — which,
    /// on a warm machine with evenly-sized chunks, is most of the time. This test removes
    /// the coincidence: chunk 0 of the first multi-chunk level sleeps long enough that at
    /// least one later chunk provably finishes first, and the output must still be
    /// bit-identical to the sequential schedule.
    ///
    /// The hook records the completion order of every multi-chunk level, and the test
    /// asserts the inversion actually happened (when there is a worker to cause it), so it
    /// cannot degrade into a slow way of running the ordinary parallel path.
    #[test]
    fn a_forced_out_of_order_completion_still_produces_the_sequential_output() {
        let (fa, initial) = blow_up_nfa(13);
        let expected = subset_construction_scheduled(&fa, &initial, Schedule::Sequential);

        // Only the FIRST level with more than one chunk delays, so the test costs one
        // sleep rather than one per level.
        let delayed = std::sync::atomic::AtomicBool::new(false);
        let completion_order = std::sync::Mutex::new(Vec::<usize>::new());
        let hook = |i: usize, chunk_count: usize, phase: ChunkPhase| {
            if chunk_count < 2 {
                return;
            }
            match phase {
                ChunkPhase::Before => {
                    if i == 0 && !delayed.swap(true, std::sync::atomic::Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(120));
                    }
                }
                ChunkPhase::After => {
                    completion_order
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(i);
                }
            }
        };

        let actual = subset_construction_scheduled(
            &fa,
            &initial,
            Schedule::Tuned {
                chunk_size: 1,
                hook: Some(&hook),
            },
        );
        assert_same_automaton(&actual, &expected, "out-of-order parallel vs sequential");

        let order = completion_order
            .into_inner()
            .unwrap_or_else(|p| p.into_inner());
        assert!(
            !order.is_empty(),
            "no multi-chunk level ran -- the delay never applied and this test is vacuous"
        );
        if crate::parallel::worker_count() > 0 {
            assert_ne!(
                order[0],
                0,
                "chunk 0 slept for 120ms yet still completed first, so completion order was \
                 never actually inverted and this test proves nothing. Order: {:?}",
                &order[..order.len().min(16)]
            );
        }
    }

    /// **Mandated concurrency test (a): repeated panics tear the scope down and rebuild it
    /// cleanly, without hanging or corrupting a later run.**
    ///
    /// The scoped design creates and joins its workers per over-threshold call, so "the
    /// worker count survives N panics" (the property a persistent pool would assert) is not a
    /// meaningful claim here. The equivalent, and the failure modes that actually threaten
    /// this design, are:
    ///
    ///   * **Clean teardown.** `thread::scope` joins its workers even while the closure is
    ///     unwinding from a re-raised panic; if [`ScopedShutdown`] failed to wake the parked
    ///     workers first, the join would hang forever. Reaching the end of these 30 panicking
    ///     rounds at all proves every scope tore down cleanly rather than hanging — CLAUDE.md's
    ///     "never hangs" guardrail, exercised.
    ///   * **The panic boundary is per JOB, inside the worker loop.** Put it one level out and
    ///     one panicking level would unwind a scoped worker out of existence, silently ending
    ///     its draining of every LATER level of that same call. The hook records which threads
    ///     STARTED a panicking chunk, so the test proves the panics reached worker threads
    ///     (scoped workers are anonymous, so "a thread other than the caller ran one" is the
    ///     check) rather than all landing on the caller inline.
    ///   * **No poisoned state.** A clean run after the 30 panics must still be bit-identical
    ///     to the sequential schedule.
    #[test]
    fn repeated_panics_tear_down_and_rebuild_the_scope_cleanly() {
        const ROUNDS: usize = 30;
        let (fa, initial) = every_level_one_metastate_panics();

        let panicking_threads = std::sync::Mutex::new(std::collections::BTreeSet::new());
        let hook = |_i: usize, chunk_count: usize, phase: ChunkPhase| {
            if phase == ChunkPhase::Before && chunk_count > 1 {
                let name = std::thread::current()
                    .name()
                    .unwrap_or("<unnamed>")
                    .to_string();
                panicking_threads
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(name);
                // Give the scope time to pick up the sibling chunks before this one panics,
                // so the panics are spread over the workers rather than raced through by
                // the calling thread alone.
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        };

        for round in 0..ROUNDS {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                subset_construction_scheduled(
                    &fa,
                    &initial,
                    Schedule::Tuned {
                        chunk_size: 1,
                        hook: Some(&hook),
                    },
                )
            }));
            assert!(outcome.is_err(), "round {round} did not panic");
        }
        // Reaching here means all 30 scopes joined cleanly rather than hanging.

        // Non-vacuity: at least one panicking chunk must have STARTED on a thread other than
        // this one, otherwise the caller absorbed every panic and no worker loop was ever
        // tested. Scoped workers are anonymous, so the check is that some other thread ran one.
        let threads = panicking_threads
            .into_inner()
            .unwrap_or_else(|p| p.into_inner());
        if crate::parallel::worker_count() > 0 {
            let this = std::thread::current()
                .name()
                .unwrap_or("<unnamed>")
                .to_string();
            assert!(
                threads.iter().any(|t| *t != this),
                "no panicking chunk ever ran off the calling thread (threads seen: {threads:?})"
            );
        }

        // ... and the engine is still usable afterwards.
        let (fresh_fa, fresh_initial) = blow_up_nfa(11);
        let expected =
            subset_construction_scheduled(&fresh_fa, &fresh_initial, Schedule::Sequential);
        let actual = subset_construction_scheduled(
            &fresh_fa,
            &fresh_initial,
            Schedule::Tuned {
                chunk_size: 1,
                hook: None,
            },
        );
        assert_same_automaton(&actual, &expected, "after 30 panics");
    }

    /// The panic a caller sees is the panic the SEQUENTIAL schedule would have raised —
    /// payload included, which is what reaches `Prover::caught`'s user-visible text.
    ///
    /// `every_level_one_metastate_panics` makes all 8 of level 1's metastates panic, each
    /// blaming a different state, so a collector that reported whichever panic arrived first
    /// would report a scheduling-dependent message. Repeated, because a race that resolves
    /// the same way every time on one run is not evidence.
    #[test]
    fn the_parallel_schedule_reports_the_panic_the_sequential_schedule_would_have_raised() {
        let (fa, initial) = every_level_one_metastate_panics();
        // What the sequential schedule blames, established by running it.
        let sequential = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            subset_construction_scheduled(&fa, &initial, Schedule::Sequential)
        }))
        .expect_err("the sequential schedule must panic too");
        let sequential_message = panic_message(&sequential);
        assert_eq!(
            sequential_message, "index out of bounds: the len is 1 but the index is 5",
            "the fixture no longer panics where this test assumes it does"
        );

        for round in 0..20 {
            let parallel = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                subset_construction_scheduled(
                    &fa,
                    &initial,
                    Schedule::Tuned {
                        chunk_size: 1,
                        hook: None,
                    },
                )
            }))
            .expect_err("the parallel schedule must panic too");
            assert_eq!(
                panic_message(&parallel),
                sequential_message,
                "round {round}: the parallel schedule blamed a different metastate"
            );
        }
    }

    /// A panic payload's text, whether it was built from a literal (`&'static str`) or a
    /// format string (`String`).
    fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = payload.downcast_ref::<&'static str>() {
            return (*s).to_string();
        }
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "<non-string panic payload>".to_string())
    }

    /// **Mandated concurrency test (b): a caught panic cannot corrupt a later run through
    /// reused scratch.**
    ///
    /// A prototype of this unit hit exactly this bug: a scratch buffer left dirty by a
    /// panicking expansion silently poisoned the next expansion's keys. Here the property
    /// holds *by construction* — a chunk's `ExpandScratch` is created inside the chunk's own
    /// closure and dropped with it, and the sequential arm's single scratch dies with the
    /// stack frame the panic unwinds — but "by construction" is exactly the kind of claim
    /// that stops being true when someone later reuses the scratch to save an allocation.
    /// This is the tripwire for that.
    ///
    /// It alternates panicking and clean runs, and every clean run is compared
    /// field-for-field against the sequential schedule, so a marker/bucket/epoch surviving a
    /// panic would show up as a wrong automaton rather than as a hang.
    #[test]
    fn a_panic_cannot_corrupt_a_later_run_through_reused_scratch() {
        let (bad_fa, bad_initial) = every_level_one_metastate_panics();
        // Deliberately the SAME alphabet size (8) as the panicking fixture, so a scratch
        // that survived a panic would be dimensionally compatible with this one -- i.e.
        // would corrupt the key silently instead of being caught by a length mismatch.
        let (good_fa, good_initial) = {
            let q = 11;
            let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(q);
            for i in 0..q {
                let mut row = BTreeMap::new();
                for sym in 0..8i32 {
                    row.insert(sym, vec![(i + 1 + sym as usize) % q, (2 * i + 3) % q]);
                }
                d.push(row);
            }
            let o: Vec<i32> = (0..q).map(|i| i32::from(i % 3 == 0)).collect();
            (
                Fa::with_states(0, q, 8, o, d),
                [0usize].into_iter().collect::<BTreeSet<usize>>(),
            )
        };
        let expected = subset_construction_scheduled(&good_fa, &good_initial, Schedule::Sequential);
        assert!(
            expected.q > 8,
            "the clean fixture is too trivial to detect corruption (q = {})",
            expected.q
        );

        let fragmented = Schedule::Tuned {
            chunk_size: 1,
            hook: None,
        };
        for round in 0..15 {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                subset_construction_scheduled(&bad_fa, &bad_initial, fragmented)
            }));
            assert!(
                outcome.is_err(),
                "round {round}: the bad fixture must panic"
            );

            let actual = subset_construction_scheduled(&good_fa, &good_initial, fragmented);
            assert_same_automaton(
                &actual,
                &expected,
                &format!("clean run after round {round}"),
            );
        }
    }

    /// The `Auto` and `Sequential` schedules are the two production arms; `Parallel`,
    /// `Tuned` and `AutoObserved` all have to agree with them on the same input, at any
    /// chunk size, or state numbering is schedule-dependent and every `.txt`/`.gv` byte
    /// this engine writes is too.
    #[test]
    fn every_schedule_agrees_on_the_blow_up_input_at_every_chunk_size() {
        let (fa, initial) = blow_up_nfa(17);
        let expected = subset_construction_scheduled(&fa, &initial, Schedule::Sequential);
        // Plain `subset_construction` (i.e. `Auto`) is the production entry point; check it
        // by the same bar rather than assuming the wrapper is transparent.
        assert_same_automaton(&subset_construction(&fa, &initial), &expected, "Auto");
        assert_same_automaton(
            &subset_construction_scheduled(&fa, &initial, Schedule::Parallel),
            &expected,
            "Parallel",
        );
        for chunk_size in [1usize, 2, 3, 7, 32, 1_000, usize::MAX] {
            let actual = subset_construction_scheduled(
                &fa,
                &initial,
                Schedule::Tuned {
                    chunk_size,
                    hook: None,
                },
            );
            assert_same_automaton(
                &actual,
                &expected,
                &format!("Tuned chunk_size = {chunk_size}"),
            );
        }
    }

    /// **The separate-process determinism gate's driver.**
    ///
    /// The worker count is resolved ONCE per process, so a `WR_CORE_THREADS` sweep inside one
    /// test binary would only ever re-test whichever value happened to initialize it first.
    /// The sweep therefore has to be run one configuration per process, and this test is what
    /// each process runs:
    ///
    /// ```text
    /// for n in 0 1 2 4 1024 ""; do
    ///   WR_CORE_THREADS=$n cargo test -p wr-core --release --lib \
    ///     the_determinization_digest_is_stable -- --nocapture | grep P5-DIGEST
    /// done
    /// ```
    ///
    /// Every line must carry the SAME digest. Within its own process it additionally
    /// repeats the whole workload 5 times and requires all five to agree, which is what
    /// catches a schedule that is merely *usually* deterministic.
    ///
    /// The digest is FNV-1a over the `Debug` rendering of each output automaton — a total
    /// structural comparison (`q`, `q0`, `alphabet_size`, `o`, `d`, `true_false`), not a
    /// semantic one, and stable across processes of the same binary by construction.
    #[test]
    fn the_determinization_digest_is_stable_across_repetitions() {
        let inputs: Vec<(Fa, BTreeSet<usize>)> =
            [11usize, 13, 17, 19].into_iter().map(blow_up_nfa).collect();

        let digest_once = || {
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for (fa, initial) in &inputs {
                let out = subset_construction(fa, initial);
                for byte in format!("{out:?}").bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            hash
        };

        let first = digest_once();
        for rep in 1..5 {
            assert_eq!(
                digest_once(),
                first,
                "repetition {rep} produced a different automaton than repetition 0"
            );
        }
        // Read by the cross-process gate above; harmless noise under a plain `cargo test`,
        // which captures it.
        println!(
            "P5-DIGEST threads={} digest={first:#018x}",
            crate::parallel::configured_threads(),
        );
    }

    /// The one `dest >= fa.q` shape the randomized comparison above deliberately cannot
    /// generate: an automaton whose `o`/`d` are longer than its declared `q`. No known
    /// production code path constructs this shape (`Fa::clear` leaves the OPPOSITE
    /// mismatch — `q` larger than the emptied vectors); it is built here via the raw
    /// struct literal, the same convention `refactor_structural_snapshots.rs`'s
    /// malformed-shape pins use. Ordinarily a destination id `>= fa.q` panics in the
    /// next BFS iteration (pinned by that suite's
    /// `subset_construction_panics_on_a_destination_id_out_of_range_of_fa_q`), which is
    /// why the generator excludes it; here `d`/`o` are big enough that the reference
    /// implementation completes normally instead.
    ///
    /// That makes this the test that distinguishes P1(a)'s marker fallback from the
    /// obvious wrong alternative. `seen` has length `fa.q == 1`, so indexing it with the
    /// destination id `5` would panic — the fallback pushes such ids WITHOUT marking
    /// instead, leaving `sort_unstable()`/`dedup()` to canonicalize them exactly as the
    /// old code did.
    ///
    /// The TWO-symbol shape is what makes this test release-mode load-bearing (both
    /// adversarial reviewers independently proved the first draft's one-symbol fixture
    /// was not): symbol 0 reaches state 5 via the duplicated raw list `[5, 5]`, symbol 1
    /// via the clean singleton `[5]`. With `dedup()` present both drains produce the
    /// canonical key `[5]`, so both symbols map to the SAME minted id and `q == 2`.
    /// With `dedup()` dropped, symbol 0's key stays `[5, 5]` — a "different" metastate —
    /// and the output silently becomes `q == 3` with `d[0] = {0: [1], 1: [2]}`: a wrong
    /// automaton caught by the plain `assert_eq!`s below in BOTH debug and release,
    /// where the `#[cfg(debug_assertions)]` cross-check (previously the only guard, per
    /// `fa.rs`'s own warning about debug-assert-only correctness guards) compiles out.
    #[test]
    fn an_unmarkable_destination_id_is_canonicalized_exactly_as_the_reference_does() {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![5, 5]);
        d0.insert(1, vec![5]);
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0, 0, 0, 0, 0, 1],
            d: vec![
                d0,
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            ],
            true_false: None,
        };
        let initial: BTreeSet<usize> = [0].into_iter().collect();

        let expected = subset_construction_reference(&fa, &initial);
        let actual = subset_construction(&fa, &initial);

        // Hand-traced: id0 = {0}; symbol 0's raw union [5, 5] and symbol 1's raw union
        // [5] BOTH canonicalize to the key [5], so both symbols reach the same minted
        // id1 = {5}, which has an empty row, and state 5's output is 1, so id1 is
        // accepting.
        assert_eq!(expected.q, 2);
        assert_eq!(expected.o, vec![0, 1]);
        let mut expected_row = BTreeMap::new();
        expected_row.insert(0, vec![1]);
        expected_row.insert(1, vec![1]);
        assert_eq!(expected.d, vec![expected_row, BTreeMap::new()]);

        assert_eq!(actual.q, expected.q, "q");
        assert_eq!(actual.q0, expected.q0, "q0");
        assert_eq!(
            actual.alphabet_size, expected.alphabet_size,
            "alphabet_size"
        );
        assert_eq!(actual.o, expected.o, "o");
        assert_eq!(actual.d, expected.d, "d");
        assert_eq!(actual.true_false, expected.true_false, "true_false");
    }

    /// The other shape the randomized comparison cannot reach: `alphabet_size == 0` on
    /// an automaton whose `d` is EMPTY while `initial` names a member. The pre-P1(a)
    /// implementation's `for sym in 0..0` body never ran, so it never read `fa.d[q]` at
    /// all and completed normally; P1(a)'s fill loop would read `fa.d[0]` — and panic —
    /// were it not skipped when the alphabet is empty.
    ///
    /// This is the tripwire for that guard (mutation-verified: removing the
    /// `fa.alphabet_size > 0` condition fails this test and nothing else in the
    /// workspace). Note the contrast with `refactor_structural_snapshots.rs`'s
    /// `subset_construction_with_zero_alphabet_size_and_an_out_of_bounds_initial_member_panics`,
    /// which pins a panic on a `d` that IS long enough and an `o` that is not — the two
    /// fixtures pin opposite halves of the same "never touch `fa.d` when the alphabet is
    /// empty" rule.
    #[test]
    fn a_zero_alphabet_automaton_with_no_transition_table_is_not_indexed_at_all() {
        let fa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 0,
            o: vec![1],
            d: vec![],
            true_false: None,
        };
        let initial: BTreeSet<usize> = [0].into_iter().collect();

        let expected = subset_construction_reference(&fa, &initial);
        let actual = subset_construction(&fa, &initial);

        assert_eq!(expected.q, 1);
        assert_eq!(expected.o, vec![1]);
        assert_eq!(expected.d, vec![BTreeMap::new()]);

        assert_eq!(actual.q, expected.q, "q");
        assert_eq!(actual.q0, expected.q0, "q0");
        assert_eq!(
            actual.alphabet_size, expected.alphabet_size,
            "alphabet_size"
        );
        assert_eq!(actual.o, expected.o, "o");
        assert_eq!(actual.d, expected.d, "d");
        assert_eq!(actual.true_false, expected.true_false, "true_false");
    }

    #[test]
    fn determinize_contains_one_preserves_language() {
        let nfa = contains_one_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let dfa = subset_construction(&nfa, &initial);
        assert!(dfa.is_deterministic());
        for word in [vec![], vec![0, 0, 0], vec![1], vec![0, 1, 0], vec![1, 1, 1]] {
            assert_eq!(
                nfa.accepts_word(&word),
                dfa.accepts_word(&word),
                "mismatch on {word:?}"
            );
        }
    }

    #[test]
    fn determinize_does_not_totalize() {
        // A 1-state NFA with no transition at all on symbol 1.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        let nfa = Fa::with_states(0, 1, 2, vec![0], vec![d0]);
        let initial: BTreeSet<usize> = [0].into_iter().collect();
        let dfa = subset_construction(&nfa, &initial);
        assert!(
            !dfa.is_deterministic_and_total(),
            "SC must not fabricate a sink transition"
        );
        assert!(!dfa.d[0].contains_key(&1));
    }

    /// Generates a random small NFA (possibly with real nondeterminism, and possibly
    /// with states unreachable from `q0`) over a FIXED alphabet size, so a
    /// correlated random word can be generated independently and still land on real
    /// symbols.
    fn arb_nfa_fixed_alphabet(q_max: usize, alphabet_size: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max).prop_flat_map(move |q| {
            let row_strategy =
                prop::collection::vec(prop::collection::vec(any::<bool>(), q), alphabet_size);
            let table_strategy = prop::collection::vec(row_strategy, q);
            let o_strategy = prop::collection::vec(0i32..=1, q);
            (table_strategy, o_strategy).prop_map(move |(table, o)| {
                let d = table
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .enumerate()
                            .filter_map(|(sym, incl)| {
                                let dests: Vec<usize> = incl
                                    .into_iter()
                                    .enumerate()
                                    .filter_map(|(dest, keep)| keep.then_some(dest))
                                    .collect();
                                if dests.is_empty() {
                                    None
                                } else {
                                    Some((sym as i32, dests))
                                }
                            })
                            .collect::<BTreeMap<i32, Vec<usize>>>()
                    })
                    .collect();
                Fa::with_states(0, q, alphabet_size, o, d)
            })
        })
    }

    proptest! {
        /// Tier-4 property #2 (DESIGN.md §5): determinize preserves language. Checked
        /// via `accepts_word` (not the equivalence oracle, since the NFA side isn't a
        /// total DFA and the oracle requires one).
        #[test]
        fn determinize_preserves_language(
            fa in arb_nfa_fixed_alphabet(4, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
            let dfa = subset_construction(&fa, &initial);
            prop_assert!(dfa.is_deterministic());
            prop_assert_eq!(fa.accepts_word(&word), dfa.accepts_word(&word));
        }
    }

    // --- Brzozowski (DeterminizationStrategies.Brz/brzStep) ---

    #[test]
    fn brzozowski_matches_the_input_nfas_language_on_contains_one() {
        let nfa = contains_one_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let brz = brzozowski(&nfa, &initial, &mut crate::logging::Logging::new()).unwrap();
        assert!(brz.is_deterministic());
        for word in [vec![], vec![0, 0, 0], vec![1], vec![0, 1, 0], vec![1, 1, 1]] {
            assert_eq!(
                nfa.accepts_word(&word),
                brz.accepts_word(&word),
                "mismatch on {word:?}"
            );
        }
    }

    #[test]
    fn brzozowski_result_is_already_minimal_on_contains_one() {
        // `contains_one_nfa`'s minimal DFA has exactly 2 states: state 0
        // (non-accepting) --1--> state 1 (accepting, self-loops on everything) --
        // state 0 is NOT a sink (it does leave, on symbol 1; an earlier version of
        // this comment wrongly called it one). A hand-derived instance of the
        // property test below, pinning a concrete expected state count.
        let nfa = contains_one_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let brz = brzozowski(&nfa, &initial, &mut crate::logging::Logging::new()).unwrap();
        assert_eq!(brz.q, 2);
    }

    #[test]
    fn brzozowski_seeds_the_second_reversal_from_the_minimized_q0_not_the_original_initial() {
        // Adversarial-review finding (mutation-tested): every OTHER test's
        // intermediate (reverse -> SC -> minimize) result happens to land back at
        // `q0 == 0`, so a mutant that seeds the second reversal from the ORIGINAL
        // `initial` parameter instead of the minimized result's own `q0` passes them
        // all unchanged. This fixture's intermediate lands at `q0 == 1`, catching it.
        // L(fa) = {"", "0"}: q0 accepting, --0--> state 1 (accepting, dead end).
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let fa = Fa::with_states(0, 2, 2, vec![1, 1], vec![d0, BTreeMap::new()]);
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let brz = brzozowski(&fa, &initial, &mut crate::logging::Logging::new()).unwrap();
        assert!(brz.accepts_word(&[]), "empty word must be accepted");
        assert!(brz.accepts_word(&[0]), "\"0\" must be accepted");
        assert!(!brz.accepts_word(&[1]), "\"1\" must be rejected");
        assert!(!brz.accepts_word(&[0, 0]), "\"00\" must be rejected");
    }

    #[test]
    fn brzozowski_handles_the_no_accepting_states_case() {
        // `Fa::reverse` returns the empty set here (no state was accepting before
        // the call) -- this is the first path in the crate that can hand
        // `subset_construction` a genuinely EMPTY `initial` set. A very plausible
        // real input (the "reject everything" automaton), introduced by this unit,
        // previously untested.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        let fa = Fa::with_states(0, 1, 2, vec![0], vec![d0]);
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let brz = brzozowski(&fa, &initial, &mut crate::logging::Logging::new()).unwrap();
        assert_eq!(brz.q, 1);
        assert!(brz.is_language_empty());
    }

    /// Picks a genuinely nondeterministic-seed-capable strategy: `q` states plus a
    /// NONEMPTY subset of `0..q` to use as a multi-state `initial` set (adversarial-
    /// review finding: every other test/proptest in this module seeds with the
    /// singleton `{fa.q0}` only, even though [`brzozowski`]'s own doc comment
    /// advertises multi-state seed support and a real caller — `wr-logic`'s
    /// `fix_leading_zeros` — passes one).
    fn arb_fa_and_nonempty_seed(
        q_max: usize,
        alphabet_size: usize,
    ) -> impl Strategy<Value = (Fa, BTreeSet<usize>)> {
        arb_nfa_fixed_alphabet(q_max, alphabet_size).prop_flat_map(|fa| {
            let q = fa.q;
            prop::collection::hash_set(0..q, 1..=q)
                .prop_map(move |seed| (fa.clone(), seed.into_iter().collect::<BTreeSet<usize>>()))
        })
    }

    proptest! {
        /// DESIGN.md §5 Tier 4's named property: "Brzozowski double-reversal =
        /// minimal DFA, cross-checked against the direct minimizer." Compares
        /// `brzozowski`'s result against BOTH the input NFA's own language (via
        /// `accepts_word`, the same anchor `determinize_preserves_language` uses
        /// above) AND `minimize(subset_construction(fa, seed))` (SC then an explicit
        /// final minimize) on two further axes: same language via the semantic-
        /// equivalence oracle (after totalizing both -- `subset_construction`/
        /// `minimize` don't preserve totality) AND same state count (proving
        /// Brzozowski's result is actually MINIMAL, not merely equivalent to
        /// something smaller). Seeded via [`arb_fa_and_nonempty_seed`], so this
        /// exercises genuine multi-state `initial` sets, not just `{fa.q0}`.
        #[test]
        fn brzozowski_yields_the_minimal_dfa_cross_checked_against_direct_minimize(
            (fa, initial) in arb_fa_and_nonempty_seed(4, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let brz = brzozowski(&fa, &initial, &mut crate::logging::Logging::new()).unwrap();
            prop_assert!(brz.is_deterministic());

            // Ground truth #1: language, against the input NFA directly (via a
            // reference NFA seeded with the same multi-state `initial`, since `fa`
            // itself only exposes single-`q0`-seeded `accepts_word`).
            let seeded_reference = subset_construction(&fa, &initial);
            prop_assert_eq!(
                seeded_reference.accepts_word(&word),
                brz.accepts_word(&word)
            );

            // Ground truth #2: exact minimality, against a direct SC-then-minimize.
            let direct_minimal = crate::minimize::minimize(&seeded_reference).unwrap();
            prop_assert_eq!(
                brz.q,
                direct_minimal.q,
                "Brzozowski's result must have exactly as many states as the \
                 directly-minimized DFA (it IS the minimal DFA, not just an \
                 equivalent one)"
            );

            let mut brz_total = brz.clone();
            brz_total.totalize(0);
            let mut direct_total = direct_minimal.clone();
            direct_total.totalize(0);
            prop_assert_eq!(
                crate::equiv::language_equivalent(&brz_total, &direct_total),
                Ok(true)
            );
        }
    }

    // --- U0c: the dispatcher and its `[strategy …]`/`[export …]` context hook ---
    //
    // (`DeterminizationStrategies.determinize`, `:90-131`, + the `MetaCommands` reads
    // at `:99-107`.)

    /// Wraps a raw [`Fa`] over `{0,1}` as a one-track `Automaton`, the type the Java
    /// dispatcher takes. The track metadata is arbitrary but non-empty on purpose: the
    /// export hook is supposed to hand the sink the WHOLE automaton (Java passes `A`,
    /// not `A.getFa()`), and that is only checkable if there is metadata to lose.
    fn as_single_track_automaton(fa: Fa) -> Automaton {
        Automaton::new(
            fa,
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    /// `contains_one_nfa` plus a third state that is unreachable from `q0`, so the
    /// determinized result (2 metastates: `{0}`, `{0,1}`) has strictly FEWER states
    /// than the input. Lets a test tell "the sink was offered the pre-determinization
    /// automaton" apart from "the sink was offered the result".
    fn contains_one_nfa_with_an_unreachable_state() -> Fa {
        let mut fa = contains_one_nfa();
        fa.q = 3;
        fa.o.push(0);
        fa.d.push(BTreeMap::new());
        fa
    }

    /// A 3-state NFA whose subset construction is NOT minimal: `0 --0--> 1`,
    /// `0 --1--> 2`, and states 1 and 2 are indistinguishable accepting sinks. So
    /// `L = Σ Σ*` ("any nonempty word"), `SC` yields 3 states, and `BRZ` yields the
    /// minimal 2. That gap is what makes a strategy override observable.
    fn sc_non_minimal_nfa() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        d0.insert(1, vec![2]);
        let mut sink = BTreeMap::new();
        sink.insert(0, vec![1]);
        sink.insert(1, vec![1]);
        let mut sink2 = BTreeMap::new();
        sink2.insert(0, vec![2]);
        sink2.insert(1, vec![2]);
        Fa::with_states(0, 3, 2, vec![0, 1, 1], vec![d0, sink, sink2])
    }

    fn assert_same_fa(actual: &Fa, expected: &Fa) {
        assert_eq!(actual.q, expected.q, "q");
        assert_eq!(actual.q0, expected.q0, "q0");
        assert_eq!(
            actual.alphabet_size, expected.alphabet_size,
            "alphabet_size"
        );
        assert_eq!(actual.o, expected.o, "o");
        assert_eq!(actual.d, expected.d, "d");
        assert_eq!(actual.true_false, expected.true_false, "true_false");
    }

    /// A stub [`DeterminizeContext`] standing in for Phase 3b's `MetaCommands`: it
    /// keeps the same post-incrementing automata counter and per-index strategy map,
    /// and records every interaction so a test can assert WHICH hook fired, WHEN, and
    /// with what.
    #[derive(Default)]
    struct RecordingContext {
        /// `MetaCommands.automataIndex` (`MetaCommands.java:11`).
        next_index: usize,
        /// `MetaCommands.strategyMap` (`:14`); absent entries fall back to `SC`, as in
        /// `getStrategy` (`:47`).
        strategies: BTreeMap<usize, DetStrategy>,
        /// Every index handed out, in order.
        indices_issued: Vec<usize>,
        /// Every `(index, answer)` strategy lookup, in order.
        strategy_queries: Vec<(usize, DetStrategy)>,
        /// Every export offer: `(index, is_fao, a CLONE of the automaton as offered)`.
        exports: Vec<(usize, bool, Automaton)>,
    }

    impl DeterminizeContext for RecordingContext {
        fn next_automaton_index(&mut self) -> usize {
            let idx = self.next_index;
            self.next_index += 1;
            self.indices_issued.push(idx);
            idx
        }

        fn strategy(&mut self, automaton_index: usize) -> DetStrategy {
            let s = self
                .strategies
                .get(&automaton_index)
                .copied()
                .unwrap_or(DetStrategy::Sc);
            self.strategy_queries.push((automaton_index, s));
            s
        }

        fn export_pre_determinization(&mut self, request: ExportRequest<'_>) {
            self.exports.push((
                request.automaton_index,
                request.is_fao,
                request.automaton.clone(),
            ));
        }
    }

    #[test]
    fn strategy_output_name_matches_javas_format() {
        // `Strategy.outputName` (`:69-71`), incl. the enum's `name` field (`:35-36`) --
        // BRZ prints as "Brzozowski", not "BRZ".
        assert_eq!(DetStrategy::Sc.output_name(0), "[#0, strategy: SC]");
        assert_eq!(
            DetStrategy::Brz.output_name(7),
            "[#7, strategy: Brzozowski]"
        );
        assert_eq!(DetStrategy::default(), DetStrategy::Sc);
    }

    #[test]
    fn no_context_is_exactly_plain_subset_construction() {
        // The load-bearing no-regression property: every pre-U0c caller passes `None`,
        // and must get the identical automaton back -- not merely an equivalent one.
        let nfa = contains_one_nfa_with_an_unreachable_state();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let expected = subset_construction(&nfa, &initial);

        let mut a = as_single_track_automaton(nfa);
        assert_eq!(
            determinize(&mut a, &initial, None, &mut crate::logging::Logging::new()),
            Ok(())
        );
        assert_same_fa(&a.fa, &expected);
        // Track metadata is untouched by determinization (Java only writes `A.getFa()`).
        assert_eq!(a.label, vec!["x".to_string()]);
    }

    #[test]
    fn the_context_hook_fires_once_and_is_offered_the_pre_determinization_automaton() {
        let nfa = contains_one_nfa_with_an_unreachable_state();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let mut a = as_single_track_automaton(nfa.clone());
        let mut ctx = RecordingContext::default();

        assert_eq!(
            determinize(
                &mut a,
                &initial,
                Some(&mut ctx),
                &mut crate::logging::Logging::new()
            ),
            Ok(())
        );

        // Java `:100-101`: exactly one index consumed, and the strategy looked up under
        // that same index.
        assert_eq!(ctx.indices_issued, vec![0]);
        assert_eq!(ctx.strategy_queries, vec![(0, DetStrategy::Sc)]);

        // Java `:103-109`: exactly one export offer, tagged with the same index and
        // with `fa.isFAO()` (false -- these are 0/1 outputs).
        assert_eq!(ctx.exports.len(), 1);
        let (idx, is_fao, offered) = &ctx.exports[0];
        assert_eq!(*idx, 0);
        assert!(!*is_fao);

        // The offer is the INPUT, not the result: same 3 states (incl. the unreachable
        // one), and still nondeterministic.
        assert_same_fa(&offered.fa, &nfa);
        assert!(!offered.fa.is_deterministic());
        // ... while what `determinize` actually produced is the 2-state DFA.
        assert!(a.fa.is_deterministic());
        assert_eq!(a.fa.q, 2);

        // The sink gets the whole `Automaton` (Java passes `A`, not `A.getFa()`) --
        // the writers it feeds need the track alphabets/labels.
        assert_eq!(offered.label, vec!["x".to_string()]);
        assert_eq!(offered.track_alphabets(), vec![vec![0, 1]]);
    }

    #[test]
    fn the_automata_index_advances_once_per_determinize_call() {
        // `MetaCommands.incrementAutomataIndex` is post-increment state that survives
        // across determinizations within one command -- which is exactly what makes
        // `[export 1 BA]` mean "the second one".
        let nfa = contains_one_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let mut ctx = RecordingContext::default();

        for _ in 0..3 {
            let mut a = as_single_track_automaton(nfa.clone());
            assert_eq!(
                determinize(
                    &mut a,
                    &initial,
                    Some(&mut ctx),
                    &mut crate::logging::Logging::new()
                ),
                Ok(())
            );
        }

        assert_eq!(ctx.indices_issued, vec![0, 1, 2]);
        assert_eq!(
            ctx.strategy_queries,
            vec![
                (0, DetStrategy::Sc),
                (1, DetStrategy::Sc),
                (2, DetStrategy::Sc)
            ]
        );
        assert_eq!(
            ctx.exports.iter().map(|e| e.0).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn a_strategy_override_actually_switches_the_algorithm() {
        let nfa = sc_non_minimal_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();

        // Baseline: no override -> `SC`, 3 states.
        let mut sc = as_single_track_automaton(nfa.clone());
        let mut sc_ctx = RecordingContext::default();
        assert_eq!(
            determinize(
                &mut sc,
                &initial,
                Some(&mut sc_ctx),
                &mut crate::logging::Logging::new()
            ),
            Ok(())
        );
        assert_eq!(sc.fa.q, 3);
        assert_same_fa(&sc.fa, &subset_construction(&nfa, &initial));

        // `[strategy 0 BRZ]` -> Brzozowski, 2 states, same language.
        let mut brz = as_single_track_automaton(nfa.clone());
        let mut brz_ctx = RecordingContext {
            strategies: [(0, DetStrategy::Brz)].into_iter().collect(),
            ..RecordingContext::default()
        };
        assert_eq!(
            determinize(
                &mut brz,
                &initial,
                Some(&mut brz_ctx),
                &mut crate::logging::Logging::new()
            ),
            Ok(())
        );
        assert_eq!(brz_ctx.strategy_queries, vec![(0, DetStrategy::Brz)]);
        assert_same_fa(
            &brz.fa,
            &brzozowski(&nfa, &initial, &mut crate::logging::Logging::new()).unwrap(),
        );
        assert_eq!(brz.fa.q, 2, "Brzozowski's result is the minimal DFA");

        for word in [
            vec![],
            vec![0],
            vec![1],
            vec![0, 1],
            vec![1, 0, 0],
            vec![1, 1, 1],
        ] {
            assert_eq!(
                sc.fa.accepts_word(&word),
                brz.fa.accepts_word(&word),
                "the two strategies must agree on {word:?}"
            );
        }
    }

    #[test]
    fn a_brzozowski_determinization_still_consumes_exactly_one_automata_index() {
        // Fidelity check on WHERE the hook lives: Java reads the metacommands once, in
        // `determinize` (`:99-107`), NOT in `brzStep` -- so a `BRZ` call, which runs
        // subset construction twice internally, advances the counter by one, not two.
        // Get this wrong and every subsequent `[strategy n …]`/`[export n …]` index
        // silently targets the wrong automaton.
        let nfa = sc_non_minimal_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let mut ctx = RecordingContext {
            strategies: [(0, DetStrategy::Brz)].into_iter().collect(),
            ..RecordingContext::default()
        };

        let mut first = as_single_track_automaton(nfa.clone());
        assert_eq!(
            determinize(
                &mut first,
                &initial,
                Some(&mut ctx),
                &mut crate::logging::Logging::new()
            ),
            Ok(())
        );
        let mut second = as_single_track_automaton(nfa);
        assert_eq!(
            determinize(
                &mut second,
                &initial,
                Some(&mut ctx),
                &mut crate::logging::Logging::new()
            ),
            Ok(())
        );

        assert_eq!(ctx.indices_issued, vec![0, 1]);
        assert_eq!(ctx.exports.len(), 2);
        // The second automaton had no strategy registered, so it fell back to `SC`
        // (`MetaCommands.getStrategy`'s `getOrDefault`) -- and is the 3-state one.
        assert_eq!(
            ctx.strategy_queries,
            vec![(0, DetStrategy::Brz), (1, DetStrategy::Sc)]
        );
        assert_eq!((first.fa.q, second.fa.q), (2, 3));
    }

    /// A DFAO ("word automaton"): some state's output exceeds 1.
    fn dfao() -> Fa {
        let mut fa = contains_one_nfa();
        fa.o = vec![0, 2];
        fa
    }

    #[test]
    fn a_non_sc_strategy_on_a_dfao_is_refused_but_only_after_the_export_offer() {
        // Java `:115-119` throws `"DFAOs are not supported for non-SC strategies."` --
        // and does so AFTER the export block (`:103-109`), so the `_pre` file is still
        // written. Order is observable; this pins it.
        let fa = dfao();
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let mut a = as_single_track_automaton(fa.clone());
        let mut ctx = RecordingContext {
            strategies: [(0, DetStrategy::Brz)].into_iter().collect(),
            ..RecordingContext::default()
        };

        assert_eq!(
            determinize(
                &mut a,
                &initial,
                Some(&mut ctx),
                &mut crate::logging::Logging::new()
            ),
            Err(DeterminizeError::DfaoWithNonScStrategy(DetStrategy::Brz))
        );
        // Nothing was determinized: Java throws before the switch, leaving `A` alone.
        assert_same_fa(&a.fa, &fa);
        // ... but the export sink was already offered it, flagged as a DFAO.
        assert_eq!(ctx.exports.len(), 1);
        assert!(
            ctx.exports[0].1,
            "isFAO must be reported to the export sink"
        );
        assert_eq!(ctx.indices_issued, vec![0]);
    }

    #[test]
    fn a_dfao_is_accepted_under_the_default_sc_strategy() {
        // The guard is strategy-gated in Java, not unconditional. `SC` then flattens
        // the word outputs to accept/reject bits, exactly as Java's
        // `FA.calculateNewStateOutput` (`FA.java:724-738`) does -- not this unit's
        // divergence to fix.
        let fa = dfao();
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let mut a = as_single_track_automaton(fa.clone());
        let mut ctx = RecordingContext::default();

        assert_eq!(
            determinize(
                &mut a,
                &initial,
                Some(&mut ctx),
                &mut crate::logging::Logging::new()
            ),
            Ok(())
        );
        assert_same_fa(&a.fa, &subset_construction(&fa, &initial));
        assert!(a.fa.o.iter().all(|&o| o <= 1));

        // And with no context at all -- the pre-U0c path -- likewise.
        let mut b = as_single_track_automaton(fa.clone());
        assert_eq!(
            determinize(&mut b, &initial, None, &mut crate::logging::Logging::new()),
            Ok(())
        );
        assert_same_fa(&b.fa, &subset_construction(&fa, &initial));
    }

    #[test]
    fn a_context_that_implements_only_the_counter_gets_metacommands_own_defaults() {
        // The two defaulted trait methods must behave like an empty `MetaCommands`:
        // `getStrategy` -> `SC` (`MetaCommands.java:47`), no export registered ->
        // nothing written (`:66-71`).
        #[derive(Default)]
        struct CounterOnly(usize);
        impl DeterminizeContext for CounterOnly {
            fn next_automaton_index(&mut self) -> usize {
                let i = self.0;
                self.0 += 1;
                i
            }
        }

        let nfa = sc_non_minimal_nfa();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let mut a = as_single_track_automaton(nfa.clone());
        let mut ctx = CounterOnly::default();

        assert_eq!(
            determinize(
                &mut a,
                &initial,
                Some(&mut ctx),
                &mut crate::logging::Logging::new()
            ),
            Ok(())
        );
        assert_eq!(ctx.0, 1, "the counter still advances");
        assert_same_fa(&a.fa, &subset_construction(&nfa, &initial));
    }

    #[test]
    fn the_hook_is_reachable_through_automatons_determinize_and_minimize_call_sites() {
        // `Automaton.determinizeAndMinimize` (`Automaton.java:394`, `:404`) are Java's
        // ONLY callers of the dispatcher; this port routes through it too, so the
        // hook is on the real call graph rather than an orphan entry point. Those two
        // methods pass `None` today (Phase 3b widens them), so what is checkable here
        // is the no-regression half: they must still produce exactly what the
        // pre-U0c `subset_construction` + `minimize` sequence produced.
        let nfa = contains_one_nfa_with_an_unreachable_state();
        let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
        let expected = crate::minimize::minimize(&subset_construction(&nfa, &initial)).unwrap();

        let mut a = as_single_track_automaton(nfa.clone());
        a.determinize_and_minimize();
        assert_same_fa(&a.fa, &expected);

        let mut b = as_single_track_automaton(nfa);
        b.determinize_and_minimize_from(&initial);
        assert_same_fa(&b.fa, &expected);
    }
}
