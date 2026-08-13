// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Automaton.writeAutomata` (`Automata/Automaton.java:59-70`) — the small glue that
//! writes a result automaton to both the `Result/` directory (a `.gv` and a `.txt`) and
//! copies the `.txt` into a library directory (`Automata Library/` or
//! `Word Automata Library/`, depending on the caller).
//!
//! Added in Phase 3a's U16 as the shared write path `Reg::reg`/`alphabet::alphabet_command`
//! both need — Java's `Reg.java:35` and `Alphabet.java:29` each call
//! `<automaton>.writeAutomata(...)` directly on the real `Automaton` class; this crate's
//! [`wr_core::automaton::Automaton`] carries no I/O, so the equivalent glue lives here
//! instead, built on [`wr_io::writer`] (U12).
//!
//! # Deliberate idiom deviation from Java: I/O errors propagate, they are not swallowed
//!
//! Java's `writeAutomata` wraps only the final `Files.copy` in a `try`/`catch
//! (IOException)` that just logs (`Logging.printTruncatedStackTrace`) and continues —
//! `AutomatonWriter.writeToGV`/`writeToTxtFormat` themselves ALSO originally swallowed
//! their own `IOException`s the same way, but `wr_io::writer`'s U12 port already changed
//! that idiom to propagate a `std::io::Result` instead (see that module's docs: "this
//! crate follows `wr-io`'s established Result-propagating idiom instead"). This function
//! follows the same already-established convention for the outer copy step too, for
//! consistency, rather than reintroducing a swallow-and-log wrapper around only that one
//! call: a genuine disk/permission failure here becomes a propagated `Err` (surfaced by
//! `reg`/`alphabet_command`'s own `Result`) instead of a silently-incomplete `TestCase`
//! whose write quietly didn't happen. This changes behavior only on a real I/O failure,
//! never in the success path Tier 1/3 exercise.
use std::io;

use wr_core::automaton::Automaton;
use wr_core::numsys::TXT_EXTENSION;
use wr_io::writer::{write_automaton_gv, write_automaton_txt};

use crate::session::Session;

/// `Automaton.GV_EXTENSION` (also referenced directly as `Prover.GV_EXTENSION` at
/// `EvalDef.java:74`, the same string constant) — `pub(crate)` since U15's `eval_def`
/// needs it too, to build `TestCase`'s `gv_address` field without a second,
/// independently-drifting copy of `".gv"`.
pub(crate) const GV_EXTENSION: &str = ".gv";

/// `Automaton.writeAutomata(String predicate, String outLibrary, String name, boolean
/// isDFAO)`. `is_dfao_for_gv` is Java's `isDFAO` parameter — note both real callers
/// (`Reg.reg`, `Alphabet.alphabetCommand`) always pass `false` here regardless of the
/// command's own `isDFAO` flag (ported verbatim; see `crate::alphabet`'s module docs for
/// the resulting quirk on a genuine DFAO result).
pub(crate) fn write_automata(
    session: &Session,
    automaton: &mut Automaton,
    predicate: &str,
    out_library_dir: &str,
    name: &str,
    is_dfao_for_gv: bool,
) -> io::Result<()> {
    let result_dir = session.paths().address_for_result();

    let gv_path = format!("{result_dir}{name}{GV_EXTENSION}");
    write_automaton_gv(automaton, &gv_path, predicate, is_dfao_for_gv)?;

    // `String firstAddress = Session.getAddressForResult() + name + TXT_EXTENSION;`
    let txt_path = format!("{result_dir}{name}{TXT_EXTENSION}");
    write_automaton_txt(automaton, &txt_path)?;

    // `Files.copy(Paths.get(firstAddress), Paths.get(outLibrary + name + TXT_EXTENSION),
    // StandardCopyOption.REPLACE_EXISTING);` -- `std::fs::copy` likewise overwrites an
    // existing destination.
    let out_path = format!("{out_library_dir}{name}{TXT_EXTENSION}");
    std::fs::copy(&txt_path, &out_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wr_core::numsys::less_than_msd;

    fn temp_session(tag: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-automaton-output-{tag}-{}-{}",
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
        // `SessionPaths` does not append a trailing slash itself (Java's
        // `Prover.parseArgs` does that); supply one here, matching `session.rs`'s own
        // test convention (`walnut_tree`'s doc comment).
        let dir_str = format!("{}/", dir.to_str().unwrap());
        let session = Session::new(Some(&dir_str), Some(&dir_str), false);
        (session, dir)
    }

    fn small_automaton() -> Automaton {
        // `less_than_msd` (already-shipped, small two-track automaton) stands in for any
        // real result automaton -- this test only needs SOME non-trivial automaton.
        less_than_msd(2)
    }

    #[test]
    fn write_automata_writes_gv_txt_and_copies_into_the_library() {
        let (session, dir) = temp_session("basic");
        let mut a = small_automaton();
        let out_library = session.paths().write_address_for_automata_library();

        write_automata(&session, &mut a, "x<y", &out_library, "myresult", false).unwrap();

        let gv = fs::read_to_string(dir.join("Result").join("myresult.gv")).unwrap();
        assert!(
            gv.contains("x<y"),
            "predicate text must land in the .gv label"
        );

        let result_txt = fs::read_to_string(dir.join("Result").join("myresult.txt")).unwrap();
        let library_txt =
            fs::read_to_string(dir.join("Automata Library").join("myresult.txt")).unwrap();
        assert_eq!(
            result_txt, library_txt,
            "the library copy must be byte-identical to the Result/ copy"
        );
        assert!(result_txt.contains("msd_2"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_automata_propagates_a_write_failure_instead_of_swallowing_it() {
        let (session, dir) = temp_session("failure");
        let mut a = small_automaton();
        // A directory that does not exist -- `write_automaton_gv` must fail to create the
        // file there, and that failure must come back as an `Err`, not be swallowed.
        let missing_out_library = format!("{}/does-not-exist/", dir.to_str().unwrap());

        let result = write_automata(&session, &mut a, "x<y", &missing_out_library, "r", false);
        assert!(result.is_err());

        fs::remove_dir_all(&dir).ok();
    }
}
