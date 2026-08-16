// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Tier-5 fuzz target: `wr-logic`'s FOL lexer/parser (`Predicate` — the `\G`-anchored
//! tokenizer plus the shunting-yard pass that produces the postfix token stream).
//!
//! `PORTING.md` flags that tokenizer as the highest hand-written-parser risk in the port:
//! Java drives 15 `\G`-anchored `Matcher` patterns by index, a construct the `regex`
//! crate cannot express at all, so the port re-expresses it with `regex-automata`'s
//! anchored `Input::span`. A mis-anchored pattern, an off-by-one in the UTF-16 position
//! threading, or an unguarded index into a multi-byte character is exactly the class of
//! defect a fuzzer finds and a fixture corpus does not.
//!
//! Crash-freedom only: any `Predicate` or any `LexError` is a pass.
//!
//! # Why this target needs a custom environment
//!
//! `Predicate::new` is not a pure parse — it takes a [`PredicateEnv`], and lexing a
//! `?msd_k` token *eagerly constructs that number system* (`NumberSystem::new` builds the
//! addition and comparison automata, whose alphabet is `k³`). An unconstrained fuzzer
//! finds `?msd_2000000000` within seconds and OOMs on construction cost that is not a
//! bug — real Walnut does the same thing, and `docs/WALNUT-BUGS.md` WB-032 already
//! documents `msd_1000` blowing up there. [`BoundedEnv`] caps the base so the target
//! genuinely fuzzes *lexing and parsing*, which is its purpose.

#![no_main]

use std::rc::Rc;

use libfuzzer_sys::fuzz_target;
use wr_core::automaton::Automaton;
use wr_core::numsys::{NumSysError, NumberSystem};
use wr_logic::predicate::Predicate;
use wr_logic::predicate_env::{InMemoryPredicateEnv, PredicateEnv, PredicateEnvError};

/// Ceiling on input size, matching the documented `-max_len=256` and enforced here so a
/// seed or a replayed crash artifact cannot bypass it. The longest real query string in
/// this repo's own test corpus is well under 100 characters; 256 leaves ample room for
/// pathological nesting without letting the fuzzer spend its budget on length alone.
const MAX_INPUT_LEN: usize = 256;

/// Largest numeration base the stub environment will build.
///
/// `NumberSystem::new` is `O(base³)` in alphabet size, so this is the difference between
/// a target that explores the tokenizer and one that spends every iteration allocating.
/// 8 keeps `msd_2`/`msd_3`/`msd_4` — every base this port's own corpora actually use —
/// plus a couple beyond, at a worst case of 512 symbols.
const MAX_BASE: u32 = 8;

/// Longest run of ASCII digits accepted — **a known-crash bypass, not a budget filter**,
/// kept explicit rather than silent, in the spirit of `tests/golden`'s
/// `KNOWN_DIVERGENCES` list, and **to be deleted once the underlying panic is guarded**.
///
/// The `@N` alphabet-letter token (`Predicate.java:220`) is read with
/// `wr_core::util::parse_int`, which *panics* on an `i32` overflow by design — its own
/// doc reasons that every call site is already behind a regex gate, so only an
/// internal-invariant violation could reach it. That reasoning does not hold here: the
/// digits come straight out of the user's query, and `?msd_2 T[x] = @8888888888` reaches
/// it. This is the same root cause as the `wr_core_regex` target's first finding, at a
/// second, independent call site; see `README.md`'s "Findings" and
/// `regressions/wr_logic_parser/f1b-parse-int-i32-overflow-in-alphabet-letter`.
///
/// 9 digits is the largest run that cannot overflow `i32` (`999999999 < 2147483647`), so
/// this excludes exactly the known-crashing shape. The cost is that numeric literals of
/// 10+ digits go unfuzzed — which is narrow, since ordinary constants take the
/// arbitrary-precision `parse_big_integer` path instead and are unaffected.
const MAX_DIGIT_RUN: usize = 9;

fn digit_runs_are_in_budget(s: &str) -> bool {
    let mut run = 0usize;
    for c in s.chars() {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        if run > MAX_DIGIT_RUN {
            return false;
        }
    }
    true
}

/// A file-I/O-free [`PredicateEnv`] with a bounded numeration base.
///
/// Delegates wholesale to [`InMemoryPredicateEnv`], which already builds plain
/// `msd_k`/`lsd_k` systems on demand and already performs no file I/O (a custom base like
/// `msd_fib` comes back as `NumSysError::NotDefined` rather than reaching the
/// filesystem — see `wr_core::numsys`'s module docs). The only thing added here is the
/// base ceiling.
struct BoundedEnv {
    inner: InMemoryPredicateEnv,
}

impl BoundedEnv {
    fn new() -> BoundedEnv {
        // One word and one function so `T[i]` and `$f(x)` lex past the library lookup
        // into the parts this target is actually about — arity validation, the bracketed
        // argument sub-`Predicate`, and the postfix push. Without them every such token
        // dead-ends at `FileDoesNotExist` and a whole branch of the tokenizer goes
        // unfuzzed. Both are parsed from literal text through `wr-io` rather than
        // constructed by hand, so they are unambiguously well-formed inputs.
        //
        // Thue-Morse, the canonical Walnut `Word Automata Library/T.txt`: arity 1, DFAO
        // outputs 0/1.
        const T: &str = "msd_2\n0 0\n0 -> 0\n1 -> 1\n1 1\n0 -> 1\n1 -> 0\n";
        // An arity-1 predicate automaton ("x is even" over msd_2), for `$f(x)`.
        const F: &str = "msd_2\n0 1\n0 -> 0\n1 -> 1\n1 0\n0 -> 0\n1 -> 1\n";

        let env = InMemoryPredicateEnv::new()
            .with_word("T", parse(T))
            .with_function("f", parse(F))
            // Macro text is a plain `String` in the environment, so this costs nothing
            // and opens the `#name(args)` re-lexing path. Deliberately non-recursive:
            // the text names no macro, so `#m(#m(x,y),z)` nests only as deep as the
            // (bounded) input, and the target cannot diverge on a self-referential macro
            // the environment invented rather than the fuzzer.
            .with_macro("m", "%0 = %1");
        BoundedEnv { inner: env }
    }
}

/// Panics on failure by design: these are two fixed, committed literals, so a failure is
/// a broken harness, not a fuzz finding, and must be loud rather than silently degrading
/// the target's coverage.
fn parse(text: &str) -> Automaton {
    wr_io::reader::read_automaton_from_str(text).expect("the harness's own fixture must parse")
}

/// `msd_8` -> `Some(8)`; `msd_fib` / a malformed name -> `None` (nothing to bound —
/// delegation will produce the ordinary error).
fn declared_base(name: &str) -> Option<u32> {
    name.rsplit('_').next()?.parse::<u32>().ok()
}

impl PredicateEnv for BoundedEnv {
    fn number_system(&self, name: &str) -> Result<Rc<NumberSystem>, PredicateEnvError> {
        if declared_base(name).is_some_and(|b| b > MAX_BASE) {
            // Shaped as the same error an undefined base produces, so the tokenizer sees
            // an outcome it already has a code path for — this filter must not introduce
            // a control-flow shape the real environment cannot produce.
            return Err(PredicateEnvError::NumberSystem {
                name: name.to_string(),
                source: NumSysError::NotDefined(name.to_string()),
            });
        }
        self.inner.number_system(name)
    }

    fn word(&self, name: &str) -> Result<Automaton, PredicateEnvError> {
        self.inner.word(name)
    }

    fn function(&self, name: &str) -> Result<Automaton, PredicateEnvError> {
        self.inner.function(name)
    }

    fn macro_text(&self, name: &str) -> Result<String, PredicateEnvError> {
        self.inner.macro_text(name)
    }
}

thread_local! {
    /// Built once per process, not per iteration: `InMemoryPredicateEnv` memoizes number
    /// systems (Java's `computeIfAbsent`), so sharing it amortizes the `O(base³)`
    /// construction across the whole run. The memo is a pure cache — a hit and a miss
    /// return the same value — so this does not make the target non-reproducible.
    static ENV: BoundedEnv = BoundedEnv::new();
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_LEN {
        return;
    }
    // A query reaches `Predicate` as a `String` off the command line / a `.txt` command
    // file, so non-UTF-8 bytes are not a reachable input shape. (Interesting *encoding*
    // cases — multi-byte characters, lone-looking surrogate escapes, combining marks —
    // are still very much in scope: they survive `from_utf8` and are exactly what the
    // UTF-16-offset threading can get wrong.)
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };
    if !digit_runs_are_in_budget(query) {
        return;
    }

    ENV.with(|env| {
        let _ = Predicate::new(env, query);
    });
});
