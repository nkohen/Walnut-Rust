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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        // `System.out.println(usageMessage); System.exit(0);` (`Prover.java:301-302`).
        Ok(ArgsOutcome::Help) => {
            print!("{USAGE_MESSAGE}");
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
