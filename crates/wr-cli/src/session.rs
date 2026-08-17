// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `Main/Session.java`'s **path builders**, as an explicit context (Phase 3a, U14).
//!
//! Walnut resolves every library file it reads or writes through one class of static
//! `String`-concatenating helpers on `Main/Session`: the "read" family
//! (`getReadFileForWordsLibrary`, `getReadFileForAutomataLibrary`,
//! `getReadFileForMacroLibrary`, `getReadFileForMorphismLibrary`, `getTransducerFile`,
//! `getReadAddressForCustomBases`) and the "write" family (`getWriteAddressFor*`,
//! `getAddressForResult`). [`SessionPaths`] is that class, minus Java's statics.
//!
//! On top of it, [`FileLibraries`] is the **first real, file-backed implementation of
//! [`PredicateEnv`]** — until now that trait had only `wr-logic`'s in-memory double
//! (`wr_logic::predicate_env::InMemoryPredicateEnv`, U1). [`Session`] bundles the two.
//!
//! # Scope: path builders only
//!
//! Per the Phase 3 plan's U14 row, this unit ports `Session.java`'s path resolution and
//! nothing else. Deliberately **not** here:
//!
//! * **`Logging` wiring.** `setPathsAndNames`/`setPathsAndNamesIntegrationTests`/
//!   `cleanPathsAndNamesIntegrationTest` each end with
//!   `Logging.initializeGlobalLog(getAddressForResult() + GLOBAL_LOG_FILENAME)`. `Logging`
//!   itself is U0a (`wr_core::logging`); wiring the two together belongs to whichever unit
//!   builds the real CLI entry point (U21), which is also the only place that knows whether
//!   a global log should be opened at all. [`SessionPaths::address_for_result`] is the one
//!   piece of that line this unit owes it.
//! * **`createSubdirectories()`.** Directory *creation*, not path *resolution*. It is also
//!   the subject of `docs/WALNUT-BUGS.md` WB-006 (`mkdir()` rather than `mkdirs()`, i.e.
//!   non-recursive), which therefore stays `not yet reached`. **When a later unit does port
//!   it, the faithful spelling is [`std::fs::create_dir`], not `create_dir_all`** —
//!   `create_dir_all` is `mkdirs()` and would silently *fix* WB-006 rather than reproduce
//!   it, which `CLAUDE.md`'s mechanical-port rule forbids without explicit sign-off.
//! * **`setPathsAndNamesIntegrationTests` / `cleanPathsAndNamesIntegrationTest`, and
//!   `getAddressForTestResources`/`getAddressForUnitTestResources`/
//!   `getAddressForIntegrationTestResults`.** These name Maven's `src/test/resources/`
//!   layout and exist to keep *Java's own* test suite out of the user's home directory.
//!   The Rust port's fixtures do not live there and its tests do not need a `Session` to be
//!   redirected — they construct one over a temp tree directly (see this module's tests).
//!   Porting a hard-coded `"src/test/resources/"` into `wr-cli` would be dead code pointing
//!   at a directory this repo does not have.
//!
//! # WB-005 is structurally inapplicable here (not silently ignored)
//!
//! `docs/WALNUT-BUGS.md` WB-005 records two staleness defects in `Session.setPathsAndNames`,
//! both of which need the setup routine to run **twice in one process** against surviving
//! static state:
//!
//! 1. a second `setPathsAndNames(null, …)` recomputes the `name` timestamp but keeps the
//!    first call's `sessionWalnutDir` (`if (sessionWalnutDir == null)` guards only the
//!    derivation, not the name), so `getName()` drifts from the directory in use; and
//! 2. `globalSession` is sticky — set `true` once, never reset by a later `false`.
//!
//! Neither can occur here, and not by accident: [`SessionPaths`] has **no setter and no
//! `&mut self` method at all**. Every field is computed once in [`SessionPaths::new`] from
//! that call's own arguments and is immutable thereafter, so "a second call" means "a
//! second, independent value" — `name` and `session_walnut_dir` are always derived together
//! (defect 1 impossible), and `global_session` is a plain constructor argument with no prior
//! state to be sticky against (defect 2 impossible). WB-005's entry names exactly this
//! refactor as its intended resolution.
//!
//! What enforces that is **structural, not a test**: the absence of any `&mut self` method on
//! [`SessionPaths`]. [`tests::a_second_session_is_fully_independent_wb_005`] pins the
//! *observable consequence* — two independently constructed values never share or drift —
//! but it constructs both through [`SessionPaths::new`] and never mutates, so it would NOT by
//! itself fail if a future unit added a setter. Keeping [`SessionPaths`] mutator-free is a
//! maintainer's obligation; a unit that needs different paths must build a new value.
//!
//! # Java string concatenation is preserved verbatim
//!
//! Every builder here returns a `String` built by `+`, exactly as Java does — not a
//! [`std::path::PathBuf`]. Two reasons, both behavioral rather than stylistic:
//!
//! * The directory constants carry their own trailing `/` (`"Automata Library/"`), and
//!   `mainWalnutDir`/`sessionWalnutDir` are *expected* to carry one too. Java never
//!   normalizes; `Prover.parseArgs` appends the `/` to `--home-dir=`/`--session-dir=` values
//!   before handing them over (`Prover.java:304-313`), and a caller that forgets produces
//!   `"/tmp/walnutCustom Bases/x.txt"`. [`SessionPaths::new`] reproduces that exactly — it
//!   does **not** normalize — so U21's arg parser must keep doing the appending, as Java's
//!   does.
//! * The resolved address is user-visible: it is the payload of
//!   `WalnutException.fileDoesNotExist` (`"File does not exist: " + address`), which Tier 1's
//!   `error*` fixtures compare verbatim. A `PathBuf` round-trip would re-render separators.
//!
//! # Custom bases: ONE resolution path, session-aware, for queries and nested headers alike
//!
//! [`PredicateEnv::number_system`] resolves each `Custom Bases/` file through
//! [`SessionPaths::read_address_for_custom_bases`], i.e. through `globalOrSessionFile` — per
//! file, session copy preferred, exactly as Java does.
//!
//! A *word/function/automata* file can ALSO name a custom base, in its own header (`msd_fib`
//! as a track's numeration), and performing that lookup is `wr-io`'s job. **Java makes no
//! distinction between the two cases**: `ParseMethods.parseAlphabetDeclaration` (reached from
//! `AutomatonReader.firstParse` while reading any library `.txt`) calls
//! `NumberSystem.getComputeIfAbsent(base)`, whose `NumberSystem(String)` constructor reaches
//! `Session.getReadAddressForCustomBases` → `globalOrSessionFile` — the identical code path a
//! top-level `?msd_fib` query token takes. So a session-local `Custom Bases/` copy overrides
//! the global one for BOTH, and the class javadoc's promise ("if a previous session is
//! loaded, its files override the ones in the 'global' session", `Session.java:37`) applies
//! uniformly.
//!
//! This module therefore implements [`wr_io::reader::CustomBaseResolver`] on [`SessionPaths`]
//! (in terms of [`SessionPaths::read_address_for_custom_bases`] — the same
//! `globalOrSessionFile` call, not a second copy of its precedence logic) and hands it to
//! `wr-io`'s [`reader::read_automaton_txt_with_custom_base_resolver`]. `wr-io` stays
//! session-*unaware*: it does the file I/O, this crate supplies the policy, the same layering
//! U5 used for the custom-base *builder*.
//!
//! An earlier draft of this unit passed the bare **global** `Custom Bases/` directory to
//! `wr-io` instead, on the theory that a session's own `Custom Bases/` is always empty in the
//! KEEP subset (nothing in it *writes* there — `getWriteAddressForCustomBases`'s only caller
//! in all of `walnut-java` is the DROP-scope `Numeration/Ostrowski.java:228`). That reasoning
//! was wrong: hand-placing a file in a session directory is a documented, supported workflow,
//! not an accident, and Java honors it here. See
//! [`tests::a_session_custom_base_file_overrides_the_global_one_for_a_nested_header_too`].

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use wr_core::automaton::Automaton;
use wr_core::numsys::{
    self, CustomBaseCandidates, CustomBaseFiles, NumSysError, NumberSystem, TXT_EXTENSION,
};
use wr_io::reader::{self, ReadError};
use wr_logic::predicate_env::{PredicateEnv, PredicateEnvError};

// ---------------------------------------------------------------------------
// Session.java's constants (`:46-60`), verbatim -- trailing slashes included.
// ---------------------------------------------------------------------------

/// `Session.WALNUT_VERSION` (`:46`).
pub const WALNUT_VERSION: &str = "8.0-alpha";
/// `Session.PROMPT` (`:47`).
pub const PROMPT: &str = "\n[Walnut]$ ";

/// `Session.FRIENDLY_DATE_TIME_PATTERN` (`:48`), kept as a checkable record of the shape
/// [`friendly_date_time`] emits.
const FRIENDLY_DATE_TIME_PATTERN: &str = "yyyy_MM_dd_HH_mm_ss";

/// `Session.SESSION_NAME` (`:51`). (`GLOBAL_NAME`, `:50`, is used only by the
/// integration-test helpers this unit does not port.)
const SESSION_NAME: &str = "Session";

const AUTOMATA_LIB: &str = "Automata Library/";
/// Java builds this as `"Word " + AUTOMATA_LIB` (`:54`); spelled out here.
const WORD_AUTOMATA_LIB: &str = "Word Automata Library/";
const CUSTOM_BASES: &str = "Custom Bases/";
const MACRO_LIBRARY: &str = "Macro Library/";
const MORPHISM_LIBRARY: &str = "Morphism Library/";
const TRANSDUCER_LIBRARY: &str = "Transducer Library/";
const COMMAND_FILES: &str = "Command Files/";
const RESULT: &str = "Result/";
/// Inlined at `Session.java:149` rather than declared as a constant there.
const HELP_DOCUMENTATION_COMMANDS: &str = "Help Documentation/Commands/";

// ---------------------------------------------------------------------------
// SessionPaths
// ---------------------------------------------------------------------------

/// `Main/Session`'s four pieces of static path state (`:41-44`) plus its path builders.
///
/// Immutable after [`SessionPaths::new`] — see this module's docs for why that is what makes
/// `docs/WALNUT-BUGS.md` WB-005 unreachable rather than merely unlikely.
pub struct SessionPaths {
    /// `Session.name` (`:41`) — "user-friendly session name, may be a timestamp". `Option`
    /// because Java leaves it `null` in the global-session branch (`:71-73` assigns
    /// `sessionWalnutDir` but never `name`), and `getName()` really can return `null` there.
    name: Option<String>,
    /// `Session.mainWalnutDir` (`:42`). Initialized to `""` in Java, meaning "relative to the
    /// process's working directory".
    main_walnut_dir: String,
    /// `Session.sessionWalnutDir` (`:43`). Always assigned by `setPathsAndNames`, so it is
    /// not an `Option` here.
    session_walnut_dir: String,
    /// `Session.globalSession` (`:44`).
    global_session: bool,
    /// Sink for `globalOrSessionFile`'s `System.out.println` (`:200`, `:204`). Injectable for
    /// the same reason `wr_core::logging::Logging::with_writers` is: the message is real,
    /// user-visible output that tests must be able to assert on without capturing the
    /// process's stdout. Defaults to [`std::io::stdout`].
    console: RefCell<Box<dyn Write>>,
}

impl fmt::Debug for SessionPaths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionPaths")
            .field("name", &self.name)
            .field("main_walnut_dir", &self.main_walnut_dir)
            .field("session_walnut_dir", &self.session_walnut_dir)
            .field("global_session", &self.global_session)
            .finish_non_exhaustive()
    }
}

impl SessionPaths {
    /// `Session.setPathsAndNames(sessionDir, homeDir, globalSession)` (`:62-85`), minus its
    /// last two statements (`createSubdirectories()` and `Logging.initializeGlobalLog`, both
    /// out of this unit's scope — see the module docs).
    ///
    /// Java's branch structure, preserved:
    ///
    /// * `homeDir == null` → `mainWalnutDir` stays `""` **unless** the process's working
    ///   directory ends in `bin`, in which case it becomes `"../"` (`:63-66`: Walnut ships a
    ///   `bin/` launcher directory, and the home tree is its parent). Otherwise
    ///   `mainWalnutDir = homeDir`, unnormalized.
    /// * `globalSession` → `sessionWalnutDir = mainWalnutDir` and `name` stays `None`
    ///   (`:71-73`).
    /// * else `sessionDir == null` → `name = "Session/<timestamp>/"` and
    ///   `sessionWalnutDir = mainWalnutDir + name` (`:74-79`).
    /// * else → `sessionWalnutDir = name = sessionDir` (`:80-82`). Note Java does **not**
    ///   append a trailing `/` here; `Prover.parseArgs` did it (`Prover.java:304-308`).
    ///
    /// One environment-forced divergence, in the timestamp only: see [`friendly_date_time`].
    pub fn new(session_dir: Option<&str>, home_dir: Option<&str>, global_session: bool) -> Self {
        Self::with_console(
            session_dir,
            home_dir,
            global_session,
            Box::new(std::io::stdout()),
        )
    }

    /// As [`SessionPaths::new`], but with an injectable sink for `globalOrSessionFile`'s
    /// override notice — the seam this module's own tests use.
    pub fn with_console(
        session_dir: Option<&str>,
        home_dir: Option<&str>,
        global_session: bool,
        console: Box<dyn Write>,
    ) -> Self {
        // `:63-69`.
        let main_walnut_dir = match home_dir {
            None => {
                // `System.getProperty("user.dir")`. A failure to read it is not a case Java
                // has (the property is always set), so an unreadable cwd simply keeps the
                // `""` default rather than inventing an error.
                let ends_in_bin = std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(|s| s.ends_with("bin")))
                    .unwrap_or(false);
                if ends_in_bin {
                    "../".to_string()
                } else {
                    String::new()
                }
            }
            Some(dir) => dir.to_string(),
        };

        // `:71-82`.
        let (name, session_walnut_dir) = if global_session {
            (None, main_walnut_dir.clone())
        } else {
            match session_dir {
                None => {
                    let name = format!("{SESSION_NAME}/{}/", friendly_date_time());
                    let dir = format!("{main_walnut_dir}{name}");
                    (Some(name), dir)
                }
                Some(dir) => (Some(dir.to_string()), dir.to_string()),
            }
        };

        SessionPaths {
            name,
            main_walnut_dir,
            session_walnut_dir,
            global_session,
            console: RefCell::new(console),
        }
    }

    /// `Session.getName()` (`:140-142`) — `None` mirrors Java's `null` for a global session.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// `Session.globalSession` (`:44`), read-only.
    pub fn is_global_session(&self) -> bool {
        self.global_session
    }

    // ------------------------------------------ read, not session-specific (`:144-150`)

    /// `Session.getReadAddressForCommandFiles(filename)` (`:145-147`).
    pub fn read_address_for_command_files(&self, filename: &str) -> String {
        format!("{}{COMMAND_FILES}{filename}", self.main_walnut_dir)
    }

    /// `Session.getAddressForHelpCommands()` (`:148-150`).
    pub fn address_for_help_commands(&self) -> String {
        format!("{}{HELP_DOCUMENTATION_COMMANDS}", self.main_walnut_dir)
    }

    // ---------------------------------------------- read, session-specific (`:163-182`)

    /// `Session.getReadAddressForCustomBases(filename)` (`:164-166`).
    pub fn read_address_for_custom_bases(&self, filename: &str) -> String {
        self.global_or_session_file(&format!("{CUSTOM_BASES}{filename}"))
    }

    /// `Session.getReadFileForMacroLibrary(filename)` (`:167-169`).
    pub fn read_file_for_macro_library(&self, filename: &str) -> String {
        self.global_or_session_file(&format!("{MACRO_LIBRARY}{filename}"))
    }

    /// `Session.getReadFileForMorphismLibrary(fileName)` (`:170-172`).
    pub fn read_file_for_morphism_library(&self, filename: &str) -> String {
        self.global_or_session_file(&format!("{MORPHISM_LIBRARY}{filename}"))
    }

    /// `Session.getReadFileForAutomataLibrary(fileName)` (`:174-176`).
    pub fn read_file_for_automata_library(&self, filename: &str) -> String {
        self.global_or_session_file(&format!("{AUTOMATA_LIB}{filename}"))
    }

    /// `Session.getTransducerFile(fileName)` (`:177-179`). Read-only in practice: Walnut has
    /// no `getWriteAddressForTransducerLibrary`, and this method's only caller is the
    /// `transduce` command's `new Transducer(...)` (`Prover.java:696`).
    pub fn transducer_file(&self, filename: &str) -> String {
        self.global_or_session_file(&format!("{TRANSDUCER_LIBRARY}{filename}"))
    }

    /// `Session.getReadFileForWordsLibrary(fileName)` (`:180-182`).
    pub fn read_file_for_words_library(&self, filename: &str) -> String {
        self.global_or_session_file(&format!("{WORD_AUTOMATA_LIB}{filename}"))
    }

    /// `Session.globalOrSessionFile(testAddress)` (`:184-208`).
    ///
    /// "If a previous session is loaded, its files override the ones in the global session"
    /// (class javadoc). The precedence, verbatim:
    ///
    /// 1. a global session resolves to the global file, full stop — the session file is
    ///    never even stat'd;
    /// 2. otherwise, if the session copy is not an existing *file*, the global address is
    ///    returned **even when it does not exist either** (the resulting "File does not
    ///    exist" message then names the global path, which is the behavior Tier 1's `error*`
    ///    fixtures see);
    /// 3. otherwise the session copy wins, and — only if a global copy also exists and the
    ///    two differ byte-for-byte — `"Overriding global file with session file:<path>"` is
    ///    printed. Java compares with `Files.mismatch(...) != -1` and prints the same message
    ///    from the `IOException` arm too, "since this is all informational anyway"
    ///    (`:202-205`); both are preserved.
    fn global_or_session_file(&self, test_address: &str) -> String {
        let global_file = format!("{}{test_address}", self.main_walnut_dir);
        if self.global_session {
            return global_file;
        }

        let session_file = format!("{}{test_address}", self.session_walnut_dir);
        if !Path::new(&session_file).is_file() {
            return global_file;
        }
        if Path::new(&global_file).is_file() {
            // `Files.mismatch` returns -1 for identical content; anything else (including an
            // I/O failure, per Java's catch arm) prints.
            let differ = match (std::fs::read(&session_file), std::fs::read(&global_file)) {
                (Ok(a), Ok(b)) => a != b,
                _ => true,
            };
            if differ {
                let mut console = self.console.borrow_mut();
                // Java: `System.out.println` -- no space after the colon.
                let _ = writeln!(
                    console,
                    "Overriding global file with session file:{session_file}"
                );
                let _ = console.flush();
            }
        }
        session_file
    }

    // --------------------------------------------- write, session-specific (`:210-228`)

    /// `Session.getWriteAddressForCustomBases()` (`:211-213`).
    pub fn write_address_for_custom_bases(&self) -> String {
        format!("{}{CUSTOM_BASES}", self.session_walnut_dir)
    }

    /// `Session.getWriteAddressForMacroLibrary()` (`:214-216`).
    pub fn write_address_for_macro_library(&self) -> String {
        format!("{}{MACRO_LIBRARY}", self.session_walnut_dir)
    }

    /// `Session.getWriteAddressForMorphismLibrary()` (`:217-219`).
    pub fn write_address_for_morphism_library(&self) -> String {
        format!("{}{MORPHISM_LIBRARY}", self.session_walnut_dir)
    }

    /// `Session.getWriteAddressForAutomataLibrary()` (`:220-222`).
    pub fn write_address_for_automata_library(&self) -> String {
        format!("{}{AUTOMATA_LIB}", self.session_walnut_dir)
    }

    /// `Session.getAddressForResult()` (`:223-225`).
    pub fn address_for_result(&self) -> String {
        format!("{}{RESULT}", self.session_walnut_dir)
    }

    /// `Session.getWriteAddressForWordsLibrary()` (`:226-228`).
    pub fn write_address_for_words_library(&self) -> String {
        format!("{}{WORD_AUTOMATA_LIB}", self.session_walnut_dir)
    }

    /// `Session.createSubdirectories()` (`:122-135`), the `setPathsAndNames` step U14
    /// left out of scope (see this module's docs) and U21's `crate::prover::parse_args`
    /// needs, since a real CLI run has to create the tree it writes into.
    ///
    /// Java's `File.mkdir()` is **not** recursive and its failure is a
    /// `WalnutException("Couldn't create directory:" + s)` — both reproduced: this uses
    /// `fs::create_dir` (single level), and the directory list, its order, and the
    /// `s.isEmpty()` skip are verbatim. The `Err` is the message string, which
    /// `crate::prover` wraps.
    pub fn create_subdirectories(&self) -> Result<(), String> {
        for s in [
            format!("{}{SESSION_NAME}", self.main_walnut_dir),
            self.session_walnut_dir.clone(),
            self.address_for_result(),
            self.write_address_for_automata_library(),
            self.write_address_for_custom_bases(),
            self.write_address_for_macro_library(),
            self.write_address_for_morphism_library(),
            self.write_address_for_words_library(),
        ] {
            if s.is_empty() {
                continue;
            }
            let path = Path::new(&s);
            // `if (!f.isDirectory() && !f.mkdir())` -- an already-existing directory is
            // fine, anything else that fails to be created is fatal.
            if !path.is_dir() && std::fs::create_dir(path).is_err() {
                return Err(crate::walnut_exception::could_not_create_directory(&s));
            }
        }
        Ok(())
    }
}

/// Lets `wr-io` resolve a custom-base file named in a library `.txt`'s **header** through this
/// session, with the same `globalOrSessionFile` precedence every other library read gets.
///
/// This is not an extra Java method — it is the injection seam that reproduces Java's
/// *absence* of a separate path: `NumberSystem`'s constructor calls the static
/// `Session.getReadAddressForCustomBases` directly, so in Java a nested header lookup IS a
/// session lookup. Delegating to [`SessionPaths::read_address_for_custom_bases`] (rather than
/// re-deriving a directory) keeps the byte-comparison/override-notice behavior in exactly one
/// place. See this module's custom-bases section.
impl reader::CustomBaseResolver for SessionPaths {
    fn resolve(&self, filename: &str) -> PathBuf {
        // `PathBuf::from` on the concatenated address, NOT a `Path::join` of components:
        // Java's addresses are raw string concatenation (an un-slashed `--home-dir` really
        // does produce `/tmp/walnutCustom Bases/x.txt`), and `from` preserves that verbatim.
        PathBuf::from(self.read_address_for_custom_bases(filename))
    }
}

/// `LocalDateTime.now().format(ofPattern("yyyy_MM_dd_HH_mm_ss"))` (`Session.java:75-76`),
/// **in UTC**.
///
/// The one declared divergence in this unit, and it is environment-forced rather than a
/// choice: Java's `LocalDateTime.now()` is in the JVM's default time zone, and reading the
/// system time zone from Rust needs a dependency (`chrono`/`time`) that this deliberately
/// lean workspace does not carry and that `docs/DESIGN.md`'s dependency table has no entry
/// for. The entire observable consequence is the *spelling of one directory name* for a
/// session started with no `--session-dir`: `Session/2026_08_12_22_57_59/` versus the same
/// instant rendered locally. Nothing reads the timestamp back, nothing parses it, and no
/// golden fixture contains one (integration runs pass an explicit `--session-dir`). Adding a
/// time-zone dependency to change which of two equally-arbitrary directory names gets created
/// is not worth it; if it ever is, this function is the only thing to change.
///
/// The civil-date conversion is Howard Hinnant's `civil_from_days`, the standard branch-free
/// algorithm — exact for every day in the proleptic Gregorian calendar.
fn friendly_date_time() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_civil_utc(secs)
}

/// [`friendly_date_time`]'s pure core: seconds since the Unix epoch →
/// [`FRIENDLY_DATE_TIME_PATTERN`]. Split out so it is testable against known instants.
fn format_civil_utc(secs_since_epoch: u64) -> String {
    debug_assert_eq!(FRIENDLY_DATE_TIME_PATTERN, "yyyy_MM_dd_HH_mm_ss");
    let days = (secs_since_epoch / 86_400) as i64;
    let time_of_day = secs_since_epoch % 86_400;
    let (hour, minute, second) = (
        time_of_day / 3_600,
        (time_of_day % 3_600) / 60,
        time_of_day % 60,
    );

    // civil_from_days: shift the epoch to 0000-03-01 so leap days land at the end of a year.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    format!("{year:04}_{month:02}_{day:02}_{hour:02}_{minute:02}_{second:02}")
}

// ---------------------------------------------------------------------------
// FileLibraries -- the real PredicateEnv
// ---------------------------------------------------------------------------

/// The four file-library lookups `Main/Predicate.java`'s tokenizer makes, backed by real
/// files under a [`SessionPaths`].
///
/// This is a **narrow sub-struct of [`Session`], not `Session` itself**, per `PORTING.md`'s
/// Phase 3 ruling: [`PredicateEnv`]'s methods take `&self` while
/// `wr_core::determinize::DeterminizeContext`'s take `&mut self`, so a `Session` that
/// implemented both directly would make "hold the env borrowed while a nested evaluation
/// needs the determinize context mutably" an aliasing error. Keeping the two in disjoint
/// fields of [`Session`] makes that combination legal by construction.
pub struct FileLibraries {
    paths: Rc<SessionPaths>,
    /// `NumberSystem.numberSystemHash` (`NumberSystem.java:211-213`) — Java's
    /// `getComputeIfAbsent` cache, which for the real (file-backed) environment is what keeps
    /// a cache miss from re-reading `Custom Bases/` on every token. `RefCell` because
    /// [`PredicateEnv`]'s methods take `&self`; the cache is left **unmodified** on failure,
    /// matching `computeIfAbsent`'s contract (a failed resolution is not negatively cached,
    /// so a later identical lookup retries — which matters here for real, since a user can
    /// create the missing file between two commands).
    number_systems: RefCell<BTreeMap<String, Rc<NumberSystem>>>,
}

impl fmt::Debug for FileLibraries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileLibraries")
            .field("paths", &self.paths)
            .field(
                "cached_number_systems",
                &self.number_systems.borrow().keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl FileLibraries {
    /// Wraps an existing [`SessionPaths`]. Normally reached through [`Session::new`].
    pub fn new(paths: Rc<SessionPaths>) -> Self {
        FileLibraries {
            paths,
            number_systems: RefCell::new(BTreeMap::new()),
        }
    }

    /// The paths these libraries resolve against.
    pub fn paths(&self) -> &SessionPaths {
        &self.paths
    }

    /// `new Automaton(address)` (`Automata/Automaton.java`'s address constructor) →
    /// `AutomatonReader.readAutomaton`, whose single `catch (IOException)` rethrows as
    /// `WalnutException.fileDoesNotExist(address)` (`AutomatonReader.java:99-102`).
    ///
    /// So an unreadable file — missing, unreadable, a directory — is
    /// [`PredicateEnvError::FileDoesNotExist`] naming the resolved address, and every other
    /// reader failure (a genuine parse error) is [`PredicateEnvError::MalformedAutomaton`].
    /// That split is exactly Java's: it is the `IOException` boundary, not a message-text
    /// classification.
    ///
    /// `pub(crate)` (rather than private) as of U16: `crate::alphabet::alphabet_command`
    /// needs exactly this "read a `.txt` at an already-resolved address, mapping
    /// `IOException`/parse failures the same way `new Automaton(address)` does" behavior
    /// for `Automaton M = new Automaton(ProverHelper.determineInLibrary(...))`
    /// (`Alphabet.java:20`) — widening this existing method's visibility avoids a second,
    /// independent copy of the same IO/error-mapping logic drifting out of sync with this
    /// one.
    pub(crate) fn read_library_automaton(
        &self,
        address: &str,
    ) -> Result<Automaton, PredicateEnvError> {
        self.read_library_automaton_with_ctx(address, None)
    }

    /// [`Self::read_library_automaton`] with the enclosing command's
    /// [`wr_core::determinize::DeterminizeContext`], so a **nondeterministic** library file's
    /// load-time `determinizeAndMinimize()` (`AutomatonReader.java:88-98`) consumes a
    /// `[strategy n …]`/`[export n …]` index the way Java's global counter makes it.
    ///
    /// One new-with-`ctx` error shape, worth naming because the mapping below flattens it:
    /// a context that answers this load's index with a non-`SC` strategy on a DFAO file
    /// produces [`wr_io::reader::ReadError::Determinize`], which lands in
    /// [`PredicateEnvError::MalformedAutomaton`] carrying Java's own
    /// `"DFAOs are not supported for non-SC strategies."` text rather than surfacing as a
    /// bare `WalnutException` the way Java's throw would. Both refuse the command; only the
    /// wrapping differs. It is unreachable with `ctx = None` (the `SC` arm cannot fail), and
    /// with `Some` it needs a genuinely nondeterministic file that is ALSO a DFAO — which
    /// real Walnut rejects even earlier, with `nonDeterministicO` (`docs/WALNUT-BUGS.md`
    /// WB-022, this port's pre-existing gap, unchanged here).
    pub(crate) fn read_library_automaton_with_ctx(
        &self,
        address: &str,
        ctx: Option<&mut (dyn wr_core::determinize::DeterminizeContext + '_)>,
    ) -> Result<Automaton, PredicateEnvError> {
        // A custom base named in this file's own header resolves through the SESSION (session
        // copy shadowing the global one, per file), which is what Java's
        // `ParseMethods.parseAlphabetDeclaration` → `NumberSystem.getComputeIfAbsent` →
        // `Session.getReadAddressForCustomBases` chain does — see the module docs.
        reader::read_automaton_txt_with_custom_base_resolver_and_ctx(
            Path::new(address),
            &*self.paths as &dyn reader::CustomBaseResolver,
            ctx,
        )
        .map_err(|e| match e {
            ReadError::Io(_) => PredicateEnvError::FileDoesNotExist {
                address: address.to_string(),
            },
            other => PredicateEnvError::MalformedAutomaton {
                address: address.to_string(),
                detail: other.to_string(),
            },
        })
    }

    /// `new NumberSystem(name)` (`NumberSystem.java:132-163`) with this session supplying the
    /// three `loadAutomatonOrNull` probes (`:299-319`).
    ///
    /// Java's line order is preserved, and it matters: `isNeg` is checked at `:137`, before
    /// `setAdditionAutomaton` reaches a file at `:142`, so a `_neg_` name is rejected without
    /// ever probing the (genuinely present) `Custom Bases/msd_neg_fib*.txt` files. `wr-io`'s
    /// [`reader`] does the same for header tokens; this is the query-token twin of it.
    ///
    /// Note that Java probes `Custom Bases/` for **every** name, `msd_2` included — the
    /// programmatic base-*k* adder is only the *fallback* inside `setAdditionAutomaton`
    /// (`:322-330`), which is why this does not special-case standard bases. A user who drops
    /// a `Custom Bases/msd_2_addition.txt` into their home directory really does override
    /// base-2 addition in real Walnut, and does here too.
    fn load_number_system(&self, name: &str) -> Result<NumberSystem, PredicateEnvError> {
        let numsys_error = |source: NumSysError| PredicateEnvError::NumberSystem {
            name: name.to_string(),
            source,
        };
        if name.contains(numsys::UNDERSCORE_NEG_UNDERSCORE) {
            return Err(numsys_error(NumSysError::UnsupportedNegativeBase(
                name.to_string(),
            )));
        }
        let files = CustomBaseFiles {
            addition: self.probe_custom_base(name, numsys::UNDERSCORE_ADDITION_AUTOMATON)?,
            less_than: self.probe_custom_base(name, numsys::UNDERSCORE_LESS_THAN_AUTOMATON)?,
            all_representations: self.probe_custom_base(name, TXT_EXTENSION)?,
        };
        NumberSystem::with_custom_base_files(name, files).map_err(numsys_error)
    }

    /// One `NumberSystem.loadAutomatonOrNull` probe (`:299-319`), minus the decision logic
    /// (that is [`CustomBaseCandidates::resolve`], applied inside
    /// [`NumberSystem::with_custom_base_files`]).
    ///
    /// Java's `else if` is preserved rather than collapsed into two independent probes: the
    /// complement file is stat'd **only** when the main file is absent.
    fn probe_custom_base(
        &self,
        name: &str,
        extension: &str,
    ) -> Result<CustomBaseCandidates, PredicateEnvError> {
        let (main_name, complement_name) = numsys::custom_base_candidate_names(name, extension)
            .map_err(|source| PredicateEnvError::NumberSystem {
                name: name.to_string(),
                source,
            })?;

        let main_address = self.paths.read_address_for_custom_bases(&main_name);
        let main = if Path::new(&main_address).is_file() {
            Some(self.read_library_automaton(&main_address)?)
        } else {
            None
        };
        let complement = if main.is_none() {
            let address = self.paths.read_address_for_custom_bases(&complement_name);
            if Path::new(&address).is_file() {
                Some(self.read_library_automaton(&address)?)
            } else {
                None
            }
        } else {
            None
        };
        Ok(CustomBaseCandidates { main, complement })
    }
}

impl PredicateEnv for FileLibraries {
    fn number_system(&self, name: &str) -> Result<Rc<NumberSystem>, PredicateEnvError> {
        if let Some(ns) = self.number_systems.borrow().get(name) {
            return Ok(Rc::clone(ns));
        }
        // The borrow above is dropped before the (file-reading, potentially failing) build
        // below; `computeIfAbsent`'s contract is that a throwing mapping function leaves the
        // map untouched, so the insert happens only on success.
        let ns = Rc::new(self.load_number_system(name)?);
        self.number_systems
            .borrow_mut()
            .insert(name.to_string(), Rc::clone(&ns));
        Ok(ns)
    }

    /// `new NumberSystem(name)` (`Token/Function.java:52`) — genuinely unmemoized, exactly as
    /// Java's direct constructor call is, but resolving `Custom Bases/` through this session
    /// the same way that constructor does through the static `Session`. Overriding the trait's
    /// clone-out-of-the-cache default keeps the "`$name(…)` does not share the memoized
    /// handle" behavior visible here rather than only in the type.
    fn fresh_number_system(&self, name: &str) -> Result<NumberSystem, PredicateEnvError> {
        self.load_number_system(name)
    }

    // `word`/`function` are the trait's derived forms (`self.word_with_ctx(name, None)`)
    // and are deliberately NOT overridden here — the two `_with_ctx` methods below are the
    // required ones, so this file-backed environment cannot accidentally end up with a
    // context-dropping lookup. See [`wr_logic::predicate_env::PredicateEnv::word_with_ctx`].

    /// This environment is file-backed: `AutomatonReader` determinizes a nondeterministic
    /// word file on load, and Java counts that determinization — hence the `ctx`.
    fn word_with_ctx(
        &self,
        name: &str,
        ctx: Option<&mut (dyn wr_core::determinize::DeterminizeContext + '_)>,
    ) -> Result<Automaton, PredicateEnvError> {
        // `Predicate.java:295`:
        // `new Automaton(Session.getReadFileForWordsLibrary(name + ".txt"))`.
        let address = self
            .paths
            .read_file_for_words_library(&format!("{name}{TXT_EXTENSION}"));
        self.read_library_automaton_with_ctx(&address, ctx)
    }

    /// The same, for `$name(…)`; this is the commonest one in practice, since
    /// `Automata Library/` is where hand-written NFAs live. It is *not* the only one:
    /// `AutomatonReader.java:88-96` refuses to determinize a nondeterministic file only
    /// when `FA.isFAO()` (some state's output is `> 1`), so a word file whose outputs are
    /// all `0`/`1` — a characteristic sequence, say — is determinized on load and counted
    /// exactly like an `Automata Library/` NFA.
    fn function_with_ctx(
        &self,
        name: &str,
        ctx: Option<&mut (dyn wr_core::determinize::DeterminizeContext + '_)>,
    ) -> Result<Automaton, PredicateEnvError> {
        // `Automaton.readAutomatonFromFile` (`Automaton.java:148-150`).
        let address = self
            .paths
            .read_file_for_automata_library(&format!("{name}{TXT_EXTENSION}"));
        self.read_library_automaton_with_ctx(&address, ctx)
    }

    fn macro_text(&self, name: &str) -> Result<String, PredicateEnvError> {
        // `Predicate.readMacroFile` (`Predicate.java:495-511`): read line by line with
        // `BufferedReader.readLine()` and `macro.append(line)`, i.e. **newlines are dropped
        // entirely** -- a two-line macro file lexes as if its halves were written adjacent.
        // Any `IOException` becomes "Macro does not exist: " + the BARE name (no directory,
        // no `.txt`), unlike the automaton reader's full-address message.
        let address = self
            .paths
            .read_file_for_macro_library(&format!("{name}{TXT_EXTENSION}"));
        match std::fs::read_to_string(&address) {
            Ok(text) => Ok(text.lines().collect()),
            Err(_) => Err(PredicateEnvError::MacroDoesNotExist {
                name: name.to_string(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// One Walnut session: [`SessionPaths`] plus the [`PredicateEnv`] implementation over them.
///
/// The two are deliberately separate fields rather than one struct implementing everything —
/// see [`FileLibraries`] for the aliasing argument. [`SessionPaths`] is shared by [`Rc`]
/// rather than cloned so the two views can never drift apart (it is immutable, so sharing is
/// free of the usual caveats).
#[derive(Debug)]
pub struct Session {
    paths: Rc<SessionPaths>,
    libraries: FileLibraries,
}

impl Session {
    /// See [`SessionPaths::new`] for the argument semantics — this only adds the library
    /// layer on top.
    pub fn new(session_dir: Option<&str>, home_dir: Option<&str>, global_session: bool) -> Self {
        Session::from_paths(SessionPaths::new(session_dir, home_dir, global_session))
    }

    /// Builds a session over already-constructed paths (e.g. one with an injected console).
    pub fn from_paths(paths: SessionPaths) -> Self {
        let paths = Rc::new(paths);
        Session {
            libraries: FileLibraries::new(Rc::clone(&paths)),
            paths,
        }
    }

    /// The path builders.
    pub fn paths(&self) -> &SessionPaths {
        &self.paths
    }

    /// The path builders as a shared handle. Needed by `crate::meta_commands::MetaCommands`,
    /// which must be able to write `[export …]` files from inside
    /// `DeterminizeContext::export_pre_determinization` — a `&mut self` method that holds no
    /// borrow of the `Session`. [`SessionPaths`] is immutable after construction, so sharing
    /// it is free of the usual aliasing caveats (same argument as the field's own docs).
    pub fn paths_rc(&self) -> Rc<SessionPaths> {
        Rc::clone(&self.paths)
    }

    /// The file libraries, as their concrete type.
    pub fn libraries(&self) -> &FileLibraries {
        &self.libraries
    }

    /// The file libraries as the trait `wr-logic`'s lexer takes.
    pub fn predicate_env(&self) -> &dyn PredicateEnv {
        &self.libraries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use wr_core::equiv::automaton_language_equivalent;

    // ---------------------------------------------------------- temp-tree scaffolding

    /// Same convention as `wr_io::reader`'s and `wr_cli::test_case`'s tests: a
    /// process-scoped directory under [`std::env::temp_dir`], removed and recreated at the
    /// start of the test. No `tempfile` dependency is added — the workspace deliberately
    /// carries only `proptest`, `num-bigint` and `regex-automata`, each with a written
    /// justification, and a test-only temp-dir crate would be the first entry with none.
    fn temp_root(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("wr-cli-session-{tag}-{}-{n}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Lays out a realistic Walnut home/session tree:
    ///
    /// ```text
    /// <root>/Global/{Automata,Word Automata,Macro,Morphism,Transducer} Library/, Custom Bases/, ...
    /// <root>/Session/  ... the same subdirectories
    /// ```
    ///
    /// Returns `(root, global_dir, session_dir)`, the last two as the
    /// trailing-slash-terminated strings `Session`'s builders expect (Java's
    /// `Prover.parseArgs` appends that slash; `SessionPaths` faithfully does not).
    fn walnut_tree(tag: &str) -> (PathBuf, String, String) {
        let root = temp_root(tag);
        let mut dirs = Vec::new();
        for home in ["Global", "Session"] {
            for sub in [
                AUTOMATA_LIB,
                WORD_AUTOMATA_LIB,
                CUSTOM_BASES,
                MACRO_LIBRARY,
                MORPHISM_LIBRARY,
                TRANSDUCER_LIBRARY,
                COMMAND_FILES,
                RESULT,
            ] {
                std::fs::create_dir_all(root.join(home).join(sub.trim_end_matches('/'))).unwrap();
            }
            dirs.push(format!("{}/", root.join(home).display()));
        }
        (root.clone(), dirs[0].clone(), dirs[1].clone())
    }

    fn write(dir: &str, relative: &str, contents: &str) {
        let path = PathBuf::from(format!("{dir}{relative}"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// A 1-track msd base-2 automaton accepting exactly the words with no `1` in them, in
    /// Walnut's `.txt` grammar. Small, real, and its language is easy to assert on.
    const ZERO_ONLY: &str = "msd_2\n\n0 1\n0 -> 0\n1 -> 1\n\n1 0\n0 -> 1\n1 -> 1\n";
    /// Same shape, different language (accepts everything), so "which file won" is
    /// observable rather than a guess.
    const ANYTHING: &str = "msd_2\n\n0 1\n0 -> 0\n1 -> 0\n";

    #[derive(Clone)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn new() -> Self {
            SharedBuf(Arc::new(Mutex::new(Vec::new())))
        }
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Language equivalence for the two addition automata below.
    ///
    /// [`automaton_language_equivalent`] requires *total* DFAs, and Walnut's adders are
    /// partial (a digit triple that cannot occur in a valid addition simply has no
    /// transition), so both sides are totalized into a fresh sink first — the same
    /// `Fa::totalize(0)` step `determinize.rs`'s own Brzozowski-vs-subset-construction
    /// cross-check uses. The track-structure guard is kept explicitly, since totalizing at
    /// the `Fa` level bypasses the `Automaton`-level check.
    fn same_language(a: &Automaton, b: &Automaton) -> bool {
        assert_eq!(a.alphabet, b.alphabet, "track structure must match");
        let (mut x, mut y) = (a.fa.clone(), b.fa.clone());
        x.totalize(0);
        y.totalize(0);
        wr_core::equiv::language_equivalent(&x, &y).unwrap()
    }

    fn session_with_capture(
        global: &str,
        session: &str,
        global_session: bool,
    ) -> (Session, SharedBuf) {
        let buf = SharedBuf::new();
        let paths = SessionPaths::with_console(
            Some(session),
            Some(global),
            global_session,
            Box::new(buf.clone()),
        );
        (Session::from_paths(paths), buf)
    }

    // ----------------------------------------------------------- pure path building

    /// Every builder is Java's raw `+` concatenation, trailing slashes and all. Pinned
    /// against a global session so `globalOrSessionFile` short-circuits and the expected
    /// strings stay independent of what exists on disk.
    #[test]
    fn path_builders_are_javas_string_concatenation() {
        let p = SessionPaths::new(None, Some("/walnut/"), true);
        assert_eq!(
            p.read_address_for_command_files("go.txt"),
            "/walnut/Command Files/go.txt"
        );
        assert_eq!(
            p.address_for_help_commands(),
            "/walnut/Help Documentation/Commands/"
        );
        assert_eq!(
            p.read_address_for_custom_bases("msd_fib_addition.txt"),
            "/walnut/Custom Bases/msd_fib_addition.txt"
        );
        assert_eq!(
            p.read_file_for_macro_library("m.txt"),
            "/walnut/Macro Library/m.txt"
        );
        assert_eq!(
            p.read_file_for_morphism_library("mu.txt"),
            "/walnut/Morphism Library/mu.txt"
        );
        assert_eq!(
            p.read_file_for_automata_library("phi.txt"),
            "/walnut/Automata Library/phi.txt"
        );
        assert_eq!(
            p.transducer_file("RUNSUM2.txt"),
            "/walnut/Transducer Library/RUNSUM2.txt"
        );
        assert_eq!(
            p.read_file_for_words_library("T.txt"),
            "/walnut/Word Automata Library/T.txt"
        );

        // Write addresses hang off `sessionWalnutDir`, which a global session aliases to
        // `mainWalnutDir` (`Session.java:71-73`).
        assert_eq!(p.write_address_for_custom_bases(), "/walnut/Custom Bases/");
        assert_eq!(
            p.write_address_for_macro_library(),
            "/walnut/Macro Library/"
        );
        assert_eq!(
            p.write_address_for_morphism_library(),
            "/walnut/Morphism Library/"
        );
        assert_eq!(
            p.write_address_for_automata_library(),
            "/walnut/Automata Library/"
        );
        assert_eq!(p.address_for_result(), "/walnut/Result/");
        assert_eq!(
            p.write_address_for_words_library(),
            "/walnut/Word Automata Library/"
        );

        // The `wr-io` resolver seam is the same concatenation, not a `Path::join` (which
        // would insert a separator into the un-slashed-home-dir case pinned just below).
        assert_eq!(
            reader::CustomBaseResolver::resolve(&p, "msd_fib_addition.txt"),
            PathBuf::from("/walnut/Custom Bases/msd_fib_addition.txt")
        );
    }

    /// `Session.setPathsAndNames` does **not** normalize a missing trailing separator —
    /// `Prover.parseArgs` does (`Prover.java:304-313`). Pinned so U21's port cannot quietly
    /// drop that step on the assumption that `Session` covers it.
    #[test]
    fn a_home_dir_without_a_trailing_slash_is_not_normalized() {
        let p = SessionPaths::new(None, Some("/walnut"), true);
        assert_eq!(
            p.read_file_for_words_library("T.txt"),
            "/walnutWord Automata Library/T.txt"
        );
    }

    /// The three name/directory branches of `setPathsAndNames` (`:71-82`).
    #[test]
    fn session_naming_matches_javas_three_branches() {
        // Global: `name` stays null, and the session directory aliases the home directory.
        let global = SessionPaths::new(None, Some("/w/"), true);
        assert_eq!(global.name(), None);
        assert!(global.is_global_session());
        assert_eq!(global.address_for_result(), "/w/Result/");

        // Explicit session dir: `sessionWalnutDir = name = sessionDir`, NOT prefixed by the
        // home directory.
        let explicit = SessionPaths::new(Some("/tmp/s/"), Some("/w/"), false);
        assert_eq!(explicit.name(), Some("/tmp/s/"));
        assert_eq!(explicit.address_for_result(), "/tmp/s/Result/");
        assert_eq!(
            explicit.read_address_for_command_files("go.txt"),
            "/w/Command Files/go.txt",
            "command files are read from the HOME directory, never the session"
        );

        // Timestamped: `name = "Session/<stamp>/"`, prefixed by the home directory.
        let stamped = SessionPaths::new(None, Some("/w/"), false);
        let name = stamped.name().unwrap().to_string();
        assert!(name.starts_with("Session/"), "{name}");
        assert!(name.ends_with('/'), "{name}");
        assert_eq!(stamped.address_for_result(), format!("/w/{name}Result/"));
    }

    /// `Session/yyyy_MM_dd_HH_mm_ss/`, checked against instants computed independently.
    #[test]
    fn timestamps_render_javas_pattern() {
        assert_eq!(format_civil_utc(0), "1970_01_01_00_00_00");
        // 2000-02-29T23:59:59Z -- a leap day in a century leap year, the case the 100/400
        // corrections exist for.
        assert_eq!(format_civil_utc(951_868_799), "2000_02_29_23_59_59");
        // 2026-08-12T22:57:59Z.
        assert_eq!(format_civil_utc(1_786_575_479), "2026_08_12_22_57_59");
        // 2100-03-01T00:00:00Z -- 2100 is NOT a leap year.
        assert_eq!(format_civil_utc(4_107_542_400), "2100_03_01_00_00_00");

        let now = friendly_date_time();
        assert_eq!(now.len(), FRIENDLY_DATE_TIME_PATTERN.len(), "{now}");
        assert!(now.starts_with("20"), "{now}");
    }

    // ------------------------------------------------------ globalOrSessionFile behavior

    #[test]
    fn the_session_copy_overrides_the_global_one() {
        let (_root, global, session) = walnut_tree("override");
        write(&global, "Word Automata Library/T.txt", ANYTHING);
        write(&session, "Word Automata Library/T.txt", ZERO_ONLY);

        let (s, out) = session_with_capture(&global, &session, false);
        assert_eq!(
            s.paths().read_file_for_words_library("T.txt"),
            format!("{session}Word Automata Library/T.txt")
        );
        assert_eq!(
            out.text(),
            format!(
                "Overriding global file with session file:{session}Word Automata Library/T.txt\n"
            )
        );
    }

    /// Two *identical* files are not an override worth announcing (`Files.mismatch == -1`).
    #[test]
    fn identical_files_do_not_print_the_override_notice() {
        let (_root, global, session) = walnut_tree("identical");
        write(&global, "Word Automata Library/T.txt", ANYTHING);
        write(&session, "Word Automata Library/T.txt", ANYTHING);

        let (s, out) = session_with_capture(&global, &session, false);
        assert_eq!(
            s.paths().read_file_for_words_library("T.txt"),
            format!("{session}Word Automata Library/T.txt")
        );
        assert_eq!(out.text(), "");
    }

    /// No global counterpart means no notice either — Java only prints when it is genuinely
    /// shadowing something.
    #[test]
    fn a_session_only_file_prints_nothing() {
        let (_root, global, session) = walnut_tree("session-only");
        write(&session, "Word Automata Library/T.txt", ZERO_ONLY);

        let (s, out) = session_with_capture(&global, &session, false);
        assert_eq!(
            s.paths().read_file_for_words_library("T.txt"),
            format!("{session}Word Automata Library/T.txt")
        );
        assert_eq!(out.text(), "");
    }

    /// A missing session copy falls back to the global address **even when the global file
    /// does not exist either** — the returned address is what the "File does not exist"
    /// message will name.
    #[test]
    fn a_missing_session_copy_falls_back_to_the_global_address() {
        let (_root, global, session) = walnut_tree("fallback");
        write(&global, "Word Automata Library/T.txt", ANYTHING);

        let (s, _out) = session_with_capture(&global, &session, false);
        assert_eq!(
            s.paths().read_file_for_words_library("T.txt"),
            format!("{global}Word Automata Library/T.txt")
        );
        assert_eq!(
            s.paths().read_file_for_words_library("absent.txt"),
            format!("{global}Word Automata Library/absent.txt"),
            "a nonexistent file still resolves to the GLOBAL address"
        );
    }

    /// A global session never even looks at the session tree (`:186-188`), so a session file
    /// that exists is still ignored.
    #[test]
    fn a_global_session_ignores_the_session_tree_entirely() {
        let (_root, global, session) = walnut_tree("global-session");
        write(&global, "Word Automata Library/T.txt", ANYTHING);
        write(&session, "Word Automata Library/T.txt", ZERO_ONLY);

        let (s, out) = session_with_capture(&global, &session, true);
        assert_eq!(
            s.paths().read_file_for_words_library("T.txt"),
            format!("{global}Word Automata Library/T.txt")
        );
        assert_eq!(out.text(), "");
        let word = s.libraries().word("T").unwrap();
        assert!(word.fa.accepts_word(&[word.encode(&[1])]));
    }

    // ---------------------------------------------------------- PredicateEnv: words

    #[test]
    fn word_and_function_read_their_own_libraries() {
        let (_root, global, session) = walnut_tree("libs");
        // The SAME bare name in both libraries, with different languages: collapsing the two
        // lookups would be silently wrong rather than loud.
        write(&global, "Word Automata Library/T.txt", ZERO_ONLY);
        write(&global, "Automata Library/T.txt", ANYTHING);

        let (s, _out) = session_with_capture(&global, &session, false);
        let word = s.libraries().word("T").unwrap();
        let function = s.libraries().function("T").unwrap();
        assert!(
            !word.fa.accepts_word(&[word.encode(&[1])]),
            "T.txt in Word Automata Library accepts only 0"
        );
        assert!(function.fa.accepts_word(&[function.encode(&[1])]));
        assert!(!automaton_language_equivalent(&word, &function).unwrap());
    }

    /// Java constructs a fresh `Automaton` from the file per occurrence, and `Word.act` then
    /// `bind`s it — so two lookups must be two independent values.
    #[test]
    fn each_lookup_returns_an_independent_automaton() {
        let (_root, global, session) = walnut_tree("independent");
        write(&global, "Word Automata Library/T.txt", ZERO_ONLY);

        let (s, _out) = session_with_capture(&global, &session, false);
        let mut first = s.libraries().word("T").unwrap();
        // `wr_io`'s reader hands back the placeholder track labels the `.txt` grammar
        // implies ("0", "1", ...), since the file format carries no variable names.
        assert_eq!(first.label, vec!["0".to_string()]);
        first.label = vec!["i".to_string()];

        let second = s.libraries().word("T").unwrap();
        assert_eq!(
            second.label,
            vec!["0".to_string()],
            "binding one occurrence must not be visible in the next lookup"
        );
    }

    #[test]
    fn a_missing_word_or_function_reports_the_resolved_address() {
        let (_root, global, session) = walnut_tree("missing");
        let (s, _out) = session_with_capture(&global, &session, false);

        assert_eq!(
            s.libraries().word("T").unwrap_err().to_string(),
            format!("File does not exist: {global}Word Automata Library/T.txt")
        );
        assert_eq!(
            s.libraries().function("phi").unwrap_err().to_string(),
            format!("File does not exist: {global}Automata Library/phi.txt")
        );
    }

    /// A file that exists but does not parse is a *different* error from a missing one —
    /// Java's split is the `IOException` boundary inside `AutomatonReader.readAutomaton`, not
    /// a message-text judgement.
    #[test]
    fn a_malformed_automaton_is_not_reported_as_missing() {
        let (_root, global, session) = walnut_tree("malformed");
        write(
            &global,
            "Word Automata Library/T.txt",
            "msd_2\n\n0 1\n0 -> 7\n",
        );

        let (s, _out) = session_with_capture(&global, &session, false);
        let err = s.libraries().word("T").unwrap_err();
        assert!(
            matches!(err, PredicateEnvError::MalformedAutomaton { .. }),
            "{err:?}"
        );
        // The CLASSIFICATION above is the load-bearing assertion. Deliberately nothing is
        // asserted about the message TEXT: `MalformedAutomaton`'s `Display` ("File does not
        // parse: ...", `wr_logic::predicate_env`) is a placeholder that module already
        // documents as not fixture-ready — real Walnut throws distinct `WalnutException`s
        // (`undefinedStatement`, `nonDeterministicO`, ...) with their own wording. Pinning
        // the placeholder here would advertise golden-fixture coverage that does not exist.
    }

    // ---------------------------------------------------------- PredicateEnv: macros

    /// `readMacroFile` appends each line with no separator, so newlines vanish entirely
    /// (`Predicate.java:503-505`). A macro split across two lines lexes as if glued.
    #[test]
    fn macro_lines_are_concatenated_with_no_separator() {
        let (_root, global, session) = walnut_tree("macro");
        write(&global, "Macro Library/m.txt", "Ex x\n=%0\n");
        write(&global, "Macro Library/glued.txt", "a\nb\n");

        let (s, _out) = session_with_capture(&global, &session, false);
        assert_eq!(s.libraries().macro_text("m").unwrap(), "Ex x=%0");
        assert_eq!(s.libraries().macro_text("glued").unwrap(), "ab");
    }

    /// The macro error prints the BARE name, not the address — unlike every other lookup.
    #[test]
    fn a_missing_macro_reports_the_bare_name() {
        let (_root, global, session) = walnut_tree("macro-missing");
        let (s, _out) = session_with_capture(&global, &session, false);
        assert_eq!(
            s.libraries().macro_text("nope").unwrap_err().to_string(),
            "Macro does not exist: nope"
        );
    }

    #[test]
    fn a_session_macro_overrides_the_global_one() {
        let (_root, global, session) = walnut_tree("macro-override");
        write(&global, "Macro Library/m.txt", "global");
        write(&session, "Macro Library/m.txt", "sessional");

        let (s, _out) = session_with_capture(&global, &session, false);
        assert_eq!(s.libraries().macro_text("m").unwrap(), "sessional");
    }

    // -------------------------------------------------- PredicateEnv: number systems

    /// Standard bases still resolve with no files present at all — Java's
    /// `setAdditionAutomaton` falls back to the programmatic base-*k* adder.
    #[test]
    fn standard_bases_resolve_with_no_custom_base_files() {
        let (_root, global, session) = walnut_tree("standard-ns");
        let (s, _out) = session_with_capture(&global, &session, false);

        let ns = s.libraries().number_system("msd_3").unwrap();
        assert_eq!(ns.name(), "msd_3");
        assert!(ns.is_msd());
    }

    /// **The end-to-end path this unit exists for**: a custom base named only by files on
    /// disk, resolved through `getReadAddressForCustomBases` → `globalOrSessionFile` →
    /// `wr-io` → `NumberSystem::with_custom_base_files`.
    ///
    /// The adder written out is msd base 2's, so the loaded system's addition automaton must
    /// be language-equivalent to the programmatic `msd_2` one — a real semantic check
    /// (`wr_core::equiv`), not "a `NumberSystem` came back".
    #[test]
    fn a_custom_base_is_loaded_from_the_custom_bases_directory() {
        let (_root, global, session) = walnut_tree("custom-base");
        let mut msd2_addition = NumberSystem::new("msd_2").unwrap().addition().clone();
        let adder_path = PathBuf::from(format!("{global}Custom Bases/msd_kk_addition.txt"));
        wr_io::writer::write_automaton_txt(&mut msd2_addition, &adder_path).unwrap();

        let (s, _out) = session_with_capture(&global, &session, false);
        let ns = s.libraries().number_system("msd_kk").unwrap();
        assert_eq!(ns.name(), "msd_kk");
        assert!(ns.is_msd());
        assert!(
            same_language(ns.addition(), &msd2_addition),
            "the file-loaded adder must be the one on disk"
        );

        // ... and with no file at all, the same name is undefined (base "kk" is not a
        // number), which is what makes the assertion above load-bearing.
        let (_root2, global2, session2) = walnut_tree("custom-base-absent");
        let (s2, _out2) = session_with_capture(&global2, &session2, false);
        assert_eq!(
            s2.libraries()
                .number_system("msd_kk")
                .unwrap_err()
                .to_string(),
            "Number system msd_kk is not defined."
        );
    }

    /// A session copy of a custom-base file shadows the global one, per file.
    #[test]
    fn a_session_custom_base_file_overrides_the_global_one() {
        let (_root, global, session) = walnut_tree("custom-base-override");
        let mut base2 = NumberSystem::new("msd_2").unwrap().addition().clone();
        let mut base3 = NumberSystem::new("msd_3").unwrap().addition().clone();
        wr_io::writer::write_automaton_txt(
            &mut base2,
            PathBuf::from(format!("{global}Custom Bases/msd_kk_addition.txt")),
        )
        .unwrap();
        wr_io::writer::write_automaton_txt(
            &mut base3,
            PathBuf::from(format!("{session}Custom Bases/msd_kk_addition.txt")),
        )
        .unwrap();

        let (s, out) = session_with_capture(&global, &session, false);
        let ns = s.libraries().number_system("msd_kk").unwrap();
        assert!(same_language(ns.addition(), &base3));
        assert_ne!(
            ns.addition().alphabet,
            base2.alphabet,
            "the global base-2 adder must NOT be the one that got loaded"
        );
        assert!(out
            .text()
            .contains("Overriding global file with session file:"));
    }

    /// The same override, reached the OTHER way: through a custom base named in a library
    /// file's own **header**, not by a query token.
    ///
    /// Java has one mechanism for both (`ParseMethods.parseAlphabetDeclaration` →
    /// `NumberSystem.getComputeIfAbsent` → `Session.getReadAddressForCustomBases` →
    /// `globalOrSessionFile`), so a session copy must win here exactly as it does above. It is
    /// which number system's *semantics* govern the file's contents that is at stake, so the
    /// two copies are given different alphabets ({0,1} vs {0,1,2}) — the loaded word
    /// automaton's own track alphabet then says, unambiguously, which one was consulted.
    ///
    /// `Word Automata Library/T.txt` itself lives only in the GLOBAL tree: the file being read
    /// and the custom base its header names are resolved independently, and only the latter is
    /// overridden here.
    #[test]
    fn a_session_custom_base_file_overrides_the_global_one_for_a_nested_header_too() {
        // A `*` transition expands over whatever alphabet the header resolves to, so the same
        // body is valid under both candidate number systems -- the alphabet observed after
        // loading is a consequence of the base, not of the body.
        const WILDCARD_LOOP: &str = "msd_kk\n\n0 1\n* -> 0\n";

        let (_root, global, session) = walnut_tree("nested-header-override");
        let mut base2 = NumberSystem::new("msd_2").unwrap().addition().clone();
        let mut base3 = NumberSystem::new("msd_3").unwrap().addition().clone();
        wr_io::writer::write_automaton_txt(
            &mut base2,
            PathBuf::from(format!("{global}Custom Bases/msd_kk_addition.txt")),
        )
        .unwrap();
        wr_io::writer::write_automaton_txt(
            &mut base3,
            PathBuf::from(format!("{session}Custom Bases/msd_kk_addition.txt")),
        )
        .unwrap();
        write(&global, "Word Automata Library/T.txt", WILDCARD_LOOP);

        let (s, out) = session_with_capture(&global, &session, false);
        let t = s.libraries().word("T").unwrap();
        assert_eq!(
            t.alphabet,
            vec![vec![0, 1, 2]],
            "the SESSION copy of msd_kk (base 3) must be the one the header resolved to"
        );
        assert_eq!(t.msd, vec![Some(true)]);
        assert!(
            out.text()
                .contains("Overriding global file with session file:"),
            "the nested lookup goes through globalOrSessionFile, notice included: {:?}",
            out.text()
        );

        // Without the session copy, the very same file resolves to the global base -- which is
        // what makes the assertion above load-bearing rather than a restatement of base 3.
        let (_root2, global2, session2) = walnut_tree("nested-header-global-only");
        let mut base2b = NumberSystem::new("msd_2").unwrap().addition().clone();
        wr_io::writer::write_automaton_txt(
            &mut base2b,
            PathBuf::from(format!("{global2}Custom Bases/msd_kk_addition.txt")),
        )
        .unwrap();
        write(&global2, "Word Automata Library/T.txt", WILDCARD_LOOP);

        let (s2, _out2) = session_with_capture(&global2, &session2, false);
        assert_eq!(s2.libraries().word("T").unwrap().alphabet, vec![vec![0, 1]]);
    }

    /// `getComputeIfAbsent` hands the same instance to every token in a formula — which is
    /// what makes `NumberSystem`'s three memo tables shared, per `PORTING.md`'s Ruling 1.
    #[test]
    fn number_systems_are_memoized_by_name() {
        let (_root, global, session) = walnut_tree("memoize");
        let (s, _out) = session_with_capture(&global, &session, false);

        let a = s.libraries().number_system("msd_2").unwrap();
        let b = s.libraries().number_system("msd_2").unwrap();
        assert!(Rc::ptr_eq(&a, &b));
        let other = s.libraries().number_system("lsd_2").unwrap();
        assert!(!Rc::ptr_eq(&a, &other));
    }

    /// `computeIfAbsent` leaves the map unmodified when the mapping function throws, so a
    /// name that becomes resolvable between two commands resolves on the second try. For a
    /// file-backed environment that is not a theoretical nicety — the user really can drop
    /// the file in between.
    #[test]
    fn a_failed_number_system_lookup_is_not_negatively_cached() {
        let (_root, global, session) = walnut_tree("retry");
        let (s, _out) = session_with_capture(&global, &session, false);

        assert!(s.libraries().number_system("msd_kk").is_err());
        assert!(
            !s.libraries().number_systems.borrow().contains_key("msd_kk"),
            "a failed lookup must not populate the cache"
        );

        let mut adder = NumberSystem::new("msd_2").unwrap().addition().clone();
        wr_io::writer::write_automaton_txt(
            &mut adder,
            PathBuf::from(format!("{global}Custom Bases/msd_kk_addition.txt")),
        )
        .unwrap();
        assert!(s.libraries().number_system("msd_kk").is_ok());
    }

    /// `_neg_` is rejected at name-resolution time, before any file is probed — U5's declared
    /// divergence, mirroring Java's own `isNeg`-before-`setAdditionAutomaton` line order.
    /// Checked with `msd_neg_fib*` files actually PRESENT (and unparseable), so "no file was
    /// found" cannot be what produced the error.
    #[test]
    fn a_negative_base_is_rejected_before_its_files_are_read() {
        let (_root, global, session) = walnut_tree("neg");
        write(&global, "Custom Bases/msd_neg_fib_addition.txt", "garbage");
        write(&global, "Custom Bases/msd_neg_fib.txt", "garbage");

        let (s, _out) = session_with_capture(&global, &session, false);
        let err = s.libraries().number_system("msd_neg_fib").unwrap_err();
        assert!(
            matches!(
                err,
                PredicateEnvError::NumberSystem {
                    source: NumSysError::UnsupportedNegativeBase(_),
                    ..
                }
            ),
            "{err:?}"
        );
    }

    // ------------------------------------------------------------- structure / WB-005

    /// The trait object is what `wr-logic`'s lexer takes; if `FileLibraries` ever stops being
    /// usable as one this fails to compile.
    #[test]
    fn the_session_hands_out_a_predicate_env_trait_object() {
        let (_root, global, session) = walnut_tree("dyn");
        let (s, _out) = session_with_capture(&global, &session, false);
        let env: &dyn PredicateEnv = s.predicate_env();
        assert!(env.number_system("msd_2").is_ok());
    }

    /// `docs/WALNUT-BUGS.md` WB-005, both halves, made unreachable by construction — see this
    /// module's docs. Java's second `setPathsAndNames` call keeps the first call's
    /// `sessionWalnutDir` while recomputing `name` (so the two disagree), and never resets
    /// `globalSession` from `true` back to `false`.
    ///
    /// Scope note: this checks the *consequence* (two independently constructed values agree
    /// with themselves and never share state). It does not, and cannot, catch a future
    /// `&mut self` setter on [`SessionPaths`] — that reintroduction is ruled out structurally,
    /// by there being no mutator in the `impl` block, not by this test.
    #[test]
    fn a_second_session_is_fully_independent_wb_005() {
        // Defect 2: a global session followed by a non-global one. Java's `globalSession`
        // would still be `true` here; ours is a plain constructor argument.
        let first = SessionPaths::new(None, Some("/w/"), true);
        assert!(first.is_global_session());
        let second = SessionPaths::new(None, Some("/w/"), false);
        assert!(!second.is_global_session());

        // Defect 1: `name` and `sessionWalnutDir` are derived together, always. Java's second
        // timestamped setup recomputes only the former.
        let a = SessionPaths::new(None, Some("/w/"), false);
        let b = SessionPaths::new(None, Some("/w/"), false);
        for p in [&a, &b] {
            assert_eq!(
                p.address_for_result(),
                format!("/w/{}Result/", p.name().unwrap()),
                "the reported name must always describe the directory in use"
            );
        }
    }
}
