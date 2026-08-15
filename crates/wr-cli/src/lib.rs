// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `wr-cli` — ports Walnut's `Main/Prover` command dispatch + `Session`.
//!
//! Refactor Walnut's global `static` state into an explicit `Session` context
//! (the one sanctioned deviation from mechanical fidelity). Port the ~20
//! regex-dispatched commands of `Prover.java` LAST, after `wr-core`/`wr-logic`
//! are differentially green, so divergences have a small suspect surface
//! (adversarial finding F9, DESIGN.md §6).
//!
//! A library crate (not just the `walnut-rs` binary) so `test_case` and later
//! modules are importable by integration tests and other crates in the
//! workspace (e.g. a future differential/golden-corpus harness), the same
//! shape `wr-core`/`wr-io`/`wr-logic` already use.

pub mod alphabet;
pub mod automaton_ops;
pub mod automaton_output;
pub mod convert;
pub mod describe;
pub mod eval_def;
pub mod help_messages;
pub mod image;
pub mod join;
pub mod macro_cmd;
pub mod meta_commands;
pub mod morphism;
pub mod prover;
pub mod prover_helper;
pub mod quotient;
pub mod reg;
pub mod reverse;
pub mod session;
pub mod simple_transforms;
pub mod test_case;
pub mod test_command;
pub mod transduce;
pub mod walnut_exception;
