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
//!
//! Suppressing that line would otherwise throw away the ONE diagnostic Rust gives for free
//! and Java's message does not carry: the panic's source position. So the same hook records
//! it, and [`catch_walnut_panic_detailed`] hands it back in [`CaughtPanic::location`] — the
//! port's counterpart of the single frame `Logging.printTruncatedStackTrace` prints for a
//! non-`WalnutException`. Without it, drawing this boundary would have made a genuine new
//! port bug HARDER to diagnose than it was before the boundary existed.

/// Runs `f`, converting a panic escaping it into `Err(panic message)`. See the module docs.
///
/// Use [`catch_walnut_panic_detailed`] when the caller needs to *triage* the recovered
/// panic — inspect where it came from, or re-raise one it does not recognize.
pub fn catch_walnut_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_walnut_panic_detailed(f).map_err(|caught| caught.message)
}

/// A panic [`catch_walnut_panic_detailed`] recovered, with everything Java's
/// `catch (RuntimeException e)` would have had in hand.
pub struct CaughtPanic {
    /// The panic message — what [`catch_walnut_panic`] returns. Per this module's
    /// guard-authoring rule, this is the Java exception's message verbatim wherever the
    /// guard ported one.
    pub message: String,
    /// `file:line:column` of the `panic!`, from `PanicHookInfo::location()`.
    ///
    /// This has no Java analogue in the *message*, but it is the port's nearest
    /// equivalent of the one stack frame `Logging.printTruncatedStackTrace` prints for a
    /// non-`WalnutException` — i.e. for the case that is a genuine bug rather than a
    /// deliberately-thrown error. Without it, drawing this boundary would have made a new
    /// port bug strictly HARDER to diagnose than before the boundary existed, since the
    /// default hook's `panicked at file:line` line is suppressed here.
    ///
    /// `None` only if the platform gave the hook no location.
    pub location: Option<String>,
    /// Kept so an unrecognized panic can be re-raised unchanged; see [`Self::resume`].
    payload: Box<dyn std::any::Any + Send>,
}

impl CaughtPanic {
    /// Re-raises this panic exactly as it was, for a caller whose triage concluded it is
    /// NOT one of the ported guards it meant to absorb.
    ///
    /// The fuzz targets are the motivating caller: a target that blanket-catches around a
    /// step known to raise one specific ported exception would also silently swallow a
    /// genuinely new bug reached through the same input, reporting nothing.
    pub fn resume(self) -> ! {
        std::panic::resume_unwind(self.payload)
    }

    /// Whether `message` is one of the JDK exception texts this port raises on purpose,
    /// i.e. `IndexOutOfBoundsException`/`ArrayIndexOutOfBoundsException`'s
    /// `Index <i> out of bounds for length <n>`.
    ///
    /// Deliberately a shape test, not a list of exact strings: the index and the length
    /// vary with the input, and both `Automaton::decode` and
    /// `crate::product::cross_product_internal` render the same shape.
    pub fn is_ported_jdk_index_error(&self) -> bool {
        let Some(rest) = self.message.strip_prefix("Index ") else {
            return false;
        };
        let Some((index, length)) = rest.split_once(" out of bounds for length ") else {
            return false;
        };
        index.parse::<i64>().is_ok() && length.parse::<u64>().is_ok()
    }
}

impl std::fmt::Debug for CaughtPanic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaughtPanic")
            .field("message", &self.message)
            .field("location", &self.location)
            .finish_non_exhaustive()
    }
}

/// [`catch_walnut_panic`], keeping the panic's location and payload — see [`CaughtPanic`].
pub fn catch_walnut_panic_detailed<T>(f: impl FnOnce() -> T) -> Result<T, CaughtPanic> {
    install_quiet_hook();
    let result = SILENCE_PANIC.with(|s| {
        let previous = s.get();
        s.set(true);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        s.set(previous);
        r
    });
    result.map_err(|payload| CaughtPanic {
        message: panic_payload_message(payload.as_ref()),
        // Recorded by the quiet hook, which ran immediately before the unwind reached
        // this `catch_unwind`. Taken (not cloned), so a later recovered panic whose hook
        // somehow did not run cannot inherit this one's location.
        location: LAST_PANIC_LOCATION.with(|l| l.borrow_mut().take()),
        payload,
    })
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
    /// Where the most recent silenced panic on this thread was raised. Written by the
    /// quiet hook (the only place `PanicHookInfo::location()` is available at all — a
    /// `catch_unwind` payload carries the message, never the site) and taken by
    /// [`catch_walnut_panic_detailed`].
    static LAST_PANIC_LOCATION: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

static QUIET_HOOK: std::sync::Once = std::sync::Once::new();

/// Installs (once, process-wide) a panic hook that defers to whatever hook was in place
/// before, except on a thread that is currently inside [`catch_walnut_panic`].
fn install_quiet_hook() {
    QUIET_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if SILENCE_PANIC.with(|s| s.get()) {
                // Silenced, but not forgotten: record the site so the recovery path can
                // still say WHERE, which is the only diagnostic the default hook's
                // suppressed `panicked at file:line` line was carrying.
                let location = info.location().map(std::string::ToString::to_string);
                LAST_PANIC_LOCATION.with(|l| *l.borrow_mut() = location);
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

    /// The location is what makes a genuine NEW port bug caught here no less diagnosable
    /// than it was before the boundary existed (the default hook's `panicked at
    /// file:line` line is suppressed on this path).
    #[test]
    fn the_detailed_form_reports_where_the_panic_was_raised() {
        let caught = catch_walnut_panic_detailed(|| panic!("boom")).unwrap_err();
        assert_eq!(caught.message, "boom");
        let location = caught
            .location
            .expect("a location on every supported platform");
        assert!(
            location.contains("walnut_panic.rs"),
            "expected this file, got {location}"
        );
        // Not a stale one from an EARLIER recovered panic: a second, different site
        // reports a different line.
        let other = catch_walnut_panic_detailed(|| panic!("boom again")).unwrap_err();
        assert_ne!(
            other.location.expect("a location"),
            location,
            "each recovered panic must report its own site"
        );
    }

    /// The location must not leak from one recovered panic into the NEXT recovery, which
    /// would silently mislabel a real finding's site. `take()`, not `clone()`.
    #[test]
    fn the_location_is_not_inherited_by_a_later_recovery() {
        let _ = catch_walnut_panic_detailed(|| panic!("first"));
        // A payload raised with the hook bypassed entirely (`resume_unwind` does not run
        // hooks), so nothing records a location for it.
        let caught = catch_walnut_panic_detailed(|| std::panic::resume_unwind(Box::new("second")))
            .unwrap_err();
        assert_eq!(caught.message, "second");
        assert_eq!(caught.location, None, "must not inherit `first`'s site");
    }

    /// `resume()` re-raises the ORIGINAL payload, so a caller that decides a recovered
    /// panic is not one it meant to absorb can hand it back unchanged.
    #[test]
    fn resume_re_raises_the_original_payload() {
        let outer =
            catch_walnut_panic(
                || match catch_walnut_panic_detailed(|| panic!("not mine")) {
                    Ok(()) => unreachable!(),
                    Err(caught) => caught.resume(),
                },
            );
        assert_eq!(outer, Err("not mine".to_string()));
    }

    /// The shape test the fuzz targets triage on. It must accept the real messages both
    /// ported guards render — including a NEGATIVE index, which is the whole WB-038 case
    /// — and reject anything else, however similar.
    #[test]
    fn only_the_jdk_index_error_shape_counts_as_a_ported_guard() {
        let caught = |m: &'static str| catch_walnut_panic_detailed(|| panic!("{}", m)).unwrap_err();
        for message in [
            "Index -1 out of bounds for length 2",
            "Index -2 out of bounds for length 4",
            "Index 4 out of bounds for length 4",
            "Index 0 out of bounds for length 0",
        ] {
            assert!(caught(message).is_ported_jdk_index_error(), "{message}");
        }
        for message in [
            "",
            "Index out of bounds for length 2",
            "Index x out of bounds for length 2",
            "Index 1 out of bounds for length -2",
            "index -1 out of bounds for length 2",
            "Second A's alphabet must be a subset",
            "begin 0, end -1, length 4",
            "Index -1 out of bounds for length 2 (and then some)",
        ] {
            assert!(!caught(message).is_ported_jdk_index_error(), "{message:?}");
        }
    }
}
