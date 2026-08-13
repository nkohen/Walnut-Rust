// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/MetaCommands.java` (124 LOC) — the `[...]`-bracket metacommand parser, and the
//! one real implementor of [`wr_core::determinize::DeterminizeContext`] (the hook U0c
//! landed in `wr-core` for exactly this).
//!
//! A metacommand block sits at the FRONT of a command string and is peeled off before the
//! command name is even looked at:
//!
//! ```text
//! [strategy 1 BRZ][export 3 BA] eval x "?msd_2 x = 1"::
//! ```
//!
//! Three forms exist (`MetaCommands.java:102-120`):
//!
//! * `strategy <N|*> <NAME>` — use determinization strategy `NAME` for the `N`th
//!   (non-silent) determinization of this command, or for all of them with `*`.
//! * `export <N|*> <ba|txt|gv>` — dump the `N`th automaton to that format *before* it is
//!   determinized.
//! * `earlyExistTermination` — a bare token that sets a field **nothing ever reads**
//!   (WB-028; see below).
//!
//! # Java statics that became fields here
//!
//! `PORTING.md`'s standing ruling. `MetaCommands`' constructor writes two `Prover`
//! statics (`Prover.usingOTF`, `Prover.earlyExistTermination`, `:22-25`) and
//! `getExportName` reads a third (`Prover.currentEvalName`, `:68`). All three live on
//! this struct instead: [`MetaCommands::using_otf`],
//! [`MetaCommands::early_exist_termination`], and `current_eval_name` (set by
//! `crate::prover::Prover`'s `eval`/`def` arm, exactly where Java assigns the static).
//! Since Java rebuilds `MetaCommands` at the top of every `parseSetup`, "constructor
//! resets the statics" and "fresh struct" are the same thing.
//!
//! # OTF strategies are rejected, not accepted
//!
//! `docs/DESIGN.md` §9 F3/§10 defer the whole OTF determinization family (`CCL`, `CCLS`,
//! `BRZ_CCL`, `BRZ_CCLS`), and `wr_core::determinize::Strategy` therefore has only `SC`
//! and `BRZ` — its own docs spell out the consequence for this unit: "that parser must
//! reject the four OTF aliases with a clean error of its own". [`strategy_from_string`]
//! does that, with [`MetaCommandError::OtfStrategyDeferred`], which is deliberately
//! distinguishable from [`MetaCommandError::NoStrategyFound`] (a genuinely unknown name)
//! so a user who asks for `CCLS` is told it is deferred, not that it does not exist.
//!
//! A direct consequence: `Prover.usingOTF` (`:107`) can never become `true` in this port,
//! because the assignment sits *after* the point where we have already returned an error.
//! [`MetaCommands::using_otf`] therefore always answers `false`. The flag and its one
//! reader (`crate::prover::Prover::dispatch`'s OTF-citation notice) are still ported —
//! mechanical-port rule, and they become live the day the OTF family is un-deferred.
//!
//! # `Strategy.fromString`'s dashed aliases are unreachable (WB-029)
//!
//! `fromString` strips `_` and `-` from its *input* but compares against alias lists that
//! still contain dashes, so `Brzozowski-CCL` — a strategy's own printed name — never
//! matches. Ported verbatim; see `docs/WALNUT-BUGS.md` WB-029. Narrow in practice: the
//! two in-scope strategies (`SC`, `Brzozowski`) have no dash in their names and round-trip
//! fine.
//!
//! # `String.split("\\s+")` vs `str::split_whitespace`
//!
//! Java's `"".split("\\s+")` returns a one-element array holding `""`; Rust's
//! `"".split_whitespace()` yields nothing at all. [`split_java`] reproduces Java's
//! semantics so the `parts.length != 3 && (parts.length != 1 || ...)` arity check
//! (`:98-100`) sees the same array Java does. (Both paths happen to land on the same
//! `invalidCommandUse("")` error today, but the check is arity-sensitive and a future edit
//! should not have to rediscover this.)

use std::collections::BTreeMap;
use std::rc::Rc;

use wr_core::determinize::{DeterminizeContext, ExportRequest, Strategy};
use wr_core::logging::LoggableError;

use crate::prover::{self, EXPORT, GROUP_FINAL_CMD, GROUP_META_CMD, LEFT_BRACKET};
use crate::prover_helper::export_automata;
use crate::session::SessionPaths;
use crate::walnut_exception as msg;

/// `MetaCommands.WILDCARD` (`:9`).
const WILDCARD: &str = "*";

/// `MetaCommands.DEFAULT_EXPORT_NAME` (`:10`).
pub const DEFAULT_EXPORT_NAME: &str = "export";

/// `Prover.STRATEGY` (`Prover.java:232`).
pub const STRATEGY: &str = "strategy";

/// `Prover.EARLY_EXIST_TERMINATION` (`Prover.java:234`). Spelled exactly as Walnut spells
/// it, "exist" and all.
pub const EARLY_EXIST_TERMINATION: &str = "earlyExistTermination";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure the metacommand parser can report. Module-local per this crate's
/// established idiom; the message text comes from [`crate::walnut_exception`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaCommandError {
    /// `WalnutException.invalidCommand` (`:119`).
    InvalidCommand(String),
    /// `WalnutException.invalidCommandUse` (`:99`), and `ProverHelper.matchOrFail`'s
    /// failure inside `parseMetaCommands` (`:88` — note Java passes the *whole command*
    /// as the "command name" there, so the message reads
    /// `Invalid use of the [foo command.`).
    InvalidCommandUse(String),
    /// `WalnutException.unexpectedFormat` (`:56`).
    UnexpectedFormat(String),
    /// The inline `new WalnutException(...)` at `:92`.
    RequiresDoubleColon,
    /// **Port-specific.** A recognized OTF strategy name, which `docs/DESIGN.md` §9/§10
    /// defers. Java accepts these; see this module's docs.
    OtfStrategyDeferred(String),
    /// `Strategy.fromString`'s `IllegalArgumentException`
    /// (`DeterminizationStrategies.java:61`).
    NoStrategyFound(String),
    /// `Integer.parseInt`'s `NumberFormatException`, from `addStrategy`/`addExport`
    /// (`:39`, `:62`) — the automaton index is never validated before parsing.
    NumberFormat(String),
}

impl std::fmt::Display for MetaCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetaCommandError::InvalidCommand(c) => write!(f, "{}", msg::invalid_command(c)),
            MetaCommandError::InvalidCommandUse(c) => write!(f, "{}", msg::invalid_command_use(c)),
            MetaCommandError::UnexpectedFormat(x) => write!(f, "{}", msg::unexpected_format(x)),
            MetaCommandError::RequiresDoubleColon => {
                write!(f, "{}", msg::metacommands_require_double_colon())
            }
            MetaCommandError::OtfStrategyDeferred(name) => write!(
                f,
                "Determinization strategy {name} is an OTF strategy, which walnut-rs \
                 deliberately does not implement (see docs/DESIGN.md sections 9 and 10)"
            ),
            MetaCommandError::NoStrategyFound(name) => {
                write!(f, "{}", msg::no_strategy_found(name))
            }
            MetaCommandError::NumberFormat(input) => {
                write!(f, "{}", msg::number_format_exception(input))
            }
        }
    }
}

impl std::error::Error for MetaCommandError {}

impl LoggableError for MetaCommandError {
    /// Exhaustive on purpose, same discipline as `wr_logic::eval`'s `ActError` impl: this
    /// answers "was this a `WalnutException` in Java, or an escaping runtime exception?"
    fn is_handled(&self) -> bool {
        match self {
            MetaCommandError::InvalidCommand(_)
            | MetaCommandError::InvalidCommandUse(_)
            | MetaCommandError::UnexpectedFormat(_)
            | MetaCommandError::RequiresDoubleColon => true,
            // No Java analogue at all (this port's own deferral); reported message-only.
            MetaCommandError::OtfStrategyDeferred(_) => true,
            // `IllegalArgumentException` / `NumberFormatException` — not `WalnutException`.
            MetaCommandError::NoStrategyFound(_) | MetaCommandError::NumberFormat(_) => false,
        }
    }

    fn message(&self) -> Option<String> {
        Some(self.to_string())
    }

    fn kind(&self) -> String {
        match self {
            MetaCommandError::NoStrategyFound(_) => {
                "java.lang.IllegalArgumentException".to_string()
            }
            MetaCommandError::NumberFormat(_) => "java.lang.NumberFormatException".to_string(),
            _ => "Main.WalnutException".to_string(),
        }
    }

    /// This port has no JVM frames to report — see `wr_core::logging`'s module docs.
    fn stack_trace_lines(&self) -> Vec<String> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// `DeterminizationStrategies.Strategy.fromString` (`:52-63`)
// ---------------------------------------------------------------------------

/// Java's six enum members, in `Strategy.values()` order, as
/// `(name, is_otf, aliases_without_the_name)`.
///
/// Java's constructor appends `name` to the alias list (`:48`), which is why `SC`/`CCL`/
/// `CCLS` end up with their name listed twice; that duplication is harmless and is
/// reproduced by [`aliases_of`] rather than written out twice here.
const STRATEGY_TABLE: &[(&str, bool, &[&str])] = &[
    ("SC", false, &["SC"]),
    ("Brzozowski", false, &["Brz"]),
    ("CCLS", true, &["CCLS"]),
    ("Brzozowski-CCLS", true, &["BRZCCLS"]),
    ("CCL", true, &["CCL"]),
    ("Brzozowski-CCL", true, &["BRZCCL"]),
];

/// The full alias list Java's constructor builds: the declared aliases, then the name.
fn aliases_of(entry: &(&'static str, bool, &'static [&'static str])) -> Vec<&'static str> {
    let mut v = entry.2.to_vec();
    v.push(entry.0);
    v
}

/// `DeterminizationStrategies.Strategy.fromString(String)` (`:52-63`), restricted to the
/// two in-scope strategies.
///
/// Java's normalization is `name.replace("_","-").replace("-","")`, i.e. "drop every
/// underscore and dash", then a case-insensitive comparison against each alias — see this
/// module's docs (WB-029) for why the *dashed* aliases can therefore never match.
///
/// `eq_ignore_ascii_case` rather than a Unicode-aware fold: every alias is ASCII, and the
/// only way Java's `equalsIgnoreCase` could differ is a non-ASCII input character whose
/// Java case mapping lands on ASCII (e.g. `U+0131`). Not worth a Unicode dependency;
/// noted rather than silently assumed.
pub fn strategy_from_string(name: &str) -> Result<Strategy, MetaCommandError> {
    let temp_name = name.replace('_', "-").replace('-', "");
    for entry in STRATEGY_TABLE {
        for alias in aliases_of(entry) {
            if temp_name.eq_ignore_ascii_case(alias) {
                return match entry.0 {
                    "SC" => Ok(Strategy::Sc),
                    "Brzozowski" => Ok(Strategy::Brz),
                    // `strategy.isOTFStrategy()` (`:65-67`) — Java would set
                    // `Prover.usingOTF = true` and carry on; this port stops here.
                    _ => Err(MetaCommandError::OtfStrategyDeferred(name.to_string())),
                };
            }
        }
    }
    Err(MetaCommandError::NoStrategyFound(name.to_string()))
}

// ---------------------------------------------------------------------------
// MetaCommands
// ---------------------------------------------------------------------------

/// `Main/MetaCommands.java`'s state: the automaton counter plus the two per-index
/// registries, and (see the module docs) the three `Prover` statics it owned.
///
/// Construct one per command, exactly as Java's `parseSetup` does
/// (`Prover.java:433`).
pub struct MetaCommands {
    /// `automataIndex` (`:11`).
    automata_index: usize,
    /// `strategyMap` (`:14`). `BTreeMap` rather than a hash map for determinism
    /// (`PORTING.md`'s iteration-order trap); keys are `int`s straight out of
    /// `Integer.parseInt`, so they can be negative.
    strategy_map: BTreeMap<i32, Strategy>,
    /// `alwaysOnStrategy` (`:15`) — the `[strategy * …]` wildcard.
    always_on_strategy: Option<Strategy>,
    /// `exportMap` (`:19`), values already lowercased (`:54`).
    export_map: BTreeMap<i32, String>,
    /// `alwaysOnExport` (`:20`).
    always_on_export: bool,
    /// `Prover.usingOTF` (`Prover.java:253`) — see the module docs: always `false` here.
    using_otf: bool,
    /// `Prover.earlyExistTermination` (`Prover.java:254`) — **inert**, WB-028.
    early_exist_termination: bool,
    /// `Prover.currentEvalName` (`Prover.java:252`), read by `getExportName` (`:68`).
    current_eval_name: Option<String>,
    /// The `Session` half of `ProverHelper.exportAutomata`'s static reads. `None` means
    /// "no session wired up", in which case [`Self::export_pre_determinization`] records
    /// the request in [`Self::export_failures`] instead of writing a file.
    paths: Option<Rc<SessionPaths>>,
    /// **Deliberate divergence, forced by the trait signature.** Java's
    /// `ProverHelper.exportAutomata` throws, and the throw propagates out of
    /// `DeterminizationStrategies.determinize` and aborts the whole command.
    /// [`DeterminizeContext::export_pre_determinization`] returns `()` by design ("the
    /// dispatcher ignores anything the sink might want to report"), so a failed export is
    /// recorded here for the caller to inspect rather than silently dropped.
    export_failures: Vec<String>,
}

impl std::fmt::Debug for MetaCommands {
    /// Hand-written because [`SessionPaths`] is itself hand-`Debug`ged (it holds a
    /// console sink); this prints the parsed metacommand state, which is what a failing
    /// assertion wants to see.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaCommands")
            .field("automata_index", &self.automata_index)
            .field("strategy_map", &self.strategy_map)
            .field("always_on_strategy", &self.always_on_strategy)
            .field("export_map", &self.export_map)
            .field("always_on_export", &self.always_on_export)
            .field("using_otf", &self.using_otf)
            .field("early_exist_termination", &self.early_exist_termination)
            .field("current_eval_name", &self.current_eval_name)
            .field("export_failures", &self.export_failures)
            .finish_non_exhaustive()
    }
}

impl Default for MetaCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaCommands {
    /// `new MetaCommands()` (`:22-25`), including its reset of the two `Prover` statics
    /// (here: this struct's own `using_otf`/`early_exist_termination` fields).
    pub fn new() -> Self {
        MetaCommands {
            automata_index: 0,
            strategy_map: BTreeMap::new(),
            always_on_strategy: None,
            export_map: BTreeMap::new(),
            always_on_export: false,
            using_otf: false,
            early_exist_termination: false,
            current_eval_name: None,
            paths: None,
            export_failures: Vec::new(),
        }
    }

    /// As [`MetaCommands::new`], but able to actually perform `[export …]` writes.
    pub fn with_paths(paths: Rc<SessionPaths>) -> Self {
        MetaCommands {
            paths: Some(paths),
            ..MetaCommands::new()
        }
    }

    /// `MetaCommands.incrementAutomataIndex()` (`:27-29`) — a POST-increment.
    pub fn increment_automata_index(&mut self) -> usize {
        let current = self.automata_index;
        self.automata_index += 1;
        current
    }

    /// `Prover.currentEvalName = m.group(ED_NAME)` (`Prover.java:599`), which is a plain
    /// static assignment in Java (`null` in headless mode).
    pub fn set_current_eval_name(&mut self, name: Option<&str>) {
        self.current_eval_name = name.map(|s| s.to_string());
    }

    /// `Prover.usingOTF` (`Prover.java:253`). Always `false` — see the module docs.
    pub fn using_otf(&self) -> bool {
        self.using_otf
    }

    /// `Prover.earlyExistTermination` (`Prover.java:254`).
    ///
    /// **WB-028: nothing in Walnut ever reads this.** The `earlyExistTermination`
    /// metacommand parses, sets the flag, and has no effect whatsoever. Ported as inert
    /// (stored, exposed, never consulted) rather than dropped, per `CLAUDE.md`'s
    /// port-the-dead-code-verbatim rule; this accessor exists so the field is genuinely
    /// observable (and so a future unit that finds the missing reader can wire it up).
    pub fn early_exist_termination(&self) -> bool {
        self.early_exist_termination
    }

    /// Export failures recorded by [`DeterminizeContext::export_pre_determinization`] —
    /// see the field's docs for why they are collected rather than thrown.
    pub fn export_failures(&self) -> &[String] {
        &self.export_failures
    }

    /// `MetaCommands.addStrategy(String, Strategy)` (`:35-41`). Java's own comment: "it's
    /// impossible to validate the automata index when invoked."
    pub fn add_strategy(
        &mut self,
        automata_idx: &str,
        strategy: Strategy,
    ) -> Result<(), MetaCommandError> {
        if WILDCARD == automata_idx {
            self.always_on_strategy = Some(strategy);
        } else {
            self.strategy_map.insert(parse_int(automata_idx)?, strategy);
        }
        Ok(())
    }

    /// `MetaCommands.getStrategy(int)` (`:43-48`) — the wildcard wins over any per-index
    /// entry, and the fallback is `SC`.
    pub fn get_strategy(&self, automata_idx: i32) -> Strategy {
        if let Some(s) = self.always_on_strategy {
            return s;
        }
        self.strategy_map
            .get(&automata_idx)
            .copied()
            .unwrap_or(Strategy::Sc)
    }

    /// `MetaCommands.addExport(String, String)` (`:50-64`).
    ///
    /// Note the wildcard branch stores under key `0` *and* sets the flag, which is why
    /// [`Self::get_export_format`] looks up `0` whenever the flag is set.
    pub fn add_export(&mut self, automata_idx: &str, format: &str) -> Result<(), MetaCommandError> {
        let format_lower = format.to_lowercase();
        if format_lower != "ba" && format_lower != "txt" && format_lower != "gv" {
            return Err(MetaCommandError::UnexpectedFormat(format.to_string()));
        }
        if WILDCARD == automata_idx {
            self.always_on_export = true;
            self.export_map.insert(0, format_lower);
        } else {
            self.export_map
                .insert(parse_int(automata_idx)?, format_lower);
        }
        Ok(())
    }

    /// `MetaCommands.getExportName(int)` (`:66-71`) — `None` is Java's `null`.
    pub fn get_export_name(&self, index: i32) -> Option<String> {
        if self.always_on_export || self.export_map.contains_key(&index) {
            return Some(match &self.current_eval_name {
                None => DEFAULT_EXPORT_NAME.to_string(),
                Some(name) => name.clone(),
            });
        }
        None
    }

    /// `MetaCommands.getExportFormat(int)` (`:72-77`).
    pub fn get_export_format(&self, index: i32) -> Option<String> {
        if self.always_on_export || self.export_map.contains_key(&index) {
            let key = if self.always_on_export { 0 } else { index };
            return self.export_map.get(&key).cloned();
        }
        None
    }

    /// `MetaCommands.parseMetaCommands(String command, boolean printDetails)`
    /// (`:79-123`) — peels every leading `[...]` block off `command` and returns the rest.
    ///
    /// Java strips the remainder each time round the loop (`:95`), so
    /// `[a b c]  [d e f]  cmd` works and the returned string is already trimmed.
    pub fn parse_meta_commands(
        &mut self,
        command: &str,
        print_details: bool,
    ) -> Result<String, MetaCommandError> {
        let mut command = command.to_string();
        while command.starts_with(LEFT_BRACKET) {
            // `ProverHelper.matchOrFail(Prover.PAT_META_CMD, command, command)` (`:88`) --
            // yes, the whole command doubles as the "command name" in the error message.
            let caps = prover::find(&prover::patterns().meta_cmd, &command)
                .ok_or_else(|| MetaCommandError::InvalidCommandUse(command.clone()))?;
            let meta_command_string = prover::group(&caps, &command, GROUP_META_CMD)
                .unwrap_or("")
                .trim()
                .to_string();
            if !meta_command_string.is_empty() && !print_details {
                return Err(MetaCommandError::RequiresDoubleColon);
            }

            command = prover::group(&caps, &command, GROUP_FINAL_CMD)
                .unwrap_or("")
                .trim()
                .to_string();

            let parts = split_java(&meta_command_string);
            if parts.len() != 3 && (parts.len() != 1 || parts[0] != EARLY_EXIST_TERMINATION) {
                return Err(MetaCommandError::InvalidCommandUse(meta_command_string));
            }

            match parts[0].as_str() {
                // example: strategy 15 CCL
                STRATEGY => {
                    let strategy = strategy_from_string(&parts[2])?;
                    // Java's `if (strategy.isOTFStrategy()) Prover.usingOTF = true;`
                    // (`:106-108`) is unreachable here -- `strategy_from_string` already
                    // returned `Err` for every OTF name. See the module docs.
                    self.add_strategy(&parts[1], strategy)?;
                }
                // example: export 15 BA, or export * TXT
                EXPORT => {
                    self.add_export(&parts[1], &parts[2])?;
                }
                EARLY_EXIST_TERMINATION => {
                    self.early_exist_termination = true;
                }
                // Note Java reports the *remaining* command here, not the offending
                // metacommand -- `command` was already reassigned at `:95`.
                _ => return Err(MetaCommandError::InvalidCommand(command)),
            }
        }
        Ok(command)
    }
}

/// `Integer.parseInt`, with `NumberFormatException` as a `Result`.
fn parse_int(s: &str) -> Result<i32, MetaCommandError> {
    s.parse::<i32>()
        .map_err(|_| MetaCommandError::NumberFormat(s.to_string()))
}

/// Java's `\s` character class: ASCII only (`[ \t\n\x0B\f\r]`), unlike
/// `char::is_whitespace`. Same ruling as `wr-logic`'s lexer, which writes every `\s` as
/// `(?-u:\s)` for this reason (`PORTING.md`'s "Phase 3 rulings", divergence 2).
fn is_java_regex_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// Java's `String.split("\\s+")` — see the module docs for why `split_whitespace` is not
/// a substitute. (Java also keeps a leading empty field when the string *starts* with
/// whitespace; the only caller strips first, so that case cannot arise, but the helper
/// reproduces it anyway rather than depending on the caller.)
fn split_java(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if is_java_regex_whitespace(c) {
            parts.push(std::mem::take(&mut current));
            // Consume the rest of the run: `\s+` is greedy.
            while chars.peek().is_some_and(|n| is_java_regex_whitespace(*n)) {
                chars.next();
            }
        } else {
            current.push(c);
        }
    }
    parts.push(current);
    // Java drops TRAILING empty strings (`split` with limit 0); it keeps leading ones.
    while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

// ---------------------------------------------------------------------------
// DeterminizeContext
// ---------------------------------------------------------------------------

/// The three reads `DeterminizationStrategies.determinize` makes off
/// `Prover.mainProver.metaCommands` (`DeterminizationStrategies.java:99-109`).
impl DeterminizeContext for MetaCommands {
    fn next_automaton_index(&mut self) -> usize {
        self.increment_automata_index()
    }

    fn strategy(&mut self, automaton_index: usize) -> Strategy {
        // Java's index is an `int`; a `usize` past `i32::MAX` can match no key that
        // `Integer.parseInt` could have produced, so it falls through to the wildcard /
        // `SC` default -- which is exactly what `get_strategy` does with any unmapped key.
        match i32::try_from(automaton_index) {
            Ok(idx) => self.get_strategy(idx),
            Err(_) => self.always_on_strategy.unwrap_or(Strategy::Sc),
        }
    }

    fn export_pre_determinization(&mut self, request: ExportRequest<'_>) {
        let Ok(idx) = i32::try_from(request.automaton_index) else {
            return;
        };
        // `String exportName = mc.getExportName(automataIdx); if (exportName != null) {…}`
        let Some(export_name) = self.get_export_name(idx) else {
            return;
        };
        let Some(export_format) = self.get_export_format(idx) else {
            return;
        };
        // `exportName + "_" + automataIdx + "_pre"`.
        let filename = format!("{export_name}_{}_pre", request.automaton_index);
        let Some(paths) = self.paths.clone() else {
            self.export_failures.push(format!(
                "no session is wired up, so {filename} could not be exported"
            ));
            return;
        };
        // `ProverHelper.exportAutomata(Prover.currentEvalName, …, A, fa.isFAO())`.
        let predicate = self.current_eval_name.clone();
        if let Err(e) = export_automata(
            &paths,
            predicate.as_deref(),
            &filename,
            &export_format,
            request.automaton,
            request.is_fao,
        ) {
            self.export_failures.push(e.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wr_core::automaton::Automaton;
    use wr_core::numsys::less_than_msd;

    fn meta(
        print_details: bool,
        command: &str,
    ) -> Result<(MetaCommands, String), MetaCommandError> {
        let mut mc = MetaCommands::new();
        let rest = mc.parse_meta_commands(command, print_details)?;
        Ok((mc, rest))
    }

    // -- stripping ------------------------------------------------------------

    #[test]
    fn a_command_with_no_metacommands_is_returned_unchanged() {
        let (mc, rest) = meta(true, "eval x \"?msd_2 x = 1\"").unwrap();
        assert_eq!(rest, "eval x \"?msd_2 x = 1\"");
        assert_eq!(mc.get_strategy(0), Strategy::Sc);
        assert!(mc.get_export_name(0).is_none());
    }

    #[test]
    fn a_single_strategy_block_is_stripped_and_registered() {
        let (mc, rest) = meta(true, "[strategy 1 BRZ]eval x \"x = 1\"").unwrap();
        assert_eq!(rest, "eval x \"x = 1\"");
        assert_eq!(mc.get_strategy(1), Strategy::Brz);
        // Unconfigured indices fall back to Java's `SC` default.
        assert_eq!(mc.get_strategy(0), Strategy::Sc);
        assert_eq!(mc.get_strategy(2), Strategy::Sc);
    }

    #[test]
    fn several_blocks_are_peeled_in_order_and_whitespace_is_stripped() {
        let (mc, rest) = meta(true, "[strategy 2 Brz]  [export 1 GV]   eval x \"x = 1\"").unwrap();
        assert_eq!(rest, "eval x \"x = 1\"");
        assert_eq!(mc.get_strategy(2), Strategy::Brz);
        assert_eq!(mc.get_export_format(1).as_deref(), Some("gv"));
        assert_eq!(mc.get_export_name(1).as_deref(), Some("export"));
        // Not configured for index 0.
        assert!(mc.get_export_format(0).is_none());
        assert!(mc.get_export_name(0).is_none());
    }

    #[test]
    fn the_strategy_wildcard_beats_every_per_index_entry() {
        let (mc, _) = meta(true, "[strategy 1 SC][strategy * BRZ]cmd").unwrap();
        assert_eq!(mc.get_strategy(0), Strategy::Brz);
        assert_eq!(mc.get_strategy(1), Strategy::Brz);
        assert_eq!(mc.get_strategy(999), Strategy::Brz);
    }

    #[test]
    fn the_export_wildcard_applies_to_every_index() {
        let (mc, _) = meta(true, "[export * ba]cmd").unwrap();
        for i in [0, 1, 7] {
            assert_eq!(mc.get_export_format(i).as_deref(), Some("ba"));
            assert_eq!(mc.get_export_name(i).as_deref(), Some("export"));
        }
    }

    #[test]
    fn the_export_name_follows_the_current_eval_name() {
        let (mut mc, _) = meta(true, "[export 0 txt]cmd").unwrap();
        assert_eq!(mc.get_export_name(0).as_deref(), Some("export"));
        mc.set_current_eval_name(Some("myeval"));
        assert_eq!(mc.get_export_name(0).as_deref(), Some("myeval"));
    }

    #[test]
    fn export_formats_are_lowercased() {
        let (mc, _) = meta(true, "[export 3 BA]cmd").unwrap();
        assert_eq!(mc.get_export_format(3).as_deref(), Some("ba"));
    }

    // -- errors ---------------------------------------------------------------

    #[test]
    fn metacommands_need_a_double_colon_command() {
        let err = meta(false, "[strategy 1 BRZ]cmd").unwrap_err();
        assert_eq!(err, MetaCommandError::RequiresDoubleColon);
        assert_eq!(
            err.to_string(),
            "Metacommands are currently only supported for commands ending in ::"
        );
    }

    /// An EMPTY block is allowed through the `printDetails` guard (`:91`) and then fails
    /// the arity check — verbatim Java behavior, quirk and all.
    #[test]
    fn an_empty_block_is_an_invalid_command_use_of_the_empty_string() {
        let err = meta(false, "[]cmd").unwrap_err();
        assert_eq!(err, MetaCommandError::InvalidCommandUse(String::new()));
        assert_eq!(err.to_string(), "Invalid use of the  command.");
    }

    #[test]
    fn a_two_token_block_is_rejected() {
        let err = meta(true, "[strategy 1]cmd").unwrap_err();
        assert_eq!(
            err,
            MetaCommandError::InvalidCommandUse("strategy 1".to_string())
        );
    }

    #[test]
    fn an_unknown_three_token_block_reports_the_remaining_command() {
        // Java reassigns `command` before the switch, so the message names what's LEFT.
        let err = meta(true, "[bogus 1 2]cmd").unwrap_err();
        assert_eq!(err, MetaCommandError::InvalidCommand("cmd".to_string()));
    }

    #[test]
    fn an_unclosed_bracket_is_an_invalid_command_use_of_the_whole_command() {
        let err = meta(true, "[strategy 1 BRZ eval x \"x=1\"").unwrap_err();
        assert!(matches!(err, MetaCommandError::InvalidCommandUse(c) if c.starts_with('[')));
    }

    #[test]
    fn an_unsupported_export_format_is_unexpected_format() {
        let err = meta(true, "[export 1 pdf]cmd").unwrap_err();
        assert_eq!(err, MetaCommandError::UnexpectedFormat("pdf".to_string()));
        assert_eq!(err.to_string(), "Unexpected format:pdf");
    }

    #[test]
    fn a_non_numeric_automaton_index_is_a_number_format_exception() {
        let err = meta(true, "[strategy x BRZ]cmd").unwrap_err();
        assert_eq!(err, MetaCommandError::NumberFormat("x".to_string()));
        assert_eq!(err.to_string(), "For input string: \"x\"");
        assert!(
            !err.is_handled(),
            "NumberFormatException is not a WalnutException"
        );
    }

    // -- OTF deferral ---------------------------------------------------------

    #[test]
    fn every_otf_strategy_name_is_rejected_as_deferred() {
        for name in ["CCL", "CCLS", "BRZ_CCL", "BRZ_CCLS", "brzccls", "ccl"] {
            let err = strategy_from_string(name).unwrap_err();
            assert_eq!(
                err,
                MetaCommandError::OtfStrategyDeferred(name.to_string()),
                "{name} must be recognized-but-deferred, not unknown"
            );
        }
    }

    #[test]
    fn an_otf_strategy_metacommand_is_rejected_cleanly() {
        let err = meta(true, "[strategy 1 CCLS]cmd").unwrap_err();
        assert_eq!(
            err,
            MetaCommandError::OtfStrategyDeferred("CCLS".to_string())
        );
        assert!(err.to_string().contains("OTF"));
        assert!(err.is_handled(), "a deferral is reported message-only");
    }

    #[test]
    fn the_two_in_scope_strategy_names_round_trip_including_aliases() {
        assert_eq!(strategy_from_string("SC").unwrap(), Strategy::Sc);
        assert_eq!(strategy_from_string("sc").unwrap(), Strategy::Sc);
        assert_eq!(strategy_from_string("S_C").unwrap(), Strategy::Sc);
        assert_eq!(strategy_from_string("BRZ").unwrap(), Strategy::Brz);
        assert_eq!(strategy_from_string("Brz").unwrap(), Strategy::Brz);
        assert_eq!(strategy_from_string("Brzozowski").unwrap(), Strategy::Brz);
        // Both in-scope names round-trip through `Strategy::name()`.
        assert_eq!(
            strategy_from_string(Strategy::Sc.name()).unwrap(),
            Strategy::Sc
        );
        assert_eq!(
            strategy_from_string(Strategy::Brz.name()).unwrap(),
            Strategy::Brz
        );
    }

    /// WB-029: `fromString` strips dashes from its input but not from its alias list, so
    /// a strategy's own dashed printed name never parses. Pinned here as the faithful
    /// (wrong) behavior.
    #[test]
    fn wb_029_dashed_strategy_names_are_unreachable_aliases() {
        for name in ["Brzozowski-CCL", "Brzozowski-CCLS"] {
            let err = strategy_from_string(name).unwrap_err();
            assert_eq!(
                err,
                MetaCommandError::NoStrategyFound(name.to_string()),
                "{name} is its strategy's own printed name and STILL does not parse"
            );
        }
    }

    #[test]
    fn an_unknown_strategy_name_is_no_strategy_found() {
        let err = strategy_from_string("nonesuch").unwrap_err();
        assert_eq!(
            err,
            MetaCommandError::NoStrategyFound("nonesuch".to_string())
        );
        assert_eq!(err.to_string(), "No strategy found for: nonesuch");
    }

    // -- earlyExistTermination (WB-028) ---------------------------------------

    #[test]
    fn wb_028_early_exist_termination_parses_and_sets_an_inert_flag() {
        let (mc, rest) = meta(true, "[earlyExistTermination]eval x \"x=1\"").unwrap();
        assert_eq!(rest, "eval x \"x=1\"");
        assert!(mc.early_exist_termination());
        // ...and changes nothing else. Nothing in Walnut reads this flag.
        assert_eq!(mc.get_strategy(0), Strategy::Sc);
        assert!(mc.get_export_name(0).is_none());
    }

    #[test]
    fn early_exist_termination_defaults_to_false() {
        let (mc, _) = meta(true, "[strategy 1 SC]cmd").unwrap();
        assert!(!mc.early_exist_termination());
        assert!(!mc.using_otf());
    }

    // -- split_java -----------------------------------------------------------

    #[test]
    fn split_java_matches_javas_split_semantics() {
        assert_eq!(split_java(""), vec![""]);
        assert_eq!(split_java("a"), vec!["a"]);
        assert_eq!(split_java("a b c"), vec!["a", "b", "c"]);
        assert_eq!(split_java("a   b"), vec!["a", "b"]);
        // Java drops trailing empties, keeps a leading one.
        assert_eq!(split_java("a "), vec!["a"]);
        assert_eq!(split_java(" a"), vec!["", "a"]);
    }

    // -- DeterminizeContext ---------------------------------------------------

    #[test]
    fn the_automaton_index_post_increments() {
        let mut mc = MetaCommands::new();
        assert_eq!(mc.next_automaton_index(), 0);
        assert_eq!(mc.next_automaton_index(), 1);
        assert_eq!(mc.next_automaton_index(), 2);
    }

    #[test]
    fn determinize_context_strategy_reads_the_parsed_configuration() {
        let (mut mc, _) = meta(true, "[strategy 1 BRZ]cmd").unwrap();
        assert_eq!(DeterminizeContext::strategy(&mut mc, 0), Strategy::Sc);
        assert_eq!(DeterminizeContext::strategy(&mut mc, 1), Strategy::Brz);
        assert_eq!(DeterminizeContext::strategy(&mut mc, 2), Strategy::Sc);
    }

    fn temp_paths(tag: &str) -> (Rc<SessionPaths>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wr-cli-meta-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
        for sub in ["Result", "Automata Library", "Word Automata Library"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let dir_str = format!("{}/", dir.to_str().unwrap());
        (
            Rc::new(SessionPaths::new(Some(&dir_str), Some(&dir_str), false)),
            dir,
        )
    }

    #[test]
    fn export_pre_determinization_writes_the_configured_format() {
        let (paths, dir) = temp_paths("export");
        let mut mc = MetaCommands::with_paths(paths);
        mc.parse_meta_commands("[export 0 gv]cmd", true).unwrap();
        mc.set_current_eval_name(Some("myeval"));

        let a: Automaton = less_than_msd(2);
        let idx = mc.next_automaton_index();
        mc.export_pre_determinization(ExportRequest {
            automaton_index: idx,
            automaton: &a,
            is_fao: false,
        });

        assert!(
            mc.export_failures().is_empty(),
            "{:?}",
            mc.export_failures()
        );
        assert!(dir.join("Result").join("myeval_0_pre.gv").is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_pre_determinization_is_a_no_op_for_an_unconfigured_index() {
        let (paths, dir) = temp_paths("noexport");
        let mut mc = MetaCommands::with_paths(paths);
        mc.parse_meta_commands("[export 5 gv]cmd", true).unwrap();

        let a: Automaton = less_than_msd(2);
        mc.export_pre_determinization(ExportRequest {
            automaton_index: 0,
            automaton: &a,
            is_fao: false,
        });
        assert!(mc.export_failures().is_empty());
        assert!(!dir.join("Result").join("export_0_pre.gv").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_export_is_recorded_rather_than_lost() {
        // `txt` is the one format `ProverHelper.exportAutomata` refuses outright.
        let (paths, dir) = temp_paths("failedexport");
        let mut mc = MetaCommands::with_paths(paths);
        mc.parse_meta_commands("[export * txt]cmd", true).unwrap();

        let a: Automaton = less_than_msd(2);
        mc.export_pre_determinization(ExportRequest {
            automaton_index: 0,
            automaton: &a,
            is_fao: false,
        });
        assert_eq!(mc.export_failures().len(), 1);
        assert!(mc.export_failures()[0].contains("redundant"));
        fs::remove_dir_all(&dir).ok();
    }
}
