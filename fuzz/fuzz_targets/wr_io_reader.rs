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
/// materializes the full digit set `0..k` (so `msd_99999999` is a 400 MB `Vec` before a
/// single transition is read), and `alphabet_size` is the **product** over all tracks
/// (so ~20 tracks of base 99 overflows `usize` — which, under `cargo-fuzz`'s
/// `-Cdebug-assertions`, is an arithmetic-overflow panic that says nothing about the
/// port's fidelity to Java, whose `int` silently wraps there instead).
///
/// Capping the header at 48 characters with digit runs of at most 2 bounds both: at most
/// ~7 tracks of base < 100, i.e. `alphabet_size < 100^7`, comfortably inside `usize`,
/// with every real fixture header (`msd_2`, `lsd_2 lsd_2 lsd_2 lsd_2`, `{0,1} {-1,0,1}`,
/// `msd_fib`) still accepted. Header *grammar* — malformed tokens, wrong separators,
/// bogus numeration names, the `{…}` literal-set form — is fully fuzzed; only the
/// magnitude is bounded.
const MAX_HEADER_LEN: usize = 48;
const MAX_HEADER_DIGIT_RUN: usize = 2;

/// `read_automaton_txt_impl`'s own blank/comment skip (`should_skip`), duplicated here
/// rather than exported from `wr-io`: the harness must decide *before* calling the
/// reader, and a budget filter is not something the shipped crate should carry.
fn is_skipped(line: &str) -> bool {
    let t = line.trim_start();
    t.is_empty() || t.starts_with('#')
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
    let _ = wr_io::reader::read_automaton_from_str(content);
    let _ = wr_io::reader::read_transducer_from_str(content);
});
