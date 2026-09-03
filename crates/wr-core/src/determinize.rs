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
    // U34-P1 (`~/.claude/plans/glossy-compacting-lantern.md` §3) replaced the
    // per-(metastate, symbol) `BTreeSet<usize>` union — heap-allocated per iteration and
    // cloned just to probe `metastate_to_id` — with the reusable `scratch: Vec<usize>` +
    // `sort_unstable()`/`dedup()` canonicalization + borrowed `HashMap` lookup this
    // function still uses. P1(a) (`~/.claude/plans/perf-beyond-p1a-subset-construction.md`)
    // keeps all of that and changes only HOW `scratch` is filled, because profiling put
    // 59.5-89.4% of the engine's real work inside this one function:
    //
    //   * **C1 — member-outer, row-once.** The union used to be built symbol-outer, with
    //     a `fa.d[q].get(&sym)` B-tree descent per (symbol, member): `alphabet_size ×
    //     |current|` full tree lookups per metastate. It is now built member-outer —
    //     each member's row is walked ONCE, in its native ascending-symbol order, and
    //     every destination list is appended RAW into `buckets[sym]`, a flat table
    //     indexed by symbol and reused across the whole call.
    //   * **C2 — dedup at drain.** `seen`/`epoch` suppress repeats while a bucket is
    //     drained into `scratch`, so the `sort_unstable()` below sorts the DEDUPED union
    //     rather than the raw one (measured mean union size on the profiled fixtures:
    //     75.9 raw -> 42.6 deduped on 230, 29.1 -> 18.1 on 179).
    //
    // **The output `Fa` is unchanged, bit for bit** — same metastate discovery order,
    // hence the same state numbering that every `.txt`/`.gv` byte and `::`-details count
    // depends on (`docs/IDIOMATIC-REFACTOR-DO-NOT-TOUCH.md`'s `wr_core::determinize`
    // entry). One qualifier: this holds for any `alphabet_size` within the `int`-checked
    // bound the construction sites already enforce (`Automaton::determine_alphabet_size`)
    // — on a malformed `Fa` whose `alphabet_size` exceeds `i32::MAX`, the old code's
    // `as i32` cast emptied its loop and returned transition-less rows, while the eager
    // alphabet-sized bucket allocation below aborts first. The argument, in the three
    // places it could break:
    //
    //   1. *Symbol order.* The drain below walks `0..alphabet_size` ascending, the
    //      identical sequence the old symbol-outer loop walked, and skips exactly the
    //      symbols the old code's `scratch.is_empty()` check skipped (`buckets[sym]` is
    //      non-empty iff at least one member had a non-empty destination list for `sym`
    //      — precisely the old `scratch`'s emptiness condition). So ids are minted in
    //      the same order, and a symbol with no union still gets NO row entry (SC does
    //      not totalize).
    //   2. *Member order.* For a fixed symbol, `buckets[sym]` receives one contribution
    //      per member in `current`'s ascending order (the outer loop's order), each
    //      appended raw — byte-identical to the old inner `for &q in &current` fill. C1
    //      alone therefore preserves even the pre-sort sequence; C2 then deletes
    //      duplicates from it, which the old code's `dedup()` deleted one step later.
    //      Nothing observes the sequence in between: only `is_empty()` and the
    //      post-sort-dedup key are read, and dropping duplicates cannot empty a
    //      non-empty list.
    //   3. *Cross-symbol independence.* The epoch is bumped once per drained
    //      (metastate, symbol) pair — see `epoch += 1` below — so a destination seen
    //      under symbol 0 is NOT suppressed under symbol 1. (That failure mode is the
    //      silent-wrong-automaton bug class this unit's snapshot test
    //      `subset_construction_does_not_suppress_a_destination_across_symbols` and the
    //      debug cross-check at the drain exist to catch.)
    //
    // Cost of the two per-call buffers, stated plainly: `buckets` is 24 bytes ×
    // `alphabet_size` (`alphabet_size` is int-checked at the established call sites), and
    // `seen` is 8 bytes × `fa.q`. Both are allocated once per call, not per metastate.
    // `subset_construction_reference` below is a verbatim copy of the pre-P1(a) body,
    // and `new_matches_the_pre_p1a_reference_implementation` compares the two outputs
    // field-for-field over 20,000 generated automata.
    subset_construction_with_policy(fa, initial, Pipeline::default())
}

/// The pipeline's tuning, as a value so tests can force it on (or off) on inputs far
/// smaller than the production thresholds — the only way to run the existing
/// [`subset_construction_reference`] cross-check *through* the parallel path.
#[derive(Debug, Clone, Copy)]
struct Pipeline {
    /// Helper threads. `0` is the plain sequential engine.
    workers: usize,
    /// Discovered metastates below which the pipeline never starts.
    min_states: usize,
    /// Frontier width below which it is not worth starting (or continuing).
    min_lookahead: usize,
    /// How far ahead of the cursor keys may be computed.
    window: usize,
}

impl Default for Pipeline {
    fn default() -> Pipeline {
        Pipeline {
            workers: default_workers(),
            min_states: PIPELINE_MIN_STATES,
            min_lookahead: PIPELINE_MIN_LOOKAHEAD,
            window: PIPELINE_WINDOW,
        }
    }
}

/// How many helper threads [`subset_construction`] may use for its speculative
/// key-computation pipeline. `0` disables the pipeline entirely, leaving the exact
/// pre-parallel code path.
///
/// `WR_SC_THREADS` overrides it — read once per process, both so `benches/src/bin/scbench.rs`
/// can pin a thread count and so a `0` there restores the sequential engine for A/B
/// verification.
///
/// **The cap of 4 is deliberately conservative, and it is not a tuned optimum.** The only
/// sweep available when this landed ran on a heavily contended machine (8 logical cores,
/// load average 11-42, two other agents building), where per-workload speedups over the
/// sequential engine went the *wrong* way as threads were added:
///
/// | threads | fixture 293 | 179 | 230 | 261 | 286 |
/// |---------|-------------|-----|-----|-----|-----|
/// | 2       | 1.54x | 1.60x | 1.58x | 1.32x | 1.28x |
/// | 4       | 1.10x | 1.27x | 1.19x | 1.16x | 1.07x |
/// | 7       | 0.84x | 1.04x | 1.08x | 0.98x | 0.68x |
///
/// Seven workers were *slower than sequential* on two of five workloads there. Four was
/// faster than sequential on all five, so four is the largest count the evidence actually
/// supports; two may well be better still, and on a quiet machine the ordering may reverse
/// entirely. Re-run `scbench` on an idle machine before raising this.
///
/// The cap also makes this a better citizen as a library: `ct-research` embeds `wr-core`,
/// and a determinize call that unconditionally grabs every core would oversubscribe an
/// embedder that is already running its own parallel harness.
///
/// One less than the machine's parallelism is the other bound, because the calling thread is
/// itself a full participant — it does all the minting and, whenever a key is not yet ready,
/// computes one itself.
fn default_workers() -> usize {
    use std::sync::OnceLock;
    static WORKERS: OnceLock<usize> = OnceLock::new();
    *WORKERS.get_or_init(|| {
        if let Ok(v) = std::env::var("WR_SC_THREADS") {
            return v.trim().parse().unwrap_or(0);
        }
        std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).min(4))
            .unwrap_or(0)
    })
}

/// Below this many discovered metastates the pipeline is never started: thread spin-up
/// costs tens of microseconds and the corpus is dominated by determinizations of a few
/// dozen states, which must not pay for it. The whole run below the threshold is the
/// untouched sequential loop, so small determinizations are bit-identical *and*
/// unmeasurably affected.
const PIPELINE_MIN_STATES: usize = 4_096;

/// … and the pipeline is only worth starting if this much work is still queued behind the
/// cursor when the threshold trips.
const PIPELINE_MIN_LOOKAHEAD: usize = 256;

/// How far ahead of the cursor keys may be computed. Bounds the memory the speculation
/// holds (one [`KeyRows`] per in-flight metastate) without ever bounding *throughput*: the
/// measured lookahead available on the heavy corpus workloads is 25,000-61,000 metastates,
/// two orders of magnitude more than this window needs to keep every worker busy.
const PIPELINE_WINDOW: usize = 1_024;

/// The reusable per-thread scratch of [`key_rows`] — exactly the buffers the sequential
/// loop used to hold as locals, lifted into a struct so a worker thread can own its own set.
struct ScBuffers {
    /// C1's bucket table: `buckets[s]` accumulates symbol `s`'s raw union for the
    /// metastate currently being processed, and is emptied again before the next one.
    buckets: Vec<Vec<usize>>,
    /// The symbols this metastate actually touched, so the clear-down is proportional to
    /// what was filled rather than to `alphabet_size`. No duplicates: a symbol is
    /// recorded only on the fill that takes its bucket from empty to non-empty.
    touched: Vec<usize>,
    /// C2's dedup marker. `seen[dest] == epoch` means `dest` is already in the key being
    /// drained. A `u64` epoch cannot wrap in practice and so needs no reset/wraparound
    /// branch: 2^64 bumps at an implausible 10^8 drains/second is ~5,800 years of
    /// continuous running.
    seen: Vec<u64>,
    epoch: u64,
}

impl ScBuffers {
    fn new(fa: &Fa) -> ScBuffers {
        ScBuffers {
            buckets: vec![Vec::new(); fa.alphabet_size],
            touched: Vec::new(),
            seen: vec![0; fa.q],
            epoch: 0,
        }
    }
}

/// One metastate's canonical successor keys, flattened.
///
/// `spans` is in ascending symbol order and `flat[start..end]` is that symbol's sorted,
/// deduplicated destination union — the exact byte sequence the pre-parallel loop held in
/// its reused `scratch` at the moment it probed `metastate_to_id`. Flat rather than a
/// `Vec<Vec<usize>>` so one metastate's whole result is two allocations, not one per
/// symbol: the old code probed the map straight out of `scratch` and allocated nothing at
/// all on the (overwhelmingly common) already-known-metastate path, and handing keys
/// across a thread boundary must not turn that into an allocation per transition.
#[derive(Default)]
struct KeyRows {
    flat: Vec<usize>,
    spans: Vec<(i32, u32, u32)>,
}

impl KeyRows {
    fn clear(&mut self) {
        self.flat.clear();
        self.spans.clear();
    }
}

/// The pure half of subset construction: everything one metastate contributes that depends
/// **only** on `fa` and `members`, and nothing on how many metastates have been minted so
/// far. This is what makes the pipeline below sound — see [`subset_construction_with_workers`].
///
/// The body is the pre-parallel loop's, unchanged in every order-bearing respect; the only
/// edit is that where it used to probe/mint an id it now appends the key to `out`.
///
/// # Panics
///
/// Indexes `fa.d[q]` for each member, so a member `>= fa.d.len()` panics here exactly as
/// the pre-parallel loop panicked at its own `fa.d[q]`. Callers that must not panic
/// (the worker threads, which would move that panic to a different, earlier metastate)
/// screen their input with [`members_are_in_range`] first.
fn key_rows(fa: &Fa, members: &[usize], b: &mut ScBuffers, out: &mut KeyRows) {
    out.clear();
    // `alphabet_size == 0` is the one shape where the old code never read `fa.d[q]`
    // at all (its `for sym in 0..0` body never ran), so neither may this one: on a
    // malformed `Fa` whose `d` is shorter than `initial`'s members, the old code
    // reached the `o`-build and panicked there, and moving that panic earlier —
    // into a member walk that today does not happen — would be a behavior change.
    // With `alphabet_size >= 1` both shapes index `fa.d[q]` for the same first
    // offending member, so the panic site and message already coincide.
    if fa.alphabet_size > 0 {
        for &q in members {
            for (&sym, dests) in &fa.d[q] {
                // The mechanical equivalent of the old `for sym in
                // 0..fa.alphabet_size as i32` probe range, which never LOOKED UP a
                // key outside it: a negative or `>= alphabet_size` key contributes
                // nothing and is silently dropped with no diagnostic, matching Java
                // (WB-038 outcome (b)). This is that load-bearing drop, not a
                // defensive bounds check — `Fa` has no invariant excluding such
                // keys and this function is reachable from a `pub` one.
                if sym < 0 || sym as usize >= fa.alphabet_size {
                    continue;
                }
                if dests.is_empty() {
                    continue;
                }
                let bucket = &mut b.buckets[sym as usize];
                if bucket.is_empty() {
                    b.touched.push(sym as usize);
                }
                bucket.extend(dests.iter().copied());
            }
        }
    }
    for sym in 0..fa.alphabet_size as i32 {
        let bucket = &b.buckets[sym as usize];
        if bucket.is_empty() {
            // SC does not totalize: no transition is recorded here at all.
            continue;
        }
        // Exactly one bump per drained (metastate, symbol) pair, so the marker
        // never carries a destination's membership across symbols.
        b.epoch += 1;
        let start = out.flat.len();
        for &dest in bucket {
            if dest < fa.q {
                if b.seen[dest] == b.epoch {
                    continue;
                }
                b.seen[dest] = b.epoch;
            } else {
                // A destination id outside `0..fa.q` has no marker slot. Push it
                // unmarked rather than growing/bounds-checking `seen`: the
                // `sort_unstable()`/`dedup()` below canonicalizes such ids exactly
                // as the old code did, giving the identical key, and — load-bearing
                // — leaving the resulting `fa.d[garbage]` panic at the same later
                // BFS iteration, with the same message, that
                // `subset_construction_panics_on_a_destination_id_out_of_range_of_fa_q`
                // pins. Indexing `seen` here instead would move that panic earlier.
            }
            out.flat.push(dest);
        }
        out.flat[start..].sort_unstable();
        // A provable no-op on the marker-deduped run above, except for the
        // `>= fa.q` ids it deliberately does not mark. Kept because it is what
        // canonicalizes those, and because it is cheap on an already-deduped slice.
        let mut len = start;
        for i in start..out.flat.len() {
            if len == start || out.flat[len - 1] != out.flat[i] {
                out.flat[len] = out.flat[i];
                len += 1;
            }
        }
        out.flat.truncate(len);
        // Adversarial review found breaking this invariant (e.g. dropping the sort
        // or the dedup) has NO clean test tripwire: a non-canonical key makes every
        // metastate look "new" to `metastate_to_id`, so `while cursor <
        // metastate_list.len()` never terminates and the test process is killed by
        // its resource cap rather than failing an assertion -- exactly what
        // CLAUDE.md's "never hangs, always a diagnosable verdict" guardrail exists
        // to prevent. This turns that failure mode into an immediate, located panic.
        debug_assert!(
            out.flat[start..].windows(2).all(|w| w[0] < w[1]),
            "subset_construction: metastate key must be sorted with no duplicates"
        );
        // ... but that invariant is blind to OVER-dedup: dropping a destination
        // that belongs in the union leaves a shorter key that is still sorted and
        // still duplicate-free, so it passes the check above and silently builds a
        // different automaton. This is the tripwire for that class (the cross-symbol
        // suppression of C2's marker being the concrete way to cause it): the
        // canonicalized RAW bucket must equal what the epoch-dedup produced.
        #[cfg(debug_assertions)]
        {
            let mut canonical_raw = bucket.clone();
            canonical_raw.sort_unstable();
            canonical_raw.dedup();
            assert!(
                canonical_raw == out.flat[start..],
                "subset_construction: the epoch-deduped union for symbol {sym} \
                 differs from the canonicalized raw union ({:?} vs \
                 {canonical_raw:?}) -- the dedup marker dropped or kept the wrong \
                 destinations",
                &out.flat[start..]
            );
        }
        out.spans.push((sym, start as u32, out.flat.len() as u32));
    }
    for &sym in &b.touched {
        b.buckets[sym].clear();
    }
    b.touched.clear();
}

/// Whether every member indexes `fa.d` — the screen a worker applies before calling
/// [`key_rows`], so that a malformed `Fa`'s panic still fires on the main thread at the
/// same metastate the sequential engine reached, not early on a speculative one.
fn members_are_in_range(fa: &Fa, members: &[usize]) -> bool {
    fa.alphabet_size == 0 || members.iter().all(|&q| q < fa.d.len())
}

/// The sequential half: turn one metastate's keys into its transition row, minting ids for
/// keys not seen before. **This is the only place a state id is ever assigned**, it runs
/// only on the calling thread, and it consumes `spans` in ascending symbol order — which
/// together are exactly why the pipeline cannot change the output's state numbering.
fn mint_row(
    keys: &KeyRows,
    metastate_to_id: &mut HashMap<Vec<usize>, usize>,
    metastate_list: &mut Vec<std::sync::Arc<Vec<usize>>>,
) -> BTreeMap<i32, Vec<usize>> {
    let mut row = BTreeMap::new();
    for &(sym, start, end) in &keys.spans {
        let key = &keys.flat[start as usize..end as usize];
        let id = if let Some(&id) = metastate_to_id.get(key) {
            id
        } else {
            let next_id = metastate_list.len();
            metastate_to_id.insert(key.to_vec(), next_id);
            metastate_list.push(std::sync::Arc::new(key.to_vec()));
            next_id
        };
        row.insert(sym, vec![id]);
    }
    row
}

/// [`subset_construction`] with an explicit worker count; `workers == 0` is the plain
/// sequential engine. Split out so tests can force the pipeline on (and off) rather than
/// depending on the host's core count.
///
/// # Why the pipeline cannot change the output
///
/// The loop has exactly two halves, and only one of them depends on history:
///
/// * [`key_rows`] is a **pure function of `fa` and the metastate's members**. It reads no
///   id, no counter, and nothing another metastate produced. Computing it early, late, or
///   on another thread cannot change its result.
/// * [`mint_row`] is where every id is assigned, and it stays on the calling thread,
///   driven by the same `cursor`-ascending × symbol-ascending sequence over the same keys.
///
/// So the sequence of `metastate_to_id` probes — hence the id minted for each new
/// metastate, hence `metastate_list`, hence `d` and `o` — is identical to the sequential
/// engine's for any scheduling of the workers. The pipeline is a *scheduling* change over
/// a pure function, not a change to the construction.
///
/// Two behaviors are preserved deliberately rather than incidentally:
///
/// * **Panic site.** A member outside `0..fa.d.len()` panics inside `fa.d[q]`. Workers
///   screen for it ([`members_are_in_range`]) and hand such a metastate back unevaluated,
///   so the panic still fires on the calling thread, at the same cursor, as it did before.
/// * **Small inputs pay nothing.** Nothing spins up below [`PIPELINE_MIN_STATES`], so the
///   corpus's thousands of small determinizations run the identical sequential code.
fn subset_construction_with_policy(fa: &Fa, initial: &BTreeSet<usize>, policy: Pipeline) -> Fa {
    use std::sync::Arc;

    let mut metastate_list: Vec<Arc<Vec<usize>>> =
        vec![Arc::new(initial.iter().copied().collect())];
    let mut metastate_to_id: HashMap<Vec<usize>, usize> = HashMap::new();
    metastate_to_id.insert((*metastate_list[0]).clone(), 0);

    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
    let mut bufs = ScBuffers::new(fa);
    let mut keys = KeyRows::default();
    let mut cursor = 0;

    // Phase 1 — the untouched sequential engine, which is also the whole run for every
    // input below the pipeline threshold.
    while cursor < metastate_list.len() && metastate_list.len() < policy.min_states {
        let members = metastate_list[cursor].clone();
        key_rows(fa, &members, &mut bufs, &mut keys);
        let row = mint_row(&keys, &mut metastate_to_id, &mut metastate_list);
        d.push(row);
        cursor += 1;
    }

    // Phase 2 — the same loop, with keys for metastates ahead of the cursor computed on
    // worker threads. Entered only when there is enough left to pay for the threads.
    if policy.workers > 0 && metastate_list.len() - cursor >= policy.min_lookahead {
        pipelined_tail(
            fa,
            policy,
            &mut cursor,
            &mut metastate_list,
            &mut metastate_to_id,
            &mut d,
            &mut bufs,
            &mut keys,
        );
    }

    // Phase 3 — whatever the pipeline left (it stops as soon as the frontier is small),
    // again on the plain sequential engine.
    while cursor < metastate_list.len() {
        let members = metastate_list[cursor].clone();
        key_rows(fa, &members, &mut bufs, &mut keys);
        let row = mint_row(&keys, &mut metastate_to_id, &mut metastate_list);
        d.push(row);
        cursor += 1;
    }

    let o = metastate_list
        .iter()
        .map(|ms| i32::from(ms.iter().any(|&q| fa.is_accepting(q))))
        .collect();

    Fa::with_states(0, metastate_list.len(), fa.alphabet_size, o, d)
}

/// One metastate handed to a worker.
type Task = (usize, std::sync::Arc<Vec<usize>>);

/// What a worker hands back for one metastate.
enum Reply {
    /// The computed keys.
    Keys(KeyRows),
    /// Screened out before evaluation ([`members_are_in_range`]) — the mint loop must
    /// evaluate this one itself so the malformed-`Fa` panic fires on the calling thread at
    /// the right cursor.
    Defer,
    /// `key_rows` panicked on the worker. The payload is carried back and re-raised on the
    /// calling thread rather than left to kill the worker: a dead worker never answers,
    /// and the mint loop would block on that answer forever — turning a diagnosable
    /// assertion into a hang, which `CLAUDE.md`'s "never hangs, always a diagnosable
    /// verdict" guardrail exists to prevent. Reachable in practice only through
    /// `key_rows`' own `cfg(debug_assertions)` over-dedup tripwire, since
    /// [`members_are_in_range`] already screens the one panic an ordinary malformed `Fa`
    /// can cause; it is a safety net for the assertions, not a second screen.
    Panicked(Box<dyn std::any::Any + Send + 'static>),
}

type Done = (usize, Reply);

/// How many times a worker has had to carry a panic back (test builds only). The screen
/// [`members_are_in_range`] is supposed to keep this at zero even on a malformed `Fa`;
/// without it the default panic hook prints a `thread '<unnamed>' panicked` line for every
/// speculatively-evaluated bad metastate, including ones the sequential engine never
/// reaches. That stderr noise is the observable difference the screen exists to prevent,
/// and it is what `the_screen_keeps_speculation_from_panicking_on_a_worker` measures.
#[cfg(test)]
static WORKER_PANICS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The scheduling core of phase 2. Runs the mint loop on the calling thread while
/// `workers` threads compute keys for metastates already discovered but not yet reached.
#[allow(clippy::too_many_arguments)]
fn pipelined_tail(
    fa: &Fa,
    policy: Pipeline,
    cursor: &mut usize,
    metastate_list: &mut Vec<std::sync::Arc<Vec<usize>>>,
    metastate_to_id: &mut HashMap<Vec<usize>, usize>,
    d: &mut Vec<BTreeMap<i32, Vec<usize>>>,
    bufs: &mut ScBuffers,
    keys: &mut KeyRows,
) {
    use std::collections::VecDeque;
    use std::sync::mpsc;
    use std::sync::{Condvar, Mutex};

    struct Queue {
        items: VecDeque<Task>,
        closed: bool,
    }
    let queue = Mutex::new(Queue {
        items: VecDeque::new(),
        closed: false,
    });
    let wake = Condvar::new();
    let (done_tx, done_rx) = mpsc::channel::<Done>();

    /// Closes the queue however the scope is left — including by an unwind out of the
    /// mint loop (a malformed `Fa`'s `fa.d[q]` panic). Without this, `thread::scope`'s
    /// implicit join would deadlock against workers still parked on the condvar, turning
    /// a clean panic into a hang.
    struct CloseOnDrop<'a>(&'a Mutex<Queue>, &'a Condvar);
    impl Drop for CloseOnDrop<'_> {
        fn drop(&mut self) {
            if let Ok(mut q) = self.0.lock() {
                q.closed = true;
            }
            self.1.notify_all();
        }
    }

    std::thread::scope(|scope| {
        let _closer = CloseOnDrop(&queue, &wake);
        for _ in 0..policy.workers {
            let tx = done_tx.clone();
            let queue = &queue;
            let wake = &wake;
            scope.spawn(move || {
                let mut bufs = ScBuffers::new(fa);
                loop {
                    let task = {
                        let mut guard = queue.lock().unwrap();
                        loop {
                            if let Some(task) = guard.items.pop_front() {
                                break task;
                            }
                            if guard.closed {
                                return;
                            }
                            guard = wake.wait(guard).unwrap();
                        }
                    };
                    let (index, members) = task;
                    let reply = if members_are_in_range(fa, &members) {
                        let mut out = KeyRows::default();
                        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            key_rows(fa, &members, &mut bufs, &mut out)
                        }));
                        match caught {
                            Ok(()) => Reply::Keys(out),
                            Err(payload) => {
                                #[cfg(test)]
                                WORKER_PANICS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                // `key_rows` unwound part-way, so `bufs` still holds that
                                // metastate's half-filled `buckets`/`touched` — both are
                                // cleared only at the very end of a successful call. This
                                // worker goes on to serve OTHER metastates, and reusing
                                // dirty buffers would union stale destinations into their
                                // keys, which the mint loop may well consume before the
                                // cursor ever reaches the index that panicked. Start clean.
                                bufs = ScBuffers::new(fa);
                                Reply::Panicked(payload)
                            }
                        }
                    } else {
                        Reply::Defer
                    };
                    if tx.send((index, reply)).is_err() {
                        return;
                    }
                }
            });
        }
        drop(done_tx);

        let mut ready: HashMap<usize, Reply> = HashMap::new();
        // Indices in `*cursor..dispatched` are in the workers' hands; the rest of
        // `metastate_list` has not been offered yet. Bounding this to
        // `PIPELINE_WINDOW` past the cursor is what bounds the memory the speculation
        // holds, and costs no throughput (see the constant's docs).
        let mut dispatched = *cursor;
        loop {
            let mut pushed = false;
            {
                let mut guard = queue.lock().unwrap();
                while dispatched < metastate_list.len() && dispatched < *cursor + policy.window {
                    guard
                        .items
                        .push_back((dispatched, metastate_list[dispatched].clone()));
                    dispatched += 1;
                    pushed = true;
                }
            }
            if pushed {
                wake.notify_all();
            }
            if *cursor >= metastate_list.len() {
                break;
            }
            // Hand back to the plain sequential engine once the frontier no longer
            // covers the coordination cost, rather than paying a channel round trip
            // per metastate down to the last one.
            if metastate_list.len() - *cursor < policy.min_lookahead && ready.is_empty() {
                break;
            }

            let reply = match ready.remove(cursor) {
                Some(reply) => reply,
                None => {
                    let mut found = None;
                    while found.is_none() {
                        match done_rx.recv() {
                            Ok((index, reply)) => {
                                if index == *cursor {
                                    found = Some(reply);
                                } else {
                                    ready.insert(index, reply);
                                }
                            }
                            // Unreachable while any worker lives, and `_closer` has not
                            // run yet: `*cursor` was dispatched above and every
                            // dispatched index is answered exactly once (a worker that
                            // panics inside `key_rows` still answers, with
                            // `Reply::Panicked`). Falling back to computing it here keeps
                            // that reasoning off the critical path of correctness.
                            Err(_) => found = Some(Reply::Defer),
                        }
                    }
                    found.unwrap()
                }
            };

            let row = match reply {
                Reply::Keys(computed) => mint_row(&computed, metastate_to_id, metastate_list),
                // Screened out by the worker (or the channel closed): evaluate on this
                // thread, so a malformed `Fa` panics here, at this cursor, exactly as the
                // sequential engine did.
                Reply::Defer => {
                    let members = metastate_list[*cursor].clone();
                    key_rows(fa, &members, bufs, keys);
                    mint_row(keys, metastate_to_id, metastate_list)
                }
                // Re-raised on this thread, so it reaches the caller's
                // `catch_walnut_panic` boundary with its payload intact instead of
                // stranding the mint loop on an answer that will never come.
                Reply::Panicked(payload) => std::panic::resume_unwind(payload),
            };
            d.push(row);
            *cursor += 1;
        }
    });
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

    /// The parallel pipeline, run over the SAME generator and the SAME pre-parallel
    /// reference implementation as `new_matches_the_pre_p1a_reference_implementation`
    /// above — the byte-identity proof for the speculation.
    ///
    /// The production thresholds (4,096 metastates) would never trip on a generated case
    /// this small, so the policy is forced: `min_states: 0`/`min_lookahead: 1` puts every
    /// single case through the dispatch/recv machinery from its very first metastate, and
    /// `window: 3` keeps the in-flight window narrow enough that the top-up path, the
    /// out-of-order `ready` path and the window-full path are all exercised rather than
    /// every task being dispatched in one burst.
    ///
    /// Four workers against a one-metastate frontier is deliberately the *worst* case for
    /// the scheduler — maximum contention, maximum chance that a result arrives for an
    /// index the mint loop has not reached — which is exactly the shape a reordering bug
    /// would need.
    #[test]
    fn the_parallel_pipeline_matches_the_pre_p1a_reference_implementation() {
        const CASES: usize = 20_000;
        let policy = Pipeline {
            workers: 4,
            min_states: 0,
            min_lookahead: 1,
            window: 3,
        };
        let mut cov = GeneratorCoverage::default();
        let mut through_pipeline = 0;
        for case in 0..CASES {
            let seed = 0x5C_01A0_5EED_0001_u64 ^ case as u64;
            let mut rng = Rng(seed);
            let (fa, initial) = random_case(&mut rng, &mut cov);
            let expected = subset_construction_reference(&fa, &initial);
            let actual = subset_construction_with_policy(&fa, &initial, policy);
            if !initial.is_empty() {
                through_pipeline += 1;
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
        // Anti-vacuity: a policy that silently failed to engage would make this test a
        // second copy of the sequential one. `min_lookahead: 1` engages whenever there is
        // at least one metastate to process, which is every case with a non-empty seed.
        assert!(
            through_pipeline > 10_000,
            "only {through_pipeline} of {CASES} cases could have entered the pipeline"
        );
    }

    /// Determinism across repeated parallel runs of one input, which the reference
    /// comparison above does not by itself establish: that test runs each case once, so a
    /// scheduling-dependent result could in principle agree with the reference on the
    /// scheduling it happened to get. This re-runs the same input many times and requires
    /// every run to be byte-identical.
    #[test]
    fn the_parallel_pipeline_is_deterministic_across_repeated_runs() {
        let policy = Pipeline {
            workers: 4,
            min_states: 0,
            min_lookahead: 1,
            window: 2,
        };
        let mut cov = GeneratorCoverage::default();
        for case in 0..400 {
            let seed = 0x5C_01A0_D37E_0001_u64 ^ case as u64;
            let mut rng = Rng(seed);
            let (fa, initial) = random_case(&mut rng, &mut cov);
            let first = subset_construction_with_policy(&fa, &initial, policy);
            for repeat in 1..8 {
                let again = subset_construction_with_policy(&fa, &initial, policy);
                assert_eq!(
                    again.d, first.d,
                    "case {case} repeat {repeat}: transition table differs between runs"
                );
                assert_eq!(
                    again.o, first.o,
                    "case {case} repeat {repeat}: outputs differ"
                );
                assert_eq!(
                    again.q, first.q,
                    "case {case} repeat {repeat}: state count differs"
                );
            }
        }
    }

    /// The malformed-`Fa` panic must still reach the caller, with its message intact, from
    /// the calling thread — that is what `wr_core::walnut_panic`/`Prover::caught` catch.
    ///
    /// Note what this does and does not pin. Two independent mechanisms keep it true: the
    /// `members_are_in_range` screen (the metastate is never evaluated on a worker at all)
    /// and `Reply::Panicked` (a worker panic is carried back and re-raised here). Because
    /// the second alone suffices for *this* assertion, deleting the screen does NOT make
    /// this test fail — an earlier draft claimed it did, and the mutation showed otherwise.
    /// The screen's own effect is the absence of speculative panic-hook output, which
    /// `the_screen_keeps_speculation_from_panicking_on_a_worker` pins instead.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn the_parallel_pipeline_keeps_an_out_of_range_member_panicking_on_the_caller() {
        // One state, one symbol, pointing at a destination `fa.d` has no row for.
        let mut row = BTreeMap::new();
        row.insert(0, vec![7]);
        let fa = Fa::with_states(0, 1, 1, vec![0], vec![row]);
        let policy = Pipeline {
            workers: 4,
            min_states: 0,
            min_lookahead: 1,
            window: 4,
        };
        subset_construction_with_policy(&fa, &[0usize].into_iter().collect(), policy);
    }

    /// The screen's own job, which the `should_panic` test above cannot see: a metastate
    /// that would panic must never be *speculatively* evaluated on a worker.
    ///
    /// Without `members_are_in_range`, index 2 (`{7}`, a destination `fa.d` has no row
    /// for) is dispatched to a worker while the mint loop is still on index 1, and the
    /// default panic hook prints a `thread '<unnamed>' panicked` line for it — stderr the
    /// sequential engine never produces. `WORKER_PANICS` counts exactly those carried-back
    /// panics; deleting the screen makes this assertion fail.
    ///
    /// Measured as a delta rather than an absolute so a concurrently running test in the
    /// same binary cannot make it spuriously fail; no other test is expected to move it.
    #[test]
    fn the_screen_keeps_speculation_from_panicking_on_a_worker() {
        use std::sync::atomic::Ordering;

        let mut row0 = BTreeMap::new();
        row0.insert(0, vec![1]);
        row0.insert(1, vec![7]);
        let mut row1 = BTreeMap::new();
        row1.insert(0, vec![1]);
        // States 0 and 1 are well formed; symbol 1 out of state 0 mints the bad metastate
        // `{7}` as index 2, which the window dispatches while the cursor is still at 1.
        let fa = Fa::with_states(0, 2, 2, vec![0, 1], vec![row0, row1]);
        let policy = Pipeline {
            workers: 4,
            min_states: 0,
            min_lookahead: 1,
            window: 8,
        };

        let before = WORKER_PANICS.load(Ordering::Relaxed);
        let outcome = std::panic::catch_unwind(|| {
            subset_construction_with_policy(&fa, &[0usize].into_iter().collect(), policy)
        });
        let after = WORKER_PANICS.load(Ordering::Relaxed);

        assert!(
            outcome.is_err(),
            "the malformed automaton should still panic on the calling thread"
        );
        assert_eq!(
            after - before,
            0,
            "a worker evaluated a metastate the screen should have held back \
             ({} carried-back panic(s)), which prints panic-hook output the sequential \
             engine never produces",
            after - before
        );
    }

    /// Why a worker that caught a panic must throw its `ScBuffers` away rather than serve
    /// the next metastate with them.
    ///
    /// `key_rows` clears `buckets`/`touched` only at the very END of a successful call, so
    /// an unwind part-way through leaves them holding the failed metastate's partial
    /// union. This pins the consequence directly — the same members through dirty buffers
    /// produce a DIFFERENT key — which is what makes the reset load-bearing rather than
    /// defensive tidying.
    ///
    /// Stated plainly: today the only way to reach that unwind is `key_rows`' own
    /// `cfg(debug_assertions)` over-dedup tripwire, since `members_are_in_range` screens
    /// the one panic an ordinary malformed `Fa` causes. So this test pins the invariant,
    /// not a currently-reachable bug.
    #[test]
    fn key_rows_gives_a_different_answer_through_buffers_a_panic_left_dirty() {
        // Both states go to {0} on symbol 0, so the clean union is exactly [0] and a
        // stray `1` left in the bucket is genuinely visible (a residue already inside the
        // union would be absorbed by the dedup and prove nothing).
        let mut row = BTreeMap::new();
        row.insert(0, vec![0]);
        let fa = Fa::with_states(0, 2, 1, vec![0, 1], vec![row.clone(), row]);
        let members = [0usize, 1];

        let mut clean = ScBuffers::new(&fa);
        let mut from_clean = KeyRows::default();
        key_rows(&fa, &members, &mut clean, &mut from_clean);

        // Exactly the residue a half-finished call leaves: a bucket filled but never
        // drained, and its symbol recorded in `touched`.
        let mut dirty = ScBuffers::new(&fa);
        dirty.buckets[0].push(1);
        dirty.touched.push(0);
        let mut from_dirty = KeyRows::default();
        key_rows(&fa, &members, &mut dirty, &mut from_dirty);

        assert_ne!(
            from_clean.flat, from_dirty.flat,
            "dirty buffers must be observable here -- if they are not, this test has \
             stopped proving that a worker's post-panic `ScBuffers` reset matters"
        );
    }

    /// The screen itself, directly.
    #[test]
    fn members_are_in_range_screens_exactly_the_rows_that_exist() {
        let mut row = BTreeMap::new();
        row.insert(0, vec![0]);
        let fa = Fa::with_states(0, 2, 1, vec![0, 0], vec![row.clone(), row]);
        assert!(members_are_in_range(&fa, &[0, 1]));
        assert!(!members_are_in_range(&fa, &[0, 2]));
        assert!(!members_are_in_range(&fa, &[9]));
        assert!(members_are_in_range(&fa, &[]));

        // `alphabet_size == 0` never reads `fa.d` at all, so nothing needs screening --
        // the load-bearing early-out `key_rows` documents.
        let empty_alphabet = Fa::with_states(0, 1, 0, vec![0], vec![BTreeMap::new()]);
        assert!(members_are_in_range(&empty_alphabet, &[5]));
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
