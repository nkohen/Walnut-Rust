// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-036** (`docs/WALNUT-BUGS.md`), one of
//! `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s PR-16 bundle. Checked against real
//! `walnut-java` output **captured against the FIXED branch** `bugfix/wb-036` (commit
//! `732bec0`), **not mainline** — see `java_bugfix_wb002.rs`'s module docs for why
//! that's this project's standard for these follow-up units, and the same note about
//! re-pointing the commit reference once that branch merges upstream.
//!
//! # `Morphism.toWordAutomaton`'s domain/image-range mismatch
//!
//! `toWordAutomaton` used to build `Q = maxEntry + 1` states but a transition table
//! with only `mapping.size()` entries, with no check that the two agreed — so a
//! morphism whose domain didn't cover every value referenced in its own images (e.g.
//! `0->05 1->10`: two domain letters `{0,1}`, but the image of `0` references `5`)
//! returned a malformed automaton that crashed later, the first time anything walked
//! its transition table (in practice `AutomatonWriter`'s own per-state write loop
//! during `promote`'s output), with a bare `IndexOutOfBoundsException`. This port
//! already caught the shape at CONSTRUCTION time, before the malformed automaton could
//! ever escape `to_word_automaton` (see `crates/wr-core/src/morphism.rs`'s module
//! docs) — more robust than Java's old crash point, but until this fix it rendered its
//! own invented message text and (see below) on the wrong output channel.
//!
//! ## Capture recipe
//!
//! Safety note: `promote`'s destination writes into `Word Automata Library/`, and a
//! prior agent working on this exact bug accidentally overwrote real shipped fixtures
//! there (`P.txt`/`P2.txt`) by using bare names. Every name below is prefixed
//! `wb036scratch_`, which cannot collide with any real Library file, and the capture
//! itself ran in a throwaway detached worktree, never the shared checkout's own tree
//! — see `../CAPTURE.md`'s full recipe for the exact commands.
//!
//! ```bash
//! cd ~/dev/walnut-java   # bugfix/wb-036, already built: target/Walnut-all.jar
//! cat > "Command Files/wb036_capture.txt" <<'EOF'
//! morphism wb036scratch_badmor "0->05 1->10";
//! promote wb036scratch_out1 wb036scratch_badmor;
//! morphism wb036scratch_h2 "0->00 1->00";
//! promote wb036scratch_out2 wb036scratch_h2;
//! morphism wb036scratch_dualmor "0->5 1->0";
//! promote wb036scratch_out3 wb036scratch_dualmor;
//! EOF
//! java -cp target/Walnut-all.jar Main.Prover wb036_capture.txt < /dev/null
//! ```
//!
//! Output (captured 2026-08-22, `732bec0`), `stderr` empty throughout:
//!
//! ```text
//! morphism wb036scratch_badmor "0->05 1->10";
//! Defined with domain [0, 1] and range {0, 1, 5}promote wb036scratch_out1 wb036scratch_badmor;
//! A morphism's domain must cover every value referenced in its own images: found the value 5 in some image, but the domain only has 2 letters.
//! morphism wb036scratch_h2 "0->00 1->00";
//! Defined with domain [0, 1] and range {0}promote wb036scratch_out2 wb036scratch_h2;
//! morphism wb036scratch_dualmor "0->5 1->0";
//! Defined with domain [0, 1] and range {0, 5}promote wb036scratch_out3 wb036scratch_dualmor;
//! Number system msd_1 is not defined.
//! ```
//!
//! `wb036scratch_out1`/`wb036scratch_out3` are never written (each command errors out
//! before writing anything); `wb036scratch_out2` (the mirror-shape control) IS written,
//! and reads (`Word Automata Library/wb036scratch_out2.txt`):
//!
//! ```text
//! msd_2
//!
//! 0 0
//! 0 -> 0
//! 1 -> 0
//! ```
//!
//! Before `732bec0` the first case crashed with
//! `java.lang.IndexOutOfBoundsException: Index 2 out of bounds for length 2` instead of
//! the clean message above. This port already raised a clean, recoverable
//! `MorphismError::DomainDoesNotCoverImageRange` before this unit (never the raw Java
//! crash, and never even a malformed automaton) — this unit changes the message TEXT to
//! Java's own fixed wording, verbatim, and (see below) the rendering CHANNEL.
//!
//! # This file drives real dispatch, not just `Display`, for the same reason
//! `java_bugfix_wb037.rs` does
//!
//! Asserting only `err.to_string()` cannot detect a real classification bug: the
//! message text can be correct while `ProverError::is_handled()`'s arm for this variant
//! is stale (`false`, "unhandled JDK exception"), rendering the new message on the WRONG
//! channel — kind-prefixed (`Main.WalnutException: …`) to stderr, instead of the plain
//! line real (fixed) Walnut prints to stdout. `err.to_string()` never goes through
//! `Logging::print_truncated_stack_trace_with_length`, the code that actually decides
//! the channel/prefix, so it cannot tell the two apart. This file drives the command
//! through [`wr_cli::prover::Prover::read_buffer`] (the real rendering path `Prover::run`
//! / the CLI actually uses) and asserts BOTH streams, exactly as `java_bugfix_wb037.rs`
//! does for its own (structurally identical) case.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `wr_cli::prover`'s own private test-module
/// `Capture`, duplicated here since that one isn't exported (also duplicated in
/// `java_bugfix_wb002.rs`/`java_bugfix_wb037.rs`/`java_bugfix_wb044.rs`; each captures a
/// different stream, so a shared helper crate felt like more machinery than a few
/// ~15-line structs warrant).
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
/// standing in for real stdout/stderr. `console` backs BOTH `Prover`'s own `out` writer
/// AND `Logging`'s console writer — in real production (`Prover::new`) both are
/// `io::stdout()`, the same physical stream, so a command's own direct prints and
/// `Logging::print_truncated_stack_trace`'s rendering interleave on one stdout; sharing
/// one `Capture` here reproduces that merged view instead of splitting it into two
/// channels a real user's terminal never distinguishes.
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

/// WB-036 (`docs/WALNUT-BUGS.md`), fixed upstream in `walnut-java` commit `732bec0`
/// (branch `bugfix/wb-036`): `Morphism.toWordAutomaton`'s domain/image-range mismatch
/// used to escape as a malformed automaton and crash later, at write time, with a bare
/// `IndexOutOfBoundsException`; it now raises a clean `WalnutException` from inside
/// `toWordAutomaton` itself. This port already caught the shape at construction time
/// (before this unit) — the message text is now Java's fixed wording, and (the fix this
/// test actually pins) the error is now classified as a handled `WalnutException`, so it
/// renders message-only to stdout with nothing on stderr, matching fixed Java exactly.
#[test]
fn wb036_domain_gap_matches_fixed_java() {
    let (mut p, console, err, dir) = prover("wb036-domain-gap");
    let mut input = io::Cursor::new(
        b"morphism wb036scratch_badmor \"0->05 1->10\";\n\
          promote wb036scratch_out1 wb036scratch_badmor;\n"
            .to_vec(),
    );

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "morphism wb036scratch_badmor \"0->05 1->10\";\n\
         Defined with domain [0, 1] and range {0, 1, 5}\
         promote wb036scratch_out1 wb036scratch_badmor;\n\
         A morphism's domain must cover every value referenced in its own images: \
         found the value 5 in some image, but the domain only has 2 letters.\n",
        "must match real walnut-java's fixed stdout verbatim (captured against \
         bugfix/wb-036, commit 732bec0) -- read_buffer's own echo of each command line \
         (console=false) precedes each command's own output; morphism's own \"Defined \
         with domain...\" line has no trailing newline in real Walnut, matching the run \
         together with the next echoed line"
    );
    assert_eq!(
        err.text(),
        "",
        "fixed Java writes nothing to stderr for this WalnutException -- a non-empty \
         stderr here means the error is still being classified as an unhandled JDK \
         exception (kind-prefixed rendering), the exact bug this test exists to catch"
    );
    assert!(
        !dir.join("Word Automata Library/wb036scratch_out1.txt")
            .exists(),
        "the command errors out before writing anything"
    );

    fs::remove_dir_all(&dir).ok();
}

/// The MIRROR shape (a domain *wider* than the image range needs) is NOT WB-036 and
/// must stay completely unaffected by the fix -- Java genuinely accepts it, both before
/// and after `732bec0`, and this port already did too. Verified live against the fixed
/// jar: `promote` succeeds silently (no printed line at all) and the written automaton
/// is the 1-state, self-looping-on-both-digits shape captured in `../CAPTURE.md`.
#[test]
fn wb036_mirror_shape_still_succeeds_identically() {
    let (mut p, console, err, dir) = prover("wb036-mirror");
    let mut input = io::Cursor::new(
        b"morphism wb036scratch_h2 \"0->00 1->00\";\n\
          promote wb036scratch_out2 wb036scratch_h2;\n"
            .to_vec(),
    );

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "morphism wb036scratch_h2 \"0->00 1->00\";\n\
         Defined with domain [0, 1] and range {0}\
         promote wb036scratch_out2 wb036scratch_h2;\n",
        "real (fixed) Walnut prints nothing at all for a successful promote -- only the \
         two echoed command lines and morphism's own domain/range line"
    );
    assert_eq!(err.text(), "");

    let written =
        fs::read_to_string(dir.join("Word Automata Library/wb036scratch_out2.txt")).unwrap();
    assert_eq!(
        written, "msd_2\n\n0 0\n0 -> 0\n1 -> 0\n",
        "byte-for-byte match with the real jar's own written file, captured in \
         ../CAPTURE.md"
    );

    fs::remove_dir_all(&dir).ok();
}

/// The ordering control: a morphism that is BOTH `msd_1`-shaped (maxImageLength 1) AND
/// WB-036-shaped (an image references a value outside the domain) must still report the
/// `NumberSystem` error first, on both the pre-fix and fixed jar -- Java's own fix
/// commit message calls this priority out explicitly, and this port's statement order
/// (`NumberSystemNotDefined` checked before `DomainDoesNotCoverImageRange`) already
/// matched it before this unit.
#[test]
fn wb036_number_system_check_still_beats_the_domain_gap() {
    let (mut p, console, err, dir) = prover("wb036-ordering");
    let mut input = io::Cursor::new(
        b"morphism wb036scratch_dualmor \"0->5 1->0\";\n\
          promote wb036scratch_out3 wb036scratch_dualmor;\n"
            .to_vec(),
    );

    p.read_buffer(&mut input, false);

    assert_eq!(
        console.text(),
        "morphism wb036scratch_dualmor \"0->5 1->0\";\n\
         Defined with domain [0, 1] and range {0, 5}\
         promote wb036scratch_out3 wb036scratch_dualmor;\n\
         Number system msd_1 is not defined.\n",
    );
    assert_eq!(err.text(), "");
    assert!(!dir
        .join("Word Automata Library/wb036scratch_out3.txt")
        .exists());

    fs::remove_dir_all(&dir).ok();
}
