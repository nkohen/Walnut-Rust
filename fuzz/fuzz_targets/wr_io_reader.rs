// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! Tier-5 fuzz target: `wr-io`'s Walnut `.txt` reader (`AutomatonReader` +
//! `ParseMethods`) — DESIGN.md §5's named "seeded with malformed-input corpus" target.
//!
//! The property under test is **crash-freedom only**: no panic, no OOM, no hang. Every
//! outcome the reader can legitimately produce — an `Automaton`, or any `ReadError` — is
//! a pass. Semantic correctness is Tier 1/Tier 3's job (`tests/golden`,
//! `tests/differential-gen`), not this target's; asserting anything about the parse
//! result here would just re-test what those tiers already cover, against a far worse
//! oracle.
//!
//! Both entry points are driven from the same input, since the two grammars share
//! `parse_header` and differ only in the body (`0 -> 1` vs `0 -> 1 / 0`), so one corpus
//! genuinely exercises both.
//!
//! # Why the target does not stop at the read (review round 2)
//!
//! It used to. That is why 6M+ clean executions missed both of the defects the adversarial
//! review of the first fix round found: a successfully-read automaton can carry a
//! transition under the bogus key `-1` (Java's `List.indexOf` semantics, WB-038,
//! faithfully reproduced by `Automaton::encode_index_of`), and **nothing the reader itself
//! does ever touches that key again** — the panics live one step downstream, in whatever
//! consumes the automaton. So each successful read is now followed by a small, bounded
//! set of downstream steps that a real command would perform on a freshly loaded library
//! automaton:
//!
//! * `write_txt`/`write_gv` into a `Vec<u8>` — `AutomatonWriter`'s two `decode` call
//!   sites, the ones that used to fabricate a digit tuple for the `-1` key and write out
//!   a wrong automaton (silently — strictly worse than a crash);
//! * `determinize_and_minimize` — the load-time path for a nondeterministic file, and the
//!   shared prefix of nearly every command;
//! * a `sort_label` + self-`cross_product` pair — `product.rs`'s raw `[sym as usize]`
//!   indexing, which is what `union`/`intersect`/`join` reach.
//!
//! Everything here still tests **crash-freedom only**, and every step stays inside the
//! same "generate SMALL" budget as the read itself (see [`MAX_INPUT_LEN`]) — the state
//! cap below is what keeps the two superexponential steps from turning a fuzz run into a
//! cost measurement.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// Ceiling on input size, matching the documented `-max_len=512` in `README.md` and
/// enforced here too so a committed seed or a replayed crash artifact cannot bypass it.
///
/// 512 bytes is not arbitrary: 111 of the 113 real `.txt` fixtures in
/// `tests/differential/fixtures/` are under it, so it costs essentially no seed
/// coverage — and it is what keeps the reader's **auto-determinize-on-load** step
/// (`read_automaton_txt_impl`'s `subset_construction` call for a nondeterministic file)
/// inside a sane budget. Subset construction is exponential in the state count by
/// construction; a multi-kilobyte hand-crafted NFA would let libFuzzer "find" a
/// worst-case blowup that is textbook algorithmic cost, not a defect, and stall the run
/// on a non-finding. CLAUDE.md's "generate SMALL" guardrail, applied to a fuzzer.
const MAX_INPUT_LEN: usize = 512;

/// Longest header line accepted, and the longest run of ASCII digits allowed inside it.
///
/// Same "cost, not bug" filter as [`MAX_INPUT_LEN`], aimed at the one place where a few
/// input bytes buy unbounded work: the alphabet declaration. A `msd_k` track
/// materializes the full digit set `0..k`, so `msd_99999999` is a 400 MB `Vec` before a
/// single transition is read — pure allocation cost, saying nothing about the port.
///
/// Capping the header at 48 characters with digit runs of at most 2 bounds it: at most
/// ~7 tracks of base < 100, with every real fixture header (`msd_2`,
/// `lsd_2 lsd_2 lsd_2 lsd_2`, `{0,1} {-1,0,1}`, `msd_fib`) still accepted. Header
/// *grammar* — malformed tokens, wrong separators, bogus numeration names, the `{…}`
/// literal-set form — is fully fuzzed; only the magnitude is bounded.
///
/// This cap used also to be the only thing keeping the reader's `alphabet_size`
/// **product** out of arithmetic-overflow territory (a 64-track `{0,1}` header, ~380
/// bytes, overflowed it) — an incidental screen that hid a real defect rather than a
/// deliberate one. The reader now folds that product at Java `int` width and returns
/// `ReadError::AlphabetSizeOverflow`, matching `RichAlphabet.determineAlphabetSize`'s
/// `Math.multiplyExact` throw, so overflow is an ordinary in-scope outcome here and no
/// longer something this filter has to (or does) prevent.
const MAX_HEADER_LEN: usize = 48;
const MAX_HEADER_DIGIT_RUN: usize = 2;

/// `read_automaton_txt_impl`'s own blank/comment skip (`should_skip`), needed *before*
/// calling the reader so this file can size up the header it is about to hand over.
///
/// Delegates to the very functions `should_skip` itself delegates to — `wr-io` exports
/// both — rather than re-deriving the rule. The hand-rolled `trim_start().starts_with('#')`
/// that used to live here was a second, subtly different grammar: `str::trim_start` is
/// Unicode-aware where Java's `\s` is ASCII-only, and it had none of the line-terminator
/// handling that `PATTERN_COMMENT`'s `.`-exclusion set requires. It only ever chose which
/// line to *measure*, so it could not corrupt a verdict — but "one grammar, not two" is
/// exactly the invariant this reader's last several review rounds were about, and a
/// budget filter reading a different header line than the reader does is a way to
/// silently under-fuzz.
fn is_skipped(line: &str) -> bool {
    wr_io::parse_methods::is_whitespace_line(line) || wr_io::parse_methods::is_comment_line(line)
}

fn header_is_in_budget(content: &str) -> bool {
    let Some(header) = content.lines().find(|l| !is_skipped(l)) else {
        // No header at all — `ReadError::EmptyFile`, cheap, and worth exercising.
        return true;
    };
    let header = header.trim();
    if header.chars().count() > MAX_HEADER_LEN {
        return false;
    }
    let mut run = 0usize;
    for c in header.chars() {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        if run > MAX_HEADER_DIGIT_RUN {
            return false;
        }
    }
    true
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_LEN {
        return;
    }
    // Walnut reads its `.txt` files as text; a non-UTF-8 byte string is not an input the
    // reader is ever handed (`std::fs::read_to_string` would have rejected it upstream
    // with the same `ReadError::Io` for every such input, so fuzzing past this point
    // would explore exactly one uninteresting outcome).
    let Ok(content) = std::str::from_utf8(data) else {
        return;
    };
    if !header_is_in_budget(content) {
        return;
    }

    // No custom-base resolver: resolving one means real file I/O, which a fuzz target
    // must not do. A custom-base header still reaches `parse_header` and comes back as
    // `ReadError::UnsupportedNumeration` — the header parse itself is what is under test.
    let read = wr_io::reader::read_automaton_from_str(content);
    let _ = wr_io::reader::read_transducer_from_str(content);

    if let Ok(automaton) = read {
        // A file with an out-of-alphabet body digit loads with a transition stored under
        // an INVALID key (Java's `List.indexOf` -> `-1`, WB-038, faithfully ported). Every
        // `wr-core` primitive then raises the same unchecked exception real Walnut raises
        // on it — by design, and reported at `wr_cli::prover`'s `Prover::caught` boundary
        // exactly as `Prover.readBuffer`'s `catch (RuntimeException)` reports Java's. That
        // is a *ported behavior*, not a crash-freedom violation, so those inputs run the
        // downstream steps behind the same boundary the shipped binary has (and would
        // otherwise re-report the known class within seconds, discovering nothing else).
        //
        // But ONLY that one class. A blanket catch here would absorb any panic reached
        // through a corrupt-key input, so a genuinely different bug hiding behind one
        // would be silently discarded and never reported — the exact failure mode this
        // target exists to prevent. So the recovered panic is triaged, and anything whose
        // message is not the ported JDK `Index <i> out of bounds for length <n>` shape is
        // re-raised unchanged (`CaughtPanic::resume`), surfacing as a real finding with
        // its original payload — preceded by the recovered site, since the boundary
        // suppressed Rust's own `panicked at file:line` line on the way through.
        //
        // A well-formed automaton gets the steps RAW: there, ANY panic is a real finding.
        if has_invalid_transition_key(&automaton) {
            if let Err(caught) = wr_core::walnut_panic::catch_walnut_panic_detailed(|| {
                exercise_downstream(automaton)
            }) {
                if !caught.is_ported_jdk_index_error() {
                    eprintln!(
                        "unexpected panic behind a corrupt transition key: {:?} at {}",
                        caught.message,
                        caught.location.as_deref().unwrap_or("<unknown site>")
                    );
                    caught.resume();
                }
            }
        } else {
            exercise_downstream(automaton);
        }
    }
});

/// Whether any transition key is outside `0..alphabet_size` — i.e. whether this automaton
/// carries WB-038's bogus key. See the call site for why that changes how it is exercised.
fn has_invalid_transition_key(automaton: &wr_core::automaton::Automaton) -> bool {
    let size = automaton.fa.alphabet_size;
    automaton
        .fa
        .d
        .iter()
        .flat_map(|row| row.keys())
        .any(|&sym| sym < 0 || sym as usize >= size)
}

/// Peak state count a fuzzed automaton may reach before the downstream steps are skipped.
///
/// `determinize_and_minimize` (subset construction) and `cross_product` are both
/// worst-case exponential *by construction*; a fuzzer that finds a blow-up has found
/// textbook algorithmic cost, not a defect, and every second spent on it is a second not
/// spent on reachable panics. The read itself is already bounded by [`MAX_INPUT_LEN`] and
/// [`MAX_HEADER_LEN`], so this only has to bound the product of the two steps below.
const MAX_STATES_FOR_DOWNSTREAM: usize = 24;

/// Ceiling on `alphabet_size` before the **cross-product** step is skipped — a budget
/// filter, exactly like [`MAX_HEADER_LEN`], and needed for the same reason at a different
/// exponent.
///
/// `create_basic_automaton` builds the product alphabet, whose size is the **product** of
/// the two operands' — so crossing an automaton with itself is quadratic in
/// `alphabet_size`, and the header budget (which bounds each base below 100 and the track
/// count around 7) is not tight enough on its own: the committed-seed-sized header
/// `msd_92 msd_4 msd_55` is `alphabet_size = 20_240`, i.e. a 410-million-entry product
/// alphabet, which libFuzzer reports as a 33-second timeout. Textbook algorithmic cost,
/// identical in real Walnut, and it would stall every run at seed-replay time.
///
/// 64 keeps the product at <= 4_096 symbols, which comfortably covers every real fixture
/// (base-2/3/4 automata of 1-4 tracks) while making the step effectively free.
const MAX_ALPHABET_SIZE_FOR_PRODUCT: usize = 64;

/// The downstream steps described in this module's docs, on a successfully-read automaton.
///
/// Deliberately *not* driven through `wr-cli`'s `Prover`: that layer catches panics on
/// purpose (`Prover::caught`, the port of `Prover.readBuffer`'s `catch (RuntimeException)`),
/// and a target that ran everything behind it could no longer tell a recovered guard from a
/// genuine defect. These call the `wr-core`/`wr-io` primitives directly; the caller decides
/// whether a given input is allowed to reach a ported guard (see `has_invalid_transition_key`).
fn exercise_downstream(mut automaton: wr_core::automaton::Automaton) {
    if automaton.fa.q > MAX_STATES_FOR_DOWNSTREAM {
        return;
    }
    // The two `AutomatonWriter` paths, into memory (a fuzz target does no file I/O).
    let mut txt = Vec::new();
    let _ = wr_io::writer::write_txt(&mut automaton.clone(), &mut txt);
    let mut gv = Vec::new();
    let _ = wr_io::writer::write_gv(&mut automaton.clone(), &mut gv, "fuzz", false);

    // `union`/`intersect`/`join`'s shared prefix: a bound automaton crossed with itself.
    // `sort_label` is what those commands run first, and is itself an `encode`/`decode`
    // round trip over the whole alphabet.
    //
    // The TRUE/FALSE automaton (a file whose whole body is `true`/`false` — 13% of the
    // Tier-1 corpus) is excluded from THIS step only, not from the writer/determinize
    // ones above and below. `cross_product`'s first line is a deliberately-ported Java
    // guard (`ProductStrategies`' "the automata for this method cannot be true or false
    // automata", a `WalnutException`), and every real caller checks
    // `isTRUE_FALSE_AUTOMATON` before reaching it — so calling it here would be the
    // harness violating a documented precondition, not a finding. (It is also, precisely,
    // the kind of skip that must be justified rather than quietly added: see this file's
    // "known-crash bypass" rules in `README.md`. This is not one of those — the guard is
    // correct and the input is not a bug.)
    if !automaton.fa.is_true_false_automaton()
        && automaton.fa.alphabet_size <= MAX_ALPHABET_SIZE_FOR_PRODUCT
    {
        let mut crossed = automaton.clone();
        crossed.sort_label();
        let op = wr_core::product::BooleanOp::And;
        let _ = wr_core::product::cross_product(&crossed, &crossed, |a, b| op.combine(a, b));
    }

    // The load-time determinize path (and nearly every command's closing step).
    automaton.determinize_and_minimize();
}
