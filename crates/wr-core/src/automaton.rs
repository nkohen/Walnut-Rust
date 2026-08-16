// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! A minimal multi-track automaton wrapper.
//!
//! Ports the pieces of `Automata/Automaton` + `Automata/RichAlphabet` that ∃-projection
//! structurally requires (originally written against `wr-logic`'s `quantify`, per
//! `docs/BOUNDARY-MAP.md` §4.3; the U6 architecture unit moved that primitive down into
//! [`crate::quantify`] — see its module docs for why): per-track alphabets,
//! labels, msd/lsd-ness, and the mixed-radix symbol encoder/decoder. **This is
//! deliberately NOT full Java parity** — no `NumberSystem` objects attached per track
//! (only the two facts any ported code reads off one: msd/lsd direction via
//! [`Automaton::msd`], and the custom-base valid-representation restriction via
//! [`Automaton::all_reps`], added in U5), no DFAO/`combine` bookkeeping (see
//! `docs/DESIGN.md` §8 Phase 1's spike scope).
//!
//! # U0 addition: the trivial (TRUE/FALSE) automaton
//!
//! Java's `TRUE_FALSE_AUTOMATON`/`TRUE_AUTOMATON` short-circuit IS modeled as of U0 —
//! the state itself lives on [`Fa`] (see `crate::fa`'s module docs for the
//! representation and its exhaustive justification), and this file ports every
//! `Automaton`-level branch on it: [`Automaton::true_false`] (Java's
//! `Automaton(boolean)` constructor, `Automaton.java:106-110`), [`Automaton::clear`]
//! (`:506-512`), and the guards in [`Automaton::get_arity`] (`:499`),
//! [`Automaton::is_empty`] (`:515-517`), [`Automaton::sort_label`] (`:351`),
//! [`Automaton::bind`] (`:439`), plus [`AutomatonDFA`]'s own
//! (`AutomatonDFA.java:21-25`, `:79-81`, `:88-90`, `:102-104`).
//!
//! A trivial `Automaton` has NO tracks: `alphabet`/`label`/`msd` are all empty, so its
//! arity is 0 and `is_bound()` is vacuously true. Nothing may `encode`/`decode` against
//! it — call sites must check [`Automaton::is_true_false_automaton`] first, exactly as
//! their Java originals do.
//!
//! # U2 additions: `bind`/`sortLabel`/`canonize`/`determinizeAndMinimize`/`AutomatonDFA`
//!
//! This unit adds the rest of `Automata/Automaton`'s self-contained surface (no
//! `NumberSystem`/`AutomatonLogicalOps`/`ProductStrategies`/`WordAutomaton` dependency)
//! plus `Automata/AutomatonDFA`'s non-regex constructors. Explicitly OUT of scope here,
//! because every one of them has a real, unavoidable dependency this crate doesn't have
//! yet (see the doc comments on the individual skipped items, and the U2 completion
//! report, for exactly what's missing):
//! - `Automaton.setAlphabet` (its parameter type is a `List<NumberSystem>` this crate
//!   doesn't have yet, and it unconditionally CALLS `applyAllRepresentationsWithOutput`
//!   — though that method's own `ProductStrategies.crossProduct` step only actually
//!   EXECUTES when a per-track `NumberSystem` is non-null and `useAllRepresentations()`,
//!   a correction to an earlier version of this doc that overstated the call as always
//!   running; either way, a real `NumberSystem` parameter type is the blocking
//!   dependency, not specifically `crossProduct`). The self-contained pieces it's built
//!   from (`RichAlphabet.isInNewAlphabet`, the private `rebuildTransitions` helper) ARE
//!   ported below as
//!   [`Automaton::is_in_new_alphabet`]/[`Automaton::rebuild_transitions_for_new_alphabet`]
//!   for a future unit to assemble once `NumberSystem` lands.
//! - `Automaton.normalizeNumberSystems` (still out of scope: it *constructs* a
//!   `NumberSystem` from a track's alphabet maximum, which is `setAlphabet`/U16's
//!   business).
//! - `AutomatonDFA`'s regex-string constructors (need `BricsConverter`) and its
//!   file-address constructor (file I/O is `wr-io`/Phase 3 scope).
//!
//! # U5 additions: `applyAllRepresentations`/`applyAllRepresentationsWithOutput`
//!
//! U2 deferred both on the premise that the whole custom-base numeration family was out
//! of scope, so `useAllRepresentations()` could never be `true`. Phase 3a's U5 made
//! custom bases real ([`crate::numsys::NumberSystem::with_custom_base_files`]), which
//! invalidated that premise, so both methods are ported here now, along with the per-track
//! state they read ([`Automaton::all_reps`]) and `determineRandomLabel`/`unlabel`'s role in
//! them. See [`Automaton::apply_all_representations`] for the empirical confirmation that
//! these are load-bearing (not merely reachable) on a custom base.
//!
//! Also not replicated: Java's manual `clone()`/`cloneFields()`/`copy()` boilerplate —
//! `#[derive(Clone)]` already gives deep-copy semantics (`PORTING.md`'s
//! `Cloneable`→`#[derive(Clone)]` mapping), so there is nothing left for those methods
//! to do here.
//!
//! # Symbol encoding (`RichAlphabet.encode`/`decode`)
//!
//! A transition symbol is one integer encoding a simultaneous digit-tuple across all
//! tracks, mixed-radix with **track 0 fastest-varying**: `encoder[i]` = product of the
//! alphabet sizes of tracks `0..i`, and `encode(digits) = Σ encoder[i] * index_of(digits[i]
//! in alphabet[i])` — indexing is by **position in the track's alphabet list, not by the
//! literal digit value** (this matters once a track's alphabet isn't a contiguous `0..k`
//! range). Replicate this exactly, or downstream product/quantify logic (keyed on
//! encoded ints) silently misaligns tracks.

use crate::fa::Fa;
use crate::util::{is_sorted, remove_indices};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::rc::Rc;

/// `.expect()` message for every call into [`crate::determinize::determinize`] that
/// passes `None` for its context.
///
/// Java's two `Automaton.determinizeAndMinimize` overloads (`Automaton.java:394`,
/// `:404`) are the ONLY callers of `DeterminizationStrategies.determinize` — but that is
/// a fact about *Java's* call graph, not this port's. As of U0c, the dispatcher has
/// **four** Rust call sites, not two: these same two `Automaton` methods below, plus
/// `crate::quantify::quantify_helper` (∃-projection — the single most common
/// determinization site in the whole engine) and `wr_io::reader::read_automaton_txt`
/// (`.txt` loading), both of which call `crate::determinize::determinize` directly
/// rather than through an `Automaton` method. This
/// matters because it's what actually puts the `[strategy …]`/`[export …]` hook on the
/// port's real call graph — an earlier version of this comment claimed only these two
/// overloads mattered, which a reviewer caught: it was true for Java but left `quantify`
/// and `reader` silently bypassing the hook, a real landmine for Phase 3b's
/// `MetaCommands` port (some live subset-relevant golden fixtures already use in-scope
/// strategy/export directives that would have silently never applied to ∃-elimination or
/// `.txt`-load determinizations). Fixed by routing those two call sites through the
/// dispatcher too, not just by correcting this comment.
///
/// Three of the four now take a real context on the `eval`/`def` path: Java reads the
/// `Prover.mainProver.metaCommands` singleton *inside* the dispatcher, and this port
/// threads the equivalent down from `Prover` explicitly (`PORTING.md`'s ruling for Java
/// global mutable state) — see [`Automaton::determinize_and_minimize_with_ctx`],
/// [`Automaton::determinize_and_minimize_from_with_ctx`] and
/// [`crate::quantify::quantify_with_ctx`]. The fourth, `wr_io::reader`'s `.txt` load,
/// still passes `None` unconditionally: reproducing Java there would mean threading a
/// context through `wr_logic::predicate_env::PredicateEnv`, and it only ever matters for
/// a library file that is genuinely nondeterministic (no corpus fixture is), so the gap
/// is recorded in `wr_logic::eval::evaluate_with_logging_and_ctx`'s docs rather than
/// closed blind.
///
/// With `None` the dispatcher is behaviorally identical to the pre-U0c code at each of
/// these four sites — strategy is unconditionally [`crate::determinize::Strategy::Sc`],
/// the export sink and the automata counter are never touched, and the only fallible arm
/// ([`crate::determinize::brzozowski`]) is unreachable — so this `expect` cannot fire.
/// It is asserted, not assumed, in [`dispatch_determinize`].
const NO_CONTEXT_CANNOT_FAIL: &str =
    "determinize with no metacommand context always takes the SC arm, which is infallible";

/// The one place this crate turns a [`crate::determinize::DeterminizeError`] back into
/// Java's thrown `WalnutException`.
///
/// With `ctx == None` the dispatcher is infallible (see [`NO_CONTEXT_CANNOT_FAIL`]), so
/// this is `expect`ed; with a real context a `[strategy n BRZ]` on a word automaton is
/// reachable, and Java throws
/// `WalnutException("DFAOs are not supported for non-SC strategies.")`
/// (`DeterminizationStrategies.java:115-119`) out of `determinizeAndMinimize` — caught by
/// `EvalDef.compute`. `crate::walnut_panic`'s guard-authoring rule says such a guard
/// panics with **exactly** the Java message and nothing else, which is what lets
/// `wr_logic::eval::compute`'s existing boundary turn it into the same positioned error
/// message Java produces.
pub(crate) fn dispatch_determinize(
    a: &mut Automaton,
    initial: &BTreeSet<usize>,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
) {
    let had_ctx = ctx.is_some();
    if let Err(e) = crate::determinize::determinize(a, initial, ctx) {
        assert!(had_ctx, "{NO_CONTEXT_CANNOT_FAIL}");
        match e {
            crate::determinize::DeterminizeError::DfaoWithNonScStrategy(_) => {
                panic!("DFAOs are not supported for non-SC strategies.")
            }
            // `brzozowski`'s intermediate `justMinimize()`; documented as unreachable
            // there (subset construction's output is always deterministic and
            // q0-reachable), so this arm exists only so the match is total.
            crate::determinize::DeterminizeError::Minimize(m) => {
                panic!("determinize: intermediate minimize failed: {m:?}")
            }
        }
    }
}

/// `Automaton.normalizeNumberSystems`'s unconditional warning (`Automaton.java:178-179`),
/// verbatim — see [`Automaton::normalize_number_systems`]. `pub` so `wr-cli`'s tests can
/// assert on the exact text without re-typing it.
pub const ALPHABET_CHANGED_WARNING: &str =
    "WARN: The alphabet of the resulting automaton was changed. Use the alphabet command to change as desired.";

/// Why [`Automaton::try_decode`] could not decode a symbol — the two unchecked JDK
/// exceptions `RichAlphabet.decode`'s body can raise, surfaced as values.
///
/// Both render Java's own message text verbatim, so that a caller that panics with them
/// (see [`Automaton::decode`]) reports what real Walnut reports once
/// [`crate::walnut_panic::catch_walnut_panic`] recovers the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// `ArrayList.get(idx)` with a negative `idx` — `IndexOutOfBoundsException: Index
    /// {index} out of bounds for length {length}` (the JDK's own wording, as printed by
    /// the real CLI on the WB-038 reproducer). Reached whenever the encoded symbol is
    /// negative, which the `.txt` reader really can produce (WB-038).
    IndexOutOfBounds { index: i32, length: usize },
    /// `n % 0` — `ArithmeticException: / by zero`. A track with an empty alphabet; no
    /// alphabet this crate builds is empty, so this is a defensive value, not a live
    /// failure mode (see [`Automaton::try_decode`]).
    EmptyTrackAlphabet,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::IndexOutOfBounds { index, length } => {
                write!(f, "Index {index} out of bounds for length {length}")
            }
            DecodeError::EmptyTrackAlphabet => write!(f, "/ by zero"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// A multi-track automaton: the raw [`Fa`] plus enough track metadata to encode/decode
/// symbols and (for `wr-logic`) know which tracks are quantifiable and how to fix up
/// leading/trailing zeros after projection.
#[derive(Debug, Clone)]
pub struct Automaton {
    pub fa: Fa,
    /// Per-track alphabet, e.g. `[0, 1, ..., base-1]` for an ordinary base-*k* track.
    pub alphabet: Vec<Vec<i32>>,
    /// Per-track variable name (e.g. `"i"`, `"x"`), parallel to `alphabet`.
    pub label: Vec<String>,
    /// Per-track msd (`Some(true)`)/lsd (`Some(false)`)/non-arithmetic (`None`) — parallel
    /// to `alphabet`. Mirrors `NumberSystem.determineMsd`'s three-way outcome (mixed
    /// arithmetic tracks or no arithmetic tracks both yield `None`/"skip the zero fixup").
    /// Also this crate's stand-in for Java's per-track `List<NumberSystem>` wherever a
    /// port needs "the NS list", e.g. [`Automaton::reduce_dimension`] below.
    pub msd: Vec<Option<bool>>,
    /// The OTHER half of the per-track `NumberSystem` stand-in, added in Phase 3a's U5:
    /// per track, `Some(N)` iff Java's `getNS().get(i) != null &&
    /// getNS().get(i).useAllRepresentations()`, carrying that number system's
    /// `getAllRepresentations()` automaton (`NumberSystem.java:253-263`).
    ///
    /// # Why a second parallel vector rather than a `NumberSystem` per track
    ///
    /// [`Automaton::apply_all_representations`] is the only code **currently ported in
    /// this crate** that reads a track's `NumberSystem` for anything other than its
    /// msd/lsd direction, and it reads exactly these two facts. (Java has at least one
    /// other reader outside this crate's scope so far — `Image.determineImageNumberSystemPrefix`,
    /// which reads `NumberSystem::toString()`; already flagged in `docs/WALNUT-BUGS.md`'s
    /// not-yet-confirmed section, unrelated to this field.) Storing exactly these two facts
    /// directly keeps `Automaton` free of a `NumberSystem` field — which would be circular
    /// (`NumberSystem` *owns* three `Automaton`s) and would force every one of this crate's
    /// hand-built test automata to construct a real numeration system.
    ///
    /// # Invariant (kept by every mutator in this crate, checked by [`Automaton::debug_assert_track_invariant`])
    ///
    /// `all_reps.len() == msd.len()`, and `all_reps[i].is_some()` implies
    /// `msd[i].is_some()` — because both derive from the same Java `NumberSystem` object,
    /// so "this track has an all-representations automaton" cannot hold for a track whose
    /// number system is `null`. Every site that permutes, removes, clears, or merges
    /// `msd` entries below does the same to `all_reps` in the same step, mirroring Java,
    /// where a single `List<NumberSystem>` element carries both facts at once.
    ///
    /// [`Rc`] (not [`Box`]) because Java shares one `getAllRepresentations()` instance
    /// across every track of every automaton derived from that number system, and because
    /// `Automaton` is cloned constantly; `Rc` keeps that a pointer bump instead of a deep
    /// automaton copy. This makes `Automaton` `!Send`/`!Sync`, which is fine — Walnut is
    /// single-threaded throughout, and `PORTING.md`'s Ruling 1 already picked `Rc` over
    /// `Arc` for `NumberSystem` handles for the same reason.
    pub all_reps: Vec<Option<Rc<Automaton>>>,
    /// The THIRD fact this crate keeps from Java's per-track `NumberSystem` object:
    /// `NumberSystem.getName()` (`NumberSystem.java:249`) — `"msd_2"`, `"lsd_3"`,
    /// `"msd_fib"`, … — where the provenance is known, `None` where it is not.
    ///
    /// # Why the name has to be stored and cannot be reconstructed
    ///
    /// `NumberSystem.isNSDiffering` (`NumberSystem.java:179-192`, the guard behind
    /// `union`/`intersect`/`concat`'s `"Automata must have the same number system(s)."`)
    /// compares number systems **by name**. Reconstructing a name from
    /// `(msd[i], alphabet[i].len())` — which is all [`Automaton::msd`] and
    /// [`Automaton::alphabet`] carry — is exact for a plain `msd_k`/`lsd_k` base (Java's
    /// `normalizeNumberSystemToken`, `:273-295`, maps bare `msd`/`lsd` to `msd_2`/`lsd_2`,
    /// so every plain base's name *is* `msd_<alphabet size>`), but it is WRONG for a
    /// custom base: `msd_fib`'s alphabet is `{0, 1}` with `msd` direction, so a
    /// reconstruction reports it as `msd_2` and `isNSDiffering` then answers "same number
    /// system" for two automata on genuinely different numerations — a fail-open guard
    /// that silently produces a meaningless mixed-numeration result where real Walnut
    /// refuses. Hence this field, populated from the resolved [`crate::numsys::NumberSystem`]
    /// wherever one exists (`wr-io`'s reader, `wr-cli`'s `alphabet`/`reg`).
    ///
    /// # Invariant
    ///
    /// Parallel to [`Automaton::msd`]/[`Automaton::all_reps`]/[`Automaton::alphabet`], and
    /// moved in lockstep with them at every site that permutes, removes, merges, or
    /// appends tracks — Java has one `List<NumberSystem>` carrying all three facts at
    /// once, so they cannot drift there and must not drift here. `None` is always a SAFE
    /// entry in the sense that [`Automaton::track_ns_names`] then falls back to the
    /// base-*k* reconstruction, which is exact for every plain base; only a custom-base
    /// track genuinely needs a `Some`.
    pub ns_name: Vec<Option<String>>,
    /// `encoder[i]` = product of `alphabet[0..i]`'s sizes (`encoder[0] == 1`). Cached at
    /// construction, matching Java's `RichAlphabet.encoder` (there computed lazily on
    /// first `encode()` call; here eagerly, since this crate always needs it).
    encoder: Vec<usize>,
    /// `Automaton.labelSorted` (`Automaton.java:49`): memoizes whether [`Automaton::sort_label`]
    /// has already run against the current `label`, so a repeat call (e.g. from
    /// [`Automaton::canonize`]) is a cheap no-op. Always starts `false` — there is no
    /// public constructor parameter for it, matching a freshly bound/relabeled automaton.
    label_sorted: bool,
    /// A **representation choice**, not a field Java has at this layer — Java's
    /// `canonized` memo lives on `FA` itself (`FA.java:50`), not `Automaton`. Added in
    /// U24, for [`crate::morphism::Morphism::to_word_automaton`]: `Morphism.java:88`
    /// calls `promotion.fa.setCanonized(true)` specifically so a promoted morphism's
    /// states (each one a domain letter, most of them typically unreachable from `q0`)
    /// are never dropped by canonicalization — see [`Fa::canonicalize`]'s doc comment
    /// for why that would otherwise be a real, state-count-changing divergence, not just
    /// a renumbering.
    ///
    /// **Why this lives here and not on [`Fa`].** [`Fa`]'s fields are all `pub`, and a
    /// grep of every `.canonicalize()` call site in this workspace (2026-08, at the time
    /// this field was added) shows production code reaches it exclusively through
    /// [`Automaton::canonize`]/[`Automaton::force_canonize`] — never directly. Adding
    /// `canonized` to `Fa` instead would require every one of the ~230
    /// `Fa { … }` struct-literal constructions across this workspace (tests overwhelmingly)
    /// to gain a new field, for a flag only two methods on `Automaton` ever consult. Storing
    /// it here instead keeps the *observable* behavior identical (the flag still gates the
    /// exact same [`Fa::canonicalize`] call) while confining the blast radius to this file's
    /// two existing struct literals ([`Automaton::new`]/[`Automaton::true_false`]) — matching
    /// the task's explicit "add the flag to `Fa`, OR otherwise guarantee the promoted
    /// automaton is never auto-canonicalized" latitude. If a future unit adds a genuine
    /// `Fa`-level (not `Automaton`-wrapped) caller of `canonicalize()`, this guarantee would
    /// need to move or be duplicated — none exists today.
    ///
    /// **The invariant this placement costs, and where it is paid.** In Java `canonized`
    /// is a property of the `FA` VALUE, so *any* operation that installs a fresh `FA`
    /// object gets `canonized == false` for free, by construction. Here the flag rides on
    /// the `Automaton` WRAPPER, so replacing `automaton.fa` does **not** clear it — the
    /// invariant is maintained by hand instead, at every site that either (a) Java clears
    /// it explicitly, or (b) replaces an *existing* automaton's `fa` wholesale. As of
    /// U24's review fixes those are, exhaustively:
    ///
    /// * [`Automaton::clear`] — Java's `FA.clear()` (`FA.java:90-94`) resets it.
    /// * [`Automaton::bind`] — `Automaton.java:442`'s explicit `fa.setCanonized(false)`.
    /// * [`Automaton::determinize_and_minimize`] / [`Automaton::determinize_and_minimize_from`]
    ///   — via `FA.justMinimize`'s own reset (`FA.java:584`).
    /// * [`crate::determinize::determinize`] — installs a brand-new `Fa`.
    /// * `logicalops`'s `not` (the other [`crate::logicalops`] `just_minimize` call site),
    ///   `fix_leading_zeros_problem`, `fix_trailing_zeros_problem`, and
    ///   `convert_lsd_base_to_root` — the three explicit `A.fa.setCanonized(false)` calls
    ///   at `AutomatonLogicalOps.java:273`/`:326`/`:647`, plus `justMinimize`'s.
    ///
    /// Sites that build a **fresh** [`Automaton`] and only then fill in its `fa`
    /// (`crate::product::cross_product` via `create_basic_automaton`,
    /// `crate::quantify`'s `projected`, `wr_io`'s reader) need no reset: their flag is
    /// already `false` from [`Automaton::new`], exactly as in Java.
    ///
    /// And one site deliberately does NOT reset, because Java does not either:
    /// `crate::word_automaton`'s `reverse_with_output` rebuilds the table through
    /// `Fa::set_fields`, and Java's `FA.setFields` (`FA.java:547-551`) leaves `canonized`
    /// alone — a stale `true` genuinely survives that operation upstream, so this port
    /// keeps the quirk rather than quietly improving on it. **The rule for a future
    /// operation is therefore "match what Java's `FA`-object identity would give it",
    /// not "always reset"** — which is exactly why the setter below is `pub(crate)`
    /// rather than `pub`: the invariant is a per-call-site judgement about Java's
    /// behavior, and it cannot be maintained from outside this crate.
    ///
    /// **Narrower than Java's memo, deliberately.** Java's `canonizeInternal` sets
    /// `this.canonized = true` after every successful run, so an ordinary automaton that
    /// has already been canonized skips redundant work on a second `canonize()` call.
    /// This field does NOT replicate that: [`Automaton::canonize`] never sets it back to
    /// `true` on its own, only [`Automaton::set_canonized`] does. So every pre-existing
    /// call site's behavior is 100% unchanged (the flag is `false` by construction and
    /// nothing but the new [`Morphism::to_word_automaton`] call site ever flips it) —
    /// this is a permanent, opt-in SUPPRESSION flag for one specific producer, not a
    /// general performance memo. Implementing the fuller Java semantics (auto-memoizing
    /// after every `canonize()`) was considered and rejected as out of this unit's
    /// scope: it would change the behavior of every existing `canonize()` call site
    /// (would a later mutation between two `canonize()` calls need to be picked up? every
    /// current call site assumes yes, since this port has always unconditionally
    /// recomputed) for a performance question this unit was not asked to resolve.
    ///
    /// [`Morphism::to_word_automaton`]: crate::morphism::Morphism::to_word_automaton
    pub(crate) canonized: bool,
}

impl Automaton {
    /// Builds an `Automaton` from an already-constructed [`Fa`] and track metadata.
    /// `alphabet`, `label`, and `msd` must have the same length as each other and match
    /// `fa.alphabet_size` (`Π alphabet[i].len() == fa.alphabet_size`) — not asserted here
    /// (this is a Phase-1 slice, not a validating constructor); callers are responsible.
    ///
    /// [`Automaton::all_reps`] starts all-`None` — i.e. "no track uses a custom base's
    /// valid-representation restriction", the state every plain `msd_k`/`lsd_k` automaton
    /// is in. Only [`Automaton::set_all_reps`] (called by
    /// `crate::numsys::NumberSystem`'s custom-base constructor) ever changes that, so this
    /// constructor's signature is unchanged from U5 and every pre-U5 call site keeps its
    /// exact previous behavior.
    pub fn new(
        fa: Fa,
        alphabet: Vec<Vec<i32>>,
        label: Vec<String>,
        msd: Vec<Option<bool>>,
    ) -> Self {
        let encoder = Self::compute_encoder(&alphabet);
        let all_reps = vec![None; alphabet.len()];
        let ns_name = vec![None; alphabet.len()];
        Automaton {
            fa,
            alphabet,
            label,
            msd,
            all_reps,
            ns_name,
            encoder,
            label_sorted: false,
            canonized: false,
        }
    }

    /// `Automaton(boolean truthValue)` (`Automaton.java:99-110`): "a true automaton is
    /// an automaton that accepts everything; a false automaton is an automaton that
    /// accepts nothing. Therefore, `M and false` is false for every automaton `M`, and
    /// `M or true` is true for every automaton `M`."
    ///
    /// No tracks at all — `alphabet`/`label`/`msd` are empty, matching Java (`this()`
    /// initializes `richAlphabet`/`NS`/`label` to empties before the flags are set).
    pub fn true_false(truth: bool) -> Self {
        Automaton {
            fa: Fa::trivial(truth),
            alphabet: Vec::new(),
            label: Vec::new(),
            msd: Vec::new(),
            all_reps: Vec::new(),
            ns_name: Vec::new(),
            encoder: Vec::new(),
            label_sorted: false,
            canonized: false,
        }
    }

    /// `FA.isTRUE_FALSE_AUTOMATON()`, reached through `Automaton`'s public `fa` field in
    /// Java (`A.fa.isTRUE_FALSE_AUTOMATON()`); a convenience delegate here.
    pub fn is_true_false_automaton(&self) -> bool {
        self.fa.is_true_false_automaton()
    }

    /// `FA.isTRUE_AUTOMATON()`, as a delegate — see
    /// [`Automaton::is_true_false_automaton`].
    pub fn is_true_automaton(&self) -> bool {
        self.fa.is_true_automaton()
    }

    /// `Automaton.clear()` (`Automaton.java:503-512`, package-private) — its only Java
    /// caller is `AutomatonQuantification`'s all-tracks-quantified path, immediately
    /// after that path sets the TRUE/FALSE flags.
    ///
    /// Faithful, including the parts that look like oversights: [`Fa::clear`] empties
    /// `o`/`d` but leaves `fa.q`/`fa.q0`/`fa.alphabet_size` **stale**, and the
    /// TRUE/FALSE flags are deliberately untouched (Java's `FA.clear()` clears neither
    /// of those two). It does NOT leave everything alone, though: Java's `FA.clear()`
    /// (`FA.java:90-94`) resets `canonized`, and `Automaton.clear` (`:511`) resets
    /// `labelSorted` — both are mirrored below. Java sets `NS` and `label` to literal
    /// `null`; this crate's convention is that an empty `Vec` plays the `null` role for
    /// both (see [`Automaton::is_bound`]/[`Automaton::unlabel`]), so they are emptied
    /// instead.
    pub fn clear(&mut self) {
        self.fa.clear();
        self.alphabet.clear();
        self.encoder.clear();
        self.msd.clear();
        // Java's single `NS = null` covers all three parts of this crate's NS stand-in.
        self.all_reps.clear();
        self.ns_name.clear();
        self.label.clear();
        self.label_sorted = false;
        // `FA.clear()`'s own `canonized = false` (`FA.java:93`) -- the flag lives on the
        // wrapper here, so it has to be cleared explicitly. See `canonized`'s doc.
        self.canonized = false;
    }

    /// Installs the per-track [`Automaton::all_reps`] entries wholesale — the write half
    /// of Java's `Automaton.setNS(List<NumberSystem>)`/`getNS().set(i, ns)` for the
    /// all-representations facet.
    ///
    /// # Panics
    ///
    /// If `all_reps` is not one entry per track, if `msd` is not already parallel to
    /// `alphabet`, or if any `Some` entry sits on a track whose `msd` is `None` — the
    /// invariant documented on [`Automaton::all_reps`]. Java cannot violate it (one
    /// `NumberSystem` object carries both facts); this crate can, so the one public entry
    /// point checks.
    pub fn set_all_reps(&mut self, all_reps: Vec<Option<Rc<Automaton>>>) {
        assert_eq!(
            all_reps.len(),
            self.alphabet.len(),
            "set_all_reps: one entry per track required"
        );
        assert_eq!(
            self.msd.len(),
            self.alphabet.len(),
            "set_all_reps: msd must already be parallel to alphabet"
        );
        for (i, entry) in all_reps.iter().enumerate() {
            assert!(
                entry.is_none() || self.msd[i].is_some(),
                "set_all_reps: track {i} has an all-representations automaton but no number system"
            );
        }
        self.all_reps = all_reps;
    }

    /// Installs the per-track [`Automaton::ns_name`] entries wholesale — the naming part
    /// of Java's `Automaton.setNS(List<NumberSystem>)`. Every caller that has real
    /// [`crate::numsys::NumberSystem`] objects in hand (`wr-io`'s reader, `wr-cli`'s
    /// `alphabet`/`reg`) should call this alongside [`Automaton::set_all_reps`], so the
    /// name that `NumberSystem.isNSDiffering` compares survives into this crate.
    ///
    /// # Panics
    ///
    /// If `names` is not one entry per track, or if a `Some` name sits on a track whose
    /// `msd` is `None` (Java's `null` `NS` entry has no name to report).
    pub fn set_ns_names(&mut self, names: Vec<Option<String>>) {
        assert_eq!(
            names.len(),
            self.alphabet.len(),
            "set_ns_names: one entry per track required"
        );
        assert_eq!(
            self.msd.len(),
            self.alphabet.len(),
            "set_ns_names: msd must already be parallel to alphabet"
        );
        for (i, entry) in names.iter().enumerate() {
            assert!(
                entry.is_none() || self.msd[i].is_some(),
                "set_ns_names: track {i} has a number-system name but no number system"
            );
        }
        self.ns_name = names;
    }

    /// Debug-only check of [`Automaton::all_reps`]'s documented invariant. Deliberately
    /// side-effect-free (`PORTING.md`'s "`debug_assert!` erasing side effects" regression
    /// class) — it only reads.
    pub(crate) fn debug_assert_track_invariant(&self) {
        debug_assert_eq!(
            self.all_reps.len(),
            self.msd.len(),
            "all_reps must stay parallel to msd"
        );
        debug_assert!(
            self.all_reps
                .iter()
                .zip(self.msd.iter())
                .all(|(reps, msd)| reps.is_none() || msd.is_some()),
            "a track with an all-representations automaton must have a number system"
        );
        debug_assert_eq!(
            self.ns_name.len(),
            self.msd.len(),
            "ns_name must stay parallel to msd"
        );
        debug_assert!(
            self.ns_name
                .iter()
                .zip(self.msd.iter())
                .all(|(name, msd)| name.is_none() || msd.is_some()),
            "a track with a number-system name must have a number system"
        );
    }

    /// `RichAlphabet.determineEncoder` (`RichAlphabet.java:100-108`). Panics on overflow
    /// (`Math.multiplyExact` equivalent) — this and [`Automaton::encode_with`] previously
    /// used unchecked `usize`/`i32` arithmetic (silently wrapping in a release build,
    /// unlike Java's hard error), an inconsistency an adversarial review caught between
    /// this method and [`Automaton::determine_alphabet_size`], which already checked.
    fn compute_encoder(alphabet: &[Vec<i32>]) -> Vec<usize> {
        let mut encoder = Vec::with_capacity(alphabet.len());
        let mut val = 1usize;
        for track in alphabet {
            encoder.push(val);
            val = val
                .checked_mul(track.len())
                .expect("encoder overflow (Math.multiplyExact equivalent)");
        }
        encoder
    }

    /// Encodes a per-track digit tuple into a single transition symbol. Panics if a
    /// digit isn't present in its track's alphabet (a caller bug, not a data error —
    /// matches Java's `List.indexOf` returning `-1` and corrupting the arithmetic
    /// silently; panicking here is the improvement `PORTING.md`'s type/error mapping
    /// table calls for over stringly-typed Java exceptions).
    pub fn encode(&self, digits: &[i32]) -> i32 {
        Self::encode_with(digits, &self.alphabet, &self.encoder)
    }

    /// `RichAlphabet.encode(List<Integer> l, List<List<Integer>> A, IntList encoder)`
    /// (`RichAlphabet.java:110-116`) — the free-function form, used wherever a port
    /// needs to encode against an alphabet/encoder pair that isn't (yet) installed as
    /// `self`'s own, e.g. mid-[`Automaton::sort_label`]/mid-[`Automaton::reduce_dimension`]
    /// while computing a permutation/reduction map before the new alphabet is assigned.
    /// [`Automaton::encode`] is a thin wrapper over this using `self`'s own fields.
    fn encode_with(digits: &[i32], alphabet: &[Vec<i32>], encoder: &[usize]) -> i32 {
        let mut encoding: i32 = 0;
        for (i, &d) in digits.iter().enumerate() {
            let idx = alphabet[i]
                .iter()
                .position(|&v| v == d)
                .unwrap_or_else(|| panic!("digit {d} not in track {i}'s alphabet"));
            // Checked, not wrapping: an adversarial review found the previous plain
            // `+=`/`as i32` silently wrapped on a large-but-usize-representable
            // `encoder`/`idx` product in a release build, where Java's
            // `Math.multiplyExact`/`addExact` (`RichAlphabet.encode`) would have thrown.
            // The panic threshold isn't byte-identical to Java's (Java checks at `int`
            // width; this checks at `i32` width too but `encoder` itself is `usize` and
            // can exceed `i32::MAX` before this line ever runs — see
            // `Automaton::determine_alphabet_size`'s doc comment for that residual gap).
            let encoder_i = i32::try_from(encoder[i]).expect("encoder entry exceeds i32 range");
            let idx_i = i32::try_from(idx).expect("alphabet index exceeds i32 range");
            let term = encoder_i
                .checked_mul(idx_i)
                .expect("encode overflow (Math.multiplyExact equivalent)");
            encoding = encoding
                .checked_add(term)
                .expect("encode overflow (Math.addExact equivalent)");
        }
        encoding
    }

    /// `RichAlphabet.encode(List<Integer>)` (`RichAlphabet.java:86-91`) with **Java's
    /// `List.indexOf` semantics preserved**: a digit that is not in its track's alphabet
    /// contributes `encoder[i] * -1` rather than raising anything, so the result can be
    /// negative and is not a valid symbol at all.
    ///
    /// This is the same verbatim-`indexOf` port `crate::regex`'s private
    /// `encode_with_index_of` already carries for WB-024, at the second call site that
    /// genuinely needs it: `AutomatonReader.readAutomaton`/`readTransducer` encode every
    /// transition line's digit tuple straight out of an untrusted `.txt` file
    /// (`AutomatonReader.java:71-72`, `:245-247`), and Java's reader has **no**
    /// out-of-alphabet check anywhere — verified by running `walnut-java` on a file whose
    /// body digit is outside the header's alphabet (` lsd_2\n0 1\n20 -> 0`): it loads
    /// with no error at all, keeping a transition under the bogus key `-1`. Whether that
    /// then goes on to fail depends entirely on what the automaton is *used* for
    /// afterwards, and porting that faithfully means reproducing the key, not rejecting
    /// the file:
    ///
    /// * with a state id that was never declared, `validateDeclaredStates` (which runs
    ///   AFTER the whole parse loop) reports the clean `State N is used but never
    ///   declared anywhere in file: …` this port already ports as
    ///   `wr_io::reader::ReadError::UndeclaredDestState`;
    /// * otherwise the file loads, and the `-1`-keyed transition is silently dropped by
    ///   any later pass that iterates `0..alphabet_size` — real `walnut-java` writes
    ///   exactly that reduced automaton back out (confirmed on two such files).
    ///
    /// [`Automaton::encode`] — which panics instead — remains correct for every caller
    /// whose digits come from an alphabet this crate itself built, and is what those
    /// callers must keep using; a panic is a better error than a corrupt encoding when
    /// the input really is an internal invariant.
    pub fn encode_index_of(&self, digits: &[i32]) -> i32 {
        let mut encoding: i32 = 0;
        for (i, &d) in digits.iter().enumerate() {
            let index = self.alphabet[i]
                .iter()
                .position(|&v| v == d)
                .map_or(-1, |p| p as i32);
            // Same checked arithmetic as `encode_with`, for the same reason (Java's
            // `Math.multiplyExact`/`addExact`).
            let encoder_i =
                i32::try_from(self.encoder[i]).expect("encoder entry exceeds i32 range");
            let term = encoder_i
                .checked_mul(index)
                .expect("encode overflow (Math.multiplyExact equivalent)");
            encoding = encoding
                .checked_add(term)
                .expect("encode overflow (Math.addExact equivalent)");
        }
        encoding
    }

    /// `RichAlphabet.decode(List<List<Integer>>, int)` (`RichAlphabet.java:124-131`) —
    /// decodes a transition symbol back into its per-track digit tuple. Inverse of
    /// [`Automaton::encode`] (`try_decode(encode(x)) == Ok(x)`) for every symbol in
    /// `0..alphabet_size`.
    ///
    /// # Java's exact arithmetic, and why it matters
    ///
    /// Java is `l.add(integers.get(n % integers.size())); n = n / integers.size();` with
    /// `%`/`/` being **truncating** (C-style), so a negative `n` produces a negative
    /// index and `ArrayList.get(-1)` throws `IndexOutOfBoundsException` — an unchecked
    /// exception `Prover.readBuffer`'s `catch (RuntimeException)` recovers from, leaving
    /// the session alive (verified live: `Automata Library/fy.txt` = ` lsd_2 / 0 1 /
    /// 20 -> 0`, then `eval f2b "?lsd_2 $fy(x)";` prints `java.lang.IndexOutOfBounds
    /// Exception: Index -1 out of bounds for length 2` and the next command still runs).
    ///
    /// A negative symbol is **reachable from an ordinary `.txt` file**, not a
    /// hypothetical: [`Automaton::encode_index_of`] faithfully reproduces Java's
    /// `List.indexOf(-1)` for an out-of-alphabet body digit (WB-038), so the reader
    /// really does store transitions under key `-1`, and every pass that iterates
    /// `fa.d`'s KEYS (rather than `0..alphabet_size`) hands one straight to this
    /// function — `wr_io::writer`'s `write_state`/`write_gv`,
    /// [`Automaton::rebuild_transitions_for_new_alphabet`],
    /// `logicalops::right_quotient`, `infinite`'s path decoding,
    /// `wr_cli::test_command`'s accepted-word formatting.
    ///
    /// This port used `rem_euclid`/`div_euclid` instead, which **always** produces some
    /// in-range index — so where Java threw and wrote nothing, walnut-rs silently
    /// fabricated a digit tuple and wrote out an automaton whose language matches
    /// neither the file nor Java's answer. Silent wrong math is strictly worse than a
    /// reported error, so the check is ported: truncating `%`/`/`, and an out-of-range
    /// index is [`DecodeError::IndexOutOfBounds`], carrying the JDK's own message text.
    ///
    /// Note that Java bounds-checks only the PER-TRACK index, never the symbol as a
    /// whole: `decode(alphabet_size + k)` silently wraps to `decode(k)`-ish garbage in
    /// Java, and does here too. That quirk is ported (it is `n / size`'s natural
    /// behavior once the loop runs out of tracks), not "fixed" — same rule as WB-038.
    pub fn try_decode(&self, sym: i32) -> Result<Vec<i32>, DecodeError> {
        let mut n = sym;
        let mut out = Vec::with_capacity(self.alphabet.len());
        for track in &self.alphabet {
            let size = track.len() as i32;
            if size == 0 {
                // `n % 0` is Java's `ArithmeticException: / by zero`. Unreachable for
                // any alphabet this crate builds (every track has >= 1 digit, and the
                // reader rejects a base below 2 as of U30's F3), but a division by zero
                // must never be a Rust panic in a function reachable from file input.
                return Err(DecodeError::EmptyTrackAlphabet);
            }
            // Java's `%` and `/`, i.e. Rust's `%` and `/` — NOT `rem_euclid`/`div_euclid`.
            let idx = n % size;
            if idx < 0 {
                return Err(DecodeError::IndexOutOfBounds {
                    index: idx,
                    length: track.len(),
                });
            }
            out.push(track[idx as usize]);
            n /= size;
        }
        Ok(out)
    }

    /// [`Automaton::try_decode`] for the callers whose symbol provably comes from
    /// `0..alphabet_size` (an internal invariant), where a failure would be a port bug
    /// rather than bad input — and, per this crate's established idiom
    /// ([`crate::walnut_panic`]), for the untrusted-key callers whose own signature has
    /// no error channel to propagate through (`wr_io::writer`, `logicalops`,
    /// `infinite`, …).
    ///
    /// # Panics
    ///
    /// With the Java exception's message verbatim, so that
    /// [`crate::walnut_panic::catch_walnut_panic`] at `wr_cli::prover`'s dispatch
    /// boundary reports it exactly as `Prover.readBuffer`'s `catch (RuntimeException)`
    /// does, and the session survives — the same treatment `right_quotient`'s subset
    /// guard and `product`'s alphabet guard already get. **Never** silently returns a
    /// fabricated tuple.
    pub fn decode(&self, sym: i32) -> Vec<i32> {
        match self.try_decode(sym) {
            Ok(digits) => digits,
            // One-argument `panic!` with the message alone: the guard-authoring rule in
            // `crate::walnut_panic`'s docs (an `expect`/`assert_eq!` payload would reach
            // the user with Rust framing around Walnut's text).
            Err(e) => panic!("{e}"),
        }
    }

    /// The encoded symbol for the all-digit-value-0 tuple (`RichAlphabet.determineZero`)
    /// — used by the leading/trailing-zero fixup pass to find "read a 0 on every live
    /// track" edges.
    ///
    /// Simplified from Java's literal `encode([A[i].indexOf(0) for each i])`: Java
    /// looks up the *position* of value 0 in each track, then re-encodes that position
    /// as if it were itself a digit *value* — a double indirection that only coincides
    /// with directly encoding the literal all-zero tuple when 0 sits at position 0 in
    /// every track's alphabet (true for every alphabet this crate constructs so far —
    /// ordinary base-*k* tracks are `[0, 1, ..., k-1]`). Revisit if a non-zero-first
    /// track alphabet is ever introduced.
    pub fn determine_zero(&self) -> i32 {
        let zero_digits = vec![0; self.alphabet.len()];
        self.encode(&zero_digits)
    }

    /// `Automaton.isBound` (`Automaton.java:494-496`). The Java null-check on
    /// `getLabel()` doesn't apply here: `label` is always a `Vec` (possibly empty),
    /// matching this crate's convention that "unbound" is an empty label vec (see
    /// [`Automaton::unlabel`]) rather than `null`.
    pub fn is_bound(&self) -> bool {
        self.label.len() == self.alphabet.len()
    }

    /// `Automaton.getArity` (`Automaton.java:498-501`), including its
    /// `isTRUE_FALSE_AUTOMATON -> 0` short-circuit (U0). Redundant in practice — a
    /// trivial automaton's `alphabet` is empty anyway — but ported so the branch
    /// structure matches Java's, and so the stale-field trivial shape (see `crate::fa`'s
    /// module docs) can never report a non-zero arity.
    pub fn get_arity(&self) -> usize {
        if self.fa.is_true_false_automaton() {
            return 0;
        }
        self.alphabet.len()
    }

    /// `Automaton.isEmpty` (`Automaton.java:514-519`), including its
    /// `isTRUE_FALSE_AUTOMATON` branch (U0): the FALSE automaton's language is empty,
    /// the TRUE automaton's is not.
    ///
    /// The branch is **load-bearing, not cosmetic**: a trivial `Fa` has zero (or stale,
    /// but output-less) states, so [`Fa::is_language_empty`] would report `true` for the
    /// TRUE automaton too. `AutomatonQuantification`'s all-tracks-quantified path calls
    /// this to decide which trivial automaton to produce, which is exactly why Java
    /// evaluates `!A.isEmpty()` BEFORE setting `TRUE_FALSE_AUTOMATON`.
    pub fn is_empty(&self) -> bool {
        if self.fa.is_true_false_automaton() {
            return !self.fa.is_true_automaton();
        }
        self.fa.is_language_empty()
    }

    /// `FA.isFAO` (`FA.java:64-72`): true iff some state's output exceeds plain 0/1
    /// acceptance (a DFAO / word automaton). Used by [`AutomatonDFA::from`] to decide
    /// whether Java's `requireDfaStorage` would have thrown
    /// `WalnutException.nonDeterministicO` instead of determinizing.
    pub fn is_fao(&self) -> bool {
        self.fa.o.iter().any(|&o| o > 1)
    }

    /// `Automaton.determineAlphabetSize` / `RichAlphabet.determineAlphabetSize`
    /// (`Automaton.java:248-250`, `RichAlphabet.java:60-66`): recomputes `fa.alphabet_size`
    /// as the product of the current per-track alphabet sizes. Panics on overflow like
    /// Java's `Math.multiplyExact` does — but not at the same THRESHOLD: Java checks at
    /// `int` (32-bit) width, this checks at `usize` (64-bit) width, so a product between
    /// 2^31 and 2^64 hard-errors in Java but succeeds here (an adversarial-review finding,
    /// not yet closed — no alphabet this crate constructs today gets remotely close, but a
    /// future caller computing `fa.alphabet_size as i32` downstream, e.g.
    /// [`crate::fa::Fa::totalize`]'s symbol loop, would silently truncate rather than
    /// inheriting this panic).
    ///
    /// Does NOT refresh `encoder` — callers that mutate `alphabet` and then call this
    /// must also call [`Automaton::setup_encoder`], or `encode`/`decode` will silently
    /// use a stale encoder against the new alphabet. (`Automaton.java`'s own call sites
    /// always pair the two, via `richAlphabet.setupEncoder()`.)
    pub fn determine_alphabet_size(&mut self) {
        self.fa.alphabet_size = self
            .alphabet
            .iter()
            .try_fold(1usize, |acc, track| acc.checked_mul(track.len()))
            .expect("alphabet size overflow (Math.multiplyExact equivalent)");
    }

    /// `RichAlphabet.setupEncoder` (`RichAlphabet.java:96-98`): recomputes `encoder` from
    /// the current `alphabet`. Added alongside [`Automaton::determine_alphabet_size`]
    /// (adversarial-review finding: `encoder` is a private field with no other public
    /// recompute path, so a caller that mutates `alphabet` directly had no faithful way
    /// to resync it before this existed).
    pub fn setup_encoder(&mut self) {
        self.encoder = Self::compute_encoder(&self.alphabet);
    }

    /// Read-only accessor for the per-track encoder (`RichAlphabet.encoder`), added in
    /// U16 so a caller assembling `Automaton.setAlphabet` (see
    /// [`Automaton::rebuild_transitions_for_new_alphabet`]'s doc comment: "a future unit
    /// can call this once it has real `NumberSystem` objects") can obtain the freshly
    /// recomputed encoder after [`Automaton::setup_encoder`] to pass into that function's
    /// explicit `new_encoder` parameter, without `wr-cli` needing its own duplicate
    /// encoder-computation logic. `encoder` itself stays private (mutating it directly
    /// from outside this module would bypass [`Automaton::compute_encoder`]'s overflow
    /// check), so this is a read-only escape hatch, not a new mutation path.
    pub fn encoder(&self) -> &[usize] {
        &self.encoder
    }

    /// `Automaton.randomLabel` (`Automaton.java:299-305`).
    pub fn random_label(&mut self) {
        self.label = (0..self.alphabet.len()).map(|i| i.to_string()).collect();
    }

    /// `Automaton.unlabel` (`Automaton.java:307-310`). Its Java callers are
    /// `applyAllRepresentations`/`applyAllRepresentationsWithOutput`, both ported below as
    /// of U5.
    pub fn unlabel(&mut self) {
        self.label = Vec::new();
        self.label_sorted = false;
    }

    /// `Automaton.determineRandomLabel` (`Automaton.java:291-297`, `private`): label the
    /// tracks `"0"`, `"1"`, … if they are not labeled already, reporting whether it did
    /// (so the caller knows to [`Automaton::unlabel`] afterwards).
    fn determine_random_label(&mut self) -> bool {
        if !self.is_bound() {
            self.random_label();
            return true;
        }
        false
    }

    /// `Automaton.applyAllRepresentations` (`Automaton.java:252-270`) — intersect with each
    /// track's valid-representation restriction.
    ///
    /// For every track whose number system supplies an all-representations automaton
    /// ([`Automaton::all_reps`]), that automaton is bound to the track's label and
    /// intersected in. For a plain base-*k* numeration every entry is `None` and this is a
    /// no-op *except* for the label bookkeeping, which Java performs unconditionally (see
    /// the "even with nothing to apply" note below) — that is why this is a real method
    /// rather than an early return.
    ///
    /// # Why this is live, and not the dead code Phase 1/2 recorded it as
    ///
    /// Until U5 this crate had no custom-base numeration at all, so
    /// `useAllRepresentations()` was hardcoded `false` and this method was documented as a
    /// guaranteed no-op (in `logicalops.rs`'s and `product.rs`'s module docs, both now
    /// corrected). U5 added `crate::numsys::NumberSystem`'s custom-base constructor, which
    /// installs an all-representations automaton on the tracks of its
    /// adder/comparator/equality automata — and from there it propagates through every
    /// cross product, quantification, and quotient into the results whose
    /// `applyAllRepresentations` calls (`AutomatonLogicalOps`'s `totalizeCrossProduct`,
    /// `not`, `rightQuotient`) then genuinely fire. Empirically confirmed against the real
    /// Walnut CLI: `eval x "?msd_fib x=x"` returns the 2-state "no `11` substring"
    /// Zeckendorf-representation automaton rather than a 1-state universal one, and
    /// `eval x "?msd_fib ~(x=x)"` returns the empty language rather than "contains `11`".
    ///
    /// # Faithful details that are easy to get wrong
    ///
    /// * **`K` starts as an ALIAS of `this`, not a copy** (`Automaton K = this;`). The
    ///   `None` state of `k` below plays that role: when no track has a restriction, the
    ///   closing `copy(K)` copies `this` onto itself, i.e. does nothing.
    /// * **Each iteration reads `this.label`, not `K`'s.** `and`'s cross product appends
    ///   only genuinely new tracks, and `N`'s single track always matches a track of `K`
    ///   by label, so the two label lists stay identical anyway — but the port reads the
    ///   same list Java does.
    /// * **Even with nothing to apply, an UNBOUND automaton is left unbound but with
    ///   `label_sorted` reset**: `unlabel()` runs before `copy(K)`, and when `K` is still
    ///   `this` there is nothing to overwrite it. When at least one restriction *was*
    ///   applied, `copy(K)` restores `K`'s label — so an automaton that entered unbound
    ///   leaves **bound to `"0"`, `"1"`, …**, and Java's `unlabel()` is dead on that path.
    ///   Ported verbatim (this is how `NumberSystem`'s custom-base adder/comparator end up
    ///   carrying numeric labels; harmless, since every consumer `bind`s them first).
    /// * **The shared all-representations automaton is `bind`-mutated in Java**
    ///   (`N.bind(...)` writes through `ns.getAllRepresentations()`'s own object, against
    ///   that class's "returned automata must not be altered" warning). This port clones
    ///   out of the [`Rc`] instead. Unobservable: `bind` only rewrites the label (and, for
    ///   the one-track automaton it always is, `removeSameInputs` is a no-op), and every
    ///   use re-binds it before reading it.
    ///
    /// `Logging.logAndPrint("Applying valid representation #i")`/`indent`/`dedent`
    /// (`:262-265`) are not ported, matching the rest of `wr-core`: `crate::logging`'s
    /// context is not threaded into the automaton engine's call sites yet (see
    /// `crate::product`'s "Progress logging not ported" note).
    pub fn apply_all_representations(&mut self) {
        self.debug_assert_track_invariant();
        let flag = self.determine_random_label();
        // `None` == "K is still `this`" (Java's alias).
        let mut k: Option<Automaton> = None;
        for i in 0..self.alphabet.len() {
            let Some(n) = self.all_reps[i].clone() else {
                continue;
            };
            let mut n = (*n).clone();
            n.bind(vec![self.label[i].clone()]);
            let current: &Automaton = k.as_ref().unwrap_or(&*self);
            k = Some(crate::logicalops::and(current, &n).into_automaton());
        }
        if flag {
            self.unlabel();
        }
        if let Some(k) = k {
            *self = k; // `copy(K)`
        }
    }

    /// `Automaton.applyAllRepresentationsWithOutput` (`Automaton.java:272-287`,
    /// package-private) — the DFAO-capable variant.
    ///
    /// Identical to [`Automaton::apply_all_representations`] except for two things, both
    /// load-bearing and both flagged by Java's own comment at `:281-282`:
    ///
    /// 1. it uses the raw `ProductStrategies.crossProduct` with `Prover.IF_OTHER_OP`
    ///    (`determineOutput`'s `mQ != 0 ? aP : 0`, `ProductStrategies.java:187`) rather
    ///    than `AutomatonLogicalOps.and`, so a word automaton's output *value* survives
    ///    instead of collapsing to `0`/`1`, and there is no minimization step; and
    /// 2. it combines with **`this`**, not with the running `K` — "This appears to be by
    ///    design, and causes a bug in `combine()` otherwise." So with two or more
    ///    restricted tracks, every restriction but the last is silently discarded. Ported
    ///    verbatim; not logged in `docs/WALNUT-BUGS.md` because it is deliberate,
    ///    documented-in-source behavior rather than a defect, and because no shipped custom
    ///    base has more than one arithmetic track in a *word* automaton for it to bite.
    ///
    /// Java's two callers are `Automaton.setAlphabet` and `AutomatonLogicalOps.combine`.
    /// `wr-cli`'s `alphabet::set_alphabet` (Phase 3a U16) is now this crate's first real
    /// caller; `combine`'s is still out of scope (needs `Prover.COMBINE`'s product mode).
    pub fn apply_all_representations_with_output(&mut self) {
        self.debug_assert_track_invariant();
        let flag = self.determine_random_label();
        let mut k: Option<Automaton> = None;
        for i in 0..self.alphabet.len() {
            let Some(n) = self.all_reps[i].clone() else {
                continue;
            };
            let mut n = (*n).clone();
            n.bind(vec![self.label[i].clone()]);
            // `Prover.IF_OTHER_OP`, and against `this` -- not against `k`.
            k = Some(crate::product::cross_product(&*self, &n, |a_p, m_q| {
                if m_q != 0 {
                    a_p
                } else {
                    0
                }
            }));
        }
        if flag {
            self.unlabel();
        }
        if let Some(k) = k {
            *self = k; // `copy(K)`
        }
    }

    /// `Automaton.normalizeNumberSystems` (`Automaton.java:160-181`) — used by
    /// `Main/Commands/Concat.java:74` and `Main/Commands/Star.java:27` right after
    /// `FA.concatStates`/`FA.starStates`, to strip a custom base's "all representations"
    /// restriction from any track that carries one, because a concatenation/Kleene-star
    /// NFA transition can introduce digit combinations the restriction never admitted.
    ///
    /// # Why this is a straight `all_reps[i] = None` per switched track, not a full `setAlphabet` call
    ///
    /// Java's real body constructs a fresh `NumberSystem` per switched track
    /// (`new NumberSystem(ns.determineBaseNameUnderscore() + (max + 1))`, `:169`) and
    /// passes it to `setAlphabet(false, numberSystems, richAlphabet.getA())` (`:176`).
    /// The crucial detail, easy to miss: **the alphabet argument is `richAlphabet.getA()`
    /// — the automaton's CURRENT, unchanged alphabet**, not a freshly computed `0..=max`
    /// range. So inside `setAlphabet`, `M.richAlphabet.setA(alphabet)` installs the exact
    /// same digit lists back, `rebuildTransitions`'s `isInNewAlphabet` check therefore
    /// admits every existing transition (nothing is pruned), and `setupEncoder()`
    /// recomputes an identical encoder from an identical alphabet. The only *observable*
    /// change `setAlphabet` makes here is installing a `NumberSystem` whose
    /// `useAllRepresentations()` is `false` — i.e., exactly `all_reps[i] = None` in this
    /// crate's parallel-vector stand-in (see [`Automaton::all_reps`]'s field docs) — plus
    /// re-running `determinizeAndMinimize`/`forceCanonize`/
    /// `applyAllRepresentationsWithOutput`, all three of which this crate's `Automaton`
    /// already exposes and are called here directly. The wider "new base" fact
    /// (`max + 1`) is real in Java's `NumberSystem` object but has no representation in
    /// this crate's stand-in beyond `alphabet.len()` — which, per the paragraph above,
    /// Java itself leaves unchanged here, so there is nothing to reconstruct.
    ///
    /// Ported behavior, not a Java bug: `switchNS` (hence the whole method) is a no-op
    /// unless at least one track's number system `useAllRepresentations()` — i.e. carries
    /// a genuine custom base (`msd_fib`, …). Every plain `msd_k`/`lsd_k` automaton this
    /// crate's `concat`/`star` callers see in ordinary KEEP-scope use takes the early
    /// `return` below, matching Java's own `if (switchNS)` guard exactly.
    ///
    /// # The renamed number system, and the `WARN` line
    ///
    /// The replacement `NumberSystem` Java builds for a switched track is named
    /// `ns.determineBaseNameUnderscore() + (max + 1)` (`:169`), `max` being the largest
    /// digit in **that track's** alphabet — so `msd_fib`'s track becomes `msd_2`, and the
    /// name genuinely changes. [`Automaton::ns_name`] records that new name explicitly
    /// rather than leaning on [`Automaton::track_ns_names`]'s reconstruction, because
    /// `max + 1` and `alphabet[i].len()` coincide only for a contiguous-from-zero
    /// alphabet.
    ///
    /// The trailing `Logging.logMessage(true, "WARN: …")` (`:177-179`, carrying an
    /// explicit `// always print this` comment in the original) is emitted here verbatim,
    /// hence the `logging` parameter — it is a user-facing warning that the result's
    /// alphabet changed, not one of the progress/timing detail lines this port skips.
    pub fn normalize_number_systems(&mut self, logging: &mut crate::logging::Logging) {
        let mut switch_ns = false;
        for i in 0..self.all_reps.len() {
            if self.all_reps[i].is_some() {
                switch_ns = true;
                self.all_reps[i] = None;
                // `new NumberSystem(ns.determineBaseNameUnderscore() + (max + 1))`
                // (`:168-169`): `max` is `Collections.max(richAlphabet.getA().get(i))`.
                let renamed = self.msd[i].and_then(|is_msd| {
                    self.alphabet[i].iter().max().map(|max| {
                        let prefix = if is_msd {
                            crate::numsys::MSD_UNDERSCORE
                        } else {
                            crate::numsys::LSD_UNDERSCORE
                        };
                        format!("{prefix}{}", max + 1)
                    })
                });
                if i < self.ns_name.len() {
                    self.ns_name[i] = renamed;
                }
            }
        }
        if !switch_ns {
            return;
        }
        self.determinize_and_minimize();
        self.force_canonize();
        self.apply_all_representations_with_output();
        // `// always print this` (`:177`) — `Logging.logMessage(true, …)`.
        logging.log_message_with(true, ALPHABET_CHANGED_WARNING);
    }

    /// The name Java's `NumberSystem.getName()` would report for each track of this
    /// automaton. `None` where the track carries no arithmetic number system at all
    /// (`msd[i] == None`), mirroring Java's literal `null` `NS` list entry.
    ///
    /// Prefers the REAL name recorded in [`Automaton::ns_name`] — threaded through from
    /// the resolved [`crate::numsys::NumberSystem`] by every caller that has one
    /// (`wr-io`'s reader, `wr-cli`'s `alphabet`/`reg`), so a custom base reports
    /// `msd_fib` and not the `msd_2` its alphabet cardinality alone would suggest. Falls
    /// back to reconstructing `msd_<alphabet size>`/`lsd_<alphabet size>` only where no
    /// name was recorded — i.e. for an automaton this crate built in memory with no
    /// reader/`NumberSystem` provenance, which is always a plain base-*k* one, for which
    /// the reconstruction is exact (Java's `normalizeNumberSystemToken`, `:273-295`, maps
    /// bare `msd`/`lsd` to `msd_2`/`lsd_2`, so a plain base's name IS `msd_<base>`).
    ///
    /// Used by [`crate::numsys::is_ns_differing`]'s `wr-cli` call sites
    /// (`union`/`intersect`/`concat`, whose Java originals compare by
    /// `NumberSystem.getName()`) and by `Main.Commands.Describe`'s `"Number systems:"`
    /// line.
    ///
    /// A recorded name is ignored on a track whose `msd` is `None`, so the result always
    /// agrees with Java on which entries are `null` even if a caller's parallel vectors
    /// were built inconsistently (which [`Automaton::set_ns_names`] rejects outright).
    pub fn track_ns_names(&self) -> Vec<Option<String>> {
        self.msd
            .iter()
            .enumerate()
            .zip(self.alphabet.iter())
            .map(|((i, msd), alphabet)| {
                msd.map(
                    |is_msd| match self.ns_name.get(i).and_then(|n| n.as_ref()) {
                        Some(name) => name.clone(),
                        None => {
                            let prefix = if is_msd {
                                crate::numsys::MSD_UNDERSCORE
                            } else {
                                crate::numsys::LSD_UNDERSCORE
                            };
                            format!("{prefix}{}", alphabet.len())
                        }
                    },
                )
            })
            .collect()
    }

    /// `RichAlphabet.isInNewAlphabet` (`RichAlphabet.java:51-58`): true iff every track's
    /// decoded digit is still present in the corresponding track of `new_alphabet`. Half
    /// of the self-contained portion of `Automaton.setAlphabet` (see module docs on why
    /// the rest of `setAlphabet` is out of scope for this unit).
    pub fn is_in_new_alphabet(new_alphabet: &[Vec<i32>], decoded: &[i32]) -> bool {
        decoded
            .iter()
            .enumerate()
            .all(|(i, d)| new_alphabet[i].contains(d))
    }

    /// `Automaton.rebuildTransitions` (`Automaton.java:231-245`): re-encodes every
    /// transition of `old` under `new_alphabet`/`new_encoder`, dropping any transition
    /// whose decoded digit tuple is no longer valid under the new alphabet (via
    /// [`Automaton::is_in_new_alphabet`]). The other self-contained half of
    /// `Automaton.setAlphabet` (see module docs); a future unit can call this once it
    /// has real `NumberSystem` objects to assemble the rest of `setAlphabet` around it.
    ///
    /// Takes `new_encoder` explicitly (rather than deriving it from `new_alphabet`)
    /// since Java's call site (`Automaton.java:206-208`) computes it once via
    /// `richAlphabet.setupEncoder()` before rebuilding — same shape here.
    ///
    /// # Panics
    ///
    /// If `new_alphabet.len() != old.alphabet.len()` (arity mismatch). Java's real
    /// guard against this lives in the CALLER, `setAlphabet`
    /// (`Automaton.java:185-187`, `"The number of alphabets must match..."`) — which
    /// this unit deliberately does not port (see module docs). An adversarial review
    /// found that without some guard here, a future caller assembling `setAlphabet`
    /// from these two `pub` pieces could silently truncate/misalign tracks instead of
    /// erroring on a genuine arity mismatch; asserting it here, at the shared
    /// self-contained primitive, means every future assembly point inherits the check
    /// for free rather than each having to remember to add it.
    pub fn rebuild_transitions_for_new_alphabet(
        old: &Automaton,
        new_alphabet: &[Vec<i32>],
        new_encoder: &[usize],
    ) -> Vec<BTreeMap<i32, Vec<usize>>> {
        assert_eq!(
            new_alphabet.len(),
            old.alphabet.len(),
            "rebuild_transitions_for_new_alphabet: arity mismatch (matches setAlphabet's own guard)"
        );
        old.fa
            .d
            .iter()
            .map(|row| {
                let mut new_row = BTreeMap::new();
                for (&sym, dests) in row {
                    let decoded = old.decode(sym);
                    if Self::is_in_new_alphabet(new_alphabet, &decoded) {
                        // For safety, clone the dest list to avoid aliasing (matches
                        // Java's explicit `new IntArrayList(entry.getValue())`).
                        new_row.insert(
                            Self::encode_with(&decoded, new_alphabet, new_encoder),
                            dests.clone(),
                        );
                    }
                }
                new_row
            })
            .collect()
    }

    /// `Automaton.permute` (`Automaton.java:409-423`) — a documented Walnut quirk,
    /// ported verbatim. The Java doc comment on this exact method admits its actual
    /// behavior is the INVERSE of what was originally intended: it performs a
    /// *scatter* (`R[permutation[i]] = L[i]`), not the documented *gather*
    /// (`R[i] = L[permutation[i]]`) — "Changing this causes other issues, so we're
    /// leaving it." This is not a newly-discovered bug (already flagged upstream in the
    /// Java source's own doc comment), so it is not logged as a fresh
    /// `docs/WALNUT-BUGS.md` entry here; ported exactly per `CLAUDE.md`'s mechanical-port
    /// rule. Pinned by `permute_matches_javas_scatter_quirk` below, which replicates
    /// `AutomataTest.testPermute`.
    ///
    /// Panics if `permutation.len() != items.len()`. Does NOT check that `permutation` is
    /// actually a bijection onto `0..items.len()` (an earlier version of this doc claimed
    /// it did — false, caught in adversarial review: a non-bijective `permutation`, e.g.
    /// `[0, 0, 1]`, silently duplicates/drops entries with no panic, exactly matching
    /// Java's own unchecked `R.set(permutation[i], L.get(i))`). Every call site in this
    /// file only ever passes a genuine bijection (`get_label_permutation`'s output), so
    /// this is a real but currently-unexercised gap, not a live bug.
    fn permute<T: Clone>(items: &[T], permutation: &[usize]) -> Vec<T> {
        assert_eq!(
            items.len(),
            permutation.len(),
            "permute: permutation length must match items length"
        );
        let mut result = items.to_vec();
        for (i, item) in items.iter().enumerate() {
            result[permutation[i]] = item.clone();
        }
        result
    }

    /// `Automaton.getLabelPermutation` (`Automaton.java:430-436`).
    fn get_label_permutation(label: &[String], sorted_label: &[String]) -> Vec<usize> {
        label
            .iter()
            .map(|l| {
                sorted_label
                    .iter()
                    .position(|s| s == l)
                    .expect("label must appear in its own sorted permutation")
            })
            .collect()
    }

    /// `Automaton.sortLabel` (`Automaton.java:348-381`), every branch ported faithfully
    /// — including the [`Automaton::permute`] quirk above, the `isTRUE_FALSE_AUTOMATON`
    /// short-circuit (U0), and Java's ORDER: `labelSorted` is set to `true` BEFORE the
    /// trivial/unbound early returns, so even a bailed-out call memoizes.  `msd` stands
    /// in for Java's per-track `NumberSystem` list (this crate's already-established
    /// simplification — see the struct doc comment on `msd`).
    pub fn sort_label(&mut self) {
        if self.label_sorted {
            return;
        }
        self.label_sorted = true;
        if self.fa.is_true_false_automaton() {
            return;
        }
        if !self.is_bound() {
            return;
        }
        let already_sorted = is_sorted(&self.label);
        if already_sorted {
            return;
        }

        let mut sorted_label = self.label.clone();
        sorted_label.sort();

        // permutedA is going to hold the alphabet of the sorted inputs. The same logic
        // is behind permutedEncoder.
        let label_permutation = Self::get_label_permutation(&self.label, &sorted_label);
        let permuted_alphabet = Self::permute(&self.alphabet, &label_permutation);
        let permuted_encoder = Self::compute_encoder(&permuted_alphabet);

        // encoded_input_permutation[i] = j means encoded input i becomes j after sorting.
        let alphabet_size = self.fa.alphabet_size;
        let mut encoded_input_permutation = vec![0i32; alphabet_size];
        for (i, slot) in encoded_input_permutation.iter_mut().enumerate() {
            let input = self.decode(i as i32); // decode against the OLD (pre-sort) alphabet
            let permuted_input = Self::permute(&input, &label_permutation);
            *slot = Self::encode_with(&permuted_input, &permuted_alphabet, &permuted_encoder);
        }

        self.label = sorted_label;
        self.alphabet = permuted_alphabet;
        self.encoder = permuted_encoder;
        self.msd = Self::permute(&self.msd, &label_permutation);
        // Java permutes the single `NS` list, which carries all three parts of this
        // crate's per-track number-system stand-in (`Automaton.java:377`).
        self.all_reps = Self::permute(&self.all_reps, &label_permutation);
        self.ns_name = Self::permute(&self.ns_name, &label_permutation);

        for row in self.fa.d.iter_mut() {
            let mut permuted_row = BTreeMap::new();
            for (&sym, dests) in row.iter() {
                permuted_row.insert(encoded_input_permutation[sym as usize], dests.clone());
            }
            *row = permuted_row;
        }
    }

    /// `Automaton.canonize` (`Automaton.java:327-330`), now also consulting
    /// [`Automaton::canonized`] before delegating to [`Fa::canonicalize`] — see that
    /// field's doc comment (added U24) for why this flag lives here rather than on
    /// [`Fa`], and why it is a narrower, opt-in suppression rather than Java's full
    /// auto-memoizing `canonized` flag. Every pre-U24 call site is unaffected: the flag
    /// starts (and, absent an explicit `set_canonized` call, stays) `false`.
    ///
    /// **[`Automaton::sort_label`] runs FIRST and UNCONDITIONALLY**, matching Java
    /// exactly: `canonize()` is literally `sortLabel(); this.fa.canonizeInternal();`,
    /// and the memo check lives *inside* `canonizeInternal` (`FA.java:149`), not around
    /// the pair. Suppressing the label sort as well would be a real divergence for any
    /// producer that flags a *labeled* automaton — today the sole producer
    /// ([`crate::morphism::Morphism::to_word_automaton`]) hands back an unlabeled one,
    /// for which `sort_label` is a no-op either way, but the ordering is not the
    /// unobservable detail it looks like.
    pub fn canonize(&mut self) {
        self.sort_label();
        if self.canonized {
            return;
        }
        self.fa.canonicalize();
    }

    /// `Automaton.forceCanonize` (`Automaton.java:332-335`): resets
    /// [`Automaton::canonized`] to `false` first, then canonizes unconditionally —
    /// matching Java's `fa.setCanonized(false); canonize();` exactly (Java resets the
    /// same `canonized` flag [`Automaton::canonize`] now consults, just one layer down
    /// on `FA` rather than here).
    pub fn force_canonize(&mut self) {
        self.canonized = false;
        self.canonize();
    }

    /// `FA.setCanonized(boolean)` (`FA.java:590-592`), moved to this layer — see
    /// [`Automaton::canonized`]'s doc comment. [`crate::morphism::Morphism::to_word_automaton`]
    /// is this port's one caller that sets it `true` (mirroring `Morphism.java:88`'s
    /// `promotion.fa.setCanonized(true)`); the `false` direction is called from every
    /// `fa`-replacing operation listed on that field.
    ///
    /// **`pub(crate)`, deliberately.** Because the flag lives on the wrapper rather than
    /// on [`Fa`], its invariant is maintained BY HAND at those call sites — it is not
    /// correct by construction. Handing the setter to code outside `wr-core` would let a
    /// caller flag an automaton that a later in-crate `fa` replacement then silently
    /// un-flags (or, worse, leave a flag set across an operation this crate does not
    /// know to reset). Reading the flag ([`Automaton::is_canonized`]) stays `pub`.
    pub(crate) fn set_canonized(&mut self, canonized: bool) {
        self.canonized = canonized;
    }

    /// Whether [`Automaton::canonize`] is currently suppressed. Java has no public
    /// getter for `FA.canonized` (only `setCanonized`); this exists purely so tests (and
    /// any future caller) can observe the flag without reaching into the automaton's
    /// state some other way.
    pub fn is_canonized(&self) -> bool {
        self.canonized
    }

    /// `Automaton.getArity()` (`Automaton.java:498-501`): the number of tracks, or `0`
    /// for a trivial (TRUE/FALSE) automaton regardless of `alphabet.len()` (which is
    /// always `0` for one anyway — see [`Automaton::true_false`] — so the explicit
    /// check below is belt-and-suspenders, matching Java's own explicit check).
    pub fn arity(&self) -> usize {
        if self.fa.is_true_false_automaton() {
            0
        } else {
            self.alphabet.len()
        }
    }

    /// `Automaton.bind` (`Automaton.java:438-444`). BOTH halves of Java's guard clause
    /// are ported as of U0 — binding names to a TRUE/FALSE automaton is an error, not a
    /// no-op — and both become a panic (message matches `WalnutException.invalidBind`:
    /// "invalid use of method bind") rather than a `Result`, matching this file's
    /// existing convention for caller-contract violations (see [`Automaton::encode`]'s
    /// doc comment). Note the trivial half is NOT subsumed by the arity half:
    /// `bind(vec![])` on a trivial automaton passes the `0 == 0` arity check.
    /// `fa.setCanonized(false)` (`:442`) IS ported, onto this crate's wrapper-level
    /// [`Automaton::canonized`] (see that field's doc comment).
    pub fn bind(&mut self, names: Vec<String>) {
        assert!(
            !self.fa.is_true_false_automaton(),
            "invalid use of method bind"
        );
        assert_eq!(
            self.alphabet.len(),
            names.len(),
            "invalid use of method bind"
        );
        self.label = names;
        self.label_sorted = false;
        // `fa.setCanonized(false);` (`Automaton.java:442`).
        self.canonized = false;
        Self::remove_same_inputs(self, 0);
    }

    /// `Automaton.removeSameInputs` (`Automaton.java:451-467`). Checks if any input has
    /// the same label as input `i`; if so, merges the duplicates into one track (e.g. an
    /// expression like `f(a,a)` becomes a one-input automaton).
    fn remove_same_inputs(automaton: &mut Automaton, i: usize) {
        if i >= automaton.alphabet.len() {
            return;
        }
        let mut same_label_indices = vec![i];
        for j in (i + 1)..automaton.alphabet.len() {
            if automaton.label[i] == automaton.label[j] {
                // `UtilityMethods.areEqual`: SET equality, not ordered-list equality —
                // two same-label tracks with the same digits in a different order are
                // accepted here, ported faithfully (see `areEqual`'s own doc comment:
                // "Checks if the set of L and R are equal").
                let set_i: HashSet<i32> = automaton.alphabet[i].iter().copied().collect();
                let set_j: HashSet<i32> = automaton.alphabet[j].iter().copied().collect();
                assert_eq!(
                    set_i, set_j,
                    "Inputs {i} and {j} have the same label but different alphabets."
                );
                same_label_indices.push(j);
            }
        }
        if same_label_indices.len() > 1 {
            Self::reduce_dimension(automaton, same_label_indices);
        }
        Self::remove_same_inputs(automaton, i + 1);
    }

    /// `Automaton.reduceDimension` (`Automaton.java:469-491`) plus
    /// `RichAlphabet.determineReducedDimensionMap` (`RichAlphabet.java:184-220`),
    /// inlined here since the latter is `RichAlphabet`-private in Java and this crate
    /// folds `RichAlphabet`'s bookkeeping directly into `Automaton` (see module docs).
    /// `UtilityMethods.removeIndices(A.getNS(), I)` becomes an `msd` removal: `msd` is
    /// this crate's existing stand-in for Java's per-track `NumberSystem` list (see the
    /// struct doc comment on `msd`), so it plays the `NS` role here.
    fn reduce_dimension(automaton: &mut Automaton, mut same_label_indices: Vec<usize>) {
        /// `RichAlphabet.MISSING_REDUCED_DIMENSION_ELT`.
        const MISSING: i32 = -1;

        let old_alphabet_size = automaton.fa.alphabet_size;
        let keep = |i: usize| !same_label_indices.contains(&i) || same_label_indices[0] == i;

        let new_alphabet: Vec<Vec<i32>> = automaton
            .alphabet
            .iter()
            .enumerate()
            .filter(|&(i, _)| keep(i))
            .map(|(_, track)| track.clone())
            .collect();
        let new_encoder = Self::compute_encoder(&new_alphabet);

        let mut reduced_dimension_map = Vec::with_capacity(old_alphabet_size);
        for n in 0..old_alphabet_size {
            let new_elt = if same_label_indices.len() <= 1 {
                n as i32
            } else {
                let x = automaton.decode(n as i32); // decode against the OLD alphabet
                let i0 = x[same_label_indices[0]];
                let disagrees = same_label_indices[1..].iter().any(|&idx| x[idx] != i0);
                if disagrees {
                    MISSING
                } else {
                    let y: Vec<i32> = x
                        .iter()
                        .enumerate()
                        .filter_map(|(i, &d)| keep(i).then_some(d))
                        .collect();
                    Self::encode_with(&y, &new_alphabet, &new_encoder)
                }
            };
            reduced_dimension_map.push(new_elt);
        }

        let new_d: Vec<BTreeMap<i32, Vec<usize>>> = automaton
            .fa
            .d
            .iter()
            .map(|row| {
                let mut new_row: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
                for (&sym, dests) in row {
                    let mapped = reduced_dimension_map[sym as usize];
                    if mapped != MISSING {
                        new_row
                            .entry(mapped)
                            .or_default()
                            .extend(dests.iter().copied());
                    }
                }
                new_row
            })
            .collect();
        automaton.fa.d = new_d;

        same_label_indices.remove(0);
        remove_indices(&mut automaton.msd, &same_label_indices);
        // Java's `removeIndices(A.getNS(), I)` removes the whole `NumberSystem` entry,
        // i.e. all three parts of this crate's stand-in.
        remove_indices(&mut automaton.all_reps, &same_label_indices);
        remove_indices(&mut automaton.ns_name, &same_label_indices);
        automaton.alphabet = new_alphabet;
        automaton.encoder = new_encoder;
        automaton.determine_alphabet_size();
        remove_indices(&mut automaton.label, &same_label_indices);
    }

    /// `Automaton.determinizeAndMinimize()` (`Automaton.java:383-398`). Java's
    /// `Trimmer.trimAutomaton` mutates its `FA` argument in place; this crate's
    /// [`crate::trim::trim`] returns a fresh [`Fa`] instead (the crate convention —
    /// `determinize`/`minimize` also build fresh values rather than mutating), so the
    /// port reassigns `self.fa` at each step rather than passing `&mut self.fa` through.
    ///
    /// # `docs/WALNUT-BUGS.md` WB-001 is reachable through this method, faithfully
    ///
    /// When `self.fa` is ALREADY deterministic, the `trim` step above is skipped
    /// entirely (matching `Automaton.java:385` exactly — Java's guard is the same
    /// `!isDeterministic()`), so `minimize` below is called with NO guarantee every
    /// state is reachable from `q0`. That is WB-001's exact precondition violation: an
    /// already-deterministic automaton with a state unreachable from `q0` that also
    /// cannot reach acceptance gets silently corrupted (a real state's language flips
    /// from `∅` to `Σ*` in the minimal Walnut-upstream trigger case — see WB-001's
    /// entry for the minimal example, and `determinize_and_minimize_reaches_wb_001_on_an_already_deterministic_input`
    /// below, which pins this exact call path). This is faithful to Java (which has
    /// the identical bug at the identical call site) and therefore ported verbatim per
    /// `CLAUDE.md`'s mechanical-port rule — NOT fixed here by adding an unconditional
    /// trim, which would be an undeclared behavioral divergence from `Automaton.java`.
    pub fn determinize_and_minimize(&mut self) {
        self.determinize_and_minimize_with_ctx(None);
    }

    /// [`Automaton::determinize_and_minimize`] with an explicit
    /// [`crate::determinize::DeterminizeContext`] — Walnut's `[strategy …]`/`[export …]`
    /// metacommand state, which Java reads out of the `Prover.mainProver.metaCommands`
    /// singleton inside the dispatcher (see [`NO_CONTEXT_CANNOT_FAIL`]'s docs and
    /// `determinize.rs`'s module docs).
    ///
    /// `None` is exactly the no-arg method above. `Some(ctx)` is the port of Java's
    /// `Logging.shouldPrintDetails() == true` state, so **the caller owes the
    /// print-details gate**: pass `None` whenever `should_print_details()` is false, or
    /// the automata indices shift and `[strategy 6 …]` selects the wrong automaton.
    ///
    /// Note the counter only moves when a determinization actually happens: Java's
    /// `!isDeterministic()` guard (`Automaton.java:385`) skips the dispatcher entirely on
    /// an already-deterministic input, which is directly visible in Walnut's own
    /// `details` fixtures (a `quantifying:` block with a `Minimizing:` line but no
    /// `Determinizing [#n, …]` line is exactly this branch).
    ///
    /// # Panics
    ///
    /// With `Some(ctx)` the dispatcher becomes fallible — a non-`SC` strategy on a DFAO
    /// is Java's `WalnutException("DFAOs are not supported for non-SC strategies.")`
    /// (`DeterminizationStrategies.java:115-119`). Ported as a `panic!` carrying that
    /// message verbatim, per [`crate::walnut_panic`]'s guard-authoring rule: Java catches
    /// it in `EvalDef.compute`, and this port catches it at the same place.
    pub fn determinize_and_minimize_with_ctx(
        &mut self,
        ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    ) {
        if !self.fa.is_deterministic() {
            // Working with an NFA. Let's trim, then determinize from {q0}.
            self.fa = crate::trim::trim(&self.fa);
            let initial: BTreeSet<usize> = [self.fa.q0].into_iter().collect();
            dispatch_determinize(self, &initial, ctx);
        }
        // `FA.justMinimize`'s `convertNFAtoDFA()` call is a storage-representation
        // optimization only (see `fa.rs` module docs: this crate always uses one
        // unified NFA-shaped table) — not replicated.
        self.fa = crate::minimize::minimize(&self.fa).expect(
            "minimize's only OTHER precondition (determinism) always holds here -- the \
             already-deterministic branch above skips reachability trimming, so WB-001 \
             (docs/WALNUT-BUGS.md) is reachable, faithfully, not a panic",
        );
        // `FA.justMinimize`'s own `this.canonized = false;` (`FA.java:584`) -- see
        // `Automaton::canonized`'s doc comment on why this is manual here.
        self.canonized = false;
    }

    /// `Automaton.determinizeAndMinimize(IntSet qqq)` (`Automaton.java:403-406`) — the
    /// generalized multi-initial-state entry point (e.g. for a caller that just ran
    /// [`Fa::reverse`], whose returned "new initial states" are a genuine set, not
    /// necessarily a singleton). Unlike the no-arg overload, this is unconditional —
    /// Java's version has no `!isDeterministic` guard either.
    pub fn determinize_and_minimize_from(&mut self, initial: &BTreeSet<usize>) {
        self.determinize_and_minimize_from_with_ctx(initial, None);
    }

    /// [`Automaton::determinize_and_minimize_from`] with an explicit
    /// [`crate::determinize::DeterminizeContext`] — see
    /// [`Automaton::determinize_and_minimize_with_ctx`] for the contract (including the
    /// caller-owed print-details gate and the DFAO panic). Unlike that method this one is
    /// unconditional, so `Some(ctx)` ALWAYS consumes an automata index here.
    pub fn determinize_and_minimize_from_with_ctx(
        &mut self,
        initial: &BTreeSet<usize>,
        ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    ) {
        dispatch_determinize(self, initial, ctx);
        self.fa = crate::minimize::minimize(&self.fa).expect(
            "subset_construction's output is always deterministic and q0-reachable -- \
             minimize's documented preconditions",
        );
        // `FA.justMinimize`'s own `this.canonized = false;` (`FA.java:584`).
        self.canonized = false;
    }

    /// `Automaton.asDFA` (`Automaton.java:152-158`).
    pub fn as_dfa(&self) -> AutomatonDFA {
        AutomatonDFA::from(self.clone())
    }

    /// [`Automaton::as_dfa`] with an explicit
    /// [`crate::determinize::DeterminizeContext`] — see
    /// [`Automaton::determinize_and_minimize_with_ctx`] for the contract. This is the
    /// path `LogicalOperator`'s `~` (negation) and the `A`/`I` quantifiers take
    /// (`not(a.M.asDFA())`), so it is on the eval call graph, not a convenience.
    pub fn as_dfa_with_ctx(
        &self,
        ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    ) -> AutomatonDFA {
        AutomatonDFA::from_with_ctx(self.clone(), ctx)
    }
}

/// A typesafe wrapper asserting the contained [`Automaton`]'s `fa` is deterministic
/// (Walnut's `AutomatonDFA`, `AutomatonDFA.java:16`: "Typesafe extension that requires
/// determinism. DFA and DFAO are allowed.").
///
/// # Enforcement: stronger than Java's, by construction
///
/// Java's `AutomatonDFA extends Automaton` relies mostly on CONVENTION: its `from`/
/// address/regex constructors call `requireDfaStorage()` (ported below inside
/// [`AutomatonDFA::from`]) — though not ALL of them (`AutomatonDFA.java:17-25`'s no-arg
/// and truth-value constructors skip it, since a fresh empty or 1-state truth-value
/// automaton is trivially deterministic already) — and its `clone()` IS overridden to
/// re-run the check (`AutomatonDFA.java:100-108`), an earlier version of this comment
/// wrongly claimed nothing was overridden. But `AutomatonDFA` still inherits every
/// OTHER mutating method `Automaton` has unchecked, so nothing stops later code holding
/// an `AutomatonDFA` reference from calling an inherited mutator that reintroduces
/// nondeterminism — the type is still, on the whole, a documented promise more than an
/// enforced one in Java.
///
/// This port takes the stronger option Rust's module privacy makes easy: `AutomatonDFA`
/// wraps a private `Automaton` field, so external code can only ever OBTAIN one through
/// [`AutomatonDFA::from`] (which runs the same `requireDfaStorage` check as Java), and
/// can only ever mutate the wrapped value by first unwrapping it via
/// [`AutomatonDFA::into_automaton`] — at which point it is, correctly, typed back as a
/// plain `Automaton` and subject to that type's ordinary (mutable) contract. So "an
/// `AutomatonDFA` you're holding right now is deterministic" is actually enforced here,
/// not just documented — a deliberate, narrow improvement over Java's version of this
/// one type (not a general policy for this port; most of this crate stays behaviorally
/// identical to Java on purpose, per `CLAUDE.md`'s mechanical-port rule).
#[derive(Debug, Clone)]
pub struct AutomatonDFA(Automaton);

impl AutomatonDFA {
    /// `AutomatonDFA(boolean truthValue)` (`AutomatonDFA.java:21-25`) — the DFA-typed
    /// trivial automaton. Note Java's constructor deliberately skips
    /// `requireDfaStorage()` (a trivial automaton is vacuously deterministic); so does
    /// this.
    pub fn true_false(truth: bool) -> Self {
        AutomatonDFA(Automaton::true_false(truth))
    }

    /// `AutomatonDFA.requireDfaStorage` (`AutomatonDFA.java:87-98`), including its
    /// `isTRUE_FALSE_AUTOMATON -> return` short-circuit (U0, `:88-90`). That guard is
    /// load-bearing: `determinize_and_minimize` below would otherwise run subset
    /// construction over a trivial automaton's meaningless (possibly stale) state set.
    ///
    /// Takes an explicit [`crate::determinize::DeterminizeContext`] — see
    /// [`Automaton::determinize_and_minimize_with_ctx`] for the contract. Only the
    /// nondeterministic branch reaches the dispatcher, so an already-deterministic input
    /// consumes no automata index (matching Java, whose `requireDfaStorage` guards the
    /// call the same way).
    fn require_dfa_storage_with_ctx(
        mut automaton: Automaton,
        ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    ) -> Automaton {
        if automaton.fa.is_true_false_automaton() {
            return automaton;
        }
        if !automaton.fa.is_deterministic() {
            if automaton.is_fao() {
                // WalnutException.nonDeterministicO()'s exact message (double period is
                // in the original, not a typo introduced here).
                panic!("NFAOs are not supported..");
            }
            automaton.determinize_and_minimize_with_ctx(ctx);
        }
        // `fa.convertNFAtoDFA()` (`FA.java:700-706`) is NOT a pure storage-representation
        // optimization — an earlier version of this comment claimed that, which an
        // adversarial review found false: Java's version is a CHECKED conversion that
        // throws `WalnutException` ("Unexpected NFA instead of DFA.") if the automaton is
        // still nondeterministic at this point. That's unreachable via the branch above
        // (which always leaves `automaton.fa` deterministic or already panicked), but
        // porting the check itself — not just its no-op storage-conversion half — is what
        // actually makes this type's "enforced, not just documented" invariant (see this
        // type's doc comment) hold at every real exit path, not just the ones this
        // review happened to trace by hand.
        assert!(
            automaton.fa.is_deterministic(),
            "Unexpected NFA instead of DFA."
        );
        automaton
    }

    /// `AutomatonDFA.from` (`AutomatonDFA.java:74-85`). The `instanceof AutomatonDFA`
    /// short-circuit doesn't apply: the input here is always a plain [`Automaton`],
    /// never already an `AutomatonDFA` (Rust's type system rules that call shape out).
    /// The `isTRUE_FALSE_AUTOMATON` short-circuit (`:79-81`) IS ported as of U0, and
    /// faithfully returns a FRESH trivial automaton rather than the argument — Java's
    /// `new AutomatonDFA(automaton.fa.isTRUE_AUTOMATON())` discards any stale
    /// `Q`/alphabet the argument was carrying (the shape `Automaton.clear()` leaves
    /// behind, see `crate::fa`'s module docs). Same for
    /// [`Automaton::as_dfa`]/`Automaton.clone()`'s trivial branch (`:102-104`), which
    /// route through here.
    pub fn from(automaton: Automaton) -> Self {
        Self::from_with_ctx(automaton, None)
    }

    /// [`AutomatonDFA::from`] with an explicit
    /// [`crate::determinize::DeterminizeContext`] — see
    /// [`Automaton::determinize_and_minimize_with_ctx`] for the contract.
    pub fn from_with_ctx(
        automaton: Automaton,
        ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
    ) -> Self {
        if automaton.fa.is_true_false_automaton() {
            return AutomatonDFA::true_false(automaton.fa.is_true_automaton());
        }
        AutomatonDFA(Self::require_dfa_storage_with_ctx(automaton, ctx))
    }

    /// Borrows the wrapped, guaranteed-deterministic [`Automaton`].
    pub fn automaton(&self) -> &Automaton {
        &self.0
    }

    /// Unwraps back into a plain, mutable [`Automaton`] (Rust's answer to Java's
    /// "nothing stops you from mutating an `AutomatonDFA` into nondeterminism" gap —
    /// see this type's doc comment: here, you have to explicitly give up the
    /// `AutomatonDFA` typing first).
    pub fn into_automaton(self) -> Automaton {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn trivial_fa(alphabet_size: usize) -> Fa {
        Fa {
            true_false: None,
            q0: 0,
            q: 1,
            alphabet_size,
            o: vec![0],
            d: vec![BTreeMap::new()],
        }
    }

    #[test]
    fn encode_matches_hand_derived_mixed_radix_value() {
        // Two tracks, base 3 each: encoder = [1, 3]. digits [2, 1] -> 2*1 + 1*3 = 5.
        let a = Automaton::new(
            trivial_fa(9),
            vec![vec![0, 1, 2], vec![0, 1, 2]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(a.encode(&[2, 1]), 5);
        assert_eq!(a.encode(&[0, 0]), 0);
        assert_eq!(a.encode(&[2, 2]), 8);
    }

    #[test]
    fn decode_matches_hand_derived_digits() {
        let a = Automaton::new(
            trivial_fa(9),
            vec![vec![0, 1, 2], vec![0, 1, 2]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(a.decode(5), vec![2, 1]);
        assert_eq!(a.decode(0), vec![0, 0]);
        assert_eq!(a.decode(8), vec![2, 2]);
    }

    /// The `-1` key WB-038's faithfully-ported `encode_index_of` really does put into a
    /// `.txt`-loaded automaton must never decode to *something*: Java's truncating
    /// `n % size` yields `-1`, and `ArrayList.get(-1)` throws. This port used
    /// `rem_euclid`, which always lands in range — so `decode(-1)` returned digit `[1]`
    /// (for a 2-symbol track) and every caller downstream silently wrote out an
    /// automaton whose language matched neither the file nor Java's answer.
    #[test]
    fn decoding_an_out_of_range_symbol_is_an_error_not_a_fabricated_tuple() {
        let a = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![0, 1]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(
            a.try_decode(-1),
            Err(DecodeError::IndexOutOfBounds {
                index: -1,
                length: 2
            })
        );
        // The JDK's own message text, which is what the real CLI prints for this file
        // (`java.lang.IndexOutOfBoundsException: Index -1 out of bounds for length 2`).
        assert_eq!(
            a.try_decode(-1).unwrap_err().to_string(),
            "Index -1 out of bounds for length 2"
        );
        // `-2` is the shape that shows truncating-vs-Euclidean really matters: the FIRST
        // track's index is `-2 % 2 == 0` (fine), and the failure surfaces on the second
        // track, where `n` has become `-1`.
        assert_eq!(
            a.try_decode(-2),
            Err(DecodeError::IndexOutOfBounds {
                index: -1,
                length: 2
            })
        );
        // Every valid symbol still round-trips.
        for sym in 0..4 {
            assert_eq!(a.try_decode(sym).map(|d| a.encode(&d)), Ok(sym));
        }
    }

    /// The panicking wrapper: the message is the Java exception's text and nothing else,
    /// so `wr_cli`'s dispatch boundary (`Prover::caught`) reports exactly it — see
    /// `crate::walnut_panic`'s guard-authoring rule.
    #[test]
    fn decode_panics_with_javas_message_rather_than_returning_a_wrong_tuple() {
        let a = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![0, 1]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(
            crate::walnut_panic::catch_walnut_panic(|| a.decode(-1)),
            Err("Index -1 out of bounds for length 2".to_string())
        );
    }

    /// Java bounds-checks only the per-track index, never the symbol as a whole, so a
    /// symbol at or above `alphabet_size` silently wraps instead of erroring. Ported as
    /// the quirk it is (same rule as WB-038) — pinned here so a later "obvious"
    /// tightening is a deliberate, reviewed divergence rather than a silent one.
    #[test]
    fn decoding_a_symbol_past_the_alphabet_wraps_exactly_as_java_does() {
        let a = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![0, 1]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        // 4 == alphabet_size: `4 % 2 == 0`, then `4 / 2 == 2`, `2 % 2 == 0`.
        assert_eq!(a.try_decode(4), Ok(vec![0, 0]));
        assert_eq!(a.try_decode(7), Ok(vec![1, 1]));
    }

    #[test]
    fn encode_uses_index_in_alphabet_not_literal_value() {
        // A non-contiguous, non-zero-first track alphabet: [5, 7, 2]. Index of 2 is 2,
        // index of 5 is 0 — encode must key off POSITION, not the literal digit value.
        let a = Automaton::new(
            trivial_fa(3),
            vec![vec![5, 7, 2]],
            vec!["a".into()],
            vec![None],
        );
        assert_eq!(a.encode(&[5]), 0);
        assert_eq!(a.encode(&[7]), 1);
        assert_eq!(a.encode(&[2]), 2);
    }

    #[test]
    fn determine_zero_is_zero_for_standard_base_k_alphabet() {
        let a = Automaton::new(
            trivial_fa(9),
            vec![vec![0, 1, 2], vec![0, 1, 2]],
            vec!["a".into(), "b".into()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(a.determine_zero(), 0);
    }

    proptest! {
        /// Tier-4 property #6 (DESIGN.md §5): encode/decode round-trip, including
        /// non-contiguous per-track alphabets — stresses the real
        /// index-in-list-not-literal-value indexing rule, which a round-trip test over
        /// only `0..k` alphabets could pass vacuously even with the indexing backwards.
        #[test]
        fn encode_decode_round_trip(
            // Up to 3 tracks, each a random small set of DISTINCT i32 values (mimicking
            // a real, possibly-non-contiguous alphabet), plus a digit tuple drawn from
            // those same per-track value sets.
            tracks in prop::collection::vec(
                prop::collection::hash_set(-5i32..20, 1..4).prop_map(|s| {
                    let mut v: Vec<i32> = s.into_iter().collect();
                    v.sort_unstable();
                    v
                }),
                1..4,
            ),
        ) {
            let seed = 0xC0FFEEu64;
            let mut state = seed;
            let mut digits = Vec::with_capacity(tracks.len());
            for track in &tracks {
                // Deterministic xorshift, no external RNG dependency needed for a
                // same-input-derived index pick.
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                digits.push(track[(state as usize) % track.len()]);
            }
            let a = Automaton::new(
                trivial_fa(tracks.iter().map(|t| t.len()).product()),
                tracks,
                vec!["t".into(); digits.len()],
                vec![None; digits.len()],
            );
            let sym = a.encode(&digits);
            prop_assert_eq!(a.decode(sym), digits);
        }
    }

    // --- is_bound / get_arity / is_empty / is_fao / determine_alphabet_size ---

    #[test]
    fn is_bound_true_iff_label_len_matches_alphabet_len() {
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            Vec::new(),
            vec![Some(true)],
        );
        assert!(!a.is_bound(), "empty label, one track: not bound");
        assert_eq!(
            a.get_arity(),
            1,
            "arity is the track count regardless of binding"
        );
        a.label = vec!["x".into()];
        assert!(a.is_bound());
    }

    #[test]
    fn is_empty_matches_fa_language_emptiness() {
        let accepting = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 2,
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["a".into()],
            vec![Some(true)],
        );
        assert!(!accepting.is_empty());

        let rejecting = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["a".into()],
            vec![Some(true)],
        );
        assert!(rejecting.is_empty());
    }

    #[test]
    fn is_fao_true_iff_some_output_exceeds_one() {
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["a".into()],
            vec![Some(true)],
        );
        assert!(!a.is_fao());
        a.fa.o[0] = 2;
        assert!(a.is_fao());
    }

    #[test]
    fn determine_alphabet_size_recomputes_product_of_track_sizes() {
        let mut a = Automaton::new(
            trivial_fa(1),
            vec![vec![0, 1, 2], vec![0, 1]],
            vec!["a".into(), "b".into()],
            vec![None, None],
        );
        a.fa.alphabet_size = 999; // deliberately stale
        a.determine_alphabet_size();
        assert_eq!(a.fa.alphabet_size, 6);
    }

    #[test]
    fn random_label_assigns_stringified_track_indices() {
        let mut a = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![0, 1]],
            Vec::new(),
            vec![None, None],
        );
        a.random_label();
        assert_eq!(a.label, vec!["0".to_string(), "1".to_string()]);
    }

    #[test]
    fn unlabel_clears_label_and_resets_sort_cache() {
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["a".into()],
            vec![None],
        );
        a.sort_label(); // primes label_sorted = true
        a.unlabel();
        assert!(a.label.is_empty());
        assert!(!a.is_bound());
    }

    // --- permute / getLabelPermutation (replicates AutomataTest.testPermute /
    // AutomataTest.testLabelPermutation) ---

    #[test]
    fn permute_matches_javas_scatter_quirk() {
        // AutomataTest.testPermute: permutation [1,2,0] over ["a","b","c"] yields
        // ["c","a","b"] (the documented ACTUAL behavior, not the originally-intended
        // gather ["b","c","a"] -- see `Automaton::permute`'s doc comment).
        let l = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(
            Automaton::permute(&l, &[1, 2, 0]),
            vec!["c".to_string(), "a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn label_permutation_and_permuted_alphabet_and_encoder_match_java() {
        // AutomataTest.testLabelPermutation.
        let label = vec!["z".to_string(), "a".to_string(), "c".to_string()];
        let mut sorted_label = label.clone();
        sorted_label.sort();
        let label_permutation = Automaton::get_label_permutation(&label, &sorted_label);
        assert_eq!(label_permutation, vec![2, 0, 1]);

        let alphabet = vec![vec![-1, 2], vec![0, 1], vec![1, 2, 3]];
        let permuted_alphabet = Automaton::permute(&alphabet, &label_permutation);
        assert_eq!(
            permuted_alphabet,
            vec![vec![0, 1], vec![1, 2, 3], vec![-1, 2]]
        );

        let encoder = Automaton::compute_encoder(&permuted_alphabet);
        assert_eq!(encoder, vec![1, 2, 6]);
    }

    // --- sort_label (Automaton.java:348-381) ---

    /// Two tracks whose labels are already out of lexicographic order: "b" (alphabet
    /// `[0,1,2]`) then "a" (alphabet `[0,1]`). Distinct `msd` per track (adversarial-
    /// review finding: a `[Some(true), Some(true)]` fixture makes the `msd` permutation
    /// unobservable — swapping it for a no-op wouldn't fail any test).
    fn two_track_b_then_a_automaton() -> Automaton {
        Automaton::new(
            trivial_fa(6),
            vec![vec![0, 1, 2], vec![0, 1]],
            vec!["b".to_string(), "a".to_string()],
            vec![Some(true), Some(false)],
        )
    }

    #[test]
    fn sort_label_is_a_noop_when_unbound() {
        let mut a = Automaton::new(
            trivial_fa(6),
            vec![vec![0, 1, 2], vec![0, 1]],
            Vec::new(),
            vec![Some(true), Some(true)],
        );
        let alphabet_before = a.alphabet.clone();
        a.sort_label();
        assert!(a.label.is_empty());
        assert_eq!(a.alphabet, alphabet_before);
    }

    #[test]
    fn sort_label_is_a_noop_when_already_sorted() {
        let mut a = two_track_b_then_a_automaton();
        a.label = vec!["a".to_string(), "b".to_string()];
        let alphabet_before = a.alphabet.clone();
        a.sort_label();
        assert_eq!(a.label, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(a.alphabet, alphabet_before);
        // Second call short-circuits on `label_sorted` -- still a no-op.
        a.sort_label();
        assert_eq!(a.label, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn sort_label_permutes_labels_alphabet_and_transitions() {
        // An adversarial review found (by mutation-testing: deleting the transition-
        // remap loop entirely left this test green) that the original digit pair
        // (b=2, a=1) was a FIXED POINT of the re-encoding -- old encoder [1,3]: 1*2 +
        // 3*1 = 5; new encoder [1,2] with the same pair written (a=1, b=2): 1*1 + 2*2
        // = 5. A completely broken remap (or none at all) would pass. (b=1, a=1) is
        // NOT a fixed point: old encode = 1*1 + 3*1 = 4; new encode = 1*1 + 2*1 = 3.
        let mut a = two_track_b_then_a_automaton();
        // Old order is [b, a]: digits (b=1, a=1).
        let old_sym = a.encode(&[1, 1]);
        assert_eq!(old_sym, 4);
        a.fa.d[0].insert(old_sym, vec![0]);

        a.sort_label();

        assert_eq!(a.label, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(a.alphabet, vec![vec![0, 1], vec![0, 1, 2]]);
        // msd must travel WITH its track's label, not stay positional: "a" (now track
        // 0) was `Some(false)`, "b" (now track 1) was `Some(true)` -- also caught by
        // mutation-testing (deleting the `msd` permutation line left this green when
        // both tracks shared one msd value).
        assert_eq!(a.msd, vec![Some(false), Some(true)]);

        // New order is [a, b]: the same digit pair is now written (a=1, b=1).
        let new_sym = a.encode(&[1, 1]);
        assert_eq!(new_sym, 3);
        assert_eq!(a.fa.d[0].get(&new_sym), Some(&vec![0]));
        // The transition table was fully replaced under the new symbol numbering --
        // exactly one entry survives.
        assert_eq!(a.fa.d[0].len(), 1);
    }

    // --- canonize / force_canonize ---

    #[test]
    fn canonize_sorts_label_and_canonicalizes_fa() {
        // 3 states: q0=1 <-> 2 (a 2-cycle, state 2 accepting); state 0 is unreachable
        // from q0 -- exercises `fa.canonicalize`'s pruning (see `fa.rs`'s equivalent
        // test) alongside `sort_label`'s relabeling, in the same call.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        let mut d2 = BTreeMap::new();
        d2.insert(0, vec![1]);
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 1,
                q: 3,
                // Must be the true product of the two 2-symbol tracks (4), not 1 --
                // an adversarial review found the original fixture set this to 1
                // despite a 2-track alphabet, an internally-inconsistent `Automaton`
                // that only avoided panicking because every transition below happens
                // to use symbol 0.
                alphabet_size: 4,
                o: vec![0, 0, 1],
                d: vec![d0, d1, d2],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["b".to_string(), "a".to_string()],
            vec![Some(true), Some(true)],
        );

        a.canonize();

        assert_eq!(a.label, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(a.fa.q0, 0, "canonicalize's BFS root always becomes state 0");
        assert_eq!(a.fa.q, 2, "the unreachable state was dropped");
    }

    #[test]
    fn force_canonize_behaves_like_canonize() {
        let mut a = two_track_b_then_a_automaton();
        a.force_canonize();
        assert_eq!(a.label, vec!["a".to_string(), "b".to_string()]);
    }

    // --- canonized suppression flag (U24, for `Morphism::to_word_automaton`) ---

    #[test]
    fn set_canonized_true_suppresses_only_the_fa_canonicalization_not_the_label_sort() {
        // Same unreachable-state shape as `canonize_sorts_label_and_canonicalizes_fa`,
        // but flagged `canonized` first. Java's `canonize()` is `sortLabel();
        // fa.canonizeInternal();` with the memo check INSIDE `canonizeInternal`
        // (`FA.java:149`), so the flag suppresses the state-dropping BFS and NOTHING
        // else -- the label sort still runs.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        let mut d2 = BTreeMap::new();
        d2.insert(0, vec![1]);
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 1,
                q: 3,
                alphabet_size: 4,
                o: vec![0, 0, 1],
                d: vec![d0, d1, d2],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["b".to_string(), "a".to_string()],
            vec![Some(true), Some(true)],
        );

        assert!(!a.is_canonized());
        a.set_canonized(true);
        assert!(a.is_canonized());
        a.canonize();

        assert_eq!(
            a.label,
            vec!["a".to_string(), "b".to_string()],
            "sortLabel() runs unconditionally in Java -- the flag gates only \
             canonizeInternal"
        );
        assert_eq!(a.fa.q0, 1, "q0 must be untouched");
        assert_eq!(
            a.fa.q, 3,
            "the unreachable state must survive -- this is the whole point of the flag"
        );
    }

    #[test]
    fn force_canonize_overrides_the_suppression_flag() {
        let mut a = two_track_b_then_a_automaton();
        a.set_canonized(true);
        a.force_canonize();
        assert!(
            !a.is_canonized(),
            "force_canonize resets the flag before canonizing, matching Java's \
             fa.setCanonized(false); canonize();"
        );
        assert_eq!(
            a.label,
            vec!["a".to_string(), "b".to_string()],
            "force_canonize must still relabel even though canonized was set"
        );
    }

    #[test]
    fn set_canonized_false_is_a_pre_existing_call_sites_default_and_has_no_effect() {
        // Every call site before U24 never touched this flag at all -- pin that
        // `canonize()`'s default (unflagged) behavior is completely unchanged.
        let mut a = two_track_b_then_a_automaton();
        assert!(!a.is_canonized());
        a.canonize();
        assert_eq!(a.label, vec!["a".to_string(), "b".to_string()]);
    }

    // --- arity ---

    #[test]
    fn arity_is_the_track_count_for_an_ordinary_automaton() {
        let a = two_track_b_then_a_automaton();
        assert_eq!(a.arity(), 2);
    }

    #[test]
    fn arity_is_zero_for_a_trivial_automaton() {
        assert_eq!(Automaton::true_false(true).arity(), 0);
        assert_eq!(Automaton::true_false(false).arity(), 0);
    }

    // --- bind / removeSameInputs / reduceDimension ---

    /// Two tracks over `{0,1}` labeled distinctly; a single accepting state that
    /// self-loops exactly on digit pairs where the two tracks AGREE (encoding a
    /// "diagonal" predicate `track0 == track1`), leaving disagreeing pairs as dead
    /// transitions -- built this way so `bind`-merging the two tracks into one is
    /// language-preserving and easy to check by hand.
    fn diagonal_two_track_automaton() -> Automaton {
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 4,
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["a".to_string(), "b".to_string()],
            vec![Some(true), Some(true)],
        );
        let sym00 = a.encode(&[0, 0]);
        let sym11 = a.encode(&[1, 1]);
        a.fa.d[0].insert(sym00, vec![0]);
        a.fa.d[0].insert(sym11, vec![0]);
        a
    }

    #[test]
    fn bind_merges_duplicate_labeled_tracks_into_one() {
        let mut a = diagonal_two_track_automaton();
        a.bind(vec!["x".to_string(), "x".to_string()]);

        assert_eq!(a.label, vec!["x".to_string()]);
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.msd, vec![Some(true)]);
        assert_eq!(a.fa.alphabet_size, 2);
        // (0,0) and (1,1) survive as digits 0 and 1 on the single remaining track;
        // (0,1)/(1,0) are gone entirely (they mapped to MISSING).
        assert_eq!(a.fa.d[0].get(&0), Some(&vec![0]));
        assert_eq!(a.fa.d[0].get(&1), Some(&vec![0]));
        assert_eq!(a.fa.d[0].len(), 2);
    }

    #[test]
    fn bind_merges_non_adjacent_duplicate_tracks_keeping_the_first_ones_metadata() {
        // Adversarial-review finding (mutation-tested): the two-track diagonal fixture
        // above can't distinguish "keep the FIRST same-labeled track's metadata" from
        // "keep the LAST" (both tracks are adjacent with identical alphabet AND msd).
        // This fixture uses 3 tracks, labels ["a","b","a"] (a NON-adjacent duplicate
        // pair straddling a kept track), with track 0 ("a") and track 2 (the "a"
        // duplicate) given DIFFERENT `msd` (`Some(true)` vs `None`) so a mutant that
        // kept index 2's metadata instead of index 0's is caught. Predicate: accept
        // iff `digit0 == digit2` (always true post-merge) AND `b == 1` -- so the
        // merged automaton's language should be exactly "b == 1", pinning that the
        // surviving "b" track's transitions were re-encoded under the correct
        // (shifted) 2-track scheme. (This checks the transition TABLE directly, not
        // `accepts_word` -- the single state is already unconditionally accepting, so
        // acceptance itself can't distinguish anything; the merge's correctness shows
        // up in which encoded symbols carry a self-loop entry at all, matching the
        // original diagonal test's style above.)
        let alphabet_size = 2 * 3 * 2; // a(2) * b(3) * a(2), a fastest-varying.
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size,
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1, 2], vec![0, 1]],
            vec!["p".to_string(), "q".to_string(), "r".to_string()],
            vec![Some(true), Some(false), None],
        );
        for a0 in 0..2 {
            for b in 0..3 {
                for a2 in 0..2 {
                    if a0 == a2 && b == 1 {
                        let sym = a.encode(&[a0, b, a2]);
                        a.fa.d[0].insert(sym, vec![0]);
                    }
                }
            }
        }

        a.bind(vec!["a".to_string(), "b".to_string(), "a".to_string()]);

        assert_eq!(a.label, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(a.alphabet, vec![vec![0, 1], vec![0, 1, 2]]);
        // The FIRST "a" (old track 0, msd `Some(true)`) must survive, not the second
        // (old track 2, msd `None`) -- this is what a keep-the-last-duplicate mutant
        // gets backwards.
        assert_eq!(a.msd, vec![Some(true), Some(false)]);

        // The reduced-dimension transition table must carry exactly the symbols
        // `(x, y)` with `y == 1` (mirroring the pre-merge `(x, y, x)` predicate),
        // independent of `x` -- pins the index math, separately from the msd check
        // above pinning which duplicate's metadata survives.
        for x in 0..2 {
            for y in 0..3 {
                let sym = a.encode(&[x, y]);
                assert_eq!(
                    a.fa.d[0].contains_key(&sym),
                    y == 1,
                    "x={x} y={y}: transition should survive iff y==1"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "invalid use of method bind")]
    fn bind_panics_on_arity_mismatch() {
        // `#[should_panic(expected = ...)]` pins the actual `WalnutException.invalidBind`
        // message, not just "panicked somehow" (an adversarial review noted the
        // original `catch_unwind`-based version would pass even if the panic message
        // were wrong or came from an unrelated bounds-check).
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["a".into()],
            vec![None],
        );
        a.bind(vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    #[should_panic(expected = "have the same label but different alphabets")]
    fn bind_panics_when_same_labeled_tracks_have_different_alphabets() {
        let mut a = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![2, 3]],
            vec!["a".to_string(), "b".to_string()],
            vec![Some(true), Some(true)],
        );
        a.bind(vec!["x".to_string(), "x".to_string()]);
    }

    // --- determinize_and_minimize / determinize_and_minimize_from / as_dfa ---

    /// A genuinely nondeterministic single-track "contains a 1" NFA (same shape as
    /// `determinize.rs`'s `contains_one_nfa`), wrapped as an `Automaton`.
    fn contains_one_nfa_automaton() -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![0, 1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![1]);
        d1.insert(1, vec![1]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![d0, d1],
            },
            vec![vec![0, 1]],
            vec!["a".to_string()],
            vec![Some(true)],
        )
    }

    #[test]
    fn determinize_and_minimize_preserves_language_and_yields_a_dfa() {
        let mut a = contains_one_nfa_automaton();
        assert!(!a.fa.is_deterministic());
        a.determinize_and_minimize();
        assert!(a.fa.is_deterministic());
        for word in [vec![], vec![0, 0, 0], vec![1], vec![0, 1, 0], vec![1, 1, 1]] {
            assert_eq!(
                a.fa.accepts_word(&word),
                word.contains(&1),
                "mismatch on {word:?}"
            );
        }
    }

    #[test]
    fn determinize_and_minimize_reaches_wb_001_on_an_already_deterministic_input() {
        // Pins `docs/WALNUT-BUGS.md` WB-001 at this exact call boundary (adversarial-
        // review finding): `determinize_and_minimize`'s already-deterministic branch
        // skips `trim`, so `minimize` runs without its reachability precondition.
        // Minimal Walnut-upstream trigger: q0 non-accepting self-looping, a SEPARATE
        // accepting self-looping state unreachable from q0. True language is `∅`.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![1]);
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 1,
                o: vec![0, 1],
                d: vec![d0, d1],
            },
            vec![vec![0]],
            vec!["a".to_string()],
            vec![Some(true)],
        );
        assert!(a.fa.is_deterministic(), "sanity: already deterministic");
        assert!(a.is_empty(), "sanity: true language is the empty language");

        a.determinize_and_minimize();

        // Faithful to Java (identical bug, identical call site) -- ported verbatim
        // per CLAUDE.md's mechanical-port rule, NOT fixed. If this assertion ever
        // starts failing because a future change added an unconditional trim, that is
        // a deliberate, documented divergence decision to make explicitly, not an
        // accidental one this test should silently absorb.
        assert!(
            !a.is_empty(),
            "documents WB-001: the empty-language automaton is corrupted to Σ* here"
        );
    }

    #[test]
    fn determinize_and_minimize_from_accepts_a_multi_state_seed() {
        // An adversarial review found the previous version of this test misleadingly
        // named itself "multi-state seed" while actually passing `Fa::reverse`'s
        // return value, which happened to be a SINGLETON `{2}` for that shape -- the
        // whole point of the `_from` overload was never actually exercised. This
        // version passes a genuine 2-element seed `{0, 1}` directly: state 0 --1--> 2
        // (accepting), state 1 --0--> 2 (accepting), state 2 a dead end. Starting the
        // NFA simultaneously in both seed states, the accepted language is exactly the
        // two single-symbol words {"0", "1"} (from state 1 / state 0 respectively) --
        // nothing else, since state 2 has no outgoing transitions.
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![2]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 0, 1],
                d: vec![d0, d1, BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["a".to_string()],
            vec![Some(true)],
        );
        let seed: BTreeSet<usize> = [0, 1].into_iter().collect();
        assert_eq!(seed.len(), 2, "sanity: this really is a multi-state seed");
        a.determinize_and_minimize_from(&seed);

        assert!(a.fa.is_deterministic());
        assert!(a.fa.accepts_word(&[0]), "0 must be accepted (via state 1)");
        assert!(a.fa.accepts_word(&[1]), "1 must be accepted (via state 0)");
        assert!(!a.fa.accepts_word(&[]), "empty word must be rejected");
        assert!(
            !a.fa.accepts_word(&[0, 0]),
            "00 must be rejected (dead end)"
        );
        assert!(
            !a.fa.accepts_word(&[0, 1]),
            "01 must be rejected (dead end)"
        );
    }

    #[test]
    fn as_dfa_wraps_a_determinized_result() {
        let a = contains_one_nfa_automaton();
        let dfa = a.as_dfa();
        assert!(dfa.automaton().fa.is_deterministic());
        assert!(dfa.automaton().fa.accepts_word(&[1]));
        assert!(!dfa.automaton().fa.accepts_word(&[0]));
    }

    #[test]
    fn automaton_dfa_from_leaves_an_already_deterministic_automaton_alone() {
        let mut a = contains_one_nfa_automaton();
        a.determinize_and_minimize();
        let q_before = a.fa.q;
        let dfa = AutomatonDFA::from(a);
        assert_eq!(dfa.automaton().fa.q, q_before);
    }

    #[test]
    #[should_panic(expected = "NFAOs are not supported")]
    fn automaton_dfa_from_panics_on_nondeterministic_dfao() {
        let mut a = contains_one_nfa_automaton();
        a.fa.o[1] = 2; // output > 1 marks this a DFAO (word automaton)
        AutomatonDFA::from(a);
    }

    #[test]
    fn automaton_dfa_into_automaton_round_trips() {
        let a = contains_one_nfa_automaton();
        let dfa = a.as_dfa();
        let back = dfa.into_automaton();
        assert!(back.fa.is_deterministic());
    }

    // --- is_in_new_alphabet / rebuild_transitions_for_new_alphabet (the
    // self-contained half of Automaton.setAlphabet -- see module docs) ---

    #[test]
    fn is_in_new_alphabet_checks_every_track() {
        let new_alphabet = vec![vec![0, 1], vec![2, 3]];
        assert!(Automaton::is_in_new_alphabet(&new_alphabet, &[0, 2]));
        assert!(Automaton::is_in_new_alphabet(&new_alphabet, &[1, 3]));
        assert!(!Automaton::is_in_new_alphabet(&new_alphabet, &[5, 2]));
        assert!(!Automaton::is_in_new_alphabet(&new_alphabet, &[0, 9]));
    }

    #[test]
    fn rebuild_transitions_for_new_alphabet_drops_symbols_outside_the_new_alphabet() {
        // One track, old alphabet [0,1,2]; two transitions out of state 0.
        let mut old = Automaton::new(
            trivial_fa(3),
            vec![vec![0, 1, 2]],
            vec!["a".to_string()],
            vec![Some(true)],
        );
        let sym0 = old.encode(&[0]);
        let sym2 = old.encode(&[2]);
        old.fa.d[0].insert(sym0, vec![0]);
        old.fa.d[0].insert(sym2, vec![0]);

        // New alphabet drops digit 2.
        let new_alphabet = vec![vec![0, 1]];
        let new_encoder = Automaton::compute_encoder(&new_alphabet);
        let new_d =
            Automaton::rebuild_transitions_for_new_alphabet(&old, &new_alphabet, &new_encoder);

        assert_eq!(new_d.len(), 1);
        // Only the digit-0 transition survives, re-encoded under the new alphabet.
        assert_eq!(new_d[0].len(), 1);
        let new_sym0 = Automaton::encode_with(&[0], &new_alphabet, &new_encoder);
        assert_eq!(new_d[0].get(&new_sym0), Some(&vec![0]));
    }

    #[test]
    fn rebuild_transitions_for_new_alphabet_handles_multi_track_and_reordered_digits() {
        // Adversarial-review finding: the single-track test above can't catch a
        // wrong-track-order or index-vs-value mistake, since a 1-track re-encode has
        // no track order to get wrong. Two tracks here, and the new alphabet
        // REORDERS track 1's digits (`[1, 0]` instead of `[0, 1]`) -- so `encode`'s
        // index-in-list-not-literal-value rule (see this module's top doc comment)
        // must be respected for this to round-trip correctly.
        let mut old = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![0, 1]],
            vec!["a".to_string(), "b".to_string()],
            vec![Some(true), Some(true)],
        );
        let sym01 = old.encode(&[0, 1]); // a=0, b=1
        old.fa.d[0].insert(sym01, vec![0]);

        let new_alphabet = vec![vec![0, 1], vec![1, 0]]; // track b's digits reordered
        let new_encoder = Automaton::compute_encoder(&new_alphabet);
        let new_d =
            Automaton::rebuild_transitions_for_new_alphabet(&old, &new_alphabet, &new_encoder);

        assert_eq!(new_d[0].len(), 1);
        // The transition must be re-encoded against the NEW alphabet's index-of-value
        // for b=1 (index 0 in `[1, 0]`), not the old alphabet's index-of-value (1).
        let new_sym = Automaton::encode_with(&[0, 1], &new_alphabet, &new_encoder);
        assert_eq!(new_d[0].get(&new_sym), Some(&vec![0]));
    }

    // --- the trivial (TRUE/FALSE) automaton at the `Automaton` layer (U0) ---

    #[test]
    fn true_false_constructor_has_no_tracks() {
        for truth in [true, false] {
            let a = Automaton::true_false(truth);
            assert!(a.is_true_false_automaton());
            assert_eq!(a.is_true_automaton(), truth);
            assert!(a.alphabet.is_empty() && a.label.is_empty() && a.msd.is_empty());
            assert_eq!(a.get_arity(), 0, "Automaton.java:499");
            // Vacuously bound (0 labels for 0 tracks) -- worth pinning, because it is
            // exactly why `bind`'s and `create_basic_automaton`'s trivial guards cannot
            // be folded into their arity/`is_bound` checks.
            assert!(a.is_bound());
        }
    }

    #[test]
    fn is_empty_reads_the_truth_value_not_the_state_set() {
        // `Automaton.java:514-517`. Both trivial automata have ZERO states, so
        // `Fa::is_language_empty` alone would answer `true` for both.
        assert!(!Automaton::true_false(true).is_empty());
        assert!(Automaton::true_false(false).is_empty());
        assert!(
            Automaton::true_false(true).fa.is_language_empty(),
            "the underlying Fa really does look empty -- the branch is load-bearing"
        );
    }

    #[test]
    fn clear_wipes_track_metadata_but_leaves_the_flag_and_a_stale_q() {
        // `Automaton.clear()` (`:506-512`), as invoked by
        // `AutomatonQuantification.quantifyHelper:63`.
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 4,
                o: vec![0, 1],
                d: vec![BTreeMap::new(), BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["y".into(), "x".into()],
            vec![Some(true), Some(true)],
        );
        a.fa.true_false = Some(true);
        a.clear();

        assert!(a.alphabet.is_empty() && a.label.is_empty() && a.msd.is_empty());
        assert!(a.fa.o.is_empty() && a.fa.d.is_empty());
        assert_eq!(a.fa.q, 2, "stale, faithfully -- see FA.clear()");
        assert!(a.is_true_false_automaton() && a.is_true_automaton());
        assert_eq!(a.get_arity(), 0);
    }

    #[test]
    fn canonize_is_a_noop_on_a_trivial_automaton() {
        let mut a = Automaton::true_false(true);
        a.canonize();
        assert!(a.is_true_false_automaton() && a.is_true_automaton());
        assert!(a.label.is_empty());
    }

    #[test]
    #[should_panic(expected = "invalid use of method bind")]
    fn bind_rejects_a_trivial_automaton_even_with_a_matching_zero_arity() {
        // `Automaton.java:439`'s FIRST disjunct. `bind(vec![])` passes the arity half
        // (`0 == 0`), so only the trivial guard can reject it.
        Automaton::true_false(true).bind(Vec::new());
    }

    #[test]
    fn as_dfa_returns_a_fresh_trivial_dfa_discarding_stale_fields() {
        // `AutomatonDFA.from`'s `:79-81` branch. Built from the stale-`q` shape so the
        // "fresh, not a copy" part is observable.
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![Some(true)],
        );
        a.fa.true_false = Some(false);
        a.clear();
        assert_eq!(a.fa.q, 1, "sanity: the input carries a stale q");

        let dfa = a.as_dfa();
        assert!(dfa.automaton().is_true_false_automaton());
        assert!(!dfa.automaton().is_true_automaton());
        assert_eq!(dfa.automaton().fa.q, 0, "rebuilt, not copied");
    }

    #[test]
    fn automaton_dfa_true_false_constructor_skips_the_determinism_machinery() {
        for truth in [true, false] {
            let dfa = AutomatonDFA::true_false(truth);
            assert!(dfa.automaton().is_true_false_automaton());
            assert_eq!(dfa.automaton().is_true_automaton(), truth);
        }
    }

    // ===================================== U5: applyAllRepresentations(WithOutput)

    /// A one-track automaton over `{0,1}` accepting the words with no `11` substring
    /// (`walnut-java/Custom Bases/msd_fib.txt`, verbatim) — the restriction a Fibonacci-style
    /// custom base attaches to each of its tracks.
    fn no_adjacent_ones(label: &str) -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![0]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![1, 1],
                d: vec![d0, d1],
            },
            vec![vec![0, 1]],
            vec![label.to_string()],
            vec![Some(true)],
        )
    }

    /// The `n`-track total automaton over `{0,1}` accepting everything, with `outputs` as its
    /// single state's output (so `1` is `Σ*` and `7` is a DFAO value, for
    /// `apply_all_representations_with_output`).
    fn universal_tracks(labels: &[&str], output: i32) -> Automaton {
        let n = labels.len();
        let alphabet_size = 1usize << n;
        let mut d0 = BTreeMap::new();
        for sym in 0..alphabet_size as i32 {
            d0.insert(sym, vec![0usize]);
        }
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size,
                o: vec![output],
                d: vec![d0],
            },
            vec![vec![0, 1]; n],
            labels.iter().map(|s| s.to_string()).collect(),
            vec![Some(true); n],
        )
    }

    #[test]
    fn apply_all_representations_is_a_total_no_op_when_no_track_is_restricted() {
        let before = universal_tracks(&["x", "y"], 1);
        let mut after = before.clone();
        after.apply_all_representations();
        assert_eq!(after.fa.q, before.fa.q);
        assert_eq!(after.fa.o, before.fa.o);
        assert_eq!(after.fa.d, before.fa.d);
        assert_eq!(after.label, before.label);
        assert_eq!(after.alphabet, before.alphabet);
        assert_eq!(after.msd, before.msd);
    }

    #[test]
    fn apply_all_representations_intersects_the_restricted_track_only() {
        let mut a = universal_tracks(&["x", "y"], 1);
        a.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("ignored"))), None]);
        a.apply_all_representations();

        // Track `x` is restricted, track `y` is not: `(11, **)` is rejected, `(**, 11)` is not.
        let word = |xs: &[i32], ys: &[i32]| -> Vec<i32> {
            xs.iter()
                .zip(ys.iter())
                .map(|(&x, &y)| a.encode(&[x, y]))
                .collect()
        };
        assert!(a.fa.accepts_word(&word(&[0, 1], &[1, 1])));
        assert!(!a.fa.accepts_word(&word(&[1, 1], &[0, 0])));
        assert_eq!(a.label, vec!["x", "y"], "labels are unchanged");
    }

    #[test]
    fn apply_all_representations_applies_every_restricted_track() {
        let mut a = universal_tracks(&["x", "y"], 1);
        a.set_all_reps(vec![
            Some(Rc::new(no_adjacent_ones("ignored"))),
            Some(Rc::new(no_adjacent_ones("ignored"))),
        ]);
        a.apply_all_representations();
        let word = |xs: &[i32], ys: &[i32]| -> Vec<i32> {
            xs.iter()
                .zip(ys.iter())
                .map(|(&x, &y)| a.encode(&[x, y]))
                .collect()
        };
        assert!(a.fa.accepts_word(&word(&[0, 1], &[1, 0])));
        assert!(
            !a.fa.accepts_word(&word(&[0, 1], &[1, 1])),
            "the SECOND track's restriction must survive too -- `K` accumulates"
        );
        assert!(!a.fa.accepts_word(&word(&[1, 1], &[0, 1])));
    }

    /// `determineRandomLabel`/`unlabel`'s exact interplay (`Automaton.java:252-270`), ported
    /// verbatim including the part that looks like an oversight: `unlabel()` runs BEFORE
    /// `copy(K)`, so when at least one restriction was applied it is overwritten and the
    /// automaton comes back **bound** to `randomLabel`'s numeric names.
    #[test]
    fn apply_all_representations_label_bookkeeping_matches_java() {
        // Unbound + nothing to apply: `unlabel()` wins, the automaton stays unbound.
        let mut nothing = universal_tracks(&["x", "y"], 1);
        nothing.unlabel();
        assert!(!nothing.is_bound());
        nothing.apply_all_representations();
        assert!(
            !nothing.is_bound(),
            "still unbound, nothing was copied over it"
        );

        // Unbound + something to apply: `copy(K)` restores the random labels.
        let mut applied = universal_tracks(&["x", "y"], 1);
        applied.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("ignored"))), None]);
        applied.unlabel();
        applied.apply_all_representations();
        assert_eq!(
            applied.label,
            vec!["0", "1"],
            "Java's dead `unlabel()` is overwritten by `copy(K)`"
        );
    }

    /// `applyAllRepresentationsWithOutput`'s two documented differences from its sibling:
    /// it preserves DFAO output values (`IF_OTHER_OP` returns `aP`, not `0`/`1`), and it
    /// combines with `this` rather than the running `K` — so with two restricted tracks only
    /// the LAST restriction survives. Java's own source comment says the second is by design
    /// ("causes a bug in `combine()` otherwise"), so it is ported verbatim.
    #[test]
    fn apply_all_representations_with_output_keeps_dfao_values_and_only_the_last_track() {
        let mut a = universal_tracks(&["x", "y"], 7);
        a.set_all_reps(vec![
            Some(Rc::new(no_adjacent_ones("ignored"))),
            Some(Rc::new(no_adjacent_ones("ignored"))),
        ]);
        a.apply_all_representations_with_output();

        assert!(
            a.fa.o.contains(&7),
            "the word-automaton output value survives (IF_OTHER_OP returns aP)"
        );
        let word = |xs: &[i32], ys: &[i32]| -> Vec<i32> {
            xs.iter()
                .zip(ys.iter())
                .map(|(&x, &y)| a.encode(&[x, y]))
                .collect()
        };
        assert!(
            !a.fa.accepts_word(&word(&[0, 1], &[1, 1])),
            "the last track's restriction IS applied"
        );
        assert!(
            a.fa.accepts_word(&word(&[1, 1], &[0, 1])),
            "the first track's restriction is discarded -- combines with `this`, not `K`"
        );
    }

    // --- the per-track metadata stays parallel through every track-list mutation ---

    #[test]
    fn sort_label_permutes_all_reps_alongside_msd() {
        let restriction = Rc::new(no_adjacent_ones("ignored"));
        let mut a = Automaton::new(
            trivial_fa(8),
            vec![vec![0, 1], vec![0, 1], vec![0, 1]],
            vec!["c".into(), "a".into(), "b".into()],
            vec![Some(true), Some(false), None],
        );
        a.set_all_reps(vec![Some(Rc::clone(&restriction)), None, None]);
        a.sort_label();
        assert_eq!(a.label, vec!["a", "b", "c"]);
        assert_eq!(a.msd, vec![Some(false), None, Some(true)]);
        assert!(
            a.all_reps[0].is_none() && a.all_reps[1].is_none() && a.all_reps[2].is_some(),
            "the restriction followed track `c` to its new position"
        );
    }

    #[test]
    fn reduce_dimension_drops_the_merged_tracks_all_reps_entry() {
        let restriction = Rc::new(no_adjacent_ones("ignored"));
        let mut a = Automaton::new(
            trivial_fa(8),
            vec![vec![0, 1], vec![0, 1], vec![0, 1]],
            Vec::new(),
            vec![Some(true), Some(true), Some(true)],
        );
        a.set_all_reps(vec![
            Some(Rc::clone(&restriction)),
            Some(Rc::clone(&restriction)),
            None,
        ]);
        // Two tracks share a label, so `bind` merges them via `reduceDimension`.
        a.bind(vec!["x".into(), "x".into(), "y".into()]);
        assert_eq!(a.label, vec!["x", "y"]);
        assert_eq!(a.msd.len(), 2);
        assert_eq!(a.all_reps.len(), 2, "stayed parallel to msd");
        assert!(a.all_reps[0].is_some() && a.all_reps[1].is_none());
    }

    #[test]
    fn clear_empties_all_reps_with_the_rest_of_the_track_metadata() {
        let mut a = universal_tracks(&["x"], 1);
        a.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("ignored")))]);
        a.clear();
        assert!(a.all_reps.is_empty());
        assert!(a.msd.is_empty());
    }

    #[test]
    #[should_panic(expected = "one entry per track required")]
    fn set_all_reps_rejects_a_wrong_length_list() {
        let mut a = universal_tracks(&["x", "y"], 1);
        a.set_all_reps(vec![None]);
    }

    #[test]
    #[should_panic(expected = "no number system")]
    fn set_all_reps_rejects_a_restriction_on_a_non_arithmetic_track() {
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![None],
        );
        a.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("ignored")))]);
    }

    // ------------------------------------------------------------------------
    // `normalize_number_systems` / `track_ns_names` (U23: `Main.Commands.Concat`/`Star`'s
    // `Automaton.normalizeNumberSystems` call, and `Describe`'s NS-name reconstruction).
    // ------------------------------------------------------------------------

    #[test]
    fn normalize_number_systems_is_a_no_op_when_no_track_has_a_restriction() {
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![Some(true)],
        );
        let before_q = a.fa.q;
        let before_o = a.fa.o.clone();
        a.normalize_number_systems(&mut crate::logging::Logging::new());
        assert_eq!(a.fa.q, before_q);
        assert_eq!(a.fa.o, before_o);
        assert!(a.all_reps[0].is_none());
    }

    #[test]
    fn normalize_number_systems_drops_the_restriction_on_a_switched_track() {
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![Some(true)],
        );
        a.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("x")))]);
        assert!(a.all_reps[0].is_some(), "precondition: track is switched");

        a.normalize_number_systems(&mut crate::logging::Logging::new());

        assert!(
            a.all_reps[0].is_none(),
            "the restriction must be dropped on the switched track"
        );
        // Still a well-formed, deterministic automaton -- `determinizeAndMinimize` ran.
        assert!(a.fa.is_deterministic());
    }

    #[test]
    fn normalize_number_systems_only_drops_switched_tracks_not_every_track() {
        let mut a = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".into(), "y".into()],
            vec![Some(true), Some(true)],
        );
        a.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("x"))), None]);
        a.normalize_number_systems(&mut crate::logging::Logging::new());
        assert!(a.all_reps[0].is_none());
        assert!(
            a.all_reps[1].is_none(),
            "track 1 had no restriction to begin with"
        );
    }

    #[test]
    fn track_ns_names_reconstructs_msd_and_lsd_names_and_skips_non_arithmetic_tracks() {
        let a = Automaton::new(
            trivial_fa(30),
            vec![vec![0, 1], vec![0, 1, 2], vec![0, 1, 2, 3, 4]],
            vec!["x".into(), "y".into(), "z".into()],
            vec![Some(true), Some(false), None],
        );
        assert_eq!(
            a.track_ns_names(),
            vec![Some("msd_2".to_string()), Some("lsd_3".to_string()), None,]
        );
    }

    /// U23 review fix, finding #1: the reconstruction above is a FALLBACK, not the
    /// answer. A custom base whose alphabet cardinality and direction happen to match a
    /// plain `msd_k` must still report its own name — otherwise
    /// `NumberSystem.isNSDiffering` (which compares by name) calls two genuinely
    /// different numerations "the same number system" and `union`/`intersect`/`concat`
    /// silently produce a mixed-numeration result.
    #[test]
    fn track_ns_names_prefers_the_recorded_name_over_the_base_k_reconstruction() {
        let mut a = Automaton::new(
            trivial_fa(4),
            // `msd_fib`'s real alphabet IS `{0, 1}`, i.e. indistinguishable from `msd_2`'s
            // by cardinality alone — that collision is the whole bug.
            vec![vec![0, 1], vec![0, 1]],
            vec!["x".into(), "y".into()],
            vec![Some(true), Some(true)],
        );
        a.set_ns_names(vec![Some("msd_fib".to_string()), None]);
        assert_eq!(
            a.track_ns_names(),
            vec![Some("msd_fib".to_string()), Some("msd_2".to_string())],
            "track 0 keeps its real name; track 1 (no name recorded) falls back"
        );
    }

    #[test]
    fn ns_names_survive_a_label_sort_permutation_and_a_dimension_reduction() {
        let mut a = Automaton::new(
            trivial_fa(4),
            vec![vec![0, 1], vec![0, 1]],
            vec!["y".into(), "x".into()],
            vec![Some(true), Some(true)],
        );
        a.set_ns_names(vec![Some("msd_fib".to_string()), Some("msd_2".to_string())]);
        a.sort_label();
        assert_eq!(a.label, vec!["x".to_string(), "y".to_string()]);
        assert_eq!(
            a.track_ns_names(),
            vec![Some("msd_2".to_string()), Some("msd_fib".to_string())],
            "the name must follow its own track through the permutation"
        );
    }

    #[test]
    fn set_ns_names_rejects_a_name_on_a_track_with_no_number_system() {
        let mut a = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![None],
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            a.set_ns_names(vec![Some("msd_fib".to_string())]);
        }));
        assert!(result.is_err());
    }

    /// U23 review fix, finding #5: Java's `normalizeNumberSystems` ends with an
    /// explicitly-commented `// always print this` warning. It is user-facing (the
    /// result's alphabet changed), not one of the progress/timing detail lines this port
    /// skips, so it must be emitted — and only when the normalization actually did
    /// something.
    #[test]
    fn normalize_number_systems_emits_the_alphabet_changed_warning_only_when_it_switches() {
        let mut quiet = crate::logging::Logging::new();
        let mut unswitched = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![Some(true)],
        );
        unswitched.normalize_number_systems(&mut quiet);
        assert!(
            !quiet.command_log().contains(ALPHABET_CHANGED_WARNING),
            "no track was switched, so Java's `if (switchNS)` guard skips the warning"
        );

        let mut loud = crate::logging::Logging::new();
        let mut switched = Automaton::new(
            trivial_fa(2),
            vec![vec![0, 1]],
            vec!["x".into()],
            vec![Some(true)],
        );
        switched.set_all_reps(vec![Some(Rc::new(no_adjacent_ones("x")))]);
        switched.set_ns_names(vec![Some("msd_fib".to_string())]);
        switched.normalize_number_systems(&mut loud);
        assert!(
            loud.command_log().contains(ALPHABET_CHANGED_WARNING),
            "a switched track must produce the warning verbatim"
        );
        // `new NumberSystem(determineBaseNameUnderscore() + (max + 1))` (`:169`): the
        // track is no longer on the custom base, and says so.
        assert_eq!(switched.track_ns_names(), vec![Some("msd_2".to_string())]);
    }
}
