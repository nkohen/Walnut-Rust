// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Automaton-level logical operations.
//!
//! Ports `Automata/AutomatonLogicalOps.java`: the five boolean connectives
//! (`and`/`or`/`xor`/`imply`/`iff`) on top of [`crate::product`]'s cross-product
//! engine, single-automaton negation (`not`), right/left language quotient, the
//! general leading/trailing-zero fixup machinery, and the `Automaton`-level
//! `reverse`.
//!
//! # The totalization precondition (the single most load-bearing thing in this file)
//!
//! `AutomatonLogicalOps.and` (`AutomatonLogicalOps.java:41-62`) does **not** totalize
//! either operand — under intersection a missing transition already means "reject", so
//! leaving it missing is correct. Every OTHER connective (`or` `:67-75`, `xor`
//! `:80-92`, `imply` `:97-110`, `iff` `:131-139`) routes through the private
//! `totalizeCrossProduct` (`:112-126`), which calls `A.fa.totalize()` and
//! `B.fa.totalize()` (`:117-118`) **before** `crossProductAndMinimize` (`:119`). On a
//! non-total operand the disjunctive/negated connectives would otherwise silently read
//! a missing transition as "this operand contributes nothing" instead of "true", and
//! compute the wrong language. [`crate::product`]'s functions deliberately do not
//! enforce this themselves (matching Java, where the precondition lives in the caller,
//! not in `ProductStrategies`) — [`totalize_cross_product`] below is where it is
//! enforced, and [`or`]/[`xor`]/[`imply`]/[`iff`] are its only entry points.
//!
//! Java's totalization is **in place, on the caller's automata** — `totalizeCrossProduct`
//! takes plain `Automaton` references and mutates their `fa` fields, so after
//! `or(A, B, ...)` returns, the caller's own `A` and `B` have grown a sink state. That
//! is why [`or`]/[`xor`]/[`imply`]/[`iff`] take `&mut Automaton` here while [`and`]
//! takes `&Automaton`: `and`'s path (`crossProductAndMinimize` -> `A.asDFA()`) returns
//! a *copy* (`Automaton.java:152-158`: "the returned value is a DFA copy; the original
//! object is not retyped"), so `and` genuinely leaves its operands alone.
//!
//! # `friendlyOp` — dropped, after checking it carries no extra behavior
//!
//! Java threads a `String friendlyOp` through every connective. It is *not* only a
//! logging label: it is also the `op` string handed to
//! `ProductStrategies.determineOutput` (`ProductStrategies.java:172-190`), i.e. it
//! selects the truth table. But every production call site passes exactly the constant
//! matching the method it is calling — `LogicalOperator.AND`/`OR`/`XOR`/`IMPLY`/`IFF`
//! (`LogicalOperator.java:38-42`, call sites at `LogicalOperator.java:83-90`,
//! `RelationalOperator.java:128`, `ArithmeticOperator.java:191`, `Union.java:68`,
//! `Automaton.java:263`, and the ~20 `NumberSystem.java` `and` call sites; confirmed by
//! grepping every caller, not assumed). So the parameter is redundant with the method
//! name, and this port fixes the [`BooleanOp`] per function instead of accepting it.
//! A caller *could* pass a mismatched constant in Java (`or(A, B, LogicalOperator.AND)`
//! would compute AND while logging "or"); nothing does, and this port makes that
//! unrepresentable.
//!
//! # `TRUE`/`FALSE` automata: every short-circuit branch is ported (U0)
//!
//! `and`/`or`/`xor`/`imply`/`iff`/`not`/`reverse`/`fixLeadingZerosProblem` each open
//! with an `isTRUE_FALSE_AUTOMATON()` short-circuit. Phase 1/2 skipped all of them
//! because the variant wasn't modeled; U0 added it ([`crate::fa::Fa::true_false`]) and
//! every branch is now ported verbatim, including the details it is easy to get wrong:
//!
//! * **The connectives' short-circuits recurse with the operands SWAPPED** for the four
//!   symmetric ones (`and`/`or`/`xor` `:49`/`:72`/`:89`) rather than duplicating the
//!   trivial-`B` case; [`imply`] is asymmetric and spells both cases out (`:100-107`).
//! * **They bypass totalization entirely** — the trivial branch returns before
//!   `totalizeCrossProduct` runs, so unlike the ordinary path, [`or`]/[`xor`]/[`imply`]/
//!   [`iff`] do NOT mutate their operands when one side is trivial.
//! * **[`iff`]'s trivial branch is defined by recursion**, `and(imply(A,B), imply(B,A))`
//!   (`:132-136`), not by a truth table of its own.
//! * **[`not`]'s trivial branch mutates in place and returns the same object** (`:146-149`)
//!   — flipping only `TRUE_AUTOMATON`, leaving `TRUE_FALSE_AUTOMATON` set.
//!
//! ## `asDFA()` aliasing: a declared, unobservable divergence
//!
//! Java's short-circuits return `AutomatonDFA`, so several of them call `B.asDFA()` on
//! the non-trivial operand. `Automaton.asDFA` is documented as returning "a DFA copy"
//! (`Automaton.java:152-158`) — but `AutomatonDFA.from` short-circuits
//! `if (automaton instanceof AutomatonDFA dfa) return dfa;` (`AutomatonDFA.java:75-78`),
//! so when the operand is *already* an `AutomatonDFA` (which it usually is — every
//! connective here RETURNS one) the "copy" is the operand itself. Two branches then
//! mutate it: `xor`'s `not(result)` (`:85`) and `imply`'s `not(A.asDFA())` (`:106`),
//! since `not` "is mutated and returned" (`:142`). So in Java, `xor(TRUE, B)` can turn
//! the caller's own `B` into `¬B`.
//!
//! This port cannot reproduce that: [`crate::automaton::Automaton::as_dfa`] takes
//! `&self` and clones unconditionally. The divergence is **declared rather than
//! replicated**, on the grounds that it is unobservable in Walnut — every caller of
//! these connectives (`LogicalOperator.act` and friends) pops its operands off the
//! postfix stack and never reads them again, so no code path observes the aliased
//! mutation. Recorded here rather than logged to `docs/WALNUT-BUGS.md`, since nothing
//! reachable produces a wrong answer or a crash from it. Pinned indirectly by
//! `the_trivial_branches_never_totalize_their_operands`, which asserts operands come
//! back unmutated.
//!
//! # `applyAllRepresentations` — live at all three call sites as of U5
//!
//! `totalizeCrossProduct` (`:121`), `not` (`:163`) and `rightQuotient` (`:228`) each call
//! `Automaton.applyAllRepresentations()`, and all three are ported now.
//!
//! **This corrects what this doc said through Phase 2.** The old text argued the method
//! was a guaranteed no-op: its body (`Automaton.java:252-270`) only fires for a track whose
//! `NumberSystem` is non-null AND `useAllRepresentations()`, and that flag
//! (`NumberSystem.flagUseAllRepresentations`, `NumberSystem.java:130`) is cleared in the
//! constructor (`:147-150`) unless `loadAutomatonOrNull` finds a `<name>.txt`
//! "set of all representations" automaton in `Custom Bases/` — never the case for a plain
//! base-*k*, and the whole Fibonacci/Ostrowski/Pell family was then out of scope. Phase 3a's
//! U5 put the *custom-base file mechanism* back in scope (only the bespoke
//! Ostrowski/Fibonacci/Pell *algorithms* were ever dropped — see
//! `crate::numsys::NumberSystem::with_custom_base_files`), so the premise is false and the
//! three calls are real. [`crate::automaton::Automaton::apply_all_representations`] carries
//! the empirical evidence that they change answers rather than merely running.
//!
//! [`and`] deliberately has NO such call, in Java or here — it is the one connective that
//! never totalizes, so it cannot re-admit an invalid representation in the first place.
//!
//! # The former duplication with `wr-logic`'s `quantify.rs` — resolved (U6)
//!
//! `wr_logic::quantify` (landed in the Phase-1 spike, before this unit) used to carry
//! its own ad-hoc copy of `fixLeadingZerosProblem`/`zeroReachableStates`, specialized to
//! the ∃-projection call site; this unit ported the GENERAL version. The U6 architecture
//! unit then moved ∃-projection itself down into [`crate::quantify`] (see that module's
//! docs for why `NumberSystem`'s ten `quantify` call sites force it into `wr-core`), and
//! deleted the ad-hoc copy: [`crate::quantify::quantify`] now calls
//! [`fix_leading_zeros_problem`] below. The two copies were compared line by line first
//! and agreed — same forced `(q0, zero) -> q0` self-loop, same `if (result.add(q))`
//! BFS, same `determinizeAndMinimize(IntSet)` follow-up — differing only in that the
//! `wr-logic` copy surfaced `minimize`'s (unreachable) errors as a `Result` where this
//! one lets [`crate::automaton::Automaton::determinize_and_minimize_from`] panic.
//!
//! # Formerly not ported — this file is complete as of U18
//!
//! Through Phase 2 this section listed `convertNS` (`:455-529`), its four private helpers
//! (`convertMsdBaseToExponent` `:535-560`, `convertLsdBaseToRoot` `:566-657`,
//! `setAutomatonAlphabet` `:662-665`, `computeStringValue` `:671-677`), `combine`
//! (`:679-722`), and the three morphism helpers (`buildTransitionsFromMorphism` `:727-740`,
//! `updateTransitionsFromMorphism` `:745-765`, `buildInitialMorphism` `:771-781`) as
//! deferred, on the grounds that `convertNS` has an **unconditional** `WordAutomaton`
//! dependency (`reverseWithOutput` `:475`/`:496`/`:505`/`:516`/`:526` and
//! `minimizeSelfWithOutput` `:509`/`:521`) and needs a real `NumberSystem` for
//! `parseBase()`/`new NumberSystem(name)`. Both blockers are gone: Phase 3a's U6 landed
//! [`crate::word_automaton`] (and, as its own hard dependency, [`combine`] below), and the
//! `NumberSystem`-shaped inputs are handled by this crate's established per-track stand-in
//! — see [`convert_ns`]'s doc comment for the exact substitution (`parseBase()` is derived
//! from the track's own alphabet). Phase 3b's U18 ports the rest, so nothing in
//! `AutomatonLogicalOps.java` is unported now.
//!
//! **The three `*Morphism` helpers have nothing to do with [`crate::morphism::Morphism`]**
//! despite the shared word, and U18 confirmed that by tracing the call graph rather than
//! assuming. `Morphism.java` maps *letters to integer words*;
//! `buildInitialMorphism`/`updateTransitionsFromMorphism` build the `Q x alphabetSize`
//! **state-transition matrix** `morphism[q][d] = δ(q, d)` and iterate it to obtain
//! `δ*(q, w)` for every length-`exponent` digit word `w` — the digit-grouping step behind
//! `msd_k -> msd_{k^j}`. `Morphism::to_word_automaton` (still deferred in `morphism.rs`)
//! is NOT on this file's call graph, so U18 did not need it, and therefore did not need
//! the `Fa::canonized` flag that `morphism.rs`'s docs name as its prerequisite. The only
//! `setCanonized` call U18 touches at all is `convertLsdBaseToRoot`'s `setCanonized(false)`
//! — see [`convert_lsd_base_to_root`], where it is a no-op for the opposite reason.
//!
//! `removeLeadingZeros` (`:343-367`) + `removeLeadingZerosHelper` (`:375-405`) used to be
//! listed here as deferred too; **Phase 3a's U10 ports them** ([`remove_leading_zeros`]),
//! since that is the unit whose `I` quantifier consumes them
//! (`LogicalOperator.java:151`). Their original blocker — the `new Automaton(false)` fold
//! identity (`:356`) and the `new AutomatonDFA(true)` no-numeration-system case
//! (`:381-383`), both TRUE/FALSE automata — was removed by U0.
//!
//! # `fa.setCanonized(false)`
//!
//! `fixLeadingZerosProblem` (`:273`), `fixTrailingZerosProblem` (`:326`) and
//! `convertLsdBaseToRoot` (`:647`) each clear `FA`'s private `canonized` memo flag. `Fa`
//! carries no such flag (it always recomputes — see `fa.rs`'s doc comment on
//! `Fa::canonicalize`), so there is nothing to clear here.
//!
//! Note the DIRECTION, because it is what makes the missing flag harmless at all three of
//! these sites and *not* harmless at `Morphism.java:88`: clearing the flag means "canonicalize
//! next time you're asked", which is exactly what a flagless `Fa::canonicalize` already does.
//! `setCanonized(**true**)` is the dangerous one — it SUPPRESSES a canonicalization that
//! would otherwise drop `q0`-unreachable states — and no call site in this file sets it.
//!
//! # Logging / timing
//!
//! Every method in the Java file brackets its work in `logMessage`/`Logging.indent()`
//! and `System.currentTimeMillis()` timing. None of it is ported (diagnostic output, not
//! behavior — the same call this file's siblings already made), and that includes
//! `convertNS`'s own `Logging.indent()`/`dedent()` pairs (`:474-476`, `:492`/`:528`) and
//! the four `CONVERTING`/`CONVERTED` lines in its two helpers.
//!
//! **This paragraph used to say "this crate has no `Logging` module", which went stale in
//! Phase 3a** — [`crate::logging::Logging`] exists now. The decision is unchanged, for the
//! reason `determinize.rs` states in its own identical note: no `wr-core` *algorithm*
//! threads a `Logging` handle yet, so adding one here alone would be an inconsistent
//! partial wiring. Whichever unit threads logging through `wr-core` should do it for all of
//! them at once.

use crate::automaton::{Automaton, AutomatonDFA};
use crate::fa::Fa;
use crate::minimize::MinimizeError;
use crate::product::{cross_product, cross_product_and_minimize, BooleanOp};
use crate::util;
use crate::word_automaton;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// `FA`-level helpers this file needs that `fa.rs` does not (yet) expose.
//
// All four are ports of `Automata/FA/FA.java` methods. They live here rather than in
// `fa.rs` to keep this unit's diff contained to one new file; a later `fa.rs`
// consolidation unit may want to move them (and, in `totalize`'s case, reconcile them
// with `Fa::totalize`).
// ---------------------------------------------------------------------------

/// `FA.totalize()` (`FA.java:219-236`) together with its two private halves,
/// `totalizeStates` (`:336-344`)/`addMissingTransitionsForState` (`:356-367`) and
/// `addSinkState(0, sinkState)` (`:315-321`).
///
/// Routes every missing `(state, symbol)` pair to a fresh sink state (index `fa.q` at
/// entry), and appends that sink — non-accepting (Java passes literal `0` as the sink's
/// output) and self-looping on every symbol — **only if at least one transition was
/// actually missing**, exactly matching Java's `if (!totalizeStates(sinkState))
/// addSinkState(0, sinkState)` guard.
///
/// # Why not [`Fa::totalize`]
///
/// `Fa::totalize` asserts `is_deterministic()` and early-returns on
/// `is_deterministic_and_total()`. Java's `FA.totalize` does neither: it fills missing
/// symbols on an NFA-shaped table just as happily, and its "already total" test is
/// purely "does every state have every symbol", with no determinism component. That
/// difference is live here: [`totalize_cross_product`] totalizes plain `Automaton`
/// operands which may well be nondeterministic (Java only determinizes afterwards,
/// inside `crossProductAndMinimize` -> `asDFA()`), so calling `Fa::totalize` there
/// would panic where Java works. The two agree exactly on deterministic input.
///
/// A thin wrapper around [`Fa::totalize_relaxed`] (Java's sink output here is always
/// the literal `0`, per `FA.totalize`'s `addSinkState(0, sinkState)` — unlike
/// `addDistinguishedDeadState`'s `determineMinOutput() - 1`, which is what
/// `Fa::add_distinguished_dead_state` passes at its own call site instead). Kept as a
/// free function here, rather than inlined at each call site, purely to preserve this
/// module's existing call shape (`totalize(&mut a.fa)`) across its several callers
/// below.
///
/// `pub` as of U24: `Main/Commands/Join.java:82-83` (`first.fa.totalize();
/// next.fa.totalize();`) calls this exact real `FA.totalize()`, not `Fa::totalize`'s
/// stricter assert-based cousin — `crate::join`'s port needs the identical relaxed
/// behavior (a `join` operand is expected to already be deterministic in practice,
/// but Java's own `totalize()` never asserts that, so neither should the port).
pub fn totalize(fa: &mut Fa, logging: &mut crate::logging::Logging) {
    // `FA.totalize()`'s own logging (`FA.java:218-233`) -- no indent/dedent, just the
    // TOTALIZING/TOTALIZED pair. Every caller of this function (the `or`/`xor`/`imply`/
    // `iff` family's `totalize_cross_product`, `wr-cli`'s `join`) needs NO logging of its
    // own around these calls as a result. NOT `reverse` -- an earlier draft of this note
    // listed it too, which was simply wrong: `AutomatonLogicalOps.reverse` never calls
    // `FA.totalize()` at all (confirmed by reading its source), and it has its own real
    // logging pair (`REVERSING`/`REVERSED`, `reverse_with_ctx`'s own doc comment) around
    // an entirely different operation.
    let time_before = std::time::Instant::now();
    logging.log_message(&format!("{}:{} states", crate::logging::TOTALIZING, fa.q));
    fa.totalize_relaxed(0);
    logging.log_message(&format!(
        "{}:{} states - {}ms",
        crate::logging::TOTALIZED,
        fa.q,
        time_before.elapsed().as_millis()
    ));
}

/// `FA.flipOutput()` (`FA.java:423-426`), i.e. `setOutputIfEqual(q, !isAccepting(q))`
/// for every state, where `setOutputIfEqual` (`FA.java:411-413`) writes a literal
/// `0`/`1`. Note the consequence Java has too: a DFAO output value `> 1` is *not*
/// negated into something meaningful, it collapses to `0`.
fn flip_output(fa: &mut Fa) {
    for q in 0..fa.q {
        fa.o[q] = i32::from(!fa.is_accepting(q));
    }
}

/// `FA.justMinimize()` (`FA.java:576-588`): `convertNFAtoDFA()` (which throws
/// `"Unexpected NFA instead of DFA."` on genuine nondeterminism, `FA.java:700-706`)
/// followed by Valmari minimization. Both of [`crate::minimize::minimize`]'s error
/// variants are turned back into Java's own throw messages rather than surfaced as a
/// `Result` — matching this crate's established convention for faithfully-ported
/// `WalnutException`s (see `product.rs`/`automaton.rs`).
///
/// `minimize`'s OTHER (documented, unenforced) precondition — every state reachable
/// from `q0` — is *not* established here, exactly as in Java: `justMinimize` never
/// trims. That used to make `docs/WALNUT-BUGS.md` WB-001 reachable through every caller
/// of this helper ([`not`], [`fix_trailing_zeros_problem`]) whose input has a state
/// unreachable from `q0`. **WB-001 is fixed** (`walnut-java` commit `14509f1`, ported in
/// [`crate::minimize::minimize`]), so those callers now get the right language. The
/// missing trim still costs *minimality*: an unreachable-but-co-reachable state survives
/// as an extra block. Faithful to Java either way — no trim is added here.
fn just_minimize(fa: &Fa, logging: &mut crate::logging::Logging) -> Fa {
    match crate::minimize::minimize_with_logging(fa, logging) {
        Ok(minimized) => minimized,
        Err(MinimizeError::NotDeterministic) => panic!("Unexpected NFA instead of DFA."),
        Err(MinimizeError::ConflictingTransitions) => {
            panic!("Valmari minimization produced conflicting DFA transitions.")
        }
    }
}

/// `FA.setStatesReachableToFinalStatesByZeros(int zero)` (`FA.java:476-504`): marks
/// every state that can reach an accepting state by reading `zero*` as accepting
/// itself, and returns whether that changed anything.
///
/// Builds the reverse adjacency of the `zero`-labelled edges only, seeds a BFS queue
/// with every currently-accepting state, and walks backwards. The `altered` flag is
/// Java's `altered = altered || (O.getInt(q) != 1)` over the reached set — note the
/// `!= 1` rather than `== 0`, so a DFAO output value `> 1` also counts as "altered"
/// (and is then overwritten with `1`).
///
/// Java accumulates the reached set in a `HashSet<Integer>`; a `BTreeSet` is used here
/// per `PORTING.md`'s iteration-order rule. Nothing depends on the order (the final
/// loop ORs a flag and writes the same value to every member), so this is
/// determinism hygiene, not a behavior change.
fn set_states_reachable_to_final_states_by_zeros(fa: &mut Fa, zero: i32) -> bool {
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); fa.q];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for q in 0..fa.q {
        if let Some(dests) = fa.d[q].get(&zero) {
            for &p in dests {
                adjacency[p].push(q);
            }
        }
        if fa.is_accepting(q) {
            queue.push_back(q);
        }
    }

    let mut result: BTreeSet<usize> = BTreeSet::new();
    while let Some(q) = queue.pop_front() {
        // Java adds unconditionally here (unlike `zeroReachableStates`'s
        // `if (result.add(q))` guard) and re-scans an already-reached state's
        // neighbours; harmless, and ported as-is.
        result.insert(q);
        for &p in &adjacency[q] {
            if !result.contains(&p) {
                queue.push_back(p);
            }
        }
    }

    let mut altered = false;
    for &q in &result {
        altered = altered || fa.o[q] != 1;
        fa.o[q] = 1;
    }
    altered
}

/// `RichAlphabet.isSubsetA(r1, r2)` (`RichAlphabet.java:39-49`): is `r1`'s per-track
/// alphabet a subset of `r2`'s? Arity must match exactly; each track is compared as a
/// SET (`new HashSet<>(r2.A[i]).containsAll(r1.A[i])`), so digit order and duplicates
/// are irrelevant.
fn is_subset_alphabet(r1: &[Vec<i32>], r2: &[Vec<i32>]) -> bool {
    if r1.len() != r2.len() {
        return false;
    }
    r1.iter().zip(r2.iter()).all(|(t1, t2)| {
        let set2: HashSet<i32> = t2.iter().copied().collect();
        t1.iter().all(|d| set2.contains(d))
    })
}

/// `NumberSystem.flipNS(List<NumberSystem>)` (`NumberSystem.java:166-178`): swaps
/// `msd_k` for `lsd_k` and vice versa on every track, **skipping null entries**
/// (`if (NS == null) continue;`). `msd: Vec<Option<bool>>` is this crate's stand-in for
/// Java's per-track `List<NumberSystem>` (see `Automaton`'s struct doc comment), so
/// `None` plays the null role and the base — which Java re-parses out of the name and
/// preserves — is carried by the track's alphabet here.
///
/// # The NAME has to be flipped too, not just the direction flag
///
/// Java replaces the whole object with `new NumberSystem(newName)`, so
/// `NumberSystem.getName()` afterwards is the FLIPPED name. This port splits Java's one
/// object into the parallel [`Automaton::msd`] flag and [`Automaton::ns_name`] string,
/// and until this fix only the flag was flipped. That was a live, silent
/// wrong-output bug, because [`Automaton::track_ns_names`] deliberately *prefers* the
/// recorded name over reconstructing one from the flag (it has to — a custom base's name
/// is not derivable from its alphabet):
///
/// * `reverse rv $ok;` on an `msd_2` automaton wrote a `rv.txt` headed `msd_2` where real
///   Walnut writes `lsd_2` (confirmed live against `Walnut-all.jar`; the bodies were
///   byte-identical, only the header was wrong);
/// * worse, `union`/`intersect`/`concat` compare number systems BY NAME
///   (`NumberSystem.isNSDiffering`), so `union mixed rv two` — which real Walnut refuses
///   with `Automata must have the same number system(s).`, writing nothing — silently
///   SUCCEEDED here, writing a union of an lsd automaton with an msd one.
///
/// Double reversal happened to round-trip anyway (two stale flips cancel), which is why
/// the existing `reverse`-twice coverage never caught it.
///
/// The new name is Java's exactly (`:172-174`): `determineMsdOrLsd`/`determineBase` split
/// the OLD name at its FIRST `_`, and the result is
/// `(prefix == "msd" ? "lsd" : "msd") + "_" + <everything after the first underscore>`.
/// So `msd_2` ⇄ `lsd_2`, `msd_fib` ⇄ `lsd_fib`, `msd_neg_3` ⇄ `lsd_neg_3` — and note it is
/// NOT a symmetric prefix swap: a name whose prefix is neither `msd` nor `lsd` becomes
/// `msd_…`, because Java's ternary tests only for `MSD`. The direction flag is then taken
/// from the NEW name (`NumberSystem`'s constructor does `isMsd = msdOrLsd.equals(MSD)`,
/// `:136`), which keeps the two halves of this port's split representation consistent with
/// each other for that same non-`msd`/`lsd` prefix case.
///
/// A track with no recorded name (an automaton this crate built in memory, always a plain
/// base-*k* one) just has its flag flipped, which is what `track_ns_names`'s reconstruction
/// branch then renders.
///
/// # The custom-base half, and its one declared limitation (U5)
///
/// Java doesn't flip a flag: it *replaces* each entry with
/// `new NumberSystem("msd"|"lsd" + "_" + base)`, which for a custom base re-runs the whole
/// `Custom Bases/` file-loading dance under the flipped name. So the flipped system's
/// `getAllRepresentations()` is whatever
/// `loadAutomatonOrNull` finds for the new direction. `wr-core` performs no file I/O
/// (U5's whole design premise), so it cannot re-run that lookup here.
///
/// What it does instead is reproduce the *outcome* for the only shape that actually ships:
/// every automaton in `walnut-java/Custom Bases/` exists in the `msd_*` direction only, so
/// `loadAutomatonOrNull` for the flipped name always misses the main file, hits the
/// complement, and returns `AutomatonLogicalOps.reverse(loaded, false)`
/// (`NumberSystem.java:311-315`) — i.e. the language-reversal of the same automaton, which
/// is also exactly the right restriction for tracks whose digits are now read in the
/// opposite order. So each track's all-representations automaton is reversed in place.
///
/// **Where this diverges from Java:** if a user supplies BOTH directions' files with
/// languages that are not each other's reversal (e.g. a hand-written `lsd_foo.txt` that is
/// not `reverse(msd_foo.txt)`), Java would load the flipped file while this reverses the
/// original. Declared here rather than papered over; closing it needs a caller-supplied
/// flipped-base file set, which is `wr-io`/`wr-cli`'s (U13/U14's) side of the boundary —
/// see [`crate::numsys::NumberSystem::flip_with_custom_base_files`], which is the API a
/// caller that *does* have the files should use instead of relying on this.
pub(crate) fn flip_ns(a: &mut Automaton) {
    for i in 0..a.msd.len() {
        // `if (NS == null) continue;` — a `{0,1}`-style explicit-set track has no
        // number system at all, so neither its flag nor its (necessarily absent) name
        // is touched.
        if a.msd[i].is_none() {
            continue;
        }
        match a.ns_name.get(i).and_then(|n| n.clone()) {
            Some(name) => {
                let new_name = flipped_ns_name(&name);
                // `NumberSystem`'s constructor: `isMsd = determineMsdOrLsd(name).equals(MSD)`.
                a.msd[i] = Some(new_name.starts_with(crate::numsys::MSD_UNDERSCORE));
                a.ns_name[i] = Some(new_name);
            }
            None => a.msd[i] = Some(!a.msd[i].unwrap()),
        }
    }
    for slot in a.all_reps.iter_mut() {
        if let Some(all_reps) = slot.as_mut() {
            let mut flipped = (**all_reps).clone();
            reverse(&mut flipped, false);
            *all_reps = Rc::new(flipped);
        }
    }
}

/// The name half of [`flip_ns`]'s loop body (`NumberSystem.java:172-174`):
/// `(determineMsdOrLsd(name).equals(MSD) ? LSD : MSD) + "_" + determineBase(name)`.
///
/// # The no-underscore case
///
/// `determineMsdOrLsd` is `name.substring(0, name.indexOf("_"))`, so a name with no `_`
/// at all makes Java evaluate `substring(0, -1)` and throw
/// `StringIndexOutOfBoundsException`. That is unreachable through every real path into
/// [`Automaton::ns_name`] — every writer of it (`wr-io`'s reader, `wr-cli`'s
/// `alphabet`/`reg`, `crate::numsys`) sources the string from
/// `normalize_number_system_token` or a [`crate::numsys::NumberSystem`]'s own name, and
/// every branch of the former yields an `msd_`/`lsd_`-prefixed string — but
/// [`Automaton::set_ns_names`] does not validate the shape, so the case is representable
/// in-crate. It is ported the way [`Automaton::decode`]'s equivalent is: `panic!` with the
/// JDK's own message and nothing else, recovered at `wr_cli::prover`'s
/// `Prover::caught` boundary exactly as `Prover.readBuffer`'s `catch (RuntimeException)`
/// recovers Java's.
fn flipped_ns_name(name: &str) -> String {
    let Some(underscore) = name.find('_') else {
        // `String.substring(0, -1)`'s message verbatim (`checkBoundsBeginEnd`).
        panic!("begin 0, end -1, length {}", name.len());
    };
    let msd_or_lsd = &name[..underscore];
    let base = &name[underscore + 1..];
    let flipped = if msd_or_lsd == crate::numsys::MSD {
        crate::numsys::LSD
    } else {
        crate::numsys::MSD
    };
    format!("{flipped}_{base}")
}

// ---------------------------------------------------------------------------
// The five boolean connectives.
// ---------------------------------------------------------------------------

/// `AutomatonLogicalOps.and(Automaton, Automaton)` / `and(Automaton, Automaton, String)`
/// (`:41-62`) — `L(A) ∩ L(B)`.
///
/// Deliberately does **not** totalize either operand (see this module's docs): under
/// intersection a missing transition already means "reject". Consequently, unlike
/// [`or`]/[`xor`]/[`imply`]/[`iff`], this leaves both operands untouched — Java's
/// `crossProductAndMinimize(Automaton, Automaton, String)` (`ProductStrategies.java:220-222`)
/// goes through `A.asDFA()`, which copies rather than retyping in place.
///
/// The TRUE/FALSE short-circuit (`:45-50`) is ported exactly, including its
/// swap-and-recurse for the trivial-`b` case ("and is symmetric"). Note that when `a`
/// is the TRUE automaton the result is `b.as_dfa()` — which for a trivial `b` is
/// `b` itself, so `and(TRUE, TRUE) == TRUE` and `and(TRUE, FALSE) == FALSE` fall out
/// without a separate both-trivial case, exactly as in Java.
///
/// **Index-accounting note, and it applies to [`or`]/[`xor`]/[`imply`]/[`iff`] equally:**
/// the `as_dfa()` in the trivial short-circuit below passes no
/// [`crate::determinize::DeterminizeContext`], which assumes the surviving operand is
/// already deterministic. See [`Automaton::as_dfa`]'s docs for the full statement of that
/// assumption, why it holds on the `eval` call graph, and what a caller that can violate
/// it should do instead.
pub fn and(a: &Automaton, b: &Automaton, logging: &mut crate::logging::Logging) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        if a.fa.is_true_false_automaton() {
            return if a.fa.is_true_automaton() {
                b.as_dfa()
            } else {
                AutomatonDFA::true_false(false)
            };
        }
        return and(b, a, logging); // and is symmetric
    }
    // `logMessage(COMPUTING + " " + friendlyOp + ":" + A.Q + " states - " + B.Q + "
    // states")`, `indent()`, `crossProductAndMinimize`, `dedent()`, `logMessage(COMPUTED
    // ...)` (`AutomatonLogicalOps.java:51-58`) -- skipped entirely on the TRUE/FALSE
    // short-circuit above, which is why this whole block sits after it.
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} &:{} states - {} states",
        crate::logging::COMPUTING,
        a.fa.q,
        b.fa.q
    ));
    logging.indent();
    let n = cross_product_and_minimize(a, b, |p, q| BooleanOp::And.combine(p, q), logging);
    logging.dedent();
    logging.log_message(&format!(
        "{} &:{} states - {}ms",
        crate::logging::COMPUTED,
        n.automaton().fa.q,
        time_before.elapsed().as_millis()
    ));
    n
}

/// `AutomatonLogicalOps.totalizeCrossProduct` (`:112-126`) — the precondition-enforcing
/// wrapper shared by [`or`]/[`xor`]/[`imply`]/[`iff`]. Totalizes BOTH operands in
/// place (Java: `A.fa.totalize(); B.fa.totalize();`, `:117-118`) and only then runs the
/// cross product. See this module's docs for why skipping this computes the wrong
/// language for these four connectives but not for [`and`].
fn totalize_cross_product(
    a: &mut Automaton,
    b: &mut Automaton,
    op: BooleanOp,
    logging: &mut crate::logging::Logging,
) -> AutomatonDFA {
    // `logMessage(COMPUTING + " " + friendlyOp + ":" + A.Q + " states - " + B.Q + "
    // states")`, THEN `indent()` (`:112-116`) -- brackets totalize/totalize/
    // crossProductAndMinimize ONLY; `dedent()` runs BEFORE `applyAllRepresentations`,
    // unlike `not`'s bracket (which includes it). Both totalize calls log their own
    // TOTALIZING/TOTALIZED pair internally (`FA.totalize`, see `totalize`'s own docs) --
    // no separate wrapping needed here for those.
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} {}:{} states - {} states",
        crate::logging::COMPUTING,
        op.symbol(),
        a.fa.q,
        b.fa.q
    ));
    logging.indent();
    totalize(&mut a.fa, logging);
    totalize(&mut b.fa, logging);
    let mut n = cross_product_and_minimize(a, b, |p, q| op.combine(p, q), logging).into_automaton();
    logging.dedent();
    // `N.applyAllRepresentations()` (`:121`). Live as of U5 — and load-bearing exactly
    // here: `totalize` above adds a sink that accepts every previously-missing transition,
    // which for a custom base re-admits INVALID digit strings, so the restriction has to be
    // re-applied afterwards. (`and` deliberately has no such call: it never totalizes.)
    n.apply_all_representations(logging);
    logging.log_message(&format!(
        "{} {}:{} states - {}ms",
        crate::logging::COMPUTED,
        op.symbol(),
        n.fa.q,
        time_before.elapsed().as_millis()
    ));
    AutomatonDFA::from(n)
}

/// `AutomatonLogicalOps.or` (`:67-75`) — `L(A) ∪ L(B)`. Totalizes both operands in
/// place first; see [`totalize_cross_product`]. The TRUE/FALSE short-circuit
/// (`:68-73`) runs first and does NOT totalize (see this module's docs).
pub fn or(
    a: &mut Automaton,
    b: &mut Automaton,
    logging: &mut crate::logging::Logging,
) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        if a.fa.is_true_false_automaton() {
            return if a.fa.is_true_automaton() {
                AutomatonDFA::true_false(true)
            } else {
                b.as_dfa()
            };
        }
        return or(b, a, logging); // or is symmetric
    }
    totalize_cross_product(a, b, BooleanOp::Or, logging)
}

/// `AutomatonLogicalOps.xor` (`:80-92`) — symmetric difference. Totalizes both operands
/// in place first; see [`totalize_cross_product`]. The TRUE/FALSE short-circuit
/// (`:81-90`) runs first and does NOT totalize (see this module's docs).
pub fn xor(
    a: &mut Automaton,
    b: &mut Automaton,
    logging: &mut crate::logging::Logging,
) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        if a.fa.is_true_false_automaton() {
            // Java: `result = B.asDFA(); if (A.isTRUE) return not(result); return result;`
            // — i.e. `TRUE xor B == not(B)`, `FALSE xor B == B`. When `b` is trivial too,
            // `not` takes its own trivial branch and just flips the truth value.
            let result = b.as_dfa();
            if a.fa.is_true_automaton() {
                return not(result, logging);
            }
            return result;
        }
        return xor(b, a, logging); // xor is symmetric
    }
    totalize_cross_product(a, b, BooleanOp::Xor, logging)
}

/// `AutomatonLogicalOps.imply` (`:97-110`) — `¬L(A) ∪ L(B)`. **Asymmetric**: the
/// operand order matters. Totalizes both operands in place first; see
/// [`totalize_cross_product`].
///
/// The TRUE/FALSE short-circuit (`:98-108`) is the one connective that cannot
/// swap-and-recurse, so Java spells out both sides; ported verbatim. Note the second
/// half reads `B.isTRUE_AUTOMATON()` with no enclosing `B.isTRUE_FALSE_AUTOMATON()`
/// check — sound only because reaching it means `a` is NOT trivial while the outer
/// disjunction held, so `b` must be (see `crate::fa`'s module docs, which cite this
/// exact line).
pub fn imply(
    a: &mut Automaton,
    b: &mut Automaton,
    logging: &mut crate::logging::Logging,
) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        // not a or b
        if a.fa.is_true_false_automaton() {
            return if a.fa.is_true_automaton() {
                b.as_dfa()
            } else {
                AutomatonDFA::true_false(true)
            };
        }
        return if b.fa.is_true_automaton() {
            AutomatonDFA::true_false(true)
        } else {
            not(a.as_dfa(), logging)
        };
    }
    totalize_cross_product(a, b, BooleanOp::Imply, logging)
}

/// `AutomatonLogicalOps.iff` (`:131-139`) — the complement of [`xor`]. Totalizes both
/// operands in place first; see [`totalize_cross_product`].
///
/// The TRUE/FALSE short-circuit (`:132-136`) is defined by RECURSION rather than by a
/// truth table: `and(imply(A, B), imply(B, A))`. Ported verbatim. This terminates and
/// never reaches [`crate::product`]: with at least one operand trivial, each `imply`
/// takes its own short-circuit, and at least one of the two results is itself trivial,
/// so the closing [`and`] short-circuits too.
pub fn iff(
    a: &mut Automaton,
    b: &mut Automaton,
    logging: &mut crate::logging::Logging,
) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        let c = imply(a, b, logging);
        let d = imply(b, a, logging);
        return and(c.automaton(), d.automaton(), logging);
    }
    totalize_cross_product(a, b, BooleanOp::Iff, logging)
}

/// `AutomatonLogicalOps.not(AutomatonDFA)` (`:144-170`) — complementation.
///
/// Java's working sequence is `totalize()` (`:160`), `flipOutput()` (`:161`),
/// `justMinimize()` (`:162`) — all three ported below, preceded by the
/// `isTRUE_FALSE_AUTOMATON` short-circuit (`:146-149`, U0), which flips only
/// `TRUE_AUTOMATON` and returns the same (still-trivial) automaton, plus
/// `applyAllRepresentations()` (`:163`, live as of U5) and `convertNFAtoDFA()` (`:164`,
/// discharged by the closing [`AutomatonDFA::from`]). Totalization is what makes
/// this a genuine complement rather than a mere output flip — without it, a word that
/// runs off the end of a partial transition table would be rejected by BOTH the
/// automaton and its "negation".
///
/// Java takes and returns the same object ("The automaton is mutated and returned",
/// `:142`); the by-value signature here is Rust's equivalent of that ownership
/// transfer, and it is also what lets the final `convertNFAtoDFA()` (`:164`) be
/// discharged by [`AutomatonDFA::from`]'s own `requireDfaStorage` check on the way out.
///
/// Java's `if (!A.getFa().getT().isDeterministic()) throw WalnutException.nonDeterministic()`
/// guard (`:151-153`, message `"NFA found when expecting a DFA."`,
/// `WalnutException.java:99`) has no analog: this crate's [`AutomatonDFA`] enforces
/// determinism through the TYPE (see its doc comment in `automaton.rs`), so by the time
/// this function holds one, the guard is already a static invariant. That also makes
/// Java's `AutomatonLogicalOpsTest.testNotThrowsOnNondeterministicInput` unreplicable —
/// its fixture builds an `AutomatonDFA` and then reaches past the type to write a
/// two-destination transition, which module privacy rules out here.
pub fn not(a: AutomatonDFA, logging: &mut crate::logging::Logging) -> AutomatonDFA {
    // `boolean print = Logging.shouldPrintDetails();` (`:145`) -- captured ONCE, before
    // even the trivial check, and reused for both the COMPUTING and COMPUTED calls via
    // the 2-arg `logMessage(print, msg)` overload (not the 1-arg ambient-gate form used
    // elsewhere in this file).
    let print = logging.should_print_details();
    let mut m = a.into_automaton();
    if m.fa.is_true_false_automaton() {
        // Java mutates in place: `setTRUE_AUTOMATON(!isTRUE_AUTOMATON())`, leaving
        // `TRUE_FALSE_AUTOMATON` set, and returns the SAME object — so any stale
        // `q`/alphabet the argument was carrying survives.
        //
        // An earlier version of this fix rebuilt a fresh trivial automaton here instead
        // (`AutomatonDFA::true_false(...)`) and this comment claimed the difference was
        // "unobservable: every reader of a trivial automaton consults only `true_false`".
        // That claim was WRONG — found via `crates/wr-logic/src/eval.rs`'s own later
        // Phase-3a docs, which record a real, concrete divergence: for `?msd_2 Ax (x >=
        // 5)`, real Walnut logs `(A x x>=5):4 states` (the STALE state count from the
        // pre-quantify negation, since neither Java's `not` nor `AutomatonQuantification
        // .quantify`'s all-tracks-quantified early return ever reset `Q`), while this
        // port logged `0 states` — building a fresh trivial value threw the stale count
        // away instead of preserving it. `compute`'s own per-step logging is exactly the
        // "reader" this port's original claim said didn't exist.
        //
        // `AutomatonDFA::from` (not `AutomatonDFA::true_false`) is the right wrap here:
        // its own trivial branch was found to have the SAME bug via the identical call
        // graph (`actQuantifier`'s `FORALL` branch calls `M.asDFA()` again right after
        // this function returns) and was fixed alongside this — see its docs.
        m.fa.true_false = Some(!m.fa.is_true_automaton());
        return AutomatonDFA::from(m);
    }
    // `logMessage(print, COMPUTING + " " + Operator.NEGATE + ":" + Q + " states")`
    // (`:159`), THEN `Logging.indent()` (`:160`) -- brackets totalize/flipOutput/
    // justMinimize/applyAllRepresentations, dedented right before the COMPUTED line.
    let time_before = std::time::Instant::now();
    logging.log_message_with(print, &format!("computing ~:{} states", m.fa.q));
    logging.indent();
    totalize(&mut m.fa, logging);
    flip_output(&mut m.fa);
    m.fa = just_minimize(&m.fa, logging);
    // `FA.justMinimize`'s own `this.canonized = false;` (`FA.java:584`) -- this port's
    // flag lives on the `Automaton` wrapper, so it is reset by hand at every
    // `just_minimize` call site. See `Automaton::canonized`'s doc comment.
    m.set_canonized(false);
    // `A.applyAllRepresentations()` (`:163`). Live as of U5, and the single most
    // load-bearing of its three call sites: complementing a language restricted to a custom
    // base's VALID representations re-admits every invalid one, so without this
    // `~(x=x)` over `msd_fib` would return "the strings containing `11`" instead of the
    // empty language (empirically confirmed against the real Walnut CLI — see
    // `Automaton::apply_all_representations`).
    m.apply_all_representations(logging);
    logging.dedent();
    logging.log_message_with(
        print,
        &format!(
            "computed ~:{} states - {}ms",
            m.fa.q,
            time_before.elapsed().as_millis()
        ),
    );
    AutomatonDFA::from(m)
}

// ---------------------------------------------------------------------------
// Quotients.
// ---------------------------------------------------------------------------

/// `AutomatonLogicalOps.rightQuotient` (`:176-235`) — `L(A) / L(B) = { x : ∃ y ∈ L(B),
/// xy ∈ L(A) }`.
///
/// The construction: keep `A`'s states and transitions verbatim and recompute only its
/// *accepting set* — state `i` becomes accepting iff `L(A from i) ∩ L(B) ≠ ∅`, decided
/// by running the intersection through [`and`] and asking [`Automaton::is_empty`]
/// (`:222-224`).
///
/// Two bookkeeping steps carry real weight:
///
/// * **`B` is re-encoded into `A`'s alphabet** (`:193-207`): each of `B`'s transition
///   symbols is decoded under `B`'s own alphabet and re-encoded under `A`'s, and `B`'s
///   alphabet/encoder/alphabet-size/number-systems are then replaced by `A`'s
///   wholesale. Since symbols are alphabet-relative *positions* (see `automaton.rs`'s
///   module docs), skipping this would silently reinterpret `B`'s digits.
/// * **Each `A from i` is materialized by cloning `A` and moving `q0`** (`:209-216`),
///   with `forceCanonize()` for `i != 0` (which BFS-renumbers and, as a side effect,
///   drops states unreachable from the new `q0`). Only the `q0` move is observable:
///   canonicalization is language-preserving and the cross product's own BFS already
///   visits reachable pairs only, so deleting the `forceCanonize()` call cannot change
///   any answer this crate can compute (verified by mutation-testing the tests below —
///   it is the one edit here that survives, and it survives for a real reason, not a
///   test gap). Ported anyway, per `CLAUDE.md`'s mechanical-port rule.
///
/// `skip_subset_check` bypasses the "`B`'s alphabet ⊆ `A`'s alphabet" guard
/// (`:180-185`); [`left_quotient`] passes `true` — see its doc comment for the genuine
/// Walnut defect that hides behind that.
///
/// # Panics
///
/// If `!skip_subset_check` and `B`'s alphabet is not a subset of `A`'s
/// (`WalnutException` message ported verbatim).
///
/// `docs/WALNUT-BUGS.md` WB-001 used to be reachable through the closing
/// `M.determinizeAndMinimize()` (`:227`), since `M` inherits `A`'s (possibly
/// not-fully-`q0`-reachable) state set unchanged. It is fixed (`walnut-java` commit
/// `14509f1`, ported in [`crate::minimize::minimize`]); the result can still be
/// non-minimal on such an input, but is language-correct.
pub fn right_quotient(
    a: &Automaton,
    b: &Automaton,
    skip_subset_check: bool,
    logging: &mut crate::logging::Logging,
) -> Automaton {
    // `logMessage("right quotient: " + …)` (`:178`) -- no `indent()`/`dedent()` bracket of
    // its own; `M.determinizeAndMinimize()`'s bracket below is the only nesting.
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "right quotient: {} state A with {} state A",
        a.fa.q, b.fa.q
    ));
    if !skip_subset_check {
        assert!(
            is_subset_alphabet(&b.alphabet, &a.alphabet),
            "Second A's alphabet must be a subset of the first A's alphabet for right quotient."
        );
    }

    // The returned automaton has the same states and transition function as `a`; only
    // the final states differ.
    let mut m = a.clone();

    let mut other_clone = b.clone();
    let re_encoded: Vec<BTreeMap<i32, Vec<usize>>> = other_clone
        .fa
        .d
        .iter()
        .map(|row| {
            row.iter()
                .map(|(&sym, dests)| (a.encode(&other_clone.decode(sym)), dests.clone()))
                .collect()
        })
        .collect();
    other_clone.fa.d = re_encoded;
    other_clone.alphabet = a.alphabet.clone();
    other_clone.setup_encoder();
    other_clone.fa.alphabet_size = a.fa.alphabet_size;
    // `otherClone.setNS(A.getNS())` (`:206`) — all three parts of the per-track stand-in.
    other_clone.msd = a.msd.clone();
    other_clone.set_all_reps(a.all_reps.clone());
    other_clone.set_ns_names(a.ns_name.clone());

    for i in 0..a.fa.q {
        // A temporary automaton identical to `a` except that it starts from state `i`.
        let mut t = a.clone();
        if i != 0 {
            t.fa.q0 = i;
            t.force_canonize();
        }

        // The cross product (including `and`) needs both operands labeled, with
        // matching labels.
        t.random_label();
        other_clone.label = t.label.clone();

        let intersection = and(&t, &other_clone, logging);
        m.fa.o[i] = i32::from(!intersection.automaton().is_empty());
    }

    m.determinize_and_minimize_with_ctx(None, logging);
    // `M.applyAllRepresentations()` (`:228`), between `determinizeAndMinimize` and
    // `forceCanonize` — live as of U5. Needed here for the same reason as in `not`: the
    // accepting set was recomputed from scratch above (`setOutputIfEqual`), with no regard
    // for whether each state is reachable by a VALID representation.
    m.apply_all_representations(logging);
    m.force_canonize();
    // `logMessage("right quotient complete: " + … )` (`:232`).
    logging.log_message(&format!(
        "right quotient complete: {} states - {}ms",
        m.fa.q,
        time_before.elapsed().as_millis()
    ));
    m
}

/// `AutomatonLogicalOps.leftQuotient` (`:237-256`) — `L(B) \ L(A) = { z : ∃ w ∈ L(B),
/// wz ∈ L(A) }`, built by reversing both operands, taking a [`right_quotient`], and
/// reversing back.
///
/// # Panics
///
/// If `B`'s alphabet is not a subset of `A`'s (`:242-244`, `WalnutException` message
/// ported verbatim).
///
/// # WB-010 (`docs/WALNUT-BUGS.md`) — the alphabet-subset guard used to run backwards (fixed)
///
/// The guard above now checks `isSubsetA(B, A)` — "`B` ⊆ `A`". **Before the fix**
/// (upstream `walnut-java` commit `c5ff914` on `bugfix/wb-010`) it checked
/// `isSubsetA(A, B)` instead — the OPPOSITE of what the delegated
/// `rightQuotient(M1, M2, skipSubsetCheck=true)` call (`:248`) actually needs: that call
/// re-encodes `M2`'s (= `B`'s) symbols under `M1`'s (= `A`'s) alphabet
/// (`RichAlphabet.encode`, `:193-207`), which is only safe when `B`'s alphabet ⊆ `A`'s —
/// exactly the direction `rightQuotient`'s own guard (`:182`) checks, and exactly the
/// guard `skipSubsetCheck = true` disables. The two directions coincide only when the
/// alphabets are equal as sets.
///
/// The old, backwards guard had two symptoms, both now fixed: `leftquo` on `A` over
/// `{0,1}` and `B` over `{0,1,2}` used to pass the check, then hand `RichAlphabet.encode`
/// (`RichAlphabet.java:110-116`) a digit its `A.get(i).indexOf(...)` cannot find,
/// yielding `-1` and a silently corrupt (possibly negative) symbol id in Java (this
/// crate turned it into a clean [`Automaton::encode`] panic instead, a pre-existing
/// improvement over Java's silent `indexOf == -1` — see `automaton.rs`'s doc comment on
/// `encode`); and, in the other direction, a genuinely valid pair like `A` over
/// `{0,1,2}`, `B` over `{0,1}` (where `B ⊆ A` holds but the alphabets aren't equal as
/// sets) used to be wrongly REJECTED. Reachable from the plain CLI either way:
/// `Main/Commands/Quotient.java:17-23` reads both automata from `.txt` files with no
/// alphabet normalization in between.
///
/// Fixed to match Java's fix (`c5ff914`) exactly: `is_subset_alphabet(&b.alphabet,
/// &a.alphabet)` instead of `is_subset_alphabet(&a.alphabet, &b.alphabet)`, with the
/// corresponding message.
pub fn left_quotient(
    a: &Automaton,
    b: &Automaton,
    logging: &mut crate::logging::Logging,
) -> Automaton {
    // `logMessage("left quotient: " + …)` (`:239`) -- logged BEFORE the subset-alphabet
    // check below, matching Java's exact line order (a failing check still logs this
    // line, with no matching "complete" partner).
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "left quotient: {} state A with {} state A",
        a.fa.q, b.fa.q
    ));
    assert!(
        is_subset_alphabet(&b.alphabet, &a.alphabet),
        "Second A's alphabet must be a subset of the first A's alphabet for left quotient."
    );

    let m1 = reverse_and_canonize(a, logging);
    let m2 = reverse_and_canonize(b, logging);
    let mut m = right_quotient(&m1, &m2, true, logging);

    reverse_with_ctx(&mut m, true, None, logging);
    // `logMessage("left quotient complete: " + …)` (`:253`).
    logging.log_message(&format!(
        "left quotient complete: {} states - {}ms",
        m.fa.q,
        time_before.elapsed().as_millis()
    ));
    m
}

/// `AutomatonLogicalOps.reverseAndCanonize` (`:258-263`, `private`). Note the `true`:
/// each reversal here also flips the operands' msd/lsd flags, which is why
/// [`left_quotient`]'s closing `reverse(M, true)` restores the original direction
/// rather than inverting it.
fn reverse_and_canonize(a: &Automaton, logging: &mut crate::logging::Logging) -> Automaton {
    let mut m1 = a.clone();
    reverse_with_ctx(&mut m1, true, None, logging);
    m1.force_canonize();
    m1
}

// ---------------------------------------------------------------------------
// Leading/trailing-zero fixups and reverse.
// ---------------------------------------------------------------------------

/// `AutomatonLogicalOps.fixLeadingZerosProblem` (`:268-283`) — "make `A` accept `0*x`
/// iff it used to accept `x`".
///
/// Re-runs subset construction from the *set* of states reachable from `q0` by reading
/// the all-zero symbol zero-or-more times, instead of from `{q0}`
/// (`Automaton.determinizeAndMinimize(IntSet)`, `:278`). Since U6 this is also the
/// ∃-projection pipeline's fixup step, called from [`crate::quantify::quantify`] — see
/// this module's docs on the duplicate it replaced.
///
/// The trivial-automaton guard below IS Java's (`:269`, added by U0). The `fa.q == 0`
/// guard behind it has no Java counterpart: Java would dereference `q0`'s (nonexistent)
/// transition row inside `zeroReachableStates` and throw `IndexOutOfBoundsException`,
/// but a real Walnut `Automaton` never reaches this method with zero states AND the
/// flag unset (only the TRUE/FALSE automata are zero-state). This crate's `Automaton`
/// *can* express that flagless shape, so the degenerate case is a no-op here — matching
/// the identical guard, added for the identical reason, in `crate::quantify`'s private
/// `quantify_helper` (at its `a.fa.q == 0` early return).
pub fn fix_leading_zeros_problem(a: &mut Automaton) {
    fix_leading_zeros_problem_with_ctx(a, None, &mut crate::logging::Logging::new());
}

/// [`fix_leading_zeros_problem`] with an explicit
/// [`crate::determinize::DeterminizeContext`] — Walnut's `[strategy …]`/`[export …]`
/// metacommand state; see
/// [`crate::automaton::Automaton::determinize_and_minimize_with_ctx`] for the contract
/// (including the caller-owed `shouldPrintDetails()` gate).
///
/// This fixup's closing `determinizeAndMinimize(IntSet)` is **unconditional**, so with
/// `Some(ctx)` it always consumes one automata index — the `Determinizing [#n, …]` line
/// Walnut prints inside every `fixing leading zeros:` block of a `details` fixture.
///
/// `FIXING leading zeros:{q} states` is logged BEFORE the trivial-automaton guard's
/// early return has any chance to matter (the guard already returned above it), then
/// `indent()`/`dedent()` bracket the actual fixup work, then `FIXED leading zeros:{q}
/// states - {ms}ms` (`AutomatonLogicalOps.java:268-282`).
pub fn fix_leading_zeros_problem_with_ctx(
    a: &mut Automaton,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) {
    // `if (A.fa.isTRUE_FALSE_AUTOMATON()) return;` (`:269`, U0). Load-bearing, not
    // cosmetic: `determine_zero()` on a trivial automaton's empty alphabet returns 0,
    // and `zero_reachable_states` would then index `fa.d[fa.q0]` — out of bounds for
    // both trivial shapes.
    if a.fa.is_true_false_automaton() {
        return;
    }
    if a.fa.q == 0 {
        return;
    }
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} leading zeros:{} states",
        crate::logging::FIXING,
        a.fa.q
    ));
    logging.indent();
    // `A.fa.setCanonized(false);` (`:273`) -- see `Automaton::canonized`'s doc comment
    // for why this port has to do it explicitly.
    a.set_canonized(false);
    let zero = a.determine_zero();
    let initial_state = zero_reachable_states(&mut a.fa, zero);
    a.determinize_and_minimize_from_with_ctx(&initial_state, ctx, logging);
    logging.dedent();
    logging.log_message(&format!(
        "{} leading zeros:{} states - {}ms",
        crate::logging::FIXED,
        a.fa.q,
        time_before.elapsed().as_millis()
    ));
}

/// `AutomatonLogicalOps.zeroReachableStates` (`:289-316`, `private`) — the states
/// reachable from `q0` by reading `zero*`.
///
/// **This mutates `fa`, and the mutation is load-bearing, not bookkeeping.** Before the
/// BFS, Java force-adds `q0` to `q0`'s own `zero`-destination list if it is not already
/// there (`:292-295`) — creating a real `(q0, zero) -> q0` self-loop in the transition
/// table. It cannot change the returned set (`q0` is in it unconditionally), but it
/// persists into the caller's next step, subset construction, which is what makes "one
/// more leading zero from the very start is a no-op" a structural property of the
/// resulting automaton. Replicating only the returned set would silently drop that.
///
/// Note Java's `if (result.add(q))` guard (`:304`): a state is expanded only the first
/// time it is popped.
///
/// **Visibility note.** This is `private` in Java and has exactly one production caller
/// here ([`fix_leading_zeros_problem`]). It is `pub` only so that the Phase-1
/// regression tests written against it — which live in `wr_logic::quantify`'s test
/// module, alongside the ∃-projection tests they were written to protect, and which U6
/// deliberately left in place unchanged — can still reach it. Treat it as an internal
/// detail, not intended public surface.
pub fn zero_reachable_states(fa: &mut Fa, zero: i32) -> BTreeSet<usize> {
    let q0 = fa.q0;
    let dests = fa.d[q0].entry(zero).or_default();
    if !dests.contains(&q0) {
        dests.push(q0);
    }

    let mut result: BTreeSet<usize> = BTreeSet::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(q0);
    while let Some(q) = queue.pop_front() {
        if result.insert(q) {
            if let Some(transitions) = fa.d[q].get(&zero) {
                for &p in transitions {
                    if !result.contains(&p) {
                        queue.push_back(p);
                    }
                }
            }
        }
    }
    result
}

/// `AutomatonLogicalOps.fixTrailingZerosProblem` (`:321-335`) — "make `A` accept `x0*`
/// iff it used to accept `x`".
///
/// Genuinely different machinery from [`fix_leading_zeros_problem`], not its mirror
/// image: it only widens the *accepting set* (every state that can reach an accepting
/// state by reading `zero*` becomes accepting) and therefore re-minimizes **without**
/// re-determinizing — Java's own comment at `:327`, "We don't have to determinize,
/// since all that was altered was final states". The `justMinimize` step runs only when
/// [`set_states_reachable_to_final_states_by_zeros`] reports an actual change (`:322`);
/// otherwise this is a complete no-op, and in particular does NOT minimize.
///
/// # No TRUE/FALSE short-circuit — faithfully, unlike its leading-zeros sibling
///
/// Java gives this method NO `isTRUE_FALSE_AUTOMATON()` guard, and U0 does not invent
/// one. That asymmetry with `fixLeadingZerosProblem` (`:269`) is real but harmless:
/// its only two callers are `AutomatonQuantification.quantify` (`:46`), which already
/// returned at `:39` if the automaton became trivial, and `Prover`'s `fixtrailzero`
/// command, which reads its operand from a `.txt` file — and a trivial file yields the
/// `Q == 0` shape, on which both engines are a silent no-op (Java's loops don't run;
/// neither do this port's). The only shape that WOULD fault (trivial with a stale
/// non-zero `q`) exists solely inside `quantify`, behind that guard. So the missing
/// guard is unreachable in both engines and is left unported rather than "fixed".
/// Confirmed live during U0 against `Walnut-all.jar`: `fixtrailzero fz $t` on a `true`
/// automaton writes `true` back out, no exception (as do `fixleadzero` and `reverse`).
pub fn fix_trailing_zeros_problem(a: &mut Automaton, logging: &mut crate::logging::Logging) {
    let zero = a.determine_zero();
    // `if (A.fa.setStatesReachableToFinalStatesByZeros(zero))` (`:321`) -- the two
    // branches log genuinely DIFFERENT text, not "log the pair, or skip it": the
    // no-op branch gets its own single, partner-less line (`:333`).
    if set_states_reachable_to_final_states_by_zeros(&mut a.fa, zero) {
        let time_before = std::time::Instant::now();
        logging.log_message(&format!(
            "{} trailing zeros:{} states",
            crate::logging::FIXING,
            a.fa.q
        ));
        logging.indent();
        // `A.fa.setCanonized(false);` (`:326`), plus `justMinimize`'s own reset
        // (`FA.java:584`) -- one assignment covers both.
        a.set_canonized(false);
        a.fa = just_minimize(&a.fa, logging);
        logging.dedent();
        logging.log_message(&format!(
            "{} trailing zeros:{} states - {}ms",
            crate::logging::FIXED,
            a.fa.q,
            time_before.elapsed().as_millis()
        ));
    } else {
        logging.log_message(&format!(
            "{} trailing zeros: no change necessary.",
            crate::logging::FIXING
        ));
    }
}

/// Every failure [`remove_leading_zeros`] can report.
///
/// Both variants are `WalnutException`s Java throws from the same two methods; neither is
/// reachable from a well-formed `Automaton` reached through the `I` quantifier's own call
/// path (see each variant's docs), but both are surfaced as `Err` rather than `panic!` per
/// `PORTING.md`'s error-mapping rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveLeadingZerosError {
    /// `WalnutException.notFreeVariable(String)`, thrown by
    /// `AutomatonQuantification.validateLabels` (`AutomatonQuantification.java:110-116`)
    /// — the very first statement of `removeLeadingZeros` (`:344`). Same exception, same
    /// trigger and same message as [`crate::quantify::QuantifyError::NotFreeVariable`].
    NotFreeVariable(String),
    /// `removeLeadingZerosHelper`'s own guard (`:376-379`): "Cannot remove leading zeros
    /// for the `n+1`-th input when A only has `inputs` inputs."
    ///
    /// Unreachable through [`remove_leading_zeros`], which is `removeLeadingZerosHelper`'s
    /// only caller in either engine: `n` comes from `A.getLabel().indexOf(l)` for a label
    /// the validation above has already proven present, so `0 <= n < label.len()`, and
    /// `label.len() == alphabet.len()` for any well-formed `Automaton`. Ported anyway
    /// (Java's guard is likewise unreachable-but-present), and it does fire here for a
    /// hand-built `Automaton` whose `label` is longer than its `alphabet`.
    InputIndexOutOfRange { n: usize, inputs: usize },
}

impl fmt::Display for RemoveLeadingZerosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoveLeadingZerosError::NotFreeVariable(s) => write!(
                f,
                "Variable {s} in the list of quantified variables is not a free variable."
            ),
            RemoveLeadingZerosError::InputIndexOutOfRange { n, inputs } => write!(
                f,
                "Cannot remove leading zeros for the {}-th input when A only has {inputs} inputs.",
                n + 1
            ),
        }
    }
}

impl std::error::Error for RemoveLeadingZerosError {}

/// `AutomatonLogicalOps.removeLeadingZeros(Automaton, List<String>)` (`:343-367`) — the
/// `I` (infinite) quantifier's pre-pass, and its only production caller inside the ported
/// subset (`LogicalOperator.java:151`).
///
/// For each named track: build an automaton that requires that track's **first** symbol to
/// be non-zero (msd) or its **last** to be non-zero (lsd), OR all of those together, and
/// intersect the result with `a`. Java's own doc comment states the intent; note the
/// disjunction, not conjunction — "at least one of the quantified inputs is not
/// zero-padded" — which is what makes `I` count *distinct values* rather than distinct
/// zero-padded encodings of them.
///
/// Returns a clone of `a` untouched when `list_of_labels` is empty (`:345-347`), **after**
/// the label validation, which is therefore vacuous in exactly that case.
///
/// `list_of_labels` is a slice, not a set: Java passes `LogicalOperator`'s raw
/// `List<String>` here (unlike `AutomatonQuantification.quantify`, which converts to a
/// `HashSet` first), so a repeated label really does build and OR in the same helper
/// automaton twice. Harmless (`or` is idempotent) and ported as-is rather than
/// deduplicated.
pub fn remove_leading_zeros(
    a: &Automaton,
    list_of_labels: &[String],
) -> Result<Automaton, RemoveLeadingZerosError> {
    remove_leading_zeros_with_ctx(a, list_of_labels, None, &mut crate::logging::Logging::new())
}

/// [`remove_leading_zeros`] with an explicit
/// [`crate::determinize::DeterminizeContext`] — see
/// [`crate::automaton::Automaton::determinize_and_minimize_with_ctx`] for the contract.
///
/// This reaches the dispatcher only through [`remove_leading_zeros_helper`]'s closing
/// `reverse(M, false)`, i.e. **once per LSD track named in `list_of_labels`** and not at
/// all for an msd one. Java gives that `reverse` no `Logging.disablePrint()` bracket
/// (unlike everything `NumberSystem` builds internally), so it really does advance
/// Walnut's automata counter.
pub fn remove_leading_zeros_with_ctx(
    a: &Automaton,
    list_of_labels: &[String],
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) -> Result<Automaton, RemoveLeadingZerosError> {
    // `AutomatonQuantification.validateLabels(A, listOfLabels)` (`:344`). Four lines
    // rather than a call into `crate::quantify`: that module inlines the identical check
    // inside its private `quantify_helper`, where it sits *after* an early return this
    // method does not have (Java's `quantify` short-circuits on an empty label list before
    // validating; `removeLeadingZeros` validates first). Keeping the two copies separate
    // preserves that ordering difference, which is observable — `remove_leading_zeros(a,
    // &["nope"])` is an error while `quantify(a, &{})` is not.
    for s in list_of_labels {
        if !a.label.contains(s) {
            return Err(RemoveLeadingZerosError::NotFreeVariable(s.clone()));
        }
    }
    if list_of_labels.is_empty() {
        return Ok(a.clone());
    }

    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} leading zeros for:{} states",
        crate::logging::REMOVING,
        a.fa.q
    ));
    logging.indent();

    // `A.getLabel().indexOf(l)` (`:352-354`). `indexOf` takes the FIRST occurrence, so a
    // label repeated across two tracks constrains only the earlier one — the same
    // first-occurrence quirk `crate::quantify` documents.
    let list_of_inputs: Vec<usize> = list_of_labels
        .iter()
        .map(|l| {
            a.label
                .iter()
                .position(|x| x == l)
                .expect("validated above")
        })
        .collect();

    // `Automaton M = new Automaton(false);` (`:356`) — the FALSE automaton as the fold's
    // identity, expressible only since U0. `or(FALSE, N)` short-circuits to `N`, so the
    // first iteration costs nothing.
    let mut m = Automaton::true_false(false);
    let mut ctx = ctx;
    for n in list_of_inputs {
        let mut helper = remove_leading_zeros_helper(a, n, ctx.as_deref_mut(), logging)?;
        m = or(&mut m, &mut helper, logging).into_automaton();
    }
    // `M = and(A, M);` (`:361`) — note the argument order, and that `and` never mutates
    // (see its docs), so the caller's `a` survives this call unchanged.
    let result = and(a, &m, logging).into_automaton();

    logging.dedent();
    // `REMOVED + ":" + A.fa.getQ() + " states - " + ms` (`:363-364`) -- Java's quirk,
    // ported verbatim: this reports `A`'s (the ORIGINAL, unmutated input's) state count,
    // not the result's.
    logging.log_message(&format!(
        "{}:{} states - {}ms",
        crate::logging::REMOVED,
        a.fa.q,
        time_before.elapsed().as_millis()
    ));
    Ok(result)
}

/// `AutomatonLogicalOps.removeLeadingZerosHelper(Automaton, int n)` (`:375-405`,
/// `private`) — "the `n`-th input does not start (msd) / end (lsd) with a zero".
///
/// # The two-state automaton, and why BOTH states accept
///
/// `initBasicFA(IntList.of(1, 1))` (`:385`) builds `Q = 2`, `q0 = 0`, outputs `[1, 1]`.
/// State 0 steps to state 1 on exactly the symbols whose `n`-th digit is non-zero; state 1
/// self-loops on **every** symbol. So the accepted language is `{ε} ∪ {w : w[0][n] ≠ 0}`.
///
/// The `ε` looks like an oversight (state 0's own output is `1`, so the empty word is
/// accepted even though it has no non-zero leading digit) but is best read as deliberate:
/// `ε` is precisely the leading-zero-free representation of the value `0`, whose only other
/// encodings (`0`, `00`, …) this automaton is built to reject. Java's other caller of
/// `removeLeadingZeros`, `Main/Commands/Test.java`'s accepted-word enumerator (`:43`), is
/// where that matters — it lists one representation per value, and dropping `ε` would drop
/// `0` from the listing. Either way it is not observable through the `I` quantifier:
/// `Infinite::infinite` asks whether the language is *infinite*, which one extra word cannot
/// change. Ported verbatim.
///
/// For an lsd track the whole automaton is reversed at the end (`:402-404`, `reverse(M,
/// false)` — language reversal only, no msd/lsd flip), which turns "first symbol non-zero"
/// into "last symbol non-zero".
///
/// # `A.getNS().get(n) == null -> new AutomatonDFA(true)` (`:381-383`)
///
/// A track with no numeration system has no notion of a leading zero, so it imposes no
/// constraint. In this crate that is `a.msd[n].is_none()`.
fn remove_leading_zeros_helper(
    a: &Automaton,
    n: usize,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) -> Result<Automaton, RemoveLeadingZerosError> {
    // `if (n >= A.richAlphabet.getA().size() || n < 0)` (`:376`). The `n < 0` half is
    // unrepresentable for a `usize`.
    if n >= a.alphabet.len() {
        return Err(RemoveLeadingZerosError::InputIndexOutOfRange {
            n,
            inputs: a.alphabet.len(),
        });
    }

    let msd = match a.msd[n] {
        Some(msd) => msd,
        None => {
            // `new AutomatonDFA(true)` (`AutomatonLogicalOps.java:381-383`) — DFA-typed
            // by construction, unlike the plain `Automaton::true_false` this crate
            // otherwise uses for Java's `new Automaton(boolean)`. Unobservable today (a
            // fresh trivial value has `q == 0` either way, and the flag reconverges at
            // the first `as_dfa` downstream), fixed for consistency with
            // `Automaton::dfa_typed`'s docs, per adversarial review.
            let mut m = Automaton::true_false(true);
            m.dfa_typed = true;
            return Ok(m);
        }
    };

    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(), BTreeMap::new()];
    for i in 0..a.fa.alphabet_size as i32 {
        let digits = a.decode(i);
        if digits[n] != 0 {
            d[0].insert(i, vec![1]);
        }
        d[1].insert(i, vec![1]);
    }

    let mut m = Automaton::new(
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: a.fa.alphabet_size,
            o: vec![1, 1],
            d,
        },
        a.alphabet.clone(),
        a.label.clone(),
        a.msd.clone(),
    );
    // `M.setNS(A.getNS())` (`:387`) shares the whole `NumberSystem` list, i.e. both halves
    // of this crate's NS stand-in — the msd flags handed to `Automaton::new` above AND the
    // all-representations restriction, which `or`/`xor`/`imply`/`iff` re-apply after
    // totalizing (`and` never totalizes, so it never needs to).
    m.set_all_reps(a.all_reps.clone());
    m.set_ns_names(a.ns_name.clone());

    // `if (!A.getNS().get(n).isMsd()) reverse(M, false);` (`:402-404`).
    if !msd {
        reverse_with_ctx(&mut m, false, ctx, logging);
    }
    Ok(m)
}

/// `AutomatonLogicalOps.reverse(Automaton, boolean reverseMsd)` (`:414-430`) — replace
/// `L(A)` by its reversal.
///
/// `reverse_msd` does **not** control the reversal itself (which always happens); it
/// controls the single extra line at `:423-425`, `NumberSystem.flipNS(A.getNS())` —
/// i.e. whether each track's numeration direction is also flipped from msd to lsd and
/// back. Java's own doc comment (`:409`) states it that way, and reading the body
/// confirms it: `reverseToNFAInternal` + `determinizeAndMinimize` (`:420-421`) run
/// unconditionally. The distinction is real — `NumberSystem.java` calls this with
/// `false` (`:315`, `:333`, `:378`, `:459`) when it wants the reversed *language* of an
/// automaton whose declared number system must stay put, while `Main/Commands/Reverse.java:15`
/// and `LogicalOperator.java:108` pass `true`.
///
/// Java's `reverseToNFAInternal(IntSet.of(A.fa.getQ0()))` returns the *set* of new
/// initial states (the old accepting states) — see [`Fa::reverse`]'s doc comment,
/// including its warning that `self.q0` is stale afterwards. The immediately-following
/// `determinizeAndMinimize(setOfFinalStates)` is the only supported next step, and is
/// what re-establishes a valid `q0`.
///
/// The result is a DFA (Java's `:412` note: "the output of this is a DFA"), even though
/// the input may be an NFA.
pub fn reverse(a: &mut Automaton, reverse_msd: bool) {
    reverse_with_ctx(a, reverse_msd, None, &mut crate::logging::Logging::new());
}

/// [`reverse`] with an explicit [`crate::determinize::DeterminizeContext`] — see
/// [`crate::automaton::Automaton::determinize_and_minimize_with_ctx`] for the contract.
/// The closing `determinizeAndMinimize(setOfFinalStates)` is unconditional, so with
/// `Some(ctx)` this always consumes one automata index.
pub fn reverse_with_ctx(
    a: &mut Automaton,
    reverse_msd: bool,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) {
    // `if (A.fa.isTRUE_FALSE_AUTOMATON()) return;` (`:415`, U0). Note this returns
    // BEFORE the `flipNS` step, so reversing a trivial automaton does not flip its
    // (empty) msd list either — faithful, and vacuous since the list is empty. Also
    // BEFORE the logging pair below, matching Java exactly (a trivial reverse logs
    // nothing).
    if a.fa.is_true_false_automaton() {
        return;
    }
    let time_before = std::time::Instant::now();
    logging.log_message(&format!("{}:{} states", crate::logging::REVERSING, a.fa.q));
    logging.indent();
    let initial: BTreeSet<usize> = [a.fa.q0].into_iter().collect();
    let set_of_final_states = a.fa.reverse(&initial);
    a.determinize_and_minimize_from_with_ctx(&set_of_final_states, ctx, logging);

    if reverse_msd {
        flip_ns(a);
    }
    // `Logging.dedent()` sits AFTER `flipNS` (`:427`), not immediately after
    // `determinizeAndMinimize` — ported in that exact order.
    logging.dedent();
    logging.log_message(&format!(
        "{}:{} states - {}ms",
        crate::logging::REVERSED,
        a.fa.q,
        time_before.elapsed().as_millis()
    ));
}

/// `AutomatonLogicalOps.removeStatesWithOutputRebuild(FA, int minOutput)` (`:436-448`).
/// Package-private in Java (callers: `Transducer.java:321`, `WordAutomaton.java:185` —
/// both in the same `Automata` package, which corresponds to this whole crate), so
/// `pub` here.
///
/// # The javadoc oversells this, faithfully reproduced
///
/// Java's doc comment claims it "deletes all states whose output equals the given
/// value, remaps remaining states, and preserves only transitions among kept states".
/// The body does none of that: `Q` and `O` are untouched, nothing is renumbered, and
/// the pruning is *per transition entry*, keyed on the FIRST destination only —
/// `removeIf(entry -> !entry.getValue().isEmpty() && statesToRemove.contains(entry.getValue().getInt(0)))`
/// (`:445-446`). So an entry whose first destination survives is kept in full even if
/// its other destinations are all removable, and vice versa. This mismatch is a
/// documented Phase-0 finding (`docs/WALNUT-BUGS.md`'s "dead code /
/// doc-vs-implementation mismatches" section) and is pinned by
/// `remove_states_with_output_rebuild_matches_the_java_characterization_test` below,
/// which replicates `AutomatonLogicalOpsTest.testRemoveStatesWithOutputRebuild`
/// including its explicit "despite the javadoc" assertions.
pub fn remove_states_with_output_rebuild(fa: &mut Fa, min_output: i32) {
    let states_to_remove: HashSet<usize> = (0..fa.q).filter(|&q| fa.o[q] == min_output).collect();
    for q in 0..fa.q {
        fa.d[q].retain(|_sym, dests| !(!dests.is_empty() && states_to_remove.contains(&dests[0])));
    }
}

/// `AutomatonLogicalOps.combine(Automaton A, Queue<Automaton> subautomata, IntList outputs)`
/// (`:679-722`) — folds `A` and each of `subautomata` together into one DFAO whose
/// output at a state is `outputs[i]` where `i` is the index (into `outputs`, `A`
/// counting as index `0`) of the first operand that accepts there, or the running
/// combined output otherwise. Added in Phase 3a's U6 as a hard, previously-missing
/// dependency of `WordAutomaton::minimize_with_output` (`WordAutomaton.java:216-229`
/// calls it directly) — not part of U6's own file, but this file (`AutomatonLogicalOps`)
/// is `combine`'s real Java home, and `Main/Commands/Combine.java`'s later `combine`
/// command (Phase 3b, batch A) should call this directly rather than re-porting it.
///
/// # `combineIndex`/`combineOutputs` not ported as `Automaton` fields
///
/// Java threads the running output value through two fields on `Automaton`
/// (`combineIndex`/`combineOutputs`), read back inside `ProductStrategies.
/// determineOutput`'s `Prover.COMBINE` arm via `Automaton.determineCombineOutVal`
/// (`Automaton.java:72-74`) — architecture needed because Java's `crossProduct` takes
/// a `String op`, not a closure. `wr-core`'s [`crate::product::cross_product`] is
/// already generic over the combining function (`docs/BOUNDARY-MAP.md` §3a), so the
/// per-step combine value is captured directly into a closure below instead; no
/// `Automaton`-level state is needed, and none is added.
///
/// # Totalization via the free [`totalize`] helper, not [`Fa::totalize`]
///
/// Matches [`totalize_cross_product`]'s own reasoning (see this module's docs): Java's
/// `FA.totalize()` doesn't require determinism, and while `first`/`next` are
/// deterministic in every real call here (both operands always already went through
/// `determinizeAndMinimize`, and [`crate::product::cross_product`] over two
/// deterministic inputs is itself deterministic — the same injectivity argument
/// `crate::product`'s own docs give), using the assert-free helper avoids relying on
/// that invariant staying true if a future caller ever violates it.
pub fn combine(
    a: &Automaton,
    subautomata: Vec<Automaton>,
    outputs: &[i32],
    logging: &mut crate::logging::Logging,
) -> Automaton {
    let mut first = a.clone();
    for q in 0..first.fa.q {
        if first.fa.is_accepting(q) {
            first.fa.o[q] = outputs[0];
        }
    }
    // `outputs[0]` is consumed above (for `A`'s own accepting-state rewrite); each
    // subsequent element pairs with `subautomata` in order (Java: `first.combineIndex`
    // starts at `1` and increments once per loop iteration).
    //
    // `logMessage(COMPUTING + " =>:" + first.Q + " states - " + next.Q + " states")`,
    // `indent()`, [the fold step], `dedent()`, `logMessage(COMPUTED ...)`
    // (`AutomatonLogicalOps.java:692-710`) -- once per loop iteration, not once overall.
    for (combine_out, mut next) in outputs[1..].iter().copied().zip(subautomata) {
        let time_before = std::time::Instant::now();
        logging.log_message(&format!(
            "{} =>:{} states - {} states",
            crate::logging::COMPUTING,
            first.fa.q,
            next.fa.q
        ));
        logging.indent();
        first.random_label();
        next.label = first.label.clone();
        totalize(&mut first.fa, logging);
        totalize(&mut next.fa, logging);
        let product = cross_product(
            &first,
            &next,
            |a_out, b_out| {
                if b_out == 1 {
                    combine_out
                } else {
                    a_out
                }
            },
            logging,
        );
        first = product;
        logging.dedent();
        logging.log_message(&format!(
            "{} =>:{} states - {}ms",
            crate::logging::COMPUTED,
            first.fa.q,
            time_before.elapsed().as_millis()
        ));
    }
    // `Logging.indent()` (`:714`) brackets the closing totalize/forceCanonize/
    // applyAllRepresentationsWithOutput trio, with no logMessage of its own around it.
    logging.indent();
    totalize(&mut first.fa, logging);
    first.force_canonize();
    first.apply_all_representations_with_output(logging);
    logging.dedent();
    first
}

// ---------------------------------------------------------------------------
// Number-system conversion: `convertNS` and its five private helpers (U18).
// ---------------------------------------------------------------------------

/// Every failure [`convert_ns`] can report. Most variants — including
/// [`ConvertNsError::NoNumberSystem`], which used to stand in for a genuine Java
/// `NullPointerException` before it was fixed upstream (see its own docs and
/// `docs/WALNUT-BUGS.md` WB-033) — are real `WalnutException`s Java throws from
/// `convertNS`/`convertMsdBaseToExponent`/`convertLsdBaseToRoot`, with their messages
/// preserved verbatim in the [`fmt::Display`] impl (each one checked against the real
/// `walnut-java` CLI, not transcribed from source).
///
/// **Exactly three are NOT `WalnutException`s**, and each says so in its own doc:
/// [`ConvertNsError::BaseOverflowsInt`] (an uncaught `java.lang.NumberFormatException`
/// out of `Integer.parseInt`), and [`ConvertNsError::InvalidRoot`] /
/// [`ConvertNsError::NotAnExactPower`] (`java.lang.IllegalArgumentException`, from
/// WB-032's fix to `UtilityMethods.exactIntegerExponent`). All three are classified
/// `is_handled() == false` in `wr_cli::prover`, with a matching `kind()` arm, so they
/// render Java's kind-prefixed stderr line rather than a plain stdout message — and all
/// three are unreachable through `convert_ns`'s real call sites today (each variant's doc
/// says why), classified for fidelity regardless, per this crate's standing practice for
/// defensive-only ported guards. Blanket-claiming "every variant is a `WalnutException`"
/// here was wrong, and directly contradicted `prover.rs`'s own arms; corrected after
/// adversarial review.
///
/// `Result` rather than `panic!`, per `PORTING.md`'s exception-mapping rule and for the
/// same reason WB-013's entry spells out: every one of these is reachable from raw
/// user-typed CLI input (`convert` reads its operand from a `.txt` file and its target
/// number system from the command line), and Java recovers from all of them —
/// `Prover.dispatch`'s top-level `catch` prints the message and the session continues. An
/// uncaught Rust `panic!` would kill the process instead, which is *less* faithful, not
/// more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertNsError {
    /// `convertNS`'s `A.getNS().size() != 1` guard (`:456-458`). Also fires for a
    /// TRUE/FALSE automaton, which has no tracks at all — matching Java, whose trivial
    /// automata carry an empty `NS` list.
    NotSingleInput,
    /// The track exists but has **no numeration system** — Java's `A.getNS().get(0)` used
    /// to be dereferenced unguarded by `ns.parseBase()` (`:462`); as of `walnut-java`
    /// commit `c75e630` (branch `bugfix/wb-013-033-034`) it instead passes through the
    /// shared `NumberSystem.requireNumberSystem(ns, subject)` helper, which throws a real,
    /// diagnosable `WalnutException` naming the automaton.
    ///
    /// Reachable from the plain CLI on a perfectly valid input: an automaton `.txt` whose
    /// alphabet is declared explicitly (`{0,1}`) rather than as `msd_k`/`lsd_k` gets a
    /// literal `null` NS entry from `ParseMethods.parseAlphabetDeclaration`
    /// (`Automata/ParseMethods.java:91-96`, `bases.add(null)`), and `convert`ing it now
    /// prints `"the automaton being converted has no attached number system (its alphabet
    /// was declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)"` — confirmed live
    /// against the fixed jar, see `docs/WALNUT-BUGS.md` WB-033. Before the fix, the same
    /// input threw a raw `NullPointerException: Cannot invoke
    /// "Automata.NumberSystem.parseBase()" because "ns" is null`; this port never
    /// reproduced that as a `panic!` — it was always this `Err` variant, matching how
    /// WB-013 (the same "null NS reaches an unguarded dereference" shape) is handled in
    /// `wr-logic` — so the fix here is message-text-only (see also this variant's
    /// `is_handled()` classification in `wr_cli::prover`, which moved out of its own
    /// stale `false` arm into the general `ProverError::Convert(_) => true` bucket).
    NoNumberSystem,
    /// `NumberSystem.parseBase`'s own guard (`NumberSystem.java:237-243`):
    /// `"Base of automaton's number system must be > 1 and int, found: <base>"`.
    ///
    /// `<base>` is `determineBase(name)` — everything after the FIRST `_` of the number
    /// system's name — and the guard fires when that is not `^\d+$` or parses to `<= 1`.
    /// Live on a perfectly ordinary input: `convert x msd_2 FTM;` where `FTM` is a word
    /// automaton over `msd_fib` gives `found: fib` (golden-corpus fixture 554).
    ///
    /// Added by U27: this port used to derive the source base from the track's *alphabet
    /// size*, which for `msd_fib` is 2 — so `convert`ing an `msd_fib` automaton to `msd_2`
    /// reported "New and old number systems are identical: msd_2" instead. See
    /// [`convert_ns`].
    BaseNotAPositiveInt {
        /// The offending substring, e.g. `"fib"`.
        found: String,
    },
    /// **Not a `WalnutException`** — the all-digit-but-too-big case, kept distinguishable
    /// from [`ConvertNsError::BaseNotAPositiveInt`] on purpose.
    ///
    /// Java's guard is `if (!isNumber(baseStr) || Integer.parseInt(baseStr) <= 1) throw new
    /// WalnutException(...)` (`NumberSystem.java:237-243`), and `||` short-circuits: for a
    /// `baseStr` that *does* match `^\d+$` but overflows a 32-bit int (`msd_99999999999999`),
    /// `isNumber` is true, so `Integer.parseInt` runs and throws an **uncaught**
    /// `java.lang.NumberFormatException` — a different failure, with a different message,
    /// from the "base must be > 1 and int" one. Folding the two together here would be
    /// silently *improving* on Java, which `CLAUDE.md`'s prime directive #2 (faithful
    /// behavior, including quirks) forbids; so this variant reproduces
    /// `NumberFormatException.forInputString`'s own message instead.
    ///
    /// No `docs/WALNUT-BUGS.md` entry: this is unreachable from any real input path. Every
    /// name that reaches [`parse_base`] comes from
    /// [`crate::automaton::Automaton::track_ns_names`], i.e. either a recorded
    /// `NumberSystem.getName()` (always `<msd|lsd>_<base>` for a base that already parsed as
    /// an `int`) or an `msd_<k>`/`lsd_<k>` this crate built from an `i32` — neither can carry
    /// a 14-digit base. It is guarded rather than left to `unwrap` because "unreachable" is a
    /// claim about today's call graph, not about the function.
    BaseOverflowsInt {
        /// The offending all-digit substring, e.g. `"99999999999999"`.
        found: String,
    },
    /// `"New and old number systems are identical: <name>"` (`:467`). Carries the
    /// reconstructed `msd_k`/`lsd_k` name Java prints via `ns.getName()`.
    IdenticalNumberSystems {
        /// e.g. `"msd_2"`.
        name: String,
    },
    /// `"New and old number systems must have bases k^i and k^j for some integer k."`
    /// (`:484`) — [`util::common_root`] returned [`util::NO_COMMON_ROOT`].
    NoCommonRoot,
    /// `"Automaton must be deterministic for msd_k^j conversion"`
    /// (`convertMsdBaseToExponent`, `:536-538`).
    ///
    /// Unreachable through [`convert_ns`] itself (its own totalization at `:488-490`, plus
    /// the `minimizeSelfWithOutput` that precedes every path into that helper, leave the
    /// automaton deterministic and total) — Java's own characterization test reaches it
    /// only by reflection, and the test below reaches it by calling the private helper
    /// directly. Ported anyway, exactly as Java keeps the defensive guard.
    NotDeterministicAndTotal,
    /// `"Base mismatch: expected <expected>, found <found>"` (`convertLsdBaseToRoot`,
    /// `:570-572`).
    BaseMismatch {
        /// `root^exponent`.
        expected: i32,
        /// The automaton's declared base.
        found: i32,
    },
    /// `UtilityMethods.exactIntegerExponent`'s `root <= 1` guard (WB-032's fix,
    /// `Main/UtilityMethods.java`) — `IllegalArgumentException("root must be > 1, got " +
    /// root)`. **Not a `WalnutException`.** See [`exact_integer_exponent`]'s doc comment for
    /// why this is unreachable through [`convert_ns`]'s own two call sites; kept for the same
    /// reason [`ConvertNsError::NotDeterministicAndTotal`] is.
    InvalidRoot {
        /// The offending `root`, always `<= 1`.
        root: i32,
    },
    /// `UtilityMethods.exactIntegerExponent`'s final consistency check (WB-032's fix) —
    /// `IllegalArgumentException(base + " is not an exact power of " + root)`. **Not a
    /// `WalnutException`.** See [`exact_integer_exponent`]'s doc comment for why this is
    /// unreachable through [`convert_ns`]'s own two call sites; kept for the same reason
    /// [`ConvertNsError::NotDeterministicAndTotal`] is.
    NotAnExactPower {
        /// The value that turned out not to be an exact power of `root`.
        base: i32,
        /// The claimed root.
        root: i32,
    },
}

impl fmt::Display for ConvertNsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertNsError::NotSingleInput => {
                write!(f, "Automaton must have exactly one input to be converted.")
            }
            // Java's fixed `WalnutException.noNumberSystem(subject)` text, verbatim —
            // `walnut-java` commit `c75e630` (WB-033, `docs/WALNUT-BUGS.md`). `subject` is
            // `convertNS`'s own `"the automaton being converted"`.
            ConvertNsError::NoNumberSystem => write!(
                f,
                "the automaton being converted has no attached number system (its alphabet \
                 was declared explicitly, e.g. {{0,1}}, rather than as msd_k/lsd_k)"
            ),
            ConvertNsError::BaseNotAPositiveInt { found } => write!(
                f,
                "Base of automaton's number system must be > 1 and int, found: {found}"
            ),
            // `NumberFormatException.forInputString` (`java.lang.NumberFormatException:67`),
            // for radix 10 — Java appends ` under radix <r>` only when the radix is not 10.
            ConvertNsError::BaseOverflowsInt { found } => {
                write!(f, "For input string: \"{found}\"")
            }
            ConvertNsError::IdenticalNumberSystems { name } => {
                write!(f, "New and old number systems are identical: {name}")
            }
            ConvertNsError::NoCommonRoot => write!(
                f,
                "New and old number systems must have bases k^i and k^j for some integer k."
            ),
            ConvertNsError::NotDeterministicAndTotal => {
                write!(f, "Automaton must be deterministic for msd_k^j conversion")
            }
            ConvertNsError::BaseMismatch { expected, found } => {
                write!(f, "Base mismatch: expected {expected}, found {found}")
            }
            ConvertNsError::InvalidRoot { root } => {
                write!(f, "root must be > 1, got {root}")
            }
            ConvertNsError::NotAnExactPower { base, root } => {
                write!(f, "{base} is not an exact power of {root}")
            }
        }
    }
}

impl std::error::Error for ConvertNsError {}

/// `FA.isDeterministicAndTotal()` (`FA.java:521-528`) — **not** the same predicate as
/// [`Fa::is_deterministic_and_total`], which is why this exists.
///
/// Java asks only "does every state's transition map have exactly `alphabetSize` KEYS".
/// It never looks at how many destinations a key holds, and it never checks *which* keys
/// they are. [`Fa::is_deterministic_and_total`] is strictly stronger on both counts
/// (`matches!(m.get(&sym), Some(dests) if dests.len() == 1)` for every `sym` in range), so
/// an NFA-shaped table with every symbol present is "total" to Java and "not total" to it.
///
/// That difference is unobservable at [`convert_ns`]'s two `if (!isDeterministicAndTotal())
/// totalize()` sites — [`totalize`] only fills MISSING keys, so on such a table it is a
/// no-op either way — but it is **observable** at
/// [`convert_msd_base_to_exponent`]'s guard, where Java proceeds (silently taking
/// destination `0`) and the stronger predicate would return an error instead. Java's exact
/// predicate is used at all three sites so no branch diverges.
fn is_deterministic_and_total_java(fa: &Fa) -> bool {
    (0..fa.q).all(|q| fa.d[q].len() == fa.alphabet_size)
}

/// Java's `(int) Math.pow(a, b)`. `f64::powf` matches `Math.pow` exactly on the
/// small integral arguments this file uses, and Rust's saturating `f64 as i32` matches
/// Java's saturating `(int)` cast of a `double` at the extremes.
fn int_pow(base: i32, exponent: i32) -> i32 {
    f64::from(base).powf(f64::from(exponent)) as i32
}

/// `UtilityMethods.exactIntegerExponent(int, int)` — **WB-032's fix**
/// (`docs/WALNUT-BUGS.md`), landed in `walnut-java` commit `18b7c4b` on branch
/// `bugfix/wb-032`. Computes the exponent `j` such that `root^j == base` by exact integer
/// repeated multiplication, never floating-point logarithms.
///
/// # File placement: deliberately here, not in `wr_core::util`
///
/// Java's `exactIntegerExponent` lives in the general-purpose `Main/UtilityMethods.java`,
/// which this crate already ports as [`crate::util`] — the same file that hosts this
/// function's sibling [`util::common_root`], and `wr_core::util`'s module doc says as
/// much. This port keeps `exact_integer_exponent` in `logicalops.rs` instead, next to its
/// only caller ([`convert_ns`]), rather than relocating it to match Java's file layout
/// exactly: it is a same-crate, no-behavior-change move, and WB-032's fix is scoped to
/// fixing the computation, not to a structural refactor. Not required reading to trust the
/// fix, but noted explicitly per this project's convention of stating deliberate
/// deviations from a strict per-file Java->Rust mapping rather than leaving them implicit.
///
/// # What this replaces
///
/// `convert_ns`'s two call sites used to compute the same `j` as
/// `(int) (Math.log(base) / Math.log(root))` (ported here as `truncated_log_ratio`, backed
/// by a bit-for-bit transliteration of Java's own non-correctly-rounded `Math.log`,
/// `java_log`). That quotient of two logarithms is not exact — it can land a fraction of an
/// ulp below the true integer answer, and the truncating cast then rounded it DOWN by a
/// whole unit on 343 real `(root, exponent)` pairs (`root <= 46340`, `root^exponent <=
/// 2^31`), the smallest being `(1000, 10)`: `Math.log(1000) / Math.log(10) ==
/// 2.9999999999999996`, truncating to `2` instead of `3`. `docs/WALNUT-BUGS.md`'s WB-032
/// entry carries the full historical diagnosis and characterization; both `java_log` and
/// `truncated_log_ratio` are gone from this file now that nothing calls them.
///
/// # The algorithm
///
/// A direct port of Java's fix: start `exponent = 0`, `power = 1`, and repeatedly multiply
/// `power` by `root` (incrementing `exponent` each time) until `power >= base`, then check
/// `power == base` exactly. `power` is `i64` (matching Java's `long power`) even though
/// `base`/`root`/the returned exponent are all `i32` — the intermediate product can exceed
/// `i32::MAX` on the very last multiply before the loop notices `power >= base` and stops
/// (`base` itself is `<= i32::MAX`, but `power` can overshoot it by up to a factor of `root`
/// first).
///
/// # Reachability of the two error arms
///
/// Both of `convert_ns`'s call sites pass a `root` that came out of [`util::common_root`]
/// applied to `from_base`/`to_base`, which by the time either call site runs are already
/// known to differ (the `from_base == to_base` case returns earlier in `convert_ns`) and are
/// each already validated `> 1` by [`parse_base`]. `common_root`'s own contract (see its doc
/// comment) guarantees any result other than [`util::NO_COMMON_ROOT`] is a genuine common
/// divisor reached by exact integer division of the larger by the smaller — never `1` (its
/// `a == 1 || b == 1` guard returns `NO_COMMON_ROOT` first) — so `root` is always `> 1`, and
/// `base` (one of `from_base`/`to_base`) is always exactly `root^exponent` for some
/// non-negative integer `exponent`. **Neither error arm below is therefore reachable through
/// `convert_ns`'s real call sites**; both are kept anyway, matching Java's own defensive
/// `IllegalArgumentException` guards and the "ported anyway, exactly as Java keeps the
/// defensive guard" precedent [`ConvertNsError::NotDeterministicAndTotal`]'s doc comment
/// already sets for a neighbouring guard in this same function.
fn exact_integer_exponent(base: i32, root: i32) -> Result<i32, ConvertNsError> {
    if root <= 1 {
        return Err(ConvertNsError::InvalidRoot { root });
    }
    let mut exponent: i32 = 0;
    let mut power: i64 = 1;
    while power < i64::from(base) {
        power *= i64::from(root);
        exponent += 1;
    }
    if power != i64::from(base) {
        return Err(ConvertNsError::NotAnExactPower { base, root });
    }
    Ok(exponent)
}

/// `NumberSystem.parseBase()` (`NumberSystem.java:237-243`), including its
/// `determineBase` (`:265-267`) helper: the base is everything after the FIRST `_` of the
/// number system's name, and it must match `UtilityMethods.PATTERN_NUMBER` (`^\d+$`,
/// `UtilityMethods.java:35`) and parse to `> 1`.
///
/// A name with no `_` at all cannot occur here — every name reaching this point comes from
/// [`crate::automaton::Automaton::track_ns_names`], which either replays a recorded
/// `NumberSystem.getName()` (always normalized to `<msd|lsd>_<base>`, see
/// `normalizeNumberSystemToken`) or builds `msd_<k>`/`lsd_<k>` itself. Java's
/// `name.substring(name.indexOf("_") + 1)` on an underscore-free string would return the
/// whole string (`indexOf` = -1), which then fails the `isNumber` check for anything
/// non-numeric — reproduced here by treating "no `_`" as "the whole name is the base".
///
/// # The `||` short-circuit is load-bearing
///
/// Java's condition is `!isNumber(baseStr) || Integer.parseInt(baseStr) <= 1`, so
/// `Integer.parseInt` runs **only** on a string that already matched `^\d+$` — and on such a
/// string it can still fail, by overflowing a 32-bit `int`. That failure is an uncaught
/// `NumberFormatException` thrown *instead of* the "must be > 1 and int" `WalnutException`,
/// so the two stay distinguishable here as well: see [`ConvertNsError::BaseOverflowsInt`].
/// Evaluating `parse::<i32>()` up front and folding its failure into `BaseNotAPositiveInt` —
/// which is what this function used to do — would silently normalize a behavior real Walnut
/// does not have.
fn parse_base(name: &str) -> Result<i32, ConvertNsError> {
    let base_str = match name.split_once('_') {
        Some((_, rest)) => rest,
        None => name,
    };
    let not_a_positive_int = || ConvertNsError::BaseNotAPositiveInt {
        found: base_str.to_string(),
    };
    // `UtilityMethods.isNumber` — `PATTERN_NUMBER.matcher(s).matches()`, i.e. `^\d+$`.
    let is_number = !base_str.is_empty() && base_str.bytes().all(|b| b.is_ascii_digit());
    if !is_number {
        return Err(not_a_positive_int());
    }
    // Reached only when `isNumber` held, exactly as in Java. An all-ASCII-digit string can
    // fail `parse::<i32>` for one reason only — it is too big — and that is
    // `Integer.parseInt`'s `NumberFormatException`, not `parseBase`'s own guard.
    let base = base_str
        .parse::<i32>()
        .map_err(|_| ConvertNsError::BaseOverflowsInt {
            found: base_str.to_string(),
        })?;
    if base <= 1 {
        return Err(not_a_positive_int());
    }
    Ok(base)
}

/// The `msd_k`/`lsd_k` name Java would have built for a track, used only to fill in
/// [`ConvertNsError::IdenticalNumberSystems`]'s `ns.getName()` (`:467`). Exact for every
/// base-*k* system, which is the only kind [`convert_ns`] accepts (a custom base's name
/// would not survive `parseBase()` in the first place).
fn ns_name(is_msd: bool, base: i32) -> String {
    let prefix = if is_msd {
        crate::numsys::MSD_UNDERSCORE
    } else {
        crate::numsys::LSD_UNDERSCORE
    };
    format!("{prefix}{base}")
}

/// `AutomatonLogicalOps.setAutomatonAlphabet` (`:662-665`) **together with** the
/// `A.getNS().set(0, new NumberSystem(<prefix> + newBase))` statement that immediately
/// precedes each of its two call sites (`:554`/`:650`).
///
/// The two Java statements are fused into one function here on purpose, because this
/// crate splits a `NumberSystem` into the parallel vectors `msd`/`all_reps`
/// (`automaton.rs`'s field docs) and `PORTING.md`'s parallel-vector ruling requires every
/// mutation site to move them **together**, in the same statement as the original.
///
/// * `msd` <- `Some(is_msd)`, the direction encoded in the new system's name.
/// * `all_reps` <- `None`. `new NumberSystem(name)` sets `allRepresentations` from
///   `loadAutomatonOrNull` (`NumberSystem.java:147-150`), i.e. from a
///   `Custom Bases/<name>.txt` file. `wr-core` performs no file I/O (U5's design premise),
///   and no such file exists for a plain integer base in a stock install, so `None` is the
///   faithful outcome. Same declared limitation as [`flip_ns`]'s: a user who drops a
///   hand-written `Custom Bases/msd_4.txt` into the session would get a restriction in Java
///   and none here.
/// * `alphabet` <- `[0..new_base-1]` and `fa.alphabet_size` <- `new_base`
///   (`richAlphabet.setA(List.of(intRangeList(newBase)))` + `setAlphabetSize(newBase)`).
///
/// [`Automaton::setup_encoder`] is called afterwards, which Java does NOT do
/// (`RichAlphabet.setA` leaves the cached `encoder` stale). Not a divergence: `convertNS`
/// works only on arity-1 automata, and `encoder == [1]` for every arity-1 alphabet
/// whatever its size — so the recompute is a no-op that keeps this crate's eagerly-cached
/// encoder honest instead of relying on Java's stale one happening to be right.
///
/// `label` is deliberately untouched (Java touches neither), so the track keeps its name.
fn set_number_system_and_alphabet(a: &mut Automaton, is_msd: bool, new_base: i32) {
    a.msd = vec![Some(is_msd)];
    a.all_reps = vec![None];
    // Java installs `new NumberSystem(<msd|lsd>_ + newBase)`, whose name is exactly what
    // `Automaton::track_ns_names` reconstructs from the `0..new_base` alphabet installed
    // on the next line — so `None` ("no recorded name, reconstruct it") is the right
    // entry here, not a stale carry-over of the pre-conversion base's name.
    a.ns_name = vec![None];
    a.alphabet = vec![util::int_range_list(new_base)];
    a.fa.alphabet_size = new_base as usize;
    a.setup_encoder();
    a.debug_assert_track_invariant();
}

/// `AutomatonLogicalOps.computeStringValue(List<Integer>, int root)` (`:671-677`) — the
/// value of a digit list read **least-significant-first**, `Σ digits[i] * root^i`.
///
/// Java's `Math.addExact`/`Math.multiplyExact` throw `ArithmeticException` on `int`
/// overflow; the `expect`s below are the faithful analog (an unchecked throw, in a place a
/// caller cannot meaningfully recover from). Unreachable through [`convert_ns`]: the list
/// is at most `exponent` digits long and its value is bounded by the automaton's own base,
/// which already fit in an `i32` when it was parsed.
fn compute_string_value(digits: &[i32], root: i32) -> i32 {
    let mut value: i32 = 0;
    for (i, &digit) in digits.iter().enumerate() {
        let place = digit
            .checked_mul(int_pow(root, i as i32))
            .expect("computeStringValue: integer overflow (Java: ArithmeticException)");
        value = value
            .checked_add(place)
            .expect("computeStringValue: integer overflow (Java: ArithmeticException)");
    }
    value
}

/// `AutomatonLogicalOps.buildInitialMorphism(FA)` (`:771-781`) — the `Q x alphabetSize`
/// state-transition matrix `morphism[q][d] = δ(q, d)`, taking the FIRST destination of
/// each entry (`getInt(0)`).
///
/// See this module's docs: "morphism" here is Walnut's word for this matrix and has
/// nothing to do with [`crate::morphism::Morphism`].
///
/// # Panics
///
/// If any `(state, symbol)` pair in `0..q x 0..alphabet_size` is missing — Java's
/// `getNfaStateDests(q, di).getInt(0)` NPEs on exactly the same input. Its only caller
/// runs behind [`convert_msd_base_to_exponent`]'s deterministic-and-total guard, so this is
/// a precondition violation, not a reachable user error.
fn build_initial_morphism(fa: &Fa) -> Vec<Vec<usize>> {
    (0..fa.q)
        .map(|q| {
            (0..fa.alphabet_size as i32)
                .map(|di| {
                    fa.d[q]
                        .get(&di)
                        .expect("buildInitialMorphism: automaton must be total")[0]
                })
                .collect()
        })
        .collect()
}

/// `AutomatonLogicalOps.buildTransitionsFromMorphism(FA, List<List<Integer>>)`
/// (`:727-740`) — reinterpret a morphism matrix as a transition table, `row[di]` becoming
/// the (single) destination on symbol `di`.
///
/// The row length is what re-bases the automaton: after `exponent - 1` extensions each row
/// holds `alphabetSize^exponent` entries, one per length-`exponent` digit word, so the
/// resulting table is over the alphabet `0..k^exponent - 1`.
fn build_transitions_from_morphism(
    fa: &Fa,
    morphism: &[Vec<usize>],
) -> Vec<BTreeMap<i32, Vec<usize>>> {
    (0..fa.q)
        .map(|q| {
            morphism[q]
                .iter()
                .enumerate()
                .map(|(di, &dest)| (di as i32, vec![dest]))
                .collect()
        })
        .collect()
}

/// `AutomatonLogicalOps.updateTransitionsFromMorphism(FA, int exponent)` (`:745-765`) —
/// extend the one-digit transition matrix into the `exponent`-digit one and install it.
///
/// Each extension step replaces `prev[j][k]` (the state reached from `j` by the `k`-th
/// word of the current length) with one entry per outgoing symbol of `j`, in that symbol's
/// sorted order:
///
/// ```text
/// extended[j][k * k_alphabet + di] = δ(prev[j][k], di)
/// ```
///
/// so the index of a word in the row equals the word's own value read
/// **most-significant-first** — which is exactly what makes this the `msd_k -> msd_{k^j}`
/// grouping and NOT the lsd one (that direction is [`convert_lsd_base_to_root`]'s separate
/// BFS, which reads its digit strings least-significant-first).
///
/// Two Java details preserved exactly, both easy to "clean up" into a different algorithm:
///
/// * The inner loop iterates the key set of state **`j`** (`getNfaStateKeySet(j)`), not of
///   `prev[j][k]`, and looks the transition up on the *other* state. Equivalent only
///   because the caller guarantees the table is total; ported as written. `BTreeMap`'s
///   sorted key order matches Java's `Int2ObjectRBTreeMap`, so the index arithmetic above
///   holds in both engines.
/// * The loop runs for `i in 2..=exponent`, so `exponent <= 1` performs no extension at
///   all and the matrix is installed as-is.
///
/// # Panics
///
/// Same precondition as [`build_initial_morphism`] (Java: NPE).
fn update_transitions_from_morphism(fa: &mut Fa, exponent: i32) {
    let mut prev_morphism = build_initial_morphism(fa);
    for _i in 2..=exponent {
        let mut new_morphism: Vec<Vec<usize>> = Vec::with_capacity(fa.q);
        // Java's bound is `fa.getQ()`; iterating the rows is equivalent because
        // `build_initial_morphism` emits exactly `fa.q` rows and every extension below
        // emits one row per row it consumed, so `prev_morphism.len() == fa.q` throughout.
        for (j, prev_row) in prev_morphism.iter().enumerate() {
            // Hoisted out of the inner loop; Java re-reads the same key set each iteration.
            let symbols: Vec<i32> = fa.d[j].keys().copied().collect();
            let mut extended_row: Vec<usize> = Vec::new();
            for &src in prev_row {
                for &di in &symbols {
                    let next_state = fa.d[src]
                        .get(&di)
                        .expect("updateTransitionsFromMorphism: automaton must be total")[0];
                    extended_row.push(next_state);
                }
            }
            new_morphism.push(extended_row);
        }
        prev_morphism = new_morphism;
    }
    fa.d = build_transitions_from_morphism(fa, &prev_morphism);
}

/// `AutomatonLogicalOps.convertMsdBaseToExponent(Automaton, int exponent)` (`:535-560`,
/// `private`) — `msd_base` to `msd_{base^exponent}`, by grouping `exponent` consecutive
/// digits into one.
///
/// `base` is this crate's stand-in for Java's `A.getNS().get(0).parseBase()` (`:540`); see
/// [`convert_ns`]'s own doc comment for why the base is threaded as a parameter rather than
/// read back off the automaton. Its sole caller passes the common root, which is what the
/// automaton's declared base is by then on both paths into here.
///
/// The state SET is untouched — only the transition table is re-keyed — so unlike
/// [`convert_lsd_base_to_root`] this cannot invalidate `q0`.
fn convert_msd_base_to_exponent(
    a: &mut Automaton,
    base: i32,
    exponent: i32,
) -> Result<(), ConvertNsError> {
    if !is_deterministic_and_total_java(&a.fa) {
        return Err(ConvertNsError::NotDeterministicAndTotal);
    }

    let new_base = int_pow(base, exponent);

    update_transitions_from_morphism(&mut a.fa, exponent);

    // `A.getNS().set(0, new NumberSystem(MSD_UNDERSCORE + newBase))` (`:554`) +
    // `setAutomatonAlphabet(A, newBase)` (`:555`).
    set_number_system_and_alphabet(a, true, new_base);
    Ok(())
}

/// `AutomatonLogicalOps.convertLsdBaseToRoot(Automaton, int root, int exponent)`
/// (`:566-657`, `private`) — `lsd_{root^exponent}` to `lsd_root`, by splitting each
/// big-base digit into `exponent` small ones.
///
/// A BFS over pairs `(old state, digits read so far)`: while fewer than `exponent` digits
/// have accumulated the old state stands still and the partial string grows; on the
/// `exponent`-th digit the pair jumps to `δ(old state, value(string))` with an empty
/// string again. Digit strings are valued **least-significant-first**
/// ([`compute_string_value`]), which is what makes this the lsd direction. A partial
/// state's OUTPUT is the output of the state the *completed* prefix would reach — correct
/// under lsd, where the unread digits are the high-order ones and are all zero.
///
/// `base` is this crate's stand-in for `A.getNS().get(0).parseBase()` (`:568`); it is only
/// read by the `base != root^exponent` guard (`:569-572`).
///
/// # `A.fa.setCanonized(false)` (`:647`) is a no-op here
///
/// See this module's docs: `Fa` has no `canonized` memo, so it always canonicalizes when
/// asked — which is precisely what clearing the flag requests.
///
/// # `q0` is left stale after `setFields`, exactly as in Java — and it is masked
///
/// `newStates[0]` is the BFS root `(A.fa.getQ0(), [])`, so `0` is always the correct new
/// initial state, but neither `FA.setFields` nor this method assigns `q0` (the same
/// omission WB-016 (`docs/WALNUT-BUGS.md`) documented in `word_automaton::
/// reverse_with_output` — that sibling instance has since been fixed, see its own doc
/// comment; this one has not). Not filed as its own bug because it is unreachable: this
/// method's ONLY caller invokes it immediately after `WordAutomaton.reverseWithOutput`
/// (`:505`), whose closing `minimizeSelfWithOutput` -> `combine` -> `forceCanonize`
/// always leaves `q0 == 0` (`FA.canonizeInternal` assigns `q0 = permutationMap.get(q0)`,
/// and the map sends `q0` to `0`) — true regardless of whether `reverseWithOutput`'s own
/// `q0` bug is fixed, since `forceCanonize` renumbers from WHATEVER `q0` currently holds.
/// Ported verbatim anyway. Deliberately left unfixed here (WB-016's own upstream fix,
/// `walnut-java` commit `d6e9799`/`bugfix/wb-016`, explicitly scopes this sibling out —
/// "a different call path, not independently reproduced" — as separate, later work).
///
/// # Panics
///
/// If the old transition table is missing an entry the BFS needs (Java: NPE on
/// `oldD.get(...).get(...).getInt(0)`). The caller totalizes first, so this is a
/// precondition violation rather than a reachable user error.
fn convert_lsd_base_to_root(
    a: &mut Automaton,
    base: i32,
    root: i32,
    exponent: i32,
) -> Result<(), ConvertNsError> {
    let expected = int_pow(root, exponent);
    if base != expected {
        return Err(ConvertNsError::BaseMismatch {
            expected,
            found: base,
        });
    }

    let old_o = a.fa.o.clone();
    let old_d = a.fa.d.clone();
    let jump = |state: usize, value: i32| -> usize {
        old_d[state]
            .get(&value)
            .expect("convertLsdBaseToRoot: automaton must be total")[0]
    };

    // BFS structures. `HashMap` (not `BTreeMap`) matches Java's `HashMap` and is safe
    // under `PORTING.md`'s iteration-order rule: `state_map` is only ever looked up by
    // key, never iterated, and the numbering it hands out is fixed by the queue order.
    let mut new_states: Vec<(usize, Vec<i32>)> = Vec::new();
    let mut queue: VecDeque<(usize, Vec<i32>)> = VecDeque::new();
    let mut state_map: HashMap<(usize, Vec<i32>), usize> = HashMap::new();
    let mut new_d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
    let mut new_o: Vec<i32> = Vec::new();

    let init = (a.fa.q0, Vec::new());
    new_states.push(init.clone());
    queue.push_back(init.clone());
    state_map.insert(init, 0);

    while let Some(curr) = queue.pop_front() {
        new_d.push(BTreeMap::new());
        let curr_idx = state_map[&curr];

        // Output logic.
        if curr.1.is_empty() {
            new_o.push(old_o[curr.0]);
        } else {
            let string_val = compute_string_value(&curr.1, root);
            new_o.push(old_o[jump(curr.0, string_val)]);
        }

        // Build transitions for each possible digit `di` in `0..root-1`.
        for di in 0..root {
            let mut next_string = curr.1.clone();
            next_string.push(di);

            let next = if (curr.1.len() as i32) < exponent - 1 {
                // Haven't reached exponent length yet.
                (curr.0, next_string)
            } else {
                // A full 'digit string', so jump to an actual next state.
                let next_string_val = compute_string_value(&next_string, root);
                (jump(curr.0, next_string_val), Vec::new())
            };

            let next_idx = match state_map.get(&next) {
                Some(&idx) => idx,
                None => {
                    let idx = new_states.len();
                    new_states.push(next.clone());
                    queue.push_back(next.clone());
                    state_map.insert(next, idx);
                    idx
                }
            };

            new_d[curr_idx].insert(di, vec![next_idx]);
        }
    }

    a.fa.set_fields(new_states.len(), new_o, new_d);
    // `A.fa.setCanonized(false)` (`:647`) — ported onto this crate's wrapper-level flag
    // (`Automaton::canonized`; U24 added it, U24's review fixes wired up every reset
    // site Java has). An earlier draft of this line claimed there was "nothing to
    // clear", which was true only until that flag existed.
    a.set_canonized(false);

    // `A.getNS().set(0, new NumberSystem(LSD_UNDERSCORE + root))` (`:650`) +
    // `setAutomatonAlphabet(A, root)` (`:651`).
    set_number_system_and_alphabet(a, false, root);
    Ok(())
}

/// `AutomatonLogicalOps.convertNS(Automaton, boolean toMsd, int toBase)` (`:455-529`) —
/// convert a single-track automaton's number system from `[msd|lsd]_{k^i}` to
/// `[msd|lsd]_{k^j}`, in place. The `convert` command's whole implementation
/// (`Prover.java:739`).
///
/// Java's own doc comment notes "this assumes that A is a word automaton when it may not
/// be"; it works on both, since a plain DFA is just a DFAO whose outputs are `0`/`1`, and
/// the `convert` command accepts either.
///
/// # Legal conversions
///
/// Only `msd`/`lsd` **and/or** a base change between two powers of a common root:
/// `fromBase = k^i`, `toBase = k^j` for some integer `k`
/// ([`util::common_root`] decides this — note it is a genuine common-root test, so e.g.
/// `4 -> 8` is legal with `k = 2`, while `2 -> 6` is not). The four steps:
///
/// 1. equal bases -> a pure direction flip, one `reverseWithOutput` (`:465-479`);
/// 2. otherwise, put the automaton in msd form (`:495-497`), then
/// 3. ungroup `k^i -> k` if needed ([`convert_lsd_base_to_root`], run on the reversed
///    automaton — hence the extra `reverseWithOutput` at `:505`), then
/// 4. regroup `k -> k^j` if needed ([`convert_msd_base_to_exponent`]), then
/// 5. `if (toMsd == currentlyReversed) reverseWithOutput` (`:525-527`) — one final
///    reversal that simultaneously undoes step 3's and delivers the requested direction.
///
/// # The source base: this crate's stand-in for `A.getNS().get(0).parseBase()`
///
/// Java reads the source base off the track's `NumberSystem` object. `wr-core`'s
/// `Automaton` deliberately carries no `NumberSystem` (`automaton.rs`'s field docs: that
/// would be circular, since a `NumberSystem` owns three `Automaton`s), only the two facts
/// ported code reads off one — and the *base* is not among them. It is therefore
/// **derived from the automaton's own alphabet**, `a.alphabet[0].len()`.
///
/// That derivation is exact, not an approximation: a track whose number system is `msd_k`
/// or `lsd_k` always has alphabet `[0, 1, ..., k-1]` — Java's own
/// `Automaton(String address)` builds it that way from the header
/// (`NumberSystem.getBaseAlphabet`), and this crate's `wr-io` reader and
/// [`set_number_system_and_alphabet`] below both do the same — so
/// `alphabet[0].len() == parseBase()` for every input `convertNS` can legally receive. A
/// track declared with an EXPLICIT alphabet instead (`{0,1,3}`) has no number system at
/// all in Java, and takes [`ConvertNsError::NoNumberSystem`] (WB-033) before the base is
/// ever used.
///
/// **This used to be a caller-supplied `from_base` parameter, and that was a real defect**
/// (found in review): nothing tied it to the automaton, so a caller could pass a base
/// disagreeing with the actual alphabet and reach a failure mode Java has no counterpart
/// for — in Java `fromBase` comes from the very token that built the alphabet, so the two
/// cannot disagree. The doc comment used to claim such a call got "the same garbage Java
/// would from a mislabelled header"; it did not. It reached an unrelated `expect()` deep in
/// [`convert_lsd_base_to_root`] ("automaton must be total") or a corrupted-intermediate
/// panic in `product.rs`. Deriving the base removes the failure mode rather than
/// documenting it.
///
/// The msd/lsd half of the same `NumberSystem` is carried as `a.msd[0]` and read from
/// there, so both halves now come off the automaton. The base is still re-threaded
/// explicitly through the two helpers, for the reason Java re-reads it: each is passed the
/// base the automaton actually has *at that point*, which is not the one it started with.
///
/// # `docs/WALNUT-BUGS.md` WB-032 lived on this path — now FIXED
///
/// The two exponent computations below used to be `truncated_log_ratio(from_base,
/// common_root)`/`truncated_log_ratio(to_base, common_root)`, a floating-point log ratio
/// that silently computed one too few for 343 `(root, exponent)` pairs (`msd_10 ->
/// msd_1000` being the smallest), converting the automaton to the wrong base with no error.
/// As of `walnut-java` commit `18b7c4b` (branch `bugfix/wb-032`) both call sites use
/// [`exact_integer_exponent`] instead — exact integer arithmetic, never floating point — and
/// this port follows suit. `docs/WALNUT-BUGS.md`'s WB-032 entry carries the full historical
/// diagnosis.
///
/// # `docs/WALNUT-BUGS.md` WB-001 used to live on this path too — FIXED
///
/// The `k -> k^j` regrouping step can strand states: re-keying the transition table by
/// digit GROUPS makes any state reachable only "mid-group" unreachable from `q0`, and the
/// `minimizeSelfWithOutput` that immediately follows it (`:521`) bottoms out in Valmari
/// minimization with no intervening trim — exactly WB-001's precondition violation. This
/// was the **easiest of WB-001's four call sites to hit from the command line**, because
/// the stranding is a property of the *conversion*, not of an already-malformed input:
/// any automaton whose accepting behaviour depends on length parity strands its odd-phase
/// state the moment two binary digits are grouped into one base-4 digit.
///
/// Fixed inside Valmari itself (`walnut-java` commit `14509f1`, ported in
/// [`crate::minimize::minimize`]), not by adding a trim here — the regrouping step is
/// still the faithful port of `:513-522`. `convert_ns_no_longer_corrupts_when_regrouping_strands_a_state`
/// pins the now-correct answer on the very fixture that used to demonstrate the
/// corruption.
pub fn convert_ns(
    a: &mut Automaton,
    to_msd: bool,
    to_base: i32,
    logging: &mut crate::logging::Logging,
) -> Result<(), ConvertNsError> {
    // Java's guard is `A.getNS().size() != 1` alone; `alphabet.len()` is checked with it
    // because this crate reads the source base off the alphabet (see the doc comment
    // above) and Java's `NS`/`richAlphabet` lists are always the same length, so no
    // Java-reachable input can take one arm without the other.
    if a.msd.len() != 1 || a.alphabet.len() != 1 {
        return Err(ConvertNsError::NotSingleInput);
    }
    // `NumberSystem ns = A.getNS().get(0); int fromBase = ns.parseBase();` (`:460-462`) —
    // the `null` case is WB-033, see `ConvertNsError::NoNumberSystem`.
    let Some(from_msd) = a.msd[0] else {
        return Err(ConvertNsError::NoNumberSystem);
    };
    // `ns.parseBase()` (`NumberSystem.java:237-243`) parses the base out of the number
    // system's NAME, not out of its alphabet: `determineBase(name)` is everything after the
    // first `_`, and a non-`^\d+$` (or `<= 1`) result is a hard error.
    //
    // Deriving it from `a.alphabet[0].len()` instead — which is what this port did before
    // U27 — is exact for every plain `msd_k`/`lsd_k` base and WRONG for a custom one:
    // `msd_fib`'s alphabet is `{0, 1}`, so `convert x msd_2 FTM;` silently took the
    // `from_base == to_base` branch and reported "New and old number systems are identical"
    // where real Walnut reports `found: fib` (golden-corpus fixture 554).
    // `Automaton::track_ns_names` supplies the real name, falling back to the exact
    // `msd_<alphabet size>` reconstruction wherever no name was recorded.
    let from_name = a.track_ns_names()[0]
        .clone()
        .expect("msd[0] is Some, so track_ns_names[0] is Some");
    let from_base = parse_base(&from_name)?;

    // If the old and new bases are the same, check if only MSD/LSD is changing.
    if from_base == to_base {
        if from_msd == to_msd {
            return Err(ConvertNsError::IdenticalNumberSystems {
                name: ns_name(from_msd, from_base),
            });
        }
        // The conversion routines assume a complete transition function; totalize before
        // reversal.
        if !is_deterministic_and_total_java(&a.fa) {
            totalize(&mut a.fa, logging);
        }
        // If only msd <-> lsd differs, just reverse A.
        word_automaton::reverse_with_output_with_ctx(a, true, None, logging);
        return Ok(());
    }

    // Check if fromBase and toBase are powers of the same root.
    let common_root = util::common_root(from_base, to_base);
    if common_root == util::NO_COMMON_ROOT {
        return Err(ConvertNsError::NoCommonRoot);
    }

    // Base conversion groups or ungroups digits and assumes every grouped digit has a
    // transition, so totalize.
    if !is_deterministic_and_total_java(&a.fa) {
        totalize(&mut a.fa, logging);
    }

    // If originally LSD, we need to reverse to treat it as MSD for the conversions.
    if !from_msd {
        word_automaton::reverse_with_output_with_ctx(a, true, None, logging);
    }

    // We'll track if A is reversed relative to original.
    let mut currently_reversed = false;

    // Convert from k^i -> k if needed.
    if from_base != common_root {
        let exponent = exact_integer_exponent(from_base, common_root)?;
        word_automaton::reverse_with_output_with_ctx(a, true, None, logging);
        currently_reversed = true;

        convert_lsd_base_to_root(a, from_base, common_root, exponent)?;
        word_automaton::minimize_self_with_output_with_ctx(a, None, logging);
    }

    // Convert from k -> k^j if needed.
    if to_base != common_root {
        if currently_reversed {
            // Undo reversal from the previous step.
            word_automaton::reverse_with_output_with_ctx(a, true, None, logging);
            currently_reversed = false;
        }
        let exponent = exact_integer_exponent(to_base, common_root)?;
        convert_msd_base_to_exponent(a, common_root, exponent)?;
        word_automaton::minimize_self_with_output_with_ctx(a, None, logging);
    }

    // If final desired base is LSD but we are still in MSD form, reverse again.
    if to_msd == currently_reversed {
        word_automaton::reverse_with_output_with_ctx(a, true, None, logging);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equiv;
    use proptest::prelude::*;

    // ------------------------------------------------------------------ fixtures

    fn single_track(fa: Fa, msd: Option<bool>) -> Automaton {
        Automaton::new(fa, vec![vec![0, 1]], vec!["x".to_string()], vec![msd])
    }

    /// TOTAL 2-state DFA over `{0,1}` accepting words whose last symbol is `1`
    /// (rejects the empty word).
    fn ends_with_one() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![0]);
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

    /// **Deliberately NON-total**: accepts exactly the one-symbol word `"1"`. State 0
    /// has no `0`-transition and state 1 has none at all, so any word that isn't
    /// exactly `"1"` runs off the end of the table. This is the operand that makes the
    /// totalization precondition observable — every connective test below uses it.
    fn exactly_one() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![1]);
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, BTreeMap::new()],
        }
    }

    const WORDS: [&[i32]; 8] = [
        &[],
        &[0],
        &[1],
        &[0, 0],
        &[0, 1],
        &[1, 0],
        &[1, 1],
        &[0, 0, 1],
    ];

    // ---------------------------------------------------------- FA-level helpers

    #[test]
    fn totalize_fills_missing_transitions_and_appends_one_non_accepting_sink() {
        let mut fa = exactly_one();
        totalize(&mut fa, &mut crate::logging::Logging::new());
        assert_eq!(fa.q, 3);
        assert_eq!(
            fa.o,
            vec![0, 1, 0],
            "the sink is non-accepting (Java passes 0)"
        );
        assert_eq!(fa.d[0].get(&0), Some(&vec![2]));
        assert_eq!(fa.d[1].get(&0), Some(&vec![2]));
        assert_eq!(fa.d[1].get(&1), Some(&vec![2]));
        assert_eq!(fa.d[2].get(&0), Some(&vec![2]), "the sink self-loops");
        assert_eq!(fa.d[2].get(&1), Some(&vec![2]));
    }

    #[test]
    fn totalize_appends_no_sink_when_already_total() {
        let mut fa = ends_with_one();
        totalize(&mut fa, &mut crate::logging::Logging::new());
        assert_eq!(fa.q, 2, "an already-total automaton must not grow a sink");
        assert_eq!(fa.o, vec![0, 1]);
    }

    #[test]
    fn totalize_accepts_a_nondeterministic_table_unlike_fa_totalize() {
        // The exact shape `Fa::totalize` refuses (it asserts determinism) and
        // `FA.totalize` handles fine -- `totalize_cross_product` reaches it whenever an
        // `or`/`xor`/`imply`/`iff` operand is a genuine NFA.
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![0, 1]);
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, BTreeMap::new()],
        };
        totalize(&mut fa, &mut crate::logging::Logging::new());
        assert_eq!(fa.q, 3);
        assert_eq!(
            fa.d[0].get(&1),
            Some(&vec![0, 1]),
            "an existing nondeterministic entry is left alone"
        );
        assert_eq!(fa.d[0].get(&0), Some(&vec![2]));
    }

    #[test]
    fn flip_output_writes_plain_zero_one_and_collapses_dfao_values() {
        let mut fa = ends_with_one();
        fa.o = vec![0, 1];
        flip_output(&mut fa);
        assert_eq!(fa.o, vec![1, 0]);
        // A DFAO output > 1 is "accepting", so it flips to 0 rather than to some
        // negated word value -- Java's `setOutputIfEqual` writes a literal 0/1.
        fa.o = vec![7, 0];
        flip_output(&mut fa);
        assert_eq!(fa.o, vec![0, 1]);
    }

    #[test]
    fn is_subset_alphabet_requires_matching_arity_and_compares_tracks_as_sets() {
        assert!(is_subset_alphabet(&[vec![0, 1]], &[vec![0, 1]]));
        // Order-insensitive (Java uses `HashSet.containsAll`).
        assert!(is_subset_alphabet(&[vec![1, 0]], &[vec![0, 1]]));
        assert!(is_subset_alphabet(&[vec![0, 1]], &[vec![0, 1, 2]]));
        assert!(!is_subset_alphabet(&[vec![0, 1, 2]], &[vec![0, 1]]));
        // Arity mismatch fails outright, whatever the contents.
        assert!(!is_subset_alphabet(
            &[vec![0, 1]],
            &[vec![0, 1], vec![0, 1]]
        ));
    }

    #[test]
    fn flip_ns_flips_arithmetic_tracks_and_skips_non_arithmetic_ones() {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 8,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string(), "z".to_string()],
            vec![Some(true), None, Some(false)],
        );
        flip_ns(&mut a);
        assert_eq!(a.msd, vec![Some(false), None, Some(true)]);
    }

    /// The RECORDED NAME must flip too, not just the direction flag — see [`flip_ns`]'s
    /// docs for the two live wrong-output bugs the stale name caused (`reverse` wrote an
    /// `msd_2` header where real Walnut writes `lsd_2`; `union` then failed to reject a
    /// genuinely mixed-numeration pair, because `isNSDiffering` compares by NAME).
    ///
    /// [`Automaton::track_ns_names`] is what the writer and the `union`/`intersect`/
    /// `concat` guards actually read, so it is asserted rather than the raw field.
    #[test]
    fn flip_ns_flips_the_recorded_number_system_name_not_just_the_direction() {
        for (before, after) in [
            ("msd_2", "lsd_2"),
            ("lsd_2", "msd_2"),
            // A custom base keeps everything after the FIRST underscore, so the base
            // survives verbatim (`Automata Library` header `msd_fib` -> `lsd_fib`;
            // confirmed against the real `Walnut-all.jar`'s `reverse` output).
            ("msd_fib", "lsd_fib"),
            ("lsd_fib", "msd_fib"),
            ("msd_neg_3", "lsd_neg_3"),
            // NOT a symmetric prefix swap: Java's ternary only tests for `MSD`, so any
            // other prefix — including a second `msd` buried later — becomes `msd_`.
            ("foo_7", "msd_7"),
            ("lsdx_7", "msd_7"),
        ] {
            let mut a = single_track(ends_with_one(), Some(before.starts_with("msd_")));
            a.set_ns_names(vec![Some(before.to_string())]);

            flip_ns(&mut a);

            assert_eq!(
                a.track_ns_names(),
                vec![Some(after.to_string())],
                "flip of {before}"
            );
            // The direction flag is taken from the NEW name, exactly as
            // `NumberSystem`'s constructor derives `isMsd` from it.
            assert_eq!(
                a.msd,
                vec![Some(after.starts_with("msd_"))],
                "direction flag of {before}"
            );
        }
    }

    /// Two flips must land back on the ORIGINAL name. This round-tripped even with the
    /// bug (two stale flips cancel), which is exactly why the pre-existing
    /// double-reversal coverage never caught it — so it is pinned explicitly.
    #[test]
    fn flipping_a_number_system_name_twice_round_trips() {
        for name in ["msd_2", "lsd_5", "msd_fib", "lsd_neg_3"] {
            let mut a = single_track(ends_with_one(), Some(name.starts_with("msd_")));
            a.set_ns_names(vec![Some(name.to_string())]);
            flip_ns(&mut a);
            flip_ns(&mut a);
            assert_eq!(a.track_ns_names(), vec![Some(name.to_string())]);
            assert_eq!(a.msd, vec![Some(name.starts_with("msd_"))]);
        }
    }

    /// A track with no RECORDED name (an automaton built in memory, which is always a
    /// plain base-*k* one) still just flips its flag, and `track_ns_names`'s
    /// reconstruction branch renders the flipped direction.
    #[test]
    fn flip_ns_without_a_recorded_name_still_flips_the_reconstructed_one() {
        let mut a = single_track(ends_with_one(), Some(true));
        assert_eq!(a.track_ns_names(), vec![Some("msd_2".to_string())]);
        flip_ns(&mut a);
        assert_eq!(a.track_ns_names(), vec![Some("lsd_2".to_string())]);
    }

    /// `determineMsdOrLsd`'s `substring(0, indexOf("_"))` on a name with no `_` is
    /// `substring(0, -1)` — a `StringIndexOutOfBoundsException` in Java. Unreachable
    /// through any real writer of `ns_name` (see [`flipped_ns_name`]'s docs), ported as
    /// the JDK's own message so `Prover::caught` reports it the way `Prover.readBuffer`
    /// reports Java's.
    #[test]
    fn flipping_a_name_with_no_underscore_raises_javas_own_message() {
        let mut a = single_track(ends_with_one(), Some(true));
        a.set_ns_names(vec![Some("msd2".to_string())]);
        assert_eq!(
            crate::walnut_panic::catch_walnut_panic(|| flip_ns(&mut a)),
            Err("begin 0, end -1, length 4".to_string())
        );
    }

    /// `reverse(A, true)` — the `Main/Commands/Reverse.java` path — is what actually
    /// carries the flip out to a written file.
    #[test]
    fn reverse_with_msd_reversal_flips_the_name_a_writer_would_emit() {
        let mut a = single_track(ends_with_one(), Some(true));
        a.set_ns_names(vec![Some("msd_2".to_string())]);
        reverse(&mut a, true);
        assert_eq!(a.track_ns_names(), vec![Some("lsd_2".to_string())]);

        // `reverse(A, false)` (`:315`/`:333`/`:378`/`:459`) must NOT touch it.
        let mut b = single_track(ends_with_one(), Some(true));
        b.set_ns_names(vec![Some("msd_2".to_string())]);
        reverse(&mut b, false);
        assert_eq!(b.track_ns_names(), vec![Some("msd_2".to_string())]);
    }

    /// The custom-base half of `flip_ns` (U5): a track carrying an all-representations
    /// automaton gets that automaton's LANGUAGE reversed, reproducing what Java's
    /// `new NumberSystem("lsd_<base>")` would have loaded via `loadAutomatonOrNull`'s
    /// complement-with-reverse fallback. Uses a deliberately NON-reversal-symmetric
    /// language (`0*1`, i.e. "ends in 1") so a no-op implementation cannot pass.
    #[test]
    fn flip_ns_reverses_each_tracks_all_representations_automaton() {
        let ends_in_one = single_track(
            {
                let mut d0 = BTreeMap::new();
                d0.insert(0, vec![0]);
                d0.insert(1, vec![1]);
                Fa {
                    true_false: None,
                    q0: 0,
                    q: 2,
                    alphabet_size: 2,
                    o: vec![0, 1],
                    d: vec![d0, BTreeMap::new()],
                }
            },
            Some(true),
        );
        let mut a = single_track(ends_with_one(), Some(true));
        a.set_all_reps(vec![Some(Rc::new(ends_in_one.clone()))]);

        flip_ns(&mut a);

        assert_eq!(a.msd, vec![Some(false)], "direction still flips");
        let flipped = a.all_reps[0].as_ref().expect("still present");
        let mut expected = ends_in_one;
        reverse(&mut expected, false);
        let mut got = flipped.fa.clone();
        got.totalize(0);
        let mut want = expected.fa.clone();
        want.totalize(0);
        assert_eq!(equiv::language_equivalent(&got, &want), Ok(true));
        // And it really is a different language from where it started (guards against a
        // "reverse is the identity here" test that would pass with no implementation).
        let original = ends_in_one_fa_totalized();
        assert_eq!(equiv::language_equivalent(&got, &original), Ok(false));
    }

    // ============================== U5: applyAllRepresentations at its three call sites

    /// The valid-representation restriction of a Fibonacci-style base: the words over
    /// `{0, 1}` with **no `11` substring** (`walnut-java/Custom Bases/msd_fib.txt`, verbatim).
    /// Deliberately a language that ordinary `msd_k` numeration would never impose, so any
    /// test below that passes without the restriction being applied is caught.
    fn no_adjacent_ones() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![0]);
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 1],
            d: vec![d0, d1],
        }
    }

    /// A one-track automaton over `{0,1}` with `fa` as its language and the
    /// [`no_adjacent_ones`] restriction attached to its (only) track — the shape every
    /// automaton derived from a custom base carries.
    fn restricted(fa: Fa) -> Automaton {
        let mut a = single_track(fa, Some(true));
        a.set_all_reps(vec![Some(Rc::new(single_track(
            no_adjacent_ones(),
            Some(true),
        )))]);
        a
    }

    /// The 1-state total automaton accepting everything (`Σ*`).
    fn universal() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0]);
        Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1],
            d: vec![d0],
        }
    }

    fn accepts_exactly_the_valid_representations(a: &Automaton) {
        for word in [
            &[][..],
            &[0],
            &[1],
            &[0, 1],
            &[1, 0],
            &[1, 0, 1],
            &[0, 1, 0, 1],
        ] {
            assert!(a.fa.accepts_word(word), "must accept valid word {word:?}");
        }
        for word in [&[1, 1][..], &[0, 1, 1], &[1, 1, 0], &[1, 0, 1, 1]] {
            assert!(
                !a.fa.accepts_word(word),
                "must reject invalid word {word:?}"
            );
        }
    }

    /// `not`'s `A.applyAllRepresentations()` (`:163`). Complementing a language already
    /// restricted to the valid representations re-admits every INVALID one, so the
    /// restriction has to be re-applied — otherwise `~(x = x)` over a custom base returns
    /// "the words containing `11`" instead of the empty language. Real Walnut returns the
    /// empty language (verified against its CLI with `eval x "?msd_fib ~(x=x)"`).
    #[test]
    fn not_reapplies_the_valid_representation_restriction() {
        let restricted_identity = restricted(no_adjacent_ones());
        let negated = not(
            restricted_identity.as_dfa(),
            &mut crate::logging::Logging::new(),
        )
        .into_automaton();
        assert!(
            negated.is_empty(),
            "the complement of the valid representations, re-restricted, is empty"
        );

        // The discriminating half: the SAME automaton without the restriction attached
        // complements to a non-empty language, so this test cannot pass by accident.
        let unrestricted = single_track(no_adjacent_ones(), Some(true));
        let negated_unrestricted =
            not(unrestricted.as_dfa(), &mut crate::logging::Logging::new()).into_automaton();
        assert!(!negated_unrestricted.is_empty());
        assert!(negated_unrestricted.fa.accepts_word(&[1, 1]));
    }

    /// `totalizeCrossProduct`'s `N.applyAllRepresentations()` (`:121`), reached via `imply`.
    /// `A => A` is a tautology, so the raw cross product is `Σ*`; the restriction cuts it
    /// back to the valid representations. (`and` deliberately has no such call — it never
    /// totalizes, so it can never re-admit an invalid representation; asserted below.)
    #[test]
    fn totalize_cross_product_reapplies_the_valid_representation_restriction() {
        let mut a = restricted(no_adjacent_ones());
        let mut b = restricted(no_adjacent_ones());
        let implied = imply(&mut a, &mut b, &mut crate::logging::Logging::new()).into_automaton();
        assert!(implied.fa.accepts_word(&[0, 1, 0]));
        accepts_exactly_the_valid_representations(&implied);

        // Without the restriction the same tautology really is `Σ*` -- the discriminator.
        let mut c = single_track(no_adjacent_ones(), Some(true));
        let mut d = single_track(no_adjacent_ones(), Some(true));
        let unrestricted =
            imply(&mut c, &mut d, &mut crate::logging::Logging::new()).into_automaton();
        assert!(unrestricted.fa.accepts_word(&[1, 1]));
    }

    /// `and` must NOT apply the restriction (no such call in Java) — and it doesn't need to:
    /// intersection cannot widen a language. Pinned so a future edit doesn't "helpfully" add
    /// the call for symmetry.
    #[test]
    fn and_does_not_apply_the_valid_representation_restriction() {
        let a = restricted(universal());
        let b = restricted(universal());
        let intersection = and(&a, &b, &mut crate::logging::Logging::new()).into_automaton();
        assert!(
            intersection.fa.accepts_word(&[1, 1]),
            "`and` leaves the restriction unapplied, exactly as Java does"
        );
    }

    /// `rightQuotient`'s `M.applyAllRepresentations()` (`:228`). The accepting set is
    /// recomputed from scratch by `setOutputIfEqual`, with no regard for whether a state is
    /// reachable by a VALID representation, so the restriction must be re-applied after.
    /// `Σ* / Σ*` is `Σ*`, which the restriction cuts back to the valid representations.
    #[test]
    fn right_quotient_reapplies_the_valid_representation_restriction() {
        let a = restricted(universal());
        let b = single_track(universal(), Some(true));
        let quotient = right_quotient(&a, &b, false, &mut crate::logging::Logging::new());
        accepts_exactly_the_valid_representations(&quotient);

        // Discriminator: unrestricted, the same quotient is `Σ*`.
        let a_plain = single_track(universal(), Some(true));
        let plain_quotient =
            right_quotient(&a_plain, &b, false, &mut crate::logging::Logging::new());
        assert!(plain_quotient.fa.accepts_word(&[1, 1]));
    }

    /// The restriction must survive `right_quotient`'s wholesale replacement of the second
    /// operand's track metadata (`otherClone.setNS(A.getNS())`, `:206`) — both halves of the
    /// per-track number-system stand-in move together.
    #[test]
    fn right_quotient_copies_both_halves_of_the_track_metadata_onto_the_second_operand() {
        let a = restricted(universal());
        let b = single_track(universal(), Some(true));
        let quotient = right_quotient(&a, &b, false, &mut crate::logging::Logging::new());
        assert_eq!(quotient.msd, vec![Some(true)]);
        assert!(quotient.all_reps.iter().all(|r| r.is_some()));
    }

    /// `0*1` totalized — the "before" language for the test above.
    fn ends_in_one_fa_totalized() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, BTreeMap::new()],
        };
        fa.totalize(0);
        fa
    }

    // -------------------------------------------------- the boolean connectives

    /// Ground truth built WITHOUT any of this module's machinery: both operands
    /// totalized independently, combined by `equiv::product_dfa`, compared to the
    /// (totalized) result via the semantic equivalence oracle.
    fn assert_connective_matches_oracle(
        result: &AutomatonDFA,
        a_before: &Fa,
        b_before: &Fa,
        accept: impl Fn(bool, bool) -> bool,
    ) {
        let mut a_total = a_before.clone();
        a_total.totalize(0);
        let mut b_total = b_before.clone();
        b_total.totalize(0);
        let expected = equiv::product_dfa(&a_total, &b_total, accept).unwrap();

        // `minimize` does not preserve totality (it drops states that cannot reach
        // acceptance), and the oracle requires total DFAs on both sides.
        let mut actual = result.automaton().fa.clone();
        actual.totalize(0);
        assert_eq!(equiv::language_equivalent(&actual, &expected), Ok(true));
    }

    #[test]
    fn and_does_not_totalize_its_operands_and_intersects_correctly() {
        let a = single_track(exactly_one(), Some(true));
        let b = single_track(ends_with_one(), Some(true));
        let n = and(&a, &b, &mut crate::logging::Logging::new());

        assert_eq!(
            a.fa.q,
            exactly_one().q,
            "`and` must NOT totalize its operands (AutomatonLogicalOps.java:44-62)"
        );
        assert_eq!(b.fa.q, ends_with_one().q);

        for word in WORDS {
            let expected = exactly_one().accepts_word(word) && ends_with_one().accepts_word(word);
            assert_eq!(
                n.automaton().fa.accepts_word(word),
                expected,
                "word={word:?}"
            );
        }
        assert_connective_matches_oracle(&n, &exactly_one(), &ends_with_one(), |p, q| p && q);
    }

    #[test]
    fn or_totalizes_a_partial_operand_before_the_cross_product() {
        // Without the totalize, the product BFS simply has no transition on symbol 0
        // out of `a`'s q0, so "01" -- which `b` accepts -- would be lost.
        let mut a = single_track(exactly_one(), Some(true));
        let mut b = single_track(ends_with_one(), Some(true));
        let n = or(&mut a, &mut b, &mut crate::logging::Logging::new());

        assert_eq!(
            a.fa.q,
            exactly_one().q + 1,
            "`or` totalizes its operands IN PLACE (AutomatonLogicalOps.java:117-118)"
        );

        for word in WORDS {
            let expected = exactly_one().accepts_word(word) || ends_with_one().accepts_word(word);
            assert_eq!(
                n.automaton().fa.accepts_word(word),
                expected,
                "word={word:?}"
            );
        }
        assert!(
            n.automaton().fa.accepts_word(&[0, 1]),
            "the word that a non-totalizing `or` would lose"
        );
        assert_connective_matches_oracle(&n, &exactly_one(), &ends_with_one(), |p, q| p || q);
    }

    #[test]
    fn or_totalizes_the_second_operand_too() {
        // Mutation-tested gap: every other hand-written connective test puts the
        // PARTIAL operand in the `a` position and an already-total one in `b`, so
        // deleting `totalize(&mut b.fa)` alone would leave them all green (only the
        // proptest caught it). Here the partial operand is `b`.
        let mut a = single_track(ends_with_one(), Some(true));
        let mut b = single_track(exactly_one(), Some(true));
        let n = or(&mut a, &mut b, &mut crate::logging::Logging::new());

        assert_eq!(
            b.fa.q,
            exactly_one().q + 1,
            "`or` totalizes the SECOND operand in place too"
        );
        for word in WORDS {
            let expected = ends_with_one().accepts_word(word) || exactly_one().accepts_word(word);
            assert_eq!(
                n.automaton().fa.accepts_word(word),
                expected,
                "word={word:?}"
            );
        }
        assert!(
            n.automaton().fa.accepts_word(&[0, 1]),
            "the word a non-totalized SECOND operand would lose"
        );
    }

    #[test]
    fn xor_totalizes_a_partial_operand_before_the_cross_product() {
        let mut a = single_track(exactly_one(), Some(true));
        let mut b = single_track(ends_with_one(), Some(true));
        let n = xor(&mut a, &mut b, &mut crate::logging::Logging::new());

        for word in WORDS {
            let expected = exactly_one().accepts_word(word) != ends_with_one().accepts_word(word);
            assert_eq!(
                n.automaton().fa.accepts_word(word),
                expected,
                "word={word:?}"
            );
        }
        assert!(
            n.automaton().fa.accepts_word(&[0, 1]),
            "the word that a non-totalizing `xor` would lose"
        );
        assert_connective_matches_oracle(&n, &exactly_one(), &ends_with_one(), |p, q| p != q);
    }

    #[test]
    fn imply_totalizes_a_partial_operand_and_respects_operand_order() {
        let mut a = single_track(exactly_one(), Some(true));
        let mut b = single_track(ends_with_one(), Some(true));
        let n = imply(&mut a, &mut b, &mut crate::logging::Logging::new());

        for word in WORDS {
            let expected = !exactly_one().accepts_word(word) || ends_with_one().accepts_word(word);
            assert_eq!(
                n.automaton().fa.accepts_word(word),
                expected,
                "word={word:?}"
            );
        }
        assert!(
            n.automaton().fa.accepts_word(&[0]),
            "the word that a non-totalizing `imply` would lose"
        );
        // Asymmetry: swapping the operands is a DIFFERENT language, so a port that
        // passed `(b.o, a.o)` to `combine` would be caught here.
        let mut a2 = single_track(exactly_one(), Some(true));
        let mut b2 = single_track(ends_with_one(), Some(true));
        let swapped = imply(&mut b2, &mut a2, &mut crate::logging::Logging::new());
        assert!(
            !swapped.automaton().fa.accepts_word(&[0, 1]),
            "B -> A rejects \"01\" (B accepts it, A does not), while A -> B accepts it"
        );
        assert!(n.automaton().fa.accepts_word(&[0, 1]));
        assert_connective_matches_oracle(&n, &exactly_one(), &ends_with_one(), |p, q| !p || q);
    }

    #[test]
    fn iff_totalizes_a_partial_operand_before_the_cross_product() {
        let mut a = single_track(exactly_one(), Some(true));
        let mut b = single_track(ends_with_one(), Some(true));
        let n = iff(&mut a, &mut b, &mut crate::logging::Logging::new());

        for word in WORDS {
            let expected = exactly_one().accepts_word(word) == ends_with_one().accepts_word(word);
            assert_eq!(
                n.automaton().fa.accepts_word(word),
                expected,
                "word={word:?}"
            );
        }
        assert!(
            n.automaton().fa.accepts_word(&[0]),
            "the word that a non-totalizing `iff` would lose (neither operand accepts it)"
        );
        assert_connective_matches_oracle(&n, &exactly_one(), &ends_with_one(), |p, q| p == q);
    }

    // --------------------------------------------------------------------- not

    #[test]
    fn not_complements_including_words_that_run_off_a_partial_table() {
        let original = exactly_one();
        let n = not(
            single_track(original.clone(), Some(true)).as_dfa(),
            &mut crate::logging::Logging::new(),
        );

        for word in WORDS {
            assert_eq!(
                n.automaton().fa.accepts_word(word),
                !original.accepts_word(word),
                "word={word:?}"
            );
        }
        assert!(
            n.automaton().fa.accepts_word(&[0]),
            "\"0\" runs off the partial table; only the totalize step makes it accepted"
        );

        let mut original_total = original;
        original_total.totalize(0);
        let expected = equiv::complement(&original_total).unwrap();
        let mut actual = n.automaton().fa.clone();
        actual.totalize(0);
        assert_eq!(equiv::language_equivalent(&actual, &expected), Ok(true));
    }

    #[test]
    fn not_is_an_involution_up_to_language() {
        let original = exactly_one();
        let once = not(
            single_track(original.clone(), Some(true)).as_dfa(),
            &mut crate::logging::Logging::new(),
        );
        let twice = not(once, &mut crate::logging::Logging::new());
        for word in WORDS {
            assert_eq!(
                twice.automaton().fa.accepts_word(word),
                original.accepts_word(word),
                "word={word:?}"
            );
        }
    }

    #[test]
    fn not_minimizes_its_result() {
        // Mutation-tested gap: minimization is language-preserving, so no `accepts_word`
        // assertion anywhere can detect `justMinimize` (:162) going missing. This
        // fixture makes it structurally visible: a total 3-state DFA whose two
        // ACCEPTING sinks become two indistinguishable REJECTING sinks after the flip,
        // which `minimize` then collapses (and, being non-co-reachable to acceptance,
        // drops outright).
        let mut d = vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
        d[0].insert(0, vec![1]);
        d[0].insert(1, vec![2]);
        d[1].insert(0, vec![1]);
        d[1].insert(1, vec![1]);
        d[2].insert(0, vec![2]);
        d[2].insert(1, vec![2]);
        let a = single_track(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 1, 1],
                d,
            },
            Some(true),
        );
        // L(A) = every word of length >= 1, so L(not A) = {epsilon}.
        let n = not(a.as_dfa(), &mut crate::logging::Logging::new());
        assert!(n.automaton().fa.accepts_word(&[]));
        assert!(!n.automaton().fa.accepts_word(&[0]));
        assert!(!n.automaton().fa.accepts_word(&[1]));
        assert!(
            n.automaton().fa.q < 3,
            "the flipped automaton's two equivalent rejecting sinks must be minimized \
             away, got {} states",
            n.automaton().fa.q
        );
    }

    // -------------------------------------------- Tier-4 property over all six

    /// Random single-track DFAs over `{0,1}` with genuinely MISSING transitions
    /// (`Option<dest>` per cell) — the shape that makes the totalization precondition
    /// observable at all.
    fn arb_partial_dfa(q_max: usize, alphabet_size: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max).prop_flat_map(move |q| {
            let o = prop::collection::vec(0i32..=1, q);
            let trans = prop::collection::vec(
                prop::collection::vec(prop::option::of(0usize..q), alphabet_size),
                q,
            );
            (o, trans).prop_map(move |(o, trans)| {
                let d = trans
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .enumerate()
                            .filter_map(|(sym, dest)| dest.map(|d| (sym as i32, vec![d])))
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
        /// Tier-4 (`CLAUDE.md`'s correctness ladder): every connective against the
        /// semantic equivalence oracle, over random PARTIAL DFAs. This is the property
        /// that would catch a missing/misplaced totalize on any of the four
        /// precondition-dependent connectives, not just on the one hand-picked fixture
        /// above.
        #[test]
        fn every_connective_matches_the_language_oracle(
            a_fa in arb_partial_dfa(4, 2),
            b_fa in arb_partial_dfa(4, 2),
        ) {
            let build = |fa: &Fa| single_track(fa.clone(), Some(true));

            let n_and = and(&build(&a_fa), &build(&b_fa), &mut crate::logging::Logging::new());
            assert_connective_matches_oracle(&n_and, &a_fa, &b_fa, |p, q| p && q);

            let n_or = or(&mut build(&a_fa), &mut build(&b_fa), &mut crate::logging::Logging::new());
            assert_connective_matches_oracle(&n_or, &a_fa, &b_fa, |p, q| p || q);

            let n_xor = xor(&mut build(&a_fa), &mut build(&b_fa), &mut crate::logging::Logging::new());
            assert_connective_matches_oracle(&n_xor, &a_fa, &b_fa, |p, q| p != q);

            let n_imply = imply(&mut build(&a_fa), &mut build(&b_fa), &mut crate::logging::Logging::new());
            assert_connective_matches_oracle(&n_imply, &a_fa, &b_fa, |p, q| !p || q);

            let n_iff = iff(&mut build(&a_fa), &mut build(&b_fa), &mut crate::logging::Logging::new());
            assert_connective_matches_oracle(&n_iff, &a_fa, &b_fa, |p, q| p == q);
        }

        /// `not` against the oracle, on the RAW generated automaton.
        ///
        /// # Formerly trimmed for `docs/WALNUT-BUGS.md` WB-001; no longer
        ///
        /// `not`'s `justMinimize` step establishes none of `minimize`'s
        /// `q0`-reachability precondition (faithfully -- see [`just_minimize`]), so while
        /// WB-001 was live a randomly-generated automaton with a state unreachable from
        /// `q0` could legitimately hit it and produce a "wrong" complement; this property
        /// therefore ran `crate::trim::trim` on its input first. WB-001 is fixed
        /// (`walnut-java` commit `14509f1`, ported in [`crate::minimize::minimize`]) and
        /// the trim is removed, so `not` is now checked against the complement oracle on
        /// genuinely untrimmed input -- the shape a real `.txt` operand actually has.
        /// The missing trim still costs minimality, which this property never asserted.
        #[test]
        fn not_matches_the_complement_oracle(fa in arb_partial_dfa(4, 2)) {
            let n = not(single_track(fa.clone(), Some(true)).as_dfa(), &mut crate::logging::Logging::new());

            let mut total = fa;
            total.totalize(0);
            let expected = equiv::complement(&total).unwrap();
            let mut actual = n.automaton().fa.clone();
            actual.totalize(0);
            prop_assert_eq!(equiv::language_equivalent(&actual, &expected), Ok(true));
        }
    }

    // ------------------------------------- De Morgan's laws (U8; DESIGN.md §5 Tier 4:
    // "De Morgan across product" is named explicitly as a mandatory property invariant)
    //
    // `assert_connective_matches_oracle`/`not_matches_the_complement_oracle` above
    // already check each connective against an INDEPENDENTLY-computed ground truth
    // (`equiv::product_dfa`/`equiv::complement`), not "the same truth table" — a real
    // `and`/`or`/`not` bug is already caught there, since De Morgan holds for the true
    // languages regardless. What these two properties add, narrowly: `not` here is
    // fed a genuinely CROSS-PRODUCT-shaped input (the output of `and`, on the LHS of
    // the first identity) — a distribution `not_matches_the_complement_oracle`'s
    // standalone generator never produces — so they catch a `not` bug that happens to
    // only manifest on that specific input shape. Concretely, in
    // `de_morgan_not_and_equals_or_of_nots` below: the RHS's two `not` calls run on
    // the RAW (trimmed, still possibly partial) `a_fa`/`b_fa` directly — `or`'s
    // internal `totalize_cross_product` totalizes its operands, but that happens
    // AFTER `not`, on the two `not` results, not before. So both identities still
    // exercise `not` on a genuinely partial automaton (via the raw `a_fa`/`b_fa` on
    // the RHS, and via the cross-product `and_ab`/`or_ab` on the LHS) — a `not` that
    // silently skipped totalization would desync either identity, just not for the
    // "individually-total operands" reason an earlier version of this comment claimed.

    proptest! {
        /// ¬(A ∧ B) ≡ ¬A ∨ ¬B, computed with the real `and`/`or`/`not` from this file.
        ///
        /// `a_fa`/`b_fa` used to be TRIMMED first -- same rationale as
        /// `not_matches_the_complement_oracle` above, and removed for the same reason.
        /// `not_a`/`not_b` below call `not` directly on `a_fa`/`b_fa`, and `not`'s
        /// `justMinimize` step establishes none of `minimize`'s `q0`-reachability
        /// precondition (faithfully -- see `just_minimize`'s doc comment). While
        /// `docs/WALNUT-BUGS.md` WB-001 was live, a randomly-generated automaton with a
        /// state unreachable from `q0` could legitimately hit it and produce a "wrong"
        /// complement there -- confirmed at the time by reproducing the untrimmed failure
        /// and matching it to WB-001's documented trigger shape (a non-accepting,
        /// unreachable-from-nothing-else q0 plus a separate unreachable accepting state),
        /// rather than assuming it away. WB-001 is fixed (`walnut-java` commit `14509f1`,
        /// ported in [`crate::minimize::minimize`]), so both identities now run on the raw
        /// generated automata, including that shape.
        #[test]
        fn de_morgan_not_and_equals_or_of_nots(
            a_fa in arb_partial_dfa(4, 2),
            b_fa in arb_partial_dfa(4, 2),
        ) {
            let build = |fa: &Fa| single_track(fa.clone(), Some(true));

            // LHS: ¬(A ∧ B). `and` does not mutate its operands, so fresh clones
            // aren't strictly needed here, but built the same way as the RHS for
            // symmetry/readability.
            let and_ab = and(&build(&a_fa), &build(&b_fa), &mut crate::logging::Logging::new());
            let lhs = not(and_ab, &mut crate::logging::Logging::new());

            // RHS: ¬A ∨ ¬B. `or` totalizes its operands in place, so each side needs
            // its own fresh automaton -- these are one-shot, not reused afterward.
            let mut not_a = not(build(&a_fa).as_dfa(), &mut crate::logging::Logging::new()).into_automaton();
            let mut not_b = not(build(&b_fa).as_dfa(), &mut crate::logging::Logging::new()).into_automaton();
            let rhs = or(&mut not_a, &mut not_b, &mut crate::logging::Logging::new());

            let mut lhs_fa = lhs.automaton().fa.clone();
            lhs_fa.totalize(0);
            let mut rhs_fa = rhs.automaton().fa.clone();
            rhs_fa.totalize(0);
            prop_assert_eq!(equiv::language_equivalent(&lhs_fa, &rhs_fa), Ok(true));
        }

        /// ¬(A ∨ B) ≡ ¬A ∧ ¬B, computed with the real `and`/`or`/`not` from this file.
        /// `a_fa`/`b_fa` used to be trimmed first for the WB-001 reason documented on
        /// `de_morgan_not_and_equals_or_of_nots` above, and are no longer, for the same
        /// reason: WB-001 is fixed.
        #[test]
        fn de_morgan_not_or_equals_and_of_nots(
            a_fa in arb_partial_dfa(4, 2),
            b_fa in arb_partial_dfa(4, 2),
        ) {
            let build = |fa: &Fa| single_track(fa.clone(), Some(true));

            // LHS: ¬(A ∨ B).
            let or_ab = or(&mut build(&a_fa), &mut build(&b_fa), &mut crate::logging::Logging::new());
            let lhs = not(or_ab, &mut crate::logging::Logging::new());

            // RHS: ¬A ∧ ¬B (`and` doesn't mutate, so no extra `mut` needed here).
            let not_a = not(build(&a_fa).as_dfa(), &mut crate::logging::Logging::new()).into_automaton();
            let not_b = not(build(&b_fa).as_dfa(), &mut crate::logging::Logging::new()).into_automaton();
            let rhs = and(&not_a, &not_b, &mut crate::logging::Logging::new());

            let mut lhs_fa = lhs.automaton().fa.clone();
            lhs_fa.totalize(0);
            let mut rhs_fa = rhs.automaton().fa.clone();
            rhs_fa.totalize(0);
            prop_assert_eq!(equiv::language_equivalent(&lhs_fa, &rhs_fa), Ok(true));
        }
    }

    // ------------------------------------------------------- removeLeadingZeros

    /// A 2-track automaton over `{0,1} x {0,1}` accepting `Σ*`, with the given per-track
    /// numeration directions.
    fn universal_two_track(msd: Vec<Option<bool>>) -> Automaton {
        let mut d0 = BTreeMap::new();
        for sym in 0..4 {
            d0.insert(sym, vec![0]);
        }
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 4,
                o: vec![1],
                d: vec![d0],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            msd,
        )
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// The msd case: `Σ*` restricted to "the `x` track does not START with a zero" —
    /// plus, faithfully, the empty word (both states of the helper automaton accept; see
    /// [`remove_leading_zeros_helper`]'s docs).
    #[test]
    fn remove_leading_zeros_msd_requires_a_nonzero_first_digit() {
        let a = single_track(universal(), Some(true));
        let m = remove_leading_zeros(&a, &labels(&["x"])).unwrap();
        for word in [&[1][..], &[1, 0], &[1, 1], &[1, 0, 0]] {
            assert!(m.fa.accepts_word(word), "must accept {word:?}");
        }
        for word in [&[0][..], &[0, 1], &[0, 0], &[0, 1, 1]] {
            assert!(!m.fa.accepts_word(word), "must reject {word:?}");
        }
        assert!(
            m.fa.accepts_word(&[]),
            "the helper's start state is itself accepting -- epsilon is the \
             leading-zero-free representation of 0"
        );
    }

    /// The lsd case (`reverse(M, false)`, `:402-404`): the constraint moves to the LAST
    /// symbol, and the numeration direction itself is NOT flipped.
    #[test]
    fn remove_leading_zeros_lsd_requires_a_nonzero_last_digit() {
        let a = single_track(universal(), Some(false));
        let m = remove_leading_zeros(&a, &labels(&["x"])).unwrap();
        for word in [&[1][..], &[0, 1], &[1, 1], &[0, 0, 1]] {
            assert!(m.fa.accepts_word(word), "must accept {word:?}");
        }
        for word in [&[0][..], &[1, 0], &[0, 0], &[1, 1, 0]] {
            assert!(!m.fa.accepts_word(word), "must reject {word:?}");
        }
        assert_eq!(
            m.msd,
            vec![Some(false)],
            "reverse(_, false) must not flip NS"
        );
    }

    /// A track with no numeration system contributes `new AutomatonDFA(true)`
    /// (`:381-383`), i.e. no constraint at all.
    #[test]
    fn remove_leading_zeros_ignores_a_track_with_no_number_system() {
        let a = single_track(universal(), None);
        let m = remove_leading_zeros(&a, &labels(&["x"])).unwrap();
        for word in [&[][..], &[0], &[1], &[0, 0], &[0, 1]] {
            assert!(m.fa.accepts_word(word), "must still accept {word:?}");
        }
    }

    /// Multiple labels are OR'd, not AND'd (`:357-360`): the result requires **at least
    /// one** named track to be free of leading zeros.
    #[test]
    fn remove_leading_zeros_ors_the_per_track_constraints() {
        let a = universal_two_track(vec![Some(true), Some(true)]);
        let m = remove_leading_zeros(&a, &labels(&["x", "y"])).unwrap();
        // Symbol encoding is x + 2y (track 0 is least significant in `encode`).
        let sym = |x: i32, y: i32| a.encode(&[x, y]);
        assert!(m.fa.accepts_word(&[sym(1, 0), sym(0, 0)]), "x leads with 1");
        assert!(m.fa.accepts_word(&[sym(0, 1), sym(0, 0)]), "y leads with 1");
        assert!(m.fa.accepts_word(&[sym(1, 1), sym(0, 0)]), "both do");
        assert!(
            !m.fa.accepts_word(&[sym(0, 0), sym(1, 1)]),
            "neither track leads with a nonzero digit"
        );

        // ... and naming only ONE of the two constrains only that one.
        let m = remove_leading_zeros(&a, &labels(&["x"])).unwrap();
        assert!(m.fa.accepts_word(&[sym(1, 0), sym(0, 0)]));
        assert!(!m.fa.accepts_word(&[sym(0, 1), sym(0, 0)]));
    }

    /// `if (listOfLabels.isEmpty()) return A.clone();` (`:345-347`) — but only AFTER the
    /// (vacuous) validation, and it really is a clone: the argument is untouched.
    #[test]
    fn remove_leading_zeros_with_no_labels_is_a_clone() {
        let a = single_track(exactly_one(), Some(true));
        let m = remove_leading_zeros(&a, &[]).unwrap();
        for word in WORDS {
            assert_eq!(m.fa.accepts_word(word), a.fa.accepts_word(word), "{word:?}");
        }
        assert_eq!(
            m.fa.q, a.fa.q,
            "an untouched clone, not a rebuilt automaton"
        );
    }

    /// `validateLabels` runs FIRST (`:344`), before the empty-list short-circuit — so an
    /// unknown name is an error even though an empty list is fine.
    #[test]
    fn remove_leading_zeros_rejects_a_name_that_is_not_a_track() {
        let a = single_track(universal(), Some(true));
        let err = remove_leading_zeros(&a, &labels(&["nope"])).unwrap_err();
        assert_eq!(
            err,
            RemoveLeadingZerosError::NotFreeVariable("nope".to_string())
        );
        assert_eq!(
            err.to_string(),
            "Variable nope in the list of quantified variables is not a free variable."
        );
    }

    /// `removeLeadingZerosHelper`'s own guard (`:376-379`), reachable only for a
    /// malformed automaton whose `label` is longer than its `alphabet`.
    #[test]
    fn remove_leading_zeros_reports_an_out_of_range_input_index() {
        let mut a = single_track(universal(), Some(true));
        a.label.push("y".to_string());
        a.msd.push(Some(true));
        let err = remove_leading_zeros(&a, &labels(&["y"])).unwrap_err();
        assert_eq!(
            err,
            RemoveLeadingZerosError::InputIndexOutOfRange { n: 1, inputs: 1 }
        );
        assert_eq!(
            err.to_string(),
            "Cannot remove leading zeros for the 2-th input when A only has 1 inputs."
        );
    }

    /// A repeated label really does build and OR in the same helper twice (Java passes a
    /// `List`, not a `Set`) — idempotent, so the language is unchanged.
    #[test]
    fn remove_leading_zeros_tolerates_a_repeated_label() {
        let a = single_track(universal(), Some(true));
        let once = remove_leading_zeros(&a, &labels(&["x"])).unwrap();
        let twice = remove_leading_zeros(&a, &labels(&["x", "x"])).unwrap();
        for word in WORDS {
            assert_eq!(
                twice.fa.accepts_word(word),
                once.fa.accepts_word(word),
                "{word:?}"
            );
        }
    }

    /// The intersection really is with the ORIGINAL language, and the original is left
    /// untouched (Java's `and(A, M)` never mutates its operands).
    #[test]
    fn remove_leading_zeros_intersects_with_the_original_and_does_not_mutate_it() {
        // "the word contains a 1" — accepts `01`, which the fixup must then reject.
        let a = single_track(ends_with_one(), Some(true));
        let before = format!("{:?}", a.fa);
        let m = remove_leading_zeros(&a, &labels(&["x"])).unwrap();
        assert_eq!(
            format!("{:?}", a.fa),
            before,
            "the operand must survive unchanged"
        );
        for word in WORDS {
            let expected = a.fa.accepts_word(word) && word.first().is_none_or(|&d| d != 0);
            assert_eq!(m.fa.accepts_word(word), expected, "{word:?}");
        }
    }

    /// `remove_leading_zeros` on a custom-base-shaped track (an `all_reps` restriction
    /// attached, mirroring [`right_quotient_copies_both_halves_of_the_track_metadata_onto_the_second_operand`]
    /// for this function). Like `and` (which `remove_leading_zeros`'s final step calls,
    /// `:908`), this function correctly has no `apply_all_representations()` call of its
    /// own -- intersection cannot re-admit an invalid representation, so nothing needs
    /// re-applying. What DOES need checking is that the restriction's metadata (the
    /// `all_reps`/`msd` parallel-array pair) survives onto the result rather than being
    /// dropped or desynchronized.
    #[test]
    fn remove_leading_zeros_preserves_the_valid_representation_restriction_metadata() {
        let a = restricted(universal());
        let m = remove_leading_zeros(&a, &labels(&["x"])).unwrap();
        assert_eq!(m.msd, vec![Some(true)]);
        assert!(
            m.all_reps.iter().all(|r| r.is_some()),
            "the no_adjacent_ones restriction must still be attached to the x track"
        );

        // The restriction is not yet re-applied to `m.fa` itself (correctly -- see the
        // doc comment above), so `11` is still literally accepted here...
        assert!(m.fa.accepts_word(&[1, 1]));

        // ...but re-applying it now (as any real caller eventually does, since every
        // custom-base-backed automaton's `fa` is expected to already respect its own
        // `all_reps` by the time other code reads it) correctly intersects BOTH
        // constraints: leading-zero-free (from `remove_leading_zeros`) AND no-adjacent-
        // ones (from the restriction) -- `11` is now rejected, but `10`/`101` etc. still
        // accepted the same way `remove_leading_zeros_msd_requires_a_nonzero_first_digit`
        // already pins for the unrestricted case.
        let mut applied = m;
        applied.apply_all_representations(&mut crate::logging::Logging::new());
        assert!(
            !applied.fa.accepts_word(&[1, 1]),
            "11 violates no_adjacent_ones and must now be rejected"
        );
        assert!(applied.fa.accepts_word(&[1, 0, 1]));
        assert!(!applied.fa.accepts_word(&[0, 1]), "still leading-zero-free");
    }

    // ----------------------------------------------------------------- reverse

    #[test]
    fn reverse_reverses_the_language_and_leaves_number_systems_alone_when_false() {
        let mut a = single_track(ends_with_one(), Some(true));
        reverse(&mut a, false);

        // Reversing "ends with 1" gives "starts with 1".
        assert!(a.fa.accepts_word(&[1]));
        assert!(a.fa.accepts_word(&[1, 0, 0]));
        assert!(!a.fa.accepts_word(&[0, 1]));
        assert!(!a.fa.accepts_word(&[]));
        assert!(!a.fa.accepts_word(&[0]));
        assert_eq!(
            a.msd,
            vec![Some(true)],
            "reverse_msd = false must NOT flip the number system"
        );
    }

    #[test]
    fn reverse_with_reverse_msd_flips_arithmetic_tracks_only() {
        // Two tracks so the "skip the non-arithmetic track" half of `flipNS` is
        // exercised, and a deliberately NON-palindromic language so the reversal
        // itself is observable in the same test.
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 4,
                o: vec![0, 0, 1],
                d: vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![Some(true), None],
        );
        let e10 = a.encode(&[1, 0]);
        let e00 = a.encode(&[0, 0]);
        a.fa.d[0].insert(e10, vec![1]);
        a.fa.d[1].insert(e00, vec![2]);
        assert!(
            a.fa.accepts_word(&[e10, e00]),
            "sanity: L = {{ (1,0)(0,0) }}"
        );

        reverse(&mut a, true);

        assert_eq!(
            a.msd,
            vec![Some(false), None],
            "only the arithmetic track flips; the null-NumberSystem track is skipped"
        );
        assert!(a.fa.accepts_word(&[e00, e10]), "the reversed word");
        assert!(!a.fa.accepts_word(&[e10, e00]), "the original word is gone");
    }

    // -------------------------------------------------- fixLeadingZerosProblem

    #[test]
    fn fix_leading_zeros_problem_matches_the_java_characterization_test() {
        // Replicates AutomatonLogicalOpsTest.testFixLeadingZerosProblem exactly:
        //   q0 --0--> q1 --1--> qA(accept) --0/1--> qD(dead)
        //   q0 --1--> qD, q1 has NO 0-transition, qD self-loops on everything.
        // Language before: exactly {"01"}. After: 0*1.
        const Q0: usize = 0;
        const Q1: usize = 1;
        const QA: usize = 2;
        const QD: usize = 3;
        let mut d = vec![
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        ];
        d[Q0].insert(0, vec![Q1]);
        d[Q0].insert(1, vec![QD]);
        d[Q1].insert(1, vec![QA]);
        d[QA].insert(0, vec![QD]);
        d[QA].insert(1, vec![QD]);
        d[QD].insert(0, vec![QD]);
        d[QD].insert(1, vec![QD]);
        let mut a = single_track(
            Fa {
                true_false: None,
                q0: Q0,
                q: 4,
                alphabet_size: 2,
                o: vec![0, 0, 1, 0],
                d,
            },
            Some(true),
        );

        // Java's "before" sanity checks.
        assert!(a.fa.accepts_word(&[0, 1]), "\"01\" accepted before the fix");
        assert!(!a.fa.accepts_word(&[1]), "\"1\" rejected before the fix");
        assert!(!a.fa.accepts_word(&[0, 0, 1]), "\"001\" rejected before");

        fix_leading_zeros_problem(&mut a);

        assert!(!a.fa.accepts_word(&[]), "empty string still rejected");
        assert!(!a.fa.accepts_word(&[0]), "\"0\" alone still rejected");
        assert!(
            a.fa.accepts_word(&[1]),
            "\"1\" now accepted (0 leading zeros)"
        );
        assert!(a.fa.accepts_word(&[0, 1]), "\"01\" remains accepted");
        assert!(a.fa.accepts_word(&[0, 0, 1]), "\"001\" now accepted");
        assert!(a.fa.accepts_word(&[0, 0, 0, 1]), "\"0001\" now accepted");
        assert!(!a.fa.accepts_word(&[1, 1]), "\"11\" still rejected");
    }

    #[test]
    fn zero_reachable_states_forces_a_q0_self_loop_into_the_real_table() {
        // q0 has no zero-transition at all going in; the forced `(q0, zero) -> q0`
        // edge is what makes multiple leading zeros absorbable downstream.
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![BTreeMap::from([(1, vec![1])]), BTreeMap::new()],
        };
        assert_eq!(zero_reachable_states(&mut fa, 0), BTreeSet::from([0]));
        assert_eq!(fa.d[0].get(&0), Some(&vec![0]));
    }

    #[test]
    fn zero_reachable_states_is_transitive_and_does_not_duplicate_an_existing_loop() {
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 1,
            o: vec![0, 0, 1],
            d: vec![
                BTreeMap::from([(0, vec![0, 1])]),
                BTreeMap::from([(0, vec![2])]),
                BTreeMap::new(),
            ],
        };
        assert_eq!(zero_reachable_states(&mut fa, 0), BTreeSet::from([0, 1, 2]));
        assert_eq!(
            fa.d[0].get(&0),
            Some(&vec![0, 1]),
            "an existing self-loop must not be duplicated"
        );
    }

    #[test]
    fn fix_leading_zeros_problem_is_a_noop_on_a_zero_state_automaton() {
        let mut a = single_track(
            Fa {
                true_false: None,
                q0: 0,
                q: 0,
                alphabet_size: 2,
                o: vec![],
                d: vec![],
            },
            Some(true),
        );
        fix_leading_zeros_problem(&mut a);
        assert_eq!(a.fa.q, 0);
    }

    // ------------------------------------------------- fixTrailingZerosProblem

    #[test]
    fn fix_trailing_zeros_problem_closes_the_accepting_set_under_trailing_zeros() {
        // q0 --1--> q1 --0--> q2(accept) --0--> q2.  L = "1" followed by one or more 0s.
        // The backward-0 closure of the accepting set is {q2, q1}, so q1 becomes
        // accepting too and L widens to "1" followed by zero or more 0s.
        let mut d = vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
        d[0].insert(1, vec![1]);
        d[1].insert(0, vec![2]);
        d[2].insert(0, vec![2]);
        let mut a = single_track(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 0, 1],
                d,
            },
            Some(true),
        );
        assert!(!a.fa.accepts_word(&[1]), "sanity: \"1\" rejected before");
        assert!(a.fa.accepts_word(&[1, 0]));

        fix_trailing_zeros_problem(&mut a, &mut crate::logging::Logging::new());

        assert!(a.fa.accepts_word(&[1]), "\"1\" now accepted");
        assert!(a.fa.accepts_word(&[1, 0]));
        assert!(a.fa.accepts_word(&[1, 0, 0]));
        assert!(!a.fa.accepts_word(&[]));
        assert!(!a.fa.accepts_word(&[0]));
        assert!(!a.fa.accepts_word(&[1, 1]));
        assert!(!a.fa.accepts_word(&[0, 1]));
    }

    #[test]
    fn fix_trailing_zeros_problem_does_not_even_minimize_when_nothing_changed() {
        // No 0-transitions anywhere, so the backward-0 closure of the accepting set is
        // the accepting set itself and `altered` stays false. States 1 and 2 ARE
        // Myhill-Nerode equivalent (both accepting, both step to an accepting state on
        // symbol 1), so a mutant that dropped Java's `if (...)` guard at :322 and
        // always minimized would collapse them and shrink Q from 3 to 2.
        let mut d = vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
        d[0].insert(1, vec![1]);
        d[1].insert(1, vec![2]);
        d[2].insert(1, vec![2]);
        let mut a = single_track(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 1, 1],
                d,
            },
            Some(true),
        );
        fix_trailing_zeros_problem(&mut a, &mut crate::logging::Logging::new());
        assert_eq!(
            a.fa.q, 3,
            "no change was needed, so justMinimize must not have run"
        );
        assert_eq!(a.fa.o, vec![0, 1, 1]);
    }

    // --------------------- Tier-4 properties over the two standalone zero fixups (U31)
    //
    // Phase 4, U31. `wr_logic::quantify`'s `quantified_language_is_closed_under_leading_
    // zeros` checks the leading-zero fixup only as the tail of the ∃-projection pipeline,
    // on a 2-track automaton that has just been projected + determinized + minimized.
    // These cover the same two primitives as the STANDALONE `fixleadzero`/`fixtrailzero`
    // commands reach them: on an arbitrary, possibly nondeterministic, possibly partial
    // automaton read straight off disk (`wr-cli`'s `simple_transforms.rs` carries the
    // matching property at the command level, through the real file round trip).
    //
    // The two fixups are NOT mirror images and their properties are deliberately not
    // symmetric — see `fix_trailing_zeros_problem`'s own doc comment. The leading-zeros one
    // re-runs subset construction from the zero-closure of `q0` *after* forcing a
    // `(q0, zero) -> q0` self-loop into the table, so it CLOSES the language under
    // prepending zeros. The trailing-zeros one only widens the accepting set, so it is a
    // right quotient by `zero*`: it closes the language under REMOVING trailing zeros, and
    // not at all under adding them.

    /// Random single-track NFA over `{0, 1}` with genuinely missing transitions and
    /// genuinely multi-destination ones — a strictly wilder shape than `arb_partial_dfa`,
    /// because `fixleadzero`/`fixtrailzero` read their operand from a `.txt` file and so
    /// really can be handed an NFA.
    fn arb_partial_nfa(q_max: usize) -> impl Strategy<Value = Automaton> {
        (1..=q_max).prop_flat_map(move |q| {
            let o = prop::collection::vec(0i32..=1, q);
            let table = prop::collection::vec(
                prop::collection::vec(prop::collection::vec(any::<bool>(), q), 2),
                q,
            );
            (o, table).prop_map(move |(o, table)| {
                let d: Vec<BTreeMap<i32, Vec<usize>>> = table
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
                                (!dests.is_empty()).then_some((sym as i32, dests))
                            })
                            .collect()
                    })
                    .collect();
                single_track(
                    Fa {
                        true_false: None,
                        q0: 0,
                        q,
                        alphabet_size: 2,
                        o,
                        d,
                    },
                    Some(true),
                )
            })
        })
    }

    /// Can `fa`, from some state of `from`, reach an accepting state by reading `zero`
    /// zero-or-more times? A plain BFS over the zero-edges of the ORIGINAL table — the
    /// independent oracle for the trailing-zero fixup's right-quotient-by-`0*` semantics.
    fn reaches_accepting_by_zeros(fa: &Fa, from: &BTreeSet<usize>, zero: i32) -> bool {
        let mut seen = from.clone();
        let mut queue: VecDeque<usize> = from.iter().copied().collect();
        while let Some(s) = queue.pop_front() {
            if fa.is_accepting(s) {
                return true;
            }
            if let Some(dests) = fa.d[s].get(&zero) {
                for &t in dests {
                    if seen.insert(t) {
                        queue.push_back(t);
                    }
                }
            }
        }
        false
    }

    /// The EXACT language [`fix_leading_zeros_problem`] produces, decided from the
    /// ORIGINAL transition table by a hand-rolled subset-construction walk.
    ///
    /// The primitive is `Automaton.determinizeAndMinimize(IntSet)` (subset construction,
    /// then Valmari — both language-preserving) started from `Z0`, over the table `A'` =
    /// `A` with the single `(q0, zero) -> q0` edge [`zero_reachable_states`] force-writes
    /// into it, where `Z0` is `q0`'s `zero*`-closure in `A'`. So
    ///
    /// ```text
    /// L(fixed) = { w : δ_{A'}(Z0, w) ∩ F ≠ ∅ }
    /// ```
    ///
    /// and that is what this decides — with `Fa::d`/`Fa::is_accepting` lookups and
    /// nothing else. It never calls `fix_leading_zeros_problem`,
    /// `zero_reachable_states`, `determinize`, `minimize` or `equiv`.
    ///
    /// Note this is genuinely NOT a function of `L(A)` alone: the forced self-loop is
    /// usable only at `q0`, so which words it admits depends on which prefixes return
    /// the run to `q0` — a structural fact about `A`, not a language-level one. That is
    /// exactly why the closure/containment statements this replaced could not be
    /// tightened into an equality *about `L(A)`*, and why the equality has to be stated
    /// against the table instead.
    fn fix_leading_zeros_oracle(fa: &Fa, zero: i32, word: &[i32]) -> bool {
        // One subset-construction step over `A'` (the original table PLUS the forced
        // `(q0, zero) -> q0` edge, applied here rather than mutated in).
        let step = |cur: &BTreeSet<usize>, sym: i32| -> BTreeSet<usize> {
            let mut next = BTreeSet::new();
            for &s in cur {
                if let Some(dests) = fa.d[s].get(&sym) {
                    next.extend(dests.iter().copied());
                }
                if s == fa.q0 && sym == zero {
                    next.insert(fa.q0);
                }
            }
            next
        };

        // `Z0` — `q0`'s `zero*`-closure in `A'`, to a fixed point.
        let mut z0 = BTreeSet::from([fa.q0]);
        loop {
            let before = z0.len();
            let grown = step(&z0, zero);
            z0.extend(grown);
            if z0.len() == before {
                break;
            }
        }

        let mut cur = z0;
        for &sym in word {
            cur = step(&cur, sym);
        }
        cur.iter().any(|&s| fa.is_accepting(s))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Tier-4, `fixleadzero`: the **exact** characterization of what the fixup
        /// computes, plus the two corollaries the command is actually sold on.
        ///
        /// The load-bearing assertion is the first one:
        ///
        /// ```text
        /// L(fixed) = { w : δ_{A'}(Z0, w) ∩ F ≠ ∅ }
        /// ```
        ///
        /// checked word-for-word against [`fix_leading_zeros_oracle`] over EVERY word of
        /// length `0..=4` (an exhaustive 31-word sweep, not a sample), so the property has
        /// an upper bound as well as a lower one. An earlier draft asserted only closure
        /// and the two containments below; that was vacuous against this primitive's most
        /// likely failure mode — mutation-tested and confirmed: replacing the whole fixup
        /// with "return the 1-state `Σ*` automaton" satisfied every one of those
        /// assertions. It does not survive the equality.
        ///
        /// The corollaries, kept because they are the statements a reader of `fixleadzero`
        /// actually wants and they are checked with a *different* oracle (`accepts_word`
        /// on the untouched clone), are:
        ///
        /// * **Closure** — `w` is accepted iff `0w` is. (`zeroReachableStates`' forced
        ///   `(q0, zero) -> q0` self-loop makes `δ(Z0, zero) = Z0` in BOTH directions —
        ///   `⊆` from the closure, `⊇` from the self-loop.)
        /// * **Soundness** — nothing is lost: `L(A) ⊆ L(fixed)`, and more precisely every
        ///   word the ORIGINAL automaton accepted with some number of leading zeros is
        ///   accepted without them: `0^k w ∈ L(A) ⟹ w ∈ L(fixed)`.
        ///
        /// Those stay one-sided on purpose: the forced self-loop is a real mutation of
        /// the transition TABLE (see `zero_reachable_states`' doc), so `L(fixed)` can
        /// legitimately contain words no leading-zero padding of `L(A)` produces. The
        /// upper bound lives in the equality above, where it can be stated exactly.
        #[test]
        fn fix_leading_zeros_is_exactly_subset_construction_from_the_zero_closure(
            a in arb_partial_nfa(4),
            probe in prop::collection::vec(0i32..2, 0..4),
        ) {
            let original = a.clone();
            let mut fixed = a;
            fix_leading_zeros_problem(&mut fixed);

            let zero = 0; // encode([0]) over the alphabet {0, 1}

            for w in all_digit_words(&[0, 1], 4) {
                prop_assert_eq!(
                    fixed.fa.accepts_word(&w),
                    fix_leading_zeros_oracle(&original.fa, zero, &w),
                    "fixleadzero disagrees with subset construction from Z0 on {:?}", w
                );
            }

            let mut padded = vec![zero];
            padded.extend_from_slice(&probe);
            prop_assert_eq!(
                fixed.fa.accepts_word(&probe),
                fixed.fa.accepts_word(&padded),
                "not closed under a leading zero at {:?}", probe
            );

            if original.fa.accepts_word(&probe) {
                prop_assert!(fixed.fa.accepts_word(&probe), "the fixup lost a word");
            }
            for k in 0..=4usize {
                let mut with_zeros = vec![zero; k];
                with_zeros.extend_from_slice(&probe);
                if original.fa.accepts_word(&with_zeros) {
                    prop_assert!(
                        fixed.fa.accepts_word(&probe),
                        "0^{} {:?} was accepted, so {:?} must be after the fixup",
                        k, probe, probe
                    );
                }
            }
        }

        /// Tier-4, `fixtrailzero`: the EXACT characterization, which for this fixup is
        /// available (unlike its leading-zeros sibling, which mutates the transition table).
        /// `setStatesReachableToFinalStatesByZeros` only widens the accepting set, so
        ///
        /// ```text
        /// L(fixed) = { w : ∃k ≥ 0, w·0^k ∈ L(A) }
        /// ```
        ///
        /// i.e. a right quotient by `zero*`. The oracle decides that exactly — run `w` on
        /// the ORIGINAL automaton, then ask a from-scratch zero-edge BFS whether any state
        /// so reached can reach acceptance on zeros ([`reaches_accepting_by_zeros`]) — with
        /// no length bound to guess, and with no call to the fixup or to `just_minimize`.
        ///
        /// The corollary spelled out separately below is the asymmetry `CLAUDE.md`'s L1
        /// entry records: the result is closed under REMOVING a trailing zero and NOT under
        /// adding one. Asserting the mirror of the leading-zeros property here would be
        /// wrong, not merely unproven.
        ///
        /// # Two deliberate generator constraints, both faithful-behaviour driven
        ///
        /// * **Deterministic input.** Unlike its leading-zeros sibling (which re-runs
        ///   subset construction and therefore handles an NFA natively), this fixup ends in
        ///   `justMinimize`, whose `convertNFAtoDFA()` step throws `"Unexpected NFA instead
        ///   of DFA."` on genuine nondeterminism — Java and this port alike (see
        ///   `just_minimize`). So a `partial DFA` generator is the primitive's actual
        ///   domain; feeding it an NFA would test the ported rejection, not the quotient.
        /// * ~~**Trimmed input.**~~ **REMOVED, WB-001 is fixed.** `just_minimize`
        ///   establishes none of `minimize`'s `q0`-reachability precondition (faithfully
        ///   — Java's `justMinimize` does not trim either), so while
        ///   `docs/WALNUT-BUGS.md` WB-001 was live an unreachable state could legitimately
        ///   trigger it, and this property trimmed its operand for the same reason
        ///   `not_matches_the_complement_oracle` did. `walnut-java` commit `14509f1`
        ///   fixes it (ported in [`crate::minimize::minimize`]), so the operand is now
        ///   used raw — which matters here more than elsewhere, because an untrimmed
        ///   hand-written `.txt` is exactly what the real `fixtrailzero` command meets
        ///   (see `wr_cli::simple_transforms`'s own end-to-end test of this path).
        ///   Trimming never changed the quotient either way: a state that cannot reach
        ///   acceptance contributes to neither side of the equation.
        #[test]
        fn fix_trailing_zeros_is_exactly_the_right_quotient_by_zeros(
            fa in arb_partial_dfa(4, 2),
            probe in prop::collection::vec(0i32..2, 0..4),
        ) {
            let original = single_track(fa, Some(true));
            let mut fixed = original.clone();
            fix_trailing_zeros_problem(&mut fixed, &mut crate::logging::Logging::new());

            let zero = 0;
            let reached: BTreeSet<usize> = {
                let mut cur: BTreeSet<usize> = BTreeSet::from([original.fa.q0]);
                for &sym in &probe {
                    let mut next = BTreeSet::new();
                    for &s in &cur {
                        if let Some(dests) = original.fa.d[s].get(&sym) {
                            next.extend(dests.iter().copied());
                        }
                    }
                    cur = next;
                }
                cur
            };
            let expected = reaches_accepting_by_zeros(&original.fa, &reached, zero);
            prop_assert_eq!(
                fixed.fa.accepts_word(&probe), expected,
                "trailing-zero fixup disagrees with the right quotient by 0* at {:?}", probe
            );

            // The corollary: closed under REMOVING a trailing zero (never under adding —
            // that direction is genuinely false and is not asserted).
            let mut with_zero = probe.clone();
            with_zero.push(zero);
            if fixed.fa.accepts_word(&with_zero) {
                prop_assert!(
                    fixed.fa.accepts_word(&probe),
                    "{:?}0 accepted but {:?} is not", probe, probe
                );
            }
        }
    }

    // ------------------------------------- removeStatesWithOutputRebuild (Java test)

    #[test]
    fn remove_states_with_output_rebuild_matches_the_java_characterization_test() {
        // Replicates AutomatonLogicalOpsTest.testRemoveStatesWithOutputRebuild.
        //   state0 --0--> state1 (output 1: kept)
        //   state1 --0--> state0 (output 0: removed)
        //   state2 --0--> state2 (output 0, self-loop: removed)
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 1,
            o: vec![0, 1, 0],
            d: vec![
                BTreeMap::from([(0, vec![1])]),
                BTreeMap::from([(0, vec![0])]),
                BTreeMap::from([(0, vec![2])]),
            ],
        };

        remove_states_with_output_rebuild(&mut fa, 0);

        // Despite the javadoc's "deletes all states" claim, Q and O are untouched --
        // only the transitions INTO those states are pruned.
        assert_eq!(fa.q, 3);
        assert_eq!(fa.o, vec![0, 1, 0]);
        assert_eq!(fa.d[0].get(&0), Some(&vec![1]));
        assert_eq!(fa.d[1].get(&0), None);
        assert_eq!(fa.d[2].get(&0), None);
    }

    #[test]
    fn remove_states_with_output_rebuild_keys_only_on_the_first_destination() {
        // The quirk spelled out in this function's doc comment, which the Java
        // characterization test (all-deterministic) cannot distinguish: an entry whose
        // FIRST destination survives is kept whole even though its second destination
        // is removable, and vice versa.
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![1, 0, 1],
            d: vec![
                BTreeMap::from([(0, vec![2, 1]), (1, vec![1, 2])]),
                BTreeMap::new(),
                BTreeMap::new(),
            ],
        };
        remove_states_with_output_rebuild(&mut fa, 0);
        assert_eq!(
            fa.d[0].get(&0),
            Some(&vec![2, 1]),
            "first dest (2) has output 1, so the WHOLE entry survives -- including \
             the removable second dest (1)"
        );
        assert_eq!(
            fa.d[0].get(&1),
            None,
            "first dest (1) has output 0, so the whole entry goes -- including the \
             surviving second dest (2)"
        );
    }

    // ------------------------------------------------------------------ combine

    /// TOTAL 2-state DFA over `{0,1}` accepting exactly the words of EVEN length
    /// (state 0 = even, accepting; state 1 = odd, non-accepting).
    fn even_length() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![0]);
        d1.insert(1, vec![0]);
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 0],
            d: vec![d0, d1],
        }
    }

    /// Walks `fa` from `q0` along `word` and returns the output at the reached state
    /// (there is no shared "evaluate a DFAO" helper yet in this crate -- `Fa::
    /// accepts_word` only answers accept/reject -- so this test-local walker reads the
    /// raw output value `combine` produces).
    fn output_after(fa: &Fa, word: &[i32]) -> i32 {
        let mut state = fa.q0;
        for &sym in word {
            state = fa.d[state][&sym][0];
        }
        fa.o[state]
    }

    #[test]
    fn combine_two_automata_matches_the_hand_derived_truth_table() {
        // A = "ends with 1" (from `ends_with_one`, already TOTAL & deterministic).
        // next = "even length" (`even_length`, also TOTAL & deterministic), so
        // `combine`'s own `totalize` calls are no-ops and don't complicate the
        // expected output table.
        //
        // Per `AutomatonLogicalOps.combine`'s semantics (see this function's doc
        // comment): A's accepting states get rewritten to `outputs[0]`; then, for each
        // subsequent automaton (`next`), states where `next` accepts get forced to that
        // step's `outputs[i]`, overriding whatever A/earlier steps produced; states
        // where `next` doesn't accept keep the running value. So here: even-length
        // words always output 20 (evenness checked "last" wins, since it's the only
        // `next`); odd-length words output 10 if they end in `1`, else the untouched
        // original non-accepting output, `0`.
        let a = single_track(ends_with_one(), Some(true));
        let next = single_track(even_length(), Some(true));

        let result = combine(
            &a,
            vec![next],
            &[10, 20],
            &mut crate::logging::Logging::new(),
        );

        assert_eq!(output_after(&result.fa, &[]), 20, "\"\": even length");
        assert_eq!(
            output_after(&result.fa, &[1]),
            10,
            "\"1\": odd, ends with 1"
        );
        assert_eq!(
            output_after(&result.fa, &[0]),
            0,
            "\"0\": odd, doesn't end with 1"
        );
        assert_eq!(output_after(&result.fa, &[1, 1]), 20, "\"11\": even length");
        assert_eq!(output_after(&result.fa, &[0, 1]), 20, "\"01\": even length");
        assert_eq!(
            output_after(&result.fa, &[0, 1, 1]),
            10,
            "\"011\": odd, ends with 1"
        );
        assert_eq!(
            output_after(&result.fa, &[0, 1, 0]),
            0,
            "\"010\": odd, doesn't end with 1"
        );
    }

    #[test]
    fn combine_single_automaton_no_subautomata_just_rewrites_accepting_states() {
        // An empty `subautomata` list: the `while` loop never runs, so this is just
        // "rewrite A's accepting states to `outputs[0]`, leave everything else."
        let a = single_track(ends_with_one(), Some(true));
        let result = combine(&a, vec![], &[42], &mut crate::logging::Logging::new());
        assert_eq!(output_after(&result.fa, &[1]), 42, "accepting: rewritten");
        assert_eq!(
            output_after(&result.fa, &[0]),
            0,
            "non-accepting: original output untouched"
        );
    }

    // ---------------------------------------------------------------- quotients

    /// `L = {"01"}` over `{0,1}`, single track labeled `"x"`.
    fn word_01() -> Automaton {
        let mut d = vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
        d[0].insert(0, vec![1]);
        d[1].insert(1, vec![2]);
        single_track(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 0, 1],
                d,
            },
            Some(true),
        )
    }

    #[test]
    #[should_panic(
        expected = "Second A's alphabet must be a subset of the first A's alphabet for right quotient."
    )]
    fn right_quotient_panics_when_the_second_alphabet_is_not_a_subset() {
        // Replicates AutomatonLogicalOpsTest.testRightQuotientThrowsWhenSecondAlphabetNotSubset:
        // A has one track, B has two, so `isSubsetA` fails on arity alone.
        let a = single_track(exactly_one(), Some(true));
        let b = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 4,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![Some(true), Some(true)],
        );
        let _ = right_quotient(&a, &b, false, &mut crate::logging::Logging::new());
    }

    #[test]
    #[should_panic(
        expected = "Second A's alphabet must be a subset of the first A's alphabet for right quotient."
    )]
    fn right_quotient_subset_guard_is_direction_sensitive() {
        // Mutation-tested gap: the Java-replicating test above uses an ARITY mismatch,
        // which `is_subset_alphabet` rejects in EITHER direction -- so swapping the
        // guard's arguments would leave it green. Here both operands have arity 1 and
        // B's alphabet is a strict SUPERset of A's, so only the correct direction
        // (`isSubsetA(B, A)`, AutomatonLogicalOps.java:182) rejects it.
        let a = single_track(exactly_one(), Some(true));
        let b = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 3,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(
            is_subset_alphabet(&a.alphabet, &b.alphabet),
            "the OPPOSITE containment does hold, so a swapped guard would pass"
        );
        let _ = right_quotient(&a, &b, false, &mut crate::logging::Logging::new());
    }

    #[test]
    fn right_quotient_skip_subset_check_bypasses_the_guard() {
        // Exactly the shape `right_quotient_subset_guard_is_direction_sensitive`
        // rejects -- B declared over a strict SUPERSET of A's alphabet -- but with
        // `skip_subset_check = true` (the flag `leftQuotient` passes, :248). B only
        // ever *uses* digits that A also has, so once the guard is out of the way the
        // computation goes through and lands on the same hand-derived answer as the
        // in-alphabet case: {"01"} / {"1"} = {"0"}.
        let a = word_01();
        let b = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 3,
                o: vec![0, 1],
                // Symbol 1 is digit 1 under B's own [0, 1, 2]; digit 2 is never used.
                d: vec![BTreeMap::from([(1, vec![1])]), BTreeMap::new()],
            },
            vec![vec![0, 1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(
            !is_subset_alphabet(&b.alphabet, &a.alphabet),
            "precondition: the guard WOULD reject this pair if it ran"
        );

        let m = right_quotient(&a, &b, true, &mut crate::logging::Logging::new());

        assert!(m.fa.accepts_word(&[0]));
        assert!(!m.fa.accepts_word(&[]));
        assert!(!m.fa.accepts_word(&[1]));
        assert!(!m.fa.accepts_word(&[0, 1]));
    }

    #[test]
    fn right_quotient_matches_the_hand_derived_quotient() {
        // L(A) = {"01"}, L(B) = {"1"} => L(A)/L(B) = {"0"}.
        //
        // B's alphabet is written as [1, 0] -- the same SET as A's [0, 1] (so the
        // subset guard passes) but in the OPPOSITE order, so B's symbol ids mean
        // different digits than A's. That makes `rightQuotient`'s re-encode step
        // (:193-207) load-bearing: skipping it would leave B recognizing {"0"} in A's
        // alphabet and collapse the answer to the empty language.
        let a = word_01();
        let b = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                // In B's own alphabet [1, 0], symbol 0 IS the digit 1.
                o: vec![0, 1],
                d: vec![BTreeMap::from([(0, vec![1])]), BTreeMap::new()],
            },
            vec![vec![1, 0]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert_eq!(b.encode(&[1]), 0, "sanity: L(B) = {{\"1\"}}");

        let m = right_quotient(&a, &b, false, &mut crate::logging::Logging::new());

        assert!(m.fa.accepts_word(&[0]), "\"0\" is in L(A)/L(B)");
        assert!(!m.fa.accepts_word(&[]));
        assert!(!m.fa.accepts_word(&[1]));
        assert!(!m.fa.accepts_word(&[0, 1]));
        assert!(!m.fa.accepts_word(&[0, 0]));
    }

    #[test]
    fn right_quotient_labels_its_operands_before_the_cross_product() {
        // Mutation-tested gap: with pre-labeled operands, deleting `T.randomLabel()`
        // (:219) / `otherClone.setLabel(...)` (:220) changes nothing, because both
        // already carry a matching label. But the real CLI path feeds `rightQuotient`
        // UNBOUND automata -- `Automaton.readAutomatonFromFile` (`Automaton.java:148-150`)
        // never assigns labels, and `Main/Commands/Quotient.java:10-12` passes its
        // results straight in -- and there the labeling step is what keeps
        // `crossProduct`'s "must have labeled inputs" guard satisfied at all.
        let mut a = word_01();
        a.label = Vec::new();
        assert!(
            !a.is_bound(),
            "the shape a .txt-sourced automaton really has"
        );
        let mut b = {
            let mut d = vec![BTreeMap::new(), BTreeMap::new()];
            d[0].insert(1, vec![1]);
            single_track(
                Fa {
                    true_false: None,
                    q0: 0,
                    q: 2,
                    alphabet_size: 2,
                    o: vec![0, 1],
                    d,
                },
                Some(true),
            )
        };
        b.label = Vec::new();

        let m = right_quotient(&a, &b, false, &mut crate::logging::Logging::new());

        // Same hand-derived answer as the bound case: {"01"} / {"1"} = {"0"}.
        assert!(m.fa.accepts_word(&[0]));
        assert!(!m.fa.accepts_word(&[]));
        assert!(!m.fa.accepts_word(&[1]));
        assert!(!m.fa.accepts_word(&[0, 1]));
    }

    #[test]
    #[should_panic(
        expected = "Second A's alphabet must be a subset of the first A's alphabet for left quotient."
    )]
    fn left_quotient_panics_when_the_second_alphabet_is_not_a_subset() {
        // Replicates AutomatonLogicalOpsTest.testLeftQuotientThrowsWhenSecondAlphabetNotSubset
        // (renamed from testLeftQuotientThrowsWhenFirstAlphabetNotSubset by WB-010's fix,
        // c5ff914). Track-count mismatch (2 vs 1) means `is_subset_alphabet` fails
        // immediately regardless of direction, so this case alone can't distinguish a
        // correct-direction check from a backwards one -- see
        // `left_quotient_computes_the_correct_result_when_the_second_alphabet_is_a_subset`
        // (WB-010) for that.
        let a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 4,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".to_string(), "y".to_string()],
            vec![Some(true), Some(true)],
        );
        let b = single_track(exactly_one(), Some(true));
        let _ = left_quotient(&a, &b, &mut crate::logging::Logging::new());
    }

    /// **WB-010** (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit
    /// `c5ff914` (`bugfix/wb-010`): before the fix, this exact shape -- `A`'s alphabet a
    /// strict SUPERset of `B`'s, arity 1 on both sides -- was wrongly REJECTED, because
    /// the old (backwards) guard required `A ⊆ B`, not `B ⊆ A`. Replicates
    /// `AutomatonLogicalOpsTest.testLeftQuotientComputesCorrectResultWhenSecondAlphabetIsASubset`.
    #[test]
    fn left_quotient_computes_the_correct_result_when_the_second_alphabet_is_a_subset() {
        // A over {0,1,2}, language exactly {"1", "00", "01"}: q0=a0; a0-0->a1, a0-1->a2;
        // a1-0->a3, a1-1->a4; outputs [0,0,1,1,1] (a2, a3, a4 accept).
        let a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 5,
                alphabet_size: 3,
                o: vec![0, 0, 1, 1, 1],
                d: vec![
                    BTreeMap::from([(0, vec![1]), (1, vec![2])]),
                    BTreeMap::from([(0, vec![3]), (1, vec![4])]),
                    BTreeMap::new(),
                    BTreeMap::new(),
                    BTreeMap::new(),
                ],
            },
            vec![vec![0, 1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        // B over {0,1} (a genuine subset of A's alphabet, not equal to it as a set),
        // language exactly {"0"}: b0-0->b1; outputs [0, 1].
        let b = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![BTreeMap::from([(0, vec![1])]), BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(
            is_subset_alphabet(&b.alphabet, &a.alphabet),
            "sanity: the real precondition (B subset A) genuinely holds here"
        );
        assert!(
            !is_subset_alphabet(&a.alphabet, &b.alphabet),
            "sanity: but the alphabets are NOT equal as sets -- the old (backwards) \
             guard would have wrongly rejected this pair"
        );

        let m = left_quotient(&a, &b, &mut crate::logging::Logging::new());

        // L(B) \ L(A) = { z : exists w in L(B), wz in L(A) } = { z : "0"+z in L(A) }:
        // "0"+"" = "0" not in L(A); "0"+"0" = "00" in L(A) => "0" accepted; "0"+"1" =
        // "01" in L(A) => "1" accepted.
        assert!(!m.fa.accepts_word(&[]), "empty string should be rejected");
        assert!(m.fa.accepts_word(&[0]), "\"0\" should be accepted");
        assert!(m.fa.accepts_word(&[1]), "\"1\" should be accepted");
        assert!(!m.fa.accepts_word(&[2]), "\"2\" should be rejected");
        assert!(!m.fa.accepts_word(&[0, 0]), "\"00\" should be rejected");
        assert!(!m.fa.accepts_word(&[0, 1]), "\"01\" should be rejected");
    }

    /// **WB-010**, a NON-PREFIX subset alphabet — found missing by adversarial review of
    /// the sibling `left_quotient_computes_the_correct_result_when_the_second_alphabet_is_a_subset`
    /// above, whose `B` alphabet `{0,1}` is a PREFIX of `A`'s `{0,1,2}`: at every symbol,
    /// `B`'s local index equals the digit value, and that digit value sits at the SAME
    /// local index in `A` too, so `right_quotient`'s internal re-encode
    /// (`a.encode(&other_clone.decode(sym))`) is the IDENTITY function throughout —
    /// mutation-proven (deleting the re-encode entirely left the sibling test, its
    /// property-test analogue, and both new differential tests all green). `B`'s
    /// alphabet here is `{1,2}`: `B`'s local index 1 (digit value `2`) must re-encode to
    /// `A`'s local index 2 for digit value `2` — a genuine `1 -> 2` index shift, not an
    /// identity map, so a broken or deleted re-encode step changes this test's answer.
    #[test]
    fn left_quotient_computes_the_correct_result_when_the_second_alphabet_is_a_non_prefix_subset() {
        // A over {0,1,2}, language exactly {"20", "21"}: q0=a0; a0-2->a1; a1-0->a2,
        // a1-1->a3; outputs [0,0,1,1] (a2, a3 accept).
        let a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 4,
                alphabet_size: 3,
                o: vec![0, 0, 1, 1],
                d: vec![
                    BTreeMap::from([(2, vec![1])]),
                    BTreeMap::from([(0, vec![2]), (1, vec![3])]),
                    BTreeMap::new(),
                    BTreeMap::new(),
                ],
            },
            vec![vec![0, 1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        // B over {1,2} (local index 0 = digit value 1, local index 1 = digit value 2) --
        // a genuine, non-prefix subset of A's alphabet -- language exactly {"2"}:
        // b0 -(local idx 1, value 2)-> b1 (accept); no transition on local idx 0.
        let b = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![BTreeMap::from([(1, vec![1])]), BTreeMap::new()],
            },
            vec![vec![1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(
            is_subset_alphabet(&b.alphabet, &a.alphabet),
            "sanity: B's alphabet {{1,2}} is a genuine (non-prefix) subset of A's {{0,1,2}}"
        );

        let m = left_quotient(&a, &b, &mut crate::logging::Logging::new());

        // L(B) \ L(A) = { z : "2"+z in L(A) } = { z : "2"+z in {"20","21"} } = {"0","1"}.
        assert!(!m.fa.accepts_word(&[]), "empty string should be rejected");
        assert!(m.fa.accepts_word(&[0]), "\"0\" should be accepted");
        assert!(m.fa.accepts_word(&[1]), "\"1\" should be accepted");
        assert!(!m.fa.accepts_word(&[2]), "\"2\" should be rejected");
    }

    #[test]
    fn left_quotient_matches_the_hand_derived_quotient() {
        // L(A) = {"01"}, L(B) = {"0"} => { z : exists w in L(B), wz in L(A) } = {"1"}.
        let a = word_01();
        let b = {
            let mut d = vec![BTreeMap::new(), BTreeMap::new()];
            d[0].insert(0, vec![1]);
            single_track(
                Fa {
                    true_false: None,
                    q0: 0,
                    q: 2,
                    alphabet_size: 2,
                    o: vec![0, 1],
                    d,
                },
                Some(true),
            )
        };

        let m = left_quotient(&a, &b, &mut crate::logging::Logging::new());

        assert!(m.fa.accepts_word(&[1]), "\"1\" is in L(B) \\ L(A)");
        assert!(!m.fa.accepts_word(&[]));
        assert!(!m.fa.accepts_word(&[0]));
        assert!(!m.fa.accepts_word(&[0, 1]));
        assert!(!m.fa.accepts_word(&[1, 1]));
        assert_eq!(
            m.msd,
            vec![Some(true)],
            "of the three internal `reverse(_, true)` calls, only two apply to the \
             SAME automaton (`reverse_and_canonize(a)`, then the final \
             `reverse(&mut m, true)`) -- an earlier version of this comment miscounted \
             this as odd; it's an EVEN count, which is what nets out to the original \
             msd/lsd direction"
        );
    }

    /// The exact panic text the (now-fixed) guard produces on WB-010's own trigger
    /// shape. Shared with
    /// `left_quotient_rejects_the_wb_010_shape_before_any_computation`, which must
    /// distinguish THIS failure (the guard's own, correct-direction rejection) from any
    /// other panic.
    const WB_010_GUARD_PANIC: &str =
        "Second A's alphabet must be a subset of the first A's alphabet for left quotient.";

    /// **WB-010** (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit
    /// `c5ff914` (`bugfix/wb-010`). Before the fix, `A` over `{0,1}`/`B` over `{0,1,2}`
    /// passed the (backwards) guard -- `is_subset_alphabet(&a.alphabet, &b.alphabet)` is
    /// true -- and then panicked deep inside `right_quotient`'s internal re-encode when
    /// it hit `B`'s digit `2`, which has no counterpart in `A`'s alphabet (this crate's
    /// [`Automaton::encode`] turns Java's silent `indexOf == -1` corruption into a clean
    /// panic -- see this function's doc comment -- but the underlying guard defect was
    /// the same either way). After the fix, the guard itself -- now checking
    /// `is_subset_alphabet(&b.alphabet, &a.alphabet)` -- catches this exact shape and
    /// rejects it CLEANLY, before `right_quotient`/the internal re-encode ever runs.
    /// Replicates
    /// `AutomatonLogicalOpsTest.testLeftQuotientRejectsWhenSecondAlphabetIsNotActuallyASubset`.
    #[test]
    #[should_panic(
        expected = "Second A's alphabet must be a subset of the first A's alphabet for left quotient."
    )]
    fn left_quotient_rejects_the_wb_010_trigger_cleanly() {
        let a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 2,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        // The exact automaton `left_quotient_panics_on_the_wb_010_trigger_the_guard_misses`
        // (this test's pre-fix name) used, kept unchanged: L(b) = {"2"}, state0
        // (non-accepting, initial) --2--> state1 (accepting, dead end). Before the fix,
        // `b`'s transitions mattered (the panic only fired if digit `2` survived
        // `reverse_and_canonize` to reach the internal re-encode). After the fix, the
        // guard rejects on the ALPHABETS alone, before any of `b`'s transitions are
        // ever inspected -- so this shape is no longer load-bearing, just preserved for
        // continuity with the bug's original repro.
        let b = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 3,
                o: vec![0, 1],
                d: vec![[(2, vec![1])].into_iter().collect(), BTreeMap::new()],
            },
            vec![vec![0, 1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(
            is_subset_alphabet(&a.alphabet, &b.alphabet),
            "sanity: the OLD (backwards) guard (A subset B) would have wrongly passed"
        );
        assert!(
            !is_subset_alphabet(&b.alphabet, &a.alphabet),
            "sanity: the containment the FIXED guard (and rightQuotient's re-encode) \
             actually needs (B subset A) does NOT hold -- so the fixed guard rejects"
        );
        let _ = left_quotient(&a, &b, &mut crate::logging::Logging::new());
    }

    // ------------------------------------------- Tier-4 properties over the quotients
    //
    // Phase 4, U31. Everything below compares the two quotient constructions against a
    // WORD-LEVEL brute-force definition of the quotient, computed with nothing but
    // `Fa::accepts_word` on the ORIGINAL operands. That oracle never calls
    // `right_quotient`/`left_quotient`, `and`, `Automaton::is_empty`, `equiv`, or any
    // other piece of the machinery under test, so it is genuinely independent rather
    // than a re-derivation of the same construction.

    /// Random single-track partial DFAs over an ARBITRARY digit list (`arb_partial_dfa`
    /// is hard-wired to `{0, 1}` through [`single_track`]). The digits are the track's
    /// alphabet, so `alphabet_size == digits.len()` and encoded symbol `i` means digit
    /// `digits[i]` — see `automaton.rs`'s module docs.
    fn arb_partial_automaton_over(
        q_max: usize,
        digits: Vec<i32>,
    ) -> impl Strategy<Value = Automaton> {
        let alphabet_size = digits.len();
        (1..=q_max).prop_flat_map(move |q| {
            let digits = digits.clone();
            let o = prop::collection::vec(0i32..=1, q);
            let trans = prop::collection::vec(
                prop::collection::vec(prop::option::of(0usize..q), alphabet_size),
                q,
            );
            (o, trans).prop_map(move |(o, trans)| {
                let d = trans
                    .iter()
                    .map(|row| {
                        row.iter()
                            .enumerate()
                            .filter_map(|(sym, dest)| dest.map(|d| (sym as i32, vec![d])))
                            .collect::<BTreeMap<i32, Vec<usize>>>()
                    })
                    .collect();
                Automaton::new(
                    Fa {
                        true_false: None,
                        q0: 0,
                        q,
                        alphabet_size,
                        o,
                        d,
                    },
                    vec![digits.clone()],
                    vec!["x".to_string()],
                    vec![Some(true)],
                )
            })
        })
    }

    /// Every word of digits from `digits` with length `0..=max_len`, in the caller's own
    /// digit space (NOT encoded) — the enumeration both oracles below quantify over.
    fn all_digit_words(digits: &[i32], max_len: usize) -> Vec<Vec<i32>> {
        let mut out = vec![Vec::new()];
        let mut frontier = vec![Vec::new()];
        for _ in 0..max_len {
            let mut next = Vec::new();
            for w in &frontier {
                for &d in digits {
                    let mut w2 = w.clone();
                    w2.push(d);
                    next.push(w2);
                }
            }
            out.extend(next.iter().cloned());
            frontier = next;
        }
        out
    }

    /// `a`'s encoded symbol for digit `d`, or `None` if `a`'s track cannot represent it.
    /// (`Automaton::encode` panics instead — deliberately, see its doc comment — which is
    /// no use to an oracle that has to reason ABOUT that shape; WB-010's generator below
    /// feeds digits one operand does not have.)
    fn symbol_for_digit(a: &Automaton, d: i32) -> Option<i32> {
        a.alphabet[0].iter().position(|&x| x == d).map(|i| i as i32)
    }

    /// The set of states `a` can be in after reading the digit word `w` from `start`
    /// (a plain NFA simulation over `Fa::d`, nothing else).
    fn digit_run(a: &Automaton, start: &BTreeSet<usize>, w: &[i32]) -> BTreeSet<usize> {
        // A quotient whose result language is empty can come back with ZERO states (see
        // `minimize`, which drops every non-co-reachable state), and `Fa`'s own
        // `accepts_word`/`is_accepting` would index `o[q0]` out of bounds on that shape.
        // The oracle simply reports "no run", which is the right answer for L = ∅.
        let mut cur: BTreeSet<usize> = start.iter().copied().filter(|&p| p < a.fa.q).collect();
        for &d in w {
            let Some(sym) = symbol_for_digit(a, d) else {
                return BTreeSet::new();
            };
            let mut next = BTreeSet::new();
            for &p in &cur {
                if let Some(dests) = a.fa.d[p].get(&sym) {
                    next.extend(dests.iter().copied());
                }
            }
            cur = next;
        }
        cur
    }

    /// Does `a`, started in any state of `start`, accept the digit word `w`?
    fn accepts_digit_word_from(a: &Automaton, start: &BTreeSet<usize>, w: &[i32]) -> bool {
        digit_run(a, start, w).iter().any(|&p| a.fa.is_accepting(p))
    }

    /// Every `(a-state, b-state)` pair reachable from `(a_start, b.q0)` by reading ONE
    /// common digit word in both — a from-scratch synchronized-product reachability BFS,
    /// written here in the test module and calling nothing from `crate::product`,
    /// `crate::equiv` or the quotient constructions themselves.
    ///
    /// This is what lets both oracles below be *exact* rather than sampled: quantifying
    /// "∃y ∈ L(B) with xy ∈ L(A)" over the pair graph terminates, where enumerating
    /// candidate `y` words would have to guess a length bound.
    fn reachable_pairs(
        a: &Automaton,
        a_start: usize,
        b: &Automaton,
        digits: &[i32],
    ) -> BTreeSet<(usize, usize)> {
        let mut seen: BTreeSet<(usize, usize)> = BTreeSet::new();
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        seen.insert((a_start, b.fa.q0));
        queue.push_back((a_start, b.fa.q0));
        while let Some((p, q)) = queue.pop_front() {
            for &d in digits {
                let (Some(sa), Some(sb)) = (symbol_for_digit(a, d), symbol_for_digit(b, d)) else {
                    continue;
                };
                let (Some(pd), Some(qd)) = (a.fa.d[p].get(&sa), b.fa.d[q].get(&sb)) else {
                    continue;
                };
                for &p2 in pd {
                    for &q2 in qd {
                        if seen.insert((p2, q2)) {
                            queue.push_back((p2, q2));
                        }
                    }
                }
            }
        }
        seen
    }

    /// `L(A) / L(B) = { x : ∃y ∈ L(B), xy ∈ L(A) }` — the textbook definition, decided
    /// exactly: `x` is in the quotient iff some state `A` can reach on `x` admits a
    /// common continuation with `B` that both accept.
    fn brute_force_right_quotient(a: &Automaton, b: &Automaton, digits: &[i32], x: &[i32]) -> bool {
        digit_run(a, &BTreeSet::from([a.fa.q0]), x)
            .into_iter()
            .any(|p| {
                reachable_pairs(a, p, b, digits)
                    .into_iter()
                    .any(|(pa, qb)| a.fa.is_accepting(pa) && b.fa.is_accepting(qb))
            })
    }

    /// `L(B) \ L(A) = { z : ∃w ∈ L(B), wz ∈ L(A) }` (`left_quotient`'s own doc comment's
    /// definition), decided exactly the same way: collect every `A`-state reachable by
    /// SOME word of `L(B)`, then ask whether `z` is accepted from any of them.
    fn brute_force_left_quotient(a: &Automaton, b: &Automaton, digits: &[i32], z: &[i32]) -> bool {
        let after_b: BTreeSet<usize> = reachable_pairs(a, a.fa.q0, b, digits)
            .into_iter()
            .filter(|&(_, q)| b.fa.is_accepting(q))
            .map(|(p, _)| p)
            .collect();
        !after_b.is_empty() && accepts_digit_word_from(a, &after_b, z)
    }

    proptest! {
        // Each case runs |A.q| cross-product-plus-emptiness checks inside the quotient
        // (each one a `totalizeCrossProduct` + `determinizeAndMinimize`), so this runs on
        // fewer cases than the default 256 — the same `ProptestConfig` discipline
        // `numsys.rs` and `regex/tests.rs` already apply to their expensive constructions.
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// Tier-4: `rightQuotient` against the brute-force set-theoretic quotient.
        ///
        /// Both operands used to be TRIMMED first, for exactly the reason
        /// `not_matches_the_complement_oracle` trimmed: `right_quotient` closes with
        /// `M.determinizeAndMinimize()`, `M` inherits `A`'s state set verbatim, and while
        /// `docs/WALNUT-BUGS.md` WB-001 (the q0-aliasing bug) was live a randomly
        /// generated automaton with a state unreachable from `q0` could therefore return
        /// a legitimately "wrong" language. WB-001 is fixed (`walnut-java` commit
        /// `14509f1`, ported in [`crate::minimize::minimize`]) and the trims are removed,
        /// so the quotient is now checked on untrimmed operands too. (Trimming was never
        /// unsound here -- it is language-preserving and, for the quotient specifically,
        /// also *quotient*-preserving, since a state that cannot reach acceptance in `A`
        /// has `L(A from i) = ∅` and would have been assigned output `0` anyway -- it was
        /// simply a domain restriction this property no longer needs.)
        #[test]
        fn right_quotient_matches_the_brute_force_quotient(
            a in arb_partial_automaton_over(4, vec![0, 1]),
            b in arb_partial_automaton_over(4, vec![0, 1]),
        ) {
            let m = right_quotient(&a, &b, false, &mut crate::logging::Logging::new());
            for x in all_digit_words(&[0, 1], 4) {
                prop_assert_eq!(
                    accepts_digit_word_from(&m, &BTreeSet::from([m.fa.q0]), &x),
                    brute_force_right_quotient(&a, &b, &[0, 1], &x),
                    "right quotient disagrees on x = {:?}", x
                );
            }
        }

        /// Tier-4: `leftQuotient` against the brute-force set-theoretic quotient, on the
        /// EQUAL-alphabet shape — the shape naive quotient semantics apply to
        /// regardless of guard direction (both directions of `is_subset_alphabet`
        /// coincide when the alphabets are equal as sets), so the port must compute
        /// them here whether the guard is correct or (pre-WB-010-fix) backwards. See
        /// `left_quotient_matches_the_brute_force_quotient_on_a_proper_subset_alphabet`
        /// below for the shape that only the fixed guard admits.
        #[test]
        fn left_quotient_matches_the_brute_force_quotient_on_equal_alphabets(
            a in arb_partial_automaton_over(4, vec![0, 1]),
            b in arb_partial_automaton_over(4, vec![0, 1]),
        ) {
            let mut a = a;
            let mut b = b;
            a.fa = crate::trim::trim(&a.fa);
            b.fa = crate::trim::trim(&b.fa);

            let m = left_quotient(&a, &b, &mut crate::logging::Logging::new());
            for z in all_digit_words(&[0, 1], 4) {
                prop_assert_eq!(
                    accepts_digit_word_from(&m, &BTreeSet::from([m.fa.q0]), &z),
                    brute_force_left_quotient(&a, &b, &[0, 1], &z),
                    "left quotient disagrees on z = {:?}", z
                );
            }
        }

        /// Tier-4, added by **WB-010**'s fix (`docs/WALNUT-BUGS.md`, upstream
        /// `walnut-java` commit `c5ff914` on `bugfix/wb-010`): `leftQuotient` against
        /// the brute-force set-theoretic quotient, on the shape only the FIXED guard
        /// admits — `B`'s alphabet a genuine but non-equal subset of `A`'s (`A` over
        /// `{0,1,2}`, `B` over `{1,2}`). Before the fix this shape was wrongly REJECTED
        /// outright (the old guard required `A ⊆ B`, false here since `A`'s alphabet is
        /// strictly larger); the hand-built
        /// `left_quotient_computes_the_correct_result_when_the_second_alphabet_is_a_subset`
        /// pins one such input, this property covers the whole shape.
        ///
        /// `B`'s alphabet is deliberately `{1,2}`, NOT `{0,1}` — a NON-PREFIX subset.
        /// An earlier revision of this property used `{0,1}`, a prefix subset under
        /// which `right_quotient`'s internal re-encode (`a.encode(&b.decode(sym))`, the
        /// step this whole fix is about) happens to be the IDENTITY function at every
        /// symbol, so it could pass even with that re-encode step deleted entirely.
        /// Found by adversarial review, mutation-proven: replacing the re-encode with
        /// the identity kept this property green under `{0,1}` and only failed once the
        /// alphabet was changed to a genuinely index-shifting subset like `{1,2}` (`B`'s
        /// local index 0 is digit VALUE 1, which sits at `A`'s local index 1 — an
        /// accident of `{0,1}` starting at 0; `{1,2}`'s local index 0 is digit VALUE 1,
        /// which sits at `A`'s local index 1 too, so the genuinely discriminating part
        /// is local index 1 -> value 2 -> `A`'s local index 2, a real `1 -> 2` shift).
        #[test]
        fn left_quotient_matches_the_brute_force_quotient_on_a_proper_subset_alphabet(
            a in arb_partial_automaton_over(3, vec![0, 1, 2]),
            b in arb_partial_automaton_over(3, vec![1, 2]),
        ) {
            let mut a = a;
            let mut b = b;
            a.fa = crate::trim::trim(&a.fa);
            b.fa = crate::trim::trim(&b.fa);
            prop_assert!(is_subset_alphabet(&b.alphabet, &a.alphabet));
            prop_assert!(
                !is_subset_alphabet(&a.alphabet, &b.alphabet),
                "keep this shape genuinely non-equal-alphabet, or it collapses into \
                 the sibling equal-alphabet property above"
            );

            let m = left_quotient(&a, &b, &mut crate::logging::Logging::new());
            for z in all_digit_words(&[0, 1, 2], 3) {
                prop_assert_eq!(
                    accepts_digit_word_from(&m, &BTreeSet::from([m.fa.q0]), &z),
                    brute_force_left_quotient(&a, &b, &[0, 1, 2], &z),
                    "left quotient disagrees on z = {:?}", z
                );
            }
        }

        /// Tier-4 **on WB-010's own guarded shape, now fixed**: `A`'s alphabet is always
        /// SMALLER than `B`'s, so `B ⊄ A` is a syntactic guarantee of the generator, not
        /// a randomized property of the generated automaton contents — this test's own
        /// job is only to confirm the fixed guard rejects EVERY such shape, deterministically,
        /// before any of the quotient machinery runs, with the exact WB-010 message.
        /// (Found by adversarial review: an earlier revision hardcoded ONE alphabet pair
        /// for both `a` and `b` across all generated cases — i.e. every case exercised
        /// literally the same two alphabets and only the automata's states/transitions
        /// varied, which the guard doesn't even look at. `alphabet_pair` below samples
        /// among several different non-subset alphabet shapes instead, so this property
        /// is checking the guard against real shape variety, not one fixed pair.)
        /// Before the fix (`docs/WALNUT-BUGS.md`, `walnut-java` commit `c5ff914`),
        /// `leftQuotient`'s backwards `isSubsetA(A, B)` guard PASSED whenever `A ⊆ B`
        /// held (always true here, since `A`'s alphabet is a subset of `B`'s) while the
        /// containment the internal `rightQuotient` re-encode actually needed (`B ⊆ A`)
        /// did not — sometimes succeeding, sometimes panicking deep inside the re-encode,
        /// depending on whether `B`'s out-of-`A`-alphabet digits survived
        /// `reverse_and_canonize` (see this file's git history, pre-fix revision, for
        /// that property in its original "either correct or the documented failure"
        /// form).
        #[test]
        fn left_quotient_rejects_the_wb_010_shape_before_any_computation(
            (a, b) in prop_oneof![
                (arb_partial_automaton_over(3, vec![0, 1]), arb_partial_automaton_over(3, vec![0, 1, 2])),
                (arb_partial_automaton_over(3, vec![1, 2]), arb_partial_automaton_over(3, vec![0, 1, 2, 3])),
                (arb_partial_automaton_over(3, vec![0]), arb_partial_automaton_over(3, vec![0, 1])),
                (arb_partial_automaton_over(3, vec![2, 3]), arb_partial_automaton_over(3, vec![0, 1, 2, 3, 4])),
            ],
        ) {
            let mut a = a;
            let mut b = b;
            a.fa = crate::trim::trim(&a.fa);
            b.fa = crate::trim::trim(&b.fa);
            prop_assert!(!is_subset_alphabet(&b.alphabet, &a.alphabet));

            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                left_quotient(&a, &b, &mut crate::logging::Logging::new())
            }));
            let payload = match outcome {
                Ok(_) => {
                    return Err(proptest::test_runner::TestCaseError::fail(
                        "left_quotient must be rejected by its own guard on this shape, \
                         but it returned a result -- the fixed guard is no longer \
                         enforcing B's-alphabet-subset-of-A's",
                    ));
                }
                Err(payload) => payload,
            };
            // `panic!`'s payload is a `String` for a formatted message and a
            // `&'static str` for a literal one -- both tried before giving up.
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&'static str>().copied())
                .unwrap_or("<non-string panic payload>")
                .to_string();
            prop_assert_eq!(
                &msg, WB_010_GUARD_PANIC,
                "left_quotient panicked, but NOT with the fixed guard's own subset \
                 message -- this is an unexpected regression (e.g. reaching the \
                 internal re-encode unguarded again), not the expected clean rejection"
            );
        }
    }

    // ------------------------------------------------------------------------
    // The TRUE/FALSE short-circuits (U0) — `AutomatonLogicalOps.java:45-149`.
    //
    // These are pinned as full truth tables rather than sampled, because the
    // short-circuits are where an asymmetry mistake (`imply`), a missed swap-recursion
    // (`and`/`or`/`xor`), or an inverted truth value hides without any language-level
    // property test noticing: none of the Tier-4 properties in this crate generate a
    // trivial operand at all.
    //
    // `P` throughout is `exactly_one()` — deliberately the NON-total fixture, so that
    // "the trivial branch returned `B.asDFA()` verbatim, without totalizing" is
    // observable as a state count as well as a language.
    //
    // # Cross-checked against the real walnut-java CLI
    //
    // Every entry of these tables was additionally confirmed end-to-end against
    // `target/Walnut-all.jar` (v8.0-alpha) during U0, using `Ei Ex i<x` as the TRUE
    // automaton, `Ei Ex (i<x & x<i)` as FALSE, and `x=0` as `P`:
    //
    // | query                                    | Walnut's output            |
    // |------------------------------------------|----------------------------|
    // | `Ei Ex i<x`                              | `TRUE` (file: `true`)      |
    // | `Ei Ex (i<x & x<i)`                      | `FALSE` (file: `false`)    |
    // | `(Ei Ex i<x) & (x=x)`                    | the accept-all DFA (`= P`) |
    // | `(Ei Ex i<x) ^ (x=0)`                    | "contains a 1" (`= ¬P`)    |
    // | `(Ei Ex (i<x & x<i)) ^ (x=0)`            | `x=0` verbatim (`= P`)     |
    // | `(x=0) => (Ei Ex i<x)`                   | `TRUE`                     |
    // | `(x=0) => (Ei Ex (i<x & x<i))`           | `¬P`                       |
    // | `(Ei Ex (i<x & x<i)) => (x=0)`           | `TRUE`                     |
    // | `(Ei Ex i<x) <=> (x=0)`                  | `x=0` verbatim (`= P`)     |
    // | `(Ei Ex (i<x & x<i)) <=> (x=0)`          | `¬P`                       |
    //
    // Note the two rows whose output is `x=0` *verbatim* — i.e. still non-total, with
    // no sink state — which is the live evidence that the trivial branches really do
    // bypass `totalizeCrossProduct`, not merely that they happen to agree on language.
    // ------------------------------------------------------------------------

    /// What a short-circuit is expected to produce.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Expect {
        Trivial(bool),
        /// The ordinary operand `P`, unchanged.
        P,
        /// The complement of `P`.
        NotP,
    }

    fn p_automaton() -> Automaton {
        single_track(exactly_one(), Some(true))
    }

    fn check(result: &AutomatonDFA, expected: Expect, case: &str) {
        let m = result.automaton();
        match expected {
            Expect::Trivial(truth) => {
                assert!(
                    m.is_true_false_automaton(),
                    "{case}: expected a TRUE/FALSE automaton, got {m:?}"
                );
                assert_eq!(m.is_true_automaton(), truth, "{case}: wrong truth value");
            }
            Expect::P | Expect::NotP => {
                assert!(
                    !m.is_true_false_automaton(),
                    "{case}: expected an ordinary automaton"
                );
                for word in WORDS {
                    let p = exactly_one().accepts_word(word);
                    let want = if expected == Expect::P { p } else { !p };
                    assert_eq!(
                        m.fa.accepts_word(word),
                        want,
                        "{case}: wrong language on word={word:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn and_short_circuits_on_trivial_operands() {
        for t in [true, false] {
            // TRUE and X == X; FALSE and X == FALSE (`:45-50`).
            check(
                &and(
                    &Automaton::true_false(t),
                    &p_automaton(),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::P } else { Expect::Trivial(false) },
                &format!("and({t}, P)"),
            );
            // The swap-recursion arm (`:49`, "and is symmetric").
            check(
                &and(
                    &p_automaton(),
                    &Automaton::true_false(t),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::P } else { Expect::Trivial(false) },
                &format!("and(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &and(
                        &Automaton::true_false(t),
                        &Automaton::true_false(u),
                        &mut crate::logging::Logging::new(),
                    ),
                    Expect::Trivial(t && u),
                    &format!("and({t}, {u})"),
                );
            }
        }
    }

    #[test]
    fn or_short_circuits_on_trivial_operands() {
        for t in [true, false] {
            check(
                &or(
                    &mut Automaton::true_false(t),
                    &mut p_automaton(),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::Trivial(true) } else { Expect::P },
                &format!("or({t}, P)"),
            );
            check(
                &or(
                    &mut p_automaton(),
                    &mut Automaton::true_false(t),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::Trivial(true) } else { Expect::P },
                &format!("or(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &or(
                        &mut Automaton::true_false(t),
                        &mut Automaton::true_false(u),
                        &mut crate::logging::Logging::new(),
                    ),
                    Expect::Trivial(t || u),
                    &format!("or({t}, {u})"),
                );
            }
        }
    }

    #[test]
    fn xor_short_circuits_on_trivial_operands() {
        for t in [true, false] {
            check(
                &xor(
                    &mut Automaton::true_false(t),
                    &mut p_automaton(),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::NotP } else { Expect::P },
                &format!("xor({t}, P)"),
            );
            check(
                &xor(
                    &mut p_automaton(),
                    &mut Automaton::true_false(t),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::NotP } else { Expect::P },
                &format!("xor(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &xor(
                        &mut Automaton::true_false(t),
                        &mut Automaton::true_false(u),
                        &mut crate::logging::Logging::new(),
                    ),
                    Expect::Trivial(t != u),
                    &format!("xor({t}, {u})"),
                );
            }
        }
    }

    #[test]
    fn imply_short_circuits_on_trivial_operands_asymmetrically() {
        for t in [true, false] {
            // TRUE -> X == X; FALSE -> X == TRUE (`:100-102`).
            check(
                &imply(
                    &mut Automaton::true_false(t),
                    &mut p_automaton(),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::P } else { Expect::Trivial(true) },
                &format!("imply({t}, P)"),
            );
            // X -> TRUE == TRUE; X -> FALSE == not X (`:103-107`). This is the arm that
            // cannot be reached by swapping, and the one whose `B.isTRUE_AUTOMATON()`
            // read has no enclosing `isTRUE_FALSE_AUTOMATON()` check in Java.
            check(
                &imply(
                    &mut p_automaton(),
                    &mut Automaton::true_false(t),
                    &mut crate::logging::Logging::new(),
                ),
                if t {
                    Expect::Trivial(true)
                } else {
                    Expect::NotP
                },
                &format!("imply(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &imply(
                        &mut Automaton::true_false(t),
                        &mut Automaton::true_false(u),
                        &mut crate::logging::Logging::new(),
                    ),
                    Expect::Trivial(!t || u),
                    &format!("imply({t}, {u})"),
                );
            }
        }
    }

    #[test]
    fn iff_short_circuits_via_its_double_imply_recursion() {
        for t in [true, false] {
            check(
                &iff(
                    &mut Automaton::true_false(t),
                    &mut p_automaton(),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::P } else { Expect::NotP },
                &format!("iff({t}, P)"),
            );
            check(
                &iff(
                    &mut p_automaton(),
                    &mut Automaton::true_false(t),
                    &mut crate::logging::Logging::new(),
                ),
                if t { Expect::P } else { Expect::NotP },
                &format!("iff(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &iff(
                        &mut Automaton::true_false(t),
                        &mut Automaton::true_false(u),
                        &mut crate::logging::Logging::new(),
                    ),
                    Expect::Trivial(t == u),
                    &format!("iff({t}, {u})"),
                );
            }
        }
    }

    #[test]
    fn the_trivial_branches_never_totalize_their_operands() {
        // The ordinary `or`/`xor`/`imply`/`iff` path mutates both operands in place
        // (`totalizeCrossProduct`, `:117-118`); the trivial branches return before it.
        // `exactly_one()` grows from 2 states to 3 when totalized, so a state count is
        // a sufficient witness.
        for t in [true, false] {
            for op in [
                or as fn(
                    &mut Automaton,
                    &mut Automaton,
                    &mut crate::logging::Logging,
                ) -> AutomatonDFA,
                xor,
                imply,
                iff,
            ] {
                let mut p = p_automaton();
                let mut triv = Automaton::true_false(t);
                let _ = op(&mut triv, &mut p, &mut crate::logging::Logging::new());
                assert_eq!(p.fa.q, 2, "operand was totalized in a trivial branch");

                let mut p = p_automaton();
                let mut triv = Automaton::true_false(t);
                let _ = op(&mut p, &mut triv, &mut crate::logging::Logging::new());
                assert_eq!(p.fa.q, 2, "operand was totalized in a trivial branch");
            }
        }
    }

    #[test]
    fn not_flips_a_trivial_automatons_truth_value_and_stays_trivial() {
        // `:146-149`.
        for t in [true, false] {
            let n = not(
                AutomatonDFA::true_false(t),
                &mut crate::logging::Logging::new(),
            );
            assert!(n.automaton().is_true_false_automaton());
            assert_eq!(n.automaton().is_true_automaton(), !t);
        }
        // Involution, for good measure.
        let n = not(
            not(
                AutomatonDFA::true_false(true),
                &mut crate::logging::Logging::new(),
            ),
            &mut crate::logging::Logging::new(),
        );
        assert!(n.automaton().is_true_automaton());
    }

    #[test]
    fn reverse_is_a_noop_on_a_trivial_automaton() {
        // `:415`. Uses the stale-`q` shape, where the un-guarded body would panic
        // inside `Fa::reverse` / `determinize_and_minimize_from`.
        for t in [true, false] {
            let mut a = p_automaton();
            a.fa.true_false = Some(t);
            a.clear();
            reverse(&mut a, true);
            assert!(a.is_true_false_automaton());
            assert_eq!(a.is_true_automaton(), t);
            assert!(a.msd.is_empty(), "nothing to flip");
        }
    }

    #[test]
    fn fix_leading_zeros_problem_is_a_noop_on_a_trivial_automaton() {
        // `:269`. Again the stale-`q` shape: `determine_zero()` returns 0 for the empty
        // alphabet and `zero_reachable_states` would then index an empty `fa.d`.
        for t in [true, false] {
            let mut a = p_automaton();
            a.fa.true_false = Some(t);
            a.clear();
            fix_leading_zeros_problem(&mut a);
            assert!(a.is_true_false_automaton());
            assert_eq!(a.is_true_automaton(), t);
        }
        // ...and on the freshly-constructed shape too.
        let mut a = Automaton::true_false(true);
        fix_leading_zeros_problem(&mut a);
        assert!(a.is_true_automaton());
    }

    // ------------------------------------------------------- convertNS (U18)

    /// `AutomatonLogicalOpsTest.simulate(FA, int...)` (`:58-68`) — run from `q0`, taking
    /// the first destination each step, returning `0` the moment the table runs out.
    fn simulate(fa: &Fa, symbols: &[i32]) -> i32 {
        let mut state = fa.q0;
        for &sym in symbols {
            match fa.d[state].get(&sym) {
                Some(dests) if !dests.is_empty() => state = dests[0],
                _ => return 0,
            }
        }
        fa.o[state]
    }

    /// A one-state, no-transition, accepting automaton over `[0..base-1]` — Java's
    /// `A.fa.initBasicFA(IntList.of(1))` plus an explicit alphabet. Accepts exactly the
    /// empty word, and is deliberately NOT total.
    fn epsilon_only(base: i32, msd: bool) -> Automaton {
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: base as usize,
            o: vec![1],
            d: vec![BTreeMap::new()],
        };
        Automaton::new(
            fa,
            vec![util::int_range_list(base)],
            vec!["x".to_string()],
            vec![Some(msd)],
        )
    }

    /// U27 fix, found by the Tier-1 golden corpus (fixture 554,
    /// `convert test554 msd_2 FTM;` where `FTM` is a word automaton over `msd_fib`).
    ///
    /// Java's `ns.parseBase()` reads the base out of the number system's NAME and rejects
    /// `fib` outright. This port derived it from the track's ALPHABET SIZE — 2 for
    /// `msd_fib` — so the call silently took the `from_base == to_base` branch and reported
    /// "New and old number systems are identical: msd_2": a different error for a different
    /// reason, and (worse) a *successful* conversion for `convert x lsd_2 FTM;`, which
    /// would have reversed a Zeckendorf automaton as if it were binary.
    #[test]
    fn convert_ns_parses_the_base_from_the_name_not_the_alphabet_size() {
        let mut a = epsilon_only(2, true);
        a.set_ns_names(vec![Some("msd_fib".to_string())]);
        let err = convert_ns(&mut a, true, 2, &mut crate::logging::Logging::new())
            .expect_err("`fib` is not a base");
        assert_eq!(
            err.to_string(),
            "Base of automaton's number system must be > 1 and int, found: fib"
        );
        // The msd<->lsd flip must be rejected for the same reason, rather than silently
        // reversing the automaton as if it were plain base 2.
        let err = convert_ns(&mut a, false, 2, &mut crate::logging::Logging::new())
            .expect_err("`fib` is not a base");
        assert_eq!(
            err.to_string(),
            "Base of automaton's number system must be > 1 and int, found: fib"
        );

        // A track with no recorded name still reconstructs exactly (`msd_<alphabet size>`),
        // so plain bases are unaffected by the change.
        let mut plain = epsilon_only(2, true);
        assert!(matches!(
            convert_ns(&mut plain, true, 2, &mut crate::logging::Logging::new()),
            Err(ConvertNsError::IdenticalNumberSystems { ref name }) if name == "msd_2"
        ));
    }

    /// `parseBase`'s own boundary cases, straight off `NumberSystem.java:237-243`.
    #[test]
    fn parse_base_matches_javas_pattern_number_and_greater_than_one_guard() {
        assert_eq!(parse_base("msd_2"), Ok(2));
        assert_eq!(parse_base("lsd_10"), Ok(10));
        // `<= 1` is rejected even though it IS a number.
        assert!(matches!(
            parse_base("msd_1"),
            Err(ConvertNsError::BaseNotAPositiveInt { ref found }) if found == "1"
        ));
        assert!(matches!(
            parse_base("msd_0"),
            Err(ConvertNsError::BaseNotAPositiveInt { ref found }) if found == "0"
        ));
        // `^\d+$` — a sign is NOT a number to Java's `PATTERN_NUMBER`.
        assert!(matches!(
            parse_base("msd_-2"),
            Err(ConvertNsError::BaseNotAPositiveInt { ref found }) if found == "-2"
        ));
        assert!(matches!(
            parse_base("msd_fib"),
            Err(ConvertNsError::BaseNotAPositiveInt { ref found }) if found == "fib"
        ));
        // `determineBase` splits on the FIRST underscore, so a `neg_` name reports the
        // whole remainder — matching Java exactly.
        assert!(matches!(
            parse_base("msd_neg_2"),
            Err(ConvertNsError::BaseNotAPositiveInt { ref found }) if found == "neg_2"
        ));
    }

    /// The `||` short-circuit: an all-digit base that overflows a 32-bit `int` passes
    /// `isNumber`, so Java runs `Integer.parseInt` and throws an uncaught
    /// `NumberFormatException` — NOT the `WalnutException` the guard would have thrown. The
    /// two must stay distinguishable; see [`ConvertNsError::BaseOverflowsInt`] for why this
    /// is unreachable from any real input path and is guarded anyway.
    #[test]
    fn an_all_digit_base_that_overflows_an_int_is_not_the_guards_walnut_exception() {
        let err = parse_base("msd_99999999999999").expect_err("2^31 is not an int base");
        assert!(
            matches!(err, ConvertNsError::BaseOverflowsInt { ref found } if found == "99999999999999"),
            "expected the NumberFormatException path, got {err:?}"
        );
        assert_eq!(err.to_string(), "For input string: \"99999999999999\"");

        // The boundary itself: `2^31 - 1` still parses, `2^31` does not.
        assert_eq!(parse_base("msd_2147483647"), Ok(2_147_483_647));
        assert!(matches!(
            parse_base("msd_2147483648"),
            Err(ConvertNsError::BaseOverflowsInt { ref found }) if found == "2147483648"
        ));

        // A digit run with leading zeros is still `^\d+$` and still parses, as in Java.
        assert_eq!(parse_base("msd_007"), Ok(7));
    }

    /// Tier 2: `AutomatonLogicalOpsTest.testConvertNSSameBaseFlipsMsdLsd` (`:236-255`).
    /// Same base, only the direction differs — so the `identical` error must NOT fire, the
    /// non-total table must be totalized (`:470-471`), and `reverseWithOutput` must run.
    #[test]
    fn convert_ns_same_base_flips_msd_lsd() {
        let mut a = epsilon_only(2, true);
        assert_eq!(simulate(&a.fa, &[]), 1, "language before: {{epsilon}} only");

        convert_ns(&mut a, false, 2, &mut crate::logging::Logging::new())
            .expect("the flip must succeed");

        assert_eq!(a.msd, vec![Some(false)], "number system must now be lsd_2");
        assert_eq!(
            simulate(&a.fa, &[]),
            1,
            "empty string still accepted after msd<->lsd flip"
        );
        assert_eq!(simulate(&a.fa, &[0]), 0, "\"0\" still rejected");
        assert_eq!(simulate(&a.fa, &[1]), 0, "\"1\" still rejected");
    }

    /// Tier 2: `AutomatonLogicalOpsTest.testConvertNSBaseConversionFromLsd` (`:261-286`).
    /// `lsd_4 -> msd_2`: `commonRoot(4, 2) == 2 == toBase`, so only the `k^i -> k`
    /// ungrouping runs, behind the `!ns.isMsd()` pre-reversal at `:495-496`.
    #[test]
    fn convert_ns_base_conversion_from_lsd() {
        let mut a = epsilon_only(4, false);

        convert_ns(&mut a, true, 2, &mut crate::logging::Logging::new())
            .expect("the conversion must succeed");

        assert_eq!(a.msd, vec![Some(true)], "number system must now be msd_2");
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.fa.alphabet_size, 2);
        assert_eq!(
            simulate(&a.fa, &[]),
            1,
            "empty string still accepted after base conversion"
        );
        assert_eq!(simulate(&a.fa, &[0]), 0);
        assert_eq!(simulate(&a.fa, &[1]), 0);
        assert_eq!(simulate(&a.fa, &[0, 1]), 0);
        assert_eq!(simulate(&a.fa, &[1, 0]), 0);
    }

    /// Tier 2: `AutomatonLogicalOpsTest.testConvertMsdBaseToExponentThrowsWhenNotDeterministicAndTotal`
    /// (`:297-311`). Java reaches the private guard by reflection; this test module can
    /// call it directly.
    #[test]
    fn convert_msd_base_to_exponent_rejects_a_non_total_automaton() {
        let mut a = epsilon_only(2, true); // one state, no transitions -> not total
        assert_eq!(
            convert_msd_base_to_exponent(&mut a, 2, 2),
            Err(ConvertNsError::NotDeterministicAndTotal)
        );
        assert_eq!(
            ConvertNsError::NotDeterministicAndTotal.to_string(),
            "Automaton must be deterministic for msd_k^j conversion"
        );
    }

    /// Tier 2: `AutomatonLogicalOpsTest.testConvertLsdBaseToRootThrowsOnBaseMismatch`
    /// (`:313-...`). `root = 2, exponent = 2` expects base 4; the automaton says 3.
    #[test]
    fn convert_lsd_base_to_root_rejects_a_base_mismatch() {
        let mut a = epsilon_only(3, true);
        assert_eq!(
            convert_lsd_base_to_root(&mut a, 3, 2, 2),
            Err(ConvertNsError::BaseMismatch {
                expected: 4,
                found: 3
            })
        );
        assert_eq!(
            ConvertNsError::BaseMismatch {
                expected: 4,
                found: 3
            }
            .to_string(),
            "Base mismatch: expected 4, found 3"
        );
    }

    /// `docs/WALNUT-BUGS.md` WB-033: a track declared `{0,1}` rather than `msd_k`/`lsd_k`
    /// has a `null` `NumberSystem` in Java. Originally this pinned Java's raw NPE message
    /// (reproduced verbatim so the CLI text matched); as of `walnut-java` commit `c75e630`
    /// Java throws a real `WalnutException` instead, so this now pins the fixed message —
    /// verified live against the fixed jar, see
    /// `tests/differential/tests/java_bugfix_wb033.rs`. Surfaced here as an `Err`, per
    /// WB-013's established convention — no behavioral change was needed on this port's
    /// side, only the message text.
    #[test]
    fn convert_ns_rejects_a_track_with_no_number_system_wb033() {
        let mut a = epsilon_only(2, true);
        a.msd = vec![None];
        let err = convert_ns(&mut a, true, 4, &mut crate::logging::Logging::new())
            .expect_err("must reject a track with no attached number system");
        assert_eq!(err, ConvertNsError::NoNumberSystem);
        assert_eq!(
            err.to_string(),
            "the automaton being converted has no attached number system (its alphabet was \
             declared explicitly, e.g. {0,1}, rather than as msd_k/lsd_k)"
        );
    }

    /// A TRUE/FALSE automaton has no tracks at all, so it takes the arity guard — the same
    /// branch Java takes, since its trivial automata carry an empty `NS` list.
    #[test]
    fn convert_ns_rejects_a_trivial_automaton_on_the_arity_guard() {
        for t in [true, false] {
            let mut a = Automaton::true_false(t);
            assert_eq!(
                convert_ns(&mut a, true, 4, &mut crate::logging::Logging::new()),
                Err(ConvertNsError::NotSingleInput)
            );
        }
    }

    /// `UtilityMethodsTest.testExactIntegerExponentOnOrdinaryPairs` (`walnut-java` commit
    /// `18b7c4b`, `bugfix/wb-032`) — cases where the OLD float computation already happened
    /// to be correct, so the fix must not disturb them.
    #[test]
    fn exact_integer_exponent_on_ordinary_pairs() {
        assert_eq!(exact_integer_exponent(2, 2), Ok(1));
        assert_eq!(exact_integer_exponent(8, 2), Ok(3));
        assert_eq!(exact_integer_exponent(1024, 2), Ok(10));
        assert_eq!(exact_integer_exponent(16, 4), Ok(2));
        assert_eq!(exact_integer_exponent(81, 9), Ok(2));
    }

    /// `UtilityMethodsTest.testExactIntegerExponentOnWb032AffectedPairs` — every
    /// `(root, exponent)` pair with `root <= 100` on which real (pre-fix) Java's
    /// `(int)(Math.log(base)/Math.log(root))` used to return `exponent - 1`
    /// (`docs/WALNUT-BUGS.md` WB-032's own "root <= 100" list, itself independently swept
    /// from a real JVM). The fixed [`exact_integer_exponent`] must recover the CORRECT
    /// exponent on every one of these.
    ///
    /// # Why this table is a real regression guard, not incidental data
    ///
    /// Every pair here is already covered by the full round-trip sweep three tests below
    /// (`root <= 46340`), so on its own this table would add zero net coverage beyond it —
    /// a prior version of this comment claimed it was "the direct regression guard against
    /// ever silently reintroducing WB-032's float-log computation," which was an overclaim
    /// for exactly that reason. What makes it a genuine, *self-validating* guard is the
    /// first assertion in the loop below: before checking the FIXED function, it confirms
    /// the pair actually reproduces the OLD bug — `(f64::from(base).ln() /
    /// f64::from(root).ln()) as i32 == exponent - 1` — using Rust's own `f64::ln`, not a
    /// resurrected bit-for-bit `java_log`/FDLIBM port (both deleted along with
    /// `truncated_log_ratio` now that nothing calls them; see [`exact_integer_exponent`]'s
    /// doc comment). That substitution is checked here, not assumed to be safe everywhere:
    /// `f64::ln` does **not** agree with Java's `Math.log` in general (it disagrees on 29
    /// of the pairs once `root` is widened to `<= 1000` — missing `(3,5)`, `(3,10)`, …,
    /// and inventing `(185,2)`, `(196,2)`, … — see this file's git history at the WB-032
    /// fix commit for that capture), but it was independently re-verified (by direct
    /// computation, before writing this comment) to agree with Java's buggy direction on
    /// all 27 pairs in THIS specific `root <= 100` table. If this table is ever widened
    /// past `root <= 100`, that agreement must be re-checked before trusting the
    /// self-validation, not assumed to keep holding.
    #[rustfmt::skip]
    const FORMERLY_WB032_AFFECTED_ROOT_LE_100: &[(i32, i32)] = &[
        (9, 5), (10, 3), (10, 6), (10, 9), (11, 7), (12, 7), (17, 3), (17, 6), (22, 5),
        (31, 3), (31, 6), (34, 3), (34, 6), (41, 3), (46, 5), (52, 3), (52, 5), (54, 5),
        (55, 5), (56, 3), (56, 5), (69, 5), (83, 3), (88, 3), (93, 3), (98, 3), (100, 3),
    ];

    #[test]
    fn exact_integer_exponent_on_formerly_wb032_affected_pairs() {
        for &(root, exponent) in FORMERLY_WB032_AFFECTED_ROOT_LE_100 {
            let base = i64::from(root).pow(exponent as u32);
            // Self-validation: confirm this pair genuinely triggers WB-032's bug in the
            // OLD (now-deleted) computation, so the table is asserting the FIX actually
            // changed the answer on exactly the cases it needed to, not just restating
            // data the sweep below already covers.
            let old_buggy_computation = (f64::from(base as i32).ln() / f64::from(root).ln()) as i32;
            assert_eq!(
                old_buggy_computation,
                exponent - 1,
                "root={root} exponent={exponent} base={base}: this pair is expected to \
                 reproduce WB-032's float-log bug under Rust's own f64::ln (independently \
                 verified to agree with Java's Math.log on the buggy direction for every \
                 root <= 100 pair, see this table's own doc comment) -- if this assertion \
                 ever fails, the pair no longer demonstrates the regression and does not \
                 belong in this table"
            );
            assert_eq!(
                exact_integer_exponent(base as i32, root),
                Ok(exponent),
                "root={root} base={base}: must recover the exact exponent, not \
                 WB-032's off-by-one-low float result"
            );
        }
    }

    /// `UtilityMethodsTest.testExactIntegerExponentSweepAgreesWithBruteForceOverRootUpTo1000`,
    /// widened to the full `int`-alphabet range: sweep every `(root, exponent)` with
    /// `root <= 46340` (the largest `root` with `root^2 <= 2^31`, the last one an `int`
    /// alphabet can hold) and `root^exponent <= 2^31`, constructing `base` from `exponent`
    /// by plain repeated multiplication and confirming [`exact_integer_exponent`] recovers
    /// `exponent` back out of `(base, root)` alone. Construction and recovery are the
    /// opposite directions of the same arithmetic, so this is a genuine round-trip check,
    /// not a tautology — the same shape Java's own new sweep test uses.
    ///
    /// This sweep is what the deleted `truncated_log_ratio_agrees_with_real_java` (a real-JVM
    /// capture of the OLD buggy behaviour) used to cover, now at the same size (48,036 pairs,
    /// asserted below) but checking the fixed, CORRECT answer on every pair, not just the
    /// 343 that used to be wrong.
    #[test]
    fn exact_integer_exponent_recovers_the_exponent_over_the_full_int_alphabet_sweep() {
        let mut swept = 0usize;
        for root in 2i64..=46340 {
            let mut x = root;
            for exponent in 2i32.. {
                x *= root;
                if x > i64::from(i32::MAX) {
                    break;
                }
                assert_eq!(
                    exact_integer_exponent(x as i32, root as i32),
                    Ok(exponent),
                    "root {root}^{exponent} = {x}: exact_integer_exponent must recover \
                     the exponent exactly"
                );
                swept += 1;
            }
        }
        // Guards against the sweep silently collapsing (a bad bound would make the
        // assertions above vacuous).
        assert_eq!(swept, 48036, "the swept range changed");
    }

    /// `UtilityMethodsTest.testExactIntegerExponentThrowsWhenBaseIsNotAnExactPowerOfRoot`.
    /// Unreachable through [`convert_ns`]'s own call sites (see [`exact_integer_exponent`]'s
    /// doc comment), but the guard itself — and its exact message — is still pinned
    /// directly, matching Java's own defensive test.
    #[test]
    fn exact_integer_exponent_errors_when_base_is_not_an_exact_power() {
        let err = exact_integer_exponent(10, 3).unwrap_err();
        assert_eq!(err, ConvertNsError::NotAnExactPower { base: 10, root: 3 });
        assert_eq!(err.to_string(), "10 is not an exact power of 3");
    }

    /// `UtilityMethodsTest.testExactIntegerExponentThrowsWhenRootIsNotGreaterThanOne`.
    #[test]
    fn exact_integer_exponent_errors_when_root_is_not_greater_than_one() {
        for root in [1, 0, -2] {
            assert_eq!(
                exact_integer_exponent(8, root),
                Err(ConvertNsError::InvalidRoot { root })
            );
        }
    }

    /// Forces [`exact_integer_exponent`]'s `power: i64` accumulator to actually matter.
    ///
    /// Every OTHER test in this module stays at or below `base == i32::MAX`, and in every
    /// one of those cases `power` never needs to leave `i32` range before the loop's
    /// `power < i64::from(base)` check notices it has gone far enough and stops — so
    /// `power: i64 -> power: i32` is a mutation the rest of this suite cannot see. This
    /// case is the one the function's own doc comment ("`# The algorithm`") already
    /// reasons about: `base = i32::MAX = 2_147_483_647`, `root = 2`. The doubling
    /// sequence is `1, 2, 4, …, 2^30 = 1_073_741_824` (still an `i32`), then the next
    /// multiply produces `2^31 = 2_147_483_648` — one past `i32::MAX`, so `power` MUST be
    /// wider than `i32` to hold the value the loop needs to compare against `base` on
    /// that very iteration. With `power: i32` that multiply overflows: a debug build
    /// panics (`attempt to multiply with overflow`); a release build wraps to
    /// `i32::MIN`, and because a wrapped negative `i32` widened to `i64` still satisfies
    /// `power < i64::from(base)`, the loop never terminates at all — exactly the kind of
    /// hang `CLAUDE.md`'s test-performance guardrails ("every end-to-end test has a
    /// wall-time … cap") exist to keep out of the actual decision procedure
    /// (`convert_ns` calls this function directly, unguarded by any such cap of its
    /// own). Mutation-verified: temporarily changing `power`'s declared type to `i32`
    /// makes this test fail with a debug-build overflow panic; restored afterward.
    #[test]
    fn exact_integer_exponent_forces_the_i64_accumulator_past_i32_max() {
        assert_eq!(
            exact_integer_exponent(i32::MAX, 2),
            Err(ConvertNsError::NotAnExactPower {
                base: i32::MAX,
                root: 2,
            })
        );
    }

    /// `AutomatonLogicalOpsTest.testConvertNSWb032ToBaseDirectionRegroupsToExactExponentNotOneLess`
    /// (`walnut-java` commit `18b7c4b`, `bugfix/wb-032`) — WB-032's `k -> k^j` regrouping
    /// direction (`toBase != commonRoot`, `convertMsdBaseToExponent`). Before the fix, the
    /// truncated exponent (2 instead of 3) silently produced a well-formed but WRONG
    /// `msd_100` automaton with no error. Replaces `truncated_log_ratio_reproduces_wb032`,
    /// whose entire purpose was pinning that now-deleted buggy computation; this is the
    /// positive coverage that the same trigger case now computes correctly end-to-end
    /// through `convert_ns` itself, not just through `exact_integer_exponent` in isolation.
    #[test]
    fn convert_ns_wb032_to_base_direction_regroups_to_exact_exponent_not_one_less() {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 10,
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![util::int_range_list(10)],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(
            a.fa.accepts_word(&[]),
            "language before conversion: {{epsilon}} only"
        );

        convert_ns(&mut a, true, 1000, &mut crate::logging::Logging::new())
            .expect("msd_10 -> msd_1000 must succeed");

        assert_eq!(
            a.track_ns_names(),
            vec![Some("msd_1000".to_string())],
            "WB-032: must regroup 3 base-10 digits per base-1000 digit, not 2"
        );
        assert_eq!(a.alphabet, vec![util::int_range_list(1000)]);
        assert!(
            a.fa.accepts_word(&[]),
            "empty string still accepted after regrouping"
        );
        assert!(
            !a.fa.accepts_word(&[0]),
            "a single base-1000 digit (= 3 base-10 zero digits) must still be rejected"
        );
    }

    /// `AutomatonLogicalOpsTest.testConvertNSWb032FromBaseDirectionUngroupsToExactExponentNotOneLess`
    /// — WB-032's `k^i -> k` ungrouping direction (`fromBase != commonRoot`,
    /// `convertLsdBaseToRoot`). Before the fix, the truncated exponent made
    /// `convertLsdBaseToRoot`'s own base-mismatch guard fire ("Base mismatch: expected 100,
    /// found 1000"), crashing a call that should succeed.
    #[test]
    fn convert_ns_wb032_from_base_direction_ungroups_to_exact_exponent_not_one_less() {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 1000,
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![util::int_range_list(1000)],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(a.fa.accepts_word(&[]));

        convert_ns(&mut a, true, 10, &mut crate::logging::Logging::new())
            .expect("msd_1000 -> msd_10 must succeed, not hit a spurious base-mismatch error");

        assert_eq!(a.track_ns_names(), vec![Some("msd_10".to_string())]);
        assert_eq!(a.alphabet, vec![util::int_range_list(10)]);
        assert!(a.fa.accepts_word(&[]));
    }

    /// `computeStringValue` (`:671-677`) reads its digit list **least**-significant-first,
    /// which is what makes `convertLsdBaseToRoot` the lsd direction.
    #[test]
    fn compute_string_value_reads_least_significant_first() {
        assert_eq!(compute_string_value(&[], 2), 0);
        assert_eq!(compute_string_value(&[1], 2), 1);
        assert_eq!(compute_string_value(&[0, 1], 2), 2); // NOT 1
        assert_eq!(compute_string_value(&[1, 1, 0], 2), 3);
        assert_eq!(compute_string_value(&[2, 1], 3), 5);
    }

    /// `updateTransitionsFromMorphism` (`:745-765`) groups digits **most**-significant-first
    /// — the opposite convention from [`compute_string_value`], and the reason the two
    /// halves of `convertNS` are not each other's mirror image.
    ///
    /// Checked against a hand-derived table: a 3-state base-2 counter `δ(q, d) = (2q + d)
    /// mod 3`, whose 2-digit grouping must satisfy `δ₂(q, 2a + b) = δ(δ(q, a), b)`.
    #[test]
    fn update_transitions_from_morphism_groups_digits_most_significant_first() {
        let delta = |q: usize, d: i32| (2 * q + d as usize) % 3;
        let d: Vec<BTreeMap<i32, Vec<usize>>> = (0..3)
            .map(|q| (0..2).map(|dig| (dig, vec![delta(q, dig)])).collect())
            .collect();
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 2],
            d,
        };

        update_transitions_from_morphism(&mut fa, 2);

        for q in 0..3 {
            assert_eq!(fa.d[q].len(), 4, "the grouped alphabet must be 0..3");
            for a in 0..2 {
                for b in 0..2 {
                    assert_eq!(
                        fa.d[q][&(2 * a + b)],
                        vec![delta(delta(q, a), b)],
                        "symbol {} of state {q} must be the two-step transition on ({a}, {b})",
                        2 * a + b
                    );
                }
            }
        }
        // `exponent <= 1` performs no extension at all (`for i in 2..=exponent`).
        let mut untouched = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 1, 2],
            d: (0..3)
                .map(|q| (0..2).map(|dig| (dig, vec![delta(q, dig)])).collect())
                .collect(),
        };
        let before = untouched.d.clone();
        update_transitions_from_morphism(&mut untouched, 1);
        assert_eq!(untouched.d, before);
    }

    /// The Java `isDeterministicAndTotal` this file uses is genuinely weaker than
    /// [`Fa::is_deterministic_and_total`], and the difference is load-bearing at
    /// [`convert_msd_base_to_exponent`]'s guard — pinned so a later "simplification" to the
    /// `Fa` method is caught.
    #[test]
    fn is_deterministic_and_total_java_is_weaker_than_the_fa_predicate() {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0, 0]); // present, but TWO destinations
        d0.insert(1, vec![0]);
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1],
            d: vec![d0],
        };
        assert!(
            is_deterministic_and_total_java(&fa),
            "Java only counts keys, so this is 'total' to it"
        );
        assert!(
            !fa.is_deterministic_and_total(),
            "this crate's own predicate additionally requires exactly one destination"
        );
    }

    /// `setAutomatonAlphabet` + the `NS.set(0, ...)` it always accompanies: both halves of
    /// this crate's per-track number-system stand-in must move together
    /// (`PORTING.md`'s parallel-vector ruling), and the encoder must follow the alphabet.
    #[test]
    fn set_number_system_and_alphabet_moves_every_parallel_track_field() {
        let mut a = epsilon_only(2, true);
        a.label = vec!["x".to_string()];

        set_number_system_and_alphabet(&mut a, false, 5);

        assert_eq!(a.alphabet, vec![vec![0, 1, 2, 3, 4]]);
        assert_eq!(a.fa.alphabet_size, 5);
        assert_eq!(a.msd, vec![Some(false)]);
        assert_eq!(a.all_reps.len(), 1);
        assert!(a.all_reps[0].is_none());
        assert_eq!(
            a.encoder(),
            &[1],
            "arity 1 ⇒ the encoder is [1] at any base"
        );
        assert_eq!(a.label, vec!["x".to_string()], "the track keeps its name");
    }

    /// `docs/WALNUT-BUGS.md` WB-001 through `convertNS`'s `k -> k^j` regrouping — the
    /// call site found by adversarial review of Phase 3b's U18, now asserting the
    /// CORRECT answer. Formerly `convert_ns_reaches_wb_001_when_regrouping_strands_a_state`;
    /// flipped, not deleted, per `CLAUDE.md`'s merge gate.
    ///
    /// The fixture is the 2-state parity automaton over `msd_2`: `q0` accepts (even
    /// length), state 1 rejects, every digit toggles. Converting it to `msd_4` groups two
    /// binary digits into one base-4 digit, and every base-4 digit therefore returns each
    /// state to itself — so state 1 becomes unreachable from `q0`, and
    /// `minimizeSelfWithOutput` (`:521`) runs Valmari on a table violating its
    /// reachability precondition. The parity automaton's own length parity is what makes
    /// this bite: **every** even-length binary word is a legal base-4 word, so the correct
    /// answer is the constant-`1` DFAO (accept everything).
    ///
    /// Pre-fix, both engines returned the constant-`0` DFAO (accept nothing), and the
    /// complementary odd-parity fixture (outputs swapped) returned `1` where the correct
    /// answer is `0` — the corruption inverted with the fixture, so it was never a lucky
    /// constant. Verified live against the unfixed `Walnut-all.jar` at the time
    /// (`convert evenmsd4 msd_4 u18even;`).
    ///
    /// **Re-verified live against a jar built from `walnut-java` commit `14509f1`**, the
    /// fix this port follows, with both fixtures. `convert evenmsd4 msd_4 u18even;` now
    /// writes:
    ///
    /// ```text
    /// msd_4
    ///
    /// 0 1
    /// 0 -> 1
    /// 1 -> 1
    /// 2 -> 1
    /// 3 -> 1
    ///
    /// 1 1
    /// 0 -> 1
    /// 1 -> 1
    /// 2 -> 1
    /// 3 -> 1
    /// ```
    ///
    /// — and `convert oddmsd4 msd_4 u18odd;` writes the same table with both outputs `0`.
    /// The assertions below pin exactly those two tables, so this test checks agreement
    /// with the fixed engine structurally, not merely that the port reached *a* correct
    /// answer. (Both are constant DFAOs carrying a redundant second state: `combine`
    /// rebuilds the DFAO from the per-output-value sub-automata and does not re-minimize
    /// the recombination, on both engines alike.)
    #[test]
    fn convert_ns_no_longer_corrupts_when_regrouping_strands_a_state() {
        // q0 accepts (even length), state 1 rejects; both digits toggle.
        let parity = |even_accepts: i32, odd_accepts: i32| {
            Automaton::new(
                Fa {
                    true_false: None,
                    q0: 0,
                    q: 2,
                    alphabet_size: 2,
                    o: vec![even_accepts, odd_accepts],
                    d: vec![
                        BTreeMap::from([(0, vec![1]), (1, vec![1])]),
                        BTreeMap::from([(0, vec![0]), (1, vec![0])]),
                    ],
                },
                vec![vec![0, 1]],
                vec!["x".to_string()],
                vec![Some(true)],
            )
        };

        let mut even = parity(1, 0);
        convert_ns(&mut even, true, 4, &mut crate::logging::Logging::new())
            .expect("the conversion must succeed");
        assert_eq!(even.msd, vec![Some(true)]);
        assert_eq!(even.alphabet, vec![vec![0, 1, 2, 3]]);
        assert_eq!(
            even.fa.o[even.fa.q0], 1,
            "every base-4 word has even binary length, so the answer is constant-1; \
             pre-fix WB-001 made this 0"
        );
        assert!(
            even.fa.accepts_word(&[0, 3, 2]),
            "constant-1: every base-4 word is accepted"
        );
        // The exact table the FIXED jar writes, captured live (see the doc comment):
        // two states, both output 1, every base-4 digit going to state 1 from both.
        // Structural, not just "the language is right" -- so the port is pinned to
        // Java's own post-fix output, not merely to a correct answer of its own.
        let all_digits_to_one: Vec<BTreeMap<i32, Vec<usize>>> = vec![
            BTreeMap::from([(0, vec![1]), (1, vec![1]), (2, vec![1]), (3, vec![1])]),
            BTreeMap::from([(0, vec![1]), (1, vec![1]), (2, vec![1]), (3, vec![1])]),
        ];
        assert_eq!(even.fa.q, 2);
        assert_eq!(even.fa.o, vec![1, 1]);
        assert_eq!(even.fa.d, all_digits_to_one);

        // The complementary fixture. Pre-fix the corruption inverted with it (0 became 1
        // here), which is how it was shown not to be a lucky constant; post-fix both
        // fixtures are simply right.
        let mut odd = parity(0, 1);
        convert_ns(&mut odd, true, 4, &mut crate::logging::Logging::new())
            .expect("the conversion must succeed");
        assert_eq!(
            odd.fa.o[odd.fa.q0], 0,
            "odd parity accepts no base-4 word; pre-fix WB-001 made this 1"
        );
        assert!(!odd.fa.accepts_word(&[0, 3, 2]));
        assert_eq!(odd.fa.q, 2);
        assert_eq!(odd.fa.o, vec![0, 0]);
        assert_eq!(odd.fa.d, all_digits_to_one);
    }

    /// The scoping witness for `convert_ns_round_trips_through_every_direction_and_base_step`
    /// below: `msd_2 -> msd_4 -> msd_2` is **not** the identity in general, and the reason
    /// is a genuine information loss in the conversion, not any bug.
    ///
    /// Same even-parity fixture as the test above (it accepts exactly the even-length
    /// binary words). Every base-4 word expands to an even-length binary word, so the
    /// `msd_4` image is constant-`1` — correct, and there is simply nothing left in it
    /// from which "even length only" could be reconstructed. Converting back therefore
    /// yields constant-`1` over `msd_2`, which accepts `1` (odd length) where the original
    /// rejects it.
    ///
    /// Written as a separate test rather than a sentence in that doc comment so the
    /// scoping cannot silently rot: if a future change made the round trip lossless here,
    /// this fails and the neighbouring test's "NOT universal" caveat gets re-examined.
    #[test]
    fn convert_ns_round_trip_is_lossy_on_a_length_sensitive_automaton() {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![1, 0],
                d: vec![
                    BTreeMap::from([(0, vec![1]), (1, vec![1])]),
                    BTreeMap::from([(0, vec![0]), (1, vec![0])]),
                ],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        assert!(a.fa.accepts_word(&[0, 1]), "even length accepted");
        assert!(!a.fa.accepts_word(&[1]), "odd length rejected");

        convert_ns(&mut a, true, 4, &mut crate::logging::Logging::new()).expect("msd_2 -> msd_4");
        convert_ns(&mut a, true, 2, &mut crate::logging::Logging::new()).expect("msd_4 -> msd_2");

        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert!(a.fa.accepts_word(&[0, 1]), "even length still accepted");
        assert!(
            a.fa.accepts_word(&[1]),
            "the round trip is LOSSY: odd length, rejected by the original, is now \
             accepted -- base-4 cannot express representation-length parity"
        );
    }

    /// Tier 4 (Walnut-independent): converting away and back is the identity on the
    /// LANGUAGE **of this fixture** (`x < 5` over base 2 — non-trivial and not
    /// reversal-symmetric, so a dropped or doubled reversal cannot pass), across every
    /// direction/base-power step listed below.
    ///
    /// # This is NOT the universal round-trip property, and must not be re-worded as one
    ///
    /// An earlier draft of this test claimed the identity held "for every combination this
    /// method supports". It does not.
    ///
    /// The counterexample originally cited here was WB-001: `msd_2 -> msd_4` on the
    /// 2-state parity automaton corrupted the language outright, so converting back could
    /// not restore it. **That reason is gone** — WB-001 is fixed (`walnut-java` commit
    /// `14509f1`); see `convert_ns_no_longer_corrupts_when_regrouping_strands_a_state`
    /// below, which now asserts the correct constant-`1` answer.
    ///
    /// The round trip is still not universal, for an unrelated and more basic reason that
    /// has nothing to do with any bug: `msd_2 -> msd_{2^j}` is a genuinely lossy
    /// restriction on words whose length is not a multiple of `j`. That very same parity
    /// fixture is the witness — it accepts exactly the even-length binary words, every
    /// base-4 word has even binary length, so the `msd_4` image is constant-`1` and the
    /// return trip cannot recover "even length only". Pinned directly by
    /// `convert_ns_round_trip_is_lossy_on_a_length_sensitive_automaton` below, so this
    /// scoping is a checked fact rather than a claim in prose. This fixture passes because
    /// `x < 5` is a genuine value predicate, insensitive to representation length.
    #[test]
    fn convert_ns_round_trips_through_every_direction_and_base_step() {
        // `x < 5` over msd_2, total: states 0..4 count digits, 5 is a dead sink.
        let d: Vec<BTreeMap<i32, Vec<usize>>> = vec![
            BTreeMap::from([(0, vec![0]), (1, vec![1])]),
            BTreeMap::from([(0, vec![2]), (1, vec![3])]),
            BTreeMap::from([(0, vec![3]), (1, vec![4])]),
            BTreeMap::from([(0, vec![4]), (1, vec![4])]),
            BTreeMap::from([(0, vec![4]), (1, vec![4])]),
        ];
        let original = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 5,
                alphabet_size: 2,
                o: vec![1, 1, 1, 1, 0],
                d,
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        );

        for (msd_mid, base_mid) in [(false, 2), (true, 4), (false, 4), (true, 8), (false, 8)] {
            let mut there = original.clone();
            convert_ns(
                &mut there,
                msd_mid,
                base_mid,
                &mut crate::logging::Logging::new(),
            )
            .expect("forward conversion");
            assert_eq!(there.msd, vec![Some(msd_mid)]);
            assert_eq!(there.alphabet, vec![util::int_range_list(base_mid)]);

            let mut back = there.clone();
            convert_ns(&mut back, true, 2, &mut crate::logging::Logging::new())
                .expect("reverse conversion");
            assert_eq!(back.msd, vec![Some(true)]);
            assert_eq!(back.alphabet, vec![vec![0, 1]]);

            let mut lhs = back.clone();
            let mut rhs = original.clone();
            lhs.fa.totalize(0);
            rhs.fa.totalize(0);
            assert_eq!(
                equiv::automaton_language_equivalent(&lhs, &rhs),
                Ok(true),
                "msd_2 -> {}_{base_mid} -> msd_2 must be the identity on the language",
                if msd_mid { "msd" } else { "lsd" }
            );
        }
    }

    // -------------------------------------------- Tier-4 property over `convert_ns`
    //
    // Phase 4, U31; scope note updated once WB-032 was fixed (`walnut-java` commit
    // `18b7c4b`, `bugfix/wb-032`, ported here as `exact_integer_exponent`). This property
    // is scoped to base-2 SOURCE automata purely because `arb_total_msd2_automaton` and
    // `expand_digit` below are base-2-specific plumbing, not because any other base is
    // untrustworthy: WB-032 being fixed means `convert_ns`'s exponent computation is
    // provably correct for every `(root, exponent)` an `int` alphabet can express, not just
    // powers of 2 — `exact_integer_exponent_recovers_the_exponent_over_the_full_int_alphabet
    // _sweep` above already proves that directly, non-circularly, root by root. What follows
    // is a genuinely independent SECOND check, at the level `convert_ns` itself operates on
    // (a real automaton's digit-regrouping), not just the exponent arithmetic in isolation.
    // Widening its generator to arbitrary source bases would be a reasonable follow-up but
    // is not needed to close WB-032, so it is not done here.

    /// `d` written as exactly `j` base-`k` digits, most significant first — the digit-level
    /// meaning of "base `k^j`". Computed here by plain integer arithmetic; nothing in
    /// `convert_ns`'s implementation is consulted.
    fn expand_digit(d: i32, k: i32, j: usize) -> Vec<i32> {
        let mut out = vec![0; j];
        let mut rest = d;
        for slot in (0..j).rev() {
            out[slot] = rest % k;
            rest /= k;
        }
        assert_eq!(rest, 0, "digit {d} does not fit in {j} base-{k} digits");
        out
    }

    /// Every base-`base` word of length `0..=max_len`.
    fn all_words_over(base: i32, max_len: usize) -> Vec<Vec<i32>> {
        let digits: Vec<i32> = (0..base).collect();
        let mut out = vec![Vec::new()];
        let mut frontier = vec![Vec::new()];
        for _ in 0..max_len {
            let mut next = Vec::new();
            for w in &frontier {
                for &d in &digits {
                    let mut w2 = w.clone();
                    w2.push(d);
                    next.push(w2);
                }
            }
            out.extend(next.iter().cloned());
            frontier = next;
        }
        out
    }

    /// Which states of `fa` are reachable from `q0` by a word whose LENGTH is a multiple
    /// of `j` — i.e. which states survive as reachable once `convertMsdBaseToExponent`
    /// re-keys the table by digit GROUPS of size `j` (`δ' = δ^j`). A plain BFS over
    /// `(state, depth mod j)` pairs, written here rather than reused from anywhere in the
    /// crate.
    fn reachable_at_a_multiple_of(fa: &Fa, j: usize) -> Vec<bool> {
        let mut seen = vec![vec![false; j]; fa.q];
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        seen[fa.q0][0] = true;
        queue.push_back((fa.q0, 0));
        while let Some((s, phase)) = queue.pop_front() {
            for dests in fa.d[s].values() {
                for &t in dests {
                    let next = (phase + 1) % j;
                    if !seen[t][next] {
                        seen[t][next] = true;
                        queue.push_back((t, next));
                    }
                }
            }
        }
        (0..fa.q).map(|s| seen[s][0]).collect()
    }

    /// Would `docs/WALNUT-BUGS.md` **WB-001** have fired when `fa` is regrouped into digit
    /// groups of size `j`? WB-001 is **fixed** (`walnut-java` commit `14509f1`), so this no
    /// longer gates anything: the property below used to `prop_assume!` these cases away
    /// and now runs on all of them. The predicate and its pin test are KEPT, not deleted,
    /// for two reasons — the property's own doc quantifies how much of the case space this
    /// used to discard (10.7%, measured), and that number stays meaningful only if the
    /// predicate it refers to still exists and is still exactly the condition it claims to
    /// be; and it remains the precise, derived characterisation of WB-001's trigger *on
    /// this code path*, which is the thing a future reader will want if the guard in
    /// [`crate::minimize::minimize`] is ever revisited.
    ///
    /// The derivation, from the call chain rather than approximated:
    ///
    /// * `convertMsdBaseToExponent` re-keys the transition table by digit groups, so the
    ///   reachable set collapses to [`reachable_at_a_multiple_of`] — a state reachable
    ///   only "mid-group" becomes unreachable from `q0`;
    /// * the very next statement is `minimizeSelfWithOutput`, i.e.
    ///   `WordAutomaton::minimize_with_output`, which does NOT hand the DFAO to Valmari
    ///   whole. It `uncombine`s it into one plain automaton **per distinct output value**
    ///   `v` — accepting exactly the states whose output is `v` — and runs
    ///   `determinize_and_minimize()` on each (already deterministic and total here, so
    ///   the `trim` step is skipped and Valmari sees an untrimmed table);
    /// * and Valmari's quirk needs `q0` to be unable to reach ANY accepting state *while
    ///   some accepting state exists* (`minimize.rs`'s "q0 aliasing quirk" docs).
    ///
    /// Per sub-automaton that read: `q0` reaches no state whose output is `v`, while some
    /// state has output `v` (true by construction — `v` is drawn from `fa.o`, over all
    /// states, reachable or not). So the exact condition was the one below.
    ///
    /// # Why not just "is every state reachable?"
    ///
    /// That was this helper's first form, and it is *sufficient but not necessary*: a
    /// stranded state whose output value ALSO occurs on some reachable state cannot
    /// trigger the quirk, because the sub-automaton for that value still has an accepting
    /// state `q0` can reach. Concrete counterexample — the WB-001 parity fixture's own
    /// transition table with both outputs equal: `q = 2`, `j = 2`, `0 --0,1--> 1` and
    /// `1 --0,1--> 0`, outputs `[1, 1]`. State `1` is reachable only at ODD lengths, so
    /// the all-states form said "skip"; but the only present output value is `1`, which
    /// `q0` itself carries, so `q0` is trivially co-reachable in the only sub-automaton
    /// and WB-001 could not fire. That refinement recovered 19.3% of the case space while
    /// the bug was still live; fixing the bug recovered the remaining 10.7%.
    fn wb_001_can_fire_on_regrouping(fa: &Fa, j: usize) -> bool {
        let reachable = reachable_at_a_multiple_of(fa, j);
        let present: BTreeSet<i32> = fa.o.iter().copied().collect();
        present
            .into_iter()
            .any(|v| !(0..fa.q).any(|s| reachable[s] && fa.o[s] == v))
    }

    /// Pins [`wb_001_can_fire_on_regrouping`] on both sides, so the refinement cannot
    /// silently rot into either a rubber stamp (which would let the property report the
    /// deliberately-ported WB-001 corruption as a failure) or the coarse all-states form
    /// it replaced (which threw away ~19% of the case space for nothing).
    ///
    /// Both fixtures share `convert_ns_reaches_wb_001_when_regrouping_strands_a_state`'s
    /// transition table — `0 --0,1--> 1`, `1 --0,1--> 0` — where state `1` is reachable
    /// only at ODD length and so is stranded by `j = 2` regrouping. Only the outputs
    /// differ, and that alone decides whether WB-001 can fire.
    #[test]
    fn the_wb_001_regrouping_predicate_keys_on_output_values_not_bare_reachability() {
        let parity = |o: Vec<i32>| Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o,
            d: vec![
                BTreeMap::from([(0, vec![1]), (1, vec![1])]),
                BTreeMap::from([(0, vec![0]), (1, vec![0])]),
            ],
        };

        // Distinct outputs: the sub-automaton for value `0` accepts only the stranded
        // state `1`, so `q0` reaches no accepting state there. WB-001 fires.
        assert!(
            wb_001_can_fire_on_regrouping(&parity(vec![1, 0]), 2),
            "the pinned WB-001 fixture must still be rejected"
        );
        assert!(wb_001_can_fire_on_regrouping(&parity(vec![0, 1]), 2));

        // Equal outputs: state `1` is just as stranded, but the only present output value
        // is one `q0` itself carries, so every sub-automaton has an accepting state `q0`
        // reaches and WB-001 provably cannot fire. The coarse all-states predicate
        // discarded this shape.
        assert!(!wb_001_can_fire_on_regrouping(&parity(vec![1, 1]), 2));
        assert!(!wb_001_can_fire_on_regrouping(&parity(vec![0, 0]), 2));
        assert!(
            !reachable_at_a_multiple_of(&parity(vec![1, 1]), 2)[1],
            "the equal-output fixture really does strand state 1 -- otherwise the \
             assertions above would be vacuous"
        );
    }

    /// A TOTAL random single-track `msd_2` automaton (every state has both digits), so
    /// `convert_ns`'s own `totalize` never runs and the state set the oracle reasons about
    /// is exactly the generated one.
    fn arb_total_msd2_automaton(q_max: usize) -> impl Strategy<Value = Automaton> {
        (1..=q_max).prop_flat_map(move |q| {
            let o = prop::collection::vec(0i32..=1, q);
            let trans = prop::collection::vec(prop::collection::vec(0usize..q, 2), q);
            (o, trans).prop_map(move |(o, trans)| {
                let d = trans
                    .iter()
                    .map(|row| {
                        row.iter()
                            .enumerate()
                            .map(|(sym, &dest)| (sym as i32, vec![dest]))
                            .collect::<BTreeMap<i32, Vec<usize>>>()
                    })
                    .collect();
                Automaton::new(
                    Fa {
                        true_false: None,
                        q0: 0,
                        q,
                        alphabet_size: 2,
                        o,
                        d,
                    },
                    vec![vec![0, 1]],
                    vec!["x".to_string()],
                    vec![Some(true)],
                )
            })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Tier-4: `convertNS` from `msd_2` to `msd_{2^j}` denotes the same set of
        /// integers, checked against a from-scratch digit-regrouping oracle.
        ///
        /// The oracle is `expand_digit` — "one base-`2^j` digit *is* `j` base-2 digits" —
        /// applied to the ORIGINAL automaton, and nothing else; it never calls
        /// `convert_ns`, `convert_msd_base_to_exponent`, `exact_integer_exponent` or
        /// `equiv`. Because `value_{2^j}(w) == value_2(expand(w))` (asserted below, so
        /// the digit-level statement really is the integer-level one), this says exactly
        /// that the converted automaton accepts the same integers as the original —
        /// presented to each in its own base, at the corresponding representation length.
        ///
        /// Restricted to base 2 for the oracle's own plumbing (`expand_digit`/
        /// `arb_total_msd2_automaton` are base-2-specific), not because any other base
        /// is untrustworthy — WB-032 is fixed, so the port is unconditionally correct here
        /// now, not merely bug-compatible; see this file's Tier-4
        /// `exact_integer_exponent_recovers_the_exponent_over_the_full_int_alphabet_sweep`
        /// for the non-circular check that covers every other base.
        ///
        /// # Formerly scoped away from WB-001; now UNSCOPED
        ///
        /// While `docs/WALNUT-BUGS.md` WB-001 was live this property carried a
        /// `prop_assume!(!wb_001_can_fire_on_regrouping(&a.fa, j))`: on those inputs both
        /// engines genuinely corrupted the language, so the property could not have held.
        /// Measured over 4,548 generated inputs (a one-off instrumented run), that filter
        /// rejected **10.7%** of the case space (the coarser all-states predicate it
        /// replaced rejected 30.0%). WB-001 is fixed (`walnut-java` commit `14509f1`,
        /// ported in [`crate::minimize::minimize`]), the filter is **removed**, and those
        /// 10.7% are now asserted like every other case. `wb_001_can_fire_on_regrouping`
        /// is kept — see its own doc comment for why — but no longer gates anything.
        #[test]
        fn convert_ns_to_a_power_of_two_base_preserves_the_integer_language(
            a in arb_total_msd2_automaton(4),
            j in 2usize..=3,
        ) {
            let to_base = 2i32.pow(j as u32);
            let mut converted = a.clone();
            convert_ns(&mut converted, true, to_base, &mut crate::logging::Logging::new()).expect("msd_2 -> msd_2^j must succeed");
            prop_assert_eq!(&converted.alphabet, &vec![util::int_range_list(to_base)]);
            prop_assert_eq!(&converted.msd, &vec![Some(true)]);

            for w in all_words_over(to_base, 3) {
                let expanded: Vec<i32> =
                    w.iter().flat_map(|&d| expand_digit(d, 2, j)).collect();
                // The oracle's own sanity guard: the two words really do denote the same
                // integer, so "same acceptance" really is "same integer language".
                let v_hi = w.iter().fold(0i64, |acc, &d| acc * i64::from(to_base) + i64::from(d));
                let v_lo = expanded.iter().fold(0i64, |acc, &d| acc * 2 + i64::from(d));
                prop_assert_eq!(v_hi, v_lo);

                prop_assert_eq!(
                    converted.fa.accepts_word(&w),
                    a.fa.accepts_word(&expanded),
                    "msd_2 -> msd_{} disagrees on base-{} word {:?} (= base-2 {:?})",
                    to_base, to_base, w, expanded
                );
            }
        }
    }
}
