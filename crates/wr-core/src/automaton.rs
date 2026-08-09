// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! A minimal multi-track automaton wrapper.
//!
//! Ports the pieces of `Automata/Automaton` + `Automata/RichAlphabet` that ∃-projection
//! structurally requires (originally written against `wr-logic`'s `quantify`, per
//! `docs/BOUNDARY-MAP.md` §4.3; the U6 architecture unit moved that primitive down into
//! [`crate::quantify`] — see its module docs for why): per-track alphabets,
//! labels, msd/lsd-ness, and the mixed-radix symbol encoder/decoder. **This is
//! deliberately NOT full Java parity** — no `NumberSystem` objects attached per track,
//! no DFAO/`combine` bookkeeping (see `docs/DESIGN.md` §8 Phase 1's spike scope).
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
//! - `Automaton.normalizeNumberSystems`, `applyAllRepresentations`,
//!   `applyAllRepresentationsWithOutput` (all need real `NumberSystem` objects with
//!   `useAllRepresentations`/`getAllRepresentations`, plus `AutomatonLogicalOps.and`/
//!   `ProductStrategies.crossProduct`).
//! - `AutomatonDFA`'s regex-string constructors (need `BricsConverter`) and its
//!   file-address constructor (file I/O is `wr-io`/Phase 3 scope).
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
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
    /// `encoder[i]` = product of `alphabet[0..i]`'s sizes (`encoder[0] == 1`). Cached at
    /// construction, matching Java's `RichAlphabet.encoder` (there computed lazily on
    /// first `encode()` call; here eagerly, since this crate always needs it).
    encoder: Vec<usize>,
    /// `Automaton.labelSorted` (`Automaton.java:49`): memoizes whether [`Automaton::sort_label`]
    /// has already run against the current `label`, so a repeat call (e.g. from
    /// [`Automaton::canonize`]) is a cheap no-op. Always starts `false` — there is no
    /// public constructor parameter for it, matching a freshly bound/relabeled automaton.
    label_sorted: bool,
}

impl Automaton {
    /// Builds an `Automaton` from an already-constructed [`Fa`] and track metadata.
    /// `alphabet`, `label`, and `msd` must have the same length as each other and match
    /// `fa.alphabet_size` (`Π alphabet[i].len() == fa.alphabet_size`) — not asserted here
    /// (this is a Phase-1 slice, not a validating constructor); callers are responsible.
    pub fn new(
        fa: Fa,
        alphabet: Vec<Vec<i32>>,
        label: Vec<String>,
        msd: Vec<Option<bool>>,
    ) -> Self {
        let encoder = Self::compute_encoder(&alphabet);
        Automaton {
            fa,
            alphabet,
            label,
            msd,
            encoder,
            label_sorted: false,
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
            encoder: Vec::new(),
            label_sorted: false,
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
    /// `o`/`d` but leaves `fa.q`/`fa.q0`/`fa.alphabet_size` **stale**, and the flags
    /// themselves are deliberately untouched (Java clears neither). Java sets `NS` and
    /// `label` to literal `null`; this crate's convention is that an empty `Vec` plays
    /// the `null` role for both (see [`Automaton::is_bound`]/[`Automaton::unlabel`]), so
    /// they are emptied instead.
    pub fn clear(&mut self) {
        self.fa.clear();
        self.alphabet.clear();
        self.encoder.clear();
        self.msd.clear();
        self.label.clear();
        self.label_sorted = false;
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

    /// Decodes a transition symbol back into its per-track digit tuple. Inverse of
    /// [`Automaton::encode`] (`decode(encode(x)) == x`).
    pub fn decode(&self, mut sym: i32) -> Vec<i32> {
        let mut out = Vec::with_capacity(self.alphabet.len());
        for track in &self.alphabet {
            let size = track.len() as i32;
            let idx = sym.rem_euclid(size);
            out.push(track[idx as usize]);
            sym = sym.div_euclid(size);
        }
        out
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

    /// `Automaton.randomLabel` (`Automaton.java:299-305`).
    pub fn random_label(&mut self) {
        self.label = (0..self.alphabet.len()).map(|i| i.to_string()).collect();
    }

    /// `Automaton.unlabel` (`Automaton.java:307-310`). Currently only reachable from
    /// this file's own tests: its only Java caller is `applyAllRepresentations(WithOutput)`,
    /// both out of scope here (see module docs) — kept as a faithful, self-contained
    /// one-liner rather than dropped, since a future unit porting those methods will
    /// want it as-is.
    pub fn unlabel(&mut self) {
        self.label = Vec::new();
        self.label_sorted = false;
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
        let already_sorted = self.label.windows(2).all(|w| w[0] <= w[1]);
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

        for row in self.fa.d.iter_mut() {
            let mut permuted_row = BTreeMap::new();
            for (&sym, dests) in row.iter() {
                permuted_row.insert(encoded_input_permutation[sym as usize], dests.clone());
            }
            *row = permuted_row;
        }
    }

    /// `Automaton.canonize` (`Automaton.java:328-331`).
    pub fn canonize(&mut self) {
        self.sort_label();
        self.fa.canonicalize();
    }

    /// `Automaton.forceCanonize` (`Automaton.java:332-335`). Java resets the private
    /// `fa.canonized` flag before calling `canonize()`, since `FA.canonizeInternal`
    /// memoizes behind it; this crate's [`Fa::canonicalize`] carries no such flag (see
    /// its own module docs — it always recomputes), so there is nothing to reset here.
    /// Kept as a distinct method anyway for call-site parity with Java.
    pub fn force_canonize(&mut self) {
        self.canonize();
    }

    /// `Automaton.bind` (`Automaton.java:438-444`). BOTH halves of Java's guard clause
    /// are ported as of U0 — binding names to a TRUE/FALSE automaton is an error, not a
    /// no-op — and both become a panic (message matches `WalnutException.invalidBind`:
    /// "invalid use of method bind") rather than a `Result`, matching this file's
    /// existing convention for caller-contract violations (see [`Automaton::encode`]'s
    /// doc comment). Note the trivial half is NOT subsumed by the arity half:
    /// `bind(vec![])` on a trivial automaton passes the `0 == 0` arity check.
    /// `fa.setCanonized(false)` has no equivalent (see [`Automaton::force_canonize`]'s
    /// doc comment).
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
        Self::remove_indices(&mut automaton.msd, &same_label_indices);
        automaton.alphabet = new_alphabet;
        automaton.encoder = new_encoder;
        automaton.determine_alphabet_size();
        Self::remove_indices(&mut automaton.label, &same_label_indices);
    }

    /// `UtilityMethods.removeIndices` (`UtilityMethods.java:95-103`): drops the elements
    /// at the given positions, keeping the rest in their original relative order.
    fn remove_indices<T: Clone>(items: &mut Vec<T>, indices: &[usize]) {
        let mut kept = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            if !indices.contains(&i) {
                kept.push(item.clone());
            }
        }
        *items = kept;
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
        if !self.fa.is_deterministic() {
            // Working with an NFA. Let's trim, then determinize from {q0}.
            self.fa = crate::trim::trim(&self.fa);
            let initial: BTreeSet<usize> = [self.fa.q0].into_iter().collect();
            self.fa = crate::determinize::subset_construction(&self.fa, &initial);
        }
        // `FA.justMinimize`'s `convertNFAtoDFA()` call is a storage-representation
        // optimization only (see `fa.rs` module docs: this crate always uses one
        // unified NFA-shaped table) — not replicated.
        self.fa = crate::minimize::minimize(&self.fa).expect(
            "minimize's only OTHER precondition (determinism) always holds here -- the \
             already-deterministic branch above skips reachability trimming, so WB-001 \
             (docs/WALNUT-BUGS.md) is reachable, faithfully, not a panic",
        );
    }

    /// `Automaton.determinizeAndMinimize(IntSet qqq)` (`Automaton.java:403-406`) — the
    /// generalized multi-initial-state entry point (e.g. for a caller that just ran
    /// [`Fa::reverse`], whose returned "new initial states" are a genuine set, not
    /// necessarily a singleton). Unlike the no-arg overload, this is unconditional —
    /// Java's version has no `!isDeterministic` guard either.
    pub fn determinize_and_minimize_from(&mut self, initial: &BTreeSet<usize>) {
        self.fa = crate::determinize::subset_construction(&self.fa, initial);
        self.fa = crate::minimize::minimize(&self.fa).expect(
            "subset_construction's output is always deterministic and q0-reachable -- \
             minimize's documented preconditions",
        );
    }

    /// `Automaton.asDFA` (`Automaton.java:152-158`).
    pub fn as_dfa(&self) -> AutomatonDFA {
        AutomatonDFA::from(self.clone())
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
    fn require_dfa_storage(mut automaton: Automaton) -> Automaton {
        if automaton.fa.is_true_false_automaton() {
            return automaton;
        }
        if !automaton.fa.is_deterministic() {
            if automaton.is_fao() {
                // WalnutException.nonDeterministicO()'s exact message (double period is
                // in the original, not a typo introduced here).
                panic!("NFAOs are not supported..");
            }
            automaton.determinize_and_minimize();
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
        if automaton.fa.is_true_false_automaton() {
            return AutomatonDFA::true_false(automaton.fa.is_true_automaton());
        }
        AutomatonDFA(Self::require_dfa_storage(automaton))
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
}
