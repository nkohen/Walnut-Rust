// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Cross-product automaton construction — the engine behind Walnut's FIVE boolean
//! connectives (`and`/`or`/`xor`/`imply`/`iff`; `not` reaches this path only
//! indirectly and inertly for this crate's scope, see below) and, in later units,
//! `join`/`combine`/eval's per-symbol arithmetic/relational output combination.
//!
//! Ports `Automata/FA/ProductStrategies.java`'s cross-product BFS
//! (`crossProductInternal`/`crossProduct`/`crossProductAndMinimize`) plus its
//! `Automaton`-level alphabet/label-merging bookkeeping (`createBasicAutomaton` and
//! its helpers). Faithful to Java's own layering: the raw BFS
//! ([`cross_product_internal`]) operates on [`Fa`] (matching
//! `crossProductInternal(FA A, FA B, FA AxB, ...)`, which is called with `A.fa`/
//! `B.fa`/`AxB.fa`, not the `Automaton`s themselves); the alphabet/label bookkeeping
//! and the public entry points ([`cross_product`], [`cross_product_and_minimize`])
//! operate on [`Automaton`] (matching `crossProduct(Automaton A, Automaton B, ...)`).
//!
//! # Scoping the operator dispatch (see `docs/BOUNDARY-MAP.md` §2/§3, `CLAUDE.md`)
//!
//! Java's `ProductStrategies.determineOutput` (`ProductStrategies.java:172-190`)
//! dispatches on a `String op` across FOUR unrelated concern families (confirmed by
//! reading every call site, not assumed):
//!
//! 1. **The five boolean connectives this unit targets** — `LogicalOperator.{AND,OR,
//!    XOR,IMPLY,IFF}` — pure functions of the two states' 0/nonzero outputs.
//!    `AutomatonLogicalOps.and`/`totalizeCrossProduct` (used by `or`/`xor`/`imply`/
//!    `iff`) are the only callers of `crossProductAndMinimize` that pass these.
//!    `AutomatonLogicalOps.not` (`AutomatonLogicalOps.java:144-170`) itself never
//!    calls `crossProduct` — it totalizes and flips a SINGLE automaton's output in
//!    place (`totalize()` + `flipOutput()` + `justMinimize()`). An earlier version of
//!    this doc claimed `not` "does NOT go through `crossProduct` AT ALL" — false: `not`
//!    also calls `A.applyAllRepresentations()` (`AutomatonLogicalOps.java:166`), which
//!    calls `AutomatonLogicalOps.and` (`Automaton.java:263`) — but that path is INERT
//!    for base-*k* numeration (`NumberSystem.flagUseAllRepresentations` is only ever
//!    `true` for Fibonacci/Ostrowski/Pell-family bases, `NumberSystem.java:147-150`,
//!    all DROPPED from this port's scope — confirmed by reading it, not assumed). So
//!    `not` is still correctly out of scope for this unit, just not for the reason
//!    originally stated.
//!    **Precondition families 1's callers actually rely on, not yet enforced by
//!    anything in this file:** `AutomatonLogicalOps.and` (`:41-62`) does NOT
//!    totalize either operand first (a missing transition already means "reject"
//!    under intersection); `or`/`xor`/`imply`/`iff` all route through
//!    `totalizeCrossProduct` (`AutomatonLogicalOps.java:112-126`), which totalizes
//!    BOTH operands BEFORE calling `crossProduct` — on a non-total operand, `Or`/
//!    `Xor`/`Imply`/`Iff` compute the wrong language (a missing transition is
//!    silently treated as "this operand contributes nothing" rather than "true" for
//!    the disjunctive/negated connectives). This module's functions are generic and
//!    don't enforce this themselves (matching Java, where the precondition also lives
//!    in the CALLER, not in `ProductStrategies`) — a future `AutomatonLogicalOps` unit
//!    must replicate the `totalizeCrossProduct` call, not just wire `BooleanOp`
//!    straight into `cross_product`/`cross_product_and_minimize`.
//! 2. **`Prover.COMBINE`** (`ProductStrategies.java:185`) — the `combine` command's
//!    per-automaton output-value bookkeeping (`Automaton.combineOutputs`/
//!    `combineIndex`, read via `Automaton.determineCombineOutVal`, itself a
//!    "return `-1` unless `op.equals(Prover.COMBINE)`" sentinel function,
//!    `Automaton.java:72-74`). Sole call site: `AutomatonLogicalOps.combine`
//!    (`AutomatonLogicalOps.java:704`, `ProductStrategies.crossProduct(first, next,
//!    Prover.COMBINE)`).
//! 3. **`Prover.FIRST_OP`**/**`Prover.IF_OTHER_OP`** — `FIRST_OP` backs the `join`
//!    command (`Main/Commands/Join.java:84`: "first non-zero output wins" among
//!    chained DFAOs); `IF_OTHER_OP` backs `Automaton.applyAllRepresentationsWithOutput`
//!    (`Automaton.java:283`, restricting a DFAO to only its number system's
//!    canonical digit representations while preserving the original output).
//! 4. **`RelationalOperator`/`ArithmeticOperator` symbols** (`<`, `+`, etc.) — eval's
//!    per-symbol DFAO-vs-DFAO comparison/arithmetic, dispatched from
//!    `WordAutomaton.compareWordAutomata`/`applyWordOperator`
//!    (`WordAutomaton.java:55`/`96`), entirely unrelated to booleans.
//!
//! Families 2-4 all need real state (`Automaton.combineOutputs`/`combineIndex`, the
//! `Token`/`Operator` AST hierarchy) that doesn't exist in this crate yet and belongs
//! to future units (`wr-logic`'s `Token`/`Expression` port, the `combine`/`join`
//! commands). Porting Java's `String op` dispatch wholesale would drag all of that in
//! just to serve family 1. Instead, [`BooleanOp`] below is a small LOCAL enum
//! covering exactly family 1 — NOT a port of `Main.EvalComputations.Token.
//! LogicalOperator`/`ArithmeticOperator`/`RelationalOperator`/`Prover`'s op-mode
//! constants — and the BFS/entry-point functions
//! ([`cross_product_internal`]/[`cross_product`]) are generic over `combine_output:
//! impl FnMut(i32, i32) -> i32` instead of a `String op`. Families 2-4 (all DFAO/
//! word-automaton output, not plain 0/1 predicates) can plug into
//! [`cross_product_internal`]/[`cross_product`] the same way — but NOT into
//! [`cross_product_and_minimize`]/[`cross_product_and_minimize_dfa`] specifically:
//! this crate's [`crate::minimize::minimize`] (matching Java's `ValmariDFA.minValmari`)
//! partitions on `o[q] != 0` and then discards the real output values, so every real
//! DFAO-output Java caller (`AutomatonLogicalOps.combine`, `Main/Commands/Join.java`,
//! `WordAutomaton.applyWordOperator`) uses `crossProduct` + a DIFFERENT,
//! output-preserving minimize (`minimizeWithOutput`, not yet ported), never
//! `crossProductAndMinimize` — confirmed by reading each call site, not assumed. So a
//! future family-2/3/4 unit needs [`cross_product`] plus its own minimize step, not
//! this file's `_and_minimize` entry points. This is still a genuine, clean
//! separation for family 1: the BFS shape itself has ZERO dependency on which family
//! produced the combining function.
//!
//! Two `ProductStrategiesTest.java` cases have no analog and aren't otherwise
//! mentioned above: `determineOutput_ifOtherOperator` (family 3, out of scope) and
//! `determineOutput_throwsForUnknownOperator` (structurally impossible once `String
//! op` becomes the typed [`BooleanOp`] enum — there is no "unknown" variant to
//! reach).
//!
//! # State-discovery order vs. Java — audited, matches (U0 Part B)
//!
//! Walnut's `details*` golden fixtures assert *intermediate* state counts
//! (`computing cross product:4 states - 16 states`, `Minimizing:…`) and the Phase-3
//! plan's second sign-off decision is to chase **exact parity** on them rather than
//! normalizing them away. That makes this file's BFS state-discovery order part of the
//! observable contract, not just its final language, so U0 audited it against
//! `ProductStrategies.java` line by line. **Result: it already matches; no code change
//! was needed.** Recorded here so the audit isn't repeated, and pinned by
//! `state_discovery_order_matches_javas_hand_traced_bfs` /
//! `nondeterministic_destination_lists_are_visited_in_javas_nesting_order` below.
//!
//! What Java actually iterates (read, not assumed):
//!
//! | Java | ordered? | Rust counterpart |
//! |---|---|---|
//! | `statesList` — `ArrayList<IntIntPair>`, indexed by a monotonically increasing `currentState` cursor (`:42`, `:115`) | yes, FIFO | `states_list: Vec<(usize, usize)>` + `current_state` cursor |
//! | `statesHash` — `Object2IntOpenHashMap` (`:36`, `:108`) | **irrelevant**: only ever `getInt`/`put`, never iterated | `states_hash: HashMap<..>`, likewise lookup-only |
//! | outer symbol loop — `A.getT().getEntriesNfaD(p)` (`:63`) | yes: `Int2ObjectRBTreeMap.int2ObjectEntrySet()`, ascending key | `&a.d[p]`, a `BTreeMap`, ascending key |
//! | inner symbol loop — `B.getT().getEntriesNfaD(q)` (`:65`) | yes, same | `&b.d[q]` |
//! | destination loops — `for destA ... for destB ...` over `IntList`s (`:72-73`) | yes, insertion order, A outer | `for &dest_a in a_dests { for &dest_b in b_dests { … } }` |
//!
//! Two subtleties the audit specifically confirmed rather than assumed:
//! * **Both storage backends give ascending symbol order.** `TransitionsDFA` does not
//!   leak `Int2IntOpenHashMap`'s (unordered) key order: `getEntriesNfaD` goes through
//!   `convertRowToNfa`, which rebuilds an `Int2ObjectRBTreeMap`
//!   (`TransitionsDFA.java:69-79`), and the DFA-specialized BFS uses
//!   `getDfaStateKeySet`, an `IntRBTreeSet` (`:107-109`). So Java is ordered on every
//!   path, and this crate's single `BTreeMap`-backed table matches both.
//! * **The `z == -1` skip happens BEFORE any destination pair is looked up** (`:66-69`),
//!   so a disallowed symbol pair cannot register a state. Same here.
//!
//! The audit also confirms a weaker but useful fact: the *set* of reachable pairs — and
//! hence the state COUNT the `details*` fixtures print — is order-independent anyway.
//! Order only fixes the numbering. Parity is therefore belt-and-braces for those
//! fixtures specifically, but genuinely load-bearing for any future comparison that
//! reads exact state ids (e.g. `.gv`/`.txt` output diffed against Walnut's).
//!
//! # `crossProductInternalDFA` — not ported as a separate function
//!
//! Java keeps a second, DFA-storage-specialized BFS (`crossProductInternalDFA`,
//! `ProductStrategies.java:99-162`) alongside the NFA-shaped one. Per this crate's
//! established precedent (`fa.rs`'s module docs: "a single unified NFA-shaped
//! transition table backs both NFA and DFA use... a storage optimization, not a
//! behavioral quirk"), only [`cross_product_internal`] is ported. When both inputs
//! are deterministic, its result is deterministic too: `all_inputs_of_axb` is
//! injective on `(a_sym, b_sym)` pairs that don't map to `-1` (the joint symbol
//! always embeds A's own full decoded digit tuple as a prefix — see
//! `join_two_inputs_for_cross_product` — so distinct `a_sym` values never collide;
//! and for a fixed `a_sym`, distinct surviving `b_sym` values decode to distinct B
//! tuples and so append distinct remainders). So for a fixed deterministic `(p, q)`,
//! each `(a_sym, b_sym)` pair drawn from `a.d[p]`/`b.d[q]` (rows that already have at
//! most one destination each, since both inputs are deterministic) produces a
//! distinct joint symbol with exactly one destination pair. Java's
//! `crossProductInternalDFA`-specific throw ("Expected DFA-backed transitions for
//! DFA cross product.") is therefore a pure storage-representation assertion with no
//! semantic counterpart here, and [`AutomatonDFA`]'s own stronger, type-enforced
//! determinism invariant (see `automaton.rs`'s doc comment on that type) makes the
//! precondition it guards statically unreachable by the time a caller holds
//! `&AutomatonDFA` arguments anyway — see [`cross_product_and_minimize_dfa`]'s doc
//! comment.

use crate::automaton::{Automaton, AutomatonDFA};
use crate::fa::Fa;
use std::collections::{BTreeMap, HashMap, HashSet};

/// `ProductStrategies.NOT_SAME_INPUT_IN_BOTH` (`ProductStrategies.java:29`).
const NOT_SAME_INPUT_IN_BOTH: i32 = -1;

/// The four boolean connectives that reach `ProductStrategies.crossProduct*` via a
/// `LogicalOperator` op string in Java — see module docs for why this crate defines
/// a small LOCAL enum here instead of porting
/// `Main.EvalComputations.Token.LogicalOperator` (or `ArithmeticOperator`/
/// `RelationalOperator`/`Prover`'s op-mode constants) wholesale. `Not` is
/// deliberately absent: `AutomatonLogicalOps.not` never calls `crossProduct` (see
/// module docs) — negation is a single-automaton output-flip, out of scope for this
/// file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    And,
    Or,
    Xor,
    Imply,
    Iff,
}

impl BooleanOp {
    /// `ProductStrategies.determineOutput`'s `LogicalOperator` branch
    /// (`ProductStrategies.java:180-184`), restricted to the boolean connectives.
    /// `a`/`b` are the two states' RAW outputs (`FA.O`), never assumed to already be
    /// `0`/`1` — matches Java's `!= 0`/`== 0` truthiness checks exactly (not
    /// `== 1`), which matters once DFAO word-output values (`o > 1`) are involved.
    pub fn combine(self, a: i32, b: i32) -> i32 {
        let result = match self {
            BooleanOp::And => a != 0 && b != 0,
            BooleanOp::Or => a != 0 || b != 0,
            BooleanOp::Xor => (a != 0 && b == 0) || (a == 0 && b != 0),
            BooleanOp::Imply => a == 0 || b != 0,
            BooleanOp::Iff => (a == 0 && b == 0) || (a != 0 && b != 0),
        };
        i32::from(result)
    }
}

/// `ProductStrategies.crossProductInternal` (`ProductStrategies.java:33-94`) — the
/// raw cross-product BFS at the `Fa` level (matching Java's own `FA`-level call —
/// see module docs). Builds an NFA-shaped result (Walnut's own doc comment on the
/// Java method: "Output is an NFA (for now)") — see module docs for why the
/// DFA-specialized Java variant isn't ported separately.
///
/// `all_inputs_of_axb[a_sym as usize * b.alphabet_size + b_sym as usize]` gives the
/// corresponding encoded symbol in the product's alphabet, or `-1`
/// (`NOT_SAME_INPUT_IN_BOTH`) if that `(a_sym, b_sym)` pair is disallowed (built by
/// [`compute_all_inputs_of_axb`], mirroring Java's flat `int[]` lookup table).
/// `axb_alphabet_size` becomes the result's [`Fa::alphabet_size`] — Java sets this
/// on `AxB.fa` via `Automaton.determineAlphabetSize()` BEFORE `crossProductInternal`
/// ever runs (inside `createBasicAutomaton`), never inside this method itself; same
/// order is used here (see [`cross_product`]'s call site).
///
/// `combine_output` replaces Java's `String op` + `determineOutput(aP, mQ, op,
/// combineOut)` dispatch — see module docs.
///
/// # Progress logging not ported
///
/// Java logs BFS progress every 100/1000/10000/100000 states
/// (`ProductStrategies.java:43-50`) when `Logging.shouldPrintDetails()`. This crate
/// has no `Logging` module yet (diagnostic output, not behavior) — not ported.
/// `ProductStrategiesTest`'s two `*_logsProgressWhenDetailsEnabled` tests have no
/// faithful analog here; this module's own tests replicate the plain-construction
/// shape they also happened to exercise (a single reachable state pair over an
/// empty alphabet) without the logging aspect.
pub fn cross_product_internal<F>(
    a: &Fa,
    b: &Fa,
    axb_alphabet_size: usize,
    all_inputs_of_axb: &[i32],
    mut combine_output: F,
) -> Fa
where
    F: FnMut(i32, i32) -> i32,
{
    let mut states_list: Vec<(usize, usize)> = vec![(a.q0, b.q0)];
    let mut states_hash: HashMap<(usize, usize), usize> = HashMap::new();
    states_hash.insert((a.q0, b.q0), 0);

    let mut o: Vec<i32> = Vec::new();
    let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();

    let mut current_state = 0usize;
    while current_state < states_list.len() {
        let (p, q) = states_list[current_state];
        o.push(combine_output(a.o[p], b.o[q]));

        let mut state_transitions: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
        for (&a_sym, a_dests) in &a.d[p] {
            let axb_alphabet_base = (a_sym as usize)
                .checked_mul(b.alphabet_size)
                .expect("cross product alphabet-offset overflow (a_sym * b.alphabet_size)");
            for (&b_sym, b_dests) in &b.d[q] {
                let idx = axb_alphabet_base
                    .checked_add(b_sym as usize)
                    .expect("cross product alphabet-offset overflow (+ b_sym)");
                let z = all_inputs_of_axb[idx];
                if z == NOT_SAME_INPUT_IN_BOTH {
                    continue;
                }
                let mut dest = Vec::with_capacity(a_dests.len() * b_dests.len());
                for &dest_a in a_dests {
                    for &dest_b in b_dests {
                        let dest_pair = (dest_a, dest_b);
                        let id = *states_hash.entry(dest_pair).or_insert_with(|| {
                            let id = states_list.len();
                            states_list.push(dest_pair);
                            id
                        });
                        dest.push(id);
                    }
                }
                // See module docs: for the deterministic-input case this never
                // overwrites a previously-inserted `z` from a DIFFERENT (a_sym,
                // b_sym) pair -- but ported as a plain `insert` (last-write-wins),
                // exactly matching Java's `stateTransitions.put(z, dest)`.
                state_transitions.insert(z, dest);
            }
        }
        d.push(state_transitions);
        current_state += 1;
    }

    Fa {
        true_false: None,
        q0: 0,
        q: states_list.len(),
        alphabet_size: axb_alphabet_size,
        o,
        d,
    }
}

/// `ProductStrategies.joinTwoInputsForCrossProduct` (`ProductStrategies.java:339-353`).
/// Returns `None` where Java returns `null` (an incompatible symbol pair -- a shared
/// track where `first`/`second` disagree on the digit value).
fn join_two_inputs_for_cross_product(
    first: &[i32],
    second: &[i32],
    equal_indices: &[i32],
) -> Option<Vec<i32>> {
    let mut result = first.to_vec();
    for (i, &second_i) in second.iter().enumerate() {
        let equal_index_i = equal_indices[i];
        if equal_index_i == NOT_SAME_INPUT_IN_BOTH {
            result.push(second_i);
        } else if first[equal_index_i as usize] != second_i {
            return None;
        }
    }
    Some(result)
}

/// `ProductStrategies.computeSameInputs` (`ProductStrategies.java:273-288`). Indexed
/// by `b`'s track: `result[i] == j` (`j >= 0`) means input `i` of `b` shares a label
/// with input `j` of `a` (and their alphabets must match as SETS —
/// `UtilityMethods.areEqual`, ported here the same way
/// `Automaton::remove_same_inputs` already does it); `NOT_SAME_INPUT_IN_BOTH` means
/// input `i` of `b` has no counterpart in `a`.
///
/// # Panics
///
/// If two same-labeled tracks have different alphabets (`WalnutException` message
/// ported verbatim: "in computing cross product of two automaton, variables with the
/// same label must have the same alphabet").
fn compute_same_inputs(a: &Automaton, b: &Automaton) -> Vec<i32> {
    let mut same_inputs_in_a_and_b = vec![NOT_SAME_INPUT_IN_BOTH; b.label.len()];
    for ((slot, b_label_i), b_alphabet_i) in same_inputs_in_a_and_b
        .iter_mut()
        .zip(b.label.iter())
        .zip(b.alphabet.iter())
    {
        if let Some(j) = a.label.iter().position(|l| l == b_label_i) {
            let set_a: HashSet<i32> = a.alphabet[j].iter().copied().collect();
            let set_b: HashSet<i32> = b_alphabet_i.iter().copied().collect();
            assert_eq!(
                set_a, set_b,
                "in computing cross product of two automaton, variables with the same label must have the same alphabet"
            );
            *slot = j as i32;
        }
    }
    same_inputs_in_a_and_b
}

/// `ProductStrategies.updateAxBFields` (`ProductStrategies.java:290-312`). `msd`
/// plays the role of Java's per-track `List<NumberSystem>` (this crate's existing
/// stand-in — see `automaton::Automaton`'s struct doc comment).
///
/// Java is asymmetric here on the alphabet lists: `a`'s tracks are added BY
/// REFERENCE (`AxB.getA().add(aA.get(i))`, `ProductStrategies.java:295`), `b`'s BY
/// COPY (`new ArrayList<>(bA.get(i))`, `:302`). This port `.clone()`s both — an
/// undeclared (if unobservable) deviation: nothing in Java mutates a track's digit
/// list in place after this point (confirmed by grep), so the two are behaviorally
/// identical, but noting it explicitly rather than leaving it silent.
fn update_axb_fields(
    a: &Automaton,
    b: &Automaton,
    axb: &mut Automaton,
    same_inputs_in_a_and_b: &[i32],
) {
    for i in 0..a.label.len() {
        axb.alphabet.push(a.alphabet[i].clone());
        axb.label.push(a.label[i].clone());
        axb.msd.push(a.msd[i]);
    }
    for (i, &j) in same_inputs_in_a_and_b.iter().enumerate() {
        if j == NOT_SAME_INPUT_IN_BOTH {
            axb.alphabet.push(b.alphabet[i].clone());
            axb.label.push(b.label[i].clone());
            axb.msd.push(b.msd[i]);
        } else {
            let j = j as usize;
            if b.msd[i].is_some() && axb.msd[j].is_none() {
                axb.msd[j] = b.msd[i];
            }
        }
    }
    axb.determine_alphabet_size();
}

/// `ProductStrategies.computeAllInputsOfAxB` (`ProductStrategies.java:317-331`).
/// Requires `axb`'s encoder already be set up against its FINAL alphabet — Java
/// achieves this via `RichAlphabet.encode`'s lazy `if (encoder == null)
/// setupEncoder()` (the first call happens inside this very method, after
/// `updateAxBFields` already finished building `axb`'s alphabet). This crate's
/// `Automaton` always computes its encoder eagerly (at construction, or via
/// explicit [`Automaton::setup_encoder`]) rather than lazily, so the caller must
/// call `setup_encoder` after mutating `axb.alphabet` and before calling this — see
/// [`create_basic_automaton`]'s call site.
///
/// The `aAlphSize * bAlphSize` output-array size is a PLAIN, unchecked `int`
/// multiplication in Java (`ProductStrategies.java:320`) — unlike
/// `RichAlphabet.encode`'s `Math.multiplyExact`, this one silently wraps on Java
/// `int` overflow. This port uses checked arithmetic anyway (consistent with this
/// crate's established convention for every other alphabet-size-derived product,
/// e.g. `Automaton::compute_encoder`/`determine_alphabet_size`) — a deliberate,
/// already-precedented overflow-safety improvement, not a behavioral fix: no
/// alphabet this crate constructs today gets remotely close to `usize`/`i32`
/// overflow, so this can't change any test's observable outcome.
fn compute_all_inputs_of_axb(
    a: &Automaton,
    b: &Automaton,
    axb: &Automaton,
    same_inputs_in_a_and_b: &[i32],
) -> Vec<i32> {
    let a_alphabet_size = a.fa.alphabet_size;
    let b_alphabet_size = b.fa.alphabet_size;
    let total = a_alphabet_size
        .checked_mul(b_alphabet_size)
        .expect("cross product alphabet-size overflow (a.alphabet_size * b.alphabet_size)");
    let mut all_inputs = Vec::with_capacity(total);
    for i in 0..a_alphabet_size {
        let a_decode_i = a.decode(i as i32);
        for j in 0..b_alphabet_size {
            let b_decode_j = b.decode(j as i32);
            let joined =
                join_two_inputs_for_cross_product(&a_decode_i, &b_decode_j, same_inputs_in_a_and_b);
            all_inputs.push(match joined {
                Some(tuple) => axb.encode(&tuple),
                None => NOT_SAME_INPUT_IN_BOTH,
            });
        }
    }
    all_inputs
}

/// `ProductStrategies.createBasicAutomaton` (`ProductStrategies.java:246-267`).
///
/// # Panics
///
/// If either `a` or `b` is a TRUE/FALSE automaton (`:248-251`, U0) — the cross product
/// is undefined for them and every real caller
/// (`AutomatonLogicalOps.and`/`or`/`xor`/`imply`/`iff`) short-circuits on them first.
/// Message ported verbatim from `WalnutException`: "Invalid use of the crossProduct
/// method: the automata for this method cannot be true or false automata." Note this
/// guard must come FIRST, before the `is_bound` check below, because a trivial
/// automaton has zero tracks and zero labels and so passes `is_bound()` vacuously —
/// matching Java's own ordering.
///
/// If either `a` or `b` is unbound (`!is_bound()`) — ports the combined Java check
/// `aLabel == null || bLabel == null || aLabel.size() != aA.size() || bLabel.size()
/// != bA.size()` (an unbound `Automaton` here, per this crate's convention, is the
/// analog of Java's "null label" / "label size doesn't match alphabet size" cases —
/// see `automaton.rs`'s doc comment on `is_bound`/`unlabel`). Message ported
/// verbatim from `WalnutException`: "Invalid use of the crossProduct method: the
/// automata for this method must have labeled inputs."
fn create_basic_automaton(a: &Automaton, b: &Automaton) -> (Automaton, Vec<i32>) {
    assert!(
        !a.fa.is_true_false_automaton() && !b.fa.is_true_false_automaton(),
        "Invalid use of the crossProduct method: the automata for this method cannot be true or false automata."
    );
    assert!(
        a.is_bound() && b.is_bound(),
        "Invalid use of the crossProduct method: the automata for this method must have labeled inputs."
    );
    // `cross_product_and_minimize_dfa`'s injectivity argument for skipping
    // `minimize`'s Result assumes `decode` is a genuine bijection on
    // `0..fa.alphabet_size` for both operands (adversarial-review finding: neither
    // `Automaton::new` nor `AutomatonDFA::from` validates this on construction — see
    // `automaton.rs`'s own "callers are responsible" note). A malformed `Automaton`
    // here (mismatched `alphabet_size`, or a repeated digit within one track) would
    // make two distinct `(a_sym, b_sym)` pairs collide, silently violating
    // `minimize`'s q0-reachability precondition and reaching WB-001
    // (`docs/WALNUT-BUGS.md`) instead of a clean panic. Cheap to check, not a
    // guaranteed catch of every malformed shape (a repeated digit within one
    // track's alphabet Vec isn't checked here), but catches the common case.
    debug_assert_eq!(
        a.fa.alphabet_size,
        a.alphabet.iter().map(|t| t.len()).product::<usize>(),
        "malformed `a`: fa.alphabet_size doesn't match the product of its track sizes"
    );
    debug_assert_eq!(
        b.fa.alphabet_size,
        b.alphabet.iter().map(|t| t.len()).product::<usize>(),
        "malformed `b`: fa.alphabet_size doesn't match the product of its track sizes"
    );

    let same_inputs_in_a_and_b = compute_same_inputs(a, b);

    let mut axb = Automaton::new(
        Fa {
            true_false: None,
            q0: 0,
            q: 0,
            alphabet_size: 1,
            o: Vec::new(),
            d: Vec::new(),
        },
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    update_axb_fields(a, b, &mut axb, &same_inputs_in_a_and_b);
    axb.setup_encoder();

    let all_inputs_of_axb = compute_all_inputs_of_axb(a, b, &axb, &same_inputs_in_a_and_b);
    (axb, all_inputs_of_axb)
}

/// `ProductStrategies.crossProduct` (`ProductStrategies.java:207-218`). Operates at
/// the `Automaton` level (label/alphabet bookkeeping), delegating the raw BFS to
/// [`cross_product_internal`] at the `Fa` level — matching Java's own layering (see
/// module docs). Java's `A.determineCombineOutVal(op)` (only ever non-trivial for
/// `Prover.COMBINE`, see module docs) has no analog: `combine_output` already
/// receives everything it needs directly from the two states' raw outputs.
pub fn cross_product<F>(a: &Automaton, b: &Automaton, mut combine_output: F) -> Automaton
where
    F: FnMut(i32, i32) -> i32,
{
    let (mut axb, all_inputs_of_axb) = create_basic_automaton(a, b);
    let axb_alphabet_size = axb.fa.alphabet_size;
    axb.fa = cross_product_internal(
        &a.fa,
        &b.fa,
        axb_alphabet_size,
        &all_inputs_of_axb,
        &mut combine_output,
    );
    axb
}

/// `ProductStrategies.crossProductAndMinimize(Automaton, Automaton, String)`
/// (`ProductStrategies.java:220-222`).
pub fn cross_product_and_minimize<F>(
    a: &Automaton,
    b: &Automaton,
    combine_output: F,
) -> AutomatonDFA
where
    F: FnMut(i32, i32) -> i32,
{
    cross_product_and_minimize_dfa(&a.as_dfa(), &b.as_dfa(), combine_output)
}

/// `ProductStrategies.crossProductAndMinimize(AutomatonDFA, AutomatonDFA, String)`
/// (`ProductStrategies.java:224-237`).
///
/// # `crossProductInternalDFA`'s storage guard: statically unreachable here
///
/// Java re-checks `A.getT().hasDfaTransitions()`/`B...` at the top of
/// `crossProductInternalDFA` and throws if either input isn't DFA-backed storage.
/// This crate's [`AutomatonDFA`] enforces determinism through the TYPE itself (see
/// its doc comment in `automaton.rs`) — a caller can only ever obtain one through
/// [`AutomatonDFA::from`], which already ran the equivalent check. So by the time
/// this function has `&AutomatonDFA` arguments, Java's runtime guard is already a
/// static invariant; there is nothing left to check here.
///
/// Java's `AxB.fa.justMinimize()` + `AxB.fa.convertNFAtoDFA()` become
/// [`crate::minimize::minimize`] + [`AutomatonDFA::from`]. Note the shape of the
/// checks differs slightly: Java's post-minimize `convertNFAtoDFA` THROWS
/// "Unexpected NFA instead of DFA." if minimize's result is somehow still
/// nondeterministic; this crate's `AutomatonDFA::from` would instead silently
/// REPAIR it (by calling `determinize_and_minimize` again, `automaton.rs`'s
/// `require_dfa_storage`) before its own final assert. Not observably different
/// TODAY — see the injectivity argument below, and `create_basic_automaton`'s
/// `debug_assert`, for why the input to that assert is always already
/// deterministic — but worth naming precisely rather than claiming the checks are
/// identical.
///
/// `minimize` is called with `.expect(...)`, not surfaced as a `Result`: the cross
/// product of two deterministic automata is itself deterministic (see this module's
/// top doc comment for the injectivity argument — which assumes `a`/`b`'s `encode`/
/// `decode` are consistent with their `fa.alphabet_size`, checked by a
/// `debug_assert` in [`create_basic_automaton`]), and its states are exactly the
/// ones the BFS discovers by walking forward from `(a.q0, b.q0)` — i.e. already
/// `q0`-reachable by construction — so neither of `minimize`'s documented
/// preconditions (determinism, `q0`-reachability) can actually fail here.
pub fn cross_product_and_minimize_dfa<F>(
    a: &AutomatonDFA,
    b: &AutomatonDFA,
    mut combine_output: F,
) -> AutomatonDFA
where
    F: FnMut(i32, i32) -> i32,
{
    let mut axb = cross_product(a.automaton(), b.automaton(), &mut combine_output);
    axb.fa = crate::minimize::minimize(&axb.fa).expect(
        "cross product of two deterministic automata is itself deterministic and \
         q0-reachable by construction (see module docs) -- minimize's preconditions \
         always hold here",
    );
    AutomatonDFA::from(axb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equiv;
    use proptest::prelude::*;

    fn one_state_fa(alphabet_size: usize, output: i32) -> Fa {
        Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size,
            o: vec![output],
            d: vec![BTreeMap::new()],
        }
    }

    fn unbound_automaton() -> Automaton {
        // A genuinely UNBOUND automaton needs `label.len() != alphabet.len()` --
        // an all-empty automaton (0 tracks, 0 labels) is vacuously "bound" instead
        // (0 == 0), which does NOT hit `create_basic_automaton`'s guard. One real
        // track with zero labels is the actual mismatch shape.
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 2,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1]],
            Vec::new(),
            vec![None],
        )
    }

    fn bound_automaton(alphabet: Vec<Vec<i32>>, label: Vec<String>) -> Automaton {
        let alphabet_size = alphabet.iter().map(|t| t.len()).product();
        let msd = vec![None; alphabet.len()];
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            alphabet,
            label,
            msd,
        )
    }

    // --- BooleanOp::combine (ProductStrategiesTest.determineOutput_iffOperator,
    // extended to the other four connectives and to non-{0,1} raw outputs) ---

    #[test]
    fn boolean_op_combine_matches_truth_table_on_nonzero_not_just_one() {
        // Uses raw outputs 2 and 5 (not just 0/1) to pin the real `!= 0` truthiness
        // check Java uses (`ProductStrategies.java:180-184`) rather than an `== 1`
        // mistake that would still pass an all-{0,1} test suite.
        let cases: &[(BooleanOp, i32, i32, i32)] = &[
            (BooleanOp::And, 0, 0, 0),
            (BooleanOp::And, 2, 0, 0),
            (BooleanOp::And, 0, 5, 0),
            (BooleanOp::And, 2, 5, 1),
            (BooleanOp::Or, 0, 0, 0),
            (BooleanOp::Or, 2, 0, 1),
            (BooleanOp::Or, 0, 5, 1),
            (BooleanOp::Or, 2, 5, 1),
            (BooleanOp::Xor, 0, 0, 0),
            (BooleanOp::Xor, 2, 0, 1),
            (BooleanOp::Xor, 0, 5, 1),
            (BooleanOp::Xor, 2, 5, 0),
            (BooleanOp::Imply, 0, 0, 1),
            (BooleanOp::Imply, 2, 0, 0),
            (BooleanOp::Imply, 0, 5, 1),
            (BooleanOp::Imply, 2, 5, 1),
            (BooleanOp::Iff, 0, 0, 1),
            (BooleanOp::Iff, 2, 0, 0),
            (BooleanOp::Iff, 0, 5, 0),
            (BooleanOp::Iff, 2, 5, 1),
        ];
        for &(op, a, b, expected) in cases {
            assert_eq!(op.combine(a, b), expected, "{op:?}.combine({a}, {b})");
        }
    }

    // --- crossProductInternal / determineOutput_iffOperator
    // (ProductStrategiesTest.java:57-71, 100-119) ---

    #[test]
    fn cross_product_internal_single_state_pair_reaches_exactly_one_state() {
        // Mirrors crossProductInternal_logsProgressWhenDetailsEnabled's
        // construction (minus the progress-logging aspect, not modeled -- see
        // module docs): a single reachable state pair over an empty alphabet, so
        // the BFS loop body runs exactly once.
        let a = one_state_fa(0, 0);
        let b = one_state_fa(0, 1);
        let axb = cross_product_internal(&a, &b, 0, &[], |x, y| BooleanOp::And.combine(x, y));
        assert_eq!(axb.q, 1);
        assert_eq!(axb.q0, 0);
    }

    #[test]
    fn cross_product_internal_iff_matches_java_test_cases() {
        // determineOutput_iffOperator, case 1: A non-accepting, B accepting ->
        // truthiness differs -> 0.
        let a_false = one_state_fa(0, 0);
        let b_true = one_state_fa(0, 1);
        let axb_differ = cross_product_internal(&a_false, &b_true, 0, &[], |x, y| {
            BooleanOp::Iff.combine(x, y)
        });
        assert_eq!(axb_differ.q, 1);
        assert_eq!(axb_differ.o[0], 0);

        // case 2: both accepting -> truthiness matches -> 1.
        let a_true = one_state_fa(0, 1);
        let axb_match = cross_product_internal(&a_true, &b_true, 0, &[], |x, y| {
            BooleanOp::Iff.combine(x, y)
        });
        assert_eq!(axb_match.q, 1);
        assert_eq!(axb_match.o[0], 1);
    }

    // --- joinTwoInputsForCrossProduct (ProductStrategiesTest.testJoinTwoInputsForCrossProduct) ---

    #[test]
    fn join_two_inputs_for_cross_product_matches_java_test_cases() {
        let equal_indices = [NOT_SAME_INPUT_IN_BOTH, NOT_SAME_INPUT_IN_BOTH, 1];

        assert_eq!(
            join_two_inputs_for_cross_product(&[1, 2, 3], &[-1, 4, 2], &equal_indices),
            Some(vec![1, 2, 3, -1, 4])
        );
        assert_eq!(
            join_two_inputs_for_cross_product(&[1, 2, 3], &[-1, 4, 3], &equal_indices),
            None
        );
        assert_eq!(
            join_two_inputs_for_cross_product(&[1, 3], &[-1, 4, 3], &equal_indices),
            Some(vec![1, 3, -1, 4])
        );
        assert_eq!(
            join_two_inputs_for_cross_product(&[1, 2, 3], &[], &equal_indices),
            Some(vec![1, 2, 3])
        );
    }

    // --- crossProduct's labeling guards (ProductStrategiesTest.
    // crossProduct_throwsWhenLabelIsNull / crossProduct_throwsWhenLabelSizeMismatchesAlphabetSize) ---

    #[test]
    #[should_panic(expected = "the automata for this method must have labeled inputs")]
    fn cross_product_panics_when_a_is_unbound() {
        // This crate's `Automaton` has no `null` label -- an "unbound" automaton
        // (label.len() != alphabet.len()) is this crate's equivalent of Java's
        // null-label case.
        let a = unbound_automaton();
        let b = bound_automaton(vec![vec![0, 1]], vec!["x".to_string()]);
        let _ = cross_product(&a, &b, |x, y| BooleanOp::And.combine(x, y));
    }

    #[test]
    #[should_panic(expected = "the automata for this method must have labeled inputs")]
    fn cross_product_panics_when_b_is_unbound() {
        // Adversarial-review finding (mutation-tested): both pre-existing guard
        // tests put the malformed automaton in the `a` position, so
        // `create_basic_automaton`'s guard (`a.is_bound() && b.is_bound()`) has
        // never been checked for its `b` half -- deleting `&& b.is_bound()` would
        // pass every other test in this module. Java's own guard is symmetric
        // (`aLabel == null || bLabel == null || ...`).
        let a = bound_automaton(vec![vec![0, 1]], vec!["x".to_string()]);
        let b = unbound_automaton();
        let _ = cross_product(&a, &b, |x, y| BooleanOp::And.combine(x, y));
    }

    #[test]
    #[should_panic(expected = "the automata for this method must have labeled inputs")]
    fn cross_product_panics_when_label_size_mismatches_alphabet_size() {
        // Mirrors crossProduct_throwsWhenLabelSizeMismatchesAlphabetSize exactly: a
        // fresh (0-track) automaton, then a label pushed WITHOUT a matching
        // alphabet entry -- label size (1) mismatches alphabet size (0), the
        // opposite-direction mismatch from `cross_product_panics_when_a_is_unbound`.
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 1,
                o: vec![0],
                d: vec![BTreeMap::new()],
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        a.label.push("n".to_string());
        let b = bound_automaton(vec![vec![0, 1]], vec!["x".to_string()]);
        let _ = cross_product(&a, &b, |x, y| BooleanOp::And.combine(x, y));
    }

    #[test]
    #[should_panic(expected = "variables with the same label must have the same alphabet")]
    fn cross_product_panics_when_shared_label_has_different_alphabets() {
        let a = bound_automaton(vec![vec![0, 1]], vec!["x".to_string()]);
        let b = bound_automaton(vec![vec![0, 1, 2]], vec!["x".to_string()]);
        let _ = cross_product(&a, &b, |x, y| BooleanOp::And.combine(x, y));
    }

    // --- updateAxBFields' msd/NS-merge branch (ProductStrategies.java:306-307) ---

    fn one_track_automaton(msd: Option<bool>) -> Automaton {
        Automaton::new(
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
            vec![msd],
        )
    }

    #[test]
    fn update_axb_fields_fills_in_missing_msd_from_b_on_a_shared_track() {
        // A's shared track has msd None; B's corresponding track has msd
        // Some(true) -- AxB should inherit B's non-null value.
        let a = one_track_automaton(None);
        let b = one_track_automaton(Some(true));
        let axb = cross_product(&a, &b, |x, y| BooleanOp::And.combine(x, y));
        assert_eq!(axb.msd, vec![Some(true)]);
    }

    #[test]
    fn update_axb_fields_keeps_as_msd_when_already_set() {
        // Reverse direction: A's msd is already Some -- B's value must NOT
        // overwrite it (`AxB.getNS().get(j) == null` guards the assignment) -- a
        // mutant that made the assignment unconditional would flip this.
        let a = one_track_automaton(Some(false));
        let b = one_track_automaton(Some(true));
        let axb = cross_product(&a, &b, |x, y| BooleanOp::And.combine(x, y));
        assert_eq!(axb.msd, vec![Some(false)]);
    }

    // --- Shared-variable semantics: AxB collapses a shared label to one track and
    // computes the boolean combination in lockstep. ---

    fn even_ones_dfa() -> Fa {
        // Accepts words with an EVEN number of 1s (including the empty word).
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![1]);
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

    fn ends_with_one_dfa() -> Fa {
        // Accepts words whose LAST symbol is 1 (rejects the empty word).
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

    #[test]
    fn cross_product_and_minimize_computes_and_over_a_shared_variable() {
        let alphabet = vec![0, 1];
        let a = Automaton::new(
            even_ones_dfa(),
            vec![alphabet.clone()],
            vec!["x".to_string()],
            vec![None],
        );
        let b = Automaton::new(
            ends_with_one_dfa(),
            vec![alphabet.clone()],
            vec!["x".to_string()],
            vec![None],
        );

        let axb = cross_product_and_minimize(&a, &b, |p, q| BooleanOp::And.combine(p, q));

        assert_eq!(
            axb.automaton().label,
            vec!["x".to_string()],
            "a shared label collapses to ONE track, not two"
        );
        assert_eq!(axb.automaton().alphabet, vec![vec![0, 1]]);

        let expected =
            equiv::product_dfa(&even_ones_dfa(), &ends_with_one_dfa(), |p, q| p && q).unwrap();
        // `minimize` doesn't preserve totality (see the property test below's
        // comment for why) -- this fixture's result happens to stay total today, but
        // totalize defensively so a future fixture tweak can't turn a real language
        // mismatch into a misleading `is_deterministic_and_total` error instead.
        let mut result_fa = axb.automaton().fa.clone();
        result_fa.totalize(0);
        assert_eq!(equiv::language_equivalent(&result_fa, &expected), Ok(true));
    }

    // --- Disjoint-label semantics: AxB keeps both tracks, one per operand, and
    // reads them in lockstep. ---

    proptest! {
        #[test]
        fn cross_product_with_disjoint_labels_computes_the_cartesian_and(
            x_word in prop::collection::vec(0i32..2, 0..5),
            y_word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let len = x_word.len().min(y_word.len());
            let x_word = &x_word[..len];
            let y_word = &y_word[..len];

            let a = Automaton::new(even_ones_dfa(), vec![vec![0, 1]], vec!["x".to_string()], vec![None]);
            let b = Automaton::new(ends_with_one_dfa(), vec![vec![0, 1]], vec!["y".to_string()], vec![None]);
            let axb = cross_product(&a, &b, |p, q| BooleanOp::And.combine(p, q));

            prop_assert_eq!(&axb.label, &vec!["x".to_string(), "y".to_string()]);
            prop_assert_eq!(&axb.alphabet, &vec![vec![0, 1], vec![0, 1]]);

            let combined_word: Vec<i32> = (0..len).map(|i| axb.encode(&[x_word[i], y_word[i]])).collect();

            let expected = a.fa.accepts_word(x_word) && b.fa.accepts_word(y_word);
            prop_assert_eq!(axb.fa.accepts_word(&combined_word), expected);
        }
    }

    // --- Regression coverage for two mutation-tested gaps: the BFS's row stride
    // (`a_sym * b.alphabet_size`, not `a.alphabet_size`) and `combine_output`'s
    // argument order (`combine(a.o[p], b.o[q])`, not swapped) were both unpinned by
    // every other test in this module, since they all used same-size alphabets and
    // symmetric ops (And/Iff). ---

    /// A single-track, 4-symbol-alphabet DFA accepting words whose LAST digit is 3
    /// (rejects the empty word) -- deliberately a DIFFERENT alphabet size from
    /// `even_ones_dfa`/`ends_with_one_dfa`'s 2, for the row-stride test below.
    fn ends_with_three_base4_dfa() -> Fa {
        let mut d0 = BTreeMap::new();
        let mut d1 = BTreeMap::new();
        for sym in 0..4 {
            d0.insert(sym, vec![if sym == 3 { 1 } else { 0 }]);
            d1.insert(sym, vec![if sym == 3 { 1 } else { 0 }]);
        }
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 4,
            o: vec![0, 1],
            d: vec![d0, d1],
        }
    }

    #[test]
    fn cross_product_row_stride_uses_bs_alphabet_size_not_as() {
        // A has a 1-track, 2-symbol alphabet; B has a 1-track, 4-symbol alphabet
        // (DIFFERENT sizes) -- a mutant using `a.alphabet_size` instead of
        // `b.alphabet_size` as the BFS row stride (`axb_alphabet_base`) would
        // misalign every lookup into `all_inputs_of_axb` once `a_sym > 0`. Disjoint
        // labels, so every `(a_sym, b_sym)` pair is a valid product symbol -- checked
        // against independent `accepts_word` ground truth on both operands.
        let a = Automaton::new(
            ends_with_one_dfa(),
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![None],
        );
        let b = Automaton::new(
            ends_with_three_base4_dfa(),
            vec![vec![0, 1, 2, 3]],
            vec!["y".to_string()],
            vec![None],
        );
        assert_eq!(a.fa.alphabet_size, 2);
        assert_eq!(b.fa.alphabet_size, 4);

        let axb = cross_product(&a, &b, |p, q| BooleanOp::And.combine(p, q));
        for &x in &[0, 1] {
            for &y in &[0, 1, 2, 3] {
                let sym = axb.encode(&[x, y]);
                let expected = a.fa.accepts_word(&[x]) && b.fa.accepts_word(&[y]);
                assert_eq!(
                    axb.fa.accepts_word(&[sym]),
                    expected,
                    "x={x} y={y}: single-symbol word mismatch"
                );
            }
        }
        // A longer word, so a stride bug that only misfires past the first BFS
        // transition (not just at the very first lookup) is also caught.
        let word_x = [0, 1, 1];
        let word_y = [2, 3, 0];
        let combined: Vec<i32> = (0..3)
            .map(|i| axb.encode(&[word_x[i], word_y[i]]))
            .collect();
        let expected = a.fa.accepts_word(&word_x) && b.fa.accepts_word(&word_y);
        assert_eq!(axb.fa.accepts_word(&combined), expected);
    }

    #[test]
    fn cross_product_imply_respects_operand_order() {
        // Adversarial-review finding (mutation-tested): every other test drives the
        // BFS with a SYMMETRIC op (And/Iff), so swapping `combine_output(a.o[p],
        // b.o[q])`'s argument order would pass every other test in this module.
        // `Imply` is asymmetric: `L(A -> B) = not(L(A)) union L(B)`, NOT `not(L(B))
        // union L(A)`.
        let a = Automaton::new(
            even_ones_dfa(),
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![None],
        );
        let b = Automaton::new(
            ends_with_one_dfa(),
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![None],
        );
        let axb = cross_product_and_minimize(&a, &b, |p, q| BooleanOp::Imply.combine(p, q));

        for word in [
            vec![],
            vec![0],
            vec![1],
            vec![1, 1],
            vec![0, 1],
            vec![1, 0],
            vec![1, 1, 1],
        ] {
            let expected = !a.fa.accepts_word(&word) || b.fa.accepts_word(&word);
            assert_eq!(
                axb.automaton().fa.accepts_word(&word),
                expected,
                "word={word:?}: A -> B must be !L(A) || L(B), not the reverse"
            );
        }
    }

    // --- Partially-shared, differently-positioned, mixed-radix tracks -- the
    // canonical example from ProductStrategies.java's own doc comment
    // (`:199-203`): A over ["i","j"], B over ["p","q","j"] -> AxB over
    // ["i","j","p","q"]. Adversarial-review finding: every OTHER test in this
    // module either shares a track at index 0 in both operands, or shares no
    // track at all -- `compute_same_inputs`'s `i == j` case (a shared track at
    // matching positions) is never distinguished from a shared track at
    // DIFFERENT, non-zero positions with different radices. ---

    #[test]
    fn cross_product_merges_a_shared_track_at_different_positions_with_mixed_radices() {
        // A: tracks ["i","j"], radix 2 and 3. B: tracks ["p","q","j"], radix 5, 2,
        // 3 -- "j" is shared, at position 1 in A and position 2 in B, and (crucially)
        // the two operands otherwise have DIFFERENT arities and radices, so a
        // wrong-index or wrong-radix bug can't hide behind symmetry.
        let a_alphabets = vec![vec![0, 1], vec![0, 1, 2]];
        let a = Automaton::new(
            {
                let mut d = BTreeMap::new();
                // Accept iff i == 1 (regardless of j).
                for j in 0..3 {
                    let sym = 1 + j * 2; // encoder [1,2]: i + 2*j
                    d.insert(sym, vec![0]);
                }
                Fa {
                    true_false: None,
                    q0: 0,
                    q: 1,
                    alphabet_size: 6,
                    o: vec![1],
                    d: vec![d],
                }
            },
            a_alphabets,
            vec!["i".to_string(), "j".to_string()],
            vec![None, None],
        );
        let b_alphabets = vec![vec![0, 1, 2, 3, 4], vec![0, 1], vec![0, 1, 2]];
        let b = Automaton::new(
            {
                let mut d = BTreeMap::new();
                // Accept iff j == 2 (regardless of p, q) -- encoder [1,5,10]: p + 5*q + 10*j.
                for p in 0..5 {
                    for q in 0..2 {
                        let sym = p + 5 * q + 10 * 2;
                        d.insert(sym, vec![0]);
                    }
                }
                Fa {
                    true_false: None,
                    q0: 0,
                    q: 1,
                    alphabet_size: 30,
                    o: vec![1],
                    d: vec![d],
                }
            },
            b_alphabets,
            vec!["p".to_string(), "q".to_string(), "j".to_string()],
            vec![None, None, None],
        );

        let axb = cross_product(&a, &b, |x, y| BooleanOp::And.combine(x, y));

        assert_eq!(
            axb.label,
            vec![
                "i".to_string(),
                "j".to_string(),
                "p".to_string(),
                "q".to_string()
            ],
            "shared track 'j' collapses to one, in A's position; B's remaining \
             tracks are appended in B's own order"
        );
        assert_eq!(
            axb.alphabet,
            vec![vec![0, 1], vec![0, 1, 2], vec![0, 1, 2, 3, 4], vec![0, 1]]
        );

        // Ground truth: accept iff i==1 AND j==2 (A's predicate uses i,j; B's uses
        // j,p,q; the shared j must be read in LOCKSTEP, not independently).
        for i in 0..2 {
            for j in 0..3 {
                for p in 0..5 {
                    for q in 0..2 {
                        let sym = axb.encode(&[i, j, p, q]);
                        let expected = i32::from(i == 1 && j == 2);
                        assert_eq!(
                            axb.fa.d[0].contains_key(&sym),
                            expected == 1,
                            "i={i} j={j} p={p} q={q}"
                        );
                    }
                }
            }
        }
    }

    // --- Tier-4-style property: cross_product_and_minimize(And) over a shared
    // variable computes exact language intersection, checked via the semantic
    // equivalence oracle rather than a handful of hand-picked words. ---

    fn arb_total_dfa_over_alphabet(
        q_max: usize,
        alphabet_size: usize,
    ) -> impl Strategy<Value = Fa> {
        (1..=q_max).prop_flat_map(move |q| {
            let o_strategy = prop::collection::vec(0i32..=1, q);
            let trans_strategy =
                prop::collection::vec(prop::collection::vec(0usize..q, alphabet_size), q);
            (o_strategy, trans_strategy).prop_map(move |(o, trans)| {
                let d = trans
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .enumerate()
                            .map(|(sym, dest)| (sym as i32, vec![dest]))
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
        #[test]
        fn cross_product_and_minimize_and_computes_intersection_over_a_shared_variable(
            (base, a_fa, b_fa) in (2usize..4).prop_flat_map(|base| {
                (Just(base), arb_total_dfa_over_alphabet(4, base), arb_total_dfa_over_alphabet(4, base))
            })
        ) {
            let alphabet: Vec<i32> = (0..base as i32).collect();
            let a = Automaton::new(a_fa.clone(), vec![alphabet.clone()], vec!["x".to_string()], vec![None]);
            let b = Automaton::new(b_fa.clone(), vec![alphabet.clone()], vec!["x".to_string()], vec![None]);

            let axb = cross_product_and_minimize(&a, &b, |p, q| BooleanOp::And.combine(p, q));

            // `minimize` only prunes states/transitions that are backward
            // co-reachable to acceptance (see `minimize.rs`'s module docs) -- it does
            // NOT preserve totality (e.g. an all-rejecting result collapses to a
            // single state with an EMPTY transition table, still correctly
            // recognizing the empty language via `is_language_empty`/`accepts_word`,
            // but not `is_deterministic_and_total`). Totalize before handing the
            // result to the equivalence oracle, which requires total DFAs on both
            // sides -- this cannot change the recognized language.
            let mut result_fa = axb.automaton().fa.clone();
            result_fa.totalize(0);

            let expected = equiv::product_dfa(&a_fa, &b_fa, |p, q| p && q).unwrap();
            prop_assert_eq!(equiv::language_equivalent(&result_fa, &expected), Ok(true));
        }
    }
    // ------------------------------------------------------------------------
    // State-discovery order (U0 Part B) — see this module's "State-discovery order
    // vs. Java" section for the audit these two tests pin.
    //
    // Technique: give each operand's states DISTINCT output values and combine them
    // with `|p, q| p + q`, so every product state's output is a unique tag naming the
    // `(p, q)` pair it came from. The result's `o` vector is then LITERALLY the
    // discovery order, in order — a much sharper assertion than final-language
    // equivalence, and the thing a `HashMap`-keyed symbol loop or a swapped loop
    // nesting would break.
    // ------------------------------------------------------------------------

    /// `10*(p+1) + (q+1)` — a unique, human-readable tag per `(p, q)` pair.
    fn tag(p: usize, q: usize) -> i32 {
        (10 * (p + 1) + (q + 1)) as i32
    }

    /// Single-track automaton over `{0, 1}` with the given per-state outputs and
    /// explicit transition rows, labeled `name` (distinct labels on the two operands
    /// keep every `(a_sym, b_sym)` pair legal, so the joint symbol never comes back
    /// `-1` and the trace stays purely about ORDER).
    fn traced_operand(name: &str, o: Vec<i32>, rows: Vec<Vec<(i32, Vec<usize>)>>) -> Automaton {
        let d: Vec<BTreeMap<i32, Vec<usize>>> = rows
            .into_iter()
            .map(|row| row.into_iter().collect())
            .collect();
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: o.len(),
                alphabet_size: 2,
                o,
                d,
            },
            vec![vec![0, 1]],
            vec![name.to_string()],
            vec![None],
        )
    }

    #[test]
    fn state_discovery_order_matches_javas_hand_traced_bfs() {
        // A: 2 states, outputs 10/20.   B: 3 states, outputs 1/2/3.
        //   A.d[0] = {0 -> 1, 1 -> 0}      B.d[0] = {0 -> 2, 1 -> 1}
        //   A.d[1] = {0 -> 0, 1 -> 1}      B.d[1] = {0 -> 0, 1 -> 2}
        //                                  B.d[2] = {0 -> 1, 1 -> 0}
        //
        // Hand-tracing `ProductStrategies.crossProductInternal` (`:42-89`) — outer loop
        // over A's symbols ascending, inner over B's symbols ascending, appending each
        // newly-seen destination pair to `statesList`:
        //
        //   cursor 0, (0,0): a0->1 x b0->2 = (1,2) NEW #1
        //                    a0->1 x b1->1 = (1,1) NEW #2
        //                    a1->0 x b0->2 = (0,2) NEW #3
        //                    a1->0 x b1->1 = (0,1) NEW #4
        //   cursor 1, (1,2): ... a1->1 x b1->0 = (1,0) NEW #5
        //   cursors 2-5:     no further new pairs.
        //
        // => discovery order (0,0), (1,2), (1,1), (0,2), (0,1), (1,0).
        //
        // The two most plausible ways to get this wrong both produce a DIFFERENT
        // vector, so this is a real discriminator: swapping the loop nesting (B outer)
        // would discover (1,2), (0,2), (1,1), (0,1); iterating symbols in descending
        // key order would discover (0,2), (0,1), (1,2), (1,1).
        let a = traced_operand(
            "x",
            vec![10, 20],
            vec![
                vec![(0, vec![1]), (1, vec![0])],
                vec![(0, vec![0]), (1, vec![1])],
            ],
        );
        let b = traced_operand(
            "y",
            vec![1, 2, 3],
            vec![
                vec![(0, vec![2]), (1, vec![1])],
                vec![(0, vec![0]), (1, vec![2])],
                vec![(0, vec![1]), (1, vec![0])],
            ],
        );

        let axb = cross_product(&a, &b, |p, q| p + q);

        assert_eq!(
            axb.fa.o,
            vec![
                tag(0, 0),
                tag(1, 2),
                tag(1, 1),
                tag(0, 2),
                tag(0, 1),
                tag(1, 0)
            ],
            "product state i must be the i-th pair Java's BFS discovers"
        );
        assert_eq!(axb.fa.q, 6);
        assert_eq!(axb.fa.q0, 0, "`AxB.setQ0(0)` (`ProductStrategies.java:38`)");

        // The joint alphabet is [A's track "x", B's track "y"] with encoder [1, 2], so
        // the product symbol for (a_sym, b_sym) is `a_sym + 2*b_sym`. Asserting the full
        // transition table pins the numbering a second, independent way (every
        // destination id below is a discovery-order index).
        assert_eq!(axb.alphabet, vec![vec![0, 1], vec![0, 1]]);
        assert_eq!(axb.label, vec!["x".to_string(), "y".to_string()]);
        assert_eq!(axb.fa.alphabet_size, 4);
        let z = |a_sym: i32, b_sym: i32| a_sym + 2 * b_sym;
        let expected: Vec<Vec<(i32, Vec<usize>)>> = vec![
            // 0 = (0,0)
            vec![
                (z(0, 0), vec![1]),
                (z(0, 1), vec![2]),
                (z(1, 0), vec![3]),
                (z(1, 1), vec![4]),
            ],
            // 1 = (1,2)
            vec![
                (z(0, 0), vec![4]),
                (z(0, 1), vec![0]),
                (z(1, 0), vec![2]),
                (z(1, 1), vec![5]),
            ],
            // 2 = (1,1)
            vec![
                (z(0, 0), vec![0]),
                (z(0, 1), vec![3]),
                (z(1, 0), vec![5]),
                (z(1, 1), vec![1]),
            ],
            // 3 = (0,2)
            vec![
                (z(0, 0), vec![2]),
                (z(0, 1), vec![5]),
                (z(1, 0), vec![4]),
                (z(1, 1), vec![0]),
            ],
            // 4 = (0,1)
            vec![
                (z(0, 0), vec![5]),
                (z(0, 1), vec![1]),
                (z(1, 0), vec![0]),
                (z(1, 1), vec![3]),
            ],
            // 5 = (1,0)
            vec![
                (z(0, 0), vec![3]),
                (z(0, 1), vec![4]),
                (z(1, 0), vec![1]),
                (z(1, 1), vec![2]),
            ],
        ];
        let expected: Vec<BTreeMap<i32, Vec<usize>>> = expected
            .into_iter()
            .map(|row| row.into_iter().collect())
            .collect();
        assert_eq!(axb.fa.d, expected);
    }

    #[test]
    fn nondeterministic_destination_lists_are_visited_in_javas_nesting_order() {
        // The other half of the discovery order, invisible to the DFA trace above:
        // `for (int destA : entryA.getValue()) for (int destB : entryB.getValue())`
        // (`ProductStrategies.java:72-73`) — A's destination list is the OUTER loop, and
        // each list is visited in its own insertion order, NOT sorted.
        //
        // A.d[0] = {0 -> [0, 1]}, B.d[0] = {0 -> [1, 0]} (note B's list is deliberately
        // NOT ascending). Java therefore discovers, from (0,0):
        //   destA=0 x destB=1 -> (0,1) NEW #1
        //   destA=0 x destB=0 -> (0,0) already #0
        //   destA=1 x destB=1 -> (1,1) NEW #2
        //   destA=1 x destB=0 -> (1,0) NEW #3
        // and writes the destination list [1, 0, 2, 3] in exactly that order.
        let a = traced_operand("x", vec![10, 20], vec![vec![(0, vec![0, 1])], vec![]]);
        let b = traced_operand("y", vec![1, 2], vec![vec![(0, vec![1, 0])], vec![]]);

        let axb = cross_product(&a, &b, |p, q| p + q);

        assert_eq!(axb.fa.o, vec![tag(0, 0), tag(0, 1), tag(1, 1), tag(1, 0)]);
        assert_eq!(
            axb.fa.d[0].get(&0),
            Some(&vec![1, 0, 2, 3]),
            "destination-list order is A-outer/B-inner, each list in insertion order"
        );
        for q in 1..4 {
            assert!(
                axb.fa.d[q].is_empty(),
                "states {q} reached only dead ends in one operand or the other"
            );
        }
    }

    // ------------------------------------------------------------------------
    // The TRUE/FALSE guard (U0) — `ProductStrategies.createBasicAutomaton:248-251`.
    // ------------------------------------------------------------------------

    #[test]
    #[should_panic(
        expected = "Invalid use of the crossProduct method: the automata for this method cannot be true or false automata."
    )]
    fn cross_product_rejects_a_trivial_first_operand() {
        let b = traced_operand("y", vec![1, 2], vec![vec![(0, vec![1])], vec![]]);
        let _ = cross_product(&Automaton::true_false(true), &b, |p, q| p + q);
    }

    #[test]
    #[should_panic(
        expected = "Invalid use of the crossProduct method: the automata for this method cannot be true or false automata."
    )]
    fn cross_product_rejects_a_trivial_second_operand() {
        // Must panic with the TRIVIAL message, not the "labeled inputs" one: a trivial
        // automaton is vacuously `is_bound()` (0 labels, 0 tracks), so ordering the two
        // guards the other way round would report the wrong error here.
        let a = traced_operand("x", vec![10, 20], vec![vec![(0, vec![1])], vec![]]);
        let _ = cross_product(&a, &Automaton::true_false(false), |p, q| p + q);
    }
}
