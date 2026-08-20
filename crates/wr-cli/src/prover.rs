// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Prover.java` (814 LOC) — the command **dispatch core**: the regex table, the
//! `[...]`-metacommand strip + `;`/`:`/`::` suffix handling (`parseSetup`), the
//! command-name lookup (`dispatch`), the 35-arm switch (`processCommand`), and the shared
//! file/REPL reader loop (`readBuffer`/`run`).
//!
//! # What this unit ports and what it deliberately leaves as stubs
//!
//! U21's job is the *plumbing*, so that U22–U26 can fill in command bodies without
//! touching dispatch. Fully wired here (calling code that already exists):
//!
//! | command | goes to |
//! |---|---|
//! | `eval`, `def` | [`crate::eval_def::eval_def_command_with_stdout`] (U15) |
//! | `reg` | [`crate::reg::reg`] (U16) |
//! | `alphabet` | [`crate::alphabet::alphabet_command`] (U16) |
//! | `load` | [`Prover::load_command`], which re-enters [`Prover::read_buffer`] |
//! | `exit`, `quit` | no body — `dispatch` returns `false` |
//! | `cls`, `clear` | [`crate::prover_helper::clear_screen_to`] |
//!
//! Every OTHER command name in Java's `RE_FOR_THE_LIST_OF_CMDS` has a real arm here that
//! **matches its argument regex first** (so a malformed invocation still produces Walnut's
//! own `Invalid use of the X command.`) and then returns
//! [`ProverError::NotYetImplemented`] naming the unit that owns it. Each arm cites its
//! `Prover.java` line so its owning unit can find its slot.
//! [`ProverError::UnsupportedCommand`] currently has no producer: `ost` was its only one,
//! and Ostrowski numeration is now ported ([`crate::ost`] + [`wr_core::ostrowski`]). The
//! variant is kept because `split`/`rsplit` (still DROP scope, `docs/BOUNDARY-MAP.md`
//! §4.1) have no arm at all yet and would use it.
//!
//! # Java statics → fields
//!
//! `PORTING.md`'s standing ruling, already applied by `crate::session` and
//! `wr_core::logging`. [`Prover`] owns the [`Session`], the [`Logging`], the
//! [`MetaCommands`], `printFlag`/`printDetails`, `currentEvalName`, and the process stdout
//! sink (injectable, so the whole REPL is testable without capturing real streams).
//! `Prover.mainProver` — the singleton — simply does not exist; callers construct a
//! `Prover`.
//!
//! # The parsed `MetaCommands` reaches determinization through `eval`/`def`
//!
//! `[strategy …]`/`[export …]` are parsed here and
//! [`crate::meta_commands::MetaCommands`] implements
//! [`wr_core::determinize::DeterminizeContext`]; [`Prover::eval_def_commands`] hands that
//! context down the `eval` call chain
//! (`eval_def_command_with_stdout_and_ctx` → `wr_logic::eval::compute_with_ctx` →
//! `Token::act_with_ctx` → `wr_core::quantify`/`logicalops`/`word_automaton` →
//! `wr_core::determinize::determinize`), so `[strategy 6 BRZ]` really does make the
//! seventh determinization of that command use Brzozowski, and `[export 1 BA]` really
//! does write `<name>_1_pre.ba`.
//!
//! `determinize.rs`'s standing requirement — pass `None` whenever
//! `should_print_details()` is false, or the automaton indices shift — is honoured at
//! [`Prover::eval_def_commands`]'s own call site (see the comment there for how the
//! `printEnabled`/`printDetails` halves of Java's flag each map).
//!
//! **Deliberately still out of scope:** every command OTHER than `eval`/`def`. Java's
//! `metaCommands` is a process-wide singleton, so a `[strategy 0 BRZ]…rightquo x y z::`
//! would take effect there too; this port wires only the `eval`/`def` path, which is what
//! Walnut's own corpus exercises (fixtures 637-641, 659, 660 — all `eval`). Widening it is
//! a mechanical follow-on, arm by arm, not a redesign. The boundary is pinned by
//! [`tests::export_metacommands_on_a_non_eval_command_are_still_accepted_and_discarded`],
//! which carries the live-verified Java behavior it diverges from and says, in its own
//! failure message, to invert itself when an arm is wired.
//!
//! Within the wired path the context reaches BOTH halves of a command — `Predicate`'s lexer
//! (a nondeterministic `$name(…)`/`T[i]` library file is determinized on load, and Java
//! counts that as index `#0`) and the postorder execution. See
//! [`wr_logic::eval::evaluate_with_logging_and_ctx`].
//!
//! # `FreshIdentifiers` is per-evaluation, not per-session
//!
//! `PORTING.md`'s `Token.getUniqueString()` entry: one [`FreshIdentifiers`] per
//! evaluation. The `eval`/`def` arm therefore builds a fresh one per command, which is
//! what every existing call site in this workspace already does.
//!
//! # Java regex dialect
//!
//! Patterns are ported verbatim except for the three ASCII/Unicode class fixes
//! `PORTING.md`'s "Phase 3 rulings" already settled for `wr-logic`'s lexer: every `\w`,
//! `\s` and `\d` is written `(?-u:\w)`/`(?-u:\s)`/`(?-u:\d)`, because Java's `Pattern` is
//! ASCII-only for these classes unless `UNICODE_CHARACTER_CLASS` is set (it is not). Two
//! further dialect notes:
//!
//! * `]` must be escaped in Rust even where Java allows it bare (`[^]]` → `[^\]]`, and a
//!   literal `]` outside a class → `\]`). Same language, different spelling.
//! * **`\>` is not a literal `>` in Rust — it is an end-of-word boundary** (`\<`/`\>`,
//!   added to the `regex` crate in 1.10). Java has no such escape, so `\-\>` in
//!   `RE_FOR_morphism_CMD` (`:120`) means the two literals `->`. The trap is that
//!   `\-\>` **compiles clean** here and then never matches; it is written `\->` in this
//!   port and pinned by
//!   [`tests::an_escaped_gt_is_a_word_boundary_in_rust_not_a_literal`]. (Escaping `-`
//!   outside a class *is* a plain literal in both dialects, so `\-` stays.)
//! * Java's `$` also matches *before* a final line terminator; Rust's matches only at the
//!   very end. The only `$`-anchored patterns here (`PAT_META_CMD`, `PAT_FOR_ost_CMD`) are
//!   fed strings that `readBuffer` has already stripped, so no trailing terminator can
//!   survive to expose the difference.
//!
//! Group NUMBERS are preserved exactly as Java's `static int` constants declare them
//! (`R_REGEXP = 20`, `GROUP_alphabet_OLD_NAME = 21`, …) — capture groups number by opening
//! parenthesis in both engines — and [`tests`] pins each one against a real command string
//! rather than trusting the count.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::OnceLock;

use regex_automata::meta::Regex;
use regex_automata::util::captures::Captures;
use regex_automata::Input;

use wr_core::determinize::DeterminizeContext;
use wr_core::logging::{LoggableError, Logging, GLOBAL_LOG_FILENAME};
use wr_core::logicalops::ConvertNsError;
use wr_core::morphism::MorphismError;
use wr_core::util::validate_file;
use wr_core::walnut_panic::{catch_walnut_panic_detailed, CaughtPanic};
use wr_logic::predicate_env::FreshIdentifiers;

use crate::alphabet::{alphabet_command, AlphabetError};
use crate::automaton_ops::{
    combine_command, concat_command, intersect_command, star_command, union_command,
    AutomatonOpsError,
};
use crate::convert::{convert_command, ConvertError};
use crate::describe::{describe, DescribeError};
use crate::eval_def::{eval_def_command_with_stdout_and_ctx, EvalDefError};
use crate::image::{image, ImageError};
use crate::join::{join_command, JoinError};
use crate::macro_cmd::macro_command;
use crate::meta_commands::{MetaCommandError, MetaCommands};
use crate::morphism::{morphism_command_to, promote_command, MorphismCommandError};
use crate::ost::{ost_command, OstError};
use crate::prover_helper::{
    clear_screen_to, determine_in_library, export_automata_to, inf_from_address_to,
    ProverHelperError,
};
use crate::quotient::{left_quotient_command, right_quotient_command, QuotientError};
use crate::reg::{reg, RegError};
use crate::reverse::{reverse_command, ReverseError};
use crate::session::{Session, SessionPaths, PROMPT, WALNUT_VERSION};
use crate::simple_transforms::{
    fix_lead_zero_command, fix_trail_zero_command, minimize_command, SimpleTransformError,
};
use crate::test_case::TestCase;
use crate::test_command::{test_command_to, TestError};
use crate::transduce::TransduceCommandError;
use crate::walnut_exception as msg;

// ---------------------------------------------------------------------------
// Command names and small constants (`Prover.java:36-244`)
// ---------------------------------------------------------------------------

/// `Prover.RE_FOR_THE_LIST_OF_CMDS` (`:36`) — the 35 command names, verbatim and in
/// Walnut's own order.
pub const RE_FOR_THE_LIST_OF_CMDS: &str = "(eval|def|macro|reg|load|ost|exit|quit|cls|clear|combine|morphism|promote|image|inf|split|rsplit|join|test|transduce|reverse|minimize|convert|fixleadzero|fixtrailzero|alphabet|union|intersect|star|concat|rightquo|leftquo|describe|export|help)";

/// `Prover.RE_START` (`:37`).
const RE_START: &str = "^";

/// `Prover.RE_IDENTIFIER` (`:39`), with `(?-u:\w)` for Java's ASCII-only `\w`.
pub const RE_IDENTIFIER: &str = r"[a-zA-Z](?-u:\w)*";

/// `Prover.RE_WORD_OF_CMD_NO_SPC` (`:40`).
pub const RE_WORD_OF_CMD_NO_SPC: &str = r"([a-zA-Z](?-u:\w)*)";

/// `Prover.RE_WORD_OF_CMD` (`:41`).
const RE_WORD_OF_CMD: &str = r"(?-u:\s)+([a-zA-Z](?-u:\w)*)";

/// `Prover.RE_EQ_INT_OPTIONAL` (`:43`).
pub const RE_EQ_INT_OPTIONAL: &str = r"(=-?(?-u:\d)+)?";

/// `Prover.DOLLAR` (`:162`) — the optional `$` marker distinguishing a predicate automaton
/// from a word automaton (DFAO) argument.
const DOLLAR: &str = r"(?-u:\s)+(\$|(?-u:\s)*)";

pub const FIXTRAILZERO: &str = "fixtrailzero";
pub const CONVERT: &str = "convert";
/// `Prover.REVERSE_SPLIT` (`:53`) — used only as the *error-message* name for `rsplit`.
pub const REVERSE_SPLIT: &str = "reverse split";
pub const REG: &str = "reg";
pub const LOAD: &str = "load";
pub const ALPHABET: &str = "alphabet";
pub const HELP: &str = "help";
pub const CLEAR: &str = "clear";
pub const CLS: &str = "cls";
pub const DEF: &str = "def";
pub const EVAL: &str = "eval";
pub const EXIT: &str = "exit";
pub const QUIT: &str = "quit";
pub const LEFT_BRACKET: &str = "[";
pub const DOT: &str = ".";
pub const TXT_STRING: &str = "txt";
/// `Prover.TXT_EXTENSION` (`:67`) — aliased to `wr-core`'s existing constant rather than
/// spelled a second time (one literal, not two that can drift).
pub const TXT_EXTENSION: &str = wr_core::numsys::TXT_EXTENSION;
pub const GV_STRING: &str = "gv";
/// `Prover.GV_EXTENSION` (`:69`) — aliased to `crate::automaton_output`'s existing copy.
pub const GV_EXTENSION: &str = crate::automaton_output::GV_EXTENSION;
pub const BA_STRING: &str = "ba";
pub const BA_EXTENSION: &str = ".ba";
pub const FIRST_OP: &str = "first";
pub const IF_OTHER_OP: &str = "if_other";

pub const MACRO: &str = "macro";
pub const OST: &str = "ost";
pub const COMBINE: &str = "combine";
pub const MORPHISM: &str = "morphism";
pub const PROMOTE: &str = "promote";
pub const IMAGE: &str = "image";
pub const INF: &str = "inf";
pub const SPLIT: &str = "split";
pub const RSPLIT: &str = "rsplit";
pub const JOIN: &str = "join";
pub const TEST: &str = "test";
pub const TRANSDUCE: &str = "transduce";
pub const REVERSE: &str = "reverse";
pub const MINIMIZE: &str = "minimize";
pub const FIXLEADZERO: &str = "fixleadzero";
pub const UNION: &str = "union";
pub const INTERSECT: &str = "intersect";
pub const STAR: &str = "star";
pub const CONCAT: &str = "concat";
pub const RIGHTQUO: &str = "rightquo";
pub const LEFTQUO: &str = "leftquo";
pub const EXPORT: &str = "export";
pub const DESCRIBE: &str = "describe";

// -- capture-group indices, verbatim from Java's `static int` constants ------

/// `Prover.L_FILENAME` (`:78`).
pub const L_FILENAME: usize = 1;
/// `Prover.ED_NAME`/`ED_FREE_VARIABLES`/`ED_PREDICATE` (`:88`).
pub const ED_NAME: usize = 2;
pub const ED_FREE_VARIABLES: usize = 3;
pub const ED_PREDICATE: usize = 4;
/// `Prover.M_NAME`/`M_DEFINITION` (`:93`).
pub const M_NAME: usize = 1;
pub const M_DEFINITION: usize = 2;
/// `Prover.R_NAME`/`R_LIST_OF_ALPHABETS`/`R_REGEXP` (`:101`). `R_LIST_OF_ALPHABETS` is
/// reused verbatim by the `alphabet` arm (`:767`) — the two patterns share that group's
/// position.
pub const R_NAME: usize = 2;
pub const R_LIST_OF_ALPHABETS: usize = 3;
pub const R_REGEXP: usize = 20;
/// `Prover.R_NUMBER_SYSTEM`/`R_SET` (`:106`) — consumed by
/// `Alphabet.determineAlphabetsAndNS`, i.e. `crate::alphabet`, not by dispatch.
pub const R_NUMBER_SYSTEM: usize = 2;
pub const R_SET: usize = 11;
/// `Prover.GROUP_OST_*` (`:111`).
pub const GROUP_OST_NAME: usize = 1;
pub const GROUP_OST_PREPERIOD: usize = 2;
pub const GROUP_OST_PERIOD: usize = 4;
/// `Prover.GROUP_COMBINE_*` (`:117`).
pub const GROUP_COMBINE_NAME: usize = 1;
pub const GROUP_COMBINE_AUTOMATA: usize = 2;
/// `Prover.GROUP_MORPHISM_NAME` (`:122`).
///
/// **Quirk, ported verbatim:** Java declares `static int GROUP_MORPHISM_NAME = 1,
/// GROUP_MORPHISM_DEFINITION;` — the second field is never assigned, so it keeps `int`'s
/// default `0`, and `morphismCommand` passes `m.group(0)` (the WHOLE match) as the
/// morphism definition. That is not a typo in this port.
pub const GROUP_MORPHISM_NAME: usize = 1;
pub const GROUP_MORPHISM_DEFINITION: usize = 0;
/// `Prover.GROUP_PROMOTE_*` (`:127`).
pub const GROUP_PROMOTE_NAME: usize = 1;
pub const GROUP_PROMOTE_MORPHISM: usize = 2;
/// `Prover.GROUP_IMAGE_*` (`:132`).
pub const GROUP_IMAGE_NEW_NAME: usize = 1;
pub const GROUP_IMAGE_MORPHISM: usize = 2;
pub const GROUP_IMAGE_OLD_NAME: usize = 3;
/// `Prover.GROUP_INF_NAME` (`:137`).
pub const GROUP_INF_NAME: usize = 1;
/// `Prover.GROUP_SPLIT_*` (`:142`).
pub const GROUP_SPLIT_NAME: usize = 1;
pub const GROUP_SPLIT_AUTOMATA: usize = 2;
pub const GROUP_SPLIT_INPUT: usize = 3;
/// `Prover.GROUP_RSPLIT_*` (`:149`) — note the unusual order.
pub const GROUP_RSPLIT_NAME: usize = 1;
pub const GROUP_RSPLIT_AUTOMATA: usize = 4;
pub const GROUP_RSPLIT_INPUT: usize = 2;
/// `Prover.GROUP_JOIN_*` (`:154`).
pub const GROUP_JOIN_NAME: usize = 1;
pub const GROUP_JOIN_AUTOMATA: usize = 2;
/// `Prover.GROUP_TEST_*` (`:159`).
pub const GROUP_TEST_NAME: usize = 1;
pub const GROUP_TEST_NUM: usize = 2;
/// `Prover.GROUP_TRANSDUCE_*` (`:165-166`).
pub const GROUP_TRANSDUCE_NEW_NAME: usize = 1;
pub const GROUP_TRANSDUCE_TRANSDUCER: usize = 2;
pub const GROUP_TRANSDUCE_DOLLAR_SIGN: usize = 3;
pub const GROUP_TRANSDUCE_OLD_NAME: usize = 4;
/// `Prover.GROUP_REVERSE_*` (`:171`).
pub const GROUP_REVERSE_NEW_NAME: usize = 1;
pub const GROUP_REVERSE_DOLLAR_SIGN: usize = 2;
pub const GROUP_REVERSE_OLD_NAME: usize = 3;
/// `Prover.GROUP_MINIMIZE_*` (`:176`).
pub const GROUP_MINIMIZE_NEW_NAME: usize = 1;
pub const GROUP_MINIMIZE_OLD_NAME: usize = 2;
/// `Prover.GROUP_CONVERT_*` (`:180-183`).
pub const GROUP_CONVERT_NEW_NAME: usize = 2;
pub const GROUP_CONVERT_OLD_NAME: usize = 7;
pub const GROUP_CONVERT_NEW_DOLLAR_SIGN: usize = 1;
pub const GROUP_CONVERT_OLD_DOLLAR_SIGN: usize = 6;
pub const GROUP_CONVERT_MSD_OR_LSD: usize = 4;
pub const GROUP_CONVERT_BASE: usize = 5;
/// `Prover.GROUP_FIXLEADZERO_*`/`GROUP_FIXTRAILZERO_*` (`:188`, `:192`).
pub const GROUP_FIXLEADZERO_NEW_NAME: usize = 1;
pub const GROUP_FIXLEADZERO_OLD_NAME: usize = 3;
pub const GROUP_FIXTRAILZERO_NEW_NAME: usize = 1;
pub const GROUP_FIXTRAILZERO_OLD_NAME: usize = 3;
/// `Prover.GROUP_alphabet_*` (`:197`).
pub const GROUP_ALPHABET_NEW_NAME: usize = 2;
pub const GROUP_ALPHABET_DOLLAR_SIGN: usize = 20;
pub const GROUP_ALPHABET_OLD_NAME: usize = 21;
/// `Prover.GROUP_UNION_*`/`GROUP_INTERSECT_*`/`GROUP_CONCAT_*` (`:202`, `:207`, `:217`).
pub const GROUP_UNION_NAME: usize = 1;
pub const GROUP_UNION_AUTOMATA: usize = 2;
pub const GROUP_INTERSECT_NAME: usize = 1;
pub const GROUP_INTERSECT_AUTOMATA: usize = 2;
pub const GROUP_CONCAT_NAME: usize = 1;
pub const GROUP_CONCAT_AUTOMATA: usize = 2;
/// `Prover.GROUP_STAR_*` (`:212`).
pub const GROUP_STAR_NEW_NAME: usize = 1;
pub const GROUP_STAR_OLD_NAME: usize = 2;
/// `Prover.GROUP_quo_*` (`:222`), shared by `rightquo` and `leftquo`.
pub const GROUP_QUO_NEW_NAME: usize = 1;
pub const GROUP_QUO_OLD_NAME1: usize = 2;
pub const GROUP_QUO_OLD_NAME2: usize = 3;
/// `Prover.GROUP_META_CMD`/`GROUP_FINAL_CMD` (`:230`).
pub const GROUP_META_CMD: usize = 1;
pub const GROUP_FINAL_CMD: usize = 2;
/// `Prover.GROUP_export_*` (`:239`).
pub const GROUP_EXPORT_DOLLAR_SIGN: usize = 1;
pub const GROUP_EXPORT_NAME: usize = 2;
pub const GROUP_EXPORT_TYPE: usize = 3;
/// `Prover.GROUP_describe_*` (`:244`).
pub const GROUP_DESCRIBE_DOLLAR_SIGN: usize = 1;
pub const GROUP_DESCRIBE_NAME: usize = 2;

/// `Prover.usageMessage` (`:256-270`).
pub const USAGE_MESSAGE: &str = r#"Usage: walnut [OPTIONS] [<filename>]

Walnut command-line interface.

Positional arguments:
  <filename>          File of commands to execute (same effect as the `load`
                      command). If omitted, starts an interactive session.

Options:
  --global-session    Use the old (Walnut 6 and earlier) global session behavior.
  --session-dir PATH  Use PATH instead of an auto-generated Session directory.
  --home-dir PATH     Use PATH instead of the current working directory.
  --help              Show this help message and exit.
"#;

/// `Prover.OTF_MESSAGE` (`:272-276`) — printed after any command that used an OTF
/// strategy. **Unreachable in this port** (see [`crate::meta_commands`]); ported because
/// the mechanical-port rule says dead code stays.
pub const OTF_MESSAGE: &str = r#"---------------------------
If the CCL(S) or BRZ-CCL(S) algorithms are used, please cite the paper:
Nicol, John, and Markus Frohme. "Deconstructing Subset Construction: Reducing While Determinizing." International Conference on Tools and Algorithms for the Construction and Analysis of Systems. Cham: Springer Nature Switzerland, 2026.
---------------------------"#;

/// `Prover.homeDirArg`/`sessionDirArg`/`globalSessionArg` (`:278-280`).
pub const HOME_DIR_ARG: &str = "--home-dir=";
pub const SESSION_DIR_ARG: &str = "--session-dir=";
pub const GLOBAL_SESSION_ARG: &str = "--global-session";

// ---------------------------------------------------------------------------
// The compiled pattern table
// ---------------------------------------------------------------------------

/// Every `Pattern` static in `Prover.java`. Java also holds a `Matcher` per use site;
/// a `regex_automata::meta::Regex` binds to no haystack, so there is no `Matcher` half
/// here (same shape as `wr-logic`'s lexer, whose docs argue the point at length).
pub struct Patterns {
    pub cmd: Regex,
    pub list_of_cmds: Regex,
    pub load: Regex,
    pub eval_def: Regex,
    pub macro_cmd: Regex,
    pub reg: Regex,
    pub single_element_of_a_set: Regex,
    pub ost: Regex,
    pub combine: Regex,
    pub morphism: Regex,
    pub promote: Regex,
    pub image: Regex,
    pub inf: Regex,
    pub split: Regex,
    pub input_in_split: Regex,
    pub rsplit: Regex,
    pub join: Regex,
    pub test: Regex,
    pub transduce: Regex,
    pub reverse: Regex,
    pub minimize: Regex,
    pub convert: Regex,
    pub fixleadzero: Regex,
    pub fixtrailzero: Regex,
    pub alphabet: Regex,
    pub union: Regex,
    pub intersect: Regex,
    pub star: Regex,
    pub concat: Regex,
    pub rightquo: Regex,
    pub leftquo: Regex,
    pub export: Regex,
    pub describe: Regex,
    pub meta_cmd: Regex,
}

/// The alphabet/number-system token list shared by `RE_FOR_reg_CMD` (`:96`) and
/// `RE_FOR_alphabet_CMD` (`:194-195`) — identical text in both, which is why their group
/// numbering agrees and `R_LIST_OF_ALPHABETS` is reused across them.
const RE_LIST_OF_ALPHABETS: &str = r"((((((msd|lsd)_((?-u:\d)+|(?-u:\w)+))|((msd|lsd)((?-u:\d)+|(?-u:\w)+))|(msd|lsd)|((?-u:\d)+|(?-u:\w)+))|(\{((?-u:\s)*(\+|\-)?(?-u:\s)*(?-u:\d)+)((?-u:\s)*,(?-u:\s)*(\+|\-)?(?-u:\s)*(?-u:\d)+)*(?-u:\s)*\}))(?-u:\s)+)+)";

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern)
        .unwrap_or_else(|e| panic!("Prover pattern failed to compile: {pattern}: {e}"))
}

impl Patterns {
    fn compile_all() -> Patterns {
        let id = RE_IDENTIFIER;
        let w = RE_WORD_OF_CMD;
        let wn = RE_WORD_OF_CMD_NO_SPC;
        Patterns {
            // `RE_FOR_CMD` (`:48`).
            cmd: compile(&format!(r"{RE_START}((?-u:\w)+)((?-u:\s)+.*)?")),
            // `commandName.matches(RE_FOR_THE_LIST_OF_CMDS)` (`:415`) -- `String.matches`
            // anchors both ends, hence the added `^`/`$`.
            list_of_cmds: compile(&format!("^{RE_FOR_THE_LIST_OF_CMDS}$")),
            // `RE_FOR_load_CMD` (`:79`).
            load: compile(&format!(r"{RE_START}{LOAD}(?-u:\s)+((?-u:\w)+\.txt)")),
            // `RE_FOR_eval_def_CMDS` (`:83-84`).
            eval_def: compile(&format!(
                r#"{RE_START}(eval|def)(?:(?-u:\s)+({id})((?:(?-u:\s)+{id})*))?(?-u:\s)+"(.*)""#
            )),
            // `RE_FOR_macro_CMD` (`:92`).
            macro_cmd: compile(&format!(r#"{RE_START}{MACRO}{w}(?-u:\s)+"(.*)""#)),
            // `RE_FOR_reg_CMD` (`:96`).
            reg: compile(&format!(
                r#"{RE_START}(reg){w}(?-u:\s)+{RE_LIST_OF_ALPHABETS}"(.*)""#
            )),
            // `RE_FOR_A_SINGLE_ELEMENT_OF_A_SET` (`:103`).
            single_element_of_a_set: compile(r"(\+|\-)?(?-u:\s)*(?-u:\d)+"),
            // `RE_FOR_ost_CMD` (`:109`).
            ost: compile(&format!(
                r"{RE_START}{OST}{w}(?-u:\s)*\[(?-u:\s)*(((?-u:\d)+(?-u:\s)*)*)\](?-u:\s)*\[(?-u:\s)*(((?-u:\d)+(?-u:\s)*)*)\]$"
            )),
            // `RE_FOR_combine_CMD` (`:114-115`).
            combine: compile(&format!(
                r"{RE_START}{COMBINE}{w}(((?-u:\s)+({id}{RE_EQ_INT_OPTIONAL}))*)"
            )),
            // `RE_FOR_morphism_CMD` (`:120`).
            morphism: compile(&format!(
                r#"{RE_START}{MORPHISM}{w}(?-u:\s)+"((?-u:\d)+(?-u:\s)*\->(?-u:\s)*(.)*(,(?-u:\d)+(?-u:\s)*\->(?-u:\s)*(.)*)*)""#
            )),
            // `RE_FOR_promote_CMD` (`:125`).
            promote: compile(&format!(r"{RE_START}{PROMOTE}{w}{w}")),
            // `RE_FOR_image_CMD` (`:130`).
            image: compile(&format!(r"{RE_START}{IMAGE}{w}{w}{w}")),
            // `RE_FOR_inf_CMD` (`:135`).
            inf: compile(&format!(r"{RE_START}{INF}{w}")),
            // `RE_FOR_split_CMD` (`:140`).
            split: compile(&format!(
                r"{RE_START}{SPLIT}{w}{w}(((?-u:\s)*\[(?-u:\s)*[+-]?(?-u:\s)*\])+)"
            )),
            // `RE_FOR_INPUT_IN_split_CMD` (`:143`).
            input_in_split: compile(r"\[(?-u:\s)*([+-]?)(?-u:\s)*\]"),
            // `RE_FOR_rsplit_CMD` (`:147`).
            rsplit: compile(&format!(
                r"{RE_START}{RSPLIT}{w}(((?-u:\s)*\[(?-u:\s)*[+-]?(?-u:\s)*\])+){w}"
            )),
            // `RE_FOR_join_CMD` (`:152`).
            join: compile(&format!(
                r"{RE_START}{JOIN}{w}(({w}(((?-u:\s)*\[(?-u:\s)*[a-zA-Z&&[^AE]](?-u:\w)*(?-u:\s)*\])+))*)"
            )),
            // `RE_FOR_test_CMD` (`:157`).
            test: compile(&format!(r"{RE_START}{TEST}{w}(?-u:\s)*((?-u:\d)+)")),
            // `RE_FOR_transduce_CMD` (`:163`).
            transduce: compile(&format!(r"{RE_START}{TRANSDUCE}{w}{w}{DOLLAR}{wn}")),
            // `RE_FOR_reverse_CMD` (`:169`).
            reverse: compile(&format!(r"{RE_START}{REVERSE}{w}{DOLLAR}{wn}")),
            // `RE_FOR_minimize_CMD` (`:174`).
            minimize: compile(&format!(r"{RE_START}{MINIMIZE}{w}{w}")),
            // `RE_FOR_convert_CMD` (`:178`).
            convert: compile(&format!(
                r"{RE_START}convert{DOLLAR}{wn}(?-u:\s)+((msd|lsd)_((?-u:\d)+)){DOLLAR}{wn}"
            )),
            // `RE_FOR_fixleadzero_CMD` (`:186`).
            fixleadzero: compile(&format!(r"{RE_START}{FIXLEADZERO}{w}{DOLLAR}{wn}")),
            // `RE_FOR_fixtrailzero_CMD` (`:190`).
            fixtrailzero: compile(&format!(r"{RE_START}{FIXTRAILZERO}{w}{DOLLAR}{wn}")),
            // `RE_FOR_alphabet_CMD` (`:194-195`).
            alphabet: compile(&format!(
                r"{RE_START}({ALPHABET}){w}(?-u:\s)+{RE_LIST_OF_ALPHABETS}(\$|(?-u:\s)*){wn}"
            )),
            // `RE_FOR_union_CMD` (`:200`).
            union: compile(&format!(r"{RE_START}{UNION}{w}(({w})*)")),
            // `RE_FOR_intersect_CMD` (`:205`).
            intersect: compile(&format!(r"{RE_START}{INTERSECT}{w}(({w})*)")),
            // `RE_FOR_star_CMD` (`:210`).
            star: compile(&format!(r"{RE_START}{STAR}{w}{w}")),
            // `RE_FOR_concat_CMD` (`:215`).
            concat: compile(&format!(r"{RE_START}{CONCAT}{w}(({w})*)")),
            // `RE_FOR_rightquo_CMD` (`:220`).
            rightquo: compile(&format!(r"{RE_START}{RIGHTQUO}{w}{w}{w}")),
            // `RE_FOR_leftquo_CMD` (`:225`).
            leftquo: compile(&format!(r"{RE_START}{LEFTQUO}{w}{w}{w}")),
            // `RE_FOR_export_CMD` (`:237`).
            export: compile(&format!(r"{RE_START}{EXPORT}{DOLLAR}{wn}{w}")),
            // `RE_FOR_describe_CMD` (`:242`).
            describe: compile(&format!(r"{RE_START}{DESCRIBE}{DOLLAR}{wn}")),
            // `PAT_META_CMD` (`:229`) -- Java's bare `]`s must be escaped in Rust.
            meta_cmd: compile(r"^\[([^\]]*)\](.*)$"),
        }
    }
}

/// The process-wide compiled pattern table. [`OnceLock`] rather than `LazyLock` for the
/// same reason `wr-logic`'s lexer uses one: this workspace's `rust-version` is 1.75.
pub fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(Patterns::compile_all)
}

/// `Matcher.find()` — an UNANCHORED search (every pattern here starts with `^`, so the
/// distinction only matters for the ones that do not).
pub fn find(re: &Regex, hay: &str) -> Option<Captures> {
    let mut caps = re.create_captures();
    re.search_captures(&Input::new(hay), &mut caps);
    if caps.is_match() {
        Some(caps)
    } else {
        None
    }
}

/// `Matcher.group(int)` — `None` for a group that did not participate (Java's `null`).
pub fn group<'h>(caps: &Captures, hay: &'h str, index: usize) -> Option<&'h str> {
    caps.get_group(index).map(|span| &hay[span])
}

/// `Character.isWhitespace(int)` — **not** `char::is_whitespace`, which disagrees on eight
/// code points (four each way; see [`java_strip`]).
///
/// Java's rule, verbatim from its javadoc: a character is whitespace if it is a Unicode
/// space character (`Zs`/`Zl`/`Zp`) that is *not* one of the three non-breaking spaces
/// `U+00A0`/`U+2007`/`U+202F`, **or** it is one of `U+0009`–`U+000D` (HT, LF, VT, FF, CR)
/// or `U+001C`–`U+001F` (FS, GS, RS, US).
///
/// The `Zs`/`Zl`/`Zp` set is closed and small, so it is written out rather than pulled in
/// as a Unicode-table dependency: `U+0020`, `U+1680`, `U+2000`–`U+200A`, `U+2028`,
/// `U+2029`, `U+205F`, `U+3000` (minus the two non-breaking members `U+2007`/`U+202F`, and
/// `U+00A0`).
pub fn is_java_whitespace(c: char) -> bool {
    matches!(c,
        // The eleven ASCII control characters `isWhitespace` names explicitly. Note
        // `U+001C`-`U+001F` are NOT `char::is_whitespace` in Rust.
        '\u{9}'..='\u{D}' | '\u{1C}'..='\u{1F}'
        // `Zs` minus U+00A0/U+2007/U+202F, plus `Zl` (U+2028) and `Zp` (U+2029).
        | ' ' | '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}' | '\u{205F}' | '\u{3000}')
}

/// `String.strip()` — **the correct spelling of Java's "Unicode-aware trim"**, and *not*
/// `str::trim`.
///
/// `PORTING.md`'s "Phase 3 rulings" entry on this: the two functions disagree in both
/// directions. `str::trim` strips `U+00A0`/`U+0085`/`U+2007`/`U+202F`, which
/// `Character.isWhitespace` does not; `String.strip` strips `U+001C`–`U+001F`, which
/// `str::trim` does not. Every `s.strip()` in `Prover.java` (`:366`, `:448`, `:457`) and
/// `MetaCommands.java` (`:90`, `:95`) goes through here.
pub fn java_strip(s: &str) -> &str {
    s.trim_matches(is_java_whitespace)
}

/// `ProverHelper.matchOrFail(Pattern, String, String)` (`ProverHelper.java:35-41`) — see
/// `crate::prover_helper`'s docs for why it lives here rather than there.
pub fn match_or_fail(re: &Regex, input: &str, command_name: &str) -> Result<Captures, ProverError> {
    find(re, input).ok_or_else(|| ProverError::InvalidCommandUse(command_name.to_string()))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure `Prover`'s dispatch layer can report.
#[derive(Debug)]
pub enum ProverError {
    /// `WalnutException.invalidCommand` (`:411`, `:437`, `:574`).
    InvalidCommand(String),
    /// `WalnutException.noSuchCommand` (`:416`).
    NoSuchCommand,
    /// `WalnutException.invalidCommandUse`, via `ProverHelper.matchOrFail`.
    InvalidCommandUse(String),
    /// `UtilityMethods.validateFile`'s `IllegalArgumentException`
    /// (`UtilityMethods.java:153-158`).
    InvalidFile(String),
    /// A `Main.WalnutException` whose message is already formatted by
    /// [`crate::walnut_exception`]. Currently only `Session.createSubdirectories`'s
    /// `"Couldn't create directory:" + s` (`Session.java:132`) — which is a
    /// `WalnutException`, *not* the `IllegalArgumentException` that
    /// [`ProverError::InvalidFile`] models.
    WalnutMessage(String),
    Meta(MetaCommandError),
    EvalDef(EvalDefError),
    Reg(RegError),
    Alphabet(AlphabetError),
    Helper(ProverHelperError),
    Test(TestError),
    Transduce(TransduceCommandError),
    Io(io::Error),
    /// `Integer.parseInt(m.group(GROUP_TEST_NUM))`'s `NumberFormatException`
    /// (`Prover.testCommand`, `:685`) — the `\d+`-constrained capture group can still
    /// overflow `i32`, which `PAT_FOR_test_CMD` does not guard against.
    NumberFormat(String),
    /// U23, batch A: `combine`/`concat`/`union`/`intersect`/`star`.
    AutomatonOps(AutomatonOpsError),
    /// U23, batch A: `reverse`.
    Reverse(ReverseError),
    /// U23, batch A: `rightquo`/`leftquo`.
    Quotient(QuotientError),
    /// U23, batch A: `describe`.
    Describe(DescribeError),
    /// U23, batch A: `minimize`/`fixleadzero`/`fixtrailzero`.
    SimpleTransform(SimpleTransformError),
    /// `morphism`/`promote` (`crate::morphism`).
    Morphism(MorphismCommandError),
    /// `image` (`crate::image`).
    Image(ImageError),
    /// `join` (`crate::join`).
    Join(JoinError),
    /// `convert` (`crate::convert`).
    Convert(ConvertError),
    /// `ost` (`crate::ost`).
    Ost(OstError),
    /// **Port-specific.** A command this project deliberately does not implement.
    UnsupportedCommand {
        command: &'static str,
        reason: &'static str,
    },
    /// **Port-specific in form, faithful in behavior.** A `wr-core`/`wr-io` guard that
    /// models a Java `RuntimeException` as a `panic!`/`assert!` fired somewhere under
    /// this command, and [`Prover::caught`] — the port's stand-in for
    /// `Prover.readBuffer`'s `catch (RuntimeException)` — recovered its message. The
    /// payload is that message verbatim. See [`Prover::caught`] for why the boundary sits
    /// at dispatch and what the recovered message can and cannot say.
    Thrown {
        /// The guard's message, verbatim.
        message: String,
        /// `file:line:column` of the `panic!` that raised it, from
        /// [`wr_core::walnut_panic::CaughtPanic::location`]. Never rendered to the user
        /// (Java has no such text either), but carried so a `{:?}` in a failing test — or
        /// a future `Logging` hook — can still name the site. Without it, a genuine new
        /// port bug caught by this boundary would be strictly HARDER to diagnose than it
        /// was before the boundary existed, since the boundary suppresses Rust's own
        /// `panicked at file:line` line.
        location: Option<String>,
    },
    /// **Port-specific.** A command whose dispatch arm exists but whose body belongs to a
    /// later unit.
    NotYetImplemented {
        command: &'static str,
        unit: &'static str,
    },
}

impl std::fmt::Display for ProverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProverError::InvalidCommand(c) => write!(f, "{}", msg::invalid_command(c)),
            ProverError::NoSuchCommand => write!(f, "{}", msg::no_such_command()),
            ProverError::InvalidCommandUse(c) => write!(f, "{}", msg::invalid_command_use(c)),
            ProverError::InvalidFile(m) => write!(f, "{m}"),
            ProverError::WalnutMessage(m) => write!(f, "{m}"),
            ProverError::Meta(e) => write!(f, "{e}"),
            ProverError::EvalDef(e) => write!(f, "{e}"),
            ProverError::Reg(e) => write!(f, "{e}"),
            ProverError::Alphabet(e) => write!(f, "{e}"),
            ProverError::Helper(e) => write!(f, "{e}"),
            ProverError::Test(e) => write!(f, "{e}"),
            ProverError::Transduce(e) => write!(f, "{e}"),
            ProverError::Io(e) => write!(f, "{e}"),
            ProverError::NumberFormat(input) => {
                write!(f, "{}", msg::number_format_exception(input))
            }
            ProverError::AutomatonOps(e) => write!(f, "{e}"),
            ProverError::Reverse(e) => write!(f, "{e}"),
            ProverError::Quotient(e) => write!(f, "{e}"),
            ProverError::Describe(e) => write!(f, "{e}"),
            ProverError::SimpleTransform(e) => write!(f, "{e}"),
            ProverError::Morphism(e) => write!(f, "{e}"),
            ProverError::Image(e) => write!(f, "{e}"),
            ProverError::Join(e) => write!(f, "{e}"),
            ProverError::Convert(e) => write!(f, "{e}"),
            ProverError::Ost(e) => write!(f, "{e}"),
            // The recovered guard message, verbatim — the guard-authoring rule in
            // `wr_core::walnut_panic`'s docs makes it Walnut's own text wherever the
            // guard ported one.
            ProverError::Thrown { message, .. } => write!(f, "{message}"),
            ProverError::UnsupportedCommand { command, reason } => write!(
                f,
                "The {command} command is out of scope for walnut-rs ({reason})."
            ),
            ProverError::NotYetImplemented { command, unit } => write!(
                f,
                "The {command} command is not implemented yet (planned for {unit})."
            ),
        }
    }
}

impl std::error::Error for ProverError {}

impl LoggableError for ProverError {
    /// Exhaustive on purpose, same discipline as `wr_logic::eval`'s `ActError` impl.
    fn is_handled(&self) -> bool {
        match self {
            ProverError::InvalidCommand(_)
            | ProverError::NoSuchCommand
            | ProverError::InvalidCommandUse(_)
            | ProverError::WalnutMessage(_) => true,
            // `IllegalArgumentException`, not a `WalnutException`.
            ProverError::InvalidFile(_) => false,
            ProverError::Meta(e) => e.is_handled(),
            // Wrapped command errors: every variant of these is a deliberately-thrown
            // `WalnutException` (or, for the I/O ones, this port's own Result-propagating
            // idiom — see `crate::automaton_output`'s docs), so they render message-only.
            ProverError::EvalDef(_)
            | ProverError::Reg(_)
            | ProverError::Alphabet(_)
            | ProverError::Helper(_)
            | ProverError::Test(_)
            | ProverError::Transduce(_)
            | ProverError::Io(_)
            | ProverError::Reverse(_)
            | ProverError::Describe(_)
            | ProverError::SimpleTransform(_)
            | ProverError::Image(_) => true,
            // Two exceptions to the paragraph above, both non-`WalnutException` Java
            // throwables faithfully surfaced by U23's review fixes.
            ProverError::AutomatonOps(e) => !matches!(e, AutomatonOpsError::NumberFormat(_)),
            ProverError::Quotient(e) => e.is_walnut_exception(),

            // --- the three wrapped enums that are NOT uniformly `WalnutException` -----
            //
            // Same precision `ProverError::NumberFormat` below already applies, and that
            // `is_io_class_error` applies to these same enums: a Walnut command's failure
            // renders message-only ONLY when Java would have thrown a `WalnutException`.
            // The sub-variants below are ports of genuine unchecked JDK exceptions
            // (`IndexOutOfBoundsException`/`NumberFormatException`/`NullPointerException`/
            // `IllegalArgumentException`) that Java's `readBuffer` catch reports WITH its
            // exception kind and stack-trace prefix — which is what `is_handled() == false`
            // selects. Getting this wrong is a Tier-1 normalized-text divergence.

            // WB-036: real Java's `toWordAutomaton` builds a malformed FA and then throws
            // `IndexOutOfBoundsException` on the very next write.
            ProverError::Morphism(MorphismCommandError::Promote(
                MorphismError::DomainDoesNotCoverImageRange,
            )) => false,
            // `UtilityMethods.validateFile` (`:153-159`) throws `IllegalArgumentException`,
            // exactly like `ProverError::InvalidFile` above.
            ProverError::Morphism(MorphismCommandError::InvalidFile(_)) => false,
            // Every other `promote`/`morphism` failure IS a `WalnutException`:
            // `parseMorphism`'s "Morphism has no valid mappings.",
            // `WalnutException.morphismNegative()`, `morphismNotUniform()`, and
            // `NumberSystem`'s "Number system msd_k is not defined."
            ProverError::Morphism(_) => true,

            // WB-037: `subautomata.remove(0)` on an empty list is
            // `IndexOutOfBoundsException`.
            ProverError::Join(JoinError::NoAutomataSpecified) => false,
            // The label-count throw (`Join.java:53`) and the shared-label alphabet throw
            // (`ProductStrategies.java:281`) are both real `WalnutException`s.
            ProverError::Join(_) => true,

            // `Integer.parseInt(m.group(GROUP_CONVERT_BASE))` (`Prover.java:740`) —
            // `NumberFormatException`, same bucket as `ProverError::NumberFormat`.
            ProverError::Convert(ConvertError::InvalidBase(_)) => false,
            // WB-033: `ns.parseBase()` on a `null` NS is a `NullPointerException`.
            ProverError::Convert(ConvertError::Convert(ConvertNsError::NoNumberSystem)) => false,
            // The remaining `convertNS` failures, and `convertDFAOIntoFunction`, are
            // deliberately-thrown `WalnutException`s.
            ProverError::Convert(_) => true,
            // `ost`: `ParseMethods.parseList`'s `UtilityMethods.parseInt` throws a plain
            // `NumberFormatException` on an `int`-overflowing digit run (`ost o
            // [99999999999] [1];` is regex-legal), same bucket as
            // `ProverError::NumberFormat` below. Everything else `ost` can produce is a
            // real `WalnutException` (`Ostrowski`'s two `assertValues` throws and
            // `writeAutomaton`'s already-exists throw) or this port's own
            // Result-propagating I/O idiom.
            ProverError::Ost(OstError::Parse(_)) => false,
            ProverError::Ost(_) => true,
            // `Integer.parseInt`'s `NumberFormatException` — not a `WalnutException`,
            // same bucket as `MetaCommandError::NumberFormat`.
            ProverError::NumberFormat(_) => false,
            // A recovered guard panic; see `Prover::caught`'s "known, deliberate
            // limitation" section for why this is `handled` even though the payload can
            // also stand in for a genuine JDK exception. Same call this port's
            // `wr_logic::eval::ActError::Thrown` already makes, for the same reason.
            ProverError::Thrown { .. } => true,
            // This port's own scope errors; no Java analogue.
            ProverError::UnsupportedCommand { .. } | ProverError::NotYetImplemented { .. } => true,
        }
    }

    fn message(&self) -> Option<String> {
        Some(self.to_string())
    }

    fn kind(&self) -> String {
        match self {
            ProverError::InvalidFile(_) => "java.lang.IllegalArgumentException".to_string(),
            ProverError::NumberFormat(_)
            | ProverError::AutomatonOps(AutomatonOpsError::NumberFormat(_)) => {
                "java.lang.NumberFormatException".to_string()
            }
            // `RichAlphabet.encode`'s corrupt index (WB-010); see `QuotientError::Runtime`.
            ProverError::Quotient(QuotientError::Runtime(_)) => {
                "java.lang.ArrayIndexOutOfBoundsException".to_string()
            }
            ProverError::Meta(e) => e.kind(),
            _ => "Main.WalnutException".to_string(),
        }
    }

    fn stack_trace_lines(&self) -> Vec<String> {
        match self {
            ProverError::Meta(e) => e.stack_trace_lines(),
            _ => Vec::new(),
        }
    }
}

macro_rules! prover_error_from {
    ($($from:ty => $variant:ident),+ $(,)?) => {
        $(impl From<$from> for ProverError {
            fn from(e: $from) -> Self {
                ProverError::$variant(e)
            }
        })+
    };
}

prover_error_from! {
    MetaCommandError => Meta,
    EvalDefError => EvalDef,
    RegError => Reg,
    AlphabetError => Alphabet,
    ProverHelperError => Helper,
    TestError => Test,
    TransduceCommandError => Transduce,
    io::Error => Io,
    AutomatonOpsError => AutomatonOps,
    ReverseError => Reverse,
    QuotientError => Quotient,
    DescribeError => Describe,
    SimpleTransformError => SimpleTransform,
    MorphismCommandError => Morphism,
    ImageError => Image,
    JoinError => Join,
    ConvertError => Convert,
    OstError => Ost,
}

// ---------------------------------------------------------------------------
// Prover
// ---------------------------------------------------------------------------

/// `Main/Prover.java`'s instance state, plus the statics it read (see the module docs).
pub struct Prover {
    session: Session,
    logging: Logging,
    /// `Prover.metaCommands` (`:246`) — rebuilt by every `parseSetup`.
    meta_commands: MetaCommands,
    /// `Prover.printDetails` (`:249`).
    print_details: bool,
    /// `Prover.printFlag` (`:250`).
    print_flag: bool,
    /// `Prover.currentEvalName` (`:252`).
    current_eval_name: Option<String>,
    /// `System.out`, injectable. Everything Java writes with a bare `System.out.print`
    /// (the prompt, the file echo, the welcome banner, `TRUE`/`FALSE`, `clearScreen`)
    /// goes here; `Logging`'s own console sink is separate, exactly as in Java.
    out: Box<dyn Write>,
}

impl Prover {
    /// A prover over `session`, writing to the real process stdout, with the global log
    /// initialized under the session's `Result/` directory.
    ///
    /// Java runs `Logging.initializeGlobalLog(getAddressForResult() +
    /// GLOBAL_LOG_FILENAME)` as the last statement of `Session.setPathsAndNames`
    /// (`Session.java:84`); this port has no static `Logging`, so the call moves to the
    /// point where a `Logging` first exists. Nothing can be logged in between.
    pub fn new(session: Session) -> Self {
        Prover::with_output(session, Logging::new(), Box::new(io::stdout()))
    }

    /// As [`Prover::new`], with both sinks injected — the seam the tests below and any
    /// future harness use.
    pub fn with_output(session: Session, mut logging: Logging, out: Box<dyn Write>) -> Self {
        logging.initialize_global_log(&format!(
            "{}{GLOBAL_LOG_FILENAME}",
            session.paths().address_for_result()
        ));
        Prover {
            meta_commands: MetaCommands::with_paths(session.paths_rc()),
            session,
            logging,
            print_details: false,
            print_flag: false,
            current_eval_name: None,
            out,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn logging(&self) -> &Logging {
        &self.logging
    }

    pub fn logging_mut(&mut self) -> &mut Logging {
        &mut self.logging
    }

    /// `Prover.metaCommands` (`:246`) — the parsed metacommand state of the command most
    /// recently dispatched.
    pub fn meta_commands(&self) -> &MetaCommands {
        &self.meta_commands
    }

    /// As [`Prover::meta_commands`], mutably — this is the handle a later unit passes as
    /// the `&mut dyn DeterminizeContext` the determinizer takes (see the module docs on
    /// why nothing does that yet).
    pub fn meta_commands_mut(&mut self) -> &mut MetaCommands {
        &mut self.meta_commands
    }

    /// `Prover.printFlag`/`printDetails` (`:249-250`).
    pub fn print_flag(&self) -> bool {
        self.print_flag
    }

    pub fn print_details(&self) -> bool {
        self.print_details
    }

    /// `Prover.currentEvalName` (`:252`).
    pub fn current_eval_name(&self) -> Option<&str> {
        self.current_eval_name.as_deref()
    }

    // -- parseSetup / dispatch ------------------------------------------------

    /// Flattens [`wr_core::walnut_panic::catch_walnut_panic`]'s doubled result at the
    /// dispatch boundary: a recovered panic becomes [`ProverError::Thrown`].
    ///
    /// # Why the boundary lives HERE, and not at each individual index site
    ///
    /// This mirrors Java's actual architecture. `Prover.readBuffer` wraps its
    /// `dispatch(s)` call — and therefore **every** command, not a chosen few — in
    /// `catch (RuntimeException e) { Logging.printTruncatedStackTrace(e); }`
    /// (`Prover.java:388-392`), so any unchecked exception thrown anywhere under a
    /// command costs the user that command and nothing more: the REPL (or the `load`ed
    /// command file) keeps going, and the NEXT command still runs. Several of this
    /// port's `wr-core` primitives model a Java `RuntimeException` as a `panic!`
    /// (see [`wr_core::walnut_panic`]'s module docs for why), and a Rust panic with no
    /// `catch_unwind` above it kills the **process** — strictly less faithful than
    /// Java, and a denial-of-service on an ordinary bad input file.
    ///
    /// Two narrower boundaries already existed — `wr_logic::eval::compute`'s (Java's own
    /// inner `EvalDef.compute` catch, which is why `eval`/`def` never showed this
    /// problem) and the ad-hoc ones in [`crate::quotient`]/[`crate::automaton_ops`] —
    /// but everything else (`union`, `intersect`, `join`, `inf`, `test`, `reverse`,
    /// `minimize`, `export`, …) was unprotected, and Tier-5 fuzzing found live
    /// process-killing paths into `wr_core::product`, `wr_core::automaton`,
    /// `wr_core::quantify` and `wr_core::word_automaton` reachable from a single
    /// out-of-alphabet digit in a `.txt` library file (WB-038). Patching those five
    /// index sites one at a time would have been both more work and structurally
    /// incomplete against the next one; one boundary at the layer Java itself guards
    /// covers all of them, present and future.
    ///
    /// Verified live against `walnut-java/target/Walnut-all.jar` (2026-08-16) on exactly
    /// the corrupt file this port's regression test uses
    /// (`a_corrupt_library_file_costs_one_command_not_the_process`) — a command file of
    /// `reg wrok …; union wrbad wrok; inf wrbad; combine wrcb wrbad=1; reg wralive …;`
    /// prints
    ///
    /// ```text
    /// java.lang.ArrayIndexOutOfBoundsException: Index -2 out of bounds for length 4
    ///     at Automata.FA.ProductStrategies.crossProductInternalDFA(ProductStrategies.java:139)
    /// … (again for `inf`)
    /// java.lang.IndexOutOfBoundsException: Index -1 out of bounds for length 2
    ///     at java.base/jdk.internal.util.Preconditions.outOfBounds(Preconditions.java:64)
    /// ```
    ///
    /// and then **runs the final `reg` normally**. Three failed commands, one live
    /// session — which is the behavior this boundary reproduces. (Note the third one:
    /// that is `RichAlphabet.decode`'s bounds check, i.e. the Java counterpart of
    /// [`wr_core::automaton::DecodeError::IndexOutOfBounds`], reached through `combine`.)
    ///
    /// # Known, deliberate limitation of the recovered message
    ///
    /// A panic payload is a bare string, so the Java exception *class* is not
    /// recoverable. [`ProverError::Thrown`] is therefore classified as `handled`
    /// (message-only, no invented JVM frames), exactly as `wr_logic`'s
    /// `ActError::Thrown` already is — right for the `WalnutException`-modeling guards
    /// that make up most of this surface, but a text-only divergence for the few
    /// payloads that stand in for a genuine JDK exception: real Walnut prefixes those
    /// with `java.lang.…Exception: ` and one stack frame (see the transcript above), and
    /// this port prints the bare message. Verified live on the whole 20-command corrupt-
    /// file matrix in
    /// [`Self::a_corrupt_library_file_costs_one_command_not_the_process`]: every
    /// command's OUTCOME now matches the real jar exactly, and the residual difference is
    /// precisely that missing `java.lang.…Exception: ` prefix + frame line. The
    /// *behavior* — report and continue — matches either way, and no fixture in the
    /// Tier-1 corpus reaches this path. Closing it needs the guards to carry a class,
    /// which is a wider change than this boundary.
    ///
    /// What the port DOES recover is the panic's own **site**
    /// ([`ProverError::Thrown::location`]) — not user-facing (Java has no such text) but
    /// the nearest thing to the one frame `Logging.printTruncatedStackTrace` prints for a
    /// genuine-bug case, and the reason drawing this boundary does not make a NEW port
    /// bug harder to diagnose than it was before the boundary existed.
    ///
    /// # Unwind safety
    ///
    /// The guarded closure holds `&mut self`, so a panic can leave the [`Prover`] (and its
    /// [`Session`]) mid-update — a half-written library file, a `currentEvalName` set for a
    /// command that then failed. That is not a Rust-specific hazard being papered over: it
    /// is precisely what Java's caught `RuntimeException` does to the same state, since
    /// `readBuffer` resumes on the very same `Prover` instance with whatever the aborted
    /// command had already mutated. Matching it is the point.
    fn caught<T>(outcome: Result<Result<T, ProverError>, CaughtPanic>) -> Result<T, ProverError> {
        match outcome {
            Ok(inner) => inner,
            Err(caught) => Err(ProverError::Thrown {
                message: caught.message,
                location: caught.location,
            }),
        }
    }

    /// `Prover.parseSetup(String)` (`:432-454`): reset per-command state, decode the
    /// `;`/`:`/`::` suffix into `printFlag`/`printDetails`, strip metacommands, and
    /// configure logging.
    fn parse_setup(&mut self, s: &str) -> Result<String, ProverError> {
        self.meta_commands = MetaCommands::with_paths(self.session.paths_rc());
        // **`currentEvalName` SURVIVES the rebuild.** `new MetaCommands()`
        // (`MetaCommands.java:22-25`) resets exactly two `Prover` statics, `usingOTF` and
        // `earlyExistTermination`; `Prover.currentEvalName` (`:252`) is written only by
        // `evalDefCommands` (`:599`) and by nothing else, ever. So in Java the name set by
        // an earlier `eval`/`def` is still what `getExportName` (`MetaCommands.java:68`)
        // reads on EVERY later command -- `eval myname "…"::` followed by
        // `[export * gv] somecommand::` exports to `myname_0_pre.gv`. This port keeps the
        // name on the session-lifetime `Prover` (which is the faithful part) and must
        // re-seed each per-command `MetaCommands` with it (this line).
        self.meta_commands
            .set_current_eval_name(self.current_eval_name.as_deref());
        self.print_details = false;
        self.print_flag = false;

        if !s.ends_with(';') && !s.ends_with(':') {
            return Err(ProverError::InvalidCommand(s.to_string()));
        }
        let mut ending_to_remove = 1;
        if s.ends_with(':') {
            self.print_flag = true;
            if s.ends_with("::") {
                ending_to_remove += 1;
                self.print_details = true;
            }
        }
        // `s.substring(0, s.length() - endingToRemove)` -- the removed characters are `;`
        // and `:`, both ASCII, so byte and char arithmetic agree here.
        let s = &s[..s.len() - ending_to_remove];
        // `s.strip()` (`:448`) -- [`java_strip`], NOT `str::trim`; the two disagree on
        // eight code points (see that function's docs and `PORTING.md`).
        let s = java_strip(s);

        let s = self
            .meta_commands
            .parse_meta_commands(s, self.print_details)?;
        self.logging
            .configure_for_command(self.print_flag, self.print_details);
        Ok(s)
    }

    /// `Prover.dispatch(String)` (`:401-430`) — returns `false` iff the command was
    /// `exit`/`quit` (or a `load` whose file contained one).
    ///
    /// **The panic-recovery boundary.** See [`Prover::caught`]: everything below this
    /// line runs inside [`wr_core::walnut_panic::catch_walnut_panic`], the way
    /// everything Java's `readBuffer` calls runs inside its `catch (RuntimeException)`.
    pub fn dispatch(&mut self, s: &str) -> Result<bool, ProverError> {
        Self::caught(catch_walnut_panic_detailed(|| self.dispatch_uncaught(s)))
    }

    /// [`Prover::dispatch`]'s body, minus the panic boundary.
    fn dispatch_uncaught(&mut self, s: &str) -> Result<bool, ProverError> {
        let original_command = s.to_string();
        let s = self.parse_setup(s)?;
        if s.is_empty() {
            return Ok(true);
        }
        self.logging.log_command(Some(&original_command));

        let caps =
            find(&patterns().cmd, &s).ok_or_else(|| ProverError::InvalidCommand(s.clone()))?;
        let command_name = group(&caps, &s, 1).unwrap_or("").to_string();
        if !patterns().list_of_cmds.is_match(command_name.as_str()) {
            return Err(ProverError::NoSuchCommand);
        }

        let mut exit_val = !(command_name == EXIT || command_name == QUIT);
        if command_name == LOAD {
            // Special-cased BEFORE the switch, "since load is a batch command" (`:420`).
            exit_val = self.load_command(&s)?;
        } else {
            self.process_command(&s, &command_name)?;
        }

        if self.meta_commands.using_otf() {
            // Unreachable while the OTF family is deferred -- see `crate::meta_commands`.
            self.logging.log_and_print(OTF_MESSAGE);
        }
        Ok(exit_val)
    }

    /// `Prover.dispatchForIntegrationTest(String s, String msg)` (`:456-475`).
    ///
    /// Java's `msg` parameter is never used in the method body; ported as `_msg` rather
    /// than dropped, so a Java call site translates one-for-one.
    ///
    /// Java's `dispatchForIntegrationTest` has **no** try/catch of its own — an unchecked
    /// exception propagates to the calling test, which is exactly what an `Err` return
    /// models here. So this carries the same [`Prover::caught`] boundary as
    /// [`Prover::dispatch`]: without it a `panic!`-modeled Java `RuntimeException` would
    /// take down the whole harness process instead of failing (or being recorded as) one
    /// fixture.
    pub fn dispatch_for_integration_test(
        &mut self,
        s: &str,
        msg: &str,
    ) -> Result<Option<TestCase>, ProverError> {
        Self::caught(catch_walnut_panic_detailed(|| {
            self.dispatch_for_integration_test_uncaught(s, msg)
        }))
    }

    /// [`Prover::dispatch_for_integration_test`]'s body, minus the panic boundary.
    fn dispatch_for_integration_test_uncaught(
        &mut self,
        s: &str,
        _msg: &str,
    ) -> Result<Option<TestCase>, ProverError> {
        // `s.strip()` (`:457`) -- see [`java_strip`].
        let s = java_strip(s);
        let original_command = s.to_string();
        let s = self.parse_setup(s)?;

        if s.is_empty() || s.starts_with('#') {
            return Ok(None);
        }
        self.logging.log_command(Some(&original_command));

        let caps =
            find(&patterns().cmd, &s).ok_or_else(|| ProverError::InvalidCommand(s.clone()))?;
        let command_name = group(&caps, &s, 1).unwrap_or("").to_string();
        if !patterns().list_of_cmds.is_match(command_name.as_str()) {
            return Err(ProverError::NoSuchCommand);
        }

        self.process_command(&s, &command_name)
    }

    /// `Prover.processCommand(String, String)` (`:477-577`) — the 35-arm switch, in Java's
    /// own (alphabetical) arm order.
    fn process_command(
        &mut self,
        s: &str,
        command_name: &str,
    ) -> Result<Option<TestCase>, ProverError> {
        match command_name {
            // `:479-481`
            ALPHABET => Ok(Some(self.alphabet_command(s)?)),
            // `:482-485`
            CLEAR | CLS => {
                clear_screen_to(&mut self.out);
                Ok(None)
            }
            // `:486-488` -> `Combine.combineCommand`
            COMBINE => {
                let caps = match_or_fail(&patterns().combine, s, COMBINE)?;
                let automata = group(&caps, s, GROUP_COMBINE_AUTOMATA).unwrap_or("");
                let name = group(&caps, s, GROUP_COMBINE_NAME).unwrap_or("");
                Ok(Some(combine_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    automata,
                    name,
                )?))
            }
            // `:489-491` -> `Concat.concat`
            CONCAT => {
                let caps = match_or_fail(&patterns().concat, s, CONCAT)?;
                let automata = group(&caps, s, GROUP_CONCAT_AUTOMATA).unwrap_or("");
                let name = group(&caps, s, GROUP_CONCAT_NAME).unwrap_or("");
                Ok(Some(concat_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    automata,
                    name,
                )?))
            }
            // `:492-494` -> `AutomatonLogicalOps.convertNS`
            CONVERT => Ok(Some(self.convert_command(s)?)),
            // `:495-497`
            DEF | EVAL => Ok(Some(self.eval_def_commands(s)?)),
            // `:498-500` -> `Describe.describe`
            DESCRIBE => {
                let caps = match_or_fail(&patterns().describe, s, DESCRIBE)?;
                let is_dfao = group(&caps, s, GROUP_DESCRIBE_DOLLAR_SIGN) != Some("$");
                let in_file_name = format!(
                    "{}{TXT_EXTENSION}",
                    group(&caps, s, GROUP_DESCRIBE_NAME).unwrap_or("")
                );
                Ok(Some(describe(
                    &self.session,
                    &mut self.logging,
                    is_dfao,
                    &in_file_name,
                )?))
            }
            // `:501-503` -- no body; `dispatch` already computed `exitVal`.
            EXIT | QUIT => Ok(None),
            // `:504-506` -> `ProverHelper.exportAutomata`
            EXPORT => Ok(Some(self.export_command(s)?)),
            // `:507-509` -> `AutomatonLogicalOps.fixLeadingZerosProblem`
            FIXLEADZERO => {
                let caps = match_or_fail(&patterns().fixleadzero, s, FIXLEADZERO)?;
                let old_name = group(&caps, s, GROUP_FIXLEADZERO_OLD_NAME).unwrap_or("");
                let new_name = group(&caps, s, GROUP_FIXLEADZERO_NEW_NAME).unwrap_or("");
                Ok(Some(fix_lead_zero_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    old_name,
                    new_name,
                )?))
            }
            // `:510-512` -> `AutomatonLogicalOps.fixTrailingZerosProblem`
            FIXTRAILZERO => {
                let caps = match_or_fail(&patterns().fixtrailzero, s, FIXTRAILZERO)?;
                let old_name = group(&caps, s, GROUP_FIXTRAILZERO_OLD_NAME).unwrap_or("");
                let new_name = group(&caps, s, GROUP_FIXTRAILZERO_NEW_NAME).unwrap_or("");
                Ok(Some(fix_trail_zero_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    old_name,
                    new_name,
                )?))
            }
            // `:513` -> `HelpMessages.helpCommand`. Note Java does NOT return here: the
            // arm falls through to the method's trailing `return null`.
            HELP => Err(ProverError::NotYetImplemented {
                command: HELP,
                unit: "U22",
            }),
            // `:514-516` -> `Image.image`
            IMAGE => Ok(Some(self.image_command(s)?)),
            // `:517-519` -> `ProverHelper.infFromAddress` (already ported). Java's
            // `case INF -> { infCommand(s); }` discards the boolean return and falls
            // through to `processCommand`'s trailing `return null` -- same shape as
            // `MORPHISM` below.
            INF => {
                self.inf_command(s)?;
                Ok(None)
            }
            // `:520-522` -> `Intersect.intersect`
            INTERSECT => {
                let caps = match_or_fail(&patterns().intersect, s, INTERSECT)?;
                let automata = group(&caps, s, GROUP_INTERSECT_AUTOMATA).unwrap_or("");
                let name = group(&caps, s, GROUP_INTERSECT_NAME).unwrap_or("");
                Ok(Some(intersect_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    automata,
                    name,
                )?))
            }
            // `:523-525` -> `Join.joinCommand`
            JOIN => Ok(Some(self.join_command(s)?)),
            // `:526-528` -> `Quotient.leftQuotient`
            LEFTQUO => {
                let caps = match_or_fail(&patterns().leftquo, s, LEFTQUO)?;
                let old_name1 = group(&caps, s, GROUP_QUO_OLD_NAME1).unwrap_or("");
                let old_name2 = group(&caps, s, GROUP_QUO_OLD_NAME2).unwrap_or("");
                let new_name = group(&caps, s, GROUP_QUO_NEW_NAME).unwrap_or("");
                Ok(Some(left_quotient_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    old_name1,
                    old_name2,
                    new_name,
                )?))
            }
            // `:529-531` -- reachable only through `dispatchForIntegrationTest`;
            // `dispatch` special-cases `load` before the switch. Java discards the
            // `false` return the same way (`if (!loadCommand(s)) return null;` and the
            // method returns `null` regardless).
            LOAD => {
                self.load_command(s)?;
                Ok(None)
            }
            // `:532-534`
            MACRO => {
                let caps = match_or_fail(&patterns().macro_cmd, s, MACRO)?;
                let name = group(&caps, s, M_NAME).unwrap_or("");
                let definition = group(&caps, s, M_DEFINITION).unwrap_or("");
                macro_command(&self.session, name, definition, &mut self.out);
                Ok(None)
            }
            // `:535-537` -> `WordAutomaton.minimizeSelfWithOutput`
            MINIMIZE => {
                let caps = match_or_fail(&patterns().minimize, s, MINIMIZE)?;
                let old_name = group(&caps, s, GROUP_MINIMIZE_OLD_NAME).unwrap_or("");
                let new_name = group(&caps, s, GROUP_MINIMIZE_NEW_NAME).unwrap_or("");
                Ok(Some(minimize_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    old_name,
                    new_name,
                )?))
            }
            // `:538-540` -> `Main.Commands.Morphism.morphismCommand`. Java's
            // `case MORPHISM -> { morphismCommand(s); }` discards nothing (the Java
            // method itself returns `void`) and falls through to `processCommand`'s
            // trailing `return null`.
            MORPHISM => {
                self.morphism_command(s)?;
                Ok(None)
            }
            // `:541-543` -> `Ost.ostCommand`
            OST => {
                let caps = match_or_fail(&patterns().ost, s, OST)?;
                let name = group(&caps, s, GROUP_OST_NAME).unwrap_or("");
                let preperiod = group(&caps, s, GROUP_OST_PREPERIOD).unwrap_or("");
                let period = group(&caps, s, GROUP_OST_PERIOD).unwrap_or("");
                Ok(Some(ost_command(
                    &self.session,
                    &mut self.logging,
                    &mut self.out,
                    name,
                    preperiod,
                    period,
                )?))
            }
            // `:544-546` -> `Morphism.toWordAutomaton`
            PROMOTE => Ok(Some(self.promote_command(s)?)),
            // `:547-549`
            REG => Ok(Some(self.reg_command(s)?)),
            // `:550-552` -> `Reverse.reverseCommand`
            REVERSE => {
                let caps = match_or_fail(&patterns().reverse, s, REVERSE)?;
                let is_dfao = group(&caps, s, GROUP_REVERSE_DOLLAR_SIGN) != Some("$");
                let in_file_name = format!(
                    "{}{TXT_EXTENSION}",
                    group(&caps, s, GROUP_REVERSE_OLD_NAME).unwrap_or("")
                );
                let new_name = group(&caps, s, GROUP_REVERSE_NEW_NAME).unwrap_or("");
                Ok(Some(reverse_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    &in_file_name,
                    is_dfao,
                    new_name,
                )?))
            }
            // `:553-555` -> `Quotient.rightQuotient`
            RIGHTQUO => {
                let caps = match_or_fail(&patterns().rightquo, s, RIGHTQUO)?;
                let old_name1 = group(&caps, s, GROUP_QUO_OLD_NAME1).unwrap_or("");
                let old_name2 = group(&caps, s, GROUP_QUO_OLD_NAME2).unwrap_or("");
                let new_name = group(&caps, s, GROUP_QUO_NEW_NAME).unwrap_or("");
                Ok(Some(right_quotient_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    old_name1,
                    old_name2,
                    new_name,
                )?))
            }
            // `:556-558` -> `Split.processSplitCommand(…, true, …)`. Note the error name
            // Java uses here is `REVERSE_SPLIT` ("reverse split"), not "rsplit".
            RSPLIT => {
                match_or_fail(&patterns().rsplit, s, REVERSE_SPLIT)?;
                Err(ProverError::NotYetImplemented {
                    command: RSPLIT,
                    unit: "U24",
                })
            }
            // `:559-561` -> `Split.processSplitCommand(…, false, …)`
            SPLIT => {
                match_or_fail(&patterns().split, s, SPLIT)?;
                Err(ProverError::NotYetImplemented {
                    command: SPLIT,
                    unit: "U24",
                })
            }
            // `:562-564` -> `Star.star`
            STAR => {
                let caps = match_or_fail(&patterns().star, s, STAR)?;
                let old_name = group(&caps, s, GROUP_STAR_OLD_NAME).unwrap_or("");
                let new_name = group(&caps, s, GROUP_STAR_NEW_NAME).unwrap_or("");
                Ok(Some(star_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    old_name,
                    new_name,
                )?))
            }
            // `:565-567` -> `Prover.testCommand` -> `Test.testCommand`. Java's switch arm
            // (`case TEST -> { testCommand(s); }`) discards `testCommand`'s `boolean`
            // return and falls through to the switch's own `return null;`, so this arm
            // does the same: run the command for its console output/errors, then `Ok(None)`.
            TEST => {
                let caps = match_or_fail(&patterns().test, s, TEST)?;
                let test_name = group(&caps, s, GROUP_TEST_NAME).unwrap_or("");
                let needed_str = group(&caps, s, GROUP_TEST_NUM).unwrap_or("");
                // `Integer.parseInt(m.group(GROUP_TEST_NUM))` (`:685`).
                let needed: i32 = needed_str
                    .parse()
                    .map_err(|_| ProverError::NumberFormat(needed_str.to_string()))?;
                test_command_to(
                    &self.session,
                    &mut self.logging,
                    test_name,
                    needed,
                    &mut self.out,
                )?;
                Ok(None)
            }
            // `:568-570` -> `Transducer.transduceNonDeterministic`
            TRANSDUCE => Ok(Some(self.transduce_command(s)?)),
            // `:571-573` -> `Union.union`
            UNION => {
                let caps = match_or_fail(&patterns().union, s, UNION)?;
                let automata = group(&caps, s, GROUP_UNION_AUTOMATA).unwrap_or("");
                let name = group(&caps, s, GROUP_UNION_NAME).unwrap_or("");
                Ok(Some(union_command(
                    &self.session,
                    &mut self.logging,
                    s,
                    automata,
                    name,
                )?))
            }
            // `:574` -- unreachable from `dispatch` (the name was already checked against
            // `RE_FOR_THE_LIST_OF_CMDS`), but ported verbatim.
            _ => Err(ProverError::InvalidCommand(command_name.to_string())),
        }
    }

    // -- individual commands ---------------------------------------------------

    /// `Prover.loadCommand(String)` (`:584-595`) — "load x.p; loads commands from the
    /// file x.p. […] The user don't get a warning if the x.p contains load x.p but the
    /// program might end up in an infinite loop."
    pub fn load_command(&mut self, s: &str) -> Result<bool, ProverError> {
        let caps = match_or_fail(&patterns().load, s, LOAD)?;
        let filename = group(&caps, s, L_FILENAME).unwrap_or("");
        let address = self
            .session
            .paths()
            .read_address_for_command_files(filename);
        validate_file(&address).map_err(ProverError::InvalidFile)?;
        match File::open(&address) {
            Ok(f) => {
                let mut reader = BufReader::new(f);
                if !self.read_buffer(&mut reader, false) {
                    return Ok(false);
                }
            }
            // Java's `catch (IOException e) { Logging.printTruncatedStackTrace(e); }`
            // (`:591-593`) -- logged, not propagated, and `loadCommand` still returns
            // `true`.
            Err(e) => {
                let io_err = IoLoggable(e);
                self.logging.print_truncated_stack_trace(&io_err);
            }
        }
        Ok(true)
    }

    /// `Prover.evalDefCommands(String)` (`:597-602`).
    fn eval_def_commands(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().eval_def, s, "eval/def")?;
        // `currentEvalName = m.group(ED_NAME);` -- null in headless mode; used for the
        // export metacommand, hence the second assignment into `MetaCommands`.
        let eval_name = group(&caps, s, ED_NAME).map(|n| n.to_string());
        self.current_eval_name = eval_name.clone();
        self.meta_commands
            .set_current_eval_name(eval_name.as_deref());

        let predicate = group(&caps, s, ED_PREDICATE).unwrap_or("").to_string();
        let free_vars = group(&caps, s, ED_FREE_VARIABLES).map(|v| v.to_string());

        // One `FreshIdentifiers` per evaluation -- `PORTING.md`'s
        // `Token.getUniqueString()` ruling.
        let mut fresh = FreshIdentifiers::new();
        // THE `shouldPrintDetails()` GATE (`DeterminizationStrategies.java:95`, and this
        // module's docs above). Java reads `Prover.mainProver.metaCommands` from inside
        // the determinization dispatcher, but only when `Logging.shouldPrintDetails()`
        // holds -- with its own comment explaining why ("several silent automata
        // creations for NS, Ostrowski, and other caches"). `Logging.shouldPrintDetails()`
        // is `printEnabled && printDetails`; this port splits those two halves:
        //
        // * `printDetails` is `self.print_details`, set by `parse_setup` from the `::`
        //   suffix -- and `MetaCommands::parse_meta_commands` has ALREADY refused any
        //   metacommand on a command without it (`MetaCommands.java:91-93`), so a `Some`
        //   here on a `;`/`:` command could only ever carry an empty `MetaCommands`.
        //   Gating anyway keeps the counter provably still, exactly as Java's does.
        // * `printEnabled` is Java's `Logging.disablePrint()`/`enablePrint()` bracket,
        //   used only around the automata `NumberSystem` builds for itself. This port
        //   models it structurally: `wr_core::numsys` calls `quantify`/`reverse`/... with
        //   no context at all, so those constructions cannot move the counter.
        let ctx: Option<&mut dyn DeterminizeContext> = if self.print_details {
            Some(&mut self.meta_commands)
        } else {
            None
        };
        let tc = eval_def_command_with_stdout_and_ctx(
            &self.session,
            &mut self.logging,
            &mut fresh,
            self.print_flag,
            self.print_details,
            &predicate,
            eval_name.as_deref(),
            free_vars.as_deref(),
            &mut self.out,
            ctx,
        )?;
        Ok(tc)
    }

    /// `Prover.regCommand(String)` (`:615-618`).
    fn reg_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().reg, s, REG)?;
        let list_of_alphabets = group(&caps, s, R_LIST_OF_ALPHABETS)
            .unwrap_or("")
            .to_string();
        let regexp = group(&caps, s, R_REGEXP).unwrap_or("").to_string();
        let name = group(&caps, s, R_NAME).unwrap_or("").to_string();
        Ok(reg(
            &self.session,
            &mut self.logging,
            &list_of_alphabets,
            &regexp,
            &name,
        )?)
    }

    /// `Prover.transduceCommand(String)` (`:693-704`).
    fn transduce_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().transduce, s, TRANSDUCE)?;
        let new_name = group(&caps, s, GROUP_TRANSDUCE_NEW_NAME)
            .unwrap_or("")
            .to_string();
        let transducer_name = group(&caps, s, GROUP_TRANSDUCE_TRANSDUCER)
            .unwrap_or("")
            .to_string();
        // `boolean isDFAO = !(m.group(GROUP_TRANSDUCE_DOLLAR_SIGN).equals("$"));` (`:698`).
        let is_dfao = group(&caps, s, GROUP_TRANSDUCE_DOLLAR_SIGN) != Some("$");
        let old_name = group(&caps, s, GROUP_TRANSDUCE_OLD_NAME)
            .unwrap_or("")
            .to_string();
        Ok(crate::transduce::transduce_command(
            &self.session,
            s,
            &mut self.logging,
            &transducer_name,
            is_dfao,
            &old_name,
            &new_name,
        )?)
    }

    /// `Prover.alphabetCommand(String)` (`:764-771`). Note it reads
    /// `R_LIST_OF_ALPHABETS` — `reg`'s group constant — off the `alphabet` pattern.
    fn alphabet_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().alphabet, s, ALPHABET)?;
        let list_of_alphabets = group(&caps, s, R_LIST_OF_ALPHABETS).map(|v| v.to_string());
        let is_dfao = group(&caps, s, GROUP_ALPHABET_DOLLAR_SIGN) != Some("$");
        let in_file_name = format!(
            "{}{TXT_EXTENSION}",
            group(&caps, s, GROUP_ALPHABET_OLD_NAME).unwrap_or("")
        );
        let new_name = group(&caps, s, GROUP_ALPHABET_NEW_NAME)
            .unwrap_or("")
            .to_string();
        Ok(alphabet_command(
            &self.session,
            &mut self.logging,
            s,
            list_of_alphabets.as_deref(),
            is_dfao,
            &in_file_name,
            &new_name,
        )?)
    }

    /// `Prover.morphismCommand(String)` (`:632-635`) -> `Main.Commands.Morphism.
    /// morphismCommand`. Note `GROUP_MORPHISM_DEFINITION == 0` (Java's WHOLE MATCH,
    /// not a typo — see that constant's own doc comment in this file).
    fn morphism_command(&mut self, s: &str) -> Result<(), ProverError> {
        let caps = match_or_fail(&patterns().morphism, s, MORPHISM)?;
        let definition = group(&caps, s, GROUP_MORPHISM_DEFINITION).unwrap_or("");
        let name = group(&caps, s, GROUP_MORPHISM_NAME).unwrap_or("");
        // `&mut self.out`, not the real-stdout `morphism_command` convenience wrapper --
        // matches `eval_def_commands`' own injectable-sink convention (`Prover.out` is
        // everything Java writes with a bare `System.out.print`, see the struct's own
        // doc comment).
        morphism_command_to(&self.session, definition, name, &mut self.out)?;
        Ok(())
    }

    /// `Prover.promoteCommand(String)` (`:637-650`) -> `Morphism.toWordAutomaton`.
    fn promote_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().promote, s, PROMOTE)?;
        let morphism_name = group(&caps, s, GROUP_PROMOTE_MORPHISM).unwrap_or("");
        let name = group(&caps, s, GROUP_PROMOTE_NAME).unwrap_or("");
        Ok(promote_command(&self.session, s, morphism_name, name)?)
    }

    /// `Prover.imageCommand(String)` (`:652-656`) -> `Image.image`.
    fn image_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().image, s, IMAGE)?;
        let morphism_name = group(&caps, s, GROUP_IMAGE_MORPHISM).unwrap_or("");
        let old_name = group(&caps, s, GROUP_IMAGE_OLD_NAME).unwrap_or("");
        let new_name = group(&caps, s, GROUP_IMAGE_NEW_NAME).unwrap_or("");
        Ok(image(
            &self.session,
            &mut self.logging,
            s,
            morphism_name,
            old_name,
            new_name,
            self.print_flag,
        )?)
    }

    /// `Prover.infCommand(String)` (`:658-661`) -> `ProverHelper.infFromAddress`
    /// (already ported). Java's `boolean` return is discarded by `processCommand`'s
    /// `INF -> { infCommand(s); }` arm (no `return`) — see [`Self::process_command`].
    fn inf_command(&mut self, s: &str) -> Result<bool, ProverError> {
        let caps = match_or_fail(&patterns().inf, s, INF)?;
        let name = group(&caps, s, GROUP_INF_NAME).unwrap_or("");
        Ok(inf_from_address_to(
            &self.session,
            &mut self.logging,
            name,
            &mut self.out,
        )?)
    }

    /// `Prover.joinCommand(String)` (`:677-680`) -> `Join.joinCommand`.
    fn join_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().join, s, JOIN)?;
        let automata = group(&caps, s, GROUP_JOIN_AUTOMATA).unwrap_or("");
        let name = group(&caps, s, GROUP_JOIN_NAME).unwrap_or("");
        Ok(join_command(
            &self.session,
            &mut self.logging,
            s,
            automata,
            name,
        )?)
    }

    /// `Prover.convertCommand(String)` (`:724-745`).
    fn convert_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().convert, s, CONVERT)?;
        let new_dollar_sign = group(&caps, s, GROUP_CONVERT_NEW_DOLLAR_SIGN).unwrap_or("");
        let new_name = group(&caps, s, GROUP_CONVERT_NEW_NAME).unwrap_or("");
        let msd_or_lsd = group(&caps, s, GROUP_CONVERT_MSD_OR_LSD).unwrap_or("");
        let base = group(&caps, s, GROUP_CONVERT_BASE).unwrap_or("");
        let old_dollar_sign = group(&caps, s, GROUP_CONVERT_OLD_DOLLAR_SIGN).unwrap_or("");
        let old_name = group(&caps, s, GROUP_CONVERT_OLD_NAME).unwrap_or("");
        Ok(convert_command(
            &self.session,
            &mut self.logging,
            s,
            new_dollar_sign,
            new_name,
            msd_or_lsd,
            base,
            old_dollar_sign,
            old_name,
        )?)
    }

    /// `Prover.exportCommand(String)` (`:805-814`) -> `ProverHelper.exportAutomata`
    /// (already ported).
    fn export_command(&mut self, s: &str) -> Result<TestCase, ProverError> {
        let caps = match_or_fail(&patterns().export, s, EXPORT)?;
        let filename = group(&caps, s, GROUP_EXPORT_NAME).unwrap_or("");
        let in_file_name = format!("{filename}{TXT_EXTENSION}");
        let export_type = group(&caps, s, GROUP_EXPORT_TYPE).unwrap_or("");
        let is_dfao = group(&caps, s, GROUP_EXPORT_DOLLAR_SIGN) != Some("$");

        // `Automaton M = new Automaton(ProverHelper.determineInLibrary(isDFAO,
        //  inFileName));` (`:811`) -- `ProverHelperError::Read` is the same variant
        // `ProverHelper.infFromAddress`'s own automaton read already uses.
        let in_library = determine_in_library(self.session.paths(), is_dfao, &in_file_name);
        let m = self
            .session
            .libraries()
            .read_library_automaton(&in_library)
            .map_err(ProverHelperError::Read)?;

        // `ProverHelper.exportAutomata(s, filename, exportType, M, isDFAO);` (`:812`).
        export_automata_to(
            self.session.paths(),
            Some(s),
            filename,
            export_type,
            &m,
            is_dfao,
            &mut self.out,
        )?;

        Ok(TestCase::from_automaton(m))
    }

    // -- the reader loop --------------------------------------------------------

    /// `Prover.readBuffer(BufferedReader, boolean console)` (`:354-399`) — the ONE loop
    /// behind both the REPL and `load`/command-file execution. Returns `false` iff a
    /// command asked to exit.
    ///
    /// Quirks preserved: lines are concatenated into the buffer with **no separator**
    /// (`:372`), a `#` line is echoed and skipped *before* buffering, and a
    /// `RuntimeException` out of `dispatch` is logged and the loop continues (`:390-392`).
    pub fn read_buffer(&mut self, input: &mut dyn BufRead, console: bool) -> bool {
        let mut buffer = String::new();
        loop {
            if console {
                let _ = write!(self.out, "{PROMPT}");
                let _ = self.out.flush();
            }

            let mut line = String::new();
            let read = match input.read_line(&mut line) {
                Ok(n) => n,
                // `catch (IOException e)` around the whole loop (`:394-396`).
                Err(e) => {
                    let io_err = IoLoggable(e);
                    self.logging.print_truncated_stack_trace(&io_err);
                    return true;
                }
            };
            if read == 0 {
                // `if (s == null) return true;` -- end of input.
                return true;
            }
            // `BufferedReader.readLine` strips the terminator; `read_line` keeps it, and
            // the following `strip()` (`:366`, i.e. [`java_strip`]) removes it either way.
            //
            // One residual, deliberately NOT papered over: `readLine` also treats a BARE
            // `\r` as a line terminator, while `BufRead::read_line` splits on `\n` only.
            // A classic-Mac-style file would therefore arrive here as one long line. Noted
            // rather than fixed -- it would mean hand-rolling the reader, and no Walnut
            // command file in the corpus is `\r`-terminated.
            let s = java_strip(&line);
            if s.starts_with('#') {
                let _ = writeln!(self.out, "{s}");
                continue;
            }

            buffer.push_str(s);

            if !(s.ends_with(';') || s.ends_with(':')) {
                continue;
            }

            let command = std::mem::take(&mut buffer);

            if !console {
                let _ = writeln!(self.out, "{command}");
            }

            match self.dispatch(&command) {
                Ok(true) => {}
                Ok(false) => return false,
                // **The two catch clauses are not the same clause.** Java's INNER
                // `catch (RuntimeException e)` (`:390-392`) sits inside the `while`, so an
                // unchecked exception is logged and the NEXT line is read. But `dispatch`
                // is declared `throws IOException` (`:401`), and a checked `IOException`
                // is not a `RuntimeException` -- it escapes to the OUTER
                // `catch (IOException e)` (`:394-396`), which is outside the loop, so the
                // loop ENDS and `readBuffer` returns `true` (no further line of the file
                // or of the REPL is executed).
                Err(e) if is_io_class_error(&e) => {
                    self.logging.print_truncated_stack_trace(&e);
                    return true;
                }
                Err(e) => self.logging.print_truncated_stack_trace(&e),
            }
        }
    }

    /// `Prover.run(String filename)` (`:324-348`) — run a command file (if given), then
    /// print the banner and hand control to the console REPL.
    ///
    /// The `validateFile` failure is an `IllegalArgumentException` in Java, uncaught, so
    /// it aborts the process; here it is a returned `Err` for `main` to report.
    pub fn run(&mut self, filename: Option<&str>) -> Result<(), ProverError> {
        self.run_with_input(filename, &mut BufReader::new(io::stdin()))
    }

    /// As [`Prover::run`], with the console stream injected.
    pub fn run_with_input(
        &mut self,
        filename: Option<&str>,
        console_input: &mut dyn BufRead,
    ) -> Result<(), ProverError> {
        if let Some(filename) = filename {
            let address = self
                .session
                .paths()
                .read_address_for_command_files(filename);
            validate_file(&address).map_err(ProverError::InvalidFile)?;
            match File::open(&address) {
                Ok(f) => {
                    let mut reader = BufReader::new(f);
                    if !self.read_buffer(&mut reader, false) {
                        return Ok(());
                    }
                }
                Err(e) => {
                    let io_err = IoLoggable(e);
                    self.logging.print_truncated_stack_trace(&io_err);
                }
            }
        }

        // `:336-342`.
        let _ = writeln!(
            self.out,
            "Welcome to Walnut v{WALNUT_VERSION}! Type \"help;\" to see all available commands."
        );
        if self.session.paths().is_global_session() {
            let _ = writeln!(self.out, "Using global Walnut session.");
        } else {
            let _ = writeln!(
                self.out,
                "Starting Walnut session: {}",
                self.session.paths().name().unwrap_or("")
            );
        }

        self.read_buffer(console_input, true);
        Ok(())
    }
}

/// Is this error one that Java would have thrown as a **checked `IOException`** rather
/// than as an unchecked `RuntimeException`?
///
/// The distinction is invisible in Rust (one `Result`, one error enum) but load-bearing in
/// `Prover.readBuffer`, whose two `catch` clauses do opposite things — see the call site.
///
/// As of U24, three real Java methods declare `throws IOException`:
/// `morphismCommand` (`:632`), `promoteCommand` (`:637`), and `imageCommand` (`:652`)
/// — `transduce` is the fourth and still belongs to U26. `joinCommand`/`convertCommand`
/// do **not** declare `throws IOException` at all, so a real Java compile PROVES no
/// checked `IOException` ever escapes them.
///
/// This port's per-module error enums do not distinguish, at the `Io` variant level,
/// which underlying I/O call produced a failure — so the rule applied below is: for
/// [`MorphismCommandError`]/[`ImageError`] (the three genuinely `throws IOException`
/// Java methods), their `Io` sub-variant is IO-class (`true`); every other variant of
/// every command error is RuntimeException-class (`false`). This is deliberately
/// CONSERVATIVE, not a precise per-call-site match: `Morphism::write_to_file`'s two
/// writes (`morphism`) and `Files.readString`/`UtilityMethods.readFromFile`
/// (`promote`/`image`) are genuine, uncaught-in-Java `IOException` sources that really
/// do escape to `readBuffer`'s OUTER catch — but SOME `Io` instances in this port will
/// actually originate from `crate::automaton_output::write_automata`'s copy step
/// instead, which real `Automaton.writeAutomata` swallows internally (see that
/// module's own docs) and which therefore would NOT escape this far in Java. Treating
/// every `Io` from these two modules as IO-class is the safer of the two possible
/// readings ("this command is one of the three that CAN throw a real IOException"),
/// not a false claim that every instance definitely would have in Java.
///
/// [`JoinError`]/[`ConvertError`] have no such ambiguity: their `Io` variant exists
/// ONLY because of this port's own `write_automata` propagate-rather-than-swallow
/// idiom deviation (`crate::automaton_output`'s own documented, deliberate choice) —
/// never a Java-observable event, since `joinCommand`/`convertCommand` cannot let an
/// `IOException` escape at all (the Java compiler would reject that). So `false`,
/// alongside their other variants.
fn is_io_class_error(e: &ProverError) -> bool {
    match e {
        // See the paragraph above: conservative, not per-call-site-precise.
        ProverError::Morphism(MorphismCommandError::Io(_))
        | ProverError::Image(ImageError::Io(_)) => true,
        ProverError::Io(_) => true,
        // Exhaustive on purpose: a new variant must make this decision deliberately.
        ProverError::InvalidCommand(_)
        | ProverError::NoSuchCommand
        | ProverError::InvalidCommandUse(_)
        | ProverError::InvalidFile(_)
        | ProverError::WalnutMessage(_)
        | ProverError::Meta(_)
        | ProverError::EvalDef(_)
        | ProverError::Reg(_)
        | ProverError::Alphabet(_)
        // `Helper` and `Test` each carry their own nested `Io` variant (a failed write to
        // the command's output sink). Those stay NON-I/O-class, deliberately and
        // consistently with each other: Java's `System.out.println` cannot throw
        // `IOException` at all, so neither `ProverHelper` nor `Test` has an I/O failure
        // mode for `readBuffer` to classify — the variants are this port's own
        // propagate-instead-of-swallow decision (`crate::automaton_output`'s docs), and a
        // console-write failure must not end the REPL read loop when Java's would not.
        | ProverError::Helper(_)
        | ProverError::Test(_)
        | ProverError::Transduce(_)
        | ProverError::NumberFormat(_)
        | ProverError::AutomatonOps(_)
        | ProverError::Reverse(_)
        | ProverError::Quotient(_)
        | ProverError::Describe(_)
        | ProverError::SimpleTransform(_)
        | ProverError::Morphism(_)
        | ProverError::Image(_)
        | ProverError::Join(_)
        | ProverError::Convert(_)
        // `OstError::Io` exists only because `wr_io::writer` propagates where Java's
        // `AutomatonWriter.writeToTxtFormat` catches its own `IOException` and merely
        // logs it (`Ostrowski.writeAutomaton` therefore cannot let one escape at all) —
        // the same reasoning `JoinError`/`ConvertError` get above.
        | ProverError::Ost(_)
        // A recovered panic stands in for an unchecked `RuntimeException`, which is by
        // definition NOT the checked `IOException` Java's outer catch selects — so the
        // read loop must log it and read the next line, never end. This is the whole
        // point of the boundary (see `Prover::caught`): the session survives.
        | ProverError::Thrown { .. }
        | ProverError::UnsupportedCommand { .. }
        | ProverError::NotYetImplemented { .. } => false,
    }
}

/// `std::io::Error` as something [`Logging::print_truncated_stack_trace`] accepts. Java
/// passes the raw `IOException`, which is not a `WalnutException`, hence `is_handled() ==
/// false`.
struct IoLoggable(io::Error);

impl LoggableError for IoLoggable {
    fn is_handled(&self) -> bool {
        false
    }

    fn message(&self) -> Option<String> {
        Some(self.0.to_string())
    }

    fn kind(&self) -> String {
        "java.io.IOException".to_string()
    }

    fn stack_trace_lines(&self) -> Vec<String> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Command-line entry point (`Prover.main`/`parseArgs`)
// ---------------------------------------------------------------------------

/// What [`parse_args`] decided.
#[derive(Debug)]
pub enum ArgsOutcome {
    /// `--help`/`-h`: print [`USAGE_MESSAGE`] and exit 0 (`Prover.java:300-303`).
    Help,
    /// Everything else: the session to run in, and the optional command file.
    Run {
        filename: Option<String>,
        session: Session,
    },
}

/// `Prover.parseArgs(String[])` (`:293-323`), **including WB-026 verbatim**.
///
/// The command-file validation at `:318` runs *inside* the argument loop, i.e. before
/// `Session.setPathsAndNames` at `:321`, so it resolves the file against Java's
/// still-uninitialized `Session.mainWalnutDir` (`""`) and ignores `--home-dir=`
/// entirely. That is a genuine Walnut bug (`docs/WALNUT-BUGS.md` WB-026); per
/// `CLAUDE.md`'s mechanical-port rule it is replicated, not fixed — which is why this
/// function builds a throwaway `SessionPaths` with an explicitly empty home directory
/// purely to run a check whose result is then thrown away (`run` re-validates
/// correctly at `:326`). `run_command_file_validation_ignores_home_dir_wb_026` pins it.
pub fn parse_args(args: &[String]) -> Result<ArgsOutcome, ProverError> {
    let mut filename: Option<String> = None;
    let mut session_dir: Option<String> = None;
    let mut home_dir: Option<String> = None;
    let mut global_session = false;

    for arg in args {
        if arg.starts_with("--help") || arg == "-h" {
            return Ok(ArgsOutcome::Help);
        }
        if let Some(rest) = arg.strip_prefix(SESSION_DIR_ARG) {
            let mut dir = rest.to_string();
            if !dir.ends_with('/') {
                dir.push('/');
            }
            session_dir = Some(dir);
        } else if let Some(rest) = arg.strip_prefix(HOME_DIR_ARG) {
            let mut dir = rest.to_string();
            if !dir.ends_with('/') {
                dir.push('/');
            }
            home_dir = Some(dir);
        } else if arg == GLOBAL_SESSION_ARG {
            global_session = true;
        } else if filename.is_none() {
            // WB-026, verbatim: resolved against an EMPTY home directory, because Java's
            // `Session.mainWalnutDir` static initializer has not been overwritten yet.
            let premature = SessionPaths::new(Some(""), Some(""), false);
            validate_file(&premature.read_address_for_command_files(arg))
                .map_err(ProverError::InvalidFile)?;
            filename = Some(arg.clone());
        }
    }

    // `Session.setPathsAndNames(sessionDir, homeDir, globalSession);` (`:321`).
    let paths = SessionPaths::new(session_dir.as_deref(), home_dir.as_deref(), global_session);
    // `Session.createSubdirectories`'s failure is a `WalnutException` (`Session.java:132`),
    // not `validateFile`'s `IllegalArgumentException`.
    paths
        .create_subdirectories()
        .map_err(ProverError::WalnutMessage)?;
    Ok(ArgsOutcome::Run {
        filename,
        session: Session::from_paths(paths),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    // ------------------------------------------------------------ scaffolding

    /// A shared, inspectable stdout sink (the `Prover` needs to own its writer).
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

    fn temp_tree(tag: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-prover-{tag}-{}-{}",
            std::process::id(),
            line!()
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
        ] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let dir_str = format!("{}/", dir.to_str().unwrap());
        (dir, dir_str)
    }

    fn prover(tag: &str) -> (Prover, PathBuf, Capture) {
        let (dir, dir_str) = temp_tree(tag);
        let capture = Capture::default();
        let session = Session::new(Some(&dir_str), Some(&dir_str), false);
        let logging = Logging::with_writers(Box::new(io::sink()), Box::new(io::sink()));
        let p = Prover::with_output(session, logging, Box::new(capture.clone()));
        (p, dir, capture)
    }

    // ------------------------------------------------------- pattern sanity

    /// The dialect trap that cost this unit a debugging round, pinned so nobody
    /// re-introduces it: Java's `\>` is a literal `>`, Rust's is an END-OF-WORD BOUNDARY.
    /// It COMPILES either way, and simply never matches what Java's did.
    #[test]
    fn an_escaped_gt_is_a_word_boundary_in_rust_not_a_literal() {
        let javaish = Regex::new(r"a\-\>b").expect("compiles fine -- that is the trap");
        assert!(!javaish.is_match("a->b"));
        let ported = Regex::new(r"a\->b").unwrap();
        assert!(ported.is_match("a->b"));
        // The escaped `-` half IS a plain literal in both dialects.
        assert!(Regex::new(r"x\-y").unwrap().is_match("x-y"));
    }

    #[test]
    fn every_pattern_compiles() {
        // Touching the table forces `Patterns::compile_all`, whose `compile` panics with
        // the offending pattern text on a syntax error.
        let p = patterns();
        assert!(p.cmd.is_match("eval x \"x=1\""));
    }

    #[test]
    fn the_command_list_holds_exactly_javas_thirty_five_names() {
        let names: Vec<&str> = RE_FOR_THE_LIST_OF_CMDS
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split('|')
            .collect();
        assert_eq!(names.len(), 35, "{names:?}");
        for name in &names {
            assert!(
                patterns().list_of_cmds.is_match(name),
                "{name} must match the list pattern"
            );
        }
        // ...and nothing else does.
        for name in ["evaluate", "ev", "", "Eval", "loadx"] {
            assert!(!patterns().list_of_cmds.is_match(name), "{name}");
        }
    }

    #[test]
    fn eval_def_group_numbers_match_javas_constants() {
        let s = r#"def phi x y "?msd_2 x < y""#;
        let caps = find(&patterns().eval_def, s).unwrap();
        assert_eq!(group(&caps, s, 1), Some("def"));
        assert_eq!(group(&caps, s, ED_NAME), Some("phi"));
        assert_eq!(group(&caps, s, ED_FREE_VARIABLES), Some(" x y"));
        assert_eq!(group(&caps, s, ED_PREDICATE), Some("?msd_2 x < y"));
    }

    #[test]
    fn a_headless_eval_has_no_name_group() {
        let s = r#"eval "?msd_2 x < y""#;
        let caps = find(&patterns().eval_def, s).unwrap();
        assert_eq!(group(&caps, s, ED_NAME), None);
        assert_eq!(group(&caps, s, ED_PREDICATE), Some("?msd_2 x < y"));
    }

    #[test]
    fn reg_group_numbers_match_javas_constants() {
        let s = r#"reg r msd_2 "0*1""#;
        let caps = find(&patterns().reg, s).unwrap();
        assert_eq!(group(&caps, s, R_NAME), Some("r"));
        assert_eq!(group(&caps, s, R_LIST_OF_ALPHABETS), Some("msd_2 "));
        assert_eq!(group(&caps, s, R_REGEXP), Some("0*1"));
    }

    #[test]
    fn alphabet_group_numbers_match_javas_constants() {
        let s = "alphabet newname msd_3 oldname";
        let caps = find(&patterns().alphabet, s).unwrap();
        assert_eq!(group(&caps, s, GROUP_ALPHABET_NEW_NAME), Some("newname"));
        assert_eq!(group(&caps, s, R_LIST_OF_ALPHABETS), Some("msd_3 "));
        assert_eq!(group(&caps, s, GROUP_ALPHABET_DOLLAR_SIGN), Some(""));
        assert_eq!(group(&caps, s, GROUP_ALPHABET_OLD_NAME), Some("oldname"));

        let s = "alphabet newname msd_3 $oldname";
        let caps = find(&patterns().alphabet, s).unwrap();
        assert_eq!(group(&caps, s, GROUP_ALPHABET_DOLLAR_SIGN), Some("$"));
        assert_eq!(group(&caps, s, GROUP_ALPHABET_OLD_NAME), Some("oldname"));
    }

    #[test]
    fn convert_group_numbers_match_javas_constants() {
        let s = "convert new lsd_3 $old";
        let caps = find(&patterns().convert, s).unwrap();
        assert_eq!(group(&caps, s, GROUP_CONVERT_NEW_NAME), Some("new"));
        assert_eq!(group(&caps, s, GROUP_CONVERT_MSD_OR_LSD), Some("lsd"));
        assert_eq!(group(&caps, s, GROUP_CONVERT_BASE), Some("3"));
        assert_eq!(group(&caps, s, GROUP_CONVERT_OLD_DOLLAR_SIGN), Some("$"));
        assert_eq!(group(&caps, s, GROUP_CONVERT_OLD_NAME), Some("old"));
    }

    /// Every remaining command's group constants, pinned to the **actual captured text**
    /// of a well-formed invocation.
    ///
    /// These 23 constants are what U23–U26 will use to pull arguments out of a command, so
    /// an off-by-one here is a silent wrong-argument bug in a unit that has not been
    /// written yet — and "the pattern matched" (which is all this test used to assert)
    /// catches none of them, because every one of these patterns matches regardless of how
    /// its groups are numbered. Two families deserve the scrutiny particularly:
    /// `GROUP_RSPLIT_AUTOMATA`/`GROUP_RSPLIT_INPUT`, which are documented as being out of
    /// numeric order (4 and 2), and `GROUP_MORPHISM_DEFINITION`, which is Java's
    /// never-assigned `int` field and therefore group **0**, the whole match.
    #[test]
    fn every_command_pattern_pins_its_group_indices() {
        /// One compiled pattern, one well-formed invocation of it, and the
        /// `(group index, captured text)` pairs that invocation must produce.
        type GroupCase<'a> = (&'a Regex, &'a str, Vec<(usize, &'a str)>);

        let p = patterns();
        let cases: Vec<GroupCase> = vec![
            (&p.load, "load cmds.txt", vec![(L_FILENAME, "cmds.txt")]),
            (
                &p.macro_cmd,
                r#"macro m "x = 1""#,
                vec![(M_NAME, "m"), (M_DEFINITION, "x = 1")],
            ),
            (
                &p.ost,
                "ost o [1 2] [3]",
                vec![
                    (GROUP_OST_NAME, "o"),
                    (GROUP_OST_PREPERIOD, "1 2"),
                    (GROUP_OST_PERIOD, "3"),
                ],
            ),
            (
                &p.combine,
                "combine c a=1 b=2",
                vec![
                    (GROUP_COMBINE_NAME, "c"),
                    (GROUP_COMBINE_AUTOMATA, " a=1 b=2"),
                ],
            ),
            (
                &p.morphism,
                r#"morphism h "0->01,1->10""#,
                vec![
                    (GROUP_MORPHISM_NAME, "h"),
                    // Group 0 -- the WHOLE match, quotes and command name and all. Not a
                    // typo: Java's `GROUP_MORPHISM_DEFINITION` is never assigned.
                    (GROUP_MORPHISM_DEFINITION, r#"morphism h "0->01,1->10""#),
                ],
            ),
            (
                &p.promote,
                "promote P h",
                vec![(GROUP_PROMOTE_NAME, "P"), (GROUP_PROMOTE_MORPHISM, "h")],
            ),
            (
                &p.image,
                "image I h T",
                vec![
                    (GROUP_IMAGE_NEW_NAME, "I"),
                    (GROUP_IMAGE_MORPHISM, "h"),
                    (GROUP_IMAGE_OLD_NAME, "T"),
                ],
            ),
            (&p.inf, "inf T", vec![(GROUP_INF_NAME, "T")]),
            (
                &p.split,
                "split S T [+][-]",
                vec![
                    (GROUP_SPLIT_NAME, "S"),
                    (GROUP_SPLIT_AUTOMATA, "T"),
                    (GROUP_SPLIT_INPUT, " [+][-]"),
                ],
            ),
            (
                &p.rsplit,
                "rsplit S [+][-] T",
                vec![
                    (GROUP_RSPLIT_NAME, "S"),
                    // The out-of-order pair: the INPUT list is group 2 and the automaton
                    // name is group 4, because the bracket list precedes the name here.
                    (GROUP_RSPLIT_INPUT, " [+][-]"),
                    (GROUP_RSPLIT_AUTOMATA, "T"),
                ],
            ),
            (
                &p.join,
                "join J A [x] B [y]",
                vec![
                    (GROUP_JOIN_NAME, "J"),
                    (GROUP_JOIN_AUTOMATA, " A [x] B [y]"),
                ],
            ),
            (
                &p.test,
                "test T 5",
                vec![(GROUP_TEST_NAME, "T"), (GROUP_TEST_NUM, "5")],
            ),
            (
                &p.transduce,
                "transduce N T $M",
                vec![
                    (GROUP_TRANSDUCE_NEW_NAME, "N"),
                    (GROUP_TRANSDUCE_TRANSDUCER, "T"),
                    (GROUP_TRANSDUCE_DOLLAR_SIGN, "$"),
                    (GROUP_TRANSDUCE_OLD_NAME, "M"),
                ],
            ),
            (
                &p.reverse,
                "reverse R $M",
                vec![
                    (GROUP_REVERSE_NEW_NAME, "R"),
                    (GROUP_REVERSE_DOLLAR_SIGN, "$"),
                    (GROUP_REVERSE_OLD_NAME, "M"),
                ],
            ),
            (
                &p.minimize,
                "minimize N M",
                vec![
                    (GROUP_MINIMIZE_NEW_NAME, "N"),
                    (GROUP_MINIMIZE_OLD_NAME, "M"),
                ],
            ),
            (
                &p.fixleadzero,
                "fixleadzero N $M",
                vec![
                    (GROUP_FIXLEADZERO_NEW_NAME, "N"),
                    (GROUP_FIXLEADZERO_OLD_NAME, "M"),
                ],
            ),
            (
                &p.fixtrailzero,
                "fixtrailzero N $M",
                vec![
                    (GROUP_FIXTRAILZERO_NEW_NAME, "N"),
                    (GROUP_FIXTRAILZERO_OLD_NAME, "M"),
                ],
            ),
            (
                &p.union,
                "union U A B",
                vec![(GROUP_UNION_NAME, "U"), (GROUP_UNION_AUTOMATA, " A B")],
            ),
            (
                &p.intersect,
                "intersect I A B",
                vec![
                    (GROUP_INTERSECT_NAME, "I"),
                    (GROUP_INTERSECT_AUTOMATA, " A B"),
                ],
            ),
            (
                &p.star,
                "star S A",
                vec![(GROUP_STAR_NEW_NAME, "S"), (GROUP_STAR_OLD_NAME, "A")],
            ),
            (
                &p.concat,
                "concat C A B",
                vec![(GROUP_CONCAT_NAME, "C"), (GROUP_CONCAT_AUTOMATA, " A B")],
            ),
            (
                &p.rightquo,
                "rightquo Q A B",
                vec![
                    (GROUP_QUO_NEW_NAME, "Q"),
                    (GROUP_QUO_OLD_NAME1, "A"),
                    (GROUP_QUO_OLD_NAME2, "B"),
                ],
            ),
            (
                &p.leftquo,
                "leftquo Q A B",
                vec![
                    (GROUP_QUO_NEW_NAME, "Q"),
                    (GROUP_QUO_OLD_NAME1, "A"),
                    (GROUP_QUO_OLD_NAME2, "B"),
                ],
            ),
            (
                &p.export,
                "export $M gv",
                vec![
                    (GROUP_EXPORT_DOLLAR_SIGN, "$"),
                    (GROUP_EXPORT_NAME, "M"),
                    (GROUP_EXPORT_TYPE, "gv"),
                ],
            ),
            (
                &p.describe,
                "describe $M",
                vec![
                    (GROUP_DESCRIBE_DOLLAR_SIGN, "$"),
                    (GROUP_DESCRIBE_NAME, "M"),
                ],
            ),
        ];
        for (re, s, expected) in cases {
            let caps = find(re, s).unwrap_or_else(|| panic!("pattern should match {s:?}"));
            for (index, want) in expected {
                assert_eq!(
                    group(&caps, s, index),
                    Some(want),
                    "group {index} of {s:?} must capture {want:?}"
                );
            }
        }
    }

    /// The `$`-marker groups again, with the marker ABSENT — `DOLLAR`'s second alternative
    /// (`\s*`) then captures the empty string rather than failing to participate, which is
    /// what every `is_dfao`-style caller compares against `"$"`.
    #[test]
    fn an_absent_dollar_marker_captures_the_empty_string_not_nothing() {
        let p = patterns();
        let s = "reverse R M";
        let caps = find(&p.reverse, s).unwrap();
        assert_eq!(group(&caps, s, GROUP_REVERSE_DOLLAR_SIGN), Some(""));
        assert_eq!(group(&caps, s, GROUP_REVERSE_OLD_NAME), Some("M"));
    }

    #[test]
    fn the_split_input_and_set_element_patterns_work() {
        assert!(find(&patterns().input_in_split, "[ + ]").is_some());
        assert!(find(&patterns().single_element_of_a_set, "- 3").is_some());
    }

    // ------------------------------------------------------ java_strip / errors

    /// `String.strip()` and `str::trim` are NOT the same function, in either direction.
    /// Both disagreements are pinned, because a port that reaches for `.trim()` compiles,
    /// passes every ASCII test, and is wrong only on input nobody thinks to write.
    #[test]
    fn java_strip_is_not_str_trim() {
        // Ordinary ASCII: identical.
        assert_eq!(java_strip("  eval x \t\r\n"), "eval x");
        assert_eq!(java_strip(""), "");
        assert_eq!(java_strip("   "), "");

        // Rust strips these, `Character.isWhitespace` does not: the three non-breaking
        // spaces and NEL.
        for c in ['\u{A0}', '\u{2007}', '\u{202F}', '\u{85}'] {
            let s = format!("{c}x{c}");
            assert_eq!(java_strip(&s), s, "{c:?} is NOT Java whitespace");
            assert_eq!(
                s.trim(),
                "x",
                "...but str::trim strips it -- that is the trap"
            );
        }

        // Java strips these, Rust does not: the four ASCII information separators.
        for c in ['\u{1C}', '\u{1D}', '\u{1E}', '\u{1F}'] {
            let s = format!("{c}x{c}");
            assert_eq!(java_strip(&s), "x", "{c:?} IS Java whitespace");
            assert_eq!(s.trim(), s, "...but str::trim leaves it");
        }

        // The Unicode space separators both agree on.
        assert_eq!(java_strip("\u{2000}\u{3000}x\u{2028}"), "x");
    }

    /// Java's `readBuffer` has two `catch` clauses that do OPPOSITE things, and only the
    /// checked-`IOException` one ends the loop. Nothing constructs [`ProverError::Io`]
    /// yet, so this pins the classification directly rather than through `read_buffer`.
    #[test]
    fn only_io_errors_are_classified_as_javas_checked_exception() {
        assert!(is_io_class_error(&ProverError::Io(io::Error::other(
            "boom"
        ))));
        for e in [
            ProverError::NoSuchCommand,
            ProverError::InvalidCommand("x".to_string()),
            ProverError::InvalidCommandUse("x".to_string()),
            ProverError::InvalidFile("x".to_string()),
            ProverError::WalnutMessage("x".to_string()),
            ProverError::NotYetImplemented {
                command: "star",
                unit: "U23",
            },
        ] {
            assert!(!is_io_class_error(&e), "{e} must not end the read loop");
        }
    }

    /// U24's own extension of the classification above: `morphismCommand`/
    /// `promoteCommand`/`imageCommand` are the three real Java methods that declare
    /// `throws IOException` (see [`is_io_class_error`]'s doc comment), so ONLY their
    /// `Io` sub-variant is IO-class; `joinCommand`/`convertCommand` declare no such
    /// thing, so NONE of their variants (including their own `Io`, which exists purely
    /// as this port's `write_automata` idiom deviation) are.
    #[test]
    fn u24_command_errors_classify_by_javas_throws_ioexception_declaration() {
        assert!(
            is_io_class_error(&ProverError::Morphism(MorphismCommandError::Io(
                io::Error::other("boom")
            ))),
            "morphismCommand/promoteCommand declare throws IOException"
        );
        assert!(
            is_io_class_error(&ProverError::Image(ImageError::Io(io::Error::other(
                "boom"
            )))),
            "imageCommand declares throws IOException"
        );
        assert!(
            !is_io_class_error(&ProverError::Morphism(MorphismCommandError::Parse(
                wr_io::parse_methods::ParseMethodsError::NoValidMorphismMappings
            ))),
            "a non-Io MorphismCommandError variant is still RuntimeException-class"
        );
        assert!(
            !is_io_class_error(&ProverError::Join(JoinError::Io(io::Error::other("boom")))),
            "joinCommand declares no throws IOException at all"
        );
        assert!(
            !is_io_class_error(&ProverError::Convert(ConvertError::Io(io::Error::other(
                "boom"
            )))),
            "convertCommand declares no throws IOException at all"
        );
    }

    /// The OTHER half of the same distinction, on the other classifier: `is_handled`
    /// decides message-only vs. kind+stack-trace rendering, and must be `false` for
    /// exactly the sub-variants that port an unchecked JDK exception rather than a
    /// deliberately-thrown `WalnutException`. Reviewed and corrected after U24's initial
    /// landing, which had a blanket `true` for all four of these enums.
    #[test]
    fn u24_command_errors_classify_by_walnutexception_vs_jdk_exception() {
        // NOT WalnutException -> kind + stack trace.
        for (e, why) in [
            (
                ProverError::Morphism(MorphismCommandError::Promote(
                    MorphismError::DomainDoesNotCoverImageRange,
                )),
                "WB-036 is an IndexOutOfBoundsException",
            ),
            (
                ProverError::Morphism(MorphismCommandError::InvalidFile("x".to_string())),
                "validateFile throws IllegalArgumentException",
            ),
            (
                ProverError::Join(JoinError::NoAutomataSpecified),
                "WB-037 is an IndexOutOfBoundsException",
            ),
            (
                ProverError::Convert(ConvertError::InvalidBase("x".to_string())),
                "Integer.parseInt throws NumberFormatException",
            ),
            (
                ProverError::Convert(ConvertError::Convert(ConvertNsError::NoNumberSystem)),
                "WB-033 is a NullPointerException",
            ),
        ] {
            assert!(!e.is_handled(), "{why}");
        }

        // Genuine WalnutExceptions -> message only.
        for (e, why) in [
            (
                ProverError::Morphism(MorphismCommandError::Promote(
                    MorphismError::NumberSystemNotDefined(1),
                )),
                "\"Number system msd_1 is not defined.\" is a WalnutException",
            ),
            (
                ProverError::Morphism(MorphismCommandError::Promote(MorphismError::NegativeValue)),
                "WalnutException.morphismNegative",
            ),
            (
                ProverError::Join(JoinError::LabelMismatch {
                    automaton_name: "A".to_string(),
                }),
                "Join.java:53's inline WalnutException",
            ),
            (
                ProverError::Join(JoinError::AlphabetMismatch {
                    label: "x".to_string(),
                }),
                "ProductStrategies.java:281's WalnutException",
            ),
            (
                ProverError::Convert(ConvertError::DfaoIntoFunction),
                "WalnutException.convertDFAOIntoFunction",
            ),
            (
                ProverError::Convert(ConvertError::Convert(ConvertNsError::NoCommonRoot)),
                "convertNS's inline WalnutException",
            ),
            (
                ProverError::Image(ImageError::NotUnaryWordAutomaton {
                    name: "w".to_string(),
                }),
                "Image.java:50's inline WalnutException",
            ),
        ] {
            assert!(e.is_handled(), "{why}");
        }
    }

    // ------------------------------------------------------------- parseSetup

    #[test]
    fn the_suffix_decides_print_flag_and_print_details() {
        let (mut p, dir, _) = prover("suffix");
        assert_eq!(p.parse_setup("eval x \"x=1\";").unwrap(), "eval x \"x=1\"");
        assert!(!p.print_flag() && !p.print_details());
        assert_eq!(p.parse_setup("eval x \"x=1\":").unwrap(), "eval x \"x=1\"");
        assert!(p.print_flag() && !p.print_details());
        assert_eq!(p.parse_setup("eval x \"x=1\"::").unwrap(), "eval x \"x=1\"");
        assert!(p.print_flag() && p.print_details());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_command_with_no_terminator_is_an_invalid_command() {
        let (mut p, dir, _) = prover("noterm");
        let err = p.dispatch("eval x \"x=1\"").unwrap_err();
        assert_eq!(err.to_string(), "Invalid command: eval x \"x=1\"");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn metacommands_are_stripped_before_the_command_name_is_read() {
        let (mut p, dir, _) = prover("metastrip");
        let rest = p.parse_setup("[strategy 1 BRZ] eval x \"x=1\"::").unwrap();
        assert_eq!(rest, "eval x \"x=1\"");
        assert_eq!(
            p.meta_commands().get_strategy(1),
            wr_core::determinize::Strategy::Brz
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_otf_strategy_metacommand_is_rejected_through_dispatch() {
        let (mut p, dir, _) = prover("metaotf");
        let err = p
            .dispatch("[strategy 1 CCLS] eval x \"?msd_2 x=1\"::")
            .unwrap_err();
        assert!(matches!(
            err,
            ProverError::Meta(MetaCommandError::OtfStrategyDeferred(_))
        ));
        fs::remove_dir_all(&dir).ok();
    }

    // ---------------------------------------------------------------- dispatch

    #[test]
    fn an_unknown_command_name_is_no_such_command() {
        let (mut p, dir, _) = prover("nosuch");
        let err = p.dispatch("frobnicate x;").unwrap_err();
        assert!(matches!(err, ProverError::NoSuchCommand));
        assert_eq!(err.to_string(), "No such command exists.");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exit_and_quit_stop_the_loop() {
        let (mut p, dir, _) = prover("exit");
        assert!(!p.dispatch("exit;").unwrap());
        assert!(!p.dispatch("quit;").unwrap());
        assert!(p.dispatch("cls;").unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_and_cls_emit_the_ansi_escape() {
        let (mut p, dir, out) = prover("cls");
        p.dispatch("clear;").unwrap();
        assert_eq!(out.text(), "\u{1b}[H\u{1b}[2J");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn eval_runs_end_to_end_through_dispatch() {
        let (mut p, dir, out) = prover("eval");
        assert!(p.dispatch("eval whatever \"?msd_2 Ei i < x\";").unwrap());
        // `EvalDef` prints nothing for a non-trivial result, but it must have written the
        // result files.
        assert!(dir.join("Result").join("whatever.txt").is_file());
        assert!(dir.join("Automata Library").join("whatever.txt").is_file());
        let _ = out.text();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_trivially_true_eval_prints_true() {
        let (mut p, dir, out) = prover("evaltrue");
        assert!(p.dispatch("eval t \"?msd_2 Ex x = 1\";").unwrap());
        assert!(out.text().contains("TRUE"), "{:?}", out.text());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn def_runs_end_to_end_and_is_the_same_arm_as_eval() {
        let (mut p, dir, _) = prover("def");
        assert!(p.dispatch("def lt x y \"?msd_2 x < y\";").unwrap());
        assert!(dir.join("Automata Library").join("lt.txt").is_file());
        assert_eq!(p.current_eval_name(), Some("lt"));
        fs::remove_dir_all(&dir).ok();
    }

    /// `Prover.currentEvalName` is a static that `new MetaCommands()` does NOT reset, so
    /// the name set by an `eval`/`def` is still the export name for every LATER command —
    /// even one that is not an `eval` at all. `parse_setup` rebuilds `MetaCommands` per
    /// command, so it has to re-seed the name or `getExportName` silently falls back to
    /// the `"export"` placeholder.
    #[test]
    fn the_eval_name_survives_into_the_next_commands_meta_commands() {
        let (mut p, dir, _) = prover("evalnamecarry");
        p.dispatch("def myname x \"?msd_2 x = 1\";").unwrap();
        assert_eq!(p.current_eval_name(), Some("myname"));

        // A LATER, unrelated command with an export metacommand. Java would name its dump
        // `myname_0_pre.gv`, not `export_0_pre.gv`.
        let _ = p.dispatch("[export * gv] star S A::");
        assert_eq!(
            p.meta_commands().get_export_name(0).as_deref(),
            Some("myname")
        );

        // And with no `eval`/`def` ever run, the placeholder is what Java uses.
        let (mut fresh, dir2, _) = prover("evalnamefresh");
        let _ = fresh.dispatch("[export * gv] star S A::");
        assert_eq!(
            fresh.meta_commands().get_export_name(0).as_deref(),
            Some(crate::meta_commands::DEFAULT_EXPORT_NAME)
        );
        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&dir2).ok();
    }

    /// `[export …]` end-to-end through real `Prover::dispatch`: the metacommand is
    /// parsed, the parsed `MetaCommands` is threaded into `wr_core::determinize` as the
    /// [`wr_core::determinize::DeterminizeContext`], and Walnut's pre-determinization
    /// dumps (`<name>_<idx>_pre.<fmt>`) actually appear.
    ///
    /// This test began life as a TRIPWIRE pinning the opposite (`[export …]` parsed and
    /// silently discarded, no `_pre` file, counter never moved), with instructions to
    /// rewrite it the day the context was wired through. This is that rewrite; the
    /// counter/file assertions are inverted, not deleted.
    ///
    /// `?msd_2 Ei i < x` performs exactly two non-silent determinizations — the
    /// ∃-projection (`AutomatonQuantification.quantifyHelper`) and its leading-zero fixup
    /// (`AutomatonLogicalOps.fixLeadingZerosProblem`) — so indices `0` and `1` are
    /// consumed and the wildcard `*` export dumps both. Everything `NumberSystem` builds
    /// for `i < x` stays silent (Java brackets it in `Logging.disablePrint()`; this port
    /// hands `wr_core::numsys` no context at all), which is why the count is 2 and not
    /// more.
    #[test]
    fn export_metacommands_write_the_pre_determinization_dumps() {
        let (mut p, dir, _) = prover("exportwired");
        assert!(p
            .dispatch("[export * gv] eval e \"?msd_2 Ei i < x\"::")
            .unwrap());

        // Parsed and accepted...
        assert_eq!(
            p.meta_commands().get_export_format(0).as_deref(),
            Some("gv")
        );
        assert_eq!(p.meta_commands().get_export_name(0).as_deref(), Some("e"));
        assert!(
            p.meta_commands().export_failures().is_empty(),
            "{:?}",
            p.meta_commands().export_failures()
        );

        // ...and the determinizer called back exactly twice, so the NEXT index handed out
        // is 2 (the counter is a post-increment, `MetaCommands.java:27-29`).
        assert_eq!(p.meta_commands_mut().increment_automata_index(), 2);

        // ...and both `_pre` dumps were written, alongside the ordinary result file.
        let mut pre: Vec<String> = fs::read_dir(dir.join("Result"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("_pre"))
            .collect();
        pre.sort();
        assert_eq!(
            pre,
            vec!["e_0_pre.gv".to_string(), "e_1_pre.gv".to_string()]
        );
        assert!(dir.join("Result").join("e.txt").is_file());

        // The same, with a SINGLE index and the `ba` format -- fixture 660's shape
        // (`[export 1 BA]eval ...::`). Only the named index is dumped.
        assert!(p
            .dispatch("[export 1 BA] eval f \"?msd_2 Ei i < x\"::")
            .unwrap());
        assert!(
            p.meta_commands().export_failures().is_empty(),
            "{:?}",
            p.meta_commands().export_failures()
        );
        let mut f_pre: Vec<String> = fs::read_dir(dir.join("Result"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("f_") && n.contains("_pre"))
            .collect();
        f_pre.sort();
        assert_eq!(f_pre, vec!["f_1_pre.ba".to_string()]);
        fs::remove_dir_all(&dir).ok();
    }

    /// **A tripwire, not an endorsement** — the boundary of what the previous test just
    /// established. Only `eval`/`def` hand their `MetaCommands` down as a
    /// [`wr_core::determinize::DeterminizeContext`]; every OTHER command that determinizes
    /// (`reverse`, `union`, `concat`, `star`, `quotient`, `minimize`, …) still passes
    /// `None`, so an `[export …]`/`[strategy …]` on one of them is parsed, validated,
    /// accepted — and silently discarded.
    ///
    /// Real Walnut does write for these: verified live against `Walnut-all.jar`
    /// (2026-08-16), `[export 0 gv]reverse revb $base::` prints
    /// `Writing to …/Result/export_0_pre.gv` (the name is `MetaCommands`'
    /// `DEFAULT_EXPORT_NAME` placeholder, since no `eval` has run to set
    /// `Prover.currentEvalName`) and produces the file. So this test pins a **known,
    /// deliberate scope limitation**, exactly the way the `eval`-side test above pinned
    /// one before it was closed. Wiring the rest is a mechanical arm-by-arm follow-on, not
    /// a design question.
    ///
    /// **INVERT THIS ASSERTION when `reverse`'s arm gets wired** — the failure message
    /// says so too. It is meant to change, and to go red loudly if the behavior drifts in
    /// either direction (a half-wired arm that writes some files, or an accidental hard
    /// error where today the command still succeeds).
    ///
    /// (Java's own behavior on this exact command is worse than "writes a file": the `gv`
    /// writer canonizes the live automaton mid-determinization and the `reverse` then dies
    /// with an `IndexOutOfBoundsException` — `docs/WALNUT-BUGS.md` WB-040. That is a
    /// reason to wire this arm carefully, not a reason to leave it unpinned.)
    #[test]
    fn export_metacommands_on_a_non_eval_command_are_still_accepted_and_discarded() {
        let (mut p, dir, _) = prover("exportnoneval");
        fs::write(
            dir.join("Automata Library").join("base.txt"),
            "msd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 1\n",
        )
        .unwrap();

        // The command still succeeds, and the metacommand still parses/validates.
        assert!(p.dispatch("[export 0 gv]reverse revb $base::").unwrap());
        assert!(
            p.meta_commands().export_failures().is_empty(),
            "{:?}",
            p.meta_commands().export_failures()
        );
        assert_eq!(
            p.meta_commands().get_export_format(0).as_deref(),
            Some("gv")
        );

        // ...and yet the determinizer never called back: no automaton index was consumed
        // (the counter is still at its initial value, `MetaCommands.java:27-29`).
        assert_eq!(p.meta_commands_mut().increment_automata_index(), 0);

        // ...and no `_pre` dump was written, though the ordinary result file was.
        let stray: Vec<String> = fs::read_dir(dir.join("Result"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("_pre"))
            .collect();
        assert!(
            stray.is_empty(),
            "`[export …]` on a non-`eval`/`def` command is still a no-op; if these \
             appeared, that arm has been wired -- INVERT THIS TEST to assert the files \
             (and the consumed indices) that now appear: {stray:?}"
        );
        assert!(dir.join("Automata Library").join("revb.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    /// The `shouldPrintDetails()` gate at `Prover::eval_def_commands`' call site: an
    /// ordinary `eval` with NO metacommand prefix must not move `MetaCommands`' automata
    /// counter at all, because Java only reads the metacommands when
    /// `Logging.shouldPrintDetails()` holds — i.e. when the command ended in `::`.
    ///
    /// Both halves matter and are checked separately: a `;` command (no details) leaves
    /// the counter at 0 even though it performs the same two determinizations, while the
    /// identical `::` command advances it to 2. Get this wrong and every
    /// `[strategy n …]`/`[export n …]` index silently targets a different automaton.
    #[test]
    fn only_a_details_printing_command_moves_the_automata_counter() {
        let (mut p, dir, _) = prover("detailsgate");
        assert!(p.dispatch("eval q1 \"?msd_2 Ei i < x\";").unwrap());
        assert_eq!(
            p.meta_commands_mut().increment_automata_index(),
            0,
            "a `;` command must not consult the metacommands at all"
        );

        assert!(p.dispatch("eval q2 \"?msd_2 Ei i < x\"::").unwrap());
        assert_eq!(
            p.meta_commands_mut().increment_automata_index(),
            2,
            "a `::` command performs the same two determinizations, and counts them"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// `transduce` through real `Prover::dispatch` -- U26's own more detailed coverage
    /// (semantic equivalence, the size guard, WB-034) lives in `crate::transduce`'s own
    /// tests; this just confirms the command is wired to the real handler here, not a
    /// `NotYetImplemented` stub, and that the DFAO write side effect (`writeAutomata(...,
    /// true)`, unlike `reg`/`alphabet`'s `false`) lands in the Word Automata Library.
    #[test]
    fn transduce_runs_end_to_end_through_dispatch() {
        let (mut p, dir, _) = prover("transduce");
        fs::create_dir_all(dir.join("Transducer Library")).unwrap();
        fs::write(
            dir.join("Transducer Library").join("RUNSUM2.txt"),
            "{0, 1}\n\n0\n0 -> 0 / 0\n1 -> 1 / 1\n\n1\n0 -> 1 / 1\n1 -> 0 / 0\n",
        )
        .unwrap();
        fs::write(
            dir.join("Word Automata Library").join("T.txt"),
            "# The Thue-Morse sequence.\nmsd_2\n\n0 0\n0 -> 0\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n",
        )
        .unwrap();

        assert!(p.dispatch("transduce test527 RUNSUM2 T;").unwrap());
        assert!(dir
            .join("Word Automata Library")
            .join("test527.txt")
            .is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reg_runs_end_to_end_through_dispatch() {
        let (mut p, dir, _) = prover("reg");
        assert!(p.dispatch("reg rr msd_2 \"0*1\";").unwrap());
        assert!(dir.join("Automata Library").join("rr.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn alphabet_runs_end_to_end_through_dispatch() {
        let (mut p, dir, _) = prover("alphabet");
        // Build something to re-alphabetize first. `$src` marks it as a predicate
        // automaton (no `$` would send `determineInLibrary` to the WORD library).
        p.dispatch("reg src msd_2 \"0*1\";").unwrap();
        assert!(p.dispatch("alphabet dst msd_3 $src;").unwrap());
        assert!(dir.join("Automata Library").join("dst.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    /// U23, batch A: every one of the 13 dispatch arms (`combine`, `concat`, `union`,
    /// `intersect`, `star`, `reverse`, `rightquo`, `leftquo`, `describe`, `minimize`,
    /// `fixleadzero`, `fixtrailzero`, `macro`) runs REAL logic through
    /// [`Prover::dispatch`] -- the regex capture + command-name lookup + argument
    /// extraction this test exercises is the exact same path `walnut-rs`'s binary and
    /// `read_buffer`/`run` use, not a shortcut that calls the underlying `wr-cli`
    /// command function directly.
    #[test]
    fn u23_batch_a_commands_run_real_logic_through_dispatch() {
        let (mut p, dir, _) = prover("u23-batch-a");

        // Two small `msd_2` predicate automata to fold the rest of this batch over.
        assert!(p.dispatch("reg a msd_2 \"0*1\";").unwrap());
        assert!(p.dispatch("reg b msd_2 \"1\";").unwrap());
        assert!(dir.join("Automata Library").join("a.txt").is_file());
        assert!(dir.join("Automata Library").join("b.txt").is_file());

        assert!(p.dispatch("union u a b;").unwrap());
        assert!(dir.join("Automata Library").join("u.txt").is_file());

        assert!(p.dispatch("intersect i a b;").unwrap());
        assert!(dir.join("Automata Library").join("i.txt").is_file());

        assert!(p.dispatch("concat cc a b;").unwrap());
        assert!(dir.join("Automata Library").join("cc.txt").is_file());

        assert!(p.dispatch("star st a;").unwrap());
        assert!(dir.join("Automata Library").join("st.txt").is_file());

        // `combine` writes into the WORD library (it always produces a DFAO).
        assert!(p.dispatch("combine co a=1 b=2;").unwrap());
        assert!(dir.join("Word Automata Library").join("co.txt").is_file());

        // `minimize` reads/writes the WORD library too.
        assert!(p.dispatch("minimize mn co;").unwrap());
        assert!(dir.join("Word Automata Library").join("mn.txt").is_file());

        assert!(p.dispatch("reverse r $a;").unwrap());
        assert!(dir.join("Automata Library").join("r.txt").is_file());

        assert!(p.dispatch("rightquo rq a b;").unwrap());
        assert!(dir.join("Automata Library").join("rq.txt").is_file());

        assert!(p.dispatch("leftquo lq a b;").unwrap());
        assert!(dir.join("Automata Library").join("lq.txt").is_file());

        assert!(p.dispatch("fixleadzero fl a;").unwrap());
        assert!(dir.join("Automata Library").join("fl.txt").is_file());

        assert!(p.dispatch("fixtrailzero ft a;").unwrap());
        assert!(dir.join("Automata Library").join("ft.txt").is_file());

        // `describe` writes nothing; just confirm it runs to completion (not
        // `NotYetImplemented`) and produces command-log detail.
        assert!(p.dispatch("describe $a;").unwrap());

        assert!(p.dispatch(r#"macro mm "x = 1";"#).unwrap());
        let macro_text = fs::read_to_string(dir.join("Macro Library").join("mm.txt")).unwrap();
        assert_eq!(macro_text, "x = 1");

        fs::remove_dir_all(&dir).ok();
    }

    /// U23 review fix, finding #6. The batch-A test above asserts only "the command
    /// succeeded and wrote a file" for most arms, which a swapped-operand port bug would
    /// sail straight through — `concat`, `rightquo` and `leftquo` are all ASYMMETRIC in
    /// their two automaton arguments and still produce a perfectly valid output file when
    /// the operands are exchanged. This pins the LANGUAGE of each, in both orders,
    /// through real dispatch.
    #[test]
    fn asymmetric_batch_a_commands_respect_their_operand_order() {
        let (mut p, dir, _) = prover("u23-operand-order");
        // L(a) = 0*1 = {1, 01, 001, ...}; L(b) = {1}; L(z) = {0}.
        assert!(p.dispatch("reg a msd_2 \"0*1\";").unwrap());
        assert!(p.dispatch("reg b msd_2 \"1\";").unwrap());
        assert!(p.dispatch("reg z msd_2 \"0\";").unwrap());

        let language_of = |name: &str| {
            wr_io::reader::read_automaton_txt(
                dir.join("Automata Library").join(format!("{name}.txt")),
            )
            .unwrap()
        };

        // --- concat. `L(a)·L(b)` contains "011" (= "01" then "1"); `L(b)·L(a)` cannot —
        // everything in it starts with a `1`. (Both also carry WB-009's leak of the first
        // operand's own language, which contains no "011" either way.)
        assert!(p.dispatch("concat ab a b;").unwrap());
        assert!(p.dispatch("concat ba b a;").unwrap());
        assert!(
            language_of("ab").fa.accepts_word(&[0, 1, 1]),
            "concat a b must accept \"011\""
        );
        assert!(
            !language_of("ba").fa.accepts_word(&[0, 1, 1]),
            "concat b a must NOT accept \"011\" -- operand order is load-bearing"
        );

        // --- rightquo. `{z : z·w ∈ L(a) for some w ∈ L(b)}` = `{z : z1 ∈ 0*1}` = `0*`,
        // which contains "0". The swap is `{z : z·w ∈ L(b)={1}}` = `{ε}`, which does not.
        assert!(p.dispatch("rightquo rab a b;").unwrap());
        assert!(p.dispatch("rightquo rba b a;").unwrap());
        assert!(
            language_of("rab").fa.accepts_word(&[0]),
            "rightquo a b must accept \"0\""
        );
        assert!(
            !language_of("rba").fa.accepts_word(&[0]),
            "rightquo b a must NOT accept \"0\""
        );

        // --- leftquo. `{w : u·w ∈ L(a) for some u ∈ L(z)={0}}` = `{w : 0w ∈ 0*1}`, which
        // contains "1". The swap needs some u ∈ L(a) to be a prefix of "0" — none is — so
        // it is empty.
        assert!(p.dispatch("leftquo laz a z;").unwrap());
        assert!(p.dispatch("leftquo lza z a;").unwrap());
        assert!(
            language_of("laz").fa.accepts_word(&[1]),
            "leftquo a z must accept \"1\""
        );
        assert!(
            !language_of("lza").fa.accepts_word(&[1]),
            "leftquo z a must NOT accept \"1\""
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// Exercises the `{…}`-set branch of the shared alphabet-list sub-pattern, including
    /// its escaped `\-`/`\+` signs, through real dispatch.
    #[test]
    fn a_literal_set_alphabet_dispatches() {
        let (mut p, dir, _) = prover("setalpha");
        assert!(p.dispatch("reg s1 {0,1} \"0*1\";").unwrap());
        assert!(p.dispatch("reg s2 {-1,+1} \"(-1)*\";").unwrap());
        assert!(dir.join("Automata Library").join("s1.txt").is_file());
        assert!(dir.join("Automata Library").join("s2.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    // ---------------------------------------------------------------- U24: batch B

    /// The task's own required end-to-end case: a genuine Thue-Morse-shaped morphism,
    /// through the real command chain `morphism -> promote -> image` (verified against
    /// the real `Main/Commands/Morphism.java`/`Prover.promoteCommand`/`Image.java`
    /// call graph, not assumed — `promote`'s morphism-to-word-automaton construction
    /// is exactly the classical Thue-Morse-generating DFAO, see
    /// `wr_core::morphism::Morphism::to_word_automaton`'s own
    /// `to_word_automaton_thue_morse_shape` test).
    #[test]
    fn morphism_promote_image_thue_morse_chain_runs_end_to_end_through_dispatch() {
        let (mut p, dir, capture) = prover("thue-morse-chain");

        assert!(p.dispatch(r#"morphism h "0->01,1->10";"#).unwrap());
        assert_eq!(
            capture.text(),
            "Defined with domain [0, 1] and range {0, 1}"
        );
        assert!(dir.join("Morphism Library").join("h.txt").is_file());
        assert!(dir.join("Result").join("h.txt").is_file());

        // `T` is exactly the Thue-Morse-sequence-generating DFAO.
        assert!(p.dispatch("promote T h;").unwrap());
        assert!(dir.join("Word Automata Library").join("T.txt").is_file());
        assert!(dir.join("Result").join("T.txt").is_file());

        // The classical Thue-Morse identity h(T) = T -- `image` applies `h` to `T`.
        assert!(p.dispatch("image FS h T;").unwrap());
        assert!(dir.join("Word Automata Library").join("FS.txt").is_file());
        assert!(dir.join("Result").join("FS.txt").is_file());

        // ...and it really IS `T` again, not merely "some non-empty automaton": the
        // written image is byte-identical to the promoted `T` it was computed from.
        // (Both are the minimal 2-state msd_2 Thue-Morse DFAO, and both go through the
        // same writer, so byte equality is the right bar here -- if a future change
        // makes state numbering legitimately differ, weaken this to `wr_core::equiv`,
        // do NOT drop it back to a file-existence check.)
        let t_txt = fs::read_to_string(dir.join("Word Automata Library").join("T.txt")).unwrap();
        let fs_txt = fs::read_to_string(dir.join("Word Automata Library").join("FS.txt")).unwrap();
        assert_eq!(fs_txt, t_txt, "h(T) must be T (the Thue-Morse identity)");
        assert!(t_txt.starts_with("msd_2"), "unexpected header: {t_txt:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn join_runs_end_to_end_through_dispatch() {
        let (mut p, dir, _) = prover("join-e2e");

        // A word automaton (DFAO): Thue-Morse via `promote`.
        assert!(p.dispatch(r#"morphism h "0->01,1->10";"#).unwrap());
        assert!(p.dispatch("promote T h;").unwrap());
        // A plain automaton via `reg`, in a DIFFERENT track variable.
        assert!(p.dispatch("reg S msd_2 \"0*1\";").unwrap());

        assert!(p.dispatch("join J T[x] S[y];").unwrap());
        // Any sub-automaton from the Word Automata Library makes the join's own output
        // a DFAO too (`crate::join`'s own doc comment).
        assert!(dir.join("Word Automata Library").join("J.txt").is_file());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn convert_runs_end_to_end_through_dispatch() {
        let (mut p, dir, _) = prover("convert-e2e");
        assert!(p.dispatch("reg S msd_2 \"0*1\";").unwrap());
        // No `$` before `C` -> newIsDFAO -> Word Automata Library; `$S` -> oldIsDFAO ==
        // false -> read from the Automata Library, where `reg` wrote `S`.
        assert!(p.dispatch("convert C msd_4 $S;").unwrap());
        assert!(dir.join("Word Automata Library").join("C.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inf_runs_end_to_end_through_dispatch() {
        let (mut p, dir, capture) = prover("inf-e2e");
        // Every binary string starting with `1` (no leading zero): infinitely many
        // DISTINCT represented values, so this survives `infFromAddress`'s own
        // `removeLeadingZeros` step (unlike e.g. `0*1`, whose every accepted word
        // represents the single value 1).
        assert!(p.dispatch("reg S msd_2 \"1(0|1)*\";").unwrap());
        assert!(p.dispatch("inf S;").unwrap());
        assert!(
            capture
                .text()
                .starts_with("Automaton accepts infinite values, including regex:"),
            "{}",
            capture.text()
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_runs_end_to_end_through_dispatch() {
        let (mut p, dir, capture) = prover("export-e2e");
        assert!(p.dispatch("reg S msd_2 \"0*1\";").unwrap());
        assert!(p.dispatch("export $S gv;").unwrap());
        assert!(dir.join("Result").join("S.gv").is_file());
        assert!(capture.text().starts_with("Writing to "));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_runs_end_to_end_through_dispatch_reporting_shortfall_then_success() {
        let (mut p, dir, capture) = prover("test-dispatch");
        // `FINITE_TWO_WORD_AUTOMATON` from `TestTest.java`: language exactly `{"0", "1"}`.
        fs::write(
            dir.join("Automata Library").join("finiteTwoWord.txt"),
            "{0,1}\n\n0 0\n0 -> 1\n1 -> 1\n\n1 1\n",
        )
        .unwrap();

        // `dispatch`'s own `bool` is the REPL "keep going" signal (`!(EXIT|QUIT)`), NOT
        // `Test.testCommand`'s shortfall/success verdict -- Java's `processCommand` switch
        // arm (`case TEST -> { testCommand(s); }`) discards that verdict too (no `return`),
        // so both calls below report `Ok(true)` regardless of how many inputs were found;
        // the observable behavior is the printed console output.
        //
        // Only 2 inputs are accepted; asking for 5 reports the shortfall
        // (`TestTest.testTestCommandReportsShortfallAndSuccess`'s first half).
        assert!(p.dispatch("test finiteTwoWord 5;").unwrap());
        assert_eq!(
            capture.text(),
            "finiteTwoWord only accepts 2 inputs, which are as follows: \n0\n1\n"
        );

        // Asking for no more than what's accepted prints no shortfall message (the second
        // half of the same Java test); `capture` keeps accumulating, so the first call's
        // output is still the prefix.
        assert!(p.dispatch("test finiteTwoWord 1;").unwrap());
        assert_eq!(
            capture.text(),
            "finiteTwoWord only accepts 2 inputs, which are as follows: \n0\n1\n0\n"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_reports_walnuts_error_on_a_bad_needed_count() {
        // The `\d+` capture group can still overflow `i32` -- `Integer.parseInt`'s
        // `NumberFormatException`, propagated (not a `WalnutException`).
        let (mut p, dir, _) = prover("test-number-format");
        let err = p
            .dispatch("test whatever 99999999999999999999;")
            .unwrap_err();
        assert!(matches!(err, ProverError::NumberFormat(_)));
        assert_eq!(
            err.to_string(),
            "For input string: \"99999999999999999999\""
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_malformed_invocation_of_an_unimplemented_command_still_reports_walnuts_error() {
        let (mut p, dir, _) = prover("malformed");
        // `union` needs at least a name; the regex fails before the stub is reached.
        let err = p.dispatch("union;").unwrap_err();
        assert_eq!(err.to_string(), "Invalid use of the union command.");
        // `rsplit` reports itself as "reverse split" (Java's `REVERSE_SPLIT`).
        let err = p.dispatch("rsplit;").unwrap_err();
        assert_eq!(err.to_string(), "Invalid use of the reverse split command.");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_unimplemented_command_has_an_arm_that_reports_its_owning_unit() {
        let (mut p, dir, _) = prover("stubs");
        // `convert`/`export`/`image`/`inf`/`join`/`morphism`/`promote` are U24's own
        // batch (implemented as of this unit, see `every_command_pattern_pins_its_group_indices`'s
        // siblings and each command's own `dispatch`-level test below) and are
        // therefore no longer in this list.
        let cases = [
            ("help;", "U22"),
            ("rsplit S [+] T;", "U24"),
            ("split S T [+];", "U24"),
        ];
        for (command, unit) in cases {
            let err = p.dispatch(command).unwrap_err();
            match err {
                ProverError::NotYetImplemented { unit: u, .. } => {
                    assert_eq!(u, unit, "{command}")
                }
                other => panic!("{command}: expected NotYetImplemented, got {other}"),
            }
        }
        fs::remove_dir_all(&dir).ok();
    }

    /// `ost` used to be the one `UnsupportedCommand` arm; it now really runs. The two
    /// `Custom Bases/` files it writes are compared byte-for-byte against a fresh capture
    /// of the real `walnut-java` CLI on the SAME command
    /// (`tests/differential/fixtures/ostrowski/`, see `tests/differential/CAPTURE.md`) —
    /// note `ost o [1 2] [3];` exercises the `preperiod[0] == 1` rotation branch
    /// (`Ostrowski.java:105-107`), which golden fixture 625 (`[0 3 1] [1 2]`) does not.
    #[test]
    fn ost_creates_a_custom_base_and_writes_both_files() {
        let (mut p, dir, capture) = prover("ost");
        assert!(p.dispatch("ost o [1 2] [3];").unwrap());

        let bases = dir.join("Custom Bases");
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/differential/fixtures/ostrowski");
        for name in ["msd_o.txt", "msd_o_addition.txt"] {
            assert_eq!(
                fs::read_to_string(bases.join(name)).unwrap(),
                fs::read_to_string(fixtures.join(name)).unwrap(),
                "{name} differs from the real walnut-java capture"
            );
        }
        // `Ostrowski.writeAutomaton`'s two `Writing to: …` lines reach the command's
        // stdout sink, not the log.
        let printed = capture.text();
        assert_eq!(printed.matches("Writing to: ").count(), 2, "{printed:?}");

        fs::remove_dir_all(&dir).ok();
    }

    /// A second `ost` under the same name is `Ostrowski.writeAutomaton`'s already-exists
    /// `WalnutException`, reported through dispatch — one command lost, not the session.
    #[test]
    fn a_repeated_ost_name_is_a_reported_error_not_a_crash() {
        let (mut p, dir, _) = prover("ost-twice");
        assert!(p.dispatch("ost o [1 2] [3];").unwrap());
        let err = p.dispatch("ost o [1 2] [3];").unwrap_err();
        assert_eq!(err.to_string(), "Error: number system o already exists.");
        assert!(err.is_handled(), "a WalnutException renders message-only");
        // The session survives, exactly as `readBuffer`'s catch leaves it.
        assert!(p.dispatch("ost o2 [1 2] [3];").unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    /// The freshly-created base is immediately usable by a later command, which is the
    /// whole point of `ost` — the answer is compared against the real `walnut-java`
    /// output for the same two commands.
    #[test]
    fn an_ost_created_base_is_usable_by_a_later_eval() {
        let (mut p, dir, _) = prover("ost-eval");
        assert!(p.dispatch("ost o [1 2] [3];").unwrap());
        assert!(p.dispatch("eval ostq1 \"?msd_o Ex x+x=y\";").unwrap());

        let ours = wr_io::reader::read_automaton_txt_with_custom_base_resolver(
            dir.join("Automata Library/ostq1.txt"),
            p.session.paths(),
        )
        .expect("the result automaton reads back");
        let theirs = wr_io::reader::read_automaton_txt_with_custom_base_resolver(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/differential/fixtures/ostrowski/ostq1.txt"),
            p.session.paths(),
        )
        .expect("the captured walnut-java automaton reads back");
        let (mut ours, mut theirs) = (ours, theirs);
        ours.sort_label();
        ours.fa.totalize(0);
        theirs.fa.totalize(0);
        assert!(
            wr_core::equiv::automaton_language_equivalent(&ours, &theirs).unwrap(),
            "?msd_o Ex x+x=y differs from real walnut-java"
        );
        fs::remove_dir_all(&dir).ok();
    }

    // -------------------------------------------------------------------- load

    #[test]
    fn load_is_special_cased_before_the_switch_and_runs_the_file() {
        let (mut p, dir, _) = prover("load");
        fs::write(
            dir.join("Command Files").join("script.txt"),
            "reg fromfile msd_2 \"0*1\";\n",
        )
        .unwrap();

        assert!(p.dispatch("load script.txt;").unwrap());
        assert!(dir.join("Automata Library").join("fromfile.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_loaded_file_containing_exit_stops_the_outer_loop() {
        let (mut p, dir, _) = prover("loadexit");
        fs::write(dir.join("Command Files").join("bye.txt"), "exit;\n").unwrap();
        // `dispatch` returns `loadCommand`'s value verbatim -- the `load` special case.
        assert!(!p.dispatch("load bye.txt;").unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loading_a_missing_file_is_an_invalid_file_error() {
        let (mut p, dir, _) = prover("loadmissing");
        let err = p.dispatch("load nope.txt;").unwrap_err();
        assert!(matches!(err, ProverError::InvalidFile(_)));
        assert!(err
            .to_string()
            .contains("File does not exist or is not a valid file"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_with_a_bad_argument_reports_invalid_use() {
        let (mut p, dir, _) = prover("loadbad");
        // `.p` is not `.txt`, so the pattern fails.
        let err = p.dispatch("load script.p;").unwrap_err();
        assert_eq!(err.to_string(), "Invalid use of the load command.");
        fs::remove_dir_all(&dir).ok();
    }

    // -------------------------------------------------------------- readBuffer

    #[test]
    fn read_buffer_echoes_comments_and_concatenates_continuation_lines() {
        let (mut p, dir, out) = prover("buffer");
        let script = "# a comment\nreg spread msd_2\n \"0*1\";\n";
        let mut input = io::Cursor::new(script.as_bytes().to_vec());
        assert!(p.read_buffer(&mut input, false));

        let text = out.text();
        assert!(text.contains("# a comment"), "{text}");
        // Lines are concatenated with NO separator (Java's `buffer.append(s)`), so the
        // command that actually ran is `reg spread msd_2"0*1";` -- which does NOT parse.
        // The failure is logged, not returned; what matters here is the echo.
        assert!(text.contains("reg spread msd_2"), "{text}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_buffer_returns_false_on_exit_and_stops_reading() {
        let (mut p, dir, _) = prover("bufferexit");
        let mut input = io::Cursor::new(b"exit;\nreg after msd_2 \"0*1\";\n".to_vec());
        assert!(!p.read_buffer(&mut input, false));
        assert!(!dir.join("Automata Library").join("after.txt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_buffer_logs_a_failing_command_and_keeps_going() {
        let (mut p, dir, _) = prover("bufferfail");
        let mut input = io::Cursor::new(b"frobnicate;\nreg survived msd_2 \"0*1\";\n".to_vec());
        assert!(p.read_buffer(&mut input, true));
        assert!(dir.join("Automata Library").join("survived.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_console_prompt_is_printed_only_in_console_mode() {
        let (mut p, dir, out) = prover("prompt");
        let mut input = io::Cursor::new(b"exit;\n".to_vec());
        p.read_buffer(&mut input, true);
        assert!(out.text().contains(PROMPT), "{:?}", out.text());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_prints_the_welcome_banner_after_the_command_file() {
        let (mut p, dir, out) = prover("run");
        fs::write(
            dir.join("Command Files").join("s.txt"),
            "reg banner msd_2 \"0*1\";\n",
        )
        .unwrap();
        let mut console = io::Cursor::new(Vec::new());
        p.run_with_input(Some("s.txt"), &mut console).unwrap();

        let text = out.text();
        assert!(text.contains("Welcome to Walnut v"), "{text}");
        assert!(text.contains("Starting Walnut session: "), "{text}");
        assert!(dir.join("Automata Library").join("banner.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_skips_the_banner_when_the_command_file_exits() {
        let (mut p, dir, out) = prover("runexit");
        fs::write(dir.join("Command Files").join("s.txt"), "exit;\n").unwrap();
        let mut console = io::Cursor::new(Vec::new());
        p.run_with_input(Some("s.txt"), &mut console).unwrap();
        assert!(!out.text().contains("Welcome to Walnut"));
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------- dispatchForIntegrationTest

    #[test]
    fn dispatch_for_integration_test_returns_the_test_case() {
        let (mut p, dir, _) = prover("integration");
        let tc = p
            .dispatch_for_integration_test("reg it msd_2 \"0*1\";", "ignored")
            .unwrap()
            .expect("reg returns a TestCase");
        assert_eq!(tc.automaton_pairs().len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_for_integration_test_skips_comments() {
        let (mut p, dir, _) = prover("integrationcomment");
        assert!(p
            .dispatch_for_integration_test("#nothing;", "ignored")
            .unwrap()
            .is_none());
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------- parse_args

    #[test]
    fn parse_args_prints_usage_for_help() {
        assert!(matches!(
            parse_args(&["--help".to_string()]).unwrap(),
            ArgsOutcome::Help
        ));
        assert!(matches!(
            parse_args(&["-h".to_string()]).unwrap(),
            ArgsOutcome::Help
        ));
        assert!(USAGE_MESSAGE.starts_with("Usage: walnut"));
    }

    #[test]
    fn parse_args_builds_a_session_and_creates_its_subdirectories() {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-prover-args-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let dir_str = format!("{}/", dir.to_str().unwrap());

        let outcome = parse_args(&[
            format!("--home-dir={dir_str}"),
            GLOBAL_SESSION_ARG.to_string(),
        ])
        .unwrap();
        match outcome {
            ArgsOutcome::Run { filename, session } => {
                assert!(filename.is_none());
                assert!(session.paths().is_global_session());
            }
            ArgsOutcome::Help => panic!("expected Run"),
        }
        assert!(dir.join("Result").is_dir());
        assert!(dir.join("Automata Library").is_dir());
        fs::remove_dir_all(&dir).ok();
    }

    /// WB-026, pinned: the command file is validated against `""` + `Command Files/`,
    /// i.e. the CURRENT WORKING DIRECTORY, before `--home-dir=` is applied — so a file
    /// that genuinely exists under the home tree is rejected.
    #[test]
    fn run_command_file_validation_ignores_home_dir_wb_026() {
        let (dir, dir_str) = temp_tree("wb026");
        fs::write(dir.join("Command Files").join("probe.txt"), "exit;\n").unwrap();

        let err =
            parse_args(&[format!("--home-dir={dir_str}"), "probe.txt".to_string()]).unwrap_err();
        match &err {
            ProverError::InvalidFile(m) => assert_eq!(
                m, "File does not exist or is not a valid file: Command Files/probe.txt",
                "the reported path must have NO home-dir prefix -- that is the bug"
            ),
            other => panic!("expected InvalidFile, got {other}"),
        }
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------- `reverse` flips the number system

    /// End-to-end pin for `wr_core::logicalops::flip_ns`'s name flip, at the two layers
    /// where the stale name was actually observable.
    ///
    /// Every expectation below was captured from the real `Walnut-all.jar` on exactly
    /// these commands:
    ///
    /// ```text
    /// reg ok msd_2 "0*1";  reg two msd_2 "1(0|1)*";
    /// reverse rv $ok;      -> rv.txt headed `lsd_2` (walnut-rs used to write `msd_2`)
    /// reverse rv2 $rv;     -> rv2.txt headed `msd_2` (the round trip)
    /// union mixed rv two;  -> `Automata must have the same number system(s).`, no file
    /// union okrv2 rv2 two; -> succeeds (both `msd_2` again)
    /// ```
    #[test]
    fn reverse_flips_the_number_system_name_it_writes_and_the_mismatch_guard_sees_it() {
        let (mut p, dir, _) = prover("reverse-flips-ns");
        let lib = dir.join("Automata Library");
        assert!(p.dispatch("reg ok msd_2 \"0*1\";").unwrap());
        assert!(p.dispatch("reg two msd_2 \"1(0|1)*\";").unwrap());

        assert!(p.dispatch("reverse rv $ok;").unwrap());
        let rv = fs::read_to_string(lib.join("rv.txt")).unwrap();
        assert_eq!(
            rv.lines().next(),
            Some("lsd_2"),
            "reversing an msd automaton makes it lsd -- the whole point"
        );

        // Double reversal round-trips. This ALSO passed with the bug (two stale flips
        // cancel), so it is here to keep the real fix from breaking it, not as the
        // detector.
        assert!(p.dispatch("reverse rv2 $rv;").unwrap());
        let rv2 = fs::read_to_string(lib.join("rv2.txt")).unwrap();
        assert_eq!(rv2.lines().next(), Some("msd_2"));

        // The follow-on that made the stale name a silently-WRONG-ANSWER bug rather than
        // a cosmetic one: `union`'s guard compares number systems by NAME.
        let err = p.dispatch("union mixed rv two;").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Automata must have the same number system(s).",
            "an lsd operand and an msd one must be refused"
        );
        assert!(
            !lib.join("mixed.txt").exists(),
            "a refused union must write nothing"
        );

        // ...and a genuinely matching pair still goes through, so the guard is not just
        // rejecting everything.
        assert!(p.dispatch("union okrv2 rv2 two;").unwrap());
        assert!(lib.join("okrv2.txt").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------- the panic-recovery boundary

    /// WB-038's blast radius, pinned at the layer that fixes it.
    ///
    /// `bad.txt` is Java's own accepted-but-corrupt shape: a body digit outside the
    /// header's alphabet, which `Automaton::encode_index_of` faithfully stores under the
    /// key `-1` (real Walnut does exactly this — see that method's docs). That key then
    /// reaches raw `[sym as usize]` indexing in `wr_core::product`, `wr_core::automaton`,
    /// `wr_core::quantify` and `wr_core::word_automaton`, and `decode(-1)` in
    /// `wr_io::writer`/`Automaton::rebuild_transitions_for_new_alphabet`. Every one of
    /// those was a process-killing panic before [`Prover::caught`]; in real Walnut every
    /// one is a `RuntimeException` that `Prover.readBuffer` catches, printing it and
    /// running the NEXT command in the same session (verified live on `Walnut-all.jar`:
    /// `eval f2b "?lsd_2 $fy(x)";` prints `java.lang.IndexOutOfBoundsException: Index -1
    /// out of bounds for length 2`, then the following `eval` still evaluates).
    ///
    /// So one half of the assertion is *no panic escapes, and the session is still usable
    /// afterwards*.
    ///
    /// # The other half: each command's OUTCOME, captured from the real jar
    ///
    /// The first version of this test asserted only "a LATER command still works", which
    /// is not enough: it cannot tell a command that correctly errors from one that
    /// wrongly errors, nor a correct success from a silent wrong success. Two real
    /// regressions hid behind exactly that gap (a cross-product bounds check hoisted into
    /// the wrong loop, which made `intersect` reject a file real Walnut processes; and a
    /// missing `normalizeNumberSystemToken` branch). So every row below carries the
    /// outcome the real `Walnut-all.jar` produces on the same corrupt inputs — measured,
    /// not guessed, both per-command in a fresh session and in one sequential session
    /// whose surviving `Automata Library`/`Word Automata Library` contents were listed:
    /// `cc, dv, ev, fl, i, lq, ok, rv, st` and `rvw`, and nothing else.
    ///
    /// The `Ok` rows are the load-bearing ones: `intersect`/`concat`/`star`/`leftquo` all
    /// go through the `and`-family product, which does NOT totalize its operands, so the
    /// inner transition set is empty and the corrupt key is never used as an index. See
    /// `wr_core::product::cross_product_internal`'s docs.
    #[test]
    fn a_corrupt_library_file_costs_one_command_not_the_process() {
        /// Where a command's output lands, when it produces one.
        enum Out {
            /// `Automata Library/<name>.txt` must exist afterwards.
            Automata(&'static str),
            /// `Word Automata Library/<name>.txt` must exist afterwards.
            Word(&'static str),
            /// The command names an output that must NOT have been written.
            NotWritten(&'static str),
            /// A read-only command (`inf`/`test`/`describe`) — nothing to check.
            None,
        }
        use Out::*;

        let (mut p, dir, _) = prover("panic-boundary");
        let lib = dir.join("Automata Library");
        let word_lib = dir.join("Word Automata Library");
        // Out-of-alphabet digit `5` under `msd_2`, with a DECLARED destination -- the
        // sub-case real Walnut loads without complaint.
        fs::write(
            lib.join("bad.txt"),
            "msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n",
        )
        .unwrap();
        // A well-formed partner for the binary commands.
        assert!(p.dispatch("reg ok msd_2 \"0*1\";").unwrap());
        // A corrupt WORD automaton for the `reverse $...`/`minimize` (DFAO) paths.
        fs::write(
            word_lib.join("badw.txt"),
            "msd_2\n0 0\n0 -> 0\n1 -> 1\n1 2\n0 -> 0\n5 -> 1\n",
        )
        .unwrap();

        // (command, does real Walnut succeed?, where its output goes)
        let expected: &[(&str, bool, Out)] = &[
            ("union u bad ok;", false, NotWritten("u")),
            ("intersect i bad ok;", true, Automata("i")),
            ("join j bad[x] ok[x];", false, NotWritten("j")),
            ("concat cc bad ok;", true, Automata("cc")),
            ("star st bad;", true, Automata("st")),
            ("rightquo rq bad ok;", false, NotWritten("rq")),
            ("leftquo lq bad ok;", true, Automata("lq")),
            ("reverse rv $bad;", true, Automata("rv")),
            ("reverse rvw badw;", true, Word("rvw")),
            ("minimize mw badw;", false, NotWritten("mw")),
            ("fixleadzero fl bad;", true, Automata("fl")),
            ("fixtrailzero ft bad;", false, NotWritten("ft")),
            ("combine cb bad=1;", false, NotWritten("cb")),
            ("alphabet al msd_3 $bad;", false, NotWritten("al")),
            ("inf bad;", false, None),
            ("test bad 3;", false, None),
            ("describe $bad;", true, None),
            ("eval ev \"?msd_2 Ex $bad(x)\";", true, Automata("ev")),
            ("def dv x \"?msd_2 $bad(x)\";", true, Automata("dv")),
            ("[export * gv] union u2 bad ok::", false, NotWritten("u2")),
        ];

        for (command, java_succeeds, out) in expected {
            // The point of the whole panic fix: whatever this command does, it RETURNS.
            let outcome = p.dispatch(command);
            if let Err(e) = &outcome {
                // Never an I/O-class error -- `readBuffer` must keep reading (see
                // `is_io_class_error`'s `Thrown` arm).
                assert!(!is_io_class_error(e), "{command}: {e}");
            }
            // ...and it agrees with the real jar about WHETHER it worked.
            assert_eq!(
                outcome.is_ok(),
                *java_succeeds,
                "`{command}`: real Walnut {}, this port {} ({:?})",
                if *java_succeeds { "succeeds" } else { "errors" },
                if outcome.is_ok() {
                    "succeeded"
                } else {
                    "errored"
                },
                outcome.as_ref().err().map(ToString::to_string),
            );
            match out {
                Automata(name) => assert!(
                    lib.join(format!("{name}.txt")).is_file(),
                    "`{command}` must write Automata Library/{name}.txt"
                ),
                Word(name) => assert!(
                    word_lib.join(format!("{name}.txt")).is_file(),
                    "`{command}` must write Word Automata Library/{name}.txt"
                ),
                NotWritten(name) => {
                    let f = format!("{name}.txt");
                    assert!(
                        !lib.join(&f).exists() && !word_lib.join(&f).exists(),
                        "`{command}` failed, so it must have written nothing"
                    );
                }
                None => {}
            }
            // ... and the session is still alive: an ordinary command still works.
            assert!(
                p.dispatch("reg alive msd_2 \"1*\";").unwrap(),
                "session died after `{command}`"
            );
            assert!(lib.join("alive.txt").is_file(), "after `{command}`");
            fs::remove_file(lib.join("alive.txt")).unwrap();
        }
        fs::remove_dir_all(&dir).ok();
    }

    /// The same shape driven through [`Prover::read_buffer`] — i.e. through the exact
    /// loop `Prover.readBuffer` is, with its `catch (RuntimeException)`. Java's demoed
    /// behavior is "the command file keeps running"; this pins that the LAST line of the
    /// file still executes after an earlier line blew up.
    #[test]
    fn read_buffer_survives_a_command_that_panics_and_runs_the_next_one() {
        let (mut p, dir, out) = prover("panic-readbuffer");
        fs::write(
            dir.join("Automata Library").join("bad.txt"),
            " lsd_2\n0 1\n20 -> 0\n",
        )
        .unwrap();
        let script = "reg ok lsd_2 \"0*1\";\nunion u bad ok;\nreg after lsd_2 \"1*\";\n";
        let mut input = io::Cursor::new(script.as_bytes().to_vec());

        assert!(
            p.read_buffer(&mut input, false),
            "the loop must run to end-of-input, not abort"
        );
        assert!(
            dir.join("Automata Library").join("after.txt").is_file(),
            "the command AFTER the failing one must still have run: {}",
            out.text()
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// `Prover::caught` itself: a panic raised under dispatch becomes
    /// [`ProverError::Thrown`] carrying the guard's message verbatim, and a successful
    /// command is passed through untouched.
    #[test]
    fn caught_maps_a_panic_to_thrown_and_leaves_success_alone() {
        let caught = wr_core::walnut_panic::catch_walnut_panic_detailed::<bool>(|| {
            panic!("Second A's alphabet must be a subset")
        })
        .unwrap_err();
        assert!(matches!(
            Prover::caught::<bool>(Err(caught)),
            Err(ProverError::Thrown { message: m, location: Some(_) })
                if m == "Second A's alphabet must be a subset"
        ));
        assert!(matches!(Prover::caught(Ok(Ok(true))), Ok(true)));
        assert!(matches!(
            Prover::caught::<bool>(Ok(Err(ProverError::NoSuchCommand))),
            Err(ProverError::NoSuchCommand)
        ));
        // Message-only (a `WalnutException`-shaped report), never IO-class.
        let thrown = ProverError::Thrown {
            message: "boom".to_string(),
            location: Some("crates/wr-core/src/product.rs:294:21".to_string()),
        };
        assert!(thrown.is_handled());
        assert!(!is_io_class_error(&thrown));
        // The location is CARRIED but never rendered -- Java has no such text.
        assert_eq!(thrown.to_string(), "boom");
        assert!(format!("{thrown:?}").contains("product.rs:294:21"));
    }

    /// The point of carrying the location at all: a real guard panic recovered through
    /// the dispatch boundary names the site it came from.
    #[test]
    fn a_recovered_panic_records_where_it_was_raised() {
        let (mut p, dir, _) = prover("thrown-location");
        fs::write(
            dir.join("Automata Library").join("bad.txt"),
            "msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 0\n5 -> 1\n",
        )
        .unwrap();
        assert!(p.dispatch("reg ok msd_2 \"0*1\";").unwrap());
        match p.dispatch("union u bad ok;") {
            Err(ProverError::Thrown { message, location }) => {
                assert_eq!(message, "Index -2 out of bounds for length 4");
                let location = location.expect("the site must be recorded");
                assert!(location.contains("product.rs"), "{location}");
            }
            other => panic!("expected a recovered guard panic, got {other:?}"),
        }
        fs::remove_dir_all(&dir).ok();
    }
}
