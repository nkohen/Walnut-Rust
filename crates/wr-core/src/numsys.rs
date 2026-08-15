// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! `numsys` — base-k numeration systems (msd/lsd only).
//!
//! Ports the base-k paths of Walnut's `Automata/NumberSystem`. Lives INSIDE
//! `wr-core` (not a separate crate) on purpose: in Walnut, `Automaton` holds a
//! `List<NumberSystem>` field (19 refs) and `NumberSystem` references `Automaton`
//! 121 times and constructs it — genuine bidirectional coupling that a crate
//! boundary cannot express (adversarial-review kit-finding #1). The adder,
//! comparator, and constant automata over base-k that the FOL decider composes
//! all live here, alongside the automaton types they produce.
//!
//! DROPPED: negative bases (DESIGN.md §3, `docs/BOUNDARY-MAP.md` §4.1), and the bespoke
//! Ostrowski / Fibonacci / Pell *algorithms*. **NOT dropped, as of Phase 3a's U5: the
//! generic `Custom Bases/*.txt` mechanism**, which is how every one of those numerations
//! (`msd_fib`, `msd_pell`, `msd_trib`, `msd_tib`, `msd_ns`, …) is actually configured in
//! Walnut — there is no Fibonacci-specific code path in `NumberSystem.java` at all, only a
//! file loader. See [`NumberSystem::with_custom_base_files`]. (`CLAUDE.md`'s
//! "DROP: Ostrowski/Fibonacci/Pell" line predates that distinction and needs the same
//! correction the Phase 3 plan's gap #4 already records.)
//!
//! # U7 scope — what is here, and what is deliberately not
//!
//! Phase 1's spike left only [`less_than_msd`] and (from U6) [`determine_msd`]. U7
//! adds the rest of the base-*k* machinery: [`NumberSystem`] itself
//! (`NumberSystem(String name)`, `NumberSystem.java:132-163`), the programmatically
//! built adder/comparator/equality automata, the `getConstant`/`multiplication`/
//! `division` dynamic-programming families, and the `comparison`/`arithmetic`
//! dispatchers.
//!
//! ## DROPPED: negative bases (`isNeg`), decided in `docs/BOUNDARY-MAP.md` §4.1
//!
//! Java's `NumberSystem` interleaves positive- and negative-base logic through an
//! `isNeg` field. Per the recorded user decision, negative-base code is **deleted
//! outright**, not stubbed: `baseNegNAddition` (`:503-533`), `baseNegNLessThan`
//! (`:541-561`), `baseNBaseChange` (`:568-601`), `setBaseChangeAutomaton`
//! (`:443-468`), `determineNegativeNS` (`:219-230`) and the `baseChange` field are
//! all absent here. Three consequences worth naming explicitly, because they are
//! *simplifications of surviving methods*, not whole-method deletions:
//!
//! 1. `validateNeg` (`:1026-1028`) is `if (!isNeg && n.signum() < 0) throw`. With
//!    `isNeg` gone it is exactly "reject negative constants" — kept, as
//!    [`NumberSystem::validate_non_negative`].
//! 2. **Every `n.signum() < 0` branch that sits AFTER a `validateNeg` call is
//!    therefore unreachable in a positive base and is deleted**: `comparison`'s
//!    `:701-702`, `arithmetic`'s `:809-813`/`:861-864`/`:910-913`, `constant`'s
//!    `:944-951`, `multiplication`'s `:986-994`, and `division`'s `n < 0` operand
//!    selections at `:1047-1048`. Each deletion is called out again at its own
//!    porting site below.
//! 3. `setBaseChangeAutomaton`'s `isNeg == false` arms were *already* found dead by
//!    Phase 0 characterization work (`docs/WALNUT-BUGS.md`'s dead-code section;
//!    `NumberSystemTest.testBaseChangeOnAPositiveNumberSystemCannotCompare` can only
//!    reach them by reflection). Its only production caller is `determineNegativeNS`,
//!    whose own javadoc says "Currently used ONLY in split command" — and `split` is
//!    DROP (`docs/BOUNDARY-MAP.md` §6). So the whole base-change surface is dropped
//!    for two independent reasons, not just the negative-base one.
//!
//! ## U5 (Phase 3a): file-backed custom bases, I/O-free
//!
//! U7 deferred this whole surface; U5 lands it. Java's constructor tries
//! `loadAutomatonOrNull` (`:299-319`) FIRST for each of the addition / less-than /
//! all-representations automata, and only falls back to programmatic construction when no
//! file exists. **The ordinary `msd_k`/`lsd_k` case never actually loads a file** —
//! verified by listing `walnut-java/Custom Bases/`: it ships `msd_fib`, `msd_kim`,
//! `msd_nara`, `msd_neg_fib`, `msd_ns`, `msd_pell`, `msd_pisot4`, `msd_tib`, `msd_trib`
//! (+ their `_addition`/`_less_than`/`_base_change` companions) and **no `msd_<digits>`
//! file at all**. So the file path is reached only for a genuinely custom base name, or
//! for a user deliberately *overriding* a standard base.
//!
//! `wr-core` still performs no file I/O: the two `File.isFile()` probes and the two
//! `new Automaton(address)` reads stay in `wr-io`/`wr-cli` (matching `wr_io::reader`'s
//! existing "takes a path, doesn't reach into `Session`" shape). What lives here is the
//! *decision logic* — [`CustomBaseCandidates::resolve`] (Java's precedence and
//! complement-with-reverse fallback) and [`custom_base_candidate_names`] (the naming
//! convention, which is `NumberSystem.java`'s, not `Session`'s). [`NumberSystem::new`]
//! passes no files and so behaves exactly as it did pre-U5, including
//! `NumberSystem::new("msd_fib")` returning [`NumSysError::NotDefined`];
//! [`NumberSystem::with_custom_base_files`] is the full constructor.
//!
//! Three knock-on effects, all of which invalidate a claim this port previously recorded:
//! * `flagUseAllRepresentations` (`:147-150`) is no longer always `false`, so
//!   [`NumberSystem::use_all_representations`] is a real field read and
//!   `Automaton.applyAllRepresentations`'s five call sites (three in
//!   `AutomatonLogicalOps`, three here at `:153-155`) are live. Both
//!   `crate::logicalops`'s and `crate::product`'s module docs said the opposite and are
//!   corrected; `crate::automaton::Automaton::apply_all_representations` is the port.
//! * The addition/less-than *validation* checks (`:342-362`, `:383-393`) were ported as
//!   `assert!`s on the grounds that only a FILE-LOADED automaton could fail them. Now one
//!   can, on plausible user input, so they are `Err`s
//!   ([`NumSysError::AdditionInputCount`] and friends). `NumberSystemTest`'s six
//!   file-backed validation tests finally have analogs here.
//! * `msd_neg_*` no longer fails by accident (`"neg_fib"` not being `\d+`) — it names a
//!   real, shipped, file-backed base. It is now rejected on purpose and by name:
//!   [`NumSysError::UnsupportedNegativeBase`].
//!
//! ## The `lsd` half of the composed constructions (deferred by U7, delivered in 3b/L1)
//!
//! `msd_k` and `lsd_k` *both* build their adder / comparator / equality / `0` / `1`
//! automata here (the lsd direction is `AutomatonLogicalOps.reverse` of the msd one,
//! exactly as Java does it at `:333`/`:378`). But every construction that composes
//! them through ∃-elimination — `getConstant(n)` for `n >= 2`, `comparison`/
//! `arithmetic` against a constant, `multiplication`, `division` — routes through
//! [`crate::quantify::quantify`], whose lsd branch was, when U7 landed, still a hard
//! `UnsupportedLsdFixup` error (a Phase-2 scope cut in `quantify` itself, not here).
//! Every one of those constructions therefore failed on an `lsd_k` system, which is
//! most of what an `lsd_k` query *is* — U7 removed only the other half of that
//! blocker ("no lsd numeration system exists in `crate::numsys` to exercise it against
//! yet either").
//!
//! Phase 3b's L1 wired the fixup up (see [`crate::quantify`]'s "The lsd fixup"
//! section), so the whole composed family works on `lsd_k` now.
//! `lsd_composed_constructions_compute_the_right_language` and
//! `msd_and_lsd_composed_constructions_agree_after_reversal` in this file's test
//! module are the direct coverage.
//!
//! ## Scoped-down operator enums
//!
//! Java's public `comparison`/`arithmetic` signatures take
//! `Main.EvalComputations.Token.{RelationalOperator,ArithmeticOperator}.Ops`, whose
//! enclosing classes drag in the whole `Token`/`Expression` AST. Following the
//! precedent `product.rs` set with `BooleanOp`, this module defines small LOCAL
//! [`RelationalOp`]/[`ArithmeticOp`] enums covering exactly the operation kinds the
//! automaton constructions dispatch on — not a port of those `Token` classes.
//!
//! # Tier-4 property targets (DESIGN.md §5), all present in this file's test module
//!
//! * `addition_automaton_computes_real_addition` — the adder automaton computes real
//!   addition (the biconditional, in both directions).
//! * `comparison_automata_agree_with_the_integer_order` — all six relations agree with
//!   the integer order on the same inputs, which is what "the comparator is a total
//!   order" cashes out to here (a single `<` test cannot distinguish a swapped or
//!   wrongly-negated dispatch arm).
//! * `msd_and_lsd_agree_after_reversal` — plus the deterministic
//!   `lsd_adder_and_comparator_read_least_significant_digit_first`, since a
//!   randomly-generated `(x, y, z)` almost never satisfies `x + y == z` and a
//!   reject-everything mutant would otherwise slip through (this was found by
//!   mutation-testing, not assumed).
//! * `get_constant_accepts_exactly_that_value`,
//!   `multiplication_automaton_computes_real_multiplication`,
//!   `division_matches_truncating_integer_division_over_a_small_table` — the same
//!   biconditional discipline for the three composed families.
//! * `msd_and_lsd_composed_constructions_agree_after_reversal` (Phase 3b, L1) — the
//!   composed-construction analogue of `msd_and_lsd_agree_after_reversal`, i.e. the
//!   same reversal correspondence for the families that route through
//!   [`crate::quantify::quantify`] and therefore through its lsd fixup.

use crate::automaton::Automaton;
use crate::fa::Fa;
use crate::logicalops::{and, not, reverse};
use crate::quantify::{quantify, QuantifyError};
use num_bigint::{BigInt, Sign};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

/// Ports `NumberSystem.determineMsd(List<NumberSystem>)` (`NumberSystem.java:197-209`,
/// package-private `static Boolean`): `None` ("skip the zero fixup") if any track is
/// non-arithmetic (Java: `ns == null`) or if the arithmetic tracks disagree on
/// direction; otherwise the shared direction.
///
/// `msd: &[Option<bool>]` is this crate's stand-in for Java's per-track
/// `List<NumberSystem>` (see [`crate::automaton::Automaton`]'s struct doc comment on
/// `msd`), so `None` plays the `null` role and `Some(b)` plays `ns.isMsd() == b`.
///
/// Java's loop leaves `isMsd = true` untouched for an *empty* list, so zero tracks
/// defaults to msd. This IS reachable through [`crate::quantify::quantify`] — but NOT
/// by quantifying away every track: that path turns the automaton into a TRUE/FALSE
/// automaton and `quantify` returns at its `isTRUE_FALSE_AUTOMATON` guard before this
/// function is ever reached (U0; it used to be a hard `AllTracksQuantified` error, and
/// the conclusion is unchanged either way). What *does* reach it is quantifying on an
/// automaton that already has zero tracks (and so no labels either, and no TRUE/FALSE
/// flag): the `a.label.is_empty()` early return leaves `a.msd` empty and unchanged, and
/// `quantify` still consults this function afterward (a faithfully-ported quirk — see
/// `quantify`'s module docs).
///
/// Lives here rather than next to `quantify` because it is a `NumberSystem` method in
/// Java, and because `NumberSystem`'s own base-*k* constructions are the other consumer
/// once they land.
pub fn determine_msd(msd: &[Option<bool>]) -> Option<bool> {
    let mut is_msd = true;
    let mut seen_any = false;
    for entry in msd {
        let v = (*entry)?;
        if seen_any && v != is_msd {
            return None;
        }
        is_msd = v;
        seen_any = true;
    }
    Some(is_msd)
}

/// Builds the 2-state lexicographic-less-than automaton over base `base`, msd-first
/// (`NumberSystem.lexicographicLessThan`, called with `isMsd = true`; the lsd
/// direction is `AutomatonLogicalOps.reverse` of this — not built here, see module
/// docs). Two tracks, labeled `"a"`/`"b"` by default (relabel via `automaton.label`
/// before use — e.g. the Phase-1 spike relabels to `["i", "x"]`); accepts iff the
/// first track's value is lexicographically less than the second's, reading digits
/// most-significant-first.
///
/// State 0 (initial, non-accepting): "equal so far" — self-loops on any digit pair
/// `(i, i)`, moves to state 1 on `(i, j)` with `i < j`, and has NO transition on
/// `(i, j)` with `i > j` (once a digit proves `a > b`, the predicate can never
/// become true — a missing transition, not a totalized sink; this automaton is
/// deliberately partial, matching Java exactly).
/// State 1 (accepting, "already decided a < b"): self-loops on every digit pair.
///
/// # Relationship to [`NumberSystem::less_than`]
///
/// This is a standalone, label-free convenience constructor kept from the Phase-1
/// spike (its callers — `wr_logic::quantify`'s tests and `tests/differential` — predate
/// [`NumberSystem`]). U7's [`lexicographic_less_than`] is the real port of
/// `NumberSystem.lexicographicLessThan` (`:417-433`), which sorts its alphabet first
/// and is what `NumberSystem::new` installs; for a contiguous `0..base` alphabet the
/// two build the same language, and
/// `spike_less_than_msd_agrees_with_the_ported_lexicographic_less_than` below pins that
/// against the language-equivalence oracle for bases 2..=5.
pub fn less_than_msd(base: i32) -> Automaton {
    assert!(base >= 2, "less_than_msd requires base >= 2");
    let digits: Vec<i32> = (0..base).collect();
    let alphabet_size = (base * base) as usize;

    let mut automaton = Automaton::new(
        Fa {
            true_false: None,
            q0: 0,
            q: 2,
            alphabet_size,
            o: vec![0, 1],
            d: vec![BTreeMap::new(), BTreeMap::new()],
        },
        vec![digits.clone(), digits.clone()],
        vec!["a".to_string(), "b".to_string()],
        vec![Some(true), Some(true)],
    );

    for &i in &digits {
        for &j in &digits {
            let sym = automaton.encode(&[i, j]);
            match i.cmp(&j) {
                std::cmp::Ordering::Equal => {
                    automaton.fa.d[0].entry(sym).or_default().push(0);
                }
                std::cmp::Ordering::Less => {
                    automaton.fa.d[0].entry(sym).or_default().push(1);
                }
                std::cmp::Ordering::Greater => {}
            }
            automaton.fa.d[1].entry(sym).or_default().push(1);
        }
    }

    automaton
}

// ---------------------------------------------------------------------------
// Name constants (`NumberSystem.java:71-76`)
// ---------------------------------------------------------------------------

/// `NumberSystem.MSD` (`NumberSystem.java:71`).
pub const MSD: &str = "msd";
/// `NumberSystem.MSD_UNDERSCORE` (`:72`).
pub const MSD_UNDERSCORE: &str = "msd_";
/// `NumberSystem.MSD_2` (`:73`).
pub const MSD_2: &str = "msd_2";
/// `NumberSystem.LSD` (`:74`).
pub const LSD: &str = "lsd";
/// `NumberSystem.LSD_UNDERSCORE` (`:75`).
pub const LSD_UNDERSCORE: &str = "lsd_";
/// `NumberSystem.UNDERSCORE_NEG_UNDERSCORE` (`:78`). Java uses it to compute `isNeg`
/// (`:137`, with the source comment "fix: `msd_neg_fib`... but not `msd_renege`" —
/// the leading underscore is what stops `msd_renege` matching). This port uses it to
/// *reject* the whole negative-base family: see [`NumSysError::UnsupportedNegativeBase`].
/// `NEG_UNDERSCORE` (`:77`) itself is not ported — its only other use is building the
/// negative-base name in the dropped `determineNegativeNS`.
pub const UNDERSCORE_NEG_UNDERSCORE: &str = "_neg_";

/// `Prover.TXT_EXTENSION` — the extension of the "set of all representations" file
/// (`NumberSystem.java:147`, which passes `Prover.TXT_EXTENSION` as the extension).
pub const TXT_EXTENSION: &str = ".txt";
/// `NumberSystem.UNDERSCORE_ADDITION_AUTOMATON` (`:80`).
pub const UNDERSCORE_ADDITION_AUTOMATON: &str = "_addition.txt";
/// `NumberSystem.UNDERSCORE_LESS_THAN_AUTOMATON` (`:84`).
pub const UNDERSCORE_LESS_THAN_AUTOMATON: &str = "_less_than.txt";

// `NumberSystem.UNDERSCORE_BASE_CHANGE_AUTOMATON` (`:82`) is NOT ported: the whole
// base-change surface is dropped for two independent reasons (see module docs).

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every `WalnutException` (and one unchecked Java exception) reachable from the
/// ported surface, as a real error enum rather than a stringly-typed throw
/// (`PORTING.md`'s type/error mapping table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumSysError {
    /// `determineMsdOrLsd` (`:268-270`) does `name.substring(0, name.indexOf("_"))`,
    /// which throws `StringIndexOutOfBoundsException` when the name contains no `_`.
    /// `NumberSystemTest.testBogusNS` asserts exactly that ("There's no guard for this;
    /// it throws a real exception"), so it is surfaced here as an error rather than a
    /// panic.
    MalformedName(String),
    /// `"Number system " + name + " is not defined."` (`:330`). Reached when the base
    /// is not `\d+`, or is `<= 1`, and no custom-base file rescues it (this port has no
    /// file loading, so also for every genuinely custom base — see module docs).
    NotDefined(String),
    /// `"Base of automaton's number system must be > 1 and int, found: " + baseStr`
    /// (`:240`), from `parseBase` (`:237-243`).
    InvalidBase(String),
    /// The base parses as `\d+` but overflows Java's `int`. Java's
    /// `Integer.parseInt(base)` (`:325`, `:242`) throws an unchecked
    /// `NumberFormatException` here rather than a `WalnutException`; this variant is
    /// this port's stand-in for that (a named divergence, on an input — e.g.
    /// `"msd_99999999999"` — that no realistic query produces).
    BaseNotAnI32(String),
    /// `WalnutException.negativeConstant` — `"negative constant " + n` (`WalnutException.java:91`),
    /// thrown by `validateNeg` (`:1026-1028`).
    NegativeConstant(String),
    /// `WalnutException.operatorTwoVariables` — `"the operator " + op + " cannot be
    /// applied to two variables"` (`WalnutException.java:112`), from `arithmetic`
    /// (`:764`, `:904`).
    OperatorTwoVariables(&'static str),
    /// `"unexpected arithmetic operator:" + op` (`:766`) — the `default:` arm of
    /// `arithmetic(String,String,String,Ops)`, reachable only with
    /// [`ArithmeticOp::UnaryNegative`] (pinned by `NumberSystemTest.testArithmetic`).
    UnexpectedArithmeticOperator(&'static str),
    /// `WalnutException.unexpectedOperator` — `"Unexpected operator:" + op`
    /// (`WalnutException.java:136-138`), from `ArithmeticOperator.arith`'s own
    /// `default:` arm (`ArithmeticOperator.java:256`), reachable only with
    /// [`ArithmeticOp::UnaryNegative`]. **Not the same text as
    /// [`Self::UnexpectedArithmeticOperator`]** — that variant is a different Java
    /// throw site (`NumberSystem.arithmetic`'s own `default:`, lowercase "unexpected
    /// arithmetic operator:"), confirmed by reading both; do not conflate them.
    UnexpectedOperator(&'static str),
    /// This port's declared stand-in for `ArithmeticOperator.arith(Ops, int, int)`'s
    /// `BigInteger.intValueExact()` narrowing step (`ArithmeticOperator.java:237`)
    /// overflowing — an uncaught, unchecked `ArithmeticException` in real Walnut (no
    /// `WalnutException` text of its own), same pattern as [`Self::BaseNotAnI32`].
    /// Reachable only through [`ArithmeticOp::arith`], the `int`-level helper
    /// `WordAutomaton`'s per-state DFAO arithmetic uses.
    ArithmeticIntOverflow(String),
    /// `"constants cannot be divided by variables"` (`:857`).
    ConstantDividedByVariable,
    /// `WalnutException.divisionByZero` — `"division by zero"` (`WalnutException.java:41`),
    /// from `division` (`:1036`).
    DivisionByZero,
    /// `"multiplication(0)"` (`:978`).
    MultiplicationByZero,
    /// Propagated from [`crate::quantify::quantify`]. Both of that enum's remaining
    /// variants are unreachable from the call sites here —
    /// [`QuantifyError::NotFreeVariable`] because each of them quantifies away a fresh
    /// name it has just bound itself, and [`QuantifyError::Minimize`] for the reasons
    /// that variant's own docs give — so this is a "can't happen" surfaced as an `Err`
    /// per `PORTING.md`'s error-mapping rule rather than a live failure mode. It was
    /// NOT one before Phase 3b's L1: until then `quantify`'s lsd branch was a hard
    /// `UnsupportedLsdFixup` error and this variant was how every composed construction
    /// failed on an `lsd_k` system (see module docs).
    Quantify(QuantifyError),
    /// **A deliberate divergence from Java, decided in Phase 3a's U5**, not a ported
    /// `WalnutException`: any name containing `_neg_` (`msd_neg_fib`, `lsd_neg_3`, …) is
    /// rejected here at name-resolution time.
    ///
    /// Java supports negative bases through `isNeg` (`NumberSystem.java:137`) plus
    /// `baseNegNAddition`/`baseNegNLessThan`/`setBaseChangeAutomaton`, and ships
    /// `Custom Bases/msd_neg_fib*.txt`. Negative-base numeration is DROPPED from this
    /// port's scope (`docs/BOUNDARY-MAP.md` §4.1, decided in Phase 2's U7, which deleted
    /// that code outright rather than stubbing it).
    ///
    /// Before U5 the deletion produced the right answer for the wrong reason: `msd_neg_3`
    /// happened to fail with [`NumSysError::NotDefined`] because `"neg_3"` isn't `\d+`.
    /// U5's custom-base loading breaks that accident — `msd_neg_fib` names a real,
    /// shipped, file-backed base, so a caller that hands over the loaded
    /// `msd_neg_fib_addition.txt`/`msd_neg_fib_less_than.txt` would otherwise get a
    /// *silently wrong* number system: the files are the negative-base adder/comparator,
    /// but every construction layered on them here
    /// ([`NumberSystem::validate_non_negative`] and the deleted `n.signum() < 0`
    /// branches) assumes a positive base. So this variant makes the deferral explicit and
    /// loud, per `docs/DESIGN.md`'s "deferred features fail cleanly" principle.
    ///
    /// The check runs BEFORE any file is consulted (mirroring Java's own line order:
    /// `isNeg` at `:137`, `setAdditionAutomaton` at `:142`), so it can never be masked by
    /// a missing-file error and never causes a pointless load attempt.
    UnsupportedNegativeBase(String),
    /// `"The addition automaton must have exactly 3 inputs: base " + name` (`:342-345`).
    ///
    /// This and the five variants below were ported as `assert!`s in U7, on the grounds
    /// that only a FILE-LOADED automaton could fail them and this port had no file
    /// loading. U5 gave it file loading, so they are reachable on plausible user input and
    /// become real errors — Java throws a `WalnutException` at each, which
    /// `PORTING.md`'s error table maps to a `Result`, not a panic.
    AdditionInputCount(String),
    /// `"The input alphabet of addition automaton must contain 0: base " + name` (`:347-350`).
    AdditionAlphabetMissingZero(String),
    /// `"The input alphabet of addition automaton must contain 1: base " + name` (`:352-355`).
    AdditionAlphabetMissingOne(String),
    /// `"All 3 inputs of the addition automaton must have the same alphabet: base " + name`
    /// (`:357-362`).
    AdditionAlphabetsDiffer(String),
    /// `UNDERSCORE_LESS_THAN_AUTOMATON + " must have exactly 2 inputs: base " + name`
    /// (`:383-385`).
    LessThanInputCount(String),
    /// `"Inputs of " + UNDERSCORE_LESS_THAN_AUTOMATON + " must have the same alphabet as
    /// the alphabet of inputs of " + UNDERSCORE_ADDITION_AUTOMATON + " : base " + name`
    /// (`:388-393`).
    LessThanAlphabetMismatch(String),
}

impl From<QuantifyError> for NumSysError {
    fn from(e: QuantifyError) -> Self {
        NumSysError::Quantify(e)
    }
}

/// The verbatim `WalnutException` message text for each variant, so Tier 1's `error*`
/// fixtures compare real Walnut wording rather than a `{:?}` dump.
///
/// Assigned to this unit by `wr_logic::predicate_env`'s U1 notes ("pending `NumSysError`'s
/// `Display`, U5"). Three variants have no Java message of their own and say so inline:
/// [`Self::MalformedName`] (Java throws a bare `StringIndexOutOfBoundsException`),
/// [`Self::BaseNotAnI32`] (a bare `NumberFormatException`), and
/// [`Self::UnsupportedNegativeBase`] (this port's own declared divergence).
impl fmt::Display for NumSysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // No Java message: `name.substring(0, -1)` throws
            // `StringIndexOutOfBoundsException` with the JDK's own text (`:268-270`).
            NumSysError::MalformedName(name) => {
                write!(f, "Number system name must contain '_', found: {name}")
            }
            NumSysError::NotDefined(name) => write!(f, "Number system {name} is not defined."),
            NumSysError::InvalidBase(base) => write!(
                f,
                "Base of automaton's number system must be > 1 and int, found: {base}"
            ),
            // No Java message: `Integer.parseInt` throws `NumberFormatException`.
            NumSysError::BaseNotAnI32(base) => write!(
                f,
                "Base of automaton's number system must fit in an int, found: {base}"
            ),
            NumSysError::NegativeConstant(n) => write!(f, "negative constant {n}"),
            NumSysError::OperatorTwoVariables(op) => {
                write!(f, "the operator {op} cannot be applied to two variables")
            }
            NumSysError::UnexpectedArithmeticOperator(op) => {
                write!(f, "unexpected arithmetic operator:{op}")
            }
            NumSysError::UnexpectedOperator(op) => write!(f, "Unexpected operator:{op}"),
            // No Java message: `BigInteger.intValueExact()` throws an unchecked
            // `ArithmeticException` with no `WalnutException` wrapper.
            NumSysError::ArithmeticIntOverflow(msg) => {
                write!(
                    f,
                    "arithmetic result does not fit in a 32-bit output ({msg})"
                )
            }
            NumSysError::ConstantDividedByVariable => {
                write!(f, "constants cannot be divided by variables")
            }
            NumSysError::DivisionByZero => write!(f, "division by zero"),
            NumSysError::MultiplicationByZero => write!(f, "multiplication(0)"),
            NumSysError::Quantify(e) => write!(f, "{e:?}"),
            // This port's own text: Java has no such error (it supports negative bases).
            NumSysError::UnsupportedNegativeBase(name) => write!(
                f,
                "Number system {name} is not supported: negative bases are out of scope."
            ),
            NumSysError::AdditionInputCount(name) => write!(
                f,
                "The addition automaton must have exactly 3 inputs: base {name}"
            ),
            NumSysError::AdditionAlphabetMissingZero(name) => write!(
                f,
                "The input alphabet of addition automaton must contain 0: base {name}"
            ),
            NumSysError::AdditionAlphabetMissingOne(name) => write!(
                f,
                "The input alphabet of addition automaton must contain 1: base {name}"
            ),
            NumSysError::AdditionAlphabetsDiffer(name) => write!(
                f,
                "All 3 inputs of the addition automaton must have the same alphabet: base {name}"
            ),
            NumSysError::LessThanInputCount(name) => write!(
                f,
                "{UNDERSCORE_LESS_THAN_AUTOMATON} must have exactly 2 inputs: base {name}"
            ),
            NumSysError::LessThanAlphabetMismatch(name) => write!(
                f,
                "Inputs of {UNDERSCORE_LESS_THAN_AUTOMATON} must have the same alphabet as \
                 the alphabet of inputs of {UNDERSCORE_ADDITION_AUTOMATON} : base {name}"
            ),
        }
    }
}

impl std::error::Error for NumSysError {}

// ---------------------------------------------------------------------------
// Locally-scoped operation-kind enums (see module docs)
// ---------------------------------------------------------------------------

/// The six relations `NumberSystem.comparison` dispatches on
/// (`Main.EvalComputations.Token.RelationalOperator.Ops`, `RelationalOperator.java:45-51`).
/// A local enum, not a port of that `Token` class — see module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationalOp {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessEqThan,
    GreaterEqThan,
}

impl RelationalOp {
    /// `RelationalOperator.reverseOperator` (`RelationalOperator.java:213-221`) — the
    /// relation with its operands swapped.
    pub fn reverse_operator(self) -> RelationalOp {
        match self {
            RelationalOp::Equal => RelationalOp::Equal,
            RelationalOp::NotEqual => RelationalOp::NotEqual,
            RelationalOp::LessThan => RelationalOp::GreaterThan,
            RelationalOp::GreaterThan => RelationalOp::LessThan,
            RelationalOp::LessEqThan => RelationalOp::GreaterEqThan,
            RelationalOp::GreaterEqThan => RelationalOp::LessEqThan,
        }
    }

    /// `RelationalOperator.compare(Ops, int, int)` / `compare(Ops, BigInteger,
    /// BigInteger)` (`RelationalOperator.java:179-193`) — a plain, `NumberSystem`-
    /// independent integer comparison (unlike [`NumberSystem::comparison`], which
    /// builds an *automaton* recognizing the relation). Added in Phase 3a's U6 for
    /// `WordAutomaton`'s per-state DFAO output comparisons, its sole caller in this
    /// port's current scope (`compareWordAutomaton`/`compareWordAutomata`, via
    /// `ProductStrategies.determineOutput`'s `RELATIONAL_OPERATORS` branch,
    /// `ProductStrategies.java:172-175`).
    ///
    /// Java's `int` overload widens both operands to `BigInteger` and compares via
    /// `compareTo`; comparison can't overflow, so a single `i32`-native implementation
    /// is exact for every value either overload can reach — no narrowing step needed
    /// (contrast [`ArithmeticOp::arith`], where narrowing back to `i32` is the whole
    /// reason for [`NumSysError::ArithmeticIntOverflow`] existing).
    pub fn compare(self, a: i32, b: i32) -> bool {
        self.holds_for(a.cmp(&b))
    }

    /// `RelationalOperator.compare(Ops, BigInteger, BigInteger)`
    /// (`RelationalOperator.java:183-193`) — the **primary** Java overload, of which
    /// [`Self::compare`] is the `int` specialization (Java's `compare(Ops, int, int)`
    /// literally widens both operands and calls this one, `:179-181`).
    ///
    /// Added in Phase 3a's U9, whose caller is `RelationalOperator.act`'s
    /// constant-folding branch (`RelationalOperator.java:92-95`): there the operands come
    /// from `getConstantValue`, which returns a `NumberLiteralExpression`'s **unbounded**
    /// `BigInteger` value, so narrowing to `i32` first would be a real behavior change
    /// (`123456789012345678901234567890 > 5` must still fold to `true`, not overflow).
    ///
    /// Both overloads route through the same private `holds_for`, so the two can
    /// never drift apart — the pairing is pinned by
    /// `relational_op_compare_int_and_big_int_agree_on_every_op`.
    pub fn compare_big_int(self, a: &BigInt, b: &BigInt) -> bool {
        self.holds_for(a.cmp(b))
    }

    /// The single, shared decision table behind both [`Self::compare`] overloads —
    /// Java's `switch (op)` over `a.compareTo(b)`'s sign (`:185-192`), expressed once
    /// over [`Ordering`] instead of twice over two numeric types.
    fn holds_for(self, ordering: Ordering) -> bool {
        match self {
            RelationalOp::Equal => ordering == Ordering::Equal,
            RelationalOp::NotEqual => ordering != Ordering::Equal,
            RelationalOp::LessThan => ordering == Ordering::Less,
            RelationalOp::GreaterThan => ordering == Ordering::Greater,
            RelationalOp::LessEqThan => ordering != Ordering::Greater,
            RelationalOp::GreaterEqThan => ordering != Ordering::Less,
        }
    }
}

/// The arithmetic operations `NumberSystem.arithmetic` dispatches on
/// (`Main.EvalComputations.Token.ArithmeticOperator.Ops`, `ArithmeticOperator.java:47-52`).
/// [`ArithmeticOp::UnaryNegative`] is included because it is the ONLY value that
/// reaches `arithmetic`'s `default:` throw (`:765-766`) — dropping it would delete a
/// live, tested branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticOp {
    Plus,
    Minus,
    Div,
    Mult,
    UnaryNegative,
}

impl ArithmeticOp {
    /// `ArithmeticOperator.Ops.getSymbol()` — the operator's source-syntax spelling,
    /// used only in error messages here.
    pub fn symbol(self) -> &'static str {
        match self {
            ArithmeticOp::Plus => "+",
            ArithmeticOp::Minus => "-",
            ArithmeticOp::Div => "/",
            ArithmeticOp::Mult => "*",
            ArithmeticOp::UnaryNegative => "_",
        }
    }

    /// `ArithmeticOperator.arith(Ops, BigInteger, BigInteger)`
    /// (`ArithmeticOperator.java:240-258`), narrowed to `i32` exactly as the `arith(Ops,
    /// int, int)` overload (`:236-238`) does — the only form `WordAutomaton`'s per-state
    /// DFAO arithmetic calls (`applyWordArithOperator`/`applyWordOperator`, via
    /// `ProductStrategies.determineOutput`'s `ARITHMETIC_OPERATORS` branch,
    /// `ProductStrategies.java:176-178`). Added in Phase 3a's U6.
    ///
    /// Java's two-step design — compute in `BigInteger`, then narrow with
    /// `.intValueExact()` — is reproduced literally as of Phase 3a's U9: this method is
    /// now exactly `arith_big_int(BigInt::from(a), BigInt::from(b))` followed by the
    /// narrowing check, mirroring `ArithmeticOperator.java:236-238`. (U6 originally
    /// inlined an `i64`-intermediate equivalent here, correct but a second copy of the
    /// floor-division correction; U9 needed the `BigInteger` form for
    /// `ArithmeticOperator.act`'s constant folding, so the two were collapsed onto one
    /// implementation rather than left to drift. Observable behavior — results, error
    /// variants, and the overflow message's exact text — is unchanged.)
    ///
    /// - [`Self::Div`]: **floor** division (rounds toward negative infinity), NOT
    ///   Java/Rust's default truncate-toward-zero. Java's `a.divideAndRemainder(b)`
    ///   truncates toward zero, then (`:251`) subtracts `1` from the quotient whenever
    ///   the remainder is nonzero AND the truncated quotient rounded the WRONG way
    ///   (operands' signs differ) — i.e. exactly the correction that turns
    ///   truncating division into flooring division. Division by zero returns
    ///   [`NumSysError::DivisionByZero`] (`WalnutException.divisionByZero()`, `:249`) —
    ///   a real, clean, checked `WalnutException`, unlike the overflow case below. The
    ///   `r != 0 && (a<0)!=(b<0)` guard below is only ever evaluated once `b != 0`
    ///   (checked first) and, since `r == a % b` is `0` whenever `a == 0`, only once
    ///   `a != 0` too — so `(a<0)!=(b<0)` and Java's `a.signum() != b.signum()` agree
    ///   exactly on every input that reaches it (signum's third value, `0`, is
    ///   unreachable there).
    /// - [`Self::UnaryNegative`]: the `default:` arm (`:256`) — unreachable through any
    ///   real `WordAutomaton` call site (unary negation of a word automaton is rewritten
    ///   to `arith(MINUS, 0, x)` by `ArithmeticOperator.processUnaryOperator`,
    ///   `ArithmeticOperator.java:111`, before it ever reaches here), but a live, tested
    ///   branch: [`NumSysError::UnexpectedOperator`] (`WalnutException.
    ///   unexpectedOperator`, text `"Unexpected operator:_"` — see that variant's docs
    ///   for why it is a DIFFERENT text from [`NumSysError::UnexpectedArithmeticOperator`]).
    /// - Overflow: [`NumSysError::ArithmeticIntOverflow`] — see that variant's docs.
    pub fn arith(self, a: i32, b: i32) -> Result<i32, NumSysError> {
        let result = self.arith_big_int(&BigInt::from(a), &BigInt::from(b))?;
        i32::try_from(&result).map_err(|_| {
            NumSysError::ArithmeticIntOverflow(format!(
                "{a} {op} {b} = {result}",
                op = self.symbol()
            ))
        })
    }

    /// `ArithmeticOperator.arith(Ops, BigInteger, BigInteger)`
    /// (`ArithmeticOperator.java:240-258`) — the **primary** Java overload (see
    /// [`Self::arith`], which is its `int` specialization and now delegates here).
    ///
    /// Added in Phase 3a's U9 for `ArithmeticOperator.act`'s constant-folding branch
    /// (`ArithmeticOperator.java:150-154`), which pushes the *unbounded* `BigInteger`
    /// result straight into a new `NumberLiteralExpression` — no `intValueExact` step at
    /// all — so narrowing there would be a real behavior change, not an optimization.
    ///
    /// - [`Self::Div`]: **floor** division (rounds toward negative infinity), NOT
    ///   truncation toward zero. Java's `a.divideAndRemainder(b)` truncates, then (`:251`)
    ///   subtracts `1` from the quotient exactly when the remainder is nonzero AND the
    ///   operands' signs differ. Ported literally, including using the *sign* comparison
    ///   (`a.signum() != b.signum()`) rather than a `< 0` test: the two agree on every
    ///   input that reaches the correction, because a nonzero remainder already implies
    ///   `a != 0` and the `b == 0` case returned above.
    /// - [`Self::UnaryNegative`]: the `default:` arm (`:256`),
    ///   [`NumSysError::UnexpectedOperator`].
    /// - Division by zero: [`NumSysError::DivisionByZero`] (`:249`).
    ///
    /// Unlike [`Self::arith`] this cannot overflow — `BigInteger` is unbounded, and so is
    /// [`BigInt`].
    pub fn arith_big_int(self, a: &BigInt, b: &BigInt) -> Result<BigInt, NumSysError> {
        Ok(match self {
            ArithmeticOp::Plus => a + b,
            ArithmeticOp::Minus => a - b,
            ArithmeticOp::Mult => a * b,
            ArithmeticOp::Div => {
                if b.sign() == Sign::NoSign {
                    return Err(NumSysError::DivisionByZero);
                }
                let q = a / b;
                let r = a % b;
                if r.sign() != Sign::NoSign && a.sign() != b.sign() {
                    q - 1
                } else {
                    q
                }
            }
            ArithmeticOp::UnaryNegative => {
                return Err(NumSysError::UnexpectedOperator(self.symbol()));
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Name parsing / normalization helpers (all `static`, no file I/O)
// ---------------------------------------------------------------------------

/// `UtilityMethods.isNumber` (`UtilityMethods.java:42-44`) — matches `^\d+$`.
///
/// This used to be a private copy of the same one-liner (added in U7, before
/// `crate::util` existed); now delegates to the canonical port in [`crate::util`]
/// (Phase 3a's U0b) so there is only one copy of this regex's semantics.
fn is_number(s: &str) -> bool {
    crate::util::is_number(s)
}

/// `NumberSystem.determineBase(String)` (`:265-267`): everything after the FIRST `_`.
/// With no `_` at all, Java's `indexOf` returns `-1` and `substring(0)` yields the
/// whole string — replicated here.
pub fn determine_base(name: &str) -> &str {
    match name.find('_') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// `NumberSystem.determineMsdOrLsd(String)` (`:268-270`): everything before the FIRST
/// `_`. Java throws `StringIndexOutOfBoundsException` when there is no `_`
/// (`substring(0, -1)`); here that is [`NumSysError::MalformedName`].
pub fn determine_msd_or_lsd(name: &str) -> Result<&str, NumSysError> {
    match name.find('_') {
        Some(i) => Ok(&name[..i]),
        None => Err(NumSysError::MalformedName(name.to_string())),
    }
}

/// `NumberSystem.normalizeNumberSystemToken(String)` (`:273-297`) — "Normalize various
/// cases currently allowed, like null, `"msd5"`, `"fib"`, etc."
///
/// `None` plays Java's `null`. Every branch is ported in Java's order, including the
/// quirk `NumberSystemTest` pins explicitly: a lone `"?"` survives the `isEmpty()`
/// check (which runs BEFORE the `?` is stripped), becomes the empty string, and falls
/// through to produce the unusable name `"msd_"`.
pub fn normalize_number_system_token(token: Option<&str>) -> String {
    let Some(token) = token else {
        return MSD_2.to_string();
    };
    let token = token.trim();
    if token.is_empty() {
        return MSD_2.to_string();
    }
    let token = token.strip_prefix('?').unwrap_or(token);
    if token == MSD || token == LSD {
        return format!("{token}_2");
    }
    if token.starts_with(MSD_UNDERSCORE) || token.starts_with(LSD_UNDERSCORE) {
        return token.to_string();
    }
    if let Some(rest) = token.strip_prefix(MSD) {
        return format!("{MSD_UNDERSCORE}{rest}");
    }
    if let Some(rest) = token.strip_prefix(LSD) {
        return format!("{LSD_UNDERSCORE}{rest}");
    }
    format!("{MSD_UNDERSCORE}{token}")
}

/// The body of `NumberSystem.parseBase()` (`:237-243`), as a free function over a
/// number-system NAME.
///
/// Java keeps this as an instance method reading `this.name`; it is split out here so
/// the `<= 1` half of its guard stays testable. That branch is only reachable in Java
/// via a custom-base file (`NumberSystemTest.testParseBaseRejectsBaseOfOne` has to
/// write `msd_1_addition.txt` to construct an `msd_1` at all), and this port has no
/// file loading — so with the instance method alone, half the guard would be dead and
/// untestable. [`NumberSystem::parse_base`] is the faithful instance-method wrapper.
///
/// Java's `TODO` on this method ("Note this currently only works for positive bases.
/// That may be by design?") is preserved as a comment rather than acted on: positive
/// bases are the whole scope here.
pub fn parse_base_of(name: &str) -> Result<i32, NumSysError> {
    let base_str = determine_base(name);
    if !is_number(base_str) {
        return Err(NumSysError::InvalidBase(base_str.to_string()));
    }
    let base: i32 = base_str
        .parse()
        .map_err(|_| NumSysError::BaseNotAnI32(base_str.to_string()))?;
    if base <= 1 {
        return Err(NumSysError::InvalidBase(base_str.to_string()));
    }
    Ok(base)
}

/// `NumberSystem.isNSDiffering` (`:179-192`) — do two per-track number-system lists
/// (with their alphabets) disagree? Used by `Main/Commands/Union.java:58` and
/// `Main/Commands/Concat.java:64` to decide whether two operands can be combined.
///
/// **Signature deviation, deliberate.** Java compares by `NumberSystem.getName()`;
/// this crate's [`crate::automaton::Automaton`] does not carry per-track
/// `NumberSystem` objects at all — its stand-in is `msd: Vec<Option<bool>>` (see that
/// type's struct docs), which loses the BASE half of the name. Comparing the stand-in
/// would silently call `msd_2` and `msd_3` "not differing", which is precisely one of
/// the cases `NumberSystemTest.testIsNSDifferingAllBranches` asserts IS differing. So
/// this takes the per-track names directly (`None` = Java's `null` entry).
///
/// **Wired as of U23** (`wr-cli`'s `union`/`concat` commands): each call site passes
/// [`crate::automaton::Automaton::track_ns_names`], which reconstructs a plain
/// `msd_k`/`lsd_k` name from the stand-in — see that method's own docs for the
/// resulting scope narrowing on custom bases (never over-approximates "differing").
pub fn is_ns_differing(
    nns: &[Option<&str>],
    first_ns: &[Option<&str>],
    a1: &[Vec<i32>],
    a2: &[Vec<i32>],
) -> bool {
    if nns.len() != first_ns.len() || a1 != a2 {
        return true;
    }
    for (nj, first_j) in nns.iter().zip(first_ns.iter()) {
        if nj.is_none() != first_j.is_none() {
            return true;
        }
        if let (Some(n), Some(f)) = (nj, first_j) {
            if n != f {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Automaton-construction primitives
// ---------------------------------------------------------------------------

/// `FA.addNewTransition(src, dest, inp)` (`FA.java:556-561`). Note it *replaces* any
/// existing destination list for `(src, inp)` (`TransitionsNFA.setNfaDTransition` is a
/// plain `put`), it does not append — every construction below relies on writing each
/// `(state, symbol)` pair at most once anyway.
fn add_new_transition(fa: &mut Fa, src: usize, dest: usize, inp: i32) {
    fa.d[src].insert(inp, vec![dest]);
}

/// `NumberSystem.initBasicAutomaton(IntList O, int inputSize, List<Integer> alphabet)`
/// (`:613-621`) composed with the one-arg overload (`:607-611`) and `FA.initBasicFA`
/// (`FA.java:74-81`): `o.len()` states with the given outputs, no transitions, and
/// `input_size` tracks all sharing `alphabet`.
///
/// Java's `a.getNS().add(this)` becomes `msd: vec![Some(is_msd); input_size]` — this
/// crate's established stand-in for the per-track `NumberSystem` list. (Java can pass
/// the partially-constructed `this` here; Rust cannot, hence the explicit flag.)
fn init_basic_automaton(
    o: Vec<i32>,
    input_size: usize,
    alphabet: &[i32],
    is_msd: bool,
) -> Automaton {
    let q = o.len();
    let mut a = Automaton::new(
        Fa {
            true_false: None,
            q0: 0,
            q,
            alphabet_size: 1,
            o,
            d: vec![BTreeMap::new(); q],
        },
        vec![alphabet.to_vec(); input_size],
        // Java's `Automaton()` leaves `label` an empty list; these automata are bound
        // later, by `comparison`/`arithmetic`.
        Vec::new(),
        vec![Some(is_msd); input_size],
    );
    a.determine_alphabet_size();
    a
}

/// `NumberSystem.baseNadditionAutomaton(int n)` (`:474-497`) — three tracks over
/// `{0..n-1}`, accepting iff `track2 == track0 + track1`, read most-significant-digit
/// first.
///
/// Two states: `0` (accepting) = "carry 0 out of the digits read so far", `1` = "carry
/// 1". Java's flat counter `l` runs `i` fastest inside `j` inside `k`, which is
/// exactly this crate's mixed-radix encoding with track 0 fastest-varying, so `l` IS
/// `encode([i, j, k])` — the port keeps Java's explicit counter rather than calling
/// `encode`, so the two stay visibly identical.
///
/// The symbol counter is deliberately plain `i32` arithmetic, NOT checked: Java's `l`
/// is a plain `int++` here (no `Math.addExact`, unlike `RichAlphabet.encode`), so a
/// base with `n^3 > i32::MAX` wraps in Java too. It is unreachable in practice —
/// `init_basic_automaton`'s `determine_alphabet_size` already does a checked `usize`
/// multiplication, and an `n` that large would need ~2^31 transitions — but noting it
/// rather than silently "improving" one side of the port.
fn base_n_addition_automaton(n: i32, is_msd: bool) -> Automaton {
    let alphabet: Vec<i32> = (0..n).collect();
    let mut addition = init_basic_automaton(vec![1, 0], 3, &alphabet, is_msd);
    let mut l = 0i32;
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                if i + j == k {
                    add_new_transition(&mut addition.fa, 0, 0, l);
                } else if i + j + 1 == k {
                    add_new_transition(&mut addition.fa, 0, 1, l);
                }
                if i + j + 1 == k + n {
                    add_new_transition(&mut addition.fa, 1, 1, l);
                } else if i + j == k + n {
                    add_new_transition(&mut addition.fa, 1, 0, l);
                }
                l += 1;
            }
        }
    }
    addition
}

/// `NumberSystem.lexicographicLessThan(List<Integer> alphabet)` (`:417-433`) — two
/// tracks, accepting iff track 0 is lexicographically less than track 1.
///
/// The alphabet is **sorted first** (Java: `Collections.sort` on a defensive copy),
/// which is what makes "index order" mean "value order" in the comparisons below.
///
/// Java's two symbol expressions are not typos and are ported verbatim: state 0's
/// transitions use `j * size + i` (so `encode([i, j])` — track 0 gets `i`), while
/// state 1's self-loop uses `i * size + j` (`encode([j, i])`). Because the double loop
/// enumerates every ordered pair, the state-1 line still installs a self-loop on
/// *every* symbol; the swapped spelling is cosmetic there, and load-bearing at state 0.
fn lexicographic_less_than(alphabet: &[i32], is_msd: bool) -> Automaton {
    let mut alphabet = alphabet.to_vec();
    alphabet.sort_unstable();
    let size = alphabet.len();
    let mut less_than = init_basic_automaton(vec![0, 1], 2, &alphabet, is_msd);
    for i in 0..size {
        for j in 0..size {
            if i == j {
                add_new_transition(&mut less_than.fa, 0, 0, (j * size + i) as i32);
            } else if i < j {
                add_new_transition(&mut less_than.fa, 0, 1, (j * size + i) as i32);
            }
            add_new_transition(&mut less_than.fa, 1, 1, (i * size + j) as i32);
        }
    }
    less_than
}

/// `NumberSystem.setEqualityAutomaton(List<Integer> alphabet)` (`:403-409`) — a single
/// accepting state self-looping exactly on the diagonal, so it accepts iff the two
/// tracks carry the same digit at every position ("two numbers are equal if the words
/// representing them are equal", class javadoc). Unlike `lexicographicLessThan` this
/// does NOT sort the alphabet — it doesn't need to, the diagonal is index-order
/// independent — and unlike `addition`/`lessThan` it is never reversed for lsd
/// (`:144` sits outside the `if (!isMsd)` blocks).
fn equality_automaton(alphabet: &[i32], is_msd: bool) -> Automaton {
    let size = alphabet.len();
    let mut equality = init_basic_automaton(vec![1], 2, alphabet, is_msd);
    for i in 0..size {
        add_new_transition(&mut equality.fa, 0, 0, (i * size + i) as i32);
    }
    equality
}

fn label_set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Custom-base file loading, made I/O-free
// ---------------------------------------------------------------------------

/// The two files one `NumberSystem.loadAutomatonOrNull` probe considers
/// (`NumberSystem.java:299-319`), already read and parsed by the caller.
///
/// `wr-core` performs no file I/O — that stays `wr-io`/`wr-cli`'s job, matching
/// `wr_io::reader`'s existing "takes a path, doesn't reach into `Session`" shape. So the
/// two `File.isFile()` probes and the two `new Automaton(address)` reads happen outside;
/// this type carries their *results*, and [`CustomBaseCandidates::resolve`] applies Java's
/// precedence and fallback verbatim.
///
/// Use [`custom_base_candidate_names`] to get the two file names, so the naming convention
/// (which lives in `NumberSystem.java`, not in `Session`) is not re-derived at the call
/// site.
#[derive(Debug, Clone, Default)]
pub struct CustomBaseCandidates {
    /// `Custom Bases/<name><extension>` — Java's `mainName` (`:306`). Taken as-is.
    pub main: Option<Automaton>,
    /// `Custom Bases/<lsd|msd>_<base><extension>` — Java's `complementName` (`:307`): the
    /// SAME base under the OPPOSITE direction ("When the number system does not exist, we
    /// try to see whether its complement exists or not. For example `lsd_2` is the
    /// complement of `msd_2`"). Consulted only when `main` is absent, and then
    /// language-**reversed** (`AutomatonLogicalOps.reverse(A, false)` — reverse the
    /// language, leave the declared direction alone).
    pub complement: Option<Automaton>,
}

impl CustomBaseCandidates {
    /// `NumberSystem.loadAutomatonOrNull` (`:299-319`) minus its two `File.isFile()`
    /// probes: "Tries to create an Automaton from the main file path. If it does not
    /// exist, tries the complement file path and reverses. Otherwise, returns null."
    ///
    /// Note the precedence is strict — a present `main` wins outright and is **not**
    /// reversed even for an lsd system, which is exactly why
    /// [`NumberSystem::set_addition_automaton`]'s `if (!isMsd) reverse(...)` sits INSIDE
    /// the "no file was found" branch in Java and must stay there here.
    pub fn resolve(self) -> Option<Automaton> {
        if let Some(main) = self.main {
            return Some(main);
        }
        if let Some(mut complement) = self.complement {
            reverse(&mut complement, false);
            return Some(complement);
        }
        None
    }
}

/// All three `loadAutomatonOrNull` probes a `NumberSystem` constructor makes, in Java's
/// own order: the adder (`_addition.txt`, `:323`), the comparator (`_less_than.txt`,
/// `:370`), and the "set of all representations" automaton (`.txt`, `:147`).
///
/// `Default::default()` (every candidate absent) reproduces the pre-U5, no-file-loading
/// behavior exactly, which is what [`NumberSystem::new`] passes.
///
/// There is deliberately no `_base_change.txt` slot: that whole surface is dropped (see
/// this module's docs), and its sole production caller (`determineNegativeNS`, for the
/// DROP-scope `split` command) is negative-base-only.
///
/// Nothing here covers `setEqualityAutomaton`: **Java never file-loads the equality
/// automaton** (`:403-409` takes only an alphabet and always builds the diagonal
/// programmatically — verified, not assumed). It nonetheless adapts to a custom base
/// automatically, because the alphabet it is handed is `getAlphabet()`, i.e. the
/// possibly-file-loaded adder's track-0 alphabet.
#[derive(Debug, Clone, Default)]
pub struct CustomBaseFiles {
    /// Probe for `<name>_addition.txt` (`UNDERSCORE_ADDITION_AUTOMATON`).
    pub addition: CustomBaseCandidates,
    /// Probe for `<name>_less_than.txt` (`UNDERSCORE_LESS_THAN_AUTOMATON`).
    pub less_than: CustomBaseCandidates,
    /// Probe for `<name>.txt` (`TXT_EXTENSION`) — the valid-representation restriction.
    pub all_representations: CustomBaseCandidates,
}

/// The two `Custom Bases/` FILE NAMES `loadAutomatonOrNull` probes for `name` and
/// `extension`, in Java's precedence order `(main, complement)` (`:306-307`).
///
/// Returns bare file names, not paths: prefixing the custom-bases directory is
/// `Session.getReadAddressForCustomBases`'s job, i.e. `wr-cli`'s (U14). Exposed from here
/// because the *naming convention* — "same base, opposite direction" — is
/// `NumberSystem.java`'s, and a caller re-deriving it would be re-deriving the fallback
/// semantics too.
///
/// `extension` is one of [`UNDERSCORE_ADDITION_AUTOMATON`],
/// [`UNDERSCORE_LESS_THAN_AUTOMATON`], [`TXT_EXTENSION`].
pub fn custom_base_candidate_names(
    name: &str,
    extension: &str,
) -> Result<(String, String), NumSysError> {
    let msd_or_lsd = determine_msd_or_lsd(name)?;
    let is_msd = msd_or_lsd == MSD;
    let base = determine_base(name);
    let opposite = if is_msd { LSD } else { MSD };
    Ok((
        format!("{name}{extension}"),
        format!("{opposite}_{base}{extension}"),
    ))
}

// ---------------------------------------------------------------------------
// NumberSystem
// ---------------------------------------------------------------------------

/// A base-*k* numeration system (`Automata/NumberSystem`), positive bases only.
///
/// Holds the three defining automata (`addition`, `lessThan`, `equality`) plus the
/// three memoization tables Java calls "dynamic tables". See the module docs for the
/// negative-base / custom-base / lsd scope decisions.
#[derive(Debug, Clone)]
pub struct NumberSystem {
    /// `NumberSystem.name` (`:89`), e.g. `"msd_2"`.
    name: String,
    /// `NumberSystem.isMsd` (`:94`).
    is_msd: bool,
    /// `NumberSystem.addition` (`:112`): three ordered inputs, accepts iff the third is
    /// the sum of the first two.
    addition: Automaton,
    /// `NumberSystem.lessThan` (`:113`): two ordered inputs, accepts iff the first is
    /// less than the second.
    less_than: Automaton,
    /// `NumberSystem.equality` (`:114`, `public` in Java too): two inputs, accepts iff
    /// they are equal.
    pub equality: Automaton,
    /// `NumberSystem.allRepresentations` (`:116`) **and** `flagUseAllRepresentations`
    /// (`:130`), folded into a single `Option` per `PORTING.md`'s "two coupled `boolean`s
    /// where one gates the other → one `Option`" rule — with that rule's required audit
    /// done rather than assumed:
    ///
    /// * **Writers.** The field and the flag are written in exactly one place, the
    ///   constructor's `:147-156`: the flag starts `true`, and the *only* thing that ever
    ///   clears it is `allRepresentations == null`. Nothing else in the tree writes either
    ///   (`grep` over all of `src/main/java`). So `flag == true` ⟺ field non-null,
    ///   post-construction.
    /// * **Readers.** `useAllRepresentations()` (`:253-255`) has two callers,
    ///   `Automaton.normalizeNumberSystems` (`:166`) and
    ///   `applyAllRepresentations(WithOutput)` (`Automaton.java:258`/`:278`); both consult
    ///   it before `getAllRepresentations()` (`:261-263`), so no reader ever reaches the
    ///   gated value without checking the gate.
    /// * **The window where they disagree.** Between field initialization and `:147` the
    ///   flag is `true` while the field is still `null`. Unobservable: nothing calls
    ///   `useAllRepresentations()` on a half-constructed `NumberSystem` — the constructor's
    ///   own reach-out chain (`loadAutomatonOrNull` → the `.txt` reader →
    ///   `ParseMethods.parseAlphabetDeclaration` → `getComputeIfAbsent`) only ever touches
    ///   *other* instances (and, per `docs/WALNUT-BUGS.md` WB-014, blows up if it does).
    all_representations: Option<Rc<Automaton>>,
    /// `constantsDynamicTable`/`multiplicationsDynamicTable`/`divisionsDynamicTable`
    /// (`:126-128`). Java uses `HashMap`; these are `BTreeMap` because [`BigInt`] is
    /// `Ord` and nothing here ever *iterates* them (so `PORTING.md`'s
    /// iteration-order trap doesn't bite either way) — lookup/insert only.
    ///
    /// # `RefCell`: `PORTING.md`'s Ruling 1, implemented (U5)
    ///
    /// Java hands the *same* cached `NumberSystem` instance to every token in a formula and
    /// lets those tokens mutate these three tables at `act()` time. Rather than let that
    /// force `Rc<RefCell<NumberSystem>>` into every `Token`/`Expression::act` signature,
    /// the memoization lives here behind interior mutability, so
    /// [`NumberSystem::get_constant`]/[`NumberSystem::get_multiplication`]/
    /// [`NumberSystem::get_division`] take `&self` and `wr_logic::predicate_env` can hand
    /// out a plain `Rc<NumberSystem>`. Nothing in `wr-logic` may wrap the handle in a
    /// second `RefCell` — that would recreate the aliasing problem the ruling exists to
    /// prevent, and put two independent memo caches behind one logical number system.
    ///
    /// # Borrow discipline (the one hazard `RefCell` introduces)
    ///
    /// Every construction below is **recursive** — `constant(n)` calls `get_constant(n/2)`,
    /// `multiplication(n)` calls `get_multiplication(n/2)`, `division(n)` calls
    /// `comparison_const_b`. So no borrow of any of these three cells may be held across a
    /// call back into `self`: each lookup takes its borrow, clones out, and drops it in the
    /// same statement; each insert takes a fresh `borrow_mut` after all recursion has
    /// finished. Violating that is a runtime `already borrowed` panic, not a compile error,
    /// which is why it is spelled out here.
    constants_dynamic_table: RefCell<BTreeMap<BigInt, Automaton>>,
    multiplications_dynamic_table: RefCell<BTreeMap<BigInt, Automaton>>,
    divisions_dynamic_table: RefCell<BTreeMap<BigInt, Automaton>>,
}

fn big(v: i32) -> BigInt {
    BigInt::from(v)
}

impl NumberSystem {
    /// `NumberSystem(String name)` (`:132-163`) with no custom-base files supplied — the
    /// plain `msd_k`/`lsd_k` path, and the exact pre-U5 behavior.
    ///
    /// A genuinely custom base (`msd_fib`, `msd_pell`, `msd_ns`, …) has no programmatic
    /// construction, so it still fails here with [`NumSysError::NotDefined`], exactly as it
    /// did before U5. Callers that CAN read `Custom Bases/` (i.e. `wr-io`/`wr-cli`) use
    /// [`NumberSystem::with_custom_base_files`] instead.
    pub fn new(name: &str) -> Result<NumberSystem, NumSysError> {
        Self::with_custom_base_files(name, CustomBaseFiles::default())
    }

    /// `NumberSystem(String name)` (`:132-163`) in full, with the three file loads lifted
    /// out into a caller-supplied [`CustomBaseFiles`] (see that type for why).
    ///
    /// Java's sequence, preserved exactly, because the order is load-bearing:
    ///
    /// 1. `determineMsdOrLsd`/`isMsd` (`:135-136`), then `isNeg` (`:137`) — which this port
    ///    turns into a hard [`NumSysError::UnsupportedNegativeBase`] rejection, BEFORE any
    ///    file is consulted, matching Java's own line order;
    /// 2. `setAdditionAutomaton` (`:142`) — file first, programmatic fallback second;
    /// 3. `setLessThanAutomaton` (`:143`) and `setEqualityAutomaton(getAlphabet())`
    ///    (`:144`), both of which read `getAlphabet()` = the (possibly file-loaded) adder's
    ///    track-0 alphabet, which is why they must come after step 2;
    /// 4. the all-representations file (`:147-156`): if absent, `flagUseAllRepresentations`
    ///    goes `false` and nothing else happens; if present, its own number-system list is
    ///    filled with `this` and `applyAllRepresentations()` is applied to the adder, the
    ///    comparator, and the equality automaton, in that order.
    ///
    /// # One declared, proven-unobservable divergence in step 4
    ///
    /// `Collections.fill(allRepresentations.getNS(), this)` (`:151`) makes the
    /// all-representations automaton point at the very number system that owns it —
    /// a reference cycle Java's GC shrugs off but `Rc` would leak. This port fills the
    /// direction half (`msd`) and leaves the [`crate::automaton::Automaton::all_reps`] half
    /// empty. Provably unread: `all_reps[i]` is only consulted by
    /// `apply_all_representations`, and the only reader of *this* automaton's copy would be
    /// `product::update_axb_fields`'s `bNS.get(i) != null && AxB.getNS().get(j) == null`
    /// merge — which cannot fire, because the automaton it is `and`ed into always has a
    /// non-`None` `msd` on the matching track (that track is precisely the one carrying the
    /// restriction, so `all_reps`'s own invariant makes its `msd` `Some`).
    pub fn with_custom_base_files(
        name: &str,
        files: CustomBaseFiles,
    ) -> Result<NumberSystem, NumSysError> {
        let msd_or_lsd = determine_msd_or_lsd(name)?;
        // `isMsd = msdOrLsd.equals(MSD)` (`:136`) -- anything that is not EXACTLY
        // "msd" (including "MSD", or the empty prefix of a name like "_5") is lsd.
        let is_msd = msd_or_lsd == MSD;
        // `isNeg = name.contains(UNDERSCORE_NEG_UNDERSCORE)` (`:137`) -> reject.
        if name.contains(UNDERSCORE_NEG_UNDERSCORE) {
            return Err(NumSysError::UnsupportedNegativeBase(name.to_string()));
        }
        let base = determine_base(name);

        let mut addition =
            Self::set_addition_automaton(name, base, is_msd, files.addition.resolve())?;
        let alphabet = addition.alphabet[0].clone();
        let mut less_than =
            Self::set_less_than_automaton(name, &alphabet, is_msd, files.less_than.resolve())?;
        let mut equality = equality_automaton(&alphabet, is_msd);

        // `allRepresentations = loadAutomatonOrNull(name, TXT_EXTENSION, base)` (`:147`).
        let all_representations = match files.all_representations.resolve() {
            // `flagUseAllRepresentations = false` (`:149`).
            None => None,
            Some(mut n) => {
                // `Collections.fill(allRepresentations.getNS(), this)` (`:151`) -- see this
                // method's doc comment for the `all_reps` half, deliberately left empty.
                n.msd = vec![Some(is_msd); n.alphabet.len()];
                let n = Rc::new(n);
                // `addition.applyAllRepresentations(); lessThan...; equality...` (`:153-155`).
                // Java reaches the automaton via each track's `NumberSystem` (= `this`,
                // installed by the two setters above and by `initBasicAutomaton`); here the
                // per-track handle is installed explicitly.
                for a in [&mut addition, &mut less_than, &mut equality] {
                    a.set_all_reps(vec![Some(Rc::clone(&n)); a.alphabet.len()]);
                    a.apply_all_representations();
                }
                Some(n)
            }
        };

        Ok(NumberSystem {
            name: name.to_string(),
            is_msd,
            addition,
            less_than,
            equality,
            all_representations,
            constants_dynamic_table: RefCell::new(BTreeMap::new()),
            multiplications_dynamic_table: RefCell::new(BTreeMap::new()),
            divisions_dynamic_table: RefCell::new(BTreeMap::new()),
        })
    }

    /// `NumberSystem.setAdditionAutomaton(String name, String base)` (`:322-367`).
    ///
    /// `loaded` is `loadAutomatonOrNull(name, UNDERSCORE_ADDITION_AUTOMATON, base)`'s
    /// result (`:323`), resolved by the caller. The `baseNegNAddition` fallback
    /// (`:327-328`) is deleted along with the rest of the negative-base surface — and is
    /// now unreachable anyway, since [`NumberSystem::with_custom_base_files`] rejects
    /// `_neg_` names outright.
    ///
    /// **The `if (!isMsd) reverse(...)` step sits INSIDE the "no file found" branch**
    /// (`:335-337`) — a file-loaded adder is used exactly as loaded, because
    /// `loadAutomatonOrNull` has already reversed it if (and only if) it came from the
    /// opposite direction's file. Getting this wrong would double-reverse every lsd custom
    /// base.
    ///
    /// The four structural validations (`:342-362`) were `assert!`s in U7 on the grounds
    /// that only a file-loaded automaton could fail them; they are `Err`s now (see
    /// [`NumSysError::AdditionInputCount`]). The programmatic construction still satisfies
    /// all four by construction.
    ///
    /// `addition.getNS().set(i, this)` (`:364-366`) is ported explicitly: for the
    /// programmatic path [`init_basic_automaton`] already did it, but a file-loaded adder
    /// arrives with whatever number systems its `.txt` header declared (`null` for a
    /// `{0,1}`-style declaration), and Java overwrites all of them.
    fn set_addition_automaton(
        name: &str,
        base: &str,
        is_msd: bool,
        loaded: Option<Automaton>,
    ) -> Result<Automaton, NumSysError> {
        let mut addition = match loaded {
            Some(loaded) => loaded,
            None => {
                if !is_number(base) {
                    return Err(NumSysError::NotDefined(name.to_string()));
                }
                let k: i32 = base
                    .parse()
                    .map_err(|_| NumSysError::BaseNotAnI32(base.to_string()))?;
                if k <= 1 {
                    return Err(NumSysError::NotDefined(name.to_string()));
                }
                let mut addition = base_n_addition_automaton(k, is_msd);
                if !is_msd {
                    // `AutomatonLogicalOps.reverse(addition, false)` (`:336`) -- reverse the
                    // LANGUAGE, keep the declared numeration direction (`reverseMsd = false`).
                    reverse(&mut addition, false);
                }
                addition
            }
        };

        if addition.alphabet.len() != 3 {
            return Err(NumSysError::AdditionInputCount(name.to_string()));
        }
        let alphabet = addition.alphabet[0].clone();
        if !alphabet.contains(&0) {
            return Err(NumSysError::AdditionAlphabetMissingZero(name.to_string()));
        }
        if !alphabet.contains(&1) {
            return Err(NumSysError::AdditionAlphabetMissingOne(name.to_string()));
        }
        for track in &addition.alphabet[1..] {
            // `UtilityMethods.areEqual` is SET equality (see `Automaton::remove_same_inputs`).
            let lhs: BTreeSet<i32> = track.iter().copied().collect();
            let rhs: BTreeSet<i32> = alphabet.iter().copied().collect();
            if lhs != rhs {
                return Err(NumSysError::AdditionAlphabetsDiffer(name.to_string()));
            }
        }
        // `for (i) addition.getNS().set(i, this)` (`:364-366`).
        addition.msd = vec![Some(is_msd); addition.alphabet.len()];
        Ok(addition)
    }

    /// `NumberSystem.setLessThanAutomaton(String name, String base)` (`:369-396`).
    ///
    /// `loaded` is `loadAutomatonOrNull(name, UNDERSCORE_LESS_THAN_AUTOMATON, base)`'s
    /// result (`:370`); the `baseNegNLessThan` fallback (`:372-373`) is deleted with the
    /// rest of the negative-base surface. Same "the reverse lives inside the no-file
    /// branch" note as [`NumberSystem::set_addition_automaton`]. `alphabet` is
    /// `getAlphabet()`, i.e. the adder's track-0 alphabet.
    ///
    /// Note `lessThan.getNS().set(i, this)` (`:392`) sits INSIDE the validation loop, after
    /// that iteration's alphabet check — so a mismatch on track 0 leaves track 1's number
    /// system unset. Irrelevant (the error aborts construction) but ported in place rather
    /// than hoisted.
    fn set_less_than_automaton(
        name: &str,
        alphabet: &[i32],
        is_msd: bool,
        loaded: Option<Automaton>,
    ) -> Result<Automaton, NumSysError> {
        let mut less_than = match loaded {
            Some(loaded) => loaded,
            None => {
                let mut less_than = lexicographic_less_than(alphabet, is_msd);
                if !is_msd {
                    reverse(&mut less_than, false);
                }
                less_than
            }
        };
        if less_than.alphabet.len() != 2 {
            return Err(NumSysError::LessThanInputCount(name.to_string()));
        }
        let rhs: BTreeSet<i32> = alphabet.iter().copied().collect();
        for i in 0..less_than.alphabet.len() {
            let lhs: BTreeSet<i32> = less_than.alphabet[i].iter().copied().collect();
            if lhs != rhs {
                return Err(NumSysError::LessThanAlphabetMismatch(name.to_string()));
            }
            less_than.msd[i] = Some(is_msd);
        }
        Ok(less_than)
    }

    /// `NumberSystem.isMsd()` (`:245-247`).
    pub fn is_msd(&self) -> bool {
        self.is_msd
    }

    /// `NumberSystem.getName()` (`:249-251`) / `toString()` (`:652-654`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `NumberSystem.useAllRepresentations()` (`:253-255`). `true` exactly when a
    /// `Custom Bases/<name>.txt` "set of all representations" automaton was supplied —
    /// i.e. never for a plain `msd_k`/`lsd_k`, and (of the bases `walnut-java` ships)
    /// always for `msd_fib`/`msd_pell`/`msd_trib`/`msd_tib`/`msd_ns`/`msd_kim`/
    /// `msd_nara`/`msd_pisot4`.
    ///
    /// Before U5 this was a hardcoded `false`, with the port's own docs (here, in
    /// `logicalops.rs`, and in `product.rs`) reasoning from that to
    /// "`applyAllRepresentations` is dead code". All three are corrected.
    pub fn use_all_representations(&self) -> bool {
        self.all_representations.is_some()
    }

    /// `NumberSystem.getAllRepresentations()` (`:261-263`).
    ///
    /// Java returns the field directly and its callers then `bind` it, mutating the shared
    /// instance against the class's own "returned automata must not be altered" warning;
    /// this hands back the shared [`Rc`] and
    /// [`crate::automaton::Automaton::apply_all_representations`] clones out of it before
    /// binding. See that method's docs — the difference is unobservable.
    pub fn all_representations(&self) -> Option<&Rc<Automaton>> {
        self.all_representations.as_ref()
    }

    /// `NumberSystem.getAlphabet()` (`:257-259`) — `addition.richAlphabet.getA().get(0)`.
    pub fn get_alphabet(&self) -> &[i32] {
        &self.addition.alphabet[0]
    }

    /// `NumberSystem.determineBaseNameUnderscore()` (`:603-605`).
    pub fn determine_base_name_underscore(&self) -> &'static str {
        if self.is_msd {
            MSD_UNDERSCORE
        } else {
            LSD_UNDERSCORE
        }
    }

    /// `NumberSystem.parseBase()` (`:237-243`) — see [`parse_base_of`].
    pub fn parse_base(&self) -> Result<i32, NumSysError> {
        parse_base_of(&self.name)
    }

    /// Borrows the addition automaton (`NumberSystem.addition`, `private` in Java with
    /// no getter — exposed here for tests and for the eventual `wr-logic` callers that
    /// Java reaches through `arithmetic`).
    pub fn addition(&self) -> &Automaton {
        &self.addition
    }

    /// Borrows the less-than automaton (`NumberSystem.lessThan`, same note as
    /// [`NumberSystem::addition`]).
    pub fn less_than(&self) -> &Automaton {
        &self.less_than
    }

    /// The body of `NumberSystem.flipNS`'s loop (`:166-177`) for a single system:
    /// rebuild it under the opposite direction, same base.
    ///
    /// The LIST-level `flipNS` is already ported, as `logicalops`'s private `flip_ns`
    /// over this crate's `msd: Vec<Option<bool>>` stand-in (it is what
    /// `AutomatonLogicalOps.reverse(A, true)` calls). This single-system form is what
    /// that stand-in cannot express — it constructs a genuinely new `NumberSystem`,
    /// with new adder/comparator automata — and is what
    /// `NumberSystemTest.testMSDFlip`'s assertions are actually about.
    pub fn flip(&self) -> Result<NumberSystem, NumSysError> {
        self.flip_with_custom_base_files(CustomBaseFiles::default())
    }

    /// [`NumberSystem::flip`] for a custom base: the caller supplies the FLIPPED name's
    /// `Custom Bases/` files, since Java's `new NumberSystem(newName)` re-runs the whole
    /// file lookup under the new name and `wr-core` cannot.
    ///
    /// Without this, `flip()` on a custom base fails with [`NumSysError::NotDefined`]
    /// (`"fib"` is not a base this crate can build programmatically) — correct, but useless
    /// to a caller that does have the files. Use [`custom_base_candidate_names`] with the
    /// flipped name to find out which files to read.
    pub fn flip_with_custom_base_files(
        &self,
        files: CustomBaseFiles,
    ) -> Result<NumberSystem, NumSysError> {
        let msd_or_lsd = determine_msd_or_lsd(&self.name)?;
        let base = determine_base(&self.name);
        let new_name = format!("{}_{}", if msd_or_lsd == MSD { LSD } else { MSD }, base);
        NumberSystem::with_custom_base_files(&new_name, files)
    }

    /// `NumberSystem.validateNeg(BigInteger)` (`:1026-1028`), with the `!isNeg` half
    /// folded away (always true here — see module docs).
    fn validate_non_negative(&self, n: &BigInt) -> Result<(), NumSysError> {
        if *n < big(0) {
            return Err(NumSysError::NegativeConstant(n.to_string()));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // comparison
    // -----------------------------------------------------------------------

    /// `NumberSystem.applyComparison` (`:656-665`).
    fn apply_comparison(
        base: &Automaton,
        a: &str,
        b: &str,
        reverse_operands: bool,
        negate: bool,
    ) -> Automaton {
        let mut result = base.clone();
        result.bind(if reverse_operands {
            names(&[b, a])
        } else {
            names(&[a, b])
        });
        if negate {
            result = not(result.as_dfa()).into_automaton();
        }
        result
    }

    /// `NumberSystem.comparison(String a, String b, RelationalOperator.Ops)`
    /// (`:675-684`) — two inputs labelled `a` and `b`, accepting iff `a op b`.
    ///
    /// Note the pairing: `>=` is `!(a < b)` (operands NOT swapped, negated) while `<=`
    /// is `!(b < a)` (operands swapped AND negated). Java's own javadoc warns the
    /// resulting input order is not guaranteed to be `(a,b)`.
    pub fn comparison(&self, a: &str, b: &str, op: RelationalOp) -> Automaton {
        match op {
            RelationalOp::LessThan => Self::apply_comparison(&self.less_than, a, b, false, false),
            RelationalOp::GreaterThan => Self::apply_comparison(&self.less_than, a, b, true, false),
            RelationalOp::Equal => Self::apply_comparison(&self.equality, a, b, false, false),
            RelationalOp::NotEqual => Self::apply_comparison(&self.equality, a, b, false, true),
            RelationalOp::GreaterEqThan => {
                Self::apply_comparison(&self.less_than, a, b, false, true)
            }
            RelationalOp::LessEqThan => Self::apply_comparison(&self.less_than, a, b, true, true),
        }
    }

    /// `NumberSystem.comparison(String a, BigInteger b, RelationalOperator.Ops)`
    /// (`:696-723`) — one input labelled `a`, accepting iff `a op b`.
    ///
    /// The `b.signum() < 0` arm (`:700-703`) is DELETED: `validateNeg(b)` on the line
    /// above already rejected every negative `b` once negative bases are out of scope
    /// (see module docs).
    ///
    /// `EQUAL`/`NOT_EQUAL` short-circuit on the constant automaton itself; every other
    /// relation binds the constant to the fresh name `"new " + a` (Java's comment:
    /// "this way, we make sure B != a"), intersects with the two-variable comparison,
    /// and quantifies that name away.
    pub fn comparison_const_b(
        &self,
        a: &str,
        b: &BigInt,
        op: RelationalOp,
    ) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(b)?;
        let b_name = format!("new {a}");
        let mut n = self.get_constant(b)?;
        if op == RelationalOp::Equal {
            n.bind(names(&[a]));
            return Ok(n);
        }
        if op == RelationalOp::NotEqual {
            n.bind(names(&[a]));
            return Ok(not(n.as_dfa()).into_automaton());
        }
        n.bind(names(&[&b_name]));
        let m = self.comparison(a, &b_name, op);
        let mut m = and(&m, &n).into_automaton();
        quantify(&mut m, &label_set(&[&b_name]))?;
        Ok(m)
    }

    /// `NumberSystem.comparison(BigInteger a, String b, RelationalOperator.Ops)`
    /// (`:735-738`) — the constant on the LEFT, delegated by reversing the relation.
    pub fn comparison_const_a(
        &self,
        a: &BigInt,
        b: &str,
        op: RelationalOp,
    ) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(a)?;
        self.comparison_const_b(b, a, op.reverse_operator())
    }

    // -----------------------------------------------------------------------
    // arithmetic
    // -----------------------------------------------------------------------

    /// `NumberSystem.arithmetic(String a, String b, String c, ArithmeticOperator.Ops)`
    /// (`:750-769`) — three inputs, accepting iff `c = a op b`.
    ///
    /// `MINUS` is just `PLUS` with the tracks bound in a different order: the adder
    /// accepts `(x, y, z)` with `z = x + y`, so binding `(b, c, a)` asserts
    /// `a = b + c`, i.e. `c = a - b`.
    pub fn arithmetic(
        &self,
        a: &str,
        b: &str,
        c: &str,
        op: ArithmeticOp,
    ) -> Result<Automaton, NumSysError> {
        let mut m = self.addition.clone();
        match op {
            ArithmeticOp::Plus => m.bind(names(&[a, b, c])),
            ArithmeticOp::Minus => m.bind(names(&[b, c, a])),
            ArithmeticOp::Mult | ArithmeticOp::Div => {
                return Err(NumSysError::OperatorTwoVariables(op.symbol()))
            }
            // Java's `default:` arm (`:765-766`); only `UNARY_NEGATIVE` reaches it.
            ArithmeticOp::UnaryNegative => {
                return Err(NumSysError::UnexpectedArithmeticOperator(op.symbol()))
            }
        }
        Ok(m)
    }

    /// `NumberSystem.arithmetic(String a, BigInteger b, String c, ArithmeticOperator.Ops)`
    /// (`:789-824`) — two inputs `a` and `c`, accepting iff `c = a op b`.
    ///
    /// The `b.signum() < 0` rewrite (`:809-813`, "we rewrite `a-b=c` as `a+(-b)=c`") is
    /// DELETED — unreachable after `validateNeg` once negative bases are gone.
    pub fn arithmetic_const_b(
        &self,
        a: &str,
        b: &BigInt,
        c: &str,
        op: ArithmeticOp,
    ) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(b)?;
        if op == ArithmeticOp::Mult {
            let mut n = self.get_multiplication(b)?;
            n.bind(names(&[a, c]));
            return Ok(n);
        }
        if op == ArithmeticOp::Div {
            let mut n = self.get_division(b)?;
            n.bind(names(&[a, c]));
            return Ok(n);
        }

        // Java: `String B = a + c;` -- "this way we make sure that B is not equal to a
        // or c" (string CONCATENATION, not addition).
        let b_name = format!("{a}{c}");
        let mut n = self.get_constant(b)?;
        n.bind(names(&[&b_name]));
        let m = self.arithmetic(a, &b_name, c, op)?;
        let mut m = and(&m, &n).into_automaton();
        quantify(&mut m, &label_set(&[&b_name]))?;
        Ok(m)
    }

    /// `NumberSystem.arithmetic(BigInteger a, String b, String c, ArithmeticOperator.Ops)`
    /// (`:844-877`) — two inputs `b` and `c`, accepting iff `c = a op b`.
    ///
    /// The `a.signum() < 0 && PLUS` rewrite (`:861-864`) is DELETED (unreachable after
    /// `validateNeg`), so only the `else` arm survives — which is also the arm Java's
    /// own `NumberSystemTest.testConstantAsTheLeftOperand` characterizes for `a >= 0`.
    pub fn arithmetic_const_a(
        &self,
        a: &BigInt,
        b: &str,
        c: &str,
        op: ArithmeticOp,
    ) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(a)?;
        if op == ArithmeticOp::Mult {
            let mut n = self.get_multiplication(a)?;
            n.bind(names(&[b, c]));
            return Ok(n);
        }
        if op == ArithmeticOp::Div {
            return Err(NumSysError::ConstantDividedByVariable);
        }

        let a_name = format!("{b}{c}");
        let mut n = self.get_constant(a)?;
        n.bind(names(&[&a_name]));
        let m = self.arithmetic(&a_name, b, c, op)?;
        let mut m = and(&m, &n).into_automaton();
        quantify(&mut m, &label_set(&[&a_name]))?;
        Ok(m)
    }

    /// `NumberSystem.arithmetic(String a, String b, BigInteger c, ArithmeticOperator.Ops)`
    /// (`:897-926`) — two inputs `a` and `b`, accepting iff `c = a op b`.
    ///
    /// The `c.signum() < 0 && MINUS` rewrite (`:910-913`) is DELETED (unreachable after
    /// `validateNeg`).
    pub fn arithmetic_const_c(
        &self,
        a: &str,
        b: &str,
        c: &BigInt,
        op: ArithmeticOp,
    ) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(c)?;
        if op == ArithmeticOp::Mult || op == ArithmeticOp::Div {
            return Err(NumSysError::OperatorTwoVariables(op.symbol()));
        }

        // Java's comment here says "this way we make sure that A is not equal to a or
        // b" while naming the variable `C` -- a stale copy-paste, preserved as-is.
        let c_name = format!("{a}{b}");
        let mut n = self.get_constant(c)?;
        n.bind(names(&[&c_name]));
        let m = self.arithmetic(a, b, &c_name, op)?;
        let mut m = and(&m, &n).into_automaton();
        quantify(&mut m, &label_set(&[&c_name]))?;
        Ok(m)
    }

    // -----------------------------------------------------------------------
    // constant / multiplication / division (the three "dynamic tables")
    // -----------------------------------------------------------------------

    /// `NumberSystem.getConstant(BigInteger n)` (`:632-634`) — a fresh single-input
    /// automaton accepting exactly the representations of `n` (leading zeros included).
    ///
    /// Java returns `constant(n).clone()`, i.e. a deep copy of the memoized automaton,
    /// because the class-level warning is explicit that the cached instances must never
    /// be mutated by a caller. Same here.
    ///
    /// # `&self`, not `&mut self` (U5, `PORTING.md`'s Ruling 1)
    ///
    /// This and its two siblings memoize into [`RefCell`]-wrapped tables, so a formula's
    /// tokens can share one `Rc<NumberSystem>` and still populate the cache — see the
    /// doc comment on `constants_dynamic_table` for the full argument and the borrow
    /// discipline the private builders below must respect. Behaviorally identical to the
    /// pre-U5 `&mut self` version: same memo keys, same automata, same errors.
    ///
    /// One structural simplification comes with it. Java's `getConstant` is
    /// `constant(n).clone()`, where `constant` returns the cached instance by reference; a
    /// `RefCell` cannot lend out a reference that outlives the borrow, so [`Self::constant`]
    /// now returns the clone itself and this is a pass-through. That removes one of the two
    /// clones per call and changes nothing observable — every internal caller already went
    /// through the `get_*` wrappers, never through `constant`/`multiplication`/`division`
    /// directly (verified across all call sites, in both engines).
    pub fn get_constant(&self, n: &BigInt) -> Result<Automaton, NumSysError> {
        self.constant(n)
    }

    /// `NumberSystem.getMultiplication(BigInteger n)` (`:648-650`) — two inputs,
    /// accepting iff the second is `n` times the first.
    ///
    /// Java's `getMultiplication(int)`/`getDivision(int)` overloads (`:636-638`,
    /// `:644-646`) are confirmed-dead (zero callers; `docs/WALNUT-BUGS.md`'s dead-code
    /// section) and are not ported — the `BigInteger` forms below are the live ones,
    /// reached from `arithmetic` with a `MULT`/`DIV` operator.
    /// `&self` as of U5 — see [`NumberSystem::get_constant`].
    pub fn get_multiplication(&self, n: &BigInt) -> Result<Automaton, NumSysError> {
        self.multiplication(n)
    }

    /// `NumberSystem.getDivision(BigInteger n)` (`:640-642`) — two inputs, accepting
    /// iff the second is one `n`th of the first.
    /// `&self` as of U5 — see [`NumberSystem::get_constant`].
    pub fn get_division(&self, n: &BigInt) -> Result<Automaton, NumSysError> {
        self.division(n)
    }

    /// `NumberSystem.constant(BigInteger n)` (`:931-971`), memoized.
    ///
    /// `n == 0` and `n == 1` are the base cases (see [`NumberSystem::make_zero`] /
    /// [`NumberSystem::make_one`]); the `n < 0` arm (`:944-951`) is DELETED
    /// (unreachable after `validateNeg`). For `n >= 2` the automaton is built
    /// **recursively by halving**: `Ea Eb (a + b = c & a = floor(n/2) & b = ceil(n/2))`,
    /// so the recursion depth is `log2(n)` and each level costs one adder intersection
    /// plus one ∃-elimination.
    ///
    /// This is entirely self-contained arithmetic over the adder — the earlier
    /// speculation that it might need a regex/`BricsConverter` substitute applies ONLY
    /// to the two base cases, which are `0*` and `0*1`/`10*`.
    ///
    /// Returns an owned clone rather than a borrow of the cache entry — see
    /// [`NumberSystem::get_constant`]'s note on why, and the `constants_dynamic_table` doc
    /// comment for the borrow discipline the recursion below relies on (the lookup borrow
    /// is scoped and dropped before any recursive call; the insert takes a fresh one).
    fn constant(&self, n: &BigInt) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(n)?;
        {
            let table = self.constants_dynamic_table.borrow();
            if let Some(cached) = table.get(n) {
                return Ok(cached.clone());
            }
        }

        let (a, b, c) = ("a", "b", "c");
        let p = if *n == big(0) {
            self.make_zero()
        } else if *n == big(1) {
            self.make_one()
        } else {
            // `n.divideAndRemainder(2)`: floor and ceil halves. `/` on a non-negative
            // BigInt truncates toward zero, which IS floor here.
            let two = big(2);
            let floor_half = n / &two;
            let remainder = n % &two;
            let ceil_half = &floor_half + &remainder;
            let mut m = self.get_constant(&floor_half)?;
            m.bind(names(&[a]));
            let mut nn = self.get_constant(&ceil_half)?;
            nn.bind(names(&[b]));
            let p = self.arithmetic(a, b, c, ArithmeticOp::Plus)?;
            let p = and(&p, &m).into_automaton();
            let mut p = and(&p, &nn).into_automaton();
            quantify(&mut p, &label_set(&[a, b]))?;
            p
        };

        // Java stores here as well as inside `makeConstant` for the 0/1 cases -- a
        // harmless double `put` of the same value, ported as-is.
        self.constants_dynamic_table
            .borrow_mut()
            .insert(n.clone(), p.clone());
        Ok(p)
    }

    /// `NumberSystem.makeZero()` (`:1060-1062`) — `makeConstant("0*", 0)`.
    fn make_zero(&self) -> Automaton {
        // `0*`: one accepting state self-looping on digit 0.
        let mut d0 = BTreeMap::new();
        d0.insert(0, vec![0usize]);
        self.make_constant(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 1,
                o: vec![1],
                d: vec![d0],
            },
            0,
        )
    }

    /// `NumberSystem.makeOne()` (`:1064-1066`) — `makeConstant(isMsd ? "0*1" : "10*", 1)`.
    fn make_one(&self) -> Automaton {
        let fa = if self.is_msd {
            // `0*1`: state 0 loops on 0 and moves to the accepting state 1 on digit 1;
            // state 1 has no outgoing transitions.
            let mut d0 = BTreeMap::new();
            d0.insert(0, vec![0usize]);
            d0.insert(1, vec![1usize]);
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 1,
                o: vec![0, 1],
                d: vec![d0, BTreeMap::new()],
            }
        } else {
            // `10*`: state 0 moves to the accepting state 1 on digit 1; state 1 loops
            // on digit 0.
            let mut d0 = BTreeMap::new();
            d0.insert(1, vec![1usize]);
            let mut d1 = BTreeMap::new();
            d1.insert(0, vec![1usize]);
            Fa {
                true_false: None,
                q0: 0,
                q: 2,
                alphabet_size: 1,
                o: vec![0, 1],
                d: vec![d0, d1],
            }
        };
        self.make_constant(fa, 1)
    }

    /// `NumberSystem.makeConstant(String regex, int constant)` (`:1068-1078`).
    ///
    /// **The one substitution in this file.** Java builds the automaton with
    /// `new AutomatonDFA(regex, UtilityMethods.intRangeList(2), this)`, i.e. it
    /// compiles the regex over the two-letter alphabet `{0,1}` through
    /// `dk.brics.automaton` (`FA/BricsConverter.convertFromBrics`), then *widens* the
    /// track's alphabet to the full `getAlphabet()` (`0..k-1`), resets the encoder to
    /// `[1]`, and canonizes. `BricsConverter` is an open dependency question
    /// (`docs/BOUNDARY-MAP.md` §4.4) and is not ported, so the two regexes this method
    /// is EVER called with — `"0*"` and `"0*1"`/`"10*"`, from
    /// [`NumberSystem::make_zero`]/[`NumberSystem::make_one`], the only two call sites
    /// — are supplied by the caller as hand-built minimal DFAs instead.
    ///
    /// The widening is what makes that safe: because the regex alphabet is `[0, 1]`,
    /// brics symbols are alphabet *indices* `0`/`1`, and after the widening to
    /// `0..k-1` (whose index `i` is digit `i`) those same symbols mean digits `0`/`1`.
    /// Digits `>= 2` simply have no transition, so the language is `0*` / `0*1` / `10*`
    /// over the base-`k` alphabet either way. State counts may differ from brics's
    /// (both are minimal here, but this crate compares by language equivalence anyway —
    /// `CLAUDE.md` prime directive #1).
    ///
    /// Java does NOT reset the track's number system after the widening, so the track
    /// keeps the `numSys` that `AutomatonDFA`'s constructor attached (`AutomatonDFA.java:58`)
    /// — replicated by `msd: [Some(self.is_msd)]`.
    fn make_constant(&self, fa: Fa, constant: i32) -> Automaton {
        let alphabet = self.get_alphabet().to_vec();
        let mut m = Automaton::new(fa, vec![alphabet], Vec::new(), vec![Some(self.is_msd)]);
        m.determine_alphabet_size();
        m.setup_encoder();
        m.canonize();
        self.constants_dynamic_table
            .borrow_mut()
            .insert(BigInt::from(constant), m.clone());
        m
    }

    /// `NumberSystem.multiplication(BigInteger n)` (`:976-1024`), memoized. Two inputs;
    /// accepts iff the second is `n` times the first.
    ///
    /// `n == 0` is rejected outright ("the case of n==0 is handled in Computer class").
    /// The `n < 0` arm (`:986-994`) is DELETED (unreachable after `validateNeg`). For
    /// `n > 2` this is binary exponentiation over the doubler: with `k = n / 2` and
    /// `b = k*a`, either `d = 2b` (`n` even) or `d = 2b + a` (`n` odd).
    ///
    /// Java's `n == 1` arm assigns `P = equality` — the very same object, aliased into
    /// the memo table rather than copied. Cloning here is unconditionally equivalent,
    /// not merely "safe for now": `Automaton`/`Fa` hold no interior/shared mutability
    /// anywhere in this crate (no `Rc`/`Arc`/`RefCell`/`Cow`, only plain `Vec`/`BTreeMap`
    /// fields — verified, not assumed), so no clone of a memoized value can ever be
    /// mutated in a way that reaches back into `self.equality` or another memo entry,
    /// regardless of what future callers (e.g. a ported `applyAllRepresentations`) do
    /// with their own clone.
    fn multiplication(&self, n: &BigInt) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(n)?;
        if *n == big(0) {
            return Err(NumSysError::MultiplicationByZero);
        }
        {
            let table = self.multiplications_dynamic_table.borrow();
            if let Some(cached) = table.get(n) {
                return Ok(cached.clone());
            }
        }
        let (a, b, c, d) = ("a", "b", "c", "d");
        let two = big(2);

        let p = if *n == big(1) {
            self.equality.clone()
        } else if *n == two {
            // `a + a = d`: `bind` merges the two same-labelled tracks, leaving (a, d).
            let mut p = self.arithmetic(a, a, d, ArithmeticOp::Plus)?;
            p.sort_label();
            p
        } else {
            // Java evaluates the doubler BEFORE the recursive `getMultiplication(k)`;
            // order preserved (it only affects track order, never the language).
            let mut doubler = self.get_multiplication(&two)?;
            let k = n / &two;
            let mut m = self.get_multiplication(&k)?;
            m.bind(names(&[a, b]));

            let mut p = if n % &two == big(0) {
                doubler.bind(names(&[b, d]));
                let mut p = and(&m, &doubler).into_automaton();
                quantify(&mut p, &label_set(&[b]))?;
                p
            } else {
                doubler.bind(names(&[b, c]));
                let p = self.arithmetic(c, a, d, ArithmeticOp::Plus)?;
                let p = and(&p, &m).into_automaton();
                let mut p = and(&p, &doubler).into_automaton();
                quantify(&mut p, &label_set(&[b, c]))?;
                p
            };
            p.sort_label();
            p
        };

        self.multiplications_dynamic_table
            .borrow_mut()
            .insert(n.clone(), p.clone());
        Ok(p)
    }

    /// `NumberSystem.division(BigInteger n)` (`:1034-1058`), memoized. Two inputs;
    /// accepts iff the second is one `n`th of the first (integer division).
    ///
    /// `a / n = b  <=>  Er,q  a = q + r & q = n*b & 0 <= r < n` (Java's own comment).
    /// The `n < 0` operand selections at `:1047-1048` are DELETED (unreachable after
    /// `validateNeg`), so the two range comparisons are always `r >= 0` and `r < n`.
    fn division(&self, n: &BigInt) -> Result<Automaton, NumSysError> {
        self.validate_non_negative(n)?;
        if *n == big(0) {
            return Err(NumSysError::DivisionByZero);
        }
        {
            let table = self.divisions_dynamic_table.borrow();
            if let Some(cached) = table.get(n) {
                return Ok(cached.clone());
            }
        }
        let (a, b, r, q) = ("a", "b", "r", "q");

        let m = self.arithmetic(q, r, a, ArithmeticOp::Plus)?;
        let nn = self.arithmetic_const_a(n, b, q, ArithmeticOp::Mult)?;
        let p1 = self.comparison_const_b(r, &big(0), RelationalOp::GreaterEqThan)?;
        let p2 = self.comparison_const_b(r, n, RelationalOp::LessThan)?;

        let p = and(&p1, &p2).into_automaton();
        let rr = and(&m, &nn).into_automaton();
        let mut rr = and(&rr, &p).into_automaton();
        quantify(&mut rr, &label_set(&[q, r]))?;
        rr.sort_label();

        self.divisions_dynamic_table
            .borrow_mut()
            .insert(n.clone(), rr.clone());
        Ok(rr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equiv;
    use proptest::prelude::*;

    // ------------------------------------------------------------------ helpers

    /// Feeds a two-track word (one digit pair per position, msd-first) into `fa` and
    /// returns whether it's accepted — using `automaton.encode` so the test doesn't
    /// hand-compute symbol ids (that would just re-derive the encoder formula, not
    /// exercise it).
    fn accepts(a: &Automaton, digit_pairs: &[(i32, i32)]) -> bool {
        let word: Vec<i32> = digit_pairs
            .iter()
            .map(|&(x, y)| a.encode(&[x, y]))
            .collect();
        a.fa.accepts_word(&word)
    }

    /// `NumberSystemTest.acceptsTuples` (`:681-706`): simulate `a` on a word given as
    /// decoded per-track digit tuples in `a`'s own track order, determinizing on the
    /// fly so this works for NFAs too.
    fn accepts_tuples(a: &Automaton, word: &[Vec<i32>]) -> bool {
        let encoded: Vec<i32> = word.iter().map(|letter| a.encode(letter)).collect();
        a.fa.accepts_word(&encoded)
    }

    /// `NumberSystemTest.acceptsDigits` (`:659-675`): assemble the word in the
    /// automaton's OWN label order, so the test is insensitive to how `bind`/`quantify`
    /// happened to order the inputs. All digit strings must have the same length.
    fn accepts_digits(a: &Automaton, digits_by_label: &[(&str, &str)]) -> bool {
        let supplied: BTreeSet<&str> = digits_by_label.iter().map(|&(l, _)| l).collect();
        let actual: BTreeSet<&str> = a.label.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            supplied, actual,
            "test supplied digits for the wrong set of tracks"
        );
        let length = digits_by_label[0].1.len();
        let mut word = Vec::with_capacity(length);
        for position in 0..length {
            let mut tuple = Vec::with_capacity(a.label.len());
            for name in &a.label {
                let digits = digits_by_label
                    .iter()
                    .find(|&&(l, _)| l == name.as_str())
                    .unwrap()
                    .1;
                assert_eq!(length, digits.len(), "all tracks must have the same length");
                tuple.push((digits.as_bytes()[position] - b'0') as i32);
            }
            word.push(tuple);
        }
        accepts_tuples(a, &word)
    }

    /// `"0101"` -> `[[0],[1],[0],[1]]`: a word over a single-track alphabet
    /// (`NumberSystemTest.singleTrack`, `:645-651`).
    fn single_track(digits: &str) -> Vec<Vec<i32>> {
        digits.bytes().map(|b| vec![(b - b'0') as i32]).collect()
    }

    /// `v` written in base `base` with exactly `width` digits, most-significant first.
    /// `None` if it doesn't fit.
    fn msd_digits(mut v: u32, base: u32, width: usize) -> Option<Vec<i32>> {
        let mut out = vec![0i32; width];
        for slot in out.iter_mut().rev() {
            *slot = (v % base) as i32;
            v /= base;
        }
        if v == 0 {
            Some(out)
        } else {
            None
        }
    }

    fn msd_string(v: u32, base: u32, width: usize) -> String {
        msd_digits(v, base, width)
            .unwrap()
            .into_iter()
            .map(|d| char::from(b'0' + d as u8))
            .collect()
    }

    // =========================================================================
    // Phase-1 spike tests for `less_than_msd` (unchanged)
    // =========================================================================

    #[test]
    fn less_than_is_deterministic_and_two_states() {
        let a = less_than_msd(2);
        assert_eq!(a.fa.q, 2);
        assert!(a.fa.is_deterministic());
    }

    #[test]
    fn equal_length_representations_hand_cases() {
        // base 2, 3-digit msd-first representations.
        let a = less_than_msd(2);
        // 010 (=2) < 011 (=3): digits equal at pos0 (0,0) and pos1 (1,1), differ at
        // pos2 (0,1) with 0<1.
        assert!(accepts(&a, &[(0, 0), (1, 1), (0, 1)]));
        // 011 (=3) is NOT less than 010 (=2): differ at pos2 with 1 > 0.
        assert!(!accepts(&a, &[(0, 0), (1, 1), (1, 0)]));
        // Identical representations: never less than.
        assert!(!accepts(&a, &[(1, 1), (0, 0), (1, 1)]));
        // 100 (=4) < 101 (=5).
        assert!(accepts(&a, &[(1, 1), (0, 0), (0, 1)]));
    }

    #[test]
    fn base_3_hand_case() {
        let a = less_than_msd(3);
        // 12 (base3, =5) < 20 (base3, =6): first digit 1<2 decides immediately.
        assert!(accepts(&a, &[(1, 2), (2, 0)]));
        // 20 is not < 12.
        assert!(!accepts(&a, &[(2, 1), (0, 2)]));
    }

    #[test]
    fn a_greater_than_b_dead_ends_partial_automaton() {
        // Once a digit proves a > b, state 0 has no outgoing transition for that
        // symbol and the run dies — must not be misread as "eventually true".
        let a = less_than_msd(2);
        let sym_gt = a.encode(&[1, 0]); // a-digit=1 > b-digit=0
        assert!(!a.fa.d[0].contains_key(&sym_gt));
    }

    #[test]
    fn spike_less_than_msd_agrees_with_the_ported_lexicographic_less_than() {
        // The two constructors (see `less_than_msd`'s doc comment) must agree as
        // languages for a contiguous 0..base alphabet. Checked for 2..=5 so a
        // base-specific off-by-one in either can't hide.
        for base in 2..=5 {
            // Both constructions are deliberately PARTIAL (see `less_than_msd`'s doc
            // comment); the equivalence oracle needs total DFAs, so totalize copies
            // first — language-preserving, since the added sink is non-accepting.
            let mut spike = less_than_msd(base).fa;
            let mut ported = lexicographic_less_than(&(0..base).collect::<Vec<_>>(), true).fa;
            spike.totalize(0);
            ported.totalize(0);
            assert!(
                equiv::language_equivalent(&spike, &ported).unwrap(),
                "base {base}"
            );
        }
    }

    // =========================================================================
    // Tier 2: mechanical replications of NumberSystemTest.java
    // =========================================================================

    /// `NumberSystemTest.testBogusNS` (`:46-50`). Java throws
    /// `StringIndexOutOfBoundsException`; this port returns `MalformedName` (see that
    /// variant's doc comment).
    #[test]
    fn bogus_ns_empty_name_is_rejected() {
        assert_eq!(
            NumberSystem::new("").unwrap_err(),
            NumSysError::MalformedName(String::new())
        );
    }

    /// `NumberSystemTest.testLessBogusNS` (`:52-55`).
    #[test]
    fn less_bogus_ns_unknown_base_is_not_defined() {
        assert_eq!(
            NumberSystem::new("msd_BOGUSNS").unwrap_err(),
            NumSysError::NotDefined("msd_BOGUSNS".to_string())
        );
    }

    /// `NumberSystemTest.testMSD7` (`:57-76`).
    #[test]
    fn msd_7_basic_properties_and_determine_msd() {
        let ns = NumberSystem::new("msd_7").unwrap();
        assert!(ns.is_msd());
        assert!(!ns.use_all_representations());
        assert_eq!(ns.name(), "msd_7");
        assert_eq!(ns.parse_base().unwrap(), 7);
        assert_eq!(ns.get_alphabet(), (0..7).collect::<Vec<i32>>());

        // "empty list is probably not by design" -- Java's own comment.
        assert_eq!(determine_msd(&[]), Some(true));
        assert_eq!(determine_msd(&[Some(ns.is_msd())]), Some(true));
        assert_eq!(determine_msd(&[Some(ns.is_msd()), None]), None);
    }

    /// `NumberSystemTest.testMSDFlip` (`:78-104`), adapted to the single-system
    /// [`NumberSystem::flip`] (see its doc comment for why the list-level `flipNS`
    /// isn't re-ported here).
    #[test]
    fn msd_flip_produces_the_opposite_direction_same_base() {
        let ns = NumberSystem::new("msd_5").unwrap();
        assert!(ns.is_msd());
        assert_eq!(ns.name(), "msd_5");

        let flipped = ns.flip().unwrap();

        // "number system doesn't change" -- the original is untouched.
        assert!(ns.is_msd());
        assert_eq!(ns.name(), "msd_5");

        assert!(!flipped.is_msd());
        assert_eq!(flipped.name(), "lsd_5");
        assert_eq!(flipped.determine_base_name_underscore(), LSD_UNDERSCORE);

        let back = flipped.flip().unwrap();
        assert!(back.is_msd());
        assert_eq!(back.name(), "msd_5");
        assert_eq!(back.determine_base_name_underscore(), MSD_UNDERSCORE);
    }

    /// `NumberSystemTest.testLsdOne` (`:171-177`), strengthened. Java only asserts the
    /// alphabet size (its own `// TODO: add more tests here`); a language check is
    /// added because the size assertion alone would pass even if `makeOne` built the
    /// wrong direction's automaton.
    #[test]
    fn lsd_one_constant() {
        let ns = NumberSystem::new("lsd_5").unwrap();
        let one = ns.get_constant(&big(1)).unwrap();
        assert_eq!(one.alphabet[0].len(), 5);
        assert_eq!(one.get_arity(), 1);

        // lsd: the value 1 is "1" followed by any number of TRAILING zeros.
        assert!(accepts_tuples(&one, &single_track("1")));
        assert!(accepts_tuples(&one, &single_track("100")));
        // "01" would be 1 written msd-first -- must NOT be accepted by an lsd system.
        assert!(!accepts_tuples(&one, &single_track("01")));
        assert!(!accepts_tuples(&one, &single_track("0")));
        assert!(!accepts_tuples(&one, &single_track("2")));
    }

    /// `makeZero` (`:1060-1062`) — `0*`, in both directions (it is the one constant
    /// whose regex does not depend on `isMsd`). No Java test covers `getConstant(0)`
    /// directly; without this, mutation-testing showed `make_zero` could self-loop on
    /// digit **1** instead of digit 0 and every other test in this file still passed
    /// (`getConstant(0)` is only reached indirectly, via `division`'s `r >= 0` bound,
    /// which stays satisfiable either way).
    #[test]
    fn constant_zero_is_the_all_zeros_language() {
        for name in ["msd_3", "lsd_3"] {
            let ns = NumberSystem::new(name).unwrap();
            let zero = ns.get_constant(&big(0)).unwrap();
            assert_eq!(zero.get_arity(), 1, "{name}");
            // the track alphabet was widened from the regex's {0,1} to the full base
            assert_eq!(zero.alphabet[0], vec![0, 1, 2], "{name}");
            assert!(accepts_tuples(&zero, &single_track("0")), "{name}");
            assert!(accepts_tuples(&zero, &single_track("000")), "{name}");
            assert!(!accepts_tuples(&zero, &single_track("1")), "{name}");
            assert!(!accepts_tuples(&zero, &single_track("010")), "{name}");
            assert!(!accepts_tuples(&zero, &single_track("2")), "{name}");
        }
    }

    /// The msd counterpart of the test above (no Java analog: `NumberSystemTest` never
    /// checks `msd` `getConstant(1)` directly). Included so a `make_one` that ignored
    /// `is_msd` and always built `0*1` would fail SOMEWHERE — the lsd test above alone
    /// would catch that, but not the reverse mistake.
    #[test]
    fn msd_one_constant_is_the_mirror_image() {
        let ns = NumberSystem::new("msd_5").unwrap();
        let one = ns.get_constant(&big(1)).unwrap();
        assert!(accepts_tuples(&one, &single_track("1")));
        assert!(accepts_tuples(&one, &single_track("001")));
        assert!(!accepts_tuples(&one, &single_track("100")));
        assert!(!accepts_tuples(&one, &single_track("0")));
    }

    /// `NumberSystemTest.testIsNSDifferingAllBranches` (`:183-223`), replicated against
    /// the name-based signature (see [`is_ns_differing`]'s doc comment). The
    /// "distinct instances that share a name" case becomes two equal `&str`s, which is
    /// the same assertion — comparison is by name, not identity.
    #[test]
    fn is_ns_differing_all_branches() {
        let msd2 = Some("msd_2");
        let msd3 = Some("msd_3");
        let alphabet = vec![vec![0, 1]];
        let same_alphabet = vec![vec![0, 1]];
        let other_alphabet = vec![vec![0, 1, 2]];

        // differing sizes short-circuit before the alphabets are even compared
        assert!(is_ns_differing(&[msd2], &[], &alphabet, &same_alphabet));
        // same size, but the alphabets are not equal
        assert!(is_ns_differing(
            &[msd2],
            &[msd2],
            &alphabet,
            &other_alphabet
        ));
        // one entry null, the other not (in both orders)
        assert!(is_ns_differing(&[None], &[msd2], &alphabet, &same_alphabet));
        assert!(is_ns_differing(&[msd2], &[None], &alphabet, &same_alphabet));
        // both entries non-null, but with different names
        assert!(is_ns_differing(&[msd2], &[msd3], &alphabet, &same_alphabet));

        // NOT differing: equal names
        assert!(!is_ns_differing(
            &[msd2],
            &[Some("msd_2")],
            &alphabet,
            &same_alphabet
        ));
        // NOT differing: matching null entries
        assert!(!is_ns_differing(
            &[None],
            &[None],
            &alphabet,
            &same_alphabet
        ));
        // NOT differing: empty lists
        assert!(!is_ns_differing(&[], &[], &alphabet, &same_alphabet));
    }

    /// `NumberSystemTest.testNormalizeNumberSystemToken` (`:229-262`) — every branch,
    /// quirks included.
    #[test]
    fn normalize_number_system_token_all_branches() {
        assert_eq!(normalize_number_system_token(None), "msd_2");
        assert_eq!(normalize_number_system_token(Some("")), "msd_2");
        assert_eq!(normalize_number_system_token(Some("   ")), "msd_2");

        assert_eq!(normalize_number_system_token(Some("?msd_3")), "msd_3");
        assert_eq!(normalize_number_system_token(Some("?lsd")), "lsd_2");
        assert_eq!(
            normalize_number_system_token(Some("  ?msd_fib  ")),
            "msd_fib"
        );

        assert_eq!(normalize_number_system_token(Some("msd")), "msd_2");
        assert_eq!(normalize_number_system_token(Some("lsd")), "lsd_2");

        assert_eq!(normalize_number_system_token(Some("msd_7")), "msd_7");
        assert_eq!(normalize_number_system_token(Some("lsd_fib")), "lsd_fib");

        assert_eq!(normalize_number_system_token(Some("msd3")), "msd_3");
        assert_eq!(normalize_number_system_token(Some("msdfib")), "msd_fib");
        assert_eq!(normalize_number_system_token(Some("lsd3")), "lsd_3");
        assert_eq!(normalize_number_system_token(Some("lsdfib")), "lsd_fib");

        assert_eq!(normalize_number_system_token(Some("fib")), "msd_fib");
        assert_eq!(normalize_number_system_token(Some("10")), "msd_10");

        // QUIRK: a lone "?" survives isEmpty(), then becomes "", so the fall-through
        // produces the (unusable) name "msd_".
        assert_eq!(normalize_number_system_token(Some("?")), "msd_");
    }

    /// `NumberSystemTest.testBaseOfOneIsNotDefinedWithoutACustomBase` (`:268-275`).
    #[test]
    fn base_of_one_is_not_defined() {
        assert_eq!(
            NumberSystem::new("msd_1").unwrap_err(),
            NumSysError::NotDefined("msd_1".to_string())
        );
        // base 0 likewise
        assert_eq!(
            NumberSystem::new("msd_0").unwrap_err(),
            NumSysError::NotDefined("msd_0".to_string())
        );
    }

    /// `NumberSystemTest.testParseBaseRejectsBaseOfOne` (`:277-290`), adapted: Java
    /// needs a `Custom Bases/msd_1_addition.txt` file to construct an `msd_1` at all,
    /// which this port can't do — so the `<= 1` half of `parseBase`'s guard is
    /// exercised through [`parse_base_of`] directly (see its doc comment).
    #[test]
    fn parse_base_rejects_base_of_one_and_non_numbers() {
        assert_eq!(
            parse_base_of("msd_1"),
            Err(NumSysError::InvalidBase("1".to_string()))
        );
        assert_eq!(
            parse_base_of("msd_fib"),
            Err(NumSysError::InvalidBase("fib".to_string()))
        );
        assert_eq!(parse_base_of("lsd_10").unwrap(), 10);
        // `isNumber` is `^\d+$`, so leading zeros parse fine (they do in Java too).
        assert_eq!(parse_base_of("msd_007").unwrap(), 7);
        // ... but a digit string that overflows Java's `int` is a NumberFormatException
        // there and `BaseNotAnI32` here.
        assert_eq!(
            parse_base_of("msd_99999999999"),
            Err(NumSysError::BaseNotAnI32("99999999999".to_string()))
        );
    }

    /// `NumberSystemTest.testArithmetic` (`:128-144`).
    #[test]
    fn arithmetic_rejects_unsupported_operator_shapes() {
        let ns = NumberSystem::new("msd_3").unwrap();
        // Can't divide two variables.
        assert_eq!(
            ns.arithmetic("a", "b", "c", ArithmeticOp::Div).unwrap_err(),
            NumSysError::OperatorTwoVariables("/")
        );
        assert_eq!(
            ns.arithmetic_const_c("a", "b", &big(0), ArithmeticOp::Div)
                .unwrap_err(),
            NumSysError::OperatorTwoVariables("/")
        );
        // Unexpected operation.
        assert_eq!(
            ns.arithmetic("a", "b", "c", ArithmeticOp::UnaryNegative)
                .unwrap_err(),
            NumSysError::UnexpectedArithmeticOperator("_")
        );
        // Division by zero.
        assert_eq!(
            ns.arithmetic_const_b("a", &big(0), "c", ArithmeticOp::Div)
                .unwrap_err(),
            NumSysError::DivisionByZero
        );
        // Constants can't be divided by variables (`:857`).
        assert_eq!(
            ns.arithmetic_const_a(&big(3), "b", "c", ArithmeticOp::Div)
                .unwrap_err(),
            NumSysError::ConstantDividedByVariable
        );
    }

    /// `NumberSystemTest.testMultiplicationOfTwoVariablesAndByZero` (`:514-528`).
    #[test]
    fn multiplication_of_two_variables_and_by_zero() {
        let ns = NumberSystem::new("msd_3").unwrap();
        assert_eq!(
            ns.arithmetic_const_c("a", "b", &big(0), ArithmeticOp::Mult)
                .unwrap_err(),
            NumSysError::OperatorTwoVariables("*")
        );

        let base2 = NumberSystem::new("msd_2").unwrap();
        assert_eq!(
            base2
                .arithmetic_const_b("x", &big(0), "y", ArithmeticOp::Mult)
                .unwrap_err(),
            NumSysError::MultiplicationByZero
        );
    }

    /// Negative constants are rejected in a positive base — the surviving half of
    /// `validateNeg` (see module docs), reached through every entry point that has one.
    #[test]
    fn negative_constants_are_rejected_everywhere_validate_neg_guards() {
        let ns = NumberSystem::new("msd_2").unwrap();
        let neg = big(-5);
        let expected = NumSysError::NegativeConstant("-5".to_string());
        assert_eq!(ns.get_constant(&neg).unwrap_err(), expected);
        assert_eq!(ns.get_multiplication(&neg).unwrap_err(), expected);
        assert_eq!(ns.get_division(&neg).unwrap_err(), expected);
        assert_eq!(
            ns.comparison_const_b("x", &neg, RelationalOp::LessThan)
                .unwrap_err(),
            expected
        );
        assert_eq!(
            ns.comparison_const_a(&neg, "x", RelationalOp::LessThan)
                .unwrap_err(),
            expected
        );
        assert_eq!(
            ns.arithmetic_const_b("x", &neg, "y", ArithmeticOp::Plus)
                .unwrap_err(),
            expected
        );
        assert_eq!(
            ns.arithmetic_const_a(&neg, "x", "y", ArithmeticOp::Plus)
                .unwrap_err(),
            expected
        );
        assert_eq!(
            ns.arithmetic_const_c("x", "y", &neg, ArithmeticOp::Plus)
                .unwrap_err(),
            expected
        );
    }

    /// `NumberSystemTest.testPrivateIntOverloadsOfMultiplicationAndDivision`
    /// (`:417-435`), adapted: the `int` overloads themselves are dead in Java and not
    /// ported (see [`NumberSystem::get_multiplication`]), so this characterizes the
    /// live `BigInteger` forms. "each call hands back a fresh clone of the memoized
    /// automaton" is the assertion that survives translation — checked by mutating one
    /// result and confirming the next call is unaffected.
    #[test]
    fn get_multiplication_and_get_division_hand_back_fresh_clones() {
        let ns = NumberSystem::new("msd_2").unwrap();

        let mut times_three = ns.get_multiplication(&big(3)).unwrap();
        assert_eq!(times_three.get_arity(), 2);
        times_three.fa.o[0] = 7; // corrupt the copy we hold
        let again = ns.get_multiplication(&big(3)).unwrap();
        assert_ne!(again.fa.o[0], 7, "the memoized automaton was aliased");

        let mut divided_by_two = ns.get_division(&big(2)).unwrap();
        assert_eq!(divided_by_two.get_arity(), 2);
        divided_by_two.fa.o[0] = 7;
        let again = ns.get_division(&big(2)).unwrap();
        assert_ne!(again.fa.o[0], 7, "the memoized automaton was aliased");
    }

    /// `NumberSystemTest.testBaseTwoAdditionAutomatonSemantics` (`:534-554`).
    #[test]
    fn base_two_addition_automaton_semantics() {
        let ns = NumberSystem::new("msd_2").unwrap();
        let plus = ns.arithmetic("a", "b", "c", ArithmeticOp::Plus).unwrap();
        assert_eq!(plus.get_arity(), 3);

        // 3 + 5 = 8 : 0011 + 0101 = 1000
        assert!(accepts_digits(
            &plus,
            &[("a", "0011"), ("b", "0101"), ("c", "1000")]
        ));
        // 3 + 5 != 7 : 0111
        assert!(!accepts_digits(
            &plus,
            &[("a", "0011"), ("b", "0101"), ("c", "0111")]
        ));
        // 1 + 1 = 2 : 01 + 01 = 10
        assert!(accepts_digits(
            &plus,
            &[("a", "01"), ("b", "01"), ("c", "10")]
        ));
        // 1 + 1 != 3 : 11 (this one ends in the carry state)
        assert!(!accepts_digits(
            &plus,
            &[("a", "01"), ("b", "01"), ("c", "11")]
        ));
        // leading zeros are harmless
        assert!(accepts_digits(
            &plus,
            &[("a", "000"), ("b", "000"), ("c", "000")]
        ));
    }

    /// `NumberSystemTest.testBaseThreeAdditionAutomatonSemantics` (`:556-569`).
    #[test]
    fn base_three_addition_automaton_semantics() {
        let ns = NumberSystem::new("msd_3").unwrap();
        let plus = ns.arithmetic("a", "b", "c", ArithmeticOp::Plus).unwrap();

        // 2 + 2 = 4 : 02 + 02 = 11
        assert!(accepts_digits(
            &plus,
            &[("a", "02"), ("b", "02"), ("c", "11")]
        ));
        // 2 + 2 != 5 : 12
        assert!(!accepts_digits(
            &plus,
            &[("a", "02"), ("b", "02"), ("c", "12")]
        ));
        // 4 + 4 = 8 : 11 + 11 = 22
        assert!(accepts_digits(
            &plus,
            &[("a", "11"), ("b", "11"), ("c", "22")]
        ));
        // 4 + 4 != 7 : 21
        assert!(!accepts_digits(
            &plus,
            &[("a", "11"), ("b", "11"), ("c", "21")]
        ));
    }

    /// The `MINUS` binding order (`:761`), which no Java test covers directly. `c = a -
    /// b` must NOT be the same automaton as `c = a + b` — an asymmetric fixture (3 - 1
    /// = 2, and 1 - 3 rejected) is used so a port that bound the tracks in the wrong
    /// order would fail.
    #[test]
    fn minus_binds_the_adder_in_the_reversed_order() {
        let ns = NumberSystem::new("msd_2").unwrap();
        let minus = ns.arithmetic("a", "b", "c", ArithmeticOp::Minus).unwrap();
        assert_eq!(minus.get_arity(), 3);
        // 3 - 1 = 2
        assert!(accepts_digits(
            &minus,
            &[("a", "11"), ("b", "01"), ("c", "10")]
        ));
        // 3 - 1 != 1
        assert!(!accepts_digits(
            &minus,
            &[("a", "11"), ("b", "01"), ("c", "01")]
        ));
        // 1 - 3 has no non-negative answer at all
        assert!(!accepts_digits(
            &minus,
            &[("a", "01"), ("b", "11"), ("c", "10")]
        ));
        assert!(!accepts_digits(
            &minus,
            &[("a", "01"), ("b", "11"), ("c", "00")]
        ));
    }

    /// `NumberSystemTest.testBaseTwoConstantFiveSemantics` (`:591-602`).
    #[test]
    fn base_two_constant_five_semantics() {
        let ns = NumberSystem::new("msd_2").unwrap();
        let five = ns.get_constant(&big(5)).unwrap();
        assert_eq!(five.get_arity(), 1);

        assert!(accepts_tuples(&five, &single_track("101"))); // 5
        assert!(accepts_tuples(&five, &single_track("0101"))); // 5, with a leading zero
        assert!(!accepts_tuples(&five, &single_track("111"))); // 7
        assert!(!accepts_tuples(&five, &single_track("100"))); // 4
        assert!(!accepts_tuples(&five, &single_track("01"))); // 1
    }

    /// `NumberSystemTest.testMultiplicationByThreeSemantics` (`:604-616`).
    #[test]
    fn multiplication_by_three_semantics() {
        let ns = NumberSystem::new("msd_2").unwrap();
        let times_three = ns
            .arithmetic_const_b("x", &big(3), "y", ArithmeticOp::Mult)
            .unwrap();
        assert_eq!(times_three.get_arity(), 2);

        assert!(accepts_digits(&times_three, &[("x", "001"), ("y", "011")])); // 1*3=3
        assert!(accepts_digits(&times_three, &[("x", "010"), ("y", "110")])); // 2*3=6
        assert!(!accepts_digits(&times_three, &[("x", "010"), ("y", "111")])); // 2*3!=7
        assert!(!accepts_digits(&times_three, &[("x", "011"), ("y", "110")])); // 3*3!=6
    }

    /// The positive-base analogue of `NumberSystemTest.testNegativeConstantAsTheRightOperand`
    /// (`:441-472`) — that test's own fixtures are all `msd_neg_3`, which is out of
    /// scope, so the SHAPE (constant as the right operand of `arithmetic`) is
    /// replicated over `msd_2` instead. Uses `y = x + 3` and `y = x - 3`, which are
    /// each other's inverse but NOT the same automaton, so a port that confused the two
    /// overloads' operand order would fail.
    #[test]
    fn constant_as_the_right_operand() {
        let ns = NumberSystem::new("msd_2").unwrap();

        let plus_three = ns
            .arithmetic_const_b("x", &big(3), "y", ArithmeticOp::Plus)
            .unwrap();
        assert_eq!(plus_three.get_arity(), 2);
        assert!(accepts_digits(&plus_three, &[("x", "001"), ("y", "100")])); // 1+3=4
        assert!(!accepts_digits(&plus_three, &[("x", "001"), ("y", "011")])); // 1+3!=3
        assert!(accepts_digits(&plus_three, &[("x", "000"), ("y", "011")])); // 0+3=3

        let minus_three = ns
            .arithmetic_const_b("x", &big(3), "y", ArithmeticOp::Minus)
            .unwrap();
        assert!(accepts_digits(&minus_three, &[("x", "100"), ("y", "001")])); // 4-3=1
        assert!(!accepts_digits(&minus_three, &[("x", "001"), ("y", "100")])); // 1-3 != 4
    }

    /// The positive-base analogue of `NumberSystemTest.testConstantAsTheLeftOperand`
    /// (`:474-493`). `y = 3 - x` is deliberately asymmetric so the operand order is
    /// observable.
    #[test]
    fn constant_as_the_left_operand() {
        let ns = NumberSystem::new("msd_2").unwrap();

        let three_plus = ns
            .arithmetic_const_a(&big(3), "x", "y", ArithmeticOp::Plus)
            .unwrap();
        assert_eq!(three_plus.get_arity(), 2);
        assert!(accepts_digits(&three_plus, &[("x", "001"), ("y", "100")])); // 3+1=4
        assert!(!accepts_digits(&three_plus, &[("x", "001"), ("y", "011")]));

        let three_minus = ns
            .arithmetic_const_a(&big(3), "x", "y", ArithmeticOp::Minus)
            .unwrap();
        assert_eq!(three_minus.get_arity(), 2);
        assert!(accepts_digits(&three_minus, &[("x", "001"), ("y", "010")])); // 3-1=2
        assert!(accepts_digits(&three_minus, &[("x", "011"), ("y", "000")])); // 3-3=0
        assert!(accepts_digits(&three_minus, &[("x", "010"), ("y", "001")])); // 3-2=1
        assert!(!accepts_digits(&three_minus, &[("x", "010"), ("y", "010")])); // 3-2!=2
                                                                               // 3 - 5 has no non-negative answer, so nothing works for x=5.
        assert!(!accepts_digits(&three_minus, &[("x", "101"), ("y", "000")]));
        // `3 - x = y` must NOT be `x - 3 = y` (asymmetry check): 5-3=2 is rejected.
        assert!(!accepts_digits(&three_minus, &[("x", "101"), ("y", "010")]));
    }

    /// The positive-base analogue of `NumberSystemTest.testConstantAsTheResult`
    /// (`:495-512`): `x + y = 3`.
    #[test]
    fn constant_as_the_result() {
        let ns = NumberSystem::new("msd_2").unwrap();
        let sum_is_three = ns
            .arithmetic_const_c("x", "y", &big(3), ArithmeticOp::Plus)
            .unwrap();
        assert_eq!(sum_is_three.get_arity(), 2);
        assert!(accepts_digits(&sum_is_three, &[("x", "01"), ("y", "10")])); // 1+2=3
        assert!(accepts_digits(&sum_is_three, &[("x", "11"), ("y", "00")])); // 3+0=3
        assert!(!accepts_digits(&sum_is_three, &[("x", "01"), ("y", "01")])); // 1+1!=3

        let diff_is_one = ns
            .arithmetic_const_c("x", "y", &big(1), ArithmeticOp::Minus)
            .unwrap();
        assert!(accepts_digits(&diff_is_one, &[("x", "11"), ("y", "10")])); // 3-2=1
        assert!(!accepts_digits(&diff_is_one, &[("x", "10"), ("y", "11")])); // 2-3!=1
    }

    /// `division` has no Java unit test beyond the dead-overload characterization, but
    /// it is the most heavily composed construction in the file (two comparisons, one
    /// multiplication, one adder, a two-variable ∃). Checked by hand against
    /// `a / n = b` with truncation.
    #[test]
    fn division_by_three_semantics() {
        let ns = NumberSystem::new("msd_2").unwrap();
        let div_three = ns
            .arithmetic_const_b("x", &big(3), "y", ArithmeticOp::Div)
            .unwrap();
        assert_eq!(div_three.get_arity(), 2);

        assert!(accepts_digits(&div_three, &[("x", "0110"), ("y", "0010")])); // 6/3=2
        assert!(accepts_digits(&div_three, &[("x", "0111"), ("y", "0010")])); // 7/3=2
        assert!(accepts_digits(&div_three, &[("x", "1000"), ("y", "0010")])); // 8/3=2
        assert!(accepts_digits(&div_three, &[("x", "1001"), ("y", "0011")])); // 9/3=3
        assert!(!accepts_digits(&div_three, &[("x", "0111"), ("y", "0011")])); // 7/3!=3
        assert!(accepts_digits(&div_three, &[("x", "0010"), ("y", "0000")])); // 2/3=0
                                                                              // The remainder range is `0 <= r < n`, not `<= n`: `9 = 3*2 + 3` must NOT make
                                                                              // 9/3 = 2 acceptable. (Mutation-tested: relaxing `r < n` to `r <= n` in
                                                                              // `division` survives every other assertion in this file.)
        assert!(!accepts_digits(&div_three, &[("x", "1001"), ("y", "0010")])); // 9/3!=2
        assert!(!accepts_digits(&div_three, &[("x", "0110"), ("y", "0001")])); // 6/3!=1
                                                                               // ... and the lower end, `r >= 0`, is what stops the quotient overshooting.
        assert!(!accepts_digits(&div_three, &[("x", "0110"), ("y", "0011")])); // 6/3!=3
    }

    /// Tier-4-flavoured but deterministic (division is the most expensive construction
    /// here, so this enumerates a small exact table rather than running as a proptest):
    /// `y == x / n` with truncation, for every `x` in range, over two divisors.
    #[test]
    fn division_matches_truncating_integer_division_over_a_small_table() {
        let ns = NumberSystem::new("msd_2").unwrap();
        for n in [2u32, 3u32] {
            let div = ns
                .arithmetic_const_b("x", &BigInt::from(n), "y", ArithmeticOp::Div)
                .unwrap();
            for x in 0u32..12 {
                for y in 0u32..12 {
                    let word_x = msd_string(x, 2, 4);
                    let word_y = msd_string(y, 2, 4);
                    assert_eq!(
                        accepts_digits(&div, &[("x", &word_x), ("y", &word_y)]),
                        y == x / n,
                        "{x} / {n} == {y}?"
                    );
                }
            }
        }
    }

    /// `comparison(String, BigInteger, EQUAL/NOT_EQUAL)`'s two short-circuit arms
    /// (`:705-714`), which return the constant automaton (optionally negated) WITHOUT
    /// ever reaching the `and`/`quantify` tail. No Java test covers them; without this
    /// they would be the only untested branches of `comparison_const_b`.
    #[test]
    fn comparison_against_a_constant_equal_and_not_equal_short_circuit() {
        let ns = NumberSystem::new("msd_2").unwrap();

        let eq_five = ns
            .comparison_const_b("x", &big(5), RelationalOp::Equal)
            .unwrap();
        assert_eq!(eq_five.label, vec!["x".to_string()]);
        assert!(accepts_tuples(&eq_five, &single_track("101")));
        assert!(accepts_tuples(&eq_five, &single_track("0101")));
        assert!(!accepts_tuples(&eq_five, &single_track("100")));

        let ne_five = ns
            .comparison_const_b("x", &big(5), RelationalOp::NotEqual)
            .unwrap();
        assert_eq!(ne_five.label, vec!["x".to_string()]);
        assert!(!accepts_tuples(&ne_five, &single_track("101")));
        assert!(!accepts_tuples(&ne_five, &single_track("0101")));
        assert!(accepts_tuples(&ne_five, &single_track("100")));
        assert!(accepts_tuples(&ne_five, &single_track("000")));
    }

    /// `comparison(BigInteger, String, Ops)` (`:735-738`) — the constant on the LEFT,
    /// which is `comparison_const_b` under a REVERSED relation. `3 < x` and `x < 3`
    /// are deliberately both checked on the same inputs: a port that forgot
    /// `reverse_operator` would compute one when asked for the other, and only a
    /// side-by-side asymmetric fixture catches that.
    #[test]
    fn comparison_with_the_constant_on_the_left() {
        let ns = NumberSystem::new("msd_2").unwrap();

        let three_lt_x = ns
            .comparison_const_a(&big(3), "x", RelationalOp::LessThan)
            .unwrap();
        assert!(accepts_tuples(&three_lt_x, &single_track("100"))); // 3 < 4
        assert!(!accepts_tuples(&three_lt_x, &single_track("011"))); // 3 < 3 is false
        assert!(!accepts_tuples(&three_lt_x, &single_track("010"))); // 3 < 2 is false

        let x_lt_three = ns
            .comparison_const_b("x", &big(3), RelationalOp::LessThan)
            .unwrap();
        assert!(!accepts_tuples(&x_lt_three, &single_track("100"))); // 4 < 3 is false
        assert!(!accepts_tuples(&x_lt_three, &single_track("011"))); // 3 < 3 is false
        assert!(accepts_tuples(&x_lt_three, &single_track("010"))); // 2 < 3

        // GREATER_EQ_THAN with the constant on the left: `3 >= x`, i.e. x <= 3.
        let three_ge_x = ns
            .comparison_const_a(&big(3), "x", RelationalOp::GreaterEqThan)
            .unwrap();
        assert!(accepts_tuples(&three_ge_x, &single_track("011"))); // 3 >= 3
        assert!(accepts_tuples(&three_ge_x, &single_track("010"))); // 3 >= 2
        assert!(!accepts_tuples(&three_ge_x, &single_track("100"))); // 3 >= 4 is false
    }

    /// `lexicographicLessThan`'s `Collections.sort` on a defensive copy (`:418-419`).
    ///
    /// Every alphabet `NumberSystem` itself builds is already the sorted, contiguous
    /// `0..k`, so through the public API that sort is unobservable — deleting it would
    /// break nothing. This calls the ported function directly with the shape Java's own
    /// class javadoc uses as its example ("if the alphabet is {-2,0,7} then in
    /// lexicographic order..."), scrambled, so the sort is load-bearing: with it, the
    /// state-0 "goes to accepting" edges follow VALUE order; without it they would
    /// follow the caller's arbitrary list order.
    #[test]
    fn lexicographic_less_than_sorts_a_scrambled_alphabet_first() {
        let scrambled = [7, -2, 0];
        let a = lexicographic_less_than(&scrambled, true);
        // The track alphabets come out sorted, not in the caller's order.
        assert_eq!(a.alphabet[0], vec![-2, 0, 7]);
        // -2 < 0 < 7 decided on the first digit pair, in VALUE order.
        assert!(accepts(&a, &[(-2, 0), (7, -2)]));
        assert!(accepts(&a, &[(0, 7), (7, -2)]));
        assert!(!accepts(&a, &[(7, 0), (-2, 7)]));
        assert!(!accepts(&a, &[(0, 0), (7, 7)])); // equal throughout
        assert!(accepts(&a, &[(0, 0), (-2, 7)]));
    }

    /// `setEqualityAutomaton` (`:403-409`) deliberately does NOT sort (unlike
    /// `lexicographicLessThan`) — the diagonal `i * size + i` is index-order
    /// independent. Checked directly on a scrambled alphabet, since `NumberSystem`
    /// never supplies one.
    #[test]
    fn equality_automaton_is_the_diagonal_regardless_of_alphabet_order() {
        let scrambled = [7, -2, 0];
        let a = equality_automaton(&scrambled, true);
        assert_eq!(a.alphabet[0], vec![7, -2, 0], "the alphabet is NOT sorted");
        assert!(accepts(&a, &[(7, 7), (-2, -2), (0, 0)]));
        assert!(!accepts(&a, &[(7, 7), (-2, 0)]));
        assert!(!accepts(&a, &[(0, 7)]));
    }

    /// The lsd adder and comparator, hand-checked — the deterministic counterpart of
    /// the `msd_and_lsd_agree_after_reversal` property below (a hand fixture can't be
    /// defeated by an unlucky generator, and mutation-testing showed the property alone
    /// was originally too weak to catch a deleted `if (!isMsd) reverse(...)`).
    ///
    /// lsd base 2: the word is written least-significant digit FIRST, so `1 + 1 = 2` is
    /// `a = "10"`, `b = "10"`, `c = "01"`.
    #[test]
    fn lsd_adder_and_comparator_read_least_significant_digit_first() {
        let ns = NumberSystem::new("lsd_2").unwrap();
        let plus = ns.arithmetic("a", "b", "c", ArithmeticOp::Plus).unwrap();
        assert_eq!(plus.get_arity(), 3);
        // 1 + 1 = 2
        assert!(accepts_digits(
            &plus,
            &[("a", "10"), ("b", "10"), ("c", "01")]
        ));
        // ... and the msd spelling of the same fact must NOT be accepted.
        assert!(!accepts_digits(
            &plus,
            &[("a", "01"), ("b", "01"), ("c", "10")]
        ));
        // 3 + 5 = 8 : lsd "1100" + "1010" + "0001"
        assert!(accepts_digits(
            &plus,
            &[("a", "1100"), ("b", "1010"), ("c", "0001")]
        ));
        // 3 + 5 != 7
        assert!(!accepts_digits(
            &plus,
            &[("a", "1100"), ("b", "1010"), ("c", "1110")]
        ));
        // trailing zeros (the lsd analogue of leading zeros) are harmless
        assert!(accepts_digits(
            &plus,
            &[("a", "1000"), ("b", "1000"), ("c", "0100")]
        ));

        let lt = ns.comparison("p", "q", RelationalOp::LessThan);
        // 2 < 3 : lsd "01" vs "11"
        assert!(accepts_digits(&lt, &[("p", "01"), ("q", "11")]));
        assert!(!accepts_digits(&lt, &[("p", "11"), ("q", "01")]));
        // the msd spelling of 2 < 3 ("10" vs "11") is 1 < 3 read lsd -- still true, so
        // use an asymmetric pair the two directions genuinely disagree on: lsd "10" = 1,
        // lsd "01" = 2, so 1 < 2 holds lsd; read msd those are 2 and 1, so it would not.
        assert!(accepts_digits(&lt, &[("p", "10"), ("q", "01")]));
    }

    /// **This test used to pin the opposite outcome.** Until Phase 3b's L1 every
    /// construction here that composes through ∃-elimination — `getConstant(n)` for
    /// `n >= 2`, comparison/arithmetic against a constant, multiplication, division —
    /// returned `NumSysError::Quantify(UnsupportedLsdFixup)` on an `lsd_k` system,
    /// because `crate::quantify`'s lsd branch was a hard error rather than a call to
    /// `fix_trailing_zeros_problem` (see this module's and `crate::quantify`'s docs).
    /// Only the non-composed constructions (adder, comparator, equality, the constants
    /// `0` and `1`) worked. L1 wired the fixup up; this now checks that the composed
    /// family computes the RIGHT lsd language, not merely that it stopped erroring.
    ///
    /// Every digit string below is least-significant-digit-FIRST, and every expected
    /// value is stated in the comment so a reversed-convention bug cannot hide behind a
    /// self-consistent fixture (the same discipline
    /// `lsd_adder_and_comparator_read_least_significant_digit_first` above uses).
    #[test]
    fn lsd_composed_constructions_compute_the_right_language() {
        let ns = NumberSystem::new("lsd_2").unwrap();
        // Non-composed: fine before L1 as well as after.
        assert!(ns.get_constant(&big(0)).is_ok());
        assert!(ns.get_constant(&big(1)).is_ok());
        assert!(ns.arithmetic("a", "b", "c", ArithmeticOp::Plus).is_ok());

        // --- getConstant(6): the recursive-halving case (6 = 3 + 3, 3 = 1 + 2, ...),
        // three levels of ∃-elimination deep, so every one of them ran the lsd fixup.
        // 6 is msd "110", i.e. lsd "011".
        let six = ns.get_constant(&big(6)).unwrap();
        assert!(accepts_tuples(&six, &single_track("011")));
        // ... closed under TRAILING zeros (the lsd analogue of leading zeros).
        assert!(accepts_tuples(&six, &single_track("0110")));
        assert!(accepts_tuples(&six, &single_track("01100")));
        // ... and NOT under leading ones: "0011" read lsd is 12, not 6.
        assert!(!accepts_tuples(&six, &single_track("0011")));
        // "110" read lsd is 3.
        assert!(!accepts_tuples(&six, &single_track("110")));
        // The msd spelling of 6 ("110") having been rejected above is the sharp check:
        // an lsd system that silently computed the msd constant would accept it.
        assert!(!accepts_tuples(&six, &single_track("111"))); // 7
        assert!(!accepts_tuples(&six, &single_track("101"))); // 5

        // --- multiplication: `y = 3x`, two tracks.
        let times_three = ns
            .arithmetic_const_b("x", &big(3), "y", ArithmeticOp::Mult)
            .unwrap();
        // x = 2 ("0100" lsd), y = 6 ("0110" lsd)
        assert!(accepts_digits(
            &times_three,
            &[("x", "0100"), ("y", "0110")]
        ));
        // x = 2, y = 7 ("1110" lsd) -- not 3*2
        assert!(!accepts_digits(
            &times_three,
            &[("x", "0100"), ("y", "1110")]
        ));
        // x = 0, y = 0
        assert!(accepts_digits(
            &times_three,
            &[("x", "0000"), ("y", "0000")]
        ));
        // x = 5 ("1010" lsd), y = 15 ("1111" lsd)
        assert!(accepts_digits(
            &times_three,
            &[("x", "1010"), ("y", "1111")]
        ));
        // the msd spellings of the same fact must NOT be accepted (5 = "0101" msd,
        // 15 = "1111" msd): read lsd, "0101" is 10 and "1111" is 15, and 3*10 != 15.
        assert!(!accepts_digits(
            &times_three,
            &[("x", "0101"), ("y", "1111")]
        ));

        // --- comparison against a constant >= 2: `x >= 2`, the exact shape
        // `wr_logic::eval`'s own `?lsd_2 x >= 2` regression test evaluates.
        let ge_two = ns
            .comparison_const_b("x", &big(2), RelationalOp::GreaterEqThan)
            .unwrap();
        assert!(!accepts_tuples(&ge_two, &single_track("00"))); // 0
        assert!(!accepts_tuples(&ge_two, &single_track("10"))); // 1
        assert!(accepts_tuples(&ge_two, &single_track("01"))); // 2
        assert!(accepts_tuples(&ge_two, &single_track("11"))); // 3
        assert!(accepts_tuples(&ge_two, &single_track("0100"))); // 2, trailing zeros
        assert!(!accepts_tuples(&ge_two, &single_track("1000"))); // 1, trailing zeros

        // --- division, the deepest composition here (`a/n = b` needs two extra
        // quantified variables plus two range comparisons).
        let div_three = ns
            .arithmetic_const_b("x", &big(3), "y", ArithmeticOp::Div)
            .unwrap();
        // 6/3 = 2 : x = "0110" lsd (6), y = "0100" lsd (2)
        assert!(accepts_digits(&div_three, &[("x", "0110"), ("y", "0100")]));
        // 7/3 = 2 (truncating)
        assert!(accepts_digits(&div_three, &[("x", "1110"), ("y", "0100")]));
        // 7/3 != 3
        assert!(!accepts_digits(&div_three, &[("x", "1110"), ("y", "1100")]));
    }

    /// **Tier-4, and the sharpest available check on `quantify`'s lsd fixup (Phase 3b
    /// L1):** every composed construction's `lsd_k` automaton must accept exactly the
    /// digit-reversal of what its `msd_k` twin accepts — AND must independently agree
    /// with the integer facts, so the property cannot be satisfied by two uniformly
    /// wrong sides. This is the composed-construction analogue of
    /// `msd_and_lsd_agree_after_reversal` (which covers only the non-composed adder and
    /// comparator, neither of which routes through `quantify`).
    ///
    /// Why the correspondence is a legitimate oracle *here* but not for `quantify` in
    /// general: `fixLeadingZerosProblem` and `fixTrailingZerosProblem` are NOT mirror
    /// images (the former left-quotients by `0*` and then closes the result under
    /// prepending zeros, via `zeroReachableStates`'s injected `q0` self-loop; the
    /// latter only right-quotients, adding no transitions — see
    /// `crate::logicalops`'s docs on the asymmetry). So `reverse(quantify_msd(A))` and
    /// `quantify_lsd(reverse(A))` genuinely differ on an arbitrary `A`. They agree on
    /// the automata `NumberSystem` actually quantifies, because those are already
    /// closed under padding on the significant end, which is exactly the regime the
    /// asymmetry does not touch — and it is that regime, not the arbitrary one, that
    /// every real query exercises.
    #[test]
    fn msd_and_lsd_composed_constructions_agree_after_reversal() {
        let width = 5usize;
        for base in [2u32, 3u32] {
            let msd = NumberSystem::new(&format!("msd_{base}")).unwrap();
            let lsd = NumberSystem::new(&format!("lsd_{base}")).unwrap();

            // getConstant(n), for n in the recursive-halving range.
            for n in 2u32..8 {
                let cm = msd.get_constant(&BigInt::from(n)).unwrap();
                let cl = lsd.get_constant(&BigInt::from(n)).unwrap();
                for m in 0u32..12 {
                    let Some(d) = msd_digits(m, base, width) else {
                        continue;
                    };
                    let fwd: Vec<Vec<i32>> = d.iter().map(|&x| vec![x]).collect();
                    let rev: Vec<Vec<i32>> = d.iter().rev().map(|&x| vec![x]).collect();
                    assert_eq!(
                        accepts_tuples(&cm, &fwd),
                        m == n,
                        "msd getConstant({n}) on {m}, base {base}"
                    );
                    assert_eq!(
                        accepts_tuples(&cl, &rev),
                        m == n,
                        "lsd getConstant({n}) on {m}, base {base}"
                    );
                }
            }

            // `y = n*x` and `x >= n`, the two other composed families.
            for n in 2u32..5 {
                let mm = msd
                    .arithmetic_const_b("x", &BigInt::from(n), "y", ArithmeticOp::Mult)
                    .unwrap();
                let ml = lsd
                    .arithmetic_const_b("x", &BigInt::from(n), "y", ArithmeticOp::Mult)
                    .unwrap();
                for x in 0u32..6 {
                    for y in 0u32..12 {
                        let (Some(xd), Some(yd)) =
                            (msd_digits(x, base, width), msd_digits(y, base, width))
                        else {
                            continue;
                        };
                        let build = |a: &Automaton, rev: bool| -> Vec<Vec<i32>> {
                            (0..width)
                                .map(|i| {
                                    let i = if rev { width - 1 - i } else { i };
                                    a.label
                                        .iter()
                                        .map(|l| if l == "x" { xd[i] } else { yd[i] })
                                        .collect()
                                })
                                .collect()
                        };
                        assert_eq!(
                            accepts_tuples(&mm, &build(&mm, false)),
                            y == n * x,
                            "msd {n}*{x}=={y}, base {base}"
                        );
                        assert_eq!(
                            accepts_tuples(&ml, &build(&ml, true)),
                            y == n * x,
                            "lsd {n}*{x}=={y}, base {base}"
                        );
                    }
                }

                let gm = msd
                    .comparison_const_b("x", &BigInt::from(n), RelationalOp::GreaterEqThan)
                    .unwrap();
                let gl = lsd
                    .comparison_const_b("x", &BigInt::from(n), RelationalOp::GreaterEqThan)
                    .unwrap();
                for x in 0u32..12 {
                    let Some(d) = msd_digits(x, base, width) else {
                        continue;
                    };
                    let fwd: Vec<Vec<i32>> = d.iter().map(|&v| vec![v]).collect();
                    let rev: Vec<Vec<i32>> = d.iter().rev().map(|&v| vec![v]).collect();
                    assert_eq!(
                        accepts_tuples(&gm, &fwd),
                        x >= n,
                        "msd {x} >= {n}, base {base}"
                    );
                    assert_eq!(
                        accepts_tuples(&gl, &rev),
                        x >= n,
                        "lsd {x} >= {n}, base {base}"
                    );
                }
            }
        }
    }

    // =========================================================================
    // Tier 4: Walnut-independent property invariants (DESIGN.md §5)
    // =========================================================================

    /// Cheap reference decoder: value of an msd-first digit vector in base `base`.
    fn value_msd(digits: &[i32], base: u32) -> u32 {
        digits.iter().fold(0u32, |acc, &d| acc * base + d as u32)
    }

    proptest! {
        // Size-bounded per CLAUDE.md's superexponential-cost guardrails: tiny bases,
        // small values, few cases.
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// Tier-4 property: **the adder automaton computes real addition.** Both
        /// directions (accepts iff `x + y == z`).
        ///
        /// `exact` forces roughly half the cases to be genuine sums. Without it, a
        /// uniformly-drawn `z` almost never equals `x + y`, so the whole property
        /// degenerates into "this automaton rejects most things" — which a completely
        /// broken (or empty) adder also satisfies. This is not hypothetical: an earlier
        /// version of this generator, drawing `z` freely, was mutation-tested and let a
        /// deleted `if (!isMsd) reverse(...)` step pass.
        #[test]
        fn addition_automaton_computes_real_addition(
            base in 2u32..=4,
            x in 0u32..24,
            y in 0u32..24,
            z_free in 0u32..48,
            exact in any::<bool>(),
        ) {
            let z = if exact { x + y } else { z_free };
            let ns = NumberSystem::new(&format!("msd_{base}")).unwrap();
            let plus = ns.arithmetic("a", "b", "c", ArithmeticOp::Plus).unwrap();
            // A width wide enough for every operand AND the sum.
            let width = 8;
            let (Some(xd), Some(yd), Some(zd)) = (
                msd_digits(x, base, width),
                msd_digits(y, base, width),
                msd_digits(z, base, width),
            ) else { return Ok(()); };
            let word: Vec<Vec<i32>> = (0..width)
                .map(|i| {
                    // The automaton's own track order is (a, b, c) here -- `arithmetic`
                    // binds it that way and nothing re-sorts it.
                    plus.label
                        .iter()
                        .map(|l| match l.as_str() {
                            "a" => xd[i],
                            "b" => yd[i],
                            _ => zd[i],
                        })
                        .collect()
                })
                .collect();
            prop_assert_eq!(accepts_tuples(&plus, &word), x + y == z, "base {} {}+{}={}", base, x, y, z);
        }

        /// Tier-4 property: **the comparator really is the order relation** — all six
        /// relations at once, so a swapped/negated pairing in `comparison`'s dispatch
        /// cannot hide behind a symmetric fixture.
        #[test]
        fn comparison_automata_agree_with_the_integer_order(
            base in 2u32..=4,
            x in 0u32..40,
            y in 0u32..40,
        ) {
            let ns = NumberSystem::new(&format!("msd_{base}")).unwrap();
            let width = 6;
            let (Some(xd), Some(yd)) = (msd_digits(x, base, width), msd_digits(y, base, width))
                else { return Ok(()); };
            for (op, expected) in [
                (RelationalOp::LessThan, x < y),
                (RelationalOp::GreaterThan, x > y),
                (RelationalOp::Equal, x == y),
                (RelationalOp::NotEqual, x != y),
                (RelationalOp::LessEqThan, x <= y),
                (RelationalOp::GreaterEqThan, x >= y),
            ] {
                let cmp = ns.comparison("p", "q", op);
                let word: Vec<Vec<i32>> = (0..width)
                    .map(|i| {
                        cmp.label
                            .iter()
                            .map(|l| if l == "p" { xd[i] } else { yd[i] })
                            .collect()
                    })
                    .collect();
                prop_assert_eq!(
                    accepts_tuples(&cmp, &word), expected,
                    "base {} {:?} on {} vs {}", base, op, x, y
                );
            }
        }

        /// Tier-4 property: **msd and lsd agree after reversal.** The lsd adder and
        /// comparator must accept exactly the digit-reversed words the msd ones accept
        /// — which is the ONLY thing `NumberSystem::new`'s `if (!isMsd) reverse(...)`
        /// step is supposed to do. `exact` forces roughly half the cases to be genuine
        /// sums — see [`addition_automaton_computes_real_addition`]'s note on why a
        /// freely-drawn `z` makes this property vacuous.
        #[test]
        fn msd_and_lsd_agree_after_reversal(
            base in 2u32..=4,
            x in 0u32..20,
            y in 0u32..20,
            z_free in 0u32..40,
            exact in any::<bool>(),
        ) {
            let z = if exact { x + y } else { z_free };
            let msd = NumberSystem::new(&format!("msd_{base}")).unwrap();
            let lsd = NumberSystem::new(&format!("lsd_{base}")).unwrap();
            let width = 7;
            let (Some(xd), Some(yd), Some(zd)) = (
                msd_digits(x, base, width),
                msd_digits(y, base, width),
                msd_digits(z, base, width),
            ) else { return Ok(()); };

            let plus_msd = msd.arithmetic("a", "b", "c", ArithmeticOp::Plus).unwrap();
            let plus_lsd = lsd.arithmetic("a", "b", "c", ArithmeticOp::Plus).unwrap();
            let build = |a: &Automaton, rev: bool| -> Vec<Vec<i32>> {
                (0..width)
                    .map(|i| {
                        let i = if rev { width - 1 - i } else { i };
                        a.label
                            .iter()
                            .map(|l| match l.as_str() {
                                "a" => xd[i],
                                "b" => yd[i],
                                _ => zd[i],
                            })
                            .collect()
                    })
                    .collect()
            };
            prop_assert_eq!(
                accepts_tuples(&plus_lsd, &build(&plus_lsd, true)),
                accepts_tuples(&plus_msd, &build(&plus_msd, false)),
                "adder, base {}", base
            );
            // The reversed adder must still be a REAL adder, not merely "some
            // reversal" -- otherwise this property would hold even if both sides were
            // uniformly wrong.
            prop_assert_eq!(
                accepts_tuples(&plus_lsd, &build(&plus_lsd, true)),
                x + y == z,
                "lsd adder semantics, base {}", base
            );

            let lt_lsd = lsd.comparison("p", "q", RelationalOp::LessThan);
            let build2 = |a: &Automaton, rev: bool| -> Vec<Vec<i32>> {
                (0..width)
                    .map(|i| {
                        let i = if rev { width - 1 - i } else { i };
                        a.label
                            .iter()
                            .map(|l| if l == "p" { xd[i] } else { yd[i] })
                            .collect()
                    })
                    .collect()
            };
            prop_assert_eq!(
                accepts_tuples(&lt_lsd, &build2(&lt_lsd, true)),
                x < y,
                "lsd comparator semantics, base {}", base
            );
        }
    }

    proptest! {
        // `get_constant`/`get_multiplication` are the expensive constructions (each
        // level is a subset construction), so these run on fewer cases still.
        #![proptest_config(ProptestConfig::with_cases(16))]

        /// Tier-4 property: `getConstant(n)` accepts exactly the base-`k`
        /// representations of `n` (leading zeros included) and nothing else. `exact`
        /// forces roughly half the cases to feed `n` itself, so the "accepts" side of
        /// the biconditional is genuinely exercised rather than left to a 1-in-24
        /// coincidence.
        #[test]
        fn get_constant_accepts_exactly_that_value(
            base in 2u32..=3,
            n in 0u32..24,
            m_free in 0u32..24,
            exact in any::<bool>(),
            width in 5usize..8,
        ) {
            let m = if exact { n } else { m_free };
            let ns = NumberSystem::new(&format!("msd_{base}")).unwrap();
            let automaton = ns.get_constant(&BigInt::from(n)).unwrap();
            let Some(md) = msd_digits(m, base, width) else { return Ok(()); };
            let word: Vec<Vec<i32>> = md.iter().map(|&d| vec![d]).collect();
            prop_assert_eq!(
                accepts_tuples(&automaton, &word), m == n,
                "base {} constant {} fed {}", base, n, m
            );
            // And the reference decoder agrees with what we just fed in.
            prop_assert_eq!(value_msd(&md, base), m);
        }

        /// Tier-4 property: `multiplication(n)` accepts `(x, y)` iff `y == n*x`.
        /// `exact` forces roughly half the cases to be genuine products (same reason as
        /// the adder property above).
        #[test]
        fn multiplication_automaton_computes_real_multiplication(
            n in 1u32..8,
            x in 0u32..10,
            y_free in 0u32..40,
            exact in any::<bool>(),
        ) {
            let y = if exact { n * x } else { y_free };
            let base = 2u32;
            let ns = NumberSystem::new("msd_2").unwrap();
            let times_n = ns
                .arithmetic_const_b("x", &BigInt::from(n), "y", ArithmeticOp::Mult)
                .unwrap();
            let width = 7;
            let (Some(xd), Some(yd)) = (msd_digits(x, base, width), msd_digits(y, base, width))
                else { return Ok(()); };
            let word: Vec<Vec<i32>> = (0..width)
                .map(|i| {
                    times_n
                        .label
                        .iter()
                        .map(|l| if l == "x" { xd[i] } else { yd[i] })
                        .collect()
                })
                .collect();
            prop_assert_eq!(accepts_tuples(&times_n, &word), y == n * x, "{}*{}={}", n, x, y);
        }
    }

    /// A non-proptest sanity check that `msd_string` (used by nothing else) and the
    /// hand-written fixtures above agree on digit order — a guard against the whole
    /// test file silently agreeing on a reversed convention.
    #[test]
    fn msd_string_writes_the_most_significant_digit_first() {
        assert_eq!(msd_string(5, 2, 4), "0101");
        assert_eq!(msd_string(6, 3, 3), "020");
        assert_eq!(value_msd(&[0, 1, 0, 1], 2), 5);
    }

    // ================================================================ U5: custom bases

    /// Builds a single-track automaton over `{0, 1}` from `(output, [(digit, dest)])` rows —
    /// the shape `Custom Bases/*.txt` files declare.
    fn one_track_fixture(rows: &[(i32, &[(i32, usize)])]) -> Automaton {
        let q = rows.len();
        let mut fa = Fa {
            true_false: None,
            q0: 0,
            q,
            alphabet_size: 2,
            o: rows.iter().map(|(o, _)| *o).collect(),
            d: vec![BTreeMap::new(); q],
        };
        for (state, (_, edges)) in rows.iter().enumerate() {
            for (digit, dest) in *edges {
                fa.d[state].insert(*digit, vec![*dest]);
            }
        }
        // Track number system left `None`, exactly as the reader leaves a `{0,1}`-declared
        // track (`ParseMethods.parseAlphabetDeclaration`'s `bases.add(null)`); the
        // constructor is what overwrites it.
        Automaton::new(fa, vec![vec![0, 1]], Vec::new(), vec![None])
    }

    /// `walnut-java/Custom Bases/msd_fib.txt`, verbatim: the set of valid Zeckendorf
    /// representations over `{0, 1}` — i.e. the words with no `11` substring.
    fn msd_fib_all_representations() -> Automaton {
        one_track_fixture(&[(1, &[(0, 0), (1, 1)]), (1, &[(0, 0)])])
    }

    /// `walnut-java/Custom Bases/msd_fib_addition.txt`, verbatim (7 states, 3 tracks over
    /// `{0,1}`). Transition rows are the file's `d0 d1 d2 -> dest` lines; the encoded symbol
    /// is `encode([d0, d1, d2])` (track 0 fastest-varying — see `automaton.rs`'s module
    /// docs), which this helper computes rather than hard-coding.
    fn msd_fib_addition() -> Automaton {
        /// One state's `(output, transitions)` row, transitions as
        /// `(digit tuple, destination)` — factored out only to satisfy
        /// `clippy::type_complexity`.
        type Row = (i32, Vec<([i32; 3], usize)>);
        let rows: Vec<Row> = vec![
            (
                1,
                vec![
                    ([0, 0, 0], 0),
                    ([0, 0, 1], 1),
                    ([1, 0, 1], 0),
                    ([0, 1, 1], 0),
                ],
            ),
            (
                0,
                vec![
                    ([0, 0, 0], 2),
                    ([1, 0, 0], 3),
                    ([0, 1, 0], 3),
                    ([1, 1, 0], 4),
                    ([1, 0, 1], 2),
                    ([0, 1, 1], 2),
                    ([1, 1, 1], 3),
                ],
            ),
            (
                0,
                vec![
                    ([1, 0, 0], 2),
                    ([0, 1, 0], 2),
                    ([1, 1, 0], 3),
                    ([1, 1, 1], 2),
                ],
            ),
            (
                0,
                vec![
                    ([0, 0, 0], 1),
                    ([1, 0, 0], 0),
                    ([0, 1, 0], 0),
                    ([1, 0, 1], 1),
                    ([0, 1, 1], 1),
                    ([1, 1, 1], 0),
                ],
            ),
            (
                1,
                vec![
                    ([0, 0, 0], 5),
                    ([0, 0, 1], 6),
                    ([1, 0, 1], 5),
                    ([0, 1, 1], 5),
                ],
            ),
            (0, vec![([0, 0, 1], 0)]),
            (
                1,
                vec![
                    ([0, 0, 0], 3),
                    ([1, 0, 0], 4),
                    ([0, 1, 0], 4),
                    ([0, 0, 1], 2),
                    ([1, 0, 1], 3),
                    ([0, 1, 1], 3),
                    ([1, 1, 1], 4),
                ],
            ),
        ];
        let q = rows.len();
        let alphabet = vec![vec![0, 1], vec![0, 1], vec![0, 1]];
        let mut a = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q,
                alphabet_size: 8,
                o: rows.iter().map(|(o, _)| *o).collect(),
                d: vec![BTreeMap::new(); q],
            },
            alphabet,
            Vec::new(),
            vec![None, None, None],
        );
        for (state, (_, edges)) in rows.iter().enumerate() {
            for (digits, dest) in edges {
                let sym = a.encode(digits);
                a.fa.d[state].insert(sym, vec![*dest]);
            }
        }
        a
    }

    /// The full shipped `msd_fib` base, exactly as `wr-io`/`wr-cli` will hand it over:
    /// `msd_fib_addition.txt` as the main addition file, no `_less_than` file at all (Walnut
    /// ships none — the comparator falls back to lexicographic), `msd_fib.txt` as the
    /// all-representations file.
    fn msd_fib_files() -> CustomBaseFiles {
        CustomBaseFiles {
            addition: CustomBaseCandidates {
                main: Some(msd_fib_addition()),
                complement: None,
            },
            less_than: CustomBaseCandidates::default(),
            all_representations: CustomBaseCandidates {
                main: Some(msd_fib_all_representations()),
                complement: None,
            },
        }
    }

    /// Runs a single-track word (msd-first) through `a`, NFA-style.
    fn accepts_single_track_word(a: &Automaton, digits: &[i32]) -> bool {
        assert_eq!(a.alphabet.len(), 1, "single-track helper");
        let word: Vec<i32> = digits.iter().map(|&d| a.encode(&[d])).collect();
        let mut current: BTreeSet<usize> = [a.fa.q0].into_iter().collect();
        for sym in word {
            let mut next = BTreeSet::new();
            for q in &current {
                if let Some(dests) = a.fa.d[*q].get(&sym) {
                    next.extend(dests.iter().copied());
                }
            }
            current = next;
        }
        current.iter().any(|&q| a.fa.o[q] != 0)
    }

    // ------------------------------------------- loadAutomatonOrNull's fallback logic

    #[test]
    fn custom_base_candidate_names_are_the_two_paths_java_probes() {
        assert_eq!(
            custom_base_candidate_names("msd_fib", UNDERSCORE_ADDITION_AUTOMATON).unwrap(),
            (
                "msd_fib_addition.txt".to_string(),
                "lsd_fib_addition.txt".to_string()
            )
        );
        assert_eq!(
            custom_base_candidate_names("lsd_fib", UNDERSCORE_ADDITION_AUTOMATON).unwrap(),
            (
                "lsd_fib_addition.txt".to_string(),
                "msd_fib_addition.txt".to_string()
            )
        );
        assert_eq!(
            custom_base_candidate_names("lsd_fib", TXT_EXTENSION).unwrap(),
            ("lsd_fib.txt".to_string(), "msd_fib.txt".to_string())
        );
        assert_eq!(
            custom_base_candidate_names("msd_2", UNDERSCORE_LESS_THAN_AUTOMATON).unwrap(),
            (
                "msd_2_less_than.txt".to_string(),
                "lsd_2_less_than.txt".to_string()
            )
        );
        // Same `indexOf('_')` guard as everywhere else in this file.
        assert_eq!(
            custom_base_candidate_names("fib", TXT_EXTENSION),
            Err(NumSysError::MalformedName("fib".to_string()))
        );
    }

    /// `loadAutomatonOrNull`: main file present wins outright and is used **unreversed**.
    #[test]
    fn resolve_prefers_the_main_file_and_leaves_it_alone() {
        let ends_in_one = one_track_fixture(&[(0, &[(0, 0), (1, 1)]), (1, &[])]);
        let resolved = CustomBaseCandidates {
            main: Some(ends_in_one),
            complement: Some(msd_fib_all_representations()),
        }
        .resolve()
        .expect("main file present");
        assert!(accepts_single_track_word(&resolved, &[0, 0, 1]));
        assert!(!accepts_single_track_word(&resolved, &[1, 0]));
    }

    /// **The fallback this unit exists to reproduce.** Only the OPPOSITE direction's file
    /// exists (the real situation for every base `walnut-java` ships: there is a
    /// `msd_fib_addition.txt` and no `lsd_fib_addition.txt`), so Java loads it and applies
    /// `AutomatonLogicalOps.reverse(A, false)`. Checked with a deliberately
    /// NON-reversal-symmetric language, so a "forgot to reverse" implementation fails.
    #[test]
    fn resolve_falls_back_to_the_complement_and_reverses_its_language() {
        let ends_in_one = one_track_fixture(&[(0, &[(0, 0), (1, 1)]), (1, &[])]);
        let resolved = CustomBaseCandidates {
            main: None,
            complement: Some(ends_in_one),
        }
        .resolve()
        .expect("complement file present");
        // Reversal of "ends in 1" is "starts with 1".
        assert!(accepts_single_track_word(&resolved, &[1, 0, 0]));
        assert!(!accepts_single_track_word(&resolved, &[0, 0, 1]));
    }

    #[test]
    fn resolve_returns_none_when_neither_file_exists() {
        assert!(CustomBaseCandidates::default().resolve().is_none());
    }

    /// The whole-constructor version of the fallback: `lsd_fib` supplied ONLY with
    /// `msd_fib*` files (as the complement candidates) still builds, and its adder is the
    /// reversal of the msd one. Also pins that the reverse is applied EXACTLY once — the
    /// `if (!isMsd) reverse(...)` inside `setAdditionAutomaton` must not fire on a
    /// file-loaded adder, or an lsd custom base would be double-reversed back to msd.
    #[test]
    fn lsd_custom_base_built_from_the_msd_files_reverses_exactly_once() {
        let files = CustomBaseFiles {
            addition: CustomBaseCandidates {
                main: None,
                complement: Some(msd_fib_addition()),
            },
            less_than: CustomBaseCandidates::default(),
            all_representations: CustomBaseCandidates {
                main: None,
                complement: Some(msd_fib_all_representations()),
            },
        };
        let lsd = NumberSystem::with_custom_base_files("lsd_fib", files).unwrap();
        assert!(!lsd.is_msd());
        assert!(lsd.use_all_representations());

        let msd = NumberSystem::with_custom_base_files("msd_fib", msd_fib_files()).unwrap();
        // Reversing the lsd adder's language must give the msd one back, not leave it
        // unchanged (which is what a missing OR a doubled reverse would produce).
        let mut round_trip = lsd.addition().clone();
        reverse(&mut round_trip, false);
        let mut got = round_trip.fa.clone();
        got.totalize(0);
        let mut want = msd.addition().fa.clone();
        want.totalize(0);
        assert_eq!(equiv::language_equivalent(&got, &want), Ok(true));

        let mut unreversed = lsd.addition().fa.clone();
        unreversed.totalize(0);
        assert_eq!(
            equiv::language_equivalent(&unreversed, &want),
            Ok(false),
            "the msd_fib adder is not reversal-symmetric, so a no-op fallback would be caught"
        );
    }

    // ------------------------------------------------- the constructor's own wiring

    /// End-to-end reproduction of real Walnut's answer to `eval x "?msd_fib x=x";`, which
    /// writes out exactly `Custom Bases/msd_fib.txt` (verified by running `walnut-java`'s
    /// CLI). That answer is only correct because the constructor applied the
    /// valid-representation restriction to `equality`.
    #[test]
    fn msd_fib_equality_is_exactly_the_valid_representation_language() {
        let ns = NumberSystem::with_custom_base_files("msd_fib", msd_fib_files()).unwrap();
        assert!(ns.use_all_representations());
        assert_eq!(ns.get_alphabet(), &[0, 1]);

        // `?msd_fib x=x`: both tracks bound to the same name, so `bind` merges them.
        let x_equals_x = ns.comparison("x", "x", RelationalOp::Equal);
        assert_eq!(x_equals_x.alphabet.len(), 1, "the two tracks merged");
        for word in [
            vec![],
            vec![0],
            vec![1],
            vec![0, 1],
            vec![1, 0],
            vec![1, 0, 1],
            vec![0, 1, 0, 1],
        ] {
            assert!(
                accepts_single_track_word(&x_equals_x, &word),
                "valid Zeckendorf word {word:?} must be accepted"
            );
        }
        for word in [vec![1, 1], vec![0, 1, 1], vec![1, 1, 0], vec![1, 0, 1, 1]] {
            assert!(
                !accepts_single_track_word(&x_equals_x, &word),
                "word {word:?} contains `11` and is not a valid representation"
            );
        }
    }

    /// Real Walnut's answer to `eval x "?msd_fib ~(x=x)";` is the EMPTY language (verified
    /// against its CLI), not "the words containing `11`". That is `not`'s
    /// `applyAllRepresentations` call doing real work — see the dedicated `logicalops.rs`
    /// test for the same property at the primitive level.
    #[test]
    fn msd_fib_negation_stays_inside_the_valid_representations() {
        let ns = NumberSystem::with_custom_base_files("msd_fib", msd_fib_files()).unwrap();
        let x_equals_x = ns.comparison("x", "x", RelationalOp::Equal);
        let negated = not(x_equals_x.as_dfa()).into_automaton();
        assert!(
            negated.is_empty(),
            "~(x=x) over a fully-restricted base is unsatisfiable"
        );
    }

    /// The constructor installs the restriction on all three defining automata, on every
    /// track, and it survives into everything derived from them.
    #[test]
    fn the_restriction_is_installed_on_every_track_and_propagates() {
        let ns = NumberSystem::with_custom_base_files("msd_fib", msd_fib_files()).unwrap();
        for a in [ns.addition(), ns.less_than(), &ns.equality] {
            assert_eq!(a.all_reps.len(), a.alphabet.len());
            assert!(
                a.all_reps.iter().all(|r| r.is_some()),
                "every track carries the valid-representation automaton"
            );
            assert!(
                a.msd.iter().all(|m| m == &Some(true)),
                "`getNS().set(i, this)` overwrote the file's null number systems"
            );
        }
        // Propagation through `arithmetic` (a clone + bind) and then `and`/`quantify`.
        let sum = ns.arithmetic("p", "q", "r", ArithmeticOp::Plus).unwrap();
        assert!(sum.all_reps.iter().all(|r| r.is_some()));
    }

    /// `applyAllRepresentations`'s label quirk, observed at the one place in the port that
    /// actually triggers it: the constructor applies the restriction to UNBOUND automata, so
    /// `copy(K)` leaves them bound to `randomLabel`'s numeric names. Ported verbatim; every
    /// consumer re-`bind`s first, which is why it is harmless.
    #[test]
    fn the_constructor_leaves_the_defining_automata_numerically_labelled() {
        let ns = NumberSystem::with_custom_base_files("msd_fib", msd_fib_files()).unwrap();
        assert_eq!(ns.addition().label, vec!["0", "1", "2"]);
        assert_eq!(ns.equality.label, vec!["0", "1"]);
        // Without any all-representations file, nothing runs and they stay unbound.
        let plain = NumberSystem::new("msd_2").unwrap();
        assert!(plain.addition().label.is_empty());
        assert!(!plain.use_all_representations());
        assert!(plain.all_representations().is_none());
    }

    /// No `_less_than` file ships for `msd_fib`, so the comparator is the lexicographic one
    /// built over `getAlphabet()` — and it, too, gets the restriction applied.
    #[test]
    fn a_missing_less_than_file_falls_back_to_lexicographic_over_the_loaded_alphabet() {
        let ns = NumberSystem::with_custom_base_files("msd_fib", msd_fib_files()).unwrap();
        assert_eq!(ns.less_than().alphabet, vec![vec![0, 1], vec![0, 1]]);
        // 01 < 10 lexicographically; both are valid representations.
        let lt = ns.comparison("x", "y", RelationalOp::LessThan);
        assert!(accepts_digits(&lt, &[("x", "01"), ("y", "10")]));
        assert!(!accepts_digits(&lt, &[("x", "10"), ("y", "01")]));
        // 011 is not a valid representation, so no comparison involving it holds.
        assert!(!accepts_digits(&lt, &[("x", "011"), ("y", "100")]));
    }

    // ---------------------------------------------------- the validation error paths

    #[test]
    fn a_file_loaded_adder_with_the_wrong_arity_is_a_clean_error() {
        let two_track = Automaton::new(
            Fa {
                true_false: None,
                q0: 0,
                q: 1,
                alphabet_size: 4,
                o: vec![1],
                d: vec![BTreeMap::new()],
            },
            vec![vec![0, 1], vec![0, 1]],
            Vec::new(),
            vec![None, None],
        );
        let files = CustomBaseFiles {
            addition: CustomBaseCandidates {
                main: Some(two_track),
                complement: None,
            },
            ..CustomBaseFiles::default()
        };
        assert_eq!(
            NumberSystem::with_custom_base_files("msd_weird", files).unwrap_err(),
            NumSysError::AdditionInputCount("msd_weird".to_string())
        );
    }

    #[test]
    fn a_file_loaded_adder_missing_digit_one_is_a_clean_error() {
        let mut adder = msd_fib_addition();
        adder.alphabet = vec![vec![0, 2], vec![0, 2], vec![0, 2]];
        adder.setup_encoder();
        let files = CustomBaseFiles {
            addition: CustomBaseCandidates {
                main: Some(adder),
                complement: None,
            },
            ..CustomBaseFiles::default()
        };
        assert_eq!(
            NumberSystem::with_custom_base_files("msd_odd", files).unwrap_err(),
            NumSysError::AdditionAlphabetMissingOne("msd_odd".to_string())
        );
    }

    #[test]
    fn a_file_loaded_adder_with_mismatched_track_alphabets_is_a_clean_error() {
        let mut adder = msd_fib_addition();
        adder.alphabet = vec![vec![0, 1], vec![0, 1], vec![0, 1, 2]];
        adder.setup_encoder();
        let files = CustomBaseFiles {
            addition: CustomBaseCandidates {
                main: Some(adder),
                complement: None,
            },
            ..CustomBaseFiles::default()
        };
        assert_eq!(
            NumberSystem::with_custom_base_files("msd_mix", files).unwrap_err(),
            NumSysError::AdditionAlphabetsDiffer("msd_mix".to_string())
        );
    }

    #[test]
    fn a_file_loaded_comparator_with_the_wrong_arity_is_a_clean_error() {
        let files = CustomBaseFiles {
            addition: CustomBaseCandidates {
                main: Some(msd_fib_addition()),
                complement: None,
            },
            less_than: CustomBaseCandidates {
                // Three tracks where the comparator needs exactly two.
                main: Some(msd_fib_addition()),
                complement: None,
            },
            all_representations: CustomBaseCandidates::default(),
        };
        assert_eq!(
            NumberSystem::with_custom_base_files("msd_fib", files).unwrap_err(),
            NumSysError::LessThanInputCount("msd_fib".to_string())
        );
    }

    #[test]
    fn a_file_loaded_comparator_over_a_different_alphabet_is_a_clean_error() {
        let mut comparator = lexicographic_less_than(&[0, 1, 2], true);
        comparator.msd = vec![None, None];
        let files = CustomBaseFiles {
            addition: CustomBaseCandidates {
                main: Some(msd_fib_addition()),
                complement: None,
            },
            less_than: CustomBaseCandidates {
                main: Some(comparator),
                complement: None,
            },
            all_representations: CustomBaseCandidates::default(),
        };
        assert_eq!(
            NumberSystem::with_custom_base_files("msd_fib", files).unwrap_err(),
            NumSysError::LessThanAlphabetMismatch("msd_fib".to_string())
        );
    }

    // --------------------------------------------------- the `msd_neg_*` U5 decision

    /// U5's decision: a `_neg_` name is an explicit, named "unsupported numeration" error at
    /// NAME-RESOLUTION time — not a silent misparse, and not an attempt to load the
    /// (genuinely present) `Custom Bases/msd_neg_fib*.txt` files.
    #[test]
    fn negative_base_names_are_rejected_by_name_before_any_file_is_consulted() {
        for name in ["msd_neg_fib", "lsd_neg_fib", "msd_neg_3", "lsd_neg_2"] {
            assert_eq!(
                NumberSystem::new(name).unwrap_err(),
                NumSysError::UnsupportedNegativeBase(name.to_string()),
                "{name}"
            );
            // Supplying perfectly good files must NOT rescue it -- the rejection happens
            // first, so the negative-base adder can never be layered under this module's
            // positive-base-only constructions.
            assert_eq!(
                NumberSystem::with_custom_base_files(name, msd_fib_files()).unwrap_err(),
                NumSysError::UnsupportedNegativeBase(name.to_string()),
                "{name}"
            );
        }
    }

    /// Java's own comment on `isNeg` names the false positive its leading underscore
    /// avoids. `msd_renege` must NOT be treated as a negative base — it fails for the
    /// ordinary reason (`"renege"` is not a base this crate can build).
    #[test]
    fn a_name_merely_containing_neg_is_not_a_negative_base() {
        assert_eq!(
            NumberSystem::new("msd_renege").unwrap_err(),
            NumSysError::NotDefined("msd_renege".to_string())
        );
        assert_eq!(
            NumberSystem::new("msd_negative").unwrap_err(),
            NumSysError::NotDefined("msd_negative".to_string())
        );
        // `"neg_fib"` itself is NOT a negative base either: Java's guard is `contains("_neg_")`
        // WITH the leading underscore, and this name has none, so it is an ordinary
        // unrecognized base. (Same reason `msd_renege` above escapes.)
        assert_eq!(
            NumberSystem::new("neg_fib").unwrap_err(),
            NumSysError::NotDefined("neg_fib".to_string())
        );
        // The `_neg_` check runs AFTER `determineMsdOrLsd` (Java's line order, `:135` then
        // `:137`), so a name with no `_` at all still fails the earlier guard.
        assert_eq!(
            NumberSystem::new("fib").unwrap_err(),
            NumSysError::MalformedName("fib".to_string())
        );
    }

    // ------------------------------------------- Ruling 1: `&self` memoization

    /// `PORTING.md`'s Ruling 1: the three dynamic tables are populated through a SHARED,
    /// IMMUTABLE handle, which is what lets `wr-logic` hand one `Rc<NumberSystem>` to every
    /// token in a formula.
    #[test]
    fn the_dynamic_tables_memoize_through_a_shared_immutable_handle() {
        let ns = Rc::new(NumberSystem::new("msd_2").unwrap());
        assert!(ns.constants_dynamic_table.borrow().is_empty());

        let first = ns.get_constant(&big(5)).unwrap();
        let after_first = ns.constants_dynamic_table.borrow().len();
        assert!(
            after_first > 1,
            "the halving recursion caches every intermediate, not just 5"
        );

        // A second handle to the same instance -- no `&mut` anywhere, and no second cache.
        let alias = Rc::clone(&ns);
        let second = alias.get_constant(&big(5)).unwrap();
        assert_eq!(
            ns.constants_dynamic_table.borrow().len(),
            after_first,
            "the second lookup was a cache hit, not a rebuild"
        );

        let mut a = first.fa.clone();
        a.totalize(0);
        let mut b = second.fa.clone();
        b.totalize(0);
        assert_eq!(equiv::language_equivalent(&a, &b), Ok(true));

        // The other two tables, same shape (and `division` recurses through
        // `comparison_const_b`/`arithmetic_const_a`, exercising the nested-borrow case).
        assert!(ns.get_multiplication(&big(3)).is_ok());
        assert!(!ns.multiplications_dynamic_table.borrow().is_empty());
        assert!(ns.get_division(&big(3)).is_ok());
        assert!(!ns.divisions_dynamic_table.borrow().is_empty());
    }

    /// A failed lookup must not poison the cache. (This test's error paths all return
    /// before ever touching the `RefCell`, so it does NOT exercise the
    /// held-borrow-across-recursion hazard — that's covered by
    /// `the_dynamic_tables_memoize_through_a_shared_immutable_handle` below, which forces
    /// real recursion through live `borrow()`/`borrow_mut()` cycles and would panic if the
    /// scoping discipline were wrong.)
    #[test]
    fn a_rejected_lookup_leaves_the_cache_untouched() {
        let ns = NumberSystem::new("msd_2").unwrap();
        assert_eq!(
            ns.get_constant(&big(-1)).unwrap_err(),
            NumSysError::NegativeConstant("-1".to_string())
        );
        assert_eq!(
            ns.get_multiplication(&big(0)).unwrap_err(),
            NumSysError::MultiplicationByZero
        );
        assert_eq!(
            ns.get_division(&big(0)).unwrap_err(),
            NumSysError::DivisionByZero
        );
        assert!(ns.constants_dynamic_table.borrow().is_empty());
        assert!(ns.multiplications_dynamic_table.borrow().is_empty());
        assert!(ns.divisions_dynamic_table.borrow().is_empty());
        // Still usable afterwards -- i.e. nothing is stuck borrowed.
        assert!(ns.get_constant(&big(3)).is_ok());
    }

    /// Every ported `WalnutException` message, verbatim (`Display`, added in U5 for Tier 1's
    /// `error*` fixtures).
    #[test]
    fn error_display_matches_walnuts_message_text() {
        assert_eq!(
            NumSysError::NotDefined("msd_fib".to_string()).to_string(),
            "Number system msd_fib is not defined."
        );
        assert_eq!(
            NumSysError::InvalidBase("1".to_string()).to_string(),
            "Base of automaton's number system must be > 1 and int, found: 1"
        );
        assert_eq!(
            NumSysError::NegativeConstant("-5".to_string()).to_string(),
            "negative constant -5"
        );
        assert_eq!(
            NumSysError::OperatorTwoVariables("*").to_string(),
            "the operator * cannot be applied to two variables"
        );
        assert_eq!(
            NumSysError::UnexpectedArithmeticOperator("_").to_string(),
            "unexpected arithmetic operator:_"
        );
        assert_eq!(
            NumSysError::ConstantDividedByVariable.to_string(),
            "constants cannot be divided by variables"
        );
        assert_eq!(NumSysError::DivisionByZero.to_string(), "division by zero");
        assert_eq!(
            NumSysError::MultiplicationByZero.to_string(),
            "multiplication(0)"
        );
        assert_eq!(
            NumSysError::AdditionInputCount("msd_x".to_string()).to_string(),
            "The addition automaton must have exactly 3 inputs: base msd_x"
        );
        assert_eq!(
            NumSysError::AdditionAlphabetMissingZero("msd_x".to_string()).to_string(),
            "The input alphabet of addition automaton must contain 0: base msd_x"
        );
        assert_eq!(
            NumSysError::AdditionAlphabetMissingOne("msd_x".to_string()).to_string(),
            "The input alphabet of addition automaton must contain 1: base msd_x"
        );
        assert_eq!(
            NumSysError::AdditionAlphabetsDiffer("msd_x".to_string()).to_string(),
            "All 3 inputs of the addition automaton must have the same alphabet: base msd_x"
        );
        assert_eq!(
            NumSysError::LessThanInputCount("msd_x".to_string()).to_string(),
            "_less_than.txt must have exactly 2 inputs: base msd_x"
        );
        assert_eq!(
            NumSysError::LessThanAlphabetMismatch("msd_x".to_string()).to_string(),
            "Inputs of _less_than.txt must have the same alphabet as the alphabet of \
             inputs of _addition.txt : base msd_x"
        );
    }

    // -----------------------------------------------------------------------
    // `RelationalOp::compare` / `ArithmeticOp::arith` (Phase 3a U6, added for
    // `WordAutomaton`'s per-state DFAO output comparison/arithmetic).
    // -----------------------------------------------------------------------

    #[test]
    fn relational_op_compare_every_variant() {
        // `RelationalOperator.compare` (`RelationalOperator.java:183-193`): every
        // variant checked against both a `<`, `=`, and `>` pair.
        let cases: &[(RelationalOp, i32, i32, bool)] = &[
            (RelationalOp::Equal, 3, 3, true),
            (RelationalOp::Equal, 3, 5, false),
            (RelationalOp::NotEqual, 3, 5, true),
            (RelationalOp::NotEqual, 3, 3, false),
            (RelationalOp::LessThan, 3, 5, true),
            (RelationalOp::LessThan, 5, 3, false),
            (RelationalOp::LessThan, 3, 3, false),
            (RelationalOp::GreaterThan, 5, 3, true),
            (RelationalOp::GreaterThan, 3, 5, false),
            (RelationalOp::GreaterThan, 3, 3, false),
            (RelationalOp::LessEqThan, 3, 5, true),
            (RelationalOp::LessEqThan, 3, 3, true),
            (RelationalOp::LessEqThan, 5, 3, false),
            (RelationalOp::GreaterEqThan, 5, 3, true),
            (RelationalOp::GreaterEqThan, 3, 3, true),
            (RelationalOp::GreaterEqThan, 3, 5, false),
        ];
        for &(op, a, b, expected) in cases {
            assert_eq!(op.compare(a, b), expected, "{op:?}({a}, {b})");
        }
    }

    #[test]
    fn relational_op_compare_negative_operands() {
        assert!(RelationalOp::LessThan.compare(-5, -1));
        assert!(!RelationalOp::LessThan.compare(-1, -5));
        assert!(RelationalOp::Equal.compare(-3, -3));
    }

    #[test]
    fn arithmetic_op_arith_plus_minus_mult() {
        assert_eq!(ArithmeticOp::Plus.arith(3, 4), Ok(7));
        assert_eq!(ArithmeticOp::Plus.arith(-3, 4), Ok(1));
        assert_eq!(ArithmeticOp::Minus.arith(3, 4), Ok(-1));
        assert_eq!(ArithmeticOp::Minus.arith(-3, -4), Ok(1));
        assert_eq!(ArithmeticOp::Mult.arith(3, 4), Ok(12));
        assert_eq!(ArithmeticOp::Mult.arith(-3, 4), Ok(-12));
        assert_eq!(ArithmeticOp::Mult.arith(0, 5), Ok(0));
    }

    #[test]
    fn arithmetic_op_arith_div_truncating_case_matches_rust_native_division() {
        // Same-sign operands: floor division and truncating division agree.
        assert_eq!(ArithmeticOp::Div.arith(7, 2), Ok(3));
        assert_eq!(ArithmeticOp::Div.arith(-7, -2), Ok(3));
        assert_eq!(ArithmeticOp::Div.arith(6, 3), Ok(2));
    }

    #[test]
    fn arithmetic_op_arith_div_floors_toward_negative_infinity() {
        // `ArithmeticOperator.arith`'s `DIV` case (`ArithmeticOperator.java:248-252`):
        // floor division, NOT Rust/Java's native truncate-toward-zero. `7 / -2` truncates
        // to `-3` (Rust's `/`) but floors to `-4`.
        assert_eq!(ArithmeticOp::Div.arith(7, -2), Ok(-4));
        assert_eq!(ArithmeticOp::Div.arith(-7, 2), Ok(-4));
        // Sanity: Rust's own `/` truncates, confirming the two differ on this input --
        // this test is pinning FLOOR semantics, not accidentally matching native `/`.
        assert_eq!(7i32 / -2, -3);
        assert_eq!(-7i32 / 2, -3);
    }

    #[test]
    fn arithmetic_op_arith_div_exact_negative_quotient_no_floor_adjustment() {
        // Remainder is zero, so no floor correction applies even though signs differ.
        assert_eq!(ArithmeticOp::Div.arith(-8, 2), Ok(-4));
        assert_eq!(ArithmeticOp::Div.arith(8, -2), Ok(-4));
    }

    #[test]
    fn arithmetic_op_arith_div_by_zero_is_division_by_zero_error() {
        assert_eq!(
            ArithmeticOp::Div.arith(5, 0),
            Err(NumSysError::DivisionByZero)
        );
        assert_eq!(
            ArithmeticOp::Div.arith(0, 0),
            Err(NumSysError::DivisionByZero)
        );
    }

    #[test]
    fn arithmetic_op_arith_unary_negative_is_unexpected_operator() {
        // Unreachable through any real `WordAutomaton` call site (see `arith`'s doc
        // comment) but a live, directly-testable branch, matching
        // `NumberSystemTest.testArithmetic`'s analogous coverage of `NumberSystem::
        // arithmetic`'s own (textually different) `default:` throw.
        assert_eq!(
            ArithmeticOp::UnaryNegative.arith(1, 2),
            Err(NumSysError::UnexpectedOperator("_"))
        );
        assert_eq!(
            NumSysError::UnexpectedOperator("_").to_string(),
            "Unexpected operator:_"
        );
    }

    #[test]
    fn arithmetic_op_arith_i32_overflow_is_a_clean_error_not_a_panic() {
        // `i32::MAX * 2` overflows `i32` but not the `i64` intermediate.
        let err = ArithmeticOp::Mult.arith(i32::MAX, 2).unwrap_err();
        assert!(matches!(err, NumSysError::ArithmeticIntOverflow(_)));

        // `i32::MIN / -1` is the classic two's-complement overflow trap: the
        // mathematically exact result (`2147483648`) doesn't fit `i32`, even though
        // neither Rust's checked `/` nor a native `i32` division would necessarily
        // signal it the same way. Confirms the `i64` intermediate computes the exact
        // value and the final `i32::try_from` catches it.
        let err = ArithmeticOp::Div.arith(i32::MIN, -1).unwrap_err();
        assert!(matches!(err, NumSysError::ArithmeticIntOverflow(_)));
    }

    /// The two `compare` overloads must agree everywhere — they share
    /// `RelationalOp::holds_for`, and this pins that they still do (a reviewer's
    /// standing objection to "two overloads, one behavior" claims).
    #[test]
    fn relational_op_compare_int_and_big_int_agree_on_every_op() {
        let ops = [
            RelationalOp::Equal,
            RelationalOp::NotEqual,
            RelationalOp::LessThan,
            RelationalOp::GreaterThan,
            RelationalOp::LessEqThan,
            RelationalOp::GreaterEqThan,
        ];
        for op in ops {
            for a in [-3i32, -1, 0, 1, 3] {
                for b in [-3i32, -1, 0, 1, 3] {
                    assert_eq!(
                        op.compare(a, b),
                        op.compare_big_int(&BigInt::from(a), &BigInt::from(b)),
                        "{op:?} disagrees on ({a}, {b})"
                    );
                }
            }
        }
    }

    /// The whole reason the `BigInteger` overload exists: values far outside `i32`
    /// must compare exactly rather than overflow (`RelationalOperator.act`'s
    /// constant-folding branch never narrows).
    #[test]
    fn relational_op_compare_big_int_is_exact_far_outside_i32() {
        let huge = BigInt::from(i64::MAX) * BigInt::from(1_000_000);
        let five = BigInt::from(5);
        assert!(RelationalOp::GreaterThan.compare_big_int(&huge, &five));
        assert!(!RelationalOp::LessThan.compare_big_int(&huge, &five));
        assert!(RelationalOp::NotEqual.compare_big_int(&huge, &(&huge + 1)));
        assert!(RelationalOp::Equal.compare_big_int(&huge, &huge.clone()));
    }

    /// `arith_big_int`'s floor-division correction, checked directly (the `i32`
    /// `arith` tests above exercise the same code path through the narrowing wrapper,
    /// but Phase 3a's U9 calls THIS entry point for constant folding).
    #[test]
    fn arithmetic_op_arith_big_int_div_floors_toward_negative_infinity() {
        let cases: [(i64, i64, i64); 8] = [
            (7, 2, 3),
            (-7, 2, -4),
            (7, -2, -4),
            (-7, -2, 3),
            (-8, 2, -4), // exact: no correction even though the signs differ
            (8, -2, -4), // exact
            (0, 5, 0),
            (-1, 5, -1),
        ];
        for (a, b, expected) in cases {
            assert_eq!(
                ArithmeticOp::Div.arith_big_int(&BigInt::from(a), &BigInt::from(b)),
                Ok(BigInt::from(expected)),
                "floor({a} / {b})"
            );
        }
    }

    #[test]
    fn arithmetic_op_arith_big_int_errors_match_the_int_overload() {
        assert_eq!(
            ArithmeticOp::Div.arith_big_int(&BigInt::from(5), &BigInt::from(0)),
            Err(NumSysError::DivisionByZero)
        );
        assert_eq!(
            ArithmeticOp::UnaryNegative.arith_big_int(&BigInt::from(1), &BigInt::from(2)),
            Err(NumSysError::UnexpectedOperator("_"))
        );
    }

    /// Unbounded, unlike [`ArithmeticOp::arith`]: no `ArithmeticIntOverflow`, because
    /// there is no narrowing step (`ArithmeticOperator.act`'s constant folding keeps the
    /// full `BigInteger`).
    #[test]
    fn arithmetic_op_arith_big_int_never_overflows() {
        let big = BigInt::from(i64::MAX);
        assert_eq!(
            ArithmeticOp::Mult.arith_big_int(&big, &big),
            Ok(&big * &big)
        );
        // The same multiplication through the `i32` overload is an overflow error.
        assert!(matches!(
            ArithmeticOp::Mult.arith(i32::MAX, 2),
            Err(NumSysError::ArithmeticIntOverflow(_))
        ));
    }

    #[test]
    fn arithmetic_op_arith_no_overflow_at_i32_boundary_values() {
        // Values that fit exactly at the `i32` boundary must NOT be reported as
        // overflow.
        assert_eq!(ArithmeticOp::Plus.arith(i32::MAX, 0), Ok(i32::MAX));
        assert_eq!(ArithmeticOp::Plus.arith(i32::MIN, 0), Ok(i32::MIN));
        assert_eq!(ArithmeticOp::Minus.arith(0, i32::MAX), Ok(-i32::MAX));
    }
}
