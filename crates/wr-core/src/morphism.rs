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
//! # Not ported: `toWordAutomaton` (`:78-107`) and its private helpers
//!
//! `determineTransitions`/`determineMaxEntry`/`determineMaxImageLength` (`:95-132`)
//! exist solely to serve `toWordAutomaton`, which promotes a `Morphism` into a
//! `WordAutomaton`-shaped `Automaton` (one state per domain letter, one transition
//! per image position). Per this unit's explicit scope (it is "WordAutomaton-from-
//! morphism functionality," grouped with `buildTransitionsFromMorphism`/`convertNS`
//! as later, `Automaton`-consuming work — see `logicalops.rs`'s "Not ported"
//! section for the sibling helpers), it is deferred to a follow-on unit rather than
//! built here. Note for that unit: unlike the other entries in `logicalops.rs`'s
//! "Not ported" list, this is **not** blocked on a missing primitive —
//! `Automaton::richAlphabet` (`automaton.rs`), `Fa::set_fields` (`fa.rs`), and
//! `NumberSystem::MSD_UNDERSCORE`/its `msd_<k>` constructor (`numsys.rs`) all
//! already exist in this crate as of this unit; the split is purely to keep this
//! unit's surface to the standalone data primitive `Morphism` itself.
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
//! No genuine Walnut bug (wrong output / crash on a plausible, contract-respecting
//! input) was found while porting this file — unlike some `Automata/` files, there
//! is no unguarded `mapping.get(letter)`-style lookup on a letter outside the
//! morphism's domain anywhere in `Morphism.java`; the only two places that read
//! `mapping` (`write`, `makeInterPredicate`) both iterate `entrySet()` rather than
//! index into it by an externally supplied letter.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// `WalnutException.morphismNotUniform()` / the inline `WalnutException` at
/// `Morphism.java:193` — both thrown (as unchecked exceptions) by
/// `requirePositiveUniformLength` (`:188-195`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MorphismError {
    /// `length < 0` (non-uniform morphism). `WalnutException.morphismNotUniform()`
    /// (`WalnutException.java:85`).
    NotUniform,
    /// `length == 0` (uniform, but every image is empty). The inline
    /// `WalnutException` at `Morphism.java:193`.
    NotPositiveUniform,
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
    /// here (only `.size()` and membership are read off it directly; every
    /// iteration site, e.g. `Image.image`, immediately copies-and-sorts before
    /// iterating) — `BTreeSet` is a strictly stronger (sorted) guarantee, not a
    /// behavior change.
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
    expected as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(i32, &[i32])]) -> BTreeMap<i32, Vec<i32>> {
        pairs.iter().map(|(k, v)| (*k, v.to_vec())).collect()
    }

    // -- MorphismTest.testGamMorphism (partial: the file-backed length/mapping
    // shape, not `toWordAutomaton` which this unit defers) -----------------------

    #[test]
    fn thue_morse_morphism_is_uniform_length_two() {
        // The classic Thue-Morse morphism: 0 -> 01, 1 -> 10.
        let h = Morphism::from_mapping(map(&[(0, &[0, 1]), (1, &[1, 0])]));
        assert_eq!(h.length, 2);
        assert_eq!(h.mapping.get(&0), Some(&vec![0, 1]));
        assert_eq!(h.mapping.get(&1), Some(&vec![1, 0]));
        assert_eq!(h.range, BTreeSet::from([0, 1]));
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
        // Not reachable via the real `wr_io::parse_methods::parse_morphism` (it
        // errors on an empty mapping), but `from_mapping` itself, like Java's
        // `determineUniformLength`, is total and returns 0/empty here -- pinned
        // since `from_mapping`'s contract is "already-parsed map", not
        // "necessarily non-empty".
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
}
