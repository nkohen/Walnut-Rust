// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-024** and **WB-025** (`docs/WALNUT-BUGS.md`). Checked
//! against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-024-025` (commit `59eda64`, stacked on `bugfix/wb-010` (`c5ff914`), stacked
//! on `bugfix/wb-016` (`d6e9799`), stacked on `bugfix/wb-008-009` (`b5d462b`), stacked on
//! `bugfix/wb-002-012-037-044` (`50636f4`/`92776e9`/`aa4a241`/`d757221`)), **not
//! mainline** — see `java_bugfix_wb002.rs`'s module docs for why that's this project's
//! `../CAPTURE.md` discipline for these follow-up units, and the same note about
//! re-pointing the commit reference once the branch merges upstream.
//!
//! # Why this file exists, and why `fixtures/reg/r19.txt`/`r50.txt`/`r58.txt` are gone
//!
//! `tests/reg_brics_regex.rs`'s 60-case corpus originally captured three
//! out-of-alphabet-digit shapes (`r19`: bare `2*` over `{0,1}`; `r50`: `[9,9][0,0]` over
//! `{0,1,2,3} {0,1}`; `r58`: `[10]` over `{0,1}`) against MAINLINE `walnut-java`, back
//! when WB-024's bug made each one silently build a (wrong) automaton instead of
//! erroring. `wr_core::regex::determine_encoded_regex` now ports the real fix — every
//! digit is checked against its track's alphabet before it can ever be encoded, exactly
//! like `Reg.java:63-68` — so none of those three `(alphabets, regex)` pairs builds an
//! automaton at all any more; each one now cleanly errors. There is no automaton left to
//! capture, so the `.txt` fixture files were REMOVED rather than updated in place.
//!
//! This is not a silent shrink of coverage, though: it mirrors the same
//! `bugfix/wb-024-025` commit's own precedent on the walnut-java side, which flipped
//! `Main.IntegrationTest`'s `test149`-`test155` from asserting a stale (buggy)
//! `automaton<i>.txt` fixture to asserting a captured `error<i>.txt` message instead of
//! deleting coverage outright (see that commit's own message, `Main/IntegrationTest.java`
//! near `test149`). `CLAUDE.md`'s "zero tests deleted, ever" rule applies here in spirit:
//! the *coverage* survives, just moved to where THIS harness represents an error
//! outcome — inline captured-message assertions, `tests/reg_brics_regex.rs`'s own
//! pre-existing `reg_parse_errors_match_real_walnut_messages` convention — because this
//! harness's `.txt` fixture format has no room for "the command errors instead of
//! building anything" the way Java's parallel `error<i>.txt`/`automaton<i>.txt` naming
//! convention does.
//!
//! Each of the three retired cases is now pinned, against the real FIXED jar, in exactly
//! one of two places:
//!
//! * `r19` (bare out-of-alphabet digit): [`wb024_r19_bare_digit_now_rejects`] below — no
//!   other test in the repo exercises a bare (unbracketed) out-of-alphabet digit through
//!   the full `reg` pipeline.
//! * `r50` (order-dependence, the bug's headline symptom):
//!   [`wb024_r50_bracketed_vectors_reject_regardless_of_order`] below, ALSO independently
//!   pinned (through `wr_core::regex::determine_encoded_regex` directly, not through
//!   `wr-cli`'s dispatch) by
//!   `wb_024_alphabet_offset_collision_no_longer_depends_on_order` in
//!   `tests/reg_brics_regex.rs`.
//! * `r58` (a bracketed multi-digit integer literal read as a one-element vector):
//!   [`wb024_r58_bracketed_integer_outside_alphabet_now_rejects`] below, ALSO
//!   independently pinned (same "direct `determine_encoded_regex` call, not through
//!   `wr-cli`" distinction) by
//!   `wb_024_a_bracketed_integer_outside_the_alphabet_is_now_rejected` in
//!   `crates/wr-core/src/regex/tests.rs`.
//!
//! The three tests below add something the two unit-level pins above do NOT have: they
//! drive the real `wr_cli::prover::Prover::read_buffer` dispatch path (the same one the
//! `reg` CLI command actually uses) and assert the FULL captured console/stderr text
//! byte-for-byte against the real fixed jar — same discipline as `java_bugfix_wb010.rs`
//! — so a `ProverError::Reg`/`RegError` classification bug (rendering the new message on
//! the wrong channel, or with a stray exception-kind prefix) would be caught here even
//! though the message TEXT is identical either way.
//!
//! # WB-025's boundary is NOT re-verified through this file's own dispatch path
//!
//! WB-025's fix ([`validate_offset_encodable_alphabet_size` in
//! `crates/wr-core/src/regex.rs`](../../../crates/wr-core/src/regex.rs)) tightens the
//! alphabet-size guard `set_from_brics_automaton` enforces to `65408`. Driving a
//! `65409`-track (or `65409`-symbol single-track) alphabet declaration through this
//! dispatch layer's own textual grammar (`Alphabet.determineAlphabetsAndNS`, an
//! `{n1,n2,…}`-style literal set or a `msd_k`/custom-base declaration) is impractically
//! slow and, for the literal-set form, outright impractical to type — see
//! `crates/wr-core/src/regex/tests.rs`'s `wb_025_*` tests' own doc comments for the same
//! conclusion at the `wr_core::regex` layer. So WB-025's boundary is instead verified
//! directly, at the `wr_core::regex` layer (`validate_offset_encodable_alphabet_size`,
//! `convert_encoding_for_brics`, `set_from_brics_automaton`) by
//! `crates/wr-core/src/regex/tests.rs`'s `wb_025_*` tests, cross-checked against the
//! real fixed jar's own committed `Automata/FA/BricsConverterTest.java`
//! (`validateOffsetEncodableAlphabetSize(65408)` does not throw;
//! `validateOffsetEncodableAlphabetSize(65409)` throws with a message containing
//! `"65408"`; `convertEncodingForBrics(65407)` stays at code point `65535` with no
//! wraparound, `convertEncodingForBrics(65408)` wraps to code point `0`) — re-run live
//! (`./mvnw -q -Dtest=BricsConverterTest test`, JDK 17, in an isolated worktree) against
//! the real fixed jar.
//!
//! **`Main/Commands/RegTest.java` does NOT cover WB-025** — an earlier revision of this
//! module doc claimed it contained a `reg`-command-level WB-025 test (a `65420`-track
//! alphabet inside the danger zone); that claim was fabricated, not verified, and is
//! corrected here after adversarial review actually read the file: `RegTest.java`
//! contains exactly four tests, all WB-024 (out-of-alphabet-digit) cases, no WB-025
//! (alphabet-size) case at all. **WB-025's boundary genuinely has zero `reg`-command-level
//! coverage on either engine** — only the direct `BricsConverter`/`validate_offset_
//! encodable_alphabet_size`-layer tests exist, on both sides. Stated honestly rather
//! than papered over: closing this gap (a real `reg` command over a >65408-symbol
//! alphabet, on both engines) is a legitimate follow-up, not part of this unit.
//!
//! # Capture recipe (reproducible), all four WB-024 cases below
//!
//! Built in an isolated worktree, per this project's standing shared-checkout-safety
//! rule (`~/dev/walnut-java` may have other agents committing to it concurrently):
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add /tmp/walnut-java-wb024025-resume bugfix/wb-024-025
//! cd /tmp/walnut-java-wb024025-resume
//! # (jar already built: target/Walnut-all.jar)
//! cat > "Command Files/resume_capture.txt" <<'EOF'
//! reg r19new {0,1} "2*";
//! reg r50new {0,1,2,3} {0,1} "[9,9][0,0]";
//! reg r50rev {0,1,2,3} {0,1} "[0,0][9,9]";
//! reg r58new {0,1} "[10]";
//! EOF
//! /opt/homebrew/opt/openjdk@17/bin/java -cp target/Walnut-all.jar Main.Prover \
//!     resume_capture.txt >stdout.txt 2>stderr.txt
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb024025-resume --force
//! ```
//!
//! (JDK 17+ selected explicitly, same as `java_bugfix_wb010.rs`'s recipe — the shell's
//! default `java` resolves to a JDK 11 too old for this project's class file version.)
//!
//! Captured 2026-08-20 against `59eda64`. `stderr.txt` was empty. `stdout.txt` (up to
//! the REPL banner that follows the command file) was:
//!
//! ```text
//! reg r19new {0,1} "2*";
//! digit 2 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1]
//! reg r50new {0,1,2,3} {0,1} "[9,9][0,0]";
//! digit 9 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1, 2, 3]
//! reg r50rev {0,1,2,3} {0,1} "[0,0][9,9]";
//! digit 9 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1, 2, 3]
//! reg r58new {0,1} "[10]";
//! digit 10 in position 0 of a regular-expression vector is not in that input's alphabet: [0, 1]
//! ```
//!
//! None of `r19new.txt`/`r50new.txt`/`r50rev.txt`/`r58new.txt` were written to
//! `Automata Library/` — each command errors out before writing anything, on both
//! engines. The command file and worktree were removed afterward, matching
//! `../CAPTURE.md`'s established practice.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `java_bugfix_wb010.rs`'s own `Capture`
/// (duplicated across this file family; see that file's matching note for why no shared
/// helper crate exists for these ~15-line structs).
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
    }
}

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A process-scoped Walnut home tree plus a `Prover` over it, with `console`/`err`
/// standing in for real stdout/stderr — same shape as `java_bugfix_wb010.rs`'s `prover`
/// helper.
fn prover(tag: &str) -> (Prover, Capture, Capture, PathBuf) {
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
    let err = Capture::default();
    let logging = Logging::with_writers(Box::new(console.clone()), Box::new(err.clone()));
    (
        Prover::with_output(session, logging, Box::new(console.clone())),
        console,
        err,
        dir,
    )
}

/// WB-024's `r19` shape: a BARE (unbracketed) out-of-alphabet digit. `2` is not in the
/// declared `{0,1}` alphabet; before the fix this silently built a one-state,
/// `{ε}`-language automaton (`fixtures/reg/r19.txt`'s old content: state `0`, accepting,
/// no transitions). Now it cleanly rejects, matching real fixed `walnut-java` verbatim.
#[test]
fn wb024_r19_bare_digit_now_rejects() {
    let (mut p, console, err, dir) = prover("wb024-r19");

    let mut input = io::Cursor::new(b"reg r19new {0,1} \"2*\";\n".to_vec());
    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "reg r19new {0,1} \"2*\";\n\
         digit 2 in position 0 of a regular-expression vector is not in that input's \
         alphabet: [0, 1]\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-024-025, commit 59eda64)"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here would mean the error is misclassified as an unhandled JDK \
         exception instead of the clean message-only path"
    );
    assert!(
        !dir.join("Automata Library/r19new.txt").exists(),
        "the command errors out before writing anything, on both engines"
    );

    fs::remove_dir_all(&dir).ok();
}

/// WB-024's headline symptom, `r50`'s exact pair: the SAME two vectors, differing only in
/// order, used to behave completely differently (silently empty language in one order, an
/// opaque dk.brics parse error in the other). Now both orders reject IDENTICALLY, with the
/// same digit-naming message, matching real fixed `walnut-java` verbatim.
#[test]
fn wb024_r50_bracketed_vectors_reject_regardless_of_order() {
    let (mut p, console, err, dir) = prover("wb024-r50");

    let mut input = io::Cursor::new(
        b"reg r50new {0,1,2,3} {0,1} \"[9,9][0,0]\";\n\
          reg r50rev {0,1,2,3} {0,1} \"[0,0][9,9]\";\n"
            .to_vec(),
    );
    p.read_buffer(&mut input, false);

    let expected_line = "digit 9 in position 0 of a regular-expression vector is not in \
                          that input's alphabet: [0, 1, 2, 3]";
    assert_eq!(
        console.text(),
        format!(
            "reg r50new {{0,1,2,3}} {{0,1}} \"[9,9][0,0]\";\n\
             {expected_line}\n\
             reg r50rev {{0,1,2,3}} {{0,1}} \"[0,0][9,9]\";\n\
             {expected_line}\n"
        ),
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-024-025, commit 59eda64) -- both orders reject with the SAME message, \
         closing the order-dependence WB-024 was named for"
    );
    assert_eq!(err.text(), "");
    assert!(!dir.join("Automata Library/r50new.txt").exists());
    assert!(!dir.join("Automata Library/r50rev.txt").exists());

    fs::remove_dir_all(&dir).ok();
}

/// WB-024's `r58` shape: `[10]` reads as the one-element vector holding the integer `10`
/// (not as a two-character class), which is in neither track of the declared `{0,1}`
/// alphabet. Before the fix this silently became the empty language
/// (`fixtures/reg/r58.txt`'s old content: state `0`, non-accepting, no transitions). Now
/// it cleanly rejects, matching real fixed `walnut-java` verbatim.
#[test]
fn wb024_r58_bracketed_integer_outside_alphabet_now_rejects() {
    let (mut p, console, err, dir) = prover("wb024-r58");

    let mut input = io::Cursor::new(b"reg r58new {0,1} \"[10]\";\n".to_vec());
    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "reg r58new {0,1} \"[10]\";\n\
         digit 10 in position 0 of a regular-expression vector is not in that input's \
         alphabet: [0, 1]\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-024-025, commit 59eda64)"
    );
    assert_eq!(err.text(), "");
    assert!(!dir.join("Automata Library/r58new.txt").exists());

    fs::remove_dir_all(&dir).ok();
}
