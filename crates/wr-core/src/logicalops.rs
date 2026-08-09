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
//! # `applyAllRepresentations` — inert, not ported
//!
//! `totalizeCrossProduct` (`:121`), `not` (`:163`) and `rightQuotient` (`:228`) each
//! call `Automaton.applyAllRepresentations()`. That method's body
//! (`Automaton.java:253-270`) only does anything for a track whose `NumberSystem` is
//! non-null AND `useAllRepresentations()`. That flag
//! (`NumberSystem.flagUseAllRepresentations`, `NumberSystem.java:130`) starts `true` but
//! is cleared in the constructor (`:147-150`) whenever `loadAutomatonOrNull` finds no
//! `<name>.txt` "set of all representations" automaton in the custom-bases directory
//! (`:304-...`) — which is exactly how the Fibonacci/Ostrowski/Pell family is
//! configured, and never the case for a plain base-*k*. All of that family is DROPPED
//! from this port's scope, so `applyAllRepresentations` is a guaranteed no-op here and
//! is not ported (the same judgment `product.rs`'s module docs already recorded for
//! `not`'s call to it).
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
//! # Not ported (investigated, with the exact blocker)
//!
//! - **`convertNS` (`:455-529`) + `convertMsdBaseToExponent` (`:535-560`) +
//!   `convertLsdBaseToRoot` (`:566-657`) + `setAutomatonAlphabet` (`:662-665`) +
//!   `computeStringValue` (`:671-677`).** The `WordAutomaton` dependency is
//!   **unconditional**, not confined to a DFAO-only branch: `convertNS`'s same-base
//!   msd<->lsd branch ends in `WordAutomaton.reverseWithOutput` (`:475`), and on the
//!   general path `fromBase != toBase` forces at least one of `fromBase != commonRoot`
//!   / `toBase != commonRoot` to hold, each of which calls
//!   `WordAutomaton.minimizeSelfWithOutput` (`:509` / `:521`). `minimizeSelfWithOutput`
//!   (`WordAutomaton.java:231-234`) delegates to `minimizeWithOutput`
//!   (`WordAutomaton.java:215-228`), which is built out of `WordAutomaton.uncombine` +
//!   `AutomatonLogicalOps.combine` — and `combine` is itself out of scope (needs
//!   `Prover.COMBINE` product-mode state). `convertNS` additionally needs a real
//!   `NumberSystem` object for `parseBase()`/`new NumberSystem(name)`, which
//!   `crate::numsys` does not yet provide (it currently exposes only `less_than_msd`).
//!   No stub is invented here.
//! - **`combine` (`:679-722`), `buildTransitionsFromMorphism` (`:727-740`),
//!   `updateTransitionsFromMorphism` (`:745-765`), `buildInitialMorphism`
//!   (`:771-781`).** `combine` needs `Automaton.combineIndex`/`combineOutputs` plus
//!   `Prover.COMBINE`'s `determineOutput` mode; the three morphism helpers exist solely
//!   to serve `convertMsdBaseToExponent`.
//! - **`removeLeadingZeros` (`:343-367`) + `removeLeadingZerosHelper` (`:375-405`).**
//!   Not a CLI-display concern — its callers are `ProverHelper.java:52`,
//!   `LogicalOperator.java:151` (the `I` quantifier) and `Test.java:43`, i.e. the
//!   evaluation pipeline (`wr-logic` scope). Its ORIGINAL blocker — it builds
//!   `new Automaton(false)` as the fold's identity (`:356`) and
//!   `removeLeadingZerosHelper` returns `new AutomatonDFA(true)` for a non-arithmetic
//!   track (`:381-383`), both TRUE/FALSE automata — **is removed by U0**; both are now
//!   expressible ([`crate::automaton::Automaton::true_false`]). It stays unported here
//!   only because it belongs to the `I`-quantifier unit that consumes it (see the
//!   Phase-3 plan's U10), and it still needs
//!   `AutomatonQuantification.validateLabels`.
//!
//! # `fa.setCanonized(false)`
//!
//! `fixLeadingZerosProblem` (`:273`) and `fixTrailingZerosProblem` (`:326`) each clear
//! `FA`'s private `canonized` memo flag. `Fa` carries no such flag (it always
//! recomputes — see `fa.rs`'s doc comment on `Fa::canonicalize`), so there is nothing
//! to clear here.
//!
//! # Logging / timing
//!
//! Every method in the Java file brackets its work in `logMessage`/`Logging.indent()`
//! and `System.currentTimeMillis()` timing. This crate has no `Logging` module
//! (diagnostic output, not behavior — same call this file's siblings already made), so
//! none of it is ported.

use crate::automaton::{Automaton, AutomatonDFA};
use crate::fa::Fa;
use crate::minimize::{minimize, MinimizeError};
use crate::product::{cross_product_and_minimize, BooleanOp};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

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
fn totalize(fa: &mut Fa) {
    let sink_state = fa.q;
    let mut needs_sink = false;
    for q in 0..fa.q {
        for sym in 0..fa.alphabet_size as i32 {
            if let std::collections::btree_map::Entry::Vacant(entry) = fa.d[q].entry(sym) {
                entry.insert(vec![sink_state]);
                needs_sink = true;
            }
        }
    }
    if needs_sink {
        fa.o.push(0);
        fa.q += 1;
        let mut sink_row = BTreeMap::new();
        for sym in 0..fa.alphabet_size as i32 {
            sink_row.insert(sym, vec![sink_state]);
        }
        fa.d.push(sink_row);
    }
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
/// trims. So `docs/WALNUT-BUGS.md` WB-001 is reachable through every caller of this
/// helper ([`not`], [`fix_trailing_zeros_problem`]) whose input has a state unreachable
/// from `q0`. Ported verbatim per `CLAUDE.md`'s mechanical-port rule.
fn just_minimize(fa: &Fa) -> Fa {
    match minimize(fa) {
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
/// preserves — is carried by the track's alphabet here and needs no touching.
fn flip_ns(msd: &mut [Option<bool>]) {
    for slot in msd.iter_mut() {
        if let Some(is_msd) = slot.as_mut() {
            *is_msd = !*is_msd;
        }
    }
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
pub fn and(a: &Automaton, b: &Automaton) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        if a.fa.is_true_false_automaton() {
            return if a.fa.is_true_automaton() {
                b.as_dfa()
            } else {
                AutomatonDFA::true_false(false)
            };
        }
        return and(b, a); // and is symmetric
    }
    cross_product_and_minimize(a, b, |p, q| BooleanOp::And.combine(p, q))
}

/// `AutomatonLogicalOps.totalizeCrossProduct` (`:112-126`) — the precondition-enforcing
/// wrapper shared by [`or`]/[`xor`]/[`imply`]/[`iff`]. Totalizes BOTH operands in
/// place (Java: `A.fa.totalize(); B.fa.totalize();`, `:117-118`) and only then runs the
/// cross product. See this module's docs for why skipping this computes the wrong
/// language for these four connectives but not for [`and`].
fn totalize_cross_product(a: &mut Automaton, b: &mut Automaton, op: BooleanOp) -> AutomatonDFA {
    totalize(&mut a.fa);
    totalize(&mut b.fa);
    cross_product_and_minimize(a, b, |p, q| op.combine(p, q))
}

/// `AutomatonLogicalOps.or` (`:67-75`) — `L(A) ∪ L(B)`. Totalizes both operands in
/// place first; see [`totalize_cross_product`]. The TRUE/FALSE short-circuit
/// (`:68-73`) runs first and does NOT totalize (see this module's docs).
pub fn or(a: &mut Automaton, b: &mut Automaton) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        if a.fa.is_true_false_automaton() {
            return if a.fa.is_true_automaton() {
                AutomatonDFA::true_false(true)
            } else {
                b.as_dfa()
            };
        }
        return or(b, a); // or is symmetric
    }
    totalize_cross_product(a, b, BooleanOp::Or)
}

/// `AutomatonLogicalOps.xor` (`:80-92`) — symmetric difference. Totalizes both operands
/// in place first; see [`totalize_cross_product`]. The TRUE/FALSE short-circuit
/// (`:81-90`) runs first and does NOT totalize (see this module's docs).
pub fn xor(a: &mut Automaton, b: &mut Automaton) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        if a.fa.is_true_false_automaton() {
            // Java: `result = B.asDFA(); if (A.isTRUE) return not(result); return result;`
            // — i.e. `TRUE xor B == not(B)`, `FALSE xor B == B`. When `b` is trivial too,
            // `not` takes its own trivial branch and just flips the truth value.
            let result = b.as_dfa();
            if a.fa.is_true_automaton() {
                return not(result);
            }
            return result;
        }
        return xor(b, a); // xor is symmetric
    }
    totalize_cross_product(a, b, BooleanOp::Xor)
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
pub fn imply(a: &mut Automaton, b: &mut Automaton) -> AutomatonDFA {
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
            not(a.as_dfa())
        };
    }
    totalize_cross_product(a, b, BooleanOp::Imply)
}

/// `AutomatonLogicalOps.iff` (`:131-139`) — the complement of [`xor`]. Totalizes both
/// operands in place first; see [`totalize_cross_product`].
///
/// The TRUE/FALSE short-circuit (`:132-136`) is defined by RECURSION rather than by a
/// truth table: `and(imply(A, B), imply(B, A))`. Ported verbatim. This terminates and
/// never reaches [`crate::product`]: with at least one operand trivial, each `imply`
/// takes its own short-circuit, and at least one of the two results is itself trivial,
/// so the closing [`and`] short-circuits too.
pub fn iff(a: &mut Automaton, b: &mut Automaton) -> AutomatonDFA {
    if a.fa.is_true_false_automaton() || b.fa.is_true_false_automaton() {
        let c = imply(a, b);
        let d = imply(b, a);
        return and(c.automaton(), d.automaton());
    }
    totalize_cross_product(a, b, BooleanOp::Iff)
}

/// `AutomatonLogicalOps.not(AutomatonDFA)` (`:144-170`) — complementation.
///
/// Java's working sequence is `totalize()` (`:160`), `flipOutput()` (`:161`),
/// `justMinimize()` (`:162`) — all three ported below, preceded by the
/// `isTRUE_FALSE_AUTOMATON` short-circuit (`:146-149`, U0), which flips only
/// `TRUE_AUTOMATON` and returns the same (still-trivial) automaton. Only
/// `applyAllRepresentations()`/`convertNFAtoDFA()` have no analog here (see this
/// module's docs). Totalization is what makes
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
pub fn not(a: AutomatonDFA) -> AutomatonDFA {
    let mut m = a.into_automaton();
    if m.fa.is_true_false_automaton() {
        // Java mutates in place: `setTRUE_AUTOMATON(!isTRUE_AUTOMATON())`, leaving
        // `TRUE_FALSE_AUTOMATON` set, and returns the SAME object — so any stale
        // `q`/alphabet the argument was carrying survives. Rebuilding a fresh trivial
        // automaton here instead is the one difference, and it is unobservable: every
        // reader of a trivial automaton consults only `true_false` (see `crate::fa`'s
        // module docs).
        return AutomatonDFA::true_false(!m.fa.is_true_automaton());
    }
    totalize(&mut m.fa);
    flip_output(&mut m.fa);
    m.fa = just_minimize(&m.fa);
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
/// `docs/WALNUT-BUGS.md` WB-001 is reachable through the closing
/// `M.determinizeAndMinimize()` (`:227`) exactly as it is in Java, since `M` inherits
/// `A`'s (possibly not-fully-`q0`-reachable) state set unchanged.
pub fn right_quotient(a: &Automaton, b: &Automaton, skip_subset_check: bool) -> Automaton {
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
    other_clone.msd = a.msd.clone();

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

        let intersection = and(&t, &other_clone);
        m.fa.o[i] = i32::from(!intersection.automaton().is_empty());
    }

    m.determinize_and_minimize();
    m.force_canonize();
    m
}

/// `AutomatonLogicalOps.leftQuotient` (`:237-256`) — `L(B) \ L(A) = { z : ∃ w ∈ L(B),
/// wz ∈ L(A) }`, built by reversing both operands, taking a [`right_quotient`], and
/// reversing back.
///
/// # Panics
///
/// If `A`'s alphabet is not a subset of `B`'s (`:242-244`, `WalnutException` message
/// ported verbatim).
///
/// # A genuine Walnut (Java) defect this path carries (candidate for `docs/WALNUT-BUGS.md`)
///
/// The guard above checks `isSubsetA(A, B)` — "`A` ⊆ `B`" (`:242`). But the
/// `rightQuotient(M1, M2, true)` it then performs (`:248`) re-encodes `M2`'s (= `B`'s)
/// symbols under `M1`'s (= `A`'s) alphabet, which needs the OPPOSITE containment,
/// `B` ⊆ `A` — that is exactly what `rightQuotient`'s own guard (`:182`) would have
/// demanded, and it is precisely the guard `skipSubsetCheck = true` disables. The two
/// agree only when the alphabets are equal as sets. So `leftquo` on `A` over `{0,1}`
/// and `B` over `{0,1,2}` passes `leftQuotient`'s check, then hands `RichAlphabet.encode`
/// (`RichAlphabet.java:110-116`) a digit its `A.get(i).indexOf(...)` cannot find,
/// yielding `-1` and a silently corrupt (possibly negative) symbol id. Reachable from
/// the plain CLI: `Main/Commands/Quotient.java:17-23` reads both automata from `.txt`
/// files with no alphabet normalization in between.
///
/// This port surfaces it as a **panic** rather than a corrupt encoding, because
/// [`Automaton::encode`] already panics on a digit absent from its track's alphabet — a
/// pre-existing, documented improvement of this crate over Java's silent `indexOf ==
/// -1` (see `automaton.rs`'s doc comment on `encode`), not a fix introduced here.
pub fn left_quotient(a: &Automaton, b: &Automaton) -> Automaton {
    assert!(
        is_subset_alphabet(&a.alphabet, &b.alphabet),
        "First A's alphabet must be a subset of the second A's alphabet for left quotient."
    );

    let m1 = reverse_and_canonize(a);
    let m2 = reverse_and_canonize(b);
    let mut m = right_quotient(&m1, &m2, true);

    reverse(&mut m, true);
    m
}

/// `AutomatonLogicalOps.reverseAndCanonize` (`:258-263`, `private`). Note the `true`:
/// each reversal here also flips the operands' msd/lsd flags, which is why
/// [`left_quotient`]'s closing `reverse(M, true)` restores the original direction
/// rather than inverting it.
fn reverse_and_canonize(a: &Automaton) -> Automaton {
    let mut m1 = a.clone();
    reverse(&mut m1, true);
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
    let zero = a.determine_zero();
    let initial_state = zero_reachable_states(&mut a.fa, zero);
    a.determinize_and_minimize_from(&initial_state);
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
pub fn fix_trailing_zeros_problem(a: &mut Automaton) {
    let zero = a.determine_zero();
    if set_states_reachable_to_final_states_by_zeros(&mut a.fa, zero) {
        a.fa = just_minimize(&a.fa);
    }
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
    // `if (A.fa.isTRUE_FALSE_AUTOMATON()) return;` (`:415`, U0). Note this returns
    // BEFORE the `flipNS` step, so reversing a trivial automaton does not flip its
    // (empty) msd list either — faithful, and vacuous since the list is empty.
    if a.fa.is_true_false_automaton() {
        return;
    }
    let initial: BTreeSet<usize> = [a.fa.q0].into_iter().collect();
    let set_of_final_states = a.fa.reverse(&initial);
    a.determinize_and_minimize_from(&set_of_final_states);

    if reverse_msd {
        flip_ns(&mut a.msd);
    }
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
        totalize(&mut fa);
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
        totalize(&mut fa);
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
        totalize(&mut fa);
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
        let mut msd = vec![Some(true), None, Some(false)];
        flip_ns(&mut msd);
        assert_eq!(msd, vec![Some(false), None, Some(true)]);
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
        let n = and(&a, &b);

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
        let n = or(&mut a, &mut b);

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
        let n = or(&mut a, &mut b);

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
        let n = xor(&mut a, &mut b);

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
        let n = imply(&mut a, &mut b);

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
        let swapped = imply(&mut b2, &mut a2);
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
        let n = iff(&mut a, &mut b);

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
        let n = not(single_track(original.clone(), Some(true)).as_dfa());

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
        let once = not(single_track(original.clone(), Some(true)).as_dfa());
        let twice = not(once);
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
        let n = not(a.as_dfa());
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

            let n_and = and(&build(&a_fa), &build(&b_fa));
            assert_connective_matches_oracle(&n_and, &a_fa, &b_fa, |p, q| p && q);

            let n_or = or(&mut build(&a_fa), &mut build(&b_fa));
            assert_connective_matches_oracle(&n_or, &a_fa, &b_fa, |p, q| p || q);

            let n_xor = xor(&mut build(&a_fa), &mut build(&b_fa));
            assert_connective_matches_oracle(&n_xor, &a_fa, &b_fa, |p, q| p != q);

            let n_imply = imply(&mut build(&a_fa), &mut build(&b_fa));
            assert_connective_matches_oracle(&n_imply, &a_fa, &b_fa, |p, q| !p || q);

            let n_iff = iff(&mut build(&a_fa), &mut build(&b_fa));
            assert_connective_matches_oracle(&n_iff, &a_fa, &b_fa, |p, q| p == q);
        }

        /// `not` against the oracle. The input is TRIMMED first, deliberately: `not`'s
        /// `justMinimize` step establishes none of `minimize`'s `q0`-reachability
        /// precondition (faithfully -- see [`just_minimize`]), so a randomly-generated
        /// automaton with a state unreachable from `q0` can legitimately hit
        /// `docs/WALNUT-BUGS.md` WB-001 and produce a "wrong" complement. That is
        /// ported behavior, not a port defect, so the generator is constrained to the
        /// shape where WB-001 provably cannot fire rather than the property being
        /// weakened to accommodate it.
        #[test]
        fn not_matches_the_complement_oracle(fa in arb_partial_dfa(4, 2)) {
            let trimmed = crate::trim::trim(&fa);
            let n = not(single_track(trimmed.clone(), Some(true)).as_dfa());

            let mut trimmed_total = trimmed;
            trimmed_total.totalize(0);
            let expected = equiv::complement(&trimmed_total).unwrap();
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
        /// `a_fa`/`b_fa` are TRIMMED first, deliberately -- same rationale as
        /// `not_matches_the_complement_oracle` above: `not_a`/`not_b` below call `not`
        /// directly on `a_fa`/`b_fa`, and `not`'s `justMinimize` step establishes none
        /// of `minimize`'s `q0`-reachability precondition (faithfully -- see
        /// `just_minimize`'s doc comment). A randomly-generated automaton with a state
        /// unreachable from `q0` can legitimately hit `docs/WALNUT-BUGS.md` WB-001 and
        /// produce a "wrong" complement there. That is ported behavior, not a port
        /// defect (confirmed by first reproducing the untrimmed failure and matching
        /// it to WB-001's documented trigger shape -- a non-accepting, unreachable-
        /// from-nothing-else q0 plus a separate unreachable accepting state -- rather
        /// than assuming it away), so the generator is constrained to the shape where
        /// WB-001 provably cannot fire rather than the property being weakened to
        /// accommodate an unrelated bug.
        #[test]
        fn de_morgan_not_and_equals_or_of_nots(
            a_fa in arb_partial_dfa(4, 2),
            b_fa in arb_partial_dfa(4, 2),
        ) {
            let a_fa = crate::trim::trim(&a_fa);
            let b_fa = crate::trim::trim(&b_fa);
            let build = |fa: &Fa| single_track(fa.clone(), Some(true));

            // LHS: ¬(A ∧ B). `and` does not mutate its operands, so fresh clones
            // aren't strictly needed here, but built the same way as the RHS for
            // symmetry/readability.
            let and_ab = and(&build(&a_fa), &build(&b_fa));
            let lhs = not(and_ab);

            // RHS: ¬A ∨ ¬B. `or` totalizes its operands in place, so each side needs
            // its own fresh automaton -- these are one-shot, not reused afterward.
            let mut not_a = not(build(&a_fa).as_dfa()).into_automaton();
            let mut not_b = not(build(&b_fa).as_dfa()).into_automaton();
            let rhs = or(&mut not_a, &mut not_b);

            let mut lhs_fa = lhs.automaton().fa.clone();
            lhs_fa.totalize(0);
            let mut rhs_fa = rhs.automaton().fa.clone();
            rhs_fa.totalize(0);
            prop_assert_eq!(equiv::language_equivalent(&lhs_fa, &rhs_fa), Ok(true));
        }

        /// ¬(A ∨ B) ≡ ¬A ∧ ¬B, computed with the real `and`/`or`/`not` from this file.
        /// `a_fa`/`b_fa` are trimmed first for the same WB-001 reason documented on
        /// `de_morgan_not_and_equals_or_of_nots` above.
        #[test]
        fn de_morgan_not_or_equals_and_of_nots(
            a_fa in arb_partial_dfa(4, 2),
            b_fa in arb_partial_dfa(4, 2),
        ) {
            let a_fa = crate::trim::trim(&a_fa);
            let b_fa = crate::trim::trim(&b_fa);
            let build = |fa: &Fa| single_track(fa.clone(), Some(true));

            // LHS: ¬(A ∨ B).
            let or_ab = or(&mut build(&a_fa), &mut build(&b_fa));
            let lhs = not(or_ab);

            // RHS: ¬A ∧ ¬B (`and` doesn't mutate, so no extra `mut` needed here).
            let not_a = not(build(&a_fa).as_dfa()).into_automaton();
            let not_b = not(build(&b_fa).as_dfa()).into_automaton();
            let rhs = and(&not_a, &not_b);

            let mut lhs_fa = lhs.automaton().fa.clone();
            lhs_fa.totalize(0);
            let mut rhs_fa = rhs.automaton().fa.clone();
            rhs_fa.totalize(0);
            prop_assert_eq!(equiv::language_equivalent(&lhs_fa, &rhs_fa), Ok(true));
        }
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

        fix_trailing_zeros_problem(&mut a);

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
        fix_trailing_zeros_problem(&mut a);
        assert_eq!(
            a.fa.q, 3,
            "no change was needed, so justMinimize must not have run"
        );
        assert_eq!(a.fa.o, vec![0, 1, 1]);
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
        let _ = right_quotient(&a, &b, false);
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
        let _ = right_quotient(&a, &b, false);
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

        let m = right_quotient(&a, &b, true);

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

        let m = right_quotient(&a, &b, false);

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

        let m = right_quotient(&a, &b, false);

        // Same hand-derived answer as the bound case: {"01"} / {"1"} = {"0"}.
        assert!(m.fa.accepts_word(&[0]));
        assert!(!m.fa.accepts_word(&[]));
        assert!(!m.fa.accepts_word(&[1]));
        assert!(!m.fa.accepts_word(&[0, 1]));
    }

    #[test]
    #[should_panic(
        expected = "First A's alphabet must be a subset of the second A's alphabet for left quotient."
    )]
    fn left_quotient_panics_when_the_first_alphabet_is_not_a_subset() {
        // Replicates AutomatonLogicalOpsTest.testLeftQuotientThrowsWhenFirstAlphabetNotSubset.
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
        let _ = left_quotient(&a, &b);
    }

    #[test]
    #[should_panic(
        expected = "First A's alphabet must be a subset of the second A's alphabet for left quotient."
    )]
    fn left_quotient_subset_guard_is_direction_sensitive() {
        // Same mutation-tested gap as `right_quotient_subset_guard_is_direction_sensitive`,
        // mirrored: arity 1 on both sides, A's alphabet a strict SUPERset of B's, so
        // only `isSubsetA(A, B)` (AutomatonLogicalOps.java:242) rejects it.
        let a = Automaton::new(
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
        let b = single_track(exactly_one(), Some(true));
        assert!(
            is_subset_alphabet(&b.alphabet, &a.alphabet),
            "the OPPOSITE containment does hold, so a swapped guard would pass"
        );
        let _ = left_quotient(&a, &b);
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

        let m = left_quotient(&a, &b);

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

    #[test]
    #[should_panic]
    fn left_quotient_panics_on_the_wb_010_trigger_the_guard_misses() {
        // Pins WB-010 (docs/WALNUT-BUGS.md) directly, not just the guard's DIRECTION:
        // the two `*_direction_sensitive` tests above only show `left_quotient`'s
        // guard rejects inputs Java's guard would also reject (faithful direction).
        // Neither demonstrates the actual defect -- an input the guard WRONGLY
        // ACCEPTS because `A`'s alphabet genuinely is a subset of `B`'s (as sets),
        // while `right_quotient`'s internal re-encode still needs the OPPOSITE
        // containment. A over {0,1}, B over {0,1,2}: `is_subset_alphabet(&a.alphabet,
        // &b.alphabet)` is true (the guard passes, matching Java), but B's digit `2`
        // has no counterpart in A's alphabet, so the internal re-encode panics (this
        // crate's improvement over Java's silent `indexOf == -1` corruption -- see
        // this function's doc comment). A regression that "fixed" the guard direction
        // (forbidden without explicit sign-off, `CLAUDE.md`) would make this test
        // panic with a DIFFERENT message (the guard's own assert) instead of
        // `Automaton::encode`'s; a regression that silently swallowed the encode
        // panic would make it stop panicking at all -- either way this test would
        // catch it, unlike the two direction-only tests above.
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
        // `b` must have a REACHABLE, ACCEPTING run over digit 2 -- a bare self-loop
        // on a non-accepting state disappears entirely under `reverse_and_canonize`
        // (nothing was accepting, so `Fa::reverse` seeds the reversal with an EMPTY
        // initial set and the whole automaton collapses to a transitionless
        // 1-state rejector, silently removing the problematic digit before
        // `right_quotient` ever sees it -- a real mistake this test caught only by
        // actually running it, not by reasoning about it). L(b) = {"2"}: state0
        // (non-accepting, initial) --2--> state1 (accepting, dead end).
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
            "sanity: leftQuotient's own guard (A subset B) passes"
        );
        assert!(
            !is_subset_alphabet(&b.alphabet, &a.alphabet),
            "sanity: but the containment rightQuotient's re-encode actually needs \
             (B subset A) does NOT hold"
        );
        let _ = left_quotient(&a, &b);
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
                &and(&Automaton::true_false(t), &p_automaton()),
                if t { Expect::P } else { Expect::Trivial(false) },
                &format!("and({t}, P)"),
            );
            // The swap-recursion arm (`:49`, "and is symmetric").
            check(
                &and(&p_automaton(), &Automaton::true_false(t)),
                if t { Expect::P } else { Expect::Trivial(false) },
                &format!("and(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &and(&Automaton::true_false(t), &Automaton::true_false(u)),
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
                &or(&mut Automaton::true_false(t), &mut p_automaton()),
                if t { Expect::Trivial(true) } else { Expect::P },
                &format!("or({t}, P)"),
            );
            check(
                &or(&mut p_automaton(), &mut Automaton::true_false(t)),
                if t { Expect::Trivial(true) } else { Expect::P },
                &format!("or(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &or(&mut Automaton::true_false(t), &mut Automaton::true_false(u)),
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
                &xor(&mut Automaton::true_false(t), &mut p_automaton()),
                if t { Expect::NotP } else { Expect::P },
                &format!("xor({t}, P)"),
            );
            check(
                &xor(&mut p_automaton(), &mut Automaton::true_false(t)),
                if t { Expect::NotP } else { Expect::P },
                &format!("xor(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &xor(&mut Automaton::true_false(t), &mut Automaton::true_false(u)),
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
                &imply(&mut Automaton::true_false(t), &mut p_automaton()),
                if t { Expect::P } else { Expect::Trivial(true) },
                &format!("imply({t}, P)"),
            );
            // X -> TRUE == TRUE; X -> FALSE == not X (`:103-107`). This is the arm that
            // cannot be reached by swapping, and the one whose `B.isTRUE_AUTOMATON()`
            // read has no enclosing `isTRUE_FALSE_AUTOMATON()` check in Java.
            check(
                &imply(&mut p_automaton(), &mut Automaton::true_false(t)),
                if t {
                    Expect::Trivial(true)
                } else {
                    Expect::NotP
                },
                &format!("imply(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &imply(&mut Automaton::true_false(t), &mut Automaton::true_false(u)),
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
                &iff(&mut Automaton::true_false(t), &mut p_automaton()),
                if t { Expect::P } else { Expect::NotP },
                &format!("iff({t}, P)"),
            );
            check(
                &iff(&mut p_automaton(), &mut Automaton::true_false(t)),
                if t { Expect::P } else { Expect::NotP },
                &format!("iff(P, {t})"),
            );
            for u in [true, false] {
                check(
                    &iff(&mut Automaton::true_false(t), &mut Automaton::true_false(u)),
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
                or as fn(&mut Automaton, &mut Automaton) -> AutomatonDFA,
                xor,
                imply,
                iff,
            ] {
                let mut p = p_automaton();
                let mut triv = Automaton::true_false(t);
                let _ = op(&mut triv, &mut p);
                assert_eq!(p.fa.q, 2, "operand was totalized in a trivial branch");

                let mut p = p_automaton();
                let mut triv = Automaton::true_false(t);
                let _ = op(&mut p, &mut triv);
                assert_eq!(p.fa.q, 2, "operand was totalized in a trivial branch");
            }
        }
    }

    #[test]
    fn not_flips_a_trivial_automatons_truth_value_and_stays_trivial() {
        // `:146-149`.
        for t in [true, false] {
            let n = not(AutomatonDFA::true_false(t));
            assert!(n.automaton().is_true_false_automaton());
            assert_eq!(n.automaton().is_true_automaton(), !t);
        }
        // Involution, for good measure.
        let n = not(not(AutomatonDFA::true_false(true)));
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
}
