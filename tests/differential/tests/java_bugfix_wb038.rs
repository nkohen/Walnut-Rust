// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-038** (`docs/WALNUT-BUGS.md`), the `bugfix/wb-038`
//! follow-up to `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s downstream port workflow. Checked
//! against real `walnut-java` output **captured against the FIXED branch** `bugfix/wb-038`
//! (commit `601a9d2`, stacked on `bugfix/wb-021` (`c0d7fff`)), **not mainline** — see
//! `java_bugfix_wb002.rs`'s module docs for why that's this project's `../CAPTURE.md`
//! discipline for these follow-up units.
//!
//! # `AutomatonReader` silently accepted out-of-alphabet transition digits
//!
//! `AutomatonReader.validateTransition` used to check only a transition line's **arity**,
//! never whether each digit is a member of its track's declared alphabet. Downstream,
//! `RichAlphabet.encode` computes each track's contribution as
//! `encoder[i] * A.get(i).indexOf(digit)`, and `List.indexOf` answers `-1` for an absent
//! digit rather than raising — so an out-of-alphabet digit produced a bogus encoded key
//! instead of an error, with three possible outcomes, all silent at read time:
//!
//! * the key survived into the automaton and was dropped by the next pass iterating
//!   `0..alphabetSize`, so Walnut computed over **a different language than the file
//!   described, with no diagnostic at all**;
//! * on more than one track the `-1` terms could cancel onto a *valid* key, so the file
//!   silently meant a different tuple than it spelled (the aliasing case — the nastiest,
//!   since nothing downstream could ever notice);
//! * or the key reached a `decode` and died with an opaque
//!   `IndexOutOfBoundsException: Index -1 out of bounds for length 2`.
//!
//! The fix (`walnut-java` commit `601a9d2`) adds a per-digit alphabet-membership check
//! beside the existing arity check, skipping `null` (`*` wildcard) entries, so all three
//! are refused at read time with one clear message. `readTransducer` shares the same
//! `validateTransition`, so transducer files are fixed by the same change.
//!
//! # What this file asserts, and the one thing it deliberately does NOT
//!
//! Each case below drives `wr-cli`'s real dispatch loop (`Prover::read_buffer`, the same
//! path the CLI uses) on the same library files the capture used, and asserts:
//!
//! * the command **fails**, where before the fix it silently succeeded (or crashed);
//! * the reported message carries Java's new text **verbatim** — digit, 1-based position,
//!   the track's `List<Integer>`-shaped alphabet, and the line number
//!   ([`java_message_shape`] builds it, so a wrong position or a wrong alphabet rendering
//!   fails here even though the outcome would be the same);
//! * **nothing is written**, matching the fixed jar's own session directory;
//! * the session survives and the NEXT command still runs (`Prover.readBuffer`'s
//!   `catch`), and a `*` wildcard file plus a well-formed transducer are unaffected — the
//!   over-tightening regressions.
//!
//! It does **not** byte-compare the whole console transcript the way
//! `java_bugfix_wb024_wb025.rs` does, for two pre-existing reasons that are orthogonal to
//! WB-038 and would otherwise be laundered into this file's assertions:
//!
//! 1. `wr_logic::predicate_env::PredicateEnvError::MalformedAutomaton` wraps every reader
//!    error in `"File does not parse: {address} ({detail})"`, which Java does not do at
//!    all — its `WalnutException` propagates out of the reader unchanged. That divergence
//!    is documented on that variant (with the two things `wr_io::ReadError` needed before
//!    it could be unwrapped — both since done, the unwrapping itself not yet) and predates
//!    this unit by three phases.
//! 2. Java names the file by its **relative** library path (`Automata Library/x.txt`)
//!    while this harness runs over an absolute temp directory, so the tail of the message
//!    genuinely cannot match byte-for-byte here.
//!
//! Both are checked structurally instead ([`assert_reports_wb038`]), and the *whole*
//! message is printed on failure so a regression in either half is visible rather than
//! swallowed. For the record, what this port prints where Java prints the bare message
//! (temp path elided) is:
//!
//! ```text
//! File does not parse: <dir>/Automata Library/wb038fw.txt (digit 5 in position 1 is not in the alphabet [0, 1] of that input: line 7 of file <dir>/Automata Library/wb038fw.txt)
//! ```
//!
//! — Java's text verbatim, inside that wrapper, with the address absolutized twice.
//!
//! ## Capture recipe (reproducible)
//!
//! Built in an isolated worktree, per this project's standing shared-checkout-safety rule
//! (`~/dev/walnut-java` may have other agents committing to it concurrently) — and note
//! `bugfix/wb-038` was ALREADY checked out at the main `~/dev/walnut-java` working tree
//! when this was captured, so the worktree below is added by commit hash (detached), not
//! by branch name, to avoid git's "branch already checked out" refusal (the same situation
//! `java_bugfix_wb021.rs`/`java_bugfix_wb032.rs` hit):
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb038 601a9d2
//! cd /tmp/walnut-java-wb038
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! printf 'msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n'  > "Automata Library/wb038fw.txt"
//! printf ' lsd_2\n0 1\n20 -> 0\n'                             > "Automata Library/wb038fy.txt"
//! printf 'msd_2 msd_2\n0 1\n5 1 -> 0\n'                       > "Automata Library/wb038alias.txt"
//! printf 'msd_2\n0 1\n* -> 0\n'                               > "Automata Library/wb038wild.txt"
//! printf '{0, 1}\n\n0\n0 -> 0 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 1\n5 -> 0 / 0\n' \
//!                                                       > "Transducer Library/wb038td.txt"
//!
//! cat > "Command Files/wb038_capture.txt" <<'EOF'
//! def wb038e "?msd_2 $wb038fw(x)";
//! eval wb038b "?lsd_2 $wb038fy(x)";
//! eval wb038al "?msd_2 $wb038alias(x,y)";
//! eval wb038w "?msd_2 $wb038wild(x)";
//! transduce wb038tr wb038td T;
//! eval wb038alive "?msd_2 Ex x = 1";
//! EOF
//! java -cp target/Walnut-all.jar Main.Prover wb038_capture.txt \
//!     >stdout.txt 2>stderr.txt </dev/null
//!
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb038 --force
//! ```
//!
//! `java`/`mvnw` above actually ran under a JDK 17+ toolchain
//! (`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1`, selected explicitly
//! since the shell's default `java` resolves to a JDK 11 too old for this project's class
//! file version) — noted here since it's a prerequisite this recipe silently assumes
//! otherwise, per `java_bugfix_wb010.rs`'s own note. `</dev/null` matters: without it the
//! process runs the command file and then blocks in the REPL.
//!
//! ## Captured output (2026-08-20, `601a9d2`)
//!
//! `stderr.txt` was empty. `stdout.txt`, up to the REPL banner that follows the command
//! file:
//!
//! ```text
//! def wb038e "?msd_2 $wb038fw(x)";
//! digit 5 in position 1 is not in the alphabet [0, 1] of that input: line 7 of file Automata Library/wb038fw.txt
//! eval wb038b "?lsd_2 $wb038fy(x)";
//! digit 20 in position 1 is not in the alphabet [0, 1] of that input: line 3 of file Automata Library/wb038fy.txt
//! eval wb038al "?msd_2 $wb038alias(x,y)";
//! digit 5 in position 1 is not in the alphabet [0, 1] of that input: line 3 of file Automata Library/wb038alias.txt
//! eval wb038w "?msd_2 $wb038wild(x)";
//! transduce wb038tr wb038td T;
//! digit 5 in position 1 is not in the alphabet [0, 1] of that input: line 9 of file Transducer Library/wb038td.txt
//! eval wb038alive "?msd_2 Ex x = 1";
//! Converted from brics:2 states - 5ms
//! ____
//! TRUE
//! ```
//!
//! The surviving session directory held `Automata Library/wb038w.txt` and
//! `Automata Library/wb038alive.txt` and **nothing else** — i.e. the wildcard file's
//! `eval` and the final liveness `eval` are the only two commands that produced anything.
//! `Result/` additionally held a `*_log.txt` for every command including the failed ones
//! (Walnut opens the log before dispatching), which is why the tests below assert on the
//! `Automata Library` outputs rather than on `Result/`'s contents.
//!
//! Two shapes from the same capture session are asserted at the `wr-io` layer instead of
//! here, because they need no CLI to be meaningful and the message text is the whole
//! point: a bad digit in a NON-first track position (`msd_2 msd_3` / `1 7 -> 0` →
//! `digit 7 in position 2 … [0, 1, 2] … line 3`) and a `{...}`-set alphabet (`{0, 1, 3}` /
//! `2 -> 0` → `digit 2 in position 1 … [0, 1, 3] … line 3`). See
//! `the_out_of_alphabet_message_matches_real_walnut_text` in `crates/wr-io/src/reader.rs`.
//!
//! The command file, the six library files and the worktree were removed afterward,
//! matching every recipe in `../CAPTURE.md`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `java_bugfix_wb024_wb025.rs`'s own
/// `Capture` (duplicated across this file family; see `java_bugfix_wb010.rs`'s matching
/// note for why no shared helper crate exists for these ~15-line structs).
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
/// standing in for real stdout/stderr.
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

/// Java's fixed message, minus the trailing file address (see this module's docs for why
/// the address is not compared here). `position` is 1-based, exactly as Java prints it,
/// and `alphabet` is rendered `List<Integer>`-style.
fn java_message_shape(digit: i32, position: usize, alphabet: &str, line: usize) -> String {
    format!(
        "digit {digit} in position {position} is not in the alphabet {alphabet} \
         of that input: line {line} of file "
    )
}

/// Asserts that a captured console transcript reports WB-038's fixed rejection for
/// `file_name`, exactly once, with Java's message text verbatim up to the file address.
fn assert_reports_wb038(
    console: &Capture,
    err: &Capture,
    digit: i32,
    position: usize,
    alphabet: &str,
    line: usize,
    file_name: &str,
) {
    let text = console.text();
    let shape = java_message_shape(digit, position, alphabet, line);
    assert!(
        text.contains(&shape),
        "must carry real fixed walnut-java's message verbatim (captured against \
         bugfix/wb-038, commit 601a9d2).\nexpected to contain: {shape}\nfull console:\n{text}"
    );
    // ...and the address that follows it must name the offending FILE, not some other
    // operand that happened to be loaded in the same command. The address runs to the end
    // of the line, save for the `)` that closes `MalformedAutomaton`'s wrapper (see this
    // module's docs) -- stripped here rather than matched, so this assertion keeps working
    // if that pre-existing wrapper is ever removed.
    let after = text.split(&shape).nth(1).unwrap_or_default();
    let address = after.lines().next().unwrap_or("").trim_end_matches(')');
    assert!(
        address.ends_with(file_name),
        "the message must name {file_name}, got address {address:?}; full console:\n{text}"
    );
    assert_eq!(
        text.matches(&shape).count(),
        1,
        "reported exactly once -- a doubled report would mean the file is read twice and \
         the second read's failure is also surfaced.\nfull console:\n{text}"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here would mean the error is misclassified as an unhandled JDK exception \
         instead of the clean message-only path.\nfull console:\n{text}"
    );
}

/// Java's `Automata Library/wb038fw.txt`: `5` is not in the `msd_2` alphabet `{0,1}`, and
/// the destination state IS declared — WB-038's **outcome (b)**, the silent one. Pre-fix,
/// `def wb038e "?msd_2 $wb038fw(x)";` succeeded on BOTH engines and wrote out the input
/// automaton *minus* the `5 -> 1` line, with no diagnostic whatsoever.
#[test]
fn wb038_silently_dropped_transition_now_rejects_at_load() {
    let (mut p, console, err, dir) = prover("wb038-drop");
    fs::write(
        dir.join("Automata Library/wb038fw.txt"),
        "msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n",
    )
    .unwrap();

    let mut input = io::Cursor::new(
        b"def wb038e \"?msd_2 $wb038fw(x)\";\nreg wb038after msd_2 \"1*\";\n".to_vec(),
    );
    p.read_buffer(&mut input, false);

    assert_reports_wb038(&console, &err, 5, 1, "[0, 1]", 7, "wb038fw.txt");
    assert!(
        !dir.join("Automata Library/wb038e.txt").exists(),
        "the command errors out before writing anything, on both engines"
    );
    // `Prover.readBuffer`'s `catch` -- the NEXT command still runs.
    assert!(
        dir.join("Automata Library/wb038after.txt").is_file(),
        "the session must survive: {}",
        console.text()
    );

    fs::remove_dir_all(&dir).ok();
}

/// Java's `Automata Library/wb038fy.txt`: digit `20` under `lsd_2`, WB-038's **outcome
/// (c)**. Pre-fix, `eval wb038b "?lsd_2 $wb038fy(x)";` loaded fine and then died on the
/// way out with `java.lang.IndexOutOfBoundsException: Index -1 out of bounds for length 2`
/// from `RichAlphabet.decode` — an opaque failure a long way from its cause. Now it is
/// refused at the line that causes it, naming that line.
#[test]
fn wb038_index_out_of_bounds_crash_becomes_a_clean_load_time_error() {
    let (mut p, console, err, dir) = prover("wb038-crash");
    fs::write(
        dir.join("Automata Library/wb038fy.txt"),
        " lsd_2\n0 1\n20 -> 0\n",
    )
    .unwrap();

    let mut input = io::Cursor::new(
        b"eval wb038b \"?lsd_2 $wb038fy(x)\";\nreg wb038after lsd_2 \"1*\";\n".to_vec(),
    );
    p.read_buffer(&mut input, false);

    assert_reports_wb038(&console, &err, 20, 1, "[0, 1]", 3, "wb038fy.txt");
    assert!(
        !console.text().contains("out of bounds"),
        "the old opaque JDK message must be gone, not merely accompanied: {}",
        console.text()
    );
    assert!(!dir.join("Automata Library/wb038b.txt").exists());
    assert!(dir.join("Automata Library/wb038after.txt").is_file());

    fs::remove_dir_all(&dir).ok();
}

/// The **aliasing** case, and the reason this bug had to be fixed in the reader rather
/// than by any downstream guard. Under `msd_2 msd_2` (`encoder = [1, 2]`), `5 1 -> 0`
/// encodes to `1*(-1) + 2*1 == 1` — exactly the key the legitimate tuple `(1, 0)` would
/// have — so pre-fix both engines read this file as an automaton over `(1, 0)`, with no
/// crash and no diagnostic anywhere for anything downstream to catch.
#[test]
fn wb038_aliasing_case_now_rejects_instead_of_silently_meaning_another_tuple() {
    let (mut p, console, err, dir) = prover("wb038-alias");
    fs::write(
        dir.join("Automata Library/wb038alias.txt"),
        "msd_2 msd_2\n0 1\n5 1 -> 0\n",
    )
    .unwrap();

    let mut input = io::Cursor::new(
        b"eval wb038al \"?msd_2 $wb038alias(x,y)\";\nreg wb038after msd_2 \"1*\";\n".to_vec(),
    );
    p.read_buffer(&mut input, false);

    assert_reports_wb038(&console, &err, 5, 1, "[0, 1]", 3, "wb038alias.txt");
    assert!(!dir.join("Automata Library/wb038al.txt").exists());
    assert!(dir.join("Automata Library/wb038after.txt").is_file());

    fs::remove_dir_all(&dir).ok();
}

/// `readTransducer` shares Java's `validateTransition`, so a transducer file with the same
/// defect is fixed by the same change — the second call site, and the one a fix applied
/// only to `readAutomaton` would have missed.
#[test]
fn wb038_transducer_file_with_an_out_of_alphabet_digit_now_rejects() {
    let (mut p, console, err, dir) = prover("wb038-transducer");
    fs::write(
        dir.join("Transducer Library/wb038td.txt"),
        "{0, 1}\n\n0\n0 -> 0 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 1\n5 -> 0 / 0\n",
    )
    .unwrap();
    // A well-formed word automaton to transduce, so the command fails on the TRANSDUCER
    // and not on its other operand.
    fs::write(
        dir.join("Word Automata Library/wb038word.txt"),
        "msd_2\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 0\n1 -> 1\n",
    )
    .unwrap();

    let mut input = io::Cursor::new(
        b"transduce wb038tr wb038td wb038word;\nreg wb038after msd_2 \"1*\";\n".to_vec(),
    );
    p.read_buffer(&mut input, false);

    assert_reports_wb038(&console, &err, 5, 1, "[0, 1]", 9, "wb038td.txt");
    assert!(!dir.join("Word Automata Library/wb038tr.txt").exists());
    assert!(dir.join("Automata Library/wb038after.txt").is_file());

    fs::remove_dir_all(&dir).ok();
}

/// The over-tightening regression, and the single most important test here: a `*` wildcard
/// is not a literal digit (it is expanded from the track's own alphabet by
/// `RichAlphabet.expandWildcard`), so it must still be accepted. Java's fix skips its
/// `null` entries for exactly this reason. The fixed jar evaluates
/// `eval wb038w "?msd_2 $wb038wild(x)";` with no output and writes `wb038w.txt`.
#[test]
fn wb038_a_wildcard_transition_is_still_accepted() {
    let (mut p, console, err, dir) = prover("wb038-wildcard");
    fs::write(
        dir.join("Automata Library/wb038wild.txt"),
        "msd_2\n0 1\n* -> 0\n",
    )
    .unwrap();

    let mut input = io::Cursor::new(b"eval wb038w \"?msd_2 $wb038wild(x)\";\n".to_vec());
    p.read_buffer(&mut input, false);

    assert!(
        !console.text().contains("is not in the alphabet"),
        "a wildcard must not be treated as an out-of-alphabet digit: {}",
        console.text()
    );
    assert_eq!(err.text(), "");
    assert!(
        dir.join("Automata Library/wb038w.txt").is_file(),
        "the fixed jar writes this file too: {}",
        console.text()
    );

    fs::remove_dir_all(&dir).ok();
}

/// The other half of "the fix does not over-tighten": a well-formed library file — every
/// digit in its track's alphabet — behaves exactly as it did before, through the same
/// `eval` path the rejecting cases above use. Uses a real corpus transducer
/// (`RUNSUM2.txt`, the same file the capture session checked on the Java side) plus a
/// plain `reg`-built automaton, so the check covers both readers.
#[test]
fn wb038_well_formed_files_are_unaffected() {
    let (mut p, console, err, dir) = prover("wb038-wellformed");
    let runsum2 = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/wr-io/tests/fixtures")
        .join("RUNSUM2.txt");
    fs::copy(&runsum2, dir.join("Transducer Library/RUNSUM2.txt")).unwrap();
    fs::write(
        dir.join("Word Automata Library/wb038word.txt"),
        "msd_2\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 0\n1 -> 1\n",
    )
    .unwrap();

    let mut input = io::Cursor::new(
        b"reg wb038ok msd_2 \"0*1\";\n\
          eval wb038okq \"?msd_2 Ex $wb038ok(x)\";\n\
          transduce wb038trok RUNSUM2 wb038word;\n"
            .to_vec(),
    );
    p.read_buffer(&mut input, false);

    assert!(
        !console.text().contains("is not in the alphabet"),
        "no well-formed file may be rejected by the new check: {}",
        console.text()
    );
    assert_eq!(err.text(), "");
    assert!(dir.join("Automata Library/wb038ok.txt").is_file());
    assert!(
        dir.join("Word Automata Library/wb038trok.txt").is_file(),
        "transduce over a well-formed transducer still works: {}",
        console.text()
    );

    fs::remove_dir_all(&dir).ok();
}
