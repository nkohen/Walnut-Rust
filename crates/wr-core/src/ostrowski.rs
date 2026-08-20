// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Automata/Numeration/Ostrowski.java` + `Automata/Numeration/NodeState.java` — the
//! Ostrowski-numeration construction behind the `ost` command.
//!
//! Given a quadratic irrational `alpha < 1` described by the pre-period and period of
//! its continued-fraction expansion, this builds two automata:
//!
//! * the **representation automaton** (1 track) — accepts exactly the valid Ostrowski
//!   representations over the digit alphabet `{0, …, dMax}`, msd-first;
//! * the **adder automaton** (3 tracks) — accepts `(x, y, z)` iff `val(x) + val(y) =
//!   val(z)`.
//!
//! Reference: Baranwal, Aseem. *Decision algorithms for Ostrowski-automatic sequences*.
//! MS thesis, University of Waterloo, 2020 (cited by `Ostrowski.java`'s own class doc).
//!
//! # Deliberate divergences from the Java source
//!
//! 1. **`Ostrowski::new` takes already-parsed `&[i32]` slices**, where Java's
//!    `Ostrowski(String, String, String)` parses the two raw strings itself via
//!    `ParseMethods.parseList`. `ParseMethods` lives in `wr-io`, and `wr-core` does not
//!    (and must not) depend on `wr-io` — the same split `crate::morphism` already uses
//!    (`wr_io::parse_methods::parse_morphism` is the caller's job there too). The
//!    parsing now happens in `wr_cli::ost`, which owns the wildcard (`*`) rejection Java
//!    reaches by `NullPointerException`.
//! 2. **`adder_transitions` is `[[Option<(i32, i32)>; 7]; 7]`**, not Java's
//!    `int[7][7][2]` with a `99` (`NONE`) sentinel. Purely an internal representation
//!    choice: the table is never serialized, never compared against Java text, and the
//!    `NONE` checks translate one-to-one into `Option` matching. Java's `if (r == NONE
//!    || s == NONE) continue;` (`Ostrowski.java:283-285`) is still ported as a distinct
//!    `continue`, because that source-level skip is live, frequently-hit code (see
//!    "Both `continue` guards" below).
//! 3. **`node_to_index` is an unordered [`HashMap`]**, where Java uses a `TreeMap`
//!    keyed by `NodeState`'s `Comparable`. Audited against every use site in
//!    `Ostrowski.java`: `nodeToIndex` is only `put`/`containsKey`/`get`/`clear`ed
//!    (`:341, 370, 375, 383, 388, 389, 163`) and `indexToNode` only
//!    `put`/`get`/`clear`ed (`:164, 191, 268, 320, 342, 384`) — neither is ever
//!    iterated (`entrySet`/`keySet`/`values`/for-each appear nowhere for either), so
//!    `NodeState.compareTo` has no observable effect anywhere in the class. Per
//!    `PORTING.md`'s ordered-vs-unordered ruling the unordered map is the faithful
//!    choice; `NodeState` deliberately does NOT implement `Ord`.
//! 4. **`index_to_node` is a `Vec`** rather than a map: Java's keys really are dense
//!    `0..totalNodes` (every `addNodeIndices` call is followed by a `totalNodes++` at
//!    `:313`/`:399`/`:406`, and `initializeBfs` seeds index 0), so `isReprFinal`'s
//!    `node != null` guard (`:154`) is provably dead and is not modeled as an `Option`.
//! 5. **`state_transitions` is a `Vec<BTreeMap<i32, Vec<usize>>>`** — literally
//!    [`Fa::d`]'s own field type, so `Ostrowski::populate_automaton` hands it straight
//!    to the [`Fa`] with no reshaping. Java's `Map<Integer, Int2ObjectRBTreeMap<IntList>>`
//!    is *sparse* during the BFS and is densified by `populateAutomaton`'s blanket
//!    `putIfAbsent(q, …)` sweep (`:192`) — that sweep is load-bearing, not defensive
//!    filler, because plenty of nodes never get a row of their own written during the
//!    BFS (a node's row is created when it is *pointed to*, i.e. as `curNodeIdx`, not
//!    when it is allocated; and any node with `seenIndex == 1` early-`continue`s out of
//!    the dequeue loop before it ever becomes a `curNodeIdx`). This port keeps the
//!    density invariant **continuously** instead: `Ostrowski::add_node_indices` pushes
//!    a fresh empty row for every node it allocates, so
//!    `state_transitions.len() == index_to_node.len()` always, and
//!    `state_transitions.len() == total_nodes` by the time `populate_automaton` reads
//!    `fa.q = total_nodes` off it.
//! 6. **`populate_automaton` ends with an explicit `set_canonized(true)`**, which has no
//!    literal Java counterpart — it *reproduces* one. Java's `FA.canonizeInternal` sets
//!    `this.canonized = true` (`FA.java:190`) and `handleZeroState` then mutates
//!    `nfaD`/`O`/`Q` without resetting it, so `AutomatonWriter.writeTxtFormatToStream`'s
//!    own `automaton.canonize()` (`AutomatonWriter.java:52`) is a no-op and the written
//!    bytes are those of the **un-re-canonized** (but zero-state-adjusted) automaton.
//!    This port's [`Automaton::canonize`] is a narrower opt-in suppression that does not
//!    set the flag itself, and `wr_io::writer::write_txt` calls `canonize()`
//!    unconditionally — so without this call the writer would re-run [`Fa::canonicalize`]
//!    on the post-`handle_zero_state` automaton. That is language-preserving (so
//!    `wr_core::equiv`-based comparisons could never catch it) but can change the
//!    written `.txt` **bytes**, and this command's entire observable output is two
//!    written files.
//!
//! # Both `continue` guards are live code
//!
//! `performAdderBfs` has two `continue`s and neither is dead:
//!
//! * `:271-274`, `if (seenIndex == 1 && alphaSize > 2 && this.periodIndex > 1) continue;`
//!   — fires on **every** `ost` call. Post-construction `preperiod` is non-empty, so
//!   `alphaSize = 1 + |preperiod| + |period| >= 3` and `periodIndex = |preperiod| + 1 >= 2`
//!   are provably always true; the guard is really just `seenIndex == 1`, and both BFS
//!   seed loops create a `seenIndex == 1` node immediately. The always-true conjuncts
//!   are ported verbatim anyway (per `CLAUDE.md`'s mechanical-port rule — "provably
//!   always true" is context for a reviewer, not licence to drop source text).
//! * `:283-285`, `if (r == NONE || s == NONE) continue;` — the per-state-pair "is this
//!   transition even defined" skip, also hit constantly (state 0's row only defines
//!   transitions to states 0 and 1, so every dequeue of state 0 skips states 2-6).
//!   JaCoCo marks this line `nc`, which is a bytecode-instrumentation artifact of how
//!   javac compiles this specific `||` against this specific fixed table, not evidence
//!   the skip does not run.

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::automaton::Automaton;
use crate::fa::Fa;
use crate::logging::Logging;

/// `Ostrowski.NUM_STATES` (`:48`) — the number of states in the 4-input adder.
const NUM_STATES: usize = 7;

/// `Automata.Numeration.NodeState` (`NodeState.java:21`), a Java `record`.
///
/// Field-for-field. `Comparable` is deliberately NOT implemented — see this module's
/// docs, divergence 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeState {
    /// The state in the 4-input (adder) / 2-input (representation) automaton.
    pub state: i32,
    /// The continued-fraction index at which the input started.
    pub start_index: i32,
    /// The continued-fraction index that is currently active in the input.
    pub seen_index: i32,
}

impl NodeState {
    fn new(state: i32, start_index: i32, seen_index: i32) -> Self {
        NodeState {
            state,
            start_index,
            seen_index,
        }
    }
}

impl std::fmt::Display for NodeState {
    /// `NodeState.toString()` (`NodeState.java:43-45`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{} {} {}]",
            self.state, self.start_index, self.seen_index
        )
    }
}

/// Everything `Ostrowski`'s constructor can reject.
///
/// Java throws `WalnutException` with these exact strings; `crate::walnut_exception`
/// (the module that centralizes those factories) lives in `wr-cli`, one crate up, so the
/// text is spelled verbatim here instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OstrowskiError {
    /// `assertValues`'s `list.isEmpty()` throw (`Ostrowski.java:238-240`). Reachable
    /// from either of the two `assertValues` call sites (`:99`/`:100`) — note the
    /// message says "period" for both, which is Java's wording, kept.
    EmptyPeriod,
    /// `assertValues`'s `d <= 0` throw (`Ostrowski.java:241-245`).
    NonPositiveDigit,
}

impl std::fmt::Display for OstrowskiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OstrowskiError::EmptyPeriod => write!(f, "The period cannot be empty."),
            OstrowskiError::NonPositiveDigit => write!(
                f,
                "Error: All digits of the continued fraction must be positive integers."
            ),
        }
    }
}

impl std::error::Error for OstrowskiError {}

/// `Automata.Numeration.Ostrowski` (`Ostrowski.java:46`).
#[derive(Debug, Clone)]
pub struct Ostrowski {
    /// `Ostrowski.name` (`:54`) — only read by `toString()`.
    name: String,
    /// `Ostrowski.preperiod` (`:57`), package-private in Java and read directly by
    /// `OstrowskiTest`; `pub` here for the same reason.
    pub preperiod: Vec<i32>,
    /// `Ostrowski.period` (`:60`).
    pub period: Vec<i32>,
    /// `Ostrowski.alpha` (`:64`) — `[0] ++ preperiod ++ period`.
    alpha: Vec<i32>,
    /// `Ostrowski.dMax` (`:67`).
    d_max: i32,
    /// `Ostrowski.periodIndex` (`:70`) — where the period begins in `alpha`.
    period_index: i32,
    /// `Ostrowski.adderTransitions` (`:75`). `None` is Java's `NONE` (99) sentinel.
    adder_transitions: [[Option<(i32, i32)>; NUM_STATES]; NUM_STATES],
    /// `Ostrowski.nodeToIndex` (`:78`).
    node_to_index: HashMap<NodeState, usize>,
    /// `Ostrowski.indexToNode` (`:79`).
    index_to_node: Vec<NodeState>,
    /// `Ostrowski.stateTransitions` (`:80`), kept dense — see module docs, divergence 5.
    state_transitions: Vec<BTreeMap<i32, Vec<usize>>>,
    /// `Ostrowski.totalNodes` (`:82`), package-private in Java and read by
    /// `OstrowskiTest.createOstrowski`.
    pub total_nodes: usize,
}

impl std::fmt::Display for Ostrowski {
    /// `Ostrowski.toString()` (`:490-492`). Java renders `alpha` through
    /// `IntArrayList.toString()`, i.e. `[0, 3, 1, 2]`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "name: {}, alpha: {}, period index: {}",
            self.name,
            format_int_list(&self.alpha),
            self.period_index
        )
    }
}

/// Java's `AbstractCollection.toString()` shape: `[a, b, c]`.
fn format_int_list(list: &[i32]) -> String {
    let mut s = String::from("[");
    for (i, v) in list.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&v.to_string());
    }
    s.push(']');
    s
}

/// `Ostrowski.removeLeadingZeros` (`:132-137`).
fn remove_leading_zeros(list: &mut Vec<i32>) {
    let first_non_zero = list.iter().take_while(|&&d| d == 0).count();
    list.drain(0..first_non_zero);
}

/// `Ostrowski.assertValues` (`:237-246`).
fn assert_values(list: &[i32]) -> Result<(), OstrowskiError> {
    if list.is_empty() {
        return Err(OstrowskiError::EmptyPeriod);
    }
    for &d in list {
        if d <= 0 {
            return Err(OstrowskiError::NonPositiveDigit);
        }
    }
    Ok(())
}

/// `Ostrowski.initAdderTransitions` (`:439-488`) — the fixed 7-state 4-input adder
/// table, transcribed literally from the Java assignments at `:447-486`.
///
/// `adder_transitions[p][q] == Some((r, s))` means "state `p` transitions to state `q`
/// on input `r*d + s`, where `d` is the current digit in the continued-fraction
/// expansion of alpha"; `None` is Java's `NONE`.
fn init_adder_transitions() -> [[Option<(i32, i32)>; NUM_STATES]; NUM_STATES] {
    let mut t = [[None; NUM_STATES]; NUM_STATES];
    t[0][0] = Some((0, 0));
    t[0][1] = Some((0, 1));

    t[1][2] = Some((-1, 0));
    t[1][3] = Some((-1, 1));
    t[1][4] = Some((-1, -1));

    t[2][0] = Some((0, -1));
    t[2][1] = Some((0, 0));

    t[3][2] = Some((-1, -1));
    t[3][3] = Some((-1, 0));
    t[3][4] = Some((-1, -2));

    t[4][5] = Some((1, 0));
    t[4][6] = Some((1, -1));

    t[5][2] = Some((-1, 1));
    t[5][3] = Some((-1, 2));
    t[5][4] = Some((-1, 0));

    t[6][0] = Some((0, 1));
    t[6][1] = Some((0, 2));
    t
}

impl Ostrowski {
    /// `Ostrowski(String name, String preperiod, String period)` (`:84-130`), taking
    /// already-parsed digit lists — see this module's docs, divergence 1.
    pub fn new(name: &str, preperiod: &[i32], period: &[i32]) -> Result<Self, OstrowskiError> {
        let mut preperiod = preperiod.to_vec();
        let mut period = period.to_vec();

        // Remove leading 0's in the preperiod (`:92`).
        remove_leading_zeros(&mut preperiod);

        if preperiod.is_empty() {
            // "Easier implementation." (`:94-97`)
            preperiod.extend_from_slice(&period);
        }

        assert_values(&preperiod)?;
        assert_values(&period)?;

        if preperiod[0] == 1 {
            // We want to restrict alpha < 1/2 because otherwise the first two place
            // values in the number system will be 1, which is troublesome (`:102-113`).
            if preperiod.len() > 1 {
                preperiod[0] = preperiod[1] + 1;
                preperiod.remove(1);
            } else {
                // Java reads `period.getInt(0)` three times, all BEFORE the two
                // mutations below change it — so all three see the same original value.
                let p0 = period[0];
                preperiod[0] = p0 + 1;
                period.push(p0);
                period.remove(0);
            }
        }

        let mut alpha = Vec::with_capacity(1 + preperiod.len() + period.len());
        alpha.push(0);
        alpha.extend_from_slice(&preperiod);
        alpha.extend_from_slice(&period);
        let period_index = preperiod.len() as i32 + 1;

        let mut d_max = alpha[1] - 1;
        for &a in &alpha[2..] {
            d_max = d_max.max(a);
        }

        Ok(Ostrowski {
            name: name.to_string(),
            preperiod,
            period,
            alpha,
            d_max,
            period_index,
            adder_transitions: init_adder_transitions(),
            node_to_index: HashMap::new(),
            index_to_node: Vec::new(),
            state_transitions: Vec::new(),
            total_nodes: 0,
        })
    }

    /// `Ostrowski.dMax` — the largest digit of the resulting numeration system, so the
    /// digit alphabet of both automata is `0..=d_max`.
    pub fn d_max(&self) -> i32 {
        self.d_max
    }

    /// `Ostrowski.alpha` — `[0] ++ preperiod ++ period`, the continued-fraction
    /// expansion the two automata are built from.
    pub fn alpha(&self) -> &[i32] {
        &self.alpha
    }

    /// `Ostrowski.periodIndex` — the index in [`Ostrowski::alpha`] where the period
    /// begins.
    pub fn period_index(&self) -> i32 {
        self.period_index
    }

    /// `Ostrowski.createRepresentationAutomaton()` (`:139-144`).
    pub fn create_representation_automaton(&mut self, logging: &mut Logging) -> Automaton {
        let mut repr = self.init_automaton(1);
        self.perform_repr_bfs();
        self.populate_automaton(&mut repr, Self::is_repr_final, logging);
        repr
    }

    /// `Ostrowski.createAdderAutomaton()` (`:146-151`).
    pub fn create_adder_automaton(&mut self, logging: &mut Logging) -> Automaton {
        let mut adder = self.init_automaton(3);
        self.perform_adder_bfs();
        self.populate_automaton(&mut adder, Self::is_adder_final, logging);
        adder
    }

    /// `Ostrowski.isReprFinal` (`:153-155`). Java's `node != null` guard is dropped —
    /// see this module's docs, divergence 4.
    fn is_repr_final(node: &NodeState) -> bool {
        node.state == 0 && node.seen_index == 1
    }

    /// `Ostrowski.isAdderFinal` (`:157-159`).
    fn is_adder_final(node: &NodeState) -> bool {
        (node.state == 0 || node.state == 2 || node.state == 6) && node.seen_index == 1
    }

    /// `Ostrowski.initAutomaton(int inputs)` (`:161-186`).
    fn init_automaton(&mut self, inputs: usize) -> Automaton {
        // Reset global fields (`:163-166`).
        self.node_to_index.clear();
        self.index_to_node.clear();
        self.state_transitions = Vec::new();
        self.total_nodes = 0;

        // Declare the alphabet (`:169-172`).
        let list: Vec<i32> = (0..=self.d_max).collect();
        // Inputs all have the same alphabet and the null NumberSystem (`:174-179`).
        let alphabet = vec![list; inputs];
        let msd = vec![None; inputs];

        // `new Automaton()` + `setA` + `setNS` (`:181-183`). `label` stays at Java's
        // default unbound/empty state: `initAutomaton` never touches it. Passing an
        // empty `label` against a 1- or 3-long `alphabet` is safe here and is a
        // deliberate choice, not an oversight — `Automaton::sort_label` early-returns
        // on an unbound label and `wr_io::writer`'s `write_alphabet` reads
        // `ns_name`/`alphabet`, never `label`. Inventing per-track labels instead would
        // silently turn `sort_label` into a real permutation and change the written
        // track order.
        let mut automaton = Automaton::new(
            Fa {
                q0: 0,
                q: 0,
                alphabet_size: 0,
                o: Vec::new(),
                d: Vec::new(),
                true_false: None,
            },
            alphabet,
            Vec::new(),
            msd,
        );
        // `automaton.determineAlphabetSize()` (`:184`) — NOT hand-computed as
        // `(dMax+1)^inputs`: `d_max` is user-controlled and unbounded through the real
        // `ost` grammar (`ost o [1000000] [1];` is regex-legal), and this call carries
        // the `Math.multiplyExact`-equivalent overflow guard that hand-computing would
        // silently skip.
        automaton.determine_alphabet_size();
        assert_alphabet_size_fits_in_an_int(automaton.fa.alphabet_size);
        automaton
    }

    /// `Ostrowski.populateAutomaton` (`:188-204`).
    fn populate_automaton(
        &mut self,
        automaton: &mut Automaton,
        is_state_final: fn(&NodeState) -> bool,
        logging: &mut Logging,
    ) {
        automaton.fa.q = self.total_nodes;
        automaton.fa.o = Vec::with_capacity(self.total_nodes);
        automaton.fa.d = Vec::with_capacity(self.total_nodes);
        // Java's `putIfAbsent(q, new Int2ObjectRBTreeMap<>())` densification (`:192`) is
        // already maintained continuously by `add_node_indices` — see module docs,
        // divergence 5. The assertion below is the invariant that replaces it.
        debug_assert_eq!(self.state_transitions.len(), self.total_nodes);
        debug_assert_eq!(self.index_to_node.len(), self.total_nodes);
        for q in 0..self.total_nodes {
            automaton
                .fa
                .o
                .push(i32::from(is_state_final(&self.index_to_node[q])));
            automaton
                .fa
                .d
                .push(std::mem::take(&mut self.state_transitions[q]));
        }

        automaton.determinize_and_minimize_with_ctx(None, logging);

        // We need to canonize and remove the first state. The automaton will work with
        // this state as well, but it is useless. This happens because the Automaton
        // class does not support an epsilon transition for NFAs (`:198-201`).
        automaton.canonize();

        handle_zero_state(&mut automaton.fa);

        // No Java counterpart, but it REPRODUCES one — see module docs, divergence 6.
        automaton.set_canonized(true);
    }

    /// `Ostrowski.alphaI(int i)` (`:248-252`).
    ///
    /// The modular-wrap `else` branch is dead in practice (every call site's `i` is
    /// `< alpha.size()`); ported verbatim per the mechanical-port rule.
    fn alpha_i(&self, i: i32) -> i32 {
        let alpha_size = self.alpha.len() as i32;
        let index = if i < alpha_size {
            i
        } else {
            self.period_index + ((i - alpha_size) % (alpha_size - self.period_index))
        };
        self.alpha[index as usize]
    }

    /// `Ostrowski.performAdderBfs` (`:254-297`).
    fn perform_adder_bfs(&mut self) {
        // In a node, the indices mean the following.
        // 0: The state in the 4-input automaton.
        // 1: The C.F. index at which the input started.
        // 2: The C.F. index that is currently active in the input.
        let mut queue = self.initialize_bfs();
        let alpha_size = self.alpha.len() as i32;
        for i in 1..alpha_size {
            self.add_node_with_new_transitions(NodeState::new(0, i, i), 0, &mut queue, 0);
        }

        while let Some(cur_node_idx) = queue.pop_front() {
            let cur_node = self.index_to_node[cur_node_idx];
            let seen_index = cur_node.seen_index;

            if seen_index == 1 && alpha_size > 2 && self.period_index > 1 {
                // The input ends here.
                continue;
            }

            let state = cur_node.state;
            let start_index = cur_node.start_index;

            for st in 0..NUM_STATES {
                let (r, s) = match self.adder_transitions[state as usize][st] {
                    Some(rs) => rs,
                    // `if (r == NONE || s == NONE) continue;` (`:283-285`).
                    None => continue,
                };
                let st = st as i32;

                if seen_index > 1 {
                    let a = self.alpha_i(seen_index - 1);
                    self.add_transitions_and_node(
                        NodeState::new(st, start_index, seen_index - 1),
                        cur_node_idx,
                        a,
                        r,
                        s,
                        &mut queue,
                    );
                }
                if seen_index == self.period_index {
                    let a = self.alpha_i(alpha_size - 1);
                    self.add_transitions_and_node(
                        NodeState::new(st, start_index, alpha_size - 1),
                        cur_node_idx,
                        a,
                        r,
                        s,
                        &mut queue,
                    );
                }
            }
        }
    }

    /// `Ostrowski.performReprBfs` (`:299-337`).
    fn perform_repr_bfs(&mut self) {
        // In a node, the indices mean the following.
        // 0: The state in the 2-input automaton.
        // 1: The C.F. index at which the input started.
        // 2: The C.F. index that is currently active in the input.
        let mut queue = self.initialize_bfs();
        let alpha_size = self.alpha.len() as i32;
        for i in 1..alpha_size {
            let a = self.alpha_i(i);
            self.add_node_indices(NodeState::new(0, i, i));
            for inp in 0..a {
                self.put_state_transition(0, inp, self.total_nodes);
            }
            queue.push_back(self.total_nodes);
            self.total_nodes += 1;

            self.add_node(NodeState::new(1, i, i), 0, &mut queue, a);
        }

        while let Some(cur_node_idx) = queue.pop_front() {
            let cur_node = self.index_to_node[cur_node_idx];
            let seen_index = cur_node.seen_index;

            if seen_index == 1 && alpha_size > 2 && self.period_index > 1 {
                // The input ends here.
                continue;
            }

            if seen_index > 1 {
                self.handle_transition(
                    seen_index,
                    seen_index - 1,
                    cur_node.state,
                    cur_node_idx,
                    cur_node.start_index,
                    seen_index - 1,
                    &mut queue,
                );
            }
            if seen_index == self.period_index {
                self.handle_transition(
                    seen_index,
                    alpha_size - 1,
                    cur_node.state,
                    cur_node_idx,
                    cur_node.start_index,
                    alpha_size - 1,
                    &mut queue,
                );
            }
        }
    }

    /// `Ostrowski.initializeBfs` (`:339-347`).
    fn initialize_bfs(&mut self) -> VecDeque<usize> {
        // `initAutomaton` has already emptied both maps and zeroed `total_nodes`; Java
        // relies on the same ordering (`initializeBfs` clears neither).
        debug_assert!(self.index_to_node.is_empty() && self.total_nodes == 0);
        let start_node = NodeState::new(0, 0, 0); // This is the start state.
        self.node_to_index.insert(start_node, 0);
        self.index_to_node.push(start_node);
        self.total_nodes += 1;
        // These are the "0" states (`:344-345`). The dense `Vec` carries the same
        // "index 0 has a row" seeding.
        self.state_transitions = vec![BTreeMap::new()];
        VecDeque::new()
    }

    /// `Ostrowski.handleTransition` (`:349-366`).
    #[allow(clippy::too_many_arguments)]
    fn handle_transition(
        &mut self,
        seen_index: i32,
        target_seen_index: i32,
        state: i32,
        cur_node_idx: usize,
        start_index: i32,
        alpha_index: i32,
        queue: &mut VecDeque<usize>,
    ) {
        // `stateTransitions.putIfAbsent(curNodeIdx, …)` (`:351`) — already dense here.
        debug_assert!(cur_node_idx < self.state_transitions.len());

        // Compute 'a' based on the state (`:354`).
        let a = if state == 1 {
            1
        } else {
            self.alpha_i(alpha_index)
        };

        // Transition to state 0 for all values < a (`:356-360`).
        let node = NodeState::new(0, start_index, target_seen_index);
        for inp in 0..a {
            self.point_symbol_to_node(node, cur_node_idx, queue, inp);
        }

        // Transition to state 1 if needed (`:362-365`).
        if state == 0 && (seen_index > 2 || seen_index == self.period_index) {
            self.point_symbol_to_node(
                NodeState::new(1, start_index, target_seen_index),
                cur_node_idx,
                queue,
                a,
            );
        }
    }

    /// `Ostrowski.addTransitionsAndNode` (`:368-380`).
    fn add_transitions_and_node(
        &mut self,
        node: NodeState,
        cur_node_idx: usize,
        a: i32,
        r: i32,
        s: i32,
        queue: &mut VecDeque<usize>,
    ) {
        // `stateTransitions.putIfAbsent(curNodeIdx, …)` (`:369`) — already dense here.
        debug_assert!(cur_node_idx < self.state_transitions.len());
        match self.node_to_index.get(&node) {
            // This node already exists, don't create a new NodeState (`:370-376`).
            Some(&existing) => {
                let d_max = self.d_max;
                add_transitions(
                    &mut self.state_transitions[cur_node_idx],
                    a * r + s,
                    existing,
                    d_max,
                );
            }
            None => self.add_node_with_new_transitions(node, cur_node_idx, queue, a * r + s),
        }
    }

    /// `Ostrowski.addNodeIndices` (`:382-385`), plus this port's continuous density fill
    /// for `state_transitions` — see module docs, divergence 5.
    fn add_node_indices(&mut self, node: NodeState) {
        self.node_to_index.insert(node, self.total_nodes);
        self.index_to_node.push(node);
        self.state_transitions.push(BTreeMap::new());
        debug_assert_eq!(self.index_to_node.len(), self.total_nodes + 1);
        debug_assert_eq!(self.state_transitions.len(), self.total_nodes + 1);
    }

    /// `Ostrowski.pointSymbolToNode` (`:387-393`).
    fn point_symbol_to_node(
        &mut self,
        node: NodeState,
        cur_node_idx: usize,
        queue: &mut VecDeque<usize>,
        inp: i32,
    ) {
        match self.node_to_index.get(&node) {
            Some(&existing) => self.put_state_transition(cur_node_idx, inp, existing),
            None => self.add_node(node, cur_node_idx, queue, inp),
        }
    }

    /// `Ostrowski.addNode` (`:395-400`) — create a new `NodeState`.
    fn add_node(
        &mut self,
        node: NodeState,
        cur_node_idx: usize,
        queue: &mut VecDeque<usize>,
        inp: i32,
    ) {
        self.add_node_indices(node);
        self.put_state_transition(cur_node_idx, inp, self.total_nodes);
        queue.push_back(self.total_nodes);
        self.total_nodes += 1;
    }

    /// `Ostrowski.addNodeWithNewTransitions` (`:402-407`) — create a new `NodeState`.
    fn add_node_with_new_transitions(
        &mut self,
        node: NodeState,
        cur_node_idx: usize,
        queue: &mut VecDeque<usize>,
        inp: i32,
    ) {
        self.add_node_indices(node);
        let d_max = self.d_max;
        let total_nodes = self.total_nodes;
        add_transitions(
            &mut self.state_transitions[cur_node_idx],
            inp,
            total_nodes,
            d_max,
        );
        queue.push_back(self.total_nodes);
        self.total_nodes += 1;
    }

    /// `Ostrowski.putStateTransition` (`:409-412`).
    fn put_state_transition(&mut self, cur_node_idx: usize, inp: i32, value: usize) {
        self.state_transitions[cur_node_idx]
            .entry(inp)
            .or_default()
            .push(value);
    }
}

/// Java's `RichAlphabet.determineAlphabetSize`'s `Math.multiplyExact` overflow check, at
/// **`int`** width (`RichAlphabet.java:60-66`).
///
/// [`Automaton::determine_alphabet_size`] already runs the same check, but at `usize`
/// width — a deliberate, long-documented divergence on that method (see its own doc
/// comment: "a product between 2^31 and 2^64 hard-errors in Java but succeeds here …
/// an adversarial-review finding, not yet closed"). For every other caller in this crate
/// the gap is unreachable; **for `ost` it is not**, because `d_max` comes straight from
/// user-supplied digits through a `\d+`-only regex, and the adder automaton's alphabet is
/// `(d_max + 1)^3`.
///
/// Verified live against the real jar: `ost bigone [1291] [1];` (the smallest `d_max`
/// whose cube exceeds `Integer.MAX_VALUE`) makes real Walnut write `msd_bigone.txt`, then
/// throw `ArithmeticException: integer overflow` out of `Ostrowski.createAdderAutomaton`
/// and return to the prompt. Without this guard the port silently WRAPPED the `i32`
/// `input_encode` arithmetic in `add_transitions` below (identical to Java's own `int`
/// arithmetic there — Java simply never reaches it) and, in a release build, spent ~14 s
/// writing a bogus `msd_bigone_addition.txt`. That is a port defect, not a Walnut quirk,
/// so it is fixed here rather than logged in `docs/WALNUT-BUGS.md`.
///
/// Scoped to this module on purpose: widening
/// [`Automaton::determine_alphabet_size`]'s own check to `i32` would change behavior for
/// every caller in the crate, which is a separate, cross-cutting decision.
///
/// # Panics
///
/// With Java's `ArithmeticException` message verbatim, per
/// [`crate::walnut_panic`]'s guard-authoring rule — `wr_cli::prover::Prover::caught`
/// recovers it exactly where `Prover.readBuffer`'s `catch (RuntimeException)` does.
fn assert_alphabet_size_fits_in_an_int(alphabet_size: usize) {
    if i32::try_from(alphabet_size).is_err() {
        panic!("integer overflow");
    }
}

/// `Ostrowski.addTransitions` (`:414-427`).
///
/// Admits `(x, y, z)` exactly when `z = diff + x + y` — one possible `z` per `(x, y)`.
fn add_transitions(
    current_state_transitions: &mut BTreeMap<i32, Vec<usize>>,
    diff: i32,
    encoded_destination: usize,
    d_max: i32,
) {
    let base = d_max + 1;
    for x in 0..=d_max {
        for y in 0..=d_max {
            let z = diff + x + y; // Only one possible value for z
            if (0..=d_max).contains(&z) {
                let input_encode = x + base * y + base * base * z;
                current_state_transitions
                    .entry(input_encode)
                    .or_default()
                    .push(encoded_destination);
            }
        }
    }
}

/// `Ostrowski.handleZeroState(FA fa)` (`:206-225`). "Note: this is a minimized and
/// canonized DFA."
///
/// Java's `fa.ensureNfaTransitions()` (`:208`) has no counterpart: [`Fa`] has one
/// unconditional NFA-shaped table (`d`), the Java `TransitionsNFA`/`TransitionsDFA`
/// split having been deliberately collapsed (see `crate::fa`'s module docs) — there is
/// no hidden DFA-side storage to convert from.
///
/// Two details are exact, not paraphrased: the "is state 0 ever a destination" scan
/// (`:209-212`) tests only the **first** element of each destination list
/// (`es.getValue().getInt(0) == 0`), and the index shift (`:218-223`) decrements only
/// element 0 (`v.set(0, v.getInt(0) - 1)`). On the post-minimize/canonize DFA every
/// destination list is a singleton, so neither is observable for this command's actual
/// outputs — but the translation reads `[0]` rather than `.any(…)`/`.iter_mut()` so a
/// future reader is not misled about the general (non-DFA) case.
fn handle_zero_state(fa: &mut Fa) {
    let zero_state_needed = fa.d.iter().any(|tm| tm.values().any(|v| v[0] == 0));
    if !zero_state_needed {
        // remove 0th state
        fa.d.remove(0);
        fa.o.remove(0);
        fa.q -= 1;
        for tm in fa.d.iter_mut() {
            for v in tm.values_mut() {
                v[0] -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Tier 2 — `Automata/Numeration/OstrowskiTest.java`
    //
    // The eight `testAgainstFile` cases (`createFib`/`createNumSys`/`createPell`/
    // `createNs6`..`createNs10`) and `writeAutomatonThrowsWhenFileAlreadyExists` are
    // NOT here: their Java assertions are byte-level comparisons of
    // `AutomatonWriter.writeTxtFormatToStream` output / a `Session`-relative write,
    // which need `wr-io`'s writer and `wr-cli`'s `Session`. They live in
    // `wr_cli::ost`'s own test module instead — `wr-cli` is the lowest crate that has
    // both. Nothing was dropped.
    // -----------------------------------------------------------------------

    /// `OstrowskiTest.createOstrowski` (`:33-42`): alpha = sqrt(3) - 1, pre-period = []
    /// and period = [1, 2].
    #[test]
    fn create_ostrowski() {
        let on = Ostrowski::new("testOstrowski", &[], &[1, 2]).expect("valid");
        assert_eq!(
            on.to_string().replace(' ', ""),
            "name:testOstrowski,alpha:[0,3,1,2],periodindex:2"
        );
        // Java's own `// huh?` comment: the empty pre-period was copy-filled from the
        // period ([1, 2]) and then collapsed to a single digit by the `preperiod[0] == 1`
        // rotation, so it is size 1 even though the command said "".
        assert_eq!(on.preperiod.len(), 1);
        assert_eq!(on.period.len(), 2);
        assert_eq!(on.total_nodes, 0);
    }

    /// `OstrowskiTest.createOstrowskiSinglePreperiodDigitOneRotatesFromPeriod`
    /// (`:56-69`) — the `else` branch of `if (this.preperiod.size() > 1)`
    /// (`Ostrowski.java:105`), i.e. lines 109-111, reached when the copy-filled
    /// pre-period has exactly ONE element and that element is 1.
    #[test]
    fn create_ostrowski_single_preperiod_digit_one_rotates_from_period() {
        let mut on = Ostrowski::new("rotate", &[], &[1]).expect("valid");
        assert_eq!(on.preperiod, vec![2]);
        assert_eq!(on.period, vec![1]);
        assert_eq!(
            on.to_string().replace(' ', ""),
            "name:rotate,alpha:[0,2,1],periodindex:2"
        );

        // Both automaton constructions must still run to completion on this shape,
        // confirming the rotated preperiod/period is a valid input to the rest of the
        // BFS/automaton-construction pipeline, not just to the constructor.
        let mut logging = Logging::new();
        assert!(on.create_representation_automaton(&mut logging).fa.q > 0);
        assert!(on.create_adder_automaton(&mut logging).fa.q > 0);
    }

    /// The OTHER rotation branch (`Ostrowski.java:105-107`), which `OstrowskiTest` only
    /// covers incidentally through `createNs6`..`createNs10`: a multi-digit pre-period
    /// whose first digit is 1 folds into the second.
    #[test]
    fn a_multi_digit_preperiod_starting_with_one_folds_into_the_second_digit() {
        let on = Ostrowski::new("rotmulti", &[1, 2], &[3]).expect("valid");
        assert_eq!(on.preperiod, vec![3]);
        assert_eq!(on.period, vec![3]);
        assert_eq!(on.alpha(), &[0, 3, 3]);
        assert_eq!(on.period_index(), 2);
        assert_eq!(on.d_max(), 3);
    }

    /// `OstrowskiTest.emptyPeriodThrows` (`:102-107`) — `assertValues`' `isEmpty()`
    /// throw (`Ostrowski.java:239`) via the `assertValues(this.period)` call site
    /// (`:100`).
    #[test]
    fn empty_period_throws() {
        let err = Ostrowski::new("emptyPeriod", &[1], &[]).unwrap_err();
        assert_eq!(err, OstrowskiError::EmptyPeriod);
        assert_eq!(err.to_string(), "The period cannot be empty.");
    }

    /// Both `assertValues` call sites can raise `EmptyPeriod`: with an empty period AND
    /// an empty (or all-zero) pre-period, the copy-fill at `:94-97` leaves the pre-period
    /// empty too, so `assertValues(this.preperiod)` (`:99`) throws FIRST. Same message
    /// either way, which is why Java's own test cannot tell the two apart — pinned here
    /// so a port that silently reordered the two calls would still be caught by the
    /// `[0]`-indexing panic that would follow.
    #[test]
    fn an_empty_preperiod_and_period_throws_from_the_first_assert_values_call() {
        assert_eq!(
            Ostrowski::new("both", &[], &[]).unwrap_err(),
            OstrowskiError::EmptyPeriod
        );
        assert_eq!(
            Ostrowski::new("zeros", &[0, 0], &[]).unwrap_err(),
            OstrowskiError::EmptyPeriod
        );
    }

    /// `OstrowskiTest.nonPositivePreperiodDigitThrows` (`:115-121`) — `assertValues`'
    /// `d <= 0` throw (`Ostrowski.java:243`) via `assertValues(this.preperiod)` (`:99`).
    /// `[2, 0]` has no LEADING zero, so `removeLeadingZeros` leaves it untouched and the
    /// internal 0 trips the positive-digit check.
    #[test]
    fn non_positive_preperiod_digit_throws() {
        let err = Ostrowski::new("badPreperiod", &[2, 0], &[1]).unwrap_err();
        assert_eq!(err, OstrowskiError::NonPositiveDigit);
        assert_eq!(
            err.to_string(),
            "Error: All digits of the continued fraction must be positive integers."
        );
    }

    /// `OstrowskiTest.nonPositivePeriodDigitThrows` (`:129-135`) — the same source line
    /// reached via the OTHER call site, `assertValues(this.period)` (`:100`).
    #[test]
    fn non_positive_period_digit_throws() {
        assert_eq!(
            Ostrowski::new("badPeriod", &[2], &[1, 0]).unwrap_err(),
            OstrowskiError::NonPositiveDigit
        );
    }

    /// `OstrowskiTest.testNode` (`:137-145`), minus the `compareTo` assertion:
    /// `NodeState` deliberately does not implement `Ord` here (see this module's docs,
    /// divergence 3 — `compareTo` has no observable effect anywhere in `Ostrowski.java`,
    /// so porting it would be adding dead API, not fidelity). The `equals`/`toString`
    /// halves are kept, plus the inequality `compareTo` was standing in for.
    #[test]
    fn test_node() {
        let node = NodeState::new(0, 1, 2);
        assert_eq!(node.to_string(), "[0 1 2]");
        let node2 = NodeState::new(1, 1, 2);
        assert_ne!(node, node2);
        assert_eq!(node, NodeState::new(0, 1, 2));
        // The three fields really are all part of the identity `nodeToIndex` keys on.
        assert_ne!(node, NodeState::new(0, 9, 2));
        assert_ne!(node, NodeState::new(0, 1, 9));
    }

    // -----------------------------------------------------------------------
    // `removeLeadingZeros`
    // -----------------------------------------------------------------------

    #[test]
    fn remove_leading_zeros_strips_only_the_leading_run() {
        let mut v = vec![0, 0, 3, 0, 1];
        remove_leading_zeros(&mut v);
        assert_eq!(v, vec![3, 0, 1]);
        let mut all_zero = vec![0, 0];
        remove_leading_zeros(&mut all_zero);
        assert!(all_zero.is_empty());
        let mut none = vec![5, 0];
        remove_leading_zeros(&mut none);
        assert_eq!(none, vec![5, 0]);
    }

    // -----------------------------------------------------------------------
    // The `set_canonized` step (module docs, divergence 6)
    // -----------------------------------------------------------------------

    /// Both constructions must hand back an automaton flagged canonized, so
    /// [`wr_io::writer::write_txt`]'s unconditional `canonize()` is the no-op Java's
    /// memoized `FA.canonized` makes it.
    ///
    /// **Honest scope**: on every input this port currently exercises — the eight
    /// shipped systems `wr_cli::ost`'s Tier-2 tests cover, plus the rotation case —
    /// re-canonicalizing after `handle_zero_state` was measured to be byte-for-byte a
    /// no-op anyway (the state-0 removal branch fires on 16 of those 18 automata, and
    /// the shifted numbering is still BFS-canonical), so the flag is not *currently*
    /// observable in the written files. It is set regardless, because the port must not
    /// silently depend on that coincidence holding for an arbitrary user-supplied
    /// continued fraction: Java's writer provably never re-canonicalizes here, and
    /// `Fa::canonicalize` also trims, which is not obviously idempotent after a start
    /// state has been deleted out from under it.
    #[test]
    fn both_constructions_leave_the_automaton_flagged_canonized() {
        let mut ost = Ostrowski::new("fib", &[0, 2], &[1]).expect("valid");
        let mut logging = Logging::new();
        assert!(ost
            .create_representation_automaton(&mut logging)
            .is_canonized());
        assert!(ost.create_adder_automaton(&mut logging).is_canonized());
    }

    // -----------------------------------------------------------------------
    // Tier 4 — a Walnut-independent property oracle
    //
    // Derived from the continued fraction itself, NOT from anything this module
    // computes: the Ostrowski place values are
    //
    //     q[0] = 1,  q[1] = a_1,  q[i] = a_i * q[i-1] + q[i-2]
    //
    // and a digit string b_L … b_1 (msd-first, so b_i is the digit `i` positions from
    // the right) is a valid Ostrowski representation iff
    //
    //     0 <= b_1 <= a_1 - 1,   0 <= b_i <= a_i (i >= 2),
    //     b_i == a_i  =>  b_{i-1} == 0  (i >= 2)
    //
    // with value  sum_{i=1..L} b_i * q[i-1]. (Baranwal 2020, §2; the same source
    // `Ostrowski.java`'s own class doc cites.) Note the language is closed under
    // prepending zeros: a new leading digit 0 always satisfies both its own bound and
    // the implication, and never constrains the digit below it.
    //
    // For `fib` (alpha = [0; 2, 1, 1, …]) this reduces to place values 1, 2, 3, 5, 8, …
    // and "no two adjacent 1s" — Zeckendorf — which is hand-checkable; for `pell`
    // (alpha = [0; 2, 2, 2, …]) to 1, 2, 5, 12, … . Both are asserted below before the
    // general sweep, so a wrong general derivation cannot hide behind agreement with
    // the port.
    // -----------------------------------------------------------------------

    /// `alphaI` re-derived from the public accessors, so the oracle does not call the
    /// private method it is meant to check.
    fn oracle_alpha_i(alpha: &[i32], period_index: i32, i: i32) -> i32 {
        let n = alpha.len() as i32;
        let index = if i < n {
            i
        } else {
            period_index + ((i - n) % (n - period_index))
        };
        alpha[index as usize]
    }

    /// `q[0..=n]`, the Ostrowski place values.
    fn place_values(alpha: &[i32], period_index: i32, n: usize) -> Vec<i128> {
        let mut q = vec![1i128];
        if n >= 1 {
            q.push(i128::from(oracle_alpha_i(alpha, period_index, 1)));
        }
        for i in 2..=n {
            let a = i128::from(oracle_alpha_i(alpha, period_index, i as i32));
            let next = a * q[i - 1] + q[i - 2];
            q.push(next);
        }
        q
    }

    /// Is `word` (msd-first) a valid Ostrowski representation?
    fn is_valid_rep(alpha: &[i32], period_index: i32, word: &[i32]) -> bool {
        let l = word.len();
        // b[i] for i in 1..=l, i counted from the RIGHT.
        let b = |i: usize| word[l - i];
        for i in 1..=l {
            let bound = if i == 1 {
                oracle_alpha_i(alpha, period_index, 1) - 1
            } else {
                oracle_alpha_i(alpha, period_index, i as i32)
            };
            if b(i) < 0 || b(i) > bound {
                return false;
            }
            if i >= 2 && b(i) == oracle_alpha_i(alpha, period_index, i as i32) && b(i - 1) != 0 {
                return false;
            }
        }
        true
    }

    fn value_of(alpha: &[i32], period_index: i32, word: &[i32]) -> i128 {
        let l = word.len();
        let q = place_values(alpha, period_index, l.saturating_sub(1));
        (1..=l).map(|i| i128::from(word[l - i]) * q[i - 1]).sum()
    }

    /// Every word of length exactly `len` over `0..=d_max`, msd-first.
    fn words(d_max: i32, len: usize) -> Vec<Vec<i32>> {
        let base = (d_max + 1) as usize;
        let mut out = Vec::with_capacity(base.pow(len as u32));
        for n in 0..base.pow(len as u32) {
            let mut w = vec![0i32; len];
            let mut n = n;
            for slot in w.iter_mut().rev() {
                *slot = (n % base) as i32;
                n /= base;
            }
            out.push(w);
        }
        out
    }

    /// The two hand-checkable anchors for the derivation above, so the sweep below is
    /// not just "the oracle agrees with itself".
    #[test]
    fn the_place_value_derivation_reproduces_fibonacci_and_pell() {
        let fib = Ostrowski::new("fib", &[0, 2], &[1]).expect("valid");
        assert_eq!(
            place_values(fib.alpha(), fib.period_index(), 6),
            vec![1, 2, 3, 5, 8, 13, 21]
        );
        let pell = Ostrowski::new("pell", &[0], &[2]).expect("valid");
        assert_eq!(
            place_values(pell.alpha(), pell.period_index(), 5),
            vec![1, 2, 5, 12, 29, 70]
        );
        // Zeckendorf: over `fib`, exactly the words with no two adjacent 1s are valid.
        for len in 0..=5 {
            for w in words(fib.d_max(), len) {
                let no_adjacent_ones = w.windows(2).all(|p| p != [1, 1]);
                assert_eq!(
                    is_valid_rep(fib.alpha(), fib.period_index(), &w),
                    no_adjacent_ones,
                    "{w:?}"
                );
            }
        }
    }

    /// The representation automaton accepts exactly the valid Ostrowski representations,
    /// and the adder automaton accepts exactly the aligned triples of valid
    /// representations whose values add up — both checked against the from-the-math
    /// oracle above, never against the port's own construction.
    /// # Why the adder half is restricted to canonical triples
    ///
    /// The adder automaton's own language is deliberately WIDER than "the sum relation":
    /// it is the relation Walnut then intersects with the representation automaton on
    /// every track (`NumberSystem`'s `allRepresentations` restriction — see
    /// [`crate::automaton::Automaton::all_reps`], which is exactly what the custom-base
    /// loader installs from `msd_<name>.txt`). Concretely, state 0's `(r, s) = (0, 0)`
    /// self-loop makes `z = x + y` digitwise, so `(0, w, w)` is accepted for ANY digit
    /// string `w`, canonical or not — and conversely a value-equal pair of a canonical
    /// and a non-canonical string is generally rejected. Neither is a defect; both are
    /// outside the domain on which the adder is specified. Asserting the sum property on
    /// the canonical triples is therefore the real, checkable statement, and it is the
    /// one every downstream use depends on.
    fn check_against_oracle(name: &str, pre: &[i32], per: &[i32], max_len: usize) {
        let mut ost = Ostrowski::new(name, pre, per).expect("valid continued fraction");
        let alpha = ost.alpha().to_vec();
        let period_index = ost.period_index();
        let d_max = ost.d_max();
        let mut logging = Logging::new();
        let repr = ost.create_representation_automaton(&mut logging);
        let adder = ost.create_adder_automaton(&mut logging);

        let mut accepted = 0usize;
        let mut adder_accepted = 0usize;
        for len in 0..=max_len {
            let all = words(d_max, len);
            for w in &all {
                let encoded: Vec<i32> = w.iter().map(|&d| repr.encode(&[d])).collect();
                let expected = is_valid_rep(&alpha, period_index, w);
                assert_eq!(
                    repr.fa.accepts_word(&encoded),
                    expected,
                    "{name}: representation automaton on {w:?}"
                );
                if expected {
                    accepted += 1;
                }
            }

            // Every canonical word of this length has a distinct value, and together
            // they cover exactly `0 .. q[len]` -- the existence-and-uniqueness half of
            // the Ostrowski representation theorem, checked against the place values
            // rather than against the automaton.
            let canonical: Vec<&Vec<i32>> = all
                .iter()
                .filter(|w| is_valid_rep(&alpha, period_index, w))
                .collect();
            let mut values: Vec<i128> = canonical
                .iter()
                .map(|w| value_of(&alpha, period_index, w))
                .collect();
            values.sort_unstable();
            let q = place_values(&alpha, period_index, len);
            assert_eq!(
                values,
                (0..q[len]).collect::<Vec<i128>>(),
                "{name}: canonical words of length {len} do not enumerate 0..{}",
                q[len]
            );

            // The adder, over aligned (equal-length, zero-padded) CANONICAL triples --
            // see this function's doc comment for why the restriction is the property,
            // not a weakening of it.
            for x in &canonical {
                let vx = value_of(&alpha, period_index, x);
                for y in &canonical {
                    let vy = value_of(&alpha, period_index, y);
                    for z in &canonical {
                        let expected = vx + vy == value_of(&alpha, period_index, z);
                        let encoded: Vec<i32> = (0..len)
                            .map(|j| adder.encode(&[x[j], y[j], z[j]]))
                            .collect();
                        assert_eq!(
                            adder.fa.accepts_word(&encoded),
                            expected,
                            "{name}: adder on {x:?} + {y:?} = {z:?}"
                        );
                        if expected {
                            adder_accepted += 1;
                        }
                    }
                }
            }
        }
        // Tripwires against a vacuous sweep (e.g. an oracle that rejects everything, or
        // an alphabet/encoding mix-up that makes every run die on the first symbol).
        assert!(accepted > max_len, "{name}: sweep accepted almost nothing");
        assert!(
            adder_accepted > max_len,
            "{name}: adder sweep confirmed almost no sums"
        );
    }

    #[test]
    fn fib_matches_the_independent_ostrowski_oracle() {
        check_against_oracle("fib", &[0, 2], &[1], 7);
    }

    #[test]
    fn pell_matches_the_independent_ostrowski_oracle() {
        check_against_oracle("pell", &[0], &[2], 4);
    }

    #[test]
    fn a_preperiodic_system_matches_the_independent_ostrowski_oracle() {
        // `msd_numsys` — the example from Walnut's own Ostrowski documentation,
        // alpha = [0; 3, 1, (1, 2)*], the only shipped case with a genuine pre-period
        // AND a multi-digit period (so `alphaI`'s periodic wrap really is exercised).
        check_against_oracle("numsys", &[0, 3, 1], &[1, 2], 5);
    }

    #[test]
    fn a_rotated_system_matches_the_independent_ostrowski_oracle() {
        // The `preperiod[0] == 1`, `size == 1` rotation branch (`Ostrowski.java:109-111`).
        check_against_oracle("rotate", &[], &[1], 7);
    }
}
