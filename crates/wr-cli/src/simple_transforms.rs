// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Prover.java`'s inline `minimizeCommand` (`:712-722`), `fixLeadZeroCommand`
//! (`:747-753`), `fixTrailZeroCommand` (`:756-762`) — U23, batch A. Three read-mutate-write
//! triples, each over a single already-ported primitive:
//! [`wr_core::word_automaton::minimize_self_with_output`],
//! [`wr_core::logicalops::fix_leading_zeros_problem`],
//! [`wr_core::logicalops::fix_trailing_zeros_problem`]. Grouped into one module (rather
//! than three) because, unlike the `Main/Commands/*.java` files, these never had their
//! own Java class or file to begin with — they are inline `Prover` methods, and this
//! module is their equally small Rust home.

use wr_core::logicalops::{fix_leading_zeros_problem, fix_trailing_zeros_problem};
use wr_core::numsys::TXT_EXTENSION;
use wr_core::word_automaton::minimize_self_with_output;
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_ops::read_from_automata_library;
use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure the three commands in this module can produce.
#[derive(Debug)]
pub enum SimpleTransformError {
    /// The input automaton couldn't be read.
    Read(PredicateEnvError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
}

impl std::fmt::Display for SimpleTransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimpleTransformError::Read(e) => write!(f, "{e}"),
            SimpleTransformError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SimpleTransformError {}

impl From<std::io::Error> for SimpleTransformError {
    fn from(e: std::io::Error) -> Self {
        SimpleTransformError::Io(e)
    }
}

/// `Prover.minimizeCommand(String s)` (`Prover.java:712-722`). Reads/writes
/// `Word Automata Library/`, not `Automata Library/` — the one command in this module
/// that does, matching Java's `Session.getReadFileForWordsLibrary`/
/// `getWriteAddressForWordsLibrary` exactly.
pub fn minimize_command(
    session: &Session,
    s: &str,
    old_name: &str,
    new_name: &str,
) -> Result<TestCase, SimpleTransformError> {
    let address = session
        .paths()
        .read_file_for_words_library(&format!("{old_name}{TXT_EXTENSION}"));
    let mut m = session
        .libraries()
        .read_library_automaton(&address)
        .map_err(SimpleTransformError::Read)?;

    // `WordAutomaton.minimizeSelfWithOutput(M);` (`:718`).
    minimize_self_with_output(&mut m);

    write_automata(
        session,
        &mut m,
        s,
        &session.paths().write_address_for_words_library(),
        new_name,
        true,
    )?;
    Ok(TestCase::from_automaton(m))
}

/// `Prover.fixLeadZeroCommand(String s)` (`Prover.java:747-753`).
pub fn fix_lead_zero_command(
    session: &Session,
    s: &str,
    old_name: &str,
    new_name: &str,
) -> Result<TestCase, SimpleTransformError> {
    let mut m =
        read_from_automata_library(session, old_name).map_err(SimpleTransformError::Read)?;

    // `AutomatonLogicalOps.fixLeadingZerosProblem(M);` (`:750`).
    fix_leading_zeros_problem(&mut m);

    write_automata(
        session,
        &mut m,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(m))
}

/// `Prover.fixTrailZeroCommand(String s)` (`Prover.java:756-762`).
pub fn fix_trail_zero_command(
    session: &Session,
    s: &str,
    old_name: &str,
    new_name: &str,
) -> Result<TestCase, SimpleTransformError> {
    let mut m =
        read_from_automata_library(session, old_name).map_err(SimpleTransformError::Read)?;

    // `AutomatonLogicalOps.fixTrailingZerosProblem(M);` (`:759`).
    fix_trailing_zeros_problem(&mut m);

    write_automata(
        session,
        &mut m,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::automaton::Automaton;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-simple-transforms-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
        for sub in [
            "Result",
            "Automata Library",
            "Word Automata Library",
            "Custom Bases",
            "Macro Library",
            "Morphism Library",
        ] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let dir_str = format!("{}/", dir.to_str().unwrap());
        let session = Session::new(Some(&dir_str), Some(&dir_str), false);
        (session, dir)
    }

    /// Non-minimal (two equivalent accepting states) `msd_2` predicate automaton
    /// accepting exactly the words containing a `1`.
    fn non_minimal_contains_one() -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0]);
        d0.insert(1, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(0, vec![2]);
        d1.insert(1, vec![1]);
        let mut d2 = BTreeMap::new();
        d2.insert(0, vec![2]);
        d2.insert(1, vec![1]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 3,
                alphabet_size: 2,
                o: vec![0, 1, 1],
                d: vec![d0, d1, d2],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    #[test]
    fn minimize_command_shrinks_a_redundant_word_automaton() {
        let (session, dir) = temp_session("minimize");
        let path = dir.join("Word Automata Library").join("A.txt");
        let mut a = non_minimal_contains_one();
        wr_io::writer::write_automaton_txt(&mut a, &path).unwrap();

        let tc = minimize_command(&session, "minimize c A;", "A", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.q <= 2, "states 1 and 2 are language-equivalent");
        assert!(c.fa.accepts_word(&[0, 1, 0]));
        assert!(!c.fa.accepts_word(&[0, 0, 0]));
        assert!(dir.join("Word Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fix_lead_zero_command_closes_under_prepended_zeros() {
        let (session, dir) = temp_session("fixlead");
        // Accepts exactly "1" -- a leading-zero fixup must additionally accept "01",
        // "001", etc.
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![1]);
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 2,
                o: vec![0, 1],
                d: vec![d0, BTreeMap::new()],
            },
            vec![vec![0, 1]],
            vec!["x".to_string()],
            vec![Some(true)],
        );
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("A.txt"))
            .unwrap();

        let tc = fix_lead_zero_command(&session, "fixleadzero c A;", "A", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[1]));
        assert!(c.fa.accepts_word(&[0, 1]));
        assert!(c.fa.accepts_word(&[0, 0, 1]));
        assert!(!c.fa.accepts_word(&[1, 0]));
        assert!(dir.join("Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fix_trail_zero_command_admits_a_word_whose_zero_padding_was_already_accepted() {
        // `fix_trailing_zeros_problem` is NOT a forward "append a zero, stay accepted"
        // closure (that is `fix_leading_zeros_problem`'s prepend-zero shape, on the
        // other end) -- it marks a state accepting whenever it can REACH an already-
        // accepting state via a chain of zero transitions, i.e. `L(fixed) = { w : ∃k≥0,
        // w·0^k ∈ L(original) }` (a right quotient by `0*`). So build an automaton
        // accepting exactly "10"; the fixup must then ALSO accept "1" (since
        // "1"+"0" = "10" ∈ L(original)), while leaving "10" itself accepted and adding
        // nothing else.
        let (session, dir) = temp_session("fixtrail");
        let mut d0 = BTreeMap::new();
        d0.insert(1, vec![1]);
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
            vec!["x".to_string()],
            vec![Some(true)],
        );
        wr_io::writer::write_automaton_txt(&mut a, dir.join("Automata Library").join("A.txt"))
            .unwrap();

        let tc = fix_trail_zero_command(&session, "fixtrailzero c A;", "A", "c").unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(c.fa.accepts_word(&[1, 0]), "\"10\" stays accepted");
        assert!(
            c.fa.accepts_word(&[1]),
            "\"1\" must gain acceptance: \"1\"+\"0\" = \"10\" was already accepted"
        );
        assert!(
            !c.fa.accepts_word(&[]),
            "the empty word must still be rejected"
        );
        assert!(!c.fa.accepts_word(&[0, 1]), "\"01\" must still be rejected");
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------- Tier-4 property (Phase 4, U31)

    /// Random single-track partial DFAs over `{0,1}` — the shape a `.txt` in
    /// `Automata Library/` can hold, including genuinely MISSING transitions (the case
    /// that makes both fixups' behaviour observable at all).
    ///
    /// Deterministic on purpose: `fixtrailzero` bottoms out in `FA.justMinimize`, whose
    /// `convertNFAtoDFA()` step rejects genuine nondeterminism with `"Unexpected NFA
    /// instead of DFA."` in Java and in this port alike, so an NFA operand would exercise
    /// that ported rejection rather than the closure property under test.
    fn arb_partial_dfa(q_max: usize) -> impl Strategy<Value = Automaton> {
        (1..=q_max).prop_flat_map(move |q| {
            let o = prop::collection::vec(0i32..=1, q);
            let trans =
                prop::collection::vec(prop::collection::vec(prop::option::of(0usize..q), 2), q);
            (o, trans).prop_map(move |(o, trans)| {
                let d = trans
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .enumerate()
                            .filter_map(|(sym, dest)| dest.map(|t| (sym as i32, vec![t])))
                            .collect::<BTreeMap<i32, Vec<usize>>>()
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

    /// Removes a `temp_session`'s directory tree when it goes out of scope, **including
    /// on an unwinding panic**.
    ///
    /// The plain `fs::remove_dir_all(&dir).ok()` at the end of a `#[test]` body never runs
    /// if the body panics, so a single failing `proptest!` case used to leak the whole
    /// tree into `std::env::temp_dir()`. Every test here keeps its trailing explicit
    /// cleanup for the happy path (unchanged); this guard is what covers the failing one.
    struct TempDirGuard(std::path::PathBuf);

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    /// The EXACT language `fix_leading_zeros_problem` produces, decided from the operand's
    /// OWN transition table by a hand-rolled subset-construction walk — an independent
    /// re-derivation of `wr_core::logicalops`'s oracle of the same shape (that one is
    /// `#[cfg(test)]`-private to its crate, so it cannot be shared).
    ///
    /// The primitive is `determinizeAndMinimize(IntSet)` — subset construction, then
    /// Valmari, both language-preserving — run from `Z0` over the table `A'` = `A` with
    /// the one `(q0, zero) -> q0` edge `zeroReachableStates` force-writes in, where `Z0`
    /// is `q0`'s `zero*`-closure in `A'`. So `L(fixed) = { w : δ_{A'}(Z0, w) ∩ F ≠ ∅ }`.
    fn lead_zero_oracle(a: &Automaton, zero: i32, word: &[i32]) -> bool {
        let fa = &a.fa;
        let step = |cur: &std::collections::BTreeSet<usize>, sym: i32| {
            let mut next = std::collections::BTreeSet::new();
            for &s in cur {
                if let Some(dests) = fa.d[s].get(&sym) {
                    next.extend(dests.iter().copied());
                }
                if s == fa.q0 && sym == zero {
                    next.insert(fa.q0);
                }
            }
            next
        };
        let mut z0 = std::collections::BTreeSet::from([fa.q0]);
        loop {
            let before = z0.len();
            let grown = step(&z0, zero);
            z0.extend(grown);
            if z0.len() == before {
                break;
            }
        }
        let mut cur = z0;
        for &sym in word {
            cur = step(&cur, sym);
        }
        cur.iter().any(|&s| fa.is_accepting(s))
    }

    /// The EXACT language `fix_trailing_zeros_problem` produces: a right quotient by
    /// `zero*`, `L(fixed) = { w : ∃k ≥ 0, w·0^k ∈ L(A) }`. The fixup only widens the
    /// accepting set, so this needs no table mutation — run `w` on the operand, then ask
    /// a from-scratch zero-edge BFS whether any state so reached can reach acceptance on
    /// zeros. No length bound to guess, and no call to the fixup.
    fn trail_zero_oracle(a: &Automaton, zero: i32, word: &[i32]) -> bool {
        let fa = &a.fa;
        let mut cur = std::collections::BTreeSet::from([fa.q0]);
        for &sym in word {
            let mut next = std::collections::BTreeSet::new();
            for &s in &cur {
                if let Some(dests) = fa.d[s].get(&sym) {
                    next.extend(dests.iter().copied());
                }
            }
            cur = next;
        }
        let mut seen = cur.clone();
        let mut queue: std::collections::VecDeque<usize> = cur.into_iter().collect();
        while let Some(s) = queue.pop_front() {
            if fa.is_accepting(s) {
                return true;
            }
            if let Some(dests) = fa.d[s].get(&zero) {
                for &t in dests {
                    if seen.insert(t) {
                        queue.push_back(t);
                    }
                }
            }
        }
        false
    }

    /// Tier-4 (`CLAUDE.md`'s correctness ladder, `docs/DESIGN.md` §5) at the COMMAND
    /// level: the two standalone zero-fixup commands compute exactly what their primitives
    /// promise, end to end — through the real `Automata Library/` read, the primitive, and
    /// the write-back — not just when the primitive is called in isolation.
    ///
    /// The load-bearing assertions are the two **equalities**, checked word-for-word over
    /// every word of length `0..=4` (an exhaustive 31-word sweep, not a sample) against
    /// [`lead_zero_oracle`] / [`trail_zero_oracle`], each decided from the operand the
    /// command itself read back off disk:
    ///
    /// * `fixleadzero`: `L(c) = { w : δ_{A'}(Z0, w) ∩ F ≠ ∅ }`;
    /// * `fixtrailzero`: `L(d) = { w : ∃k ≥ 0, w·0^k ∈ L(A) }`.
    ///
    /// An earlier draft asserted only one-sided closure/containment statements. That was
    /// vacuous against the most likely failure mode — mutation-tested and confirmed:
    /// replacing either primitive with "return the 1-state `Σ*` automaton" satisfied every
    /// one of them. Neither survives the equalities.
    ///
    /// The closure corollaries are kept below because they are the statements a reader of
    /// these two commands actually wants, and they are deliberately NOT mirror images of
    /// each other, because the fixups are not (see `fix_trailing_zeros_problem`'s doc
    /// comment): `fixleadzero` closes the language under a leading zero in BOTH
    /// directions and loses nothing already accepted, while `fixtrailzero`'s result is
    /// closed under REMOVING a trailing zero and NOT under adding one — asserting the
    /// mirror of the first here would be wrong, not merely unproven.
    ///
    /// # The operand is TRIMMED before it is written to disk
    ///
    /// `fixtrailzero` closes with `FA.justMinimize`, which — faithfully — does not trim,
    /// so an operand carrying a state unreachable from `q0` that also cannot reach
    /// acceptance hands Valmari a table violating its reachability precondition and
    /// triggers `docs/WALNUT-BUGS.md` WB-001, a deliberately ported quirk that would show
    /// up here as an oracle disagreement. `crate::trim::trim` is language-preserving and
    /// its postcondition is exactly that precondition, so trimming the generated operand
    /// before writing it constrains the generator's domain rather than weakening the
    /// oracle. (`fixleadzero` needs no such care: it closes with
    /// `determinizeAndMinimize(IntSet)`, whose subset construction re-establishes
    /// reachability. It is trimmed anyway, so both commands see the same operand and the
    /// two equalities are about one table.) An earlier version of this test claimed the
    /// trimming without doing it.
    #[test]
    fn fix_zero_commands_establish_their_closure_properties_end_to_end() {
        let (session, dir) = temp_session("zero-closure-props");
        let _guard = TempDirGuard(dir.clone());
        let path = dir.join("Automata Library").join("A.txt");
        proptest!(
            ProptestConfig::with_cases(24),
            |(a in arb_partial_dfa(3), probe in prop::collection::vec(0i32..2, 0..4))| {
                let mut original = a;
                original.fa = wr_core::trim::trim(&original.fa);
                wr_io::writer::write_automaton_txt(&mut original, &path).unwrap();

                // The operand exactly as the commands themselves see it, so the oracles
                // reason about the table the fixups actually ran on rather than about a
                // pre-write copy the `.txt` round trip might have normalized.
                let operand = read_from_automata_library(&session, "A")
                    .expect("the file was just written by this test");
                let zero = operand.determine_zero();

                let lead = fix_lead_zero_command(&session, "fixleadzero c A;", "A", "c")
                    .expect("fixleadzero must succeed on a well-formed operand");
                let lead = lead.automaton_pairs()[0].automaton().unwrap().clone();
                for w in all_words_up_to_four() {
                    prop_assert_eq!(
                        lead.fa.accepts_word(&w),
                        lead_zero_oracle(&operand, zero, &w),
                        "fixleadzero disagrees with subset construction from Z0 on {:?}", w
                    );
                }

                let mut padded = vec![0];
                padded.extend_from_slice(&probe);
                prop_assert_eq!(
                    lead.fa.accepts_word(&probe),
                    lead.fa.accepts_word(&padded),
                    "fixleadzero: not closed under a leading zero at {:?}", probe
                );
                if original.fa.accepts_word(&probe) {
                    prop_assert!(
                        lead.fa.accepts_word(&probe),
                        "fixleadzero dropped {:?}, which the operand already accepted", probe
                    );
                }

                let trail = fix_trail_zero_command(&session, "fixtrailzero d A;", "A", "d")
                    .expect("fixtrailzero must succeed on a well-formed operand");
                let trail = trail.automaton_pairs()[0].automaton().unwrap().clone();
                for w in all_words_up_to_four() {
                    prop_assert_eq!(
                        trail.fa.accepts_word(&w),
                        trail_zero_oracle(&operand, zero, &w),
                        "fixtrailzero disagrees with the right quotient by 0* on {:?}", w
                    );
                }

                let mut with_zero = probe.clone();
                with_zero.push(0);
                if trail.fa.accepts_word(&with_zero) {
                    prop_assert!(
                        trail.fa.accepts_word(&probe),
                        "fixtrailzero: {:?}0 accepted but {:?} is not", probe, probe
                    );
                }
                if original.fa.accepts_word(&with_zero) {
                    prop_assert!(
                        trail.fa.accepts_word(&probe),
                        "fixtrailzero: {:?}0 was accepted by the operand, so {:?} must be \
                         accepted by the result", probe, probe
                    );
                }
            }
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// Every `{0,1}` word of length `0..=4` — the 31-word sweep both equalities above are
    /// checked over.
    fn all_words_up_to_four() -> Vec<Vec<i32>> {
        let mut out = vec![Vec::new()];
        let mut frontier = vec![Vec::new()];
        for _ in 0..4 {
            let mut next = Vec::new();
            for w in &frontier {
                for d in [0i32, 1] {
                    let mut w2 = w.clone();
                    w2.push(d);
                    next.push(w2);
                }
            }
            out.extend(next.iter().cloned());
            frontier = next;
        }
        out
    }

    #[test]
    fn fix_lead_zero_command_propagates_a_missing_file_read_error() {
        let (session, dir) = temp_session("missing");
        let err = fix_lead_zero_command(&session, "fixleadzero c A;", "A", "c").unwrap_err();
        assert!(matches!(err, SimpleTransformError::Read(_)));
        fs::remove_dir_all(&dir).ok();
    }
}
