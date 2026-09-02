// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-032** (`docs/WALNUT-BUGS.md`), the `bugfix/wb-032`
//! follow-up to `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s downstream port workflow. Checked
//! against real `walnut-java` output **captured against the FIXED branch** `bugfix/wb-032`
//! (commit `18b7c4b`, stacked on `bugfix/wb-024-025`), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's this project's `../CAPTURE.md`
//! discipline for these follow-up units.
//!
//! # `convertNS`'s exponent was a truncated floating-point log ratio
//!
//! `AutomatonLogicalOps.convertNS` computed the exponent `j` in `base == root^j` as
//! `(int) (Math.log(base) / Math.log(root))` — a double quotient of two logarithms that is
//! not exact, and whose truncating `(int)` cast rounded a large fraction of results DOWN by
//! a whole unit (the smallest case: `Math.log(1000) / Math.log(10) ==
//! 2.9999999999999996`, truncating to `2` instead of `3`). Two symptoms, one per call site,
//! both covered below through the real `convert` CLI command (not just the `convert_ns`
//! primitive directly — `crates/wr-core/src/logicalops.rs`'s own unit tests and
//! `tests/differential/tests/convert_ns.rs`'s branch-coverage suite already cover the
//! primitive; this file is the end-to-end command-level pin, matching
//! `java_bugfix_wb010.rs`'s established practice):
//!
//! - **the `k -> k^j` regrouping direction** (`toBase != commonRoot`,
//!   `convertMsdBaseToExponent`): used to silently write a well-formed but WRONG
//!   `msd_100`-shaped automaton when asked for `msd_1000` — no error, no warning.
//! - **the `k^i -> k` ungrouping direction** (`fromBase != commonRoot`,
//!   `convertLsdBaseToRoot`): used to crash with a spurious `Base mismatch: expected 100,
//!   found 1000` from that helper's own consistency guard, on a call that should succeed.
//!
//! ## Capture recipe (reproducible)
//!
//! Built in an isolated worktree, per this project's standing shared-checkout-safety rule
//! (`~/dev/walnut-java` may have other agents committing to it concurrently) — and note
//! `bugfix/wb-032` was ALREADY checked out at the main `~/dev/walnut-java` working tree
//! when this was captured, so the worktree below is added by commit hash (detached), not by
//! branch name, to avoid git's "branch already checked out" refusal:
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb032 18b7c4b
//! cd /tmp/walnut-java-wb032
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! # -- Case 1: the regrouping direction (msd_10 -> msd_1000, was silently msd_100) --
//! cp <repo>/tests/differential/fixtures/convert_ns/base10.txt "Automata Library/wb032base10.txt"
//! cat > "Command Files/wb032_capture.txt" <<'EOF'
//! convert $wb032b10msd1000 msd_1000 $wb032base10;
//! EOF
//! java -jar target/Walnut-all.jar wb032_capture.txt < /dev/null
//! # -> Session/<timestamp>/Automata Library/wb032b10msd1000.txt, header "msd_1000" (was
//! #    "msd_100" pre-fix). Copied over tests/differential/fixtures/convert_ns/b10msd1000.txt
//! #    as a straight overwrite -- that fixture's own recapture, per convert_ns.rs's module
//! #    docs -- and reused from there by this file's Case 1 test, rather than duplicated
//! #    (it is ~3,000 lines, a 1000-symbol alphabet).
//!
//! # -- Case 2: the ungrouping direction (msd_1000 -> msd_10, used to crash) --
//! cat > "Automata Library/wb032eps1000.txt" <<'EOF'
//! msd_1000
//!
//! 0 1
//! EOF
//! cat > "Command Files/wb032_capture2.txt" <<'EOF'
//! convert $wb032eps1000to10 msd_10 $wb032eps1000;
//! EOF
//! java -jar target/Walnut-all.jar wb032_capture2.txt < /dev/null
//! # -> Session/<timestamp>/Automata Library/wb032eps1000to10.txt (below, inlined -- only
//! #    ~20 lines).
//!
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb032 --force
//! ```
//!
//! `java`/`mvnw` above actually ran under a JDK 17+ toolchain
//! (`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1`, selected explicitly
//! since the shell's default `java` resolves to a JDK 11 too old for this project's class
//! file version) — noted here since it's a prerequisite this recipe silently assumes
//! otherwise, per `java_bugfix_wb010.rs`'s own note.
//!
//! ## Case 1 output (captured 2026-08-20, `18b7c4b`)
//!
//! `Automata Library/wb032b10msd1000.txt` (`x < 15` over `msd_10`, converted to `msd_1000`):
//! header `msd_1000`, alphabet `0..1000`, 3 states — byte-identical to (and now the source
//! of) `tests/differential/fixtures/convert_ns/b10msd1000.txt`. Both engines succeed with no
//! error, unlike the pre-fix `msd_100` header.
//!
//! ## Case 2 output (captured 2026-08-20, `18b7c4b`)
//!
//! ```text
//! msd_10
//!
//! 0 1
//! 0 -> 1
//! 1 -> 1
//! 2 -> 1
//! 3 -> 1
//! 4 -> 1
//! 5 -> 1
//! 6 -> 1
//! 7 -> 1
//! 8 -> 1
//! 9 -> 1
//!
//! 1 0
//! 0 -> 1
//! 1 -> 1
//! 2 -> 1
//! 3 -> 1
//! 4 -> 1
//! 5 -> 1
//! 6 -> 1
//! 7 -> 1
//! 8 -> 1
//! 9 -> 1
//! ```
//!
//! Both engines succeed with no `Base mismatch` error, unlike the pre-fix crash. State `0`
//! (accepting, `q0`) rejects every digit into non-accepting sink state `1`, so the
//! converted language is exactly `{ε}` — the same language the msd_1000 source (a single
//! accepting state with no transitions, totalized before conversion) started with: a
//! single, INCOMPLETE msd_1000 digit-group can never correspond to the empty msd_10 word,
//! so every digit is correctly rejected.
//!
//! The command files and hand-authored automaton files were deleted from the isolated
//! worktree afterward, matching every recipe above.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `java_bugfix_wb010.rs`'s own `Capture`.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

const WB032EPS1000: &str = "msd_1000\n\n0 1\n";

/// Real `walnut-java` fixed output for Case 2 (`convert $wb032eps1000to10 msd_10
/// $wb032eps1000;`), captured against `bugfix/wb-032` (commit `18b7c4b`) per this file's
/// own module docs.
const WB032EPS1000TO10_CAPTURED: &str = "msd_10\n\n\
     0 1\n0 -> 1\n1 -> 1\n2 -> 1\n3 -> 1\n4 -> 1\n5 -> 1\n6 -> 1\n7 -> 1\n8 -> 1\n9 -> 1\n\n\
     1 0\n0 -> 1\n1 -> 1\n2 -> 1\n3 -> 1\n4 -> 1\n5 -> 1\n6 -> 1\n7 -> 1\n8 -> 1\n9 -> 1\n";

/// `tests/differential/fixtures/convert_ns/<name>` — reused directly rather than
/// duplicated, per this file's own module docs (Case 1's expected automaton is ~3,000
/// lines, a 1000-symbol alphabet).
fn convert_ns_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/convert_ns")
        .join(name)
}

/// A process-scoped Walnut home tree plus a `Prover` over it — same shape as
/// `java_bugfix_wb010.rs`'s own `prover` helper.
fn prover(tag: &str) -> (Prover, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-javabugfix-{tag}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    for sub in [
        "Result",
        "Automata Library",
        "Word Automata Library",
        "Custom Bases",
        "Macro Library",
        "Morphism Library",
        "Command Files",
        "Transducer Library",
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    let console = Capture::default();
    let logging = Logging::with_writers(Box::new(console.clone()), Box::new(Capture::default()));
    (
        Prover::with_output(session, logging, Box::new(console)),
        dir,
    )
}

/// WB-032 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `18b7c4b`
/// (branch `bugfix/wb-032`): the `k -> k^j` regrouping direction. `x < 15` over `msd_10`,
/// converted to `msd_1000` — the smallest trigger for the old float-log exponent bug
/// (`Math.log(1000) / Math.log(10)` used to truncate to `2`, not `3`), which used to
/// silently produce a well-formed `msd_100`-shaped automaton instead, with no error.
///
/// Built entirely through the real `def`/`convert` CLI commands (no hand-authored fixture
/// needed for the SOURCE automaton — `def base10 "?msd_10 x < 15";` is the exact formula
/// `tests/differential/fixtures/convert_ns/base10.txt` was itself captured from, per
/// `convert_ns.rs`'s own module docs), so this is a genuine command-level integration
/// check, not just a call to `convert_ns` directly.
#[test]
fn wb032_convert_regroups_msd10_to_msd1000_not_msd100() {
    let (mut p, dir) = prover("wb032-case1");
    p.dispatch("def base10 \"?msd_10 x < 15\";")
        .expect("def must succeed");
    p.dispatch("convert $b10msd1000 msd_1000 $base10;").expect(
        "convert must succeed now that WB-032 is fixed -- it used to silently \
                 succeed with the WRONG base, which this call alone cannot distinguish; \
                 the header/alphabet checks below are what actually pin the fix",
    );

    let ours_path = dir.join("Automata Library/b10msd1000.txt");
    let ours = wr_io::reader::read_automaton_txt(&ours_path).expect("b10msd1000.txt");

    assert_eq!(
        ours.track_alphabets(),
        vec![(0..1000).collect::<Vec<i32>>()],
        "WB-032: must regroup 3 base-10 digits per base-1000 digit (alphabet 0..1000), \
         not 2 (0..100, the old bug's answer)"
    );
    assert_eq!(ours.track_msds(), vec![Some(true)]);

    let theirs = wr_io::reader::read_automaton_txt(convert_ns_fixture("b10msd1000.txt"))
        .expect("real walnut-java's fixed b10msd1000.txt fixture must parse cleanly");
    assert_eq!(
        theirs.track_alphabets(),
        vec![(0..1000).collect::<Vec<i32>>()],
        "sanity: the captured fixture itself must be base 1000, not the old bug's 100"
    );

    let mut ours_total = ours.clone();
    let mut theirs_total = theirs.clone();
    ours_total.fa.totalize(0);
    theirs_total.fa.totalize(0);
    assert_eq!(
        automaton_language_equivalent(&ours_total, &theirs_total),
        Ok(true),
        "the fixed port's converted language must match real (fixed) walnut-java's"
    );

    fs::remove_dir_all(&dir).ok();
}

/// WB-032's other symptom, also fixed by `18b7c4b`: the `k^i -> k` ungrouping direction.
/// Before the fix, `convertLsdBaseToRoot`'s own base-mismatch guard fired on the truncated
/// exponent (`Base mismatch: expected 100, found 1000`), crashing a call that should
/// succeed. A single-state, no-transition `msd_1000` automaton (language `{ε}` once
/// `convert_ns`'s own totalize guard runs) converted to `msd_10` must now succeed and
/// compute the correct (still `{ε}`-only) result.
#[test]
fn wb032_convert_ungroups_msd1000_to_msd10_without_a_spurious_base_mismatch() {
    let (mut p, dir) = prover("wb032-case2");
    fs::write(dir.join("Automata Library/wb032eps1000.txt"), WB032EPS1000).unwrap();

    p.dispatch("convert $wb032eps1000to10 msd_10 $wb032eps1000;")
        .expect(
            "convert must succeed now that WB-032 is fixed -- it used to crash with a \
             spurious \"Base mismatch: expected 100, found 1000\"",
        );

    let ours_path = dir.join("Automata Library/wb032eps1000to10.txt");
    let ours_text = fs::read_to_string(&ours_path).expect("wb032eps1000to10.txt");
    assert_eq!(
        ours_text, WB032EPS1000TO10_CAPTURED,
        "must be byte-identical to real walnut-java's fixed output (captured against \
         bugfix/wb-032, commit 18b7c4b)"
    );

    // Independent re-derivation, not just "matches the fixture": the source accepts only
    // the empty string (after convert_ns's own totalize, since the single state has no
    // declared transitions at all), so no single base-10 digit -- an INCOMPLETE base-1000
    // digit-group -- can correspond to the empty base-1000 word, and every digit must be
    // rejected.
    let ours = wr_io::reader::read_automaton_txt(&ours_path).expect("must parse cleanly");
    assert_eq!(ours.track_alphabets(), vec![(0..10).collect::<Vec<i32>>()]);
    assert_eq!(ours.track_msds(), vec![Some(true)]);
    assert!(ours.fa.accepts_word(&[]), "empty string must be accepted");
    for d in 0..10 {
        assert!(
            !ours.fa.accepts_word(&[d]),
            "a single base-10 digit ({d}) must be rejected"
        );
        assert!(
            !ours.fa.accepts_word(&[d, d]),
            "two base-10 digits ({d},{d}) must also be rejected"
        );
    }

    fs::remove_dir_all(&dir).ok();
}
