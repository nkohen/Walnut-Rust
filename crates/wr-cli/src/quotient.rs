// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Commands/Quotient.java` (25 LOC) — U23, batch A. `rightquo`/`leftquo`, both a
//! read-read-compute-write triple over the already-ported
//! [`wr_core::logicalops::right_quotient`]/[`wr_core::logicalops::left_quotient`].

use wr_core::automaton::Automaton;
use wr_core::logging::Logging;
use wr_core::logicalops::{left_quotient, right_quotient};
use wr_logic::predicate_env::PredicateEnvError;

use crate::automaton_ops::read_from_automata_library;
use crate::automaton_output::write_automata;
use crate::session::Session;
use crate::test_case::TestCase;

/// Every failure [`right_quotient_command`]/[`left_quotient_command`] can produce.
#[derive(Debug)]
pub enum QuotientError {
    /// `Automaton.readAutomatonFromFile` failed for either operand.
    Read(PredicateEnvError),
    /// See `crate::automaton_output::write_automata`'s docs for why this propagates
    /// rather than being swallowed-and-logged the way Java's `writeAutomata` is.
    Io(std::io::Error),
    /// One of `AutomatonLogicalOps.rightQuotient`/`leftQuotient`'s subset-alphabet
    /// `WalnutException`s, ported in `wr-core` as an `assert!` and recovered at this
    /// crate's boundary — see [`crate::walnut_exception::catch_walnut_panic`]. Message
    /// verbatim from `wr-core`'s guard, which is verbatim from Java's.
    Walnut(String),
    /// Any OTHER panic escaping the quotient primitive, standing in for an uncaught Java
    /// `RuntimeException` that is not a `WalnutException` — hence rendered by
    /// `Prover`'s handler with a stack-trace header rather than message-only.
    ///
    /// **`left_quotient_command`'s WB-010 instance of this variant is now closed**
    /// (`docs/WALNUT-BUGS.md`, `walnut-java` commit `c5ff914` on `bugfix/wb-010`,
    /// matched here in `wr_core::logicalops::left_quotient`): `leftQuotient` used to
    /// check its subset guard in the wrong direction, letting a genuinely-mismatched
    /// pair slip past it to die later inside `RichAlphabet.encode`. With the fix,
    /// `left_quotient`'s own guard now catches that exact shape (`B`'s alphabet ⊄ `A`'s)
    /// before the re-encode ever runs, reporting it as [`QuotientError::Walnut`]
    /// instead. `left_quotient_command`'s internal `reverse_and_canonize` step also
    /// determinizes both operands before the re-encode runs, which independently wipes
    /// WB-038's bogus out-of-alphabet `-1` encoding key if either operand carries one —
    /// so that trigger is closed on this path too, not just WB-010's.
    ///
    /// **`right_quotient_command`'s WB-038 instance is now closed as well**, one PR later
    /// (`walnut-java` commit `601a9d2` on `bugfix/wb-038`, matched here in
    /// `wr_io::reader::validate_transition`). That trigger — found by adversarial review
    /// of the WB-010 fix — worked because `AutomatonReader` encoded an out-of-alphabet
    /// transition digit to a bogus `-1` key, and `rightquo` called directly (not through
    /// `leftQuotient`'s delegation, which determinizes first) carried it all the way into
    /// `right_quotient`'s own re-encode. The reader now refuses such a file outright, so
    /// the operand never loads: the same command reports a read error instead, on both
    /// engines. `right_quotient_rejects_wb_038s_file_before_it_can_reach_the_re_encode`
    /// below is that trigger's test, flipped to the fixed behavior.
    ///
    /// **This variant therefore has no known live trigger today**, and is deliberately
    /// kept anyway rather than removed: it is the port of Java's *unclassified*
    /// `RuntimeException` arm (`Prover.readBuffer`'s `catch`, and this crate's own
    /// [`crate::walnut_exception::catch_walnut_panic`] wrapper), whose job is to give
    /// whatever the NEXT such panic turns out to be the right rendering — a stack-trace
    /// header rather than the message-only treatment a `WalnutException` gets. Removing
    /// it would mean an unexpected panic in the quotient primitive either escaping to
    /// kill the process or being mis-rendered as a Walnut-level message. Flipping the one
    /// end-to-end test that used to reach it would have left
    /// `QuotientError::from_panic`'s classification with no coverage at all, so that
    /// function gained a direct unit test in the same change
    /// (`from_panic_classifies_only_the_two_walnut_messages_as_walnut`).
    Runtime(String),
}

impl QuotientError {
    /// Classifies a caught panic message: the two ported `WalnutException` texts are
    /// [`QuotientError::Walnut`] (message-only in Java's handler), anything else is
    /// [`QuotientError::Runtime`] (stack-trace header).
    fn from_panic(message: String) -> Self {
        if message == RIGHT_QUOTIENT_SUBSET_MESSAGE || message == LEFT_QUOTIENT_SUBSET_MESSAGE {
            QuotientError::Walnut(message)
        } else {
            QuotientError::Runtime(message)
        }
    }

    /// Java's handler treats a `WalnutException` as message-only and anything else as a
    /// stack-trace-headed report; `crate::prover::ProverError` delegates here for that
    /// triage.
    pub(crate) fn is_walnut_exception(&self) -> bool {
        !matches!(self, QuotientError::Runtime(_))
    }
}

/// `AutomatonLogicalOps.rightQuotient`'s guard message, as `wr_core::logicalops` spells it.
const RIGHT_QUOTIENT_SUBSET_MESSAGE: &str =
    "Second A's alphabet must be a subset of the first A's alphabet for right quotient.";
/// `AutomatonLogicalOps.leftQuotient`'s guard message, as `wr_core::logicalops` spells
/// it. Direction fixed by WB-010 (`docs/WALNUT-BUGS.md`, `walnut-java` commit `c5ff914`
/// on `bugfix/wb-010`): "second" (`B`), not "first" (`A`).
const LEFT_QUOTIENT_SUBSET_MESSAGE: &str =
    "Second A's alphabet must be a subset of the first A's alphabet for left quotient.";

impl std::fmt::Display for QuotientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuotientError::Read(e) => write!(f, "{e}"),
            QuotientError::Io(e) => write!(f, "{e}"),
            QuotientError::Walnut(m) | QuotientError::Runtime(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for QuotientError {}

impl From<std::io::Error> for QuotientError {
    fn from(e: std::io::Error) -> Self {
        QuotientError::Io(e)
    }
}

fn read_pair(
    session: &Session,
    old_name1: &str,
    old_name2: &str,
) -> Result<(Automaton, Automaton), QuotientError> {
    let m1 = read_from_automata_library(session, old_name1).map_err(QuotientError::Read)?;
    let m2 = read_from_automata_library(session, old_name2).map_err(QuotientError::Read)?;
    Ok((m1, m2))
}

/// `Quotient.rightQuotient(String s, String oldName1, String oldName2, String newName)`
/// (`Quotient.java:9-15`).
///
/// `right_quotient`'s third parameter (`skip_subset_check`) is hardcoded `false` here,
/// matching Java's own `AutomatonLogicalOps.rightQuotient(M1, M2, false)` call — the real,
/// non-`skip` subset-alphabet guard runs.
///
/// `logging` is the caller's real, already-`configure_for_command`-d
/// [`Logging`] (`Prover`'s `self.logging`) — `rightquo`/`leftquo` are not `eval`/`def`
/// commands, but real Walnut's `::`-suffix support is universal (`Prover.parseSetup`'s
/// `Logging.configureForCommand` runs for every command, not just `eval`/`def`), so a
/// throwaway `Logging::new()` here used to silently discard `rightquo x A B;::`'s detail
/// text.
pub fn right_quotient_command(
    session: &Session,
    logging: &mut Logging,
    s: &str,
    old_name1: &str,
    old_name2: &str,
    new_name: &str,
) -> Result<TestCase, QuotientError> {
    let (m1, m2) = read_pair(session, old_name1, old_name2)?;
    let mut c =
        crate::walnut_exception::catch_walnut_panic(|| right_quotient(&m1, &m2, false, logging))
            .map_err(QuotientError::from_panic)?;
    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(c))
}

/// `Quotient.leftQuotient(String s, String oldName1, String oldName2, String newName)`
/// (`Quotient.java:17-23`). `logging` — see [`right_quotient_command`]'s matching note.
pub fn left_quotient_command(
    session: &Session,
    logging: &mut Logging,
    s: &str,
    old_name1: &str,
    old_name2: &str,
    new_name: &str,
) -> Result<TestCase, QuotientError> {
    let (m1, m2) = read_pair(session, old_name1, old_name2)?;
    let mut c = crate::walnut_exception::catch_walnut_panic(|| left_quotient(&m1, &m2, logging))
        .map_err(QuotientError::from_panic)?;
    write_automata(
        session,
        &mut c,
        s,
        &session.paths().write_address_for_automata_library(),
        new_name,
        false,
    )?;
    Ok(TestCase::from_automaton(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use wr_core::fa::Fa;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-quotient-{tag}-{}-{}",
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

    /// A single-track `msd_2` automaton whose language is exactly `{word}` where `word`
    /// is the given single symbol.
    fn single_symbol_automaton(symbol: i32) -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(symbol, vec![1]);
        Automaton::new(
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
        )
    }

    /// A two-symbol-word automaton accepting exactly `"01"`.
    fn accepts_zero_one() -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![1]);
        let mut d1 = BTreeMap::new();
        d1.insert(1, vec![2]);
        Automaton::new(
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
        )
    }

    fn write_library_automaton(dir: &std::path::Path, name: &str, mut a: Automaton) {
        let path = dir.join("Automata Library").join(format!("{name}.txt"));
        wr_io::writer::write_automaton_txt(&mut a, &path).unwrap();
    }

    #[test]
    fn right_quotient_of_zero_one_by_one_gives_zero() {
        let (session, dir) = temp_session("right");
        write_library_automaton(&dir, "A", accepts_zero_one());
        write_library_automaton(&dir, "B", single_symbol_automaton(1));

        let tc = right_quotient_command(
            &session,
            &mut Logging::new(),
            "rightquo c A B;",
            "A",
            "B",
            "c",
        )
        .unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(
            c.fa.accepts_word(&[0]),
            "\"01\" with \"1\" quotiented off the right is \"0\""
        );
        assert!(!c.fa.accepts_word(&[0, 1]));
        assert!(dir.join("Automata Library").join("c.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn left_quotient_of_zero_one_by_zero_gives_one() {
        // `left_quotient(a, b) = { z : ∃w ∈ L(b), wz ∈ L(a) }` (`wr_core::logicalops
        // ::left_quotient`'s own doc comment) -- so `a` must be the LARGER set ("01")
        // being stripped, `b` the prefix language ("0") stripped off it.
        let (session, dir) = temp_session("left");
        write_library_automaton(&dir, "A", accepts_zero_one());
        write_library_automaton(&dir, "B", single_symbol_automaton(0));

        let tc = left_quotient_command(
            &session,
            &mut Logging::new(),
            "leftquo c A B;",
            "A",
            "B",
            "c",
        )
        .unwrap();
        let c = tc.automaton_pairs()[0].automaton().unwrap();
        assert!(
            c.fa.accepts_word(&[1]),
            "\"01\" with \"0\" quotiented off the left is \"1\""
        );
        assert!(!c.fa.accepts_word(&[0, 1]));
        fs::remove_dir_all(&dir).ok();
    }

    /// A single-track automaton over a 3-symbol alphabet, so its alphabet is a strict
    /// SUPERSET of [`single_symbol_automaton`]'s `{0, 1}` — the shape both quotient
    /// subset guards reject (in opposite directions).
    fn wider_alphabet_automaton() -> Automaton {
        let mut d0 = BTreeMap::new();
        d0.insert(2, vec![1]);
        Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 3,
                o: vec![0, 1],
                d: vec![d0, BTreeMap::new()],
            },
            vec![vec![0, 1, 2]],
            vec!["x".to_string()],
            vec![Some(true)],
        )
    }

    /// U23 review fix, finding #2. `wr_core::logicalops::right_quotient`'s subset guard is
    /// an `assert!` replicating Java's `WalnutException`; Java's REPL catches it and keeps
    /// going, while an unwrapped Rust panic here kills the whole process (there is no
    /// `catch_unwind` boundary in this workspace), taking a `load`ed batch session with
    /// it.
    #[test]
    fn right_quotient_reports_a_mismatched_alphabet_as_an_error_not_a_panic() {
        let (session, dir) = temp_session("right-mismatch");
        write_library_automaton(&dir, "A", single_symbol_automaton(1));
        write_library_automaton(&dir, "B", wider_alphabet_automaton());

        let err = right_quotient_command(
            &session,
            &mut Logging::new(),
            "rightquo c A B;",
            "A",
            "B",
            "c",
        )
        .unwrap_err();
        assert!(matches!(err, QuotientError::Walnut(_)));
        assert_eq!(err.to_string(), RIGHT_QUOTIENT_SUBSET_MESSAGE);
        assert!(
            err.is_walnut_exception(),
            "Java throws a WalnutException here, so it renders message-only"
        );
        assert!(!dir.join("Automata Library").join("c.txt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    /// The `leftquo` half. `A` over `{0,1}`, `B` over `{0,1,2}` is exactly WB-010's own
    /// trigger shape (`docs/WALNUT-BUGS.md`): before the fix (`walnut-java` commit
    /// `c5ff914` on `bugfix/wb-010`), `left_quotient`'s backwards guard let this pair
    /// through and it died deeper, inside `Automaton::encode` (Java's equivalent: an
    /// uncaught `ArrayIndexOutOfBoundsException`) — reported as [`QuotientError::Runtime`].
    /// After the fix, the guard itself (now checking the correct direction) rejects this
    /// shape cleanly, reported as [`QuotientError::Walnut`] instead. Either way the
    /// command must fail and the session must survive.
    #[test]
    fn left_quotient_reports_a_mismatched_alphabet_as_an_error_not_a_panic() {
        let (session, dir) = temp_session("left-mismatch");
        write_library_automaton(&dir, "A", single_symbol_automaton(1));
        write_library_automaton(&dir, "B", wider_alphabet_automaton());

        let err = left_quotient_command(
            &session,
            &mut Logging::new(),
            "leftquo c A B;",
            "A",
            "B",
            "c",
        )
        .unwrap_err();
        assert!(
            matches!(err, QuotientError::Walnut(_)),
            "the fixed guard rejects this shape cleanly (WB-010); got {err:?}"
        );
        assert_eq!(err.to_string(), LEFT_QUOTIENT_SUBSET_MESSAGE);
        assert!(
            err.is_walnut_exception(),
            "Java (post-fix) throws a WalnutException here, so it renders message-only"
        );
        assert!(!dir.join("Automata Library").join("c.txt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    /// The trigger [`QuotientError::Runtime`]'s doc comment used to name, **flipped to
    /// WB-038's fixed behavior** (`walnut-java` commit `601a9d2`, ported in
    /// `wr_io::reader`). `pb` declares `msd_2` (alphabet `{0,1}`) but has a transition on
    /// digit `2`. Pre-fix, `AutomatonReader` encoded that digit to the bogus key `-1`,
    /// the subset guard passed (both operands declare the same `{0,1}` alphabet), and
    /// `rightquo` — called directly, not through `leftQuotient`'s determinizing
    /// delegation — carried the key into `right_quotient`'s own re-encode, where it blew
    /// up as an unclassified `RuntimeException`.
    ///
    /// The reader now refuses `pb.txt` before any of that, so this command fails one
    /// layer earlier and for a much better reason. The test is kept (rather than deleted
    /// with the trigger) because it is the only coverage that this specific operand
    /// shape — a digit that is out of alphabet but whose destination state IS declared,
    /// i.e. the sub-case nothing downstream would have complained about — is refused at
    /// all, on the one command that used to get furthest with it.
    #[test]
    fn right_quotient_rejects_wb_038s_file_before_it_can_reach_the_re_encode() {
        let (session, dir) = temp_session("right-wb038");
        fs::write(
            dir.join("Automata Library/pa.txt"),
            "msd_2\n\n0 0\n0 -> 1\n\n1 1\n",
        )
        .unwrap();
        fs::write(
            dir.join("Automata Library/pb.txt"),
            "msd_2\n\n0 0\n0 -> 1\n2 -> 1\n\n1 1\n",
        )
        .unwrap();

        let err = right_quotient_command(
            &session,
            &mut Logging::new(),
            "rightquo pc pa pb;",
            "pa",
            "pb",
            "pc",
        )
        .unwrap_err();
        assert!(
            matches!(err, QuotientError::Read(_)),
            "the operand must be refused at READ time, not carried into the re-encode; \
             got {err:?}"
        );
        assert!(
            err.to_string().contains(
                "digit 2 in position 1 is not in the alphabet [0, 1] of that input: line 5"
            ),
            "and for WB-038's reason, verbatim from the fixed jar; got {err}"
        );
        assert!(!dir.join("Automata Library").join("pc.txt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    /// [`QuotientError::from_panic`]'s classification, direct — it lost its only coverage
    /// when the test above was flipped, since WB-038's fix removed the one live path that
    /// produced a [`QuotientError::Runtime`]. The distinction is not cosmetic: Java
    /// renders a `WalnutException` message-only and anything else with a stack-trace
    /// header, and `crate::prover` triages on exactly this.
    #[test]
    fn from_panic_classifies_only_the_two_walnut_messages_as_walnut() {
        for message in [RIGHT_QUOTIENT_SUBSET_MESSAGE, LEFT_QUOTIENT_SUBSET_MESSAGE] {
            let e = QuotientError::from_panic(message.to_string());
            assert!(
                matches!(e, QuotientError::Walnut(ref m) if m == message),
                "{e:?}"
            );
            assert!(e.is_walnut_exception());
        }
        // Anything else -- e.g. the JDK text `Automaton::decode`'s guard raises -- is an
        // unclassified RuntimeException, rendered with a stack-trace header.
        let e = QuotientError::from_panic("Index -1 out of bounds for length 2".to_string());
        assert!(matches!(e, QuotientError::Runtime(_)), "{e:?}");
        assert!(!e.is_walnut_exception());
        // A near-miss on the Walnut text must NOT be classified as one.
        let almost = format!("{RIGHT_QUOTIENT_SUBSET_MESSAGE} ");
        assert!(!QuotientError::from_panic(almost).is_walnut_exception());
    }

    /// Both quotients are ASYMMETRIC in their two automaton arguments, so a swapped-operand
    /// port bug would still write a file and still produce a valid automaton — only a
    /// language assertion catches it. (`right_quotient(a, b) = { z : ∃w ∈ L(b), zw ∈ L(a) }`.)
    #[test]
    fn quotient_operand_order_is_not_interchangeable() {
        let (session, dir) = temp_session("quo-order");
        write_library_automaton(&dir, "A", accepts_zero_one());
        write_library_automaton(&dir, "B", single_symbol_automaton(1));

        let right = right_quotient_command(
            &session,
            &mut Logging::new(),
            "rightquo c A B;",
            "A",
            "B",
            "c",
        )
        .unwrap();
        let c = right.automaton_pairs()[0].automaton().unwrap();
        assert!(
            c.fa.accepts_word(&[0]),
            "\"01\" / \"1\" on the right is \"0\""
        );

        // Swapping the operands is a genuinely different question ("strip '01' off the
        // right of '1'"), whose answer is the empty language -- so it does NOT accept "0".
        let swapped = right_quotient_command(
            &session,
            &mut Logging::new(),
            "rightquo d B A;",
            "B",
            "A",
            "d",
        )
        .unwrap();
        let d = swapped.automaton_pairs()[0].automaton().unwrap();
        assert!(
            !d.fa.accepts_word(&[0]),
            "operand order must matter: the swap has a different language"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn right_quotient_propagates_a_missing_file_read_error() {
        let (session, dir) = temp_session("missing");
        write_library_automaton(&dir, "A", accepts_zero_one());
        let err = right_quotient_command(
            &session,
            &mut Logging::new(),
            "rightquo c A B;",
            "A",
            "B",
            "c",
        )
        .unwrap_err();
        assert!(matches!(err, QuotientError::Read(_)));
        fs::remove_dir_all(&dir).ok();
    }
}
