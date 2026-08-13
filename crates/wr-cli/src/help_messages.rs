// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/HelpMessages.java` (225 LOC) — the `help` command and its help-text database.
//!
//! Ports the lookup logic for Walnut's hierarchical help system, organized by category
//! and command name. All help text is embedded at compile time via `include_str!` macros,
//! eliminating runtime file I/O.
//!
//! # Help command variants
//!
//! - `help` with no arguments: lists all categories and their commands.
//! - `help <group>`: lists all commands in the named category.
//! - `help <command>`: searches all categories for a command by name.
//! - `help <group> <command>`: shows help for a specific command in a specific category.
//!
//! # Subset scope
//!
//! Help text for all commands from the Walnut Java port is included verbatim
//! (`docs/BOUNDARY-MAP.md` §7 confirms `HelpMessages.java` is KEEP scope). This includes
//! commands designated DROP in `BOUNDARY-MAP.md` (e.g., `ost`, `split`, `rsplit`) — the
//! help text is data, not code; including it here is harmless and avoids any risk of
//! forgetting to exclude it. If a user requests help for a DROP-scope command and that
//! command is not wired into the actual `wr-cli` dispatcher, they'll simply see "No
//! documentation found" or get an error from the command dispatcher itself — the help
//! module does not validate whether commands are actually implemented.

use std::collections::BTreeMap;

// ============================================================================
// Embedded help text for each command
// ============================================================================

const HELP_AUTOMATA_COMBINE: &str = include_str!("../data/help/Commands/Automata/combine.txt");
const HELP_AUTOMATA_CONCAT: &str = include_str!("../data/help/Commands/Automata/concat.txt");
const HELP_AUTOMATA_CONVERT: &str = include_str!("../data/help/Commands/Automata/convert.txt");
const HELP_AUTOMATA_DEF: &str = include_str!("../data/help/Commands/Automata/def.txt");
const HELP_AUTOMATA_DESCRIBE: &str = include_str!("../data/help/Commands/Automata/describe.txt");
const HELP_AUTOMATA_EVAL: &str = include_str!("../data/help/Commands/Automata/eval.txt");
const HELP_AUTOMATA_FIXLEADZERO: &str =
    include_str!("../data/help/Commands/Automata/fixleadzero.txt");
const HELP_AUTOMATA_FIXTRAILZERO: &str =
    include_str!("../data/help/Commands/Automata/fixtrailzero.txt");
const HELP_AUTOMATA_INF: &str = include_str!("../data/help/Commands/Automata/inf.txt");
const HELP_AUTOMATA_INTERSECT: &str = include_str!("../data/help/Commands/Automata/intersect.txt");
const HELP_AUTOMATA_LEFTQUO: &str = include_str!("../data/help/Commands/Automata/leftquo.txt");
const HELP_AUTOMATA_OST: &str = include_str!("../data/help/Commands/Automata/ost.txt");
const HELP_AUTOMATA_REG: &str = include_str!("../data/help/Commands/Automata/reg.txt");
const HELP_AUTOMATA_REVERSE: &str = include_str!("../data/help/Commands/Automata/reverse.txt");
const HELP_AUTOMATA_RIGHTQUO: &str = include_str!("../data/help/Commands/Automata/rightquo.txt");
const HELP_AUTOMATA_STAR: &str = include_str!("../data/help/Commands/Automata/star.txt");
const HELP_AUTOMATA_TEST: &str = include_str!("../data/help/Commands/Automata/test.txt");
const HELP_AUTOMATA_TRANSDUCE: &str = include_str!("../data/help/Commands/Automata/transduce.txt");
const HELP_AUTOMATA_UNION: &str = include_str!("../data/help/Commands/Automata/union.txt");

const HELP_METACOMMANDS_EXPORT: &str =
    include_str!("../data/help/Commands/Metacommands/[export].txt");
const HELP_METACOMMANDS_STRATEGY: &str =
    include_str!("../data/help/Commands/Metacommands/[strategy].txt");

const HELP_MORPHISMS_ALPHABET: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/alphabet.txt");
const HELP_MORPHISMS_IMAGE: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/image.txt");
const HELP_MORPHISMS_JOIN: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/join.txt");
const HELP_MORPHISMS_MINIMIZE: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/minimize.txt");
const HELP_MORPHISMS_MORPHISM: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/morphism.txt");
const HELP_MORPHISMS_PROMOTE: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/promote.txt");
const HELP_MORPHISMS_RSPLIT: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/rsplit.txt");
const HELP_MORPHISMS_SPLIT: &str =
    include_str!("../data/help/Commands/Morphisms And Word Automata/split.txt");

const HELP_WALNUT_CLS: &str = include_str!("../data/help/Commands/Walnut/cls.txt");
const HELP_WALNUT_EXPORT: &str = include_str!("../data/help/Commands/Walnut/export.txt");
const HELP_WALNUT_LOAD: &str = include_str!("../data/help/Commands/Walnut/load.txt");
const HELP_WALNUT_MACRO: &str = include_str!("../data/help/Commands/Walnut/macro.txt");
const HELP_WALNUT_QUIT: &str = include_str!("../data/help/Commands/Walnut/quit.txt");

// ============================================================================
// Static help database: category -> (command -> text)
// ============================================================================

type HelpDatabase = BTreeMap<&'static str, BTreeMap<&'static str, &'static str>>;

fn build_help_database() -> HelpDatabase {
    let mut db = BTreeMap::new();

    let mut automata = BTreeMap::new();
    automata.insert("combine", HELP_AUTOMATA_COMBINE);
    automata.insert("concat", HELP_AUTOMATA_CONCAT);
    automata.insert("convert", HELP_AUTOMATA_CONVERT);
    automata.insert("def", HELP_AUTOMATA_DEF);
    automata.insert("describe", HELP_AUTOMATA_DESCRIBE);
    automata.insert("eval", HELP_AUTOMATA_EVAL);
    automata.insert("fixleadzero", HELP_AUTOMATA_FIXLEADZERO);
    automata.insert("fixtrailzero", HELP_AUTOMATA_FIXTRAILZERO);
    automata.insert("inf", HELP_AUTOMATA_INF);
    automata.insert("intersect", HELP_AUTOMATA_INTERSECT);
    automata.insert("leftquo", HELP_AUTOMATA_LEFTQUO);
    automata.insert("ost", HELP_AUTOMATA_OST);
    automata.insert("reg", HELP_AUTOMATA_REG);
    automata.insert("reverse", HELP_AUTOMATA_REVERSE);
    automata.insert("rightquo", HELP_AUTOMATA_RIGHTQUO);
    automata.insert("star", HELP_AUTOMATA_STAR);
    automata.insert("test", HELP_AUTOMATA_TEST);
    automata.insert("transduce", HELP_AUTOMATA_TRANSDUCE);
    automata.insert("union", HELP_AUTOMATA_UNION);
    db.insert("Automata", automata);

    let mut metacommands = BTreeMap::new();
    metacommands.insert("[export]", HELP_METACOMMANDS_EXPORT);
    metacommands.insert("[strategy]", HELP_METACOMMANDS_STRATEGY);
    db.insert("Metacommands", metacommands);

    let mut morphisms = BTreeMap::new();
    morphisms.insert("alphabet", HELP_MORPHISMS_ALPHABET);
    morphisms.insert("image", HELP_MORPHISMS_IMAGE);
    morphisms.insert("join", HELP_MORPHISMS_JOIN);
    morphisms.insert("minimize", HELP_MORPHISMS_MINIMIZE);
    morphisms.insert("morphism", HELP_MORPHISMS_MORPHISM);
    morphisms.insert("promote", HELP_MORPHISMS_PROMOTE);
    morphisms.insert("rsplit", HELP_MORPHISMS_RSPLIT);
    morphisms.insert("split", HELP_MORPHISMS_SPLIT);
    db.insert("Morphisms And Word Automata", morphisms);

    let mut walnut = BTreeMap::new();
    walnut.insert("cls", HELP_WALNUT_CLS);
    walnut.insert("export", HELP_WALNUT_EXPORT);
    walnut.insert("load", HELP_WALNUT_LOAD);
    walnut.insert("macro", HELP_WALNUT_MACRO);
    walnut.insert("quit", HELP_WALNUT_QUIT);
    db.insert("Walnut", walnut);

    db
}

// ============================================================================
// Public API
// ============================================================================

/// Processes a help command and returns the output text.
///
/// # Arguments
///
/// * `tokens` - parsed help command arguments (group and/or command name).
///
/// # Returns
///
/// A `String` containing the help output to display to the user, or an error message
/// if the requested help is not found.
///
/// # Examples
///
/// - `help_command(&[])` — lists all groups and commands
/// - `help_command(&["Automata"])` — lists all commands in "Automata" group
/// - `help_command(&["eval"])` — searches for command "eval" across all groups
/// - `help_command(&["Automata", "eval"])` — shows help for "Automata.eval"
pub fn help_command(tokens: &[&str]) -> String {
    let db = build_help_database();

    match tokens.len() {
        0 => list_all_groups_detailed(&db),
        1 => {
            let maybe_group_or_command = tokens[0];
            if db.contains_key(maybe_group_or_command) {
                // It's a known group
                list_commands_in_group(&db, maybe_group_or_command)
            } else {
                // Assume it's a command name; search across all groups
                show_command_help_across_all_groups(&db, maybe_group_or_command)
            }
        }
        2 => {
            let group = tokens[0];
            let command = tokens[1];
            show_command_help(&db, group, command)
        }
        _ => "Too many arguments. Usage: help [group] [command];".to_string(),
    }
}

// ============================================================================
// Internal functions
// ============================================================================

/// Lists all groups and their commands when the user types `help;`
fn list_all_groups_detailed(db: &HelpDatabase) -> String {
    let mut output = String::from("Available help groups and commands:\n");

    for (group_name, commands) in db.iter() {
        output.push_str(&format!("Group: {}\n", group_name));
        output.push_str("  Commands:\n");

        for command_name in commands.keys() {
            output.push_str(&format!("   - {}\n", command_name));
        }
        output.push('\n');
    }

    output
}

/// Lists all commands in a specific group.
fn list_commands_in_group(db: &HelpDatabase, group_name: &str) -> String {
    match db.get(group_name) {
        Some(commands) => {
            let mut output = format!("Commands in group \"{}\":\n", group_name);
            for command_name in commands.keys() {
                output.push_str(&format!(" - {}\n", command_name));
            }
            output
        }
        None => format!("Unknown group: \"{}\"", group_name),
    }
}

/// Searches all groups for a command by name and shows its help.
fn show_command_help_across_all_groups(db: &HelpDatabase, command_name: &str) -> String {
    for commands in db.values() {
        if let Some(help_text) = commands.get(command_name) {
            return format_help_output(command_name, help_text);
        }
    }

    format!("No documentation found for command \"{}\".", command_name)
}

/// Shows help for a specific command in a specific group.
fn show_command_help(db: &HelpDatabase, group: &str, command: &str) -> String {
    match db.get(group) {
        Some(commands) => match commands.get(command) {
            Some(help_text) => format_help_output(command, help_text),
            None => format!(
                "No documentation found for command \"{}\" in group \"{}\".",
                command, group
            ),
        },
        None => format!("Unknown group: \"{}\"", group),
    }
}

/// Formats a help file's content with a header and footer.
fn format_help_output(command_name: &str, help_text: &str) -> String {
    format!(
        "=== Help: {} ===\n{}\n=============================",
        command_name, help_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_no_args_lists_all_groups() {
        let output = help_command(&[]);
        assert!(output.contains("Available help groups and commands:"));
        // Check that major groups are listed
        assert!(output.contains("Automata"));
        assert!(output.contains("Walnut"));
        assert!(output.contains("Metacommands"));
    }

    #[test]
    fn test_help_no_args_lists_sample_commands() {
        let output = help_command(&[]);
        // Some representative commands should appear
        assert!(output.contains("eval"));
        assert!(output.contains("combine"));
        assert!(output.contains("promote"));
    }

    #[test]
    fn test_help_group_only_lists_commands_in_group() {
        let output = help_command(&["Automata"]);
        assert!(output.contains("Commands in group \"Automata\""));
        assert!(output.contains("eval"));
        assert!(output.contains("combine"));
        // Should not list commands from other groups
        assert!(!output.contains("promote"));
    }

    #[test]
    fn test_help_single_command_searches_all_groups() {
        let output = help_command(&["eval"]);
        assert!(output.contains("=== Help: eval ==="));
        assert!(output.contains("identical to the \"def\" command"));
    }

    #[test]
    fn test_help_combine_command() {
        let output = help_command(&["combine"]);
        assert!(output.contains("=== Help: combine ==="));
        assert!(output.contains("highest index automaton"));
    }

    #[test]
    fn test_help_group_and_command() {
        let output = help_command(&["Automata", "eval"]);
        assert!(output.contains("=== Help: eval ==="));
    }

    #[test]
    fn test_help_unknown_command() {
        let output = help_command(&["nonexistent"]);
        assert!(output.contains("No documentation found for command \"nonexistent\""));
    }

    #[test]
    fn test_help_unknown_group() {
        let output = help_command(&["NonexistentGroup", "somecommand"]);
        assert!(output.contains("Unknown group"));
    }

    #[test]
    fn test_help_too_many_args() {
        let output = help_command(&["a", "b", "c"]);
        assert!(output.contains("Too many arguments"));
    }

    #[test]
    fn test_help_promote_from_morphisms_group() {
        let output = help_command(&["Morphisms And Word Automata", "promote"]);
        assert!(output.contains("=== Help: promote ==="));
        assert!(output.contains("morphism"));
    }

    #[test]
    fn test_help_metacommand_bracket_syntax() {
        let output = help_command(&["[export]"]);
        assert!(output.contains("=== Help: [export] ==="));
    }

    #[test]
    fn test_help_walnut_quit_command() {
        let output = help_command(&["Walnut", "quit"]);
        assert!(output.contains("=== Help: quit ==="));
    }
}
