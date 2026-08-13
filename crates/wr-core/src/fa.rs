// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Core finite-automaton representation.
//!
//! Ports the state/transition/output core of Walnut's `Automata/FA/FA.java`. A single
//! unified NFA-shaped transition table backs both NFA and DFA use here — Java keeps two
//! backing types, `TransitionsNFA`/`TransitionsDFA`, as a storage optimization, not a
//! behavioral quirk, so collapsing them doesn't violate the mechanical-port discipline.
//! Callers that require determinism (`minimize`, the equivalence oracle) must check
//! [`Fa::is_deterministic`]/[`Fa::is_deterministic_and_total`] themselves; those checks
//! are hard errors at the call site, never `debug_assert!` (`PORTING.md`'s named
//! "debug_assert erasing side effects" regression class — it vanishes in release builds).
//!
//! Symbols are pre-encoded integers (mixed-radix multi-track digit tuples, matching
//! Java's `FA`/`Transitions` layering) — `Fa` itself is track-agnostic; track
//! encode/decode lives in `crate::automaton`.
//!
//! # The trivial (TRUE/FALSE) automaton — [`Fa::true_false`] (U0)
//!
//! Walnut has two *special* automata that are not really automata at all: the TRUE
//! automaton (accepts everything) and the FALSE automaton (accepts nothing). Java tags
//! them with **two** private `boolean` fields on `FA` itself (`FA.java:52-53`,
//! documented at `FA.java:593-617`):
//!
//! | `TRUE_FALSE_AUTOMATON` | `TRUE_AUTOMATON` | meaning |
//! |---|---|---|
//! | `false` | (ignored) | an ordinary automaton |
//! | `true`  | `false`   | the FALSE automaton |
//! | `true`  | `true`    | the TRUE automaton |
//!
//! This port collapses that pair into ONE `Option<bool>` field ([`Fa::true_false`]):
//! `None` / `Some(false)` / `Some(true)`, matching the three rows above in order.
//! That is a *representation* choice, not a behavior change, and it was verified
//! exhaustively rather than assumed — the fourth combination (`TRUE_FALSE_AUTOMATON ==
//! false && TRUE_AUTOMATON == true`) is both unreachable and unobservable in Walnut:
//!
//! * **Every writer of `TRUE_AUTOMATON` also sets `TRUE_FALSE_AUTOMATON = true`** (all
//!   five sites, from grepping the whole `src/main/java` tree): `Automaton.java:108-109`
//!   and `AutomatonDFA.java:23-24` (the boolean constructors),
//!   `AutomatonReader.java:143-144` (the `true`/`false` file path),
//!   `AutomatonQuantification.java:61-62` (the all-tracks-quantified path — note it
//!   writes `TRUE_AUTOMATON` *first*, so the invalid combination does exist *between*
//!   those two statements, but nothing reads it in between), and `FA.clone()`
//!   (`FA.java:450-451`, copies both). `AutomatonLogicalOps.java:147`'s
//!   `setTRUE_AUTOMATON(!isTRUE_AUTOMATON())` runs only *inside* an
//!   `isTRUE_FALSE_AUTOMATON()` branch, so it cannot create the combination either.
//! * **Every reader of `isTRUE_AUTOMATON()` is already guarded by
//!   `isTRUE_FALSE_AUTOMATON()`** — including the one that looks unguarded at a glance,
//!   `AutomatonLogicalOps.java:103` (`imply`), which sits under the enclosing
//!   `A.fa.isTRUE_FALSE_AUTOMATON() || B.fa.isTRUE_FALSE_AUTOMATON()` test *after* the
//!   `A`-side case has already returned, so `B` is necessarily the trivial one there.
//!
//! [`Fa::is_true_false_automaton`], [`Fa::is_true_automaton`] and
//! [`Fa::true_false_string`] mirror Java's getters one-for-one, so ported call sites
//! read the same as their Java originals.
//!
//! A trivial `Fa` carries **no** meaningful `q`/`o`/`d`. Two distinct shapes occur in
//! Walnut and every `Fa`-level method has to tolerate both:
//! * the fresh one from `new Automaton(boolean)` — `Q == 0`, empty `O`/`d`
//!   ([`Fa::trivial`] builds exactly this);
//! * the one `AutomatonQuantification`'s all-tracks-quantified path leaves behind,
//!   where `Automaton.clear()` -> `FA.clear()` (`FA.java:90-94`) empties `O`/`d` but
//!   **leaves `Q` stale and non-zero** (see [`Fa::clear`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// A nondeterministic finite automaton with per-state integer output.
///
/// `o[s] == 0` means state `s` is non-accepting; a plain NFA/DFA predicate automaton
/// uses only `0`/`1`. Larger values are DFAO word-output values (Walnut's `isFAO`) —
/// not yet exercised by this port (the equivalence oracle in particular only handles
/// 0/1 acceptance, see `equiv` module docs), but the field shape matches Java's `FA.O`
/// so it's ready when DFAO support lands.
#[derive(Debug, Clone)]
pub struct Fa {
    pub q0: usize,
    pub q: usize,
    pub alphabet_size: usize,
    pub o: Vec<i32>,
    /// `d[state][symbol] = destinations`. NFA-shaped (more than one destination is
    /// allowed); `is_deterministic()` distinguishes DFA use at the call site rather
    /// than the type system, mirroring Java's runtime `Transitions.isDeterministic()`.
    pub d: Vec<BTreeMap<i32, Vec<usize>>>,
    /// Walnut's `FA.TRUE_FALSE_AUTOMATON`/`TRUE_AUTOMATON` pair, collapsed into one
    /// `Option` — `None` for an ordinary automaton, `Some(true)` for the TRUE
    /// automaton, `Some(false)` for the FALSE automaton. See this module's docs for
    /// the exhaustive justification that this loses nothing, and for the two `q`/`o`/`d`
    /// shapes a trivial `Fa` can have.
    ///
    /// When this is `Some(..)`, `q`/`o`/`d`/`alphabet_size` are **meaningless** and must
    /// not be interpreted; use [`Fa::is_true_automaton`] instead.
    pub true_false: Option<bool>,
}

impl Fa {
    /// `new Automaton(boolean)`'s `FA` half (`Automaton.java:106-110` +
    /// `FA()`/`FA.java:55-58`): the trivial TRUE (`truth == true`) or FALSE
    /// (`truth == false`) automaton — zero states, no transitions, no alphabet.
    pub fn trivial(truth: bool) -> Fa {
        Fa {
            q0: 0,
            q: 0,
            alphabet_size: 0,
            o: Vec::new(),
            d: Vec::new(),
            true_false: Some(truth),
        }
    }

    /// `FA.isTRUE_FALSE_AUTOMATON()` (`FA.java:599-601`).
    pub fn is_true_false_automaton(&self) -> bool {
        self.true_false.is_some()
    }

    /// `FA.isTRUE_AUTOMATON()` (`FA.java:607-609`). Only meaningful when
    /// [`Fa::is_true_false_automaton`] holds — see this module's docs on why Java's
    /// "`TRUE_AUTOMATON` without `TRUE_FALSE_AUTOMATON`" combination is unreachable
    /// and unread.
    pub fn is_true_automaton(&self) -> bool {
        self.true_false == Some(true)
    }

    /// `FA.trueFalseString()` (`FA.java:611-613`): `Boolean.toString(TRUE_AUTOMATON)`.
    /// This is what `Writer/AutomatonWriter.writeTxtFormatToStream` (`:49-50`) writes
    /// as the ENTIRE body of a trivial automaton's `.txt` file — the string the 85
    /// trivial golden fixtures consist of.
    pub fn true_false_string(&self) -> &'static str {
        if self.is_true_automaton() {
            "true"
        } else {
            "false"
        }
    }

    /// `FA.clear()` (`FA.java:90-94`): empties `O` and the transition table and clears
    /// the `canonized` memo (which `Fa` doesn't carry — see [`Fa::canonicalize`]).
    ///
    /// **Faithfully leaves `q`, `q0`, `alphabet_size` and `true_false` untouched**, so
    /// its one Java caller (`Automaton.clear()`, itself only called from
    /// `AutomatonQuantification`'s all-tracks-quantified path) leaves behind a trivial
    /// automaton with a stale, non-zero `q` — the second of the two trivial shapes this
    /// module's docs describe. Not "tidied up" here: `Fa::canonicalize`/`crate::trim`
    /// both branch on `true_false` exactly where Java does, so the stale value is never
    /// interpreted, and normalizing it would be an undeclared divergence.
    pub fn clear(&mut self) {
        self.o.clear();
        self.d.clear();
    }

    pub fn is_accepting(&self, s: usize) -> bool {
        self.o[s] != 0
    }

    /// `FA.isFAO()` (`FA.java:64-71`): true iff some state's output is `> 1`, i.e. this
    /// is a word automaton (DFAO) rather than a plain accept/reject predicate automaton.
    ///
    /// Used by [`crate::determinize::determinize`] to reproduce Java's "DFAOs are not
    /// supported for non-SC strategies" guard (`DeterminizationStrategies.java:115-119`)
    /// and to fill in the `isFAO` argument its `[export ...]` metacommand hands to
    /// `ProverHelper.exportAutomata` (`:107-108`).
    ///
    /// Fidelity note: Java iterates `for (int i = 0; i < Q; i++) O.getInt(i)`, so on the
    /// one malformed shape this crate can produce — a cleared trivial automaton, whose
    /// `q` stays stale while `o` is emptied (see [`Fa::clear`]) — Java would throw
    /// `IndexOutOfBoundsException` where this returns `false`. Iterating `o` rather than
    /// `0..q` matches the convention every other predicate on this type already follows
    /// ([`Fa::is_deterministic`], [`Fa::is_language_empty`]), and the two are
    /// indistinguishable for well-formed automata, where `o.len() == q`.
    pub fn is_fao(&self) -> bool {
        self.o.iter().any(|&out| out > 1)
    }

    /// True iff every (state, symbol) pair present has at most one destination.
    /// Does NOT require every symbol be present — see [`Fa::is_deterministic_and_total`]
    /// for that stronger check.
    pub fn is_deterministic(&self) -> bool {
        self.d
            .iter()
            .all(|m| m.values().all(|dests| dests.len() <= 1))
    }

    /// True iff every state has EXACTLY one destination for EVERY symbol in
    /// `0..alphabet_size` — the precondition the equivalence oracle requires. Checks
    /// symbol presence explicitly (not just `map.len() == alphabet_size`, which could
    /// pass with the wrong set of keys).
    pub fn is_deterministic_and_total(&self) -> bool {
        (0..self.alphabet_size as i32).all(|sym| {
            self.d
                .iter()
                .all(|m| matches!(m.get(&sym), Some(dests) if dests.len() == 1))
        })
    }

    /// BFS from `q0`; true iff no accepting state is reachable (the automaton
    /// recognizes the empty language). Ports the reachability half of
    /// `FA.isLanguageEmpty` (no track/label bookkeeping at this layer).
    pub fn is_language_empty(&self) -> bool {
        if self.q == 0 {
            return true;
        }
        let mut seen = vec![false; self.q];
        let mut queue = VecDeque::new();
        seen[self.q0] = true;
        queue.push_back(self.q0);
        while let Some(s) = queue.pop_front() {
            if self.is_accepting(s) {
                return false;
            }
            for dests in self.d[s].values() {
                for &d in dests {
                    if !seen[d] {
                        seen[d] = true;
                        queue.push_back(d);
                    }
                }
            }
        }
        true
    }

    /// NFA simulation: does at least one run over `word` (a sequence of already-encoded
    /// symbols) end in an accepting state? Used both by real callers and as a
    /// ground-truth oracle for property tests on nondeterministic intermediates (where
    /// the language-equivalence oracle doesn't apply, since it requires total DFAs).
    pub fn accepts_word(&self, word: &[i32]) -> bool {
        let mut current: BTreeSet<usize> = [self.q0].into_iter().collect();
        for &sym in word {
            let mut next = BTreeSet::new();
            for &s in &current {
                if let Some(dests) = self.d[s].get(&sym) {
                    next.extend(dests.iter().copied());
                }
            }
            current = next;
            if current.is_empty() {
                return false;
            }
        }
        current.iter().any(|&s| self.is_accepting(s))
    }

    /// Adds an explicit sink state (output `sink_output`) and routes every missing
    /// `(state, symbol)` transition to it, including a self-loop on the sink itself.
    /// Requires the automaton already be deterministic (`is_deterministic`); does not
    /// attempt to totalize genuine nondeterminism. A no-op if already total.
    pub fn totalize(&mut self, sink_output: i32) {
        assert!(
            self.is_deterministic(),
            "totalize requires an already-deterministic automaton"
        );
        if self.is_deterministic_and_total() {
            return;
        }
        let sink = self.q;
        self.o.push(sink_output);
        self.q += 1;
        for sym in 0..self.alphabet_size as i32 {
            for s in 0..sink {
                self.d[s].entry(sym).or_insert_with(|| vec![sink]);
            }
        }
        let mut sink_map = BTreeMap::new();
        for sym in 0..self.alphabet_size as i32 {
            sink_map.insert(sym, vec![sink]);
        }
        self.d.push(sink_map);
    }

    /// Appends `destination` onto `transitions[state]`'s per-`symbol` destination list,
    /// creating that list on the first insertion. Ports the small generic
    /// transition-table-mutation helper `FA.addTransition` (`FA.java:305-313`), used
    /// directly by [`Fa::reverse`] below.
    ///
    /// This is also the closest thing `FA.java` has to a reusable per-symbol
    /// "product-transition" primitive for a LATER unit's `FA/ProductStrategies.java`
    /// port to build on: see [`Fa::reverse`]'s doc comment and this module's test for
    /// why the *actual* cross-product BFS
    /// (`ProductStrategies.crossProductInternal`/`crossProductInternalDFA`) is not
    /// itself portable yet at this layer — it is not, in fact, built out of this helper
    /// even in Java (it builds destination lists inline), but it is the same shape of
    /// building block a Rust product implementation would still want.
    pub fn add_transition(
        transitions: &mut [BTreeMap<i32, Vec<usize>>],
        state: usize,
        symbol: i32,
        destination: usize,
    ) {
        transitions[state]
            .entry(symbol)
            .or_default()
            .push(destination);
    }

    /// Reverses every transition and swaps the initial/accepting roles (Walnut's
    /// `FA.reverseToNFAInternal`, `FA.java:274-303`) — the basis for Brzozowski
    /// determinization (`DeterminizationStrategies.Brz`) in a later unit.
    /// `old_initial_states` is the set of states to treat as "the" initial states for
    /// this reversal (ordinarily just `{self.q0}`, matching
    /// `AutomatonLogicalOps.reverse`'s call site; Brzozowski's algorithm and
    /// `DeterminizationStrategies`'s generalized entry points can pass a genuine
    /// multi-state seed, since subset construction — unlike `Fa` itself — has no
    /// restriction to a single initial state).
    ///
    /// Mutates `self` in place (transitions reversed; outputs updated so old accepting
    /// states become non-accepting and `old_initial_states` become accepting) and
    /// returns the set of states that were accepting BEFORE the call — these are the
    /// new automaton's initial *states* (plural). `Fa` has no multi-initial-state
    /// field, so unlike Java's `FA` object this does not attempt to stash them into
    /// `self.q0`; pass the returned set directly as the `initial` argument to
    /// [`crate::determinize::subset_construction`], exactly mirroring the Java call
    /// sites, which immediately feed `reverseToNFAInternal`'s return value into
    /// `SC`/`OTF`. `self.q0` is left completely unmodified — Java doesn't touch its
    /// `q0` field here either; it is stale until the caller re-determinizes, which
    /// always resets `q0` to the fresh DFA's `0`.
    ///
    /// Java also calls `t.reduceMemory()` after reassigning transitions — a
    /// capacity-trimming memory optimization with no behavioral effect, and a no-op for
    /// this crate's `BTreeMap`-backed storage, so it is not replicated.
    ///
    /// # Invariant: `self` is not a usable automaton until re-determinized
    ///
    /// After this call, `self.q0` is stale (unchanged, but meaningless — Java has the
    /// identical staleness). Do not call [`Fa::accepts_word`] or
    /// [`Fa::is_language_empty`] on `self` directly afterward; both would silently read
    /// the wrong start state and give a wrong-but-not-panicking answer. The only
    /// supported next step is feeding the returned set into
    /// [`crate::determinize::subset_construction`], which produces a fresh `Fa` with
    /// its own correct `q0`.
    pub fn reverse(&mut self, old_initial_states: &BTreeSet<usize>) -> BTreeSet<usize> {
        let mut new_d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); self.q];
        for q in 0..self.q {
            for (&sym, dests) in &self.d[q] {
                for &dest in dests {
                    Fa::add_transition(&mut new_d, dest, sym, q);
                }
            }
        }
        self.d = new_d;

        let mut new_initial_states = BTreeSet::new();
        for q in 0..self.q {
            if self.is_accepting(q) {
                new_initial_states.insert(q);
                self.o[q] = 0;
            }
        }
        for &init_state in old_initial_states {
            self.o[init_state] = 1;
        }
        new_initial_states
    }

    /// Builds the raw (possibly-nondeterministic) Kleene-star state/transition
    /// additions onto `n`, which the caller must already have set to a clone of
    /// `automaton` (mirrors Java's `FA.starStates(FA automaton, FA N)`,
    /// `FA.java:97-104`, a mutating two-argument "out-param" static method — kept as
    /// the same shape here rather than a `&mut self` method, since `n` starts as a
    /// distinct clone rather than `automaton` itself; see the call site in
    /// `Main.Commands.Star`, which is NOT in scope for this unit).
    ///
    /// Adds one new state, makes it both the new initial state and accepting, and
    /// gives every already-accepting state of `n` (the new one included) a copy of
    /// `automaton.q0`'s outgoing transitions. This simulates the epsilon transitions
    /// "new q0 -> old q0" and "old final -> old q0" that a textbook epsilon-NFA
    /// Kleene-star construction would use — Walnut's `FA` has no representation for an
    /// actual epsilon edge, so instead of adding one, the target's outgoing
    /// transitions are copied directly onto the source. The new state being marked
    /// accepting handles the "new q0 -> old q0" edge (so old q0's own acceptance, if
    /// any, is inherited); copying old q0's transitions onto every old-and-new
    /// accepting state handles both "new q0 -> old q0" and "old final -> old q0"
    /// continuations in one pass. The result is often nondeterministic; callers must
    /// determinize+minimize afterward (out of scope here).
    pub fn star_states(automaton: &Fa, n: &mut Fa) {
        // Java: `N.q0 = N.Q++` — assigns the OLD length to q0, then bumps the count.
        let new_state = n.q;
        n.q0 = new_state;
        n.q += 1;
        n.o.push(1); // The newly added state is a final (accepting) state.
        n.d.push(BTreeMap::new());

        let source_entries = automaton.d[automaton.q0].clone();
        Fa::merge_in_transitions(n, n.q, &source_entries);
    }

    /// Builds the raw (possibly-nondeterministic) concatenation state/transition
    /// additions onto `n`, which the caller must already have set to a clone of the
    /// FIRST operand automaton (mirrors Java's
    /// `FA.concatStates(FA other, FA N, int originalQ)`, `FA.java:107-124`; see the
    /// call site in `Main.Commands.Concat`, NOT in scope for this unit).
    ///
    /// Appends every state of `other`, shifting its transition destinations by
    /// `original_q` (the first operand's original state count — `other`'s states now
    /// live at indices `[original_q, original_q + other.q)` in `n`), then gives every
    /// state that was accepting in the FIRST operand a copy of `other`'s initial
    /// state's outgoing transitions — the concatenation NFA's "old final -> other's
    /// q0" epsilon transition, again simulated by direct duplication (see
    /// [`Fa::star_states`]'s doc comment for why).
    ///
    /// # Two genuine Walnut (Java) bugs, ported verbatim
    ///
    /// 1. **Wrong source state when `other.q0 != 0`.** The "other's initial state's
    ///    outgoing transitions" above is, in Java, literally `N.t.getEntriesNfaD(originalQ)`
    ///    — i.e. `other`'s state at *index* `0` (shifted), not `other.q0`. This is
    ///    silently correct only when `other.q0 == 0` — true whenever `other` was
    ///    round-tripped through `Writer/AutomatonWriter.java` (which canonizes before
    ///    writing), but **not guaranteed in general**: `AutomatonReader.java` sets
    ///    `q0` to whichever state is declared FIRST in a `.txt` file, so a hand-authored
    ///    file that lists a non-zero state first reads back with `q0 != 0` with no
    ///    canonicalize step in between — confirmed reproducible against the real
    ///    Walnut CLI this way (`docs/WALNUT-BUGS.md` WB-008), not merely an in-memory
    ///    corner case. Contrast [`Fa::star_states`] above, which correctly uses
    ///    `automaton.q0` (not a hardcoded `0`). Pinned by
    ///    `concat_states_quirk_uses_others_state_zero_not_others_q0`.
    /// 2. **`first`'s old final states are never un-marked accepting.** The shared
    ///    `merge_in_transitions` helper is correct for `star_states` (a starred
    ///    automaton's old final states SHOULD remain accepting), but reusing it here
    ///    means `first`'s pre-existing accepting states keep their accepting flag
    ///    after concatenation, too. Whenever ε is NOT in `other`'s language, the raw
    ///    concatenation automaton's language is `L(first) ∪ L(first)·L(other)`, not
    ///    the documented `L(first)·L(other)` (`Help Documentation/Commands/Automata/concat.txt`:
    ///    "accepts the concatenation of the inputs"). Pinned by
    ///    `concat_states_quirk_leaks_first_operands_language_when_second_lacks_epsilon`.
    ///
    /// Both are cataloged for `docs/WALNUT-BUGS.md` (not this crate's job to fix per
    /// `CLAUDE.md`'s mechanical-port rule) and ported exactly as Java has them.
    pub fn concat_states(other: &Fa, n: &mut Fa, original_q: usize) {
        // To access `other`'s states, just use `q`. To access them within `n`, use
        // `original_q + q`.
        for q in 0..other.q {
            n.o.push(other.o[q]);
            let mut new_row = BTreeMap::new();
            for (&sym, dests) in &other.d[q] {
                let shifted: Vec<usize> = dests.iter().map(|&i| original_q + i).collect();
                new_row.insert(sym, shifted);
            }
            n.d.push(new_row);
        }

        // See bug #1 above: this should arguably be `n.d[original_q + other.q0]`.
        let source_entries = n.d[original_q].clone();
        Fa::merge_in_transitions(n, original_q, &source_entries);

        n.q = original_q + other.q;
    }

    /// Shared by [`Fa::star_states`]/[`Fa::concat_states`]: for every state `q` in
    /// `0..bound` that is currently accepting in `n`, merges `source_entries` into
    /// `n.d[q]` (appending to any existing destination list for a shared symbol rather
    /// than overwriting it). Ports `FA.mergeInTransitions` (`FA.java:130-142`).
    fn merge_in_transitions(n: &mut Fa, bound: usize, source_entries: &BTreeMap<i32, Vec<usize>>) {
        for q in 0..bound {
            if !n.is_accepting(q) {
                continue;
            }
            for (&sym, dests) in source_entries {
                n.d[q].entry(sym).or_default().extend(dests.iter().copied());
            }
        }
    }

    /// Renumbers states into breadth-first order from `q0`, mutating `self` in place
    /// (Walnut's `FA.canonizeInternal`, `FA.java:148-191`, plus its
    /// `determinePermutationMap` helper at `FA.java:196-214`). As a side effect, drops
    /// any state BFS never reaches from `q0` — an unreached state has no entry in the
    /// permutation map, so it (and any transition into it) is silently dropped; an
    /// entry all of whose destinations were dropped is removed from its source
    /// state's row entirely rather than kept as an empty destination list
    /// (`FA.java:183-187`: `if (!newDestination.isEmpty()) set-the-remapped-list; else
    /// remove-the-entry` — note the condition, not its negation).
    ///
    /// This is a DIFFERENT reachability notion from [`crate::trim::trim`]:
    /// canonicalize checks only FORWARD reachability from `q0`, never backward
    /// co-reachability to acceptance, so a dead-end branch that can never lead to an
    /// accepting state survives canonicalize untouched (only `trim` removes that).
    ///
    /// Java memoizes behind a `canonized` flag and a `TRUE_FALSE_AUTOMATON`
    /// short-circuit — both are private fields **on `FA` itself** (`FA.java:47-50`,
    /// checked at `FA.java:149`). U0 added the `TRUE_FALSE_AUTOMATON` half to this
    /// crate ([`Fa::true_false`]) and the guard below now reproduces it exactly; the
    /// `canonized` memo is still absent, so this always recomputes — idempotent
    /// (re-running on an already-BFS-ordered automaton is a no-op
    /// transitions-and-output-wise) but behaviorally NOT identical: Java's `canonized`
    /// guard means a flagged `FA` is left un-renumbered even if its actual state order
    /// has changed underneath it (pinned upstream by
    /// `FATest.canonizeInternal_alreadyCanonized_isNoOp`, not replicated here — the flag
    /// doesn't exist on `Fa`).
    ///
    /// **How much this can be observed — an earlier draft of this doc understated it.**
    /// That draft claimed the gap was merely a renumbering, "so no differential/golden
    /// comparison can observe it." That is false in general, because `canonized` is not
    /// only a memo: Walnut also *sets it deliberately to suppress canonicalization*, and
    /// canonicalization DROPS states BFS cannot reach from `q0` (see above). The known
    /// counterexample is `Automata/Morphism.java:88`
    /// (`promotion.fa.setCanonized(true)`, flagging a freshly promoted morphism
    /// automaton whose states are domain letters, most typically unreachable) — running
    /// canonicalization there would change the automaton's state count, not just its
    /// numbering. That call site is not yet ported (`toWordAutomaton` is deferred; see
    /// [`crate::morphism`]'s module docs), so nothing in this crate reaches it *today* —
    /// but the blanket claim was wrong, and the follow-on unit that ports
    /// `toWordAutomaton` must supply the flag or an equivalent guarantee.
    ///
    /// The zero-state guard below agrees with Java on every input Java can actually
    /// construct, but for a different reason than an earlier draft of this doc claimed:
    /// Java does NOT "corrupt the automaton" on a nonexistent `q0` — `determinePermutationMap`
    /// seeds `permutationMap.put(q0, 0)` then immediately dereferences `q0`'s (nonexistent)
    /// transition row and throws `IndexOutOfBoundsException`. That path is simply
    /// unreachable in practice: the *only* `Q == 0` automata Walnut ever builds are the
    /// TRUE/FALSE automata (`Automaton.java`'s boolean constructor), and those hit the
    /// `TRUE_FALSE_AUTOMATON` short-circuit before `canonizeInternal`'s body ever runs.
    /// U0 added that short-circuit here too, as the FIRST guard below, so this crate now
    /// matches Java branch-for-branch; the `q == 0 || d.is_empty()` guard is kept behind
    /// it as defence in depth for the same shapes reached WITHOUT the flag set (nothing
    /// in this crate constructs one, but `Fa`'s fields are `pub`).
    pub fn canonicalize(&mut self) {
        // `FA.canonizeInternal`'s `if (this.canonized || this.isTRUE_FALSE_AUTOMATON())
        // return;` (`FA.java:149`), minus the absent `canonized` memo.
        if self.is_true_false_automaton() {
            return;
        }
        if self.q == 0 || self.d.is_empty() {
            return;
        }
        let permutation = self.determine_permutation_map();
        let new_q = permutation.len();

        let mut new_o = vec![0i32; new_q];
        let mut new_d: Vec<BTreeMap<i32, Vec<usize>>> = vec![BTreeMap::new(); new_q];
        for q in 0..self.q {
            if let Some(&new_id) = permutation.get(&q) {
                new_o[new_id] = self.o[q];
                new_d[new_id] = self.d[q].clone();
            }
        }

        for row in new_d.iter_mut() {
            let mut pruned = BTreeMap::new();
            for (&sym, dests) in row.iter() {
                let remapped: Vec<usize> = dests
                    .iter()
                    .filter_map(|d| permutation.get(d).copied())
                    .collect();
                if !remapped.is_empty() {
                    pruned.insert(sym, remapped);
                }
            }
            *row = pruned;
        }

        self.q0 = permutation[&self.q0];
        self.q = new_q;
        self.o = new_o;
        self.d = new_d;
    }

    /// BFS assigns each state reachable from `q0` a dense id in visit order (Walnut's
    /// `FA.determinePermutationMap`, `FA.java:196-214`). `HashMap` is safe here
    /// (`PORTING.md`'s iteration-order rule notwithstanding): the numbering is fully
    /// determined by BFS visit order (queue-pop order, each state's `BTreeMap`-sorted
    /// transition table, and each destination list's insertion order), and this map is
    /// never iterated for that numbering — only looked up by key (`get`/vacancy check)
    /// — so no Rust-`HashMap`-seed-per-process nondeterminism can leak into the
    /// result. (Matches `trim.rs`'s `old_to_new`, which makes the identical judgment
    /// call for the same reason.)
    fn determine_permutation_map(&self) -> HashMap<usize, usize> {
        let mut queue: VecDeque<usize> = VecDeque::new();
        queue.push_back(self.q0);
        let mut permutation: HashMap<usize, usize> = HashMap::new();
        permutation.insert(self.q0, 0);
        let mut next_id = 1usize;
        while let Some(q) = queue.pop_front() {
            for dests in self.d[q].values() {
                for &p in dests {
                    if let std::collections::hash_map::Entry::Vacant(e) = permutation.entry(p) {
                        e.insert(next_id);
                        next_id += 1;
                        queue.push_back(p);
                    }
                }
            }
        }
        permutation
    }

    /// `FA.setOutputIfEqual(int idx, boolean output)` (`FA.java:411-413`).
    pub fn set_output_if_equal(&mut self, idx: usize, output: bool) {
        self.o[idx] = i32::from(output);
    }

    /// `FA.setOutputIfEqual(int output)` (`FA.java:414-418`) — collapses every state's
    /// output to `1` if it equals `output`, else `0`. Distinct Rust name from
    /// [`Fa::set_output_if_equal`] because Rust has no method overloading; Java
    /// disambiguates the two purely by parameter count.
    pub fn restrict_output_to(&mut self, output: i32) {
        for j in 0..self.o.len() {
            self.set_output_if_equal(j, self.o[j] == output);
        }
    }

    /// `FA.determineMinOutput()` (`FA.java:534-545`). Panics (matching Java's
    /// `WalnutException.alphabetIsEmpty()`, text `"Output alphabet is empty"` — a
    /// pre-existing Walnut naming quirk: the guarded condition is really "no states",
    /// not "no alphabet symbols", ported verbatim including the misleading factory
    /// name) if there are no states to take a minimum over.
    pub fn determine_min_output(&self) -> i32 {
        assert!(!self.o.is_empty(), "Output alphabet is empty");
        self.o.iter().copied().min().unwrap()
    }

    /// `FA.addDistinguishedDeadState()` (`FA.java:244-266`) — adds a dead state whose
    /// output is one less than the automaton's current minimum output, totalizing in
    /// the process. Returns whether a dead state was actually added (`false` iff the
    /// automaton was already total in Java's relaxed, key-presence-only sense —
    /// [`Fa::totalize_relaxed`]'s doc comment explains why that's a different, weaker
    /// notion than [`Fa::is_deterministic_and_total`]).
    ///
    /// # Why not [`Fa::totalize`]
    ///
    /// An earlier version of this method used exactly the `is_deterministic_and_total`
    /// pre-check + [`Fa::totalize`] shape suggested by the two methods' names lining up
    /// so neatly. That is a real bug (caught in adversarial review of this unit): Java's
    /// `addDistinguishedDeadState` calls `ensureNfaTransitions()` then
    /// `totalizeStates(this.Q)` (`FA.java:244-266`, `:336-344`) — the same
    /// key-presence-only, determinism-agnostic pass `logicalops::totalize` already had
    /// to replicate for the same reason (see that function's doc comment) — never
    /// `FA.totalize()`'s DFA-only fast path. So an `Fa` where every `(state, symbol)`
    /// pair has SOME destination list, but one of those lists has 2+ entries (an
    /// NFA-shaped-but-symbol-total automaton), is "already totalized" to Java: no dead
    /// state is added, no exception is thrown. The old
    /// `is_deterministic_and_total`-gated version instead correctly noticed that case
    /// was NOT total-with-determinism, fell through to `self.totalize(min - 1)`, and hit
    /// that method's own `assert!(self.is_deterministic())` — a panic Java never
    /// produces on this input. [`Fa::totalize_relaxed`] is the shared, assert-free fix,
    /// used identically by `logicalops::totalize`.
    pub fn add_distinguished_dead_state(&mut self) -> bool {
        let missing_some_transition = (0..self.q)
            .any(|q| (0..self.alphabet_size as i32).any(|sym| !self.d[q].contains_key(&sym)));
        if !missing_some_transition {
            return false;
        }
        let min = self.determine_min_output();
        self.totalize_relaxed(min - 1);
        true
    }

    /// Assert-free, key-presence-only totalization shared by
    /// [`Fa::add_distinguished_dead_state`] and `logicalops::totalize`. Ports
    /// `FA.totalizeStates`/`FA.addMissingTransitionsForState`/`FA.addSinkState`
    /// (`FA.java:336-367`, `:315-321`) — the NFA-transitions half of Java's
    /// `totalize()` dispatch, which both real call sites actually hit
    /// (`addDistinguishedDeadState` always calls `ensureNfaTransitions()` first, and
    /// `logicalops::totalize_cross_product`'s operands are plain `Automaton`s that may
    /// be genuinely nondeterministic).
    ///
    /// Unlike [`Fa::totalize`], this makes NO determinism assumption at all: it only
    /// asks "does this `(state, symbol)` pair have SOME destination list", exactly
    /// mirroring `addMissingTransitionsForState`'s `!iMap.containsKey(x)` check, which
    /// is oblivious to how many destinations that list holds. So an automaton with a
    /// present-but-multi-destination entry for every `(state, symbol)` pair is already
    /// "totalized" by this definition and gets no dead state — the case
    /// [`Fa::is_deterministic_and_total`] (which additionally requires
    /// `dests.len() == 1`) would call not-total, and [`Fa::totalize`] would panic on it
    /// via its own `is_deterministic()` assert instead.
    ///
    /// Routes every missing pair to a fresh sink state (index `self.q` at entry, output
    /// `sink_output`, self-looping on every symbol) — but only appends that sink if at
    /// least one transition was actually missing, matching Java's
    /// `if (!totalizeStates(sinkState)) addSinkState(...)` guard. Returns whether a sink
    /// was added.
    pub(crate) fn totalize_relaxed(&mut self, sink_output: i32) -> bool {
        let sink_state = self.q;
        let mut needs_sink = false;
        for q in 0..self.q {
            for sym in 0..self.alphabet_size as i32 {
                if let std::collections::btree_map::Entry::Vacant(entry) = self.d[q].entry(sym) {
                    entry.insert(vec![sink_state]);
                    needs_sink = true;
                }
            }
        }
        if needs_sink {
            self.o.push(sink_output);
            self.q += 1;
            let mut sink_row = BTreeMap::new();
            for sym in 0..self.alphabet_size as i32 {
                sink_row.insert(sym, vec![sink_state]);
            }
            self.d.push(sink_row);
        }
        needs_sink
    }

    /// `FA.setFields(int newStates, IntList newO, List<Int2ObjectRBTreeMap<IntList>> newD)`
    /// (`FA.java:547-551`). Note this does **not** touch `q0` — faithfully: see
    /// `WordAutomaton::reverse_with_output`'s doc comment (WB-016) for the one call
    /// site where that omission is a genuine Walnut bug, ported verbatim.
    pub fn set_fields(
        &mut self,
        new_q: usize,
        new_o: Vec<i32>,
        new_d: Vec<BTreeMap<i32, Vec<usize>>>,
    ) {
        self.q = new_q;
        self.o = new_o;
        self.d = new_d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 2-state total DFA over a 2-symbol alphabet accepting words containing at
    /// least one `1` (symbol 1); mirrors the hand-derived spike result shape.
    fn contains_one_dfa() -> Fa {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
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
    fn accepting_and_empty() {
        let fa = contains_one_dfa();
        assert!(fa.is_accepting(1));
        assert!(!fa.is_accepting(0));
        assert!(!fa.is_language_empty());
    }

    #[test]
    fn language_empty_no_accepting_state() {
        let mut fa = contains_one_dfa();
        fa.o = vec![0, 0];
        assert!(fa.is_language_empty());
    }

    #[test]
    fn zero_state_automaton_is_empty() {
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 0,
            alphabet_size: 2,
            o: vec![],
            d: vec![],
        };
        assert!(fa.is_language_empty());
    }

    #[test]
    fn accepts_word_matches_expected_language() {
        let fa = contains_one_dfa();
        assert!(!fa.accepts_word(&[0, 0, 0]));
        assert!(fa.accepts_word(&[0, 1, 0]));
        assert!(fa.accepts_word(&[1]));
        assert!(!fa.accepts_word(&[]));
    }

    #[test]
    fn is_deterministic_and_total_true_for_dfa() {
        let fa = contains_one_dfa();
        assert!(fa.is_deterministic());
        assert!(fa.is_deterministic_and_total());
    }

    #[test]
    fn is_deterministic_and_total_false_when_symbol_missing() {
        let mut fa = contains_one_dfa();
        fa.d[0].remove(&1);
        assert!(fa.is_deterministic()); // still <=1 dest per present symbol
        assert!(!fa.is_deterministic_and_total()); // but not total anymore
    }

    #[test]
    fn is_deterministic_false_for_nfa() {
        let mut fa = contains_one_dfa();
        fa.d[0].insert(1, vec![0, 1]); // two destinations for symbol 1
        assert!(!fa.is_deterministic());
    }

    #[test]
    fn totalize_fills_missing_transitions_with_a_sink() {
        let mut fa = contains_one_dfa();
        fa.d[0].remove(&1);
        assert!(!fa.is_deterministic_and_total());
        fa.totalize(0);
        assert!(fa.is_deterministic_and_total());
        assert_eq!(fa.q, 3);
        // The formerly-missing transition now routes to the sink, which self-loops
        // and is non-accepting — language is unchanged for words that never hit it,
        // and the old symbol-1-from-state-0 input now correctly leads to rejection.
        assert!(!fa.accepts_word(&[0]));
    }

    #[test]
    fn totalize_is_noop_when_already_total() {
        let mut fa = contains_one_dfa();
        fa.totalize(0);
        assert_eq!(fa.q, 2, "already-total automaton should not grow");
    }

    // --- add_transition ---

    #[test]
    fn add_transition_appends_to_existing_and_creates_new_entries() {
        let mut table = vec![BTreeMap::new(), BTreeMap::new()];
        Fa::add_transition(&mut table, 0, 5, 2);
        Fa::add_transition(&mut table, 0, 5, 3);
        assert_eq!(table[0].get(&5), Some(&vec![2, 3]));
        assert!(table[1].is_empty());
    }

    // --- reverse (FA.java:274-303) ---

    #[test]
    fn reverse_reverses_transitions_and_swaps_initial_and_accepting() {
        let mut fa = contains_one_dfa();
        let old_initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let new_initial = fa.reverse(&old_initial);

        // State 1 was the only accepting state before the call.
        assert_eq!(new_initial, [1].into_iter().collect::<BTreeSet<_>>());
        // old_initial_states (={0}) are now accepting; old accepting states (={1}) are not.
        assert_eq!(fa.o, vec![1, 0]);
        // Original: 0--0-->0, 0--1-->1, 1--0-->1, 1--1-->1.
        // Reversed: 0--0-->0, 1--1-->0, 1--0-->1, 1--1-->1 (merged with the previous line).
        assert_eq!(fa.d[0].get(&0), Some(&vec![0]));
        assert_eq!(fa.d[0].get(&1), None);
        assert_eq!(fa.d[1].get(&0), Some(&vec![1]));
        assert_eq!(fa.d[1].get(&1), Some(&vec![0, 1]));
    }

    #[test]
    fn reverse_of_reachability_only_dfa_is_the_reverse_language() {
        // "ends with symbol 1" over {0,1}.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![0]);
        d1.insert(1, vec![1]);
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, d1],
        };
        let old_initial: BTreeSet<usize> = [fa.q0].into_iter().collect();
        let new_initial = fa.reverse(&old_initial);
        let reversed_dfa = crate::determinize::subset_construction(&fa, &new_initial);

        // Reversing "ends with 1" gives "starts with 1".
        fn starts_with_one(word: &[i32]) -> bool {
            matches!(word.first(), Some(1))
        }
        for word in [
            vec![],
            vec![1],
            vec![0],
            vec![1, 0, 0],
            vec![0, 1, 1],
            vec![1, 1, 1],
        ] {
            assert_eq!(
                reversed_dfa.accepts_word(&word),
                starts_with_one(&word),
                "mismatch on {word:?}"
            );
        }
    }

    /// Generates a total DFA (`q0` fixed at `0`) over a FIXED `alphabet_size` — fixed,
    /// not randomized, so that a correlated random word (drawn from `0..alphabet_size`
    /// at the call site) always lands on real symbols. An earlier version of this
    /// generator randomized `alphabet_size` too while callers still drew words from a
    /// hardcoded `0..2` range, so on every `alphabet_size == 1` case the word strategy
    /// could draw symbol `1`, which both sides of any comparison reject vacuously —
    /// contributing no signal. `determinize.rs`'s `arb_nfa_fixed_alphabet` hit and
    /// documented this identical pitfall first; this generator now follows the same
    /// fixed-alphabet rule.
    fn arb_total_dfa(q_max: usize, alphabet_size: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max).prop_flat_map(move |q| {
            let o_strategy = prop::collection::vec(0i32..=1, q);
            let trans_strategy =
                prop::collection::vec(prop::collection::vec(0usize..q, alphabet_size), q);
            (o_strategy, trans_strategy).prop_map(move |(o, trans)| {
                let d = trans
                    .into_iter()
                    .map(|r| {
                        r.into_iter()
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

    /// Generates a genuine NFA (nondeterminism + partial transitions allowed) with a
    /// RANDOM `q0` over a fixed `alphabet_size` — the shape `reverse`/`star_states`/
    /// `concat_states`/`canonicalize` actually operate on, unlike [`arb_total_dfa`]'s
    /// always-`q0`-0 total DFAs. Mirrors `determinize.rs`'s `arb_nfa_fixed_alphabet`
    /// (same per-symbol-subset-of-destinations shape) plus a randomized `q0`, needed
    /// here because several of this unit's reviewed gaps were specifically about
    /// `q0 != 0`, `q0` accepting, and `q0` with incoming edges never being generated.
    fn arb_nfa_with_q0(q_max: usize, alphabet_size: usize) -> impl Strategy<Value = Fa> {
        (1..=q_max).prop_flat_map(move |q| {
            let row_strategy =
                prop::collection::vec(prop::collection::vec(any::<bool>(), q), alphabet_size);
            let table_strategy = prop::collection::vec(row_strategy, q);
            let o_strategy = prop::collection::vec(0i32..=1, q);
            let q0_strategy = 0..q;
            (table_strategy, o_strategy, q0_strategy).prop_map(move |(table, o, q0)| {
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
                    q0,
                    q,
                    alphabet_size,
                    o,
                    d,
                }
            })
        })
    }

    /// `word ∈ L(A)*` via a naive O(n^2)-`accepts_word`-calls DP split-check — cheap
    /// enough for the tiny (`<=5`-symbol) words these proptests generate, and gives an
    /// independent ground truth for `star_states` that doesn't reuse any of its own
    /// machinery.
    fn star_accepts(a: &Fa, word: &[i32]) -> bool {
        let n = word.len();
        let mut dp = vec![false; n + 1];
        dp[0] = true;
        for i in 1..=n {
            for j in 0..i {
                if dp[j] && a.accepts_word(&word[j..i]) {
                    dp[i] = true;
                    break;
                }
            }
        }
        dp[n]
    }

    /// Ground truth for `concat_states`'s ACTUAL (quirky, see its doc comment)
    /// behavior: `L(a) ∪ L(a)·L(b_from_index_0)`, i.e. `a`'s language leaks through
    /// whole (WB-009) and the continuation is spliced from `b`'s state index `0`, not
    /// `b.q0` (WB-008) — `b_index0` must already have `q0` forced to `0` by the
    /// caller. This deliberately encodes the two ported bugs, not the documented
    /// "correct" concatenation semantics — the point of this test is to pin the
    /// quirks so a future refactor can't silently "fix" them without a test noticing.
    fn concat_quirk_accepts(a: &Fa, b_index0: &Fa, word: &[i32]) -> bool {
        if a.accepts_word(word) {
            return true;
        }
        (0..=word.len()).any(|i| a.accepts_word(&word[..i]) && b_index0.accepts_word(&word[i..]))
    }

    /// Hand-derived multi-state-seed case for `reverse` (adversarial-review finding:
    /// every OTHER test in this file only ever seeds `reverse` with `{self.q0}`, which
    /// cannot distinguish the `old_initial_states` loop from `self.o[self.q0] = 1`).
    /// `q=3`, `0--0-->1--0-->2`, only state 2 accepting, seeded with `{0, 1}`.
    #[test]
    fn reverse_with_a_genuine_multi_state_seed() {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 1,
            o: vec![0, 0, 1],
            d: vec![d0, d1, BTreeMap::new()],
        };
        let old_initial: BTreeSet<usize> = [0, 1].into_iter().collect();
        let new_initial = fa.reverse(&old_initial);

        assert_eq!(new_initial, [2].into_iter().collect::<BTreeSet<_>>());
        assert_eq!(fa.o, vec![1, 1, 0], "both seed states are now accepting");

        let dfa = crate::determinize::subset_construction(&fa, &new_initial);
        assert!(dfa.accepts_word(&[0]), "0 must be accepted");
        assert!(dfa.accepts_word(&[0, 0]), "00 must be accepted");
        assert!(!dfa.accepts_word(&[]), "empty word must be rejected");
        assert!(
            !dfa.accepts_word(&[0, 0, 0]),
            "000 must be rejected (dead end)"
        );
    }

    proptest! {
        /// Tier-4 sanity check named directly in the unit brief: reversing twice (with
        /// a re-determinize in between, since `reverse` itself only ever produces an
        /// NFA-shaped result) returns to the original language. This is the same
        /// "Brzozowski double-reversal" property `CLAUDE.md`'s correctness ladder
        /// names as a Tier-4 invariant, minus the minimization step (not needed to
        /// observe language equality via `accepts_word`).
        #[test]
        fn double_reverse_preserves_language(
            fa in arb_total_dfa(5, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let mut once = fa.clone();
            let seed1: BTreeSet<usize> = [once.q0].into_iter().collect();
            let new_initial1 = once.reverse(&seed1);
            let mut twice = crate::determinize::subset_construction(&once, &new_initial1);

            let seed2: BTreeSet<usize> = [twice.q0].into_iter().collect();
            let new_initial2 = twice.reverse(&seed2);
            let thrice = crate::determinize::subset_construction(&twice, &new_initial2);

            prop_assert_eq!(fa.accepts_word(&word), thrice.accepts_word(&word));
        }

        /// The DIRECT reversal property (adversarial-review finding: double-reversal
        /// alone is self-cancelling and can survive a symmetric fault in `reverse`) —
        /// `L(reverse(A)) == reverse(L(A))` — checked over genuine NFAs (nondeterminism,
        /// partial transition tables, random `q0`), not just `arb_total_dfa`'s total
        /// DFAs, since `reverse`'s real inputs are exactly those shapes.
        #[test]
        fn reverse_computes_the_reversed_language(
            fa in arb_nfa_with_q0(5, 3),
            word in prop::collection::vec(0i32..3, 0..5),
        ) {
            let mut reversed_fa = fa.clone();
            let seed: BTreeSet<usize> = [fa.q0].into_iter().collect();
            let new_initial = reversed_fa.reverse(&seed);
            let reversed_dfa = crate::determinize::subset_construction(&reversed_fa, &new_initial);

            let reversed_word: Vec<i32> = word.iter().rev().copied().collect();
            prop_assert_eq!(fa.accepts_word(&word), reversed_dfa.accepts_word(&reversed_word));
        }
    }

    // --- star_states / concat_states (FA.java:97-124) ---

    #[test]
    fn star_states_builds_kleene_star_nfa() {
        // A accepts exactly the single word "1" (symbol 1): q0=0 --1--> 1 (accepting,
        // dead end).
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![1]);
        let a = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, BTreeMap::new()],
        };
        let mut n = a.clone();
        Fa::star_states(&a, &mut n);

        assert_eq!(n.q0, 2);
        assert_eq!(n.q, 3);
        assert!(n.is_accepting(n.q0), "star always accepts the empty word");
        for word in [vec![], vec![1], vec![1, 1], vec![1, 1, 1]] {
            assert!(n.accepts_word(&word), "1* must accept {word:?}");
        }
        for word in [vec![0], vec![1, 0], vec![0, 1]] {
            assert!(!n.accepts_word(&word), "1* must reject {word:?}");
        }
    }

    #[test]
    fn concat_states_builds_concatenation_nfa() {
        // "first" accepts exactly {"0"}; "other" accepts exactly {eps, "1"} (so
        // eps is in L(other), which happens to mask the leaked-language quirk
        // documented on `Fa::concat_states` and pinned below).
        let mut d_first0 = BTreeMap::new();
        d_first0.insert(0, vec![1]);
        let first = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d_first0, BTreeMap::new()],
        };
        let mut d_other0 = BTreeMap::new();
        d_other0.insert(1, vec![1]);
        let other = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 1], // q0 accepting: eps is in L(other)
            d: vec![d_other0, BTreeMap::new()],
        };
        let original_q = first.q;
        let mut n = first.clone();
        Fa::concat_states(&other, &mut n, original_q);

        assert_eq!(n.q, 4);
        assert_eq!(
            n.q0, 0,
            "concat_states never touches q0 -- inherited from the `first` clone"
        );
        for word in [vec![0], vec![0, 1]] {
            assert!(
                n.accepts_word(&word),
                "{word:?} should be in {{0}}.{{eps,1}} = {{0, 01}}"
            );
        }
        for word in [vec![], vec![1], vec![1, 0], vec![0, 0]] {
            assert!(!n.accepts_word(&word), "{word:?} should be rejected");
        }
    }

    #[test]
    fn concat_states_quirk_leaks_first_operands_language_when_second_lacks_epsilon() {
        // Ported Walnut bug (candidate for docs/WALNUT-BUGS.md): `concatStates`
        // reuses `mergeInTransitions`, which is correct for `starStates` (old final
        // states of a starred automaton SHOULD remain accepting) but never clears
        // `first`'s old accepting flags for concatenation. So whenever eps is NOT in
        // the second operand's language, the raw concatenation-NFA's language is
        // `L(first) union L(first).L(other)` instead of the documented
        // `L(first).L(other)` -- L(first) leaks through, unminimized/undeterminized.
        // Ported verbatim per CLAUDE.md's mechanical-port rule.
        let mut d_first0 = BTreeMap::new();
        d_first0.insert(0, vec![1]);
        let first = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d_first0, BTreeMap::new()],
        };
        let mut d_other0 = BTreeMap::new();
        d_other0.insert(1, vec![1]);
        let other = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1], // q0 NOT accepting: eps is NOT in L(other)
            d: vec![d_other0, BTreeMap::new()],
        };
        let original_q = first.q;
        let mut n = first.clone();
        Fa::concat_states(&other, &mut n, original_q);

        // Correct concatenation {"0"}.{"1"} = {"01"} only:
        assert!(n.accepts_word(&[0, 1]), "01 must be accepted");
        // But the bare "0" is ALSO accepted -- documents the quirk, not the intended
        // semantics ("first"'s own old-final-state acceptance was never cleared).
        assert!(
            n.accepts_word(&[0]),
            "documents FA.concatStates' leaked-first-language quirk, see module docs"
        );
    }

    #[test]
    fn concat_states_quirk_uses_others_state_zero_not_others_q0() {
        // Ported Walnut bug (candidate for docs/WALNUT-BUGS.md): `FA.concatStates`
        // (FA.java:107-124) splices in `N.t.getEntriesNfaD(originalQ)` -- i.e.
        // `other`'s state INDEX 0 shifted into `n` -- as the "continue into `other`"
        // transitions for each of `first`'s old final states, rather than `other`'s
        // actual initial state `other.q0`. This is silently correct only when
        // `other.q0 == 0`, which is NOT guaranteed in general -- confirmed reachable
        // via a hand-authored `.txt` file, since `AutomatonReader` sets `q0` to
        // whichever state is declared first in the file, not necessarily state 0 (see
        // this method's doc comment / docs/WALNUT-BUGS.md WB-008). Contrast
        // `star_states`, which correctly uses `automaton.q0` (see
        // `star_states_builds_kleene_star_nfa` above).
        //
        // `first` accepts exactly {"0"}. `other` (q0 = 1, NOT 0) accepts `0.1*`:
        // q0=1 --0--> state 0 (accepting), and state 0 self-loops on symbol 1 while
        // remaining accepting -- so state 0 is very much reachable, not a distractor.
        // The mathematically correct concatenation is {"0"}.(0.1*) = "00", "001", ...
        // -- in particular "00" must be accepted and "01" must NOT be. The ported bug
        // instead splices state-INDEX-0's transitions (symbol 1, self-loop) onto
        // `first`'s final state, so it's exactly backwards: "00" is rejected and "01"
        // is accepted.
        let mut d_first0 = BTreeMap::new();
        d_first0.insert(0, vec![1]);
        let first = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d_first0, BTreeMap::new()],
        };
        let mut d_other0 = BTreeMap::new();
        d_other0.insert(1, vec![0]); // distractor: state 0 --1--> state 0, unreachable from q0=1
        let mut d_other1 = BTreeMap::new();
        d_other1.insert(0, vec![0]); // real: q0=1 --0--> state 0 (accepting)
        let other = Fa {
            true_false: None,
            q0: 1,
            q: 2,
            alphabet_size: 2,
            o: vec![1, 0],
            d: vec![d_other0, d_other1],
        };
        assert!(other.accepts_word(&[0]), "sanity: L(other) = 0.1*");
        assert!(!other.accepts_word(&[1]), "sanity: L(other) = 0.1*");
        assert!(
            other.accepts_word(&[0, 1]),
            "sanity: L(other) = 0.1* (not just {{\"0\"}})"
        );
        assert!(
            other.accepts_word(&[0, 1, 1]),
            "sanity: L(other) = 0.1* (not just {{\"0\"}})"
        );

        let original_q = first.q;
        let mut n = first.clone();
        Fa::concat_states(&other, &mut n, original_q);

        // Documents the quirk: the mathematically correct "00" is rejected...
        assert!(
            !n.accepts_word(&[0, 0]),
            "documents the quirk: the correct concatenation string is dropped"
        );
        // ...while "01" (never in L(first).(0.1*)) is incorrectly accepted, because
        // state-INDEX-0's symbol-1 self-loop was spliced in instead of q0's real
        // symbol-0 transition.
        assert!(
            n.accepts_word(&[0, 1]),
            "documents the quirk: a wrong string is accepted instead"
        );
    }

    proptest! {
        /// Pins `star_states`' actual language against an independent ground truth
        /// (adversarial-review finding: only one hand-built shape was previously
        /// exercised, and `automaton.q0` accepting / having incoming edges was never
        /// generated). A future refactor that breaks the Kleene-star construction
        /// while every hand-written test still passes would be caught here.
        #[test]
        fn star_states_computes_kleene_star_over_random_nfas(
            a in arb_nfa_with_q0(4, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let mut n = a.clone();
            Fa::star_states(&a, &mut n);
            prop_assert_eq!(n.accepts_word(&word), star_accepts(&a, &word));
        }

        /// Pins `concat_states`' ACTUAL (quirky) language — `L(a) ∪ L(a)·L(b)` with
        /// `b` read from state INDEX 0, not `b.q0` — against an independent ground
        /// truth (`concat_quirk_accepts`), over random NFAs including `q0 != 0` on
        /// both operands. This is deliberately NOT a "concat is correct" test (it
        /// isn't, per WB-008/WB-009); it exists so a future refactor that
        /// half-fixes one of the two documented bugs without updating both this test
        /// AND the doc comment gets caught immediately.
        #[test]
        fn concat_states_matches_the_documented_quirks_exactly(
            a in arb_nfa_with_q0(4, 2),
            b in arb_nfa_with_q0(4, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let original_q = a.q;
            let mut n = a.clone();
            Fa::concat_states(&b, &mut n, original_q);

            let mut b_index0 = b.clone();
            b_index0.q0 = 0;
            prop_assert_eq!(n.accepts_word(&word), concat_quirk_accepts(&a, &b_index0, &word));
        }
    }

    // --- canonicalize (FA.java:148-214) ---

    #[test]
    fn canonicalize_prunes_entries_with_empty_destination_list() {
        // Mirrors FATest.canonizeInternal_prunesEntriesWithEmptyDestinationList:
        // state 1 has a transition on symbol 0 to an EMPTY destination list
        // (constructible only via direct raw-`Fa` manipulation). Since there is
        // nothing to remap, the entry must be pruned entirely rather than kept as an
        // empty list.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]); // state 0 --0--> state 1 (real transition)
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![]); // state 1 --0--> {} (empty destination list)
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![d0, d1],
        };

        fa.canonicalize();

        assert_eq!(fa.q0, 0);
        assert_eq!(fa.q, 2);
        assert_eq!(fa.o, vec![0, 1]);
        assert_eq!(fa.d[0].get(&0), Some(&vec![1]));
        assert!(
            fa.d[1].is_empty(),
            "the empty-destination entry was pruned entirely"
        );
    }

    #[test]
    fn canonicalize_drops_forward_unreachable_states_and_bfs_renumbers() {
        // 3 states: q0=1 <-> 2 (a 2-cycle); state 0 is unreachable from q0 even though
        // it has an outgoing transition of its own.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        let mut d2 = BTreeMap::new();
        d2.insert(0, vec![1]);
        let mut fa = Fa {
            true_false: None,
            q0: 1,
            q: 3,
            alphabet_size: 1,
            o: vec![0, 0, 1],
            d: vec![d0, d1, d2],
        };

        fa.canonicalize();

        assert_eq!(fa.q0, 0, "the BFS root always becomes state 0");
        assert_eq!(fa.q, 2, "the unreachable state was dropped");
        assert!(fa.accepts_word(&[0]), "old q0(=1) --0--> old 2 (accepting)");
    }

    #[test]
    fn canonicalize_is_a_noop_on_a_zero_state_automaton() {
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 0,
            alphabet_size: 2,
            o: vec![],
            d: vec![],
        };
        fa.canonicalize();
        assert_eq!(fa.q, 0);
    }

    // --- isFAO (U0c, FA.java:64-71) ---

    #[test]
    fn is_fao_is_true_exactly_when_some_output_exceeds_one() {
        // A plain predicate automaton's outputs are 0/1 -- not a DFAO.
        let mut fa = contains_one_dfa();
        assert!(!fa.is_fao());

        // A word automaton's are arbitrary non-negative ints. `2` is the smallest
        // value Java's `O.getInt(i) > 1` test accepts, so it pins the boundary.
        fa.o = vec![0, 2];
        assert!(fa.is_fao());

        // Boundary the other way: output exactly 1 is accepting, but NOT a DFAO.
        fa.o = vec![1, 1];
        assert!(!fa.is_fao());
    }

    #[test]
    fn is_fao_is_false_for_a_trivial_automaton() {
        // No outputs at all -> the `any` is vacuously false. (Java would agree here:
        // `Q == 0` for a freshly-built trivial automaton, so its loop never runs.)
        assert!(!Fa::trivial(true).is_fao());
        assert!(!Fa::trivial(false).is_fao());
    }

    // --- the trivial (TRUE/FALSE) automaton (U0, FA.java:52-53/90-94/149/593-617) ---

    #[test]
    fn trivial_constructs_a_stateless_flagged_automaton() {
        let t = Fa::trivial(true);
        assert!(t.is_true_false_automaton());
        assert!(t.is_true_automaton());
        assert_eq!(t.true_false_string(), "true");
        assert_eq!((t.q, t.q0, t.alphabet_size), (0, 0, 0));
        assert!(t.o.is_empty() && t.d.is_empty());

        let f = Fa::trivial(false);
        assert!(f.is_true_false_automaton());
        assert!(!f.is_true_automaton());
        assert_eq!(f.true_false_string(), "false");
    }

    #[test]
    fn an_ordinary_automaton_is_not_a_trivial_one() {
        let fa = contains_one_dfa();
        assert!(!fa.is_true_false_automaton());
        assert!(!fa.is_true_automaton());
    }

    #[test]
    fn clone_carries_the_trivial_flag() {
        // `FA.clone()` explicitly copies BOTH flags (`FA.java:450-451`); `derive(Clone)`
        // must do the same or every `Automaton::clone`-based call path silently
        // downgrades a trivial automaton to an ordinary 0-state one.
        for truth in [true, false] {
            let c = Fa::trivial(truth).clone();
            assert!(c.is_true_false_automaton());
            assert_eq!(c.is_true_automaton(), truth);
        }
    }

    #[test]
    fn clear_empties_storage_but_faithfully_leaves_q_stale_and_the_flag_set() {
        // Replicates the exact sequence `AutomatonQuantification.quantifyHelper:61-63`
        // produces: flags set, then `FA.clear()`. Java leaves `Q` untouched, and that
        // stale value is what every downstream guard has to tolerate.
        let mut fa = contains_one_dfa();
        fa.true_false = Some(true);
        fa.clear();

        assert_eq!(fa.q, 2, "Q is deliberately NOT reset -- matches FA.clear()");
        assert!(fa.o.is_empty());
        assert!(fa.d.is_empty());
        assert!(fa.is_true_false_automaton() && fa.is_true_automaton());
    }

    #[test]
    fn canonicalize_short_circuits_on_a_trivial_automaton_with_a_stale_q() {
        // The `q == 0 || d.is_empty()` guard would also catch this shape, but the
        // TRUE/FALSE guard is what Java uses (`FA.java:149`) and what must fire first;
        // the assertion here is that nothing panics and nothing is renumbered.
        let mut fa = contains_one_dfa();
        fa.true_false = Some(false);
        fa.clear();
        fa.canonicalize();
        assert_eq!(fa.q, 2);
        assert!(fa.is_true_false_automaton() && !fa.is_true_automaton());
    }

    #[test]
    fn set_output_if_equal_sets_only_the_given_index() {
        let mut fa = contains_one_dfa();
        fa.set_output_if_equal(0, true);
        assert_eq!(fa.o, vec![1, 1]);
        fa.set_output_if_equal(0, false);
        assert_eq!(fa.o, vec![0, 1]);
    }

    #[test]
    fn restrict_output_to_collapses_every_state_to_a_boolean() {
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 3,
            alphabet_size: 1,
            o: vec![3, 7, 3],
            d: vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()],
        };
        fa.restrict_output_to(3);
        assert_eq!(fa.o, vec![1, 0, 1]);
    }

    #[test]
    fn determine_min_output_finds_the_minimum() {
        let mut fa = contains_one_dfa();
        fa.o = vec![5, -2];
        assert_eq!(fa.determine_min_output(), -2);
    }

    #[test]
    #[should_panic(expected = "Output alphabet is empty")]
    fn determine_min_output_panics_on_no_states() {
        let fa = Fa {
            true_false: None,
            q0: 0,
            q: 0,
            alphabet_size: 2,
            o: Vec::new(),
            d: Vec::new(),
        };
        fa.determine_min_output();
    }

    #[test]
    fn add_distinguished_dead_state_no_op_when_already_total() {
        let mut fa = contains_one_dfa(); // already deterministic and total
        let added = fa.add_distinguished_dead_state();
        assert!(!added);
        assert_eq!(fa.q, 2, "no state added");
    }

    #[test]
    fn add_distinguished_dead_state_adds_a_sink_with_output_one_below_the_minimum() {
        // Partial automaton (state 0 has no transition for symbol 1), min output 0.
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size: 2,
            o: vec![0],
            d: vec![BTreeMap::from([(0, vec![0])])],
        };
        let added = fa.add_distinguished_dead_state();
        assert!(added);
        assert_eq!(fa.q, 2);
        assert_eq!(fa.o[1], -1, "one less than the pre-add minimum output (0)");
        assert!(fa.is_deterministic_and_total());
    }

    #[test]
    fn add_distinguished_dead_state_no_op_on_nfa_shaped_but_symbol_total_automaton() {
        // Pins the adversarial-review finding on this unit: an earlier version of
        // `add_distinguished_dead_state` gated on `is_deterministic_and_total`, then
        // fell through to the assert-requiring `Fa::totalize` -- so an automaton where
        // EVERY (state, symbol) pair has SOME destination list, but one pair has TWO
        // destinations (genuinely nondeterministic, yet still "total" by key
        // presence), made `is_deterministic_and_total` return false and then panicked
        // inside `totalize`'s `assert!(self.is_deterministic())`.
        //
        // Real `FA.addDistinguishedDeadState` (`FA.java:244-266`) never asserts
        // determinism at all -- it calls `ensureNfaTransitions()` then
        // `totalizeStates(this.Q)`, whose only test is `!iMap.containsKey(x)`
        // (`FA.java:356-367`), oblivious to how many destinations are already there.
        // So Java treats this exact shape as already-totalized: no dead state added,
        // no exception. This test constructs that scenario directly and checks the
        // Rust port now matches -- no panic, `add_distinguished_dead_state` returns
        // `false`, and the automaton is left untouched.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0, 1]); // two destinations for symbol 1: present, but nondeterministic
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![1]);
        d1.insert(1, vec![1]);
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![d0, d1],
        };
        assert!(
            !fa.is_deterministic_and_total(),
            "sanity: the strict, determinism-requiring check correctly says NOT total"
        );

        let added = fa.add_distinguished_dead_state();

        assert!(
            !added,
            "Java's key-presence-only totality check says this is already totalized"
        );
        assert_eq!(fa.q, 2, "no state added");
        assert_eq!(
            fa.d[0].get(&1),
            Some(&vec![0, 1]),
            "the existing nondeterministic entry is left alone, not pruned or panicked on"
        );
    }

    #[test]
    fn set_fields_replaces_q_o_and_d_but_leaves_q0_untouched() {
        // Pins WB-016 (`docs/WALNUT-BUGS.md`): `set_fields` faithfully does NOT touch
        // `q0`, matching `FA.setFields` exactly -- callers that need a fresh `q0` after
        // a full rebuild (like `word_automaton::reverse_with_output` SHOULD, per the
        // bug entry) must set it themselves; this port doesn't add that call either.
        // Rebuilding down to a single state here leaves the pre-existing `q0 = 1`
        // strictly OUT OF BOUNDS for the new `q = 1` -- exactly the shape of corruption
        // WB-016 documents `reverse_with_output` producing on real (in-bounds, but
        // wrong) inputs.
        let mut fa = contains_one_dfa();
        fa.q0 = 1;
        let new_d = vec![BTreeMap::from([(0, vec![0])])];
        fa.set_fields(1, vec![9], new_d.clone());
        assert_eq!(fa.q, 1);
        assert_eq!(fa.o, vec![9]);
        assert_eq!(fa.d, new_d);
        assert_eq!(
            fa.q0, 1,
            "left stale/out-of-bounds, matching Java's setFields"
        );
    }

    #[test]
    fn canonicalize_short_circuits_even_when_the_flagged_automaton_is_renumberable() {
        // The sharper version of the test above: a fully-formed, renumberable automaton
        // that merely CARRIES the flag must still be left completely untouched, which
        // only the flag guard can achieve (`q`/`d` are both non-degenerate here). This
        // shape is not constructible through this crate's own APIs, but it is the only
        // way to observe the guard independently of the emptiness fallbacks.
        let mut fa = Fa {
            true_false: Some(true),
            q0: 1,
            q: 2,
            alphabet_size: 1,
            o: vec![0, 1],
            d: vec![BTreeMap::new(), BTreeMap::new()],
        };
        let before = format!("{fa:?}");
        fa.canonicalize();
        assert_eq!(
            format!("{fa:?}"),
            before,
            "the TRUE/FALSE guard must suppress BFS renumbering entirely"
        );
    }

    proptest! {
        /// `canonicalize` must not change the language of any state that survives it
        /// (in particular, `q0` always survives, since it is the BFS root) -- checked
        /// via `accepts_word` before vs. after, since exact state identity isn't the
        /// invariant this project cares about (`CLAUDE.md` prime directive #1).
        ///
        /// Uses `arb_nfa_with_q0`, not `arb_total_dfa` (adversarial-review finding: a
        /// total-DFA-only, `q0`-always-`0` generator makes every destination of every
        /// surviving row forward-reachable by construction, so the prune-on-empty-
        /// remap branch at `canonicalize`'s `if !remapped.is_empty()` check is a
        /// guaranteed no-op on every generated case — real nondeterminism/partial
        /// rows/random `q0` are needed to actually exercise dropped-destination
        /// pruning here, on top of the dedicated hand-written unit test above).
        #[test]
        fn canonicalize_preserves_language(
            fa in arb_nfa_with_q0(6, 2),
            word in prop::collection::vec(0i32..2, 0..5),
        ) {
            let before = fa.accepts_word(&word);
            let mut canon = fa;
            canon.canonicalize();
            prop_assert_eq!(before, canon.accepts_word(&word));
        }
    }
}
