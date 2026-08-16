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
//! # Cost, and the deterministic work budget (a port-specific divergence)
//!
//! Neither of Java's two loops is bounded. The first runs until the iterate-vector
//! sequence repeats, and its lag-plus-period `p + q` is capped only by the number of
//! distinct vectors of maps `S_T -> S_T` over `M`'s states, i.e. `Q_T^(Q_T · Q_M)`; a BFS
//! state is then a pair `(M-state, [phi_i]_{i<p+q})`, so the state count is capped only by
//! `Q_M · Q_T^(Q_T · (p+q))`. On top of that — and this is the term that actually bites —
//! the words these loops carry (`create_iterates`' `dests`, and a `StateTuple`'s `i_list`)
//! grow by a factor of `M`'s out-degree per level, and *every letter of the word costs a
//! `create_map` call*. So the real driver is exponential in BFS **depth**, not in `Q_M` or
//! `Q_T`: the shipped two-state `RUNSUM2` against an eight-state input DFAO already runs
//! for tens of seconds, and a four-state input over a three-letter alphabet through a
//! four-state transducer runs for minutes and hundreds of megabytes. A guard keyed on
//! either automaton's *state count* — the only two numbers a caller has cheaply in hand
//! before the call — therefore does not bound this at all; it is off by orders of
//! magnitude, on the wrong axis.
//!
//! Nor can a caller wrap this in a wall-clock watchdog: [`Automaton`] is unconditionally
//! `!Send` (it carries `Rc`s in `all_reps`), so the computation cannot be moved to a
//! thread that the caller could abandon on a deadline.
//!
//! So the cap lives **here**, inside the primitive's own loops, as a deterministic
//! [`TransduceBudget`]: a ceiling on `create_map` calls (the innermost unit of work, and
//! the quantity every other cost is proportional to), on the BFS state count actually
//! explored, and on the length of the words carried around (which bounds peak memory,
//! since word storage is what the megabytes above are). Exceeding any of the three is
//! [`TransduceError::Exploded`] — `CLAUDE.md`'s "per-test resource caps, never hangs …
//! `EXPLODED` verdict" — never a hang and never an OOM.
//!
//! **This is a deliberate, documented divergence from Java**, and the only one in this
//! file that is not a ported quirk: real Walnut, given long enough and enough heap, would
//! eventually answer where this returns `Exploded`. It is *not* logged in
//! `docs/WALNUT-BUGS.md`, because it is not a Java defect — Walnut's unbounded loops are
//! the faithful behavior and are still exactly what runs inside the budget. The budget's
//! defaults ([`TransduceBudget::default`]) are set two-plus orders of magnitude above
//! every fixture in this repo and every transducer/word-automaton pair in Walnut's own
//! libraries, so nothing Walnut is realistically used for is rejected; see that impl for
//! the arithmetic behind each number. A caller that genuinely wants Java's unbounded
//! behavior can pass its own budget to
//! [`Transducer::transduce_non_deterministic_with_budget`].
//!
//! # WB-035: `minOutput` is used both as an encoded INPUT symbol and as an OUTPUT marker
//!
//! See `docs/WALNUT-BUGS.md`. `transduceNonDeterministic`'s partial-automaton path
//! (`:303-323`) picks `minOutput` — a value from **`M`'s output alphabet** — and uses
//! it, unencoded, as (a) an encoded input symbol of the transducer and (b) the marker
//! output whose states are deleted from the *result*. Both are category errors that
//! only work by coincidence; both are ported verbatim and pinned by tests below.
//!
//! # WB-034: a track with no number system NPEs before the transduction even starts
//!
//! See `docs/WALNUT-BUGS.md` and [`Transducer::transduce_non_deterministic`]'s doc.
//! `Transducer.java:286` dereferences `M.getNS().get(0)` unguarded, and that entry is
//! `null` for any track declared with an explicit `{…}` alphabet. Ported verbatim as
//! [`TransduceError::NoNumberSystem`].
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
/// first three carry Java's message text verbatim; [`TransduceError::NoNumberSystem`]
/// carries Java's `NullPointerException` text verbatim (WB-034); the last has no Java
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
    /// **WB-034** (`docs/WALNUT-BUGS.md`) — `Transducer.java:286`'s
    /// `M.getNS().get(0).isMsd()` dereferences a `null` `NumberSystem`, which is what
    /// `ParseMethods.parseAlphabetDeclaration` puts in `NS` for a track declared with an
    /// explicit alphabet (`{0,1}`) rather than `msd_k`/`lsd_k`. This crate's stand-in
    /// for that `null` is `Automaton::msd[0] == None`, so the guard is `is_none()`.
    ///
    /// Ported verbatim as a *rejection*, not a divergence: Java's NPE is an unchecked
    /// `RuntimeException` that `Prover.dispatch`'s top-level `catch (RuntimeException)`
    /// recovers from — the message prints and the session continues — so a `Result::Err`
    /// whose `Display` is Java's own NPE text is more faithful than a Rust `panic!`
    /// would be (this port has no `catch_unwind` boundary; a panic would kill the
    /// process). Same treatment, and the same defect class, as WB-033/WB-013.
    NoNumberSystem,
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
    /// **WB-035**, half one, as a *rejection* rather than a panic.
    /// `Transducer.createMap` (`:400`) does
    /// `getNfaStateDests(mapSoFar.get(j), encoded).getInt(0)` with no null check, so a
    /// transducer that has no transition on the encoded symbol it is asked for throws
    ///
    /// ```text
    /// java.lang.NullPointerException: Cannot invoke
    ///   "it.unimi.dsi.fastutil.ints.IntList.getInt(int)" because the return value of
    ///   "Automata.FA.Transitions.getNfaStateDests(int, int)" is null
    /// ```
    ///
    /// which `Prover.dispatch`'s top-level `catch (RuntimeException)` prints and
    /// recovers from — the REPL keeps going. Two inputs reach it, both from ordinary
    /// hand-authored library files: WB-035's shifted-alphabet dead-state path, and a
    /// **partial** (well-formed but non-total) `Transducer Library/*.txt` — a state
    /// missing a transition on one letter of its own declared alphabet. Since `transduce`
    /// is now reachable from user-supplied files (U26), a Rust `panic!` here would unwind
    /// out of a REPL that has no `catch_unwind` and kill the whole session, which is a
    /// strictly worse divergence than Java's. Same treatment and the same reasoning as
    /// [`TransduceError::NoNumberSystem`] (WB-034) and WB-033/WB-013.
    ///
    /// One sub-case is knowingly approximated: an *empty* destination list (rather than a
    /// missing one) is `getInt(0)` on an empty `IntList`, i.e. Java's
    /// `IndexOutOfBoundsException` with different message text, not this NPE. No `.txt`
    /// this crate can read produces an empty destination list, so the two are folded into
    /// one variant rather than splitting a message no input can observe.
    NoTransducerTransition,
    /// The same defect class one line earlier in the algorithm: the BFS's per-state
    /// output (`Transducer.java:187`) is `(int) sigma.get(s).get(encoded)`, an
    /// `Integer`-to-`int` unboxing cast that throws
    ///
    /// ```text
    /// java.lang.NullPointerException: Cannot invoke "java.lang.Integer.intValue()"
    ///   because the return value of "java.util.Map.get(Object)" is null
    /// ```
    ///
    /// when `sigma` has no output for that (state, symbol) pair — the `sigma` twin of
    /// [`TransduceError::NoTransducerTransition`]'s transition-table hole, reachable from
    /// the same partial-transducer files. Rejected rather than panicked for the same
    /// reason.
    NoTransducerOutput,
    /// **Port-specific; no Java counterpart.** The [`TransduceBudget`] ran out — see this
    /// module's "Cost" docs for why the cap has to live inside these loops and why this
    /// is a deliberate divergence rather than a logged Walnut bug.
    Exploded(TransduceLimit),
}

/// Which ceiling of a [`TransduceBudget`] was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransduceLimit {
    /// [`TransduceBudget::max_map_steps`].
    MapSteps,
    /// [`TransduceBudget::max_bfs_states`].
    BfsStates,
    /// [`TransduceBudget::max_word_len`].
    WordLength,
}

impl fmt::Display for TransduceLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransduceLimit::MapSteps => write!(f, "transducer-map composition steps"),
            TransduceLimit::BfsStates => write!(f, "states explored"),
            TransduceLimit::WordLength => write!(f, "intermediate word length"),
        }
    }
}

/// The deterministic work budget [`Transducer::transduce_msd_deterministic`] spends.
///
/// See this module's "Cost" docs for why this exists and why it is checked inside the
/// loops rather than by the caller. All three ceilings are *deterministic* — the same
/// input always spends exactly the same budget, on any machine — so a budget-exhaustion
/// verdict is reproducible and testable, unlike a wall-clock timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransduceBudget {
    /// Total `createMap` compositions allowed across the whole transduction. This is the
    /// innermost unit of work and every other cost in the algorithm is proportional to
    /// it: the periodicity search spends one per letter of each iterate word per `M`
    /// state, and each BFS state spends one per letter of each of its `p + q` iterates.
    /// Bounding it therefore bounds *time* (each step is `Q_T` transition lookups) and,
    /// transitively, the words that were charged for (peak *memory*).
    pub max_map_steps: u64,
    /// Ceiling on the number of BFS states actually explored — `states.len()`, the size of
    /// the automaton being built, **not** the input's or the transducer's state count.
    pub max_bfs_states: usize,
    /// Ceiling on the length of any single intermediate word of `M`-states (`iList`,
    /// `createIterates`' `dests`). Caps a single allocation, so peak memory cannot
    /// overshoot between two budget checks.
    pub max_word_len: usize,
}

impl Default for TransduceBudget {
    /// Generous enough that no realistic Walnut input is rejected, tight enough that the
    /// worst case is seconds rather than unbounded.
    ///
    /// * `max_map_steps = 10_000_000`. Measured on this crate's own fixtures (release,
    ///   2026-08-15): ~19.5 million steps/second, so the ceiling is **~0.5 s** of release
    ///   CPU (~7 s in an unoptimized debug build), and — since a word can only be as long
    ///   as the steps already charged for it — at most ~80 MB of live word storage. For
    ///   scale, the two end-to-end pairs this repo ships (`RUNSUM2` over Thue-Morse,
    ///   `RUNSUM2` over the `lsd_2` paperfolding word — the same pairs
    ///   `IntegrationTest.java` uses) spend under `2^10` and `2^12` steps respectively,
    ///   so this is ~2400x above the larger of them, and Walnut's own `Transducer
    ///   Library`/`Word Automata Library` contain nothing structurally larger.
    ///   `the_default_budget_has_orders_of_magnitude_of_headroom_on_the_shipped_fixtures`
    ///   re-measures that margin on every test run rather than trusting this comment.
    /// * `max_bfs_states = 100_000`. The result is minimized afterwards, and a *useful*
    ///   transduction result is tens of states (the shipped fixtures are 7 and 8); four
    ///   orders of magnitude of headroom. Mostly redundant with the step budget (a BFS
    ///   state's word is never shorter than its predecessor's, so states cost steps), but
    ///   cheap, and it bounds the one quantity a reader expects to see bounded.
    /// * `max_word_len = 1_000_000`. 8 MB for one word of `usize`, so a single
    ///   allocation can never overshoot between two step charges by more than that.
    fn default() -> Self {
        TransduceBudget {
            max_map_steps: 10_000_000,
            max_bfs_states: 100_000,
            max_word_len: 1_000_000,
        }
    }
}

/// Mutable budget state: a [`TransduceBudget`] plus what has been spent so far.
#[derive(Debug)]
struct BudgetState {
    limits: TransduceBudget,
    map_steps: u64,
}

impl BudgetState {
    fn new(limits: TransduceBudget) -> Self {
        BudgetState {
            limits,
            map_steps: 0,
        }
    }

    /// Charges one `createMap` composition.
    fn charge_map_step(&mut self) -> Result<(), TransduceError> {
        self.map_steps += 1;
        if self.map_steps > self.limits.max_map_steps {
            return Err(TransduceError::Exploded(TransduceLimit::MapSteps));
        }
        Ok(())
    }

    fn check_word_len(&self, len: usize) -> Result<(), TransduceError> {
        if len > self.limits.max_word_len {
            return Err(TransduceError::Exploded(TransduceLimit::WordLength));
        }
        Ok(())
    }

    fn check_bfs_states(&self, count: usize) -> Result<(), TransduceError> {
        if count > self.limits.max_bfs_states {
            return Err(TransduceError::Exploded(TransduceLimit::BfsStates));
        }
        Ok(())
    }
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
            // Java's own NPE text, reproduced verbatim (captured from
            // `Walnut-all.jar`, 2026-08-13) so CLI output still matches. See WB-034.
            TransduceError::NoNumberSystem => write!(
                f,
                "Cannot invoke \"Automata.NumberSystem.isMsd()\" because the return value of \
                 \"java.util.List.get(int)\" is null"
            ),
            TransduceError::TrivialAutomaton => write!(
                f,
                "a TRUE/FALSE automaton has no states or tracks and cannot be transduced"
            ),
            // Java's own NPE text, reproduced verbatim (`Transducer.java:400`; the
            // message is transcribed in WB-035's entry from a real CLI run, 2026-08-13).
            TransduceError::NoTransducerTransition => write!(
                f,
                "Cannot invoke \"it.unimi.dsi.fastutil.ints.IntList.getInt(int)\" because the \
                 return value of \"Automata.FA.Transitions.getNfaStateDests(int, int)\" is null"
            ),
            // Java's own NPE text for the unboxing cast at `Transducer.java:187`.
            TransduceError::NoTransducerOutput => write!(
                f,
                "Cannot invoke \"java.lang.Integer.intValue()\" because the return value of \
                 \"java.util.Map.get(Object)\" is null"
            ),
            TransduceError::Exploded(limit) => write!(
                f,
                "transduce exceeded walnut-rs's resource budget ({limit}); \
                 the input is too large for this port to transduce"
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
    /// Charges one [`BudgetState`] map step.
    ///
    /// # Errors
    ///
    /// [`TransduceError::NoTransducerTransition`] if the transducer has no transition on
    /// that encoded symbol from some state, or has an empty destination list there.
    /// Java's `getNfaStateDests(...).getInt(0)` throws
    /// `NullPointerException`/`IndexOutOfBoundsException` in exactly those two cases,
    /// uncaught, and `transduceNonDeterministic`'s only guard against it (`:276-281`)
    /// checks the transducer's state `0` alone — this is one of WB-035's two confirmed
    /// manifestations, and is also what a partial transducer `.txt` hits. See that
    /// variant's doc for why this is a `Result::Err` and not a `panic!`.
    fn create_map(
        &self,
        m_fa: &Fa,
        i: usize,
        map_so_far: &[usize],
        budget: &mut BudgetState,
    ) -> Result<Vec<usize>, TransduceError> {
        budget.charge_map_step()?;
        let encoded = self.encode_input(m_fa.o[i]);
        (0..self.automaton.fa.q)
            .map(|j| {
                self.automaton.fa.d[map_so_far[j]]
                    .get(&encoded)
                    .and_then(|dests| dests.first().copied())
                    .ok_or(TransduceError::NoTransducerTransition)
            })
            .collect()
    }

    /// `Transducer.createMapSoFar(FA M, Map identity, List iString)` (`:388-394`) —
    /// `phi_{M.O(iString)}`, i.e. [`Transducer::create_map`] folded left-to-right over
    /// the word, starting from the identity.
    fn create_map_so_far(
        &self,
        m_fa: &Fa,
        identity: &[usize],
        i_string: &[usize],
        budget: &mut BudgetState,
    ) -> Result<Vec<usize>, TransduceError> {
        let mut map_so_far = identity.to_vec();
        for &i in i_string {
            map_so_far = self.create_map(m_fa, i, &map_so_far, budget)?;
        }
        Ok(map_so_far)
    }

    /// `Transducer.createIterates(Automaton M, List string, int size)` (`:359-378`) —
    /// `[phi_{M.O(string)}, phi_{M.O(h(string))}, ..., phi_{M.O(h^{size-1}(string))}]`.
    ///
    /// This is where the algorithm's real cost lives: `dests` grows by a factor of `M`'s
    /// out-degree per iterate and every letter of it costs a [`Transducer::create_map`],
    /// so `budget` is what stops it. See this module's "Cost" docs.
    fn create_iterates(
        &self,
        m: &Automaton,
        string: &[usize],
        size: usize,
        budget: &mut BudgetState,
    ) -> Result<Vec<Vec<usize>>, TransduceError> {
        let mut iterates = Vec::with_capacity(size);
        let identity = self.identity_map();
        let mut dests: Vec<usize> = string.to_vec();

        for i in 0..size {
            iterates.push(self.create_map_so_far(&m.fa, &identity, &dests, budget)?);
            // Java: `if (i != size - 1)`. Written as `i + 1 != size` so the guard is
            // also correct (rather than underflowing) at `size == 0`, which Java's
            // loop simply never enters.
            if i + 1 != size {
                dests = Self::get_destination_for_dfa(m, &dests, budget)?;
            }
        }
        Ok(iterates)
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
    ///
    /// The length check is inside the loop, not after it, so the vector cannot overshoot
    /// `budget.limits.max_word_len` by more than one state's out-degree — that is what
    /// makes the word-length ceiling a real bound on peak memory rather than a
    /// post-hoc complaint about an allocation already made.
    fn get_destination_for_dfa(
        m: &Automaton,
        prev_string: &[usize],
        budget: &mut BudgetState,
    ) -> Result<Vec<usize>, TransduceError> {
        let mut i_string = Vec::new();
        for &state in prev_string {
            Self::add_first_entries(m, state, &mut i_string);
            budget.check_word_len(i_string.len())?;
        }
        Ok(i_string)
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
    /// # Errors
    ///
    /// Where Java throws an unchecked `NullPointerException`/`IndexOutOfBoundsException`
    /// from an ill-formed (partial) transducer — see
    /// [`TransduceError::NoTransducerTransition`] and [`TransduceError::NoTransducerOutput`]
    /// — and [`TransduceError::Exploded`] when the default [`TransduceBudget`] runs out.
    /// Use [`Transducer::transduce_msd_deterministic_with_budget`] to choose the budget.
    pub fn transduce_msd_deterministic(
        &self,
        m: &Automaton,
        logging: &mut Logging,
    ) -> Result<Automaton, TransduceError> {
        self.transduce_msd_deterministic_with_budget(m, logging, TransduceBudget::default())
    }

    /// As [`Transducer::transduce_msd_deterministic`], with an explicit work budget.
    pub fn transduce_msd_deterministic_with_budget(
        &self,
        m: &Automaton,
        logging: &mut Logging,
        budget: TransduceBudget,
    ) -> Result<Automaton, TransduceError> {
        let budget = &mut BudgetState::new(budget);
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
        // so this must too. Guarded rather than asserted only for the port's own
        // convenience: hand-built automata in tests may carry short parallel vectors.
        // Java is NOT this tolerant — `clonePartialFields` loops to `richAlphabet.getA()
        // .size()` and indexes `getNS().get(i)` unconditionally, so a shorter `NS` list
        // throws `IndexOutOfBoundsException` there. On the well-formed input every
        // real caller produces, the two behave identically.
        if m.all_reps.len() == m.alphabet.len() && m.msd.len() == m.alphabet.len() {
            n.set_all_reps(m.all_reps.clone());
            if m.ns_name.len() == m.alphabet.len() {
                n.set_ns_names(m.ns_name.clone());
            }
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
            init_maps.push(self.create_map(&m.fa, i, &identity, budget)?);
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
                let i_string = Self::get_destination_for_dfa(m, prev, budget)?;
                // start off with the identity.
                new_maps.push(self.create_map_so_far(&m.fa, &identity, &i_string, budget)?);
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
            iterates: self.create_iterates(m, &[], p + q, budget)?,
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
            // `(int) sigma.get(...).get(...)` (`:187`) — an unboxing NPE on a hole in
            // `sigma`, ported as a rejection; see `TransduceError::NoTransducerOutput`.
            let output = *self.sigma[transducer_state]
                .get(&encoded)
                .ok_or(TransduceError::NoTransducerOutput)?;
            n.fa.o.push(output);
            n.fa.d.push(BTreeMap::new());

            // get h(w) where w = currState.iList.
            let new_string = Self::get_destination_for_dfa(m, &curr_state.i_list, budget)?;

            // relying on the di's to be sorted here...
            let mut state_morphed: Vec<usize> = Vec::new();
            Self::add_first_entries(m, curr_state.state, &mut state_morphed);

            // look at the states that this state transitions to.
            let symbols: Vec<i32> = m.fa.d[curr_state.state].keys().copied().collect();
            for di in symbols {
                // Java indexes `stateMorphed` with the raw `int` key. A negative key is
                // only reachable from an ill-formed transition table (`encode`'s `-1`
                // never lands in `M.fa.d`), and would be an
                // `ArrayIndexOutOfBoundsException` in Java; a checked conversion here so
                // it is a diagnosable panic rather than a silently wrapped `as usize`
                // index that panics confusingly further down.
                let di_index = usize::try_from(di)
                    .unwrap_or_else(|_| panic!("negative encoded symbol {di} in M's transitions"));

                // make new state string
                let mut new_state_string = new_string.clone();
                new_state_string.extend_from_slice(&state_morphed[..di_index]);

                // new state
                let new_state = StateTuple {
                    state: state_morphed[di_index],
                    iterates: self.create_iterates(m, &new_state_string, p + q, budget)?,
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
                        // Not a Java check — the BFS state ceiling. Counted on
                        // `states.len()`, i.e. the automaton actually being built, not on
                        // either input's state count (see this module's "Cost" docs for
                        // why the latter is not a bound at all).
                        budget.check_bfs_states(states.len())?;
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
    /// Java reads a `NumberSystem` object out of `M.getNS()` and calls `.isMsd()` on it
    /// with no null check (`Transducer.java:286`). A track declared with an **explicit
    /// alphabet** (`{0,1}`) rather than `msd_k`/`lsd_k` has a literal `null` there
    /// (`ParseMethods.parseAlphabetDeclaration:91-96`), and such a file is perfectly
    /// valid input everywhere else — so real Walnut throws
    /// `NullPointerException: Cannot invoke "Automata.NumberSystem.isMsd()" …` on it.
    /// That is **WB-034**, and it is reachable straight from `wr_io::reader`'s
    /// `HeaderToken::Set(..)` branch, which is exactly what produces `msd[0] == None`.
    ///
    /// This port replicates it as [`TransduceError::NoNumberSystem`] rather than
    /// silently treating `None` as msd — see that variant's doc for why a `Result::Err`
    /// (not a `panic!`) is the faithful representation of Java's unchecked NPE here.
    pub fn transduce_non_deterministic(
        &self,
        m: &mut Automaton,
        logging: &mut Logging,
    ) -> Result<Automaton, TransduceError> {
        self.transduce_non_deterministic_with_budget(m, logging, TransduceBudget::default())
    }

    /// As [`Transducer::transduce_non_deterministic`], with an explicit work budget for
    /// the [`Transducer::transduce_msd_deterministic`] call(s) it makes. See this
    /// module's "Cost" docs.
    ///
    /// Note the budget is *per inner call*, matching where the exponential work happens;
    /// this path makes exactly one such call either way.
    pub fn transduce_non_deterministic_with_budget(
        &self,
        m: &mut Automaton,
        logging: &mut Logging,
        budget: TransduceBudget,
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

        // WB-034: `Transducer.java:286` calls `.isMsd()` on `M.getNS().get(0)` with no
        // null check, and an explicit-alphabet track's `NumberSystem` *is* null. Ported
        // verbatim as a rejection at exactly Java's position — after the arity and
        // alphabet-compatibility guards, before the reversal.
        let Some(is_msd) = m.msd[0] else {
            return Err(TransduceError::NoNumberSystem);
        };

        if !is_msd {
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
            n = self.transduce_msd_deterministic_with_budget(m, logging, budget)?;
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

            n = t_new.transduce_msd_deterministic_with_budget(&m_new, logging, budget)?;

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
    use proptest::prelude::*;

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

    /// `Word Automata Library/PR.txt` — the regular paperfolding sequence as a 4-state
    /// **`lsd_2`** DFAO, transcribed verbatim from `walnut-java`'s copy.
    fn paperfolding_lsd() -> Automaton {
        let mut a = word_automaton(
            &[0, 0, 0, 1],
            &[
                &[(0, 1), (1, 0)],
                &[(0, 2), (1, 3)],
                &[(0, 2), (1, 2)],
                &[(0, 3), (1, 3)],
            ],
        );
        a.msd = vec![Some(false)];
        a
    }

    /// The `k`-digit msd-first base-2 representation of `n` (leading zeros kept).
    fn msd_digits(n: u32, k: u32) -> Vec<i32> {
        (0..k).rev().map(|b| ((n >> b) & 1) as i32).collect()
    }

    /// An **independent** brute-force oracle for Dekking's construction, written
    /// straight from the definition with no reference to the BFS above: feed the
    /// transducer `m`'s output sequence one letter at a time, left to right, and read
    /// off `sigma` at the last position.
    ///
    /// Positions live inside `h^k(q0)` for a fixed word length `k`, so the letter at
    /// position `i` is `m`'s output on the `k`-digit representation of `i`, and the
    /// position of `word` is its base-2 value. `m` must be total.
    fn dekking_oracle(t: &Transducer, m: &Automaton, k: u32, word: &[i32]) -> i32 {
        assert_eq!(word.len(), k as usize);
        let letter_at = |i: u32| -> i32 {
            let mut s = m.fa.q0;
            for sym in msd_digits(i, k) {
                s = m.fa.d[s][&sym][0];
            }
            m.fa.o[s]
        };
        let position = word.iter().fold(0u32, |acc, &b| acc * 2 + b as u32);
        let mut ts = t.automaton.fa.q0;
        for i in 0..position {
            ts = t.automaton.fa.d[ts][&t.encode_input(letter_at(i))][0];
        }
        t.sigma[ts][&t.encode_input(letter_at(position))]
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
        // Check the WHOLE per-state table, not two probed keys: a spurious third symbol
        // or a second destination in any list must fail here.
        for (q, row_expected) in expected.iter().enumerate() {
            let row = &c.fa.d[q];
            assert_eq!(
                row.keys().copied().collect::<Vec<i32>>(),
                vec![0, 1],
                "state {q} must have exactly the symbols 0 and 1, no more"
            );
            assert_eq!(row[&0], vec![row_expected[0]], "state {q} on symbol 0");
            assert_eq!(row[&1], vec![row_expected[1]], "state {q} on symbol 1");
        }
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

    /// **Composition order.** Everything above is blind to the direction
    /// [`Transducer::create_map_so_far`] folds in: RUNSUM2's transition monoid is
    /// `Z/2` (abelian) and the identity/relabelling transducers have one state, so a
    /// reversed fold computes the same maps. This transducer's monoid is `S_3` —
    /// letter `0` acts as the 3-cycle `(0 1 2)`, letter `1` as the transposition
    /// `(0 1)` — which is emphatically non-commutative, and `sigma` depends on the
    /// state reached, so the order is observable. Checked against
    /// [`dekking_oracle`], an independent left-to-right reading of the definition.
    #[test]
    fn transduction_respects_composition_order_for_a_noncommutative_transducer() {
        let mut logging = Logging::new();
        let t = transducer(
            &[0, 1],
            &[
                // (input value, destination, output)
                &[(0, 1, 0), (1, 1, 10)],
                &[(0, 2, 1), (1, 0, 11)],
                &[(0, 0, 2), (1, 2, 12)],
            ],
        );
        let reference = thue_morse();
        let mut m = thue_morse();
        let c = t.transduce_non_deterministic(&mut m, &mut logging).unwrap();

        let k = 5u32;
        for n in 0..(1u32 << k) {
            let word = msd_digits(n, k);
            assert_eq!(
                word_output(&c, &word),
                Some(dekking_oracle(&t, &reference, k, &word)),
                "S_3 transduction of Thue-Morse at position {n}"
            );
        }
    }

    /// **`minimize_self_with_output` is load-bearing.** This transducer has RUNSUM2's
    /// exact transition structure — so the BFS builds the very same 8 `StateTuple`s the
    /// headline test above pins — but both its states emit the *input letter*, so the
    /// transduced sequence is Thue-Morse again and those 8 tuples must collapse to 2.
    /// Dropping the post-BFS minimization leaves 8 states here.
    #[test]
    fn the_bfs_result_is_minimized_with_output() {
        let mut logging = Logging::new();
        let t = transducer(&[0, 1], &[&[(0, 0, 0), (1, 1, 1)], &[(0, 1, 0), (1, 0, 1)]]);
        let mut m = thue_morse();
        let c = t.transduce_non_deterministic(&mut m, &mut logging).unwrap();

        assert_eq!(
            c.fa.q, 2,
            "the BFS over-generates 8 tuples here; without minimize_self_with_output \
             all 8 survive"
        );
        for n in 0u32..16 {
            let word: Vec<i32> = format!("{n:b}")
                .bytes()
                .map(|b| (b - b'0') as i32)
                .collect();
            assert_eq!(
                word_output(&c, &word),
                Some((n.count_ones() % 2) as i32),
                "the minimized result must still be Thue-Morse, at n = {n}"
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

    /// **WB-034** (`docs/WALNUT-BUGS.md`). A track declared with an explicit `{0,1}`
    /// alphabet instead of `msd_k`/`lsd_k` has a `null` `NumberSystem` in Java, and
    /// `Transducer.java:286` dereferences it unguarded. Empirically confirmed against
    /// the real `walnut-java` CLI (`target/Walnut-all.jar`, 2026-08-13): a `{0,1}` word
    /// automaton through `transduce … RUNSUM2 …` prints
    /// `java.lang.NullPointerException: Cannot invoke "Automata.NumberSystem.isMsd()"
    /// because the return value of "java.util.List.get(int)" is null / at
    /// Automata.Transducer.transduceNonDeterministic(Transducer.java:286)` and returns
    /// to the REPL. Ported verbatim as a rejection carrying that very message.
    #[test]
    fn wb034_a_track_with_no_number_system_is_rejected() {
        let mut logging = Logging::new();
        let mut m = thue_morse();
        // What `wr_io::reader`'s `HeaderToken::Set(..)` branch produces for `{0,1}`.
        m.msd = vec![None];
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::NoNumberSystem
        );
        assert_eq!(
            TransduceError::NoNumberSystem.to_string(),
            "Cannot invoke \"Automata.NumberSystem.isMsd()\" because the return value \
             of \"java.util.List.get(int)\" is null"
        );

        // The guard sits at Java's own position — after the arity check (`:271`) and
        // after the alphabet-compatibility loop (`:276-281`) — so both still win when
        // they also apply.
        let mut two_track = thue_morse();
        two_track.msd = vec![None, None];
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut two_track, &mut logging)
                .unwrap_err(),
            TransduceError::NotSingleInput
        );
        let mut bad_alphabet = word_automaton(&[0, 7], &[&[(0, 0), (1, 1)], &[(0, 1), (1, 0)]]);
        bad_alphabet.msd = vec![None];
        assert_eq!(
            runsum2()
                .transduce_non_deterministic(&mut bad_alphabet, &mut logging)
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

    /// The same partial automaton through a **multi-state** transducer (RUNSUM2), so the
    /// `for q in 0..t_new.automaton.fa.q` loop that installs the dead letter really does
    /// iterate: a bug that wrote to a fixed index instead of `q` would only show up
    /// here, not in the one-state cases above.
    ///
    /// Ground truth captured from the real `walnut-java` CLI (`target/Walnut-all.jar`,
    /// 2026-08-13) as `transduce PARTOUT RUNSUM2 PARTIAL;` with
    /// `Word Automata Library/PARTIAL.txt` = `msd_2` / `0 0 / 0 -> 1` / `1 1 / 0 -> 1,
    /// 1 -> 0`; the 8-state table below is that output verbatim, state numbering
    /// included.
    #[test]
    fn partial_automaton_through_a_multi_state_transducer_matches_java() {
        let mut logging = Logging::new();
        let mut m = word_automaton(&[0, 1], &[&[(0, 1)], &[(0, 1), (1, 0)]]);
        let c = runsum2()
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();

        assert_eq!(c.fa.o, vec![0, 1, 1, 0, 1, 1, 0, 0]);
        let expected: [&[(i32, usize)]; 8] = [
            &[(0, 1)],
            &[(0, 1), (1, 2)],
            &[(0, 3)],
            &[(0, 4), (1, 5)],
            &[(0, 6), (1, 0)],
            &[(0, 4)],
            &[(0, 3), (1, 7)],
            &[(0, 6)],
        ];
        assert_eq!(c.fa.q, expected.len());
        for (q, row_expected) in expected.iter().enumerate() {
            let actual: Vec<(i32, usize)> = c.fa.d[q]
                .iter()
                .map(|(&sym, dests)| {
                    assert_eq!(dests.len(), 1, "state {q} on symbol {sym} must be a DFA");
                    (sym, dests[0])
                })
                .collect();
            assert_eq!(actual, row_expected.to_vec(), "state {q}");
        }

        // ... and semantically: RUNSUM2's running sum, with the dead letter looping in
        // place (so undefined positions contribute nothing), and the result undefined
        // exactly where the input was.
        let k = 4u32;
        for n in 0..(1u32 << k) {
            let letter_at = |i: u32| word_output(&m, &msd_digits(i, k));
            let mut running = 0i32;
            for i in 0..n {
                running = (running + letter_at(i).unwrap_or(0)) % 2;
            }
            let expected = letter_at(n).map(|a| (running + a) % 2);
            assert_eq!(
                word_output(&c, &msd_digits(n, k)),
                expected,
                "partial running sum at position {n}"
            );
        }
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
    /// confirmed, 2026-08-13).
    ///
    /// This port answers with [`TransduceError::NoTransducerTransition`] at the same
    /// point, carrying Java's own NPE text — not a `panic!`, because Java's NPE is a
    /// `RuntimeException` its REPL catches and continues past, whereas a Rust panic would
    /// unwind out of a `wr-cli` session that has no `catch_unwind` and kill the process.
    #[test]
    fn wb035_shifted_alphabet_errors_where_java_npes() {
        let mut logging = Logging::new();
        let t = transducer(&[1, 2], &[&[(1, 0, 7), (2, 0, 8)]]);
        let mut m = word_automaton(&[1, 2], &[&[(0, 1)], &[(0, 1), (1, 0)]]);
        assert_eq!(
            t.transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::NoTransducerTransition
        );
        assert_eq!(
            TransduceError::NoTransducerTransition.to_string(),
            "Cannot invoke \"it.unimi.dsi.fastutil.ints.IntList.getInt(int)\" because the return \
             value of \"Automata.FA.Transitions.getNfaStateDests(int, int)\" is null"
        );
    }

    /// A **partial transducer**: `{0, 1}` declared, but state `1` has no transition on
    /// letter `1`. Nothing about the file is malformed — `read_transducer_txt` accepts
    /// it, and `transduce_non_deterministic`'s only compatibility guard checks the
    /// transducer's state `0` (WB-035), which is total here. So `create_map` reaches the
    /// hole in state `1` and Java NPEs at `Transducer.java:400`; this port returns the
    /// same [`TransduceError::NoTransducerTransition`] rather than panicking. This is the
    /// shape U26's `transduce` command makes reachable from an ordinary user-supplied
    /// `.txt` — see `wr_cli::transduce`'s end-to-end twin of this test.
    #[test]
    fn a_partial_transducer_is_a_clean_error_not_a_panic() {
        let mut logging = Logging::new();
        // State 0: total. State 1: only letter 0.
        let t = transducer(&[0, 1], &[&[(0, 0, 0), (1, 1, 1)], &[(0, 1, 1)]]);
        let mut m = thue_morse();
        assert_eq!(
            t.transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::NoTransducerTransition
        );
    }

    /// The `sigma` twin of the test above: the transition table is total but `sigma` has
    /// a hole, which is Java's unboxing NPE at `Transducer.java:187`. Not reachable from
    /// a `.txt` (`readTransducer` fills both tables from the same lines) but reachable
    /// from any programmatic `Transducer::new`, which is a public constructor.
    #[test]
    fn a_transducer_with_a_hole_in_sigma_is_a_clean_error_not_a_panic() {
        let mut logging = Logging::new();
        let mut t = transducer(&[0, 1], &[&[(0, 0, 0), (1, 0, 1)]]);
        t.sigma[0].remove(&1);
        let mut m = thue_morse();
        assert_eq!(
            t.transduce_non_deterministic(&mut m, &mut logging)
                .unwrap_err(),
            TransduceError::NoTransducerOutput
        );
        assert_eq!(
            TransduceError::NoTransducerOutput.to_string(),
            "Cannot invoke \"java.lang.Integer.intValue()\" because the return value of \
             \"java.util.Map.get(Object)\" is null"
        );
    }

    // -------------------------------------------------------------------
    // The deterministic work budget (port-specific; see the module docs)
    // -------------------------------------------------------------------

    /// The two end-to-end pairs this repo ships — the same ones `IntegrationTest.java`
    /// uses — must be orders of magnitude inside the default budget, or the guard would
    /// be rejecting ordinary Walnut usage. Asserted by *measuring* the spend rather than
    /// by restating a constant, so the margin is visible and cannot silently erode.
    #[test]
    fn the_default_budget_has_orders_of_magnitude_of_headroom_on_the_shipped_fixtures() {
        for (name, m) in [
            ("thue-morse", thue_morse()),
            ("paperfolding", paperfolding_lsd()),
        ] {
            // Binary-search the smallest map-step budget that still succeeds: that is
            // exactly what the transduction spends.
            let spend = (1..)
                .map(|k| 1usize << k)
                .find(|&cap| {
                    let budget = TransduceBudget {
                        max_map_steps: cap as u64,
                        ..TransduceBudget::default()
                    };
                    runsum2()
                        .transduce_non_deterministic_with_budget(
                            &mut m.clone(),
                            &mut Logging::new(),
                            budget,
                        )
                        .is_ok()
                })
                .expect("the shipped fixtures transduce within some finite budget");
            assert!(
                (spend as u64) * 1000 < TransduceBudget::default().max_map_steps,
                "{name}: spends ~{spend} map steps, which is not >=1000x inside the default \
                 budget of {} -- either the fixture or the default has drifted",
                TransduceBudget::default().max_map_steps
            );
        }
    }

    /// Each of the three ceilings is separately reachable, and each reports itself.
    #[test]
    fn each_budget_ceiling_trips_with_its_own_limit_and_never_hangs() {
        // Map steps: RUNSUM2 over Thue-Morse spends more than one.
        assert_eq!(
            runsum2()
                .transduce_non_deterministic_with_budget(
                    &mut thue_morse(),
                    &mut Logging::new(),
                    TransduceBudget {
                        max_map_steps: 1,
                        ..TransduceBudget::default()
                    },
                )
                .unwrap_err(),
            TransduceError::Exploded(TransduceLimit::MapSteps)
        );

        // Word length: `h` of a one-letter word is already two letters.
        assert_eq!(
            runsum2()
                .transduce_non_deterministic_with_budget(
                    &mut thue_morse(),
                    &mut Logging::new(),
                    TransduceBudget {
                        max_word_len: 1,
                        ..TransduceBudget::default()
                    },
                )
                .unwrap_err(),
            TransduceError::Exploded(TransduceLimit::WordLength)
        );

        // BFS states: the Thue-Morse result has 8 of them before minimization.
        assert_eq!(
            runsum2()
                .transduce_non_deterministic_with_budget(
                    &mut thue_morse(),
                    &mut Logging::new(),
                    TransduceBudget {
                        max_bfs_states: 1,
                        ..TransduceBudget::default()
                    },
                )
                .unwrap_err(),
            TransduceError::Exploded(TransduceLimit::BfsStates)
        );
    }

    /// A three-letter, single-track word automaton — the shared [`word_automaton`]
    /// helper hardcodes a two-letter alphabet.
    fn word_automaton3(outputs: &[i32], d: &[&[(i32, usize)]]) -> Automaton {
        assert_eq!(outputs.len(), d.len());
        let table: Vec<BTreeMap<i32, Vec<usize>>> = d
            .iter()
            .map(|row| row.iter().map(|&(sym, dest)| (sym, vec![dest])).collect())
            .collect();
        let fa = Fa {
            q0: 0,
            q: outputs.len(),
            alphabet_size: 3,
            o: outputs.to_vec(),
            d: table,
            true_false: None,
        };
        Automaton::new(
            fa,
            vec![vec![0, 1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    /// The case a state-count guard cannot see, and the whole reason the budget had to
    /// move inside this file: a **four**-state input over a three-letter alphabet through
    /// a **four**-state transducer — numbers any `Q <= 500`-style precheck waves straight
    /// through, and numbers barely larger than the shipped `RUNSUM2`/Thue-Morse pair,
    /// which finishes in under `2^10` map steps.
    ///
    /// This pair was found by search and *measured* (release, 2026-08-15) to spend more
    /// than **100,000,000** map steps — 5+ seconds of release CPU, still climbing, past
    /// 10x the default budget — i.e. five orders of magnitude more than the shipped
    /// fixtures at the same state counts. The assertion below uses a small explicit
    /// budget so the test itself stays in the milliseconds; the 100M figure is recorded
    /// as prose because asserting it would cost seconds per run.
    ///
    /// The contrast is the point: at one and the same `100_000`-step budget, the shipped
    /// pair succeeds and this one does not. No function of the two state counts can tell
    /// them apart.
    #[test]
    fn a_tiny_but_exponential_input_is_rejected_where_a_state_count_guard_sees_nothing() {
        let budget = TransduceBudget {
            max_map_steps: 100_000,
            ..TransduceBudget::default()
        };

        let t = transducer(
            &[0, 1, 2],
            &[
                &[(0, 1, 2), (1, 2, 0), (2, 2, 0)],
                &[(0, 1, 1), (1, 1, 1), (2, 2, 1)],
                &[(0, 3, 2), (1, 2, 2), (2, 3, 1)],
                &[(0, 2, 0), (1, 2, 0), (2, 3, 0)],
            ],
        );
        let mut m = word_automaton3(
            &[0, 1, 1, 0],
            &[
                &[(0, 3), (1, 1), (2, 3)],
                &[(0, 0), (1, 0), (2, 2)],
                &[(0, 0), (1, 0), (2, 0)],
                &[(0, 2), (1, 2), (2, 1)],
            ],
        );
        assert_eq!(
            m.fa.q, 4,
            "four input states -- under any state-count guard"
        );
        assert_eq!(
            t.automaton.fa.q, 4,
            "four transducer states -- likewise under any state-count guard"
        );
        assert_eq!(
            t.transduce_non_deterministic_with_budget(&mut m, &mut Logging::new(), budget)
                .unwrap_err(),
            TransduceError::Exploded(TransduceLimit::MapSteps)
        );

        // Same budget, the shipped two-state pair: comfortably fine.
        assert!(runsum2()
            .transduce_non_deterministic_with_budget(&mut thue_morse(), &mut Logging::new(), budget)
            .is_ok());
    }

    // -------------------------------------------------------------------
    // lsd input (the `reverseWithOutput` round trip)
    // -------------------------------------------------------------------

    /// **Walnut's own golden `lsd` transduce fixture**, and the test that actually pins
    /// the two `reverse_with_output` calls: `IntegrationTest.java:674`'s
    /// `transduce test529 RUNSUM2 PR;` — RUNSUM2 applied to the `lsd_2` paperfolding
    /// word automaton — whose expected output is
    /// `walnut-java/src/test/resources/integrationTests/Global/Word Automata Library/
    /// test529.txt`, transcribed verbatim below (state numbering included). Unlike the
    /// single-state relabelling round-trip below, RUNSUM2's output genuinely depends on
    /// the direction the sequence is read, so deleting *either* reversal changes the
    /// answer.
    #[test]
    fn transduce_runsum2_over_paperfolding_lsd_matches_javas_golden_fixture() {
        let mut logging = Logging::new();
        let pr = paperfolding_lsd();
        let mut m = paperfolding_lsd();
        let c = runsum2()
            .transduce_non_deterministic(&mut m, &mut logging)
            .unwrap();

        // test529.txt: `lsd_2`, seven states.
        assert_eq!(c.fa.q, 7);
        assert_eq!(c.fa.o, vec![0, 0, 0, 1, 0, 1, 1]);
        let expected: [[usize; 2]; 7] = [[1, 2], [1, 3], [4, 5], [6, 3], [1, 4], [3, 5], [6, 4]];
        for (q, row_expected) in expected.iter().enumerate() {
            let row = &c.fa.d[q];
            assert_eq!(
                row.keys().copied().collect::<Vec<i32>>(),
                vec![0, 1],
                "state {q} must have exactly the symbols 0 and 1"
            );
            assert_eq!(row[&0], vec![row_expected[0]], "state {q} on symbol 0");
            assert_eq!(row[&1], vec![row_expected[1]], "state {q} on symbol 1");
        }
        assert_eq!(
            c.msd,
            vec![Some(false)],
            "the result is reversed back to lsd on the way out"
        );

        // ... and semantically: read lsd-first, the result is the running sum mod 2 of
        // the paperfolding sequence.
        let mut running = 0i32;
        for n in 0u32..24 {
            let lsd_rep: Vec<i32> = format!("{n:b}")
                .bytes()
                .rev()
                .map(|b| (b - b'0') as i32)
                .collect();
            running = (running + word_output(&pr, &lsd_rep).expect("PR is total")) % 2;
            assert_eq!(
                word_output(&c, &lsd_rep),
                Some(running),
                "running sum of the paperfolding sequence at n = {n}"
            );
        }
    }

    /// An `lsd_2` input takes the reverse-transduce-reverse path (`:286-292`,
    /// `:325-327`). Applied to a transducer that only relabels outputs (single-state,
    /// so the reversal cannot change which output a word gets), the result must agree
    /// with relabelling the input directly. **Deliberately weak** — it is the
    /// *structure* test for the lsd path (that `M` is left rewritten in place); the
    /// reversals themselves are pinned by the golden fixture above, not here.
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

        let b = &mut BudgetState::new(TransduceBudget::default());
        // M state 0 has output 0; RUNSUM2 on letter 0 is the identity on states.
        assert_eq!(t.create_map(&m.fa, 0, &identity, b), Ok(vec![0, 1]));
        // M state 1 has output 1; RUNSUM2 on letter 1 swaps the two states.
        assert_eq!(t.create_map(&m.fa, 1, &identity, b), Ok(vec![1, 0]));
        // Composing letter 1 twice is the identity again.
        assert_eq!(
            t.create_map_so_far(&m.fa, &identity, &[1, 1], b),
            Ok(vec![0, 1])
        );
        assert_eq!(
            t.create_map_so_far(&m.fa, &identity, &[1, 0, 1], b),
            Ok(vec![0, 1])
        );
    }

    #[test]
    fn get_destination_for_dfa_applies_the_morphism_in_symbol_order() {
        let m = thue_morse();
        let b = &mut BudgetState::new(TransduceBudget::default());
        // h(0) = 0 1, h(1) = 1 0.
        assert_eq!(
            Transducer::get_destination_for_dfa(&m, &[0], b),
            Ok(vec![0, 1])
        );
        assert_eq!(
            Transducer::get_destination_for_dfa(&m, &[1], b),
            Ok(vec![1, 0])
        );
        assert_eq!(
            Transducer::get_destination_for_dfa(&m, &[0, 1], b),
            Ok(vec![0, 1, 1, 0])
        );
        assert_eq!(
            Transducer::get_destination_for_dfa(&m, &[], b),
            Ok(Vec::<usize>::new())
        );
    }

    #[test]
    fn create_iterates_of_the_empty_word_is_all_identities() {
        let t = runsum2();
        let m = thue_morse();
        let b = &mut BudgetState::new(TransduceBudget::default());
        assert_eq!(t.create_iterates(&m, &[], 3, b), Ok(vec![vec![0, 1]; 3]));
    }

    #[test]
    fn is_totalized_distinguishes_the_four_shapes() {
        assert_eq!(Transducer::is_totalized(&thue_morse().fa), Ok(true));

        let partial = word_automaton(&[0, 1], &[&[(0, 1)], &[(0, 1), (1, 0)]]);
        assert_eq!(Transducer::is_totalized(&partial.fa), Ok(false));

        let mut nfa = thue_morse();
        nfa.fa.d[1].insert(1, vec![0, 1]);
        assert_eq!(
            Transducer::is_totalized(&nfa.fa),
            Err(TransduceError::MultipleTransitionsPerInput)
        );

        // The quirk `is_totalized`'s `Some(_) => {}` arm encodes: an entry that is
        // PRESENT but has an EMPTY destination list is neither `null` nor `size() > 1`,
        // so Java leaves `totalized` alone and calls this automaton total — even though
        // the transition is every bit as undefined as a missing key.
        let mut empty_dests = thue_morse();
        empty_dests.fa.d[0].insert(1, Vec::new());
        assert_eq!(
            Transducer::is_totalized(&empty_dests.fa),
            Ok(true),
            "an empty destination list falls through both of Java's branches"
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
    // ---------------------------------------------------- Tier-4 property (Phase 4, U31)

    /// A TOTAL random single-track `msd_2` DFAO: every state has a destination on both
    /// digits, and every output is in `{0, 1}`.
    ///
    /// Totality is the load-bearing constraint. `transduceNonDeterministic` routes a total
    /// `M` straight to `transduceMsdDeterministic` and never enters the dead-state branch,
    /// which is where `docs/WALNUT-BUGS.md` **WB-035** lives — its `minOutput` marker is
    /// used both as an un-encoded transducer INPUT symbol and as a marker in the RESULT's
    /// output alphabet, and it is ported verbatim as a bug. A mathematical oracle
    /// disagrees with the port BY DESIGN on every WB-035 trigger, so a generator that
    /// reached that branch would drown the genuine Dekking-construction signal this
    /// property exists to check. (WB-035 has three dedicated tests of its own above,
    /// including the one-value-different control; it is covered, just not here.)
    ///
    /// `msd = Some(true)` for the same class of reason: the lsd direction reverses `M`
    /// before transducing and reverses the result afterwards, which is a different code
    /// path with its own (separately closed) history.
    fn arb_total_msd_dfao(q_max: usize) -> impl Strategy<Value = Automaton> {
        (1..=q_max).prop_flat_map(move |q| {
            let o = prop::collection::vec(0i32..=1, q);
            let trans = prop::collection::vec(prop::collection::vec(0usize..q, 2), q);
            (o, trans).prop_map(move |(o, trans)| {
                let d: Vec<BTreeMap<i32, Vec<usize>>> = trans
                    .iter()
                    .map(|row| {
                        row.iter()
                            .enumerate()
                            .map(|(sym, &dest)| (sym as i32, vec![dest]))
                            .collect()
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

    /// A TOTAL transducer over the input alphabet `{0, 1}` — every state has a transition
    /// AND a `sigma` entry for both letters — with NON-NEGATIVE outputs.
    ///
    /// Totality keeps `createMap`/`sigma` out of `TransduceError::NoTransducerTransition`/
    /// `NoTransducerOutput` (Java's two uncaught NPEs on a partial transducer, ported as
    /// rejections and pinned by their own tests); the `{0, 1}` alphabet matches the
    /// generated `M`'s output values exactly, so `transduceNonDeterministic`'s
    /// compatibility guard passes and `encode_input` is the identity — the coincidence
    /// WB-035's half (1) depends on, kept deliberately intact here so this property is
    /// about the construction and not about that quirk.
    fn arb_total_transducer(q_max: usize) -> impl Strategy<Value = Transducer> {
        (1..=q_max).prop_flat_map(move |q| {
            let dests = prop::collection::vec(prop::collection::vec(0usize..q, 2), q);
            let outs = prop::collection::vec(prop::collection::vec(0i32..=2, 2), q);
            (dests, outs).prop_map(move |(dests, outs)| {
                let rows: Vec<Vec<(i32, usize, i32)>> = dests
                    .iter()
                    .zip(outs.iter())
                    .map(|(drow, orow)| {
                        (0..2)
                            .map(|letter| (letter as i32, drow[letter], orow[letter]))
                            .collect()
                    })
                    .collect();
                let borrowed: Vec<&[(i32, usize, i32)]> = rows.iter().map(Vec::as_slice).collect();
                transducer(&[0, 1], &borrowed)
            })
        })
    }

    proptest! {
        // Each case builds a whole transduction (a periodicity search plus a BFS whose
        // per-state cost grows with the words it carries), so this runs well under the
        // default case count — `CLAUDE.md`'s "generate SMALL" guardrail, and the same
        // `ProptestConfig` discipline `numsys.rs` applies to its expensive constructions.
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Tier-4: the Dekking transduction really is "run the transducer along the
        /// automatic sequence", checked against a step-by-step brute-force oracle.
        ///
        /// For a `k`-digit input word `w` denoting `n`, let `a_j` be `M`'s output on the
        /// `k`-digit representation of `j`. The result `N` must satisfy
        ///
        /// ```text
        /// N(w) = sigma_T( delta_T(T.q0, a_0 a_1 … a_{n-1}), a_n )
        /// ```
        ///
        /// i.e. `N`'s output at `n` is what the transducer emits when it reaches position
        /// `n` of the sequence, having already consumed positions `0 … n-1`.
        ///
        /// The oracle is [`dekking_oracle`] — written for a prior unit in this same
        /// module, and reused here rather than duplicated. It walks `M` once per position
        /// and then walks `T` along the resulting letter sequence, using only `Fa::d` /
        /// `Fa::o` / `sigma` lookups. It never calls
        /// `transduce_*`, `create_map`, `create_iterates`, `get_destination_for_dfa` or
        /// `minimize_self_with_output`, and in particular it knows nothing about the
        /// `phi_a` state-map machinery or the `(M-state, iterates)` BFS key that the
        /// construction is actually built out of — so a wrong lag/period, a wrong
        /// `stateMorphed` index, or a wrong `iterates[0][T.q0]` read shows up here as a
        /// disagreement rather than being re-derived.
        ///
        /// Note this needs no prolongability assumption on `M` (`delta_M(q0, 0) == q0` is
        /// NOT required): the oracle re-reads the level-`k` sequence at the same width `k`
        /// the automaton is being asked about.
        #[test]
        fn transduction_matches_a_step_by_step_oracle(
            m in arb_total_msd_dfao(3),
            t in arb_total_transducer(2),
            k in 1usize..=3,
            n_raw in 0usize..8,
        ) {
            let original = m.clone();
            let mut m = m;
            let mut logging = Logging::new();
            // A tight explicit budget rather than the (huge) default: this is the fast
            // test tier, and an over-budget case is skipped, never silently passed as a
            // success. See this module's "Cost" docs for why no cheaper a-priori bound on
            // the work exists.
            let budget = TransduceBudget {
                max_map_steps: 2_000_000,
                max_bfs_states: 2_000,
                max_word_len: 100_000,
            };
            let n_result = t.transduce_non_deterministic_with_budget(&mut m, &mut logging, budget);
            // `prop_assume!`, not a bare `return Ok(())`: proptest counts an early return
            // as an ordinary PASS, so if a future change made every generated case blow
            // the budget this property would go silently vacuous and stay green forever.
            // A rejection is tracked instead, and starving the property aborts the run
            // with "too many local rejects".
            prop_assume!(!matches!(n_result, Err(TransduceError::Exploded(_))));
            let n_auto = n_result.expect("the generators exclude every other error path");

            let width = 1usize << k; // 2^k, the number of positions at this width
            let n = n_raw % width;
            let word = msd_digits(n as u32, k as u32);

            // The expected output is `dekking_oracle`'s — the same independent
            // step-by-step oracle a prior unit already wrote in this module, reused rather
            // than re-derived inline. (An earlier draft of this property duplicated it
            // verbatim; the two copies were read against each other ONCE, by hand, at the
            // moment they were merged into this single call -- there is no preserved
            // regression check on that agreement, and none is needed now that only one
            // copy exists.)
            let expected = dekking_oracle(&t, &original, k as u32, &word);

            // And read N at the same position.
            let mut state = n_auto.fa.q0;
            for &digit in &word {
                state = n_auto.fa.d[state][&digit][0];
            }
            prop_assert_eq!(
                n_auto.fa.o[state], expected,
                "transduction disagrees at position {} of the width-{} sequence", n, k
            );
        }
    }
}
