// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **`::`-suffixed detail printing on non-`eval`/`def`
//! commands** — `docs/BACKLOG-LSD-INFINITE-LOGGING-DISPATCH.md` item 3.
//!
//! # The bug this closes
//!
//! `crates/wr-cli/src/prover.rs`'s `Prover` has a real `logging: Logging` field that
//! gets correctly configured on EVERY command dispatch (`parse_setup`'s
//! `self.logging.configure_for_command(self.print_flag, self.print_details)`, driven by
//! the `:`/`::` suffix, exactly matching Java's `Prover.parseSetup` -> universal
//! `Logging.configureForCommand`). But fifteen call sites across eight `wr-cli` modules
//! (`quotient.rs`'s `right_quotient_command`/`left_quotient_command`, `convert.rs`'s
//! `convert_command`, `automaton_ops.rs`'s `combine_command`/`union_or_intersect` (shared
//! by `union_command`/`intersect_command`)/`concat_pair`/`star`, `alphabet.rs`'s
//! `determine_alphabets_and_ns`/`set_alphabet` (also reached from `reg.rs`'s `reg`),
//! `simple_transforms.rs`'s `fix_lead_zero_command`/`fix_trail_zero_command`/
//! `minimize_command`, `reverse.rs`'s `reverse_command`, `prover_helper.rs`'s
//! `inf_from_address_to`, `test_command.rs`'s `find_accepted`) used to construct their
//! own throwaway `Logging::new()` internally instead of receiving `&mut self.logging` —
//! so `combine c A B;::` (etc.) silently printed nothing, even though real Walnut's
//! `AutomatonLogicalOps`/`WordAutomaton` primitives these commands call have logged this
//! whole time and the Rust ports of those primitives already accept a `&mut Logging`
//! parameter (post-U28).
//!
//! Five of those fifteen (`reverse_command`, `concat_pair`, `star`, `inf_from_address_to`,
//! `find_accepted`) were missed by the first pass and found by adversarial review; so were
//! five `Commands/*.java` log lines this port had simply never emitted at all —
//! `Union.java:76`'s `computed =>:Q states - Tms` (once per `union`/`intersect` fold
//! iteration), `Concat.java:54`/`:61`/`:80`'s `concatenated =>:`/`concat: `/`concat
//! complete: `, and `Star.java:23`/`:33`'s `star: `/`star complete: `. All are now ported,
//! each with a live capture below.
//!
//! This file proves the fix reaches all the way through the real
//! [`wr_cli::prover::Prover::dispatch_for_integration_test`] dispatch path — not just
//! that the free functions compile with a new parameter — by comparing
//! `prover.logging().detailed_log()` after each `::`-suffixed command against real
//! `walnut-java` CLI output, one representative command per family (per this backlog
//! item's own "at least one `::`-suffixed command per family" instruction), plus the two
//! newly-discovered gaps (`fixleadzero`/`minimize`) the backlog item asked to be
//! independently checked against real Java rather than assumed in scope.
//!
//! Every operand automaton below is built with `reg`, over the exact same alphabet
//! declaration and regex as the real capture, so state counts match Java's line for
//! line — this file checks the actual message TEXT (state counts included, matching
//! `tests/golden`'s own `details`-fixture discipline that "state counts survive"), not
//! merely "some text was produced". Timing suffixes (`- Nms`) are the one thing
//! deliberately not checked digit-for-digit (both engines have no obligation to match
//! wall-clock milliseconds) — every assertion below checks a fixed substring that stops
//! just short of the timing digits.
//!
//! ## Capture recipe (reproducible, same discipline as `../CAPTURE.md`)
//!
//! ```bash
//! cd ~/dev/walnut-java     # built with ./mvnw -q clean package -DskipTests -Pfat-jar
//! cat > "Command Files/u_backlog3_capture.txt" <<'EOF'
//! reg A {0,1} "0*1";
//! reg B {0,1} "1*";
//! combine cc A=7;
//! rightquo rq A B;::
//! leftquo lq A B;::
//! combine cc2 A=7;::
//! union u2 A B;::
//! intersect i2 A B;::
//! alphabet arestrict2 {0,1} $A;::
//! fixleadzero fl2 A;::
//! fixtrailzero ft2 A;::
//! minimize m2 cc;::
//! reg r2 {0,1} "0*1";::
//! reverse rv $A;::
//! concat cn A B;::
//! star st A;::
//! inf A;::
//! test A 3;::
//! EOF
//! java -jar target/Walnut-all.jar u_backlog3_capture.txt < /dev/null
//! ```
//!
//! `convert` needed a second, separate capture: `convert`'s source automaton must
//! already carry a real `NumberSystem` (`AutomatonLogicalOps.convertNS` reads the
//! source's own NS to build the conversion and NPEs on a literal `{0,1}`-alphabet
//! automaton with none — confirmed live, not a port bug: `Automata/
//! AutomatonLogicalOps.java:462` dereferences `A.getNS()` unconditionally). So:
//!
//! ```bash
//! cat > "Command Files/u_backlog3_capture2.txt" <<'EOF'
//! reg AN msd_2 "0*1";
//! convert $c3 msd_4 $AN;::
//! EOF
//! java -jar target/Walnut-all.jar u_backlog3_capture2.txt < /dev/null
//! ```
//!
//! Both runs' full console output (every line `Logging.logMessage`'s
//! `consoleLogger.info` printed) is reproduced in each test's own doc comment below,
//! immediately above the assertions transcribed from it.
//!
//! ## Third capture: `getDetailedLog()`, not the console
//!
//! The console is NOT the channel these tests assert on, and for one command family the
//! difference is load-bearing. `Logging.logDetail` (`Main/Logging.java:204-221`) appends
//! to `detailedLog` under `if (printDetails)` alone, but only reaches
//! `consoleLogger.info` under `if (printEnabled && print)`. `NumberSystem`'s constructor
//! brackets itself in `disablePrint()`/`enablePrint()` (WB-039), which clears
//! `printEnabled` — so a custom base's construction text is **silently absent from the
//! console while still fully present in `getDetailedLog()`**, provided it was logged with
//! `logAndPrint` (which ignores `printEnabled` on the way in) rather than `logMessage`
//! (which does not: `logMessage` is gated on `printEnabled && print` *before* calling
//! `logDetail`). `Automaton.applyAllRepresentations`' `"Applying valid representation
//! #i"` (`Automaton.java:261`) is a `logAndPrint`, so it lands in `detailedLog`.
//!
//! Capturing that needs a driver, not the CLI — this project's `phase0-artifacts/
//! CAPTURE.md` throwaway-driver convention (the same one U28 used for `CaptureLog.java`):
//!
//! ```java
//! // DetailProbe.java -- javac -cp target/Walnut-all.jar; run with a scratch Walnut tree
//! // as args[0] (containing Custom Bases/msd_fib*.txt) and commands as args[1..].
//! import Main.Logging; import Main.Prover; import Main.Session;
//! public final class DetailProbe {
//!   public static void main(String[] args) throws Exception {
//!     Session.setPathsAndNames(args[0] + "Session/", args[0], false);
//!     Prover p = new Prover(); Prover.mainProver = p;
//!     for (int i = 1; i < args.length; i++) {
//!       Logging.resetIndent();
//!       p.dispatchForIntegrationTest(args[i], "probe");
//!       System.out.println("### " + args[i]);
//!       System.out.print(Logging.getDetailedLog());
//!     }
//!   }
//! }
//! ```
//!
//! # Two real findings from these captures, both OUT of this backlog item's scope
//!
//! Building these tests against real captured text (rather than "is `detailed_log()`
//! non-empty") surfaced two genuine gaps beyond the throwaway-`Logging` call sites and
//! missing `Commands/*` log lines this item's scope covers. Neither is fixed here — both
//! are `wr-core`-level (not `wr-cli` parameter-threading) and would need their own
//! adversarially-reviewed unit, per `CLAUDE.md`'s "stop and report back" rule for
//! anything beyond plumbing. Two independent adversarial reviewers each confirmed these
//! two — and only these two — are correctly out of scope:
//!
//! 1. **`Determinizing [#n, strategy: S]: Q states` is coupled to `ctx.is_some()`, not
//!    to `should_print_details()`.** Real Java logs this line for EVERY determinize call
//!    once `Logging.shouldPrintDetails()` is true (`DeterminizationStrategies.java:100-112`),
//!    regardless of whether a `[strategy …]`/`[export …]` metacommand is in play — see
//!    `leftquo`/`fixleadzero`'s captures above, neither of which uses `[strategy]` yet
//!    both show the line. `wr_core::determinize::determinize` (this port) only emits it
//!    inside its `if let Some(ctx) = ctx` arm (`determinize.rs:246-269`, its own doc
//!    comment says as much: "`ctx.is_some()` IS that gate"). U32 deliberately scoped
//!    real `DeterminizeContext` threading to the `eval`/`def` path only, and this
//!    backlog item's own instructions say to thread `Logging` only, not
//!    `DeterminizeContext` — so `rightquo`/`leftquo`/`fixleadzero`/`minimize`/etc. still
//!    pass `None` for `ctx` after this fix, and will keep missing this ONE line class
//!    even with the caller's real `Logging` now correctly threaded through everything
//!    else. Confirmed live: `rightquo`'s own capture (no reversal, so no
//!    `determine_and_minimize_with_ctx` call in its path) has NO `Determinizing` line
//!    either in real Java or in this port — consistent both ways; `leftquo`
//!    (`reverse_with_output_with_ctx`), `fixleadzero`
//!    (`determinize_and_minimize_from_with_ctx`) and `reverse`
//!    (`reverse_with_ctx`/`reverse_with_output_with_ctx`) are the three that diverge.
//!    `reverse_prints_detail_text_matching_real_walnut` below pins that explicitly: it
//!    asserts every OTHER line of the real capture, and asserts the absence of this one,
//!    so closing the gap breaks the test rather than leaving it quietly stale.
//! 2. **`AutomatonLogicalOps.convertNS`'s own direct `CONVERTING`/`CONVERTED` log pair
//!    is not ported at all** (`wr_core::logicalops`'s own module docs already flagged
//!    this generally — "None of it is ported... including convertNS's own Logging.indent
//!    ()/dedent() pairs and the four CONVERTING/CONVERTED lines in its two helpers" —
//!    this capture just gives it a live confirmation). `convert_ns` DOES thread the
//!    caller's real `Logging` into the primitives it calls (`totalize`,
//!    `reverse_with_output_with_ctx`), which is what this fix wires up and is why
//!    `convert_prints_detail_text_matching_real_walnut` below sees real
//!    `totalizing`/`Minimizing` text — it just never calls `logging.log_message` itself
//!    for its own two announcement lines.

use std::fs;
use std::path::{Path, PathBuf};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::logging::Logging;

fn temp_session(tag: &str) -> (Session, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-cli-logging-{tag}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
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
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    (session, dir)
}

/// As [`temp_session`], additionally seeded with the real `msd_fib` custom-base files —
/// same fixture copy `lsd_custom_base.rs` uses, per that file's own "one copy per crate"
/// convention.
fn temp_session_with_msd_fib(tag: &str) -> (Session, PathBuf) {
    let (session, dir) = temp_session(tag);
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/phase3a_checkpoint");
    for name in ["msd_fib.txt", "msd_fib_addition.txt"] {
        fs::copy(src.join(name), dir.join("Custom Bases").join(name)).unwrap_or_else(|e| {
            panic!("must be able to install fixtures/phase3a_checkpoint/{name}: {e}")
        });
    }
    (session, dir)
}

/// Every writer is a sink — the `Logging` console/error writers included, per
/// `crates/wr-cli/src/join.rs`'s tests' convention. `Logging::new()` would default its
/// console writer to the real process stdout, so `cargo test` would dump every one of
/// these `::`-suffixed commands' detail text into the test output. `detailed_log()` (the
/// buffer every assertion below reads) is populated independently of the console writer,
/// exactly as Java's `logDetail` appends to `detailedLog` independently of
/// `consoleLogger` — so sinking the console costs no coverage.
fn fresh_prover(session: Session) -> Prover {
    Prover::with_output(
        session,
        Logging::with_writers(Box::new(std::io::sink()), Box::new(std::io::sink())),
        Box::new(std::io::sink()),
    )
}

/// Dispatches `command` (already `;`/`:`/`::`-terminated) and returns
/// `prover.logging().detailed_log()` afterward — `parse_setup` resets that buffer on
/// every dispatch, so this is exactly `command`'s own detail text, nothing leaked from
/// an earlier command in the same test.
fn dispatch_and_get_details(prover: &mut Prover, command: &str) -> String {
    prover
        .dispatch_for_integration_test(command, "u_backlog3")
        .unwrap_or_else(|e| panic!("`{command}` must dispatch without error, got {e:?}"));
    prover.logging().detailed_log().to_string()
}

/// `rightquo`/`leftquo` (`quotient.rs`) — both go through the SAME throwaway-logger fix,
/// so one capture (`rightquo`) plus a second (`leftquo`, which itself calls
/// `right_quotient` internally via `reverse`+`reverse`) covers both.
///
/// Real `walnut-java` console output for `rightquo rq A B;::` (`A`/`B` from `reg A {0,1}
/// "0*1";`/`reg B {0,1} "1*";`):
///
/// ```text
/// right quotient: 2 state A with 1 state A
/// computing &:2 states - 1 states
///  computing cross product:2 states - 1 states
///  computed cross product:2 states - 7ms
///  Minimizing: 2 states.
///  Minimized:2 states - 1ms.
/// computed &:2 states - 11ms
/// computing &:1 states - 1 states
///  computing cross product:1 states - 1 states
///  computed cross product:1 states - 0ms
///  Minimizing: 1 states.
///  Minimized:1 states - 0ms.
/// computed &:1 states - 0ms
///  Minimizing: 2 states.
///  Minimized:2 states - 0ms.
/// right quotient complete: 2 states - 15ms
/// ```
#[test]
fn rightquo_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("rightquo");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");
    dispatch_and_get_details(&mut prover, "reg B {0,1} \"1*\";");

    let details = dispatch_and_get_details(&mut prover, "rightquo rq A B;::");
    for expected in [
        "right quotient: 2 state A with 1 state A",
        "computing &:2 states - 1 states",
        "computing cross product:2 states - 1 states",
        "Minimizing: 2 states.",
        "Minimized:2 states",
        "computed &:2 states",
        "right quotient complete: 2 states",
    ] {
        assert!(
            details.contains(expected),
            "rightquo's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `leftquo` — Real `walnut-java` console output for `leftquo lq A B;::` (same `A`/`B`):
///
/// ```text
/// left quotient: 2 state A with 1 state A
/// reversing:2 states
///  Determinizing [#0, strategy: SC]: 2 states
///  Determinized: 2 states - 2ms
///  Minimizing: 2 states.
///  Minimized:2 states - 0ms.
/// reversed:2 states - 5ms
/// reversing:1 states
///  Determinizing [#1, strategy: SC]: 1 states
///  Determinized: 1 states - 0ms
///  Minimizing: 1 states.
///  Minimized:1 states - 0ms.
/// reversed:1 states - 0ms
/// right quotient: 2 state A with 1 state A
///   ... (the same rightquo shape as above, on the reversed operands) ...
/// right quotient complete: 2 states - 2ms
/// reversing:2 states
///  Determinizing [#2, strategy: SC]: 2 states
///  Determinized: 3 states - 0ms
///  Minimizing: 3 states.
///  Minimized:3 states - 0ms.
/// reversed:3 states - 0ms
/// left quotient complete: 3 states - 7ms
/// ```
///
/// **Not checked**: the three `Determinizing [#n, strategy: SC]: Q states` lines — see
/// this file's module docs, finding 1 (`ctx` is `None` on this path, so this port never
/// emits that specific line, independent of `Logging` wiring).
#[test]
fn leftquo_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("leftquo");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");
    dispatch_and_get_details(&mut prover, "reg B {0,1} \"1*\";");

    let details = dispatch_and_get_details(&mut prover, "leftquo lq A B;::");
    for expected in [
        "left quotient: 2 state A with 1 state A",
        "reversing:2 states",
        "Determinized: 2 states",
        "reversed:2 states",
        "left quotient complete: 3 states",
    ] {
        assert!(
            details.contains(expected),
            "leftquo's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `convert` (`convert.rs`). Real `walnut-java` console output for `convert $c3 msd_4
/// $AN;::` (`AN` from `reg AN msd_2 "0*1";`):
///
/// ```text
/// totalizing:2 states
/// totalized:3 states - 2ms
///  Converting: msd_2 to msd_4, 3 states
///  Converted: msd_2 to msd_4, 3 states - 3ms
///   Minimizing: 3 states.
///   Minimized:3 states - 2ms.
///   Minimizing: 3 states.
///   Minimized:2 states - 0ms.
///  computing =>:3 states - 2 states
///   ...
/// ```
///
/// **Not checked**: `Converting: …`/`Converted: …` — see this file's module docs,
/// finding 2 (`convert_ns`'s own two direct log lines are simply not ported, a
/// pre-existing `wr-core`-level gap `logicalops.rs`'s own module docs already flagged;
/// this fix only rewires what `convert_ns` was already given to log through
/// `totalize`/`reverse_with_output_with_ctx`, which the assertions below confirm).
#[test]
fn convert_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("convert");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg AN msd_2 \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "convert $c3 msd_4 $AN;::");
    for expected in [
        "totalizing:2 states",
        "totalized:3 states",
        "Minimizing: 3 states.",
    ] {
        assert!(
            details.contains(expected),
            "convert's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `combine`/`union`/`intersect` (`automaton_ops.rs`) — two genuinely separate call
/// sites (`combine_command`'s own `combine(...)` call, and `union_or_intersect`'s shared
/// `or`/`and` call reached from both `union_command` and `intersect_command`), so this
/// covers both with `combine` (a single-operand combine, so only `totalize` fires — no
/// cross product with zero subautomata) and `union` (two operands, so the fold's cross
/// product fires). `intersect` shares `union_or_intersect`'s exact code path with
/// `union` (only the boolean op differs), so it is not separately captured.
///
/// Real `walnut-java` console output for `combine cc2 A=7;::`:
///
/// ```text
///  totalizing:2 states
///  totalized:3 states - 0ms
/// ```
///
/// and for `union u2 A B;::`:
///
/// ```text
/// computing |:2 states - 1 states
///  totalizing:2 states
///  totalized:3 states - 1ms
///  totalizing:1 states
///  totalized:2 states - 0ms
///  computing cross product:3 states - 2 states
///  computed cross product:6 states - 2ms
///  Minimizing: 6 states.
///  Minimized:4 states - 0ms.
/// computed |:4 states - 4ms
/// computed =>:4 states - 5ms
/// ```
///
/// That last line is `Union.unionOrIntersect`'s own
/// `Logging.logMessage(COMPUTED + " =>:" + first.fa.getQ() + " states - " + … + "ms")`
/// (`Union.java:76`), emitted once per fold iteration with `timeBefore` captured at the
/// top of the iteration (`:54`, before the operand is even read from the library). An
/// earlier draft of this transcript omitted it, and the port omitted the line entirely;
/// adversarial review caught both. It is asserted below for `union` and (shared code
/// path, `Intersect.java:36` calls straight into `Union.unionOrIntersect`) for
/// `intersect`.
#[test]
fn combine_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("combine");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "combine cc2 A=7;::");
    for expected in ["totalizing:2 states", "totalized:3 states"] {
        assert!(
            details.contains(expected),
            "combine's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn union_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("union");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");
    dispatch_and_get_details(&mut prover, "reg B {0,1} \"1*\";");

    let details = dispatch_and_get_details(&mut prover, "union u2 A B;::");
    for expected in [
        "computing |:2 states - 1 states",
        "computing cross product:3 states - 2 states",
        "computed cross product:6 states",
        "Minimizing: 6 states.",
        "computed |:4 states",
        "computed =>:4 states",
    ] {
        assert!(
            details.contains(expected),
            "union's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `intersect` — the other half of `union_or_intersect`'s shared fold. Only captured
/// here for `Union.java:76`'s `computed =>:` line (the whole reason `intersect` gets its
/// own test now: it is emitted by the SHARED function, so a fix that only reached
/// `union_command` would pass the test above and still be wrong). Real `walnut-java`
/// console output for `intersect i2 A B;::` (same `A`/`B`):
///
/// ```text
/// computing &:2 states - 1 states
///  computing cross product:2 states - 1 states
///  computed cross product:2 states - 0ms
///  Minimizing: 2 states.
///  Minimized:2 states - 0ms.
/// computed &:2 states - 1ms
/// computed =>:2 states - 1ms
/// ```
#[test]
fn intersect_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("intersect");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");
    dispatch_and_get_details(&mut prover, "reg B {0,1} \"1*\";");

    let details = dispatch_and_get_details(&mut prover, "intersect i2 A B;::");
    for expected in [
        "computing &:2 states - 1 states",
        "computing cross product:2 states - 1 states",
        "computed cross product:2 states",
        "Minimizing: 2 states.",
        "computed &:2 states",
        "computed =>:2 states",
    ] {
        assert!(
            details.contains(expected),
            "intersect's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `reverse` (`reverse.rs`) — missed entirely by this pass's first draft (found by
/// adversarial review): `reverse_command` called the plain `reverse_with_output`/
/// `reverse` wrappers, each of which constructs a throwaway `Logging::new()` internally,
/// rather than the already-existing, already-instrumented `reverse_with_output_with_ctx`/
/// `reverse_with_ctx`.
///
/// Real `walnut-java` console output for `reverse rv $A;::` (`A` from `reg A {0,1}
/// "0*1";`):
///
/// ```text
/// reversing:2 states
///  Determinizing [#0, strategy: SC]: 2 states
///  Determinized: 2 states - 4ms
///  Minimizing: 2 states.
///  Minimized:2 states - 2ms.
/// reversed:2 states - 12ms
/// ```
#[test]
fn reverse_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("reverse");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "reverse rv $A;::");
    for expected in [
        "reversing:2 states",
        " Determinized: 2 states",
        " Minimizing: 2 states.",
        " Minimized:2 states",
        "reversed:2 states",
    ] {
        assert!(
            details.contains(expected),
            "reverse's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    // The one line of that capture this port deliberately still does not emit -- see
    // this file's module docs, finding 1 (`ctx` is `None` on every non-`eval`/`def`
    // path, and `wr_core::determinize` gates this line on `ctx.is_some()`).
    assert!(
        !details.contains("Determinizing ["),
        "known open gap: `Determinizing [#n, strategy: S]` is gated on `ctx.is_some()`, \
         which is `None` here -- if this now fails, that gap was just closed and this \
         assertion (plus this file's module docs, finding 1) should be updated, not \
         deleted; got:\n{details}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// `split` — the SIXTEENTH call site of the same bug, found by adversarial review of
/// `docs/NEGATIVE-BASE-SPLIT-DISPATCH.md`'s Layer B, months after this backlog item
/// nominally closed the class.
///
/// `crate::split::process_split` threaded the caller's real `Logging` into every `and`,
/// `arithmetic_const_c`, `combine` and `apply_all_representations` call — and then used
/// the plain `wr_core::quantify::quantify` wrapper, which substitutes a throwaway
/// `Logging::new()`, for exactly one primitive. The whole per-subautomaton
/// `quantifying:` / `quantified:` / `fixing leading zeros:` / `fixed leading zeros:`
/// block therefore vanished (23 lines on the two-track case below, live-diffed against
/// the real jar).
///
/// **No golden fixture would ever have caught this**: none of the corpus's 15
/// `split`/`rsplit` fixtures carries a `::` suffix (checked against
/// `phase0-artifacts/test-manifest.json`). That is precisely why this file exists.
///
/// Real `walnut-java` console output for `split slout sl[+][-];::` over
/// `?msd_2 x < y`, the four `quantify`-owned line kinds (there are two subautomata, so
/// the block appears twice):
///
/// ```text
/// quantifying:1 states
///   Minimizing: 1 states.
///   Minimized:1 states - 0ms.
/// quantified:1 states - 0ms
/// fixing leading zeros:1 states
///  Determinizing [#0, strategy: SC]: 1 states
///  Determinized: 1 states - 1ms
///  Minimizing: 1 states.
///  Minimized:1 states - 0ms.
/// fixed leading zeros:1 states - 2ms
/// ```
#[test]
fn split_prints_the_quantify_detail_text_real_walnut_does() {
    let (session, dir) = temp_session("split");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "eval sl \"?msd_2 x < y\";");

    let details = dispatch_and_get_details(&mut prover, "split slout sl[+][-];::");
    for expected in [
        "quantifying:",
        "quantified:",
        "fixing leading zeros:",
        "fixed leading zeros:",
    ] {
        assert!(
            details.contains(expected),
            "split's detailed_log must contain {expected:?} -- this is the exact line \
             class the throwaway-`Logging::new()` bug swallowed; got:\n{details}"
        );
    }
    // Two subautomata (`uncombine` over the operand's two distinct outputs), so the block
    // really does appear twice -- a single occurrence would mean the real `Logging`
    // reached one call and not the other.
    assert_eq!(
        details.matches("fixed leading zeros:").count(),
        2,
        "one `fixing/fixed leading zeros` block per subautomaton; got:\n{details}"
    );
    // Same known open gap as `reverse` above: `ctx` is `None` on every non-`eval`/`def`
    // path. Java prints `Determinizing [#0…]`/`[#1…]`/`[#2…]` inside this very block, so
    // this assertion also records that split's metacommand INDICES differ from Java's --
    // see `crate::split::process_split`'s own comment on that deliberate boundary.
    assert!(
        !details.contains("Determinizing ["),
        "known open gap (U32-scoped): if this now fails the gap was closed and this \
         assertion, plus `crate::split::process_split`'s comment, should be updated \
         rather than deleted; got:\n{details}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// `concat` (`automaton_ops.rs`'s `concat_pair` + `concat_command`'s fold) — two
/// separate adversarial-review findings in one command: `concat_pair` called the plain
/// `determinize_and_minimize()` (throwing away its own `Logging` internally) despite
/// already holding the caller's real one on the surrounding lines, AND all three of
/// `Concat.java`'s own direct `Logging.logMessage` calls (`:61` `concat: `, `:80`
/// `concat complete: `, `:54` `concatenated =>:`) had never been ported at all.
///
/// Real `walnut-java` console output for `concat cn A B;::`:
///
/// ```text
/// concat: 2 state automaton with 1 state automaton
///  Minimizing: 3 states.
///  Minimized:2 states - 0ms.
/// concat complete: 2 states - 0ms
/// concatenated =>:2 states - 1ms
/// ```
///
/// The single-space indent on the two middle lines is `Automaton.determinizeAndMinimize`'s
/// own `Logging.indent()`/`dedent()` bracket (`Automaton.java:384`/`:397`), so it is
/// asserted verbatim here — it is the part that proves the caller's real `Logging`
/// genuinely reached that call, not just that some text was produced.
#[test]
fn concat_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("concat");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");
    dispatch_and_get_details(&mut prover, "reg B {0,1} \"1*\";");

    let details = dispatch_and_get_details(&mut prover, "concat cn A B;::");
    for expected in [
        "concat: 2 state automaton with 1 state automaton",
        " Minimizing: 3 states.",
        " Minimized:2 states",
        "concat complete: 2 states",
        "concatenated =>:2 states",
    ] {
        assert!(
            details.contains(expected),
            "concat's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `star` (`automaton_ops.rs`'s `star`) — the same pair of findings as `concat`:
/// a throwaway `determinize_and_minimize()`, plus `Star.java:23`/`:33`'s own two
/// `Logging.logMessage` lines never having been ported.
///
/// Real `walnut-java` console output for `star st A;::`:
///
/// ```text
/// star: 2 state automaton
///  Minimizing: 3 states.
///  Minimized:2 states - 0ms.
/// star complete: 2 states - 0ms
/// ```
#[test]
fn star_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("star");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "star st A;::");
    for expected in [
        "star: 2 state automaton",
        " Minimizing: 3 states.",
        " Minimized:2 states",
        "star complete: 2 states",
    ] {
        assert!(
            details.contains(expected),
            "star's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `inf` (`prover_helper.rs`'s `inf_from_address_to`) and `test` (`test_command.rs`'s
/// `find_accepted`) — two more throwaway-`Logging::new()` sites found by adversarial
/// review, outside the originally-enumerated set because they live in modules the first
/// pass never opened. Both are the same one call: `AutomatonLogicalOps.removeLeadingZeros`
/// (`ProverHelper.java:52` / `Test.java:43`), whose own `removing leading zeros for:`/
/// `removed:` pair was being thrown away.
///
/// Real `walnut-java` console output for `inf A;::` and `test A 3;::` (`A` from
/// `reg A {0,1} "0*1";`) — identical detail text, differing only in the non-`Logging`
/// console lines each command prints afterward (`Automaton accepts infinite values,
/// including regex:([0])*[1]` / the three accepted inputs `1`, `01`, `001`), which go to
/// the `Prover`'s own stdout sink rather than `detailed_log()`:
///
/// ```text
/// removing leading zeros for:2 states
/// removed:2 states - 0ms
/// ```
#[test]
fn inf_and_test_print_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("inf-test");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");

    for command in ["inf A;::", "test A 3;::"] {
        let details = dispatch_and_get_details(&mut prover, command);
        for expected in ["removing leading zeros for:2 states", "removed:2 states"] {
            assert!(
                details.contains(expected),
                "`{command}`'s detailed_log must contain {expected:?}; got:\n{details}"
            );
        }
    }
    fs::remove_dir_all(&dir).ok();
}

/// `alphabet` (`alphabet.rs`'s `set_alphabet`, reached via `determine_alphabets_and_ns`
/// too). Real `walnut-java` console output for `alphabet arestrict2 {0,1} $A;::`:
///
/// ```text
/// setting alphabet to [[0, 1]]
///  Minimizing: 2 states.
///  Minimized:2 states - 0ms.
/// set alphabet complete:0ms
/// ```
///
/// **Two of these four lines are a known, documented, still-open gap** —
/// `"setting alphabet to …"` and `"set alphabet complete:…ms"` are logged directly by
/// `Automaton.setAlphabet` itself (`Automaton.java:192-197`/`225`), not by any of the
/// primitives `set_alphabet` calls, so wiring the CALLER's `Logging` through (this
/// backlog item's actual scope) does not produce them — see `alphabet.rs`'s
/// `set_alphabet` doc comment. This test pins BOTH halves of that honestly: the two
/// lines this fix DOES now produce (proving the wiring works), and an explicit
/// `!contains` on the two it deliberately still doesn't (so a future fix that adds them
/// updates this test rather than silently leaving it stale).
#[test]
fn alphabet_prints_detail_text_matching_real_walnut_for_the_wired_portion() {
    let (session, dir) = temp_session("alphabet");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "alphabet arestrict2 {0,1} $A;::");
    for expected in ["Minimizing: 2 states.", "Minimized:2 states"] {
        assert!(
            details.contains(expected),
            "alphabet's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    for still_missing in ["setting alphabet to", "set alphabet complete"] {
        assert!(
            !details.contains(still_missing),
            "known open gap: `Automaton.setAlphabet`'s own two direct log lines \
             ({still_missing:?}) are not ported yet -- if this now fails, that gap was \
             just closed and this assertion (and the module doc above it) should be \
             updated, not deleted"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// The `Logging.indent()`/`dedent()` bracket around `applyAllRepresentationsWithOutput`
/// (`Automaton.java:218-220`) — the one genuinely NEW behavior this pass introduced into
/// `set_alphabet`, as opposed to rerouting an existing throwaway logger.
/// `alphabet_prints_detail_text_matching_real_walnut_for_the_wired_portion` above cannot
/// see it: its `{0,1}` literal alphabet carries no `NumberSystem` at all, so
/// `applyAllRepresentationsWithOutput`'s loop body never runs and the bracket is a no-op
/// (every line it could indent is absent). This case uses `msd_fib`, a real custom base
/// with `useAllRepresentations()`, where the loop DOES run a `crossProduct` and the
/// bracket becomes observable as a one-space indent on that cross product's own two
/// lines.
///
/// Captured live with `DetailProbe` (this file's module docs, "Third capture") — the
/// console shows the identical text here, so `java -jar target/Walnut-all.jar` on a
/// command file containing `reg A msd_fib "0*1"; alphabet ar msd_fib $A;::` reproduces it
/// too:
///
/// ```text
/// setting alphabet to [msd_fib]
///  Minimizing: 2 states.
///  Minimized:2 states - 0ms.
///  computing cross product:2 states - 2 states
///  computed cross product:2 states - 2ms
/// set alphabet complete:3ms
/// ```
///
/// The `Minimizing:`/`Minimized:` pair is indented by `determinizeAndMinimize`'s OWN
/// bracket (`:384`), not this one, so it proves nothing about `:218-220`; the two
/// `cross product` lines are reached only through `applyAllRepresentationsWithOutput`
/// and are therefore the ones asserted in their indented form below. (The two
/// `setAlphabet`-direct lines remain the documented open gap the sibling test pins.)
#[test]
fn set_alphabet_indents_apply_all_representations_like_real_walnut() {
    let (session, dir) = temp_session_with_msd_fib("alphabet-indent");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A msd_fib \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "alphabet ar msd_fib $A;::");
    for expected in [
        " computing cross product:2 states - 2 states",
        " computed cross product:2 states",
    ] {
        assert!(
            details.contains(expected),
            "set_alphabet's detailed_log must contain {expected:?} -- with the LEADING \
             SPACE, which is exactly `Logging.indent()`'s effect at \
             `Automaton.java:218`; got:\n{details}"
        );
    }
    // The bracket must also be balanced: the next line logged after `set_alphabet`
    // returns has to be back at indent 0, not permanently shifted right. Nothing in
    // `setAlphabet` logs after the `dedent()` in this port yet (the `set alphabet
    // complete:` line is the documented open gap), so this checks it via a following
    // command instead.
    let after = dispatch_and_get_details(&mut prover, "reverse rv $A;::");
    assert!(
        after.starts_with("reversing:"),
        "`dedent()` must have restored indent 0 -- the next command's first line would \
         otherwise be shifted right; got:\n{after}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// `fixleadzero` (`simple_transforms.rs`) — one of the two gaps this backlog item's own
/// investigation step found beyond the originally-enumerated eight (`fix_lead_zero_command`
/// called the plain `fix_leading_zeros_problem` wrapper, itself a `Logging::new()`
/// throwaway, rather than `fix_leading_zeros_problem_with_ctx` with the real logger).
///
/// Real `walnut-java` console output for `fixleadzero fl2 A;::`:
///
/// ```text
/// fixing leading zeros:2 states
///  Determinizing [#0, strategy: SC]: 2 states
///  Determinized: 2 states - 0ms
///  Minimizing: 2 states.
///  Minimized:2 states - 0ms.
/// fixed leading zeros:2 states - 0ms
/// ```
///
/// **Not checked**: the `Determinizing [#0, strategy: SC]: 2 states` line — see this
/// file's module docs, finding 1.
#[test]
fn fixleadzero_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("fixleadzero");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "fixleadzero fl2 A;::");
    for expected in [
        "fixing leading zeros:2 states",
        "Minimizing: 2 states.",
        "fixed leading zeros:2 states",
    ] {
        assert!(
            details.contains(expected),
            "fixleadzero's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `fixtrailzero` (`simple_transforms.rs`) — the ORIGINALLY-enumerated eighth site.
/// Deliberately reused the same `A` as the other cases, which happens to take the
/// no-op branch (`fixTrailingZerosProblem`'s "no change necessary" shape, since `A`'s
/// language is already closed under removing a trailing zero) -- a single,
/// partner-less line, unlike every other command captured in this file. Real
/// `walnut-java` console output for `fixtrailzero ft2 A;::`:
///
/// ```text
/// fixing trailing zeros: no change necessary.
/// ```
#[test]
fn fixtrailzero_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("fixtrailzero");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");

    let details = dispatch_and_get_details(&mut prover, "fixtrailzero ft2 A;::");
    assert!(
        details.contains("fixing trailing zeros: no change necessary."),
        "fixtrailzero's detailed_log must contain the no-op line; got:\n{details}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// `minimize` (`simple_transforms.rs`) — the second of the two gaps beyond the original
/// eight (`minimize_command` called the plain `minimize_self_with_output` wrapper,
/// itself a `Logging::new()` throwaway). `cc` (the operand, a genuine WORD automaton in
/// `Word Automata Library/`) is built by a prior, non-`::` `combine cc A=7;`.
///
/// Real `walnut-java` console output for `minimize m2 cc;::`:
///
/// ```text
///  Minimizing: 3 states.
///  Minimized:3 states - 0ms.
///  Minimizing: 3 states.
///  Minimized:2 states - 0ms.
/// computing =>:3 states - 2 states
///  totalizing:3 states
///  totalized:3 states - 1ms
///  totalizing:2 states
///  totalized:3 states - 0ms
///  computing cross product:3 states - 3 states
///  computed cross product:3 states - 0ms
/// computed =>:3 states - 1ms
///  totalizing:3 states
///  totalized:3 states - 0ms
/// ```
#[test]
fn minimize_prints_detail_text_matching_real_walnut() {
    let (session, dir) = temp_session("minimize");
    let mut prover = fresh_prover(session);
    dispatch_and_get_details(&mut prover, "reg A {0,1} \"0*1\";");
    dispatch_and_get_details(&mut prover, "combine cc A=7;");

    let details = dispatch_and_get_details(&mut prover, "minimize m2 cc;::");
    for expected in [
        "Minimizing: 3 states.",
        "Minimized:3 states",
        "computing =>:3 states - 2 states",
        "computing cross product:3 states - 3 states",
    ] {
        assert!(
            details.contains(expected),
            "minimize's detailed_log must contain {expected:?}; got:\n{details}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// `reg` (`reg.rs`) — shares `alphabet.rs`'s `determine_alphabets_and_ns`, which is the
/// call site that reaches `PredicateEnv::number_system` with the caller's real
/// `Logging`; `alphabet_prints_detail_text_matching_real_walnut_for_the_wired_portion`
/// above already proves that call site's wiring works (same function, same fix).
///
/// **An earlier draft of this test was a weak smoke test resting on a false premise, and
/// adversarial review caught it.** That draft observed that `reg r2 msd_fib "0*1";::`
/// prints nothing to real Walnut's CONSOLE and concluded real Walnut "logs nothing here",
/// so it asserted only that the command still dispatched. The console is the wrong
/// channel: `NumberSystem`'s constructor is bracketed in `disablePrint()`/`enablePrint()`
/// (WB-039), which clears `printEnabled`, and `Logging.logDetail` gates the console on
/// `printEnabled && print` while gating `detailedLog` on `printDetails` **alone**
/// (`Main/Logging.java:204-221`). `Automaton.applyAllRepresentations`' `"Applying valid
/// representation #i"` (`Automaton.java:261`) is a `logAndPrint` — which, unlike
/// `logMessage`, does not re-check `printEnabled` on the way in — so it reaches
/// `detailedLog` even with the console suppressed.
///
/// Confirmed live with `DetailProbe` (this file's module docs, "Third capture"), on a
/// completely fresh session so `msd_fib` is genuinely constructed for the first time:
///
/// ```text
/// $ java -cp target/Walnut-all.jar:. DetailProbe "$SCRATCH/" 'reg r2 msd_fib "0*1";::'
/// [console] Set from brics:2 states - 6ms
/// [getDetailedLog()]
/// Applying valid representation #0
/// Applying valid representation #1
/// Applying valid representation #2
/// Applying valid representation #0
/// Applying valid representation #1
/// Applying valid representation #0
/// Applying valid representation #1
/// ```
///
/// Those are exactly the seven lines `crates/wr-core/src/numsys.rs`'s
/// `a_cold_msd_fib_construction_logs_exactly_these_seven_lines` already pins for the
/// equivalent direct `NumberSystem::with_custom_base_files` call — so this test is the
/// end-to-end counterpart of that unit test: it proves the caller's real `Logging`
/// reaches `PredicateEnv::number_system` through `determine_alphabets_and_ns` (this
/// backlog item's actual scope), whereas the previous throwaway `Logging::new()` would
/// have discarded all seven.
///
/// `Set from brics:…` is separately NOT expected: it is not `wr_core::regex`-logged at
/// all (`AutomatonDFA::from_encoded_regex` takes no `Logging` parameter), a pre-existing
/// gap this pass neither closed nor claimed to.
#[test]
fn reg_over_a_custom_base_prints_the_construction_detail_text_real_walnut_does() {
    let (session, dir) = temp_session_with_msd_fib("reg");
    let mut prover = fresh_prover(session);

    let details = dispatch_and_get_details(&mut prover, "reg r2 msd_fib \"0*1\";::");
    assert_eq!(
        details.lines().collect::<Vec<_>>(),
        [
            "Applying valid representation #0",
            "Applying valid representation #1",
            "Applying valid representation #2",
            "Applying valid representation #0",
            "Applying valid representation #1",
            "Applying valid representation #0",
            "Applying valid representation #1",
        ],
        "reg over a cold custom base must reproduce real Walnut's `getDetailedLog()` \
         burst exactly (see this test's doc comment for the live capture)"
    );

    // A second `reg` over the SAME base logs nothing: `PredicateEnv::number_system`
    // memoizes, so the construction happens once per session -- same as Java's
    // `NumberSystem.getComputeIfAbsent`. This is the asymmetry `tests/golden`'s
    // construction-recording mechanism exists for.
    let again = dispatch_and_get_details(&mut prover, "reg r3 msd_fib \"1*0\";::");
    assert!(
        again.is_empty(),
        "the memoized second lookup must not re-log the construction; got:\n{again}"
    );

    fs::remove_dir_all(&dir).ok();
}
