// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-019** (`docs/WALNUT-BUGS.md`), PR-18 of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md` — the last entry in this mechanical batch.
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-019` (commit `cee8352`, stacked on `bugfix/wb-040`'s `0cf02d3`),
//! **not mainline** — see `java_bugfix_wb002.rs`'s module docs for why that's
//! established practice for these follow-up units, and the same note about
//! re-pointing the commit reference once that branch merges upstream.
//!
//! # `putMacro`'s `%N` substitution, no longer regex-replacement-string-aware
//!
//! `Predicate.putMacro` (`:419-459`) used to build each macro call's expanded body via
//! `String.replaceAll("%" + arg, arguments.get(arg))`. The PATTERN side is harmless
//! (always plain digits), but the REPLACEMENT side — `arguments.get(arg)`, a macro
//! call's raw, verbatim argument text — used to be parsed through
//! `java.util.regex.Matcher.appendReplacement`'s own mini-language, where `$`
//! introduces a group reference and `\` escapes the next character. A macro argument
//! containing `\x` silently dropped the backslash; a lone trailing `\` threw an
//! uncaught `IllegalArgumentException`. Fixed upstream by switching to
//! `String.replace(CharSequence, CharSequence)`, which does no regex/replacement-string
//! parsing on either side. This port's equivalent fix is a genuine simplification:
//! Rust's `str::replace` was ALREADY a purely literal replace with no escape
//! semantics, so the fix is switching `put_macro`'s call site over to it directly and
//! deleting the now-dead `java_replace_all_literal`/`expand_java_replacement`
//! machinery (and `LexError::MacroArgumentReplacementError`) that used to faithfully
//! reproduce Java's old escape-mini-language quirk.
//!
//! ## Capture recipe (reproducible, against the FIXED branch)
//!
//! ```bash
//! cd ~/dev/walnut-java   # bugfix/wb-019, already built: target/Walnut-all.jar
//! cat > "Macro Library/wb019scratch_echo.txt" <<'EOF'
//! %0
//! EOF
//! printf 'eval wb019scratch_out "#wb019scratch_echo(\\)";\n\
//! eval wb019scratch_out2 "#wb019scratch_echo(\\x)";\n\
//! eval wb019scratch_out3 "#wb019scratch_echo($5)";\n' \
//!     > "Command Files/wb019scratch_capture.txt"
//! java -cp target/Walnut-all.jar Main.Prover wb019scratch_capture.txt \
//!     >stdout.txt 2>stderr.txt </dev/null
//! rm -f "Macro Library/wb019scratch_echo.txt" "Command Files/wb019scratch_capture.txt"
//! rm -rf Session
//! ```
//!
//! `stderr.txt` was empty for all three commands (confirming no uncaught
//! `IllegalArgumentException`, no anything). `stdout.txt` (captured 2026-08-22,
//! `cee8352`, after a fresh `./mvnw -q clean package -DskipTests -Pfat-jar` to make
//! sure the jar actually reflected the fix — the first run, against a stale jar, still
//! showed the pre-fix crash, which is what caught the staleness):
//!
//! ```text
//! eval wb019scratch_out "#wb019scratch_echo(\)";
//! Undefined token: char at 0
//! eval wb019scratch_out2 "#wb019scratch_echo(\x)";
//! Undefined token: char at 0
//! eval wb019scratch_out3 "#wb019scratch_echo($5)";
//! a function/macro cannot be called from inside another function/macro's argument list: char at 19
//! Welcome to Walnut v8.0-alpha! ...
//! ```
//!
//! Before `cee8352` the first two commands crashed as described above (uncaught,
//! non-`WalnutException` text, confirmed in an earlier capture attempt in this same
//! session before the jar was rebuilt). None of the three commands write an automaton
//! file (`Session/*/Automata Library/` stayed empty across all three) — every one
//! fails before `EvalDef.compute` could ever produce a result. The third command
//! (`$5`, the `$`-group-reference control case) is unaffected by this fix either way —
//! `parseParenthesizedArguments`'s `$`/`#` guard rejects it before `putMacro`'s
//! substitution ever runs — included here to confirm that ordering held identically
//! before and after, matching `walnut-java`'s own new
//! `macroCallArgumentWithDollarSignStillBlockedBeforeSubstitutionRuns` test.
//!
//! ## The SILENT-WRONG-ANSWER half (`\x=1`), captured separately on both sides
//!
//! Every case above pins the crash half of WB-019 — an argument that errors on both
//! sides of the fix (uncaught pre-fix, a clean `Undefined token` post-fix). The higher-
//! severity half — an argument that used to silently compute and WRITE a wrong answer,
//! not just crash or misprint in memory — has no coverage above, because bare `\x` isn't
//! a complete predicate even with the backslash dropped. `\x=1` is: pre-fix,
//! `Matcher.appendReplacement` dropped the backslash and `#wb019scratch_echo(\x=1)`
//! silently became the perfectly valid, computable predicate `x=1`.
//!
//! Captured against a fresh build of the commit immediately BEFORE `cee8352`
//! (`0cf02d3`, in an isolated `git worktree` off `~/dev/walnut-java`, its untracked
//! `.java-version` copied over so `jenv` picks the right JDK):
//!
//! ```bash
//! git worktree add /tmp/wj-prefix-check 0cf02d3
//! cp ~/dev/walnut-java/.java-version /tmp/wj-prefix-check/.java-version
//! cd /tmp/wj-prefix-check && ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Macro Library/wb019scratch_echo.txt" <<'EOF'
//! %0
//! EOF
//! printf 'eval wb019scratch_out4 "#wb019scratch_echo(\\x=1)";\n' \
//!     > "Command Files/wb019scratch_capture2.txt"
//! java -cp target/Walnut-all.jar Main.Prover wb019scratch_capture2.txt \
//!     >stdout2.txt 2>stderr2.txt </dev/null
//! ```
//!
//! `stdout2.txt`: `eval wb019scratch_out4 "#wb019scratch_echo(\x=1)";` followed by
//! `Converted from brics:2 states - 5ms`, no error at all; `stderr2.txt` empty; and
//! `find . -path '*/Automata Library/wb019scratch_out4.txt'` finds a real file, under
//! `Session/…/Automata Library/`, containing:
//!
//! ```text
//! msd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 1
//!
//! 1 1
//! ```
//!
//! — a genuine two-state automaton accepting exactly the representation of `1`, silently
//! computed and persisted for a query (`\x=1`) that has no valid interpretation at all
//! (Walnut's predicate grammar has no escape syntax, so a *correct* engine can only ever
//! reject this input). The SAME command, re-run against the fixed jar
//! (`~/dev/walnut-java` on `bugfix/wb-019`, freshly rebuilt to rule out jar staleness
//! per the note above) instead prints `Undefined token: char at 0` and writes nothing —
//! matching `wb019_backslash_prefixed_valid_predicate_argument_no_longer_silently_computes_wrong_result`
//! below exactly.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated in several
/// sibling `java_bugfix_wb*.rs` files in this directory).
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
/// standing in for real stdout/stderr — same shape as `java_bugfix_wb013.rs`'s copy of
/// this helper. Also writes the one shared macro fixture every test below calls
/// through (`wb019scratch_echo`, body `%0` — the same shape as `walnut-java`'s own
/// `PredicateTest` fixture `my_macro0.txt`).
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
    fs::write(dir.join("Macro Library/wb019scratch_echo.txt"), "%0\n").unwrap();
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

/// WB-019 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `cee8352`
/// (branch `bugfix/wb-019`): a macro-call argument ending in a lone, unescaped
/// backslash used to throw an uncaught `IllegalArgumentException` out of `putMacro`'s
/// substitution step. Now the backslash survives substitution as literal text
/// (`wr_logic::predicate::Predicate::put_macro`'s expanded body becomes `"\"`, one
/// character), and tokenizing that then correctly, cleanly rejects it as
/// `LexError::UndefinedToken` — matching real, fixed `walnut-java`'s
/// `Undefined token: char at 0` exactly, with nothing on stderr.
#[test]
fn wb019_trailing_backslash_argument_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb019a");
    let mut input =
        io::Cursor::new(b"eval wb019scratch_out \"#wb019scratch_echo(\\)\";\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "eval wb019scratch_out \"#wb019scratch_echo(\\)\";\n\
         Undefined token: char at 0\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-019, commit cee8352) -- read_buffer's own echo of the command line \
         precedes the printed message; unlike WB-013's Act-stage failures this is a \
         Lex-stage (tokenizing) failure, so the message prints exactly once, not \
         doubled"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here would mean the port is still reproducing the old uncaught, \
         non-WalnutException crash"
    );
    assert!(
        !dir.join("Automata Library/wb019scratch_out.txt").exists(),
        "the command errors out before writing anything"
    );

    fs::remove_dir_all(&dir).ok();
}

/// WB-019 (`docs/WALNUT-BUGS.md`), same fix: `\x` in a macro-call argument used to
/// silently become literal `x` (the backslash consumed as `Matcher.appendReplacement`'s
/// escape-the-next-character marker). Now both characters survive substitution
/// verbatim (`"\x"`, not `"x"`) -- and the very fact that tokenizing this then throws
/// (where the old buggy one-character `"x"` alone would NOT, `x` being a perfectly
/// ordinary `Variable` token) is itself the observable proof the backslash was
/// preserved, not dropped. Matches real, fixed `walnut-java` exactly.
#[test]
fn wb019_backslash_escape_sequence_argument_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb019b");
    let mut input =
        io::Cursor::new(b"eval wb019scratch_out2 \"#wb019scratch_echo(\\x)\";\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "eval wb019scratch_out2 \"#wb019scratch_echo(\\x)\";\n\
         Undefined token: char at 0\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-019, commit cee8352)"
    );
    assert_eq!(err.text(), "");
    assert!(!dir.join("Automata Library/wb019scratch_out2.txt").exists());

    fs::remove_dir_all(&dir).ok();
}

/// WB-019 (`docs/WALNUT-BUGS.md`) -- the higher-severity, SILENT-WRONG-ANSWER half of
/// this bug, which every other case in this file (and every unit test in
/// `predicate.rs`) leaves untested: they all pin a case that ERRORS on both sides of the
/// fix (uncaught crash before, clean `Undefined token` after). `\x` alone isn't a
/// complete predicate even with the backslash dropped (bare `x` lexes fine as a
/// `Variable` token but never reaches a full, computable predicate), so it can't
/// demonstrate this. `\x=1` can: pre-fix, `Matcher.appendReplacement`'s escape
/// mini-language silently dropped the backslash, turning the macro-call argument into
/// the perfectly valid predicate `x=1`, which `EvalDef.compute` happily evaluated and
/// wrote out as a real (silently WRONG -- there being no valid interpretation of the
/// user's literal `\x=1` at all, Walnut's grammar having no escape syntax) automaton
/// file. Post-fix, the same command correctly fails to tokenize at all and writes
/// nothing.
///
/// Confirmed live on both sides before writing this assertion (2026-08-22, per this
/// unit's brief): against a fresh build of the FIXED jar (`bugfix/wb-019`, `cee8352`),
/// `eval wb019scratch_out4 "#wb019scratch_echo(\x=1)";` prints `Undefined token: char at
/// 0` and the run's `Automata Library/` stays empty. Against a fresh build of the commit
/// immediately BEFORE the fix (`0cf02d3`, checked out in an isolated `git worktree`, its
/// own untracked `.java-version` copied over so the right JDK is picked up), the exact
/// same command instead prints `Converted from brics:2 states - Nms` and silently writes
/// `Session/…/Automata Library/wb019scratch_out4.txt` -- a genuine two-state `msd_2`
/// automaton accepting exactly the representation of `1`:
///
/// ```text
/// msd_2
///
/// 0 0
/// 0 -> 0
/// 1 -> 1
///
/// 1 1
/// ```
///
/// i.e. real Walnut, pre-fix, silently computed and persisted an answer to a query the
/// user never actually asked (there is no way to spell `x=1` as `\x=1` on purpose --
/// Walnut's predicate grammar has no escape syntax at all).
#[test]
fn wb019_backslash_prefixed_valid_predicate_argument_no_longer_silently_computes_wrong_result() {
    let (mut p, console, err, dir) = prover("wb019d");
    let mut input =
        io::Cursor::new(b"eval wb019scratch_out4 \"#wb019scratch_echo(\\x=1)\";\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "eval wb019scratch_out4 \"#wb019scratch_echo(\\x=1)\";\n\
         Undefined token: char at 0\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-019, commit cee8352)"
    );
    assert_eq!(err.text(), "");
    assert!(
        !dir.join("Automata Library/wb019scratch_out4.txt").exists(),
        "pre-fix, this EXACT command silently computed and wrote a real automaton file \
         for the predicate \"x=1\" -- the backslash-dropping half of WB-019, distinct \
         from (and more severe than) the crash half every other case in this file pins. \
         Post-fix the command must fail before EvalDef.compute ever runs, so nothing \
         gets written at all; a regression back toward the old dropped-backslash \
         behavior would make this assertion fail while the two tests above (which only \
         ever assert an error string) would stay green -- this is the one case in this \
         file that would actually catch that regression."
    );

    fs::remove_dir_all(&dir).ok();
}

/// Control case, unaffected by this fix either way: the `$`-group-reference half of
/// the same underlying `Matcher.appendReplacement` quirk can never reach `putMacro`'s
/// substitution step at all -- `parseParenthesizedArguments`'s `$`/`#` guard rejects
/// any `$` in a macro/function call's argument text first. Confirms that ordering
/// held identically before and after WB-019's fix (which only touches the
/// substitution logic, not the argument parser), matching `walnut-java`'s own new
/// `macroCallArgumentWithDollarSignStillBlockedBeforeSubstitutionRuns` test.
#[test]
fn wb019_dollar_argument_control_case_unaffected_by_fix() {
    let (mut p, console, err, dir) = prover("wb019c");
    let mut input =
        io::Cursor::new(b"eval wb019scratch_out3 \"#wb019scratch_echo($5)\";\n".to_vec());

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "eval wb019scratch_out3 \"#wb019scratch_echo($5)\";\n\
         a function/macro cannot be called from inside another function/macro's \
         argument list: char at 19\n",
        "must match real walnut-java's stdout verbatim (captured against bugfix/wb-019, \
         commit cee8352) -- identical before and after this fix"
    );
    assert_eq!(err.text(), "");
    assert!(!dir.join("Automata Library/wb019scratch_out3.txt").exists());

    fs::remove_dir_all(&dir).ok();
}
