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
//! 3. **Fix leading zeros** ([`crate::logicalops::fix_leading_zeros_problem`]), so the
//!    result accepts `0*x` whenever it used to accept `x` — necessary because projecting
//!    a track away can strand a representation behind a run that only existed for the
//!    quantified track's longer representation.
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
//! * **Step 2 always trims.** Java's no-arg `determinizeAndMinimize()` trims *only* when
//!   the freshly-rebuilt table is not already deterministic (`Automaton.java:385`); this
//!   port trims unconditionally, so the two differ exactly on the (rare) projection that
//!   happens to come out deterministic. Trimming is always language-preserving, so the
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
//! * **`TRUE_FALSE_AUTOMATON` is not modeled.** [`crate::automaton::Automaton`] has no
//!   such variant (see its module docs), so the Java branch that collapses an
//!   all-tracks-quantified automaton to a true/false automaton
//!   (`AutomatonQuantification.java:60-65`) becomes a hard
//!   [`QuantifyError::AllTracksQuantified`] instead of a silently-wrong encoding. The
//!   `if (A.fa.isTRUE_FALSE_AUTOMATON()) return;` guard between the helper and the
//!   fixup (`:39`) likewise has no analog.
//! * **The lsd fixup is not wired up here.** Java's `:46` calls
//!   `AutomatonLogicalOps.fixTrailingZerosProblem`, and this crate *does* now have that
//!   function ([`crate::logicalops::fix_trailing_zeros_problem`], landed in U5) — but
//!   turning the lsd branch on would change this function's observable behavior on an
//!   lsd automaton from "reject" to "compute", which is a scope decision, not a
//!   refactoring step. So the branch still yields
//!   [`QuantifyError::UnsupportedLsdFixup`], and enabling it stays an explicit,
//!   separately-reviewed change (no lsd numeration system exists in
//!   [`crate::numsys`] to exercise it against yet either).
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
use crate::determinize::subset_construction;
use crate::logicalops::fix_leading_zeros_problem;
use crate::minimize::{minimize, MinimizeError};
use crate::numsys::determine_msd;
use crate::trim::trim;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, PartialEq, Eq)]
pub enum QuantifyError {
    /// A requested label is not one of the automaton's track labels. Ports
    /// `WalnutException.notFreeVariable` (thrown from
    /// `AutomatonQuantification.validateLabels`, `:110-116`).
    NotFreeVariable(String),
    /// Every track was quantified away. Java collapses this to a TRUE/FALSE automaton
    /// (true iff the language was non-empty); this crate does not model that variant, so
    /// the out-of-scope precondition is reported instead of guessed at.
    AllTracksQuantified,
    /// The surviving tracks are lsd, which would need
    /// [`crate::logicalops::fix_trailing_zeros_problem`] — not wired up (see module
    /// docs).
    UnsupportedLsdFixup,
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
/// numeration-system-dependent leading-zero fixup. `labels` must be a subset of `a.label`
/// (else [`QuantifyError::NotFreeVariable`]) and must not name *every* track (else
/// [`QuantifyError::AllTracksQuantified`]).
///
/// On success `a.alphabet` / `a.label` / `a.msd` have had the quantified tracks removed,
/// `a.fa` is a minimal (generally *partial* — `minimize` drops non-co-reachable states)
/// DFA over the reduced alphabet, and state numbering bears no relation to Walnut's.
pub fn quantify(a: &mut Automaton, labels: &BTreeSet<String>) -> Result<(), QuantifyError> {
    quantify_helper(a, labels)?;

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
            fix_leading_zeros_problem(a);
            Ok(())
        }
        Some(false) => Err(QuantifyError::UnsupportedLsdFixup),
    }
}

/// Ports `AutomatonQuantification.quantifyHelper` (`:49-108`): the projection itself,
/// followed by determinize + minimize. Leaves `a` untouched on any `Err`.
fn quantify_helper(a: &mut Automaton, labels: &BTreeSet<String>) -> Result<(), QuantifyError> {
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

    // Java: `if (labelsToQuantify.size() == A.richAlphabet.getA().size())` (`:60`) —
    // every track quantified. See `QuantifyError::AllTracksQuantified`.
    if labels.len() == a.alphabet.len() {
        return Err(QuantifyError::AllTracksQuantified);
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
    let new_alphabet_size: usize = new_alphabet.iter().map(|t| t.len()).product();

    // Building the reduced `Automaton` here (rather than mutating `a` in place) is what
    // rebuilds the mixed-radix encoder — Java's `richAlphabet.setEncoder(null)` +
    // `determineAlphabetSize()` (`:76-77`), which force a lazy recompute on the next
    // `encode`.
    let mut projected = Automaton::new(a.fa.clone(), new_alphabet, new_label, new_msd);
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
    let trimmed = trim(&projected.fa);
    let initial: BTreeSet<usize> = [trimmed.q0].into_iter().collect();
    projected.fa = minimize(&subset_construction(&trimmed, &initial))?;

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
    fn quantify_rejects_an_unknown_label() {
        let mut a = two_track_y_x();
        assert_eq!(
            quantify(&mut a, &labels(&["z"])),
            Err(QuantifyError::NotFreeVariable("z".to_string()))
        );
    }
}
