// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Ports `Automata/Transducer.java` (438 LOC) — a **deterministic finite-state
//! transducer** (DFST) with all states final, 1-uniform, and the Dekking (1994)
//! transduction that applies one to an automatic sequence (a DFAO).
//!
//! # What a `Transducer` *is*, structurally
//!
//! Java's `Transducer extends Automaton` and adds one field, `sigma`
//! (`Transducer.java:61`): the 1-uniform output function with domain
//! `states × input-alphabet`. Rust has no inheritance, so this is composition —
//! [`Transducer::automaton`] is the inherited `Automaton` half and
//! [`Transducer::sigma`] the added field. Nothing in the transduction ever reads the
//! transducer's per-state *output* (`fa.o`); only `fa.q`, `fa.q0`, `fa.d`,
//! `alphabet`, and `sigma` are consulted. That matches `AutomatonReader.
//! readTransducer`, which discards state outputs outright ("state output does not
//! matter for transducers").
//!
//! Like `Automaton`'s own transition table, `sigma` is keyed by the **encoded**
//! input symbol, not the raw digit tuple (Java's own comment at `:58-59`: "instead of
//! saying that 'on (0, -1) we output 1', we really store 'on 0, output 1'").
//!
//! # Constructing one (the recipe U26's `transduce` command needs)
//!
//! Parsing a `.txt` DFST already lives in `wr_io::reader::read_transducer_txt`
//! (Phase 3a's U13), which returns a `TransducerData`. `wr-io` depends on `wr-core`,
//! not the reverse, so the conversion belongs on the `wr-io`/`wr-cli` side — exactly
//! the same split [`crate::morphism::Morphism::from_mapping`] already uses for
//! `ParseMethods.parseMorphism`. The mapping is mechanical, and is spelled out here
//! so the later unit doesn't have to re-derive it:
//!
//! ```text
//! Fa { q0: data.q0, q: data.q, alphabet_size: data.alphabet_size,
//!      o: vec![0; data.q],           // readTransducer's discarded state outputs
//!      d: data.d, true_false: None }
//! Transducer::new(Automaton::new(fa, data.alphabet, label, data.msd), data.sigma)
//! ```
//!
//! (`label` is only used to be copied onto the *result* automaton — and only from the
//! **input** automaton `M`, never from the transducer — so any per-track placeholder
//! matching `readTransducer`'s own `0`, `1`, … will do.)
//!
//! # The algorithm (`transduceMsdDeterministic`, `:99-240`)
//!
//! Dekking's construction. `M` is read as a morphism `h`: state `x` maps to the word
//! `h(x)` of its destinations, listed in **encoded-symbol order**. Each letter `a` of
//! `M`'s output alphabet induces a map `phi_a : S_T -> S_T` on the transducer's
//! states (where `phi_a(s) = delta_T(s, a)`), and a word `w` of `M`-states induces
//! the composition `phi_{M.O(w)}`. Because there are finitely many such maps, the
//! sequence `phi_{M.O(h^m(·))}` (as a vector over all of `M`'s states) is ultimately
//! periodic — the first loop finds its lag `q` and period `p` by hashing each
//! iterate-vector. A state of the result is then the pair
//! `(M-state, [phi_{M.O(h^i(w))} for i in 0..p+q])`, and the BFS below explores those
//! pairs.
//!
//! Two things about that state key are load-bearing and are ported exactly:
//! - The word `w` (`StateTuple.iList`) is carried along to *build* successors but is
//!   deliberately **excluded from equality and hashing** (Java's hand-written
//!   `equals`/`hashCode` at `:406-437`, both carrying an explicit "DO NOT compare/use
//!   the string" comment). Including it would make the state set infinite.
//! - Java's `hashCode` is `state ^ (state >>> 32)` — on an `int`, `>>> 32` is a shift
//!   by `32 % 32 == 0`, so that whole term is `state ^ state == 0` and the `state`
//!   field contributes **nothing** to the hash (a copy-paste from a `long`-typed
//!   `hashCode`). That is a pure quality-of-hash quirk with no observable effect
//!   (`equals` still compares `state`), so [`StateTuple`]'s `Hash` impl here hashes
//!   both fields — strictly better distribution, identical semantics.
//!
//! # Java's `encode` returns `-1` for an out-of-alphabet digit, and this file DEPENDS on it
//!
//! Every input symbol here is produced by `richAlphabet.encode(List.of(v))` where `v`
//! is one of `M`'s per-state output values (`:188`, `:277`, `:397`). `RichAlphabet.
//! encode` (`RichAlphabet.java:110-116`) is `sum(encoder[i] * A[i].indexOf(l[i]))`,
//! and `List.indexOf` returns `-1` when the value is absent — so for a one-element
//! list (`encoder[0] == 1` always) an out-of-alphabet value encodes to exactly `-1`,
//! silently, rather than failing.
//!
//! [`Automaton::encode`] deliberately **panics** instead (a documented improvement
//! over Java's silent corruption, see its doc comment) — so it cannot be used here.
//! `transduceNonDeterministic`'s dead-state path genuinely relies on the `-1`
//! behavior: it marks the added dead state with output `min(M.O) - 1`, a value that is
//! by construction *not* in the transducer's input alphabet, and then looks it up.
//! [`Transducer::encode_input`] below is therefore a local, single-track port of
//! Java's `encode` **including** the `indexOf` → `-1` fallback. Using
//! `Automaton::encode` here would panic on the exact `RUNSUM`-on-a-partial-automaton
//! shape Walnut supports.
//!
//! # WB-035: `minOutput` is used both as an encoded INPUT symbol and as an OUTPUT marker
//!
//! See `docs/WALNUT-BUGS.md`. `transduceNonDeterministic`'s partial-automaton path
//! (`:303-323`) picks `minOutput` — a value from **`M`'s output alphabet** — and uses
//! it, unencoded, as (a) an encoded input symbol of the transducer and (b) the marker
//! output whose states are deleted from the *result*. Both are category errors that
//! only work by coincidence; both are ported verbatim and pinned by tests below.
//!
//! # Logging
//!
//! This is [`crate::logging::Logging`]'s first real consumer in the port. Java calls
//! the `Main.Logging` statics at five places in this file (`:101-102`, `:236-237`,
//! `:287-291`, `:294`, `:329`); all five are replicated against a threaded
//! `&mut Logging`, in the same order, with the same message text and the same
//! indent/dedent nesting — including the fact that Java leaves the indent
//! **unbalanced** when it throws out of the indented region (a Rust `?` early-return
//! does the same here).
//!
//! # Iteration order
//!
//! `PORTING.md`'s iteration-order trap does not bite here: both Java `HashMap`s
//! (`iterateMapHash`, `statesHash`) are only ever used for whole-key
//! `containsKey`/`get`/`put`, never iterated, so the result is order-independent.
//! The one place order *does* matter — the per-state transition entry set, which
//! `addFirstEntries` reads positionally — is a sorted `Int2ObjectRBTreeMap` in Java
//! and a [`BTreeMap`] here, so the ordering matches.

use crate::automaton::Automaton;
use crate::fa::Fa;
use crate::logging::Logging;
use crate::logicalops;
use crate::word_automaton;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::time::Instant;

/// The errors `Transducer.java` raises, as a typed enum rather than Walnut's
/// stringly-typed `WalnutException` (`PORTING.md`'s type/error mapping table). The
/// first three carry Java's message text verbatim; the fourth has no Java
/// counterpart — see its own doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransduceError {
    /// `Transducer.java:271` — `M.getNS().size() != 1`. (Java's message says the
    /// opposite of what it means; ported verbatim.)
    NotSingleInput,
    /// `Transducer.java:279` — some output value of `M` has no transition out of the
    /// transducer's state `0`.
    IncompatibleAlphabet,
    /// `Transducer.java:343` — `isTotalized` found a state/input pair with more than
    /// one destination.
    MultipleTransitionsPerInput,
    /// **No Java counterpart as a checked error.** `transduceMsdDeterministic` has no
    /// guard at all against a `TRUE_FALSE_AUTOMATON` input: such an automaton has zero
    /// states and an empty `O`, so Java's very first BFS step (`:187-188`,
    /// `M.fa.getO().getInt(currState.state)`) throws an unchecked
    /// `IndexOutOfBoundsException`. `PORTING.md` maps an unchecked exception to a
    /// typed `Result`, so [`Transducer::transduce_msd_deterministic`] rejects it up
    /// front with this variant instead of panicking deep inside the BFS. No input Java
    /// *handles* behaves differently — the only Java caller,
    /// `transduceNonDeterministic`, already rejects a trivial automaton earlier with
    /// [`TransduceError::NotSingleInput`] (a trivial automaton has no tracks at all,
    /// so its track count is `0 != 1`).
    TrivialAutomaton,
}

impl fmt::Display for TransduceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransduceError::NotSingleInput => {
                write!(f, "Automata with only one input can be transduced.")
            }
            TransduceError::IncompatibleAlphabet => write!(
                f,
                "Output alphabet of automaton must be compatible with the transducer input alphabet"
            ),
            TransduceError::MultipleTransitionsPerInput => {
                write!(
                    f,
                    "Automaton must have at most one transition per input per state."
                )
            }
            TransduceError::TrivialAutomaton => write!(
                f,
                "a TRUE/FALSE automaton has no states or tracks and cannot be transduced"
            ),
        }
    }
}

impl std::error::Error for TransduceError {}

/// One BFS state of the transduced automaton: `Transducer.StateTuple`
/// (`Transducer.java:405-438`).
///
/// `i_list` is Java's `iList` — the word `w` of `M`-states this tuple was reached by.
/// It is needed to *compute* successors but is excluded from [`PartialEq`]/[`Hash`],
/// exactly as Java's hand-written `equals`/`hashCode` exclude it. See this module's
/// docs for why that is load-bearing (and for Java's dead `state ^ (state >>> 32)`
/// term, not replicated because it is unobservable).
#[derive(Debug, Clone)]
struct StateTuple {
    state: usize,
    i_list: Vec<usize>,
    /// `[phi_{M.O(h^i(w))} for i in 0..p+q]`, each map represented densely — see
    /// [`Transducer::create_map`].
    iterates: Vec<Vec<usize>>,
}

impl PartialEq for StateTuple {
    fn eq(&self, other: &Self) -> bool {
        // DO NOT compare the string (Java's own comment, `:408`).
        self.state == other.state && self.iterates == other.iterates
    }
}

impl Eq for StateTuple {}

impl Hash for StateTuple {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // DO NOT use the string to hash (Java's own comment, `:433`).
        self.state.hash(state);
        self.iterates.hash(state);
    }
}

/// A deterministic finite-state transducer with all states final that is 1-uniform
/// (`Automata/Transducer.java`).
#[derive(Debug, Clone)]
pub struct Transducer {
    /// The `Automaton` half Java inherits. Only `fa.q`, `fa.q0`, `fa.d` and
    /// `alphabet` are ever read by the transduction; `fa.o` is meaningless here.
    pub automaton: Automaton,
    /// `Transducer.sigma` (`:61`): `sigma[state][encoded input symbol] = output`.
    pub sigma: Vec<BTreeMap<i32, i32>>,
}

impl Transducer {
    /// Builds a transducer from its already-constructed `Automaton` half and output
    /// function. Java's two constructors are the empty one (`:66-69`) and the
    /// file-reading one (`:75-78`, `AutomatonReader.readTransducer`); the latter's
    /// parsing already lives in `wr_io::reader::read_transducer_txt`, so this is the
    /// seam that joins the two — see this module's docs for the exact field mapping.
    ///
    /// `sigma` must have one entry per transducer state; not asserted (this crate's
    /// constructors are non-validating by convention, see [`Automaton::new`]).
    pub fn new(automaton: Automaton, sigma: Vec<BTreeMap<i32, i32>>) -> Self {
        Transducer { automaton, sigma }
    }

    /// `RichAlphabet.encode(List.of(value))` against **this transducer's** alphabet,
    /// including the `indexOf` → `-1` fallback for a value outside the alphabet.
    ///
    /// Java passes a one-element list regardless of how many tracks the transducer
    /// declares, so only track `0` is ever consulted and `encoder[0]` is always `1` —
    /// the encoding is literally `A[0].indexOf(value)`. See this module's docs for why
    /// [`Automaton::encode`] (which panics on an absent digit) cannot be used here.
    ///
    /// # Panics
    ///
    /// If the transducer declares no tracks at all, matching Java's `A.get(0)`
    /// `IndexOutOfBoundsException`.
    fn encode_input(&self, value: i32) -> i32 {
        self.automaton.alphabet[0]
            .iter()
            .position(|&v| v == value)
            .map_or(-1, |i| i as i32)
    }

    /// `Transducer.createIdentityMap(int Q)` (`:380-386`), as a dense vector.
    ///
    /// Java's maps are `Map<Integer, Integer>` whose key set is *always* exactly
    /// `0..Q_T` — `createIdentityMap` seeds it that way and `createMap` rebuilds it
    /// with the same key set every time — so a `Vec<usize>` indexed by transducer
    /// state is an exact, allocation-free representation. Crucially it also preserves
    /// the *equality* semantics `StateTuple`/`iterateMapHash` depend on: two dense
    /// vectors of the same length are equal iff the corresponding Java maps are.
    fn identity_map(&self) -> Vec<usize> {
        (0..self.automaton.fa.q).collect()
    }

    /// `Transducer.createMap(FA M, Integer i, Map mapSoFar)` (`:396-403`) — extends
    /// `map_so_far` by one letter, namely `M`'s output at state `i`: the result maps
    /// each transducer state `j` to `delta_T(map_so_far[j], encode(M.O(i)))`.
    ///
    /// # Panics
    ///
    /// If the transducer has no transition on that encoded symbol from some state, or
    /// has an empty destination list there. Java's `getNfaStateDests(...).getInt(0)`
    /// throws `NullPointerException`/`IndexOutOfBoundsException` in exactly those two
    /// cases, uncaught, and `transduceNonDeterministic`'s only guard against it
    /// (`:276-281`) checks the transducer's state `0` alone — this is one of WB-035's
    /// two confirmed manifestations.
    fn create_map(&self, m_fa: &Fa, i: usize, map_so_far: &[usize]) -> Vec<usize> {
        let encoded = self.encode_input(m_fa.o[i]);
        (0..self.automaton.fa.q)
            .map(|j| {
                self.automaton.fa.d[map_so_far[j]]
                    .get(&encoded)
                    .and_then(|dests| dests.first().copied())
                    .unwrap_or_else(|| {
                        panic!(
                            "Transducer::create_map: transducer state {} has no destination on \
                             encoded input {encoded} (see WB-035)",
                            map_so_far[j]
                        )
                    })
            })
            .collect()
    }

    /// `Transducer.createMapSoFar(FA M, Map identity, List iString)` (`:388-394`) —
    /// `phi_{M.O(iString)}`, i.e. [`Transducer::create_map`] folded left-to-right over
    /// the word, starting from the identity.
    fn create_map_so_far(&self, m_fa: &Fa, identity: &[usize], i_string: &[usize]) -> Vec<usize> {
        let mut map_so_far = identity.to_vec();
        for &i in i_string {
            map_so_far = self.create_map(m_fa, i, &map_so_far);
        }
        map_so_far
    }

    /// `Transducer.createIterates(Automaton M, List string, int size)` (`:359-378`) —
    /// `[phi_{M.O(string)}, phi_{M.O(h(string))}, ..., phi_{M.O(h^{size-1}(string))}]`.
    fn create_iterates(&self, m: &Automaton, string: &[usize], size: usize) -> Vec<Vec<usize>> {
        let mut iterates = Vec::with_capacity(size);
        let identity = self.identity_map();
        let mut dests: Vec<usize> = string.to_vec();

        for i in 0..size {
            iterates.push(self.create_map_so_far(&m.fa, &identity, &dests));
            // Java: `if (i != size - 1)`. Written as `i + 1 != size` so the guard is
            // also correct (rather than underflowing) at `size == 0`, which Java's
            // loop simply never enters.
            if i + 1 != size {
                dests = Self::get_destination_for_dfa(m, &dests);
            }
        }
        iterates
    }

    /// `Transducer.addFirstEntries(Automaton M, Integer integer, List iString)`
    /// (`:251-258`) — appends `h(state)`: the FIRST destination of each of `state`'s
    /// transitions, in encoded-symbol order ("we assuming it's a DFA for now, so this
    /// has length 1 we're assuming...", Java's own comment).
    fn add_first_entries(m: &Automaton, state: usize, i_string: &mut Vec<usize>) {
        for dests in m.fa.d[state].values() {
            i_string.push(dests[0]);
        }
    }

    /// `Transducer.getDestinationForDFA(Automaton M, List prevString)` (`:242-249`) —
    /// `h` applied to a whole word.
    fn get_destination_for_dfa(m: &Automaton, prev_string: &[usize]) -> Vec<usize> {
        let mut i_string = Vec::new();
        for &state in prev_string {
            Self::add_first_entries(m, state, &mut i_string);
        }
        i_string
    }

    /// `Transducer.transduceMsdDeterministic(Automaton M)` (`:99-240`) — transduce an
    /// msd-k automaton as in Dekking (1994). See this module's docs for the
    /// construction.
    ///
    /// `M` must be deterministic and **total** (every state has a destination for
    /// every encoded symbol). Java states this only in `transduceNonDeterministic`'s
    /// doc and enforces it by routing partial automata through the dead-state path
    /// instead; called directly on a partial automaton, both engines misbehave
    /// identically, because `:207-209`/`:213` index `stateMorphed` — a list built by
    /// *position* in the transition entry set — with the encoded symbol `di`. Those
    /// coincide only when the entry set is exactly `0..alphabet_size` ("relying on the
    /// di's to be sorted here...", Java's own comment at `:197`); otherwise Java takes
    /// the wrong destination or throws `IndexOutOfBoundsException`, and this port
    /// takes the same wrong destination or panics on the same slice index.
    ///
    /// # Panics
    ///
    /// Where Java throws an unchecked `NullPointerException`/`IndexOutOfBoundsException`
    /// from an ill-formed transducer — see [`Transducer::create_map`], and the `sigma`
    /// lookup below, whose Java form is an `Integer`-to-`int` unboxing cast (`:187`)
    /// that NPEs on a missing entry.
    pub fn transduce_msd_deterministic(
        &self,
        m: &Automaton,
        logging: &mut Logging,
    ) -> Result<Automaton, TransduceError> {
        // Not a Java guard — see `TransduceError::TrivialAutomaton`. Placed before the
        // timer/logging so the indent stays balanced on this path.
        if m.is_true_false_automaton() {
            return Err(TransduceError::TrivialAutomaton);
        }

        let time_before = Instant::now();
        logging.log_message(&format!(
            "transducing: {} state automaton - {} state transducer",
            m.fa.q, self.automaton.fa.q
        ));
        logging.indent();

        // N will be the returned Automaton, just have to build it up.
        // `M.clonePartialFields(N)` (`Automaton.java:139-146`): the rich alphabet, one
        // number system per track, and the labels *only if* `M` is bound.
        let n_fa = Fa {
            q0: 0,
            q: 0,
            alphabet_size: 0,
            o: Vec::new(),
            d: Vec::new(),
            true_false: None,
        };
        let n_label = if m.is_bound() {
            m.label.clone()
        } else {
            Vec::new()
        };
        let mut n = Automaton::new(n_fa, m.alphabet.clone(), n_label, m.msd.clone());
        // The other half of this crate's per-track `NumberSystem` stand-in
        // (`PORTING.md`'s parallel-vector ruling): Java's single `getNS().add(...)`
        // moves the msd/lsd direction and the all-representations automaton together,
        // so this must too. Guarded rather than asserted because `clonePartialFields`
        // itself tolerates a shorter `NS` list, and hand-built automata in tests may
        // have one.
        if m.all_reps.len() == m.alphabet.len() && m.msd.len() == m.alphabet.len() {
            n.set_all_reps(m.all_reps.clone());
        }

        // ---- Find P and Q so the transducer's transition function becomes ultimately
        // ---- periodic with lag Q and period P.

        // Will be used for hashing the iterate maps.
        let mut iterate_map_hash: HashMap<Vec<Vec<usize>>, usize> = HashMap::new();
        // iterate_strings[i] will be a map from a state q of M to h^i(q).
        let mut iterate_strings: Vec<Vec<Vec<usize>>> = Vec::new();

        let identity = self.identity_map();

        // init_maps[i] is the map phi_{M.O(i)}; init_strings[i] is [i].
        let mut init_maps: Vec<Vec<usize>> = Vec::with_capacity(m.fa.q);
        let mut init_strings: Vec<Vec<usize>> = Vec::with_capacity(m.fa.q);
        for i in 0..m.fa.q {
            init_maps.push(self.create_map(&m.fa, i, &identity));
            init_strings.push(vec![i]);
        }

        iterate_map_hash.insert(init_maps, 0);
        iterate_strings.push(init_strings);

        let mut m_found = 0usize;
        let mut n_found = 0usize;
        let mut found = false;
        let mut iteration = 1usize;
        loop {
            let prev_strings = iterate_strings
                .last()
                .expect("iterate_strings is seeded before the loop")
                .clone();
            debug_assert_eq!(prev_strings.len(), m.fa.q);

            let mut new_maps: Vec<Vec<usize>> = Vec::with_capacity(m.fa.q);
            let mut new_strings: Vec<Vec<usize>> = Vec::with_capacity(m.fa.q);

            for prev in &prev_strings {
                // will be h^m(i)
                let i_string = Self::get_destination_for_dfa(m, prev);
                // start off with the identity.
                new_maps.push(self.create_map_so_far(&m.fa, &identity, &i_string));
                new_strings.push(i_string);
            }

            iterate_strings.push(new_strings);

            if let Some(&previous) = iterate_map_hash.get(&new_maps) {
                n_found = previous;
                m_found = iteration;
                found = true;
            } else {
                iterate_map_hash.insert(new_maps, iteration);
            }

            if found {
                break;
            }
            iteration += 1;
        }

        let p = m_found - n_found;
        let q = n_found;

        // ---- Make the states of the automaton.

        n.fa.q0 = 0;

        let init_state = StateTuple {
            state: m.fa.q0,
            i_list: Vec::new(),
            iterates: self.create_iterates(m, &[], p + q),
        };
        let mut states: Vec<StateTuple> = vec![init_state.clone()];
        let mut states_hash: HashMap<StateTuple, usize> = HashMap::new();
        states_hash.insert(init_state.clone(), 0);
        let mut states_queue: VecDeque<StateTuple> = VecDeque::new();
        states_queue.push_back(init_state);

        while let Some(curr_state) = states_queue.pop_front() {
            // set up the output of this state.
            let transducer_state = curr_state.iterates[0][self.automaton.fa.q0];
            let encoded = self.encode_input(m.fa.o[curr_state.state]);
            let output = *self.sigma[transducer_state]
                .get(&encoded)
                .unwrap_or_else(|| {
                    panic!(
                        "Transducer::transduce_msd_deterministic: sigma has no output for \
                         transducer state {transducer_state} on encoded input {encoded}"
                    )
                });
            n.fa.o.push(output);
            n.fa.d.push(BTreeMap::new());

            // get h(w) where w = currState.iList.
            let new_string = Self::get_destination_for_dfa(m, &curr_state.i_list);

            // relying on the di's to be sorted here...
            let mut state_morphed: Vec<usize> = Vec::new();
            Self::add_first_entries(m, curr_state.state, &mut state_morphed);

            // look at the states that this state transitions to.
            let symbols: Vec<i32> = m.fa.d[curr_state.state].keys().copied().collect();
            for di in symbols {
                let di_index = di as usize;

                // make new state string
                let mut new_state_string = new_string.clone();
                new_state_string.extend_from_slice(&state_morphed[..di_index]);

                // new state
                let new_state = StateTuple {
                    state: state_morphed[di_index],
                    iterates: self.create_iterates(m, &new_state_string, p + q),
                    i_list: new_state_string,
                };

                // check if the state is already hashed.
                let destination = match states_hash.get(&new_state) {
                    Some(&index) => index,
                    None => {
                        states.push(new_state.clone());
                        states_queue.push_back(new_state.clone());
                        let index = states.len() - 1;
                        states_hash.insert(new_state, index);
                        index
                    }
                };

                // set up the transition. `FA.addNewTransition` (`FA.java:556-561`):
                // "note that this will overwrite previous transitions if it exists" —
                // it cannot here, since `di` ranges over distinct keys of one map.
                let source = n.fa.d.len() - 1;
                n.fa.d[source].insert(di, vec![destination]);
            }
        }

        n.fa.q = states.len();
        n.fa.alphabet_size = m.fa.alphabet_size;

        word_automaton::minimize_self_with_output(&mut n);

        logging.dedent();
        logging.log_message(&format!(
            "transduced: {} states - {}ms",
            n.fa.q,
            time_before.elapsed().as_millis()
        ));

        Ok(n)
    }

    /// `Transducer.isTotalized(FA fa)` (`:334-348`).
    fn is_totalized(fa: &Fa) -> Result<bool, TransduceError> {
        let mut totalized = true;
        for q in 0..fa.q {
            for x in 0..fa.alphabet_size as i32 {
                match fa.d[q].get(&x) {
                    None => totalized = false,
                    // An EMPTY destination list falls through both of Java's branches
                    // (`iList == null` fails, `iList.size() > 1` fails) and leaves
                    // `totalized` untouched — ported as written.
                    Some(dests) if dests.len() > 1 => {
                        return Err(TransduceError::MultipleTransitionsPerInput)
                    }
                    Some(_) => {}
                }
            }
        }
        Ok(totalized)
    }

    /// `Transducer.transduceNonDeterministic(Automaton M)` (`:268-332`) — transduce an
    /// automaton that may have undefined transitions, as in Dekking (1994). The
    /// automaton may not have more than one transition per input character per state.
    ///
    /// **`m` is mutated**, faithfully: Java's `WordAutomaton.reverseWithOutput(M, true)`
    /// at `:290` rewrites the caller's automaton in place when its number system is
    /// lsd (the sole Java caller, `Prover.transduceCommand`, never reads `M` again).
    ///
    /// # `M.getNS().get(0).isMsd()` and this crate's `msd: Vec<Option<bool>>`
    ///
    /// Java reads a `NumberSystem` object and would NPE if the track had none. This
    /// crate stores the derived fact as an `Option<bool>` (see [`Automaton::msd`]), so
    /// the `None` ("no number system / non-arithmetic track") case has no Java
    /// counterpart to be faithful to. It is treated as msd — i.e. the same
    /// "skip the numeration-specific handling" reading `crate::quantify` already gives
    /// `None` — rather than as a panic, since nothing downstream of the reversal needs
    /// a number system.
    pub fn transduce_non_deterministic(
        &self,
        m: &mut Automaton,
        logging: &mut Logging,
    ) -> Result<Automaton, TransduceError> {
        // check that the input automaton only has one input!
        if m.msd.len() != 1 {
            return Err(TransduceError::NotSingleInput);
        }

        // Check that the output alphabet of the automaton is compatible with the input
        // alphabet of the transducer. NOTE (ported verbatim): Java checks the
        // transducer's state `0` only — not its `q0`, and not every state. See
        // `create_map`'s doc and WB-035 for the crash that leaves reachable.
        for &output in &m.fa.o {
            let encoded = self.encode_input(output);
            if !self.automaton.fa.d[0].contains_key(&encoded) {
                return Err(TransduceError::IncompatibleAlphabet);
            }
        }

        // make sure the number system is lsd.
        let mut to_lsd = false;

        if m.msd[0] == Some(false) {
            logging.log_message("Automaton number system is lsd, reversing");
            to_lsd = true;
            logging.indent();
            word_automaton::reverse_with_output(m, true);
            logging.dedent();
        }

        logging.indent();

        // verify that the automaton is indeed nondeterministic, i.e. it has undefined
        // transitions. If it is not, transduce normally.
        let totalized = Self::is_totalized(&m.fa)?;
        let mut n;
        if totalized {
            // transduce normally
            n = self.transduce_msd_deterministic(m, logging)?;
        } else {
            let mut m_new = m.clone();
            m_new.fa.add_distinguished_dead_state();

            // after transducing, all states with this minimum output will be removed.
            let min_output = m_new.fa.determine_min_output();

            let mut t_new = self.clone();

            // WB-035, half one: `min_output` is an output value of `M`, used here
            // straight as an ENCODED INPUT SYMBOL of the transducer, with none of the
            // `encode_input` indirection every other site in this file applies.
            for q in 0..t_new.automaton.fa.q {
                t_new.automaton.fa.d[q].insert(min_output, vec![q]);
                t_new.sigma[q].insert(min_output, min_output);
            }

            n = t_new.transduce_msd_deterministic(&m_new, logging)?;

            // WB-035, half two: and here as a marker in the RESULT's output alphabet,
            // which is the transducer's, not `M`'s — so a transducer that can
            // legitimately emit `min_output` has its real states deleted.
            logicalops::remove_states_with_output_rebuild(&mut n.fa, min_output);
            n.force_canonize();
        }

        if to_lsd {
            word_automaton::reverse_with_output(&mut n, true);
        }

        logging.dedent();

        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A single-track `msd_2`-shaped DFAO with the given per-state outputs and
    /// transition table; `d[state]` lists `(symbol, destination)` pairs, so a state
    /// can be left partial (the shape `transduce_non_deterministic`'s dead-state path
    /// exists for).
    fn word_automaton(outputs: &[i32], d: &[&[(i32, usize)]]) -> Automaton {
        assert_eq!(outputs.len(), d.len());
        let table: Vec<BTreeMap<i32, Vec<usize>>> = d
            .iter()
            .map(|row| row.iter().map(|&(sym, dest)| (sym, vec![dest])).collect())
            .collect();
        let fa = Fa {
            q0: 0,
            q: outputs.len(),
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

    /// A transducer over the single-track input alphabet `alphabet`. `rows[state]`
    /// lists `(input value, destination, output)` triples — input *values*, which this
    /// helper encodes to symbols exactly the way `readTransducer` does.
    fn transducer(alphabet: &[i32], rows: &[&[(i32, usize, i32)]]) -> Transducer {
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::new();
        let mut sigma: Vec<BTreeMap<i32, i32>> = Vec::new();
        for row in rows {
            let mut d_row = BTreeMap::new();
            let mut sigma_row = BTreeMap::new();
            for &(value, dest, output) in row.iter() {
                let sym = alphabet
                    .iter()
                    .position(|&v| v == value)
                    .expect("test transducer transition on an out-of-alphabet value")
                    as i32;
                d_row.insert(sym, vec![dest]);
                sigma_row.insert(sym, output);
            }
            d.push(d_row);
            sigma.push(sigma_row);
        }
        let fa = Fa {
            q0: 0,
            q: rows.len(),
            alphabet_size: alphabet.len(),
            // `readTransducer` discards state outputs ("state output does not matter
            // for transducers").
            o: vec![0; rows.len()],
            d,
            true_false: None,
        };
        let automaton = Automaton::new(
            fa,
            vec![alphabet.to_vec()],
            vec!["0".to_string()],
            vec![None],
        );
        Transducer::new(automaton, sigma)
    }

    /// `Transducer Library/RUNSUM2.txt` — the running-sum-mod-2 transducer.
    fn runsum2() -> Transducer {
        transducer(&[0, 1], &[&[(0, 0, 0), (1, 1, 1)], &[(0, 1, 1), (1, 0, 0)]])
    }

    /// `unitTests/T.txt` — the Thue-Morse sequence as a 2-state `msd_2` DFAO.
    fn thue_morse() -> Automaton {
        word_automaton(&[0, 1], &[&[(0, 0), (1, 1)], &[(0, 1), (1, 0)]])
    }

    /// Walks a deterministic word automaton and returns the output at the state
    /// reached by `word`, or `None` if the walk falls off a missing transition.
    fn word_output(a: &Automaton, word: &[i32]) -> Option<i32> {
        let mut state = a.fa.q0;
        for sym in word {
            state = *a.fa.d[state].get(sym)?.first()?;
        }
        Some(a.fa.o[state])
    }

    fn transition_count(a: &Automaton) -> usize {
        a.fa.d.iter().map(|row| row.len()).sum()
    }

    // -------------------------------------------------------------------
    // TransducerTest.java
    // -------------------------------------------------------------------

    /// Replicates `TransducerTest.testTransducerRUNSUM2_T` — the RUNSUM2 transducer
    /// applied to the Thue-Morse word automaton, asserting Java's exact expected
    /// transition table (an 8-state result).
    #[test]
    fn transduce_runsum2_over_thue_morse_matches_java() {
        let mut logging = Logging::new();
        let mut m = thue_morse();
        assert_eq!(m.fa.q, 2);

        let c = runsum2()
            .transduce_non_deterministic(&mut m, &mut logging)
            .expect("Thue-Morse is total and single-track");

        // Java: "[{0=>[0], 1=>[1]}, {0=>[2], 1=>[3]}, {0=>[4], 1=>[5]}, {0=>[6],
        // 1=>[7]}, {0=>[4], 1=>[5]}, {0=>[6], 1=>[7]}, {0=>[0], 1=>[1]},
        // {0=>[2], 1=>[3]}]"
        let expected: [[usize; 2]; 8] = [
            [0, 1],
            [2, 3],
            [4, 5],
            [6, 7],
            [4, 5],
            [6, 7],
            [0, 1],
            [2, 3],
        ];
        assert_eq!(c.fa.q, expected.len());
        let actual: Vec<[usize; 2]> = (0..c.fa.q)
            .map(|q| [c.fa.d[q][&0][0], c.fa.d[q][&1][0]])
            .collect();
        assert_eq!(actual, expected.to_vec());
    }

    /// The same result, checked semantically rather than structurally (`CLAUDE.md`'s
    /// prime directive): RUNSUM2 emits the running sum mod 2 of Thue-Morse, so the
    /// output on the base-2 representation of `n` must be `t(0) + ... + t(n) mod 2`.
    #[test]
    fn transduce_runsum2_over_thue_morse_computes_the_running_sum_mod_2() {
        let mut logging = Logging::new();
        let mut m = thue_morse();
        let c = runsum2()
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();

        // Thue-Morse t(n) = parity of the popcount of n; the input automaton reads the
        // msd-first base-2 representation.
        let mut running = 0i32;
        for n in 0u32..32 {
            running = (running + (n.count_ones() % 2) as i32) % 2;
            let word: Vec<i32> = format!("{n:b}")
                .bytes()
                .map(|b| (b - b'0') as i32)
                .collect();
            assert_eq!(
                word_output(&c, &word),
                Some(running),
                "running sum mod 2 at n = {n}"
            );
        }
    }

    /// A 1-uniform transducer applied to an automaton it does not change: the identity
    /// transducer must reproduce the input's per-word outputs exactly.
    #[test]
    fn identity_transducer_preserves_the_sequence() {
        let mut logging = Logging::new();
        let identity = transducer(&[0, 1], &[&[(0, 0, 0), (1, 0, 1)]]);
        let mut m = thue_morse();
        let c = identity
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();
        for n in 0u32..16 {
            let word: Vec<i32> = format!("{n:b}")
                .bytes()
                .map(|b| (b - b'0') as i32)
                .collect();
            assert_eq!(
                word_output(&c, &word),
                Some((n.count_ones() % 2) as i32),
                "identity transduction at n = {n}"
            );
        }
    }

    // -------------------------------------------------------------------
    // Trivial (TRUE/FALSE) automata
    // -------------------------------------------------------------------

    #[test]
    fn transducing_a_true_automaton_is_rejected_not_a_panic() {
        let mut logging = Logging::new();
        let mut m = Automaton::true_false(true);
        // A trivial automaton has no tracks at all, so Java's `getNS().size() != 1`
        // guard fires first -- this is how Walnut itself keeps the zero-state
        // automaton out of `transduceMsdDeterministic`.
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::NotSingleInput
        );
        assert_eq!(
            runsum2()
                .transduce_msd_deterministic(&m, &mut logging)
                .unwrap_err(),
            TransduceError::TrivialAutomaton
        );
    }

    #[test]
    fn transducing_a_false_automaton_is_rejected_not_a_panic() {
        let mut logging = Logging::new();
        let mut m = Automaton::true_false(false);
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::NotSingleInput
        );
        assert_eq!(
            runsum2()
                .transduce_msd_deterministic(&m, &mut logging)
                .unwrap_err(),
            TransduceError::TrivialAutomaton
        );
    }

    // -------------------------------------------------------------------
    // The other guards
    // -------------------------------------------------------------------

    #[test]
    fn a_two_track_automaton_is_rejected() {
        let mut logging = Logging::new();
        let mut m = thue_morse();
        m.msd = vec![Some(true), Some(true)];
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::NotSingleInput
        );
    }

    #[test]
    fn an_output_outside_the_transducer_alphabet_is_rejected() {
        let mut logging = Logging::new();
        // Outputs {0, 7}; RUNSUM2's input alphabet is {0, 1}.
        let mut m = word_automaton(&[0, 7], &[&[(0, 0), (1, 1)], &[(0, 1), (1, 0)]]);
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::IncompatibleAlphabet
        );
    }

    #[test]
    fn a_genuinely_nondeterministic_automaton_is_rejected() {
        let mut logging = Logging::new();
        let mut m = thue_morse();
        m.fa.d[0].insert(0, vec![0, 1]);
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::MultipleTransitionsPerInput
        );
    }

    // -------------------------------------------------------------------
    // The partial-automaton (dead state) path, and WB-035
    // -------------------------------------------------------------------

    /// State `0` has no transition on symbol `1`, so `isTotalized` is false and the
    /// dead-state path runs. The transducer here emits `5`/`1`, neither of which
    /// collides with `min(M.O) - 1 == -1`, so the result is CORRECT: it keeps exactly
    /// the input's defined transitions, with outputs relabelled `0 -> 5`, `1 -> 1`.
    ///
    /// Empirically cross-checked against the real `walnut-java` CLI (2026-08-13,
    /// `target/Walnut-all.jar`), which produces `0 5 / 0 -> 1`, `1 1 / 0 -> 1, 1 -> 0`
    /// — three transitions.
    #[test]
    fn partial_automaton_transduces_through_the_dead_state_path() {
        let mut logging = Logging::new();
        let t = transducer(&[0, 1], &[&[(0, 0, 5), (1, 0, 1)]]);
        let mut m = word_automaton(&[0, 1], &[&[(0, 1)], &[(0, 1), (1, 0)]]);

        let c = t.transduce_non_deterministic(&mut m, &mut logging).unwrap();

        assert_eq!(c.fa.q, 2);
        assert_eq!(transition_count(&c), 3);
        assert_eq!(word_output(&c, &[]), Some(5));
        assert_eq!(word_output(&c, &[0]), Some(1));
        assert_eq!(word_output(&c, &[0, 1]), Some(5));
        // The one genuinely undefined transition of the input stays undefined.
        assert_eq!(word_output(&c, &[1]), None);
    }

    /// **WB-035** (`docs/WALNUT-BUGS.md`), half two. Literally the same input
    /// automaton as the test above, and a transducer differing only in ONE output
    /// value: `5` becomes `-1`, which happens to equal `min(M.O) - 1`, the marker the
    /// dead-state path uses. Real, reachable states are then silently deleted — here
    /// the `1 -> 0` transition, which the input defines and which the sibling test
    /// above (identical in every other respect) keeps.
    ///
    /// This asserts the WRONG answer, matching real `walnut-java`'s empirically
    /// confirmed output, per `CLAUDE.md`'s mechanical-port rule.
    #[test]
    fn wb035_partial_automaton_loses_states_whose_output_collides_with_the_marker() {
        let mut logging = Logging::new();
        let t = transducer(&[0, 1], &[&[(0, 0, -1), (1, 0, 1)]]);
        let mut m = word_automaton(&[0, 1], &[&[(0, 1)], &[(0, 1), (1, 0)]]);

        let c = t.transduce_non_deterministic(&mut m, &mut logging).unwrap();

        // Faithful (buggy) result: `0 -1 / 0 -> 1`, `1 1 / 0 -> 1` -- TWO transitions,
        // not the three the sibling test gets. The mathematically correct answer would
        // additionally have `1 -> 0` from the output-1 state.
        assert_eq!(c.fa.q, 2);
        assert_eq!(transition_count(&c), 2);
        assert_eq!(word_output(&c, &[]), Some(-1));
        assert_eq!(word_output(&c, &[0]), Some(1));
        assert_eq!(
            word_output(&c, &[0, 1]),
            None,
            "WB-035: this transition is real in the input but is deleted by the marker collision"
        );
    }

    /// **WB-035**, half one: the same root cause reached through the *input* side. The
    /// transducer's alphabet is `{1, 2}` and `M`'s outputs are `{1, 2}`, so the
    /// compatibility check passes — but `min(M.O) - 1 == 0` is installed as a raw
    /// encoded symbol (`0`, i.e. the letter `1`, clobbering a real transition), while
    /// `create_map` looks the dead state's output up via `encode([0]) == -1`. Real
    /// Walnut throws `NullPointerException` at `Transducer.java:400` (empirically
    /// confirmed, 2026-08-13); this port panics at the same point.
    #[test]
    #[should_panic(expected = "no destination on encoded input -1")]
    fn wb035_shifted_alphabet_panics_where_java_npes() {
        let mut logging = Logging::new();
        let t = transducer(&[1, 2], &[&[(1, 0, 7), (2, 0, 8)]]);
        let mut m = word_automaton(&[1, 2], &[&[(0, 1)], &[(0, 1), (1, 0)]]);
        let _ = t.transduce_non_deterministic(&mut m, &mut logging);
    }

    // -------------------------------------------------------------------
    // lsd input (the `reverseWithOutput` round trip)
    // -------------------------------------------------------------------

    /// An `lsd_2` input takes the reverse-transduce-reverse path (`:286-292`,
    /// `:325-327`). Applied to a transducer that only relabels outputs (single-state,
    /// so the reversal cannot change which output a word gets), the result must agree
    /// with relabelling the input directly.
    #[test]
    fn lsd_input_round_trips_through_reverse_with_output() {
        let mut logging = Logging::new();
        let relabel = transducer(&[0, 1], &[&[(0, 0, 9), (1, 0, 4)]]);
        let mut m = thue_morse();
        m.msd = vec![Some(false)];

        let c = relabel
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();

        // Reversal restores the original direction, so `m` is back to lsd on the way
        // out; check the outputs against Thue-Morse read lsd-first (which, for the
        // popcount parity, is the same value either way).
        for n in 0u32..16 {
            let word: Vec<i32> = format!("{n:b}")
                .bytes()
                .rev()
                .map(|b| (b - b'0') as i32)
                .collect();
            let expected = if n.count_ones() % 2 == 0 { 9 } else { 4 };
            assert_eq!(
                word_output(&c, &word),
                Some(expected),
                "lsd input at n = {n}"
            );
        }
        // Faithful side effect: only the RESULT is reversed back (`:325-327`). The
        // caller's `M` is left rewritten as the msd automaton `:290` turned it into --
        // Java mutates its argument and never restores it, and `Prover.
        // transduceCommand`, its only caller, never looks at `M` again.
        assert_eq!(
            m.msd,
            vec![Some(true)],
            "M is left REVERSED in place; Java never reverses it back"
        );
    }

    // -------------------------------------------------------------------
    // Logging (this module is `Logging`'s first real consumer)
    // -------------------------------------------------------------------

    /// With details enabled, the two `Logging.logMessage` calls in
    /// `transduceMsdDeterministic` reach the detailed-log buffer, indented one level
    /// deeper by `transduceNonDeterministic`'s own `indent()` (`:294`), and the
    /// closing line is emitted at the outer level again (`dedent()` precedes it,
    /// `:236-237`).
    #[test]
    fn transduction_writes_javas_log_lines() {
        let mut logging = Logging::new();
        logging.configure_for_command(false, true);
        let mut m = thue_morse();
        runsum2()
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();

        let log = logging.detailed_log();
        assert!(
            log.contains(" transducing: 2 state automaton - 2 state transducer"),
            "missing/unindented opening line in:\n{log}"
        );
        assert!(
            log.contains("transduced: 8 states - "),
            "missing closing line in:\n{log}"
        );
        assert!(
            log.contains("ms"),
            "closing line should carry an elapsed time in:\n{log}"
        );
    }

    /// The lsd path logs its own line before reversing (`:287`).
    #[test]
    fn lsd_input_logs_the_reversal() {
        let mut logging = Logging::new();
        logging.configure_for_command(false, true);
        let mut m = thue_morse();
        m.msd = vec![Some(false)];
        transducer(&[0, 1], &[&[(0, 0, 0), (1, 0, 1)]])
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();

        assert!(
            logging
                .detailed_log()
                .contains("Automaton number system is lsd, reversing"),
            "missing lsd line in:\n{}",
            logging.detailed_log()
        );
    }

    // -------------------------------------------------------------------
    // The pieces of the construction
    // -------------------------------------------------------------------

    #[test]
    fn encode_input_returns_minus_one_for_an_out_of_alphabet_value() {
        let t = runsum2();
        assert_eq!(t.encode_input(0), 0);
        assert_eq!(t.encode_input(1), 1);
        // Java's `List.indexOf` contract, which the dead-state path depends on.
        assert_eq!(t.encode_input(-1), -1);
        assert_eq!(t.encode_input(42), -1);
    }

    #[test]
    fn encode_input_uses_alphabet_position_not_the_value() {
        let t = transducer(&[3, 7], &[&[(3, 0, 0), (7, 0, 1)]]);
        assert_eq!(t.encode_input(3), 0);
        assert_eq!(t.encode_input(7), 1);
        assert_eq!(t.encode_input(0), -1);
    }

    #[test]
    fn state_tuple_equality_and_hashing_ignore_the_word() {
        let a = StateTuple {
            state: 1,
            i_list: vec![0, 1, 0],
            iterates: vec![vec![0, 1]],
        };
        let b = StateTuple {
            state: 1,
            i_list: vec![1, 1],
            iterates: vec![vec![0, 1]],
        };
        let c = StateTuple {
            state: 2,
            i_list: vec![0, 1, 0],
            iterates: vec![vec![0, 1]],
        };
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut seen: HashMap<StateTuple, usize> = HashMap::new();
        seen.insert(a.clone(), 0);
        assert_eq!(seen.get(&b), Some(&0), "differing words must collide");
        assert_eq!(seen.get(&c), None, "a differing state must not");
    }

    #[test]
    fn create_map_composes_left_to_right() {
        let t = runsum2();
        let m = thue_morse();
        let identity = t.identity_map();
        assert_eq!(identity, vec![0, 1]);

        // M state 0 has output 0; RUNSUM2 on letter 0 is the identity on states.
        assert_eq!(t.create_map(&m.fa, 0, &identity), vec![0, 1]);
        // M state 1 has output 1; RUNSUM2 on letter 1 swaps the two states.
        assert_eq!(t.create_map(&m.fa, 1, &identity), vec![1, 0]);
        // Composing letter 1 twice is the identity again.
        assert_eq!(t.create_map_so_far(&m.fa, &identity, &[1, 1]), vec![0, 1]);
        assert_eq!(
            t.create_map_so_far(&m.fa, &identity, &[1, 0, 1]),
            vec![0, 1]
        );
    }

    #[test]
    fn get_destination_for_dfa_applies_the_morphism_in_symbol_order() {
        let m = thue_morse();
        // h(0) = 0 1, h(1) = 1 0.
        assert_eq!(Transducer::get_destination_for_dfa(&m, &[0]), vec![0, 1]);
        assert_eq!(Transducer::get_destination_for_dfa(&m, &[1]), vec![1, 0]);
        assert_eq!(
            Transducer::get_destination_for_dfa(&m, &[0, 1]),
            vec![0, 1, 1, 0]
        );
        assert_eq!(
            Transducer::get_destination_for_dfa(&m, &[]),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn create_iterates_of_the_empty_word_is_all_identities() {
        let t = runsum2();
        let m = thue_morse();
        assert_eq!(t.create_iterates(&m, &[], 3), vec![vec![0, 1]; 3]);
    }

    #[test]
    fn is_totalized_distinguishes_the_three_shapes() {
        assert_eq!(Transducer::is_totalized(&thue_morse().fa), Ok(true));

        let partial = word_automaton(&[0, 1], &[&[(0, 1)], &[(0, 1), (1, 0)]]);
        assert_eq!(Transducer::is_totalized(&partial.fa), Ok(false));

        let mut nfa = thue_morse();
        nfa.fa.d[1].insert(1, vec![0, 1]);
        assert_eq!(
            Transducer::is_totalized(&nfa.fa),
            Err(TransduceError::MultipleTransitionsPerInput)
        );
    }

    #[test]
    fn transduce_preserves_the_input_label_when_bound() {
        let mut logging = Logging::new();
        let mut m = thue_morse();
        m.bind(vec!["n".to_string()]);
        let c = runsum2()
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();
        assert_eq!(c.label, vec!["n".to_string()]);
        assert_eq!(c.alphabet, vec![vec![0, 1]]);
        assert_eq!(c.msd, vec![Some(true)]);
    }
}
