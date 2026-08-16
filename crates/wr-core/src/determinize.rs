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
    /// fragment of Java's `Determinizing …` log line. Nothing emits it yet (see this
    /// module's docs); it is ported and pinned now because the `details*` golden
    /// fixtures compare that line verbatim.
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
) -> Result<(), DeterminizeError> {
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

        // Java `:111-112` logs `DETERMINIZING <outputName>: <Q> states` here. Not
        // emitted yet -- see this module's docs.
    }

    // Java `:115-119`.
    if strategy != Strategy::Sc && a.fa.is_fao() {
        return Err(DeterminizeError::DfaoWithNonScStrategy(strategy));
    }

    // Java `:121-125`'s switch, minus the deferred OTF arm.
    a.fa = match strategy {
        Strategy::Sc => subset_construction(&a.fa, initial),
        Strategy::Brz => brzozowski(&a.fa, initial)?,
    };
    // In Java this is a brand-new `FA` object, so its `canonized` memo is `false` by
    // construction; this port's flag lives on the `Automaton` wrapper and survives the
    // `fa` swap, so it is reset by hand. See `Automaton::canonized`'s doc comment for
    // the exhaustive list of sites that owe this.
    a.set_canonized(false);

    // Java `:127-130` logs `DETERMINIZED: <Q> states - <ms>ms` here -- unlike the block
    // above, unconditionally (no `shouldPrintDetails` guard; `Logging.logMessage` does
    // its own filtering). Not emitted yet, same reason.
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
    let mut metastate_list: Vec<BTreeSet<usize>> = vec![initial.clone()];
    let mut metastate_to_id: HashMap<BTreeSet<usize>, usize> = HashMap::new();
    metastate_to_id.insert(initial.clone(), 0);

    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
    let mut cursor = 0;
    while cursor < metastate_list.len() {
        let current = metastate_list[cursor].clone();
        let mut row = BTreeMap::new();
        for sym in 0..fa.alphabet_size as i32 {
            let mut union: BTreeSet<usize> = BTreeSet::new();
            for &q in &current {
                if let Some(dests) = fa.d[q].get(&sym) {
                    union.extend(dests.iter().copied());
                }
            }
            if union.is_empty() {
                // SC does not totalize: no transition is recorded here at all.
                continue;
            }
            let next_id = metastate_list.len();
            let id = *metastate_to_id.entry(union.clone()).or_insert_with(|| {
                metastate_list.push(union);
                next_id
            });
            row.insert(sym, vec![id]);
        }
        d.push(row);
        cursor += 1;
    }

    let o = metastate_list
        .iter()
        .map(|ms| i32::from(ms.iter().any(|&q| fa.is_accepting(q))))
        .collect();

    Fa {
        true_false: None,
        q0: 0,
        q: metastate_list.len(),
        alphabet_size: fa.alphabet_size,
        o,
        d,
    }
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
/// reachable, deterministic input, satisfying the real hypothesis. This is also why
/// the `minimize.rs` WB-001 quirk (`q0` not co-reachable to acceptance) cannot fire on
/// the intermediate: a reachable automaton with any accepting state has that state
/// reachable from `q0`, hence `q0` is co-reachable to it. The mid-sequence minimize
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
pub fn brzozowski(fa: &Fa, initial: &BTreeSet<usize>) -> Result<Fa, MinimizeError> {
    debug_assert!(
        fa.o.iter().all(|&o| o <= 1),
        "brzozowski: caller must reject DFAOs first, matching DeterminizationStrategies.\
         determinize's dispatcher-level guard (DeterminizationStrategies.java:115-118) \
         -- this function has no way to error cleanly on one itself"
    );
    // Step 1 (`brzStep(fa, initialStates, SC, "Reverse")`): reverse, then SC.
    let mut reversed = fa.clone();
    let new_initial = reversed.reverse(initial);
    let determinized = subset_construction(&reversed, &new_initial);

    // `fa.justMinimize()`.
    let minimized = crate::minimize::minimize(&determinized)?;

    // Step 2 (`brzStep(fa, IntSet.of(fa.getQ0()), SC, "Reverse of reverse")`): seed
    // with the NEW automaton's `q0` (Java's comment: "Note that initial state is now
    // q0" — i.e. after minimizing, re-seed from the minimized result's own `q0`, not
    // the original `initial` parameter), reverse again, then SC. No minimize after
    // this one (see doc comment above).
    let mut reversed_again = minimized.clone();
    let seed2: BTreeSet<usize> = [reversed_again.q0].into_iter().collect();
    let new_initial2 = reversed_again.reverse(&seed2);
    Ok(subset_construction(&reversed_again, &new_initial2))
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
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, d1],
        }
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
        let nfa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0],
            d: vec![d0],
        };
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
                Fa {
                    true_false: None,
                    q0: 0,
                    q,
                    alphabet_size,
                    o,
                    d,
                }
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
        let brz = brzozowski(&nfa, &initial).unwrap();
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
        let brz = brzozowski(&nfa, &initial).unwrap();
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
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 1],
            d: vec![d0, BTreeMap::new()],
        };
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let brz = brzozowski(&fa, &initial).unwrap();
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
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0],
            d: vec![d0],
        };
        let initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let brz = brzozowski(&fa, &initial).unwrap();
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
            let brz = brzozowski(&fa, &initial).unwrap();
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
        Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 1],
            d: vec![d0, sink, sink2],
        }
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
        assert_eq!(determinize(&mut a, &initial, None), Ok(()));
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

        assert_eq!(determinize(&mut a, &initial, Some(&mut ctx)), Ok(()));

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
        assert_eq!(offered.alphabet, vec![vec![0, 1]]);
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
            assert_eq!(determinize(&mut a, &initial, Some(&mut ctx)), Ok(()));
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
        assert_eq!(determinize(&mut sc, &initial, Some(&mut sc_ctx)), Ok(()));
        assert_eq!(sc.fa.q, 3);
        assert_same_fa(&sc.fa, &subset_construction(&nfa, &initial));

        // `[strategy 0 BRZ]` -> Brzozowski, 2 states, same language.
        let mut brz = as_single_track_automaton(nfa.clone());
        let mut brz_ctx = RecordingContext {
            strategies: [(0, DetStrategy::Brz)].into_iter().collect(),
            ..RecordingContext::default()
        };
        assert_eq!(determinize(&mut brz, &initial, Some(&mut brz_ctx)), Ok(()));
        assert_eq!(brz_ctx.strategy_queries, vec![(0, DetStrategy::Brz)]);
        assert_same_fa(&brz.fa, &brzozowski(&nfa, &initial).unwrap());
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
        assert_eq!(determinize(&mut first, &initial, Some(&mut ctx)), Ok(()));
        let mut second = as_single_track_automaton(nfa);
        assert_eq!(determinize(&mut second, &initial, Some(&mut ctx)), Ok(()));

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
            determinize(&mut a, &initial, Some(&mut ctx)),
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

        assert_eq!(determinize(&mut a, &initial, Some(&mut ctx)), Ok(()));
        assert_same_fa(&a.fa, &subset_construction(&fa, &initial));
        assert!(a.fa.o.iter().all(|&o| o <= 1));

        // And with no context at all -- the pre-U0c path -- likewise.
        let mut b = as_single_track_automaton(fa.clone());
        assert_eq!(determinize(&mut b, &initial, None), Ok(()));
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

        assert_eq!(determinize(&mut a, &initial, Some(&mut ctx)), Ok(()));
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
