// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! The Brics-dialect regular-expression engine behind Walnut's `reg` command.
//!
//! # What this replaces, and why it is not a one-file transliteration
//!
//! Java's `Automata/FA/BricsConverter.java` is only 166 lines, but it is a *format
//! converter*, not a regex engine: every line of real parsing and automaton
//! construction lives in the external `dk.brics:automaton` jar
//! (`RegExp` 887 LOC, `BasicOperations` 694, `BasicAutomata` 487,
//! `MinimizationOperations` 662). So this module is necessarily fresh Rust code for
//! the *architecture* — there is no single Java file to transliterate — while every
//! observable behavior (the accepted dialect, the exact `IllegalArgumentException`
//! messages and their character positions, the resulting language, and Walnut's own
//! encoding quirks on top) is a mechanical port of what the real
//! `walnut-java` + `dk.brics` pair does. The grammar below is transcribed from
//! `dk.brics.automaton.RegExp` (version 1.12-4, the version `pom.xml` pins) method by
//! method: [`Parser::parse_union_exp`] is `parseUnionExp`, [`Parser::parse_inter_exp`]
//! is `parseInterExp`, and so on down to `parseCharExp`.
//!
//! ## Two deliberate scope reductions (both verified, not assumed)
//!
//! 1. **The AST is evaluated directly over `wr-core`'s dense integer alphabet**, not
//!    over Brics' 16-bit char universe with its interval/`Transition(min,max)`
//!    machinery. This is sound, not a shortcut, because `BricsConverter`
//!    *unconditionally* wraps the user's regex as `"(" + userRegex + ")&[" +
//!    alphabetChars + "]*"` (`BricsConverter.java:47`) — the result is always
//!    intersected with `A*`. Writing `L_Σ(E)` for Brics' language over the full char
//!    universe and `L_A(E)` for this module's language over the alphabet `A`, an
//!    induction over every AST node gives `L_A(E) = L_Σ(E) ∩ A*`:
//!    * leaves (`Char`/`CharRange`/`AnyChar`/`AnyString`/`Str`/`Empty`) restrict to `A`
//!      by construction;
//!    * union/intersection distribute over `∩ A*`;
//!    * concatenation and the repetitions do too, because `A*` is subword-closed
//!      (`uv ∈ A*` implies `u ∈ A*` and `v ∈ A*`);
//!    * and complement, the only genuinely delicate case, works because
//!      `(Σ* \ L) ∩ A* = A* \ (L ∩ A*)`.
//!
//!    So the top-level `& [A]*` becomes a no-op here — but it is still *parsed*, since
//!    it changes the character positions in parse errors and can itself make an
//!    otherwise-fine regex unparseable (see [`Parser`]'s docs and WB-024).
//!
//! 2. **Numerical intervals `<n-m>` are not constructed.** They parse (so that Brics'
//!    own `interval syntax error` / `illegal identifier` diagnostics reproduce exactly),
//!    but building one returns [`RegexError::UnsupportedInterval`]. Justification: on
//!    the `reg` path they are *unreachable* — [`determine_encoded_regex`] rewrites every
//!    bare digit into a private-use character **before** the parser ever sees the string,
//!    so `<1-5>` arrives as `<\u{81}-\u{85}>` and Brics' own
//!    `Integer.parseInt`/`NumberFormatException` path throws `interval syntax error at
//!    position N` first. Confirmed empirically against the real `walnut-java` CLI. The
//!    only way to reach a *constructible* interval is
//!    [`AutomatonDFA::from_regex_over_alphabet`] (Java's
//!    `AutomatonDFA(String, List<Integer>, NumberSystem)`). That constructor **does**
//!    have a real, load-bearing production caller:
//!    `NumberSystem.makeConstant(String regex, int constant)`
//!    (`NumberSystem.java:1068-1078`) calls it directly, and `makeConstant` is in turn
//!    invoked from `makeZero()`/`makeOne()` (`NumberSystem.java:1060-1066`) — core
//!    arithmetic helpers behind every "0"/"1" constant automaton in real Walnut (see
//!    this crate's own `NumberSystem::make_zero`/`make_one`/`make_constant` in
//!    `numsys.rs`, which already document this exact call site from an earlier phase).
//!    It is still safe to skip interval construction, but for a narrower reason than "no
//!    caller": `makeConstant`'s only two call sites (`makeZero`/`makeOne`) always pass
//!    fixed literal regexes — `"0*"` and `"0*1"`/`"10*"` depending on msd/lsd — and
//!    neither literal contains `{n,m}` interval syntax, so the interval-construction path
//!    is never actually *reached* through this real caller, even though the caller itself
//!    is real.
//!
//!    Named automaton references `<identifier>` are likewise parsed and then rejected,
//!    but that is *not* a scope reduction: Walnut always calls `RE.toAutomaton()` with a
//!    `null` automaton map and a `null` provider, so `dk.brics` itself throws
//!    `IllegalArgumentException("'<id>' not found")` there.
//!    [`RegexError::AutomatonNotFound`] carries that exact message.
//!
//!    `{n,m}` repeat counts, which the plan grouped with the two above, ARE implemented:
//!    they are unreachable on the `reg` path for the same digit-rewriting reason (and the
//!    resulting `integer expected at position N` error reproduces exactly — it is the
//!    error WB-024's second half surfaces), but unlike `<n-m>` they cost ~15 lines rather
//!    than a port of `BasicAutomata.makeInterval`, so implementing them removes a
//!    divergence instead of adding one.
//!
//! ## What is reused rather than reimplemented
//!
//! Determinization and minimization are this crate's own already-ported
//! [`crate::determinize::subset_construction`] and [`crate::minimize::minimize`] —
//! Brics' `MinimizationOperations` is deliberately NOT ported. Intersection reuses
//! [`crate::product::cross_product_internal`]. Only the Thompson construction
//! ([`NfaBuilder`]) and the dead-state pruning that mirrors Brics'
//! `Automaton.removeDeadTransitions` are new.
//!
//! The pipeline in [`regex_to_fa`] is exactly Brics' own
//! (`RegExp.toAutomaton` -> `Automaton.minimize()` -> `MinimizationOperations.minimize`,
//! which internally determinizes, totalizes, partition-refines and finally calls
//! `removeDeadTransitions`), so the resulting **state counts** match real Walnut's
//! `"Set from brics:N states"` line, not merely the language. State *numbering* does
//! not and cannot match: Brics numbers states by `new ArrayList<>(M.getStates())` over a
//! `HashSet<State>` whose elements use identity hash codes. (Empirically that ordering
//! is stable across repeated JVM runs of the same command — five runs of a 77-state
//! case produced byte-identical output — so it is a reproducibility hazard on paper
//! only, not a live nondeterminism bug worth a `docs/WALNUT-BUGS.md` entry.)
//!
//! # `docs/WALNUT-BUGS.md` WB-024
//!
//! [`determine_encoded_regex`] ports `Main/Commands/Reg.determineEncodedRegex`
//! verbatim, **including** its collision between `RichAlphabet.encode`'s
//! `List.indexOf` returning `-1` for a digit outside the declared alphabet and
//! `BricsConverter.convertEncodingForBrics`'s `+128` offset. See that function's own
//! docs and WB-024 for the full reproduction.
//!
//! # Character width
//!
//! Everything here works on `u16` code units, exactly Java's `char`: the encoded regex
//! is built by casting an `int` encoding to `char`, which can produce unpaired
//! surrogates that are not valid Rust `char`s at all, and all of Brics' error positions
//! are UTF-16 indices.

use crate::automaton::{Automaton, AutomatonDFA};
use crate::determinize::subset_construction;
use crate::fa::Fa;
use crate::minimize::minimize;
use crate::product::cross_product_internal;
use crate::util::NumberFormatError;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure this module can produce, split by which Java exception type it
/// corresponds to — `Prover` prints the two differently (an uncaught
/// `java.lang.IllegalArgumentException` from inside `dk.brics` shows its class name and
/// a stack trace; a `Main.WalnutException` is caught and printed as a plain message), so
/// a later `wr-cli` unit needs to be able to tell them apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegexError {
    /// `java.lang.IllegalArgumentException` thrown by `dk.brics.automaton.RegExp`'s
    /// parser. The payload is the exception's message, verbatim.
    Brics(String),
    /// `Main.WalnutException`, thrown by `BricsConverter`/`AutomatonDFA`/`Reg` around
    /// the Brics call. The payload is the exception's message, verbatim.
    Walnut(String),
    /// `dk.brics.automaton.RegExp`'s `IllegalArgumentException("'" + s + "' not found")`
    /// for a `<identifier>` reference (`RegExp.java:394-395`) — always thrown in Walnut,
    /// which passes neither an automaton map nor a provider.
    AutomatonNotFound(String),
    /// A `<n-m>` numerical interval reached automaton construction. Not a Java behavior:
    /// this port deliberately does not implement `BasicAutomata.makeInterval` (see the
    /// module docs for why the construction is unreachable from every `walnut-java`
    /// production call path).
    UnsupportedInterval,
    /// `java.lang.NumberFormatException` from `UtilityMethods.parseInt` on an
    /// `i32`-overflowing element of a `[a,b,…]` alphabet vector (`Reg.java:56-59`) —
    /// e.g. `reg r {0,1} "([8888888800])"`. Like [`Self::Brics`] this is an UNCHECKED
    /// Java exception rather than a `WalnutException`, so `Prover` prints it with its
    /// class name and a stack trace; unlike a `WalnutException` it is still caught by
    /// `Prover.readBuffer`'s `catch (RuntimeException)` (`Prover.java:390-392`), so the
    /// session survives — verified against `target/Walnut-all.jar`. Found by Tier-5
    /// fuzzing (Phase 4, U30, finding F1); this port used to `panic!` here, which was
    /// process-fatal and therefore a port defect, not Walnut behavior.
    NumberFormat(NumberFormatError),
}

impl RegexError {
    /// The message text Java would surface, matching the corresponding exception's
    /// `getMessage()` verbatim for the three ported variants.
    pub fn message(&self) -> String {
        match self {
            RegexError::Brics(m) | RegexError::Walnut(m) => m.clone(),
            RegexError::AutomatonNotFound(s) => format!("'{s}' not found"),
            RegexError::UnsupportedInterval => {
                "walnut-rs does not implement dk.brics numerical intervals (<n-m>)".to_string()
            }
            // `NumberFormatException.getMessage()` verbatim.
            RegexError::NumberFormat(e) => e.to_string(),
        }
    }
}

impl std::fmt::Display for RegexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for RegexError {}

// ---------------------------------------------------------------------------
// AST — `dk.brics.automaton.RegExp`'s `Kind` enum + its `make*` smart constructors
// ---------------------------------------------------------------------------

/// One node of a parsed Brics regular expression — `RegExp.Kind` (`RegExp.java:110-145`)
/// with each kind's payload fields inlined into the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegexNode {
    /// `REGEXP_UNION`
    Union(Box<RegexNode>, Box<RegexNode>),
    /// `REGEXP_CONCATENATION`
    Concatenation(Box<RegexNode>, Box<RegexNode>),
    /// `REGEXP_INTERSECTION` (`&`)
    Intersection(Box<RegexNode>, Box<RegexNode>),
    /// `REGEXP_OPTIONAL` (`?`)
    Optional(Box<RegexNode>),
    /// `REGEXP_REPEAT` (`*`)
    Repeat(Box<RegexNode>),
    /// `REGEXP_REPEAT_MIN` (`{n,}`, and `+` via `makeRepeat(e, 1)`)
    RepeatMin(Box<RegexNode>, i32),
    /// `REGEXP_REPEAT_MINMAX` (`{n,m}`, and `{n}` via `min == max`)
    RepeatMinMax(Box<RegexNode>, i32, i32),
    /// `REGEXP_COMPLEMENT` (`~`)
    Complement(Box<RegexNode>),
    /// `REGEXP_CHAR`
    Char(u16),
    /// `REGEXP_CHAR_RANGE` (`a-z` inside a character class)
    CharRange(u16, u16),
    /// `REGEXP_ANYCHAR` (`.`)
    AnyChar,
    /// `REGEXP_EMPTY` (`#`) — the empty *language*, not the empty string.
    Empty,
    /// `REGEXP_STRING` (a `"..."` literal, or adjacent chars fused by
    /// [`make_concatenation`])
    Str(Vec<u16>),
    /// `REGEXP_ANYSTRING` (`@`)
    AnyString,
    /// `REGEXP_AUTOMATON` (`<identifier>`)
    NamedAutomaton(String),
    /// `REGEXP_INTERVAL` (`<n-m>`): `(min, max, digits)`.
    Interval(i32, i32, usize),
}

/// `RegExp.makeConcatenation` (`RegExp.java:558-579`) — including its char/string
/// fusion. Purely a representation optimization in Brics (it never changes the accepted
/// language), ported so this port's AST is node-for-node identical to Brics' and a
/// future `toString`-style debugging aid cannot silently diverge.
fn make_concatenation(exp1: RegexNode, exp2: RegexNode) -> RegexNode {
    fn is_charlike(e: &RegexNode) -> bool {
        matches!(e, RegexNode::Char(_) | RegexNode::Str(_))
    }
    fn text(e: &RegexNode) -> Vec<u16> {
        match e {
            RegexNode::Char(c) => vec![*c],
            RegexNode::Str(s) => s.clone(),
            _ => unreachable!("text() is only called on Char/Str nodes"),
        }
    }
    fn fuse(exp1: &RegexNode, exp2: &RegexNode) -> RegexNode {
        let mut s = text(exp1);
        s.extend(text(exp2));
        RegexNode::Str(s)
    }

    if is_charlike(&exp1) && is_charlike(&exp2) {
        return fuse(&exp1, &exp2);
    }
    match (exp1, exp2) {
        (RegexNode::Concatenation(a1, a2), e2) if is_charlike(&a2) && is_charlike(&e2) => {
            RegexNode::Concatenation(a1, Box::new(fuse(&a2, &e2)))
        }
        (e1, RegexNode::Concatenation(b1, b2)) if is_charlike(&e1) && is_charlike(&b1) => {
            RegexNode::Concatenation(Box::new(fuse(&e1, &b1)), b2)
        }
        (e1, e2) => RegexNode::Concatenation(Box::new(e1), Box::new(e2)),
    }
}

// ---------------------------------------------------------------------------
// Parser — a method-for-method port of `dk.brics.automaton.RegExp`'s recursive descent
// ---------------------------------------------------------------------------

/// `RegExp.INTERSECTION` (`RegExp.java:127`).
pub const FLAG_INTERSECTION: u32 = 0x0001;
/// `RegExp.COMPLEMENT` (`:132`).
pub const FLAG_COMPLEMENT: u32 = 0x0002;
/// `RegExp.EMPTY` (`:137`).
pub const FLAG_EMPTY: u32 = 0x0004;
/// `RegExp.ANYSTRING` (`:142`).
pub const FLAG_ANYSTRING: u32 = 0x0008;
/// `RegExp.AUTOMATON` (`:150`).
pub const FLAG_AUTOMATON: u32 = 0x0010;
/// `RegExp.INTERVAL` (`:155`).
pub const FLAG_INTERVAL: u32 = 0x0020;
/// `RegExp.ALL` (`:160`) — what `new RegExp(String)` uses, and therefore the only value
/// Walnut ever passes (`BricsConverter.java:48`).
pub const FLAG_ALL: u32 = 0xffff;

/// Brics' recursive-descent parser over a UTF-16 buffer.
///
/// Faithful down to the error positions: every `throw new
/// IllegalArgumentException("… at position " + pos)` reports the *code-unit* index into
/// the **wrapped** string `"(" + userRegex + ")&[" + alphabet + "]*"`, not into the
/// user's own regex, which is why a `reg` diagnostic's position looks off by one from
/// what the user typed.
pub struct Parser<'a> {
    b: &'a [u16],
    pos: usize,
    flags: u32,
}

/// ASCII helper: `b.charAt(pos)` compared against a literal.
fn is(c: u16, lit: char) -> bool {
    c == lit as u16
}

impl<'a> Parser<'a> {
    /// `new RegExp(s, syntax_flags)` (`RegExp.java:198-220`) — parse the whole buffer,
    /// requiring it to be fully consumed.
    pub fn parse(b: &'a [u16], flags: u32) -> Result<RegexNode, RegexError> {
        // `RegExp.java:202-203`: the empty string is the empty-string language, not a
        // parse error. Unreachable through either Walnut entry point (both wrap the
        // user's regex in at least `"()&[]*"`), ported anyway.
        if b.is_empty() {
            return Ok(RegexNode::Str(Vec::new()));
        }
        let mut p = Parser { b, pos: 0, flags };
        let e = p.parse_union_exp()?;
        if p.pos < p.b.len() {
            return Err(RegexError::Brics(format!(
                "end-of-string expected at position {}",
                p.pos
            )));
        }
        Ok(e)
    }

    /// `RegExp.peek` (`:696-698`).
    fn peek(&self, s: &str) -> bool {
        self.more() && s.chars().any(|c| is(self.b[self.pos], c))
    }

    /// `RegExp.match` (`:700-708`) — renamed, `match` is a Rust keyword.
    fn eat(&mut self, c: char) -> bool {
        if self.pos >= self.b.len() {
            return false;
        }
        if is(self.b[self.pos], c) {
            self.pos += 1;
            return true;
        }
        false
    }

    /// `RegExp.more` (`:710-712`).
    fn more(&self) -> bool {
        self.pos < self.b.len()
    }

    /// `RegExp.next` (`:714-718`).
    fn next(&mut self) -> Result<u16, RegexError> {
        if !self.more() {
            return Err(RegexError::Brics("unexpected end-of-string".to_string()));
        }
        let c = self.b[self.pos];
        self.pos += 1;
        Ok(c)
    }

    /// `RegExp.check` (`:720-722`).
    fn check(&self, flag: u32) -> bool {
        (self.flags & flag) != 0
    }

    /// `RegExp.parseUnionExp` (`:724-729`).
    fn parse_union_exp(&mut self) -> Result<RegexNode, RegexError> {
        let e = self.parse_inter_exp()?;
        if self.eat('|') {
            return Ok(RegexNode::Union(
                Box::new(e),
                Box::new(self.parse_union_exp()?),
            ));
        }
        Ok(e)
    }

    /// `RegExp.parseInterExp` (`:731-736`).
    fn parse_inter_exp(&mut self) -> Result<RegexNode, RegexError> {
        let e = self.parse_concat_exp()?;
        if self.check(FLAG_INTERSECTION) && self.eat('&') {
            return Ok(RegexNode::Intersection(
                Box::new(e),
                Box::new(self.parse_inter_exp()?),
            ));
        }
        Ok(e)
    }

    /// `RegExp.parseConcatExp` (`:738-743`).
    fn parse_concat_exp(&mut self) -> Result<RegexNode, RegexError> {
        let e = self.parse_repeat_exp()?;
        if self.more() && !self.peek(")|") && (!self.check(FLAG_INTERSECTION) || !self.peek("&")) {
            return Ok(make_concatenation(e, self.parse_concat_exp()?));
        }
        Ok(e)
    }

    /// `RegExp.parseRepeatExp` (`:745-779`).
    fn parse_repeat_exp(&mut self) -> Result<RegexNode, RegexError> {
        let mut e = self.parse_compl_exp()?;
        while self.peek("?*+{") {
            if self.eat('?') {
                e = RegexNode::Optional(Box::new(e));
            } else if self.eat('*') {
                e = RegexNode::Repeat(Box::new(e));
            } else if self.eat('+') {
                e = RegexNode::RepeatMin(Box::new(e), 1);
            } else if self.eat('{') {
                let start = self.pos;
                while self.peek("0123456789") {
                    self.next()?;
                }
                if start == self.pos {
                    return Err(RegexError::Brics(format!(
                        "integer expected at position {}",
                        self.pos
                    )));
                }
                let n = parse_ascii_int(&self.b[start..self.pos]);
                let mut m: i32 = -1;
                if self.eat(',') {
                    let start = self.pos;
                    while self.peek("0123456789") {
                        self.next()?;
                    }
                    if start != self.pos {
                        m = parse_ascii_int(&self.b[start..self.pos]);
                    }
                } else {
                    m = n;
                }
                if !self.eat('}') {
                    return Err(RegexError::Brics(format!(
                        "expected '}}' at position {}",
                        self.pos
                    )));
                }
                if m == -1 {
                    e = RegexNode::RepeatMin(Box::new(e), n);
                } else {
                    e = RegexNode::RepeatMinMax(Box::new(e), n, m);
                }
            }
        }
        Ok(e)
    }

    /// `RegExp.parseComplExp` (`:781-786`).
    fn parse_compl_exp(&mut self) -> Result<RegexNode, RegexError> {
        if self.check(FLAG_COMPLEMENT) && self.eat('~') {
            return Ok(RegexNode::Complement(Box::new(self.parse_compl_exp()?)));
        }
        self.parse_char_class_exp()
    }

    /// `RegExp.parseCharClassExp` (`:788-801`). Note Brics desugars `[^…]` into
    /// `AnyChar & ~(…)` right here — this port keeps that desugaring rather than adding
    /// a negated-class node, so `[^…]` exercises exactly the same intersection and
    /// complement code paths `&`/`~` do.
    fn parse_char_class_exp(&mut self) -> Result<RegexNode, RegexError> {
        if self.eat('[') {
            let mut negate = false;
            if self.eat('^') {
                negate = true;
            }
            let mut e = self.parse_char_classes()?;
            if negate {
                e = RegexNode::Intersection(
                    Box::new(RegexNode::AnyChar),
                    Box::new(RegexNode::Complement(Box::new(e))),
                );
            }
            if !self.eat(']') {
                return Err(RegexError::Brics(format!(
                    "expected ']' at position {}",
                    self.pos
                )));
            }
            return Ok(e);
        }
        self.parse_simple_exp()
    }

    /// `RegExp.parseCharClasses` (`:803-808`).
    fn parse_char_classes(&mut self) -> Result<RegexNode, RegexError> {
        let mut e = self.parse_char_class()?;
        while self.more() && !self.peek("]") {
            e = RegexNode::Union(Box::new(e), Box::new(self.parse_char_class()?));
        }
        Ok(e)
    }

    /// `RegExp.parseCharClass` (`:810-819`), including its "a trailing `-` before `]` is
    /// a literal hyphen" special case (`:813-814`).
    fn parse_char_class(&mut self) -> Result<RegexNode, RegexError> {
        let c = self.parse_char_exp()?;
        if self.eat('-') {
            if self.peek("]") {
                return Ok(RegexNode::Union(
                    Box::new(RegexNode::Char(c)),
                    Box::new(RegexNode::Char('-' as u16)),
                ));
            }
            return Ok(RegexNode::CharRange(c, self.parse_char_exp()?));
        }
        Ok(RegexNode::Char(c))
    }

    /// `RegExp.parseSimpleExp` (`:821-881`).
    fn parse_simple_exp(&mut self) -> Result<RegexNode, RegexError> {
        if self.eat('.') {
            return Ok(RegexNode::AnyChar);
        }
        if self.check(FLAG_EMPTY) && self.eat('#') {
            return Ok(RegexNode::Empty);
        }
        if self.check(FLAG_ANYSTRING) && self.eat('@') {
            return Ok(RegexNode::AnyString);
        }
        if self.eat('"') {
            let start = self.pos;
            while self.more() && !self.peek("\"") {
                self.next()?;
            }
            if !self.eat('"') {
                return Err(RegexError::Brics(format!(
                    "expected '\"' at position {}",
                    self.pos
                )));
            }
            return Ok(RegexNode::Str(self.b[start..self.pos - 1].to_vec()));
        }
        if self.eat('(') {
            if self.eat(')') {
                return Ok(RegexNode::Str(Vec::new()));
            }
            let e = self.parse_union_exp()?;
            if !self.eat(')') {
                return Err(RegexError::Brics(format!(
                    "expected ')' at position {}",
                    self.pos
                )));
            }
            return Ok(e);
        }
        if (self.check(FLAG_AUTOMATON) || self.check(FLAG_INTERVAL)) && self.eat('<') {
            let start = self.pos;
            while self.more() && !self.peek(">") {
                self.next()?;
            }
            if !self.eat('>') {
                return Err(RegexError::Brics(format!(
                    "expected '>' at position {}",
                    self.pos
                )));
            }
            let s: Vec<u16> = self.b[start..self.pos - 1].to_vec();
            let dash = s.iter().position(|&c| is(c, '-'));
            return match dash {
                None => {
                    if !self.check(FLAG_AUTOMATON) {
                        return Err(RegexError::Brics(format!(
                            "interval syntax error at position {}",
                            self.pos - 1
                        )));
                    }
                    // Java keeps the identifier as a `String`; a lone surrogate cannot be
                    // an identifier Walnut would ever resolve (it always throws below), so
                    // lossy decoding here only affects the message text of an error that
                    // is raised either way.
                    Ok(RegexNode::NamedAutomaton(String::from_utf16_lossy(&s)))
                }
                Some(i) => {
                    if !self.check(FLAG_INTERVAL) {
                        return Err(RegexError::Brics(format!(
                            "illegal identifier at position {}",
                            self.pos - 1
                        )));
                    }
                    let interval_error = RegexError::Brics(format!(
                        "interval syntax error at position {}",
                        self.pos - 1
                    ));
                    // `RegExp.java:857-877`'s `try { … } catch (NumberFormatException)`.
                    if i == 0 || i == s.len() - 1 || Some(i) != s.iter().rposition(|&c| is(c, '-'))
                    {
                        return Err(interval_error);
                    }
                    let smin = &s[0..i];
                    let smax = &s[i + 1..];
                    let (Some(mut imin), Some(mut imax)) =
                        (java_parse_int(smin), java_parse_int(smax))
                    else {
                        return Err(interval_error);
                    };
                    let digits = if smin.len() == smax.len() {
                        smin.len()
                    } else {
                        0
                    };
                    if imin > imax {
                        std::mem::swap(&mut imin, &mut imax);
                    }
                    Ok(RegexNode::Interval(imin, imax, digits))
                }
            };
        }
        Ok(RegexNode::Char(self.parse_char_exp()?))
    }

    /// `RegExp.parseCharExp` (`:883-886`) — a backslash escapes exactly one code unit,
    /// with no escape-sequence interpretation whatsoever (`\n` is the letter `n`).
    fn parse_char_exp(&mut self) -> Result<u16, RegexError> {
        self.eat('\\');
        self.next()
    }
}

/// `Integer.parseInt` over a slice already known to be ASCII digits
/// (`RegExp.java:760`/`:767`). Java throws `NumberFormatException` on overflow, which
/// `parseRepeatExp` does not catch — an uncaught exception, exactly like the one this
/// panic produces.
fn parse_ascii_int(digits: &[u16]) -> i32 {
    let s: String = digits.iter().map(|&c| (c as u8) as char).collect();
    s.parse::<i32>()
        .unwrap_or_else(|_| panic!("For input string: \"{s}\""))
}

/// `Integer.parseInt(String)` as used inside `parseSimpleExp`'s interval branch, where
/// a `NumberFormatException` is *caught* and turned into `interval syntax error`
/// (`RegExp.java:875-877`). Returns `None` where Java would throw.
///
/// Java accepts an optional leading `+`/`-` and nothing else; in particular it rejects
/// non-ASCII digits, which is exactly what makes `<1-5>` fail on the `reg` path once
/// [`determine_encoded_regex`] has rewritten `1` and `5` into private-use characters.
fn java_parse_int(digits: &[u16]) -> Option<i32> {
    if digits.is_empty() {
        return None;
    }
    let mut s = String::with_capacity(digits.len());
    for (i, &c) in digits.iter().enumerate() {
        let ch = char::from_u32(c as u32)?;
        let ok = ch.is_ascii_digit() || (i == 0 && (ch == '+' || ch == '-') && digits.len() > 1);
        if !ok {
            return None;
        }
        s.push(ch);
    }
    s.parse::<i32>().ok()
}

// ---------------------------------------------------------------------------
// Thompson construction over the dense integer alphabet
// ---------------------------------------------------------------------------

/// Epsilon label in [`NfaBuilder::trans`]. Real symbols are `0..alphabet_size`.
const EPSILON: i32 = -1;

/// A Thompson fragment: one entry state, one exit state.
#[derive(Debug, Clone, Copy)]
struct Frag {
    start: usize,
    accept: usize,
}

/// An arena of epsilon-NFA states, plus the char-to-dense-symbol map the AST is
/// evaluated against (see the module docs for why evaluating over `A` rather than over
/// Brics' full char universe is exact, not approximate).
struct NfaBuilder {
    /// `trans[q]` = outgoing `(symbol, destination)` edges; `symbol == EPSILON` for an
    /// epsilon edge.
    trans: Vec<Vec<(i32, usize)>>,
    /// UTF-16 code unit -> dense symbol index, in ascending code-unit order.
    symbol_of: BTreeMap<u16, i32>,
    alphabet_size: usize,
}

impl NfaBuilder {
    fn new(symbol_of: BTreeMap<u16, i32>, alphabet_size: usize) -> Self {
        NfaBuilder {
            trans: Vec::new(),
            symbol_of,
            alphabet_size,
        }
    }

    fn new_state(&mut self) -> usize {
        self.trans.push(Vec::new());
        self.trans.len() - 1
    }

    fn edge(&mut self, from: usize, sym: i32, to: usize) {
        self.trans[from].push((sym, to));
    }

    /// The empty *language* (`BasicAutomata.makeEmpty`): an accept state no path can
    /// reach.
    fn empty_language(&mut self) -> Frag {
        let start = self.new_state();
        let accept = self.new_state();
        Frag { start, accept }
    }

    /// The empty *string* (`BasicAutomata.makeEmptyString`).
    fn empty_string(&mut self) -> Frag {
        let start = self.new_state();
        let accept = self.new_state();
        self.edge(start, EPSILON, accept);
        Frag { start, accept }
    }

    /// One transition per symbol in `syms` (a possibly-empty set — an empty set is the
    /// empty language, which is what a `Char`/`CharRange` outside the alphabet becomes).
    fn symbol_set(&mut self, syms: &[i32]) -> Frag {
        let start = self.new_state();
        let accept = self.new_state();
        for &s in syms {
            self.edge(start, s, accept);
        }
        Frag { start, accept }
    }

    fn concat(&mut self, a: Frag, b: Frag) -> Frag {
        self.edge(a.accept, EPSILON, b.start);
        Frag {
            start: a.start,
            accept: b.accept,
        }
    }

    fn union(&mut self, a: Frag, b: Frag) -> Frag {
        let start = self.new_state();
        let accept = self.new_state();
        self.edge(start, EPSILON, a.start);
        self.edge(start, EPSILON, b.start);
        self.edge(a.accept, EPSILON, accept);
        self.edge(b.accept, EPSILON, accept);
        Frag { start, accept }
    }

    fn star(&mut self, a: Frag) -> Frag {
        let start = self.new_state();
        let accept = self.new_state();
        self.edge(start, EPSILON, a.start);
        self.edge(start, EPSILON, accept);
        self.edge(a.accept, EPSILON, a.start);
        self.edge(a.accept, EPSILON, accept);
        Frag { start, accept }
    }

    fn optional(&mut self, a: Frag) -> Frag {
        let start = self.new_state();
        let accept = self.new_state();
        self.edge(start, EPSILON, a.start);
        self.edge(start, EPSILON, accept);
        self.edge(a.accept, EPSILON, accept);
        Frag { start, accept }
    }

    /// Splices an already-built [`Fa`] (the result of a complement or an intersection,
    /// both of which have to leave the epsilon-NFA world to be computed) back into this
    /// arena as a single-entry/single-exit fragment.
    fn splice(&mut self, fa: &Fa) -> Frag {
        let base = self.trans.len();
        for _ in 0..fa.q {
            self.new_state();
        }
        let accept = self.new_state();
        for q in 0..fa.q {
            for (&sym, dests) in &fa.d[q] {
                for &dest in dests {
                    self.edge(base + q, sym, base + dest);
                }
            }
            if fa.is_accepting(q) {
                self.edge(base + q, EPSILON, accept);
            }
        }
        Frag {
            start: base + fa.q0,
            accept,
        }
    }

    /// Evaluates one AST node into a fragment of this arena.
    ///
    /// # Resource note (matches Java, deliberately not guarded)
    ///
    /// `RepeatMin`/`RepeatMinMax` unroll their operand `min`/`max` times, so a regex like
    /// `0{2000000000}` builds two billion copies. `BasicOperations.repeat`'s
    /// `while (min-- > 0) as.add(a)` does exactly the same and dies the same way, so no
    /// cap is imposed here — it would be an undeclared divergence. Unreachable through
    /// the `reg` command in any case (its digits are rewritten before the parser runs, so
    /// `{n,m}` never parses); reachable only through
    /// [`AutomatonDFA::from_regex_over_alphabet`] — which DOES have a real production
    /// caller (`NumberSystem::make_constant`, see the module docs above), but that
    /// caller's only two literal regexes (`"0*"`, `"0*1"`/`"10*"`) never contain `{n,m}`,
    /// so this branch stays unreached through it either way.
    /// `CLAUDE.md`'s test-performance guardrails apply to this module's own tests, which
    /// keep every repeat count in single digits.
    fn build(&mut self, node: &RegexNode) -> Result<Frag, RegexError> {
        match node {
            RegexNode::Union(a, b) => {
                let fa = self.build(a)?;
                let fb = self.build(b)?;
                Ok(self.union(fa, fb))
            }
            RegexNode::Concatenation(a, b) => {
                let fa = self.build(a)?;
                let fb = self.build(b)?;
                Ok(self.concat(fa, fb))
            }
            RegexNode::Intersection(a, b) => {
                let fa = self.build_fa(a)?;
                let fb = self.build_fa(b)?;
                let prod = intersect_fa(&fa, &fb, self.alphabet_size);
                Ok(self.splice(&prod))
            }
            RegexNode::Optional(a) => {
                let f = self.build(a)?;
                Ok(self.optional(f))
            }
            RegexNode::Repeat(a) => {
                let f = self.build(a)?;
                Ok(self.star(f))
            }
            RegexNode::RepeatMin(a, min) => {
                // `BasicOperations.repeat(a, min)` (`:185-196`): `a^min · a*`, with
                // `min == 0` short-circuiting to plain `a*`.
                if *min <= 0 {
                    let f = self.build(a)?;
                    return Ok(self.star(f));
                }
                let mut acc = self.build(a)?;
                for _ in 1..*min {
                    let next = self.build(a)?;
                    acc = self.concat(acc, next);
                }
                let tail = self.build(a)?;
                let tail = self.star(tail);
                Ok(self.concat(acc, tail))
            }
            RegexNode::RepeatMinMax(a, min, max) => {
                // `BasicOperations.repeat(a, min, max)` (`:203-232`): `min > max` is the
                // empty language; otherwise `a^min · (a?)^(max-min)`, which is exactly
                // `⋃_{k=min..max} a^k`.
                if min > max {
                    return Ok(self.empty_language());
                }
                let min_clamped = (*min).max(0);
                let mut acc = if min_clamped == 0 {
                    self.empty_string()
                } else {
                    let mut acc = self.build(a)?;
                    for _ in 1..min_clamped {
                        let next = self.build(a)?;
                        acc = self.concat(acc, next);
                    }
                    acc
                };
                for _ in min_clamped..*max {
                    let next = self.build(a)?;
                    let next = self.optional(next);
                    acc = self.concat(acc, next);
                }
                Ok(acc)
            }
            RegexNode::Complement(a) => {
                let inner = self.build_fa(a)?;
                let comp = complement_fa(&inner, self.alphabet_size);
                Ok(self.splice(&comp))
            }
            RegexNode::Char(c) => {
                let syms: Vec<i32> = self.symbol_of.get(c).copied().into_iter().collect();
                Ok(self.symbol_set(&syms))
            }
            RegexNode::CharRange(from, to) => {
                // `BasicAutomata.makeCharRange` (`:106-118`) guards the transition with
                // `if (min <= max)`, so an inverted range like `[2-0]` is the empty
                // language rather than an error (confirmed against the real CLI:
                // `reg r {0,1,2} "[2-0]"` yields the 1-state empty automaton). Rust's
                // `BTreeMap::range` would panic on the inverted bounds instead.
                let syms: Vec<i32> = if from <= to {
                    self.symbol_of.range(*from..=*to).map(|(_, &s)| s).collect()
                } else {
                    Vec::new()
                };
                Ok(self.symbol_set(&syms))
            }
            RegexNode::AnyChar => {
                let syms: Vec<i32> = self.symbol_of.values().copied().collect();
                Ok(self.symbol_set(&syms))
            }
            RegexNode::Empty => Ok(self.empty_language()),
            RegexNode::Str(s) => {
                let mut acc = self.empty_string();
                for c in s {
                    let syms: Vec<i32> = self.symbol_of.get(c).copied().into_iter().collect();
                    let next = self.symbol_set(&syms);
                    acc = self.concat(acc, next);
                }
                Ok(acc)
            }
            RegexNode::AnyString => {
                let syms: Vec<i32> = self.symbol_of.values().copied().collect();
                let f = self.symbol_set(&syms);
                Ok(self.star(f))
            }
            RegexNode::NamedAutomaton(s) => Err(RegexError::AutomatonNotFound(s.clone())),
            RegexNode::Interval(..) => Err(RegexError::UnsupportedInterval),
        }
    }

    /// Builds `node` in a FRESH arena and returns it as an epsilon-free [`Fa`]. Used by
    /// the two operators that cannot stay in the epsilon-NFA world (`&` needs a product,
    /// `~` needs a determinization), mirroring how Brics' own `toAutomaton` recursion
    /// hands each operand a self-contained `Automaton`.
    fn build_fa(&self, node: &RegexNode) -> Result<Fa, RegexError> {
        let mut b = NfaBuilder::new(self.symbol_of.clone(), self.alphabet_size);
        let frag = b.build(node)?;
        Ok(b.to_fa(frag))
    }

    /// Epsilon-elimination: the reachable part of `frag` as an [`Fa`] NFA whose state
    /// `q` accepts iff `frag.accept` is in `q`'s epsilon closure.
    fn to_fa(&self, frag: Frag) -> Fa {
        // Reachable states, in BFS order from `frag.start` (so state 0 is the start).
        let mut index: HashMap<usize, usize> = HashMap::new();
        let mut order: Vec<usize> = Vec::new();
        let mut queue: VecDeque<usize> = VecDeque::new();
        index.insert(frag.start, 0);
        order.push(frag.start);
        queue.push_back(frag.start);
        while let Some(q) = queue.pop_front() {
            for &(_, dest) in &self.trans[q] {
                if let std::collections::hash_map::Entry::Vacant(slot) = index.entry(dest) {
                    slot.insert(order.len());
                    order.push(dest);
                    queue.push_back(dest);
                }
            }
        }

        let mut o: Vec<i32> = Vec::with_capacity(order.len());
        let mut d: Vec<BTreeMap<i32, Vec<usize>>> = Vec::with_capacity(order.len());
        for &q in &order {
            let closure = self.epsilon_closure(q);
            o.push(i32::from(closure.contains(&frag.accept)));
            let mut row: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
            for &r in &closure {
                for &(sym, dest) in &self.trans[r] {
                    if sym == EPSILON {
                        continue;
                    }
                    row.entry(sym).or_default().push(index[&dest]);
                }
            }
            for dests in row.values_mut() {
                dests.sort_unstable();
                dests.dedup();
            }
            d.push(row);
        }

        Fa {
            true_false: None,
            q0: 0,
            q: order.len(),
            alphabet_size: self.alphabet_size,
            o,
            d,
        }
    }

    fn epsilon_closure(&self, q: usize) -> BTreeSet<usize> {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut stack = vec![q];
        seen.insert(q);
        while let Some(cur) = stack.pop() {
            for &(sym, dest) in &self.trans[cur] {
                if sym == EPSILON && seen.insert(dest) {
                    stack.push(dest);
                }
            }
        }
        seen
    }
}

/// Language intersection of two NFAs over the same dense alphabet, via this crate's
/// already-ported cross-product BFS. No determinization is needed: the product of two
/// NFAs already accepts the intersection.
fn intersect_fa(a: &Fa, b: &Fa, alphabet_size: usize) -> Fa {
    // `ProductStrategies`'s flat `(a_sym, b_sym) -> product symbol` table. Here both
    // operands read the SAME single track, so only the diagonal is legal.
    let mut all_inputs =
        vec![crate::product::NOT_SAME_INPUT_IN_BOTH; alphabet_size * alphabet_size];
    for s in 0..alphabet_size {
        all_inputs[s * alphabet_size + s] = s as i32;
    }
    cross_product_internal(a, b, alphabet_size, &all_inputs, |x, y| {
        i32::from(x != 0 && y != 0)
    })
}

/// Language complement of an NFA over the dense alphabet — determinize, totalize, flip
/// (`BasicOperations.complement`, `:242-249`, minus its `removeDeadTransitions` tail,
/// which is a size optimization the final [`regex_to_fa`] pass performs anyway).
fn complement_fa(a: &Fa, alphabet_size: usize) -> Fa {
    let initial: BTreeSet<usize> = [a.q0].into_iter().collect();
    let mut dfa = subset_construction(a, &initial);
    debug_assert_eq!(dfa.alphabet_size, alphabet_size);
    // Totalization is what makes the flip below a complement rather than a no-op: every
    // MISSING transition is a rejected word that must become an accepted one.
    dfa.totalize(0);
    for o in dfa.o.iter_mut() {
        *o = i32::from(*o == 0);
    }
    dfa
}

/// `Automaton.removeDeadTransitions` + `Automaton.reduce` (`Automaton.java:456-467`,
/// `:366-380`): keep exactly the states that are both reachable from `q0` and able to
/// reach an accepting state — plus `q0` itself, which Brics' `reduce()` never drops even
/// when it is dead (that is why the empty language comes back as a **one-state**
/// automaton with no transitions, matching real Walnut's `Set from brics:1 states`).
///
/// Deliberately not [`crate::trim::trim`], which is the port of a *different* Java class
/// (`Trimmer`) and, for a wholly dead automaton, substitutes a canonical fully
/// self-looping sink instead of the transition-free single state Brics produces.
fn remove_dead_states(fa: &Fa) -> Fa {
    let forward = forward_reachable(fa);
    let backward = co_reachable_to_accepting(fa);
    let keep: Vec<usize> = (0..fa.q)
        .filter(|&s| (forward[s] && backward[s]) || s == fa.q0)
        .collect();
    let old_to_new: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new_id, &old_id)| (old_id, new_id))
        .collect();

    let d: Vec<BTreeMap<i32, Vec<usize>>> = keep
        .iter()
        .map(|&old| {
            let mut row: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
            for (&sym, dests) in &fa.d[old] {
                let mapped: Vec<usize> = dests
                    .iter()
                    // A transition into a state that is NOT live is dropped even if its
                    // head survived as `q0` — matching `removeDeadTransitions`, which
                    // filters on `live.contains(t.to)`, not on `keep`.
                    .filter(|&&dest| forward[dest] && backward[dest])
                    .map(|dest| old_to_new[dest])
                    .collect();
                if !mapped.is_empty() {
                    row.insert(sym, mapped);
                }
            }
            row
        })
        .collect();

    Fa {
        true_false: None,
        q0: old_to_new[&fa.q0],
        q: keep.len(),
        alphabet_size: fa.alphabet_size,
        o: keep.iter().map(|&old| fa.o[old]).collect(),
        d,
    }
}

fn forward_reachable(fa: &Fa) -> Vec<bool> {
    let mut seen = vec![false; fa.q];
    if fa.q == 0 {
        return seen;
    }
    let mut stack = vec![fa.q0];
    seen[fa.q0] = true;
    while let Some(q) = stack.pop() {
        for dests in fa.d[q].values() {
            for &dest in dests {
                if !seen[dest] {
                    seen[dest] = true;
                    stack.push(dest);
                }
            }
        }
    }
    seen
}

fn co_reachable_to_accepting(fa: &Fa) -> Vec<bool> {
    let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); fa.q];
    for q in 0..fa.q {
        for dests in fa.d[q].values() {
            for &dest in dests {
                predecessors[dest].push(q);
            }
        }
    }
    let mut seen = vec![false; fa.q];
    let mut stack: Vec<usize> = Vec::new();
    for (q, reached) in seen.iter_mut().enumerate() {
        if fa.is_accepting(q) {
            *reached = true;
            stack.push(q);
        }
    }
    while let Some(q) = stack.pop() {
        for &p in &predecessors[q] {
            if !seen[p] {
                seen[p] = true;
                stack.push(p);
            }
        }
    }
    seen
}

/// Parses `regex` and builds the minimal partial DFA of its language over the dense
/// alphabet described by `symbol_of` (UTF-16 code unit -> symbol index).
///
/// The pipeline mirrors `RegExp.toAutomaton()` followed by `Automaton.minimize()`:
/// Thompson construction, subset construction, totalization, partition refinement, and
/// finally dead-state removal — so the state count matches Brics', not only the
/// language.
pub fn regex_to_fa(
    regex: &[u16],
    symbol_of: BTreeMap<u16, i32>,
    alphabet_size: usize,
) -> Result<Fa, RegexError> {
    let ast = Parser::parse(regex, FLAG_ALL)?;
    let mut builder = NfaBuilder::new(symbol_of, alphabet_size);
    let frag = builder.build(&ast)?;
    let nfa = builder.to_fa(frag);
    let initial: BTreeSet<usize> = [nfa.q0].into_iter().collect();
    let mut dfa = subset_construction(&nfa, &initial);
    debug_assert_eq!(dfa.alphabet_size, alphabet_size);
    dfa.totalize(0);
    let minimal = minimize(&dfa).expect(
        "subset_construction + totalize always yields a total, q0-reachable DFA -- \
         minimize's two documented preconditions",
    );
    Ok(remove_dead_states(&minimal))
}

// ---------------------------------------------------------------------------
// `Automata/FA/BricsConverter.java`
// ---------------------------------------------------------------------------

/// `BricsConverter.convertEncodingForBrics` (`:158-165`): offset an encoded input vector
/// by 128 to dodge dk.brics' reserved characters, then narrow to a `char`.
///
/// The `as u16` narrowing is Java's `(char) vectorEncoding` int-to-char cast, which
/// truncates rather than range-checks — load-bearing for WB-024, where a **negative**
/// encoding lands back inside the reserved range instead of above it.
pub fn convert_encoding_for_brics(vector_encoding: i32) -> u16 {
    vector_encoding.wrapping_add(128) as u16
}

/// `BricsConverter.validateBricsAlphabetSize` (`:151-156`).
fn validate_brics_alphabet_size(alphabet_size: usize) -> Result<(), RegexError> {
    const MAX_BRICS_CHARACTER: usize = (1 << 16) - 1;
    if alphabet_size > MAX_BRICS_CHARACTER {
        return Err(RegexError::Walnut(format!(
            "size of input alphabet exceeds the limit of {MAX_BRICS_CHARACTER}"
        )));
    }
    Ok(())
}

/// `BricsConverter.buildBricsAutomaton` (`:46-52`) — wraps the user's regex as
/// `"(" + regex + ")&[" + internal + "]*"`.
///
/// This wrapping is *not* cosmetic: it is what makes evaluating over the dense alphabet
/// exact (module docs), it shifts every parse-error position by one, and — because the
/// user's regex is spliced into a larger expression rather than parsed standalone — it
/// turns some standalone-valid regexes into parse errors and vice versa.
fn wrap_with_alphabet(regex: &[u16], internal: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(regex.len() + internal.len() + 5);
    out.push('(' as u16);
    out.extend_from_slice(regex);
    out.push(')' as u16);
    out.push('&' as u16);
    out.push('[' as u16);
    out.extend_from_slice(internal);
    out.push(']' as u16);
    out.push('*' as u16);
    out
}

/// `BricsConverter.convertFromBrics` (`:20-44`) — the single-track, digits-as-characters
/// entry point: the alphabet must be a subset of `{0,…,9}`, each digit is its own
/// character in the regex, and the resulting symbol is the digit's *index* in
/// `alphabet` (so `reg r {2,4,1} "4*"` produces transitions on symbol 1, not 4).
///
/// Java's `keyMapper` drops any transition whose character is outside the alphabet
/// (`alphabet.contains(digit) ? … : -1`); here that never arises, because a character
/// outside the alphabet has no dense symbol and contributes no transition in the first
/// place.
pub fn convert_from_brics(alphabet: &[i32], regular_expression: &str) -> Result<Fa, RegexError> {
    let mut internal: Vec<u16> = Vec::with_capacity(alphabet.len());
    for &x in alphabet {
        if !(0..=9).contains(&x) {
            return Err(RegexError::Walnut(
                "the input alphabet of an automaton generated from a regular expression must be a subset of {0,1,...,9}"
                    .to_string(),
            ));
        }
        internal.push(u16::from(b'0') + x as u16);
    }

    let mut symbol_of: BTreeMap<u16, i32> = BTreeMap::new();
    for (i, &x) in alphabet.iter().enumerate() {
        // `List.indexOf` returns the FIRST occurrence, so a duplicated digit keeps its
        // earliest index (`AutomatonDFA`'s constructor de-duplicates first, but
        // `convertFromBrics` itself is a public entry point that does not).
        symbol_of
            .entry(u16::from(b'0') + x as u16)
            .or_insert(i as i32);
    }

    let regex: Vec<u16> = regular_expression.encode_utf16().collect();
    let wrapped = wrap_with_alphabet(&regex, &internal);
    regex_to_fa(&wrapped, symbol_of, alphabet.len())
}

/// `BricsConverter.setFromBricsAutomaton` (`:54-80`) — the multi-track entry point the
/// `reg` command actually uses. Every already-encoded input vector `x` is represented by
/// the character `128 + x`, and the `-128` offset Java re-applies afterwards
/// (`addOffsetToInputs(fa, -128)`) is folded into the dense symbol map here.
///
/// Java's `if (!M.isDeterministic()) throw WalnutException.bricsNFA()` guard
/// (`:68-69`) is unreachable — `Automaton.minimize()` always leaves the automaton
/// deterministic — and equally unreachable here, where [`regex_to_fa`] ends in
/// [`crate::minimize::minimize`]. Kept as a debug assertion rather than a live throw so
/// the (dead) branch is not silently dropped.
pub fn set_from_brics_automaton(
    alphabet_size: usize,
    regular_expression: &[u16],
) -> Result<Fa, RegexError> {
    validate_brics_alphabet_size(alphabet_size)?;

    let mut internal: Vec<u16> = Vec::with_capacity(alphabet_size);
    let mut symbol_of: BTreeMap<u16, i32> = BTreeMap::new();
    for x in 0..alphabet_size {
        let next_char = convert_encoding_for_brics(x as i32);
        internal.push(next_char);
        symbol_of.insert(next_char, x as i32);
    }

    let wrapped = wrap_with_alphabet(regular_expression, &internal);
    let fa = regex_to_fa(&wrapped, symbol_of, alphabet_size)?;
    debug_assert!(
        fa.is_deterministic(),
        "cannot set an automaton of type Automaton to a non-deterministic automaton of type dk.bricks.automaton.Automaton"
    );
    Ok(fa)
}

// ---------------------------------------------------------------------------
// `Main/Commands/Reg.determineEncodedRegex` — the dialect-mangling step (WB-024)
// ---------------------------------------------------------------------------

/// `RichAlphabet.determineEncoder` (`RichAlphabet.java:99-107`), as a free function —
/// [`Automaton`]'s own encoder is private and, more importantly, its
/// `Automaton::encode` *panics* on a digit outside the track's alphabet, which is
/// exactly the case WB-024 needs to survive.
fn determine_encoder(alphabet: &[Vec<i32>]) -> Vec<i32> {
    let mut encoder = Vec::with_capacity(alphabet.len());
    let mut val: i32 = 1;
    for track in alphabet {
        encoder.push(val);
        val = val
            .checked_mul(track.len() as i32)
            .expect("encoder overflow (Math.multiplyExact equivalent)");
    }
    encoder
}

/// `RichAlphabet.encode(List<Integer> l, List<List<Integer>> A, IntList encoder)`
/// (`RichAlphabet.java:109-115`) — **with Java's `List.indexOf` semantics preserved**,
/// i.e. a digit that is not in its track's alphabet contributes `encoder[i] * -1`
/// instead of raising anything.
///
/// # `docs/WALNUT-BUGS.md` WB-024 — ported verbatim, deliberately not fixed
///
/// This is the first half of WB-024. `Reg.determineEncodedRegex` feeds the result
/// straight into [`convert_encoding_for_brics`], whose `+128` offset exists precisely to
/// lift every encoding above dk.brics' reserved ASCII range — an invariant that only
/// holds for **non-negative** encodings. An out-of-alphabet digit produces a negative
/// encoding, which lands back *inside* the reserved range and is then parsed as regex
/// syntax. `reg foo {0,1,2,3} {0,1} "[9,9][0,0]"` and `reg foo {0,1,2,3} {0,1}
/// "[0,0][9,9]"` therefore behave completely differently — the first silently yields the
/// empty language, the second throws `integer expected at position 3`, purely from the
/// order the two vectors appear in.
fn encode_with_index_of(digits: &[i32], alphabet: &[Vec<i32>], encoder: &[i32]) -> i32 {
    let mut encoding: i32 = 0;
    for (i, &d) in digits.iter().enumerate() {
        let index = alphabet[i]
            .iter()
            .position(|&v| v == d)
            .map_or(-1, |p| p as i32);
        let term = encoder[i]
            .checked_mul(index)
            .expect("encode overflow (Math.multiplyExact equivalent)");
        encoding = encoding
            .checked_add(term)
            .expect("encode overflow (Math.addExact equivalent)");
    }
    encoding
}

/// `Reg.determineEncodedRegex` (`Reg.java:42-76`): rewrite every alphabet vector
/// (`[a,b,…]`) and every bare digit in the user's regex into the single private-use
/// character that stands for its encoded input vector, then strip all whitespace.
///
/// This is what turns a multi-track regex like `"[1,0][0,1][0,0]*"` into something
/// dk.brics — which only knows about characters — can parse at all, and it is why
/// `<n-m>`/`{n,m}` are unreachable through the `reg` command (their digits get rewritten
/// before the parser ever runs; see the module docs).
///
/// Ported verbatim including WB-024 (see [`encode_with_index_of`]) and including the
/// ordering quirk in the last step: whitespace is stripped **after** substitution, so a
/// replacement character that happens to be a whitespace code unit (reachable only via
/// WB-024's negative encodings — e.g. an encoding of `-119` yields a literal TAB) is
/// deleted along with the user's own spaces.
///
/// # Errors
///
/// [`RegexError::Walnut`] if a bracketed vector's arity does not match the number of
/// declared tracks (`Reg.java:60-62`); [`RegexError::NumberFormat`] if one of that
/// vector's elements overflows `i32` (see [`parse_set_elements`]).
pub fn determine_encoded_regex(
    baseexp: &str,
    alphabet: &[Vec<i32>],
) -> Result<Vec<u16>, RegexError> {
    let input_length = alphabet.len();
    let encoder = determine_encoder(alphabet);
    let units: Vec<u16> = baseexp.encode_utf16().collect();

    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut i = 0usize;
    while i < units.len() {
        let Some(end) = match_alphabet_vector(&units, i) else {
            out.push(units[i]);
            i += 1;
            continue;
        };
        let mut vector = &units[i..end];
        // `Reg.java:51-53`: truncate the surrounding brackets, if any.
        if is(vector[0], '[') {
            vector = &vector[1..vector.len() - 1];
        }
        let l = parse_set_elements(vector)?;
        if l.len() != input_length {
            return Err(RegexError::Walnut(
                "Mismatch between vector length in regex and specified number of inputs to automaton"
                    .to_string(),
            ));
        }
        out.push(convert_encoding_for_brics(encode_with_index_of(
            &l, alphabet, &encoder,
        )));
        i = end;
    }

    // `sb.toString().replaceAll("\\s", "")`. Java's `\s` is the six ASCII whitespace
    // characters only (no `UNICODE_CHARACTER_CLASS` flag is set), NOT Unicode
    // whitespace — a real difference from Rust's `char::is_whitespace`.
    out.retain(|&c| !is_java_regex_space(c));
    Ok(out)
}

/// Java's `\s` character class: `[ \t\n\x0B\f\r]`.
fn is_java_regex_space(c: u16) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D)
}

fn is_ascii_digit_unit(c: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&c)
}

/// `Reg.RE_FOR_AN_ALPHABET_VECTOR`
/// (`"(\\[(\\s*(\\+|\\-)?\\s*\\d+)(\\s*,\\s*(\\+|\\-)?\\s*\\d+)*\\s*\\])|(\\d)"`),
/// as a hand-written scanner anchored at `at`: returns the end offset of the match, or
/// `None` if the pattern does not match starting exactly there.
///
/// Hand-written rather than delegating to `regex-automata` on purpose. `wr-logic`'s
/// `Cargo.toml` records the workspace ruling that the `regex-automata` dependency stays
/// out of `wr-core`; this pattern is small, has no alternation ambiguity that needs
/// real backtracking (each `\s*` is followed by a character class disjoint from
/// whitespace), and Java's `\d`/`\s` are ASCII-only where Rust's are Unicode — so a
/// direct scanner is both dependency-free and less likely to diverge than a
/// hand-translated pattern.
fn match_alphabet_vector(units: &[u16], at: usize) -> Option<usize> {
    if at >= units.len() {
        return None;
    }
    if !is(units[at], '[') {
        // `|(\d)` — the second alternative.
        return is_ascii_digit_unit(units[at]).then_some(at + 1);
    }

    let mut p = at + 1;
    // `(\s*(\+|\-)?\s*\d+)`
    p = match_signed_number(units, p)?;
    // `(\s*,\s*(\+|\-)?\s*\d+)*`
    loop {
        let mut q = skip_spaces(units, p);
        if q < units.len() && is(units[q], ',') {
            q += 1;
            match match_signed_number(units, q) {
                Some(r) => {
                    p = r;
                    continue;
                }
                // The group's iteration failed after consuming the comma; Java's engine
                // backtracks out of the whole iteration, leaving `p` where it was.
                None => break,
            }
        }
        break;
    }
    // `\s*\]`
    let p = skip_spaces(units, p);
    (p < units.len() && is(units[p], ']')).then_some(p + 1)
}

fn skip_spaces(units: &[u16], mut p: usize) -> usize {
    while p < units.len() && is_java_regex_space(units[p]) {
        p += 1;
    }
    p
}

/// `\s*(\+|\-)?\s*\d+` anchored at `p`.
fn match_signed_number(units: &[u16], p: usize) -> Option<usize> {
    let mut p = skip_spaces(units, p);
    if p < units.len() && (is(units[p], '+') || is(units[p], '-')) {
        p += 1;
    }
    p = skip_spaces(units, p);
    let start = p;
    while p < units.len() && is_ascii_digit_unit(units[p]) {
        p += 1;
    }
    (p > start).then_some(p)
}

/// `Prover.PAT_FOR_A_SINGLE_ELEMENT_OF_A_SET` (`Prover.java:103-104`,
/// `"(\\+|\\-)?\\s*\\d+"`) applied with `Matcher.find()` in a loop, followed by
/// `UtilityMethods.parseInt` on each match (`Reg.java:56-59`).
///
/// # Errors
///
/// [`RegexError::NumberFormat`] if a matched element overflows `i32`. The pattern is a
/// purely syntactic `\d+`, so it constrains the element's *shape* and not its
/// magnitude — `reg r {0,1} "([8888888800])"` reaches this from raw user input, and
/// [`crate::util::try_parse_int`] (not the panicking `parse_int`) is what makes it a
/// recoverable command failure here, exactly as Java's caught `NumberFormatException`
/// is one there.
fn parse_set_elements(units: &[u16]) -> Result<Vec<i32>, RegexError> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < units.len() {
        // Leftmost match: try with a sign first (Java's greedy optional group), then
        // without — the optional `(\+|\-)?` backtracks to "absent" if the rest fails.
        let with_sign = if is(units[i], '+') || is(units[i], '-') {
            let after = skip_spaces(units, i + 1);
            let mut p = after;
            while p < units.len() && is_ascii_digit_unit(units[p]) {
                p += 1;
            }
            (p > after).then_some(p)
        } else {
            None
        };
        let m = with_sign.or_else(|| {
            let mut p = i;
            while p < units.len() && is_ascii_digit_unit(units[p]) {
                p += 1;
            }
            (p > i).then_some(p)
        });
        match m {
            Some(end) => {
                let text = String::from_utf16_lossy(&units[i..end]);
                out.push(crate::util::try_parse_int(&text).map_err(RegexError::NumberFormat)?);
                i = end;
            }
            None => i += 1,
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// `Automata/AutomatonDFA.java`'s two regex constructors
// ---------------------------------------------------------------------------

impl AutomatonDFA {
    /// `AutomatonDFA(String regularExpression, List<Integer> alphabet, NumberSystem
    /// numSys)` (`AutomatonDFA.java:47-59`) — a single-track automaton over an alphabet
    /// of decimal digits, e.g. `"01*"` over `[0,1,2]`.
    ///
    /// `msd` stands in for Java's `NumberSystem` argument, matching this crate's
    /// existing per-track `Option<bool>` representation (`Some(true)` = msd,
    /// `Some(false)` = lsd, `None` = Java's `null` number system). This constructor DOES
    /// have a real production caller — `NumberSystem.makeConstant`, via `makeZero()`/
    /// `makeOne()` (see the module docs) — though that caller's two literal regexes never
    /// exercise the interval-construction path this module deliberately did not
    /// implement. It is also ported because `AutomatonDFATest`/`AutomatonExpressionTest`/
    /// `WordTest` exercise it directly, and because it is the only path on which a raw
    /// digit character survives to the parser.
    ///
    /// # Errors
    ///
    /// [`RegexError::Walnut`] for an empty alphabet (`:49`) or one containing something
    /// outside `{0,…,9}` (`BricsConverter.java:24-26`); [`RegexError::Brics`] for a
    /// parse failure.
    pub fn from_regex_over_alphabet(
        regular_expression: &str,
        alphabet: &[i32],
        msd: Option<bool>,
    ) -> Result<AutomatonDFA, RegexError> {
        if alphabet.is_empty() {
            return Err(RegexError::Walnut(
                "empty alphabet is not accepted".to_string(),
            ));
        }
        // "The alphabet is a set and does not allow repeated elements. However, the user
        // might enter the command reg myreg {1,1,0,0,0} "10*";" (`:50-53`).
        let mut alphabet = alphabet.to_vec();
        crate::util::remove_duplicates(&mut alphabet);

        let fa = convert_from_brics(&alphabet, regular_expression)?;
        let automaton = Automaton::new(fa, vec![alphabet], Vec::new(), vec![msd]);
        // `requireDfaStorage()` (`:58`).
        Ok(AutomatonDFA::from(automaton))
    }

    /// `AutomatonDFA(String regularExpression, Integer alphabetSize)`
    /// (`AutomatonDFA.java:63-67`) **plus** the alphabet/number-system installation its
    /// only caller, `Reg.reg` (`Reg.java:33-36`), performs on the result immediately
    /// afterwards.
    ///
    /// Java splits this in two because `setFromBricsAutomaton` needs only the alphabet
    /// *size*: the constructor leaves `richAlphabet.A` empty and `fa.alphabetSize` at
    /// its `new FA()` default, and `Reg` then calls `setA`/`determineAlphabetSize`. This
    /// port takes the track list up front and derives the size from it, so the
    /// intermediate object — an `Automaton` carrying real transitions over an alphabet
    /// it claims is empty — never exists. Same end state, one fewer invalid
    /// intermediate; called out here rather than left silent because it is a deliberate
    /// deviation from the Java call shape.
    ///
    /// `regular_expression` is already-encoded UTF-16, i.e. the output of
    /// [`determine_encoded_regex`].
    ///
    /// # Errors
    ///
    /// [`RegexError::Walnut`] if the alphabet exceeds dk.brics' 16-bit character limit;
    /// [`RegexError::Brics`] for a parse failure.
    pub fn from_encoded_regex(
        regular_expression: &[u16],
        alphabet: Vec<Vec<i32>>,
        msd: Vec<Option<bool>>,
    ) -> Result<AutomatonDFA, RegexError> {
        assert_eq!(
            alphabet.len(),
            msd.len(),
            "from_encoded_regex: one number system per track required"
        );
        let alphabet_size = alphabet
            .iter()
            .try_fold(1usize, |acc, track| acc.checked_mul(track.len()))
            .expect("alphabet size overflow (Math.multiplyExact equivalent)");

        let fa = set_from_brics_automaton(alphabet_size, regular_expression)?;
        let automaton = Automaton::new(fa, alphabet, Vec::new(), msd);
        Ok(AutomatonDFA::from(automaton))
    }
}

// The test suite lives in `regex/tests.rs` rather than inline, unlike this crate's other
// modules: the implementation above is already the longest single unit in `wr-core` and
// the suite is comparable in size again. Same `mod tests` privileges either way.
#[cfg(test)]
mod tests;
