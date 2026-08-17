// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `walnut-rs` binary — the port of `Main/Prover.main(String[])` (`Prover.java:288-291`):
//! parse the arguments, then hand off to [`wr_cli::prover::Prover::run`], which executes an
//! optional command file and then starts the console REPL.
//!
//! Java's `main` is three lines because everything else is `static`; here the two steps
//! own explicit state (a [`wr_cli::session::Session`], a `Logging`), so this file also
//! does the "print the error and set an exit code" job the JVM did by letting an unchecked
//! exception escape.
//!
//! Only the commands U21 wired up (`eval`/`def`/`reg`/`alphabet`/`load`/`exit`/`quit`/
//! `cls`/`clear`) actually run; every other command name dispatches to a real arm that
//! reports which unit owns it. See `wr_cli::prover`'s module docs.

use std::process::ExitCode;

use wr_cli::prover::{parse_args, ArgsOutcome, Prover, USAGE_MESSAGE};

/// The global allocator (U33, the follow-up to U32's measurement).
///
/// `benches/STATUS.md` measured **51.5% of the port's CPU time inside the macOS system
/// allocator** on a representative decision-procedure workload: `wr_core::fa::Fa` stores
/// transitions as `Vec<BTreeMap<i32, Vec<usize>>>`, a faithful transliteration of Java's
/// `List<Int2ObjectRBTreeMap<IntList>>` (`CLAUDE.md`'s mechanical-port rule), so every
/// product/determinize/minimize step allocates and frees a very large number of small,
/// short-lived objects — the pattern a JVM nursery is best at and a general-purpose `malloc`
/// is worst at.
///
/// This changes **no algorithm and no data structure**, only the `malloc`/`free` underneath
/// them, so it cannot change any automaton's language. It is declared here rather than in
/// `lib.rs` because `#[global_allocator]` is a whole-program, link-time choice: the `wr_cli`
/// *library* deliberately does not make it, leaving an embedder (e.g. `ct-research`) free to
/// pick its own.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        // `System.out.println(usageMessage); System.exit(0);` (`Prover.java:301-302`).
        // `println`, not `print`: `usageMessage` already ends in a newline, so Java's
        // output ends in TWO. Byte-compared against the real jar.
        Ok(ArgsOutcome::Help) => {
            println!("{USAGE_MESSAGE}");
            ExitCode::SUCCESS
        }
        Ok(ArgsOutcome::Run { filename, session }) => {
            let mut prover = Prover::new(session);
            match prover.run(filename.as_deref()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        // `UtilityMethods.validateFile`'s `IllegalArgumentException` escapes `main` in
        // Java (see WB-026); here it is a message plus a non-zero exit status.
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
