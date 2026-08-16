// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Shared machinery for the **Tier-1 golden-corpus harness** (`docs/DESIGN.md` §5, Tier 1;
//! Phase 3b's U27). See `../golden_corpus.rs` for the harness itself and for the overall
//! design rationale — this module holds the pieces that are worth unit-testing on their own
//! (Java's message normalization, the manifest loader, the fixture-selection rules).
//!
//! Everything here is a port of, or a direct counterpart to, something in
//! `walnut-java/src/test/java/Main/IntegrationTest.java` — line references throughout are to
//! that file unless stated otherwise.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Locating the corpus
// ---------------------------------------------------------------------------

/// The sibling `walnut-java` checkout that owns the corpus (`CLAUDE.md`, "The three-repo
/// world": `walnut-java` is *the* oracle repo and the home of the golden corpus). Overridable
/// with `WALNUT_JAVA_DIR` for a checkout that does not sit next to this one.
///
/// The corpus is deliberately **not vendored** into this repo: it is ~9 MB across ~1,400
/// machine-generated files that `walnut-java` regenerates whenever its own
/// `IntegrationTest.createTestCases` is re-run, so a copy here would be a silent
/// staleness trap (and would bury this unit's real diff). The cost is that a checkout
/// without the sibling repo cannot run Tier 1 — which [`corpus_root`]'s `None` makes
/// loud rather than silent (see `../golden_corpus.rs`).
pub fn walnut_java_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WALNUT_JAVA_DIR") {
        return PathBuf::from(dir);
    }
    // `CARGO_MANIFEST_DIR` is `<walnut-rs>/tests/golden` for an ordinary checkout, but
    // `<walnut-rs>/.claude/worktrees/<name>/tests/golden` when an agent works in a git
    // worktree — so walk UP looking for a directory that has `walnut-java` beside it,
    // rather than hard-coding a fixed number of `..` hops.
    let start = Path::new(env!("CARGO_MANIFEST_DIR"));
    for ancestor in start.ancestors() {
        let candidate = ancestor.join("walnut-java");
        if candidate
            .join("src/test/resources/integrationTests")
            .is_dir()
        {
            return candidate;
        }
    }
    start.join("../../..").join("walnut-java")
}

/// `UtilityMethods.ADDRESS_FOR_UNIT_TEST_INTEGRATION_TEST_RESULTS`
/// (= `Session.getAddressForIntegrationTestResults()`, `Session.java:159-161`) — the directory
/// holding both the recorded expectations (`automaton<i>.txt`, `details<i>.txt`,
/// `error<i>.txt`, `gv<i>.gv`, `automaton<i>.{mpl,m,wl,sage}`) and the `Global`/`Session`
/// library trees the run reads from.
///
/// `None` when the sibling checkout is absent or does not contain the corpus.
pub fn corpus_root() -> Option<PathBuf> {
    let root = walnut_java_dir().join("src/test/resources/integrationTests");
    if root.join("Global").is_dir() && root.join("automaton0.txt").is_file() {
        Some(root)
    } else {
        None
    }
}

/// The two Phase-0 manifests, in the sibling repo's `phase0-artifacts/`.
pub fn manifest_paths() -> (PathBuf, PathBuf) {
    let base = walnut_java_dir().join("phase0-artifacts");
    (
        base.join("test-manifest.json"),
        base.join("subset-filter.json"),
    )
}

// ---------------------------------------------------------------------------
// The manifests
// ---------------------------------------------------------------------------

/// One row of `phase0-artifacts/test-manifest.json`'s `fixtures` list, joined with the
/// matching row of `subset-filter.json` (same `id` space, both 675 long).
#[derive(Debug, Clone)]
pub struct Fixture {
    pub id: usize,
    /// The literal command string `IntegrationTest`'s `L` list holds, i.e. exactly what
    /// `runSpecificTest` feeds to `dispatchForIntegrationTest` (`:927`).
    pub command_script: String,
    /// `automaton` / `automaton_repr` / `details` / `error`. Used only for the loader's own
    /// self-check — the comparison itself is driven by which expectation FILES exist, exactly
    /// as `loadTestCases` (`:1015-1046`) does.
    pub expected_kind: Vec<String>,
    /// `false` iff `subset-filter.json` marked the fixture DROP-scope.
    pub subset_relevant: bool,
    /// `subset-filter.json`'s `drop_reason` list (empty when `subset_relevant`).
    pub drop_reason: Vec<String>,
}

/// Loads and joins both manifests, and **self-checks the parse** against the counts the
/// manifests themselves declare (`fixture_count`, `kind_counts`, `subset_relevant_count`,
/// `drop_reason_counts`) — a mis-parse fails here rather than silently shrinking Tier 1's
/// coverage.
pub fn load_fixtures() -> Result<Vec<Fixture>, String> {
    let (manifest_path, filter_path) = manifest_paths();
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?,
    )
    .map_err(|e| format!("parsing {}: {e}", manifest_path.display()))?;
    let filter: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&filter_path)
            .map_err(|e| format!("reading {}: {e}", filter_path.display()))?,
    )
    .map_err(|e| format!("parsing {}: {e}", filter_path.display()))?;

    let rows = manifest["fixtures"]
        .as_array()
        .ok_or("test-manifest.json: `fixtures` is not an array")?;
    let filter_rows = filter["fixtures"]
        .as_array()
        .ok_or("subset-filter.json: `fixtures` is not an array")?;

    let mut filter_by_id: BTreeMap<usize, (bool, Vec<String>)> = BTreeMap::new();
    for row in filter_rows {
        let id = row["id"].as_u64().ok_or("subset-filter.json: missing id")? as usize;
        let relevant = row["subset_relevant"]
            .as_bool()
            .ok_or("subset-filter.json: missing subset_relevant")?;
        let reasons = string_list(&row["drop_reason"]);
        filter_by_id.insert(id, (relevant, reasons));
    }

    let mut fixtures = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row["id"].as_u64().ok_or("test-manifest.json: missing id")? as usize;
        let (subset_relevant, drop_reason) = filter_by_id
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("subset-filter.json has no row for fixture {id}"))?;
        fixtures.push(Fixture {
            id,
            command_script: row["command_script"]
                .as_str()
                .ok_or_else(|| format!("test-manifest.json: fixture {id} has no command_script"))?
                .to_string(),
            expected_kind: string_list(&row["expected_kind"]),
            subset_relevant,
            drop_reason,
        });
    }

    // -- self-checks against the manifests' own declared counts ---------------
    check_count(
        "test-manifest.json fixture_count",
        manifest["fixture_count"].as_u64(),
        fixtures.len(),
    )?;
    check_count(
        "subset-filter.json fixture_count",
        filter["fixture_count"].as_u64(),
        fixtures.len(),
    )?;
    check_count(
        "subset-filter.json subset_relevant_count",
        filter["subset_relevant_count"].as_u64(),
        fixtures.iter().filter(|f| f.subset_relevant).count(),
    )?;
    if let Some(kind_counts) = manifest["kind_counts"].as_object() {
        for (kind, expected) in kind_counts {
            let actual = fixtures
                .iter()
                .filter(|f| f.expected_kind.iter().any(|k| k == kind))
                .count();
            check_count(&format!("kind_counts[{kind}]"), expected.as_u64(), actual)?;
        }
    }
    if fixtures.iter().enumerate().any(|(i, f)| f.id != i) {
        return Err("test-manifest.json: fixture ids are not 0..n in order".to_string());
    }
    Ok(fixtures)
}

fn check_count(what: &str, declared: Option<u64>, actual: usize) -> Result<(), String> {
    match declared {
        Some(d) if d as usize == actual => Ok(()),
        Some(d) => Err(format!(
            "manifest self-check failed: {what} declares {d}, the loader found {actual}"
        )),
        None => Err(format!("manifest self-check failed: {what} is missing")),
    }
}

fn string_list(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Selection: which subset-relevant fixtures are still excluded, and why
// ---------------------------------------------------------------------------

/// Why a fixture is executed for its session side effects but **not** asserted on.
///
/// Every variant is recorded in the run's report against the specific fixture id — nothing is
/// dropped silently (`CLAUDE.md`, test-performance guardrails: "an over-budget case is
/// recorded as `skip-too-big` **with its cap, visibly** — a silent drop reads as 'covered'
/// when it isn't").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Excluded {
    /// `subset-filter.json` says DROP scope (negative-base numeration, or a `split`/`rsplit`/
    /// `ost` command). Carries the filter's own reason strings.
    DropScope(Vec<String>),
    /// Uses one of the deferred on-the-fly determinization strategies. The whole OTF family
    /// (`CCL`/`CCLS`/`BRZ_CCL`/`BRZ_CCLS`/`OTF()`) is an explicit, already-decided deferral —
    /// `docs/DESIGN.md` §9/§10 and `walnut-java/phase0-artifacts/PROGRESS.md`'s Item 7.
    /// `SC` (the default) and plain `BRZ` are in scope and are NOT excluded.
    OtfStrategy(String),
    /// Subset-relevant itself, but consumes a library automaton that only a DROP-scope
    /// fixture could have produced (`docs/BOUNDARY-MAP.md` §4.2). Detected structurally, not
    /// hard-coded: see [`transitively_dropped`].
    TransitiveDropDependency(Vec<String>),
}

impl std::fmt::Display for Excluded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Excluded::DropScope(reasons) => write!(f, "skip-drop-scope[{}]", reasons.join(",")),
            Excluded::OtfStrategy(s) => write!(f, "skip-otf-deferred[{s}]"),
            Excluded::TransitiveDropDependency(names) => {
                write!(f, "skip-transitive-drop-dep[{}]", names.join(","))
            }
        }
    }
}

/// The deferred OTF determinization strategy names, as they appear inside a `[strategy N X]`
/// metacommand. `BRZ_CCL`/`BRZ_CCLS` are listed before `CCL`/`CCLS` only so a report string
/// names the longest matching token.
pub const DEFERRED_OTF_STRATEGIES: [&str; 4] = ["BRZ_CCLS", "BRZ_CCL", "CCLS", "CCL"];

/// The bodies of every leading `[...]` metacommand block, in order (`"strategy 6 BRZ"`,
/// `"export 1 BA"`, …). Empty for a command with no metacommand prefix. Mirrors
/// `MetaCommands`' own grammar loosely enough to classify, not to execute.
pub fn declared_metacommands(command_script: &str) -> Vec<String> {
    let mut rest = command_script.trim_start();
    let mut out = Vec::new();
    while rest.starts_with('[') {
        let Some(end) = rest.find(']') else { break };
        out.push(rest[1..end].trim().to_string());
        rest = rest[end + 1..].trim_start();
    }
    out
}

fn declared_strategy(command_script: &str) -> Option<String> {
    declared_metacommands(command_script)
        .into_iter()
        .find(|m| m.starts_with("strategy"))
        .map(|m| m["strategy".len()..].trim().to_string())
}

/// `true` iff the command asks for one of the deferred OTF strategies.
pub fn deferred_otf_strategy(command_script: &str) -> Option<String> {
    let decl = declared_strategy(command_script)?;
    // The token after the automaton index, e.g. "6 BRZ_CCLS" -> "BRZ_CCLS".
    let name = decl.split_whitespace().last()?.to_uppercase();
    DEFERRED_OTF_STRATEGIES
        .iter()
        .find(|s| **s == name)
        .map(|s| s.to_string())
}

/// The commands whose SECOND token is the name of a library entry the command **creates**
/// (Walnut's `<cmd> <newname> <args…>` shape). Every one of them writes
/// `<library>/<newname>.txt` (or `.mpl`/`.m`, for a morphism/macro) on success.
///
/// Split out of [`NON_DEFINING_COMMANDS`] deliberately — see that constant.
pub const DEFINING_COMMANDS: [&str; 25] = [
    "alphabet",
    "combine",
    "concat",
    "convert",
    "def",
    "eval",
    "fixleadzero",
    "fixtrailzero",
    "image",
    "intersect",
    "join",
    "leftquo",
    "macro",
    "minimize",
    "morphism",
    "ost",
    "promote",
    "reg",
    "reverse",
    "rightquo",
    "rsplit",
    "split",
    "star",
    "transduce",
    "union",
];

/// The rest of Walnut's dispatch table: commands whose second token is **not** a library
/// entry they produce — either because they only READ the named entry (`describe`/`inf`/
/// `test`/`export`), because the token is a file name (`load`) or a command name
/// (`help`), or because there is no second token at all (`exit`/`quit`/`cls`/`clear`).
///
/// # Why this split exists at all
///
/// [`defined_name`]'s answer feeds [`transitively_dropped`] twice: it builds the
/// "name → produced by a DROP fixture / by a kept fixture" tables, and it tells the scan
/// which identifier in a command to *skip* (a fixture may reference the name it is itself
/// defining). Take the second token of every command unconditionally and `describe GG;`
/// reports that it defines `GG` — so a hypothetical `describe X;` whose `X` only a DROP-scope
/// fixture produced would have its one offending identifier skipped, be classified "not a
/// transitive drop dependency", and get executed and compared against an automaton that was
/// never built. That is precisely the class of silent mis-verdict [`transitively_dropped`]
/// exists to prevent, so the read-only commands are excluded by name rather than by luck.
///
/// (No fixture in the *current* corpus is affected — `describe`'s two live targets, `GG` and
/// `$diffbyone`, are both `Global`-seeded rather than corpus-produced. The rule is a
/// classifier invariant, not a bug fix for today's manifest, and
/// `every_dispatchable_command_is_classified` keeps it honest if `walnut-java` regenerates
/// the corpus with new fixtures.)
pub const NON_DEFINING_COMMANDS: [&str; 10] = [
    "clear", "cls", "describe", "exit", "export", "help", "inf", "load", "quit", "test",
];

/// The library name a fixture *defines* — the second whitespace-delimited token of the
/// command, after any leading metacommand block, **when the command is one that creates a
/// library entry** ([`DEFINING_COMMANDS`]).
///
/// `None` for a read-only command ([`NON_DEFINING_COMMANDS`]), for a command with no second
/// token, and for anything outside Walnut's dispatch table (`invalidcommand;`, fixture 616) —
/// none of which produce a name.
///
/// # The leading `$`
///
/// One [`DEFINING_COMMANDS`] entry — `convert` — may have its created name written with a bare
/// `$` glued straight onto the identifier: `convert $aut msd_2 AUT;` (fixture 667). Java parses
/// the two with *adjacent* capture groups, `GROUP_CONVERT_NEW_DOLLAR_SIGN` then
/// `GROUP_CONVERT_NEW_NAME` (`Prover.java:180-181`), and writes the library entry under the
/// name alone; the `$` only selects the Macro/Automata library. So the sigil is stripped here
/// before the identifier is read — without that, `take_while` stops on the `$` immediately,
/// yields an empty string, and the fixture is reported as defining *nothing*. That would keep
/// its own created name out of `kept_names` and out of [`transitively_dropped`]'s
/// self-reference skip, which is exactly the misclassification that scan exists to prevent.
pub fn defined_name(command_script: &str) -> Option<String> {
    let mut rest = command_script.trim_start();
    if rest.starts_with('[') {
        // Skip every leading metacommand block: `[strategy 6 BRZ][export 1 BA]eval ...`.
        while rest.starts_with('[') {
            let end = rest.find(']')?;
            rest = rest[end + 1..].trim_start();
        }
    }
    let mut tokens = rest.split_whitespace();
    let command = tokens.next()?;
    // `eval test0 "a=4";` splits cleanly, but `describe GG;`'s command token is glued to
    // nothing while `exit;` carries its terminator — strip the terminators Java's
    // `parseSetup` would have removed before comparing.
    let command = command.trim_end_matches([';', ':']);
    if !DEFINING_COMMANDS.contains(&command) {
        return None;
    }
    let name = tokens.next()?;
    // `convert $blah msd_2 foo;`: Java's DOLLAR_SIGN group is separate from the NAME group.
    let name = name.strip_prefix('$').unwrap_or(name);
    // `test450[+][]` / `test444[a][b]`: the bare identifier is the library file name.
    let ident: String = name
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if ident.is_empty() {
        None
    } else {
        Some(ident)
    }
}

/// The set of subset-relevant fixtures whose command references a library name that **only a
/// DROP-scope fixture defines**, mapped to the offending names.
///
/// This is `docs/BOUNDARY-MAP.md` §4.2's transitive-dependency rule, computed from the
/// manifest rather than hard-coded: a name is "DROP-produced" iff some DROP-scope fixture
/// defines it and no subset-relevant fixture does. A consumer of such a name cannot possibly
/// reproduce Walnut's recorded output, because the input it needs was never built.
///
/// # Why the seeded `Global` library does NOT count as "the name is available"
///
/// An earlier draft of this rule exempted a name that already exists in the seeded `Global`
/// tree, on the theory that the fixture could read the seed copy instead. Concretely wrong,
/// and the corpus proves it: `Global/Word Automata Library/test444.txt` exists — but it is a
/// leftover from a previous Java run of fixture 444, `rsplit test444[+][+] test440;`, and its
/// header is `msd_neg_2`. Negative-base numeration is exactly what makes 444 DROP-scope, so
/// the seed copy is itself DROP-scope output and this port cannot even read it
/// (`UnsupportedNegativeBase`). A name a DROP fixture defines is unavailable no matter which
/// copy is on disk.
///
/// `global_library_names` is therefore no longer consulted for the availability decision; it
/// is kept as a parameter (and asserted against) purely so the harness can report when the two
/// notions disagree.
pub fn transitively_dropped(
    fixtures: &[Fixture],
    global_library_names: &BTreeSet<String>,
) -> BTreeMap<usize, Vec<String>> {
    let mut dropped_names: BTreeSet<String> = BTreeSet::new();
    let mut kept_names: BTreeSet<String> = BTreeSet::new();
    for f in fixtures {
        if let Some(name) = defined_name(&f.command_script) {
            if f.subset_relevant {
                kept_names.insert(name);
            } else {
                dropped_names.insert(name);
            }
        }
    }
    let unavailable: BTreeSet<&String> = dropped_names
        .iter()
        .filter(|n| !kept_names.contains(*n))
        .collect();
    // Informational only — see this function's doc: a stale `Global` copy of a DROP-produced
    // name is itself DROP-scope output, so its presence changes nothing.
    let _seeded_but_still_unavailable = unavailable
        .iter()
        .filter(|n| global_library_names.contains(**n))
        .count();

    let mut out = BTreeMap::new();
    for f in fixtures.iter().filter(|f| f.subset_relevant) {
        let own = defined_name(&f.command_script);
        let mut hits: Vec<String> = Vec::new();
        for token in identifiers(&f.command_script) {
            if Some(&token) == own.as_ref() {
                continue;
            }
            if unavailable.contains(&token) {
                hits.push(token);
            }
        }
        hits.dedup();
        if !hits.is_empty() {
            out.insert(f.id, hits);
        }
    }
    out
}

/// `true` iff `command_script` mentions `name` as a whole identifier (not as a prefix of a
/// longer one — `test63` must not match `test637`).
pub fn references(command_script: &str, name: &str) -> bool {
    identifiers(command_script).iter().any(|t| t == name)
}

/// Every maximal `[A-Za-z][A-Za-z0-9_]*` run in `s`, in order, deduplicated by first
/// appearance. Java's own identifier shape (`Prover.RE_IDENTIFIER`).
fn identifiers(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            if !out.contains(&tok) {
                out.push(tok);
            }
        } else {
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// `IntegrationTest.assertEqualMessages` (`:968-990`)
// ---------------------------------------------------------------------------

/// `String.strip()` — trims by `Character.isWhitespace`, which Rust's `str::trim` (Unicode
/// `White_Space`) matches on every character these fixtures contain. Kept as its own function
/// so the Java call it stands for is visible at each call site.
fn java_strip(s: &str) -> &str {
    s.trim()
}

/// The exact normalization `IntegrationTest.assertEqualMessages` applies to BOTH sides before
/// comparing them with `String.equals` (`:968-980`), in Java's own statement order:
///
/// ```text
/// s = s.strip();
/// s = s.replaceAll(" {2}", " ");        // one non-overlapping pass, NOT a full collapse
/// s = s.replaceAll("\\d+ms", "");       // timings
/// s = s.replaceAll("\\s*Progress:.*", "");
/// ```
///
/// Nothing else is removed. In particular **state counts survive**, including the
/// pre-minimization ones (`Minimizing: 34 states.` / `computed cross product:166 states`) —
/// a `details` fixture therefore pins the port's intermediate automaton sizes and its
/// cross-product traversal exactly, which is a deliberate, already-made decision for this
/// project, not an accident of the normalization.
pub fn normalize_message(s: &str) -> String {
    let s = java_strip(s);
    let s = s.replace("  ", " ");
    let s = strip_millis(&s);
    strip_progress_lines(&s)
}

/// `replaceAll("\\d+ms", "")`. Java's `\d` is ASCII-only here (no
/// `UNICODE_CHARACTER_CLASS`), and greedy `\d+` can only ever match the whole maximal digit
/// run: the character after a maximal run is by definition not a digit, so a shorter
/// (backtracked) match could only succeed if `ms` followed *inside* the run, which is
/// impossible. Hence the single-pass scan below is exactly equivalent.
fn strip_millis(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if b[i..].starts_with(b"ms") {
                i += 2; // drop the digits AND the "ms"
            } else {
                out.push_str(&s[start..i]);
            }
        } else {
            // Multi-byte UTF-8 is copied whole; only ASCII digits can start a match.
            let ch = s[i..].chars().next().expect("index is a char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// `replaceAll("\\s*Progress:.*", "")`. Java's `.` excludes the line terminators
/// `\n \r     `, so the match runs from the start of the whitespace run
/// preceding `Progress:` up to (not including) the next line terminator — the terminator
/// itself survives, joining the previous line to the following one.
fn strip_progress_lines(s: &str) -> String {
    const TERMINATORS: [char; 5] = ['\n', '\r', '\u{0085}', '\u{2028}', '\u{2029}'];
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0usize;
    while let Some(rel) = s[cursor..].find("Progress:") {
        let at = cursor + rel;
        // `\s*` is greedy and the regex engine's leftmost match therefore starts at the first
        // character of the whitespace run immediately before `Progress:`. Java's `\s`
        // (no UNICODE flag) is `[ \t\n\x0B\f\r]`.
        let mut start = at;
        while start > cursor {
            let prev = s[..start]
                .chars()
                .next_back()
                .expect("start > cursor implies a preceding char");
            if matches!(prev, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r') {
                start -= prev.len_utf8();
            } else {
                break;
            }
        }
        out.push_str(&s[cursor..start]);
        let rest = &s[at..];
        let end = rest
            .find(TERMINATORS)
            .map(|i| at + i)
            .unwrap_or_else(|| s.len());
        cursor = end;
    }
    out.push_str(&s[cursor..]);
    out
}

/// Where the harness put its throwaway library trees, and the paths `walnut-java` recorded
/// for the same directories when it produced the corpus.
///
/// `describe` prints the automaton's file address (`describe GG;` → `File location: <addr>`),
/// so the recorded `details656.txt`/`details668.txt` contain
/// `src/test/resources/integrationTests/Session/Word Automata Library/GG.txt` — a path
/// relative to the `walnut-java` checkout root, which is where Java's own harness runs. This
/// harness cannot use that path (it would write into the oracle repo), so it runs against a
/// temp directory instead and rewrites its own prefix back before comparing.
///
/// **This is the only normalization applied beyond `assertEqualMessages`' own, and it cannot
/// mask a port defect:** it substitutes the exact directory strings *this harness chose and
/// passed into `Session::new`* — values the port never computes — for the exact strings Java
/// was given. Every other byte, including the rest of the path, is compared unchanged; a port
/// that printed the wrong file NAME, or the wrong directory within the session, still fails.
///
/// `Clone` because each fixture's worker thread (`golden_corpus.rs`'s `FixtureJob`) owns its
/// own copy — nothing borrowed may cross that boundary.
#[derive(Clone)]
pub struct PathRewrite {
    pub home_dir: String,
    pub session_dir: String,
}

impl PathRewrite {
    const RECORDED_ROOT: &'static str = "src/test/resources/integrationTests/";

    pub fn apply(&self, s: &str) -> String {
        s.replace(
            &self.session_dir,
            &format!("{}Session/", Self::RECORDED_ROOT),
        )
        .replace(&self.home_dir, &format!("{}Global/", Self::RECORDED_ROOT))
    }
}

/// `IntegrationTest.assertEqualMessages(expected, actual)` (`:968-990`) as a `Result` rather
/// than a JUnit assertion: `Ok(())` when the two normalize equal, otherwise a diff message
/// built the same way Java's failure message is (`findFirstDifferingIndex`, `:992-1013`).
pub fn compare_messages(expected: &str, actual: &str, what: &str) -> Result<(), String> {
    let e = normalize_message(expected);
    let a = normalize_message(actual);
    if e == a {
        return Ok(());
    }
    let idx = first_differing_index(&e, &a);
    let ctx = |s: &str, from: usize| -> String {
        let from = floor_char_boundary(s, from);
        let to = floor_char_boundary(s, (from + 220).min(s.len()));
        s[from..to].to_string()
    };
    let start = idx.saturating_sub(60);
    Err(format!(
        "{what} differs at index {idx}\n --- BEFORE:\n{}\n --- EXPECTED:\n{}\n --- ACTUAL:\n{}",
        ctx(&e, floor_char_boundary(&e, start)),
        ctx(&e, idx),
        ctx(&a, idx),
    ))
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i > s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// `IntegrationTest.findFirstDifferingIndex` (`:992-1013`), simplified to the "start of the
/// differing section" the failure message actually uses. Byte-indexed (both inputs are ASCII
/// in this corpus); callers clamp to a char boundary before slicing.
fn first_differing_index(a: &str, b: &str) -> usize {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    while i < ab.len() && i < bb.len() && ab[i] == bb[i] {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------
// Loading the recorded expectations (`IntegrationTest.loadTestCases`, `:1015-1046`)
// ---------------------------------------------------------------------------

/// `AutomatonMatrixWriter.EMITTERS` (`AutomatonMatrixWriter.java:16-18`) in order:
/// Maple, MATLAB, Mathematica, Sage. `loadTestCases` (`:1037-1039`) builds one expected
/// matrix address per emitter, as `automaton<i><extension>`.
pub const MATRIX_EXTENSIONS: [&str; 4] = [".mpl", ".m", ".wl", ".sage"];

/// Everything `loadTestCases` records for one fixture, as raw text/paths — the automaton is
/// left as a path so the caller can read it through the session's own custom-base resolver
/// (Java's `new Automaton(path)` resolves number systems through the static `Session` for
/// exactly the same reason).
#[derive(Debug, Clone)]
pub struct Expected {
    pub automaton_path: Option<PathBuf>,
    pub error: String,
    pub details: String,
    pub graph_viz: String,
    pub matrix_output: Vec<String>,
}

impl Expected {
    /// `true` iff any expected CAS matrix file has content. The CAS incidence-matrix writer is
    /// confirmed DROP scope for this port (`docs/DESIGN.md`; `crates/wr-cli/src/test_case.rs`'s
    /// module docs), so these fixtures get their automaton/details/error compared and their
    /// matrix comparison explicitly skipped and reported.
    pub fn has_cas_matrices(&self) -> bool {
        self.matrix_output.iter().any(|m| !m.trim().is_empty())
    }
}

pub fn load_expected(corpus_root: &Path, id: usize) -> io::Result<Expected> {
    let automaton_path = corpus_root.join(format!("automaton{id}.txt"));
    let mut matrix_output = Vec::with_capacity(MATRIX_EXTENSIONS.len());
    for ext in MATRIX_EXTENSIONS {
        matrix_output.push(read_from_file(
            &corpus_root.join(format!("automaton{id}{ext}")),
        )?);
    }
    Ok(Expected {
        automaton_path: automaton_path.is_file().then_some(automaton_path),
        error: read_from_file(&corpus_root.join(format!("error{id}.txt")))?,
        details: read_from_file(&corpus_root.join(format!("details{id}.txt")))?,
        graph_viz: read_from_file(&corpus_root.join(format!("gv{id}.gv")))?,
        matrix_output,
    })
}

/// `UtilityMethods.readFromFile` — already ported as `wr_core::util::read_from_file`
/// (a missing file reads as `""`, and the trailing newline `writeToFile`'s `println` adds is
/// dropped). Re-exported through a `Path`-taking wrapper for this module's call sites.
fn read_from_file(path: &Path) -> io::Result<String> {
    wr_core::util::read_from_file(path.to_str().unwrap_or_default())
}

// ---------------------------------------------------------------------------
// The session tree (`Session.setPathsAndNamesIntegrationTests` + `clean...`)
// ---------------------------------------------------------------------------

/// The five files `Session.cleanPathsAndNamesIntegrationTest` (`Session.java:103`) explicitly
/// KEEPS in the session tree when it wipes it. They are checked in under
/// `integrationTests/Session/Word Automata Library/`, and because
/// `Session.globalOrSessionFile` prefers a session file over the global one, they really do
/// shadow `Global/Word Automata Library/`'s copies for the whole run (`P.txt` genuinely
/// differs from the global copy — by a blank line — so this is not cosmetic).
pub const SESSION_FILES_KEPT: [&str; 5] = ["P.txt", "PD.txt", "PR.txt", "RS.txt", "T2.txt"];

/// The library subdirectories `Session.createSubdirectories` (`Session.java:123-135`) makes.
pub const SESSION_SUBDIRS: [&str; 6] = [
    "Result",
    "Automata Library",
    "Custom Bases",
    "Macro Library",
    "Morphism Library",
    "Word Automata Library",
];

/// Builds a throwaway home/session tree that mirrors what
/// `Session.setPathsAndNamesIntegrationTests()` + `cleanPathsAndNamesIntegrationTest()` leave
/// behind: `Global/` copied verbatim (the read-only seed library — Walnut never writes
/// there), a `Session/` tree with the six subdirectories and the five kept word-automaton
/// files, and nothing else.
///
/// Returns `(home_dir, session_dir)`, both with the trailing slash `Prover.parseArgs` appends
/// (`SessionPaths` concatenates raw strings, exactly as Java does).
pub fn build_session_tree(corpus_root: &Path, dest: &Path) -> io::Result<(String, String)> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    let home = dest.join("Global");
    copy_dir_all(&corpus_root.join("Global"), &home)?;

    let session = dest.join("Session");
    for sub in SESSION_SUBDIRS {
        fs::create_dir_all(session.join(sub))?;
    }
    for name in SESSION_FILES_KEPT {
        let src = corpus_root
            .join("Session")
            .join("Word Automata Library")
            .join(name);
        if src.is_file() {
            fs::copy(&src, session.join("Word Automata Library").join(name))?;
        }
    }
    Ok((
        format!("{}/", home.to_str().unwrap_or_default()),
        format!("{}/", session.to_str().unwrap_or_default()),
    ))
}

fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Every library file name (without extension) the seeded `Global` tree provides, across the
/// automata/word-automata/morphism/macro libraries — the "already exists, not produced by a
/// fixture" set [`transitively_dropped`] needs.
pub fn global_library_names(home_dir: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for sub in [
        "Automata Library",
        "Word Automata Library",
        "Morphism Library",
        "Macro Library",
        "Transducer Library",
        "Custom Bases",
    ] {
        let dir = Path::new(home_dir).join(sub);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_string());
                }
            }
        }
    }
    names
}

// ---------------------------------------------------------------------------
// `IntegrationTest.initialize()`'s prelude (`:43-78`)
// ---------------------------------------------------------------------------

/// The two `reg` commands `initialize()` runs through `Prover.dispatch` (`:48-49`), followed
/// by the `integTests` list it runs through `dispatchForIntegrationTest`, each on a fresh
/// `Prover` (`:50-74`). Transcribed verbatim, including the `msd_fib`/`lsd_fib` entries — the
/// prelude is what populates the session library every later fixture reads from, so an
/// entry that fails here is itself a finding, not something to quietly prune.
pub const PRELUDE: [&str; 19] = [
    "reg endsIn2Zeros lsd_2 \"(0|1)*00\";",
    "reg startsWith2Zeros msd_2 \"00(0|1)*\";",
    "def thueeq \"T[x]=T[y]\";",
    "def func \"(?msd_3 c < 5) & (a = b+1) & (?msd_10 e = 17)\";",
    "def thuefactoreq \"Ak (k < n) => T[i+k] = T[j+k]\";",
    "def thueuniquepref \"Aj (j > 0 & j < n-m) => ~$thuefactoreq(i,i+j,m)\";",
    "def thueuniquesuff \"Aj (j > 0 & j < n-m) => ~$thuefactoreq(i+n-m,i+n-m-j,m)\";",
    "def thuepriv \"(n >= 1) & Am (m <= n & m >= 1) => (Ep (p <= m & p >= 1) & $thueuniquepref(i,p,m) & $thueuniquesuff(i+n-m,p,m) & $thuefactoreq(i, i+n-p, p))\";",
    "def fibmr \"?msd_fib (i<=j)&(j<n)&Ep ((p>=1)&(2*p+i<=j+1)&(Ak (k+i+p<=j) => (F[i+k]=F[i+k+p]))&((i>=1) => (Aq ((1<=q)&(q<=p)) =>(El (l+i+q<=j+1)&(F[i+l-1]!=F[i+l+q-1]))))&((j+2<=n) => (Ar ((1<=r)&(r<=p)) =>(Em (m+r+i<=j+1)&(F[i+m]!=F[i+m+r])))))\";",
    "load thue_tests.txt;",
    "load rudin_shapiro_tests.txt;",
    "load rudin_shapiro_trapezoidal_tests.txt;",
    "load paperfolding_tests.txt;",
    "load paperfolding_trapezoidal_tests.txt;",
    "load period_doubling_tests.txt;",
    "load fibonacci_tests.txt;",
    "load morphism_image_tests.txt;",
    "macro palindrome \"?%0 Ak (k<n) => %1[i+k] = %1[i+n-1-k]\";",
    "macro border \"?%0 m>=1 & m<=n & $%1_factoreq(i,i+n-m,m)\";",
];

#[cfg(test)]
mod tests {
    use super::*;

    // -- normalize_message ----------------------------------------------------

    #[test]
    fn doubled_spaces_collapse_once_not_repeatedly() {
        // Java's `replaceAll(" {2}", " ")` is a single non-overlapping pass: four spaces
        // become two, NOT one. Getting this wrong would silently make `details` comparison
        // more permissive than Java's.
        assert_eq!(normalize_message("a    b"), "a  b");
        assert_eq!(normalize_message("a   b"), "a  b");
        assert_eq!(normalize_message("a  b"), "a b");
    }

    #[test]
    fn timings_are_stripped_digits_and_unit_together() {
        assert_eq!(
            normalize_message("Minimized:4 states - 0ms."),
            "Minimized:4 states - ."
        );
        assert_eq!(
            normalize_message("Total computation time: 143ms."),
            "Total computation time: ."
        );
    }

    #[test]
    fn a_digit_run_not_followed_by_ms_survives() {
        assert_eq!(normalize_message("166 states"), "166 states");
        assert_eq!(normalize_message("12m"), "12m");
        assert_eq!(normalize_message("1ms2ms3"), "3");
    }

    #[test]
    fn state_counts_including_pre_minimization_ones_are_preserved() {
        let s = "Minimizing: 34 states.\nMinimized:32 states - 0ms.";
        assert_eq!(
            normalize_message(s),
            "Minimizing: 34 states.\nMinimized:32 states - ."
        );
    }

    #[test]
    fn progress_lines_are_stripped_with_their_leading_whitespace() {
        let s = "computing cross product:4 states\n      Progress: Added 100 states - 1ms\ncomputed cross product:166 states";
        // The newline that TERMINATES the progress line survives (Java's `.` never matches a
        // line terminator), so the two surviving lines stay on separate lines.
        assert_eq!(
            normalize_message(s),
            "computing cross product:4 states\ncomputed cross product:166 states"
        );
    }

    #[test]
    fn multiple_progress_lines_are_all_stripped() {
        let s = "a\n Progress: one\nb\n Progress: two\nc";
        assert_eq!(normalize_message(s), "a\nb\nc");
    }

    #[test]
    fn strip_trims_both_ends() {
        assert_eq!(normalize_message("\n  hello \n"), "hello");
    }

    // -- classification -------------------------------------------------------

    #[test]
    fn deferred_otf_strategies_are_recognised_and_brz_and_sc_are_not() {
        assert_eq!(
            deferred_otf_strategy("[strategy 6 CCLS]eval t \"x=x\";"),
            Some("CCLS".to_string())
        );
        assert_eq!(
            deferred_otf_strategy("[strategy 6 BRZ_CCL]eval t \"x=x\";"),
            Some("BRZ_CCL".to_string())
        );
        assert_eq!(
            deferred_otf_strategy("[strategy 6 BRZ]eval t \"x=x\";"),
            None
        );
        assert_eq!(deferred_otf_strategy("eval t \"x=x\";"), None);
        assert_eq!(deferred_otf_strategy("[export 1 BA]eval t \"x=x\";"), None);
    }

    #[test]
    fn defined_name_skips_metacommands_and_track_brackets() {
        assert_eq!(
            defined_name("eval test0 \"a=4\";").as_deref(),
            Some("test0")
        );
        assert_eq!(
            defined_name("[strategy 6 BRZ]eval test637 \"x=x\"::").as_deref(),
            Some("test637")
        );
        assert_eq!(
            defined_name("rsplit test444[+][+] test440;").as_deref(),
            Some("test444")
        );
        assert_eq!(defined_name("exit;"), None);
        // A writer whose shape is not `<cmd> <newname> "<predicate>"`.
        assert_eq!(
            defined_name("minimize test657 GG;").as_deref(),
            Some("test657")
        );
        assert_eq!(
            defined_name("rightquo test590 test588 test589;").as_deref(),
            Some("test590")
        );
    }

    /// `convert`'s created name may carry a bare `$` (fixture 667, `convert $blah msd_2 foo;`).
    /// Java splits the sigil into its own capture group and names the library entry `blah`; a
    /// `take_while` that starts on the `$` returns the empty string instead, which reads as
    /// "this fixture defines nothing" — see [`defined_name`]'s "The leading `$`" section.
    #[test]
    fn a_dollar_prefixed_created_name_keeps_its_identifier() {
        assert_eq!(
            defined_name("convert $blah msd_2 foo;").as_deref(),
            Some("blah")
        );
        assert_eq!(
            defined_name("convert $test_1 lsd_2 $src;").as_deref(),
            Some("test_1")
        );
        // The un-sigiled form is unchanged, and a lone `$` still defines nothing.
        assert_eq!(
            defined_name("convert test540 msd_4 T;").as_deref(),
            Some("test540")
        );
        assert_eq!(defined_name("convert $ msd_2 foo;"), None);
    }

    /// The consequence of the rule above, at the level that matters: a `$`-prefixed *defining*
    /// fixture must be recognised as the producer of its own name — both so the name lands in
    /// `kept_names` (making it available to later fixtures) and so
    /// [`transitively_dropped`]'s self-reference skip fires for the fixture itself. With the
    /// `$` mis-parsed, `converted` looked DROP-produced and fixture 2 was flagged as a
    /// transitive drop dependency even though it consumes a name a KEPT fixture defines.
    #[test]
    fn a_dollar_prefixed_defining_fixture_produces_its_own_name() {
        let fixture = |id: usize, cmd: &str, relevant: bool| Fixture {
            id,
            command_script: cmd.to_string(),
            expected_kind: vec!["automaton".to_string()],
            subset_relevant: relevant,
            drop_reason: if relevant {
                vec![]
            } else {
                vec!["drop_command:rsplit".to_string()]
            },
        };
        let fixtures = vec![
            // A DROP fixture defines `converted` too, so the name is "DROP-produced" unless
            // fixture 1 is correctly credited with defining it as well.
            fixture(0, "rsplit converted[+][+] source;", false),
            fixture(1, "convert $converted msd_2 src;", true),
            fixture(2, "eval later \"$converted(x)\";", true),
        ];
        let flagged = transitively_dropped(&fixtures, &BTreeSet::new());
        assert!(
            !flagged.contains_key(&1),
            "the `convert $converted` fixture DEFINES `converted`; it must never be flagged as \
             consuming a DROP-produced name: {flagged:?}"
        );
        assert!(
            !flagged.contains_key(&2),
            "`converted` is produced by a KEPT fixture, so its consumer is not a transitive \
             drop dependency either: {flagged:?}"
        );
        assert!(
            flagged.is_empty(),
            "nothing else should be flagged: {flagged:?}"
        );

        // Control: with the SAME shape but no kept producer, the consumer IS flagged — so the
        // assertions above are proving the `$` is parsed, not that the rule stopped firing.
        let no_kept_producer = vec![
            fixture(0, "rsplit converted[+][+] source;", false),
            fixture(2, "eval later \"$converted(x)\";", true),
        ];
        let flagged = transitively_dropped(&no_kept_producer, &BTreeSet::new());
        assert_eq!(
            flagged.get(&2).map(Vec::as_slice),
            Some(&["converted".to_string()][..]),
            "without the `convert $converted` producer the rule must still fire"
        );
    }

    /// A **read-only** command's second token is a name the fixture CONSUMES, never one it
    /// produces — see [`NON_DEFINING_COMMANDS`]. Treating `describe GG;` as "defines GG" makes
    /// [`transitively_dropped`] skip the one identifier it needs to inspect.
    #[test]
    fn a_read_only_command_defines_no_name() {
        assert_eq!(defined_name("describe GG;"), None);
        assert_eq!(defined_name("describe $diffbyone;"), None);
        assert_eq!(defined_name("inf test123;"), None);
        assert_eq!(defined_name("test test123 5;"), None);
        assert_eq!(defined_name("export test1 BA;"), None);
        assert_eq!(defined_name("load thue_tests.txt;"), None);
        assert_eq!(defined_name("help eval;"), None);
        // Not a command at all (`invalidcommand;`, fixture 616).
        assert_eq!(defined_name("invalidcommand;"), None);
    }

    /// The regression the rule above exists for: a kept, read-only fixture that consumes a
    /// name only a DROP-scope fixture ever produced MUST be flagged. Before `defined_name`
    /// was gated on [`DEFINING_COMMANDS`], `describe dropped;` was read as *defining*
    /// `dropped`, the identifier scan skipped it as the fixture's own name, and the fixture
    /// was executed and compared against an automaton that was never built.
    #[test]
    fn a_read_only_fixture_consuming_a_drop_produced_name_is_flagged() {
        let fixture = |id: usize, cmd: &str, relevant: bool| Fixture {
            id,
            command_script: cmd.to_string(),
            expected_kind: vec!["automaton".to_string()],
            subset_relevant: relevant,
            drop_reason: if relevant {
                vec![]
            } else {
                vec!["drop_command:rsplit".to_string()]
            },
        };
        let fixtures = vec![
            fixture(0, "rsplit dropped[+][+] source;", false),
            fixture(1, "describe dropped;", true),
            // A kept fixture that consumes a kept name is untouched by the rule.
            fixture(2, "eval kept \"x=x\";", true),
            fixture(3, "describe kept;", true),
        ];
        let flagged = transitively_dropped(&fixtures, &BTreeSet::new());
        assert_eq!(
            flagged.get(&1).map(Vec::as_slice),
            Some(&["dropped".to_string()][..]),
            "the `describe` of a DROP-produced name must be a transitive drop dependency"
        );
        assert!(!flagged.contains_key(&3), "flagged: {flagged:?}");
        assert!(
            !flagged.contains_key(&0),
            "DROP fixtures are not classified here"
        );
    }

    /// Both classification lists together must cover Walnut's whole dispatch table, and
    /// nothing else — otherwise a command this corpus grows later is classified by accident.
    /// Checked against `Prover.RE_FOR_THE_LIST_OF_CMDS` itself, not a transcription of it.
    #[test]
    fn every_dispatchable_command_is_classified() {
        let table = wr_cli::prover::RE_FOR_THE_LIST_OF_CMDS
            .trim_start_matches('(')
            .trim_end_matches(')');
        let dispatchable: BTreeSet<&str> = table.split('|').collect();
        assert!(
            dispatchable.len() > 30,
            "the dispatch table did not parse: {dispatchable:?}"
        );
        let classified: BTreeSet<&str> = DEFINING_COMMANDS
            .iter()
            .chain(NON_DEFINING_COMMANDS.iter())
            .copied()
            .collect();
        assert_eq!(
            classified.len(),
            DEFINING_COMMANDS.len() + NON_DEFINING_COMMANDS.len(),
            "a command is listed as BOTH defining and non-defining"
        );
        assert_eq!(
            dispatchable,
            classified,
            "every dispatchable command must be classified exactly once: \
             unclassified {:?}, stale {:?}",
            dispatchable.difference(&classified).collect::<Vec<_>>(),
            classified.difference(&dispatchable).collect::<Vec<_>>()
        );
    }

    // -- PathRewrite ----------------------------------------------------------

    #[test]
    fn path_rewrite_maps_only_the_harness_chosen_directories() {
        let r = PathRewrite {
            home_dir: "/tmp/wr-golden-1/Global/".to_string(),
            session_dir: "/tmp/wr-golden-1/Session/".to_string(),
        };
        assert_eq!(
            r.apply("File location: /tmp/wr-golden-1/Session/Word Automata Library/GG.txt"),
            "File location: src/test/resources/integrationTests/Session/Word Automata Library/GG.txt"
        );
        assert_eq!(
            r.apply("File does not exist: /tmp/wr-golden-1/Global/Automata Library/x.txt"),
            "File does not exist: src/test/resources/integrationTests/Global/Automata Library/x.txt"
        );
        // Everything else is untouched — a wrong file name or a wrong subdirectory still
        // differs after the rewrite, which is what keeps this from hiding a port defect.
        assert_eq!(
            r.apply("File location: /tmp/wr-golden-1/Session/Automata Library/GG.txt"),
            "File location: src/test/resources/integrationTests/Session/Automata Library/GG.txt"
        );
        assert_eq!(r.apply("State count:7"), "State count:7");
        assert_eq!(r.apply("/some/other/path.txt"), "/some/other/path.txt");
    }

    #[test]
    fn identifiers_are_maximal_runs_in_first_appearance_order() {
        assert_eq!(
            identifiers("join test448 test444[a][b] test444[a][b];"),
            vec!["join", "test448", "test444", "a", "b"]
        );
    }
}
