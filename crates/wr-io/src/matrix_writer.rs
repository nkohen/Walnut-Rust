// SPDX-License-Identifier: GPL-3.0-or-later
// Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).

//! CAS incidence-matrix export — a mechanical port of `Automata/Writer/AutomatonMatrixWriter.java`
//! (188 LOC) plus its four emitter files (`MapleEmitter.java`, `MatlabEmitter.java`,
//! `MathematicaEmitter.java`, `SageEmitter.java`), all under `Automata/Writer/`.
//!
//! `eval`/`def` with an explicit free-variable list on the command line (`eval r x y
//! "…";`, NOT auto-detected from the predicate — see `wr_cli::eval_def`'s own module
//! docs) walks the result automaton and streams, per free variable, one incidence
//! matrix per value in that variable's domain, to four CAS syntaxes at once: Maple,
//! MATLAB/Octave, Mathematica/Wolfram Language, and SageMath. `wr_cli::eval_def::eval_def_command`
//! is the sole real caller (`EvalDef.writeMatrices`, `EvalDef.java:160-172`).
//!
//! # Text-compared, not equivalence-compared
//!
//! Unlike every other automaton comparison in this project (`CLAUDE.md`'s prime
//! directive: compare by semantic language-equivalence, never structural identity),
//! matrix output is raw CAS source text with no automaton-equivalence oracle to fall
//! back on — the golden corpus compares it byte-for-byte (modulo the same
//! whitespace/timing normalization every other text field gets,
//! `tests/golden/tests/support::compare_messages`). Every line, comma, and space below
//! is load-bearing.
//!
//! # `Q == 0` is unreachable through this path
//!
//! [`write_matrix`]'s first check rejects a TRUE/FALSE automaton
//! (`AutomatonMatrixWriter.java:32-34`) before anything else runs. `wr_core::fa::Fa::canonicalize`'s
//! own module docs (`crates/wr-core/src/fa.rs:530-532`) establish that **the only `Q ==
//! 0` automata Walnut ever builds are the TRUE/FALSE ones** — so by the time this
//! function's `Q`/`q0` are read, `Q > 0` is guaranteed, not merely assumed. This was
//! flagged by the porting plan as "plausible but unverified"; it is verified here by
//! that citation, not by a fresh guess. (For what it is worth: were `Q == 0` ever
//! reached, [`SageEmitter::begin_matrix`]'s `firstRowOpen = true` would leave an
//! unbalanced `[[` in the output when `end_matrix` immediately follows with zero
//! `emit_row` calls — a latent shared quirk with real Java, not a Rust-specific bug,
//! since Java's `SageEmitter` has the identical unconditional `out.print("[")`.)
//!
//! # Two "unreachable, ported anyway" validation checks
//!
//! [`MatrixWriteError::DuplicateLabelVariable`] (`:44-45`) and
//! [`MatrixWriteError::EmptyValueDomain`] (`:68-69`) guard invariants the rest of this
//! crate already maintains by construction (a track label can't repeat; a track's
//! alphabet can't be empty) — JaCoCo shows both branches uncovered in the real Java
//! test suite for the same reason. Ported verbatim per `CLAUDE.md`'s mechanical-port
//! rule rather than silently dropped as dead code, since "provably unreachable today"
//! is not the same claim as "will always be unreachable."
//!
//! # The `writeAll` catch-and-continue quirk (not a bug, faithfully preserved)
//!
//! `AutomatonMatrixWriter.writeAll` (`:177-187`) opens each emitter's output file
//! *before* calling `writeMatrix` (which performs all validation), and its `catch`
//! clause only catches `IOException` — a validation `WalnutException` is NOT an
//! `IOException`, so it propagates straight out of `writeAll`, uncaught, after the
//! *first* emitter's file (Maple, per [`EMITTERS`]'s order) has already been created
//! (and left at zero bytes, since `writeMatrix`'s validation runs before any emitter
//! method is called). The remaining three emitters are never attempted at all. This
//! port's [`write_all`] reproduces that shape exactly: file creation, then
//! [`write_matrix`] (which validates internally on every call, once per emitter — also
//! ported verbatim, even though the four calls are behaviorally redundant given the
//! same automaton/free-variable-list input each time), with a validation error
//! propagated immediately rather than caught. `wr_cli::eval_def` deliberately does
//! **not** pre-validate before calling [`write_all`] for this reason — doing so would
//! quietly suppress the zero-byte-Maple-file quirk. A real file-open `io::Error` (the
//! actual `IOException` case) is instead swallowed and the loop continues to the next
//! emitter, matching `Logging.printTruncatedStackTrace(e)` + no rethrow — no `Logging`
//! is plumbed into this free function for that (a real file-open failure here has never
//! been reachable in any test in either engine; this crate's other writer entry points,
//! e.g. `crate::writer::write_automaton_txt`, are plain `std::io` for the same reason).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};

use wr_core::automaton::Automaton;

// ---------------------------------------------------------------------------
// Errors (`AutomatonMatrixWriter.writeMatrix`'s five `WalnutException`s, `:33,45,54,57,69`)
// ---------------------------------------------------------------------------

/// Every `WalnutException` [`write_matrix`] can raise. Display text is verbatim Java.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixWriteError {
    /// `:32-34`: a non-empty free-variable list against a trivial TRUE/FALSE automaton.
    NoFreeVariableOnTrivialAutomaton,
    /// `:44-45`: two entries of `automaton.label` are identical. Confirmed unreachable
    /// in practice (this crate's own invariants keep track labels unique) — ported
    /// anyway, see this module's docs.
    DuplicateLabelVariable { name: String },
    /// `:52-55`: a name in the caller's free-variable list is not one of
    /// `automaton.label`'s entries.
    NotAFreeVariable { name: String },
    /// `:56-58`: a name repeated in the caller's free-variable list.
    DuplicateFreeVariable { name: String },
    /// `:68-69`: a free variable's track has an empty domain. Confirmed unreachable in
    /// practice (this crate never constructs a track with an empty alphabet) — ported
    /// anyway, see this module's docs.
    EmptyValueDomain { name: String },
}

impl std::fmt::Display for MatrixWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Verbatim `AutomatonMatrixWriter.java:33`.
            MatrixWriteError::NoFreeVariableOnTrivialAutomaton => write!(
                f,
                "incidence matrices cannot be calculated, because the automaton does not \
                 have a free variable."
            ),
            // Verbatim `AutomatonMatrixWriter.java:45`.
            MatrixWriteError::DuplicateLabelVariable { name } => {
                write!(f, "Duplicate variable in automaton label: {name}")
            }
            // Verbatim `AutomatonMatrixWriter.java:53-54`.
            MatrixWriteError::NotAFreeVariable { name } => write!(
                f,
                "incidence matrices for the variable {name} cannot be calculated, because \
                 {name} is not a free variable."
            ),
            // Verbatim `AutomatonMatrixWriter.java:57`.
            MatrixWriteError::DuplicateFreeVariable { name } => {
                write!(f, "Duplicate free variable: {name}")
            }
            // Verbatim `AutomatonMatrixWriter.java:69`.
            MatrixWriteError::EmptyValueDomain { name } => {
                write!(f, "Empty value domain for free variable: {name}")
            }
        }
    }
}

impl std::error::Error for MatrixWriteError {}

// ---------------------------------------------------------------------------
// `MatrixEmitter` (`Automata/Writer/MatrixEmitter.java`)
// ---------------------------------------------------------------------------

/// `Automata.Writer.MatrixEmitter` (`:6-26`). Every method is `void` in Java because
/// `PrintWriter` (what every real implementation wraps) never surfaces an `IOException`
/// from `print`/`println` — it swallows write failures internally and only exposes them
/// via `checkError()`, which nothing here calls. This port mirrors that by discarding
/// each write's `io::Result` rather than threading one through every method — the one
/// real failure mode (the underlying file couldn't be opened at all) is handled once,
/// by [`write_all`], before any emitter method runs.
pub trait MatrixEmitter {
    fn begin(&mut self);
    fn emit_initial_row_vector(&mut self, name: &str, q: usize, q0: usize);
    /// Called before the `Q` rows are streamed via [`Self::emit_row`].
    fn begin_matrix(&mut self, name: &str, q: usize);
    /// Called exactly `Q` times per matrix.
    fn emit_row(&mut self, row: &[i32]);
    fn end_matrix(&mut self);
    fn emit_final_column_vector(&mut self, name: &str, is_accepting: &[bool]);
    fn emit_fixup(&mut self, v_name: &str, m_name: &str);
    /// `default void close() {}` (`:25`) — every real emitter overrides this to flush;
    /// ported as a real default (not dropped) so a future emitter that forgets to
    /// override it fails the same (silent, no-op) way Java's would.
    fn close(&mut self) {}
}

// Shared comment blocks (`AutomatonMatrixWriter.java:156-172`), one per vector kind,
// parameterized on the per-emitter comment character.

fn write_initial_row_vector_comment<W: Write>(out: &mut W, v_name: &str, comment_char: &str) {
    let _ = writeln!(
        out,
        "{comment_char} The row vector {v_name} denotes the indicator vector of the (singleton)"
    );
    let _ = writeln!(out, "{comment_char} set of initial states.");
}

fn write_incidence_matrices_comment<W: Write>(out: &mut W, comment_char: &str) {
    let _ = writeln!(
        out,
        "{comment_char} In what follows, the M_i_x, for a free variable i and a value x, denotes"
    );
    let _ = writeln!(
        out,
        "{comment_char} an incidence matrix of the underlying graph of (the automaton of)"
    );
    let _ = writeln!(out, "{comment_char} the predicate in the query.");
    let _ = writeln!(
        out,
        "{comment_char} For every pair of states p and q, the entry M_i_x[p][q] denotes the number of"
    );
    let _ = writeln!(out, "{comment_char} transitions with i=x from p to q.");
}

fn write_final_column_vector_comment<W: Write>(out: &mut W, w_name: &str, comment_char: &str) {
    let _ = writeln!(
        out,
        "{comment_char} The column vector {w_name} denotes the indicator vector of the"
    );
    let _ = writeln!(out, "{comment_char} set of final states.");
}

// ---------------------------------------------------------------------------
// MapleEmitter (`MapleEmitter.java`)
// ---------------------------------------------------------------------------

pub struct MapleEmitter<W: Write> {
    out: W,
    first_row_open: bool,
}

impl<W: Write> MapleEmitter<W> {
    pub fn new(out: W) -> Self {
        MapleEmitter {
            out,
            first_row_open: false,
        }
    }
}

impl<W: Write> MatrixEmitter for MapleEmitter<W> {
    fn begin(&mut self) {
        let _ = writeln!(self.out, "with(ArrayTools):");
    }

    fn emit_initial_row_vector(&mut self, name: &str, q: usize, q0: usize) {
        write_initial_row_vector_comment(&mut self.out, name, "#");
        let _ = write!(self.out, "{name} := Vector[row]([");
        for qi in 0..q {
            let _ = write!(self.out, "{}", if qi == q0 { "1" } else { "0" });
            if qi < q - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = writeln!(self.out, "]);");
        let _ = writeln!(self.out);
        write_incidence_matrices_comment(&mut self.out, "#");
    }

    fn begin_matrix(&mut self, name: &str, _q: usize) {
        let _ = writeln!(self.out);
        let _ = write!(self.out, "{name} := Matrix([");
        self.first_row_open = false; // we open per-row explicitly
    }

    fn emit_row(&mut self, row: &[i32]) {
        if self.first_row_open {
            let _ = writeln!(self.out, ",");
        } else {
            self.first_row_open = true;
        }
        let _ = write!(self.out, "[");
        for (i, v) in row.iter().enumerate() {
            let _ = write!(self.out, "{v}");
            if i < row.len() - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = write!(self.out, "]");
    }

    fn end_matrix(&mut self) {
        let _ = writeln!(self.out, "]);");
    }

    fn emit_final_column_vector(&mut self, name: &str, is_accepting: &[bool]) {
        let _ = writeln!(self.out);
        write_final_column_vector_comment(&mut self.out, name, "#");
        let _ = write!(self.out, "{name} := Vector[column]([");
        for (i, &acc) in is_accepting.iter().enumerate() {
            let _ = write!(self.out, "{}", if acc { "1" } else { "0" });
            if i < is_accepting.len() - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = writeln!(self.out, "]);");
    }

    fn emit_fixup(&mut self, v_name: &str, m_name: &str) {
        let _ = writeln!(self.out);
        // NOTE: the trailing comment hardcodes the literal "v", NOT `v_name`, matching
        // `MapleEmitter.java:97` exactly (`"...; od; #fix up v by multiplying"` — every
        // OTHER emitter's fixup comment interpolates its own `vName`; Maple's alone does
        // not). Unobservable today (the sole real caller always passes `v_name = "v"`),
        // but a faithful mechanical port per `CLAUDE.md` preserves the inconsistency
        // rather than "fixing" it to look uniform with the other three.
        let _ = write!(
            self.out,
            "for i from 1 to Size({v_name})[2] do {v_name} := {v_name}.{m_name}; od; #fix up v by multiplying"
        );
    }

    fn close(&mut self) {
        let _ = self.out.flush();
    }
}

// ---------------------------------------------------------------------------
// MatlabEmitter (`MatlabEmitter.java`)
// ---------------------------------------------------------------------------

pub struct MatlabEmitter<W: Write> {
    out: W,
    first_row: bool,
}

impl<W: Write> MatlabEmitter<W> {
    pub fn new(out: W) -> Self {
        MatlabEmitter {
            out,
            first_row: true,
        }
    }
}

impl<W: Write> MatrixEmitter for MatlabEmitter<W> {
    fn begin(&mut self) {
        let _ = writeln!(self.out, "% MATLAB/Octave output");
    }

    fn emit_initial_row_vector(&mut self, name: &str, q: usize, q0: usize) {
        write_initial_row_vector_comment(&mut self.out, name, "%");
        let _ = write!(self.out, "{name} = [");
        for qi in 0..q {
            let _ = write!(self.out, "{}", if qi == q0 { "1" } else { "0" });
            if qi < q - 1 {
                let _ = write!(self.out, " ");
            }
        }
        let _ = writeln!(self.out, "];");
        let _ = writeln!(self.out);
        write_incidence_matrices_comment(&mut self.out, "%");
    }

    fn begin_matrix(&mut self, name: &str, _q: usize) {
        let _ = writeln!(self.out);
        let _ = write!(self.out, "{name} = [");
        self.first_row = true;
    }

    fn emit_row(&mut self, row: &[i32]) {
        if !self.first_row {
            let _ = write!(self.out, "; ");
        }
        self.first_row = false;
        for (i, v) in row.iter().enumerate() {
            let _ = write!(self.out, "{v}");
            if i < row.len() - 1 {
                let _ = write!(self.out, " ");
            }
        }
    }

    fn end_matrix(&mut self) {
        let _ = writeln!(self.out, "];");
    }

    fn emit_final_column_vector(&mut self, name: &str, is_accepting: &[bool]) {
        let _ = writeln!(self.out);
        write_final_column_vector_comment(&mut self.out, name, "%");
        let _ = write!(self.out, "{name} = [");
        for (i, &acc) in is_accepting.iter().enumerate() {
            let _ = write!(self.out, "{}", if acc { "1" } else { "0" });
            if i < is_accepting.len() - 1 {
                let _ = write!(self.out, "; ");
            }
        }
        let _ = writeln!(self.out, "];");
    }

    fn emit_fixup(&mut self, v_name: &str, m_name: &str) {
        let _ = writeln!(self.out);
        let _ = writeln!(self.out, "% fix up {v_name} by multiplying");
        let _ = writeln!(
            self.out,
            "for i = 1:size({v_name}, 2), {v_name} = {v_name} * {m_name}; end"
        );
    }

    fn close(&mut self) {
        let _ = self.out.flush();
    }
}

// ---------------------------------------------------------------------------
// MathematicaEmitter (`MathematicaEmitter.java`) — WB-042, see `docs/WALNUT-BUGS.md`.
// ---------------------------------------------------------------------------

/// **WB-042** (`docs/WALNUT-BUGS.md`): this emitter's comment lines use `#`, which is a
/// Wolfram Language syntax error (`Slot`) — every generated `.wl` file is unloadable in
/// real Mathematica/WL. Ported verbatim per `CLAUDE.md`'s rule 2: the four golden
/// `.wl` fixtures (374-379, 383) encode the buggy `#`-prefixed output, and "fixing" it
/// to the file's own correct `(* ... *)` style (used elsewhere in the same file, at
/// `begin`/`emit_fixup`) would itself be a text-fixture regression.
pub struct MathematicaEmitter<W: Write> {
    out: W,
    first_row: bool,
}

impl<W: Write> MathematicaEmitter<W> {
    pub fn new(out: W) -> Self {
        MathematicaEmitter {
            out,
            first_row: true,
        }
    }
}

impl<W: Write> MatrixEmitter for MathematicaEmitter<W> {
    fn begin(&mut self) {
        let _ = writeln!(self.out, "(* Wolfram Language / Mathematica output *)");
    }

    fn emit_initial_row_vector(&mut self, name: &str, q: usize, q0: usize) {
        // WB-042: `#`, not `(* *)` -- see this struct's doc comment.
        write_initial_row_vector_comment(&mut self.out, name, "#");
        let _ = write!(self.out, "{name} = {{{{");
        for qi in 0..q {
            let _ = write!(self.out, "{}", if qi == q0 { "1" } else { "0" });
            if qi < q - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = writeln!(self.out, "}}}};");
        let _ = writeln!(self.out);
        write_incidence_matrices_comment(&mut self.out, "#");
    }

    fn begin_matrix(&mut self, name: &str, _q: usize) {
        let _ = writeln!(self.out);
        let _ = write!(self.out, "{name} = {{");
        self.first_row = true;
    }

    fn emit_row(&mut self, row: &[i32]) {
        if !self.first_row {
            let _ = write!(self.out, ",");
        }
        self.first_row = false;

        let _ = write!(self.out, "{{");
        for (i, v) in row.iter().enumerate() {
            let _ = write!(self.out, "{v}");
            if i < row.len() - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = write!(self.out, "}}");
    }

    fn end_matrix(&mut self) {
        let _ = writeln!(self.out, "}};");
    }

    fn emit_final_column_vector(&mut self, name: &str, is_accepting: &[bool]) {
        let _ = writeln!(self.out);
        write_final_column_vector_comment(&mut self.out, name, "#");
        let _ = write!(self.out, "{name} = {{");
        for (i, &acc) in is_accepting.iter().enumerate() {
            let _ = write!(self.out, "{{{}}}", if acc { "1" } else { "0" });
            if i < is_accepting.len() - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = writeln!(self.out, "}};");
    }

    fn emit_fixup(&mut self, v_name: &str, m_name: &str) {
        let _ = writeln!(self.out);
        let _ = writeln!(self.out, "(* fix up {v_name} by multiplying *)");
        let _ = writeln!(
            self.out,
            "Do[{v_name} = {v_name} . {m_name}, {{i, 1, Dimensions[{v_name}][[2]]}}];"
        );
    }

    fn close(&mut self) {
        let _ = self.out.flush();
    }
}

// ---------------------------------------------------------------------------
// SageEmitter (`SageEmitter.java`)
// ---------------------------------------------------------------------------

pub struct SageEmitter<W: Write> {
    out: W,
    first_row_open: bool,
}

impl<W: Write> SageEmitter<W> {
    pub fn new(out: W) -> Self {
        SageEmitter {
            out,
            first_row_open: false,
        }
    }
}

impl<W: Write> MatrixEmitter for SageEmitter<W> {
    fn begin(&mut self) {
        let _ = writeln!(self.out, "# SageMath output");
    }

    fn emit_initial_row_vector(&mut self, name: &str, q: usize, q0: usize) {
        write_initial_row_vector_comment(&mut self.out, name, "#");
        let _ = write!(self.out, "{name} = matrix(ZZ, 1, {q}, [");
        for qi in 0..q {
            let _ = write!(self.out, "{}", if qi == q0 { "1" } else { "0" });
            if qi < q - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = writeln!(self.out, "])");
        let _ = writeln!(self.out);
        write_incidence_matrices_comment(&mut self.out, "#");
    }

    fn begin_matrix(&mut self, name: &str, q: usize) {
        let _ = writeln!(self.out);
        let _ = write!(self.out, "{name} = matrix(ZZ, {q}, {q}, [");
        let _ = write!(self.out, "["); // open first row
        self.first_row_open = true; // we have one row bracket open
    }

    fn emit_row(&mut self, row: &[i32]) {
        if !self.first_row_open {
            let _ = write!(self.out, ",["); // start new row after a comma
        }
        for (i, v) in row.iter().enumerate() {
            let _ = write!(self.out, "{v}");
            if i < row.len() - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = write!(self.out, "]"); // close this row
        self.first_row_open = false; // subsequent rows will prepend ",["
    }

    fn end_matrix(&mut self) {
        let _ = writeln!(self.out, "])");
    }

    fn emit_final_column_vector(&mut self, name: &str, is_accepting: &[bool]) {
        let _ = writeln!(self.out);
        write_final_column_vector_comment(&mut self.out, name, "#");
        let _ = write!(self.out, "{name} = matrix(ZZ, {}, 1, [", is_accepting.len());
        for (i, &acc) in is_accepting.iter().enumerate() {
            let _ = write!(self.out, "{}", if acc { "1" } else { "0" });
            if i < is_accepting.len() - 1 {
                let _ = write!(self.out, ",");
            }
        }
        let _ = writeln!(self.out, "])");
    }

    fn emit_fixup(&mut self, v_name: &str, m_name: &str) {
        let _ = writeln!(self.out);
        let _ = writeln!(self.out, "# fix up {v_name} by multiplying");
        let _ = writeln!(self.out, "for _ in range({v_name}.ncols()):");
        let _ = writeln!(self.out, "    {v_name} = {v_name} * {m_name}");
    }

    fn close(&mut self) {
        let _ = self.out.flush();
    }
}

// ---------------------------------------------------------------------------
// `write_matrix` (`AutomatonMatrixWriter.writeMatrix`, `:28-134`)
// ---------------------------------------------------------------------------

/// `AutomatonMatrixWriter.writeMatrix(Automaton, List<String>, MatrixEmitter)`
/// (`:28-134`). Walks `automaton` and streams `v`, all `M_<vars>_<values>`, and `w` to
/// `emitter` — see this module's docs for the full validation order and the `Q == 0`
/// unreachability argument. Assumes `free_variables` is non-empty (the sole caller,
/// [`write_all`], is only ever reached from `wr_cli::eval_def` when that already holds
/// — matching Java's own doc comment on `writeMatrix`, "It is assumed that
/// freeVariables is non-null and non-empty").
pub fn write_matrix(
    automaton: &mut Automaton,
    free_variables: &[String],
    emitter: &mut dyn MatrixEmitter,
) -> Result<(), MatrixWriteError> {
    // `:32-34`.
    if automaton.fa.is_true_false_automaton() {
        return Err(MatrixWriteError::NoFreeVariableOnTrivialAutomaton);
    }

    // `:37` — port verbatim even though very likely a no-op in practice (see this
    // module's docs and the plan this ports from).
    automaton.canonize();

    // `:40-47`: variable -> index, from the automaton's own label.
    let mut var_index: HashMap<&str, usize> = HashMap::with_capacity(automaton.label.len());
    for (i, v) in automaton.label.iter().enumerate() {
        if var_index.insert(v.as_str(), i).is_some() {
            return Err(MatrixWriteError::DuplicateLabelVariable { name: v.clone() });
        }
    }

    // `:50-59`: validate the caller's free variables exist and are unique.
    let mut seen: HashSet<&str> = HashSet::with_capacity(free_variables.len());
    let mut indices: Vec<usize> = Vec::with_capacity(free_variables.len());
    for v in free_variables {
        let Some(&idx) = var_index.get(v.as_str()) else {
            return Err(MatrixWriteError::NotAFreeVariable { name: v.clone() });
        };
        if !seen.insert(v.as_str()) {
            return Err(MatrixWriteError::DuplicateFreeVariable { name: v.clone() });
        }
        indices.push(idx);
    }

    // `:62-71`: per-free-variable domains, from the automaton's own alphabet.
    let mut domains: Vec<Vec<i32>> = Vec::with_capacity(indices.len());
    for (i, &idx) in indices.iter().enumerate() {
        let dom = &automaton.alphabet[idx];
        if dom.is_empty() {
            return Err(MatrixWriteError::EmptyValueDomain {
                name: free_variables[i].clone(),
            });
        }
        domains.push(dom.clone());
    }

    // `:73-79`: representative for the fix-up line -- prefer 0, else the domain's first
    // value.
    let rep: Vec<i32> = domains
        .iter()
        .map(|dom| if dom.contains(&0) { 0 } else { dom[0] })
        .collect();

    let q = automaton.fa.q;
    let q0 = automaton.fa.q0;

    emitter.begin();
    emitter.emit_initial_row_vector("v", q, q0);

    let free_vars_joined = free_variables.join("_");
    // `as i32` alone would silently truncate a huge `alphabet_size` (a `usize`) instead of
    // erroring — e.g. a value one above `i32::MAX` truncates to a NEGATIVE `i32`, making the
    // `0..alphabet_size` loop below iterate zero times and silently emit all-zero matrices,
    // where Java's `int automaton.getAlphabetSize()` (backed by `Math.multiplyExact`, which
    // already hard-errors at construction well before this point) would never see a value
    // this large in the first place. Same overflow class as WB-038 and the header-parser
    // fix `crates/wr-io/src/reader.rs` made in U30 — panic loudly rather than truncate
    // silently, matching `Automaton::determine_alphabet_size`'s own established idiom
    // (`crates/wr-core/src/automaton.rs`) for this exact conversion.
    let alphabet_size = i32::try_from(automaton.fa.alphabet_size)
        .expect("alphabet size overflow (Math.multiplyExact equivalent)");

    // `:87-123`.
    for assignment in cartesian(&domains) {
        let assignment_joined = assignment
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("_");
        let m_name = format!("M_{free_vars_joined}_{assignment_joined}");
        emitter.begin_matrix(&m_name, q);

        // `:93-105`: which encoded symbols match this assignment on the free-variable
        // tracks.
        let mut encoded_values: HashSet<i32> = HashSet::new();
        for x in 0..alphabet_size {
            let decoding = automaton.decode(x);
            let matches = indices
                .iter()
                .zip(assignment.iter())
                .all(|(&i, &want)| decoding[i] == want);
            if matches {
                encoded_values.insert(x);
            }
        }

        // `:107-120`.
        let mut row = vec![0i32; q];
        for p in 0..q {
            row.iter_mut().for_each(|r| *r = 0);
            for (&sym, targets) in automaton.fa.d[p].iter() {
                if !encoded_values.contains(&sym) {
                    continue;
                }
                for &t in targets {
                    row[t] += 1;
                }
            }
            emitter.emit_row(&row);
        }

        emitter.end_matrix();
    }

    // `:125-128`.
    let is_acc: Vec<bool> = (0..q).map(|s| automaton.fa.is_accepting(s)).collect();
    emitter.emit_final_column_vector("w", &is_acc);

    // `:130-133`.
    let rep_joined = rep
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("_");
    let rep_name = format!("M_{free_vars_joined}_{rep_joined}");
    emitter.emit_fixup("v", &rep_name);

    Ok(())
}

/// `AutomatonMatrixWriter`'s private recursive `cartesian` helper (`:136-154`), ported
/// with the SAME iteration order: `domains[0]` varies slowest (outermost), the last
/// domain varies fastest (innermost) -- this determines the ORDER matrices are emitted
/// in, which the golden corpus compares byte-for-byte (see fixture 374's
/// `M_i_j_0_0`/`M_i_j_0_1`/`M_i_j_1_0`/`M_i_j_1_1` ordering).
fn cartesian(domains: &[Vec<i32>]) -> Vec<Vec<i32>> {
    let Some((first, rest)) = domains.split_first() else {
        return vec![Vec::new()];
    };
    let rest_product = cartesian(rest);
    let mut result = Vec::with_capacity(first.len() * rest_product.len());
    for &x in first {
        for r in &rest_product {
            let mut new_list = Vec::with_capacity(1 + r.len());
            new_list.push(x);
            new_list.extend_from_slice(r);
            result.push(new_list);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// `writeAll` (`AutomatonMatrixWriter.writeAll`, `:177-187`)
// ---------------------------------------------------------------------------

/// `AutomatonMatrixWriter.EMITTERS` (`:16-18`) — order is load-bearing (see
/// `EvalDef.writeMatrices`'s `"Matrix files:"` print, `EvalDef.java:165-169`, and
/// `tests/golden/tests/support::MATRIX_EXTENSIONS`, which must agree with it): Maple,
/// MATLAB/Octave, Mathematica, Sage.
pub struct EmitterSpec {
    pub intro: &'static str,
    pub extension: &'static str,
    ctor: fn(BufWriter<File>) -> Box<dyn MatrixEmitter>,
}

fn maple_ctor(w: BufWriter<File>) -> Box<dyn MatrixEmitter> {
    Box::new(MapleEmitter::new(w))
}
fn matlab_ctor(w: BufWriter<File>) -> Box<dyn MatrixEmitter> {
    Box::new(MatlabEmitter::new(w))
}
fn mathematica_ctor(w: BufWriter<File>) -> Box<dyn MatrixEmitter> {
    Box::new(MathematicaEmitter::new(w))
}
fn sage_ctor(w: BufWriter<File>) -> Box<dyn MatrixEmitter> {
    Box::new(SageEmitter::new(w))
}

pub const EMITTERS: [EmitterSpec; 4] = [
    EmitterSpec {
        intro: "Maple",
        extension: ".mpl",
        ctor: maple_ctor,
    },
    EmitterSpec {
        intro: "MATLAB/Octave",
        extension: ".m",
        ctor: matlab_ctor,
    },
    EmitterSpec {
        intro: "Mathematica",
        extension: ".wl",
        ctor: mathematica_ctor,
    },
    EmitterSpec {
        intro: "Sage",
        extension: ".sage",
        ctor: sage_ctor,
    },
];

/// `AutomatonMatrixWriter.writeAll(Automaton, String, List<String>)` (`:177-187`) —
/// writes `automaton` in all four matrix formats to `<address><extension>`. See this
/// module's docs for the exact catch-and-continue-vs-propagate shape this preserves
/// (a validation error aborts the whole loop after the first emitter's file was
/// already created at zero bytes; a real file-open I/O error is swallowed per-emitter
/// and the loop continues).
pub fn write_all(
    automaton: &mut Automaton,
    address: &str,
    free_variables: &[String],
) -> Result<(), MatrixWriteError> {
    for spec in EMITTERS.iter() {
        let filename = format!("{address}{}", spec.extension);
        let file = match File::create(&filename) {
            Ok(f) => f,
            // `catch (IOException e) { Logging.printTruncatedStackTrace(e); }` -- logged
            // and swallowed in Java, not rethrown; see this module's docs.
            Err(_) => continue,
        };
        let mut emitter = (spec.ctor)(BufWriter::new(file));
        let result = write_matrix(automaton, free_variables, emitter.as_mut());
        // `MatrixEmitter`'s `close()` always runs on try-with-resources exit in Java,
        // success or exception -- flush unconditionally before propagating.
        emitter.close();
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::read_automaton_txt;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// `trim_end`-ed to match `tests/golden`'s own established leniency for this exact
    /// field (`golden_corpus.rs`'s `compare_test_case` trims both sides before comparing
    /// matrix output) — the checked-in fixture files carry a trailing newline
    /// (a repo/editor convention) that `MapleEmitter`'s final `emitFixup` line (a bare
    /// `print`, not `println`) never itself produces.
    fn read_fixture_text(name: &str) -> String {
        std::fs::read_to_string(fixture(name))
            .unwrap()
            .trim_end()
            .to_string()
    }

    fn free_vars(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // ------------------------------------------------------------------
    // Exact-text pins against real captured `walnut-java` output
    // (`automaton374.{mpl,m,wl,sage}` -- see `tests/fixtures/ATTRIBUTION.md`).
    // ------------------------------------------------------------------

    /// `automaton374.txt` carries no variable names (the `.txt` grammar has none --
    /// see `wr_io::reader`'s module docs, "Track labels default to their track index").
    /// Real Walnut's own `eval r i j "…";` labels the result automaton from the
    /// predicate's own free-variable tokens as it evaluates, well before
    /// `writeMatrices` ever runs -- relabel here to reproduce that starting point.
    fn automaton374() -> Automaton {
        let mut a = read_automaton_txt(fixture("automaton374.txt")).unwrap();
        a.label = vec!["i".to_string(), "j".to_string()];
        a
    }

    #[test]
    fn maple_matches_real_walnut_output_for_fixture_374() {
        let mut a = automaton374();
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = MapleEmitter::new(&mut buf);
        write_matrix(&mut a, &free_vars(&["i", "j"]), &mut emitter).unwrap();
        emitter.close();
        assert_eq!(
            String::from_utf8(buf).unwrap().trim_end(),
            read_fixture_text("automaton374.mpl")
        );
    }

    #[test]
    fn matlab_matches_real_walnut_output_for_fixture_374() {
        let mut a = automaton374();
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = MatlabEmitter::new(&mut buf);
        write_matrix(&mut a, &free_vars(&["i", "j"]), &mut emitter).unwrap();
        emitter.close();
        assert_eq!(
            String::from_utf8(buf).unwrap().trim_end(),
            read_fixture_text("automaton374.m")
        );
    }

    #[test]
    fn mathematica_matches_real_walnut_output_for_fixture_374() {
        let mut a = automaton374();
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = MathematicaEmitter::new(&mut buf);
        write_matrix(&mut a, &free_vars(&["i", "j"]), &mut emitter).unwrap();
        emitter.close();
        assert_eq!(
            String::from_utf8(buf).unwrap().trim_end(),
            read_fixture_text("automaton374.wl")
        );
    }

    #[test]
    fn sage_matches_real_walnut_output_for_fixture_374() {
        let mut a = automaton374();
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = SageEmitter::new(&mut buf);
        write_matrix(&mut a, &free_vars(&["i", "j"]), &mut emitter).unwrap();
        emitter.close();
        assert_eq!(
            String::from_utf8(buf).unwrap().trim_end(),
            read_fixture_text("automaton374.sage")
        );
    }

    /// The one shape no existing golden fixture reaches (verified: none of
    /// 374-379/383's real `.mpl` fix-up lines reference anything but a `_0`-suffixed
    /// representative, meaning every free variable's domain in the real corpus contains
    /// 0) -- a free variable whose domain does NOT contain 0, so the fix-up
    /// representative falls back to the domain's first value (`AutomatonMatrixWriter.java:76-78`).
    /// Hand-built: a single track with an explicit, non-zero-containing alphabet
    /// `{1, 2, 3}` (Walnut supports this via an explicit `{...}` set header, not just
    /// numeration bases).
    #[test]
    fn fixup_uses_the_first_domain_value_when_the_domain_excludes_zero() {
        // Hand-built `Fa`: 2 states, symbol 0/1/2 encode digit values 1/2/3
        // respectively (this track's alphabet is `[1, 2, 3]`, not `[0, 1, 2]`). State 0
        // (q0, non-accepting) transitions to state 1 (accepting, no outgoing
        // transitions) on every symbol.
        let fa = wr_core::fa::Fa {
            q0: 0,
            q: 2,
            alphabet_size: 3,
            o: vec![0, 1],
            d: vec![
                [(0i32, vec![1usize]), (1, vec![1]), (2, vec![1])]
                    .into_iter()
                    .collect(),
                std::collections::BTreeMap::new(),
            ],
            true_false: None,
        };
        let mut a = Automaton::new(fa, vec![vec![1, 2, 3]], vec!["x".to_string()], vec![None]);
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = MapleEmitter::new(&mut buf);
        write_matrix(&mut a, &free_vars(&["x"]), &mut emitter).unwrap();
        emitter.close();
        let text = String::from_utf8(buf).unwrap();
        assert!(
            text.contains("od; #fix up v by multiplying") && text.contains(".M_x_1;"),
            "fixup should reference M_x_1 (domain's first value, since 0 is not in the \
             domain), got:\n{text}"
        );
        assert!(
            !text.contains("M_x_0"),
            "M_x_0 should never appear -- 0 is not in this track's domain"
        );
    }

    // ------------------------------------------------------------------
    // Validation errors
    // ------------------------------------------------------------------

    #[test]
    fn trivial_automaton_with_free_variables_is_an_error() {
        let mut a = Automaton::true_false(true);
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = MapleEmitter::new(&mut buf);
        let err = write_matrix(&mut a, &free_vars(&["x"]), &mut emitter).unwrap_err();
        assert_eq!(err, MatrixWriteError::NoFreeVariableOnTrivialAutomaton);
    }

    #[test]
    fn unknown_free_variable_is_an_error() {
        let mut a = automaton374();
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = MapleEmitter::new(&mut buf);
        let err = write_matrix(&mut a, &free_vars(&["z"]), &mut emitter).unwrap_err();
        assert_eq!(
            err,
            MatrixWriteError::NotAFreeVariable {
                name: "z".to_string()
            }
        );
    }

    #[test]
    fn duplicate_free_variable_is_an_error() {
        let mut a = automaton374();
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = MapleEmitter::new(&mut buf);
        let err = write_matrix(&mut a, &free_vars(&["i", "i"]), &mut emitter).unwrap_err();
        assert_eq!(
            err,
            MatrixWriteError::DuplicateFreeVariable {
                name: "i".to_string()
            }
        );
    }

    // ------------------------------------------------------------------
    // `write_all` -- real files, all four extensions, in `EMITTERS` order.
    // ------------------------------------------------------------------

    #[test]
    fn write_all_produces_all_four_files_matching_real_walnut_output() {
        let mut a = automaton374();
        let dir = std::env::temp_dir().join(format!(
            "wr-io-matrix-writer-test-write-all-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let address = dir.join("automaton374").to_str().unwrap().to_string();

        write_all(&mut a, &address, &free_vars(&["i", "j"])).unwrap();

        for (ext, fixture_name) in [
            (".mpl", "automaton374.mpl"),
            (".m", "automaton374.m"),
            (".wl", "automaton374.wl"),
            (".sage", "automaton374.sage"),
        ] {
            let produced = std::fs::read_to_string(format!("{address}{ext}")).unwrap();
            assert_eq!(
                produced.trim_end(),
                read_fixture_text(fixture_name),
                "extension {ext}"
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `write_all`'s validation-error path: the first emitter's file (Maple) is still
    /// created (at zero bytes), and the remaining three are never attempted at all --
    /// see this module's docs on why this quirk is preserved rather than cleaned up.
    #[test]
    fn write_all_validation_error_leaves_a_zero_byte_first_file_and_no_others() {
        let mut a = Automaton::true_false(true);
        let dir = std::env::temp_dir().join(format!(
            "wr-io-matrix-writer-test-write-all-err-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let address = dir.join("trivial").to_str().unwrap().to_string();

        let err = write_all(&mut a, &address, &free_vars(&["x"])).unwrap_err();
        assert_eq!(err, MatrixWriteError::NoFreeVariableOnTrivialAutomaton);

        // Maple's file (first in `EMITTERS`) was created, at zero bytes.
        let maple_meta = std::fs::metadata(format!("{address}.mpl")).unwrap();
        assert_eq!(maple_meta.len(), 0);

        // No other emitter's file was ever created.
        for ext in [".m", ".wl", ".sage"] {
            assert!(
                !std::path::Path::new(&format!("{address}{ext}")).exists(),
                "extension {ext} should never have been attempted"
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }
}
