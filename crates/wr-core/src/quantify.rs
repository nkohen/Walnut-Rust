// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Existential quantification (∃-elimination) — the decision procedure's crux.
//!
//! Ports `Automata/AutomatonQuantification.java`'s `quantify`/`quantifyHelper`/
//! `validateLabels`.
//!
//! # Why this lives in `wr-core` and not `wr-logic` (the U6 architecture call)
//!
//! `docs/BOUNDARY-MAP.md` §4.3 originally recommended porting
//! `AutomatonQuantification.java` into `wr-logic`, on the grounds that ∃-projection reads
//! as logic-layer semantics despite the file's `Automata/` package location — and §3's
//! own coupling analysis, read carefully, only traced `AutomatonQuantification`'s
//! OUTGOING calls (concluding a `wr-logic` placement creates "a normal forward
//! wr-logic→wr-core dependency, not a cycle"). **This unit found the missing INCOMING
//! edge that overturns that conclusion**, not previously recorded in either document:
//! `NumberSystem.java` — which `docs/DESIGN.md` and `Cargo.toml` both pin inside
//! `wr-core`, because `Automaton` and `NumberSystem` are bidirectionally coupled — calls
//! `AutomatonQuantification.quantify` **ten** times (verified: `NumberSystem.java:720,
//! 821, 874, 923, 951, 965, 993, 1010, 1016, 1053`) to build its base-*k*
//! adder/comparator automata by quantifying carry variables away. `wr-logic` must depend
//! on `wr-core` (its whole job is operating on `Automaton`), so a `wr-logic`-resident
//! `quantify` that `wr-core::numsys` had to call would be a genuine Cargo dependency
//! cycle. (`docs/BOUNDARY-MAP.md` §4.3 and `docs/DESIGN.md`'s crate-mapping table are
//! updated to record this decision as superseding the original `wr-logic`
//! recommendation — see those documents.)
//!
//! The resolution is the one `docs/DESIGN.md` already applied to the
//! `Automaton`↔`NumberSystem` coupling (kit-finding #1): **push the shared primitive
//! down into the crate that needs it structurally.** Nothing in this file is
//! formula/AST-level — there is no `Token`, no `Predicate`, no parser state. It is
//! "delete these tracks from every symbol, determinize, fix up leading zeros", which is
//! automaton theory over [`crate::automaton::Automaton`] and [`crate::fa::Fa`] alone.
//! Java's own call graph agrees that this is a shared leaf, not a `wr-logic` internal:
//! besides `NumberSystem`, `quantify` is called from `Main/EvalComputations/Token/`
//! (`ArithmeticOperator`, `Function`, `LogicalOperator`, `Operator`,
//! `RelationalOperator` — all `wr-logic` scope) and `AutomatonLogicalOps.java` reuses
//! `validateLabels` directly.
//!
//! `wr_logic::quantify::exists` remains the logic layer's entry point and is now a thin
//! delegation to [`quantify`]; the eventual formula-level quantifier elimination
//! (`∀ = ¬∃¬`, quantifier bookkeeping over a parsed `Predicate`) is what will give that
//! module its own content.
//!
//! # The pipeline, exactly as Java sequences it
//!
//! 1. **Project.** Decode every transition symbol into its per-track digit tuple, delete
//!    the quantified tracks from the tuple *and* from the alphabet/label/msd lists,
//!    re-encode the reduced tuple. Distinct old symbols routinely collapse onto the same
//!    new symbol — that collapse is precisely what introduces nondeterminism, and is the
//!    whole mathematical content of ∃-elimination.
//! 2. **Determinize + minimize** (`Automaton.determinizeAndMinimize()`, the no-arg
//!    overload).
//! 3. **Fix the padding zeros** — [`crate::logicalops::fix_leading_zeros_problem`] on an
//!    msd automaton, [`crate::logicalops::fix_trailing_zeros_problem`] on an lsd one, so
//!    the result accepts `0*x` (resp. `x0*`) whenever it used to accept `x` — necessary
//!    because projecting a track away can strand a representation behind a run that only
//!    existed for the quantified track's longer representation.
//!
//! # Deliberate divergences from a literal transliteration
//!
//! * **Destination merging is set union, not Java's order-preserving
//!   `addAllWithoutRepetition`** (`AutomatonQuantification.java:122-125`). When two old
//!   symbols collapse onto one new symbol their destination lists are merged; Java
//!   appends the second list's new elements to the first, preserving insertion order.
//!   Order of an NFA destination list is not observable in any language-level sense, and
//!   `CLAUDE.md`'s prime directive #1 is to compare by language equivalence, never by
//!   structure — so a [`BTreeSet`] union is used, which is simpler and obviously
//!   dedup-correct.
//! * **Step 2 always trims** (but no longer always DETERMINIZES). Java's no-arg
//!   `determinizeAndMinimize()` skips *both* the trim and the determinization when the
//!   freshly-rebuilt table is already deterministic (`Automaton.java:385`); this
//!   port trims unconditionally, so the two differ exactly on the (rare) projection that
//!   happens to come out deterministic. The determinization itself is now guarded by
//!   Java's own condition — it has to be, because the automata counter behind
//!   `[strategy n …]`/`[export n …]` advances once per *actual* determinization, and
//!   Walnut's own `details637.txt` shows the guard firing (a `quantifying:` block with a
//!   `Minimizing:` line and no `Determinizing [#n, …]` line). Trimming is always
//!   language-preserving, so the
//!   divergence is invisible to the correctness bar, and it is the *safer* choice: it
//!   establishes [`crate::minimize::minimize`]'s documented precondition (every state
//!   reachable from `q0`; violate it and the "q0 aliasing quirk", `docs/WALNUT-BUGS.md`
//!   WB-001, silently flips the language) rather than leaving it resting on the separate
//!   argument that subset construction from `{q0}` already emits only reachable states.
//!   It also shrinks the input to the exponential subset construction. Note this is a
//!   *narrower* divergence than it looks: [`crate::automaton::Automaton::determinize_and_minimize`]
//!   (ported in U2) reproduces Java's conditional trim faithfully and is *not* used here,
//!   precisely so this pre-existing Phase-1 divergence is not silently altered by the U6
//!   refactor. Reconciling the two is a deliberate later decision, not this unit's.
//!
//! (The lsd fixup used to be a third divergence here, and is no longer — see the next
//! section.)
//!
//! # The lsd fixup: deferred in Phase 2, wired up in Phase 3b (L1)
//!
//! Phase 2's U6 landed this module with its `Some(false)` arm returning a
//! `QuantifyError::UnsupportedLsdFixup` instead of calling
//! [`crate::logicalops::fix_trailing_zeros_problem`] (which U5 had already ported, and
//! which Java's `AutomatonQuantification.java:46` calls). The stated reason was that
//! flipping "reject" to "compute" is a scope decision rather than a refactoring step,
//! and that no lsd numeration system existed in [`crate::numsys`] to exercise it against
//! — the second half of which went stale the moment U7 landed `NumberSystem`'s full
//! msd/lsd surface one unit later.
//!
//! The cost of leaving it turned off turned out to be far wider than "explicit `E`/`A`/`I`
//! over an lsd variable": [`crate::numsys`] calls [`quantify`] ten times to *build* its
//! own automata, so on an lsd system every comparison or arithmetic against a constant
//! `>= 2`, every `get_constant`/`get_multiplication`/`get_division`, and therefore nearly
//! every `lsd_k` query more complex than `x < y` failed here. That was found by Phase
//! 3a's exit checkpoint and closed by this unit; the branch is now the plain port of
//! Java's `else fixTrailingZerosProblem(A)`, and `QuantifyError` no longer has an
//! `UnsupportedLsdFixup` variant to return.
//!
//! # U0 behavior change: all-tracks-quantified now yields a TRUE/FALSE automaton
//!
//! Phase 1/2 reported [`QuantifyError`]`::AllTracksQuantified` when the caller asked to
//! quantify away *every* track, because [`crate::automaton::Automaton`] had no way to
//! express Java's answer. U0 added that representation
//! ([`crate::fa::Fa::true_false`]), and this module now ports
//! `AutomatonQuantification.java:58-65` exactly:
//!
//! ```text
//! if (labelsToQuantify.size() == A.richAlphabet.getA().size()) {
//!     A.fa.setTRUE_AUTOMATON(!A.isEmpty());
//!     A.fa.setTRUE_FALSE_AUTOMATON(true);
//!     A.clear();
//!     return;
//! }
//! ```
//!
//! i.e. **`∃x₁…∃xₙ φ` over all `n` tracks is TRUE iff `L(φ) ≠ ∅`**, and the automaton is
//! then wiped by [`crate::automaton::Automaton::clear`]. Two details in that sequence
//! are load-bearing and are reproduced, not paraphrased:
//! * `!A.isEmpty()` is evaluated **before** the flags are set, so it takes
//!   `Automaton.isEmpty`'s ordinary `fa.isLanguageEmpty()` branch rather than its own
//!   new trivial branch. Reversing the order would make every result FALSE.
//! * `A.clear()` leaves `fa.q` **stale** (see [`crate::fa::Fa::clear`]); the shape is
//!   tolerated by `Fa::canonicalize`/`crate::trim`, which both branch on the flag.
//!
//! [`quantify`] then honours Java's `if (A.fa.isTRUE_FALSE_AUTOMATON()) return;` (`:39`)
//! and skips the numeration-direction fixup entirely.
//!
//! # Faithfully-ported quirks (not divergences)
//!
//! * [`quantify`] runs the leading-zero fixup **even when [`quantify_helper`]
//!   short-circuits** (empty label set, or an automaton with no labels at all): Java's
//!   `quantify` (`:37-47`) calls the helper for effect and then unconditionally consults
//!   `determineMsd`. So `quantify(a, &{})` is *not* a no-op — see
//!   `wr_logic::quantify`'s `empty_label_set_still_runs_the_zero_fixup`.
//! * A label that appears on more than one track is resolved by `List.indexOf` (`:70`),
//!   i.e. only its **first** occurrence is quantified away.
//! * `zeroReachableStates` mutates the automaton it inspects — see
//!   [`crate::logicalops::zero_reachable_states`].

use crate::automaton::Automaton;
use crate::logicalops::{fix_leading_zeros_problem_with_ctx, fix_trailing_zeros_problem};
use crate::minimize::{minimize, MinimizeError};
use crate::numsys::determine_msd;
use crate::trim::trim;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuantifyError {
    /// A requested label is not one of the automaton's track labels. Ports
    /// `WalnutException.notFreeVariable` (thrown from
    /// `AutomatonQuantification.validateLabels`, `:110-116`).
    NotFreeVariable(String),
    /// Propagated from [`crate::minimize::minimize`] inside [`quantify_helper`]. Both
    /// variants should be unreachable there (subset construction always yields a
    /// deterministic automaton), but they are surfaced rather than `unwrap`ped:
    /// `PORTING.md`'s error-mapping rule is that a "can't happen" is still a `Result`,
    /// never a panic or a `debug_assert!`.
    Minimize(MinimizeError),
}

impl From<MinimizeError> for QuantifyError {
    fn from(e: MinimizeError) -> Self {
        QuantifyError::Minimize(e)
    }
}

/// Existentially quantifies `labels` out of `a`, in place.
///
/// Ports `AutomatonQuantification.quantify(Automaton, Set<String>)` (`:37-47`): project
/// the labelled tracks away, determinize + minimize, then apply the
/// numeration-system-dependent padding-zero fixup (leading zeros on an msd system,
/// trailing zeros on an lsd one). `labels` must be a subset of `a.label`
/// (else [`QuantifyError::NotFreeVariable`]).
///
/// Naming *every* track is legal and turns `a` into a TRUE/FALSE automaton — see this
/// module's "U0 behavior change" section.
///
/// On success `a.alphabet` / `a.label` / `a.msd` have had the quantified tracks removed,
/// `a.fa` is a minimal (generally *partial* — `minimize` drops non-co-reachable states)
/// DFA over the reduced alphabet, and state numbering bears no relation to Walnut's.
pub fn quantify(a: &mut Automaton, labels: &BTreeSet<String>) -> Result<(), QuantifyError> {
    quantify_with_ctx(a, labels, None)
}

/// [`quantify`] with an explicit [`crate::determinize::DeterminizeContext`] — Walnut's
/// `[strategy …]`/`[export …]` metacommand state, which Java reads out of the
/// `Prover.mainProver.metaCommands` singleton inside the determinization dispatcher.
///
/// `None` is exactly [`quantify`]. `Some(ctx)` is the port of
/// `Logging.shouldPrintDetails() == true`, so the caller owes the print-details gate (see
/// [`crate::automaton::Automaton::determinize_and_minimize_with_ctx`]).
///
/// **One `quantify` consumes up to TWO automata indices**, and Walnut's own `details`
/// fixtures show both: `quantifyHelper`'s `determinizeAndMinimize()` (skipped when the
/// projected automaton happens to be deterministic already) and then the zero fixup's
/// unconditional `determinizeAndMinimize(IntSet)` — e.g. `details637.txt`'s
/// `Determinizing [#6, strategy: Brzozowski]` (the projection) immediately followed by
/// `Determinizing [#7, strategy: SC]` (its `fixing leading zeros` block).
pub fn quantify_with_ctx(
    a: &mut Automaton,
    labels: &BTreeSet<String>,
    mut ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
) -> Result<(), QuantifyError> {
    quantify_helper(a, labels, ctx.as_deref_mut())?;

    // `if (A.fa.isTRUE_FALSE_AUTOMATON()) return;` (`:39`): a trivial automaton has no
    // tracks and no numeration direction, so the fixup is skipped entirely. Note this
    // also fires when `a` was ALREADY trivial on entry (the helper short-circuits on an
    // empty label list), matching Java.
    if a.fa.is_true_false_automaton() {
        return Ok(());
    }

    // `quantify`'s tail: consult the surviving tracks' numeration direction. Note this
    // runs even when `quantify_helper` short-circuited — a faithfully-ported quirk, see
    // the module docs.
    match determine_msd(&a.msd) {
        None => Ok(()),
        Some(true) => {
            // `AutomatonLogicalOps.fixLeadingZerosProblem(A)` (`:44`). Its closing
            // `determinizeAndMinimize(IntSet)` panics rather than returning a
            // `MinimizeError`, matching every other `wr-core` call site of that method;
            // both of `minimize`'s error variants are unreachable there (subset
            // construction's output is deterministic and `q0`-reachable by
            // construction). The Phase-1 copy of this fixup that used to live in
            // `wr_logic::quantify` returned `Err(QuantifyError::Minimize(..))` on that
            // same unreachable path instead — the only observable difference between
            // the two copies, and only on input neither can produce.
            //
            // This is a real, adversarial-review-flagged tension with `PORTING.md`'s
            // rule that "a 'can't happen' is still a `Result`, never a panic" — worth
            // recording rather than quietly accepting. It is NOT reopened here: this
            // call site inherits `fix_leading_zeros_problem`'s already-reviewed (U5)
            // panic, and rebuilding a `?`-propagating duplicate of that function inside
            // this one just to keep `quantify`'s own signature panic-free would recreate
            // the exact fixup-logic duplication U6 exists to eliminate. If the
            // panic-vs-`Result` policy for this whole class of "provably-unreachable
            // post-subset-construction minimize" call sites is ever revisited, it should
            // be a single crate-wide decision (starting from `determinize_and_minimize`/
            // `determinize_and_minimize_from` in `automaton.rs`, U2), not a one-off fix
            // here.
            fix_leading_zeros_problem_with_ctx(a, ctx);
            Ok(())
        }
        Some(false) => {
            // `AutomatonLogicalOps.fixTrailingZerosProblem(A)` (`:46`) — the lsd branch,
            // wired up in Phase 3b's L1 (it was a Phase-2 scope cut; see the module docs'
            // "The lsd fixup" section for the history).
            //
            // Unlike its msd sibling this one needs no guard of its own at this call
            // site, and deliberately has none (matching Java, which likewise gives it no
            // `isTRUE_FALSE_AUTOMATON()` check — see `fix_trailing_zeros_problem`'s own
            // docs). The two shapes that would fault are both excluded here:
            // * TRIVIAL (`true_false.is_some()`): `quantify` already returned above, at
            //   its `is_true_false_automaton()` guard, so the stale-`q` trivial shape
            //   never reaches this arm.
            // * ZERO-STATE (`fa.q == 0`, flag unset): reachable — `quantify_helper`'s
            //   `a.fa.q == 0` early return fires *after* its empty-label-set return, so
            //   `quantify(a, &{})` on a 0-state automaton lands here. It is nonetheless
            //   safe, and for a stronger reason than "the caller checks": unlike
            //   `zero_reachable_states` (which unconditionally indexes `fa.d[fa.q0]` and
            //   is exactly why `fix_leading_zeros_problem` needs its own `fa.q == 0`
            //   guard), `set_states_reachable_to_final_states_by_zeros` only ever indexes
            //   inside `for q in 0..fa.q`, so on a 0-state automaton every one of its
            //   loops is empty, it reports `altered == false`, and the whole call is a
            //   silent no-op. `determine_zero()` is likewise total (`encode(&[])` is `0`
            //   with no indexing). Pinned by
            //   `quantify_on_a_zero_state_lsd_automaton_is_a_silent_noop`.
            //
            // One asymmetry worth recording here rather than rediscovering later: this
            // arm can reach `docs/WALNUT-BUGS.md` WB-001 where the msd arm cannot.
            // `fix_trailing_zeros_problem` closes with `just_minimize`, which never trims
            // (Java's `justMinimize` doesn't either — see `logicalops.rs`'s note on that
            // helper), whereas `fix_leading_zeros_problem` closes with
            // `determinize_and_minimize_from`, whose subset construction re-establishes
            // `minimize`'s reachability precondition on the way through. The ordinary
            // path is unaffected — `quantify_helper`'s own determinize+minimize leaves
            // every state reachable from `q0` — so this can only bite when the helper
            // SHORT-CIRCUITED (empty label set, or a label-less automaton) on an input
            // that already had an unreachable state. Java has the identical shape at the
            // identical call site, so it is ported verbatim, not guarded.
            //
            // NOTE the asymmetry this creates for the automata-index counter: unlike its
            // msd sibling, this fixup closes with `just_minimize` and never reaches the
            // determinization dispatcher, so it consumes NO automata index. That is
            // Java's shape too (`AutomatonLogicalOps.fixTrailingZerosProblem` calls
            // `fa.justMinimize()`, not `determinizeAndMinimize`), hence the unused `ctx`
            // here -- an lsd `quantify` costs one index, an msd one costs two.
            let _ = ctx;
            fix_trailing_zeros_problem(a);
            Ok(())
        }
    }
}

/// Ports `AutomatonQuantification.quantifyHelper` (`:49-108`): the projection itself,
/// followed by determinize + minimize. Leaves `a` untouched on any `Err`.
fn quantify_helper(
    a: &mut Automaton,
    labels: &BTreeSet<String>,
    ctx: Option<&mut (dyn crate::determinize::DeterminizeContext + '_)>,
) -> Result<(), QuantifyError> {
    // Java: `if (labelsToQuantify.isEmpty() || A.getLabel() == null ||
    // A.getLabel().isEmpty()) return;` (`:50-52`) — note this precedes `validateLabels`,
    // so asking to quantify a name out of a *label-less* automaton is silently accepted,
    // not an error.
    if labels.is_empty() || a.label.is_empty() {
        return Ok(());
    }

    // `validateLabels` (`:110-116`).
    for l in labels {
        if !a.label.contains(l) {
            return Err(QuantifyError::NotFreeVariable(l.clone()));
        }
    }

    // Java: `if (labelsToQuantify.size() == A.richAlphabet.getA().size())` (`:60-65`) —
    // every track quantified, so the result is the TRUE automaton iff the language was
    // non-empty. See this module's "U0 behavior change" section for why the two
    // statements below must stay in this order and why `clear` leaves `fa.q` stale.
    //
    // NOTE the comparison Java actually makes: `labelsToQuantify.size()` is the size of
    // the deduplicated label SET, compared against the TRACK count — so an automaton
    // with a repeated label (`f(a,a)`-shaped, two tracks both named "a") takes the
    // ordinary projection path here, not this one. `BTreeSet<String>` reproduces the
    // same deduplication.
    if labels.len() == a.alphabet.len() {
        let truth = !a.is_empty();
        a.fa.true_false = Some(truth);
        a.clear();
        return Ok(());
    }

    // A 0-state automaton (labels present, but no states — legal to construct via this
    // crate's API even though a real Walnut `Automaton` never reaches it) has nothing
    // for the projection to do. Without this guard, `trim`/`subset_construction` below
    // would index the stale `q0` into an empty `fa.d` and panic —
    // `logicalops::fix_leading_zeros_problem` has the identical guard for the identical
    // reason; this mirrors it for parity.
    if a.fa.q == 0 {
        return Ok(());
    }

    // `A.getLabel().indexOf(l)` for each label (`:70`): first occurrence only.
    let dropped: BTreeSet<usize> = labels
        .iter()
        .map(|l| {
            a.label
                .iter()
                .position(|x| x == l)
                .expect("label presence was just validated")
        })
        .collect();
    let kept: Vec<usize> = (0..a.alphabet.len())
        .filter(|i| !dropped.contains(i))
        .collect();

    // `allInputs` (`:71-73`): decode every OLD symbol before the alphabet shrinks.
    let old_alphabet_size = a.fa.alphabet_size;
    let all_inputs: Vec<Vec<i32>> = (0..old_alphabet_size as i32).map(|s| a.decode(s)).collect();

    // `removeIndices` on A / NS / label (`:75-81`) — order of the survivors is preserved.
    let new_alphabet: Vec<Vec<i32>> = kept.iter().map(|&i| a.alphabet[i].clone()).collect();
    let new_label: Vec<String> = kept.iter().map(|&i| a.label[i].clone()).collect();
    let new_msd: Vec<Option<bool>> = kept.iter().map(|&i| a.msd[i]).collect();
    // `removeIndices(A.getNS(), I)` removes the whole per-track `NumberSystem`, i.e. all
    // three parts of this crate's stand-in (`Automaton::all_reps`'s invariant, U5, plus
    // `Automaton::ns_name`).
    let new_all_reps: Vec<Option<std::rc::Rc<Automaton>>> =
        kept.iter().map(|&i| a.all_reps[i].clone()).collect();
    let new_ns_names: Vec<Option<String>> = kept
        .iter()
        .map(|&i| a.ns_name.get(i).cloned().flatten())
        .collect();
    let new_alphabet_size: usize = new_alphabet.iter().map(|t| t.len()).product();

    // Building the reduced `Automaton` here (rather than mutating `a` in place) is what
    // rebuilds the mixed-radix encoder — Java's `richAlphabet.setEncoder(null)` +
    // `determineAlphabetSize()` (`:76-77`), which force a lazy recompute on the next
    // `encode`.
    let mut projected = Automaton::new(a.fa.clone(), new_alphabet, new_label, new_msd);
    projected.set_all_reps(new_all_reps);
    projected.set_ns_names(new_ns_names);
    projected.fa.alphabet_size = new_alphabet_size;

    // `permutation[old] = new` (`:83-85`): re-encode each decoded tuple minus the
    // quantified tracks. Many old symbols map to one new symbol — that collapse is the
    // projection.
    let permutation: Vec<i32> = all_inputs
        .iter()
        .map(|digits| {
            let reduced: Vec<i32> = kept.iter().map(|&i| digits[i]).collect();
            projected.encode(&reduced)
        })
        .collect();

    // Rebuild the transition table under the new symbol ids (`:87-102`), unioning the
    // destinations of old symbols that collapsed together (see the module docs on why
    // union, not Java's order-preserving append). Indexing `permutation` panics on an
    // out-of-range symbol, exactly as Java's `permutation.get(...)` throws — a corrupt
    // table is a caller bug.
    let mut merged: Vec<BTreeMap<i32, BTreeSet<usize>>> = vec![BTreeMap::new(); a.fa.d.len()];
    for (q, row) in a.fa.d.iter().enumerate() {
        for (&sym, dests) in row {
            let mapped = permutation[sym as usize];
            merged[q]
                .entry(mapped)
                .or_default()
                .extend(dests.iter().copied());
        }
    }
    projected.fa.d = merged
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(sym, dests)| (sym, dests.into_iter().collect::<Vec<usize>>()))
                .collect()
        })
        .collect();

    // `A.determinizeAndMinimize()` (`:104`) — with the unconditional trim described in
    // the module docs (Java trims only when the rebuilt table is nondeterministic).
    //
    // Routed through the `determinize` dispatcher (U0c, `crate::determinize::determinize`)
    // rather than calling `subset_construction` directly, so that Phase 3b's
    // `[strategy …]`/`[export …]` metacommands apply here too -- this is the single most
    // common determinization site in the whole engine (every existentially-quantified
    // variable goes through it; see `automaton.rs`'s corrected module-level note on
    // U0c's actual call-graph coverage). `ctx = None` is bit-for-bit identical to the
    // pre-dispatcher direct call (`determinize.rs`'s
    // `no_context_is_exactly_plain_subset_construction` pins this), and with `ctx = None`
    // the dispatcher's only fallible arm is unreachable -- the same reasoning
    // `Automaton::determinize_and_minimize`'s `NO_CONTEXT_CANNOT_FAIL` already documents.
    // With `ctx = Some(..)` that arm IS reachable (a non-`SC` strategy on a DFAO), and
    // `crate::automaton::dispatch_determinize` renders it as Java's own thrown
    // `WalnutException` message -- see its docs. Either way this is a Rust-native
    // dispatcher concern, not a ported Java "can't happen", so `PORTING.md`'s
    // Result-not-panic rule for the latter (see `QuantifyError::Minimize`'s doc just
    // above) doesn't apply to it -- unlike the `minimize` call below, which stays a
    // propagated `Result` exactly as before.
    //
    // The `!isDeterministic()` GUARD, however, is Java's own (`Automaton.java:385`) and
    // is ported verbatim -- evaluated on the freshly-rebuilt table, BEFORE the trim,
    // exactly where Java evaluates it. It used to be absent here (this port determinized
    // unconditionally), which was invisible while `ctx` was always `None` but is not
    // invisible now: `MetaCommands`' automata counter advances once per *actual*
    // determinization, so an extra one shifts every later `[strategy n …]`/`[export n …]`
    // index. Walnut's own golden fixture proves the guard fires in practice --
    // `details637.txt`'s second `quantifying:` block has a `Minimizing:` line and NO
    // `Determinizing [#n, …]` line, i.e. Java skipped the dispatcher there, and the very
    // next index (`#2`) went to that quantification's `fixing leading zeros` step
    // instead. (Skipping also preserves a DFAO's real output values, which subset
    // construction would flatten to 0/1 -- another way the old unconditional call
    // diverged.)
    let was_deterministic = projected.fa.is_deterministic();
    projected.fa = trim(&projected.fa);
    if !was_deterministic {
        let initial: BTreeSet<usize> = [projected.fa.q0].into_iter().collect();
        crate::automaton::dispatch_determinize(&mut projected, &initial, ctx);
    }
    projected.fa = minimize(&projected.fa)?;

    *a = projected;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Direct coverage of [`quantify`] IN THIS CRATE (adversarial-review finding: after
    //! U6's move, `wr-core` — the crate that now owns this trust-critical logic, and the
    //! one a future `NumberSystem` unit will call directly — had zero tests of its own;
    //! every existing test reached it only indirectly through `wr_logic::quantify::exists`).
    //! These are a small, direct smoke suite calling [`quantify`] with a bare
    //! `BTreeSet<String>`, exactly the shape `NumberSystem` will use (no formula/AST
    //! layer involved) — not a duplicate of `wr_logic::quantify`'s full 15-test suite,
    //! which stays the authoritative regression suite for this pipeline's edge cases.
    use super::*;
    use crate::fa::Fa;

    fn labels(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// Two tracks `y`,`x` over `{0,1}`; accepts exactly `(y=0,x=1)` at position 0 (a
    /// single-symbol word). `∃y` should yield "exists a `y` digit making `(y,x)`
    /// accepted" = "`x` = 1", closed under leading zeros by `quantify`'s own tail step.
    fn two_track_y_x() -> Automaton {
        let sym = |a: &Automaton, y: i32, x: i32| a.encode(&[y, x]);
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
            vec!["y".to_string(), "x".to_string()],
            vec![Some(true), Some(true)],
        );
        let edge = sym(&a, 0, 1);
        a.fa.d[0].insert(edge, vec![1]);
        a
    }

    #[test]
    fn quantify_called_directly_with_a_bare_label_set_projects_and_fixes_zeros() {
        let mut a = two_track_y_x();
        quantify(&mut a, &labels(&["y"])).unwrap();

        assert_eq!(a.label, vec!["x".to_string()]);
        let word = |a: &Automaton, digits: &[i32]| -> Vec<i32> {
            digits.iter().map(|&d| a.encode(&[d])).collect()
        };
        assert!(a.fa.accepts_word(&word(&a, &[1])), "\"1\" must be accepted");
        assert!(
            a.fa.accepts_word(&word(&a, &[0, 1])),
            "leading zeros must be absorbed"
        );
        assert!(
            !a.fa.accepts_word(&word(&a, &[0])),
            "\"0\" alone must be rejected"
        );
    }

    #[test]
    fn quantify_with_an_empty_label_set_still_runs_the_zero_fixup() {
        // Same faithfully-ported quirk `wr_logic::quantify`'s equivalent test pins:
        // `quantify` consults `determineMsd`/runs the fixup unconditionally, even when
        // nothing was quantified.
        let mut a = two_track_y_x();
        let word = |a: &Automaton, pairs: &[(i32, i32)]| -> Vec<i32> {
            pairs.iter().map(|&(y, x)| a.encode(&[y, x])).collect()
        };
        assert!(!a.fa.accepts_word(&word(&a, &[(0, 0), (0, 1)])));

        quantify(&mut a, &BTreeSet::new()).unwrap();

        assert_eq!(a.label, vec!["y".to_string(), "x".to_string()]);
        assert!(
            a.fa.accepts_word(&word(&a, &[(0, 0), (0, 1)])),
            "the fixup ran despite nothing being quantified"
        );
    }

    #[test]
    fn quantifying_every_track_collapses_to_a_true_false_automaton() {
        // `AutomatonQuantification.java:58-65` (U0). `two_track_y_x` accepts the single
        // symbol `(y=0, x=1)`, so its language is non-empty and `∃y ∃x φ` is TRUE.
        let mut a = two_track_y_x();
        assert!(!a.is_empty(), "sanity");
        quantify(&mut a, &labels(&["y", "x"])).unwrap();
        assert!(a.is_true_false_automaton() && a.is_true_automaton());
        assert!(a.alphabet.is_empty() && a.label.is_empty() && a.msd.is_empty());
        assert_eq!(
            a.fa.q, 2,
            "`Automaton.clear()` leaves `q` stale -- FA.clear() never resets it"
        );

        // The FALSE side. This is the case that catches the two easiest ways to get the
        // ported sequence wrong: negating `isEmpty` the wrong way, or evaluating it
        // AFTER setting the flag (at which point `Automaton::is_empty` takes its own
        // trivial branch and would answer "empty" unconditionally).
        let mut a = two_track_y_x();
        a.fa.o = vec![0, 0];
        assert!(a.is_empty(), "sanity");
        quantify(&mut a, &labels(&["y", "x"])).unwrap();
        assert!(a.is_true_false_automaton() && !a.is_true_automaton());
    }

    #[test]
    fn quantifying_a_repeated_label_is_not_the_all_tracks_case() {
        // Java compares the deduplicated label SET's size against the TRACK count
        // (`:60`), so a two-track automaton whose tracks share one label takes the
        // ordinary projection path even though `{"y"}` "covers" both tracks. Pinned
        // because `labels.len() == a.alphabet.len()` is easy to misread as
        // "every track is named".
        let mut a = two_track_y_x();
        a.label = vec!["y".to_string(), "y".to_string()];
        quantify(&mut a, &labels(&["y"])).unwrap();
        assert!(
            !a.is_true_false_automaton(),
            "one label, two tracks -> ordinary projection"
        );
        assert_eq!(a.label, vec!["y".to_string()], "one track survives");
    }

    // ------------------------------------------------- the lsd branch (Phase 3b, L1)

    /// Three tracks' worth of structure in two: `(y, x)` over `{0,1}`, accepting exactly
    /// the words `x = 1 0^k` with `k >= 1` (for either value of `y` on the first symbol).
    /// The two `y`-values on that first symbol collapse onto one projected symbol, so
    /// `∃y` really does exercise the projection, and the projected language `1 0+` is
    /// **not** closed under trailing zeros at its left end — `"1"` alone is rejected —
    /// which is exactly what [`crate::logicalops::fix_trailing_zeros_problem`] repairs.
    fn two_track_needing_the_trailing_fixup(msd: bool) -> Automaton {
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
            vec!["y".to_string(), "x".to_string()],
            vec![Some(msd), Some(msd)],
        );
        let s = |a: &Automaton, y: i32, x: i32| a.encode(&[y, x]);
        let (y0x1, y1x1, y0x0) = (s(&a, 0, 1), s(&a, 1, 1), s(&a, 0, 0));
        a.fa.d[0].insert(y0x1, vec![1]);
        a.fa.d[0].insert(y1x1, vec![1]);
        a.fa.d[1].insert(y0x0, vec![2]);
        a.fa.d[2].insert(y0x0, vec![2]);
        a
    }

    /// The lsd arm of `quantify`'s tail (`AutomatonQuantification.java:46`), wired up in
    /// Phase 3b's L1 — it used to be a hard `UnsupportedLsdFixup` error, so this test
    /// replaces a pinned rejection with a pinned *result*.
    ///
    /// The same automaton is quantified twice, once declared msd and once lsd, and the
    /// two answers must DIFFER in the specific way the two fixups differ. That is what
    /// makes this more than "it no longer errors": a branch that ran
    /// `fix_leading_zeros_problem` on both directions, or ran neither, fails here.
    #[test]
    fn quantify_on_an_lsd_automaton_runs_the_trailing_zero_fixup() {
        let word = |a: &Automaton, digits: &[i32]| -> Vec<i32> {
            digits.iter().map(|&d| a.encode(&[d])).collect()
        };

        let mut lsd = two_track_needing_the_trailing_fixup(false);
        quantify(&mut lsd, &labels(&["y"])).unwrap();
        assert_eq!(lsd.label, vec!["x".to_string()]);
        // `1 0+` right-quotiented by `0*` is `1 0*`: the trailing zeros became optional.
        assert!(
            lsd.fa.accepts_word(&word(&lsd, &[1])),
            "the trailing-zero fixup must make \"1\" accepted"
        );
        assert!(lsd.fa.accepts_word(&word(&lsd, &[1, 0])));
        assert!(lsd.fa.accepts_word(&word(&lsd, &[1, 0, 0])));
        assert!(!lsd.fa.accepts_word(&word(&lsd, &[])));
        assert!(!lsd.fa.accepts_word(&word(&lsd, &[0])));
        assert!(!lsd.fa.accepts_word(&word(&lsd, &[0, 1])));
        assert!(!lsd.fa.accepts_word(&word(&lsd, &[1, 1])));

        // The msd control on the identical transition table. `fixLeadingZerosProblem`
        // instead left-quotients by `0*` and closes under PREPENDING zeros, so "1" stays
        // rejected while "01 0" becomes accepted -- the mirror image of the assertions
        // above, and impossible to satisfy with the same fixup.
        let mut msd = two_track_needing_the_trailing_fixup(true);
        quantify(&mut msd, &labels(&["y"])).unwrap();
        assert!(
            !msd.fa.accepts_word(&word(&msd, &[1])),
            "the LEADING-zero fixup must not make \"1\" accepted"
        );
        assert!(msd.fa.accepts_word(&word(&msd, &[1, 0])));
        assert!(msd.fa.accepts_word(&word(&msd, &[0, 1, 0])));
        assert!(!msd.fa.accepts_word(&word(&msd, &[0, 1])));
    }

    /// The one shape that reaches the lsd arm with a degenerate automaton:
    /// `quantify_helper`'s `labels.is_empty()` early return fires *before* its
    /// `a.fa.q == 0` one, so a 0-state automaton with an lsd track lands in
    /// `fix_trailing_zeros_problem` — which, unlike `fix_leading_zeros_problem`, has no
    /// `fa.q == 0` guard of its own. It needs none (its helper only ever indexes inside
    /// `for q in 0..fa.q`), and this pins that: no panic, no change.
    #[test]
    fn quantify_on_a_zero_state_lsd_automaton_is_a_silent_noop() {
        let empty = || {
            Automaton::new(
                Fa {
                    true_false: None,
                    q0: 0,
                    q: 0,
                    alphabet_size: 4,
                    o: vec![],
                    d: vec![],
                },
                vec![vec![0, 1], vec![0, 1]],
                vec!["y".to_string(), "x".to_string()],
                vec![Some(false), Some(false)],
            )
        };

        // Via the empty-label-set early return (the only path that skips the `q == 0`
        // guard in `quantify_helper`).
        let mut a = empty();
        quantify(&mut a, &BTreeSet::new()).unwrap();
        assert_eq!(a.fa.q, 0);
        assert_eq!(a.label, vec!["y".to_string(), "x".to_string()]);

        // And via the `q == 0` guard itself, with a real label to quantify.
        let mut a = empty();
        quantify(&mut a, &labels(&["y"])).unwrap();
        assert_eq!(a.fa.q, 0);
    }

    #[test]
    fn quantify_rejects_an_unknown_label() {
        let mut a = two_track_y_x();
        assert_eq!(
            quantify(&mut a, &labels(&["z"])),
            Err(QuantifyError::NotFreeVariable("z".to_string()))
        );
    }

    // ------------------------------------------------- Tier-4: quantifier duality (U9)
    //
    // DESIGN.md §5 Tier 4 traceability (this table is U9's audit result, not a
    // restatement of coverage this unit wrote -- see U9's final report for the full
    // per-property assessment):
    //   * `L(minimize(A)) == L(A)`, idempotence        -> wr-core/src/minimize.rs
    //     (`minimize_preserves_language`, `minimize_is_idempotent`)
    //   * `L(determinize(A)) == L(A)`, Brzozowski
    //     vs. direct minimize                           -> wr-core/src/determinize.rs
    //     (`determinize_preserves_language`,
    //     `brzozowski_yields_the_minimal_dfa_cross_checked_against_direct_minimize`)
    //   * De Morgan across product                       -> wr-core/src/logicalops.rs
    //     (`de_morgan_not_and_equals_or_of_nots`, `de_morgan_not_or_equals_and_of_nots`)
    //   * adder/comparator/msd-lsd number-system laws     -> wr-core/src/numsys.rs
    //     (`addition_automaton_computes_real_addition`,
    //     `comparison_automata_agree_with_the_integer_order`,
    //     `msd_and_lsd_agree_after_reversal`)
    //   * projection soundness (∃)                        -> wr-logic/src/quantify.rs
    //     (`quantify_matches_brute_force_existential`)
    //   * quantifier duality: `∀y φ ≡ ¬∃y ¬φ`              -> HERE (U9):
    //     `forall_via_not_exists_not_matches_brute_force_universal`, below. NOTE
    //     (adversarial-review finding, both reviewers independently confirmed by
    //     exhaustive sweep): this property is checked only in the regime where
    //     `quantify`'s internal leading-zero fixup is a no-op. The regime where the
    //     fixup does real work, COMPOSED WITH negation, is NOT covered by any test in
    //     this repository — see the section doc comment below for the honest account
    //     (an earlier version of this comment claimed that gap didn't cost anything;
    //     it does, and the gap is real).
    //
    // (The cross-oracle "Moore `minimize` agrees with ported Valmari `minimize`"
    // property is a separate unit, U9a's responsibility -- not built here.)
    //
    // # Why this test lives here, in `wr-core`, not `wr-logic`
    //
    // `∀y φ ≡ ¬∃y ¬φ` is stated at the logic layer, but every primitive it composes --
    // [`quantify`] (this module) and [`crate::logicalops::not`] -- is a `wr-core`
    // primitive; `wr_logic::quantify::exists` is a thin one-line delegation to
    // [`quantify`] (see this module's own docs) that adds nothing able to change this
    // property. There is no formula/AST/`Predicate` layer yet for a general `forall`
    // command to live in (that is explicitly out of scope for this unit, deferred to a
    // future `wr-cli`/`wr-logic` formula-evaluation unit) -- this test exists solely to
    // pin the algebraic law using exactly the primitives that exist today. Co-locating
    // it with `quantify`'s own direct-coverage smoke suite (added by U6, above) follows
    // this codebase's established convention of co-locating Tier-4 property tests with
    // the module under test (see `minimize.rs`, `determinize.rs`, `logicalops.rs`,
    // `numsys.rs`, all audited above).
    //
    // # The ground truth, and why it is independent (not circular)
    //
    // `brute_force_forall_y` below decides "for every `y`-word of the same length as
    // `x_digits`, does the ORIGINAL (pre-negation) automaton `phi` accept?" by exhaustive
    // enumeration using `Fa::accepts_word` on `phi` itself -- it never calls `quantify`
    // or `not`. Comparing the real `not`+`quantify`+`not` composition's answer against
    // this checks the composition against direct automaton semantics, not a restatement
    // of the composition in different words (checking "¬∃¬ agrees with itself" would
    // pass even if `not` and `quantify` shared a compensating bug -- this cannot).
    //
    // # Neutralizing the leading-zero fixup — what this buys, and what it honestly
    // # does NOT cover (adversarial-review correction: an earlier version of this
    // # section overclaimed both halves of this argument)
    //
    // `quantify`'s internal `fix_leading_zeros_problem` step exists to make the
    // quantified language closed under leading zeros. To keep this property's ground
    // truth (`brute_force_forall_y`) simple -- pure per-length ∃/∀ semantics, no
    // zero-closure logic of its own to get independently right -- `phi` below is built
    // so the fixup is provably a no-op on it, via `arb_self_looping_zero_two_track`,
    // which pins state `q0` (only) to self-loop on the reduced "x = 0" symbol, for both
    // values of `y` -- the SAME `pin_zero_class` shape `wr_logic::quantify`'s own
    // `quantify_matches_brute_force_existential` already uses, not a stronger
    // whole-automaton version. **A stronger, every-state pin was tried in an earlier
    // version of this test and REMOVED**: two independent adversarial reviewers
    // exhaustively verified the weaker `q0`-only pin is already sufficient (a `q0`
    // self-loop is preserved by ANY language-preserving quotient/subset-construction,
    // since Nerode-equivalence classes and reachable-metastate unions both map a
    // self-looping representative to itself) and that the every-state version
    // needlessly killed genuine random structure on roughly half the generated cases
    // (any state whose "x=1" transitions also happened not to matter).
    //
    // With `q0` alone pinned: `not(phi)`'s `q0`-representative block still self-loops
    // on "x=0" (accepting-ness of that block may flip, but the self-loop survives
    // `not`'s `totalize`/`just_minimize` for the reason above), so after `quantify`'s
    // projection the reduced automaton's `q0` self-loops on the projected "x=0"
    // symbol, its zero-closure is exactly `{q0}`, and `fix_leading_zeros_problem`
    // changes nothing. (One honestly-checked exception, found by adversarial review:
    // when `L(¬phi) = ∅`, `minimize` can strip ALL transitions including that self-loop
    // -- but the fixup is STILL neutral there too, because forcing a self-loop onto a
    // language that is already `∅` cannot add anything. Both cases end at "the fixup
    // doesn't change this specific composition's answer", just via two different
    // arguments, not one -- an earlier version of this comment asserted only the
    // first case's mechanism as if it were universal, which was false on exactly the
    // `L(¬phi) = ∅` shape.)
    //
    // **What this does NOT cover, and no other test in this repository covers either
    // (adversarial-review finding, confirmed by exhaustive sweep on the UNPINNED
    // generator: ~4% of cases disagree with pure per-length ∀, and the fixup's actual
    // "zero-closed" semantics still disagrees with a smaller but nonzero fraction):**
    // the regime where the leading-zero fixup does real, non-trivial work IN
    // COMPOSITION WITH negation. `wr_logic::quantify`'s own closure test
    // (`quantified_language_is_closed_under_leading_zeros`) only checks
    // self-consistency (`L(w) == L(0w)`) against `quantify` alone, never against an
    // independent oracle, and never composed with `not`. That is a genuine, currently
    // open gap in this project's Tier-4 coverage, not a redundant case -- flagging it
    // here plainly rather than claiming (as an earlier version of this comment did)
    // that covering it would add "no extra bug-catching power". A future unit wanting
    // to close it would need an oracle that computes the fixup's own zero-closure
    // semantics independently (a small BFS over `phi` directly, not reusing
    // `fix_leading_zeros_problem`/`quantify`), generated over the UNPINNED automaton
    // family.
    //
    // `phi` is trimmed at construction time so the FIRST of the two explicit `not`
    // calls below cannot hit `docs/WALNUT-BUGS.md` WB-001 (`not`'s `just_minimize`
    // calls `minimize` directly with no trim of its own -- the same reasoning
    // `logicalops.rs`'s De Morgan properties document for their own `not` calls).
    // WB-001's actual precondition is every state forward-reachable from `q0` (see
    // `minimize.rs`'s "q0 aliasing quirk" section). The SECOND `not` call (on
    // `exists_not_phi`, `quantify`'s output) is safe for a DIFFERENT, specific reason,
    // not just "because `quantify` trims internally": `quantify`'s own
    // `subset_construction` call never materializes an unreachable metastate (every
    // metastate it emits is discovered by BFS from the seed), so every state `minimize`
    // sees there is already forward-reachable, and Valmari's quotient cannot orphan a
    // reachable block. If `subset_construction` ever changed to pre-allocate or
    // totalize a dead/unreachable metastate, this argument (and this test's safety
    // from WB-001) would need re-deriving, not just re-asserting.

    use proptest::prelude::*;

    /// A 2-track (`y`, `x`) automaton where ONLY `q0` is forced to deterministically
    /// self-loop on the symbol "x = 0" (for both values of `y`) -- the same
    /// `pin_zero_class` shape `wr_logic::quantify`'s `arb_two_track` already uses for
    /// its own ∃-only property, not a stronger whole-automaton version (see the
    /// section doc comment above for why the weaker pin is enough). Every OTHER
    /// state's "x = 0" transitions, and every state's "x = 1" transitions, are free --
    /// genuine random structure everywhere except the one pinned edge. Trimmed at the
    /// end so every state is reachable from `q0` (see the section doc comment's WB-001
    /// argument).
    fn arb_self_looping_zero_two_track(q_max: usize) -> impl Strategy<Value = Automaton> {
        (1..=q_max).prop_flat_map(|q| {
            let o = prop::collection::vec(0i32..=1, q);
            // Free destinations for every (state, symbol) pair EXCEPT q0's two
            // "x = 0" entries, which are pinned below.
            let dest_y0_x0 = prop::collection::vec(0..q, q);
            let dest_y1_x0 = prop::collection::vec(0..q, q);
            let dest_y0_x1 = prop::collection::vec(0..q, q);
            let dest_y1_x1 = prop::collection::vec(0..q, q);
            (o, dest_y0_x0, dest_y1_x0, dest_y0_x1, dest_y1_x1).prop_map(
                move |(o, dest_y0_x0, dest_y1_x0, dest_y0_x1, dest_y1_x1)| {
                    let fa = Fa {
                        true_false: None,
                        q0: 0,
                        q,
                        alphabet_size: 4,
                        o,
                        d: vec![BTreeMap::new(); q],
                    };
                    let mut a = Automaton::new(
                        fa,
                        vec![vec![0, 1], vec![0, 1]],
                        vec!["y".to_string(), "x".to_string()],
                        vec![Some(true), Some(true)],
                    );
                    let zero_y0 = a.encode(&[0, 0]);
                    let zero_y1 = a.encode(&[1, 0]);
                    let one_y0 = a.encode(&[0, 1]);
                    let one_y1 = a.encode(&[1, 1]);
                    for s in 0..q {
                        // Free everywhere...
                        a.fa.d[s].insert(zero_y0, vec![dest_y0_x0[s]]);
                        a.fa.d[s].insert(zero_y1, vec![dest_y1_x0[s]]);
                        a.fa.d[s].insert(one_y0, vec![dest_y0_x1[s]]);
                        a.fa.d[s].insert(one_y1, vec![dest_y1_x1[s]]);
                    }
                    // ...except q0's two "x = 0" entries, forced to self-loop: the
                    // one invariant this generator exists to guarantee.
                    a.fa.d[0].insert(zero_y0, vec![0]);
                    a.fa.d[0].insert(zero_y1, vec![0]);
                    a.fa = trim(&a.fa);
                    a
                },
            )
        })
    }

    /// Independent ground truth for `∀y φ(x, y)` at a fixed word length: does `phi`
    /// (the ORIGINAL, un-negated automaton) accept `(y, x)` for every one of the
    /// `2^|x_digits|` candidate `y`-words of the same length? Uses only
    /// `Fa::accepts_word`/`Automaton::encode` on `phi` directly -- never `quantify` or
    /// `not` -- so it is a genuine oracle, the universal analogue of
    /// `wr_logic::quantify`'s `brute_force_exists_y`.
    fn brute_force_forall_y(phi: &Automaton, x_digits: &[i32]) -> bool {
        let n = x_digits.len();
        for mask in 0..(1u32 << n) {
            let word: Vec<i32> = (0..n)
                .map(|i| {
                    let y = ((mask >> i) & 1) as i32;
                    phi.encode(&[y, x_digits[i]])
                })
                .collect();
            if !phi.fa.accepts_word(&word) {
                return false;
            }
        }
        true
    }

    proptest! {
        /// Tier-4 (DESIGN.md §5): quantifier duality, `∀y φ ≡ ¬∃y ¬φ`, built from the
        /// real [`crate::logicalops::not`] and this module's [`quantify`], checked
        /// against `brute_force_forall_y`'s independent ground truth. See the section
        /// doc comment above for the full argument: why the leading-zero fixup is
        /// provably a no-op on this generator (and, honestly, what regime that leaves
        /// uncovered), and why the ground truth is not circular. If `not` or `quantify`
        /// had a plausible subtle bug (e.g. `not` skipping its `totalize` step, or
        /// `quantify` unioning the wrong destination sets, or negating in the wrong
        /// place), it would show up as a mismatch between `actual` and this exhaustive
        /// `phi`-level computation. Default case count (no `proptest_config` override --
        /// an earlier version of this test capped it at 32, well below this crate's
        /// established convention for a composed property, with no resource
        /// justification; nothing here is expensive enough to need one).
        #[test]
        fn forall_via_not_exists_not_matches_brute_force_universal(
            phi in arb_self_looping_zero_two_track(4),
            x_digits in prop::collection::vec(0i32..2, 0..4),
        ) {
            let not_phi = crate::logicalops::not(phi.as_dfa());
            let mut exists_not_phi = not_phi.into_automaton();
            quantify(&mut exists_not_phi, &labels(&["y"])).unwrap();
            // Structural sanity: the quantified track must actually be gone (a
            // `quantify` that silently no-op'd on projection would otherwise be
            // compared via a mis-encoded word instead of failing loudly).
            prop_assert_eq!(&exists_not_phi.label, &vec!["x".to_string()]);
            let forall_phi = crate::logicalops::not(exists_not_phi.as_dfa());

            let word: Vec<i32> = x_digits
                .iter()
                .map(|&d| forall_phi.automaton().encode(&[d]))
                .collect();
            let actual = forall_phi.automaton().fa.accepts_word(&word);
            let expected = brute_force_forall_y(&phi, &x_digits);

            prop_assert_eq!(actual, expected, "mismatch on x = {:?}", x_digits);
        }
    }
}
