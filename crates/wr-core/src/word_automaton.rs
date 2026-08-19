// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! DFAO ("word automaton") operations.
//!
//! Ports `Automata/WordAutomaton.java` (235 LOC) — static helpers on automata that
//! carry per-state OUTPUT values (`FA.O`), not just accept/reject: comparing a DFAO's
//! per-state output against a constant or against another DFAO's per-state outputs
//! (relational), combining two DFAOs' outputs arithmetically, reversing a DFAO
//! (Theorem 4.3.3, Allouche & Shallit), and the general "un/re-combine" machinery a
//! DFAO-aware minimize is built from.
//!
//! Landed in Phase 3a's U6, per `docs/BOUNDARY-MAP.md` §3a's coupling-avoidance
//! recommendation: the two per-operator-kind semantics this file needs
//! ([`crate::numsys::RelationalOp::compare`]/[`crate::numsys::ArithmeticOp::arith`])
//! were added directly to the already-hoisted `RelationalOp`/`ArithmeticOp` enums in
//! `numsys.rs` rather than porting `Main.EvalComputations.Token.{RelationalOperator,
//! ArithmeticOperator}` wholesale (that full `Token`/`Operator`/`Expression` hierarchy
//! is `wr-logic`'s later units, U2/U9/U10). Cross-automaton combination reuses
//! `crate::product`'s existing generic-over-`combine_output` cross-product BFS
//! (`docs/BOUNDARY-MAP.md` §3a again, and `product.rs`'s own module docs, which name
//! `WordAutomaton.compareWordAutomata`/`applyWordOperator` directly as "family 4" of
//! its op-dispatch survey) — no new product/BFS machinery is added here.
//!
//! # `crossProduct` + `minimizeWithOutput`, not `crossProductAndMinimize`, for arithmetic
//!
//! `product.rs`'s own module docs already worked out which entry point each family
//! needs: [`crate::product::cross_product_and_minimize`] partitions on `o[q] != 0` and
//! discards the real output value, so it is only correct for a combining function whose
//! result is always `0`/`1` (a genuine boolean predicate) — exactly
//! [`compare_word_automata`]'s case ([`crate::numsys::RelationalOp::compare`] is
//! boolean by construction). [`apply_word_operator`]'s combining function
//! ([`crate::numsys::ArithmeticOp::arith`]) can return ANY `i32`, so it must go through
//! the output-preserving pair [`crate::product::cross_product`] +
//! [`minimize_with_output`] (this file's own DFAO-aware minimize, ported below) —
//! matching Java's `WordAutomaton.applyWordOperator` exactly (`crossProduct` then
//! `minimizeWithOutput`, never `crossProductAndMinimize`).
//!
//! # A new hard dependency this unit surfaced: `AutomatonLogicalOps.combine`
//!
//! `minimizeWithOutput` (`WordAutomaton.java:216-229`) calls `AutomatonLogicalOps.
//! combine` directly — not yet ported anywhere in this crate before this unit (checked:
//! no `combine` existed in `logicalops.rs`). Added there as [`crate::logicalops::combine`]
//! as part of this unit (see its own doc comment for why it lives in `logicalops.rs`,
//! `combine`'s real Java home, rather than here).
//!
//! # Fallibility: `RelationalOp::compare` can't fail, `ArithmeticOp::arith` can
//!
//! [`crate::numsys::RelationalOp::compare`] is a pure comparison — infallible — so
//! [`compare_word_automaton`]/[`compare_word_automata`] are infallible too.
//! [`crate::numsys::ArithmeticOp::arith`] can fail (division by zero, `i32` overflow —
//! see its own doc comment), which this file surfaces two different ways depending on
//! whether [`crate::product::cross_product`]'s already-shipped, infallible
//! `FnMut(i32, i32) -> i32` closure signature is in the way:
//! - [`apply_word_arith_operator`] (single automaton vs. a constant) never goes through
//!   `cross_product` — it is a plain per-state loop — so it threads the `Result`
//!   through cleanly and returns `Result<(), NumSysError>`, same as Java's own
//!   `WalnutException` would propagate to its (eventual, `wr-cli`-side) caller.
//! - [`apply_word_operator`] (two automata) MUST supply `cross_product` an infallible
//!   closure. This is not a compromise: Java's own `ProductStrategies.
//!   crossProductInternal` calls `ArithmeticOperator.arith` with no `try`/`catch`
//!   either — an exception there unwinds out through the whole BFS uncaught, exactly
//!   like a Rust panic unwinding out through `cross_product`. So the closure here calls
//!   `.unwrap_or_else(|e| panic!("{e}"))`, which is the faithful match, not a weaker
//!   one — see [`apply_word_operator`]'s own doc comment.

use crate::automaton::Automaton;
use crate::logicalops;
use crate::numsys::{ArithmeticOp, NumSysError, RelationalOp};
use crate::product;
use crate::util::remove_duplicates;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// `WordAutomaton.compareWordAutomaton(Automaton, int, RelationalOperator.Ops)`
/// (`WordAutomaton.java:28-42`) — rewrites `wordA` IN PLACE from a DFAO into a plain
/// (0/1-output) automaton accepting `x` iff `wordA[x] op o`. "As of now, this is *not*
/// a word automaton" (Java's own comment, `:37`) — the final
/// [`Automaton::determinize_and_minimize`] call collapses states that became
/// equivalent once output was restricted to `0`/`1`.
pub fn compare_word_automaton(word_a: &mut Automaton, o: i32, op: RelationalOp) {
    compare_word_automaton_with_ctx(word_a, o, op, None, &mut crate::logging::Logging::new());
}

/// [`compare_word_automaton`] with an explicit
/// [`crate::determinize::DeterminizeContext`] — see
/// [`Automaton::determinize_and_minimize_with_ctx`] for the contract. The closing call is
/// the CONDITIONAL no-arg `determinizeAndMinimize()`, so a comparison whose rewritten
/// table is still deterministic (the usual case — a DFAO's outputs were just replaced by
/// 0/1) consumes no automata index. Walnut's `details637.txt` shows exactly that: its
/// `comparing (=) against 1:` blocks carry a `Minimizing:` line and no
/// `Determinizing [#n, …]` line.
pub fn compare_word_automaton_with_ctx(
    word_a: &mut Automaton,
    o: i32,
    op: RelationalOp,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) {
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} ({}) against {}:{} states",
        crate::logging::COMPARING,
        op.symbol(),
        o,
        word_a.fa.q
    ));
    logging.indent();
    for p in 0..word_a.fa.q {
        let out_p = word_a.fa.o[p];
        word_a.fa.set_output_if_equal(p, op.compare(out_p, o));
    }
    // As of now, this is *not* a word automaton.
    word_a.determinize_and_minimize_with_ctx(ctx, logging);
    logging.dedent();
    logging.log_message(&format!(
        "{} ({}) against {}:{} states - {}ms",
        crate::logging::COMPARED,
        op.symbol(),
        o,
        word_a.fa.q,
        time_before.elapsed().as_millis()
    ));
}

/// `WordAutomaton.compareWordAutomata(Automaton, Automaton, String)`
/// (`WordAutomaton.java:50-61`) — returns a DFA accepting `x` iff
/// `wordA[x] op wordB[x]` lexicographically. See this module's docs for why
/// [`crate::product::cross_product_and_minimize`] (not `cross_product` +
/// [`minimize_with_output`]) is the right pair here: the combining function
/// ([`RelationalOp::compare`]) is always `0`/`1`.
pub fn compare_word_automata(
    word_a: &Automaton,
    word_b: &Automaton,
    op: RelationalOp,
    logging: &mut crate::logging::Logging,
) -> Automaton {
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} ({}):{} states - {} states",
        crate::logging::COMPARING,
        op.symbol(),
        word_a.fa.q,
        word_b.fa.q
    ));
    logging.indent();
    let m = product::cross_product_and_minimize(
        word_a,
        word_b,
        |a_out, b_out| i32::from(op.compare(a_out, b_out)),
        logging,
    );
    logging.dedent();
    logging.log_message(&format!(
        "{} ({}):{} states - {}ms",
        crate::logging::COMPARED,
        op.symbol(),
        m.automaton().fa.q,
        time_before.elapsed().as_millis()
    ));
    m.into_automaton()
}

/// `WordAutomaton.applyWordArithOperator(Automaton, int, ArithmeticOperator.Ops,
/// boolean)` (`WordAutomaton.java:69-84`) — rewrites `wordA`'s per-state output IN
/// PLACE to `o op wordA[state]` (or `wordA[state] op o` if `reverse`), then
/// re-minimizes as a DFAO via [`minimize_self_with_output`]. Never goes through
/// [`crate::product`] (a single-automaton, per-state loop), so `Result` threads
/// through cleanly — see this module's docs on fallibility.
pub fn apply_word_arith_operator(
    word_a: &mut Automaton,
    o: i32,
    op: ArithmeticOp,
    reverse: bool,
) -> Result<(), NumSysError> {
    apply_word_arith_operator_with_ctx(
        word_a,
        o,
        op,
        reverse,
        None,
        &mut crate::logging::Logging::new(),
    )
}

/// [`apply_word_arith_operator`] with an explicit
/// [`crate::determinize::DeterminizeContext`] — see
/// [`Automaton::determinize_and_minimize_with_ctx`] for the contract. Reaches the
/// dispatcher only through [`minimize_self_with_output`]'s per-output-value
/// sub-automata, each of whose `determinizeAndMinimize()` calls is itself conditional.
pub fn apply_word_arith_operator_with_ctx(
    word_a: &mut Automaton,
    o: i32,
    op: ArithmeticOp,
    reverse: bool,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) -> Result<(), NumSysError> {
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} operator ({}):{} states",
        crate::logging::APPLYING,
        op.symbol(),
        word_a.fa.q
    ));
    logging.indent();
    for p in 0..word_a.fa.q {
        let this_p = word_a.fa.o[p];
        word_a.fa.o[p] = if reverse {
            op.arith(this_p, o)?
        } else {
            op.arith(o, this_p)?
        };
    }
    minimize_self_with_output_with_ctx(word_a, ctx, logging);
    logging.dedent();
    logging.log_message(&format!(
        "{} operator ({}):{} states - {}ms",
        crate::logging::APPLIED,
        op.symbol(),
        word_a.fa.q,
        time_before.elapsed().as_millis()
    ));
    Ok(())
}

/// `WordAutomaton.applyWordOperator(Automaton, Automaton, String)`
/// (`WordAutomaton.java:91-103`) — returns a DFAO outputting `wordA[x] op wordB[x]` on
/// input `x`. Panics if [`ArithmeticOp::arith`] fails (division by zero, or the `i32`
/// overflow case) — see this module's docs on fallibility for why this is a faithful
/// port of Java's own uncaught-exception-through-the-BFS behavior, not a shortcut.
pub fn apply_word_operator(word_a: &Automaton, word_b: &Automaton, op: ArithmeticOp) -> Automaton {
    apply_word_operator_with_ctx(
        word_a,
        word_b,
        op,
        None,
        &mut crate::logging::Logging::new(),
    )
}

/// [`apply_word_operator`] with an explicit
/// [`crate::determinize::DeterminizeContext`] — see
/// [`Automaton::determinize_and_minimize_with_ctx`] for the contract.
pub fn apply_word_operator_with_ctx(
    word_a: &Automaton,
    word_b: &Automaton,
    op: ArithmeticOp,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) -> Automaton {
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{} operator ({}):{} states - {} states",
        crate::logging::APPLYING,
        op.symbol(),
        word_a.fa.q,
        word_b.fa.q
    ));
    logging.indent();
    let n = product::cross_product(
        word_a,
        word_b,
        |a_out, b_out| {
            op.arith(a_out, b_out)
                .unwrap_or_else(|e| panic!("WordAutomaton::apply_word_operator: {e}"))
        },
        logging,
    );
    let n = minimize_with_output_with_ctx(&n, ctx, logging);
    logging.dedent();
    logging.log_message(&format!(
        "{} operator ({}):{} states - {}ms",
        crate::logging::APPLIED,
        op.symbol(),
        n.fa.q,
        time_before.elapsed().as_millis()
    ));
    n
}

/// `WordAutomaton.reverseWithOutput(Automaton, boolean)` (`WordAutomaton.java:109-192`)
/// — reverses a DFAO in place (Theorem 4.3.3, Allouche & Shallit): the returned
/// automaton's output on `x` equals the input's output on `reverse(x)`. The result is
/// deterministic even if the input was an NFA (it never is, in practice — see the
/// precondition note below).
///
/// # WB-016 (`docs/WALNUT-BUGS.md`) — `q0` is never updated after the rebuild
///
/// Ported **verbatim**, including a genuine Walnut bug found while porting this exact
/// method: after the subset-construction rebuild below (`Fa::set_fields`), the new
/// state `0` is ALWAYS the correct new initial state (it's the BFS root, pushed before
/// the loop starts, and `new_o`/`new_d` are built in BFS order) — but neither Java's
/// `FA.setFields` nor this method (in either language) ever assigns `q0`, so it keeps
/// its STALE pre-reversal value. This is usually masked (`canonicalize` always leaves
/// `q0 == 0`, and most DFAOs reach this method via a canonicalizing pipeline where that
/// already holds), but is a real, empirically confirmed silent-wrong-answer bug
/// whenever the input's `q0` is not already `0` — e.g. a hand-authored `.txt`
/// word-automaton file, a fully supported Walnut input path. See WB-016 for the full
/// trigger and verification against the real `walnut-java` CLI. NOT fixed here per
/// `CLAUDE.md`'s mechanical-port rule — `q0` is left exactly as stale as Java leaves it.
///
/// # The deterministic-and-total precondition, checked only at `q0` — also ported verbatim
///
/// Like Java, this only checks that `word_a.fa.d[q0]` has `alphabet_size` entries
/// (`WalnutException`'s "Automaton should be deterministic!", ported as a `panic!` with
/// the same text — this is a documented, caller-enforced precondition, matching
/// `PORTING.md`'s panic-for-precondition-violation convention, not a
/// `Result`-worthy user error) — NOT that every state does. This is safe in practice
/// only because a genuine DFAO is always fully deterministic-and-total (every state has
/// exactly `alphabet_size` outgoing transitions), so checking any one state's key count
/// against `alphabet_size` is equivalent to checking totality everywhere. If that
/// invariant is ever violated on a specific NON-`q0` state, both engines silently do
/// the wrong thing (index out of a destination list Java would instead NPE on, or a
/// panic here) rather than catching it — a coding shortcut, not a bug in itself (no
/// concrete reachable counterexample found; every real word automaton is total), so not
/// logged as a separate `WALNUT-BUGS.md` entry.
pub fn reverse_with_output(word_a: &mut Automaton, reverse_msd: bool) {
    reverse_with_output_with_ctx(
        word_a,
        reverse_msd,
        None,
        &mut crate::logging::Logging::new(),
    );
}

/// [`reverse_with_output`] with an explicit
/// [`crate::determinize::DeterminizeContext`] and the caller's real
/// [`crate::logging::Logging`] — Java's own `REVERSING`/`REVERSED` pair +
/// `indent()`/`dedent()` bracket (`WordAutomaton.java:114-115`, `:189-191`) around the
/// WHOLE method, including `minimizeSelfWithOutput`'s own subtree (which gets NO
/// separate bracket here — it is already inside this one). Note the message text has a
/// SPACE after the colon (`REVERSING + ": "`), unlike `AutomatonLogicalOps.reverse`'s
/// `REVERSING + ":"` (no space) — both ported verbatim, not unified.
pub fn reverse_with_output_with_ctx(
    word_a: &mut Automaton,
    reverse_msd: bool,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) {
    if word_a.fa.is_true_false_automaton() {
        return;
    }
    let time_before = std::time::Instant::now();
    logging.log_message(&format!(
        "{}: {} states",
        crate::logging::REVERSING,
        word_a.fa.q
    ));
    logging.indent();

    let added_dead_state = word_a.fa.add_distinguished_dead_state();

    let mut min_output = 0;
    if added_dead_state {
        min_output = word_a.fa.determine_min_output();
    }

    let q = word_a.fa.q;
    let q0 = word_a.fa.q0;
    let alphabet_size = word_a.fa.alphabet_size;

    // need to define states, an initial state, transitions, and outputs.
    let new_init_state: BTreeMap<usize, i32> = (0..q).map(|i| (i, word_a.fa.o[i])).collect();

    let mut new_o: Vec<i32> = Vec::new();
    let mut new_d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();

    let mut new_states: Vec<BTreeMap<usize, i32>> = vec![new_init_state.clone()];
    let mut new_states_hash: HashMap<BTreeMap<usize, i32>, usize> = HashMap::new();
    new_states_hash.insert(new_init_state.clone(), 0);

    let mut new_states_queue: VecDeque<BTreeMap<usize, i32>> = VecDeque::new();
    new_states_queue.push_back(new_init_state);

    while let Some(curr_state) = new_states_queue.pop_front() {
        // set up the output of this state to be g(q0), where g = currState.
        new_o.push(curr_state[&q0]);

        new_d.push(BTreeMap::new());

        if word_a.fa.d[q0].len() != alphabet_size {
            panic!("Automaton should be deterministic!");
        }
        let symbols: Vec<i32> = word_a.fa.d[q0].keys().copied().collect();
        for l in symbols {
            let mut to_state: BTreeMap<usize, i32> = BTreeMap::new();

            for i in 0..q {
                let dest = word_a.fa.d[i][&l][0];
                to_state.insert(i, curr_state[&dest]);
            }

            let to_idx = if let Some(&idx) = new_states_hash.get(&to_state) {
                idx
            } else {
                new_states.push(to_state.clone());
                new_states_queue.push_back(to_state.clone());
                let idx = new_states.len() - 1;
                new_states_hash.insert(to_state, idx);
                idx
            };

            // set up the transition.
            new_d.last_mut().unwrap().insert(l, vec![to_idx]);
        }
    }

    word_a.fa.set_fields(new_states.len(), new_o, new_d);
    // See WB-016 above: `q0` is deliberately NOT set here, matching Java exactly.

    if reverse_msd {
        logicalops::flip_ns(word_a);
    }

    minimize_self_with_output_with_ctx(word_a, ctx, logging);

    if added_dead_state {
        // note: word_a is deterministic
        logicalops::remove_states_with_output_rebuild(&mut word_a.fa, min_output);
        word_a.force_canonize();
    }

    logging.dedent();
    logging.log_message(&format!(
        "{}: {} states - {}ms",
        crate::logging::REVERSED,
        word_a.fa.q,
        time_before.elapsed().as_millis()
    ));
}

/// `WordAutomaton.uncombine(Automaton, List<Integer>)` (`WordAutomaton.java:200-209`) —
/// splits a DFAO into one plain (accept/reject) automaton per requested output value,
/// each accepting exactly the states where `wordA`'s output equals that value.
pub fn uncombine(word_a: &Automaton, outputs: &[i32]) -> Vec<Automaton> {
    outputs
        .iter()
        .map(|&output| {
            let mut m = word_a.clone();
            // `M` is *not* a word automaton.
            m.fa.restrict_output_to(output);
            m
        })
        .collect()
}

/// `WordAutomaton.minimizeWithOutput(Automaton)` (`WordAutomaton.java:216-229`) — a
/// minimal DFAO recognizing the same (output-annotated) language as `word_a`, built by
/// uncombining into plain automata (one per distinct output value), minimizing each
/// (they are NOT word automata, so ordinary [`Automaton::determinize_and_minimize`]
/// applies), and recombining via [`crate::logicalops::combine`]. If the uncombined
/// automata are each minimal, the recombined DFAO is minimal too.
pub fn minimize_with_output(word_a: &Automaton) -> Automaton {
    minimize_with_output_with_ctx(word_a, None, &mut crate::logging::Logging::new())
}

/// [`minimize_with_output`] with an explicit
/// [`crate::determinize::DeterminizeContext`] — see
/// [`Automaton::determinize_and_minimize_with_ctx`] for the contract. Note the loop:
/// Java reads the metacommands once per `determinizeAndMinimize()` call, so this can
/// consume ONE AUTOMATA INDEX PER DISTINCT OUTPUT VALUE (each conditional on that
/// sub-automaton actually being nondeterministic), not one per call to this function.
pub fn minimize_with_output_with_ctx(
    word_a: &Automaton,
    mut ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) -> Automaton {
    let mut outputs = word_a.fa.o.clone();
    remove_duplicates(&mut outputs);
    let mut subautomata = uncombine(word_a, &outputs);
    for subautomaton in subautomata.iter_mut() {
        // These are *not* word automata.
        subautomaton.determinize_and_minimize_with_ctx(ctx.as_deref_mut(), logging);
    }
    let mut n = subautomata.remove(0);
    let label = n.label.clone(); // We keep the old labels, since they are replaced in combine.
    n = logicalops::combine(&n, subautomata, &outputs, logging);
    n.label = label;
    n
}

/// `WordAutomaton.minimizeSelfWithOutput(Automaton)` (`WordAutomaton.java:231-234`).
/// `Automaton.copy(N)`'s Java role (a full field-by-field overwrite) is exactly Rust
/// move-assignment here — see `automaton.rs`'s own module docs on why this crate never
/// needs to port Java's manual `clone`/`copy` boilerplate. **Except one field**: `copy`
/// copies FIELDS only and never reassigns `this`'s own runtime class, but
/// [`crate::automaton::Automaton::dfa_typed`] is not a Java field this crate's plain
/// move-assignment naturally preserves — `n` here comes from `minimize_with_output_with_ctx`
/// → `logicalops::combine`, whose own result is plain-typed whenever the DFAO has ≥2
/// distinct output values (`combine`'s own docs). Explicitly carried across, matching
/// every other `copy(...)`-shaped site — see `Automaton::dfa_typed`'s docs. Currently
/// latent (every real call site here feeds a value that is plain-typed in Java too, so
/// this line is a no-op today), fixed defensively per that field's own warning that this
/// exact bug class recurs at every whole-struct-reassignment site.
pub fn minimize_self_with_output(word_a: &mut Automaton) {
    minimize_self_with_output_with_ctx(word_a, None, &mut crate::logging::Logging::new());
}

/// [`minimize_self_with_output`] with an explicit
/// [`crate::determinize::DeterminizeContext`] — see
/// [`minimize_with_output_with_ctx`].
pub fn minimize_self_with_output_with_ctx(
    word_a: &mut Automaton,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    logging: &mut crate::logging::Logging,
) {
    let mut n = minimize_with_output_with_ctx(word_a, ctx, logging);
    n.dfa_typed = word_a.dfa_typed;
    *word_a = n;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fa::Fa;
    use std::collections::BTreeMap as Map;

    /// A deterministic, total, single-track `msd_2`-shaped word automaton with the
    /// given per-state outputs and transition table (`d[state][symbol] = dest`).
    fn word_automaton(q0: usize, outputs: &[i32], d: &[[usize; 2]]) -> Automaton {
        let q = outputs.len();
        let mut table: Vec<Map<i32, Vec<usize>>> = Vec::with_capacity(q);
        for row in d {
            let mut m = Map::new();
            m.insert(0, vec![row[0]]);
            m.insert(1, vec![row[1]]);
            table.push(m);
        }
        let fa = Fa {
            q0,
            q,
            alphabet_size: 2,
            o: outputs.to_vec(),
            d: table,
            true_false: None,
        };
        Automaton::new(
            fa,
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    // -------------------------------------------------------------------
    // compare_word_automaton
    // -------------------------------------------------------------------
    //
    // These use single-state automata deliberately: `compare_word_automaton` ends in
    // an `Automaton::determinize_and_minimize` call, and a *disconnected* multi-state
    // fixture (e.g. two states that each only self-loop, with q0 unable to reach the
    // other) is exactly WB-001's trigger shape (`docs/WALNUT-BUGS.md`) — a q0 that
    // can't reach an elsewhere-accepting state gets silently misclassified by the
    // ported Valmari minimizer. Single-state automata sidestep that unrelated,
    // already-documented quirk entirely, so these tests pin `compare_word_automaton`'s
    // own behavior instead of accidentally re-testing WB-001.

    #[test]
    fn compare_word_automaton_equal_constant() {
        let mut a = word_automaton(0, &[5], &[[0, 0]]);
        compare_word_automaton(&mut a, 5, RelationalOp::Equal);
        assert!(a.fa.is_accepting(a.fa.q0));
    }

    #[test]
    fn compare_word_automaton_not_equal_rejects_when_q0_matches() {
        let mut a = word_automaton(0, &[5], &[[0, 0]]);
        compare_word_automaton(&mut a, 5, RelationalOp::NotEqual);
        assert!(!a.fa.is_accepting(a.fa.q0));
    }

    #[test]
    fn compare_word_automaton_less_than() {
        let mut a = word_automaton(0, &[3], &[[0, 0]]);
        compare_word_automaton(&mut a, 5, RelationalOp::LessThan);
        assert!(a.fa.is_accepting(a.fa.q0)); // 3 < 5
    }

    // -------------------------------------------------------------------
    // compare_word_automata
    // -------------------------------------------------------------------

    #[test]
    fn compare_word_automata_less_than_two_single_state_automata() {
        let a = word_automaton(0, &[3], &[[0, 0]]);
        let b = word_automaton(0, &[5], &[[0, 0]]);
        let m = compare_word_automata(
            &a,
            &b,
            RelationalOp::LessThan,
            &mut crate::logging::Logging::new(),
        );
        assert!(m.fa.accepts_word(&[]));
    }

    #[test]
    fn compare_word_automata_equal_is_false_for_distinct_constants() {
        let a = word_automaton(0, &[3], &[[0, 0]]);
        let b = word_automaton(0, &[5], &[[0, 0]]);
        let m = compare_word_automata(
            &a,
            &b,
            RelationalOp::Equal,
            &mut crate::logging::Logging::new(),
        );
        assert!(!m.fa.accepts_word(&[]));
    }

    // -------------------------------------------------------------------
    // apply_word_arith_operator / apply_word_operator
    // -------------------------------------------------------------------

    #[test]
    fn apply_word_arith_operator_adds_constant_to_each_state() {
        let mut a = word_automaton(0, &[3], &[[0, 0]]);
        // reverse == false: `o op thisP`, i.e. `10 + 3`.
        apply_word_arith_operator(&mut a, 10, ArithmeticOp::Plus, false).unwrap();
        assert_eq!(a.fa.o[a.fa.q0], 13);
    }

    #[test]
    fn apply_word_arith_operator_reverse_flips_operand_order() {
        let mut a = word_automaton(0, &[3], &[[0, 0]]);
        // reverse == true: `thisP op o`, i.e. `3 - 10`.
        apply_word_arith_operator(&mut a, 10, ArithmeticOp::Minus, true).unwrap();
        assert_eq!(a.fa.o[a.fa.q0], -7);
    }

    #[test]
    fn apply_word_arith_operator_division_by_zero_is_an_error() {
        // reverse == false computes `arith(op, o, thisP)`, so `thisP == 0` is the
        // divisor.
        let mut a = word_automaton(0, &[0], &[[0, 0]]);
        let err = apply_word_arith_operator(&mut a, 5, ArithmeticOp::Div, false).unwrap_err();
        assert_eq!(err, NumSysError::DivisionByZero);
    }

    #[test]
    fn apply_word_operator_multiplies_two_word_automata() {
        let a = word_automaton(0, &[3], &[[0, 0]]);
        let b = word_automaton(0, &[4], &[[0, 0]]);
        let n = apply_word_operator(&a, &b, ArithmeticOp::Mult);
        assert_eq!(n.fa.o[n.fa.q0], 12);
    }

    #[test]
    #[should_panic(expected = "division by zero")]
    fn apply_word_operator_division_by_zero_panics() {
        let a = word_automaton(0, &[3], &[[0, 0]]);
        let b = word_automaton(0, &[0], &[[0, 0]]);
        apply_word_operator(&a, &b, ArithmeticOp::Div);
    }

    // -------------------------------------------------------------------
    // reverse_with_output
    // -------------------------------------------------------------------

    #[test]
    fn reverse_with_output_true_false_automaton_is_a_no_op() {
        let mut a = Automaton::true_false(true);
        reverse_with_output(&mut a, true);
        assert!(a.fa.is_true_automaton());
    }

    /// Reversal at the empty string must reproduce the original's output at `q0`
    /// (Theorem 4.3.3: reverse("") == "", so `reversed[""] == original[""]`), when
    /// `q0` is already `0` — the common, non-buggy case.
    #[test]
    fn reverse_with_output_empty_string_output_matches_q0_when_q0_is_zero() {
        let mut a = word_automaton(0, &[10, 20], &[[0, 1], [1, 0]]);
        reverse_with_output(&mut a, false);
        assert_eq!(a.fa.o[a.fa.q0], 10);
    }

    /// **WB-016** (`docs/WALNUT-BUGS.md`): the SAME automaton as the test above, just
    /// with its two states relabeled so `q0` is `1` instead of `0`, isomorphic in every
    /// other respect. A correct reversal would still report `10` at the empty string
    /// (Theorem 4.3.3 doesn't care how states are numbered) — but this pins the REAL,
    /// verbatim-ported Walnut bug: `q0` is never updated after the rebuild, so the
    /// wrong state ends up treated as initial. This test asserts the WRONG value,
    /// matching real `walnut-java`'s own empirically-confirmed output, per
    /// `CLAUDE.md`'s mechanical-port rule (pin the bug, don't silently fix it).
    #[test]
    fn reverse_with_output_wb016_wrong_q0_on_non_zero_initial_state() {
        // state 1 (q0), output 10, 0->1, 1->0; state 0, output 20, 0->0, 1->1.
        let mut a = word_automaton(1, &[20, 10], &[[0, 1], [1, 0]]);
        reverse_with_output(&mut a, false);
        // Faithful (buggy) Rust output: 20, not the mathematically-correct 10 (see the
        // sibling test above, which uses the isomorphic q0==0 layout and gets 10).
        assert_eq!(a.fa.o[a.fa.q0], 20);
    }

    // -------------------------------------------------------------------
    // uncombine / minimize_with_output
    // -------------------------------------------------------------------

    #[test]
    fn uncombine_splits_by_output_value() {
        let a = word_automaton(0, &[1, 2, 1], &[[1, 2], [1, 2], [1, 2]]);
        let subs = uncombine(&a, &[1, 2]);
        assert_eq!(subs.len(), 2);
        assert!(subs[0].fa.is_accepting(0));
        assert!(!subs[0].fa.is_accepting(1));
        assert!(subs[1].fa.is_accepting(1));
        assert!(!subs[1].fa.is_accepting(0));
        // State 2 shares output value 1 with state 0 (both untouched by the earlier
        // asserts) -- `uncombine` preserves state indices (it clones and calls
        // `restrict_output_to`, never renumbers), so it must land in the SAME
        // partition as state 0, not state 1's.
        assert!(
            subs[0].fa.is_accepting(2),
            "state 2 (output 1) belongs in the output-1 partition, same as state 0"
        );
        assert!(
            !subs[1].fa.is_accepting(2),
            "state 2 (output 1) must NOT be accepting in the output-2 partition"
        );
    }

    // Two states, distinct outputs, and CONNECTED (q0 can reach the other state) —
    // deliberately not the disconnected self-loop-only shape, which is WB-001's
    // trigger (`docs/WALNUT-BUGS.md`): `minimize_with_output` uncombines into one
    // boolean sub-automaton per output value, and for the sub-automaton where q0's
    // OWN output isn't the target value, q0 is non-accepting there — if it also
    // couldn't reach the (elsewhere) accepting state, the ported Valmari minimizer
    // would silently misclassify it, corrupting `combine`'s result. Connected
    // transitions keep these tests about `minimize_with_output`'s own correctness.
    /// Walks a deterministic, total word automaton's transitions for `word` starting
    /// at `q0` and returns the output at the resulting state. Used below to check the
    /// SECOND fixture state's output post-minimize by REACHING it via a word (symbol
    /// `1` from `q0`) rather than by a fixed state index — minimization is free to
    /// renumber states, so there is no guarantee the original "state 1" is still
    /// index 1 afterwards.
    fn word_output(a: &Automaton, word: &[i32]) -> i32 {
        let mut state = a.fa.q0;
        for &sym in word {
            state = a.fa.d[state][&sym][0];
        }
        a.fa.o[state]
    }

    #[test]
    fn minimize_with_output_preserves_per_state_output_language() {
        let a = word_automaton(0, &[3, 9], &[[0, 1], [1, 0]]);
        let n = minimize_with_output(&a);
        assert_eq!(n.fa.o[n.fa.q0], 3);
        assert_eq!(
            word_output(&n, &[1]),
            9,
            "the second state's output (reached via symbol 1 from q0) must also be preserved"
        );
    }

    #[test]
    fn minimize_self_with_output_mutates_in_place() {
        let mut a = word_automaton(0, &[3, 9], &[[0, 1], [1, 0]]);
        minimize_self_with_output(&mut a);
        assert_eq!(a.fa.o[a.fa.q0], 3);
        assert_eq!(
            word_output(&a, &[1]),
            9,
            "the second state's output (reached via symbol 1 from q0) must also be preserved"
        );
    }

    /// `Automaton.copy(N)` (`Automaton.java:312-318`) copies fields only, never `this`'s
    /// runtime class — so `dfa_typed` must survive `minimize_self_with_output` exactly
    /// like every other field, even though `combine`'s own internal result (two distinct
    /// outputs here, `3` and `9`, so it takes the plain-`crossProduct` branch) is not
    /// itself DFA-typed. Currently unreachable from any real call site (see this
    /// function's own docs), added per adversarial review's "fix the bug class, not just
    /// the one live instance" finding.
    #[test]
    fn minimize_self_with_output_preserves_dfa_typed_like_javas_copy() {
        let mut a = word_automaton(0, &[3, 9], &[[0, 1], [1, 0]]);
        a.dfa_typed = true;
        minimize_self_with_output(&mut a);
        assert!(
            a.dfa_typed,
            "copy(N) never reassigns the receiver's own runtime class"
        );
    }
}
