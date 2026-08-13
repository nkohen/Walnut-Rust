// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Shortest-witness BFS over `int[]`-tuple product states.
//!
//! Ports `Automata/Search/ProductBFS.java` (406 LOC) — the search PRIMITIVE behind
//! the `test` command (`Main/Commands/Test.java`'s "find the first N shortlex-smallest
//! accepted inputs"), not the command itself. `Test.java` is a later unit (U25); this
//! module gives it exactly what Java's `ProductBFS` gives `Test.java`: a generic BFS
//! over caller-defined product states (an on-the-fly search, not a full product
//! automaton construction — `docs/BOUNDARY-MAP.md` §71 is explicit that this is *not*
//! a `ProductStrategies` alternative), plus a dead-but-KEEP-scoped specialized DFA
//! variant with reverse-reachability pruning (`docs/BOUNDARY-MAP.md` §71/§4.5;
//! confirmed zero callers by `ProductBFSTest.java`'s own class doc comment, in both
//! `main` and `test`).
//!
//! # Sanctioned deviation: static mutable singleton fields become owned locals
//!
//! Java's `idOf`/`states`/`prevId`/`prevSym`/`q` are `private static final` fields —
//! a de facto singleton reused across calls and cleared at the top of every call
//! (`docs/BOUNDARY-MAP.md` §4.5, `PORTING.md`'s "public static global state" entry).
//! This is one of the project's small number of explicitly sanctioned exceptions to
//! "mechanical port first, idiomatize later" (`CLAUDE.md`, U19's task brief): every
//! one of those fields is owned local state in [`shortest_witness_word_int`] below —
//! a `HashMap`, three `Vec`s and a `VecDeque` — not a Rust `static`. Nothing else
//! about the algorithm (BFS order, pruning, tie-breaking, reconstruction) changed.
//!
//! # `net.automatalib` types are a format surface, not a dependency to add
//!
//! Java's specialized variant ([`shortest_witness_word_product`]) is generic over
//! `CompactDFA<Integer>` (AutomataLib's deterministic-automaton type) and returns
//! `Word<Integer>`. Per U19's task brief, these have no Rust crate equivalent worth
//! adding — [`shortest_witness_word_product`] instead operates on this crate's own
//! [`crate::fa::Fa`] (assumed deterministic per component, exactly as `CompactDFA`
//! is: [`dfa_successor`] reads at most one destination per `(state, symbol)`, mirroring
//! `CompactDFA.getSuccessor`'s "-1 means no transition" contract) and both search
//! functions return a plain `Vec<i32>` witness (`Word.epsilon()` -> `Vec::new()`,
//! `Word.fromSymbols(..)` -> the symbols in order) rather than an AutomataLib `Word`.
//!
//! # Trivial (`TRUE`/`FALSE`) automaton inputs
//!
//! Java's `ProductBFS.java` itself has **zero** awareness of `Automaton`/`FA` at all
//! — it is pure `int[]`-tuple BFS driven entirely by caller-supplied closures, so it
//! cannot "assume every automaton has real states" in the first place; the guard
//! lives at the call site instead (`Test.findNextAcceptedWord`'s own
//! `M.fa.isTRUE_FALSE_AUTOMATON()` check *before* ever calling `ProductBFS`,
//! `Test.java:73-77` — that guard is `Test.java`'s own logic, out of scope for this
//! unit, deferred to U25 along with the rest of the `test` command). Because U25 will
//! still need *some* automaton-aware entry point into this primitive, this module
//! adds one small piece of genuinely new (not ported) glue for that integration
//! seam: [`shortest_accepted_word`], a single-automaton convenience matching
//! `Test.java`'s own call shape (search from `fa.q0`, take the first NFA
//! destination per symbol, accept on `Fa::is_accepting`). It checks
//! [`crate::automaton::Automaton::fa`]'s [`crate::fa::Fa::is_true_false_automaton`]
//! explicitly up front rather than indexing into the empty/stale-`q` trivial `Fa`
//! shapes `fa.rs`'s own module docs describe, so a `TRUE`/`FALSE` automaton input is
//! handled cleanly rather than panicking.

use crate::automaton::Automaton;
use crate::fa::Fa;
use std::collections::{HashMap, VecDeque};

/// `ProductBFS.shortestWitnessWordInt(int[], int, IntStep, IntAccepting)`
/// (`ProductBFS.java:89-149`).
///
/// Breadth-first search over product states represented as fixed-length `i32`
/// tuples, starting from `start`. `step(state, symbol, out)` writes the successor of
/// `(state, symbol)` into `out` and returns whether that successor should be
/// explored (`false` prunes it — matches Java's `IntStep`, `:45-52`).
/// `accepting(state)` tests whether `state` is a target (`IntAccepting`, `:54-57`).
/// The global alphabet is `0..alphabet_size`, tried in that order at every state, so
/// BFS discovery order — and hence which witness is returned when several exist at
/// the same length — is deterministic (`ProductBFS.java:132`'s `for (int symbol = 0;
/// symbol < alphabetSize; symbol++)`).
///
/// Returns `Some(vec![])` if `start` is already accepting (`Word.epsilon()`,
/// `:93-95`), `Some(witness)` (shortlex-shortest, symbols in read order) if some
/// reachable state is accepting, or `None` if the BFS exhausts the reachable state
/// space without ever finding one (`:148`, Java's `null`; see `PORTING.md`'s
/// `null`-return -> `Option<T>` ruling).
pub fn shortest_witness_word_int<Step, Accepting>(
    start: &[i32],
    alphabet_size: usize,
    mut step: Step,
    mut accepting: Accepting,
) -> Option<Vec<i32>>
where
    Step: FnMut(&[i32], i32, &mut [i32]) -> bool,
    Accepting: FnMut(&[i32]) -> bool,
{
    if accepting(start) {
        return Some(Vec::new());
    }

    let tuple_length = start.len();

    // Java: idOf (visited: tuple content -> state id, default -1).
    let mut id_of: HashMap<Vec<i32>, usize> = HashMap::new();
    // Java: states (state id -> owned tuple).
    let mut states: Vec<Vec<i32>> = Vec::with_capacity(1024);
    // Java: prevId / prevSym (backpointers). `None` at index 0 stands in for Java's
    // prevId sentinel `-1` (reconstructWord's loop condition `prevId[id] != -1`);
    // prevSym[0] is correspondingly never read (the loop body never runs for id 0),
    // so it carries no meaningful value, matching Java's stored `-1` there.
    let mut prev_id: Vec<Option<usize>> = Vec::with_capacity(1024);
    let mut prev_sym: Vec<i32> = Vec::with_capacity(1024);
    // Java: q (an IntArrayFIFOQueue -- despite the `IntPriorityQueue` interface type,
    // it is a plain FIFO with no comparator, i.e. ordinary BFS order).
    let mut queue: VecDeque<usize> = VecDeque::new();

    let start_key = start.to_vec();
    id_of.insert(start_key.clone(), 0);
    states.push(start_key);
    prev_id.push(None);
    prev_sym.push(-1);
    queue.push_back(0);

    // Java: nextStateBuffer, a reused scratch buffer for step(...)'s output.
    let mut next_state_buffer = vec![0i32; tuple_length];

    while let Some(current_state_id) = queue.pop_front() {
        // Cloned (not borrowed) so `states` can be mutated later in this same
        // iteration while `current_state` is still in scope -- `current_state` never
        // changes across the symbol loop below, matching Java's `currentState`
        // local, which is also read-only for the rest of the iteration.
        let current_state = states[current_state_id].clone();

        for symbol_usize in 0..alphabet_size {
            let symbol = symbol_usize as i32;

            if !step(&current_state, symbol, &mut next_state_buffer) {
                continue;
            }

            if id_of.contains_key(next_state_buffer.as_slice()) {
                continue; // already visited
            }

            let next_state_id = states.len();
            let stored_next_state = next_state_buffer.clone();

            id_of.insert(stored_next_state.clone(), next_state_id);
            states.push(stored_next_state);
            prev_id.push(Some(current_state_id));
            prev_sym.push(symbol);

            if accepting(&states[next_state_id]) {
                return Some(reconstruct_word(next_state_id, &prev_id, &prev_sym));
            }

            queue.push_back(next_state_id);
        }
    }

    None
}

/// `ProductBFS.reconstructWord(int)` (`ProductBFS.java:395-404`) — walks parent
/// pointers back from `accepting_state_id` to the start state (id `0`, whose
/// `prev_id` entry is `None`), collecting the symbol used to reach each state, then
/// reverses to read start-to-end.
fn reconstruct_word(mut state_id: usize, prev_id: &[Option<usize>], prev_sym: &[i32]) -> Vec<i32> {
    let mut reversed_syms = Vec::new();
    while let Some(parent) = prev_id[state_id] {
        reversed_syms.push(prev_sym[state_id]);
        state_id = parent;
    }
    reversed_syms.reverse();
    reversed_syms
}

/// `CompactDFA.getSuccessor(int, int)`'s contract, as this port's stand-in for
/// `net.automatalib`'s `CompactDFA<Integer>` (see this module's docs): the single
/// destination of `(state, sym)` in a DETERMINISTIC component, or `None` if that
/// transition is unset ("dead"). [`shortest_witness_word_product`] never calls this
/// on a component whose [`Fa::is_deterministic`] doesn't hold -- callers are
/// responsible for supplying deterministic components, exactly as Java's
/// `CompactDFA<Integer>[]` type already guarantees at the call site.
fn dfa_successor(dfa: &Fa, state: usize, sym: i32) -> Option<usize> {
    dfa.d[state]
        .get(&sym)
        .and_then(|dests| dests.first().copied())
}

/// `ProductBFS.shortestWitnessWordProduct(int[], int, CompactDFA<Integer>[],
/// int[][], boolean[])` (`ProductBFS.java:161-260`, package-private in Java; `pub`
/// here since this crate has no package-private visibility and a future `wr-cli`
/// unit is this function's only plausible caller). Dead code today (zero callers in
/// `main` or `test`, per `ProductBFSTest.java`'s own class doc comment) but
/// KEEP-scoped (`docs/BOUNDARY-MAP.md` §71/§2), so it's ported and characterized
/// here like any other KEEP file.
///
/// Product of `dfas.len()` deterministic component automata, with per-global-symbol
/// projection (`symbol_maps[global_sym][component_index]` is the local symbol
/// component `component_index` reads for global symbol `global_sym`) and a desired
/// per-component acceptance (`want_accept[component_index]`). The target is a
/// product state whose component-wise acceptance matches `want_accept` in every
/// component. Includes Java's reverse-reachability pruning precomputation: for each
/// component and each of its live states, whether that state can still reach a
/// local state matching the desired acceptance; a product successor is discarded
/// immediately (never enqueued) the moment any one component becomes "hopeless".
///
/// Dead local state (`< 0`) is handled the same way Java's `-1` sentinel is: if
/// `want_accept[component_index]` is `false`, dead is already good (a dead state can
/// never accept again); if `true`, dead can never recover.
pub fn shortest_witness_word_product(
    start: &[i32],
    alphabet_size: usize,
    dfas: &[Fa],
    symbol_maps: &[Vec<i32>],
    want_accept: &[bool],
) -> Option<Vec<i32>> {
    let component_count = dfas.len();

    let projected_syms_by_component = projected_syms_by_component(symbol_maps, component_count);

    let live_state_can_reach_wanted: Vec<Vec<bool>> = (0..component_count)
        .map(|component_index| {
            compute_live_state_can_reach_wanted(
                &dfas[component_index],
                &projected_syms_by_component[component_index],
                want_accept[component_index],
            )
        })
        .collect();

    // Early impossibility check at the start state.
    for component_index in 0..component_count {
        let start_local_state = start[component_index];

        if start_local_state < 0 {
            if want_accept[component_index] {
                return None;
            }
        } else if !live_state_can_reach_wanted[component_index][start_local_state as usize] {
            return None;
        }
    }

    shortest_witness_word_int(
        start,
        alphabet_size,
        |product_state, global_sym, succ_state| {
            let projected_syms_for_global_sym = &symbol_maps[global_sym as usize];

            for component_index in 0..component_count {
                let current_local_state = product_state[component_index];
                let projected_local_sym = projected_syms_for_global_sym[component_index];
                let next_local_state = if current_local_state < 0 {
                    -1
                } else {
                    dfa_successor(
                        &dfas[component_index],
                        current_local_state as usize,
                        projected_local_sym,
                    )
                    .map_or(-1, |s| s as i32)
                };

                succ_state[component_index] = next_local_state;

                // Prune: once a component enters a local state from which the
                // desired local acceptance is no longer reachable, this whole
                // product successor can never lead to a witness.
                if next_local_state < 0 {
                    if want_accept[component_index] {
                        return false;
                    }
                } else if !live_state_can_reach_wanted[component_index][next_local_state as usize] {
                    return false;
                }
            }

            true
        },
        |product_state| {
            for component_index in 0..component_count {
                let component_is_accepting = product_state[component_index] >= 0
                    && dfas[component_index].is_accepting(product_state[component_index] as usize);

                if component_is_accepting != want_accept[component_index] {
                    return false;
                }
            }
            true
        },
    )
}

/// `ProductBFS.projectedSymsByComponent(int[][], int)` (`ProductBFS.java:263-286`).
/// For each component position, collect the distinct local symbols that can appear
/// there when a global symbol is projected through `symbol_maps`, in first-seen
/// order (order-preserving dedup, matching Java's linear "already present" scan).
fn projected_syms_by_component(symbol_maps: &[Vec<i32>], component_count: usize) -> Vec<Vec<i32>> {
    let mut projected_sym_lists: Vec<Vec<i32>> = vec![Vec::new(); component_count];

    for projected_syms_for_global_sym in symbol_maps {
        for component_index in 0..component_count {
            let projected_local_sym = projected_syms_for_global_sym[component_index];
            let syms_seen_for_component = &mut projected_sym_lists[component_index];

            if !syms_seen_for_component.contains(&projected_local_sym) {
                syms_seen_for_component.push(projected_local_sym);
            }
        }
    }

    projected_sym_lists
}

/// `ProductBFS.computeLiveStateCanReachWanted(CompactDFA<Integer>, int[], boolean)`
/// (`ProductBFS.java:308-393`). Reverse reachability over one component DFA's graph,
/// restricted to `allowed_syms`: `result[q] == true` iff live local state `q` has
/// some continuation reaching a local state whose acceptance matches `want_accept`.
/// Dead state is not stored in the returned `Vec` (callers handle it directly, as
/// documented on [`shortest_witness_word_product`]).
fn compute_live_state_can_reach_wanted(
    dfa: &Fa,
    allowed_syms: &[i32],
    want_accept: bool,
) -> Vec<bool> {
    let live_state_count = dfa.q;

    // Pass 1: count predecessors per target state, and separately count
    // transitions that go directly to the dead state.
    let mut pred_count_by_target_state = vec![0usize; live_state_count];
    let mut pred_count_into_dead_state = 0usize;

    for source_state in 0..live_state_count {
        for &allowed_sym in allowed_syms {
            match dfa_successor(dfa, source_state, allowed_sym) {
                None => pred_count_into_dead_state += 1,
                Some(successor_state) => pred_count_by_target_state[successor_state] += 1,
            }
        }
    }

    // Pass 2: allocate exact-sized predecessor lists and fill them.
    let mut pred_states_by_target_state: Vec<Vec<usize>> = pred_count_by_target_state
        .iter()
        .map(|&count| Vec::with_capacity(count))
        .collect();
    let mut pred_states_into_dead_state: Vec<usize> =
        Vec::with_capacity(pred_count_into_dead_state);

    for source_state in 0..live_state_count {
        for &allowed_sym in allowed_syms {
            match dfa_successor(dfa, source_state, allowed_sym) {
                None => pred_states_into_dead_state.push(source_state),
                Some(successor_state) => {
                    pred_states_by_target_state[successor_state].push(source_state)
                }
            }
        }
    }

    let mut live_state_can_reach_wanted = vec![false; live_state_count];
    let mut reverse_bfs_queue: VecDeque<usize> = VecDeque::new();

    // Seed with every live state that is already good: matching acceptance
    // directly, or (when want_accept is false) a direct transition into dead,
    // since dead is non-accepting.
    for (state, can_reach_wanted) in live_state_can_reach_wanted.iter_mut().enumerate() {
        if dfa.is_accepting(state) == want_accept {
            *can_reach_wanted = true;
            reverse_bfs_queue.push_back(state);
        }
    }

    if !want_accept {
        for &predecessor_of_dead_state in &pred_states_into_dead_state {
            if !live_state_can_reach_wanted[predecessor_of_dead_state] {
                live_state_can_reach_wanted[predecessor_of_dead_state] = true;
                reverse_bfs_queue.push_back(predecessor_of_dead_state);
            }
        }
    }

    while let Some(known_good_target_state) = reverse_bfs_queue.pop_front() {
        for &pred_state in &pred_states_by_target_state[known_good_target_state] {
            if !live_state_can_reach_wanted[pred_state] {
                live_state_can_reach_wanted[pred_state] = true;
                reverse_bfs_queue.push_back(pred_state);
            }
        }
    }

    live_state_can_reach_wanted
}

/// New (not ported) integration-seam convenience for U25 -- see this module's docs,
/// "Trivial (TRUE/FALSE) automaton inputs". Finds a shortest word accepted by `a`,
/// searching from `a.fa.q0` and taking the first NFA destination per symbol at each
/// state (matching `Test.java`'s own call shape, `destinations.getInt(0)`,
/// `Test.java:86`). Returns `None` for the FALSE automaton (no word is accepted) and
/// `Some(vec![])` for the TRUE automaton (the empty word is accepted, and the TRUE
/// automaton has no further alphabet to search over -- handled up front rather than
/// falling through into `Fa`'s empty/stale-`q` trivial shapes).
pub fn shortest_accepted_word(a: &Automaton) -> Option<Vec<i32>> {
    if a.fa.is_true_false_automaton() {
        return if a.fa.is_true_automaton() {
            Some(Vec::new())
        } else {
            None
        };
    }

    let start = [a.fa.q0 as i32];

    shortest_witness_word_int(
        &start,
        a.fa.alphabet_size,
        |state, sym, out| match a.fa.d[state[0] as usize].get(&sym) {
            Some(dests) if !dests.is_empty() => {
                out[0] = dests[0] as i32;
                true
            }
            _ => false,
        },
        |state| a.fa.is_accepting(state[0] as usize),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // --- shortest_witness_word_int -- mirrors ProductBFSTest.java's cases ---

    #[test]
    fn start_already_accepting_returns_epsilon() {
        let start = [5];
        let result = shortest_witness_word_int(
            &start,
            2,
            |state, _sym, out| {
                out[0] = state[0] + 1;
                true
            },
            |state| state[0] == 5,
        );
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn no_accepting_state_reachable_returns_none() {
        // Single self-loop under the only symbol; accepting condition never true.
        let start = [0];
        let result = shortest_witness_word_int(
            &start,
            1,
            |_state, _sym, out| {
                out[0] = 0;
                true
            },
            |state| state[0] == 1,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn pruned_transitions_are_skipped() {
        // Symbol 0 is always pruned by step(); only symbol 1 makes progress.
        let start = [0];
        let result = shortest_witness_word_int(
            &start,
            2,
            |_state, sym, out| {
                if sym == 0 {
                    return false;
                }
                out[0] = 1;
                true
            },
            |state| state[0] == 1,
        );
        assert_eq!(result, Some(vec![1]));
    }

    #[test]
    fn finds_shortest_not_first_discovered_path() {
        // From 0: sym0 -> +1, sym1 -> +3. Target is 4.
        // A same-symbol-only path (0,0,0,0) takes 4 steps; BFS must find the 2-step
        // path (sym0 then sym1).
        let start = [0];
        let result = shortest_witness_word_int(
            &start,
            2,
            |state, sym, out| {
                let next = state[0] + if sym == 0 { 1 } else { 3 };
                if next > 10 {
                    return false; // keep the search space bounded
                }
                out[0] = next;
                true
            },
            |state| state[0] == 4,
        );
        assert_eq!(result, Some(vec![0, 1]));
    }

    #[test]
    fn already_visited_successor_is_not_re_explored() {
        // From 0: both symbols lead to content {1}. The second (sym1) must hit the
        // "already visited" skip, not create a duplicate state. From 1: sym0 -> {2},
        // which is accepting. Expected shortest word is [0, 0].
        let start = [0];
        let result = shortest_witness_word_int(
            &start,
            2,
            |state, sym, out| {
                if state[0] == 0 {
                    out[0] = 1; // both symbols collapse to the same successor content
                    return true;
                }
                if state[0] == 1 && sym == 0 {
                    out[0] = 2;
                    return true;
                }
                false
            },
            |state| state[0] == 2,
        );
        assert_eq!(result, Some(vec![0, 0]));
    }

    #[test]
    fn multi_component_tuple_dedupes_by_content_not_identity() {
        // int[2] state; only the second component advances. Confirms tuple-content
        // equality (not reference identity) drives the visited-state memoization.
        let start = [7, 0];
        let result = shortest_witness_word_int(
            &start,
            1,
            |state, _sym, out| {
                out[0] = state[0];
                out[1] = state[1] + 1;
                true
            },
            |state| state[1] == 2,
        );
        assert_eq!(result, Some(vec![0, 0]));
    }

    // --- shortest_witness_word_product -- mirrors ProductBFSTest.java's cases ---

    /// Accepts iff the input contains at least one occurrence of `watched_symbol`,
    /// over a two-symbol alphabet `{0, 1}`.
    fn contains_symbol(watched_symbol: i32) -> Fa {
        let mut d = vec![BTreeMap::new(), BTreeMap::new()];
        for sym in [0, 1] {
            d[0].insert(sym, vec![if sym == watched_symbol { 1 } else { 0 }]);
            d[1].insert(sym, vec![1]);
        }
        Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d,
            true_false: None,
        }
    }

    #[test]
    fn product_finds_shortest_word_satisfying_both_components() {
        // Component 0 wants to have seen a '1'; component 1 wants to have seen a
        // '0'. Shortest word satisfying both simultaneously is length 2 ("01" or
        // "10").
        let dfas = [contains_symbol(1), contains_symbol(0)];
        let symbol_maps = vec![vec![0, 0], vec![1, 1]]; // global symbol drives both components identically
        let want_accept = [true, true];

        let result = shortest_witness_word_product(&[0, 0], 2, &dfas, &symbol_maps, &want_accept);

        let result = result.expect("expected a witness");
        assert_eq!(result.len(), 2);
        assert_ne!(result[0], result[1]);
    }

    #[test]
    fn product_negative_start_component_with_want_accept_true_returns_none_immediately() {
        // start[0] == -1 (already-dead marker) with want_accept[0] == true must
        // short-circuit to None before any BFS.
        let mut trivial = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 1,
            o: vec![0],
            d: vec![BTreeMap::new()],
            true_false: None,
        };
        trivial.d[0].insert(0, vec![0]);

        let dfas = [trivial.clone(), trivial];
        let symbol_maps = vec![vec![0, 0]];
        let want_accept = [true, false];

        let result = shortest_witness_word_product(&[-1, 0], 1, &dfas, &symbol_maps, &want_accept);
        assert_eq!(result, None);
    }

    #[test]
    fn product_negative_start_component_with_want_accept_false_is_fine_not_an_early_failure() {
        // Same setup, but want_accept[0] == false: a dead component is already
        // "good enough" for that component, so this must NOT be rejected by the
        // `start_local_state < 0` branch. Component 1 still needs to reach its own
        // accepting state via BFS.
        let mut comp1 = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![BTreeMap::new(), BTreeMap::new()],
            true_false: None,
        };
        comp1.d[0].insert(0, vec![1]);
        comp1.d[1].insert(0, vec![1]);

        let mut dead_component = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 1,
            o: vec![0],
            d: vec![BTreeMap::new()],
            true_false: None,
        };
        dead_component.d[0].insert(0, vec![0]);

        let dfas = [dead_component, comp1];
        let symbol_maps = vec![vec![0, 0]];
        let want_accept = [false, true];

        let result = shortest_witness_word_product(&[-1, 0], 1, &dfas, &symbol_maps, &want_accept);
        assert_eq!(result, Some(vec![0]));
    }

    #[test]
    fn product_live_start_that_can_never_reach_wanted_returns_none_via_live_check() {
        // start component is >= 0 (live) but its DFA can never reach an accepting
        // state at all, while want_accept requires accepting.
        let mut never_accepts = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 1,
            o: vec![0],
            d: vec![BTreeMap::new()],
            true_false: None,
        };
        never_accepts.d[0].insert(0, vec![0]);

        let dfas = [never_accepts];
        let symbol_maps = vec![vec![0]];
        let want_accept = [true];

        let result = shortest_witness_word_product(&[0], 1, &dfas, &symbol_maps, &want_accept);
        assert_eq!(result, None);
    }

    #[test]
    fn product_mid_bfs_pruning_discards_hopeless_successor() {
        // state 0 -> (sym 0) -> trap (never accepts) or -> (sym 1) -> acc
        // (accepting). want_accept = true. Taking sym 0 first leads into a state
        // from which acceptance is unreachable, so mid-BFS pruning must discard it,
        // forcing the search to use sym 1 directly.
        let mut dfa = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 2,
            o: vec![0, 0, 1],
            d: vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()],
            true_false: None,
        };
        dfa.d[0].insert(0, vec![1]);
        dfa.d[0].insert(1, vec![2]);
        dfa.d[1].insert(0, vec![1]);
        dfa.d[1].insert(1, vec![1]);
        dfa.d[2].insert(0, vec![2]);
        dfa.d[2].insert(1, vec![2]);

        let dfas = [dfa];
        let symbol_maps = vec![vec![0], vec![1]];
        let want_accept = [true];

        let result = shortest_witness_word_product(&[0], 2, &dfas, &symbol_maps, &want_accept);
        assert_eq!(result, Some(vec![1]));
    }

    #[test]
    fn product_duplicate_projected_local_symbol_deduped_by_projected_syms_by_component() {
        // Global symbol 2 projects the same local symbol (0) onto component 0 as
        // global symbol 0 does -- exercises the "already present" dedup path.
        let comp = contains_symbol(1);
        let dfas = [comp.clone(), comp];
        let symbol_maps = vec![vec![0, 0], vec![1, 1], vec![0, 0]]; // global 2 duplicates global 0
        let want_accept = [true, true];

        let result = shortest_witness_word_product(&[0, 0], 3, &dfas, &symbol_maps, &want_accept);
        assert_eq!(result, Some(vec![1]));
    }

    #[test]
    fn product_dead_transition_with_want_accept_false_is_treated_as_satisfying() {
        // A component whose only relevant local symbol is unset (dead) from its
        // (accepting) start state, with want_accept == false for that component:
        // reaching dead is already good when want_accept is false.
        let mut dfa = Fa {
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![1], // accepting start state, so it does NOT already satisfy want_accept=false
            d: vec![BTreeMap::new()],
            true_false: None,
        };
        dfa.d[0].insert(0, vec![0]); // local symbol 0 defined (self-loop); local symbol 1 left UNSET (dead)

        let dfas = [dfa];
        let symbol_maps = vec![vec![1]]; // the only global symbol projects to the dead local symbol
        let want_accept = [false];

        let result = shortest_witness_word_product(&[0], 1, &dfas, &symbol_maps, &want_accept);
        assert_eq!(result, Some(vec![0]));
    }

    // --- shortest_accepted_word -- trivial (TRUE/FALSE) automaton handling (U19) ---

    #[test]
    fn shortest_accepted_word_false_automaton_returns_none_without_panicking() {
        let a = Automaton::true_false(false);
        assert_eq!(shortest_accepted_word(&a), None);
    }

    #[test]
    fn shortest_accepted_word_true_automaton_returns_epsilon_without_panicking() {
        let a = Automaton::true_false(true);
        assert_eq!(shortest_accepted_word(&a), Some(Vec::new()));
    }

    #[test]
    fn shortest_accepted_word_finds_shortest_word_on_a_real_automaton() {
        // Two-state DFA over {0,1}: state 0 (non-accepting) --1--> state 1 (accepting);
        // state 0 --0--> state 0 (self-loop); state 1 total on both symbols.
        let mut d = vec![BTreeMap::new(), BTreeMap::new()];
        d[0].insert(0, vec![0]);
        d[0].insert(1, vec![1]);
        d[1].insert(0, vec![1]);
        d[1].insert(1, vec![1]);
        let fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d,
            true_false: None,
        };
        let a = Automaton::new(fa, vec![vec![0, 1]], vec!["x".to_string()], vec![None]);
        assert_eq!(shortest_accepted_word(&a), Some(vec![1]));
    }
}
