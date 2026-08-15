// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Prover.java`'s inline `macroCommand` (`:604-613`) — U23, batch A. The
//! *production* side of `macro`: writing a macro's raw definition text to
//! `Macro Library/<name>.txt`. The *consumption* side — expanding `#name(args)` during
//! parsing — was already ported (`wr_logic::predicate::Predicate::put_macro`, which reads
//! back through [`wr_logic::predicate_env::PredicateEnv::macro_text`]); this module writes
//! into exactly the file that reader already reads
//! (`crate::session::SessionPaths::read_file_for_macro_library`,
//! `crate::session::FileLibraries::macro_text`), so no new storage is introduced —
//! `macroCommand`'s write and `macro_text`'s read already agree on one file per macro
//! name under `Macro Library/`.
//!
//! # A genuine Java quirk, ported verbatim: a write failure is swallowed, not thrown
//!
//! Unlike every other command in this batch (`crate::automaton_ops`/`crate::reverse`/
//! `crate::quotient`/`crate::simple_transforms`, all of which propagate a write failure
//! as a real `Err` — see `crate::automaton_output::write_automata`'s docs for why THAT
//! specific, pre-existing idiom deviation was made), `macroCommand`'s own `try`/`catch
//! (IOException)` (`:607-611`) is Java's own, unrelated code, not a call through
//! `AutomatonWriter`/`wr_io::writer` at all — it never throws. On a write failure it
//! prints `"Could not write the macro " + name` straight to `System.out` and returns
//! `null` regardless, exactly as on success. `CLAUDE.md`'s mechanical-port rule applies
//! here with no prior, already-reviewed reason to diverge, so this function is `()`-
//! returning (Java always returns `null`, i.e. no [`crate::test_case::TestCase`] at
//! all) and never propagates an `Err` for the write itself.

use std::io::Write;

use wr_core::numsys::TXT_EXTENSION;

use crate::session::Session;

/// `Prover.macroCommand(String s)` (`Prover.java:604-613`). `stdout` is the same
/// injectable-writer seam `crate::eval_def::eval_def_command_with_stdout` already
/// established for a raw, `Logging`-independent `System.out.println` (see this module's
/// docs and that module's own for why this print is not routed through `Logging`).
pub fn macro_command(session: &Session, name: &str, definition: &str, stdout: &mut dyn Write) {
    // `new File(Session.getWriteAddressForMacroLibrary() + name + TXT_EXTENSION)`
    // (`:606`).
    let path = format!(
        "{}{name}{TXT_EXTENSION}",
        session.paths().write_address_for_macro_library()
    );

    // `BufferedWriter.write(definition)` writes the raw text with no trailing
    // terminator appended; `std::fs::write` creates/truncates and writes the given
    // bytes verbatim, matching that shape exactly (and closes the file on success just
    // as the Java try-with-resources does on its way out of the block).
    if std::fs::write(&path, definition).is_err() {
        // `System.out.println("Could not write the macro " + name);` (`:610`) -- caught
        // and printed, NOT propagated. `writeln!` to an injected sink, not real stdout
        // directly, mirroring `crate::eval_def`'s identical seam.
        let _ = writeln!(stdout, "Could not write the macro {name}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wr_logic::predicate_env::PredicateEnv;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-macro-cmd-{tag}-{}-{}",
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

    #[test]
    fn macro_command_writes_the_raw_definition_verbatim_with_no_added_terminator() {
        let (session, dir) = temp_session("basic");
        let mut stdout = Vec::new();
        macro_command(&session, "m", "Ex x=%0", &mut stdout);

        let written = fs::read_to_string(dir.join("Macro Library").join("m.txt")).unwrap();
        assert_eq!(written, "Ex x=%0", "no trailing newline appended");
        assert!(stdout.is_empty(), "no message printed on success");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn macro_command_written_file_round_trips_through_the_consumption_side() {
        let (session, dir) = temp_session("round-trip");
        let mut stdout = Vec::new();
        macro_command(&session, "m", "%0=1", &mut stdout);

        // `macroCommand`'s write and `macro_text`'s read must agree on the same file --
        // this is the "no separate storage introduced" claim in this module's docs.
        assert_eq!(
            session.libraries().macro_text("m").unwrap(),
            "%0=1",
            "the consumption side must read back exactly what macro_command wrote"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn macro_command_prints_a_message_and_does_not_panic_on_a_write_failure() {
        let (session, dir) = temp_session("write-failure");
        // Remove the macro library directory so the write fails.
        fs::remove_dir_all(dir.join("Macro Library")).unwrap();

        let mut stdout = Vec::new();
        macro_command(&session, "m", "Ex x=%0", &mut stdout);

        let printed = String::from_utf8(stdout).unwrap();
        assert_eq!(printed, "Could not write the macro m\n");
        fs::remove_dir_all(&dir).ok();
    }
}
