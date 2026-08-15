// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Ports `Automata/Morphism.java` (196 LOC) — a morphism from a finite alphabet to
//! the integers, defined by the (possibly non-uniform-length) integer word each
//! letter maps to. E.g. over `{0, 1, 2}`: `0 -> [-3]0102[11], 1 -> 2113, 2 -> 314`
//! (square brackets escape a value outside `0..=9`, in both the domain letter and
//! the image, as of Walnut 8).
//!
//! # Constructor split: parsing lives in `wr-io`, not here
//!
//! Java's constructor is `Morphism(String mapString)`, which calls
//! `ParseMethods.parseMorphism(mapString)` (`:53-59`) and derives `range`/`length`
//! from the result. That parser is already ported as
//! [`wr_io::parse_methods::parse_morphism`] (`Automata/ParseMethods.java:166-193`,
//! confirmed identical mapping semantics: a `TreeMap<Integer, IntList>`, i.e. this
//! crate's `BTreeMap<i32, Vec<i32>>`) — but `wr-io` depends on `wr-core`, not the
//! reverse (`lib.rs`'s crate-boundary rule), so `Morphism` here cannot call it
//! directly without inverting that edge. [`Morphism::from_mapping`] is therefore the
//! faithful two-step split of Java's one-step constructor: a `wr-io`/`wr-cli` caller
//! parses the string with `parse_morphism` first, then hands the resulting
//! `BTreeMap<i32, Vec<i32>>` here. `BTreeMap` (not `HashMap`) matters for real
//! observable behavior, not just determinism-hygiene: [`Morphism::mapping`]'s
//! iteration order is read directly into [`Morphism::write`]'s file output and
//! [`Morphism::make_inter_predicate`]'s generated predicate text, both matching
//! Java's `TreeMap` (sorted-by-key) order exactly via `BTreeMap`'s same guarantee.
//!
//! # `toWordAutomaton` (`:78-107`) and its private helpers — ported in U24
//!
//! [`Morphism::to_word_automaton`] plus its private helpers
//! [`determine_transitions`]/[`determine_max_entry`]/[`determine_max_image_length`]
//! (`determineTransitions`/`determineMaxEntry`/`determineMaxImageLength`, `:95-132`)
//! promote a `Morphism` into a `WordAutomaton`-shaped [`crate::automaton::Automaton`]
//! (one state per domain letter, one transition per image position) — the primitive
//! `wr-cli`'s U24 `promote` command needs. Deferred by this file's original author
//! (grouped with `buildTransitionsFromMorphism`/`convertNS` as later,
//! `Automaton`-consuming work — see `logicalops.rs`'s "Not ported" section for the
//! sibling helpers), landed here once `promote` was the unit that actually needed it.
//!
//! **The one genuinely missing primitive this unit had to add: `Fa`/`Automaton`
//! carried no `canonized` memo at all.** `Morphism.java:88` calls
//! `promotion.fa.setCanonized(true)` specifically so the promoted automaton is never
//! canonicalized ("this word automaton is purely symbolic in input and we want it in
//! the exact order given", `:87`) — `canonizeInternal` *drops states BFS cannot reach
//! from `q0`*, and a promoted morphism's states are its domain letters, most of which
//! are typically unreachable, so letting canonicalization run would change the
//! automaton's state COUNT and its state↔letter correspondence, a language-level
//! difference, not a renumbering. **Resolved by adding the flag to
//! [`crate::automaton::Automaton`], not to [`crate::fa::Fa`]** — see
//! [`crate::automaton::Automaton::canonized`]'s doc comment for the full argument
//! (in short: every production caller of [`crate::fa::Fa::canonicalize`] already goes
//! through [`crate::automaton::Automaton::canonize`]/`force_canonize`, so gating there
//! is behaviorally identical to gating on `Fa` while confining the new field to two
//! existing struct literals instead of the ~230 `Fa { … }` literals across this
//! workspace that adding it to `Fa` itself would break).
//!
//! **A second genuine bug found while porting this method, logged as WB-036 (see
//! `docs/WALNUT-BUGS.md`).** Java's `toWordAutomaton` builds `Q = maxEntry + 1`
//! states but a transition table (`newD`) with only `mapping.size()` entries, indexed
//! by DOMAIN-LETTER SORT POSITION, not by the letter's own value — with no check that
//! the two agree. Whenever the morphism's domain doesn't cover every value up to
//! `maxEntry` (e.g. `0->05 1->10`: two domain letters, but images reference value `5`,
//! so `Q = 6` while the transition list has only 2 entries), any later access to a
//! state past `mapping.size() - 1` — confirmed to include `AutomatonWriter`'s own
//! per-state loop, i.e. `promote`'s ordinary output path — throws Java's
//! `IndexOutOfBoundsException`. [`Morphism::to_word_automaton`] reproduces this as
//! [`MorphismError::DomainDoesNotCoverImageRange`], checked at construction time
//! (before ever handing back a malformed automaton whose `Fa::d.len() != Fa::q`,
//! an invariant essentially every other algorithm in this crate assumes) rather than
//! deferred to wherever Java's crash happens to surface — see WB-036 for the full
//! empirical repro against real `walnut-java`. The MIRROR case (a domain *larger* than
//! the image range needs, e.g. `0->00 1->00`) is not an error in Java and is not one
//! here either: Java silently ignores the dangling transition rows (every consumer loops
//! `0..Q`), which this port reproduces by truncating `newD` to `Q`.
//!
//! **The `NumberSystem` construction at `Morphism.java:90` is a VALIDATION step, not
//! just metadata.** `new NumberSystem("msd_" + maxImageLength)` throws
//! `WalnutException("Number system msd_<k> is not defined.")` for any `k < 2`
//! (`NumberSystem.java:322-331` — there is no addition automaton to synthesize below
//! base 2, and no custom-base file to fall back on), so real Walnut cleanly refuses to
//! `promote` a morphism whose longest image is one letter. This crate's `Automaton`
//! stores no `NumberSystem`, so [`MorphismError::NumberSystemNotDefined`] carries that
//! check (and Java's message) explicitly — see [`Morphism::to_word_automaton`]'s doc
//! comment for the statement-order argument fixing its priority relative to the other
//! two failure modes.
//!
//! # `escapedInt` / `write`
//!
//! [`Morphism::write`] ports `write(String address)` (`:62-72`) plus its private
//! `escapedInt` helper (`:74-76`), splitting Java's one `String address` overload
//! into a generic `io::Write` sink ([`Morphism::write`]) and a path-based wrapper
//! ([`Morphism::write_to_file`]) — the same split `wr-io`'s `writer.rs` uses for
//! every other Java `write(String address)` method in this codebase. Java's
//! `System.lineSeparator()` becomes a plain `\n` (this workspace targets Unix CI/
//! dev, and no golden fixture depends on `\r\n`).
//!
//! # `requirePositiveUniformLength` becomes `Result`, not a panic
//!
//! Java's `requirePositiveUniformLength` (`:188-195`) throws an unchecked
//! `WalnutException` — one of two distinct messages depending on whether `length`
//! is negative (non-uniform) or exactly zero (uniform but empty). Per
//! `PORTING.md`'s "checked/unchecked exception -> `Result<T, WalnutError>` with a
//! real error enum" rule, this is [`Morphism::require_positive_uniform_length`]
//! returning `Result<(), MorphismError>` with the two messages preserved verbatim
//! in [`MorphismError`]'s `Display` impl, rather than a stringly-typed panic.
//!
//! # `makeInterPredicate` trusts its caller for uniformity — faithful, not a bug
//!
//! [`Morphism::make_inter_predicate`] (`:160-186`) uses `self.length` directly with
//! no uniformity check of its own; if called on a non-uniform morphism (`length ==
//! -1`) it emits a nonsensical `r>=0 & r<-1` clause (unsatisfiable). This is NOT
//! logged as a `docs/WALNUT-BUGS.md` finding: `Morphism`'s only real caller,
//! `Main/Commands/Image.java:21` (`h.requirePositiveUniformLength()`, called
//! immediately after construction, before any `makeInterPredicate` call at `:34`),
//! already enforces the precondition before ever calling this method — so the
//! un-self-checking method is a normal "caller's contract" shape, not a reachable
//! defect. Ported verbatim (no defensive check added here, matching Java).
//!
//! No genuine Walnut bug was found in the original (`from_mapping`/`write`/
//! `require_positive_uniform_length`/`make_inter_predicate`) portion of this file —
//! unlike some `Automata/` files, there is no unguarded `mapping.get(letter)`-style
//! lookup on a letter outside the morphism's domain anywhere in those methods; the
//! two that read `mapping` (`write`, `makeInterPredicate`) both iterate `entrySet()`
//! rather than index into it by an externally supplied letter. `toWordAutomaton`
//! (ported later, U24) is different — see WB-036 above.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// `WalnutException.morphismNotUniform()` / the inline `WalnutException` at
/// `Morphism.java:193` — both thrown (as unchecked exceptions) by
/// `requirePositiveUniformLength` (`:188-195`) — plus, as of U24, the two failure
/// modes of [`Morphism::to_word_automaton`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MorphismError {
    /// `length < 0` (non-uniform morphism). `WalnutException.morphismNotUniform()`
    /// (`WalnutException.java:85`).
    NotUniform,
    /// `length == 0` (uniform, but every image is empty). The inline
    /// `WalnutException` at `Morphism.java:193`.
    NotPositiveUniform,
    /// `determineMaxEntry` (`Morphism.java:118-128`): some image contains a negative
    /// value. `WalnutException.morphismNegative()` (`WalnutException.java:83`).
    NegativeValue,
    /// WB-036 (`docs/WALNUT-BUGS.md`): the morphism's domain doesn't cover every
    /// value `0..=maxEntry` referenced in some image, so `toWordAutomaton` would
    /// build an `FA` with more states than transition-table entries — a shape real
    /// Java doesn't reject either, but that reliably throws
    /// `IndexOutOfBoundsException` the moment anything (in practice,
    /// `AutomatonWriter`'s own per-state loop) touches one of the missing states.
    /// See the module docs and WB-036 for the full derivation and empirical repro.
    DomainDoesNotCoverImageRange,
    /// `new NumberSystem(NumberSystem.MSD_UNDERSCORE + maxImageLength)`
    /// (`Morphism.java:90`) — Java's `NumberSystem` constructor is **validating**, not
    /// just metadata storage: `setAdditionAutomaton` (`NumberSystem.java:322-331`) has
    /// no `msd_0`/`msd_1` addition automaton to build and no custom-base file to fall
    /// back on, so it throws `WalnutException("Number system msd_" + maxImageLength +
    /// " is not defined.")` for every `maxImageLength < 2`.
    ///
    /// So real Walnut cleanly REFUSES `promote` on any morphism whose longest image is
    /// a single letter (e.g. the 1-uniform `0->1 1->0`), writing no file at all. The
    /// carried value is `maxImageLength`, so the message reproduces Java's exactly.
    /// Shares [`crate::numsys::NumSysError::NotDefined`]'s message text verbatim —
    /// the two are literally the same Java throw site.
    NumberSystemNotDefined(usize),
}

impl fmt::Display for MorphismError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MorphismError::NotUniform => {
                write!(f, "A morphism applied to a word automaton must be uniform.")
            }
            MorphismError::NotPositiveUniform => {
                write!(
                    f,
                    "A morphism applied to a word automaton must have positive uniform length."
                )
            }
            // Verbatim `WalnutException.morphismNegative()`.
            MorphismError::NegativeValue => {
                write!(f, "Cannot promote a morphism with negative values.")
            }
            MorphismError::DomainDoesNotCoverImageRange => write!(
                f,
                "morphism domain does not cover every value referenced in an image \
                 (WB-036: real Walnut throws IndexOutOfBoundsException for this shape \
                 once the promoted automaton is written)"
            ),
            // Verbatim `NumberSystem.java:330`'s `"Number system " + name + " is not
            // defined."`, with `name` being the `MSD_UNDERSCORE + maxImageLength` the
            // constructor at `Morphism.java:90` was handed. Same formatting convention as
            // `crate::numsys::NumSysError::NotDefined`'s own `Display` arm.
            MorphismError::NumberSystemNotDefined(max_image_length) => {
                write!(f, "Number system msd_{max_image_length} is not defined.")
            }
        }
    }
}

impl std::error::Error for MorphismError {}

/// `Automata/Morphism.java`'s `Morphism` class (`:40-48`'s three fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Morphism {
    /// `Morphism.length` (`:42`): the uniform length of every letter's image, or
    /// `-1` if the morphism is not uniform (images have differing lengths). `0` is
    /// a valid (uniform) length when `mapping` is non-empty but every image is
    /// empty, and is ALSO the value `determineUniformLength` returns for an empty
    /// `mapping` (`firstElement` never flips, `imageLength` stays at its initial
    /// `0`) — ported verbatim, see [`Morphism::from_mapping`]'s doc comment.
    pub length: i32,
    /// `Morphism.mapping` (`:45`): each domain letter's image, in the SAME sorted
    /// order Java's `TreeMap` iterates (see module docs — this is observable, not
    /// cosmetic).
    pub mapping: BTreeMap<i32, Vec<i32>>,
    /// `Morphism.range` (`:48`): the set of values appearing in ANY image, i.e.
    /// the union of every `mapping` value. Java uses an unordered `IntOpenHashSet`
    /// here; this is a `BTreeSet` (sorted).
    ///
    /// **Known, accepted divergence — display-only, but real.** There are exactly
    /// two readers of this field in Walnut:
    ///
    /// * `Main/Commands/Image.java:26-27` — the only *behavior-affecting* reader.
    ///   It does `new ArrayList<>(h.range)` then `range.sort(Integer::compareTo)`,
    ///   i.e. re-sorts before iterating, so the automaton `image` builds is
    ///   unaffected by the set's iteration order. `BTreeSet` agrees with it.
    /// * `Main/Commands/Morphism.java:14` — `System.out.print(M.range)`, which
    ///   prints the raw `IntOpenHashSet` (fastutil *hash* order) straight to the
    ///   `morphism` command's stdout. This one is NOT re-sorted, and a `BTreeSet`
    ///   will print a different order: for `0->0123 1->21 2->03 3->23`, Java prints
    ///   `{0, 2, 1, 3}` where this crate would print `{0, 1, 2, 3}`.
    ///
    /// So the future `wr-cli` `morphism` command's "and range …" output line will
    /// differ from Java's, in element order only (same elements, same count).
    /// Reproducing fastutil's hash order is deliberately not attempted: it is an
    /// implementation detail of a third-party hash table, and per `CLAUDE.md`'s
    /// semantic-equivalence rule the comparison bar is language equivalence plus
    /// *normalized* text, not byte-identical stdout. Do not read this note as "no
    /// divergence exists" — one does; it is confined to that one printed line.
    pub range: BTreeSet<i32>,
}

impl Morphism {
    /// `Morphism(String mapString)` (`:53-59`), minus the parsing step — see the
    /// module docs' "Constructor split" section for why parsing happens in
    /// `wr-io` (`wr_io::parse_methods::parse_morphism`) and this takes the
    /// already-parsed mapping directly.
    pub fn from_mapping(mapping: BTreeMap<i32, Vec<i32>>) -> Morphism {
        let mut range = BTreeSet::new();
        for image in mapping.values() {
            range.extend(image.iter().copied());
        }
        let length = determine_uniform_length(&mapping);
        Morphism {
            length,
            mapping,
            range,
        }
    }

    /// `write(String address)` (`:62-72`), the generic-sink half — see
    /// [`Morphism::write_to_file`] for the path-based wrapper matching Java's
    /// signature. Writes one `<escaped key> -> <escaped image, concatenated>`
    /// line per mapping entry, in `self.mapping`'s (sorted) iteration order.
    pub fn write<W: Write>(&self, out: &mut W) -> io::Result<()> {
        for (key, image) in &self.mapping {
            write!(out, "{} -> ", escaped_int(*key))?;
            for y in image {
                write!(out, "{}", escaped_int(*y))?;
            }
            writeln!(out)?;
        }
        Ok(())
    }

    /// `write(String address)` (`:62-72`) — opens `path` and writes through
    /// [`Morphism::write`], matching `wr-io`'s `write_automaton_txt`/
    /// `write_automaton_gv` path-wrapper idiom (`crates/wr-io/src/writer.rs`).
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let file = File::create(path)?;
        let mut out = BufWriter::new(file);
        self.write(&mut out)?;
        out.flush()
    }

    /// `toWordAutomaton()` (`:78-92`) — promotes this morphism into a
    /// `WordAutomaton`-shaped [`crate::automaton::Automaton`]: one state per domain
    /// letter (state `q`'s output is `q` itself), one transition per image position
    /// (`d[q][position] = image[q][position]`), over a single track whose alphabet is
    /// `0..maxImageLength` (the base of the "number system" this promoted automaton
    /// is read in — see module docs' NS section below). See the module docs for the
    /// `canonized` flag this sets (so the caller's typically-mostly-unreachable
    /// domain-letter states survive being written out) and WB-036 (the domain/image
    /// mismatch this validates against, up front, before ever returning a malformed
    /// automaton).
    ///
    /// Unlike [`Morphism::require_positive_uniform_length`]/
    /// [`Morphism::make_inter_predicate`]'s caller (`Image.image`, which enforces
    /// uniformity first), `toWordAutomaton`'s one real caller — `Prover.promoteCommand`
    /// — never calls `requirePositiveUniformLength` first, and this method has no
    /// uniformity requirement of its own either: `maxImageLength` is simply the
    /// LONGEST image among all letters, and a shorter letter's image just leaves that
    /// letter's state without a transition on the higher digit positions (a dead run,
    /// not an error). Ported faithfully — no uniformity check added here.
    ///
    /// # NS: `msd_<maxImageLength>`, represented as `msd[0] = Some(true)` + a real check
    ///
    /// Java constructs a real `NumberSystem(MSD_UNDERSCORE + maxImageLength)` and adds
    /// it to `promotion.getNS()` (`:90`). This crate's `Automaton` doesn't carry a
    /// `NumberSystem` object at all (`automaton.rs`'s own module docs: "deliberately
    /// NOT full Java parity" — see also `wr-io`'s `writer.rs`, which reconstructs the
    /// same canonical name from `msd`+`alphabet.len()` for the identical reason), only
    /// the msd/lsd direction (`Automaton::msd`) and the custom-base restriction
    /// (`Automaton::all_reps`, unused here — `msd_<k>` is not a custom base). A plain
    /// `msd_<k>` `NumberSystem`'s alphabet is exactly `intRangeList(k)`
    /// (`NumberSystem.java`'s standard-base constructor), which is precisely what this
    /// method's own `alphabet` local already is — so `msd = vec![Some(true)]` captures
    /// every *stored* fact this crate's `Automaton` can represent about that
    /// `NumberSystem`.
    ///
    /// **But storage is not all that constructor does — it VALIDATES**, and dropping the
    /// validation was a real divergence (found in adversarial review, verified live
    /// against the real jar). `NumberSystem(name)` runs `setAdditionAutomaton`
    /// (`NumberSystem.java:322-331`): with no `msd_<k>_addition.txt` custom-base file on
    /// disk, it can only synthesize an addition automaton when `Integer.parseInt(base) >
    /// 1`, and otherwise throws `WalnutException("Number system " + name + " is not
    /// defined.")`. So a morphism whose longest image is a single letter (`maxImageLength
    /// == 1`, e.g. the 1-uniform `0->1 1->0`) — or the degenerate all-empty-images case
    /// (`0`) — makes real Walnut REFUSE `promote`, writing no automaton at all.
    /// [`MorphismError::NumberSystemNotDefined`] is that check, with Java's message
    /// verbatim.
    ///
    /// **Statement order matters and is ported exactly.** Java runs
    /// `determineMaxEntry` (which throws on a negative image value) at `:82`, well
    /// before the `NumberSystem` construction at `:90` — so `NegativeValue` wins over
    /// `NumberSystemNotDefined` when both apply. Conversely WB-036's
    /// `IndexOutOfBoundsException` is not thrown inside `toWordAutomaton` at all (it
    /// surfaces later, at write time), so the `NumberSystem` throw beats it: the check
    /// below sits between the two, reproducing Java's real priority in both directions.
    pub fn to_word_automaton(&self) -> Result<crate::automaton::Automaton, MorphismError> {
        use crate::automaton::Automaton;
        use crate::fa::Fa;

        let max_image_length = determine_max_image_length(&self.mapping);
        let max_entry = determine_max_entry(&self.mapping)?;
        let mut new_d = determine_transitions(&self.mapping);

        // `promotion.getNS().add(new NumberSystem(MSD_UNDERSCORE + maxImageLength));`
        // (`:90`) -- the constructor's own validation, which is the only part of it this
        // crate's representation cannot store. See the doc comment above for why this
        // sits AFTER `determine_max_entry` (Java's `:82` throws first) but BEFORE the
        // WB-036 check (whose Java crash happens later still, outside this method).
        if max_image_length < 2 {
            return Err(MorphismError::NumberSystemNotDefined(max_image_length));
        }

        // WB-036: `newD.len() == self.mapping.len()`, indexed by domain-letter SORT
        // POSITION, not value; `Q = max_entry + 1` states are about to be declared.
        // If the domain doesn't cover every value up to `max_entry`, some state in
        // `mapping.len()..Q` has no transition-table entry at all -- Java doesn't
        // check this either, it just crashes the first time anything touches that
        // state's row. Checked here, before ever building the malformed `Fa`, whose
        // `d.len() != q` would otherwise be a live landmine for every other algorithm
        // in this crate (all of which assume that invariant).
        let q = max_entry as usize + 1;
        if new_d.len() < q {
            return Err(MorphismError::DomainDoesNotCoverImageRange);
        }
        // The OTHER side of the same `newD.len() != Q` mismatch: a domain LARGER than
        // the image range needs (e.g. `0->00 1->00`, where no image ever references a
        // value above `0`, so `Q == 1` but the domain has two letters). Java tolerates
        // this silently -- `setFields` stores the over-long `newD` as-is, and every
        // consumer (`AutomatonWriter`'s per-state loop, `canonizeInternal`'s
        // `for q in 0..Q`) iterates `0..Q` and simply never touches the dangling rows.
        // Truncating here reproduces that observable behavior exactly while restoring
        // this crate's `d.len() == q` invariant; REJECTING instead would diverge from
        // Java on input Java genuinely accepts.
        new_d.truncate(q);

        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q: 0,
            // Java leaves `alphabetSize` at its `0` default here and lets a later
            // `setAlphabetSize`/`richAlphabet` pass fill it in; setting it up front is
            // an intentional, safe divergence -- `max_image_length` is exactly the
            // product of this automaton's one track's alphabet (`0..max_image_length`),
            // i.e. what any later recomputation would produce anyway, and the flagless
            // `0` would make `Automaton::decode`/`encode` misbehave in this crate.
            alphabet_size: max_image_length,
            o: Vec::new(),
            d: Vec::new(),
        };
        // `promotion.getFa().setFields(maxEntry + 1, new IntArrayList(
        //  UtilityMethods.intRangeList(maxEntry + 1)), newD);` (`:84-85`) -- the
        // output vector is `0, 1, ..., maxEntry`, i.e. state `q`'s output IS `q`.
        fa.set_fields(q, (0..=max_entry).collect(), new_d);

        let alphabet = vec![(0..max_image_length as i32).collect::<Vec<i32>>()];
        let mut promotion = Automaton::new(fa, alphabet, Vec::new(), vec![Some(true)]);
        // `promotion.fa.setCanonized(true);` (`:87-88`) -- see module docs.
        promotion.set_canonized(true);
        Ok(promotion)
    }

    /// `requirePositiveUniformLength()` (`:188-195`) — see module docs for why
    /// this returns `Result` rather than panicking.
    pub fn require_positive_uniform_length(&self) -> Result<(), MorphismError> {
        if self.length < 0 {
            return Err(MorphismError::NotUniform);
        }
        if self.length == 0 {
            return Err(MorphismError::NotPositiveUniform);
        }
        Ok(())
    }

    /// `makeInterPredicate(int i, String baseAutomatonName, String numSys)`
    /// (`:160-186`) — generates the predicate for an intermediary automaton that
    /// accepts `n` iff value `i` appears at position `n` in the image of the
    /// letter `baseAutomatonName[q]` (`q = n / length`, `r = n % length` the
    /// within-image offset). See module docs: assumes `self.length` is a
    /// positive uniform length (the caller's job to enforce, matching Java).
    pub fn make_inter_predicate(&self, i: i32, base_automaton_name: &str, num_sys: &str) -> String {
        let mut predicate = String::from(num_sys);
        predicate.push_str(&format!(
            " E q, r (n={}*q+r & r>=0 & r<{}",
            self.length, self.length
        ));
        for (key, symbol_image) in &self.mapping {
            let mut exists = false;
            let mut clause = format!(" & ({base_automaton_name}[q]");
            for (j, &value) in symbol_image.iter().enumerate() {
                if value == i {
                    if !exists {
                        clause.push_str(&format!("= @{key} => (r={j}"));
                        exists = true;
                    } else {
                        clause.push_str(&format!("|r={j}"));
                    }
                }
            }
            if exists {
                clause.push_str("))");
            } else {
                clause.push_str(&format!("!= @{key})"));
            }
            predicate.push_str(&clause);
        }
        predicate.push(')');
        predicate
    }
}

/// `escapedInt(Integer y)` (`:74-76`): `0..=9` prints bare, anything else (including
/// negative values) is bracketed.
fn escaped_int(y: i32) -> String {
    if (0..=9).contains(&y) {
        y.to_string()
    } else {
        format!("[{y}]")
    }
}

/// `determineTransitions(Map<Integer, IntList>)` (`:95-107`): one entry per domain
/// letter, IN `mapping`'s SORTED-KEY ORDER (i.e. list index `k` is the `k`-th
/// smallest domain letter, not the letter's own value — see [`Morphism::to_word_automaton`]'s
/// WB-036 doc note on why that distinction matters), mapping each image POSITION to
/// a singleton destination-state list holding that position's value.
fn determine_transitions(mapping: &BTreeMap<i32, Vec<i32>>) -> Vec<BTreeMap<i32, Vec<usize>>> {
    mapping
        .values()
        .map(|image| {
            let mut xmap = BTreeMap::new();
            for (i, &y) in image.iter().enumerate() {
                // `y >= 0` is guaranteed here: `to_word_automaton` calls
                // `determine_max_entry` (which rejects any negative value) before
                // this function, matching Java's own `maxEntry`-before-`newD`
                // statement order in `toWordAutomaton` (`:82-83`).
                xmap.insert(i as i32, vec![y as usize]);
            }
            xmap
        })
        .collect()
}

/// `determineMaxEntry(Map<Integer, IntList>)` (`:118-128`): the largest value
/// appearing in any image, or `0` for a mapping whose images are all empty (Java's
/// `maxEntry` starts at `0` and only ever increases). Errors on the FIRST negative
/// value found, in `mapping`'s (sorted-key, then within-image) iteration order —
/// matching Java's early `throw` inside the same nested loop that also tracks the
/// max, not a separate up-front scan.
fn determine_max_entry(mapping: &BTreeMap<i32, Vec<i32>>) -> Result<i32, MorphismError> {
    let mut max_entry = 0;
    for image in mapping.values() {
        for &y in image {
            if y < 0 {
                return Err(MorphismError::NegativeValue);
            } else if y > max_entry {
                max_entry = y;
            }
        }
    }
    Ok(max_entry)
}

/// `determineMaxImageLength(Map<Integer, IntList>)` (`:130-140`): the length of the
/// longest image among all domain letters, or `0` for an empty `mapping`.
fn determine_max_image_length(mapping: &BTreeMap<i32, Vec<i32>>) -> usize {
    mapping.values().map(Vec::len).max().unwrap_or(0)
}

/// `determineUniformLength(Map<Integer, IntList>)` (`:143-155`). `-1` if any two
/// entries' images differ in length; the length shared by every entry otherwise
/// (including `0` for an empty `mapping` — Java's `firstElement` flag never flips,
/// so `imageLength` stays at its initial `0`).
fn determine_uniform_length(mapping: &BTreeMap<i32, Vec<i32>>) -> i32 {
    let mut values = mapping.values();
    let Some(first) = values.next() else {
        return 0;
    };
    let expected = first.len();
    for image in values {
        if image.len() != expected {
            return -1;
        }
    }
    // Java returns `entry.getValue().size()`, an `int`, so an image longer than
    // `i32::MAX` is not representable there in the first place. `expected as i32`
    // would silently truncate (and could even manufacture a bogus `-1`
    // "non-uniform" answer); this cannot be reached from any real input, but a
    // panic beats a wrong length.
    i32::try_from(expected).expect("morphism image length exceeds i32::MAX")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(i32, &[i32])]) -> BTreeMap<i32, Vec<i32>> {
        pairs.iter().map(|(k, v)| (*k, v.to_vec())).collect()
    }

    // -- independent smoke test (NOT a port of MorphismTest.testGamMorphism) -------
    //
    // Java's `testGamMorphism` uses the real `gam` mapping `0->01 1->21 2->03 3->23`
    // and asserts a transition table derived from `toWordAutomaton`; its substance is
    // therefore deferred along with `toWordAutomaton` itself (see the module docs'
    // "Not ported" section) and is NOT replicated by anything below. The mapping used
    // here is Thue-Morse, not `gam`, and it only checks `from_mapping`'s own outputs.

    #[test]
    fn thue_morse_morphism_is_uniform_length_two() {
        // The classic Thue-Morse morphism: 0 -> 01, 1 -> 10.
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0])]));
        assert_eq!(h.length, 2);
        assert_eq!(h.mapping.get(&0), Some(&vec![0, 1]));
        assert_eq!(h.mapping.get(&1), Some(&vec![1, 0]));
        assert_eq!(h.range, BTreeSet::from([0, 1]));
    }

    // -- MorphismTest.testGamMorphism / to_word_automaton (U24) --------------------

    #[test]
    fn to_word_automaton_gam_matches_real_walnut_transition_table() {
        // Direct port of `MorphismTest.testGamMorphism`: gam = "0->01 1->21 2->03 3->23",
        // asserted against `P.getFa().getT().getNfaD().toString()`'s real value,
        // `"[{0=>[0], 1=>[1]}, {0=>[2], 1=>[1]}, {0=>[0], 1=>[3]}, {0=>[2], 1=>[3]}]"`.
        let h = Morphism::from_mapping(map(&[
            (0, &[0, 1]),
            (1, &[2, 1]),
            (2, &[0, 3]),
            (3, &[2, 3]),
        ]));
        assert_eq!(h.length, 2);
        let p = h.to_word_automaton().unwrap();

        assert_eq!(p.fa.q, 4);
        assert_eq!(p.fa.alphabet_size, 2);
        assert_eq!(p.alphabet, vec![vec![0, 1]]);
        assert_eq!(p.msd, vec![Some(true)]);
        assert!(p.is_canonized());
        // State q's output IS q.
        assert_eq!(p.fa.o, vec![0, 1, 2, 3]);

        let expected: Vec<BTreeMap<i32, Vec<usize>>> = vec![
            BTreeMap::from([(0, vec![0]), (1, vec![1])]),
            BTreeMap::from([(0, vec![2]), (1, vec![1])]),
            BTreeMap::from([(0, vec![0]), (1, vec![3])]),
            BTreeMap::from([(0, vec![2]), (1, vec![3])]),
        ];
        assert_eq!(p.fa.d, expected);
    }

    #[test]
    fn to_word_automaton_thue_morse_shape() {
        // The Thue-Morse morphism, end-to-end through `to_word_automaton`: both the
        // domain and the range are exactly `{0, 1}`, the simplest well-formed case.
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0])]));
        let p = h.to_word_automaton().unwrap();

        assert_eq!(p.fa.q, 2);
        assert_eq!(p.fa.alphabet_size, 2);
        assert_eq!(p.fa.o, vec![0, 1]);
        let expected: Vec<BTreeMap<i32, Vec<usize>>> = vec![
            BTreeMap::from([(0, vec![0]), (1, vec![1])]),
            BTreeMap::from([(0, vec![1]), (1, vec![0])]),
        ];
        assert_eq!(p.fa.d, expected);
    }

    #[test]
    fn to_word_automaton_does_not_require_uniform_length() {
        // Unlike `Image.image` (which calls `requirePositiveUniformLength` first),
        // `Prover.promoteCommand` never does -- ported faithfully, no uniformity
        // check added here. `0->0` (image length 1), `1->10` (image length 2).
        let h = Morphism::from_mapping(map(&[(0, &[0]), (1, &[1, 0])]));
        assert_eq!(h.length, -1, "non-uniform");
        let p = h.to_word_automaton().unwrap();

        assert_eq!(p.fa.alphabet_size, 2, "max image length");
        assert_eq!(p.fa.q, 2, "max_entry(=1) + 1");
        // Letter 0's image is just `[0]`: only position 0 has a transition.
        assert_eq!(p.fa.d[0].len(), 1);
        assert!(p.fa.d[0].contains_key(&0));
        assert!(!p.fa.d[0].contains_key(&1));
    }

    /// `0 -> 01`, `1 -> 10`, `2 -> 22`: `maxImageLength = 2` (so the promoted
    /// automaton's `msd_2` number system is genuinely constructible -- a 1-uniform
    /// morphism would be rejected outright, see
    /// [`MorphismError::NumberSystemNotDefined`]), `maxEntry = 2` and a 3-letter
    /// domain (so WB-036's guard passes), and state 2 transitions ONLY to itself,
    /// making it unreachable from `q0 = 0`. Exactly the shape the `canonized` flag
    /// exists to protect.
    fn unreachable_state_morphism() -> Morphism {
        Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0]), (2, &[2, 2])]))
    }

    /// A deterministic `.txt`-shaped rendering of the whole state/output/transition
    /// table, used instead of asserting `fa.q` alone: the WRITTEN CONTENT is the
    /// observable property the `canonized` flag protects (without it, `canonize()` runs
    /// inside the writer and state 2 disappears from the file), and a state count alone
    /// would not catch a renumbering that silently changed the state<->domain-letter
    /// correspondence.
    ///
    /// Hand-rolled rather than calling `wr-io`'s real writer, which depends on
    /// `wr-core`, not the reverse (`lib.rs`'s crate-boundary rule); the `wr-cli` sibling
    /// test `promote_command_keeps_an_unreachable_domain_letter_state` runs the real
    /// writer end-to-end on this same morphism.
    fn render_txt(a: &crate::automaton::Automaton) -> String {
        let mut s = String::new();
        s.push_str("msd_2\n\n");
        for q in 0..a.fa.q {
            s.push_str(&format!("{} {}\n", q, a.fa.o[q]));
            for (&sym, dests) in &a.fa.d[q] {
                for &dest in dests {
                    s.push_str(&format!("{sym} -> {dest}\n"));
                }
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn to_word_automaton_preserves_a_state_unreachable_from_q0_via_the_canonized_flag() {
        // This is the whole reason `Morphism.java:88` sets `setCanonized(true)`:
        // without it, an ordinary `canonize()` call (which `AutomatonWriter`'s real
        // `.txt`/`.gv` paths always make) would silently drop state 2 and its
        // domain-letter correspondence.
        let p = unreachable_state_morphism().to_word_automaton().unwrap();
        assert_eq!(p.fa.q, 3);
        assert!(p.is_canonized());

        let mut canonized = p.clone();
        canonized.canonize();
        assert_eq!(
            canonized.fa.q, 3,
            "the canonized flag must survive an ordinary canonize() call"
        );
        // The observable property, not just the state count: all three states, with
        // state 2's self-loops intact, survive into the written text.
        assert_eq!(
            render_txt(&canonized),
            "msd_2\n\n\
             0 0\n0 -> 0\n1 -> 1\n\n\
             1 1\n0 -> 1\n1 -> 0\n\n\
             2 2\n0 -> 2\n1 -> 2\n\n"
        );
        assert_eq!(
            render_txt(&canonized),
            render_txt(&p),
            "canonize() on a flagged automaton must not change the output at all"
        );
    }

    #[test]
    fn to_word_automaton_state_would_genuinely_be_dropped_without_the_flag() {
        // Sanity companion to the test above -- confirms state 2 really is
        // unreachable (so canonicalize's pruning is legitimate) by explicitly
        // clearing the flag before canonizing.
        let mut p = unreachable_state_morphism().to_word_automaton().unwrap();
        p.set_canonized(false);
        p.canonize();
        assert_eq!(p.fa.q, 2, "state 2 is genuinely unreachable from q0=0");
        assert_eq!(
            render_txt(&p),
            "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n\n",
            "state 2 is gone from the written text once the flag is cleared"
        );
    }

    #[test]
    fn to_word_automaton_rejects_a_max_image_length_below_two() {
        // `Morphism.java:90`'s `new NumberSystem("msd_" + maxImageLength)` VALIDATES:
        // there is no base-1 (or base-0) number system, so real Walnut refuses
        // `promote` on the simplest possible 1-uniform morphism and writes nothing.
        let h = Morphism::from_mapping(map(&[(0, &[1]), (1, &[0])]));
        let err = h.to_word_automaton().unwrap_err();
        assert_eq!(err, MorphismError::NumberSystemNotDefined(1));
        assert_eq!(err.to_string(), "Number system msd_1 is not defined.");
    }

    #[test]
    fn to_word_automaton_negative_value_beats_the_number_system_check() {
        // Java's statement order: `determineMaxEntry` (`:82`) throws before the
        // `NumberSystem` construction (`:90`), so a 1-uniform morphism that ALSO
        // contains a negative value reports the negative value, not `msd_1`.
        let h = Morphism::from_mapping(map(&[(0, &[-1]), (1, &[0])]));
        assert_eq!(
            h.to_word_automaton().unwrap_err(),
            MorphismError::NegativeValue
        );
    }

    #[test]
    fn to_word_automaton_number_system_check_beats_wb036() {
        // The reverse priority: WB-036's Java crash happens at WRITE time, outside
        // `toWordAutomaton`, whereas the `NumberSystem` throw is inside it. `0->5`
        // (one domain letter, maxEntry 5, so Q=6 > 1 transition row) is a WB-036 shape,
        // but its maxImageLength is 1, so real Java reports `msd_1` first.
        let h = Morphism::from_mapping(map(&[(0, &[5])]));
        assert_eq!(
            h.to_word_automaton().unwrap_err(),
            MorphismError::NumberSystemNotDefined(1)
        );
    }

    #[test]
    fn to_word_automaton_tolerates_a_domain_wider_than_the_image_range() {
        // The mirror of WB-036, which Java ACCEPTS: `0->00 1->00` references no value
        // above `0`, so `Q = maxEntry + 1 = 1` while `newD` has two rows. Java stores
        // the over-long table and every consumer just loops `0..Q`, never touching
        // row 1; this port truncates instead, which is observably identical and keeps
        // `d.len() == q`.
        let h = Morphism::from_mapping(map(&[(0, &[0, 0]), (1, &[0, 0])]));
        let p = h.to_word_automaton().unwrap();
        assert_eq!(p.fa.q, 1);
        assert_eq!(p.fa.d.len(), 1, "d.len() == q invariant restored");
        assert_eq!(p.fa.o, vec![0]);
        assert_eq!(p.fa.d[0], BTreeMap::from([(0, vec![0]), (1, vec![0])]));
    }

    #[test]
    fn to_word_automaton_rejects_negative_values() {
        let h = Morphism::from_mapping(map(&[(0, &[0, -1]), (1, &[1, 0])]));
        let err = h.to_word_automaton().unwrap_err();
        assert_eq!(err, MorphismError::NegativeValue);
        assert_eq!(
            err.to_string(),
            "Cannot promote a morphism with negative values."
        );
    }

    #[test]
    fn to_word_automaton_wb036_domain_gap_is_rejected_up_front() {
        // "0->05 1->10": two domain letters {0,1}, but the image of `0` references
        // value 5, which is not itself a domain letter -- Q = 6 but the transition
        // table has only 2 entries. Real Java doesn't reject this either; it
        // silently builds the malformed `FA` and crashes with
        // `IndexOutOfBoundsException` the first time anything touches states
        // 2..=5 -- confirmed empirically against real `walnut-java`'s `promote`
        // command. See WB-036 in `docs/WALNUT-BUGS.md`.
        let h = Morphism::from_mapping(map(&[(0, &[0, 5]), (1, &[1, 0])]));
        let err = h.to_word_automaton().unwrap_err();
        assert_eq!(err, MorphismError::DomainDoesNotCoverImageRange);
    }

    #[test]
    fn to_word_automaton_on_an_empty_mapping_is_the_number_system_error() {
        // Totality edge case beyond what real Java can reach (`parseMorphism` rejects
        // an empty mapping outright, `parse_methods.rs`'s
        // `morphism_no_valid_mappings_errors`) -- `from_mapping` makes the empty map
        // representable (see its own doc comment), so this pins what
        // `to_word_automaton` does with it rather than leaving it undefined. Following
        // Java's statement order: `maxImageLength = 0`, so the `NumberSystem("msd_0")`
        // construction at `:90` throws before WB-036's later, write-time crash on the
        // same input (`maxEntry = 0`, `Q = 1`, zero transition rows) could ever happen.
        let h = Morphism::from_mapping(BTreeMap::new());
        let err = h.to_word_automaton().unwrap_err();
        assert_eq!(err, MorphismError::NumberSystemNotDefined(0));
        assert_eq!(err.to_string(), "Number system msd_0 is not defined.");
    }

    // -- MorphismTest.testImageLength ---------------------------------------------

    #[test]
    fn test_image_length_uniform() {
        // "0->01 1->21 2->03 3->23"
        let h = Morphism::from_mapping(map(&[
            (0, &[0, 1]),
            (1, &[2, 1]),
            (2, &[0, 3]),
            (3, &[2, 3]),
        ]));
        assert_eq!(h.length, 2);
    }

    #[test]
    fn test_image_length_non_uniform() {
        // "0->0123 1->21 2->03 3->23" -- differing image lengths.
        let h = Morphism::from_mapping(map(&[
            (0, &[0, 1, 2, 3]),
            (1, &[2, 1]),
            (2, &[0, 3]),
            (3, &[2, 3]),
        ]));
        assert_eq!(h.length, -1);
    }

    // -- MorphismTest.testBigAlphabet ---------------------------------------------

    #[test]
    fn test_big_alphabet() {
        // "0->01 [11]->012 [12]->02"
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (11, &[0, 1, 2]), (12, &[0, 2])]));
        assert_eq!(h.length, -1); // not uniform
        assert_eq!(h.range.len(), 3); // {0, 1, 2}
        assert_eq!(h.mapping.get(&0).unwrap().len(), 2); // [0, 1]
        assert_eq!(h.mapping.get(&10), None);
        assert_eq!(h.mapping.get(&11).unwrap().len(), 3); // [0, 1, 2]
        assert_eq!(h.mapping.get(&12).unwrap().len(), 2); // [0, 2]
    }

    // -- edge cases beyond the Java suite ------------------------------------------

    #[test]
    fn empty_mapping_has_length_zero_and_empty_range() {
        // Documents the TOTALITY of the split constructor -- NOT a supported
        // "empty morphism" feature. In Java this state is unreachable: the only
        // path into `Morphism`'s fields is the `Morphism(String)` constructor,
        // which calls `ParseMethods.parseMorphism`, which throws
        // `WalnutException("Morphism has no valid mappings.")` on an empty result
        // (`ParseMethods.java:189-191`, ported as `parse_morphism`'s
        // `morphism_no_valid_mappings_errors`). Splitting that constructor in two
        // (parsing in `wr-io`, this in `wr-core` -- see the module docs) makes the
        // empty map *representable* here, so this pins what `from_mapping` does
        // with it rather than leaving it undefined: exactly what Java's
        // `determineUniformLength` would do, i.e. `firstElement` never flips and
        // `imageLength` stays 0. No `debug_assert!` is used to reject it -- this
        // crate's stated convention (see `fa.rs`/`minimize.rs`/`equiv.rs` module
        // docs) is that preconditions are hard errors or documented contracts,
        // never `debug_assert!`, which vanishes in release builds.
        let h = Morphism::from_mapping(BTreeMap::new());
        assert_eq!(h.length, 0);
        assert!(h.range.is_empty());
        assert!(h.mapping.is_empty());
    }

    #[test]
    fn uniform_zero_length_when_every_image_is_empty() {
        // "0 ->" / "1 ->" -- valid per `parse_morphism`'s own test coverage
        // (`parse_methods.rs`'s `morphism_empty_image_is_allowed`), each image empty.
        let h = Morphism::from_mapping(map(&[(0, &[]), (1, &[])]));
        assert_eq!(h.length, 0);
        assert!(h.range.is_empty());
    }

    #[test]
    fn require_positive_uniform_length_rejects_non_uniform() {
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1])]));
        assert_eq!(h.length, -1);
        let err = h.require_positive_uniform_length().unwrap_err();
        assert_eq!(err, MorphismError::NotUniform);
        assert_eq!(
            err.to_string(),
            "A morphism applied to a word automaton must be uniform."
        );
    }

    #[test]
    fn require_positive_uniform_length_rejects_zero_length() {
        let h = Morphism::from_mapping(map(&[(0, &[]), (1, &[])]));
        let err = h.require_positive_uniform_length().unwrap_err();
        assert_eq!(err, MorphismError::NotPositiveUniform);
        assert_eq!(
            err.to_string(),
            "A morphism applied to a word automaton must have positive uniform length."
        );
    }

    #[test]
    fn require_positive_uniform_length_accepts_positive_uniform() {
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0])]));
        assert!(h.require_positive_uniform_length().is_ok());
    }

    // -- escapedInt / write --------------------------------------------------------

    #[test]
    fn escaped_int_brackets_outside_zero_through_nine() {
        assert_eq!(escaped_int(0), "0");
        assert_eq!(escaped_int(9), "9");
        assert_eq!(escaped_int(10), "[10]");
        assert_eq!(escaped_int(-3), "[-3]");
    }

    #[test]
    fn write_round_trips_escaping_and_order() {
        // Mirrors the class doc's own example: 0 -> [-3]0102[11], 1 -> 2113, 2 -> 314.
        let h = Morphism::from_mapping(map(&[
            (0, &[-3, 0, 1, 0, 2, 11]),
            (1, &[2, 1, 1, 3]),
            (2, &[3, 1, 4]),
        ]));
        let mut out = Vec::new();
        h.write(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "0 -> [-3]0102[11]\n1 -> 2113\n2 -> 314\n");
    }

    #[test]
    fn write_escapes_out_of_range_domain_keys_too() {
        // As of Walnut 8 the square-bracket escape applies to the DOMAIN letter as
        // well as the image (`Morphism.java:38`'s class doc; `write` calls
        // `escapedInt` on `entry.getKey()` at `:65`, not just on the image at
        // `:67`). Every other `write` test here uses keys in 0..=9, where
        // `escapedInt` is the identity and the key-side escaping is invisible --
        // this one makes dropping it a test failure.
        //
        // Also pins the on-disk round trip: the text below is exactly what
        // `wr_io::parse_methods::parse_morphism` accepts back (`[-3] -> ...`), so
        // the Morphism Library format survives a write/read cycle for such keys.
        // And it pins `BTreeMap` key order: -3 sorts before 11 numerically, which
        // is NOT the order they would take as strings.
        let h = Morphism::from_mapping(map(&[(-3, &[0, 11]), (11, &[1]), (2, &[9, -1])]));
        let mut out = Vec::new();
        h.write(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "[-3] -> 0[11]\n2 -> 9[-1]\n[11] -> 1\n");
    }

    #[test]
    fn write_to_file_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "wr-core-morphism-write-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("h.txt");
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0])]));
        h.write_to_file(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "0 -> 01\n1 -> 10\n");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    // -- makeInterPredicate ---------------------------------------------------------

    #[test]
    fn make_inter_predicate_thue_morse_value_zero() {
        // h = {0 -> 01, 1 -> 10}, length 2. Looking for where value 0 occurs.
        // For letter 0 (image "01"): 0 occurs at position 0 -> "L[q]= @0 => (r=0))".
        // For letter 1 (image "10"): 0 occurs at position 1 -> "L[q]= @1 => (r=1))".
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0])]));
        let predicate = h.make_inter_predicate(0, "L", "?msd_2");
        assert_eq!(
            predicate,
            "?msd_2 E q, r (n=2*q+r & r>=0 & r<2 & (L[q]= @0 => (r=0)) & (L[q]= @1 => (r=1)))"
        );
    }

    #[test]
    fn make_inter_predicate_value_absent_from_every_image() {
        // Value 5 appears in no image at all -- every clause takes the "!=" arm.
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0])]));
        let predicate = h.make_inter_predicate(5, "L", "?msd_2");
        assert_eq!(
            predicate,
            "?msd_2 E q, r (n=2*q+r & r>=0 & r<2 & (L[q]!= @0) & (L[q]!= @1))"
        );
    }

    #[test]
    fn make_inter_predicate_value_repeated_within_one_image_ors_positions() {
        // letter 0 -> [1, 1, 0]: value 1 occurs at both position 0 and 1, so the
        // clause should OR them ("r=0|r=1"), not just take the first.
        let h = Morphism::from_mapping(map(&[(0, &[1, 1, 0])]));
        let predicate = h.make_inter_predicate(1, "L", "?msd_3");
        assert_eq!(
            predicate,
            "?msd_3 E q, r (n=3*q+r & r>=0 & r<3 & (L[q]= @0 => (r=0|r=1)))"
        );
    }

    #[test]
    fn make_inter_predicate_emits_out_of_range_keys_unescaped() {
        // The escaped/raw ASYMMETRY between the two `mapping`-reading methods,
        // pinned. `write` runs every key through `escapedInt` (`Morphism.java:65`),
        // but `makeInterPredicate` appends `entry.getKey()` RAW (`:170`, `:180`) --
        // it is generating predicate source for `Predicate`, not Morphism Library
        // text, and `Predicate.PATTERN_FOR_ALPHABET_LETTER`
        // (`Main/Predicate.java:105`, `@(\s*([+\-])?\s*\d+)`) matches a bare signed
        // integer after `@`, with no bracket syntax. So `@11`, never `@[11]`.
        // Faithful to Java, and previously unpinned: every other predicate test
        // here uses keys in 0..=9, where escaped and raw coincide.
        let h = Morphism::from_mapping(map(&[(11, &[2, 0]), (12, &[0, 2])]));
        let predicate = h.make_inter_predicate(2, "L", "?msd_2");
        assert_eq!(
            predicate,
            "?msd_2 E q, r (n=2*q+r & r>=0 & r<2 & (L[q]= @11 => (r=0)) & (L[q]= @12 => (r=1)))"
        );
        // The "!=" arm takes the key raw as well (`:180`).
        let absent = h.make_inter_predicate(7, "L", "?msd_2");
        assert_eq!(
            absent,
            "?msd_2 E q, r (n=2*q+r & r>=0 & r<2 & (L[q]!= @11) & (L[q]!= @12))"
        );
    }

    #[test]
    fn make_inter_predicate_emits_negative_domain_letters_unescaped() {
        // "[-3]->[11]0 [12]->0[11]" -- a NEGATIVE domain letter, which
        // `parse_morphism` accepts (bracketed) and which `makeInterPredicate` emits
        // as `@-3`, unescaped, same as any other key. That is legitimate predicate
        // syntax rather than a Walnut bug: `PATTERN_FOR_ALPHABET_LETTER`
        // (`Main/Predicate.java:105`) explicitly allows an optional `[+\-]` sign.
        // Note `-3` sorts before `12`, so its clause comes first.
        let h = Morphism::from_mapping(map(&[(-3, &[11, 0]), (12, &[0, 11])]));
        let predicate = h.make_inter_predicate(11, "L", "?msd_2");
        assert_eq!(
            predicate,
            "?msd_2 E q, r (n=2*q+r & r>=0 & r<2 & (L[q]= @-3 => (r=0)) & (L[q]= @12 => (r=1)))"
        );
    }
}
