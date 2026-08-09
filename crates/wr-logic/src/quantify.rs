// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Existential quantification (∃-elimination) — the decision procedure's crux.
//!
//! Ports `Automata/AutomatonQuantification.java`'s `quantify`/`quantifyHelper` plus the
//! msd half of its follow-up fixup, `Automata/AutomatonLogicalOps.java`'s
//! `fixLeadingZerosProblem`/`zeroReachableStates`.
//!
//! The pipeline, exactly as Java sequences it:
//!
//! 1. **Project.** Decode every transition symbol into its per-track digit tuple, delete
//!    the quantified tracks from the tuple *and* from the alphabet/label/msd lists,
//!    re-encode the reduced tuple. Distinct old symbols routinely collapse onto the same
//!    new symbol — that collapse is precisely what introduces nondeterminism, and is the
//!    whole mathematical content of ∃-elimination.
//! 2. **Determinize + minimize** (`Automaton.determinizeAndMinimize()`, the no-arg
//!    overload).
//! 3. **Fix leading zeros** (`fixLeadingZerosProblem`), so the result accepts `0*x`
//!    whenever it used to accept `x` — necessary because projecting a track away can
//!    strand a representation behind a run that only existed for the quantified track's
//!    longer representation.
//!
//! # Deliberate divergences from a literal transliteration
//!
//! * **Destination merging is set union, not Java's order-preserving
//!   `addAllWithoutRepetition`.** When two old symbols collapse onto one new symbol their
//!   destination lists are merged; Java appends the second list's new elements to the
//!   first, preserving insertion order. Order of an NFA destination list is not
//!   observable in any language-level sense, and `CLAUDE.md`'s prime directive #1 is to
//!   compare by language equivalence, never by structure — so a [`BTreeSet`] union is
//!   used, which is simpler and obviously dedup-correct.
//! * **Step 2 always trims.** Java's no-arg `determinizeAndMinimize()` trims *only* when
//!   the freshly-rebuilt table is not already deterministic; this port trims
//!   unconditionally, so the two differ exactly on the (rare) projection that happens to
//!   come out deterministic. Trimming is always language-preserving, so the divergence is
//!   invisible to the correctness bar, and it is the *safer* choice: it establishes
//!   [`wr_core::minimize::minimize`]'s documented precondition (every state reachable from
//!   `q0`; violate it and the "q0 aliasing quirk" silently flips the language) rather than
//!   leaving it resting on the separate argument that subset construction from `{q0}`
//!   already emits only reachable states. It also shrinks the input to the exponential
//!   subset construction.
//! * **`TRUE_FALSE_AUTOMATON` is not modeled.** `wr_core::automaton::Automaton` has no
//!   such variant (Phase-1 spike scope, see its module docs), so the Java branch that
//!   collapses an all-tracks-quantified automaton to a true/false automaton becomes a
//!   hard [`QuantifyError::AllTracksQuantified`] instead of a silently-wrong encoding.
//! * **The lsd fixup is out of scope.** `fixTrailingZerosProblem` is genuinely different
//!   logic (it mutates only the final-state set and re-minimizes, without
//!   re-determinizing) and no lsd numeration system exists in this crate yet, so an lsd
//!   automaton yields [`QuantifyError::UnsupportedLsdFixup`] rather than a wrong answer.
//!
//! # Faithfully-ported quirks (not divergences)
//!
//! * `quantify` runs the leading-zero fixup **even when `quantifyHelper` short-circuits**
//!   (empty label set, or an automaton with no labels at all): Java's `quantify` calls
//!   the helper for effect and then unconditionally consults `determineMsd`. So
//!   `exists(a, &{})` is *not* a no-op — see `empty_label_set_still_runs_the_zero_fixup`.
//! * A label that appears on more than one track is resolved by `List.indexOf`, i.e. only
//!   its **first** occurrence is quantified away.
//! * `zeroReachableStates` mutates the automaton it inspects — see
//!   [`zero_reachable_states`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use wr_core::automaton::Automaton;
use wr_core::determinize::subset_construction;
use wr_core::fa::Fa;
use wr_core::minimize::{minimize, MinimizeError};
use wr_core::trim::trim;

#[derive(Debug, PartialEq, Eq)]
pub enum QuantifyError {
    /// A requested label is not one of the automaton's track labels. Ports
    /// `WalnutException.notFreeVariable` (thrown from
    /// `AutomatonQuantification.validateLabels`).
    NotFreeVariable(String),
    /// Every track was quantified away. Java collapses this to a TRUE/FALSE automaton
    /// (true iff the language was non-empty); this crate does not model that variant, so
    /// the out-of-scope precondition is reported instead of guessed at.
    AllTracksQuantified,
    /// The surviving tracks are lsd, which would need `fixTrailingZerosProblem` — not
    /// ported (see module docs).
    UnsupportedLsdFixup,
    /// Propagated from [`wr_core::minimize::minimize`]. Both variants should be
    /// unreachable here (subset construction always yields a deterministic automaton),
    /// but they are surfaced rather than `unwrap`ped: `PORTING.md`'s error-mapping rule
    /// is that a "can't happen" is still a `Result`, never a panic or a `debug_assert!`.
    Minimize(MinimizeError),
}

impl From<MinimizeError> for QuantifyError {
    fn from(e: MinimizeError) -> Self {
        QuantifyError::Minimize(e)
    }
}

/// Existentially quantifies `labels` out of `a`, in place.
///
/// Ports `AutomatonQuantification.quantify(Automaton, Set<String>)`: project the labelled
/// tracks away, determinize + minimize, then apply the numeration-system-dependent
/// leading-zero fixup. `labels` must be a subset of `a.label` (else
/// [`QuantifyError::NotFreeVariable`]) and must not name *every* track (else
/// [`QuantifyError::AllTracksQuantified`]).
///
/// On success `a.alphabet` / `a.label` / `a.msd` have had the quantified tracks removed,
/// `a.fa` is a minimal (generally *partial* — `minimize` drops non-co-reachable states)
/// DFA over the reduced alphabet, and state numbering bears no relation to Walnut's.
pub fn exists(a: &mut Automaton, labels: &BTreeSet<String>) -> Result<(), QuantifyError> {
    quantify_helper(a, labels)?;

    // `quantify`'s tail: consult the surviving tracks' numeration direction. Note this
    // runs even when `quantify_helper` short-circuited — a faithfully-ported quirk, see
    // the module docs.
    match determine_msd(&a.msd) {
        None => Ok(()),
        Some(true) => fix_leading_zeros(a),
        Some(false) => Err(QuantifyError::UnsupportedLsdFixup),
    }
}

/// Ports `NumberSystem.determineMsd(List<NumberSystem>)`: `None` ("skip the fixup") if any
/// track is non-arithmetic or if the arithmetic tracks disagree on direction; otherwise
/// the shared direction.
///
/// Java's loop leaves `isMsd = true` untouched for an *empty* list, so zero tracks
/// defaults to msd. This IS reachable through [`exists`] — not by quantifying away
/// every track (`quantify_helper` rejects that as [`QuantifyError::AllTracksQuantified`]
/// before ever reaching this function), but by calling [`exists`] on an automaton that
/// already has zero tracks: `quantify_helper`'s `a.label.is_empty()` early return leaves
/// `a.msd` empty and unchanged, and `exists` still unconditionally consults
/// `determine_msd` afterward (see the module docs on this faithfully-ported quirk). See
/// `empty_label_set_still_runs_the_zero_fixup` for the analogous (but importantly
/// different — that test starts from a *populated* automaton) case.
fn determine_msd(msd: &[Option<bool>]) -> Option<bool> {
    let mut is_msd = true;
    let mut seen_any = false;
    for entry in msd {
        let v = (*entry)?;
        if seen_any && v != is_msd {
            return None;
        }
        is_msd = v;
        seen_any = true;
    }
    Some(is_msd)
}

/// Ports `AutomatonQuantification.quantifyHelper`: the projection itself, followed by
/// determinize + minimize. Leaves `a` untouched on any `Err`.
fn quantify_helper(a: &mut Automaton, labels: &BTreeSet<String>) -> Result<(), QuantifyError> {
    // Java: `if (labelsToQuantify.isEmpty() || A.getLabel() == null ||
    // A.getLabel().isEmpty()) return;` — note this precedes `validateLabels`, so asking to
    // quantify a name out of a *label-less* automaton is silently accepted, not an error.
    if labels.is_empty() || a.label.is_empty() {
        return Ok(());
    }

    // `validateLabels`.
    for l in labels {
        if !a.label.contains(l) {
            return Err(QuantifyError::NotFreeVariable(l.clone()));
        }
    }

    // Java: `if (labelsToQuantify.size() == A.richAlphabet.getA().size())` — every track
    // quantified. See `QuantifyError::AllTracksQuantified`.
    if labels.len() == a.alphabet.len() {
        return Err(QuantifyError::AllTracksQuantified);
    }

    // A 0-state automaton (labels present, but no states — legal to construct via this
    // crate's API even though a real Walnut `Automaton` never reaches it) has nothing
    // for the projection to do. Without this guard, `trim`/`subset_construction` below
    // would index the stale `q0` into an empty `fa.d` and panic — `fix_leading_zeros`
    // has the identical guard for the identical reason; this mirrors it for parity.
    if a.fa.q == 0 {
        return Ok(());
    }

    // `A.getLabel().indexOf(l)` for each label: first occurrence only.
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

    // `allInputs`: decode every OLD symbol before the alphabet shrinks.
    let old_alphabet_size = a.fa.alphabet_size;
    let all_inputs: Vec<Vec<i32>> = (0..old_alphabet_size as i32).map(|s| a.decode(s)).collect();

    // `removeIndices` on A / NS / label — order of the survivors is preserved.
    let new_alphabet: Vec<Vec<i32>> = kept.iter().map(|&i| a.alphabet[i].clone()).collect();
    let new_label: Vec<String> = kept.iter().map(|&i| a.label[i].clone()).collect();
    let new_msd: Vec<Option<bool>> = kept.iter().map(|&i| a.msd[i]).collect();
    let new_alphabet_size: usize = new_alphabet.iter().map(|t| t.len()).product();

    // Building the reduced `Automaton` here (rather than mutating `a` in place) is what
    // rebuilds the mixed-radix encoder — Java's `richAlphabet.setEncoder(null)` +
    // `determineAlphabetSize()`, which force a lazy recompute on the next `encode`.
    let mut projected = Automaton::new(a.fa.clone(), new_alphabet, new_label, new_msd);
    projected.fa.alphabet_size = new_alphabet_size;

    // `permutation[old] = new`: re-encode each decoded tuple minus the quantified tracks.
    // Many old symbols map to one new symbol — that collapse is the projection.
    let permutation: Vec<i32> = all_inputs
        .iter()
        .map(|digits| {
            let reduced: Vec<i32> = kept.iter().map(|&i| digits[i]).collect();
            projected.encode(&reduced)
        })
        .collect();

    // Rebuild the transition table under the new symbol ids, unioning the destinations of
    // old symbols that collapsed together (see the module docs on why union, not Java's
    // order-preserving append). Indexing `permutation` panics on an out-of-range symbol,
    // exactly as Java's `permutation.get(...)` throws — a corrupt table is a caller bug.
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

    // `A.determinizeAndMinimize()` — with the unconditional trim described in the module
    // docs (Java trims only when the rebuilt table is nondeterministic).
    let trimmed = trim(&projected.fa);
    let initial: BTreeSet<usize> = [trimmed.q0].into_iter().collect();
    projected.fa = minimize(&subset_construction(&trimmed, &initial))?;

    *a = projected;
    Ok(())
}

/// Ports `AutomatonLogicalOps.fixLeadingZerosProblem`: make `a` accept `0*x` whenever it
/// accepted `x`, by re-running subset construction from the *set* of states reachable
/// from `q0` on `0*` instead of from `{q0}`.
///
/// Java's `determinizeAndMinimize(IntSet)` overload does **not** trim (unlike the no-arg
/// one), and neither does this — faithfully. It is safe: subset construction from a
/// metastate only ever emits states reachable from that metastate, so `minimize`'s
/// all-states-reachable precondition already holds.
fn fix_leading_zeros(a: &mut Automaton) -> Result<(), QuantifyError> {
    // Java would dereference `q0`'s transition row unconditionally; a 0-state automaton
    // has none. Nothing to fix in that degenerate case (this crate's `trim`/`minimize`
    // both pass 0-state automata through untouched too).
    if a.fa.q == 0 {
        return Ok(());
    }
    let zero = a.determine_zero();
    let initial = zero_reachable_states(&mut a.fa, zero);
    a.fa = minimize(&subset_construction(&a.fa, &initial))?;
    Ok(())
}

/// Ports `AutomatonLogicalOps.zeroReachableStates`: the states reachable from `q0` by
/// reading the all-zero symbol zero-or-more times.
///
/// **This mutates `fa`, and the mutation is the point.** Before the BFS, Java force-adds
/// a literal `(q0, zero) -> q0` self-loop to the *real* transition table if it is not
/// already there. That does not change the returned set (`q0` is added to the BFS result
/// unconditionally), but it persists — and the caller's next step, subset construction,
/// reads the mutated table. So the self-loop is Walnut's mechanism for making "one more
/// leading zero, from the very start, is a no-op" a structural invariant of the automaton
/// going forward, not merely a fact about this one computation. Replicating only the
/// returned set would silently drop that.
fn zero_reachable_states(fa: &mut Fa, zero: i32) -> BTreeSet<usize> {
    let q0 = fa.q0;
    let dests = fa.d[q0].entry(zero).or_default();
    if !dests.contains(&q0) {
        dests.push(q0);
    }

    let mut result: BTreeSet<usize> = BTreeSet::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(q0);
    while let Some(q) = queue.pop_front() {
        // Java: `if (result.add(q))` — only expand a state the first time it is seen.
        if result.insert(q) {
            if let Some(next) = fa.d[q].get(&zero) {
                for &p in next {
                    if !result.contains(&p) {
                        queue.push_back(p);
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use wr_core::numsys::less_than_msd;

    fn labels(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// Encodes a single-track word (one digit per position) through `a`'s own encoder,
    /// so tests never hand-compute symbol ids.
    fn word1(a: &Automaton, digits: &[i32]) -> Vec<i32> {
        digits.iter().map(|&d| a.encode(&[d])).collect()
    }

    /// A 2-track automaton over tracks `y` (quantified) and `x`, digits `{0, 1}` each,
    /// built from an explicit transition list of `((state, y, x), dest)`.
    fn two_track(q: usize, o: Vec<i32>, edges: &[((usize, i32, i32), usize)]) -> Automaton {
        let mut a = Automaton::new(
            Fa {
                q0: 0,
                q,
                alphabet_size: 4,
                o,
                d: vec![BTreeMap::new(); q],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["y".to_string(), "x".to_string()],
            vec![Some(true), Some(true)],
        );
        for &((from, y, x), to) in edges {
            let sym = a.encode(&[y, x]);
            a.fa.d[from].entry(sym).or_default().push(to);
        }
        a
    }

    // ---------------------------------------------------------------- spike criterion

    /// **The Phase-1 spike's exit criterion.** `∃i (i < x)` over msd base-2 must be
    /// exactly "`x` is nonzero", i.e. "the msd-first representation of `x` contains a 1".
    ///
    /// Hand-derivation of the intermediate NFA (2-track symbol = `i + 2x`): state 0
    /// self-loops on `(0,0)`=0 and `(1,1)`=3, goes to state 1 on `(0,1)`=2, and has no
    /// edge for `(1,0)`=1. Projecting `i` away maps symbols `{0,1} -> 0` and `{2,3} -> 1`,
    /// so state 0 gets `0 -> {0}` and `1 -> {1} ∪ {0}` — a genuine two-symbol collapse
    /// requiring destination merging. The result is the classic 2-state "contains a 1".
    #[test]
    fn exists_i_less_than_x_is_x_nonzero() {
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];

        exists(&mut a, &labels(&["i"])).unwrap();

        assert_eq!(a.label, vec!["x".to_string()]);
        assert_eq!(a.alphabet, vec![vec![0, 1]]);
        assert_eq!(a.msd, vec![Some(true)]);
        assert_eq!(a.fa.alphabet_size, 2);
        assert_eq!(a.fa.q, 2, "expected the minimal 'contains a 1' DFA");
        assert!(a.fa.is_deterministic());

        // x = 0 in any number of digits: rejected.
        assert!(!a.fa.accepts_word(&word1(&a, &[0])));
        assert!(!a.fa.accepts_word(&word1(&a, &[0, 0, 0])));
        // Any representation containing a 1: accepted (msd-first, leading zeros allowed).
        assert!(a.fa.accepts_word(&word1(&a, &[1])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 1, 0])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 0, 1])));
        assert!(a.fa.accepts_word(&word1(&a, &[1, 1])));
        // The empty representation denotes no value at all: rejected.
        assert!(!a.fa.accepts_word(&word1(&a, &[])));
    }

    // ------------------------------------------------------------------- error paths

    #[test]
    fn unknown_label_is_not_a_free_variable() {
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];
        assert_eq!(
            exists(&mut a, &labels(&["z"])),
            Err(QuantifyError::NotFreeVariable("z".to_string()))
        );
        // The automaton is untouched by the rejected call.
        assert_eq!(a.label, vec!["i".to_string(), "x".to_string()]);
        assert_eq!(a.fa.q, 2);
    }

    #[test]
    fn quantifying_every_track_is_out_of_scope() {
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];
        assert_eq!(
            exists(&mut a, &labels(&["i", "x"])),
            Err(QuantifyError::AllTracksQuantified)
        );
    }

    #[test]
    fn lsd_tracks_are_rejected_rather_than_mishandled() {
        let mut a = less_than_msd(2);
        a.label = vec!["i".to_string(), "x".to_string()];
        a.msd = vec![Some(false), Some(false)];
        assert_eq!(
            exists(&mut a, &labels(&["i"])),
            Err(QuantifyError::UnsupportedLsdFixup)
        );
        // The projection itself still happened (Java sequences it before the fixup).
        assert_eq!(a.label, vec!["x".to_string()]);
    }

    #[test]
    fn non_arithmetic_or_mixed_tracks_skip_the_fixup() {
        assert_eq!(determine_msd(&[Some(true), Some(true)]), Some(true));
        assert_eq!(determine_msd(&[Some(false), Some(false)]), Some(false));
        assert_eq!(determine_msd(&[Some(true), Some(false)]), None);
        assert_eq!(determine_msd(&[Some(true), None]), None);
        assert_eq!(determine_msd(&[None]), None);
        // Java's loop never runs on an empty list, leaving the `true` initializer. See
        // `determine_msd`'s docs on when this is actually reachable through `exists`
        // (an already label-less input automaton, not "quantified away every track" —
        // that path is now a hard error).
        assert_eq!(determine_msd(&[]), Some(true));
    }

    #[test]
    fn zero_state_automaton_is_a_noop_not_a_panic() {
        // Legal to construct via this crate's API even though a real Walnut Automaton
        // never reaches it. Regression test for a reviewer-found panic: quantify_helper
        // used to index the stale q0 into an empty transition table via
        // trim -> subset_construction.
        let mut a = Automaton::new(
            Fa {
                q0: 0,
                q: 0,
                alphabet_size: 4,
                o: vec![],
                d: vec![],
            },
            vec![vec![0, 1], vec![0, 1]],
            vec!["y".to_string(), "x".to_string()],
            vec![Some(true), Some(true)],
        );
        assert_eq!(exists(&mut a, &labels(&["y"])), Ok(()));
        assert_eq!(
            a.fa.q, 0,
            "left untouched, matching fix_leading_zeros's precedent"
        );
    }

    // ---------------------------------------------------- symbol collapse / merging

    /// Two old symbols collapse onto one new symbol *and* their destinations must be
    /// unioned for the language to survive. Original 2-track language (over `(y, x)`):
    /// `{(0,0)}` and `{(1,0), (·,1)}`. Projecting `y` gives state 0 the single new symbol
    /// `x = 0` with destinations `{1, 2}` — state 1 accepts immediately, state 2 needs one
    /// more `x = 1`. Keeping only the first destination loses `"01"`; keeping only the
    /// last loses `"0"`. Asserting both are accepted pins the union.
    #[test]
    fn collapsing_symbols_union_their_destinations() {
        let mut a = two_track(
            4,
            vec![0, 1, 0, 1],
            &[
                ((0, 0, 0), 1),
                ((0, 1, 0), 2),
                ((2, 0, 1), 3),
                ((2, 1, 1), 3),
            ],
        );

        exists(&mut a, &labels(&["y"])).unwrap();

        assert!(a.fa.accepts_word(&word1(&a, &[0])), "lost the first branch");
        assert!(
            a.fa.accepts_word(&word1(&a, &[0, 1])),
            "lost the second branch"
        );
        assert!(!a.fa.accepts_word(&word1(&a, &[1, 1])));
        assert!(!a.fa.accepts_word(&word1(&a, &[0, 1, 0])));
    }

    // ------------------------------------------------------------- >2-track survivors

    /// Regression coverage for the `kept`/`new_label`/`new_alphabet`/reduced-digit-tuple
    /// index bookkeeping when MORE THAN ONE track survives and the quantified track is
    /// neither first nor last. `two_track`-based tests above always leave exactly one
    /// (trivially "ordered") survivor, so a track-order bug in the general N-track case
    /// would go undetected by them.
    ///
    /// 3 tracks `a`, `b` (quantified), `c`; the only accepting transition is on the
    /// EXACT symbol `(a=1, b=0, c=0)` — deliberately asymmetric in `a` vs `c` (unlike an
    /// equality- or sum-based predicate), so a bug that silently reordered the
    /// surviving tracks to `[c, a]` instead of `[a, c]` would flip which reduced symbol
    /// is accepted, not just how many states exist.
    #[test]
    fn quantifying_a_middle_track_preserves_survivor_order() {
        let mut a = Automaton::new(
            Fa {
                q0: 0,
                q: 2,
                alphabet_size: 8,
                o: vec![0, 1],
                d: vec![BTreeMap::new(), BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1], vec![0, 1]],
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            vec![Some(true), Some(true), Some(true)],
        );
        let sym = a.encode(&[1, 0, 0]); // a=1, b=0, c=0
        a.fa.d[0].entry(sym).or_default().push(1);

        exists(&mut a, &labels(&["b"])).unwrap();

        assert_eq!(a.label, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(a.alphabet, vec![vec![0, 1], vec![0, 1]]);

        // Only (a=1, c=0) is accepted. (a=0, c=1) is the swapped pair — accepting it
        // instead is exactly the symptom a [c, a] order bug would produce.
        assert!(a.fa.accepts_word(&[a.encode(&[1, 0])]));
        assert!(!a.fa.accepts_word(&[a.encode(&[0, 1])]));
        assert!(!a.fa.accepts_word(&[a.encode(&[0, 0])]));
        assert!(!a.fa.accepts_word(&[a.encode(&[1, 1])]));
    }

    // ------------------------------------------- zeroReachableStates + its mutation

    #[test]
    fn zero_reachable_states_forces_a_q0_self_loop() {
        // q0 has no zero-transition at all going in.
        let mut fa = Fa {
            q0: 0,
            q: 2,
            alphabet_size: 2,
            o: vec![0, 1],
            d: vec![BTreeMap::from([(1, vec![1])]), BTreeMap::new()],
        };
        let reached = zero_reachable_states(&mut fa, 0);
        assert_eq!(reached, BTreeSet::from([0]));
        assert_eq!(
            fa.d[0].get(&0),
            Some(&vec![0]),
            "the (q0, zero) -> q0 self-loop must be written into the real table"
        );
    }

    #[test]
    fn zero_reachable_states_is_a_multi_step_closure_and_does_not_duplicate() {
        // 0 -0-> 1 -0-> 2, and 0 already self-loops on zero.
        let mut fa = Fa {
            q0: 0,
            q: 3,
            alphabet_size: 1,
            o: vec![0, 0, 1],
            d: vec![
                BTreeMap::from([(0, vec![0, 1])]),
                BTreeMap::from([(0, vec![2])]),
                BTreeMap::new(),
            ],
        };
        let reached = zero_reachable_states(&mut fa, 0);
        assert_eq!(reached, BTreeSet::from([0, 1, 2]), "BFS must be transitive");
        assert_eq!(
            fa.d[0].get(&0),
            Some(&vec![0, 1]),
            "an existing self-loop must not be duplicated"
        );
    }

    /// End-to-end proof that the forced self-loop reaches the *language*: the original
    /// automaton accepts exactly the one-position word `(y=0, x=1)`, and `q0` has no
    /// zero-transition whatsoever. Without the mutation, subset construction from `{q0}`
    /// would yield "exactly one digit, and it is 1" — with it, arbitrarily many leading
    /// zeros are absorbed.
    #[test]
    fn leading_zero_fixup_absorbs_leading_zeros_via_the_forced_self_loop() {
        let mut a = two_track(2, vec![0, 1], &[((0, 0, 1), 1)]);
        assert!(
            !a.fa.d[0].contains_key(&a.encode(&[0, 0])),
            "precondition: q0 has no zero-transition before quantification"
        );

        exists(&mut a, &labels(&["y"])).unwrap();

        assert!(a.fa.accepts_word(&word1(&a, &[1])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 1])));
        assert!(a.fa.accepts_word(&word1(&a, &[0, 0, 0, 1])));
        assert!(!a.fa.accepts_word(&word1(&a, &[0])));
        assert!(!a.fa.accepts_word(&word1(&a, &[1, 1])));
    }

    /// Faithfully-ported quirk: `quantify` consults `determineMsd` and runs the fixup even
    /// when `quantifyHelper` short-circuited on an empty label set, so `exists(a, &{})`
    /// still rewrites the automaton. Same 2-track automaton as above, nothing quantified:
    /// the language gains leading `(0,0)` symbols it did not have.
    #[test]
    fn empty_label_set_still_runs_the_zero_fixup() {
        let mut a = two_track(2, vec![0, 1], &[((0, 0, 1), 1)]);
        let zero = a.encode(&[0, 0]);
        let one = a.encode(&[0, 1]);
        assert!(!a.fa.accepts_word(&[zero, one]));

        exists(&mut a, &BTreeSet::new()).unwrap();

        assert_eq!(a.label, vec!["y".to_string(), "x".to_string()]);
        assert!(a.fa.accepts_word(&[one]));
        assert!(
            a.fa.accepts_word(&[zero, zero, one]),
            "the fixup ran despite nothing being quantified"
        );
    }

    // --------------------------------------------------------------- Tier-4 properties

    /// Random 2-track NFA over tracks `y`/`x` with digits `{0, 1}` (so `alphabet_size` is
    /// 4), `q0 = 0`.
    ///
    /// `pin_zero_class` forces `q0`'s two "x-digit is 0" symbols (`(0,0)` and `(1,0)`) to
    /// map to exactly `{q0}`. That makes the *projected* automaton's `(q0, zero)` row
    /// exactly `{q0}`, which makes `fixLeadingZerosProblem` provably language-neutral
    /// (the zero-closure of `q0` is `{q0}`, and the forced self-loop already exists) — so
    /// the property below can compare against a pure ∃-projection oracle with no
    /// leading-zero closure baked into it. The fixup's own behavior is covered by the
    /// hand tests above and by `quantified_language_is_closed_under_leading_zeros`, which
    /// uses the unpinned generator.
    fn arb_two_track(q_max: usize, pin_zero_class: bool) -> impl Strategy<Value = Automaton> {
        (1..=q_max).prop_flat_map(move |q| {
            let o = prop::collection::vec(0i32..=1, q);
            let table = prop::collection::vec(
                prop::collection::vec(prop::collection::vec(any::<bool>(), q), 4),
                q,
            );
            (o, table).prop_map(move |(o, table)| {
                let d: Vec<BTreeMap<i32, Vec<usize>>> = table
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
                                (!dests.is_empty()).then_some((sym as i32, dests))
                            })
                            .collect()
                    })
                    .collect();
                let mut a = Automaton::new(
                    Fa {
                        q0: 0,
                        q,
                        alphabet_size: 4,
                        o,
                        d,
                    },
                    vec![vec![0, 1], vec![0, 1]],
                    vec!["y".to_string(), "x".to_string()],
                    vec![Some(true), Some(true)],
                );
                if pin_zero_class {
                    for y in [0, 1] {
                        let sym = a.encode(&[y, 0]);
                        a.fa.d[0].insert(sym, vec![0]);
                    }
                }
                a
            })
        })
    }

    /// Independent ground truth for `∃y`: does *some* `y`-word of the same length make the
    /// ORIGINAL automaton accept? Brute force over all `2^|x|` candidate `y`-words, using
    /// only `Fa::accepts_word` and `Automaton::encode` — it never calls `exists` or any of
    /// its helpers, so it is a genuine oracle rather than a re-derivation.
    fn brute_force_exists_y(original: &Automaton, x_digits: &[i32]) -> bool {
        let n = x_digits.len();
        for mask in 0..(1u32 << n) {
            let word: Vec<i32> = (0..n)
                .map(|i| {
                    let y = ((mask >> i) & 1) as i32;
                    original.encode(&[y, x_digits[i]])
                })
                .collect();
            if original.fa.accepts_word(&word) {
                return true;
            }
        }
        false
    }

    proptest! {
        /// Tier-4 (CLAUDE.md §correctness ladder, DESIGN.md §5): ∃-elimination against an
        /// independent brute-force oracle. This is the analogue of `minimize`'s
        /// Moore-reference cross-check — the property that catches a *wrong* projection,
        /// as opposed to language-preservation properties an identity function satisfies.
        #[test]
        fn quantify_matches_brute_force_existential(
            original in arb_two_track(4, true),
            x_digits in prop::collection::vec(0i32..2, 0..4),
        ) {
            let mut a = original.clone();
            exists(&mut a, &labels(&["y"])).unwrap();

            let word: Vec<i32> = x_digits.iter().map(|&d| a.encode(&[d])).collect();
            prop_assert_eq!(
                a.fa.accepts_word(&word),
                brute_force_exists_y(&original, &x_digits),
                "mismatch on x = {:?}", x_digits
            );
        }

        /// The invariant `fixLeadingZerosProblem` exists to establish, checked on the
        /// UNPINNED generator (where the fixup genuinely does work): the quantified
        /// automaton's language is closed under adding *and* removing a leading zero.
        ///
        /// This holds because the final DFA's start metastate `Z0` satisfies
        /// `δ(Z0, zero) = Z0`: `⊆` because `Z0` is the zero-closure of `q0`, and `⊇`
        /// because every `q ∈ δ(q0, 0^b)` with `b ≥ 1` is in `δ(Z0, zero)` while `q0`
        /// itself is, thanks to the forced self-loop. Drop the mutation and the `⊇`
        /// direction fails.
        #[test]
        fn quantified_language_is_closed_under_leading_zeros(
            original in arb_two_track(4, false),
            x_digits in prop::collection::vec(0i32..2, 0..4),
        ) {
            let mut a = original;
            exists(&mut a, &labels(&["y"])).unwrap();

            let zero = a.encode(&[0]);
            let word: Vec<i32> = x_digits.iter().map(|&d| a.encode(&[d])).collect();
            let mut padded = vec![zero];
            padded.extend_from_slice(&word);

            prop_assert_eq!(a.fa.accepts_word(&word), a.fa.accepts_word(&padded));
        }

        /// Structural post-conditions of `exists`: the quantified track is gone from every
        /// parallel metadata list, the alphabet size matches the surviving tracks, and the
        /// result is a DFA (projection introduces nondeterminism; determinization must
        /// remove it again).
        #[test]
        fn quantify_leaves_a_well_formed_single_track_dfa(original in arb_two_track(4, false)) {
            let mut a = original;
            exists(&mut a, &labels(&["y"])).unwrap();

            prop_assert_eq!(a.label.len(), 1);
            prop_assert_eq!(a.label[0].as_str(), "x");
            prop_assert_eq!(a.alphabet.len(), 1);
            prop_assert_eq!(a.msd, vec![Some(true)]);
            prop_assert_eq!(a.fa.alphabet_size, 2);
            prop_assert_eq!(a.fa.d.len(), a.fa.q);
            prop_assert!(a.fa.is_deterministic());
            prop_assert!(a.fa.q0 < a.fa.q);
        }
    }
}
