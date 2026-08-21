// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Differential coverage for **WB-035** (`docs/WALNUT-BUGS.md`), the `bugfix/wb-035`
//! follow-up to `docs/WALNUT-JAVA-BUGFIX-DISPATCH.md`'s downstream port workflow (PR-9).
//! Checked against real `walnut-java` output **captured against the FIXED branch**
//! `bugfix/wb-035` (commit `7f54eff`, stacked on `bugfix/wb-038` (`601a9d2`)), **not
//! mainline** — see `java_bugfix_wb002.rs`'s module docs for why that is this project's
//! `../CAPTURE.md` discipline for these follow-up units.
//!
//! # `Transducer.transduceNonDeterministic`'s dead-state marker was one number doing two
//! # jobs it did not belong to
//!
//! When the input automaton `M` is **partial**, Walnut totalizes it with a distinguished
//! dead state whose output is `min(M.O) - 1`, extends the transducer so that reading that
//! dead letter loops in place, transduces, and then deletes every result state carrying
//! that same value as its output. That single number was used raw in both roles:
//!
//! * as an **encoded transducer INPUT symbol** — every other site in `Transducer.java`
//!   goes through `richAlphabet.encode(List.of(v))`, i.e. `A[0].indexOf(v)`, a *position*
//!   in the transducer's input alphabet rather than the value itself. Position and value
//!   coincide only when `A[0] == [0, 1, …, k-1]` and `min(M.O) == 0` (which makes the
//!   value `-1`, also `List.indexOf`'s not-found answer). Shift either and the dead-state
//!   self-loop lands on a real letter's slot — silently swapping real and dead states, or
//!   crashing with `NullPointerException` at `Transducer.createMap`;
//! * as the **marker in the RESULT's OUTPUT alphabet** — but the result's outputs come
//!   from the *transducer's* `sigma`, which has nothing to do with `M`'s outputs. A
//!   transducer that legitimately emits `min(M.O) - 1` had those real states deleted.
//!
//! The fix (`walnut-java` commit `7f54eff`) relabels the dead state onto a value strictly
//! below both `M`'s outputs and the transducer's input alphabet, appends that value to a
//! *copy* of the transducer's input-alphabet track so `encode` finds it at a slot of its
//! own, keys the self-loop on that encoded position, and marks the dead states with a
//! separate value one below everything the transducer can emit.
//!
//! # What this file asserts
//!
//! Each case drives `wr-cli`'s real dispatch loop (`Prover::read_buffer`, the same path the
//! CLI uses) on the same library files the capture session used, then compares the
//! automaton it wrote to `Word Automata Library/` against the one **real fixed
//! `walnut-java` wrote for the identical command**, two ways:
//!
//! 1. [`wr_core::equiv::automaton_language_equivalent`] — this project's semantic-
//!    equivalence oracle and `CLAUDE.md`'s default comparison. Unlike
//!    `java_bugfix_wb021.rs`/`java_bugfix_wb016.rs`, this is *not* a writer-fidelity unit,
//!    so there is no reason to reach for byte comparison; the claim is about the language
//!    the construction computes.
//! 2. …plus a per-word **output** comparison over every input word up to length 4
//!    ([`assert_same_dfao`]). The oracle above is `EqualityUtils.faEqual`'s notion —
//!    acceptance only — which for a DFAO collapses every non-zero output to "accepting".
//!    That is genuinely enough to catch every case here (each divergence moves a
//!    transition, not just an output value), but it would NOT catch a transduction that
//!    got the right shape with the wrong emitted values, which is exactly half (2)'s
//!    failure mode in general. So the stronger check runs beside it rather than instead of
//!    it, and both must pass.
//!
//! Every expectation in this file is a captured artifact, not a hand-derived one:
//! `../fixtures/wb035/*.txt` are byte-for-byte the files the fixed jar wrote.
//!
//! ## Capture recipe (reproducible)
//!
//! Built in an isolated worktree, per this project's standing shared-checkout-safety rule
//! (`~/dev/walnut-java` may have other agents committing to it concurrently) — and note
//! `bugfix/wb-035` was ALREADY checked out at the main `~/dev/walnut-java` working tree
//! when this was captured, so the worktrees below are added by commit hash (detached), not
//! by branch name, to avoid git's "branch already checked out" refusal (the same situation
//! `java_bugfix_wb021.rs`/`java_bugfix_wb032.rs`/`java_bugfix_wb038.rs` hit). **Both** the
//! fixed commit and its parent are built, because the point of this unit is a before/after
//! difference and this port should not take upstream's word for either half:
//!
//! ```bash
//! git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb035     7f54eff
//! git -C ~/dev/walnut-java worktree add --detach /tmp/walnut-java-wb035-pre 601a9d2
//! # in each:
//! ./mvnw -q clean package -DskipTests -Pfat-jar
//!
//! printf 'msd_2\n0 0\n0 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035P.txt"
//! printf 'msd_2\n0 1\n0 -> 1\n\n1 2\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035P12.txt"
//! printf 'msd_2\n0 2\n0 -> 1\n\n1 3\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035P23.txt"
//! printf 'msd_2\n0 0\n0 -> 1\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n' > "Word Automata Library/WB035T.txt"
//! printf '{0, 1}\n\n0\n0 -> 0 / -1\n1 -> 0 / 1\n'    > "Transducer Library/WB035NEG.txt"
//! printf '{0, 1}\n\n0\n0 -> 0 / 5\n1 -> 0 / 1\n'     > "Transducer Library/WB035POS.txt"
//! printf '{1, 2}\n\n0\n1 -> 0 / 7\n2 -> 0 / 8\n'     > "Transducer Library/WB035SHIFT.txt"
//! printf '{0, 1, 2}\n\n0\n0 -> 0 / 4\n1 -> 0 / 0\n2 -> 0 / 8\n' > "Transducer Library/WB035COL.txt"
//! printf '{1, 2, 3}\n\n0\n1 -> 0 / 7\n2 -> 0 / 8\n3 -> 0 / 9\n' > "Transducer Library/WB035S123.txt"
//!
//! cat > "Command Files/wb035_capture.txt" <<'EOF'
//! transduce wb035neg WB035NEG WB035P;
//! transduce wb035pos WB035POS WB035P;
//! transduce wb035tot WB035NEG WB035T;
//! transduce wb035shift WB035SHIFT WB035P12;
//! transduce wb035col WB035COL WB035P12;
//! transduce wb035s123 WB035S123 WB035P23;
//! transduce wb035rs RUNSUM2 WB035P;
//! eval wb035alive "?msd_2 Ex x = 1";
//! EOF
//! java -cp target/Walnut-all.jar Main.Prover wb035_capture.txt \
//!     >stdout.txt 2>stderr.txt </dev/null
//! # results land in Session/<timestamp>/Word Automata Library/
//!
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb035     --force
//! git -C ~/dev/walnut-java worktree remove /tmp/walnut-java-wb035-pre --force
//! ```
//!
//! `java`/`mvnw` above actually ran under a JDK 17+ toolchain
//! (`/Users/nkohen/Library/Java/JavaVirtualMachines/openjdk-19.0.1`, selected explicitly
//! since the shell's default `java` resolves to a JDK 11 too old for this project's class
//! file version). `</dev/null` matters: without it the process runs the command file and
//! then blocks in the REPL.
//!
//! ## Captured output (2026-08-21) — the before/after, measured on both jars
//!
//! | command | pre-fix (`601a9d2`) | post-fix (`7f54eff`) |
//! |---|---|---|
//! | `wb035neg`   | `0 -1 / 0->1` · `1 1 / 0->1` — **`1 -> 0` deleted** | `0 -1 / 0->1` · `1 1 / 0->1, 1->0` |
//! | `wb035col`   | `0 0 / 0->1` · `1 8 / 0->1` — **`1 -> 0` deleted** | `0 0 / 0->1` · `1 8 / 0->1, 1->0` |
//! | `wb035shift` | *no file*; `java.lang.NullPointerException … at Automata.Transducer.createMap(Transducer.java:400)` on stderr | `0 7 / 0->1` · `1 8 / 0->1, 1->0` |
//! | `wb035s123`  | `0 1 / 0->1, 1->2` · `1 9 / 0->1` · `2 7 / 0->2, 1->2` — **three states, real and dead swapped** | `0 8 / 0->1` · `1 9 / 0->1, 1->0` |
//! | `wb035pos`   | identical on both (the one-value-different control) | ” |
//! | `wb035tot`   | identical on both (total input; branch never entered) | ” |
//! | `wb035rs`    | identical on both (`RUNSUM2`, the shipped safe-coincidence shape) | ” |
//!
//! The seven post-fix files are `../fixtures/wb035/*.txt`, byte-for-byte. `stderr.txt` was
//! empty for the fixed jar and carried exactly the NPE above for the pre-fix one.
//!
//! The command file, the nine hand-authored library files and both worktrees were removed
//! afterward, matching every recipe in `../CAPTURE.md`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wr_cli::prover::Prover;
use wr_cli::session::Session;
use wr_core::automaton::Automaton;
use wr_core::equiv::automaton_language_equivalent;
use wr_core::logging::Logging;

/// A shared, inspectable sink — same shape as `java_bugfix_wb038.rs`'s own `Capture`
/// (duplicated across this file family; see `java_bugfix_wb010.rs`'s matching note for why
/// no shared helper crate exists for these ~15-line structs).
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

/// A process-scoped Walnut home tree plus a `Prover` over it, with `console`/`err` standing
/// in for real stdout/stderr.
fn prover(tag: &str) -> (Prover, Capture, Capture, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "wr-differential-javabugfix-{tag}-{}",
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
        "Command Files",
        "Transducer Library",
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let dir_str = format!("{}/", dir.to_str().unwrap());
    let session = Session::new(Some(&dir_str), Some(&dir_str), false);
    let console = Capture::default();
    let err = Capture::default();
    let logging = Logging::with_writers(Box::new(console.clone()), Box::new(err.clone()));
    (
        Prover::with_output(session, logging, Box::new(console.clone())),
        console,
        err,
        dir,
    )
}

/// The `walnut-java` fixture the fixed jar wrote for `name`.
fn java_output(name: &str) -> Automaton {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/wb035")
        .join(format!("{name}.txt"));
    wr_io::reader::read_automaton_txt(path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("captured fixture {name}.txt must parse: {e}"))
}

/// Walks a deterministic word automaton and returns the output at the state reached by
/// `word`, or `None` if the walk falls off a missing transition. (`transduce`'s results are
/// always deterministic; a multi-destination entry would be a bug in its own right and is
/// asserted against rather than silently taking the first.)
fn word_output(a: &Automaton, word: &[i32]) -> Option<i32> {
    let mut state = a.fa.q0;
    for sym in word {
        let dests = a.fa.d[state].get(sym)?;
        assert_eq!(dests.len(), 1, "transduce results must be deterministic");
        state = dests[0];
    }
    Some(a.fa.o[state])
}

/// The DFAO comparison this module's docs describe: same language by
/// [`automaton_language_equivalent`], AND the same **output** at every input word up to
/// length 4 (`2^0 + … + 2^4 == 31` words over `{0, 1}`, which covers every state of every
/// automaton in this file several times over).
fn assert_same_dfao(ours: &Automaton, java: &Automaton, case: &str) {
    // `totalize(0)` on both first, exactly as `tests/golden`'s comparator does and for the
    // same reason: a Walnut `.txt` automaton need not be total (a missing transition means
    // implicit rejection) while the equivalence oracle requires total DFAs. Partial results
    // are precisely what this file is about, so this normalization is load-bearing here,
    // not incidental -- and it is language-preserving, so it cannot mask a divergence. The
    // per-word output comparison below runs on the ORIGINAL pair, where a missing
    // transition is still visible as `None`.
    let mut ours_total = ours.clone();
    let mut java_total = java.clone();
    ours_total.fa.totalize(0);
    java_total.fa.totalize(0);
    assert!(
        automaton_language_equivalent(&ours_total, &java_total)
            .unwrap_or_else(|e| panic!("{case}: {e:?}")),
        "{case}: the port's transduction is not language-equivalent to real fixed \
         walnut-java's.\nours:  o={:?} d={:?}\njava:  o={:?} d={:?}",
        ours.fa.o,
        ours.fa.d,
        java.fa.o,
        java.fa.d
    );
    for len in 0..=4u32 {
        for n in 0..(1u32 << len) {
            let word: Vec<i32> = (0..len).rev().map(|b| ((n >> b) & 1) as i32).collect();
            assert_eq!(
                word_output(ours, &word),
                word_output(java, &word),
                "{case}: output disagrees at word {word:?}\nours:  o={:?} d={:?}\n\
                 java:  o={:?} d={:?}",
                ours.fa.o,
                ours.fa.d,
                java.fa.o,
                java.fa.d
            );
        }
    }
}

/// `Word Automata Library/WB035P.txt` — a partial `msd_2` DFAO with outputs `{0, 1}` (so
/// the dead state's output is `min(M.O) - 1 == -1`). State `0` has no transition on symbol
/// `1`, and only that one.
const PARTIAL_01: &str = "msd_2\n0 0\n0 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n";
/// `PARTIAL_01` with its outputs shifted to `{1, 2}`, so the dead state's output is `0`.
const PARTIAL_12: &str = "msd_2\n0 1\n0 -> 1\n\n1 2\n0 -> 1\n1 -> 0\n";
/// `PARTIAL_01` with its outputs shifted to `{2, 3}`, so the dead state's output is `1`.
const PARTIAL_23: &str = "msd_2\n0 2\n0 -> 1\n\n1 3\n0 -> 1\n1 -> 0\n";
/// `PARTIAL_01` totalized (state `0` gains `1 -> 1`), so the branch is never entered.
const TOTAL_01: &str = "msd_2\n0 0\n0 -> 1\n1 -> 1\n\n1 1\n0 -> 1\n1 -> 0\n";

/// Runs one `transduce` through the real dispatch loop and returns what it wrote, having
/// first checked that the session stayed alive and printed nothing to stderr.
fn run_transduce(
    tag: &str,
    transducer_name: &str,
    transducer_txt: &str,
    word_name: &str,
    word_txt: &str,
    out_name: &str,
) -> Automaton {
    let (mut p, console, err, dir) = prover(tag);
    fs::write(
        dir.join("Transducer Library")
            .join(format!("{transducer_name}.txt")),
        transducer_txt,
    )
    .unwrap();
    fs::write(
        dir.join("Word Automata Library")
            .join(format!("{word_name}.txt")),
        word_txt,
    )
    .unwrap();

    let command = format!(
        "transduce {out_name} {transducer_name} {word_name};\nreg wb035after msd_2 \"1*\";\n"
    );
    let mut input = io::Cursor::new(command.into_bytes());
    p.read_buffer(&mut input, false);

    assert_eq!(
        err.text(),
        "",
        "the fixed jar writes nothing to stderr for this command; the port must not \
         either.\nconsole:\n{}",
        console.text()
    );
    let out_path = dir
        .join("Word Automata Library")
        .join(format!("{out_name}.txt"));
    assert!(
        out_path.is_file(),
        "the fixed jar writes {out_name}.txt for this command.\nconsole:\n{}",
        console.text()
    );
    // `Prover.readBuffer`'s `catch` -- the NEXT command still runs, i.e. nothing above
    // killed the session.
    assert!(
        dir.join("Automata Library/wb035after.txt").is_file(),
        "the session must survive: {}",
        console.text()
    );

    let ours = wr_io::reader::read_automaton_txt(out_path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("the port's own output must parse: {e}"));
    fs::remove_dir_all(&dir).ok();
    ours
}

/// **Half (2), the silent one.** `WB035NEG` emits `-1`, which is exactly `min(M.O) - 1` for
/// this input, so pre-fix `removeStatesWithOutputRebuild` deleted the real state carrying
/// it: real Walnut wrote `1 1 / 0 -> 1`, losing `PARTIAL_01`'s perfectly well-defined
/// `1 -> 0` transition, with no diagnostic anywhere. The fixed jar keeps it.
#[test]
fn wb035_a_transducer_output_colliding_with_the_marker_no_longer_deletes_real_states() {
    let ours = run_transduce(
        "wb035-neg",
        "WB035NEG",
        "{0, 1}\n\n0\n0 -> 0 / -1\n1 -> 0 / 1\n",
        "WB035P",
        PARTIAL_01,
        "wb035neg",
    );
    let java = java_output("wb035neg");
    assert_same_dfao(&ours, &java, "wb035neg");

    // The transition the bug used to eat, named explicitly so a regression reads as
    // "WB-035 is back" rather than as an opaque shape mismatch.
    assert_eq!(
        word_output(&ours, &[0, 1]),
        Some(-1),
        "PARTIAL_01 defines `1 -> 0` out of its output-1 state; pre-fix the marker \
         collision deleted the transition into it"
    );
    // ...and the input's one genuinely undefined transition stays undefined.
    assert_eq!(word_output(&ours, &[1]), None);
}

/// The control from WB-035's own entry: identical in every respect except the one colliding
/// output value (`-1` becomes `5`). This already produced the right answer pre-fix on both
/// engines, and must keep producing it — it is what makes the case above a *collision*
/// result rather than a partial-input result.
#[test]
fn wb035_the_one_value_different_control_is_unchanged() {
    let ours = run_transduce(
        "wb035-pos",
        "WB035POS",
        "{0, 1}\n\n0\n0 -> 0 / 5\n1 -> 0 / 1\n",
        "WB035P",
        PARTIAL_01,
        "wb035pos",
    );
    assert_same_dfao(&ours, &java_output("wb035pos"), "wb035pos");
    assert_eq!(word_output(&ours, &[0, 1]), Some(5));
}

/// **Half (2) again, with `min(M.O) == 1`** rather than `0` — so the marker is `0`, and the
/// `List.indexOf`-returns-`-1` coincidence that made half (1) harmless does not apply
/// either. `WB035COL` legitimately emits `0` on letter `1`; pre-fix that real state was
/// deleted.
#[test]
fn wb035_marker_collision_with_a_nonzero_minimum_output() {
    let ours = run_transduce(
        "wb035-col",
        "WB035COL",
        "{0, 1, 2}\n\n0\n0 -> 0 / 4\n1 -> 0 / 0\n2 -> 0 / 8\n",
        "WB035P12",
        PARTIAL_12,
        "wb035col",
    );
    assert_same_dfao(&ours, &java_output("wb035col"), "wb035col");
    assert_eq!(word_output(&ours, &[0, 1]), Some(0));
}

/// **Half (1), the crashing form.** `M`'s outputs are `{1, 2}` and `WB035SHIFT`'s input
/// alphabet is `{1, 2}`, so `transduceNonDeterministic`'s state-`0`-only compatibility
/// guard passes — and `min(M.O) - 1 == 0` used to be written into the transition table as a
/// raw encoded symbol (meaning *the letter `1`*) while `createMap` looked the dead state up
/// as `encode([0]) == -1`, which was never written. The pre-fix jar wrote no file and died
/// with `NullPointerException … at Automata.Transducer.createMap(Transducer.java:400)`; the
/// port answered with `TransduceError::NoTransducerTransition` at the same point. Both now
/// transduce.
#[test]
fn wb035_a_shifted_input_alphabet_transduces_instead_of_crashing() {
    let ours = run_transduce(
        "wb035-shift",
        "WB035SHIFT",
        "{1, 2}\n\n0\n1 -> 0 / 7\n2 -> 0 / 8\n",
        "WB035P12",
        PARTIAL_12,
        "wb035shift",
    );
    assert_same_dfao(&ours, &java_output("wb035shift"), "wb035shift");
    assert_eq!(word_output(&ours, &[]), Some(7));
    assert_eq!(word_output(&ours, &[0, 1]), Some(7));
}

/// **Half (1), the purely silent form** — and the shape that shows the bug was never only
/// about crashes. `M`'s outputs are `{2, 3}` and the transducer's alphabet is `{1, 2, 3}`,
/// so the dead state's output `1` is *also a valid encoded symbol* (the position of the
/// letter `2`). Pre-fix the self-loop overwrote letter `2`'s `sigma` entry while the dead
/// state's own lookup resolved to letter `1`'s real entry, and both engines produced a
/// three-state automaton with the real and dead states swapped — no exception at all.
#[test]
fn wb035_the_dead_letter_no_longer_clobbers_a_real_letter_of_the_transducer() {
    let ours = run_transduce(
        "wb035-s123",
        "WB035S123",
        "{1, 2, 3}\n\n0\n1 -> 0 / 7\n2 -> 0 / 8\n3 -> 0 / 9\n",
        "WB035P23",
        PARTIAL_23,
        "wb035s123",
    );
    assert_same_dfao(&ours, &java_output("wb035s123"), "wb035s123");
    assert_eq!(
        ours.fa.q, 2,
        "the pre-fix answer had a spurious third state"
    );
    assert_eq!(word_output(&ours, &[]), Some(8));
}

/// The overwhelmingly common case, and the fix's zero-regression half: a **total** input
/// automaton never enters the dead-state branch at all. Same `WB035NEG` transducer whose
/// `-1` output breaks the partial case above.
#[test]
fn wb035_a_total_input_automaton_is_unaffected() {
    let ours = run_transduce(
        "wb035-tot",
        "WB035NEG",
        "{0, 1}\n\n0\n0 -> 0 / -1\n1 -> 0 / 1\n",
        "WB035T",
        TOTAL_01,
        "wb035tot",
    );
    assert_same_dfao(&ours, &java_output("wb035tot"), "wb035tot");
    // The transition `PARTIAL_01` lacks, which this input has: it must survive here, where
    // the marker collision is not even in play.
    assert_eq!(word_output(&ours, &[1]), Some(1));
}

/// The other zero-regression half: a **multi-state** transducer over a partial input, using
/// the real shipped `RUNSUM2` — the shape Walnut's own corpus actually exercises (Walnut's
/// `Word Automata Library` really does reach this branch, on 32 of the 95 shipped
/// transducer/word-automaton combinations this port swept; see `docs/WALNUT-BUGS.md`
/// WB-035's corrected reachability bullet). Every one of those is in the safe-coincidence
/// case, so this must be byte-for-byte what both jars produce — and it also makes the
/// `for q in 0..T.fa.q` loop that installs the dead letter really iterate, which the
/// one-state cases above cannot.
#[test]
fn wb035_the_shipped_runsum2_shape_over_a_partial_input_is_unaffected() {
    let runsum2 = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/wr-io/tests/fixtures")
            .join("RUNSUM2.txt"),
    )
    .unwrap();
    let ours = run_transduce(
        "wb035-rs", "RUNSUM2", &runsum2, "WB035P", PARTIAL_01, "wb035rs",
    );
    assert_same_dfao(&ours, &java_output("wb035rs"), "wb035rs");
    assert_eq!(ours.fa.q, 8);
}
