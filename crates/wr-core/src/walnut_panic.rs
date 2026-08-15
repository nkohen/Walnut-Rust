// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! The **panic → catchable-error boundary**: Rust's stand-in for a Java `try { … } catch
//! (RuntimeException e)`.
//!
//! # Why this exists
//!
//! Several primitives in this crate replicate a Java `RuntimeException` (usually a
//! `WalnutException`) as a `panic!`/`assert!` — see e.g. [`crate::logicalops`]'s
//! `right_quotient` subset guard, [`crate::product`]'s `compute_same_inputs`/
//! `create_basic_automaton` guards. In Java those are **caught**, at two distinct depths:
//!
//! * `Prover.dispatch`'s handler prints them and the REPL keeps going, so one bad
//!   `rightquo`/`combine` costs you a command, not a session;
//! * `Commands/EvalDef.compute`'s `catch (RuntimeException e)` (`EvalDef.java:123-128`)
//!   catches one **inside** the postorder-execution loop and rethrows it with the offending
//!   token's position appended (`"\n\t: char at N"`) — so an `eval` whose cross-product hits
//!   a same-label/different-alphabet mismatch is a clean, positioned error message, not a
//!   crash.
//!
//! In Rust a panic has no `catch_unwind` boundary unless one is drawn, so the same input
//! **kills the process**. That is strictly less faithful than Java, not a ported quirk.
//!
//! Changing the guarding primitives' signatures to return `Result` is a wider cross-cutting
//! decision (they have other, infallibility-assuming callers), so the boundary is drawn at
//! each point Java itself wraps in a try/catch. This module holds the one shared mechanism;
//! it lives in `wr-core` (rather than in `wr-cli`, where Phase 3b's U23 first introduced it)
//! because `wr-logic`'s `eval::compute` needs the same boundary for `EvalDef.compute`'s inner
//! catch, and `wr-logic` depends on `wr-core`, not on `wr-cli`.
//!
//! The panic message is the Java exception message verbatim wherever the guard ported one;
//! where it is this workspace's own wording, the text necessarily differs — the *behavior*
//! (report and continue) is what is being matched.
//!
//! # Guard authoring rule
//!
//! A guard whose message is meant to survive this boundary must `panic!` with **exactly** the
//! Java message and nothing else. `assert!(cond, "msg")` is fine (its payload is just `msg`);
//! `assert_eq!(a, b, "msg")` is **not** — its payload is `assertion \`left == right\` failed:
//! msg\n  left: …\n right: …`, which would reach the user instead of Walnut's wording.
//!
//! # Console noise
//!
//! Rust's default panic hook writes `thread '…' panicked at …` to stderr before unwinding.
//! That has no Java analogue, so the hook is silenced **for the duration of this call on this
//! thread only** (a thread-local flag consulted by a wrapper hook installed once). Panics
//! anywhere else — including on other threads running concurrently, which matters for the test
//! harness — still print normally.

/// Runs `f`, converting a panic escaping it into `Err(panic message)`. See the module docs.
pub fn catch_walnut_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    install_quiet_hook();
    let result = SILENCE_PANIC.with(|s| {
        let previous = s.get();
        s.set(true);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        s.set(previous);
        r
    });
    result.map_err(|payload| panic_payload_message(payload.as_ref()))
}

/// The panic message, however it was raised: `panic!("literal")` yields a `&'static str`
/// payload, `panic!("{fmt}")`/`assert!(cond, "{fmt}")` a `String`. Anything else has no
/// message at all, which Java's `getMessage()` also models as `null`; the placeholder
/// matches what `Throwable.toString()` would show for a message-less exception.
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        String::new()
    }
}

thread_local! {
    static SILENCE_PANIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

static QUIET_HOOK: std::sync::Once = std::sync::Once::new();

/// Installs (once, process-wide) a panic hook that defers to whatever hook was in place
/// before, except on a thread that is currently inside [`catch_walnut_panic`].
fn install_quiet_hook() {
    QUIET_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if SILENCE_PANIC.with(|s| s.get()) {
                return;
            }
            previous(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_walnut_panic_passes_a_success_through_untouched() {
        assert_eq!(catch_walnut_panic(|| 6 * 7), Ok(42));
    }

    #[test]
    fn catch_walnut_panic_recovers_both_panic_payload_shapes() {
        // `panic!("literal")` / `assert!(cond, "literal")` -> `&'static str` payload.
        assert_eq!(
            catch_walnut_panic(|| panic!("Second A's alphabet must be a subset")),
            Err::<(), _>("Second A's alphabet must be a subset".to_string())
        );
        // A formatted message -> `String` payload.
        let digit = 7;
        assert_eq!(
            catch_walnut_panic(|| panic!("digit {digit} not in track 0's alphabet")),
            Err::<(), _>("digit 7 not in track 0's alphabet".to_string())
        );
    }

    /// The silencing flag must be scoped to the guarded call: a panic raised AFTER one
    /// completes still reaches the default hook (otherwise a genuine bug elsewhere in the
    /// process would be swallowed silently).
    #[test]
    fn catch_walnut_panic_restores_the_silence_flag_on_both_paths() {
        let _ = catch_walnut_panic(|| panic!("inner"));
        assert!(!SILENCE_PANIC.with(|s| s.get()));
        let _ = catch_walnut_panic(|| ());
        assert!(!SILENCE_PANIC.with(|s| s.get()));
    }

    /// Nesting must not clear the flag early: an inner guarded call restores the OUTER
    /// call's `true`, not an unconditional `false`. `eval::compute` nests one of these
    /// inside `wr-cli`'s dispatch-level guard on every single token, so this is a live
    /// shape, not a hypothetical.
    #[test]
    fn nested_calls_restore_the_outer_flag_not_a_default() {
        let observed_inside = catch_walnut_panic(|| {
            let inner = catch_walnut_panic(|| ());
            assert_eq!(inner, Ok(()));
            SILENCE_PANIC.with(|s| s.get())
        });
        assert_eq!(
            observed_inside,
            Ok(true),
            "after an inner guarded call returns, the outer guard's silencing must still be on"
        );
        assert!(!SILENCE_PANIC.with(|s| s.get()));
    }

    /// A message-less payload (`panic_any` with a non-string value) has no Java analogue
    /// message, and must not be mistaken for a successful run.
    #[test]
    fn a_message_less_payload_is_still_an_error_with_an_empty_message() {
        let r: Result<(), String> = catch_walnut_panic(|| std::panic::panic_any(7u32));
        assert_eq!(r, Err(String::new()));
    }
}
