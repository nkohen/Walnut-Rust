// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/TestCase.java` (90 LOC) — a result/test-case value object, ported as Phase 3a's
//! U11b. Pulled forward ahead of the rest of `wr-cli` because both `EvalDef` (U15) and
//! `Reg` (U16) return it.
//!
//! # Scope note: `AutomatonMatrixWriter.EMPTY_MATRIX_TEST_CASES`
//!
//! Java's file imports `Automata.Writer.AutomatonMatrixWriter.EMPTY_MATRIX_TEST_CASES`
//! (`TestCase.java:27`), a constant defined as `List.of("", "", "", "")`
//! (`AutomatonMatrixWriter.java:21`). **Updated (2026-08-19, `docs/CAS-EXPORT-DISPATCH.md`):
//! `AutomatonMatrixWriter` — the CAS (Maple/MATLAB/Mathematica/Sage) incidence-matrix
//! exporter — is no longer DROP scope; it is ported at [`wr_io::matrix_writer`], reached
//! from `wr-cli`'s real `eval`/`def` dispatch (`eval_def.rs`) whenever a non-empty
//! free-variable list is supplied on a non-trivial result.** [`EMPTY_MATRIX_TEST_CASES`]
//! below remains a real, still-needed local constant, not a placeholder for a dropped
//! writer: it is the bare four-empty-string literal `TestCase.java` itself falls back to
//! whenever NO free-variable list was supplied (the ordinary case for a plain `eval`,
//! e.g. [`TestCase::from_automaton`] below). This module still has no *unconditional*
//! dependency on the writer — it is `wr-cli`'s `eval_def.rs` that calls into
//! `wr_io::matrix_writer` directly and passes the real, non-empty matrix addresses to
//! [`TestCase::new`] when a free-variable list was supplied; this file never calls the
//! writer itself.
//!
//! # `UtilityMethods.readFromFile`
//!
//! `getMatrixOutput`/`getGraphViz` (`TestCase.java:62-78`) both call
//! `UtilityMethods.readFromFile` (`UtilityMethods.java:161-176`), already ported (U0b) as
//! [`wr_core::util::read_from_file`] — this module calls that directly rather than
//! reimplementing it, to avoid two independent copies of the same Java method silently
//! drifting apart.

use std::io;

use wr_core::automaton::Automaton;
use wr_core::util::read_from_file;

// ---------------------------------------------------------------------------
// Constants (`TestCase.java:36-39`)
// ---------------------------------------------------------------------------

/// `TestCase.DEFAULT_TESTFILE` (`:36`).
pub const DEFAULT_TESTFILE: &str = "automaton";

/// `TestCase.OST_REPR_TESTFILE` (`:37`): `DEFAULT_TESTFILE + "_repr"`. Java computes
/// this from [`DEFAULT_TESTFILE`] at class-load time via string concatenation; Rust
/// `const` evaluation can't easily express "concatenate this other `const &str`" without
/// extra machinery, so the derived value is spelled out literally here instead — kept in
/// sync with [`DEFAULT_TESTFILE`] by the test below rather than by construction.
pub const OST_REPR_TESTFILE: &str = "automaton_repr";

/// `TestCase.ERROR_FILE` (`:38`).
pub const ERROR_FILE: &str = "error";

/// `TestCase.DETAILS_FILE` (`:39`).
pub const DETAILS_FILE: &str = "details";

/// Local stand-in for `AutomatonMatrixWriter.EMPTY_MATRIX_TEST_CASES` — see this
/// module's scope note above for why this is not a port of `AutomatonMatrixWriter`
/// itself.
const EMPTY_MATRIX_TEST_CASES: [&str; 4] = ["", "", "", ""];

fn empty_matrix_test_cases() -> Vec<String> {
    EMPTY_MATRIX_TEST_CASES
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// TestCase
// ---------------------------------------------------------------------------

/// `Main/TestCase.java` (`:29-90`) — an immutable result/test-case value object: the
/// return type of `eval`/`def`/`reg` and friends, and the shape the integration-test
/// harness compares "expected" against "actual" with.
///
/// All fields are private with getters, mirroring Java's private-final-fields-plus-
/// getters shape (there are no setters in Java either — every field is set once, at
/// construction).
#[derive(Debug, Clone)]
pub struct TestCase {
    error: String,
    details: String,
    matrix_addresses: Vec<String>,
    gv_address: String,
    automaton_pairs: Vec<AutomatonFilenamePair>,
}

impl TestCase {
    /// The full constructor (`TestCase.java:41-49`).
    ///
    /// **Parameter order matches Java's constructor signature** — `error,
    /// matrix_addresses, gv_address, details, automaton_pairs` — which is deliberately
    /// NOT the same order as the struct's own field declarations above (`error, details,
    /// matrix_addresses, gv_address, automaton_pairs`). Java's constructor parameter
    /// list and its class's field declaration list simply disagree on ordering; this is
    /// preserved verbatim as a faithful mechanical port rather than quietly
    /// "corrected" to look more consistent — a caller porting a Java call site can copy
    /// the argument order directly.
    pub fn new(
        error: impl Into<String>,
        matrix_addresses: Vec<String>,
        gv_address: impl Into<String>,
        details: impl Into<String>,
        automaton_pairs: Vec<AutomatonFilenamePair>,
    ) -> Self {
        TestCase {
            error: error.into(),
            details: details.into(),
            matrix_addresses,
            gv_address: gv_address.into(),
            automaton_pairs,
        }
    }

    /// `TestCase(Automaton result)` (`:50-53`) — wraps a single result automaton under
    /// [`DEFAULT_TESTFILE`], with no error/matrix/graphviz/details payload. This is the
    /// constructor most of `Prover.java`'s command handlers and `Reg.regCommand` use
    /// (`return new TestCase(M);` / `return new TestCase(R);`).
    pub fn from_automaton(result: Automaton) -> Self {
        TestCase::new(
            "",
            empty_matrix_test_cases(),
            "",
            "",
            vec![AutomatonFilenamePair::new(result, DEFAULT_TESTFILE)],
        )
    }

    /// `TestCase(List<AutomatonFilenamePair> automatonPairs)` (`:54-56`).
    pub fn from_automaton_pairs(automaton_pairs: Vec<AutomatonFilenamePair>) -> Self {
        TestCase::new("", empty_matrix_test_cases(), "", "", automaton_pairs)
    }

    /// `TestCase.getAutomatonPairs()` (`:58-60`).
    pub fn automaton_pairs(&self) -> &[AutomatonFilenamePair] {
        &self.automaton_pairs
    }

    /// `TestCase.getMatrixOutput()` (`:62-71`).
    ///
    /// If `matrix_addresses` is empty, returns [`EMPTY_MATRIX_TEST_CASES`] directly
    /// (Java also checks for `null`; there is no `null` `Vec` in Rust, so `is_empty()`
    /// alone is the whole check). Otherwise reads each address as a file and collects
    /// the contents in order.
    ///
    /// **A quirk worth knowing, not a bug:** [`Self::from_automaton`]/
    /// [`Self::from_automaton_pairs`] set `matrix_addresses` to *four empty-string*
    /// addresses (via [`EMPTY_MATRIX_TEST_CASES`]) — not an empty `Vec`. So calling this
    /// method on a `TestCase` built by either of those constructors does NOT take the
    /// `is_empty()` short-circuit; instead it calls [`read_from_file`] on `""` four
    /// times. `""` is never an existing regular file, so each call harmlessly returns
    /// `""` — reproducing [`EMPTY_MATRIX_TEST_CASES`]'s own value by a roundabout path.
    /// Same observable result either way, confirmed by
    /// [`tests::from_automaton_matrix_output_matches_empty_short_circuit`] below.
    pub fn matrix_output(&self) -> io::Result<Vec<String>> {
        if self.matrix_addresses.is_empty() {
            return Ok(empty_matrix_test_cases());
        }
        self.matrix_addresses
            .iter()
            .map(|address| read_from_file(address))
            .collect()
    }

    /// `TestCase.getGraphViz()` (`:73-78`).
    pub fn graph_viz(&self) -> io::Result<String> {
        if self.gv_address.is_empty() {
            return Ok(String::new());
        }
        read_from_file(&self.gv_address)
    }

    /// `TestCase.getDetails()` (`:80-82`).
    pub fn details(&self) -> &str {
        &self.details
    }

    /// `TestCase.getError()` (`:84-86`).
    pub fn error(&self) -> &str {
        &self.error
    }
}

// ---------------------------------------------------------------------------
// AutomatonFilenamePair
// ---------------------------------------------------------------------------

/// `TestCase.AutomatonFilenamePair` (`:88`), a Java `record`. The Rust translation is a
/// plain struct with the same two accessors a record auto-generates — `automaton()`/
/// `filename()`, no `get` prefix — rather than a tuple struct, which would lose the
/// field names at call sites.
///
/// `automaton` is `Option<Automaton>`, not a bare [`Automaton`]: Java's record component
/// is a plain (nullable) reference type with no non-null annotation, and
/// `IntegrationTest.java` (the harness this value object exists to serve) genuinely
/// constructs one with a `null` automaton (`new TestCase.AutomatonFilenamePair(null,
/// TestCase.DEFAULT_TESTFILE)`, `IntegrationTest.java:1058`) to represent "expected: an
/// error, not an automaton" — and then compares against it with an explicit
/// `expectedA == null` branch. [`Self::new`] accepts `impl Into<Option<Automaton>>` so
/// call sites passing a real automaton don't need to wrap it in `Some(..)` (the
/// blanket `impl<T> From<T> for Option<T>` in `std` makes a bare `Automaton` argument
/// convert automatically), while a caller that needs the null case can pass `None`.
#[derive(Debug, Clone)]
pub struct AutomatonFilenamePair {
    automaton: Option<Automaton>,
    filename: String,
}

impl AutomatonFilenamePair {
    pub fn new(automaton: impl Into<Option<Automaton>>, filename: impl Into<String>) -> Self {
        AutomatonFilenamePair {
            automaton: automaton.into(),
            filename: filename.into(),
        }
    }

    /// `AutomatonFilenamePair.automaton()` — the record's auto-generated accessor.
    pub fn automaton(&self) -> Option<&Automaton> {
        self.automaton.as_ref()
    }

    /// `AutomatonFilenamePair.filename()` — the record's auto-generated accessor.
    pub fn filename(&self) -> &str {
        &self.filename
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn stub_automaton() -> Automaton {
        Automaton::true_false(true)
    }

    // -- constants ------------------------------------------------------------

    #[test]
    fn ost_repr_testfile_stays_derived_from_default_testfile() {
        // Java computes this as `DEFAULT_TESTFILE + "_repr"`; pin the relationship so a
        // future edit to one constant can't silently desync from the other.
        assert_eq!(OST_REPR_TESTFILE, format!("{DEFAULT_TESTFILE}_repr"));
    }

    // -- constructors -----------------------------------------------------------

    #[test]
    fn full_constructor_populates_every_field() {
        let pair = AutomatonFilenamePair::new(stub_automaton(), "foo");
        let tc = TestCase::new(
            "some error",
            vec!["addr1".to_string(), "addr2".to_string()],
            "result.gv",
            "some details",
            vec![pair],
        );
        assert_eq!(tc.error(), "some error");
        assert_eq!(tc.details(), "some details");
        assert_eq!(tc.automaton_pairs().len(), 1);
        assert_eq!(tc.automaton_pairs()[0].filename(), "foo");
        assert!(tc.automaton_pairs()[0].automaton().is_some());
    }

    #[test]
    fn from_automaton_wraps_under_default_testfile_with_empty_payload() {
        let tc = TestCase::from_automaton(stub_automaton());
        assert_eq!(tc.error(), "");
        assert_eq!(tc.details(), "");
        assert_eq!(tc.automaton_pairs().len(), 1);
        assert_eq!(tc.automaton_pairs()[0].filename(), DEFAULT_TESTFILE);
        assert!(tc.graph_viz().unwrap().is_empty());
    }

    #[test]
    fn from_automaton_pairs_forwards_the_list_unchanged() {
        let pairs = vec![
            AutomatonFilenamePair::new(stub_automaton(), "a"),
            AutomatonFilenamePair::new(stub_automaton(), "b"),
        ];
        let tc = TestCase::from_automaton_pairs(pairs);
        assert_eq!(tc.automaton_pairs().len(), 2);
        assert_eq!(tc.automaton_pairs()[0].filename(), "a");
        assert_eq!(tc.automaton_pairs()[1].filename(), "b");
    }

    // -- AutomatonFilenamePair: the real null-automaton case --------------------

    #[test]
    fn automaton_filename_pair_accepts_a_bare_automaton_via_into() {
        let pair = AutomatonFilenamePair::new(stub_automaton(), "x");
        assert!(pair.automaton().is_some());
    }

    /// Mirrors `IntegrationTest.java:1057-1058`'s `new TestCase.AutomatonFilenamePair(null,
    /// TestCase.DEFAULT_TESTFILE)`, used there to represent "expected: an error, no
    /// automaton".
    #[test]
    fn automaton_filename_pair_accepts_none_for_the_null_automaton_case() {
        let pair = AutomatonFilenamePair::new(None, DEFAULT_TESTFILE);
        assert!(pair.automaton().is_none());
        assert_eq!(pair.filename(), DEFAULT_TESTFILE);
    }

    // -- matrix_output / graph_viz ------------------------------------------------

    #[test]
    fn matrix_output_on_empty_addresses_returns_empty_matrix_test_cases() {
        let tc = TestCase::new("", vec![], "", "", vec![]);
        assert_eq!(
            tc.matrix_output().unwrap(),
            vec![
                "".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string()
            ]
        );
    }

    /// The quirk documented on [`TestCase::matrix_output`]: the convenience
    /// constructors set `matrix_addresses` to four empty-string ADDRESSES (not an empty
    /// list), so they take the file-reading path rather than the `is_empty()`
    /// short-circuit -- but land on the exact same value either way, since `""` is
    /// never an existing file.
    #[test]
    fn from_automaton_matrix_output_matches_empty_short_circuit() {
        let via_convenience_ctor = TestCase::from_automaton(stub_automaton())
            .matrix_output()
            .unwrap();
        let via_explicit_empty = TestCase::new("", vec![], "", "", vec![])
            .matrix_output()
            .unwrap();
        assert_eq!(via_convenience_ctor, via_explicit_empty);
        assert_eq!(via_convenience_ctor, vec!["", "", "", ""]);
    }

    #[test]
    fn matrix_output_reads_real_files_in_order() {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-test-case-matrix-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("m1.txt");
        let f2 = dir.join("m2.txt");
        fs::write(&f1, "row one\nrow two").unwrap();
        fs::write(&f2, "second file").unwrap();

        let tc = TestCase::new(
            "",
            vec![
                f1.to_str().unwrap().to_string(),
                f2.to_str().unwrap().to_string(),
            ],
            "",
            "",
            vec![],
        );
        assert_eq!(
            tc.matrix_output().unwrap(),
            vec!["row one\nrow two".to_string(), "second file".to_string()]
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn graph_viz_empty_address_returns_empty_string() {
        let tc = TestCase::new("", vec![], "", "", vec![]);
        assert_eq!(tc.graph_viz().unwrap(), "");
    }

    #[test]
    fn graph_viz_reads_a_real_file() {
        let path = std::env::temp_dir().join(format!(
            "wr-cli-test-case-gv-{}-{}.gv",
            std::process::id(),
            line!()
        ));
        fs::write(&path, "digraph { a -> b; }").unwrap();

        let tc = TestCase::new("", vec![], path.to_str().unwrap(), "", vec![]);
        assert_eq!(tc.graph_viz().unwrap(), "digraph { a -> b; }");

        fs::remove_file(&path).ok();
    }

    // -- read_from_file directly --------------------------------------------------

    #[test]
    fn read_from_file_on_a_missing_path_returns_empty_ok_not_an_error() {
        assert_eq!(
            read_from_file("/definitely/does/not/exist/anywhere.txt").unwrap(),
            ""
        );
    }

    #[test]
    fn read_from_file_joins_lines_with_newline_and_drops_trailing_newline() {
        let path = std::env::temp_dir().join(format!(
            "wr-cli-test-case-readfromfile-{}-{}.txt",
            std::process::id(),
            line!()
        ));
        fs::write(&path, "line1\nline2\nline3\n").unwrap();
        assert_eq!(
            read_from_file(path.to_str().unwrap()).unwrap(),
            "line1\nline2\nline3"
        );
        fs::remove_file(&path).ok();
    }
}
