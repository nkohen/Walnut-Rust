// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/HelpMessages.java` (225 LOC) — the `help` command and its help-text database.
//!
//! Ports the tokenizer (`parseHelpArguments`) and the four lookup modes of Walnut's
//! hierarchical help system, organized by group (folder) and command name (`<name>.txt`).
//!
//! # Help command variants (Java's `helpCommand` javadoc)
//!
//! - `help;` — lists all groups and their commands.
//! - `help <group>;` — lists all commands in the named group.
//! - `help <command>;` — searches all groups for `<command>.txt`.
//! - `help <group> <command>;` — shows `<command>.txt` inside `<group>` only.
//! - three or more tokens — `Too many arguments.`
//!
//! # Output convention (differs from Java's shape, not its bytes)
//!
//! Java writes to `System.out` with one `println` per line. This port returns the same text
//! as a `String` instead, so **every** mode's return value is the exact concatenation of the
//! lines Java would `println`, each terminated by a single `\n` — i.e. **the returned string
//! always ends in a `\n`**, in all four modes and in every error path, and a caller can
//! `print!` (not `println!`) it verbatim. (The bare-`help;` mode ends in `"\n\n"`: Java's
//! `listAllGroupsDetailed` genuinely emits a blank line after each group, the last one
//! included. That trailing blank line is Java's output, not a formatting artifact here.)
//!
//! This matters for the file-printing mode in particular: Java reads help files with
//! `BufferedReader.readLine()`, which (a) accepts `\n`, `\r\n` *and* a bare `\r` as a line
//! terminator and reports none of them, and (b) does **not** yield a final empty line for a
//! file that ends with a newline. 26 of the 34 embedded files are CRLF and 9 end with a
//! trailing newline, so splicing their raw bytes in would emit stray `\r`s and a spurious
//! blank line before the footer. [`java_read_lines`] reproduces `readLine`'s exact splitting
//! instead.
//!
//! # Divergence: compile-time embedding vs. Java's live filesystem
//!
//! Java resolves every lookup against the real directory tree under
//! `Session.getAddressForHelpCommands()` (`$WALNUT_HOME/Help Documentation/Commands/`) at call
//! time. This port embeds the same tree at compile time with `include_str!`. Two deliberate,
//! user-visible consequences:
//!
//! 1. **Group/command names are matched case-SENSITIVELY here, always.** Java delegates to
//!    `File.isDirectory()`/`File.isFile()`, so on a case-insensitive filesystem (the default on
//!    macOS and Windows) `help automata eval;` works there and does not here; on Linux it fails
//!    in both.
//! 2. **User-added or user-edited help files are invisible here.** Dropping a new `.txt` into
//!    the help tree changes Java's output immediately; here it requires a rebuild (and an entry
//!    in [`build_help_database`] — `embedded_database_matches_the_on_disk_help_tree` fails loudly
//!    if the two drift apart).
//!
//! `Session::address_for_help_commands` is still ported (`session.rs`) and stays unused by this
//! module for exactly this reason.
//!
//! # Divergence: mojibake in two help files (WB-030)
//!
//! `Commands/Automata/reg.txt` and `Commands/Morphisms And Word Automata/image.txt` are
//! Windows-1252 in upstream Walnut, but Java reads them through a charset-less `FileReader`,
//! i.e. the JVM default charset (UTF-8 on the oracle's JDK 17). The four cp1252 bytes are not
//! valid UTF-8, so Java's own reader substitutes `U+FFFD`. **This port stores `U+FFFD` at those
//! four positions**, reproducing what real Walnut actually prints rather than what upstream
//! evidently meant to write (`…`, `’`, `’`, `–`). See `docs/WALNUT-BUGS.md` WB-030.
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

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// `Prover.TXT_EXTENSION`, which `HelpMessages` both filters on and sorts by.
const TXT_EXTENSION: &str = ".txt";

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
// Static help database: group -> (command -> text)
// ============================================================================

type CommandMap = BTreeMap<&'static str, &'static str>;
type HelpDatabase = BTreeMap<&'static str, CommandMap>;

/// The embedded stand-in for Java's `Help Documentation/Commands/` directory tree.
///
/// `BTreeMap` is used for *lookup* only; it is deliberately **not** relied on for display
/// order — see [`sorted_by_java_name`] for why Java's order is not the same rule.
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

/// The database, built once per process (it is pure static data).
fn help_database() -> &'static HelpDatabase {
    static DB: OnceLock<HelpDatabase> = OnceLock::new();
    DB.get_or_init(build_help_database)
}

// ============================================================================
// Public API
// ============================================================================

/// `HelpMessages.helpCommand(fullCommandLine)` (`:25-67`) — the `help` command.
///
/// Takes the **raw command line** (e.g. `"help Automata eval;"`), exactly as Java does, and
/// returns the text Java would have printed. See the module docs for the newline convention:
/// the result always ends in exactly one `\n`, in every mode and on every error path.
///
/// Java's `IOException` arm (rethrow as `WalnutException.errorCommand("help")`) has no
/// counterpart here: there is no runtime I/O to fail, the help tree being embedded.
///
/// # Examples
///
/// - `help_command("help;")` — lists all groups and commands
/// - `help_command("help Automata;")` — lists all commands in the `Automata` group
/// - `help_command("help eval;")` — searches for command `eval` across all groups
/// - `help_command("help Automata eval;")` — shows help for `Automata`'s `eval`
pub fn help_command(full_command_line: &str) -> String {
    help_command_tokens(&parse_help_arguments(full_command_line))
}

/// `HelpMessages.parseHelpArguments(fullCommandLine)` (`:77-97`).
///
/// Strips surrounding whitespace, then a trailing `;`, then a case-insensitive leading `help`
/// (plus any whitespace after it), then splits on runs of whitespace.
///
/// Two ported quirks worth naming:
///
/// - The leading-`help` regex is `(?i)^help\s*`, i.e. the whitespace is **optional**, so
///   `helpeval;` tokenizes to `["eval"]` just like `help eval;` does.
/// - The split is on whitespace with no quoting whatsoever, so a group whose *name contains a
///   space* can never be addressed: `help Morphisms And Word Automata promote;` yields five
///   tokens and lands in Java's `Too many arguments` arm. See `docs/WALNUT-BUGS.md` WB-031.
///
/// Java's `\s` is ASCII-only (`[ \t\n\x0B\f\r]`) while `String.strip()` is Unicode-aware; the
/// split below matches Java's ASCII class exactly, and `str::trim` stands in for `strip()`
/// (they differ only on characters like U+00A0, which `Character.isWhitespace` excludes and
/// Rust's `White_Space` property includes — unreachable from a real Walnut command line).
fn parse_help_arguments(full_command_line: &str) -> Vec<&str> {
    // `fullCommandLine.strip()`
    let mut s = full_command_line.trim();

    // `if (s.endsWith(";")) s = s.substring(0, s.length() - 1).strip();`
    if let Some(rest) = s.strip_suffix(';') {
        s = rest.trim();
    }

    // `s = s.replaceFirst("(?i)^help\\s*", "").strip();`
    if s.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("help")) {
        s = s[4..].trim_start_matches(is_java_regex_space);
    }
    s = s.trim();

    // `if (s.isEmpty()) return new String[0];`
    if s.is_empty() {
        return Vec::new();
    }

    // `return s.split("\\s+");`
    s.split(is_java_regex_space)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Java regex `\s` without `UNICODE_CHARACTER_CLASS`: `[ \t\n\x0B\f\r]`.
fn is_java_regex_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// The token-dispatch half of `helpCommand` (`:30-62`), split out so the tokenizer can be
/// tested independently. Not public: real callers hand in a raw command line, and only
/// [`parse_help_arguments`] decides what a token is.
fn help_command_tokens(tokens: &[&str]) -> String {
    let db = help_database();

    match tokens.len() {
        0 => list_all_groups_detailed(db),
        1 => {
            let maybe_group_or_command = tokens[0];
            match db.get(maybe_group_or_command) {
                // `isGroup(...)` — a real subdirectory.
                Some(commands) => list_commands_in_group(maybe_group_or_command, commands),
                // Otherwise assume it is a command name and search every group.
                None => show_command_help_across_all_groups(db, maybe_group_or_command),
            }
        }
        2 => show_command_help(db, tokens[0], tokens[1]),
        _ => "Too many arguments. Usage: help [group] [command];\n".to_string(),
    }
}

// ============================================================================
// Internal functions
// ============================================================================

/// `HelpMessages.sortByNames` (`:137-139`): `Arrays.sort(files, compareToIgnoreCase(getName()))`.
///
/// Two details the obvious `BTreeMap`-iteration-order shortcut gets wrong, both currently
/// invisible on the shipped data but neither guaranteed by anything:
///
/// - Java compares **case-insensitively**; `BTreeMap<&str, _>` compares by code point, so
///   `Zebra` sorts before `apple` there and after it in Java.
/// - Java compares the **file name**, i.e. the command name *plus* `.txt`. That only changes
///   the answer when one name is a prefix of another (`a` vs `a!`: `a.txt` < `a!.txt` by name
///   because `!` < `.`, but `a` < `a!` bare), which today's all-lowercase, no-prefix-pair data
///   never triggers.
///
/// So this implements Java's actual rule rather than relying on the coincidence.
fn sorted_by_java_name<'a>(names: impl Iterator<Item = &'a str>, suffix: &str) -> Vec<&'a str> {
    let mut v: Vec<&'a str> = names.collect();
    v.sort_by(|a, b| compare_to_ignore_case(&format!("{a}{suffix}"), &format!("{b}{suffix}")));
    v
}

/// `String.compareToIgnoreCase`: per code unit, fold to upper then to lower, else compare
/// lengths. Exact for the ASCII names this module sorts (Java folds UTF-16 code units, this
/// folds `char`s; they can only disagree above the BMP, where no help file name lives).
fn compare_to_ignore_case(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (a.chars(), b.chars());
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) => {
                if x != y {
                    let (xu, yu) = (x.to_ascii_uppercase(), y.to_ascii_uppercase());
                    if xu != yu {
                        let (xl, yl) = (xu.to_ascii_lowercase(), yu.to_ascii_lowercase());
                        if xl != yl {
                            return xl.cmp(&yl);
                        }
                    }
                }
            }
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}

/// `HelpMessages.listAllGroupsDetailed` (`:116-135`) — the output of a bare `help;`.
fn list_all_groups_detailed(db: &HelpDatabase) -> String {
    let mut output = String::from("Available help groups and commands:\n");

    for group_name in sorted_by_java_name(db.keys().copied(), "") {
        output.push_str(&format!("Group: {group_name}\n"));

        let commands = &db[group_name];
        // `if (txtFiles != null && txtFiles.length > 0)` — an empty group gets no
        // "  Commands:" header at all. Unreachable on the shipped data (every group has
        // files), ported anyway.
        if !commands.is_empty() {
            output.push_str("  Commands:\n");
            for command_name in sorted_by_java_name(commands.keys().copied(), TXT_EXTENSION) {
                output.push_str(&format!("   - {command_name}\n"));
            }
        }
        output.push('\n');
    }

    output
}

/// `HelpMessages.listCommandsInGroup` (`:146-156`).
///
/// Java's "group does not exist" case cannot reach here: the caller only calls this after
/// `isGroup()` returned true, so `commands` is always a real group's contents.
fn list_commands_in_group(group_name: &str, commands: &CommandMap) -> String {
    let mut output = format!("Commands in group \"{group_name}\":\n");
    for command_name in sorted_by_java_name(commands.keys().copied(), TXT_EXTENSION) {
        output.push_str(&format!(" - {command_name}\n"));
    }
    output
}

/// `HelpMessages.showCommandHelpAcrossAllGroups` (`:172-198`).
///
/// Java iterates `rootDir.listFiles(File::isDirectory)` in **unspecified filesystem order** and
/// takes the first hit ("we assume only one match"). This iterates in sorted order instead; the
/// two agree because no command name appears in two groups, which
/// `no_command_name_appears_in_two_groups` pins.
fn show_command_help_across_all_groups(db: &HelpDatabase, command_name: &str) -> String {
    for commands in db.values() {
        if let Some(help_text) = commands.get(command_name) {
            return format_help_output(command_name, help_text);
        }
    }

    format!("No documentation found for command \"{command_name}\".\n")
}

/// `HelpMessages.showCommandHelp` (`:206-213`) — the `help <group> <command>` mode.
///
/// Java builds the path `<root>/<group>/<command>.txt` and tests `isFile()`, so an unknown
/// *group* and a known group missing that *command* are indistinguishable: both produce the
/// single message below, which names only the command and says "in this group".
fn show_command_help(db: &HelpDatabase, group: &str, command: &str) -> String {
    match db.get(group).and_then(|commands| commands.get(command)) {
        Some(help_text) => format_help_output(command, help_text),
        None => format!("No documentation found for command \"{command}\" in this group.\n"),
    }
}

/// `HelpMessages.printHelpFile` (`:215-224`) — header, the file's lines, footer.
fn format_help_output(command_name: &str, help_text: &str) -> String {
    let mut output = format!("=== Help: {command_name} ===\n");
    for line in java_read_lines(help_text) {
        output.push_str(line);
        output.push('\n');
    }
    output.push_str("=============================\n");
    output
}

/// `BufferedReader.readLine()` in a loop, as a splitter.
///
/// A line is terminated by `\n`, `\r\n`, or a bare `\r`, none of which is included in the
/// returned slice; the terminator at end-of-input does **not** produce a trailing empty line,
/// but unterminated trailing content does produce a final line.
fn java_read_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut start, mut i) = (0usize, 0usize);

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&text[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                lines.push(&text[start..i]);
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------- tokenizer

    #[test]
    fn parse_help_arguments_ports_javas_stripping_and_splitting() {
        assert_eq!(parse_help_arguments("help;"), Vec::<&str>::new());
        assert_eq!(parse_help_arguments("  help  ;  "), Vec::<&str>::new());
        assert_eq!(parse_help_arguments("help"), Vec::<&str>::new());
        assert_eq!(parse_help_arguments("HELP eval;"), vec!["eval"]);
        assert_eq!(
            parse_help_arguments("Help\tAutomata\t eval ;"),
            vec!["Automata", "eval"]
        );
        assert_eq!(parse_help_arguments("help a b c;"), vec!["a", "b", "c"]);
        // Only ONE trailing semicolon is removed (Java's `if`, not a loop).
        assert_eq!(parse_help_arguments("help eval;;"), vec!["eval;"]);
    }

    /// `(?i)^help\s*` — the whitespace after `help` is optional, so the prefix is eaten
    /// regardless and a word merely *starting* with `help` loses its first four letters.
    ///
    /// Unreachable through the real REPL, which is why this tests the tokenizer directly:
    /// `Prover` extracts the command name first and rejects anything that is not exactly one
    /// of `RE_FOR_THE_LIST_OF_CMDS`, so real Walnut answers `helpeval;` with
    /// `No such command exists.` (verified against the `walnut-java` CLI).
    #[test]
    fn parse_help_arguments_ports_the_optional_space_after_help() {
        assert_eq!(parse_help_arguments("helpeval;"), vec!["eval"]);
        assert_eq!(parse_help_arguments("helper;"), vec!["er"]);
    }

    #[test]
    fn parse_help_arguments_leaves_a_non_help_line_alone() {
        // No leading "help": nothing is stripped but the semicolon.
        assert_eq!(
            parse_help_arguments("hel p eval;"),
            vec!["hel", "p", "eval"]
        );
    }

    #[test]
    fn parse_help_arguments_is_byte_safe_on_short_and_multibyte_input() {
        // `s.get(..4)` must not panic on a 1-3 byte line or split a multi-byte char.
        assert_eq!(parse_help_arguments(""), Vec::<&str>::new());
        assert_eq!(parse_help_arguments("h;"), vec!["h"]);
        assert_eq!(parse_help_arguments("héllo;"), vec!["héllo"]);
    }

    // ------------------------------------------------------------- readLine port

    #[test]
    fn java_read_lines_matches_bufferedreader_semantics() {
        assert_eq!(java_read_lines("a\r\nb\r\n"), vec!["a", "b"]);
        assert_eq!(java_read_lines("a\nb"), vec!["a", "b"]);
        assert_eq!(java_read_lines("a\rb\r"), vec!["a", "b"]);
        assert_eq!(java_read_lines(""), Vec::<&str>::new());
        assert_eq!(java_read_lines("\n"), vec![""]);
        assert_eq!(java_read_lines("a\n\nb\n"), vec!["a", "", "b"]);
    }

    #[test]
    fn embedded_help_text_never_leaks_a_carriage_return_into_the_output() {
        for (group, commands) in help_database() {
            for command in commands.keys() {
                let output = help_command_tokens(&[group, command]);
                assert!(
                    !output.contains('\r'),
                    "stray CR in help output for {group}/{command}"
                );
                assert!(output.ends_with("=============================\n"));
            }
        }
    }

    // -------------------------------------------------------------- sort order

    #[test]
    fn compare_to_ignore_case_matches_java() {
        assert_eq!(compare_to_ignore_case("Zebra", "apple"), Ordering::Greater);
        assert_eq!(compare_to_ignore_case("apple", "APPLE"), Ordering::Equal);
        assert_eq!(compare_to_ignore_case("a", "ab"), Ordering::Less);
        // The `.txt` suffix is load-bearing when one name is a prefix of the other.
        assert_eq!(compare_to_ignore_case("a.txt", "a!.txt"), Ordering::Greater);
        assert_eq!(compare_to_ignore_case("a", "a!"), Ordering::Less);
    }

    // --------------------------------------------------------------- the modes

    #[test]
    fn test_help_no_args_lists_all_groups() {
        let output = help_command("help;");
        assert!(output.contains("Available help groups and commands:"));
        // Check that major groups are listed
        assert!(output.contains("Automata"));
        assert!(output.contains("Walnut"));
        assert!(output.contains("Metacommands"));
    }

    #[test]
    fn test_help_no_args_lists_sample_commands() {
        let output = help_command("help;");
        // Some representative commands should appear
        assert!(output.contains("eval"));
        assert!(output.contains("combine"));
        assert!(output.contains("promote"));
    }

    #[test]
    fn help_no_args_output_is_exact() {
        let output = help_command("help;");
        assert_eq!(
            output,
            "Available help groups and commands:\n\
             Group: Automata\n  Commands:\n\
             \x20  - combine\n   - concat\n   - convert\n   - def\n   - describe\n\
             \x20  - eval\n   - fixleadzero\n   - fixtrailzero\n   - inf\n   - intersect\n\
             \x20  - leftquo\n   - ost\n   - reg\n   - reverse\n   - rightquo\n   - star\n\
             \x20  - test\n   - transduce\n   - union\n\n\
             Group: Metacommands\n  Commands:\n   - [export]\n   - [strategy]\n\n\
             Group: Morphisms And Word Automata\n  Commands:\n\
             \x20  - alphabet\n   - image\n   - join\n   - minimize\n   - morphism\n\
             \x20  - promote\n   - rsplit\n   - split\n\n\
             Group: Walnut\n  Commands:\n\
             \x20  - cls\n   - export\n   - load\n   - macro\n   - quit\n\n"
        );
    }

    #[test]
    fn test_help_group_only_lists_commands_in_group() {
        let output = help_command("help Automata;");
        assert!(output.contains("Commands in group \"Automata\""));
        assert!(output.contains("eval"));
        assert!(output.contains("combine"));
        // Should not list commands from other groups
        assert!(!output.contains("promote"));
    }

    #[test]
    fn help_group_only_output_is_exact() {
        assert_eq!(
            help_command("help Walnut;"),
            "Commands in group \"Walnut\":\n - cls\n - export\n - load\n - macro\n - quit\n"
        );
    }

    #[test]
    fn test_help_single_command_searches_all_groups() {
        let output = help_command("help eval;");
        assert!(output.contains("=== Help: eval ==="));
        assert!(output.contains("identical to the \"def\" command"));
    }

    #[test]
    fn test_help_combine_command() {
        let output = help_command("help combine;");
        assert!(output.contains("=== Help: combine ==="));
        assert!(output.contains("highest index automaton"));
    }

    #[test]
    fn test_help_group_and_command() {
        let output = help_command("help Automata eval;");
        assert!(output.contains("=== Help: eval ==="));
    }

    #[test]
    fn test_help_unknown_command() {
        assert_eq!(
            help_command("help nonexistent;"),
            "No documentation found for command \"nonexistent\".\n"
        );
    }

    /// Java has NO distinct "unknown group" message: `showCommandHelp` just fails `isFile()`
    /// on `<root>/<group>/<command>.txt` and prints the one message below, naming only the
    /// command. An unknown group and a known group missing that command are identical.
    #[test]
    fn test_help_unknown_group() {
        let expected = "No documentation found for command \"somecommand\" in this group.\n";
        assert_eq!(help_command("help NonexistentGroup somecommand;"), expected);
        // Known group, unknown command -> byte-identical message.
        assert_eq!(
            help_command("help Automata somecommand;"),
            "No documentation found for command \"somecommand\" in this group.\n"
        );
    }

    #[test]
    fn test_help_too_many_args() {
        assert_eq!(
            help_command("help a b c;"),
            "Too many arguments. Usage: help [group] [command];\n"
        );
    }

    /// WB-031: the `Morphisms And Word Automata` group name contains spaces, and
    /// `parseHelpArguments` splits on whitespace with no quoting, so the group's two- and
    /// one-token help modes are **unreachable** from a real Walnut command line — the group
    /// name alone already tokenizes to four arguments.
    #[test]
    fn test_help_promote_from_morphisms_group() {
        assert_eq!(
            help_command("help Morphisms And Word Automata promote;"),
            "Too many arguments. Usage: help [group] [command];\n"
        );
        assert_eq!(
            help_command("help Morphisms And Word Automata;"),
            "Too many arguments. Usage: help [group] [command];\n"
        );
        // The single-token search mode is how the group's commands are actually reachable.
        assert!(help_command("help promote;").contains("=== Help: promote ==="));

        // The two-token dispatch itself is correct; it simply has no reachable caller for
        // this group. Exercised through the internal helper, not the real entry point.
        let output = help_command_tokens(&["Morphisms And Word Automata", "promote"]);
        assert!(output.contains("=== Help: promote ==="));
        assert!(output.contains("morphism"));
    }

    #[test]
    fn test_help_metacommand_bracket_syntax() {
        let output = help_command("help [export];");
        assert!(output.contains("=== Help: [export] ==="));
    }

    #[test]
    fn test_help_walnut_quit_command() {
        let output = help_command("help Walnut quit;");
        assert!(output.contains("=== Help: quit ==="));
    }

    // ------------------------------------------------------- exact-output goldens

    /// `combine.txt` is CRLF *and* ends with a trailing newline — the two defects the raw
    /// `include_str!` splice produced (stray `\r`s, a spurious blank line before the footer).
    #[test]
    fn help_combine_output_is_exact() {
        assert_eq!(
            help_command("help Automata combine;"),
            "=== Help: combine ===\n\
             The \"combine\" command produces a DFAO whose output on a given input corresponds to the highest index automaton in the list supplied that accepts said input. The syntax for the \"combine\" command is:\n\
             \n\
             \tcombine <new> <automaton exp> ... <automaton exp>\n\
             \n\
             Results saved in: Result/, Word Automata Library/.\n\
             \n\
             An automaton expression is either the name of an automaton on its own, (eg. myAutomaton) or the name with a value assigned by an equals sign (eg. myAutomaton=3). Each automaton is assumed to be in \"Automata Library/\". Walnut assigns a default value equal to the index of the automaton in the list, beginning with 1. For example,\n\
             \n\
             \tcombine A A1 A2=10 A3 \n\
             \t\n\
             produces the same output as\n\
             \n\
             \tcombine A A1=1 A2=10 A3=3 \n\
             \n\
             This output is a DFAO called A that outputs 0 if none of A1, A2, or A3 accepts an input, 1 if A1 accepts but A2 and A3 do not, 10 if A2 accepts but A3 does not, and 3 if A3 accepts.\n\
             =============================\n"
        );
    }

    /// WB-030: `image.txt` is cp1252 upstream; Java's UTF-8 `FileReader` turns its `0x96`
    /// (an en dash, `164–192`) into `U+FFFD`. This pins Walnut's real output, mojibake included.
    #[test]
    fn help_image_output_is_exact_including_the_wb_030_replacement_char() {
        assert_eq!(
            help_command("help image;"),
            "=== Help: image ===\n\
             The \"image\" command applies a uniform morphism to a DFAO to produce a new DFAO. The syntax is as follows:\n\
             \n\
             \timage <name> <morphism> <DFAO>\n\
             \t\n\
             Results saved in: Result/, Word Automata Library/.\n\
             \n\
             Walnut's procedure to apply the uniform morphism is as per Cobham, A. Uniform tag sequences. Math. Systems Theory 6, 164\u{FFFD}192 (1972). https://doi.org/10.1007/BF01706087.\n\
             \n\
             If the morphism supplied is not uniform, an error will be produced.\n\
             =============================\n"
        );
    }

    /// WB-030 again: `reg.txt` carries three bad bytes — `0x85` (an intended `…`) on line 3 and
    /// two `0x92`s (intended `’`) on line 13. `0x85` is also why `file(1)` calls the raw file
    /// "NEL line terminators"; Java's UTF-8 reader never sees a NEL, so the line count is 21.
    #[test]
    fn help_reg_output_is_exact_including_the_wb_030_replacement_chars() {
        assert_eq!(
            help_command("help reg;"),
            "=== Help: reg ===\n\
             The \"reg\" command creates an automaton based on a specified regular expression. The syntax for the \"reg\" command is:\n\
             \n\
             \treg <name> <number system/alphabet> \u{FFFD} <number system/alphabet> \"<regular expression>\"\n\
             \n\
             Results saved in: Result/, Automata Library/.\n\
             \n\
             Regular expressions consist of regular operations such as:\n\
             \t- OR (|)\n\
             \t- AND (&)\n\
             \t- concatenation\n\
             \t- Kleene star (*)\n\
             \t\n\
             The alphabet for the regular expressions are arbitrary tuples of integers. One may define regular expressions on an alphabet of 0-9, but can also specify, for example, [1, -1, 1][0, 1, -1]* to mean 10* in the first coordinate, -11* in the second, and 1-1* in the third. In particular, one can specify numbers above 9 and below 0, provided brackets are used (eg. 1[10]* is 1 followed by an arbitrary number of 10\u{FFFD}s and similar for 1[-1]*). Brackets may also be used for numbers 0-9 (eg. [1][10]*) but are not mandatory in this case. A number system or alphabet must be supplied for each coordinate in the expression\u{FFFD}s tuples.\n\
             \n\
             Sample usages of the \"reg\" command:\n\
             \n\
             \treg foo {-1,0,1} \"(1[-1])*0*|(1[-1])*10*\"\n\
             \t\n\
             \treg bar msd_2 msd_2 \"[1, 0][0, 0]*\"\n\
             \t\n\
             \treg tmp msd_3 \"((012)*2*)|((012)*01)\"\n\
             =============================\n"
        );
    }

    // ------------------------------------------------------------ data integrity

    /// Every mode's returned string is newline-terminated (the module's stated convention),
    /// and only the bare-`help;` mode ends in a blank line — because Java's
    /// `listAllGroupsDetailed` really does `println()` after every group.
    #[test]
    fn every_mode_terminates_with_a_newline() {
        for line in [
            "help Automata;",
            "help eval;",
            "help Automata eval;",
            "help nonexistent;",
            "help Automata nonexistent;",
            "help a b c;",
        ] {
            let output = help_command(line);
            assert!(output.ends_with('\n'), "{line}: missing trailing newline");
            assert!(
                !output.ends_with("\n\n"),
                "{line}: spurious trailing blank line"
            );
        }
        let all = help_command("help;");
        assert!(all.ends_with("   - quit\n\n"));
        assert!(!all.ends_with("\n\n\n"));
    }

    #[test]
    fn no_command_name_appears_in_two_groups() {
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
        for (group, commands) in help_database() {
            for command in commands.keys() {
                if let Some(other) = seen.insert(command, group) {
                    panic!("command {command} is in both {other} and {group}");
                }
            }
        }
    }

    /// The embedded database must stay in sync with the on-disk help tree: a `.txt` added to
    /// `data/help/Commands/` without a matching `include_str!` would otherwise 404 silently.
    #[test]
    fn embedded_database_matches_the_on_disk_help_tree() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/help/Commands");

        let mut on_disk: Vec<(String, String)> = Vec::new();
        for group in std::fs::read_dir(&root).expect("help tree missing") {
            let group = group.expect("unreadable group entry");
            if !group.file_type().expect("no file type").is_dir() {
                continue;
            }
            let group_name = group.file_name().to_string_lossy().into_owned();
            for file in std::fs::read_dir(group.path()).expect("unreadable group") {
                let file = file.expect("unreadable help entry");
                let name = file.file_name().to_string_lossy().into_owned();
                if let Some(command) = name.strip_suffix(TXT_EXTENSION) {
                    on_disk.push((group_name.clone(), command.to_string()));
                }
            }
        }
        on_disk.sort();

        let mut embedded: Vec<(String, String)> = help_database()
            .iter()
            .flat_map(|(group, commands)| {
                commands
                    .keys()
                    .map(move |command| ((*group).to_string(), (*command).to_string()))
            })
            .collect();
        embedded.sort();

        assert_eq!(on_disk, embedded);
    }
}
