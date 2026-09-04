// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
// Copyright (C) 2026 Nadav Kohen. New code, not ported from Walnut.

//! The shell-out binary's resource budget, read from the environment — the analog of
//! the `-Xmx8192m` a consumer passes to the JVM (`docs/CT-RESEARCH-INTEGRATION.md`).
//!
//! | Variable | Meaning |
//! | --- | --- |
//! | `WR_MAX_STATES` | cap on the state count of any single automaton under construction |
//! | `WR_MAX_BYTES` | cap on live heap bytes (the `walnut-rs` binary installs the tracking allocator this needs); accepts a `K`/`M`/`G` suffix (powers of 1024, case-insensitive) |
//!
//! Unset = unlimited, i.e. the pre-existing behavior. A malformed value is a startup
//! error, never silently ignored — this is a safety knob.

use std::fmt;

use wr_core::resource::ResourceBudget;

pub const MAX_STATES_VAR: &str = "WR_MAX_STATES";
pub const MAX_BYTES_VAR: &str = "WR_MAX_BYTES";

/// A budget variable that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetEnvError {
    pub variable: &'static str,
    pub value: String,
}

impl fmt::Display for BudgetEnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={:?} is not a valid limit (expected a non-negative integer{})",
            self.variable,
            self.value,
            if self.variable == MAX_BYTES_VAR {
                ", optionally with a K/M/G suffix"
            } else {
                ""
            }
        )
    }
}

impl std::error::Error for BudgetEnvError {}

/// Read the budget from the process environment.
pub fn budget_from_env() -> Result<ResourceBudget, BudgetEnvError> {
    budget_from(
        std::env::var(MAX_STATES_VAR).ok().as_deref(),
        std::env::var(MAX_BYTES_VAR).ok().as_deref(),
    )
}

/// [`budget_from_env`] on explicit values (the testable core).
pub fn budget_from(
    max_states: Option<&str>,
    max_bytes: Option<&str>,
) -> Result<ResourceBudget, BudgetEnvError> {
    let max_states = match max_states.map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => Some(s.parse::<usize>().map_err(|_| BudgetEnvError {
            variable: MAX_STATES_VAR,
            value: s.to_string(),
        })?),
    };
    let max_bytes = match max_bytes.map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => Some(parse_bytes(s).ok_or_else(|| BudgetEnvError {
            variable: MAX_BYTES_VAR,
            value: s.to_string(),
        })?),
    };
    Ok(ResourceBudget {
        max_states,
        max_bytes,
    })
}

fn parse_bytes(s: &str) -> Option<usize> {
    let (digits, multiplier): (&str, usize) = match s.chars().last()? {
        'k' | 'K' => (&s[..s.len() - 1], 1 << 10),
        'm' | 'M' => (&s[..s.len() - 1], 1 << 20),
        'g' | 'G' => (&s[..s.len() - 1], 1 << 30),
        c if c.is_ascii_digit() => (s, 1),
        _ => return None,
    };
    let n: usize = digits.parse().ok()?;
    n.checked_mul(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_or_empty_means_unlimited() {
        assert_eq!(budget_from(None, None), Ok(ResourceBudget::UNLIMITED));
        assert_eq!(
            budget_from(Some(""), Some(" ")),
            Ok(ResourceBudget::UNLIMITED)
        );
    }

    #[test]
    fn parses_plain_integers_and_byte_suffixes() {
        assert_eq!(
            budget_from(Some("1000000"), Some("2G")),
            Ok(ResourceBudget {
                max_states: Some(1_000_000),
                max_bytes: Some(2 << 30),
            })
        );
        assert_eq!(parse_bytes("512m"), Some(512 << 20));
        assert_eq!(parse_bytes("8k"), Some(8 << 10));
        assert_eq!(parse_bytes("123"), Some(123));
    }

    #[test]
    fn rejects_garbage_loudly_naming_the_variable() {
        let e = budget_from(Some("lots"), None).unwrap_err();
        assert_eq!(e.variable, MAX_STATES_VAR);
        assert!(e.to_string().contains("WR_MAX_STATES"), "{e}");
        let e = budget_from(None, Some("2T")).unwrap_err();
        assert_eq!(e.variable, MAX_BYTES_VAR);
        assert!(e.to_string().contains("K/M/G"), "{e}");
        assert!(budget_from(Some("-1"), None).is_err());
        assert!(budget_from(None, Some("G")).is_err());
        // A suffix that overflows `usize` is an error, not a wrapped value.
        assert!(budget_from(None, Some("99999999999999999999G")).is_err());
    }
}
